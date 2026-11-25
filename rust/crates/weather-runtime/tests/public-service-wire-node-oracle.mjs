import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import { createWeatherApi, siteForecastDayWindow } from "../../../../apps/api/dist/index.js";

const parent = process.argv[2];
const dataAt = "2026-08-22T05:00:00.000Z";
const forecastValid = "2026-08-22T06:00:00.000Z";
const received = "2026-08-21T07:00:00.000Z";
const site = { attributionLabel: "Weather data by Open-Meteo", attributionUrl: "https://open-meteo.com/",
  latitude: 47.95, longitude: -122.42, providerKey: "open-meteo", providerName: "Open-Meteo",
  siteName: "Ballydidean", siteSlug: "ballydidean", sourceId: "12", sourceKey: "open-meteo-forecast-v4",
  sourceKind: "forecast", stationKind: "virtual", stationLatitude: 47.96, stationLongitude: -122.43,
  stationName: "Open-Meteo Forecast", stationSlug: "open-meteo-forecast", timezone: "America/Los_Angeles" };
const gauge = { ...site, attributionLabel: "Gauge", attributionUrl: "https://example.test/gauge", providerKey: "gauge",
  providerName: "Gauge", sourceId: "13", sourceKey: "gauge-source", sourceKind: "physical_sensor",
  stationKind: "physical", stationName: "Gauge station", stationSlug: "gauge-station" };

// exercise full metrics, nested provider/device/quality, and explicit null fields through the original service
const row = { adapterVersion: "open-meteo-forecast-daily/v4", apparentTemperatureC: 11.7,
  blackGlobeTemperatureC: null, cloudCoverPercent: 72, contentHash: "sha256-fixture", contractEpoch: "v4",
  deviceModel: "model-x", deviceSerial: null, deviceVendor: "vendor-x", firstReceivedAt: received,
  id: "forecast-fixture", lastReceivedAt: received, pm25MicrogramsPerCubicMeter: 5,
  precipitationMm: 0.1, precipitationRateMmPerHour: 0.1, pressureHpa: 1018.1, productRunAt: received,
  providerKey: "open-meteo", providerMetadata: { dataset: "forecast", elevation_m: 15.5, grid_cell: "cell-1" },
  qualityMetadata: { confidence_percent: 95, flags: ["fixture"], status: "ready" }, relativeHumidityPercent: 83,
  revisionCount: 2, siteSlug: site.siteSlug, sourceConfigFingerprint: "fingerprint", sourceId: site.sourceId,
  sourceKey: site.sourceKey, sourceKind: site.sourceKind, stationSlug: site.stationSlug,
  soilElectricalConductivityMicrosiemensPerCm: null, soilMoisturePercent: null, solarRadiationWm2: 120,
  temperatureC: 12.1, upstreamModel: "best_match", upstreamTimezone: "GMT", uvIndex: 0.2,
  validAt: forecastValid, waterLevelM: null, wetBulbGlobeTemperatureC: null,
  windDirectionDegrees: 180, windGustMps: 3.1, windSpeedMps: 1.8 };
const projectionFrom = siteForecastDayWindow(dataAt, site.timezone).asOf;
const queryFrom = new Date(Math.min(Date.parse(projectionFrom)-3*3_600_000,Date.parse(dataAt)-24*3_600_000-20*60_000)).toISOString();

const handler = createWeatherApi({
  async listSites() { return [site, gauge]; },
  async getCurrent() { return [{ ...row, pm25HourlyConcentrations: null, pm25HourlyBySensor: null,
    pm25HourlyThrough: dataAt, pressureChange3hHpa: 1.5 }]; },
  async listHistory() { return [row]; },
  async getForecast() { return [row]; },
  async getForecastPressureContext() { return []; },
  async getDailyPrecipitation() { return { accumulationMm: 2.54, sourceId: gauge.sourceId,
    stationSlug: gauge.stationSlug, validThrough: dataAt }; },
  async listTides() { return [{ attributionLabel: "NOAA", attributionUrl: "https://example.test/tides",
    predictionType: "H", providerKey: "noaa", sourceId: "42", sourceKind: "tide_prediction",
    stationName: "Tide station", stationSlug: "tide-station", validAt: forecastValid, waterLevelM: 3.271 }]; },
  async listTrends() { return [{ apparentTemperatureC: 10.1, pm25MicrogramsPerCubicMeter: 12.3,
    precipitationMm: 1.2, pressureHpa: 1012.4, relativeHumidityPercent: 82, temperatureC: 11.5,
    temperatureMaximumC: 19.4, temperatureMinimumC: 8.2, validAt: new Date(received),
    windDirectionDegrees: -135, windGustMps: 7.2, windSpeedMps: 4.1 }]; },
  async getHealth() { return { database: "ready", migration: { status: "current", version: "fixture-migration" }, workerLastLoopAt: dataAt }; },
  async getRainCollectionStatus() { return { claims: 12, receipts: 11, validForecasts: 3, timelyForecasts: 2,
    validStationWindows: 7, stationsSeen: 4, failedRequests: 1, pendingRequests: 2, unknownRequests: 1,
    compressedBytes: 4096, lastClaimAt: dataAt, lastReceiptAt: dataAt, lastForecastReceiptAt: null,
    lastStationReceiptAt: null, pausedUntil: null }; },
  async getForecastObservationInput() { return { siteSlug: site.siteSlug, timezone: site.timezone, projectionFrom,
    queryFrom, through: dataAt, records: [{ ...row, sourceId: gauge.sourceId, sourceKind: gauge.sourceKind,
      sourceKey: gauge.sourceKey, providerKey: gauge.providerKey, stationSlug: gauge.stationSlug, validAt: dataAt }] }; },
}, { dataAt, now: () => new Date(dataAt), version: "wire-fixture" });

const base = "/api/v1/sites/ballydidean";
const routes = [ ["sites", "/api/v1/sites"], ["current", `${base}/current?source=12`],
  ["history", `${base}/history?limit=10`], ["forecast", `${base}/forecast?days=1`],
  ["precipitation", `${base}/daily-precipitation`], ["tides", `${base}/tides`], ["trends", `${base}/trends`],
  ["rain", `${base}/rain-collection`], ["health", "/api/v1/health"],
  ["observation", "/internal/v1/forecast-observation-input"], ["error", "/api/v1/missing"] ];
// capture exact JSON.stringify response bytes, not a reserialized approximation
for (const [name, path] of routes) {
  const response = await handler(new Request(`http://weather.test${path}`));
  const body = await response.text();
  assert.equal(response.status, name === "error" ? 404 : 200, `${name}: ${body}`);
  await writeFile(join(parent, `${name}.json`), body);
}
