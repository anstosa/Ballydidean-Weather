import { createHash } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import {
  appendRainAdjustmentShadow,
  appendRainAdjustmentRun,
  bindAdjustmentRainGateRevision,
  markAdjustmentRevisionGap,
  readPendingRainAdjustmentCaptures,
  registerRainAdjustmentShadow,
  type AdjustmentRevisionCommitReceipt,
  type AdjustmentShadowPrediction,
  type AdjustmentShadowRegistration,
  type createDatabasePool,
  type RainAdjustmentCapture,
  type RainAdjustmentRun,
  type PersistedRainAdjustmentRevision,
  type Queryable,
} from "@weather/database";
import { RAIN_COLLECTION_STATIONS, type JsonValue } from "@weather/domain";
import {
  MAINTENANCE_SHADOW_SOURCE_VERSION,
  MAINTENANCE_SHADOW_VALUES_VERSION,
  MAINTENANCE_SHADOW_COMPARATOR_VERSION,
  ADJUSTMENT_RAIN_SOURCE_CONFIG_FINGERPRINT,
  canonicalJsonBytes,
  adjustmentRevisionClockIsAfterCaptureEpoch,
  adjustmentRevisionLogicalKeySha256,
  adjustmentRevisionProjectionIdentity,
  adjustmentRainGateFeatureRowSha256,
  createRainHurdleWindPortablePerformanceEvaluator,
  createRainMaintenancePersistenceTarget,
  createMaintenanceShadowPredictionMetadata,
  createMaintenanceShadowSourceIdentity,
  decodeMaintenanceBinary64,
  encodeMaintenanceShadowComparator,
  encodeMaintenanceBinary64,
  encodeMaintenanceShadowSourceProjection,
  encodeMaintenanceShadowValues,
  encodeAdjustmentRainGateFeatureProjection,
  encodeAdjustmentRainGateControlProjection,
  evaluateRainMaintenanceControls,
  parseAdjustmentRevisionProjectionDocument,
  parseRainMaintenanceControlState,
  parseMaintenanceShadowSourceProjection,
  predictRainHurdleWindFeatureProjection,
  projectRainHurdleWindFeatures,
  requireAdjustmentRevisionProjectionAfterCaptureEpoch,
  requireMaintenanceShadowSourceAfterCaptureEpoch,
  RAIN_HURDLE_WIND_MODEL_SHA256,
  type LoadedForecastAdjustmentRainRuntimeRegistryV1,
  type InstalledMaintenanceShadowCandidate,
  type AdjustmentRevisionCaptureEpochWitness,
  type RainWindRunProfile,
  type RainWindPerformanceResult,
  type RainHurdleWindFeatureProjection,
  type RainMaintenancePersistenceTarget,
  type RainMaintenanceControlState,
  type RainWindPredictionInput,
  type RainWindStationHour,
  type MaintenanceShadowComparator,
  type MaintenanceShadowServingAuthority,
} from "@weather/forecast-adjustment";
import {
  buildRainFixedGaugeTargetProjection,
  replayRainFixedGaugeTargetHour,
  type RainFixedGaugeTargetBuildResult,
  type RainFixedGaugeTargetSource,
} from "./rain-fixed-gauge-target.js";

export {
  buildRainFixedGaugeTargetProjection,
  replayRainFixedGaugeTargetHour,
  type RainFixedGaugeTargetBuildResult,
  type RainFixedGaugeTargetSource,
};

const HOUR_MS = 3_600_000;
const MINUTE_MS = 60_000;
const REVISION_STAGE_BACKPRESSURE_ATTEMPTS = 7;
const REVISION_STAGE_BACKPRESSURE_INTERVAL_MS = 5_000;
type Pool = ReturnType<typeof createDatabasePool>;

export interface RainAdjustmentMaintenanceStageReceipt {
  readonly comparatorSha256?: string;
  readonly contractVersion: "adjustment-shadow-stage-receipt/v1" | "adjustment-shadow-stage-receipt/v2";
  readonly durable: true;
  readonly durableAt: string;
  readonly dueKey: string;
  readonly predictionBodySha256: string;
  readonly registrationSha256: string;
  readonly sourceProjectionSha256: string;
  readonly stageReceiptSha256: string;
}

export interface RainAdjustmentMaintenanceClient {
  readonly publish: (
    sourceProjection: Uint8Array,
    body: Uint8Array,
    comparator: Uint8Array,
    predictionCommittedAt: string,
    stageReceipt: RainAdjustmentMaintenanceStageReceipt,
    revisionReceipt: AdjustmentRevisionCommitReceipt,
  ) => Promise<void>;
  readonly recordGap: (input: Readonly<{
    dueKey: string;
    family: "rain";
    reason: RainAdjustmentMaintenanceGapReason;
    registrationSha256: string;
  }>) => Promise<void>;
  readonly stage: (
    sourceProjection: Uint8Array,
    body: Uint8Array,
    comparator: Uint8Array,
  ) => Promise<RainAdjustmentMaintenanceStageReceipt>;
  readonly publishRevision: (
    projection: Uint8Array,
    stageReceipt: AdjustmentRevisionStageReceipt,
    revisionReceipt: AdjustmentRevisionCommitReceipt,
  ) => Promise<void>;
  readonly publishRevisionBatch: (
    projection: Uint8Array,
    stageReceipt: AdjustmentRevisionStageReceipt,
    revisionReceipts: readonly AdjustmentRevisionCommitReceipt[],
  ) => Promise<void>;
  readonly recordRevisionGap: (input: AdjustmentRevisionGap) => Promise<void>;
  readonly recordRainControlStateGap: (
    input: RainMaintenanceControlStateGapInput,
  ) => Promise<void>;
  readonly recordRainFixedGaugeTargetGap: (input: Readonly<{
    logicalHourAt: string;
    logicalKeySha256: string;
    reason: "target_source_oversized";
  }>) => Promise<Readonly<{
    contractVersion: "adjustment-rain-fixed-gauge-target-gap/v1";
    gapAt: string;
    gapSha256: string;
    logicalHourAt: string;
    logicalKeySha256: string;
    qualificationDisposition: "forever_unqualified";
    reason: "target_source_oversized";
  }>>;
  readonly readRainFixedGaugeTargetGap: (input: Readonly<{
    logicalHourAt: string;
    logicalKeySha256: string;
  }>) => Promise<Readonly<{
    contractVersion: "adjustment-rain-fixed-gauge-target-gap-status/v1";
    state: "absent";
  }> | Readonly<{
    contractVersion: "adjustment-rain-fixed-gauge-target-gap-status/v1";
    gap: Readonly<{
      contractVersion: "adjustment-rain-fixed-gauge-target-gap/v1";
      gapAt: string;
      gapSha256: string;
      logicalHourAt: string;
      logicalKeySha256: string;
      qualificationDisposition: "forever_unqualified";
      reason: "target_source_oversized";
    }>;
    state: "present";
  }>>;
  readonly stageRainControlState: (
    state: Uint8Array,
  ) => Promise<RainMaintenanceControlStateStageReceipt>;
  readonly stageRevision: (projection: Uint8Array) => Promise<AdjustmentRevisionStageReceipt>;
}

export interface RainMaintenanceControlStateStageReceipt {
  readonly contractVersion: "adjustment-rain-control-state-stage-receipt/v1";
  readonly durable: true;
  readonly durableAt: string;
  readonly stateSha256: string;
  readonly stageReceiptSha256: string;
}

export interface RainMaintenanceControlStateGapInput {
  readonly reason: "projection_stage_failed";
  readonly stateSha256: string;
  readonly stageReceiptSha256: string;
}

// retry only the archive's explicit bounded-capacity refusal
export async function stageAdjustmentRevisionWithBackpressure(
  client: Pick<RainAdjustmentMaintenanceClient, "stageRevision">,
  projection: Uint8Array,
  pause: (milliseconds: number) => Promise<void> = waitForRevisionArchiveCapacity,
): Promise<AdjustmentRevisionStageReceipt> {
  return await stageWithRevisionArchiveBackpressure(
    () => client.stageRevision(projection),
    pause,
  );
}

