import { createHash } from "node:crypto";
import { gunzipSync } from "node:zlib";
import type { Pool, QueryResultRow } from "pg";

import type { Queryable } from "./pool.js";

const HOUR_MS = 3_600_000;
const HASH = /^[a-f0-9]{64}$/u;

// retain private inputs only inside the ingestion boundary
export interface RainAdjustmentCapture {
  readonly claimId: string;
  readonly kind: "forecast" | "station";
  readonly stationId: number | null;
  readonly runInitializedAt: string | null;
  readonly windowStart: string | null;
  readonly windowEndExclusive: string | null;
  readonly completedAt: string;
  readonly bodySha256: string;
  readonly body: Uint8Array;
}

// expose only the model output and its forecast identity
export interface RainAdjustmentHour {
  readonly validAt: string;
  readonly modelLeadHours: number;
  readonly rawPrecipitationMm: number;
  readonly correctedPrecipitationMm: number;
  readonly applied: boolean;
  readonly reasonCode: string | null;
}

export interface RainAdjustmentRun {
  readonly runInitializedAt: string;
  readonly modelSha256: string;
  readonly inputSha256: string;
  readonly forecastClaimId: string;
  readonly firstReceivedAt: string;
  readonly decisionAt: string;
  readonly generatedAt: string;
  readonly hours: readonly RainAdjustmentHour[];
}

// expose only the exact server-persisted rain gate needed for revision archival
export interface PersistedRainAdjustmentRevision {
  readonly generatedAt: string;
  readonly hours: readonly RainAdjustmentHour[];
  readonly inputSha256: string;
  readonly modelSha256: string;
  readonly runInitializedAt: string;
  readonly storedContentSha256: string;
}

// finish cold publication only after the serving transaction commits
export interface RainAdjustmentRevisionPublication {
  readonly publish: () => Promise<void>;
}

// stage and bind one exact persisted gate inside its serving transaction
export type RainAdjustmentRevisionArchiver = (
  queryable: Queryable,
  revision: PersistedRainAdjustmentRevision,
) => Promise<RainAdjustmentRevisionPublication | null>;

interface CaptureRow extends QueryResultRow {
  readonly id: string;
  readonly kind: "forecast" | "station";
  readonly station_id: number | null;
  readonly run_initialized_at: Date | null;
  readonly window_start: Date | null;
  readonly window_end_exclusive: Date | null;
  readonly completed_at: Date;
  readonly body_sha256: string;
  readonly compressed_body: Buffer;
}

// select one timely current run not already evaluated by this immutable model
export async function readPendingRainAdjustmentCaptures(
  pool: Pool,
  modelSha256: string,
): Promise<readonly RainAdjustmentCapture[]> {
  // reject unpinned models before reading private evidence
  if (!HASH.test(modelSha256)) {
    throw new RangeError("invalid rain model identity");
  }
  const pending = await pool.query<{ run_initialized_at: Date }>(`
    SELECT c.run_initialized_at FROM rain_capture_claims c
    JOIN rain_capture_receipts r ON r.claim_id = c.id
    WHERE c.kind = 'forecast' AND r.outcome = 'valid' AND r.available_by_decision IS TRUE
      AND c.run_initialized_at + interval '8 hours' <= clock_timestamp()
      AND c.run_initialized_at >= clock_timestamp() - interval '20 hours'
      AND NOT EXISTS (SELECT 1 FROM rain_adjustment_runs a
        WHERE a.run_initialized_at = c.run_initialized_at AND a.model_sha256 = $1)
    ORDER BY c.run_initialized_at DESC LIMIT 1
  `, [modelSha256]);
  const initialized = pending.rows[0]?.run_initialized_at;
  // stop without re-reading already frozen inputs
  if (initialized === undefined) {
    return [];
  }
  const decision = new Date(initialized.getTime() + 8 * HOUR_MS);
  const result = await pool.query<CaptureRow>(`
    SELECT c.id, c.kind, c.station_id, c.run_initialized_at, c.window_start,
      c.window_end_exclusive, r.completed_at, r.body_sha256, r.compressed_body
    FROM rain_capture_claims c JOIN rain_capture_receipts r ON r.claim_id = c.id
    WHERE r.outcome = 'valid' AND r.completed_at <= $2::timestamptz AND (
      (c.kind = 'forecast' AND c.run_initialized_at IN
        ($1::timestamptz, $1::timestamptz - interval '6 hours', $1::timestamptz - interval '12 hours'))
      OR (c.kind = 'station' AND c.window_end_exclusive > $2::timestamptz - interval '26 hours'
        AND c.window_start < $2::timestamptz))
    ORDER BY r.completed_at, c.id LIMIT 350
  `, [initialized.toISOString(), decision.toISOString()]);
  let decodedBytes = 0;
  // cap memory independently of compressed size and preserve exact source bytes
  return result.rows.map((row) => {
    const body = gunzipSync(row.compressed_body, { maxOutputLength: 2_000_000 });
    decodedBytes += body.byteLength;
    // keep one inference below the worker memory budget even for highly compressed bodies
    if (decodedBytes > 16 * 1024 * 1024) {
      throw new Error("rain inference input exceeds memory budget");
    }
    // reject storage corruption before parsing provider data
    if (createHash("sha256").update(body).digest("hex") !== row.body_sha256) {
      throw new Error("rain capture body identity mismatch");
    }
    return {
      claimId: row.id,
      kind: row.kind,
      stationId: row.station_id,
      runInitializedAt: row.run_initialized_at?.toISOString() ?? null,
      windowStart: row.window_start?.toISOString() ?? null,
      windowEndExclusive: row.window_end_exclusive?.toISOString() ?? null,
      completedAt: row.completed_at.toISOString(),
      bodySha256: row.body_sha256,
      body,
    };
  });
}

