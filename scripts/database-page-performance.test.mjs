import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import {
  chmod,
  lstat,
  mkdtemp,
  readFile,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  CONFIG_CONTRACT_VERSION,
  ENDPOINT_INVENTORY,
  EVIDENCE_CONTRACT_VERSION,
  OBSERVATION_CACHE_TTL_MS,
  SUPERVISOR_CONFIG_CONTRACT_VERSION,
  SUPERVISOR_PRODUCER,
  assertPrivateOwnedPath,
  collectHttpSample,
  evaluateEvidence,
  nearestRankPercentile,
  parseDockerContainerObservation,
  parseSupervisorDatabaseObservation,
  pgdataColdProofBindingSha256,
  requiredTiersForEndpoint,
  runCli,
  superviseColdSamples,
  validateConfiguration,
  validateLoopbackOrigin,
  validateSupervisorConfiguration,
} from "./database-page-performance.mjs";

// build one frozen runtime identity
function runtimeIdentity(environment, suffix) {
  return {
    apiImage: `api@sha256:${suffix}`,
    databaseImage: `postgres@sha256:${suffix}`,
    environment,
    querySetIdentity: `queries-${suffix}`,
    release: `2026.10.28-${suffix}`,
    schemaVersion: "0018",
    sourceCommit: suffix.repeat(40).slice(0, 40),
    webImage: `web@sha256:${suffix}`,
  };
}

// build the exact tier assignments
function runtimeAssignments() {
  return {
    K0: "blueberry-runtime",
    K1: "blueberry-runtime",
    K2: "blueberry-runtime",
    K3: "disposable-runtime",
    warm: "blueberry-runtime",
  };
}

// build the required runtime map
function runtimeIdentities() {
  return {
    "blueberry-runtime": runtimeIdentity("blueberry", "b"),
    "disposable-runtime": runtimeIdentity("disposable", "d"),
  };
}

// label every cache and process layer honestly
function topologyFor(endpoint, tier) {
  const applicationCache = endpoint.applicationCache ? "empty" : "not-applicable";

  // label measured warm traffic
  if (tier === "warm") {
    return {
      apiProcess: "existing",
      applicationCache: endpoint.applicationCache ? "warm" : "not-applicable",
      boundary: "loopback-web-complete-body",
      collector: "web-container",
      connection: "reused",
      databasePool: "existing",
      osCache: "uncontrolled",
      postgresCache: "uncontrolled",
      webProcess: "existing",
    };
  }

  // isolate natural application cache expiry
  if (tier === "K1") {
    return {
      apiProcess: "existing",
      applicationCache: "naturally-expired",
      boundary: "loopback-web-complete-body",
      collector: "web-container",
      connection: "fresh",
      databasePool: "existing",
      osCache: "uncontrolled",
      postgresCache: "uncontrolled",
      webProcess: "existing",
    };
  }

  // label cacheless first connections without cold process claims
  if (tier === "K0") {
    return {
      apiProcess: "existing",
      applicationCache,
      boundary: "loopback-web-complete-body",
      collector: "web-container",
      connection: "fresh",
      databasePool: "existing",
      osCache: "uncontrolled",
      postgresCache: "uncontrolled",
      webProcess: "existing",
    };
  }

  // label a supported fresh service and pool
  if (tier === "K2") {
    return {
      apiProcess: "fresh",
      applicationCache,
      boundary: "loopback-web-complete-body",
      collector: "web-container",
      connection: "fresh",
      databasePool: "fresh",
      osCache: "uncontrolled",
      postgresCache: "uncontrolled",
      webProcess: "fresh",
    };
  }

  return {
    apiProcess: "fresh",
    applicationCache,
    boundary: "loopback-web-complete-body",
    collector: "web-container",
    connection: "fresh",
    databasePool: "fresh",
    osCache: "fresh-disposable",
    postgresCache: "fresh-disposable",
    webProcess: "fresh",
  };
}

// create one complete HTTP timing sample
function makeSample(endpoint, tier, run, index, elapsedMs = 100) {
  const cacheStateId = `${endpoint.id}-${tier}-${String(run)}-${String(index)}`;
  const startedAt = tier === "K1"
    ? new Date(Date.parse("2026-10-28T17:00:00.000Z") + index * 6 * 60_000).toISOString()
    : "2026-10-28T17:00:00.000Z";
  const completedAt = new Date(Date.parse(startedAt) + 1_000).toISOString();
  const responseGeneratedAt = endpoint.applicationCache && endpoint.id !== "viewer-context" && new Set(["K1", "K2", "K3"]).has(tier)
    ? new Date(Date.parse(startedAt) + 100).toISOString()
    : null;

  return {
    bodyComplete: true,
    cacheStateId,
    completedAt,
    elapsedMs,
    eligibility: endpoint.eligibility,
    endpointId: endpoint.id,
    error: null,
    requestPath: endpoint.path,
    responseBytes: 128,
    responseGeneratedAt,
    run,
    runtimeId: tier === "K3" ? "disposable-runtime" : "blueberry-runtime",
    sampleId: `${endpoint.id}-${tier}-${String(run)}-${String(index)}`,
    startedAt,
    status: 200,
    tier,
    timingBoundary: "dispatch-to-complete-body",
    topology: topologyFor(endpoint, tier),
    transport: "http",
    witnessId: new Set(["K1", "K2", "K3"]).has(tier) ? cacheStateId : null,
  };
}

// create one synthetic supervisor witness for validator tests
function makeStateWitness(sample) {
  const applicationCache = sample.tier === "K1"
    ? sample.endpointId === "viewer-context"
      ? {
          cfPathObserved: true,
          discoveryRefreshedAt: sample.startedAt,
          expiredAt: sample.startedAt,
          state: "naturally-expired",
        }
      : {
          cfPathObserved: false,
          cohortId: `cohort-${sample.sampleId}`,
          cohortWitnessIds: [sample.witnessId],
          completedAt: sample.completedAt,
          discoveryRefreshedAt: null,
          dispatchAt: sample.startedAt,
          expiredAt: sample.startedAt,
          priorGeneratedAt: new Date(Date.parse(sample.startedAt) - OBSERVATION_CACHE_TTL_MS).toISOString(),
          responseGeneratedAt: sample.responseGeneratedAt,
          state: "naturally-expired",
        }
    : null;
  const identity = runtimeIdentities()[sample.runtimeId];

  // build one exact compose service observation
  function process(service, pid) {
    const imageField = service === "web" ? "webImage" : service === "api" ? "apiImage" : "databaseImage";
    return {
      configImage: identity[imageField],
      containerId: `container-${service}-${sample.sampleId}`,
      dataVolume: service === "database" ? `volume-${sample.sampleId}` : null,
      imageId: `sha256:${createHash("sha256").update(service).digest("hex")}`,
      pid,
      project: "weather",
      service: service === "database" ? "postgres" : service,
      sourceCommit: service === "web" ? identity.sourceCommit : null,
      startedAt: sample.startedAt,
    };
  }
  const processes = {
    api: process("api", 100),
    database: process("database", 101),
    web: process("web", 102),
  };
  const backends = [{ pid: 200, startedAt: sample.startedAt }];
  const databasePool = {
    backends,
    generationId: createHash("sha256").update(JSON.stringify(backends)).digest("hex"),
  };
  const observation = {
    after: processes,
    before: processes,
    databasePool,
    osBootId: `12345678-1234-1234-1234-${sample.sampleId.padEnd(12, "0").slice(0, 12)}`,
    postgresStartedAt: sample.startedAt,
    schemaLedgerSha256: "a".repeat(64),
    schemaVersion: "0018",
    serverVersionNum: "170010",
  };
  const observationSha256 = createHash("sha256").update(JSON.stringify(observation)).digest("hex");
  const pgdataColdProof = sample.tier === "K3"
    ? {
        bindingSha256: null,
        completedAt: new Date(Date.parse(sample.startedAt) - 1_000).toISOString(),
        contractVersion: "weather-page-pgdata-cold-proof/v1",
        databaseContainerId: processes.database.containerId,
        databaseStartedAt: processes.database.startedAt,
        fileCount: 100,
        proofSha256: "c".repeat(64),
        residentPagesAfter: 0,
        totalPages: 1_000,
        volumeName: processes.database.dataVolume,
      }
    : null;

  // bind synthetic K3 proof to the exact validator fixture attempt
  if (pgdataColdProof !== null) {
    pgdataColdProof.bindingSha256 = pgdataColdProofBindingSha256(
      pgdataColdProof,
      sample,
      observationSha256,
    );
  }

  return {
    applicationCache,
    endpointId: sample.endpointId,
    id: sample.witnessId,
    observation,
    observationSha256,
    observedAt: sample.startedAt,
    pgdataColdProof,
    producer: SUPERVISOR_PRODUCER,
    runtimeId: sample.runtimeId,
    sampleId: sample.sampleId,
    scope: sample.tier === "K3" ? "disposable" : "blueberry",
    tier: sample.tier,
  };
}

