import { createServer, type IncomingHttpHeaders, type Server } from "node:http";

import {
  getDailyPrecipitation,
  getCurrentWeather,
  getEcmwfTemperatureCanarySidecar,
  getEcmwfTemperatureCanaryStatus,
  getForecastPressureContext as readForecastPressureContext,
  getWeatherForecast,
  getLatestWorkerHeartbeat,
  listActiveSites,
  listForecastObservationInputs,
  listTideRecords,
  listWeatherHistory,
  listWeatherTrends,
  readRainCollectionStatus,
  readRainAdjustmentRun,
  verifyMigrationReadiness,
  type ActiveSiteRow,
  type CurrentQuery,
  type CurrentWeatherRecordRow,
  type DailyPrecipitationRow,
  type EcmwfTemperatureCanarySidecar,
  type EcmwfTemperatureCanaryStatus,
  type ForecastProductSelection,
  type ForecastObservationInput,
  type HistoryQuery,
  type MigrationReadinessAuthorization,
  type TrendPointRow,
  type TideRecordRow,
  type WeatherRecordRow,
} from "@weather/database";
import {
  applyForecastAdjustment,
  applyForecastAdjustmentTemperatureCanary,
  type ApplyForecastAdjustmentInputV1,
  type ApplyTemperatureCanaryInputV1,
  type ForecastTemperatureCanaryDecisionV1,
  type ForecastAdjustmentWindQualifiedMaintenanceDecision,
  type LoadedForecastAdjustmentRuntimeV1,
  type LoadedForecastAdjustmentTemperatureCanaryRuntime,
  type LoadedForecastAdjustmentWindCanaryRuntime,
} from "@weather/forecast-adjustment";
import {
  createForecastAdjustmentFailRawDecision,
  parseSourceKind,
  sourceKindLabel,
  validateStableKey,
  validateUtcInstant,
  type CanonicalWeatherMetrics,
  type ForecastAdjustmentDecision,
  type ForecastAdjustmentMetric,
  type JsonValue,
  type SourceKind,
  type StationKind,
  type RainCollectionStatus,
} from "@weather/domain";
import { projectRainCollectionStatus } from "./rain-collection.js";
import type { RainAdjustmentRun } from "@weather/database";
import {
  rainAdjustmentDecision,
  rainAdjustmentRuntime,
  validRainAdjustmentRun,
  type ApiRainAdjustmentDecision,
  type ApiRainAdjustmentStartupSnapshot,
} from "./rain-adjustment.js";

export * from "./adjustment-maintenance-shadow.js";

type DatabasePool = Parameters<typeof listActiveSites>[0];
type LoadedApiForecastAdjustmentRuntime =
  | LoadedForecastAdjustmentRuntimeV1
  | LoadedForecastAdjustmentWindCanaryRuntime;
type ApiForecastAdjustmentDecision =
  | ForecastAdjustmentDecision
  | ForecastAdjustmentWindQualifiedMaintenanceDecision;

export interface HealthSnapshot {
  readonly database: "ready" | "unavailable";
  readonly migration: {
    readonly status: "current" | "outdated" | "unavailable";
    readonly version: string | null;
  };
  readonly workerLastLoopAt: string | null;
}

export interface WeatherReadStore {
  getRainAdjustmentRun?(siteSlug: string, asOf: string): Promise<RainAdjustmentRun | null>;
  getRainCollectionStatus?(): Promise<RainCollectionStatus>;
  getDailyPrecipitation(
    siteSlug: string,
    from: string,
    to: string,
  ): Promise<DailyPrecipitationRow | null>;
  getCurrent(
    siteSlug: string,
    query: CurrentQuery,
  ): Promise<readonly CurrentWeatherRecordRow[]>;
  getForecast(
    siteSlug: string,
    asOf: string,
    hours: number,
    productSelection?: ForecastProductSelection,
  ): Promise<readonly WeatherRecordRow[]>;
  // read one fixed private projection input without exposing archive controls
  getForecastObservationInput?(
    asOf: string,
  ): Promise<ForecastObservationInput | null>;
  // read optional retained pressure context
  getForecastPressureContext?(
    siteSlug: string,
    asOf: string,
  ): Promise<readonly WeatherRecordRow[]>;
  getTemperatureCanarySidecar?(
    siteSlug: string,
    asOf: string,
    from: string,
    to: string,
  ): Promise<EcmwfTemperatureCanarySidecar | null>;
  getTemperatureCanaryStatus?(
    siteSlug: string,
    asOf: string,
  ): Promise<EcmwfTemperatureCanaryStatus | null>;
  getHealth(): Promise<HealthSnapshot>;
  listHistory(query: HistoryQuery): Promise<readonly WeatherRecordRow[]>;
  listSites(): Promise<readonly ActiveSiteRow[]>;
  listTides(
    siteSlug: string,
    from: string,
    to: string,
  ): Promise<readonly TideRecordRow[]>;
  listTrends(
    siteSlug: string,
    from: string,
    to: string,
  ): Promise<readonly TrendPointRow[]>;
}

export interface ApiSource {
  readonly attribution: {
    readonly label: string;
    readonly url: string;
  };
  readonly id: string;
  readonly key: string;
  readonly kind: SourceKind;
  readonly providerKey: string;
  readonly providerName: string;
  readonly provenanceLabel: string;
}

export interface ApiStation {
  readonly kind: StationKind;
  readonly latitude: number;
  readonly longitude: number;
  readonly name: string;
  readonly slug: string;
  readonly sources: readonly ApiSource[];
}

export interface ApiSite {
  readonly latitude: number;
  readonly longitude: number;
  readonly name: string;
  readonly slug: string;
  readonly stations: readonly ApiStation[];
  readonly timezone: string;
}

export interface ApiWeatherRecord {
  readonly freshness: {
    readonly ageSeconds: number;
    readonly label: string;
    readonly status: "delayed" | "fresh" | "stale";
  };
  readonly id: string;
  readonly metadata: {
    readonly device: {
      readonly model: string | null;
      readonly serial: string | null;
      readonly vendor: string | null;
    } | null;
    readonly provider: {
      readonly dataset: string | null;
      readonly elevationM: number | null;
      readonly gridCell: string | null;
      readonly propertySensors: readonly ApiPropertySensorSnapshot[] | null;
    } | null;
    readonly quality: {
      readonly confidencePercent: number | null;
      readonly flags: readonly string[] | null;
      readonly interpolation: string | null;
      readonly status: string | null;
    } | null;
    readonly upstream: {
      readonly model: string | null;
      readonly timezone: string;
    };
  };
  readonly metrics: {
    readonly apparentTemperatureC: number | null;
    readonly blackGlobeTemperatureC: number | null;
    readonly cloudCoverPercent: number | null;
    readonly pm25MicrogramsPerCubicMeter: number | null;
    readonly precipitationMm: number | null;
    readonly precipitationRateMmPerHour: number | null;
    readonly pressureHpa: number | null;
    readonly relativeHumidityPercent: number | null;
    readonly soilElectricalConductivityMicrosiemensPerCm: number | null;
    readonly soilMoisturePercent: number | null;
    readonly solarRadiationWm2: number | null;
    readonly temperatureC: number | null;
    readonly uvIndex: number | null;
    readonly windDirectionDegrees: number | null;
    readonly windGustMps: number | null;
    readonly windSpeedMps: number | null;
    readonly wetBulbGlobeTemperatureC: number | null;
  };
  readonly productRunAt: string | null;
  readonly provenance: {
    readonly attribution: {
      readonly label: string;
      readonly url: string;
    };
    readonly label: string;
    readonly providerKey: string;
    readonly sourceId: string;
    readonly sourceKey: string;
    readonly sourceKind: SourceKind;
    readonly stationSlug: string;
  };
  readonly receivedAt: string;
  readonly revisionCount: number;
  readonly validAt: string;
}

// add source-local tendency only to current records
export interface ApiCurrentWeatherRecord extends ApiWeatherRecord {
  readonly pressureChange3hHpa: number | null;
}

// extend only forecast rows with fail-raw adjustment decisions
export interface ApiForecastWeatherRecord extends ApiWeatherRecord {
  readonly rainAdjustment: ApiRainAdjustmentDecision;
  readonly adjustment: ApiForecastAdjustmentDecision;
  readonly temperatureAdjustment: ForecastTemperatureCanaryDecisionV1;
}

// expose only bounded startup selection metadata
export interface ApiForecastAdjustmentRuntime {
  readonly actionSha256?: string;
  readonly activationMode: "maintenance_qualified" | "qualified" | "wind_canary" | null;
  readonly activeBundle: string | null;
  readonly authorizationSha256: string | null;
  readonly candidateArtifactSha256: string | null;
  readonly enabledMetrics: readonly ForecastAdjustmentMetric[];
  readonly evaluationReportSha256: string | null;
  readonly expiresAt: string | null;
  readonly fullMemberRootSha256?: string;
  readonly loadedAt: string;
  readonly qualificationReceiptSha256: string | null;
  readonly reasonCode: LoadedApiForecastAdjustmentRuntime["reasonCode"];
  readonly state: LoadedApiForecastAdjustmentRuntime["state"];
  readonly policyReportSha256?: string;
  readonly transferReportSha256: string | null;
}

// expose bounded independent temperature-canary state
export interface ApiForecastTemperatureAdjustmentRuntime {
  readonly actionSha256: string | null;
  readonly activationMode: "maintenance_qualified" | "temperature_canary" | null;
  readonly activeBundle: string | null;
  readonly authorizationSha256: string | null;
  readonly expiresAt: string | null;
  readonly fullMemberRootSha256: string | null;
  readonly loadedAt: string;
  readonly policyReportSha256: string | null;
  readonly reasonCode: LoadedForecastAdjustmentTemperatureCanaryRuntime["reasonCode"];
  readonly source: null | {
    readonly adaptiveReady: boolean;
    readonly firstReceivedAt: string;
    readonly hourCount: number;
    readonly latestRunInitializedAt: string;
    readonly stateReason: string | null;
    readonly stateStatus: EcmwfTemperatureCanaryStatus["stateStatus"];
  };
  readonly state: LoadedForecastAdjustmentTemperatureCanaryRuntime["state"];
}

export interface ApiPropertySensorSnapshot {
  readonly channel: number | null;
  readonly key: string;
  readonly model: string;
  readonly readings: Readonly<Record<string, number>>;
}

export interface ApiTideRecord {
  readonly eventType: "high" | "low" | null;
  readonly kind: "observation" | "prediction";
  readonly source: {
    readonly attribution: { readonly label: string; readonly url: string };
    readonly providerKey: string;
    readonly stationName: string;
    readonly stationSlug: string;
  };
  readonly validAt: string;
  readonly waterLevelM: number;
}

export interface ApiTrendPoint {
  readonly metrics: {
    readonly apparentTemperatureC: number | null;
    readonly precipitationMm: number | null;
    readonly pressureHpa: number | null;
    readonly relativeHumidityPercent: number | null;
    readonly temperatureC: number | null;
    readonly temperatureMaximumC: number | null;
    readonly temperatureMinimumC: number | null;
    readonly windDirectionDegrees: number | null;
    readonly windGustMps: number | null;
    readonly windSpeedMps: number | null;
  };
  readonly validAt: string;
}

