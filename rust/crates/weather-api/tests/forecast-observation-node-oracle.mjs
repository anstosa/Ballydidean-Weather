import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";

const dataAt = "2026-08-22T05:00:00.000Z";
const safetyAt = "2026-08-23T05:00:00.000Z";
let clockReads = 0;
let inputReads = 0;
const handler = createWeatherApi({
  // return one fixed compact private projection input
  async getForecastObservationInput(asOf) {
    inputReads += 1;
    assert.equal(asOf, dataAt);
    return {
      projectionFrom: "2026-08-21T07:00:00.000Z",
      queryFrom: "2026-08-21T04:00:00.000Z",
      records: [{
        apparentTemperatureC: 9,
        cloudCoverPercent: 20,
        id: "100",
        lastReceivedAt: "2026-08-22T04:00:00.000Z",
        pm25MicrogramsPerCubicMeter: null,
        precipitationRateMmPerHour: 0,
        pressureHpa: 1010,
        providerKey: "ecowitt-local",
        relativeHumidityPercent: 80,
        solarRadiationWm2: 0,
        sourceId: "1",
        sourceKey: "ecowitt-local-live-v1",
        sourceKind: "physical_sensor",
        stationSlug: "ballydidean-ecowitt",
        temperatureC: 10,
        uvIndex: 0,
        validAt: "2026-08-22T04:00:00.000Z",
        windDirectionDegrees: 90,
        windGustMps: 3,
        windSpeedMps: 2,
      }],
      siteSlug: "ballydidean",
      through: dataAt,
      timezone: "America/Los_Angeles",
    };
  },
  // prove the fixed private input bypasses site discovery
  async listSites() {
    assert.fail("private forecast observation input must not discover sites");
  },
}, {
  dataAt,
  forecastAdjustment: {
    loadedAt: dataAt,
    runtime: { bundle: null, reasonCode: "registry_inactive", state: "disabled" },
  },
  temperatureAdjustment: {
    loadedAt: dataAt,
    runtime: { bundle: null, reasonCode: "registry_inactive", state: "disabled" },
  },
  // preserve an independent request safety clock
  now() {
    clockReads += 1;
    return new Date(safetyAt);
  },
});

const response = await handler(new Request(
  "http://weather.test/internal/v1/forecast-observation-input",
));
const body = await response.json();
const head = await handler(new Request(
  "http://weather.test/internal/v1/forecast-observation-input",
  { method: "HEAD" },
));
const invalid = await handler(new Request(
  "http://weather.test/internal/v1/forecast-observation-input?from=x",
));
const method = await handler(new Request(
  "http://weather.test/internal/v1/forecast-observation-input",
  { method: "POST" },
));

assert.deepEqual(
  [response.status, head.status, invalid.status, method.status],
  [200, 200, 400, 405],
);
assert.equal((await head.arrayBuffer()).byteLength, 0);
assert.equal(clockReads, 4);
assert.equal(inputReads, 2);
assert.equal(body.contractVersion, "forecast-observation-input/v1");
assert.equal(body.generatedAt, dataAt);
assert.equal(body.projectionFrom, "2026-08-21T07:00:00.000Z");
assert.equal(body.queryFrom, "2026-08-21T04:00:00.000Z");
assert.equal(body.through, dataAt);
assert.deepEqual(Object.keys(body.records[0]).sort(), [
  "id", "metrics", "provenance", "receivedAt", "validAt",
]);
assert.equal(JSON.stringify(body).includes("metadata"), false);

process.stdout.write("private forecast observation route Node oracle passed\n");
