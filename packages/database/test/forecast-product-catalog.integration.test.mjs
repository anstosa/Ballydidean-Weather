import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";

import {
  getForecastPressureContext,
  getWeatherForecast,
  runMigrations,
} from "../dist/index.js";
import {
  createTestPool,
  prepareRuntimeRoles,
  startPostgres,
  stopPostgres,
} from "./postgres-harness.mjs";

const migrationDirectory = resolve(import.meta.dirname, "../migrations");

// copy one immutable migration prefix
async function copyMigrationsThrough(targetDirectory, maximumName) {
  await mkdir(targetDirectory);

  // retain only the requested ordered prefix
  for (const name of (await readdir(migrationDirectory)).sort()) {
    // skip migrations after the prefix
    if (name > maximumName) {
      continue;
    }

    await copyFile(join(migrationDirectory, name), join(targetDirectory, name));
  }
}

// build a transaction-fault migration fixture
async function createFaultMigrationDirectory(
  targetDirectory,
  transformCatalogMigration,
) {
  await copyMigrationsThrough(targetDirectory, "0021_adjustment_rolling_registration.sql");
  const catalogSql = await readFile(
    join(migrationDirectory, "0022_forecast_product_catalog.sql"),
    "utf8",
  );
  await writeFile(
    join(targetDirectory, "0022_forecast_product_catalog.sql"),
    transformCatalogMigration(catalogSql),
  );
}

