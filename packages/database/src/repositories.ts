import { createHash } from "node:crypto";

import {
  FORECAST_ADJUSTMENT_METRICS,
  FORECAST_OBSERVATION_SOURCE_LINEAGES,
  FORECAST_OBSERVATION_STATIONS,
  createBackfillChunkIdentity,
  createFixedLeadAnchorTrainingRow,
  createLegacyV4RetrievalSnapshotTrainingRow,
  createNormalizedWeatherRecord,
  forecastAnchorRecordContent,
  validateFingerprint,
  validateIngestionError,
  validateUtcInstant,
  validateVersion,
  weatherRecordContent,
  type BackfillChunkIdentity,
  type CanonicalWeatherMetrics,
  type FixedLeadAnchorTrainingRow,
  type ForecastAdjustmentMetric,
  type ForecastObservationProviderFamily,
  type ForecastObservationStationKey,
  type IngestionError,
  type IngestionMode,
  type JsonValue,
  type LegacyV4RetrievalSnapshotTrainingRow,
  type NormalizedForecastAnchorRecord,
  type NormalizedWeatherRecord,
  type SourceKind,
  type StationKind,
} from "@weather/domain";
import type { Pool, PoolClient, QueryResultRow } from "pg";

import type { SiteConfiguration } from "./config.js";
import type { EcowittConfiguration } from "./ecowitt-config.js";
import { withTransaction, type Queryable } from "./pool.js";
import type { PublicStationConfiguration } from "./public-stations-config.js";
import type { TempestConfiguration } from "./tempest-config.js";
import type { TideConfiguration } from "./tides-config.js";

// remain below PostgreSQL's parameter limit
const WEATHER_RECORD_BATCH_SIZE = 2_000;
// remain below PostgreSQL's parameter limit for anchor rows
const FORECAST_ANCHOR_RECORD_BATCH_SIZE = 2_000;
// bound ten civil forecast days with daylight-saving headroom
const MAX_FORECAST_HOURS = 264;
// match the production export window ceiling
const MAX_FORECAST_TRAINING_DAYS = 450;
// freeze one UTC hour
const HOUR_MILLISECONDS = 3_600_000;
// bind the researched ECMWF model-era cutover
const ECMWF_50R1_CUTOVER_MILLISECONDS = Date.parse(
  "2026-05-12T06:00:00.000Z",
);
// freeze station sampling windows
const FIVE_MINUTES_MILLISECONDS = 5 * 60_000;
const TEN_MINUTES_MILLISECONDS = 10 * 60_000;

// hide non-live or superseded sources
const CURRENT_SOURCE_PREDICATE = "weather_source_is_current(s.id)";

export interface DueSource extends QueryResultRow {
  readonly active: boolean;
  readonly cadenceSeconds: number;
  readonly id: string;
  readonly materialProviderConfig: JsonValue;
  readonly providerKey: string;
  readonly siteSlug: string;
  readonly sourceConfigFingerprint: string;
  readonly sourceKey: string;
  readonly sourceKind: SourceKind;
  readonly stationSlug: string;
  readonly timezone: string;
}

// expose one active discovery row
export interface ActiveSiteRow extends QueryResultRow {
  readonly attributionLabel: string;
  readonly attributionUrl: string;
  readonly latitude: number;
  readonly longitude: number;
  readonly providerKey: string;
  readonly providerName: string;
  readonly siteName: string;
  readonly siteSlug: string;
  readonly sourceId: string;
  readonly sourceKey: string;
  readonly sourceKind: SourceKind;
  readonly stationKind: StationKind;
  readonly stationLatitude: number;
  readonly stationLongitude: number;
  readonly stationName: string;
  readonly stationSlug: string;
  readonly timezone: string;
}

export interface StartIngestionRunInput {
  readonly adapterVersion: string;
  readonly attempts?: number;
  readonly chunkPlanVersion?: string | null;
  readonly deadlineAt: string;
  readonly mode: IngestionMode;
  readonly requestMetadata?: Readonly<Record<string, JsonValue>> | null;
  readonly requestedEndExclusive: string;
  readonly requestedStart: string;
  readonly sourceConfigFingerprint: string;
}

export interface StartedIngestionRun extends QueryResultRow {
  readonly id: string;
  readonly startedAt: string;
  readonly state: "running";
}

export interface ScheduledCheckpointState {
  readonly lastCommittedAt: string;
  readonly lastValidAt: string;
  readonly providerCursor: Readonly<Record<string, JsonValue>> | null;
  readonly sourceId: string;
  readonly version: number;
  readonly windowEndExclusive: string;
  readonly windowStart: string;
}

export interface CompleteScheduledIngestionInput {
  readonly attempts: number;
  readonly expectedCheckpointVersion: number | null;
  readonly lastValidAt: string;
  readonly providerCursor: Readonly<Record<string, JsonValue>> | null;
  readonly records: readonly NormalizedWeatherRecord[];
  readonly responseMetadata?: Readonly<Record<string, JsonValue>> | null;
  readonly runId: string;
  readonly upstreamResponseChecksum?: string | null;
  readonly revisionArchiver?: WeatherAdjustmentRevisionArchiver;
  readonly windowEndExclusive: string;
  readonly windowStart: string;
}

// expose one exact new or content-changing normalized serving revision
export interface PersistedWeatherAdjustmentRevision {
  readonly adapterVersion: string;
  readonly contentHash: string;
  readonly logicalReceivedAt: string;
  readonly providerKey: string;
  readonly record: NormalizedWeatherRecord;
  readonly sourceConfigFingerprint: string;
  readonly sourceKey: string;
}

// finish cold publication only after the serving transaction commits
export interface WeatherAdjustmentRevisionPublication {
  readonly publish: () => Promise<void>;
}

// stage and bind one bounded normalized revision group inside its serving transaction
export type WeatherAdjustmentRevisionArchiver = (
  queryable: Queryable,
  revisions: readonly PersistedWeatherAdjustmentRevision[],
) => Promise<WeatherAdjustmentRevisionPublication | null>;

interface PendingPhysicalWeatherAdjustmentRevisionRow extends QueryResultRow {
  readonly adapterVersion: string;
  readonly apparentTemperatureC: number | null;
  readonly blackGlobeTemperatureC: number | null;
  readonly cloudCoverPercent: number | null;
  readonly contentHash: string;
  readonly deviceModel: string | null;
  readonly deviceSerial: string | null;
  readonly deviceVendor: string | null;
  readonly logicalReceivedAt: Date;
  readonly pm25MicrogramsPerCubicMeter: number | null;
  readonly precipitationMm: number | null;
  readonly precipitationRateMmPerHour: number | null;
  readonly pressureHpa: number | null;
  readonly providerKey: string;
  readonly providerMetadata: Readonly<Record<string, JsonValue>> | null;
  readonly qualityMetadata: Readonly<Record<string, JsonValue>> | null;
  readonly receivedAt: Date;
  readonly relativeHumidityPercent: number | null;
  readonly sourceConfigFingerprint: string;
  readonly sourceId: string;
  readonly sourceKey: string;
  readonly soilElectricalConductivityMicrosiemensPerCm: number | null;
  readonly soilMoisturePercent: number | null;
  readonly solarRadiationWm2: number | null;
  readonly temperatureC: number | null;
  readonly upstreamModel: string | null;
  readonly upstreamTimezone: string;
  readonly uvIndex: number | null;
  readonly validAt: Date;
  readonly waterLevelM: number | null;
  readonly wetBulbGlobeTemperatureC: number | null;
  readonly windDirectionDegrees: number | null;
  readonly windGustMps: number | null;
  readonly windSpeedMps: number | null;
}

export interface CompleteBackfillIngestionInput {
  readonly attempts: number;
  readonly identity: BackfillChunkIdentity;
  readonly records: readonly NormalizedWeatherRecord[];
  readonly responseMetadata?: Readonly<Record<string, JsonValue>> | null;
  readonly runId: string;
  readonly upstreamResponseChecksum?: string | null;
}

export interface CompleteForecastAnchorBackfillIngestionInput {
  readonly attempts: number;
  readonly identity: BackfillChunkIdentity;
  readonly records: readonly NormalizedForecastAnchorRecord[];
  readonly responseMetadata?: Readonly<Record<string, JsonValue>> | null;
  readonly runId: string;
  readonly upstreamResponseChecksum?: string | null;
}

export interface FailIngestionRunInput {
  readonly attempts: number;
  readonly backfillIdentity?: BackfillChunkIdentity;
  readonly error: IngestionError;
  readonly responseMetadata?: Readonly<Record<string, JsonValue>> | null;
  readonly runId: string;
}

export interface HistoryQuery {
  readonly cursor?: Readonly<{ id: string; validAt: string }>;
  readonly from?: string;
  readonly limit?: number;
  readonly siteSlug: string;
  readonly sourceId?: string;
  readonly sourceKind?: SourceKind;
  readonly stationSlug?: string;
  readonly to?: string;
}

// retain only fields consumed by the fixed forecast observation projections
export interface ForecastObservationInputRow extends QueryResultRow {
  readonly apparentTemperatureC: number | null;
  readonly cloudCoverPercent: number | null;
  readonly id: string;
  readonly lastReceivedAt: Date | string;
  readonly pm25MicrogramsPerCubicMeter: number | null;
  readonly precipitationRateMmPerHour: number | null;
  readonly pressureHpa: number | null;
  readonly providerKey: string;
  readonly relativeHumidityPercent: number | null;
  readonly solarRadiationWm2: number | null;
  readonly sourceId: string;
  readonly sourceKey: string;
  readonly sourceKind: "model_current" | "physical_sensor";
  readonly stationSlug: string;
  readonly temperatureC: number | null;
  readonly uvIndex: number | null;
  readonly validAt: Date | string;
  readonly windDirectionDegrees: number | null;
  readonly windGustMps: number | null;
  readonly windSpeedMps: number | null;
}

export interface ForecastObservationInput {
  readonly projectionFrom: Date | string;
  readonly queryFrom: Date | string;
  readonly records: readonly ForecastObservationInputRow[];
  readonly siteSlug: "ballydidean";
  readonly timezone: "America/Los_Angeles";
  readonly through: Date | string;
}

export interface CurrentQuery {
  readonly sourceId?: string;
  readonly stationSlug?: string;
}

export interface ForecastQuery {
  readonly asOf: string;
  readonly hours: number;
  readonly productSelection?: ForecastProductSelection;
  readonly siteSlug: string;
}

export type ForecastProductSelection = "anchor-containing" | "latest";

// bound one local-midnight pressure context read
export interface ForecastPressureContextQuery {
  readonly asOf: string;
  readonly siteSlug: string;
}

export interface ForecastTrainingQuery {
  readonly from: string;
  readonly siteSlug: string;
  readonly to: string;
}

export interface ForecastTrainingCohorts {
  readonly fixedLeadAnchors: readonly FixedLeadAnchorTrainingRow[];
  readonly legacyV4RetrievalSnapshots: readonly LegacyV4RetrievalSnapshotTrainingRow[];
}

// persist the exact rolling-error receipt consumed by inference
export interface EcmwfTemperatureCanaryRecentErrorState {
  readonly b24C: number | null;
  readonly b72C: number | null;
  readonly cohort: "ecmwf_single_run_hindcast";
  readonly localDates: number;
  readonly mad72C: number | null;
  readonly maximumSourceRunInitializedAt: string | null;
  readonly maximumSourceValidAt: string | null;
  readonly n24: number;
  readonly n72: number;
  readonly sourceKeys: readonly string[];
  readonly supported: boolean;
  readonly targetRunInitializedAt: string;
  readonly windowEndValidAt: string;
}

// retain one private ECMWF input hour
export interface EcmwfTemperatureCanaryHour {
  readonly contentHash: string;
  readonly modelLeadHours: number;
  readonly rawRelativeHumidityPercent: number | null;
  readonly rawTemperatureC: number;
  readonly rawWindSpeedMps: number | null;
  readonly validAt: string;
}

// retain one selected private ECMWF run
export interface EcmwfTemperatureCanaryRun {
  readonly adapterVersion: "open-meteo-ecmwf-single-run/v1";
  readonly firstReceivedAt: string;
  readonly id: string;
  readonly modelCycle: "49r1" | "50r1";
  readonly providerResponseSha256: string;
  readonly recentErrorState: EcmwfTemperatureCanaryRecentErrorState;
  readonly runInitializedAt: string;
  readonly stateReason: string;
  readonly stateStatus: "supported" | "cold" | "insufficient" | "invalid";
  readonly upstreamModel: "ecmwf_ifs";
}

// return one run without mixing forecast vintages
export interface EcmwfTemperatureCanarySidecar {
  readonly hours: readonly EcmwfTemperatureCanaryHour[];
  readonly run: EcmwfTemperatureCanaryRun;
}

// expose bounded collector monitoring
export interface EcmwfTemperatureCanaryStatus {
  readonly firstReceivedAt: string;
  readonly hourCount: number;
  readonly latestRunInitializedAt: string;
  readonly stateReason: string;
  readonly stateStatus: EcmwfTemperatureCanaryRun["stateStatus"];
}

// retain one prospective audit input without exposing observations
export interface EcmwfTemperatureCanaryAuditRow {
  readonly hour: EcmwfTemperatureCanaryHour;
  readonly run: EcmwfTemperatureCanaryRun;
}

// persist one normalized run atomically
export interface PersistEcmwfTemperatureCanaryRunInput {
  readonly adapterVersion: EcmwfTemperatureCanaryRun["adapterVersion"];
  readonly hours: readonly Omit<EcmwfTemperatureCanaryHour, "contentHash">[];
  readonly modelCycle: EcmwfTemperatureCanaryRun["modelCycle"];
  readonly providerResponseSha256: string;
  readonly receivedAt: string;
  readonly recentErrorState: EcmwfTemperatureCanaryRecentErrorState;
  readonly runInitializedAt: string;
  readonly siteSlug: string;
  readonly stateReason: string;
  readonly stateStatus: EcmwfTemperatureCanaryRun["stateStatus"];
  readonly upstreamModel: EcmwfTemperatureCanaryRun["upstreamModel"];
}

// expose the exact newly persisted run needed for revision archival
export interface PersistedEcmwfTemperatureRevision {
  readonly adapterVersion: EcmwfTemperatureCanaryRun["adapterVersion"];
  readonly bestMatchRows: readonly WeatherRecordRow[];
  readonly contentHash: string;
  readonly hours: readonly EcmwfTemperatureCanaryHour[];
  readonly logicalReceivedAt: string;
  readonly modelCycle: EcmwfTemperatureCanaryRun["modelCycle"];
  readonly providerResponseSha256: string;
  readonly recentErrorState: EcmwfTemperatureCanaryRecentErrorState;
  readonly recentErrorStateSha256: string;
  readonly runInitializedAt: string;
  readonly siteId: string;
  readonly siteLatitude: number;
  readonly siteLongitude: number;
  readonly siteSlug: string;
  readonly upstreamModel: EcmwfTemperatureCanaryRun["upstreamModel"];
}

// finish cold publication only after the serving transaction commits
export interface EcmwfTemperatureRevisionPublication {
  readonly publish: () => Promise<void>;
}

// stage and bind one exact persisted run inside its serving transaction
export type EcmwfTemperatureRevisionArchiver = (
  queryable: Queryable,
  revision: PersistedEcmwfTemperatureRevision,
) => Promise<EcmwfTemperatureRevisionPublication | null>;

// expose one prior forecast selected for a causal target hour
export interface EcmwfTemperatureCanaryPriorHour extends EcmwfTemperatureCanaryHour {
  readonly key: string;
  readonly runInitializedAt: string;
}

export type ForecastObservationHourlyMetrics = Readonly<
  Record<ForecastAdjustmentMetric, number | null>
>;

export interface ForecastObservationHourlyStationRow {
  readonly metrics: ForecastObservationHourlyMetrics;
  readonly physicalStationKey: ForecastObservationStationKey;
  readonly providerFamily: ForecastObservationProviderFamily;
  readonly sourceKeys: readonly string[];
  readonly validAt: string;
}

interface ForecastAnchorStorageRow extends QueryResultRow {
  readonly adapterVersion: string;
  readonly apparentTemperatureC: number | null;
  readonly cloudCoverPercent: number | null;
  readonly contractEpoch: string;
  readonly dataset: "previous_runs";
  readonly leadHours: number;
  readonly pressureHpa: number | null;
  readonly precipitationMm: number | null;
  readonly providerMetadata: Readonly<Record<string, JsonValue>> | null;
  readonly qualityMetadata: Readonly<Record<string, JsonValue>> | null;
  readonly receivedAt: Date | string;
  readonly relativeHumidityPercent: number | null;
  readonly sourceConfigFingerprint: string;
  readonly sourceId: string;
  readonly temperatureC: number | null;
  readonly upstreamModel: "best_match";
  readonly upstreamTimezone: string;
  readonly validAt: Date | string;
  readonly windDirectionDegrees: number | null;
  readonly windGustMps: number | null;
  readonly windSpeedMps: number | null;
}

interface LegacyV4ForecastStorageRow extends QueryResultRow {
  readonly adapterVersion: string | null;
  readonly apparentTemperatureC: number | null;
  readonly blackGlobeTemperatureC: number | null;
  readonly cloudCoverPercent: number | null;
  readonly dataset: string | null;
  readonly pm25MicrogramsPerCubicMeter: number | null;
  readonly precipitationMm: number | null;
  readonly precipitationRateMmPerHour: number | null;
  readonly pressureHpa: number | null;
  readonly referenceAt: Date | string;
  readonly relativeHumidityPercent: number | null;
  readonly soilElectricalConductivityMicrosiemensPerCm: number | null;
  readonly soilMoisturePercent: number | null;
  readonly solarRadiationWm2: number | null;
  readonly sourceConfigFingerprint: string;
  readonly sourceId: string;
  readonly stableRecordId: string;
  readonly temperatureC: number | null;
  readonly upstreamModel: string | null;
  readonly uvIndex: number | null;
  readonly validAt: Date | string;
  readonly waterLevelM: number | null;
  readonly wetBulbGlobeTemperatureC: number | null;
  readonly windDirectionDegrees: number | null;
  readonly windGustMps: number | null;
  readonly windSpeedMps: number | null;
}

interface ForecastObservationStorageRow extends QueryResultRow {
  readonly adapterContract: string | null;
  readonly id: string;
  readonly qualityMetadata: Readonly<Record<string, JsonValue>> | null;
  readonly relativeHumidityPercent: number | null;
  readonly sourceConfigFingerprint: string;
  readonly sourceKey: string;
  readonly stationSlug: string;
  readonly temperatureC: number | null;
  readonly validAt: Date | string;
  readonly windDirectionDegrees: number | null;
  readonly windGustMps: number | null;
  readonly windSpeedMps: number | null;
}

interface EcmwfTemperatureCanaryRunStorageRow extends QueryResultRow {
  readonly adapterVersion: string;
  readonly contentHash: string;
  readonly firstReceivedAt: Date | string;
  readonly hourCount?: number | string;
  readonly id: string;
  readonly modelCycle: string;
  readonly providerResponseSha256: string;
  readonly recentErrorState: EcmwfTemperatureCanaryRecentErrorState;
  readonly recentErrorStateSha256: string;
  readonly runInitializedAt: Date | string;
  readonly stateReason: string;
  readonly stateStatus: string;
  readonly upstreamModel: string;
}

interface EcmwfTemperatureCanaryHourStorageRow extends QueryResultRow {
  readonly contentHash?: string;
  readonly modelLeadHours: number;
  readonly rawRelativeHumidityPercent: number | null;
  readonly rawTemperatureC: number;
  readonly rawWindSpeedMps: number | null;
  readonly runInitializedAt?: Date | string;
  readonly validAt: Date | string;
}

interface ValidatedForecastObservationStorageRow {
  readonly id: string;
  readonly physicalStationKey: ForecastObservationStationKey;
  readonly relativeHumidityPercent: number | null;
  readonly sourceKey: string;
  readonly temperatureC: number | null;
  readonly validAt: string;
  readonly validAtMilliseconds: number;
  readonly windDirectionDegrees: number | null;
  readonly windGustMps: number | null;
  readonly windSpeedMps: number | null;
}

export interface TrendQuery {
  readonly from: string;
  readonly siteSlug: string;
  readonly to: string;
}

export interface DailyPrecipitationQuery {
  readonly from: string;
  readonly siteSlug: string;
  readonly to: string;
}

export interface DailyPrecipitationRow extends QueryResultRow {
  readonly accumulationMm: number;
  readonly sourceId: string;
  readonly stationSlug: string;
  readonly validThrough: Date | string;
}

export interface TideQuery {
  readonly from: string;
  readonly limit?: number;
  readonly siteSlug: string;
  readonly to: string;
}

export interface TrendPointRow extends QueryResultRow {
  readonly apparentTemperatureC: number | null;
  readonly precipitationMm: number | null;
  readonly pressureHpa: number | null;
  readonly relativeHumidityPercent: number | null;
  readonly temperatureC: number | null;
  readonly temperatureMaximumC: number | null;
  readonly temperatureMinimumC: number | null;
  readonly validAt: Date;
  readonly windDirectionDegrees: number | null;
  readonly windGustMps: number | null;
  readonly windSpeedMps: number | null;
}

export interface LatestWorkerHeartbeat extends QueryResultRow {
  readonly lastLoopAt: Date | string;
}

