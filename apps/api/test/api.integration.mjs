import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";

import {
  bootstrapSiteConfiguration,
  loadSiteConfiguration,
  runMigrations,
} from "@weather/database";

import {
  createDatabaseWeatherReadStore,
  createWeatherApi,
  createWeatherApiServer,
} from "../dist/index.js";
import {
  createRuntimeRoles,
  createTestPool,
  startPostgres,
  stopPostgres,
} from "../../../packages/database/test/postgres-harness.mjs";

const repositoryRoot = resolve(import.meta.dirname, "../../..");
const migrationDirectory = join(repositoryRoot, "packages/database/migrations");
const siteConfigurationPath = join(repositoryRoot, "config/sites/ballydidean.json");

test("real PostgreSQL serves active versioned API reads and exact readiness", { timeout: 300_000 }, async (context) => {
  const postgres = await startPostgres(17, "api");
  const admin = createTestPool(postgres);
  let owner;
  let apiPool;
  let server;

  try {
    await createRuntimeRoles(admin);
    // match the production migration principal
    await admin.query(`
      ALTER ROLE weather_owner LOGIN PASSWORD 'owner-test';
      ALTER DATABASE weather_test OWNER TO weather_owner;
      ALTER SCHEMA public OWNER TO weather_owner;
    `);
    owner = createTestPool(postgres, "weather_test", "weather_owner", "owner-test");
    await runMigrations(owner, migrationDirectory);
    // apply deployed privileges before exercising the private collection view
    execFileSync("docker", ["exec", "-i", postgres.name, "psql", "--set=ON_ERROR_STOP=1",
      "--username", postgres.user, "--dbname", "weather_test"], {
      input: await readFile(join(repositoryRoot, "deploy/postgres/runtime-acl-v2.sql")),
    });
    const configuration = await loadSiteConfiguration(siteConfigurationPath);
    await bootstrapSiteConfiguration(owner, configuration);
    const sources = await owner.query(
      "SELECT active, id, source_key, source_kind, source_config_fingerprint FROM sources ORDER BY source_kind, source_key",
    );
    const currentSource = sources.rows.find(
      // select the current source
      (source) => source.source_kind === "model_current",
    );
    const historySource = sources.rows.find(
      // select the history source
      (source) => source.source_kind === "reanalysis",
    );
    const forecastSource = sources.rows.find(
      // select only the checked live-v4 forecast source
      (source) => source.source_key === "open-meteo-forecast-v4" && source.active,
    );

    assert.ok(currentSource);
    assert.ok(historySource);
    assert.ok(forecastSource);
    const currentRun = await insertSucceededRun(owner, currentSource, "scheduled");
    const historyRun = await insertSucceededRun(owner, historySource, "backfill");
    const forecastContextRun = await insertSucceededRun(
      owner,
      forecastSource,
      "scheduled",
    );
    const forecastRun = await insertSucceededRun(owner, forecastSource, "scheduled");
    await insertRecord(owner, currentSource, currentRun, {
      idSuffix: "current-new",
      temperatureC: 16.2,
      validAt: "2026-08-22T04:50:00.000Z",
    });
    await insertRecord(owner, currentSource, currentRun, {
      idSuffix: "current-old",
      temperatureC: 15.8,
      validAt: "2026-08-22T03:50:00.000Z",
    });
    await insertRecord(owner, historySource, historyRun, {
      idSuffix: "history-tied",
      temperatureC: 14.9,
      validAt: "2026-08-22T04:50:00.000Z",
      windDirectionDegrees: 225,
    });
    // store one complete pre-midnight forecast vintage
    for (let index = 0; index < 6; index += 1) {
      await insertRecord(owner, forecastSource, forecastContextRun, {
        idSuffix: `forecast-context-${String(index)}`,
        pressureHpa: 1008 + index,
        productRunAt: "2026-08-21T06:00:00.000Z",
        temperatureC: 15 + index / 10,
        validAt: new Date(
          Date.parse("2026-08-21T04:00:00.000Z") + index * 3_600_000,
        ).toISOString(),
      });
    }
    await insertRecord(owner, forecastSource, forecastRun, {
      idSuffix: "forecast-first",
      productRunAt: "2026-08-22T05:00:00.000Z",
      temperatureC: 16.8,
      validAt: "2026-08-21T07:00:00.000Z",
    });
    await insertRecord(owner, forecastSource, forecastRun, {
      idSuffix: "forecast-second",
      productRunAt: "2026-08-22T05:00:00.000Z",
      temperatureC: 17.1,
      validAt: "2026-08-22T06:00:00.000Z",
    });
    await owner.query(
      `
        INSERT INTO worker_heartbeats (
          worker_instance,
          last_loop_at,
          last_success_at,
          current_activity,
          worker_version
        )
        VALUES ('api-integration', '2026-08-22T04:55:00.000Z', '2026-08-22T04:50:00.000Z', 'idle', 'test/v1')
      `,
    );
    await insertInactiveMetadata(owner);

    apiPool = createTestPool(postgres, "weather_test", "weather_api", "api-test");
    const store = createDatabaseWeatherReadStore(apiPool, {
      migrationAuthorization: null,
      migrationDirectory,
      release: "integration/v1",
    });
    const handler = createWeatherApi(store, {
      now: () => new Date("2026-08-22T05:00:00.000Z"),
      version: "integration/v1",
    });
    server = createWeatherApiServer(handler);
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const address = server.address();
    assert.ok(address && typeof address === "object");
    const origin = `http://127.0.0.1:${String(address.port)}`;

    // exercise the real aggregate view through the restricted API role
    await context.test("rain collection exposes empty evidence without model activation", async () => {
      const response = await fetch(`${origin}/api/v1/sites/ballydidean/rain-collection`);
      const body = await response.json();
      assert.equal(response.status, 200);
      assert.equal(response.headers.get("cache-control"), "no-store");
      assert.equal(body.data.contractVersion, "rain-prospective-capture/v1");
      assert.equal(body.data.claims, 0);
      assert.equal(body.data.receipts, 0);
      assert.equal(body.data.modelEnabled, false);
      assert.equal(body.data.qualificationEnabled, false);
      assert.equal(body.data.stationAccessAuthorized, true);
      assert.equal(body.data.lastReceiptAt, null);
      assert.doesNotMatch(JSON.stringify(body), /compressed_body|body_sha256|apiKey|password/u);
    });

    await context.test("sites omit every inactive entity", async () => {
      const response = await fetch(`${origin}/api/v1/sites`);
      const body = await response.json();

      assert.equal(response.status, 200);
      assert.deepEqual(body.data.map((site) => site.slug), ["ballydidean"]);
      assert.equal(body.data[0].stations.length, 1);
      assert.deepEqual(
        body.data[0].stations[0].sources.map(
          // retain the complete public source contract
          (source) => source.key,
        ),
        [
          "open-meteo-current-v1",
          "open-meteo-forecast-v4",
          "open-meteo-reanalysis-v1",
        ],
      );
      assert.equal(body.data[0].stations[0].latitude, 47.950429954185445);
      assert.equal(body.data[0].stations[0].longitude, -122.42797012608193);
      assert.doesNotMatch(
        JSON.stringify(body),
        /inactive|material_provider_config|open-meteo-previous-runs-v1/u,
      );
    });

    await context.test("current filters active station and source", async () => {
      const response = await fetch(
        `${origin}/api/v1/sites/ballydidean/current?station=open-meteo-virtual&source=${String(currentSource.id)}`,
      );
      const body = await response.json();

      assert.equal(response.status, 200);
      assert.equal(body.data.length, 1);
      assert.equal(body.data[0].metrics.temperatureC, 16.2);
      assert.equal(body.data[0].metadata.upstream.model, "best_match");
      assert.equal(body.data[0].metadata.provider.dataset, "integration");
      assert.equal(body.data[0].metadata.provider.requestId, undefined);
    });

    await context.test("history filters and cursor pages are stable and complete", async () => {
      const pagedIds = [];
      let cursor;

      // traverse the complete stable cursor sequence
      do {
        const cursorQuery = cursor === undefined
          ? ""
          : `&cursor=${encodeURIComponent(cursor)}`;
        const response = await fetch(
          `${origin}/api/v1/sites/ballydidean/history?limit=1${cursorQuery}`,
        );
        const page = await response.json();
        assert.equal(response.status, 200);
        assert.equal(page.data.length, 1);
        assert.notEqual(
          page.data[0].provenance.sourceKey,
          "open-meteo-previous-runs-v1",
        );
        pagedIds.push(page.data[0].id);
        cursor = page.page.nextCursor ?? undefined;
      } while (cursor !== undefined);

      assert.equal(pagedIds.length, 3);
      assert.equal(new Set(pagedIds).size, 3);

      const filteredResponse = await fetch(
        `${origin}/api/v1/sites/ballydidean/history?station=open-meteo-virtual&source=${String(historySource.id)}&sourceKind=reanalysis&from=2026-08-22T04%3A00%3A00Z&to=2026-08-22T05%3A00%3A00Z`,
      );
      const filtered = await filteredResponse.json();
      assert.equal(filteredResponse.status, 200);
      assert.equal(filtered.data.length, 1);
      assert.equal(filtered.data[0].provenance.sourceKind, "reanalysis");
    });

    await context.test("forecast and trend products expose normalized read models", async () => {
      const forecastResponse = await fetch(
        `${origin}/api/v1/sites/ballydidean/forecast`,
      );
      const forecast = await forecastResponse.json();
      const trendResponse = await fetch(
        `${origin}/api/v1/sites/ballydidean/trends`,
      );
      const trend = await trendResponse.json();

      assert.equal(forecastResponse.status, 200);
      assert.deepEqual(
        forecast.data.map((entry) => entry.metrics.temperatureC),
        [16.8, 17.1],
      );
      assert.deepEqual(
        forecast.pressureContext.map(
          // project the retained context identity and pressure
          (entry) => ({
            pressureHpa: entry.metrics.pressureHpa,
            productRunAt: entry.productRunAt,
            validAt: entry.validAt,
          }),
        ),
        Array.from(
          { length: 6 },
          // retain one complete prior-vintage context window
          (_unused, index) => ({
            pressureHpa: 1008 + index,
            productRunAt: "2026-08-21T06:00:00.000Z",
            validAt: new Date(
              Date.parse("2026-08-21T04:00:00.000Z") + index * 3_600_000,
            ).toISOString(),
          }),
        ),
      );
      assert.equal(
        forecast.data.every(
          // retain only authoritative live-v4 rows
          (entry) => entry.provenance.sourceKey === "open-meteo-forecast-v4",
        ),
        true,
      );
      assert.equal(
        forecast.data.every(
          // retain fail-raw decisions while the registry is inactive
          (entry) => entry.adjustment.state === "disabled",
        ),
        true,
      );
      assert.equal(forecast.data[0].provenance.sourceKind, "forecast");
      assert.equal(trendResponse.status, 200);
      assert.equal(typeof trend.generatedAt, "string");
      assert.equal(trend.data.length > 0, true);
      // prefer complete reanalysis days over current-model fallback
      assert.equal(trend.data.at(-1).metrics.temperatureC, 14.9);
      assert.equal(trend.data.at(-1).metrics.temperatureMaximumC, 14.9);
      assert.equal(trend.data.at(-1).metrics.temperatureMinimumC, 14.9);
      assert.ok(Math.abs(trend.data.at(-1).metrics.windDirectionDegrees + 135) < 0.000_001);
    });

    await context.test("forecast provenance permission faults preserve the raw response", async () => {
      const baselineResponse = await fetch(
        `${origin}/api/v1/sites/ballydidean/forecast`,
      );
      const baselineBody = await baselineResponse.json();

      await owner.query(
        "REVOKE SELECT ON forecast_runtime_provenance_v1 FROM weather_api",
      );

      // restore the production role grant after fault injection
      try {
        const faultResponse = await fetch(
          `${origin}/api/v1/sites/ballydidean/forecast`,
        );
        const faultBody = await faultResponse.json();

        assert.equal(baselineResponse.status, 200);
        assert.equal(faultResponse.status, 200);
        assert.equal(JSON.stringify(faultBody), JSON.stringify(baselineBody));
      } finally {
        await owner.query(
          "GRANT SELECT ON forecast_runtime_provenance_v1 TO weather_api",
        );
      }
    });

    await context.test("deactivated sources disappear from current and history", async () => {
      await owner.query("UPDATE sources SET active = false WHERE id = $1", [
        currentSource.id,
      ]);
      const currentResponse = await fetch(
        `${origin}/api/v1/sites/ballydidean/current`,
      );
      const currentBody = await currentResponse.json();
      const historyResponse = await fetch(
        `${origin}/api/v1/sites/ballydidean/history`,
      );
      const historyBody = await historyResponse.json();

      assert.equal(currentResponse.status, 200);
      assert.equal(
        currentBody.data.some((record) => record.provenance.sourceId === String(currentSource.id)),
        false,
      );
      assert.equal(
        historyBody.data.some((record) => record.provenance.sourceId === String(currentSource.id)),
        false,
      );
      await owner.query("UPDATE sources SET active = true WHERE id = $1", [
        currentSource.id,
      ]);
    });

    await context.test("health rejects unproven trailing migrations and changed known history", async () => {
      const healthyResponse = await fetch(`${origin}/api/v1/health`);
      const healthy = await healthyResponse.json();
      assert.equal(healthyResponse.status, 200);
      assert.deepEqual(healthy.data.migration, {
        status: "current",
        version: "0023_forecast_product_read_index.sql",
      });
      assert.deepEqual(healthy.data.worker, { freshness: "fresh" });

      await owner.query(
        "INSERT INTO schema_migrations (name, checksum) VALUES ($1, $2)",
        ["9999_future.sql", "1".repeat(64)],
      );
      const unprovenResponse = await fetch(`${origin}/api/v1/health`);
      const unproven = await unprovenResponse.json();
      assert.equal(unprovenResponse.status, 503);
      assert.deepEqual(unproven.data.migration, {
        status: "outdated",
        version: null,
      });
      const migrationHistory = await owner.query(
        "SELECT name, checksum FROM schema_migrations ORDER BY name",
      );
      const historySha256 = createHash("sha256")
        .update(
          migrationHistory.rows
            // serialize exact ledger rows
            .map((migration) => `${migration.name}:${migration.checksum}\n`)
            .join(""),
        )
        .digest("hex");
      const authorizedStore = createDatabaseWeatherReadStore(apiPool, {
        migrationAuthorization: {
          historySha256,
          release: "2026.08.22-1",
        },
        migrationDirectory,
        release: "2026.08.22-1",
      });
      const authorizedHandler = createWeatherApi(authorizedStore, {
        // freeze compatibility health time
        now: () => new Date("2026-08-22T05:00:00.000Z"),
        version: "2026.08.22-1",
      });
      const authorizedResponse = await authorizedHandler(
        new Request("http://weather.test/api/v1/health"),
      );
      const authorized = await authorizedResponse.json();
      assert.equal(authorizedResponse.status, 200);
      assert.deepEqual(authorized.data.migration, {
        status: "current",
        version: "0023_forecast_product_read_index.sql",
      });

      const ledger = await owner.query(
        "SELECT checksum FROM schema_migrations WHERE name = '0001_initial_weather.sql'",
      );
      await owner.query(
        "UPDATE schema_migrations SET checksum = $1 WHERE name = '0001_initial_weather.sql'",
        ["0".repeat(64)],
      );
      const unhealthyResponse = await fetch(`${origin}/api/v1/health`);
      const unhealthy = await unhealthyResponse.json();
      assert.equal(unhealthyResponse.status, 503);
      assert.deepEqual(unhealthy.data.migration, {
        status: "outdated",
        version: null,
      });
      assert.doesNotMatch(JSON.stringify(unhealthy), /checksum|postgres|password|error/u);
      await owner.query(
        "UPDATE schema_migrations SET checksum = $1 WHERE name = '0001_initial_weather.sql'",
        [ledger.rows[0].checksum],
      );
      await owner.query(
        "DELETE FROM schema_migrations WHERE name = '9999_future.sql'",
      );
    });

    await context.test("API role cannot mutate stored data", async () => {
      await assert.rejects(
        () => apiPool.query("UPDATE sites SET display_name = 'denied'"),
        (error) => error?.code === "42501",
      );
    });
  } finally {
    // close the HTTP listener first
    if (server !== undefined) {
      server.close();
      await once(server, "close");
    }

    await Promise.all([
      apiPool?.end(),
      owner?.end(),
      admin.end(),
    ]);
    await stopPostgres(postgres);
  }
});

