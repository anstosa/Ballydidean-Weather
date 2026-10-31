import assert from "node:assert/strict";
import { test } from "node:test";

import {
  getForecastPressureContext,
  getWeatherForecast,
} from "../dist/index.js";

// capture one repository query
function capturingPool() {
  const queries = [];

  return {
    pool: {
      // retain generated SQL and bindings
      async query(text, values) {
        queries.push({ text, values });
        return { rows: [] };
      },
    },
    queries,
  };
}

// keep history exclusion in the covering index before exact unique-key pressure reads
test("pressure context filters covering identities before exact point reads", async () => {
  const captured = capturingPool();

  await getForecastPressureContext(captured.pool, {
    asOf: "2026-11-01T09:00:00.000Z",
    siteSlug: "ballydidean",
  });

  assert.equal(captured.queries.length, 1);
  const sql = captured.queries[0].text;
  assert.doesNotMatch(sql, /candidate_products AS MATERIALIZED/u);
  assert.match(sql, /FROM public\.forecast_products product\s+JOIN LATERAL/u);
  assert.match(sql, /product\.product_run_at <= \$5\s+ORDER BY product\.product_run_at DESC\s+LIMIT 1/u);
  assert.match(sql, /identity_keys AS MATERIALIZED/u);
  assert.match(sql, /identity\.product_run_at >= \$4/u);
  assert.match(sql, /identity\.product_run_at <= \$5/u);
  assert.match(sql, /raw\.source_id = identity\.source_id/u);
  assert.match(sql, /raw\.source_kind = 'forecast'/u);
  assert.match(sql, /ROW\(raw\.valid_at, raw\.product_run_at\) >= ROW/u);
  assert.match(sql, /ROW\(raw\.valid_at, raw\.product_run_at\) <= ROW/u);
  assert.match(sql, /array_agg\(raw\.id ORDER BY raw\.valid_at ASC, raw\.id ASC\)/u);
  assert.match(sql, /candidate\.id = ANY\(context_run\.record_ids\)/u);
  assert.doesNotMatch(sql, /candidate\.product_run_at = context_run\.product_run_at/u);
});

// partition every context instant without dropping off-grid records
test("pressure identity enumeration includes every grid point and open gap", async () => {
  const captured = capturingPool();
  await getForecastPressureContext(captured.pool, {
    asOf: "2026-11-01T09:30:00.000Z",
    siteSlug: "ballydidean",
  });
  const sql = captured.queries[0].text;
  const start = sql.indexOf("WITH slots AS MATERIALIZED");
  const end = sql.indexOf("SELECT product.product_run_at, qualified.record_ids", start);
  assert.ok(start >= 0);
  assert.ok(end > start);
  const enumeration = sql.slice(start, end);
  assert.match(enumeration, /generate_series\(\s*\$2::timestamptz,\s*\$3::timestamptz - interval '1 hour',\s*interval '1 hour'/u);
  assert.match(enumeration, /UNION ALL/u);
  assert.match(enumeration, /ROW\(identity\.valid_at, identity\.product_run_at\)\s*>= ROW\(slots\.slot, \$4::timestamptz\)/u);
  assert.match(enumeration, /ROW\(identity\.valid_at, identity\.product_run_at\)\s*<= ROW\(slots\.slot, \$5::timestamptz\)/u);
  assert.match(enumeration, /identity\.valid_at > slots\.slot/u);
  assert.match(enumeration, /identity\.valid_at < slots\.slot \+ interval '1 hour'/u);
  assert.equal([...enumeration.matchAll(/OFFSET 0/gu)].length, 2);
  assert.equal([...enumeration.matchAll(/identity\.source_id = s\.id/gu)].length, 2);
  assert.equal([...enumeration.matchAll(/identity\.source_kind = 'forecast'/gu)].length, 2);
  assert.equal([...enumeration.matchAll(/identity\.product_run_at >= \$4/gu)].length, 2);
  assert.equal([...enumeration.matchAll(/identity\.product_run_at <= \$5/gu)].length, 2);
  assert.doesNotMatch(enumeration, /\bLIMIT\b|pressure_hpa/u);
});

// require catalog lookup only for the latest policy
test("latest forecast selection uses the product catalog while anchor selection stays raw", async () => {
  const latest = capturingPool();
  const anchor = capturingPool();

  await getWeatherForecast(latest.pool, {
    asOf: "2026-11-01T09:00:00.000Z",
    hours: 24,
    siteSlug: "ballydidean",
  });
  await getWeatherForecast(anchor.pool, {
    asOf: "2026-11-01T09:00:00.000Z",
    hours: 24,
    productSelection: "anchor-containing",
    siteSlug: "ballydidean",
  });

  assert.equal(latest.queries.length, 1);
  assert.match(latest.queries[0].text, /FROM public\.forecast_products product/u);
  assert.match(
    latest.queries[0].text,
    /WHERE product\.source_id = s\.id\s+ORDER BY product\.product_run_at DESC\s+LIMIT 1/u,
  );
  assert.doesNotMatch(
    latest.queries[0].text,
    /SELECT MAX\(product\.product_run_at\)/u,
  );

  assert.equal(anchor.queries.length, 1);
  assert.match(
    anchor.queries[0].text,
    /SELECT MAX\(product\.product_run_at\)\s+FROM weather_records product\s+WHERE product\.source_id = s\.id\s+AND product\.source_kind = 'forecast'\s+AND product\.valid_at = \$2/u,
  );
  assert.doesNotMatch(anchor.queries[0].text, /forecast_products/u);
});

// qualify each ordered vintage before stopping at one complete context
test("pressure context qualifies catalog vintages before selecting the newest", async () => {
  const captured = capturingPool();

  await getForecastPressureContext(captured.pool, {
    asOf: "2026-11-01T09:00:00.000Z",
    siteSlug: "ballydidean",
  });

  assert.equal(captured.queries.length, 1);
  const sql = captured.queries[0].text;
  const catalogStart = sql.indexOf("FROM public.forecast_products product");
  const correlatedQualification = sql.indexOf("JOIN LATERAL", catalogStart);
  const qualification = sql.indexOf("HAVING COUNT(*) = 6", catalogStart);
  const chosenLimit = sql.indexOf("LIMIT 1", qualification);

  assert.notEqual(catalogStart, -1);
  assert.ok(correlatedQualification > catalogStart);
  assert.ok(qualification > correlatedQualification);
  assert.match(sql, /product\.source_id = s\.id/u);
  assert.match(sql, /product\.product_run_at >= \$4/u);
  assert.match(sql, /product\.product_run_at <= \$5/u);
  assert.match(
    sql,
    /identity\.product_run_at = product\.product_run_at/u,
  );
  assert.match(sql, /raw\.pressure_hpa > '-Infinity'::double precision/u);
  assert.match(sql, /raw\.pressure_hpa < 'Infinity'::double precision/u);
  assert.match(sql, /HAVING COUNT\(\*\) = 6/u);
  assert.match(sql, /MIN\(raw\.valid_at\) = \$2/u);
  assert.match(
    sql,
    /MAX\(raw\.valid_at\) = \$3::timestamptz - interval '1 hour'/u,
  );
  assert.match(
    sql,
    /BOOL_AND\(raw\.valid_at = date_trunc\('hour', raw\.valid_at\)\)/u,
  );
  assert.ok(qualification > catalogStart);
  assert.ok(chosenLimit > qualification);
  assert.equal(sql.slice(catalogStart, qualification).includes("LIMIT"), false);
});
