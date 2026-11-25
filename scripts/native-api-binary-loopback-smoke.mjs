#!/usr/bin/env node

import { access, mkdir, mkdtemp, readlink, rm, writeFile } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const PORT = 38_081;
const READY_DEADLINE_MS = 10_000;
const DRAIN_DEADLINE_MS = 10_000;
const EXPECTED_BODY = '{"error":{"code":"not_found","message":"Endpoint not found"}}';
const SUCCESS = "native-api-binary-loopback-smoke: ok\n";

// pause one bounded interval between listener probes
function delay(milliseconds) {
  // resolve only after the requested probe delay
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

// run one namespace setup command without inheriting ambient input
function requireCommand(command, arguments_) {
  const result = spawnSync(command, arguments_, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });

  // stop before starting the binary when namespace preparation differs
  if (result.status !== 0) {
    throw new Error(`${command} failed: ${result.stderr.trim()}`);
  }
}

// exchange one close-delimited HTTP request entirely inside the private network namespace
function exchange(port) {
  // retain one socket until its complete response closes
  return new Promise((resolveExchange, rejectExchange) => {
    const chunks = [];
    const socket = createConnection({ host: "127.0.0.1", port });
    socket.setTimeout(1_000);
    // keep the request half open until Hyper sends the close-delimited response
    socket.once("connect", () => {
      socket.write(
        "GET /api/v1/does-not-exist HTTP/1.1\r\n" +
          "Host: 127.0.0.1\r\n" +
          "Connection: close\r\n\r\n",
      );
    });
    // retain response bytes in arrival order
    socket.on("data", (chunk) => chunks.push(chunk));
    // decode only the complete close-delimited response
    socket.once("end", () => resolveExchange(Buffer.concat(chunks).toString("utf8")));
    // fail a stalled admitted connection
    socket.once("timeout", () => socket.destroy(new Error("HTTP response timed out")));
    socket.once("error", rejectExchange);
  });
}

// wait for the listener while failing immediately when startup exits
async function awaitResponse(child, stderr) {
  const deadline = Date.now() + READY_DEADLINE_MS;
  let lastError = new Error("listener was not probed");

  // retry only connection startup until the fixed deadline
  while (Date.now() < deadline) {
    // preserve early process failure rather than reporting a socket timeout
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error(`weather-api exited during startup: ${stderr.join("")}`);
    }

    try {
      return await exchange(PORT);
    } catch (error) {
      lastError = error;
      await delay(50);
    }
  }

  throw new Error(`weather-api listener deadline expired: ${lastError.message}`);
}

// wait for one child exit without allowing shutdown to hang the smoke test
function awaitExit(child, deadlineMilliseconds) {
  // settle on the first clean close or deadline
  return new Promise((resolveExit, rejectExit) => {
    // reject a process that retained work beyond its bound
    const timeout = setTimeout(
      () => rejectExit(new Error("weather-api drain deadline expired")),
      deadlineMilliseconds,
    );
    // cancel the deadline after process resources close
    child.once("close", (code, signal) => {
      clearTimeout(timeout);
      resolveExit({ code, signal });
    });
  });
}

// verify the fixed source-owned not-found representation
function requireNotFound(response) {
  const separator = response.indexOf("\r\n\r\n");

  // require one complete HTTP head before inspecting source bytes
  if (separator < 0) {
    throw new Error(`weather-api returned an incomplete response: ${response}`);
  }
  const head = response.slice(0, separator).toLowerCase();
  const body = response.slice(separator + 4);

  // retain the exact public source response and its fixed wire metadata
  if (
    !head.startsWith("http/1.1 404 not found\r\n") ||
    !head.includes("\r\ncontent-type: application/json; charset=utf-8") ||
    !head.includes("\r\ncache-control: no-store") ||
    !head.includes(`\r\ncontent-length: ${Buffer.byteLength(EXPECTED_BODY)}`) ||
    body !== EXPECTED_BODY
  ) {
    throw new Error(`weather-api returned an unexpected not-found response: ${response}`);
  }
}

