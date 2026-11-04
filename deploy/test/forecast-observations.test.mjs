import assert from "node:assert/strict";
import test from "node:test";
import {
  buildForecastObservationPoints,
  buildSolarCloudEvidence,
  fromSiteWallClock,
  toSiteWallClock,
} from "../../apps/web/dist/index.js";
import { ForecastObservationCache } from "../scripts/forecast-observations.mjs";

const at = new Date("2026-08-22T05:37:00.000Z");
const metricKeys = [
  "apparentTemperatureC", "cloudCoverPercent", "pm25MicrogramsPerCubicMeter",
  "precipitationRateMmPerHour", "pressureHpa", "relativeHumidityPercent",
  "solarRadiationWm2", "temperatureC", "uvIndex", "windDirectionDegrees",
  "windGustMps", "windSpeedMps",
];

// create one narrow input record
function record(index, overrides = {}) {
  const metrics = Object.fromEntries(metricKeys.map((key) => [key, null]));
  return {
    id: String(index + 1),
    metrics: { ...metrics, temperatureC: 10, ...overrides.metrics },
    provenance: {
      providerKey: "ecowitt-local", sourceId: "1",
      sourceKey: "ecowitt-88f15505d89f-local-live-v1",
      sourceKind: "physical_sensor", stationSlug: "ballydidean-ecowitt",
      ...overrides.provenance,
    },
    receivedAt: overrides.receivedAt ?? overrides.validAt ?? "2026-08-21T12:00:00.000Z",
    validAt: overrides.validAt ?? "2026-08-21T12:00:00.000Z",
  };
}

// create one exact internal envelope
function envelope(records = [], generatedAt = at.toISOString(), overrides = {}) {
  const localDay = toSiteWallClock(generatedAt, "America/Los_Angeles").slice(0, 10);
  const projectionFrom = fromSiteWallClock(`${localDay}T00:00`, "America/Los_Angeles");
  const queryFrom = new Date(Math.min(
    Date.parse(projectionFrom) - 3 * 3_600_000,
    Date.parse(generatedAt) - 24 * 3_600_000 - 20 * 60_000,
  )).toISOString();
  return {
    contractVersion: "forecast-observation-input/v1",
    generatedAt,
    projectionFrom,
    queryFrom,
    records,
    site: { slug: "ballydidean", timezone: "America/Los_Angeles" },
    through: generatedAt,
    ...overrides,
  };
}

// serve one JSON envelope
function response(value) {
  return Response.json(value);
}

// mirror the repository's frozen physical-then-model order
function ordered(records) {
  return records.toSorted((left, right) => {
    const leftKind = left.provenance.sourceKind === "physical_sensor" ? 0 : 1;
    const rightKind = right.provenance.sourceKind === "physical_sensor" ? 0 : 1;
    return leftKind - rightKind || Date.parse(right.validAt) - Date.parse(left.validAt) ||
      Number(BigInt(right.id) - BigInt(left.id));
  });
}

// prove the compact transport preserves complete legacy projection results
 test("compact input preserves observations and solar evidence with one shared request", async () => {
  const physical = Array.from({ length: 251 }, (_, index) => record(index));
  physical.push(record(300, {
    metrics: { solarRadiationWm2: 800, temperatureC: 42 },
    validAt: "2026-08-22T04:00:00.000Z",
  }));
  const regional = record(400, {
    metrics: { cloudCoverPercent: 80 },
    provenance: {
      providerKey: "open-meteo", sourceId: "2", sourceKey: "open-meteo-current-v1",
      sourceKind: "model_current", stationSlug: "open-meteo-virtual",
    },
    validAt: "2026-08-22T03:55:00.000Z",
  });
  const records = ordered([...physical, regional]);
  let requests = 0;
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const cache = new ForecastObservationCache({
    origin: "http://api:3001", now: () => at,
    fetcher: async (input) => {
      requests += 1;
      assert.equal(new URL(input).pathname, "/internal/v1/forecast-observation-input");
      await gate;
      return response(envelope(records));
    },
  });
  const first = cache.get();
  const second = cache.get();
  release();
  const [left, right] = await Promise.all([first, second]);
  const source = envelope(records);
  assert.deepEqual(left.data, buildForecastObservationPoints(records, source.projectionFrom, source.through));
  assert.deepEqual(left.solarCloudEvidence, buildSolarCloudEvidence(records, { latitude: 47.950429954185445, longitude: -122.42797012608193 }, source.through));
  assert.equal(left, right);
  assert.equal(requests, 1);
  assert.equal(await cache.get(), left);
  assert.equal(requests, 1);
});

// refuse malformed geometry records and order without publishing partial state
 test("compact input rejects malformed geometry records and ordering then recovers", async () => {
  const valid = [record(2, { validAt: "2026-08-21T13:00:00.000Z" }), record(1)];
  for (const broken of [
    envelope(valid, at.toISOString(), { queryFrom: "2026-08-21T05:00:00.000Z" }),
    envelope([{ ...valid[0], metrics: { ...valid[0].metrics, pressureHpa: "bad" } }]),
    envelope([{ ...valid[0], id: 3 }]),
    envelope(valid.toReversed()),
    envelope([{ ...valid[0], metadata: {} }]),
  ]) {
    let fail = true;
    const cache = new ForecastObservationCache({
      origin: "http://api:3001", now: () => at, retryMs: 0,
      fetcher: async () => response(fail ? broken : envelope([])),
    });
    await assert.rejects(cache.get());
    fail = false;
    assert.equal((await cache.get()).data.length, 23);
  }
});

