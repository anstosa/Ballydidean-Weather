import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";

import {
  FORECAST_OBSERVATION_SOURCE_LINEAGES,
  FORECAST_OBSERVATION_STATIONS,
  RAIN_COLLECTION_POLICY,
} from "@weather/domain";
import {
  abandonExpiredRuns,
  acquireSourceSession,
  bindAdjustmentEcmwfTemperatureRevision,
  bindAdjustmentWeatherRevisions,
  completeScheduledIngestion,
  createDatabasePool,
  discoverDueSources,
  failIngestionRun,
  getScheduledCheckpoint,
  listCausalEcmwfTemperatureCanaryPriorHours,
  listCausalForecastObservationHourlyStations,
  listEcmwfTemperatureCanaryRunInitializations,
  listPendingPhysicalWeatherAdjustmentRevisions,
  persistEcmwfTemperatureCanaryRun,
  markAdjustmentRevisionGap,
  startIngestionRun,
  updateWorkerHeartbeat,
  type DueSource,
  type AdjustmentRevisionCommitReceipt,
  type EcmwfTemperatureCanaryRecentErrorState,
  type PersistedEcmwfTemperatureRevision,
  type PersistedWeatherAdjustmentRevision,
  type Queryable,
  type EcowittConfiguration,
  type PublicStationConfiguration,
  type PublicStationConfigurationStation,
  type PublicStationSourceConfiguration,
  type ScheduledCheckpointState,
  type SiteConfiguration,
  type SourceSession,
  type TempestConfiguration,
  type TideConfiguration,
  type TideStationConfiguration,
} from "@weather/database";
import {
  ECOWITT_LOCAL_LIVE_ADAPTER_VERSION,
  NOAA_TIDE_OBSERVATION_ADAPTER_VERSION,
  NOAA_TIDE_PREDICTION_ADAPTER_VERSION,
  OPEN_METEO_CURRENT_ADAPTER_VERSION,
  OPEN_METEO_FORECAST_ADAPTER_VERSION,
  ProviderFailure,
  TEMPEST_OBSERVATION_ADAPTER_VERSION,
  asProviderFailure,
  createOpenMeteoCurrentOperation,
  createOpenMeteoEcmwfSingleRunOperation,
  createOpenMeteoForecastOperation,
  createTempestObservationOperation,
  fetchEcowittLive,
  fetchOpenMeteoEcmwfSingleRun,
  fetchOpenMeteoCurrent,
  fetchOpenMeteoForecast,
  fetchNoaaTideRange,
  fetchPublicStationRange,
  publicStationAdapterVersion,
  type EcowittLiveOperation,
  type OpenMeteoCurrentOperation,
  type OpenMeteoEcmwfSingleRunOperation,
  type OpenMeteoForecastOperation,
  type NoaaTideRangeOperation,
  type NoaaTideRangeRequest,
  type ProviderFetchOptions,
  type PublicStationRangeOperation,
  type PublicStationRangeRequest,
  type TempestObservationOperation,
} from "@weather/providers";
import {
  createForecastAdjustmentRainRuntimeRegistryLoader,
  createForecastAdjustmentTemperatureCanaryRuntimeLoader,
  loadAdjustmentRevisionCaptureEpochWitness,
  forecastAdjustmentTemperatureCanaryIsActiveAt,
  loadInstalledMaintenanceShadowCandidate,
  loadInstalledRainMaintenanceControlReference,
  localCalendarFeaturesFor,
  scalarNetworkActual,
  adjustmentRevisionLogicalKeySha256,
  adjustmentRevisionProjectionIdentity,
  parseAdjustmentRevisionBatchProjection,
  parseAdjustmentRevisionProjection,
  parseAdjustmentTemperatureNativeSourceProjection,
  requireAdjustmentRevisionProjectionAfterCaptureEpoch,
  encodeAdjustmentRevisionProjection,
  encodeAdjustmentTemperatureNativeSourceProjection,
  encodeAdjustmentRevisionBatchProjection,
  encodeMaintenanceBinary64,
  type LoadedForecastAdjustmentTemperatureCanaryRuntime,
  type AdjustmentRevisionCaptureEpochWitness,
  type LoadedForecastAdjustmentRainRuntimeRegistryV1,
  type InstalledRainMaintenanceControlReference,
} from "@weather/forecast-adjustment";

import { loadWorkerConfiguration } from "./config.js";
import {
  boundedWorkerError,
  combineWorkerDiagnostics,
  createWorkerDiagnostic,
  guardReleaseSession,
  writeWorkerDiagnostic,
  type WorkerDiagnostic,
} from "./errors.js";
import {
  assertWorkerDatabaseReadiness,
  readWorkerHealth,
} from "./health.js";
import { planIngestionDeadlines } from "./run-deadline.js";
import { collectRainEvidence, isRainCollectionEnabled, type RainCollectionOptions } from "./rain-collection.js";
import { publishRainFixedGaugeTarget } from "./rain-fixed-gauge-target-producer.js";
import {
  createHttpRainAdjustmentMaintenanceClient,
  publishRainAdjustment,
  stageAdjustmentRevisionWithBackpressure,
  type AdjustmentRevisionGap,
  type AdjustmentRevisionStageReceipt,
  type RainAdjustmentMaintenanceClient,
  type RainAdjustmentMaintenanceOptions,
} from "./rain-adjustment.js";
import {
  executePublicStationBackfill,
  resolvePublicStationBackfillSources,
} from "./public-stations-backfill-cli.js";
import {
  WORKER_CADENCE_MS,
  createNonOverlappingScheduler,
} from "./scheduler.js";
import {
  sourceIdentityMatchesConfiguration,
  sourceIdentityMatchesEcowittConfiguration,
  sourceIdentityMatchesPublicStationConfiguration,
  sourceIdentityMatchesTempestConfiguration,
  sourceIdentityMatchesTideConfiguration,
} from "./source-identity.js";

type DatabasePool = ReturnType<typeof createDatabasePool>;

export interface WorkerRepository {
  readonly abandonExpiredRuns: typeof abandonExpiredRuns;
  readonly acquireSourceSession: typeof acquireSourceSession;
  readonly completeScheduledIngestion: typeof completeScheduledIngestion;
  readonly discoverDueSources: typeof discoverDueSources;
  readonly failIngestionRun: typeof failIngestionRun;
  readonly getScheduledCheckpoint: typeof getScheduledCheckpoint;
  readonly listCausalEcmwfTemperatureCanaryPriorHours: typeof listCausalEcmwfTemperatureCanaryPriorHours;
  readonly listCausalForecastObservationHourlyStations: typeof listCausalForecastObservationHourlyStations;
  readonly listEcmwfTemperatureCanaryRunInitializations: typeof listEcmwfTemperatureCanaryRunInitializations;
  readonly persistEcmwfTemperatureCanaryRun: typeof persistEcmwfTemperatureCanaryRun;
  readonly startIngestionRun: typeof startIngestionRun;
  readonly updateWorkerHeartbeat: typeof updateWorkerHeartbeat;
}

export interface WorkerIterationOptions {
  readonly adjustmentCaptureEpoch?: AdjustmentRevisionCaptureEpochWitness;
  readonly adjustmentMaintenanceCapture?: (
    request: Readonly<{ dueKey: string; issuedAt: string }>,
  ) => Promise<void>;
  readonly diagnosticWriter?: (diagnostic: WorkerDiagnostic) => void;
  readonly fetchCurrent?: OpenMeteoCurrentOperation;
  readonly fetchEcmwfSingleRun?: OpenMeteoEcmwfSingleRunOperation;
  readonly fetchEcowitt?: EcowittLiveOperation;
  readonly fetchForecast?: OpenMeteoForecastOperation;
  readonly fetchTempest?: TempestObservationOperation;
  readonly fetchOptions?: ProviderFetchOptions;
  readonly fetchPublicStation?: PublicStationRangeOperation;
  readonly fetchTide?: NoaaTideRangeOperation;
  readonly instance: string;
  readonly lastSuccessAt: string | null;
  readonly now?: () => Date;
  readonly repository?: WorkerRepository;
  readonly site: SiteConfiguration;
  readonly ecowitt?: EcowittConfiguration | null;
  readonly publicStations?: PublicStationConfiguration | null;
  readonly rainCollection?: RainCollectionOptions;
  readonly rainAdjustmentEnabled?: boolean;
  readonly rainAdjustmentMaintenance?: RainAdjustmentMaintenanceOptions;
  readonly rainAdjustmentControlReference?: InstalledRainMaintenanceControlReference;
  readonly rainAdjustmentRevisionClient?: RainAdjustmentMaintenanceClient;
  readonly rainAdjustmentRuntime?: LoadedForecastAdjustmentRainRuntimeRegistryV1;
  readonly tempest?: TempestConfiguration | null;
  readonly temperatureCanaryRuntime?: LoadedForecastAdjustmentTemperatureCanaryRuntime;
  readonly tides?: TideConfiguration | null;
  readonly version: string;
}

export interface SourceRunResult {
  readonly durationMs: number;
  readonly recordCount: number;
  readonly reason: string | null;
  readonly runId: string | null;
  readonly secondaryError: string | null;
  readonly sourceId: string;
  readonly status: "failed" | "skipped" | "succeeded";
}

export interface WorkerIterationResult {
  readonly completedAt: string;
  readonly lastSuccessAt: string | null;
  readonly sources: readonly SourceRunResult[];
}

interface WorkerSuccessState {
  lastSuccessAt: string | null;
}

const databaseRepository: WorkerRepository = {
  abandonExpiredRuns,
  acquireSourceSession,
  completeScheduledIngestion,
  discoverDueSources,
  failIngestionRun,
  getScheduledCheckpoint,
  listCausalEcmwfTemperatureCanaryPriorHours,
  listCausalForecastObservationHourlyStations,
  listEcmwfTemperatureCanaryRunInitializations,
  persistEcmwfTemperatureCanaryRun,
  startIngestionRun,
  updateWorkerHeartbeat,
};

// create one runner with retained success state
export function createWorkerIterationRunner(
  pool: DatabasePool,
  options: WorkerIterationOptions,
): () => Promise<WorkerIterationResult> {
  const successState: WorkerSuccessState = {
    lastSuccessAt: options.lastSuccessAt,
  };

  // preserve committed success across iteration failures
  return async () => await runWorkerIterationWithState(pool, options, successState);
}

// execute one standalone worker iteration
export async function runWorkerIteration(
  pool: DatabasePool,
  options: WorkerIterationOptions,
): Promise<WorkerIterationResult> {
  return await runWorkerIterationWithState(pool, options, {
    lastSuccessAt: options.lastSuccessAt,
  });
}

