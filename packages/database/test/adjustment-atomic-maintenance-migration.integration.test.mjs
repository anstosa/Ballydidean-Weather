import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { cp, mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import { runMigrations } from "../dist/index.js";
import { createTestPool, prepareRuntimeRoles, startPostgres, stopPostgres } from "./postgres-harness.mjs";
import { buildAdjustmentRegistrationLifecycleStatusSqlV4,
  projectAdjustmentRegistrationLifecycleStatusV4, buildAdjustmentShadowMetadataPreparationSql,
  buildAdjustmentShadowMetadataFinalizationSql } from "../../../deploy/scripts/adjustment-evaluation-package.mjs";
import { adjustmentSha256, canonicalJsonBytes } from "../../../scripts/research/adjustment_plaintext_archive.mjs";
import { buildAdjustmentRollingScheduleBootstrap,
  buildAdjustmentRollingWindow, adjustmentRollingLocalDateAt } from "../../../scripts/research/adjustment_rolling_schedule.mjs";

const migrationDirectory = resolve(import.meta.dirname, "../migrations");

// copy only genuine immutable migration artifacts into a disposable test prefix
async function copyPrefix(root, count) {
  const names = (await readdir(migrationDirectory)).filter(
    // match the production migration loader
    (name) => /^\d{4}_[a-z0-9_]+\.sql$/u.test(name),
  ).sort();
  // preserve exact file bytes without transforming SQL
  for (const name of names.slice(0, count)) await cp(join(migrationDirectory, name), join(root, name));
  return names;
}

// prove a real PostgreSQL error in the final file cannot strand a partial bridge
test("atomic maintenance bridge restores 18 on failure and commits all three files together", {
  timeout: 180_000,
}, async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-atomic-maintenance-"));
  const bridgeRoot = await mkdtemp(join(tmpdir(), "weather-atomic-maintenance-bridge-"));
  const server = await startPostgres(17, "atomic-maintenance-migration");
  const admin = createTestPool(server);
  let owner;
  try {
    await prepareRuntimeRoles(admin);
    owner = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    const names = await copyPrefix(root, 18);
    await copyPrefix(bridgeRoot, 21);
    await runMigrations(owner, root);
    let finalFileReached = false;
    // inject an actual database abort only after the preceding frozen files ran
    const faultPool = {
      // preserve the real session lock and transaction boundaries
      async connect() {
        const client = await owner.connect();
        return {
          // delegate all statements except the deliberate final-file database error
          async query(sql, values) {
            // fail inside the same live transaction as both preceding tail files
            if (typeof sql === "string" && sql.includes("CREATE TABLE adjustment_registration_schedule_v3")) {
              finalFileReached = true;
              return await client.query("SELECT 1 / 0");
            }
            return await client.query(sql, values);
          },
          // return the underlying session after rollback and advisory unlock
          release() { client.release(); },
        };
      },
    };
    await assert.rejects(runMigrations(faultPool, bridgeRoot, {
      atomicMaintenanceV14: true,
    }), /division by zero/u);
    assert.equal(finalFileReached, true);
    assert.deepEqual((await owner.query("SELECT name FROM schema_migrations ORDER BY name")).rows,
      names.slice(0, 18).map(
        // retain the exact source history after the final-file error
        (name) => ({ name }),
      ));
    assert.deepEqual((await owner.query(`SELECT
      to_regclass('public.adjustment_maintenance_actions_v2') AS actions,
      to_regclass('public.adjustment_revision_frontier_v1') AS frontier,
      to_regclass('public.adjustment_registration_schedule_v3') AS schedule`)).rows[0], {
      actions: null, frontier: null, schedule: null,
    });
    await assert.rejects(runMigrations(owner, root, { atomicMaintenanceV14: true }), /exact 18-to-21/u);
    const applied = await runMigrations(owner, bridgeRoot, { atomicMaintenanceV14: true });
    assert.deepEqual(applied.applied, names.slice(18, 21));
    assert.equal(applied.current.length, 18);
    const retry = await runMigrations(owner, bridgeRoot, { atomicMaintenanceV14: true });
    assert.deepEqual(retry.applied, []);
    assert.deepEqual(retry.current, names.slice(0, 21));
    // refuse an extended target after the reviewed bridge ledger is complete
    await copyPrefix(root, 21);
    await writeFile(join(root, "0022_unreviewed.sql"), "SELECT 1;\n");
    await assert.rejects(runMigrations(owner, root, { atomicMaintenanceV14: true }), /exact 18-to-21/u);
    const ordinary = await runMigrations(owner, migrationDirectory);
    assert.deepEqual(ordinary.applied, names.slice(21));
    assert.deepEqual(ordinary.current, names.slice(0, 21));
    const witnessUnsigned = { activationKind: "inert_v14_pre_activation", archiveCommitOrdinal: "0",
      catalogFrontierSha256: adjustmentSha256(Buffer.from("adjustment-revision-frontier/v1\n0\n")),
      contractVersion: "adjustment-revision-capture-epoch-witness/v1", controlPlaneSha256: "a".repeat(64),
      controlPlaneVersion: "14", databaseMigrationHistorySha256:
        "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
      epochAt: "2024-01-01T08:00:00.000Z", servingSnapshotSha256: "b".repeat(64),
      sourceCommit: "c".repeat(40), sourceRelease: "2026.10.09-3",
      sourceServerImageDigest: `sha256:${"d".repeat(64)}`, sourceWebImageDigest: `sha256:${"e".repeat(64)}` };
    const witness = { ...witnessUnsigned, witnessSha256: adjustmentSha256(canonicalJsonBytes(witnessUnsigned)) };
    const bootstrap = buildAdjustmentRollingScheduleBootstrap({ epochAt: witness.epochAt,
      epochWitnessSha256: witness.witnessSha256 });
    await owner.query("SELECT weather_initialize_adjustment_registration_schedule_v3($1)", [bootstrap]);
    // exercise the actual closed owner query rather than substituting database metadata
    const readLifecycle = async () => {
      const results = await owner.query(buildAdjustmentRegistrationLifecycleStatusSqlV4());
      const selected = results.find((result) => result.rows[0]?.json_build_object !== undefined);
      return projectAdjustmentRegistrationLifecycleStatusV4(selected.rows[0].json_build_object, witness);
    };
    assert.equal((await readLifecycle()).entries[0].slot.state, "free");
    await admin.query("ALTER ROLE weather_owner IN DATABASE weather_test SET weather.adjustment_maintenance_v2_enabled = 'on'");
    const clock = new Date((await owner.query("SELECT clock_timestamp() AS clock")).rows[0].clock).toISOString();
    const plan = buildAdjustmentRollingWindow({ epochAt: witness.epochAt, epochWitnessSha256: witness.witnessSha256,
      family: "temperature", fitMonth: adjustmentRollingLocalDateAt(clock).slice(0, 7),
      requestedAt: clock, predecessorTerminalAt: null });
    const registration = { artifactSha256: "a".repeat(64), candidateSha256: "b".repeat(64),
      cohortSha256: "c".repeat(64), epochWitnessSha256: witness.witnessSha256, family: "temperature",
      intervalEndAt: plan.intervalEndAt, intervalStartAt: plan.intervalStartAt, policySha256: "d".repeat(64),
      predecessorRegistrationSha256: null, reservedKeySha256: "e".repeat(64),
      scheduleContractSha256: plan.scheduleContractSha256, siteKey: "ballydidean", sourceSha256: "f".repeat(64),
      targetCutoffAt: plan.targetCutoffAt, terminalAt: plan.terminalAt };
    registration.registrationSha256 = createHash("sha256").update([
      "adjustment-shadow-registration/v3", registration.siteKey, registration.family,
      registration.candidateSha256, registration.artifactSha256, registration.policySha256,
      registration.cohortSha256, registration.reservedKeySha256, registration.sourceSha256,
      registration.epochWitnessSha256, registration.scheduleContractSha256, "none",
      registration.intervalStartAt, registration.intervalEndAt, registration.targetCutoffAt, registration.terminalAt,
    ].join("\n") + "\n").digest("hex");
    await owner.query("SELECT weather_register_adjustment_shadow_v3($1)", [registration]);
    const actualActive = (await readLifecycle()).entries[0];
    assert.deepEqual(actualActive.activeRegistration, registration);
    assert.equal(actualActive.metadata.generation, 0);
    // admit one synthetic future halo through the genuine owner append boundary
    const issuedAt = new Date(Date.now() - 1_000).toISOString();
    const dueAt = new Date(issuedAt);
    dueAt.setUTCHours(Math.floor(dueAt.getUTCHours() / 6) * 6, 35, 0, 0);
    if (dueAt.getTime() > Date.parse(issuedAt)) dueAt.setUTCHours(dueAt.getUTCHours() - 6);
    const prediction = { bodyByteCount: 2_048, candidateSha256: registration.candidateSha256,
      dueKey: `capture/${dueAt.toISOString()}`, inputSha256: "1".repeat(64), issuedAt,
      minValidAt: registration.intervalStartAt,
      maxValidAt: new Date(Date.parse(registration.intervalStartAt) + 11 * 3_600_000).toISOString(),
      predictionBodySha256: "2".repeat(64),
      predictionSchemaSha256: "4255feacfd464adf2cbbf1139ecdf30d9d00b847775c556407367ad1449d9e63",
      registrationSha256: registration.registrationSha256, rowCount: 12,
      sourceSha256: registration.sourceSha256, stageReceiptSha256: "3".repeat(64) };
    prediction.sourceReceiptSha256 = adjustmentSha256(Buffer.from([
      "adjustment-shadow-source-receipt/v1", prediction.registrationSha256, prediction.candidateSha256,
      prediction.sourceSha256, prediction.dueKey, prediction.issuedAt, prediction.minValidAt,
      prediction.maxValidAt, String(prediction.rowCount), prediction.inputSha256,
    ].join("\n")));
    prediction.predictionSha256 = adjustmentSha256(Buffer.from([
      "adjustment-shadow-prediction/v3", prediction.registrationSha256, prediction.candidateSha256,
      prediction.sourceSha256, prediction.dueKey, prediction.issuedAt, prediction.minValidAt,
      prediction.maxValidAt, prediction.sourceReceiptSha256, prediction.inputSha256,
      prediction.predictionBodySha256, prediction.predictionSchemaSha256,
      String(prediction.rowCount), String(prediction.bodyByteCount),
    ].join("\n")));
    const appended = (await owner.query("SELECT weather_append_adjustment_temperature_shadow_v2($1)", [prediction]))
      .rows[0].weather_append_adjustment_temperature_shadow_v2;
    const receipt = appended.revisionReceipt;
    const predictionCommittedAt = (await owner.query("SELECT committed_at FROM adjustment_shadow_predictions_v2 WHERE prediction_sha256=$1",
      [prediction.predictionSha256])).rows[0].committed_at.toISOString();
    const unsignedProof = { acknowledgementSha256: "4".repeat(64), archiveCommitOrdinal: receipt.archiveCommitOrdinal,
      authority: "shadow_metadata_custody_only", contractVersion: "adjustment-shadow-metadata-custody-proof/v1",
      custodyCheckpointSha256: "5".repeat(64), frontierSha256: receipt.frontierSha256,
      memberRootSha256: "6".repeat(64), pageSha256: "7".repeat(64), preparedAt: new Date().toISOString(),
      entries: [{ archiveCommitOrdinal: receipt.archiveCommitOrdinal, archiveCommittedAt: receipt.archiveCommittedAt,
        bodyByteCount: prediction.bodyByteCount, capsuleSha256: "8".repeat(64), dueKey: prediction.dueKey,
        frontierSha256: receipt.frontierSha256, inputSha256: prediction.inputSha256, issuedAt: prediction.issuedAt,
        maxValidAt: prediction.maxValidAt, minValidAt: prediction.minValidAt,
        predictionBodySha256: prediction.predictionBodySha256, predictionCommittedAt,
        predictionSchemaSha256: prediction.predictionSchemaSha256, predictionSha256: prediction.predictionSha256,
        predecessorFrontierSha256: receipt.predecessorFrontierSha256, receiptSha256: receipt.receiptSha256,
        registrationSha256: prediction.registrationSha256, rowCount: prediction.rowCount,
        sourceReceiptSha256: prediction.sourceReceiptSha256, stageReceiptSha256: prediction.stageReceiptSha256 }] };
    const proof = { ...unsignedProof, proofSha256: adjustmentSha256(canonicalJsonBytes(unsignedProof)) };
    const client = await owner.connect();
    try {
      const changedProof = { ...unsignedProof, entries: [{ ...unsignedProof.entries[0], inputSha256: "9".repeat(64) }] };
      changedProof.proofSha256 = adjustmentSha256(canonicalJsonBytes(changedProof));
      await assert.rejects(client.query(buildAdjustmentShadowMetadataPreparationSql(changedProof)), /custody row differs/u);
      await client.query("ROLLBACK");
      const prepared = (await client.query(buildAdjustmentShadowMetadataPreparationSql(proof)))
        .find((result) => result.rows[0]?.json_build_object !== undefined).rows[0].json_build_object;
      const unsignedPreparation = { authority: "shadow_metadata_transfer",
        contractVersion: "adjustment-shadow-metadata-custody-preparation/v1", finalizations: prepared.finalizations,
        preparedAt: new Date().toISOString(), proofSha256: proof.proofSha256 };
      const preparation = { ...unsignedPreparation,
        preparationSha256: adjustmentSha256(canonicalJsonBytes(unsignedPreparation)) };
      const finalize = async () => (await client.query(buildAdjustmentShadowMetadataFinalizationSql(preparation)))
        .find((result) => result.rows[0]?.json_build_object !== undefined).rows[0].json_build_object;
      const first = await finalize();
      assert.equal(first.results[0].status, "finalized");
      assert.equal((await finalize()).results[0].status, "already_finalized");
      assert.equal(first.results[0].metadataManifestSha256, preparation.finalizations[0].metadataManifestSha256);
      assert.equal(Number((await client.query("SELECT count(*) FROM adjustment_shadow_predictions_v2")).rows[0].count), 0);
      assert.equal(Number((await client.query("SELECT count(*) FROM adjustment_shadow_registrations_v2")).rows[0].count), 1);
    } finally { client.release(); }
    assert.equal((await readLifecycle()).entries[0].metadata.generation, 1);
  } finally {
    await owner?.end();
    await admin.end();
    await stopPostgres(server);
    await rm(root, { recursive: true, force: true });
    await rm(bridgeRoot, { recursive: true, force: true });
  }
});
