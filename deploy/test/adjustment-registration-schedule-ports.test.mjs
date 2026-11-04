import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import test from "node:test";
import {
  buildAdjustmentRegistrationScheduleInitializationSql,
  projectAdjustmentRegistrationScheduleStatusV3,
  validateAdjustmentRegistrationScheduleBootstrapV3,
} from "../scripts/adjustment-evaluation-package.mjs";
import {
  ADJUSTMENT_ROLLING_SCHEDULE_SHA256,
  buildAdjustmentRollingScheduleBootstrap,
} from "../../scripts/research/adjustment_rolling_schedule.mjs";
import { adjustmentSha256, canonicalJsonBytes } from "../../scripts/research/adjustment_plaintext_archive.mjs";

const root = resolve(import.meta.dirname, "../..");
const migrationRoot = join(root, "packages/database/migrations");
const names = readdirSync(migrationRoot).filter(
  // retain the complete ordered database ledger
  (name) => name.endsWith(".sql"),
).sort();
const checksums = names.map(
  // bind every migration byte rather than a schema version label
  (name) => adjustmentSha256(readFileSync(join(migrationRoot, name))),
);
const history = adjustmentSha256(Buffer.from(names.map(
  // serialize the actual migration ledger preimage
  (name, index) => `${name}:${checksums[index]}`,
).join("\n")));
const retainedLedgerEnd = names.indexOf("0021_adjustment_rolling_registration.sql");
assert(retainedLedgerEnd >= 0);
// retain the epoch witness ledger without changing the live database manifest
const retainedHistory = adjustmentSha256(Buffer.from(names.slice(0, retainedLedgerEnd + 1).map(
  // serialize only the retained 0021 migration ledger preimage
  (name, index) => `${name}:${checksums[index]}`,
).join("\n")));

// create a complete typed epoch fixture without claiming production authority
function witnessAt(epochAt) {
  const unsigned = {
    activationKind: "inert_v14_pre_activation",
    archiveCommitOrdinal: "0",
    catalogFrontierSha256: adjustmentSha256(Buffer.from("adjustment-revision-frontier/v1\n0\n")),
    contractVersion: "adjustment-revision-capture-epoch-witness/v1",
    controlPlaneSha256: "a".repeat(64),
    controlPlaneVersion: "14",
    databaseMigrationHistorySha256: retainedHistory,
    epochAt,
    servingSnapshotSha256: "b".repeat(64),
    sourceCommit: "c".repeat(40),
    sourceRelease: "2026.10.09-3",
    sourceServerImageDigest: `sha256:${"d".repeat(64)}`,
    sourceWebImageDigest: `sha256:${"e".repeat(64)}`,
  };
  return { ...unsigned, witnessSha256: adjustmentSha256(canonicalJsonBytes(unsigned)) };
}

