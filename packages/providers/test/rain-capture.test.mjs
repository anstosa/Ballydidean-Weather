import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { RAIN_COLLECTION_STATIONS } from "@weather/domain";

import { fetchRainCapture } from "../dist/index.js";

const run = "2026-09-13T00:00:00.000Z";
const forecast = { kind: "forecast", slotKey: `forecast:${run}:1`, runInitializedAt: run, attempt: 1 };
const beforeDecision = () => new Date("2026-09-13T07:00:00.000Z");

// construct an exact synthetic seven-variable HRES response
function forecastPayload() {
  const origin = Date.parse(run);
  return {
    latitude: 47.97891,
    longitude: -122.44185,
    elevation: 50,
    timezone: "GMT",
    utc_offset_seconds: 0,
    hourly_units: {
      time: "iso8601",
      temperature_2m: "°C",
      relative_humidity_2m: "%",
      wind_speed_10m: "m/s",
      precipitation: "mm",
      surface_pressure: "hPa",
      cloud_cover: "%",
      wind_direction_10m: "°",
    },
    hourly: {
      time: Array.from({ length: 49 }, (_, lead) => new Date(origin + lead * 3_600_000).toISOString().slice(0, 16)),
      temperature_2m: Array(49).fill(12),
      relative_humidity_2m: Array(49).fill(70),
      wind_speed_10m: Array(49).fill(4),
      precipitation: Array(49).fill(0.2),
      surface_pressure: Array(49).fill(1010),
      cloud_cover: Array(49).fill(30),
      wind_direction_10m: Array(49).fill(360),
    },
  };
}

// capture one synthetic response without public HTTP
async function capture(body, options = {}) {
  const calls = [];
  const receipt = await fetchRainCapture(forecast, {
    now: beforeDecision,
    fetch: async (url, initialization) => {
      calls.push({ url, initialization });
      return new Response(body, { status: 200 });
    },
    ...options,
  });
  return { calls, receipt };
}

test("forecast request pins the exact seven-variable 49-hour run and raw bytes", async () => {
  const payload = forecastPayload();
  payload.hourly.precipitation[8] = null;
  const raw = Buffer.from(`\n${JSON.stringify(payload)}\n`);
  const { calls, receipt } = await capture(raw);
  const url = calls[0].url;

  assert.equal(url.origin, "https://single-runs-api.open-meteo.com");
  assert.equal(url.pathname, "/v1/forecast");
  assert.equal(url.searchParams.get("run"), "2026-09-13T00:00");
  assert.equal(url.searchParams.get("models"), "ecmwf_ifs");
  assert.equal(url.searchParams.get("forecast_hours"), "49");
  assert.equal(url.searchParams.get("hourly"), "temperature_2m,relative_humidity_2m,wind_speed_10m,precipitation,surface_pressure,cloud_cover,wind_direction_10m");
  assert.equal(url.searchParams.get("latitude"), "47.950429954185445");
  assert.equal(url.searchParams.get("longitude"), "-122.42797012608193");
  assert.equal(url.searchParams.get("precipitation_unit"), "mm");
  assert.equal(url.searchParams.get("timezone"), "GMT");
  assert.equal(calls[0].initialization.redirect, "error");
  assert.equal(receipt.outcome, "valid");
  assert.equal(receipt.rowCount, 48);
  assert.equal(receipt.metadata.nullCount, 1);
  assert.equal(receipt.availableByDecision, true);
  assert.equal(receipt.completedAt, "2026-09-13T07:00:00.000Z");
  assert.deepEqual(Buffer.from(receipt.body), raw);
  assert.equal(receipt.bodySha256, createHash("sha256").update(raw).digest("hex"));
});

test("late valid forecast remains intact but is unavailable at decision", async () => {
  const { receipt } = await capture(JSON.stringify(forecastPayload()), { now: () => new Date("2026-09-13T08:00:00.001Z") });
  assert.equal(receipt.outcome, "valid");
  assert.equal(receipt.availableByDecision, false);
  assert.ok(receipt.bodySha256);
});