export interface WeatherRecordRow extends QueryResultRow {
  readonly adapterVersion: string | null;
  readonly apparentTemperatureC: number | null;
  readonly blackGlobeTemperatureC: number | null;
  readonly cloudCoverPercent: number | null;
  readonly contentHash: string;
  readonly contractEpoch: string | null;
  readonly deviceModel: string | null;
  readonly deviceSerial: string | null;
  readonly deviceVendor: string | null;
  readonly firstReceivedAt: Date | string;
  readonly id: string;
  readonly lastReceivedAt: Date | string;
  readonly pm25MicrogramsPerCubicMeter: number | null;
  readonly precipitationMm: number | null;
  readonly precipitationRateMmPerHour: number | null;
  readonly pressureHpa: number | null;
  readonly productRunAt: Date | string | null;
  readonly providerKey: string;
  readonly providerMetadata: Readonly<Record<string, JsonValue>> | null;
  readonly qualityMetadata: Readonly<Record<string, JsonValue>> | null;
  readonly relativeHumidityPercent: number | null;
  readonly revisionCount: number;
  readonly siteSlug: string;
  readonly sourceId: string;
  readonly sourceConfigFingerprint: string | null;
  readonly sourceKey: string;
  readonly sourceKind: SourceKind;
  readonly stationSlug: string;
  readonly soilElectricalConductivityMicrosiemensPerCm: number | null;
  readonly soilMoisturePercent: number | null;
  readonly solarRadiationWm2: number | null;
  readonly temperatureC: number | null;
  readonly upstreamModel: string | null;
  readonly upstreamTimezone: string;
  readonly uvIndex: number | null;
  readonly validAt: Date | string;
  readonly waterLevelM: number | null;
  readonly windDirectionDegrees: number | null;
  readonly windGustMps: number | null;
  readonly windSpeedMps: number | null;
  readonly wetBulbGlobeTemperatureC: number | null;
}

// retain same-source pressure change with its current row
export interface CurrentWeatherRecordRow extends WeatherRecordRow {
  readonly pressureChange3hHpa: number | null;
}

interface ForecastRuntimeProvenanceRow extends QueryResultRow {
  readonly adapterVersion: string;
  readonly contractEpoch: string;
  readonly sourceConfigFingerprint: string;
  readonly sourceId: string;
  readonly sourceKey: string;
  readonly weatherRecordId: string;
}

export interface TideRecordRow extends QueryResultRow {
  readonly attributionLabel: string;
  readonly attributionUrl: string;
  readonly predictionType: string | null;
  readonly providerKey: string;
  readonly sourceId: string;
  readonly sourceKind: "tide_observation" | "tide_prediction";
  readonly stationName: string;
  readonly stationSlug: string;
  readonly validAt: Date | string;
  readonly waterLevelM: number;
}

// retain one source-scoped advisory lock
export class SourceSession {
  readonly #client: PoolClient;
  #released = false;
  readonly sourceId: string;

  // initialize a claimed source session
  constructor(client: PoolClient, sourceId: string) {
    this.#client = client;
    this.sourceId = sourceId;
  }

  // expose the retained client internally
  get client(): PoolClient {
    // reject use after release
    if (this.#released) {
      throw new Error("source session has already been released");
    }

    return this.#client;
  }

  // release the source lock and client
  async release(): Promise<void> {
    // make release idempotent
    if (this.#released) {
      return;
    }

    try {
      await this.#client.query(
        "SELECT pg_advisory_unlock(hashtextextended('weather-source:' || $1, 0))",
        [this.sourceId],
      );
    } finally {
      this.#released = true;
      this.#client.release();
    }
  }
}

// idempotently bootstrap configured entities
export async function bootstrapSiteConfiguration(
  pool: Pool,
  configuration: SiteConfiguration,
): Promise<Readonly<{ providerId: string; siteId: string; sourceIds: readonly string[]; stationId: string }>> {
  const client = await pool.connect();

  try {
    return await withTransaction(client, async () => {
      const site = await client.query<{ id: string }>(
        `
          INSERT INTO sites (slug, display_name, latitude, longitude, timezone, active)
          VALUES ($1, $2, $3, $4, $5, $6)
          ON CONFLICT (slug) DO UPDATE SET
            display_name = EXCLUDED.display_name,
            latitude = EXCLUDED.latitude,
            longitude = EXCLUDED.longitude,
            timezone = EXCLUDED.timezone,
            active = EXCLUDED.active,
            updated_at = clock_timestamp()
          RETURNING id
        `,
        [
          configuration.site.key,
          configuration.site.displayName,
          configuration.site.latitude,
          configuration.site.longitude,
          configuration.site.timezone,
          configuration.site.active,
        ],
      );
      const provider = await client.query<{ id: string }>(
        `
          INSERT INTO providers (
            provider_key,
            display_name,
            attribution_label,
            attribution_url,
            active
          )
          VALUES ($1, $2, $3, $4, $5)
          ON CONFLICT (provider_key) DO UPDATE SET
            display_name = EXCLUDED.display_name,
            attribution_label = EXCLUDED.attribution_label,
            attribution_url = EXCLUDED.attribution_url,
            active = EXCLUDED.active,
            updated_at = clock_timestamp()
          RETURNING id
        `,
        [
          configuration.provider.key,
          configuration.provider.displayName,
          configuration.provider.attributionLabel,
          configuration.provider.attributionUrl,
          configuration.provider.active,
        ],
      );
      const siteId = requireRow(site.rows[0], "site bootstrap").id;
      const providerId = requireRow(provider.rows[0], "provider bootstrap").id;
      const station = await client.query<{ id: string }>(
        `
          INSERT INTO stations (
            site_id,
            slug,
            display_name,
            station_kind,
            latitude,
            longitude,
            vendor,
            model,
            serial,
            active
          )
          VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
          ON CONFLICT (site_id, slug) DO UPDATE SET
            display_name = EXCLUDED.display_name,
            station_kind = EXCLUDED.station_kind,
            latitude = EXCLUDED.latitude,
            longitude = EXCLUDED.longitude,
            vendor = EXCLUDED.vendor,
            model = EXCLUDED.model,
            serial = EXCLUDED.serial,
            active = EXCLUDED.active,
            updated_at = clock_timestamp()
          RETURNING id
        `,
        [
          siteId,
          configuration.station.key,
          configuration.station.displayName,
          configuration.station.kind,
          configuration.station.latitude,
          configuration.station.longitude,
          configuration.station.vendor,
          configuration.station.model,
          configuration.station.serial,
          configuration.station.active,
        ],
      );
      const stationId = requireRow(station.rows[0], "station bootstrap").id;
      const sourceIds: string[] = [];

      // bootstrap every immutable source
      for (const source of configuration.sources) {
        const result = await client.query<{ id: string }>(
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
            VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7::jsonb, $8, $9)
            ON CONFLICT (station_id, source_key) DO UPDATE SET
              cadence_seconds = EXCLUDED.cadence_seconds,
              active = EXCLUDED.active
            WHERE sources.source_config_fingerprint = EXCLUDED.source_config_fingerprint
            RETURNING id
          `,
          [
            stationId,
            providerId,
            source.key,
            source.sourceKind,
            JSON.stringify(source.adapterConfig),
            source.fingerprint,
            JSON.stringify(source.capabilities),
            source.cadenceSeconds,
            source.active,
          ],
        );

        // reject silent semantic drift
        if (result.rowCount !== 1 || result.rows[0] === undefined) {
          throw new Error(
            `source ${source.key} changed material configuration; create a new source key`,
          );
        }

        sourceIds.push(result.rows[0].id);
      }

      return { providerId, siteId, sourceIds, stationId };
    });
  } finally {
    client.release();
  }
}

// bootstrap the configured physical Tempest sources
export async function bootstrapTempestConfiguration(
  pool: Pool,
  configuration: TempestConfiguration,
): Promise<Readonly<{
  providerId: string;
  sourceIds: Readonly<Record<string, string>>;
  stationIds: Readonly<Record<string, string>>;
}>> {
  const client = await pool.connect();

  try {
    return await withTransaction(client, async () => {
      const site = await client.query<{ id: string }>(
        "SELECT id FROM sites WHERE slug = $1 AND active",
        [configuration.siteKey],
      );
      const siteId = requireRow(site.rows[0], "Tempest site bootstrap").id;
      const provider = await client.query<{ id: string }>(
        `
          INSERT INTO providers (
            provider_key,
            display_name,
            attribution_label,
            attribution_url,
            active
          )
          VALUES ($1, $2, $3, $4, $5)
          ON CONFLICT (provider_key) DO UPDATE SET
            display_name = EXCLUDED.display_name,
            attribution_label = EXCLUDED.attribution_label,
            attribution_url = EXCLUDED.attribution_url,
            active = EXCLUDED.active,
            updated_at = clock_timestamp()
          RETURNING id
        `,
        [
          configuration.provider.key,
          configuration.provider.displayName,
          configuration.provider.attributionLabel,
          configuration.provider.attributionUrl,
          configuration.provider.active,
        ],
      );
      const providerId = requireRow(provider.rows[0], "Tempest provider bootstrap").id;
      const stationIds: Record<string, string> = {};
      const sourceIds: Record<string, string> = {};

      // bootstrap every configured station and source
      for (const station of configuration.stations) {
        const stationResult = await client.query<{ id: string }>(
          `
            INSERT INTO stations (
              site_id,
              slug,
              display_name,
              station_kind,
              latitude,
              longitude,
              vendor,
              model,
              serial,
              active
            )
            VALUES ($1, $2, $3, 'physical', $4, $5, 'WeatherFlow', 'Tempest', $6, $7)
            ON CONFLICT (site_id, slug) DO UPDATE SET
              display_name = EXCLUDED.display_name,
              station_kind = EXCLUDED.station_kind,
              latitude = EXCLUDED.latitude,
              longitude = EXCLUDED.longitude,
              vendor = EXCLUDED.vendor,
              model = EXCLUDED.model,
              serial = EXCLUDED.serial,
              active = EXCLUDED.active,
              updated_at = clock_timestamp()
            RETURNING id
          `,
          [
            siteId,
            station.key,
            station.displayName,
            station.latitude,
            station.longitude,
            station.serial,
            station.active,
          ],
        );
        const stationId = requireRow(
          stationResult.rows[0],
          `Tempest station ${station.key} bootstrap`,
        ).id;
        const sourceResult = await client.query<{ id: string }>(
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
            VALUES ($1, $2, $3, 'physical_sensor', $4::jsonb, $5, $6::jsonb, $7, $8)
            ON CONFLICT (station_id, source_key) DO UPDATE SET
              cadence_seconds = EXCLUDED.cadence_seconds,
              active = EXCLUDED.active
            WHERE sources.source_config_fingerprint = EXCLUDED.source_config_fingerprint
            RETURNING id
          `,
          [
            stationId,
            providerId,
            station.sourceKey,
            JSON.stringify(station.adapterConfig),
            station.fingerprint,
            JSON.stringify(["current", "historical"]),
            station.cadenceSeconds,
            station.active,
          ],
        );

        // reject silent immutable-source drift
        if (sourceResult.rowCount !== 1 || sourceResult.rows[0] === undefined) {
          throw new Error(
            `Tempest source ${station.sourceKey} changed material configuration; create a new source key`,
          );
        }

        stationIds[station.key] = stationId;
        sourceIds[station.sourceKey] = sourceResult.rows[0].id;
      }

      return { providerId, sourceIds, stationIds };
    });
  } finally {
    client.release();
  }
}

// bootstrap configured first-party Ecowitt gateways
export async function bootstrapEcowittConfiguration(
  pool: Pool,
  configuration: EcowittConfiguration,
): Promise<Readonly<{
  providerId: string;
  sourceIds: Readonly<Record<string, string>>;
  stationIds: Readonly<Record<string, string>>;
}>> {
  const client = await pool.connect();

  try {
    return await withTransaction(client, async () => {
      const site = await client.query<{ id: string }>(
        "SELECT id FROM sites WHERE slug = $1 AND active",
        [configuration.siteKey],
      );
      const siteId = requireRow(site.rows[0], "Ecowitt site bootstrap").id;
      const provider = await client.query<{ id: string }>(
        `
          INSERT INTO providers (
            provider_key,
            display_name,
            attribution_label,
            attribution_url,
            active
          )
          VALUES ($1, $2, $3, $4, $5)
          ON CONFLICT (provider_key) DO UPDATE SET
            display_name = EXCLUDED.display_name,
            attribution_label = EXCLUDED.attribution_label,
            attribution_url = EXCLUDED.attribution_url,
            active = EXCLUDED.active,
            updated_at = clock_timestamp()
          RETURNING id
        `,
        [
          configuration.provider.key,
          configuration.provider.displayName,
          configuration.provider.attributionLabel,
          configuration.provider.attributionUrl,
          configuration.provider.active,
        ],
      );
      const providerId = requireRow(provider.rows[0], "Ecowitt provider bootstrap").id;
      const stationIds: Record<string, string> = {};
      const sourceIds: Record<string, string> = {};

      // bootstrap every configured gateway and source
      for (const station of configuration.stations) {
        const stationResult = await client.query<{ id: string }>(
          `
            INSERT INTO stations (
              site_id,
              slug,
              display_name,
              station_kind,
              latitude,
              longitude,
              vendor,
              model,
              serial,
              active
            )
            VALUES ($1, $2, $3, 'physical', $4, $5, 'Ecowitt', $6, $7, $8)
            ON CONFLICT (site_id, slug) DO UPDATE SET
              display_name = EXCLUDED.display_name,
              station_kind = EXCLUDED.station_kind,
              latitude = EXCLUDED.latitude,
              longitude = EXCLUDED.longitude,
              vendor = EXCLUDED.vendor,
              model = EXCLUDED.model,
              serial = EXCLUDED.serial,
              active = EXCLUDED.active,
              updated_at = clock_timestamp()
            RETURNING id
          `,
          [
            siteId,
            station.key,
            station.displayName,
            station.latitude,
            station.longitude,
            station.model,
            station.expectedMac,
            station.active,
          ],
        );
        const stationId = requireRow(
          stationResult.rows[0],
          `Ecowitt station ${station.key} bootstrap`,
        ).id;
        const sourceResult = await client.query<{ id: string }>(
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
            VALUES ($1, $2, $3, 'physical_sensor', $4::jsonb, $5, $6::jsonb, $7, $8)
            ON CONFLICT (station_id, source_key) DO UPDATE SET
              cadence_seconds = EXCLUDED.cadence_seconds,
              active = EXCLUDED.active
            WHERE sources.source_config_fingerprint = EXCLUDED.source_config_fingerprint
            RETURNING id
          `,
          [
            stationId,
            providerId,
            station.sourceKey,
            JSON.stringify(station.adapterConfig),
            station.fingerprint,
            JSON.stringify(["current"]),
            station.cadenceSeconds,
            station.active,
          ],
        );

        // reject silent immutable-source drift
        if (sourceResult.rowCount !== 1 || sourceResult.rows[0] === undefined) {
          throw new Error(
            `Ecowitt source ${station.sourceKey} changed material configuration; create a new source key`,
          );
        }

        stationIds[station.key] = stationId;
        sourceIds[station.sourceKey] = sourceResult.rows[0].id;
      }

      return { providerId, sourceIds, stationIds };
    });
  } finally {
    client.release();
  }
}

// bootstrap configured public physical-station sources
export async function bootstrapPublicStationConfiguration(
  pool: Pool,
  configuration: PublicStationConfiguration,
): Promise<Readonly<{
  providerIds: Readonly<Record<string, string>>;
  sourceIds: Readonly<Record<string, string>>;
  stationIds: Readonly<Record<string, string>>;
}>> {
  const client = await pool.connect();

  try {
    return await withTransaction(client, async () => {
      const site = await client.query<{ id: string }>(
        "SELECT id FROM sites WHERE slug = $1 AND active",
        [configuration.siteKey],
      );
      const siteId = requireRow(site.rows[0], "public-station site bootstrap").id;
      const providerIds: Record<string, string> = {};

      // bootstrap every declared provider
      for (const provider of configuration.providers) {
        const result = await client.query<{ id: string }>(
          `
            INSERT INTO providers (
              provider_key,
              display_name,
              attribution_label,
              attribution_url,
              active
            )
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (provider_key) DO UPDATE SET
              display_name = EXCLUDED.display_name,
              attribution_label = EXCLUDED.attribution_label,
              attribution_url = EXCLUDED.attribution_url,
              active = EXCLUDED.active,
              updated_at = clock_timestamp()
            RETURNING id
          `,
          [
            provider.key,
            provider.displayName,
            provider.attributionLabel,
            provider.attributionUrl,
            provider.active,
          ],
        );
        providerIds[provider.key] = requireRow(
          result.rows[0],
          `public-station provider ${provider.key} bootstrap`,
        ).id;
      }

      const stationIds: Record<string, string> = {};
      const sourceIds: Record<string, string> = {};

      // bootstrap every configured station
      for (const station of configuration.stations) {
        const stationResult = await client.query<{ id: string }>(
          `
            INSERT INTO stations (
              site_id,
              slug,
              display_name,
              station_kind,
              latitude,
              longitude,
              vendor,
              model,
              serial,
              active
            )
            VALUES ($1, $2, $3, 'physical', $4, $5, $6, $7, $8, $9)
            ON CONFLICT (site_id, slug) DO UPDATE SET
              display_name = EXCLUDED.display_name,
              station_kind = EXCLUDED.station_kind,
              latitude = EXCLUDED.latitude,
              longitude = EXCLUDED.longitude,
              vendor = EXCLUDED.vendor,
              model = EXCLUDED.model,
              serial = EXCLUDED.serial,
              active = EXCLUDED.active,
              updated_at = clock_timestamp()
            RETURNING id
          `,
          [
            siteId,
            station.key,
            station.displayName,
            station.latitude,
            station.longitude,
            station.vendor,
            station.model,
            station.serial,
            station.active,
          ],
        );
        const stationId = requireRow(
          stationResult.rows[0],
          `public station ${station.key} bootstrap`,
        ).id;
        stationIds[station.key] = stationId;

        // bootstrap every immutable source
        for (const source of station.sources) {
          const providerId = providerIds[source.providerKey];

          // require the parsed provider mapping
          if (providerId === undefined) {
            throw new Error(`public-station provider ${source.providerKey} is missing`);
          }

          const sourceResult = await client.query<{ id: string }>(
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
              VALUES ($1, $2, $3, 'physical_sensor', $4::jsonb, $5, $6::jsonb, $7, $8)
              ON CONFLICT (station_id, source_key) DO UPDATE SET
                cadence_seconds = EXCLUDED.cadence_seconds,
                active = EXCLUDED.active
              WHERE sources.source_config_fingerprint = EXCLUDED.source_config_fingerprint
              RETURNING id
            `,
            [
              stationId,
              providerId,
              source.key,
              JSON.stringify(source.adapterConfig),
              source.fingerprint,
              JSON.stringify(source.capabilities),
              source.cadenceSeconds,
              source.active,
            ],
          );

          // reject silent immutable-source drift
          if (sourceResult.rowCount !== 1 || sourceResult.rows[0] === undefined) {
            throw new Error(
              `public-station source ${source.key} changed material configuration; create a new source key`,
            );
          }

          sourceIds[source.key] = sourceResult.rows[0].id;
        }
      }

      return { providerIds, sourceIds, stationIds };
    });
  } finally {
    client.release();
  }
}

