import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_UNIT_PREFERENCES,
  forecastMetricValue,
  forecastPressureChanges,
  renderWeatherDashboard,
  strongestPressureChange,
  WeatherDashboardController,
  withLocalForecastAdjustments,
  withSolarCloudAdjustment,
} from "../dist/index.js";

const now = new Date("2026-06-21T20:00:00.000Z");
const site = {
  latitude: 47.950429954185445,
  longitude: -122.42797012608193,
  name: "Ballydídean", slug: "ballydidean", stations: [], timezone: "America/Los_Angeles",
};

// create one complete normalized record without serialized projection metadata
function record(kind, id, validAt, metrics = {}, overrides = {}) {
  return {
    freshness: { ageSeconds: 0, label: "Current", status: "fresh" },
    id,
    metadata: {
      device: null,
      provider: { dataset: "forecast", elevationM: 28, gridCell: null, propertySensors: null },
      quality: null, upstream: { model: "best_match", timezone: site.timezone },
    },
    metrics: {
      apparentTemperatureC: 12, blackGlobeTemperatureC: null, cloudCoverPercent: 80,
      pm25MicrogramsPerCubicMeter: 20, precipitationMm: 0, precipitationRateMmPerHour: 0,
      pressureHpa: 1000, relativeHumidityPercent: 50, soilElectricalConductivityMicrosiemensPerCm: null,
      soilMoisturePercent: null, solarRadiationWm2: null, temperatureC: 14, uvIndex: 6,
      windDirectionDegrees: 180, windGustMps: 2, windSpeedMps: 1, wetBulbGlobeTemperatureC: null,
      ...metrics,
    },
    pressureChange3hHpa: kind === "model_current" ? -0.5 : null,
    productRunAt: kind === "forecast" ? "2026-06-21T19:50:00.000Z" : null,
    provenance: {
      attribution: { label: "Weather", url: "https://example.test" }, label: kind,
      providerKey: kind === "physical_sensor" ? "ecowitt-local" : "open-meteo",
      sourceId: kind, sourceKey: kind, sourceKind: kind,
      stationSlug: kind === "physical_sensor" ? "ballydidean-ecowitt" : "open-meteo-virtual",
    },
    receivedAt: kind === "forecast" ? "2026-06-21T19:50:00.000Z" : validAt,
    revisionCount: 0, validAt, ...overrides,
  };
}

// construct a farm with missing particulate readings and two independent nearby sensors
function state(overrides = {}) {
  const farm = record("physical_sensor", "farm", now.toISOString(), {
    relativeHumidityPercent: 70, pm25MicrogramsPerCubicMeter: null, uvIndex: 3,
    pressureHpa: 1025, solarRadiationWm2: 800,
  }, { pressureChange3hHpa: 1 });
  const regional = record("model_current", "regional", "2026-06-21T19:45:00.000Z");
  // maintain one continuous same-source and same-run pressure timeline
  const forecast = Array.from({ length: 40 }, (_, index) => record(
    "forecast", `hour-${index}`, new Date(now.getTime() + (index - 3) * 3_600_000).toISOString(),
    { pressureHpa: 1000 + index / 10, uvIndex: index === 15 ? 0 : 6 },
    { temperatureAdjustment: { state: "active", correctedTemperatureC: 10 } },
  ));
  // retain distinct stations while testing duplicate provider weighting
  const nearby = (id, concentration, station) => record("physical_sensor", id, now.toISOString(), {
    pm25MicrogramsPerCubicMeter: concentration,
  }, {
    provenance: { ...farm.provenance, providerKey: "purpleair", sourceId: id, sourceKey: id, stationSlug: station },
  });
  return {
    ...new WeatherDashboardController({ storage: null }).state,
    current: [farm, regional, nearby("neighbor-a", 8, "station-a"), nearby("neighbor-b", 12, "station-b")],
    forecast, forecastDays: 5, loading: false, selectedSite: site, units: DEFAULT_UNIT_PREFERENCES,
    ...overrides,
  };
}