// pause for bounded lock observation
function delay(milliseconds) {
  // resolve after the observation window
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

// create one site station and source
async function createSource(
  pool,
  providerId,
  { config = {}, siteSlug, sourceKey, sourceKind = "forecast", stationId = null },
) {
  let targetStationId = stationId;

  // create an independent station when none is shared
  if (targetStationId === null) {
    const site = await pool.query(
      `INSERT INTO sites (slug, display_name, latitude, longitude, timezone)
       VALUES ($1, $2, 47.95, -122.43, 'America/Los_Angeles') RETURNING id`,
      [siteSlug, `${siteSlug} site`],
    );
    const station = await pool.query(
      `INSERT INTO stations
         (site_id, slug, display_name, station_kind, latitude, longitude)
       VALUES ($1, $2, $3, 'virtual', 47.95, -122.43) RETURNING id`,
      [site.rows[0].id, `${siteSlug}-station`, `${siteSlug} station`],
    );
    targetStationId = station.rows[0].id;
  }

  const source = await pool.query(
    `INSERT INTO sources (
       station_id, provider_id, source_key, source_kind,
       material_provider_config, source_config_fingerprint,
       capabilities, cadence_seconds, active
     ) VALUES (
       $1, $2, $3, $4, $5, $6, $7, 3600, true
     ) RETURNING id, station_id, source_config_fingerprint`,
    [
      targetStationId,
      providerId,
      sourceKey,
      sourceKind,
      config,
      createHash("sha256").update(`${sourceKey}:${sourceKind}`).digest("hex"),
      JSON.stringify(sourceKind === "forecast" ? ["forecast"] : ["current"]),
    ],
  );

  return source.rows[0];
}

// create one completed ingestion run
async function createRun(pool, source, startedAt) {
  const result = await pool.query(
    `INSERT INTO ingestion_runs (
       source_id, mode, requested_start, requested_end_exclusive,
       source_config_fingerprint, adapter_version, started_at, deadline_at,
       completed_at, state, attempts, record_count
     ) VALUES (
       $1, 'scheduled', $3::timestamptz, $3::timestamptz + interval '1 hour',
       $2, 'catalog-test/v1', $3, $3::timestamptz + interval '5 minutes',
       $3::timestamptz + interval '1 minute', 'succeeded', 1, 0
     ) RETURNING id`,
    [source.id, source.source_config_fingerprint, startedAt],
  );

  return result.rows[0].id;
}

// insert exact raw rows
async function insertRows(
  pool,
  { hash, pressures, productRunAt, runId, sourceId, sourceKind, validAt },
) {
  await pool.query(
    `INSERT INTO weather_records (
       source_id, source_kind, valid_at, product_run_at,
       first_ingestion_run_id, last_ingestion_run_id,
       first_received_at, last_received_at, upstream_timezone,
       upstream_model, provider_metadata, temperature_c, pressure_hpa, content_hash
     )
     SELECT
       $1::bigint, $2::varchar(32), input.valid_at, $3::timestamptz,
       $4::bigint, $4::bigint,
       '2026-11-01T12:00:00.000Z', '2026-11-01T12:00:00.000Z', 'UTC',
       CASE WHEN $2::varchar(32) = 'forecast' THEN 'best_match' ELSE NULL END,
       CASE WHEN $2::varchar(32) = 'forecast'
         THEN '{"dataset":"forecast"}'::jsonb ELSE NULL END,
       10, input.pressure_hpa, $7::char(64)
     FROM unnest($5::timestamptz[], $6::double precision[])
       AS input(valid_at, pressure_hpa)`,
    [sourceId, sourceKind, productRunAt, runId, validAt, pressures, hash],
  );
}

// compare both invariant directions
async function assertExactCatalog(pool) {
  const result = await pool.query(`
    SELECT count(*)::integer AS count
    FROM (
      (SELECT DISTINCT source_id, product_run_at FROM weather_records
       WHERE source_kind = 'forecast'
       EXCEPT SELECT source_id, product_run_at FROM forecast_products)
      UNION ALL
      (SELECT source_id, product_run_at FROM forecast_products
       EXCEPT SELECT DISTINCT source_id, product_run_at FROM weather_records
       WHERE source_kind = 'forecast')
    ) differences
  `);

  assert.equal(result.rows[0].count, 0);
}

// compare generated identity partitions with the complete former window
async function assertPressureEnumerationCoverage(pool) {
  // cover aligned and shifted windows across the repeated local hour
  for (const asOf of ["2026-11-01T09:00:00.000Z", "2026-11-01T09:30:00.000Z"]) {
    let captured;
    await getForecastPressureContext({
      // retain the generated SQL without querying real records
      async query(text, values) {
        captured = { text, values };
        return { rows: [] };
      },
    }, { asOf, siteSlug: "enumeration-boundaries" });
    const start = captured.text.indexOf("WITH slots AS MATERIALIZED");
    const end = captured.text.indexOf("SELECT product.product_run_at, qualified.record_ids", start);
    assert.ok(start >= 0 && end > start);
    const enumeration = captured.text.slice(start, end);
    const bindings = [7, ...captured.values.slice(1)];
    const fixture = `
      WITH weather_records AS MATERIALIZED (
        SELECT source.source_id, kind.source_kind,
          $2::timestamptz + hour_index * interval '1 hour' + delta AS valid_at,
          product.product_run_at
        FROM unnest(ARRAY[6, 7, 8, NULL]::bigint[]) source(source_id)
        CROSS JOIN unnest(ARRAY[
          'forecast', 'model_current', 'physical_sensor', NULL
        ]::varchar(32)[]) kind(source_kind)
        CROSS JOIN generate_series(0, 6) hour_index
        CROSS JOIN unnest(ARRAY[
          interval '-1 microsecond', interval '0 seconds',
          interval '1 microsecond', interval '30 minutes'
        ]) delta
        CROSS JOIN unnest(ARRAY[
          $4::timestamptz - interval '1 microsecond', $4::timestamptz,
          $5::timestamptz, $5::timestamptz + interval '1 microsecond', NULL
        ]) product(product_run_at)
      )
    `;
    const projection = `
      identity.source_id::text AS "sourceId", identity.source_kind AS "sourceKind",
      identity.valid_at::text AS "validAt", identity.product_run_at::text AS "productRunAt"
    `;
    const ordering = `ORDER BY identity.source_id, identity.source_kind,
      identity.valid_at, identity.product_run_at`;
    const actual = await pool.query(`${fixture}
      SELECT ${projection}
      FROM (SELECT $1::bigint AS id) s
      CROSS JOIN LATERAL (
        ${enumeration}
        SELECT * FROM identity_keys
      ) identity
      ${ordering}
    `, bindings);
    const former = await pool.query(`${fixture}
      SELECT ${projection}
      FROM weather_records identity
      WHERE identity.source_id = $1
        AND identity.source_kind = 'forecast'
        AND identity.valid_at >= $2 AND identity.valid_at < $3
        AND identity.product_run_at >= $4 AND identity.product_run_at <= $5
      ${ordering}
    `, bindings);
    assert.equal(former.rows.length, 48);
    assert.deepEqual(actual.rows, former.rows);
  }
}

// require one runtime denial
async function assertDenied(operation) {
  await assert.rejects(operation, /permission denied/u);
}

// verify additive upgrade maintenance query semantics and ACLs
test("0022 maintains an exact catalog for schema-21 workers and public reads", {
  timeout: 300_000,
}, async () => {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "weather-product-catalog-"));
  const legacyDirectory = join(temporaryDirectory, "migrations");
  const catalogDirectory = join(temporaryDirectory, "catalog-migrations");
  const server = await startPostgres(17, "forecast-product-catalog");
  const admin = createTestPool(server);
  let pool;
  let apiPool;
  let ingestPool;
  let trainingPool;

  try {
    await copyMigrationsThrough(
      legacyDirectory,
      "0021_adjustment_rolling_registration.sql",
    );
    // isolate catalog behavior from later index migrations
    await copyMigrationsThrough(catalogDirectory, "0022_forecast_product_catalog.sql");

    await prepareRuntimeRoles(admin);
    pool = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    assert.equal((await runMigrations(pool, legacyDirectory)).applied.length, 21);

    const provider = await pool.query(`
      INSERT INTO providers
        (provider_key, display_name, attribution_label, attribution_url)
      VALUES
        ('catalog-provider', 'Catalog provider', 'Catalog provider',
         'https://example.com/catalog')
      RETURNING id
    `);
    const pressureSource = await createSource(pool, provider.rows[0].id, {
      siteSlug: "pressure-catalog",
      sourceKey: "open-meteo-forecast-v4",
    });
    const oldSource = await createSource(pool, provider.rows[0].id, {
      siteSlug: "latest-catalog",
      sourceKey: "old-forecast",
    });
    const successorSource = await createSource(pool, provider.rows[0].id, {
      config: { supersedesSourceKey: "old-forecast" },
      siteSlug: "latest-catalog",
      sourceKey: "new-forecast",
      stationId: oldSource.station_id,
    });
    const emptySource = await createSource(pool, provider.rows[0].id, {
      siteSlug: "empty-catalog",
      sourceKey: "empty-forecast",
    });
    const currentSource = await createSource(pool, provider.rows[0].id, {
      siteSlug: "current-catalog",
      sourceKey: "current-model",
      sourceKind: "model_current",
    });
    const pressureRun = await createRun(pool, pressureSource, "2026-11-01T05:00:00Z");
    const oldRun = await createRun(pool, oldSource, "2026-11-01T10:30:00Z");
    const successorRun = await createRun(pool, successorSource, "2026-11-01T09:30:00Z");
    const currentRun = await createRun(pool, currentSource, "2026-11-01T06:00:00Z");

    await insertRows(pool, {
      hash: "a".repeat(64),
      pressures: [1000],
      productRunAt: "2026-11-01T05:00:00Z",
      runId: pressureRun,
      sourceId: pressureSource.id,
      sourceKind: "forecast",
      validAt: ["2026-11-01T06:00:00Z"],
    });
    await insertRows(pool, {
      hash: "b".repeat(64),
      pressures: [1001],
      productRunAt: "2026-11-01T10:30:00Z",
      runId: oldRun,
      sourceId: oldSource.id,
      sourceKind: "forecast",
      validAt: ["2026-11-01T12:00:00Z"],
    });
    await insertRows(pool, {
      hash: "c".repeat(64),
      pressures: [1002],
      productRunAt: "2026-11-01T01:30:00-08:00",
      runId: successorRun,
      sourceId: successorSource.id,
      sourceKind: "forecast",
      validAt: ["2026-11-01T12:00:00Z"],
    });
    await insertRows(pool, {
      hash: "d".repeat(64),
      pressures: [1003],
      productRunAt: "2026-11-01T01:30:00-07:00",
      runId: successorRun,
      sourceId: successorSource.id,
      sourceKind: "forecast",
      validAt: ["2026-11-01T12:00:00Z"],
    });
    await insertRows(pool, {
      hash: "e".repeat(64),
      pressures: [1000, 1000],
      productRunAt: null,
      runId: currentRun,
      sourceId: currentSource.id,
      sourceKind: "model_current",
      validAt: ["2026-11-01T06:00:00Z", "2026-11-01T07:00:00Z"],
    });

    const upgraded = await runMigrations(pool, catalogDirectory);
    assert.deepEqual(upgraded.applied, ["0022_forecast_product_catalog.sql"]);
    assert.equal(upgraded.current.length, 21);
    assert.equal(
      (await pool.query("SELECT count(*)::integer AS count FROM forecast_products"))
        .rows[0].count,
      4,
    );
    await assert.rejects(
      pool.query(
        "INSERT INTO forecast_products (source_id, product_run_at) VALUES ($1, NULL)",
        [pressureSource.id],
      ),
      /null value in column "product_run_at"/u,
    );
    await assertExactCatalog(pool);
    assert.equal(
      (await pool.query(`
        SELECT count(*)::integer AS count
        FROM pg_stats
        WHERE schemaname = 'public' AND tablename = 'forecast_products'
      `)).rows[0].count,
      2,
    );
    const repeated = await runMigrations(pool, catalogDirectory);
    assert.deepEqual(repeated.applied, []);
    assert.equal(repeated.current.length, 22);

    assert.deepEqual(
      (await pool.query(`
        SELECT column_name, data_type, is_nullable
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = 'forecast_products'
        ORDER BY ordinal_position
      `)).rows,
      [
        { column_name: "source_id", data_type: "bigint", is_nullable: "NO" },
        {
          column_name: "product_run_at",
          data_type: "timestamp with time zone",
          is_nullable: "NO",
        },
      ],
    );
    assert.deepEqual(
      (await pool.query(`
        SELECT contype, pg_get_constraintdef(oid) AS definition
        FROM pg_constraint
        WHERE conrelid = 'public.forecast_products'::regclass
        ORDER BY contype
      `)).rows,
      [
        {
          contype: "f",
          definition: "FOREIGN KEY (source_id) REFERENCES sources(id) ON DELETE RESTRICT",
        },
        { contype: "p", definition: "PRIMARY KEY (source_id, product_run_at)" },
      ],
    );
    assert.deepEqual(
      (await pool.query(`
        SELECT tgname, tgenabled,
          (tgtype & 1) = 0 AS statement_level
        FROM pg_trigger
        WHERE tgrelid = 'public.weather_records'::regclass
          AND tgname LIKE 'weather_records_forecast_products_%'
        ORDER BY tgname
      `)).rows,
      [
        {
          statement_level: true,
          tgenabled: "O",
          tgname: "weather_records_forecast_products_delete",
        },
        {
          statement_level: true,
          tgenabled: "O",
          tgname: "weather_records_forecast_products_insert",
        },
        {
          statement_level: true,
          tgenabled: "O",
          tgname: "weather_records_forecast_products_truncate",
        },
      ],
    );
    assert.deepEqual(
      (await pool.query(`
        SELECT proname, proowner::regrole::text AS owner,
          prosecdef, provolatile, proconfig
        FROM pg_proc
        WHERE pronamespace = 'public'::regnamespace
          AND proname LIKE 'weather_sync_forecast_products_after_%'
        ORDER BY proname
      `)).rows,
      [
        {
          owner: "weather_owner",
          proconfig: ["search_path=pg_catalog, public"],
          proname: "weather_sync_forecast_products_after_delete",
          prosecdef: true,
          provolatile: "v",
        },
        {
          owner: "weather_owner",
          proconfig: ["search_path=pg_catalog, public"],
          proname: "weather_sync_forecast_products_after_insert",
          prosecdef: true,
          provolatile: "v",
        },
        {
          owner: "weather_owner",
          proconfig: ["search_path=pg_catalog, public"],
          proname: "weather_sync_forecast_products_after_truncate",
          prosecdef: true,
          provolatile: "v",
        },
      ],
    );

    // mirror the existing runtime grant needed by weather-record checks
    await pool.query(`
      GRANT EXECUTE ON FUNCTION weather_adjustment_revision_receipt_valid_v1(jsonb)
      TO weather_api, weather_ingest
    `);

    apiPool = createTestPool(server, "weather_test", "weather_api", "api-test");
    ingestPool = createTestPool(
      server,
      "weather_test",
      "weather_ingest",
      "ingest-test",
    );
    trainingPool = createTestPool(
      server,
      "weather_test",
      "weather_training_export",
      "training-export-test",
    );

    // execute the unchanged old-worker bulk statement twice
    for (const revision of [0, 1]) {
      await ingestPool.query(
        `INSERT INTO weather_records (
           source_id, source_kind, valid_at, product_run_at,
           first_ingestion_run_id, last_ingestion_run_id,
           first_received_at, last_received_at, upstream_timezone,
           upstream_model, provider_metadata, temperature_c, pressure_hpa,
           content_hash
         )
         SELECT
           $1, 'forecast',
           '2026-11-02T00:00:00Z'::timestamptz + index * interval '1 hour',
           '2026-11-01T05:30:00Z', $2, $2,
           '2026-11-01T12:00:00Z',
           '2026-11-01T12:00:00Z'::timestamptz + $3 * interval '5 minutes',
           'UTC', 'best_match', '{"dataset":"forecast"}'::jsonb,
           10 + $3, 1000 + $3, repeat($4, 64)
         FROM generate_series(0, 239) index
         ON CONFLICT (source_id, source_kind, valid_at, product_run_at)
         DO UPDATE SET
           last_ingestion_run_id = EXCLUDED.last_ingestion_run_id,
           last_received_at = EXCLUDED.last_received_at,
           temperature_c = EXCLUDED.temperature_c,
           pressure_hpa = EXCLUDED.pressure_hpa,
           content_hash = EXCLUDED.content_hash,
           revision_count = weather_records.revision_count + 1`,
        [pressureSource.id, pressureRun, revision, revision === 0 ? "f" : "1"],
      );
    }
    assert.deepEqual(
      (await pool.query(
        `SELECT count(*)::integer AS count,
           min(revision_count)::integer AS minimum,
           max(revision_count)::integer AS maximum
         FROM weather_records
         WHERE source_id = $1 AND product_run_at = '2026-11-01T05:30:00Z'`,
        [pressureSource.id],
      )).rows[0],
      { count: 240, maximum: 1, minimum: 1 },
    );
    assert.equal(
      (await pool.query(
        `SELECT count(*)::integer AS count FROM forecast_products
         WHERE source_id = $1 AND product_run_at = '2026-11-01T05:30:00Z'`,
        [pressureSource.id],
      )).rows[0].count,
      1,
    );
    await assertExactCatalog(pool);

    const completeHours = Array.from(
      { length: 6 },
      // build the exact UTC pressure grid
      (_unused, index) =>
        new Date(Date.parse("2026-11-01T06:00:00Z") + index * 3_600_000)
          .toISOString(),
    );
    await insertRows(pool, {
      hash: "2".repeat(64),
      pressures: [1000, 1001, 1002, 1003, 1004, 1005],
      productRunAt: "2026-11-01T07:00:00Z",
      runId: pressureRun,
      sourceId: pressureSource.id,
      sourceKind: "forecast",
      validAt: completeHours,
    });
    await insertRows(pool, {
      hash: "3".repeat(64),
      pressures: [1000, 1001, 1002, 1003, 1004, 1005, 1006],
      productRunAt: "2026-11-01T08:00:00Z",
      runId: pressureRun,
      sourceId: pressureSource.id,
      sourceKind: "forecast",
      validAt: [completeHours[0], "2026-11-01T06:30:00Z", ...completeHours.slice(1)],
    });
    await insertRows(pool, {
      hash: "4".repeat(64),
      pressures: [1000, null, 1001, 1002, 1003, 1004, 1005],
      productRunAt: "2026-11-01T08:45:00Z",
      runId: pressureRun,
      sourceId: pressureSource.id,
      sourceKind: "forecast",
      validAt: [completeHours[0], "2026-11-01T06:30:00Z", ...completeHours.slice(1)],
    });
    await assert.rejects(
      // retain the raw pressure constraint
      () => insertRows(pool, {
        hash: "5".repeat(64),
        pressures: [Number.POSITIVE_INFINITY],
        productRunAt: "2026-11-01T08:50:00Z",
        runId: pressureRun,
        sourceId: pressureSource.id,
        sourceKind: "forecast",
        validAt: [completeHours[0]],
      }),
      /weather_records_pressure_check/u,
    );

    let pressureQuery;
    const tracingApiPool = {
      // capture the exact executed repository query
      async query(text, values) {
        pressureQuery = { text, values };
        return apiPool.query(text, values);
      },
    };
    const pressure = await getForecastPressureContext(tracingApiPool, {
      asOf: "2026-11-01T09:00:00Z",
      siteSlug: "pressure-catalog",
    });
    assert.deepEqual(
      pressure.map(
        // retain the selected complete older product
        (record) => ({
          pressureHpa: record.pressureHpa,
          productRunAt: record.productRunAt.toISOString(),
          validAt: record.validAt.toISOString(),
        }),
      ),
      completeHours.map(
        // build the expected pressure result
        (validAt, index) => ({
          pressureHpa: 1000 + index,
          productRunAt: "2026-11-01T08:45:00.000Z",
          validAt,
        }),
      ),
    );
    await pool.query("BEGIN");

    try {
      await pool.query("SET LOCAL enable_seqscan = off");
      const explained = await pool.query(
        `EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) ${pressureQuery.text}`,
        pressureQuery.values,
      );
      const planRoot = explained.rows[0]["QUERY PLAN"][0].Plan;
      const plan = JSON.stringify(explained.rows[0]["QUERY PLAN"]);
      const nodes = [];
      const pending = [planRoot];

      // inspect every physical scan without depending on serialized field adjacency
      while (pending.length > 0) {
        const node = pending.pop();
        nodes.push(node);
        pending.push(...node.Plans ?? []);
      }

      assert.match(
        plan,
        /"Scan Direction":"Backward","Index Name":"forecast_products_pkey"/u,
      );
      const rawScans = nodes.filter(
        // isolate covering enumeration and both exact identity fetches
        (node) => node["Relation Name"] === "weather_records",
      );
      const windowScans = rawScans.filter(
        // enumerate every singleton boundary and open interval
        (node) => node.Alias?.startsWith("identity"),
      );
      const pointScan = rawScans.find(
        // look up finite pressure by the complete unique identity
        (node) => node.Alias === "raw",
      );
      const finalScan = rawScans.find(
        // read only the six identities that already passed qualification
        (node) => node.Alias === "candidate",
      );
      assert.equal(rawScans.length, 4);
      assert.equal(windowScans.length, 2);

      // tiny fixtures may apply product bounds as filters rather than index conditions
      for (const windowScan of windowScans) {
        assert.equal(windowScan["Actual Loops"], 6);
        const predicates = `${windowScan["Index Cond"] ?? ""} ${windowScan.Filter ?? ""}`;
        assert.match(predicates, /product_run_at >=[\s\S]*product_run_at <=/u);
      }

      assert.ok(windowScans.some(
        // retain strict open-gap bounds
        (node) => /valid_at >[\s\S]*valid_at </u.test(`${node["Index Cond"] ?? ""} ${node.Filter ?? ""}`),
      ));
      assert.ok(pointScan);
      const pointAccess = [pointScan, ...pointScan.Plans ?? []];
      assert.ok(pointAccess.some(
        // full-size natural-plan qualification separately bounds the actual access work
        (node) => typeof node["Index Name"] === "string",
      ));
      // the tiny planner may retain the timestamp-pair predicate at the join
      assert.match(plan, /ROW\([^)]*valid_at[^)]*product_run_at\)/u);
      const finalIndex = [finalScan, ...finalScan.Plans ?? []].find(
        // permit the equivalent primary-key bitmap access on the tiny fixture
        (node) => node["Index Name"] === "weather_records_pkey",
      );
      assert.ok(finalIndex);
      assert.match(finalScan["Index Cond"] ?? finalScan["Recheck Cond"], /id = ANY/u);
      assert.equal(finalScan["Actual Rows"], 6);
      assert.equal(finalScan["Actual Loops"], 1);
    } finally {
      await pool.query("ROLLBACK");
    }

    const latest = await getWeatherForecast(apiPool, {
      asOf: "2026-11-01T12:00:00Z",
      hours: 1,
      siteSlug: "latest-catalog",
    });
    assert.equal(latest.length, 1);
    assert.equal(latest[0].sourceKey, "new-forecast");
    assert.equal(latest[0].productRunAt.toISOString(), "2026-11-01T09:30:00.000Z");

    await insertRows(pool, {
      hash: "6".repeat(64),
      pressures: Array.from(
        { length: 9 },
        // fill the extended horizon
        () => 1002,
      ),
      productRunAt: "2026-11-01T09:30:00Z",
      runId: successorRun,
      sourceId: successorSource.id,
      sourceKind: "forecast",
      validAt: Array.from(
        { length: 9 },
        // extend the latest product through the ten-hour horizon
        (_unused, index) =>
          new Date(Date.parse("2026-11-01T13:00:00Z") + index * 3_600_000)
            .toISOString(),
      ),
    });

    // compare catalog selection with the former raw maximum policy
    for (const hours of [1, 5, 10]) {
      const selected = await getWeatherForecast(apiPool, {
        asOf: "2026-11-01T12:00:00Z",
        hours,
        siteSlug: "latest-catalog",
      });
      const former = await pool.query(
        `SELECT id
         FROM weather_records
         WHERE source_id = $1
           AND source_kind = 'forecast'
           AND product_run_at = (
             SELECT max(product_run_at)
             FROM weather_records
             WHERE source_id = $1 AND source_kind = 'forecast'
           )
           AND valid_at >= '2026-11-01T12:00:00Z'
           AND valid_at < '2026-11-01T12:00:00Z'::timestamptz
             + $2::integer * interval '1 hour'
         ORDER BY valid_at, id`,
        [successorSource.id, hours],
      );
      assert.deepEqual(
        selected.map(
          // compare stable raw identities
          (record) => record.id,
        ),
        former.rows.map(
          // retain former query ordering
          (record) => record.id,
        ),
      );
    }

    const concurrentProduct = "2026-10-31T20:00:00Z";
    await Promise.all([
      insertRows(pool, {
        hash: "7".repeat(64),
        pressures: [1000, 1001],
        productRunAt: concurrentProduct,
        runId: pressureRun,
        sourceId: pressureSource.id,
        sourceKind: "forecast",
        validAt: ["2026-11-03T00:00:00Z", "2026-11-03T01:00:00Z"],
      }),
      insertRows(pool, {
        hash: "8".repeat(64),
        pressures: [1002, 1003],
        productRunAt: concurrentProduct,
        runId: pressureRun,
        sourceId: pressureSource.id,
        sourceKind: "forecast",
        validAt: ["2026-11-03T02:00:00Z", "2026-11-03T03:00:00Z"],
      }),
    ]);
    assert.equal(
      (await pool.query(
        `SELECT count(*)::integer AS count FROM forecast_products
         WHERE source_id = $1 AND product_run_at = $2`,
        [pressureSource.id, concurrentProduct],
      )).rows[0].count,
      1,
    );
    await assertExactCatalog(pool);
    assert.deepEqual(
      await getWeatherForecast(apiPool, {
        asOf: "2026-11-01T12:00:00Z",
        hours: 1,
        siteSlug: "empty-catalog",
      }),
      [],
    );
    assert.equal(emptySource.id > 0, true);

    assert.equal(
      (await apiPool.query("SELECT count(*)::integer AS count FROM forecast_products"))
        .rows[0].count > 0,
      true,
    );
    await assertDenied(apiPool.query(
      "INSERT INTO forecast_products (source_id, product_run_at) VALUES ($1, now())",
      [pressureSource.id],
    ));
    await assertDenied(apiPool.query(
      "UPDATE forecast_products SET product_run_at = product_run_at",
    ));
    await assertDenied(apiPool.query("DELETE FROM forecast_products"));
    await assertDenied(apiPool.query("TRUNCATE forecast_products"));
    await assertDenied(ingestPool.query("SELECT * FROM forecast_products"));
    await assertDenied(ingestPool.query(
      "INSERT INTO forecast_products (source_id, product_run_at) VALUES ($1, now())",
      [pressureSource.id],
    ));
    await assertDenied(trainingPool.query("SELECT * FROM forecast_products"));
    await assert.rejects(
      trainingPool.query(
        "INSERT INTO forecast_products (source_id, product_run_at) VALUES ($1, now())",
        [pressureSource.id],
      ),
      /cannot execute INSERT in a read-only transaction|permission denied/u,
    );
    assert.deepEqual(
      (await pool.query(`
        SELECT
          has_function_privilege(
            'weather_api',
            'public.weather_sync_forecast_products_after_insert()',
            'EXECUTE'
          ) AS api_insert,
          has_function_privilege(
            'weather_ingest',
            'public.weather_sync_forecast_products_after_delete()',
            'EXECUTE'
          ) AS ingest_delete,
          has_function_privilege(
            'weather_training_export',
            'public.weather_sync_forecast_products_after_truncate()',
            'EXECUTE'
          ) AS export_truncate
      `)).rows[0],
      { api_insert: false, export_truncate: false, ingest_delete: false },
    );

    const bulkIds = await pool.query(
      `SELECT id FROM weather_records
       WHERE source_id = $1 AND product_run_at = '2026-11-01T05:30:00Z'
       ORDER BY id`,
      [pressureSource.id],
    );
    await pool.query("DELETE FROM weather_records WHERE id = $1", [bulkIds.rows[0].id]);
    assert.equal(
      (await pool.query(
        `SELECT count(*)::integer AS count FROM forecast_products
         WHERE source_id = $1 AND product_run_at = '2026-11-01T05:30:00Z'`,
        [pressureSource.id],
      )).rows[0].count,
      1,
    );
    await pool.query(
      `DELETE FROM weather_records
       WHERE source_id = $1 AND product_run_at = '2026-11-01T05:30:00Z'`,
      [pressureSource.id],
    );
    assert.equal(
      (await pool.query(
        `SELECT count(*)::integer AS count FROM forecast_products
         WHERE source_id = $1 AND product_run_at = '2026-11-01T05:30:00Z'`,
        [pressureSource.id],
      )).rows[0].count,
      0,
    );
    await assertExactCatalog(pool);

    await pool.query("TRUNCATE TABLE weather_records CASCADE");
    assert.equal(
      (await pool.query("SELECT count(*)::integer AS count FROM forecast_products"))
        .rows[0].count,
      0,
    );
    await assertExactCatalog(pool);
  } finally {
    // ignore owned teardown failures
    await trainingPool?.end().catch(() => undefined);
    await ingestPool?.end().catch(() => undefined);
    await apiPool?.end().catch(() => undefined);
    await pool?.end().catch(() => undefined);
    await admin.end().catch(() => undefined);
    await stopPostgres(server);
    await rm(temporaryDirectory, { force: true, recursive: true });
  }
});