// create a complete passing matrix
function passingEvidence() {
  const samples = [];
  const stateWitnesses = {};

  // retain each sample and its independent cold witness
  function addSample(sample) {
    samples.push(sample);

    // bind only K1 through K3 cold claims
    if (sample.witnessId !== null) {
      stateWitnesses[sample.witnessId] = makeStateWitness(sample);
    }
  }

  // cover every immutable endpoint
  for (const endpoint of ENDPOINT_INVENTORY) {
    // preserve three separate warm runs
    for (let run = 1; run <= 3; run += 1) {
      // include exactly forty measured attempts
      for (let index = 0; index < 40; index += 1) {
        addSample(makeSample(endpoint, "warm", run, index));
      }
    }

    const coldTiers = requiredTiersForEndpoint(endpoint.id).filter(
      // omit warm from the cold tier loop
      function omitWarm(tier) {
        return tier !== "warm";
      },
    );

    // cover every endpoint-specific cold tier
    for (const tier of coldTiers) {
      // include at least three cold attempts
      for (let index = 0; index < 3; index += 1) {
        addSample(makeSample(endpoint, tier, null, index));
      }
    }
  }

  // bind each synthetic shared-cache pair to one overlapping refill
  for (let index = 0; index < 3; index += 1) {
    const observation = findSample({ samples }, "forecast-observations", "K1", null, index);
    const solar = findSample({ samples }, "solar-cloud-evidence", "K1", null, index);
    const cohortId = `shared-cache-cohort-${String(index)}`;
    const cohortWitnessIds = [observation.witnessId, solar.witnessId];

    // apply the exact shared transition to both route witnesses
    for (const sample of [observation, solar]) {
      stateWitnesses[sample.witnessId].applicationCache.cohortId = cohortId;
      stateWitnesses[sample.witnessId].applicationCache.cohortWitnessIds = cohortWitnessIds;
    }
  }

  return {
    contractVersion: EVIDENCE_CONTRACT_VERSION,
    measurement: {
      candidateId: "baseline-2026-10-28",
      blueberryProcessColdStatus: "observed-supported-deployment",
      clock: "node-performance-now",
      collectedAt: "2026-10-28T17:00:00.000Z",
      ingestionWitnessId: "worker-heartbeat-2026-10-28T17:00:00Z",
      loadCondition: "normal-ingestion",
      maximumHttpConcurrency: 8,
      phase: "baseline",
      queryIdentity: "schema0018-api-queries",
    },
    runtimeAssignments: runtimeAssignments(),
    runtimeIdentities: runtimeIdentities(),
    samples,
    stateWitnesses,
  };
}

// find one synthetic sample
function findSample(evidence, endpointId, tier, run = null, index = 0) {
  let matched = 0;

  // preserve deterministic selection
  for (const sample of evidence.samples) {
    // match the exact stratum
    if (sample.endpointId === endpointId && sample.tier === tier && sample.run === run) {
      // select the requested occurrence
      if (matched === index) {
        return sample;
      }
      matched += 1;
    }
  }

  throw new Error(`sample not found for ${endpointId} ${tier}`);
}

// build one valid evaluator configuration
function validConfiguration(overrides = {}) {
  return {
    collections: [],
    concurrency: 4,
    contractVersion: CONFIG_CONTRACT_VERSION,
    evidenceFiles: [],
    measurement: passingEvidence().measurement,
    requestTimeoutMs: 5_000,
    runtimeAssignments: runtimeAssignments(),
    runtimeIdentities: runtimeIdentities(),
    stateWitnessFiles: [],
    ...overrides,
  };
}

// build one valid observing supervisor configuration
function validSupervisorConfiguration(overrides = {}) {
  return {
    containers: {
      api: "weather-api-1",
      database: "weather-postgres-1",
      web: "weather-web-1",
    },
    contractVersion: SUPERVISOR_CONFIG_CONTRACT_VERSION,
    cycles: 1,
    database: {
      applicationName: "weather-api",
      name: "weather",
      user: "postgres",
    },
    endpointIds: ["forecast-observations", "solar-cloud-evidence"],
    expiryBufferMs: 2_000,
    measurement: passingEvidence().measurement,
    requestTimeoutMs: 5_000,
    runtimeAssignments: runtimeAssignments(),
    runtimeId: "blueberry-runtime",
    runtimeIdentities: runtimeIdentities(),
    tier: "K1",
    ...overrides,
  };
}

// freeze one D-backed storage identity
function dLoopStorageIdentity() {
  return {
    backingFile: "/mnt/d/Weather-DB-benchmark-20261030T0424Z/benchmark.ext4",
    backingFileBytes: 137_438_953_472,
    loopDevice: "/dev/loop0",
    loopUuid: "439491a2-6408-4c49-b9e4-36044f1ef084",
    mount: "/mnt/weather-db-benchmark-20261030t0424z",
    mountSource: "/dev/loop0",
    volumeNames: [
      "weather-b5d-20261030t0424z-proofSeed",
      "database-volume",
      "weather-b10d-query-candidate",
      "weather-b10d-write-baseline",
      "weather-b10d-write-candidate",
    ],
  };
}

