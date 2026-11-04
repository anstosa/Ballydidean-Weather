import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmod,
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  unlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { createHash } from "node:crypto";
import { gzipSync, gunzipSync } from "node:zlib";
import {
  assertExportRowCount,
  assertLocalReadOnlyTarget,
} from "../scripts/forecast-training-package.mjs";
import {
  createTestPool,
  startPostgres,
  stopPostgres,
} from "../../packages/database/test/postgres-harness.mjs";

const repoRoot = resolve(import.meta.dirname, "../..");
const helper = join(repoRoot, "deploy/scripts/forecast-training-package.mjs");
const exportScript = join(repoRoot, "deploy/scripts/forecast-training-export.sh");
const pullScript = join(repoRoot, "deploy/scripts/pull-forecast-training-export.sh");
const sshRunScript = join(repoRoot, "deploy/scripts/ssh-run.sh");
const runDeployIntegration = process.env.WEATHER_RUN_DEPLOY_INTEGRATION === "1";
const publicValidatorSha256 = "109a4d2be56bb8af4c5bf267ec4df1ba7f8f2b4b2d47f38fa443c7487fd50a87";
const publicValidatorForbiddenValueHit = String.raw`apps/web/src/adjustment-maintenance-contract.mjs:  /(?:^|[/\\])\.weather[/\\]model-evidence(?:[/\\]|$)/iu,` + "\n";

// permit only the hash-bound validator's exact private-value rejection rule
function assertNoApplicationExportAccess(result, surface, validatorSha256) {
  assert.ok(result.status === 0 || result.status === 1, `${surface} export boundary scan failed`);
  assert.equal(result.stderr, "", `${surface} export boundary scan produced a diagnostic`);

  // retain the complete public boundary even when the private-value rule disappears
  if (surface === "apps/web") {
    assert.equal(validatorSha256, publicValidatorSha256, "public validator bytes changed");
  }

  // an empty successful scan retains the original no-access boundary
  if (result.status === 1) {
    assert.equal(result.stdout, "");
    return;
  }
  assert.equal(surface, "apps/web", `${surface} gained export evidence or key access`);
  assert.equal(result.stdout, publicValidatorForbiddenValueHit, "apps/web gained export evidence or key access");
}

// reject every access, extra hit, or drift around the one public denial rule
test("application export boundary admits only the exact hash-bound denial rule", () => {
  const allowed = { status: 0, stderr: "", stdout: publicValidatorForbiddenValueHit };
  assertNoApplicationExportAccess(allowed, "apps/web", publicValidatorSha256);
  assertNoApplicationExportAccess({ status: 1, stderr: "", stdout: "" }, "apps/api");
  const rejected = [
    { ...allowed, stdout: `${allowed.stdout}${allowed.stdout}` },
    { ...allowed, stdout: `${allowed.stdout}apps/web/src/index.ts:readFile('model-evidence')\n` },
    { ...allowed, stdout: "apps/web/src/index.ts:weather_training_export_password\n" },
    { ...allowed, stdout: allowed.stdout.replace("/iu,", "/iu; readFile('model-evidence');") },
    { ...allowed, stdout: allowed.stdout.replace("adjustment-maintenance-contract.mjs", "index.ts") },
    { ...allowed, status: 2, stderr: "fatal: scan unavailable" },
  ];

  // preserve the rejection of each hostile scanner result
  for (const result of rejected) {
    assert.throws(() => assertNoApplicationExportAccess(result, "apps/web", publicValidatorSha256));
  }
  assert.throws(() => assertNoApplicationExportAccess(allowed, "apps/api", publicValidatorSha256));
  assert.throws(() => assertNoApplicationExportAccess(allowed, "apps/web", "0".repeat(64)));
  assert.throws(() => assertNoApplicationExportAccess(
    { status: 1, stderr: "", stdout: "" }, "apps/web", "0".repeat(64),
  ));
});

const hashA = "a".repeat(64);
const hashB = "b".repeat(64);
const ambientFingerprint =
  "7a7528a6278924ca5280a1a6045b6647b7e660b112d7fa3008c542a17ff99df4";
const wundergroundFingerprint =
  "52dda6c5444d0a234fbe23d6218027d417ac966ecf291a7d5dfff42fd0dc207c";
const tempest225947Fingerprint =
  "b4dd6105d9a56a7c5d0dc4063f830e1cf28d693222a8de15536dd83d3a6178c4";
const legacyV4AdapterVersion = "open-meteo-forecast-daily/v4";
const legacyV4Fingerprint =
  "ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4";
const legacyV4ContractEpoch =
  "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83";
const stationDiagnosticReasons = [
  "metric_ineligible",
  "metric_missing",
  "quality_flag_rejected",
  "quality_status_rejected",
  "source_interval_out_of_range",
  "source_superseded",
  "station_direction_calm",
  "station_gust_coverage_incomplete",
];
const fixedHashes = {
  aggregation: "9c309ef5a00780167570746ad6c31b9128c266db50954fe4645287e1f2b31e64",
  coordinate: "04bfd93a03c393e977c8767a9aca6fe2a4cba9c263cb46e6987fa733b666ba58",
  metric: "53731954b347836a26500b05a195ca15cf26214c4d561fe482c5ff87ef56a82e",
  query: "3b7926c47bbdb208ac2e305ee7798bfe4ea9590ce2863f556e752a71d1158e76",
  row: "2717b6c3c704a1b52c7748b59c37d635efd92d92efb9dc97ea4ddef97cd504fc",
  source: "261a134589a12c1bbbd9a783343950317fd1fbc87e08383e60e805b7761566cc",
  spatial: "8ed5ce70d33edd4a5166049d9938cbaaf800151b6a0b3345d3005419e9041c74",
  station: "a1f76440c056987bbb434d5315e4916f961deeb2951fe889d785943f559cdd49",
};
const migrationNames = [
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
];
const migrationChecksums = [
  "f4264482606a3476b4b54e3089e66c900f0ad5104ea678a9bf17e5e5ba385a62",
  "954731c9ddb791c85defc3bffd887a3d30e9ff9fb5c9dac96e050ac52a796f03",
  "250e9b49176fda732b23f797a6dbce6878ca93818fa1bf6a408b5d6611ee5a48",
  "5487c90a4e70f8e049f9ed48c3ee3f025eb9f1908ed04a1febfb736c22f30d34",
  "462be10d1a35419b52068d09ae5254134e0168fed0ada66f957c87d290c8e6f1",
  "33f3fc5b1b475add0e862b45488a4a3482435df0ae674599005bafd37c3886a8",
  "4e2ed3037ea82011e947e48102149a6ea6456e5fb9f504436f7ec35f6e191390",
  "3934568dcdbb840f5026a09bab676bede7ad7d2571c1d7415efdabb902dd8b81",
  "e144415b6ac8e19338cc4b53d7daeffbf3d32650af880ab7acb809c6f71ded20",
  "2590423b7787b0e5e5668bfa4af21db775b8926540b1d2e339d6656aa3f27dc8",
  "ebf5789636095982e6010a868cdfdd4d7449ca79fb1057adf3b8dd8f49a337dc",
  "663ed323104bb70f4d0e735c3b546fb111038be4d0fee83e0c76c018cf0da15c",
  "e05f0397529108641ad9b9eb5381ac968f35449514fe4d8ed0ac502c9d5bf3ee",
];
const migrationHistory = createHash("sha256")
  .update(migrationNames.map((name, index) =>
    `${name}:${migrationChecksums[index]}`).join("\n"))
  .digest("hex");
const rainCollectionMigration = {
  checksum: "2a311e2effcc975442c1011c627c125521cc4396323b32f0d0c4f92ea1ccba04",
  name: "0014_rain_collection.sql",
};
const rainStationAccessMigration = {
  checksum: "2f560ccec001246a3caa5d27476900b2694dfb7cc0701b35181265ef1fb78822",
  name: "0015_rain_station_access.sql",
};
const rainAdjustmentMigration = {
  checksum: "9fa5659c032dc21fdf82dca693fa962d8b5212d1192ae35c4cfd04bef94be5f9",
  name: "0016_rain_adjustment.sql",
};
const adjustmentEvaluationExportMigration = {
  checksum: "6f18210453a18f0deefe95a0b70657cc4c4dd3fc20d0bab72f847ee04958ab90",
  name: "0017_adjustment_evaluation_export.sql",
};
const stationProviders = new Map([
  ["ambient-maxweather", "ambient"],
  ["ambient-merlin", "ambient"],
  ["ballydidean-ecowitt", "ecowitt"],
  ["netatmo-nearby", "netatmo"],
  ["tempest-126537", "tempest"],
  ["tempest-168853", "tempest"],
  ["tempest-201058", "tempest"],
  ["tempest-203055", "tempest"],
  ["tempest-225947", "tempest"],
  ["tempest-38270", "tempest"],
  ["tempest-64255", "tempest"],
]);
const localFormatter = new Intl.DateTimeFormat("en-CA", {
  day: "2-digit",
  month: "2-digit",
  timeZone: "America/Los_Angeles",
  year: "numeric",
});

// normalize canonical JSON
function canonicalize(value) {
  // preserve arrays
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }

  // sort objects
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonicalize(value[key])]),
    );
  }

  return value;
}

// encode canonical JSON
function canonicalJson(value) {
  return `${JSON.stringify(canonicalize(value))}\n`;
}