// compare scalar projections without relying on decimal rounding artifacts
function close(actual, expected) {
  assert.ok(Math.abs(actual - expected) < 1e-9, `${actual} != ${expected}`);
}

// read the actual serialized chart values rendered for one family
function chart(html, key) {
  const index = html.indexOf(`data-forecast-chart="${key}"`);
  assert.notEqual(index, -1);
  const start = html.lastIndexOf("<article", index);
  const end = html.indexOf("</article>", index);
  const markup = html.slice(start, end + 10);
  const encoded = markup.match(/data-forecast-series="([^"]+)"/u)?.[1];
  assert.ok(encoded);
  return { markup, series: JSON.parse(encoded.replaceAll("&quot;", '"').replaceAll("&amp;", "&")) };
}

// prove every accepted family fades forward without mutating regional data or absolute pressure
test("local biases reach forecast values and same-source pressure trends for only 24 hours", () => {
  const raw = state();
  const before = JSON.stringify(raw);
  const adjusted = withLocalForecastAdjustments(raw, now);
  const currentHour = adjusted.forecast[3];
  close(forecastMetricValue(currentHour, "relativeHumidityPercent"), 70);
  close(forecastMetricValue(currentHour, "pm25MicrogramsPerCubicMeter"), 10);
  close(forecastMetricValue(currentHour, "uvIndex"), 3);
  close(forecastPressureChanges(adjusted.forecast, [currentHour], true)[0], 1.8);
  close(forecastMetricValue(currentHour, "pressureHpa"), currentHour.metrics.pressureHpa);
  close(forecastMetricValue(adjusted.forecast[15], "relativeHumidityPercent"), 60);
  close(forecastMetricValue(adjusted.forecast[15], "pm25MicrogramsPerCubicMeter"), 15);
  assert.equal(forecastMetricValue(adjusted.forecast[15], "uvIndex"), 0);
  close(forecastPressureChanges(adjusted.forecast, [adjusted.forecast[15]], true)[0], 1.05);

  // preserve past hours raw mode and the exact expiry boundary for every metric
  for (const index of [0, 2, 27, 39]) {
    for (const metric of ["relativeHumidityPercent", "pm25MicrogramsPerCubicMeter", "uvIndex"]) {
      assert.equal(forecastMetricValue(adjusted.forecast[index], metric), raw.forecast[index].metrics[metric]);
    }
  }
  assert.equal(JSON.stringify(raw), before);
  assert.equal(forecastMetricValue(currentHour, "relativeHumidityPercent", false), 50);
  assert.equal(forecastMetricValue(currentHour, "pm25MicrogramsPerCubicMeter", false), 20);
  assert.equal(forecastMetricValue(currentHour, "uvIndex", false), 6);
  close(forecastPressureChanges(adjusted.forecast, [currentHour], false)[0], 0.3);
  assert.equal(JSON.stringify(adjusted).includes("local-forecast-bias-experiment"), false);
  const restored = withLocalForecastAdjustments({ ...adjusted, forecastAdjustmentMode: "raw" }, now);
  assert.equal(forecastMetricValue(restored.forecast[3], "relativeHumidityPercent"), 50);
  const repeated = withLocalForecastAdjustments(adjusted, now);
  assert.equal(forecastMetricValue(repeated.forecast[3], "relativeHumidityPercent"), 70);
});

// prove current-hour forecast products remain valid across their real publication cadence
test("hourly UV and particulate baselines accept pre-valid publication and delayed hourly labels", () => {
  const input = state();
  input.forecast = input.forecast.map(
    // every target must retain the same explicitly published forecast vintage
    (item) => ({ ...item, productRunAt: "2026-06-21T19:20:00.000Z", receivedAt: "2026-06-21T19:20:00.000Z" }),
  );
  input.forecast[3] = {
    ...input.forecast[3], freshness: { ...input.forecast[3].freshness, status: "delayed" },
  };
  const adjusted = withLocalForecastAdjustments(input, now);
  assert.equal(forecastMetricValue(adjusted.forecast[4], "pm25MicrogramsPerCubicMeter") < 20, true);
  assert.equal(forecastMetricValue(adjusted.forecast[4], "uvIndex") < 6, true);
});

