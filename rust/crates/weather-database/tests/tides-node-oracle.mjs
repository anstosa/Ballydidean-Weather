import assert from "node:assert/strict";

import { listTideRecords } from "../../../../packages/database/dist/index.js";

const captured = [];
const rows = [
  { sourceId: "42", sourceKind: "tide_observation" },
  { sourceId: "43", sourceKind: "tide_prediction" },
];
const pool = {
  // capture each query and return ordered decoded tide rows
  async query(text, values) {
    captured.push({ text, values });
    return { rowCount: rows.length, rows };
  },
};

const result = await listTideRecords(pool, {
  from: "2026-08-21T05:00:00.000Z",
  limit: 7,
  siteSlug: "ballydidean",
  to: "2026-09-01T05:00:00.000Z",
});
await listTideRecords(pool, {
  from: "2026-08-21T05:00:00.000Z",
  siteSlug: "ballydidean",
  to: "2026-09-01T05:00:00.000Z",
});
assert.deepEqual(result, rows);
assert.equal(captured.length, 2);
assert.deepEqual(captured[0].values, [
  "ballydidean",
  "2026-08-21T05:00:00.000Z",
  "2026-09-01T05:00:00.000Z",
  7,
]);
assert.equal(captured[1].values[3], 10_000);

// collapse irrelevant formatting while retaining the complete SQL plan
const sql = captured[0].text.trim().split(/\s+/u).join(" ");
process.stdout.write(`sql=${sql}\n`);
process.stdout.write(`binds=${captured[0].values.join("|")}\n`);
process.stdout.write(`default-limit=${captured[1].values[3]}\n`);
process.stdout.write(`rows=${result.map(({ sourceId }) => sourceId).join("|")}\n`);
