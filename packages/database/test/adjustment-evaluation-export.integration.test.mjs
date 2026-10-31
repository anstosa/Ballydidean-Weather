import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { cp, mkdir, mkdtemp, readdir, rm } from "node:fs/promises";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { promisify } from "node:util";
import { gzipSync } from "node:zlib";
import test from "node:test";

import {
  bootstrapSiteConfiguration,
  loadSiteConfiguration,
  runMigrations,
} from "../dist/index.js";
import {
  createTestPool,
  prepareRuntimeRoles,
  startPostgres,
  stopPostgres,
} from "./postgres-harness.mjs";

const executeFile = promisify(execFile);
const repositoryRoot = resolve(import.meta.dirname, "../../..");
const migrationDirectory = join(repositoryRoot, "packages/database/migrations");
const runtimeAclPath = join(repositoryRoot, "deploy/postgres/runtime-acl-v2.sql");
const sitePath = join(repositoryRoot, "config/sites/ballydidean.json");

// apply the production ACL through the container administrator
async function applyRuntimeAcl(server) {
  await executeFile("docker", ["cp", runtimeAclPath, `${server.name}:/tmp/runtime-acl-v2.sql`]);
  await executeFile("docker", [
    "exec",
    server.name,
    "psql",
    "--set=ON_ERROR_STOP=1",
    "--username",
    server.user,
    "--dbname",
    "weather_test",
    "--file",
    "/tmp/runtime-acl-v2.sql",
  ]);
}

// list every relation readable by the export role
async function exportRoleRelations(pool) {
  const grants = await pool.query(`SELECT relation.relname
    FROM pg_class relation
    JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'public'
      AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
      AND has_table_privilege('weather_training_export', relation.oid, 'SELECT')
    ORDER BY relation.relname`);
  return grants.rows.map((row) => row.relname);
}

// select one bounded export range in a read-only transaction
async function exportRows(pool, fromDate, toDate) {
  const client = await pool.connect();

  try {
    await client.query("BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY");
    await client.query(
      "SELECT set_config('weather.adjustment_evaluation_from_date', $1, true)",
      [fromDate],
    );
    await client.query(
      "SELECT set_config('weather.adjustment_evaluation_to_date', $1, true)",
      [toDate],
    );
    const rows = await client.query(`SELECT *
      FROM adjustment_evaluation_export_rows_v1
      ORDER BY record_kind, record_revision_identity`);
    await client.query("COMMIT");
    return rows.rows;
  } catch (error) {
    await client.query("ROLLBACK").catch(() => undefined);
    throw error;
  } finally {
    client.release();
  }
}

test("migration 0017 upgrades the exact 0016 ledger without changing retained data", { timeout: 180_000 }, async () => {
  const server = await startPostgres(17, "adjustment-evaluation-upgrade");
  const admin = createTestPool(server);
  const root = await mkdtemp(join(tmpdir(), "weather-adjustment-upgrade-"));
  const legacyDirectory = join(root, "migrations");
  const targetDirectory = join(root, "target-migrations");
  let owner;

  try {
    await mkdir(legacyDirectory);
    await mkdir(targetDirectory);

    // construct the immutable historical ledgers
    for (const name of (await readdir(migrationDirectory)).sort()) {
      // retain the exact predecessor
      if (name < "0017_adjustment_evaluation_export.sql") {
        await cp(join(migrationDirectory, name), join(legacyDirectory, name));
      }
      // retain the exact target
      if (name <= "0017_adjustment_evaluation_export.sql") {
        await cp(join(migrationDirectory, name), join(targetDirectory, name));
      }
    }
    await prepareRuntimeRoles(admin);
    owner = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    const legacy = await runMigrations(owner, legacyDirectory);
    assert.equal(legacy.applied.at(-1), "0016_rain_adjustment.sql");
    await applyRuntimeAcl(server);
    assert.deepEqual(await exportRoleRelations(owner), [
      "forecast_training_export_manifest_v1",
      "forecast_training_export_rows_v1",
    ]);
    await owner.query(`INSERT INTO sites
      (slug, display_name, latitude, longitude, timezone)
      VALUES ('upgrade-retained', 'Upgrade retained', 47.95, -122.43, 'UTC')`);
    const upgraded = await runMigrations(owner, targetDirectory);
    assert.deepEqual(upgraded.applied, ["0017_adjustment_evaluation_export.sql"]);
    await applyRuntimeAcl(server);
    assert.deepEqual(await exportRoleRelations(owner), [
      "adjustment_evaluation_export_manifest_v1",
      "adjustment_evaluation_export_rows_v1",
      "forecast_training_export_manifest_v1",
      "forecast_training_export_rows_v1",
    ]);
    assert.equal(
      (await owner.query("SELECT count(*)::integer AS count FROM sites WHERE slug = 'upgrade-retained'"))
        .rows[0].count,
      1,
    );
    assert.deepEqual(
      (await owner.query(`SELECT relname
        FROM pg_class
        WHERE relname IN (
          'adjustment_evaluation_export_rows_v1',
          'adjustment_evaluation_export_manifest_v1'
        )
        ORDER BY relname`)).rows.map((row) => row.relname),
      [
        "adjustment_evaluation_export_manifest_v1",
        "adjustment_evaluation_export_rows_v1",
      ],
    );
  } finally {
    await Promise.all([owner?.end(), admin.end()]);
    await stopPostgres(server);
    await rm(root, { force: true, recursive: true });
  }
});

