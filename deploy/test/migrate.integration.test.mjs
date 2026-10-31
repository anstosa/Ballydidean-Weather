import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { promisify } from "node:util";

import {
  createTestPool,
  startPostgres,
  stopPostgres,
} from "../../packages/database/test/postgres-harness.mjs";

const executeFile = promisify(execFile);
const repoRoot = resolve(import.meta.dirname, "../..");
const migrationDirectory = join(repoRoot, "packages/database/migrations");
const runtimeAclPath = join(repoRoot, "deploy/postgres/runtime-acl-v2.sql");
const siteConfigurationPath = join(repoRoot, "config/sites/ballydidean.json");
const ecowittConfigurationPath = join(repoRoot, "config/ecowitt/gateways.json");
const publicStationConfigurationPath = join(
  repoRoot,
  "config/public-stations/stations.json",
);
const tempestConfigurationPath = join(repoRoot, "config/tempest/stations.json");
const tideConfigurationPath = join(repoRoot, "config/tides/noaa.json");
const runIntegration = process.env.WEATHER_RUN_DEPLOY_INTEGRATION === "1";

// run one semantic snapshot command against the disposable PostgreSQL server
async function runSemanticProof(server, action, contractPath, beforePath, afterPath) {
  const script = `
    source "$UPDATE_SCRIPT"
    compose() {
      while (( $# > 0 )); do
        if [[ "$1" == psql ]]; then
          shift
          PGPASSWORD="$PGPASSWORD" command psql --host "$PGHOST" --port "$PGPORT" "$@"
          return
        fi
        shift
      done
      return 1
    }
    case "$ACTION" in
      contract)
        write_preexisting_data_contract ignored weather_test "$CONTRACT_PATH"
        ;;
      before)
        write_preexisting_data_snapshot ignored weather_test "$CONTRACT_PATH" "$BEFORE_PATH"
        ;;
      after)
        write_preexisting_data_snapshot ignored weather_test "$CONTRACT_PATH" "$AFTER_PATH"
        ;;
      verify)
        verify_preexisting_data_snapshots "$CONTRACT_PATH" "$BEFORE_PATH" "$AFTER_PATH"
        ;;
      *)
        exit 64
        ;;
    esac
  `;
  await executeFile("bash", ["-c", script], {
    cwd: repoRoot,
    env: {
      ...process.env,
      ACTION: action,
      AFTER_PATH: afterPath,
      BEFORE_PATH: beforePath,
      CONTRACT_PATH: contractPath,
      PGHOST: server.host,
      PGPASSWORD: server.password,
      PGPORT: String(server.port),
      UPDATE_SCRIPT: join(repoRoot, "deploy/scripts/update.sh"),
    },
    timeout: 120_000,
  });
}

// hash one raw PG17 plain dump including its randomized restrict envelope
async function rawDumpSha256(server) {
  const { stdout } = await executeFile(
    "docker",
    [
      "exec",
      server.name,
      "pg_dump",
      "--username",
      "weather_owner",
      "--dbname",
      "weather_test",
      "--format=plain",
      "--data-only",
      "--no-owner",
      "--no-privileges",
    ],
    { encoding: "buffer", timeout: 120_000 },
  );
  return createHash("sha256").update(stdout).digest("hex");
}

// enumerate exact export authority
const exportAuthoritySnapshotSql = `
  SELECT
    ARRAY(
      SELECT privilege.name
      FROM unnest(ARRAY['CONNECT', 'CREATE', 'TEMP']) privilege(name)
      WHERE has_database_privilege(
        'weather_training_export', current_database(), privilege.name
      )
      ORDER BY privilege.name
    ) AS database_privileges,
    ARRAY(
      SELECT format('%s:%s', namespace.nspname, privilege.name)
      FROM pg_namespace namespace
      CROSS JOIN LATERAL unnest(ARRAY['CREATE', 'USAGE']) privilege(name)
      WHERE namespace.nspname <> 'information_schema'
        AND namespace.nspname NOT LIKE 'pg_%'
        AND has_schema_privilege(
          'weather_training_export', namespace.oid, privilege.name
        )
      ORDER BY namespace.nspname, privilege.name
    ) AS schema_privileges,
    ARRAY(
      SELECT format(
        '%s.%s:%s', namespace.nspname, relation.relname, privilege.name
      )
      FROM pg_class relation
      JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
      CROSS JOIN LATERAL unnest(ARRAY[
        'SELECT', 'INSERT', 'UPDATE', 'DELETE', 'TRUNCATE', 'REFERENCES', 'TRIGGER'
      ]) privilege(name)
      WHERE namespace.nspname <> 'information_schema'
        AND namespace.nspname NOT LIKE 'pg_%'
        AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
        AND has_table_privilege(
          'weather_training_export', relation.oid, privilege.name
        )
      ORDER BY namespace.nspname, relation.relname, privilege.name
    ) AS relation_privileges,
    ARRAY(
      SELECT format(
        '%s.%s:%s', namespace.nspname, sequence.relname, privilege.name
      )
      FROM pg_class sequence
      JOIN pg_namespace namespace ON namespace.oid = sequence.relnamespace
      CROSS JOIN LATERAL unnest(ARRAY['SELECT', 'UPDATE', 'USAGE']) privilege(name)
      WHERE namespace.nspname <> 'information_schema'
        AND namespace.nspname NOT LIKE 'pg_%'
        AND sequence.relkind = 'S'
        AND has_sequence_privilege(
          'weather_training_export', sequence.oid, privilege.name
        )
      ORDER BY namespace.nspname, sequence.relname, privilege.name
    ) AS sequence_privileges,
    ARRAY(
      SELECT procedure.oid::regprocedure::text
      FROM pg_proc procedure
      JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
      WHERE namespace.nspname <> 'information_schema'
        AND namespace.nspname NOT LIKE 'pg_%'
        AND procedure.prosecdef
        AND has_function_privilege(
          'weather_training_export', procedure.oid, 'EXECUTE'
        )
      ORDER BY procedure.oid::regprocedure::text
    ) AS executable_functions,
    ARRAY(
      SELECT format(
        '%s:%s', database.datname, array_to_string(setting.setconfig, ',')
      )
      FROM pg_db_role_setting setting
      JOIN pg_database database ON database.oid = setting.setdatabase
      WHERE setting.setrole = 'weather_training_export'::regrole
      ORDER BY database.datname
    ) AS database_settings
`;

// retain the exact read-only export authority
const expectedExportAuthoritySnapshot = {
  database_privileges: ["CONNECT"],
  database_settings: [],
  executable_functions: [
    "adjustment_confirmation_availability_v2(text)",
    "adjustment_confirmation_export_v2(text,text,smallint)",
    "adjustment_revision_frontier_v1()",
    "adjustment_revision_serving_snapshot_v1(timestamp with time zone,bigint)",
    "adjustment_shadow_registration_slot_v3(text)",
  ],
  relation_privileges: [
    "public.adjustment_evaluation_export_manifest_v1:SELECT",
    "public.adjustment_evaluation_export_rows_v1:SELECT",
    "public.forecast_training_export_manifest_v1:SELECT",
    "public.forecast_training_export_rows_v1:SELECT",
  ],
  schema_privileges: ["public:USAGE"],
  sequence_privileges: [],
};

// insert one fixed-lead anchor fixture
const insertAnchorSql = `
  INSERT INTO forecast_anchor_records (
    source_id,
    source_kind,
    source_config_fingerprint,
    valid_at,
    lead_hours,
    dataset,
    upstream_model,
    contract_epoch,
    adapter_version,
    first_ingestion_run_id,
    last_ingestion_run_id,
    first_received_at,
    last_received_at,
    upstream_timezone,
    quality_metadata,
    provider_metadata,
    temperature_c,
    content_hash,
    revision_count
  )
  VALUES (
    $1,
    'forecast',
    $2,
    $3,
    $4,
    $5,
    $6,
    $7,
    $8,
    $9,
    $10,
    $11,
    $12,
    'UTC',
    '{"status":"validated"}'::jsonb,
    '{"dataset":"previous_runs","request_id":"anchor-integration"}'::jsonb,
    $13,
    $14,
    $15
  )
`;

// revise one anchor only when content changes
const upsertAnchorSql = `${insertAnchorSql}
  ON CONFLICT ON CONSTRAINT forecast_anchor_records_identity_key DO UPDATE SET
    last_ingestion_run_id = EXCLUDED.last_ingestion_run_id,
    last_received_at = EXCLUDED.last_received_at,
    temperature_c = CASE
      WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash
        THEN EXCLUDED.temperature_c
      ELSE forecast_anchor_records.temperature_c
    END,
    revision_count = forecast_anchor_records.revision_count + CASE
      WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN 1
      ELSE 0
    END,
    content_hash = EXCLUDED.content_hash
`;

// require PostgreSQL privilege denial
async function assertPrivilegeDenied(operation) {
  await assert.rejects(operation, { code: "42501" });
}

// set one validated export range on the pooled session
async function setTrainingExportRange(pool, fromDate, toDate) {
  await pool.query(
    `
      SELECT
        set_config('weather.forecast_training_from_date', $1, false),
        set_config('weather.forecast_training_to_date', $2, false)
    `,
    [fromDate, toDate],
  );
}

// configure production-equivalent database roles
async function bootstrapRuntimeRoles(server, directory) {
  const adminPath = join(directory, "admin");
  const ownerPath = join(directory, "owner");
  const apiPath = join(directory, "api");
  const ingestPath = join(directory, "ingest");
  const trainingExportPath = join(directory, "training-export");
  await Promise.all([
    writeFile(adminPath, `${server.password}\n`, { mode: 0o600 }),
    writeFile(ownerPath, "owner-test\n", { mode: 0o600 }),
    writeFile(apiPath, "api-test\n", { mode: 0o600 }),
    writeFile(ingestPath, "ingest-test\n", { mode: 0o600 }),
    writeFile(trainingExportPath, "training-export-test\n", { mode: 0o600 }),
  ]);
  await executeFile(join(repoRoot, "deploy/postgres/010-create-runtime-roles.sh"), [], {
    env: {
      ...process.env,
      PGHOST: server.host,
      PGPASSWORD: server.password,
      PGPORT: String(server.port),
      POSTGRES_DB: "weather_test",
      POSTGRES_USER: server.user,
      WEATHER_ADMIN_PASSWORD_FILE: adminPath,
      WEATHER_API_PASSWORD_FILE: apiPath,
      WEATHER_INGEST_PASSWORD_FILE: ingestPath,
      WEATHER_OWNER_PASSWORD_FILE: ownerPath,
      WEATHER_TRAINING_EXPORT_PASSWORD_FILE: trainingExportPath,
    },
    timeout: 30_000,
  });
  return ownerPath;
}