// retry one monthly state only while the bounded spool is full
export async function stageRainControlStateWithBackpressure(
  client: Pick<RainAdjustmentMaintenanceClient, "stageRainControlState">,
  state: Uint8Array,
  pause: (milliseconds: number) => Promise<void> = waitForRevisionArchiveCapacity,
): Promise<RainMaintenanceControlStateStageReceipt> {
  return await stageWithRevisionArchiveBackpressure(
    () => client.stageRainControlState(state),
    pause,
  );
}

// share the exact bounded wait policy across both payload slots
async function stageWithRevisionArchiveBackpressure<T>(
  operation: () => Promise<T>,
  pause: (milliseconds: number) => Promise<void>,
): Promise<T> {
  // bound six waits to thirty seconds before returning the refusal
  for (let attempt = 0; attempt < REVISION_STAGE_BACKPRESSURE_ATTEMPTS; attempt += 1) {
    try {
      return await operation();
    } catch (error) {
      // never retry validation, transport, archive, or exhausted-capacity failures
      if (!isAdjustmentRevisionSpoolRefusal(error) ||
          attempt === REVISION_STAGE_BACKPRESSURE_ATTEMPTS - 1) {
        throw error;
      }
      await pause(REVISION_STAGE_BACKPRESSURE_INTERVAL_MS);
    }
  }
  throw new Error("adjustment revision stage attempts exhausted");
}

// identify the one retryable internal capacity signal
function isAdjustmentRevisionSpoolRefusal(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error &&
    error.code === "adjustment_revision_spool_refused";
}

// wait without retaining a hidden payload queue
async function waitForRevisionArchiveCapacity(milliseconds: number): Promise<void> {
  await delay(milliseconds);
}

export interface AdjustmentRevisionStageReceipt {
  readonly contractVersion: "adjustment-revision-stage-receipt/v1";
  readonly durable: true;
  readonly durableAt: string;
  readonly projectionIdentitySha256: string;
  readonly projectionKind: "actual_best_match" | "native_source" |
    "rain_gate_input" | "target_revision";
  readonly projectionSha256: string;
  readonly stageReceiptSha256: string;
}

export interface AdjustmentRevisionGap {
  readonly logicalKeySha256: string;
  readonly projectionIdentitySha256: string | null;
  readonly projectionKind: "actual_best_match" | "native_source" |
    "rain_gate_input" | "target_revision";
  readonly projectionSha256: string | null;
  readonly reason: "archive_stage_failed" | "database_bind_failed" |
    "database_admission_failed" | "archive_publish_failed";
}

export interface RainAdjustmentMaintenanceOptions {
  readonly candidate: InstalledMaintenanceShadowCandidate<"rain">;
  readonly captureEpoch: AdjustmentRevisionCaptureEpochWitness;
  readonly client: RainAdjustmentMaintenanceClient;
}

// retain only the root-selected reference members needed for causal control scoring
export interface RainAdjustmentControlReference {
  readonly controlStateBytes: Uint8Array;
  readonly controlStateSha256: string;
  readonly ordinalArtifactBytes: Uint8Array;
  readonly ordinalArtifactSha256: string;
}

export type RainAdjustmentMaintenanceGapReason =
  | "candidate_unavailable"
  | "comparator_unavailable"
  | "source_incomplete"
  | "archive_stage_failed"
  | "database_append_failed"
  | "archive_publish_failed";

export type RainAdjustmentMaintenanceResult =
  | Readonly<{ predictionSha256: string; status: "published" }>
  | Readonly<{ reason: RainAdjustmentMaintenanceGapReason; status: "gap" }>;

export interface RainAdjustmentEvaluation {
  readonly captures: readonly RainAdjustmentCapture[];
  readonly current: RainAdjustmentCapture;
  readonly featureProjection: RainHurdleWindFeatureProjection;
  readonly input: RainWindPredictionInput;
  readonly performance: RainWindPerformanceResult;
  readonly run: RainAdjustmentRun;
}

// decode only source bodies already validated and hash-checked during capture
export function rainForecastProfile(capture: RainAdjustmentCapture): RainWindRunProfile {
  // bind the original six-hour run rather than a retrieval snapshot
  if (capture.kind !== "forecast" || capture.runInitializedAt === null) {
    throw new Error("invalid rain forecast capture");
  }
  const payload = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(capture.body));
  const hourly = payload.hourly as Record<string, (number | string | null)[]>;
  const initialized = Date.parse(capture.runInitializedAt);
  // reject source geometry drift before indexing any features
  if (!Array.isArray(hourly?.time) || hourly.time.length !== 49 || payload.utc_offset_seconds !== 0) {
    throw new Error("invalid rain forecast profile");
  }
  const hours: RainWindRunProfile["hours"][number][] = [];
  // omit initialization and preserve the original one-based lead grid
  for (let lead = 1; lead <= 48; lead += 1) {
    // refuse an hour shifted away from the claimed initialized run
    if (hourly.time[lead] !== new Date(initialized + lead * HOUR_MS).toISOString().slice(0, 16)) {
      throw new Error("rain forecast hour mismatch");
    }
    // retain null covariates without fabricating zeros
    const value = (field: string): number | null => {
      const cell = hourly[field]?.[lead];
      // reject missing schema and nonnumeric fields
      if (cell !== null && (typeof cell !== "number" || !Number.isFinite(cell))) {
        throw new Error("invalid rain forecast value");
      }
      return cell;
    };
    hours.push({
      leadHours: lead,
      precipitationMm: value("precipitation"), temperatureC: value("temperature_2m"),
      relativeHumidityPercent: value("relative_humidity_2m"), cloudCoverPercent: value("cloud_cover"),
      pressureHpa: value("surface_pressure"), windSpeedMps: value("wind_speed_10m"),
      windDirectionDegrees: value("wind_direction_10m"),
    });
  }
  return { runInitializedAt: capture.runInitializedAt, completedAt: capture.completedAt, hours };
}

// reconstruct exact lagged gauge hours without future receipts or interval interpolation
export function rainStationHours(
  captures: readonly RainAdjustmentCapture[],
  decisionAt: string,
): readonly RainWindStationHour[] {
  const result: RainWindStationHour[] = [];
  // reproduce only the model's frozen causal lag requests
  for (const lag of [1, 2, 3, 6, 12, 24]) {
    const validAt = new Date(Date.parse(decisionAt) - lag * HOUR_MS).toISOString();
    const replay = replayRainFixedGaugeTargetHour(captures, validAt, decisionAt, false);
    // preserve every available fixed gauge without creating a receipt for absent values
    for (const row of replay) {
      if (row.receivedAt !== null && (row.precipitationMm !== null || row.temperatureC !== null)) {
        result.push({
          hourAt: validAt,
          precipitationMm: row.precipitationMm,
          receivedAt: row.receivedAt,
          stationId: row.stationId,
          temperatureC: row.temperatureC,
        });
      }
    }
  }
  return result;
}

// freeze one current inference from the existing model without training or extra requests
export function createRainAdjustmentRun(
  captures: readonly RainAdjustmentCapture[],
  nowUtc: string,
): RainAdjustmentRun | null {
  return createRainAdjustmentEvaluation(captures, nowUtc)?.run ?? null;
}

// evaluate one retained input set once for serving and shadow evidence
export function createRainAdjustmentEvaluation(
  captures: readonly RainAdjustmentCapture[],
  nowUtc: string,
): RainAdjustmentEvaluation | null {
  const forecastCaptures = captures.filter((capture) => capture.kind === "forecast")
    .sort((left, right) => Date.parse(right.runInitializedAt!) - Date.parse(left.runInitializedAt!));
  const current = forecastCaptures[0];
  // preserve a missing source as raw instead of inventing another provider
  if (current?.runInitializedAt == null) {
    return null;
  }
  const decisionAt = new Date(Date.parse(current.runInitializedAt) + 8 * HOUR_MS).toISOString();
  const input = {
    currentRun: rainForecastProfile(current),
    priorRuns: forecastCaptures.slice(1).map(rainForecastProfile),
    stationHours: rainStationHours(captures, decisionAt),
    nowUtc,
  };
  const featureProjection = projectRainHurdleWindFeatures(input);
  const performance = predictRainHurdleWindFeatureProjection(featureProjection);
  const run: RainAdjustmentRun = {
    runInitializedAt: current.runInitializedAt, modelSha256: performance.modelSha256,
    inputSha256: createHash("sha256").update(JSON.stringify(captures.map((capture) => [
      capture.claimId, capture.bodySha256, capture.completedAt,
    ]))).digest("hex"),
    forecastClaimId: current.claimId, firstReceivedAt: current.completedAt,
    decisionAt, generatedAt: nowUtc, hours: performance.hours.map((hour) => ({
      applied: hour.applied,
      correctedPrecipitationMm: hour.correctedPrecipitationMm,
      modelLeadHours: hour.modelLeadHours,
      rawPrecipitationMm: hour.rawPrecipitationMm,
      reasonCode: hour.reasonCode,
      validAt: hour.validAt,
    })),
  };
  return { captures, current, featureProjection, input, performance, run };
}