// generate the reviewed 450-date capacity shape
function generateConservativeExportShape() {
  let retrievalRows = 0;
  let stationRows = 0;
  let anchorRows = 0;

  // count every maximum date partition
  for (let date = 0; date < 450; date += 1) {
    // count 24 retrievals across 264 horizons
    for (let retrieval = 0; retrieval < 24; retrieval += 1) {
      for (let horizon = 0; horizon < 264; horizon += 1) {
        retrievalRows += 1;
      }
    }

    // count 11 stations across 24 hours
    for (let station = 0; station < 11; station += 1) {
      for (let hour = 0; hour < 24; hour += 1) {
        stationRows += 1;
      }
    }

    // count every fixed-lead anchor category
    for (let anchor = 0; anchor < 168; anchor += 1) {
      anchorRows += 1;
    }
  }

  const totalRows = retrievalRows + stationRows + anchorRows;
  return {
    anchorRows,
    headroomRows: 4_000_000 - totalRows,
    label: "export_rows_not_training_events",
    retrievalRows,
    stationRows,
    totalRows,
  };
}

// create one database manifest fixture
function databaseManifest() {
  return {
    aggregation_contract_sha256: fixedHashes.aggregation,
    contract_version: "forecast-training-export-manifest/v1",
    coordinate_manifest_sha256: fixedHashes.coordinate,
    metric_eligibility_sha256: fixedHashes.metric,
    migration_checksums: [...migrationChecksums],
    migration_history_sha256: migrationHistory,
    migration_names: [...migrationNames],
    query_contract_sha256: fixedHashes.query,
    query_contract_version: "forecast-training-export-query/v2",
    row_schema_sha256: fixedHashes.row,
    schema_migration: "0010_forecast_training_export.sql",
    site_key: "ballydidean",
    site_timezone: "America/Los_Angeles",
    source_lineage_sha256: fixedHashes.source,
    spatial_weights_sha256: fixedHashes.spatial,
    station_manifest_sha256: fixedHashes.station,
  };
}

// create one transaction manifest record
function transactionManifest(fromLocalDate = "2026-08-23", toLocalDate = "2026-08-24") {
  return {
    payload: databaseManifest(),
    record_type: "manifest",
    transaction: {
      created_at_utc: "2026-08-24T08:00:00.000000Z",
      from_local_date: fromLocalDate,
      idle_in_transaction_session_timeout: "30s",
      isolation_level: "repeatable read",
      lock_timeout: "5s",
      read_only: "on",
      statement_timeout: "15min",
      to_local_date: toLocalDate,
    },
  };
}

// create one station-hour row
function stationRow(overrides = {}) {
  return {
    adapter_contracts: ["ambient-device-data/v1", "wunderground-pws-history/v1"],
    collision_count: 0,
    content_hashes: [hashA, hashB],
    contract_epoch: "physical-station-hourly/v1",
    dataset: null,
    exclusion_reason_codes: [],
    ingestion_run_ids: ["9007199254740993", "9007199254740994"],
    physical_station_key: "ambient-maxweather",
    provider_family: "ambient",
    received_at: "2026-08-24T00:05:00.000000Z",
    record_kind: "station_hour",
    reference_at: null,
    reference_kind: null,
    relative_humidity_percent: 81,
    site_key: "ballydidean",
    source_config_fingerprints: [ambientFingerprint, wundergroundFingerprint],
    source_keys: [
      "ambient-maxweather-observations-v1",
      "wunderground-maxweather-history-v1",
    ],
    target_lead_hours: null,
    temperature_c: 12.5,
    upstream_model: null,
    valid_at: "2026-08-24T00:00:00.000000Z",
    wind_direction_degrees: 182,
    wind_gust_mps: 6.2,
    wind_speed_mps: 3.1,
    ...overrides,
  };
}

// create one exact live v4 forecast row
function legacyForecastRow(overrides = {}) {
  return {
    adapter_contracts: ["forecast-daily/v4"],
    collision_count: 0,
    content_hashes: [hashA],
    contract_epoch:
      legacyV4ContractEpoch,
    dataset: "forecast",
    exclusion_reason_codes: [],
    ingestion_run_ids: ["9007199254740993"],
    physical_station_key: null,
    provider_family: null,
    received_at: "2026-08-23T00:31:00.000000Z",
    record_kind: "legacy_v4_retrieval_snapshot",
    reference_at: "2026-08-23T00:30:00.000000Z",
    reference_kind: "retrieval_snapshot",
    relative_humidity_percent: 81,
    site_key: "ballydidean",
    source_config_fingerprints: [legacyV4Fingerprint],
    source_keys: ["open-meteo-forecast-v4"],
    target_lead_hours: 24,
    temperature_c: 12.5,
    upstream_model: "best_match",
    valid_at: "2026-08-24T00:00:00.000000Z",
    wind_direction_degrees: 182,
    wind_gust_mps: 6.2,
    wind_speed_mps: 3.1,
    ...overrides,
  };
}

// create one explicit station gap
function gapRow(overrides = {}) {
  return stationRow({
    adapter_contracts: [],
    content_hashes: [],
    exclusion_reason_codes: ["station_coverage_insufficient"],
    ingestion_run_ids: [],
    received_at: null,
    relative_humidity_percent: null,
    source_config_fingerprints: [],
    source_keys: [],
    temperature_c: null,
    valid_at: "2026-08-24T08:00:00.000000Z",
    wind_direction_degrees: null,
    wind_gust_mps: null,
    wind_speed_mps: null,
    ...overrides,
  });
}

// derive one local date label
function localDate(instant) {
  const parts = Object.fromEntries(
    localFormatter
      .formatToParts(new Date(instant))
      .filter((part) => part.type !== "literal")
      .map((part) => [part.type, part.value]),
  );
  return `${parts.year}-${parts.month}-${parts.day}`;
}

// generate every real UTC hour for one local date
function localHours(date) {
  const center = Date.parse(`${date}T00:00:00.000Z`);
  const hours = [];

  // scan every possible Pacific offset
  for (let current = center - 43_200_000; current <= center + 129_600_000; current += 3_600_000) {
    const instant = new Date(current).toISOString();

    // retain the requested local date
    if (localDate(instant) === date) {
      hours.push(instant.replace(".000Z", ".000000Z"));
    }
  }

  return hours;
}

// create the complete two-day station grid
function completeStationRows() {
  const rows = [];

  // add every station-hour gap
  for (const date of ["2026-08-23", "2026-08-24"]) {
    // add every real UTC hour
    for (const validAt of localHours(date)) {
      // add all physical stations
      for (const [stationKey, providerFamily] of stationProviders) {
        rows.push(gapRow({
          physical_station_key: stationKey,
          provider_family: providerFamily,
          valid_at: validAt,
        }));
      }
    }
  }

  const cutoverIndex = rows.findIndex((row) =>
    row.physical_station_key === "ambient-maxweather" &&
    row.valid_at === "2026-08-24T00:00:00.000000Z");
  assert.notEqual(cutoverIndex, -1);
  rows[cutoverIndex] = stationRow();
  return rows;
}

// write one complete transaction stream
async function writeTransaction(
  path,
  rows = completeStationRows(),
  fromLocalDate = "2026-08-23",
  toLocalDate = "2026-08-24",
) {
  const records = [transactionManifest(fromLocalDate, toLocalDate), ...rows.map((payload) => ({
    payload,
    record_type: "row",
  }))];
  await writeFile(path, records.map((record) => JSON.stringify(record)).join("\n") + "\n");
}

// run the package helper
function runHelper(argumentsList) {
  return spawnSync(process.execPath, [helper, ...argumentsList], {
    cwd: repoRoot,
    encoding: "utf8",
  });
}

// create one deterministic package archive
function createArchive(packageRoot, archive) {
  const tar = spawnSync("tar", [
    "--create",
    "--gzip",
    "--file",
    archive,
    "--directory",
    packageRoot,
    ".",
  ], { cwd: repoRoot, encoding: "utf8" });
  assert.equal(tar.status, 0, tar.stderr);
}

// install one isolated pull harness
async function installPullHarness(directory) {
  const scripts = join(directory, "deploy/scripts");
  await mkdir(scripts, { recursive: true });
  await Promise.all([
    cp(join(repoRoot, "deploy/scripts/common.sh"), join(scripts, "common.sh")),
    cp(helper, join(scripts, "forecast-training-package.mjs")),
    cp(pullScript, join(scripts, "pull-forecast-training-export.sh")),
    writeFile(
      join(scripts, "ssh-run.sh"),
      "#!/usr/bin/env bash\ncat -- \"${WEATHER_TEST_ARCHIVE:?}\"\n",
    ),
  ]);
  await Promise.all([
    chmod(join(scripts, "ssh-run.sh"), 0o700),
    chmod(join(scripts, "pull-forecast-training-export.sh"), 0o700),
  ]);
  return scripts;
}