// execute one iteration against retained success state
async function runWorkerIterationWithState(
  pool: DatabasePool,
  options: WorkerIterationOptions,
  successState: WorkerSuccessState,
): Promise<WorkerIterationResult> {
  const repository = options.repository ?? databaseRepository;
  const now = options.now ?? defaultNow;
  const diagnosticWriter = options.diagnosticWriter ?? writeWorkerDiagnostic;
  const iterationStartedAt = now().getTime();
  const loopAt = now().toISOString();
  const dueSources = await repository.discoverDueSources(pool, loopAt);
  const results: SourceRunResult[] = [];

  // isolate every due source
  for (const source of dueSources) {
    try {
      const result = await runScheduledSource(pool, source, {
        ...(options.fetchOptions === undefined
          ? {}
          : { fetchOptions: options.fetchOptions }),
        now,
        ...(options.rainAdjustmentRevisionClient === undefined
          ? {}
          : { revisionClient: options.rainAdjustmentRevisionClient }),
        ...(options.adjustmentCaptureEpoch === undefined
          ? {}
          : { captureEpoch: options.adjustmentCaptureEpoch }),
        repository,
        site: options.site,
        ...(options.fetchCurrent === undefined
          ? {}
          : { fetchCurrent: options.fetchCurrent }),
        ...(options.fetchEcowitt === undefined
          ? {}
          : { fetchEcowitt: options.fetchEcowitt }),
        ...(options.fetchForecast === undefined
          ? {}
          : { fetchForecast: options.fetchForecast }),
        ...(options.fetchTempest === undefined
          ? {}
          : { fetchTempest: options.fetchTempest }),
        ...(options.fetchPublicStation === undefined
          ? {}
          : { fetchPublicStation: options.fetchPublicStation }),
        ...(options.fetchTide === undefined
          ? {}
          : { fetchTide: options.fetchTide }),
        publicStations: options.publicStations ?? null,
        ecowitt: options.ecowitt ?? null,
        tempest: options.tempest ?? null,
        tides: options.tides ?? null,
      });
      results.push(result);
      diagnosticWriter(
        createWorkerDiagnostic({
          count: result.recordCount,
          durationMs: result.durationMs,
          errorCode: result.status === "failed" ? result.reason : null,
          event: "source_run",
          release: options.version,
          runId: result.runId,
          sourceId: result.sourceId,
        }),
      );

      // track ingestion success separately from loop liveness
      if (result.status === "succeeded") {
        successState.lastSuccessAt = now().toISOString();
      }
    } catch (error) {
      const result: SourceRunResult = {
        durationMs: elapsedMilliseconds(iterationStartedAt, now()),
        reason: "worker_source_failure",
        recordCount: 0,
        runId: null,
        secondaryError: boundedWorkerError(error),
        sourceId: source.id,
        status: "failed",
      };
      results.push(result);
      diagnosticWriter(
        createWorkerDiagnostic({
          count: 0,
          durationMs: result.durationMs,
          errorCode: result.reason,
          event: "source_run",
          release: options.version,
          runId: null,
          sourceId: source.id,
        }),
      );
    }
  }

  // collect only while the independently authorized canary is active
  if (temperatureCanaryCollectionIsActive(options.temperatureCanaryRuntime, loopAt)) {
    const collectionStartedAt = now().getTime();

    try {
      const collection = await collectEcmwfTemperatureCanaryRuns(pool, {
        ...(options.fetchOptions === undefined
          ? {}
          : { fetchOptions: options.fetchOptions }),
        ...(options.fetchEcmwfSingleRun === undefined
          ? {}
          : { fetchEcmwfSingleRun: options.fetchEcmwfSingleRun }),
        now,
        ...(options.rainAdjustmentRevisionClient === undefined
          ? {}
          : { revisionClient: options.rainAdjustmentRevisionClient }),
        ...(options.adjustmentCaptureEpoch === undefined
          ? {}
          : { captureEpoch: options.adjustmentCaptureEpoch }),
        repository,
        site: options.site,
      });
      diagnosticWriter(
        createWorkerDiagnostic({
          count: collection.persistedRuns,
          durationMs: elapsedMilliseconds(collectionStartedAt, now()),
          errorCode:
            collection.failedRuns === 0
              ? null
              : "temperature_canary_collection_failed",
          event: "source_run",
          release: options.version,
          runId: null,
          sourceId: "ecmwf-temperature-canary",
        }),
      );
    } catch {
      // isolate the optional sidecar from normal ingestion and heartbeat
      diagnosticWriter(
        createWorkerDiagnostic({
          count: 0,
          durationMs: elapsedMilliseconds(collectionStartedAt, now()),
          errorCode: "temperature_canary_collection_failed",
          event: "source_run",
          release: options.version,
          runId: null,
          sourceId: "ecmwf-temperature-canary",
        }),
      );
    }
  }

  // isolate the separately authorized evidence collector from serving and canaries
  if (options.rainCollection !== undefined) {
    const startedAt = now().getTime();

    try {
      const result = await collectRainEvidence(pool, options.version, {
        ...options.rainCollection,
        now,
      });
      diagnosticWriter(createWorkerDiagnostic({
        count: result.valid,
        durationMs: elapsedMilliseconds(startedAt, now()),
        errorCode: result.failed === 0 ? null : "rain_collection_failed",
        event: "source_run",
        release: options.version,
        runId: null,
        sourceId: "rain-prospective-capture",
      }));
    } catch {
      // contain storage and transport failures without claiming collection success
      diagnosticWriter(createWorkerDiagnostic({
        count: 0,
        durationMs: elapsedMilliseconds(startedAt, now()),
        errorCode: "rain_collection_failed",
        event: "source_run",
        release: options.version,
        runId: null,
        sourceId: "rain-prospective-capture",
      }));
    }
  }

  // materialize one post-epoch closed target hour after raw collection completes
  if (options.rainCollection !== undefined &&
      options.rainAdjustmentRevisionClient !== undefined &&
      options.adjustmentCaptureEpoch !== undefined) {
    const startedAt = now().getTime();
    try {
      const result = await publishRainFixedGaugeTarget(
        pool,
        options.adjustmentCaptureEpoch,
        options.rainAdjustmentRevisionClient,
      );
      diagnosticWriter(createWorkerDiagnostic({
        count: result.state === "published" ? result.revisionCount : 0,
        durationMs: elapsedMilliseconds(startedAt, now()),
        errorCode: result.state === "gap" ? result.reason : null,
        event: "source_run",
        release: options.version,
        runId: null,
        sourceId: "rain-fixed-gauge-target",
      }));
    } catch {
      // preserve ordinary serving when target custody or storage is unavailable
      diagnosticWriter(createWorkerDiagnostic({
        count: 0,
        durationMs: elapsedMilliseconds(startedAt, now()),
        errorCode: "rain_fixed_gauge_target_failed",
        event: "source_run",
        release: options.version,
        runId: null,
        sourceId: "rain-fixed-gauge-target",
      }));
    }
  }

  // publish model output independently of collection and other adjustment failures
  if (rainAdjustmentInferenceIsActive(
    options.rainAdjustmentRuntime,
    options.rainAdjustmentEnabled,
  )) {
    const startedAt = now().getTime();
    try {
      const published = await publishRainAdjustment(
        pool,
        now(),
        options.rainAdjustmentRuntime,
        options.rainAdjustmentMaintenance,
        options.rainAdjustmentRevisionClient,
        options.adjustmentCaptureEpoch,
        options.rainAdjustmentControlReference,
      );
      diagnosticWriter(createWorkerDiagnostic({
        count: published ? 1 : 0, durationMs: elapsedMilliseconds(startedAt, now()),
        errorCode: null, event: "source_run", release: options.version,
        runId: null, sourceId: "rain-adjustment",
      }));
    } catch {
      // leave raw fallback available when source aggregation or inference fails
      diagnosticWriter(createWorkerDiagnostic({
        count: 0, durationMs: elapsedMilliseconds(startedAt, now()),
        errorCode: "rain_adjustment_failed", event: "source_run", release: options.version,
        runId: null, sourceId: "rain-adjustment",
      }));
    }
  }

  // trigger the stable private capture key after all retained producers have run
  if (options.adjustmentMaintenanceCapture !== undefined) {
    const captureStartedAt = now().getTime();
    try {
      const capture = adjustmentMaintenanceCaptureDueAt(new Date(loopAt));
      await options.adjustmentMaintenanceCapture(capture);
    } catch {
      // keep public ingestion and serving available when graph capture is blocked
      diagnosticWriter(createWorkerDiagnostic({
        count: 0,
        durationMs: elapsedMilliseconds(captureStartedAt, now()),
        errorCode: "adjustment_maintenance_capture_failed",
        event: "source_run",
        release: options.version,
        runId: null,
        sourceId: "adjustment-maintenance-capture",
      }));
    }
  }

  const completedAt = now().toISOString();
  await repository.updateWorkerHeartbeat(pool, {
    activity: results.some((result) => result.status === "failed")
      ? "degraded"
      : null,
    instance: options.instance,
    lastLoopAt: completedAt,
    lastSuccessAt: successState.lastSuccessAt,
    version: options.version,
  });
  diagnosticWriter(
    createWorkerDiagnostic({
      count: results.length,
      durationMs: elapsedMilliseconds(iterationStartedAt, now()),
      errorCode: results.some((result) => result.status === "failed")
        ? "worker_iteration_degraded"
        : null,
      event: "worker_iteration",
      release: options.version,
      runId: null,
      sourceId: null,
    }),
  );

  return {
    completedAt,
    lastSuccessAt: successState.lastSuccessAt,
    sources: results,
  };
}

// preserve legacy injected enablement unless a registry selection exists
function rainAdjustmentInferenceIsActive(
  runtime: LoadedForecastAdjustmentRainRuntimeRegistryV1 | undefined,
  legacyEnabled: boolean | undefined,
): boolean {
  const rootSelectedRaw = runtime?.state === "disabled" && runtime.reasonCode === "policy_raw" &&
    runtime.comparatorAuthority !== undefined;
  // preserve raw service while retaining its genuine future-only capture path
  return legacyEnabled === true &&
    (runtime === undefined || runtime.state === "active" || rootSelectedRaw);
}

// derive the latest fixed six-hour capture clock without using mutable loop jitter
export function adjustmentMaintenanceCaptureDueAt(
  now: Date,
): Readonly<{ dueKey: string; issuedAt: string }> {
  const nowMilliseconds = now.getTime();
  // reject invalid injected scheduler clocks
  if (!Number.isFinite(nowMilliseconds)) {
    throw new RangeError("adjustment maintenance scheduler clock is invalid");
  }
  const cycleHours = Math.floor(now.getUTCHours() / 6) * 6;
  const cycle = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate(), cycleHours);
  let issuedAtMilliseconds = cycle + 35 * 60_000;
  // use the previous exact cycle before the current cycle reaches its due minute
  if (issuedAtMilliseconds > nowMilliseconds) {
    issuedAtMilliseconds -= 6 * 3_600_000;
  }
  const issuedAt = new Date(issuedAtMilliseconds).toISOString();
  return { dueKey: `capture/${issuedAt}`, issuedAt };
}

