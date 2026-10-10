import assert from "node:assert/strict";
import test from "node:test";

import {
  currentWeatherIcon,
  DEFAULT_UNIT_PREFERENCES,
  forecastMetricValue,
  NOW_ICON_STORAGE_KEY,
  renderWeatherDashboard,
  WeatherDashboardController,
  withSolarCloudAdjustment,
} from "../dist/index.js";

const latitude = 47.950429954185445;
const longitude = -122.42797012608193;
const now = new Date("2026-06-21T20:00:00.000Z");
const site = {
  latitude,
  longitude,
  name: "Ballydídean",
  slug: "ballydidean",
  stations: [],
  timezone: "America/Los_Angeles",
};

// create one complete normalized weather record
function weatherRecord({
  cloudCoverPercent = null,
  freshnessStatus = "fresh",
  id,
  precipitationRateMmPerHour = 0,
  providerKey,
  receivedAt,
  solarRadiationWm2 = null,
  sourceKind,
  stationSlug,
  validAt,
}) {
  return {
    freshness: {
      ageSeconds: 0,
      label: "Current",
      status: freshnessStatus,
    },
    id,
    metadata: {
      device: null,
      provider: {
        dataset: sourceKind === "forecast" ? "forecast" : "best_match",
        elevationM: 17,
        gridCell: null,
        propertySensors: null,
      },
      quality: null,
      upstream: { model: "best_match", timezone: site.timezone },
    },
    metrics: {
      apparentTemperatureC: 20,
      blackGlobeTemperatureC: null,
      cloudCoverPercent,
      pm25MicrogramsPerCubicMeter: null,
      precipitationMm: 0,
      precipitationRateMmPerHour,
      pressureHpa: 1013,
      relativeHumidityPercent: 50,
      soilElectricalConductivityMicrosiemensPerCm: null,
      soilMoisturePercent: null,
      solarRadiationWm2,
      temperatureC: 20,
      uvIndex: 5,
      windDirectionDegrees: 180,
      windGustMps: 2,
      windSpeedMps: 1,
      wetBulbGlobeTemperatureC: null,
    },
    pressureChange3hHpa: 0,
    productRunAt: sourceKind === "forecast" ? "2026-06-21T19:00:00.000Z" : null,
    provenance: {
      attribution: { label: providerKey, url: "https://example.test/weather" },
      label: sourceKind,
      providerKey,
      sourceId: `${providerKey}-${sourceKind}-${id}`,
      sourceKey: `${providerKey}-${sourceKind}`,
      sourceKind,
      stationSlug,
    },
    receivedAt,
    revisionCount: 0,
    validAt,
  };
}

// create one exact on-site radiation sample
function ws90(overrides = {}) {
  return weatherRecord({
    id: "ws90",
    providerKey: "ecowitt-local",
    receivedAt: "2026-06-21T20:00:00.000Z",
    solarRadiationWm2: 800,
    sourceKind: "physical_sensor",
    stationSlug: "ballydidean-ecowitt",
    validAt: "2026-06-21T20:00:00.000Z",
    ...overrides,
  });
}

// create one regional current cloud estimate
function regional(overrides = {}) {
  return weatherRecord({
    cloudCoverPercent: 80,
    id: "regional",
    providerKey: "open-meteo",
    receivedAt: "2026-06-21T19:56:00.000Z",
    sourceKind: "model_current",
    stationSlug: "open-meteo-virtual",
    validAt: "2026-06-21T19:55:00.000Z",
    ...overrides,
  });
}

// create one hourly cloud forecast
function forecast(validAt, cloudCoverPercent, overrides = {}) {
  return weatherRecord({
    cloudCoverPercent,
    id: `forecast-${validAt}`,
    providerKey: "open-meteo",
    receivedAt: "2026-06-21T19:50:00.000Z",
    sourceKind: "forecast",
    stationSlug: "open-meteo-virtual",
    validAt,
    ...overrides,
  });
}

// create one render-complete dashboard state
function dashboardState(overrides = {}) {
  return {
    ...new WeatherDashboardController({ storage: null }).state,
    current: [ws90(), regional()],
    forecast: [forecast(now.toISOString(), 80)],
    forecastAdjustmentSettings: { version: 1, temperature: false, wind: false, rain: false },
    loading: false,
    selectedSite: site,
    units: DEFAULT_UNIT_PREFERENCES,
    ...overrides,
  };
}