// bootstrap configured NOAA tide sources
export async function bootstrapTideConfiguration(
  pool: Pool,
  configuration: TideConfiguration,
): Promise<Readonly<{
  providerId: string;
  sourceIds: Readonly<Record<string, string>>;
  stationIds: Readonly<Record<string, string>>;
}>> {
  const client = await pool.connect();

  try {
    return await withTransaction(client, async () => {
      const site = await client.query<{ id: string }>(
        "SELECT id FROM sites WHERE slug = $1 AND active",
        [configuration.siteKey],
      );
      const siteId = requireRow(site.rows[0], "tide site bootstrap").id;
      const provider = await client.query<{ id: string }>(
        `
          INSERT INTO providers (
            provider_key,
            display_name,
            attribution_label,
            attribution_url,
            active
          )
          VALUES ($1, $2, $3, $4, $5)
          ON CONFLICT (provider_key) DO UPDATE SET
            display_name = EXCLUDED.display_name,
            attribution_label = EXCLUDED.attribution_label,
            attribution_url = EXCLUDED.attribution_url,
            active = EXCLUDED.active,
            updated_at = clock_timestamp()
          RETURNING id
        `,
        [
          configuration.provider.key,
          configuration.provider.displayName,
          configuration.provider.attributionLabel,
          configuration.provider.attributionUrl,
          configuration.provider.active,
        ],
      );
      const providerId = requireRow(provider.rows[0], "tide provider bootstrap").id;
      const stationIds: Record<string, string> = {};
      const sourceIds: Record<string, string> = {};

      // bootstrap every tide station and source
      for (const station of configuration.stations) {
        const stationResult = await client.query<{ id: string }>(
          `
            INSERT INTO stations (
              site_id,
              slug,
              display_name,
              station_kind,
              latitude,
              longitude,
              vendor,
              model,
              serial,
              active
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (site_id, slug) DO UPDATE SET
              display_name = EXCLUDED.display_name,
              station_kind = EXCLUDED.station_kind,
              latitude = EXCLUDED.latitude,
              longitude = EXCLUDED.longitude,
              vendor = EXCLUDED.vendor,
              model = EXCLUDED.model,
              serial = EXCLUDED.serial,
              active = EXCLUDED.active,
              updated_at = clock_timestamp()
            RETURNING id
          `,
          [
            siteId,
            station.key,
            station.displayName,
            station.kind,
            station.latitude,
            station.longitude,
            station.vendor,
            station.model,
            station.serial,
            station.active,
          ],
        );
        const stationId = requireRow(
          stationResult.rows[0],
          `tide station ${station.key} bootstrap`,
        ).id;
        const source = station.source;
        const sourceResult = await client.query<{ id: string }>(
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
            VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7::jsonb, $8, $9)
            ON CONFLICT (station_id, source_key) DO UPDATE SET
              cadence_seconds = EXCLUDED.cadence_seconds,
              active = EXCLUDED.active
            WHERE sources.source_config_fingerprint = EXCLUDED.source_config_fingerprint
            RETURNING id
          `,
          [
            stationId,
            providerId,
            source.key,
            source.sourceKind,
            JSON.stringify(source.adapterConfig),
            source.fingerprint,
            JSON.stringify(source.capabilities),
            source.cadenceSeconds,
            station.active && source.active,
          ],
        );

        // reject silent immutable-source drift
        if (sourceResult.rowCount !== 1 || sourceResult.rows[0] === undefined) {
          throw new Error(
            `tide source ${source.key} changed material configuration; create a new source key`,
          );
        }

        stationIds[station.key] = stationId;
        sourceIds[source.key] = sourceResult.rows[0].id;
      }

      return { providerId, sourceIds, stationIds };
    });
  } finally {
    client.release();
  }
}

// discover scheduled sources due for work
export async function discoverDueSources(
  pool: Pool,
  asOf: string,
  limit = 100,
): Promise<readonly DueSource[]> {
  const validatedAsOf = validateUtcInstant(asOf, "asOf");

  // enforce bounded discovery
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > 500) {
    throw new RangeError("source discovery limit must be between 1 and 500");
  }

  const result = await pool.query<DueSource>(
    `
      SELECT
        s.id,
        s.source_key AS "sourceKey",
        s.source_kind AS "sourceKind",
        s.source_config_fingerprint AS "sourceConfigFingerprint",
        s.material_provider_config AS "materialProviderConfig",
        s.cadence_seconds AS "cadenceSeconds",
        s.active,
        st.slug AS "stationSlug",
        si.slug AS "siteSlug",
        si.timezone,
        p.provider_key AS "providerKey"
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      LEFT JOIN ingestion_checkpoints checkpoint ON checkpoint.source_id = s.id
      WHERE s.active
        AND st.active
        AND si.active
        AND p.active
        AND ${CURRENT_SOURCE_PREDICATE}
        AND s.cadence_seconds IS NOT NULL
        AND (
          checkpoint.last_committed_at IS NULL
          OR checkpoint.last_committed_at + make_interval(secs => s.cadence_seconds) <= $1
        )
      ORDER BY checkpoint.last_committed_at ASC NULLS FIRST, s.id ASC
      LIMIT $2
    `,
    [validatedAsOf, limit],
  );

  return result.rows;
}

// try to retain one per-source session lock
export async function acquireSourceSession(
  pool: Pool,
  sourceId: string,
): Promise<SourceSession | null> {
  const client = await pool.connect();

  try {
    const result = await client.query<{ acquired: boolean }>(
      "SELECT pg_try_advisory_lock(hashtextextended('weather-source:' || $1, 0)) AS acquired",
      [sourceId],
    );

    // release unclaimed clients
    if (result.rows[0]?.acquired !== true) {
      client.release();
      return null;
    }

    return new SourceSession(client, sourceId);
  } catch (error) {
    client.release();
    throw error;
  }
}

// recover expired work under its source lock
export async function abandonExpiredRuns(
  session: SourceSession,
  asOf: string,
): Promise<readonly string[]> {
  const result = await session.client.query<{ id: string }>(
    `
      UPDATE ingestion_runs
      SET
        state = 'abandoned',
        completed_at = $2,
        error_classification = 'retryable',
        error_code = 'deadline_expired',
        error_message = 'ingestion run exceeded its recovery deadline'
      WHERE source_id = $1
        AND state = 'running'
        AND deadline_at < $2
      RETURNING id
    `,
    [session.sourceId, validateUtcInstant(asOf, "asOf")],
  );

  return result.rows.map((row) => row.id);
}

// commit a running row before provider I/O
export async function startIngestionRun(
  session: SourceSession,
  input: StartIngestionRunInput,
): Promise<StartedIngestionRun> {
  const requestedStart = validateUtcInstant(input.requestedStart, "requestedStart");
  const requestedEndExclusive = validateUtcInstant(
    input.requestedEndExclusive,
    "requestedEndExclusive",
  );
  const deadlineAt = validateUtcInstant(input.deadlineAt, "deadlineAt");

  // require ordered request bounds
  if (requestedStart >= requestedEndExclusive) {
    throw new RangeError("ingestion interval must be increasing");
  }

  // require a future recovery deadline
  if (deadlineAt <= new Date().toISOString()) {
    throw new RangeError("ingestion deadline must be in the future");
  }

  validateFingerprint(input.sourceConfigFingerprint);
  validateVersion(input.adapterVersion, "adapterVersion");

  // validate backfill plan identity
  if (input.mode === "backfill" && input.chunkPlanVersion == null) {
    throw new RangeError("backfill runs require chunkPlanVersion");
  }

  // validate optional plan version
  if (input.chunkPlanVersion != null) {
    validateVersion(input.chunkPlanVersion, "chunkPlanVersion");
  }

  const existing = await session.client.query<{ deadline_at: Date; id: string }>(
    `
      SELECT id, deadline_at
      FROM ingestion_runs
      WHERE source_id = $1
        AND state = 'running'
      FOR UPDATE
    `,
    [session.sourceId],
  );

  // reject replacement before explicit recovery
  if (existing.rowCount !== 0) {
    throw new Error(
      `source already has a running ingestion: ${existing.rows[0]?.id ?? "unknown"}`,
    );
  }

  const result = await session.client.query<StartedIngestionRun>(
    `
      INSERT INTO ingestion_runs (
        source_id,
        mode,
        requested_start,
        requested_end_exclusive,
        source_config_fingerprint,
        adapter_version,
        chunk_plan_version,
        deadline_at,
        attempts,
        request_metadata
      )
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb)
      RETURNING id, started_at AS "startedAt", state
    `,
    [
      session.sourceId,
      input.mode,
      requestedStart,
      requestedEndExclusive,
      input.sourceConfigFingerprint,
      input.adapterVersion,
      input.chunkPlanVersion ?? null,
      deadlineAt,
      input.attempts ?? 0,
      serializeNullableJson(input.requestMetadata ?? null),
    ],
  );

  return requireRow(result.rows[0], "running ingestion insert");
}

// atomically store scheduled success
export async function completeScheduledIngestion(
  session: SourceSession,
  input: CompleteScheduledIngestionInput,
): Promise<void> {
  validateCompletionCounts(input.attempts, input.records.length);

  const publications = await withTransaction(session.client, async () => {
    await assertScheduledRunMatches(session, input);
    const changed = input.revisionArchiver === undefined
      ? []
      : await selectChangedWeatherRevisionInputs(session, input.records);
    await upsertWeatherRecords(session, input.runId, input.records);
    const staged = input.revisionArchiver === undefined
      ? []
      : await archiveChangedWeatherRevisions(
          session,
          input.runId,
          changed,
          input.revisionArchiver,
        );
    await advanceScheduledCheckpoint(session, input);
    await finalizeSuccessfulRun(
      session,
      input.runId,
      input.attempts,
      input.records.length,
      input.responseMetadata ?? null,
      input.upstreamResponseChecksum ?? null,
      "scheduled",
    );
    return staged;
  });
  // publish each staged revision only after the serving transaction commits
  for (const publication of publications) {
    try {
      await publication.publish();
    } catch {
      // preserve ingestion success after a permanent cold publication gap
    }
  }
}

// atomically store backfill success
export async function completeBackfillIngestion(
  session: SourceSession,
  input: CompleteBackfillIngestionInput,
): Promise<void> {
  const identity = createBackfillChunkIdentity(input.identity);

  // require matching source identity
  if (identity.sourceId !== session.sourceId) {
    throw new Error("backfill identity source does not match the locked source");
  }

  validateCompletionCounts(input.attempts, input.records.length);

  await withTransaction(session.client, async () => {
    await assertBackfillRunMatches(session, input.runId, identity);
    await upsertWeatherRecords(session, input.runId, input.records);
    await upsertBackfillOutcome(session, input.runId, identity, "succeeded", null);
    await finalizeSuccessfulRun(
      session,
      input.runId,
      input.attempts,
      input.records.length,
      input.responseMetadata ?? null,
      input.upstreamResponseChecksum ?? null,
      "backfill",
    );
  });
}

// atomically store fixed-anchor backfill success
export async function completeForecastAnchorBackfillIngestion(
  session: SourceSession,
  input: CompleteForecastAnchorBackfillIngestionInput,
): Promise<void> {
  const identity = createBackfillChunkIdentity(input.identity);

  // require matching source identity
  if (identity.sourceId !== session.sourceId) {
    throw new Error("backfill identity source does not match the locked source");
  }

  // bind every anchor to the exact backfill provenance
  for (const record of input.records) {
    // reject cross-run anchor provenance
    if (
      record.sourceConfigFingerprint !== identity.sourceConfigFingerprint ||
      record.adapterVersion !== identity.adapterVersion
    ) {
      throw new Error("forecast anchor provenance does not match the backfill identity");
    }
  }

  validateCompletionCounts(input.attempts, input.records.length);

  await withTransaction(session.client, async () => {
    await assertBackfillRunMatches(session, input.runId, identity);
    await upsertForecastAnchorRecords(session, input.runId, input.records);
    await upsertBackfillOutcome(session, input.runId, identity, "succeeded", null);
    await finalizeSuccessfulRun(
      session,
      input.runId,
      input.attempts,
      input.records.length,
      input.responseMetadata ?? null,
      input.upstreamResponseChecksum ?? null,
      "backfill",
    );
  });
}

// guard-finalize a failed run
export async function failIngestionRun(
  session: SourceSession,
  input: FailIngestionRunInput,
): Promise<void> {
  const error = validateIngestionError(input.error);
  validateCompletionCounts(input.attempts, 0);

  await withTransaction(session.client, async () => {
    // record an eligible failed chunk
    if (input.backfillIdentity !== undefined) {
      const identity = createBackfillChunkIdentity(input.backfillIdentity);

      // require matching source identity
      if (identity.sourceId !== session.sourceId) {
        throw new Error("backfill identity source does not match the locked source");
      }

      await assertBackfillRunMatches(session, input.runId, identity);
      await upsertBackfillOutcome(
        session,
        input.runId,
        identity,
        "failed",
        error.code,
      );
    } else {
      await assertRunningMode(session, input.runId, "scheduled");
    }

    const result = await session.client.query(
      `
        UPDATE ingestion_runs
        SET
          state = 'failed',
          completed_at = clock_timestamp(),
          attempts = $3,
          response_metadata = $4::jsonb,
          error_classification = $5,
          error_code = $6,
          error_message = $7
        WHERE id = $1
          AND source_id = $2
          AND state = 'running'
      `,
      [
        input.runId,
        session.sourceId,
        input.attempts,
        serializeNullableJson(input.responseMetadata ?? null),
        error.classification,
        error.code,
        error.message,
      ],
    );
    requireGuardedUpdate(result.rowCount, "failed ingestion finalization");
  });
}

// test exact successful resume identity
export async function hasSuccessfulBackfillChunk(
  pool: Pool,
  identity: BackfillChunkIdentity,
): Promise<boolean> {
  const validated = createBackfillChunkIdentity(identity);
  const result = await pool.query(
    `
      SELECT 1
      FROM backfill_chunk_outcomes
      WHERE source_id = $1
        AND interval_start = $2
        AND interval_end_exclusive = $3
        AND source_config_fingerprint = $4
        AND adapter_version = $5
        AND chunk_plan_version = $6
        AND outcome = 'succeeded'
    `,
    [
      validated.sourceId,
      validated.intervalStart,
      validated.intervalEndExclusive,
      validated.sourceConfigFingerprint,
      validated.adapterVersion,
      validated.chunkPlanVersion,
    ],
  );

  return result.rowCount === 1;
}

// read checkpoint state under the source lock
export async function getScheduledCheckpoint(
  session: SourceSession,
): Promise<ScheduledCheckpointState | null> {
  const result = await session.client.query<{
    last_committed_at: Date;
    last_valid_at: Date;
    provider_cursor: Readonly<Record<string, JsonValue>> | null;
    source_id: string;
    version: string;
    window_end_exclusive: Date;
    window_start: Date;
  }>(
    `
      SELECT
        source_id,
        last_valid_at,
        window_start,
        window_end_exclusive,
        provider_cursor,
        version,
        last_committed_at
      FROM ingestion_checkpoints
      WHERE source_id = $1
    `,
    [session.sourceId],
  );
  const row = result.rows[0];

  // represent an uninitialized checkpoint
  if (row === undefined) {
    return null;
  }

  return {
    lastCommittedAt: row.last_committed_at.toISOString(),
    lastValidAt: row.last_valid_at.toISOString(),
    providerCursor: row.provider_cursor,
    sourceId: row.source_id,
    version: Number(row.version),
    windowEndExclusive: row.window_end_exclusive.toISOString(),
    windowStart: row.window_start.toISOString(),
  };
}

// update liveness independently from ingestion success
export async function updateWorkerHeartbeat(
  pool: Pool,
  input: Readonly<{
    activity: string | null;
    instance: string;
    lastLoopAt: string;
    lastSuccessAt: string | null;
    version: string;
  }>,
): Promise<void> {
  const lastLoopAt = validateUtcInstant(input.lastLoopAt, "lastLoopAt");
  const lastSuccessAt =
    input.lastSuccessAt === null
      ? null
      : validateUtcInstant(input.lastSuccessAt, "lastSuccessAt");

  // require bounded heartbeat fields
  if (
    input.instance.length === 0 ||
    input.instance.length > 128 ||
    input.version.length === 0 ||
    input.version.length > 128 ||
    (input.activity !== null && input.activity.length > 256)
  ) {
    throw new RangeError("heartbeat fields must be non-empty and bounded");
  }

  await pool.query(
    `
      INSERT INTO worker_heartbeats (
        worker_instance,
        last_loop_at,
        last_success_at,
        current_activity,
        worker_version
      )
      VALUES ($1, $2, $3, $4, $5)
      ON CONFLICT (worker_instance) DO UPDATE SET
        last_loop_at = EXCLUDED.last_loop_at,
        last_success_at = EXCLUDED.last_success_at,
        current_activity = EXCLUDED.current_activity,
        worker_version = EXCLUDED.worker_version,
        updated_at = clock_timestamp()
    `,
    [input.instance, lastLoopAt, lastSuccessAt, input.activity, input.version],
  );
}

// list active site and source metadata
export async function listActiveSites(pool: Pool): Promise<readonly ActiveSiteRow[]> {
  const result = await pool.query<ActiveSiteRow>(
    `
      SELECT
        si.slug AS "siteSlug",
        si.display_name AS "siteName",
        si.latitude,
        si.longitude,
        si.timezone,
        st.slug AS "stationSlug",
        st.display_name AS "stationName",
        st.station_kind AS "stationKind",
        st.latitude AS "stationLatitude",
        st.longitude AS "stationLongitude",
        s.id AS "sourceId",
        s.source_key AS "sourceKey",
        s.source_kind AS "sourceKind",
        p.provider_key AS "providerKey",
        p.display_name AS "providerName",
        p.attribution_label AS "attributionLabel",
        p.attribution_url AS "attributionUrl"
      FROM sites si
      JOIN stations st ON st.site_id = si.id AND st.active
      JOIN sources s ON s.station_id = st.id AND s.active
      JOIN providers p ON p.id = s.provider_id AND p.active
      WHERE si.active
        AND (
          s.source_kind <> 'forecast'
          OR s.capabilities @> '["forecast"]'::jsonb
        )
        AND ${CURRENT_SOURCE_PREDICATE}
      ORDER BY si.slug, st.slug, s.source_key
    `,
  );

  return result.rows;
}

// read the newest worker loop
export async function getLatestWorkerHeartbeat(
  pool: Pool,
): Promise<LatestWorkerHeartbeat | null> {
  const result = await pool.query<LatestWorkerHeartbeat>(
    `
      SELECT last_loop_at AS "lastLoopAt"
      FROM worker_heartbeats
      ORDER BY last_loop_at DESC, worker_instance ASC
      LIMIT 1
    `,
  );

  return result.rows[0] ?? null;
}

// read the latest row per source
export async function getCurrentWeather(
  pool: Pool,
  siteSlug: string,
  query: CurrentQuery = {},
): Promise<readonly CurrentWeatherRecordRow[]> {
  const result = await pool.query<CurrentWeatherRecordRow>(
    `
      SELECT
        ${weatherRecordSelection()},
        -- require complete three-hour evidence
        CASE
          WHEN wr.pressure_hpa IS NULL
            OR pressure_before.pressure_hpa IS NULL
            OR pressure_after.pressure_hpa IS NULL
          THEN NULL
          WHEN pressure_before.valid_at = pressure_after.valid_at
          THEN wr.pressure_hpa - pressure_before.pressure_hpa
          ELSE wr.pressure_hpa - (
            pressure_before.pressure_hpa
            + (pressure_after.pressure_hpa - pressure_before.pressure_hpa)
              * EXTRACT(EPOCH FROM (wr.valid_at - interval '3 hours' - pressure_before.valid_at))
              / EXTRACT(EPOCH FROM (pressure_after.valid_at - pressure_before.valid_at))
          )
        END AS "pressureChange3hHpa"
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      JOIN LATERAL (
        SELECT ${weatherRecordLateralSelection()}
        FROM weather_records candidate
        WHERE candidate.source_id = s.id
        ORDER BY candidate.valid_at DESC, candidate.id DESC
        LIMIT 1
      ) wr ON true
      -- find pressure immediately before target
      LEFT JOIN LATERAL (
        SELECT candidate.valid_at, candidate.pressure_hpa
        FROM weather_records candidate
        WHERE candidate.source_id = wr.source_id
          AND candidate.pressure_hpa IS NOT NULL
          AND candidate.valid_at <= wr.valid_at - interval '3 hours'
          AND candidate.valid_at >= wr.valid_at - interval '3 hours 30 minutes'
        ORDER BY candidate.valid_at DESC, candidate.id DESC
        LIMIT 1
      ) pressure_before ON true
      -- find pressure immediately after target
      LEFT JOIN LATERAL (
        SELECT candidate.valid_at, candidate.pressure_hpa
        FROM weather_records candidate
        WHERE candidate.source_id = wr.source_id
          AND candidate.pressure_hpa IS NOT NULL
          AND candidate.valid_at >= wr.valid_at - interval '3 hours'
          AND candidate.valid_at <= wr.valid_at - interval '2 hours 30 minutes'
        ORDER BY candidate.valid_at ASC, candidate.id ASC
        LIMIT 1
      ) pressure_after ON true
      WHERE si.slug = $1
        AND si.active
        AND st.active
        AND s.active
        AND p.active
        AND s.source_kind IN ('physical_sensor', 'model_current', 'reanalysis')
        AND ${CURRENT_SOURCE_PREDICATE}
        AND ($2::text IS NULL OR st.slug = $2)
        AND ($3::bigint IS NULL OR s.id = $3)
      ORDER BY wr.source_id
    `,
    [siteSlug, query.stationSlug ?? null, query.sourceId ?? null],
  );

  return result.rows;
}

// read the latest bounded forecast product
export async function getWeatherForecast(
  pool: Pool,
  query: ForecastQuery,
): Promise<readonly WeatherRecordRow[]> {
  const asOf = validateUtcInstant(query.asOf, "asOf");
  const productSelection = query.productSelection ?? "latest";

  // require a bounded public horizon
  if (!Number.isSafeInteger(query.hours) || query.hours < 1 || query.hours > MAX_FORECAST_HOURS) {
    throw new RangeError(`forecast hours must be between 1 and ${String(MAX_FORECAST_HOURS)}`);
  }

  // reject selectors outside the two reviewed product policies
  if (productSelection !== "anchor-containing" && productSelection !== "latest") {
    throw new RangeError("forecast product selection is invalid");
  }

  const endExclusive = new Date(
    Date.parse(asOf) + query.hours * 3_600_000,
  ).toISOString();
  // choose either the compatibility product or the newest anchor-bearing vintage
  const productRunPredicate = productSelection === "anchor-containing"
    ? `candidate.product_run_at = (
            SELECT MAX(product.product_run_at)
            FROM weather_records product
            WHERE product.source_id = s.id
              AND product.source_kind = 'forecast'
              AND product.valid_at = $2
          )`
    : `candidate.product_run_at = (
            SELECT product.product_run_at
            FROM public.forecast_products product
            WHERE product.source_id = s.id
            ORDER BY product.product_run_at DESC
            LIMIT 1
          )`;
  const rawResult = await pool.query<WeatherRecordRow>(
    `
      SELECT
        ${weatherRecordSelection()}
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      JOIN LATERAL (
        SELECT ${weatherRecordLateralSelection()}
        FROM weather_records candidate
        WHERE candidate.source_id = s.id
          -- expose the existing identity-index prefix before filtering old runs
          AND candidate.source_kind = 'forecast'
          AND ${productRunPredicate}
          AND candidate.valid_at >= $2
          AND candidate.valid_at < $3
        ORDER BY candidate.valid_at ASC, candidate.id ASC
        LIMIT ${String(MAX_FORECAST_HOURS)}
      ) wr ON true
      WHERE si.slug = $1
        AND si.active
        AND st.active
        AND s.active
        AND p.active
        AND s.source_kind = 'forecast'
        AND s.capabilities @> '["forecast"]'::jsonb
        AND ${CURRENT_SOURCE_PREDICATE}
      ORDER BY wr.valid_at ASC, wr.id ASC
    `,
    [query.siteSlug, asOf, endExclusive],
  );

  // skip optional provenance without raw records
  if (rawResult.rows.length === 0) {
    return rawResult.rows;
  }

  // fail raw when optional provenance is unavailable
  try {
    const provenanceResult = await pool.query<ForecastRuntimeProvenanceRow>(
      `
        SELECT provenance.*
        FROM unnest($1::bigint[]) AS requested(weather_record_id)
        -- probe exact identities instead of scanning historical provenance
        CROSS JOIN LATERAL (
          SELECT
            weather_record_id AS "weatherRecordId",
            source_id AS "sourceId",
            source_key AS "sourceKey",
            source_config_fingerprint AS "sourceConfigFingerprint",
            adapter_version AS "adapterVersion",
            contract_epoch AS "contractEpoch"
          FROM forecast_runtime_provenance_v1
          WHERE weather_record_id = requested.weather_record_id
          -- retain duplicate-linkage detection without flattening the probe
          LIMIT 2
        ) provenance
        ORDER BY provenance."weatherRecordId" ASC
        LIMIT ${String(MAX_FORECAST_HOURS)}
      `,
      [
        rawResult.rows.map(
          // bind only returned record identifiers
          (record) => record.id,
        ),
      ],
    );
    const rawByRecordId = new Map(
      rawResult.rows.map(
        // retain exact source linkage
        (record) => [record.id, record] as const,
      ),
    );
    const provenanceByRecordId = new Map<string, ForecastRuntimeProvenanceRow>();

    // validate every optional linkage before decoration
    for (const provenance of provenanceResult.rows) {
      const rawRecord = rawByRecordId.get(provenance.weatherRecordId);

      // reject partial or duplicate linkage
      if (
        rawRecord === undefined
        || rawRecord.sourceId !== provenance.sourceId
        || rawRecord.sourceKey !== provenance.sourceKey
        || provenanceByRecordId.has(provenance.weatherRecordId)
      ) {
        throw new Error("forecast runtime provenance linkage is invalid");
      }

      provenanceByRecordId.set(provenance.weatherRecordId, provenance);
    }

    return rawResult.rows.map(
      // decorate matching records without changing ordering
      (record) => {
        const provenance = provenanceByRecordId.get(record.id);

        // retain the authoritative raw record when unmatched
        if (provenance === undefined) {
          return record;
        }

        return {
          ...record,
          adapterVersion: provenance.adapterVersion,
          contractEpoch: provenance.contractEpoch,
          sourceConfigFingerprint: provenance.sourceConfigFingerprint,
        };
      },
    );
  } catch {
    return rawResult.rows;
  }
}

// qualify covering-index identities before fetching exact retained midnight rows
export async function getForecastPressureContext(
  pool: Pool,
  query: ForecastPressureContextQuery,
): Promise<readonly WeatherRecordRow[]> {
  const asOf = validateUtcInstant(query.asOf, "asOf");
  const contextStart = new Date(
    Date.parse(asOf) - 3 * HOUR_MILLISECONDS,
  ).toISOString();
  const contextEnd = new Date(
    Date.parse(asOf) + 3 * HOUR_MILLISECONDS,
  ).toISOString();
  const oldestRun = new Date(
    Date.parse(asOf) - 48 * HOUR_MILLISECONDS,
  ).toISOString();
  const result = await pool.query<WeatherRecordRow>(
    `
      SELECT
        ${weatherRecordSelection()}
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      JOIN LATERAL (
        WITH slots AS MATERIALIZED (
          SELECT generated.slot
          FROM generate_series(
            $2::timestamptz,
            $3::timestamptz - interval '1 hour',
            interval '1 hour'
          ) AS generated(slot)
        ), identity_keys AS MATERIALIZED (
          -- singleton boundaries use exact covering-index suffix ranges
          SELECT
            point.source_id,
            point.source_kind,
            point.valid_at,
            point.product_run_at
          FROM slots
          CROSS JOIN LATERAL (
            SELECT
              identity.source_id,
              identity.source_kind,
              identity.valid_at,
              identity.product_run_at
            FROM weather_records identity
            WHERE identity.source_id = s.id
              AND identity.source_kind = 'forecast'
              AND ROW(identity.valid_at, identity.product_run_at)
                >= ROW(slots.slot, $4::timestamptz)
              AND ROW(identity.valid_at, identity.product_run_at)
                <= ROW(slots.slot, $5::timestamptz)
              AND identity.product_run_at >= $4
              AND identity.product_run_at <= $5
            -- retain the parameterized range without removing rows
            OFFSET 0
          ) point
          UNION ALL
          -- strict gaps preserve every nonhour identity without duplicating boundaries
          SELECT
            gap.source_id,
            gap.source_kind,
            gap.valid_at,
            gap.product_run_at
          FROM slots
          CROSS JOIN LATERAL (
            SELECT
              identity.source_id,
              identity.source_kind,
              identity.valid_at,
              identity.product_run_at
            FROM weather_records identity
            WHERE identity.source_id = s.id
              AND identity.source_kind = 'forecast'
              AND identity.valid_at > slots.slot
              AND identity.valid_at < slots.slot + interval '1 hour'
              AND identity.product_run_at >= $4
              AND identity.product_run_at <= $5
            -- retain the parameterized range without removing rows
            OFFSET 0
          ) gap
        )
        SELECT product.product_run_at, qualified.record_ids
        FROM public.forecast_products product
        JOIN LATERAL (
          SELECT
            array_agg(raw.id ORDER BY raw.valid_at ASC, raw.id ASC) AS record_ids
          FROM identity_keys identity
          JOIN weather_records raw
            ON raw.source_id = identity.source_id
            AND raw.source_kind = 'forecast'
            -- equal inclusive bounds retain every component of the unique identity
            AND ROW(raw.valid_at, raw.product_run_at) >= ROW(
              identity.valid_at, identity.product_run_at
            )
            AND ROW(raw.valid_at, raw.product_run_at) <= ROW(
              identity.valid_at, identity.product_run_at
            )
          WHERE identity.source_id = product.source_id
            AND identity.product_run_at = product.product_run_at
            AND raw.pressure_hpa > '-Infinity'::double precision
            AND raw.pressure_hpa < 'Infinity'::double precision
          HAVING COUNT(*) = 6
            AND MIN(raw.valid_at) = $2
            AND MAX(raw.valid_at) = $3::timestamptz - interval '1 hour'
            AND BOOL_AND(raw.valid_at = date_trunc('hour', raw.valid_at))
        ) qualified ON true
        WHERE product.source_id = s.id
          AND product.product_run_at >= $4
          AND product.product_run_at <= $5
        ORDER BY product.product_run_at DESC
        LIMIT 1
      ) context_run ON true
      JOIN LATERAL (
        SELECT ${weatherRecordLateralSelection()}
        FROM weather_records candidate
        WHERE candidate.id = ANY(context_run.record_ids)
        ORDER BY candidate.valid_at ASC, candidate.id ASC
      ) wr ON true
      WHERE si.slug = $1
        AND si.active
        AND st.active
        AND s.active
        AND p.active
        AND s.source_kind = 'forecast'
        AND s.source_key = 'open-meteo-forecast-v4'
        AND s.capabilities @> '["forecast"]'::jsonb
        AND ${CURRENT_SOURCE_PREDICATE}
      ORDER BY wr.valid_at ASC, wr.id ASC
      LIMIT 6
    `,
    [query.siteSlug, contextStart, contextEnd, oldestRun, asOf],
  );

  return result.rows;
}

// read provenance-separated forecast training cohorts
export async function listForecastTrainingCohorts(
  pool: Pool,
  query: ForecastTrainingQuery,
): Promise<ForecastTrainingCohorts> {
  const { from, to } = validateForecastTrainingWindow(query.from, query.to);
  const anchors = await pool.query<ForecastAnchorStorageRow>(
    `
      SELECT
        far.source_id AS "sourceId",
        far.source_config_fingerprint AS "sourceConfigFingerprint",
        far.valid_at AS "validAt",
        far.lead_hours AS "leadHours",
        far.dataset,
        far.upstream_model AS "upstreamModel",
        far.contract_epoch AS "contractEpoch",
        far.adapter_version AS "adapterVersion",
        far.last_received_at AS "receivedAt",
        far.upstream_timezone AS "upstreamTimezone",
        far.quality_metadata AS "qualityMetadata",
        far.provider_metadata AS "providerMetadata",
        far.temperature_c AS "temperatureC",
        far.apparent_temperature_c AS "apparentTemperatureC",
        far.precipitation_mm AS "precipitationMm",
        far.wind_speed_mps AS "windSpeedMps",
        far.wind_gust_mps AS "windGustMps",
        far.pressure_hpa AS "pressureHpa",
        far.relative_humidity_percent AS "relativeHumidityPercent",
        far.cloud_cover_percent AS "cloudCoverPercent",
        far.wind_direction_degrees AS "windDirectionDegrees"
      FROM forecast_anchor_records far
      JOIN sources s ON s.id = far.source_id
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      WHERE si.slug = $1
        AND far.valid_at >= $2
        AND far.valid_at < $3
        AND far.source_kind = 'forecast'
        AND far.dataset = 'previous_runs'
        AND far.upstream_model = 'best_match'
        AND s.capabilities @> '["historical"]'::jsonb
      ORDER BY far.valid_at ASC, far.lead_hours ASC, far.id ASC
    `,
    [query.siteSlug, from, to],
  );
  const retrievals = await pool.query<LegacyV4ForecastStorageRow>(
    `
      SELECT
        wr.id AS "stableRecordId",
        wr.source_id AS "sourceId",
        s.source_config_fingerprint AS "sourceConfigFingerprint",
        wr.valid_at AS "validAt",
        wr.product_run_at AS "referenceAt",
        wr.upstream_model AS "upstreamModel",
        wr.provider_metadata ->> 'dataset' AS dataset,
        last_run.adapter_version AS "adapterVersion",
        wr.temperature_c AS "temperatureC",
        wr.apparent_temperature_c AS "apparentTemperatureC",
        wr.precipitation_mm AS "precipitationMm",
        wr.wind_speed_mps AS "windSpeedMps",
        wr.wind_gust_mps AS "windGustMps",
        wr.pressure_hpa AS "pressureHpa",
        wr.relative_humidity_percent AS "relativeHumidityPercent",
        wr.cloud_cover_percent AS "cloudCoverPercent",
        wr.wind_direction_degrees AS "windDirectionDegrees",
        wr.black_globe_temperature_c AS "blackGlobeTemperatureC",
        wr.pm25_micrograms_per_cubic_meter AS "pm25MicrogramsPerCubicMeter",
        wr.precipitation_rate_mm_per_hour AS "precipitationRateMmPerHour",
        wr.soil_electrical_conductivity_us_cm AS "soilElectricalConductivityMicrosiemensPerCm",
        wr.soil_moisture_percent AS "soilMoisturePercent",
        wr.solar_radiation_wm2 AS "solarRadiationWm2",
        wr.uv_index AS "uvIndex",
        wr.wet_bulb_globe_temperature_c AS "wetBulbGlobeTemperatureC",
        wr.water_level_m AS "waterLevelM"
      FROM weather_records wr
      JOIN sources s ON s.id = wr.source_id
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN ingestion_runs last_run ON last_run.id = wr.last_ingestion_run_id
      WHERE si.slug = $1
        AND wr.valid_at >= $2
        AND wr.valid_at < $3
        AND s.source_key = 'open-meteo-forecast-v4'
        AND s.source_kind = 'forecast'
        AND s.capabilities @> '["forecast"]'::jsonb
      ORDER BY wr.valid_at ASC, wr.product_run_at ASC, wr.id ASC
    `,
    [query.siteSlug, from, to],
  );

  return {
    fixedLeadAnchors: anchors.rows.map(projectFixedLeadAnchorStorageRow),
    legacyV4RetrievalSnapshots: projectLegacyV4RetrievalRows(retrievals.rows),
  };
}

// read deterministic physical-station hours without spatial fitting
export async function listForecastObservationHourlyStations(
  pool: Pool,
  query: ForecastTrainingQuery,
): Promise<readonly ForecastObservationHourlyStationRow[]> {
  const { from, to } = validateForecastTrainingWindow(query.from, query.to);

  // require exact hourly projection boundaries
  if (Date.parse(from) % HOUR_MILLISECONDS !== 0 || Date.parse(to) % HOUR_MILLISECONDS !== 0) {
    throw new RangeError("forecast observation projection bounds must align to UTC hours");
  }

  const sourceKeys = FORECAST_OBSERVATION_SOURCE_LINEAGES.map(
    (lineage) => lineage.sourceKey,
  );
  const result = await pool.query<ForecastObservationStorageRow>(
    `
      SELECT
        wr.id,
        wr.valid_at AS "validAt",
        wr.quality_metadata AS "qualityMetadata",
        wr.temperature_c AS "temperatureC",
        wr.relative_humidity_percent AS "relativeHumidityPercent",
        wr.wind_speed_mps AS "windSpeedMps",
        wr.wind_gust_mps AS "windGustMps",
        wr.wind_direction_degrees AS "windDirectionDegrees",
        s.source_key AS "sourceKey",
        s.source_config_fingerprint AS "sourceConfigFingerprint",
        s.material_provider_config ->> 'contractVersion' AS "adapterContract",
        st.slug AS "stationSlug"
      FROM weather_records wr
      JOIN sources s ON s.id = wr.source_id
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      WHERE si.slug = $1
        AND wr.valid_at >= $2::timestamptz - interval '1 hour'
        AND wr.valid_at < $3::timestamptz + interval '5 minutes'
        AND s.source_kind = 'physical_sensor'
        AND s.source_key = ANY($4::text[])
      ORDER BY wr.valid_at ASC, wr.id ASC
    `,
    [query.siteSlug, from, to, sourceKeys],
  );
  const records = validateForecastObservationRows(result.rows);

  return projectForecastObservationHours(records, from, to);
}

// read causal station hours available by one initialization
export async function listCausalForecastObservationHourlyStations(
  pool: Pool,
  query: ForecastTrainingQuery & Readonly<{ asOf: string }>,
): Promise<readonly ForecastObservationHourlyStationRow[]> {
  const { from, to } = validateForecastTrainingWindow(query.from, query.to);
  const asOf = validateUtcInstant(query.asOf, "asOf");

  // require exact hourly projection boundaries
  if (Date.parse(from) % HOUR_MILLISECONDS !== 0 || Date.parse(to) % HOUR_MILLISECONDS !== 0) {
    throw new RangeError("causal observation projection bounds must align to UTC hours");
  }

  const sourceKeys = FORECAST_OBSERVATION_SOURCE_LINEAGES.map(
    (lineage) => lineage.sourceKey,
  );
  const result = await pool.query<ForecastObservationStorageRow>(
    `
      SELECT
        wr.id,
        wr.valid_at AS "validAt",
        wr.quality_metadata AS "qualityMetadata",
        wr.temperature_c AS "temperatureC",
        wr.relative_humidity_percent AS "relativeHumidityPercent",
        wr.wind_speed_mps AS "windSpeedMps",
        wr.wind_gust_mps AS "windGustMps",
        wr.wind_direction_degrees AS "windDirectionDegrees",
        s.source_key AS "sourceKey",
        s.source_config_fingerprint AS "sourceConfigFingerprint",
        s.material_provider_config ->> 'contractVersion' AS "adapterContract",
        st.slug AS "stationSlug"
      FROM weather_records wr
      JOIN sources s ON s.id = wr.source_id
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      WHERE si.slug = $1
        AND wr.valid_at >= $2::timestamptz - interval '5 minutes'
        AND wr.valid_at < $3::timestamptz + interval '5 minutes'
        AND wr.first_received_at <= $4
        AND wr.last_received_at <= $4
        AND s.source_kind = 'physical_sensor'
        AND s.source_key = ANY($5::text[])
      ORDER BY wr.valid_at ASC, wr.id ASC
    `,
    [query.siteSlug, from, to, asOf, sourceKeys],
  );
  const records = validateForecastObservationRows(result.rows);

  return projectForecastObservationHours(records, from, to);
}

// persist one immutable private run and its exact hours
export async function persistEcmwfTemperatureCanaryRun(
  pool: Pool,
  input: PersistEcmwfTemperatureCanaryRunInput,
  revisionArchiver?: EcmwfTemperatureRevisionArchiver,
): Promise<EcmwfTemperatureCanaryRun> {
  const validated = validateEcmwfTemperatureCanaryRunInput(input);
  const client = await pool.connect();

  try {
    const committed = await withTransaction(client, async () => {
      const site = await client.query<{ id: string; latitude: number; longitude: number }>(
        "SELECT id, latitude, longitude FROM sites WHERE slug = $1 AND active",
        [validated.siteSlug],
      );
      const siteRow = requireRow(site.rows[0], "ECMWF temperature canary site");
      const siteId = siteRow.id;
      const inserted = await client.query<EcmwfTemperatureCanaryRunStorageRow>(
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
          VALUES (
            $1, $2, $3, $3, $4, $5, $6, $7, $8::jsonb, $9, $10, $11, $12
          )
          ON CONFLICT (site_id, run_initialized_at) DO NOTHING
          RETURNING ${ecmwfTemperatureCanaryRunSelection()}
        `,
        [
          siteId,
          validated.runInitializedAt,
          validated.receivedAt,
          validated.upstreamModel,
          validated.adapterVersion,
          validated.providerResponseSha256,
          validated.modelCycle,
          JSON.stringify(validated.recentErrorState),
          validated.recentErrorStateSha256,
          validated.stateStatus,
          validated.stateReason,
          validated.contentHash,
        ],
      );
      let stored = inserted.rows[0];
      const newlyInserted = stored !== undefined;

      // insert hours only for a newly claimed initialization
      if (stored !== undefined) {
        // retain every exact lead atomically
        for (const hour of validated.hours) {
          await client.query(
            `
              INSERT INTO ecmwf_temperature_canary_hours (
                run_id,
                valid_at,
                model_lead_hours,
                raw_temperature_c,
                raw_relative_humidity_percent,
                raw_wind_speed_mps,
                content_hash
              )
              VALUES ($1, $2, $3, $4, $5, $6, $7)
            `,
            [
              stored.id,
              hour.validAt,
              hour.modelLeadHours,
              hour.rawTemperatureC,
              hour.rawRelativeHumidityPercent,
              hour.rawWindSpeedMps,
              hour.contentHash,
            ],
          );
        }
      } else {
        const existing = await client.query<EcmwfTemperatureCanaryRunStorageRow>(
          `
            SELECT ${ecmwfTemperatureCanaryRunSelection()}
            FROM ecmwf_temperature_canary_runs
            WHERE site_id = $1 AND run_initialized_at = $2
            FOR UPDATE
          `,
          [siteId, validated.runInitializedAt],
        );
        stored = requireRow(existing.rows[0], "existing ECMWF temperature canary run");

        // reject provider revisions and causal-state drift
        if (stored.contentHash !== validated.contentHash) {
          throw new Error("ECMWF temperature canary run revision rejected");
        }

        const existingHours = await client.query<EcmwfTemperatureCanaryHourStorageRow>(
          `
            SELECT
              valid_at AS "validAt",
              model_lead_hours AS "modelLeadHours",
              raw_temperature_c AS "rawTemperatureC",
              raw_relative_humidity_percent AS "rawRelativeHumidityPercent",
              raw_wind_speed_mps AS "rawWindSpeedMps",
              content_hash AS "contentHash"
            FROM ecmwf_temperature_canary_hours
            WHERE run_id = $1
            ORDER BY model_lead_hours ASC
          `,
          [stored.id],
        );

        // reject incomplete or altered persisted hours
        if (
          existingHours.rows.length !== validated.hours.length ||
          existingHours.rows.some(
            (row, index) => row.contentHash !== validated.hours[index]?.contentHash,
          )
        ) {
          throw new Error("ECMWF temperature canary stored hours do not match retry");
        }

        await client.query(
          `
            UPDATE ecmwf_temperature_canary_runs
            SET last_received_at = GREATEST(last_received_at, $2)
            WHERE id = $1
          `,
          [stored.id, validated.receivedAt],
        );
      }

      let publication: EcmwfTemperatureRevisionPublication | null = null;
      // archive only a newly inserted revision and never backfill a legacy null pointer
      if (newlyInserted && revisionArchiver !== undefined) {
        const received = await client.query<{ logicalReceivedAt: Date }>(
          "SELECT clock_timestamp() AS \"logicalReceivedAt\"",
        );
        const logicalReceivedAt = requireRow(received.rows[0],
          "ECMWF temperature revision receipt").logicalReceivedAt.toISOString();
        await client.query("SAVEPOINT ecmwf_temperature_revision_archive");
        try {
          const bestMatchRows = await selectEcmwfTemperatureBestMatchRows(
            client,
            validated.siteSlug,
            validated.runInitializedAt,
            logicalReceivedAt,
          );
          publication = await revisionArchiver(client, {
            adapterVersion: validated.adapterVersion,
            bestMatchRows,
            contentHash: stored.contentHash,
            hours: validated.hours,
            logicalReceivedAt,
            modelCycle: validated.modelCycle,
            providerResponseSha256: stored.providerResponseSha256,
            recentErrorState: validated.recentErrorState,
            recentErrorStateSha256: validated.recentErrorStateSha256,
            runInitializedAt: storageInstant(stored.runInitializedAt, "runInitializedAt"),
            siteId,
            siteLatitude: siteRow.latitude,
            siteLongitude: siteRow.longitude,
            siteSlug: validated.siteSlug,
            upstreamModel: validated.upstreamModel,
          });
          await client.query("RELEASE SAVEPOINT ecmwf_temperature_revision_archive");
        } catch {
          // retain source hours while removing aborted SQL and partial archive writes
          await client.query("ROLLBACK TO SAVEPOINT ecmwf_temperature_revision_archive");
          await client.query("RELEASE SAVEPOINT ecmwf_temperature_revision_archive");
          publication = null;
        }
      }
      return { publication, run: projectEcmwfTemperatureCanaryRun(stored) };
    });
    // publish staged bytes only after the serving transaction commits
    if (committed.publication !== null) {
      try {
        await committed.publication.publish();
      } catch {
        // preserve serving after a permanent cold publication gap
      }
    }
    return committed.run;
  } finally {
    client.release();
  }
}