export interface ApiDailyPrecipitation {
  readonly accumulationMm: number;
  readonly source: {
    readonly sourceId: string;
    readonly stationSlug: string;
  };
  readonly validThrough: string;
}

export interface ApiOptions {
  readonly forecastAdjustment?: {
    readonly apply?: (
      runtime: LoadedApiForecastAdjustmentRuntime,
      input: ApplyForecastAdjustmentInputV1,
    ) => ApiForecastAdjustmentDecision;
    readonly loadedAt: string;
    readonly runtime: LoadedApiForecastAdjustmentRuntime;
  };
  readonly temperatureAdjustment?: {
    readonly apply?: (
      runtime: LoadedForecastAdjustmentTemperatureCanaryRuntime,
      input: ApplyTemperatureCanaryInputV1,
    ) => ForecastTemperatureCanaryDecisionV1;
    readonly loadedAt: string;
    readonly runtime: LoadedForecastAdjustmentTemperatureCanaryRuntime;
  };
  readonly rainAdjustment?: ApiRainAdjustmentStartupSnapshot;
  readonly logDiagnostic?: (diagnostic: ApiDiagnostic) => void;
  readonly now?: () => Date;
  readonly version?: string;
}

export type ForecastDays = 1 | 5 | 10;

export interface ApiDiagnostic {
  readonly errorCode: string | null;
  readonly errorName: string;
  readonly event: "api_request_failed" | "temperature_canary_fallback" | "rain_adjustment_fallback";
  readonly method: string;
  readonly operation?: TemperatureCanaryOperation;
  readonly status: number;
}

// identify bounded optional failure boundaries
type TemperatureCanaryOperation = "sidecar_read" | "status_read" | "inference";

// collect failures without changing public serving
type TemperatureCanaryFailureReporter = (
  operation: TemperatureCanaryOperation,
  error: unknown,
) => void;

export interface ApiServerOptions {
  readonly logDiagnostic?: (diagnostic: ApiDiagnostic) => void;
}

export interface DatabaseStoreOptions {
  readonly migrationAuthorization?: MigrationReadinessAuthorization | null;
  readonly migrationDirectory: string;
  readonly release?: string;
}

interface MutableStation {
  readonly kind: StationKind;
  readonly latitude: number;
  readonly longitude: number;
  readonly name: string;
  readonly slug: string;
  readonly sources: ApiSource[];
}

interface MutableSite {
  readonly latitude: number;
  readonly longitude: number;
  readonly name: string;
  readonly slug: string;
  readonly stations: Map<string, MutableStation>;
  readonly timezone: string;
}

interface ParsedHistoryQuery {
  readonly query: HistoryQuery;
  readonly requestedLimit: number;
}

interface SourceDetails {
  readonly attribution: ApiSource["attribution"];
  readonly key: string;
  readonly providerKey: string;
}

// describe one exact read route
type Route =
  | Readonly<{ kind: "forecastObservationInput" }>
  | Readonly<{ kind: "health" }>
  | Readonly<{ kind: "sites" }>
  | Readonly<{ kind: "current"; siteSlug: string }>
  | Readonly<{ kind: "dailyPrecipitation"; siteSlug: string }>
  | Readonly<{ kind: "forecast"; siteSlug: string }>
  | Readonly<{ kind: "rainCollection"; siteSlug: string }>
  | Readonly<{ kind: "history"; siteSlug: string }>
  | Readonly<{ kind: "tides"; siteSlug: string }>
  | Readonly<{ kind: "trends"; siteSlug: string }>;

type TrendRange = "24h" | "7d" | "30d";

const HISTORY_DEFAULT_LIMIT = 100;
const HISTORY_MAX_LIMIT = 250;
const WORKER_FRESH_SECONDS = 1_800;
const TIMEZONE_OFFSET_SAMPLE_MS = 6 * 60 * 60 * 1_000;
const TIMEZONE_OFFSET_WINDOW_MS = 48 * 60 * 60 * 1_000;
const DIAGNOSTIC_TOKEN = /^[a-zA-Z0-9_:-]{1,64}$/u;
const JSON_HEADERS = {
  "cache-control": "no-store",
  "content-type": "application/json; charset=utf-8",
} as const;

// represent an intentional client-visible failure
class HttpError extends Error {
  readonly code: string;
  readonly status: number;

  // retain a stable response code
  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "HttpError";
    this.code = code;
    this.status = status;
  }
}

// read the production release contract
export function readApiRelease(
  environment: Readonly<Record<string, string | undefined>> = process.env,
): string {
  const release = environment.WEATHER_RELEASE ?? "development";

  // reject unsafe public release labels
  if (release.trim().length === 0 || release.length > 128) {
    throw new RangeError("WEATHER_RELEASE must be non-empty and bounded");
  }

  return release;
}

// write one redacted diagnostic event
export function writeApiDiagnostic(diagnostic: ApiDiagnostic): void {
  process.stderr.write(`${JSON.stringify(diagnostic)}\n`);
}

// bind the API to frozen database reads
export function createDatabaseWeatherReadStore(
  pool: DatabasePool,
  options: DatabaseStoreOptions,
): WeatherReadStore {
  return {
    // read only bounded model output, never private provider receipts
    async getRainAdjustmentRun(siteSlug, asOf) {
      return await readRainAdjustmentRun(pool, siteSlug, asOf);
    },
    // read the aggregate-only view without access to raw provider bodies
    async getRainCollectionStatus() {
      return await readRainCollectionStatus(pool);
    },
    // sum the nearest physical gauge for one local day
    async getDailyPrecipitation(siteSlug, from, to) {
      return await getDailyPrecipitation(pool, { from, siteSlug, to });
    },
    // read the newest rows
    async getCurrent(siteSlug, query) {
      return await getCurrentWeather(pool, siteSlug, query);
    },
    // read the newest forecast product
    async getForecast(siteSlug, asOf, hours, productSelection) {
      return await getWeatherForecast(pool, {
        asOf,
        hours,
        // omit the selector to preserve the repository compatibility default
        ...(productSelection === undefined ? {} : { productSelection }),
        siteSlug,
      });
    },
    // read one fixed-site compact observation input
    async getForecastObservationInput(asOf) {
      return await listForecastObservationInputs(pool, asOf);
    },
    // read one complete retained pressure vintage
    async getForecastPressureContext(siteSlug, asOf) {
      return await readForecastPressureContext(pool, { asOf, siteSlug });
    },
    // read one isolated current ECMWF canary run
    async getTemperatureCanarySidecar(siteSlug, asOf, from, to) {
      return await getEcmwfTemperatureCanarySidecar(pool, {
        asOf,
        from,
        siteSlug,
        to,
      });
    },
    // read bounded canary collector status
    async getTemperatureCanaryStatus(siteSlug, asOf) {
      return await getEcmwfTemperatureCanaryStatus(pool, { asOf, siteSlug });
    },
    // classify readiness without raw errors
    async getHealth() {
      try {
        await pool.query("SELECT 1");
      } catch {
        return unavailableHealth();
      }

      let migration: HealthSnapshot["migration"];

      try {
        const readiness = await verifyMigrationReadiness(
          pool,
          options.migrationDirectory,
          {
            authorization: options.migrationAuthorization ?? null,
            release: options.release ?? "development",
          },
        );
        migration = { status: "current", version: readiness.version };
      } catch {
        migration = { status: "outdated", version: null };
      }

      let workerLastLoopAt: string | null = null;

      try {
        const heartbeat = await getLatestWorkerHeartbeat(pool);
        workerLastLoopAt =
          heartbeat === null ? null : toIsoInstant(heartbeat.lastLoopAt);
      } catch {
        workerLastLoopAt = null;
      }

      return { database: "ready", migration, workerLastLoopAt };
    },
    // read a bounded page
    async listHistory(query) {
      return await listWeatherHistory(pool, query);
    },
    // read active metadata
    async listSites() {
      return await listActiveSites(pool);
    },
    // read bounded tide observations and predictions
    async listTides(siteSlug, from, to) {
      return await listTideRecords(pool, { from, siteSlug, to });
    },
    // read normalized chart buckets
    async listTrends(siteSlug, from, to) {
      return await listWeatherTrends(pool, {
        from,
        siteSlug,
        to,
      });
    },
  };
}

// create the fetch-compatible API boundary
export function createWeatherApi(
  store: WeatherReadStore,
  options: ApiOptions = {},
): (request: Request) => Promise<Response> {
  const logDiagnostic = options.logDiagnostic;
  const now = options.now ?? currentDate;
  const version = options.version ?? "development";
  const forecastAdjustment = options.forecastAdjustment ?? {
    loadedAt: now().toISOString(),
    runtime: disabledForecastAdjustmentRuntime(),
  };
  const applyAdjustment =
    forecastAdjustment.apply ?? applyForecastAdjustment;
  const temperatureAdjustment = options.temperatureAdjustment ?? {
    loadedAt: now().toISOString(),
    runtime: disabledForecastTemperatureAdjustmentRuntime(),
  };
  const applyTemperatureAdjustment =
    temperatureAdjustment.apply ?? applyForecastAdjustmentTemperatureCanary;
  const rainAdjustment = options.rainAdjustment;

  // route one request
  return async function handleWeatherRequest(request: Request): Promise<Response> {
    const temperatureFailures = new Map<TemperatureCanaryOperation, unknown>();
    let rainReadFailed = false;
    // record bounded rain degradation without inspecting private error details
    const reportRainFailure = (): void => { rainReadFailed = true; };
    // retain only the first failure at each request boundary
    const reportTemperatureFailure: TemperatureCanaryFailureReporter = (operation, error) => {
      // avoid unbounded per-row diagnostics or unused error retention
      if (logDiagnostic !== undefined && !temperatureFailures.has(operation)) {
        temperatureFailures.set(operation, error);
      }
    };
    let response: Response;

    try {
      const url = new URL(request.url);
      const route = matchRoute(url.pathname);
      const generatedAt = now().toISOString();
      const adjustmentRuntime = projectForecastAdjustmentRuntime(
        forecastAdjustment.runtime,
        forecastAdjustment.loadedAt,
        generatedAt,
      );
      const temperatureAdjustmentRuntime = projectForecastTemperatureAdjustmentRuntime(
        temperatureAdjustment.runtime,
        temperatureAdjustment.loadedAt,
        generatedAt,
        null,
      );

      // keep every exact endpoint read-only
      if (request.method !== "GET" && request.method !== "HEAD") {
        response = jsonResponse(
          { error: { code: "method_not_allowed", message: "Only GET and HEAD are supported" } },
          405,
          { allow: "GET, HEAD" },
        );
      } else {
        response = await handleReadRoute(
          store,
          route,
          url,
          generatedAt,
          version,
          forecastAdjustment.runtime,
          adjustmentRuntime,
          applyAdjustment,
          temperatureAdjustment.runtime,
          temperatureAdjustmentRuntime,
          applyTemperatureAdjustment,
          reportTemperatureFailure,
          reportRainFailure,
          rainAdjustment,
        );
      }
    } catch (error) {
      response = errorResponse(error);

      // record only non-public failure details
      if (!(error instanceof HttpError) && logDiagnostic !== undefined) {
        emitApiDiagnostic(
          logDiagnostic,
          createApiDiagnostic(error, request.method, response.status),
        );
      }
    }

    // report optional degradation with the actual public response status
    if (logDiagnostic !== undefined) {
      // distinguish broken rain serving from an ordinary absent source
      if (rainReadFailed) {
        emitApiDiagnostic(logDiagnostic, {
          ...createApiDiagnostic(null, request.method, response.status),
          errorCode: "rain_sidecar_read_failed",
          event: "rain_adjustment_fallback",
          operation: "sidecar_read",
        });
      }
      // emit at most one event per optional operation
      for (const [operation, error] of temperatureFailures) {
        let diagnostic: ApiDiagnostic;
        // sanitize without trusting error property access
        try {
          diagnostic = createApiDiagnostic(error, request.method, response.status);
        } catch {
          // contain hostile error property getters
          diagnostic = createApiDiagnostic(null, request.method, response.status);
        }
        emitApiDiagnostic(logDiagnostic, {
          ...diagnostic,
          event: "temperature_canary_fallback",
          operation,
        });
      }
    }

    return request.method === "HEAD" ? withoutBody(response) : response;
  };
}

