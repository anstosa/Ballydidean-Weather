import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import vm from "node:vm";

import { XweatherTileMemoryCache } from "../../../../deploy/scripts/xweather-tile-cache.mjs";
import { XweatherUsageBudget } from "../../../../deploy/scripts/xweather-usage-budget.mjs";

const repositoryRoot = new URL("../../../../", import.meta.url);
const webSource = await readFile(
  new URL("deploy/scripts/web-server.mjs", repositoryRoot),
  "utf8",
);

// extract one complete source function
function sourceFunction(source, signature) {
  const start = source.indexOf(signature);
  assert.notEqual(start, -1);
  const opening = source.indexOf("{", start);
  let depth = 0;
  let quote = null;
  let escaped = false;

  // scan strings while balancing every executable block
  for (let index = opening; index < source.length; index += 1) {
    const character = source[index];

    // retain escaped string characters without balancing braces
    if (quote !== null) {
      // advance the active string state
      if (escaped) {
        escaped = false;
      } else if (character === "\\") {
        escaped = true;
      } else if (character === quote) {
        quote = null;
      }
      continue;
    }

    // enter one ordinary source string
    if (character === "\"" || character === "'" || character === "`") {
      quote = character;
      continue;
    }

    // balance the executable block
    if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;

      // return the first complete function block
      if (depth === 0) {
        return source.slice(start, index + 1);
      }
    }
  }

  throw new Error(`unterminated source function: ${signature}`);
}

// render one redacted usage snapshot
function snapshotSignature(snapshot) {
  return `${snapshot.day}/${snapshot.dayUnits}/${snapshot.month}/${snapshot.monthUnits}/${snapshot.dailyLimit}/${snapshot.monthlyLimit}`;
}

// capture one ordinary source failure object
async function captureError(operation) {
  try {
    await operation();
  } catch (error) {
    return error;
  }
  throw new Error("expected fixture failure");
}

// render one source failure and optional code
function errorSignature(error) {
  return `${error.message}:${error.code ?? "-"}`;
}

// build one parsed static-frame fixture
function frame(layer, width = 256) {
  return {
    height: 256,
    kind: "frame",
    latitude: 47.95,
    layer,
    longitude: -122.43,
    phase: "forecast",
    validTime: "20260828120000",
    width,
    zoom: 10,
  };
}

const root = await mkdtemp(join(tmpdir(), "weather-edge-xweather-budget-node-"));
let now = Date.parse("2026-08-28T12:00:00Z");

