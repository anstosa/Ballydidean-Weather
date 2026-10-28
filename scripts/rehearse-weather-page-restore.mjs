import { spawn } from "node:child_process";
import { createWriteStream } from "node:fs";
import { chmod, chown, cp, lstat, mkdir, readFile, rm, stat, statfs, writeFile } from "node:fs/promises";
import { arch } from "node:os";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { runMigrations } from "../packages/database/dist/index.js";
import { createTestPool, prepareRuntimeRoles } from "../packages/database/test/postgres-harness.mjs";
import { buildWeatherPageIndexPackage } from "./package-weather-page-indexes.mjs";
import {
  deriveBurstReplayEvidence,
  runCommand,
  sealAdmission,
  sha256,
  stableJson,
  validateIngestionReport,
  validateIndexManifest,
  validateRehearsalReport,
} from "./weather-page-indexes.mjs";

const INSTALL_ROOT = "/var/lib/weather/maintenance/page-read-index-v1";
const FIXTURE_RELEASE = "2099.01.01-99";
const POSTGRES_IMAGE = "postgres:17.10-bookworm";
const FIXTURE_DATABASE = "weather_test";
const IDENTITY_KEYS = ["aclDigest", "catalogDigest", "controlDigest", "databaseDigest", "forecastSettingsDigest", "imageDigest", "ledgerDigest", "manifestDigest", "packageDigest", "settingsDigest", "sourceDigest"];
const PEAK_FIELDS = ["indexBytesHighWater", "tempBytesHighWater", "walGeneratedBytes", "walRetainedBytesHighWater", "inodesHighWater"];

// require one successful child command
async function execute(file, argumentsList, options = {}) {
  const result = await runCommand(file, argumentsList, {
    cwd: options.cwd,
    env: options.env,
    timeoutMilliseconds: options.timeoutMilliseconds ?? 180_000,
    allowFailure: true,
  });

  // reject every failed command
  if (result.code !== 0) {
    throw new Error(`${basename(file)} failed (${String(result.code)}): ${result.stderr.trim()}`);
  }
  return result;
}

// parse the root-only factory interface
export function parseRestoreRehearsalArguments(argv) {
  // require the one closed option pair
  if (argv.length !== 2 || argv[0] !== "--output") {
    throw new Error("usage: rehearse-weather-page-restore.mjs --output ABS_PRIVATE_DIR");
  }
  const output = String(argv[1]);

  // require a canonical absolute destination
  if (!isAbsolute(output) || resolve(output) !== output) {
    throw new Error("restore rehearsal output must be an absolute canonical path");
  }
  return { output };
}

// write one new private file
async function writePrivate(path, bytes) {
  await writeFile(path, bytes, { flag: "wx", mode: 0o600 });
  await chmod(path, 0o600);
  return path;
}

// write one immutable private json artifact
async function writePrivateJson(path, value) {
  const bytes = Buffer.from(`${JSON.stringify(value, null, 2)}\n`, "utf8");
  await writePrivate(path, bytes);
  return { path, bytes, digest: sha256(bytes) };
}

// write one concise result and detached digest
async function writeResult(outputRoot, value) {
  const path = join(outputRoot, "result.json");
  const result = await writePrivateJson(path, value);
  await writePrivate(`${path}.sha256`, `${result.digest}  result.json\n`);
}

// run one scoped git command
async function git(repository, argumentsList, options = {}) {
  return execute("git", ["-c", `safe.directory=${repository}`, "-C", repository, ...argumentsList], options);
}

// prepare a clean local tagged package source
export async function prepareRepository(sourceRoot, fixtureRoot, options = {}) {
  const status = await execute("git", ["-c", `safe.directory=${sourceRoot}`, "-C", sourceRoot, "status", "--porcelain=v1"], options);
  if (status.stdout.trim() !== "") {
    throw new Error("restore rehearsal requires a clean exact source checkout");
  }
  const repository = join(fixtureRoot, "repository");
  await execute("git", ["-c", `safe.directory=${sourceRoot}`, "clone", "--no-hardlinks", "--quiet", sourceRoot, repository], options);
  await git(repository, ["remote", "set-url", "origin", "git@github.com:anstosa/weather.git"], options);
  await git(repository, [
    "-c", "user.name=Weather Restore Rehearsal",
    "-c", "user.email=weather-restore-rehearsal@ballydidean.farm",
    "tag", "--annotate", FIXTURE_RELEASE, "--message", "private restore rehearsal fixture",
  ], options);
  return {
    repository,
    commit: (await git(repository, ["rev-parse", `${FIXTURE_RELEASE}^{commit}`], options)).stdout.trim(),
  };
}

// return fixture-only publication evidence
async function verifyFixturePublication(input) {
  return {
    repository: input.repository,
    canonicalRepository: "anstosa/Ballydidean-Weather",
    repositoryId: 1_342_404_160,
    tag: input.tag,
    commit: input.commit,
    tree: input.tree,
    tagObject: input.tagObject,
    checkRunId: 1,
    checkRunAttempt: 1,
    publicationRunId: 1,
    publicationRunAttempt: 1,
  };
}