// build one complete raw D-loop eviction proof
function dLoopColdProof(overrides = {}) {
  const storageIdentity = dLoopStorageIdentity();
  const target = {
    fileCount: 1_485,
    inventoryAfterSha256: "b".repeat(64),
    inventoryBeforeSha256: "b".repeat(64),
    pgdataRoot: `${storageIdentity.mount}/queryBaseline`,
    postmasterPidAbsent: true,
    totalBytes: 15_267_659_335,
    volumeName: "database-volume",
  };
  const inner = {
    completedAt: "2026-10-28T16:59:57.000Z",
    completedFiles: target.fileCount,
    fadviseCompleted: true,
    fallbackUsed: false,
    mincoreCompleted: true,
    startedAt: "2026-10-28T16:59:50.000Z",
    totalPages: 3_726_479,
    totalResidentAfter: 0,
  };
  const outer = {
    backingFileBytes: storageIdentity.backingFileBytes,
    completedAt: "2026-10-28T16:59:59.000Z",
    completedFiles: 1,
    fadviseCompleted: true,
    fallbackUsed: false,
    mincoreCompleted: true,
    startedAt: inner.completedAt,
    totalPages: 33_554_432,
    totalResidentAfter: 0,
  };

  return {
    completedAt: outer.completedAt,
    contractVersion: "weather-page-d-loop-cold-proof/v1",
    inner,
    outer,
    startedAt: inner.startedAt,
    storageIdentity,
    target,
    volumes: storageIdentity.volumeNames.map(
      // prove every benchmark volume was detached during eviction
      (name) => ({ name, runningAttachments: [] }),
    ),
    ...overrides,
  };
}

// write one private raw proof fixture
async function writeDLoopColdProof(directory, name, proof) {
  const path = join(directory, name);
  await writeFile(path, `${JSON.stringify(proof)}\n`, { mode: 0o600 });
  return path;
}

// configure one disposable K3 observation
function k3SupervisorConfiguration(coldProofFile, endpointIds = ["current"]) {
  return validSupervisorConfiguration({
    coldProofFile,
    endpointIds,
    expiryBufferMs: 0,
    runtimeId: "disposable-runtime",
    storageIdentity: dLoopStorageIdentity(),
    tier: "K3",
  });
}

// build one complete web-container probe result
function rawProbe(endpointId, startedAt, completedAt, responseGeneratedAt) {
  return {
    bodyComplete: true,
    completedAt,
    elapsedMs: Date.parse(completedAt) - Date.parse(startedAt),
    error: null,
    path: ENDPOINT_INVENTORY.find(
      // locate the exact frozen endpoint
      function findEndpoint(endpoint) {
        return endpoint.id === endpointId;
      },
    ).path,
    responseBytes: 128,
    responseGeneratedAt,
    startedAt,
    status: 200,
  };
}

// build one stable three-container observation
function observedProcesses(
  startedAt = "2026-10-28T16:59:00.000Z",
  runtimeId = "blueberry-runtime",
) {
  const identity = runtimeIdentities()[runtimeId];

  // build one observed compose service
  function observed(service, pid, configImage) {
    return {
      configImage,
      containerId: `${service}-container`,
      dataVolume: service === "database" ? "database-volume" : null,
      imageId: `sha256:${createHash("sha256").update(service).digest("hex")}`,
      pid,
      project: "weather",
      service: service === "database" ? "postgres" : service,
      sourceCommit: service === "web" ? identity.sourceCommit : null,
      startedAt,
    };
  }
  return {
    api: observed("api", 101, identity.apiImage),
    database: observed("database", 102, identity.databaseImage),
    web: observed("web", 103, identity.webImage),
  };
}

// build one observed application pool
function observedDatabase(startedAt = "2026-10-28T16:59:00.000Z") {
  const backends = [{ pid: 201, startedAt }];
  return {
    databasePool: {
      backends,
      generationId: createHash("sha256").update(JSON.stringify(backends)).digest("hex"),
    },
    osBootId: "12345678-1234-1234-1234-123456789abc",
    postgresStartedAt: startedAt,
    schemaLedgerSha256: "a".repeat(64),
    schemaVersion: "0018",
    serverVersionNum: "170010",
  };
}

// calculate nearest-rank boundaries without interpolation
test("nearest-rank percentile uses the exact ranked sample", () => {
  assert.equal(nearestRankPercentile([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20], 0.95), 19);
  assert.equal(nearestRankPercentile([10], 0.95), 10);
  // reject an empty sample set
  assert.throws(() => nearestRankPercentile([], 0.95), /requires values/u);
  // reject invalid duration values
  assert.throws(() => nearestRankPercentile([1, Number.NaN], 0.95), /finite nonnegative/u);
});

// freeze the complete Now and Forecast inventory
test("endpoint inventory includes every exact required URL and conditional case", () => {
  const identities = ENDPOINT_INVENTORY.map(
    // select stable inventory fields
    function selectIdentity(endpoint) {
      return [endpoint.id, endpoint.path, endpoint.eligibility];
    },
  );

  assert.deepEqual(
    identities,
    [
      ["current", "/api/v1/sites/ballydidean/current", "unconditional"],
      ["daily-precipitation", "/api/v1/sites/ballydidean/daily-precipitation", "unconditional"],
      ["forecast-default", "/api/v1/sites/ballydidean/forecast", "unconditional"],
      ["forecast-1d", "/api/v1/sites/ballydidean/forecast?days=1", "unconditional"],
      ["forecast-5d", "/api/v1/sites/ballydidean/forecast?days=5", "unconditional"],
      ["forecast-10d", "/api/v1/sites/ballydidean/forecast?days=10", "unconditional"],
      ["tides", "/api/v1/sites/ballydidean/tides", "unconditional"],
      ["forecast-observations", "/api/v1/sites/ballydidean/forecast-observations", "unconditional"],
      ["solar-cloud-evidence", "/api/v1/sites/ballydidean/solar-cloud-evidence", "unconditional"],
      ["viewer-context", "/api/v1/viewer-context", "unconditional"],
      ["property-sensor-layout", "/api/v1/sites/ballydidean/property-sensor-layout", "eligible-live"],
    ],
  );
  assert.deepEqual(requiredTiersForEndpoint("forecast-observations"), ["warm", "K0", "K1", "K2", "K3"]);
  assert.deepEqual(requiredTiersForEndpoint("current"), ["warm", "K0", "K2", "K3"]);
});

// accept only a complete per-stratum matrix
test("complete warm and cold evidence passes every endpoint independently", () => {
  const result = evaluateEvidence(passingEvidence());

  assert.equal(result.pass, true, result.issues.join("\n"));
  assert.equal(result.strata.every(
    // require every independent stratum to pass
    function allPass(stratum) {
      return stratum.pass;
    },
  ), true);
  assert.equal(result.strata.length, 69);
});

// preserve a fresh process-local pool without retained PostgreSQL connections
test("K2 accepts an observed empty pool only with fresh stable processes", () => {
  const evidence = passingEvidence();
  const sample = findSample(evidence, "current", "K2");
  const witness = evidence.stateWitnesses[sample.witnessId];
  witness.observation.databasePool = {
    backends: [],
    generationId: createHash("sha256").update("[]").digest("hex"),
  };
  witness.observationSha256 = createHash("sha256")
    .update(JSON.stringify(witness.observation)).digest("hex");
  const result = evaluateEvidence(evidence);
  assert.equal(result.pass, true, result.issues.join("\n"));

  const stale = "2026-10-28T16:00:00.000Z";
  witness.observation.before.api.startedAt = stale;
  witness.observation.after.api.startedAt = stale;
  witness.observationSha256 = createHash("sha256")
    .update(JSON.stringify(witness.observation)).digest("hex");
  const rejected = evaluateEvidence(evidence);
  assert.equal(rejected.pass, false);
  assert.match(JSON.stringify(rejected.strata), /K2 witness api process is not fresh/u);
});