// bind the exact prior-hour station graph consumed by the persistence control
export function createRainAdjustmentPersistenceTarget(
  evaluation: RainAdjustmentEvaluation,
): RainMaintenancePersistenceTarget {
  const decisionAt = evaluation.run.decisionAt;
  const decision = Date.parse(decisionAt);
  const validAt = new Date(decision - HOUR_MS).toISOString();
  const earliestEndpoint = decision - HOUR_MS - 5 * MINUTE_MS;
  const earliestIntervalStart = earliestEndpoint - HOUR_MS;
  const stationHours = new Map(evaluation.input.stationHours
    .filter((row) => row.hourAt === validAt)
    .map((row) => [row.stationId, row]));
  return createRainMaintenancePersistenceTarget({
    decisionAt,
    rows: RAIN_COLLECTION_STATIONS.map((station) => {
      const hour = stationHours.get(station.locationId);
      const captureMembers = evaluation.captures.filter((capture) => {
        // retain every pre-decision station response overlapping the reconstructed hour
        if (capture.kind !== "station" || capture.stationId !== station.locationId ||
            capture.windowStart === null || capture.windowEndExclusive === null ||
            Date.parse(capture.completedAt) > decision) {
          return false;
        }
        return Date.parse(capture.windowStart) <= decision - HOUR_MS &&
          Date.parse(capture.windowEndExclusive) > earliestIntervalStart;
      }).map((capture) => ({
        bodySha256: capture.bodySha256,
        claimId: capture.claimId,
        completedAt: capture.completedAt,
      }));
      return {
        captureMembers,
        precipitationMm: hour?.precipitationMm ?? null,
        receivedAt: hour?.receivedAt ?? null,
        stationId: station.locationId,
      };
    }),
  });
}

// isolate inference storage from the append-only provider collection loop
export async function publishRainAdjustment(
  pool: Pool,
  now: Date = new Date(),
  runtime?: LoadedForecastAdjustmentRainRuntimeRegistryV1,
  maintenance?: RainAdjustmentMaintenanceOptions,
  revisionClient?: RainAdjustmentMaintenanceClient,
  captureEpoch?: AdjustmentRevisionCaptureEpochWitness,
  controlReference?: RainAdjustmentControlReference,
): Promise<boolean> {
  const rawSelected = runtime?.state === "disabled" &&
    runtime.reasonCode === "policy_raw" &&
    runtime.comparatorAuthority !== undefined;
  // invalid startup policy stops before capture storage or model inference
  if (runtime?.state === "disabled" && !rawSelected) {
    return false;
  }
  const captures = await readPendingRainAdjustmentCaptures(pool, RAIN_HURDLE_WIND_MODEL_SHA256);
  const evaluation = createRainAdjustmentEvaluation(captures, now.toISOString());
  // stop without storage when no mature source exists
  if (evaluation === null) {
    return false;
  }
  const servingEvaluation = rawSelected
    ? { ...evaluation, run: createPolicyRawRainAdjustmentRun(evaluation.run) }
    : evaluation;
  const published = await appendRainAdjustmentRun(
    pool,
    servingEvaluation.run,
    revisionClient === undefined || captureEpoch === undefined
      ? undefined
      : createRainAdjustmentRevisionArchiver(
          pool,
          revisionClient,
          captureEpoch,
              servingEvaluation,
              undefined,
              controlReference,
              runtime,
        ),
  );
  // keep shadow failures isolated from the already-served sidecar
  if (maintenance !== undefined) {
    try {
      await publishRainAdjustmentMaintenanceShadow(
        pool,
        evaluation,
        now.toISOString(),
        maintenance,
        runtime,
      );
    } catch {
      // preserve serving on every unclassified shadow failure
    }
  }
  return published;
}

// persist the exact operator-selected raw decision without executing an adjustment
function createPolicyRawRainAdjustmentRun(run: RainAdjustmentRun): RainAdjustmentRun {
  return {
    ...run,
    hours: run.hours.map(
      // preserve native values while making the serving gate explicitly raw
      (hour) => ({
        ...hour,
        applied: false,
        correctedPrecipitationMm: hour.rawPrecipitationMm,
        reasonCode: "policy_raw",
      }),
    ),
  };
}

