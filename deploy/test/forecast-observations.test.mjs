import assert from "node:assert/strict";
import test from "node:test";
import { ForecastObservationCache } from "../scripts/forecast-observations.mjs";

const at = new Date("2026-08-22T05:37:00.000Z");
const site = { slug: "ballydidean", name: "Ballydídean", latitude: 47.950429954185445, longitude: -122.42797012608193, stations: [], timezone: "America/Los_Angeles" };

// create the same normalized raw envelope served by the frozen core
function record(index, validAt = "2026-08-21T12:00:00.000Z", temperatureC = 10) {
  return {
    id: String(index), validAt, receivedAt: validAt, productRunAt: null, revisionCount: 0,
    freshness: { ageSeconds: 1000, status: "stale", label: "Historical" },
    metadata: { device: null, provider: null, quality: null, upstream: { model: null, timezone: site.timezone } },
    metrics: {
      apparentTemperatureC: null, blackGlobeTemperatureC: null, cloudCoverPercent: null, pm25MicrogramsPerCubicMeter: null,
      precipitationMm: null, precipitationRateMmPerHour: null, pressureHpa: null, relativeHumidityPercent: null,
      soilElectricalConductivityMicrosiemensPerCm: null, soilMoisturePercent: null, solarRadiationWm2: null,
      temperatureC, uvIndex: null, windDirectionDegrees: null, windGustMps: null, windSpeedMps: null, wetBulbGlobeTemperatureC: null,
    },
    provenance: { attribution: { label: "Weather", url: "https://example.test/" }, label: "physical sensor", providerKey: "ecowitt-local", sourceId: "1", sourceKey: "ecowitt-88f15505d89f-local-live-v1", sourceKind: "physical_sensor", stationSlug: "ballydidean-ecowitt" },
  };
}

// encode a complete bounded page with an opaque stable cursor
function page(data = [], nextCursor = null, overrides = {}) {
  return Response.json({ data, page: { limit: 250, nextCursor }, site, ...overrides });
}

// prove all pages and both source classes use one immutable geometry and shared cache
test("forecast history fully pages beyond 250 and singleflights concurrent viewers", async () => {
  const requests = [];
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const cache = new ForecastObservationCache({ origin: "http://api:3001", now: () => at, fetcher: async (input) => {
    const url = new URL(input);
    requests.push(url);
    await gate;
    // a missing regional archive leaves the physical class independently usable
    if (url.searchParams.get("sourceKind") === "model_current") {
      return page();
    }
    // retain a second page containing the farm metric actually plotted
    if (url.searchParams.get("cursor") === "second") {
      return page([record(251, "2026-08-22T04:00:00.000Z", 42)]);
    }
    return page(Array.from({ length: 250 }, (_, index) => record(index)), "second");
  } });
  const first = cache.get();
  const second = cache.get();
  release();
  const [left, right] = await Promise.all([first, second]);
  assert.deepEqual(left, right);
  assert.equal(left.data.find((point) => point.validAt === "2026-08-22T04:00:00.000Z").metrics.temperatureC, 42);
  assert.equal(requests.length, 3);
  assert.equal(await cache.get(), left);
  assert.equal(requests.length, 3);
  // retain local midnight plus pressure context on every page
  for (const url of requests) {
    assert.equal(url.pathname, "/api/v1/sites/ballydidean/history");
    assert.equal(url.searchParams.get("from"), "2026-08-21T04:00:00.000Z");
    assert.equal(url.searchParams.get("to"), at.toISOString());
    assert.equal(url.searchParams.get("limit"), "250");
  }
});

// cache only fully completed paging and permit bounded recovery after every refusal
test("repeated cursors limits malformed pages and errors never publish partial observations", async () => {
  // exercise independent refusal branches without weakening archive completeness
  for (const scenario of ["cursor", "pages", "bytes", "invalid", "status", "timeout"]) {
    let broken = true;
    let requests = 0;
    const cache = new ForecastObservationCache({
      origin: "http://api:3001", now: () => at, retryMs: 0, maxPages: 2, maxBytes: scenario === "bytes" ? 100 : 100_000, timeoutMs: scenario === "timeout" ? 1 : 30_000,
      fetcher: async (input) => {
        requests += 1;
        // after refusal a fresh empty envelope proves that no failed fill was cached
        if (!broken) {
          return page();
        }
        // isolate each frozen upstream class while forcing one incomplete class
        if (new URL(input).searchParams.get("sourceKind") === "model_current") {
          return page();
        }
        // retain transport refusal separately from payload and capacity guards
        if (scenario === "status") {
          return new Response("unavailable", { status: 503 });
        }
        // simulate a transport that returns too late despite receiving an abort signal
        if (scenario === "timeout") {
          await new Promise((resolve) => setTimeout(resolve, 5));
          return page();
        }
        // malformed classes cannot cross the normalized parser boundary
        if (scenario === "invalid") {
          return page([record(1)], null, { site: { ...site, slug: "other" } });
        }
        const cursor = scenario === "pages" ? `page-${requests}` : "repeat";
        return page([record(1)], cursor);
      },
    });
    await assert.rejects(cache.get());
    broken = false;
    // the tiny byte limit still refuses even an empty envelope as designed
    if (scenario === "bytes" || scenario === "timeout") {
      continue;
    }
    const before = requests;
    const recovered = await cache.get();
    assert.equal(requests, before + 2);
    assert.ok(recovered.data.length > 0);
    assert.equal(recovered.data.every((point) => Object.keys(point.metrics).length === 0), true);
  }
});