// insert one finalized ingestion run
async function insertSucceededRun(pool, source, mode) {
  // preserve the real live-v4 adapter identity
  const adapterVersion = source.source_key === "open-meteo-forecast-v4"
    ? "open-meteo-forecast-daily/v4"
    : "integration/v1";
  const result = await pool.query(
    `
      INSERT INTO ingestion_runs (
        source_id,
        mode,
        requested_start,
        requested_end_exclusive,
        source_config_fingerprint,
        adapter_version,
        chunk_plan_version,
        started_at,
        deadline_at,
        completed_at,
        state,
        attempts,
        record_count
      )
      VALUES (
        $1,
        $2,
        '2026-08-22T03:00:00.000Z',
        '2026-08-22T05:00:00.000Z',
        $3,
        $5,
        $4,
        '2026-08-22T04:00:00.000Z',
        '2026-08-22T05:10:00.000Z',
        '2026-08-22T04:55:00.000Z',
        'succeeded',
        1,
        2
      )
      RETURNING id
    `,
    [
      source.id,
      mode,
      source.source_config_fingerprint,
      mode === "backfill" ? "integration-plan/v1" : null,
      adapterVersion,
    ],
  );
  return result.rows[0].id;
}

// insert one normalized integration record
async function insertRecord(pool, source, runId, input) {
  const contentHash = Buffer.from(input.idSuffix).toString("hex").padEnd(64, "0").slice(0, 64);
  // preserve the checked live-v4 dataset identity
  const dataset = source.source_key === "open-meteo-forecast-v4"
    ? "forecast"
    : "integration";
  await pool.query(
    `
      INSERT INTO weather_records (
        source_id,
        source_kind,
        valid_at,
        product_run_at,
        first_ingestion_run_id,
        last_ingestion_run_id,
        first_received_at,
        last_received_at,
        upstream_timezone,
        upstream_model,
        device_vendor,
        device_model,
        quality_metadata,
        provider_metadata,
        temperature_c,
        pressure_hpa,
        wind_direction_degrees,
        content_hash
      )
      VALUES (
        $1,
        $2,
        $3,
        $7,
        $4,
        $4,
        $3::timestamptz + interval '1 minute',
        $3::timestamptz + interval '1 minute',
        'America/Los_Angeles',
        'best_match',
        'Open-Meteo',
        'virtual-grid',
        '{"confidence_percent":93,"flags":["integration"],"status":"accepted"}'::jsonb,
        jsonb_build_object(
          'dataset', $9::text,
          'elevation_m', 17,
          'request_id', 'private'
        ),
        $5,
        $10,
        $8,
        $6
      )
    `,
    [
      source.id,
      source.source_kind,
      input.validAt,
      runId,
      input.temperatureC,
      contentHash,
      input.productRunAt ?? null,
      input.windDirectionDegrees ?? null,
      dataset,
      input.pressureHpa ?? null,
    ],
  );
}

