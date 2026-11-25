import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";

const dataAt = "2026-08-22T05:00:00.000Z";
const source = {
  attributionLabel: "Weather data by Ecowitt",
  attributionUrl: "https://www.ecowitt.com/",
  latitude: 47.95,
  longitude: -122.42,
  providerKey: "ecowitt-local",
  providerName: "Ecowitt",
  siteName: "Ballydidean",
  siteSlug: "ballydidean",
  sourceId: "13",
  sourceKey: "ecowitt-rain",
  sourceKind: "physical_sensor",
  stationKind: "physical",
  stationLatitude: 47.951,
  stationLongitude: -122.421,
  stationName: "Ballydidean Ecowitt",
  stationSlug: "ballydidean-ecowitt",
  timezone: "America/Los_Angeles",
};

// route one nullable repository row through the compiled API
async function readDailyPrecipitation(row) {
  const queries = [];
  const handler = createWeatherApi({
    // capture the resolved local-day query
    async getDailyPrecipitation(siteSlug, from, to) {
      queries.push({ from, siteSlug, to });
      return row;
    },
    // return one active physical source
    async listSites() {
      return [source];
    },
  }, {
    dataAt,
    now: () => new Date(dataAt),
  });
  const response = await handler(new Request(
    "http://weather.test/api/v1/sites/ballydidean/daily-precipitation",
  ));
  const body = await response.json();
  assert.equal(response.status, 200);
  assert.deepEqual(queries, [{
    from: "2026-08-21T07:00:00.000Z",
    siteSlug: "ballydidean",
    to: dataAt,
  }]);
  return body;
}

const present = await readDailyPrecipitation({
  accumulationMm: 2.54,
  sourceId: "13",
  stationSlug: "ballydidean-ecowitt",
  validThrough: "2026-08-22T04:59:00.000Z",
});
assert.deepEqual(present.data, {
  accumulationMm: 2.54,
  source: {
    sourceId: "13",
    stationSlug: "ballydidean-ecowitt",
  },
  validThrough: "2026-08-22T04:59:00.000Z",
});
assert.equal(present.generatedAt, dataAt);
assert.equal(present.site.slug, "ballydidean");

const absent = await readDailyPrecipitation(null);
assert.equal(absent.data, null);
assert.equal(absent.generatedAt, dataAt);

process.stdout.write("daily precipitation Node query/projection oracle passed\n");
