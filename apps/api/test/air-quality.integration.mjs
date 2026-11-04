import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { resolve } from "node:path";
import test from "node:test";

import {
  getCurrentWeather,
  listWeatherTrends,
  runMigrations,
} from "@weather/database";
import {
  createRuntimeRoles,
  createTestPool,
  startPostgres,
  stopPostgres,
} from "../../../packages/database/test/postgres-harness.mjs";

const migrationDirectory = resolve(import.meta.dirname, "../../../packages/database/migrations");

// exercise hourly and daily particulate aggregation against PostgreSQL
test("AQI inputs preserve source hours and complete standard-time days", { timeout: 180_000 }, async () => {
  const server = await startPostgres(17, "air-quality");
  const pool = createTestPool(server);

  try {
    await createRuntimeRoles(pool);
    await runMigrations(pool, migrationDirectory);
    const fixture = await createAirQualityFixture(pool);
    await insertCurrentContext(pool, fixture);
    await assertCurrentContext(pool, fixture);
    await insertTrendContext(pool, fixture);
    await assertTrendContext(pool);
  } finally {
    await pool.end();
    await stopPostgres(server);
  }
});

// create one farm source one unrelated source and one weather trend source
async function createAirQualityFixture(pool) {
  const site = await pool.query(`
    INSERT INTO sites (slug, display_name, latitude, longitude, timezone, active)
    VALUES ('ballydidean', 'Ballydidean', 47.95, -122.43, 'America/Los_Angeles', true)
    RETURNING id
  `);
  const ecowitt = await insertProvider(pool, "ecowitt-local");
  const model = await insertProvider(pool, "open-meteo");
  const farmStation = await insertStation(pool, site.rows[0].id, "ballydidean-ecowitt", "physical");
  const otherStation = await insertStation(pool, site.rows[0].id, "other-sensor", "physical");
  const modelStation = await insertStation(pool, site.rows[0].id, "open-meteo-virtual", "virtual");
  const farm = await insertSource(pool, farmStation, ecowitt, "farm-live-v1", "physical_sensor", "a");
  const other = await insertSource(pool, otherStation, ecowitt, "other-live-v1", "physical_sensor", "b");
  const reanalysis = await insertSource(pool, modelStation, model, "reanalysis-v1", "reanalysis", "c");

  return {
    farm,
    farmRun: await insertRun(pool, farm),
    other,
    otherRun: await insertRun(pool, other),
    reanalysis,
    reanalysisRun: await insertRun(pool, reanalysis),
  };
}

// insert one active provider
async function insertProvider(pool, key) {
  const result = await pool.query(
    `
      INSERT INTO providers (provider_key, display_name, attribution_label, attribution_url, active)
      VALUES ($1, $1, $1, 'https://example.test', true)
      RETURNING id
    `,
    [key],
  );
  return result.rows[0].id;
}

// insert one active station
async function insertStation(pool, siteId, slug, kind) {
  const result = await pool.query(
    `
      INSERT INTO stations (site_id, slug, display_name, station_kind, latitude, longitude, active)
      VALUES ($1, $2, $2, $3, 47.95, -122.43, true)
      RETURNING id
    `,
    [siteId, slug, kind],
  );
  return result.rows[0].id;
}

// insert one immutable source
async function insertSource(pool, stationId, providerId, key, kind, fingerprintCharacter) {
  const result = await pool.query(
    `
      INSERT INTO sources (
        station_id,
        provider_id,
        source_key,
        source_kind,
        material_provider_config,
        source_config_fingerprint,
        capabilities,
        cadence_seconds,
        active
      )
      VALUES ($1, $2, $3, $4, '{}'::jsonb, $5, '["current","historical"]'::jsonb, 60, true)
      RETURNING id, source_config_fingerprint, source_kind
    `,
    [stationId, providerId, key, kind, fingerprintCharacter.repeat(64)],
  );
  return result.rows[0];
}

