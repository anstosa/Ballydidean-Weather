import assert from "node:assert/strict";

import { listWeatherHistory } from "../../../../packages/database/dist/index.js";

const captured = [];
const pool = {
  // capture one query and return ordered decoded rows
  async query(text, values) {
    captured.push({ text, values });
    return {
      rowCount: 2,
      rows: [{ id: "103" }, { id: "102" }],
    };
  },
};
const rows = await listWeatherHistory(pool, {
  cursor: { id: "90", validAt: "2026-08-10T00:00:00.000Z" },
  from: "2026-08-01T00:00:00.000Z",
  limit: 251,
  siteSlug: "ballydidean",
  sourceId: "11",
  sourceKind: "reanalysis",
  stationSlug: "open-meteo-virtual",
  to: "2026-08-22T00:00:00.000Z",
});
assert.equal(captured.length, 1);
assert.deepEqual(captured[0].values, [
  "ballydidean",
  "open-meteo-virtual",
  "11",
  "reanalysis",
  "2026-08-01T00:00:00.000Z",
  "2026-08-22T00:00:00.000Z",
  "2026-08-10T00:00:00.000Z",
  "90",
  251,
]);
assert.deepEqual(rows.map(({ id }) => id), ["103", "102"]);

// collapse irrelevant formatting while retaining the complete SQL plan
const sql = captured[0].text.trim().split(/\s+/u).join(" ");
process.stdout.write(`sql=${sql}\n`);
process.stdout.write(`binds=${captured[0].values.join("|")}\n`);
process.stdout.write(`rows=${rows.map(({ id }) => id).join("|")}\n`);