// read the current wall clock
function currentDate(): Date {
  return new Date();
}

// provide a fail-raw default when no startup runtime is supplied
function disabledForecastAdjustmentRuntime(): LoadedForecastAdjustmentRuntimeV1 {
  return {
    bundle: null,
    reasonCode: "registry_inactive",
    state: "disabled",
  };
}

// provide an independently inactive temperature default
function disabledForecastTemperatureAdjustmentRuntime(): LoadedForecastAdjustmentTemperatureCanaryRuntime {
  return {
    bundle: null,
    reasonCode: "registry_inactive",
    state: "disabled",
  };
}

// project the cached runtime through a strict response allowlist
function projectForecastAdjustmentRuntime(
  runtime: LoadedApiForecastAdjustmentRuntime,
  loadedAtInput: string,
  evaluatedAtInput: string,
): ApiForecastAdjustmentRuntime {
  const loadedAt = validateUtcInstant(loadedAtInput, "forecastAdjustment.loadedAt");
  const evaluatedAt = validateUtcInstant(
    evaluatedAtInput,
    "forecastAdjustment.evaluatedAt",
  );

  // redact all model content while the runtime is inactive
  if (runtime.state === "disabled") {
    return {
      activationMode: null,
      activeBundle: null,
      authorizationSha256: null,
      candidateArtifactSha256: null,
      enabledMetrics: [],
      evaluationReportSha256: null,
      expiresAt: null,
      loadedAt,
      qualificationReceiptSha256: null,
      reasonCode: runtime.reasonCode,
      state: runtime.state,
      transferReportSha256: null,
    };
  }

  const canary = "artifactKind" in runtime.bundle;
  const maintenance = "maintenanceBundleSha256" in runtime.bundle;
  const expiresAt = canary && "expiresAt" in runtime.bundle.authorization
    ? runtime.bundle.authorization.expiresAt
    : null;
  const evaluatedAtMs = Date.parse(evaluatedAt);

  // check only the cached authorization clock after startup validation
  if (
    canary &&
    (evaluatedAtMs < Date.parse(runtime.bundle.authorization.activatedAt) ||
      (expiresAt !== null && evaluatedAtMs >= Date.parse(expiresAt)))
  ) {
    return {
      activationMode: null,
      activeBundle: null,
      authorizationSha256: null,
      candidateArtifactSha256: null,
      enabledMetrics: [],
      evaluationReportSha256: null,
      expiresAt: null,
      loadedAt,
      qualificationReceiptSha256: null,
      reasonCode: "canary_expired",
      state: "disabled",
      transferReportSha256: null,
    };
  }

  const enabledMetrics = [...new Set(runtime.bundle.candidate.enabledMetricBands.map(
    // project each enabled pair to its public metric identity
    (metricBand) => metricBand.metric,
  ))] as ForecastAdjustmentMetric[];

  return {
    ...(maintenance ? {
      actionSha256: runtime.bundle.maintenanceAuthority.actionSha256,
      fullMemberRootSha256: runtime.bundle.maintenanceAuthority.fullMemberRootSha256,
      policyReportSha256: runtime.bundle.maintenanceAuthority.policyReportSha256,
    } : {}),
    activationMode: canary ? "wind_canary" : maintenance ? "maintenance_qualified" : "qualified",
    activeBundle: runtime.bundle.bundleSha256,
    authorizationSha256: canary
      ? runtime.bundle.authorization.authorizationSha256
      : null,
    candidateArtifactSha256:
      runtime.bundle.candidate.candidateArtifactSha256,
    enabledMetrics,
    evaluationReportSha256: canary || maintenance
      ? null
      : runtime.bundle.evaluationReport.evaluationReportSha256,
    expiresAt,
    loadedAt,
    qualificationReceiptSha256: canary || maintenance
      ? null
      : runtime.bundle.qualificationReceipt.qualificationReceiptSha256,
    reasonCode: null,
    state: runtime.state,
    transferReportSha256: canary
      ? runtime.bundle.transferReport.transferReportSha256
      : null,
  };
}

// project independent canary and bounded collector status
function projectForecastTemperatureAdjustmentRuntime(
  runtime: LoadedForecastAdjustmentTemperatureCanaryRuntime,
  loadedAtInput: string,
  evaluatedAtInput: string,
  source: EcmwfTemperatureCanaryStatus | null,
): ApiForecastTemperatureAdjustmentRuntime {
  const loadedAt = validateUtcInstant(
    loadedAtInput,
    "temperatureAdjustment.loadedAt",
  );
  const evaluatedAt = validateUtcInstant(
    evaluatedAtInput,
    "temperatureAdjustment.evaluatedAt",
  );

  // redact artifact content while loader state is disabled
  if (runtime.state === "disabled") {
    return {
      actionSha256: null,
      activationMode: null,
      activeBundle: null,
      authorizationSha256: null,
      expiresAt: null,
      fullMemberRootSha256: null,
      loadedAt,
      policyReportSha256: null,
      reasonCode: runtime.reasonCode,
      source: projectTemperatureCanarySourceStatus(source),
      state: "disabled",
    };
  }

  const maintenance = "maintenancePackage" in runtime.bundle;
  const expiresAt = maintenance ? null : runtime.bundle.authorization.expiresAt;
  const evaluatedAtMs = Date.parse(evaluatedAt);

  // check only the cached authorization clock after startup validation
  if (!maintenance && (evaluatedAtMs < Date.parse(runtime.bundle.authorization.activatedAt) ||
    (expiresAt !== null && evaluatedAtMs >= Date.parse(expiresAt)))) {
    return {
      actionSha256: null,
      activationMode: null,
      activeBundle: null,
      authorizationSha256: null,
      expiresAt: null,
      fullMemberRootSha256: null,
      loadedAt,
      policyReportSha256: null,
      reasonCode: "canary_expired",
      source: projectTemperatureCanarySourceStatus(source),
      state: "disabled",
    };
  }

  return {
    actionSha256: maintenance ? runtime.bundle.maintenanceAuthority.actionSha256 : null,
    activationMode: maintenance ? "maintenance_qualified" : "temperature_canary",
    activeBundle: runtime.bundle.bundleSha256,
    authorizationSha256: maintenance ? null : runtime.bundle.authorization.authorizationSha256,
    expiresAt,
    fullMemberRootSha256: maintenance
      ? runtime.bundle.maintenanceAuthority.fullMemberRootSha256
      : null,
    loadedAt,
    policyReportSha256: maintenance ? runtime.bundle.maintenanceAuthority.policyReportSha256 : null,
    reasonCode: null,
    source: projectTemperatureCanarySourceStatus(source),
    state: "active",
  };
}

// retain only bounded collector monitoring fields
function projectTemperatureCanarySourceStatus(
  source: EcmwfTemperatureCanaryStatus | null,
): ApiForecastTemperatureAdjustmentRuntime["source"] {
  // preserve an empty collector
  if (source === null) {
    return null;
  }

  return {
    adaptiveReady: source.stateStatus === "supported",
    firstReceivedAt: validateUtcInstant(
      source.firstReceivedAt,
      "temperatureAdjustment.source.firstReceivedAt",
    ),
    hourCount: source.hourCount,
    latestRunInitializedAt: validateUtcInstant(
      source.latestRunInitializedAt,
      "temperatureAdjustment.source.latestRunInitializedAt",
    ),
    stateReason: source.stateReason,
    stateStatus: source.stateStatus,
  };
}

interface SiteDateTimeParts {
  readonly day: number;
  readonly hour: number;
  readonly minute: number;
  readonly month: number;
  readonly year: number;
}

// calculate one exact site-local midnight-to-midnight forecast window
export function siteForecastDayWindow(
  value: string,
  timezone: string,
  days: ForecastDays = 1,
): Readonly<{ asOf: string; hours: number }> {
  const instant = new Date(value);

  // reject invalid reference instants
  if (!Number.isFinite(instant.getTime())) {
    throw new RangeError("forecast reference must be a valid instant");
  }

  // reject unreviewed forecast lengths
  if (![1, 5, 10].includes(days)) {
    throw new RangeError("forecast days must be 1, 5, or 10");
  }

  const today = formatSiteDateTimeParts(instant, timezone);
  const nextDate = new Date(Date.UTC(today.year, today.month - 1, today.day + days));
  const endDate = {
    day: nextDate.getUTCDate(),
    hour: 0,
    minute: 0,
    month: nextDate.getUTCMonth() + 1,
    year: nextDate.getUTCFullYear(),
  };
  const start = siteWallClockEpoch({ ...today, hour: 0, minute: 0 }, timezone);
  const end = siteWallClockEpoch(endDate, timezone);
  const hours = (end - start) / 3_600_000;

  // require a reviewed range and realistic civil-hour duration
  if (
    !Number.isSafeInteger(hours) ||
    hours < days * 24 - 1 ||
    hours > days * 24 + 1
  ) {
    throw new RangeError("site forecast range has an unsupported duration");
  }

  return { asOf: new Date(start).toISOString(), hours };
}

// calculate the widget's anchor midnight through next-date 07:00 window
export function siteOvernightForecastWindow(
  value: string,
  timezone: string,
): Readonly<{ asOf: string; hours: number }> {
  const instant = new Date(value);

  // reject invalid reference instants
  if (!Number.isFinite(instant.getTime())) {
    throw new RangeError("forecast reference must be a valid instant");
  }

  const current = formatSiteDateTimeParts(instant, timezone);
  const currentDate = new Date(Date.UTC(
    current.year,
    current.month - 1,
    current.day,
  ));
  // keep the prior date active until the exact local 07:00 boundary
  const anchorDate = new Date(
    current.hour < 7
      ? currentDate.getTime() - 86_400_000
      : currentDate.getTime(),
  );
  const nextDate = new Date(anchorDate.getTime() + 86_400_000);
  const start = siteWallClockEpoch({
    day: anchorDate.getUTCDate(),
    hour: 0,
    minute: 0,
    month: anchorDate.getUTCMonth() + 1,
    year: anchorDate.getUTCFullYear(),
  }, timezone);
  const end = siteWallClockEpoch({
    day: nextDate.getUTCDate(),
    hour: 7,
    minute: 0,
    month: nextDate.getUTCMonth() + 1,
    year: nextDate.getUTCFullYear(),
  }, timezone);
  const hours = (end - start) / 3_600_000;

  // accept only the three real-hour capacities spanning a DST transition
  if (!Number.isSafeInteger(hours) || hours < 30 || hours > 32) {
    throw new RangeError("site overnight forecast range has an unsupported duration");
  }

  return { asOf: new Date(start).toISOString(), hours };
}