// independently resolve every tested daylight-saving and leap-year boundary
test("root schedule bootstrap matches the shared planner byte-for-byte", () => {
  for (const epochAt of [
    "2026-10-09T04:10:45.000Z", "2027-03-14T10:00:00.000Z",
    "2028-02-29T08:00:00.000Z", "2036-11-02T09:30:00.000Z",
  ]) {
    const witness = witnessAt(epochAt);
    const bootstrap = buildAdjustmentRollingScheduleBootstrap({
      epochAt, epochWitnessSha256: witness.witnessSha256,
    });
    assert.equal(validateAdjustmentRegistrationScheduleBootstrapV3(bootstrap, witness), bootstrap);
    const sql = buildAdjustmentRegistrationScheduleInitializationSql({
      bootstrapBytes: canonicalJsonBytes(bootstrap), bootstrapSha256: bootstrap.bootstrapSha256, witness,
    });
    assert.match(sql, /^BEGIN;\nSET LOCAL statement_timeout = '30s';/u);
    assert.ok(sql.includes(`$bootstrap$${canonicalJsonBytes(bootstrap).toString("utf8").trimEnd()}$bootstrap$`));
    assert.match(sql, /SELECT weather_initialize_adjustment_registration_schedule_v3\(/u);
    assert.match(sql, /COMMIT;\n$/u);
    assert.throws(() => buildAdjustmentRegistrationScheduleInitializationSql({
      bootstrapBytes: Buffer.from(JSON.stringify(bootstrap)), bootstrapSha256: bootstrap.bootstrapSha256, witness,
    }));
    assert.throws(() => buildAdjustmentRegistrationScheduleInitializationSql({
      bootstrapBytes: canonicalJsonBytes(bootstrap), bootstrapSha256: "f".repeat(64), witness,
    }));
    assert.throws(() => validateAdjustmentRegistrationScheduleBootstrapV3({
      ...bootstrap, horizonEndAt: "2099-01-01T08:00:00.000Z",
    }, witness));
  }
});

// project only the authenticated finite database horizon and fixed family slots
test("schedule status rejects caller clocks, slot drift and extra authority", () => {
  const witness = witnessAt("2026-10-09T04:10:45.000Z");
  const bootstrap = buildAdjustmentRollingScheduleBootstrap({
    epochAt: witness.epochAt, epochWitnessSha256: witness.witnessSha256,
  });
  const envelope = {
    databaseManifest: {
      contract_version: "adjustment-evaluation-export-manifest/v1",
      migration_checksums: checksums,
      migration_history_sha256: history,
      migration_names: names,
      query_contract_sha256: "c860039c72818a9b813ed9f9d93f5d8e115f4144e99d698734e9159ca3457bd4",
      query_contract_version: "adjustment-evaluation-export-query/v1",
      row_schema_sha256: "c21782034130d003e4af48fada4c8f6545f6f1c4dff550f92d0523c7d96d20e2",
      schema_migration: "0017_adjustment_evaluation_export.sql",
      site_key: "ballydidean", site_timezone: "America/Los_Angeles",
    },
    payload: ["temperature", "wind", "rain"].map(
      // keep the database-defined family order and empty slot shape
      (family) => ({
        contractVersion: "adjustment-shadow-registration-slot/v3",
        epochWitnessSha256: witness.witnessSha256,
        family, horizonEndAt: bootstrap.horizonEndAt,
        registrationSha256: null, scheduleContractSha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256,
        state: "free", terminalAt: null,
      }),
    ),
    transaction: {
      created_at_utc: "2026-10-09T04:11:00.123456Z",
      idle_in_transaction_session_timeout: "30s", isolation_level: "repeatable read",
      lock_timeout: "5s", read_only: "on", statement_timeout: "5min",
    },
  };
  const projected = projectAdjustmentRegistrationScheduleStatusV3(envelope, witness);
  assert.equal(projected.snapshotAt, "2026-10-09T04:11:00.123Z");
  assert.equal(projected.horizonEndAt, bootstrap.horizonEndAt);
  assert.equal(projected.epochAt, witness.epochAt);
  assert.deepEqual(projected.slots, envelope.payload);
  assert.throws(() => projectAdjustmentRegistrationScheduleStatusV3({ ...envelope, extra: true }, witness));
  const drift = structuredClone(envelope);
  drift.payload[1].horizonEndAt = "2099-01-01T08:00:00.000Z";
  assert.throws(() => projectAdjustmentRegistrationScheduleStatusV3(drift, witness));
  drift.payload = structuredClone(envelope.payload);
  drift.transaction.read_only = "off";
  assert.throws(() => projectAdjustmentRegistrationScheduleStatusV3(drift, witness));
});

// build only exact epoch-bound identities for the readonly owner lifecycle reader
function lifecycleEnvelope(witness, snapshotAt) {
  const bootstrap = buildAdjustmentRollingScheduleBootstrap({ epochAt: witness.epochAt,
    epochWitnessSha256: witness.witnessSha256 });
  return {
    databaseManifest: { contract_version: "adjustment-evaluation-export-manifest/v1",
      migration_checksums: checksums, migration_history_sha256: history, migration_names: names,
      query_contract_sha256: "c860039c72818a9b813ed9f9d93f5d8e115f4144e99d698734e9159ca3457bd4",
      query_contract_version: "adjustment-evaluation-export-query/v1",
      row_schema_sha256: "c21782034130d003e4af48fada4c8f6545f6f1c4dff550f92d0523c7d96d20e2",
      schema_migration: "0017_adjustment_evaluation_export.sql", site_key: "ballydidean",
      site_timezone: "America/Los_Angeles" },
    payload: ["temperature", "wind", "rain"].map(
      // distinguish genuine genesis from a missing retired predecessor
      (family) => ({ slot: { contractVersion: "adjustment-shadow-registration-slot/v3",
        epochWitnessSha256: witness.witnessSha256, family, horizonEndAt: bootstrap.horizonEndAt,
        registrationSha256: null, scheduleContractSha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256,
        state: "free", terminalAt: null }, activeRegistration: null, metadata: null,
        latestRegistrationSha256: null, predecessor: null }),
    ),
    transaction: { created_at_utc: snapshotAt, idle_in_transaction_session_timeout: "30s",
      isolation_level: "repeatable read", lock_timeout: "5s", read_only: "on", statement_timeout: "5min" },
  };
}

// the new owner reader retains lineage without widening the training-export database role
test("lifecycle reader closes active v3 identity and refuses lost predecessor history", async () => {
  const { buildAdjustmentRegistrationLifecycleStatusSqlV4,
    projectAdjustmentRegistrationLifecycleStatusV4 } = await import("../scripts/adjustment-evaluation-package.mjs");
  const witness = witnessAt("2026-10-09T04:10:45.000Z");
  const envelope = lifecycleEnvelope(witness, "2026-10-10T08:00:00.000Z");
  assert.equal(projectAdjustmentRegistrationLifecycleStatusV4(envelope, witness).entries.length, 3);
  const identityFields = ["siteKey", "family", "candidateSha256", "artifactSha256", "policySha256",
    "cohortSha256", "reservedKeySha256", "sourceSha256", "epochWitnessSha256", "scheduleContractSha256",
    "predecessorRegistrationSha256", "intervalStartAt", "intervalEndAt", "targetCutoffAt", "terminalAt"];
  const registration = { siteKey: "ballydidean", family: "temperature", candidateSha256: "a".repeat(64),
    artifactSha256: "b".repeat(64), policySha256: "c".repeat(64), cohortSha256: "d".repeat(64),
    reservedKeySha256: "e".repeat(64), sourceSha256: "f".repeat(64), epochWitnessSha256: witness.witnessSha256,
    scheduleContractSha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256, predecessorRegistrationSha256: null,
    intervalStartAt: "2026-11-01T07:00:00.000Z", intervalEndAt: "2027-11-02T07:00:00.000Z",
    targetCutoffAt: "2027-11-09T07:00:00.000Z", terminalAt: "2027-11-09T07:00:00.000Z" };
  registration.registrationSha256 = adjustmentSha256(Buffer.from([
    "adjustment-shadow-registration/v3", ...identityFields.map(
      // retain the exact frozen registration preimage including its nullable predecessor
      (key) => registration[key] ?? "none",
    ),
  ].join("\n") + "\n"));
  const active = envelope.payload[0];
  active.activeRegistration = registration;
  active.latestRegistrationSha256 = registration.registrationSha256;
  active.slot.state = "busy_v3";
  active.slot.registrationSha256 = registration.registrationSha256;
  active.slot.terminalAt = registration.terminalAt;
  active.metadata = { generation: 0,
    rootSha256: "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945",
    finalizedPredictionCount: 0, throughAt: null, lastAnchorSha256: null };
  assert.deepEqual(projectAdjustmentRegistrationLifecycleStatusV4(envelope, witness)
    .entries[0].activeRegistration, registration);
  const drift = structuredClone(envelope);
  drift.payload[0].activeRegistration.sourceSha256 = "1".repeat(64);
  assert.throws(() => projectAdjustmentRegistrationLifecycleStatusV4(drift, witness), /geometry differs/u);
  drift.payload = structuredClone(envelope.payload);
  drift.payload[0].metadata.generation = 1;
  drift.payload[0].metadata.finalizedPredictionCount = 1;
  drift.payload[0].metadata.lastAnchorSha256 = "1".repeat(64);
  assert.throws(() => projectAdjustmentRegistrationLifecycleStatusV4(drift, witness), /metadata identity/u);
  const free = lifecycleEnvelope(witness, "2028-01-01T08:00:00.000Z");
  free.payload[0].latestRegistrationSha256 = registration.registrationSha256;
  assert.throws(() => projectAdjustmentRegistrationLifecycleStatusV4(free, witness), /free predecessor/u);
  free.payload[0].predecessor = { registrationSha256: registration.registrationSha256,
    terminalAt: registration.terminalAt, reconciliationSha256: "1".repeat(64), sourceSha256: registration.sourceSha256,
    reservedKeySha256: registration.reservedKeySha256, epochWitnessSha256: witness.witnessSha256,
    scheduleContractSha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256 };
  assert.equal(projectAdjustmentRegistrationLifecycleStatusV4(free, witness).entries[0]
    .predecessor.terminalAt, registration.terminalAt);
  free.payload[0].predecessor.epochWitnessSha256 = "2".repeat(64);
  assert.throws(() => projectAdjustmentRegistrationLifecycleStatusV4(free, witness), /epoch or clock/u);
  const sql = buildAdjustmentRegistrationLifecycleStatusSqlV4();
  assert.match(sql, /^BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;/u);
  assert.match(sql, /ORDER BY interval_start_at DESC LIMIT 1/u);
  assert.match(sql, /adjustment_shadow_terminal_results_v2/u);
  assert.doesNotMatch(sql, /\b(?:INSERT|UPDATE|DELETE|GRANT|TRUNCATE)\b/u);
});