// insert one completed ingestion receipt
async function insertRun(pool, source) {
  const result = await pool.query(
    `
      INSERT INTO ingestion_runs (
        source_id,
        mode,
        requested_start,
        requested_end_exclusive,
        source_config_fingerprint,
        adapter_version,
        started_at,
        deadline_at,
        completed_at,
        state,
        attempts,
        record_count
      )
      VALUES (
        $1,
        'scheduled',
        '2026-07-01T00:00:00Z',
        '2026-08-23T00:00:00Z',
        $2,
        'integration/v1',
        '2026-08-23T00:00:00Z',
        '2026-08-23T01:00:00Z',
        '2026-08-23T00:30:00Z',
        'succeeded',
        1,
        1
      )
      RETURNING id
    `,
    [source.id, source.source_config_fingerprint],
  );
  return result.rows[0].id;
}

// insert one normalized particulate or weather row
async function insertRecord(pool, source, runId, input) {
  const contentHash = createHash("sha256")
    .update(`${String(source.id)}:${input.validAt}:${String(input.minute ?? 0)}`)
    .digest("hex");
  const validAt = new Date(Date.parse(input.validAt) + (input.minute ?? 0) * 60_000).toISOString();
  const providerMetadata = input.sensorPm25 === undefined
    ? { dataset: "integration" }
    : {
      dataset: "integration",
      property_sensors: [{
        channel: 1,
        key: "air-quality-1",
        model: "WH41",
        readings: { pm25MicrogramsPerCubicMeter: input.sensorPm25 },
      }],
    };
  await pool.query(
    `
      INSERT INTO weather_records (
        source_id,
        source_kind,
        valid_at,
        first_ingestion_run_id,
        last_ingestion_run_id,
        first_received_at,
        last_received_at,
        upstream_timezone,
        provider_metadata,
        pm25_micrograms_per_cubic_meter,
        temperature_c,
        content_hash
      )
      VALUES ($1, $2, $3, $4, $4, $3, $3, 'America/Los_Angeles', $5::jsonb, $6, $7, $8)
    `,
    [
      source.id,
      source.source_kind,
      validAt,
      runId,
      JSON.stringify(providerMetadata),
      input.pm25 ?? null,
      input.temperatureC ?? null,
      contentHash,
    ],
  );
}

// populate newest completed hours with a gap and an unrelated source
async function insertCurrentContext(pool, fixture) {
  await Promise.all([
    insertRecord(pool, fixture.farm, fixture.farmRun, {
      pm25: 10,
      sensorPm25: 22,
      validAt: "2026-08-22T04:05:00Z",
    }),
    insertRecord(pool, fixture.farm, fixture.farmRun, {
      pm25: 14,
      sensorPm25: 24,
      validAt: "2026-08-22T04:35:00Z",
    }),
    insertRecord(pool, fixture.farm, fixture.farmRun, {
      pm25: 30,
      sensorPm25: 40,
      validAt: "2026-08-22T02:05:00Z",
    }),
    insertRecord(pool, fixture.farm, fixture.farmRun, {
      pm25: 500,
      sensorPm25: 500,
      validAt: "2026-08-22T06:05:00Z",
    }),
    insertRecord(pool, fixture.other, fixture.otherRun, {
      pm25: 99,
      sensorPm25: 99,
      validAt: "2026-08-22T04:15:00Z",
    }),
  ]);
}

// verify newest-first gaps source isolation and the fixed completed-hour anchor
async function assertCurrentContext(pool, fixture) {
  const rows = await getCurrentWeather(pool, "ballydidean", {
    asOf: "2026-08-22T05:37:00Z",
    sourceId: String(fixture.farm.id),
  });
  const sourceIndex = await pool.query(`
    SELECT indexdef
    FROM pg_indexes
    WHERE indexname = 'weather_records_source_valid_idx'
  `);

  assert.equal(rows.length, 1);
  assert.equal(new Date(rows[0].validAt).toISOString(), "2026-08-22T04:35:00.000Z");
  assert.equal(new Date(rows[0].pm25HourlyThrough).toISOString(), "2026-08-22T04:00:00.000Z");
  assert.deepEqual(rows[0].pm25HourlyConcentrations, [12, null, 30, null, null, null, null, null, null, null, null, null]);
  assert.deepEqual(rows[0].pm25HourlyBySensor, {
    "air-quality-1": [23, null, 40, null, null, null, null, null, null, null, null, null],
  });
  assert.match(sourceIndex.rows[0].indexdef, /\(source_id, valid_at DESC, id DESC\)/u);
}