// resolve one local wall clock without assuming a fixed UTC offset
function siteWallClockEpoch(
  requested: SiteDateTimeParts,
  timezone: string,
): number {
  const requestedEpoch = siteDateTimeEpoch(requested);
  const matches = new Set<number>();

  // sample every plausible offset around the calendar date
  for (
    let delta = -TIMEZONE_OFFSET_WINDOW_MS;
    delta <= TIMEZONE_OFFSET_WINDOW_MS;
    delta += TIMEZONE_OFFSET_SAMPLE_MS
  ) {
    const sampleEpoch = requestedEpoch + delta;
    const represented = formatSiteDateTimeParts(new Date(sampleEpoch), timezone);
    const offset = siteDateTimeEpoch(represented) - sampleEpoch;
    const candidateEpoch = requestedEpoch - offset;
    const candidate = formatSiteDateTimeParts(new Date(candidateEpoch), timezone);

    // retain exact wall-clock matches
    if (siteDateTimeEquals(candidate, requested)) {
      matches.add(candidateEpoch);
    }
  }

  // require one unambiguous wall clock
  if (matches.size !== 1) {
    throw new RangeError("site forecast wall clock is not uniquely representable");
  }

  return [...matches][0]!;
}

// format one instant in the requested site timezone
function formatSiteDateTimeParts(
  instant: Date,
  timezone: string,
): SiteDateTimeParts {
  const values = new Map<string, string>();
  const formatter = new Intl.DateTimeFormat("en-CA", {
    day: "2-digit",
    hour: "2-digit",
    hourCycle: "h23",
    minute: "2-digit",
    month: "2-digit",
    timeZone: timezone,
    year: "numeric",
  });

  // collect named calendar fields
  for (const part of formatter.formatToParts(instant)) {
    // ignore locale punctuation
    if (part.type !== "literal") {
      values.set(part.type, part.value);
    }
  }

  return {
    day: requireSiteDateTimePart(values, "day"),
    hour: requireSiteDateTimePart(values, "hour"),
    minute: requireSiteDateTimePart(values, "minute"),
    month: requireSiteDateTimePart(values, "month"),
    year: requireSiteDateTimePart(values, "year"),
  };
}

// require one formatted site-calendar field
function requireSiteDateTimePart(
  values: ReadonlyMap<string, string>,
  name: string,
): number {
  const value = values.get(name);

  // fail closed on incomplete timezone formatting
  if (value === undefined) {
    throw new RangeError(`site timezone omitted ${name}`);
  }

  return Number(value);
}

// compare two minute-precision site wall clocks
function siteDateTimeEquals(
  left: SiteDateTimeParts,
  right: SiteDateTimeParts,
): boolean {
  return left.year === right.year &&
    left.month === right.month &&
    left.day === right.day &&
    left.hour === right.hour &&
    left.minute === right.minute;
}

// project site-calendar fields onto a UTC epoch
function siteDateTimeEpoch(parts: SiteDateTimeParts): number {
  return Date.UTC(
    parts.year,
    parts.month - 1,
    parts.day,
    parts.hour,
    parts.minute,
  );
}

// adapt the fetch handler to a Node HTTP server
export function createWeatherApiServer(
  handler: (request: Request) => Promise<Response>,
  options: ApiServerOptions = {},
): Server {
  // bridge one Node request
  return createServer(async (incoming, outgoing) => {
    try {
      const host = incoming.headers.host ?? "localhost";
      const request = new Request(
        `http://${host}${incoming.url ?? "/"}`,
        {
          headers: toFetchHeaders(incoming.headers),
          method: incoming.method ?? "GET",
        },
      );
      const response = await handler(request);
      outgoing.statusCode = response.status;

      // copy response headers
      response.headers.forEach((value, name) => {
        outgoing.setHeader(name, value);
      });

      outgoing.end(Buffer.from(await response.arrayBuffer()));
    } catch (error) {
      // record only bounded server boundary fields
      if (options.logDiagnostic !== undefined) {
        emitApiDiagnostic(
          options.logDiagnostic,
          createApiDiagnostic(error, incoming.method ?? "GET", 500),
        );
      }

      outgoing.statusCode = 500;
      outgoing.setHeader("content-type", JSON_HEADERS["content-type"]);
      outgoing.end(
        JSON.stringify({
          error: { code: "internal_error", message: "Unexpected server error" },
        }),
      );
    }
  });
}

// execute one matched read route
async function handleReadRoute(
  store: WeatherReadStore,
  route: Route,
  url: URL,
  generatedAt: string,
  version: string,
  forecastAdjustmentRuntime: LoadedApiForecastAdjustmentRuntime,
  adjustmentRuntime: ApiForecastAdjustmentRuntime,
  applyAdjustment: (
    runtime: LoadedApiForecastAdjustmentRuntime,
    input: ApplyForecastAdjustmentInputV1,
  ) => ApiForecastAdjustmentDecision,
  temperatureAdjustmentRuntime: LoadedForecastAdjustmentTemperatureCanaryRuntime,
  projectedTemperatureAdjustmentRuntime: ApiForecastTemperatureAdjustmentRuntime,
  applyTemperatureAdjustment: (
    runtime: LoadedForecastAdjustmentTemperatureCanaryRuntime,
    input: ApplyTemperatureCanaryInputV1,
  ) => ForecastTemperatureCanaryDecisionV1,
  reportTemperatureFailure: TemperatureCanaryFailureReporter,
  reportRainFailure: () => void,
  rainAdjustment: ApiRainAdjustmentStartupSnapshot | undefined,
): Promise<Response> {
  // keep the private fixed input outside site discovery and public routing
  if (route.kind === "forecastObservationInput") {
    rejectUnexpectedParameters(url.searchParams, new Set());
    const input = await store.getForecastObservationInput?.(generatedAt) ?? null;

    // fail closed when the fixed site or compact reader is unavailable
    if (input === null || input.siteSlug !== "ballydidean" ||
      input.timezone !== "America/Los_Angeles" || input.records.length > 20_000) {
      throw new HttpError(404, "not_found", "Endpoint not found");
    }

    const projectionFrom = toIsoInstant(input.projectionFrom);
    const queryFrom = toIsoInstant(input.queryFrom);
    const through = toIsoInstant(input.through);
    const expectedProjection = siteForecastDayWindow(
      generatedAt,
      input.timezone,
    ).asOf;
    const expectedQuery = new Date(Math.min(
      Date.parse(expectedProjection) - 3 * 3_600_000,
      Date.parse(generatedAt) - 24 * 3_600_000 - 20 * 60_000,
    )).toISOString();

    // bind the repository result to this request's one fixed geometry
    if (through !== generatedAt || projectionFrom !== expectedProjection ||
      queryFrom !== expectedQuery) {
      throw new Error("Forecast observation input geometry is invalid");
    }

    const body = JSON.stringify({
      contractVersion: "forecast-observation-input/v1",
      generatedAt,
      projectionFrom,
      queryFrom,
      records: input.records.map(
        // project only fields used by the edge algorithms
        (row) => ({
          id: row.id,
          metrics: {
            apparentTemperatureC: row.apparentTemperatureC,
            cloudCoverPercent: row.cloudCoverPercent,
            pm25MicrogramsPerCubicMeter: row.pm25MicrogramsPerCubicMeter,
            precipitationRateMmPerHour: row.precipitationRateMmPerHour,
            pressureHpa: row.pressureHpa,
            relativeHumidityPercent: row.relativeHumidityPercent,
            solarRadiationWm2: row.solarRadiationWm2,
            temperatureC: row.temperatureC,
            uvIndex: row.uvIndex,
            windDirectionDegrees: row.windDirectionDegrees,
            windGustMps: row.windGustMps,
            windSpeedMps: row.windSpeedMps,
          },
          provenance: {
            providerKey: row.providerKey,
            sourceId: row.sourceId,
            sourceKey: row.sourceKey,
            sourceKind: row.sourceKind,
            stationSlug: row.stationSlug,
          },
          receivedAt: toIsoInstant(row.lastReceivedAt),
          validAt: toIsoInstant(row.validAt),
        }),
      ),
      site: { slug: input.siteSlug, timezone: input.timezone },
      through,
    });

    // refuse an oversized private envelope before allocating a second body
    if (Buffer.byteLength(body, "utf8") > 4 * 1024 * 1024) {
      throw new RangeError("forecast observation input byte limit exceeded");
    }

    return new Response(body, { headers: JSON_HEADERS });
  }

  // serve liveness and readiness
  if (route.kind === "health") {
    rejectUnexpectedParameters(url.searchParams, new Set());
    const status = projectedTemperatureAdjustmentRuntime.state === "active"
      ? await readTemperatureCanaryStatusSafely(
          store,
          "ballydidean",
          generatedAt,
          reportTemperatureFailure,
        )
      : null;
    return await healthResponse(
      store,
      generatedAt,
      version,
      adjustmentRuntime,
      projectForecastTemperatureAdjustmentRuntime(
        temperatureAdjustmentRuntime,
        projectedTemperatureAdjustmentRuntime.loadedAt,
        generatedAt,
        status,
      ),
    );
  }

  // serve active site metadata
  if (route.kind === "sites") {
    rejectUnexpectedParameters(url.searchParams, new Set());
    return jsonResponse({ data: groupSites(await store.listSites()) });
  }

  const sites = groupSites(await store.listSites());
  const site = findSite(sites, route.siteSlug);

  // expose collection evidence without implying an active rain adjustment
  if (route.kind === "rainCollection") {
    rejectUnexpectedParameters(url.searchParams, new Set());
    // restrict the new capture contract to its one authorized site
    if (route.siteSlug !== "ballydidean" || store.getRainCollectionStatus === undefined) {
      throw new HttpError(404, "not_found", "Endpoint not found");
    }
    const rainRun = await readRainAdjustmentSafely(
      store,
      route.siteSlug,
      generatedAt,
      reportRainFailure,
      rainAdjustment,
    );
    return jsonResponse({
      data: { ...projectRainCollectionStatus(await store.getRainCollectionStatus()),
        modelEnabled: rainAdjustmentRuntime(
          rainRun,
          generatedAt,
          rainAdjustment,
        ).state === "active" },
      generatedAt,
      version,
    });
  }

  // serve selected current rows
  if (route.kind === "current") {
    rejectUnexpectedParameters(url.searchParams, new Set(["source", "station"]));
    const query = parsePublicQuery(
      // parse only the current query boundary
      () => parseCurrentQuery(url.searchParams, site),
    );
    const rows = await store.getCurrent(route.siteSlug, query);
    const records = mapCurrentWeatherRecords(rows, indexSources(site), generatedAt);
    return jsonResponse({ data: records, generatedAt, site });
  }

  // serve today's nearest physical rain accumulation
  if (route.kind === "dailyPrecipitation") {
    rejectUnexpectedParameters(url.searchParams, new Set());
    const window = siteForecastDayWindow(generatedAt, site.timezone);
    const row = await store.getDailyPrecipitation(
      route.siteSlug,
      window.asOf,
      generatedAt,
    );
    const data: ApiDailyPrecipitation | null = row === null
      ? null
      : {
        accumulationMm: row.accumulationMm,
        source: {
          sourceId: row.sourceId,
          stationSlug: row.stationSlug,
        },
        validThrough: toIsoInstant(row.validThrough),
      };
    return jsonResponse({ data, generatedAt, site });
  }

  // serve the latest normalized forecast product
  if (route.kind === "forecast") {
    rejectUnexpectedParameters(url.searchParams, new Set(["days", "window"]));
    const forecastWindow = parsePublicQuery(
      // parse only the reviewed daily or overnight horizon
      () => parseForecastRequestWindow(
        url.searchParams,
        generatedAt,
        site.timezone,
      ),
    );
    const rows = await store.getForecast(
      route.siteSlug,
      forecastWindow.asOf,
      forecastWindow.hours,
      forecastWindow.productSelection,
    );
    const pressureContextRows = await readForecastPressureContextSafely(
      store,
      route.siteSlug,
      forecastWindow.asOf,
    );
    const to = new Date(
      Date.parse(forecastWindow.asOf) + forecastWindow.hours * 3_600_000,
    ).toISOString();
    const [temperatureSidecar, temperatureStatus] =
      projectedTemperatureAdjustmentRuntime.state === "active"
        ? await Promise.all([
            readTemperatureCanarySidecarSafely(
              store,
              route.siteSlug,
              generatedAt,
              forecastWindow.asOf,
              to,
              reportTemperatureFailure,
            ),
            readTemperatureCanaryStatusSafely(
              store,
              route.siteSlug,
              generatedAt,
              reportTemperatureFailure,
            ),
          ])
        : [null, null];
    const rainRun = await readRainAdjustmentSafely(
      store,
      route.siteSlug,
      generatedAt,
      reportRainFailure,
      rainAdjustment,
    );
    const records = mapForecastWeatherRecords(
      rows,
      indexSources(site),
      generatedAt,
      forecastAdjustmentRuntime,
      applyAdjustment,
      temperatureAdjustmentRuntime,
      temperatureSidecar,
      applyTemperatureAdjustment,
      reportTemperatureFailure,
      rainRun,
      rainAdjustment,
    );
    return jsonResponse({
      adjustmentRuntime,
      rainAdjustmentRuntime: rainAdjustmentRuntime(
        rainRun,
        generatedAt,
        rainAdjustment,
      ),
      data: records,
      days: forecastWindow.days,
      generatedAt,
      pressureContext: mapForecastPressureContextSafely(
        pressureContextRows,
        indexSources(site),
        generatedAt,
      ),
      site,
      temperatureAdjustmentRuntime: projectForecastTemperatureAdjustmentRuntime(
        temperatureAdjustmentRuntime,
        projectedTemperatureAdjustmentRuntime.loadedAt,
        generatedAt,
        temperatureStatus,
      ),
    });
  }

  // serve daily calendar-year trend buckets
  if (route.kind === "trends") {
    rejectUnexpectedParameters(url.searchParams, new Set(["range"]));
    parsePublicQuery(
      // retain bounded compatibility with cached range clients
      () => parseLegacyTrendRange(url.searchParams),
    );
    const window = calendarTrendWindow(generatedAt, site.timezone);
    const rows = await store.listTrends(
      route.siteSlug,
      window.from,
      generatedAt,
    );
    return jsonResponse({
      data: mapTrendPoints(rows),
      generatedAt,
      site,
    });
  }

  // serve recent observations and the local tide forecast
  if (route.kind === "tides") {
    rejectUnexpectedParameters(url.searchParams, new Set(["from", "to"]));
    const range = parsePublicQuery(
      // parse only the bounded tide range
      () => parseTideRange(url.searchParams, generatedAt),
    );
    const rows = await store.listTides(route.siteSlug, range.from, range.to);
    return jsonResponse({
      data: mapTideRecords(rows),
      generatedAt,
      site,
    });
  }

  const parsed = parsePublicQuery(
    // parse only the history query boundary
    () => parseHistoryQuery(url.searchParams, route.siteSlug, site),
  );
  const rows = await store.listHistory(parsed.query);
  const hasMore = rows.length > parsed.requestedLimit;
  const visibleRows = hasMore ? rows.slice(0, parsed.requestedLimit) : rows;
  const records = mapWeatherRecords(visibleRows, indexSources(site), generatedAt);
  const last = records.at(-1);
  const nextCursor =
    hasMore && last !== undefined
      ? encodeCursor({ id: last.id, validAt: last.validAt })
      : null;
  return jsonResponse({
    data: records,
    generatedAt,
    page: { limit: parsed.requestedLimit, nextCursor },
    site,
  });
}