test("shifted grid, changed units, range violations and wrong run are rejected", async () => {
  const shifted = forecastPayload();
  shifted.hourly.time[8] = "2026-09-13T09:00";
  assert.equal((await capture(JSON.stringify(shifted))).receipt.errorCode, "invalid_forecast_grid");

  const units = forecastPayload();
  units.hourly_units.wind_direction_10m = "rad";
  assert.equal((await capture(JSON.stringify(units))).receipt.errorCode, "invalid_forecast_schema");

  const range = forecastPayload();
  range.hourly.precipitation[1] = -1;
  assert.equal((await capture(JSON.stringify(range))).receipt.errorCode, "invalid_forecast_cell");

  const wrongGrid = forecastPayload();
  wrongGrid.latitude = 48.5;
  wrongGrid.longitude = -123;
  const rejected = (await capture(JSON.stringify(wrongGrid))).receipt;
  assert.equal(rejected.errorCode, "invalid_forecast_grid");
  assert.ok(rejected.bodySha256);

  const roundedGrid = forecastPayload();
  roundedGrid.latitude += 0.000001;
  assert.equal((await capture(JSON.stringify(roundedGrid))).receipt.outcome, "valid");

  await assert.rejects(fetchRainCapture({ ...forecast, runInitializedAt: "2026-09-13T01:00:00.000Z" }, { fetch: async () => { throw new Error("unreachable"); } }), /invalid forecast run identity/u);
  await assert.rejects(fetchRainCapture({ ...forecast, slotKey: "forecast:wrong" }, { fetch: async () => { throw new Error("unreachable"); } }), /invalid forecast run identity/u);
});

test("invalid UTF-8 and JSON retain complete bytes without leaking text", async () => {
  for (const raw of [Uint8Array.of(0xff), Buffer.from("provider secret: invalid json")]) {
    const { receipt } = await capture(raw);
    assert.equal(receipt.outcome, "invalid");
    assert.equal(receipt.errorCode, "invalid_json_or_payload");
    assert.deepEqual(Buffer.from(receipt.body), Buffer.from(raw));
    assert.equal(receipt.bodySha256, createHash("sha256").update(raw).digest("hex"));
    assert.doesNotMatch(JSON.stringify({ ...receipt, body: null }), /provider secret/u);
  }
});

test("complete HTTP error bodies are retained with safe status and retry hints", async () => {
  for (const [status, outcome] of [[429, "rate_limited"], [401, "unauthorized"], [503, "invalid"]]) {
    const raw = Buffer.from("do not expose provider details");
    const { receipt } = await capture(null, { fetch: async () => new Response(raw, { status, headers: { "retry-after": "60" } }) });
    assert.equal(receipt.httpStatus, status);
    assert.equal(receipt.outcome, outcome);
    assert.deepEqual(Buffer.from(receipt.body), raw);
    assert.equal(receipt.metadata.retryAfterSeconds, status === 429 ? 60 : undefined);
    assert.equal(receipt.metadata.retryAfterRequiresManualResume, status === 429 ? false : undefined);
    assert.doesNotMatch(JSON.stringify({ ...receipt, body: null }), /provider details/u);
  }
  const large = await capture(null, { fetch: async () => new Response("error", { status: 429, headers: { "retry-after": "604800" } }) });
  assert.equal(large.receipt.metadata.retryAfterSeconds, 604800);
  assert.equal(large.receipt.metadata.retryAfterRequiresManualResume, false);

  const manual = await capture(null, { fetch: async () => new Response("error", { status: 429, headers: { "retry-after": "604801" } }) });
  assert.equal(manual.receipt.metadata.retryAfterSeconds, null);
  assert.equal(manual.receipt.metadata.retryAfterRequiresManualResume, true);

  const unsupported = await capture(null, { fetch: async () => new Response("error", { status: 429, headers: { "retry-after": "Wed, 21 Oct 2030 07:28:00 GMT" } }) });
  assert.equal(unsupported.receipt.metadata.retryAfterRequiresManualResume, true);
});

