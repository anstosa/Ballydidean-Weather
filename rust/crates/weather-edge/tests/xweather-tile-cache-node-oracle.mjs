import { XweatherTileMemoryCache } from "../../../../deploy/scripts/xweather-tile-cache.mjs";

// build one parsed tile fixture without exercising URL behavior
function tile(layer, phase = "forecast") {
  return {
    column: 1,
    kind: "tile",
    layer,
    phase,
    row: 2,
    validTime: "2026-11-23T12:00:00Z",
    zoom: 7,
  };
}

// retain deterministic provider outcomes and call counts
function sequenceLoader(sequences) {
  const calls = new Map();
  return {
    calls,
    // load the next typed fixture outcome for one parsed tile
    load(tileRequest) {
      calls.set(tileRequest.layer, (calls.get(tileRequest.layer) ?? 0) + 1);
      const next = sequences.get(tileRequest.layer)?.shift();

      // preserve ordinary injected provider failures
      if (next instanceof Error) {
        throw next;
      }

      // retain deferred provider results for coalescing coverage
      if (next instanceof Promise) {
        return next;
      }

      return { body: next };
    },
  };
}

// render one returned cache entry without another wire codec
function responseSignature(response) {
  return `${response.cacheStatus}/${response.fetchedAt}/${response.body.toString("utf8")}`;
}

// render bounded counters in source field order
function statsSignature(cache) {
  const stats = cache.stats();
  return `${stats.entries}/${stats.inflight}/${stats.maximumBytes}/${stats.storedBytes}`;
}

// capture one exact ordinary failure message
async function failureMessage(operation) {
  try {
    await operation();
  } catch (error) {
    return error.message;
  }
  throw new Error("expected fixture failure");
}

let now = 1_000;
const basicLoader = sequenceLoader(new Map([
  ["forecast-basic", [Buffer.from("aa"), Buffer.from("bbb")]],
  ["history-basic", [Buffer.from("hh")]],
]));
const basic = new XweatherTileMemoryCache({
  forecastFreshnessMs: 100,
  loadTile: basicLoader.load,
  maximumBytes: 16,
  now: () => now,
});
const forecastMiss = await basic.get(tile("forecast-basic"));
now = 1_099;
const forecastHit = await basic.get(tile("forecast-basic"));
now = 1_100;
const forecastRefresh = await basic.get(tile("forecast-basic"));
const historyMiss = await basic.get(tile("history-basic", "history"));
now = 99_999;
const historyHit = await basic.get(tile("history-basic", "history"));

const priorityLoader = sequenceLoader(new Map([
  ["history-1", [Buffer.from("h1")]],
  ["forecast-1", [Buffer.from("f1"), Buffer.from("F1"), Buffer.from("R1")]],
  ["history-2", [Buffer.from("h2")]],
]));
const priority = new XweatherTileMemoryCache({
  forecastFreshnessMs: 100,
  loadTile: priorityLoader.load,
  maximumBytes: 4,
  now: () => now,
});
const priorityHistoryOne = await priority.get(tile("history-1", "history"));
const priorityForecastOne = await priority.get(tile("forecast-1"));
const priorityHistoryTwo = await priority.get(tile("history-2", "history"));
const priorityAfterHistory = statsSignature(priority);
const priorityHistoryHit = await priority.get(tile("history-1", "history"));
const priorityForecastMiss = await priority.get(tile("forecast-1"));
const priorityForecastRepeated = await priority.get(tile("forecast-1"));

const oversizeLoader = sequenceLoader(new Map([
  ["oversize", [Buffer.from("wide"), Buffer.from("WIDE")]],
]));
const oversize = new XweatherTileMemoryCache({
  forecastFreshnessMs: 100,
  loadTile: oversizeLoader.load,
  maximumBytes: 3,
  now: () => now,
});
const oversizeFirst = await oversize.get(tile("oversize"));
const oversizeSecond = await oversize.get(tile("oversize"));