// render one safe health response
async function healthResponse(
  store: WeatherReadStore,
  generatedAt: string,
  version: string,
  adjustmentRuntime: ApiForecastAdjustmentRuntime,
  temperatureAdjustmentRuntime: ApiForecastTemperatureAdjustmentRuntime,
): Promise<Response> {
  let health: HealthSnapshot;

  try {
    health = await store.getHealth();
  } catch {
    health = unavailableHealth();
  }

  const ready =
    health.database === "ready" && health.migration.status === "current";
  const body = {
    data: {
      adjustmentRuntime,
      temperatureAdjustmentRuntime,
      database: health.database,
      live: true,
      migration: health.migration,
      ready,
      version,
      worker: {
        freshness: describeWorkerFreshness(
          health.workerLastLoopAt,
          generatedAt,
        ),
      },
    },
  };
  return jsonResponse(body, ready ? 200 : 503);
}

// classify a completely unavailable database
function unavailableHealth(): HealthSnapshot {
  return {
    database: "unavailable",
    migration: { status: "unavailable", version: null },
    workerLastLoopAt: null,
  };
}

// classify coarse worker loop freshness
function describeWorkerFreshness(
  lastLoopAt: string | null,
  generatedAt: string,
): "fresh" | "stale" | "unknown" {
  // preserve absent heartbeat state
  if (lastLoopAt === null) {
    return "unknown";
  }

  const ageSeconds = Math.floor(
    (Date.parse(generatedAt) - Date.parse(lastLoopAt)) / 1_000,
  );

  // fail closed on invalid or future heartbeats
  if (!Number.isFinite(ageSeconds) || ageSeconds < 0) {
    return "stale";
  }

  return ageSeconds <= WORKER_FRESH_SECONDS ? "fresh" : "stale";
}

// match only the frozen public surface
function matchRoute(pathname: string): Route {
  const segments = parsePathSegments(pathname);

  // match only the exact private fixed-input path
  if (pathname === "/internal/v1/forecast-observation-input") {
    return { kind: "forecastObservationInput" };
  }

  // match top-level reads
  if (segments.length === 3 && segments[0] === "api" && segments[1] === "v1") {
    // match active site discovery
    if (segments[2] === "sites") {
      return { kind: "sites" };
    }

    // match health
    if (segments[2] === "health") {
      return { kind: "health" };
    }
  }

  // match site-specific reads
  if (
    segments.length === 5 &&
    segments[0] === "api" &&
    segments[1] === "v1" &&
    segments[2] === "sites"
  ) {
    let siteSlug: string;

    try {
      siteSlug = validateStableKey(segments[3] ?? "", "siteSlug");
    } catch {
      // keep invalid path segments outside the public surface
      throw new HttpError(404, "not_found", "Endpoint not found");
    }

    const resource = segments[4];

    // match current rows
    if (resource === "current") {
      return { kind: "current", siteSlug };
    }

    // match daily precipitation
    if (resource === "daily-precipitation") {
      return { kind: "dailyPrecipitation", siteSlug };
    }

    // match history rows
    if (resource === "history") {
      return { kind: "history", siteSlug };
    }

    // match forecast rows
    if (resource === "forecast") {
      return { kind: "forecast", siteSlug };
    }

    // match aggregate evidence rather than private observations or model outputs
    if (resource === "rain-collection") {
      return { kind: "rainCollection", siteSlug };
    }

    // match trend buckets
    if (resource === "trends") {
      return { kind: "trends", siteSlug };
    }

    // match tide observations and predictions
    if (resource === "tides") {
      return { kind: "tides", siteSlug };
    }
  }

  throw new HttpError(404, "not_found", "Endpoint not found");
}

// collect active rows into a stable nested contract
function groupSites(rows: readonly ActiveSiteRow[]): readonly ApiSite[] {
  const sites = new Map<string, MutableSite>();

  // group every metadata row
  for (const row of rows) {
    let site = sites.get(row.siteSlug);

    // create the site once
    if (site === undefined) {
      site = {
        latitude: row.latitude,
        longitude: row.longitude,
        name: row.siteName,
        slug: row.siteSlug,
        stations: new Map(),
        timezone: row.timezone,
      };
      sites.set(row.siteSlug, site);
    }

    let station = site.stations.get(row.stationSlug);

    // create the station once
    if (station === undefined) {
      station = {
        kind: row.stationKind,
        latitude: row.stationLatitude,
        longitude: row.stationLongitude,
        name: row.stationName,
        slug: row.stationSlug,
        sources: [],
      };
      site.stations.set(row.stationSlug, station);
    }

    station.sources.push({
      attribution: {
        label: row.attributionLabel,
        url: row.attributionUrl,
      },
      id: row.sourceId,
      key: row.sourceKey,
      kind: row.sourceKind,
      providerKey: row.providerKey,
      providerName: row.providerName,
      provenanceLabel: sourceKindLabel(row.sourceKind),
    });
  }

  const result: ApiSite[] = [];

  // freeze map-backed stations into arrays
  for (const site of sites.values()) {
    result.push({
      latitude: site.latitude,
      longitude: site.longitude,
      name: site.name,
      slug: site.slug,
      stations: Array.from(site.stations.values()),
      timezone: site.timezone,
    });
  }

  return result;
}

// require the requested active site
function findSite(sites: readonly ApiSite[], siteSlug: string): ApiSite {
  const site = sites.find(
    // compare stable slugs
    (candidate) => candidate.slug === siteSlug,
  );

  // distinguish unknown sites from empty datasets
  if (site === undefined) {
    throw new HttpError(404, "site_not_found", `Unknown site: ${siteSlug}`);
  }

  return site;
}

// parse selected current filters
function parseCurrentQuery(
  parameters: URLSearchParams,
  site: ApiSite,
): CurrentQuery {
  const stationValue = getOptionalParameter(parameters, "station");
  const stationSlug =
    stationValue === undefined
      ? undefined
      : validateStableKey(stationValue, "station");
  const sourceId = parseOptionalSourceId(parameters, "source");
  validateSelection(site, stationSlug, sourceId);
  return {
    ...(sourceId === undefined ? {} : { sourceId }),
    ...(stationSlug === undefined ? {} : { stationSlug }),
  };
}

