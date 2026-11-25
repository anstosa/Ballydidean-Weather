import {
  createWeatherApi,
  createWeatherApiServer,
} from "../apps/api/dist/index.js";

const REFERENCE_TIME = "2026-08-22T05:00:00.000Z";

let server = null;
let shuttingDown = false;

// report one child message
function report(message, callback = undefined) {
  // fail closed without the parent ipc channel
  if (typeof process.send !== "function") {
    process.exitCode = 2;
    return;
  }
  process.send(message, callback);
}

// capture one bounded process usage snapshot
function resourceSnapshot() {
  const usage = process.resourceUsage();
  return {
    involuntaryContextSwitches: usage.involuntaryContextSwitches,
    maximumResidentSetBytes: usage.maxRSS * 1024,
    systemCpuMicroseconds: usage.systemCPUTime,
    userCpuMicroseconds: usage.userCPUTime,
    voluntaryContextSwitches: usage.voluntaryContextSwitches,
  };
}

// close the disposable listener and process
function shutdown() {
  // keep shutdown idempotent
  if (shuttingDown) {
    return;
  }
  shuttingDown = true;
  const finish = () => {
    report({
      pid: process.pid,
      resourceSnapshot: resourceSnapshot(),
      type: "stopped",
    },
    // exit only after ipc delivery
    () => process.exit(0));
  };
  // finish immediately before listener creation
  if (server === null) {
    finish();
    return;
  }
  server.close(finish);
}

// start one source-owned Weather API listener
function start() {
  const store = {
    // return the reviewed ready health fixture
    async getHealth() {
      return {
        database: "ready",
        migration: { status: "current", version: "0001_initial_weather.sql" },
        workerLastLoopAt: "2026-08-22T04:45:00.000Z",
      };
    },
  };
  const handler = createWeatherApi(store, {
    // supply one explicit request clock
    now() {
      return new Date(REFERENCE_TIME);
    },
    version: "direct-weather-api-cold-fixture",
  });
  server = createWeatherApiServer(handler);
  server.listen(0, "127.0.0.1", () => {
    const address = server.address();
    // require one assigned loopback address
    if (address === null || typeof address === "string") {
      process.exitCode = 2;
      shutdown();
      return;
    }
    report({
      origin: `http://127.0.0.1:${String(address.port)}`,
      pid: process.pid,
      type: "ready",
    });
  });
}

// accept only the parent shutdown request
process.on("message", (message) => {
  // close on the exact control message
  if (message?.type === "shutdown") {
    shutdown();
  }
});

// close when the parent ipc channel disappears
process.on("disconnect", () => {
  shutdown();
});

// close on bounded termination
process.on("SIGTERM", () => {
  shutdown();
});

// reject standalone execution without ipc
if (typeof process.send !== "function") {
  process.stderr.write("direct-weather-api-child.ipc: parent ipc channel is required\n");
  process.exitCode = 2;
} else {
  start();
}