// retain natural ordered access before the catalog becomes all-visible
test("pressure context streams a fresh catalog without planner overrides", {
  timeout: 300_000,
}, async () => {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "weather-pressure-plan-"));
  const legacyDirectory = join(temporaryDirectory, "migrations");
  const server = await startPostgres(17, "pressure-natural-plan");
  const admin = createTestPool(server);
  let pool;
  let apiPool;

  try {
    await copyMigrationsThrough(legacyDirectory, "0021_adjustment_rolling_registration.sql");
    await prepareRuntimeRoles(admin);
    pool = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    await runMigrations(pool, legacyDirectory);
    const provider = await pool.query(`
      INSERT INTO providers
        (provider_key, display_name, attribution_label, attribution_url)
      VALUES ('natural-plan', 'Natural plan', 'Natural plan', 'https://example.com/plan')
      RETURNING id
    `);
    const source = await createSource(pool, provider.rows[0].id, {
      siteSlug: "natural-pressure",
      sourceKey: "open-meteo-forecast-v4",
    });
    const asOf = "2026-10-28T07:00:00.000Z";
    const runId = await createRun(pool, source, asOf);
    await pool.query(`
      WITH products AS (
        SELECT $2::timestamptz - interval '1 hour'
          - run_index * interval '6 hours' AS product_run_at
        FROM generate_series(0, 1447) run_index
        UNION ALL
        SELECT $2::timestamptz
      )
      INSERT INTO weather_records (
        source_id, source_kind, valid_at, product_run_at,
        first_ingestion_run_id, last_ingestion_run_id,
        first_received_at, last_received_at, upstream_timezone,
        upstream_model, provider_metadata, pressure_hpa, content_hash
      )
      SELECT $1::bigint, 'forecast',
        $2::timestamptz - interval '3 hours' + hour_index * interval '1 hour',
        product_run_at, $3::bigint, $3::bigint,
        $2, $2, 'UTC', 'best_match', '{"dataset":"forecast"}'::jsonb,
        1000 + hour_index, repeat('c', 64)
      FROM products
      CROSS JOIN generate_series(0, 5) hour_index
      WHERE product_run_at <> $2::timestamptz OR hour_index >= 3
    `, [source.id, asOf, runId]);
    await pool.query("ANALYZE weather_records");
    await runMigrations(pool, migrationDirectory);
    await pool.query("ALTER TABLE forecast_products SET (autovacuum_enabled = false)");
    const visibility = await pool.query(`
      SELECT relpages, relallvisible FROM pg_class
      WHERE oid = 'forecast_products'::regclass
    `);
    assert.ok(visibility.rows[0].relpages > 0);
    assert.equal(visibility.rows[0].relallvisible, 0);
    apiPool = createTestPool(server, "weather_test", "weather_api", "api-test");
    let captured;
    const tracingPool = {
      // retain the actual public read and bindings
      async query(text, values) {
        captured = { text, values };
        return apiPool.query(text, values);
      },
    };
    const result = await getForecastPressureContext(tracingPool, {
      asOf,
      siteSlug: "natural-pressure",
    });
    assert.equal(result.length, 6);
    await assertPressureEnumerationCoverage(apiPool);
    assert.ok(result.every(
      // skip the newest incomplete product without truncating the search
      (row) => row.productRunAt.toISOString() === "2026-10-28T06:00:00.000Z",
    ));
    const explained = await apiPool.query(
      `EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) ${captured.text}`,
      captured.values,
    );
    const nodes = [];
    const pending = [explained.rows[0]["QUERY PLAN"][0].Plan];

    // inspect the natural plan without changing planner settings
    while (pending.length > 0) {
      const node = pending.pop();
      nodes.push(node);
      pending.push(...node.Plans ?? []);
    }

    const catalog = nodes.find(
      // require bounded descending traversal even before vacuum
      (node) => node["Relation Name"] === "forecast_products"
        && node["Index Name"] === "forecast_products_pkey"
        && node["Scan Direction"] === "Backward",
    );
    assert.ok(catalog, "fresh catalog must use the ordered primary-key range");
    assert.match(catalog["Index Cond"], /source_id[\s\S]*product_run_at >=[\s\S]*product_run_at <=/u);
    assert.equal(nodes.some(
      // reject materialized product sorting rather than the final six-row order
      (node) => node["Node Type"] === "Sort"
        && (node["Sort Key"] ?? []).some(
          // identify vintage-order sorts only
          (key) => key.includes("product_run_at"),
        ),
    ), false);
  } finally {
    await Promise.allSettled([apiPool?.end(), pool?.end(), admin.end()]);
    await stopPostgres(server);
    await rm(temporaryDirectory, { force: true, recursive: true });
  }
});