// retire stale or unavailable inputs independently instead of disabling working families
test("local models fail raw on stale sources missing hours wrong stations and incompatible products", () => {
  const input = state();
  const stale = withLocalForecastAdjustments(input, new Date(now.getTime() + 16 * 60_000));
  assert.equal(forecastMetricValue(stale.forecast[4], "relativeHumidityPercent"), 50);
  assert.equal(forecastMetricValue(stale.forecast[4], "pm25MicrogramsPerCubicMeter"), 20);
  const missingHour = withLocalForecastAdjustments({ ...input, forecast: input.forecast.filter((_, index) => index !== 3) }, now);
  assert.equal(forecastMetricValue(missingHour.forecast[3], "relativeHumidityPercent") > 50, true);
  assert.equal(forecastMetricValue(missingHour.forecast[3], "pm25MicrogramsPerCubicMeter"), 20);
  assert.equal(forecastMetricValue(missingHour.forecast[3], "uvIndex"), 6);
  const unsupported = { ...input.forecast[4], metadata: { ...input.forecast[4].metadata, upstream: { model: "ecmwf_ifs", timezone: site.timezone } } };
  const futurePublication = { ...input.forecast[5], receivedAt: "2026-06-21T20:01:00.000Z" };
  const wrongStation = { ...input.forecast[6], provenance: { ...input.forecast[6].provenance, stationSlug: "elsewhere" } };
  const wrongSource = { ...input.forecast[7], provenance: { ...input.forecast[7].provenance, sourceId: "foreign-model" } };
  const olderRun = { ...input.forecast[8], productRunAt: "2026-06-21T19:40:00.000Z" };
  const projected = withLocalForecastAdjustments({ ...input, forecast: [input.forecast[3], unsupported, futurePublication, wrongStation, wrongSource, olderRun] }, now);
  // never apply the farm estimate to a different product identity or unreceived forecast
  for (const item of projected.forecast.slice(1)) {
    assert.equal(forecastMetricValue(item, "relativeHumidityPercent"), 50);
  }
  const withoutHour = withLocalForecastAdjustments({ ...input, forecast: [input.forecast[4], olderRun] }, now);
  assert.equal(forecastMetricValue(withoutHour.forecast[0], "relativeHumidityPercent") > 50, true);
  assert.equal(forecastMetricValue(withoutHour.forecast[1], "relativeHumidityPercent"), 50);
});

// retain farm precedence station weighting qualified model precedence and missing pressure windows
test("local projection preserves per-metric source priority and qualified model output", () => {
  const input = state();
  const duplicate = { ...input.current[2], id: "duplicate", receivedAt: "2026-06-21T19:59:00.000Z", metrics: { ...input.current[2].metrics, pm25MicrogramsPerCubicMeter: 100 } };
  const averaged = withLocalForecastAdjustments({ ...input, current: [...input.current, duplicate] }, now);
  assert.equal(forecastMetricValue(averaged.forecast[3], "pm25MicrogramsPerCubicMeter"), 10);
  const farm = { ...input.current[0], metrics: { ...input.current[0].metrics, pm25MicrogramsPerCubicMeter: 4 } };
  const preferred = withLocalForecastAdjustments({ ...input, current: [farm, ...input.current.slice(1)] }, now);
  assert.equal(forecastMetricValue(preferred.forecast[3], "pm25MicrogramsPerCubicMeter"), 4);
  const qualified = { ...input.forecast[3], adjustment: { state: "active", appliedMetrics: ["relativeHumidityPercent"], adjustedMetrics: { relativeHumidityPercent: 85 } } };
  const governed = withLocalForecastAdjustments({ ...input, forecast: [qualified] }, now);
  assert.equal(forecastMetricValue(governed.forecast[0], "relativeHumidityPercent"), 85);
  const gap = averaged.forecast.filter((_, index) => index !== 2);
  assert.equal(forecastPressureChanges(gap, [averaged.forecast[3]], true)[0], null);
  const wrongRun = [averaged.forecast[0], averaged.forecast[1], { ...averaged.forecast[2], productRunAt: "2026-06-21T19:49:00.000Z" }, averaged.forecast[3]];
  assert.equal(forecastPressureChanges(wrongRun, [averaged.forecast[3]], true)[0], null);
});