// prove the release semantic data guard against real PostgreSQL bootstrap effects
test(
  "semantic migration proof accepts bootstrap volatility and rejects application drift",
  {
    skip: runIntegration ? false : "set WEATHER_RUN_DEPLOY_INTEGRATION=1",
    timeout: 300_000,
  },
  async () => {
    const directory = await mkdtemp(join(tmpdir(), "weather-semantic-proof-"));
    let server;
    let pool;

    try {
      server = await startPostgres(17, "semantic-proof");
      const ownerPath = await bootstrapRuntimeRoles(server, directory);
      const contractPath = join(directory, "contract");
      const beforePath = join(directory, "before");
      const afterPath = join(directory, "after");
      const configuredSitePath = join(directory, "ballydidean.json");
      const configuredSite = JSON.parse(await readFile(siteConfigurationPath, "utf8"));
      await Promise.all([
        writeFile(contractPath, "", { mode: 0o600 }),
        writeFile(beforePath, "", { mode: 0o600 }),
        writeFile(afterPath, "", { mode: 0o600 }),
        writeFile(configuredSitePath, `${JSON.stringify(configuredSite)}\n`),
      ]);
      const environment = {
        ...process.env,
        WEATHER_DATABASE_APPLICATION_NAME: "weather-semantic-proof-test",
        WEATHER_DATABASE_HOST: server.host,
        WEATHER_DATABASE_NAME: "weather_test",
        WEATHER_DATABASE_PASSWORD_FILE: ownerPath,
        WEATHER_DATABASE_PORT: String(server.port),
        WEATHER_DATABASE_SSL: "false",
        WEATHER_DATABASE_USER: "weather_owner",
        WEATHER_ECOWITT_CONFIG_PATH: ecowittConfigurationPath,
        WEATHER_MIGRATION_DIRECTORY: migrationDirectory,
        WEATHER_PUBLIC_STATIONS_CONFIG_PATH: publicStationConfigurationPath,
        WEATHER_SITE_CONFIG_PATH: configuredSitePath,
        WEATHER_TEMPEST_CONFIG_PATH: tempestConfigurationPath,
        WEATHER_TIDE_CONFIG_PATH: tideConfigurationPath,
      };
      await executeFile(process.execPath, ["deploy/scripts/migrate.mjs"], {
        cwd: repoRoot,
        env: environment,
        timeout: 120_000,
      });
      pool = createTestPool(server, "weather_test", "weather_owner", "owner-test");
      // seed one metric and one independently deletable application row
      await pool.query(`
        WITH selected_source AS (
          SELECT id, source_kind, source_config_fingerprint
          FROM sources
          WHERE source_kind = 'physical_sensor'
          ORDER BY id
          LIMIT 1
        ), inserted_run AS (
          INSERT INTO ingestion_runs (
            source_id, mode, requested_start, requested_end_exclusive,
            source_config_fingerprint, adapter_version, started_at, deadline_at,
            completed_at, state, attempts, record_count
          )
          SELECT id, 'scheduled', '2026-01-01T00:00:00Z',
            '2026-01-01T01:00:00Z', source_config_fingerprint,
            'semantic-proof-v1', '2026-01-01T00:00:00Z',
            '2026-01-01T02:00:00Z', '2026-01-01T00:01:00Z',
            'succeeded', 1, 1
          FROM selected_source
          RETURNING id, source_id
        )
        INSERT INTO weather_records (
          source_id, source_kind, valid_at, first_ingestion_run_id,
          last_ingestion_run_id, first_received_at, last_received_at,
          upstream_timezone, temperature_c, content_hash
        )
        SELECT selected_source.id, selected_source.source_kind,
          '2026-01-01T00:30:00Z', inserted_run.id, inserted_run.id,
          '2026-01-01T00:31:00Z', '2026-01-01T00:31:00Z',
          'UTC', 7.5, repeat('a', 64)
        FROM selected_source
        JOIN inserted_run ON inserted_run.source_id = selected_source.id;
        INSERT INTO worker_heartbeats (
          worker_instance, last_loop_at, worker_version
        ) VALUES ('semantic-proof-worker', '2026-01-01T00:00:00Z', 'test-v1');
      `);
      await runSemanticProof(server, "contract", contractPath, beforePath, afterPath);
      await runSemanticProof(server, "before", contractPath, beforePath, afterPath);
      const rawBefore = await rawDumpSha256(server);

      // repeat the unchanged production bootstrap entrypoint
      await executeFile(process.execPath, ["deploy/scripts/migrate.mjs"], {
        cwd: repoRoot,
        env: environment,
        timeout: 120_000,
      });
      const rawAfter = await rawDumpSha256(server);
      assert.notEqual(rawAfter, rawBefore);
      await runSemanticProof(server, "after", contractPath, beforePath, afterPath);
      await runSemanticProof(server, "verify", contractPath, beforePath, afterPath);

      // reject a changed metric despite valid revision bookkeeping
      await pool.query(`
        UPDATE weather_records
        SET temperature_c = 8.5,
          content_hash = repeat('b', 64),
          revision_count = revision_count + 1
        WHERE content_hash = repeat('a', 64)
      `);
      await runSemanticProof(server, "after", contractPath, beforePath, afterPath);
      await assert.rejects(
        runSemanticProof(server, "verify", contractPath, beforePath, afterPath),
      );
      await runSemanticProof(server, "before", contractPath, beforePath, afterPath);

      // reject semantic source configuration drift while ignoring only updated_at
      await pool.query(`
        UPDATE sources
        SET cadence_seconds = CASE cadence_seconds WHEN 60 THEN 120 ELSE 60 END
        WHERE id = (SELECT min(id) FROM sources)
      `);
      await runSemanticProof(server, "after", contractPath, beforePath, afterPath);
      await assert.rejects(
        runSemanticProof(server, "verify", contractPath, beforePath, afterPath),
      );
      await runSemanticProof(server, "before", contractPath, beforePath, afterPath);

      // reject deletion of an otherwise independent application row
      await pool.query("DELETE FROM worker_heartbeats WHERE worker_instance = $1", [
        "semantic-proof-worker",
      ]);
      await runSemanticProof(server, "after", contractPath, beforePath, afterPath);
      await assert.rejects(
        runSemanticProof(server, "verify", contractPath, beforePath, afterPath),
      );
      await runSemanticProof(server, "before", contractPath, beforePath, afterPath);

      // reject a reset of one reviewed bootstrap identity sequence
      await pool.query("ALTER SEQUENCE sources_id_seq RESTART WITH 1");
      await runSemanticProof(server, "after", contractPath, beforePath, afterPath);
      await assert.rejects(
        runSemanticProof(server, "verify", contractPath, beforePath, afterPath),
      );
    } finally {
      await pool?.end().catch(() => undefined);
      await stopPostgres(server ?? { name: "missing" });
      await rm(directory, { force: true, recursive: true });
    }
  },
);

// read stable bootstrap identities and counts
async function readBootstrapSnapshot(pool) {
  const [sites, stations, providers, sources, migrations, identity, owners, ordering] =
    await Promise.all([
    pool.query("SELECT id, slug, display_name FROM sites ORDER BY slug"),
    pool.query("SELECT id, slug FROM stations ORDER BY slug"),
    pool.query("SELECT id, provider_key FROM providers ORDER BY provider_key"),
    pool.query("SELECT id, source_key FROM sources ORDER BY source_key"),
    pool.query("SELECT name FROM schema_migrations ORDER BY name"),
    pool.query("SELECT current_user, session_user"),
    pool.query(`
      SELECT tablename, tableowner
      FROM pg_tables
      WHERE schemaname = 'public'
        AND tablename = ANY($1::text[])
      ORDER BY tablename
    `, [["providers", "schema_migrations", "sites", "sources", "stations"]]),
    pool.query(`
      SELECT
        (SELECT max(applied_at) FROM schema_migrations) <=
        (SELECT min(created_at) FROM sites) AS migrations_first
    `),
  ]);

  return {
    identity: identity.rows[0],
    migrations: migrations.rows,
    migrationsFirst: ordering.rows[0].migrations_first,
    owners: owners.rows,
    providers: providers.rows,
    sites: sites.rows,
    sources: sources.rows,
    stations: stations.rows,
  };
}