// create one transactional rain-gate archive adapter for the database append
export function createRainAdjustmentRevisionArchiver(
  pool: Pool,
  client: RainAdjustmentMaintenanceClient,
  captureEpoch: AdjustmentRevisionCaptureEpochWitness,
  evaluation: RainAdjustmentEvaluation,
  pause?: (milliseconds: number) => Promise<void>,
  controlReference?: RainAdjustmentControlReference,
  incumbentRuntime?: LoadedForecastAdjustmentRainRuntimeRegistryV1,
) {
  return async (
    queryable: Queryable,
    revision: PersistedRainAdjustmentRevision,
  ): Promise<Readonly<{ publish: () => Promise<void> }>> => {
    const logicalKey = {
      inputSha256: revision.inputSha256,
      modelSha256: revision.modelSha256,
      runInitializedAt: revision.runInitializedAt,
    };
    const logicalKeySha256 = adjustmentRevisionLogicalKeySha256("rain_gate_input", logicalKey);
    const databaseKey = {
      ...logicalKey,
      storedContentSha256: revision.storedContentSha256,
    };
    let projection: Buffer;
    let identity: string;
    // encode only the exact immutable row returned by the database
    try {
      requireRainFeatureInputsAfterCaptureEpoch(captureEpoch, evaluation);
      validateRainFeatureProjectionBinding(evaluation, revision);
      projection = controlReference === undefined || incumbentRuntime === undefined
        ? encodeAdjustmentRainGateFeatureProjection({
            contractVersion: "adjustment-rain-gate-feature-projection/v2",
            family: "rain",
            logicalKey,
            logicalReceivedAt: revision.generatedAt,
            projectionKind: "rain_gate_input",
            rows: evaluation.featureProjection.rows.map(
              // retain every actual pre-fit vector and native source target
              (row) => ({
                features64: Array.from(row.features, encodeRainFeatureValue),
                modelLeadHours: row.modelLeadHours,
                rawPrecipitationMm64: encodeMaintenanceBinary64(row.rawPrecipitationMm),
                rawTargetHourTemperatureC64:
                  encodeMaintenanceBinary64(row.rawTargetHourTemperatureC),
                validAt: row.validAt,
              }),
            ),
            source: {
              adapterVersion: "rain-hurdle-wind-features/v1",
              contractEpoch: "rain-prospective-capture/v1",
              dataset: "ecmwf_ifs",
              providerKey: "open-meteo-single-runs",
              sourceConfigFingerprint: ADJUSTMENT_RAIN_SOURCE_CONFIG_FINGERPRINT,
              sourceId: evaluation.current.claimId,
              sourceKey: "rain-prospective-forecast",
              sourceKind: "forecast",
              upstreamModel: "ecmwf_ifs",
            },
            storedContentSha256: revision.storedContentSha256,
          })
        : await createRainAdjustmentControlProjection(
            client,
            captureEpoch,
            evaluation,
            revision,
            controlReference,
            incumbentRuntime,
            pause,
          );
      requireAdjustmentRevisionProjectionAfterCaptureEpoch(
        captureEpoch,
        parseAdjustmentRevisionProjectionDocument(projection),
      );
      identity = adjustmentRevisionProjectionIdentity(projection);
    } catch {
      const gap = rainAdjustmentRevisionGap(logicalKeySha256, null, "archive_stage_failed");
      await markAdjustmentRevisionGap(queryable, "rain_gate", databaseKey, gap);
      return { publish: () => recordRainAdjustmentRevisionGap(client, gap) };
    }
    let stageReceipt: AdjustmentRevisionStageReceipt;
    // require a durable receipt before the database assigns a frontier ordinal
    try {
      stageReceipt = await stageAdjustmentRevisionWithBackpressure(client, projection, pause);
      validateRainAdjustmentRevisionStageReceipt(stageReceipt, identity);
    } catch {
      const parsed = parseAdjustmentRevisionProjectionDocument(projection);
      // abandon the auxiliary only when no staged control projection retained it
      if (parsed.contractVersion === "adjustment-rain-gate-control-projection/v3") {
        await recordRainControlStateGap(client, {
          reason: "projection_stage_failed",
          stateSha256: parsed.stateSha256,
          stageReceiptSha256: parsed.stateStageReceiptSha256,
        });
      }
      const gap = rainAdjustmentRevisionGap(logicalKeySha256, identity, "archive_stage_failed");
      await markAdjustmentRevisionGap(queryable, "rain_gate", databaseKey, gap);
      return { publish: () => recordRainAdjustmentRevisionGap(client, gap) };
    }
    let revisionReceipt: AdjustmentRevisionCommitReceipt;
    // bind the durable body to the exact current rain gate in the same transaction
    await queryable.query("SAVEPOINT rain_adjustment_revision_bind");
    try {
      revisionReceipt = (await bindAdjustmentRainGateRevision(queryable, {
        ...databaseKey,
        projectionIdentitySha256: identity,
        projectionSha256: identity,
        stageReceiptSha256: stageReceipt.stageReceiptSha256,
      })).revisionReceipt;
      await queryable.query("RELEASE SAVEPOINT rain_adjustment_revision_bind");
    } catch {
      // recover SQL failure before committing the serving row's permanent gap
      await queryable.query("ROLLBACK TO SAVEPOINT rain_adjustment_revision_bind");
      await queryable.query("RELEASE SAVEPOINT rain_adjustment_revision_bind");
      const gap = rainAdjustmentRevisionGap(logicalKeySha256, identity, "database_bind_failed");
      await markAdjustmentRevisionGap(queryable, "rain_gate", databaseKey, gap);
      return { publish: () => recordRainAdjustmentRevisionGap(client, gap) };
    }
    return {
      // publish only after appendRainAdjustmentRun commits the serving transaction
      async publish() {
        try {
          await client.publishRevision(projection, stageReceipt, revisionReceipt);
        } catch (error) {
          const gap = rainAdjustmentRevisionGap(logicalKeySha256, identity, "archive_publish_failed");
          let marked = false;
          // preserve a stricter api-side admission marker if it already won the race
          try {
            await markAdjustmentRevisionGap(pool, "rain_gate", databaseKey, gap);
            marked = true;
          } catch {
            // the api may already have persisted a different permanent category
          }
          // archive the category only when this caller persisted the matching marker
          if (marked) {
            await recordRainAdjustmentRevisionGap(client, gap);
          }
          throw error;
        }
      },
    };
  };
}

// stage one installed monthly state before scoring its exact pre-target controls
async function createRainAdjustmentControlProjection(
  client: RainAdjustmentMaintenanceClient,
  captureEpoch: AdjustmentRevisionCaptureEpochWitness,
  evaluation: RainAdjustmentEvaluation,
  revision: PersistedRainAdjustmentRevision,
  reference: RainAdjustmentControlReference,
  incumbentRuntime: LoadedForecastAdjustmentRainRuntimeRegistryV1,
  pause?: (milliseconds: number) => Promise<void>,
): Promise<Buffer> {
  const stateBytes = Buffer.from(reference.controlStateBytes);
  const ordinalArtifactBytes = Buffer.from(reference.ordinalArtifactBytes);
  const state = parseRainMaintenanceControlState(stateBytes);
  // require the complete root-selected control tuple and future-only lineage
  if (createHash("sha256").update(stateBytes).digest("hex") !== reference.controlStateSha256 ||
      createHash("sha256").update(ordinalArtifactBytes).digest("hex") !==
        reference.ordinalArtifactSha256 ||
      state.ordinalArtifactSha256 !== reference.ordinalArtifactSha256 ||
      state.epochWitnessSha256 !== captureEpoch.witnessSha256 ||
      !adjustmentRevisionClockIsAfterCaptureEpoch(captureEpoch, state.trainingMaximumValidAt) ||
      !adjustmentRevisionClockIsAfterCaptureEpoch(captureEpoch, state.generatedAt) ||
      state.modelMonth !== revision.runInitializedAt.slice(0, 7)) {
    throw new RangeError("rain control package lineage differs");
  }
  const persistenceTarget = createRainAdjustmentPersistenceTarget(evaluation);
  const stateReceipt = await stageRainControlStateWithBackpressure(client, stateBytes, pause);
  try {
    validateRainControlStateStageReceipt(stateReceipt, reference.controlStateSha256, state.generatedAt);
    const authority = incumbentRuntime.comparatorAuthority;
    // require the same immutable serving authority used by the shadow comparator
    if (authority === undefined ||
        (incumbentRuntime.state !== "active" && incumbentRuntime.reasonCode !== "policy_raw")) {
      throw new RangeError("rain control incumbent authority differs");
    }
    const policyRaw = authority.authorityKind === "policy_raw";
    const logicalKey = {
      inputSha256: revision.inputSha256,
      modelSha256: revision.modelSha256,
      runInitializedAt: revision.runInitializedAt,
    };
    const persistencePrediction = persistenceTarget.prediction64 === null
      ? null
      : decodeMaintenanceBinary64(persistenceTarget.prediction64);
    const rows = evaluation.featureProjection.rows.map((row, index) => {
      const incumbent = evaluation.performance.hours[index];
      // cross-bind the incumbent execution to the same feature row and lead clock
      if (incumbent === undefined || incumbent.validAt !== row.validAt ||
          incumbent.modelLeadHours !== row.modelLeadHours) {
        throw new RangeError("rain control incumbent row differs");
      }
      const controls = evaluateRainMaintenanceControls({
        artifactBytes: ordinalArtifactBytes,
        features: row.features,
        persistencePrediction,
        rawPrecipitationMm: row.rawPrecipitationMm,
        state,
      });
      const probabilities = policyRaw || incumbent.occurrenceProbabilities === null
        ? controls.nativeSourceProbability
        : incumbent.occurrenceProbabilities;
      const source = {
        features64: Array.from(row.features, encodeRainFeatureValue),
        modelLeadHours: row.modelLeadHours,
        rawPrecipitationMm64: encodeMaintenanceBinary64(row.rawPrecipitationMm),
        rawTargetHourTemperatureC64: encodeMaintenanceBinary64(row.rawTargetHourTemperatureC),
        validAt: row.validAt,
      };
      return {
        ...source,
        incumbentArtifactIdentitySha256: authority.artifactIdentitySha256,
        incumbentPrediction64: encodeMaintenanceBinary64(
          policyRaw || !incumbent.applied
            ? incumbent.rawPrecipitationMm
            : incumbent.correctedPrecipitationMm,
        ),
        incumbentProbability: encodeRainControlProbability(probabilities),
        incumbentReceiptMemberSha256: authority.receiptMemberSha256,
        nativeSourceProbability: encodeRainControlProbability(controls.nativeSourceProbability),
        persistencePrediction64: encodeMaintenanceBinary64(controls.persistencePrediction),
        persistenceReason: controls.persistenceReason,
        persistenceTargetMemberSha256: controls.persistenceReason === "causal_target"
          ? persistenceTarget.targetMemberSha256
          : null,
        recentVolumeScalePrediction64:
          encodeMaintenanceBinary64(controls.recentVolumeScalePrediction),
        sameWindowVolumeScalePrediction64:
          encodeMaintenanceBinary64(controls.sameWindowVolumeScalePrediction),
        sourceRowSha256: adjustmentRainGateFeatureRowSha256(logicalKey, source),
        unchangedOrdinalPrediction64:
          encodeMaintenanceBinary64(controls.unchangedOrdinalPrediction),
        volumeScalePrediction64: encodeMaintenanceBinary64(controls.volumeScalePrediction),
      };
    });
    return encodeAdjustmentRainGateControlProjection({
      contractVersion: "adjustment-rain-gate-control-projection/v3",
      family: "rain",
      logicalKey,
      logicalReceivedAt: revision.generatedAt,
      ordinalArtifactSha256: reference.ordinalArtifactSha256,
      persistenceTarget,
      projectionKind: "rain_gate_input",
      rows,
      source: {
        adapterVersion: "rain-hurdle-wind-features/v1",
        contractEpoch: "rain-prospective-capture/v1",
        dataset: "ecmwf_ifs",
        providerKey: "open-meteo-single-runs",
        sourceConfigFingerprint: ADJUSTMENT_RAIN_SOURCE_CONFIG_FINGERPRINT,
        sourceId: evaluation.current.claimId,
        sourceKey: "rain-prospective-forecast",
        sourceKind: "forecast",
        upstreamModel: "ecmwf_ifs",
      },
      stateSha256: reference.controlStateSha256,
      stateStageReceiptSha256: stateReceipt.stageReceiptSha256,
      storedContentSha256: revision.storedContentSha256,
    });
  } catch (error) {
    await recordRainControlStateGap(client, {
      reason: "projection_stage_failed",
      stateSha256: reference.controlStateSha256,
      stageReceiptSha256: stateReceipt.stageReceiptSha256,
    });
    throw error;
  }
}