// retain array, digest and observed-connection validation
test("K2 rejects missing malformed stale or misbound backend observations", () => {
  const invalidPools = [
    { generationId: createHash("sha256").update("[]").digest("hex") },
    { backends: null, generationId: "a".repeat(64) },
    { backends: [], generationId: "a".repeat(64) },
    { backends: [{ pid: 0, startedAt: "2026-10-28T17:00:00.000Z" }], generationId: "a".repeat(64) },
    { backends: [{ pid: 201, startedAt: "2026-10-28T16:00:00.000Z" }], generationId: "a".repeat(64) },
  ];

  // reject each independent malformed observation without changing the denominator
  for (const invalid of invalidPools) {
    // isolate malformed and stale backends from digest failures
    if (Array.isArray(invalid.backends) && invalid.backends.length > 0) {
      invalid.generationId = createHash("sha256")
        .update(JSON.stringify(invalid.backends)).digest("hex");
    }
    const evidence = passingEvidence();
    const sample = findSample(evidence, "current", "K2");
    const witness = evidence.stateWitnesses[sample.witnessId];
    witness.observation.databasePool = invalid;
    witness.observationSha256 = createHash("sha256")
      .update(JSON.stringify(witness.observation)).digest("hex");
    assert.equal(evaluateEvidence(evidence).pass, false);
  }
});

// bind an idle pool through the actual bundled supervision path
test("bundled K2 supervisor retains a fresh empty connection pool", async () => {
  const startedAt = "2026-10-28T17:00:00.000Z";
  const database = observedDatabase(startedAt);
  database.databasePool = {
    backends: [],
    generationId: createHash("sha256").update("[]").digest("hex"),
  };
  const result = await superviseColdSamples(validSupervisorConfiguration({
    endpointIds: ["current"],
    expiryBufferMs: 0,
    tier: "K2",
  }), {
    // return one complete measured response
    async collectGroup() {
      return [rawProbe("current", startedAt, "2026-10-28T17:00:01.000Z", null)];
    },
    // retain the exact observed empty backend set
    async inspectDatabase() {
      return database;
    },
    // retain one exact fresh API generation around dispatch
    async inspectProcesses() {
      return observedProcesses(startedAt);
    },
  });
  assert.equal(result.supervisionResult.pass, true,
    result.supervisionResult.issues.join("\n"));
  assert.deepEqual(result.witnessFile.witnesses[0].observation.databasePool, database.databasePool);
  assert.equal(result.evidence.samples[0].status, 200);
  assert.equal(result.evidence.samples[0].bodyComplete, true);
});

// represent an observed idle pool without inventing a PostgreSQL backend
test("database supervisor parses and hashes the exact empty backend set", () => {
  const observation = {
    backends: [],
    postgresStartedAt: "2026-10-28T17:00:00.000Z",
    schemaLedger: [{ name: "0018_adjustment_maintenance_v2.sql", checksum: "a".repeat(64) }],
    serverVersionNum: "170010",
  };
  const parsed = parseSupervisorDatabaseObservation(JSON.stringify(observation));
  assert.deepEqual(parsed.databasePool.backends, []);
  assert.equal(parsed.databasePool.generationId, createHash("sha256").update("[]").digest("hex"));

  // reject absent and malformed arrays before producing any witness
  for (const backends of [undefined, null, {}, [{ pid: 0, startedAt: observation.postgresStartedAt }]]) {
    assert.throws(() => parseSupervisorDatabaseObservation(JSON.stringify({ ...observation, backends })));
  }
});

// reject one omitted response instead of pooling neighboring endpoints
test("missing warm and cold samples fail their exact strata", () => {
  const evidence = passingEvidence();
  const missingWarm = findSample(evidence, "forecast-10d", "warm", 3, 39);
  const missingCold = findSample(evidence, "tides", "K2", null, 2);
  evidence.samples = evidence.samples.filter(
    // remove only the two selected attempts
    function retainOtherSamples(sample) {
      return sample !== missingWarm && sample !== missingCold;
    },
  );
  const result = evaluateEvidence(evidence);

  assert.equal(result.pass, false);
  assert.match(result.issues.join("\n"), /forecast-10d warm run 3 failed/u);
  assert.match(result.issues.join("\n"), /tides K2 failed/u);
});

// reject a slow run even when a pooled percentile would pass
test("warm p95 is enforced per endpoint and per run", () => {
  const evidence = passingEvidence();

  // make exactly the nearest-rank tail slow
  for (let index = 37; index < 40; index += 1) {
    findSample(evidence, "forecast-5d", "warm", 2, index).elapsedMs = 1_001;
  }
  const result = evaluateEvidence(evidence);
  const failed = result.strata.find(
    // find the exact failed warm run
    function findFailed(stratum) {
      return stratum.endpointId === "forecast-5d" && stratum.tier === "warm" && stratum.run === 2;
    },
  );

  assert.equal(result.pass, false);
  assert.equal(failed?.p95Ms, 1_001);
  assert.match(failed?.issues.join("\n") ?? "", /exceeds 1000ms/u);
});

// keep status errors incomplete bodies and timeouts in the denominator
test("failed and incomplete HTTP attempts fail without being discarded", () => {
  const evidence = passingEvidence();
  const statusFailure = findSample(evidence, "current", "warm", 1, 0);
  const incomplete = findSample(evidence, "current", "warm", 1, 1);
  const timeout = findSample(evidence, "current", "warm", 1, 2);
  statusFailure.status = 502;
  incomplete.bodyComplete = false;
  timeout.error = "TimeoutError";
  const result = evaluateEvidence(evidence);
  const failed = result.strata.find(
    // find the exact failed attempt group
    function findFailed(stratum) {
      return stratum.endpointId === "current" && stratum.tier === "warm" && stratum.run === 1;
    },
  );

  assert.equal(failed?.observedSamples, 40);
  assert.match(failed?.issues.join("\n") ?? "", /HTTP status 502/u);
  assert.match(failed?.issues.join("\n") ?? "", /response body was not completed/u);
  assert.match(failed?.issues.join("\n") ?? "", /transport error/u);
});

// reject cold samples that exceed the absolute budget
test("every cold response must remain within two seconds", () => {
  const evidence = passingEvidence();
  findSample(evidence, "forecast-default", "K0", null, 1).elapsedMs = 2_001;
  const result = evaluateEvidence(evidence);
  const failed = result.strata.find(
    // find the exact cold stratum
    function findFailed(stratum) {
      return stratum.endpointId === "forecast-default" && stratum.tier === "K0";
    },
  );

  assert.equal(result.pass, false);
  assert.equal(failed?.maximumMs, 2_001);
  assert.match(failed?.issues.join("\n") ?? "", /exceeds 2000ms/u);
});