export interface EcmwfTemperatureCanaryCollectionResult {
  readonly failedRuns: number;
  readonly persistedRuns: number;
  readonly requestedRuns: readonly string[];
}

// collect at most two missing runs with latest-first warmup
export async function collectEcmwfTemperatureCanaryRuns(
  pool: DatabasePool,
  options: Readonly<{
    fetchEcmwfSingleRun?: OpenMeteoEcmwfSingleRunOperation;
    fetchOptions?: ProviderFetchOptions;
    now: () => Date;
    repository: WorkerRepository;
    revisionClient?: RainAdjustmentMaintenanceClient;
    captureEpoch?: AdjustmentRevisionCaptureEpochWitness;
    site: SiteConfiguration;
  }>,
): Promise<EcmwfTemperatureCanaryCollectionResult> {
  const now = options.now();
  const latestInitialization = latestAvailableEcmwfInitialization(now);
  const oldestInitialization = new Date(
    Date.parse(latestInitialization) - 72 * 3_600_000,
  ).toISOString();
  const collected = new Set(
    await options.repository.listEcmwfTemperatureCanaryRunInitializations(
      pool,
      {
        from: oldestInitialization,
        siteSlug: options.site.site.key,
        to: latestInitialization,
      },
    ),
  );
  const missing: string[] = [];

  // prioritize the current run before older warmup runs
  for (let offsetHours = 0; offsetHours <= 72; offsetHours += 6) {
    const runInitializedAt = new Date(
      Date.parse(latestInitialization) - offsetHours * 3_600_000,
    ).toISOString();

    // skip exact initializations already persisted
    if (!collected.has(runInitializedAt)) {
      missing.push(runInitializedAt);
    }
  }

  const requestedRuns = missing.slice(0, 2);
  let failedRuns = 0;
  let persistedRuns = 0;

  // isolate each bounded provider request
  for (const runInitializedAt of requestedRuns) {
    try {
      const fetchOptions = boundedTemperatureCanaryFetchOptions(
        options.fetchOptions,
        options.now,
      );
      const batch = await (
        options.fetchEcmwfSingleRun ?? fetchOpenMeteoEcmwfSingleRun
      )(
        {
          latitude: options.site.site.latitude,
          longitude: options.site.site.longitude,
          runInitializedAt,
        },
        fetchOptions,
      );
      const state = runInitializedAt === latestInitialization
        ? await buildCausalTemperatureRecentErrorState(pool, {
            repository: options.repository,
            siteSlug: options.site.site.key,
            targetRunInitializedAt: runInitializedAt,
          })
        : emptyTemperatureRecentErrorState(runInitializedAt);
      const stateStatus = state.supported
        ? "supported"
        : state.n72 === 0
          ? "cold"
          : "insufficient";
      await options.repository.persistEcmwfTemperatureCanaryRun(pool, {
        adapterVersion: batch.adapterVersion,
        hours: batch.hours,
        modelCycle: batch.modelCycle,
        providerResponseSha256: batch.providerResponseSha256,
        receivedAt: batch.receivedAt,
        recentErrorState: state,
        runInitializedAt: batch.runInitializedAt,
        siteSlug: options.site.site.key,
        stateReason:
          stateStatus === "supported"
            ? "causal_recent_error_state_supported"
            : stateStatus === "cold"
              ? "no_causal_forecast_observation_pairs"
              : "causal_recent_error_state_insufficient",
        stateStatus,
        upstreamModel: batch.upstreamModel,
      }, options.revisionClient === undefined
        ? undefined
        : options.captureEpoch === undefined
          ? undefined
          : createEcmwfTemperatureRevisionArchiver(pool, options.revisionClient, options.captureEpoch));
      persistedRuns += 1;
    } catch {
      // continue warming after one unavailable or rejected run
      failedRuns += 1;
    }
  }

  return { failedRuns, persistedRuns, requestedRuns };
}

// create one transactional ECMWF archive adapter for a newly persisted run
export function createEcmwfTemperatureRevisionArchiver(
  pool: DatabasePool,
  client: RainAdjustmentMaintenanceClient,
  captureEpoch: AdjustmentRevisionCaptureEpochWitness,
  pause?: (milliseconds: number) => Promise<void>,
) {
  return async (
    queryable: Queryable,
    revision: PersistedEcmwfTemperatureRevision,
  ): Promise<Readonly<{ publish: () => Promise<void> }>> => {
    const logicalKey = {
      contentSha256: revision.contentHash,
      leadHours: null,
      providerResponseSha256: revision.providerResponseSha256,
      runInitializedAt: revision.runInitializedAt,
      siteId: revision.siteId,
      sourceId: null,
      sourceType: "ecmwf_temperature_run",
      validAt: null,
    };
    const logicalKeySha256 = adjustmentRevisionLogicalKeySha256("native_source", logicalKey);
    const databaseKey = {
      providerResponseSha256: revision.providerResponseSha256,
      runInitializedAt: revision.runInitializedAt,
      siteId: revision.siteId,
      storedContentSha256: revision.contentHash,
    };
    let projection: Buffer;
    let identity: string;
    // encode the complete eighteen-hour run returned by the database transaction
    try {
      projection = encodeAdjustmentTemperatureNativeSourceProjection({
        contractVersion: "adjustment-temperature-native-source-projection/v2",
        family: "temperature",
        logicalKey,
        logicalReceivedAt: revision.logicalReceivedAt,
        projectionKind: "native_source",
        recentErrorState: revision.recentErrorState,
        recentErrorStateSha256: revision.recentErrorStateSha256,
        rows: temperatureNativeRevisionRows(revision),
        source: {
          adapterVersion: revision.adapterVersion,
          contractEpoch: "ecmwf-temperature-canary/v1",
          dataset: "single_run",
          providerKey: "open-meteo",
          sourceConfigFingerprint: ecmwfTemperatureRevisionSourceFingerprint(revision),
          sourceId: `site:${revision.siteId}`,
          sourceKey: `open-meteo-ecmwf-single-run:${revision.siteSlug}`,
          sourceKind: "forecast",
          upstreamModel: revision.upstreamModel,
        },
        storedContentSha256: revision.contentHash,
      });
      requireAdjustmentRevisionProjectionAfterCaptureEpoch(
        captureEpoch,
        parseAdjustmentTemperatureNativeSourceProjection(projection),
      );
      identity = adjustmentRevisionProjectionIdentity(projection);
    } catch {
      const gap = ecmwfTemperatureRevisionGap(logicalKeySha256, null, "archive_stage_failed");
      await markAdjustmentRevisionGap(queryable, "ecmwf_temperature", databaseKey, gap);
      return { publish: () => recordWorkerRevisionGap(client, gap) };
    }
    let stageReceipt: AdjustmentRevisionStageReceipt;
    // require archive durability before assigning the database frontier ordinal
    try {
      stageReceipt = await stageAdjustmentRevisionWithBackpressure(client, projection, pause);
      validateWorkerRevisionStageReceipt(stageReceipt, "native_source", identity);
    } catch {
      const gap = ecmwfTemperatureRevisionGap(logicalKeySha256, identity, "archive_stage_failed");
      await markAdjustmentRevisionGap(queryable, "ecmwf_temperature", databaseKey, gap);
      return { publish: () => recordWorkerRevisionGap(client, gap) };
    }
    let revisionReceipt: AdjustmentRevisionCommitReceipt;
    // bind the durable complete run inside the same serving transaction
    await queryable.query("SAVEPOINT ecmwf_temperature_revision_bind");
    try {
      revisionReceipt = (await bindAdjustmentEcmwfTemperatureRevision(queryable, {
        ...databaseKey,
        projectionIdentitySha256: identity,
        projectionSha256: identity,
        stageReceiptSha256: stageReceipt.stageReceiptSha256,
      })).revisionReceipt;
      await queryable.query("RELEASE SAVEPOINT ecmwf_temperature_revision_bind");
    } catch {
      // recover SQL failure before retaining a permanent gap for the staged body
      await queryable.query("ROLLBACK TO SAVEPOINT ecmwf_temperature_revision_bind");
      await queryable.query("RELEASE SAVEPOINT ecmwf_temperature_revision_bind");
      const gap = ecmwfTemperatureRevisionGap(logicalKeySha256, identity, "database_bind_failed");
      await markAdjustmentRevisionGap(queryable, "ecmwf_temperature", databaseKey, gap);
      return { publish: () => recordWorkerRevisionGap(client, gap) };
    }
    return {
      // publish only after the temperature serving transaction commits
      async publish() {
        try {
          await client.publishRevision(projection, stageReceipt, revisionReceipt);
        } catch (error) {
          const gap = ecmwfTemperatureRevisionGap(logicalKeySha256, identity, "archive_publish_failed");
          let marked = false;
          // preserve a stricter api-side admission marker if one already exists
          try {
            await markAdjustmentRevisionGap(pool, "ecmwf_temperature", databaseKey, gap);
            marked = true;
          } catch {
            // the api may already have persisted a different permanent category
          }
          // archive only the matching category this caller persisted
          if (marked) {
            await recordWorkerRevisionGap(client, gap);
          }
          throw error;
        }
      },
    };
  };
}

// bind the full native run to the exact serving best-match product
function temperatureNativeRevisionRows(
  revision: PersistedEcmwfTemperatureRevision,
): readonly Readonly<Record<string, string | number | boolean | null>>[] {
  const bestMatchByValidAt = new Map(revision.bestMatchRows.map(
    // retain the exact database valid-time identity
    (row) => [new Date(row.validAt).toISOString(), row],
  ));
  // require one complete source-decision serving horizon before encoding
  if (revision.hours.length !== 18 || revision.bestMatchRows.length !== 12 ||
      new Set(revision.bestMatchRows.map((row) => row.sourceId)).size !== 1 ||
      new Set(revision.bestMatchRows.map((row) => String(row.productRunAt))).size !== 1) {
    throw new RangeError("temperature native comparator geometry differs");
  }
  return revision.hours.map(
    // retain every native hour and only its eligible serving comparator
    (hour) => {
      const bestMatch = hour.modelLeadHours < 7
        ? null
        : bestMatchByValidAt.get(hour.validAt) ?? null;
      // prohibit a missing or non-best-match eligible source row
      if (hour.modelLeadHours >= 7 && (bestMatch === null ||
          bestMatch.productRunAt === null || bestMatch.upstreamModel !== "best_match" ||
          bestMatch.providerMetadata?.dataset !== "best_match")) {
        throw new RangeError("temperature native comparator differs");
      }
      return {
        bestMatchContentSha256: bestMatch?.contentHash ?? null,
        bestMatchProductRunAt: bestMatch === null
          ? null : new Date(bestMatch.productRunAt!).toISOString(),
        bestMatchSourceId: bestMatch?.sourceId ?? null,
        bestMatchTemperatureC64: nullableRevisionBinary64(bestMatch?.temperatureC ?? null),
        contentSha256: hour.contentHash,
        modelCycle: revision.modelCycle,
        modelLeadHours: hour.modelLeadHours,
        rawRelativeHumidityPercent64: nullableRevisionBinary64(hour.rawRelativeHumidityPercent),
        rawTemperatureC64: encodeMaintenanceBinary64(hour.rawTemperatureC),
        rawWindSpeedMps64: nullableRevisionBinary64(hour.rawWindSpeedMps),
        validAt: hour.validAt,
      };
    },
  );
}