// isolate one current condition card
function conditionCard(html, key) {
  const card = html.match(new RegExp(`<article[^>]*data-condition="${key}"[\\s\\S]*?<\\/article>`, "u"))?.[0];
  assert.ok(card);
  return card;
}

// isolate one serialized chart series
function chartSeries(html, key) {
  const marker = `data-forecast-chart="${key}"`;
  const markerIndex = html.indexOf(marker);
  assert.notEqual(markerIndex, -1);
  const start = html.lastIndexOf("<article", markerIndex);
  const end = html.indexOf("</article>", markerIndex);
  const chart = html.slice(start, end + "</article>".length);
  const encoded = chart.match(/data-forecast-series="([^"]+)"/u)?.[1];
  assert.ok(encoded);
  return {
    chart,
    series: JSON.parse(encoded.replaceAll("&quot;", '"').replaceAll("&amp;", "&")),
  };
}

// read one projected record without exposing the private marker
function cloud(record, adjusted = true) {
  return forecastMetricValue(record, "cloudCoverPercent", adjusted);
}

// project exact sources at one deterministic instant
function project(current, forecasts, at = now, mode = "adjusted") {
  return withSolarCloudAdjustment({
    current,
    forecast: forecasts,
    forecastAdjustmentMode: mode,
    selectedSite: site,
  }, at);
}

// apply bright and dim observations across the fixed forecast horizon
test("solar projection changes only trusted ephemeral cloud values and retires safely", () => {
  const current = [ws90(), regional()];
  const forecasts = [
    forecast("2026-06-21T19:59:59.999Z", 50),
    forecast("2026-06-21T20:00:00.000Z", 80),
    forecast("2026-06-22T08:00:00.000Z", 80),
    forecast("2026-06-22T20:00:00.000Z", 80),
    forecast("2026-06-22T21:00:00.000Z", 80),
    forecast("2026-06-21T20:00:00.000Z", 35, { providerKey: "weatherkit" }),
  ];
  const projected = project(current, forecasts);
  assert.equal(cloud(projected.current[1]), 0);
  assert.deepEqual(projected.forecast.map((record) => cloud(record)), [50, 0, 40, 80, 80, 35]);
  assert.deepEqual(projected.forecast.map((record) => cloud(record, false)), [50, 80, 80, 80, 80, 35]);
  assert.equal(current[1].metrics.cloudCoverPercent, 80);
  assert.deepEqual(forecasts.map((record) => record.metrics.cloudCoverPercent), [50, 80, 80, 80, 80, 35]);
  assert.equal(Object.hasOwn(projected.forecast[1], "solarCloudAdjustment"), false);
  assert.equal(JSON.stringify(projected).includes("solar-cloud-experiment"), false);

  const raw = project(projected.current, projected.forecast, now, "raw");
  assert.equal(cloud(raw.current[1]), 80);
  assert.deepEqual(raw.forecast.map((record) => cloud(record)), [50, 80, 80, 80, 80, 35]);
  const repeated = project(projected.current, projected.forecast);
  assert.deepEqual(repeated.forecast.map((record) => cloud(record)), [50, 0, 40, 80, 80, 35]);
  const expired = project(projected.current, projected.forecast, new Date("2026-06-21T20:06:00.000Z"));
  assert.deepEqual(expired.forecast.map((record) => cloud(record)), [50, 80, 80, 80, 80, 35]);

  const dimCurrent = [ws90({ solarRadiationWm2: 100 }), regional({ cloudCoverPercent: 20 })];
  const dim = project(dimCurrent, [forecast(now.toISOString(), 10)]);
  assert.equal(cloud(dim.current[1]) > 99, true);
  assert.equal(cloud(dim.forecast[0]), 90);

  const newSample = project([
    ws90({ id: "new-ws90", solarRadiationWm2: 100 }),
    projected.current[1],
  ], [forecast(now.toISOString(), 10)]);
  assert.equal(cloud(newSample.forecast[0]), 30);

  const spoof = {
    ...forecast(now.toISOString(), 55, { providerKey: "weatherkit" }),
    solarCloudAdjustment: { contractVersion: "solar-cloud-experiment/v1", value: 0 },
  };
  assert.equal(cloud(spoof), 55);
});