// reject dishonest cache and process labels
test("cold topology labels cannot overstate the established cache tier", () => {
  const evidence = passingEvidence();
  findSample(evidence, "current", "K0", null, 0).topology.databasePool = "fresh";
  findSample(evidence, "viewer-context", "K1", null, 0).topology.applicationCache = "manually-cleared";
  findSample(evidence, "current", "K3", null, 0).topology.osCache = "uncontrolled";
  findSample(evidence, "tides", "K2", null, 1).cacheStateId = findSample(evidence, "tides", "K2", null, 0).cacheStateId;
  const result = evaluateEvidence(evidence);

  assert.equal(result.pass, false);
  assert.match(JSON.stringify(result.strata), /K0 must not claim fresh processes/u);
  assert.match(JSON.stringify(result.strata), /naturally-expired application cache/u);
  assert.match(JSON.stringify(result.strata), /fresh disposable process/u);
  assert.match(JSON.stringify(result.strata), /cold cacheStateId .* is duplicated/u);
});

// allow K0 to report an uncontrolled server cache honestly
test("K0 fresh connection does not imply an empty application cache", () => {
  const evidence = passingEvidence();
  const observation = findSample(evidence, "forecast-observations", "K0", null, 0);
  observation.topology.applicationCache = "uncontrolled";
  const result = evaluateEvidence(evidence);
  const stratum = result.strata.find(
    // find the exact K0 observation stratum
    function findK0(item) {
      return item.endpointId === "forecast-observations" && item.tier === "K0";
    },
  );

  assert.equal(stratum?.pass, true, stratum?.issues.join("\n"));
});

// refuse fixture eligibility as live layout proof
test("conditional layout evidence requires eligible live identity", () => {
  const evidence = passingEvidence();
  findSample(evidence, "property-sensor-layout", "K2", null, 0).eligibility = "deterministic-fixture";
  const result = evaluateEvidence(evidence);

  assert.equal(result.pass, false);
  assert.match(JSON.stringify(result.strata), /eligibility must be eligible-live/u);
});

// refuse cold labels without independent supervisor observations
test("K1 through K3 require bound supervisor state witnesses", () => {
  const evidence = passingEvidence();
  const solar = findSample(evidence, "solar-cloud-evidence", "K1", null, 0);
  const viewer = findSample(evidence, "viewer-context", "K1", null, 0);
  const currentK3 = findSample(evidence, "current", "K3", null, 0);
  delete evidence.stateWitnesses[solar.witnessId];
  evidence.stateWitnesses[viewer.witnessId].applicationCache.cfPathObserved = false;
  evidence.stateWitnesses[currentK3.witnessId].pgdataColdProof.bindingSha256 = "0".repeat(64);
  const result = evaluateEvidence(evidence);

  assert.equal(result.pass, false);
  assert.match(JSON.stringify(result.strata), /supervisor state witness is missing/u);
  assert.match(JSON.stringify(result.strata), /observed CF discovery refresh/u);
  assert.match(JSON.stringify(result.strata), /zero-residency PGDATA proof is missing or invalid/u);
});

// accept disposable K2 measurements while retaining the live gap
test("disposable K2 remains pending until supported Blueberry evidence exists", () => {
  const evidence = passingEvidence();
  evidence.runtimeIdentities["disposable-k2"] = runtimeIdentity("disposable", "k");
  evidence.runtimeAssignments.K2 = "disposable-k2";
  evidence.measurement.blueberryProcessColdStatus = "pending";

  // rebind every K2 sample and witness to the disposable runtime
  for (const sample of evidence.samples) {
    // update only K2 evidence
    if (sample.tier === "K2") {
      sample.runtimeId = "disposable-k2";
      const witness = evidence.stateWitnesses[sample.witnessId];
      const identity = evidence.runtimeIdentities["disposable-k2"];
      witness.runtimeId = "disposable-k2";
      witness.scope = "disposable";

      // rebind the synthetic observation to the selected disposable images
      for (const phase of ["before", "after"]) {
        witness.observation[phase].web.configImage = identity.webImage;
        witness.observation[phase].web.sourceCommit = identity.sourceCommit;
        witness.observation[phase].api.configImage = identity.apiImage;
        witness.observation[phase].database.configImage = identity.databaseImage;
      }
      witness.observationSha256 = createHash("sha256").update(JSON.stringify(witness.observation)).digest("hex");
    }
  }

  const result = evaluateEvidence(evidence);
  assert.equal(result.pass, false);
  assert.match(result.issues.join("\n"), /Blueberry process-cold evidence remains pending/u);
  assert.equal(result.strata.filter(
    // select only disposable K2 strata
    function selectK2(stratum) {
      return stratum.tier === "K2";
    },
  ).every(
    // require the disposable K2 matrix itself to remain valid
    function allK2Pass(stratum) {
      return stratum.pass;
    },
  ), true);
});

// validate the bundled observer boundary before docker access
test("supervisor configuration permits only bounded exact cold observations", () => {
  const configuration = validSupervisorConfiguration();
  assert.equal(validateSupervisorConfiguration(configuration).cycles, 1);
  const partial = validSupervisorConfiguration({
    runtimeAssignments: { K1: "blueberry-runtime" },
    runtimeIdentities: { "blueberry-runtime": runtimeIdentities()["blueberry-runtime"] },
  });
  assert.equal(validateSupervisorConfiguration(partial).runtimeId, "blueberry-runtime");
  const diagnosticMeasurement = {
    ...passingEvidence().measurement,
    evidenceClass: "quiescent-cold-diagnostic",
    loadCondition: "quiescent-cold-diagnostic",
  };
  assert.equal(validateSupervisorConfiguration(validSupervisorConfiguration({
    endpointIds: ["current"],
    expiryBufferMs: 0,
    measurement: diagnosticMeasurement,
    tier: "K2",
  })).measurement.loadCondition, "quiescent-cold-diagnostic");
  // keep quiescent evidence outside the accepted K1 matrix
  assert.throws(
    () => validateSupervisorConfiguration(validSupervisorConfiguration({ measurement: diagnosticMeasurement })),
    /require K2 or K3/u,
  );
  // reject one route because K1 shares one cache across both consumers
  assert.throws(
    () => validateSupervisorConfiguration(validSupervisorConfiguration({ endpointIds: ["forecast-observations"] })),
    /both shared-cache endpoints/u,
  );
  // reject repeated process-cold samples without a new fresh runtime
  assert.throws(
    () => validateSupervisorConfiguration(validSupervisorConfiguration({ cycles: 2, expiryBufferMs: 0, tier: "K2" })),
    /one fresh-runtime cycle/u,
  );
  // reject secret-shaped fields before any executable runs
  assert.throws(
    () => validateSupervisorConfiguration(validSupervisorConfiguration({ password: "forbidden" })),
    /unsupported/u,
  );
});