// hash the exact fixed provider request configuration for this site
function ecmwfTemperatureRevisionSourceFingerprint(
  revision: PersistedEcmwfTemperatureRevision,
): string {
  return createHash("sha256").update([
    revision.adapterVersion,
    revision.siteSlug,
    revision.siteLatitude.toFixed(6),
    revision.siteLongitude.toFixed(6),
    revision.upstreamModel,
    "forecast_hours=19",
    "temperature_2m,relative_humidity_2m,wind_speed_10m",
  ].join("\n") + "\n").digest("hex");
}

// create one closed permanent temperature revision gap
function ecmwfTemperatureRevisionGap(
  logicalKeySha256: string,
  identity: string | null,
  reason: AdjustmentRevisionGap["reason"],
): AdjustmentRevisionGap {
  return {
    logicalKeySha256,
    projectionIdentitySha256: identity,
    projectionKind: "native_source",
    projectionSha256: identity,
    reason,
  };
}

// relay one already-persisted revision gap without affecting serving
async function recordWorkerRevisionGap(
  client: RainAdjustmentMaintenanceClient,
  gap: AdjustmentRevisionGap,
): Promise<void> {
  try {
    await client.recordRevisionGap(gap);
  } catch {
    // keep the database marker authoritative during an archive outage
  }
}

// validate one whole-body durable stage receipt from the private api
function validateWorkerRevisionStageReceipt(
  receipt: AdjustmentRevisionStageReceipt,
  projectionKind: AdjustmentRevisionStageReceipt["projectionKind"],
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
      receipt.projectionKind !== projectionKind || receipt.projectionIdentitySha256 !== identity ||
      receipt.projectionSha256 !== identity || !Number.isFinite(Date.parse(receipt.durableAt)) ||
      new Date(receipt.durableAt).toISOString() !== receipt.durableAt ||
      receipt.stageReceiptSha256 !== createHash("sha256")
        .update(`${JSON.stringify(unsigned)}\n`).digest("hex")) {
    throw new RangeError("adjustment revision stage receipt differs");
  }
}

// retain null while encoding one exact finite provider value
function nullableRevisionBinary64(value: number | null): string | null {
  return value === null ? null : encodeMaintenanceBinary64(value);
}

// create one transactional target/comparator archive adapter for normalized ingestion
export function createWeatherAdjustmentRevisionArchiver(
  pool: DatabasePool,
  client: RainAdjustmentMaintenanceClient,
  captureEpoch: AdjustmentRevisionCaptureEpochWitness,
  pause?: (milliseconds: number) => Promise<void>,
) {
  return async (
    queryable: Queryable,
    changedRevisions: readonly PersistedWeatherAdjustmentRevision[],
  ): Promise<Readonly<{ publish: () => Promise<void> }> | null> => {
    const selected = await selectWeatherAdjustmentRevisionGroup(
      queryable,
      changedRevisions,
      captureEpoch,
    );
    // omit irrelevant or not-yet-complete cohorts without fabricating a gap
    if (selected === null) {
      return null;
    }
    const { projectionKind, revisions } = selected;
    const first = revisions[0]!;
    const record = first.record;
    const dataset = record.metadata.provider?.dataset;
    const logicalKey = {
      productRunAt: record.productRunAt,
      sourceId: record.sourceId,
      sourceKind: record.sourceKind as "forecast" | "physical_sensor",
      validAt: record.validAt,
    };
    let projection: Buffer;
    let identity: string;
    // encode the complete same-source group retained by the serving transaction
    try {
      projection = encodeAdjustmentRevisionBatchProjection({
        contractVersion: "adjustment-revision-batch-projection/v2",
        family: projectionKind === "actual_best_match" ? "wind" : "shared",
        logicalKey,
        logicalReceivedAt: first.logicalReceivedAt,
        projectionKind,
        rows: revisions.map(weatherAdjustmentRevisionRow),
        source: {
          adapterVersion: first.adapterVersion,
          contractEpoch: weatherAdjustmentRevisionContractEpoch(first, projectionKind),
          dataset: projectionKind === "actual_best_match" ? String(dataset) : first.sourceKey,
          providerKey: first.providerKey,
          sourceConfigFingerprint: first.sourceConfigFingerprint,
          sourceId: record.sourceId,
          sourceKey: first.sourceKey,
          sourceKind: record.sourceKind,
          upstreamModel: projectionKind === "actual_best_match"
            ? record.metadata.model
            : null,
        },
        storedContentSha256: first.contentHash,
      });
      requireAdjustmentRevisionProjectionAfterCaptureEpoch(
        captureEpoch,
        parseAdjustmentRevisionBatchProjection(projection),
      );
      identity = adjustmentRevisionProjectionIdentity(projection);
    } catch {
      const gaps = await markWeatherAdjustmentRevisionGaps(
        queryable,
        revisions,
        projectionKind,
        null,
        "archive_stage_failed",
      );
      return { publish: () => recordWorkerRevisionGaps(client, gaps) };
    }
    let stageReceipt: AdjustmentRevisionStageReceipt;
    // require durable canonical bytes before binding every grouped serving row
    try {
      stageReceipt = await stageAdjustmentRevisionWithBackpressure(client, projection, pause);
      validateWorkerRevisionStageReceipt(stageReceipt, projectionKind, identity);
    } catch {
      const gaps = await markWeatherAdjustmentRevisionGaps(
        queryable,
        revisions,
        projectionKind,
        identity,
        "archive_stage_failed",
      );
      return { publish: () => recordWorkerRevisionGaps(client, gaps) };
    }
    let revisionReceipts: readonly AdjustmentRevisionCommitReceipt[];
    // bind the one durable body to every exact grouped serving revision
    await queryable.query("SAVEPOINT weather_revision_bind");
    try {
      const batch = await bindAdjustmentWeatherRevisions(queryable, revisions.map((revision) => ({
        productRunAt: revision.record.productRunAt,
        projectionIdentitySha256: identity,
        projectionSha256: identity,
        sourceId: revision.record.sourceId,
        sourceKind: revision.record.sourceKind as "forecast" | "physical_sensor",
        stageReceiptSha256: stageReceipt.stageReceiptSha256,
        storedContentSha256: revision.contentHash,
        validAt: revision.record.validAt,
      })));
      // require the database batch to preserve every exact ordered content identity
      if (batch.receipts.length !== revisions.length || batch.receipts.some((entry, index) =>
        entry.storedContentSha256 !== revisions[index]?.contentHash ||
        entry.validAt !== revisions[index]?.record.validAt)) {
        throw new Error("weather revision receipt batch differs");
      }
      revisionReceipts = batch.receipts.map(
        // retain the server-assigned global-frontier receipts unchanged
        (entry) => entry.revisionReceipt as unknown as AdjustmentRevisionCommitReceipt,
      );
      await queryable.query("RELEASE SAVEPOINT weather_revision_bind");
    } catch {
      // remove partial pointers and frontier ordinals before marking every grouped gap
      await queryable.query("ROLLBACK TO SAVEPOINT weather_revision_bind");
      await queryable.query("RELEASE SAVEPOINT weather_revision_bind");
      const gaps = await markWeatherAdjustmentRevisionGaps(
        queryable,
        revisions,
        projectionKind,
        identity,
        "database_bind_failed",
      );
      return { publish: () => recordWorkerRevisionGaps(client, gaps) };
    }
    return {
      // publish only after completeScheduledIngestion commits the serving transaction
      async publish() {
        try {
          await client.publishRevisionBatch(projection, stageReceipt, revisionReceipts);
        } catch (error) {
          const gaps = revisions.map(
            // retain every row-specific logical key in its permanent marker
            (revision) => weatherAdjustmentRevisionGap(
              adjustmentRevisionLogicalKeySha256(projectionKind, {
                productRunAt: revision.record.productRunAt,
                sourceId: revision.record.sourceId,
                sourceKind: revision.record.sourceKind,
                validAt: revision.record.validAt,
              }),
              projectionKind,
              identity,
              "archive_publish_failed",
            ),
          );
          // preserve stricter api-side markers while completing every matching row marker
          for (const [index, gap] of gaps.entries()) {
            const revision = revisions[index]!;
            try {
              await markAdjustmentRevisionGap(pool, "weather_record", {
                productRunAt: revision.record.productRunAt,
                sourceId: revision.record.sourceId,
                sourceKind: revision.record.sourceKind as "forecast" | "physical_sensor",
                storedContentSha256: revision.contentHash,
                validAt: revision.record.validAt,
              }, gap);
            } catch {
              // the api may already have persisted a different permanent category
            }
          }
          try {
            // archive one terminal body disposition while retaining every row marker
            await client.recordRevisionGap(gaps[0]!);
          } catch {
            // the api may already have archived the grouped terminal disposition
          }
          throw error;
        }
      },
    };
  };
}

// select one bounded future-only comparator run or accumulated target group
async function selectWeatherAdjustmentRevisionGroup(
  queryable: Queryable,
  changed: readonly PersistedWeatherAdjustmentRevision[],
  captureEpoch: AdjustmentRevisionCaptureEpochWitness,
): Promise<Readonly<{
  projectionKind: "actual_best_match" | "target_revision";
  revisions: readonly PersistedWeatherAdjustmentRevision[];
}> | null> {
  const first = changed[0];
  // omit empty or mixed caller batches before any archive I/O
  if (first === undefined || changed.some((revision) =>
    revision.record.sourceId !== first.record.sourceId ||
    revision.record.sourceKind !== first.record.sourceKind)) {
    return null;
  }
  const dataset = first.record.metadata.provider?.dataset;
  // capture only four complete 168-hour Best Match runs per UTC day
  if (first.record.sourceKind === "forecast" && first.sourceKey === "open-meteo-forecast-v4" &&
      first.record.productRunAt !== null && dataset === "best_match" &&
      first.record.metadata.model === "best_match") {
    const runAt = Date.parse(first.record.productRunAt);
    // align comparator custody to the same four daily causal evaluation cycles
    if (new Date(runAt).getUTCHours() % 6 !== 0) {
      return null;
    }
    const revisions = changed.filter((revision) =>
      revision.record.productRunAt === first.record.productRunAt &&
      Date.parse(revision.record.validAt) > runAt &&
      Date.parse(revision.record.validAt) <= runAt + 168 * 3_600_000)
      .sort((left, right) => Date.parse(left.record.validAt) - Date.parse(right.record.validAt));
    // refuse a partial run rather than pretending it is a complete wind source
    return revisions.length === 168
      ? { projectionKind: "actual_best_match", revisions }
      : null;
  }
  const lineage = FORECAST_OBSERVATION_SOURCE_LINEAGES.find(
    // admit only exact source identities consumed by adjustment training
    (candidate) => candidate.sourceKey === first.sourceKey &&
      candidate.checkedFingerprint === first.sourceConfigFingerprint,
  );
  // exclude unrelated physical products and superseded training lineages
  if (first.record.sourceKind !== "physical_sensor" || first.record.productRunAt !== null ||
      lineage === undefined) {
    return null;
  }
  const revisions = await listPendingPhysicalWeatherAdjustmentRevisions(
    queryable,
    first.record.sourceId,
    captureEpoch.epochAt,
  );
  return revisions.length === 0 ? null : { projectionKind: "target_revision", revisions };
}