test("transport failures and interrupted or oversized bodies do not invent complete bytes", async () => {
  const rejected = await capture(null, { fetch: async () => { throw new Error("secret-url?api_key=secret"); } });
  assert.equal(rejected.receipt.outcome, "transport_error");
  assert.equal(rejected.receipt.body, null);
  assert.equal(rejected.receipt.httpStatus, null);
  assert.doesNotMatch(JSON.stringify(rejected.receipt), /secret-url|api_key/u);

  let oversizedAborted = false;
  const oversized = await capture(null, { fetch: async (_url, initialization) => {
    initialization.signal.addEventListener("abort", () => { oversizedAborted = true; });
    return new Response(new Uint8Array(2_000_001));
  } });
  assert.equal(oversized.receipt.errorCode, "body_too_large");
  assert.equal(oversized.receipt.body, null);
  assert.equal(oversizedAborted, true);

  const interrupted = await capture(null, { fetch: async () => new Response(new ReadableStream({ start(controller) { controller.error(new Error("partial secret")); } })) });
  assert.equal(interrupted.receipt.outcome, "transport_error");
  assert.equal(interrupted.receipt.body, null);
});

test("Fetch null-body statuses differ from an ordinary complete empty body", async () => {
  // retain every Fetch-mandated null-body status as an incomplete exchange
  for (const status of [204, 205, 304]) {
    const { receipt } = await capture(null, {
      // return one response whose body is null by Fetch contract
      fetch: async () => new Response(null, { status }),
    });
    assert.deepEqual({
      body: receipt.body,
      bodySha256: receipt.bodySha256,
      errorCode: receipt.errorCode,
      httpStatus: receipt.httpStatus,
      outcome: receipt.outcome,
    }, {
      body: null,
      bodySha256: null,
      errorCode: "missing_body",
      httpStatus: null,
      outcome: "transport_error",
    });
  }

  const empty = await capture(new Uint8Array());
  assert.equal(empty.receipt.outcome, "invalid");
  assert.equal(empty.receipt.errorCode, "invalid_json_or_payload");
  assert.equal(empty.receipt.httpStatus, 200);
  assert.equal(empty.receipt.body.byteLength, 0);
  assert.equal(
    empty.receipt.bodySha256,
    createHash("sha256").update(new Uint8Array()).digest("hex"),
  );
});

test("one deadline covers a stalled response body", async () => {
  const originalSetTimeout = globalThis.setTimeout;
  // fire only the capture deadline immediately
  globalThis.setTimeout = (callback) => {
    queueMicrotask(callback);
    return 1;
  };
  try {
    const { receipt } = await capture(null, { fetch: async (_url, initialization) => new Response(new ReadableStream({
      start(controller) {
        controller.enqueue(Uint8Array.of(123));
        initialization.signal.addEventListener("abort", () => controller.error(new Error("interrupted")));
      },
    })) });
    assert.equal(receipt.outcome, "transport_error");
    assert.equal(receipt.errorCode, "timeout");
    assert.equal(receipt.body, null);
  } finally {
    globalThis.setTimeout = originalSetTimeout;
  }
});