// re-read after five minutes and never serve the previous farm day after midnight
test("recorded projection expires and handles farm-local DST days", async () => {
  let now = at;
  const requests = [];
  const cache = new ForecastObservationCache({ origin: "http://api:3001", now: () => now, fetcher: async (input) => {
    requests.push(new URL(input));
    return page();
  } });
  await cache.get();
  now = new Date(at.getTime() + 5 * 60_000);
  await cache.get();
  assert.equal(requests.length, 4);
  now = new Date("2026-11-02T07:59:00.000Z");
  const fall = await cache.get();
  assert.equal(fall.data.length, 25);
  assert.equal(requests.at(-1).searchParams.get("from"), "2026-11-01T04:00:00.000Z");
  now = new Date("2026-11-02T08:01:00.000Z");
  const next = await cache.get();
  assert.equal(next.data.length, 1);
  assert.equal(requests.at(-1).searchParams.get("from"), "2026-11-01T07:41:00.000Z");
});

// bound repeated public failure load without caching partial data as a success
test("failed fills have a short retry cooldown before complete recovery", async () => {
  let now = at;
  let calls = 0;
  let broken = true;
  const cache = new ForecastObservationCache({ origin: "http://api:3001", now: () => now, fetcher: async () => {
    calls += 1;
    return broken ? new Response("offline", { status: 503 }) : page();
  } });
  await assert.rejects(cache.get());
  const before = calls;
  await assert.rejects(cache.get(), /cooldown/u);
  assert.equal(calls, before);
  now = new Date(at.getTime() + 30_000);
  broken = false;
  assert.ok((await cache.get()).data.length > 0);
  assert.equal(calls, before + 2);
});

// a restarted edge recovers yesterday's original daylight pair without changing today's graph
test("shared completed archive recovers daylight evidence across local midnight", async () => {
  const now = new Date("2026-06-22T08:01:00.000Z");
  const physical = { ...record("ws90", "2026-06-21T20:00:00.000Z"), metrics: { ...record(1).metrics, solarRadiationWm2: 800 } };
  const regional = { ...record("regional", "2026-06-21T19:55:00.000Z"), metrics: { ...record(1).metrics, cloudCoverPercent: 80 },
    provenance: { ...record(1).provenance, providerKey: "open-meteo", sourceId: "model", sourceKey: "open-meteo-current-v1", sourceKind: "model_current", stationSlug: "open-meteo-virtual" } };
  const urls = [];
  const cache = new ForecastObservationCache({ origin: "http://api:3001", now: () => now, fetcher: async (input) => {
    const url = new URL(input);
    urls.push(url);
    return page([url.searchParams.get("sourceKind") === "physical_sensor" ? physical : regional]);
  } });
  const [observations, evidence] = await Promise.all([cache.get(), cache.get()]);
  assert.equal(observations, evidence);
  assert.equal(urls.length, 2);
  assert.equal(urls[0].searchParams.get("from"), "2026-06-21T07:41:00.000Z");
  assert.equal(evidence.solarCloudEvidence.evaluatedAt, "2026-06-21T20:00:00.000Z");
  assert.equal(evidence.solarCloudEvidence.ws90.recordId, "ws90");
  assert.equal(evidence.solarCloudEvidence.regional.recordId, "regional");
  assert.equal(observations.data.length, 2);
  assert.deepEqual(observations.data[0].metrics, {});
  assert.equal(JSON.stringify(evidence.solarCloudEvidence).includes("metadata"), false);
});

// refuse pathological optional pair growth without discarding a fully recorded graph
test("optional solar comparison refusal leaves complete observations available", async () => {
  const now = new Date("2026-06-22T05:00:00.000Z");
  const physical = Array.from({ length: 257 }, (_, index) => ({ ...record(index, "2026-06-21T20:00:00.000Z"), metrics: { ...record(1).metrics, solarRadiationWm2: 0 } }));
  const regional = Array.from({ length: 250 }, (_, index) => ({ ...record(`model-${index}`, "2026-06-21T20:00:00.000Z"), metrics: { ...record(1).metrics, cloudCoverPercent: 80 },
    provenance: { ...record(1).provenance, providerKey: "open-meteo", sourceId: "model", sourceKey: "open-meteo-current-v1", sourceKind: "model_current", stationSlug: "open-meteo-virtual" } }));
  const cache = new ForecastObservationCache({ origin: "http://api:3001", now: () => now, fetcher: async (input) => {
    const url = new URL(input);
    // page the complete physical revision population before evaluating the optional pair budget
    if (url.searchParams.get("sourceKind") === "physical_sensor") {
      return url.searchParams.has("cursor") ? page(physical.slice(250)) : page(physical.slice(0, 250), "more");
    }
    return page(regional);
  } });
  const body = await cache.get();
  assert.equal(body.solarCloudEvidence, null);
  assert.equal(body.data.find((point) => point.validAt === "2026-06-21T20:00:00.000Z").metrics.cloudCoverPercent, 80);
  assert.equal(await cache.get(), body);
});
