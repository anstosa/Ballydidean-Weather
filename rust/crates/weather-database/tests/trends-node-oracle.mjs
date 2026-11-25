import assert from "node:assert/strict";

import { listWeatherTrends } from "../../../../packages/database/dist/index.js";

const captured = [];
const rows = [
  { temperatureC: 11.5, validAt: new Date("2019-01-01T08:00:00.000Z") },
  { temperatureC: 16.2, validAt: new Date("2026-08-21T07:00:00.000Z") },
];
const pool = {
  // capture the exact statement and ordered scalar binds
  async query(text, values) {
    captured.push({ text, values });
    return { rowCount: rows.length, rows };
  },
};

const result = await listWeatherTrends(pool, {
  from: "2019-01-01T08:00:00.000Z",
  siteSlug: "ballydidean",
  to: "2026-08-22T05:00:00.000Z",
});
assert.deepEqual(result, rows);
assert.deepEqual(captured[0].values, [
  "ballydidean",
  "2019-01-01T08:00:00.000Z",
  "2026-08-22T05:00:00.000Z",
]);

// collapse irrelevant formatting while retaining every SQL token
const sql = captured[0].text.trim().split(/\s+/u).join(" ");
process.stdout.write(`sql=${sql}\n`);
process.stdout.write(`binds=${captured[0].values.join("|")}\n`);
process.stdout.write(`rows=${result.map(({ temperatureC }) => temperatureC).join("|")}\n`);