// verify migration rollback and lock behavior
test("0022 rolls back timeout faults and uses a read-compatible table lock", {
  timeout: 300_000,
}, async () => {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "weather-product-locks-"));
  const legacyDirectory = join(temporaryDirectory, "legacy");
  const catalogDirectory = join(temporaryDirectory, "catalog-migrations");
  const lockFaultDirectory = join(temporaryDirectory, "lock-fault");
  const statementFaultDirectory = join(temporaryDirectory, "statement-fault");
  const server = await startPostgres(17, "forecast-product-locks");
  const admin = createTestPool(server);
  let pool;
  let blocker;
  let migrationPool;

  try {
    await copyMigrationsThrough(
      legacyDirectory,
      "0021_adjustment_rolling_registration.sql",
    );
    // retain the exact catalog-only retry boundary
    await copyMigrationsThrough(catalogDirectory, "0022_forecast_product_catalog.sql");
    await createFaultMigrationDirectory(
      lockFaultDirectory,
      // shorten only the injected lock fault
      (sql) => sql.replace("lock_timeout = '10s'", "lock_timeout = '50ms'"),
    );
    await createFaultMigrationDirectory(
      statementFaultDirectory,
      // inject one bounded statement fault
      (sql) => sql
        .replace("statement_timeout = '120s'", "statement_timeout = '50ms'")
        .replace(
          "LOCK TABLE public.weather_records IN SHARE ROW EXCLUSIVE MODE;",
          "LOCK TABLE public.weather_records IN SHARE ROW EXCLUSIVE MODE;\nSELECT pg_sleep(0.1);",
        ),
    );

    await prepareRuntimeRoles(admin);
    pool = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    migrationPool = createTestPool(
      server,
      "weather_test",
      "weather_owner",
      "owner-test",
    );
    assert.equal((await runMigrations(pool, legacyDirectory)).applied.length, 21);

    blocker = await pool.connect();
    await blocker.query("BEGIN");
    await blocker.query("LOCK TABLE weather_records IN ROW EXCLUSIVE MODE");
    await assert.rejects(
      runMigrations(migrationPool, lockFaultDirectory),
      /canceling statement due to lock timeout/u,
    );
    assert.deepEqual(
      (await pool.query(
        `SELECT to_regclass('public.forecast_products') AS relation,
           count(*)::integer AS ledger_count
         FROM schema_migrations
         WHERE name = '0022_forecast_product_catalog.sql'`,
      )).rows[0],
      { ledger_count: 0, relation: null },
    );
    await blocker.query("ROLLBACK");
    blocker.release();
    blocker = undefined;

    await assert.rejects(
      runMigrations(migrationPool, statementFaultDirectory),
      /canceling statement due to statement timeout/u,
    );
    assert.deepEqual(
      (await pool.query(
        `SELECT to_regclass('public.forecast_products') AS relation,
           count(*)::integer AS ledger_count
         FROM schema_migrations
         WHERE name = '0022_forecast_product_catalog.sql'`,
      )).rows[0],
      { ledger_count: 0, relation: null },
    );
    assert.deepEqual(
      (await runMigrations(migrationPool, catalogDirectory)).applied,
      ["0022_forecast_product_catalog.sql"],
    );

    const provider = await pool.query(`
      INSERT INTO providers
        (provider_key, display_name, attribution_label, attribution_url)
      VALUES
        ('catalog-lock-provider', 'Catalog lock provider',
         'Catalog lock provider', 'https://example.com/catalog-lock')
      RETURNING id
    `);
    const source = await createSource(pool, provider.rows[0].id, {
      siteSlug: "catalog-lock",
      sourceKey: "catalog-lock-forecast",
    });
    const runId = await createRun(pool, source, "2026-11-01T00:00:00Z");

    blocker = await pool.connect();
    await blocker.query("BEGIN");
    await blocker.query(
      "LOCK TABLE weather_records IN SHARE ROW EXCLUSIVE MODE",
    );
    assert.equal(
      (await pool.query("SELECT count(*)::integer AS count FROM weather_records"))
        .rows[0].count,
      0,
    );
    const blockedInsert = insertRows(pool, {
      hash: "9".repeat(64),
      pressures: [1000],
      productRunAt: "2026-11-01T00:00:00Z",
      runId,
      sourceId: source.id,
      sourceKind: "forecast",
      validAt: ["2026-11-01T01:00:00Z"],
    });
    const earlyOutcome = await Promise.race([
      // detect premature insertion
      blockedInsert.then(() => "inserted"),
      // retain the bounded observation
      delay(100).then(() => "blocked"),
    ]);
    assert.equal(earlyOutcome, "blocked");
    await blocker.query("COMMIT");
    blocker.release();
    blocker = undefined;
    await blockedInsert;
    assert.deepEqual(
      (await pool.query(
        `SELECT source_id, product_run_at
         FROM forecast_products`,
      )).rows.map(
        // normalize the retained catalog identity
        (row) => ({
          productRunAt: row.product_run_at.toISOString(),
          sourceId: row.source_id,
        }),
      ),
      [{ productRunAt: "2026-11-01T00:00:00.000Z", sourceId: source.id }],
    );
  } finally {
    // release only an owned open transaction
    if (blocker !== undefined) {
      await blocker.query("ROLLBACK").catch(() => undefined);
      blocker.release();
    }

    // ignore owned teardown failures
    await migrationPool?.end().catch(() => undefined);
    await pool?.end().catch(() => undefined);
    await admin.end().catch(() => undefined);
    await stopPostgres(server);
    await rm(temporaryDirectory, { force: true, recursive: true });
  }
});