// record one exact abandoned state while allowing a concurrently staged body to win
async function recordRainControlStateGap(
  client: Pick<RainAdjustmentMaintenanceClient, "recordRainControlStateGap">,
  input: RainMaintenanceControlStateGapInput,
): Promise<void> {
  try {
    await client.recordRainControlStateGap(input);
  } catch {
    // a staged control body owns the state when the archive rejects abandonment
  }
}

// encode one exact nested event probability vector in binary64 form
function encodeRainControlProbability(value: Readonly<{
  atLeast0_1: number;
  atLeast1_0: number;
  atLeast2_5: number;
}>): Readonly<Record<"atLeast0_1" | "atLeast1_0" | "atLeast2_5", string>> {
  return {
    atLeast0_1: encodeMaintenanceBinary64(value.atLeast0_1),
    atLeast1_0: encodeMaintenanceBinary64(value.atLeast1_0),
    atLeast2_5: encodeMaintenanceBinary64(value.atLeast2_5),
  };
}

// bind a state stage receipt to the exact canonical state bytes
function validateRainControlStateStageReceipt(
  receipt: RainMaintenanceControlStateStageReceipt,
  stateSha256: string,
  generatedAt: string,
): void {
  const unsigned = {
    contractVersion: receipt.contractVersion,
    durable: receipt.durable,
    durableAt: receipt.durableAt,
    stateSha256: receipt.stateSha256,
  };
  // reject generic acknowledgements and pre-state archive clocks
  if (receipt.contractVersion !== "adjustment-rain-control-state-stage-receipt/v1" ||
      receipt.durable !== true || receipt.stateSha256 !== stateSha256 ||
      !Number.isFinite(Date.parse(receipt.durableAt)) ||
      new Date(receipt.durableAt).toISOString() !== receipt.durableAt ||
      Date.parse(receipt.durableAt) < Date.parse(generatedAt) ||
      receipt.stageReceiptSha256 !== createHash("sha256")
        .update(`${JSON.stringify(unsigned)}\n`).digest("hex")) {
    throw new RangeError("rain control state stage receipt differs");
  }
}

// require every causal capture and derived observation to follow the root epoch
function requireRainFeatureInputsAfterCaptureEpoch(
  witness: AdjustmentRevisionCaptureEpochWitness,
  evaluation: RainAdjustmentEvaluation,
): void {
  const clocks: (string | null)[] = [];
  // retain every raw capture receipt and source interval boundary
  for (const capture of evaluation.captures) {
    clocks.push(capture.completedAt, capture.runInitializedAt, capture.windowStart,
      capture.windowEndExclusive);
  }
  // retain every decoded run receipt and initialization consumed by the builder
  for (const run of [evaluation.input.currentRun, ...evaluation.input.priorRuns]) {
    clocks.push(run.runInitializedAt, run.completedAt);
  }
  // retain every gauge observation and receipt consumed by the builder
  for (const station of evaluation.input.stationHours) {
    clocks.push(station.hourAt, station.receivedAt);
  }
  // ignore mutually exclusive null capture fields but reject any old actual clock
  if (clocks.some((clock) => clock !== null &&
      !adjustmentRevisionClockIsAfterCaptureEpoch(witness, clock))) {
    throw new RangeError("rain feature input predates capture epoch");
  }
}

// bind one retained feature graph to the exact persisted serving gate
function validateRainFeatureProjectionBinding(
  evaluation: RainAdjustmentEvaluation,
  revision: PersistedRainAdjustmentRevision,
): void {
  const projected = evaluation.featureProjection;
  // require one causal run and one immutable database key
  if (projected.runInitializedAt !== revision.runInitializedAt ||
      projected.decisionAt !== evaluation.run.decisionAt ||
      evaluation.run.inputSha256 !== revision.inputSha256 ||
      evaluation.run.modelSha256 !== revision.modelSha256 ||
      projected.rows.length !== 23 || revision.hours.length !== 23 ||
      evaluation.run.hours.length !== 23) {
    throw new RangeError("rain feature projection gate identity differs");
  }
  // cross-bind every native row to the exact serving row inserted in this transaction
  for (const [index, feature] of projected.rows.entries()) {
    const stored = revision.hours[index];
    const evaluated = evaluation.run.hours[index];
    if (stored === undefined || evaluated === undefined ||
        feature.modelLeadHours !== stored.modelLeadHours ||
        feature.rawPrecipitationMm !== stored.rawPrecipitationMm ||
        feature.validAt !== stored.validAt ||
        stored.applied !== evaluated.applied ||
        stored.correctedPrecipitationMm !== evaluated.correctedPrecipitationMm ||
        stored.modelLeadHours !== evaluated.modelLeadHours ||
        stored.rawPrecipitationMm !== evaluated.rawPrecipitationMm ||
        stored.reasonCode !== evaluated.reasonCode || stored.validAt !== evaluated.validAt) {
      throw new RangeError("rain feature projection gate row differs");
    }
  }
}

// preserve explicit missing predictors without inventing numeric values
function encodeRainFeatureValue(value: number): string | null {
  // the native builder uses only NaN for an unavailable causal feature
  if (Number.isNaN(value)) {
    return null;
  }
  return encodeMaintenanceBinary64(value);
}

// create one closed permanent rain-gate gap document
function rainAdjustmentRevisionGap(
  logicalKeySha256: string,
  identity: string | null,
  reason: AdjustmentRevisionGap["reason"],
): AdjustmentRevisionGap {
  return {
    logicalKeySha256,
    projectionIdentitySha256: identity,
    projectionKind: "rain_gate_input",
    projectionSha256: identity,
    reason,
  };
}

// relay one already-persisted permanent gap without affecting serving
async function recordRainAdjustmentRevisionGap(
  client: RainAdjustmentMaintenanceClient,
  gap: AdjustmentRevisionGap,
): Promise<void> {
  try {
    await client.recordRevisionGap(gap);
  } catch {
    // keep the database's permanent marker authoritative during archive outage
  }
}

