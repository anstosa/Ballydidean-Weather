import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";

// build one active repository metadata row
function row(stationSlug, sourceId) {
  return {
    attributionLabel: "Provider",
    attributionUrl: "https://provider.test/",
    latitude: 47.95,
    longitude: -122.42,
    providerKey: "provider",
    providerName: "Provider",
    siteName: "Ballydidean",
    siteSlug: "ballydidean",
    sourceId,
    sourceKey: `source-${sourceId}`,
    sourceKind: "model_current",
    stationKind: "virtual",
    stationLatitude: 47.96,
    stationLongitude: -122.43,
    stationName: stationSlug,
    stationSlug,
    timezone: "America/Los_Angeles",
  };
}

const selections = [];
const handler = createWeatherApi(
  {
    // return no records after capturing one validated selection
    async getCurrent(_siteSlug, selection) {
      selections.push(selection);
      return [];
    },
    // expose two disjoint active station sources
    async listSites() {
      return [row("virtual", "10"), row("physical", "12")];
    },
  },
  {
    // freeze the repository as-of field outside this query selector
    now: () => new Date("2026-08-22T05:00:00.000Z"),
  },
);

// invoke one current request and read its public error body when present
async function request(query) {
  const response = await handler(new Request(
    `http://weather.test/api/v1/sites/ballydidean/current${query}`,
  ));
  return { body: await response.json(), status: response.status };
}

assert.equal((await request("?station=virtual&source=10")).status, 200);
assert.deepEqual(selections.at(-1), {
  asOf: "2026-08-22T05:00:00.000Z",
  sourceId: "10",
  stationSlug: "virtual",
});
assert.equal((await request("?source=12")).status, 200);
assert.deepEqual(selections.at(-1), {
  asOf: "2026-08-22T05:00:00.000Z",
  sourceId: "12",
});
assert.equal((await request("?station=&source=")).status, 200);
assert.deepEqual(selections.at(-1), { asOf: "2026-08-22T05:00:00.000Z" });

const station = await request("?station=missing");
assert.deepEqual(station, {
  body: { error: { code: "station_not_found", message: "Unknown station: missing" } },
  status: 404,
});
const source = await request("?station=virtual&source=12");
assert.deepEqual(source, {
  body: { error: { code: "source_not_found", message: "Unknown source: 12" } },
  status: 404,
});
const invalidStation = await request("?station=Bad");
assert.deepEqual(invalidStation, {
  body: { error: { code: "invalid_query", message: "Request query is invalid" } },
  status: 400,
});
const invalidSource = await request("?source=0");
assert.deepEqual(invalidSource, {
  body: { error: { code: "invalid_query", message: "source must be a positive integer" } },
  status: 400,
});
const duplicate = await request("?source=10&source=10");
assert.deepEqual(duplicate, {
  body: { error: { code: "invalid_query", message: "source may be provided once" } },
  status: 400,
});
const unsupported = await request("?other=value");
assert.deepEqual(unsupported, {
  body: {
    error: {
      code: "invalid_query",
      message: "Unsupported query parameter: other",
    },
  },
  status: 400,
});
process.stdout.write("current selection Node oracle passed\n");