// select the exact best-match product visible at the source-decision clock
async function selectEcmwfTemperatureBestMatchRows(
  queryable: Queryable,
  siteSlug: string,
  runInitializedAt: string,
  decisionAt: string,
): Promise<readonly WeatherRecordRow[]> {
  const from = new Date(Date.parse(runInitializedAt) + 7 * HOUR_MILLISECONDS).toISOString();
  const to = new Date(Date.parse(runInitializedAt) + 19 * HOUR_MILLISECONDS).toISOString();
  const result = await queryable.query<WeatherRecordRow>(
    `
      SELECT
        ${weatherRecordSelection()}
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      JOIN LATERAL (
        SELECT candidate.product_run_at
        FROM weather_records candidate
        WHERE candidate.source_id = s.id
          AND candidate.source_kind = 'forecast'
          AND candidate.product_run_at IS NOT NULL
          AND candidate.valid_at >= $2
          AND candidate.valid_at < $3
          AND candidate.first_received_at <= $4
          AND candidate.last_received_at <= $4
          AND candidate.upstream_model = 'best_match'
          AND candidate.provider_metadata ->> 'dataset' = 'best_match'
        GROUP BY candidate.product_run_at
        HAVING COUNT(*) = 12
          AND MIN(candidate.valid_at) = $2
          AND MAX(candidate.valid_at) = $3::timestamptz - interval '1 hour'
          AND BOOL_AND(candidate.valid_at = date_trunc('hour', candidate.valid_at))
        ORDER BY candidate.product_run_at DESC
        LIMIT 1
      ) selected ON true
      JOIN weather_records wr ON wr.source_id = s.id
        AND wr.source_kind = 'forecast'
        AND wr.product_run_at = selected.product_run_at
        AND wr.valid_at >= $2
        AND wr.valid_at < $3
        AND wr.first_received_at <= $4
        AND wr.last_received_at <= $4
        AND wr.upstream_model = 'best_match'
        AND wr.provider_metadata ->> 'dataset' = 'best_match'
      WHERE si.slug = $1
        AND si.active
        AND st.active
        AND s.active
        AND p.active
        AND s.source_kind = 'forecast'
        AND s.source_key = 'open-meteo-forecast-v4'
        AND s.capabilities @> '["forecast"]'::jsonb
        AND ${CURRENT_SOURCE_PREDICATE}
      ORDER BY wr.valid_at ASC, wr.id ASC
      LIMIT 12
    `,
    [siteSlug, from, to, decisionAt],
  );
  const first = result.rows[0];
  // return no partial geometry for a later permanent source gap
  if (first === undefined || first.productRunAt === null || result.rows.length !== 12 ||
      result.rows.some((row, index) => row.sourceId !== first.sourceId ||
        row.productRunAt === null ||
        storageInstant(row.productRunAt, "productRunAt") !==
          storageInstant(first.productRunAt!, "productRunAt") ||
        storageInstant(row.validAt, "validAt") !==
          new Date(Date.parse(from) + index * HOUR_MILLISECONDS).toISOString() ||
        !/^[a-f0-9]{64}$/u.test(row.contentHash))) {
    return [];
  }
  return result.rows;
}

