import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { access, chmod, mkdir, mkdtemp, readFile, rename, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  acquireStateDirectoryLock,
  bindRestoreCandidate,
  calculateAdmission,
  classifyPredecessorCompensation,
  captureDeploymentSource,
  collectIngestionSamples,
  collectCanarySettings,
  deriveBurstReplayEvidence,
  ensureOwnedBackendTerminal,
  catalogFingerprintHash,
  evaluateAdmission,
  normalizeCatalogFingerprint,
  parseArguments,
  hashControlPlane,
  readRuntimeConfig,
  refreshAdmissionLiveSample,
  runCommand,
  sealAdmission,
  sha256,
  stableJson,
  startExclusiveIntervalSampler,
  summarizeIngestionSamples,
  validateAdmissionDigest,
  validateCatalogFingerprint,
  validateDisposableMount,
  validateIndexManifest,
  validateIngestionReport,
  validateOwnedIndex,
  validateRestoreCoexistence,
  validateReplacementInventory,
  validateReplacementOperationState,
  validateRecoveryInvariantIdentity,
  validateRuntimeMode,
} from "./weather-page-indexes.mjs";

const manifest = validateIndexManifest(JSON.parse(await readFile(new URL("./weather-page-indexes.json", import.meta.url), "utf8")));
const indexSpec = manifest.replacement.target;

// create one complete synthetic catalog tuple
function catalogFor(spec, overrides = {}) {
  const attributes = [...spec.keys, ...spec.include];
  const indexOid = overrides.indexOid ?? "18000";
  const tableOid = overrides.tableOid ?? "17000";
  const constraintOid = "19000";
  const indkey = attributes.map((_attribute, position) => position + 1);
  const constraintBacking = spec.constraint === null ? [] : [{ oid: constraintOid, schema: spec.schema, ...spec.constraint }];
  const dependencies = spec.constraint === null
    ? spec.dependencyColumns.map((name) => ({
        class: "pg_class",
        objectId: indexOid,
        objectSubId: 0,
        referencedClass: "pg_class",
        referencedObjectId: tableOid,
        referencedObjectSubId: attributes.findIndex((attribute) => attribute.name === name) + 1,
        type: "a",
      }))
    : [{
        class: "pg_class",
        objectId: indexOid,
        objectSubId: 0,
        referencedClass: "pg_constraint",
        referencedObjectId: constraintOid,
        referencedObjectSubId: 0,
        type: "i",
      }];
  return {
    database: "weather",
    databaseOid: "16384",
    schema: "public",
    table: "weather_records",
    tableOid,
    tableOwner: "weather_owner",
    indexSchema: "public",
    index: spec.name,
    indexOid,
    indexOwner: "weather_owner",
    accessMethod: "btree",
    keyCount: spec.keys.length,
    attributeCount: attributes.length,
    indkey,
    columns: attributes.map((attribute, position) => ({ position: position + 1, attributeNumber: position + 1, name: attribute.name, type: attribute.type })),
    opclasses: spec.keys.map((key, position) => ({ position: position + 1, oid: String(3124 + position), schema: key.opclassSchema, name: key.opclass, default: true })),
    collations: attributes.map((attribute, position) => attribute.collation === null || attribute.collation === undefined ? null : { position: position + 1, oid: "100", ...attribute.collation }),
    indoption: spec.keys.map((key) => (key.direction === "DESC" ? 1 : 0) | (key.nulls === "FIRST" ? 2 : 0)),
    ordering: attributes.map((attribute, position) => ({ position: position + 1, direction: attribute.direction ?? "ASC", nulls: attribute.nulls ?? "LAST" })),
    expressions: spec.expressions,
    predicate: spec.predicate,
    unique: spec.unique,
    primary: spec.primary,
    exclusion: spec.exclusion,
    immediate: spec.immediate,
    clustered: spec.clustered,
    checkXmin: spec.checkXmin,
    replicaIdentity: spec.replicaIdentity,
    nullsNotDistinct: spec.nullsNotDistinct,
    valid: true,
    ready: true,
    live: true,
    canonicalDefinition: spec.canonicalDefinition,
    storedTablespace: null,
    effectiveTablespace: "pg_default",
    reloptions: [],
    constraintBacking,
    dependencies,
    ...overrides,
  };
}

// create the target tuple by default
function catalog(overrides = {}) {
  return catalogFor(indexSpec, overrides);
}

