import assert from "node:assert/strict";

import { createWeatherApi } from "../../../../apps/api/dist/index.js";

const dataAt = "2026-08-22T05:00:00.000Z";
const source = {
  attributionLabel: "NOAA Tides & Currents",
  attributionUrl: "https://tidesandcurrents.noaa.gov/",
  latitude: 47.95,
  longitude: -122.42,
  providerKey: "noaa-co-ops",
  providerName: "NOAA CO-OPS",
  siteName: "Ballydidean",
  siteSlug: "ballydidean",
  sourceId: "42",
  sourceKey: "glendale-tide-predictions",
  sourceKind: "tide_prediction",
  stationKind: "virtual",
  stationLatitude: 48.12,
  stationLongitude: -122.58,
  stationName: "Glendale Tide Predictions",
  stationSlug: "glendale-tide-predictions",
  timezone: "America/Los_Angeles",
};
const rows = [
  {
    attributionLabel: source.attributionLabel,
    attributionUrl: source.attributionUrl,
    predictionType: "H",
    providerKey: source.providerKey,
    sourceId: "42",
    sourceKind: "tide_prediction",
    stationName: source.stationName,
    stationSlug: source.stationSlug,
    validAt: "2026-08-22T11:27:00.000Z",
    waterLevelM: 3.271,
  },
  {
    attributionLabel: source.attributionLabel,
    attributionUrl: source.attributionUrl,
    predictionType: null,
    providerKey: source.providerKey,
    sourceId: "41",
    sourceKind: "tide_observation",
    stationName: "Glendale Tide Observations",
    stationSlug: "glendale-tide-observations",
    validAt: "2026-08-22T11:30:00.000Z",
    waterLevelM: 3.2,
  },
  {
    attributionLabel: source.attributionLabel,
    attributionUrl: source.attributionUrl,
    predictionType: "M",
    providerKey: source.providerKey,
    sourceId: "42",
    sourceKind: "tide_prediction",
    stationName: source.stationName,
    stationSlug: source.stationSlug,
    validAt: "2026-08-22T11:33:00.000Z",
    waterLevelM: 3.1,
  },
  {
    attributionLabel: source.attributionLabel,
    attributionUrl: source.attributionUrl,
    predictionType: "L",
    providerKey: source.providerKey,
    sourceId: "42",
    sourceKind: "tide_prediction",
    stationName: source.stationName,
    stationSlug: source.stationSlug,
    validAt: "2026-08-22T11:36:00.000Z",
    waterLevelM: 0.4,
  },
];
const queries = [];
const handler = createWeatherApi({
  // capture the resolved range and return ordered tide rows
  async listTides(siteSlug, from, to) {
    queries.push({ from, siteSlug, to });
    return rows;
  },
  // expose the active tide source and site
  async listSites() {
    return [source];
  },
}, {
  dataAt,
  now: () => new Date(dataAt),
});

const defaultResponse = await handler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/tides",
));
const body = await defaultResponse.json();
assert.equal(defaultResponse.status, 200);
assert.deepEqual(queries[0], {
  from: "2026-08-21T05:00:00.000Z",
  siteSlug: "ballydidean",
  to: "2026-09-01T05:00:00.000Z",
});
assert.deepEqual(body.data.map(({ eventType, kind }) => ({ eventType, kind })), [
  { eventType: "high", kind: "prediction" },
  { eventType: null, kind: "observation" },
  { eventType: null, kind: "prediction" },
  { eventType: "low", kind: "prediction" },
]);
assert.equal(body.data[0].source.stationSlug, "glendale-tide-predictions");
assert.equal(body.data[0].validAt, "2026-08-22T11:27:00.000Z");
assert.equal(body.generatedAt, dataAt);

const customResponse = await handler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/tides?from=2026-08-20T00%3A00%3A00Z&to=2026-08-25T00%3A00%3A00Z",
));
assert.equal(customResponse.status, 200);
assert.deepEqual(queries[1], {
  from: "2026-08-20T00:00:00.000Z",
  siteSlug: "ballydidean",
  to: "2026-08-25T00:00:00.000Z",
});

const excessive = await handler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/tides?from=2026-08-01T00%3A00%3A00Z&to=2026-09-02T00%3A00%3A00Z",
));
assert.equal(excessive.status, 400);
const duplicate = await handler(new Request(
  "http://weather.test/api/v1/sites/ballydidean/tides?from=2026-08-20T00%3A00%3A00Z&from=2026-08-21T00%3A00%3A00Z",
));
assert.equal(duplicate.status, 400);

process.stdout.write("tides query and projection Node oracle passed\n");