// validate a durable stage receipt against the whole canonical rain body
function validateRainAdjustmentRevisionStageReceipt(
  receipt: AdjustmentRevisionStageReceipt,
  identity: string,
): void {
  const unsigned = {
    contractVersion: receipt.contractVersion,
    durable: receipt.durable,
    durableAt: receipt.durableAt,
    projectionIdentitySha256: receipt.projectionIdentitySha256,
    projectionKind: receipt.projectionKind,
    projectionSha256: receipt.projectionSha256,
  };
  // reject a generic acknowledgement or a self-hash over different fields
  if (receipt.contractVersion !== "adjustment-revision-stage-receipt/v1" || receipt.durable !== true ||
      receipt.projectionKind !== "rain_gate_input" || receipt.projectionIdentitySha256 !== identity ||
      receipt.projectionSha256 !== identity || !Number.isFinite(Date.parse(receipt.durableAt)) ||
      new Date(receipt.durableAt).toISOString() !== receipt.durableAt ||
      receipt.stageReceiptSha256 !== createHash("sha256")
        .update(`${JSON.stringify(unsigned)}\n`).digest("hex")) {
    throw new RangeError("rain adjustment revision stage receipt differs");
  }
}

// stage, append and publish one compiled-artifact rain shadow body
export async function publishRainAdjustmentMaintenanceShadow(
  pool: Pool,
  evaluation: RainAdjustmentEvaluation,
  issuedAt: string,
  options: RainAdjustmentMaintenanceOptions,
  incumbentRuntime?: LoadedForecastAdjustmentRainRuntimeRegistryV1,
): Promise<RainAdjustmentMaintenanceResult> {
  const dueKey = rainMaintenanceDueKey(evaluation.current);
  // fail closed before registration when immutable deployment proof is absent
  try {
    validateRainInstalledCandidate(options.candidate);
  } catch {
    return await reportRainMaintenanceGap(options, dueKey, "candidate_unavailable");
  }
  let sourceBytes: Buffer;
  let bodyBytes: Buffer;
  let comparatorBytes: Buffer;
  let metadata: ReturnType<typeof createMaintenanceShadowPredictionMetadata>;
  // bind the compiled prediction to exact retained capture bytes and receipt
  try {
    const artifactJson = canonicalJsonBytes(options.candidate.bundle as JsonValue);
    const evaluateCandidate = createRainHurdleWindPortablePerformanceEvaluator(
      artifactJson,
      options.candidate.receipt.bundleSha256,
    );
    const candidatePerformance = evaluateCandidate(evaluation.input);
    sourceBytes = createRainMaintenanceSourceProjection(
      evaluation,
      issuedAt,
      dueKey,
      options.candidate.registration,
    );
    requireMaintenanceShadowSourceAfterCaptureEpoch(
      options.captureEpoch,
      parseMaintenanceShadowSourceProjection(sourceBytes),
    );
    bodyBytes = createRainMaintenanceBody(sourceBytes, candidatePerformance);
    metadata = createMaintenanceShadowPredictionMetadata(bodyBytes, sourceBytes);
  } catch {
    return await reportRainMaintenanceGap(options, dueKey, "source_incomplete");
  }
  // bind the exact already-executed incumbent and immutable startup authority
  try {
    if (incumbentRuntime === undefined ||
        incumbentRuntime.comparatorAuthority === undefined ||
        (incumbentRuntime.state !== "active" && incumbentRuntime.reasonCode !== "policy_raw")) {
      throw new RangeError("rain incumbent runtime is unavailable");
    }
    comparatorBytes = createRainMaintenanceComparator(
      sourceBytes,
      bodyBytes,
      evaluation.performance,
      incumbentRuntime.comparatorAuthority,
    );
  } catch {
    return await reportRainMaintenanceGap(options, dueKey, "comparator_unavailable");
  }
  let stageReceipt: RainAdjustmentMaintenanceStageReceipt;
  // require archive durability before compact database state
  try {
    stageReceipt = await options.client.stage(sourceBytes, bodyBytes, comparatorBytes);
    validateRainStageReceipt(stageReceipt, metadata, sourceBytes, comparatorBytes);
  } catch {
    return await reportRainMaintenanceGap(options, dueKey, "archive_stage_failed");
  }
  let predictionCommittedAt: string;
  let revisionReceipt: AdjustmentRevisionCommitReceipt;
  // register only the deployed candidate and append through the ingest role
  try {
    await registerRainAdjustmentShadow(pool, options.candidate.registration);
    const prediction = {
      ...metadata,
      stageReceiptSha256: stageReceipt.stageReceiptSha256,
    } as AdjustmentShadowPrediction;
    const appended = await appendRainAdjustmentShadow(pool, prediction);
    predictionCommittedAt = appended.committedAt;
    revisionReceipt = appended.revisionReceipt;
  } catch {
    return await reportRainMaintenanceGap(options, dueKey, "database_append_failed");
  }
  // let the API role perform exact admission before archive publication
  try {
    await options.client.publish(sourceBytes, bodyBytes, comparatorBytes, predictionCommittedAt,
      stageReceipt, revisionReceipt);
  } catch {
    return await reportRainMaintenanceGap(options, dueKey, "archive_publish_failed");
  }
  return { predictionSha256: metadata.predictionSha256, status: "published" };
}

// bind a durable stage receipt to the exact source and body identities
function validateRainStageReceipt(
  receipt: RainAdjustmentMaintenanceStageReceipt,
  metadata: ReturnType<typeof createMaintenanceShadowPredictionMetadata>,
  sourceBytes: Uint8Array,
  comparatorBytes: Uint8Array,
): void {
  const sourceProjectionSha256 = createHash("sha256").update(sourceBytes).digest("hex");
  const comparatorSha256 = createHash("sha256").update(comparatorBytes).digest("hex");
  const unsigned = {
    contractVersion: receipt.contractVersion,
    durable: receipt.durable,
    durableAt: receipt.durableAt,
    dueKey: receipt.dueKey,
    predictionBodySha256: receipt.predictionBodySha256,
    registrationSha256: receipt.registrationSha256,
    sourceProjectionSha256: receipt.sourceProjectionSha256,
    comparatorSha256: receipt.comparatorSha256,
  };
  // reject generic acknowledgements before compact database mutation
  if (receipt.contractVersion !== "adjustment-shadow-stage-receipt/v2" || receipt.durable !== true ||
      receipt.registrationSha256 !== metadata.registrationSha256 || receipt.dueKey !== metadata.dueKey ||
      receipt.predictionBodySha256 !== metadata.predictionBodySha256 ||
      receipt.sourceProjectionSha256 !== sourceProjectionSha256 ||
      receipt.comparatorSha256 !== comparatorSha256 ||
      !/^[a-f0-9]{64}$/u.test(receipt.stageReceiptSha256) ||
      !Number.isFinite(Date.parse(receipt.durableAt)) ||
      new Date(receipt.durableAt).toISOString() !== receipt.durableAt ||
      Date.parse(receipt.durableAt) < Date.parse(metadata.issuedAt) ||
      receipt.stageReceiptSha256 !== createHash("sha256")
        .update(`${JSON.stringify(unsigned)}\n`).digest("hex")) {
    throw new RangeError("rain maintenance stage receipt differs");
  }
}