// freeze the one allowlisted physical design
test("U-IDX-01 manifest and full catalog tuple are exact", () => {
  assert.equal(indexSpec.createSql.includes("IF NOT EXISTS"), false);
  assert.equal(indexSpec.dropSql.includes("CASCADE"), false);
  const actual = validateCatalogFingerprint(catalog(), indexSpec, "weather_owner");
  assert.equal(actual.indexOid, "18000");
  assert.match(catalogFingerprintHash(actual), /^[a-f0-9]{64}$/u);

  assert.throws(
    () => validateCatalogFingerprint(catalog({ ordering: [
      { position: 1, direction: "DESC", nulls: "FIRST" },
      { position: 2, direction: "ASC", nulls: "LAST" },
    ] }), indexSpec, "weather_owner"),
    /ordering 1 mismatch/u,
  );
  assert.throws(
    () => validateCatalogFingerprint(catalog({ constraintBacking: [{ name: "foreign" }] }), indexSpec, "weather_owner"),
    /backs a constraint/u,
  );
  assert.throws(() => validateCatalogFingerprint(catalog({ valid: false, ready: false }), indexSpec, "weather_owner"), /validity flags are incomplete/u);
  assert.equal(validateCatalogFingerprint(catalog({ valid: false, ready: false }), indexSpec, "weather_owner", { requireValid: false }).valid, false);
  const renamed = structuredClone(manifest);
  renamed.replacement.target.id = "foreign-id";
  assert.throws(() => validateIndexManifest(renamed), /ordering changed/u);
  const recollated = structuredClone(manifest);
  recollated.replacement.protected[1].keys[1].collation = null;
  assert.throws(() => validateIndexManifest(recollated), /keys changed/u);
});

// preserve dependency order independence only
test("U-IDX-01 catalog normalization sorts complete dependencies", () => {
  const first = catalog({ dependencies: [{ type: "n", objectId: "2" }, { type: "a", objectId: "1" }] });
  const second = catalog({ dependencies: [{ type: "a", objectId: "1" }, { type: "n", objectId: "2" }] });
  assert.deepEqual(normalizeCatalogFingerprint(first), normalizeCatalogFingerprint(second));
  assert.equal(catalogFingerprintHash(first), catalogFingerprintHash(second));
});

// freeze the B1 source, final, and compensation states
test("U-IDX-01 B1 replacement inventory and predecessor recovery are closed", () => {
  const replacement = manifest.replacement;
  const predecessor = catalogFor(replacement.predecessor, { indexOid: "18100" });
  const covering = catalogFor(replacement.covering, { indexOid: "18200" });
  const protectedIndexes = replacement.protected.map((spec, position) => catalogFor(spec, { indexOid: String(18300 + position) }));
  const extra = catalogFor(replacement.predecessor, { index: "weather_records_foreign_extra_idx", indexOid: "18400" });
  const source = [predecessor, covering, ...protectedIndexes, extra];
  const target = catalogFor(replacement.target, { indexOid: "18500" });
  const final = [target, covering, ...protectedIndexes, extra];
  assert.equal(validateReplacementInventory(source, manifest, "weather_owner", "source").target, null);
  assert.equal(validateReplacementInventory(final, manifest, "weather_owner", "final", source).target.indexOid, "18500");
  assert.throws(() => validateReplacementInventory([target, ...source], manifest, "weather_owner", "source"), /preexisting target forbids/u);
  assert.throws(() => validateReplacementInventory([...final.slice(0, -1), { ...extra, indexOid: "19400" }], manifest, "weather_owner", "final", source), /preserved index changed/u);
  assert.equal(classifyPredecessorCompensation(predecessor, predecessor, replacement.predecessor, "weather_owner"), "retain");
  assert.equal(classifyPredecessorCompensation({ ...predecessor, valid: false }, predecessor, replacement.predecessor, "weather_owner"), "drop-recreate");
  assert.equal(classifyPredecessorCompensation(null, predecessor, replacement.predecessor, "weather_owner"), "recreate");
  assert.equal(classifyPredecessorCompensation({ ...predecessor, indexOid: "19100" }, predecessor, replacement.predecessor, "weather_owner"), "mismatch");
  assert.equal(classifyPredecessorCompensation({ ...predecessor, predicate: "foreign" }, predecessor, replacement.predecessor, "weather_owner"), "mismatch");
});

// refuse crashed stale and incomplete durable replacement states
test("U-IDX-01 B1 operation journal admits only closed terminal states", () => {
  const operation = {
    schema: "weather-page-index-operation/v2",
    operationId: "12345678-1234-4123-8123-123456789abc",
    clusterDigest: "a".repeat(64),
    databaseOid: "1",
    tableOid: "2",
    sourceInventoryDigest: "b".repeat(64),
    sourceIdentities: { databaseDigest: "c".repeat(64) },
  };
  const result = { schema: "weather-page-index-result/v2", operationId: operation.operationId, status: "replaced", afterIdentities: operation.sourceIdentities };
  const completed = { ...operation, status: "completed", resultDigest: sha256(stableJson(result)) };
  const completion = { schema: "weather-page-index-completion/v1", operationId: operation.operationId, resultDigest: completed.resultDigest, beforeIdentities: operation.sourceIdentities, afterIdentities: result.afterIdentities };
  assert.equal(validateReplacementOperationState(operation, completed, result, undefined, undefined, completion), "active");
  assert.throws(() => validateReplacementOperationState(undefined, completed, result), /identity is missing/u);
  assert.throws(() => validateReplacementOperationState(operation, { ...completed, tableOid: "3" }, result), /state identity changed/u);
  assert.throws(() => validateReplacementOperationState(operation, { ...operation, status: "source-drop-started" }, result), /incomplete or unsafe/u);
  assert.throws(() => validateReplacementOperationState(operation, { ...operation, status: "failed-compensation-incomplete" }, { ...result, status: "failed-compensation-incomplete" }), /incomplete or unsafe/u);
  assert.throws(() => validateReplacementOperationState(operation, { ...operation, status: "revert-started" }, result, { schema: "weather-page-index-compensation/v1", operationId: operation.operationId }, undefined, completion), /incomplete or unsafe/u);
  assert.throws(() => validateReplacementOperationState(operation, { ...operation, status: "revert-compensation-incomplete" }, result, { schema: "weather-page-index-compensation/v1", operationId: operation.operationId }, undefined, completion), /incomplete or unsafe/u);
});