// bootstrap one production-shaped disposable database
async function bootstrapDisposableExportDatabase(server, directory) {
  const binaries = join(directory, "database-bin");
  const secrets = join(directory, "database-secrets");
  await mkdir(binaries);
  await mkdir(secrets);
  const secretValues = {
    admin: server.password,
    api: "api-export-smoke",
    ingest: "ingest-export-smoke",
    owner: "owner-export-smoke",
    training: "training-export-smoke",
  };
  const secretPaths = Object.fromEntries(
    Object.keys(secretValues).map((name) => [name, join(secrets, name)]),
  );
  await Promise.all(Object.entries(secretValues).map(([name, value]) =>
    writeFile(secretPaths[name], `${value}\n`, { mode: 0o600 })));
  await writeFile(
    join(binaries, "psql"),
    "#!/usr/bin/env bash\nexec \"${WEATHER_TEST_DOCKER_REAL:?}\" run --interactive --rm --network host --env PGHOST --env PGPORT --env PGPASSWORD postgres:17-bookworm psql \"$@\"\n",
  );
  await chmod(join(binaries, "psql"), 0o700);
  const dockerExecutable = spawnSync("sh", ["-c", "command -v docker"], {
    encoding: "utf8",
  }).stdout.trim();
  assert.notEqual(dockerExecutable, "");
  const roleEnvironment = {
    ...process.env,
    PATH: `${binaries}:${process.env.PATH}`,
    PGHOST: server.host,
    PGPASSWORD: server.password,
    PGPORT: String(server.port),
    POSTGRES_DB: "weather_test",
    POSTGRES_USER: server.user,
    WEATHER_ADMIN_PASSWORD_FILE: secretPaths.admin,
    WEATHER_API_PASSWORD_FILE: secretPaths.api,
    WEATHER_INGEST_PASSWORD_FILE: secretPaths.ingest,
    WEATHER_OWNER_PASSWORD_FILE: secretPaths.owner,
    WEATHER_TEST_DOCKER_REAL: dockerExecutable,
    WEATHER_TRAINING_EXPORT_PASSWORD_FILE: secretPaths.training,
  };
  const roles = spawnSync(join(repoRoot, "deploy/postgres/010-create-runtime-roles.sh"), [], {
    cwd: repoRoot,
    encoding: "utf8",
    env: roleEnvironment,
  });
  assert.equal(roles.status, 0, roles.stderr);
  const migration = spawnSync(process.execPath, ["deploy/scripts/migrate.mjs"], {
    cwd: repoRoot,
    encoding: "utf8",
    env: {
      ...process.env,
      WEATHER_DATABASE_APPLICATION_NAME: "forecast-training-export-smoke",
      WEATHER_DATABASE_HOST: server.host,
      WEATHER_DATABASE_NAME: "weather_test",
      WEATHER_DATABASE_PASSWORD_FILE: secretPaths.owner,
      WEATHER_DATABASE_PORT: String(server.port),
      WEATHER_DATABASE_SSL: "false",
      WEATHER_DATABASE_USER: "weather_owner",
      WEATHER_ECOWITT_CONFIG_PATH: join(repoRoot, "config/ecowitt/gateways.json"),
      WEATHER_MIGRATION_DIRECTORY: join(repoRoot, "packages/database/migrations"),
      WEATHER_PUBLIC_STATIONS_CONFIG_PATH: join(
        repoRoot,
        "config/public-stations/stations.json",
      ),
      WEATHER_SITE_CONFIG_PATH: join(repoRoot, "config/sites/ballydidean.json"),
      WEATHER_TEMPEST_CONFIG_PATH: join(repoRoot, "config/tempest/stations.json"),
      WEATHER_TIDE_CONFIG_PATH: join(repoRoot, "config/tides/noaa.json"),
    },
  });
  assert.equal(migration.status, 0, migration.stderr);
  return { dockerExecutable, trainingSecret: secretPaths.training };
}

// seed one rejected physical observation
async function seedPoisonObservation(pool) {
  const source = await pool.query(`
    SELECT id, source_config_fingerprint
    FROM sources
    WHERE source_key = 'tempest-38270-observations-v2'
  `);
  assert.equal(source.rowCount, 1);
  const run = await pool.query(
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
      ) VALUES (
        $1,
        'scheduled',
        TIMESTAMPTZ '2026-08-24 00:00:00+00',
        TIMESTAMPTZ '2026-08-25 00:00:00+00',
        $2,
        'tempest-observations/v2',
        TIMESTAMPTZ '2026-08-25 00:00:00+00',
        TIMESTAMPTZ '2026-08-25 00:05:00+00',
        TIMESTAMPTZ '2026-08-25 00:01:00+00',
        'succeeded',
        1,
        1
      )
      RETURNING id
    `,
    [source.rows[0].id, source.rows[0].source_config_fingerprint],
  );
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
        content_hash
      ) VALUES (
        $1,
        'physical_sensor',
        TIMESTAMPTZ '2026-08-24 08:00:00+00',
        NULL,
        $2,
        $2,
        TIMESTAMPTZ '2026-08-24 08:01:00+00',
        TIMESTAMPTZ '2026-08-24 08:01:00+00',
        'UTC',
        'serial-password-poison',
        '{"flags":["poison_flag"]}'::jsonb,
        '{"request_id":"provider-password-poison"}'::jsonb,
        12.5,
        $3
      )
    `,
    [source.rows[0].id, run.rows[0].id, "d".repeat(64)],
  );
}

// install one actual exporter with a disposable Compose bridge
async function installDatabaseExportHarness(directory) {
  const scripts = join(directory, "deploy/scripts");
  const binaries = join(directory, "export-bin");
  await mkdir(scripts, { recursive: true });
  await mkdir(binaries);
  await Promise.all([
    cp(join(repoRoot, "deploy/scripts/common.sh"), join(scripts, "common.sh")),
    cp(exportScript, join(scripts, "forecast-training-export.sh")),
    cp(helper, join(scripts, "forecast-training-package.mjs")),
    cp(pullScript, join(scripts, "pull-forecast-training-export.sh")),
    writeFile(join(directory, "deploy/.env"), "WEATHER_DATABASE_NAME=weather_test\n"),
    writeFile(
      join(binaries, "docker"),
      `#!/usr/bin/env bash
# locate the fixed postgres service operand
while (($# > 0)) && [[ "$1" != postgres ]]; do
  shift
done
[[ "\${1:-}" == postgres ]] || exit 2
shift
exec "\${WEATHER_TEST_DOCKER_REAL:?}" run --interactive --rm \
  --network "container:\${WEATHER_TEST_POSTGRES_CONTAINER:?}" \
  --volume "\${WEATHER_TEST_EXPORT_SECRET:?}:/run/secrets/weather_training_export_password:ro" \
  postgres:17-bookworm "$@"
`,
    ),
  ]);
  await Promise.all([
    chmod(join(scripts, "forecast-training-export.sh"), 0o700),
    chmod(join(scripts, "pull-forecast-training-export.sh"), 0o700),
    chmod(join(binaries, "docker"), 0o700),
  ]);
  return { binaries, scripts };
}

// build one fixture package
async function buildFixture(directory, rows) {
  const input = join(directory, "transaction.jsonl");
  const output = join(directory, "package");
  await writeTransaction(input, rows);
  const result = runHelper([
    "build",
    input,
    output,
    "2026-08-23",
    "2026-08-24",
  ]);
  assert.equal(result.status, 0, result.stderr);
  return { hash: result.stdout.trim(), output };
}

// rewrite one canonical manifest and checksum
async function rewriteManifest(root, mutate) {
  const path = join(root, "manifest.json");
  const manifest = JSON.parse(await readFile(path, "utf8"));
  mutate(manifest);
  const bytes = canonicalJson(manifest);
  const digest = createHash("sha256").update(bytes).digest("hex");
  await writeFile(path, bytes);
  await writeFile(join(root, "manifest.sha256"), `${digest}  manifest.json\n`);
}

// rewrite one compressed member with consistent metadata
async function rewriteMember(root, memberIndex, transform) {
  const manifestPath = join(root, "manifest.json");
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  const member = manifest.members[memberIndex];
  const path = join(root, member.path);
  const plaintext = gunzipSync(await readFile(path)).toString("utf8");
  const originalLines = plaintext.trimEnd().split("\n");
  const lines = transform(originalLines).sort();
  const updatedPlaintext = Buffer.from(`${lines.join("\n")}\n`);
  const compressed = gzipSync(updatedPlaintext, { level: 9, mtime: 0 });
  const instants = lines.map((line) =>
    new Date(JSON.parse(line).valid_at).toISOString()).sort();
  await writeFile(path, compressed);
  member.maxValidAt = instants.at(-1);
  member.minValidAt = instants[0];
  member.plaintextBytes = updatedPlaintext.byteLength;
  member.rowCount = lines.length;
  member.sha256 = createHash("sha256").update(compressed).digest("hex");
  member.sizeBytes = compressed.byteLength;
  manifest.totalRowCount += lines.length - originalLines.length;
  await rewriteManifest(root, (value) => Object.assign(value, manifest));
}

test("package migration ledger matches immutable repository bytes", async () => {
  // bind every expected checksum to its migration file
  for (let index = 0; index < migrationNames.length; index += 1) {
    const bytes = await readFile(
      join(repoRoot, "packages/database/migrations", migrationNames[index]),
    );
    assert.equal(createHash("sha256").update(bytes).digest("hex"), migrationChecksums[index]);
  }
  const rainMigration = await readFile(
    join(repoRoot, "packages/database/migrations", rainCollectionMigration.name),
  );
  assert.equal(createHash("sha256").update(rainMigration).digest("hex"), rainCollectionMigration.checksum);
  const stationAccessMigration = await readFile(
    join(repoRoot, "packages/database/migrations", rainStationAccessMigration.name),
  );
  assert.equal(createHash("sha256").update(stationAccessMigration).digest("hex"), rainStationAccessMigration.checksum);
  const adjustmentMigration = await readFile(
    join(repoRoot, "packages/database/migrations", rainAdjustmentMigration.name),
  );
  assert.equal(createHash("sha256").update(adjustmentMigration).digest("hex"), rainAdjustmentMigration.checksum);
  const adjustmentExportMigration = await readFile(
    join(repoRoot, "packages/database/migrations", adjustmentEvaluationExportMigration.name),
  );
  assert.equal(
    createHash("sha256").update(adjustmentExportMigration).digest("hex"),
    adjustmentEvaluationExportMigration.checksum,
  );
});