// require exact fresh source identities without nearby or temporal fallback
test("solar projection fails raw for ineligible current evidence", () => {
  const target = forecast(now.toISOString(), 50);
  const nearby = ws90({ id: "nearby", stationSlug: "nearby-ecowitt", validAt: "2026-06-21T20:01:00.000Z", receivedAt: "2026-06-21T20:01:00.000Z" });
  const exactWithNearby = project([nearby, ws90(), regional()], [target]);
  assert.notEqual(cloud(exactWithNearby.forecast[0]), 50);

  const night = new Date("2026-06-21T05:00:00.000Z");
  const nightCurrent = [
    ws90({ receivedAt: night.toISOString(), validAt: night.toISOString() }),
    regional({ receivedAt: "2026-06-21T04:56:00.000Z", validAt: "2026-06-21T04:55:00.000Z" }),
  ];
  const invalid = [
    [nearby, regional()],
    [ws90({ providerKey: "ecowitt-cloud" }), regional()],
    [ws90(), regional({ providerKey: "weatherkit" })],
    [ws90({ freshnessStatus: "stale" }), regional()],
    [ws90({ solarRadiationWm2: null }), regional()],
    [ws90({ receivedAt: "2026-06-21T20:01:00.000Z", validAt: "2026-06-21T20:01:00.000Z" }), regional()],
    [ws90(), regional({ receivedAt: "2026-06-21T19:40:00.000Z", validAt: "2026-06-21T19:39:00.000Z" })],
    [ws90(), regional({ cloudCoverPercent: null })],
  ];

  // keep every independent invalid source case unchanged
  for (const current of invalid) {
    assert.equal(cloud(project(current, [target]).forecast[0]), 50);
  }
  assert.equal(cloud(project(nightCurrent, [forecast(night.toISOString(), 50)], night).forecast[0]), 50);
});

// share selected current cloud values with artwork without inventing missing rain
test("current artwork uses experimental cloud only in adjusted mode", () => {
  assert.deepEqual(currentWeatherIcon({
    current: [ws90(), regional()],
    forecastAdjustmentMode: "raw",
    selectedSite: site,
  }, now), { name: "05-cloudy", label: "Cloudy" });
  assert.deepEqual(currentWeatherIcon({
    current: [ws90(), regional()],
    forecastAdjustmentMode: "adjusted",
    selectedSite: site,
  }, now), { name: "01-sunny", label: "Sunny" });

  const unavailable = [
    ws90({ precipitationRateMmPerHour: null }),
    regional({ precipitationRateMmPerHour: null }),
  ];
  assert.equal(currentWeatherIcon({ current: unavailable, forecastAdjustmentMode: "adjusted", selectedSite: site }, now).name, "12-unavailable");
  assert.equal(currentWeatherIcon({ current: unavailable, forecastAdjustmentMode: "raw", selectedSite: site }, now).name, "12-unavailable");
});

// render selected cloud values through cards ranges charts and all-off controls
test("solar projection drives cloud summaries and remains separate from governed settings", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now });
  const forecasts = [
    forecast("2026-06-21T20:00:00.000Z", 80),
    forecast("2026-06-21T21:00:00.000Z", 70),
    forecast("2026-06-21T22:00:00.000Z", 60),
  ];
  const state = dashboardState({ forecast: forecasts });
  const adjustedHome = renderWeatherDashboard(state, "home");
  const rawHome = renderWeatherDashboard({ ...state, forecastAdjustmentMode: "raw" }, "home");
  const adjustedForecast = renderWeatherDashboard(state, "forecast");
  const rawForecast = renderWeatherDashboard({ ...state, forecastAdjustmentMode: "raw" }, "forecast");
  const adjustedCard = conditionCard(adjustedHome, "clouds");
  const rawCard = conditionCard(rawHome, "clouds");
  const adjustedChart = chartSeries(adjustedForecast, "clouds");
  const rawChart = chartSeries(rawForecast, "clouds");

  assert.match(adjustedCard, /class="condition-primary"><strong>0<small>%<\/small>/u);
  assert.match(adjustedCard, /Sunlight estimate/u);
  assert.match(adjustedCard, /forecast-adjusted-icon/u);
  assert.match(adjustedCard, /Clearest[\s\S]*?1–4<small>PM/u);
  assert.match(adjustedCard, /Max[\s\S]*?0<small>%[\s\S]*?Min[\s\S]*?0<small>%/u);
  assert.match(rawCard, /class="condition-primary"><strong>80<small>%<\/small>/u);
  assert.doesNotMatch(rawCard, /experimental|forecast-adjusted-icon/u);
  assert.match(rawCard, /Clearest[\s\S]*?3–4<small>PM/u);
  assert.match(rawCard, /Max[\s\S]*?80<small>%[\s\S]*?Min[\s\S]*?60<small>%/u);
  assert.deepEqual(adjustedChart.series, [{ label: "Cover", values: [0, 0, 0] }]);
  assert.deepEqual(rawChart.series, [{ label: "Cover", values: [80, 70, 60] }]);
  assert.match(adjustedChart.chart, /forecast-adjusted-icon[\s\S]*?Clouds/u);
  assert.doesNotMatch(rawChart.chart, /experimental|forecast-adjusted-icon/u);
  assert.match(adjustedForecast, /data-forecast-adjustment-available="true"/u);
});