// bind the full acl and administrator settings identity to compensation
test("U-IDX-01 B1 compensation refuses administrator settings drift", () => {
  const value = {
    database: { oid: "1" },
    acl: { roles: [], columns: [], defaultAcls: [] },
    forecastSettings: { combinedDigest: "a".repeat(64), marker: { digest: "b".repeat(64) } },
  };
  const invariants = { value, digest: sha256(stableJson(value)) };
  const operation = {
    sourceAclDigest: sha256(stableJson(value.acl)),
    sourceForecastSettingsDigest: value.forecastSettings.combinedDigest,
    sourceInvariantDigest: invariants.digest,
  };
  assert.deepEqual(validateRecoveryInvariantIdentity(invariants, operation), {
    aclDigest: operation.sourceAclDigest,
    forecastSettingsDigest: operation.sourceForecastSettingsDigest,
  });
  const changedSettings = {
    value: { ...value, forecastSettings: { ...value.forecastSettings, combinedDigest: "c".repeat(64) } },
    digest: "d".repeat(64),
  };
  assert.throws(() => validateRecoveryInvariantIdentity(changedSettings, operation), /administrator forecast settings/u);
});

// calculate exact capacity formulas and equality admission
test("U-IDX-02 deterministic F J R H S D L admission", () => {
  const now = new Date("2026-10-28T16:00:00.000Z");
  const document = {
    schema: "weather-page-index-admission/v1",
    identities: {
      aclDigest: "1".repeat(64),
      catalogDigest: "2".repeat(64),
      controlDigest: "3".repeat(64),
      databaseDigest: "4".repeat(64),
      forecastSettingsDigest: "5".repeat(64),
      imageDigest: "6".repeat(64),
      ledgerDigest: "7".repeat(64),
      manifestDigest: "8".repeat(64),
      packageDigest: "9".repeat(64),
      settingsDigest: "a".repeat(64),
      sourceDigest: "b".repeat(64),
    },
    timestamps: {
      rehearsalMeasuredAt: "2026-10-28T15:00:00.000Z",
      ingestionMeasuredAt: "2026-10-28T15:30:00.000Z",
      liveSampledAt: "2026-10-28T15:59:30.000Z",
    },
    scale: {
      liveRows: "400",
      rehearsalRows: "100",
      liveHeapBytes: "250",
      rehearsalHeapBytes: "100",
      columnDistributionDigest: "e".repeat(64),
      buildConfigDigest: "f".repeat(64),
    },
    buildDurationSeconds: 5400,
    host: {
      baselineP95LoopSeconds: 75,
      fixtureWriteP95Ratio: 1,
    },
    filesystems: [{
      id: "2049",
      paths: ["/data", "/wal"],
      dockerShares: true,
      totalInodes: "100000",
      rehearsal: {
        indexBytes: "100",
        tempBytes: "20",
        walBytes: "30",
        allocatedInodes: "4",
      },
      growth: {
        bytesPerSecond: "1",
        inodesPerSecond: "1",
      },
    }],
  };
  const computed = calculateAdmission(document, manifest.admission);
  assert.equal(computed.scale, "4");
  assert.equal(computed.buildDurationSeconds, 5400);
  assert.equal(computed.heartbeatAllowanceSeconds, 150);
  assert.equal(computed.filesystems[0].protectedBytes, (10n * 1024n ** 3n + 2_034_155_520n).toString());
  assert.equal(computed.filesystems[0].protectedInodes, "32768");
  assert.equal(computed.filesystems[0].reserveBytes, (4n * 150n + 5400n + 256n * 1024n ** 2n).toString());
  assert.equal(computed.filesystems[0].reserveInodes, (4n * 4n + 5400n + 256n).toString());
  document.computed = computed;
  const sealed = sealAdmission(document);
  validateAdmissionDigest(sealed);
  const current = {
    identities: document.identities,
    scale: { liveRows: "404", liveHeapBytes: "252" },
    filesystems: [{
      id: "2049",
      freeBytes: computed.filesystems[0].requiredFreeBytes,
      freeInodes: computed.filesystems[0].requiredFreeInodes,
    }],
    host: {
      architecture: "arm64",
      cpuCount: 4,
      load15: 2,
      availableMemoryBytes: String(1792 * 1024 * 1024),
      swapBytesPerMinute: String(1024 * 1024),
      fixtureWriteP95Ratio: 1.1,
    },
  };
  assert.deepEqual(evaluateAdmission(sealed, manifest.admission, current, now).reasons, []);
  const staleArtifact = sealAdmission({
    ...document,
    timestamps: { ...document.timestamps, liveSampledAt: "2026-10-28T15:00:00.000Z" },
  });
  assert.match(evaluateAdmission(staleArtifact, manifest.admission, current, now).reasons.join(" "), /live filesystem sample expired/u);
  const refreshed = refreshAdmissionLiveSample(staleArtifact, { ...current, capturedAt: "2026-10-28T15:59:30.000Z" });
  assert.deepEqual(evaluateAdmission(refreshed, manifest.admission, current, now).reasons, []);
  const staleCurrent = refreshAdmissionLiveSample(staleArtifact, { ...current, capturedAt: "2026-10-28T15:00:00.000Z" });
  assert.match(evaluateAdmission(staleCurrent, manifest.admission, current, now).reasons.join(" "), /live filesystem sample expired/u);
  current.scale.liveRows = "405";
  assert.match(evaluateAdmission(sealed, manifest.admission, current, now).reasons.join(" "), /more than one percent/u);
  const omittedIdentity = structuredClone(document);
  delete omittedIdentity.identities.aclDigest;
  assert.throws(() => calculateAdmission(omittedIdentity, manifest.admission), /identity set is incomplete/u);
  const zeroPeak = structuredClone(document);
  zeroPeak.filesystems[0].rehearsal.indexBytes = "0";
  assert.throws(() => calculateAdmission(zeroPeak, manifest.admission), /must be measured and nonzero/u);
});