// run the real binary after all host-visible paths and interfaces are isolated
async function runInner(binary, fixture, hostNetworkNamespace, hostMountNamespace) {
  const networkNamespace = await readlink("/proc/self/ns/net");
  const mountNamespace = await readlink("/proc/self/ns/mnt");

  // refuse direct invocation that could bind or mount in the host namespaces
  if (networkNamespace === hostNetworkNamespace || mountNamespace === hostMountNamespace) {
    throw new Error("weather-api smoke requires private network and mount namespaces");
  }
  requireCommand("mount", ["--bind", join(fixture, "opt"), "/opt"]);
  requireCommand("ip", ["link", "set", "lo", "up"]);
  const stderr = [];
  const child = spawn(
    "/usr/bin/env",
    [
      "-i",
      `WEATHER_API_PORT=${PORT}`,
      "WEATHER_DATABASE_HOST=127.0.0.1",
      "WEATHER_DATABASE_NAME=weather",
      `WEATHER_DATABASE_PASSWORD_FILE=${join(fixture, "database-password")}`,
      "WEATHER_DATABASE_SSL=false",
      "WEATHER_DATABASE_USER=weather",
      "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=1",
      "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=1",
      `WEATHER_MIGRATION_DIRECTORY=${join(fixture, "migrations")}`,
      "WEATHER_RELEASE=native-api-binary-loopback-smoke",
      binary,
    ],
    { stdio: ["ignore", "ignore", "pipe"] },
  );
  child.stderr.setEncoding("utf8");
  // retain value-free startup diagnostics for bounded failures
  child.stderr.on("data", (chunk) => stderr.push(chunk));

  try {
    const response = await awaitResponse(child, stderr);
    requireNotFound(response);

    // deliver the same service-manager signal used by production shutdown
    if (!child.kill("SIGTERM")) {
      throw new Error("weather-api rejected SIGTERM");
    }
    const exit = await awaitExit(child, DRAIN_DEADLINE_MS);

    // require an orderly drain rather than signal-default termination
    if (exit.code !== 0 || exit.signal !== null) {
      throw new Error(
        `weather-api did not drain cleanly: code=${exit.code} signal=${exit.signal} ${stderr.join("")}`,
      );
    }
  } finally {
    // contain every failure without leaving the isolated binary running
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGKILL");
      // ignore only the cleanup wait after a prior retained failure
      await awaitExit(child, 2_000).catch(() => undefined);
    }
  }
}

// create disposable installed inputs before entering the private mount namespace
async function runOuter(binaryArgument) {
  const binary = resolve(binaryArgument);
  await access(binary, fsConstants.X_OK);
  const fixture = await mkdtemp(join(tmpdir(), "weather-native-api-binary-smoke-"));
  let child;

  try {
    await mkdir(join(fixture, "opt", "weather", "config", "forecast-adjustments"), {
      recursive: true,
    });
    await mkdir(join(fixture, "migrations"));
    await writeFile(join(fixture, "database-password"), "fixture-password\n", { mode: 0o600 });
    const hostNetworkNamespace = await readlink("/proc/self/ns/net");
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
        binary,
        fixture,
        hostNetworkNamespace,
        hostMountNamespace,
      ],
      { detached: true, stdio: ["ignore", "pipe", "pipe"] },
    );
    const stdout = [];
    const stderr = [];
    // retain only the isolated worker output for failure reporting
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    // retain only the isolated worker diagnostic for failure reporting
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    const exit = await awaitExit(child, READY_DEADLINE_MS + DRAIN_DEADLINE_MS + 5_000);

    // expose the isolated diagnostic only when the smoke owner failed
    if (exit.code !== 0 || exit.signal !== null) {
      throw new Error(
        `isolated weather-api smoke failed: code=${exit.code} signal=${exit.signal}\n` +
          Buffer.concat(stderr).toString("utf8") +
          Buffer.concat(stdout).toString("utf8"),
      );
    }
    process.stdout.write(SUCCESS);
  } finally {
    // kill the complete private namespace group only after a failed or stalled run
    if (child !== undefined && child.exitCode === null && child.signalCode === null) {
      process.kill(-child.pid, "SIGKILL");
      // ignore only the cleanup wait after the failure is already retained
      await awaitExit(child, 2_000).catch(() => undefined);
    }
    await rm(fixture, { force: true, recursive: true });
  }
}

// select the private namespace worker without adding a runtime fixture mode to weather-api
if (process.argv[2] === "--inner") {
  // require the outer owner to supply both original namespace identities
  if (process.argv.length !== 7) {
    throw new Error("private namespace smoke arguments are incomplete");
  }
  const [, , , binary, fixture, hostNetworkNamespace, hostMountNamespace] = process.argv;
  await runInner(binary, fixture, hostNetworkNamespace, hostMountNamespace);
} else {
  const binary = process.argv[2];

  // require the exact built artifact instead of compiling inside the smoke test
  if (binary === undefined || process.argv.length !== 3) {
    throw new Error(`usage: ${process.argv[1]} /absolute/or/relative/path/to/weather-api`);
  }
  await runOuter(binary);
}
