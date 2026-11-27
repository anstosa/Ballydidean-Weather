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
  cursor: { id: "90", validAt: "2026-08-10T01:00:00+01:00" },
  from: "2026-08-01T01:00:00+01:00",
  limit: 251,
  siteSlug: "ballydidean",
  sourceId: "11",
  sourceKind: "reanalysis",
  stationSlug: "open-meteo-virtual",
  to: "2026-08-22T01:00:00+01:00",
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
await assert.rejects(
  listWeatherHistory(pool, {
    from: "2026-08-01T00:30:00-01:00",
    siteSlug: "ballydidean",
    to: "2026-08-01T01:00:00Z",
  }),
  {
    name: "RangeError",
    message: "history from must be earlier than to",
  },
);
assert.equal(captured.length, 1);

// collapse irrelevant formatting while retaining the complete SQL plan
const sql = captured[0].text.trim().split(/\s+/u).join(" ");
process.stdout.write(`sql=${sql}\n`);
process.stdout.write(`binds=${captured[0].values.join("|")}\n`);
process.stdout.write(`rows=${rows.map(({ id }) => id).join("|")}\n`);
