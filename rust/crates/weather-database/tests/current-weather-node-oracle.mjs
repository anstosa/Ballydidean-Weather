import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import { getCurrentWeather } from "../../../../packages/database/dist/index.js";

const sql = await readFile(
  new URL("../fixtures/current-weather.sql", import.meta.url),
  "utf8",
);
const row = {
  adapterVersion: null,
  pm25HourlyConcentrations: [5.5, null],
  pressureChange3hHpa: -1.25,
  pressureHpa: 1018.25,
  validAt: "2026-08-22T05:00:00.000Z",
};
let captured;
const rows = await getCurrentWeather(
  {
    // capture the compiled repository query and return one typed sentinel row
    async query(text, values = []) {
      captured = { text, values };
      return { rowCount: 1, rows: [row] };
    },
  },
  "ballydidean",
  {
    asOf: "2026-08-22T05:37:00.000Z",
    sourceId: "10",
    stationSlug: "open-meteo-virtual",
  },
);

assert.deepEqual(captured, {
  text: sql,
  values: [
    "ballydidean",
    "open-meteo-virtual",
    "10",
    "2026-08-22T05:37:00.000Z",
  ],
});
assert.equal(rows[0], row);
process.stdout.write("current-weather Node SQL/bind/projection oracle passed\n");
