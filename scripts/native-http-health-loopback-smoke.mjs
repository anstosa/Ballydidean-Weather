#!/usr/bin/env node

import { access, readlink } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const PROBE_DEADLINE_MS = 6_000;
const OUTER_DEADLINE_MS = 30_000;
const SUCCESS = "native-http-health-loopback-smoke: ok\n";

// run one namespace setup command without inheriting ambient input
function requireCommand(command, arguments_) {
  const result = spawnSync(command, arguments_, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });

  // stop before opening fixtures when loopback setup differs
  if (result.status !== 0) {
    throw new Error(`${command} failed: ${result.stderr.trim()}`);
  }
}

// wait for one child exit without allowing a probe to exceed compose's outer timeout
function awaitExit(child, deadlineMilliseconds) {
  // settle on the first process close or fixed deadline
  return new Promise((resolveExit, rejectExit) => {
    // reject work retained beyond its finite bound
    const timeout = setTimeout(
      () => rejectExit(new Error("native health probe deadline expired")),
      deadlineMilliseconds,
    );
    // cancel the deadline after all process resources close
    child.once("close", (code, signal) => {
      clearTimeout(timeout);
      resolveExit({ code, signal });
    });
  });
}

// start one fixed loopback fixture with mutable ordinary readiness status
function startFixture(port, expectedPath, state, observations) {
  // admit only the source request assigned to this fixture
  const server = createServer((request, response) => {
    observations.push({ method: request.method, path: request.url, port });

    // make any non-source request fail independently of the selected fixture status
    if (request.method !== "GET" || request.url !== expectedPath) {
      response.writeHead(500, { "content-type": "text/plain" });
      response.end("unexpected request");
      return;
    }
    response.writeHead(state.status, { "content-type": "text/plain" });
    response.end(state.status >= 200 && state.status < 300 ? "ready" : "not ready");
  });

  // bind only the isolated namespace loopback address
  return new Promise((resolveFixture, rejectFixture) => {
    server.once("error", rejectFixture);
    // expose the server only after its exact fixed port is bound
    server.listen(port, "127.0.0.1", () => resolveFixture(server));
  });
}

// close one bounded fixture after every native probe exits
function closeFixture(server) {
  // resolve only after the listener and retained sockets close
  return new Promise((resolveClose, rejectClose) => {
    // report fixture cleanup failures rather than leaking a namespace process
    server.close((error) => {
      // distinguish a genuine close failure from ordinary completion
      if (error !== undefined) {
        rejectClose(error);
      } else {
        resolveClose();
      }
    });
  });
}

// execute the actual built probe with an empty environment and assert its closed result
async function runProbe(binary, target, expectedCode, expectedStderr) {
  const arguments_ = Array.isArray(target) ? target : [target];
  const child = spawn("/usr/bin/env", ["-i", binary, ...arguments_], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  const stdout = [];
  const stderr = [];
  // retain unexpected output only for bounded failure diagnostics
  child.stdout.on("data", (chunk) => stdout.push(chunk));
  // retain the expected not-ready diagnostic without printing it from the smoke test
  child.stderr.on("data", (chunk) => stderr.push(chunk));

  try {
    const exit = await awaitExit(child, PROBE_DEADLINE_MS);
    const actualStdout = Buffer.concat(stdout).toString("utf8");
    const actualStderr = Buffer.concat(stderr).toString("utf8");

    // require exact exit and output behavior for ready and ordinary not-ready states
    if (
      exit.code !== expectedCode ||
      exit.signal !== null ||
      actualStdout !== "" ||
      actualStderr !== expectedStderr
    ) {
      throw new Error(
        `${target} probe differed: code=${exit.code} signal=${exit.signal} ` +
          `stdout=${JSON.stringify(actualStdout)} stderr=${JSON.stringify(actualStderr)}`,
      );
    }
  } finally {
    // contain every failure without leaving a health process in the namespace
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGKILL");
      // ignore only the cleanup wait after a prior retained failure
      await awaitExit(child, 2_000).catch(() => undefined);
    }
  }
}

// exercise both fixed targets entirely inside the private network namespace
async function runInner(binary, hostNetworkNamespace) {
  const networkNamespace = await readlink("/proc/self/ns/net");

  // refuse direct invocation that could open either fixed port on the host network
  if (networkNamespace === hostNetworkNamespace) {
    throw new Error("native health smoke requires a private network namespace");
  }
  requireCommand("ip", ["link", "set", "lo", "up"]);
  const observations = [];
  const apiState = { status: 204 };
  const webState = { status: 200 };
  const api = await startFixture(3_001, "/api/v1/health", apiState, observations);
  let web;

  try {
    web = await startFixture(3_000, "/", webState, observations);
    await runProbe(binary, "api", 0, "");
    await runProbe(binary, "web", 0, "");
    apiState.status = 503;
    webState.status = 503;
    await runProbe(binary, "api", 1, "weather-http-health: api returned HTTP 503\n");
    await runProbe(binary, "web", 1, "weather-http-health: web returned HTTP 503\n");
    const expected = [
      { method: "GET", path: "/api/v1/health", port: 3_001 },
      { method: "GET", path: "/", port: 3_000 },
      { method: "GET", path: "/api/v1/health", port: 3_001 },
      { method: "GET", path: "/", port: 3_000 },
    ];

    // require exactly one genuine GET per native invocation in source order
    if (JSON.stringify(observations) !== JSON.stringify(expected)) {
      throw new Error(`native health requests differed: ${JSON.stringify(observations)}`);
    }
  } finally {
    // close the later fixture before releasing the first fixed port
    if (web !== undefined) {
      await closeFixture(web);
    }
    await closeFixture(api);
  }
}