now = 10_000;
const refreshLoader = sequenceLoader(new Map([
  ["refresh", [Buffer.from("ok"), Buffer.from("wide")]],
]));
const refresh = new XweatherTileMemoryCache({
  forecastFreshnessMs: 10,
  loadTile: refreshLoader.load,
  maximumBytes: 3,
  now: () => now,
});
const refreshFirst = await refresh.get(tile("refresh"));
now = 10_010;
const refreshOversize = await refresh.get(tile("refresh"));

const invalidLoader = sequenceLoader(new Map([
  ["invalid", [Buffer.alloc(0), Buffer.from("ok")]],
]));
const invalid = new XweatherTileMemoryCache({
  forecastFreshnessMs: 100,
  loadTile: invalidLoader.load,
  maximumBytes: 4,
  now: () => now,
});
const invalidError = await failureMessage(() => invalid.get(tile("invalid")));
const invalidRetry = await invalid.get(tile("invalid"));

const failureLoader = sequenceLoader(new Map([
  ["failure", [new Error("provider failed"), Buffer.from("ok")]],
]));
const failure = new XweatherTileMemoryCache({
  forecastFreshnessMs: 100,
  loadTile: failureLoader.load,
  maximumBytes: 4,
  now: () => now,
});
const providerError = await failureMessage(() => failure.get(tile("failure")));
const providerRetry = await failure.get(tile("failure"));

let releaseCoalesced;
const coalescedBody = new Promise((resolve) => {
  releaseCoalesced = resolve;
});
const coalescedLoader = sequenceLoader(new Map([
  ["coalesced", [coalescedBody]],
]));
const coalesced = new XweatherTileMemoryCache({
  forecastFreshnessMs: 100,
  loadTile: coalescedLoader.load,
  maximumBytes: 4,
  now: () => now,
});
const coalescedFirstPending = coalesced.get(tile("coalesced"));
const coalescedSecondPending = coalesced.get(tile("coalesced"));
const coalescedPendingStats = statsSignature(coalesced);
releaseCoalesced({ body: Buffer.from("cc") });
const [coalescedFirst, coalescedSecond] = await Promise.all([
  coalescedFirstPending,
  coalescedSecondPending,
]);

const invalidConfiguration = await failureMessage(
  () => Promise.resolve(new XweatherTileMemoryCache({
    forecastFreshnessMs: 100,
    loadTile: basicLoader.load,
    maximumBytes: 0,
    now: () => now,
  })),
);

process.stdout.write(`forecast=${responseSignature(forecastMiss)},${responseSignature(forecastHit)},${responseSignature(forecastRefresh)}:calls=${basicLoader.calls.get("forecast-basic")}:${statsSignature(basic)}\n`);
process.stdout.write(`history=${responseSignature(historyMiss)},${responseSignature(historyHit)}:calls=${basicLoader.calls.get("history-basic")}\n`);
process.stdout.write(`priority=${responseSignature(priorityHistoryOne)},${responseSignature(priorityForecastOne)},${responseSignature(priorityHistoryTwo)}:${priorityAfterHistory}:${responseSignature(priorityHistoryHit)},${responseSignature(priorityForecastMiss)},${responseSignature(priorityForecastRepeated)}:calls=${priorityLoader.calls.get("forecast-1")}:${statsSignature(priority)}\n`);
process.stdout.write(`oversize=${responseSignature(oversizeFirst)},${responseSignature(oversizeSecond)}:calls=${oversizeLoader.calls.get("oversize")}:${statsSignature(oversize)}\n`);
process.stdout.write(`refresh-oversize=${responseSignature(refreshFirst)},${responseSignature(refreshOversize)}:${statsSignature(refresh)}\n`);
process.stdout.write(`invalid=${invalidError}:${responseSignature(invalidRetry)}:${statsSignature(invalid)}\n`);
process.stdout.write(`failure=${providerError}:${responseSignature(providerRetry)}:${statsSignature(failure)}\n`);
process.stdout.write(`coalesced=${coalescedPendingStats}:calls=${coalescedLoader.calls.get("coalesced")}:${responseSignature(coalescedFirst)},${responseSignature(coalescedSecond)}:${statsSignature(coalesced)}\n`);
process.stdout.write(`configuration=${invalidConfiguration}\n`);
