import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { promisify } from "node:util";

import { createNormalizedWeatherRecord } from "@weather/domain";

import {
  abandonExpiredRuns,
  acquireSourceSession,
  bootstrapEcowittConfiguration,
  bootstrapSiteConfiguration,
  bootstrapTempestConfiguration,
  completeBackfillIngestion,
  completeScheduledIngestion,
  discoverDueSources,
  failIngestionRun,
  getCurrentWeather,
  getDailyPrecipitation,
  getForecastPressureContext,
  getScheduledCheckpoint,
  getWeatherForecast,
  hasSuccessfulBackfillChunk,
  listActiveSites,
  listForecastObservationInputs,
  loadEcowittConfiguration,
  loadSiteConfiguration,
  loadTempestConfiguration,
  runMigrations,
  startIngestionRun,
  updateWorkerHeartbeat,
  verifyMigrationReadiness,
} from "../dist/index.js";
import {
  createTestPool,
  prepareRuntimeRoles,
  startPostgres,
  stopPostgres,
} from "./postgres-harness.mjs";

const repositoryRoot = resolve(import.meta.dirname, "../../..");
const ecowittConfigurationPath = join(repositoryRoot, "config/ecowitt/gateways.json");
const migrationDirectory = join(repositoryRoot, "packages/database/migrations");
const siteConfigurationPath = join(repositoryRoot, "config/sites/ballydidean.json");
const tempestConfigurationPath = join(repositoryRoot, "config/tempest/stations.json");
const executeFile = promisify(execFile);