// bind one raw dual-layer proof to a complete concurrent K3 group
test("bundled K3 supervisor binds one exact D-loop proof to every sample", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-k3-proof-"));

  try {
    const proofPath = await writeDLoopColdProof(directory, "proof.json", dLoopColdProof());
    const endpointIds = ENDPOINT_INVENTORY.slice(0, 8).map(
      // retain one maximum-concurrency endpoint group
      (endpoint) => endpoint.id,
    );
    const configuration = k3SupervisorConfiguration(proofPath, endpointIds);
    const startedAt = "2026-10-28T17:00:00.000Z";
    const result = await superviseColdSamples(configuration, {
      // return one complete concurrent K3 group
      async collectGroup() {
        return endpointIds.map(
          // retain generatedAt only for process-local cache endpoints
          (endpointId, index) => rawProbe(
            endpointId,
            `2026-10-28T17:00:01.${String(index).padStart(3, "0")}Z`,
            "2026-10-28T17:00:02.000Z",
            endpointId === "forecast-observations" ? "2026-10-28T17:00:01.500Z" : null,
          ),
        );
      },
      // retain one exact fresh database generation
      async inspectDatabase() {
        const database = observedDatabase(startedAt);
        database.databasePool = {
          backends: [],
          generationId: createHash("sha256").update("[]").digest("hex"),
        };
        return database;
      },
      // retain one stable disposable process generation
      async inspectProcesses() {
        return observedProcesses(startedAt, "disposable-runtime");
      },
    });

    assert.equal(result.supervisionResult.pass, true,
      result.supervisionResult.issues.join("\n"));
    assert.equal(result.witnessFile.witnesses.length, 8);
    const proofHashes = new Set();
    const bindingHashes = new Set();

    // require compact raw-proof binding without duplicated inventories
    for (const witness of result.witnessFile.witnesses) {
      assert.equal(witness.pgdataColdProof.contractVersion,
        "weather-page-d-loop-cold-proof/v1");
      assert.equal(witness.pgdataColdProof.innerResidentPagesAfter, 0);
      assert.equal(witness.pgdataColdProof.outerResidentPagesAfter, 0);
      assert.deepEqual(witness.pgdataColdProof.storageIdentity, dLoopStorageIdentity());
      assert.equal("files" in witness.pgdataColdProof, false);
      proofHashes.add(witness.pgdataColdProof.proofSha256);
      bindingHashes.add(witness.pgdataColdProof.bindingSha256);
    }
    assert.equal(proofHashes.size, 1);
    assert.equal(bindingHashes.size, 8);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// reject K3 proof capabilities outside K3 and require them inside K3
test("supervisor scopes D-loop proof configuration to K3", () => {
  const proofFields = {
    coldProofFile: "/tmp/private-proof.json",
    storageIdentity: dLoopStorageIdentity(),
  };
  assert.throws(
    () => validateSupervisorConfiguration(validSupervisorConfiguration({
      endpointIds: ["current"],
      expiryBufferMs: 0,
      runtimeId: "disposable-runtime",
      tier: "K3",
    })),
    /cold proof/u,
  );

  // prevent K1 and K2 from accepting irrelevant cache-eviction authority
  for (const tier of ["K1", "K2"]) {
    const base = tier === "K1" ? {} : {
      endpointIds: ["current"],
      expiryBufferMs: 0,
      tier,
    };
    assert.throws(
      () => validateSupervisorConfiguration(validSupervisorConfiguration({ ...base, ...proofFields })),
      /only supported for K3/u,
    );
  }
});

// reject malformed dual-layer proofs before dispatching HTTP
test("bundled K3 supervisor rejects incomplete D-loop eviction proofs", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-k3-invalid-"));
  const cases = [
    ["inner-resident", (proof) => { proof.inner.totalResidentAfter = 1; }],
    ["outer-resident", (proof) => { proof.outer.totalResidentAfter = 1; }],
    ["running-attachment", (proof) => { proof.volumes[0].runningAttachments = ["container"]; }],
    ["postmaster", (proof) => { proof.target.postmasterPidAbsent = false; }],
    ["inventory-drift", (proof) => { proof.target.inventoryAfterSha256 = "c".repeat(64); }],
    ["inner-fallback", (proof) => { proof.inner.fallbackUsed = true; }],
    ["inner-mincore", (proof) => { proof.inner.mincoreCompleted = false; }],
    ["outer-fadvise", (proof) => { proof.outer.fadviseCompleted = false; }],
    ["wrong-backing-size", (proof) => { proof.outer.backingFileBytes += 1; }],
    ["wrong-loop", (proof) => { proof.storageIdentity.loopDevice = "/dev/loop1"; }],
    ["wrong-uuid", (proof) => { proof.storageIdentity.loopUuid = "00000000-0000-0000-0000-000000000000"; }],
    ["wrong-mount", (proof) => { proof.storageIdentity.mount = "/mnt/wrong"; }],
    ["wrong-backing", (proof) => { proof.storageIdentity.backingFile = "/mnt/d/wrong.ext4"; }],
  ];
  let dispatches = 0;

  try {
    // isolate every invalid proof in a private immutable fixture
    for (const [name, mutate] of cases) {
      const proof = dLoopColdProof();
      mutate(proof);
      const proofPath = await writeDLoopColdProof(directory, `${name}.json`, proof);
      await assert.rejects(
        () => superviseColdSamples(k3SupervisorConfiguration(proofPath), {
          async collectGroup() {
            dispatches += 1;
            return [];
          },
        }),
        /D-loop cold proof/u,
      );
    }
    assert.equal(dispatches, 0);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// reject proof mutation and observations that do not follow the exact eviction
test("bundled K3 supervisor rechecks proof bytes and startup ordering", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-k3-race-"));
  const original = dLoopColdProof();
  const proofPath = await writeDLoopColdProof(directory, "proof.json", original);
  const configuration = k3SupervisorConfiguration(proofPath);
  const startedAt = "2026-10-28T17:00:00.000Z";
  const controls = {
    async collectGroup() {
      return [rawProbe("current", "2026-10-28T17:00:01.000Z", "2026-10-28T17:00:02.000Z", null)];
    },
    async inspectDatabase() {
      return observedDatabase(startedAt);
    },
    async inspectProcesses() {
      return observedProcesses(startedAt, "disposable-runtime");
    },
  };

  try {
    const mutatedControls = {
      ...controls,
      async collectGroup() {
        await writeFile(proofPath, `${JSON.stringify({ ...original, mutation: true })}\n`, {
          mode: 0o600,
        });
        return controls.collectGroup();
      },
    };
    await assert.rejects(
      () => superviseColdSamples(configuration, mutatedControls),
      /changed after initial validation/u,
    );
    await writeFile(proofPath, `${JSON.stringify(original)}\n`, { mode: 0o600 });

    const late = dLoopColdProof({ completedAt: startedAt });
    late.outer.completedAt = startedAt;
    await writeFile(proofPath, `${JSON.stringify(late)}\n`, { mode: 0o600 });
    await assert.rejects(
      () => superviseColdSamples(configuration, controls),
      /must complete before PostgreSQL starts/u,
    );

    await writeFile(proofPath, `${JSON.stringify(original)}\n`, { mode: 0o600 });
    const mismatchedProcesses = {
      ...controls,
      async inspectProcesses() {
        const processes = observedProcesses(startedAt, "disposable-runtime");
        processes.database.dataVolume = "wrong-volume";
        return processes;
      },
    };
    await assert.rejects(
      () => superviseColdSamples(configuration, mismatchedProcesses),
      /target volume/u,
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// reject public or linked proof inputs before any HTTP request
test("bundled K3 supervisor requires a private owned regular proof file", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-k3-private-"));

  try {
    const proofPath = await writeDLoopColdProof(directory, "proof.json", dLoopColdProof());
    await chmod(proofPath, 0o644);
    await assert.rejects(
      () => superviseColdSamples(k3SupervisorConfiguration(proofPath)),
      /must not grant group or other permissions/u,
    );
    await chmod(proofPath, 0o400);
    await assert.rejects(
      () => superviseColdSamples(k3SupervisorConfiguration(proofPath)),
      /must have mode 0600/u,
    );
    await chmod(proofPath, 0o600);
    const linkPath = join(directory, "proof-link.json");
    await symlink(proofPath, linkPath);
    await assert.rejects(
      () => superviseColdSamples(k3SupervisorConfiguration(linkPath)),
      /must not be a symbolic link/u,
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// observe both shared-cache consumers during one natural refill
test("bundled supervisor binds overlapping K1 responses to observed runtime facts", async () => {
  const primeGeneratedAt = "2026-10-28T17:00:00.000Z";
  const dispatchAt = "2026-10-28T17:05:02.000Z";
  const completedAt = "2026-10-28T17:05:03.000Z";
  const refillGeneratedAt = "2026-10-28T17:05:02.100Z";
  const groups = [
    [
      rawProbe("forecast-observations", primeGeneratedAt, "2026-10-28T17:00:01.000Z", primeGeneratedAt),
      rawProbe("solar-cloud-evidence", primeGeneratedAt, "2026-10-28T17:00:01.000Z", primeGeneratedAt),
    ],
    [
      rawProbe("forecast-observations", dispatchAt, completedAt, refillGeneratedAt),
      rawProbe("solar-cloud-evidence", "2026-10-28T17:05:02.001Z", completedAt, refillGeneratedAt),
    ],
  ];
  const waits = [];
  const result = await superviseColdSamples(validSupervisorConfiguration(), {
    // return one deterministic concurrent probe group
    async collectGroup() {
      return groups.shift();
    },
    // return one observed pool and database identity
    async inspectDatabase() {
      return observedDatabase();
    },
    // retain one stable service generation around the request
    async inspectProcesses() {
      return observedProcesses();
    },
    // anchor the natural-expiry wait before the known deadline
    now() {
      return Date.parse("2026-10-28T17:00:01.000Z");
    },
    // record rather than delay the unit fixture
    async sleep(milliseconds) {
      waits.push(milliseconds);
    },
  });

  assert.deepEqual(waits, [301_000]);
  assert.equal(result.evidence.samples.length, 2);
  assert.equal(result.witnessFile.witnesses.length, 2);
  assert.equal(result.witnessFile.producer, SUPERVISOR_PRODUCER);
  assert.deepEqual(
    result.witnessFile.witnesses[0].applicationCache.cohortWitnessIds,
    result.witnessFile.witnesses.map(
      // select the emitted exact witness order
      function selectWitnessId(witness) {
        return witness.id;
      },
    ),
  );
  assert.equal(result.witnessFile.witnesses[0].applicationCache.expiredAt, "2026-10-28T17:05:00.000Z");
});

// refuse a pair where the second request begins after the first completed
test("bundled supervisor retains serialized K1 responses as failed evidence", async () => {
  const primeGeneratedAt = "2026-10-28T17:00:00.000Z";
  const groups = [
    [
      rawProbe("forecast-observations", primeGeneratedAt, "2026-10-28T17:00:01.000Z", primeGeneratedAt),
      rawProbe("solar-cloud-evidence", primeGeneratedAt, "2026-10-28T17:00:01.000Z", primeGeneratedAt),
    ],
    [
      rawProbe("forecast-observations", "2026-10-28T17:05:02.000Z", "2026-10-28T17:05:03.000Z", "2026-10-28T17:05:02.100Z"),
      rawProbe("solar-cloud-evidence", "2026-10-28T17:05:04.000Z", "2026-10-28T17:05:05.000Z", "2026-10-28T17:05:02.100Z"),
    ],
  ];

  const result = await superviseColdSamples(validSupervisorConfiguration(), {
      // return one deterministic serialized probe group
      async collectGroup() {
        return groups.shift();
      },
      // return one observed pool and database identity
      async inspectDatabase() {
        return observedDatabase();
      },
      // retain one stable service generation around the request
      async inspectProcesses() {
        return observedProcesses();
      },
      // anchor the fixture after natural expiry
      now() {
        return Date.parse("2026-10-28T17:05:02.000Z");
      },
      // avoid real time in the unit fixture
      async sleep() {},
    });

  assert.equal(result.evidence.samples.length, 2);
  assert.equal(result.witnessFile.witnesses.length, 2);
  assert.equal(result.supervisionResult.pass, false);
  assert.match(result.supervisionResult.issues.join("\n"), /do not overlap one observed refill/u);
});

// retain transport and body failures in the measured denominator
test("bundled supervisor records failed HTTP attempts before rejecting their labels", async () => {
  const primeGeneratedAt = "2026-10-28T17:00:00.000Z";
  const groups = [
    [
      rawProbe("forecast-observations", primeGeneratedAt, "2026-10-28T17:00:01.000Z", primeGeneratedAt),
      rawProbe("solar-cloud-evidence", primeGeneratedAt, "2026-10-28T17:00:01.000Z", primeGeneratedAt),
    ],
    [
      {
        ...rawProbe("forecast-observations", "2026-10-28T17:05:02.000Z", "2026-10-28T17:05:03.000Z", null),
        bodyComplete: false,
        error: "socket closed",
        status: 502,
      },
      rawProbe("solar-cloud-evidence", "2026-10-28T17:05:02.001Z", "2026-10-28T17:05:03.000Z", "2026-10-28T17:05:02.100Z"),
    ],
  ];
  const result = await superviseColdSamples(validSupervisorConfiguration(), {
    // return one deterministic failed probe group
    async collectGroup() {
      return groups.shift();
    },
    // return one observed pool and database identity
    async inspectDatabase() {
      return observedDatabase();
    },
    // retain one stable service generation around the request
    async inspectProcesses() {
      return observedProcesses();
    },
    // anchor the fixture after natural expiry
    now() {
      return Date.parse("2026-10-28T17:05:02.000Z");
    },
    // avoid real time in the unit fixture
    async sleep() {},
  });

  assert.equal(result.evidence.samples.length, 2);
  assert.equal(result.evidence.samples[0].status, 502);
  assert.equal(result.evidence.samples[0].bodyComplete, false);
  assert.equal(result.evidence.samples[0].error, "socket closed");
  assert.equal(result.supervisionResult.pass, false);
  assert.match(result.supervisionResult.issues.join("\n"), /HTTP status 502/u);
  assert.match(result.supervisionResult.issues.join("\n"), /response body was not completed/u);
  assert.match(result.supervisionResult.issues.join("\n"), /transport error/u);
});

// accept missing revision labels only outside the source-owning web image
test("docker observations retain null API and PostgreSQL source revisions", () => {
  const observation = parseDockerContainerObservation([
    JSON.stringify("container-api"),
    JSON.stringify("2026-10-28T17:00:00.000Z"),
    JSON.stringify(42),
    JSON.stringify(`sha256:${"a".repeat(64)}`),
    JSON.stringify("weather-api@sha256:digest"),
    JSON.stringify("weather"),
    JSON.stringify("api"),
    JSON.stringify(null),
    JSON.stringify([]),
  ].join("\t"));

  assert.equal(observation.sourceCommit, null);
  assert.equal(observation.configImage, "weather-api@sha256:digest");
});

// serve delayed and aborted bodies for transport tests
function fixtureHandler(request, response) {
  // delay completion for the current response
  if (request.url === "/api/v1/sites/ballydidean/current") {
    response.writeHead(200, { "Content-Type": "application/json" });
    response.write("{\"generatedAt\":\"2026-10-28T17:00:00.000Z\",\"data\":");

    // complete the delayed body
    function finishCurrent() {
      response.end("true}");
    }

    setTimeout(finishCurrent, 60);
    return;
  }

  response.writeHead(200, { "Content-Type": "application/json" });
  response.write("{\"data\":");

  // abort after a partial body
  function abortDaily() {
    response.destroy();
  }

  setTimeout(abortDaily, 10);
}

// listen on one random loopback port
async function listen(server) {
  await new Promise(function waitForListening(resolvePromise, rejectPromise) {
    server.once("error", rejectPromise);

    // resolve only after listening
    function handleListening() {
      server.off("error", rejectPromise);
      resolvePromise();
    }

    server.listen(0, "127.0.0.1", handleListening);
  });
}

// close one fixture server
async function close(server) {
  await new Promise(function waitForClose(resolvePromise, rejectPromise) {
    // resolve the callback-style close
    function handleClose(error) {
      // reject close failures
      if (error !== undefined) {
        rejectPromise(error);
        return;
      }
      resolvePromise();
    }

    server.close(handleClose);
  });
}

// include the complete response body in elapsed HTTP time
test("HTTP collector ends timing only after body completion and retains aborts", async () => {
  const server = createServer(fixtureHandler);
  await listen(server);
  const address = server.address();
  assert.notEqual(address, null);
  assert.equal(typeof address, "object");
  const origin = `http://127.0.0.1:${String(address.port)}`;
  const current = ENDPOINT_INVENTORY[0];
  const daily = ENDPOINT_INVENTORY[1];

  try {
    const complete = await collectHttpSample({
      agent: false,
      cacheStateId: "fixture-warm",
      eligibility: current.eligibility,
      endpointId: current.id,
      origin,
      run: 1,
      runtimeId: "blueberry-runtime",
      tier: "warm",
      timeoutMs: 1_000,
      topology: topologyFor(current, "warm"),
      witnessId: null,
    });
    const aborted = await collectHttpSample({
      agent: false,
      cacheStateId: "fixture-abort",
      eligibility: daily.eligibility,
      endpointId: daily.id,
      origin,
      run: 1,
      runtimeId: "blueberry-runtime",
      tier: "warm",
      timeoutMs: 1_000,
      topology: topologyFor(daily, "warm"),
      witnessId: null,
    });

    assert.equal(complete.bodyComplete, true);
    assert.ok(complete.responseBytes > 13);
    assert.equal(complete.responseGeneratedAt, "2026-10-28T17:00:00.000Z");
    assert.equal(complete.topology.connection, "fresh");
    assert.ok(complete.elapsedMs >= 50, `expected delayed body timing, received ${String(complete.elapsedMs)}ms`);
    assert.equal(aborted.bodyComplete, false);
    assert.notEqual(aborted.error, null);
  } finally {
    await close(server);
  }
});

// reject remote origins and secret-bearing configuration
test("configuration permits only bounded loopback collection without secrets", () => {
  assert.equal(validateLoopbackOrigin("http://127.0.0.1:3000"), "http://127.0.0.1:3000");
  // reject a remote origin
  assert.throws(() => validateLoopbackOrigin("https://weather.ballydidean.farm"), /loopback HTTP origin/u);
  // reject inline origin credentials
  assert.throws(() => validateLoopbackOrigin("http://user:pass@127.0.0.1:3000"), /credential-free/u);
  // reject secret-shaped top-level fields
  assert.throws(
    () => validateConfiguration(validConfiguration({ authorizationToken: "do-not-store" })),
    /unsupported configuration field/u,
  );
  const invalidLoad = validConfiguration();
  invalidLoad.measurement.loadCondition = "idle";
  // reject an idle substitute for normal ingestion
  assert.throws(() => validateConfiguration(invalidLoad), /normal-ingestion/u);

  const current = ENDPOINT_INVENTORY[0];
  const collection = {
    cacheStateId: "warm-fixture",
    count: 1,
    eligibility: current.eligibility,
    endpointId: current.id,
    origin: "http://127.0.0.1:3000",
    run: 1,
    runtimeId: "blueberry-runtime",
    tier: "warm",
    topology: topologyFor(current, "warm"),
  };
  assert.equal(validateConfiguration(validConfiguration({ collections: [collection] })).collections.length, 1);
  // reject secret-bearing nested request controls
  assert.throws(
    () => validateConfiguration(validConfiguration({ collections: [{ ...collection, headers: { cookie: "secret" } }] })),
    /forbidden/u,
  );
});

// enforce private ownership and reject symlink inputs
test("private evidence paths reject public modes and symbolic links", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-page-performance-private-"));
  const privatePath = join(directory, "private.json");
  const linkPath = join(directory, "link.json");

  try {
    await chmod(directory, 0o700);
    await writeFile(privatePath, "{}\n", { mode: 0o600 });
    await assertPrivateOwnedPath(privatePath);
    await chmod(privatePath, 0o644);
    // reject public file permissions
    await assert.rejects(() => assertPrivateOwnedPath(privatePath), /group or other/u);
    await chmod(privatePath, 0o600);
    await symlink(privatePath, linkPath);
    // reject a link to a private file
    await assert.rejects(() => assertPrivateOwnedPath(linkPath), /symbolic link/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// judge existing private samples and emit immutable private artifacts
test("evaluate CLI judges existing evidence without collecting or overwriting", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-page-performance-cli-"));
  const configPath = join(directory, "config.json");
  const evidencePath = join(directory, "input-evidence.json");
  const witnessPath = join(directory, "state-witnesses.json");
  const outputPath = join(directory, "output");
  const evidence = passingEvidence();
  const witnesses = Object.values(evidence.stateWitnesses);
  evidence.stateWitnesses = {};
  const configuration = validConfiguration({
    evidenceFiles: ["input-evidence.json"],
    stateWitnessFiles: ["state-witnesses.json"],
  });

  try {
    await chmod(directory, 0o700);
    await writeFile(evidencePath, `${JSON.stringify(evidence)}\n`, { mode: 0o600 });
    await writeFile(witnessPath, `${JSON.stringify({
      contractVersion: "weather-page-state-witness/v1",
      producer: SUPERVISOR_PRODUCER,
      witnesses,
    })}\n`, { mode: 0o600 });
    await writeFile(configPath, `${JSON.stringify(configuration)}\n`, { mode: 0o600 });
    assert.equal(await runCli(["evaluate", "--config", configPath, "--output", outputPath]), 0);
    const result = JSON.parse(await readFile(join(outputPath, "result.json"), "utf8"));
    const outputMetadata = await lstat(join(outputPath, "evidence.json"));

    assert.equal(result.pass, true);
    assert.equal(outputMetadata.mode & 0o077, 0);
    // reject overwriting prior evidence
    await assert.rejects(
      () => runCli(["evaluate", "--config", configPath, "--output", outputPath]),
      /EEXIST/u,
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});