// require active station and source selections
function validateSelection(
  site: ApiSite,
  stationSlug?: string,
  sourceId?: string,
): void {
  const stations =
    stationSlug === undefined
      ? site.stations
      : site.stations.filter(
        // match one active station
        (station) => station.slug === stationSlug,
      );

  // reject unknown stations
  if (stationSlug !== undefined && stations.length === 0) {
    throw new HttpError(404, "station_not_found", `Unknown station: ${stationSlug}`);
  }

  // preserve an absent source filter
  if (sourceId === undefined) {
    return;
  }

  const sourceExists = stations.some(
    // inspect selected station sources
    (station) => station.sources.some(
      // compare stable source IDs
      (source) => source.id === sourceId,
    ),
  );

  // reject inactive or unrelated sources
  if (!sourceExists) {
    throw new HttpError(404, "source_not_found", `Unknown source: ${sourceId}`);
  }
}

// index source metadata for provenance enrichment
function indexSources(site: ApiSite): ReadonlyMap<string, SourceDetails> {
  const sources = new Map<string, SourceDetails>();

  // visit every station
  for (const station of site.stations) {
    // visit every source
    for (const source of station.sources) {
      sources.set(source.id, {
        attribution: source.attribution,
        key: source.key,
        providerKey: source.providerKey,
      });
    }
  }

  return sources;
}

// map storage rows into the public weather contract
function mapWeatherRecords(
  rows: readonly WeatherRecordRow[],
  sources: ReadonlyMap<string, SourceDetails>,
  generatedAt: string,
): readonly ApiWeatherRecord[] {
  const records: ApiWeatherRecord[] = [];

  // enrich each storage row
  for (const row of rows) {
    const source = sources.get(row.sourceId);

    // fail closed on inconsistent metadata
    if (source === undefined) {
      throw new Error(`Missing active metadata for source ${row.sourceId}`);
    }

    const validAt = toIsoInstant(row.validAt);
    records.push({
      freshness: describeFreshness(validAt, generatedAt),
      id: row.id,
      metadata: projectMetadata(row),
      metrics: {
        apparentTemperatureC: row.apparentTemperatureC,
        blackGlobeTemperatureC: row.blackGlobeTemperatureC,
        cloudCoverPercent: row.cloudCoverPercent,
        pm25MicrogramsPerCubicMeter: row.pm25MicrogramsPerCubicMeter,
        precipitationMm: row.precipitationMm,
        precipitationRateMmPerHour: row.precipitationRateMmPerHour,
        pressureHpa: row.pressureHpa,
        relativeHumidityPercent: row.relativeHumidityPercent,
        soilElectricalConductivityMicrosiemensPerCm:
          row.soilElectricalConductivityMicrosiemensPerCm,
        soilMoisturePercent: row.soilMoisturePercent,
        solarRadiationWm2: row.solarRadiationWm2,
        temperatureC: row.temperatureC,
        uvIndex: row.uvIndex,
        windDirectionDegrees: row.windDirectionDegrees,
        windGustMps: row.windGustMps,
        windSpeedMps: row.windSpeedMps,
        wetBulbGlobeTemperatureC: row.wetBulbGlobeTemperatureC,
      },
      productRunAt:
        row.productRunAt === null ? null : toIsoInstant(row.productRunAt),
      provenance: {
        attribution: source.attribution,
        label: sourceKindLabel(row.sourceKind),
        providerKey: source.providerKey,
        sourceId: row.sourceId,
        sourceKey: source.key,
        sourceKind: row.sourceKind,
        stationSlug: row.stationSlug,
      },
      receivedAt: toIsoInstant(row.lastReceivedAt),
      revisionCount: row.revisionCount,
      validAt,
    });
  }

  return records;
}

// add current-only pressure tendency
function mapCurrentWeatherRecords(
  rows: readonly CurrentWeatherRecordRow[],
  sources: ReadonlyMap<string, SourceDetails>,
  generatedAt: string,
): readonly ApiCurrentWeatherRecord[] {
  const records = mapWeatherRecords(rows, sources, generatedAt);

  return records.map(
    // preserve source-aligned repository order
    (record, index) => ({
      ...record,
      pressureChange3hHpa: rows[index]?.pressureChange3hHpa ?? null,
    }),
  );
}

// append one exact domain decision to every forecast row
function mapForecastWeatherRecords(
  rows: readonly WeatherRecordRow[],
  sources: ReadonlyMap<string, SourceDetails>,
  generatedAt: string,
  runtime: LoadedApiForecastAdjustmentRuntime,
  applyAdjustment: (
    runtime: LoadedApiForecastAdjustmentRuntime,
    input: ApplyForecastAdjustmentInputV1,
  ) => ApiForecastAdjustmentDecision,
  temperatureRuntime: LoadedForecastAdjustmentTemperatureCanaryRuntime,
  temperatureSidecar: EcmwfTemperatureCanarySidecar | null,
  applyTemperatureAdjustment: (
    runtime: LoadedForecastAdjustmentTemperatureCanaryRuntime,
    input: ApplyTemperatureCanaryInputV1,
  ) => ForecastTemperatureCanaryDecisionV1,
  reportTemperatureFailure: TemperatureCanaryFailureReporter,
  rainRun: RainAdjustmentRun | null,
  rainAdjustment: ApiRainAdjustmentStartupSnapshot | undefined,
): readonly ApiForecastWeatherRecord[] {
  const rawRecords = mapWeatherRecords(rows, sources, generatedAt);
  const temperatureHours = new Map(
    (temperatureSidecar?.hours ?? []).map((hour) => [hour.validAt, hour]),
  );

  // retain storage order and raw record bytes
  return rawRecords.map((record, index) => ({
    ...record,
    rainAdjustment: rainAdjustmentDecision(
      rows[index]!.upstreamModel === "best_match" ? rainRun : null,
      record.validAt, record.metrics.precipitationMm, generatedAt,
      rainAdjustment,
    ),
    adjustment: applyForecastAdjustmentSafely(
      runtime,
      rows[index]!,
      generatedAt,
      applyAdjustment,
    ),
    temperatureAdjustment: applyForecastTemperatureAdjustmentSafely(
      temperatureRuntime,
      rows[index]!,
      generatedAt,
      temperatureSidecar,
      temperatureHours.get(record.validAt) ?? null,
      applyTemperatureAdjustment,
      reportTemperatureFailure,
    ),
  }));
}

// contain malformed optional context projections
function mapForecastPressureContextSafely(
  rows: readonly WeatherRecordRow[],
  sources: ReadonlyMap<string, SourceDetails>,
  generatedAt: string,
): readonly ApiWeatherRecord[] {
  try {
    return mapWeatherRecords(rows, sources, generatedAt);
  } catch {
    return [];
  }
}

// keep optional pressure context from affecting forecast availability
async function readForecastPressureContextSafely(
  store: WeatherReadStore,
  siteSlug: string,
  asOf: string,
): Promise<readonly WeatherRecordRow[]> {
  // preserve compatibility with stores without context support
  if (store.getForecastPressureContext === undefined) {
    return [];
  }

  try {
    return await store.getForecastPressureContext(siteSlug, asOf);
  } catch {
    return [];
  }
}

// isolate rain storage failure from every ordinary forecast and other adjustment
async function readRainAdjustmentSafely(
  store: WeatherReadStore,
  siteSlug: string,
  asOf: string,
  reportFailure: () => void,
  rainAdjustment: ApiRainAdjustmentStartupSnapshot | undefined,
): Promise<RainAdjustmentRun | null> {
  // skip storage completely for explicit raw or invalid policy
  if (rainAdjustment?.runtime.state === "disabled") {
    return null;
  }
  try {
    const run = await store.getRainAdjustmentRun?.(siteSlug, asOf) ?? null;
    // report corrupt or unbound projections separately from normal missing data
    if (run !== null && !validRainAdjustmentRun(run, asOf)) {
      reportFailure();
      return null;
    }
    return run;
  } catch {
    reportFailure();
    return null;
  }
}

// contain every temperature model or projection failure
function applyForecastTemperatureAdjustmentSafely(
  runtime: LoadedForecastAdjustmentTemperatureCanaryRuntime,
  row: WeatherRecordRow,
  evaluatedAt: string,
  sidecar: EcmwfTemperatureCanarySidecar | null,
  hour: EcmwfTemperatureCanarySidecar["hours"][number] | null,
  applyTemperatureAdjustment: (
    runtime: LoadedForecastAdjustmentTemperatureCanaryRuntime,
    input: ApplyTemperatureCanaryInputV1,
  ) => ForecastTemperatureCanaryDecisionV1,
  reportTemperatureFailure: TemperatureCanaryFailureReporter,
): ForecastTemperatureCanaryDecisionV1 {
  const validAt = toIsoInstant(row.validAt);
  const sourceForecast =
    sidecar === null || hour === null || row.upstreamModel !== "best_match"
      ? null
      : {
          adapterVersion: sidecar.run.adapterVersion,
          dataset: "single_run" as const,
          firstReceivedAt: sidecar.run.firstReceivedAt,
          modelCycle: sidecar.run.modelCycle,
          modelLeadHours: hour.modelLeadHours,
          providerKey: "open-meteo" as const,
          providerResponseSha256: sidecar.run.providerResponseSha256,
          rawRelativeHumidityPercent: hour.rawRelativeHumidityPercent,
          rawTemperatureC: hour.rawTemperatureC,
          rawWindSpeedMps: hour.rawWindSpeedMps,
          runInitializedAt: sidecar.run.runInitializedAt,
          upstreamModel: sidecar.run.upstreamModel,
          validAt: hour.validAt,
        };

  // keep temperature failure independent from wind serving
  try {
    return applyTemperatureAdjustment(runtime, {
      evaluatedAt,
      rawBestMatchTemperatureC: row.temperatureC,
      recentErrorState: sidecar?.run.recentErrorState ?? null,
      sourceForecast,
      validAt,
    });
  } catch (error) {
    reportTemperatureFailure("inference", error);
    return applyForecastAdjustmentTemperatureCanary(
      {
        bundle: null,
        reasonCode: "bundle_invalid",
        state: "disabled",
      },
      {
        evaluatedAt,
        rawBestMatchTemperatureC: row.temperatureC,
        recentErrorState: null,
        sourceForecast: null,
        validAt,
      },
    );
  }
}

// read one sidecar without affecting raw forecast availability
async function readTemperatureCanarySidecarSafely(
  store: WeatherReadStore,
  siteSlug: string,
  asOf: string,
  from: string,
  to: string,
  reportTemperatureFailure: TemperatureCanaryFailureReporter,
): Promise<EcmwfTemperatureCanarySidecar | null> {
  // preserve compatibility with stores that predate the sidecar
  if (store.getTemperatureCanarySidecar === undefined) {
    return null;
  }

  try {
    return await store.getTemperatureCanarySidecar(siteSlug, asOf, from, to);
  } catch (error) {
    reportTemperatureFailure("sidecar_read", error);
    return null;
  }
}