// persist every row-specific permanent marker for one grouped body failure
async function markWeatherAdjustmentRevisionGaps(
  queryable: Queryable,
  revisions: readonly PersistedWeatherAdjustmentRevision[],
  projectionKind: "actual_best_match" | "target_revision",
  identity: string | null,
  reason: AdjustmentRevisionGap["reason"],
): Promise<readonly AdjustmentRevisionGap[]> {
  const gaps: AdjustmentRevisionGap[] = [];
  // bind each permanent category to its exact current serving revision
  for (const revision of revisions) {
    const logicalKey = {
      productRunAt: revision.record.productRunAt,
      sourceId: revision.record.sourceId,
      sourceKind: revision.record.sourceKind,
      validAt: revision.record.validAt,
    };
    const gap = weatherAdjustmentRevisionGap(
      adjustmentRevisionLogicalKeySha256(projectionKind, logicalKey),
      projectionKind,
      identity,
      reason,
    );
    await markAdjustmentRevisionGap(queryable, "weather_record", {
      ...logicalKey,
      sourceKind: revision.record.sourceKind as "forecast" | "physical_sensor",
      storedContentSha256: revision.contentHash,
    }, gap);
    gaps.push(gap);
  }
  return gaps;
}

// archive every row-specific permanent category after the serving transaction commits
async function recordWorkerRevisionGaps(
  client: RainAdjustmentMaintenanceClient,
  gaps: readonly AdjustmentRevisionGap[],
): Promise<void> {
  const archiveGaps = gaps[0]?.projectionIdentitySha256 === null ? gaps : gaps.slice(0, 1);
  // preserve every no-body row gap but only one disposition for one shared body
  for (const gap of archiveGaps) {
    await client.recordRevisionGap(gap);
  }
}

// encode every canonical stored weather metric as exact nullable binary64
function weatherAdjustmentRevisionRow(
  revision: PersistedWeatherAdjustmentRevision,
): Readonly<Record<string, string | null>> {
  const metrics = revision.record.metrics;
  return {
    apparentTemperatureC64: nullableRevisionBinary64(metrics.apparentTemperatureC),
    blackGlobeTemperatureC64: nullableRevisionBinary64(metrics.blackGlobeTemperatureC),
    cloudCoverPercent64: nullableRevisionBinary64(metrics.cloudCoverPercent),
    contentSha256: revision.contentHash,
    pm25MicrogramsPerCubicMeter64: nullableRevisionBinary64(metrics.pm25MicrogramsPerCubicMeter),
    precipitationMm64: nullableRevisionBinary64(metrics.precipitationMm),
    precipitationRateMmPerHour64: nullableRevisionBinary64(metrics.precipitationRateMmPerHour),
    pressureHpa64: nullableRevisionBinary64(metrics.pressureHpa),
    relativeHumidityPercent64: nullableRevisionBinary64(metrics.relativeHumidityPercent),
    soilElectricalConductivityMicrosiemensPerCm64:
      nullableRevisionBinary64(metrics.soilElectricalConductivityMicrosiemensPerCm),
    soilMoisturePercent64: nullableRevisionBinary64(metrics.soilMoisturePercent),
    solarRadiationWm264: nullableRevisionBinary64(metrics.solarRadiationWm2),
    temperatureC64: nullableRevisionBinary64(metrics.temperatureC),
    uvIndex64: nullableRevisionBinary64(metrics.uvIndex),
    validAt: revision.record.validAt,
    waterLevelM64: nullableRevisionBinary64(metrics.waterLevelM),
    wetBulbGlobeTemperatureC64: nullableRevisionBinary64(metrics.wetBulbGlobeTemperatureC),
    windDirectionDegrees64: nullableRevisionBinary64(metrics.windDirectionDegrees),
    windGustMps64: nullableRevisionBinary64(metrics.windGustMps),
    windSpeedMps64: nullableRevisionBinary64(metrics.windSpeedMps),
  };
}

// derive one explicit adapter/config contract epoch without provider-private fields
function weatherAdjustmentRevisionContractEpoch(
  revision: PersistedWeatherAdjustmentRevision,
  projectionKind: "actual_best_match" | "target_revision",
): string {
  const identity = createHash("sha256").update(
    `${revision.adapterVersion}\0${revision.sourceConfigFingerprint}`,
  ).digest("hex");
  return `${projectionKind === "actual_best_match" ? "legacy-v4" : "normalized-weather/v1"}/${identity}`;
}

// create one closed permanent target or comparator gap
function weatherAdjustmentRevisionGap(
  logicalKeySha256: string,
  projectionKind: "actual_best_match" | "target_revision",
  identity: string | null,
  reason: AdjustmentRevisionGap["reason"],
): AdjustmentRevisionGap {
  return {
    logicalKeySha256,
    projectionIdentitySha256: identity,
    projectionKind,
    projectionSha256: identity,
    reason,
  };
}

// freeze one causal recent-error state at model initialization
export async function buildCausalTemperatureRecentErrorState(
  pool: DatabasePool,
  input: Readonly<{
    repository: Pick<
      WorkerRepository,
      | "listCausalEcmwfTemperatureCanaryPriorHours"
      | "listCausalForecastObservationHourlyStations"
    >;
    siteSlug: string;
    targetRunInitializedAt: string;
  }>,
): Promise<EcmwfTemperatureCanaryRecentErrorState> {
  const targetRunInitializedAt = input.targetRunInitializedAt;
  const targetMilliseconds = Date.parse(targetRunInitializedAt);

  // require one canonical target initialization
  if (
    !Number.isFinite(targetMilliseconds) ||
    new Date(targetMilliseconds).toISOString() !== targetRunInitializedAt
  ) {
    throw new RangeError("temperature state target initialization is invalid");
  }

  const windowEndValidAt = new Date(
    targetMilliseconds - 7 * 3_600_000,
  ).toISOString();
  const windowStart = new Date(
    Date.parse(windowEndValidAt) - 71 * 3_600_000,
  ).toISOString();
  const observationEndExclusive = new Date(
    Date.parse(windowEndValidAt) + 3_600_000,
  ).toISOString();
  const [priorHours, stationHours] = await Promise.all([
    input.repository.listCausalEcmwfTemperatureCanaryPriorHours(pool, {
      from: windowStart,
      siteSlug: input.siteSlug,
      targetRunInitializedAt,
      toInclusive: windowEndValidAt,
    }),
    input.repository.listCausalForecastObservationHourlyStations(pool, {
      asOf: targetRunInitializedAt,
      from: windowStart,
      siteSlug: input.siteSlug,
      to: observationEndExclusive,
    }),
  ]);
  const forecastsByValidAt = new Map(
    priorHours.map((hour) => [hour.validAt, hour] as const),
  );
  const stationHoursByValidAt = new Map<string, typeof stationHours>();

  // group the bounded station matrix by target hour
  for (const stationHour of stationHours) {
    const existing = stationHoursByValidAt.get(stationHour.validAt) ?? [];
    stationHoursByValidAt.set(stationHour.validAt, [...existing, stationHour]);
  }

  const selected: Array<Readonly<{
    errorC: number;
    key: string;
    runInitializedAt: string;
    validAt: string;
  }>> = [];

  // walk the exact inclusive 72-hour causal slots
  for (let age = 0; age < 72; age += 1) {
    const validAt = new Date(
      Date.parse(windowEndValidAt) - age * 3_600_000,
    ).toISOString();
    const forecast = forecastsByValidAt.get(validAt);
    const rows = stationHoursByValidAt.get(validAt) ?? [];
    const spatial = rows.flatMap((row) => {
      const temperatureC = row.metrics.temperatureC;
      const station = FORECAST_OBSERVATION_STATIONS.find(
        (candidate) => candidate.key === row.physicalStationKey,
      );

      // omit missing or impossible station identities
      if (temperatureC === null || station === undefined) {
        return [];
      }

      return [{
        nearestRank: station.nearestRank,
        physicalStationKey: station.key,
        unnormalizedSpatialWeight: station.unnormalizedSpatialWeight,
        value: temperatureC,
      }];
    });
    const actual = scalarNetworkActual(spatial);

    // retain only hours with both causal forecast and network target
    if (forecast === undefined || actual === null) {
      continue;
    }

    selected.push({
      errorC: actual.value - forecast.rawTemperatureC,
      key: forecast.key,
      runInitializedAt: forecast.runInitializedAt,
      validAt,
    });
  }

  selected.sort((left, right) =>
    left.validAt.localeCompare(right.validAt) ||
    left.runInitializedAt.localeCompare(right.runInitializedAt) ||
    left.key.localeCompare(right.key)
  );
  const shortStart = Date.parse(windowEndValidAt) - 23 * 3_600_000;
  const short = selected.filter(
    (item) => Date.parse(item.validAt) >= shortStart,
  );
  const localDates = new Set(
    selected.map((item) => localCalendarFeaturesFor(item.validAt).localDate),
  );
  const shortSupported = short.length >= 6;
  const longSupported = selected.length >= 24;
  const rawB72 = longSupported
    ? median(selected.map((item) => item.errorC))
    : null;
  const maximumValid = selected.at(-1);

  return {
    b24C: shortSupported
      ? clipStateStatistic(median(short.map((item) => item.errorC)))
      : null,
    b72C: rawB72 === null ? null : clipStateStatistic(rawB72),
    cohort: "ecmwf_single_run_hindcast",
    localDates: localDates.size,
    mad72C:
      rawB72 === null
        ? null
        : Math.min(
            6,
            median(selected.map((item) => Math.abs(item.errorC - rawB72))),
          ),
    maximumSourceRunInitializedAt:
      selected.length === 0
        ? null
        : selected.reduce((maximum, item) =>
            item.runInitializedAt > maximum ? item.runInitializedAt : maximum,
          selected[0]!.runInitializedAt),
    maximumSourceValidAt: maximumValid?.validAt ?? null,
    n24: short.length,
    n72: selected.length,
    sourceKeys: selected.map((item) => item.key),
    supported: shortSupported && longSupported && localDates.size >= 2,
    targetRunInitializedAt,
    windowEndValidAt,
  };
}