// select one latest available run without hour mixing
export async function getEcmwfTemperatureCanarySidecar(
  pool: Pool,
  query: Readonly<{
    asOf: string;
    from: string;
    siteSlug: string;
    to: string;
  }>,
): Promise<EcmwfTemperatureCanarySidecar | null> {
  const asOf = validateUtcInstant(query.asOf, "asOf");
  const from = validateUtcInstant(query.from, "from");
  const to = validateUtcInstant(query.to, "to");

  // require one ordered bounded horizon
  if (from >= to || Date.parse(to) - Date.parse(from) > MAX_FORECAST_HOURS * HOUR_MILLISECONDS) {
    throw new RangeError("ECMWF temperature canary horizon is invalid");
  }

  const runs = await pool.query<EcmwfTemperatureCanaryRunStorageRow>(
    `
      SELECT ${ecmwfTemperatureCanaryRunSelection("run")}
      FROM ecmwf_temperature_canary_runs run
      JOIN sites site ON site.id = run.site_id
      WHERE site.slug = $1
        AND site.active
        AND run.first_received_at <= $2
        AND run.run_initialized_at + interval '6 hours' <= $2
      ORDER BY run.run_initialized_at DESC, run.first_received_at DESC, run.id DESC
      LIMIT 1
    `,
    [query.siteSlug, asOf],
  );
  const stored = runs.rows[0];

  // preserve raw when no eligible model run exists
  if (stored === undefined) {
    return null;
  }

  const hours = await pool.query<EcmwfTemperatureCanaryHourStorageRow>(
    `
      SELECT
        valid_at AS "validAt",
        model_lead_hours AS "modelLeadHours",
        raw_temperature_c AS "rawTemperatureC",
        raw_relative_humidity_percent AS "rawRelativeHumidityPercent",
        raw_wind_speed_mps AS "rawWindSpeedMps",
        content_hash AS "contentHash"
      FROM ecmwf_temperature_canary_hours
      WHERE run_id = $1
        AND model_lead_hours BETWEEN 7 AND 18
        AND valid_at >= $2
        AND valid_at < $3
      ORDER BY valid_at ASC
    `,
    [stored.id, from, to],
  );

  return {
    hours: hours.rows.map(projectEcmwfTemperatureCanaryHour),
    run: projectEcmwfTemperatureCanaryRun(stored),
  };
}

// expose one bounded collection status row
export async function getEcmwfTemperatureCanaryStatus(
  pool: Pool,
  query: Readonly<{ asOf: string; siteSlug: string }>,
): Promise<EcmwfTemperatureCanaryStatus | null> {
  const asOf = validateUtcInstant(query.asOf, "asOf");
  const result = await pool.query<EcmwfTemperatureCanaryRunStorageRow>(
    `
      SELECT
        ${ecmwfTemperatureCanaryRunSelection("run")},
        count(hour.run_id)::integer AS "hourCount"
      FROM ecmwf_temperature_canary_runs run
      JOIN sites site ON site.id = run.site_id
      JOIN ecmwf_temperature_canary_hours hour ON hour.run_id = run.id
      WHERE site.slug = $1
        AND site.active
        AND run.first_received_at <= $2
      GROUP BY run.id
      ORDER BY run.run_initialized_at DESC, run.first_received_at DESC, run.id DESC
      LIMIT 1
    `,
    [query.siteSlug, asOf],
  );
  const row = result.rows[0];

  // preserve an empty collector state
  if (row === undefined) {
    return null;
  }

  return {
    firstReceivedAt: storageInstant(row.firstReceivedAt, "firstReceivedAt"),
    hourCount: Number(row.hourCount),
    latestRunInitializedAt: storageInstant(
      row.runInitializedAt,
      "runInitializedAt",
    ),
    stateReason: row.stateReason,
    stateStatus: requireEcmwfTemperatureCanaryStateStatus(row.stateStatus),
  };
}

// list bounded collected initialization identities for warmup planning
export async function listEcmwfTemperatureCanaryRunInitializations(
  pool: Pool,
  query: Readonly<{ from: string; siteSlug: string; to: string }>,
): Promise<readonly string[]> {
  const from = validateUtcInstant(query.from, "from");
  const to = validateUtcInstant(query.to, "to");

  // constrain collection planning to the warmup envelope
  if (
    from > to ||
    Date.parse(to) - Date.parse(from) > 7 * 86_400_000
  ) {
    throw new RangeError("ECMWF temperature canary run inventory interval is invalid");
  }

  const result = await pool.query<{ runInitializedAt: Date | string }>(
    `
      SELECT run.run_initialized_at AS "runInitializedAt"
      FROM ecmwf_temperature_canary_runs run
      JOIN sites site ON site.id = run.site_id
      WHERE site.slug = $1
        AND site.active
        AND run.run_initialized_at >= $2
        AND run.run_initialized_at <= $3
      ORDER BY run.run_initialized_at DESC
    `,
    [query.siteSlug, from, to],
  );

  return result.rows.map((row) =>
    storageInstant(row.runInitializedAt, "runInitializedAt")
  );
}

// list bounded immutable inputs for prospective outcome scoring
export async function listEcmwfTemperatureCanaryAuditRows(
  pool: Pool,
  query: Readonly<{ from: string; siteSlug: string; to: string }>,
): Promise<readonly EcmwfTemperatureCanaryAuditRow[]> {
  const from = validateUtcInstant(query.from, "from");
  const to = validateUtcInstant(query.to, "to");

  // bound one canary-window audit
  if (
    from >= to ||
    Date.parse(to) - Date.parse(from) > 15 * 86_400_000
  ) {
    throw new RangeError("ECMWF temperature canary audit interval is invalid");
  }

  const result = await pool.query<
    EcmwfTemperatureCanaryRunStorageRow & EcmwfTemperatureCanaryHourStorageRow
  >(
    `
      SELECT
        ${ecmwfTemperatureCanaryRunSelection("run")},
        hour.valid_at AS "validAt",
        hour.model_lead_hours AS "modelLeadHours",
        hour.raw_temperature_c AS "rawTemperatureC",
        hour.raw_relative_humidity_percent AS "rawRelativeHumidityPercent",
        hour.raw_wind_speed_mps AS "rawWindSpeedMps",
        hour.content_hash AS "contentHash"
      FROM ecmwf_temperature_canary_hours hour
      JOIN ecmwf_temperature_canary_runs run ON run.id = hour.run_id
      JOIN sites site ON site.id = run.site_id
      WHERE site.slug = $1
        AND site.active
        AND hour.valid_at >= $2
        AND hour.valid_at < $3
      ORDER BY run.run_initialized_at ASC, hour.model_lead_hours ASC
    `,
    [query.siteSlug, from, to],
  );

  return result.rows.map((row) => ({
    hour: projectEcmwfTemperatureCanaryHour(row),
    run: projectEcmwfTemperatureCanaryRun(row),
  }));
}

// select prior ECMWF hours with authentic availability receipts
export async function listCausalEcmwfTemperatureCanaryPriorHours(
  pool: Pool,
  query: Readonly<{
    from: string;
    siteSlug: string;
    targetRunInitializedAt: string;
    toInclusive: string;
  }>,
): Promise<readonly EcmwfTemperatureCanaryPriorHour[]> {
  const from = validateUtcInstant(query.from, "from");
  const toInclusive = validateUtcInstant(query.toInclusive, "toInclusive");
  const targetRunInitializedAt = validateUtcInstant(
    query.targetRunInitializedAt,
    "targetRunInitializedAt",
  );

  // bind one ordered 72-hour state interval
  if (
    from > toInclusive ||
    Date.parse(toInclusive) - Date.parse(from) > 71 * HOUR_MILLISECONDS
  ) {
    throw new RangeError("ECMWF causal prior-hour interval is invalid");
  }

  const result = await pool.query<EcmwfTemperatureCanaryHourStorageRow>(
    `
      SELECT DISTINCT ON (hour.valid_at)
        run.run_initialized_at AS "runInitializedAt",
        hour.valid_at AS "validAt",
        hour.model_lead_hours AS "modelLeadHours",
        hour.raw_temperature_c AS "rawTemperatureC",
        hour.raw_relative_humidity_percent AS "rawRelativeHumidityPercent",
        hour.raw_wind_speed_mps AS "rawWindSpeedMps",
        hour.content_hash AS "contentHash"
      FROM ecmwf_temperature_canary_hours hour
      JOIN ecmwf_temperature_canary_runs run ON run.id = hour.run_id
      JOIN sites site ON site.id = run.site_id
      WHERE site.slug = $1
        AND site.active
        AND hour.valid_at >= $2
        AND hour.valid_at <= $3
        AND hour.model_lead_hours BETWEEN 7 AND 18
        AND run.adapter_version = 'open-meteo-ecmwf-single-run/v1'
        AND run.run_initialized_at < $4
        AND run.first_received_at <= $4
      ORDER BY hour.valid_at ASC, run.run_initialized_at DESC, run.id DESC
    `,
    [query.siteSlug, from, toInclusive, targetRunInitializedAt],
  );

  return result.rows.map((row) => {
    const hour = projectEcmwfTemperatureCanaryHour(row);
    const runInitializedAt = storageInstant(
      requireRow(row.runInitializedAt, "prior run initialization"),
      "runInitializedAt",
    );

    return {
      ...hour,
      key: `${runInitializedAt}/${hour.validAt}`,
      runInitializedAt,
    };
  });
}

// sum today's nearest physical rain gauge
export async function getDailyPrecipitation(
  pool: Pool,
  query: DailyPrecipitationQuery,
): Promise<DailyPrecipitationRow | null> {
  const from = validateUtcInstant(query.from, "from");
  const to = validateUtcInstant(query.to, "to");

  // require an ordered local-day window
  if (from >= to) {
    throw new RangeError("daily precipitation from must be earlier than to");
  }

  const result = await pool.query<DailyPrecipitationRow>(
    `
      WITH nearest_source AS (
        SELECT
          s.id AS source_id,
          st.slug AS station_slug
        FROM sources s
        JOIN stations st ON st.id = s.station_id
        JOIN sites si ON si.id = st.site_id
        JOIN providers p ON p.id = s.provider_id
        WHERE si.slug = $1
          AND si.active
          AND st.active
          AND s.active
          AND p.active
          AND s.source_kind = 'physical_sensor'
          AND ${CURRENT_SOURCE_PREDICATE}
          AND EXISTS (
            SELECT 1
            FROM weather_records candidate
            WHERE candidate.source_id = s.id
              AND candidate.valid_at >= $2
              AND candidate.valid_at < $3
              AND candidate.precipitation_mm IS NOT NULL
          )
        ORDER BY
          POWER(st.latitude - si.latitude, 2) +
            POWER((st.longitude - si.longitude) * COS(RADIANS(si.latitude)), 2),
          st.slug ASC,
          s.id ASC
        LIMIT 1
      )
      SELECT
        SUM(wr.precipitation_mm)::double precision AS "accumulationMm",
        nearest_source.source_id AS "sourceId",
        nearest_source.station_slug AS "stationSlug",
        MAX(wr.valid_at) AS "validThrough"
      FROM nearest_source
      JOIN weather_records wr ON wr.source_id = nearest_source.source_id
      WHERE wr.valid_at >= $2
        AND wr.valid_at < $3
        AND wr.precipitation_mm IS NOT NULL
      GROUP BY nearest_source.source_id, nearest_source.station_slug
    `,
    [query.siteSlug, from, to],
  );

  return result.rows[0] ?? null;
}

// aggregate normalized recent conditions into chart buckets
export async function listWeatherTrends(
  pool: Pool,
  query: TrendQuery,
): Promise<readonly TrendPointRow[]> {
  const from = validateUtcInstant(query.from, "from");
  const to = validateUtcInstant(query.to, "to");

  // require an ordered trend window
  if (from >= to) {
    throw new RangeError("trend from must be earlier than to");
  }

  const result = await pool.query<TrendPointRow>(
    `
      WITH daily_sources AS (
        SELECT
          date_trunc('day', wr.valid_at AT TIME ZONE si.timezone) AT TIME ZONE si.timezone AS valid_at,
          s.source_kind,
          AVG(wr.temperature_c) AS temperature_c,
          MAX(wr.temperature_c) AS temperature_maximum_c,
          MIN(wr.temperature_c) AS temperature_minimum_c,
          AVG(wr.apparent_temperature_c) AS apparent_temperature_c,
          SUM(wr.precipitation_mm) AS precipitation_mm,
          AVG(wr.wind_speed_mps) AS wind_speed_mps,
          MAX(wr.wind_gust_mps) AS wind_gust_mps,
          DEGREES(ATAN2(
            AVG(SIN(RADIANS(wr.wind_direction_degrees))),
            AVG(COS(RADIANS(wr.wind_direction_degrees)))
          )) AS wind_direction_degrees,
          AVG(wr.pressure_hpa) AS pressure_hpa,
          AVG(wr.relative_humidity_percent) AS relative_humidity_percent
        FROM weather_records wr
        JOIN sources s ON s.id = wr.source_id
        JOIN stations st ON st.id = s.station_id
        JOIN sites si ON si.id = st.site_id
        JOIN providers p ON p.id = s.provider_id
        WHERE si.slug = $1
          AND wr.valid_at >= $2
          AND wr.valid_at < $3
          AND si.active
          AND st.active
          AND s.active
          AND p.active
          AND s.source_kind IN ('model_current', 'reanalysis')
          AND ${CURRENT_SOURCE_PREDICATE}
        GROUP BY 1, s.source_kind
      ),
      preferred_days AS (
        SELECT
          daily_sources.*,
          ROW_NUMBER() OVER (
            PARTITION BY valid_at
            ORDER BY CASE source_kind WHEN 'reanalysis' THEN 0 ELSE 1 END
          ) AS source_priority
        FROM daily_sources
      )
      SELECT
        valid_at AS "validAt",
        temperature_c AS "temperatureC",
        temperature_maximum_c AS "temperatureMaximumC",
        temperature_minimum_c AS "temperatureMinimumC",
        apparent_temperature_c AS "apparentTemperatureC",
        precipitation_mm AS "precipitationMm",
        wind_speed_mps AS "windSpeedMps",
        wind_gust_mps AS "windGustMps",
        wind_direction_degrees AS "windDirectionDegrees",
        pressure_hpa AS "pressureHpa",
        relative_humidity_percent AS "relativeHumidityPercent"
      FROM preferred_days
      WHERE source_priority = 1
      ORDER BY valid_at ASC
    `,
    [query.siteSlug, from, to],
  );

  return result.rows;
}

// read bounded observed and predicted tide levels
export async function listTideRecords(
  pool: Pool,
  query: TideQuery,
): Promise<readonly TideRecordRow[]> {
  const from = validateUtcInstant(query.from, "from");
  const to = validateUtcInstant(query.to, "to");
  const limit = query.limit ?? 10_000;

  // require one bounded ordered public range
  if (
    from >= to ||
    Date.parse(to) - Date.parse(from) > 31 * 86_400_000 ||
    !Number.isSafeInteger(limit) ||
    limit < 1 ||
    limit > 10_000
  ) {
    throw new RangeError("tide query range or limit is invalid");
  }

  const result = await pool.query<TideRecordRow>(
    `
      SELECT
        wr.source_id AS "sourceId",
        wr.source_kind AS "sourceKind",
        wr.valid_at AS "validAt",
        wr.water_level_m AS "waterLevelM",
        wr.provider_metadata ->> 'prediction_type' AS "predictionType",
        st.slug AS "stationSlug",
        st.display_name AS "stationName",
        p.provider_key AS "providerKey",
        p.attribution_label AS "attributionLabel",
        p.attribution_url AS "attributionUrl"
      FROM weather_records wr
      JOIN sources s ON s.id = wr.source_id
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      WHERE si.slug = $1
        AND wr.valid_at >= $2
        AND wr.valid_at < $3
        AND wr.water_level_m IS NOT NULL
        AND s.source_kind IN ('tide_observation', 'tide_prediction')
        AND si.active
        AND st.active
        AND s.active
        AND p.active
        AND ${CURRENT_SOURCE_PREDICATE}
      ORDER BY wr.valid_at ASC, wr.id ASC
      LIMIT $4
    `,
    [query.siteSlug, from, to, limit],
  );

  return result.rows;
}

// read one complete fixed-site projection input in one snapshot
export async function listForecastObservationInputs(
  pool: Pool,
  asOfValue: string,
): Promise<ForecastObservationInput | null> {
  const asOf = validateUtcInstant(asOfValue, "asOf");
  const result = await pool.query<ForecastObservationInputRow & {
    projectionFrom: Date | string;
    queryFrom: Date | string;
    siteSlug: string;
    timezone: string;
    through: Date | string;
  }>(
    `
      WITH target_site AS (
        SELECT
          si.id,
          si.slug,
          si.timezone,
          (($1::timestamptz AT TIME ZONE si.timezone)::date::timestamp
            AT TIME ZONE si.timezone) AS projection_from
        FROM sites si
        WHERE si.slug = 'ballydidean'
          AND si.timezone = 'America/Los_Angeles'
          AND si.active
      ), geometry AS (
        SELECT
          target_site.*,
          LEAST(
            target_site.projection_from - interval '3 hours',
            $1::timestamptz - interval '24 hours 20 minutes'
          ) AS query_from
        FROM target_site
      )
      SELECT
        geometry.slug AS "siteSlug",
        geometry.timezone,
        geometry.projection_from AS "projectionFrom",
        geometry.query_from AS "queryFrom",
        $1::timestamptz AS through,
        input.id,
        input.valid_at AS "validAt",
        input.last_received_at AS "lastReceivedAt",
        input.source_id AS "sourceId",
        input.source_key AS "sourceKey",
        input.source_kind AS "sourceKind",
        input.station_slug AS "stationSlug",
        input.provider_key AS "providerKey",
        input.temperature_c AS "temperatureC",
        input.apparent_temperature_c AS "apparentTemperatureC",
        input.wind_speed_mps AS "windSpeedMps",
        input.wind_gust_mps AS "windGustMps",
        input.wind_direction_degrees AS "windDirectionDegrees",
        input.precipitation_rate_mm_per_hour AS "precipitationRateMmPerHour",
        input.cloud_cover_percent AS "cloudCoverPercent",
        input.relative_humidity_percent AS "relativeHumidityPercent",
        input.pm25_micrograms_per_cubic_meter AS "pm25MicrogramsPerCubicMeter",
        input.uv_index AS "uvIndex",
        input.pressure_hpa AS "pressureHpa",
        input.solar_radiation_wm2 AS "solarRadiationWm2"
      FROM geometry
      LEFT JOIN LATERAL (
        SELECT
          wr.id, wr.valid_at, wr.last_received_at, wr.source_id,
          s.source_key, s.source_kind, st.slug AS station_slug,
          p.provider_key, wr.temperature_c, wr.apparent_temperature_c,
          wr.wind_speed_mps, wr.wind_gust_mps, wr.wind_direction_degrees,
          wr.precipitation_rate_mm_per_hour, wr.cloud_cover_percent,
          wr.relative_humidity_percent, wr.pm25_micrograms_per_cubic_meter,
          wr.uv_index, wr.pressure_hpa, wr.solar_radiation_wm2
        FROM stations st
        JOIN sources s ON s.station_id = st.id
        JOIN providers p ON p.id = s.provider_id
        JOIN weather_records wr ON wr.source_id = s.id
        WHERE st.site_id = geometry.id
          AND st.active
          AND s.active
          AND p.active
          AND s.source_kind IN ('physical_sensor', 'model_current')
          AND ${CURRENT_SOURCE_PREDICATE}
          AND wr.valid_at >= geometry.query_from
          AND wr.valid_at < $1::timestamptz
        ORDER BY
          CASE s.source_kind WHEN 'physical_sensor' THEN 0 ELSE 1 END,
          wr.valid_at DESC,
          wr.id DESC
        LIMIT 20001
      ) input ON true
      ORDER BY
        CASE input.source_kind WHEN 'physical_sensor' THEN 0 ELSE 1 END,
        input.valid_at DESC NULLS LAST,
        input.id DESC NULLS LAST
    `,
    [asOf],
  );
  const first = result.rows[0];

  // distinguish an unavailable site from one with no retained inputs
  if (first === undefined) {
    return null;
  }

  const records: ForecastObservationInputRow[] = [];
  // discard only the left-lateral empty sentinel
  for (const row of result.rows) {
    if ((row.id as string | null) !== null) {
      records.push(row);
    }
  }

  // refuse an incomplete archive instead of truncating projection input
  if (records.length > 20_000) {
    throw new RangeError("forecast observation input row limit exceeded");
  }

  return {
    projectionFrom: first.projectionFrom,
    queryFrom: first.queryFrom,
    records,
    siteSlug: "ballydidean",
    timezone: "America/Los_Angeles",
    through: first.through,
  };
}