// read bounded status without affecting health readiness
async function readTemperatureCanaryStatusSafely(
  store: WeatherReadStore,
  siteSlug: string,
  asOf: string,
  reportTemperatureFailure: TemperatureCanaryFailureReporter,
): Promise<EcmwfTemperatureCanaryStatus | null> {
  // preserve compatibility with stores that predate monitoring
  if (store.getTemperatureCanaryStatus === undefined) {
    return null;
  }

  try {
    return await store.getTemperatureCanaryStatus(siteSlug, asOf);
  } catch (error) {
    reportTemperatureFailure("status_read", error);
    return null;
  }
}

// contain every model or projection exception at the row boundary
function applyForecastAdjustmentSafely(
  runtime: LoadedApiForecastAdjustmentRuntime,
  row: WeatherRecordRow,
  evaluatedAt: string,
  applyAdjustment: (
    runtime: LoadedApiForecastAdjustmentRuntime,
    input: ApplyForecastAdjustmentInputV1,
  ) => ApiForecastAdjustmentDecision,
): ApiForecastAdjustmentDecision {
  // contain every adjustment failure
  try {
    return applyAdjustment(runtime, forecastAdjustmentInput(row, evaluatedAt));
  } catch {
    return createForecastAdjustmentFailRawDecision(
      "not_applicable",
      "adjustment_error",
    );
  }
}

// derive the immutable v4 retrieval identity from one selected row
function forecastAdjustmentInput(
  row: WeatherRecordRow,
  evaluatedAt: string,
): ApplyForecastAdjustmentInputV1 {
  // retain missing reference identity for fail-raw handling
  const referenceAt = row.productRunAt === null
    ? ""
    : toIsoInstant(row.productRunAt);
  const validAt = toIsoInstant(row.validAt);
  const targetLeadHours = Math.ceil(
    (Date.parse(validAt) - Date.parse(referenceAt)) / 3_600_000,
  );
  const dataset = row.providerMetadata?.dataset;

  return {
    evaluatedAt,
    metrics: canonicalWeatherMetrics(row),
    rawForecastProvenance: {
      adapterVersion: row.adapterVersion ?? "",
      cohort: "legacy_v4_retrieval_snapshot",
      contractEpoch: row.contractEpoch ?? "",
      dataset: typeof dataset === "string" ? dataset : "",
      referenceAt,
      referenceKind: "retrieval_snapshot",
      sourceConfigFingerprint: row.sourceConfigFingerprint ?? "",
      sourceKey: row.sourceKey,
      targetLeadHours,
      upstreamModel: row.upstreamModel ?? "",
      validAt,
    },
  };
}

// retain the complete canonical raw metric shape for model application
function canonicalWeatherMetrics(row: WeatherRecordRow): CanonicalWeatherMetrics {
  return {
    apparentTemperatureC: row.apparentTemperatureC,
    blackGlobeTemperatureC: row.blackGlobeTemperatureC,
    cloudCoverPercent: row.cloudCoverPercent,
    pm25MicrogramsPerCubicMeter: row.pm25MicrogramsPerCubicMeter,
    precipitationMm: row.precipitationMm,
    precipitationRateMmPerHour: row.precipitationRateMmPerHour,
    pressureHpa: row.pressureHpa,
    relativeHumidityPercent: row.relativeHumidityPercent,
    soilElectricalConductivityMicrosiemensPerCm:
      row.soilElectricalConductivityMicrosiemensPerCm,
    soilMoisturePercent: row.soilMoisturePercent,
    solarRadiationWm2: row.solarRadiationWm2,
    temperatureC: row.temperatureC,
    uvIndex: row.uvIndex,
    waterLevelM: row.waterLevelM,
    windDirectionDegrees: row.windDirectionDegrees,
    windGustMps: row.windGustMps,
    windSpeedMps: row.windSpeedMps,
    wetBulbGlobeTemperatureC: row.wetBulbGlobeTemperatureC,
  };
}

// map aggregate storage rows into the public chart contract
function mapTrendPoints(rows: readonly TrendPointRow[]): readonly ApiTrendPoint[] {
  return rows.map((row) => ({
    metrics: {
      apparentTemperatureC: row.apparentTemperatureC,
      precipitationMm: row.precipitationMm,
      pressureHpa: row.pressureHpa,
      relativeHumidityPercent: row.relativeHumidityPercent,
      temperatureC: row.temperatureC,
      temperatureMaximumC: row.temperatureMaximumC,
      temperatureMinimumC: row.temperatureMinimumC,
      windDirectionDegrees: row.windDirectionDegrees,
      windGustMps: row.windGustMps,
      windSpeedMps: row.windSpeedMps,
    },
    validAt: toIsoInstant(row.validAt),
  }));
}

// map tide storage rows into the public contract
function mapTideRecords(rows: readonly TideRecordRow[]): readonly ApiTideRecord[] {
  return rows.map((row) => ({
    eventType:
      row.predictionType === "H"
        ? "high"
        : row.predictionType === "L"
          ? "low"
          : null,
    kind: row.sourceKind === "tide_observation" ? "observation" : "prediction",
    source: {
      attribution: {
        label: row.attributionLabel,
        url: row.attributionUrl,
      },
      providerKey: row.providerKey,
      stationName: row.stationName,
      stationSlug: row.stationSlug,
    },
    validAt: toIsoInstant(row.validAt),
    waterLevelM: row.waterLevelM,
  }));
}

// parse one reviewed forecast horizon
function parseForecastDays(parameters: URLSearchParams): ForecastDays {
  const value = getOptionalParameter(parameters, "days") ?? "1";

  // accept only product-supported horizons
  if (value !== "1" && value !== "5" && value !== "10") {
    throw new RangeError("forecast days must be 1, 5, or 10");
  }

  return Number(value) as ForecastDays;
}

// resolve one strict daily or widget-overnight forecast query
function parseForecastRequestWindow(
  parameters: URLSearchParams,
  generatedAt: string,
  timezone: string,
): Readonly<{
  asOf: string;
  days: ForecastDays;
  hours: number;
  productSelection?: ForecastProductSelection;
}> {
  const requestedWindow = getOptionalParameter(parameters, "window");

  // reject a present empty selector instead of treating it as the default
  if (requestedWindow === undefined && parameters.has("window")) {
    throw new RangeError("forecast window must be overnight");
  }

  // preserve every existing parameter-free and days-based query
  if (requestedWindow === undefined) {
    const days = parseForecastDays(parameters);
    return { ...siteForecastDayWindow(generatedAt, timezone, days), days };
  }

  // reject unknown or ambiguous window selection
  if (requestedWindow !== "overnight" || parameters.has("days")) {
    throw new RangeError("forecast window must be overnight and omit days");
  }

  return {
    ...siteOvernightForecastWindow(generatedAt, timezone),
    days: 1,
    productSelection: "anchor-containing",
  };
}

// parse one bounded tide API range
function parseTideRange(
  parameters: URLSearchParams,
  generatedAt: string,
): Readonly<{ from: string; to: string }> {
  const fromValue = getOptionalParameter(parameters, "from");
  const toValue = getOptionalParameter(parameters, "to");
  const from = fromValue === undefined
    ? new Date(Date.parse(generatedAt) - 24 * 3_600_000).toISOString()
    : validateUtcInstant(fromValue, "from");
  const to = toValue === undefined
    ? new Date(Date.parse(generatedAt) + 10 * 86_400_000).toISOString()
    : validateUtcInstant(toValue, "to");

  // require an ordered range no longer than 31 days
  if (from >= to || Date.parse(to) - Date.parse(from) > 31 * 86_400_000) {
    throw new RangeError("tide range must be ordered and no longer than 31 days");
  }

  return { from, to };
}

// parse one legacy reviewed trend range
function parseLegacyTrendRange(parameters: URLSearchParams): TrendRange | null {
  const value = getOptionalParameter(parameters, "range");

  // accept the parameter-free calendar contract
  if (value === undefined) {
    return null;
  }

  // reject unbounded query windows
  if (value !== "24h" && value !== "7d" && value !== "30d") {
    throw new RangeError("trend range is unsupported");
  }

  return value;
}

// calculate the fixed local-calendar trend history
export function calendarTrendWindow(
  generatedAt: string,
  timezone: string,
): Readonly<{ from: string }> {
  const instant = new Date(generatedAt);

  // reject invalid reference instants
  if (!Number.isFinite(instant.getTime())) {
    throw new RangeError("trend reference must be a valid instant");
  }

  return {
    from: new Date(siteWallClockEpoch({ day: 1, hour: 0, minute: 0, month: 1, year: 2019 }, timezone)).toISOString(),
  };
}

// project only public metadata fields
function projectMetadata(row: WeatherRecordRow): ApiWeatherRecord["metadata"] {
  const device =
    row.deviceVendor === null &&
    row.deviceModel === null &&
    row.deviceSerial === null
      ? null
      : {
        model: row.deviceModel,
        serial: row.deviceSerial,
        vendor: row.deviceVendor,
      };
  const quality = projectQuality(row.qualityMetadata);
  const provider = projectProvider(row.providerMetadata);
  return {
    device,
    provider,
    quality,
    upstream: {
      model: row.upstreamModel,
      timezone: row.upstreamTimezone,
    },
  };
}

// project allowlisted quality metadata
function projectQuality(
  metadata: Readonly<Record<string, JsonValue>> | null,
): ApiWeatherRecord["metadata"]["quality"] {
  // preserve absent metadata
  if (metadata === null) {
    return null;
  }

  return {
    confidencePercent: boundedNumber(metadata.confidence_percent),
    flags: boundedStrings(metadata.flags),
    interpolation: boundedString(metadata.interpolation),
    status: boundedString(metadata.status),
  };
}

// project allowlisted provider metadata
function projectProvider(
  metadata: Readonly<Record<string, JsonValue>> | null,
): ApiWeatherRecord["metadata"]["provider"] {
  // preserve absent metadata
  if (metadata === null) {
    return null;
  }

  return {
    dataset: boundedString(metadata.dataset),
    elevationM: boundedNumber(metadata.elevation_m),
    gridCell: boundedString(metadata.grid_cell),
    propertySensors: boundedPropertySensors(metadata.property_sensors),
  };
}

// project bounded EcoWitt property sensor snapshots
function boundedPropertySensors(
  value: JsonValue | undefined,
): readonly ApiPropertySensorSnapshot[] | null {
  // require one sensor array
  if (!Array.isArray(value)) {
    return null;
  }

  const sensors: ApiPropertySensorSnapshot[] = [];

  // retain only bounded sensor objects
  for (const candidate of value.slice(0, 32)) {
    // omit malformed entries
    if (
      candidate === null ||
      Array.isArray(candidate) ||
      typeof candidate !== "object"
    ) {
      continue;
    }

    const key = boundedString(candidate.key);
    const model = boundedString(candidate.model);
    const channel = candidate.channel === null
      ? null
      : boundedNumber(candidate.channel);

    // require stable public sensor identity
    if (
      key === null ||
      model === null ||
      !/^[a-z0-9]+(?:-[a-z0-9]+)*$/u.test(key) ||
      (channel !== null && (!Number.isSafeInteger(channel) || channel < 1 || channel > 32))
    ) {
      continue;
    }

    sensors.push({
      channel,
      key,
      model,
      readings: boundedSensorReadings(candidate.readings),
    });
  }

  return sensors;
}