// preserve retained snapshots while allowing the exact additive migration
test("package verifies both retained 13 and new 14 migration ledgers", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-rain-ledger-"));

  try {
    const retainedDirectory = join(directory, "retained");
    await mkdir(retainedDirectory);
    const retained = await buildFixture(retainedDirectory);
    const retainedVerified = runHelper(["verify", retained.output]);
    assert.equal(retainedVerified.status, 0, retainedVerified.stderr);

    const input = join(directory, "new-transaction.jsonl");
    await writeTransaction(input);
    const lines = (await readFile(input, "utf8")).trimEnd().split("\n");
    const envelope = JSON.parse(lines[0]);
    envelope.payload.migration_names.push(rainCollectionMigration.name);
    envelope.payload.migration_checksums.push(rainCollectionMigration.checksum);
    envelope.payload.migration_history_sha256 = createHash("sha256")
      .update(envelope.payload.migration_names.map((name, index) =>
        `${name}:${envelope.payload.migration_checksums[index]}`).join("\n"))
      .digest("hex");
    lines[0] = JSON.stringify(envelope);
    await writeFile(input, `${lines.join("\n")}\n`);

    const output = join(directory, "new-package");
    const built = runHelper(["build", input, output, "2026-08-23", "2026-08-24"]);
    assert.equal(built.status, 0, built.stderr);
    const verified = runHelper(["verify", output]);
    assert.equal(verified.status, 0, verified.stderr);
    assert.equal(verified.stdout.trim(), built.stdout.trim());
    const packageManifest = JSON.parse(await readFile(join(output, "manifest.json"), "utf8"));
    assert.deepEqual(packageManifest.databaseManifest.migration_names, [
      ...migrationNames,
      rainCollectionMigration.name,
    ]);

    // reject a self-consistent forged additive checksum
    envelope.payload.migration_checksums[envelope.payload.migration_checksums.length - 1] = hashA;
    envelope.payload.migration_history_sha256 = createHash("sha256")
      .update(envelope.payload.migration_names.map((name, index) =>
        `${name}:${envelope.payload.migration_checksums[index]}`).join("\n"))
      .digest("hex");
    lines[0] = JSON.stringify(envelope);
    await writeFile(input, `${lines.join("\n")}\n`);
    const forged = runHelper(["build", input, join(directory, "forged-package"), "2026-08-23", "2026-08-24"]);
    assert.notEqual(forged.status, 0);
    assert.match(forged.stderr, /checked repository ledger/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// bind the station authorization to the next exact ordered ledger
test("package verifies the exact 15 migration ledger and rejects a forged station checksum", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-station-ledger-"));

  try {
    const input = join(directory, "transaction.jsonl");
    await writeTransaction(input);
    const lines = (await readFile(input, "utf8")).trimEnd().split("\n");
    const envelope = JSON.parse(lines[0]);
    envelope.payload.migration_names.push(rainCollectionMigration.name, rainStationAccessMigration.name);
    envelope.payload.migration_checksums.push(rainCollectionMigration.checksum, rainStationAccessMigration.checksum);
    envelope.payload.migration_history_sha256 = createHash("sha256")
      .update(envelope.payload.migration_names.map((name, index) =>
        `${name}:${envelope.payload.migration_checksums[index]}`).join("\n"))
      .digest("hex");
    lines[0] = JSON.stringify(envelope);
    await writeFile(input, `${lines.join("\n")}\n`);

    const output = join(directory, "package");
    const built = runHelper(["build", input, output, "2026-08-23", "2026-08-24"]);
    assert.equal(built.status, 0, built.stderr);
    const verified = runHelper(["verify", output]);
    assert.equal(verified.status, 0, verified.stderr);
    const packageManifest = JSON.parse(await readFile(join(output, "manifest.json"), "utf8"));
    assert.deepEqual(packageManifest.databaseManifest.migration_names, [
      ...migrationNames,
      rainCollectionMigration.name,
      rainStationAccessMigration.name,
    ]);

    // reject one forged but self-consistent station ledger
    envelope.payload.migration_checksums[envelope.payload.migration_checksums.length - 1] = hashA;
    envelope.payload.migration_history_sha256 = createHash("sha256")
      .update(envelope.payload.migration_names.map((name, index) =>
        `${name}:${envelope.payload.migration_checksums[index]}`).join("\n"))
      .digest("hex");
    lines[0] = JSON.stringify(envelope);
    await writeFile(input, `${lines.join("\n")}\n`);
    const forged = runHelper(["build", input, join(directory, "forged"), "2026-08-23", "2026-08-24"]);
    assert.notEqual(forged.status, 0);
    assert.match(forged.stderr, /checked repository ledger/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// bind the bounded rain projection to the next exact ordered ledger
test("package verifies the exact 16 migration ledger and rejects a forged adjustment checksum", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-adjustment-ledger-"));

  try {
    const input = join(directory, "transaction.jsonl");
    await writeTransaction(input);
    const lines = (await readFile(input, "utf8")).trimEnd().split("\n");
    const envelope = JSON.parse(lines[0]);
    envelope.payload.migration_names.push(
      rainCollectionMigration.name,
      rainStationAccessMigration.name,
      rainAdjustmentMigration.name,
    );
    envelope.payload.migration_checksums.push(
      rainCollectionMigration.checksum,
      rainStationAccessMigration.checksum,
      rainAdjustmentMigration.checksum,
    );
    envelope.payload.migration_history_sha256 = createHash("sha256")
      .update(envelope.payload.migration_names.map((name, index) =>
        `${name}:${envelope.payload.migration_checksums[index]}`).join("\n"))
      .digest("hex");
    lines[0] = JSON.stringify(envelope);
    await writeFile(input, `${lines.join("\n")}\n`);

    const output = join(directory, "package");
    const built = runHelper(["build", input, output, "2026-08-23", "2026-08-24"]);
    assert.equal(built.status, 0, built.stderr);
    const verified = runHelper(["verify", output]);
    assert.equal(verified.status, 0, verified.stderr);
    const packageManifest = JSON.parse(await readFile(join(output, "manifest.json"), "utf8"));
    assert.deepEqual(packageManifest.databaseManifest.migration_names, [
      ...migrationNames,
      rainCollectionMigration.name,
      rainStationAccessMigration.name,
      rainAdjustmentMigration.name,
    ]);

    // reject a self-consistent but unreviewed migration identity
    envelope.payload.migration_checksums[envelope.payload.migration_checksums.length - 1] = hashA;
    envelope.payload.migration_history_sha256 = createHash("sha256")
      .update(envelope.payload.migration_names.map((name, index) =>
        `${name}:${envelope.payload.migration_checksums[index]}`).join("\n"))
      .digest("hex");
    lines[0] = JSON.stringify(envelope);
    await writeFile(input, `${lines.join("\n")}\n`);
    const forged = runHelper(["build", input, join(directory, "forged"), "2026-08-23", "2026-08-24"]);
    assert.notEqual(forged.status, 0);
    assert.match(forged.stderr, /checked repository ledger/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// keep existing target export usable after the private evaluation migration
test("package verifies the exact 17 migration ledger and rejects a forged evaluation checksum", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-evaluation-ledger-"));

  try {
    const input = join(directory, "transaction.jsonl");
    await writeTransaction(input);
    const lines = (await readFile(input, "utf8")).trimEnd().split("\n");
    const envelope = JSON.parse(lines[0]);
    const additions = [
      rainCollectionMigration,
      rainStationAccessMigration,
      rainAdjustmentMigration,
      adjustmentEvaluationExportMigration,
    ];
    envelope.payload.migration_names.push(...additions.map((migration) => migration.name));
    envelope.payload.migration_checksums.push(...additions.map((migration) => migration.checksum));
    envelope.payload.migration_history_sha256 = createHash("sha256")
      .update(envelope.payload.migration_names.map((name, index) =>
        `${name}:${envelope.payload.migration_checksums[index]}`).join("\n"))
      .digest("hex");
    lines[0] = JSON.stringify(envelope);
    await writeFile(input, `${lines.join("\n")}\n`);

    const output = join(directory, "package");
    const built = runHelper(["build", input, output, "2026-08-23", "2026-08-24"]);
    assert.equal(built.status, 0, built.stderr);
    const verified = runHelper(["verify", output]);
    assert.equal(verified.status, 0, verified.stderr);
    const packageManifest = JSON.parse(await readFile(join(output, "manifest.json"), "utf8"));
    assert.deepEqual(packageManifest.databaseManifest.migration_names, [
      ...migrationNames,
      ...additions.map((migration) => migration.name),
    ]);

    // reject a self-consistent forged evaluation migration identity
    envelope.payload.migration_checksums[envelope.payload.migration_checksums.length - 1] = hashA;
    envelope.payload.migration_history_sha256 = createHash("sha256")
      .update(envelope.payload.migration_names.map((name, index) =>
        `${name}:${envelope.payload.migration_checksums[index]}`).join("\n"))
      .digest("hex");
    lines[0] = JSON.stringify(envelope);
    await writeFile(input, `${lines.join("\n")}\n`);
    const forged = runHelper(["build", input, join(directory, "forged"), "2026-08-23", "2026-08-24"]);
    assert.notEqual(forged.status, 0);
    assert.match(forged.stderr, /checked repository ledger/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// bind the query identity to exact view bytes
test("query contract hash matches the exact range-bound row view", async () => {
  const migration = await readFile(
    join(repoRoot, "packages/database/migrations/0010_forecast_training_export.sql"),
    "utf8",
  );
  const manifestStart = migration.indexOf(
    "CREATE VIEW forecast_training_export_manifest_v1",
  );
  assert.notEqual(manifestStart, -1);
  const rowView = migration.slice(0, manifestStart);
  assert.equal(
    createHash("sha256").update(rowView).digest("hex"),
    fixedHashes.query,
  );
});

test("package migration ledger rejects missing, extra, and reordered entries", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-ledger-reject-"));

  // require one semantically rejected ledger
  async function assertLedgerRejected(label, mutate, expected) {
    const envelope = transactionManifest();
    mutate(envelope.payload);
    envelope.payload.migration_history_sha256 = createHash("sha256")
      .update(envelope.payload.migration_names.map((name, index) =>
        `${name}:${envelope.payload.migration_checksums[index]}`).join("\n"))
      .digest("hex");
    const input = join(directory, `${label}.jsonl`);
    await writeFile(input, `${JSON.stringify(envelope)}\n`);
    const result = runHelper([
      "build",
      input,
      join(directory, `${label}-package`),
      "2026-08-23",
      "2026-08-24",
    ]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, expected);
  }

  try {
    // remove the additive migration
    await assertLedgerRejected("missing", (manifest) => {
      manifest.migration_names.pop();
      manifest.migration_checksums.pop();
    }, /checked repository ledger/u);
    // append an unreviewed migration
    await assertLedgerRejected("extra", (manifest) => {
      manifest.migration_names.push("0013_unreviewed.sql");
      manifest.migration_checksums.push(hashA);
    }, /checked repository ledger/u);
    // reorder the final ledger pair
    await assertLedgerRejected("reordered", (manifest) => {
      [manifest.migration_names[10], manifest.migration_names[11]] =
        [manifest.migration_names[11], manifest.migration_names[10]];
      [manifest.migration_checksums[10], manifest.migration_checksums[11]] =
        [manifest.migration_checksums[11], manifest.migration_checksums[10]];
    }, /database migration manifest is invalid/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("legacy v4 epoch binds the ingestion adapter and source fingerprint", async () => {
  const [providerSource, migration, siteConfigBytes] = await Promise.all([
    readFile(join(repoRoot, "packages/providers/src/open-meteo.ts"), "utf8"),
    readFile(
      join(repoRoot, "packages/database/migrations/0010_forecast_training_export.sql"),
      "utf8",
    ),
    readFile(join(repoRoot, "config/sites/ballydidean.json"), "utf8"),
  ]);
  const derivedEpoch = `legacy-v4/${createHash("sha256")
    .update(legacyV4AdapterVersion, "utf8")
    .update(Buffer.from([0]))
    .update(legacyV4Fingerprint, "utf8")
    .digest("hex")}`;
  const siteConfig = JSON.parse(siteConfigBytes);
  // locate the distinct source-material contract
  const forecastSource = siteConfig.sources.find(
    (source) => source.key === "open-meteo-forecast-v4",
  );
  assert.equal(derivedEpoch, legacyV4ContractEpoch);
  assert.match(
    providerSource,
    /OPEN_METEO_FORECAST_ADAPTER_VERSION\s*=\s*"open-meteo-forecast-daily\/v4"/u,
  );
  assert.match(
    migration,
    /convert_to\(last_run\.adapter_version, 'UTF8'\)\s*\|\| decode\('00', 'hex'\)\s*\|\|\s*convert_to\(configured_source\.source_config_fingerprint, 'UTF8'\)/u,
  );
  assert.equal(forecastSource.adapterConfig.contractVersion, "forecast-daily/v4");
  assert.equal(legacyForecastRow().adapter_contracts[0], "forecast-daily/v4");
});

test("row and local database boundaries fail closed", () => {
  assert.deepEqual(generateConservativeExportShape(), {
    anchorRows: 75_600,
    headroomRows: 954_400,
    label: "export_rows_not_training_events",
    retrievalRows: 2_851_200,
    stationRows: 118_800,
    totalRows: 3_045_600,
  });
  let lastAcceptedRowCount = 0;
  let conservativeOracleVisited = false;
  assert.throws(() => {
    // generate the complete cap plus one stopping row
    for (let rowCount = 1; rowCount <= 4_000_001; rowCount += 1) {
      lastAcceptedRowCount = assertExportRowCount(rowCount);

      // mark the exact conservative oracle
      if (rowCount === 3_045_600) {
        conservativeOracleVisited = true;
      }
    }
  }, /exceeds 4000000/u);
  assert.equal(conservativeOracleVisited, true);
  assert.equal(lastAcceptedRowCount, 4_000_000);
  assert.equal(
    assertLocalReadOnlyTarget(
      "postgresql://127.0.0.1/weather?options=-c%20default_transaction_read_only%3Don",
    ),
    "local-read-only-target-valid",
  );
  assert.throws(
    () => assertLocalReadOnlyTarget(
      "postgresql://sentinel-secret@127.0.0.1/weather?options=-c%20default_transaction_read_only%3Don",
    ),
    /credential-free/u,
  );
  assert.throws(
    () => assertLocalReadOnlyTarget(
      "postgresql://database.example/weather?options=-c%20default_transaction_read_only%3Don",
    ),
    /loopback/u,
  );
  assert.throws(
    () => assertLocalReadOnlyTarget("postgresql://localhost/weather", "production"),
    /production-marked/u,
  );
  assert.throws(
    () => assertLocalReadOnlyTarget(
      "postgresql://localhost/production?options=-c%20default_transaction_read_only%3Don",
    ),
    /production-marked/u,
  );
  assert.throws(
    () => assertLocalReadOnlyTarget(
      "postgresql://localhost/weather?options=-c%20default_transaction_read_only%3Doff",
    ),
    /read-only/u,
  );
  const sentinel = "never-print-this-password";
  const credentialProbe = runHelper([
    "validate-local-target",
    `postgresql://operator:${sentinel}@127.0.0.1/weather?options=-c%20default_transaction_read_only%3Don`,
  ]);
  assert.notEqual(credentialProbe.status, 0);
  assert.doesNotMatch(`${credentialProbe.stdout}${credentialProbe.stderr}`, new RegExp(sentinel, "u"));
});

test("package build preserves mixed cutover lineage and explicit gaps", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-package-"));

  try {
    const fixture = await buildFixture(directory);
    assert.match(fixture.hash, /^[a-f0-9]{64}$/u);
    const verified = runHelper(["verify", fixture.output]);
    assert.equal(verified.status, 0, verified.stderr);
    assert.equal(verified.stdout.trim(), fixture.hash);
    const manifest = JSON.parse(await readFile(join(fixture.output, "manifest.json"), "utf8"));
    assert.equal(manifest.totalRowCount, 528);
    assert.deepEqual(
      manifest.members
        .filter((member) => member.stationKey === "ambient-maxweather")
        .map((member) => [member.localDate, member.recordKind, member.stationKey]),
      [
        ["2026-08-23", "station-hour", "ambient-maxweather"],
        ["2026-08-24", "station-hour", "ambient-maxweather"],
      ],
    );
    assert.deepEqual(
      manifest.observedSourceIdentities.filter((source) =>
        source.sourceKey.includes("maxweather")),
      [
        {
          adapterContract: "ambient-device-data/v1",
          sourceConfigFingerprint: ambientFingerprint,
          sourceKey: "ambient-maxweather-observations-v1",
        },
        {
          adapterContract: "wunderground-pws-history/v1",
          sourceConfigFingerprint: wundergroundFingerprint,
          sourceKey: "wunderground-maxweather-history-v1",
        },
      ],
    );
    assert.equal(manifest.limits.conservativeExportRows, 3_045_600);
    assert.equal(manifest.limits.exportRowHeadroom, 954_400);
    assert.equal(manifest.limits.rowCountMeaning, "export_rows_not_training_events");
    assert.deepEqual(
      manifest.stationMetricCoverage.find((coverage) =>
        coverage.stationKey === "ambient-maxweather"),
      {
        eligibleMetricNonNullLocalDates: {
          relative_humidity_percent: 1,
          temperature_c: 1,
          wind_direction_degrees: 1,
          wind_gust_mps: 1,
          wind_speed_mps: 1,
        },
        stationKey: "ambient-maxweather",
      },
    );
    assert.deepEqual(
      manifest.stationMetricCoverage.find((coverage) =>
        coverage.stationKey === "ballydidean-ecowitt")
        .eligibleMetricNonNullLocalDates,
      {
        relative_humidity_percent: 0,
        temperature_c: 0,
        wind_direction_degrees: 0,
        wind_gust_mps: 0,
        wind_speed_mps: 0,
      },
    );
    const repeatedRoot = join(directory, "repeated");
    await mkdir(repeatedRoot);
    const repeated = await buildFixture(repeatedRoot);
    assert.equal(repeated.hash, fixture.hash);
    assert.equal(
      await readFile(join(repeated.output, "manifest.json"), "utf8"),
      await readFile(join(fixture.output, "manifest.json"), "utf8"),
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("package build binds the live v4 forecast dataset separately from its model", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-v4-identity-"));

  try {
    const validRoot = join(directory, "valid");
    await mkdir(validRoot);
    const fixture = await buildFixture(validRoot, [
      ...completeStationRows(),
      legacyForecastRow(),
    ]);
    const verified = runHelper(["verify", fixture.output]);
    assert.equal(verified.status, 0, verified.stderr);
    const manifest = JSON.parse(
      await readFile(join(fixture.output, "manifest.json"), "utf8"),
    );
    const forecastMember = manifest.members.find((member) =>
      member.recordKind === "legacy-v4-retrieval");
    assert.notEqual(forecastMember, undefined);
    const forecastRows = gunzipSync(
      await readFile(join(fixture.output, forecastMember.path)),
    ).toString("utf8").trimEnd().split("\n").map(JSON.parse);
    assert.equal(forecastRows.length, 1);
    assert.deepEqual(forecastRows[0].adapter_contracts, ["forecast-daily/v4"]);
    assert.equal(forecastRows[0].contract_epoch, legacyV4ContractEpoch);
    assert.equal(forecastRows[0].dataset, "forecast");
    assert.equal(forecastRows[0].upstream_model, "best_match");

    const invalidRoot = join(directory, "invalid");
    await mkdir(invalidRoot);
    const input = join(invalidRoot, "transaction.jsonl");
    await writeTransaction(input, [legacyForecastRow({ dataset: "best_match" })]);
    const rejected = runHelper([
      "build",
      input,
      join(invalidRoot, "package"),
      "2026-08-23",
      "2026-08-24",
    ]);
    assert.notEqual(rejected.status, 0);
    assert.match(rejected.stderr, /legacy v4 retrieval row identity is invalid/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("package build preserves canonical station diagnostics", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-diagnostics-"));

  try {
    const rows = completeStationRows();
    const populated = rows.find((row) =>
      row.physical_station_key === "ambient-maxweather" &&
      row.valid_at === "2026-08-24T00:00:00.000000Z");
    assert.notEqual(populated, undefined);
    populated.exclusion_reason_codes = stationDiagnosticReasons;
    const fixture = await buildFixture(directory, rows);
    const verified = runHelper(["verify", fixture.output]);
    assert.equal(verified.status, 0, verified.stderr);
    assert.equal(verified.stdout.trim(), fixture.hash);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("package verification rejects member and semantic manifest tampering", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-tamper-"));

  try {
    const firstDirectory = join(directory, "first");
    await mkdir(firstDirectory);
    const first = await buildFixture(firstDirectory);
    const firstManifest = JSON.parse(await readFile(join(first.output, "manifest.json"), "utf8"));
    const memberPath = join(first.output, firstManifest.members[0].path);
    const bytes = await readFile(memberPath);
    bytes[bytes.length - 1] ^= 1;
    await writeFile(memberPath, bytes);
    const memberTamper = runHelper(["verify", first.output]);
    assert.notEqual(memberTamper.status, 0);
    assert.match(memberTamper.stderr, /member checksum or size mismatch/u);

    const secondDirectory = join(directory, "second");
    await mkdir(secondDirectory);
    const second = await buildFixture(secondDirectory);
    await rewriteManifest(second.output, (manifest) => {
      manifest.transaction.readOnly = "off";
    });
    const transactionTamper = runHelper(["verify", second.output]);
    assert.notEqual(transactionTamper.status, 0);
    assert.match(transactionTamper.stderr, /package manifest boundary is invalid/u);

    const thirdDirectory = join(directory, "third");
    await mkdir(thirdDirectory);
    const third = await buildFixture(thirdDirectory);
    await rewriteManifest(third.output, (manifest) => {
      manifest.members[0].stationKey = "ambient-merlin";
    });
    const partitionTamper = runHelper(["verify", third.output]);
    assert.notEqual(partitionTamper.status, 0);
    assert.match(partitionTamper.stderr, /member date bounds mismatch/u);

    const fourthDirectory = join(directory, "fourth");
    await mkdir(fourthDirectory);
    const fourth = await buildFixture(fourthDirectory);
    await rewriteManifest(fourth.output, (manifest) => {
      manifest.databaseManifest.query_contract_sha256 = hashB;
      manifest.queryContractSha256 = hashB;
    });
    const contractTamper = runHelper(["verify", fourth.output]);
    assert.notEqual(contractTamper.status, 0);
    assert.match(contractTamper.stderr, /does not match the frozen contract/u);

    const fifthDirectory = join(directory, "fifth");
    await mkdir(fifthDirectory);
    const fifth = await buildFixture(fifthDirectory);
    await rewriteManifest(fifth.output, (manifest) => {
      manifest.databaseManifest.migration_checksums[0] = hashB;
      const history = createHash("sha256")
        .update(manifest.databaseManifest.migration_names.map((name, index) =>
          `${name}:${manifest.databaseManifest.migration_checksums[index]}`).join("\n"))
        .digest("hex");
      manifest.databaseManifest.migration_history_sha256 = history;
      manifest.migrationHistorySha256 = history;
    });
    const ledgerTamper = runHelper(["verify", fifth.output]);
    assert.notEqual(ledgerTamper.status, 0);
    assert.match(ledgerTamper.stderr, /checked repository ledger/u);

    const sixthDirectory = join(directory, "sixth");
    await mkdir(sixthDirectory);
    const sixth = await buildFixture(sixthDirectory);
    await mkdir(join(sixth.output, "unlisted"));
    const inventoryTamper = runHelper(["verify", sixth.output]);
    assert.notEqual(inventoryTamper.status, 0);
    assert.match(inventoryTamper.stderr, /file inventory mismatch/u);

    const coverageDirectory = join(directory, "coverage");
    await mkdir(coverageDirectory);
    const coverage = await buildFixture(coverageDirectory);
    await rewriteManifest(coverage.output, (manifest) => {
      manifest.stationMetricCoverage.find((entry) =>
        entry.stationKey === "ballydidean-ecowitt")
        .eligibleMetricNonNullLocalDates.temperature_c = 1;
    });
    const coverageTamper = runHelper(["verify", coverage.output]);
    assert.notEqual(coverageTamper.status, 0);
    assert.match(coverageTamper.stderr, /station metric coverage mismatch/u);

    const seventhDirectory = join(directory, "seventh");
    await mkdir(seventhDirectory);
    const seventh = await buildFixture(seventhDirectory);
    const seventhManifest = JSON.parse(
      await readFile(join(seventh.output, "manifest.json"), "utf8"),
    );
    const populatedMemberIndex = seventhManifest.members.findIndex((member) =>
      member.localDate === "2026-08-23" && member.stationKey === "ambient-maxweather");
    assert.notEqual(populatedMemberIndex, -1);
    let semanticMutationCount = 0;
    await rewriteMember(seventh.output, populatedMemberIndex, (lines) => lines.map((line) => {
      const row = JSON.parse(line);

      // mutate only the populated cutover row
      if (row.valid_at === "2026-08-24T00:00:00.000000Z") {
        row.exclusion_reason_codes = ["unknown_reason"];
        semanticMutationCount += 1;
      }

      return JSON.stringify(canonicalize(row));
    }));
    assert.equal(semanticMutationCount, 1);
    const semanticTamper = runHelper(["verify", seventh.output]);
    assert.notEqual(semanticTamper.status, 0);
    assert.match(semanticTamper.stderr, /exclusion reason is invalid/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("package verification requires every unique station hour", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-grid-"));

  try {
    const deletionRoot = join(directory, "member-deletion");
    await mkdir(deletionRoot);
    const deletion = await buildFixture(deletionRoot);
    const deletionManifest = JSON.parse(
      await readFile(join(deletion.output, "manifest.json"), "utf8"),
    );
    const removed = deletionManifest.members.shift();
    deletionManifest.totalRowCount -= removed.rowCount;
    await unlink(join(deletion.output, removed.path));
    await rewriteManifest(deletion.output, (manifest) => Object.assign(manifest, deletionManifest));
    const missingMember = runHelper(["verify", deletion.output]);
    assert.notEqual(missingMember.status, 0);
    assert.match(missingMember.stderr, /station-hour grid is incomplete/u);

    const hourRoot = join(directory, "hour-deletion");
    await mkdir(hourRoot);
    const hourDeletion = await buildFixture(hourRoot);
    await rewriteMember(hourDeletion.output, 0, (lines) => lines.slice(1));
    const missingHour = runHelper(["verify", hourDeletion.output]);
    assert.notEqual(missingHour.status, 0);
    assert.match(missingHour.stderr, /station-hour grid is incomplete/u);

    const duplicateRoot = join(directory, "hour-duplicate");
    await mkdir(duplicateRoot);
    const duplicate = await buildFixture(duplicateRoot);
    await rewriteMember(duplicate.output, 0, (lines) => [...lines, lines[0]]);
    const duplicateHour = runHelper(["verify", duplicate.output]);
    assert.notEqual(duplicateHour.status, 0);
    assert.match(duplicateHour.stderr, /station-hour grid is incomplete/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("collision, lineage drift, and malformed gap rows reject before publication", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-reject-"));
  const cases = [
    [stationRow({ collision_count: 1 }), /source collision/u, "2026-08-23", "2026-08-24"],
    [
      stationRow({ source_config_fingerprints: [hashA, wundergroundFingerprint] }),
      /lineage mismatch/u,
      "2026-08-23",
      "2026-08-24",
    ],
    [gapRow(), null, "2026-08-23", "2026-08-24"],
    [gapRow(), null, "2026-08-23", "2026-08-24"],
    [
      stationRow({ valid_at: "2026-08-24T01:00:00.000000Z" }),
      /lineage mismatch/u,
      "2026-08-23",
      "2026-08-24",
    ],
    [
      stationRow({ valid_at: "2026-08-23T23:00:00.000000Z" }),
      /lineage mismatch/u,
      "2026-08-23",
      "2026-08-24",
    ],
    [
      stationRow({
        adapter_contracts: ["tempest-observations/v2"],
        content_hashes: [hashA],
        ingestion_run_ids: ["9007199254740993"],
        physical_station_key: "tempest-225947",
        provider_family: "tempest",
        source_config_fingerprints: [tempest225947Fingerprint],
        source_keys: ["tempest-225947-observations-v2"],
        valid_at: "2026-07-13T23:00:00.000000Z",
      }),
      /lineage mismatch/u,
      "2026-07-13",
      "2026-07-13",
    ],
    [
      stationRow({ exclusion_reason_codes: ["unknown_reason"] }),
      /exclusion reason is invalid/u,
      "2026-08-23",
      "2026-08-24",
    ],
    [
      stationRow({ exclusion_reason_codes: ["metric_missing", "metric_missing"] }),
      /lineage arrays are invalid/u,
      "2026-08-23",
      "2026-08-24",
    ],
    [
      stationRow({ exclusion_reason_codes: ["station_coverage_insufficient"] }),
      /gap evidence is invalid/u,
      "2026-08-23",
      "2026-08-24",
    ],
  ];
  cases[2][0].exclusion_reason_codes = [];
  cases[2][1] = /gap evidence is invalid/u;
  cases[3][0].source_keys = ["ambient-maxweather-observations-v1"];
  cases[3][0].source_config_fingerprints = [ambientFingerprint];
  cases[3][0].adapter_contracts = ["ambient-device-data/v1"];
  cases[3][1] = /provenance is incomplete/u;

  try {
    // exercise each fail-closed mutation
    for (let index = 0; index < cases.length; index += 1) {
      const caseRoot = join(directory, String(index));
      await mkdir(caseRoot);
      const input = join(caseRoot, "input.jsonl");
      await writeTransaction(input, [cases[index][0]], cases[index][2], cases[index][3]);
      const result = runHelper([
        "build",
        input,
        join(caseRoot, "package"),
        cases[index][2],
        cases[index][3],
      ]);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, cases[index][1]);
    }
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("export scripts reject options, injection, reverse ranges, and day 451", () => {
  const commands = [
    [exportScript, ["--from", "2026-01-01"]],
    [exportScript, ["2026-01-01;touch-pwned", "2026-01-02"]],
    [exportScript, ["2026-01-02", "2026-01-01"]],
    [exportScript, ["2025-01-01", "2026-03-27"]],
    [pullScript, ["--output", "/tmp/export"]],
    [pullScript, ["2025-01-01", "2026-03-27"]],
    [sshRunScript, ["forecast-training-export", "2026-01-02", "2026-01-01"]],
    [sshRunScript, ["forecast-training-export", "2025-01-01", "2026-03-27"]],
    [sshRunScript, ["forecast-training-export", "2026-02-30", "2026-03-01"]],
  ];

  // reject before any network or database access
  for (const [script, argumentsList] of commands) {
    const result = spawnSync(script, argumentsList, { cwd: repoRoot, encoding: "utf8" });
    assert.notEqual(result.status, 0, `${script} accepted ${argumentsList.join(" ")}`);
    assert.doesNotMatch(`${result.stdout}${result.stderr}`, /password|postgresql:\/\//iu);
  }
});

test("forced SSH dispatch accepts only the exact two-date operation", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-dispatch-"));
  const sudo = join(directory, "sudo");

  try {
    await writeFile(sudo, "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\"\n");
    await chmod(sudo, 0o700);
    const valid = spawnSync(join(repoRoot, "deploy/scripts/ssh-dispatch.sh"), [], {
      cwd: repoRoot,
      encoding: "utf8",
      env: {
        ...process.env,
        PATH: `${directory}:${process.env.PATH}`,
        SSH_ORIGINAL_COMMAND: "forecast-training-export 2026-01-01 2026-01-02",
      },
    });
    assert.equal(valid.status, 0, valid.stderr);
    assert.equal(
      valid.stdout,
      "-n /usr/local/sbin/weather-remote-ops forecast-training-export 2026-01-01 2026-01-02\n",
    );

    const invalidCommands = [
      "forecast-training-export 2026-01-01",
      "forecast-training-export 2026-01-01 2026-01-02 extra",
      "forecast-training-export --from 2026-01-01",
      "forecast-training-export 2026-01-01;id 2026-01-02",
      "forecast-training-export 2026-01-01 2026-01-02 --output=/tmp/x",
    ];

    // reject every grammar expansion
    for (const command of invalidCommands) {
      const invalid = spawnSync(join(repoRoot, "deploy/scripts/ssh-dispatch.sh"), [], {
        cwd: repoRoot,
        encoding: "utf8",
        env: { ...process.env, PATH: `${directory}:${process.env.PATH}`, SSH_ORIGINAL_COMMAND: command },
      });
      assert.equal(invalid.status, 126, command);
      assert.equal(invalid.stderr, "operation denied\n");
      assert.equal(invalid.stdout, "");
    }
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("failed local pull removes every partial directory", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-pull-"));
  const scripts = join(directory, "deploy/scripts");

  try {
    await mkdir(scripts, { recursive: true });
    await Promise.all([
      cp(join(repoRoot, "deploy/scripts/common.sh"), join(scripts, "common.sh")),
      cp(helper, join(scripts, "forecast-training-package.mjs")),
      cp(pullScript, join(scripts, "pull-forecast-training-export.sh")),
      writeFile(join(scripts, "ssh-run.sh"), "#!/usr/bin/env bash\nprintf 'not-an-archive'\n"),
    ]);
    await Promise.all([
      chmod(join(scripts, "ssh-run.sh"), 0o700),
      chmod(join(scripts, "pull-forecast-training-export.sh"), 0o700),
    ]);
    const result = spawnSync(
      join(scripts, "pull-forecast-training-export.sh"),
      ["2026-08-23", "2026-08-24"],
      { cwd: directory, encoding: "utf8" },
    );
    assert.notEqual(result.status, 0);
    const entries = await readdir(join(directory, ".weather-data"));
    assert.deepEqual(entries, []);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("interrupted local pull removes its partial directory", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-interrupt-"));

  try {
    const scripts = await installPullHarness(directory);
    await writeFile(
      join(scripts, "ssh-run.sh"),
      "#!/usr/bin/env bash\nkill -TERM \"$PPID\"\nsleep 1\n",
    );
    await chmod(join(scripts, "ssh-run.sh"), 0o700);
    const result = spawnSync(
      join(scripts, "pull-forecast-training-export.sh"),
      ["2026-08-23", "2026-08-24"],
      { cwd: directory, encoding: "utf8" },
    );
    assert.notEqual(result.status, 0);
    assert.deepEqual(await readdir(join(directory, ".weather-data")), []);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("interrupted producer removes its server-side temporary", async (context) => {
  const namespaceProbe = spawnSync("unshare", ["-Ur", "true"], { encoding: "utf8" });

  // skip where unprivileged user namespaces are unavailable
  if (namespaceProbe.status !== 0) {
    context.skip("unprivileged user namespaces are unavailable");
    return;
  }

  const directory = await mkdtemp(join(tmpdir(), "weather-training-producer-interrupt-"));

  try {
    const scripts = join(directory, "deploy/scripts");
    const binaries = join(directory, "bin");
    const temporary = join(directory, "producer-temporary");
    await mkdir(scripts, { recursive: true });
    await mkdir(binaries);
    await Promise.all([
      cp(join(repoRoot, "deploy/scripts/common.sh"), join(scripts, "common.sh")),
      cp(exportScript, join(scripts, "forecast-training-export.sh")),
      cp(helper, join(scripts, "forecast-training-package.mjs")),
      writeFile(join(directory, "deploy/.env"), "WEATHER_DATABASE_NAME=weather\n"),
      writeFile(
        join(binaries, "mktemp"),
        "#!/usr/bin/env bash\nmkdir -m 0700 -- \"${WEATHER_TEST_TEMP:?}\"\nprintf '%s\\n' \"$WEATHER_TEST_TEMP\"\n",
      ),
      writeFile(
        join(binaries, "docker"),
        "#!/usr/bin/env bash\nkill -TERM \"$PPID\"\nsleep 1\nexit 1\n",
      ),
    ]);
    await Promise.all([
      chmod(join(scripts, "forecast-training-export.sh"), 0o700),
      chmod(join(binaries, "mktemp"), 0o700),
      chmod(join(binaries, "docker"), 0o700),
    ]);
    const result = spawnSync(
      "unshare",
      ["-Ur", join(scripts, "forecast-training-export.sh"), "2026-08-23", "2026-08-24"],
      {
        cwd: directory,
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${binaries}:${process.env.PATH}`,
          WEATHER_TEST_TEMP: temporary,
        },
      },
    );
    assert.notEqual(result.status, 0);
    assert.equal((await readdir(directory)).includes("producer-temporary"), false);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test(
  "disposable PostgreSQL exports and pulls one verified two-date package",
  {
    skip: runDeployIntegration ? false : "set WEATHER_RUN_DEPLOY_INTEGRATION=1",
    timeout: 300_000,
  },
  async (context) => {
    const directory = await mkdtemp(join(tmpdir(), "weather-training-database-smoke-"));
    let server;
    let pool;

    try {
      server = await startPostgres(17, "forecast-training-export");
      const database = await bootstrapDisposableExportDatabase(server, directory);
      pool = createTestPool(server);
      await seedPoisonObservation(pool);
      const harness = await installDatabaseExportHarness(directory);
      const archive = join(directory, "database-export.tar.gz");
      const namespaceAvailable = spawnSync("unshare", ["-Ur", "true"], {
        encoding: "utf8",
      }).status === 0;
      const sudoAvailable = spawnSync("sudo", ["--non-interactive", "true"], {
        encoding: "utf8",
      }).status === 0;

      // require one isolated privileged launcher
      if (!namespaceAvailable && !sudoAvailable) {
        context.skip("a privileged export launcher is unavailable");
        return;
      }

      const exportEnvironment = {
        ...process.env,
        PATH: `${harness.binaries}:${process.env.PATH}`,
        WEATHER_TEST_DOCKER_REAL: database.dockerExecutable,
        WEATHER_TEST_EXPORT_SECRET: database.trainingSecret,
        WEATHER_TEST_POSTGRES_CONTAINER: server.name,
      };
      const exportCommand = namespaceAvailable ? "unshare" : "sudo";
      const exportArguments = namespaceAvailable
        ? [
            "-Ur",
            join(harness.scripts, "forecast-training-export.sh"),
            "2026-08-23",
            "2026-08-24",
          ]
        : [
            "--non-interactive",
            "/usr/bin/env",
            `PATH=${exportEnvironment.PATH}`,
            `WEATHER_TEST_DOCKER_REAL=${exportEnvironment.WEATHER_TEST_DOCKER_REAL}`,
            `WEATHER_TEST_EXPORT_SECRET=${exportEnvironment.WEATHER_TEST_EXPORT_SECRET}`,
            `WEATHER_TEST_POSTGRES_CONTAINER=${exportEnvironment.WEATHER_TEST_POSTGRES_CONTAINER}`,
            join(harness.scripts, "forecast-training-export.sh"),
            "2026-08-23",
            "2026-08-24",
          ];
      const exported = spawnSync(
        exportCommand,
        exportArguments,
        {
          cwd: directory,
          env: exportEnvironment,
          maxBuffer: 32 * 1024 * 1024,
        },
      );
      assert.equal(exported.status, 0, exported.stderr.toString("utf8"));
      assert.equal(exported.stdout.subarray(0, 2).toString("hex"), "1f8b");
      assert.doesNotMatch(
        exported.stderr.toString("utf8"),
        /serial-password-poison|provider-password-poison|training-export-smoke/u,
      );
      await writeFile(archive, exported.stdout);
      await writeFile(
        join(harness.scripts, "ssh-run.sh"),
        "#!/usr/bin/env bash\ncat -- \"${WEATHER_TEST_ARCHIVE:?}\"\n",
      );
      await chmod(join(harness.scripts, "ssh-run.sh"), 0o700);
      const pulled = spawnSync(
        join(harness.scripts, "pull-forecast-training-export.sh"),
        ["2026-08-23", "2026-08-24"],
        {
          cwd: directory,
          encoding: "utf8",
          env: { ...process.env, WEATHER_TEST_ARCHIVE: archive },
        },
      );
      assert.equal(pulled.status, 0, pulled.stderr);
      const snapshots = await readdir(join(directory, ".weather-data"));
      assert.equal(snapshots.length, 1);
      const snapshot = join(directory, ".weather-data", snapshots[0]);
      const manifest = JSON.parse(await readFile(join(snapshot, "manifest.json"), "utf8"));
      assert.equal(manifest.totalRowCount, 528);
      assert.deepEqual(manifest.transaction, {
        idleInTransactionSessionTimeout: "30s",
        isolationLevel: "repeatable read",
        lockTimeout: "5s",
        readOnly: "on",
        statementTimeout: "15min",
      });
      assert.deepEqual(
        manifest.members.map((member) => member.path),
        manifest.members.map((member) => member.path).sort(),
      );

      // verify every published member hash
      for (const member of manifest.members) {
        const bytes = await readFile(join(snapshot, member.path));
        assert.equal(createHash("sha256").update(bytes).digest("hex"), member.sha256);
      }

      const poisonMember = manifest.members.find((member) =>
        member.localDate === "2026-08-24" &&
        member.recordKind === "station-hour" &&
        member.stationKey === "tempest-38270");
      assert.notEqual(poisonMember, undefined);
      const poisonRows = gunzipSync(await readFile(join(snapshot, poisonMember.path)))
        .toString("utf8")
        .trimEnd()
        .split("\n")
        .map((line) => JSON.parse(line));
      const poison = poisonRows.find((row) =>
        row.valid_at === "2026-08-24T08:00:00.000000Z");
      assert.notEqual(poison, undefined);
      assert.deepEqual(poison.exclusion_reason_codes, [
        "metric_missing",
        "quality_flag_rejected",
        "station_coverage_insufficient",
      ]);
      assert.deepEqual(poison.source_keys, []);
      assert.deepEqual(poison.content_hashes, []);
      assert.doesNotMatch(
        JSON.stringify({ manifest, poison }),
        /serial-password-poison|provider-password-poison|training-export-smoke/u,
      );
    } finally {
      // close only the disposable database
      if (pool !== undefined) {
        await pool.end();
      }

      // stop only the disposable postgres container
      if (server !== undefined) {
        await stopPostgres(server);
      }

      await rm(directory, { force: true, recursive: true });
    }
  },
);

test("local pull publishes atomically and rejects tampered or duplicate snapshots", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-atomic-pull-"));

  try {
    const fixtureRoot = join(directory, "fixture");
    await mkdir(fixtureRoot);
    const fixture = await buildFixture(fixtureRoot);
    const archive = join(directory, "valid.tar.gz");
    createArchive(fixture.output, archive);
    const scripts = await installPullHarness(directory);
    const environment = { ...process.env, WEATHER_TEST_ARCHIVE: archive };
    const first = spawnSync(
      join(scripts, "pull-forecast-training-export.sh"),
      ["2026-08-23", "2026-08-24"],
      { cwd: directory, encoding: "utf8", env: environment },
    );
    assert.equal(first.status, 0, first.stderr);
    assert.match(first.stdout, new RegExp(`${fixture.hash}\\n$`, "u"));
    assert.deepEqual(await readdir(join(directory, ".weather-data")), [fixture.hash]);

    const duplicate = spawnSync(
      join(scripts, "pull-forecast-training-export.sh"),
      ["2026-08-23", "2026-08-24"],
      { cwd: directory, encoding: "utf8", env: environment },
    );
    assert.notEqual(duplicate.status, 0);
    assert.match(duplicate.stderr, /already exists/u);
    assert.deepEqual(await readdir(join(directory, ".weather-data")), [fixture.hash]);

    const tamperRoot = join(directory, "tamper-fixture");
    await mkdir(tamperRoot);
    const tampered = await buildFixture(tamperRoot);
    const tamperedManifest = JSON.parse(
      await readFile(join(tampered.output, "manifest.json"), "utf8"),
    );
    const tamperedMember = join(tampered.output, tamperedManifest.members[0].path);
    const tamperedBytes = await readFile(tamperedMember);
    tamperedBytes[tamperedBytes.length - 1] ^= 1;
    await writeFile(tamperedMember, tamperedBytes);
    const tamperedArchive = join(directory, "tampered.tar.gz");
    createArchive(tampered.output, tamperedArchive);
    const rejected = spawnSync(
      join(scripts, "pull-forecast-training-export.sh"),
      ["2026-08-23", "2026-08-24"],
      {
        cwd: directory,
        encoding: "utf8",
        env: { ...process.env, WEATHER_TEST_ARCHIVE: tamperedArchive },
      },
    );
    assert.notEqual(rejected.status, 0);
    assert.deepEqual(await readdir(join(directory, ".weather-data")), [fixture.hash]);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("local pull refuses a destination created during publication", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-training-raced-pull-"));

  try {
    const fixtureRoot = join(directory, "fixture");
    await mkdir(fixtureRoot);
    const fixture = await buildFixture(fixtureRoot);
    const archive = join(directory, "valid.tar.gz");
    createArchive(fixture.output, archive);
    const scripts = await installPullHarness(directory);
    const binaries = join(directory, "bin");
    await mkdir(binaries);
    await writeFile(
      join(binaries, "mv"),
      "#!/usr/bin/env bash\ndestination=${!#}\nmkdir -p -- \"$destination\"\nprintf 'racer\\n' >\"$destination/race-marker\"\nexec /usr/bin/mv \"$@\"\n",
    );
    await chmod(join(binaries, "mv"), 0o700);
    const result = spawnSync(
      join(scripts, "pull-forecast-training-export.sh"),
      ["2026-08-23", "2026-08-24"],
      {
        cwd: directory,
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${binaries}:${process.env.PATH}`,
          WEATHER_TEST_ARCHIVE: archive,
        },
      },
    );
    assert.notEqual(result.status, 0);
    assert.equal(
      await readFile(join(directory, ".weather-data", fixture.hash, "race-marker"), "utf8"),
      "racer\n",
    );
    assert.deepEqual(
      await readdir(join(directory, ".weather-data", fixture.hash)),
      ["race-marker"],
    );
    assert.deepEqual(await readdir(join(directory, ".weather-data")), [fixture.hash]);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("remote contracts expose no caller-selected database, source, SQL, or path", async () => {
  const files = await Promise.all([
    readFile(exportScript, "utf8"),
    readFile(pullScript, "utf8"),
    readFile(join(repoRoot, "deploy/scripts/remote-ops.sh"), "utf8"),
    readFile(join(repoRoot, "deploy/scripts/ssh-run.sh"), "utf8"),
  ]);
  const joined = files.join("\n");
  assert.match(joined, /forecast-training-export/u);
  assert.match(joined, /forecast_training_export_rows_v1/u);
  assert.match(joined, /forecast_training_export_manifest_v1/u);
  assert.match(joined, /READ ONLY/u);
  assert.match(joined, /REPEATABLE READ/u);
  assert.match(
    files[0],
    /SET LOCAL weather\.forecast_training_from_date TO :'from_date'/u,
  );
  assert.match(
    files[0],
    /SET LOCAL weather\.forecast_training_to_date TO :'to_date'/u,
  );
  assert.equal(files[0].match(/^COPY \(/gmu)?.length, 1);
  assert.equal(
    files[0].match(/^BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;/gmu)
      ?.length,
    1,
  );
  assert.match(joined, /LIMIT 4000001/u);
  assert.ok(files[0].indexOf("manifest_hash=$(node") < files[0].indexOf("tar --create"));
  assert.match(files[1], /mv --no-copy --no-target-directory --update=none/u);
  assert.match(files[1], /forecast-training snapshot publication raced/u);
  assert.doesNotMatch(joined, /--(?:database-url|source-key|sql|output)\b/u);
  assert.doesNotMatch(joined, /eval\s/u);

  const ignored = await readFile(join(repoRoot, ".gitignore"), "utf8");
  assert.match(ignored, /^\.weather-data\/$/mu);
  assert.match(ignored, /^\.weather-models\/$/mu);

  const forbiddenSurfaces = [
    "apps/api",
    "apps/web",
    "deploy/scripts/migrate.mjs",
  ];

  // keep evidence and export keys outside application surfaces
  for (const surface of forbiddenSurfaces) {
    const result = spawnSync("git", [
      "grep",
      "--extended-regexp",
      "weather_training_export_password|\\.weather-data|model-evidence",
      "--",
      surface,
    ], { cwd: repoRoot, encoding: "utf8" });
    // bind the sole private-value denial match to the complete reviewed public validator
    const validatorHash = surface === "apps/web"
      ? createHash("sha256").update(await readFile(join(repoRoot,
        "apps/web/src/adjustment-maintenance-contract.mjs"))).digest("hex")
      : undefined;
    assertNoApplicationExportAccess(result, surface, validatorHash);
  }

  const workerSecrets = spawnSync("git", [
    "grep",
    "--extended-regexp",
    "weather_training_export_password|model-evidence",
    "--",
    "apps/worker",
  ], { cwd: repoRoot, encoding: "utf8" });
  assert.equal(workerSecrets.status, 1, "apps/worker gained export key or evidence-store access");
});
