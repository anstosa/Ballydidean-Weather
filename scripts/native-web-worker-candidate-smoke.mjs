#!/usr/bin/env node

import { access, mkdir, mkdtemp, readlink, rm } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const REPOSITORY_ROOT = resolve(dirname(SCRIPT_PATH), "..");
const PORT = 38_082;
const READY_DEADLINE_MS = 10_000;
const DRAIN_DEADLINE_MS = 10_000;
const WORKER_DEADLINE_MS = 5_000;
const OUTER_DEADLINE_MS = 30_000;
const MAX_HTTP_BYTES = 4 * 1024 * 1024;
const EXPECTED_WORKER_DIAGNOSTIC =
  "weather-worker: worker supports only the optional --once flag\n";
const SUCCESS = "native-web-worker-candidate-smoke: ok\n";

// pause between bounded listener probes
function delay(milliseconds) {
  // settle after the requested interval
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

// run one namespace preparation command
function requireCommand(command, arguments_) {
  const result = spawnSync(command, arguments_, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });

  // stop before candidate startup on isolation failure
  if (result.status !== 0) {
    throw new Error(`${command} failed: ${result.stderr.trim()}`);
  }
}

// exchange one static-shell request inside private networking
function exchangeForecastShell() {
  // retain one socket through its close-delimited response
  return new Promise((resolveExchange, rejectExchange) => {
    const chunks = [];
    let received = 0;
    const socket = createConnection({ host: "127.0.0.1", port: PORT });
    socket.setTimeout(1_000);
    // request only the source-owned static forecast shell
    socket.once("connect", () => {
      socket.write(
        "GET /forecast HTTP/1.1\r\n" +
          "Host: weather.invalid\r\n" +
          "Connection: close\r\n\r\n",
      );
    });
    // bound candidate response capture
    socket.on("data", (chunk) => {
      received += chunk.length;
      // reject an unexpected response before retaining it
      if (received > MAX_HTTP_BYTES) {
        socket.destroy(new Error("weather-web response exceeded the smoke bound"));
        return;
      }
      chunks.push(chunk);
    });
    // decode only the complete response
    socket.once("end", () => resolveExchange(Buffer.concat(chunks).toString("utf8")));
    // fail a stalled response
    socket.once("timeout", () => socket.destroy(new Error("weather-web response timed out")));
    // preserve the first socket failure
    socket.once("error", rejectExchange);
  });
}

// wait for the public listener or startup failure
async function awaitForecastShell(child, stderr) {
  const deadline = Date.now() + READY_DEADLINE_MS;
  let lastError = new Error("listener was not probed");

  // retry only loopback connection startup
  while (Date.now() < deadline) {
    // preserve candidate startup diagnostics
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error(`weather-web exited during startup: ${stderr.join("")}`);
    }

    try {
      return await exchangeForecastShell();
    } catch (error) {
      // retain the last connection-only failure
      lastError = error;
      await delay(50);
    }
  }

  throw new Error(`weather-web listener deadline expired: ${lastError.message}`);
}

// wait for one child without allowing shutdown to hang
function awaitExit(child, deadlineMilliseconds, label) {
  // preserve an exit observed before listener registration
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve({ code: child.exitCode, signal: child.signalCode });
  }
  // settle on process close or the fixed deadline
  return new Promise((resolveExit, rejectExit) => {
    // reject retained work after the deadline
    const timeout = setTimeout(
      () => rejectExit(new Error(`${label} deadline expired`)),
      deadlineMilliseconds,
    );
    // cancel the deadline after all process resources close
    child.once("close", (code, signal) => {
      clearTimeout(timeout);
      resolveExit({ code, signal });
    });
  });
}