// prove homepage summaries charts and independent sunlight projections consume the same corrections
test("rendered local summaries and air temperature chart match adjusted values", (context) => {
  context.mock.timers.enable({ apis: ["Date"], now });
  const input = state();
  const combined = withLocalForecastAdjustments(withSolarCloudAdjustment(input, now), now);
  assert.equal(forecastMetricValue(combined.forecast[3], "cloudCoverPercent"), 0);
  assert.equal(forecastMetricValue(combined.forecast[3], "relativeHumidityPercent"), 70);
  const adjustedHtml = renderWeatherDashboard(input, "forecast");
  const rawHtml = renderWeatherDashboard({ ...input, forecastAdjustmentMode: "raw" }, "forecast");
  const temperature = chart(adjustedHtml, "temperature").series;
  assert.deepEqual(temperature.map((series) => series.label), ["Feels like", "Air Temp"]);
  assert.equal(temperature[0].values[3], 12);
  assert.equal(temperature[1].values[3], 10);
  assert.equal(chart(rawHtml, "temperature").series[1].values[3], 14);
  // verify each forecast chart's actual projected values rather than its gold class alone
  for (const [key, expected] of [["humidity", 70], ["air-quality", 10], ["uv-index", 3], ["pressure", 1.8]]) {
    close(chart(adjustedHtml, key).series[0].values[3], expected);
    assert.doesNotMatch(chart(adjustedHtml, key).markup, /experimental/u);
    assert.doesNotMatch(chart(rawHtml, key).markup, /experimental/u);
  }
  const correctedMax = strongestPressureChange(combined.forecast, now, site.timezone, true);
  assert.ok(correctedMax);
  const home = renderWeatherDashboard(input, "home");
  assert.match(home, /data-condition="humidity"[\s\S]*?Max[\s\S]*?70/u);
  assert.match(home, /data-condition="pressure"[\s\S]*?Max[\s\S]*?\+1\.8/u);
  const serialized = JSON.parse(JSON.stringify(combined));
  assert.equal(forecastMetricValue(serialized.forecast[3], "relativeHumidityPercent"), 50);
  const qualified = { ...input.forecast[3], adjustment: { state: "active", appliedMetrics: ["relativeHumidityPercent"], adjustedMetrics: { relativeHumidityPercent: 85 } } };
  const governed = chart(renderWeatherDashboard({ ...input, forecast: [qualified] }, "forecast"), "humidity");
  assert.equal(governed.series[0].values[0], 85);
  assert.doesNotMatch(governed.markup, /experimental/u);
});

// retire each family's marker independently when observation clocks differ near the final hour
test("local projections expire independently without changing clean chart labels", (context) => {
  const later = new Date(now.getTime() + 5 * 60_000);
  context.mock.timers.enable({ apis: ["Date"], now: later });
  const input = state();
  const current = input.current.map(
    // fresh particulate observations outlive the farm's earlier humidity reading by five minutes
    (item, index) => index < 2 ? item : { ...item, validAt: later.toISOString(), receivedAt: later.toISOString() },
  );
  const forecast = [input.forecast[3], input.forecast[27]];
  const adjusted = withLocalForecastAdjustments({ ...input, current, forecast }, later);
  assert.equal(forecastMetricValue(adjusted.forecast[1], "relativeHumidityPercent"), 50);
  assert.ok(forecastMetricValue(adjusted.forecast[1], "pm25MicrogramsPerCubicMeter") < 20);
  const html = renderWeatherDashboard({ ...input, current, forecast }, "forecast");
  assert.doesNotMatch(chart(html, "humidity").markup, /experimental/u);
  assert.doesNotMatch(chart(html, "air-quality").markup, /experimental/u);
});