// insert metadata that must never cross the active boundary
async function insertInactiveMetadata(pool) {
  await pool.query(
    `
      WITH inserted_site AS (
        INSERT INTO sites (slug, display_name, latitude, longitude, timezone, active)
        VALUES ('inactive-site', 'Inactive site', 47, -122, 'UTC', false)
        RETURNING id
      ),
      inserted_station AS (
        INSERT INTO stations (
          site_id,
          slug,
          display_name,
          station_kind,
          latitude,
          longitude,
          active
        )
        SELECT id, 'inactive-station', 'Inactive station', 'virtual', 47, -122, false
        FROM inserted_site
        RETURNING id
      ),
      inserted_provider AS (
        INSERT INTO providers (
          provider_key,
          display_name,
          attribution_label,
          attribution_url,
          active
        )
        VALUES ('inactive-provider', 'Inactive provider', 'Inactive', 'https://example.test/', false)
        RETURNING id
      )
      INSERT INTO sources (
        station_id,
        provider_id,
        source_key,
        source_kind,
        material_provider_config,
        source_config_fingerprint,
        capabilities,
        active
      )
      SELECT
        inserted_station.id,
        inserted_provider.id,
        'inactive-source',
        'model_current',
        '{}'::jsonb,
        $1,
        '["current"]'::jsonb,
        false
      FROM inserted_station, inserted_provider
    `,
    ["a".repeat(64)],
  );
}
