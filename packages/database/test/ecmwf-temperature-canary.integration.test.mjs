import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import test from "node:test";

import {
  bootstrapSiteConfiguration,
  getEcmwfTemperatureCanarySidecar,
  getEcmwfTemperatureCanaryStatus,
  loadSiteConfiguration,
  listEcmwfTemperatureCanaryAuditRows,
  persistEcmwfTemperatureCanaryRun,
  runMigrations,
} from "../dist/index.js";
import {
  createTestPool,
  prepareRuntimeRoles,
  startPostgres,
  stopPostgres,
} from "./postgres-harness.mjs";

const repositoryRoot = resolve(import.meta.dirname, "../../..");
const migrationDirectory = join(repositoryRoot, "packages/database/migrations");
const runtimeAclPath = join(repositoryRoot, "deploy/postgres/runtime-acl-v2.sql");
const sitePath = join(repositoryRoot, "config/sites/ballydidean.json");
const executeFile = promisify(execFile);

// create one exact cold run input
function runInput() {
  const runInitializedAt = "2026-09-07T12:00:00.000Z";

  return {
    adapterVersion: "open-meteo-ecmwf-single-run/v1",
    hours: Array.from({ length: 18 }, (_, index) => ({
      modelLeadHours: index + 1,
      rawRelativeHumidityPercent: 70 + index,
      rawTemperatureC: 10 + index / 10,
      rawWindSpeedMps: 2 + index / 10,
      validAt: new Date(
        Date.parse(runInitializedAt) + (index + 1) * 3_600_000,
      ).toISOString(),
    })),
    modelCycle: "50r1",
    providerResponseSha256: "a".repeat(64),
    receivedAt: "2026-09-07T18:03:00.000Z",
    recentErrorState: {
      b24C: null,
      b72C: null,
      cohort: "ecmwf_single_run_hindcast",
      localDates: 0,
      mad72C: null,
      maximumSourceRunInitializedAt: null,
      maximumSourceValidAt: null,
      n24: 0,
      n72: 0,
      sourceKeys: [],
      supported: false,
      targetRunInitializedAt: runInitializedAt,
      windowEndValidAt: "2026-09-07T05:00:00.000Z",
    },
    runInitializedAt,
    siteSlug: "ballydidean",
    stateReason: "no_causal_forecast_observation_pairs",
    stateStatus: "cold",
    upstreamModel: "ecmwf_ifs",
  };
}

