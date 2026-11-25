import { createServer } from "node:http";
import { pathToFileURL } from "node:url";

import {
  BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
  evaluateBackendPerformanceDirect,
} from "./backend-performance-direct-evaluator.mjs";

// listen on one disposable loopback port
function listen(server) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
}

// close one disposable server
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

// run one executable local evaluator proof
export async function runBackendPerformanceDirectCheck() {
  const server = createServer((request, response) => {
    // return one ordinary successful request
    if (request.url === "/ok") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end('{"ok":true}');
      return;
    }
    response.writeHead(503, { "content-type": "application/json" });
    response.end('{"ok":false}');
  });
  await listen(server);
  try {
    const address = server.address();
    // require one assigned tcp address
    if (address === null || typeof address === "string") {
      throw new Error("disposable evaluator listener has no tcp address");
    }
    return await evaluateBackendPerformanceDirect({
      attempts: [
        {
          attemptId: "direct-check-success",
          body: null,
          expectedStatuses: [200],
          headers: {},
          method: "GET",
          operationId: "direct-check.success",
          path: "/ok",
          runSlotId: "direct-check",
          stratum: "warm",
          timeoutMs: 2_000,
        },
        {
          attemptId: "direct-check-failure",
          body: null,
          expectedStatuses: [200],
          headers: {},
          method: "GET",
          operationId: "direct-check.failure",
          path: "/ordinary-failure",
          runSlotId: "direct-check",
          stratum: "warm",
          timeoutMs: 2_000,
        },
      ],
      baseUrl: `http://127.0.0.1:${String(address.port)}`,
      contractVersion: BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
      generation: "disposable-local",
      maximumResponseBytes: 1_024,
      resourceInterval: null,
      role: "baseline",
      runId: "direct-disposable-check",
    });
  } finally {
    await close(server);
  }
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runBackendPerformanceDirectCheck().then(
    // emit one machine-readable result
    (result) => process.stdout.write(`${JSON.stringify(result, null, 2)}\n`),
    // emit one controlled failure
    (error) => {
      process.stderr.write(`${error?.code ?? "direct-check.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    },
  );
}