// exercise the real release-ready command through the same private fixed web port
async function runReleaseInner(binary, hostNetworkNamespace) {
  const networkNamespace = await readlink("/proc/self/ns/net");
  if (networkNamespace === hostNetworkNamespace) {
    throw new Error("native release health smoke requires a private network namespace");
  }
  requireCommand("ip", ["link", "set", "lo", "up"]);
  const release = "2026.11.25-1";
  const requests = [];
  const siteRoot = "/api/v1/sites/north%20field%2F%CE%B2";
  const responses = new Map([
    ["/", `<script src="/assets/${release}/app.js"></script>`],
    ["/api/v1/health", JSON.stringify({ data: { ready: true } })],
    ["/api/v1/sites", JSON.stringify({ data: [{ slug: "north field/β" }] })],
    [`${siteRoot}/current`, JSON.stringify({ data: [] })],
    [`${siteRoot}/forecast`, JSON.stringify({ data: [{ time: "fixture" }] })],
  ]);
  const server = createServer((request, response) => {
    requests.push(request.url);
    const body = responses.get(request.url);
    if (request.method !== "GET" || body === undefined) {
      response.writeHead(404).end("unexpected request");
      return;
    }
    response.writeHead(200).end(body);
  });
  // expose only the private namespace listener after bind completes
  await new Promise((resolveFixture, rejectFixture) => {
    server.once("error", rejectFixture);
    server.listen(3_000, "127.0.0.1", resolveFixture);
  });
  try {
    await runProbe(binary, ["web-release", release], 0, "");
    responses.set("/api/v1/health", JSON.stringify({ data: { ready: false } }));
    await runProbe(binary, ["web-release", release], 1,
      "weather-http-health: web release probe failed: web API health is not ready\n");
    // require full success reads and early ordinary not-ready exit
    const expected = [...responses.keys(), "/", "/api/v1/health"];
    if (JSON.stringify(requests) !== JSON.stringify(expected)) {
      throw new Error(`native release health reads differed: ${JSON.stringify(requests)}`);
    }
  } finally {
    await closeFixture(server);
  }
}

// launch the inner owner only after unshare creates the private network namespace
async function runOuter(binaryArgument, releaseOnly = false) {
  const binary = resolve(binaryArgument);
  await access(binary, fsConstants.X_OK);
  const hostNetworkNamespace = await readlink("/proc/self/ns/net");
  const child = spawn(
    "unshare",
    [
      "-Urn",
      "--",
      process.execPath,
      SCRIPT_PATH,
      releaseOnly ? "--release-inner" : "--inner",
      binary,
      hostNetworkNamespace,
    ],
    { detached: true, stdio: ["ignore", "pipe", "pipe"] },
  );
  const stdout = [];
  const stderr = [];
  // retain only inner output needed if namespace execution fails
  child.stdout.on("data", (chunk) => stdout.push(chunk));
  // retain only inner diagnostics needed if namespace execution fails
  child.stderr.on("data", (chunk) => stderr.push(chunk));

  try {
    const exit = await awaitExit(child, OUTER_DEADLINE_MS);

    // surface the bounded private-namespace diagnostic on failure
    if (exit.code !== 0 || exit.signal !== null) {
      throw new Error(
        `isolated native health smoke failed: code=${exit.code} signal=${exit.signal}\n` +
          Buffer.concat(stderr).toString("utf8") +
          Buffer.concat(stdout).toString("utf8"),
      );
    }
    process.stdout.write(SUCCESS);
  } finally {
    // kill the complete private namespace group only after a failed or stalled run
    if (child.exitCode === null && child.signalCode === null) {
      process.kill(-child.pid, "SIGKILL");
      // ignore only the cleanup wait after the failure is already retained
      await awaitExit(child, 2_000).catch(() => undefined);
    }
  }
}

// select the private namespace worker without adding a test mode to the health executable
if (process.argv[2] === "--inner" || process.argv[2] === "--release-inner") {
  // require the outer owner to supply the original network namespace identity
  if (process.argv.length !== 5) {
    throw new Error("private network namespace smoke arguments are incomplete");
  }
  const [, , operation, binary, hostNetworkNamespace] = process.argv;
  if (operation === "--release-inner") {
    await runReleaseInner(binary, hostNetworkNamespace);
  } else {
    await runInner(binary, hostNetworkNamespace);
  }
} else {
  const binary = process.argv[2];
  const releaseOnly = process.argv[3] === "--release-only";

  // require the exact built artifact instead of compiling inside the smoke test
  if (binary === undefined || (process.argv.length !== 3 && !(releaseOnly && process.argv.length === 4))) {
    throw new Error(`usage: ${process.argv[1]} /absolute/or/relative/path/to/weather-http-health [--release-only]`);
  }
  await runOuter(binary, releaseOnly);
}
