import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const migration = await readFile(
  new URL("../migrations/0023_forecast_product_read_index.sql", import.meta.url),
  "utf8",
);
const deploymentCommon = await readFile(
  new URL("../../../deploy/scripts/common.sh", import.meta.url),
  "utf8",
);

// lock the narrow forecast read index contract
test("forecast product reads use one product-first partial index", () => {
  assert.match(migration, /SET LOCAL lock_timeout = '10s';/u);
  assert.match(migration, /SET LOCAL statement_timeout = '120s';/u);
  assert.match(
    migration,
    /CREATE INDEX weather_records_forecast_product_valid_idx\s+ON public\.weather_records \(\s*source_id,\s*product_run_at DESC,\s*valid_at ASC,\s*id ASC\s*\)\s+WHERE source_kind = 'forecast';/u,
  );
  assert.doesNotMatch(migration, /\bINCLUDE\b/u);
  assert.doesNotMatch(migration, /\bCONCURRENTLY\b/u);
  assert.doesNotMatch(migration, /\b(?:INSERT|UPDATE|DELETE|TRUNCATE)\b/u);
});

// lock optional schema 22 and exact schema 23 runtime verification
test("runtime verification binds the forecast read index to migration 0023", () => {
  const verifier = deploymentCommon
    .split("verify_forecast_product_read_index() {")[1]
    .split("\n}\n\n# verify effective runtime grants")[0];

  assert.match(verifier, /weather_records_forecast_product_valid_idx/u);
  assert.match(verifier, /0023_forecast_product_read_index\.sql/u);
  assert.match(
    verifier,
    /e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a/u,
  );
  assert.match(verifier, /metadata\.indkey/u);
  assert.match(verifier, /ARRAY\[0, 3, 0, 0\]::smallint\[\]/u);
  assert.match(verifier, /\(source_kind\)::text = ''forecast''::text/u);
  assert.match(
    deploymentCommon,
    /forecast_product_read_index_verified=\$\(verify_forecast_product_read_index/u,
  );
});