// project finite numeric readings only
function boundedSensorReadings(value: JsonValue | undefined): Readonly<Record<string, number>> {
  // discard malformed reading objects
  if (value === null || Array.isArray(value) || typeof value !== "object") {
    return {};
  }

  const readings: Record<string, number> = {};

  // retain bounded camel-case metric keys
  for (const [key, reading] of Object.entries(value).slice(0, 24)) {
    // omit unknown shapes and unbounded keys
    if (/^[a-z][A-Za-z0-9]{0,63}$/u.test(key) && typeof reading === "number" && Number.isFinite(reading)) {
      readings[key] = reading;
    }
  }

  return readings;
}

// retain one bounded public string
function boundedString(value: JsonValue | undefined): string | null {
  return typeof value === "string" ? value.slice(0, 256) : null;
}

// retain one finite public number
function boundedNumber(value: JsonValue | undefined): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

// retain a bounded public string list
function boundedStrings(value: JsonValue | undefined): readonly string[] | null {
  // require an array
  if (!Array.isArray(value)) {
    return null;
  }

  const strings: string[] = [];

  // retain only bounded string entries
  for (const entry of value.slice(0, 20)) {
    // omit non-string entries
    if (typeof entry === "string") {
      strings.push(entry.slice(0, 128));
    }
  }

  return strings;
}

// normalize database instants
function toIsoInstant(value: Date | string): string {
  const date = value instanceof Date ? value : new Date(value);

  // reject corrupt storage values
  if (Number.isNaN(date.getTime())) {
    throw new Error("database returned an invalid timestamp");
  }

  return date.toISOString();
}

// describe recency without implying one source type
function describeFreshness(
  validAt: string,
  generatedAt: string,
): ApiWeatherRecord["freshness"] {
  const ageSeconds = Math.max(
    0,
    Math.floor((Date.parse(generatedAt) - Date.parse(validAt)) / 1_000),
  );

  // mark values within two source cadences as fresh
  if (ageSeconds <= 1_800) {
    return { ageSeconds, label: "Weather value is current", status: "fresh" };
  }

  // call out a moderate delay explicitly
  if (ageSeconds <= 7_200) {
    return { ageSeconds, label: "Weather value is delayed", status: "delayed" };
  }

  return { ageSeconds, label: "Weather value may be stale", status: "stale" };
}

// parse and validate history filters
function parseHistoryQuery(
  parameters: URLSearchParams,
  siteSlug: string,
  site: ApiSite,
): ParsedHistoryQuery {
  rejectUnexpectedParameters(
    parameters,
    new Set(["cursor", "from", "limit", "source", "sourceKind", "station", "to"]),
  );
  const requestedLimit = parseLimit(parameters);
  const cursorValue = getOptionalParameter(parameters, "cursor");
  const fromValue = getOptionalParameter(parameters, "from");
  const sourceKindValue = getOptionalParameter(parameters, "sourceKind");
  const sourceId = parseOptionalSourceId(parameters, "source");
  const stationValue = getOptionalParameter(parameters, "station");
  const stationSlug =
    stationValue === undefined
      ? undefined
      : validateStableKey(stationValue, "station");
  const toValue = getOptionalParameter(parameters, "to");
  validateSelection(site, stationSlug, sourceId);
  const query: HistoryQuery = {
    limit: requestedLimit + 1,
    siteSlug,
  };

  // apply the cursor
  if (cursorValue !== undefined) {
    Object.assign(query, { cursor: decodeCursor(cursorValue) });
  }

  // apply the lower bound
  if (fromValue !== undefined) {
    Object.assign(query, { from: validateUtcInstant(fromValue, "from") });
  }

  // apply provenance filtering
  if (sourceKindValue !== undefined) {
    Object.assign(query, { sourceKind: parseSourceKind(sourceKindValue) });
  }

  // apply source filtering
  if (sourceId !== undefined) {
    Object.assign(query, { sourceId });
  }

  // apply station filtering
  if (stationSlug !== undefined) {
    Object.assign(query, { stationSlug });
  }

  // apply the upper bound
  if (toValue !== undefined) {
    Object.assign(query, { to: validateUtcInstant(toValue, "to") });
  }

  // reject reversed ranges before storage
  if (query.from !== undefined && query.to !== undefined && query.from >= query.to) {
    throw new HttpError(400, "invalid_query", "from must be earlier than to");
  }

  return { query, requestedLimit };
}

// enforce a bounded page size
function parseLimit(parameters: URLSearchParams): number {
  const value = getOptionalParameter(parameters, "limit");

  // use the product default
  if (value === undefined) {
    return HISTORY_DEFAULT_LIMIT;
  }

  // require a canonical positive integer
  if (!/^\d+$/u.test(value)) {
    throw new HttpError(400, "invalid_query", "limit must be an integer");
  }

  const limit = Number(value);

  // enforce the frozen public maximum
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > HISTORY_MAX_LIMIT) {
    throw new HttpError(
      400,
      "invalid_query",
      `limit must be between 1 and ${String(HISTORY_MAX_LIMIT)}`,
    );
  }

  return limit;
}

// validate an optional bigint identifier
function parseOptionalSourceId(
  parameters: URLSearchParams,
  name: string,
): string | undefined {
  const value = getOptionalParameter(parameters, name);

  // preserve an absent filter
  if (value === undefined) {
    return undefined;
  }

  // require a positive decimal identifier
  if (!/^[1-9]\d*$/u.test(value)) {
    throw new HttpError(400, "invalid_query", `${name} must be a positive integer`);
  }

  return value;
}

// reject duplicate query fields
function getOptionalParameter(
  parameters: URLSearchParams,
  name: string,
): string | undefined {
  const values = parameters.getAll(name);

  // reject ambiguous duplicates
  if (values.length > 1) {
    throw new HttpError(400, "invalid_query", `${name} may be provided once`);
  }

  const value = values[0];

  // treat an empty value as absent
  if (value === undefined || value.length === 0) {
    return undefined;
  }

  return value;
}

// reject unsupported filters
function rejectUnexpectedParameters(
  parameters: URLSearchParams,
  allowed: ReadonlySet<string>,
): void {
  // inspect every submitted field
  for (const name of parameters.keys()) {
    // fail closed on misspelled filters
    if (!allowed.has(name)) {
      throw new HttpError(400, "invalid_query", `Unsupported query parameter: ${name}`);
    }
  }
}

// encode a stable pagination cursor
function encodeCursor(cursor: Readonly<{ id: string; validAt: string }>): string {
  return Buffer.from(JSON.stringify(cursor), "utf8").toString("base64url");
}

// decode an opaque pagination cursor
function decodeCursor(value: string): Readonly<{ id: string; validAt: string }> {
  try {
    const parsed = JSON.parse(
      Buffer.from(value, "base64url").toString("utf8"),
    ) as unknown;

    // require the exact cursor fields
    if (
      typeof parsed !== "object" ||
      parsed === null ||
      Object.keys(parsed).length !== 2 ||
      !("id" in parsed) ||
      !("validAt" in parsed) ||
      typeof parsed.id !== "string" ||
      typeof parsed.validAt !== "string" ||
      !/^[1-9]\d*$/u.test(parsed.id)
    ) {
      throw new TypeError("invalid cursor fields");
    }

    return {
      id: parsed.id,
      validAt: validateUtcInstant(parsed.validAt, "cursor.validAt"),
    };
  } catch {
    throw new HttpError(400, "invalid_query", "cursor is invalid");
  }
}

// split one exact URL path
function parsePathSegments(pathname: string): readonly string[] {
  try {
    // reject non-canonical separators
    if (!pathname.startsWith("/") || pathname.endsWith("/") || pathname.includes("//")) {
      throw new HttpError(404, "not_found", "Endpoint not found");
    }

    const rawSegments = pathname.slice(1).split("/");
    const segments: string[] = [];

    // decode every exact segment
    for (const segment of rawSegments) {
      const decoded = decodeURIComponent(segment);

      // reject encoded separators
      if (decoded.length === 0 || decoded.includes("/")) {
        throw new HttpError(404, "not_found", "Endpoint not found");
      }

      segments.push(decoded);
    }

    return segments;
  } catch (error) {
    // preserve intentional route errors
    if (error instanceof HttpError) {
      throw error;
    }

    throw new HttpError(400, "invalid_path", "Path is not valid UTF-8");
  }
}

// serialize a successful JSON response
function jsonResponse(
  body: unknown,
  status = 200,
  headers: Readonly<Record<string, string>> = {},
): Response {
  return new Response(JSON.stringify(body), {
    headers: { ...JSON_HEADERS, ...headers },
    status,
  });
}

// strip one head response body
function withoutBody(response: Response): Response {
  return new Response(null, {
    headers: response.headers,
    status: response.status,
    statusText: response.statusText,
  });
}

// convert failures to bounded public errors
function errorResponse(error: unknown): Response {
  // preserve intentional HTTP errors
  if (error instanceof HttpError) {
    return jsonResponse(
      { error: { code: error.code, message: error.message } },
      error.status,
    );
  }

  return jsonResponse(
    { error: { code: "internal_error", message: "Unexpected server error" } },
    500,
  );
}

// classify only request parsing failures
function parsePublicQuery<Result>(read: () => Result): Result {
  try {
    return read();
  } catch (error) {
    // preserve intentional query responses
    if (error instanceof HttpError) {
      throw error;
    }

    // hide validation implementation details
    if (error instanceof RangeError || error instanceof TypeError) {
      throw new HttpError(400, "invalid_query", "Request query is invalid");
    }

    throw error;
  }
}

// build one allowlisted diagnostic event
function createApiDiagnostic(
  error: unknown,
  method: string,
  status: number,
): ApiDiagnostic {
  const errorName =
    error instanceof Error && DIAGNOSTIC_TOKEN.test(error.name)
      ? error.name
      : "UnknownError";
  const candidateCode =
    typeof error === "object" && error !== null && "code" in error
      ? error.code
      : undefined;
  const errorCode =
    typeof candidateCode === "string" && DIAGNOSTIC_TOKEN.test(candidateCode)
      ? candidateCode
      : null;
  const safeMethod = /^[A-Z]{1,16}$/u.test(method) ? method : "UNKNOWN";
  return {
    errorCode,
    errorName,
    event: "api_request_failed",
    method: safeMethod,
    status,
  };
}

// isolate diagnostic sink failures
function emitApiDiagnostic(
  sink: (diagnostic: ApiDiagnostic) => void,
  diagnostic: ApiDiagnostic,
): void {
  try {
    sink(diagnostic);
  } catch {
    // preserve the public response boundary
  }
}

// convert Node request headers to Fetch headers
function toFetchHeaders(incoming: IncomingHttpHeaders): Headers {
  const headers = new Headers();

  // copy every incoming header
  for (const [name, value] of Object.entries(incoming)) {
    // omit absent values
    if (value === undefined) {
      continue;
    }

    // preserve repeated headers
    if (Array.isArray(value)) {
      // append every value
      for (const entry of value) {
        headers.append(name, entry);
      }
    } else {
      headers.set(name, value);
    }
  }

  return headers;
}