// choose the newest run beyond the conservative six-hour delay
export function latestAvailableEcmwfInitialization(now: Date): string {
  const eligibleMilliseconds = now.getTime() - 6 * 3_600_000;
  const cycleMilliseconds = 6 * 3_600_000;
  return new Date(
    Math.floor(eligibleMilliseconds / cycleMilliseconds) * cycleMilliseconds,
  ).toISOString();
}

// cap optional HTTP controls for the sidecar
function boundedTemperatureCanaryFetchOptions(
  options: ProviderFetchOptions | undefined,
  now: () => Date,
): ProviderFetchOptions {
  return {
    ...options,
    maxAttempts: 1,
    maxBodyBytes: Math.min(options?.maxBodyBytes ?? 512_000, 512_000),
    now,
    timeoutMs: Math.min(options?.timeoutMs ?? 5_000, 5_000),
  };
}

// calculate one deterministic numeric median
function median(values: readonly number[]): number {
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);

  // average the two middle values for even samples
  if (sorted.length % 2 === 0) {
    return (sorted[middle - 1]! + sorted[middle]!) / 2;
  }

  return sorted[middle]!;
}

// cap one signed state statistic
function clipStateStatistic(value: number): number {
  return Math.max(-6, Math.min(6, value));
}

// retain truthful cold state for non-serving warmup runs
function emptyTemperatureRecentErrorState(
  targetRunInitializedAt: string,
): EcmwfTemperatureCanaryRecentErrorState {
  return {
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
    targetRunInitializedAt,
    windowEndValidAt: new Date(
      Date.parse(targetRunInitializedAt) - 7 * 3_600_000,
    ).toISOString(),
  };
}

// fail closed before optional collector work
function temperatureCanaryCollectionIsActive(
  runtime: LoadedForecastAdjustmentTemperatureCanaryRuntime | undefined,
  now: string,
): boolean {
  // preserve disabled loader results
  if (runtime?.state !== "active") {
    return false;
  }

  // keep collection active for a root-qualified permanent package
  if ("maintenancePackage" in runtime.bundle) {
    return true;
  }

  try {
    return forecastAdjustmentTemperatureCanaryIsActiveAt(runtime.bundle, now);
  } catch {
    // keep invalid in-memory state away from heartbeat work
    return false;
  }
}

// execute one committed scheduled run
export async function runScheduledSource(
  pool: DatabasePool,
  source: DueSource,
  options: Readonly<{
    fetchCurrent?: OpenMeteoCurrentOperation;
    fetchEcowitt?: EcowittLiveOperation;
    fetchForecast?: OpenMeteoForecastOperation;
    fetchTempest?: TempestObservationOperation;
    fetchOptions?: ProviderFetchOptions;
    fetchPublicStation?: PublicStationRangeOperation;
    fetchTide?: NoaaTideRangeOperation;
    now: () => Date;
    repository: WorkerRepository;
    revisionClient?: RainAdjustmentMaintenanceClient;
    captureEpoch?: AdjustmentRevisionCaptureEpochWitness;
    site: SiteConfiguration;
    ecowitt?: EcowittConfiguration | null;
    publicStations?: PublicStationConfiguration | null;
    tempest?: TempestConfiguration | null;
    tides?: TideConfiguration | null;
  }>,
): Promise<SourceRunResult> {
  const executionStartedAt = options.now().getTime();
  const ecowittConfiguration = options.ecowitt ?? null;
  const tempestConfiguration = options.tempest ?? null;
  const publicStationConfiguration = options.publicStations ?? null;
  const tideConfiguration = options.tides ?? null;
  const sourceConfiguration = options.site.sources.find(
    (candidate) => candidate.key === source.sourceKey,
  );
  const ecowittStation = ecowittConfiguration?.stations.find(
    (candidate) => candidate.sourceKey === source.sourceKey,
  );
  const tempestStation = tempestConfiguration?.stations.find(
    (candidate) => candidate.sourceKey === source.sourceKey,
  );
  const publicStationMatch = findPublicStationSource(
    publicStationConfiguration,
    source.sourceKey,
  );
  const tideStation = tideConfiguration?.stations.find(
    (candidate) => candidate.source.key === source.sourceKey,
  );
  const openMeteoCurrentSource =
    source.active &&
    source.providerKey === "open-meteo" &&
    source.sourceKind === "model_current" &&
    source.siteSlug === options.site.site.key &&
    sourceConfiguration !== undefined &&
    sourceConfiguration.capabilities.includes("current") &&
    sourceIdentityMatchesConfiguration(source, options.site, sourceConfiguration);
  const openMeteoForecastSource =
    source.active &&
    source.providerKey === "open-meteo" &&
    source.sourceKind === "forecast" &&
    source.siteSlug === options.site.site.key &&
    sourceConfiguration !== undefined &&
    sourceConfiguration.capabilities.includes("forecast") &&
    sourceIdentityMatchesConfiguration(source, options.site, sourceConfiguration);
  const openMeteoSource = openMeteoCurrentSource || openMeteoForecastSource;
  const ecowittSource =
    source.active &&
    ecowittConfiguration !== null &&
    ecowittStation !== undefined &&
    sourceIdentityMatchesEcowittConfiguration(
      source,
      ecowittConfiguration,
      ecowittStation,
    );
  const tempestSource =
    source.active &&
    tempestConfiguration !== null &&
    tempestStation !== undefined &&
    sourceIdentityMatchesTempestConfiguration(
      source,
      tempestConfiguration,
      tempestStation,
    );
  const publicStationSource =
    source.active &&
    publicStationConfiguration !== null &&
    publicStationMatch !== null &&
    publicStationMatch.source.active &&
    publicStationMatch.source.capabilities.includes("current") &&
    sourceIdentityMatchesPublicStationConfiguration(
      source,
      publicStationConfiguration,
      publicStationMatch.station,
      publicStationMatch.source,
    );
  const tideSource =
    source.active &&
    tideConfiguration !== null &&
    tideStation !== undefined &&
    tideStation.active &&
    tideStation.source.active &&
    sourceIdentityMatchesTideConfiguration(
      source,
      tideConfiguration,
      tideStation,
    );

  // skip sources outside a loaded exact runtime contract
  if (
    !openMeteoSource &&
    !ecowittSource &&
    !tempestSource &&
    !publicStationSource &&
    !tideSource
  ) {
    return {
      durationMs: elapsedMilliseconds(executionStartedAt, options.now()),
      reason: "source is not an active configured scheduled source",
      recordCount: 0,
      runId: null,
      secondaryError: null,
      sourceId: source.id,
      status: "skipped",
    };
  }

  // verify the selected frozen adapter contract
  if (openMeteoSource) {
    requireContractVersion(
      sourceConfiguration.adapterConfig,
      openMeteoForecastSource ? "forecast-daily/v4" : "forecast-current/v1",
    );
  } else if (ecowittStation !== undefined) {
    requireContractVersion(
      ecowittStation.adapterConfig,
      ECOWITT_LOCAL_LIVE_ADAPTER_VERSION,
    );
  } else if (tempestStation !== undefined) {
    requireContractVersion(tempestStation.adapterConfig, "tempest-observations/v2");
  } else if (publicStationMatch !== null) {
    requireContractVersion(
      publicStationMatch.source.adapterConfig,
      publicStationAdapterVersion(publicStationMatch.source.adapter),
    );
  } else if (tideStation !== undefined) {
    requireContractVersion(
      tideStation.source.adapterConfig,
      tideStation.source.sourceKind === "tide_observation"
        ? NOAA_TIDE_OBSERVATION_ADAPTER_VERSION
        : NOAA_TIDE_PREDICTION_ADAPTER_VERSION,
    );
  }
  const session = await options.repository.acquireSourceSession(pool, source.id);

  // skip a source already owned elsewhere
  if (session === null) {
    return {
      durationMs: elapsedMilliseconds(executionStartedAt, options.now()),
      reason: "source lock is held",
      recordCount: 0,
      runId: null,
      secondaryError: null,
      sourceId: source.id,
      status: "skipped",
    };
  }

  let runId: string | null = null;
  let attempts = 0;
  let recordCount = 0;
  let result: SourceRunResult;

  try {
    const now = options.now();
    const deadlines = planIngestionDeadlines(now, options.fetchOptions);
    await options.repository.abandonExpiredRuns(session, now.toISOString());
    const checkpoint = await options.repository.getScheduledCheckpoint(session);
    const window = tideSource && tideStation?.source.sourceKind === "tide_observation"
      ? scheduledWindow(
          new Date(now.getTime() - 12 * 60_000),
          source.cadenceSeconds,
          checkpoint,
          31 * 86_400,
        )
      : ecowittSource || tempestSource
      ? scheduledWindow(now, source.cadenceSeconds, null)
      : scheduledWindow(
          now,
          source.cadenceSeconds,
          checkpoint,
          publicStationSource && publicStationMatch !== null
            ? publicStationMatch.source.maximumChunkDays * 86_400
            : null,
        );
    const started = await options.repository.startIngestionRun(session, {
      adapterVersion: tempestSource
        ? TEMPEST_OBSERVATION_ADAPTER_VERSION
        : ecowittSource
          ? ECOWITT_LOCAL_LIVE_ADAPTER_VERSION
        : publicStationSource && publicStationMatch !== null
          ? publicStationAdapterVersion(publicStationMatch.source.adapter)
          : tideSource && tideStation !== undefined
            ? tideStation.source.sourceKind === "tide_observation"
              ? NOAA_TIDE_OBSERVATION_ADAPTER_VERSION
              : NOAA_TIDE_PREDICTION_ADAPTER_VERSION
          : openMeteoForecastSource
            ? OPEN_METEO_FORECAST_ADAPTER_VERSION
            : OPEN_METEO_CURRENT_ADAPTER_VERSION,
      deadlineAt: deadlines.runDeadlineAt,
      mode: "scheduled",
      requestMetadata: {
        endpoint: tempestSource
          ? "observations/device"
          : ecowittSource
            ? "get_livedata_info"
          : publicStationSource && publicStationMatch !== null
            ? publicStationEndpoint(publicStationMatch.source.adapter)
            : tideSource
              ? "noaa-co-ops/datagetter"
            : openMeteoForecastSource
              ? "forecast/hourly"
              : "forecast/current",
      },
      requestedEndExclusive: window.endExclusive,
      requestedStart: window.start,
      sourceConfigFingerprint: source.sourceConfigFingerprint,
    });
    runId = started.id;
    const providerOptions = {
      ...options.fetchOptions,
      deadlineAt: deadlines.providerDeadlineAt,
      now: options.now,
    };
    const batch =
      ecowittSource && ecowittStation !== undefined
        ? await (options.fetchEcowitt ?? fetchEcowittLive)(
            {
              expectedMac: ecowittStation.expectedMac,
              gatewayHost: ecowittStation.gatewayHost,
              model: ecowittStation.model,
              previousCursor: checkpoint?.providerCursor ?? null,
              sourceId: source.id,
              timezone: ecowittStation.timezone,
            },
            providerOptions,
          )
        : tempestSource && tempestStation !== undefined
        ? await requireTempestOperation(options.fetchTempest)(
            {
              deviceId: tempestStation.deviceId,
              endExclusive: window.endExclusive,
              locationId: tempestStation.locationId,
              serial: tempestStation.serial,
              sourceId: source.id,
              start: window.start,
              timezone: tempestStation.timezone,
            },
            providerOptions,
          )
        : publicStationSource && publicStationMatch !== null
          ? await (options.fetchPublicStation ?? fetchPublicStationRange)(
              publicStationRequest(
                publicStationMatch.station,
                publicStationMatch.source,
                source.id,
                window,
              ),
              providerOptions,
            )
          : tideSource && tideStation !== undefined
            ? await (options.fetchTide ?? fetchNoaaTideRange)(
                tideScheduledRequest(tideStation, source.id, window, now),
                providerOptions,
              )
          : openMeteoForecastSource
            ? await (options.fetchForecast ?? fetchOpenMeteoForecast)(
                {
                  latitude: options.site.site.latitude,
                  longitude: options.site.site.longitude,
                  sourceId: source.id,
                  timezone: options.site.site.timezone,
                },
                providerOptions,
              )
          : await (options.fetchCurrent ?? fetchOpenMeteoCurrent)(
              {
                latitude: options.site.site.latitude,
                longitude: options.site.site.longitude,
                sourceId: source.id,
                timezone: options.site.site.timezone,
              },
              providerOptions,
            );
    attempts = batch.attempts;
    recordCount = batch.records.length;
    const lastRecord = batch.records.at(-1);

    // reject successful empty batches
    if (lastRecord === undefined) {
      throw new ProviderFailure({
        classification: "invalid_payload",
        code: "empty_payload",
        message: "provider returned no records",
      });
    }

    await options.repository.completeScheduledIngestion(session, {
      attempts,
      expectedCheckpointVersion: checkpoint?.version ?? null,
      lastValidAt: openMeteoForecastSource ||
        tideStation?.source.sourceKind === "tide_prediction"
        ? window.endExclusive
        : lastRecord.validAt,
      providerCursor: batch.providerCursor,
      records: batch.records,
      ...(options.revisionClient === undefined
        ? {}
        : options.captureEpoch === undefined
          ? {}
          : { revisionArchiver: createWeatherAdjustmentRevisionArchiver(
            pool,
            options.revisionClient,
            options.captureEpoch,
          ) }),
      responseMetadata: batch.responseMetadata,
      runId,
      upstreamResponseChecksum: batch.checksum,
      windowEndExclusive: window.endExclusive,
      windowStart: window.start,
    });

    result = {
      durationMs: 0,
      reason: null,
      recordCount,
      runId,
      secondaryError: null,
      sourceId: source.id,
      status: "succeeded",
    };
  } catch (error) {
    const failure = asProviderFailure(error, Math.max(1, attempts));
    let secondaryError: string | null = null;

    // finalize only after a committed running row exists
    if (runId !== null) {
      secondaryError = await guardFailScheduledRun(
        options.repository,
        session,
        runId,
        failure,
      );
    }

    result = {
      durationMs: 0,
      reason: failure.ingestionError.code,
      recordCount,
      runId,
      secondaryError,
      sourceId: source.id,
      status: "failed",
    };
  }

  const releaseError = await guardReleaseSession(session);

  // retain primary results when cleanup also fails
  if (releaseError !== null) {
    return {
      ...result,
      durationMs: elapsedMilliseconds(executionStartedAt, options.now()),
      reason:
        result.status === "succeeded"
          ? "session_release_failed"
          : result.reason,
      secondaryError: combineWorkerDiagnostics([
        { label: "finalization", value: result.secondaryError },
        { label: "release", value: releaseError },
      ]),
      status: "failed",
    };
  }

  return {
    ...result,
    durationMs: elapsedMilliseconds(executionStartedAt, options.now()),
  };
}

