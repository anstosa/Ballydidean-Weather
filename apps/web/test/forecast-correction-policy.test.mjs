import assert from "node:assert/strict";
import test from "node:test";
import { createLocalForecastBias, projectLocalForecastBias, projectForecastLocalBias } from "../dist/local-forecast-bias.js";
import { createSolarCloudBias, projectSolarCloudCover, projectForecastCloudCover, radiationCloudProxy } from "../dist/solar-cloud.js";
import { DEFAULT_FORECAST_CORRECTION_POLICIES, forecastCorrectionWeight } from "../dist/forecast-correction-policy.js";
import { forecastPressureChanges, forecastPressureSeries } from "../dist/index.js";

const at = "2026-06-21T20:00:00.000Z";
const site = { latitude: 47.95043, longitude: -122.42797 };
const policy = { horizonHours: 6, strength: 0.5 };

// create one causal explicit scalar sample
function sample(value, sourceId = "farm") {
  return { value, sourceId, recordId: sourceId, validAt: at, receivedAt: at, freshnessStatus: "fresh" };
}

// preserve legacy values while shortened policies use one observation-age taper
test("forecast-only scalar policies preserve legacy projectors and immutable expiry", () => {
  const bias = createLocalForecastBias({ ...site, metric: "uvIndex", now: at, observed: sample(3), regional: sample(6, "regional") });
  assert.ok(bias);
  assert.equal(projectLocalForecastBias(6, "2026-06-21T23:00:00.000Z", bias), 3.375);
  assert.equal(projectForecastLocalBias(6, "2026-06-21T23:00:00.000Z", bias, policy), 5.25);
  assert.equal(projectForecastLocalBias(6, "2026-06-22T02:00:00.000Z", bias, policy), 6);
  assert.equal(projectForecastLocalBias(6, at, bias, { ...policy, strength: 0 }), 6);
  assert.equal(projectForecastLocalBias(6, at, { ...bias, expiresAt: "2026-06-22T02:00:00.000Z" }, policy), 6);
  assert.equal(projectForecastLocalBias(6, at, { ...bias, adjustmentValue: 1.5 }, policy), 6);
  const humidity = createLocalForecastBias({ ...site, metric: "relativeHumidityPercent", now: at, observed: sample(70), regional: sample(50, "regional") });
  assert.equal(projectLocalForecastBias(50, at, humidity), 70);
  assert.equal(projectForecastLocalBias(50, at, humidity, policy), 50);
});

// freeze cloud targets without changing current cloud projection
test("cloud forecast policy and pointwise radiation proxy keep current behavior", () => {
  const bias = createSolarCloudBias({ ...site, now: at, ws90: sample(800), regional: sample(80, "regional") });
  assert.ok(bias);
  assert.equal(radiationCloudProxy(site.latitude, site.longitude, at, 800), bias.estimatedCloudCoverPercent);
  assert.equal(projectSolarCloudCover(80, at, bias), 0);
  assert.equal(projectSolarCloudCover(80, "2026-06-21T23:00:00.000Z", bias), 10);
  assert.equal(projectForecastCloudCover(80, "2026-06-21T23:00:00.000Z", bias, policy), 60);
  assert.equal(projectForecastCloudCover(80, at, bias, DEFAULT_FORECAST_CORRECTION_POLICIES.cloudCoverPercent.ws90), 80);
  assert.equal(radiationCloudProxy(site.latitude, site.longitude, "2026-06-21T05:00:00.000Z", 800), null);
  assert.equal(radiationCloudProxy(site.latitude, site.longitude, at, 2501), null);
});

// refuse unreviewed strength and horizon values before projection arithmetic
test("policy grid defaults to regional and cannot renew observation age", () => {
  assert.ok(Object.isFrozen(DEFAULT_FORECAST_CORRECTION_POLICIES));
  assert.equal(forecastCorrectionWeight(Date.parse(at), Date.parse(at), policy), 0.5);
  assert.equal(forecastCorrectionWeight(Date.parse(at) + 3 * 3_600_000, Date.parse(at), policy), 0.25);
  // every unsupported policy or time is an exact no-op
  for (const invalid of [null, {}, { strength: 2, horizonHours: 6 }, { strength: 0.5, horizonHours: 25 }, { strength: 0.1, horizonHours: 6 }]) {
    assert.equal(forecastCorrectionWeight(Date.parse(at), Date.parse(at), invalid), 0);
  }
  assert.equal(forecastCorrectionWeight(Date.parse(at) - 1, Date.parse(at), policy), 0);
  assert.equal(forecastCorrectionWeight(NaN, Date.parse(at), policy), 0);
});

// create a complete isolated pressure window without normalized ui fields
function pressureRecord(hour, run, pressure = 1000 + hour) {
  return { id: `${run}/${hour}`, validAt: new Date(Date.parse(at) + hour * 3_600_000).toISOString(), productRunAt: run,
    metrics: { pressureHpa: pressure }, provenance: { sourceId: "regional", sourceKind: "forecast" } };
}

// reuse isolated context differences instead of mixing old and new absolute pressures
test("shared pressure series reproduces first-three-hour retained context exactly", () => {
  const context = [-3, -2, -1, 0, 1, 2].map(
    // retain all six endpoints within one earlier retrieval vintage
    (hour) => pressureRecord(hour, "older", 900 + 2 * hour),
  );
  const visible = [0, 1, 2, 3, 4].map(
    // a different absolute datum cannot contaminate context differences
    (hour) => pressureRecord(hour, "newer"),
  );
  assert.deepEqual(forecastPressureChanges(visible), [null, null, null, 3, 3]);
  assert.deepEqual(forecastPressureSeries(visible, visible, context), [6, 6, 6, 3, 3]);
  assert.deepEqual(forecastPressureSeries(visible, visible, []), [null, null, null, 3, 3]);
  assert.deepEqual(forecastPressureSeries(visible, visible, context.slice(1)), [null, null, null, 3, 3]);
  const wrongSource = context.map(
    // reject context from a different physical model source
    (record) => ({ ...record, provenance: { ...record.provenance, sourceId: "other" } }),
  );
  assert.deepEqual(forecastPressureSeries(visible, visible, wrongSource), [null, null, null, 3, 3]);
});