// refill both waiters when one successful request crosses farm midnight
test("compact cache initiator and joiner recheck the farm day after a fill", async () => {
  const beforeMidnight = new Date("2026-08-22T06:59:59.000Z");
  const afterMidnight = new Date("2026-08-22T07:00:01.000Z");
  let now = beforeMidnight;
  let requests = 0;
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const cache = new ForecastObservationCache({
    origin: "http://api:3001", now: () => now,
    fetcher: async () => {
      requests += 1;
      // delay only the first pre-midnight snapshot until the next farm day
      if (requests === 1) {
        await gate;
        return response(envelope([], beforeMidnight.toISOString()));
      }
      return response(envelope([], afterMidnight.toISOString()));
    },
  });
  const initiator = cache.get();
  const joiner = cache.get();
  now = afterMidnight;
  release();
  const [left, right] = await Promise.all([initiator, joiner]);
  assert.equal(left, right);
  assert.equal(left.generatedAt, afterMidnight.toISOString());
  assert.equal(left.data.length, 1);
  assert.equal(requests, 2);
  assert.equal(await cache.get(), left);
  assert.equal(requests, 2);
});

// preserve expiry and local-midnight invalidation including fall DST geometry
 test("compact cache expires and never serves the prior farm day", async () => {
  let now = at;
  let requests = 0;
  const cache = new ForecastObservationCache({
    origin: "http://api:3001", now: () => now,
    fetcher: async () => { requests += 1; return response(envelope([], now.toISOString())); },
  });
  await cache.get();
  now = new Date(at.getTime() + 5 * 60_000);
  await cache.get();
  assert.equal(requests, 2);
  now = new Date("2026-11-02T07:59:00.000Z");
  assert.equal((await cache.get()).data.length, 25);
  now = new Date("2026-11-02T08:01:00.000Z");
  assert.equal((await cache.get()).data.length, 1);
  assert.equal(requests, 4);
});

// keep retry cooldown and body limits independent from successful cache state
 test("compact failures cool down and bounded recovery never caches partial input", async () => {
  const oversized = new ForecastObservationCache({
    origin: "http://api:3001", now: () => at, maxBytes: 3, retryMs: 0,
    fetcher: async () => new Response(new ReadableStream({
      // expose invalid UTF-8 that must never reach the fatal decoder
      start(controller) {
        controller.enqueue(Uint8Array.from([0xff, 0xff, 0xff, 0xff]));
        controller.close();
      },
    })),
  });
  await assert.rejects(oversized.get(), /limit/u);
  let now = at;
  let broken = true;
  let calls = 0;
  const cache = new ForecastObservationCache({
    origin: "http://api:3001", now: () => now, maxBytes: 100,
    fetcher: async () => {
      calls += 1;
      return broken ? response(envelope([record(1)])) : response(envelope([]));
    },
  });
  await assert.rejects(cache.get(), /limit/u);
  const failedCalls = calls;
  await assert.rejects(cache.get(), /cooldown/u);
  assert.equal(calls, failedCalls);
  now = new Date(at.getTime() + 30_000);
  broken = false;
  await assert.rejects(cache.get(), /limit/u);
});

// contain an optional solar comparison refusal while retaining observations
 test("optional solar refusal leaves compact observations available", async () => {
  const generatedAt = "2026-06-22T05:00:00.000Z";
  const now = new Date(generatedAt);
  const physical = Array.from({ length: 257 }, (_, index) => record(index, {
    metrics: { solarRadiationWm2: 0 }, validAt: "2026-06-21T20:00:00.000Z",
  }));
  const regional = Array.from({ length: 250 }, (_, index) => record(index + 300, {
    metrics: { cloudCoverPercent: 80 },
    provenance: {
      providerKey: "open-meteo", sourceId: "model", sourceKey: "open-meteo-current-v1",
      sourceKind: "model_current", stationSlug: "open-meteo-virtual",
    },
    validAt: "2026-06-21T20:00:00.000Z",
  }));
  const cache = new ForecastObservationCache({
    origin: "http://api:3001", now: () => now,
    fetcher: async () => response(envelope(ordered([...physical, ...regional]), generatedAt)),
  });
  const body = await cache.get();
  assert.equal(body.solarCloudEvidence, null);
  assert.equal(body.data.find((point) => point.validAt === "2026-06-21T20:00:00.000Z").metrics.cloudCoverPercent, 80);
});

// abort an input transport that exceeds the unchanged complete-fill deadline
 test("compact input timeout refuses incomplete work", async () => {
  const cache = new ForecastObservationCache({
    origin: "http://api:3001", now: () => at, timeoutMs: 1,
    fetcher: async () => {
      await new Promise((resolve) => setTimeout(resolve, 5));
      return response(envelope([]));
    },
  });
  await assert.rejects(cache.get());
});