// append one public-safe projection without changing any previous forecast
export async function appendRainAdjustmentRun(
  pool: Pool,
  run: RainAdjustmentRun,
  revisionArchiver?: RainAdjustmentRevisionArchiver,
): Promise<boolean> {
  const client = await pool.connect();
  let publication: RainAdjustmentRevisionPublication | null = null;
  let inserted = false;
  try {
    await client.query("BEGIN");
    const result = await client.query<{
      generatedAt: Date;
      hours: readonly RainAdjustmentHour[];
      inputSha256: string;
      modelSha256: string;
      runInitializedAt: Date;
      storedContentSha256: string;
    }>(`
      INSERT INTO rain_adjustment_runs (run_initialized_at, model_sha256, input_sha256,
        forecast_claim_id, first_received_at, decision_at, hours)
      VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)
      ON CONFLICT (run_initialized_at, model_sha256) DO NOTHING
      RETURNING run_initialized_at AS "runInitializedAt", model_sha256 AS "modelSha256",
        input_sha256 AS "inputSha256", generated_at AS "generatedAt", hours,
        encode(sha256(convert_to(hours::text, 'UTF8')), 'hex') AS "storedContentSha256"
    `, [run.runInitializedAt, run.modelSha256, run.inputSha256, run.forecastClaimId,
      run.firstReceivedAt, run.decisionAt, JSON.stringify(run.hours)]);
    const row = result.rows[0];
    inserted = row !== undefined;
    // archive only the new immutable serving revision and never backfill conflict rows
    if (row !== undefined && revisionArchiver !== undefined) {
      await client.query("SAVEPOINT rain_adjustment_revision_archive");
      try {
        publication = await revisionArchiver(client, {
          generatedAt: row.generatedAt.toISOString(),
          hours: row.hours,
          inputSha256: row.inputSha256,
          modelSha256: row.modelSha256,
          runInitializedAt: row.runInitializedAt.toISOString(),
          storedContentSha256: row.storedContentSha256,
        });
        await client.query("RELEASE SAVEPOINT rain_adjustment_revision_archive");
      } catch {
        // clear aborted SQL and partial archive writes without losing the serving row
        await client.query("ROLLBACK TO SAVEPOINT rain_adjustment_revision_archive");
        await client.query("RELEASE SAVEPOINT rain_adjustment_revision_archive");
        publication = null;
      }
    }
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
  // publish staged bytes only after the live row and pointer transaction commits
  if (publication !== null) {
    try {
      await publication.publish();
    } catch {
      // preserve serving after a permanent cold publication gap
    }
  }
  return inserted;
}

// read one recent inference without granting the API access to provider bodies
export async function readRainAdjustmentRun(
  pool: Pool,
  siteSlug: string,
  asOf: string,
): Promise<RainAdjustmentRun | null> {
  // restrict this model to its frozen site and a valid reference time
  if (siteSlug !== "ballydidean" || !Number.isFinite(Date.parse(asOf))) {
    return null;
  }
  const result = await pool.query<{
    run_initialized_at: Date; model_sha256: string; input_sha256: string;
    forecast_claim_id: string; first_received_at: Date; decision_at: Date;
    generated_at: Date; hours: readonly RainAdjustmentHour[];
  }>(`
    SELECT run_initialized_at, model_sha256, input_sha256, forecast_claim_id,
      first_received_at, decision_at, generated_at, hours FROM rain_adjustment_runs
    WHERE decision_at <= $1::timestamptz AND decision_at > $1::timestamptz - interval '12 hours'
      AND generated_at <= $1::timestamptz
    ORDER BY run_initialized_at DESC, generated_at DESC LIMIT 1
  `, [asOf]);
  const row = result.rows[0];
  // preserve source outages as explicit missing sidecars
  if (row === undefined) {
    return null;
  }
  return {
    runInitializedAt: row.run_initialized_at.toISOString(), modelSha256: row.model_sha256,
    inputSha256: row.input_sha256, forecastClaimId: row.forecast_claim_id,
    firstReceivedAt: row.first_received_at.toISOString(), decisionAt: row.decision_at.toISOString(),
    generatedAt: row.generated_at.toISOString(), hours: row.hours,
  };
}