// construct a private data-network client without adding credentials or providers
export function createHttpRainAdjustmentMaintenanceClient(
  origin: string,
): RainAdjustmentMaintenanceClient {
  const base = new URL(origin);
  // accept only one credential-free private http origin
  if (base.protocol !== "http:" || !["api", "127.0.0.1", "localhost", "[::1]"].includes(base.hostname) ||
      base.username !== "" || base.password !== "" ||
      base.pathname !== "/" || base.search !== "" || base.hash !== "") {
    throw new RangeError("rain maintenance api origin is invalid");
  }
  return {
    // ask the api role to admit and publish the exact staged body
    async publish(sourceProjection, body, comparator, predictionCommittedAt,
      stageReceipt, revisionReceipt) {
      await postRainMaintenance(base, "/internal/adjustment-maintenance/shadow/publish", {
        bodyBase64: Buffer.from(body).toString("base64"),
        comparatorBase64: Buffer.from(comparator).toString("base64"),
        predictionCommittedAt,
        revisionReceipt,
        sourceProjectionBase64: Buffer.from(sourceProjection).toString("base64"),
        stageReceipt,
      });
    },
    // ask the api role to authenticate and publish one persisted revision
    async publishRevision(projection, stageReceipt, revisionReceipt) {
      await postRainMaintenance(base, "/internal/adjustment-maintenance/revision/publish", {
        projectionBase64: Buffer.from(projection).toString("base64"),
        revisionReceipt,
        stageReceipt,
      });
    },
    // ask the api role to authenticate every row receipt before one grouped publication
    async publishRevisionBatch(projection, stageReceipt, revisionReceipts) {
      await postRainMaintenance(base, "/internal/adjustment-maintenance/revision/publish", {
        projectionBase64: Buffer.from(projection).toString("base64"),
        revisionReceipts,
        stageReceipt,
      });
    },
    // relay only a categorical value-free gap
    async recordGap(input) {
      await postRainMaintenance(base, "/internal/adjustment-maintenance/shadow/gap", input);
    },
    // relay one permanent value-free revision gap
    async recordRevisionGap(input) {
      await postRainMaintenance(base, "/internal/adjustment-maintenance/revision/gap", input);
    },
    // abandon only an exact state that never reached a staged control projection
    async recordRainControlStateGap(input) {
      await postRainMaintenance(
        base,
        "/internal/adjustment-maintenance/rain-control-state/gap",
        input,
      );
    },
    // retain one value-free oversized target hour before body staging
    async recordRainFixedGaugeTargetGap(input) {
      return await postRainMaintenance(
        base,
        "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap",
        input,
      );
    },
    // avoid reconstructing an hour already classified permanently oversized
    async readRainFixedGaugeTargetGap(input) {
      return await postRainMaintenance(
        base,
        "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status",
        input,
      );
    },
    // stage exact bytes before the ingest-role compact append
    async stage(sourceProjection, body, comparator) {
      return await postRainMaintenance<RainAdjustmentMaintenanceStageReceipt>(
        base,
        "/internal/adjustment-maintenance/shadow/stage",
        {
          bodyBase64: Buffer.from(body).toString("base64"),
          comparatorBase64: Buffer.from(comparator).toString("base64"),
          sourceProjectionBase64: Buffer.from(sourceProjection).toString("base64"),
        },
      );
    },
    // stage one root-selected monthly control state before any control scoring
    async stageRainControlState(state) {
      return await postRainMaintenance<RainMaintenanceControlStateStageReceipt>(
        base,
        "/internal/adjustment-maintenance/rain-control-state/stage",
        { stateBase64: Buffer.from(state).toString("base64") },
      );
    },
    // stage one canonical revision body before its serving pointer bind
    async stageRevision(projection) {
      return await postRainMaintenance<AdjustmentRevisionStageReceipt>(
        base,
        "/internal/adjustment-maintenance/revision/stage",
        { projectionBase64: Buffer.from(projection).toString("base64") },
      );
    },
  };
}

// encode the exact selected rain forecast capture as a closed source projection
function createRainMaintenanceSourceProjection(
  evaluation: RainAdjustmentEvaluation,
  issuedAt: string,
  dueKey: string,
  registration: AdjustmentShadowRegistration<"rain">,
): Buffer {
  const profile = rainForecastProfile(evaluation.current);
  const forecastCaptures = evaluation.captures.filter(
    // retain the same sorted current and prior run selection used by inference
    (capture) => capture.kind === "forecast",
  ).sort((left, right) => Date.parse(right.runInitializedAt!) - Date.parse(left.runInitializedAt!));
  const stationInputs = rainStationHours(evaluation.captures, evaluation.performance.decisionAt);
  const sourceConfigFingerprint = ADJUSTMENT_RAIN_SOURCE_CONFIG_FINGERPRINT;
  return encodeMaintenanceShadowSourceProjection({
    candidateSha256: registration.candidateSha256,
    contractVersion: MAINTENANCE_SHADOW_SOURCE_VERSION,
    dueKey,
    family: "rain",
    issuedAt,
    causalInputs: {
      captureSet: evaluation.captures.map(
        // retain every exact raw capture identity used to build the input graph
        (capture) => ({
          bodySha256: capture.bodySha256,
          claimId: capture.claimId,
          completedAt: capture.completedAt,
          kind: capture.kind,
          runInitializedAt: capture.runInitializedAt,
          stationId: capture.stationId,
          windowEndExclusive: capture.windowEndExclusive,
          windowStart: capture.windowStart,
        }),
      ),
      contractVersion: "adjustment-shadow-rain-causal-inputs/v1",
      currentRun: projectRainMaintenanceRun(evaluation.current),
      priorRuns: forecastCaptures.slice(1).map(projectRainMaintenanceRun),
      stationHours: stationInputs.map(
        // retain exact derived station rows passed to the compiled feature builder
        (hour) => ({
          hourAt: hour.hourAt,
          precipitationMm64: nullableRainBinary64(hour.precipitationMm),
          receivedAt: hour.receivedAt,
          stationId: hour.stationId,
          temperatureC64: nullableRainBinary64(hour.temperatureC),
        }),
      ),
    },
    registrationSha256: registration.registrationSha256,
    rowCount: evaluation.performance.hours.length,
    rows: evaluation.performance.hours.map(
      // bind each prediction target to its exact raw forecast cell
      (hour, index) => {
        const raw = profile.hours[hour.modelLeadHours - 1];
        // reject model output outside the captured one-based lead geometry
        if (raw === undefined || raw.leadHours !== hour.modelLeadHours ||
            raw.precipitationMm !== hour.rawPrecipitationMm) {
          throw new RangeError("rain maintenance source geometry differs");
        }
        return {
          adapterVersion: "open-meteo-rain-capture/v1",
          contentSha256: evaluation.current.bodySha256,
          contractEpoch: "rain-prospective-capture/v1",
          dataset: "ecmwf_ifs",
          leadHours: index + 1,
          modelLeadHours: hour.modelLeadHours,
          precipitationMm64: encodeMaintenanceBinary64(hour.rawPrecipitationMm),
          providerKey: "open-meteo-single-runs",
          receivedAt: evaluation.current.completedAt,
          referenceAt: evaluation.current.runInitializedAt!,
          revisionCount: 0,
          sourceConfigFingerprint,
          sourceKey: "rain-prospective-forecast",
          sourceSha256: registration.sourceSha256,
          upstreamModel: "ecmwf_ifs",
          validAt: hour.validAt,
        };
      },
    ),
    sourceSha256: registration.sourceSha256,
  });
}

// project one exact decoded 48-hour forecast run and raw content identity
function projectRainMaintenanceRun(
  capture: RainAdjustmentCapture,
): Readonly<Record<string, unknown>> {
  const profile = rainForecastProfile(capture);
  return {
    completedAt: profile.completedAt,
    contentSha256: capture.bodySha256,
    hours: profile.hours.map(
      // encode every forecast covariate consumed by feature construction
      (hour) => ({
        cloudCoverPercent64: nullableRainBinary64(hour.cloudCoverPercent),
        leadHours: hour.leadHours,
        precipitationMm64: nullableRainBinary64(hour.precipitationMm),
        pressureHpa64: nullableRainBinary64(hour.pressureHpa),
        relativeHumidityPercent64: nullableRainBinary64(hour.relativeHumidityPercent),
        temperatureC64: nullableRainBinary64(hour.temperatureC),
        windDirectionDegrees64: nullableRainBinary64(hour.windDirectionDegrees),
        windSpeedMps64: nullableRainBinary64(hour.windSpeedMps),
      }),
    ),
    runInitializedAt: profile.runInitializedAt,
  };
}

// preserve explicit missing rain input values
function nullableRainBinary64(value: number | null): string | null {
  return value === null ? null : encodeMaintenanceBinary64(value);
}