// require durable progress ownership before cleanup
test("U-IDX-03 cleanup authority rejects uncaptured or replaced indexes", () => {
  const actual = catalog();
  const operation = {
    operationId: "f626bd47-f32e-4a84-b125-8ac6dcfc5f37",
    clusterDigest: "d".repeat(64),
    databaseOid: "16384",
    tableOid: "17000",
  };
  const ownership = {
    schema: "weather-page-index-ownership/v2",
    operationId: operation.operationId,
    captured: true,
    clusterDigest: operation.clusterDigest,
    databaseOid: operation.databaseOid,
    tableOid: operation.tableOid,
    indexOid: actual.indexOid,
    fingerprintHash: catalogFingerprintHash(actual),
  };
  assert.equal(validateOwnedIndex(ownership, actual, operation), true);
  assert.throws(() => validateOwnedIndex({ ...ownership, captured: false }, actual, operation), /never durably captured/u);
  assert.throws(() => validateOwnedIndex(ownership, catalog({ indexOid: "19000" }), operation), /OID changed/u);
  assert.throws(() => validateOwnedIndex(ownership, catalog({ reloptions: ["fillfactor=90"] }), operation), /fingerprint changed/u);
});

// bind restore ownership without child stdout
test("U-RESTORE-02 binds exactly one absent-before PID candidate", () => {
  const expectation = {
    fixtureDatabase: "fixture_weather",
    childPid: 31415,
    owner: "weather_owner",
    startedAtEpochMilliseconds: Date.parse("2026-10-28T16:00:00.000Z"),
    observedAtEpochMilliseconds: Date.parse("2026-10-28T16:00:02.000Z"),
    clockSkewMilliseconds: 100,
  };
  const before = [{ name: "fixture_weather", oid: "1", owner: "weather_owner" }];
  const candidate = {
    name: "fixture_weather_verify_20261028160001_31415",
    oid: "2",
    owner: "weather_owner",
    createdAt: "2026-10-28T16:00:01.000Z",
  };
  assert.deepEqual(bindRestoreCandidate(before, [...before, candidate], expectation), {
    name: candidate.name,
    oid: "2",
    owner: candidate.owner,
    createdAt: candidate.createdAt,
  });
  assert.throws(() => bindRestoreCandidate(before, [...before, candidate, { ...candidate, oid: "3" }], expectation), /ambiguous/u);
  assert.throws(() => bindRestoreCandidate(before, before, expectation), /ambiguous/u);
});

// reject names-only restore isolation
test("U-RESTORE-03 validates the actual postgres data mount", () => {
  const config = {
    composeProject: "pageidx_restore_f626bd47",
    fixtureDatabase: "fixture_weather",
    weatherPostgresDir: "/srv/pageidx/postgres",
  };
  const inspect = {
    Id: "container-id",
    Image: "sha256:image",
    State: { StartedAt: "2026-10-28T16:00:00Z" },
    Config: { Labels: { "com.docker.compose.project": config.composeProject, "com.docker.compose.service": "postgres" } },
    Mounts: [{ Destination: "/var/lib/postgresql/data", Source: "/srv/pageidx/postgres", Type: "bind" }],
  };
  assert.equal(validateDisposableMount(inspect, config, manifest.restoreContext).mountSource, "/srv/pageidx/postgres");
  assert.throws(
    () => validateDisposableMount({ ...inspect, Mounts: [{ ...inspect.Mounts[0], Source: "/var/lib/weather/postgres" }] }, { ...config, weatherPostgresDir: "/var/lib/weather/postgres" }, manifest.restoreContext),
    /production/u,
  );
});