try {
  const persistedPath = join(root, "persisted", "usage.json");
  const createPersisted = () => new XweatherUsageBudget({
    dailyLimit: 3,
    monthlyLimit: 5,
    now: () => now,
    path: persistedPath,
  });
  const persisted = createPersisted();
  persisted.reserve(2);
  const persistedInitial = persisted.snapshot();
  const recreated = createPersisted();
  const persistedRecreated = recreated.snapshot();
  now = Date.parse("2026-08-29T00:00:00Z");
  const dailyReset = recreated.snapshot();
  recreated.reserve(1);
  const dailyReserved = recreated.snapshot();
  now = Date.parse("2026-09-01T00:00:00Z");
  const monthlyReset = recreated.snapshot();
  recreated.reserve(1);
  const monthlyReserved = recreated.snapshot();

  now = Date.parse("2026-08-28T12:00:00Z");
  const exhausted = new XweatherUsageBudget({
    dailyLimit: 2,
    monthlyLimit: 3,
    now: () => now,
  });
  exhausted.reserve(2);
  const dailyExhausted = await captureError(() => exhausted.reserve(1));
  now = Date.parse("2026-08-29T00:00:00Z");
  exhausted.reserve(1);
  const monthlyExhausted = await captureError(() => exhausted.reserve(1));
  const exhaustedSnapshot = exhausted.snapshot();

  const blockedParent = join(root, "blocked-reserve");
  const blockedPath = join(blockedParent, "usage.json");
  const blocked = new XweatherUsageBudget({
    dailyLimit: 3,
    monthlyLimit: 5,
    now: () => now,
    path: blockedPath,
  });
  await writeFile(blockedParent, "not a directory");
  const reservePersistence = await captureError(() => blocked.reserve(1));
  const afterReservePersistence = blocked.snapshot();

  const rollDirectory = join(root, "blocked-roll");
  const rollPath = join(rollDirectory, "usage.json");
  const rolling = new XweatherUsageBudget({
    dailyLimit: 3,
    monthlyLimit: 5,
    now: () => now,
    path: rollPath,
  });
  rolling.reserve(1);
  await rm(rollDirectory, { force: true, recursive: true });
  await writeFile(rollDirectory, "not a directory");
  now = Date.parse("2026-08-30T00:00:00Z");
  const rollPersistence = await captureError(() => rolling.snapshot());
  now = Date.parse("2026-08-29T00:00:00Z");
  const afterRollPersistence = rolling.snapshot();

  now = 1_000;
  let providerCalls = 0;
  let releaseCoalesced;
  const coalescedGate = new Promise((resolve) => {
    releaseCoalesced = resolve;
  });
  const providerOutcomes = [Buffer.from("aa"), new Error("provider failed"), Buffer.from("bb")];
  const cacheBudget = new XweatherUsageBudget({
    dailyLimit: 10,
    monthlyLimit: 10,
    now: () => Date.parse("2026-08-28T12:00:00Z"),
  });
  const cache = new XweatherTileMemoryCache({
    forecastFreshnessMs: 100,
    maximumBytes: 32,
    now: () => now,
    // reserve immediately before each actual provider attempt
    async loadTile(tile) {
      const units = Math.ceil(tile.width / 256) * Math.ceil(tile.height / 256);
      cacheBudget.reserve(units);
      providerCalls += 1;

      // retain one blocked provider request for pending coalescing
      if (tile.layer === "coalesced") {
        await coalescedGate;
        return { body: Buffer.from("cc") };
      }

      const outcome = providerOutcomes.shift();

      // preserve the injected retryable provider failure
      if (outcome instanceof Error) {
        throw outcome;
      }

      return { body: outcome };
    },
  });
  const cachedFrame = frame("cached");
  const cacheMiss = await cache.get(cachedFrame);
  const cacheHit = await cache.get(cachedFrame);
  const afterHit = cacheBudget.snapshot();
  now = 1_100;
  const providerFailure = await captureError(() => cache.get(cachedFrame));
  const afterFailure = cacheBudget.snapshot();
  const cacheRetry = await cache.get(cachedFrame);
  const afterRetry = cacheBudget.snapshot();
  const coalescedFrame = frame("coalesced", 512);
  const firstPending = cache.get(coalescedFrame);
  const secondPending = cache.get(coalescedFrame);
  const pendingSnapshot = cacheBudget.snapshot();
  releaseCoalesced();
  const [coalescedFirst, coalescedSecond] = await Promise.all([firstPending, secondPending]);
  const afterPending = cacheBudget.snapshot();

  let exhaustedProviderCalls = 0;
  const cacheExhaustionBudget = new XweatherUsageBudget({
    dailyLimit: 2,
    monthlyLimit: 5,
    now: () => Date.parse("2026-08-28T12:00:00Z"),
  });
  const exhaustedCache = new XweatherTileMemoryCache({
    forecastFreshnessMs: 100,
    maximumBytes: 32,
    now: () => now,
    // reserve before exposing one provider call
    async loadTile(tile) {
      const units = Math.ceil(tile.width / 256) * Math.ceil(tile.height / 256);
      cacheExhaustionBudget.reserve(units);
      exhaustedProviderCalls += 1;
      return { body: Buffer.from("ok") };
    },
  });
  await exhaustedCache.get(frame("full", 512));
  const exhaustedError = await captureError(() => exhaustedCache.get(frame("rejected")));
  const exhaustedRetry = await captureError(() => exhaustedCache.get(frame("rejected")));

  const context = vm.createContext({
    Date: class extends Date {
      // fix only the route retry projection clock
      constructor(value) {
        super(value === undefined ? "2026-08-28T12:00:00Z" : value);
      }
    },
    Math,
    String,
    exhaustedError,
    providerFailure,
  });
  new vm.Script(`
const xweatherCredentials = {};
let activeError = exhaustedError;
const xweatherTileCache = { async get() { throw activeError; } };
function parseXweatherMapImage() { return { phase: "forecast" }; }
function sendText(response, status, body, headers = {}) {
  response.result = { body, headers, status };
}
function setSecurityHeaders() {}
${sourceFunction(webSource, "function secondsUntilUtcDay()")}
${sourceFunction(webSource, "async function proxyXweatherMap(request, response, requestUrl)")}
this.runProxy = async () => {
  const response = {};
  await proxyXweatherMap({ method: "GET" }, response, { pathname: "/fixture" });
  return response.result;
};
this.selectProviderFailure = () => { activeError = providerFailure; };
`).runInContext(context);
  const exhaustionResponse = await context.runProxy();
  context.selectProviderFailure();
  const fallbackResponse = await context.runProxy();

  process.stdout.write(`persisted=${snapshotSignature(persistedInitial)}:${snapshotSignature(persistedRecreated)}\n`);
  process.stdout.write(`windows=${snapshotSignature(dailyReset)}:${snapshotSignature(dailyReserved)}:${snapshotSignature(monthlyReset)}:${snapshotSignature(monthlyReserved)}\n`);
  process.stdout.write(`ceilings=${errorSignature(dailyExhausted)}:${errorSignature(monthlyExhausted)}:${snapshotSignature(exhaustedSnapshot)}\n`);
  process.stdout.write(`reserve-persistence=${errorSignature(reservePersistence)}:${snapshotSignature(afterReservePersistence)}\n`);
  process.stdout.write(`roll-persistence=${errorSignature(rollPersistence)}:${snapshotSignature(afterRollPersistence)}\n`);
  process.stdout.write(`cache=${cacheMiss.cacheStatus}/${cacheHit.cacheStatus}:${snapshotSignature(afterHit)}:${errorSignature(providerFailure)}:${snapshotSignature(afterFailure)}:${cacheRetry.cacheStatus}:${snapshotSignature(afterRetry)}\n`);
  process.stdout.write(`pending=${pendingSnapshot.dayUnits}:${coalescedFirst.cacheStatus}/${coalescedSecond.cacheStatus}:${afterPending.dayUnits}:calls=${providerCalls}\n`);
  process.stdout.write(`exhaustion=${errorSignature(exhaustedError)}:${errorSignature(exhaustedRetry)}:calls=${exhaustedProviderCalls}:${snapshotSignature(cacheExhaustionBudget.snapshot())}\n`);
  process.stdout.write(`decision=${exhaustionResponse.status}:${exhaustionResponse.body.trimEnd()}:${Object.entries(exhaustionResponse.headers)[0].join("=")}\n`);
  process.stdout.write(`fallback=${fallbackResponse.status}:${fallbackResponse.body.trimEnd()}:${Object.keys(fallbackResponse.headers).length === 0 ? "-" : "unexpected"}\n`);
} finally {
  await rm(root, { force: true, recursive: true });
}