test("ECMWF temperature sidecar is immutable private and single-run", async () => {
  const server = await startPostgres(15, "ecmwf-temperature-canary");
  const adminPool = createTestPool(server);
  let ownerPool;
  let ingestPool;
  let apiPool;

  try {
    await prepareRuntimeRoles(adminPool);
    ownerPool = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    await runMigrations(ownerPool, migrationDirectory);
    await bootstrapSiteConfiguration(
      ownerPool,
      await loadSiteConfiguration(sitePath),
    );
    await executeFile(
      "docker",
      [
        "cp",
        runtimeAclPath,
        `${server.name}:/tmp/runtime-acl-v2.sql`,
      ],
      { timeout: 30_000 },
    );
    await executeFile(
      "docker",
      [
        "exec",
        server.name,
        "psql",
        "--set=ON_ERROR_STOP=1",
        "--username",
        server.user,
        "--dbname",
        "weather_test",
        "--file",
        "/tmp/runtime-acl-v2.sql",
      ],
      { timeout: 30_000 },
    );
    ingestPool = createTestPool(
      server,
      "weather_test",
      "weather_ingest",
      "ingest-test",
    );
    apiPool = createTestPool(
      server,
      "weather_test",
      "weather_api",
      "api-test",
    );

    const input = runInput();
    const persisted = await persistEcmwfTemperatureCanaryRun(ingestPool, input);
    assert.equal(persisted.runInitializedAt, input.runInitializedAt);
    assert.equal(persisted.stateStatus, "cold");

    const sidecar = await getEcmwfTemperatureCanarySidecar(apiPool, {
      asOf: "2026-09-07T18:03:00.000Z",
      from: "2026-09-07T12:00:00.000Z",
      siteSlug: "ballydidean",
      to: "2026-09-09T00:00:00.000Z",
    });
    assert.ok(sidecar);
    assert.equal(sidecar.run.id, persisted.id);
    assert.equal(sidecar.hours.length, 12);
    assert.deepEqual(
      sidecar.hours.map((hour) => hour.modelLeadHours),
      [7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18],
    );

    const status = await getEcmwfTemperatureCanaryStatus(apiPool, {
      asOf: "2026-09-07T18:03:00.000Z",
      siteSlug: "ballydidean",
    });
    assert.deepEqual(status, {
      firstReceivedAt: input.receivedAt,
      hourCount: 18,
      latestRunInitializedAt: input.runInitializedAt,
      stateReason: input.stateReason,
      stateStatus: "cold",
    });
    const auditRows = await listEcmwfTemperatureCanaryAuditRows(apiPool, {
      from: "2026-09-07T12:00:00.000Z",
      siteSlug: "ballydidean",
      to: "2026-09-09T00:00:00.000Z",
    });
    assert.equal(auditRows.length, 18);
    assert.equal(auditRows[0].run.id, persisted.id);
    assert.equal(auditRows[0].hour.modelLeadHours, 1);

    let archivedRevision = null;
    const nextInitializedAt = "2026-09-07T18:00:00.000Z";
    await persistEcmwfTemperatureCanaryRun(ingestPool, {
      ...input,
      hours: input.hours.map(
        // preserve exact lead geometry for the next initialization
        (hour) => ({
          ...hour,
          validAt: new Date(Date.parse(hour.validAt) + 6 * 3_600_000).toISOString(),
        }),
      ),
      providerResponseSha256: "b".repeat(64),
      receivedAt: "2026-09-08T00:03:00.000Z",
      recentErrorState: {
        ...input.recentErrorState,
        targetRunInitializedAt: nextInitializedAt,
        windowEndValidAt: "2026-09-07T11:00:00.000Z",
      },
      runInitializedAt: nextInitializedAt,
    }, async (_queryable, revision) => {
      archivedRevision = revision;
      return null;
    });
    assert.equal(archivedRevision.modelCycle, "50r1");
    assert.deepEqual(archivedRevision.bestMatchRows, []);

    await persistEcmwfTemperatureCanaryRun(ingestPool, {
      ...input,
      receivedAt: "2026-09-07T18:05:00.000Z",
    });
    await assert.rejects(
      persistEcmwfTemperatureCanaryRun(ingestPool, {
        ...input,
        adapterVersion: "open-meteo-ecmwf-single-run/v2",
        runInitializedAt: "2026-09-07T18:00:00.000Z",
      }),
      /provenance is invalid/u,
    );
    await assert.rejects(
      ownerPool.query(
        `
          INSERT INTO ecmwf_temperature_canary_runs (
            site_id,
            run_initialized_at,
            first_received_at,
            last_received_at,
            upstream_model,
            adapter_version,
            provider_response_sha256,
            model_cycle,
            recent_error_state,
            recent_error_state_sha256,
            state_status,
            state_reason,
            content_hash
          )
          SELECT
            site_id,
            run_initialized_at + interval '6 hours',
            first_received_at + interval '6 hours',
            last_received_at + interval '6 hours',
            upstream_model,
            'open-meteo-ecmwf-single-run/v2',
            provider_response_sha256,
            model_cycle,
            recent_error_state,
            recent_error_state_sha256,
            state_status,
            state_reason,
            content_hash
          FROM ecmwf_temperature_canary_runs
          WHERE id = $1
        `,
        [persisted.id],
      ),
      {
        code: "23514",
        constraint: "ecmwf_temperature_canary_runs_adapter_check",
      },
    );
    await assert.rejects(
      ownerPool.query(
        `
          INSERT INTO ecmwf_temperature_canary_runs (
            site_id,
            run_initialized_at,
            first_received_at,
            last_received_at,
            upstream_model,
            adapter_version,
            provider_response_sha256,
            model_cycle,
            recent_error_state,
            recent_error_state_sha256,
            state_status,
            state_reason,
            content_hash
          )
          SELECT
            site_id,
            run_initialized_at + interval '12 hours',
            first_received_at + interval '12 hours',
            last_received_at + interval '12 hours',
            upstream_model,
            adapter_version,
            provider_response_sha256,
            '49r1',
            recent_error_state,
            recent_error_state_sha256,
            state_status,
            state_reason,
            content_hash
          FROM ecmwf_temperature_canary_runs
          WHERE id = $1
        `,
        [persisted.id],
      ),
      {
        code: "23514",
        constraint: "ecmwf_temperature_canary_runs_cycle_check",
      },
    );
    await assert.rejects(
      persistEcmwfTemperatureCanaryRun(ingestPool, {
        ...input,
        hours: input.hours.map((hour, index) =>
          index === 7
            ? { ...hour, rawTemperatureC: hour.rawTemperatureC + 1 }
            : hour,
        ),
        receivedAt: "2026-09-07T18:06:00.000Z",
      }),
      /revision rejected/u,
    );
    await assert.rejects(
      apiPool.query(
        "INSERT INTO ecmwf_temperature_canary_hours (run_id, valid_at, model_lead_hours, raw_temperature_c, content_hash) VALUES ($1, now(), 1, 10, repeat('a', 64))",
        [persisted.id],
      ),
      { code: "42501" },
    );
    await assert.rejects(
      ingestPool.query(
        "UPDATE ecmwf_temperature_canary_hours SET raw_temperature_c = raw_temperature_c WHERE run_id = $1",
        [persisted.id],
      ),
      { code: "42501" },
    );
    await ownerPool.query("CREATE TABLE archive_probe (identity text); GRANT INSERT ON archive_probe TO weather_ingest");
    // retain the complete source run after optional archive failures
    for (const [index, failSql] of [true, false].entries()) {
      const shiftMs = (index + 2) * 6 * 3_600_000;
      const shiftedRunAt = new Date(Date.parse(input.runInitializedAt) + shiftMs).toISOString();
      const shiftedInput = {
        ...input,
        hours: input.hours.map(
          // retain exact initialization-to-valid-time geometry
          (hour) => ({ ...hour, validAt: new Date(Date.parse(hour.validAt) + shiftMs).toISOString() }),
        ),
        providerResponseSha256: String(index + 1).repeat(64),
        receivedAt: new Date(Date.parse(input.receivedAt) + shiftMs).toISOString(),
        recentErrorState: {
          ...input.recentErrorState,
          targetRunInitializedAt: shiftedRunAt,
          windowEndValidAt: new Date(Date.parse(input.recentErrorState.windowEndValidAt) + shiftMs).toISOString(),
        },
        runInitializedAt: shiftedRunAt,
      };
      let archiveCalled = false;
      const retained = await persistEcmwfTemperatureCanaryRun(ingestPool, shiftedInput,
        // reject archive SQL without sacrificing the already inserted source hours
        async (queryable) => {
          archiveCalled = true;
          await queryable.query("INSERT INTO archive_probe VALUES ($1)", [shiftedRunAt]);
          // exercise the database-aborted transaction branch
          if (failSql) {
            await queryable.query("SELECT 1 / 0");
          }
          throw new Error("archive unavailable");
        });
      assert.equal(archiveCalled, true);
      const stored = await ownerPool.query(
        "SELECT count(*)::integer AS count FROM ecmwf_temperature_canary_hours WHERE run_id = $1",
        [retained.id],
      );
      assert.equal(stored.rows[0].count, 18);
      assert.equal((await ownerPool.query("SELECT count(*)::integer AS count FROM archive_probe")).rows[0].count, 0);
    }
  } finally {
    await apiPool?.end().catch(() => undefined);
    await ingestPool?.end().catch(() => undefined);
    await ownerPool?.end().catch(() => undefined);
    await adminPool.end().catch(() => undefined);
    await stopPostgres(server);
  }
});
