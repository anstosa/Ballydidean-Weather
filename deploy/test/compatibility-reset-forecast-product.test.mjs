import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const update = await readFile(new URL("../scripts/update.sh", import.meta.url), "utf8");
const reset = update.match(/--command "(CREATE OR REPLACE FUNCTION weather_source_is_current[^\n]+)"/u)?.[1] ?? "";

// lock the schema-23 compatibility-clone rollback boundary
test("legacy compatibility reset removes the forecast read index and catalog objects", () => {
  assert.notEqual(reset, "");
  const orderedDrops = [
    "DROP INDEX IF EXISTS public.weather_records_forecast_product_valid_idx;",
    "DROP TRIGGER IF EXISTS weather_records_forecast_products_insert ON weather_records;",
    "DROP TRIGGER IF EXISTS weather_records_forecast_products_delete ON weather_records;",
    "DROP TRIGGER IF EXISTS weather_records_forecast_products_truncate ON weather_records;",
    "DROP FUNCTION IF EXISTS weather_sync_forecast_products_after_insert();",
    "DROP FUNCTION IF EXISTS weather_sync_forecast_products_after_delete();",
    "DROP FUNCTION IF EXISTS weather_sync_forecast_products_after_truncate();",
    "DROP TABLE IF EXISTS forecast_products;",
  ];
  let cursor = -1;

  // require triggers before their owner functions and table
  for (const statement of orderedDrops) {
    const index = reset.indexOf(statement);
    assert(index > cursor, `${statement} is missing or out of order`);
    cursor = index;
  }
  assert.match(
    reset,
    /DELETE FROM schema_migrations WHERE name IN \([^)]*'0022_forecast_product_catalog\.sql', '0023_forecast_product_read_index\.sql'\)/u,
  );
  assert.doesNotMatch(reset, /\bCASCADE\b/u);
  assert.doesNotMatch(reset, /DELETE FROM (?:public\.)?weather_records\b/u);
});