// exercise the complete Phase 2 PostgreSQL contract
test(
  "I-DB-01 through I-DB-27 PostgreSQL foundation",
  { timeout: 600_000 },
  async (context) => {
    const server = await startPostgres(17, "main");
    const adminPool = createTestPool(server);
    await runRuntimeRoleBootstrap(server);
    const pool = createTestPool(
      server,
      "weather_test",
      "weather_owner",
      "owner-test",
    );
    let configuration;
    let bootstrap;
    let currentSource;
    let forecastSource;
    let reanalysisSource;
    let currentFirstRunId;
    let backfillRunId;
    let successfulBackfillIdentity;

    try {
      // verify the complete 0022 migration and checksum ledger
      await context.test("I-DB-01 empty database migrates with checksums", async () => {
        const result = await runMigrations(pool, migrationDirectory);
        const ledger = await pool.query(
          "SELECT name, checksum FROM schema_migrations ORDER BY name",
        );

        assert.deepEqual(result.applied, [
          "0001_initial_weather.sql",
          "0002_worker_migration_readiness.sql",
          "0003_ecowitt_measurements.sql",
          "0004_tempest_metadata.sql",
          "0005_source_supersession.sql",
          "0006_station_coordinates.sql",
          "0007_tide_sources.sql",
          "0008_ecowitt_property_sensors.sql",
          "0009_forecast_anchor_records.sql",
          "0010_forecast_training_export.sql",
          "0011_forecast_runtime_provenance.sql",
          "0012_hide_archive_only_forecasts_from_live_reads.sql",
          "0013_ecmwf_temperature_canary.sql",
          "0014_rain_collection.sql",
          "0015_rain_station_access.sql",
          "0016_rain_adjustment.sql",
          "0017_adjustment_evaluation_export.sql",
          "0018_adjustment_maintenance_v2.sql",
          "0019_adjustment_maintenance_recurring.sql",
          "0020_adjustment_revision_frontier.sql",
          "0021_adjustment_rolling_registration.sql",
          "0022_forecast_product_catalog.sql",
        ]);
        assert.equal(result.serverVersionNum >= 150_000, true);
        assert.equal(ledger.rowCount, 22);
        // require every migration checksum
        for (const row of ledger.rows) {
          assert.match(row.checksum, /^[a-f0-9]{64}$/u);
        }
      });

      // verify rerun and changed-checksum failure
      await context.test("I-DB-02 rerun is a no-op and tampering fails closed", async () => {
        const rerun = await runMigrations(pool, migrationDirectory);
        const directory = await mkdtemp(join(tmpdir(), "weather-migrations-"));

        try {
          const source = await readFile(
            join(migrationDirectory, "0001_initial_weather.sql"),
            "utf8",
          );
          await writeFile(
            join(directory, "0001_initial_weather.sql"),
            `${source}\n-- tampered copy\n`,
          );
          assert.deepEqual(rerun.current, [
            "0001_initial_weather.sql",
            "0002_worker_migration_readiness.sql",
            "0003_ecowitt_measurements.sql",
            "0004_tempest_metadata.sql",
            "0005_source_supersession.sql",
            "0006_station_coordinates.sql",
            "0007_tide_sources.sql",
            "0008_ecowitt_property_sensors.sql",
            "0009_forecast_anchor_records.sql",
            "0010_forecast_training_export.sql",
            "0011_forecast_runtime_provenance.sql",
            "0012_hide_archive_only_forecasts_from_live_reads.sql",
            "0013_ecmwf_temperature_canary.sql",
            "0014_rain_collection.sql",
            "0015_rain_station_access.sql",
            "0016_rain_adjustment.sql",
            "0017_adjustment_evaluation_export.sql",
            "0018_adjustment_maintenance_v2.sql",
            "0019_adjustment_maintenance_recurring.sql",
            "0020_adjustment_revision_frontier.sql",
            "0021_adjustment_rolling_registration.sql",
            "0022_forecast_product_catalog.sql",
          ]);
          await assert.rejects(
            () => runMigrations(pool, directory),
            /migration checksum mismatch/u,
          );
          await writeFile(join(directory, "0001_initial_weather.sql"), source);
          await writeFile(
            join(directory, "0000_retroactive.sql"),
            "SELECT 1;\n",
          );
          await assert.rejects(
            () => runMigrations(pool, directory),
            /migration history diverges/u,
          );
        } finally {
          await rm(directory, { force: true, recursive: true });
        }
      });

      // verify concurrent serialization through migration 0022
      await context.test("I-DB-03 concurrent migrators serialize", async () => {
        const database = `weather_concurrent_${process.pid}`;
        await adminPool.query(`CREATE DATABASE ${database} OWNER weather_owner`);
        const left = createTestPool(server, database, "weather_owner", "owner-test");
        const right = createTestPool(server, database, "weather_owner", "owner-test");

        try {
          const [first, second] = await Promise.all([
            runMigrations(left, migrationDirectory),
            runMigrations(right, migrationDirectory),
          ]);
          assert.equal(first.applied.length + second.applied.length, 22);
          assert.equal(first.current.length + second.current.length, 22);
        } finally {
          await Promise.all([left.end(), right.end()]);
          await adminPool.query(`DROP DATABASE ${database}`);
        }
      });

      // verify idempotent Ballydidean bootstrap
      await context.test("I-DB-04 Ballydidean bootstrap is idempotent", async () => {
        configuration = await loadSiteConfiguration(siteConfigurationPath);
        bootstrap = await bootstrapSiteConfiguration(pool, configuration);
        const second = await bootstrapSiteConfiguration(pool, configuration);
        const counts = await pool.query(`
          SELECT
            (SELECT count(*)::integer FROM sites) AS sites,
            (SELECT count(*)::integer FROM stations) AS stations,
            (SELECT count(*)::integer FROM providers) AS providers,
            (SELECT count(*)::integer FROM sources) AS sources
        `);

        assert.deepEqual(second, bootstrap);
        assert.deepEqual(counts.rows[0], {
          providers: 1,
          sites: 1,
          sources: 7,
          stations: 1,
        });
        const expectedPublicSourceKeys = [
          "open-meteo-current-v1",
          "open-meteo-forecast-v4",
          "open-meteo-reanalysis-v1",
        ];
        const publicSources = await listActiveSites(pool);
        assert.deepEqual(
          publicSources.map(
            // retain exact public source identities
            (source) => source.sourceKey,
          ),
          expectedPublicSourceKeys,
        );
        assert.equal(
          publicSources.some(
            // reject the archive-only source from discovery
            (source) => source.sourceKey === "open-meteo-previous-runs-v1",
          ),
          false,
        );
        const legacyPublicSources = await pool.query(`
          SELECT s.source_key
          FROM sites si
          JOIN stations st ON st.site_id = si.id AND st.active
          JOIN sources s ON s.station_id = st.id AND s.active
          JOIN providers p ON p.id = s.provider_id AND p.active
          WHERE si.active
            AND weather_source_is_current(s.id)
          ORDER BY si.slug, st.slug, s.source_key
        `);
        assert.deepEqual(
          legacyPublicSources.rows.map(
            // prove rollback discovery compatibility
            (source) => source.source_key,
          ),
          expectedPublicSourceKeys,
        );
        const station = await pool.query(
          "SELECT latitude, longitude FROM stations WHERE id = $1",
          [bootstrap.stationId],
        );
        assert.deepEqual(station.rows[0], {
          latitude: 47.950429954185445,
          longitude: -122.42797012608193,
        });
        const sources = await pool.query(
          "SELECT active, id, source_key, source_kind, source_config_fingerprint FROM sources ORDER BY source_kind, source_key",
        );
        currentSource = sources.rows.find(
          (source) => source.source_kind === "model_current",
        );
        reanalysisSource = sources.rows.find(
          (source) => source.source_kind === "reanalysis",
        );
        forecastSource = sources.rows.find(
          // select the checked active v4 source
          (source) => source.source_key === "open-meteo-forecast-v4" && source.active,
        );
        assert.ok(currentSource);
        assert.ok(forecastSource);
        assert.ok(reanalysisSource);
      });

      // verify schema constraints
      await context.test("I-DB-05 coordinates timezone kinds states and FKs reject invalid rows", async () => {
        await assert.rejects(
          () =>
            pool.query(
              "INSERT INTO sites (slug, display_name, latitude, longitude, timezone) VALUES ('invalid-lat', 'invalid', 91, 0, 'UTC')",
            ),
          hasDatabaseCode("23514"),
        );
        await assert.rejects(
          () =>
            pool.query(
              `
                INSERT INTO sources (
                  station_id,
                  provider_id,
                  source_key,
                  source_kind,
                  material_provider_config,
                  source_config_fingerprint,
                  capabilities
                )
                VALUES ($1, $2, 'invalid-capability', 'model_current', '{}'::jsonb, $3, '["invented"]'::jsonb)
              `,
              [bootstrap.stationId, bootstrap.providerId, "e".repeat(64)],
            ),
          hasDatabaseCode("23514"),
        );
        await assert.rejects(
          () =>
            pool.query(
              "INSERT INTO sites (slug, display_name, latitude, longitude, timezone) VALUES ('invalid-zone', 'invalid', 0, 0, 'Mars/Olympus')",
            ),
          hasDatabaseCode("23503"),
        );
        await assert.rejects(
          () =>
            pool.query(
              "INSERT INTO stations (site_id, slug, display_name, station_kind, latitude, longitude) VALUES (999999, 'missing', 'missing', 'virtual', 0, 0)",
            ),
          hasDatabaseCode("23503"),
        );
        await assert.rejects(
          () =>
            pool.query(
              `
                INSERT INTO sources (
                  station_id,
                  provider_id,
                  source_key,
                  source_kind,
                  material_provider_config,
                  source_config_fingerprint,
                  capabilities
                )
                VALUES ($1, $2, 'invalid-kind', 'observation', '{}'::jsonb, $3, '["current"]'::jsonb)
              `,
              [bootstrap.stationId, bootstrap.providerId, "f".repeat(64)],
            ),
          hasDatabaseCode("23514"),
        );
      });

      // verify read-only API role
      await context.test("I-DB-06 API role reads but cannot mutate or DDL", async () => {
        const apiPool = createTestPool(server, "weather_test", "weather_api", "api-test");

        try {
          assert.equal((await apiPool.query("SELECT * FROM sites")).rowCount, 1);
          const runtimeProvenance = await apiPool.query(
            "SELECT * FROM forecast_runtime_provenance_v1",
          );
          assert.equal(runtimeProvenance.rowCount, 0);
          assert.deepEqual(
            runtimeProvenance.fields.map(
              // retain the exact least-privilege projection
              (field) => field.name,
            ),
            [
              "weather_record_id",
              "source_id",
              "source_key",
              "source_config_fingerprint",
              "adapter_version",
              "contract_epoch",
            ],
          );
          await assert.rejects(
            () => apiPool.query("DELETE FROM sites"),
            hasDatabaseCode("42501"),
          );
          await assert.rejects(
            () => apiPool.query("CREATE TABLE api_escape (id integer)"),
            hasDatabaseCode("42501"),
          );
          await assert.rejects(
            () => apiPool.query("SELECT material_provider_config FROM sources"),
            hasDatabaseCode("42501"),
          );
          await assert.rejects(
            () => apiPool.query("SELECT source_config_fingerprint FROM sources"),
            hasDatabaseCode("42501"),
          );
          await assert.rejects(
            () => apiPool.query("SELECT * FROM ingestion_runs"),
            hasDatabaseCode("42501"),
          );
          await assert.rejects(
            () => apiPool.query("SELECT * FROM forecast_anchor_records"),
            hasDatabaseCode("42501"),
          );
          await assert.rejects(
            () => apiPool.query("SELECT * FROM forecast_training_export_rows_v1"),
            hasDatabaseCode("42501"),
          );
          await assert.rejects(
            () => apiPool.query("SELECT * FROM forecast_training_export_manifest_v1"),
            hasDatabaseCode("42501"),
          );
          await assert.rejects(
            () => apiPool.query("CREATE TEMP TABLE api_temp_escape (id integer)"),
            hasDatabaseCode("42501"),
          );
        } finally {
          await apiPool.end();
        }
      });

      // verify write-limited ingestion role
      await context.test("I-DB-07 ingest role writes operations but cannot DDL or roles", async () => {
        const ingestPool = createTestPool(
          server,
          "weather_test",
          "weather_ingest",
          "ingest-test",
        );

        try {
          const knownMigration = await pool.query(
            "SELECT checksum FROM schema_migrations WHERE name = '0001_initial_weather.sql'",
          );
          assert.deepEqual(
            await verifyMigrationReadiness(ingestPool, migrationDirectory),
            { version: "0022_forecast_product_catalog.sql" },
          );
          try {
            // reject unproven candidate history
            await pool.query(
              "INSERT INTO schema_migrations (name, checksum) VALUES ('9999_candidate_only.sql', $1)",
              ["3".repeat(64)],
            );
            await assert.rejects(
              () => verifyMigrationReadiness(ingestPool, migrationDirectory),
              /migration history diverges/u,
            );
            const history = await pool.query(
              "SELECT name, checksum FROM schema_migrations ORDER BY name",
            );
            const historySha256 = createHash("sha256")
              .update(
                history.rows
                  // serialize exact ledger rows
                  .map((migration) => `${migration.name}:${migration.checksum}\n`)
                  .join(""),
              )
              .digest("hex");
            assert.deepEqual(
              await verifyMigrationReadiness(ingestPool, migrationDirectory, {
                authorization: {
                  historySha256,
                  release: "2026.08.22-1",
                },
                release: "2026.08.22-1",
              }),
              { version: "0022_forecast_product_catalog.sql" },
            );
            await pool.query(
              "UPDATE schema_migrations SET checksum = $1 WHERE name = '0001_initial_weather.sql'",
              ["0".repeat(64)],
            );
            await assert.rejects(
              () =>
                verifyMigrationReadiness(ingestPool, migrationDirectory, {
                  authorization: {
                    historySha256,
                    release: "2026.08.22-1",
                  },
                  release: "2026.08.22-1",
                }),
              /migration checksum mismatch/u,
            );
          } finally {
            // restore the shared migration ledger
            await pool.query(
              "UPDATE schema_migrations SET checksum = $1 WHERE name = '0001_initial_weather.sql'",
              [knownMigration.rows[0].checksum],
            );
            await pool.query(
              "DELETE FROM schema_migrations WHERE name = '9999_candidate_only.sql'",
            );
          }
          await ingestPool.query(
            `
              INSERT INTO worker_heartbeats (
                worker_instance,
                last_loop_at,
                worker_version
              )
              VALUES ('role-test', clock_timestamp(), 'test/v1')
            `,
          );
          assert.equal((await ingestPool.query("SELECT * FROM sources")).rowCount, 7);
          await assert.rejects(
            () => ingestPool.query("ALTER TABLE weather_records ADD COLUMN escaped text"),
            hasDatabaseCode("42501"),
          );
          await assert.rejects(
            () => ingestPool.query("CREATE ROLE escaped"),
            hasDatabaseCode("42501"),
          );
          await assert.rejects(
            () =>
              ingestPool.query(
                "UPDATE weather_records SET first_received_at = clock_timestamp()",
              ),
            hasDatabaseCode("42501"),
          );
        } finally {
          await ingestPool.end();
        }
      });

      // verify PostgreSQL version compatibility
      await context.test("I-DB-08 PostgreSQL 15 migrates and PostgreSQL 14 fails preflight", async () => {
        const version15 = await startPostgres(15, "pg15");
        const version14 = await startPostgres(14, "pg14");
        const admin15 = createTestPool(version15);
        let pool15;
        const pool14 = createTestPool(version14);

        try {
          await prepareRuntimeRoles(admin15);
          pool15 = createTestPool(
            version15,
            "weather_test",
            "weather_owner",
            "owner-test",
          );
          const migrated = await runMigrations(pool15, migrationDirectory);
          assert.equal(migrated.serverVersionNum >= 150_000, true);
          await assert.rejects(
            () => runMigrations(pool14, migrationDirectory),
            /PostgreSQL 15 or newer/u,
          );
          const ledger14 = await pool14.query(
            "SELECT to_regclass('public.schema_migrations') AS ledger",
          );
          assert.equal(ledger14.rows[0].ledger, null);
        } finally {
          await Promise.all([pool15?.end(), admin15.end(), pool14.end()]);
          await Promise.all([stopPostgres(version15), stopPostgres(version14)]);
        }
      });

      // verify immutable source semantics
      await context.test("I-DB-09 source material is immutable but cadence and active are mutable", async () => {
        await assert.rejects(
          () =>
            pool.query(
              "UPDATE sources SET material_provider_config = '{\"changed\":true}'::jsonb WHERE id = $1",
              [currentSource.id],
            ),
          hasDatabaseCode("23514"),
        );
        await pool.query(
          "UPDATE sources SET active = false, cadence_seconds = 1800 WHERE id = $1",
          [currentSource.id],
        );
        await pool.query(
          "UPDATE sources SET active = true, cadence_seconds = 900 WHERE id = $1",
          [currentSource.id],
        );
      });

      // verify active successor visibility
      await context.test("versioned successors hide replaced sources", async () => {
        const inserted = await pool.query(
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
            VALUES ($1, $2, 'open-meteo-current-v2', 'model_current',
              jsonb_build_object('supersedesSourceKey', 'open-meteo-current-v1'),
              $3, '["current"]'::jsonb, 900, true)
            RETURNING id
          `,
          [bootstrap.stationId, bootstrap.providerId, "9".repeat(64)],
        );
        const successorId = inserted.rows[0].id;

        try {
          const visible = await listActiveSites(pool);
          const due = await discoverDueSources(
            pool,
            "2099-01-01T00:00:00.000Z",
          );
          assert.equal(
            visible.some((source) => source.sourceId === currentSource.id),
            false,
          );
          assert.equal(due.some((source) => source.id === currentSource.id), false);
          assert.equal(due.some((source) => source.id === successorId), true);
        } finally {
          // remove the isolated successor fixture
          await pool.query("DELETE FROM sources WHERE id = $1", [successorId]);
        }
      });

      // verify multi-source storage and first scheduled success
      await context.test("I-DB-10 two sources persist the same valid instant", async () => {
        const currentSession = await requireSession(pool, currentSource.id);
        const reanalysisSession = await requireSession(pool, reanalysisSource.id);

        try {
          const currentRun = await createRun(
            currentSession,
            currentSource,
            "scheduled",
            "2026-08-20T00:00:00.000Z",
            "2026-08-20T00:15:00.000Z",
          );
          currentFirstRunId = currentRun.id;
          await completeScheduledIngestion(currentSession, {
            attempts: 1,
            expectedCheckpointVersion: null,
            lastValidAt: "2026-08-20T00:00:00.000Z",
            providerCursor: { dataset: "current" },
            records: [
              makeRecord(currentSource.id, "model_current", "2026-08-20T00:00:00.000Z"),
            ],
            runId: currentRun.id,
            windowEndExclusive: "2026-08-20T00:15:00.000Z",
            windowStart: "2026-08-20T00:00:00.000Z",
          });
          successfulBackfillIdentity = makeChunkIdentity(
            reanalysisSource,
            "2026-08-20T00:00:00.000Z",
            "2026-08-20T09:00:00.000Z",
          );
          const backfillRun = await createRun(
            reanalysisSession,
            reanalysisSource,
            "backfill",
            successfulBackfillIdentity.intervalStart,
            successfulBackfillIdentity.intervalEndExclusive,
          );
          backfillRunId = backfillRun.id;
          // cross the bounded insert batch boundary
          const minuteRecords = Array.from({ length: 2_001 }, (_unused, index) =>
            makeRecord(
              reanalysisSource.id,
              "reanalysis",
              new Date(Date.parse("2026-08-20T00:00:00.000Z") + index * 60_000)
                .toISOString(),
            ),
          );
          await completeBackfillIngestion(reanalysisSession, {
            attempts: 1,
            identity: successfulBackfillIdentity,
            records: minuteRecords,
            runId: backfillRun.id,
          });
          const rows = await pool.query(
            `
              SELECT
                source_id,
                black_globe_temperature_c,
                pm25_micrograms_per_cubic_meter,
                precipitation_rate_mm_per_hour,
                soil_electrical_conductivity_us_cm,
                soil_moisture_percent,
                solar_radiation_wm2,
                uv_index,
                water_level_m,
                wet_bulb_globe_temperature_c
              FROM weather_records
              WHERE valid_at = '2026-08-20T00:00:00.000Z'
              ORDER BY source_id
            `,
          );
          assert.equal(rows.rowCount, 2);
          assert.deepEqual(
            {
              blackGlobeTemperatureC: rows.rows[0].black_globe_temperature_c,
              pm25MicrogramsPerCubicMeter:
                rows.rows[0].pm25_micrograms_per_cubic_meter,
              precipitationRateMmPerHour:
                rows.rows[0].precipitation_rate_mm_per_hour,
              soilElectricalConductivityMicrosiemensPerCm:
                rows.rows[0].soil_electrical_conductivity_us_cm,
              soilMoisturePercent: rows.rows[0].soil_moisture_percent,
              solarRadiationWm2: rows.rows[0].solar_radiation_wm2,
              uvIndex: rows.rows[0].uv_index,
              waterLevelM: rows.rows[0].water_level_m,
              wetBulbGlobeTemperatureC:
                rows.rows[0].wet_bulb_globe_temperature_c,
            },
            {
              blackGlobeTemperatureC: 18,
              pm25MicrogramsPerCubicMeter: 7,
              precipitationRateMmPerHour: 0,
              soilElectricalConductivityMicrosiemensPerCm: 420,
              soilMoisturePercent: 34,
              solarRadiationWm2: 320,
              uvIndex: 2,
              waterLevelM: null,
              wetBulbGlobeTemperatureC: 11,
            },
          );
        } finally {
          await Promise.all([currentSession.release(), reanalysisSession.release()]);
        }
      });

      await context.test("compact forecast observation input retains one fixed snapshot", async () => {
        const input = await listForecastObservationInputs(
          pool,
          "2026-08-20T01:00:00.000Z",
        );
        assert.notEqual(input, null);
        assert.equal(input.siteSlug, "ballydidean");
        assert.equal(input.timezone, "America/Los_Angeles");
        assert.equal(input.records.length, 1);
        assert.equal(input.records[0].sourceKind, "model_current");
        assert.equal(input.records[0].sourceId, currentSource.id);
        assert.equal(input.records[0].validAt.toISOString(), "2026-08-20T00:00:00.000Z");
      });

      // verify run fingerprint composite FK
      await context.test("I-DB-11 run fingerprint must match its source", async () => {
        await assert.rejects(
          () =>
            pool.query(
              `
                INSERT INTO ingestion_runs (
                  source_id,
                  mode,
                  requested_start,
                  requested_end_exclusive,
                  source_config_fingerprint,
                  adapter_version,
                  deadline_at
                )
                VALUES ($1, 'scheduled', clock_timestamp(), clock_timestamp() + interval '1 minute', $2, 'test/v1', clock_timestamp() + interval '2 minutes')
              `,
              [currentSource.id, reanalysisSource.source_config_fingerprint],
            ),
          hasDatabaseCode("23503"),
        );
        await assert.rejects(
          () =>
            insertRawRecord(pool, {
              firstRunId: currentFirstRunId,
              lastRunId: currentFirstRunId,
              providerMetadata: { unrecognized: true },
              sourceId: currentSource.id,
              sourceKind: "model_current",
              validAt: "2026-08-20T04:00:00.000Z",
            }),
          hasDatabaseCode("23514"),
        );
        await assert.rejects(
          () =>
            insertRawRecord(pool, {
              firstRunId: currentFirstRunId,
              lastRunId: currentFirstRunId,
              soilMoisturePercent: 101,
              sourceId: currentSource.id,
              sourceKind: "model_current",
              validAt: "2026-08-20T04:10:00.000Z",
            }),
          hasDatabaseCode("23514"),
        );
        await assert.rejects(
          () =>
            insertRawRecord(pool, {
              firstRunId: currentFirstRunId,
              lastRunId: currentFirstRunId,
              sourceId: currentSource.id,
              sourceKind: "model_current",
              uvIndex: 21,
              validAt: "2026-08-20T04:20:00.000Z",
            }),
          hasDatabaseCode("23514"),
        );
      });

      // verify record provenance composite FKs
      await context.test("I-DB-12 record kind and run linkage cannot cross sources", async () => {
        await assert.rejects(
          () =>
            insertRawRecord(pool, {
              firstRunId: currentFirstRunId,
              lastRunId: currentFirstRunId,
              sourceId: currentSource.id,
              sourceKind: "reanalysis",
              validAt: "2026-08-20T02:00:00.000Z",
            }),
          hasDatabaseCode("23503"),
        );
        await assert.rejects(
          () =>
            insertRawRecord(pool, {
              firstRunId: backfillRunId,
              lastRunId: backfillRunId,
              sourceId: currentSource.id,
              sourceKind: "model_current",
              validAt: "2026-08-20T03:00:00.000Z",
            }),
          hasDatabaseCode("23503"),
        );
      });

      // lock the raw v4 read query before additive storage work
      await context.test("legacy forecast reads only the newest product in horizon order and limit", async () => {
        assert.equal(
          forecastSource.source_config_fingerprint,
          "ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4",
        );
        const forecastSession = await requireSession(pool, forecastSource.id);

        try {
          const olderRun = await createRun(
            forecastSession,
            forecastSource,
            "scheduled",
            "2026-08-22T06:00:00.000Z",
            "2026-08-22T07:00:00.000Z",
          );
          await completeScheduledIngestion(forecastSession, {
            attempts: 1,
            expectedCheckpointVersion: null,
            lastValidAt: "2026-08-22T09:00:00.000Z",
            providerCursor: { product_run_at: "2026-08-22T04:00:00.000Z" },
            records: Array.from(
              { length: 6 },
              // create one exact complete midnight pressure window
              (_unused, index) => makeRecord(
                forecastSource.id,
                "forecast",
                new Date(
                  Date.parse("2026-08-22T04:00:00.000Z") + index * 3_600_000,
                ).toISOString(),
                {
                  pressureHpa: 1000 + index,
                  productRunAt: "2026-08-22T04:00:00.000Z",
                  providerDataset: "forecast",
                  temperatureC: -10,
                  upstreamModel: "best_match",
                },
              ),
            ),
            runId: olderRun.id,
            windowEndExclusive: "2026-08-22T07:00:00.000Z",
            windowStart: "2026-08-22T06:00:00.000Z",
          });
          const checkpoint = await getScheduledCheckpoint(forecastSession);
          assert.equal(checkpoint?.version, 1);
          const newestRun = await createRun(
            forecastSession,
            forecastSource,
            "scheduled",
            "2026-08-22T07:00:00.000Z",
            "2026-08-22T12:00:00.000Z",
          );
          const newestRecords = Array.from(
            { length: 265 },
            // create one more row than the repository limit
            (_unused, index) => makeRecord(
              forecastSource.id,
              "forecast",
              new Date(Date.parse("2026-08-22T07:00:00.000Z") + index * 60_000)
                .toISOString(),
              {
                productRunAt: "2026-08-22T05:00:00.000Z",
                providerDataset: "forecast",
                temperatureC: 20 + (index % 10) / 10,
                upstreamModel: "best_match",
              },
            ),
          ).reverse();
          await completeScheduledIngestion(forecastSession, {
            attempts: 2,
            expectedCheckpointVersion: checkpoint?.version ?? null,
            lastValidAt: "2026-08-22T11:24:00.000Z",
            providerCursor: { product_run_at: "2026-08-22T05:00:00.000Z" },
            records: newestRecords,
            runId: newestRun.id,
            windowEndExclusive: "2026-08-22T12:00:00.000Z",
            windowStart: "2026-08-22T07:00:00.000Z",
          });

          const anchorContainingHorizon = await getWeatherForecast(pool, {
            asOf: "2026-08-22T04:00:00.000Z",
            hours: 6,
            productSelection: "anchor-containing",
            siteSlug: configuration.site.key,
          });
          assert.deepEqual(
            anchorContainingHorizon.map(
              // retain the newest product with the requested anchor hour
              (record) => ({
                productRunAt: record.productRunAt.toISOString(),
                temperatureC: record.temperatureC,
                validAt: record.validAt.toISOString(),
              }),
            ),
            Array.from(
              { length: 6 },
              // build the complete older anchor-bearing product
              (_unused, index) => ({
                productRunAt: "2026-08-22T04:00:00.000Z",
                temperatureC: -10,
                validAt: new Date(
                  Date.parse("2026-08-22T04:00:00.000Z") + index * 3_600_000,
                ).toISOString(),
              }),
            ),
          );

          const horizon = await getWeatherForecast(pool, {
            asOf: "2026-08-22T07:00:00.000Z",
            hours: 3,
            siteSlug: configuration.site.key,
          });
          assert.equal(horizon.length, 180);
          const pressureContext = await getForecastPressureContext(pool, {
            asOf: "2026-08-22T07:00:00.000Z",
            siteSlug: configuration.site.key,
          });
          assert.deepEqual(
            pressureContext.map(
              // retain one complete prior-vintage pressure window
              (record) => ({
                pressureHpa: record.pressureHpa,
                productRunAt: record.productRunAt?.toISOString(),
                validAt: record.validAt.toISOString(),
              }),
            ),
            Array.from(
              { length: 6 },
              // build the exact expected hourly context
              (_unused, index) => ({
                pressureHpa: 1000 + index,
                productRunAt: "2026-08-22T04:00:00.000Z",
                validAt: new Date(
                  Date.parse("2026-08-22T04:00:00.000Z") + index * 3_600_000,
                ).toISOString(),
              }),
            ),
          );
          // return no context without a complete retained window
          const missingPressureContext = await getForecastPressureContext(pool, {
            asOf: "2026-08-23T07:00:00.000Z",
            siteSlug: configuration.site.key,
          });
          assert.deepEqual(missingPressureContext, []);
          assert.equal(
            horizon[0].adapterVersion,
            "open-meteo-forecast-daily/v4",
          );
          assert.equal(
            horizon[0].sourceConfigFingerprint,
            "ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4",
          );
          assert.equal(
            horizon[0].contractEpoch,
            "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83",
          );
          assert.equal(horizon[0].providerMetadata.dataset, "forecast");
          assert.equal(horizon[0].upstreamModel, "best_match");
          assert.deepEqual(
            horizon.map(
              // expose the complete newest-product ordering proof
              (record) => ({
                productRunAt: record.productRunAt.toISOString(),
                temperatureC: record.temperatureC,
                validAt: record.validAt.toISOString(),
              }),
            ),
            Array.from(
              { length: 180 },
              // build the literal expected ordered horizon
              (_unused, index) => ({
                productRunAt: "2026-08-22T05:00:00.000Z",
                temperatureC: 20 + (index % 10) / 10,
                validAt: new Date(
                  Date.parse("2026-08-22T07:00:00.000Z") + index * 60_000,
                ).toISOString(),
              }),
            ),
          );
          const apiPool = createTestPool(
            server,
            "weather_test",
            "weather_api",
            "api-test",
          );

          try {
            await pool.query(
              "REVOKE SELECT ON forecast_runtime_provenance_v1 FROM weather_api",
            );
            const rawHorizon = await getWeatherForecast(apiPool, {
              asOf: "2026-08-22T07:00:00.000Z",
              hours: 3,
              siteSlug: configuration.site.key,
            });
            const expectedRawHorizon = horizon.map(
              // remove only optional runtime provenance
              (record) => ({
                ...record,
                adapterVersion: null,
                contractEpoch: null,
                sourceConfigFingerprint: null,
              }),
            );

            assert.equal(
              JSON.stringify(rawHorizon),
              JSON.stringify(expectedRawHorizon),
            );
          } finally {
            await pool.query(
              "GRANT SELECT ON forecast_runtime_provenance_v1 TO weather_api",
            );
            await apiPool.end();
          }
          const bounded = await getWeatherForecast(pool, {
            asOf: "2026-08-22T07:00:00.000Z",
            hours: 264,
            siteSlug: configuration.site.key,
          });
          assert.equal(bounded.length, 264);
          assert.equal(
            bounded.at(-1).validAt.toISOString(),
            "2026-08-22T11:23:00.000Z",
          );
        } finally {
          await forecastSession.release();
        }
      });

      // verify forecast identity requirements and separation
      await context.test("I-DB-13 and I-DB-15 forecast product runs are required and distinguishable", async () => {
        const forecastSource = await createForecastSource(pool, bootstrap);
        const forecastSession = await requireSession(pool, forecastSource.id);

        try {
          const run = await createRun(
            forecastSession,
            forecastSource,
            "scheduled",
            "2026-08-20T06:00:00.000Z",
            "2026-08-20T07:00:00.000Z",
          );
          await assert.rejects(
            () =>
              insertRawRecord(pool, {
                firstRunId: run.id,
                lastRunId: run.id,
                productRunAt: null,
                sourceId: forecastSource.id,
                sourceKind: "forecast",
                validAt: "2026-08-20T06:00:00.000Z",
              }),
            hasDatabaseCode("23514"),
          );
          await completeScheduledIngestion(forecastSession, {
            attempts: 1,
            expectedCheckpointVersion: null,
            lastValidAt: "2026-08-20T06:00:00.000Z",
            providerCursor: null,
            records: [
              makeRecord(
                forecastSource.id,
                "forecast",
                "2026-08-20T06:00:00.000Z",
                { productRunAt: "2026-08-20T00:00:00.000Z" },
              ),
              makeRecord(
                forecastSource.id,
                "forecast",
                "2026-08-20T06:00:00.000Z",
                { productRunAt: "2026-08-20T01:00:00.000Z" },
              ),
            ],
            runId: run.id,
            windowEndExclusive: "2026-08-20T07:00:00.000Z",
            windowStart: "2026-08-20T06:00:00.000Z",
          });
          const rows = await pool.query(
            "SELECT product_run_at FROM weather_records WHERE source_id = $1 ORDER BY product_run_at",
            [forecastSource.id],
          );
          assert.equal(rows.rowCount, 2);
        } finally {
          await forecastSession.release();
        }
      });

      // verify retry and revision semantics
      await context.test("I-DB-14 identical retry preserves first linkage and revisions increment only on change", async () => {
        const retrySession = await requireSession(pool, currentSource.id);

        try {
          const retry = await createRun(
            retrySession,
            currentSource,
            "scheduled",
            "2026-08-20T00:15:00.000Z",
            "2026-08-20T00:30:00.000Z",
          );
          await completeScheduledIngestion(retrySession, {
            attempts: 1,
            expectedCheckpointVersion: 1,
            lastValidAt: "2026-08-20T00:00:00.000Z",
            providerCursor: { dataset: "current" },
            records: [
              makeRecord(
                currentSource.id,
                "model_current",
                "2026-08-20T00:00:00.000Z",
                { receivedAt: "2026-08-20T00:06:00.000Z" },
              ),
            ],
            runId: retry.id,
            windowEndExclusive: "2026-08-20T00:30:00.000Z",
            windowStart: "2026-08-20T00:15:00.000Z",
          });
          const identical = await pool.query(
            "SELECT first_ingestion_run_id, last_ingestion_run_id, revision_count FROM weather_records WHERE source_id = $1 AND valid_at = '2026-08-20T00:00:00.000Z'",
            [currentSource.id],
          );
          assert.equal(identical.rows[0].first_ingestion_run_id, currentFirstRunId);
          assert.equal(identical.rows[0].last_ingestion_run_id, retry.id);
          assert.equal(identical.rows[0].revision_count, 0);

          const revised = await createRun(
            retrySession,
            currentSource,
            "scheduled",
            "2026-08-20T00:30:00.000Z",
            "2026-08-20T00:45:00.000Z",
          );
          await completeScheduledIngestion(retrySession, {
            attempts: 1,
            expectedCheckpointVersion: 2,
            lastValidAt: "2026-08-20T00:00:00.000Z",
            providerCursor: { dataset: "current" },
            records: [
              makeRecord(
                currentSource.id,
                "model_current",
                "2026-08-20T00:00:00.000Z",
                { receivedAt: "2026-08-20T00:07:00.000Z", temperatureC: 12 },
              ),
            ],
            runId: revised.id,
            windowEndExclusive: "2026-08-20T00:45:00.000Z",
            windowStart: "2026-08-20T00:30:00.000Z",
          });
          const changed = await pool.query(
            "SELECT first_ingestion_run_id, last_ingestion_run_id, revision_count, temperature_c FROM weather_records WHERE source_id = $1 AND valid_at = '2026-08-20T00:00:00.000Z'",
            [currentSource.id],
          );
          assert.equal(changed.rows[0].first_ingestion_run_id, currentFirstRunId);
          assert.equal(changed.rows[0].last_ingestion_run_id, revised.id);
          assert.equal(changed.rows[0].revision_count, 1);
          assert.equal(changed.rows[0].temperature_c, 12);
          await assert.rejects(
            () =>
              pool.query(
                "UPDATE weather_records SET first_ingestion_run_id = $1 WHERE source_id = $2 AND valid_at = '2026-08-20T00:00:00.000Z'",
                [revised.id, currentSource.id],
              ),
            hasDatabaseCode("23514"),
          );
          await assert.rejects(
            () =>
              pool.query(
                "UPDATE weather_records SET soil_moisture_percent = 35 WHERE source_id = $1 AND valid_at = '2026-08-20T00:00:00.000Z'",
                [currentSource.id],
              ),
            hasDatabaseCode("23514"),
          );
        } finally {
          await retrySession.release();
        }
      });

      // verify source-local three-hour pressure change
      await context.test("current pressure change brackets and interpolates same-source records", async () => {
        const target = "2026-08-19T21:00:00.000Z";
        const currentTimes = [
          "2026-08-19T20:29:00.000Z",
          "2026-08-19T20:50:00.000Z",
          target,
          "2026-08-19T21:10:00.000Z",
          "2026-08-19T21:31:00.000Z",
          "2026-08-20T00:01:00.000Z",
        ];

        // isolate mutable fixtures
        try {
          await Promise.all([
            insertRawRecord(pool, {
              firstRunId: currentFirstRunId,
              lastRunId: currentFirstRunId,
              pressureHpa: 1008,
              sourceId: currentSource.id,
              sourceKind: "model_current",
              validAt: currentTimes[1],
            }),
            insertRawRecord(pool, {
              firstRunId: currentFirstRunId,
              lastRunId: currentFirstRunId,
              pressureHpa: 1010,
              sourceId: currentSource.id,
              sourceKind: "model_current",
              validAt: currentTimes[3],
            }),
            insertRawRecord(pool, {
              firstRunId: backfillRunId,
              lastRunId: backfillRunId,
              pressureHpa: 900,
              sourceId: reanalysisSource.id,
              sourceKind: "reanalysis",
              validAt: target,
            }),
          ]);
          // read selected current source
          const readCurrent = async () => {
            const rows = await getCurrentWeather(pool, configuration.site.key, {
              sourceId: String(currentSource.id),
            });
            assert.equal(rows.length, 1);
            return rows[0];
          };
          assert.equal((await readCurrent()).pressureChange3hHpa, 4);

          await pool.query(
            "DELETE FROM weather_records WHERE source_id = $1 AND valid_at = ANY($2::timestamptz[])",
            [currentSource.id, [currentTimes[1], currentTimes[3]]],
          );
          await insertRawRecord(pool, {
            firstRunId: currentFirstRunId,
            lastRunId: currentFirstRunId,
            pressureHpa: 1011,
            sourceId: currentSource.id,
            sourceKind: "model_current",
            validAt: target,
          });
          assert.equal((await readCurrent()).pressureChange3hHpa, 2);

          await pool.query(
            "DELETE FROM weather_records WHERE source_id = $1 AND valid_at = $2",
            [currentSource.id, target],
          );
          await insertRawRecord(pool, {
            firstRunId: currentFirstRunId,
            lastRunId: currentFirstRunId,
            pressureHpa: 1008,
            sourceId: currentSource.id,
            sourceKind: "model_current",
            validAt: currentTimes[1],
          });
          assert.equal((await readCurrent()).pressureChange3hHpa, null);

          await pool.query(
            "DELETE FROM weather_records WHERE source_id = $1 AND valid_at = $2",
            [currentSource.id, currentTimes[1]],
          );
          await Promise.all([
            insertRawRecord(pool, {
              firstRunId: currentFirstRunId,
              lastRunId: currentFirstRunId,
              pressureHpa: 1008,
              sourceId: currentSource.id,
              sourceKind: "model_current",
              validAt: currentTimes[0],
            }),
            insertRawRecord(pool, {
              firstRunId: currentFirstRunId,
              lastRunId: currentFirstRunId,
              pressureHpa: 1010,
              sourceId: currentSource.id,
              sourceKind: "model_current",
              validAt: currentTimes[4],
            }),
          ]);
          assert.equal((await readCurrent()).pressureChange3hHpa, null);

          await insertRawRecord(pool, {
            firstRunId: currentFirstRunId,
            lastRunId: currentFirstRunId,
            pressureHpa: 1009,
            sourceId: currentSource.id,
            sourceKind: "model_current",
            validAt: target,
          });
          assert.equal((await readCurrent()).pressureChange3hHpa, 4);
          await insertRawRecord(pool, {
            firstRunId: currentFirstRunId,
            lastRunId: currentFirstRunId,
            pressureHpa: null,
            sourceId: currentSource.id,
            sourceKind: "model_current",
            validAt: currentTimes[5],
          });
          assert.equal((await readCurrent()).pressureChange3hHpa, null);
        } finally {
          // restore shared fixtures
          await pool.query(
            "DELETE FROM weather_records WHERE (source_id = $1 AND valid_at = ANY($3::timestamptz[])) OR (source_id = $2 AND valid_at = $4)",
            [currentSource.id, reanalysisSource.id, currentTimes, target],
          );
        }
      });

      // verify current/history index availability
      await context.test("I-DB-16 representative current lookup uses the source-time index", async () => {
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
              temperature_c,
              content_hash
            )
            SELECT
              $1,
              'model_current',
              '2020-01-01T00:00:00.000Z'::timestamptz + generated * interval '1 hour',
              $2,
              $2,
              '2020-01-01T00:05:00.000Z'::timestamptz + generated * interval '1 hour',
              '2020-01-01T00:05:00.000Z'::timestamptz + generated * interval '1 hour',
              'UTC',
              10,
              md5(generated::text) || md5(generated::text)
            FROM generate_series(1, 2000) AS generated
            ON CONFLICT ON CONSTRAINT weather_records_identity_key DO NOTHING
          `,
          [currentSource.id, currentFirstRunId],
        );
        await pool.query("ANALYZE weather_records");
        const plan = await pool.query(
          "EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT) SELECT * FROM weather_records WHERE source_id = $1 ORDER BY valid_at DESC, id DESC LIMIT 10",
          [currentSource.id],
        );
        assert.match(
          plan.rows.map((row) => row["QUERY PLAN"]).join("\n"),
          /weather_records_(?:current|source_valid)_idx/u,
        );
        const current = await getCurrentWeather(pool, configuration.site.key);
        // isolate the populated source
        const currentRows = current.filter(
          (record) => record.sourceId === currentSource.id,
        );

        assert.equal(currentRows.length, 1);
        assert.equal(
          new Date(currentRows[0].validAt).toISOString(),
          "2026-08-20T00:00:00.000Z",
        );
      });

      // verify committed running visibility
      await context.test("I-DB-17 committed pre-fetch run is visible to another session", async () => {
        const session = await requireSession(pool, currentSource.id);

        try {
          const run = await createRun(session, currentSource, "scheduled");
          const visible = await pool.query(
            "SELECT state FROM ingestion_runs WHERE id = $1",
            [run.id],
          );
          assert.equal(visible.rows[0].state, "running");
          await failIngestionRun(session, {
            attempts: 1,
            error: testError("probe_failed"),
            runId: run.id,
          });
        } finally {
          await session.release();
        }
      });

      // verify scheduled atomic final state
      await context.test("I-DB-18 scheduled records checkpoint and run finalize atomically", async () => {
        const state = await pool.query(
          `
            SELECT
              run.state,
              checkpoint.version,
              (SELECT count(*)::integer FROM weather_records WHERE source_id = $1) AS records
            FROM ingestion_runs run
            JOIN ingestion_checkpoints checkpoint ON checkpoint.source_id = run.source_id
            WHERE run.id = $2
          `,
          [currentSource.id, currentFirstRunId],
        );
        assert.equal(state.rows[0].state, "succeeded");
        assert.equal(state.rows[0].version >= 1, true);
        assert.equal(state.rows[0].records >= 1, true);
      });

      // verify backfill atomic final state
      await context.test("I-DB-19 backfill records exact outcome and run finalize atomically", async () => {
        const state = await pool.query(
          `
            SELECT run.state, outcome.outcome
            FROM ingestion_runs run
            JOIN backfill_chunk_outcomes outcome ON outcome.ingestion_run_id = run.id
            WHERE run.id = $1
          `,
          [backfillRunId],
        );
        assert.deepEqual(state.rows[0], {
          outcome: "succeeded",
          state: "succeeded",
        });
      });

      // verify rollback after record write
      await context.test("I-DB-20 stale checkpoint fault rolls back rows and finalization", async () => {
        const session = await requireSession(pool, currentSource.id);
        const validAt = "2026-08-20T08:00:00.000Z";

        try {
          const run = await createRun(
            session,
            currentSource,
            "scheduled",
            validAt,
            "2026-08-20T08:15:00.000Z",
          );
          await assert.rejects(
            () =>
              completeScheduledIngestion(session, {
                attempts: 1,
                expectedCheckpointVersion: 999,
                lastValidAt: validAt,
                providerCursor: null,
                records: [makeRecord(currentSource.id, "model_current", validAt)],
                runId: run.id,
                windowEndExclusive: "2026-08-20T08:15:00.000Z",
                windowStart: validAt,
              }),
            /checkpoint compare-and-set/u,
          );
          const state = await pool.query(
            `
              SELECT
                (SELECT count(*)::integer FROM weather_records WHERE source_id = $1 AND valid_at = $2) AS records,
                (SELECT state FROM ingestion_runs WHERE id = $3) AS run_state
            `,
            [currentSource.id, validAt, run.id],
          );
          assert.deepEqual(state.rows[0], { records: 0, run_state: "running" });
          await failIngestionRun(session, {
            attempts: 1,
            error: testError("checkpoint_conflict"),
            runId: run.id,
          });
        } finally {
          await session.release();
        }
      });

      // verify failure preserves last-good state
      await context.test("I-DB-21 provider failure preserves records and checkpoint", async () => {
        const session = await requireSession(pool, currentSource.id);
        const before = await operationalCounts(pool, currentSource.id);

        try {
          const run = await createRun(session, currentSource, "scheduled");
          await failIngestionRun(session, {
            attempts: 3,
            error: testError("provider_unavailable"),
            runId: run.id,
          });
          const after = await operationalCounts(pool, currentSource.id);
          const state = await pool.query(
            "SELECT state, error_code FROM ingestion_runs WHERE id = $1",
            [run.id],
          );
          assert.deepEqual(after, before);
          assert.deepEqual(state.rows[0], {
            error_code: "provider_unavailable",
            state: "failed",
          });
        } finally {
          await session.release();
        }
      });

      // verify stale recovery after lock release
      await context.test("I-DB-22 released session permits deadline-based abandonment", async () => {
        const first = await requireSession(pool, currentSource.id);
        const run = await createRun(first, currentSource, "scheduled");
        await pool.query(
          "UPDATE ingestion_runs SET started_at = clock_timestamp() - interval '2 minutes', deadline_at = clock_timestamp() - interval '1 minute' WHERE id = $1",
          [run.id],
        );
        await first.release();
        const recovered = await requireSession(pool, currentSource.id);

        try {
          const abandoned = await abandonExpiredRuns(
            recovered,
            new Date().toISOString(),
          );
          assert.deepEqual(abandoned, [run.id]);
          const state = await pool.query(
            "SELECT state FROM ingestion_runs WHERE id = $1",
            [run.id],
          );
          assert.equal(state.rows[0].state, "abandoned");
        } finally {
          await recovered.release();
        }
      });

      // verify lock exclusion and non-expired protection
      await context.test("I-DB-23 active lock excludes a second worker and live run is not recovered", async () => {
        const first = await requireSession(pool, currentSource.id);
        let run;

        try {
          assert.equal(await acquireSourceSession(pool, currentSource.id), null);
          run = await createRun(first, currentSource, "scheduled");
          assert.deepEqual(
            await abandonExpiredRuns(first, new Date().toISOString()),
            [],
          );
        } finally {
          await first.release();
        }

        const recovered = await requireSession(pool, currentSource.id);

        try {
          await assert.rejects(
            () => createRun(recovered, currentSource, "scheduled"),
            /already has a running ingestion/u,
          );
          await failIngestionRun(recovered, {
            attempts: 1,
            error: testError("test_cleanup"),
            runId: run.id,
          });
        } finally {
          await recovered.release();
        }
      });

      // verify exact six-part resume behavior
      await context.test("I-DB-24 only exact successful chunks are skipped", async () => {
        assert.equal(
          await hasSuccessfulBackfillChunk(pool, successfulBackfillIdentity),
          true,
        );
        const changedIdentities = [
          { ...successfulBackfillIdentity, adapterVersion: "archive/v2" },
          { ...successfulBackfillIdentity, chunkPlanVersion: "archive-hourly/v2" },
          { ...successfulBackfillIdentity, sourceConfigFingerprint: "b".repeat(64) },
          { ...successfulBackfillIdentity, intervalEndExclusive: "2026-08-20T02:00:00.000Z" },
        ];

        // reject every changed identity from skip
        for (const identity of changedIdentities) {
          assert.equal(await hasSuccessfulBackfillChunk(pool, identity), false);
        }

        const failedSession = await requireSession(pool, reanalysisSource.id);

        try {
          const failedIdentity = makeChunkIdentity(
            reanalysisSource,
            "2026-08-21T00:00:00.000Z",
            "2026-08-21T01:00:00.000Z",
          );
          const run = await createRun(
            failedSession,
            reanalysisSource,
            "backfill",
            failedIdentity.intervalStart,
            failedIdentity.intervalEndExclusive,
          );
          await assert.rejects(
            () =>
              completeBackfillIngestion(failedSession, {
                attempts: 1,
                identity: {
                  ...failedIdentity,
                  intervalEndExclusive: "2026-08-21T02:00:00.000Z",
                },
                records: [],
                runId: run.id,
              }),
            /backfill run identity check/u,
          );
          await failIngestionRun(failedSession, {
            attempts: 1,
            backfillIdentity: failedIdentity,
            error: testError("chunk_failed"),
            runId: run.id,
          });
          assert.equal(await hasSuccessfulBackfillChunk(pool, failedIdentity), false);
          await assert.rejects(
            () =>
              pool.query(
                `
                  UPDATE backfill_chunk_outcomes
                  SET source_id = $1
                  WHERE ingestion_run_id = $2
                `,
                [currentSource.id, run.id],
              ),
            hasDatabaseCode("23503"),
          );
        } finally {
          await failedSession.release();
        }

        const preservedSession = await requireSession(pool, reanalysisSource.id);

        try {
          const run = await createRun(
            preservedSession,
            reanalysisSource,
            "backfill",
            successfulBackfillIdentity.intervalStart,
            successfulBackfillIdentity.intervalEndExclusive,
          );
          await failIngestionRun(preservedSession, {
            attempts: 1,
            backfillIdentity: successfulBackfillIdentity,
            error: testError("later_retry_failed"),
            runId: run.id,
          });
          assert.equal(
            await hasSuccessfulBackfillChunk(pool, successfulBackfillIdentity),
            true,
          );
        } finally {
          await preservedSession.release();
        }
      });

      // verify checkpoint compare-and-set conflict protection
      await context.test("I-DB-25 stale checkpoint cannot overwrite newer state", async () => {
        const session = await requireSession(pool, currentSource.id);

        try {
          const initial = await getScheduledCheckpoint(session);
          assert.ok(initial);
          const priorVersion = initial.version;
          const winning = await createRun(
            session,
            currentSource,
            "scheduled",
            "2026-08-20T09:00:00.000Z",
            "2026-08-20T09:15:00.000Z",
          );
          await completeScheduledIngestion(session, {
            attempts: 1,
            expectedCheckpointVersion: priorVersion,
            lastValidAt: "2026-08-20T09:00:00.000Z",
            providerCursor: null,
            records: [
              makeRecord(currentSource.id, "model_current", "2026-08-20T09:00:00.000Z"),
            ],
            runId: winning.id,
            windowEndExclusive: "2026-08-20T09:15:00.000Z",
            windowStart: "2026-08-20T09:00:00.000Z",
          });
          const stale = await createRun(
            session,
            currentSource,
            "scheduled",
            "2026-08-20T10:00:00.000Z",
            "2026-08-20T10:15:00.000Z",
          );
          await assert.rejects(
            () =>
              completeScheduledIngestion(session, {
                attempts: 1,
                expectedCheckpointVersion: priorVersion,
                lastValidAt: "2026-08-20T10:00:00.000Z",
                providerCursor: null,
                records: [
                  makeRecord(currentSource.id, "model_current", "2026-08-20T10:00:00.000Z"),
                ],
                runId: stale.id,
                windowEndExclusive: "2026-08-20T10:15:00.000Z",
                windowStart: "2026-08-20T10:00:00.000Z",
              }),
            /checkpoint compare-and-set/u,
          );
          const checkpoint = await getScheduledCheckpoint(session);
          assert.ok(checkpoint);
          assert.equal(checkpoint.version, priorVersion + 1);
          assert.equal(
            checkpoint.lastValidAt,
            "2026-08-20T09:00:00.000Z",
          );
          await failIngestionRun(session, {
            attempts: 1,
            error: testError("stale_checkpoint"),
            runId: stale.id,
          });
        } finally {
          await session.release();
        }
      });

      // verify heartbeat success independence
      await context.test("I-DB-26 heartbeat loop freshness is independent from last success", async () => {
        await updateWorkerHeartbeat(pool, {
          activity: "scheduled",
          instance: "worker-main",
          lastLoopAt: "2026-08-20T11:00:00.000Z",
          lastSuccessAt: "2026-08-20T10:00:00.000Z",
          version: "worker/v1",
        });
        await updateWorkerHeartbeat(pool, {
          activity: "idle",
          instance: "worker-main",
          lastLoopAt: "2026-08-20T11:15:00.000Z",
          lastSuccessAt: "2026-08-20T10:00:00.000Z",
          version: "worker/v1",
        });
        const heartbeat = await pool.query(
          "SELECT last_loop_at, last_success_at, current_activity FROM worker_heartbeats WHERE worker_instance = 'worker-main'",
        );
        assert.equal(
          heartbeat.rows[0].last_loop_at.toISOString(),
          "2026-08-20T11:15:00.000Z",
        );
        assert.equal(
          heartbeat.rows[0].last_success_at.toISOString(),
          "2026-08-20T10:00:00.000Z",
        );
        assert.equal(heartbeat.rows[0].current_activity, "idle");
      });

      // verify nearest-gauge daily accumulation
      await context.test("I-DB-27 daily precipitation selects one nearest physical source", async () => {
        const tempestConfiguration = await loadTempestConfiguration(tempestConfigurationPath);
        const tempestBootstrap = await bootstrapTempestConfiguration(
          pool,
          tempestConfiguration,
        );
        const precipitationFixtures = [
          {
            accumulation: [0.3, 0.7],
            sourceId: tempestBootstrap.sourceIds["tempest-64255-observations-v2"],
          },
          {
            accumulation: [9],
            sourceId: tempestBootstrap.sourceIds["tempest-225947-observations-v2"],
          },
        ];

        // populate two competing physical gauges
        for (const fixture of precipitationFixtures) {
          assert.notEqual(fixture.sourceId, undefined);
          const sourceResult = await pool.query(
            "SELECT id, source_kind, source_config_fingerprint FROM sources WHERE id = $1",
            [fixture.sourceId],
          );
          const source = sourceResult.rows[0];
          const session = await requireSession(pool, source.id);

          try {
            const run = await createRun(
              session,
              source,
              "scheduled",
              "2026-08-28T07:00:00.000Z",
              "2026-08-28T08:00:00.000Z",
            );
            await completeScheduledIngestion(session, {
              attempts: 1,
              expectedCheckpointVersion: null,
              lastValidAt: `2026-08-28T07:0${String(fixture.accumulation.length)}:00.000Z`,
              providerCursor: null,
              records: fixture.accumulation.map(
                // create one interval accumulation
                (precipitationMm, index) => makeRecord(
                  source.id,
                  "physical_sensor",
                  `2026-08-28T07:0${String(index + 1)}:00.000Z`,
                  { precipitationMm },
                ),
              ),
              runId: run.id,
              windowEndExclusive: "2026-08-28T08:00:00.000Z",
              windowStart: "2026-08-28T07:00:00.000Z",
            });
          } finally {
            await session.release();
          }
        }

        const daily = await getDailyPrecipitation(pool, {
          from: "2026-08-28T07:00:00.000Z",
          siteSlug: configuration.site.key,
          to: "2026-08-28T18:00:00.000Z",
        });
        assert.equal(daily?.sourceId, precipitationFixtures[0].sourceId);
        assert.equal(daily?.stationSlug, "tempest-64255");
        assert.equal(daily?.accumulationMm, 1);
        assert.equal(
          new Date(daily?.validThrough).toISOString(),
          "2026-08-28T07:02:00.000Z",
        );
      });

      // verify idempotent first-party gateway bootstrap after baseline contracts
      await context.test("Ecowitt gateway bootstrap preserves its LAN identity", async () => {
        const ecowittConfiguration = await loadEcowittConfiguration(
          ecowittConfigurationPath,
        );
        const first = await bootstrapEcowittConfiguration(pool, ecowittConfiguration);
        const second = await bootstrapEcowittConfiguration(pool, ecowittConfiguration);
        const station = await pool.query(
          `
            SELECT st.slug, st.vendor, st.model, st.serial,
                   s.source_key, s.source_kind, s.cadence_seconds
            FROM stations st
            JOIN sources s ON s.station_id = st.id
            WHERE st.id = $1
          `,
          [first.stationIds["ballydidean-ecowitt"]],
        );

        assert.deepEqual(second, first);
        assert.deepEqual(station.rows[0], {
          cadence_seconds: 60,
          model: "GW3000",
          serial: "88:F1:55:05:D8:9F",
          slug: "ballydidean-ecowitt",
          source_key: "ecowitt-88f15505d89f-local-live-v1",
          source_kind: "physical_sensor",
          vendor: "Ecowitt",
        });
      });
      // preserve ordinary ingestion and its checkpoint when the optional archive fails
      await context.test("scheduled serving survives archive SQL and plain errors", async () => {
        const session = await requireSession(pool, currentSource.id);
        try {
          await pool.query("CREATE TABLE archive_probe (identity text)");
          // exercise both database-aborted and JavaScript-only failures
          for (const [index, failSql] of [true, false].entries()) {
            const validAt = `2026-09-01T0${String(index)}:00:00.000Z`;
            const end = `2026-09-01T0${String(index)}:15:00.000Z`;
            const checkpoint = await getScheduledCheckpoint(session);
            const run = await createRun(session, currentSource, "scheduled", validAt, end);
            let archiveCalled = false;
            await completeScheduledIngestion(session, {
              attempts: 1,
              expectedCheckpointVersion: checkpoint.version,
              lastValidAt: validAt,
              providerCursor: null,
              records: [makeRecord(currentSource.id, "model_current", validAt)],
              // fail inside the existing serving transaction
              async revisionArchiver(queryable) {
                archiveCalled = true;
                await queryable.query("INSERT INTO archive_probe VALUES ($1)", [validAt]);
                // trigger the PostgreSQL transaction-aborted path
                if (failSql) {
                  await queryable.query("SELECT 1 / 0");
                }
                throw new Error("archive unavailable");
              },
              runId: run.id,
              windowEndExclusive: end,
              windowStart: validAt,
            });
            assert.equal(archiveCalled, true);
            const stored = await pool.query(
              "SELECT temperature_c FROM weather_records WHERE source_id = $1 AND valid_at = $2",
              [currentSource.id, validAt],
            );
            assert.equal(stored.rows[0]?.temperature_c, 10);
            const state = await pool.query("SELECT state FROM ingestion_runs WHERE id = $1", [run.id]);
            assert.equal(state.rows[0].state, "succeeded");
            assert.equal((await pool.query("SELECT count(*)::integer AS count FROM archive_probe")).rows[0].count, 0);
            assert.equal((await getScheduledCheckpoint(session)).version, checkpoint.version + 1);
          }
        } finally {
          await session.release();
        }
      });
    } finally {
      await Promise.all([pool.end(), adminPool.end()]);
      await stopPostgres(server);
    }
  },
);