// calculate an anchored half-open scheduled interval
export function scheduledWindow(
  now: Date,
  cadenceSeconds: number,
  checkpoint: ScheduledCheckpointState | null,
  maximumRangeSeconds: number | null = null,
): Readonly<{ endExclusive: string; start: string }> {
  const cadenceMs = cadenceSeconds * 1_000;

  // require a bounded source cadence
  if (!Number.isSafeInteger(cadenceMs) || cadenceMs < 60_000) {
    throw new RangeError("source cadence must be at least one minute");
  }

  // require an optional range at least as large as one cadence
  if (
    maximumRangeSeconds !== null &&
    (!Number.isSafeInteger(maximumRangeSeconds) ||
      maximumRangeSeconds < cadenceSeconds)
  ) {
    throw new RangeError("scheduled maximum range must cover one cadence");
  }

  const end = Math.floor(now.getTime() / cadenceMs) * cadenceMs;
  const checkpointStart = checkpoint === null
    ? end - cadenceMs
    : Math.min(Date.parse(checkpoint.windowEndExclusive), end - cadenceMs);
  const maximumStart = maximumRangeSeconds === null
    ? checkpointStart
    : end - maximumRangeSeconds * 1_000;
  const start = Math.max(checkpointStart, maximumStart);

  return {
    endExclusive: new Date(end).toISOString(),
    start: new Date(start).toISOString(),
  };
}

// finalize a failed scheduled run without masking its original failure
async function guardFailScheduledRun(
  repository: WorkerRepository,
  session: SourceSession,
  runId: string,
  failure: ProviderFailure,
): Promise<string | null> {
  try {
    await repository.failIngestionRun(session, {
      attempts: failure.attempts,
      error: failure.ingestionError,
      responseMetadata:
        failure.status === null ? null : { http_status: failure.status },
      runId,
    });
    return null;
  } catch (error) {
    // retain bounded secondary diagnostics
    return boundedWorkerError(error);
  }
}

// validate the frozen source contract version
function requireContractVersion(
  adapterConfig: unknown,
  expected: string,
): void {
  // require object configuration
  if (
    typeof adapterConfig !== "object" ||
    adapterConfig === null ||
    Array.isArray(adapterConfig) ||
    !("contractVersion" in adapterConfig) ||
    adapterConfig.contractVersion !== expected
  ) {
    throw new Error(`source adapter contract must be ${expected}`);
  }
}

// require a credential-bound Tempest operation
function requireTempestOperation(
  operation: TempestObservationOperation | undefined,
): TempestObservationOperation {
  // fail before provider I/O when credentials are unavailable
  if (operation === undefined) {
    throw new Error("Tempest scheduled ingestion requires a configured API key");
  }

  return operation;
}

// find one public-station source and its station
function findPublicStationSource(
  configuration: PublicStationConfiguration | null,
  sourceKey: string,
): Readonly<{
  source: PublicStationSourceConfiguration;
  station: PublicStationConfigurationStation;
}> | null {
  // preserve an omitted integration
  if (configuration === null) {
    return null;
  }

  // scan the bounded checked catalog
  for (const station of configuration.stations) {
    const source = station.sources.find((candidate) => candidate.key === sourceKey);

    // return the unique parsed match
    if (source !== undefined) {
      return { source, station };
    }
  }

  return null;
}

// build one provider range from checked material
function publicStationRequest(
  station: PublicStationConfigurationStation,
  source: PublicStationSourceConfiguration,
  sourceId: string,
  window: Readonly<{ endExclusive: string; start: string }>,
): PublicStationRangeRequest {
  const shared = {
    endExclusive: window.endExclusive,
    model: station.model,
    serial: station.serial,
    sourceId,
    start: window.start,
    timezone: station.timezone,
  } as const;

  // build the Ambient request
  if (source.adapter === "ambient-weather") {
    return {
      ...shared,
      adapter: source.adapter,
      deviceId: requireConfiguredString(source.adapterConfig.deviceId, "deviceId"),
      macAddress: requireConfiguredString(
        source.adapterConfig.macAddress,
        "macAddress",
      ),
    };
  }

  // build the Weather Underground request
  if (source.adapter === "weather-underground") {
    return {
      ...shared,
      adapter: source.adapter,
      publicApiKey: requireConfiguredString(
        source.adapterConfig.publicApiKey,
        "publicApiKey",
      ),
      stationId: requireConfiguredString(
        source.adapterConfig.stationId,
        "stationId",
      ),
    };
  }

  // build the PurpleAir request
  if (source.adapter === "purpleair") {
    return {
      ...shared,
      adapter: source.adapter,
      mapVersion: requireConfiguredString(
        source.adapterConfig.mapVersion,
        "mapVersion",
      ),
      sensorIndex: requireConfiguredInteger(
        source.adapterConfig.sensorIndex,
        "sensorIndex",
      ),
    };
  }

  return {
    ...shared,
    adapter: source.adapter,
    deviceId: requireConfiguredString(source.adapterConfig.deviceId, "deviceId"),
    outdoorModuleId: requireConfiguredString(
      source.adapterConfig.outdoorModuleId,
      "outdoorModuleId",
    ),
    rainModuleId: requireConfiguredString(
      source.adapterConfig.rainModuleId,
      "rainModuleId",
    ),
    windModuleId: requireConfiguredString(
      source.adapterConfig.windModuleId,
      "windModuleId",
    ),
  };
}

// build one scheduled NOAA tide range
function tideScheduledRequest(
  station: TideStationConfiguration,
  sourceId: string,
  window: Readonly<{ endExclusive: string; start: string }>,
  now: Date,
): NoaaTideRangeRequest {
  const shared = {
    datum: "MLLW" as const,
    sourceId,
    stationId: station.serial,
    timezone: station.timezone,
  };

  // retain the checkpointed observation window
  if (station.source.sourceKind === "tide_observation") {
    return {
      ...shared,
      endExclusive: window.endExclusive,
      product: "water_level",
      sourceKind: station.source.sourceKind,
      start: window.start,
    };
  }

  const start = Math.floor(now.getTime() / 60_000) * 60_000;
  return {
    ...shared,
    endExclusive: new Date(start + 30 * 86_400_000).toISOString(),
    interval: "hilo",
    product: "predictions",
    sourceKind: station.source.sourceKind,
    start: new Date(start).toISOString(),
  };
}

// label one public provider endpoint without credentials
function publicStationEndpoint(
  adapter: PublicStationSourceConfiguration["adapter"],
): string {
  // label Ambient requests
  if (adapter === "ambient-weather") {
    return "device-data";
  }

  // label Weather Underground requests
  if (adapter === "weather-underground") {
    return "pws/history/all";
  }

  // label PurpleAir public-map history requests
  if (adapter === "purpleair") {
    return "sensors/history/csv";
  }

  return "getmeasure";
}

// require one already-validated material string
function requireConfiguredString(value: unknown, field: string): string {
  // fail closed on impossible in-memory drift
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`public-station ${field} is invalid`);
  }

  return value;
}

