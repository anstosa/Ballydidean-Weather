import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";

const dataAt = "2026-08-22T05:00:00.000Z";
const source = {
  attributionLabel: "Weather data by Open-Meteo",
  attributionUrl: "https://open-meteo.com/",
  latitude: 47.95,
  longitude: -122.42,
  providerKey: "open-meteo",
  providerName: "Open-Meteo",
  siteName: "Ballydidean",
  siteSlug: "ballydidean",
  sourceId: "12",
  sourceKey: "open-meteo-forecast-v4",
  sourceKind: "forecast",
  stationKind: "virtual",
  stationLatitude: 47.95,
  stationLongitude: -122.42,
  stationName: "Open-Meteo Virtual",
  stationSlug: "open-meteo-virtual",
  timezone: "America/Los_Angeles",
};
const reads = [];
const handler = createWeatherApi({
  // capture each resolved forecast repository selection
  async getForecast(siteSlug, asOf, hours, productSelection) {
    reads.push({
      asOf,
      hours,
      ...(productSelection === undefined ? {} : { productSelection }),
      siteSlug,
    });
    return [];
  },
  // expose the active forecast source and site calendar
  async listSites() {
    return [source];
  },
}, {
  dataAt,
  now: () => new Date(dataAt),
});

// exercise compatibility daily and anchor-containing overnight reads
for (const suffix of ["", "?days=10", "?window=overnight"]) {
  const response = await handler(new Request(
    `http://weather.test/api/v1/sites/ballydidean/forecast${suffix}`,
  ));
  assert.equal(response.status, 200);
}
assert.deepEqual(reads, [
  {
    asOf: "2026-08-20T19:00:00.000Z",
    hours: 37,
    siteSlug: "ballydidean",
  },
  {
    asOf: "2026-08-20T19:00:00.000Z",
    hours: 253,
    siteSlug: "ballydidean",
  },
  {
    asOf: "2026-08-21T07:00:00.000Z",
    hours: 31,
    productSelection: "anchor-containing",
    siteSlug: "ballydidean",
  },
]);

process.stdout.write("forecast endpoint read selection Node oracle passed\n");
