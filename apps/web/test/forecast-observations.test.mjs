import assert from "node:assert/strict";
import test from "node:test";
import {
  buildForecastObservationPoints,
  parseForecastObservationResponse,
  renderWeatherDashboard,
  WeatherDashboardController,
} from "../dist/index.js";

const at = "2026-08-22T05:37:00.000Z";
const from = "2026-08-21T07:00:00.000Z";
const hour = "2026-08-22T04:00:00.000Z";
const site = { latitude: 47.950429954185445, longitude: -122.42797012608193, name: "Ballydidean", slug: "ballydidean", stations: [], timezone: "America/Los_Angeles" };

// create one normalized recording without adjustment metadata
function record(id, validAt, metrics, station = "ballydidean-ecowitt", source = station, kind = "physical_sensor") {
  return {
    id, validAt, receivedAt: validAt, productRunAt: null, revisionCount: 0,
    freshness: { ageSeconds: 60_000, status: "stale", label: "Historical" },
    metadata: { device: null, provider: null, quality: null, upstream: { model: null, timezone: site.timezone } },
    metrics: Object.fromEntries([
      "apparentTemperatureC", "blackGlobeTemperatureC", "cloudCoverPercent", "pm25MicrogramsPerCubicMeter", "precipitationMm",
      "precipitationRateMmPerHour", "pressureHpa", "relativeHumidityPercent", "soilElectricalConductivityMicrosiemensPerCm",
      "soilMoisturePercent", "solarRadiationWm2", "temperatureC", "uvIndex", "windDirectionDegrees", "windGustMps", "windSpeedMps", "wetBulbGlobeTemperatureC",
    ].map((key) => [key, metrics[key] ?? null])),
    provenance: {
      attribution: { label: "Weather", url: "https://example.test" }, label: kind,
      providerKey: station === "ballydidean-ecowitt" ? "ecowitt-local" : kind === "model_current" || kind === "forecast" ? "open-meteo" : "weatherflow-tempest",
      sourceId: source, sourceKey: station === "ballydidean-ecowitt" ? "ecowitt-88f15505d89f-local-live-v1" : source, sourceKind: kind, stationSlug: station,
    },
  };
}

// read the rendered synchronized series rather than decorative classes
function series(html, key) {
  const start = html.indexOf(`data-forecast-chart="${key}"`);
  const encoded = html.slice(start).match(/data-forecast-series="([^"]+)"/u)?.[1];
  assert.ok(encoded);
  return JSON.parse(encoded.replaceAll("&quot;", '"').replaceAll("&amp;", "&"));
}

// enforce per-metric farm precedence and one equal vote per nearby station
test("recorded hourly points use farm then station mean then recorded regional values", () => {
  const records = [
    record("farm-1", "2026-08-22T04:10:00.000Z", { temperatureC: 10, relativeHumidityPercent: 0, precipitationRateMmPerHour: 0 }),
    record("farm-2", "2026-08-22T04:20:00.000Z", { temperatureC: 14, pm25MicrogramsPerCubicMeter: -1 }),
    record("a-1", "2026-08-22T04:10:00.000Z", { temperatureC: 40, pm25MicrogramsPerCubicMeter: 4 }, "a"),
    record("a-2", "2026-08-22T04:20:00.000Z", { pm25MicrogramsPerCubicMeter: 8 }, "a"),
    record("a-duplicate", "2026-08-22T04:05:00.000Z", { pm25MicrogramsPerCubicMeter: 100 }, "a", "a-other"),
    record("b-1", "2026-08-22T04:10:00.000Z", { pm25MicrogramsPerCubicMeter: 10 }, "b"),
    record("regional", "2026-08-22T04:15:00.000Z", { cloudCoverPercent: 25, temperatureC: 99 }, "open-meteo-virtual", "model", "model_current"),
    record("foreign", "2026-08-22T04:15:00.000Z", { uvIndex: 19 }, "foreign", "reanalysis", "reanalysis"),
    record("future", "2026-08-22T05:50:00.000Z", { temperatureC: 50 }),
  ];
  const before = JSON.stringify(records);
  const points = buildForecastObservationPoints(records, from, at);
  const point = points.find((item) => item.validAt === hour);
  assert.equal(point.metrics.temperatureC, 12);
  assert.equal(point.metrics.relativeHumidityPercent, 0);
  assert.equal(point.metrics.precipitationRateMmPerHour, 0);
  assert.equal(point.metrics.pm25MicrogramsPerCubicMeter, 8);
  assert.equal(point.metrics.cloudCoverPercent, 25);
  assert.equal(point.metrics.uvIndex, undefined);
  assert.deepEqual(point.sources, { temperatureC: "ballydidean", relativeHumidityPercent: "ballydidean", precipitationRateMmPerHour: "ballydidean", pm25MicrogramsPerCubicMeter: "local_mean", cloudCoverPercent: "regional" });
  assert.equal(JSON.stringify(records), before);
  assert.equal(points.some((item) => item.validAt === "2026-08-22T06:00:00.000Z"), false);
});