// read bounded cursor history
export async function listWeatherHistory(
  pool: Pool,
  query: HistoryQuery,
): Promise<readonly WeatherRecordRow[]> {
  const limit = query.limit ?? 100;
  const from =
    query.from === undefined ? undefined : validateUtcInstant(query.from, "from");
  const to = query.to === undefined ? undefined : validateUtcInstant(query.to, "to");

  // enforce the API bound at the repository edge
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > 251) {
    throw new RangeError("history limit must be between 1 and 251");
  }

  // reject reversed query ranges
  if (from !== undefined && to !== undefined && from >= to) {
    throw new RangeError("history from must be earlier than to");
  }

  const sourceConditions = [
    "si.slug = $1",
    "si.active",
    "st.active",
    "s.active",
    "p.active",
    "s.source_kind IN ('physical_sensor', 'model_current', 'reanalysis')",
    CURRENT_SOURCE_PREDICATE,
  ];
  const recordConditions = ["candidate.source_id = s.id"];
  const values: unknown[] = [query.siteSlug];

  // add station filter
  if (query.stationSlug !== undefined) {
    values.push(query.stationSlug);
    sourceConditions.push(`st.slug = $${values.length}`);
  }

  // add source filter
  if (query.sourceId !== undefined) {
    values.push(query.sourceId);
    sourceConditions.push(`s.id = $${values.length}`);
  }

  // add provenance filter
  if (query.sourceKind !== undefined) {
    values.push(query.sourceKind);
    sourceConditions.push(`s.source_kind = $${values.length}`);
  }

  // add lower time bound
  if (from !== undefined) {
    values.push(from);
    recordConditions.push(`candidate.valid_at >= $${values.length}`);
  }

  // add upper time bound
  if (to !== undefined) {
    values.push(to);
    recordConditions.push(`candidate.valid_at < $${values.length}`);
  }

  // add stable cursor
  if (query.cursor !== undefined) {
    values.push(validateUtcInstant(query.cursor.validAt, "cursor.validAt"));
    const validAtParameter = values.length;
    values.push(query.cursor.id);
    const idParameter = values.length;
    recordConditions.push(
      `(candidate.valid_at, candidate.id) < ($${validAtParameter}::timestamptz, $${idParameter}::bigint)`,
    );
  }

  values.push(limit);
  const limitParameter = values.length;
  const result = await pool.query<WeatherRecordRow>(
    `
      SELECT
        ${weatherRecordSelection()}
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      JOIN LATERAL (
        SELECT ${weatherRecordLateralSelection()}
        FROM weather_records candidate
        WHERE ${recordConditions.join("\n          AND ")}
        ORDER BY candidate.valid_at DESC, candidate.id DESC
        LIMIT $${limitParameter}
      ) wr ON true
      WHERE ${sourceConditions.join("\n        AND ")}
      ORDER BY wr.valid_at DESC, wr.id DESC
      LIMIT $${limitParameter}
    `,
    values,
  );

  return result.rows;
}

// atomically upsert normalized fixed anchors
async function upsertForecastAnchorRecords(
  session: SourceSession,
  runId: string,
  records: readonly NormalizedForecastAnchorRecord[],
): Promise<void> {
  // persist bounded batches
  for (
    let offset = 0;
    offset < records.length;
    offset += FORECAST_ANCHOR_RECORD_BATCH_SIZE
  ) {
    const batch = records.slice(
      offset,
      offset + FORECAST_ANCHOR_RECORD_BATCH_SIZE,
    );
    await upsertForecastAnchorRecordBatch(session, runId, batch);
  }
}

// upsert one bounded fixed-anchor batch
async function upsertForecastAnchorRecordBatch(
  session: SourceSession,
  runId: string,
  records: readonly NormalizedForecastAnchorRecord[],
): Promise<void> {
  const values: unknown[] = [];

  // serialize every truthful fixed anchor
  const placeholders = records.map((record) => {
    // reject cross-source writes
    if (record.sourceId !== session.sourceId) {
      throw new Error("forecast anchor source does not match the locked source");
    }

    const contentHash = createHash("sha256")
      .update(forecastAnchorRecordContent(record))
      .digest("hex");
    const firstParameter = values.length + 1;
    values.push(
      session.sourceId,
      record.sourceKind,
      record.sourceConfigFingerprint,
      record.validAt,
      record.leadHours,
      record.dataset,
      record.upstreamModel,
      record.contractEpoch,
      record.adapterVersion,
      runId,
      record.receivedAt,
      record.metadata.upstreamTimezone,
      serializeNullableJson(record.metadata.quality),
      serializeNullableJson(record.metadata.provider),
      record.metrics.temperatureC,
      record.metrics.apparentTemperatureC,
      record.metrics.precipitationMm,
      record.metrics.windSpeedMps,
      record.metrics.windGustMps,
      record.metrics.pressureHpa,
      record.metrics.relativeHumidityPercent,
      record.metrics.cloudCoverPercent,
      record.metrics.windDirectionDegrees,
      contentHash,
    );

    return forecastAnchorRecordPlaceholder(firstParameter);
  });

  // skip empty caller batches
  if (placeholders.length === 0) {
    return;
  }

  const result = await session.client.query(
    `
      INSERT INTO forecast_anchor_records (
        source_id,
        source_kind,
        source_config_fingerprint,
        valid_at,
        lead_hours,
        dataset,
        upstream_model,
        contract_epoch,
        adapter_version,
        first_ingestion_run_id,
        last_ingestion_run_id,
        first_received_at,
        last_received_at,
        upstream_timezone,
        quality_metadata,
        provider_metadata,
        temperature_c,
        apparent_temperature_c,
        precipitation_mm,
        wind_speed_mps,
        wind_gust_mps,
        pressure_hpa,
        relative_humidity_percent,
        cloud_cover_percent,
        wind_direction_degrees,
        content_hash
      )
      VALUES ${placeholders.join(",\n        ")}
      ON CONFLICT ON CONSTRAINT forecast_anchor_records_identity_key DO UPDATE SET
        last_ingestion_run_id = EXCLUDED.last_ingestion_run_id,
        last_received_at = EXCLUDED.last_received_at,
        quality_metadata = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.quality_metadata ELSE forecast_anchor_records.quality_metadata END,
        provider_metadata = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.provider_metadata ELSE forecast_anchor_records.provider_metadata END,
        temperature_c = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.temperature_c ELSE forecast_anchor_records.temperature_c END,
        apparent_temperature_c = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.apparent_temperature_c ELSE forecast_anchor_records.apparent_temperature_c END,
        precipitation_mm = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.precipitation_mm ELSE forecast_anchor_records.precipitation_mm END,
        wind_speed_mps = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_speed_mps ELSE forecast_anchor_records.wind_speed_mps END,
        wind_gust_mps = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_gust_mps ELSE forecast_anchor_records.wind_gust_mps END,
        pressure_hpa = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.pressure_hpa ELSE forecast_anchor_records.pressure_hpa END,
        relative_humidity_percent = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.relative_humidity_percent ELSE forecast_anchor_records.relative_humidity_percent END,
        cloud_cover_percent = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.cloud_cover_percent ELSE forecast_anchor_records.cloud_cover_percent END,
        wind_direction_degrees = CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_direction_degrees ELSE forecast_anchor_records.wind_direction_degrees END,
        revision_count = forecast_anchor_records.revision_count + CASE WHEN forecast_anchor_records.content_hash <> EXCLUDED.content_hash THEN 1 ELSE 0 END,
        content_hash = EXCLUDED.content_hash
      WHERE forecast_anchor_records.source_kind = EXCLUDED.source_kind
        AND forecast_anchor_records.source_config_fingerprint = EXCLUDED.source_config_fingerprint
        AND forecast_anchor_records.dataset = EXCLUDED.dataset
        AND forecast_anchor_records.upstream_model = EXCLUDED.upstream_model
        AND forecast_anchor_records.contract_epoch = EXCLUDED.contract_epoch
        AND forecast_anchor_records.adapter_version = EXCLUDED.adapter_version
        AND forecast_anchor_records.upstream_timezone = EXCLUDED.upstream_timezone
    `,
    values,
  );

  // fail the transaction on immutable-provenance conflicts
  if (result.rowCount !== records.length) {
    throw new Error("forecast anchor upsert did not persist every input row");
  }
}

// create one repeated fixed-anchor tuple
function forecastAnchorRecordPlaceholder(firstParameter: number): string {
  const parameters = Array.from(
    { length: 24 },
    (_unused, index) => `$${String(firstParameter + index)}`,
  );
  parameters[12] = `${parameters[12]!}::jsonb`;
  parameters[13] = `${parameters[13]!}::jsonb`;
  parameters.splice(10, 0, parameters[9]!);
  parameters.splice(12, 0, parameters[11]!);
  return `(${parameters.join(", ")})`;
}

// atomically upsert normalized records
async function upsertWeatherRecords(
  session: SourceSession,
  runId: string,
  records: readonly NormalizedWeatherRecord[],
): Promise<void> {
  // persist bounded batches
  for (let offset = 0; offset < records.length; offset += WEATHER_RECORD_BATCH_SIZE) {
    const batch = records.slice(offset, offset + WEATHER_RECORD_BATCH_SIZE);
    await upsertWeatherRecordBatch(session, runId, batch);
  }
}

interface ExistingWeatherRevisionIdentity extends QueryResultRow {
  readonly contentHash: string;
  readonly productRunAt: Date | string | null;
  readonly sourceKind: SourceKind;
  readonly validAt: Date | string;
}

// select only revisions introduced or content-changed by this transaction
async function selectChangedWeatherRevisionInputs(
  session: SourceSession,
  records: readonly NormalizedWeatherRecord[],
): Promise<readonly Readonly<{ contentHash: string; record: NormalizedWeatherRecord }>[]> {
  // avoid an empty recordset type inference query
  if (records.length === 0) {
    return [];
  }
  const requested = records.map(
    // compute the same content identity used by the serving upsert
    (record) => ({
      contentHash: createHash("sha256").update(weatherRecordContent(record)).digest("hex"),
      productRunAt: record.productRunAt,
      sourceKind: record.sourceKind,
      validAt: record.validAt,
    }),
  );
  const existing = await session.client.query<ExistingWeatherRevisionIdentity>(`
    SELECT wr.content_hash AS "contentHash", wr.product_run_at AS "productRunAt",
      wr.source_kind AS "sourceKind", wr.valid_at AS "validAt"
    FROM weather_records wr
    JOIN jsonb_to_recordset($2::jsonb) AS requested(
      "productRunAt" timestamptz, "sourceKind" text, "validAt" timestamptz
    ) ON requested."sourceKind" = wr.source_kind
      AND requested."validAt" = wr.valid_at
      AND requested."productRunAt" IS NOT DISTINCT FROM wr.product_run_at
    WHERE wr.source_id = $1
  `, [session.sourceId, JSON.stringify(requested)]);
  const prior = new Map(existing.rows.map(
    // preserve the exact nullable product clock in the identity key
    (row) => [weatherRevisionIdentityKey(
      row.sourceKind,
      storageInstant(row.validAt, "validAt"),
      row.productRunAt === null ? null : storageInstant(row.productRunAt, "productRunAt"),
    ), row.contentHash],
  ));
  return records.flatMap((record, index) => {
    const contentHash = requested[index]!.contentHash;
    const previous = prior.get(weatherRevisionIdentityKey(
      record.sourceKind,
      record.validAt,
      record.productRunAt,
    ));
    // never backfill an unchanged legacy null pointer on ordinary ingestion retry
    return previous === contentHash ? [] : [{ contentHash, record }];
  });
}

// stage one bounded eligible normalized group before transaction commit
async function archiveChangedWeatherRevisions(
  session: SourceSession,
  runId: string,
  changed: readonly Readonly<{ contentHash: string; record: NormalizedWeatherRecord }>[],
  archiver: WeatherAdjustmentRevisionArchiver,
): Promise<readonly WeatherAdjustmentRevisionPublication[]> {
  // skip context queries when no serving value changed
  if (changed.length === 0) {
    return [];
  }
  const context = await session.client.query<{
    adapterVersion: string;
    logicalReceivedAt: Date;
    providerKey: string;
    sourceConfigFingerprint: string;
    sourceKey: string;
  }>(`
    SELECT run.adapter_version AS "adapterVersion", run.started_at AS "logicalReceivedAt",
      provider.provider_key AS "providerKey",
      run.source_config_fingerprint::text AS "sourceConfigFingerprint",
      source.source_key AS "sourceKey"
    FROM ingestion_runs run
    JOIN sources source ON source.id = run.source_id
    JOIN providers provider ON provider.id = source.provider_id
    WHERE run.id = $1 AND run.source_id = $2
  `, [runId, session.sourceId]);
  const source = requireRow(context.rows[0], "weather revision source context");
  const revisions = changed.map(
    // attach the locked source lineage to each changed stored value
    (revision) => ({
      adapterVersion: source.adapterVersion,
      contentHash: revision.contentHash,
      logicalReceivedAt: source.logicalReceivedAt.toISOString(),
      providerKey: source.providerKey,
      record: revision.record,
      sourceConfigFingerprint: source.sourceConfigFingerprint,
      sourceKey: source.sourceKey,
    }),
  );
  await session.client.query("SAVEPOINT weather_revision_archive");
  try {
    const publication = await archiver(session.client, revisions);
    await session.client.query("RELEASE SAVEPOINT weather_revision_archive");
    // omit irrelevant weather products without fabricating a gap
    return publication === null ? [] : [publication];
  } catch {
    // recover aborted SQL before finalizing the ordinary ingestion checkpoint
    await session.client.query("ROLLBACK TO SAVEPOINT weather_revision_archive");
    await session.client.query("RELEASE SAVEPOINT weather_revision_archive");
    return [];
  }
}

// read one full post-epoch physical target group awaiting first archive binding
export async function listPendingPhysicalWeatherAdjustmentRevisions(
  queryable: Queryable,
  sourceId: string,
  epochAt: string,
): Promise<readonly PersistedWeatherAdjustmentRevision[]> {
  // reject generic source selectors and caller-defined historical windows
  if (!/^[1-9]\d*$/u.test(sourceId)) {
    throw new RangeError("physical revision source identity is invalid");
  }
  const lowerBound = validateUtcInstant(epochAt, "epochAt");
  const result = await queryable.query<PendingPhysicalWeatherAdjustmentRevisionRow>(`
    SELECT run.adapter_version AS "adapterVersion",
      wr.apparent_temperature_c AS "apparentTemperatureC",
      wr.black_globe_temperature_c AS "blackGlobeTemperatureC",
      wr.cloud_cover_percent AS "cloudCoverPercent",
      wr.content_hash AS "contentHash",
      wr.device_model AS "deviceModel", wr.device_serial AS "deviceSerial",
      wr.device_vendor AS "deviceVendor", wr.first_received_at AS "logicalReceivedAt",
      wr.pm25_micrograms_per_cubic_meter AS "pm25MicrogramsPerCubicMeter",
      wr.precipitation_mm AS "precipitationMm",
      wr.precipitation_rate_mm_per_hour AS "precipitationRateMmPerHour",
      wr.pressure_hpa AS "pressureHpa", provider.provider_key AS "providerKey",
      wr.provider_metadata AS "providerMetadata", wr.quality_metadata AS "qualityMetadata",
      wr.last_received_at AS "receivedAt",
      wr.relative_humidity_percent AS "relativeHumidityPercent",
      run.source_config_fingerprint::text AS "sourceConfigFingerprint",
      wr.source_id::text AS "sourceId", source.source_key AS "sourceKey",
      wr.soil_electrical_conductivity_us_cm AS "soilElectricalConductivityMicrosiemensPerCm",
      wr.soil_moisture_percent AS "soilMoisturePercent",
      wr.solar_radiation_wm2 AS "solarRadiationWm2", wr.temperature_c AS "temperatureC",
      wr.upstream_model AS "upstreamModel", wr.upstream_timezone AS "upstreamTimezone",
      wr.uv_index AS "uvIndex", wr.valid_at AS "validAt", wr.water_level_m AS "waterLevelM",
      wr.wet_bulb_globe_temperature_c AS "wetBulbGlobeTemperatureC",
      wr.wind_direction_degrees AS "windDirectionDegrees", wr.wind_gust_mps AS "windGustMps",
      wr.wind_speed_mps AS "windSpeedMps"
    FROM weather_records wr
    JOIN sources source ON source.id = wr.source_id
    JOIN providers provider ON provider.id = source.provider_id
    JOIN ingestion_runs run ON run.id = wr.last_ingestion_run_id
    WHERE wr.source_id = $1 AND wr.source_kind = 'physical_sensor'
      AND wr.product_run_at IS NULL AND wr.valid_at >= $2 AND wr.first_received_at >= $2
      AND wr.adjustment_revision_receipt IS NULL
    ORDER BY wr.valid_at, wr.id
    LIMIT 168
  `, [sourceId, lowerBound]);
  // defer stable lineages until one full bounded body is available
  if (result.rows.length < 168 && result.rows.every((row) =>
    row.adapterVersion === result.rows[0]?.adapterVersion &&
    row.sourceConfigFingerprint === result.rows[0]?.sourceConfigFingerprint)) {
    return [];
  }
  const first = result.rows[0];
  // preserve an exact short terminal group only at a lineage boundary
  if (first === undefined) {
    return [];
  }
  const boundaryIndex = result.rows.findIndex((row) =>
    row.adapterVersion !== first.adapterVersion ||
    row.sourceConfigFingerprint !== first.sourceConfigFingerprint);
  const rows = result.rows.slice(0, boundaryIndex === -1
    ? result.rows.length
    : boundaryIndex);
  return rows.map(
    // reconstruct only the canonical normalized values already retained in storage
    (row) => ({
      adapterVersion: row.adapterVersion,
      contentHash: row.contentHash,
      logicalReceivedAt: storageInstant(row.logicalReceivedAt, "logicalReceivedAt"),
      providerKey: row.providerKey,
      record: createNormalizedWeatherRecord({
        metadata: {
          device: row.deviceVendor === null && row.deviceModel === null && row.deviceSerial === null
            ? null
            : {
                ...(row.deviceModel === null ? {} : { model: row.deviceModel }),
                ...(row.deviceSerial === null ? {} : { serial: row.deviceSerial }),
                ...(row.deviceVendor === null ? {} : { vendor: row.deviceVendor }),
              },
          model: row.upstreamModel,
          provider: row.providerMetadata,
          quality: row.qualityMetadata,
          upstreamTimezone: row.upstreamTimezone,
        },
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
          waterLevelM: row.waterLevelM,
          wetBulbGlobeTemperatureC: row.wetBulbGlobeTemperatureC,
          windDirectionDegrees: row.windDirectionDegrees,
          windGustMps: row.windGustMps,
          windSpeedMps: row.windSpeedMps,
        },
        productRunAt: null,
        receivedAt: storageInstant(row.receivedAt, "receivedAt"),
        sourceId: row.sourceId,
        sourceKind: "physical_sensor",
        validAt: storageInstant(row.validAt, "validAt"),
      }),
      sourceConfigFingerprint: row.sourceConfigFingerprint,
      sourceKey: row.sourceKey,
    }),
  );
}

// create one stable nullable serving-row lookup key
function weatherRevisionIdentityKey(
  sourceKind: SourceKind,
  validAt: string,
  productRunAt: string | null,
): string {
  return `${sourceKind}\n${validAt}\n${productRunAt ?? "null"}`;
}