// populate one complete standard-time day one sparse day and one unfinished day
async function insertTrendContext(pool, fixture) {
  const records = [];

  // weight the first hour once despite its denser sampling
  for (let minute = 0; minute < 10; minute += 1) {
    records.push(insertRecord(pool, fixture.farm, fixture.farmRun, {
      minute,
      pm25: 100,
      validAt: "2026-07-01T08:00:00Z",
    }));
  }
  // provide sixteen more standard-time hours
  for (let hour = 1; hour <= 16; hour += 1) {
    records.push(insertRecord(pool, fixture.farm, fixture.farmRun, {
      pm25: 0,
      validAt: new Date(Date.parse("2026-07-01T08:05:00Z") + hour * 3_600_000).toISOString(),
    }));
  }
  records.push(insertRecord(pool, fixture.farm, fixture.farmRun, {
    pm25: 0,
    validAt: "2026-07-02T07:30:00Z",
  }));

  // leave the next completed day below the eighteen-hour minimum
  for (let hour = 0; hour < 17; hour += 1) {
    records.push(insertRecord(pool, fixture.farm, fixture.farmRun, {
      pm25: 20,
      validAt: new Date(Date.parse("2026-07-02T08:05:00Z") + hour * 3_600_000).toISOString(),
    }));
  }
  // give the current day enough hours but keep its standard-time boundary unfinished
  for (let hour = 0; hour < 18; hour += 1) {
    records.push(insertRecord(pool, fixture.farm, fixture.farmRun, {
      pm25: 30,
      validAt: new Date(Date.parse("2026-08-21T08:05:00Z") + hour * 3_600_000).toISOString(),
    }));
  }
  await Promise.all(records);
  await Promise.all([
    insertRecord(pool, fixture.reanalysis, fixture.reanalysisRun, {
      temperatureC: 15,
      validAt: "2026-07-01T12:00:00Z",
    }),
    insertRecord(pool, fixture.reanalysis, fixture.reanalysisRun, {
      temperatureC: 16,
      validAt: "2026-07-02T12:00:00Z",
    }),
    insertRecord(pool, fixture.reanalysis, fixture.reanalysisRun, {
      temperatureC: 17,
      validAt: "2026-08-21T12:00:00Z",
    }),
  ]);
}

// verify equal hourly weighting completeness and unfinished-day exclusion
async function assertTrendContext(pool) {
  const rows = await listWeatherTrends(pool, {
    from: "2026-07-01T07:00:00Z",
    siteSlug: "ballydidean",
    to: "2026-08-22T05:00:00Z",
  });
  const complete = rows.find(
    // select the July first local trend point
    (row) => new Date(row.validAt).toISOString() === "2026-07-01T07:00:00.000Z",
  );
  const sparse = rows.find(
    // select the July second local trend point
    (row) => new Date(row.validAt).toISOString() === "2026-07-02T07:00:00.000Z",
  );
  const unfinished = rows.find(
    // select the unfinished August trend point
    (row) => new Date(row.validAt).toISOString() === "2026-08-21T07:00:00.000Z",
  );

  assert.ok(complete);
  assert.ok(sparse);
  assert.ok(unfinished);
  assert.ok(Math.abs(complete.pm25MicrogramsPerCubicMeter - 100 / 18) < 0.000_001);
  assert.equal(sparse.pm25MicrogramsPerCubicMeter, null);
  assert.equal(unfinished.pm25MicrogramsPerCubicMeter, null);
}