// keep report increments distinct from rain rates and retain the actual preceding-hour gust
test("historical gust uses recorded hourly maximum and rain uses only measured rates", () => {
  const points = buildForecastObservationPoints([
    record("a", "2026-08-22T03:05:00.000Z", { windGustMps: 10, precipitationMm: 8 }),
    record("b", "2026-08-22T03:55:00.000Z", { windGustMps: 2, precipitationMm: 9 }),
    record("c", "2026-08-22T04:15:00.000Z", { windGustMps: 5, precipitationMm: 10 }),
  ], from, at);
  const point = points.find((item) => item.validAt === hour);
  assert.equal(point.metrics.windGustMps, 10);
  assert.equal(point.metrics.precipitationRateMmPerHour, undefined);
  assert.equal(point.metrics.precipitationMm, undefined);
});

// resolve revisions once and never derive a pressure jump across providers or stations
test("recorded pressure changes require complete same-source windows before tier averaging", () => {
  const records = [];
  // construct source-local pressure windows with different absolute datums
  for (let index = 0; index < 4; index += 1) {
    const validAt = new Date(Date.parse(hour) - (3 - index) * 3_600_000 + 10 * 60_000).toISOString();
    records.push(record(`farm-${index}`, validAt, { pressureHpa: 1020 + index }));
    records.push(record(`a-${index}`, validAt, { pressureHpa: 980 + 2 * index }, "a"));
    records.push(record(`b-${index}`, validAt, { pressureHpa: 1000 + 4 * index }, "b"));
  }
  const points = buildForecastObservationPoints(records, from, at);
  assert.equal(points.find((point) => point.validAt === hour).pressureChange3hHpa, 3);
  const withoutFarm = records.filter((item) => !item.id.startsWith("farm"));
  const average = buildForecastObservationPoints(withoutFarm, from, at).find((point) => point.validAt === hour);
  assert.equal(average.pressureChange3hHpa, 9);
  assert.equal(average.sources.pressureChange3hHpa, "local_mean");
  const mixed = records.filter((item) => item.id.startsWith("farm") && item.id !== "farm-1").map((item) => item.id === "farm-3" ? { ...item, provenance: { ...item.provenance, sourceId: "replacement" } } : item);
  assert.equal(buildForecastObservationPoints(mixed, from, at).find((point) => point.validAt === hour).pressureChange3hHpa, null);
  const revised = { ...records[0], receivedAt: "2026-08-22T05:00:00.000Z", metrics: { ...records[0].metrics, pressureHpa: 1021 } };
  assert.equal(buildForecastObservationPoints([records[0], revised, ...records.slice(1)], from, at).find((point) => point.validAt === hour).pressureChange3hHpa, 2);
});

// keep normalized domains and circular bearings safe in historical means
test("recorded means reject invalid readings and average north across zero", () => {
  const points = buildForecastObservationPoints([
    record("a", hour, { windDirectionDegrees: 350, uvIndex: 100, relativeHumidityPercent: -1 }, "a"),
    record("b", hour, { windDirectionDegrees: 10, uvIndex: 0, relativeHumidityPercent: 100 }, "b"),
  ], from, at);
  const point = points.find((item) => item.validAt === hour);
  assert.ok(Math.min(point.metrics.windDirectionDegrees, 360 - point.metrics.windDirectionDegrees) < 1e-9);
  assert.equal(point.metrics.uvIndex, 0);
  assert.equal(point.metrics.relativeHumidityPercent, 100);
});

