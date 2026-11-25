import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { pathToFileURL } from "node:url";

import { runWorkerIteration } from "../apps/worker/dist/index.js";
import { loadSiteConfiguration } from "../packages/database/dist/index.js";
import {
  BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
  evaluateBackendPerformanceDirect,
} from "./backend-performance-direct-evaluator.mjs";
import { beginDisposableResourceInterval } from
  "./backend-performance-direct-pipeline-check.mjs";
import {
  BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
  BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";

const WORKLOAD_URL = new URL("../config/backend-performance-workload-v2.json", import.meta.url);
const WORKLOAD_SHA256 = "b44eb296f1145a55202e941e24e750c0b0d5a4c37075c6f8e9ca346470bca53f";
const SITE_URL = new URL("../config/sites/ballydidean.json", import.meta.url);
const FIXED_NOW = "2026-08-22T05:20:00.000Z";
const RECORD_VALID_AT = "2026-08-22T05:19:00.000Z";
const SOURCE_ID = "00000000-0000-4000-8000-000000000001";

// create one controlled ingestion-fixture error
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one ingestion-fixture invariant
function requireInvariant(condition, code, message) {
  // fail closed on source or execution drift
  if (!condition) {
    fail(code, message);
  }
}

// load the reviewed current-provider source operation
function loadSourceBinding() {
  const workloadBytes = readFileSync(WORKLOAD_URL);
  requireInvariant(
    createHash("sha256").update(workloadBytes).digest("hex") === WORKLOAD_SHA256,
    "direct-worker-ingestion.workload",
    "backend performance workload identity changed",
  );
  const workload = JSON.parse(workloadBytes.toString("utf8"));
  const operation = workload.operations.find(
    // select the reviewed current success operation
    (value) => value.id === "provider.open-meteo-current.success",
  );
  requireInvariant(
    operation !== undefined && operation.exactHttp === null
      && JSON.stringify(operation.nodeArgv) === JSON.stringify([{
        phase: "execute",
        argv: ["node", "apps/worker/dist/worker.js", "--once"],
      }])
      && operation.stateEffects === "provider-cassette-read+database-ingestion/checkpoint"
      && operation.completedWorkUnit === "validated-provider-scenario",
    "direct-worker-ingestion.operation",
    "reviewed current-provider operation changed",
  );
  const attempts = [];
  const schedule = buildBackendPerformanceFunctionalSchedule();
  // visit every reviewed source run slot
  for (const runSlot of schedule.runSlots) {
    const selected = [];
    // visit every source group
    for (const group of runSlot.groups) {
      // select the exact current-provider success attempt
      for (const attempt of group.attempts) {
        // retain one once-per-run source attempt
        if (attempt.operationId === operation.id) {
          selected.push(attempt);
        }
      }
    }
    requireInvariant(
      selected.length === 1 && selected[0].stratum === "once-per-run",
      "direct-worker-ingestion.schedule",
      "each run slot must contain one current-provider success attempt",
    );
    attempts.push(selected[0]);
  }
  requireInvariant(
    attempts.length === 5,
    "direct-worker-ingestion.schedule",
    "current-provider fixture requires all five reviewed run slots",
  );
  return { attempts, operation };
}

// build one exact active current-source discovery row
function createDueSource(site) {
  const configuration = site.sources.find(
    // select the active current source configuration
    (source) => source.key === "open-meteo-current-v1",
  );
  requireInvariant(
    configuration !== undefined,
    "direct-worker-ingestion.site",
    "site configuration has no active current source",
  );
  return {
    active: true,
    cadenceSeconds: configuration.cadenceSeconds,
    id: SOURCE_ID,
    materialProviderConfig: configuration.adapterConfig,
    providerKey: site.provider.key,
    siteSlug: site.site.key,
    sourceConfigFingerprint: configuration.fingerprint,
    sourceKey: configuration.key,
    sourceKind: configuration.sourceKind,
    stationSlug: site.station.key,
    timezone: site.site.timezone,
  };
}

// create one isolated repository completion boundary
function createRepository(runSlotId, dueSource, observation) {
  const session = {
    // release one retained fake source session
    async release() {
      observation.events.push("release");
    },
    sourceId: SOURCE_ID,
  };
  return {
    // retain expired-run cleanup order
    async abandonExpiredRuns() {
      observation.events.push("abandon-expired");
      return [];
    },
    // acquire one isolated source session
    async acquireSourceSession() {
      observation.events.push("acquire-session");
      return session;
    },
    // capture the worker's single atomic completion handoff
    async completeScheduledIngestion(_session, input) {
      observation.events.push("complete-ingestion");
      observation.completion = input;
    },
    // return one reviewed due source
    async discoverDueSources() {
      observation.events.push("discover-due-source");
      return [dueSource];
    },
    // reject unexpected failure finalization in the success fixture
    async failIngestionRun() {
      observation.events.push("fail-ingestion");
    },
    // return one prior checkpoint version
    async getScheduledCheckpoint() {
      observation.events.push("read-checkpoint");
      return {
        lastCommittedAt: "2026-08-22T05:00:01.000Z",
        lastValidAt: "2026-08-22T04:59:00.000Z",
        providerCursor: null,
        sourceId: SOURCE_ID,
        version: 7,
        windowEndExclusive: "2026-08-22T05:00:00.000Z",
        windowStart: "2026-08-22T04:45:00.000Z",
      };
    },
    // capture the committed running-row boundary
    async startIngestionRun(_session, input) {
      observation.events.push("start-ingestion");
      observation.started = input;
      return {
        id: `direct-worker-${runSlotId}`,
        startedAt: FIXED_NOW,
        state: "running",
      };
    },
    // capture the iteration heartbeat
    async updateWorkerHeartbeat(_pool, input) {
      observation.events.push("update-heartbeat");
      observation.heartbeat = input;
    },
  };
}

// run one compiled worker iteration against disposable collaborators
async function executeWorkerIteration(runSlotId, site) {
  const observation = {
    completion: null,
    events: [],
    heartbeat: null,
    providerInput: null,
    started: null,
  };
  const dueSource = createDueSource(site);
  const repository = createRepository(runSlotId, dueSource, observation);
  const diagnostics = [];
  const result = await runWorkerIteration({}, {
    // retain bounded worker diagnostics
    diagnosticWriter(diagnostic) {
      diagnostics.push(diagnostic);
    },
    // return one normalized provider batch without external traffic
    async fetchCurrent(input) {
      observation.events.push("fetch-current");
      observation.providerInput = input;
      return {
        attempts: 1,
        checksum: "f".repeat(64),
        providerCursor: { fixture_cursor: runSlotId },
        records: [{ validAt: RECORD_VALID_AT }],
        responseMetadata: { fixture: true },
      };
    },
    instance: `direct-${runSlotId}`,
    lastSuccessAt: null,
    // hold every worker clock read at one reviewed instant
    now() {
      return new Date(FIXED_NOW);
    },
    repository,
    site,
    version: "direct-worker-ingestion-fixture",
  });
  return { diagnostics, observation, result, runSlotId };
}

// listen on one disposable loopback port
function listen(server) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      const address = server.address();
      // require one assigned loopback address
      if (address === null || typeof address === "string") {
        reject(new Error("worker ingestion fixture listener has no TCP address"));
        return;
      }
      resolve(`http://127.0.0.1:${String(address.port)}`);
    });
  });
}

