import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { chmod, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
  evaluateBackendPerformanceDirect,
} from "./backend-performance-direct-evaluator.mjs";
import {
  BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
  BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
  buildBackendPerformanceFunctionalSchedule,
} from "./backend-performance-functional-schedule.mjs";

const REPOSITORY_ROOT = resolve(new URL("..", import.meta.url).pathname);
const WEB_SERVER_PATH = join(REPOSITORY_ROOT, "deploy/scripts/web-server.mjs");
const WORKLOAD_URL = new URL("../config/backend-performance-workload-v2.json", import.meta.url);
const WORKLOAD_SHA256 = "b44eb296f1145a55202e941e24e750c0b0d5a4c37075c6f8e9ca346470bca53f";
const ADMIN_PASSWORD = "direct-admin-fixture-password";
const BOOTSTRAP_TOKEN = "direct-admin-bootstrap-token-32-bytes";
const START_TIMEOUT_MS = 10_000;
const STOP_TIMEOUT_MS = 5_000;
const MAXIMUM_DIAGNOSTIC_BYTES = 64 * 1024;

// create one controlled admin workflow error
function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

// require one admin workflow invariant
function requireInvariant(condition, code, message) {
  // fail closed on source or workflow drift
  if (!condition) {
    fail(code, message);
  }
}

// load the reviewed login and authenticated page-read workflow
function loadSourceWorkflow() {
  const workloadBytes = readFileSync(WORKLOAD_URL);
  requireInvariant(
    createHash("sha256").update(workloadBytes).digest("hex") === WORKLOAD_SHA256,
    "direct-admin-read.workload",
    "backend performance workload identity changed",
  );
  const workload = JSON.parse(workloadBytes.toString("utf8"));
  const login = workload.operations.find(
    // select the reviewed administrator login
    (operation) => operation.id === "admin.live.login",
  );
  const readPage = workload.operations.find(
    // select the reviewed administrator page read
    (operation) => operation.id === "admin.live.read-page",
  );
  requireInvariant(
    login?.family === "admin-live-safe"
      && login.exactHttp?.method === "POST"
      && login.exactHttp?.target === "/admin/login"
      && readPage?.family === "admin-live-safe"
      && readPage.exactHttp?.method === "GET"
      && readPage.exactHttp?.target === "/admin"
      && readPage.stateEffects === "read-only",
    "direct-admin-read.operation",
    "reviewed administrator login or page-read operation changed",
  );
  const schedule = buildBackendPerformanceFunctionalSchedule();
  const attempts = [];
  const loginAttemptIds = [];

  // bind one login and page read from every reviewed run slot
  for (const runSlot of schedule.runSlots) {
    const slotAttempts = runSlot.groups.flatMap(
      // retain every planned group attempt
      (group) => group.attempts,
    );
    const loginAttempt = slotAttempts.find(
      // select the same-slot login transition
      (attempt) => attempt.operationId === login.id,
    );
    const readAttempt = slotAttempts.find(
      // select the same-slot authenticated page read
      (attempt) => attempt.operationId === readPage.id,
    );
    requireInvariant(
      loginAttempt !== undefined && loginAttempt.stratum === "once-per-run"
        && readAttempt !== undefined && readAttempt.stratum === "once-per-run",
      "direct-admin-read.schedule",
      `${runSlot.runSlotId} lacks one reviewed administrator login and page read`,
    );
    loginAttemptIds.push(loginAttempt.attemptId);
    attempts.push({
      attemptId: readAttempt.attemptId,
      body: null,
      expectedStatuses: [200],
      headers: {},
      method: "GET",
      operationId: readAttempt.operationId,
      path: "/admin",
      runSlotId: readAttempt.runSlotId,
      stratum: readAttempt.stratum,
      timeoutMs: readAttempt.deadlineMs,
    });
  }
  requireInvariant(
    attempts.length === 5 && loginAttemptIds.length === 5,
    "direct-admin-read.schedule",
    "administrator read fixture requires all five reviewed run slots",
  );
  return { attempts, loginAttemptIds };
}

