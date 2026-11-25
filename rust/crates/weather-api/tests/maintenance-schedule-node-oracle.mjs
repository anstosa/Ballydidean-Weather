import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";

import { readAdjustmentShadowRegistrationSlot } from "../../../../packages/database/dist/index.js";

const queries = [];
const queryable = {
  // return one family-bound value-free slot
  async query(sql, values) {
    queries.push({ sql, values });
    return {
      rows: [{
        value: {
          contractVersion: "adjustment-shadow-registration-slot/v3",
          epochWitnessSha256: "d".repeat(64),
          family: values[0],
          horizonEndAt: "2028-08-26T07:00:00.000Z",
          registrationSha256: null,
          scheduleContractSha256: "e".repeat(64),
          state: "free",
          terminalAt: null,
        },
      }],
    };
  },
};

const slots = await Promise.all([
  readAdjustmentShadowRegistrationSlot(queryable, "temperature"),
  readAdjustmentShadowRegistrationSlot(queryable, "wind"),
  readAdjustmentShadowRegistrationSlot(queryable, "rain"),
]);

assert.deepEqual(slots.map((slot) => slot.family), ["temperature", "wind", "rain"]);
assert.ok(slots.every((slot) => slot.state === "free"));
assert.deepEqual(queries.map((query) => query.values), [
  ["temperature"],
  ["wind"],
  ["rain"],
]);
assert.ok(queries.every((query) =>
  query.sql === "SELECT adjustment_shadow_registration_slot_v3($1::text) AS value"
));
// expose the exact original response bytes to the concrete runtime wire differential
if (process.argv[2] !== undefined) {
  await writeFile(process.argv[2], JSON.stringify(slots));
}
process.stdout.write("maintenance schedule slot Node oracle passed\n");