test("station requests use every frozen device and preserve physical interval counts", async () => {
  const fixture = JSON.parse(await readFile(new URL("./fixtures/tempest/observations.json", import.meta.url), "utf8"));
  assert.equal(RAIN_COLLECTION_STATIONS.length, 12);
  for (const station of RAIN_COLLECTION_STATIONS) {
    const request = { kind: "station", slotKey: `station:${station.locationId}:2026-08-20T02:00:00.000Z`, stationId: station.locationId, start: "2026-08-20T00:00:00.000Z", endExclusive: "2026-08-20T02:00:00.000Z" };
    let seenUrl;
    const receipt = await fetchRainCapture(request, {
      apiKey: "private-test-key",
      now: () => new Date("2026-08-20T02:01:00.000Z"),
      fetch: async (url) => {
        seenUrl = url;
        return new Response(JSON.stringify({ ...fixture, device_id: station.deviceId }));
      },
    });
    assert.equal(seenUrl.pathname, `/swd/rest/observations/device/${station.deviceId}`);
    assert.equal(Number(seenUrl.searchParams.get("time_end")) - Number(seenUrl.searchParams.get("time_start")), 7199);
    assert.equal(receipt.outcome, "valid");
    assert.equal(receipt.rowCount, 3);
    assert.equal(receipt.metadata.stationId, station.locationId);
    assert.equal(receipt.metadata.deviceId, station.deviceId);
    assert.equal(receipt.availableByDecision, null);
    assert.doesNotMatch(JSON.stringify({ ...receipt, body: null }), /private-test-key|api_key/u);
  }
});

test("station identity, interval duration and future windows fail closed", async () => {
  const fixture = JSON.parse(await readFile(new URL("./fixtures/tempest/observations.json", import.meta.url), "utf8"));
  const request = { kind: "station", slotKey: "station:203055:2026-08-20T02:00:00.000Z", stationId: 203055, start: "2026-08-20T00:00:00.000Z", endExclusive: "2026-08-20T02:00:00.000Z" };
  const options = { apiKey: "private-test-key", now: () => new Date("2026-08-20T02:01:00.000Z") };

  const wrongDevice = await fetchRainCapture(request, { ...options, fetch: async () => new Response(JSON.stringify({ ...fixture, device_id: 123 })) });
  assert.equal(wrongDevice.outcome, "invalid");
  assert.deepEqual(Object.keys(wrongDevice.metadata), []);

  fixture.obs[0][17] = 6;
  const wrongInterval = await fetchRainCapture(request, { ...options, fetch: async () => new Response(JSON.stringify(fixture)) });
  assert.equal(wrongInterval.errorCode, "invalid_station_interval");

  await assert.rejects(fetchRainCapture({ ...request, endExclusive: "2026-08-21T00:00:00.000Z", slotKey: "station:203055:2026-08-21T00:00:00.000Z" }, { ...options, fetch: async () => { throw new Error("unreachable"); } }), /invalid station observation window/u);
  await assert.rejects(fetchRainCapture({ ...request, stationId: 999 }, { ...options, fetch: async () => { throw new Error("unreachable"); } }), /invalid station request identity/u);
  await assert.rejects(fetchRainCapture({ ...request, slotKey: "station:wrong" }, { ...options, fetch: async () => { throw new Error("unreachable"); } }), /invalid station request identity/u);
});

test("duplicate raw Tempest endpoints never first-wins, even when values agree", async () => {
  const fixture = JSON.parse(await readFile(new URL("./fixtures/tempest/observations.json", import.meta.url), "utf8"));
  const request = { kind: "station", slotKey: "station:203055:2026-08-20T02:00:00.000Z", stationId: 203055, start: "2026-08-20T00:00:00.000Z", endExclusive: "2026-08-20T02:00:00.000Z" };
  const options = { apiKey: "private-test-key", now: () => new Date("2026-08-20T02:01:00.000Z") };

  for (const amount of [fixture.obs[0][12], 42]) {
    const duplicate = structuredClone(fixture);
    const repeated = [...duplicate.obs[0]];
    repeated[12] = amount;
    duplicate.obs.push(repeated);
    const raw = Buffer.from(JSON.stringify(duplicate));
    const receipt = await fetchRainCapture(request, { ...options, fetch: async () => new Response(raw) });
    assert.equal(receipt.outcome, "invalid");
    assert.equal(receipt.errorCode, "duplicate_station_interval");
    assert.equal(receipt.rowCount, 0);
    assert.deepEqual(Buffer.from(receipt.body), raw);
    assert.equal(receipt.bodySha256, createHash("sha256").update(raw).digest("hex"));
  }
});
