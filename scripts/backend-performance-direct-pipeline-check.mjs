import { createServer } from "node:http";
import { chmod, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { bindBackendPerformanceDirectSchedule } from
  "./backend-performance-direct-binding.mjs";
import { collectBackendPerformanceDirectBaseline } from
  "./backend-performance-direct-pipeline.mjs";
import {
  writeBackendPerformanceDirectBaseline,
  writeBackendPerformanceDirectTarget,
} from "./backend-performance-direct-target.mjs";
import { buildBackendPerformanceFunctionalSchedule } from
  "./backend-performance-functional-schedule.mjs";

const REFERENCE_TIME = "2026-11-23T12:00:00.000Z";

// select forty warm attempts for both source operations
function selectedAttemptIds() {
  const selected = [];
  const counts = new Map([
    ["page.home", 0],
    ["api.health", 0],
    ["api.private-forecast-observation-input", 0],
    ["maintenance.schedule-slots", 0],
  ]);
  const schedule = buildBackendPerformanceFunctionalSchedule();
  // visit each fixed source slot
  for (const runSlot of schedule.runSlots) {
    // visit each fixed source group
    for (const group of runSlot.groups) {
      // retain the first run-slot warm attempts
      for (const attempt of group.attempts) {
        const count = counts.get(attempt.operationId);
        // retain forty attempts per selected operation
        const limit = attempt.operationId === "page.home" || attempt.operationId === "api.health"
          ? 40
          : 1;
        // retain warm matrices and once-per-run private reads
        if (runSlot.id === "run-slot-01"
          && (attempt.stratum === "warm" || attempt.stratum === "once-per-run")
          && count !== undefined && count < limit) {
          selected.push(attempt.attemptId);
          counts.set(attempt.operationId, count + 1);
        }
      }
    }
  }
  // require the intended fixed source sample
  if (selected.length !== 82) {
    throw new Error("disposable pipeline did not select eighty-two source attempts");
  }
  return selected;
}

// classify typed ordinary fixture responses
function classifyFixtureResponse({ attempt, bodyBytes }) {
  // classify the typed private forecast input
  if (attempt.operationId === "api.private-forecast-observation-input") {
    const value = JSON.parse(bodyBytes.toString("utf8"));
    const completed = value.contractVersion === "forecast-observation-input/v1"
      && value.generatedAt === REFERENCE_TIME && Array.isArray(value.records);
    return {
      classification: completed
        ? "forecast-observation-input-v1-at-reference-time"
        : "forecast-observation-input-invalid",
      completed,
    };
  }
  // classify the three-family maintenance projection
  if (attempt.operationId === "maintenance.schedule-slots") {
    const value = JSON.parse(bodyBytes.toString("utf8"));
    const completed = value.capturedAt === REFERENCE_TIME
      && JSON.stringify(value.families) === JSON.stringify(["temperature", "wind", "rain"]);
    return {
      classification: completed
        ? "maintenance-three-family-schedule-at-reference-time"
        : "maintenance-schedule-invalid",
      completed,
    };
  }
  return { classification: "ordinary-http-status-and-complete-body", completed: true };
}

// listen on one disposable loopback port
function listen(server) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      const address = server.address();
      // require one assigned tcp address
      if (address === null || typeof address === "string") {
        reject(new Error("pipeline fixture listener has no tcp address"));
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

// collect one explicitly non-cgroup fixture resource interval
export async function beginDisposableResourceInterval() {
  const cpuBefore = process.cpuUsage();
  const samples = [process.memoryUsage().rss];
  let closed = false;
  return {
    // close one abandoned fixture interval
    async abort() {
      closed = true;
    },
    // finish one fixture process observation
    async finish() {
      const cpu = process.cpuUsage(cpuBefore);
      samples.push(process.memoryUsage().rss);
      closed = true;
      return {
        authority: {
          evidenceClass: "synthetic-fixture",
          kind: "disposable-process-snapshot-not-cgroup",
          nativeEligible: false,
        },
        contractVersion: "weather-backend-disposable-process-resource/v1",
        cpu: { usageUsec: cpu.user + cpu.system },
        memory: {
          peak: { valueBytes: Math.max(...samples) },
          steady: { sampledMaximumBytes: Math.max(...samples) },
        },
        sampleCount: samples.length,
      };
    },
    // append one fixture process sample
    async sample() {
      // reject samples after closure
      if (closed) {
        throw new Error("disposable resource interval is closed");
      }
      samples.push(process.memoryUsage().rss);
    },
  };
}

// run one complete disposable baseline-to-target pipeline
export async function runBackendPerformanceDirectPipelineCheck() {
  const server = createServer((request, response) => {
    // serve the two selected reviewed operations
    if (request.url === "/" || request.url === "/api/v1/health") {
      setTimeout(
        // dominate scheduler jitter with fixed fixture work
        () => {
          response.writeHead(200, { "content-type": "application/json" });
          response.end('{"ok":true}');
        },
        5,
      );
      return;
    }
    // serve one typed private forecast input at the explicit fixture time
    if (request.url === "/internal/v1/forecast-observation-input") {
      setTimeout(
        // retain one fixed disposable input workload
        () => {
          response.writeHead(200, { "content-type": "application/json" });
          response.end(JSON.stringify({
            contractVersion: "forecast-observation-input/v1",
            generatedAt: REFERENCE_TIME,
            records: [],
          }));
        },
        5,
      );
      return;
    }
    // serve one typed maintenance schedule at the explicit fixture time
    if (request.url === "/internal/adjustment-maintenance/schedule-slots") {
      setTimeout(
        // retain one fixed disposable schedule workload
        () => {
          response.writeHead(200, { "content-type": "application/json" });
          response.end(JSON.stringify({
            capturedAt: REFERENCE_TIME,
            families: ["temperature", "wind", "rain"],
          }));
        },
        5,
      );
      return;
    }
    response.writeHead(404);
    response.end();
  });
  const baseUrl = await listen(server);
  const directory = await mkdtemp(join(tmpdir(), "weather-direct-pipeline-"));
  await chmod(directory, 0o700);
  try {
    const warmupBinding = bindBackendPerformanceDirectSchedule({
      attemptIds: selectedAttemptIds(),
      baseUrl,
      generation: "node-v1",
      maximumResponseBytes: 1_024,
      resourceInterval: null,
      role: "baseline",
      runId: "pipeline-fixture-warmup",
    });
    await collectBackendPerformanceDirectBaseline(warmupBinding, {
      evidenceClass: "synthetic-fixture",
      evaluatorAdapters: { classifyResponse: classifyFixtureResponse },
    });
    const baselineRuns = [];
    // collect exactly five independent fixture repetitions
    for (let index = 0; index < 5; index += 1) {
      const binding = bindBackendPerformanceDirectSchedule({
        attemptIds: selectedAttemptIds(),
        baseUrl,
        generation: "node-v1",
        maximumResponseBytes: 1_024,
        resourceInterval: { scopeId: "disposable-process" },
        role: "baseline",
        runId: `pipeline-fixture-${String(index + 1)}`,
      });
      const collected = await collectBackendPerformanceDirectBaseline(binding, {
        evidenceClass: "synthetic-fixture",
        evaluatorAdapters: {
          beginResourceInterval: beginDisposableResourceInterval,
          classifyResponse: classifyFixtureResponse,
        },
      });
      await writeBackendPerformanceDirectBaseline(directory, collected.baseline);
      baselineRuns.push(collected.baseline);
    }
    const publication = await writeBackendPerformanceDirectTarget(directory, {
      baselineRuns,
      contractVersion: "weather-backend-performance-direct-target-input/v1",
    });
    return {
      attemptDenominator: baselineRuns[0].attemptDenominator,
      baselineCount: baselineRuns.length,
      classifications: baselineRuns[0].classifications,
      evidenceClass: publication.target.evidenceClass,
      metricTargets: publication.target.metricTargets,
      resourceEvidencePresent: baselineRuns.every(
        // require one measured fixture resource observation
        (run) => run.resourceEvidenceSha256 !== null,
      ),
      targetReceiptSha256: publication.receiptSha256,
    };
  } finally {
    await Promise.all([
      close(server),
      rm(directory, { force: true, recursive: true }),
    ]);
  }
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runBackendPerformanceDirectPipelineCheck().then(
    // emit one machine-readable fixture result
    (result) => process.stdout.write(`${JSON.stringify(result, null, 2)}\n`),
    // emit one controlled fixture failure
    (error) => {
      process.stderr.write(`${error?.code ?? "direct-pipeline.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    },
  );
}