// encode native probabilities and positive amounts without changing serving output
function createRainMaintenanceBody(
  sourceBytes: Uint8Array,
  performance: RainWindPerformanceResult,
): Buffer {
  const identity = createMaintenanceShadowSourceIdentity(sourceBytes);
  const source = parseMaintenanceShadowSourceProjection(sourceBytes);
  return encodeMaintenanceShadowValues({
    candidateSha256: source.candidateSha256,
    contractVersion: MAINTENANCE_SHADOW_VALUES_VERSION,
    dueKey: source.dueKey,
    family: "rain",
    inputSha256: identity.inputSha256,
    issuedAt: source.issuedAt,
    registrationSha256: source.registrationSha256,
    rowCount: performance.hours.length,
    rows: performance.hours.map(
      // retain all three native event heads and the positive amount head
      (hour, index) => {
        const probabilities = hour.occurrenceProbabilities;
        const fallbackCode = hour.applied
          ? "none"
          : hour.reasonCode === "phase_unsupported"
            ? "ineligible"
            : "model_unavailable";
        return {
          atLeast1_0Probability64: encodeMaintenanceBinary64(probabilities?.atLeast1_0 ?? 0),
          atLeast2_5Probability64: encodeMaintenanceBinary64(probabilities?.atLeast2_5 ?? 0),
          candidatePrecipitationMm64: encodeMaintenanceBinary64(
            hour.applied ? hour.correctedPrecipitationMm : Math.min(30, hour.rawPrecipitationMm),
          ),
          fallbackCode,
          leadHours: index + 1,
          occurrenceProbability64: encodeMaintenanceBinary64(probabilities?.atLeast0_1 ?? 0),
          positiveAmountMm64: encodeMaintenanceBinary64(
            hour.positiveAmountMm ?? Math.min(30, hour.rawPrecipitationMm),
          ),
          sourceRowSha256: identity.sourceRowSha256[index]!,
          validAt: hour.validAt,
          wouldApply: hour.applied,
        };
      },
    ),
    sourceReceiptSha256: identity.sourceReceiptSha256,
    sourceSha256: source.sourceSha256,
  }, sourceBytes);
}

// archive the actual compiled incumbent heads evaluated on the same causal input
function createRainMaintenanceComparator(
  sourceBytes: Uint8Array,
  bodyBytes: Uint8Array,
  incumbent: RainWindPerformanceResult,
  authority: MaintenanceShadowServingAuthority,
): Buffer {
  const source = parseMaintenanceShadowSourceProjection(sourceBytes);
  const identity = createMaintenanceShadowSourceIdentity(sourceBytes);
  const policyRaw = authority.authorityKind === "policy_raw";
  if (source.family !== "rain" || (!policyRaw && incumbent.hours.length !== source.rowCount)) {
    throw new RangeError("rain incumbent geometry differs");
  }
  const rows = source.rows.map(
    // retain actual serving heads rather than recomputing them in the controller
    (sourceRow, index) => {
      const hour = incumbent.hours[index];
      if (!policyRaw && (hour === undefined || hour.validAt !== sourceRow.validAt ||
          hour.modelLeadHours !== sourceRow.modelLeadHours)) {
        throw new RangeError("rain incumbent row differs");
      }
      return {
        validAt: String(sourceRow.validAt),
        leadHours: index + 1,
        sourceRowSha256: identity.sourceRowSha256[index]!,
        incumbentPrecipitationMm64: policyRaw
          ? String(sourceRow.precipitationMm64)
          : encodeMaintenanceBinary64(
              hour!.applied ? hour!.correctedPrecipitationMm : hour!.rawPrecipitationMm,
            ),
        occurrenceProbability64: policyRaw || hour!.occurrenceProbabilities === null
          ? null
          : encodeMaintenanceBinary64(hour!.occurrenceProbabilities.atLeast0_1),
        atLeast1_0Probability64: policyRaw || hour!.occurrenceProbabilities === null
          ? null
          : encodeMaintenanceBinary64(hour!.occurrenceProbabilities.atLeast1_0),
        atLeast2_5Probability64: policyRaw || hour!.occurrenceProbabilities === null
          ? null
          : encodeMaintenanceBinary64(hour!.occurrenceProbabilities.atLeast2_5),
        positiveAmountMm64: policyRaw || hour!.positiveAmountMm === null
          ? null
          : encodeMaintenanceBinary64(hour!.positiveAmountMm),
        applied: policyRaw ? false : hour!.applied,
        reasonCode: policyRaw ? "policy_raw" : hour!.reasonCode,
      };
    },
  );
  const comparator: MaintenanceShadowComparator = {
    contractVersion: MAINTENANCE_SHADOW_COMPARATOR_VERSION,
    family: "rain",
    registrationSha256: source.registrationSha256,
    candidateSha256: source.candidateSha256,
    sourceSha256: source.sourceSha256,
    dueKey: source.dueKey,
    issuedAt: source.issuedAt,
    sourceProjectionSha256: createHash("sha256").update(sourceBytes).digest("hex"),
    predictionBodySha256: createHash("sha256").update(bodyBytes).digest("hex"),
    servingAuthority: authority,
    rowCount: rows.length,
    rows,
  };
  return encodeMaintenanceShadowComparator(comparator, sourceBytes, bodyBytes);
}

// derive the fixed six-hour collection scheduler key from the captured cycle
function rainMaintenanceDueKey(current: RainAdjustmentCapture): string {
  // require the selected source to be an exact forecast cycle
  if (current.kind !== "forecast" || current.runInitializedAt === null) {
    throw new RangeError("rain maintenance forecast cycle is missing");
  }
  const initialized = Date.parse(current.runInitializedAt);
  const hour = new Date(initialized).getUTCHours();
  // reject noncanonical provider cycles before creating a scheduler key
  if (!Number.isFinite(initialized) || initialized % HOUR_MS !== 0 || ![0, 6, 12, 18].includes(hour)) {
    throw new RangeError("rain maintenance forecast cycle is invalid");
  }
  return `capture/${new Date(initialized + 35 * MINUTE_MS).toISOString()}`;
}

// require the root-installed action, graph, bundle and registration tuple
function validateRainInstalledCandidate(
  candidate: InstalledMaintenanceShadowCandidate<"rain">,
): void {
  const registration = candidate.registration;
  const receipt = candidate.receipt;
  const pendingReceipt = receipt.contractVersion === "adjustment-installed-candidate-receipt/v1" ||
    (receipt.contractVersion === "adjustment-installed-candidate-receipt/v2" &&
      receipt.actionKind === "shadow" && receipt.policyDecision === "pending" &&
      receipt.fullMemberRootSha256 === null);
  // prevent registration from literal caller hash claims
  if (candidate.family !== "rain" || registration.family !== "rain" ||
      !pendingReceipt ||
      receipt.registrationSha256 !== registration.registrationSha256 ||
      receipt.candidateSha256 !== registration.candidateSha256 ||
      receipt.bundleSha256 !== registration.artifactSha256 || receipt.sourceSha256 !== registration.sourceSha256 ||
      candidate.action.candidateGraphSha256 !== receipt.candidateGraphSha256 ||
      candidate.action.candidateSha256 !== receipt.candidateSha256 ||
      candidate.bundle.contractVersion !== "rain-hurdle-wind-runtime/v1" ||
      !/^[a-f0-9]{64}$/u.test(candidate.catalogSha256)) {
    throw new RangeError("installed rain maintenance candidate differs");
  }
}

// report one best-effort value-free gap through the api relay
async function reportRainMaintenanceGap(
  options: RainAdjustmentMaintenanceOptions,
  dueKey: string,
  reason: RainAdjustmentMaintenanceGapReason,
): Promise<RainAdjustmentMaintenanceResult> {
  try {
    await options.client.recordGap({
      dueKey,
      family: "rain",
      reason,
      registrationSha256: options.candidate.registration.registrationSha256,
    });
  } catch {
    // keep serving independent when the private relay is unavailable
  }
  return { reason, status: "gap" };
}

// post one bounded json request to the internal api
async function postRainMaintenance<T>(
  base: URL,
  path: string,
  value: unknown,
): Promise<T> {
  const body = JSON.stringify(value);
  // keep the full private source projection within the combined-cycle ceiling
  if (Buffer.byteLength(body) > 2 * 1_024 * 1_024) {
    throw new RangeError("rain maintenance request is too large");
  }
  const response = await fetch(new URL(path, base), {
    body,
    headers: { "content-type": "application/json" },
    method: "POST",
  });
  // refuse any transport response that did not complete the private operation
  if (!response.ok) {
    const error = new Error("rain maintenance api failed") as Error & { code?: string };
    // preserve only the private archive's bounded capacity signal for worker retry
    if (response.status === 429) {
      error.code = "adjustment_revision_spool_refused";
    }
    throw error;
  }
  return await response.json() as T;
}