// upsert one bounded provider-neutral batch
async function upsertWeatherRecordBatch(
  session: SourceSession,
  runId: string,
  records: readonly NormalizedWeatherRecord[],
): Promise<void> {
  const values: unknown[] = [];

  // serialize each normalized row
  const placeholders = records.map((record) => {
    // reject cross-source writes
    if (record.sourceId !== session.sourceId) {
      throw new Error("weather record source does not match the locked source");
    }

    const contentHash = createHash("sha256")
      .update(weatherRecordContent(record))
      .digest("hex");
    const firstParameter = values.length + 1;
    values.push(
      session.sourceId,
      record.sourceKind,
      record.validAt,
      record.productRunAt,
      runId,
      record.receivedAt,
      record.metadata.upstreamTimezone,
      record.metadata.model,
      record.metadata.device?.vendor ?? null,
      record.metadata.device?.model ?? null,
      record.metadata.device?.serial ?? null,
      serializeNullableJson(record.metadata.quality),
      serializeNullableJson(record.metadata.provider),
      record.metrics.temperatureC,
      record.metrics.apparentTemperatureC,
      record.metrics.precipitationMm,
      record.metrics.windSpeedMps,
      record.metrics.windGustMps,
      record.metrics.pressureHpa,
      record.metrics.relativeHumidityPercent,
      record.metrics.cloudCoverPercent,
      record.metrics.windDirectionDegrees,
      record.metrics.blackGlobeTemperatureC,
      record.metrics.pm25MicrogramsPerCubicMeter,
      record.metrics.precipitationRateMmPerHour,
      record.metrics.soilElectricalConductivityMicrosiemensPerCm,
      record.metrics.soilMoisturePercent,
      record.metrics.solarRadiationWm2,
      record.metrics.uvIndex,
      record.metrics.wetBulbGlobeTemperatureC,
      record.metrics.waterLevelM,
      contentHash,
    );

    return weatherRecordPlaceholder(firstParameter);
  });

  // skip empty caller batches
  if (placeholders.length === 0) {
    return;
  }

  await session.client.query(
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
          device_serial,
          quality_metadata,
          provider_metadata,
          temperature_c,
          apparent_temperature_c,
          precipitation_mm,
          wind_speed_mps,
          wind_gust_mps,
          pressure_hpa,
          relative_humidity_percent,
          cloud_cover_percent,
          wind_direction_degrees,
          black_globe_temperature_c,
          pm25_micrograms_per_cubic_meter,
          precipitation_rate_mm_per_hour,
          soil_electrical_conductivity_us_cm,
          soil_moisture_percent,
          solar_radiation_wm2,
          uv_index,
          wet_bulb_globe_temperature_c,
          water_level_m,
          content_hash
      )
      VALUES ${placeholders.join(",\n        ")}
      ON CONFLICT ON CONSTRAINT weather_records_identity_key DO UPDATE SET
        last_ingestion_run_id = EXCLUDED.last_ingestion_run_id,
        last_received_at = EXCLUDED.last_received_at,
        upstream_timezone = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.upstream_timezone ELSE weather_records.upstream_timezone END,
        upstream_model = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.upstream_model ELSE weather_records.upstream_model END,
        device_vendor = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.device_vendor ELSE weather_records.device_vendor END,
        device_model = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.device_model ELSE weather_records.device_model END,
        device_serial = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.device_serial ELSE weather_records.device_serial END,
        quality_metadata = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.quality_metadata ELSE weather_records.quality_metadata END,
        provider_metadata = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.provider_metadata ELSE weather_records.provider_metadata END,
        temperature_c = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.temperature_c ELSE weather_records.temperature_c END,
        apparent_temperature_c = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.apparent_temperature_c ELSE weather_records.apparent_temperature_c END,
        precipitation_mm = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.precipitation_mm ELSE weather_records.precipitation_mm END,
        wind_speed_mps = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_speed_mps ELSE weather_records.wind_speed_mps END,
        wind_gust_mps = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_gust_mps ELSE weather_records.wind_gust_mps END,
        pressure_hpa = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.pressure_hpa ELSE weather_records.pressure_hpa END,
        relative_humidity_percent = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.relative_humidity_percent ELSE weather_records.relative_humidity_percent END,
        cloud_cover_percent = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.cloud_cover_percent ELSE weather_records.cloud_cover_percent END,
        wind_direction_degrees = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wind_direction_degrees ELSE weather_records.wind_direction_degrees END,
        black_globe_temperature_c = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.black_globe_temperature_c ELSE weather_records.black_globe_temperature_c END,
        pm25_micrograms_per_cubic_meter = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.pm25_micrograms_per_cubic_meter ELSE weather_records.pm25_micrograms_per_cubic_meter END,
        precipitation_rate_mm_per_hour = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.precipitation_rate_mm_per_hour ELSE weather_records.precipitation_rate_mm_per_hour END,
        soil_electrical_conductivity_us_cm = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.soil_electrical_conductivity_us_cm ELSE weather_records.soil_electrical_conductivity_us_cm END,
        soil_moisture_percent = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.soil_moisture_percent ELSE weather_records.soil_moisture_percent END,
        solar_radiation_wm2 = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.solar_radiation_wm2 ELSE weather_records.solar_radiation_wm2 END,
        uv_index = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.uv_index ELSE weather_records.uv_index END,
        wet_bulb_globe_temperature_c = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.wet_bulb_globe_temperature_c ELSE weather_records.wet_bulb_globe_temperature_c END,
        water_level_m = CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN EXCLUDED.water_level_m ELSE weather_records.water_level_m END,
        revision_count = weather_records.revision_count + CASE WHEN weather_records.content_hash <> EXCLUDED.content_hash THEN 1 ELSE 0 END,
        content_hash = EXCLUDED.content_hash
    `,
    values,
  );
}

// create one repeated insert tuple
function weatherRecordPlaceholder(firstParameter: number): string {
  const parameters = Array.from(
    { length: 32 },
    (_unused, index) => `$${String(firstParameter + index)}`,
  );
  parameters[11] = `${parameters[11]!}::jsonb`;
  parameters[12] = `${parameters[12]!}::jsonb`;
  parameters.splice(5, 0, parameters[4]!);
  parameters.splice(7, 0, parameters[6]!);
  return `(${parameters.join(", ")})`;
}

// compare-and-set a scheduled checkpoint
async function advanceScheduledCheckpoint(
  session: SourceSession,
  input: CompleteScheduledIngestionInput,
): Promise<void> {
  const lastValidAt = validateUtcInstant(input.lastValidAt, "lastValidAt");
  const windowStart = validateUtcInstant(input.windowStart, "windowStart");
  const windowEndExclusive = validateUtcInstant(
    input.windowEndExclusive,
    "windowEndExclusive",
  );

  // create the initial checkpoint
  if (input.expectedCheckpointVersion === null) {
    const result = await session.client.query(
      `
        INSERT INTO ingestion_checkpoints (
          source_id,
          last_valid_at,
          window_start,
          window_end_exclusive,
          provider_cursor
        )
        VALUES ($1, $2, $3, $4, $5::jsonb)
        ON CONFLICT (source_id) DO NOTHING
      `,
      [
        session.sourceId,
        lastValidAt,
        windowStart,
        windowEndExclusive,
        serializeNullableJson(input.providerCursor),
      ],
    );
    requireGuardedUpdate(result.rowCount, "initial checkpoint insert");
    return;
  }

  const result = await session.client.query(
    `
      UPDATE ingestion_checkpoints
      SET
        last_valid_at = $3,
        window_start = $4,
        window_end_exclusive = $5,
        provider_cursor = $6::jsonb,
        version = version + 1,
        last_committed_at = clock_timestamp()
      WHERE source_id = $1
        AND version = $2
    `,
    [
      session.sourceId,
      input.expectedCheckpointVersion,
      lastValidAt,
      windowStart,
      windowEndExclusive,
      serializeNullableJson(input.providerCursor),
    ],
  );
  requireGuardedUpdate(result.rowCount, "checkpoint compare-and-set");
}

// insert or revise an exact chunk outcome
async function upsertBackfillOutcome(
  session: SourceSession,
  runId: string,
  identity: BackfillChunkIdentity,
  outcome: "failed" | "succeeded",
  errorCode: string | null,
): Promise<void> {
  await session.client.query(
    `
      INSERT INTO backfill_chunk_outcomes (
        source_id,
        interval_start,
        interval_end_exclusive,
        source_config_fingerprint,
        adapter_version,
        chunk_plan_version,
        requested_from_date,
        requested_to_date,
        ingestion_run_id,
        outcome,
        error_code
      )
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
      ON CONFLICT (
        source_id,
        interval_start,
        interval_end_exclusive,
        source_config_fingerprint,
        adapter_version,
        chunk_plan_version
      ) DO UPDATE SET
        ingestion_run_id = EXCLUDED.ingestion_run_id,
        requested_from_date = EXCLUDED.requested_from_date,
        requested_to_date = EXCLUDED.requested_to_date,
        outcome = EXCLUDED.outcome,
        error_code = EXCLUDED.error_code,
        completed_at = clock_timestamp()
      WHERE NOT (
        backfill_chunk_outcomes.outcome = 'succeeded'
        AND EXCLUDED.outcome = 'failed'
      )
    `,
    [
      session.sourceId,
      identity.intervalStart,
      identity.intervalEndExclusive,
      identity.sourceConfigFingerprint,
      identity.adapterVersion,
      identity.chunkPlanVersion,
      identity.requestedFromDate,
      identity.requestedToDate,
      runId,
      outcome,
      errorCode,
    ],
  );
}

// guard-finalize successful work
async function finalizeSuccessfulRun(
  session: SourceSession,
  runId: string,
  attempts: number,
  recordCount: number,
  responseMetadata: Readonly<Record<string, JsonValue>> | null,
  upstreamResponseChecksum: string | null,
  mode: IngestionMode,
): Promise<void> {
  // validate optional upstream checksum
  if (upstreamResponseChecksum !== null) {
    validateFingerprint(upstreamResponseChecksum);
  }

  const result = await session.client.query(
    `
      UPDATE ingestion_runs
      SET
        state = 'succeeded',
        completed_at = clock_timestamp(),
        attempts = $3,
        record_count = $4,
        response_metadata = $5::jsonb,
        upstream_response_checksum = $6,
        error_classification = NULL,
        error_code = NULL,
        error_message = NULL
      WHERE id = $1
        AND source_id = $2
        AND state = 'running'
        AND mode = $7
    `,
    [
      runId,
      session.sourceId,
      attempts,
      recordCount,
      serializeNullableJson(responseMetadata),
      upstreamResponseChecksum,
      mode,
    ],
  );
  requireGuardedUpdate(result.rowCount, "successful ingestion finalization");
}

// bind scheduled completion to its committed run
async function assertScheduledRunMatches(
  session: SourceSession,
  input: CompleteScheduledIngestionInput,
): Promise<void> {
  const windowStart = validateUtcInstant(input.windowStart, "windowStart");
  const windowEndExclusive = validateUtcInstant(
    input.windowEndExclusive,
    "windowEndExclusive",
  );
  const result = await session.client.query(
    `
      SELECT 1
      FROM ingestion_runs
      WHERE id = $1
        AND source_id = $2
        AND state = 'running'
        AND mode = 'scheduled'
        AND requested_start = $3
        AND requested_end_exclusive = $4
    `,
    [input.runId, session.sourceId, windowStart, windowEndExclusive],
  );
  requireGuardedUpdate(result.rowCount, "scheduled run identity check");
}

// bind backfill outcome to its committed run
async function assertBackfillRunMatches(
  session: SourceSession,
  runId: string,
  identity: BackfillChunkIdentity,
): Promise<void> {
  const result = await session.client.query(
    `
      SELECT 1
      FROM ingestion_runs
      WHERE id = $1
        AND source_id = $2
        AND state = 'running'
        AND mode = 'backfill'
        AND requested_start = $3
        AND requested_end_exclusive = $4
        AND source_config_fingerprint = $5
        AND adapter_version = $6
        AND chunk_plan_version = $7
    `,
    [
      runId,
      session.sourceId,
      identity.intervalStart,
      identity.intervalEndExclusive,
      identity.sourceConfigFingerprint,
      identity.adapterVersion,
      identity.chunkPlanVersion,
    ],
  );
  requireGuardedUpdate(result.rowCount, "backfill run identity check");
}

// require the expected running mode
async function assertRunningMode(
  session: SourceSession,
  runId: string,
  mode: IngestionMode,
): Promise<void> {
  const result = await session.client.query(
    `
      SELECT 1
      FROM ingestion_runs
      WHERE id = $1
        AND source_id = $2
        AND state = 'running'
        AND mode = $3
    `,
    [runId, session.sourceId, mode],
  );
  requireGuardedUpdate(result.rowCount, "running ingestion mode check");
}

// validate completion counts
function validateCompletionCounts(attempts: number, recordCount: number): void {
  // require non-negative integer counts
  if (
    !Number.isSafeInteger(attempts) ||
    attempts < 0 ||
    !Number.isSafeInteger(recordCount) ||
    recordCount < 0
  ) {
    throw new RangeError("completion counts must be non-negative integers");
  }
}

// require exactly one guarded row
function requireGuardedUpdate(
  rowCount: number | null,
  operation: string,
): void {
  // reject stale or duplicated lifecycle transitions
  if (rowCount !== 1) {
    throw new Error(`${operation} did not affect exactly one row`);
  }
}

// require an expected query row
function requireRow<R>(row: R | undefined, operation: string): R {
  // reject missing results
  if (row === undefined) {
    throw new Error(`${operation} returned no row`);
  }

  return row;
}

// validate and hash one complete private ECMWF run
function validateEcmwfTemperatureCanaryRunInput(
  input: PersistEcmwfTemperatureCanaryRunInput,
): PersistEcmwfTemperatureCanaryRunInput & Readonly<{
  contentHash: string;
  hours: readonly EcmwfTemperatureCanaryHour[];
  recentErrorStateSha256: string;
}> {
  const runInitializedAt = validateUtcInstant(
    input.runInitializedAt,
    "runInitializedAt",
  );
  const receivedAt = validateUtcInstant(input.receivedAt, "receivedAt");
  const runDate = new Date(runInitializedAt);

  // require one exact ECMWF initialization
  if (
    ![0, 6, 12, 18].includes(runDate.getUTCHours()) ||
    runDate.getUTCMinutes() !== 0 ||
    runDate.getUTCSeconds() !== 0 ||
    runDate.getUTCMilliseconds() !== 0 ||
    receivedAt < runInitializedAt
  ) {
    throw new RangeError("ECMWF temperature canary run timing is invalid");
  }

  // require frozen provider identity and bounded provenance
  if (
    input.upstreamModel !== "ecmwf_ifs" ||
    !["49r1", "50r1"].includes(input.modelCycle) ||
    input.adapterVersion !== "open-meteo-ecmwf-single-run/v1" ||
    !/^[a-f0-9]{64}$/u.test(input.providerResponseSha256)
  ) {
    throw new RangeError("ECMWF temperature canary provenance is invalid");
  }

  const expectedCycle = Date.parse(runInitializedAt) < ECMWF_50R1_CUTOVER_MILLISECONDS
    ? "49r1"
    : "50r1";

  // reject model-era drift
  if (input.modelCycle !== expectedCycle) {
    throw new RangeError("ECMWF temperature canary model cycle is invalid");
  }

  const stateStatus = requireEcmwfTemperatureCanaryStateStatus(input.stateStatus);

  // require one bounded state explanation
  if (input.stateReason.trim().length === 0 || input.stateReason.length > 128) {
    throw new RangeError("ECMWF temperature canary state reason is invalid");
  }

  const recentErrorState = validateEcmwfTemperatureCanaryRecentErrorState(
    input.recentErrorState,
    runInitializedAt,
  );

  // bind the status to its frozen state
  if (
    (stateStatus === "supported") !== recentErrorState.supported ||
    (stateStatus === "cold" && recentErrorState.n72 !== 0) ||
    (stateStatus === "insufficient" && recentErrorState.n72 === 0)
  ) {
    throw new RangeError("ECMWF temperature canary state status does not match state");
  }

  // require every exact lead once
  if (
    input.hours.length !== 18 ||
    input.hours.some((hour, index) => hour.modelLeadHours !== index + 1)
  ) {
    throw new RangeError("ECMWF temperature canary run must contain exact leads 1 through 18");
  }

  const hours = input.hours.map((hour) => {
    const validAt = validateUtcInstant(hour.validAt, "validAt");
    const expectedValidAt = new Date(
      Date.parse(runInitializedAt) + hour.modelLeadHours * HOUR_MILLISECONDS,
    ).toISOString();

    // bind every metric row to its forecast identity
    if (
      validAt !== expectedValidAt ||
      !Number.isFinite(hour.rawTemperatureC) ||
      hour.rawTemperatureC < -100 ||
      hour.rawTemperatureC > 70 ||
      !nullableMetricInRange(hour.rawRelativeHumidityPercent, 0, 100) ||
      !nullableMetricInRange(hour.rawWindSpeedMps, 0, 150)
    ) {
      throw new RangeError("ECMWF temperature canary hour is invalid");
    }

    const normalized = {
      modelLeadHours: hour.modelLeadHours,
      rawRelativeHumidityPercent: hour.rawRelativeHumidityPercent,
      rawTemperatureC: hour.rawTemperatureC,
      rawWindSpeedMps: hour.rawWindSpeedMps,
      validAt,
    };

    return {
      ...normalized,
      contentHash: sha256Json(normalized),
    };
  });
  const recentErrorStateSha256 = sha256Json(recentErrorState);
  const contentHash = sha256Json({
    adapterVersion: input.adapterVersion,
    hourContentHashes: hours.map((hour) => hour.contentHash),
    modelCycle: input.modelCycle,
    providerResponseSha256: input.providerResponseSha256,
    recentErrorStateSha256,
    runInitializedAt,
    stateReason: input.stateReason,
    stateStatus,
    upstreamModel: input.upstreamModel,
  });

  return {
    ...input,
    contentHash,
    hours,
    receivedAt,
    recentErrorState,
    recentErrorStateSha256,
    runInitializedAt,
    stateStatus,
  };
}

// validate one rolling-error receipt structurally
function validateEcmwfTemperatureCanaryRecentErrorState(
  state: EcmwfTemperatureCanaryRecentErrorState,
  runInitializedAt: string,
): EcmwfTemperatureCanaryRecentErrorState {
  const windowEndValidAt = new Date(
    Date.parse(runInitializedAt) - 7 * HOUR_MILLISECONDS,
  ).toISOString();

  // require the exact serialized state identity and bounds
  if (
    state === null ||
    typeof state !== "object" ||
    state.cohort !== "ecmwf_single_run_hindcast" ||
    state.targetRunInitializedAt !== runInitializedAt ||
    state.windowEndValidAt !== windowEndValidAt ||
    !Number.isInteger(state.n24) ||
    !Number.isInteger(state.n72) ||
    !Number.isInteger(state.localDates) ||
    state.n24 < 0 ||
    state.n24 > 24 ||
    state.n72 < state.n24 ||
    state.n72 > 72 ||
    state.localDates < 0 ||
    state.localDates > state.n72 ||
    !Array.isArray(state.sourceKeys) ||
    state.sourceKeys.length !== state.n72 ||
    state.sourceKeys.some((key) => typeof key !== "string" || key.length === 0) ||
    new Set(state.sourceKeys).size !== state.sourceKeys.length
  ) {
    throw new RangeError("ECMWF temperature canary recent-error state is invalid");
  }

  const shortSupported = state.n24 >= 6;
  const longSupported = state.n72 >= 24;
  const supported = shortSupported && longSupported && state.localDates >= 2;

  // recompute support and statistic presence
  if (
    state.supported !== supported ||
    !boundedNullableStateStatistic(state.b24C, shortSupported, -6, 6) ||
    !boundedNullableStateStatistic(state.b72C, longSupported, -6, 6) ||
    !boundedNullableStateStatistic(state.mad72C, longSupported, 0, 6)
  ) {
    throw new RangeError("ECMWF temperature canary recent-error statistics are invalid");
  }

  // require populated maxima exactly with selected rows
  if (
    (state.n72 === 0 &&
      (state.maximumSourceValidAt !== null || state.maximumSourceRunInitializedAt !== null)) ||
    (state.n72 > 0 &&
      (state.maximumSourceValidAt === null || state.maximumSourceRunInitializedAt === null))
  ) {
    throw new RangeError("ECMWF temperature canary recent-error maxima are invalid");
  }

  // validate populated causal maxima
  if (state.maximumSourceValidAt !== null && state.maximumSourceRunInitializedAt !== null) {
    const maximumValidAt = validateUtcInstant(
      state.maximumSourceValidAt,
      "maximumSourceValidAt",
    );
    const maximumRunInitializedAt = validateUtcInstant(
      state.maximumSourceRunInitializedAt,
      "maximumSourceRunInitializedAt",
    );
    const maximumRunDate = new Date(maximumRunInitializedAt);
    const maximumValidMilliseconds = Date.parse(maximumValidAt);
    const windowEndMilliseconds = Date.parse(windowEndValidAt);

    // exclude future or incompatible model sources
    if (
      maximumValidAt > windowEndValidAt ||
      maximumValidMilliseconds < windowEndMilliseconds - 71 * HOUR_MILLISECONDS ||
      (state.n24 > 0 &&
        maximumValidMilliseconds < windowEndMilliseconds - 23 * HOUR_MILLISECONDS) ||
      (windowEndMilliseconds - maximumValidMilliseconds) % HOUR_MILLISECONDS !== 0 ||
      maximumRunInitializedAt >= runInitializedAt ||
      ![0, 6, 12, 18].includes(maximumRunDate.getUTCHours()) ||
      maximumRunDate.getUTCMinutes() !== 0 ||
      maximumRunDate.getUTCSeconds() !== 0 ||
      maximumRunDate.getUTCMilliseconds() !== 0 ||
      Date.parse(maximumRunInitializedAt) < Date.parse(maximumValidAt) - 18 * HOUR_MILLISECONDS ||
      Date.parse(maximumRunInitializedAt) > Date.parse(maximumValidAt) - 7 * HOUR_MILLISECONDS
    ) {
      throw new RangeError("ECMWF temperature canary recent-error maxima are noncausal");
    }
  }

  return state;
}

// project one trusted run row
function projectEcmwfTemperatureCanaryRun(
  row: EcmwfTemperatureCanaryRunStorageRow,
): EcmwfTemperatureCanaryRun {
  const upstreamModel = row.upstreamModel;
  const modelCycle = row.modelCycle;
  const adapterVersion = row.adapterVersion;
  const runInitializedAt = storageInstant(
    row.runInitializedAt,
    "runInitializedAt",
  );
  const expectedCycle = Date.parse(runInitializedAt) < ECMWF_50R1_CUTOVER_MILLISECONDS
    ? "49r1"
    : "50r1";

  // reject impossible persisted model identity
  if (
    adapterVersion !== "open-meteo-ecmwf-single-run/v1" ||
    upstreamModel !== "ecmwf_ifs" ||
    modelCycle !== expectedCycle
  ) {
    throw new Error("stored ECMWF temperature canary provenance is invalid");
  }

  return {
    adapterVersion,
    firstReceivedAt: storageInstant(row.firstReceivedAt, "firstReceivedAt"),
    id: String(row.id),
    modelCycle,
    providerResponseSha256: row.providerResponseSha256,
    recentErrorState: row.recentErrorState,
    runInitializedAt,
    stateReason: row.stateReason,
    stateStatus: requireEcmwfTemperatureCanaryStateStatus(row.stateStatus),
    upstreamModel,
  };
}

// project one trusted hour row
function projectEcmwfTemperatureCanaryHour(
  row: EcmwfTemperatureCanaryHourStorageRow,
): EcmwfTemperatureCanaryHour {
  // require the stored normalized content identity on every projected hour
  if (typeof row.contentHash !== "string" || !/^[a-f0-9]{64}$/u.test(row.contentHash)) {
    throw new Error("stored ECMWF temperature canary hour content hash is invalid");
  }
  return {
    contentHash: row.contentHash,
    modelLeadHours: Number(row.modelLeadHours),
    rawRelativeHumidityPercent: row.rawRelativeHumidityPercent,
    rawTemperatureC: row.rawTemperatureC,
    rawWindSpeedMps: row.rawWindSpeedMps,
    validAt: storageInstant(row.validAt, "validAt"),
  };
}

// centralize the private run projection
function ecmwfTemperatureCanaryRunSelection(alias?: string): string {
  const prefix = alias === undefined ? "" : `${alias}.`;

  return `
    ${prefix}id,
    ${prefix}run_initialized_at AS "runInitializedAt",
    ${prefix}first_received_at AS "firstReceivedAt",
    ${prefix}upstream_model AS "upstreamModel",
    ${prefix}adapter_version AS "adapterVersion",
    ${prefix}provider_response_sha256 AS "providerResponseSha256",
    ${prefix}model_cycle AS "modelCycle",
    ${prefix}recent_error_state AS "recentErrorState",
    ${prefix}recent_error_state_sha256 AS "recentErrorStateSha256",
    ${prefix}state_status AS "stateStatus",
    ${prefix}state_reason AS "stateReason",
    ${prefix}content_hash AS "contentHash"
  `;
}

// require one allowlisted state status
function requireEcmwfTemperatureCanaryStateStatus(
  value: string,
): EcmwfTemperatureCanaryRun["stateStatus"] {
  // reject unknown stored states
  if (
    value !== "supported" &&
    value !== "cold" &&
    value !== "insufficient" &&
    value !== "invalid"
  ) {
    throw new Error("ECMWF temperature canary state status is invalid");
  }

  return value;
}

// check one nullable physical metric
function nullableMetricInRange(
  value: number | null,
  minimum: number,
  maximum: number,
): boolean {
  return value === null || (Number.isFinite(value) && value >= minimum && value <= maximum);
}

// check one state statistic and its support-dependent nullability
function boundedNullableStateStatistic(
  value: number | null,
  required: boolean,
  minimum: number,
  maximum: number,
): boolean {
  // reject a missing required statistic
  if (required) {
    return value !== null && Number.isFinite(value) && value >= minimum && value <= maximum;
  }

  return value === null;
}

// hash one stable fixed-order JSON value
function sha256Json(value: unknown): string {
  return createHash("sha256").update(JSON.stringify(value)).digest("hex");
}

// validate one bounded training/export interval
function validateForecastTrainingWindow(
  fromInput: string,
  toInput: string,
): Readonly<{ from: string; to: string }> {
  const from = validateUtcInstant(fromInput, "from");
  const to = validateUtcInstant(toInput, "to");

  // require an ordered bounded export window
  if (
    from >= to ||
    Date.parse(to) - Date.parse(from) >
      MAX_FORECAST_TRAINING_DAYS * 86_400_000 + 2 * HOUR_MILLISECONDS
  ) {
    throw new RangeError("forecast training range must be increasing and at most 450 days");
  }

  return { from, to };
}

// project one stored fixed anchor through the domain contract
function projectFixedLeadAnchorStorageRow(
  row: ForecastAnchorStorageRow,
): FixedLeadAnchorTrainingRow {
  return createFixedLeadAnchorTrainingRow({
    adapterVersion: row.adapterVersion,
    contractEpoch: row.contractEpoch,
    contractVersion: "forecast-anchor-record/v1",
    dataset: row.dataset,
    leadHours: row.leadHours as NormalizedForecastAnchorRecord["leadHours"],
    metadata: {
      device: null,
      model: row.upstreamModel,
      provider: row.providerMetadata,
      quality: row.qualityMetadata,
      upstreamTimezone: row.upstreamTimezone,
    },
    metrics: canonicalMetricsFromForecastStorage(row),
    receivedAt: storageInstant(row.receivedAt, "receivedAt"),
    sourceConfigFingerprint: row.sourceConfigFingerprint,
    sourceId: String(row.sourceId),
    sourceKind: "forecast",
    upstreamModel: row.upstreamModel,
    validAt: storageInstant(row.validAt, "validAt"),
  });
}

// project only eligible legacy v4 retrieval snapshots
function projectLegacyV4RetrievalRows(
  rows: readonly LegacyV4ForecastStorageRow[],
): readonly LegacyV4RetrievalSnapshotTrainingRow[] {
  const projected: LegacyV4RetrievalSnapshotTrainingRow[] = [];

  // retain storage order across eligible rows
  for (const row of rows) {
    const adapterVersion = requireStorageText(row.adapterVersion, "adapterVersion");
    const dataset = requireStorageText(row.dataset, "dataset");
    const upstreamModel = requireStorageText(row.upstreamModel, "upstreamModel");
    const trainingRow = createLegacyV4RetrievalSnapshotTrainingRow({
      adapterVersion,
      contractEpoch: deriveLegacyV4ContractEpoch(
        adapterVersion,
        row.sourceConfigFingerprint,
      ),
      dataset,
      metrics: canonicalMetricsFromForecastStorage(row),
      referenceAt: storageInstant(row.referenceAt, "referenceAt"),
      sourceConfigFingerprint: row.sourceConfigFingerprint,
      sourceId: String(row.sourceId),
      stableRecordId: String(row.stableRecordId),
      upstreamModel,
      validAt: storageInstant(row.validAt, "validAt"),
    });

    // exclude current and unsupported target leads
    if (trainingRow === null) {
      continue;
    }

    // fail closed on a malformed domain projection
    if (!Number.isInteger(trainingRow.targetLeadHours)) {
      throw new Error("legacy v4 target lead must be an integer");
    }

    projected.push(trainingRow);
  }

  return projected;
}

// derive one local epoch from the immutable v4 tuple
export function deriveLegacyV4ContractEpoch(
  adapterVersionInput: string,
  sourceConfigFingerprintInput: string,
): string {
  const adapterVersion = validateVersion(adapterVersionInput, "adapterVersion");
  const sourceConfigFingerprint = validateFingerprint(
    sourceConfigFingerprintInput,
  );
  const tupleHash = createHash("sha256")
    .update(adapterVersion)
    .update("\0")
    .update(sourceConfigFingerprint)
    .digest("hex");

  return `legacy-v4/${tupleHash}`;
}

// build the complete canonical metric shape from storage
function canonicalMetricsFromForecastStorage(
  row: ForecastAnchorStorageRow | LegacyV4ForecastStorageRow,
): CanonicalWeatherMetrics {
  return {
    apparentTemperatureC: row.apparentTemperatureC,
    blackGlobeTemperatureC:
      "blackGlobeTemperatureC" in row ? row.blackGlobeTemperatureC : null,
    cloudCoverPercent: row.cloudCoverPercent,
    pm25MicrogramsPerCubicMeter:
      "pm25MicrogramsPerCubicMeter" in row
        ? row.pm25MicrogramsPerCubicMeter
        : null,
    precipitationMm: "precipitationMm" in row ? row.precipitationMm : null,
    precipitationRateMmPerHour:
      "precipitationRateMmPerHour" in row
        ? row.precipitationRateMmPerHour
        : null,
    pressureHpa: row.pressureHpa,
    relativeHumidityPercent: row.relativeHumidityPercent,
    soilElectricalConductivityMicrosiemensPerCm:
      "soilElectricalConductivityMicrosiemensPerCm" in row
        ? row.soilElectricalConductivityMicrosiemensPerCm
        : null,
    soilMoisturePercent:
      "soilMoisturePercent" in row ? row.soilMoisturePercent : null,
    solarRadiationWm2:
      "solarRadiationWm2" in row ? row.solarRadiationWm2 : null,
    temperatureC: row.temperatureC,
    uvIndex: "uvIndex" in row ? row.uvIndex : null,
    waterLevelM: "waterLevelM" in row ? row.waterLevelM : null,
    wetBulbGlobeTemperatureC:
      "wetBulbGlobeTemperatureC" in row
        ? row.wetBulbGlobeTemperatureC
        : null,
    windDirectionDegrees: row.windDirectionDegrees,
    windGustMps: row.windGustMps,
    windSpeedMps: row.windSpeedMps,
  };
}

// validate source lineage and row quality before aggregation
function validateForecastObservationRows(
  rows: readonly ForecastObservationStorageRow[],
): readonly ValidatedForecastObservationStorageRow[] {
  const validated: ValidatedForecastObservationStorageRow[] = [];

  // verify every selected accepted source row
  for (const row of rows) {
    const lineage = FORECAST_OBSERVATION_SOURCE_LINEAGES.find(
      (candidate) => candidate.sourceKey === row.sourceKey,
    );

    // reject unexpected repository rows
    if (lineage === undefined) {
      throw new Error(`unexpected forecast observation source ${row.sourceKey}`);
    }

    // fail closed on station or immutable source drift
    if (
      row.stationSlug !== lineage.physicalStationKey ||
      row.sourceConfigFingerprint !== lineage.checkedFingerprint ||
      row.adapterContract !== lineage.adapterContract
    ) {
      throw new Error(`forecast observation lineage mismatch for ${row.sourceKey}`);
    }

    const validAt = storageInstant(row.validAt, "validAt");
    const validAtMilliseconds = Date.parse(validAt);

    // exclude rows outside the literal half-open lineage interval
    if (
      (lineage.acceptedStartInclusive !== null &&
        validAtMilliseconds < Date.parse(lineage.acceptedStartInclusive)) ||
      (lineage.acceptedEndExclusive !== null &&
        validAtMilliseconds >= Date.parse(lineage.acceptedEndExclusive))
    ) {
      continue;
    }

    // exclude rows rejected by literal quality rules
    if (!forecastObservationQualityAccepted(row.qualityMetadata, lineage.qualityRule)) {
      continue;
    }

    validated.push({
      id: String(row.id),
      physicalStationKey: lineage.physicalStationKey,
      relativeHumidityPercent: row.relativeHumidityPercent,
      sourceKey: row.sourceKey,
      temperatureC: row.temperatureC,
      validAt,
      validAtMilliseconds,
      windDirectionDegrees: row.windDirectionDegrees,
      windGustMps: row.windGustMps,
      windSpeedMps: row.windSpeedMps,
    });
  }

  // establish bounded-memory collision order
  validated.sort(
    (left, right) =>
      left.validAtMilliseconds - right.validAtMilliseconds ||
      left.physicalStationKey.localeCompare(right.physicalStationKey) ||
      left.id.localeCompare(right.id),
  );
  assertNoForecastObservationCollisions(validated);
  return validated;
}

// apply literal status and flag allowlists
function forecastObservationQualityAccepted(
  quality: Readonly<Record<string, JsonValue>> | null,
  rule: Readonly<{
    allowedFlags: readonly string[];
    statusRule: "absent" | "absent_or_provider_qc_1";
  }>,
): boolean {
  const rawFlags = quality?.flags;
  const rawStatus = quality?.status;

  // reject malformed or unknown flags
  if (
    rawFlags !== undefined &&
    (!Array.isArray(rawFlags) ||
      rawFlags.some(
        (flag) => typeof flag !== "string" || !rule.allowedFlags.includes(flag),
      ))
  ) {
    return false;
  }

  // require absent status for non-WU sources
  if (rule.statusRule === "absent") {
    return rawStatus === undefined;
  }

  return rawStatus === undefined || rawStatus === "provider_qc_1";
}

// reject post-precedence station/time/metric collisions
function assertNoForecastObservationCollisions(
  rows: readonly ValidatedForecastObservationStorageRow[],
): void {
  const latestMetricInstants = new Map<string, number>();

  // inspect each eligible station metric
  for (const row of rows) {
    // inspect the exact five-metric matrix
    for (const metric of FORECAST_ADJUSTMENT_METRICS) {
      // skip missing station values
      if (row[metric] === null) {
        continue;
      }

      // exclude calm directions before collision identity
      if (
        metric === "windDirectionDegrees" &&
        (row.windSpeedMps === null || row.windSpeedMps < 1)
      ) {
        continue;
      }

      const identity = `${row.physicalStationKey}\0${metric}`;

      // fail on duplicate post-precedence observations
      if (latestMetricInstants.get(identity) === row.validAtMilliseconds) {
        throw new Error(
          `forecast observation collision for ${row.physicalStationKey} ${row.validAt} ${metric}`,
        );
      }

      latestMetricInstants.set(identity, row.validAtMilliseconds);
    }
  }
}

// aggregate literal station hours without network fitting
function projectForecastObservationHours(
  rows: readonly ValidatedForecastObservationStorageRow[],
  from: string,
  to: string,
): readonly ForecastObservationHourlyStationRow[] {
  const byStation = new Map<
    ForecastObservationStationKey,
    readonly ValidatedForecastObservationStorageRow[]
  >();

  // group source-precedence rows by physical identity
  for (const station of FORECAST_OBSERVATION_STATIONS) {
    byStation.set(
      station.key,
      rows
        .filter((row) => row.physicalStationKey === station.key)
        .sort(
          (left, right) =>
            left.validAtMilliseconds - right.validAtMilliseconds ||
            left.id.localeCompare(right.id),
        ),
    );
  }

  const projected: ForecastObservationHourlyStationRow[] = [];

  // emit every requested UTC hour
  for (
    let validAtMilliseconds = Date.parse(from);
    validAtMilliseconds < Date.parse(to);
    validAtMilliseconds += HOUR_MILLISECONDS
  ) {
    const validAt = new Date(validAtMilliseconds).toISOString();

    // emit every frozen physical station
    for (const station of FORECAST_OBSERVATION_STATIONS) {
      const stationRows = byStation.get(station.key) ?? [];
      const temperature = selectInstantObservation(
        stationRows,
        validAtMilliseconds,
        "temperatureC",
      );
      const humidity = selectInstantObservation(
        stationRows,
        validAtMilliseconds,
        "relativeHumidityPercent",
      );
      const speed = selectInstantObservation(
        stationRows,
        validAtMilliseconds,
        "windSpeedMps",
      );
      const direction = selectInstantObservation(
        stationRows,
        validAtMilliseconds,
        "windDirectionDegrees",
      );
      const gust = selectCoveredGustObservation(stationRows, validAtMilliseconds);
      const sourceKeys = [
        ...temperature.sourceKeys,
        ...humidity.sourceKeys,
        ...speed.sourceKeys,
        ...direction.sourceKeys,
        ...gust.sourceKeys,
      ];

      projected.push({
        metrics: {
          relativeHumidityPercent: humidity.value,
          temperatureC: temperature.value,
          windDirectionDegrees: direction.value,
          windGustMps: gust.value,
          windSpeedMps: speed.value,
        },
        physicalStationKey: station.key,
        providerFamily: station.providerFamily,
        sourceKeys: [...new Set(sourceKeys)].sort(),
        validAt,
      });
    }
  }

  return projected;
}

// choose one closest instant metric with earlier ties
function selectInstantObservation(
  rows: readonly ValidatedForecastObservationStorageRow[],
  validAtMilliseconds: number,
  metric: Exclude<ForecastAdjustmentMetric, "windGustMps">,
): Readonly<{ sourceKeys: readonly string[]; value: number | null }> {
  const candidates = rows
    .slice(
      firstObservationAtOrAfter(
        rows,
        validAtMilliseconds - FIVE_MINUTES_MILLISECONDS,
      ),
      firstObservationAtOrAfter(
        rows,
        validAtMilliseconds + FIVE_MINUTES_MILLISECONDS,
      ),
    )
    .filter((row) => {
      // skip missing station metrics
      if (row[metric] === null) {
        return false;
      }

      // require paired station speed for direction
      if (
        metric === "windDirectionDegrees" &&
        (row.windSpeedMps === null || row.windSpeedMps < 1)
      ) {
        return false;
      }

      return true;
    });
  candidates.sort((left, right) => {
    const distanceDifference =
      Math.abs(left.validAtMilliseconds - validAtMilliseconds) -
      Math.abs(right.validAtMilliseconds - validAtMilliseconds);

    // prefer the closest instant
    if (distanceDifference !== 0) {
      return distanceDifference;
    }

    // break equal-distance ties earlier
    if (left.validAtMilliseconds !== right.validAtMilliseconds) {
      return left.validAtMilliseconds - right.validAtMilliseconds;
    }

    return left.id.localeCompare(right.id);
  });
  const selected = candidates[0];

  // preserve missing instant metrics
  if (selected === undefined) {
    return { sourceKeys: [], value: null };
  }

  return { sourceKeys: [selected.sourceKey], value: selected[metric] };
}

// select a fully covered preceding-hour gust maximum
function selectCoveredGustObservation(
  rows: readonly ValidatedForecastObservationStorageRow[],
  validAtMilliseconds: number,
): Readonly<{ sourceKeys: readonly string[]; value: number | null }> {
  const windowStart = validAtMilliseconds - HOUR_MILLISECONDS;
  const candidates = rows
    .slice(
      firstObservationAfter(rows, windowStart),
      firstObservationAfter(rows, validAtMilliseconds),
    )
    .filter((row) => row.windGustMps !== null)
    .sort(
      (left, right) =>
        left.validAtMilliseconds - right.validAtMilliseconds ||
        left.id.localeCompare(right.id),
    );
  const first = candidates[0];
  const last = candidates.at(-1);

  // require observations at both window boundaries
  if (
    first === undefined ||
    last === undefined ||
    first.validAtMilliseconds - windowStart > TEN_MINUTES_MILLISECONDS ||
    validAtMilliseconds - last.validAtMilliseconds > TEN_MINUTES_MILLISECONDS
  ) {
    return { sourceKeys: [], value: null };
  }

  // reject any internal coverage gap
  for (let index = 1; index < candidates.length; index += 1) {
    // preserve missing gust on wide gaps
    if (
      candidates[index]!.validAtMilliseconds -
        candidates[index - 1]!.validAtMilliseconds >
      TEN_MINUTES_MILLISECONDS
    ) {
      return { sourceKeys: [], value: null };
    }
  }

  return {
    sourceKeys: [...new Set(candidates.map((row) => row.sourceKey))].sort(),
    value: Math.max(...candidates.map((row) => row.windGustMps!)),
  };
}

// find the first observation at or after one instant
function firstObservationAtOrAfter(
  rows: readonly ValidatedForecastObservationStorageRow[],
  instant: number,
): number {
  let lower = 0;
  let upper = rows.length;

  // narrow the sorted observation interval
  while (lower < upper) {
    const middle = Math.floor((lower + upper) / 2);

    // keep the matching half
    if (rows[middle]!.validAtMilliseconds < instant) {
      lower = middle + 1;
    } else {
      upper = middle;
    }
  }

  return lower;
}

// find the first observation strictly after one instant
function firstObservationAfter(
  rows: readonly ValidatedForecastObservationStorageRow[],
  instant: number,
): number {
  let lower = 0;
  let upper = rows.length;

  // narrow the sorted observation interval
  while (lower < upper) {
    const middle = Math.floor((lower + upper) / 2);

    // keep the matching half
    if (rows[middle]!.validAtMilliseconds <= instant) {
      lower = middle + 1;
    } else {
      upper = middle;
    }
  }

  return lower;
}

// convert one PostgreSQL timestamptz value
function storageInstant(value: Date | string, fieldName: string): string {
  return validateUtcInstant(
    value instanceof Date ? value.toISOString() : value,
    fieldName,
  );
}

// require one bounded storage provenance value
function requireStorageText(value: string | null, fieldName: string): string {
  // reject missing or oversized storage provenance
  if (value === null || value.trim().length === 0 || value.length > 128) {
    throw new Error(`legacy v4 ${fieldName} is missing or invalid`);
  }

  return value;
}

// centralize the raw read projection
function weatherRecordSelection(): string {
  return `
    wr.id,
    wr.content_hash AS "contentHash",
    wr.source_id AS "sourceId",
    NULL::text AS "sourceConfigFingerprint",
    NULL::text AS "adapterVersion",
    NULL::text AS "contractEpoch",
    s.source_key AS "sourceKey",
    wr.source_kind AS "sourceKind",
    wr.valid_at AS "validAt",
    wr.product_run_at AS "productRunAt",
    wr.first_received_at AS "firstReceivedAt",
    wr.last_received_at AS "lastReceivedAt",
    wr.upstream_timezone AS "upstreamTimezone",
    wr.upstream_model AS "upstreamModel",
    wr.device_vendor AS "deviceVendor",
    wr.device_model AS "deviceModel",
    wr.device_serial AS "deviceSerial",
    wr.quality_metadata AS "qualityMetadata",
    wr.provider_metadata AS "providerMetadata",
    wr.revision_count AS "revisionCount",
    wr.temperature_c AS "temperatureC",
    wr.apparent_temperature_c AS "apparentTemperatureC",
    wr.precipitation_mm AS "precipitationMm",
    wr.wind_speed_mps AS "windSpeedMps",
    wr.wind_gust_mps AS "windGustMps",
    wr.pressure_hpa AS "pressureHpa",
    wr.relative_humidity_percent AS "relativeHumidityPercent",
    wr.cloud_cover_percent AS "cloudCoverPercent",
    wr.wind_direction_degrees AS "windDirectionDegrees",
    wr.black_globe_temperature_c AS "blackGlobeTemperatureC",
    wr.pm25_micrograms_per_cubic_meter AS "pm25MicrogramsPerCubicMeter",
    wr.precipitation_rate_mm_per_hour AS "precipitationRateMmPerHour",
    wr.soil_electrical_conductivity_us_cm AS "soilElectricalConductivityMicrosiemensPerCm",
    wr.soil_moisture_percent AS "soilMoisturePercent",
    wr.solar_radiation_wm2 AS "solarRadiationWm2",
    wr.uv_index AS "uvIndex",
    wr.wet_bulb_globe_temperature_c AS "wetBulbGlobeTemperatureC",
    wr.water_level_m AS "waterLevelM",
    st.slug AS "stationSlug",
    si.slug AS "siteSlug",
    p.provider_key AS "providerKey"
  `;
}

// select only the public columns inside lateral scans
function weatherRecordLateralSelection(): string {
  return `
    candidate.id,
    candidate.source_id,
    candidate.source_kind,
    candidate.valid_at,
    candidate.product_run_at,
    candidate.first_received_at,
    candidate.last_received_at,
    candidate.upstream_timezone,
    candidate.upstream_model,
    candidate.device_vendor,
    candidate.device_model,
    candidate.device_serial,
    candidate.quality_metadata,
    candidate.provider_metadata,
    candidate.temperature_c,
    candidate.apparent_temperature_c,
    candidate.precipitation_mm,
    candidate.wind_speed_mps,
    candidate.wind_gust_mps,
    candidate.pressure_hpa,
    candidate.relative_humidity_percent,
    candidate.cloud_cover_percent,
    candidate.wind_direction_degrees,
    candidate.content_hash,
    candidate.revision_count,
    candidate.black_globe_temperature_c,
    candidate.pm25_micrograms_per_cubic_meter,
    candidate.precipitation_rate_mm_per_hour,
    candidate.soil_electrical_conductivity_us_cm,
    candidate.soil_moisture_percent,
    candidate.solar_radiation_wm2,
    candidate.uv_index,
    candidate.wet_bulb_globe_temperature_c,
    candidate.water_level_m
  `;
}

// preserve SQL null for absent JSON
function serializeNullableJson(value: JsonValue | undefined): string | null {
  // retain absent metadata as SQL null
  if (value === null || value === undefined) {
    return null;
  }

  return JSON.stringify(value);
}