// require a separately bound three-copy restore reserve
test("U-RESTORE-03 restore coexistence capacity uses source database bytes", () => {
  const capturedAt = "2026-10-28T16:00:00.000Z";
  const actual = {
    reportDigest: "a".repeat(64),
    archive: { digest: "b".repeat(64), size: 1024 },
    database: { name: "fixture_weather", oid: "16384", owner: "weather_owner", systemIdentifier: "1234", sizeBytes: String(1024 ** 3) },
    filesystem: { dev: "2049", path: "/data", freeBytes: String(9 * 1024 ** 3) },
  };
  const report = {
    schema: "weather-page-restore-source-size/v1",
    capturedAt,
    archive: actual.archive,
    database: actual.database,
  };
  const admitted = validateRestoreCoexistence(report, actual, new Date("2026-10-28T16:01:00.000Z"));
  assert.equal(admitted.requiredFreeBytes, String(8 * 1024 ** 3));
  assert.throws(
    () => validateRestoreCoexistence(
      { ...report, database: { ...report.database, sizeBytes: String(10 * 1024 ** 3) } },
      { ...actual, database: { ...actual.database, sizeBytes: String(10 * 1024 ** 3) }, filesystem: { ...actual.filesystem, freeBytes: String(32 * 1024 ** 3) } },
      new Date("2026-10-28T16:01:00.000Z"),
    ),
    /coexistence capacity is insufficient/u,
  );
  assert.throws(() => validateRestoreCoexistence({ ...report, archive: { ...report.archive, digest: "c".repeat(64) } }, actual, new Date("2026-10-28T16:01:00.000Z")), /archive identity mismatch/u);
  assert.throws(() => validateRestoreCoexistence(
    { ...report, archive: { digest: "b".repeat(64), size: 2 * 1024 ** 3 }, database: { ...report.database, sizeBytes: String(1024 ** 2) } },
    { ...actual, archive: { digest: "b".repeat(64), size: 2 * 1024 ** 3 }, database: { ...actual.database, sizeBytes: String(1024 ** 2) }, filesystem: { ...actual.filesystem, freeBytes: String(64 * 1024 ** 3) } },
    new Date("2026-10-28T16:01:00.000Z"),
  ), /archive is larger than its measured source database/u);
});

// keep mutating command gates explicit
test("U-IDX-04 cli refuses missing mutating gates", () => {
  assert.deepEqual(parseArguments(["plan", "--config", "/private/config.json"]), {
    mode: "plan",
    config: "/private/config.json",
  });
  assert.throws(() => parseArguments(["plan", "--config", "/private/config.json", "--admission", "/private/admission.json"]), /unknown option/u);
  assert.throws(() => parseArguments(["apply", "--config", "/private/config.json"]), /--admission is required/u);
  assert.throws(() => parseArguments(["revert", "--config", "/private/config.json"]), /--operation is required/u);
  assert.throws(() => parseArguments(["revert", "--config", "/private/config.json", "--operation", "/private/operation"]), /--admission is required for revert/u);
  assert.throws(() => parseArguments(["verify-restore", "--config", "/private/config.json"]), /--admission is required/u);
  assert.throws(() => parseArguments(["verify-restore", "--config", "/private/config.json", "--admission", "/private/admission.json"]), /--restore-context is required/u);
  const sample = parseArguments([
    "sample-ingestion",
    "--config", "/private/config.json",
    "--duration-seconds", "600",
    "--output", "/private/ingestion.json",
    "--rehearsal-report", "/private/rehearsal.json",
    "--rehearsal-digest", "a".repeat(64),
  ]);
  assert.equal(sample.mode, "sample-ingestion");
  assert.equal(sample.durationSeconds, "600");
  const admission = parseArguments([
    "build-admission",
    "--config", "/private/config.json",
    "--output", "/private/admission.json",
    "--rehearsal-report", "/private/rehearsal.json",
    "--rehearsal-digest", "a".repeat(64),
    "--ingestion-report", "/private/ingestion.json",
    "--ingestion-digest", "b".repeat(64),
  ]);
  assert.equal(admission.mode, "build-admission");
  assert.equal(admission.ingestionDigest, "b".repeat(64));
  assert.throws(() => parseArguments([
    "build-admission",
    "--config", "/private/config.json",
    "--output", "/private/admission.json",
    "--rehearsal-report", "/private/rehearsal.json",
    "--rehearsal-digest", "a".repeat(64),
    "--ingestion-report", "/private/ingestion.json",
  ]), /--ingestion-digest is required/u);
  assert.throws(() => parseArguments([
    "sample-ingestion",
    "--config", "/private/config.json",
    "--duration-seconds", "599",
    "--output", "/private/ingestion.json",
    "--rehearsal-report", "/private/rehearsal.json",
    "--rehearsal-digest", "a".repeat(64),
  ]), /600 through 4500/u);
});