// close one disposable loopback server
function close(server) {
  return new Promise((resolve, reject) => {
    server.close((error) => {
      // reject one close failure
      if (error !== undefined) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}

// classify one actual compiled-worker completion response
function classifyWorkerResponse({ attempt, bodyBytes }) {
  const body = JSON.parse(bodyBytes.toString("utf8"));
  const completion = body.observation?.completion;
  const source = body.result?.sources?.[0];
  const completed = body.runSlotId === attempt.runSlotId
    && source?.status === "succeeded"
    && source.recordCount === 1
    && source.runId === `direct-worker-${attempt.runSlotId}`
    && completion?.expectedCheckpointVersion === 7
    && completion?.records?.length === 1
    && completion.records[0].validAt === RECORD_VALID_AT
    && completion.lastValidAt === RECORD_VALID_AT
    && completion.runId === source.runId
    && body.observation.events.indexOf("start-ingestion")
      < body.observation.events.indexOf("fetch-current")
    && body.observation.events.indexOf("fetch-current")
      < body.observation.events.indexOf("complete-ingestion")
    && body.observation.events.at(-1) === "update-heartbeat";
  return {
    classification: completed
      ? "compiled-worker-current-ingestion-causal-subset-complete"
      : "compiled-worker-current-ingestion-invalid",
    completed,
  };
}

// execute five reviewed provider attempts through the compiled worker
export async function runBackendPerformanceDirectWorkerIngestionCheck() {
  const source = loadSourceBinding();
  const site = await loadSiteConfiguration(SITE_URL.pathname);
  const observations = [];
  const server = createServer(async (request, response) => {
    const url = new URL(request.url ?? "/", "http://fixture.invalid");
    const prefix = "/__direct/worker/open-meteo-current-success/";
    // accept only one fixture control route
    if (request.method !== "POST" || !url.pathname.startsWith(prefix)) {
      response.writeHead(404);
      response.end();
      return;
    }
    const runSlotId = url.pathname.slice(prefix.length);
    try {
      const execution = await executeWorkerIteration(runSlotId, site);
      observations.push(execution);
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify(execution));
    } catch (error) {
      response.writeHead(500, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: error instanceof Error ? error.message : String(error) }));
    }
  });
  const baseUrl = await listen(server);
  try {
    const result = await evaluateBackendPerformanceDirect({
      attempts: source.attempts.map(
        // adapt one non-http source attempt to the disposable control route
        (attempt) => ({
          attemptId: attempt.attemptId,
          body: null,
          expectedStatuses: [200],
          headers: {},
          method: "POST",
          operationId: attempt.operationId,
          path: `/__direct/worker/open-meteo-current-success/${attempt.runSlotId}`,
          runSlotId: attempt.runSlotId,
          stratum: attempt.stratum,
          timeoutMs: attempt.deadlineMs,
        }),
      ),
      baseUrl,
      contractVersion: BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
      generation: "node-v1",
      maximumResponseBytes: 64 * 1024,
      resourceInterval: { scopeId: "compiled-worker-ingestion-fixture" },
      role: "baseline",
      runId: "compiled-worker-current-ingestion-fixture",
    }, {
      beginResourceInterval: beginDisposableResourceInterval,
      classifyResponse: classifyWorkerResponse,
    });
    return {
      attemptDenominator: result.summary.durationCoverage,
      completionHandoffs: observations.map(
        // project one atomic repository input boundary
        (execution) => ({
          attempts: execution.observation.completion.attempts,
          expectedCheckpointVersion:
            execution.observation.completion.expectedCheckpointVersion,
          lastValidAt: execution.observation.completion.lastValidAt,
          recordCount: execution.observation.completion.records.length,
          runId: execution.observation.completion.runId,
          runSlotId: execution.runSlotId,
          windowEndExclusive: execution.observation.completion.windowEndExclusive,
          windowStart: execution.observation.completion.windowStart,
        }),
      ),
      evidenceClass: "synthetic-fixture",
      execution: {
        classification: "compiled-runWorkerIteration-current-ingestion-causal-subset",
        completionCalls: observations.filter(
          // count one completed repository boundary
          (execution) => execution.observation.completion !== null,
        ).length,
        heartbeatCalls: observations.filter(
          // count one retained heartbeat boundary
          (execution) => execution.observation.heartbeat !== null,
        ).length,
        providerCalls: observations.filter(
          // count one injected provider boundary
          (execution) => execution.observation.providerInput !== null,
        ).length,
        recordsHandedOff: observations.reduce(
          // add every atomic completion record count
          (count, execution) => count + execution.observation.completion.records.length,
          0,
        ),
      },
      nativeEligible: false,
      operationStratum: result.summary.operationStrata[0],
      resourceEvidence: {
        contractVersion: result.resourceEvidence.contractVersion,
        kind: result.resourceEvidence.authority.kind,
        nativeEligible: result.resourceEvidence.authority.nativeEligible,
        sampleCount: result.resourceEvidence.sampleCount,
      },
      semanticClassifications: [...new Set(result.attempts.map(
        // retain every terminal semantic classification
        (attempt) => attempt.semanticClassification,
      ))],
      sourceBinding: {
        functionalDefinitionSha256: BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
        functionalScheduleVersion: BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
        nodeArgv: source.operation.nodeArgv,
        operationId: source.operation.id,
        sourceAttemptIds: source.attempts.map(
          // retain one exact source attempt identity
          (attempt) => attempt.attemptId,
        ),
        workloadSha256: WORKLOAD_SHA256,
      },
      terminalCounts: result.summary.terminalCounts,
      unsupportedBoundaries: [
        "apps/worker/dist/worker.js --once process startup is not executed",
        "the injected provider bypasses cassette transport and parser execution",
        "the injected repository captures the atomic completion input without a database transaction",
      ],
    };
  } finally {
    await close(server);
  }
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runBackendPerformanceDirectWorkerIngestionCheck().then(
    // emit one machine-readable ingestion result
    (result) => process.stdout.write(`${JSON.stringify(result, null, 2)}\n`),
    // emit one controlled ingestion failure
    (error) => {
      process.stderr.write(`${error?.code ?? "direct-worker-ingestion.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    },
  );
}