// keep historical projections independent of forecast corrections and the persisted switch
test("recorded chart values and raw regional historical fallback do not change with adjustments", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date(at) });
  const forecast = [hour, "2026-08-22T05:00:00.000Z", "2026-08-22T06:00:00.000Z"].map((validAt, index) => ({
    ...record(`forecast-${index}`, validAt, { temperatureC: 20, apparentTemperatureC: 21, relativeHumidityPercent: 50, pressureHpa: 1000 }, "open-meteo-virtual", "forecast", "forecast"),
    temperatureAdjustment: { state: "active", correctedTemperatureC: 25 },
  }));
  const state = {
    ...new WeatherDashboardController({ storage: null }).state,
    loading: false, selectedSite: site, forecast,
    current: [record("now", at, {}, "open-meteo-virtual", "model", "model_current")],
    forecastObservations: [{ validAt: hour, metrics: { temperatureC: 12, relativeHumidityPercent: 75 }, pressureChange3hHpa: 1.5, sources: { temperatureC: "ballydidean", relativeHumidityPercent: "ballydidean", pressureChange3hHpa: "ballydidean" } }],
  };
  const adjusted = renderWeatherDashboard(state, "forecast");
  const raw = renderWeatherDashboard({ ...state, forecastAdjustmentMode: "raw" }, "forecast");
  assert.deepEqual(series(adjusted, "temperature")[1].values, [12, 20, 25]);
  assert.deepEqual(series(raw, "temperature")[1].values, [12, 20, 20]);
  assert.deepEqual(series(adjusted, "humidity")[0].values, [75, 50, 50]);
  assert.equal(series(adjusted, "pressure")[0].values[0], 1.5);
  assert.equal(series(raw, "pressure")[0].values[0], 1.5);
  assert.doesNotMatch(adjusted, /experimental/iu);
});

// refuse malformed or future hourly projections instead of partially accepting an envelope
test("historical projection boundary validates geometry values and source labels", () => {
  const point = { validAt: hour, metrics: { temperatureC: 12 }, pressureChange3hHpa: null, sources: { temperatureC: "ballydidean" } };
  assert.deepEqual(parseForecastObservationResponse({ generatedAt: at, data: [point] }), [point]);
  // reject malformed geometry and unsupported metric values atomically
  for (const data of [[point, point], [{ ...point, validAt: "2026-08-22T04:01:00.000Z" }], [{ ...point, validAt: "2026-08-22T06:00:00.000Z" }], [{ ...point, metrics: { temperatureC: Infinity } }], [{ ...point, metrics: { privateMetric: 12 } }], [{ ...point, sources: { temperatureC: "forecast" } }]]) {
    assert.throws(() => parseForecastObservationResponse({ generatedAt: at, data }), /invalid/u);
  }
});

// keep optional archive latency out of required products and fence late route or range results
test("deferred history cannot block forecasts or overwrite a newer route and horizon", async () => {
  let resolveHistory;
  let reads = 0;
  const pending = new Promise((resolve) => { resolveHistory = resolve; });
  const forecast = record("f", hour, { temperatureC: 20 }, "open-meteo-virtual", "forecast", "forecast");
  const controller = new WeatherDashboardController({ view: "forecast", storage: null, fetcher: async (input) => {
    const url = String(input);
    // hold only the optional archive while other required products finish
    if (url.endsWith("/forecast-observations")) {
      reads += 1;
      return pending;
    }
    return Response.json({ data: url.includes("/forecast") ? [forecast] : [], site });
  } });
  await controller.initialize();
  assert.equal(controller.state.loading, false);
  assert.equal(controller.state.forecastObservationsLoading, true);
  await controller.setForecastDays(5);
  assert.equal(reads, 1);
  await controller.setView("home");
  resolveHistory(Response.json({ generatedAt: at, data: [{ validAt: hour, metrics: { temperatureC: 42 }, pressureChange3hHpa: null, sources: { temperatureC: "ballydidean" } }] }));
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(controller.view, "home");
  assert.equal(controller.state.forecastDays, 5);
  assert.deepEqual(controller.state.forecastObservations, []);
});

// retain actual old samples without allowing them to hold a newer day at yesterday
test("a fresh regional clock advances past stale retained historical recordings", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now: new Date("2026-08-22T08:30:00.000Z") });
  const yesterday = { validAt: hour, metrics: { temperatureC: 12 }, pressureChange3hHpa: null, sources: { temperatureC: "ballydidean" }, recordedThrough: at };
  const future = record("f", "2026-08-22T09:00:00.000Z", { temperatureC: 20 }, "open-meteo-virtual", "forecast", "forecast");
  const state = { ...new WeatherDashboardController({ storage: null }).state, loading: false, selectedSite: site, current: [record("now", "2026-08-22T08:30:00.000Z", {}, "open-meteo-virtual", "model", "model_current")], forecast: [future], forecastObservations: [yesterday], forecastObservationsReady: false };
  const html = renderWeatherDashboard(state, "forecast");
  assert.match(html, /2026-08-22T09:00:00.000Z/u);
  assert.deepEqual(series(html, "temperature")[1].values, [20]);
  assert.match(html, /latest available recordings and regional values/u);
});