// emulate browser storage without exposing implementation state
function memoryStorage() {
  const values = new Map();
  return {
    getItem(key) {
      return values.get(key) ?? null;
    },
    setItem(key, value) {
      values.set(key, value);
    },
  };
}

// defer one response to verify overlap and route invalidation
function deferredResponse() {
  let resolve;
  const promise = new Promise(
    // expose one deterministic request completion
    (complete) => { resolve = complete; },
  );
  return { promise, resolve };
}

// refresh only current readings while preserving raw cache inputs and route generations
test("current polling deduplicates failures and late route responses", async (context) => {
  context.mock.timers.enable({ apis: ["Date"], now });
  const storage = memoryStorage();
  let currentRecords = [ws90(), regional()];
  let currentReads = 0;
  let pending = null;
  let failCurrent = false;
  const requested = [];

  // serve one controllable forecast-page api
  async function fetcher(input) {
    const url = String(input);
    requested.push(url);
    // isolate current refresh outcomes
    if (url.includes("/current")) {
      currentReads += 1;
      // hold one selected current response
      if (pending !== null) {
        return pending.promise;
      }
      // fail one selected current response
      if (failCurrent) {
        return Response.json({ error: "unavailable" }, { status: 503 });
      }
      return Response.json({ data: currentRecords, site });
    }
    // return one valid forecast collection
    if (url.includes("/forecast")) {
      return Response.json({ data: [forecast(now.toISOString(), 80)], site });
    }
    // preserve an empty tide collection
    if (url.includes("/tides")) {
      return Response.json({ data: [], generatedAt: now.toISOString(), site });
    }
    return Response.json({ data: [], page: { limit: 100, nextCursor: null }, site });
  }

  const controller = new WeatherDashboardController({ fetcher, storage, view: "forecast" });
  await controller.initialize();
  assert.equal(currentReads, 1);
  assert.equal(JSON.parse(storage.getItem(NOW_ICON_STORAGE_KEY)).cloud, 80);
  assert.equal(currentWeatherIcon(controller.state, now).name, "01-sunny");

  requested.length = 0;
  pending = deferredResponse();
  currentRecords = [ws90({ id: "ws90-refresh" }), regional({ cloudCoverPercent: 90, id: "regional-refresh" })];
  const first = controller.refreshCurrentReadings();
  const second = controller.refreshCurrentReadings();
  assert.equal(currentReads, 2);
  assert.equal(requested.filter((url) => url.includes("/current")).length, 1);
  assert.equal(requested.some((url) => url.includes("/forecast") || url.includes("/tides")), false);
  pending.resolve(Response.json({ data: currentRecords, site }));
  pending = null;
  await Promise.all([first, second]);
  assert.equal(JSON.parse(storage.getItem(NOW_ICON_STORAGE_KEY)).cloud, 90);
  assert.equal(currentWeatherIcon(controller.state, now).name, "01-sunny");

  const retained = controller.state.current;
  let emissions = 0;
  const unsubscribe = controller.subscribe(() => { emissions += 1; });
  context.mock.timers.setTime(new Date("2026-06-21T20:06:00.000Z").getTime());
  failCurrent = true;
  await controller.refreshCurrentReadings();
  assert.equal(controller.state.current, retained);
  assert.equal(emissions >= 3, true);
  assert.equal(currentWeatherIcon(controller.state, new Date()).name, "05-cloudy");
  unsubscribe();

  failCurrent = false;
  context.mock.timers.setTime(now.getTime());
  pending = deferredResponse();
  const late = controller.refreshCurrentReadings();
  await controller.setView("logs");
  pending.resolve(Response.json({ data: [ws90({ id: "late-ws90" }), regional({ cloudCoverPercent: 5, id: "late-regional" })], site }));
  pending = null;
  await late;
  assert.equal(controller.view, "logs");
  assert.equal(controller.state.current, retained);
});