// execute the real migration one-shot twice
test(
  "migration one-shot bootstraps configured Ballydidean data idempotently",
  {
    skip: runIntegration ? false : "set WEATHER_RUN_DEPLOY_INTEGRATION=1",
    timeout: 300_000,
  },
  async () => {
    const directory = await mkdtemp(join(tmpdir(), "weather-migrate-entrypoint-"));
    let server;
    let adminPool;
    let pool;
    let apiPool;
    let ingestPool;
    let trainingExportPool;
    let reconciledTrainingExportPool;
    let migrationLockClient;
    let temporaryAclClient;

    try {
      server = await startPostgres(17, "migrate-entrypoint");
      adminPool = createTestPool(server);
      const ownerPath = await bootstrapRuntimeRoles(server, directory);
      const configuredSitePath = join(directory, "ballydidean.json");
      const configuredSite = JSON.parse(await readFile(siteConfigurationPath, "utf8"));
      configuredSite.site.displayName = "Ballydidean entrypoint fixture";
      await writeFile(configuredSitePath, `${JSON.stringify(configuredSite)}\n`);
      const environment = {
        ...process.env,
        WEATHER_DATABASE_APPLICATION_NAME: "weather-migration-entrypoint-test",
        WEATHER_DATABASE_HOST: server.host,
        WEATHER_DATABASE_NAME: "weather_test",
        WEATHER_DATABASE_PASSWORD_FILE: ownerPath,
        WEATHER_DATABASE_PORT: String(server.port),
        WEATHER_DATABASE_SSL: "false",
        WEATHER_DATABASE_USER: "weather_owner",
        WEATHER_ECOWITT_CONFIG_PATH: ecowittConfigurationPath,
        WEATHER_MIGRATION_DIRECTORY: migrationDirectory,
        WEATHER_PUBLIC_STATIONS_CONFIG_PATH: publicStationConfigurationPath,
        WEATHER_SITE_CONFIG_PATH: configuredSitePath,
        WEATHER_TEMPEST_CONFIG_PATH: tempestConfigurationPath,
        WEATHER_TIDE_CONFIG_PATH: tideConfigurationPath,
      };
      const first = await executeFile(process.execPath, ["deploy/scripts/migrate.mjs"], {
        cwd: repoRoot,
        env: environment,
        timeout: 60_000,
      });
      pool = createTestPool(server, "weather_test", "weather_owner", "owner-test");
      const firstSnapshot = await readBootstrapSnapshot(pool);
      const second = await executeFile(process.execPath, ["deploy/scripts/migrate.mjs"], {
        cwd: repoRoot,
        env: environment,
        timeout: 60_000,
      });
      const secondSnapshot = await readBootstrapSnapshot(pool);
      const firstEvent = JSON.parse(first.stdout.trim());
      const secondEvent = JSON.parse(second.stdout.trim());

      assert.equal(firstEvent.event, "migrations_complete");
      // require the complete immutable 0022 ledger
      assert.deepEqual(firstEvent.applied, [
        "0001_initial_weather.sql",
        "0002_worker_migration_readiness.sql",
        "0003_ecowitt_measurements.sql",
        "0004_tempest_metadata.sql",
        "0005_source_supersession.sql",
        "0006_station_coordinates.sql",
        "0007_tide_sources.sql",
        "0008_ecowitt_property_sensors.sql",
        "0009_forecast_anchor_records.sql",
        "0010_forecast_training_export.sql",
        "0011_forecast_runtime_provenance.sql",
        "0012_hide_archive_only_forecasts_from_live_reads.sql",
        "0013_ecmwf_temperature_canary.sql",
        "0014_rain_collection.sql",
        "0015_rain_station_access.sql",
        "0016_rain_adjustment.sql",
        "0017_adjustment_evaluation_export.sql",
        "0018_adjustment_maintenance_v2.sql",
        "0019_adjustment_maintenance_recurring.sql",
        "0020_adjustment_revision_frontier.sql",
        "0021_adjustment_rolling_registration.sql",
        "0022_forecast_product_catalog.sql",
        "0023_forecast_product_read_index.sql",
      ]);
      assert.deepEqual(firstEvent.current, []);
      assert.equal(secondEvent.event, "migrations_complete");
      assert.deepEqual(secondEvent.applied, []);
      assert.deepEqual(secondEvent.current, [
        "0001_initial_weather.sql",
        "0002_worker_migration_readiness.sql",
        "0003_ecowitt_measurements.sql",
        "0004_tempest_metadata.sql",
        "0005_source_supersession.sql",
        "0006_station_coordinates.sql",
        "0007_tide_sources.sql",
        "0008_ecowitt_property_sensors.sql",
        "0009_forecast_anchor_records.sql",
        "0010_forecast_training_export.sql",
        "0011_forecast_runtime_provenance.sql",
        "0012_hide_archive_only_forecasts_from_live_reads.sql",
        "0013_ecmwf_temperature_canary.sql",
        "0014_rain_collection.sql",
        "0015_rain_station_access.sql",
        "0016_rain_adjustment.sql",
        "0017_adjustment_evaluation_export.sql",
        "0018_adjustment_maintenance_v2.sql",
        "0019_adjustment_maintenance_recurring.sql",
        "0020_adjustment_revision_frontier.sql",
        "0021_adjustment_rolling_registration.sql",
        "0022_forecast_product_catalog.sql",
        "0023_forecast_product_read_index.sql",
      ]);
      assert.deepEqual(secondEvent.bootstrap, firstEvent.bootstrap);
      assert.deepEqual(secondEvent.ecowittBootstrap, firstEvent.ecowittBootstrap);
      assert.deepEqual(
        secondEvent.publicStationBootstrap,
        firstEvent.publicStationBootstrap,
      );
      assert.deepEqual(secondEvent.tempestBootstrap, firstEvent.tempestBootstrap);
      assert.deepEqual(secondEvent.tideBootstrap, firstEvent.tideBootstrap);
      assert.deepEqual(secondSnapshot, firstSnapshot);
      assert.deepEqual(firstSnapshot.identity, {
        current_user: "weather_owner",
        session_user: "weather_owner",
      });
      assert.equal(firstSnapshot.migrationsFirst, true);
      assert.deepEqual(firstSnapshot.migrations, [
        { name: "0001_initial_weather.sql" },
        { name: "0002_worker_migration_readiness.sql" },
        { name: "0003_ecowitt_measurements.sql" },
        { name: "0004_tempest_metadata.sql" },
        { name: "0005_source_supersession.sql" },
        { name: "0006_station_coordinates.sql" },
        { name: "0007_tide_sources.sql" },
        { name: "0008_ecowitt_property_sensors.sql" },
        { name: "0009_forecast_anchor_records.sql" },
        { name: "0010_forecast_training_export.sql" },
        { name: "0011_forecast_runtime_provenance.sql" },
        { name: "0012_hide_archive_only_forecasts_from_live_reads.sql" },
        { name: "0013_ecmwf_temperature_canary.sql" },
        { name: "0014_rain_collection.sql" },
        { name: "0015_rain_station_access.sql" },
        { name: "0016_rain_adjustment.sql" },
        { name: "0017_adjustment_evaluation_export.sql" },
        { name: "0018_adjustment_maintenance_v2.sql" },
        { name: "0019_adjustment_maintenance_recurring.sql" },
        { name: "0020_adjustment_revision_frontier.sql" },
        { name: "0021_adjustment_rolling_registration.sql" },
        { name: "0022_forecast_product_catalog.sql" },
        { name: "0023_forecast_product_read_index.sql" },
      ]);
      assert.deepEqual(firstSnapshot.owners, [
        { tableowner: "weather_owner", tablename: "providers" },
        { tableowner: "weather_owner", tablename: "schema_migrations" },
        { tableowner: "weather_owner", tablename: "sites" },
        { tableowner: "weather_owner", tablename: "sources" },
        { tableowner: "weather_owner", tablename: "stations" },
      ]);
      assert.deepEqual(firstSnapshot.providers, [
        {
          id: firstEvent.publicStationBootstrap.providerIds[
            "ambient-weather-network"
          ],
          provider_key: "ambient-weather-network",
        },
        {
          id: firstEvent.ecowittBootstrap.providerId,
          provider_key: "ecowitt-local",
        },
        {
          id: firstEvent.publicStationBootstrap.providerIds[
            "netatmo-weathermap"
          ],
          provider_key: "netatmo-weathermap",
        },
        {
          id: firstEvent.tideBootstrap.providerId,
          provider_key: "noaa-co-ops",
        },
        { id: firstEvent.bootstrap.providerId, provider_key: "open-meteo" },
        {
          id: firstEvent.publicStationBootstrap.providerIds.purpleair,
          provider_key: "purpleair",
        },
        {
          id: firstEvent.tempestBootstrap.providerId,
          provider_key: "weatherflow-tempest",
        },
        {
          id: firstEvent.publicStationBootstrap.providerIds[
            "weather-underground"
          ],
          provider_key: "weather-underground",
        },
      ]);
      assert.deepEqual(firstSnapshot.sites, [
        {
          display_name: "Ballydidean entrypoint fixture",
          id: firstEvent.bootstrap.siteId,
          slug: "ballydidean",
        },
      ]);
      assert.deepEqual(firstSnapshot.sources, [
        {
          id: firstEvent.publicStationBootstrap.sourceIds[
            "ambient-maxweather-observations-v1"
          ],
          source_key: "ambient-maxweather-observations-v1",
        },
        {
          id: firstEvent.publicStationBootstrap.sourceIds[
            "ambient-merlin-observations-v1"
          ],
          source_key: "ambient-merlin-observations-v1",
        },
        {
          id: firstEvent.ecowittBootstrap.sourceIds[
            "ecowitt-88f15505d89f-local-live-v1"
          ],
          source_key: "ecowitt-88f15505d89f-local-live-v1",
        },
        {
          id: firstEvent.publicStationBootstrap.sourceIds[
            "netatmo-nearby-observations-v1"
          ],
          source_key: "netatmo-nearby-observations-v1",
        },
        {
          id: firstEvent.tideBootstrap.sourceIds[
            "noaa-glendale-tide-predictions-v1"
          ],
          source_key: "noaa-glendale-tide-predictions-v1",
        },
        {
          id: firstEvent.tideBootstrap.sourceIds[
            "noaa-port-townsend-water-level-v1"
          ],
          source_key: "noaa-port-townsend-water-level-v1",
        },
        { id: firstEvent.bootstrap.sourceIds[0], source_key: "open-meteo-current-v1" },
        { id: firstEvent.bootstrap.sourceIds[2], source_key: "open-meteo-forecast-v1" },
        { id: firstEvent.bootstrap.sourceIds[3], source_key: "open-meteo-forecast-v2" },
        { id: firstEvent.bootstrap.sourceIds[4], source_key: "open-meteo-forecast-v3" },
        { id: firstEvent.bootstrap.sourceIds[5], source_key: "open-meteo-forecast-v4" },
        {
          id: firstEvent.bootstrap.sourceIds[6],
          source_key: "open-meteo-previous-runs-v1",
        },
        { id: firstEvent.bootstrap.sourceIds[1], source_key: "open-meteo-reanalysis-v1" },
        {
          id: firstEvent.publicStationBootstrap.sourceIds[
            "purpleair-samara-observations-v1"
          ],
          source_key: "purpleair-samara-observations-v1",
        },
        {
          id: firstEvent.tempestBootstrap.sourceIds["tempest-126537-observations-v2"],
          source_key: "tempest-126537-observations-v2",
        },
        {
          id: firstEvent.tempestBootstrap.sourceIds["tempest-168853-observations-v2"],
          source_key: "tempest-168853-observations-v2",
        },
        {
          id: firstEvent.tempestBootstrap.sourceIds["tempest-201058-observations-v2"],
          source_key: "tempest-201058-observations-v2",
        },
        {
          id: firstEvent.tempestBootstrap.sourceIds["tempest-203055-observations-v2"],
          source_key: "tempest-203055-observations-v2",
        },
        {
          id: firstEvent.tempestBootstrap.sourceIds["tempest-225947-observations-v2"],
          source_key: "tempest-225947-observations-v2",
        },
        {
          id: firstEvent.tempestBootstrap.sourceIds["tempest-38270-observations-v2"],
          source_key: "tempest-38270-observations-v2",
        },
        {
          id: firstEvent.tempestBootstrap.sourceIds["tempest-64255-observations-v2"],
          source_key: "tempest-64255-observations-v2",
        },
        {
          id: firstEvent.publicStationBootstrap.sourceIds[
            "wunderground-maxweather-history-v1"
          ],
          source_key: "wunderground-maxweather-history-v1",
        },
      ]);
      assert.deepEqual(firstSnapshot.stations, [
        {
          id: firstEvent.publicStationBootstrap.stationIds[
            "ambient-maxweather"
          ],
          slug: "ambient-maxweather",
        },
        {
          id: firstEvent.publicStationBootstrap.stationIds["ambient-merlin"],
          slug: "ambient-merlin",
        },
        {
          id: firstEvent.ecowittBootstrap.stationIds["ballydidean-ecowitt"],
          slug: "ballydidean-ecowitt",
        },
        {
          id: firstEvent.tideBootstrap.stationIds["glendale-tide-predictions"],
          slug: "glendale-tide-predictions",
        },
        {
          id: firstEvent.publicStationBootstrap.stationIds["netatmo-nearby"],
          slug: "netatmo-nearby",
        },
        { id: firstEvent.bootstrap.stationId, slug: "open-meteo-virtual" },
        {
          id: firstEvent.tideBootstrap.stationIds["port-townsend-tide-gauge"],
          slug: "port-townsend-tide-gauge",
        },
        {
          id: firstEvent.publicStationBootstrap.stationIds["purpleair-samara"],
          slug: "purpleair-samara",
        },
        { id: firstEvent.tempestBootstrap.stationIds["tempest-126537"], slug: "tempest-126537" },
        { id: firstEvent.tempestBootstrap.stationIds["tempest-168853"], slug: "tempest-168853" },
        { id: firstEvent.tempestBootstrap.stationIds["tempest-201058"], slug: "tempest-201058" },
        { id: firstEvent.tempestBootstrap.stationIds["tempest-203055"], slug: "tempest-203055" },
        { id: firstEvent.tempestBootstrap.stationIds["tempest-225947"], slug: "tempest-225947" },
        { id: firstEvent.tempestBootstrap.stationIds["tempest-38270"], slug: "tempest-38270" },
        { id: firstEvent.tempestBootstrap.stationIds["tempest-64255"], slug: "tempest-64255" },
      ]);

      // hold the production migration lock
      migrationLockClient = await pool.connect();
      await migrationLockClient.query("SELECT pg_advisory_lock($1::bigint)", [
        8_032_416_683_782_917,
      ]);
      const contentionStartedAt = performance.now();
      await assert.rejects(
        executeFile(process.execPath, ["deploy/scripts/migrate.mjs"], {
          cwd: repoRoot,
          env: {
            ...environment,
            WEATHER_DATABASE_LOCK_TIMEOUT_MS: "250",
            WEATHER_DATABASE_STATEMENT_TIMEOUT_MS: "5000",
          },
          timeout: 5_000,
        }),
        // require the database lock timeout
        (error) => {
          assert.match(error.stderr, /canceling statement due to lock timeout/u);
          return true;
        },
      );
      const contentionElapsedMs = performance.now() - contentionStartedAt;
      assert.equal(contentionElapsedMs >= 200, true);
      assert.equal(contentionElapsedMs < 2_500, true);
      await migrationLockClient.query("SELECT pg_advisory_unlock($1::bigint)", [
        8_032_416_683_782_917,
      ]);
      migrationLockClient.release();
      migrationLockClient = undefined;

      await executeFile(
        "psql",
        [
          "--set=ON_ERROR_STOP=1",
          "--host",
          server.host,
          "--port",
          String(server.port),
          "--username",
          server.user,
          "--dbname",
          "weather_test",
          "--file",
          runtimeAclPath,
        ],
        {
          env: { ...process.env, PGPASSWORD: server.password },
          timeout: 30_000,
        },
      );
      apiPool = createTestPool(server, "weather_test", "weather_api", "api-test");
      ingestPool = createTestPool(
        server,
        "weather_test",
        "weather_ingest",
        "ingest-test",
      );
      trainingExportPool = createTestPool(
        server,
        "weather_test",
        "weather_training_export",
        "training-export-test",
      );
      const runtimeProvenance = await apiPool.query(
        "SELECT * FROM forecast_runtime_provenance_v1",
      );
      assert.equal(runtimeProvenance.rowCount, 0);
      assert.deepEqual(
        runtimeProvenance.fields.map(
          // retain the exact API-only projection
          (field) => field.name,
        ),
        [
          "weather_record_id",
          "source_id",
          "source_key",
          "source_config_fingerprint",
          "adapter_version",
          "contract_epoch",
        ],
      );
      // deny both export families to the API role
      for (const deniedApiQuery of [
        "SELECT source_config_fingerprint FROM sources",
        "SELECT * FROM ingestion_runs",
        "SELECT * FROM forecast_anchor_records",
        "SELECT * FROM forecast_training_export_rows_v1",
        "SELECT * FROM forecast_training_export_manifest_v1",
        "SELECT * FROM adjustment_evaluation_export_rows_v1",
        "SELECT * FROM adjustment_evaluation_export_manifest_v1",
      ]) {
        // deny every training or ingestion authority
        await assertPrivilegeDenied(apiPool.query(deniedApiQuery));
      }
      const anchorSources = await pool.query(
        `
          INSERT INTO sources (
            station_id,
            provider_id,
            source_key,
            source_kind,
            material_provider_config,
            source_config_fingerprint,
            capabilities,
            cadence_seconds,
            active
          )
          VALUES
            ($1, $2, 'anchor-history-integration-v1', 'forecast',
              '{"contractVersion":"previous-runs-hourly/v1"}'::jsonb,
              $3, '["historical"]'::jsonb, NULL, true),
            ($1, $2, 'anchor-forecast-only-integration-v1', 'forecast',
              '{"contractVersion":"forecast-hourly/v1"}'::jsonb,
              $4, '["forecast"]'::jsonb, 3600, true)
          RETURNING id, source_config_fingerprint, source_key
        `,
        [
          firstEvent.bootstrap.stationId,
          firstEvent.bootstrap.providerId,
          "a".repeat(64),
          "b".repeat(64),
        ],
      );
      const historicalAnchorSource = anchorSources.rows.find(
        // select the historical anchor fixture
        (source) => source.source_key === "anchor-history-integration-v1",
      );
      const forecastOnlySource = anchorSources.rows.find(
        // select the forecast-only denial fixture
        (source) => source.source_key === "anchor-forecast-only-integration-v1",
      );
      assert.ok(historicalAnchorSource);
      assert.ok(forecastOnlySource);
      const anchorRuns = await pool.query(
        `
          INSERT INTO ingestion_runs (
            source_id,
            mode,
            requested_start,
            requested_end_exclusive,
            source_config_fingerprint,
            adapter_version,
            chunk_plan_version,
            started_at,
            deadline_at,
            completed_at,
            state,
            attempts,
            record_count
          )
          VALUES
            ($1, 'backfill', '2026-08-01T00:00:00Z', '2026-08-02T00:00:00Z', $2,
              'previous-runs/v1', 'previous-runs-utc/v1', '2026-08-02T00:00:00Z',
              '2026-08-02T00:05:00Z', '2026-08-02T00:01:00Z', 'succeeded', 1, 1),
            ($1, 'backfill', '2026-08-01T00:00:00Z', '2026-08-02T00:00:00Z', $2,
              'previous-runs/v1', 'previous-runs-utc/v1', '2026-08-02T00:02:00Z',
              '2026-08-02T00:07:00Z', '2026-08-02T00:03:00Z', 'succeeded', 1, 1),
            ($3, 'scheduled', '2026-08-01T00:00:00Z', '2026-08-02T00:00:00Z', $4,
              'forecast/v1', NULL, '2026-08-02T00:00:00Z',
              '2026-08-02T00:05:00Z', '2026-08-02T00:01:00Z', 'succeeded', 1, 1)
          RETURNING id, source_id
        `,
        [
          historicalAnchorSource.id,
          historicalAnchorSource.source_config_fingerprint,
          forecastOnlySource.id,
          forecastOnlySource.source_config_fingerprint,
        ],
      );
      const historicalRunIds = anchorRuns.rows
        .filter(
          // retain only historical-source runs
          (run) => run.source_id === historicalAnchorSource.id,
        )
        .map(
          // retain stable run identities
          (run) => run.id,
        );
      const forecastOnlyRunId = anchorRuns.rows.find(
        // select the forecast-only run
        (run) => run.source_id === forecastOnlySource.id,
      )?.id;
      assert.equal(historicalRunIds.length, 2);
      assert.ok(forecastOnlyRunId);
      const anchorParameters = [
        historicalAnchorSource.id,
        historicalAnchorSource.source_config_fingerprint,
        "2026-08-01T12:00:00Z",
        24,
        "previous_runs",
        "best_match",
        "open-meteo-previous-runs-best-match/2026-09",
        "previous-runs/v1",
        historicalRunIds[0],
        historicalRunIds[0],
        "2026-08-02T00:00:00Z",
        "2026-08-02T00:00:00Z",
        12,
        "c".repeat(64),
        0,
      ];
      const insertedAnchor = await ingestPool.query(insertAnchorSql, anchorParameters);
      assert.equal(insertedAnchor.rowCount, 1);
      const anchorIdentity = await pool.query(
        `
          SELECT id
          FROM forecast_anchor_records
          WHERE source_id = $1
            AND valid_at = $2
            AND lead_hours = $3
        `,
        [historicalAnchorSource.id, anchorParameters[2], anchorParameters[3]],
      );
      assert.equal(anchorIdentity.rowCount, 1);
      const exactRetryParameters = [...anchorParameters];
      exactRetryParameters[9] = historicalRunIds[1];
      exactRetryParameters[11] = "2026-08-02T00:02:00Z";
      await ingestPool.query(upsertAnchorSql, exactRetryParameters);
      const changedContentParameters = [...exactRetryParameters];
      changedContentParameters[12] = 13;
      changedContentParameters[13] = "d".repeat(64);
      await ingestPool.query(upsertAnchorSql, changedContentParameters);
      const revisedAnchor = await pool.query(
        `
          SELECT
            first_ingestion_run_id,
            last_ingestion_run_id,
            revision_count,
            temperature_c
          FROM forecast_anchor_records
          WHERE id = $1
        `,
        [anchorIdentity.rows[0].id],
      );
      assert.deepEqual(revisedAnchor.rows[0], {
        first_ingestion_run_id: historicalRunIds[0],
        last_ingestion_run_id: historicalRunIds[1],
        revision_count: 1,
        temperature_c: 13,
      });
      await assert.rejects(
        pool.query(
          `
            UPDATE forecast_anchor_records
            SET content_hash = $1,
                revision_count = revision_count + 1
            WHERE id = $2
          `,
          ["e".repeat(64), anchorIdentity.rows[0].id],
        ),
        { code: "23514" },
      );
      await assert.rejects(
        pool.query(
          `
            UPDATE forecast_anchor_records
            SET temperature_c = 14,
                content_hash = $1
            WHERE id = $2
          `,
          ["e".repeat(64), anchorIdentity.rows[0].id],
        ),
        { code: "23514" },
      );
      const wrongDatasetParameters = [...anchorParameters];
      wrongDatasetParameters[2] = "2026-08-01T13:00:00Z";
      wrongDatasetParameters[4] = "forecast";
      await assert.rejects(
        ingestPool.query(insertAnchorSql, wrongDatasetParameters),
        { code: "23514" },
      );
      const wrongModelParameters = [...anchorParameters];
      wrongModelParameters[2] = "2026-08-01T14:00:00Z";
      wrongModelParameters[5] = "gfs_seamless";
      await assert.rejects(
        ingestPool.query(insertAnchorSql, wrongModelParameters),
        { code: "23514" },
      );
      const wrongRunParameters = [...anchorParameters];
      wrongRunParameters[2] = "2026-08-01T15:00:00Z";
      wrongRunParameters[8] = forecastOnlyRunId;
      wrongRunParameters[9] = forecastOnlyRunId;
      await assert.rejects(
        ingestPool.query(insertAnchorSql, wrongRunParameters),
        { code: "23503" },
      );
      await assert.rejects(
        pool.query(
          "UPDATE forecast_anchor_records SET contract_epoch = 'changed/v2' WHERE id = $1",
          [anchorIdentity.rows[0].id],
        ),
        { code: "23514" },
      );
      await assert.rejects(
        ingestPool.query(insertAnchorSql, [
          ...anchorParameters.slice(0, 3),
          25,
          ...anchorParameters.slice(4),
        ]),
        { code: "23514" },
      );
      await assert.rejects(
        ingestPool.query(insertAnchorSql, [
          forecastOnlySource.id,
          forecastOnlySource.source_config_fingerprint,
          "2026-08-01T12:00:00Z",
          24,
          "previous_runs",
          "best_match",
          "open-meteo-previous-runs-best-match/2026-09",
          "previous-runs/v1",
          forecastOnlyRunId,
          forecastOnlyRunId,
          "2026-08-02T00:00:00Z",
          "2026-08-02T00:00:00Z",
          12,
          "f".repeat(64),
          0,
        ]),
        { code: "23514" },
      );
      await assertPrivilegeDenied(apiPool.query("SELECT * FROM forecast_anchor_records"));
      await assertPrivilegeDenied(ingestPool.query("SELECT * FROM forecast_anchor_records"));
      assert.equal((await pool.query("SELECT * FROM forecast_anchor_records")).rowCount, 1);
      const tempestSuccessor = await pool.query(
        `
          SELECT s.station_id, s.provider_id
          FROM sources s
          WHERE s.source_key = 'tempest-203055-observations-v2'
        `,
      );
      const successor = tempestSuccessor.rows[0];
      assert.ok(successor);
      await pool.query(
        `
          INSERT INTO sources (
            station_id,
            provider_id,
            source_key,
            source_kind,
            material_provider_config,
            source_config_fingerprint,
            capabilities,
            cadence_seconds,
            active
          )
          VALUES ($1, $2, 'tempest-203055-observations-v1', 'physical_sensor',
            '{"contractVersion":"tempest-observations/v1"}'::jsonb,
            $3, '["current","historical"]'::jsonb, 3600, true)
        `,
        [successor.station_id, successor.provider_id, "f".repeat(64)],
      );
      const visibleTempestSources = await apiPool.query(
        `
          SELECT source_key
          FROM sources
          WHERE source_key LIKE 'tempest-203055-%'
            AND weather_source_is_current(id)
          ORDER BY source_key
        `,
      );
      assert.deepEqual(visibleTempestSources.rows, [
        { source_key: "tempest-203055-observations-v2" },
      ]);
      const ingestMigrations = await ingestPool.query(
        "SELECT name FROM schema_migrations ORDER BY name",
      );
      assert.deepEqual(ingestMigrations.rows, [
        { name: "0001_initial_weather.sql" },
        { name: "0002_worker_migration_readiness.sql" },
        { name: "0003_ecowitt_measurements.sql" },
        { name: "0004_tempest_metadata.sql" },
        { name: "0005_source_supersession.sql" },
        { name: "0006_station_coordinates.sql" },
        { name: "0007_tide_sources.sql" },
        { name: "0008_ecowitt_property_sensors.sql" },
        { name: "0009_forecast_anchor_records.sql" },
        { name: "0010_forecast_training_export.sql" },
        { name: "0011_forecast_runtime_provenance.sql" },
        { name: "0012_hide_archive_only_forecasts_from_live_reads.sql" },
        { name: "0013_ecmwf_temperature_canary.sql" },
        { name: "0014_rain_collection.sql" },
        { name: "0015_rain_station_access.sql" },
        { name: "0016_rain_adjustment.sql" },
        { name: "0017_adjustment_evaluation_export.sql" },
        { name: "0018_adjustment_maintenance_v2.sql" },
        { name: "0019_adjustment_maintenance_recurring.sql" },
        { name: "0020_adjustment_revision_frontier.sql" },
        { name: "0021_adjustment_rolling_registration.sql" },
        { name: "0022_forecast_product_catalog.sql" },
        { name: "0023_forecast_product_read_index.sql" },
      ]);
      // retain all normalized metric update grants
      const metricUpdatePrivileges = await ingestPool.query(
        `
          SELECT bool_and(has_column_privilege(
            'weather_ingest',
            'weather_records',
            column_name,
            'UPDATE'
          )) AS allowed
          FROM unnest(ARRAY[
            'black_globe_temperature_c',
            'pm25_micrograms_per_cubic_meter',
            'precipitation_rate_mm_per_hour',
            'soil_electrical_conductivity_us_cm',
            'soil_moisture_percent',
            'solar_radiation_wm2',
            'uv_index',
            'wet_bulb_globe_temperature_c'
          ]) AS metric_columns(column_name)
        `,
      );
      assert.equal(metricUpdatePrivileges.rows[0].allowed, true);
      await assertPrivilegeDenied(
        ingestPool.query(
          "INSERT INTO schema_migrations (name, checksum) VALUES ('forbidden.sql', repeat('0', 64))",
        ),
      );
      await assertPrivilegeDenied(
        ingestPool.query("UPDATE schema_migrations SET name = name WHERE false"),
      );
      await assertPrivilegeDenied(
        ingestPool.query("DELETE FROM schema_migrations WHERE false"),
      );
      await assertPrivilegeDenied(ingestPool.query("TRUNCATE schema_migrations"));
      await assertPrivilegeDenied(
        ingestPool.query("CREATE TABLE weather_ingest_acl_probe (id integer)"),
      );

      const exportRole = await pool.query(`
        SELECT
          rolcanlogin,
          rolcreatedb,
          rolcreaterole,
          rolinherit,
          rolreplication,
          rolsuper,
          rolbypassrls,
          rolconfig,
          (
            SELECT count(*)::integer
            FROM pg_auth_members
            WHERE member = role.oid
          ) AS membership_count,
          (
            SELECT count(*)::integer
            FROM pg_db_role_setting
            WHERE setrole = role.oid
              AND setdatabase <> 0
          ) AS database_setting_count
        FROM pg_roles role
        WHERE rolname = 'weather_training_export'
      `);
      assert.deepEqual(exportRole.rows[0], {
        database_setting_count: 0,
        membership_count: 0,
        rolbypassrls: false,
        rolcanlogin: true,
        rolconfig: ["default_transaction_read_only=on"],
        rolcreatedb: false,
        rolcreaterole: false,
        rolinherit: false,
        rolreplication: false,
        rolsuper: false,
      });
      assert.deepEqual(
        (await pool.query(exportAuthoritySnapshotSql)).rows[0],
        expectedExportAuthoritySnapshot,
      );
      assert.deepEqual(
        (await trainingExportPool.query("SHOW default_transaction_read_only")).rows,
        [{ default_transaction_read_only: "on" }],
      );
      await trainingExportPool.query("BEGIN ISOLATION LEVEL REPEATABLE READ");
      assert.deepEqual(
        (await trainingExportPool.query("SHOW transaction_read_only")).rows,
        [{ transaction_read_only: "on" }],
      );
      await trainingExportPool.query("ROLLBACK");
      const exportColumns = await pool.query(`
        SELECT column_name
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'forecast_training_export_rows_v1'
        ORDER BY ordinal_position
      `);
      assert.deepEqual(exportColumns.rows.map((column) => column.column_name), [
        "record_kind",
        "site_key",
        "physical_station_key",
        "provider_family",
        "source_keys",
        "source_config_fingerprints",
        "adapter_contracts",
        "valid_at",
        "reference_at",
        "received_at",
        "reference_kind",
        "target_lead_hours",
        "dataset",
        "upstream_model",
        "contract_epoch",
        "temperature_c",
        "relative_humidity_percent",
        "wind_speed_mps",
        "wind_gust_mps",
        "wind_direction_degrees",
        "ingestion_run_ids",
        "content_hashes",
        "exclusion_reason_codes",
        "collision_count",
      ]);
      const exportManifest = await trainingExportPool.query(
        "SELECT * FROM forecast_training_export_manifest_v1",
      );
      assert.deepEqual(
        {
          aggregation: exportManifest.rows[0].aggregation_contract_sha256,
          coordinates: exportManifest.rows[0].coordinate_manifest_sha256,
          metrics: exportManifest.rows[0].metric_eligibility_sha256,
          query: exportManifest.rows[0].query_contract_sha256,
          row: exportManifest.rows[0].row_schema_sha256,
          sources: exportManifest.rows[0].source_lineage_sha256,
          spatial: exportManifest.rows[0].spatial_weights_sha256,
          stations: exportManifest.rows[0].station_manifest_sha256,
        },
        {
          aggregation: "9c309ef5a00780167570746ad6c31b9128c266db50954fe4645287e1f2b31e64",
          coordinates: "04bfd93a03c393e977c8767a9aca6fe2a4cba9c263cb46e6987fa733b666ba58",
          metrics: "53731954b347836a26500b05a195ca15cf26214c4d561fe482c5ff87ef56a82e",
          query: "3b7926c47bbdb208ac2e305ee7798bfe4ea9590ce2863f556e752a71d1158e76",
          row: "2717b6c3c704a1b52c7748b59c37d635efd92d92efb9dc97ea4ddef97cd504fc",
          sources: "261a134589a12c1bbbd9a783343950317fd1fbc87e08383e60e805b7761566cc",
          spatial: "8ed5ce70d33edd4a5166049d9938cbaaf800151b6a0b3345d3005419e9041c74",
          stations: "a1f76440c056987bbb434d5315e4916f961deeb2951fe889d785943f559cdd49",
        },
      );
      await assert.rejects(
        trainingExportPool.query(
          "SELECT count(*) FROM forecast_training_export_rows_v1",
        ),
        { code: "22012" },
      );
      await setTrainingExportRange(
        trainingExportPool,
        "not-a-date",
        "2026-08-24",
      );
      await assert.rejects(
        trainingExportPool.query(
          "SELECT count(*) FROM forecast_training_export_rows_v1",
        ),
        { code: "22007" },
      );
      await setTrainingExportRange(
        trainingExportPool,
        "2026-8-24",
        "2026-08-24",
      );
      await assert.rejects(
        trainingExportPool.query(
          "SELECT count(*) FROM forecast_training_export_rows_v1",
        ),
        { code: "22012" },
      );
      await setTrainingExportRange(
        trainingExportPool,
        "2026-08-24",
        "2026-08-23",
      );
      await assert.rejects(
        trainingExportPool.query(
          "SELECT count(*) FROM forecast_training_export_rows_v1",
        ),
        { code: "22012" },
      );
      await setTrainingExportRange(
        trainingExportPool,
        "2025-01-01",
        "2026-03-27",
      );
      await assert.rejects(
        trainingExportPool.query(
          "SELECT count(*) FROM forecast_training_export_rows_v1",
        ),
        { code: "22012" },
      );
      await setTrainingExportRange(
        trainingExportPool,
        "2025-03-09",
        "2025-03-09",
      );
      assert.equal(
        Number((await trainingExportPool.query(`
          SELECT count(*) AS count
          FROM forecast_training_export_rows_v1
          WHERE record_kind = 'station_hour'
        `)).rows[0].count),
        11 * 23,
      );
      await setTrainingExportRange(
        trainingExportPool,
        "2025-11-02",
        "2025-11-02",
      );
      assert.equal(
        Number((await trainingExportPool.query(`
          SELECT count(*) AS count
          FROM forecast_training_export_rows_v1
          WHERE record_kind = 'station_hour'
        `)).rows[0].count),
        11 * 25,
      );
      await setTrainingExportRange(
        trainingExportPool,
        "2025-06-09",
        "2026-09-01",
      );
      assert.equal(
        Number((await trainingExportPool.query(`
          SELECT count(*) AS count
          FROM forecast_training_export_rows_v1
          WHERE record_kind = 'station_hour'
        `)).rows[0].count),
        118_800,
      );
      await setTrainingExportRange(
        trainingExportPool,
        "2021-01-01",
        "2021-01-01",
      );
      const gapRows = await trainingExportPool.query(`
        SELECT source_keys, ingestion_run_ids, content_hashes
        FROM forecast_training_export_rows_v1
        WHERE record_kind = 'station_hour'
          AND valid_at = TIMESTAMPTZ '2021-01-01 08:00:00+00'
        ORDER BY physical_station_key
      `);
      assert.equal(gapRows.rowCount, 11);
      assert.equal(
        gapRows.rows.every(
          // require provenance-free explicit gaps
          (row) =>
            row.source_keys.length === 0 &&
            row.ingestion_run_ids.length === 0 &&
            row.content_hashes.length === 0,
        ),
        true,
      );
      await pool.query(`
        INSERT INTO sources (
          station_id,
          provider_id,
          source_key,
          source_kind,
          material_provider_config,
          source_config_fingerprint,
          capabilities,
          cadence_seconds,
          active
        )
        SELECT
          source.station_id,
          source.provider_id,
          'tempest-38270-observations-v1',
          'physical_sensor',
          '{"contractVersion":"tempest-observations/v1"}'::jsonb,
          repeat('f', 64),
          '["current","historical"]'::jsonb,
          3600,
          true
        FROM sources source
        WHERE source.source_key = 'tempest-38270-observations-v2'
      `);
      const stationSources = await pool.query(`
        SELECT id, source_key, source_config_fingerprint
        FROM sources
        WHERE source_key = ANY(ARRAY[
          'ambient-maxweather-observations-v1',
          'wunderground-maxweather-history-v1',
          'tempest-38270-observations-v1',
          'tempest-38270-observations-v2'
        ])
        ORDER BY source_key
      `);
      assert.equal(stationSources.rowCount, 4);
      await pool.query(`
        SELECT setval(
          'ingestion_runs_id_seq',
          greatest((SELECT coalesce(max(id), 0) FROM ingestion_runs), 8),
          true
        )
      `);
      const stationRuns = await pool.query(
        `
          INSERT INTO ingestion_runs (
            source_id,
            mode,
            requested_start,
            requested_end_exclusive,
            source_config_fingerprint,
            adapter_version,
            started_at,
            deadline_at,
            completed_at,
            state,
            attempts,
            record_count
          )
          SELECT
            fixture.source_id,
            'scheduled',
            TIMESTAMPTZ '2026-08-23 00:00:00+00',
            TIMESTAMPTZ '2026-08-26 00:00:00+00',
            fixture.source_config_fingerprint,
            'forecast-training-export-fixture/v1',
            TIMESTAMPTZ '2026-08-26 00:00:00+00' + fixture.ordinality * INTERVAL '1 minute',
            TIMESTAMPTZ '2026-08-26 01:00:00+00' + fixture.ordinality * INTERVAL '1 minute',
            TIMESTAMPTZ '2026-08-26 00:30:00+00' + fixture.ordinality * INTERVAL '1 minute',
            'succeeded',
            1,
            1
          FROM unnest($1::bigint[], $2::text[]) WITH ORDINALITY
            AS fixture(source_id, source_config_fingerprint, ordinality)
          RETURNING id, source_id
        `,
        [
          stationSources.rows.map(
            // preserve ordered source identities
            (source) => source.id,
          ),
          stationSources.rows.map(
            // preserve ordered source fingerprints
            (source) => source.source_config_fingerprint,
          ),
        ],
      );
      const runBySourceId = new Map(
        stationRuns.rows.map(
          // index runs by stable source identity
          (run) => [String(run.source_id), run.id],
        ),
      );
      const ambientSource = stationSources.rows.find(
        // select the Ambient fixture source
        (source) => source.source_key === "ambient-maxweather-observations-v1",
      );
      const wundergroundSource = stationSources.rows.find(
        // select the Weather Underground fixture source
        (source) => source.source_key === "wunderground-maxweather-history-v1",
      );
      const tempestV1Source = stationSources.rows.find(
        // select the excluded Tempest fixture source
        (source) => source.source_key === "tempest-38270-observations-v1",
      );
      const tempestV2Source = stationSources.rows.find(
        // select the accepted Tempest fixture source
        (source) => source.source_key === "tempest-38270-observations-v2",
      );
      assert.ok(ambientSource && wundergroundSource && tempestV1Source && tempestV2Source);
      const ambientRunId = runBySourceId.get(String(ambientSource.id));
      const wundergroundRunId = runBySourceId.get(String(wundergroundSource.id));
      const tempestV1RunId = runBySourceId.get(String(tempestV1Source.id));
      const tempestV2RunId = runBySourceId.get(String(tempestV2Source.id));
      assert.ok(ambientRunId && wundergroundRunId && tempestV1RunId && tempestV2RunId);
      const stationRecordFixtures = [
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T00:55:00Z", temperatureC: 30 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T00:58:00Z", temperatureC: 10, relativeHumidityPercent: 80, windSpeedMps: 0.999, windDirectionDegrees: 90 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T01:01:00Z", temperatureC: 69, qualityMetadata: { flags: ["unknown_flag"] } },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T01:02:00Z", temperatureC: 20, relativeHumidityPercent: 70, windSpeedMps: 2, windDirectionDegrees: 200 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T01:05:00Z", temperatureC: 60 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T00:10:00Z", windGustMps: 2 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T00:20:00Z", windGustMps: 3 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T00:30:00Z", windGustMps: 4 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T00:40:00Z", windGustMps: 5 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T00:50:00Z", windGustMps: 6 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T01:00:00Z", windGustMps: 7 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T02:55:00Z", temperatureC: 15 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T03:05:00Z", temperatureC: 16 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T03:00:00Z", windGustMps: 99 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T03:10:00Z", windGustMps: 4 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T03:21:00Z", windGustMps: 5 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-25T04:00:00Z", windGustMps: 6 },
        { sourceId: ambientSource.id, runId: ambientRunId, validAt: "2026-08-24T00:00:00Z", temperatureC: 5 },
        { sourceId: wundergroundSource.id, runId: wundergroundRunId, validAt: "2026-08-23T23:10:00Z", windGustMps: 2, qualityMetadata: { status: "provider_qc_1" } },
        { sourceId: wundergroundSource.id, runId: wundergroundRunId, validAt: "2026-08-23T23:20:00Z", windGustMps: 3, qualityMetadata: { status: "provider_qc_1" } },
        { sourceId: wundergroundSource.id, runId: wundergroundRunId, validAt: "2026-08-23T23:30:00Z", windGustMps: 4, qualityMetadata: { status: "provider_qc_1" } },
        { sourceId: wundergroundSource.id, runId: wundergroundRunId, validAt: "2026-08-23T23:40:00Z", windGustMps: 5, qualityMetadata: { status: "provider_qc_1" } },
        { sourceId: wundergroundSource.id, runId: wundergroundRunId, validAt: "2026-08-23T23:50:00Z", windGustMps: 6, qualityMetadata: { status: "provider_qc_1" } },
        { sourceId: wundergroundSource.id, runId: wundergroundRunId, validAt: "2026-08-23T23:59:00Z", temperatureC: 69, qualityMetadata: { status: "provider_qc_2" } },
        { sourceId: wundergroundSource.id, runId: wundergroundRunId, validAt: "2026-08-24T00:00:00Z", temperatureC: 68, qualityMetadata: { status: "provider_qc_1" } },
        { sourceId: tempestV1Source.id, runId: tempestV1RunId, validAt: "2026-08-25T01:00:00Z", temperatureC: 69 },
        { sourceId: tempestV2Source.id, runId: tempestV2RunId, validAt: "2026-08-25T01:00:00Z", temperatureC: 11, qualityMetadata: { flags: ["uv_index_out_of_range"] } },
      ];
      await pool.query(
        `
          INSERT INTO weather_records (
            source_id,
            source_kind,
            valid_at,
            product_run_at,
            first_ingestion_run_id,
            last_ingestion_run_id,
            first_received_at,
            last_received_at,
            upstream_timezone,
            device_serial,
            quality_metadata,
            provider_metadata,
            temperature_c,
            wind_speed_mps,
            wind_gust_mps,
            relative_humidity_percent,
            wind_direction_degrees,
            content_hash
          )
          SELECT
            fixture."sourceId",
            'physical_sensor',
            fixture."validAt",
            NULL,
            fixture."runId",
            fixture."runId",
            fixture."validAt" + INTERVAL '1 minute',
            fixture."validAt" + INTERVAL '1 minute',
            'UTC',
            'serial-lan-property-password-poison',
            fixture."qualityMetadata",
            '{"request_id":"credential-poison"}'::jsonb,
            fixture."temperatureC",
            fixture."windSpeedMps",
            fixture."windGustMps",
            fixture."relativeHumidityPercent",
            fixture."windDirectionDegrees",
            repeat('a', 64)
          FROM jsonb_to_recordset($1::jsonb) AS fixture(
            "sourceId" bigint,
            "runId" bigint,
            "validAt" timestamptz,
            "qualityMetadata" jsonb,
            "temperatureC" double precision,
            "windSpeedMps" double precision,
            "windGustMps" double precision,
            "relativeHumidityPercent" double precision,
            "windDirectionDegrees" double precision
          )
        `,
        [JSON.stringify(stationRecordFixtures)],
      );
      await setTrainingExportRange(
        trainingExportPool,
        "2026-08-23",
        "2026-08-26",
      );
      const sampledStation = await trainingExportPool.query(`
        SELECT *
        FROM forecast_training_export_rows_v1
        WHERE record_kind = 'station_hour'
          AND physical_station_key = 'ambient-maxweather'
          AND valid_at = TIMESTAMPTZ '2026-08-25 01:00:00+00'
      `);
      assert.deepEqual(
        {
          direction: sampledStation.rows[0].wind_direction_degrees,
          gust: sampledStation.rows[0].wind_gust_mps,
          humidity: sampledStation.rows[0].relative_humidity_percent,
          ingestionRunIds: sampledStation.rows[0].ingestion_run_ids,
          reasons: sampledStation.rows[0].exclusion_reason_codes,
          speed: sampledStation.rows[0].wind_speed_mps,
          temperature: sampledStation.rows[0].temperature_c,
        },
        {
          direction: 200,
          gust: 7,
          humidity: 80,
          ingestionRunIds: [String(ambientRunId)],
          reasons: ["quality_flag_rejected", "station_direction_calm"],
          speed: 0.999,
          temperature: 10,
        },
      );
      assert.doesNotMatch(
        JSON.stringify(sampledStation.rows[0]),
        /serial-lan-property-password-poison|credential-poison/u,
      );
      const boundaryStation = await trainingExportPool.query(`
        SELECT temperature_c
        FROM forecast_training_export_rows_v1
        WHERE record_kind = 'station_hour'
          AND physical_station_key = 'ambient-maxweather'
          AND valid_at = TIMESTAMPTZ '2026-08-25 03:00:00+00'
      `);
      assert.equal(boundaryStation.rows[0].temperature_c, 15);
      const incompleteGust = await trainingExportPool.query(`
        SELECT wind_gust_mps, exclusion_reason_codes
        FROM forecast_training_export_rows_v1
        WHERE record_kind = 'station_hour'
          AND physical_station_key = 'ambient-maxweather'
          AND valid_at = TIMESTAMPTZ '2026-08-25 04:00:00+00'
      `);
      assert.equal(incompleteGust.rows[0].wind_gust_mps, null);
      assert.equal(
        incompleteGust.rows[0].exclusion_reason_codes.includes(
          "station_gust_coverage_incomplete",
        ),
        true,
      );
      const cutoverStation = await trainingExportPool.query(`
        SELECT
          source_keys,
          source_config_fingerprints,
          adapter_contracts,
          temperature_c,
          wind_gust_mps,
          exclusion_reason_codes
        FROM forecast_training_export_rows_v1
        WHERE record_kind = 'station_hour'
          AND physical_station_key = 'ambient-maxweather'
          AND valid_at = TIMESTAMPTZ '2026-08-24 00:00:00+00'
      `);
      assert.deepEqual(cutoverStation.rows[0].source_keys, [
        "ambient-maxweather-observations-v1",
        "wunderground-maxweather-history-v1",
      ]);
      assert.deepEqual(cutoverStation.rows[0].source_config_fingerprints, [
        "7a7528a6278924ca5280a1a6045b6647b7e660b112d7fa3008c542a17ff99df4",
        "52dda6c5444d0a234fbe23d6218027d417ac966ecf291a7d5dfff42fd0dc207c",
      ]);
      assert.deepEqual(cutoverStation.rows[0].adapter_contracts, [
        "ambient-device-data/v1",
        "wunderground-pws-history/v1",
      ]);
      assert.equal(cutoverStation.rows[0].temperature_c, 5);
      assert.equal(cutoverStation.rows[0].wind_gust_mps, 6);
      assert.deepEqual(cutoverStation.rows[0].exclusion_reason_codes, [
        "metric_missing",
        "quality_status_rejected",
        "source_interval_out_of_range",
      ]);
      const tempestStation = await trainingExportPool.query(`
        SELECT temperature_c, source_keys, exclusion_reason_codes
        FROM forecast_training_export_rows_v1
        WHERE record_kind = 'station_hour'
          AND physical_station_key = 'tempest-38270'
          AND valid_at = TIMESTAMPTZ '2026-08-25 01:00:00+00'
      `);
      assert.equal(tempestStation.rows[0].temperature_c, 11);
      assert.deepEqual(tempestStation.rows[0].source_keys, [
        "tempest-38270-observations-v2",
      ]);
      assert.deepEqual(tempestStation.rows[0].exclusion_reason_codes, [
        "metric_missing",
        "source_superseded",
      ]);
      const ambientSourceMaterial = await pool.query(`
        SELECT material_provider_config, source_config_fingerprint::text
        FROM sources
        WHERE id = ${String(ambientSource.id)}
      `);
      await pool.query(`
        ALTER TABLE sources DISABLE TRIGGER sources_material_immutable;
        UPDATE sources
        SET material_provider_config = material_provider_config - 'contractVersion'
        WHERE id = ${String(ambientSource.id)};
        ALTER TABLE sources ENABLE TRIGGER sources_material_immutable;
      `);
      const missingAdapterStation = await trainingExportPool.query(`
        SELECT temperature_c, exclusion_reason_codes
        FROM forecast_training_export_rows_v1
        WHERE record_kind = 'station_hour'
          AND physical_station_key = 'ambient-maxweather'
          AND valid_at = TIMESTAMPTZ '2026-08-25 01:00:00+00'
      `);
      assert.equal(missingAdapterStation.rows[0].temperature_c, null);
      assert.equal(
        missingAdapterStation.rows[0].exclusion_reason_codes.includes(
          "source_superseded",
        ),
        true,
      );
      await pool.query(
        "ALTER TABLE sources DISABLE TRIGGER sources_material_immutable",
      );
      await pool.query(
        `
          UPDATE sources
          SET material_provider_config = $1::jsonb
          WHERE id = $2
        `,
        [ambientSourceMaterial.rows[0].material_provider_config, ambientSource.id],
      );
      await pool.query(
        "ALTER TABLE sources ENABLE TRIGGER sources_material_immutable",
      );
      await adminPool.query(`
        ALTER TABLE sources DISABLE TRIGGER ALL;
        ALTER TABLE ingestion_runs DISABLE TRIGGER ALL;
        UPDATE ingestion_runs
        SET source_config_fingerprint = repeat('e', 64)
        WHERE source_id = ${String(ambientSource.id)};
        UPDATE sources
        SET source_config_fingerprint = repeat('e', 64)
        WHERE id = ${String(ambientSource.id)};
        ALTER TABLE ingestion_runs ENABLE TRIGGER ALL;
        ALTER TABLE sources ENABLE TRIGGER ALL;
      `);
      const wrongFingerprintStation = await trainingExportPool.query(`
        SELECT temperature_c, exclusion_reason_codes
        FROM forecast_training_export_rows_v1
        WHERE record_kind = 'station_hour'
          AND physical_station_key = 'ambient-maxweather'
          AND valid_at = TIMESTAMPTZ '2026-08-25 01:00:00+00'
      `);
      assert.equal(wrongFingerprintStation.rows[0].temperature_c, null);
      assert.equal(
        wrongFingerprintStation.rows[0].exclusion_reason_codes.includes(
          "source_superseded",
        ),
        true,
      );
      await adminPool.query(`
        ALTER TABLE sources DISABLE TRIGGER ALL;
        ALTER TABLE ingestion_runs DISABLE TRIGGER ALL;
        UPDATE sources
        SET source_config_fingerprint = '${ambientSourceMaterial.rows[0].source_config_fingerprint}'
        WHERE id = ${String(ambientSource.id)};
        UPDATE ingestion_runs
        SET source_config_fingerprint = '${ambientSourceMaterial.rows[0].source_config_fingerprint}'
        WHERE source_id = ${String(ambientSource.id)};
        ALTER TABLE ingestion_runs ENABLE TRIGGER ALL;
        ALTER TABLE sources ENABLE TRIGGER ALL;
      `);
      for (const deniedQuery of [
        "SELECT * FROM sites",
        "SELECT * FROM sources",
        "SELECT * FROM weather_records",
        "SELECT * FROM forecast_anchor_records",
        "SELECT nextval('weather_records_id_seq')",
        "SELECT weather_source_is_current(1)",
        "CREATE TABLE export_acl_probe (id integer)",
        "SET ROLE weather_owner",
      ]) {
        // deny every non-projection authority
        await assert.rejects(
          trainingExportPool.query(deniedQuery),
          (error) => error?.code === "42501" || error?.code === "25006",
        );
      }
      const exportAclClient = await trainingExportPool.connect();
      try {
        // disable the safety default
        await exportAclClient.query("SET default_transaction_read_only = off");
        for (const deniedMutation of [
          "INSERT INTO sites (slug, display_name, timezone) VALUES ('export-denied', 'denied', 'UTC')",
          "UPDATE sites SET display_name = display_name WHERE false",
          "DELETE FROM sites WHERE false",
          "TRUNCATE sites",
        ]) {
          // prove ACL denial directly
          await assertPrivilegeDenied(exportAclClient.query(deniedMutation));
        }
      } finally {
        // release the isolated session
        exportAclClient.release();
      }

      temporaryAclClient = await adminPool.connect();
      await temporaryAclClient.query(`
        CREATE FUNCTION pg_temp.weather_temp_acl_probe() RETURNS integer
        LANGUAGE sql SECURITY DEFINER AS 'SELECT 1'
      `);
      const temporaryAclBefore = await temporaryAclClient.query(`
        SELECT procedure.proacl
        FROM pg_proc procedure
        WHERE procedure.oid = 'pg_temp.weather_temp_acl_probe()'::regprocedure
      `);
      const systemAclBefore = await pool.query(`
        SELECT namespace.nspname, procedure.oid::regprocedure::text AS identity, procedure.proacl
        FROM pg_proc procedure
        JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
        WHERE namespace.nspname IN ('pg_catalog', 'information_schema', 'pg_toast')
        ORDER BY namespace.nspname, identity
      `);
      await adminPool.query(`
        CREATE ROLE weather_export_extra NOLOGIN;
        GRANT weather_export_extra TO weather_training_export;
        ALTER ROLE weather_training_export SET search_path = pg_catalog;
        ALTER ROLE weather_training_export IN DATABASE weather_test
          SET default_transaction_read_only = off;
        ALTER ROLE weather_training_export IN DATABASE weather_test
          SET search_path = pg_catalog;
      `);
      await pool.query(`
        CREATE FUNCTION weather_export_acl_probe() RETURNS integer
        LANGUAGE sql SECURITY DEFINER AS 'SELECT 1';
        GRANT EXECUTE ON FUNCTION weather_export_acl_probe()
        TO weather_api, weather_ingest, weather_training_export;
      `);
      await adminPool.query(`
        CREATE FUNCTION weather_foreign_invoker_probe() RETURNS integer
        LANGUAGE sql SECURITY INVOKER AS 'SELECT 1';
        CREATE SCHEMA weather_foreign_acl AUTHORIZATION postgres;
        REVOKE ALL ON SCHEMA weather_foreign_acl FROM PUBLIC;
        GRANT USAGE ON SCHEMA weather_foreign_acl TO weather_api;
        CREATE FUNCTION weather_foreign_acl.split_reachability_probe() RETURNS integer
        LANGUAGE sql SECURITY DEFINER AS 'SELECT 1';
        REVOKE ALL ON FUNCTION weather_foreign_acl.split_reachability_probe() FROM PUBLIC;
        GRANT EXECUTE ON FUNCTION weather_foreign_acl.split_reachability_probe()
        TO weather_ingest;
        CREATE SCHEMA weather_public_reachable AUTHORIZATION postgres;
        GRANT USAGE ON SCHEMA weather_public_reachable TO PUBLIC;
        CREATE FUNCTION weather_public_reachable.probe() RETURNS integer
        LANGUAGE sql SECURITY DEFINER AS 'SELECT 1';
        CREATE FUNCTION weather_public_reachable.invoker_probe() RETURNS integer
        LANGUAGE sql SECURITY INVOKER AS 'SELECT 42';
        CREATE SCHEMA weather_api_reachable AUTHORIZATION postgres;
        REVOKE ALL ON SCHEMA weather_api_reachable FROM PUBLIC;
        GRANT USAGE ON SCHEMA weather_api_reachable TO weather_api;
        CREATE FUNCTION weather_api_reachable.probe() RETURNS integer
        LANGUAGE sql SECURITY DEFINER AS 'SELECT 1';
        REVOKE ALL ON FUNCTION weather_api_reachable.probe() FROM PUBLIC;
        GRANT EXECUTE ON FUNCTION weather_api_reachable.probe() TO weather_api;
        CREATE SCHEMA weather_ingest_reachable AUTHORIZATION postgres;
        REVOKE ALL ON SCHEMA weather_ingest_reachable FROM PUBLIC;
        GRANT USAGE ON SCHEMA weather_ingest_reachable TO weather_ingest;
        CREATE FUNCTION weather_ingest_reachable.probe() RETURNS integer
        LANGUAGE sql SECURITY DEFINER AS 'SELECT 1';
        REVOKE ALL ON FUNCTION weather_ingest_reachable.probe() FROM PUBLIC;
        GRANT EXECUTE ON FUNCTION weather_ingest_reachable.probe() TO weather_ingest;
        CREATE SCHEMA weather_export_reachable AUTHORIZATION postgres;
        REVOKE ALL ON SCHEMA weather_export_reachable FROM PUBLIC;
        GRANT USAGE ON SCHEMA weather_export_reachable TO weather_training_export;
        CREATE FUNCTION weather_export_reachable.probe() RETURNS integer
        LANGUAGE sql SECURITY DEFINER AS 'SELECT 1';
        REVOKE ALL ON FUNCTION weather_export_reachable.probe() FROM PUBLIC;
        GRANT EXECUTE ON FUNCTION weather_export_reachable.probe()
        TO weather_training_export;
      `);
      assert.deepEqual(
        (
          await trainingExportPool.query(
            "SELECT weather_public_reachable.invoker_probe() AS value",
          )
        ).rows,
        [{ value: 42 }],
      );
      await executeFile(
        "psql",
        [
          "--set=ON_ERROR_STOP=1",
          "--host",
          server.host,
          "--port",
          String(server.port),
          "--username",
          server.user,
          "--dbname",
          "weather_test",
          "--file",
          runtimeAclPath,
        ],
        {
          env: { ...process.env, PGPASSWORD: server.password },
          timeout: 30_000,
        },
      );
      const convergedAcl = await adminPool.query(`
        SELECT
          NOT has_function_privilege(
            'weather_api', 'weather_export_acl_probe()', 'EXECUTE'
          ) AS api_extra_revoked,
          NOT has_function_privilege(
            'weather_ingest', 'weather_export_acl_probe()', 'EXECUTE'
          ) AS ingest_extra_revoked,
          NOT has_function_privilege(
            'weather_training_export', 'weather_export_acl_probe()', 'EXECUTE'
          ) AS export_extra_revoked,
          has_function_privilege(
            'weather_ingest',
            'weather_foreign_acl.split_reachability_probe()',
            'EXECUTE'
          ) AS split_reachability_unchanged,
          NOT has_function_privilege(
            'weather_api', 'weather_public_reachable.probe()', 'EXECUTE'
          ) AS public_reachable_revoked,
          NOT has_function_privilege(
            'weather_api', 'weather_api_reachable.probe()', 'EXECUTE'
          ) AS api_reachable_revoked,
          NOT has_function_privilege(
            'weather_ingest', 'weather_ingest_reachable.probe()', 'EXECUTE'
          ) AS ingest_reachable_revoked,
          NOT has_function_privilege(
            'weather_training_export', 'weather_export_reachable.probe()', 'EXECUTE'
          ) AS export_reachable_revoked,
          has_function_privilege(
            'weather_api', 'weather_source_is_current(bigint)', 'EXECUTE'
          ) AS api_intended_retained,
          has_function_privilege(
            'weather_ingest', 'weather_json_object_keys_allowed(jsonb,text[])', 'EXECUTE'
          ) AS ingest_intended_retained,
          has_function_privilege(
            'weather_training_export', 'weather_foreign_invoker_probe()', 'EXECUTE'
          ) AS foreign_invoker_unchanged,
          (SELECT rolconfig FROM pg_roles WHERE rolname = 'weather_training_export')
            = ARRAY['default_transaction_read_only=on'] AS settings_reset,
          NOT EXISTS (
            SELECT 1 FROM pg_db_role_setting
            WHERE setrole = 'weather_training_export'::regrole
              AND setdatabase <> 0
          ) AS database_settings_reset,
          NOT EXISTS (
            SELECT 1 FROM pg_auth_members
            WHERE member = 'weather_training_export'::regrole
          ) AS memberships_reset
      `);
      assert.deepEqual(convergedAcl.rows[0], {
        api_extra_revoked: true,
        api_intended_retained: true,
        api_reachable_revoked: true,
        database_settings_reset: true,
        export_extra_revoked: true,
        export_reachable_revoked: true,
        foreign_invoker_unchanged: true,
        ingest_extra_revoked: true,
        ingest_intended_retained: true,
        ingest_reachable_revoked: true,
        memberships_reset: true,
        public_reachable_revoked: true,
        settings_reset: true,
        split_reachability_unchanged: true,
      });
      assert.deepEqual(
        (
          await trainingExportPool.query(
            "SELECT weather_public_reachable.invoker_probe() AS value",
          )
        ).rows,
        [{ value: 42 }],
      );
      await assertPrivilegeDenied(
        trainingExportPool.query("SELECT weather_public_reachable.probe()"),
      );
      assert.deepEqual(
        (await adminPool.query(exportAuthoritySnapshotSql)).rows[0],
        {
          ...expectedExportAuthoritySnapshot,
          schema_privileges: [
            "public:USAGE",
            "weather_public_reachable:USAGE",
          ],
        },
      );
      reconciledTrainingExportPool = createTestPool(
        server,
        "weather_test",
        "weather_training_export",
        "training-export-test",
      );
      assert.deepEqual(
        (
          await reconciledTrainingExportPool.query(
            "SHOW default_transaction_read_only",
          )
        ).rows,
        [{ default_transaction_read_only: "on" }],
      );
      const systemAclAfter = await adminPool.query(`
        SELECT namespace.nspname, procedure.oid::regprocedure::text AS identity, procedure.proacl
        FROM pg_proc procedure
        JOIN pg_namespace namespace ON namespace.oid = procedure.pronamespace
        WHERE namespace.nspname IN ('pg_catalog', 'information_schema', 'pg_toast')
        ORDER BY namespace.nspname, identity
      `);
      assert.deepEqual(systemAclAfter.rows, systemAclBefore.rows);
      const temporaryAclAfter = await temporaryAclClient.query(`
        SELECT procedure.proacl
        FROM pg_proc procedure
        WHERE procedure.oid = 'pg_temp.weather_temp_acl_probe()'::regprocedure
      `);
      assert.deepEqual(temporaryAclAfter.rows, temporaryAclBefore.rows);
    } finally {
      // clean disposable resources
      migrationLockClient?.release();
      temporaryAclClient?.release();
      await apiPool?.end();
      await ingestPool?.end();
      await trainingExportPool?.end();
      await reconciledTrainingExportPool?.end();
      await pool?.end();
      await adminPool?.end();
      // stop only a started container
      if (server !== undefined) {
        await stopPostgres(server);
      }
      await rm(directory, { force: true, recursive: true });
    }
  },
);