// reserve one disposable listener port
async function reservePort() {
  const listener = createServer();
  listener.listen(0, "127.0.0.1");
  await new Promise((resolveListen, rejectListen) => {
    listener.once("error", rejectListen);
    listener.once("listening", resolveListen);
  });
  const address = listener.address();
  requireInvariant(
    address !== null && typeof address !== "string",
    "direct-admin-read.listener",
    "disposable listener did not receive one TCP address",
  );
  await new Promise((resolveClose, rejectClose) => {
    listener.close(
      // close only the temporary reservation
      (error) => error === undefined ? resolveClose() : rejectClose(error),
    );
  });
  return address.port;
}

// wait for the production web adapter to accept requests
async function waitForServer(origin, child) {
  const deadline = Date.now() + START_TIMEOUT_MS;

  // poll only the disposable child listener
  while (Date.now() < deadline) {
    // stop immediately when the child already failed
    if (child.exitCode !== null || child.signalCode !== null) {
      fail("direct-admin-read.start", "production web adapter exited before readiness");
    }
    try {
      const response = await fetch(origin, { signal: AbortSignal.timeout(500) });
      await response.body?.cancel();

      // accept one completed HTTP response as listener readiness
      if (response.status > 0) {
        return;
      }
    } catch {
      await new Promise(
        // bound one local retry delay
        (resolveWait) => setTimeout(resolveWait, 50),
      );
    }
  }
  fail("direct-admin-read.start", "production web adapter startup timed out");
}

// wait for one child process exit
function waitForExit(child, timeoutMs) {
  // retain an exit observed before listener attachment
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve({ code: child.exitCode, signal: child.signalCode });
  }
  return new Promise((resolveExit, rejectExit) => {
    const timer = setTimeout(
      // reject one expired child shutdown
      () => {
        cleanup();
        rejectExit(new Error("production web adapter shutdown timed out"));
      },
      timeoutMs,
    );
    const cleanup = () => {
      clearTimeout(timer);
      child.off("error", onError);
      child.off("exit", onExit);
    };
    const onError = (error) => {
      cleanup();
      rejectExit(error);
    };
    const onExit = (code, signal) => {
      cleanup();
      resolveExit({ code, signal });
    };
    child.once("error", onError);
    child.once("exit", onExit);
  });
}

// stop and reap one disposable production web adapter
async function stopChild(child) {
  // preserve an already reaped child
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  child.kill("SIGTERM");
  try {
    await waitForExit(child, STOP_TIMEOUT_MS);
  } catch (error) {
    child.kill("SIGKILL");
    await waitForExit(child, STOP_TIMEOUT_MS);
    throw error;
  }
}

// classify one authenticated production admin page
function classifyAdminPage({ attempt, bodyBytes, response }) {
  const body = bodyBytes.toString("utf8");
  const completed = attempt.operationId === "admin.live.read-page"
    && response.headers.get("content-type") === "text/html; charset=utf-8"
    && body.includes('data-weather-admin="true"');
  return {
    classification: completed
      ? "production-web-adapter-authenticated-admin-page-complete"
      : "production-web-adapter-admin-page-invalid",
    completed,
  };
}

