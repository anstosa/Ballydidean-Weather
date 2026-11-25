import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import { listActiveSites } from "../../../../packages/database/dist/index.js";

const sql = await readFile(
  new URL("../fixtures/list-active-sites.sql", import.meta.url),
  "utf8",
);
const expectedRows = [{ sourceKey: "open-meteo-current-v1" }];
let captured;
const result = await listActiveSites({
  // capture the actual compiled Node repository statement
  async query(text, values = []) {
    captured = { text, values };
    return { rowCount: expectedRows.length, rows: expectedRows };
  },
});

assert.deepEqual(captured, { text: sql, values: [] });
assert.equal(result, expectedRows);
process.stdout.write("active-site Node SQL/projection oracle passed\n");