// verify the genuine development forecast shell
function requireForecastShell(response) {
  const separator = response.indexOf("\r\n\r\n");

  // require one complete HTTP head
  if (separator < 0) {
    throw new Error(`weather-web returned an incomplete response: ${response}`);
  }
  const head = response.slice(0, separator).toLowerCase();
  const body = response.slice(separator + 4);

  // retain the source shell status and media type
  if (
    !head.startsWith("http/1.1 200") ||
    !head.includes("\r\ncontent-type: text/html; charset=utf-8") ||
    !body.includes("/assets/development/")
  ) {
    throw new Error(`weather-web returned an unexpected forecast shell: ${response}`);
  }
}

// prove invalid worker grammar before configuration or database startup
function requireWorkerArgumentRefusal(worker, repository) {
  const result = spawnSync("/usr/bin/env", ["-i", worker, "--watch"], {
    cwd: repository,
    encoding: "utf8",
    maxBuffer: 64 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
    timeout: WORKER_DEADLINE_MS,
  });

  // reject spawn and deadline failures separately
  if (result.error !== undefined) {
    throw result.error;
  }
  // require an ordinary process exit
  if (result.signal !== null) {
    throw new Error(`weather-worker argument check ended by ${result.signal}`);
  }
  // require the candidate's exact argument-only refusal
  if (
    result.status !== 1 ||
    result.stdout !== "" ||
    result.stderr !== EXPECTED_WORKER_DIAGNOSTIC
  ) {
    throw new Error(
      `weather-worker returned an unexpected argument refusal: status=${result.status} ` +
        `stdout=${JSON.stringify(result.stdout)} stderr=${JSON.stringify(result.stderr)}`,
    );
  }
}

// run both candidates after namespace isolation is proven
async function runInner(
  web,
  worker,
  fixture,
  repository,
  hostNetworkNamespace,
  hostUserNamespace,
  hostMountNamespace,
) {
  const networkNamespace = await readlink("/proc/self/ns/net");
  const userNamespace = await readlink("/proc/self/ns/user");
  const mountNamespace = await readlink("/proc/self/ns/mnt");

  // refuse any host network, user, or mount namespace
  if (
    networkNamespace === hostNetworkNamespace ||
    userNamespace === hostUserNamespace ||
    mountNamespace === hostMountNamespace
  ) {
    throw new Error("candidate smoke requires private network, user, and mount namespaces");
  }
  requireCommand("mount", ["--bind", join(fixture, "var-lib"), "/var/lib"]);
  requireCommand("mount", ["--bind", join(fixture, "run"), "/run"]);
  requireCommand("ip", ["link", "set", "lo", "up"]);

  const stderr = [];
  const child = spawn(
    "/usr/bin/env",
    [
      "-i",
      `PORT=${PORT}`,
      "NODE_ENV=development",
      "WEATHER_RELEASE=development",
      "WEATHER_WEB_REVISION=development",
      "WEATHER_API_ORIGIN=http://127.0.0.1:9",
      "WEATHER_ADJUSTMENT_EVIDENCE_ROOT=/var/lib/weather/xweather/adjustment-evidence",
      "WEATHER_ADMIN_AUTH_PATH=/var/lib/weather/xweather/admin-auth.json",
      "WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH=/run/secrets/weather_xweather_client_secret",
      "WEATHER_PROPERTY_SENSOR_LAYOUT_PATH=/var/lib/weather/xweather/property-sensor-layout.json",
      `WEATHER_SITE_CONFIG_PATH=${join(repository, "config/sites/ballydidean.json")}`,
      "WEATHER_XWEATHER_CLIENT_ID_FILE=/run/secrets/weather_xweather_client_id",
      "WEATHER_XWEATHER_CLIENT_SECRET_FILE=/run/secrets/weather_xweather_client_secret",
      web,
    ],
    { cwd: repository, stdio: ["ignore", "ignore", "pipe"] },
  );
  child.stderr.setEncoding("utf8");
  // retain bounded startup diagnostics for failures
  child.stderr.on("data", (chunk) => stderr.push(chunk));

  // serve and stop the public candidate before checking worker grammar
  try {
    const response = await awaitForecastShell(child, stderr);
    requireForecastShell(response);

    // use the production service-manager signal
    if (!child.kill("SIGTERM")) {
      throw new Error("weather-web rejected SIGTERM");
    }
    const exit = await awaitExit(child, DRAIN_DEADLINE_MS, "weather-web drain");

    // require coordinated shutdown rather than signal termination
    if (exit.code !== 0 || exit.signal !== null) {
      throw new Error(
        `weather-web did not drain cleanly: code=${exit.code} signal=${exit.signal} ${stderr.join("")}`,
      );
    }
  } finally {
    // contain every web failure inside the namespace
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGKILL");
      // ignore only cleanup after a retained failure
      await awaitExit(child, 2_000, "weather-web cleanup").catch(() => undefined);
    }
  }

  requireWorkerArgumentRefusal(worker, repository);
}