// run the reviewed login plus authenticated admin page workflow
export async function runBackendPerformanceDirectAdminReadCheck() {
  const source = loadSourceWorkflow();
  const fixtureRoot = await mkdtemp(join(tmpdir(), "weather-direct-admin-read-"));
  await chmod(fixtureRoot, 0o700);
  const bootstrapTokenPath = join(fixtureRoot, "bootstrap-token");
  const authPath = join(fixtureRoot, "admin-auth.json");
  const evidenceRoot = join(fixtureRoot, "adjustment-evidence");
  const layoutPath = join(fixtureRoot, "property-sensor-layout.json");
  await writeFile(bootstrapTokenPath, `${BOOTSTRAP_TOKEN}\n`, { mode: 0o600 });
  const port = await reservePort();
  const origin = `http://127.0.0.1:${String(port)}`;
  const child = spawn(process.execPath, [WEB_SERVER_PATH], {
    cwd: REPOSITORY_ROOT,
    env: {
      ...process.env,
      NODE_ENV: "test",
      PORT: String(port),
      WEATHER_ADJUSTMENT_EVIDENCE_ROOT: evidenceRoot,
      WEATHER_ADMIN_AUTH_PATH: authPath,
      WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH: bootstrapTokenPath,
      WEATHER_API_ORIGIN: "http://127.0.0.1:9",
      WEATHER_PROPERTY_SENSOR_LAYOUT_PATH: layoutPath,
      WEATHER_RELEASE: "development",
      WEATHER_XWEATHER_CLIENT_ID_FILE: join(fixtureRoot, "absent-client-id"),
      WEATHER_XWEATHER_CLIENT_SECRET_FILE: join(fixtureRoot, "absent-client-secret"),
    },
    stdio: ["ignore", "ignore", "pipe"],
  });
  let diagnosticBytes = 0;
  const diagnostics = [];
  child.stderr.on(
    "data",
    // retain only bounded disposable startup diagnostics
    (chunk) => {
      diagnosticBytes += chunk.byteLength;

      // stop retaining after the local diagnostic ceiling
      if (diagnosticBytes <= MAXIMUM_DIAGNOSTIC_BYTES) {
        diagnostics.push(String(chunk));
      }
    },
  );
  let currentSessionCookie = null;
  const loginStatuses = [];
  const logoutStatuses = [];

  try {
    await waitForServer(origin, child);
    const bootstrap = await fetch(`${origin}/api/v1/admin/bootstrap`, {
      body: JSON.stringify({ password: ADMIN_PASSWORD }),
      headers: {
        "content-type": "application/json",
        "x-weather-admin-bootstrap": BOOTSTRAP_TOKEN,
      },
      method: "POST",
      redirect: "manual",
      signal: AbortSignal.timeout(5_000),
    });
    await bootstrap.arrayBuffer();
    requireInvariant(
      bootstrap.status === 201,
      "direct-admin-read.bootstrap",
      "disposable administrator bootstrap did not complete",
    );
    const result = await evaluateBackendPerformanceDirect({
      attempts: source.attempts,
      baseUrl: origin,
      contractVersion: BACKEND_PERFORMANCE_DIRECT_INPUT_VERSION,
      generation: "node-v1",
      maximumResponseBytes: 1024 * 1024,
      resourceInterval: null,
      role: "baseline",
      runId: "production-web-admin-read-fixture",
    }, {
      classifyResponse: classifyAdminPage,
      // revoke each disposable session after its page read
      async cleanupAttempt() {
        requireInvariant(
          currentSessionCookie !== null,
          "direct-admin-read.cleanup",
          "administrator session is unavailable during cleanup",
        );
        const logout = await fetch(`${origin}/admin/logout`, {
          headers: { cookie: currentSessionCookie },
          method: "POST",
          redirect: "manual",
          signal: AbortSignal.timeout(5_000),
        });
        await logout.arrayBuffer();
        logoutStatuses.push(logout.status);
        currentSessionCookie = null;
        requireInvariant(
          logout.status === 303,
          "direct-admin-read.cleanup",
          "disposable administrator logout did not complete",
        );
      },
      // resolve the transition cookie without exposing it to result data
      async fetch(url, init) {
        requireInvariant(
          currentSessionCookie !== null,
          "direct-admin-read.session",
          "administrator read started without its same-slot session",
        );
        return fetch(url, {
          ...init,
          headers: { ...init.headers, cookie: currentSessionCookie },
        });
      },
      // create one same-slot session through the production login handler
      async prepareAttempt() {
        const login = await fetch(`${origin}/admin/login`, {
          body: new URLSearchParams({ password: ADMIN_PASSWORD }),
          headers: {
            "content-type": "application/x-www-form-urlencoded",
            "x-forwarded-proto": "https",
          },
          method: "POST",
          redirect: "manual",
          signal: AbortSignal.timeout(5_000),
        });
        await login.arrayBuffer();
        const cookie = (login.headers.get("set-cookie") ?? "").split(";", 1)[0];
        requireInvariant(
          login.status === 303 && login.headers.get("location") === "/admin"
            && cookie.startsWith("weather_admin_session="),
          "direct-admin-read.login",
          "production administrator login did not create its same-slot session",
        );
        loginStatuses.push(login.status);
        currentSessionCookie = cookie;
        return {
          baseUrl: origin,
          classification: "production-web-adapter-same-slot-admin-login",
        };
      },
    });
    const operationStratum = result.summary.operationStrata.find(
      // select the exact measured administrator page stratum
      (value) => value.operationId === "admin.live.read-page"
        && value.stratum === "once-per-run",
    );
    requireInvariant(
      operationStratum !== undefined,
      "direct-admin-read.summary",
      "administrator page-read operation summary is missing",
    );
    return {
      attemptDenominator: result.summary.durationCoverage,
      coverage: {
        family: "admin-live-safe",
        kind: "single-reviewed-operation-causal-subset",
        operationId: "admin.live.read-page",
        stratum: "once-per-run",
      },
      evidenceClass: "synthetic-fixture",
      execution: {
        adapter: "deploy/scripts/web-server.mjs",
        bootstrapStatus: bootstrap.status,
        childExitObservedDuringExecution: child.exitCode !== null || child.signalCode !== null,
        loginStatuses,
        logoutStatuses,
      },
      latencyScope: "same-slot-production-login-plus-authenticated-page-read; cleanup logout excluded",
      nativeEligible: false,
      operationStratum,
      preparationClassifications: [...new Set(result.attempts.map(
        // retain one production login classification
        (attempt) => attempt.preparationClassification,
      ))],
      semanticClassifications: [...new Set(result.attempts.map(
        // retain one authenticated page classification
        (attempt) => attempt.semanticClassification,
      ))],
      sourceBinding: {
        functionalDefinitionSha256: BACKEND_PERFORMANCE_FUNCTIONAL_DEFINITION_SHA256,
        functionalScheduleVersion: BACKEND_PERFORMANCE_FUNCTIONAL_SCHEDULE_VERSION,
        loginAttemptIds: source.loginAttemptIds,
        readAttemptIds: source.attempts.map(
          // retain every reviewed read attempt identity
          (attempt) => attempt.attemptId,
        ),
        transitionResolution: "same-slot admin.live.login supplies admin.liveSessionCookie",
        workloadSha256: WORKLOAD_SHA256,
      },
      terminalCounts: result.summary.terminalCounts,
      unsupportedBoundaries: [
        "the production web adapter runs as one disposable local Node process rather than its native ARM64 image",
        "administrator credential persistence and sessions exist only inside the removed fixture root",
        "other admin-live-safe reads and mutations are not exercised by this causal subset",
      ],
    };
  } catch (error) {
    // attach bounded child diagnostics only to the thrown local failure
    if (error instanceof Error && diagnostics.length > 0) {
      error.cause = new Error(diagnostics.join("").slice(0, MAXIMUM_DIAGNOSTIC_BYTES));
    }
    throw error;
  } finally {
    await stopChild(child);
    await rm(fixtureRoot, { force: true, recursive: true });
  }
}

// print only on direct invocation
if (process.argv[1] !== undefined
  && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runBackendPerformanceDirectAdminReadCheck().then(
    // emit one machine-readable admin workflow result
    (result) => process.stdout.write(`${JSON.stringify(result, null, 2)}\n`),
    // emit one controlled admin workflow failure
    (error) => {
      process.stderr.write(`${error?.code ?? "direct-admin-read.error"}: ${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    },
  );
}