// create the fixed fresh installer ancestry
async function prepareInstallRoot() {
  try {
    await lstat(INSTALL_ROOT);
    throw new Error(`restore rehearsal requires absent install root: ${INSTALL_ROOT}`);
  } catch (error) {
    // accept only a missing root
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
  await mkdir(dirname(dirname(INSTALL_ROOT)), { recursive: true, mode: 0o755 });
  await mkdir(dirname(INSTALL_ROOT), { recursive: true, mode: 0o755 });
  await mkdir(INSTALL_ROOT, { mode: 0o700 });
  await chown(INSTALL_ROOT, 0, 0);
  await chmod(INSTALL_ROOT, 0o700);
  const info = await lstat(INSTALL_ROOT);
  return { dev: info.dev, ino: info.ino };
}

// install one exact generated package
async function installPackage(build) {
  const incoming = join(INSTALL_ROOT, `.incoming.fixture-${process.pid}`);
  await mkdir(incoming, { mode: 0o700 });
  await chown(incoming, 0, 0);

  // copy exactly the trusted three installer inputs
  for (const name of [build.archive.name, build.restoreContext.name, build.installer.name]) {
    const destination = join(incoming, name);
    await cp(join(build.outputDirectory, name), destination, { errorOnExist: true, force: false });
    await chown(destination, 0, 0);
    await chmod(destination, 0o600);
  }
  await execute("/usr/bin/bash", [
    join(incoming, build.installer.name), "install",
    "--incoming-root", incoming,
    "--archive-sha256", build.archive.digest,
    "--restore-sha256", build.restoreContext.digest,
    "--installer-sha256", build.installer.digest,
    "--release", build.tag,
    "--commit", build.commit,
  ]);
  return {
    helper: join(INSTALL_ROOT, "releases", `${build.tag}-${build.commit.slice(0, 12)}`, "weather-page-indexes.mjs"),
    context: join(INSTALL_ROOT, "restore-contexts", "2026.10.09-1-56b327d9c750"),
  };
}

// create compose-only synthetic secrets in the allowlisted context
async function createSyntheticSecrets(contextRoot) {
  const names = ["cloudflare_tunnel_token", "weather_api_password", "weather_migration_owner_password", "weather_postgres_admin_password", "weather_postgres_api_password", "weather_postgres_ingest_password", "weather_postgres_owner_password", "weather_postgres_training_export_password", "weather_tempest_api_key", "weather_worker_ingest_password", "weather_xweather_client_id", "weather_xweather_client_secret"];
  const secretRoot = join(contextRoot, "deploy", "secrets");
  await mkdir(secretRoot, { mode: 0o700 });
  const paths = [];

  // create every private fixture secret
  for (const name of names) {
    const relative = `deploy/secrets/${name}`;
    await writePrivate(join(contextRoot, relative), "fixture-only\n");
    paths.push(relative);
  }
  return paths.sort();
}

// start one isolated compose-labelled PostgreSQL 17.10 container
async function startPostgres(fixtureRoot, composeProject) {
  const dataRoot = join(fixtureRoot, "postgres-data");
  await mkdir(dataRoot, { mode: 0o700 });
  await chown(dataRoot, 999, 999);
  const name = `${composeProject}-postgres-1`;
  await execute("docker", [
    "run", "--detach", "--rm", "--name", name,
    "--label", `com.docker.compose.project=${composeProject}`,
    "--label", "com.docker.compose.service=postgres",
    "--label", "com.docker.compose.container-number=1",
    "--label", "com.docker.compose.oneoff=False",
    "--env", "POSTGRES_PASSWORD=postgres-test",
    "--env", `POSTGRES_DB=${FIXTURE_DATABASE}`,
    "--publish", "127.0.0.1::5432",
    "--volume", `${dataRoot}:/var/lib/postgresql/data`,
    POSTGRES_IMAGE,
  ]);
  const port = Number((await execute("docker", ["inspect", "--format", '{{(index (index .NetworkSettings.Ports "5432/tcp") 0).HostPort}}', name])).stdout.trim());
  const server = { host: "127.0.0.1", name, password: "postgres-test", port, user: "postgres", dataRoot };
  let lastError;

  // wait for bounded container readiness
  for (let attempt = 0; attempt < 120; attempt += 1) {
    const pool = createTestPool(server);
    try {
      await pool.query("SELECT 1");
      await pool.end();
      const version = (await execute("docker", ["exec", name, "psql", "-U", "postgres", "-d", FIXTURE_DATABASE, "-Atc", "SHOW server_version_num"])).stdout.trim();
      if (version !== "170010") {
        throw new Error(`restore rehearsal requires PostgreSQL 17.10, found ${version}`);
      }
      return server;
    } catch (error) {
      lastError = error;
      await pool.end().catch(() => undefined);
      await new Promise((resolvePromise) => setTimeout(resolvePromise, 500));
    }
  }
  throw new Error("fixture PostgreSQL did not become ready", { cause: lastError });
}

// migrate and seed one realistic frozen-0018 fixture
async function seedDatabase(server, contextRoot) {
  const admin = createTestPool(server);
  await prepareRuntimeRoles(admin);
  const owner = createTestPool(server, FIXTURE_DATABASE, "weather_owner", "owner-test");
  try {
    await runMigrations(owner, join(contextRoot, "packages", "database", "migrations"));
    await owner.query(`
      INSERT INTO sites (slug, display_name, latitude, longitude, timezone) VALUES ('fixture', 'Fixture', 47, -122, 'UTC');
      INSERT INTO stations (site_id, slug, display_name, station_kind) VALUES (1, 'fixture', 'Fixture', 'physical');
      INSERT INTO providers (provider_key, display_name, attribution_label, attribution_url) VALUES ('fixture', 'Fixture', 'Fixture', 'https://example.invalid');
      INSERT INTO sources (station_id, provider_id, source_key, source_kind, material_provider_config, source_config_fingerprint, capabilities, cadence_seconds)
      VALUES (1, 1, 'fixture-source', 'physical_sensor', '{}'::jsonb, repeat('a', 64), '["current","historical"]'::jsonb, 60);
      INSERT INTO ingestion_runs (source_id, mode, requested_start, requested_end_exclusive, source_config_fingerprint, adapter_version, started_at, deadline_at, completed_at, state, attempts, record_count)
      VALUES (1, 'scheduled', '2026-01-01', '2026-02-01', repeat('a', 64), 'fixture-v1', '2026-02-01', '2026-02-01 01:00:00+00', '2026-02-01 00:30:00+00', 'succeeded', 1, 500000);
      INSERT INTO weather_records (source_id, source_kind, valid_at, product_run_at, first_ingestion_run_id, last_ingestion_run_id, first_received_at, last_received_at, upstream_timezone, content_hash, temperature_c)
      SELECT 1, 'physical_sensor', timestamptz '2026-01-01' + series * interval '1 minute', CASE WHEN series % 11 = 0 THEN NULL ELSE timestamptz '2025-12-31' + (series % 40) * interval '6 hours' END,
        1, 1, timestamptz '2026-02-01', timestamptz '2026-02-01', 'UTC', md5(series::text) || md5(series::text), 10 + (series % 20)
      FROM generate_series(1, 500000) AS series;
      INSERT INTO worker_heartbeats (worker_instance, last_loop_at, last_success_at, current_activity, worker_version)
      VALUES ('fixture-worker', clock_timestamp(), clock_timestamp(), 'idle', 'fixture-v1');
      ANALYZE weather_records;
    `);
  } finally {
    await owner.end();
    await admin.end();
  }
}

// read one WAL and temporary-file baseline
async function resourceBaseline(pool) {
  return (await pool.query(`SELECT pg_current_wal_insert_lsn()::text AS wal_lsn, temp_bytes::text AS temp_bytes
    FROM pg_stat_database WHERE datname = current_database()`)).rows[0];
}

// finish one positive measured resource envelope
async function finishMeasurement(pool, before, spec, elapsedMs, observed) {
  const row = (await pool.query(`SELECT
    pg_catalog.pg_wal_lsn_diff(pg_current_wal_insert_lsn(), $1)::bigint::text AS wal_bytes,
    GREATEST(temp_bytes - $2::bigint, 0)::text AS temp_bytes,
    COALESCE(pg_relation_size(to_regclass($3)), 0)::text AS index_bytes
    FROM pg_stat_database WHERE datname = current_database()`, [before.wal_lsn, before.temp_bytes, `public.${spec.name}`])).rows[0];
  const peaks = {
    indexBytesHighWater: Number(row.index_bytes),
    tempBytesHighWater: Number(row.temp_bytes),
    walGeneratedBytes: Number(row.wal_bytes),
    walRetainedBytesHighWater: 0,
    inodesHighWater: observed ? 1 : 0,
  };

  // reject unmeasured fixture resources
  if (!Number.isSafeInteger(peaks.walGeneratedBytes) || peaks.walGeneratedBytes <= 0 || peaks.inodesHighWater !== 1) {
    throw new Error(`fixture build did not produce a measurable resource envelope: ${spec.name}`);
  }
  return { elapsedMs, peaks };
}

// measure one valid concurrent build
async function measureValid(pool, spec, retain) {
  const before = await resourceBaseline(pool);
  const startedAt = performance.now();
  await pool.query(spec.createSql);
  const measured = await finishMeasurement(pool, before, spec, performance.now() - startedAt, true);

  // remove only nonretained fixture output
  if (!retain) {
    await pool.query(spec.dropSql);
  }
  if (measured.peaks.indexBytesHighWater <= 0) {
    throw new Error(`valid fixture index size is missing: ${spec.name}`);
  }
  return measured;
}

// measure one genuinely cancelled concurrent build
async function measureCancelled(pool, spec) {
  const blocker = await pool.connect();
  const builder = await pool.connect();
  const before = await resourceBaseline(pool);
  const startedAt = performance.now();
  let buildError;
  let buildFinished = false;
  let buildPromise = Promise.resolve();
  let cancellationRequested = false;
  let observed = false;

  try {
    await blocker.query("BEGIN ISOLATION LEVEL REPEATABLE READ");
    await blocker.query("SELECT count(*) FROM weather_records");
    buildPromise = builder.query(spec.createSql)
      .catch((error) => {
        buildError = error;
      })
      .finally(() => {
        buildFinished = true;
      });

    // wait until the real build is safely cancellable
    for (let attempt = 0; attempt < 1200; attempt += 1) {
      const status = (await pool.query(`SELECT to_regclass($1) IS NOT NULL AS observed,
        EXISTS (SELECT 1 FROM pg_stat_activity WHERE pid = $2 AND wait_event_type IS NOT NULL) AS waiting`,
      [`public.${spec.name}`, builder.processID])).rows[0];
      observed ||= status.observed;

      // cancel only a waiting backend with a catalog object
      if (observed && status.waiting) {
        const cancelled = (await pool.query("SELECT pg_cancel_backend($1) AS cancelled", [builder.processID])).rows[0].cancelled;
        if (cancelled !== true) {
          throw new Error(`fixture cancellation was refused: ${spec.name}`);
        }
        cancellationRequested = true;
        break;
      }
      // stop polling a build that completed unexpectedly
      if (buildFinished) {
        break;
      }
      await new Promise((resolvePromise) => setTimeout(resolvePromise, 25));
    }
    // terminate one unbounded fixture attempt before returning
    if (!cancellationRequested) {
      await blocker.query("ROLLBACK");
      if (!buildFinished) {
        await pool.query("SELECT pg_cancel_backend($1)", [builder.processID]);
      }
      await buildPromise;
      throw new Error(`fixture build did not reach the cancellable phase: ${spec.name}`);
    }
    await buildPromise;
    if (buildError?.code !== "57014") {
      throw new Error(`fixture build was not cancelled with 57014: ${spec.name}`);
    }
    const measured = await finishMeasurement(pool, before, spec, performance.now() - startedAt, observed);
    await blocker.query("ROLLBACK");
    const relation = (await pool.query("SELECT to_regclass($1) AS relation", [`public.${spec.name}`])).rows[0].relation;

    // remove the exact cancelled object
    if (relation !== null) {
      await pool.query(spec.dropSql);
    }
    return measured;
  } finally {
    // cancel any exceptional in-flight build before releasing its client
    if (!buildFinished) {
      await pool.query("SELECT pg_cancel_backend($1)", [builder.processID]).catch(() => undefined);
    }
    await blocker.query("ROLLBACK").catch(() => undefined);
    await buildPromise.catch(() => undefined);
    blocker.release();
    builder.release();
  }
}

// measure all four B1 build components
async function measureReplacement(server, manifest) {
  const pool = createTestPool(server, FIXTURE_DATABASE, "weather_owner", "owner-test");
  try {
    const targetValid = await measureValid(pool, manifest.replacement.target, false);
    const targetCancelled = await measureCancelled(pool, manifest.replacement.target);
    await pool.query(manifest.replacement.predecessor.dropSql);
    const predecessorValid = await measureValid(pool, manifest.replacement.predecessor, true);
    await pool.query(manifest.replacement.predecessor.dropSql);
    const predecessorCancelled = await measureCancelled(pool, manifest.replacement.predecessor);
    await pool.query(manifest.replacement.predecessor.createSql);
    return {
      resources: {
        targetValid: targetValid.peaks,
        targetCancelled: targetCancelled.peaks,
        predecessorValid: predecessorValid.peaks,
        predecessorCancelled: predecessorCancelled.peaks,
      },
      elapsedMs: targetValid.elapsedMs + predecessorValid.elapsedMs,
    };
  } finally {
    await pool.end();
  }
}

// combine the exact four measured component envelopes
function combinePeaks(resources) {
  const combined = {};

  // sum each conservative component maximum
  for (const field of PEAK_FIELDS) {
    combined[field] = Math.max(resources.targetValid[field], resources.targetCancelled[field])
      + Math.max(resources.predecessorValid[field], resources.predecessorCancelled[field]);
  }
  return combined;
}

// build one explicit fixture-only forecast phase
function forecastPhase() {
  const outcomes = Array.from({ length: 200 }, (_unused, position) => ({ position, rows: 240 }));
  return { batchCount: 200, durationsMs: Array(200).fill(1), p95Ms: 1, outcomes, outcomeSha256: sha256(JSON.stringify(outcomes)) };
}

// build one explicit fixture-only mixed phase
function burstPhase() {
  const batchMix = { forecast: 240, tideObservation: 24, tidePrediction: 24, physical: 50 };
  const outcomes = Array.from({ length: 20 }, () => ({ forecast: 240, tide_observation: 24, tide_prediction: 24, physical: 50 }));
  return { batchCount: 20, batchMix, durationsMs: Array(20).fill(1), p95Ms: 1, generatedWalBytes: 1, relationBytesDelta: 0, outcomes, outcomeSha256: sha256(JSON.stringify(outcomes)) };
}

// construct closed fixture-only admission reports
export function buildFixtureAdmissionReports({ manifest, resources, rows, heapBytes, capturedAt, elapsedMs = 1, fixtureSnapshot = undefined }) {
  const identities = Object.fromEntries(IDENTITY_KEYS.map((name, position) => [name, String(position + 1).padStart(64, "0")]));
  const originalCatalogSha256 = sha256(stableJson({ predecessor: manifest.replacement.predecessor, covering: manifest.replacement.covering, protected: manifest.replacement.protected }));
  const finalCatalogSha256 = sha256(stableJson({ target: manifest.replacement.target, covering: manifest.replacement.covering, protected: manifest.replacement.protected }));
  const forecast = forecastPhase();
  const burst = burstPhase();
  const rehearsal = {
    contractVersion: "weather-page-index-rehearsal/v1",
    fixtureOnly: true,
    qualification: false,
    pass: true,
    writes: { measured: false, fixtureContract: true, pass: true, p95Ratio: 1 },
    admission: {
      contractVersion: "weather-page-index-rehearsal-admission/v1",
      sourceIdentity: { tag: manifest.restoreContext.tag, commit: manifest.restoreContext.commit, tree: manifest.restoreContext.tree, migrationHistorySha256: sha256(stableJson(manifest.restoreContext.migrations)) },
      postgres: { major: 17 },
      index: { name: manifest.replacement.target.name, sql: manifest.replacement.target.createSql, maintenanceWorkMemBytes: 64 * 1024 * 1024, maxParallelMaintenanceWorkers: 0, lockTimeoutMs: 5000, statementTimeoutMs: 2_700_000 },
      replacement: {
        target: sha256(stableJson(manifest.replacement.target)),
        predecessor: sha256(stableJson(manifest.replacement.predecessor)),
        covering: sha256(stableJson(manifest.replacement.covering)),
        protected: sha256(stableJson(manifest.replacement.protected)),
        originalCatalogSha256,
        finalCatalogSha256,
        restoredCatalogSha256: originalCatalogSha256,
      },
      distribution: { forecast: 0, physical: rows, tide: 0, modelAndReanalysis: 0, total: rows },
      base: { rows, heapBytes, allIndexBytes: 1, totalBytes: heapBytes + 1, sampledRowBytes: 1, rowShapes: [{ sourceKind: "physical_sensor", rows }] },
      validBuild: { elapsedMs, ...combinePeaks(resources), finishedAt: capturedAt },
      failedBuild: { kind: "owned-backend-cancellation", errorCode: "57014", objectAbsent: true, resourcePeaks: resources.targetCancelled },
      replacementResources: resources,
      forecastReplay: { measured: false, fixtureContract: true, pass: true, outcomesEqual: true, p95Ratio: 1, baseline: forecast, candidate: forecast },
      burstReplay: { measured: false, fixtureContract: true, pass: true, outcomesEqual: true, p95Ratio: 1, baseline: burst, candidate: burst },
    },
  };
  validateRehearsalReport(rehearsal, manifest, { allowFixtureOnly: true });
  const rehearsalBytes = Buffer.from(`${JSON.stringify(rehearsal, null, 2)}\n`, "utf8");
  const rehearsalDigest = sha256(rehearsalBytes);
  const burstEvidence = deriveBurstReplayEvidence(rehearsal, rehearsalDigest, { allowFixtureOnly: true });
  const current = fixtureSnapshot ?? {
    walBytes: "0",
    heartbeats: [{ workerInstance: "fixture-worker", lastLoopAt: capturedAt }],
    filesystems: [{ id: "fixture-device", allocatedBytes: "0", allocatedInodes: "0", freeBytes: "1", freeInodes: "1" }],
  };
  const ingestion = {
    contractVersion: "weather-page-ingestion-admission/v1",
    fixtureOnly: true,
    qualification: false,
    contractOnly: true,
    identities,
    startedAt: capturedAt,
    finishedAt: capturedAt,
    durationSeconds: 0,
    cadenceMilliseconds: null,
    sampleCount: 1,
    events: { eventCoverageOrigin: "fixture-contract", forecast: false, tide: false, burstEnvelopeMeasured: false, liveRunIds: { forecast: [], tide: [] }, observedLiveRuns: [] },
    burstEvidence,
    loopP95Seconds: 2,
    heartbeat: { capturedAt, heartbeats: current.heartbeats },
    wal: { capturedAt, walBytes: current.walBytes },
    filesystems: current.filesystems.map((filesystem) => ({ ...filesystem, capturedAt, bytesPerSecond: "0", inodesPerSecond: "0" })),
  };
  validateIngestionReport(ingestion, identities, current.filesystems.map((filesystem) => ({ id: filesystem.id })), rehearsal, rehearsalDigest, { allowFixtureOnly: true });
  return { rehearsal, rehearsalBytes, rehearsalDigest, ingestion };
}

// stream one binary pg dump without text conversion
async function writeDump(container, destination) {
  const output = createWriteStream(destination, { flags: "wx", mode: 0o600 });
  const child = spawn("docker", ["exec", container, "pg_dump", "-U", "postgres", "-d", FIXTURE_DATABASE, "-Fc", "--no-owner"], { stdio: ["ignore", "pipe", "pipe"] });
  const errors = [];

  // collect bounded dump diagnostics
  child.stderr.on("data", (chunk) => {
    errors.push(chunk);
  });
  const childCompletion = new Promise((resolvePromise, rejectPromise) => {
    // reject spawn failures
    child.once("error", rejectPromise);
    // resolve child completion
    child.once("close", resolvePromise);
  });
  const outputCompletion = new Promise((resolvePromise, rejectPromise) => {
    // resolve output completion
    output.once("close", resolvePromise);
    // reject stream failures
    output.once("error", rejectPromise);
  });
  child.stdout.pipe(output);
  const [code] = await Promise.all([childCompletion, outputCompletion]);
  if (code !== 0) {
    throw new Error(`pg_dump failed: ${Buffer.concat(errors).toString("utf8")}`);
  }
  await chmod(destination, 0o600);
}

// encrypt one actual fixture dump
async function createArchive(container, evidenceRoot, name, identity, recipient) {
  const dump = join(evidenceRoot, `${name}.dump`);
  const path = join(evidenceRoot, `${name}.dump.age`);
  await writeDump(container, dump);
  await execute("age", ["--encrypt", "--recipient", recipient, "--output", path, dump]);
  await chmod(path, 0o600);
  await rm(dump, { force: true });
  const bytes = await readFile(path);
  const digest = sha256(bytes);
  await writePrivate(`${path}.sha256`, `${digest}  ${basename(path)}\n`);
  return { path, digest, size: bytes.length, identity };
}

// capture the exact base database identity
async function captureDatabase(server) {
  const pool = createTestPool(server);
  try {
    return (await pool.query(`SELECT json_build_object(
      'name', current_database(), 'oid', database_relation.oid::text,
      'owner', pg_get_userbyid(database_relation.datdba),
      'systemIdentifier', (SELECT system_identifier::text FROM pg_control_system()),
      'sizeBytes', pg_database_size(current_database())::text
    ) AS value FROM pg_database database_relation WHERE database_relation.datname = current_database()`)).rows[0].value;
  } finally {
    await pool.end();
  }
}

// capture one actual current fixture resource snapshot
async function captureFixtureSnapshot(server) {
  const pool = createTestPool(server);
  const filesystem = await statfs(server.dataRoot, { bigint: true });
  const metadata = await stat(server.dataRoot);
  try {
    const database = (await pool.query(`SELECT
      pg_catalog.pg_wal_lsn_diff(pg_current_wal_lsn(), '0/0')::bigint::text AS wal_bytes,
      COALESCE((SELECT json_agg(json_build_object(
        'workerInstance', worker_instance,
        'lastLoopAt', last_loop_at,
        'lastSuccessAt', last_success_at,
        'currentActivity', current_activity,
        'workerVersion', worker_version,
        'updatedAt', updated_at
      ) ORDER BY worker_instance) FROM worker_heartbeats), '[]'::json) AS heartbeats`)).rows[0];
    return {
      capturedAt: new Date().toISOString(),
      walBytes: database.wal_bytes,
      heartbeats: database.heartbeats,
      filesystems: [{
        id: metadata.dev.toString(),
        allocatedBytes: ((filesystem.blocks - filesystem.bfree) * filesystem.bsize).toString(),
        allocatedInodes: (filesystem.files - filesystem.ffree).toString(),
        freeBytes: (filesystem.bavail * filesystem.bsize).toString(),
        freeInodes: filesystem.ffree.toString(),
      }],
    };
  } finally {
    await pool.end();
  }
}

// write one archive-bound source-size report
async function sourceSizeReport(evidenceRoot, archive, database, name) {
  const written = await writePrivateJson(join(evidenceRoot, `${name}-source-size.json`), {
    schema: "weather-page-restore-source-size/v1",
    fixtureOnly: true,
    capturedAt: new Date().toISOString(),
    archive: { digest: archive.digest, size: archive.size },
    database,
  });
  return { path: written.path, digest: written.digest };
}

// write one exact restore runtime configuration
async function writeConfig({ configRoot, name, archive, sourceSize, common }) {
  const value = {
    schema: "weather-page-index-runtime/v1",
    restoreOnly: true,
    paths: {
      stateRoot: common.stateRoot,
      installRoot: common.context,
      forecastSettings: common.forecastSettings,
      evidenceRoot: common.evidenceRoot,
    },
    postgres: {
      applicationName: "weather-page-index-v1",
      container: common.server.name,
      composeProject: common.composeProject,
      database: FIXTURE_DATABASE,
      queryTimeoutMilliseconds: 30_000,
      tableOwner: "weather_owner",
      user: "postgres",
    },
    restore: {
      fixtureOnlyAdmission: true,
      owner: "weather_owner",
      fixtureDatabase: FIXTURE_DATABASE,
      composeProject: common.composeProject,
      postgresContainer: common.server.name,
      weatherPostgresDir: common.server.dataRoot,
      fixtureEnv: common.fixtureEnv,
      identity: common.identity,
      archive: archive.path,
      sourceSizeReport: sourceSize.path,
      sourceSizeDigest: sourceSize.digest,
      syntheticSecretFiles: common.syntheticSecretFiles,
    },
  };
  return (await writePrivateJson(join(configRoot, `${name}.json`), value)).path;
}

// create the source final intermediate and failure scenarios
async function prepareScenario({ fixtureRoot, evidenceRoot, configRoot, context, helper, server, composeProject, manifest, admission, syntheticSecretFiles }) {
  const identity = join(evidenceRoot, "fixture-age-identity.txt");
  await execute("age-keygen", ["--output", identity]);
  await chmod(identity, 0o600);
  const recipient = (await execute("age-keygen", ["--y", identity])).stdout.trim();
  const fixtureEnv = join(evidenceRoot, "fixture.env");
  const zero = "0".repeat(64);
  await writePrivate(fixtureEnv, [
    `POSTGRES_IMAGE=${POSTGRES_IMAGE}`,
    `WEATHER_SERVER_IMAGE=example.invalid/server@sha256:${zero}`,
    `WEATHER_WEB_IMAGE=example.invalid/web@sha256:${zero}`,
    `CLOUDFLARED_IMAGE=example.invalid/cloudflared@sha256:${zero}`,
    `WEATHER_RELEASE=${FIXTURE_RELEASE}`,
    `WEATHER_DATABASE_NAME=${FIXTURE_DATABASE}`,
    "",
  ].join("\n"));
  const pool = createTestPool(server, FIXTURE_DATABASE, "weather_owner", "owner-test");
  const source = await createArchive(server.name, evidenceRoot, "source", identity, recipient);
  await pool.query(manifest.replacement.target.createSql);
  await pool.query(manifest.replacement.predecessor.dropSql);
  const final = await createArchive(server.name, evidenceRoot, "final", identity, recipient);
  await pool.query(manifest.replacement.predecessor.createSql);
  const intermediate = await createArchive(server.name, evidenceRoot, "intermediate", identity, recipient);
  await pool.query(manifest.replacement.target.dropSql);
  await pool.end();
  const database = await captureDatabase(server);
  const badBytes = Buffer.from("fixture-only-invalid-age-payload\n", "utf8");
  const badPath = join(evidenceRoot, "bad.dump.age");
  await writePrivate(badPath, badBytes);
  const bad = { path: badPath, digest: sha256(badBytes), size: badBytes.length };
  await writePrivate(`${badPath}.sha256`, `${bad.digest}  ${basename(badPath)}\n`);
  const stateRoot = join(fixtureRoot, "state");
  const forecastSettings = join(fixtureRoot, "unused-forecast-settings.json");
  await mkdir(stateRoot, { mode: 0o700 });
  await writePrivate(forecastSettings, "{}\n");
  const common = { context, stateRoot, forecastSettings, evidenceRoot, server, composeProject, fixtureEnv, identity, syntheticSecretFiles };
  const noIndexConfig = await writeConfig({ configRoot, name: "source", archive: source, sourceSize: await sourceSizeReport(evidenceRoot, source, database, "source"), common });
  const indexedConfig = await writeConfig({ configRoot, name: "final", archive: final, sourceSize: await sourceSizeReport(evidenceRoot, final, database, "final"), common });
  const intermediateConfig = await writeConfig({ configRoot, name: "intermediate", archive: intermediate, sourceSize: await sourceSizeReport(evidenceRoot, intermediate, database, "intermediate"), common });
  const badArchiveConfig = await writeConfig({ configRoot, name: "bad", archive: bad, sourceSize: await sourceSizeReport(evidenceRoot, bad, database, "bad"), common });
  const scenario = {
    schema: "weather-page-index-restore-scenario/v2",
    fixtureOnly: true,
    performanceQualification: false,
    helper,
    context,
    admission,
    postgresContainer: server.name,
    fixtureDatabase: FIXTURE_DATABASE,
    foreignDatabase: `${FIXTURE_DATABASE}_verify_foreign`,
    noIndexConfig,
    indexedConfig,
    intermediateConfig,
    badArchiveConfig,
  };
  const scenarioPath = join(fixtureRoot, "scenario.json");
  await writePrivateJson(scenarioPath, scenario);
  return scenarioPath;
}

// remove only the exact installer root created here
async function cleanupInstall(identity) {
  const current = await lstat(INSTALL_ROOT);
  if (current.dev !== identity.dev || current.ino !== identity.ino) {
    throw new Error("installer root identity changed before cleanup");
  }
  await rm(INSTALL_ROOT, { recursive: true, force: false });
}

// execute the complete root-only restore rehearsal
export async function runRestoreRehearsal(options) {
  if (typeof process.getuid !== "function" || process.getuid() !== 0) {
    throw new Error("restore rehearsal requires root");
  }
  const sourceRoot = resolve(import.meta.dirname, "..");
  await Promise.all([
    execute("docker", ["info", "--format", "{{.ServerVersion}}"]),
    execute("age", ["--version"]),
    execute("age-keygen", ["--version"]),
  ]);
  try {
    await lstat(options.output);
    throw new Error("restore rehearsal output already exists");
  } catch (error) {
    // accept only one new output root
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
  await mkdir(options.output, { mode: 0o700 });
  await chown(options.output, 0, 0);
  await chmod(options.output, 0o700);
  const packageRoot = join(options.output, "package");
  const evidenceRoot = join(options.output, "evidence");
  const configRoot = join(options.output, "configs");
  await mkdir(packageRoot, { mode: 0o700 });
  await mkdir(evidenceRoot, { mode: 0o700 });
  await mkdir(configRoot, { mode: 0o700 });
  let server;
  let installIdentity;

  try {
    const repository = await prepareRepository(sourceRoot, options.output);
    const build = await buildWeatherPageIndexPackage({
      repositoryPath: repository.repository,
      release: FIXTURE_RELEASE,
      outputDirectory: packageRoot,
      verifyPublication: verifyFixturePublication,
    });
    installIdentity = await prepareInstallRoot();
    const installed = await installPackage(build);
    const syntheticSecretFiles = await createSyntheticSecrets(installed.context);
    const manifest = validateIndexManifest(JSON.parse(await readFile(join(dirname(installed.helper), "weather-page-indexes.json"), "utf8")));
    const composeProject = `pageidx_restore_${process.pid}`;
    server = await startPostgres(options.output, composeProject);
    await seedDatabase(server, installed.context);
    const measurement = await measureReplacement(server, manifest);
    const metricsPool = createTestPool(server);
    const metrics = (await metricsPool.query("SELECT count(*)::int AS rows, pg_relation_size('weather_records')::int AS heap_bytes FROM weather_records")).rows[0];
    await metricsPool.end();
    const fixtureSnapshot = await captureFixtureSnapshot(server);
    const capturedAt = fixtureSnapshot.capturedAt;
    const reports = buildFixtureAdmissionReports({
      manifest,
      resources: measurement.resources,
      rows: metrics.rows,
      heapBytes: metrics.heap_bytes,
      capturedAt,
      elapsedMs: measurement.elapsedMs,
      fixtureSnapshot,
    });
    const rehearsalPath = join(evidenceRoot, "fixture-rehearsal.json");
    await writePrivate(rehearsalPath, reports.rehearsalBytes);
    await writePrivate(`${rehearsalPath}.sha256`, `${reports.rehearsalDigest}  ${basename(rehearsalPath)}\n`);
    const ingestion = await writePrivateJson(join(evidenceRoot, "fixture-ingestion.json"), reports.ingestion);
    await writePrivate(`${ingestion.path}.sha256`, `${ingestion.digest}  ${basename(ingestion.path)}\n`);
    const admissionValue = sealAdmission({
      schema: "weather-page-index-admission/v1",
      fixtureOnly: true,
      identities: reports.ingestion.identities,
      evidence: {
        rehearsal: { schema: reports.rehearsal.contractVersion, path: rehearsalPath, digest: reports.rehearsalDigest, capturedAt },
        ingestion: { schema: reports.ingestion.contractVersion, path: ingestion.path, digest: ingestion.digest, capturedAt },
      },
    });
    const admission = (await writePrivateJson(join(evidenceRoot, "fixture-admission.json"), admissionValue)).path;
    const scenario = await prepareScenario({
      fixtureRoot: options.output,
      evidenceRoot,
      configRoot,
      context: installed.context,
      helper: installed.helper,
      server,
      composeProject,
      manifest,
      admission,
      syntheticSecretFiles,
    });
    const integration = await runCommand(process.execPath, ["--test", "--test-concurrency=1", "packages/database/test/page-indexes.integration.test.mjs"], {
      cwd: sourceRoot,
      env: { ...process.env, WEATHER_PAGE_INDEX_RESTORE_E2E_SCENARIO: scenario },
      allowFailure: true,
      timeoutMilliseconds: 900_000,
    });
    await writePrivate(join(options.output, "integration.stdout.log"), integration.stdout);
    await writePrivate(join(options.output, "integration.stderr.log"), integration.stderr);
    if (integration.code !== 0) {
      throw new Error(`restore integration failed with exit ${String(integration.code)}`);
    }
    await execute("docker", ["rm", "--force", server.name]);
    server = undefined;
    await cleanupInstall(installIdentity);
    installIdentity = undefined;
    const result = {
      schema: "weather-page-index-restore-rehearsal-result/v1",
      status: "passed",
      fixtureOnly: true,
      performanceQualification: false,
      defaultSwapSampleMilliseconds: 60_000,
      sourceCommit: repository.commit,
      release: build.tag,
      packageDigest: build.archive.digest,
      replacementResources: measurement.resources,
      scenarios: ["source-refusal", "source-reconcile", "final-verify", "intermediate-refusal", "decrypt-failure-cleanup", "foreign-preservation"],
      integration: { code: integration.code, stdoutDigest: sha256(integration.stdout), stderrDigest: sha256(integration.stderr) },
      completedAt: new Date().toISOString(),
      host: { architecture: arch() },
    };
    await writeResult(options.output, result);
    return result;
  } catch (error) {
    await writeResult(options.output, {
      schema: "weather-page-index-restore-rehearsal-result/v1",
      status: "failed",
      fixtureOnly: true,
      performanceQualification: false,
      completedAt: new Date().toISOString(),
      error: error instanceof Error ? error.message : String(error),
    }).catch(() => undefined);
    throw error;
  } finally {
    // stop only the exact fixture container
    if (server !== undefined) {
      await runCommand("docker", ["rm", "--force", server.name], { allowFailure: true, timeoutMilliseconds: 30_000 });
    }
    // remove only the exact fresh installer root
    if (installIdentity !== undefined) {
      await cleanupInstall(installIdentity);
    }
  }
}

// run only as the direct cli entrypoint
if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  runRestoreRehearsal(parseRestoreRehearsalArguments(process.argv.slice(2)))
    .then((result) => {
      process.stdout.write(`${JSON.stringify(result)}\n`);
    })
    .catch((error) => {
      process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    });
}