// match PostgreSQL error codes
function hasDatabaseCode(code) {
  // inspect structured database errors
  return (error) => error?.code === code;
}

// execute the real first-init role boundary
async function runRuntimeRoleBootstrap(server) {
  const directory = await mkdtemp(join(tmpdir(), "weather-role-secrets-"));
  const adminPath = join(directory, "admin");
  const ownerPath = join(directory, "owner");
  const apiPath = join(directory, "api");
  const ingestPath = join(directory, "ingest");
  const trainingExportPath = join(directory, "training-export");

  try {
    await Promise.all([
      writeFile(adminPath, "admin-test\n", { mode: 0o600 }),
      writeFile(ownerPath, "owner-test\n", { mode: 0o600 }),
      writeFile(apiPath, "api-test\n", { mode: 0o600 }),
      writeFile(ingestPath, "ingest-test\n", { mode: 0o600 }),
      writeFile(trainingExportPath, "training-export-test\n", { mode: 0o600 }),
    ]);
    await executeFile(
      join(repositoryRoot, "deploy/postgres/010-create-runtime-roles.sh"),
      [],
      {
        env: {
          ...process.env,
          PGHOST: server.host,
          PGPASSWORD: server.password,
          PGPORT: String(server.port),
          POSTGRES_DB: "weather_test",
          POSTGRES_USER: server.user,
          WEATHER_ADMIN_PASSWORD_FILE: adminPath,
          WEATHER_API_PASSWORD_FILE: apiPath,
          WEATHER_INGEST_PASSWORD_FILE: ingestPath,
          WEATHER_OWNER_PASSWORD_FILE: ownerPath,
          WEATHER_TRAINING_EXPORT_PASSWORD_FILE: trainingExportPath,
        },
        timeout: 30_000,
      },
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
}

// require a retained source session
async function requireSession(pool, sourceId) {
  const session = await acquireSourceSession(pool, sourceId);

  // reject unexpected lock contention
  if (session === null) {
    throw new Error(`could not acquire source session for ${sourceId}`);
  }

  return session;
}

// create a committed running row
async function createRun(
  session,
  source,
  mode,
  requestedStart = "2026-08-22T00:00:00.000Z",
  requestedEndExclusive = "2026-08-22T01:00:00.000Z",
) {
  // preserve the checked live-v4 adapter identity
  const adapterVersion = mode === "backfill"
    ? "archive/v1"
    : source.source_key === "open-meteo-forecast-v4"
      ? "open-meteo-forecast-daily/v4"
      : "current/v1";

  return startIngestionRun(session, {
    adapterVersion,
    chunkPlanVersion: mode === "backfill" ? "archive-hourly/v1" : null,
    deadlineAt: new Date(Date.now() + 120_000).toISOString(),
    mode,
    requestedEndExclusive,
    requestedStart,
    sourceConfigFingerprint: source.source_config_fingerprint,
  });
}

// create a normalized test record
function makeRecord(sourceId, sourceKind, validAt, overrides = {}) {
  return createNormalizedWeatherRecord({
    metadata: {
      device: null,
      model: overrides.upstreamModel ?? "test-grid/v1",
      provider: { dataset: overrides.providerDataset ?? "integration" },
      quality: { status: "validated" },
      upstreamTimezone: "America/Los_Angeles",
    },
    metrics: {
      apparentTemperatureC: 9,
      blackGlobeTemperatureC: 18,
      cloudCoverPercent: 50,
      pm25MicrogramsPerCubicMeter: 7,
      precipitationMm: overrides.precipitationMm ?? 0,
      precipitationRateMmPerHour: 0,
      pressureHpa: overrides.pressureHpa ?? 1013,
      relativeHumidityPercent: 70,
      soilElectricalConductivityMicrosiemensPerCm: 420,
      soilMoisturePercent: overrides.soilMoisturePercent ?? 34,
      solarRadiationWm2: 320,
      temperatureC: overrides.temperatureC ?? 10,
      uvIndex: 2,
      waterLevelM: null,
      windDirectionDegrees: 180,
      windGustMps: 7,
      windSpeedMps: 4,
      wetBulbGlobeTemperatureC: 11,
    },
    productRunAt: overrides.productRunAt ?? null,
    receivedAt: overrides.receivedAt ?? "2026-08-20T00:05:00.000Z",
    sourceId,
    sourceKind,
    validAt,
  });
}

// create an exact chunk identity
function makeChunkIdentity(source, intervalStart, intervalEndExclusive) {
  return {
    adapterVersion: "archive/v1",
    chunkPlanVersion: "archive-hourly/v1",
    intervalEndExclusive,
    intervalStart,
    requestedFromDate: intervalStart.slice(0, 10),
    requestedToDate: intervalStart.slice(0, 10),
    sourceConfigFingerprint: source.source_config_fingerprint,
    sourceId: source.id,
  };
}

// create a bounded failure
function testError(code) {
  return {
    classification: "retryable",
    code,
    message: `integration failure: ${code}`,
  };
}

// insert a raw row for constraint tests
async function insertRawRecord(pool, input) {
  return pool.query(
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
        provider_metadata,
        pressure_hpa,
        soil_moisture_percent,
        uv_index,
        content_hash
      )
      VALUES ($1, $2, $3, $4, $5, $6, $3, $3, 'UTC', $7::jsonb, $8, $9, $10, $11)
    `,
    [
      input.sourceId,
      input.sourceKind,
      input.validAt,
      input.productRunAt ?? null,
      input.firstRunId,
      input.lastRunId,
      input.providerMetadata === undefined
        ? null
        : JSON.stringify(input.providerMetadata),
      input.pressureHpa ?? null,
      input.soilMoisturePercent ?? null,
      input.uvIndex ?? null,
      "c".repeat(64),
    ],
  );
}

// add one future forecast source
async function createForecastSource(pool, bootstrap) {
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
      VALUES ($1, $2, 'forecast-test-v1', 'forecast', '{"contract":"forecast/v1"}'::jsonb, $3, '["forecast"]'::jsonb, 3600, true)
      RETURNING id, source_kind, source_config_fingerprint
    `,
    [bootstrap.stationId, bootstrap.providerId, "d".repeat(64)],
  );

  return result.rows[0];
}

// snapshot record and checkpoint state
async function operationalCounts(pool, sourceId) {
  const result = await pool.query(
    `
      SELECT
        (SELECT count(*)::integer FROM weather_records WHERE source_id = $1) AS records,
        (SELECT version FROM ingestion_checkpoints WHERE source_id = $1) AS checkpoint_version
    `,
    [sourceId],
  );

  return result.rows[0];
}
