import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { resolve } from "node:path";
import { promisify } from "node:util";
import { gzipSync } from "node:zlib";
import test from "node:test";
import { appendRainAdjustmentRun, readPendingRainAdjustmentCaptures, readRainAdjustmentRun, runMigrations } from "../dist/index.js";
import { createTestPool, prepareRuntimeRoles, startPostgres, stopPostgres } from "./postgres-harness.mjs";

const executeFile = promisify(execFile);
const root = resolve(import.meta.dirname, "../../..");

// exercise real PostgreSQL guards and runtime grants without provider traffic
test("rain serving is an immutable bounded projection with private inputs", { timeout: 180_000 }, async () => {
  const server = await startPostgres(17, "rain-adjustment");
  const admin = createTestPool(server);
  let owner;
  let ingest;
  let api;
  let exporter;
  try {
    await prepareRuntimeRoles(admin);
    owner = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    await runMigrations(owner, resolve(root, "packages/database/migrations"));
    await executeFile("docker", ["cp", resolve(root, "deploy/postgres/runtime-acl-v2.sql"), `${server.name}:/tmp/acl.sql`]);
    await executeFile("docker", ["exec", server.name, "psql", "--username", server.user, "--dbname", "weather_test", "--file", "/tmp/acl.sql"]);
    ingest = createTestPool(server, "weather_test", "weather_ingest", "ingest-test");
    api = createTestPool(server, "weather_test", "weather_api", "api-test");
    exporter = createTestPool(server, "weather_test", "weather_training_export", "training-export-test");
    const now = (await owner.query("SELECT clock_timestamp() AS now")).rows[0].now;
    const initialized = new Date(Math.floor((now.getTime() - 8 * 3_600_000) / (6 * 3_600_000)) * 6 * 3_600_000);
    const received = new Date(initialized.getTime() + 6 * 3_600_000);
    const decision = new Date(initialized.getTime() + 8 * 3_600_000);
    const body = Buffer.from('{"fixture":"private capture"}');
    const compressed = gzipSync(body);
    // seed historical clocks only in this disposable owner-controlled database
    await owner.query("ALTER TABLE rain_capture_claims DISABLE TRIGGER rain_capture_claims_guard_insert; ALTER TABLE rain_capture_receipts DISABLE TRIGGER rain_capture_receipts_guard_insert");
    const claim = (await owner.query(`INSERT INTO rain_capture_claims
      (kind, slot_key, run_initialized_at, attempt, release, policy, policy_sha256, claimed_at)
      VALUES ('forecast', 'fixture-source', $1, 1, '2026.09.14-1', '{}', $2, $3) RETURNING id`,
    [initialized.toISOString(), "0".repeat(64), received.toISOString()])).rows[0].id;
    await owner.query(`INSERT INTO rain_capture_receipts
      (claim_id, started_at, completed_at, http_status, outcome, compressed_body, body_sha256,
       body_bytes, compressed_bytes, parser_version, row_count, available_by_decision, metadata)
      VALUES ($1, $2, $2, 200, 'valid', $3, $4, $5, $6, 'rain-prospective-capture/v1', 49, true, '{}')`,
    [claim, received.toISOString(), compressed, createHash("sha256").update(body).digest("hex"), body.length, compressed.length]);
    await owner.query("ALTER TABLE rain_capture_claims ENABLE TRIGGER rain_capture_claims_guard_insert; ALTER TABLE rain_capture_receipts ENABLE TRIGGER rain_capture_receipts_guard_insert");
    const model = "a".repeat(64);
    const captures = await readPendingRainAdjustmentCaptures(ingest, model);
    assert.equal(captures.length, 1);
    assert.deepEqual(Buffer.from(captures[0].body), body);
    const run = { runInitializedAt: initialized.toISOString(), firstReceivedAt: received.toISOString(),
      decisionAt: decision.toISOString(), generatedAt: now.toISOString(), modelSha256: model,
      inputSha256: "b".repeat(64), forecastClaimId: claim,
      hours: [{ validAt: new Date(initialized.getTime() + 9 * 3_600_000).toISOString(), modelLeadHours: 9,
        rawPrecipitationMm: 0.5, correctedPrecipitationMm: 0.25, applied: true, reasonCode: null }] };
    let archivedRevision;
    const expectedStoredContentSha256 = (await owner.query(`SELECT encode(sha256(
      convert_to($1::jsonb::text, 'UTF8')), 'hex') AS value`, [JSON.stringify(run.hours)]))
      .rows[0].value;
    assert.equal(await appendRainAdjustmentRun(ingest, run,
      // capture only the public immutable revision returned by PostgreSQL
      async (_queryable, revision) => {
        archivedRevision = revision;
        return null;
      }), true);
    assert.equal(archivedRevision.storedContentSha256, expectedStoredContentSha256);
    assert.equal(await appendRainAdjustmentRun(ingest, run), false);
    assert.deepEqual(await readPendingRainAdjustmentCaptures(ingest, model), []);
    const readAt = (await owner.query("SELECT clock_timestamp() AS now")).rows[0].now.toISOString();
    const publicRun = await readRainAdjustmentRun(api, "ballydidean", readAt);
    assert.deepEqual(publicRun.hours, run.hours);
    assert.equal(JSON.stringify(publicRun).includes("private capture"), false);
    assert.equal(await readRainAdjustmentRun(api, "other-site", readAt), null);
    assert.equal(await readRainAdjustmentRun(api, "ballydidean", new Date(decision.getTime() + 12 * 3_600_000).toISOString()), null);
    // keep all raw source evidence inaccessible to readers and exporters
    for (const reader of [api, exporter]) {
      await assert.rejects(reader.query("SELECT compressed_body FROM rain_capture_receipts"), /permission denied/u);
      await assert.rejects(reader.query("SELECT * FROM rain_capture_claims"), /permission denied/u);
      await assert.rejects(appendRainAdjustmentRun(reader, { ...run, modelSha256: "c".repeat(64) }), /permission denied|read-only/u);
    }
    await assert.rejects(exporter.query("SELECT * FROM rain_adjustment_runs"), /permission denied/u);
    await assert.rejects(owner.query("UPDATE rain_adjustment_runs SET hours = hours"), /immutable/u);
    // reject each independent invalid hour even through direct ingestion SQL
    for (const hour of [
      { ...run.hours[0], modelLeadHours: null }, { ...run.hours[0], validAt: null },
      { ...run.hours[0], modelLeadHours: 8 }, { ...run.hours[0], correctedPrecipitationMm: -1 },
      { ...run.hours[0], correctedPrecipitationMm: 31 }, { ...run.hours[0], privateBody: "not allowed" },
    ]) {
      await assert.rejects(appendRainAdjustmentRun(ingest, { ...run, modelSha256: "d".repeat(64), hours: [hour] }), /invalid/u);
    }
    await assert.rejects(appendRainAdjustmentRun(ingest, { ...run, modelSha256: "e".repeat(64),
      firstReceivedAt: decision.toISOString() }), /source is unavailable/u);
    await owner.query("CREATE TABLE archive_probe (identity text); GRANT INSERT ON archive_probe TO weather_ingest");
    // retain serving after either an aborted SQL archive or a plain archive failure
    for (const [index, failSql] of [true, false].entries()) {
      const failedArchiveRun = { ...run, modelSha256: String(index + 1).repeat(64) };
      let archiveCalled = false;
      assert.equal(await appendRainAdjustmentRun(ingest, failedArchiveRun,
        // force the optional archive to fail inside the serving transaction
        async (queryable) => {
          archiveCalled = true;
          await queryable.query("INSERT INTO archive_probe VALUES ($1)", [failedArchiveRun.modelSha256]);
          // exercise PostgreSQL's aborted transaction state
          if (failSql) {
            await queryable.query("SELECT 1 / 0");
          }
          throw new Error("archive unavailable");
        }), true);
      assert.equal(archiveCalled, true);
      const stored = await owner.query(
        "SELECT hours FROM rain_adjustment_runs WHERE run_initialized_at = $1 AND model_sha256 = $2",
        [failedArchiveRun.runInitializedAt, failedArchiveRun.modelSha256],
      );
      assert.deepEqual(stored.rows[0]?.hours, run.hours);
      assert.equal((await owner.query("SELECT count(*)::integer AS count FROM archive_probe")).rows[0].count, 0);
      assert.equal(await appendRainAdjustmentRun(ingest, failedArchiveRun), false);
    }
    let published = false;
    const archivedRun = { ...run, modelSha256: "3".repeat(64) };
    assert.equal(await appendRainAdjustmentRun(ingest, archivedRun,
      // prove publication observes the committed serving row from another connection
      async () => ({
        async publish() {
          const visible = await owner.query("SELECT hours FROM rain_adjustment_runs WHERE model_sha256 = $1",
            [archivedRun.modelSha256]);
          assert.deepEqual(visible.rows[0]?.hours, run.hours);
          published = true;
        },
      })), true);
    assert.equal(published, true);
  } finally {
    await Promise.all([owner?.end(), ingest?.end(), api?.end(), exporter?.end(), admin.end()]);
    await stopPostgres(server);
  }
});
