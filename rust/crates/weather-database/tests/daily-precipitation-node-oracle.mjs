import assert from "node:assert/strict";

import { getDailyPrecipitation } from "../../../../packages/database/dist/index.js";

const captured = [];
const rows = [{
  accumulationMm: 2.54,
  sourceId: "13",
  stationSlug: "ballydidean-ecowitt",
  validThrough: new Date("2026-08-22T04:59:00.000Z"),
}];
const pool = {
  // capture the exact statement and ordered scalar binds
  async query(text, values) {
    captured.push({ text, values });
    return { rowCount: rows.length, rows };
  },
};

const result = await getDailyPrecipitation(pool, {
  from: "2026-08-21T07:00:00.000Z",
  siteSlug: "ballydidean",
  to: "2026-08-22T05:00:00.000Z",
});
assert.equal(result, rows[0]);
assert.deepEqual(captured[0].values, [
  "ballydidean",
  "2026-08-21T07:00:00.000Z",
  "2026-08-22T05:00:00.000Z",
]);

// collapse irrelevant formatting while retaining every SQL token
const sql = captured[0].text.trim().split(/\s+/u).join(" ");
process.stdout.write(`sql=${sql}\n`);
process.stdout.write(`binds=${captured[0].values.join("|")}\n`);
process.stdout.write(`row=${result.sourceId}|${result.accumulationMm}\n`);