// require one already-validated material integer
function requireConfiguredInteger(value: unknown, field: string): number {
  // fail closed on impossible in-memory drift
  if (!Number.isSafeInteger(value) || Number(value) < 1) {
    throw new Error(`public-station ${field} is invalid`);
  }

  return Number(value);
}

// start the long-running worker process
export async function startWorkerProcess(
  options: Readonly<{ once?: boolean }> = {},
): Promise<void> {
  const configuration = await loadWorkerConfiguration();
  const pool = createDatabasePool(configuration.database);
  const fetchCurrent = createOpenMeteoCurrentOperation(
    configuration.openMeteoCompatibilityOrigin,
  );
  const fetchForecast = createOpenMeteoForecastOperation(
    configuration.openMeteoCompatibilityOrigin,
  );
  const fetchEcmwfSingleRun = createOpenMeteoEcmwfSingleRunOperation(
    configuration.openMeteoCompatibilityOrigin,
  );
  const temperatureCanaryRuntime =
    await createForecastAdjustmentTemperatureCanaryRuntimeLoader({
      ...(configuration.temperatureCanaryKillSwitch === undefined
        ? {}
        : {
            environmentKillSwitch: configuration.temperatureCanaryKillSwitch,
          }),
      now: () => new Date().toISOString(),
    }).load();
  const rainAdjustmentRuntime =
    await createForecastAdjustmentRainRuntimeRegistryLoader().load();
  let adjustmentCaptureEpoch: AdjustmentRevisionCaptureEpochWitness | undefined;
  // keep every producer inactive until the root-installed zero-frontier witness exists
  try {
    adjustmentCaptureEpoch = await loadAdjustmentRevisionCaptureEpochWitness();
  } catch {
    adjustmentCaptureEpoch = undefined;
  }
  const rainAdjustmentRevisionClient = configuration.adjustmentMaintenanceApiOrigin === null ||
    adjustmentCaptureEpoch === undefined
    ? undefined
    : createHttpRainAdjustmentMaintenanceClient(configuration.adjustmentMaintenanceApiOrigin);
  const rainAdjustmentMaintenance = await loadRainAdjustmentMaintenance(
    rainAdjustmentRevisionClient,
    adjustmentCaptureEpoch,
  );
  const rainAdjustmentControlReference = await loadRainAdjustmentControlReference(
    rainAdjustmentRevisionClient,
    adjustmentCaptureEpoch,
  );
  const adjustmentMaintenanceCapture = configuration.adjustmentMaintenanceApiOrigin === null ||
    adjustmentCaptureEpoch === undefined
    ? undefined
    : createAdjustmentMaintenanceCaptureTrigger(configuration.adjustmentMaintenanceApiOrigin);
  const fetchTempest =
    configuration.tempestApiKey === null
      ? undefined
      : createTempestObservationOperation(configuration.tempestApiKey);
  await assertWorkerDatabaseReadiness(
    pool,
    configuration.migrationDirectory,
    configuration.version,
    configuration.migrationAuthorization,
  );
  const durableHealth = await readWorkerHealth(pool, configuration.instance);
  const runIteration = createWorkerIterationRunner(pool, {
    ...(adjustmentCaptureEpoch === undefined ? {} : { adjustmentCaptureEpoch }),
    fetchCurrent,
    fetchEcmwfSingleRun,
    fetchForecast,
    ...(adjustmentMaintenanceCapture === undefined ? {} : { adjustmentMaintenanceCapture }),
    ...(fetchTempest === undefined ? {} : { fetchTempest }),
    instance: configuration.instance,
    lastSuccessAt: durableHealth.lastSuccessAt,
    site: configuration.site,
    publicStations: configuration.publicStations,
    // require explicit capture opt-in and reject compatibility runs
    ...(isRainCollectionEnabled(configuration)
      ? {
          rainAdjustmentEnabled: true,
          rainCollection: {
            stationsAuthorized: RAIN_COLLECTION_POLICY.stationAccessAuthorized,
            ...(configuration.tempestApiKey === null ? {} : { apiKey: configuration.tempestApiKey }),
          },
        }
      : {}),
    ecowitt: configuration.ecowitt,
    rainAdjustmentRuntime,
    ...(rainAdjustmentControlReference === undefined ? {} : { rainAdjustmentControlReference }),
    ...(rainAdjustmentMaintenance === undefined ? {} : { rainAdjustmentMaintenance }),
    ...(rainAdjustmentRevisionClient === undefined ? {} : { rainAdjustmentRevisionClient }),
    tempest: configuration.tempest,
    temperatureCanaryRuntime,
    tides: configuration.tides,
    version: configuration.version,
  });
  const scheduler = createNonOverlappingScheduler({
    cadenceMs: WORKER_CADENCE_MS,
    key: configuration.instance,
    onError: (_error: unknown) => {
      // emit an allowlisted scheduler failure
      writeWorkerDiagnostic(
        createWorkerDiagnostic({
          count: 0,
          durationMs: 0,
          errorCode: "worker_iteration_failed",
          event: "worker_iteration",
          release: configuration.version,
          runId: null,
          sourceId: null,
        }),
      );
    },
    run: async () => {
      await runIteration();
    },
  });
  let startupBackfill: Promise<void> | null = null;

  // run one compatibility loop without retaining timers
  if (options.once === true) {
    await scheduler.trigger();
    await pool.end();
    return;
  }

  // close the retained database pool on termination
  const shutdown = async (): Promise<void> => {
    scheduler.stop();

    // allow an active exact chunk to finish during graceful shutdown
    if (startupBackfill !== null) {
      await startupBackfill.catch(() => undefined);
    }

    await pool.end();
  };
  process.once("SIGINT", () => {
    void shutdown();
  });
  process.once("SIGTERM", () => {
    void shutdown();
  });
  scheduler.start();
  await scheduler.trigger();
  startupBackfill = runStartupPublicStationBackfill(
    pool,
    configuration.publicStations,
    configuration.version,
  );
}

// load shadow-only rain state without blocking the served worker process
async function loadRainAdjustmentMaintenance(
  client: RainAdjustmentMaintenanceClient | undefined,
  captureEpoch: AdjustmentRevisionCaptureEpochWitness | undefined,
): Promise<RainAdjustmentMaintenanceOptions | undefined> {
  // preserve an omitted private relay as an inactive shadow path
  if (client === undefined || captureEpoch === undefined) {
    return undefined;
  }
  try {
    const candidate = await loadInstalledMaintenanceShadowCandidate({ family: "rain" });
    // keep serving active when no root-installed rain slot exists
    if (candidate === null) {
      return undefined;
    }
    return {
      candidate,
      captureEpoch,
      client,
    };
  } catch {
    // fail closed for shadow eligibility without stopping serving
    return undefined;
  }
}

// load pre-month control authority independently of any fitted shadow candidate
async function loadRainAdjustmentControlReference(
  client: RainAdjustmentMaintenanceClient | undefined,
  captureEpoch: AdjustmentRevisionCaptureEpochWitness | undefined,
): Promise<InstalledRainMaintenanceControlReference | undefined> {
  // keep control scoring inactive without the private archive path and epoch witness
  if (client === undefined || captureEpoch === undefined) {
    return undefined;
  }
  try {
    const reference = await loadInstalledRainMaintenanceControlReference();
    // an omitted control slot is an explicit feature-only capture state
    if (reference === null) {
      return undefined;
    }
    return reference;
  } catch {
    // preserve serving and feature capture when control authority is unavailable
    return undefined;
  }
}

// create one credential-free trigger on the existing private data network
function createAdjustmentMaintenanceCaptureTrigger(
  origin: string,
): NonNullable<WorkerIterationOptions["adjustmentMaintenanceCapture"]> {
  const base = new URL(origin);
  // prohibit public hosts, paths, credentials and query controls
  if (base.protocol !== "http:" ||
      !["api", "127.0.0.1", "localhost", "[::1]"].includes(base.hostname) ||
      base.username !== "" || base.password !== "" || base.pathname !== "/" ||
      base.search !== "" || base.hash !== "") {
    throw new RangeError("adjustment maintenance api origin is invalid");
  }
  return async (request) => {
    const response = await fetch(
      new URL("/internal/adjustment-maintenance/shadow/capture", base),
      {
        body: JSON.stringify(request),
        headers: { "content-type": "application/json" },
        method: "POST",
      },
    );
    // do not reinterpret a blocked graph capture as worker success
    if (!response.ok) {
      throw new Error("adjustment maintenance capture failed");
    }
  };
}

// resume configured public archives without blocking worker readiness
async function runStartupPublicStationBackfill(
  pool: DatabasePool,
  configuration: PublicStationConfiguration | null,
  version: string,
): Promise<void> {
  // preserve an explicitly disabled integration
  if (configuration === null) {
    return;
  }

  const startedAt = Date.now();

  try {
    const sources = await resolvePublicStationBackfillSources(
      pool,
      configuration,
      [],
    );
    const today = new Date().toISOString().slice(0, 10);
    const report = await executePublicStationBackfill(
      pool,
      {
        dryRun: false,
        from: null,
        reportPath: null,
        resume: true,
        site: configuration.siteKey,
        sourceKeys: [],
        to: addUtcDays(today, -1),
      },
      configuration,
      sources,
    );

    // emit one bounded result per source
    for (const source of report.sources) {
      writeWorkerDiagnostic(
        createWorkerDiagnostic({
          count: source.records,
          durationMs: Date.now() - startedAt,
          errorCode:
            source.exitCode === 0 ? null : "public_station_backfill_failed",
          event: "source_run",
          release: version,
          runId: null,
          sourceId: source.source,
        }),
      );
    }
  } catch {
    // emit only an allowlisted startup failure
    writeWorkerDiagnostic(
      createWorkerDiagnostic({
        count: 0,
        durationMs: Date.now() - startedAt,
        errorCode: "public_station_backfill_failed",
        event: "worker_iteration",
        release: version,
        runId: null,
        sourceId: null,
      }),
    );
  }
}

// add UTC calendar days
function addUtcDays(value: string, days: number): string {
  const date = new Date(`${value}T00:00:00.000Z`);
  date.setUTCDate(date.getUTCDate() + days);
  return date.toISOString().slice(0, 10);
}

// calculate a bounded non-negative duration
function elapsedMilliseconds(startedAt: number, completedAt: Date): number {
  return Math.max(0, Math.round(completedAt.getTime() - startedAt));
}

// read the current clock
function defaultNow(): Date {
  return new Date();
}

// run only from the built worker entrypoint
if (
  process.argv[1] !== undefined &&
  pathToFileURL(process.argv[1]).href === import.meta.url
) {
  const arguments_ = process.argv.slice(2);

  // reject undocumented process controls
  if (arguments_.some((argument) => argument !== "--once")) {
    process.stderr.write("worker supports only the optional --once flag\n");
    process.exitCode = 1;
  } else {
    startWorkerProcess({ once: arguments_.includes("--once") }).catch((error: unknown) => {
      process.stderr.write(`${boundedWorkerError(error)}\n`);
      process.exitCode = 1;
    });
  }
}
