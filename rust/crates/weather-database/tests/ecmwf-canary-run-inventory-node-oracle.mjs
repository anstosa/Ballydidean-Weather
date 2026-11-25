import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import { listEcmwfTemperatureCanaryRunInitializations } from "../../../../packages/database/dist/index.js";

const sql = await readFile(
  new URL("../fixtures/ecmwf-canary-run-initializations.sql", import.meta.url),
  "utf8",
);

// capture exact database calls while returning caller-owned rows
function recordingPool(rows = []) {
  const calls = [];
  return {
    calls,
    pool: {
      // preserve submitted SQL and ordered binds without a database
      async query(text, values = []) {
        calls.push({ text, values });
        return { rowCount: rows.length, rows };
      },
    },
  };
}

const inventoryRows = [
  { runInitializedAt: new Date("2026-09-06T12:00:00.000Z") },
  { runInitializedAt: "2026-09-05T18:30:00-05:30" },
  { runInitializedAt: "2026-09-07T23:30:00+01:30" },
];
const sevenDays = recordingPool(inventoryRows);
const inventory = await listEcmwfTemperatureCanaryRunInitializations(
  sevenDays.pool,
  {
    from: "2026-09-01T01:00:00+01:00",
    siteSlug: "ballydidean",
    to: "2026-09-07T19:00:00-05:00",
  },
);

assert.deepEqual(sevenDays.calls, [{
  text: sql,
  values: [
    "ballydidean",
    "2026-09-01T00:00:00.000Z",
    "2026-09-08T00:00:00.000Z",
  ],
}]);
assert.deepEqual(inventory, [
  "2026-09-06T12:00:00.000Z",
  "2026-09-06T00:00:00.000Z",
  "2026-09-07T22:00:00.000Z",
]);

const equal = recordingPool();
assert.deepEqual(
  await listEcmwfTemperatureCanaryRunInitializations(equal.pool, {
    from: "2026-09-08T01:00:00+01:00",
    siteSlug: "ballydidean",
    to: "2026-09-08T00:00:00Z",
  }),
  [],
);
assert.deepEqual(equal.calls, [{
  text: sql,
  values: [
    "ballydidean",
    "2026-09-08T00:00:00.000Z",
    "2026-09-08T00:00:00.000Z",
  ],
}]);

const reversed = recordingPool();
await assert.rejects(
  listEcmwfTemperatureCanaryRunInitializations(reversed.pool, {
    from: "2026-09-08T00:00:00.000Z",
    siteSlug: "ballydidean",
    to: "2026-09-07T23:59:59.999Z",
  }),
  {
    message: "ECMWF temperature canary run inventory interval is invalid",
    name: "RangeError",
  },
);
assert.equal(reversed.calls.length, 0);

const overSevenDays = recordingPool();
await assert.rejects(
  listEcmwfTemperatureCanaryRunInitializations(overSevenDays.pool, {
    from: "2026-09-01T00:00:00.000Z",
    siteSlug: "ballydidean",
    to: "2026-09-08T00:00:00.001Z",
  }),
  {
    message: "ECMWF temperature canary run inventory interval is invalid",
    name: "RangeError",
  },
);
assert.equal(overSevenDays.calls.length, 0);

const unzoned = recordingPool();
await assert.rejects(
  listEcmwfTemperatureCanaryRunInitializations(unzoned.pool, {
    from: "2026-09-01T00:00:00.000",
    siteSlug: "ballydidean",
    to: "2026-09-01T01:00:00.000Z",
  }),
  {
    message: "from must include an explicit timezone",
    name: "RangeError",
  },
);
assert.equal(unzoned.calls.length, 0);

const invalidStorage = recordingPool([{ runInitializedAt: new Date(NaN) }]);
await assert.rejects(
  listEcmwfTemperatureCanaryRunInitializations(invalidStorage.pool, {
    from: "2026-09-01T00:00:00.000Z",
    siteSlug: "ballydidean",
    to: "2026-09-01T01:00:00.000Z",
  }),
  { name: "RangeError" },
);
assert.equal(invalidStorage.calls.length, 1);

process.stdout.write("ECMWF canary run inventory Node oracle passed\n");
