import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";

// build one source-owned active metadata row
function row(siteSlug, stationSlug, sourceId, sourceKey, sourceKind) {
  return {
    attributionLabel: "Weather data by provider",
    attributionUrl: "https://provider.test/",
    latitude: 47.95,
    longitude: -122.42,
    providerKey: "provider",
    providerName: "Provider",
    siteName: `Site ${siteSlug}`,
    siteSlug,
    sourceId,
    sourceKey,
    sourceKind,
    stationKind: "virtual",
    stationLatitude: 47.96,
    stationLongitude: -122.43,
    stationName: `Station ${stationSlug}`,
    stationSlug,
    timezone: "America/Los_Angeles",
  };
}

const rows = [
  row("ballydidean", "virtual", "10", "current", "model_current"),
  row("ballydidean", "virtual", "11", "forecast", "forecast"),
  row("ballydidean", "physical", "12", "sensor", "physical_sensor"),
  row("second", "virtual", "13", "history", "reanalysis"),
];
const handler = createWeatherApi({
  // return only the typed repository metadata used by this route
  async listSites() {
    return rows;
  },
});
const response = await handler(new Request("http://weather.test/api/v1/sites"));
const body = await response.json();

assert.equal(response.status, 200);
assert.deepEqual(
  body.data.map(
    // retain insertion order for sites
    (site) => site.slug,
  ),
  ["ballydidean", "second"],
);
assert.deepEqual(
  body.data[0].stations.map(
    // retain insertion order for stations
    (station) => station.slug,
  ),
  ["virtual", "physical"],
);
assert.deepEqual(
  body.data[0].stations[0].sources.map(
    // retain insertion order for sources
    (source) => source.key,
  ),
  ["current", "forecast"],
);
assert.deepEqual(body.data[0].stations[0].sources[1], {
  attribution: {
    label: "Weather data by provider",
    url: "https://provider.test/",
  },
  id: "11",
  key: "forecast",
  kind: "forecast",
  providerKey: "provider",
  providerName: "Provider",
  provenanceLabel: "forecast model product",
});
process.stdout.write("active-site API grouping Node oracle passed\n");