test("adjustment evaluation export is bounded, repeatable-read, and limited to four views", { timeout: 180_000 }, async () => {
  const server = await startPostgres(17, "adjustment-evaluation-export");
  const admin = createTestPool(server);
  let owner;
  let exporter;
  let api;

  try {
    await prepareRuntimeRoles(admin);
    owner = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    await runMigrations(owner, migrationDirectory);
    await bootstrapSiteConfiguration(owner, await loadSiteConfiguration(sitePath));
    await applyRuntimeAcl(server);
    exporter = createTestPool(
      server,
      "weather_test",
      "weather_training_export",
      "training-export-test",
    );
    api = createTestPool(server, "weather_test", "weather_api", "api-test");
    const siteId = (await owner.query("SELECT id FROM sites WHERE slug = 'ballydidean'")).rows[0].id;
    const run = (await owner.query(`INSERT INTO ecmwf_temperature_canary_runs
      (site_id, run_initialized_at, first_received_at, last_received_at, upstream_model,
       adapter_version, provider_response_sha256, model_cycle, recent_error_state,
       recent_error_state_sha256, state_status, state_reason, content_hash, revision_count)
      VALUES ($1, '2026-10-01T00:00:00Z', '2026-10-01T06:01:00Z',
        '2026-10-01T06:01:00Z', 'ecmwf_ifs', 'open-meteo-ecmwf-single-run/v1',
        $2, '50r1', '{}', $3, 'cold', 'fixture', $4, 0) RETURNING id`,
    [siteId, "a".repeat(64), "b".repeat(64), "c".repeat(64)])).rows[0];
    await owner.query(`INSERT INTO ecmwf_temperature_canary_hours
      (run_id, valid_at, model_lead_hours, raw_temperature_c,
       raw_relative_humidity_percent, raw_wind_speed_mps, content_hash)
      VALUES ($1, '2026-10-01T07:00:00Z', 7, 12.5, 80, 3.2, $2)`,
    [run.id, "d".repeat(64)]);
    const physicalSource = (await owner.query(`WITH provider AS (
        INSERT INTO providers
          (provider_key, display_name, attribution_label, attribution_url)
        VALUES ('target-revision-fixture', 'Target revision fixture',
          'Target revision fixture', 'https://example.invalid')
        RETURNING id
      ), station AS (
        INSERT INTO stations
          (site_id, slug, display_name, station_kind, latitude, longitude)
        VALUES ($1, 'target-revision-fixture', 'Target revision fixture',
          'physical', 47.95, -122.43)
        RETURNING id, slug
      )
      INSERT INTO sources
        (station_id, provider_id, source_key, source_kind,
         material_provider_config, source_config_fingerprint, capabilities,
         cadence_seconds)
      SELECT station.id, provider.id, 'target-revision-fixture-v1',
        'physical_sensor', '{"contractVersion":"target-revision-fixture/v1"}',
        $2, '["historical"]', 300
      FROM provider, station
      RETURNING id, source_config_fingerprint,
        'target-revision-fixture'::text AS physical_station_key`,
    [siteId, "0".repeat(64)])).rows[0];
    const physicalRun = (await owner.query(`INSERT INTO ingestion_runs
      (source_id, mode, requested_start, requested_end_exclusive,
       source_config_fingerprint, adapter_version, started_at, deadline_at,
       completed_at, state, attempts, record_count)
      VALUES ($1, 'scheduled', '2026-10-01T07:00:00Z', '2026-10-01T08:00:00Z',
        $2, 'target-revision-fixture/v1', '2026-10-01T08:00:00Z',
        '2026-10-01T08:05:00Z', '2026-10-01T08:01:00Z', 'succeeded', 1, 2)
      RETURNING id`, [physicalSource.id, physicalSource.source_config_fingerprint])).rows[0];
    await owner.query(`INSERT INTO weather_records
      (source_id, source_kind, valid_at, first_ingestion_run_id,
       last_ingestion_run_id, first_received_at, last_received_at,
       upstream_timezone, temperature_c, content_hash, revision_count)
      VALUES
        ($1, 'physical_sensor', '2026-10-01T07:05:00Z', $2, $2,
          '2026-10-01T07:06:00Z', '2026-10-01T07:07:00Z', 'UTC', 12.4, $3, 0),
        ($1, 'physical_sensor', '2026-10-01T07:25:00Z', $2, $2,
          '2026-10-01T07:26:00Z', '2026-10-01T07:29:00Z', 'UTC', 12.6, $4, 2)`,
    [physicalSource.id, physicalRun.id, "1".repeat(64), "2".repeat(64)]);
    const bodyPlaintext = Buffer.from(JSON.stringify({
      fixture: Array.from({ length: 128 }, (_value, index) =>
        createHash("sha256").update(String(index)).digest("hex")),
    }));
    const body = gzipSync(bodyPlaintext);
    const receiptMetadata = { fixture: "line\nslash\\value" };
    await owner.query(`ALTER TABLE rain_capture_claims
      DISABLE TRIGGER rain_capture_claims_guard_insert`);
    await owner.query(`ALTER TABLE rain_capture_receipts
      DISABLE TRIGGER rain_capture_receipts_guard_insert`);
    const claim = (await owner.query(`INSERT INTO rain_capture_claims
      (kind, slot_key, run_initialized_at, attempt, release, policy, policy_sha256,
       claimed_at)
      VALUES ('forecast', 'evaluation-fixture', '2026-10-01T12:00:00Z', 1,
        '2026.10.01-6', '{}', $1, '2026-10-01T06:01:00Z') RETURNING id`,
    ["e".repeat(64)])).rows[0];
    await owner.query(`INSERT INTO rain_capture_receipts
      (claim_id, started_at, completed_at, http_status, outcome, compressed_body,
       body_sha256, body_bytes, compressed_bytes, parser_version, row_count,
       available_by_decision, metadata)
      VALUES ($1, '2026-10-01T06:00:00Z', '2026-10-01T06:01:00Z', 200, 'valid',
        $2, $3, $4, $5, 'rain-prospective-capture/v1', 49, true, $6::jsonb)`,
    [
      claim.id,
      body,
      createHash("sha256").update(bodyPlaintext).digest("hex"),
      bodyPlaintext.length,
      body.length,
      JSON.stringify(receiptMetadata),
    ]);
    await owner.query(`ALTER TABLE rain_capture_claims
      ENABLE TRIGGER rain_capture_claims_guard_insert`);
    await owner.query(`ALTER TABLE rain_capture_receipts
      ENABLE TRIGGER rain_capture_receipts_guard_insert`);

    const rows = await exportRows(exporter, "2026-10-01", "2026-10-01");
    assert.deepEqual(
      rows.map((row) => row.record_kind),
      [
        "rain_claim",
        "rain_receipt",
        "target_revision_diagnostic",
        "temperature_hour",
        "temperature_run",
      ],
    );
    assert.deepEqual(rows.find((row) => row.record_kind === "rain_receipt").compressed_body, body);
    const targetDiagnostic = rows.find((row) => row.record_kind === "target_revision_diagnostic");
    assert.equal(targetDiagnostic.payload.physicalStationKey, physicalSource.physical_station_key);
    assert.equal(targetDiagnostic.payload.recordCount, 2);
    assert.equal(targetDiagnostic.payload.revisedRecordCount, 1);
    assert.equal(targetDiagnostic.payload.maxRevisionCount, 2);
    assert.deepEqual(targetDiagnostic.payload.sourceKeys, [targetDiagnostic.payload.sourceKey]);
    assert.match(targetDiagnostic.payload.contributorContentHashesSha256, /^[a-f0-9]{64}$/u);
    assert.match(targetDiagnostic.payload.contributorRevisionSha256, /^[a-f0-9]{64}$/u);
    assert.equal(Object.hasOwn(targetDiagnostic.payload, "recordId"), false);
    assert.equal(rows.some((row) => JSON.stringify(row.payload).includes("https://")), false);
    const copySql = `BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;
      SET LOCAL weather.adjustment_evaluation_from_date TO '2026-10-01';
      SET LOCAL weather.adjustment_evaluation_to_date TO '2026-10-01';
      COPY (
        SELECT jsonb_build_object(
          'compressed_body_base64', encode(compressed_body, 'base64'),
          'metadata', payload -> 'metadata'
        )::text
        FROM adjustment_evaluation_export_rows_v1
        WHERE record_kind = 'rain_receipt'
      ) TO STDOUT WITH (
        FORMAT csv,
        DELIMITER E'\\x01',
        QUOTE E'\\x02',
        ESCAPE E'\\x02'
      );
      COMMIT;`;
    const copied = await executeFile("docker", [
      "exec",
      server.name,
      "psql",
      "--quiet",
      "--username",
      server.user,
      "--dbname",
      "weather_test",
      "--command",
      copySql,
    ]);
    const copiedReceipt = JSON.parse(copied.stdout.trim());
    assert.deepEqual(Buffer.from(copiedReceipt.compressed_body_base64, "base64"), body);
    assert.deepEqual(copiedReceipt.metadata, receiptMetadata);
    const manifest = await exporter.query("SELECT * FROM adjustment_evaluation_export_manifest_v1");
    assert.equal(manifest.rows[0].schema_migration, "0017_adjustment_evaluation_export.sql");
    // retain the complete current ledger without changing the legacy export view
    assert.equal(manifest.rows[0].migration_names.at(-1), "0022_forecast_product_catalog.sql");

    await assert.rejects(exportRows(exporter, "2026-10-02", "2026-10-01"), /division by zero/u);
    await assert.rejects(exportRows(exporter, "2026-10-01", "2026-10-15"), /division by zero/u);
    await assert.rejects(
      exporter.query("SELECT * FROM adjustment_evaluation_export_rows_v1"),
      /division by zero|invalid input syntax for type date/u,
    );
    await assert.rejects(exporter.query("SELECT * FROM rain_capture_receipts"), /permission denied/u);
    await assert.rejects(
      exporter.query("CREATE TEMP TABLE leak(value text)"),
      /permission denied|read-only/u,
    );
    await assert.rejects(
      exporter.query("DELETE FROM adjustment_evaluation_export_rows_v1"),
      /cannot delete|read-only|permission denied/u,
    );
    await assert.rejects(api.query("SELECT * FROM adjustment_evaluation_export_rows_v1"), /permission denied/u);

    assert.deepEqual(await exportRoleRelations(owner), [
      "adjustment_evaluation_export_manifest_v1",
      "adjustment_evaluation_export_rows_v1",
      "forecast_training_export_manifest_v1",
      "forecast_training_export_rows_v1",
    ]);
  } finally {
    await Promise.all([owner?.end(), exporter?.end(), api?.end(), admin.end()]);
    await stopPostgres(server);
  }
});
