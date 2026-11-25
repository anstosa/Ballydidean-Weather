import assert from "node:assert/strict";

import { getWeatherForecast } from "../../../../packages/database/dist/index.js";

const rawRows = [
  {
    adapterVersion: null,
    contractEpoch: null,
    id: "201",
    sourceConfigFingerprint: null,
    sourceId: "12",
    sourceKey: "open-meteo-forecast-v4",
  },
  {
    adapterVersion: null,
    contractEpoch: null,
    id: "202",
    sourceConfigFingerprint: null,
    sourceId: "12",
    sourceKey: "open-meteo-forecast-v4",
  },
];
const captured = [];
const pool = {
  // return raw rows, optional provenance, and an empty anchor product
  async query(text, values) {
    captured.push({ text, values });

    // decorate only the first raw identity
    if (text.includes("forecast_runtime_provenance_v1")) {
      return {
        rows: [{
          adapterVersion: "open-meteo/v4",
          contractEpoch: "forecast-contract/v4",
          sourceConfigFingerprint: "f".repeat(64),
          sourceId: "12",
          sourceKey: "open-meteo-forecast-v4",
          weatherRecordId: "201",
        }],
      };
    }

    // keep the anchor branch empty while retaining its captured plan
    if (values[1] === "2026-08-21T07:00:00.000Z") {
      return { rows: [] };
    }

    return { rows: rawRows };
  },
};

const latest = await getWeatherForecast(pool, {
  asOf: "2026-08-20T19:00:00.000Z",
  hours: 37,
  siteSlug: "ballydidean",
});
const anchor = await getWeatherForecast(pool, {
  asOf: "2026-08-21T07:00:00.000Z",
  hours: 31,
  productSelection: "anchor-containing",
  siteSlug: "ballydidean",
});
assert.equal(anchor.length, 0);
assert.deepEqual(latest.map((row) => ({
  adapterVersion: row.adapterVersion,
  contractEpoch: row.contractEpoch,
  id: row.id,
  sourceConfigFingerprint: row.sourceConfigFingerprint,
})), [
  {
    adapterVersion: "open-meteo/v4",
    contractEpoch: "forecast-contract/v4",
    id: "201",
    sourceConfigFingerprint: "f".repeat(64),
  },
  {
    adapterVersion: null,
    contractEpoch: null,
    id: "202",
    sourceConfigFingerprint: null,
  },
]);
assert.deepEqual(captured[0].values, [
  "ballydidean",
  "2026-08-20T19:00:00.000Z",
  "2026-08-22T08:00:00.000Z",
]);
assert.deepEqual(captured[1].values, [["201", "202"]]);
assert.deepEqual(captured[2].values, [
  "ballydidean",
  "2026-08-21T07:00:00.000Z",
  "2026-08-22T14:00:00.000Z",
]);

// reject invalid hours and runtime product selectors before query execution
for (const query of [
  { asOf: "2026-08-20T19:00:00.000Z", hours: 0, siteSlug: "ballydidean" },
  { asOf: "2026-08-20T19:00:00.000Z", hours: 265, siteSlug: "ballydidean" },
  {
    asOf: "2026-08-20T19:00:00.000Z",
    hours: 37,
    productSelection: "oldest",
    siteSlug: "ballydidean",
  },
]) {
  await assert.rejects(getWeatherForecast(pool, query), RangeError);
}
assert.equal(captured.length, 3);

// collapse irrelevant formatting while retaining every SQL token
const normalize = (text) => text.trim().split(/\s+/u).join(" ");
process.stdout.write(`latest-sql=${normalize(captured[0].text)}\n`);
process.stdout.write(`latest-binds=${captured[0].values.join("|")}\n`);
process.stdout.write(`provenance-sql=${normalize(captured[1].text)}\n`);
process.stdout.write(`provenance-binds=${captured[1].values[0].join("|")}\n`);
process.stdout.write(`anchor-sql=${normalize(captured[2].text)}\n`);
process.stdout.write(`anchor-binds=${captured[2].values.join("|")}\n`);
process.stdout.write("decorated=201|202\n");