// construct one genuine mixed-ingestion replay fixture
function burstReplayFixture() {
  const batchMix = { forecast: 240, physical: 50, tideObservation: 24, tidePrediction: 24 };
  const outcomes = [{ forecast: 240, tide_observation: 24, tide_prediction: 24, physical: 50 }];
  const phase = {
    batchCount: 1,
    batchMix,
    durationsMs: [20],
    generatedWalBytes: 4096,
    outcomeSha256: sha256(JSON.stringify(outcomes)),
    outcomes,
    p95Ms: 20,
    relationBytesDelta: 2048,
  };
  return {
    measured: true,
    pass: true,
    outcomesEqual: true,
    p95Ratio: 1,
    baseline: structuredClone(phase),
    candidate: structuredClone(phase),
  };
}

// refuse overlapping monitor work at the next cadence
test("U-IDX-05 monitor cadence never overlaps resource samples", async () => {
  let scheduled;
  let releaseSample;
  let samples = 0;
  const failures = [];
  const blocked = new Promise((resolvePromise) => {
    releaseSample = resolvePromise;
  });
  const sampler = startExclusiveIntervalSampler({
    intervalMilliseconds: 2000,
    // hold the first sample across one cadence
    async sample() {
      samples += 1;
      await blocked;
    },
    // collect the exact cadence failure
    onFailure(message) {
      failures.push(message);
    },
    // capture the timer callback without waiting
    schedule(callback) {
      scheduled = callback;
      return 1;
    },
    // avoid a real timer cleanup
    unschedule() {},
  });
  await Promise.resolve();
  scheduled();
  assert.equal(samples, 1);
  assert.deepEqual(failures, ["resource monitor missed its 2000-millisecond cadence"]);
  releaseSample();
  await sampler.stop();
});