// create disposable fixed-path inputs before namespace entry
async function runOuter(webArgument, workerArgument) {
  const web = resolve(webArgument);
  const worker = resolve(workerArgument);
  await access(web, fsConstants.X_OK);
  await access(worker, fsConstants.X_OK);
  const fixture = await mkdtemp(join(tmpdir(), "weather-native-web-worker-smoke-"));
  let child;

  // retain all fixture and namespace cleanup in the outer owner
  try {
    await mkdir(join(fixture, "run", "secrets"), { recursive: true });
    await mkdir(join(fixture, "var-lib", "weather", "xweather", "adjustment-evidence"), {
      recursive: true,
    });
    const hostNetworkNamespace = await readlink("/proc/self/ns/net");
    const hostUserNamespace = await readlink("/proc/self/ns/user");
    const hostMountNamespace = await readlink("/proc/self/ns/mnt");
    child = spawn(
      "unshare",
      [
        "-Urnm",
        "--propagation",
        "private",
        "--",
        process.execPath,
        SCRIPT_PATH,
        "--inner",
        web,
        worker,
        fixture,
        REPOSITORY_ROOT,
        hostNetworkNamespace,
        hostUserNamespace,
        hostMountNamespace,
      ],
      { detached: true, stdio: ["ignore", "pipe", "pipe"] },
    );
    const stdout = [];
    const stderr = [];
    // retain isolated output for failure reporting
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    // retain isolated diagnostics for failure reporting
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    const exit = await awaitExit(child, OUTER_DEADLINE_MS, "isolated candidate smoke");

    // expose private diagnostics only on failure
    if (exit.code !== 0 || exit.signal !== null) {
      throw new Error(
        `isolated candidate smoke failed: code=${exit.code} signal=${exit.signal}\n` +
          Buffer.concat(stderr).toString("utf8") +
          Buffer.concat(stdout).toString("utf8"),
      );
    }
    process.stdout.write(SUCCESS);
  } finally {
    // terminate the complete private group after failure or deadline
    if (child !== undefined && child.exitCode === null && child.signalCode === null) {
      process.kill(-child.pid, "SIGKILL");
      // ignore only cleanup after the retained result
      await awaitExit(child, 2_000, "isolated cleanup").catch(() => undefined);
    }
    await rm(fixture, { force: true, recursive: true });
  }
}

// select the namespace worker without candidate fixture modes
if (process.argv[2] === "--inner") {
  // require all isolated ownership inputs
  if (process.argv.length !== 10) {
    throw new Error("private namespace smoke arguments are incomplete");
  }
  const [
    ,
    ,
    ,
    web,
    worker,
    fixture,
    repository,
    hostNetworkNamespace,
    hostUserNamespace,
    hostMountNamespace,
  ] = process.argv;
  await runInner(
    web,
    worker,
    fixture,
    repository,
    hostNetworkNamespace,
    hostUserNamespace,
    hostMountNamespace,
  );
} else {
  const web = process.argv[2];
  const worker = process.argv[3];

  // require both prebuilt candidate artifacts
  if (web === undefined || worker === undefined || process.argv.length !== 4) {
    throw new Error(
      `usage: ${process.argv[1]} /path/to/weather-web /path/to/weather-worker`,
    );
  }
  await runOuter(web, worker);
}