// bind the exact API-backed Now and Forecast probes
test("U-IDX-05 runtime monitoring rejects static page probes", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-page-index-runtime-"));
  const path = join(root, "runtime.json");
  const runtime = {
    schema: "weather-page-index-runtime/v1",
    restoreOnly: false,
    paths: {
      stateRoot: "/opt/weather/current/deploy/state",
      installRoot: "/opt/weather/current",
      forecastSettings: "/var/lib/weather/xweather/forecast-adjustment-settings.json",
      evidenceRoot: "/var/lib/weather/page-index-evidence",
    },
    postgres: { database: "weather", user: "postgres" },
    containers: ["api", "cloudflared", "postgres", "web", "worker"],
    monitor: {
      apiContainer: "api",
      endpoints: [
        { name: "now", path: "/api/v1/sites/ballydidean/current", timeoutMilliseconds: 2000 },
        { name: "forecast", path: "/api/v1/sites/ballydidean/forecast?days=10", timeoutMilliseconds: 2000 },
      ],
    },
  };

  try {
    await writeFile(path, `${JSON.stringify(runtime)}\n`);
    await chmod(path, 0o600);
    assert.equal((await readRuntimeConfig(path)).monitor.apiContainer, "api");
    runtime.admission = { swapSampleMilliseconds: 1 };
    await writeFile(path, `${JSON.stringify(runtime)}\n`);
    await assert.rejects(readRuntimeConfig(path), /bounded integer/u);
    runtime.admission = { swapSampleMilliseconds: 5000 };
    await writeFile(path, `${JSON.stringify(runtime)}\n`);
    await assert.rejects(readRuntimeConfig(path), /at least sixty seconds/u);
    runtime.restoreOnly = true;
    await writeFile(path, `${JSON.stringify(runtime)}\n`);
    await assert.rejects(readRuntimeConfig(path), /labeled diagnostic/u);
    runtime.admission.diagnosticShortSwapSample = true;
    await writeFile(path, `${JSON.stringify(runtime)}\n`);
    assert.equal((await readRuntimeConfig(path)).admission.diagnosticShortSwapSample, true);
    runtime.restoreOnly = false;
    delete runtime.admission;
    runtime.monitor.endpoints[0].path = "/";
    await writeFile(path, `${JSON.stringify(runtime)}\n`);
    await assert.rejects(readRuntimeConfig(path), /exact Now and Forecast probes/u);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// produce a ten-minute report with an injected clock
test("U-IDX-05 ingestion producer binds replay, heartbeat, WAL, and filesystem growth", async () => {
  const epoch = Date.parse("2026-10-28T18:00:00.000Z");
  let clock = 0;
  const samples = await collectIngestionSamples({
    durationMilliseconds: 600_000,
    // capture one deterministic production-shaped sample
    async capture() {
      const capturedAt = new Date(epoch + clock).toISOString();
      const loopAt = new Date(epoch + Math.floor(clock / 60_000) * 60_000).toISOString();
      return {
        capturedAt,
        database: {
          capturedAt,
          walLsn: `0/${String(clock)}`,
          walBytes: String(1000 + clock),
          heartbeats: [{ workerInstance: "worker-1", lastLoopAt: loopAt }],
        },
        filesystems: [{
          id: "2049",
          allocatedBytes: String(10_000 + clock),
          allocatedInodes: String(100 + Math.floor(clock / 60_000)),
          freeBytes: String(1_000_000 - clock),
          freeInodes: "10000",
          totalBytes: "2000000",
          totalInodes: "20000",
        }],
      };
    },
    // expose one monotonic fake clock
    monotonicNow() {
      return clock;
    },
    // advance without a real ten-minute delay
    async sleep(milliseconds) {
      clock += milliseconds;
    },
  });
  assert.equal(samples.length, 301);
  const rehearsalDigest = "b".repeat(64);
  const rehearsal = { admission: { burstReplay: burstReplayFixture() } };
  const burstEvidence = deriveBurstReplayEvidence(rehearsal, rehearsalDigest);
  const identityNames = [
    "aclDigest", "catalogDigest", "controlDigest", "databaseDigest", "forecastSettingsDigest", "imageDigest",
    "ledgerDigest", "manifestDigest", "packageDigest", "settingsDigest", "sourceDigest",
  ];
  const identities = Object.fromEntries(identityNames.map((name, position) => [name, String(position % 10).repeat(64)]));
  const filesystems = [{ id: "2049", paths: ["/data"], roles: ["postgres-data"], dockerShares: false, totalInodes: "20000" }];
  const report = summarizeIngestionSamples({ samples, identities, filesystems, observedRuns: [], burstEvidence });
  assert.equal(report.events.eventCoverageOrigin, "disposable-replay");
  assert.deepEqual(report.events.liveRunIds, { forecast: [], tide: [] });
  assert.equal(report.sampleCount, 301);
  assert.equal(report.loopP95Seconds, 60);
  assert.equal(validateIngestionReport(report, identities, filesystems, rehearsal, rehearsalDigest), report);
});

// contend on the exact state-directory inode
test("I-IDX-03 shared directory inode flock is exclusive", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-page-index-lock-"));

  try {
    const external = spawn("sh", ["-c", "exec 9<\"$1\"; flock --exclusive --nonblock 9 || exit 73; printf 'LOCKED\\n'; cat >/dev/null", "external-lock", root], {
      stdio: ["pipe", "pipe", "pipe"],
    });
    await new Promise((resolvePromise, rejectPromise) => {
      let output = "";

      // await the external update-style lock
      external.stdout.on("data", (chunk) => {
        output += chunk.toString("utf8");

        // accept the exact marker
        if (output.includes("LOCKED\n")) {
          resolvePromise();
        }
      });

      // reject external lock failures
      external.on("error", rejectPromise);
      external.on("close", (code) => {
        // reject early completion
        if (!output.includes("LOCKED\n")) {
          rejectPromise(new Error(`external lock exited ${code}`));
        }
      });
    });
    await assert.rejects(acquireStateDirectoryLock(root), /lock is busy/u);
    external.stdin.end();
    await new Promise((resolvePromise) => {
      // await external lock release
      external.once("close", resolvePromise);
    });

    const first = await acquireStateDirectoryLock(root);
    await assert.rejects(acquireStateDirectoryLock(root), /lock is busy/u);
    const externalAttempt = await runCommand("sh", ["-c", "exec 9<\"$1\"; flock --exclusive --nonblock 9", "external-lock", root], { allowFailure: true });
    assert.notEqual(externalAttempt.code, 0);
    await first.release();
    const second = await acquireStateDirectoryLock(root);
    assert.equal(second.identity.path, root);
    await second.release();
    await assert.rejects(access(join(root, "3")), /ENOENT/u);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// release the old inode when post-lock identity validation fails
test("I-IDX-03 changed state-directory inode cannot leak its lock holder", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-page-index-replaced-lock-"));
  const moved = `${root}-moved`;

  try {
    await assert.rejects(acquireStateDirectoryLock(root, {
      // replace the named directory after the old inode is locked
      async afterAcknowledged() {
        await rename(root, moved);
        await mkdir(root);
      },
    }), /inode changed/u);
    const external = await runCommand("sh", ["-c", "exec 9<\"$1\"; flock --exclusive --nonblock 9", "external-lock", moved], { allowFailure: true });
    assert.equal(external.code, 0);
  } finally {
    await rm(root, { recursive: true, force: true });
    await rm(moved, { recursive: true, force: true });
  }
});

// build one realistic installed source fixture
async function createInstalledSourceFixture() {
  const root = await mkdtemp(join(tmpdir(), "weather-page-index-source-"));
  const deploy = join(root, "deploy");

  // create every fixed control subtree
  for (const directory of ["scripts", "postgres", "systemd", "sudoers", "state", "releases"]) {
    await mkdir(join(deploy, directory), { recursive: true });
  }

  await writeFile(join(deploy, "scripts/a.sh"), "a\n");
  await writeFile(join(deploy, "scripts/b.sh"), "b\n");
  await writeFile(join(deploy, "postgres/a.sql"), "select 1;\n");
  await writeFile(join(deploy, "systemd/a.service"), "service\n");
  await writeFile(join(deploy, "sudoers/a"), "sudoers\n");
  await writeFile(join(deploy, "compose.yaml"), "services: {}\n");
  await writeFile(join(deploy, "state/current-release"), "2026.10.09-1\n");
  await writeFile(join(deploy, "state/schema-release"), "2026.10.09-1\n");
  await writeFile(join(deploy, "state/current-web-release"), "2026.10.14-1\n");
  const coreEnvironment = [
    "WEATHER_RELEASE=2026.10.09-1",
    `WEATHER_CONTROL_PLANE_SHA256=${"6".repeat(64)}`,
    "WEATHER_CONTROL_PLANE_VERSION=13",
    `WEATHER_SERVER_IMAGE=server@sha256:${"1".repeat(64)}`,
    `WEATHER_WEB_IMAGE=web@sha256:${"2".repeat(64)}`,
    `POSTGRES_IMAGE=postgres@sha256:${"3".repeat(64)}`,
    `CLOUDFLARED_IMAGE=cloudflared@sha256:${"4".repeat(64)}`,
    "WEATHER_DATABASE_NAME=weather",
    "",
  ].join("\n");
  await writeFile(join(deploy, "releases/2026.10.09-1.env"), coreEnvironment);
  await writeFile(join(deploy, "releases/2026.10.14-1.web.env"), `WEATHER_WEB_IMAGE=web@sha256:${"5".repeat(64)}\n`);
  await symlink("../releases/2026.10.09-1.env", join(deploy, "state/active.env"));
  await symlink("../releases/2026.10.14-1.web.env", join(deploy, "state/active-web.env"));
  return root;
}

// bind the real active env and complete fixed control inventory
test("U-IDX-02 source snapshot distinguishes recorded and installed control", async () => {
  const root = await createInstalledSourceFixture();

  try {
    const source = await captureDeploymentSource(root);
    const firstControl = await hashControlPlane(root);
    assert.equal(source.recordedControlDigest, "6".repeat(64));
    assert.equal(source.core, "2026.10.09-1");
    assert.equal(source.schema, "2026.10.09-1");
    assert.equal(source.web, "2026.10.14-1");
    assert.notEqual(firstControl.digest, source.recordedControlDigest);
    await writeFile(join(root, "deploy/scripts/nested-drift.sh"), "drift\n");
    const secondControl = await hashControlPlane(root);
    assert.notEqual(secondControl.digest, firstControl.digest);
    await writeFile(join(root, "deploy/state/dashboard-web-transaction.env"), "pending\n");
    await assert.rejects(captureDeploymentSource(root), /incomplete release transaction/u);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// bind canary values and prevent restore-only live bypasses
test("U-IDX-02 canary and runtime mode gates are closed", () => {
  assert.deepEqual(collectCanarySettings({ wind: { canaryEnabled: true }, temperatureKillSwitch: false, unrelated: 1 }), {
    "wind.canaryEnabled": true,
    temperatureKillSwitch: false,
  });
  const restoreConfig = { restoreOnly: true, paths: {} };
  assert.equal(validateRuntimeMode(restoreConfig, "verify-restore"), true);
  assert.throws(() => validateRuntimeMode(restoreConfig, "apply"), /cannot run a live lifecycle mode/u);
  const liveConfig = {
    restoreOnly: false,
    paths: {
      stateRoot: "/opt/weather/current/deploy/state",
      installRoot: "/opt/weather/current",
      forecastSettings: "/var/lib/weather/xweather/forecast-adjustment-settings.json",
    },
  };
  assert.equal(validateRuntimeMode(liveConfig, "apply"), true);
  assert.throws(() => validateRuntimeMode(liveConfig, "verify-restore"), /cannot run an isolated restore wrapper/u);
});

// escalate refused cancellation until exact terminal proof
test("U-IDX-03 cancellation refusal cannot release an active backend", async () => {
  const events = [];
  const statuses = [
    { active: true, progress: true },
    { active: true, progress: true },
    { active: false, progress: false },
  ];
  const result = await ensureOwnedBackendTerminal({}, {}, "weather", {
    // return deterministic liveness states
    async inspect() {
      events.push("inspect");
      return statuses.shift();
    },
    // refuse the initial cancellation
    async cancel() {
      events.push("cancel-false");
      return false;
    },
    // accept the exact terminate fallback
    async terminate() {
      events.push("terminate");
      return true;
    },
    // avoid real retry delays
    async delay() {
      events.push("delay");
    },
  });
  assert.deepEqual(result, { active: false, progress: false });
  assert.deepEqual(events, ["inspect", "cancel-false", "delay", "inspect", "terminate", "delay", "inspect"]);
});
