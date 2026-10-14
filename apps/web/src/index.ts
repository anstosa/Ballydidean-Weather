import {
  browserUnitPreferenceStorage,
  DEFAULT_UNIT_PREFERENCES,
  formatMeasurement,
  loadUnitPreferences,
  normalizeUnitPreferences,
  persistUnitPreferences,
  type FormattedMeasurement,
  type UnitPreferences,
  type UnitPreferenceStorage,
} from "./units.js";
import { createSolarCloudBias, projectSolarCloudCover, projectForecastCloudCover, type SolarCloudBias, type SolarCloudSample } from "./solar-cloud.js";
import { parseSolarCloudEvidenceResponse, solarCloudBiasFromEvidence, type SolarCloudEvidence } from "./solar-cloud-evidence.js";
export { buildSolarCloudEvidence, parseSolarCloudEvidenceResponse, solarCloudBiasFromEvidence } from "./solar-cloud-evidence.js";
export type { SolarCloudEvidence } from "./solar-cloud-evidence.js";
import {
  createLocalForecastBias,
  projectLocalForecastBias,
  projectForecastLocalBias,
  type LocalForecastBias,
  type LocalForecastBiasMetric,
  type LocalForecastSample,
} from "./local-forecast-bias.js";

import {
  DEFAULT_FORECAST_CORRECTION_POLICIES,
  forecastCorrectionPolicy,
  type ForecastCorrectionAnchorTier,
  type ForecastCorrectionMetric,
  type ForecastCorrectionPolicies,
  type ForecastCorrectionPolicy,
} from "./forecast-correction-policy.js";
export { DEFAULT_FORECAST_CORRECTION_POLICIES } from "./forecast-correction-policy.js";
export type { ForecastCorrectionPolicies } from "./forecast-correction-policy.js";

export {
  DEFAULT_UNIT_PREFERENCES,
  formatMeasurement,
  loadUnitPreferences,
  normalizeUnitPreferences,
  UNIT_PREFERENCE_STORAGE_KEY,
} from "./units.js";
export type { UnitPreferences, UnitPreferenceStorage } from "./units.js";

export interface SiteSource {
  readonly attribution: {
    readonly label: string;
    readonly url: string;
  };
  readonly id: string;
  readonly key: string;
  readonly kind: "forecast" | "model_current" | "physical_sensor" | "reanalysis" | "tide_observation" | "tide_prediction";
  readonly providerKey: string;
  readonly providerName: string;
  readonly provenanceLabel: string;
}

export interface WeatherSite {
  readonly latitude: number;
  readonly longitude: number;
  readonly name: string;
  readonly slug: string;
  readonly stations: readonly {
    readonly kind: "physical" | "virtual";
    readonly latitude: number;
    readonly longitude: number;
    readonly name: string;
    readonly slug: string;
    readonly sources: readonly SiteSource[];
  }[];
  readonly timezone: string;
}

export interface WeatherRecord {
  // retain the same-source observed three-hour pressure difference
  readonly pressureChange3hHpa?: number | null;
  readonly adjustment?: ForecastAdjustmentDecision;
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
      readonly propertySensors: readonly PropertySensorSnapshot[] | null;
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
    readonly sourceKind: SiteSource["kind"];
    readonly stationSlug: string;
  };
  readonly receivedAt: string;
  readonly revisionCount: number;
  readonly rainAdjustment?: ForecastRainAdjustmentDecision;
  readonly temperatureAdjustment?: ForecastTemperatureCanaryDecision;
  readonly validAt: string;
}

// describe one recorded hourly projection without qualified forecast decisions
export interface ForecastObservationPoint {
  readonly validAt: string;
  readonly metrics: Partial<WeatherRecord["metrics"]>;
  readonly pressureChange3hHpa: number | null;
  readonly sources: Partial<Record<WeatherMetricKey | "pressureChange3hHpa", "ballydidean" | "local_mean" | "regional">>;
  readonly counts?: Partial<Record<WeatherMetricKey | "pressureChange3hHpa", Readonly<{ samples: number; stations: number }>>>;
  readonly recordedThrough?: string;
  readonly partial?: boolean;
}

// limit history to the scalar families shown by the weather charts
const FORECAST_OBSERVATION_METRICS: readonly WeatherMetricKey[] = [
  "temperatureC", "apparentTemperatureC", "windSpeedMps", "windGustMps", "windDirectionDegrees",
  "precipitationRateMmPerHour", "cloudCoverPercent", "relativeHumidityPercent",
  "pm25MicrogramsPerCubicMeter", "uvIndex", "pressureHpa",
];
const FORECAST_OBSERVATION_PROVIDERS = new Set([
  "weatherflow-tempest", "ambient-weather-network", "weather-underground", "netatmo-weathermap", "purpleair",
]);

// reduce complete paged recordings before selecting an independent tier for every metric
export function buildForecastObservationPoints(records: readonly WeatherRecord[], from: string, through: string): readonly ForecastObservationPoint[] {
  const first = Date.parse(from);
  const cutoff = Date.parse(through);
  // refuse unbounded or unaligned projection geometry
  if (!Number.isFinite(first) || !Number.isFinite(cutoff) || first % 3_600_000 !== 0 || cutoff < first || cutoff - first > 25 * 3_600_000) {
    throw new RangeError("Forecast observation window is invalid");
  }
  const revisions = new Map<string, WeatherRecord>();
  // retain the newest revision once without treating historical age as a sensor outage
  for (const record of records) {
    const valid = Date.parse(record.validAt);
    const received = Date.parse(record.receivedAt);
    const source = record.provenance;
    const physical = source.sourceKind === "physical_sensor" && !source.sourceKey.startsWith("rain-target-") &&
      (isFarmCurrentRecord(record) ? source.sourceKey === "ecowitt-88f15505d89f-local-live-v1" :
        source.stationSlug !== "ballydidean-ecowitt" && FORECAST_OBSERVATION_PROVIDERS.has(source.providerKey));
    const regional = source.sourceKind === "model_current" && source.providerKey === "open-meteo" && source.stationSlug === "open-meteo-virtual";
    // exclude future samples unsupported sources and out-of-window context
    if ((!physical && !regional) || !Number.isFinite(valid) || !Number.isFinite(received) || valid < first - 3 * 3_600_000 || valid >= cutoff || received > cutoff) {
      continue;
    }
    const key = `${source.sourceId}/${valid}`;
    const previous = revisions.get(key);
    // a repeated logical row never receives an extra averaging vote
    if (previous === undefined || Date.parse(previous.receivedAt) < received) {
      revisions.set(key, record);
    }
  }
  const bins = new Map<string, WeatherRecord[]>();
  // retain separate source-local hourly bins for pressure datums and sampling cadence
  for (const record of revisions.values()) {
    const key = `${record.provenance.sourceId}/${Math.floor(Date.parse(record.validAt) / 3_600_000) * 3_600_000}`;
    const bin = bins.get(key) ?? [];
    bin.push(record);
    bins.set(key, bin);
  }
  const points: ForecastObservationPoint[] = [];
  // visit every elapsed hourly cell including the explicitly partial current hour
  for (let hour = first; hour < cutoff; hour += 3_600_000) {
    const metrics: { -readonly [Key in WeatherMetricKey]?: number | null } = {};
    const sources: ForecastObservationPoint["sources"] = {};
    const counts: NonNullable<ForecastObservationPoint["counts"]> = {};
    const candidates = new Map<WeatherMetricKey | "pressureChange3hHpa", { value: number; samples: number; record: WeatherRecord }[]>();
    // aggregate each exact source before choosing one source per station
    for (const [key, rows] of bins) {
      const binHour = Number(key.slice(key.lastIndexOf("/") + 1));
      // gust maxima describe the preceding hour while scalars describe the labeled hour
      if (binHour !== hour && binHour !== hour - 3_600_000) {
        continue;
      }
      const latest = rows.toSorted((left, right) => Date.parse(right.validAt) - Date.parse(left.validAt))[0]!;
      // collect one valid aggregate for each metric without converting report rain increments
      for (const metric of FORECAST_OBSERVATION_METRICS) {
        // keep preceding-hour gusts separate from the scalar hourly bin
        if (binHour !== (metric === "windGustMps" ? hour - 3_600_000 : hour)) {
          continue;
        }
        const values = rows.flatMap((record) => {
          const value = usableCurrentMetric(record.metrics[metric], metric);
          return value === null ? [] : [value];
        });
        const value = values.length === 0 ? null : metric === "windGustMps" ? Math.max(...values) :
          metric === "windDirectionDegrees" ? meanCurrentWindDirection(values) : meanCurrentValues(values);
        // missing metrics never disqualify the other working source readings
        if (value !== null) {
          const available = candidates.get(metric) ?? [];
          available.push({ value, samples: values.length, record: latest });
          candidates.set(metric, available);
        }
      }
      // pressure changes never cross station source identities or an intervening empty hour
      if (binHour === hour) {
        const window = [0, 1, 2, 3].map((offset) => bins.get(`${latest.provenance.sourceId}/${hour - offset * 3_600_000}`) ?? []);
        const pressures = window.map((samples) => meanCurrentValues(samples.flatMap((record) => {
          const value = usableCurrentMetric(record.metrics.pressureHpa, "pressureHpa");
          return value === null ? [] : [value];
        })));
        // require all four source-local hourly bins before computing a tendency
        if (pressures.every((value) => value !== null)) {
          const available = candidates.get("pressureChange3hHpa") ?? [];
          const value = pressures[0]! - pressures[3]!;
          // reject impossible tendencies without erasing valid scalar pressure
          if (Math.abs(value) <= 20) {
            available.push({ value, samples: window.reduce((total, samples) => total + samples.filter((record) => usableCurrentMetric(record.metrics.pressureHpa, "pressureHpa") !== null).length, 0), record: latest });
            candidates.set("pressureChange3hHpa", available);
          }
        }
      }
    }
    let pressureChange3hHpa: number | null = null;
    // apply farm precedence and equal station weighting separately for every scalar and tendency
    for (const [metric, available] of candidates) {
      available.sort((left, right) => Date.parse(right.record.validAt) - Date.parse(left.record.validAt) || left.record.provenance.sourceKey.localeCompare(right.record.provenance.sourceKey));
      const farm = available.find((sample) => isFarmCurrentRecord(sample.record));
      const stations = new Map<string, typeof available[number]>();
      // collapse duplicate provider feeds into one newest usable station aggregate
      for (const sample of available) {
        // regional rows and the farm never contribute a second local vote
        if (sample.record.provenance.sourceKind === "physical_sensor" && !isFarmCurrentRecord(sample.record) && !stations.has(sample.record.provenance.stationSlug)) {
          stations.set(sample.record.provenance.stationSlug, sample);
        }
      }
      const regional = available.find((sample) => sample.record.provenance.sourceKind === "model_current");
      const selected = farm !== undefined ? [farm] : stations.size > 0 ? [...stations.values()] : regional === undefined ? [] : [regional];
      const values = selected.map((sample) => sample.value);
      const value = values.length === 0 ? null : metric === "windDirectionDegrees" ? meanCurrentWindDirection(values) : meanCurrentValues(values);
      // retain a missing circular mean rather than manufacturing north for opposing winds
      if (value === null) {
        continue;
      }
      // the pressure chart consumes a tendency rather than an absolute pressure datum
      if (metric === "pressureChange3hHpa") {
        pressureChange3hHpa = value;
      } else {
        metrics[metric] = value;
      }
      sources[metric] = farm !== undefined ? "ballydidean" : stations.size > 0 ? "local_mean" : "regional";
      counts[metric] = { samples: selected.reduce((total, sample) => total + sample.samples, 0), stations: selected.length };
    }
    points.push({ validAt: new Date(hour).toISOString(), metrics, pressureChange3hHpa, sources, counts, recordedThrough: through, partial: hour + 3_600_000 > cutoff });
  }
  return points;
}

// validate the compact edge projection atomically before it can replace plotted forecast values
export function parseForecastObservationResponse(value: unknown): readonly ForecastObservationPoint[] {
  const response = forecastAdjustmentObject(value);
  const generated = Date.parse(typeof response?.generatedAt === "string" ? response.generatedAt : "");
  const data = response?.data;
  // refuse malformed or unbounded envelopes instead of accepting partial projections
  if (!Number.isFinite(generated) || !Array.isArray(data) || data.length > 25) {
    throw new RangeError("Forecast observations are invalid");
  }
  const seen = new Set<number>();
  // bind every point to its actual recording window metric domains and provenance
  for (const item of data) {
    const point = forecastAdjustmentObject(item);
    const valid = Date.parse(typeof point?.validAt === "string" ? point.validAt : "");
    const metrics = forecastAdjustmentObject(point?.metrics);
    const sources = forecastAdjustmentObject(point?.sources);
    // a future duplicate or malformed point invalidates the entire optional projection
    if (point === null || !Number.isFinite(valid) || valid % 3_600_000 !== 0 || valid >= generated || generated - valid > 25 * 3_600_000 || seen.has(valid) || metrics === null || sources === null ||
      (point.pressureChange3hHpa !== null && (typeof point.pressureChange3hHpa !== "number" || !Number.isFinite(point.pressureChange3hHpa) || Math.abs(point.pressureChange3hHpa) > 20))) {
      throw new RangeError("Forecast observation point is invalid");
    }
    // bind optional partial-hour provenance to the exact projection timestamp
    if ((point.recordedThrough !== undefined && point.recordedThrough !== response!.generatedAt) ||
      (point.partial !== undefined && (typeof point.partial !== "boolean" || point.partial !== (valid + 3_600_000 > generated)))) {
      throw new RangeError("Forecast observation coverage is invalid");
    }
    const counts = point.counts === undefined ? null : forecastAdjustmentObject(point.counts);
    // reject malformed optional counts rather than exposing an unsupported completeness claim
    if (point.counts !== undefined && counts === null) {
      throw new RangeError("Forecast observation counts are invalid");
    }
    // retain only bounded count metadata associated with actual values
    for (const [metric, value] of Object.entries(counts ?? {})) {
      const count = forecastAdjustmentObject(value);
      // sample counts cannot be fractional zero private or detached from a recorded tier
      if (!Object.hasOwn(sources, metric) || count === null || !Number.isSafeInteger(count.samples) || !Number.isSafeInteger(count.stations) ||
        (count.samples as number) < 1 || (count.stations as number) < 1 || (count.stations as number) > (count.samples as number) || (count.samples as number) > 64_000) {
        throw new RangeError("Forecast observation counts are invalid");
      }
    }
    seen.add(valid);
    // validate each scalar with the same normalized domains used for current recordings
    for (const [metric, reading] of Object.entries(metrics)) {
      // reject unsupported metrics nonnumbers and absent provenance
      if (!FORECAST_OBSERVATION_METRICS.includes(metric as WeatherMetricKey) || (reading !== null && (typeof reading !== "number" || usableCurrentMetric(reading, metric as WeatherMetricKey) === null || !Object.hasOwn(sources, metric)))) {
        throw new RangeError("Forecast observation metric is invalid");
      }
    }
    // retain only explicit recorded tiers never qualified or forecast metadata
    for (const [metric, source] of Object.entries(sources)) {
      // reject unknown labels or source claims without an associated actual value
      if (!["ballydidean", "local_mean", "regional"].includes(source as string) || (metric === "pressureChange3hHpa" ? point.pressureChange3hHpa === null : !Object.hasOwn(metrics, metric) || metrics[metric] === null)) {
        throw new RangeError("Forecast observation source is invalid");
      }
    }
    // every measured pressure tendency also requires a provenance tier
    if (point.pressureChange3hHpa !== null && !Object.hasOwn(sources, "pressureChange3hHpa")) {
      throw new RangeError("Forecast observation pressure source is invalid");
    }
  }
  return data as readonly ForecastObservationPoint[];
}

// keep experimental projections private rather than trusting serialized api metadata
const solarCloudValues = new WeakMap<WeatherRecord, number>();

// keep short-term local experiments separate from qualified server decisions
const localForecastBiases = new WeakMap<WeatherRecord, Partial<Record<LocalForecastBiasMetric, LocalForecastBias>>>();
// retain trusted construction-time policy and source tiers outside serialized records
const localForecastPolicies = new WeakMap<LocalForecastBias, Readonly<{ policy: ForecastCorrectionPolicy; tier: ForecastCorrectionAnchorTier }>>();
const cloudForecastAnchors = new WeakMap<WeatherRecord, SolarCloudBias>();
const localForecastAnchors = new WeakMap<WeatherRecord, Partial<Record<ForecastCorrectionMetric,
  Readonly<{ tier: ForecastCorrectionAnchorTier; observedAt: string }>>>>();

// expose only causal anchor metadata for the shared offline evaluator
export function forecastCorrectionAnchor(record: WeatherRecord, metric: ForecastCorrectionMetric):
  Readonly<{ tier: ForecastCorrectionAnchorTier; observedAt: string }> | null {
  // clouds have only the exact on-site ws90 source regime
  if (metric === "cloudCoverPercent") {
    const cloud = cloudForecastAnchors.get(record);
    return cloud === undefined ? null : { tier: "ws90", observedAt: cloud.observedAt };
  }
  const anchor = localForecastAnchors.get(record)?.[metric];
  // audit anchors survive expired forecast windows so unchanged hours keep their evaluation vote
  if (anchor !== undefined) {
    return anchor;
  }
  const bias = localForecastBiases.get(record)?.[metric];
  const metadata = bias === undefined ? undefined : localForecastPolicies.get(bias);
  return bias === undefined || metadata === undefined ? null : { tier: metadata.tier, observedAt: bias.observedAt };
}

// return the trusted issue tier without interpreting composite provenance strings
export function forecastCorrectionAnchorTier(record: WeatherRecord, metric: ForecastCorrectionMetric): ForecastCorrectionAnchorTier | null {
  return forecastCorrectionAnchor(record, metric)?.tier ?? null;
}

const LOCAL_FORECAST_BIAS_METRICS: readonly LocalForecastBiasMetric[] = [
  "relativeHumidityPercent", "pm25MicrogramsPerCubicMeter", "uvIndex", "pressureChange3hHpa",
];

// pair current local observations with regional estimates without rewriting raw products
export function withLocalForecastAdjustments<T extends Pick<DashboardState, "current" | "selectedSite"> &
  Partial<Pick<DashboardState, "forecast" | "forecastAdjustmentMode">>>(state: T, now = new Date(),
    policies: ForecastCorrectionPolicies = DEFAULT_FORECAST_CORRECTION_POLICIES): T {
  // retire former local projections while retaining independently evaluated sunlight
  const rawRecord = (record: WeatherRecord): WeatherRecord => {
    // unchanged normalized records need no replacement
    if (!localForecastBiases.has(record) && !localForecastAnchors.has(record)) {
      return record;
    }
    const raw = { ...record };
    const cloud = solarCloudValues.get(record);
    const cloudAnchor = cloudForecastAnchors.get(record);
    // keep the independently refreshed cloud anchor available for paired evaluation
    if (cloudAnchor !== undefined) {
      cloudForecastAnchors.set(raw, cloudAnchor);
    }
    // preserve only the already refreshed cloud estimate
    if (cloud !== undefined) {
      solarCloudValues.set(raw, cloud);
    }
    return raw;
  };
  const forecast = state.forecast?.map(rawRecord);
  const raw = { ...state, ...forecast === undefined ? {} : { forecast } };
  // the regional view never consumes local forecast experiments
  if (state.forecastAdjustmentMode === "raw" || forecast === undefined) {
    return raw;
  }
  const nowMs = now.getTime();
  // newest revisions take precedence separately for each metric
  const newest = (records: readonly WeatherRecord[]): WeatherRecord[] => records.toSorted(
    // use reception time to resolve source revisions at the same valid time
    (left, right) => Date.parse(right.validAt) - Date.parse(left.validAt) || Date.parse(right.receivedAt) - Date.parse(left.receivedAt),
  );
  const regional = newest(state.current.filter(
    // only the farm's regional current model can establish a bias
    (record) => record.provenance.providerKey === "open-meteo" &&
      record.provenance.sourceKind === "model_current" && record.provenance.stationSlug === "open-meteo-virtual",
  ))[0];
  // absence of a current regional anchor must not manufacture local forecasts
  if (regional === undefined) {
    return raw;
  }
  const hour = Math.floor(nowMs / 3_600_000) * 3_600_000;
  const regionalForecasts = forecast.filter(
    // accept only supported regional forecast products already published within the local freshness budget
    (record) => record.provenance.providerKey === regional.provenance.providerKey &&
      record.provenance.stationSlug === regional.provenance.stationSlug && record.provenance.sourceKind === "forecast" &&
      record.metadata.upstream.model === "best_match" &&
      Date.parse(record.receivedAt) <= nowMs && Date.parse(record.productRunAt ?? record.receivedAt) <= nowMs &&
      nowMs - Date.parse(record.productRunAt ?? record.receivedAt) <= REGIONAL_FORECAST_MAX_AGE_MS,
  ).toSorted(
    // one newest model vintage anchors both pairing and all forward projections
    (left, right) => Date.parse(right.productRunAt ?? right.receivedAt) - Date.parse(left.productRunAt ?? left.receivedAt),
  );
  const regionalHour = regionalForecasts.find(
    // uv and particulate values require the exact current forecast hour
    (record) => Date.parse(record.validAt) === hour,
  );
  const forecastAnchor = regionalHour ?? regionalForecasts[0];
  const physical = newest(state.current.filter(
    // slower particulate sensors still require fresh local observations rather than cached outages
    (record) => record.provenance.sourceKind === "physical_sensor" && record.freshness.status === "fresh" &&
      Date.parse(record.validAt) <= nowMs && nowMs - Date.parse(record.validAt) <= 15 * 60_000 &&
      Date.parse(record.receivedAt) >= Date.parse(record.validAt) && Date.parse(record.receivedAt) <= nowMs &&
      nowMs - Date.parse(record.receivedAt) <= 15 * 60_000,
  ));
  // pressure uses same-source tendencies because absolute pressure datums differ
  const value = (record: WeatherRecord, metric: LocalForecastBiasMetric): number | null =>
    metric === "pressureChange3hHpa" ? record.pressureChange3hHpa ?? null : record.metrics[metric];
  // carry each scalar's true observation time and identity into the guarded estimator
  const sample = (record: WeatherRecord, metric: LocalForecastBiasMetric): LocalForecastSample => ({
    value: value(record, metric), validAt: record.validAt, receivedAt: record.receivedAt,
    // current-hour forecast freshness follows its hourly cadence and bounded publication age
    freshnessStatus: record === regionalHour ? "fresh" : record.freshness.status,
    forecastProduct: record.provenance.sourceKind === "forecast",
    recordId: record.id, sourceId: record.provenance.sourceId,
  });
  const biases: Partial<Record<LocalForecastBiasMetric, LocalForecastBias>> = {};
  const site = state.selectedSite ?? PRODUCT_SITE;
  // qualify the four experimental families independently
  for (const metric of LOCAL_FORECAST_BIAS_METRICS) {
    const baseline = metric === "uvIndex" || metric === "pm25MicrogramsPerCubicMeter" ? regionalHour : regional;
    // preserve unavailable regional metrics instead of substituting another hour
    if (baseline === undefined) {
      continue;
    }
    const farm = physical.find(
      // farm precedence never allows a missing or invalid value to hide a working neighbor
      (record) => {
        const reading = value(record, metric);
        return isFarmCurrentRecord(record) && reading !== null && Number.isFinite(reading) &&
          (metric === "pressureChange3hHpa" ? Math.abs(reading) <= 20 : usableCurrentMetric(reading, metric) !== null);
      },
    );
    let observed = farm === undefined ? null : sample(farm, metric);
    // mirror the homepage's one-vote-per-station fallback when the farm metric is unavailable
    if (observed === null) {
      const stations = new Map<string, WeatherRecord>();
      // retain each station's newest usable reading and its real provenance
      for (const record of physical) {
        const reading = value(record, metric);
        // invalid normalized readings must not poison the local average
        if (!isFarmCurrentRecord(record) && !stations.has(record.provenance.stationSlug) &&
          reading !== null && Number.isFinite(reading) &&
          (metric === "pressureChange3hHpa" ? Math.abs(reading) <= 20 : usableCurrentMetric(reading, metric) !== null)) {
          stations.set(record.provenance.stationSlug, record);
        }
      }
      const records = [...stations.values()];
      // an empty physical network cannot establish a correction
      if (records.length > 0) {
        observed = {
          value: meanCurrentValues(records.map(
            // equal station weighting avoids duplicate provider votes
            (record) => value(record, metric)!,
          )),
          validAt: new Date(Math.min(...records.map(
            // the oldest contributor bounds freshness and the fixed projection horizon
            (record) => Date.parse(record.validAt),
          ))).toISOString(),
          receivedAt: new Date(Math.min(...records.map(
            // never refresh observation age merely by recomputing the mean
            (record) => Date.parse(record.receivedAt),
          ))).toISOString(),
          freshnessStatus: "fresh",
          recordId: `nearby-average/${records.map(
            // identify every contributing observation without claiming a farm measurement
            (record) => record.id,
          ).sort().join("/")}`,
          sourceId: `nearby-average/${records.map(
            // retain all contributing source identities for the experimental mean
            (record) => record.provenance.sourceId,
          ).sort().join("/")}`,
        };
      }
    }
    const bias = createLocalForecastBias({ ...site, metric, now: now.toISOString(), observed, regional: sample(baseline, metric) });
    // rejected samples leave only their own metric raw
    if (bias !== null) {
      // freeze only forecast uv and pressure at their actual selected issue tier
      if (metric === "uvIndex" || metric === "pressureChange3hHpa") {
        const tier = farm === undefined ? "local_mean" : "farm";
        localForecastPolicies.set(bias, { tier, policy: forecastCorrectionPolicy(policies, metric, tier) });
      }
      biases[metric] = bias;
    }
  }
  return { ...raw, forecast: forecast.map(
    // mark only this regional source's forward hours inside an accepted observation horizon
    (record) => {
      const target = Date.parse(record.validAt);
      const published = Date.parse(record.productRunAt ?? record.receivedAt);
      const received = Date.parse(record.receivedAt);
      // historical hours and other stations never receive render-only projections
      if (record.provenance.providerKey !== regional.provenance.providerKey ||
        record.provenance.stationSlug !== regional.provenance.stationSlug || record.provenance.sourceKind !== "forecast" ||
        record.metadata.upstream.model !== "best_match" ||
        forecastAnchor === undefined || record.provenance.sourceId !== forecastAnchor.provenance.sourceId ||
        (record.productRunAt ?? record.receivedAt) !== (forecastAnchor.productRunAt ?? forecastAnchor.receivedAt) ||
        !Number.isFinite(target) || !Number.isFinite(published) || !Number.isFinite(received) ||
        published > nowMs || received > nowMs || nowMs - published > REGIONAL_FORECAST_MAX_AGE_MS ||
        target < nowMs) {
        return record;
      }
      const covered: Partial<Record<LocalForecastBiasMetric, LocalForecastBias>> = {};
      // every family expires at its own observation time rather than another sensor's horizon
      for (const metric of LOCAL_FORECAST_BIAS_METRICS) {
        const bias = biases[metric];
        // never mark a retired family as experimentally adjusted
        if (bias !== undefined && target >= Date.parse(bias.evaluatedAt) && target < Date.parse(bias.expiresAt)) {
          covered[metric] = bias;
        }
      }
      const anchors: Partial<Record<ForecastCorrectionMetric, Readonly<{ tier: ForecastCorrectionAnchorTier; observedAt: string }>>> = {};
      // retain the issue regime on unchanged next-day hours without projecting expired biases
      for (const metric of ["uvIndex", "pressureChange3hHpa"] as const) {
        const bias = biases[metric];
        const metadata = bias === undefined ? undefined : localForecastPolicies.get(bias);
        // only accepted issue anchors and the stated next-day forecast enter this audit metadata
        if (bias !== undefined && metadata !== undefined && target <= nowMs + 24 * 3_600_000) {
          anchors[metric] = { tier: metadata.tier, observedAt: bias.observedAt };
        }
      }
      // a completely unanchored and unadjusted hour needs no ephemeral clone
      if (Object.keys(covered).length === 0 && Object.keys(anchors).length === 0) {
        return record;
      }
      const projected = { ...record };
      localForecastBiases.set(projected, covered);
      localForecastAnchors.set(projected, anchors);
      const cloud = solarCloudValues.get(record);
      const cloudAnchor = cloudForecastAnchors.get(record);
      // keep trusted cloud provenance when another local metric clones this hour
      if (cloudAnchor !== undefined) {
        cloudForecastAnchors.set(projected, cloudAnchor);
      }
      // independent cloud and local-bias experiments may coexist on the same render clone
      if (cloud !== undefined) {
        solarCloudValues.set(projected, cloud);
      }
      return projected;
    },
  ) };
}

// retain daytime forecast evidence without inferring nighttime current clouds
export function withSolarCloudAdjustment<T extends Pick<DashboardState, "current" | "selectedSite"> &
  Partial<Pick<DashboardState, "forecast" | "forecastAdjustmentMode" | "solarCloudEvidence">>>(state: T, now = new Date(),
    policies: ForecastCorrectionPolicies = DEFAULT_FORECAST_CORRECTION_POLICIES): T {
  // discard former render-only projections before re-evaluating freshness
  const rawRecord = (record: WeatherRecord): WeatherRecord => (solarCloudValues.has(record) || cloudForecastAnchors.has(record)) ? { ...record } : record;
  const current = state.current.map(rawRecord);
  const forecast = state.forecast?.map(rawRecord);
  const raw = { ...state, current, ...forecast === undefined ? {} : { forecast } };
  // raw mode never consumes experimental sunlight estimates
  if (state.forecastAdjustmentMode === "raw") {
    return raw;
  }
  // choose the newest exact source without falling back to a nearby station
  const latest = (records: readonly WeatherRecord[]): WeatherRecord | undefined => records.toSorted(
    // keep the latest source revision first
    (left, right) => Date.parse(right.validAt) - Date.parse(left.validAt),
  )[0];
  const ws90 = latest(current.filter(
    // the gateway model is gw3000 even though its radiation sensor is the ws90
    (record) => record.provenance.providerKey === "ecowitt-local" &&
      record.provenance.sourceKind === "physical_sensor" && record.provenance.stationSlug === "ballydidean-ecowitt",
  ));
  const regional = latest(current.filter(
    // pair only the regional model's current cloud estimate
    (record) => record.provenance.providerKey === "open-meteo" && record.provenance.sourceKind === "model_current",
  ));
  // carry explicit freshness and provenance into the pure estimator
  const sample = (record: WeatherRecord | undefined, metric: "solarRadiationWm2" | "cloudCoverPercent"): SolarCloudSample | null =>
    record === undefined ? null : {
      value: record.metrics[metric], validAt: record.validAt, receivedAt: record.receivedAt,
      freshnessStatus: record.freshness.status, recordId: record.id, sourceId: record.provenance.sourceId,
    };
  const site = state.selectedSite ?? PRODUCT_SITE;
  const currentBias = createSolarCloudBias({ ...site, now: now.toISOString(), ws90: sample(ws90, "solarRadiationWm2"), regional: sample(regional, "cloudCoverPercent") });
  const retainedBias = solarCloudBiasFromEvidence(state.solarCloudEvidence, site, now);
  // a newer live daytime pair including zero bias supersedes older retained evidence
  const forecastBias = currentBias === null ? retainedBias : retainedBias === null ||
    Date.parse(currentBias.observedAt) > Date.parse(retainedBias.observedAt) ||
    (currentBias.observedAt === retainedBias.observedAt && Date.parse(currentBias.evaluatedAt) >= Date.parse(retainedBias.evaluatedAt))
    ? currentBias : retainedBias;
  const forecastRun = forecast?.filter(
    // use one newest causal best-match retrieval and never an alternate source vintage
    (record) => record.provenance.providerKey === "open-meteo" && record.provenance.sourceKind === "forecast" &&
      record.provenance.stationSlug === "open-meteo-virtual" && record.metadata.upstream.model === "best_match" &&
      Date.parse(record.receivedAt) <= now.getTime() && Date.parse(record.productRunAt ?? record.receivedAt) <= now.getTime(),
  ).toSorted(
    // retrieval identity controls eligibility rather than target time or prediction value
    (left, right) => Date.parse(right.productRunAt ?? right.receivedAt) - Date.parse(left.productRunAt ?? left.receivedAt),
  )[0];
  // retain corrected values only on trusted ephemeral render clones
  const project = (record: WeatherRecord, targetAt: string, bias: SolarCloudBias | null, policy?: ForecastCorrectionPolicy): WeatherRecord => {
    // missing expired or low-sun current evidence preserves its regional value
    if (bias === null) {
      return record;
    }
    const value = policy === undefined ? projectSolarCloudCover(record.metrics.cloudCoverPercent, targetAt, bias)
      : projectForecastCloudCover(record.metrics.cloudCoverPercent, targetAt, bias, policy);
    // a zero policy retains only trusted causal evaluation metadata never an adjusted value
    if (policy !== undefined) {
      const target = Date.parse(targetAt);
      // only future next-day hours retain issue metadata never a renewed adjustment expiry
      if (target < Date.parse(bias.evaluatedAt) || target > now.getTime() + 24 * 3_600_000) {
        return record;
      }
      const projected = { ...record };
      cloudForecastAnchors.set(projected, bias);
      // only an actual numerical difference receives a sunlight value marker
      if (value !== null && Number.isFinite(value) && value !== record.metrics.cloudCoverPercent) {
        solarCloudValues.set(projected, value);
      }
      return projected;
    }
    // omit unchanged unavailable and out-of-horizon values
    if (value === null || !Number.isFinite(value) || value === record.metrics.cloudCoverPercent) {
      return record;
    }
    const projected = { ...record };
    solarCloudValues.set(projected, value);
    return projected;
  };
  return {
    ...raw,
    current: current.map(
      // current estimates apply at evaluation time rather than the model's older interval
      (record) => record === regional ? project(record, now.toISOString(), currentBias) : record,
    ),
    ...forecast === undefined ? {} : { forecast: forecast.map(
      // require causal supported regional products before applying any retained forecast signal
      (record) => {
        const runAt = Date.parse(record.productRunAt ?? record.receivedAt);
        const receivedAt = Date.parse(record.receivedAt);
        const targetAt = Date.parse(record.validAt);
        // past malformed future-publication and stale products remain exact raw
        if (record.provenance.providerKey !== "open-meteo" || record.provenance.sourceKind !== "forecast" ||
          record.provenance.stationSlug !== "open-meteo-virtual" || record.metadata.upstream.model !== "best_match" ||
          forecastRun === undefined || record.provenance.sourceId !== forecastRun.provenance.sourceId ||
          (record.productRunAt ?? record.receivedAt) !== (forecastRun.productRunAt ?? forecastRun.receivedAt) ||
          !Number.isFinite(targetAt) || targetAt < now.getTime() || !Number.isFinite(runAt) || !Number.isFinite(receivedAt) ||
          runAt > now.getTime() || receivedAt > now.getTime() || now.getTime() - runAt > REGIONAL_FORECAST_MAX_AGE_MS) {
          return record;
        }
        return project(record, record.validAt, forecastBias, forecastCorrectionPolicy(policies, "cloudCoverPercent", "ws90"));
      },
    ) },
  };
}

// name only the governed browser adjustment metrics
export type ForecastAdjustmentMetric =
  | "relativeHumidityPercent"
  | "temperatureC"
  | "windDirectionDegrees"
  | "windGustMps"
  | "windSpeedMps";

// name one reviewed runtime activation mode
export type ForecastAdjustmentActivationMode = "maintenance_qualified" | "qualified" | "wind_canary";

// name one bounded adjustment failure
export type ForecastAdjustmentReasonCode =
  | "adjustment_error"
  | "admin_disabled"
  | "bundle_invalid"
  | "bundle_missing"
  | "canary_expired"
  | "canary_killed"
  | "coefficient_missing"
  | "cross_link_mismatch"
  | "direction_calm"
  | "evidence_redundancy_missing"
  | "hash_mismatch"
  | "identity_mismatch"
  | "insufficient_data"
  | "metric_not_enabled"
  | "metric_out_of_bounds"
  | "policy_raw"
  | "qualification_failed"
  | "registry_inactive"
  | "registry_invalid"
  | "runtime_fingerprint_mismatch"
  | "training_envelope_mismatch"
  | "unsupported_lead"
  | "wrong_cohort";

// describe unchanged raw output
export interface ForecastAdjustmentFailRawDecision {
  readonly adjustedMetrics: Readonly<Record<string, never>>;
  readonly appliedMetrics: readonly [];
  readonly contractVersion: "forecast-adjustment-decision/v1";
  readonly reasonCode: ForecastAdjustmentReasonCode;
  readonly state: "disabled" | "not_applicable";
}

// describe the raw forecast identity
export interface ForecastAdjustmentRawForecastProvenance {
  readonly adapterVersion: string;
  readonly cohort: "legacy_v4_retrieval_snapshot";
  readonly contractEpoch: string;
  readonly dataset: string;
  readonly referenceAt: string;
  readonly referenceKind: "retrieval_snapshot";
  readonly sourceConfigFingerprint: string;
  readonly sourceKey: string;
  readonly targetLeadHours: number;
  readonly upstreamModel: string;
  readonly validAt: string;
}

// describe one usable qualified adjustment
export interface ForecastAdjustmentQualifiedActiveDecision {
  readonly adjustedMetrics: Readonly<Partial<Record<ForecastAdjustmentMetric, number>>>;
  readonly algorithmContractVersion: "robust-hierarchical-median/v1";
  readonly appliedMetrics: readonly ForecastAdjustmentMetric[];
  readonly candidateArtifactSha256: string;
  readonly contractVersion: "forecast-adjustment-decision/v1";
  readonly evaluationReportSha256: string;
  readonly leadBand: ForecastAdjustmentLeadBand;
  readonly qualificationReceiptSha256: string;
  readonly rawForecastProvenance: ForecastAdjustmentRawForecastProvenance;
  readonly reasonCode: null;
  readonly state: "active";
}

// describe one usable wind-canary adjustment
export interface ForecastAdjustmentWindCanaryActiveDecision {
  readonly activationKind: "wind_transfer_canary";
  readonly adjustedMetrics: Readonly<Partial<Record<ForecastAdjustmentMetric, number>>>;
  readonly algorithmContractVersion: "robust-hierarchical-median/v1";
  readonly appliedMetrics: readonly ForecastAdjustmentMetric[];
  readonly authorizationSha256: string;
  readonly candidateArtifactSha256: string;
  readonly contractVersion: "forecast-adjustment-decision/v1";
  readonly leadBand: ForecastAdjustmentLeadBand;
  readonly rawForecastProvenance: ForecastAdjustmentRawForecastProvenance;
  readonly reasonCode: null;
  readonly state: "active";
  readonly transferReportSha256: string;
}

// describe one root-qualified recurring maintenance adjustment
export interface ForecastAdjustmentMaintenanceActiveDecision {
  readonly actionSha256: string;
  readonly activationKind: "maintenance_qualified";
  readonly adjustedMetrics: Readonly<Partial<Record<ForecastAdjustmentMetric, number>>>;
  readonly algorithmContractVersion: "robust-hierarchical-median/v1";
  readonly appliedMetrics: readonly ForecastAdjustmentMetric[];
  readonly candidateArtifactSha256: string;
  readonly contractVersion: "forecast-adjustment-decision/v1";
  readonly fullMemberRootSha256: string;
  readonly leadBand: ForecastAdjustmentLeadBand;
  readonly policyReportSha256: string;
  readonly rawForecastProvenance: ForecastAdjustmentRawForecastProvenance;
  readonly reasonCode: null;
  readonly state: "active";
}

// unite active adjustment modes
export type ForecastAdjustmentActiveDecision =
  | ForecastAdjustmentQualifiedActiveDecision
  | ForecastAdjustmentMaintenanceActiveDecision
  | ForecastAdjustmentWindCanaryActiveDecision;

// unite API decision states
export type ForecastAdjustmentDecision =
  | ForecastAdjustmentActiveDecision
  | ForecastAdjustmentFailRawDecision;

// name one fixed adjustment lead band
export type ForecastAdjustmentLeadBand =
  | "001-024"
  | "025-048"
  | "049-072"
  | "073-096"
  | "097-120"
  | "121-144"
  | "145-168";

// describe the bounded runtime summary
export interface ForecastAdjustmentRuntimeStatus {
  readonly actionSha256?: string | null;
  readonly activationMode: ForecastAdjustmentActivationMode | null;
  readonly activeBundle: string | null;
  readonly authorizationSha256: string | null;
  readonly candidateArtifactSha256: string | null;
  readonly enabledMetrics: readonly ForecastAdjustmentMetric[];
  readonly evaluationReportSha256: string | null;
  readonly expiresAt: string | null;
  readonly fullMemberRootSha256?: string | null;
  readonly loadedAt: string | null;
  readonly qualificationReceiptSha256: string | null;
  readonly reasonCode: ForecastAdjustmentReasonCode | null;
  readonly state: "active" | "disabled";
  readonly policyReportSha256?: string | null;
  readonly transferReportSha256: string | null;
}

// name bounded temperature-canary failures
type ForecastTemperatureCanaryReasonCode =
  | "adjustment_error"
  | "admin_disabled"
  | "bundle_invalid"
  | "bundle_missing"
  | "canary_expired"
  | "canary_killed"
  | "inference_error"
  | "invalid_forecast"
  | "invalid_model"
  | "invalid_recent_error_state"
  | "missing_source_forecast"
  | "model_identity_mismatch"
  | "model_not_supported"
  | "model_not_yet_available"
  | "outside_assumed_delay6_next12"
  | "outside_initialization_first12"
  | "outside_operational_window"
  | "policy_raw"
  | "recent_error_state_as_of_mismatch"
  | "recent_error_state_contains_future_data"
  | "recent_error_state_outside_window"
  | "recent_error_state_run_mismatch"
  | "recent_error_state_source_run_mismatch"
  | "registry_inactive"
  | "registry_invalid"
  | "source_identity_mismatch"
  | "source_not_available"
  | "source_stale"
  | "source_time_mismatch"
  | "strength_band_not_supported"
  | "unsupported_cohort";

// describe one explicit live ECMWF temperature source
interface ForecastTemperatureCanarySource {
  readonly adapterVersion: string;
  readonly dataset: "single_run";
  readonly firstReceivedAt: string;
  readonly modelCycle: "49r1" | "50r1";
  readonly modelLeadHours: number;
  readonly operationalHorizonHours: number;
  readonly providerKey: "open-meteo";
  readonly providerResponseSha256: string;
  readonly rawRelativeHumidityPercent: number | null;
  readonly rawTemperatureC: number;
  readonly rawWindSpeedMps: number | null;
  readonly runInitializedAt: string;
  readonly upstreamModel: "ecmwf_ifs";
  readonly validAt: string;
}

// describe one independent temperature decision
interface ForecastTemperatureCanaryDecision {
  readonly branch: "adaptive" | "direct" | null;
  readonly bundleSha256: string | null;
  readonly contractVersion: "forecast-temperature-canary-decision/v1";
  readonly correctedTemperatureC: number | null;
  readonly rawBestMatchTemperatureC: number | null;
  readonly reasonCode: ForecastTemperatureCanaryReasonCode | null;
  readonly recentErrorStateSha256: string | null;
  readonly sourceForecast: ForecastTemperatureCanarySource | null;
  readonly state: "active" | "disabled" | "raw_fallback";
}

// describe bounded temperature runtime monitoring
interface ForecastTemperatureAdjustmentRuntimeStatus {
  readonly actionSha256: string | null;
  readonly activationMode: "maintenance_qualified" | "temperature_canary" | null;
  readonly activeBundle: string | null;
  readonly authorizationSha256: string | null;
  readonly expiresAt: string | null;
  readonly fullMemberRootSha256: string | null;
  readonly loadedAt: string | null;
  readonly policyReportSha256: string | null;
  readonly reasonCode: ForecastTemperatureCanaryReasonCode | null;
  readonly source: null | {
    readonly adaptiveReady: boolean;
    readonly firstReceivedAt: string;
    readonly hourCount: number;
    readonly latestRunInitializedAt: string;
    readonly stateReason: string | null;
    readonly stateStatus: "cold" | "insufficient" | "invalid" | "supported";
  };
  readonly state: "active" | "disabled";
}

// describe independent adjustment groups in the v1 wire envelope
export interface ForecastAdjustmentSettings {
  readonly version: 1;
  readonly temperature: boolean;
  readonly wind: boolean;
  readonly rain: boolean;
}

// describe one received rain model hour
interface ForecastRainAdjustmentSource {
  readonly runInitializedAt: string;
  readonly firstReceivedAt: string;
  readonly validAt: string;
  readonly rawPrecipitationMm: number;
  readonly modelLeadHours: number;
  readonly decisionAt: string;
  readonly upstreamModel: "ecmwf_ifs";
  readonly providerKey: "open-meteo";
}

// describe one independently verified rain decision
interface ForecastRainAdjustmentDecision {
  readonly contractVersion: "forecast-rain-adjustment-decision/v1";
  readonly state: "active" | "disabled" | "raw_fallback";
  readonly reasonCode: string | null;
  readonly bundleSha256: string | null;
  readonly correctedPrecipitationMm: number | null;
  readonly rawBestMatchPrecipitationMm: number | null;
  readonly sourceForecast: ForecastRainAdjustmentSource | null;
}

// describe one bounded rain runtime
interface ForecastRainAdjustmentRuntimeStatus {
  readonly state: "active" | "disabled";
  readonly activeBundle: string | null;
  readonly reasonCode: string | null;
  readonly loadedAt: string | null;
  readonly source: null | {
    readonly runInitializedAt: string;
    readonly firstReceivedAt: string;
    readonly decisionAt: string;
    readonly hourCount: number;
  };
}

// name the published model families
export type ForecastAdjustmentScorecardFamilyName = "temperature" | "wind" | "rain";

// describe one aggregate scorecard metric
export interface ForecastAdjustmentScorecardMetric {
  readonly unit: "celsius" | "meters_per_second" | "millimeters_per_hour";
  readonly rawMae: number | null;
  readonly adjustedMae: number | null;
  readonly deltaMae: number | null;
  readonly rawBias: number | null;
  readonly adjustedBias: number | null;
  readonly rawRmse: number | null;
  readonly adjustedRmse: number | null;
  readonly rawP95: number | null;
  readonly adjustedP95: number | null;
  readonly skillPercent: number | null;
  readonly skillInterval95: null | {
    readonly lower: number;
    readonly upper: number;
  };
}

// describe one aggregate support summary
export interface ForecastAdjustmentScorecardSupport {
  readonly dateCount: number;
  readonly validHourCount: number;
  readonly vintageCount: number;
  readonly targetRowCount: number;
  readonly rowCount: number;
  readonly eventCount: number;
  readonly wetDateCount: number;
  readonly wetRowCount: number;
  readonly effectiveWeightSum: number;
  readonly fallbackCount: number;
  readonly gapCount: number;
  readonly excludedCount: number;
  readonly exclusionReasons: Readonly<Record<string, number>>;
  readonly fallbackReasons: Readonly<Record<string, number>>;
}

// describe one closed scorecard slice
export interface ForecastAdjustmentScorecardSlice {
  readonly dimension: "horizon" | "month" | "season" | "daypart";
  readonly label: string;
  readonly rowCount: number;
  readonly metrics: ForecastAdjustmentScorecardMetric;
}

// describe one separately matched best-match comparison
export interface ForecastAdjustmentBestMatchDiagnostic {
  readonly rowCount: number;
  readonly dateCount: number;
  readonly bestMatchRawMae: number | null;
  readonly sourceRawMae: number | null;
  readonly sourceAdjustedMae: number | null;
  readonly unit: ForecastAdjustmentScorecardMetric["unit"];
}

// describe one rain reliability bin
export interface ForecastAdjustmentRainReliabilityBin {
  readonly count: number;
  readonly meanProbability: number | null;
  readonly observedFrequency: number | null;
}

// describe one reviewed rain threshold
export interface ForecastAdjustmentRainThreshold {
  readonly thresholdMmPerHour: number;
  readonly rawBrier: number | null;
  readonly adjustedBrier: number | null;
  readonly hits: number;
  readonly misses: number;
  readonly falseAlarms: number;
  readonly pod: number | null;
  readonly far: number | null;
  readonly csi: number | null;
  readonly reliability: readonly ForecastAdjustmentRainReliabilityBin[];
}

// describe one same-run rain accumulation window
export interface ForecastAdjustmentRainAccumulation {
  readonly hours: number;
  readonly completeWindows: number;
  readonly rawMae: number | null;
  readonly adjustedMae: number | null;
}

// describe rain-only scorecard diagnostics
export interface ForecastAdjustmentRainDiagnostics {
  readonly annualBalancedVolumeRatio: number | null;
  readonly winterBalancedVolumeRatio: number | null;
  readonly wetRawMae: number | null;
  readonly wetAdjustedMae: number | null;
  readonly heavyRawMae: number | null;
  readonly heavyAdjustedMae: number | null;
  readonly probabilityOrderViolationCount: number;
  readonly thresholds: readonly ForecastAdjustmentRainThreshold[];
  readonly accumulations: readonly ForecastAdjustmentRainAccumulation[];
}

// describe one complete model-family review card
export interface ForecastAdjustmentScorecardFamily {
  readonly family: ForecastAdjustmentScorecardFamilyName;
  readonly servingIdentitySha256: string | null;
  readonly evidenceClass: "as_issued" | "prospective_receipt" | "retrospective_counterfactual" | "development";
  readonly evidenceCutoffAt: string | null;
  readonly supportState: "sufficient" | "insufficient" | "invalid";
  readonly comparisonState: "unscored" | "better" | "mixed" | "worse";
  readonly qualificationState: "development_only" | "counterfactual_only" | "pending_support" | "supported" | "rejected";
  readonly servingState: "authorized_active" | "admin_disabled" | "fail_raw" | "pending_review";
  readonly recommendation: "retain" | "review_candidate" | "review_disable" | "none";
  readonly support: ForecastAdjustmentScorecardSupport;
  readonly metrics: ForecastAdjustmentScorecardMetric;
  readonly slices: readonly ForecastAdjustmentScorecardSlice[];
  readonly rainDiagnostics: ForecastAdjustmentRainDiagnostics | null;
  readonly bestMatchDiagnostic: ForecastAdjustmentBestMatchDiagnostic | null;
}

// describe the shared immutable scorecard inputs
interface ForecastAdjustmentScorecardInputs {
  readonly adjustmentEvidenceManifestSha256: string;
  readonly adjustmentEvidenceWatermarkSha256: string;
  readonly forecastTrainingManifestSha256: string;
  readonly localDateFrom: string;
  readonly localDateTo: string;
  readonly reportSha256s: Readonly<Record<ForecastAdjustmentScorecardFamilyName, string>>;
  readonly sourceRevision: string;
  readonly targetCutoffAt: string;
}

// describe the historical display-only scorecard
export interface ForecastAdjustmentScorecardV1 {
  readonly contractVersion: "forecast-adjustment-scorecard/v1";
  readonly siteKey: "ballydidean";
  readonly generatedAt: string;
  readonly validThrough: string;
  readonly servingChanged: false;
  readonly automaticActivationEligible: false;
  readonly operatorApprovalRequired: true;
  readonly inputs: ForecastAdjustmentScorecardInputs;
  readonly families: Readonly<Record<ForecastAdjustmentScorecardFamilyName, ForecastAdjustmentScorecardFamily>>;
}

// describe one sanitized automatic-policy history entry
export interface ForecastAdjustmentScorecardHistoryEntry {
  readonly actionLineageSha256: string;
  readonly actionProjectionSha256: string;
  readonly actionState: ForecastAdjustmentScorecardV2["actionState"];
  readonly attemptSha256: string;
  readonly occurredAt: string;
  readonly policyDecision: ForecastAdjustmentScorecardV2["policyDecision"];
  readonly releaseManifestSha256: string | null;
}

// describe the authenticated automatic-policy scorecard
export interface ForecastAdjustmentScorecardV2 {
  readonly contractVersion: "forecast-adjustment-scorecard/v2";
  readonly siteKey: "ballydidean";
  readonly generatedAt: string;
  readonly validThrough: string;
  readonly policyDecision: "pending" | "qualified" | "failed" | "regressed";
  readonly actionState: "none" | "shadow_pending" | "confirmation_registered" |
    "pending_support" | "release_pending" | "deploying" | "active" | "failed" |
    "expired_unapplied" | "deployed_operator_off" | "rolled_back_prior" | "raw";
  readonly actionProjectionSha256: string;
  readonly actionLineage: {
    readonly actionSha256: string;
    readonly attemptSha256: string;
    readonly predecessorActionSha256: string | null;
    readonly releaseManifestSha256: string | null;
    readonly sourceRevision: string;
  };
  readonly inputs: ForecastAdjustmentScorecardInputs & {
    readonly frontierSha256: string;
    readonly inputManifestSha256: string;
    readonly reportSha256: string;
  };
  readonly job: {
    readonly attemptState: "not_due" | "due" | "running" | "completed" | "failed" | "blocked";
    readonly backlogState: "clear" | "present" | "blocked";
    readonly dueState: "not_due" | "due" | "overdue" | "expired";
    readonly operatorState: "enabled" | "disabled";
    readonly successState: "none" | "succeeded" | "failed";
  };
  readonly warnings: Readonly<Record<"source" | "fallback" | "gauge" | "capture" | "capacity", readonly string[]>>;
  readonly identities: Readonly<Record<
    "active" | "prior" | "shadow",
    Readonly<Record<ForecastAdjustmentScorecardFamilyName, string | null>>
  >> & Readonly<{ raw: Readonly<Record<ForecastAdjustmentScorecardFamilyName, string>> }>;
  readonly progress: {
    readonly confirmationCompletedEpochs: number;
    readonly confirmationRequiredEpochs: number;
    readonly rollbackCompletedEpochs: number;
    readonly rollbackRequiredEpochs: number;
    readonly rainCaptureExpiresAt: string;
  };
  readonly history: readonly ForecastAdjustmentScorecardHistoryEntry[];
  readonly families: Readonly<Record<ForecastAdjustmentScorecardFamilyName, ForecastAdjustmentScorecardFamily>>;
}

// accept v1 only for display and v2 as the current policy projection
export type ForecastAdjustmentScorecard =
  ForecastAdjustmentScorecardV1 | ForecastAdjustmentScorecardV2;

// name safe protected scorecard outcomes
export type ForecastAdjustmentScorecardLoadState = "loading" | "ready" | "unavailable" | "unauthorized";
export type ForecastAdjustmentScorecardPublicationState =
  "current" | "pending_unapplied" | "legacy_display";

export interface PropertySensorSnapshot {
  readonly channel: number | null;
  readonly key: string;
  readonly model: string;
  readonly readings: Readonly<Record<string, number>>;
}

export interface PropertySensorLayout {
  readonly displayName: string;
  readonly icon: PropertySensorIcon | null;
  readonly latitude: number;
  readonly longitude: number;
  readonly sensorKey: string;
  readonly updatedAt: string;
}

export interface TideRecord {
  readonly eventType: "high" | "low" | null;
  readonly kind: "observation" | "prediction";
  readonly source: {
    readonly attribution: {
      readonly label: string;
      readonly url: string;
    };
    readonly providerKey: string;
    readonly stationName: string;
    readonly stationSlug: string;
  };
  readonly validAt: string;
  readonly waterLevelM: number;
}

export interface HistoryFilters {
  readonly from?: string;
  readonly sourceId?: string;
  readonly sourceKind?: SiteSource["kind"];
  readonly stationSlug?: string;
  readonly to?: string;
}

interface NowIconInputs {
  readonly rain: number;
  readonly cloud: number | null;
  readonly windy: boolean;
}

interface CachedNowIcon extends NowIconInputs {
  readonly cachedAt: number;
  readonly forecastAdjustmentMode?: ForecastAdjustmentMode;
}

export interface DashboardState {
  readonly current: readonly WeatherRecord[];
  readonly cachedNowIcon: CachedNowIcon | null;
  readonly dailyPrecipitation: DailyPrecipitation | null;
  readonly error: string | null;
  readonly filters: HistoryFilters;
  readonly homeNetwork: boolean;
  readonly forecastAdjustmentMode: ForecastAdjustmentMode;
  readonly forecastAdjustmentSettings: ForecastAdjustmentSettings | null;
  readonly forecastAdjustmentRuntime: ForecastAdjustmentRuntimeStatus | null;
  readonly forecastRainAdjustmentRuntime: ForecastRainAdjustmentRuntimeStatus | null;
  readonly forecastTemperatureAdjustmentRuntime: ForecastTemperatureAdjustmentRuntimeStatus | null;
  readonly adminAdjustmentSettingsSaving: boolean;
  readonly adminAdjustmentSettingsMessage: string | null;
  readonly adminAdjustmentScorecard: ForecastAdjustmentScorecard | null;
  readonly adminAdjustmentScorecardPublicationState: ForecastAdjustmentScorecardPublicationState | null;
  readonly adminAdjustmentScorecardState: ForecastAdjustmentScorecardLoadState;
  readonly forecast: readonly WeatherRecord[];
  readonly forecastPressureContext: readonly WeatherRecord[];
  readonly solarCloudEvidence: SolarCloudEvidence | null;
  readonly forecastObservations: readonly ForecastObservationPoint[];
  readonly forecastObservationsReady: boolean;
  readonly forecastObservationsLoading: boolean;
  readonly forecastDays: ForecastDays;
  readonly history: readonly WeatherRecord[];
  readonly loading: boolean;
  readonly mapLayer: MapLayer;
  readonly nextCursor: string | null;
  readonly page: number;
  readonly propertyMapLayer: MapLayer;
  readonly propertySensorLayout: readonly PropertySensorLayout[] | null;
  readonly propertySensorLayoutLoading: boolean;
  readonly selectedPropertySensorKey: string | null;
  readonly selectedStationSlug: string | null;
  readonly trendDetail: TrendDetail;
  readonly trendDisplayMode: TrendDisplayMode;
  readonly trendExtremeKind: TrendExtremeKind;
  readonly trendExtremeThreshold: number;
  readonly selectedTrendMetric: TrendChartMetric;
  readonly selectedTrendYear: number | null;
  readonly selectedSite: WeatherSite | null;
  readonly sites: readonly WeatherSite[];
  readonly tideGeneratedAt: string | null;
  readonly tides: readonly TideRecord[];
  readonly trendGeneratedAt: string | null;
  readonly trends: readonly TrendPoint[];
  readonly units: UnitPreferences;
}

export interface DashboardOptions {
  readonly apiBaseUrl?: string;
  readonly fetcher?: typeof fetch;
  readonly isAdmin?: boolean;
  readonly storage?: UnitPreferenceStorage | null;
  readonly view?: WeatherView;
}

export type WeatherView = "admin" | "forecast" | "home" | "logs" | "map" | "settings" | "trends";
export type ForecastAdjustmentMode = "adjusted" | "raw";
export type ForecastDays = 1 | 5 | 10;
export type MapLayer = "roads" | "satellite" | "topo";
export type TrendDetail = "daily" | "rolling";
export type TrendDisplayMode = "aggregate" | "all";
export type TrendChartMetric =
  | "apparentTemperatureC"
  | "cumulativePrecipitationMm"
  | "drySpellDays"
  | "extremeDayCount"
  | "frostDayCount"
  | "growingDegreeDaysC"
  | "precipitationMm"
  | "pressureHpa"
  | "relativeHumidityPercent"
  | "temperatureAnomalyC"
  | "temperatureC"
  | "temperatureRangeC"
  | "windDirectionRose"
  | "windGustMps"
  | "windSpeedMps";
export type TrendExtremeKind = "cold" | "heat" | "rain" | "wind";
export type PropertySensorIcon = "air-quality" | "rain" | "temperature" | "wind";
export type ForecastMapLayer = "clouds" | "precipitation" | "radar" | "wind";
type ForecastMapPhase = "forecast" | "history";

// map the three indoor sensors onto their physical floors
const INDOOR_HOUSE_LEVELS = [
  { label: "Second floor", sensorKey: "gateway", slug: "second" },
  { label: "First floor", sensorKey: "temperature-1", slug: "first" },
  { label: "Basement", sensorKey: "temperature-2", slug: "basement" },
] as const;

interface ForecastWeatherMapBinding {
  readonly scrubSurface: SVGSVGElement;
  readonly updateTime: (value: string, immediate: boolean) => void;
}

export interface TrendPoint {
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

export interface DailyPrecipitation {
  readonly accumulationMm: number;
  readonly source: {
    readonly sourceId: string;
    readonly stationSlug: string;
  };
  readonly validThrough: string;
}

interface RecordsResponse {
  readonly data: readonly WeatherRecord[];
  readonly page?: {
    readonly limit: number;
    readonly nextCursor: string | null;
  };
  readonly site: WeatherSite;
}

// describe the adjusted forecast boundary
interface ForecastRecordsResponse extends RecordsResponse {
  readonly pressureContext: readonly WeatherRecord[];
  readonly adjustmentSettings: ForecastAdjustmentSettings | null;
  readonly adjustmentRuntime: ForecastAdjustmentRuntimeStatus;
  readonly rainAdjustmentRuntime: ForecastRainAdjustmentRuntimeStatus;
  readonly temperatureAdjustmentRuntime: ForecastTemperatureAdjustmentRuntimeStatus;
}

interface TrendsResponse {
  readonly data: readonly TrendPoint[];
  readonly generatedAt: string;
  readonly site: WeatherSite;
}

interface TidesResponse {
  readonly data: readonly TideRecord[];
  readonly generatedAt: string;
  readonly site: WeatherSite;
}

interface DailyPrecipitationResponse {
  readonly data: DailyPrecipitation | null;
  readonly generatedAt: string;
  readonly site: WeatherSite;
}

interface PropertySensorLayoutResponse {
  readonly data: readonly PropertySensorLayout[];
}

type DashboardListener = (state: DashboardState) => void;

const EMPTY_STATE: DashboardState = {
  current: [],
  cachedNowIcon: null,
  dailyPrecipitation: null,
  error: null,
  filters: {},
  homeNetwork: false,
  forecastAdjustmentMode: "adjusted",
  forecastAdjustmentSettings: null,
  forecastAdjustmentRuntime: null,
  forecastRainAdjustmentRuntime: null,
  forecastTemperatureAdjustmentRuntime: null,
  adminAdjustmentSettingsSaving: false,
  adminAdjustmentSettingsMessage: null,
  adminAdjustmentScorecard: null,
  adminAdjustmentScorecardPublicationState: null,
  adminAdjustmentScorecardState: "loading",
  forecast: [],
  forecastPressureContext: [],
  solarCloudEvidence: null,
  forecastObservations: [],
  forecastObservationsReady: false,
  forecastObservationsLoading: false,
  forecastDays: 1,
  history: [],
  loading: false,
  mapLayer: "roads",
  nextCursor: null,
  page: 0,
  propertyMapLayer: "satellite",
  propertySensorLayout: null,
  propertySensorLayoutLoading: false,
  selectedPropertySensorKey: null,
  selectedStationSlug: null,
  trendDetail: "rolling",
  trendDisplayMode: "aggregate",
  trendExtremeKind: "heat",
  trendExtremeThreshold: 30,
  selectedTrendMetric: "temperatureC",
  selectedTrendYear: null,
  selectedSite: null,
  sites: [],
  tideGeneratedAt: null,
  tides: [],
  trendGeneratedAt: null,
  trends: [],
  units: DEFAULT_UNIT_PREFERENCES,
};

export const FORECAST_ADJUSTMENT_MODE_STORAGE_KEY = "weather.forecast-adjustment-mode.v1";
export const NOW_ICON_STORAGE_KEY = "weather.now-icon.ballydidean.v1";
const NOW_ICON_CACHE_TTL_MS = 30 * 60 * 1_000;
// allow two hourly regional source cadences without keeping an old run indefinitely
const REGIONAL_FORECAST_MAX_AGE_MS = 2 * 60 * 60 * 1_000;
// check the regional run without requesting another forecast every minute
const REGIONAL_FORECAST_REFRESH_MS = 15 * 60 * 1_000;

// retain only the three public inputs needed to choose weather artwork
function parseNowIconInputs(value: unknown): NowIconInputs | null {
  // reject primitives and arrays from browser storage
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  const { rain, cloud, windy } = value as Record<string, unknown>;
  // require bounded metrics without inventing dry conditions from missing data
  if (
    typeof rain !== "number" || !Number.isFinite(rain) || rain < 0 ||
    typeof windy !== "boolean" ||
    (cloud !== null && (typeof cloud !== "number" || !Number.isFinite(cloud) || cloud < 0 || cloud > 100)) ||
    (rain === 0 && cloud === null)
  ) {
    return null;
  }
  return { rain, cloud, windy };
}

// reject expired or future-dated cached conditions
function isNowIconCacheFresh(cache: CachedNowIcon | null | undefined, now: number): cache is CachedNowIcon {
  return cache != null && cache.cachedAt <= now && now - cache.cachedAt < NOW_ICON_CACHE_TTL_MS;
}

// restore recent artwork before the first route render without restoring weather records
function loadNowIconCache(storage: UnitPreferenceStorage | null): CachedNowIcon | null {
  try {
    const value: unknown = JSON.parse(storage?.getItem(NOW_ICON_STORAGE_KEY) ?? "null");
    const inputs = parseNowIconInputs(value);
    const cachedAt = (value as Partial<CachedNowIcon> | null)?.cachedAt;
    const forecastAdjustmentMode = (value as Partial<CachedNowIcon> | null)?.forecastAdjustmentMode;
    // ignore invalid storage without preventing the page from opening
    if (
      inputs === null || typeof cachedAt !== "number" || !Number.isFinite(cachedAt) ||
      (forecastAdjustmentMode !== undefined && forecastAdjustmentMode !== "adjusted" && forecastAdjustmentMode !== "raw")
    ) {
      return null;
    }
    const cache = { ...inputs, cachedAt, forecastAdjustmentMode: forecastAdjustmentMode ?? "adjusted" };
    return isNowIconCacheFresh(cache, Date.now()) ? cache : null;
  } catch {
    return null;
  }
}

// replace the small public cache or invalidate an authoritative unavailable response
function persistNowIconCache(storage: UnitPreferenceStorage | null, cache: CachedNowIcon | null): void {
  try {
    storage?.setItem(NOW_ICON_STORAGE_KEY, JSON.stringify(cache));
  } catch {
    // retain navigation continuity in memory when browser storage is blocked
  }
}

// load one validated forecast display preference
function loadForecastAdjustmentMode(
  storage: UnitPreferenceStorage | null,
): ForecastAdjustmentMode {
  // retain adjusted forecasts without browser storage
  if (storage === null) {
    return "adjusted";
  }

  try {
    return storage.getItem(FORECAST_ADJUSTMENT_MODE_STORAGE_KEY) === "raw"
      ? "raw"
      : "adjusted";
  } catch {
    return "adjusted";
  }
}

// persist one forecast display preference
function persistForecastAdjustmentMode(
  storage: UnitPreferenceStorage | null,
  mode: ForecastAdjustmentMode,
): void {
  // skip unavailable browser storage
  if (storage === null) {
    return;
  }

  try {
    storage.setItem(FORECAST_ADJUSTMENT_MODE_STORAGE_KEY, mode);
  } catch {
    // retain the in-memory preference
  }
}

const PRODUCT_SITE: WeatherSite = {
  latitude: 47.950429954185445,
  longitude: -122.42797012608193,
  name: "Ballydídean",
  slug: "ballydidean",
  stations: [],
  timezone: "America/Los_Angeles",
};

const INVALID_HISTORY_WALL_CLOCK_MESSAGE =
  "That site time does not exist or occurs twice because of daylight saving time. Choose another time.";
const WALL_CLOCK_OFFSET_SAMPLE_MS = 6 * 60 * 60 * 1_000;
const WALL_CLOCK_OFFSET_WINDOW_MS = 48 * 60 * 60 * 1_000;

// freeze the browser adjustment allowlists
const FORECAST_ADJUSTMENT_METRIC_KEYS = new Set<ForecastAdjustmentMetric>([
  "relativeHumidityPercent",
  "temperatureC",
  "windDirectionDegrees",
  "windGustMps",
  "windSpeedMps",
]);
const FORECAST_ADJUSTMENT_REASON_CODE_KEYS = new Set<ForecastAdjustmentReasonCode>([
  "adjustment_error",
  "admin_disabled",
  "bundle_invalid",
  "bundle_missing",
  "canary_expired",
  "canary_killed",
  "coefficient_missing",
  "cross_link_mismatch",
  "direction_calm",
  "evidence_redundancy_missing",
  "hash_mismatch",
  "identity_mismatch",
  "insufficient_data",
  "metric_not_enabled",
  "metric_out_of_bounds",
  "policy_raw",
  "qualification_failed",
  "registry_inactive",
  "registry_invalid",
  "runtime_fingerprint_mismatch",
  "training_envelope_mismatch",
  "unsupported_lead",
  "wrong_cohort",
]);
const FORECAST_ADJUSTMENT_RUNTIME_KEYS = new Set([
  "actionSha256",
  "activationMode",
  "activeBundle",
  "authorizationSha256",
  "candidateArtifactSha256",
  "enabledMetrics",
  "evaluationReportSha256",
  "expiresAt",
  "fullMemberRootSha256",
  "loadedAt",
  "qualificationReceiptSha256",
  "reasonCode",
  "state",
  "policyReportSha256",
  "transferReportSha256",
]);
const FORECAST_ADJUSTMENT_RUNTIME_LEGACY_KEYS = new Set([
  "activationMode",
  "activeBundle",
  "authorizationSha256",
  "candidateArtifactSha256",
  "enabledMetrics",
  "evaluationReportSha256",
  "expiresAt",
  "loadedAt",
  "qualificationReceiptSha256",
  "reasonCode",
  "state",
  "transferReportSha256",
]);
const FORECAST_ADJUSTMENT_FAIL_RAW_KEYS = new Set([
  "adjustedMetrics",
  "appliedMetrics",
  "contractVersion",
  "reasonCode",
  "state",
]);
const FORECAST_ADJUSTMENT_ACTIVE_KEYS = new Set([
  "adjustedMetrics",
  "algorithmContractVersion",
  "appliedMetrics",
  "candidateArtifactSha256",
  "contractVersion",
  "evaluationReportSha256",
  "leadBand",
  "qualificationReceiptSha256",
  "rawForecastProvenance",
  "reasonCode",
  "state",
]);
const FORECAST_ADJUSTMENT_WIND_CANARY_ACTIVE_KEYS = new Set([
  "activationKind",
  "adjustedMetrics",
  "algorithmContractVersion",
  "appliedMetrics",
  "authorizationSha256",
  "candidateArtifactSha256",
  "contractVersion",
  "leadBand",
  "rawForecastProvenance",
  "reasonCode",
  "state",
  "transferReportSha256",
]);
const FORECAST_ADJUSTMENT_MAINTENANCE_ACTIVE_KEYS = new Set([
  "actionSha256",
  "activationKind",
  "adjustedMetrics",
  "algorithmContractVersion",
  "appliedMetrics",
  "candidateArtifactSha256",
  "contractVersion",
  "fullMemberRootSha256",
  "leadBand",
  "policyReportSha256",
  "rawForecastProvenance",
  "reasonCode",
  "state",
]);
const FORECAST_ADJUSTMENT_RAW_PROVENANCE_KEYS = new Set([
  "adapterVersion",
  "cohort",
  "contractEpoch",
  "dataset",
  "referenceAt",
  "referenceKind",
  "sourceConfigFingerprint",
  "sourceKey",
  "targetLeadHours",
  "upstreamModel",
  "validAt",
]);
const FORECAST_ADJUSTMENT_METRIC_BOUNDS: Readonly<
  Record<
    ForecastAdjustmentMetric,
    Readonly<{ maximum: number; maximumExclusive?: boolean; minimum: number }>
  >
> = {
  relativeHumidityPercent: { maximum: 100, minimum: 0 },
  temperatureC: { maximum: 70, minimum: -100 },
  windDirectionDegrees: { maximum: 360, maximumExclusive: true, minimum: 0 },
  windGustMps: { maximum: 150, minimum: 0 },
  windSpeedMps: { maximum: 150, minimum: 0 },
};
// mirror canonical observed domains from packages/domain/src/weather-record.ts without a browser dependency
const CURRENT_READING_METRIC_BOUNDS: Readonly<Partial<Record<
  WeatherMetricKey,
  Readonly<{ maximum: number; maximumExclusive?: boolean; minimum: number }>
>>> = {
  ...FORECAST_ADJUSTMENT_METRIC_BOUNDS,
  apparentTemperatureC: FORECAST_ADJUSTMENT_METRIC_BOUNDS.temperatureC,
  pm25MicrogramsPerCubicMeter: { maximum: 999, minimum: 0 },
  precipitationRateMmPerHour: { maximum: 10_000, minimum: 0 },
  pressureHpa: { maximum: 1_200, minimum: 100 },
  uvIndex: { maximum: 20, minimum: 0 },
  wetBulbGlobeTemperatureC: { maximum: 125, minimum: -100 },
};
const FORECAST_TEMPERATURE_REASON_CODE_KEYS =
  new Set<ForecastTemperatureCanaryReasonCode>([
    "adjustment_error",
    "admin_disabled",
    "bundle_invalid",
    "bundle_missing",
    "canary_expired",
    "canary_killed",
    "inference_error",
    "invalid_forecast",
    "invalid_model",
    "invalid_recent_error_state",
    "missing_source_forecast",
    "model_identity_mismatch",
    "model_not_supported",
    "model_not_yet_available",
    "outside_assumed_delay6_next12",
    "outside_initialization_first12",
    "outside_operational_window",
    "policy_raw",
    "recent_error_state_as_of_mismatch",
    "recent_error_state_contains_future_data",
    "recent_error_state_outside_window",
    "recent_error_state_run_mismatch",
    "recent_error_state_source_run_mismatch",
    "registry_inactive",
    "registry_invalid",
    "source_identity_mismatch",
    "source_not_available",
    "source_stale",
    "source_time_mismatch",
    "strength_band_not_supported",
    "unsupported_cohort",
  ]);
const FORECAST_TEMPERATURE_RUNTIME_KEYS = new Set([
  "actionSha256",
  "activationMode",
  "activeBundle",
  "authorizationSha256",
  "expiresAt",
  "fullMemberRootSha256",
  "loadedAt",
  "policyReportSha256",
  "reasonCode",
  "source",
  "state",
]);
const FORECAST_TEMPERATURE_RUNTIME_LEGACY_KEYS = new Set([
  "activeBundle",
  "authorizationSha256",
  "expiresAt",
  "loadedAt",
  "reasonCode",
  "source",
  "state",
]);
const FORECAST_TEMPERATURE_RUNTIME_SOURCE_KEYS = new Set([
  "adaptiveReady",
  "firstReceivedAt",
  "hourCount",
  "latestRunInitializedAt",
  "stateReason",
  "stateStatus",
]);
const FORECAST_TEMPERATURE_DECISION_KEYS = new Set([
  "branch",
  "bundleSha256",
  "contractVersion",
  "correctedTemperatureC",
  "rawBestMatchTemperatureC",
  "reasonCode",
  "recentErrorStateSha256",
  "sourceForecast",
  "state",
]);
const FORECAST_TEMPERATURE_SOURCE_KEYS = new Set([
  "adapterVersion",
  "dataset",
  "firstReceivedAt",
  "modelCycle",
  "modelLeadHours",
  "operationalHorizonHours",
  "providerKey",
  "providerResponseSha256",
  "rawRelativeHumidityPercent",
  "rawTemperatureC",
  "rawWindSpeedMps",
  "runInitializedAt",
  "upstreamModel",
  "validAt",
]);
const FORECAST_ADJUSTMENT_SETTINGS_KEYS = new Set([
  "version",
  "temperature",
  "wind",
  "rain",
]);
const FORECAST_ADJUSTMENT_SCORECARD_FAMILIES: readonly ForecastAdjustmentScorecardFamilyName[] = [
  "temperature",
  "wind",
  "rain",
];
const FORECAST_ADJUSTMENT_SCORECARD_V1_KEYS = new Set([
  "automaticActivationEligible",
  "contractVersion",
  "families",
  "generatedAt",
  "inputs",
  "operatorApprovalRequired",
  "servingChanged",
  "siteKey",
  "validThrough",
]);
const FORECAST_ADJUSTMENT_SCORECARD_INPUT_KEYS = new Set([
  "adjustmentEvidenceManifestSha256",
  "adjustmentEvidenceWatermarkSha256",
  "forecastTrainingManifestSha256",
  "localDateFrom",
  "localDateTo",
  "reportSha256s",
  "sourceRevision",
  "targetCutoffAt",
]);
const FORECAST_ADJUSTMENT_SCORECARD_V2_KEYS = new Set([
  "actionLineage",
  "actionProjectionSha256",
  "actionState",
  "contractVersion",
  "families",
  "generatedAt",
  "history",
  "identities",
  "inputs",
  "job",
  "policyDecision",
  "progress",
  "siteKey",
  "validThrough",
  "warnings",
]);
const FORECAST_ADJUSTMENT_SCORECARD_V2_INPUT_KEYS = new Set([
  ...FORECAST_ADJUSTMENT_SCORECARD_INPUT_KEYS,
  "frontierSha256",
  "inputManifestSha256",
  "reportSha256",
]);
const FORECAST_ADJUSTMENT_SCORECARD_V2_LINEAGE_KEYS = new Set([
  "actionSha256",
  "attemptSha256",
  "predecessorActionSha256",
  "releaseManifestSha256",
  "sourceRevision",
]);
const FORECAST_ADJUSTMENT_SCORECARD_V2_JOB_KEYS = new Set([
  "attemptState",
  "backlogState",
  "dueState",
  "operatorState",
  "successState",
]);
const FORECAST_ADJUSTMENT_SCORECARD_V2_WARNING_KEYS = new Set([
  "capacity",
  "capture",
  "fallback",
  "gauge",
  "source",
]);
const FORECAST_ADJUSTMENT_SCORECARD_V2_IDENTITY_KEYS = new Set([
  "active",
  "prior",
  "raw",
  "shadow",
]);
const FORECAST_ADJUSTMENT_SCORECARD_V2_PROGRESS_KEYS = new Set([
  "confirmationCompletedEpochs",
  "confirmationRequiredEpochs",
  "rainCaptureExpiresAt",
  "rollbackCompletedEpochs",
  "rollbackRequiredEpochs",
]);
const FORECAST_ADJUSTMENT_SCORECARD_V2_HISTORY_KEYS = new Set([
  "actionLineageSha256",
  "actionProjectionSha256",
  "actionState",
  "attemptSha256",
  "occurredAt",
  "policyDecision",
  "releaseManifestSha256",
]);
const FORECAST_ADJUSTMENT_SCORECARD_FAMILY_KEYS = new Set([
  "bestMatchDiagnostic",
  "comparisonState",
  "evidenceClass",
  "evidenceCutoffAt",
  "family",
  "metrics",
  "qualificationState",
  "rainDiagnostics",
  "recommendation",
  "servingIdentitySha256",
  "servingState",
  "slices",
  "support",
  "supportState",
]);
const FORECAST_ADJUSTMENT_SCORECARD_SUPPORT_KEYS = new Set([
  "dateCount",
  "effectiveWeightSum",
  "eventCount",
  "excludedCount",
  "exclusionReasons",
  "fallbackCount",
  "fallbackReasons",
  "gapCount",
  "rowCount",
  "targetRowCount",
  "validHourCount",
  "vintageCount",
  "wetDateCount",
  "wetRowCount",
]);
const FORECAST_ADJUSTMENT_SCORECARD_METRIC_KEYS = new Set([
  "adjustedBias",
  "adjustedMae",
  "adjustedP95",
  "adjustedRmse",
  "deltaMae",
  "rawBias",
  "rawMae",
  "rawP95",
  "rawRmse",
  "skillInterval95",
  "skillPercent",
  "unit",
]);
const FORECAST_ADJUSTMENT_SCORECARD_SLICE_KEYS = new Set([
  "dimension",
  "label",
  "metrics",
  "rowCount",
]);
const FORECAST_ADJUSTMENT_SCORECARD_BEST_MATCH_KEYS = new Set([
  "bestMatchRawMae",
  "dateCount",
  "rowCount",
  "sourceAdjustedMae",
  "sourceRawMae",
  "unit",
]);
const FORECAST_ADJUSTMENT_SCORECARD_RAIN_KEYS = new Set([
  "accumulations",
  "annualBalancedVolumeRatio",
  "heavyAdjustedMae",
  "heavyRawMae",
  "probabilityOrderViolationCount",
  "thresholds",
  "wetAdjustedMae",
  "wetRawMae",
  "winterBalancedVolumeRatio",
]);
const FORECAST_ADJUSTMENT_SCORECARD_THRESHOLD_KEYS = new Set([
  "adjustedBrier",
  "csi",
  "falseAlarms",
  "far",
  "hits",
  "misses",
  "pod",
  "rawBrier",
  "reliability",
  "thresholdMmPerHour",
]);
const FORECAST_ADJUSTMENT_SCORECARD_RELIABILITY_KEYS = new Set([
  "count",
  "meanProbability",
  "observedFrequency",
]);
const FORECAST_ADJUSTMENT_SCORECARD_ACCUMULATION_KEYS = new Set([
  "adjustedMae",
  "completeWindows",
  "hours",
  "rawMae",
]);
const FORECAST_RAIN_RUNTIME_KEYS = new Set([
  "state",
  "activeBundle",
  "reasonCode",
  "loadedAt",
  "source",
]);
const FORECAST_RAIN_RUNTIME_SOURCE_KEYS = new Set([
  "runInitializedAt",
  "firstReceivedAt",
  "decisionAt",
  "hourCount",
]);
const FORECAST_RAIN_DECISION_KEYS = new Set([
  "contractVersion",
  "state",
  "reasonCode",
  "bundleSha256",
  "correctedPrecipitationMm",
  "rawBestMatchPrecipitationMm",
  "sourceForecast",
]);
const FORECAST_RAIN_SOURCE_KEYS = new Set([
  "runInitializedAt",
  "firstReceivedAt",
  "validAt",
  "rawPrecipitationMm",
  "modelLeadHours",
  "decisionAt",
  "upstreamModel",
  "providerKey",
]);
const DISABLED_FORECAST_ADJUSTMENT_SETTINGS: ForecastAdjustmentSettings = {
  version: 1,
  temperature: false,
  wind: false,
  rain: false,
};
const SHA256_HEX_PATTERN = /^[a-f0-9]{64}$/u;

// freeze the raw weather record allowlists
const WEATHER_RECORD_FRESHNESS_STATUSES = new Set<WeatherRecord["freshness"]["status"]>([
  "delayed",
  "fresh",
  "stale",
]);
const WEATHER_RECORD_METRIC_KEYS: readonly (keyof WeatherRecord["metrics"])[] = [
  "apparentTemperatureC",
  "blackGlobeTemperatureC",
  "cloudCoverPercent",
  "pm25MicrogramsPerCubicMeter",
  "precipitationMm",
  "precipitationRateMmPerHour",
  "pressureHpa",
  "relativeHumidityPercent",
  "soilElectricalConductivityMicrosiemensPerCm",
  "soilMoisturePercent",
  "solarRadiationWm2",
  "temperatureC",
  "uvIndex",
  "windDirectionDegrees",
  "windGustMps",
  "windSpeedMps",
  "wetBulbGlobeTemperatureC",
];
const WEATHER_RECORD_SOURCE_KINDS = new Set<SiteSource["kind"]>([
  "forecast",
  "model_current",
  "physical_sensor",
  "reanalysis",
  "tide_observation",
  "tide_prediction",
]);

// create one bounded invalid-metadata fallback
function invalidForecastAdjustmentRuntime(): ForecastAdjustmentRuntimeStatus {
  return {
    activationMode: null,
    activeBundle: null,
    authorizationSha256: null,
    candidateArtifactSha256: null,
    enabledMetrics: [],
    evaluationReportSha256: null,
    expiresAt: null,
    loadedAt: null,
    qualificationReceiptSha256: null,
    reasonCode: "adjustment_error",
    state: "disabled",
    transferReportSha256: null,
  };
}

// create one isolated invalid temperature fallback
function invalidForecastTemperatureAdjustmentRuntime(): ForecastTemperatureAdjustmentRuntimeStatus {
  return {
    actionSha256: null,
    activationMode: null,
    activeBundle: null,
    authorizationSha256: null,
    expiresAt: null,
    fullMemberRootSha256: null,
    loadedAt: null,
    policyReportSha256: null,
    reasonCode: "adjustment_error",
    source: null,
    state: "disabled",
  };
}

// create one isolated invalid rain fallback
function invalidForecastRainAdjustmentRuntime(): ForecastRainAdjustmentRuntimeStatus {
  return {
    state: "disabled",
    activeBundle: null,
    reasonCode: "adjustment_error",
    loadedAt: null,
    source: null,
  };
}

// narrow one JSON object
function forecastAdjustmentObject(value: unknown): Record<string, unknown> | null {
  // reject arrays and primitive values
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  return value as Record<string, unknown>;
}

// require one exact JSON key set
function hasExactForecastAdjustmentKeys(
  value: Record<string, unknown>,
  keys: ReadonlySet<string>,
): boolean {
  const actual = Object.keys(value);
  return actual.length === keys.size && actual.every(
    // reject every unknown field
    (key) => keys.has(key),
  );
}

// validate one bounded contract string
function isBoundedForecastAdjustmentText(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 256;
}

// validate one canonical digest
function isForecastAdjustmentSha256(value: unknown): value is string {
  return typeof value === "string" && SHA256_HEX_PATTERN.test(value);
}

// validate one explicit UTC instant
function isForecastAdjustmentInstant(value: unknown): value is string {
  // require an explicit timezone suffix
  if (typeof value !== "string" || !/(?:Z|[+-]\d{2}:\d{2})$/u.test(value)) {
    return false;
  }

  return Number.isFinite(Date.parse(value));
}

// validate one nullable API string
function isWeatherRecordNullableText(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

// validate one nullable API number
function isWeatherRecordNullableNumber(value: unknown): value is number | null {
  return value === null || (typeof value === "number" && Number.isFinite(value));
}

// validate one device metadata object
function isWeatherRecordDevice(value: unknown): boolean {
  // preserve absent device metadata
  if (value === null) {
    return true;
  }

  const device = forecastAdjustmentObject(value);

  // require every public device field
  if (device === null) {
    return false;
  }

  return isWeatherRecordNullableText(device.model) &&
    isWeatherRecordNullableText(device.serial) &&
    isWeatherRecordNullableText(device.vendor);
}

// validate one property sensor reading map
function isWeatherRecordSensorReadings(value: unknown): boolean {
  const readings = forecastAdjustmentObject(value);

  // require one finite reading map
  if (readings === null) {
    return false;
  }

  // validate every additive sensor metric
  return Object.values(readings).every(
    // require one finite reading
    (reading) => typeof reading === "number" && Number.isFinite(reading),
  );
}

// validate one property sensor snapshot
function isWeatherRecordPropertySensor(value: unknown): boolean {
  const sensor = forecastAdjustmentObject(value);

  // require the complete public snapshot
  if (sensor === null) {
    return false;
  }

  const validChannel = sensor.channel === null ||
    (typeof sensor.channel === "number" &&
      Number.isSafeInteger(sensor.channel) &&
      sensor.channel >= 1 &&
      sensor.channel <= 32);
  return validChannel &&
    typeof sensor.key === "string" &&
    typeof sensor.model === "string" &&
    isWeatherRecordSensorReadings(sensor.readings);
}

// validate nullable property sensor snapshots
function isWeatherRecordPropertySensors(value: unknown): boolean {
  // preserve absent property sensor metadata
  if (value === null) {
    return true;
  }

  // require and validate every sensor snapshot
  return Array.isArray(value) && value.every(
    // validate one sensor
    (sensor) => isWeatherRecordPropertySensor(sensor),
  );
}

// validate one provider metadata object
function isWeatherRecordProvider(value: unknown): boolean {
  // preserve absent provider metadata
  if (value === null) {
    return true;
  }

  const provider = forecastAdjustmentObject(value);

  // require every public provider field
  if (provider === null) {
    return false;
  }

  return isWeatherRecordNullableText(provider.dataset) &&
    isWeatherRecordNullableNumber(provider.elevationM) &&
    isWeatherRecordNullableText(provider.gridCell) &&
    isWeatherRecordPropertySensors(provider.propertySensors);
}

// validate one quality metadata object
function isWeatherRecordQuality(value: unknown): boolean {
  // preserve absent quality metadata
  if (value === null) {
    return true;
  }

  const quality = forecastAdjustmentObject(value);

  // require every public quality field
  if (quality === null) {
    return false;
  }

  const validFlags = quality.flags === null ||
    (Array.isArray(quality.flags) && quality.flags.every(
      // require one string flag
      (flag) => typeof flag === "string",
    ));
  return isWeatherRecordNullableNumber(quality.confidencePercent) &&
    validFlags &&
    isWeatherRecordNullableText(quality.interpolation) &&
    isWeatherRecordNullableText(quality.status);
}

// validate the complete public metadata shape
function isWeatherRecordMetadata(value: unknown): boolean {
  const metadata = forecastAdjustmentObject(value);

  // require every metadata branch
  if (metadata === null) {
    return false;
  }

  const upstream = forecastAdjustmentObject(metadata.upstream);

  // require the upstream identity
  if (upstream === null) {
    return false;
  }

  return isWeatherRecordDevice(metadata.device) &&
    isWeatherRecordProvider(metadata.provider) &&
    isWeatherRecordQuality(metadata.quality) &&
    isWeatherRecordNullableText(upstream.model) &&
    typeof upstream.timezone === "string";
}

// validate all canonical weather metrics
function isWeatherRecordMetrics(value: unknown): boolean {
  const metrics = forecastAdjustmentObject(value);

  // require one metrics object
  if (metrics === null) {
    return false;
  }

  // require every canonical metric while allowing additive fields
  return WEATHER_RECORD_METRIC_KEYS.every(
    // validate one finite-or-null metric
    (metric) => isWeatherRecordNullableNumber(metrics[metric]),
  );
}

// validate one public provenance object
function isWeatherRecordProvenance(value: unknown): boolean {
  const provenance = forecastAdjustmentObject(value);

  // require one provenance object
  if (provenance === null) {
    return false;
  }

  const attribution = forecastAdjustmentObject(provenance.attribution);

  // require the attribution identity
  if (attribution === null) {
    return false;
  }

  return typeof attribution.label === "string" &&
    typeof attribution.url === "string" &&
    typeof provenance.label === "string" &&
    typeof provenance.providerKey === "string" &&
    typeof provenance.sourceId === "string" &&
    typeof provenance.sourceKey === "string" &&
    typeof provenance.sourceKind === "string" &&
    WEATHER_RECORD_SOURCE_KINDS.has(provenance.sourceKind as SiteSource["kind"]) &&
    typeof provenance.stationSlug === "string";
}

// validate one complete raw weather record
function isRawWeatherRecord(value: Record<string, unknown>): boolean {
  const freshness = forecastAdjustmentObject(value.freshness);

  // require safe freshness metadata
  if (
    freshness === null ||
    typeof freshness.ageSeconds !== "number" ||
    !Number.isSafeInteger(freshness.ageSeconds) ||
    freshness.ageSeconds < 0 ||
    typeof freshness.label !== "string" ||
    typeof freshness.status !== "string" ||
    !WEATHER_RECORD_FRESHNESS_STATUSES.has(
      freshness.status as WeatherRecord["freshness"]["status"],
    )
  ) {
    return false;
  }

  return typeof value.id === "string" &&
    isWeatherRecordMetadata(value.metadata) &&
    isWeatherRecordMetrics(value.metrics) &&
    (value.productRunAt === null || isForecastAdjustmentInstant(value.productRunAt)) &&
    isWeatherRecordProvenance(value.provenance) &&
    isForecastAdjustmentInstant(value.receivedAt) &&
    typeof value.revisionCount === "number" &&
    Number.isSafeInteger(value.revisionCount) &&
    value.revisionCount >= 0 &&
    isForecastAdjustmentInstant(value.validAt);
}

// resolve one exact supported lead band
function forecastAdjustmentLeadBand(
  targetLeadHours: number,
): ForecastAdjustmentLeadBand | null {
  // reject unsupported leads
  if (!Number.isSafeInteger(targetLeadHours) || targetLeadHours < 1 || targetLeadHours > 168) {
    return null;
  }

  const minimum = Math.floor((targetLeadHours - 1) / 24) * 24 + 1;
  const maximum = minimum + 23;
  return `${String(minimum).padStart(3, "0")}-${String(maximum).padStart(3, "0")}` as ForecastAdjustmentLeadBand;
}

// validate one adjusted metric value
function isForecastAdjustmentMetricValue(
  metric: ForecastAdjustmentMetric,
  value: unknown,
): value is number {
  const bounds = FORECAST_ADJUSTMENT_METRIC_BOUNDS[metric];
  return typeof value === "number" &&
    Number.isFinite(value) &&
    value >= bounds.minimum &&
    value <= bounds.maximum &&
    (bounds.maximumExclusive !== true || value < bounds.maximum);
}

// parse one bounded reason code
function parseForecastAdjustmentReasonCode(
  value: unknown,
): ForecastAdjustmentReasonCode | null {
  return typeof value === "string" &&
    FORECAST_ADJUSTMENT_REASON_CODE_KEYS.has(value as ForecastAdjustmentReasonCode)
    ? value as ForecastAdjustmentReasonCode
    : null;
}

// parse the response-level runtime state
function parseForecastAdjustmentRuntime(
  value: unknown,
): ForecastAdjustmentRuntimeStatus | null {
  const decoded = forecastAdjustmentObject(value);
  const runtime = decoded !== null &&
      hasExactForecastAdjustmentKeys(decoded, FORECAST_ADJUSTMENT_RUNTIME_LEGACY_KEYS)
    ? {
        ...decoded,
        actionSha256: null,
        fullMemberRootSha256: null,
        policyReportSha256: null,
      }
    : decoded;
  const enabledMetrics = Array.isArray(runtime?.enabledMetrics)
    ? runtime.enabledMetrics
    : [];
  const uniqueMetrics = new Set(enabledMetrics);

  // require the exact runtime envelope
  if (
    runtime === null ||
    !hasExactForecastAdjustmentKeys(runtime, FORECAST_ADJUSTMENT_RUNTIME_KEYS) ||
    !isForecastAdjustmentInstant(runtime.loadedAt) ||
    enabledMetrics.length !== uniqueMetrics.size ||
    enabledMetrics.some(
      // reject every unknown enabled metric
      (metric) => typeof metric !== "string" ||
        !FORECAST_ADJUSTMENT_METRIC_KEYS.has(metric as ForecastAdjustmentMetric),
    )
  ) {
    return null;
  }

  // accept one complete active identity
  if (
    runtime.state === "active" &&
    (runtime.activationMode === "maintenance_qualified" ||
      runtime.activationMode === "qualified" || runtime.activationMode === "wind_canary") &&
    runtime.reasonCode === null &&
    isForecastAdjustmentSha256(runtime.activeBundle) &&
    isForecastAdjustmentSha256(runtime.candidateArtifactSha256) &&
    enabledMetrics.length > 0 &&
    (runtime.expiresAt === null || isForecastAdjustmentInstant(runtime.expiresAt)) &&
    (runtime.activationMode === "qualified" || (
      enabledMetrics.every(
        // confine a permanent or bounded activation to wind metrics
        (metric) => metric === "windDirectionDegrees" ||
          metric === "windGustMps" ||
          metric === "windSpeedMps",
      )
    )) &&
    (runtime.activationMode === "wind_canary"
      ? runtime.evaluationReportSha256 === null &&
        runtime.qualificationReceiptSha256 === null &&
        isForecastAdjustmentSha256(runtime.authorizationSha256) &&
        isForecastAdjustmentSha256(runtime.transferReportSha256) &&
        runtime.actionSha256 === null && runtime.fullMemberRootSha256 === null &&
        runtime.policyReportSha256 === null
      : runtime.activationMode === "maintenance_qualified"
        ? runtime.evaluationReportSha256 === null && runtime.qualificationReceiptSha256 === null &&
          runtime.authorizationSha256 === null && runtime.transferReportSha256 === null &&
          isForecastAdjustmentSha256(runtime.actionSha256) &&
          isForecastAdjustmentSha256(runtime.fullMemberRootSha256) &&
          isForecastAdjustmentSha256(runtime.policyReportSha256)
        : isForecastAdjustmentSha256(runtime.evaluationReportSha256) &&
        isForecastAdjustmentSha256(runtime.qualificationReceiptSha256) &&
        runtime.authorizationSha256 === null &&
        runtime.transferReportSha256 === null && runtime.actionSha256 === null &&
        runtime.fullMemberRootSha256 === null && runtime.policyReportSha256 === null)
  ) {
    return {
      ...runtime,
      enabledMetrics: enabledMetrics as ForecastAdjustmentMetric[],
    } as unknown as ForecastAdjustmentRuntimeStatus;
  }

  const reasonCode = parseForecastAdjustmentReasonCode(runtime.reasonCode);

  // accept one complete disabled identity
  if (
    runtime.state === "disabled" &&
    runtime.actionSha256 === null &&
    runtime.activationMode === null &&
    reasonCode !== null &&
    runtime.activeBundle === null &&
    runtime.authorizationSha256 === null &&
    runtime.candidateArtifactSha256 === null &&
    enabledMetrics.length === 0 &&
    runtime.evaluationReportSha256 === null &&
    runtime.expiresAt === null &&
    runtime.fullMemberRootSha256 === null &&
    runtime.qualificationReceiptSha256 === null
    && runtime.policyReportSha256 === null
    && runtime.transferReportSha256 === null
  ) {
    return { ...runtime, reasonCode } as ForecastAdjustmentRuntimeStatus;
  }

  return null;
}

// parse one bounded temperature reason
function parseForecastTemperatureReasonCode(
  value: unknown,
): ForecastTemperatureCanaryReasonCode | null {
  return typeof value === "string" &&
    FORECAST_TEMPERATURE_REASON_CODE_KEYS.has(
      value as ForecastTemperatureCanaryReasonCode,
    )
    ? value as ForecastTemperatureCanaryReasonCode
    : null;
}

// parse one bounded collector status
function parseForecastTemperatureRuntimeSource(
  value: unknown,
): ForecastTemperatureAdjustmentRuntimeStatus["source"] | undefined {
  // preserve a valid empty collector state
  if (value === null) {
    return null;
  }

  const source = forecastAdjustmentObject(value);

  // require the exact public monitoring shape
  if (
    source === null ||
    !hasExactForecastAdjustmentKeys(
      source,
      FORECAST_TEMPERATURE_RUNTIME_SOURCE_KEYS,
    ) ||
    typeof source.adaptiveReady !== "boolean" ||
    !isForecastAdjustmentInstant(source.firstReceivedAt) ||
    !Number.isSafeInteger(source.hourCount) ||
    (source.hourCount as number) < 0 ||
    (source.hourCount as number) > 19 ||
    !isForecastAdjustmentInstant(source.latestRunInitializedAt) ||
    (source.stateReason !== null &&
      !isBoundedForecastAdjustmentText(source.stateReason)) ||
    (source.stateStatus !== "cold" &&
      source.stateStatus !== "insufficient" &&
      source.stateStatus !== "invalid" &&
      source.stateStatus !== "supported") ||
    source.adaptiveReady !== (source.stateStatus === "supported")
  ) {
    return undefined;
  }

  return source as unknown as ForecastTemperatureAdjustmentRuntimeStatus["source"];
}

// parse the independent response-level temperature runtime
function parseForecastTemperatureAdjustmentRuntime(
  value: unknown,
): ForecastTemperatureAdjustmentRuntimeStatus | null {
  const parsed = forecastAdjustmentObject(value);
  const runtime = parsed !== null && hasExactForecastAdjustmentKeys(
    parsed,
    FORECAST_TEMPERATURE_RUNTIME_LEGACY_KEYS,
  ) ? {
      ...parsed,
      actionSha256: null,
      activationMode: parsed.state === "active" ? "temperature_canary" : null,
      fullMemberRootSha256: null,
      policyReportSha256: null,
    } : parsed;

  // require one exact runtime envelope
  if (
    runtime === null ||
    !hasExactForecastAdjustmentKeys(runtime, FORECAST_TEMPERATURE_RUNTIME_KEYS) ||
    !isForecastAdjustmentInstant(runtime.loadedAt)
  ) {
    return null;
  }

  const source = parseForecastTemperatureRuntimeSource(runtime.source);

  // reject malformed monitoring independently
  if (source === undefined) {
    return null;
  }

  // accept one complete active selection
  if (
    runtime.state === "active" &&
    runtime.reasonCode === null &&
    isForecastAdjustmentSha256(runtime.activeBundle) &&
    (runtime.activationMode === "temperature_canary"
      ? isForecastAdjustmentSha256(runtime.authorizationSha256) &&
        runtime.actionSha256 === null && runtime.fullMemberRootSha256 === null &&
        runtime.policyReportSha256 === null
      : runtime.activationMode === "maintenance_qualified" && runtime.authorizationSha256 === null &&
        isForecastAdjustmentSha256(runtime.actionSha256) &&
        isForecastAdjustmentSha256(runtime.fullMemberRootSha256) &&
        isForecastAdjustmentSha256(runtime.policyReportSha256)) &&
    (runtime.expiresAt === null || isForecastAdjustmentInstant(runtime.expiresAt))
  ) {
    return { ...runtime, source } as ForecastTemperatureAdjustmentRuntimeStatus;
  }

  const reasonCode = parseForecastTemperatureReasonCode(runtime.reasonCode);

  // accept one fully redacted disabled selection
  if (
    runtime.state === "disabled" &&
    runtime.actionSha256 === null &&
    runtime.activationMode === null &&
    reasonCode !== null &&
    runtime.activeBundle === null &&
    runtime.authorizationSha256 === null &&
    runtime.expiresAt === null &&
    runtime.fullMemberRootSha256 === null &&
    runtime.policyReportSha256 === null &&
    source === null
  ) {
    return { ...runtime, reasonCode, source } as ForecastTemperatureAdjustmentRuntimeStatus;
  }

  return null;
}

// parse one exact live ECMWF source receipt
function parseForecastTemperatureSource(
  value: unknown,
  record: WeatherRecord,
): ForecastTemperatureCanarySource | null {
  const source = forecastAdjustmentObject(value);

  // require one closed source schema
  if (
    source === null ||
    !hasExactForecastAdjustmentKeys(source, FORECAST_TEMPERATURE_SOURCE_KEYS) ||
    source.adapterVersion !== "open-meteo-ecmwf-single-run/v1" ||
    source.dataset !== "single_run" ||
    !isForecastAdjustmentInstant(source.firstReceivedAt) ||
    (source.modelCycle !== "49r1" && source.modelCycle !== "50r1") ||
    !Number.isSafeInteger(source.modelLeadHours) ||
    (source.modelLeadHours as number) < 7 ||
    (source.modelLeadHours as number) > 18 ||
    !Number.isSafeInteger(source.operationalHorizonHours) ||
    source.operationalHorizonHours !== (source.modelLeadHours as number) - 6 ||
    source.providerKey !== "open-meteo" ||
    !isForecastAdjustmentSha256(source.providerResponseSha256) ||
    typeof source.rawTemperatureC !== "number" ||
    !Number.isFinite(source.rawTemperatureC) ||
    source.rawTemperatureC < -100 ||
    source.rawTemperatureC > 70 ||
    (source.rawRelativeHumidityPercent !== null &&
      (typeof source.rawRelativeHumidityPercent !== "number" ||
        !Number.isFinite(source.rawRelativeHumidityPercent) ||
        source.rawRelativeHumidityPercent < 0 ||
        source.rawRelativeHumidityPercent > 100)) ||
    (source.rawWindSpeedMps !== null &&
      (typeof source.rawWindSpeedMps !== "number" ||
        !Number.isFinite(source.rawWindSpeedMps) ||
        source.rawWindSpeedMps < 0 ||
        source.rawWindSpeedMps > 150)) ||
    !isForecastAdjustmentInstant(source.runInitializedAt) ||
    source.upstreamModel !== "ecmwf_ifs" ||
    !isForecastAdjustmentInstant(source.validAt) ||
    source.validAt !== record.validAt
  ) {
    return null;
  }

  const initializedAt = Date.parse(source.runInitializedAt as string);
  const receivedAt = Date.parse(source.firstReceivedAt as string);
  const validAt = Date.parse(source.validAt as string);

  // bind receipt, lead, and valid-hour identities
  if (
    receivedAt < initializedAt ||
    validAt - initializedAt !==
      (source.modelLeadHours as number) * 3_600_000
  ) {
    return null;
  }

  return source as unknown as ForecastTemperatureCanarySource;
}

// parse one independent per-hour temperature decision
function parseForecastTemperatureDecision(
  value: unknown,
  record: WeatherRecord,
  runtime: ForecastTemperatureAdjustmentRuntimeStatus,
): ForecastTemperatureCanaryDecision | null {
  const decision = forecastAdjustmentObject(value);

  // require one exact decision envelope and raw Best Match binding
  if (
    decision === null ||
    !hasExactForecastAdjustmentKeys(decision, FORECAST_TEMPERATURE_DECISION_KEYS) ||
    decision.contractVersion !== "forecast-temperature-canary-decision/v1" ||
    decision.rawBestMatchTemperatureC !== record.metrics.temperatureC
  ) {
    return null;
  }

  // accept only fully linked active decisions
  if (decision.state === "active") {
    const source = parseForecastTemperatureSource(decision.sourceForecast, record);

    // bind active output to the runtime and physical range
    if (
      runtime.state !== "active" ||
      source === null ||
      decision.bundleSha256 !== runtime.activeBundle ||
      (decision.branch !== "adaptive" && decision.branch !== "direct") ||
      typeof decision.correctedTemperatureC !== "number" ||
      !Number.isFinite(decision.correctedTemperatureC) ||
      decision.correctedTemperatureC < -100 ||
      decision.correctedTemperatureC > 70 ||
      decision.reasonCode !== null ||
      !isForecastAdjustmentSha256(decision.recentErrorStateSha256)
    ) {
      return null;
    }

    return { ...decision, sourceForecast: source } as ForecastTemperatureCanaryDecision;
  }

  const reasonCode = parseForecastTemperatureReasonCode(decision.reasonCode);

  // accept schema-complete raw fallbacks only
  if (
    reasonCode === null ||
    decision.branch !== null ||
    decision.correctedTemperatureC !== null ||
    (decision.state !== "disabled" && decision.state !== "raw_fallback") ||
    (decision.state === "disabled" &&
      (runtime.state !== "disabled" ||
        decision.bundleSha256 !== null ||
        decision.recentErrorStateSha256 !== null ||
        decision.sourceForecast !== null)) ||
    (decision.state === "raw_fallback" &&
      (runtime.state !== "active" ||
        decision.bundleSha256 !== runtime.activeBundle ||
        (decision.recentErrorStateSha256 !== null &&
          !isForecastAdjustmentSha256(decision.recentErrorStateSha256)) ||
        (decision.sourceForecast !== null &&
          parseForecastTemperatureSource(decision.sourceForecast, record) === null)))
  ) {
    return null;
  }

  return { ...decision, reasonCode } as ForecastTemperatureCanaryDecision;
}

// parse one unchanged-raw row decision
function parseForecastAdjustmentFailRawDecision(
  value: Record<string, unknown>,
): ForecastAdjustmentFailRawDecision | null {
  const adjustedMetrics = forecastAdjustmentObject(value.adjustedMetrics);
  const reasonCode = parseForecastAdjustmentReasonCode(value.reasonCode);

  // require the exact empty fail-raw contract
  if (
    !hasExactForecastAdjustmentKeys(value, FORECAST_ADJUSTMENT_FAIL_RAW_KEYS) ||
    value.contractVersion !== "forecast-adjustment-decision/v1" ||
    (value.state !== "disabled" && value.state !== "not_applicable") ||
    reasonCode === null ||
    adjustedMetrics === null ||
    Object.keys(adjustedMetrics).length !== 0 ||
    !Array.isArray(value.appliedMetrics) ||
    value.appliedMetrics.length !== 0
  ) {
    return null;
  }

  return { ...value, reasonCode } as unknown as ForecastAdjustmentFailRawDecision;
}

// parse one active row decision
function parseForecastAdjustmentActiveDecision(
  value: Record<string, unknown>,
  record: WeatherRecord,
  runtime: ForecastAdjustmentRuntimeStatus,
): ForecastAdjustmentActiveDecision | null {
  const adjustedMetrics = forecastAdjustmentObject(value.adjustedMetrics);
  const provenance = forecastAdjustmentObject(value.rawForecastProvenance);
  const appliedMetrics = Array.isArray(value.appliedMetrics)
    ? value.appliedMetrics
    : [];
  const canary = runtime.activationMode === "wind_canary";
  const maintenance = runtime.activationMode === "maintenance_qualified";

  // require the exact active envelope and runtime cross-links
  if (
    !hasExactForecastAdjustmentKeys(
      value,
      canary
        ? FORECAST_ADJUSTMENT_WIND_CANARY_ACTIVE_KEYS
        : maintenance
          ? FORECAST_ADJUSTMENT_MAINTENANCE_ACTIVE_KEYS
          : FORECAST_ADJUSTMENT_ACTIVE_KEYS,
    ) ||
    value.contractVersion !== "forecast-adjustment-decision/v1" ||
    value.algorithmContractVersion !== "robust-hierarchical-median/v1" ||
    value.state !== "active" ||
    value.reasonCode !== null ||
    runtime.state !== "active" ||
    value.candidateArtifactSha256 !== runtime.candidateArtifactSha256 ||
    (canary
      ? value.activationKind !== "wind_transfer_canary" ||
        value.authorizationSha256 !== runtime.authorizationSha256 ||
        value.transferReportSha256 !== runtime.transferReportSha256
      : maintenance
        ? value.activationKind !== "maintenance_qualified" ||
          value.actionSha256 !== runtime.actionSha256 ||
          value.fullMemberRootSha256 !== runtime.fullMemberRootSha256 ||
          value.policyReportSha256 !== runtime.policyReportSha256
        : value.evaluationReportSha256 !== runtime.evaluationReportSha256 ||
          value.qualificationReceiptSha256 !== runtime.qualificationReceiptSha256) ||
    adjustedMetrics === null ||
    provenance === null ||
    !hasExactForecastAdjustmentKeys(provenance, FORECAST_ADJUSTMENT_RAW_PROVENANCE_KEYS)
  ) {
    return null;
  }

  const targetLeadHours = provenance.targetLeadHours;
  const leadBand = typeof targetLeadHours === "number"
    ? forecastAdjustmentLeadBand(targetLeadHours)
    : null;
  const referenceAt = isForecastAdjustmentInstant(provenance.referenceAt)
    ? Date.parse(provenance.referenceAt)
    : Number.NaN;
  const validAt = isForecastAdjustmentInstant(provenance.validAt)
    ? Date.parse(provenance.validAt)
    : Number.NaN;
  const derivedLeadHours = Math.ceil((validAt - referenceAt) / 3_600_000);

  // bind the active decision to the displayed raw row
  if (
    provenance.cohort !== "legacy_v4_retrieval_snapshot" ||
    provenance.referenceKind !== "retrieval_snapshot" ||
    !isBoundedForecastAdjustmentText(provenance.adapterVersion) ||
    !isBoundedForecastAdjustmentText(provenance.contractEpoch) ||
    !isBoundedForecastAdjustmentText(provenance.dataset) ||
    !isBoundedForecastAdjustmentText(provenance.sourceKey) ||
    !isBoundedForecastAdjustmentText(provenance.upstreamModel) ||
    !isForecastAdjustmentSha256(provenance.sourceConfigFingerprint) ||
    leadBand === null ||
    derivedLeadHours !== targetLeadHours ||
    value.leadBand !== leadBand ||
    provenance.validAt !== record.validAt ||
    provenance.sourceKey !== record.provenance.sourceKey ||
    provenance.dataset !== record.metadata.provider?.dataset ||
    provenance.upstreamModel !== record.metadata.upstream.model
  ) {
    return null;
  }

  const uniqueMetrics = new Set<ForecastAdjustmentMetric>();

  // validate every adjusted metric contribution
  for (const metric of appliedMetrics) {
    // reject duplicates and unsupported metrics
    if (
      typeof metric !== "string" ||
      !FORECAST_ADJUSTMENT_METRIC_KEYS.has(metric as ForecastAdjustmentMetric) ||
      !runtime.enabledMetrics.includes(metric as ForecastAdjustmentMetric) ||
      ((canary || maintenance) &&
        metric !== "windDirectionDegrees" &&
        metric !== "windGustMps" &&
        metric !== "windSpeedMps") ||
      uniqueMetrics.has(metric as ForecastAdjustmentMetric)
    ) {
      return null;
    }

    const typedMetric = metric as ForecastAdjustmentMetric;
    const rawValue = record.metrics[typedMetric];
    const adjustedValue = adjustedMetrics[typedMetric];

    // require usable raw and adjusted values
    if (
      rawValue === null ||
      !Number.isFinite(rawValue) ||
      !isForecastAdjustmentMetricValue(typedMetric, adjustedValue)
    ) {
      return null;
    }

    uniqueMetrics.add(typedMetric);
  }

  // require a nonempty exact adjusted metric set
  if (
    uniqueMetrics.size === 0 ||
    Object.keys(adjustedMetrics).length !== uniqueMetrics.size ||
    Object.keys(adjustedMetrics).some((metric) => !uniqueMetrics.has(metric as ForecastAdjustmentMetric))
  ) {
    return null;
  }

  return value as unknown as ForecastAdjustmentActiveDecision;
}

// parse one row decision without affecting raw values
function parseForecastAdjustmentDecision(
  value: unknown,
  record: WeatherRecord,
  runtime: ForecastAdjustmentRuntimeStatus,
): ForecastAdjustmentDecision | null {
  const decision = forecastAdjustmentObject(value);

  // reject missing and malformed decisions
  if (decision === null) {
    return null;
  }

  return decision.state === "active"
    ? parseForecastAdjustmentActiveDecision(decision, record, runtime)
    : parseForecastAdjustmentFailRawDecision(decision);
}

// accept the three independent adjustment controls
function parseForecastAdjustmentSettings(value: unknown): ForecastAdjustmentSettings | null {
  const settings = forecastAdjustmentObject(value);

  // reject partial and unversioned settings
  if (
    settings === null ||
    !hasExactForecastAdjustmentKeys(settings, FORECAST_ADJUSTMENT_SETTINGS_KEYS) ||
    settings.version !== 1 ||
    typeof settings.temperature !== "boolean" ||
    typeof settings.wind !== "boolean" ||
    typeof settings.rain !== "boolean"
  ) {
    return null;
  }

  return {
    version: 1,
    temperature: settings.temperature,
    wind: settings.wind,
    rain: settings.rain,
  };
}

// validate one bounded aggregate count
function isForecastAdjustmentScorecardCount(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0 && (value as number) <= 1_000_000_000;
}

// validate one nullable finite scorecard value
function isForecastAdjustmentScorecardNumber(value: unknown): value is number | null {
  return value === null || (typeof value === "number" && Number.isFinite(value));
}

// validate one nonnegative nullable scorecard value
function isForecastAdjustmentScorecardNonnegative(value: unknown): value is number | null {
  return isForecastAdjustmentScorecardNumber(value) && (value === null || value >= 0);
}

// validate one canonical scorecard UTC instant
function isForecastAdjustmentScorecardInstant(value: unknown): value is string {
  return typeof value === "string" &&
    value.endsWith("Z") &&
    Number.isFinite(Date.parse(value)) &&
    new Date(value).toISOString() === value;
}

// validate one closed reason-count map
function isForecastAdjustmentScorecardReasonMap(value: unknown): boolean {
  const reasons = forecastAdjustmentObject(value);

  // reject nonobjects and unbounded diagnostic sets
  if (reasons === null || Object.keys(reasons).length > 64) {
    return false;
  }

  return Object.entries(reasons).every(
    // accept only safe codes and aggregate counts
    ([code, count]) => /^[a-z][a-z0-9_]{0,79}$/u.test(code) &&
      isForecastAdjustmentScorecardCount(count),
  );
}

// validate one aggregate metric block
function isForecastAdjustmentScorecardMetric(
  value: unknown,
  unit: ForecastAdjustmentScorecardMetric["unit"],
): value is ForecastAdjustmentScorecardMetric {
  const metric = forecastAdjustmentObject(value);

  // require the exact shared metric shape
  if (
    metric === null ||
    !hasExactForecastAdjustmentKeys(metric, FORECAST_ADJUSTMENT_SCORECARD_METRIC_KEYS) ||
    metric.unit !== unit ||
    !isForecastAdjustmentScorecardNonnegative(metric.rawMae) ||
    !isForecastAdjustmentScorecardNonnegative(metric.adjustedMae) ||
    !isForecastAdjustmentScorecardNumber(metric.deltaMae) ||
    !isForecastAdjustmentScorecardNumber(metric.rawBias) ||
    !isForecastAdjustmentScorecardNumber(metric.adjustedBias) ||
    !isForecastAdjustmentScorecardNonnegative(metric.rawRmse) ||
    !isForecastAdjustmentScorecardNonnegative(metric.adjustedRmse) ||
    !isForecastAdjustmentScorecardNonnegative(metric.rawP95) ||
    !isForecastAdjustmentScorecardNonnegative(metric.adjustedP95) ||
    !isForecastAdjustmentScorecardNumber(metric.skillPercent)
  ) {
    return false;
  }

  // accept explicit interval absence
  if (metric.skillInterval95 === null) {
    return true;
  }

  const interval = forecastAdjustmentObject(metric.skillInterval95);
  return interval !== null &&
    hasExactForecastAdjustmentKeys(interval, new Set(["lower", "upper"])) &&
    typeof interval.lower === "number" && Number.isFinite(interval.lower) &&
    typeof interval.upper === "number" && Number.isFinite(interval.upper) &&
    interval.lower <= interval.upper;
}

// validate one family support summary
function isForecastAdjustmentScorecardSupport(value: unknown): value is ForecastAdjustmentScorecardSupport {
  const support = forecastAdjustmentObject(value);

  // require every closed support field
  if (support === null ||
    !hasExactForecastAdjustmentKeys(support, FORECAST_ADJUSTMENT_SCORECARD_SUPPORT_KEYS)) {
    return false;
  }

  const countKeys = [
    "dateCount",
    "eventCount",
    "excludedCount",
    "fallbackCount",
    "gapCount",
    "rowCount",
    "targetRowCount",
    "validHourCount",
    "vintageCount",
    "wetDateCount",
    "wetRowCount",
  ];
  return countKeys.every(
    // require one bounded count
    (key) => isForecastAdjustmentScorecardCount(support[key]),
  ) &&
    typeof support.effectiveWeightSum === "number" &&
    Number.isFinite(support.effectiveWeightSum) &&
    support.effectiveWeightSum >= 0 &&
    isForecastAdjustmentScorecardReasonMap(support.exclusionReasons) &&
    isForecastAdjustmentScorecardReasonMap(support.fallbackReasons);
}

// validate one separately matched best-match block
function isForecastAdjustmentBestMatchDiagnostic(
  value: unknown,
  unit: ForecastAdjustmentScorecardMetric["unit"],
  family: ForecastAdjustmentScorecardFamilyName,
): value is ForecastAdjustmentBestMatchDiagnostic | null {
  // wind already uses best match as its primary baseline
  if (family === "wind") {
    return value === null;
  }

  // preserve honest diagnostic absence
  if (value === null) {
    return true;
  }

  const diagnostic = forecastAdjustmentObject(value);
  return diagnostic !== null &&
    hasExactForecastAdjustmentKeys(diagnostic, FORECAST_ADJUSTMENT_SCORECARD_BEST_MATCH_KEYS) &&
    diagnostic.unit === unit &&
    isForecastAdjustmentScorecardCount(diagnostic.rowCount) &&
    isForecastAdjustmentScorecardCount(diagnostic.dateCount) &&
    isForecastAdjustmentScorecardNonnegative(diagnostic.bestMatchRawMae) &&
    isForecastAdjustmentScorecardNonnegative(diagnostic.sourceRawMae) &&
    isForecastAdjustmentScorecardNonnegative(diagnostic.sourceAdjustedMae);
}

// validate rain-only aggregate diagnostics
function isForecastAdjustmentRainDiagnostics(value: unknown): value is ForecastAdjustmentRainDiagnostics {
  const rain = forecastAdjustmentObject(value);

  // require the exact rain diagnostic envelope
  if (rain === null ||
    !hasExactForecastAdjustmentKeys(rain, FORECAST_ADJUSTMENT_SCORECARD_RAIN_KEYS) ||
    !isForecastAdjustmentScorecardNonnegative(rain.annualBalancedVolumeRatio) ||
    !isForecastAdjustmentScorecardNonnegative(rain.winterBalancedVolumeRatio) ||
    !isForecastAdjustmentScorecardNonnegative(rain.wetRawMae) ||
    !isForecastAdjustmentScorecardNonnegative(rain.wetAdjustedMae) ||
    !isForecastAdjustmentScorecardNonnegative(rain.heavyRawMae) ||
    !isForecastAdjustmentScorecardNonnegative(rain.heavyAdjustedMae) ||
    !isForecastAdjustmentScorecardCount(rain.probabilityOrderViolationCount) ||
    !Array.isArray(rain.thresholds) || rain.thresholds.length !== 3 ||
    !Array.isArray(rain.accumulations) || rain.accumulations.length !== 3
  ) {
    return false;
  }

  const thresholdValues = [0.1, 1, 2.5];
  const thresholdsValid = rain.thresholds.every(
    // validate one fixed threshold and its complete reliability deciles
    (value, index) => {
      const threshold = forecastAdjustmentObject(value);

      // reject malformed threshold aggregates
      if (threshold === null ||
        !hasExactForecastAdjustmentKeys(threshold, FORECAST_ADJUSTMENT_SCORECARD_THRESHOLD_KEYS) ||
        threshold.thresholdMmPerHour !== thresholdValues[index] ||
        !isForecastAdjustmentScorecardNonnegative(threshold.rawBrier) ||
        !isForecastAdjustmentScorecardNonnegative(threshold.adjustedBrier) ||
        !isForecastAdjustmentScorecardCount(threshold.hits) ||
        !isForecastAdjustmentScorecardCount(threshold.misses) ||
        !isForecastAdjustmentScorecardCount(threshold.falseAlarms) ||
        !isForecastAdjustmentScorecardNonnegative(threshold.pod) ||
        !isForecastAdjustmentScorecardNonnegative(threshold.far) ||
        !isForecastAdjustmentScorecardNonnegative(threshold.csi) ||
        !Array.isArray(threshold.reliability) || threshold.reliability.length !== 10
      ) {
        return false;
      }

      return [threshold.rawBrier, threshold.adjustedBrier, threshold.pod, threshold.far, threshold.csi].every(
        // keep probability measures within their physical range
        (metric) => metric === null || metric <= 1,
      ) && threshold.reliability.every(
        // validate one fixed reliability bin
        (binValue) => {
          const bin = forecastAdjustmentObject(binValue);
          return bin !== null &&
            hasExactForecastAdjustmentKeys(bin, FORECAST_ADJUSTMENT_SCORECARD_RELIABILITY_KEYS) &&
            isForecastAdjustmentScorecardCount(bin.count) &&
            isForecastAdjustmentScorecardNonnegative(bin.meanProbability) &&
            isForecastAdjustmentScorecardNonnegative(bin.observedFrequency) &&
            (bin.meanProbability === null || bin.meanProbability <= 1) &&
            (bin.observedFrequency === null || bin.observedFrequency <= 1);
        },
      );
    },
  );

  // stop before validating accumulation windows
  if (!thresholdsValid) {
    return false;
  }

  const accumulationHours = [6, 12, 23];
  return rain.accumulations.every(
    // validate one fixed same-run accumulation window
    (value, index) => {
      const accumulation = forecastAdjustmentObject(value);
      return accumulation !== null &&
        hasExactForecastAdjustmentKeys(accumulation, FORECAST_ADJUSTMENT_SCORECARD_ACCUMULATION_KEYS) &&
        accumulation.hours === accumulationHours[index] &&
        isForecastAdjustmentScorecardCount(accumulation.completeWindows) &&
        isForecastAdjustmentScorecardNonnegative(accumulation.rawMae) &&
        isForecastAdjustmentScorecardNonnegative(accumulation.adjustedMae);
    },
  );
}

// validate one closed model-family card
function isForecastAdjustmentScorecardFamily(
  value: unknown,
  family: ForecastAdjustmentScorecardFamilyName,
): value is ForecastAdjustmentScorecardFamily {
  const card = forecastAdjustmentObject(value);
  const unit = family === "temperature"
    ? "celsius"
    : family === "wind"
      ? "meters_per_second"
      : "millimeters_per_hour";

  // require the exact family states and identities
  if (
    card === null ||
    !hasExactForecastAdjustmentKeys(card, FORECAST_ADJUSTMENT_SCORECARD_FAMILY_KEYS) ||
    card.family !== family ||
    (card.servingIdentitySha256 !== null && !isForecastAdjustmentSha256(card.servingIdentitySha256)) ||
    !["as_issued", "prospective_receipt", "retrospective_counterfactual", "development"].includes(card.evidenceClass as string) ||
    (card.evidenceCutoffAt !== null && !isForecastAdjustmentScorecardInstant(card.evidenceCutoffAt)) ||
    !["sufficient", "insufficient", "invalid"].includes(card.supportState as string) ||
    !["unscored", "better", "mixed", "worse"].includes(card.comparisonState as string) ||
    !["development_only", "counterfactual_only", "pending_support", "supported", "rejected"].includes(card.qualificationState as string) ||
    !["authorized_active", "admin_disabled", "fail_raw", "pending_review"].includes(card.servingState as string) ||
    !["retain", "review_candidate", "review_disable", "none"].includes(card.recommendation as string) ||
    !isForecastAdjustmentScorecardSupport(card.support) ||
    !isForecastAdjustmentScorecardMetric(card.metrics, unit) ||
    !isForecastAdjustmentBestMatchDiagnostic(card.bestMatchDiagnostic, unit, family) ||
    !Array.isArray(card.slices) || card.slices.length > 256
  ) {
    return false;
  }

  const slicesValid = card.slices.every(
    // validate one bounded aggregate slice
    (value) => {
      const slice = forecastAdjustmentObject(value);
      return slice !== null &&
        hasExactForecastAdjustmentKeys(slice, FORECAST_ADJUSTMENT_SCORECARD_SLICE_KEYS) &&
        ["horizon", "month", "season", "daypart"].includes(slice.dimension as string) &&
        typeof slice.label === "string" && /^[a-z0-9][a-z0-9_.:+-]{0,79}$/u.test(slice.label) &&
        isForecastAdjustmentScorecardCount(slice.rowCount) &&
        isForecastAdjustmentScorecardMetric(slice.metrics, unit);
    },
  );

  // preserve rain-only diagnostics
  if (!slicesValid) {
    return false;
  }

  return family === "rain"
    ? isForecastAdjustmentRainDiagnostics(card.rainDiagnostics)
    : card.rainDiagnostics === null;
}

// reject private confirmation member fields at every response depth
function hasForecastAdjustmentPrivateMemberField(value: unknown): boolean {
  // stop recursion at scalar leaves
  if (value === null || typeof value !== "object") {
    return false;
  }

  // inspect every bounded array member
  if (Array.isArray(value)) {
    return value.some(
      // reject one nested private field
      (entry) => hasForecastAdjustmentPrivateMemberField(entry),
    );
  }

  return Object.entries(value).some(
    // reject private and reserved-member result fields
    ([key, entry]) => /(?:private|reserved.*member|member.*result)/iu.test(key) ||
      hasForecastAdjustmentPrivateMemberField(entry),
  );
}

// validate common immutable scorecard inputs
function isForecastAdjustmentScorecardInputs(
  value: unknown,
  expectedKeys: ReadonlySet<string>,
): value is ForecastAdjustmentScorecardInputs {
  const inputs = forecastAdjustmentObject(value);
  const reportSha256s = forecastAdjustmentObject(inputs?.reportSha256s);
  return inputs !== null &&
    hasExactForecastAdjustmentKeys(inputs, expectedKeys) &&
    isForecastAdjustmentSha256(inputs.adjustmentEvidenceManifestSha256) &&
    isForecastAdjustmentSha256(inputs.adjustmentEvidenceWatermarkSha256) &&
    isForecastAdjustmentSha256(inputs.forecastTrainingManifestSha256) &&
    typeof inputs.localDateFrom === "string" && /^\d{4}-\d{2}-\d{2}$/u.test(inputs.localDateFrom) &&
    typeof inputs.localDateTo === "string" && /^\d{4}-\d{2}-\d{2}$/u.test(inputs.localDateTo) &&
    inputs.localDateFrom <= inputs.localDateTo &&
    typeof inputs.sourceRevision === "string" && /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/u.test(inputs.sourceRevision) &&
    isForecastAdjustmentScorecardInstant(inputs.targetCutoffAt) &&
    reportSha256s !== null &&
    hasExactForecastAdjustmentKeys(reportSha256s, new Set(FORECAST_ADJUSTMENT_SCORECARD_FAMILIES)) &&
    FORECAST_ADJUSTMENT_SCORECARD_FAMILIES.every(
      // bind every family card to one immutable report
      (family) => isForecastAdjustmentSha256(reportSha256s[family]),
    );
}

// validate one complete family collection
function isForecastAdjustmentScorecardFamilies(value: unknown): boolean {
  const families = forecastAdjustmentObject(value);
  return families !== null &&
    hasExactForecastAdjustmentKeys(families, new Set(FORECAST_ADJUSTMENT_SCORECARD_FAMILIES)) &&
    FORECAST_ADJUSTMENT_SCORECARD_FAMILIES.every(
      // validate one closed aggregate card
      (family) => isForecastAdjustmentScorecardFamily(families[family], family),
    );
}

// validate one fixed family identity set
function isForecastAdjustmentScorecardIdentitySet(value: unknown, nullable: boolean): boolean {
  const identities = forecastAdjustmentObject(value);
  return identities !== null &&
    hasExactForecastAdjustmentKeys(identities, new Set(FORECAST_ADJUSTMENT_SCORECARD_FAMILIES)) &&
    FORECAST_ADJUSTMENT_SCORECARD_FAMILIES.every(
      // allow explicit absence only outside the raw fallback set
      (family) => nullable && identities[family] === null ||
        isForecastAdjustmentSha256(identities[family]),
    );
}

// validate the closed v2 automatic-policy projection
function isForecastAdjustmentScorecardV2(scorecard: Record<string, unknown>, now: number): boolean {
  const generatedAt = scorecard.generatedAt;
  const validThrough = scorecard.validThrough;
  const inputs = forecastAdjustmentObject(scorecard.inputs);
  const lineage = forecastAdjustmentObject(scorecard.actionLineage);
  const job = forecastAdjustmentObject(scorecard.job);
  const warnings = forecastAdjustmentObject(scorecard.warnings);
  const identities = forecastAdjustmentObject(scorecard.identities);
  const progress = forecastAdjustmentObject(scorecard.progress);
  const history = scorecard.history;

  // require a fresh seven-day maximum sanitized envelope
  if (!hasExactForecastAdjustmentKeys(scorecard, FORECAST_ADJUSTMENT_SCORECARD_V2_KEYS) ||
    scorecard.contractVersion !== "forecast-adjustment-scorecard/v2" ||
    scorecard.siteKey !== "ballydidean" ||
    !isForecastAdjustmentScorecardInstant(generatedAt) ||
    !isForecastAdjustmentScorecardInstant(validThrough) ||
    Date.parse(validThrough) <= Date.parse(generatedAt) ||
    Date.parse(validThrough) - Date.parse(generatedAt) > 7 * 24 * 60 * 60 * 1_000 ||
    Date.parse(validThrough) <= now ||
    !["pending", "qualified", "failed", "regressed"].includes(scorecard.policyDecision as string) ||
    !["none", "shadow_pending", "confirmation_registered", "pending_support", "release_pending", "deploying", "active", "failed", "expired_unapplied", "deployed_operator_off", "rolled_back_prior", "raw"].includes(scorecard.actionState as string) ||
    !isForecastAdjustmentSha256(scorecard.actionProjectionSha256) ||
    !isForecastAdjustmentScorecardInputs(inputs, FORECAST_ADJUSTMENT_SCORECARD_V2_INPUT_KEYS) ||
    !isForecastAdjustmentSha256(inputs.frontierSha256) ||
    !isForecastAdjustmentSha256(inputs.inputManifestSha256) ||
    !isForecastAdjustmentSha256(inputs.reportSha256) ||
    !isForecastAdjustmentScorecardFamilies(scorecard.families)) {
    return false;
  }

  // require immutable action lineage with no approval or signature fields
  if (lineage === null ||
    !hasExactForecastAdjustmentKeys(lineage, FORECAST_ADJUSTMENT_SCORECARD_V2_LINEAGE_KEYS) ||
    !isForecastAdjustmentSha256(lineage.actionSha256) ||
    !isForecastAdjustmentSha256(lineage.attemptSha256) ||
    !(lineage.predecessorActionSha256 === null || isForecastAdjustmentSha256(lineage.predecessorActionSha256)) ||
    !(lineage.releaseManifestSha256 === null || isForecastAdjustmentSha256(lineage.releaseManifestSha256)) ||
    typeof lineage.sourceRevision !== "string" ||
    !/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/u.test(lineage.sourceRevision)) {
    return false;
  }

  // validate attempt, success, due, backlog and operator states
  if (job === null ||
    !hasExactForecastAdjustmentKeys(job, FORECAST_ADJUSTMENT_SCORECARD_V2_JOB_KEYS) ||
    !["not_due", "due", "running", "completed", "failed", "blocked"].includes(job.attemptState as string) ||
    !["clear", "present", "blocked"].includes(job.backlogState as string) ||
    !["not_due", "due", "overdue", "expired"].includes(job.dueState as string) ||
    !["enabled", "disabled"].includes(job.operatorState as string) ||
    !["none", "succeeded", "failed"].includes(job.successState as string)) {
    return false;
  }

  // accept only bounded warning codes in fixed categories
  if (warnings === null ||
    !hasExactForecastAdjustmentKeys(warnings, FORECAST_ADJUSTMENT_SCORECARD_V2_WARNING_KEYS) ||
    !["source", "fallback", "gauge", "capture", "capacity"].every(
      // validate one warning category
      (key) => Array.isArray(warnings[key]) && (warnings[key] as unknown[]).length <= 16 &&
        (warnings[key] as unknown[]).every(
          // prohibit warning prose and paths
          (warning) => typeof warning === "string" && /^[a-z0-9][a-z0-9_.:+-]{0,79}$/u.test(warning),
        ),
    )) {
    return false;
  }

  // validate active, shadow, prior and raw identity projections
  if (identities === null ||
    !hasExactForecastAdjustmentKeys(identities, FORECAST_ADJUSTMENT_SCORECARD_V2_IDENTITY_KEYS) ||
    !isForecastAdjustmentScorecardIdentitySet(identities.active, true) ||
    !isForecastAdjustmentScorecardIdentitySet(identities.prior, true) ||
    !isForecastAdjustmentScorecardIdentitySet(identities.shadow, true) ||
    !isForecastAdjustmentScorecardIdentitySet(identities.raw, false)) {
    return false;
  }

  // validate bounded confirmation and rollback progress
  if (progress === null ||
    !hasExactForecastAdjustmentKeys(progress, FORECAST_ADJUSTMENT_SCORECARD_V2_PROGRESS_KEYS) ||
    !isForecastAdjustmentScorecardCount(progress.confirmationCompletedEpochs) ||
    !isForecastAdjustmentScorecardCount(progress.confirmationRequiredEpochs) ||
    !isForecastAdjustmentScorecardCount(progress.rollbackCompletedEpochs) ||
    !isForecastAdjustmentScorecardCount(progress.rollbackRequiredEpochs) ||
    (progress.confirmationCompletedEpochs as number) > (progress.confirmationRequiredEpochs as number) ||
    (progress.rollbackCompletedEpochs as number) > (progress.rollbackRequiredEpochs as number) ||
    !isForecastAdjustmentScorecardInstant(progress.rainCaptureExpiresAt)) {
    return false;
  }

  // validate the newest 64 sanitized immutable action references
  if (!Array.isArray(history) || history.length > 64) {
    return false;
  }
  let previousTime = -Infinity;
  const historyValid = history.every(
    // validate one chronological action history projection
    (entryValue) => {
      const entry = forecastAdjustmentObject(entryValue);

      // reject malformed or private history entries
      if (entry === null ||
        !hasExactForecastAdjustmentKeys(entry, FORECAST_ADJUSTMENT_SCORECARD_V2_HISTORY_KEYS) ||
        !isForecastAdjustmentSha256(entry.actionLineageSha256) ||
        !isForecastAdjustmentSha256(entry.actionProjectionSha256) ||
        !["none", "shadow_pending", "confirmation_registered", "pending_support", "release_pending", "deploying", "active", "failed", "expired_unapplied", "deployed_operator_off", "rolled_back_prior", "raw"].includes(entry.actionState as string) ||
        !isForecastAdjustmentSha256(entry.attemptSha256) ||
        !isForecastAdjustmentScorecardInstant(entry.occurredAt) ||
        !["pending", "qualified", "failed", "regressed"].includes(entry.policyDecision as string) ||
        !(entry.releaseManifestSha256 === null || isForecastAdjustmentSha256(entry.releaseManifestSha256)) ||
        Date.parse(entry.occurredAt as string) < previousTime) {
        return false;
      }
      previousTime = Date.parse(entry.occurredAt as string);
      return true;
    },
  );

  // bind the latest history entry to the current projection when present
  if (!historyValid) {
    return false;
  }
  const latest = history.at(-1);
  return latest === undefined || (
    latest.actionProjectionSha256 === scorecard.actionProjectionSha256 &&
    latest.actionState === scorecard.actionState &&
    latest.policyDecision === scorecard.policyDecision &&
    latest.attemptSha256 === lineage.attemptSha256 &&
    latest.releaseManifestSha256 === lineage.releaseManifestSha256
  );
}

// parse one authenticated closed scorecard response
export function parseForecastAdjustmentScorecard(value: unknown, now = Date.now()): ForecastAdjustmentScorecard | null {
  const scorecard = forecastAdjustmentObject(value);

  // reject nonobjects and any structurally private confirmation-member field
  if (scorecard === null || hasForecastAdjustmentPrivateMemberField(scorecard)) {
    return null;
  }

  // accept v2 only through the complete automatic-policy validator
  if (scorecard.contractVersion === "forecast-adjustment-scorecard/v2") {
    return isForecastAdjustmentScorecardV2(scorecard, now)
      ? scorecard as unknown as ForecastAdjustmentScorecardV2
      : null;
  }

  // preserve v1 as a fresh display-only historical report
  if (!hasExactForecastAdjustmentKeys(scorecard, FORECAST_ADJUSTMENT_SCORECARD_V1_KEYS) ||
    scorecard.contractVersion !== "forecast-adjustment-scorecard/v1" ||
    scorecard.siteKey !== "ballydidean" ||
    !isForecastAdjustmentScorecardInstant(scorecard.generatedAt) ||
    !isForecastAdjustmentScorecardInstant(scorecard.validThrough) ||
    Date.parse(scorecard.validThrough as string) <= Date.parse(scorecard.generatedAt as string) ||
    Date.parse(scorecard.validThrough as string) <= now ||
    scorecard.servingChanged !== false ||
    scorecard.automaticActivationEligible !== false ||
    scorecard.operatorApprovalRequired !== true ||
    !isForecastAdjustmentScorecardInputs(scorecard.inputs, FORECAST_ADJUSTMENT_SCORECARD_INPUT_KEYS) ||
    !isForecastAdjustmentScorecardFamilies(scorecard.families)) {
    return null;
  }
  return scorecard as unknown as ForecastAdjustmentScorecardV1;
}

// map adjusted metrics onto their admin controls
function forecastAdjustmentMetricEnabled(
  metric: ForecastAdjustmentMetric,
  settings: ForecastAdjustmentSettings | null,
): boolean {
  // preserve the prior contract when settings are absent
  if (settings === null) {
    return true;
  }

  // preserve qualified humidity unless every switch is off
  if (metric === "relativeHumidityPercent") {
    return settings.temperature || settings.wind || settings.rain;
  }

  return metric === "windDirectionDegrees" || metric === "windGustMps" || metric === "windSpeedMps"
    ? settings.wind
    : settings.temperature;
}

// remove disabled metrics from one validated mixed decision
function projectForecastAdjustmentDecision(
  decision: ForecastAdjustmentDecision | undefined,
  settings: ForecastAdjustmentSettings | null,
): ForecastAdjustmentDecision | undefined {
  // retain existing behavior without a settings contract
  if (decision === undefined || settings === null) {
    return decision;
  }

  // hide a disabled or valueless group entirely
  if (decision.state !== "active") {
    return settings.temperature || settings.wind ? decision : undefined;
  }

  const appliedMetrics = decision.appliedMetrics.filter(
    // retain only allowed adjusted metrics
    (metric) => forecastAdjustmentMetricEnabled(metric, settings),
  );

  // remove a fully disabled active decision
  if (appliedMetrics.length === 0) {
    return undefined;
  }

  const adjustedMetrics: Partial<Record<ForecastAdjustmentMetric, number>> = {};

  // copy only values selected for this view
  for (const metric of appliedMetrics) {
    adjustedMetrics[metric] = decision.adjustedMetrics[metric]!;
  }

  return { ...decision, appliedMetrics, adjustedMetrics };
}

// validate the rain model's bounded source summary
function parseForecastRainRuntimeSource(
  value: unknown,
): ForecastRainAdjustmentRuntimeStatus["source"] | undefined {
  // preserve a disabled or cold source
  if (value === null) {
    return null;
  }

  const source = forecastAdjustmentObject(value);

  // reject incomplete source clocks or counts
  if (
    source === null ||
    !hasExactForecastAdjustmentKeys(source, FORECAST_RAIN_RUNTIME_SOURCE_KEYS) ||
    !isForecastAdjustmentInstant(source.runInitializedAt) ||
    !isForecastAdjustmentInstant(source.firstReceivedAt) ||
    !isForecastAdjustmentInstant(source.decisionAt) ||
    !Number.isSafeInteger(source.hourCount) ||
    (source.hourCount as number) < 0 ||
    (source.hourCount as number) > 23
  ) {
    return undefined;
  }

  return source as unknown as ForecastRainAdjustmentRuntimeStatus["source"];
}

// validate one independent rain runtime
function parseForecastRainAdjustmentRuntime(value: unknown): ForecastRainAdjustmentRuntimeStatus | null {
  const runtime = forecastAdjustmentObject(value);

  // require one complete runtime identity
  if (
    runtime === null ||
    !hasExactForecastAdjustmentKeys(runtime, FORECAST_RAIN_RUNTIME_KEYS) ||
    (runtime.loadedAt !== null && !isForecastAdjustmentInstant(runtime.loadedAt)) ||
    parseForecastRainRuntimeSource(runtime.source) === undefined
  ) {
    return null;
  }

  // accept a bound active bundle
  if (
    runtime.state === "active" &&
    isForecastAdjustmentSha256(runtime.activeBundle) &&
    runtime.reasonCode === null &&
    isForecastAdjustmentInstant(runtime.loadedAt)
  ) {
    return runtime as unknown as ForecastRainAdjustmentRuntimeStatus;
  }

  // accept an explicit disabled boundary
  if (
    runtime.state === "disabled" &&
    runtime.activeBundle === null &&
    isBoundedForecastAdjustmentText(runtime.reasonCode)
  ) {
    return runtime as unknown as ForecastRainAdjustmentRuntimeStatus;
  }

  return null;
}

// validate one causal ECMWF rain source
function parseForecastRainSource(
  value: unknown,
  record: WeatherRecord,
): ForecastRainAdjustmentSource | null {
  const source = forecastAdjustmentObject(value);

  // bind the source to its displayed forecast hour
  if (
    source === null ||
    !hasExactForecastAdjustmentKeys(source, FORECAST_RAIN_SOURCE_KEYS) ||
    !isForecastAdjustmentInstant(source.runInitializedAt) ||
    !isForecastAdjustmentInstant(source.firstReceivedAt) ||
    !isForecastAdjustmentInstant(source.validAt) ||
    !isForecastAdjustmentInstant(source.decisionAt) ||
    source.validAt !== record.validAt ||
    source.upstreamModel !== "ecmwf_ifs" ||
    source.providerKey !== "open-meteo" ||
    !Number.isSafeInteger(source.modelLeadHours) ||
    (source.modelLeadHours as number) < 9 ||
    (source.modelLeadHours as number) > 31 ||
    typeof source.rawPrecipitationMm !== "number" ||
    !Number.isFinite(source.rawPrecipitationMm) ||
    source.rawPrecipitationMm < 0 ||
    source.rawPrecipitationMm > 2000 ||
    Date.parse(source.validAt as string) - Date.parse(source.runInitializedAt as string) !==
      (source.modelLeadHours as number) * 3_600_000 ||
    Date.parse(source.firstReceivedAt as string) > Date.parse(source.decisionAt as string) ||
    Date.parse(source.decisionAt as string) - Date.parse(source.runInitializedAt as string) !== 8 * 3_600_000 ||
    Date.parse(source.decisionAt as string) > Date.parse(source.validAt as string)
  ) {
    return null;
  }

  return source as unknown as ForecastRainAdjustmentSource;
}

// validate one rain amount without rewriting raw metrics
function parseForecastRainDecision(
  value: unknown,
  record: WeatherRecord,
  runtime: ForecastRainAdjustmentRuntimeStatus,
): ForecastRainAdjustmentDecision | null {
  const decision = forecastAdjustmentObject(value);

  // require the exact decision and raw Best Match amount
  if (
    decision === null ||
    !hasExactForecastAdjustmentKeys(decision, FORECAST_RAIN_DECISION_KEYS) ||
    decision.contractVersion !== "forecast-rain-adjustment-decision/v1" ||
    decision.rawBestMatchPrecipitationMm !== record.metrics.precipitationMm
  ) {
    return null;
  }

  // accept only a finite, bound active correction
  if (decision.state === "active") {
    const source = parseForecastRainSource(decision.sourceForecast, record);

    // reject orphaned or impossible amounts
    if (
      runtime.state !== "active" ||
      source === null ||
      decision.bundleSha256 !== runtime.activeBundle ||
      decision.reasonCode !== null ||
      typeof decision.correctedPrecipitationMm !== "number" ||
      !Number.isFinite(decision.correctedPrecipitationMm) ||
      decision.correctedPrecipitationMm < 0 ||
      decision.correctedPrecipitationMm > 30
    ) {
      return null;
    }

    return { ...decision, sourceForecast: source } as ForecastRainAdjustmentDecision;
  }

  // require an explicit raw fallback state
  if (
    (decision.state !== "disabled" && decision.state !== "raw_fallback") ||
    !isBoundedForecastAdjustmentText(decision.reasonCode) ||
    decision.correctedPrecipitationMm !== null ||
    (decision.state === "disabled" &&
      (runtime.state !== "disabled" || decision.bundleSha256 !== null || decision.sourceForecast !== null)) ||
    (decision.state === "raw_fallback" &&
      (runtime.state !== "active" ||
        decision.bundleSha256 !== runtime.activeBundle ||
        (decision.sourceForecast !== null &&
          parseForecastRainSource(decision.sourceForecast, record) === null)))
  ) {
    return null;
  }

  return decision as unknown as ForecastRainAdjustmentDecision;
}

// remove untrusted adjustment metadata from one raw record
function rawForecastRecord(value: unknown): WeatherRecord | null {
  const record = forecastAdjustmentObject(value);

  // require one complete renderable base record
  if (record === null || !isRawWeatherRecord(record)) {
    return null;
  }

  const {
    adjustment: _adjustment,
    rainAdjustment: _rainAdjustment,
    temperatureAdjustment: _temperatureAdjustment,
    ...raw
  } = record;
  return raw as unknown as WeatherRecord;
}

// accept one complete retained pressure window without mixing forecast vintages
function parseForecastPressureContext(value: unknown): readonly WeatherRecord[] {
  // keep older responses and incomplete context harmless
  if (!Array.isArray(value) || value.length !== 6) {
    return [];
  }

  const records = value.map(rawForecastRecord);
  // reject malformed or non-forecast context independently of the visible forecast
  if (records.some(
    // require finite pressure at every hourly endpoint
    (record) => record === null || record.provenance.sourceKind !== "forecast" ||
      record.productRunAt === null || record.metrics.pressureHpa === null ||
      !Number.isFinite(record.metrics.pressureHpa),
  )) {
    return [];
  }

  const hours = (records as WeatherRecord[]).toSorted(
    // normalize the six-hour context order
    (left, right) => Date.parse(left.validAt) - Date.parse(right.validAt),
  );
  const first = hours[0]!;
  return hours.every(
    // retain one exact contiguous run from one source
    (record, index) => record.provenance.sourceId === first.provenance.sourceId &&
      record.productRunAt === first.productRunAt &&
      Date.parse(record.validAt) === Date.parse(first.validAt) + index * 3_600_000,
  ) ? hours : [];
}

// parse one forecast response with a global fail-raw boundary
export function parseForecastRecordsResponse(value: unknown): ForecastRecordsResponse {
  const response = forecastAdjustmentObject(value);

  // retain ordinary response failures outside adjustment fallback
  if (
    response === null ||
    !Array.isArray(response.data) ||
    forecastAdjustmentObject(response.site) === null
  ) {
    throw new RangeError("Forecast response is invalid");
  }

  const responseData = response.data;
  const rawRecords = responseData.map(rawForecastRecord);

  // reject malformed raw rows normally
  if (rawRecords.some((record) => record === null)) {
    throw new RangeError("Forecast response contains an invalid raw record");
  }

  const records = rawRecords as WeatherRecord[];
  const runtime = parseForecastAdjustmentRuntime(response.adjustmentRuntime);
  let invalidDecision = false;
  const parsedRecords = runtime === null ? records : records.map((record, index) => {
    const source = forecastAdjustmentObject(responseData[index]);
    const decision = parseForecastAdjustmentDecision(source?.adjustment, record, runtime);

    // reject any missing decision or active row under a disabled runtime
    if (decision === null || (runtime.state === "disabled" && decision.state === "active")) {
      invalidDecision = true;
      return record;
    }

    return { ...record, adjustment: decision };
  });
  const effectiveRuntime = runtime === null || invalidDecision
    ? invalidForecastAdjustmentRuntime()
    : runtime;
  const effectiveRecords = runtime === null || invalidDecision
    ? records
    : parsedRecords;
  const temperatureRuntime = parseForecastTemperatureAdjustmentRuntime(
    response.temperatureAdjustmentRuntime,
  );
  let invalidTemperatureDecision = false;
  const temperatureRecords = temperatureRuntime === null
    ? effectiveRecords
    : effectiveRecords.map((record, index) => {
        const source = forecastAdjustmentObject(responseData[index]);
        const decision = parseForecastTemperatureDecision(
          source?.temperatureAdjustment,
          record,
          temperatureRuntime,
        );

        // reject partial or runtime-inconsistent temperature activation
        if (decision === null) {
          invalidTemperatureDecision = true;
          return record;
        }

        return { ...record, temperatureAdjustment: decision };
      });
  const effectiveTemperatureRuntime =
    temperatureRuntime === null || invalidTemperatureDecision
      ? invalidForecastTemperatureAdjustmentRuntime()
      : temperatureRuntime;
  const rainRuntime = parseForecastRainAdjustmentRuntime(response.rainAdjustmentRuntime);
  const rainBaseRecords = invalidTemperatureDecision ? effectiveRecords : temperatureRecords;
  let invalidRainDecision = false;
  const rainRecords = rainRuntime === null
    ? rainBaseRecords
    : rainBaseRecords.map((record, index) => {
        const source = forecastAdjustmentObject(responseData[index]);
        const decision = parseForecastRainDecision(source?.rainAdjustment, record, rainRuntime);

        // reject any incomplete rain decision set
        if (decision === null) {
          invalidRainDecision = true;
          return record;
        }

        return { ...record, rainAdjustment: decision };
      });
  const effectiveRainRuntime = rainRuntime === null || invalidRainDecision
    ? invalidForecastRainAdjustmentRuntime()
    : rainRuntime;
  const settings = parseForecastAdjustmentSettings(response.adjustmentSettings) ??
    (Object.hasOwn(response, "adjustmentSettings")
      ? DISABLED_FORECAST_ADJUSTMENT_SETTINGS
      : null);
  const safeRecords = (invalidRainDecision ? rainBaseRecords : rainRecords).map((record) => {
    // apply independent server switches at the browser boundary
    if (settings === null) {
      return record;
    }

    const {
      adjustment,
      rainAdjustment,
      temperatureAdjustment,
      ...raw
    } = record;
    const selectedAdjustment = projectForecastAdjustmentDecision(adjustment, settings);
    return {
      ...raw,
      ...selectedAdjustment === undefined ? {} : { adjustment: selectedAdjustment },
      ...settings.rain && rainAdjustment !== undefined ? { rainAdjustment } : {},
      ...settings.temperature && temperatureAdjustment !== undefined ? { temperatureAdjustment } : {},
    };
  });

  return {
    ...(response as unknown as RecordsResponse),
    adjustmentSettings: settings,
    adjustmentRuntime: effectiveRuntime,
    data: safeRecords,
    pressureContext: parseForecastPressureContext(response.pressureContext),
    rainAdjustmentRuntime: effectiveRainRuntime,
    temperatureAdjustmentRuntime: effectiveTemperatureRuntime,
  };
}

// coordinate browser reads and pagination
export class WeatherDashboardController {
  readonly #apiBaseUrl: string;
  readonly #cursors: Array<string | undefined> = [undefined];
  readonly #fetcher: typeof fetch;
  readonly #isAdmin: boolean;
  readonly #listeners = new Set<DashboardListener>();
  readonly #storage: UnitPreferenceStorage | null;
  #homeNetworkGeneration = 0;
  #homeNetworkLayoutRequest: Promise<void> | null = null;
  #homeNetworkRefresh: Promise<void> | null = null;
  #currentRefresh: Promise<void> | null = null;
  #currentGeneration = 0;
  #lastForecastRefreshAt = 0;
  #lastForecastObservationRefreshAt = 0;
  #lastSolarCloudEvidenceRefreshAt = 0;
  #solarCloudEvidenceRequest: Promise<SolarCloudEvidence | null> | null = null;
  #forecastObservationRequest: Promise<readonly ForecastObservationPoint[]> | null = null;
  #view: WeatherView;
  #state: DashboardState;

  // retain injectable browser boundaries
  constructor(options: DashboardOptions = {}) {
    this.#apiBaseUrl = normalizeBaseUrl(options.apiBaseUrl ?? "/api/v1");
    this.#fetcher = options.fetcher ?? fetch;
    this.#isAdmin = options.isAdmin ?? (options.view === "admin");
    this.#storage = options.storage === undefined
      ? browserUnitPreferenceStorage()
      : options.storage;
    this.#view = options.view ?? "home";
    this.#state = {
      ...EMPTY_STATE,
      cachedNowIcon: loadNowIconCache(this.#storage),
      forecastAdjustmentMode: loadForecastAdjustmentMode(this.#storage),
      loading: true,
      selectedSite: PRODUCT_SITE,
      sites: [PRODUCT_SITE],
      units: loadUnitPreferences(this.#storage),
    };
  }

  // expose the latest immutable view
  get state(): DashboardState {
    return this.#state;
  }

  // expose the active browser route to the renderer
  get view(): WeatherView {
    return this.#view;
  }

  // expose the server-authenticated display boundary
  get isAdmin(): boolean {
    return this.#isAdmin;
  }

  // notify one dashboard view
  subscribe(listener: DashboardListener): () => void {
    this.#listeners.add(listener);
    listener(this.#state);

    // remove the listener safely
    return () => {
      this.#listeners.delete(listener);
    };
  }

  // load the fixed Ballydidean view directly
  async initialize(): Promise<void> {
    await this.loadSelectedSite();
  }

  // apply new history filters
  async setFilters(filters: HistoryFilters): Promise<void> {
    this.resetPagination();
    this.#state = { ...this.#state, filters };
    this.emit();
    await this.loadSelectedSite();
  }

  // report a rejected site time
  reportInvalidHistoryWallClock(): void {
    this.patch({
      error: INVALID_HISTORY_WALL_CLOCK_MESSAGE,
      loading: false,
    });
  }

  // apply and persist one complete unit preference record
  setUnitPreferences(value: unknown): void {
    const units = normalizeUnitPreferences(value);
    persistUnitPreferences(this.#storage, units);
    this.patch({ units });
  }

  // switch and persist the current and forecast display source
  toggleForecastAdjustmentMode(): void {
    const forecastAdjustmentMode: ForecastAdjustmentMode = this.#state.forecastAdjustmentMode === "raw"
      ? "adjusted"
      : "raw";

    persistForecastAdjustmentMode(this.#storage, forecastAdjustmentMode);
    const inputs = currentWeatherIconInputs(this.#state.current, forecastAdjustmentMode, false, this.#state.forecast);
    const cachedNowIcon = inputs === null ? null : { ...inputs, cachedAt: Date.now(), forecastAdjustmentMode };
    persistNowIconCache(this.#storage, cachedNowIcon);
    this.patch({ cachedNowIcon, forecastAdjustmentMode });
  }

  // load the newly selected public application route
  async setView(view: WeatherView): Promise<void> {
    // preserve the current page without duplicate reads
    if (view === this.#view) {
      return;
    }

    // invalidate any in-flight homepage location check
    this.#homeNetworkGeneration += 1;
    this.#homeNetworkLayoutRequest = null;
    this.#homeNetworkRefresh = null;
    this.#currentGeneration += 1;
    this.#view = view;
    this.patch({ homeNetwork: false, propertySensorLayoutLoading: false });
    await this.loadSelectedSite();
  }

  // re-evaluate expiry and refresh only current readings without resetting forecast selection
  async refreshCurrentReadings(): Promise<void> {
    const site = this.#state.selectedSite;
    // keep weather polling off unrelated routes and full-page loads
    if (site === null || (this.#view !== "home" && this.#view !== "forecast") || this.#state.loading) {
      return;
    }
    this.emit();
    // avoid overlapping current requests after timer and resume events
    if (this.#currentRefresh !== null) {
      return this.#currentRefresh;
    }
    const generation = this.#currentGeneration;
    const refreshForecast = this.#state.forecastAdjustmentMode === "raw" &&
      Date.now() - this.#lastForecastRefreshAt >= REGIONAL_FORECAST_REFRESH_MS;
    const refreshObservations = this.#view === "forecast" &&
      Date.now() - this.#lastForecastObservationRefreshAt >= 5 * 60_000;
    // throttle failed history attempts without repeatedly scanning the archive
    if (refreshObservations) {
      void this.refreshForecastObservations(generation);
    }
    // share a bounded optional daylight read on both weather routes independent of toggles
    if (Date.now() - this.#lastSolarCloudEvidenceRefreshAt >= 5 * 60_000) {
      void this.refreshSolarCloudEvidence(generation);
    }
    // throttle failed forecast attempts as well as successful reads
    if (refreshForecast) {
      this.#lastForecastRefreshAt = Date.now();
    }
    // isolate failures from the retained forecast and let timestamp gates retire old estimates
    const refresh = (async (): Promise<void> => {
      try {
        const [response, forecast] = await Promise.all([
          // retain last-good current readings independently of forecast refresh failures
          getJson<RecordsResponse>(this.#fetcher, buildCurrentUrl(this.#apiBaseUrl, site.slug, {})).catch(() => null),
          refreshForecast
            ? getForecastJson(this.#fetcher, buildForecastUrl(this.#apiBaseUrl, site.slug, this.#state.forecastDays)).then(
                // reject incompatible sites before using the new regional run
                (result) => { requireProductSite(result.site); return result; },
              ).catch(() => null)
            : Promise.resolve(null),
        ]);
        // ignore late responses from former routes sites or full-weather loads
        if (generation !== this.#currentGeneration || this.#state.selectedSite?.slug !== site.slug || this.#state.loading) {
          return;
        }
        // re-evaluate expiry without renewing the cache after a complete read failure
        if (response === null && forecast === null) {
          this.emit();
          return;
        }
        const responseSite = response === null ? site : requireProductSite(response.site);
        const current = response?.data ?? this.#state.current;
        const forecastRecords = forecast?.data ?? this.#state.forecast;
        const forecastAdjustmentMode = this.#state.forecastAdjustmentMode;
        const inputs = currentWeatherIconInputs(current, forecastAdjustmentMode, false, forecastRecords);
        const cachedNowIcon = inputs === null ? null : { ...inputs, cachedAt: Date.now(), forecastAdjustmentMode };
        persistNowIconCache(this.#storage, cachedNowIcon);
        this.patch({
          current,
          cachedNowIcon,
          selectedSite: responseSite,
          forecast: forecastRecords,
          forecastPressureContext: forecast?.pressureContext ?? this.#state.forecastPressureContext,
          forecastAdjustmentSettings: forecast?.adjustmentSettings ?? this.#state.forecastAdjustmentSettings,
          forecastAdjustmentRuntime: forecast?.adjustmentRuntime ?? this.#state.forecastAdjustmentRuntime,
          forecastRainAdjustmentRuntime: forecast?.rainAdjustmentRuntime ?? this.#state.forecastRainAdjustmentRuntime,
          forecastTemperatureAdjustmentRuntime: forecast?.temperatureAdjustmentRuntime ?? this.#state.forecastTemperatureAdjustmentRuntime,
        });
      } catch {
        // preserve last-good raw readings while re-rendering to enforce freshness
        if (generation === this.#currentGeneration) {
          this.emit();
        }
      }
    })();
    this.#currentRefresh = refresh;
    try {
      await refresh;
    } finally {
      this.#currentRefresh = null;
    }
  }

  // recover the last genuine daylight pair without blocking required weather products
  private async refreshSolarCloudEvidence(generation: number): Promise<void> {
    const site = this.#state.selectedSite;
    // optional evidence belongs only to the fixed home and forecast product
    if (site === null || (this.#view !== "home" && this.#view !== "forecast")) {
      return;
    }
    // route and horizon changes may adopt an existing request without adding archive load
    if (this.#solarCloudEvidenceRequest === null && Date.now() - this.#lastSolarCloudEvidenceRefreshAt < 5 * 60_000) {
      return;
    }
    this.#lastSolarCloudEvidenceRefreshAt = Date.now();
    const request = this.#solarCloudEvidenceRequest ?? getSolarCloudEvidence(this.#fetcher, this.#apiBaseUrl, site);
    this.#solarCloudEvidenceRequest = request;
    try {
      const evidence = await request;
      // a late response cannot overwrite a newer route generation or a different site
      if (generation === this.#currentGeneration && this.#state.selectedSite?.slug === site.slug &&
        (this.#view === "home" || this.#view === "forecast")) {
        this.#lastSolarCloudEvidenceRefreshAt = Date.now();
        this.patch({ solarCloudEvidence: evidence });
      } else if (this.#solarCloudEvidenceRequest === request) {
        // discarded success permits immediate cache-backed recovery on a returning weather route
        this.#lastSolarCloudEvidenceRefreshAt = 0;
      }
    } catch {
      // retain only genuine last-good evidence whose original expiry is checked on every render
    } finally {
      // only the shared request owner clears its in-flight identity
      if (this.#solarCloudEvidenceRequest === request) {
        this.#solarCloudEvidenceRequest = null;
      }
    }
  }

  // refresh optional recordings without blocking current conditions or future forecasts
  private async refreshForecastObservations(generation: number): Promise<void> {
    const site = this.#state.selectedSite;
    const days = this.#state.forecastDays;
    // keep archive reads off other routes and incomplete product initialization
    if (site === null || this.#view !== "forecast") {
      return;
    }
    this.#lastForecastObservationRefreshAt = Date.now();
    const request = this.#forecastObservationRequest ?? getForecastObservations(this.#fetcher, this.#apiBaseUrl, site.slug);
    this.#forecastObservationRequest = request;
    this.patch({ forecastObservationsLoading: true });
    // fence both success and failure against later routes and forecast horizons
    const current = (): boolean => generation === this.#currentGeneration && this.#view === "forecast" &&
      this.#state.selectedSite?.slug === site.slug && this.#state.forecastDays === days;
    try {
      const observations = await request;
      // late optional data must not commit an obsolete route or day
      if (current()) {
        this.patch({ forecastObservations: observations, forecastObservationsReady: true, forecastObservationsLoading: false });
      }
    } catch {
      // retain genuine last-good recordings while exposing the failed refresh honestly
      if (current()) {
        this.patch({ forecastObservationsReady: false, forecastObservationsLoading: false });
      }
    } finally {
      // only the owner of the shared request clears its in-flight identity
      if (this.#forecastObservationRequest === request) {
        this.#forecastObservationRequest = null;
      }
    }
  }

  // recheck one ephemeral homepage display boundary
  async refreshHomeNetwork(force = false): Promise<void> {
    // keep local-network display separate from administrator authority
    if (this.#view !== "home" || this.#isAdmin) {
      return;
    }

    // avoid overlapping timer and resume checks
    if (!force && this.#homeNetworkRefresh !== null) {
      return this.#homeNetworkRefresh;
    }

    // preserve an authorized layout read through a positive recheck
    const generation = force || !this.#state.homeNetwork
      ? ++this.#homeNetworkGeneration
      : this.#homeNetworkGeneration;

    // abandon layout from a former full-weather refresh
    if (force) {
      this.#homeNetworkLayoutRequest = null;
    }

    // clear access only for an explicit full-weather refresh
    if (force && this.#state.homeNetwork) {
      this.patch({ homeNetwork: false, propertySensorLayoutLoading: false });
    }

    // resolve only the current location check
    const refresh = (async (): Promise<void> => {
      const allowed = await getHomeNetworkViewerContext(this.#fetcher, this.#apiBaseUrl);

      // ignore a response from a former route or check
      if (generation !== this.#homeNetworkGeneration || this.#view !== "home") {
        return;
      }

      // leave former local-network panels hidden on denial or failure
      if (!allowed) {
        // reject any outstanding layout from the former positive claim
        this.#homeNetworkGeneration += 1;
        this.#homeNetworkLayoutRequest = null;

        // revoke a visible panel only after a negative result
        if (this.#state.homeNetwork) {
          this.patch({ homeNetwork: false, propertySensorLayoutLoading: false });
        }

        return;
      }

      // keep a positive periodic recheck from redrawing the homepage
      if (this.#state.homeNetwork) {
        // retry only a missing layout without duplicating an active read
        if (this.#state.propertySensorLayout === null) {
          this.loadHomeNetworkLayout(generation);
        }

        return;
      }

      this.patch({ homeNetwork: true, propertySensorLayout: null });
      this.loadHomeNetworkLayout(generation);
    })();
    this.#homeNetworkRefresh = refresh;

    try {
      await refresh;
    } finally {
      // release only the completed check
      if (this.#homeNetworkRefresh === refresh) {
        this.#homeNetworkRefresh = null;
      }
    }
  }

  // load one optional layout without duplicating an active read
  private loadHomeNetworkLayout(generation: number): void {
    // avoid duplicate reads during a positive recheck
    if (this.#homeNetworkLayoutRequest !== null) {
      return;
    }

    this.patch({ propertySensorLayoutLoading: true });
    const request = getJson<PropertySensorLayoutResponse>(
      this.#fetcher,
      buildPropertySensorLayoutUrl(this.#apiBaseUrl, PRODUCT_SITE.slug),
    ).then(
      // publish only a valid layout for the current grant
      (layout) => {
        // reject malformed or revoked layout responses
        if (
          Array.isArray(layout.data) &&
          generation === this.#homeNetworkGeneration &&
          this.#view === "home" &&
          this.#state.homeNetwork
        ) {
          this.patch({ propertySensorLayout: layout.data });
        }
      },
    ).catch(
      // leave the map's unavailable message visible on failure
      () => undefined,
    );
    this.#homeNetworkLayoutRequest = request;
    void request.finally(
      // release only the completed layout read
      () => {
        // preserve any newer layout request
        if (this.#homeNetworkLayoutRequest === request) {
          this.#homeNetworkLayoutRequest = null;
          this.patch({ propertySensorLayoutLoading: false });
        }
      },
    );
  }

  // switch and reload the forecast horizon
  async setForecastDays(days: ForecastDays): Promise<void> {
    const site = this.#state.selectedSite;

    // wait for initialization
    if (site === null || days === this.#state.forecastDays) {
      return;
    }

    const previousDays = this.#state.forecastDays;
    // fence background reads from the former forecast range
    this.#currentGeneration += 1;
    this.patch({ error: null, forecastDays: days, loading: true });
    // adopt a pending daylight read without refetching solely for a horizon change
    if (this.#solarCloudEvidenceRequest !== null) {
      void this.refreshSolarCloudEvidence(this.#currentGeneration);
    }
    // adopt the same pending archive read without issuing another horizon-dependent request
    if (this.#forecastObservationRequest !== null) {
      void this.refreshForecastObservations(this.#currentGeneration);
    }

    try {
      const response = await getForecastJson(
        this.#fetcher,
        buildForecastUrl(this.#apiBaseUrl, site.slug, days),
      );
      const responseSite = requireProductSite(response.site);
      this.#lastForecastRefreshAt = Date.now();
      this.patch({
        error: null,
        forecast: response.data,
        forecastPressureContext: response.pressureContext,
        forecastAdjustmentMode: this.#state.forecastAdjustmentMode,
        forecastAdjustmentSettings: response.adjustmentSettings,
        forecastAdjustmentRuntime: response.adjustmentRuntime,
        forecastRainAdjustmentRuntime: response.rainAdjustmentRuntime,
        forecastTemperatureAdjustmentRuntime:
          response.temperatureAdjustmentRuntime,
        loading: false,
        selectedSite: responseSite,
        sites: [responseSite],
      });
    } catch (error) {
      this.#state = { ...this.#state, forecastDays: previousDays };
      this.fail(error);
    }
  }

  // switch the dependency-free station base map
  setMapLayer(layer: MapLayer): void {
    this.patch({ mapLayer: layer });
  }

  // switch the bounded property base map
  setPropertyMapLayer(layer: MapLayer): void {
    this.patch({ propertyMapLayer: layer });
  }

  // switch the visible annual trend measurement
  setSelectedTrendMetric(metric: TrendChartMetric): void {
    this.patch({ selectedTrendMetric: metric, selectedTrendYear: null });
  }

  // switch the extreme-day measurement and its safe default threshold
  setTrendExtremeKind(kind: TrendExtremeKind): void {
    this.patch({
      selectedTrendYear: null,
      trendExtremeKind: kind,
      trendExtremeThreshold: trendExtremeConfiguration(kind).defaultThreshold,
    });
  }

  // update one bounded canonical extreme-day threshold
  setTrendExtremeThreshold(threshold: number): void {
    const configuration = trendExtremeConfiguration(this.#state.trendExtremeKind);

    // reject malformed or unsafe chart boundaries
    if (
      !Number.isFinite(threshold) ||
      threshold < configuration.minimum ||
      threshold > configuration.maximum
    ) {
      return;
    }

    this.patch({ selectedTrendYear: null, trendExtremeThreshold: threshold });
  }

  // switch between the aggregate and individual years
  toggleTrendDisplayMode(): void {
    const trendDisplayMode = this.#state.trendDisplayMode === "aggregate" ? "all" : "aggregate";
    this.patch({ selectedTrendYear: null, trendDisplayMode });
  }

  // switch between a rolling overview and daily detail
  toggleTrendDetail(): void {
    const trendDetail = this.#state.trendDetail === "rolling" ? "daily" : "rolling";
    this.patch({ trendDetail });
  }

  // emphasize one annual trend line
  setSelectedTrendYear(year: number): void {
    this.patch({ selectedTrendYear: this.#state.selectedTrendYear === year ? null : year });
  }

  // select one physical station for compact conditions
  setSelectedStation(stationSlug: string): void {
    const station = this.#state.selectedSite?.stations.find(
      // require a rendered physical weather station
      (candidate) =>
        candidate.slug === stationSlug &&
        candidate.kind === "physical" &&
        candidate.sources.some((source) => source.kind === "physical_sensor"),
    );

    // reject stale or synthetic rendered values
    if (station === undefined) {
      return;
    }

    this.patch({ selectedStationSlug: station.slug });
  }

  // select one EcoWitt property sensor for editing
  setSelectedPropertySensor(sensorKey: string): void {
    const sensor = propertySensorSnapshots(this.#state).find(
      // require one currently reported sensor key
      (candidate) => candidate.key === sensorKey,
    );

    // reject stale rendered sensor identities
    if (sensor === undefined) {
      return;
    }

    this.patch({ selectedPropertySensorKey: sensor.key });
  }

  // persist one shared property sensor name and position
  async savePropertySensorLayout(
    sensorKey: string,
    displayName: string,
    icon: PropertySensorIcon,
    latitude: number,
    longitude: number,
  ): Promise<void> {
    this.patch({ error: null, loading: true });

    try {
      const response = await putJson<{ readonly data: PropertySensorLayout }>(
        this.#fetcher,
        buildAdminPropertySensorLayoutUrl(this.#apiBaseUrl, PRODUCT_SITE.slug, sensorKey),
        { displayName, icon, latitude, longitude },
      );
      const next = (this.#state.propertySensorLayout ?? []).filter(
        // replace only the saved sensor key
        (entry) => entry.sensorKey !== response.data.sensorKey,
      );
      next.push(response.data);
      this.patch({
        loading: false,
        propertySensorLayout: next.sort(
          // retain deterministic editor order
          (left, right) => left.sensorKey.localeCompare(right.sensorKey),
        ),
        selectedPropertySensorKey: response.data.sensorKey,
      });
    } catch (error) {
      this.fail(error);
    }
  }

  // persist the independent forecast switches
  async saveForecastAdjustmentSettings(value: ForecastAdjustmentSettings): Promise<void> {
    // reject non-admin or concurrent updates
    if (!this.#isAdmin || this.#state.adminAdjustmentSettingsSaving) {
      return;
    }

    const settings = parseForecastAdjustmentSettings(value);

    // reject a partial local form
    if (settings === null) {
      this.patch({ adminAdjustmentSettingsMessage: "Invalid forecast adjustment settings." });
      return;
    }

    this.patch({ adminAdjustmentSettingsSaving: true, adminAdjustmentSettingsMessage: null });

    try {
      const url = buildAdminForecastAdjustmentSettingsUrl(this.#apiBaseUrl, PRODUCT_SITE.slug);
      const written = await putJson<unknown>(this.#fetcher, url, { ...settings });
      const writeSettings = parseForecastAdjustmentSettings(forecastAdjustmentObject(written)?.data);
      const read = await getJson<unknown>(this.#fetcher, url);
      const readSettings = parseForecastAdjustmentSettings(forecastAdjustmentObject(read)?.data);

      // require server acknowledgment and persisted readback
      if (
        writeSettings === null ||
        readSettings === null ||
        writeSettings.temperature !== settings.temperature ||
        writeSettings.wind !== settings.wind ||
        writeSettings.rain !== settings.rain ||
        readSettings.temperature !== settings.temperature ||
        readSettings.wind !== settings.wind ||
        readSettings.rain !== settings.rain
      ) {
        throw new Error("Forecast adjustment settings did not persist.");
      }

      this.patch({
        adminAdjustmentSettingsSaving: false,
        adminAdjustmentSettingsMessage: "Forecast adjustments saved.",
        forecastAdjustmentSettings: readSettings,
      });
    } catch (error) {
      this.patch({
        adminAdjustmentSettingsSaving: false,
        adminAdjustmentSettingsMessage: error instanceof Error ? error.message : "Forecast adjustment settings could not be saved.",
      });
    }
  }

  // advance to the next cursor page
  async nextPage(): Promise<void> {
    // stop at the final page
    if (this.#state.nextCursor === null || this.#state.loading) {
      return;
    }

    const previousPage = this.#state.page;
    this.#cursors.push(this.#state.nextCursor);
    this.#state = { ...this.#state, page: previousPage + 1 };
    this.emit();
    await this.loadHistory();

    // restore last-good pagination after failure
    if (this.#state.error !== null) {
      this.#cursors.pop();
      this.#state = { ...this.#state, page: previousPage };
      this.emit();
    }
  }

  // return to the preceding cursor page
  async previousPage(): Promise<void> {
    // stop at the first page
    if (this.#state.page === 0 || this.#state.loading) {
      return;
    }

    const previousPage = this.#state.page;
    const removedCursor = this.#cursors.pop();
    this.#state = { ...this.#state, page: previousPage - 1 };
    this.emit();
    await this.loadHistory();

    // restore last-good pagination after failure
    if (this.#state.error !== null) {
      this.#cursors.push(removedCursor);
      this.#state = { ...this.#state, page: previousPage };
      this.emit();
    }
  }

  // load only the active page data
  async loadSelectedSite(): Promise<void> {
    const site = this.#state.selectedSite;

    // wait for initialization
    if (site === null) {
      return;
    }

    // keep weather reads off the local-only settings page
    if (this.#view === "settings") {
      this.patch({ loading: false });
      return;
    }

    // keep historical reads off the homepage
    if (this.#view === "logs") {
      await this.loadHistory();
      return;
    }

    await this.loadCurrent();
  }

  // load only the current conditions panel
  async loadCurrent(): Promise<void> {
    const site = this.#state.selectedSite;
    let pendingCurrent: Promise<RecordsResponse> | null = null;

    // wait for initialization
    if (site === null) {
      return;
    }

    const generation = ++this.#currentGeneration;

    this.patch({
      error: null,
      homeNetwork: false,
      loading: true,
      propertySensorLayoutLoading: false,
      adminAdjustmentScorecardState: this.#view === "admin"
        ? "loading"
        : this.#state.adminAdjustmentScorecardState,
      adminAdjustmentScorecardPublicationState: this.#view === "admin"
        ? null
        : this.#state.adminAdjustmentScorecardPublicationState,
    });

    // daylight recovery never delays required current or future weather reads
    if (this.#view === "home" || this.#view === "forecast") {
      void this.refreshSolarCloudEvidence(generation);
    }
    // optional archive work cannot hold the required weather products in loading state
    if (this.#view === "forecast") {
      void this.refreshForecastObservations(generation);
    }

    // start the private location check without delaying weather rendering
    if (this.#view === "home" && !this.#isAdmin) {
      void this.refreshHomeNetwork(true);
    }

    try {
      const needsCurrent = this.#view === "home" || this.#view === "map" || this.#view === "forecast" || this.#view === "admin";
      const needsDailyPrecipitation = this.#view === "home";
      const needsForecast = this.#view === "home" || this.#view === "forecast";
      const needsTides = this.#view === "home" || this.#view === "forecast";
      const needsTrends = this.#view === "trends";
      const needsPropertySensorLayout =
        this.#view === "map" ||
        this.#view === "admin" ||
        (this.#view === "home" && this.#isAdmin);
      // isolate current products from history filters and sibling read failures
      pendingCurrent = needsCurrent
        ? getJson<RecordsResponse>(
          this.#fetcher,
          buildCurrentUrl(this.#apiBaseUrl, site.slug, {}),
        ).then((response) => {
          const responseSite = requireProductSite(response.site);
          // ignore current responses from a superseded full-weather load
          if (generation !== this.#currentGeneration) {
            return response;
          }
          const forecastAdjustmentMode = this.#state.forecastAdjustmentMode;
          const inputs = currentWeatherIconInputs(response.data, forecastAdjustmentMode, false, this.#state.forecast);
          const cachedNowIcon = inputs === null ? null : { ...inputs, cachedAt: Date.now(), forecastAdjustmentMode };
          persistNowIconCache(this.#storage, cachedNowIcon);
          this.patch({ current: response.data, cachedNowIcon, selectedSite: responseSite, sites: [responseSite] });
          return response;
        })
        : null;
      const [
        current,
        dailyPrecipitation,
        forecast,
        tides,
        trends,
        propertySensorLayout,
        adminSettings,
        adminScorecard,
      ] = await Promise.all([
        pendingCurrent,
        // load today's gauge total only on home
        needsDailyPrecipitation
          ? getJson<DailyPrecipitationResponse>(
            this.#fetcher,
            buildDailyPrecipitationUrl(this.#apiBaseUrl, site.slug),
          )
          : Promise.resolve(null),
        // load modeled hours only where rendered
        needsForecast
          ? getForecastJson(
            this.#fetcher,
            buildForecastUrl(this.#apiBaseUrl, site.slug, this.#state.forecastDays),
          )
          : Promise.resolve(null),
        // load tide curves only where rendered
        needsTides
          ? getJson<TidesResponse>(
            this.#fetcher,
            buildTidesUrl(this.#apiBaseUrl, site.slug),
          )
          : Promise.resolve(null),
        // load trend buckets only on Trends
        needsTrends
          ? getJson<TrendsResponse>(
            this.#fetcher,
            buildTrendsUrl(
              this.#apiBaseUrl,
              site.slug,
            ),
          )
          : Promise.resolve(null),
        // load shared sensor positions only where rendered
        needsPropertySensorLayout
          ? getJson<PropertySensorLayoutResponse>(
            this.#fetcher,
            buildPropertySensorLayoutUrl(this.#apiBaseUrl, site.slug),
          )
          : Promise.resolve(null),
        // read independent adjustment controls only for the protected editor
        this.#view === "admin"
          ? getJson<unknown>(
            this.#fetcher,
            buildAdminForecastAdjustmentSettingsUrl(this.#apiBaseUrl, site.slug),
          ).then((value) => parseForecastAdjustmentSettings(forecastAdjustmentObject(value)?.data))
            .catch(() => null)
          : Promise.resolve(null),
        // read the review-only scorecard independently of every weather panel
        this.#view === "admin"
          ? getAdminForecastAdjustmentScorecard(
            this.#fetcher,
            buildAdminForecastAdjustmentScorecardUrl(this.#apiBaseUrl, site.slug),
          )
          : Promise.resolve(null),
      ]);
      // an older route or horizon cannot replace a newer completed load
      if (generation !== this.#currentGeneration) {
        return;
      }
      const responseSite = requireProductSite(
        current?.site ?? dailyPrecipitation?.site ?? forecast?.site ?? tides?.site ?? trends?.site ?? site,
      );
      this.#state = {
        ...this.#state,
        dailyPrecipitation: dailyPrecipitation === null
          ? this.#state.dailyPrecipitation
          : dailyPrecipitation.data,
        error: null,
        forecast: forecast?.data ?? this.#state.forecast,
        forecastPressureContext: forecast?.pressureContext ?? this.#state.forecastPressureContext,
        forecastAdjustmentMode: this.#state.forecastAdjustmentMode,
        forecastAdjustmentSettings: adminSettings ?? forecast?.adjustmentSettings ?? this.#state.forecastAdjustmentSettings,
        forecastAdjustmentRuntime: forecast?.adjustmentRuntime ?? this.#state.forecastAdjustmentRuntime,
        forecastRainAdjustmentRuntime:
          forecast?.rainAdjustmentRuntime ?? this.#state.forecastRainAdjustmentRuntime,
        forecastTemperatureAdjustmentRuntime:
          forecast?.temperatureAdjustmentRuntime ??
          this.#state.forecastTemperatureAdjustmentRuntime,
        adminAdjustmentSettingsMessage: this.#view === "admin" && adminSettings === null
          ? "Forecast adjustment settings could not be loaded."
          : this.#state.adminAdjustmentSettingsMessage,
        // clear any prior publication after a failed protected read
        adminAdjustmentScorecard: adminScorecard === null
          ? this.#state.adminAdjustmentScorecard
          : adminScorecard.scorecard,
        adminAdjustmentScorecardPublicationState: adminScorecard === null
          ? this.#state.adminAdjustmentScorecardPublicationState
          : adminScorecard.publicationState,
        adminAdjustmentScorecardState: adminScorecard?.state ?? this.#state.adminAdjustmentScorecardState,
        loading: false,
        propertySensorLayout: propertySensorLayout?.data ?? this.#state.propertySensorLayout,
        selectedSite: responseSite,
        sites: [responseSite],
        tideGeneratedAt: tides?.generatedAt ?? this.#state.tideGeneratedAt,
        tides: tides?.data ?? this.#state.tides,
        trendGeneratedAt: trends?.generatedAt ?? this.#state.trendGeneratedAt,
        trends: trends?.data ?? this.#state.trends,
      };
      // cache the completed regional blend rather than the earlier current-only response
      if (needsForecast) {
        this.#lastForecastRefreshAt = Date.now();
        const forecastAdjustmentMode = this.#state.forecastAdjustmentMode;
        const inputs = currentWeatherIconInputs(this.#state.current, forecastAdjustmentMode, false, this.#state.forecast);
        const cachedNowIcon = inputs === null ? null : { ...inputs, cachedAt: Date.now(), forecastAdjustmentMode };
        persistNowIconCache(this.#storage, cachedNowIcon);
        this.#state = { ...this.#state, cachedNowIcon };
      }
      this.emit();
    } catch (error) {
      // keep cold artwork loading until current settles even when a sibling fails first
      await pendingCurrent?.catch(() => null);
      // ignore failures from a superseded full-weather load
      if (generation === this.#currentGeneration) {
        this.fail(error);
      }
    }
  }

  // load only the history panel
  async loadHistory(): Promise<void> {
    const site = this.#state.selectedSite;

    // wait for initialization
    if (site === null) {
      return;
    }

    this.patch({ error: null, loading: true });

    try {
      const response = await getJson<RecordsResponse>(
        this.#fetcher,
        buildHistoryUrl(
          this.#apiBaseUrl,
          site.slug,
          this.#state.filters,
          this.#cursors[this.#state.page],
        ),
      );
      const responseSite = requireProductSite(response.site);
      this.#state = {
        ...this.#state,
        error: null,
        history: response.data,
        loading: false,
        nextCursor: response.page?.nextCursor ?? null,
        selectedSite: responseSite,
        sites: [responseSite],
      };
      this.emit();
    } catch (error) {
      this.fail(error);
    }
  }

  // merge a partial state update
  private patch(update: Partial<DashboardState>): void {
    this.#state = { ...this.#state, ...update };
    this.emit();
  }

  // publish the current state
  private emit(): void {
    // notify every mounted view
    for (const listener of this.#listeners) {
      listener(this.#state);
    }
  }

  // publish a bounded error
  private fail(error: unknown): void {
    const message = error instanceof Error ? error.message : "Weather data could not be loaded";

    // revoke former anonymous homepage access after a failed weather refresh
    if (this.#view === "home" && !this.#isAdmin) {
      this.#homeNetworkGeneration += 1;
      this.#homeNetworkLayoutRequest = null;
      this.#homeNetworkRefresh = null;
      this.patch({ error: message, homeNetwork: false, loading: false, propertySensorLayoutLoading: false });
      return;
    }

    this.patch({ error: message, loading: false });
  }

  // restore the first cursor page
  private resetPagination(): void {
    this.#cursors.splice(0, this.#cursors.length, undefined);
    this.#state = { ...this.#state, nextCursor: null, page: 0 };
  }
}

// construct a filtered history endpoint
export function buildHistoryUrl(
  apiBaseUrl: string,
  siteSlug: string,
  filters: HistoryFilters,
  cursor?: string,
): string {
  const parameters = new URLSearchParams({ limit: "25" });

  // include the station filter
  if (filters.stationSlug !== undefined) {
    parameters.set("station", filters.stationSlug);
  }

  // include the source filter
  if (filters.sourceId !== undefined) {
    parameters.set("source", filters.sourceId);
  }

  // include the provenance filter
  if (filters.sourceKind !== undefined) {
    parameters.set("sourceKind", filters.sourceKind);
  }

  // include the lower bound
  if (filters.from !== undefined) {
    parameters.set("from", filters.from);
  }

  // include the upper bound
  if (filters.to !== undefined) {
    parameters.set("to", filters.to);
  }

  // include the page cursor
  if (cursor !== undefined) {
    parameters.set("cursor", cursor);
  }

  return `${normalizeBaseUrl(apiBaseUrl)}/sites/${encodeURIComponent(siteSlug)}/history?${parameters.toString()}`;
}

// construct a filtered current endpoint
export function buildCurrentUrl(
  apiBaseUrl: string,
  siteSlug: string,
  filters: HistoryFilters,
): string {
  const parameters = new URLSearchParams();

  // include the station filter
  if (filters.stationSlug !== undefined) {
    parameters.set("station", filters.stationSlug);
  }

  // include the source filter
  if (filters.sourceId !== undefined) {
    parameters.set("source", filters.sourceId);
  }

  const query = parameters.size === 0 ? "" : `?${parameters.toString()}`;
  return `${normalizeBaseUrl(apiBaseUrl)}/sites/${encodeURIComponent(siteSlug)}/current${query}`;
}

// construct today's nearest-gauge accumulation endpoint
export function buildDailyPrecipitationUrl(
  apiBaseUrl: string,
  siteSlug: string,
): string {
  return `${normalizeBaseUrl(apiBaseUrl)}/sites/${encodeURIComponent(siteSlug)}/daily-precipitation`;
}

// construct the fixed normalized forecast endpoint
export function buildForecastUrl(
  apiBaseUrl: string,
  siteSlug: string,
  days: ForecastDays = 1,
): string {
  const query = days === 1 ? "" : `?days=${String(days)}`;
  return `${normalizeBaseUrl(apiBaseUrl)}/sites/${encodeURIComponent(siteSlug)}/forecast${query}`;
}

// construct the bounded observed and predicted tide endpoint
export function buildTidesUrl(
  apiBaseUrl: string,
  siteSlug: string,
): string {
  return `${normalizeBaseUrl(apiBaseUrl)}/sites/${encodeURIComponent(siteSlug)}/tides`;
}

// construct the shared property sensor layout endpoint
export function buildPropertySensorLayoutUrl(
  apiBaseUrl: string,
  siteSlug: string,
): string {
  return `${normalizeBaseUrl(apiBaseUrl)}/sites/${encodeURIComponent(siteSlug)}/property-sensor-layout`;
}

// construct the private per-request viewer context endpoint
export function buildViewerContextUrl(apiBaseUrl: string): string {
  return `${normalizeBaseUrl(apiBaseUrl)}/viewer-context`;
}

// construct one authenticated property sensor update endpoint
export function buildAdminPropertySensorLayoutUrl(
  apiBaseUrl: string,
  siteSlug: string,
  sensorKey: string,
): string {
  return `${normalizeBaseUrl(apiBaseUrl)}/admin/sites/${encodeURIComponent(siteSlug)}/property-sensor-layout/${encodeURIComponent(sensorKey)}`;
}

// construct the authenticated adjustment settings endpoint
export function buildAdminForecastAdjustmentSettingsUrl(
  apiBaseUrl: string,
  siteSlug: string,
): string {
  return `${normalizeBaseUrl(apiBaseUrl)}/admin/sites/${encodeURIComponent(siteSlug)}/forecast-adjustment-settings`;
}

// construct the authenticated queryless scorecard endpoint
export function buildAdminForecastAdjustmentScorecardUrl(
  apiBaseUrl: string,
  siteSlug: string,
): string {
  return `${normalizeBaseUrl(apiBaseUrl)}/admin/sites/${encodeURIComponent(siteSlug)}/forecast-adjustment-scorecard`;
}

// construct one bounded trend endpoint
export function buildTrendsUrl(
  apiBaseUrl: string,
  siteSlug: string,
): string {
  return `${normalizeBaseUrl(apiBaseUrl)}/sites/${encodeURIComponent(siteSlug)}/trends`;
}

// mount the interactive dashboard
export function mountWeatherDashboard(
  root: HTMLElement,
  options: DashboardOptions = {},
): WeatherDashboardController {
  const controller = new WeatherDashboardController(options);
  bindDashboardSize(root);

  // redraw and wire one state snapshot
  controller.subscribe((state) => {
    const toggleHadFocus = root.querySelector("[data-forecast-adjustment-toggle]") === document.activeElement;
    const forecastHadFocus = root.querySelector("[data-forecast-charts]") === document.activeElement;
    const forecastPosition = root.querySelector<HTMLElement>("[data-forecast-charts]")
      ?.dataset.forecastSelectedPosition;
    const retainForecast = !state.loading;
    root.innerHTML = renderWeatherDashboard(state, controller.view, controller.isAdmin);

    // retain the selected forecast hour across preferences and background weather refreshes
    if ((toggleHadFocus || retainForecast) && forecastPosition !== undefined && forecastPosition !== null) {
      root.querySelector<HTMLElement>("[data-forecast-charts]")
        ?.setAttribute("data-forecast-initial-index", forecastPosition);
    }

    fitHomepageTitle(root);
    fitStickyHeader(root);
    bindDashboardControls(root, controller);

    // retain keyboard focus on the replaced preference switch
    if (toggleHadFocus) {
      root.querySelector<HTMLButtonElement>("[data-forecast-adjustment-toggle]")
        ?.focus({ preventScroll: true });
    }
    // keep keyboard scrubbing usable after an automatic weather refresh
    if (forecastHadFocus && retainForecast) {
      root.querySelector<HTMLElement>("[data-forecast-charts]")?.focus({ preventScroll: true });
    }
  });
  bindConditionDayRefresh(root, controller);
  bindHomeNetworkRefresh(root, controller);
  bindCurrentReadingsRefresh(root, controller);
  void controller.initialize();
  return controller;
}

// keep sunlight input and expiry current while weather-bearing pages are visible
function bindCurrentReadingsRefresh(root: HTMLElement, controller: WeatherDashboardController): void {
  let timer: number | undefined;
  // retire listeners and polling with the detached dashboard
  const cleanup = (): void => {
    window.clearInterval(timer);
    window.removeEventListener("online", refresh);
    document.removeEventListener("visibilitychange", refresh);
  };
  // resume safely without fetching historical or administrative products
  const refresh = (): void => {
    // detached roots cannot create background requests
    if (!root.isConnected) {
      cleanup();
      return;
    }
    // hidden tabs wait for an explicit visibility resume
    if (document.visibilityState !== "visible") {
      return;
    }
    void controller.refreshCurrentReadings();
  };
  window.addEventListener("online", refresh);
  document.addEventListener("visibilitychange", refresh);
  timer = window.setInterval(refresh, 60_000);
}

// refit dashboard labels and sticky offsets after viewport and font changes
function bindDashboardSize(root: HTMLElement): void {
  const observer = new ResizeObserver(() => {
    // release the observer when its application root is removed
    if (!root.isConnected) {
      observer.disconnect();
      return;
    }
    fitHomepageTitle(root);
    fitStickyHeader(root);
    fitForecastValueLabels(root);
  });
  observer.observe(root);
  // measure labels again after the final fonts load
  void document.fonts.ready.then(() => {
    fitHomepageTitle(root);
    fitStickyHeader(root);
    fitForecastValueLabels(root);
  });
}

// reserve the complete pinned header height for other sticky elements
function fitStickyHeader(root: HTMLElement): void {
  const header = root.querySelector<HTMLElement>(".home-masthead, .forecast-masthead");
  const height = header === null ? 0 : header.getBoundingClientRect().height +
    (Number.parseFloat(getComputedStyle(header).top) || 0);
  root.style.setProperty("--weather-sticky-header-height", `${String(height)}px`);
}

// retain one complete title line at the largest size that fits beside the switch
function fitHomepageTitle(root: HTMLElement): void {
  const heading = root.querySelector<HTMLElement>(".home-masthead h1");
  const text = heading?.querySelector<HTMLElement>(".masthead-title-text");
  // leave other routes and detached application roots unchanged
  if (heading == null || text == null || !root.isConnected) {
    return;
  }
  heading.style.removeProperty("font-size");
  // shrink only at unusually narrow widths or enlarged text settings
  if (text.scrollWidth > text.clientWidth && text.clientWidth > 0) {
    const size = Number.parseFloat(getComputedStyle(heading).fontSize);
    heading.style.fontSize = `${size * (text.clientWidth - 1) / text.scrollWidth}px`;
  }
}

// contain selected forecast values without covering chart headings
function fitForecastValueLabels(root: HTMLElement): void {
  // preserve the preferred plot edge unless its pill would cover the title
  for (const chart of root.querySelectorAll<HTMLElement>("[data-forecast-chart]")) {
    const value = chart.querySelector<HTMLElement>(".forecast-chart-value");
    const heading = chart.querySelector<HTMLElement>(".forecast-chart-heading h3");
    const preferredEdge = value?.dataset.forecastValueEdge;
    // skip skeletons and incomplete chart furniture
    if (value === null || heading === null || preferredEdge === undefined) {
      continue;
    }
    value.classList.toggle("forecast-chart-value-top", preferredEdge === "top");
    value.classList.toggle("forecast-chart-value-bottom", preferredEdge === "bottom");
    value.style.marginLeft = "0px";
    const valueBounds = value.getBoundingClientRect();
    const chartBounds = chart.getBoundingClientRect();
    const innerLeft = chartBounds.left + chart.clientLeft;
    const innerRight = innerLeft + chart.clientWidth;
    const shift = Math.max(innerLeft - valueBounds.left, Math.min(0, innerRight - valueBounds.right));
    // use actual pill width rather than the fixed crosshair edge threshold
    value.style.marginLeft = `${String(shift)}px`;
    // lower pills cannot intersect the fixed top heading
    if (preferredEdge !== "top") {
      continue;
    }
    const headingBounds = heading.getBoundingClientRect();
    // retain a small clearance around the actual title rather than its padded wrapper
    if (valueBounds.left + shift < headingBounds.right + 2 && valueBounds.right + shift > headingBounds.left - 2 &&
      valueBounds.top < headingBounds.bottom + 2 && valueBounds.bottom > headingBounds.top - 2) {
      value.classList.remove("forecast-chart-value-top");
      value.classList.add("forecast-chart-value-bottom");
    }
  }
}

// revalidate a visible homepage without refreshing weather data
function bindHomeNetworkRefresh(root: HTMLElement, controller: WeatherDashboardController): void {
  let timer: number | undefined;

  // retire listeners with the mounted dashboard
  const cleanup = (): void => {
    window.clearInterval(timer);
    window.removeEventListener("online", refresh);
    document.removeEventListener("visibilitychange", refresh);
  };

  // check only a visible anonymous homepage
  const refresh = (): void => {
    // discard a detached dashboard
    if (!root.isConnected) {
      cleanup();
      return;
    }

    // avoid background and unrelated-route requests
    if (document.visibilityState !== "visible" || controller.view !== "home" || controller.isAdmin) {
      return;
    }

    void controller.refreshHomeNetwork();
  };

  window.addEventListener("online", refresh);
  document.addEventListener("visibilitychange", refresh);
  timer = window.setInterval(refresh, 60_000);
}

// refresh calendar-dependent tiles at farm midnight and after a suspended tab resumes
function bindConditionDayRefresh(root: HTMLElement, controller: WeatherDashboardController): void {
  let timer: number | undefined;
  let previousDay: string | null = null;

  // update daily summaries and replace the one-day forecast after rollover
  const refresh = (): void => {
    window.clearTimeout(timer);

    // retire the clock when its dashboard is removed
    if (!root.isConnected) {
      document.removeEventListener("visibilitychange", refresh);
      return;
    }

    const tile = root.querySelector("[data-condition='sunset']:not(.skeleton-card)");

    // leave loading skeletons and other routes untouched
    if (tile !== null) {
      tile.outerHTML = renderSunsetCondition(controller.state);
    }

    const clouds = root.querySelector("[data-condition='clouds']:not(.skeleton-card)");

    // roll the clearest forecast into the new farm day
    if (clouds !== null) {
      clouds.outerHTML = renderCloudsCondition(controller.state);
    }

    const now = new Date();
    const site = controller.state.selectedSite ?? PRODUCT_SITE;
    const timezone = site.timezone;
    const icon = root.querySelector(".section-nav-weather-icon");
    // switch the current illustration at sunrise and sunset without refreshing weather
    if (icon !== null) {
      icon.outerHTML = renderCurrentWeatherIcon(controller.state, now);
    }
    const day = formatWallClockParts(now, timezone);
    const today = forecastSiteDateKey(now.toISOString(), timezone);

    // replace yesterday's exhausted forecast only when the homepage crosses a day
    if (clouds !== null && previousDay !== null && previousDay !== today) {
      void controller.loadSelectedSite();
    }

    previousDay = today;
    const tomorrow = new Date(Date.UTC(day.year, day.month - 1, day.day + 1)).toISOString().slice(0, 10);
    const midnight = Date.parse(fromSiteWallClock(`${tomorrow}T00:00`, timezone));
    const sun = eveningSunTimes(site, now);
    const cacheExpiry = controller.state.cachedNowIcon === null
      ? undefined
      : controller.state.cachedNowIcon.cachedAt + NOW_ICON_CACHE_TTL_MS;
    const boundaries = [midnight, sun.sunrise?.getTime(), sun.sunset?.getTime(), cacheExpiry].filter(
      // ignore absent and already elapsed daylight or cache boundaries
      (instant): instant is number => instant !== undefined && instant > now.getTime(),
    );
    timer = window.setTimeout(refresh, Math.max(1, Math.min(...boundaries) - now.getTime()));
  };

  document.addEventListener("visibilitychange", refresh);
  refresh();
}

// render the complete accessible dashboard
export function renderWeatherDashboard(
  state: DashboardState,
  view: WeatherView = "home",
  isAdmin = false,
): string {
  state = withLocalForecastAdjustments(withSolarCloudAdjustment(state));
  return `
    <main class="shell">
      <header class="masthead${view === "forecast" ? " forecast-masthead" : view === "home" ? " home-masthead" : ""}">
        ${view === "home" ? `<h1><span class="masthead-title-text"><span>Ballydídean</span> <span>Weather</span></span></h1>` : "<h1>Ballydídean Weather</h1>"}
        ${view === "forecast" ? renderForecastRangeSelector(state.forecastDays ?? 1, state.loading) : ""}
        <div class="masthead-actions">
          ${renderForecastAdjustmentToggle(state, view)}
        </div>
      </header>
      ${renderSectionNavigation(state, view)}
      ${renderLoadingStatus(state)}
      <div class="weather-content" aria-busy="${String(state.loading)}">
        ${renderErrorStatus(state)}
        ${renderWeatherView(state, view, isAdmin)}
        ${renderCredits(state, view)}
      </div>
    </main>
  `;
}

// select the approved artwork from the same current metrics as the homepage cards
export function currentWeatherIcon(
  state: Pick<DashboardState, "current" | "selectedSite"> & Partial<Pick<DashboardState, "forecast" | "forecastAdjustmentMode">>,
  now = new Date(),
): Readonly<{ name: string; label: string }> {
  const projected = withSolarCloudAdjustment(state, now);
  return selectCurrentWeatherIcon(currentWeatherIconInputs(projected.current, state.forecastAdjustmentMode, true, state.forecast, now), state.selectedSite ?? PRODUCT_SITE, now);
}

// share selected source priority and cloud provenance between live and cached artwork
function currentWeatherIconInputs(
  records: readonly WeatherRecord[],
  mode: ForecastAdjustmentMode = "adjusted",
  useSolarAdjustment = false,
  forecast: readonly WeatherRecord[] = [],
  now = new Date(),
): NowIconInputs | null {
  const current = currentWeatherReadings(records, mode, forecast, now);
  const rain = current?.metrics.precipitationRateMmPerHour ?? null;
  const windy = (current?.metrics.windSpeedMps ?? 0) >= 8.9408;
  const cloudRecord = records.find(
    // cloud cover is modeled rather than measured by the on-site gateway
    (record) => record.provenance.sourceKind === "model_current" && record.metrics.cloudCoverPercent !== null,
  );
  const cloud = cloudRecord === undefined ? null : forecastMetricValue(cloudRecord, "cloudCoverPercent", useSolarAdjustment && mode !== "raw");
  return parseNowIconInputs({ rain, cloud, windy });
}

// recompute sun or moon artwork at the current farm time even for cached inputs
function selectCurrentWeatherIcon(
  inputs: NowIconInputs | null,
  site: WeatherSite,
  now: Date,
): Readonly<{ name: string; label: string }> {
  // missing rainfall must not imply a sunny or dry condition
  if (inputs === null) {
    return { name: "12-unavailable", label: "Conditions unavailable" };
  }
  const { rain, cloud, windy: wind } = inputs;
  const suffix = wind ? ", high wind" : "";
  // rain takes precedence even when cloud cover is unavailable
  if (rain > 0) {
    // use the widget's light and heavy rain boundary
    if (rain < 2.5) {
      return { name: wind ? "08-light-rain-wind" : "07-light-rain", label: `Light rain${suffix}` };
    }
    return { name: wind ? "10-heavy-rain-wind" : "09-heavy-rain", label: `Heavy rain${suffix}` };
  }
  // overcast artwork is shared between day and night
  if (cloud !== null && cloud >= 75) {
    return { name: wind ? "06-cloudy-wind" : "05-cloudy", label: `Cloudy${suffix}` };
  }
  const sun = eveningSunTimes(site, now);
  const night = sun.sunrise !== null && sun.sunset !== null &&
    (now < sun.sunrise || now >= sun.sunset);
  // replace only the sun-bearing illustrations after dark
  if (cloud !== null && cloud < 25) {
    return night
      ? { name: wind ? "14-clear-night-wind" : "13-clear-night", label: `Clear night${suffix}` }
      : { name: wind ? "02-sunny-wind" : "01-sunny", label: `Sunny${suffix}` };
  }
  return night
    ? { name: wind ? "16-partly-cloudy-night-wind" : "15-partly-cloudy-night", label: `Partly cloudy night${suffix}` }
    : { name: wind ? "04-partly-cloudy-wind" : "03-partly-cloudy", label: `Partly cloudy${suffix}` };
}

// keep known artwork through navigation and reserve its space while first loading
function renderCurrentWeatherIcon(state: DashboardState, now = new Date()): string {
  state = withSolarCloudAdjustment(state, now);
  const mode = state.forecastAdjustmentMode ?? "adjusted";
  const cached = isNowIconCacheFresh(state.cachedNowIcon, now.getTime()) &&
    (state.cachedNowIcon.forecastAdjustmentMode ?? "adjusted") === mode
    ? state.cachedNowIcon
    : null;
  const inputs = currentWeatherIconInputs(state.current, mode, true, state.forecast, now) ?? cached;
  // loading is not an unavailable weather condition
  if (inputs === null && state.loading) {
    return `<span class="section-nav-weather-icon section-nav-weather-skeleton skeleton-line" role="img" aria-label="Loading current weather" aria-busy="true"></span>`;
  }
  const icon = selectCurrentWeatherIcon(inputs, state.selectedSite ?? PRODUCT_SITE, now);
  return `<img class="section-nav-weather-icon" src="/weather-icons/${icon.name}.svg" alt="Current weather: ${escapeHtml(icon.label)}" width="32" height="32">`;
}

// keep adjustment semantics accessible while the thumb alone shows the preference
function renderForecastAdjustmentToggle(
  state: DashboardState,
  view: WeatherView,
): string {
  // limit the preference to forecast-bearing pages
  if (view !== "home" && view !== "forecast") {
    return "";
  }

  const available = forecastAdjustmentsAvailable(state);
  // reflect the persisted preference even during regional fallback
  const adjusted = state.forecastAdjustmentMode !== "raw";
  return `
    <button
      type="button"
      class="forecast-adjustment-toggle"
      role="switch"
      aria-checked="${String(adjusted)}"
      aria-label="Adjusted"
      title="Switch between adjusted and regional values."
      data-forecast-adjustment-toggle
      data-forecast-adjustment-activation-mode="${escapeHtml(state.forecastAdjustmentRuntime?.activationMode ?? "disabled")}"
      data-forecast-adjustment-available="${String(available)}"
      data-forecast-adjustment-fallback="${String(!available && adjusted)}"
    >
      <span class="forecast-adjustment-toggle-track" aria-hidden="true">
        <span class="forecast-adjustment-toggle-thumb">
          <svg class="forecast-adjustment-sparkle" data-sparkle-tone="${adjusted ? "gold" : "gray"}" viewBox="0 0 24 24" focusable="false">
            <path class="forecast-adjustment-sparkle-ink" fill="currentColor" stroke="currentColor" stroke-width="0.45" stroke-linejoin="round" d="m10 3.5 2.1 6.4 6.4 2.1-6.4 2.1-2.1 6.4-2.1-6.4L1.5 12l6.4-2.1L10 3.5Zm8.5-2 .9 2.6L22 5l-2.6.9-.9 2.6-.9-2.6L15 5l2.6-.9.9-2.6Z"/>
          </svg>
        </span>
      </span>
    </button>
  `;
}

// detect one usable adjusted forecast value
function forecastAdjustmentsAvailable(state: DashboardState): boolean {
  const settings = state.forecastAdjustmentSettings ?? null;
  return [...state.current, ...state.forecast].some(
    // experimental local corrections remain separate from governed model families
    (record) => solarCloudValues.has(record) || localForecastBiases.has(record),
  ) || state.forecast.some(
    // require one validated active decision from either isolated runtime
    (record) =>
      (state.forecastAdjustmentRuntime?.state === "active" &&
        record.adjustment?.state === "active" &&
        record.adjustment.appliedMetrics.some(
          // expose any permitted qualified or wind metric
          (metric) => forecastAdjustmentMetricEnabled(metric, settings),
        )) ||
      ((settings === null || settings.temperature) &&
        state.forecastTemperatureAdjustmentRuntime?.state === "active" &&
        record.temperatureAdjustment?.state === "active") ||
      ((settings === null || settings.rain) &&
        state.forecastRainAdjustmentRuntime?.state === "active" &&
        record.rainAdjustment?.state === "active"),
  );
}

// render one route body
function renderWeatherView(state: DashboardState, view: WeatherView, isAdmin: boolean): string {
  // render the authenticated property sensor editor
  if (view === "admin") {
    return `${renderForecastAdjustmentAdmin(state)}${renderForecastAdjustmentScorecard(state)}${renderPropertySensorAdmin(state)}`;
  }

  // render historical records alone
  if (view === "logs") {
    return renderHistory(state);
  }

  // render nearby stations alone
  if (view === "map") {
    return `${renderPropertySensorMap(state)}${renderStationMap(state)}`;
  }

  // render the daily forecast alone
  if (view === "forecast") {
    return renderForecast(state);
  }

  // render historical trends alone
  if (view === "trends") {
    return renderTrends(state);
  }

  // render device-local display preferences alone
  if (view === "settings") {
    return renderUnitSettings(state.units);
  }

  return renderHomepage(state, isAdmin);
}

// render homepage panels for administrators or home-network viewers
function renderHomepage(state: DashboardState, isAdmin: boolean): string {
  return `
    ${renderAlerts(state)}
    ${renderCurrent(state)}
    ${state.forecastAdjustmentMode !== "raw" && (isAdmin || state.homeNetwork) ? `${renderIndoorHouse(state)}${renderAdminSoilMoistureMap(state)}` : ""}
  `;
}

// render stable product routes with current conditions as the now illustration
function renderSectionNavigation(state: DashboardState, view: WeatherView): string {
  const settingsCurrent = view === "settings" || view === "logs" || view === "admin";

  return `
    <nav class="section-nav" aria-label="Weather sections">
      <a class="section-nav-home" href="/" data-weather-route aria-label="Now"${view === "home" ? ' aria-current="page"' : ""}><span class="section-nav-icon">${renderCurrentWeatherIcon(state)}</span><span>Now</span></a>
      <a class="section-nav-forecast" href="/forecast" data-weather-route${view === "forecast" ? ' aria-current="page"' : ""}><span class="section-nav-icon">${renderMaterialIcon("partly_cloudy_day")}</span><span>Forecast</span></a>
      <a class="section-nav-trends" href="/trends" data-weather-route${view === "trends" ? ' aria-current="page"' : ""}><span class="section-nav-icon">${renderMaterialIcon("trending_up")}</span><span>Trends</span></a>
      <a class="section-nav-map" href="/map" data-weather-route${view === "map" ? ' aria-current="page"' : ""}><span class="section-nav-icon">${renderMaterialIcon("map")}</span><span>Map</span></a>
      <a class="section-nav-settings" href="/settings" data-weather-route${settingsCurrent ? ' aria-current="page"' : ""}><span class="section-nav-icon">${renderMaterialIcon("settings")}</span><span>Settings</span></a>
    </nav>
  `;
}

// render local preferences and full-page policy access
function renderUnitSettings(units: UnitPreferences): string {
  return `
    <div class="settings-page">
      <nav class="settings-destinations" aria-label="Weather data">
        <a class="settings-logs-link" href="/logs" data-weather-route aria-label="Logs">
          <span class="settings-destination-icon">${renderMaterialIcon("history")}</span>
          <span><strong>Logs</strong><small>Browse current and historical readings</small></span>
        </a>
        <a class="settings-logs-link" href="/admin" aria-label="Admin">
          <span class="settings-destination-icon">${renderMaterialIcon("settings")}</span>
          <span><strong>Admin</strong></span>
        </a>
        <a class="settings-logs-link" href="/privacy" aria-label="Privacy policy">
          <span class="settings-destination-icon"><svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M12 3 4 6v6c0 5 8 9 8 9s8-4 8-9V6Z"/><path d="m8 12 3 3 5-6"/></svg></span>
          <span><strong>Privacy policy</strong><small>What happens to your data</small></span>
        </a>
      </nav>
      <section class="unit-settings-page" aria-labelledby="unit-settings-heading">
        <div class="unit-settings-heading">
          <p class="eyebrow">Display preferences</p>
          <h2 id="unit-settings-heading">Measurement units</h2>
          <p class="unit-settings-intro">Choose how weather measurements appear on this device.</p>
        </div>
        <form class="unit-settings-form" data-unit-settings-form>
          <div class="unit-settings-grid">
            <label><span>Temperature</span><select name="temperature">
              <option value="fahrenheit"${units.temperature === "fahrenheit" ? " selected" : ""}>Fahrenheit (°F)</option>
              <option value="celsius"${units.temperature === "celsius" ? " selected" : ""}>Celsius (°C)</option>
            </select></label>
            <label><span>Wind speed</span><select name="windSpeed">
              <option value="miles_per_hour"${units.windSpeed === "miles_per_hour" ? " selected" : ""}>Miles per hour (mph)</option>
              <option value="kilometers_per_hour"${units.windSpeed === "kilometers_per_hour" ? " selected" : ""}>Kilometers per hour (km/h)</option>
              <option value="meters_per_second"${units.windSpeed === "meters_per_second" ? " selected" : ""}>Meters per second (m/s)</option>
            </select></label>
            <label><span>Precipitation</span><select name="precipitation">
              <option value="inches"${units.precipitation === "inches" ? " selected" : ""}>Inches (in)</option>
              <option value="millimeters"${units.precipitation === "millimeters" ? " selected" : ""}>Millimeters (mm)</option>
            </select></label>
            <label><span>Pressure</span><select name="pressure">
              <option value="atmosphere_percent"${units.pressure === "atmosphere_percent" ? " selected" : ""}>Difference from 1 atm (%)</option>
              <option value="inches_of_mercury"${units.pressure === "inches_of_mercury" ? " selected" : ""}>Inches of mercury (inHg)</option>
              <option value="hectopascals"${units.pressure === "hectopascals" ? " selected" : ""}>Hectopascals (hPa)</option>
            </select></label>
            <label><span>Tide height</span><select name="waterLevel">
              <option value="feet"${units.waterLevel === "feet" ? " selected" : ""}>Feet (ft)</option>
              <option value="meters"${units.waterLevel === "meters" ? " selected" : ""}>Meters (m)</option>
            </select></label>
          </div>
          <div class="unit-settings-actions">
            <button type="submit">Save units</button>
          </div>
        </form>
      </section>
      <footer class="settings-version" role="group" aria-label="Application version" data-app-version></footer>
    </div>
  `;
}

// render provider licenses and the visible sanctuary credit
function renderCredits(state: DashboardState, view: WeatherView): string {
  const attributions = new Map<string, string>();
  let includesOpenMeteo = false;

  // collect every station attribution
  for (const station of state.selectedSite?.stations ?? []) {
    // collect every unique provider
    for (const source of station.sources) {
      attributions.set(source.attribution.url, source.attribution.label);

      // retain the Open-Meteo license requirement
      if (source.providerKey === "open-meteo") {
        includesOpenMeteo = true;
      }
    }
  }

  let providerCredits = "";

  // render each unique provider once
  for (const [url, label] of attributions) {
    providerCredits += `<a href="${escapeHtml(url)}" rel="noreferrer">${escapeHtml(label)}</a><span aria-hidden="true">·</span>`;
  }

  // include the required Open-Meteo license
  const licenseCredit = includesOpenMeteo
    ? `<span>Open-Meteo data licensed under <a href="https://creativecommons.org/licenses/by/4.0/" rel="license noreferrer">CC BY 4.0</a></span><span aria-hidden="true">·</span>`
    : "";

  // show map credits only while the embedded forecast map is enabled
  const forecastMapCredits = SHOW_FORECAST_WEATHER_MAP && view === "forecast"
    ? `<span>Map © <a href="https://www.openstreetmap.org/copyright" rel="noreferrer">OpenStreetMap contributors</a></span><span aria-hidden="true">·</span><a href="https://www.xweather.com/" rel="noreferrer">Weather maps by Xweather</a><span aria-hidden="true">·</span>`
    : "";
  // attribute only an enabled temperature adjustment
  const temperatureCanaryCredit =
    state.forecastTemperatureAdjustmentRuntime?.state === "active" &&
    (state.forecastAdjustmentSettings?.temperature ?? true)
      ? `<span>Adjusted temperature uses ECMWF IFS single-run data; raw temperature uses Open-Meteo Best Match.</span><span aria-hidden="true">·</span>`
      : "";

  return `
    <footer class="credits" aria-label="Weather data credits">
      <details>
        <summary>Data sources &amp; credits</summary>
        <div class="credit-list">
          ${providerCredits}
          ${licenseCredit}
          ${forecastMapCredits}
          ${temperatureCanaryCredit}
        </div>
      </details>
      <p class="project-credit">Built with love by <a href="https://ballydidean.farm" rel="noreferrer">Ballydidean Farm Sanctuary</a></p>
    </footer>
  `;
}

// announce refreshes without a visible header indicator
function renderLoadingStatus(state: DashboardState): string {
  const message = state.loading || state.propertySensorLayoutLoading
    ? "Refreshing weather data…"
    : state.error === null
      ? "Weather data is up to date."
      : "Weather refresh failed.";
  return `<p class="sr-only" role="status">${message}</p>`;
}

// render error feedback without affecting routine refreshes
function renderErrorStatus(state: DashboardState): string {
  // expose only actionable failures in the document flow
  if (state.error !== null) {
    return `<p class="notice error" role="alert">${escapeHtml(state.error)}</p>`;
  }

  return "";
}

// render the current summary
function renderCurrent(state: DashboardState): string {
  // reserve the cards for initial reads and later refreshes
  if (state.loading) {
    return renderCurrentSkeleton();
  }

  const current = currentWeatherReadings(state.current, state.forecastAdjustmentMode, state.forecast);

  // render an honest empty state
  if (current === null) {
    return '<p class="notice">No current weather value is available yet.</p>';
  }

  const airQuality = current.metrics.pm25MicrogramsPerCubicMeter;
  const dailyRain = state.forecastAdjustmentMode === "raw" ? null : state.dailyPrecipitation?.accumulationMm ?? null;
  const rainRate = current.metrics.precipitationRateMmPerHour;
  const uvIndex = current.metrics.uvIndex;
  const windGust = current.metrics.windGustMps;
  const windDirection = formatWindDirection(current.metrics.windDirectionDegrees);
  const windMeasurement = formatMeasurement(current.metrics.windSpeedMps, "windSpeed", state.units, 0);
  // place the direction immediately after the speed unit
  const directionalWindMeasurement = {
    ...windMeasurement,
    unit: windDirection === null ? windMeasurement.unit : `${windMeasurement.unit} ${windDirection}`,
  };
  const forecast = forecastForSiteDay(
    state.forecast,
    current.forecastReferenceAt,
    state.selectedSite?.timezone ?? PRODUCT_SITE.timezone,
  );
  const useForecastAdjustments = state.forecastAdjustmentMode !== "raw";
  const regional = currentWeatherReadings(state.current, "raw", state.forecast);
  const temperatureForecast = forecastTemperature(forecast, state.units, useForecastAdjustments);
  const windForecast = forecastWind(forecast, state.units, useForecastAdjustments);
  const rainForecast = forecastRain(forecast, state.units, useForecastAdjustments);
  const humidityForecast = forecastHumidity(forecast, useForecastAdjustments);
  const airQualityForecast = forecastMaximumFixed(forecast, "pm25MicrogramsPerCubicMeter", "", 0, airQualityBand, useForecastAdjustments);
  const uvForecast = forecastMaximumFixed(forecast, "uvIndex", "", 1, uvBand, useForecastAdjustments);
  // homepage gold identifies changed current readings or a changed displayed daily summary
  const changed = (keys: readonly WeatherMetricKey[], selected: ForecastCardValue, raw: ForecastCardValue): boolean =>
    useForecastAdjustments && (keys.some(
      // source selection counts only when two usable numeric readings actually differ
      (key) => adjustmentValuesDiffer(current.metrics[key], regional?.metrics[key] ?? null),
    ) || forecastReadingsDiffer(selected, raw));
  return `
    <section class="current-conditions" aria-label="Current conditions">
      ${renderConditionCard({
          adjusted: changed(["apparentTemperatureC", "temperatureC"], temperatureForecast, forecastTemperature(forecast, state.units, false)),
          band: temperatureBand(current.metrics.apparentTemperatureC),
          className: "temperature-condition",
          icon: "device_thermostat",
          label: "Temperature",
          measurement: formatMeasurement(current.metrics.apparentTemperatureC, "temperature", state.units, 0),
          forecast: temperatureForecast,
          secondary: {
            label: "Air Temp",
            measurement: formatMeasurement(current.metrics.temperatureC, "temperature", state.units, 0),
          },
        })}
      ${renderConditionCard({
          adjusted: changed(["windSpeedMps", "windGustMps", "windDirectionDegrees"], windForecast, forecastWind(forecast, state.units, false)),
          band: windBand(current.metrics.windSpeedMps, windGust, state.units),
          className: "wind-condition",
          icon: "air",
          label: "Wind",
          measurement: directionalWindMeasurement,
          forecast: windForecast,
          secondary: {
            label: "Gusts",
            measurement: formatMeasurement(windGust, "windSpeed", state.units, 0),
          },
        })}
      ${renderConditionCard({
          adjusted: changed(["precipitationRateMmPerHour"], rainForecast, forecastRain(forecast, state.units, false)),
          band: rainBand(rainRate),
          className: "rain-condition",
          icon: "rainy",
          label: "Rain",
          measurement: formatPrecipitationRate(rainRate, state.units),
          forecast: rainForecast,
          secondary: {
            label: "Accumulation",
            measurement: formatPrecipitationAccumulation(dailyRain, state.units),
          },
        })}
      ${renderCloudsCondition(state)}
      ${renderConditionCard({
          adjusted: changed(["relativeHumidityPercent"], humidityForecast, forecastHumidity(forecast, false)),
          band: humidityBand(current.metrics.relativeHumidityPercent, current.metrics.temperatureC),
          className: "compact-condition",
          icon: "humidity_percentage",
          label: "Humidity",
          measurement: formatFixedMeasurement(current.metrics.relativeHumidityPercent, "%"),
          forecast: humidityForecast,
        })}
      ${renderConditionCard({
          adjusted: changed(["pm25MicrogramsPerCubicMeter"], airQualityForecast, forecastMaximumFixed(forecast, "pm25MicrogramsPerCubicMeter", "", 0, airQualityBand, false)),
          band: airQualityBand(airQuality),
          className: "air-quality-condition",
          icon: "masks",
          label: "Air quality",
          measurement: formatFixedMeasurement(airQuality, "", 0),
          forecast: airQualityForecast,
        })}
      ${renderPressureCondition(state)}
      ${renderConditionCard({
          adjusted: changed(["uvIndex"], uvForecast, forecastMaximumFixed(forecast, "uvIndex", "", 1, uvBand, false)),
          band: uvBand(uvIndex),
          className: "compact-condition",
          icon: "wb_sunny",
          label: "UV index",
          measurement: formatFixedMeasurement(uvIndex, ""),
          forecast: uvForecast,
        })}
      ${renderTideCondition(state)}
      ${renderSunsetCondition(state)}
    </section>
  `;
}

// render the admin-only house cross section
function renderIndoorHouse(state: DashboardState): string {
  const sensorsByKey = new Map(
    propertySensorSnapshots(state).map(
      // index every current hardware snapshot
      (sensor) => [sensor.key, sensor],
    ),
  );
  return `
    <section class="indoor-house-panel" data-indoor-house aria-labelledby="indoor-house-heading" aria-busy="${String(state.loading)}">
      <div class="indoor-house-heading">
        ${renderMaterialIcon("home")}
        <div><p class="eyebrow">Inside the house</p><h2 id="indoor-house-heading">Indoor temperatures</h2></div>
      </div>
      <div class="indoor-house-illustration">
        <div class="indoor-house-roof" aria-hidden="true"><span></span></div>
        <ol class="indoor-house-levels" aria-label="Indoor temperature by floor">
          ${INDOOR_HOUSE_LEVELS.map(
            // render every physical floor in top-to-bottom order
            (level) => {
              const temperatureC = sensorsByKey.get(level.sensorKey)?.readings.temperatureC ?? null;
              const measurement = formatMeasurement(temperatureC, "temperature", state.units, 0);
              // share forecast thresholds without coloring pending placeholders
              const tone = forecastTemperatureTone(state.loading ? null : temperatureC);
              return `<li class="indoor-house-level indoor-house-level-${level.slug}"><span>${level.label}</span><span class="indoor-house-temperature condition-forecast-tone-${tone}">${state.loading ? '<span class="skeleton-line skeleton-temperature" aria-hidden="true"></span>' : renderConditionMeasurement(measurement)}</span></li>`;
            },
          ).join("")}
        </ol>
      </div>
    </section>
  `;
}

// keep derived homepage values separate from any station's provenance
interface CurrentWeatherReadings {
  readonly forecastReferenceAt: string;
  readonly metrics: WeatherRecord["metrics"];
  readonly pressureChange3hHpa: number | null;
}

// share metric-specific farm and nearby fallbacks across cards alerts and artwork
function currentWeatherReadings(
  records: readonly WeatherRecord[],
  mode: ForecastAdjustmentMode = "adjusted",
  forecast: readonly WeatherRecord[] = [],
  now = new Date(),
): CurrentWeatherReadings | null {
  const regional = records.filter(
    // retain only current regional products rather than historical reanalysis
    (record) => record.provenance.sourceKind === "model_current",
  ).map(
    // reuse the bounded raw-hour supplements without applying model corrections
    (record) => regionalCurrentForecastRecord(record, forecast, now),
  );
  const model = regional[0];
  // never fill regional gaps with physical station readings
  if (mode === "raw") {
    const pressure = regional.find(
      // retain the existing same-source pressure tendency contract
      (record) => usableCurrentMetric(record.metrics.pressureHpa, "pressureHpa") !== null,
    );
    return model === undefined ? null : {
      forecastReferenceAt: model.validAt,
      metrics: {
        ...model.metrics,
        pm25MicrogramsPerCubicMeter: findMetric(regional, "pm25MicrogramsPerCubicMeter"),
        precipitationRateMmPerHour: findMetric(regional, "precipitationRateMmPerHour"),
        uvIndex: findMetric(regional, "uvIndex"),
        windGustMps: findMetric(regional, "windGustMps"),
      },
      pressureChange3hHpa: pressure?.freshness.status === "fresh" ? usableCurrentMetric(pressure.pressureChange3hHpa ?? null) : null,
    };
  }

  const physical = records.filter(
    // delayed hourly stations remain available but stale and future observations do not
    (record) => record.provenance.sourceKind === "physical_sensor" &&
      record.freshness.status !== "stale" && Date.parse(record.validAt) <= now.getTime(),
  ).toSorted(
    // prefer the newest usable station source independently of API ordering
    (left, right) => Date.parse(right.validAt) - Date.parse(left.validAt) ||
      Date.parse(right.receivedAt) - Date.parse(left.receivedAt) || left.provenance.sourceKey.localeCompare(right.provenance.sourceKey),
  );
  const farmRecords = physical.filter(isFarmCurrentRecord);
  const nearby = physical.filter(
    // exclude the farm from the fallback average rather than giving it two votes
    (record) => !isFarmCurrentRecord(record),
  );
  const reference = farmRecords[0] ?? nearby[0] ?? model;
  // never resurrect stale observations when all current sources are missing
  if (reference === undefined) {
    return null;
  }

  // give each physical station at most one usable value for each metric
  const contributors = (metric: WeatherMetricKey): readonly WeatherRecord[] => {
    const stations = new Map<string, WeatherRecord>();
    // sorted sources allow a newer missing metric to retain an older usable source
    for (const record of nearby) {
      // include valid zeros without counting duplicate providers twice
      if (!stations.has(record.provenance.stationSlug) && usableCurrentMetric(record.metrics[metric], metric) !== null) {
        stations.set(record.provenance.stationSlug, record);
      }
    }
    return [...stations.values()];
  };
  // use the newest nonmissing farm source separately for every metric
  const farmReading = (metric: WeatherMetricKey): WeatherRecord | undefined => farmRecords.find(
    // preserve farm precedence even when another current source lacks this sensor
    (record) => usableCurrentMetric(record.metrics[metric], metric) !== null,
  );
  // preserve every available farm reading before consulting nearby stations or models
  const metric = (key: WeatherMetricKey): number | null => {
    const observed = usableCurrentMetric(farmReading(key)?.metrics[key] ?? null, key);
    // partial sensor outages must not replace the other working farm sensors
    if (observed !== null) {
      return observed;
    }
    const values = contributors(key).map(
      // retain equal station weighting independent of provider or source count
      (record) => record.metrics[key]!,
    );
    // average only stations that actually measured this metric
    if (values.length > 0) {
      return key === "windDirectionDegrees" ? meanCurrentWindDirection(values) : meanCurrentValues(values);
    }
    return regional.map(
      // skip an unavailable model field without consuming physical or historical sources
      (record) => usableCurrentMetric(record.metrics[key], key),
    ).find(
      // retain the first available regional fallback
      (value) => value !== null,
    ) ?? null;
  };
  const pressureSources = contributors("pressureHpa");
  const farmPressure = farmReading("pressureHpa");
  const pressure = farmPressure !== undefined ? [farmPressure] : pressureSources.length > 0 ? pressureSources : regional.filter(
    // never display a model tendency when its associated current pressure is missing
    (record) => usableCurrentMetric(record.metrics.pressureHpa, "pressureHpa") !== null,
  ).slice(0, 1);
  const pressureChanges = pressure.flatMap(
    // average only fresh same-source tendencies from the selected pressure contributors
    (record) => record.freshness.status === "fresh" && usableCurrentMetric(record.pressureChange3hHpa ?? null) !== null
      ? [record.pressureChange3hHpa!] : [],
  );
  return {
    // hourly neighbors may still be dated yesterday just after farm-local midnight
    forecastReferenceAt: farmRecords.length === 0 && nearby.length > 0 ? now.toISOString() : reference.validAt,
    metrics: {
      ...reference.metrics,
      apparentTemperatureC: metric("apparentTemperatureC"),
      pm25MicrogramsPerCubicMeter: metric("pm25MicrogramsPerCubicMeter"),
      precipitationRateMmPerHour: metric("precipitationRateMmPerHour"),
      pressureHpa: metric("pressureHpa"),
      relativeHumidityPercent: metric("relativeHumidityPercent"),
      temperatureC: metric("temperatureC"),
      uvIndex: metric("uvIndex"),
      windDirectionDegrees: metric("windDirectionDegrees"),
      windGustMps: metric("windGustMps"),
      windSpeedMps: metric("windSpeedMps"),
      wetBulbGlobeTemperatureC: metric("wetBulbGlobeTemperatureC"),
    },
    pressureChange3hHpa: meanCurrentValues(pressureChanges),
  };
}

// identify the farm gateway without promoting another ecowitt station
function isFarmCurrentRecord(record: WeatherRecord): boolean {
  return record.provenance.providerKey === "ecowitt-local" && record.provenance.stationSlug === "ballydidean-ecowitt";
}

// retain valid zeros and freezing temperatures while rejecting impossible readings
function usableCurrentMetric(value: number | null, metric?: WeatherMetricKey): number | null {
  // preserve signed pressure changes independently of the displayed weather metric bounds
  if (value === null || !Number.isFinite(value)) {
    return null;
  }
  const bounds = metric === undefined ? undefined : CURRENT_READING_METRIC_BOUNDS[metric];
  return bounds === undefined || (value >= bounds.minimum && value <= bounds.maximum &&
    (bounds.maximumExclusive !== true || value < bounds.maximum)) ? value : null;
}

// average only present contributors without treating missing values as zero
function meanCurrentValues(values: readonly number[]): number | null {
  return values.length === 0 ? null : values.reduce(
    // divide before summing to avoid overflow from finite station readings
    (total, value) => total + value / values.length,
    0,
  );
}

// average wind bearings around north without inventing a direction for opposing winds
function meanCurrentWindDirection(values: readonly number[]): number | null {
  const east = meanCurrentValues(values.map(
    // project each bearing onto the east axis
    (value) => Math.sin(value * Math.PI / 180),
  ))!;
  const north = meanCurrentValues(values.map(
    // project each bearing onto the north axis
    (value) => Math.cos(value * Math.PI / 180),
  ))!;
  return Math.hypot(east, north) < 0.000_001 ? null : (Math.atan2(east, north) * 180 / Math.PI + 360) % 360;
}

// fill three regional gaps without replacing current values or applying corrections
function regionalCurrentForecastRecord(
  current: WeatherRecord,
  forecast: readonly WeatherRecord[],
  now: Date,
): WeatherRecord {
  const hour = Math.floor(now.getTime() / 3_600_000) * 3_600_000;
  const regional = forecast.filter(
    // reject physical sources other locations stale products and adjacent hours
    (record) => record.provenance.sourceKind === "forecast" &&
      record.provenance.providerKey === current.provenance.providerKey &&
      record.provenance.stationSlug === current.provenance.stationSlug &&
      Date.parse(record.validAt) === hour &&
      Date.parse(record.productRunAt ?? record.receivedAt) <= now.getTime() &&
      now.getTime() - Date.parse(record.productRunAt ?? record.receivedAt) <= REGIONAL_FORECAST_MAX_AGE_MS,
  ).toSorted(
    // prefer the newest published run independently of response ordering
    (left, right) => Date.parse(right.productRunAt ?? right.receivedAt) - Date.parse(left.productRunAt ?? left.receivedAt) ||
      Date.parse(right.receivedAt) - Date.parse(left.receivedAt),
  )[0];
  // keep missing forecast hours honestly unavailable
  if (regional === undefined) {
    return current;
  }
  // accept dry nighttime and clean-air zeros without inventing invalid values
  const usable = (value: number | null): number | null => value !== null && Number.isFinite(value) && value >= 0 ? value : null;
  return {
    ...current,
    metrics: {
      ...current.metrics,
      pm25MicrogramsPerCubicMeter: current.metrics.pm25MicrogramsPerCubicMeter ?? usable(regional.metrics.pm25MicrogramsPerCubicMeter),
      precipitationRateMmPerHour: current.metrics.precipitationRateMmPerHour ?? usable(regional.metrics.precipitationRateMmPerHour),
      uvIndex: current.metrics.uvIndex ?? usable(regional.metrics.uvIndex),
    },
  };
}

// reserve the complete current-condition grid
function renderCurrentSkeleton(): string {
  const cards: readonly Readonly<{
    className: string;
    forecast: ForecastCardValue;
    icon: MaterialIconName;
    label: string;
    measurement?: FormattedMeasurement;
    secondary?: string;
    detail?: string | null;
  }>[] = [
    { className: "temperature-condition", forecast: { readings: [{ label: "Max", measurement: { unit: "°F", value: "00" } }, { label: "Min", measurement: { unit: "°F", value: "00" } }, { label: "Max", measurement: { unit: "°F", value: "00" } }, { label: "Min", measurement: { unit: "°F", value: "00" } }] }, icon: "device_thermostat", label: "Temperature", secondary: "Air Temp" },
    { className: "wind-condition", forecast: { readings: [{ label: "Max", measurement: { unit: "mph", value: "00" } }, { label: "Max", measurement: { unit: "mph", value: "00" } }] }, icon: "air", label: "Wind", secondary: "Gusts" },
    { className: "rain-condition", forecast: { readings: [{ label: "Max", measurement: { unit: "in/h", value: "0.00" } }, { label: "Total", measurement: { unit: "in", value: "0.00" } }] }, icon: "rainy", label: "Rain", secondary: "Accumulation" },
    { className: "compact-condition clouds-condition", detail: null, forecast: { readings: [{ label: "Max", measurement: { unit: "%", value: "00" } }, { label: "Min", measurement: { unit: "%", value: "00" } }] }, icon: "cloud", label: "Clouds", secondary: "Clearest" },
    { className: "compact-condition", forecast: { readings: [{ label: "Max", measurement: { unit: "%", value: "00" } }] }, icon: "humidity_percentage", label: "Humidity" },
    { className: "air-quality-condition", forecast: { readings: [{ label: "Max", measurement: { unit: "", value: "00" } }] }, icon: "masks", label: "Air quality" },
    { className: "compact-condition pressure-condition", detail: null, forecast: { readings: [{ label: "Max", measurement: { unit: "", value: "+0.0" } }, { label: "", measurement: { unit: "PM", value: "00:00" } }] }, icon: "speed", label: "Pressure", measurement: { unit: "", value: "+0.0" } },
    { className: "compact-condition", forecast: { readings: [{ label: "Max", measurement: { unit: "", value: "0.0" } }] }, icon: "wb_sunny", label: "UV index" },
    { className: "compact-condition tide-condition", detail: null, forecast: { readings: [{ label: "Next low", measurement: { unit: "", value: "00:00 PM" } }] }, icon: "water", label: "Tide", secondary: "Direction" },
    { className: "compact-condition sunset-condition", detail: null, forecast: { readings: [{ label: "vs yesterday", measurement: { unit: "mins", value: "+0" } }] }, icon: "wb_sunny", label: "Sunset", secondary: "Golden hour" },
  ];

  return `
    <section class="current-conditions skeleton-region" aria-label="Loading current conditions" aria-busy="true">
      ${cards.map(
        // preserve every final grid span
        (card) => `
          <article class="condition-card ${card.className} skeleton-card" data-condition="${card.label.toLowerCase().replaceAll(" ", "-")}" aria-hidden="true">
            <div class="condition-card-content">
              <div class="condition-card-heading"><span class="condition-label">${renderMaterialIcon(card.icon)}<span>${renderConditionLabel(card.label)}</span></span><span class="condition-status">Loading</span></div>
              <div class="condition-body${card.secondary === undefined ? "" : " condition-body-secondary"}">
                <div class="condition-live">
                  <div class="condition-primary">${renderConditionMeasurement(card.measurement ?? { unit: "unit", value: "00" })}</div>
                  ${card.secondary === undefined ? "" : `<div class="condition-secondary"><span>${card.secondary}</span><strong>00<small>unit</small></strong></div>`}
                </div>
                ${renderConditionForecast(card.forecast)}
              </div>
              ${card.detail === null ? "" : `<p class="condition-detail">${card.detail ?? "Loading"}</p>`}
            </div>
          </article>
        `,
      ).join("")}
    </section>
  `;
}

// show observed pressure movement beside the day's strongest change and its time
function renderPressureCondition(state: DashboardState): string {
  const current = currentWeatherReadings(state.current, state.forecastAdjustmentMode, state.forecast);
  const change = current?.pressureChange3hHpa ?? null;
  const site = state.selectedSite ?? PRODUCT_SITE;
  const useAdjustments = state.forecastAdjustmentMode !== "raw";
  const now = new Date();
  const maximum = strongestPressureChange(state.forecast, now, site.timezone, useAdjustments);
  const rawMaximum = strongestPressureChange(state.forecast, now, site.timezone, false);
  const regional = currentWeatherReadings(state.current, "raw", state.forecast);
  return renderConditionCard({
    // compare actual same-source tendencies and the displayed strongest window
    adjusted: useAdjustments && (adjustmentValuesDiffer(change, regional?.pressureChange3hHpa ?? null) ||
      adjustmentValuesDiffer(maximum?.changeHpa ?? null, rawMaximum?.changeHpa ?? null) ||
      (maximum !== null && rawMaximum !== null && maximum.validAt !== rawMaximum.validAt)),
    band: { ...pressureChangeBand(change), detail: "" },
    className: "compact-condition pressure-condition",
    forecast: {
      readings: [
        {
          label: "Max",
          measurement: formatPressureChange(maximum?.changeHpa ?? null),
          tone: forecastToneForBand(maximum?.changeHpa ?? null, pressureChangeBand(maximum?.changeHpa ?? null)),
        },
        {
          label: "",
          measurement: formatConditionTime(maximum === null ? null : new Date(maximum.validAt), site.timezone),
        },
      ],
    },
    icon: "speed",
    label: "Pressure",
    measurement: formatPressureChange(change),
  });
}

// format unitless signed three-hour movement without implying a health-risk score
function formatPressureChange(changeHpa: number | null): FormattedMeasurement {
  // distinguish missing history from genuinely steady pressure
  if (changeHpa === null || !Number.isFinite(changeHpa)) {
    return { unit: "", value: "—" };
  }

  const rounded = Math.round(Math.abs(changeHpa) * 10) / 10 * Math.sign(changeHpa);
  return {
    unit: "",
    value: new Intl.NumberFormat("en-US", {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
      signDisplay: "exceptZero",
    }).format(rounded === 0 ? 0 : rounded),
  };
}

// derive aligned rolling changes from complete same-source forecast windows
export function forecastPressureChanges(
  records: readonly WeatherRecord[],
  hours: readonly WeatherRecord[] = records,
  useAdjustments = false,
): readonly (number | null)[] {
  const available = records.filter(
    // exclude observations and incomplete pressure samples
    (record) => record.provenance.sourceKind === "forecast" &&
      record.metrics.pressureHpa !== null && Number.isFinite(record.metrics.pressureHpa),
  );
  const byHour = new Map(available.map(
    // keep each forecast source and model run isolated
    (record) => [`${record.provenance.sourceId}/${record.productRunAt ?? ""}/${Date.parse(record.validAt)}`, record],
  ));
  return hours.map(
    // retain unavailable windows at their exact timeline positions
    (end) => {
      const prefix = `${end.provenance.sourceId}/${end.productRunAt ?? ""}/`;
      const endMs = Date.parse(end.validAt);
      const start = byHour.get(`${prefix}${endMs - 3 * 3_600_000}`);
      // require all four finite forecast samples without crossing a source or run gap
      if (end.provenance.sourceKind !== "forecast" || end.metrics.pressureHpa === null ||
        !Number.isFinite(end.metrics.pressureHpa) || start === undefined ||
        !byHour.has(`${prefix}${endMs - 2 * 3_600_000}`) || !byHour.has(`${prefix}${endMs - 3_600_000}`)) {
        return null;
      }

      return forecastPressureChangeValue(end, end.metrics.pressureHpa - start.metrics.pressureHpa!, useAdjustments);
    },
  );
}

// correct only the displayed same-source tendency without mixing absolute pressure datums
function forecastPressureChangeValue(record: WeatherRecord, raw: number | null, useAdjustments: boolean): number | null {
  const bias = useAdjustments ? localForecastBiases.get(record)?.pressureChange3hHpa ?? null : null;
  const metadata = bias === null ? undefined : localForecastPolicies.get(bias);
  return metadata === undefined ? raw : projectForecastLocalBias(raw, record.validAt, bias, metadata.policy);
}

// share chart and evaluator pressure composition without crossing source or run datums
export function forecastPressureSeries(
  records: readonly WeatherRecord[],
  hours: readonly WeatherRecord[],
  pressureContext: readonly WeatherRecord[],
  useAdjustments = false,
): readonly (number | null)[] {
  const context = pressureContext.toSorted(
    // six contiguous prior-vintage rows have deterministic endpoint order
    (left, right) => Date.parse(left.validAt) - Date.parse(right.validAt),
  );
  const first = context[0];
  const validContext = context.length === 6 && first !== undefined && typeof first.productRunAt === "string" && context.every(
    // reject incomplete mixed-source mixed-run or malformed context independently
    (record, index) => record.provenance.sourceKind === "forecast" && record.provenance.sourceId === first.provenance.sourceId &&
      record.productRunAt !== null && record.productRunAt === first.productRunAt && record.metrics.pressureHpa !== null &&
      Number.isFinite(record.metrics.pressureHpa) && Date.parse(record.validAt) === Date.parse(first.validAt) + index * 3_600_000,
  );
  const contextChanges = validContext ? forecastPressureChanges(context) : [];
  const contextByHour = new Map(context.map(
    // index completed differences rather than mixing absolute endpoint pressures
    (record, index) => [`${record.provenance.sourceId}/${Date.parse(record.validAt)}`, contextChanges[index] ?? null],
  ));
  return forecastPressureChanges(records, hours).map(
    // preserve the existing first-three-position fallback and visible end-record policy
    (change, index) => {
      const record = hours[index]!;
      const raw = change ?? (index < 3 ? contextByHour.get(`${record.provenance.sourceId}/${Date.parse(record.validAt)}`) ?? null : null);
      return forecastPressureChangeValue(record, raw, useAdjustments);
    },
  );
}

// select the earliest strongest complete three-hour window across the whole day
export function strongestPressureChange(
  records: readonly WeatherRecord[],
  now: Date,
  timezone: string,
  useAdjustments = false,
): Readonly<{ changeHpa: number; validAt: string }> | null {
  const hours = forecastForSiteDay(records, now.toISOString(), timezone).toSorted(
    // resolve equal-magnitude windows by their earliest ending time
    (left, right) => Date.parse(left.validAt) - Date.parse(right.validAt),
  );
  const changes = forecastPressureChanges(hours, hours, useAdjustments);
  let strongest: Readonly<{ changeHpa: number; validAt: string }> | null = null;
  // compare only continuous three-hour forecast windows
  for (const [index, end] of hours.entries()) {
    const changeHpa = changes[index];
    // keep the first window when magnitudes tie
    if (changeHpa !== null && changeHpa !== undefined &&
      (strongest === null || Math.abs(changeHpa) > Math.abs(strongest.changeHpa))) {
      strongest = { changeHpa, validAt: end.validAt };
    }
  }

  return strongest;
}

// color magnitude using Met Éireann's three-hour meteorological tendency bands
// https://www.met.ie/forecasts/marine-inland-lakes/sea-area-forecast-terminology
export function pressureChangeBand(changeHpa: number | null): ConditionBand {
  // preserve unavailable observations instead of assigning a calm color
  if (changeHpa === null || !Number.isFinite(changeHpa)) {
    return unavailableBand("Three-hour pressure history unavailable");
  }

  const magnitude = Math.round(Math.abs(changeHpa) * 10) / 10;
  const direction = changeHpa < 0 ? "fall" : "rise";
  const detail = "Pressure change over 3 hours; weather tendency, not a discomfort prediction";
  // distinguish steady pressure from directional movement
  if (magnitude < 0.5) {
    return { color: "rgb(0, 146, 63)", detail, label: "Steady" };
  }
  // show gradual changes with the mildest directional color
  if (magnitude < 2) {
    return { color: "rgb(200, 183, 68)", detail, label: `Slow ${direction}` };
  }
  // show ordinary pressure movement without a rapid modifier
  if (magnitude < 3.5) {
    return { color: "rgb(230, 181, 25)", detail, label: changeHpa < 0 ? "Falling" : "Rising" };
  }
  // reserve orange for rapid change in either direction
  if (magnitude < 6) {
    return { color: "rgb(239, 126, 31)", detail, label: `Rapid ${direction}` };
  }

  return { color: "rgb(207, 67, 55)", detail, label: `Very rapid ${direction}` };
}

// compare daylight and overall clarity alongside full-day cloud extrema
function renderCloudsCondition(state: DashboardState): string {
  state = withSolarCloudAdjustment(state);
  const site = state.selectedSite ?? PRODUCT_SITE;
  const useAdjustments = state.forecastAdjustmentMode !== "raw";
  const current = state.current.filter(
    // keep model estimates distinct from on-site observations
    (record) => record.provenance.sourceKind === "model_current",
  );
  const currentCloud = current.find(
    // select one modeled current value for both its reading and correction marker
    (record) => record.metrics.cloudCoverPercent !== null,
  );
  const cover = currentCloud === undefined ? null : forecastMetricValue(currentCloud, "cloudCoverPercent", useAdjustments);
  const sunlightEstimate = currentCloud !== undefined && useAdjustments && solarCloudValues.has(currentCloud);
  const now = new Date();
  const forecast = forecastForSiteDay(state.forecast, now.toISOString(), site.timezone);
  const sun = eveningSunTimes(site, now);
  const daytime = clearestCloudRange(forecast, site.timezone, sun, useAdjustments);
  const overall = clearestCloudRange(forecast, site.timezone, undefined, useAdjustments);
  const rawDaytime = clearestCloudRange(forecast, site.timezone, sun, false);
  const rawOverall = clearestCloudRange(forecast, site.timezone, undefined, false);
  const differentRange = overall.value !== "—" &&
    (overall.value !== daytime.value || overall.unit !== daytime.unit);

  return renderConditionCard({
    // gold requires a changed current cloud reading or a displayed daily comparison
    adjusted: useAdjustments && (adjustmentValuesDiffer(cover, currentCloud?.metrics.cloudCoverPercent ?? null) ||
      adjustmentValuesDiffer(maximumMetric(forecast, "cloudCoverPercent", true), maximumMetric(forecast, "cloudCoverPercent", false)) ||
      adjustmentValuesDiffer(minimumMetric(forecast, "cloudCoverPercent", true), minimumMetric(forecast, "cloudCoverPercent", false)) ||
      adjustmentMeasurementsDiffer(daytime, rawDaytime) || adjustmentMeasurementsDiffer(overall, rawOverall)),
    band: sunlightEstimate ? { ...cloudBand(cover), detail: "Sunlight estimate" } : cloudBand(cover),
    className: `compact-condition clouds-condition${sunlightEstimate ? " solar-cloud-estimate" : ""}`,
    forecast: {
      readings: [
        { label: "Max", measurement: formatFixedMeasurement(maximumMetric(forecast, "cloudCoverPercent", useAdjustments), "%", 0) },
        { label: "Min", measurement: formatFixedMeasurement(minimumMetric(forecast, "cloudCoverPercent", useAdjustments), "%", 0) },
      ],
    },
    icon: "cloud",
    label: "Clouds",
    measurement: formatFixedMeasurement(cover, "%", 0),
    secondary: {
      label: "Clearest",
      measurement: daytime,
      comparison: differentRange ? { label: "Overnight", measurement: overall } : undefined,
    },
  });
}

// format the earliest minimum-cover window with nearest-hour endpoints
export function clearestCloudRange(
  records: readonly WeatherRecord[],
  timezone: string,
  daylight?: Readonly<{ sunrise: Date | null; sunset: Date | null }>,
  useAdjustments = false,
): FormattedMeasurement {
  const hours = records.filter(
    // reject bins wholly outside daylight before choosing the minimum
    (hour) => daylight === undefined || (
      daylight.sunrise !== null && daylight.sunset !== null &&
      Date.parse(hour.validAt) < daylight.sunset.getTime() &&
      Date.parse(hour.validAt) + 3_600_000 > daylight.sunrise.getTime()
    ),
  ).toSorted(
    // preserve caller order while finding adjacent hourly bins
    (left, right) => Date.parse(left.validAt) - Date.parse(right.validAt),
  );
  const minimum = minimumMetric(hours, "cloudCoverPercent", useAdjustments);

  // keep missing forecasts distinct from clear skies
  if (minimum === null) {
    return { unit: "", value: "—" };
  }

  const first = hours.findIndex(
    // retain the earliest tied minimum rather than spanning separate windows
    (record) => forecastMetricValue(record, "cloudCoverPercent", useAdjustments) === minimum,
  );
  const firstHour = Date.parse(hours[first]!.validAt);
  const start = new Date(Math.max(firstHour, daylight?.sunrise?.getTime() ?? firstHour));
  let end = firstHour + 3_600_000;

  // include every consecutive minimum-cover hour and its full interval
  for (const hour of hours.slice(first + 1)) {
    // stop at higher cover, missing data or a gap in the hourly series
    if (forecastMetricValue(hour, "cloudCoverPercent", useAdjustments) !== minimum || Date.parse(hour.validAt) !== end) {
      break;
    }

    end += 3_600_000;
  }

  const day = formatWallClockParts(start, timezone);
  const tomorrow = new Date(Date.UTC(day.year, day.month - 1, day.day + 1)).toISOString().slice(0, 10);
  const midnight = Date.parse(fromSiteWallClock(`${tomorrow}T00:00`, timezone));
  const finish = new Date(Math.min(end, midnight, daylight?.sunset?.getTime() ?? midnight));
  // round only the display after selecting and clipping the actual window
  const roundedStart = roundConditionHour(start, timezone);
  const roundedFinish = roundConditionHour(finish, timezone);
  const from = formatConditionTime(roundedStart, timezone);
  const to = formatConditionTime(roundedFinish, timezone);
  const fromClock = from.value.replace(/:00$/u, "");
  const toClock = to.value.replace(/:00$/u, "");

  // show one time when both rounded endpoints coincide
  if (roundedStart.getTime() === roundedFinish.getTime()) {
    return roundedFinish.getTime() === midnight
      ? { unit: "", value: "midnight" }
      : { unit: from.unit, value: fromClock };
  }

  // make the end-of-day boundary explicit rather than repeating twelve am
  if (roundedFinish.getTime() === midnight) {
    return { unit: "", value: `${fromClock} ${from.unit}–midnight` };
  }

  // disambiguate the repeated clock hour when daylight saving time ends
  if (from.value === to.value && from.unit === to.unit) {
    const formatter = new Intl.DateTimeFormat("en-US", {
      hour: "numeric",
      minute: "2-digit",
      timeZone: timezone,
      timeZoneName: "short",
    });
    return { unit: "", value: `${formatter.format(roundedStart).replace(":00", "")}–${formatter.format(roundedFinish).replace(":00", "")}` };
  }

  const startLabel = from.unit === to.unit ? fromClock : `${fromClock} ${from.unit}`;
  return { unit: to.unit, value: `${startLabel}–${toClock}` };
}

// choose the nearest real local hour across offset changes
function roundConditionHour(value: Date, timezone: string): Date {
  const minute = formatWallClockParts(value, timezone).minute;
  const hourStart = value.getTime() - minute * 60_000 -
    value.getUTCSeconds() * 1_000 - value.getUTCMilliseconds();
  // realign boundaries when daylight saving shifts by part of an hour
  const previous = new Date(hourStart - formatWallClockParts(new Date(hourStart), timezone).minute * 60_000);
  const nextStart = hourStart + 3_600_000;
  const nextMinute = formatWallClockParts(new Date(nextStart), timezone).minute;
  const next = new Date(nextStart + (60 - nextMinute) % 60 * 60_000);
  return value.getTime() - previous.getTime() < next.getTime() - value.getTime() ? previous : next;
}

// calculate sunrise, evening events and sunset's change from the previous site day
export function eveningSunTimes(
  site: Pick<WeatherSite, "latitude" | "longitude" | "timezone">,
  now = new Date(),
): Readonly<{ goldenHourStart: Date | null; sunrise: Date | null; sunset: Date | null; sunsetChangeMinutes: number | null }> {
  const day = formatWallClockParts(now, site.timezone);
  const midnight = Date.UTC(day.year, day.month - 1, day.day);
  const localNoon = Date.parse(fromSiteWallClock(
    formatWallClock({ ...day, hour: 12, minute: 0 }),
    site.timezone,
  ));
  const solarNoon = midnight + (720 - 4 * site.longitude) * 60_000;
  // align civil and solar dates across the international date line
  const solarDay = midnight + Math.round((localNoon - solarNoon) / 86_400_000) * 86_400_000;
  const sunset = solarEvent(solarDay, site.latitude, site.longitude, -0.833);
  const previousSunset = solarEvent(solarDay - 86_400_000, site.latitude, site.longitude, -0.833);
  let sunsetChangeMinutes: number | null = null;

  // compare the displayed local minutes including daylight-saving clock changes
  if (sunset !== null && previousSunset !== null) {
    const todayClock = formatWallClockParts(new Date(Math.round(sunset.getTime() / 60_000) * 60_000), site.timezone);
    const previousClock = formatWallClockParts(new Date(Math.round(previousSunset.getTime() / 60_000) * 60_000), site.timezone);
    sunsetChangeMinutes = (wallClockEpoch(todayClock) - wallClockEpoch(previousClock)) / 60_000 - 1_440;
  }

  return {
    // use the evening +6° crossing rather than a fixed hour before sunset
    goldenHourStart: solarEvent(solarDay, site.latitude, site.longitude, 6),
    sunrise: solarEvent(solarDay, site.latitude, site.longitude, -0.833, "rising"),
    sunset,
    sunsetChangeMinutes,
  };
}

// solve NOAA's rising or descending hour angle with event-time declination and refraction
// https://gml.noaa.gov/grad/solcalc/main.js
// golden-hour convention: https://github.com/mourner/suncalc
function solarEvent(
  midnight: number,
  latitude: number,
  longitude: number,
  altitude: number,
  direction: "rising" | "descending" = "descending",
): Date | null {
  const radians = Math.PI / 180;
  const latitudeRadians = latitude * radians;
  let instant = midnight + 12 * 3_600_000;

  // refine each event to sub-minute precision
  for (let iteration = 0; iteration < 3; iteration += 1) {
    const century = (instant / 86_400_000 + 2_440_587.5 - 2_451_545) / 36_525;
    const meanLongitude = (280.46646 + century * (36_000.76983 + 0.0003032 * century)) * radians;
    const meanAnomaly = (357.52911 + century * (35_999.05029 - 0.0001537 * century)) * radians;
    const eccentricity = 0.016708634 - century * (0.000042037 + 0.0000001267 * century);
    const center = Math.sin(meanAnomaly) * (1.914602 - century * (0.004817 + 0.000014 * century)) +
      Math.sin(2 * meanAnomaly) * (0.019993 - 0.000101 * century) +
      Math.sin(3 * meanAnomaly) * 0.000289;
    const omega = (125.04 - 1934.136 * century) * radians;
    const apparentLongitude = meanLongitude + (center - 0.00569 - 0.00478 * Math.sin(omega)) * radians;
    const obliquity = (23 + (26 + (21.448 - century * (46.815 +
      century * (0.00059 - 0.001813 * century))) / 60) / 60 + 0.00256 * Math.cos(omega)) * radians;
    const declination = Math.asin(Math.sin(obliquity) * Math.sin(apparentLongitude));
    const tangentSquared = Math.tan(obliquity / 2) ** 2;
    const equationMinutes = 4 / radians * (
      tangentSquared * Math.sin(2 * meanLongitude) - 2 * eccentricity * Math.sin(meanAnomaly) +
      4 * eccentricity * tangentSquared * Math.sin(meanAnomaly) * Math.cos(2 * meanLongitude) -
      0.5 * tangentSquared ** 2 * Math.sin(4 * meanLongitude) -
      1.25 * eccentricity ** 2 * Math.sin(2 * meanAnomaly)
    );
    const cosineHourAngle = (Math.sin(altitude * radians) -
      Math.sin(latitudeRadians) * Math.sin(declination)) /
      (Math.cos(latitudeRadians) * Math.cos(declination));

    // preserve absent polar crossings without inventing a clock time
    if (!Number.isFinite(cosineHourAngle) || Math.abs(cosineHourAngle) > 1) {
      return null;
    }

    const hourAngle = Math.acos(cosineHourAngle) / radians * (direction === "rising" ? -1 : 1);
    instant = midnight + (720 - 4 * longitude + 4 * hourAngle - equationMinutes) * 60_000;
  }

  return new Date(instant);
}

// show today's evening events and signed sunset change independently of observations
function renderSunsetCondition(state: DashboardState): string {
  const site = state.selectedSite ?? PRODUCT_SITE;
  const times = eveningSunTimes(site);
  const change = times.sunsetChangeMinutes;

  return renderConditionCard({
    band: { color: "rgb(239, 126, 31)", detail: "", label: "Today" },
    className: "compact-condition sunset-condition",
    forecast: {
      readings: [{
        label: "vs yesterday",
        measurement: {
          unit: change === null ? "" : "mins",
          value: change === null ? "—" : `${change > 0 ? "+" : ""}${change}`,
        },
      }],
    },
    icon: "wb_sunny",
    label: "Sunset",
    measurement: formatConditionTime(times.sunset, site.timezone),
    secondary: {
      label: "Golden hour",
      measurement: formatConditionTime(times.goldenHourStart, site.timezone),
    },
  });
}

// round condition times to a local minute with subordinate meridiem
function formatConditionTime(value: Date | null, timezone: string): FormattedMeasurement {
  // keep unavailable event times explicit
  if (value === null) {
    return { unit: "", value: "—" };
  }

  const rounded = new Date(Math.round(value.getTime() / 60_000) * 60_000);
  const [time, period = ""] = formatForecastTime(rounded.toISOString(), timezone).split(" ");
  return { unit: period, value: time! };
}

// render the latest observed tide and next local event
function renderTideCondition(state: DashboardState): string {
  const generatedAt = Date.parse(state.tideGeneratedAt ?? "");
  const asOf = Number.isFinite(generatedAt) ? generatedAt : Date.now();
  const observations = state.tides.filter(
    // retain observations available at response generation
    (record) => record.kind === "observation" && Date.parse(record.validAt) <= asOf,
  );
  const current = observations.at(-1);
  const previous = observations.at(-2);
  const next = state.tides.find(
    // select the next explicit tide turn
    (record) =>
      record.kind === "prediction" &&
      record.eventType !== null &&
      Date.parse(record.validAt) >= asOf,
  );
  const direction = tideDirectionBand(current, previous, next);
  const currentMeasurement = formatMeasurement(current?.waterLevelM ?? null, "waterLevel", state.units);
  const level = tideLevelLabel(current, state.tides);
  const nextLow = state.tides.find(
    // locate the next predicted low tide
    (record) =>
      record.kind === "prediction" &&
      record.eventType === "low" &&
      Date.parse(record.validAt) >= asOf,
  );

  return renderConditionCard({
    band: { ...direction, detail: "", label: level },
    className: "compact-condition tide-condition",
    icon: "water",
    label: "Tide",
    measurement: currentMeasurement,
    forecast: {
      readings: [{
        label: "Next low",
        measurement: {
          unit: "",
          value: nextLow === undefined
            ? "Unavailable"
            : formatForecastTime(nextLow.validAt, state.selectedSite?.timezone),
        },
      }],
    },
    secondary: {
      label: "Direction",
      measurement: { unit: "", value: direction.label },
    },
  });
}

// classify the recent observed tide direction
function tideDirectionBand(
  current: TideRecord | undefined,
  previous: TideRecord | undefined,
  next: TideRecord | undefined,
): ConditionBand {
  // preserve an unavailable observation honestly
  if (current === undefined) {
    return unavailableBand("No recent NOAA tide observation");
  }

  // infer direction from the next turn when only one observation exists
  if (previous === undefined) {
    return next?.eventType === "low"
      ? { color: "rgb(124, 81, 116)", detail: "Falling", label: "Falling" }
      : { color: "rgb(56, 120, 197)", detail: "Rising", label: "Rising" };
  }

  const changeM = current.waterLevelM - previous.waterLevelM;
  return changeM >= 0
    ? { color: "rgb(56, 120, 197)", detail: "Rising", label: "Rising" }
    : { color: "rgb(124, 81, 116)", detail: "Falling", label: "Falling" };
}

// classify observed height within the surrounding local tidal range
export function tideLevelLabel(
  current: TideRecord | undefined,
  records: readonly TideRecord[],
): "High" | "Medium" | "Low" | "Unavailable" {
  // preserve a missing observation honestly
  if (current === undefined) {
    return "Unavailable";
  }

  const currentTime = Date.parse(current.validAt);
  const turns = records
    .filter(
      // retain explicit predicted turns
      (record) => record.kind === "prediction" && record.eventType !== null,
    )
    .toSorted(
      // order turns around the observation
      (left, right) => Date.parse(left.validAt) - Date.parse(right.validAt),
    );
  const previousTurn = turns.findLast(
    // locate the prior local extreme
    (record) => Date.parse(record.validAt) <= currentTime,
  );
  const nextTurn = turns.find(
    // locate the next local extreme
    (record) => Date.parse(record.validAt) >= currentTime,
  );
  const surroundingTurns = previousTurn !== undefined && nextTurn !== undefined
    ? [previousTurn, nextTurn]
    : turns;
  const levels = surroundingTurns.map(
    // collect comparable local datum levels
    (record) => record.waterLevelM,
  );
  const low = Math.min(...levels);
  const high = Math.max(...levels);

  // avoid inventing a level without a usable range
  if (!Number.isFinite(low) || !Number.isFinite(high) || high - low < 0.05) {
    return "Medium";
  }

  const position = (current.waterLevelM - low) / (high - low);

  // label the lower third of the local range
  if (position <= 1 / 3) {
    return "Low";
  }

  // label the upper third of the local range
  if (position >= 2 / 3) {
    return "High";
  }

  return "Medium";
}

interface LocalWeatherAlert {
  readonly detail: string;
  readonly label: string;
  readonly tone: "caution" | "danger";
}

// describe one current-condition health or comfort band
interface ConditionBand {
  readonly color: string;
  readonly detail: string;
  readonly label: string;
}

// configure one friendly current-condition card
interface ConditionCardOptions {
  readonly adjusted?: boolean;
  readonly band: ConditionBand;
  readonly className: string;
  readonly forecast: ForecastCardValue;
  readonly icon: MaterialIconName;
  readonly label: string;
  readonly measurement: FormattedMeasurement;
  readonly secondary?: Readonly<{
    label: string;
    measurement: FormattedMeasurement;
    comparison?: Readonly<{
      label: string;
      measurement: FormattedMeasurement;
    }> | undefined;
  }>;
}

interface ForecastCardValue {
  readonly readings: readonly Readonly<{
    label: string;
    measurement: FormattedMeasurement;
    tone?: ForecastTone;
  }>[];
}

type ForecastTone = "blue" | "burgundy" | "gold" | "gray" | "green" | "neutral" | "orange" | "purple" | "red" | "yellow";

// map bearings to compact compass labels
const WIND_CARDINAL_DIRECTIONS = [
  "N",
  "NNE",
  "NE",
  "ENE",
  "E",
  "ESE",
  "SE",
  "SSE",
  "S",
  "SSW",
  "SW",
  "WSW",
  "W",
  "WNW",
  "NW",
  "NNW",
] as const;

// describe one synchronized forecast chart
interface ForecastChartDefinition {
  readonly adjusted?: boolean;
  readonly domain?: Readonly<{ maximum: number; minimum: number }>;
  readonly format: ForecastChartFormat;
  readonly icon: MaterialIconName;
  readonly key: string;
  readonly label: string;
  readonly series: readonly ForecastChartSeries[];
  // pair humidity colors with the same forecast hour's air temperature
  readonly temperaturesC?: readonly (number | null)[];
}

// describe one line inside a forecast chart
interface ForecastChartSeries {
  readonly label: string;
  readonly values: readonly (number | null)[];
}

type ForecastChartFormat =
  | "airQuality"
  | "cloudCover"
  | "humidity"
  | "precipitationRate"
  | "pressureChange"
  | "temperature"
  | "uvIndex"
  | "waterLevel"
  | "windSpeed";

type MaterialIconName =
  | "air"
  | "cloud"
  | "close"
  | "device_thermostat"
  | "history"
  | "home"
  | "humidity_percentage"
  | "logout"
  | "map"
  | "masks"
  | "partly_cloudy_day"
  | "radar"
  | "rainy"
  | "settings"
  | "speed"
  | "trending_up"
  | "water"
  | "wb_sunny";

type RgbColor = readonly [number, number, number];

type WeatherMetricKey = keyof WeatherRecord["metrics"];
type TrendMetricKey = keyof TrendPoint["metrics"];

// render threshold-based local weather watches
function renderAlerts(state: DashboardState): string {
  const alerts = deriveAlerts(state);

  // hide the watch region when every threshold is clear
  if (alerts.length === 0) {
    return "";
  }

  return `
    <section class="alert-list" aria-label="Conditions to watch">
      ${alerts.map((alert) => `<article class="local-alert ${alert.tone}"><strong>${escapeHtml(alert.label)}</strong><span>${escapeHtml(alert.detail)}</span></article>`).join("")}
    </section>
  `;
}

// derive watches and displayed readings from the selected forecast values
function deriveAlerts(state: DashboardState): readonly LocalWeatherAlert[] {
  const alerts: LocalWeatherAlert[] = [];
  const current = currentWeatherReadings(state.current, state.forecastAdjustmentMode, state.forecast);
  const useForecastAdjustments = state.forecastAdjustmentMode !== "raw";
  const forecastLow = minimumMetric(state.forecast, "temperatureC", useForecastAdjustments);
  const apparentHigh = current?.metrics.apparentTemperatureC ?? null;
  const wetBulbHigh = current?.metrics.wetBulbGlobeTemperatureC ?? null;
  const currentGust = current?.metrics.windGustMps ?? null;
  const forecastGust = maximumMetric(state.forecast, "windGustMps", useForecastAdjustments);
  const windHigh = currentGust === null ? forecastGust : Math.max(currentGust, forecastGust ?? currentGust);
  const rainRate = current?.metrics.precipitationRateMmPerHour ?? null;
  const forecastRain = maximumMetric(state.forecast, "precipitationMm", useForecastAdjustments);
  const pm25 = current?.metrics.pm25MicrogramsPerCubicMeter ?? null;

  // flag forecast frost
  if (forecastLow !== null && forecastLow <= 0) {
    const measurement = formatMeasurement(forecastLow, "temperature", state.units);
    alerts.push({
      detail: `Forecast low ${measurement.value}${measurement.unit}`,
      label: "Frost possible",
      tone: "caution",
    });
  }

  // flag heat-stress conditions
  if (
    (apparentHigh !== null && apparentHigh >= 32.2) ||
    (wetBulbHigh !== null && wetBulbHigh >= 29)
  ) {
    const details: string[] = [];
    // display the apparent temperature that crossed its threshold
    if (apparentHigh !== null && apparentHigh >= 32.2) {
      const measurement = formatMeasurement(apparentHigh, "temperature", state.units);
      details.push(`Apparent temperature ${measurement.value}${measurement.unit}`);
    }
    // include the independent wet-bulb threshold when exceeded
    if (wetBulbHigh !== null && wetBulbHigh >= 29) {
      const measurement = formatMeasurement(wetBulbHigh, "temperature", state.units);
      details.push(`Wet-bulb globe temperature ${measurement.value}${measurement.unit}`);
    }
    alerts.push({
      detail: details.join("; "),
      label: "Heat stress",
      tone: "danger",
    });
  }

  // flag damaging gust potential
  if (windHigh !== null && windHigh >= 15.65) {
    const measurement = formatMeasurement(windHigh, "windSpeed", state.units);
    alerts.push({
      detail: `Gusts reaching ${measurement.value} ${measurement.unit}`,
      label: "High wind",
      tone: "danger",
    });
  }

  // flag heavy observed or forecast precipitation
  if (
    (rainRate !== null && rainRate >= 7.62) ||
    (forecastRain !== null && forecastRain >= 6.35)
  ) {
    const details: string[] = [];
    // display the current hourly rate that crossed its threshold
    if (rainRate !== null && rainRate >= 7.62) {
      const measurement = formatPrecipitationRate(rainRate, state.units);
      details.push(`Current rain ${measurement.value} ${measurement.unit}`);
    }
    // display the selected forecast amount that crossed its threshold
    if (forecastRain !== null && forecastRain >= 6.35) {
      const measurement = formatMeasurement(forecastRain, "precipitation", state.units);
      details.push(`Forecast hourly rain ${measurement.value} ${measurement.unit}`);
    }
    alerts.push({
      detail: details.join("; "),
      label: "Heavy rain",
      tone: "caution",
    });
  }

  // flag unhealthy particulate levels
  if (pm25 !== null && pm25 >= 35.5) {
    alerts.push({
      detail: `PM2.5 is ${formatNumber(pm25)} µg/m³`,
      label: "Air quality",
      tone: "danger",
    });
  }

  return alerts;
}

// render the site-local forecast day
function renderForecast(state: DashboardState): string {
  // replace obsolete hours while the requested horizon loads
  if (state.loading) {
    return renderForecastSkeleton(state);
  }

  const days = state.forecastDays ?? 1;
  const regionalClock = state.current.find(
    // retain the current regional clock independently of optional archive availability
    (record) => record.provenance.sourceKind === "model_current",
  )?.validAt;
  const recordedClock = state.forecastObservations?.at(-1)?.recordedThrough;
  const clocks = [regionalClock, recordedClock].filter(
    // stale retained history cannot freeze a newer regional day or current-time line
    (value): value is string => value !== undefined && Number.isFinite(Date.parse(value)),
  );
  const reference = clocks.length === 0 ? state.forecast[0]?.validAt : new Date(Math.max(...clocks.map(
    // choose the newest trusted clock without renewing the actual observation timestamps
    (value) => Date.parse(value),
  ))).toISOString();
  const hours = reference === undefined
    ? []
    : forecastForSiteDays(
        state.forecast,
        reference,
        state.selectedSite?.timezone ?? "UTC",
        days,
      );

  // render an honest ingestion warm-up state
  if (hours.length === 0) {
    return `
      <section class="panel forecast-panel" aria-label="Weather forecast">
        <p class="empty-panel">The first normalized forecast product is being collected.</p>
      </section>
    `;
  }

  const useForecastAdjustments = state.forecastAdjustmentMode !== "raw";
  const charts = buildForecastCharts(hours, state.tides, useForecastAdjustments, state.forecast, state.forecastPressureContext ?? [], state.forecastObservations ?? [], Date.parse(reference ?? new Date().toISOString()));
  const hourlyTimes = hours.map(
    // retain one shared continuous clock
    (record) => record.validAt,
  );
  const finalHourlyTime = hourlyTimes.at(-1) ?? hours[0]!.validAt;
  const finalBoundary = new Date(new Date(finalHourlyTime).getTime() + 60 * 60 * 1_000).toISOString();
  const forecastTimes = [...hourlyTimes, finalBoundary];
  const daylightBands = renderForecastDaylightBands(hours.map(
    // align every chart's light bands to the shared hourly axis
    (hour) => forecastDaylightState(hour, state.selectedSite?.timezone),
  ));
  const dayMarkers = renderForecastDayMarkers(
    hours,
    state.selectedSite?.timezone,
    days,
  );
  const currentPosition = forecastPositionForInstant(hours, reference ?? hours[0]?.validAt);
  const selectedIndex = Math.max(0, Math.min(hours.length - 1, Math.round(currentPosition)));
  const selectedTime = interpolateForecastInstant(forecastTimes, currentPosition);
  return `
    <section class="panel forecast-panel" aria-label="Weather forecast">
      ${state.forecastObservationsLoading ? '<p class="condition-detail">Loading recorded history; regional values are shown.</p>' : state.forecastObservationsReady === false ? `<p class="condition-detail">Recorded history update is unavailable; ${state.forecastObservations?.length ? "latest available recordings and regional values" : "regional values"} are shown.</p>` : ""}
      <div class="forecast-chart-shell">
        <div class="forecast-current-time-line" aria-hidden="true"></div>
        <div class="forecast-shared-crosshair" aria-hidden="true"></div>
        <div
          class="forecast-chart-grid"
          data-forecast-charts
          data-forecast-days="${String(days)}"
          data-forecast-initial-index="${String(currentPosition)}"
          data-forecast-current-position="${String(currentPosition)}"
          data-forecast-times="${escapeHtml(JSON.stringify(forecastTimes))}"
          tabindex="0"
          role="slider"
          aria-label="Forecast time scrubber"
          aria-valuemin="0"
          aria-valuemax="${String(Math.max(0, forecastTimes.length - 1))}"
          aria-valuenow="${String(selectedIndex)}"
        >
          <div class="forecast-current-time-label" aria-hidden="true"><span>Now</span></div>
          <div class="forecast-crosshair-label" aria-hidden="true">
            <time data-forecast-crosshair-time datetime="${escapeHtml(selectedTime)}">${formatForecastHour(selectedTime, state.selectedSite?.timezone, days)}</time>
          </div>
          ${charts.map(
            // render every current-condition forecast chart
            (chart) => renderForecastChart(chart, selectedIndex, state.units, daylightBands, dayMarkers, days),
          ).join("")}
        </div>
        ${SHOW_FORECAST_WEATHER_MAP && days === 1 ? renderForecastWeatherMap(state, hours, reference ?? hours[0]?.validAt) : ""}
        ${renderForecastXAxis(hours, state.selectedSite?.timezone, days)}
      </div>
    </section>
  `;
}

// temporarily hide the embedded forecast map without loading tiles or reserving space
const SHOW_FORECAST_WEATHER_MAP = false;
const FORECAST_MAP_HEIGHT = 168;
const FORECAST_MAP_WIDTH = 256;
const FORECAST_MAP_ZOOM = 10;
const FORECAST_MAP_FORECAST_INTERVAL_MS = 60 * 60 * 1_000;
const FORECAST_MAP_CLIENT_CACHE_NAME = "weather-xweather-tiles-v1";
const FORECAST_MAP_CLIENT_CACHE_TIMESTAMP_HEADER = "X-Weather-Client-Cached-At";
const FORECAST_MAP_CLIENT_FORECAST_FRESHNESS_MS = 60 * 60 * 1_000;
const FORECAST_MAP_LAYERS: readonly Readonly<{
  icon: MaterialIconName;
  key: ForecastMapLayer;
  label: string;
}>[] = [
  { icon: "radar", key: "radar", label: "Radar" },
  { icon: "cloud", key: "clouds", label: "Clouds" },
  { icon: "rainy", key: "precipitation", label: "Rain" },
  { icon: "air", key: "wind", label: "Wind" },
];

interface ForecastMapLegendPresentation {
  readonly labels: readonly string[];
  readonly title: string;
  readonly unit: string | null;
}

// select one documented Xweather overlay legend
function forecastMapLegend(
  layer: ForecastMapLayer,
  phase: ForecastMapPhase,
): ForecastMapLegendPresentation {
  // match each public overlay to its raster color scale
  switch (layer) {
    case "radar":
      return { labels: ["10", "30", "50", "70+"], title: "Radar intensity", unit: "dBZ" };
    case "clouds":
      return {
        labels: ["Clear", "Dense"],
        title: phase === "history" ? "Satellite clouds" : "Forecast clouds",
        unit: null,
      };
    case "precipitation":
      return {
        labels: phase === "history" ? ["0", "1", "3", "5+"] : ["0", "2", "6", "10+"],
        title: phase === "history" ? "Past-hour rain" : "Forecast 1-hour rain",
        unit: "in",
      };
    case "wind":
      return { labels: ["0", "20", "50", "100"], title: "Wind speed", unit: "mph" };
  }
}

// render one compact overlay scale
function renderForecastMapLegendContent(
  presentation: ForecastMapLegendPresentation,
): string {
  return `
    <p><strong>${presentation.title}</strong>${presentation.unit === null ? "" : `<span>${presentation.unit}</span>`}</p>
    <span class="forecast-map-legend-bar" aria-hidden="true"></span>
    <span class="forecast-map-legend-labels">
      ${presentation.labels.map(
        // render every scale stop label
        (label) => `<small>${label}</small>`,
      ).join("")}
    </span>
  `;
}

// render the Today-only observed and forecast weather map
function renderForecastWeatherMap(
  state: DashboardState,
  hours: readonly WeatherRecord[],
  reference: string | undefined,
): string {
  const site = state.selectedSite;
  const first = hours[0]?.validAt;
  const last = hours.at(-1)?.validAt;

  // require one complete site-local day contract
  if (site === null || first === undefined || last === undefined || reference === undefined) {
    return "";
  }

  const startMs = new Date(first).getTime();
  const endMs = new Date(last).getTime() + 60 * 60 * 1_000;
  const nowMs = Math.max(startMs, Math.min(endMs, new Date(reference).getTime()));
  const mapStepMs = 10 * 60 * 1_000;
  const initialCacheFrames = 7;
  const initialMs = startMs + Math.floor((nowMs - startMs) / mapStepMs) * mapStepMs;
  const selectedLayer: ForecastMapLayer = "radar";
  const initialPhase: ForecastMapPhase = initialMs <= nowMs ? "history" : "forecast";
  const initialLegend = forecastMapLegend(selectedLayer, initialPhase);
  const viewport = createCenteredMapViewport(
    site.latitude,
    site.longitude,
    FORECAST_MAP_WIDTH,
    FORECAST_MAP_HEIGHT,
    FORECAST_MAP_ZOOM,
  );
  const sitePoint = projectMapPoint(site.latitude, site.longitude, viewport);
  const initialTime = new Date(initialMs).toISOString();

  return `
    <div
      class="forecast-weather-map"
      data-forecast-weather-map
      data-forecast-map-start="${String(startMs)}"
      data-forecast-map-now="${String(nowMs)}"
      data-forecast-map-end="${String(endMs)}"
      data-forecast-map-step="${String(mapStepMs)}"
      data-forecast-map-selected="${String(initialMs)}"
      data-forecast-map-layer="${selectedLayer}"
      data-forecast-map-timezone="${escapeHtml(site.timezone)}"
      aria-busy="false"
    >
      <div class="forecast-map-canvas" role="img" aria-label="Radar near ${escapeHtml(site.name)} at ${escapeHtml(formatForecastMapTime(initialTime, site.timezone))}">
        <svg class="forecast-map-svg" data-forecast-map-scrubber viewBox="0 0 ${FORECAST_MAP_WIDTH} ${FORECAST_MAP_HEIGHT}" focusable="false" aria-hidden="true">
          <g class="map-tile-layer">
            ${renderMapTiles("roads", viewport, FORECAST_MAP_WIDTH, FORECAST_MAP_HEIGHT)}
          </g>
          <g class="forecast-map-weather-layer">
            ${renderForecastWeatherFrame(selectedLayer, "history", initialTime, site)}
          </g>
          <g class="forecast-map-place-layer">
            <circle cx="${sitePoint.x.toFixed(2)}" cy="${sitePoint.y.toFixed(2)}" r="8" class="farm-marker"/>
            <text x="${(sitePoint.x + 13).toFixed(2)}" y="${(sitePoint.y + 4).toFixed(2)}">Ballydídean</text>
          </g>
        </svg>
        <div class="forecast-map-layer-controls" role="group" aria-label="Weather map layer">
          ${FORECAST_MAP_LAYERS.map(
            // render each weather overlay choice
            (layer) => `<button type="button" data-forecast-map-layer="${layer.key}" aria-pressed="${String(layer.key === selectedLayer)}">${renderMaterialIcon(layer.icon)}<span>${layer.label}</span></button>`,
          ).join("")}
        </div>
        <div class="forecast-map-selection-phase" data-forecast-map-selection-phase="${initialPhase}" aria-hidden="true">
          <span data-forecast-map-selection-phase-label>${initialPhase === "history" ? "Historical" : "Forecast"}</span>
        </div>
        <div
          class="forecast-map-legend"
          data-forecast-map-legend
          data-forecast-map-legend-layer="${selectedLayer}"
          data-forecast-map-legend-phase="${initialPhase}"
          role="img"
          aria-label="${initialLegend.title} color legend"
        >
          ${renderForecastMapLegendContent(initialLegend)}
        </div>
        <div
          class="forecast-map-cache-progress"
          data-forecast-map-cache-progress
          aria-label="Map cache progress"
          hidden
        >
          <p><strong data-forecast-map-cache-label>Caching Radar</strong><span data-forecast-map-cache-percent>0%</span></p>
          <progress data-forecast-map-cache-bar max="${String(initialCacheFrames)}" value="0" aria-label="Cached nearby Radar map frames"></progress>
          <small data-forecast-map-cache-count>0 of ${String(initialCacheFrames)} nearby frames ready</small>
        </div>
        <div class="forecast-map-loading" data-forecast-map-loading hidden aria-hidden="true"></div>
        <p class="forecast-map-error" data-forecast-map-error hidden>Weather tiles are temporarily unavailable.</p>
      </div>
    </div>
  `;
}

// create one fixed weather-map viewport around the farm
function createCenteredMapViewport(
  latitude: number,
  longitude: number,
  width: number,
  height: number,
  zoom: number,
): MapViewport {
  const center = webMercatorPoint(latitude, longitude, zoom);
  return {
    left: center.x - width / 2,
    top: center.y - height / 2,
    zoom,
  };
}

// render one aligned transparent Xweather static frame
function renderForecastWeatherFrame(
  layer: ForecastMapLayer,
  phase: ForecastMapPhase,
  validAt: string,
  site: WeatherSite,
  loadImmediately = false,
): string {
  const validTime = xweatherValidTime(validAt);
  const url = xweatherFrameUrl(phase, layer, validTime, FORECAST_MAP_ZOOM, FORECAST_MAP_WIDTH, FORECAST_MAP_HEIGHT, site.latitude, site.longitude);
  const immediateSource = loadImmediately ? ` href="${escapeHtml(url)}" fetchpriority="high"` : "";
  return `<image class="forecast-map-weather-tile" data-forecast-map-tile data-map-zoom="${String(FORECAST_MAP_ZOOM)}" data-map-width="${String(FORECAST_MAP_WIDTH)}" data-map-height="${String(FORECAST_MAP_HEIGHT)}" data-map-latitude="${site.latitude.toFixed(6)}" data-map-longitude="${site.longitude.toFixed(6)}" data-map-tile-url="${escapeHtml(url)}"${immediateSource} x="0" y="0" width="${String(FORECAST_MAP_WIDTH)}" height="${String(FORECAST_MAP_HEIGHT)}" preserveAspectRatio="none"/>`;
}

// build one same-origin Xweather static-frame proxy URL
function xweatherFrameUrl(
  phase: ForecastMapPhase,
  layer: ForecastMapLayer,
  validTime: string,
  zoom: number,
  width: number,
  height: number,
  latitude: number,
  longitude: number,
): string {
  return `/maps/xweather/${phase}/${layer}/${validTime}/${String(zoom)}/${String(width)}x${String(height)}/${latitude.toFixed(6)},${longitude.toFixed(6)}.png`;
}

// format one UTC instant for the Xweather tile API
function xweatherValidTime(value: string): string {
  const instant = new Date(value);
  return [
    String(instant.getUTCFullYear()).padStart(4, "0"),
    String(instant.getUTCMonth() + 1).padStart(2, "0"),
    String(instant.getUTCDate()).padStart(2, "0"),
    String(instant.getUTCHours()).padStart(2, "0"),
    String(instant.getUTCMinutes()).padStart(2, "0"),
    String(instant.getUTCSeconds()).padStart(2, "0"),
  ].join("");
}

// format one site-local weather-map clock
function formatForecastMapTime(value: string, timezone: string): string {
  return new Intl.DateTimeFormat("en-US", {
    hour: "numeric",
    minute: "2-digit",
    timeZone: timezone,
    weekday: "short",
  }).format(new Date(value));
}

// render the reviewed forecast horizon control
function renderForecastRangeSelector(
  selected: ForecastDays,
  loading: boolean,
): string {
  const options: readonly Readonly<{ days: ForecastDays; label: string }>[] = [
    { days: 1, label: "Today" },
    { days: 5, label: "5 days" },
    { days: 10, label: "10 days" },
  ];
  return `
    <div class="range-selector forecast-range-selector" role="group" aria-label="Forecast range">
      ${options.map(
        // render each reviewed forecast horizon
        (option) => `<button type="button" data-forecast-days="${String(option.days)}" aria-pressed="${String(selected === option.days)}"${loading ? " disabled" : ""}>${option.label}</button>`,
      ).join("")}
    </div>
  `;
}

// reserve one complete forecast chart stack
function renderForecastSkeleton(state: DashboardState): string {
  const charts: readonly Readonly<{ icon: MaterialIconName; label: string }>[] = [
    { icon: "device_thermostat", label: "Temperature" },
    { icon: "air", label: "Wind" },
    { icon: "rainy", label: "Rain rate" },
    { icon: "cloud", label: "Clouds" },
    { icon: "humidity_percentage", label: "Humidity" },
    { icon: "masks", label: "Air quality" },
    { icon: "wb_sunny", label: "UV index" },
    { icon: "speed", label: "Pressure" },
    { icon: "water", label: "Tide" },
  ];
  const site = state.selectedSite ?? PRODUCT_SITE;
  const previewMs = Math.floor(Date.now() / (10 * 60 * 1_000)) * 10 * 60 * 1_000;
  const previewTime = new Date(previewMs).toISOString();
  const viewport = createCenteredMapViewport(
    site.latitude,
    site.longitude,
    FORECAST_MAP_WIDTH,
    FORECAST_MAP_HEIGHT,
    FORECAST_MAP_ZOOM,
  );
  const sitePoint = projectMapPoint(site.latitude, site.longitude, viewport);
  const previewLegend = forecastMapLegend("radar", "history");

  return `
    <section class="panel forecast-panel skeleton-region" aria-label="Loading weather forecast" aria-busy="true">
      <div class="forecast-chart-shell" aria-label="Loading weather forecast charts for today">
        <div class="forecast-chart-grid">
          ${charts.map(
            // preserve every final chart frame
            (chart) => `
              <article class="forecast-chart skeleton-forecast-chart" aria-label="${chart.label} forecast loading">
                <div class="forecast-chart-heading forecast-chart-heading-top"><h3>${renderMaterialIcon(chart.icon)}<span>${chart.label}</span></h3></div>
                <div class="forecast-chart-plot" aria-hidden="true"><svg viewBox="0 0 720 150" focusable="false"><rect class="skeleton-chart-fill" width="720" height="150" rx="8"/></svg></div>
              </article>
            `,
          ).join("")}
        </div>
        ${SHOW_FORECAST_WEATHER_MAP ? `<div class="forecast-weather-map skeleton-forecast-map" aria-busy="true">
          <div class="forecast-map-canvas" role="img" aria-label="Loading radar near ${escapeHtml(site.name)}">
            <svg class="forecast-map-svg" viewBox="0 0 ${FORECAST_MAP_WIDTH} ${FORECAST_MAP_HEIGHT}" focusable="false" aria-hidden="true">
              <g class="map-tile-layer">
                ${renderMapTiles("roads", viewport, FORECAST_MAP_WIDTH, FORECAST_MAP_HEIGHT)}
              </g>
              <g class="forecast-map-weather-layer">
                ${renderForecastWeatherFrame("radar", "history", previewTime, site, true)}
              </g>
              <g class="forecast-map-place-layer">
                <circle cx="${sitePoint.x.toFixed(2)}" cy="${sitePoint.y.toFixed(2)}" r="8" class="farm-marker"/>
                <text x="${(sitePoint.x + 13).toFixed(2)}" y="${(sitePoint.y + 4).toFixed(2)}">Ballydídean</text>
              </g>
            </svg>
            <div class="forecast-map-layer-controls" role="group" aria-label="Weather map layer loading">
              ${FORECAST_MAP_LAYERS.map(
                // preserve every final layer control
                (layer) => `<button type="button" aria-pressed="${String(layer.key === "radar")}" disabled>${renderMaterialIcon(layer.icon)}<span>${layer.label}</span></button>`,
              ).join("")}
            </div>
            <div class="forecast-map-selection-phase" data-forecast-map-selection-phase="history" aria-hidden="true"><span>Historical</span></div>
            <div class="forecast-map-legend" data-forecast-map-legend-layer="radar" data-forecast-map-legend-phase="history" role="img" aria-label="${previewLegend.title} color legend">
              ${renderForecastMapLegendContent(previewLegend)}
            </div>
          </div>
        </div>` : ""}
        <div class="forecast-x-axis" aria-hidden="true">
          ${Array.from({ length: 24 },
            // preserve every final hourly tick
            (_, index) => `<span class="forecast-x-tick" data-forecast-light="${index >= 7 && index < 19 ? "day" : "night"}">${index % 6 === 0 || index === 23 ? `<time>${formatForecastAxisHour(new Date(Date.UTC(2026, 0, 1, index)).toISOString(), "UTC")}</time>` : ""}</span>`,
          ).join("")}
        </div>
      </div>
    </section>
  `;
}

// build the current-condition forecast series
function buildForecastCharts(
  hours: readonly WeatherRecord[],
  tides: readonly TideRecord[],
  useAdjustments: boolean,
  records: readonly WeatherRecord[],
  pressureContext: readonly WeatherRecord[],
  observations: readonly ForecastObservationPoint[],
  now: number,
): readonly ForecastChartDefinition[] {
  const observed = new Map(observations.map((point) => [Date.parse(point.validAt), point]));
  // recorded past never inherits forecast corrections or the persisted adjustment mode
  const value = (record: WeatherRecord, key: WeatherMetricKey, adjusted = useAdjustments): number | null => Date.parse(record.validAt) < now
    ? observed.get(Date.parse(record.validAt))?.metrics[key] ?? forecastMetricValue(record, key, false)
    : forecastMetricValue(record, key, adjusted);
  // apply the selected adjustment mode only beyond the recorded past
  const metric = (key: WeatherMetricKey, adjusted = useAdjustments): readonly (number | null)[] => hours.map(
    // align every weather metric to the shared hourly index
    (record) => value(record, key, adjusted),
  );

  // any changed plotted hour qualifies even when the selected hour is unchanged
  const changed = (key: WeatherMetricKey): boolean => useAdjustments && metric(key).some(
    // recorded past and zero corrections compare equal to the same raw series
    (reading, index) => adjustmentValuesDiffer(reading, value(hours[index]!, key, false)),
  );
  // compare the displayed rate fallback rather than unused precipitation fields
  const rainRates = (adjusted: boolean): readonly (number | null)[] => hours.map(
    // preserve direct rate then hourly amount selection in both modes
    (record) => value(record, "precipitationRateMmPerHour", adjusted) ??
      forecastMetricValue(record, "precipitationMm", Date.parse(record.validAt) >= now && adjusted),
  );
  const selectedRain = rainRates(useAdjustments);
  const rawRain = rainRates(false);

  // recorded history remains a presentation overlay after shared forecast inference
  const pressure = (adjusted: boolean): readonly (number | null)[] => forecastPressureSeries(records, hours, pressureContext, adjusted).map(
    // historical recorded tendencies take precedence in both display modes
    (change, index) => Date.parse(hours[index]!.validAt) < now
      ? observed.get(Date.parse(hours[index]!.validAt))?.pressureChange3hHpa ?? change : change,
  );
  const pressureChanges = pressure(useAdjustments);
  const rawPressure = pressure(false);
  const pressureExtent = Math.max(6, ...pressureChanges.map(
    // center the signed scale on steady pressure without clipping rapid changes
    (value) => Math.ceil(Math.abs(value ?? 0)),
  ));

  return [
    {
      adjusted: changed("temperatureC") || changed("apparentTemperatureC"),
      domain: { maximum: 26.666_666_666_7, minimum: -1.111_111_111_1 },
      format: "temperature",
      icon: "device_thermostat",
      key: "temperature",
      label: "Temperature",
      series: [
        { label: "Feels like", values: metric("apparentTemperatureC") },
        // expose corrected air temperature without inventing an apparent-temperature model
        { label: "Air Temp", values: metric("temperatureC") },
      ],
    },
    {
      adjusted: changed("windSpeedMps") || changed("windGustMps"),
      domain: { maximum: 22.351_999_999_5, minimum: 0 },
      format: "windSpeed",
      icon: "air",
      key: "wind",
      label: "Wind",
      series: [
        { label: "Wind", values: metric("windSpeedMps") },
        { label: "Gust", values: metric("windGustMps") },
      ],
    },
    {
      adjusted: useAdjustments && selectedRain.some(
        // dry zeros and past-only corrections never color the rain icon
        (reading, index) => adjustmentValuesDiffer(reading, rawRain[index] ?? null),
      ),
      domain: { maximum: 25.4, minimum: 0 },
      format: "precipitationRate",
      icon: "rainy",
      key: "rain-rate",
      label: "Rain rate",
      series: [{
        label: "Rate",
        values: selectedRain,
      }],
    },
    {
      adjusted: changed("cloudCoverPercent"),
      domain: { maximum: 100, minimum: 0 },
      format: "cloudCover",
      icon: "cloud",
      key: "clouds",
      label: "Clouds",
      series: [{ label: "Cover", values: metric("cloudCoverPercent") }],
    },
    {
      adjusted: changed("relativeHumidityPercent"),
      format: "humidity",
      icon: "humidity_percentage",
      key: "humidity",
      label: "Humidity",
      series: [{ label: "Humidity", values: metric("relativeHumidityPercent") }],
      temperaturesC: metric("temperatureC"),
    },
    {
      adjusted: changed("pm25MicrogramsPerCubicMeter"),
      domain: HISTORICAL_FORECAST_DOMAINS.airQuality,
      format: "airQuality",
      icon: "masks",
      key: "air-quality",
      label: "Air quality",
      series: [{ label: "PM2.5", values: metric("pm25MicrogramsPerCubicMeter") }],
    },
    {
      adjusted: changed("uvIndex"),
      domain: FIXED_FORECAST_DOMAINS.uvIndex,
      format: "uvIndex",
      icon: "wb_sunny",
      key: "uv-index",
      label: "UV index",
      series: [{ label: "Index", values: metric("uvIndex") }],
    },
    {
      adjusted: useAdjustments && pressureChanges.some(
        // compare the displayed three-hour tendency rather than absolute pressure datums
        (reading, index) => adjustmentValuesDiffer(reading, rawPressure[index] ?? null),
      ),
      domain: { maximum: pressureExtent, minimum: -pressureExtent },
      format: "pressureChange",
      icon: "speed",
      key: "pressure",
      label: "Pressure",
      series: [{ label: "3h change", values: pressureChanges }],
    },
    {
      domain: FIXED_FORECAST_DOMAINS.tide,
      format: "waterLevel",
      icon: "water",
      key: "tide",
      label: "Tide",
      series: [{ label: "Level", values: forecastTideValues(hours, tides) }],
    },
  ];
}

// render one dependency-free synchronized SVG forecast chart
function renderForecastChart(
  chart: ForecastChartDefinition,
  selectedIndex: number,
  units: UnitPreferences,
  daylightBands: string,
  dayMarkers: string,
  days: ForecastDays,
): string {
  const width = 720;
  const height = 150;
  const edgePadding = 12;
  const headingPadding = 52;
  const domain = chart.domain ?? forecastChartDomain(chart.series);
  const scaleMaximum = formatForecastChartAxisValue(domain.maximum, chart.format, units);
  const scaleMinimum = formatForecastChartAxisValue(domain.minimum, chart.format, units);
  const valueEdge = forecastValueLabelEdge(
    chart.series.map(
      // read every initial selector intersection
      (series) => series.values[selectedIndex] ?? null,
    ),
    domain.minimum,
    domain.maximum,
  );
  const paddingBottom = edgePadding;
  const paddingTop = headingPadding;

  return `
    <article
      class="forecast-chart"
      data-forecast-chart="${escapeHtml(chart.key)}"
      data-forecast-format="${chart.format}"
      data-forecast-min="${String(domain.minimum)}"
      data-forecast-max="${String(domain.maximum)}"
      data-forecast-series="${escapeHtml(JSON.stringify(chart.series))}"
    >
      <div class="forecast-chart-heading forecast-chart-heading-top"><h3>${renderMaterialIcon(chart.icon, chart.adjusted)}<span>${escapeHtml(chart.label)}</span></h3></div>
      <div class="forecast-chart-plot">
        ${daylightBands}
        ${dayMarkers}
        <span class="forecast-chart-scale forecast-chart-scale-maximum">${escapeHtml(scaleMaximum)}</span>
        <span class="forecast-chart-scale forecast-chart-scale-minimum">${escapeHtml(scaleMinimum)}</span>
        <svg viewBox="0 0 ${width} ${height}" preserveAspectRatio="none" role="img" aria-label="${escapeHtml(chart.label)} forecast for ${days === 1 ? "today" : `${String(days)} days`}">
          <defs>
            ${chart.series.map(
              // color every line with the matching condition scale
              (series, seriesIndex) => renderForecastLineGradient(chart, series, seriesIndex, width),
            ).join("")}
          </defs>
          ${chart.series.map(
            // draw every available series
            (series, seriesIndex) => {
              const boundaryValues = series.values.length === 0
                ? []
                : [...series.values, series.values.at(-1) ?? null];
              const segments: string[][] = [[]];
              // preserve pressure gaps instead of joining unrelated complete windows
              for (const [index, value] of boundaryValues.entries()) {
                // retain the existing sparse display for other forecast metrics
                if (value === null || !Number.isFinite(value)) {
                  // start the next complete pressure window on a separate line
                  if (chart.format === "pressureChange" && segments.at(-1)!.length > 0) {
                    segments.push([]);
                  }
                  continue;
                }

                segments.at(-1)!.push(`${((index / Math.max(1, boundaryValues.length - 1)) * width).toFixed(2)},${forecastChartY(value, domain.minimum, domain.maximum, height, paddingTop, paddingBottom).toFixed(2)}`);
              }
              return segments.map(
                // render each uninterrupted forecast segment
                (points) => `<polyline points="${points.join(" ")}" class="forecast-chart-line forecast-chart-line-${String(seriesIndex)}" stroke="url(#${forecastLineGradientId(chart.key, seriesIndex)})"/>`,
              ).join("");
            },
          ).join("")}
        </svg>
        <output class="forecast-chart-value forecast-chart-value-${valueEdge}" data-forecast-value-edge="${valueEdge}" aria-live="off">
          ${chart.series.map(
            // show every value intersecting the shared line
            (series, seriesIndex) => `<span><small>${escapeHtml(series.label)}</small><strong data-forecast-value="${String(seriesIndex)}">${escapeHtml(compactMeasurement(formatForecastChartValue(series.values[selectedIndex] ?? null, chart.format, units)) ?? "—")}</strong></span>`,
          ).join("")}
        </output>
      </div>
    </article>
  `;
}

// place one value pill opposite its line intersections
function forecastValueLabelEdge(
  values: readonly (number | null)[],
  minimum: number,
  maximum: number,
): "bottom" | "top" {
  const positions = values.flatMap(
    // normalize every available selector intersection
    (value) => value === null || !Number.isFinite(value)
      ? []
      : [Math.max(0, Math.min(1, (value - minimum) / (maximum - minimum)))],
  );

  // retain the familiar lower edge without data
  if (positions.length === 0) {
    return "bottom";
  }

  const average = positions.reduce(
    // combine multi-line selector intersections
    (total, position) => total + position,
    0,
  ) / positions.length;
  return average >= 0.5 ? "bottom" : "top";
}

// use one population deviation from the normalized local archives
const HISTORICAL_FORECAST_DOMAINS = {
  airQuality: { maximum: 14.488_261_472_4, minimum: 0 },
} as const;

// retain the requested consumer chart scales in canonical units
const FIXED_FORECAST_DOMAINS = {
  tide: { maximum: 3.6576, minimum: -0.3048 },
  uvIndex: { maximum: 4, minimum: 0 },
} as const;

// render one CSP-safe condition-color gradient
function renderForecastLineGradient(
  chart: ForecastChartDefinition,
  series: ForecastChartSeries,
  seriesIndex: number,
  width: number,
): string {
  const boundaryValues = series.values.length === 0
    ? []
    : [...series.values, series.values.at(-1) ?? null];
  const denominator = Math.max(1, boundaryValues.length - 1);
  return `
    <linearGradient id="${forecastLineGradientId(chart.key, seriesIndex)}" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="${String(width)}" y2="0">
      ${boundaryValues.map(
        // align every color stop with its forecast hour
        (value, index) => {
          const temperatureC = chart.temperaturesC?.[Math.min(index, series.values.length - 1)] ?? null;
          const color = forecastLineColor(chart.format, value, index, boundaryValues, temperatureC);
          return `<stop offset="${((index / denominator) * 100).toFixed(3)}%" stop-color="${escapeHtml(color)}"/>`;
        },
      ).join("")}
    </linearGradient>
  `;
}

// create one document-safe line gradient identifier
function forecastLineGradientId(chartKey: string, seriesIndex: number): string {
  return `forecast-line-${chartKey}-${String(seriesIndex)}`;
}

// match each plotted value to its current-condition color
function forecastLineColor(
  format: ForecastChartFormat,
  value: number | null,
  index: number,
  values: readonly (number | null)[],
  temperatureC: number | null,
): string {
  switch (format) {
    case "temperature":
      return temperatureBand(value).color;
    case "windSpeed":
      return windBand(value, value).color;
    case "precipitationRate":
      return rainBand(value).color;
    case "airQuality":
      return airQualityBand(value).color;
    case "uvIndex":
      return uvBand(value).color;
    case "pressureChange":
      return pressureChangeBand(value).color;
    case "cloudCover":
      return cloudBand(value).color;
    case "humidity":
      return humidityBand(value, temperatureC).color;
    case "waterLevel": {
      const previous = values[index - 1];

      // show the same rise-and-fall colors as the tide card
      if (value === null || previous === undefined || previous === null) {
        return unavailableBand("Tide forecast unavailable").color;
      }

      return value >= previous ? "rgb(56, 120, 197)" : "rgb(124, 81, 116)";
    }
  }
}

// render one shared forecast time axis
function renderForecastXAxis(
  hours: readonly WeatherRecord[],
  timezone?: string,
  days: ForecastDays = 1,
): string {
  const finalIndex = Math.max(0, hours.length - 1);
  const labelIndexes = forecastAxisLabelIndexes(hours, timezone, days);
  return `<div class="forecast-x-axis" aria-label="Forecast hourly time axis with day and night shading">${hours.map(
    // render every static hourly tick
    (hour, index) => {
      const showLabel = days === 1
        ? index % 6 === 0 || index === finalIndex
        : labelIndexes.has(index);
      const light = forecastDaylightState(hour, timezone) ? "day" : "night";
      return `<span class="forecast-x-tick" data-forecast-light="${light}"${showLabel ? ' data-major="true"' : ""}>${showLabel ? `<time datetime="${escapeHtml(hour.validAt)}">${days === 1 ? formatForecastAxisHour(hour.validAt, timezone) : formatForecastAxisDate(hour.validAt, timezone)}</time>` : ""}</span>`;
    },
  ).join("")}</div>`;
}

// choose readable local-date labels for extended ranges
function forecastAxisLabelIndexes(
  hours: readonly WeatherRecord[],
  timezone: string | undefined,
  days: ForecastDays,
): ReadonlySet<number> {
  const labels = new Set<number>();
  const seenDates = new Set<string>();
  let dateIndex = 0;

  // visit the first hour from each site-local date
  for (const [index, hour] of hours.entries()) {
    const date = forecastSiteDateKey(hour.validAt, timezone ?? "UTC");

    // skip repeated hours inside one date
    if (seenDates.has(date)) {
      continue;
    }

    seenDates.add(date);

    // label every date for five days and alternating dates for ten
    if (days === 5 || dateIndex % 2 === 0) {
      labels.add(index);
    }

    dateIndex += 1;
  }

  // retain the final visible endpoint
  if (hours.length > 0) {
    labels.add(hours.length - 1);
  }

  return labels;
}

// render CSP-safe hourly daylight bands
function renderForecastDaylightBands(states: readonly boolean[]): string {
  return `<div class="forecast-chart-daylight" aria-hidden="true">${states.map(
    // preserve one equal-width band per forecast hour
    (daylight) => `<span data-forecast-light="${daylight ? "day" : "night"}"></span>`,
  ).join("")}</div>`;
}

// render site-local midnight dividers and day panels for extended ranges
function renderForecastDayMarkers(
  hours: readonly WeatherRecord[],
  timezone: string | undefined,
  days: ForecastDays,
): string {
  // keep today's compact chart free of redundant day dividers
  if (days === 1) {
    return "";
  }

  return `<div class="forecast-chart-days" aria-hidden="true">${hours.map(
    // align every divider to its exact hourly cell
    (hour, index) => {
      const date = forecastSiteDateKey(hour.validAt, timezone ?? "UTC");
      const previousDate = index === 0
        ? null
        : forecastSiteDateKey(hours[index - 1]?.validAt ?? hour.validAt, timezone ?? "UTC");
      const startsDay = date !== previousDate;
      return startsDay
        ? `<span class="forecast-chart-day-start" data-forecast-day="${date}"><b>${escapeHtml(formatForecastDayPanel(hour.validAt, timezone))}</b></span>`
        : "<span></span>";
    },
  ).join("")}</div>`;
}

// classify one modeled hour as daylight
function forecastDaylightState(
  hour: WeatherRecord,
  timezone?: string,
): boolean {
  // prefer modeled surface sunlight
  if (hour.metrics.solarRadiationWm2 !== null) {
    return hour.metrics.solarRadiationWm2 > 0;
  }

  // use modeled ultraviolet light when radiation is unavailable
  if (hour.metrics.uvIndex !== null) {
    return hour.metrics.uvIndex > 0;
  }

  const localHour = Number(new Intl.DateTimeFormat("en-US", {
    hour: "numeric",
    hourCycle: "h23",
    timeZone: timezone ?? "UTC",
  }).format(new Date(hour.validAt)));
  return localHour >= 7 && localHour < 19;
}

// locate one instant continuously on the shared hourly axis
function forecastPositionForInstant(
  hours: readonly WeatherRecord[],
  reference?: string,
): number {
  // start at the first hour without a usable clock
  if (hours.length === 0 || reference === undefined) {
    return 0;
  }

  const target = new Date(reference).getTime();
  const first = new Date(hours[0]?.validAt ?? reference).getTime();

  // clamp times before the forecast day
  if (target <= first) {
    return 0;
  }

  // locate the surrounding hourly pair
  for (let index = 1; index < hours.length; index += 1) {
    const previous = new Date(hours[index - 1]?.validAt ?? reference).getTime();
    const next = new Date(hours[index]?.validAt ?? reference).getTime();

    // interpolate inside the located interval
    if (target <= next) {
      return index - 1 + (target - previous) / Math.max(1, next - previous);
    }
  }

  return Math.max(0, hours.length - 1);
}

// derive a padded chart range across every visible line
function forecastChartDomain(
  series: readonly ForecastChartSeries[],
): Readonly<{ maximum: number; minimum: number }> {
  const values = series.flatMap(
    // retain finite chart values only
    (line) => line.values.filter((value): value is number => value !== null && Number.isFinite(value)),
  );

  // provide a stable empty chart range
  if (values.length === 0) {
    return { maximum: 1, minimum: 0 };
  }

  const minimum = Math.min(...values);
  const maximum = Math.max(...values);

  // expand a flat series visibly
  if (minimum === maximum) {
    const padding = Math.max(1, Math.abs(minimum) * 0.05);
    return { maximum: maximum + padding, minimum: minimum - padding };
  }

  const padding = (maximum - minimum) * 0.08;
  return { maximum: maximum + padding, minimum: minimum - padding };
}

// map one chart value into the SVG plot height
function forecastChartY(
  value: number,
  minimum: number,
  maximum: number,
  height: number,
  paddingTop: number,
  paddingBottom: number,
): number {
  const position = Math.max(0, Math.min(1, (value - minimum) / (maximum - minimum)));
  return height - paddingBottom - position * (height - paddingTop - paddingBottom);
}

// format one forecast chart value in the browser preference
function formatForecastChartValue(
  value: number | null,
  format: ForecastChartFormat,
  units: UnitPreferences,
): FormattedMeasurement {
  // preserve unavailable intersections
  if (value === null || !Number.isFinite(value)) {
    return { unit: "", value: "—" };
  }

  switch (format) {
    case "temperature":
      return formatMeasurement(value, "temperature", units, 0);
    case "windSpeed":
      return formatMeasurement(value, "windSpeed", units, 0);
    case "precipitationRate":
      return formatPrecipitationRate(value, units);
    case "airQuality":
      return formatFixedMeasurement(value, "", 0);
    case "uvIndex":
      return formatFixedMeasurement(value, "", 1);
    case "pressureChange":
      return formatPressureChange(value);
    case "cloudCover":
    case "humidity":
      return formatFixedMeasurement(value, "%", 0);
    case "waterLevel":
      return formatMeasurement(value, "waterLevel", units, 1);
  }
}

// format one compact fixed-domain endpoint
function formatForecastChartAxisValue(
  value: number,
  format: ForecastChartFormat,
  units: UnitPreferences,
): string {
  const measurement = formatForecastChartValue(value, format, units);
  return compactMeasurement(measurement) ?? "—";
}

// interpolate NOAA high and low tide events onto the hourly forecast clock
function forecastTideValues(
  hours: readonly WeatherRecord[],
  tides: readonly TideRecord[],
): readonly (number | null)[] {
  const predictions = tides.filter(
    // retain reviewed high and low events only
    (record) => record.kind === "prediction" && record.eventType !== null,
  ).toSorted((left, right) => new Date(left.validAt).getTime() - new Date(right.validAt).getTime());
  return hours.map(
    // interpolate one hourly level between adjacent extrema
    (hour) => interpolateTidePrediction(new Date(hour.validAt).getTime(), predictions),
  );
}

// interpolate one smooth level between adjacent NOAA tide extrema
function interpolateTidePrediction(
  instant: number,
  predictions: readonly TideRecord[],
): number | null {
  let previous: TideRecord | undefined;

  // locate the adjacent prediction pair
  for (const prediction of predictions) {
    const predictionInstant = new Date(prediction.validAt).getTime();

    // return an exact NOAA event
    if (predictionInstant === instant) {
      return prediction.waterLevelM;
    }

    // interpolate once the upper event is found
    if (predictionInstant > instant) {
      // require a complete pair
      if (previous === undefined) {
        return null;
      }

      const previousInstant = new Date(previous.validAt).getTime();
      const position = (instant - previousInstant) / (predictionInstant - previousInstant);
      const easedPosition = (1 - Math.cos(Math.PI * position)) / 2;
      return previous.waterLevelM + (prediction.waterLevelM - previous.waterLevelM) * easedPosition;
    }

    previous = prediction;
  }

  return null;
}

interface TrendCalendarSample {
  readonly dayKey: string;
  readonly value: number;
  readonly x: number;
}

interface TrendYearSeries {
  readonly points: readonly TrendCalendarSample[];
  readonly year: number;
}

interface TrendAggregateSample {
  readonly dayKey: string;
  readonly lowerQuartile: number;
  readonly maximum: number;
  readonly median: number;
  readonly minimum: number;
  readonly upperQuartile: number;
  readonly x: number;
}

interface TrendCrosshairSeries {
  readonly colorClass: string;
  readonly key: string;
  readonly label: string;
  readonly points: readonly TrendCalendarSample[];
}

interface TrendWindRoseSector {
  readonly currentPercent: number;
  readonly historicalPercent: number;
  readonly label: string;
}

const TREND_CHART_HEIGHT = 280;
const TREND_CHART_PADDING_BOTTOM = 34;
const TREND_CHART_PADDING_LEFT = 42;
const TREND_CHART_PADDING_RIGHT = 18;
const TREND_CHART_PADDING_TOP = 42;
const TREND_CHART_WIDTH = 720;
const TREND_ROLLING_WINDOW_DAYS = 7;
const TREND_MONTH_LABELS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"] as const;
const WIND_ROSE_DIRECTIONS = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"] as const;
const TREND_CURRENT_YEAR_COLOR = "var(--brand-orange)";
const TREND_YEAR_COLORS = ["#3878c5", "#439756", "#e6b519", "#ef7e1f", "#cf4337", "#8d6e63", "#545450", "#00838f"] as const;
type TrendChartFormat = keyof UnitPreferences | "count" | "degreeDays" | "humidity" | "temperatureDelta";
interface TrendChartOption {
  readonly format: TrendChartFormat;
  readonly group: "Farm insights" | "Measurements";
  readonly includeZero?: boolean;
  readonly kind?: "line" | "windRose";
  readonly label: string;
  readonly maximum?: number;
  readonly metric: TrendChartMetric;
  readonly minimum?: number;
  readonly supportsDetail?: boolean;
}
const TREND_CHART_OPTIONS: readonly TrendChartOption[] = [
  { format: "temperature", group: "Measurements", label: "Temperature", metric: "temperatureC" },
  { format: "temperature", group: "Measurements", label: "Feels like", metric: "apparentTemperatureC" },
  { format: "windSpeed", group: "Measurements", label: "Wind speed", metric: "windSpeedMps", minimum: 0 },
  { format: "windSpeed", group: "Measurements", label: "Wind gust", metric: "windGustMps", minimum: 0 },
  { format: "precipitation", group: "Measurements", label: "Daily rain", metric: "precipitationMm", minimum: 0 },
  { format: "humidity", group: "Measurements", label: "Humidity", maximum: 100, metric: "relativeHumidityPercent", minimum: 0 },
  { format: "pressure", group: "Measurements", label: "Pressure", metric: "pressureHpa" },
  { format: "precipitation", group: "Farm insights", label: "Cumulative rainfall", metric: "cumulativePrecipitationMm", minimum: 0, supportsDetail: false },
  { format: "temperatureDelta", group: "Farm insights", includeZero: true, label: "Temperature anomaly", metric: "temperatureAnomalyC" },
  { format: "temperatureDelta", group: "Farm insights", label: "Daily temperature range", metric: "temperatureRangeC", minimum: 0 },
  { format: "count", group: "Farm insights", label: "Dry spell length", metric: "drySpellDays", minimum: 0, supportsDetail: false },
  { format: "degreeDays", group: "Farm insights", label: "Accumulated growing heat", metric: "growingDegreeDaysC", minimum: 0, supportsDetail: false },
  { format: "count", group: "Farm insights", label: "Frost days", metric: "frostDayCount", minimum: 0, supportsDetail: false },
  { format: "count", group: "Farm insights", label: "Extreme day counts", metric: "extremeDayCount", minimum: 0, supportsDetail: false },
  { format: "count", group: "Farm insights", kind: "windRose", label: "Wind direction rose", metric: "windDirectionRose", supportsDetail: false },
];

interface TrendExtremeConfiguration {
  readonly comparator: "atLeast" | "atMost";
  readonly defaultThreshold: number;
  readonly format: "precipitation" | "temperature" | "windSpeed";
  readonly label: string;
  readonly maximum: number;
  readonly metric: "precipitationMm" | "temperatureMaximumC" | "temperatureMinimumC" | "windGustMps";
  readonly minimum: number;
}

const TREND_EXTREME_CONFIGURATIONS: Readonly<Record<TrendExtremeKind, TrendExtremeConfiguration>> = {
  cold: { comparator: "atMost", defaultThreshold: -5, format: "temperature", label: "Cold", maximum: 25, metric: "temperatureMinimumC", minimum: -60 },
  heat: { comparator: "atLeast", defaultThreshold: 30, format: "temperature", label: "Heat", maximum: 60, metric: "temperatureMaximumC", minimum: -10 },
  rain: { comparator: "atLeast", defaultThreshold: 25.4, format: "precipitation", label: "Heavy rain", maximum: 500, metric: "precipitationMm", minimum: 0 },
  wind: { comparator: "atLeast", defaultThreshold: 17.881_6, format: "windSpeed", label: "High wind", maximum: 100, metric: "windGustMps", minimum: 0 },
};

// resolve one reviewed extreme-day configuration
function trendExtremeConfiguration(kind: TrendExtremeKind): TrendExtremeConfiguration {
  return TREND_EXTREME_CONFIGURATIONS[kind];
}

// define the destination-state toggle glyphs
const TREND_TOGGLE_ICON_PATHS = {
  aggregate: '<path class="trend-toggle-icon-fill" d="m1.5 5.5 3-2.3 2.7 1.9 3.2-2.3 6.1 3v6.4l-6.1-1.7L7.2 13l-2.7-2.2-3 1.7Z"/><path d="m1.5 9 3-2 2.7 2 3.2-2.5 6.1 2.5"/>',
  daily: '<path d="M2 12V8m2.3 4V4.5m2.3 7.5V7m2.4 5V3m2.4 9V6m2.3 6V5m2.3 7V8.5"/>',
  rolling: '<path class="trend-toggle-icon-muted" d="M2 12V8m2.3 4V4.5m2.3 7.5V7m2.4 5V3m2.4 9V6m2.3 6V5m2.3 7V8.5"/><path d="M1.5 10.5c2-2.7 3.6-2.9 5.3-2.1 2.1 1 3.2.6 4.7-1.2 1.4-1.7 3-1.8 5-.2"/>',
  "show-all": '<path d="m1.5 5.1 3-1.9 2.7 2 3.2-2.9 6.1 2.9M1.5 9l3-1.9 2.7 2 3.2-2.9 6.1 2.9M1.5 12.9l3-1.9 2.7 2 3.2-2.9 6.1 2.9"/>',
} as const;
type TrendToggleIcon = keyof typeof TREND_TOGGLE_ICON_PATHS;

// render calendar-year comparison charts
function renderTrends(state: DashboardState): string {
  // reserve the selected chart until its data read settles
  if (state.loading) {
    return renderTrendsSkeleton();
  }

  const selected = TREND_CHART_OPTIONS.find(
    // resolve the selected chart configuration
    (option) => option.metric === state.selectedTrendMetric,
  ) ?? TREND_CHART_OPTIONS[0]!;

  return `
    <section class="panel trends-panel" aria-label="Trends">
      ${state.trends.length === 0
        ? '<p class="empty-panel">No normalized calendar-year trend buckets are available yet.</p>'
        : `<div class="trend-grid">
            ${selected.kind === "windRose"
              ? renderTrendWindRose(state, selected)
              : renderTrendLineChart(state, selected)}
          </div>`}
    </section>
  `;
}

// reserve the loaded chart's responsive frame during loading
function renderTrendsSkeleton(): string {
  return `
    <section class="panel trends-panel skeleton-region" aria-label="Trends" aria-busy="true">
      <div class="trend-grid">
        <article class="trend-chart skeleton-trend-chart" aria-hidden="true">
          <div class="trend-chart-frame">
            <div class="trend-chart-viewport">
              <div class="trend-chart-landscape">
                ${renderTrendMetricControl("temperatureC", true)}
                <span class="trend-chart-range">00–00 unit</span>
                <svg viewBox="0 0 ${TREND_CHART_WIDTH} ${TREND_CHART_HEIGHT}" preserveAspectRatio="none" focusable="false">
                  ${renderTrendMonthGrid()}
                  <rect class="skeleton-chart-fill" x="${TREND_CHART_PADDING_LEFT}" y="${TREND_CHART_PADDING_TOP}" width="${TREND_CHART_WIDTH - TREND_CHART_PADDING_LEFT - TREND_CHART_PADDING_RIGHT}" height="${TREND_CHART_HEIGHT - TREND_CHART_PADDING_TOP - TREND_CHART_PADDING_BOTTOM}" rx="3"/>
                </svg>
                ${renderTrendMonthAxis()}
                <div class="trend-chart-legend skeleton-trend-legend"><span>25th–75th</span><span>Historical median</span></div>
              </div>
            </div>
          </div>
        </article>
      </div>
    </section>
  `;
}

// render daily prevailing-wind frequency as a polar rose
function renderTrendWindRose(
  state: DashboardState,
  option: TrendChartOption,
): string {
  const sectors = buildTrendWindRoseSectors(state);
  const currentYear = trendCurrentYear(state, []);
  const maximumPercent = Math.max(
    1,
    ...sectors.flatMap(
      // compare both visible distributions on one radial scale
      (sector) => [sector.currentPercent, sector.historicalPercent],
    ),
  );
  const populated = sectors.some(
    // require at least one daily direction sample
    (sector) => sector.currentPercent > 0 || sector.historicalPercent > 0,
  );

  // render a per-series empty state
  if (!populated) {
    return `<article class="trend-chart">${renderTrendMetricControl(option.metric)}<p>No wind direction values</p></article>`;
  }

  return `
    <article class="trend-chart trend-wind-rose-chart" data-trend-chart="${escapeHtml(option.metric)}" data-trend-detail="rolling" data-trend-display-mode="aggregate">
      <div class="trend-chart-frame">
        <div class="trend-chart-viewport">
          <div class="trend-chart-landscape trend-wind-rose-landscape">
            ${renderTrendMetricControl(option.metric)}
            <p class="trend-wind-rose-caption">Share of daily prevailing winds</p>
            <svg class="trend-wind-rose" viewBox="0 0 320 320" preserveAspectRatio="xMidYMid meet" role="img" aria-label="Wind direction rose comparing historical days with ${String(currentYear)}">
              <circle cx="160" cy="160" r="35" class="trend-wind-rose-grid"/>
              <circle cx="160" cy="160" r="70" class="trend-wind-rose-grid"/>
              <circle cx="160" cy="160" r="105" class="trend-wind-rose-grid"/>
              ${[0, 2, 4, 6, 8, 10, 12, 14].map(
                // draw one directional guide
                (index) => {
                  const edge = trendPolarPoint(160, 160, 112, index * 22.5 - 90);
                  return `<line x1="160" y1="160" x2="${edge.x.toFixed(2)}" y2="${edge.y.toFixed(2)}" class="trend-wind-rose-axis"/>`;
                },
              ).join("")}
              ${sectors.map(
                // draw one historical direction sector
                (sector, index) => renderTrendWindRoseSector(
                  index,
                  sector.historicalPercent,
                  maximumPercent,
                  "trend-wind-rose-sector-historical",
                ),
              ).join("")}
              ${sectors.map(
                // draw one current-year direction sector
                (sector, index) => renderTrendWindRoseSector(
                  index,
                  sector.currentPercent,
                  maximumPercent,
                  "trend-wind-rose-sector-current",
                ),
              ).join("")}
              ${[0, 2, 4, 6, 8, 10, 12, 14].map(
                // label the eight principal compass points
                (index) => {
                  const label = trendPolarPoint(160, 160, 132, index * 22.5 - 90);
                  return `<text x="${label.x.toFixed(2)}" y="${label.y.toFixed(2)}" class="trend-wind-rose-label">${WIND_ROSE_DIRECTIONS[index]}</text>`;
                },
              ).join("")}
            </svg>
            <div class="trend-chart-legend trend-wind-rose-legend" aria-label="Wind rose legend">
              <span class="trend-wind-rose-historical"><i aria-hidden="true"></i>Historical</span>
              <span class="trend-wind-rose-current"><i aria-hidden="true"></i>${String(currentYear)}</span>
            </div>
            <ol class="sr-only" aria-label="Wind direction percentages">
              ${sectors.map(
                // expose every polar sector to assistive technology
                (sector) => `<li>${sector.label}: historical ${sector.historicalPercent.toFixed(1)}%, ${String(currentYear)} ${sector.currentPercent.toFixed(1)}%</li>`,
              ).join("")}
            </ol>
          </div>
        </div>
      </div>
    </article>
  `;
}

// render one dependency-free accessible SVG trend chart
function renderTrendLineChart(
  state: DashboardState,
  option: TrendChartOption,
): string {
  const rawSeries = buildTrendChartYearSeries(state, option);

  // render a per-series empty state
  if (rawSeries.length === 0) {
    return `<article class="trend-chart">${renderTrendMetricControl(option.metric)}<p>No values</p></article>`;
  }

  const currentYear = trendCurrentYear(state, rawSeries);
  const supportsDetail = option.supportsDetail !== false;
  const renderedDetail: TrendDetail = supportsDetail ? state.trendDetail : "rolling";
  const series = renderedDetail === "rolling" && supportsDetail
    ? smoothTrendYearSeries(rawSeries, TREND_ROLLING_WINDOW_DAYS)
    : rawSeries;
  const historicalSeries = series.filter(
    // exclude the incomplete current year from historical statistics
    (year) => year.year !== currentYear,
  );
  const aggregate = buildTrendAggregateSeries(
    // retain a useful fallback before a second calendar year exists
    historicalSeries.length === 0 ? series : historicalSeries,
  );
  const selectedTrendYear = state.trendDisplayMode === "all" ? state.selectedTrendYear : null;
  const { maximum, minimum } = trendVisibleDomain(
    series,
    aggregate,
    state.trendDisplayMode,
    selectedTrendYear,
    currentYear,
    option,
    state.units,
  );
  const span = maximum === minimum ? 1 : maximum - minimum;
  const minimumLabel = formatTrendMeasurement(minimum, option.format, state.units);
  const maximumLabel = formatTrendMeasurement(maximum, option.format, state.units);
  const aggregateBandPath = renderTrendAggregateBandPath(aggregate, minimum, span);
  const aggregateMaximum = renderTrendAggregateLinePoints(aggregate, "maximum", minimum, span);
  const aggregateMedian = renderTrendAggregateLinePoints(aggregate, "median", minimum, span);
  const aggregateMinimum = renderTrendAggregateLinePoints(aggregate, "minimum", minimum, span);
  const initialPosition = trendInitialPosition(state, rawSeries);
  const todayPosition = trendTodayPosition(state);
  const indexedSeries = series.map(
    // retain each stable year color while changing its SVG stacking order
    (year, index) => ({ index, year }),
  );
  const selectedSeries = indexedSeries.find(
    // locate the emphasized calendar year
    (entry) => entry.year.year === selectedTrendYear,
  );
  const currentSeries = indexedSeries.find(
    // locate the permanent current-year comparison
    (entry) => entry.year.year === currentYear,
  );
  const drawableSeries = selectedSeries === undefined
    ? indexedSeries
    : [
        ...indexedSeries.filter(
          // draw every muted year below the emphasized line
          (entry) => entry.year.year !== selectedSeries.year.year,
        ),
        selectedSeries,
      ];
  const crosshairSeries = buildTrendCrosshairSeries(
    series,
    aggregate,
    state.trendDisplayMode,
    selectedTrendYear,
    currentYear,
  );
  const detailLabel = supportsDetail
    ? renderedDetail === "daily" ? "Daily values" : "7-day average"
    : "annual progression";

  // move non-data chart chrome outside the daily scrollport
  const dailyDetail = renderedDetail === "daily";
  const metricControl = renderTrendMetricControl(option.metric);
  const chartRange = `<span class="trend-chart-range">${escapeHtml(minimumLabel.value)}–${escapeHtml(maximumLabel.value)} ${escapeHtml(maximumLabel.unit)}</span>`;
  // show the destination display mode
  const modeToggleContent = state.trendDisplayMode === "aggregate"
    ? { icon: "show-all", label: "Show all" } as const
    : { icon: "aggregate", label: "Aggregate" } as const;
  // show the destination detail level
  const detailToggleContent = dailyDetail
    ? { icon: "rolling", label: "7-day average" } as const
    : { icon: "daily", label: "Daily detail" } as const;
  const modeToggle = `<button type="button" class="trend-mode-toggle" data-trend-mode-toggle aria-pressed="${state.trendDisplayMode === "all" ? "true" : "false"}">${renderTrendToggleIcon(modeToggleContent.icon)}<span>${modeToggleContent.label}</span></button>`;
  const detailToggle = supportsDetail
    ? `<button type="button" class="trend-detail-toggle" data-trend-detail-toggle aria-pressed="${dailyDetail ? "true" : "false"}">${renderTrendToggleIcon(detailToggleContent.icon)}<span>${detailToggleContent.label}</span></button>`
    : "";
  const insightControls = renderTrendInsightControls(state, option);
  const yAxis = renderTrendYAxis(minimum, maximum, option.format, state.units);

  // share one interactive legend between both layouts
  const legend = `
    <div class="trend-chart-legend" aria-label="Trend legend">
      <span class="trend-legend-quartiles"><i aria-hidden="true"></i>25th–75th</span>
      <span class="trend-legend-range"><i aria-hidden="true"></i>Historical min/max</span>
      ${state.trendDisplayMode === "aggregate"
        ? `<span class="trend-legend-median"><i aria-hidden="true"></i>Historical median</span>${currentSeries === undefined ? "" : `<span class="trend-legend-current"><i class="trend-current-year-color" aria-hidden="true"></i>${String(currentYear)}</span>`}`
        : series.map(
            // label every clickable overlaid calendar year
            (year, index) => `<button type="button" class="trend-year-legend${year.year === currentYear ? " trend-legend-current" : ""}${year.year === selectedTrendYear ? " trend-year-legend-selected" : ""}" data-trend-year-select="${String(year.year)}" aria-pressed="${year.year === selectedTrendYear ? "true" : "false"}"><i class="${year.year === currentYear ? "trend-current-year-color" : `trend-year-color-${String(index % TREND_YEAR_COLORS.length)}`}" aria-hidden="true" data-trend-legend-year="${String(year.year)}"></i>${String(year.year)}</button>`,
          ).join("")}
    </div>
  `;

  // preserve every non-scrolling daily control in one overlay
  const fixedChrome = `
    ${metricControl}
    ${chartRange}
    ${modeToggle}
    ${yAxis}
    ${detailToggle}
    ${insightControls}
    ${legend}
  `;

  return `
    <article class="trend-chart" data-trend-chart="${escapeHtml(option.metric)}" data-trend-detail="${renderedDetail}" data-trend-display-mode="${state.trendDisplayMode}" data-trend-maximum="${maximum.toFixed(6)}" data-trend-minimum="${minimum.toFixed(6)}" data-trend-domain="visible"${selectedSeries === undefined ? "" : ` data-selected-trend-year="${String(selectedSeries.year.year)}"`}>
      <div class="trend-chart-frame">
        <div class="trend-chart-viewport">
          <div class="trend-chart-landscape" data-trend-scrub-surface data-trend-initial-position="${initialPosition.toFixed(12)}" data-trend-today-position="${todayPosition.toFixed(12)}">
            ${dailyDetail ? "" : metricControl}
            ${dailyDetail ? "" : chartRange}
            ${dailyDetail ? "" : modeToggle}
            ${dailyDetail ? "" : insightControls}
            <svg viewBox="0 0 ${TREND_CHART_WIDTH} ${TREND_CHART_HEIGHT}" preserveAspectRatio="none" role="img" aria-label="${escapeHtml(option.label)} ${escapeHtml(detailLabel)} with historical median, quartiles, and range">
              ${renderTrendMonthGrid()}
              ${aggregateBandPath.length === 0 ? "" : `<path d="${aggregateBandPath}" class="trend-historical-quartile-band"/>`}
              <polyline points="${aggregateMinimum}" class="trend-historical-range-line" data-trend-historical-range="minimum"/>
              <polyline points="${aggregateMaximum}" class="trend-historical-range-line" data-trend-historical-range="maximum"/>
              ${state.trendDisplayMode === "aggregate"
                ? `<polyline points="${aggregateMedian}" class="trend-aggregate-median-line"/>${currentSeries === undefined ? "" : `<polyline points="${renderTrendLinePoints(currentSeries.year.points, minimum, span)}" class="trend-year-line trend-year-line-current" data-trend-year="${String(currentYear)}" stroke="${TREND_CURRENT_YEAR_COLOR}" aria-label="${String(currentYear)} ${escapeHtml(option.label)}"/>`}`
                : drawableSeries.map(
                    // render one forgiving tap target beneath its visible yearly line
                    ({ index, year }) => {
                      const points = renderTrendLinePoints(year.points, minimum, span);
                      const selected = year.year === selectedTrendYear;
                      const current = year.year === currentYear;
                      return `<polyline points="${points}" class="trend-year-hit-target${current ? " trend-year-hit-target-current" : ""}${selected ? " trend-year-hit-target-selected" : ""}" data-trend-year-select="${String(year.year)}" stroke="transparent" aria-hidden="true"/><polyline points="${points}" class="trend-year-line${current ? " trend-year-line-current" : ""}${selected ? " trend-year-line-selected" : ""}" data-trend-year="${String(year.year)}" data-trend-year-select="${String(year.year)}" stroke="${current ? TREND_CURRENT_YEAR_COLOR : trendYearColor(index)}" role="button" tabindex="0" aria-label="${String(year.year)} ${escapeHtml(option.label)}" aria-pressed="${selected ? "true" : "false"}"/>`;
                    },
                  ).join("")}
            </svg>
            <div class="trend-today-marker" aria-hidden="true"><span>Today</span></div>
            ${dailyDetail ? "" : yAxis}
            ${renderTrendMonthAxis()}
            ${renderTrendCrosshair(crosshairSeries, initialPosition, option, state.units)}
            ${dailyDetail ? "" : detailToggle}
            ${dailyDetail ? "" : legend}
          </div>
        </div>
        ${dailyDetail ? `<div class="trend-chart-fixed-chrome">${fixedChrome}</div>` : ""}
      </div>
    </article>
  `;
}

// render one decorative trend toggle glyph
function renderTrendToggleIcon(icon: TrendToggleIcon): string {
  return `<svg class="trend-toggle-icon" data-trend-toggle-icon="${icon}" viewBox="0 0 18 16" aria-hidden="true" focusable="false">${TREND_TOGGLE_ICON_PATHS[icon]}</svg>`;
}

// render responsive vertical scale labels outside the stretched SVG
function renderTrendYAxis(
  minimum: number,
  maximum: number,
  format: TrendChartFormat,
  units: UnitPreferences,
): string {
  const ticks = Array.from({ length: 5 },
    // interpolate one label from the vertical maximum to minimum
    (_value, index) => maximum - ((maximum - minimum) * index) / 4,
  );
  return `
    <div class="trend-y-axis" aria-hidden="true">
      ${ticks.map(
        // format one preferred-unit vertical tick
        (value) => {
          const measurement = formatTrendMeasurement(value, format, units);
          return `<span>${escapeHtml(measurement.value)} ${escapeHtml(measurement.unit)}</span>`;
        },
      ).join("")}
    </div>
  `;
}

// render the one-chart measurement title and flyover
function renderTrendMetricControl(
  selected: TrendChartMetric,
  disabled = false,
): string {
  const selectedOption = TREND_CHART_OPTIONS.find(
    // resolve the visible title from the reviewed metrics
    (option) => option.metric === selected,
  ) ?? TREND_CHART_OPTIONS[0]!;

  // reserve a non-interactive title during loading
  if (disabled) {
    return `
      <div class="trend-metric-control trend-metric-control-skeleton">
        <h2 class="trend-chart-title">${escapeHtml(selectedOption.label)}</h2>
        <span class="trend-metric-caret" aria-hidden="true"></span>
      </div>
    `;
  }

  return `
    <div class="trend-metric-control" data-trend-metric-control>
      <h2 class="trend-chart-title">
        <button type="button" class="trend-metric-trigger" data-trend-metric-trigger aria-expanded="false" aria-haspopup="menu" aria-controls="trend-metric-flyover">
          <span>${escapeHtml(selectedOption.label)}</span>
          <span class="trend-metric-caret" aria-hidden="true"></span>
        </button>
      </h2>
      <div class="trend-metric-flyover" id="trend-metric-flyover" role="menu" aria-label="Trend measurement" hidden>
        ${(["Measurements", "Farm insights"] as const).map(
          // render one labeled chart family
          (group) => `
            <div class="trend-metric-option-group" role="presentation">
              <span>${group}</span>
              ${TREND_CHART_OPTIONS.filter(
                // retain options from one chart family
                (option) => option.group === group,
              ).map(
                // render one reviewed flyover option
                (option) => `<button type="button" class="trend-metric-option" data-trend-metric-option="${escapeHtml(option.metric)}" role="menuitemradio" aria-checked="${String(option.metric === selected)}">${escapeHtml(option.label)}</button>`,
              ).join("")}
            </div>
          `,
        ).join("")}
      </div>
    </div>
  `;
}

// render one concise farm-chart definition or interactive threshold
function renderTrendInsightControls(
  state: DashboardState,
  option: TrendChartOption,
): string {
  // describe each derived chart without crowding its title
  switch (option.metric) {
    case "cumulativePrecipitationMm":
      return '<p class="trend-insight-note">Running annual total</p>';
    case "temperatureAnomalyC":
      return '<p class="trend-insight-note">Versus historical daily average</p>';
    case "temperatureRangeC":
      return '<p class="trend-insight-note">Daily high − low</p>';
    case "drySpellDays": {
      const threshold = formatTrendMeasurement(0.254, "precipitation", state.units);
      return `<p class="trend-insight-note">Rain below ${escapeHtml(threshold.value)} ${escapeHtml(threshold.unit)}</p>`;
    }
    case "growingDegreeDaysC": {
      const base = formatTrendMeasurement(10, "temperature", state.units);
      return `<p class="trend-insight-note">Base ${escapeHtml(base.value)} ${escapeHtml(base.unit)}</p>`;
    }
    case "frostDayCount": {
      const freezing = formatTrendMeasurement(0, "temperature", state.units);
      return `<p class="trend-insight-note">Daily low ≤ ${escapeHtml(freezing.value)} ${escapeHtml(freezing.unit)}</p>`;
    }
    case "extremeDayCount":
      return renderTrendExtremeControls(state);
    default:
      return "";
  }
}

// render the configurable extreme-day threshold controls
function renderTrendExtremeControls(state: DashboardState): string {
  const kind = normalizeTrendExtremeKind(state.trendExtremeKind);
  const configuration = trendExtremeConfiguration(kind);
  const threshold = normalizeTrendExtremeThreshold(state.trendExtremeThreshold, configuration);
  const thresholdValue = trendDisplayValue(threshold, configuration.format, state.units);
  const minimum = trendDisplayValue(configuration.minimum, configuration.format, state.units);
  const maximum = trendDisplayValue(configuration.maximum, configuration.format, state.units);
  const unit = formatTrendMeasurement(threshold, configuration.format, state.units).unit;

  return `
    <div class="trend-extreme-controls" data-trend-extreme-controls>
      <label>
        <span class="sr-only">Extreme-day measurement</span>
        <select data-trend-extreme-kind aria-label="Extreme-day measurement">
          ${Object.entries(TREND_EXTREME_CONFIGURATIONS).map(
            // render one supported threshold family
            ([value, candidate]) => `<option value="${value}"${value === kind ? " selected" : ""}>${candidate.label}</option>`,
          ).join("")}
        </select>
      </label>
      <span class="trend-extreme-comparator" aria-hidden="true">${configuration.comparator === "atLeast" ? "≥" : "≤"}</span>
      <label>
        <span class="sr-only">Extreme-day threshold</span>
        <input data-trend-extreme-threshold aria-label="Extreme-day threshold" type="number" min="${minimum.toFixed(2)}" max="${maximum.toFixed(2)}" step="1" value="${thresholdValue.toFixed(1)}">
      </label>
      <span class="trend-extreme-unit">${escapeHtml(unit)}</span>
    </div>
  `;
}

// normalize one runtime extreme-day family
function normalizeTrendExtremeKind(value: TrendExtremeKind | undefined): TrendExtremeKind {
  // retain only reviewed threshold families
  if (value === "cold" || value === "heat" || value === "rain" || value === "wind") {
    return value;
  }

  return "heat";
}

// normalize one runtime threshold against its canonical bounds
function normalizeTrendExtremeThreshold(
  value: number | undefined,
  configuration: TrendExtremeConfiguration,
): number {
  // replace missing or unsafe thresholds with one reviewed default
  if (
    value === undefined ||
    !Number.isFinite(value) ||
    value < configuration.minimum ||
    value > configuration.maximum
  ) {
    return configuration.defaultThreshold;
  }

  return value;
}

type TrendValueResolver = (point: TrendPoint) => number | null;

// build the selected raw or farm-derived annual series
function buildTrendChartYearSeries(
  state: DashboardState,
  option: TrendChartOption,
): readonly TrendYearSeries[] {
  const timezone = state.selectedSite?.timezone ?? "UTC";

  // derive each farm chart from the shared daily payload
  switch (option.metric) {
    case "cumulativePrecipitationMm":
      return accumulateTrendYearSeries(
        buildTrendYearSeries(state.trends, "precipitationMm", timezone),
        // retain only physical precipitation accumulation
        (value) => Math.max(0, value),
      );
    case "temperatureAnomalyC":
      return buildTemperatureAnomalySeries(state, timezone);
    case "temperatureRangeC":
      return buildTrendValueYearSeries(
        state.trends,
        timezone,
        // calculate one daily high-to-low swing
        (point) => {
          const maximum = point.metrics.temperatureMaximumC;
          const minimum = point.metrics.temperatureMinimumC;
          return maximum === null || minimum === null ? null : Math.max(0, maximum - minimum);
        },
      );
    case "drySpellDays":
      return buildDrySpellSeries(
        buildTrendYearSeries(state.trends, "precipitationMm", timezone),
      );
    case "growingDegreeDaysC":
      return accumulateTrendYearSeries(
        buildTrendYearSeries(state.trends, "temperatureC", timezone),
        // accumulate heat above the standard 10 °C crop base
        (value) => Math.max(0, value - 10),
      );
    case "frostDayCount":
      return buildCumulativeCountSeries(
        buildTrendYearSeries(state.trends, "temperatureMinimumC", timezone),
        // count days that reached freezing
        (value) => value <= 0,
      );
    case "extremeDayCount": {
      const kind = normalizeTrendExtremeKind(state.trendExtremeKind);
      const configuration = trendExtremeConfiguration(kind);
      const threshold = normalizeTrendExtremeThreshold(
        state.trendExtremeThreshold,
        configuration,
      );
      return buildCumulativeCountSeries(
        buildTrendYearSeries(state.trends, configuration.metric, timezone),
        // compare one daily extreme to the selected boundary
        (value) => configuration.comparator === "atLeast"
          ? value >= threshold
          : value <= threshold,
      );
    }
    case "windDirectionRose":
      return [];
    default:
      return buildTrendYearSeries(state.trends, option.metric, timezone);
  }
}

// group one raw daily metric by its site-local calendar year
function buildTrendYearSeries(
  trends: readonly TrendPoint[],
  metric: TrendMetricKey,
  timezone: string,
): readonly TrendYearSeries[] {
  return buildTrendValueYearSeries(
    trends,
    timezone,
    // read one canonical daily metric
    (point) => point.metrics[metric],
  );
}

// group resolved daily values by their site-local calendar year
function buildTrendValueYearSeries(
  trends: readonly TrendPoint[],
  timezone: string,
  resolveValue: TrendValueResolver,
): readonly TrendYearSeries[] {
  const years = new Map<number, Map<string, TrendCalendarSample>>();

  // retain each finite daily reading
  for (const point of trends) {
    const value = resolveValue(point);

    // omit missing daily readings
    if (value === null || !Number.isFinite(value)) {
      continue;
    }

    const parts = formatWallClockParts(new Date(point.validAt), timezone);
    const dayKey = `${String(parts.month).padStart(2, "0")}-${String(parts.day).padStart(2, "0")}`;
    const samples = years.get(parts.year) ?? new Map<string, TrendCalendarSample>();
    samples.set(dayKey, {
      dayKey,
      value,
      x: trendCalendarPosition(parts.month, parts.day),
    });
    years.set(parts.year, samples);
  }

  return [...years.entries()].sort(([left], [right]) => left - right).map(
    // order one year from January through December
    ([year, samples]) => ({
      points: [...samples.values()].sort((left, right) => left.x - right.x),
      year,
    }),
  );
}

// accumulate one annual series from daily contributions
function accumulateTrendYearSeries(
  series: readonly TrendYearSeries[],
  contribution: (value: number) => number,
): readonly TrendYearSeries[] {
  return series.map(
    // restart each accumulation on January 1
    (year) => {
      let total = 0;
      return {
        points: year.points.map(
          // retain the cumulative value after one day
          (point) => {
            total += contribution(point.value);
            return { ...point, value: total };
          },
        ),
        year: year.year,
      };
    },
  );
}

// count matching days cumulatively within each calendar year
function buildCumulativeCountSeries(
  series: readonly TrendYearSeries[],
  matches: (value: number) => boolean,
): readonly TrendYearSeries[] {
  return accumulateTrendYearSeries(
    series,
    // add one only when the daily threshold matched
    (value) => matches(value) ? 1 : 0,
  );
}

// measure the active consecutive dry-day streak
function buildDrySpellSeries(
  series: readonly TrendYearSeries[],
): readonly TrendYearSeries[] {
  return series.map(
    // restart each dry spell on January 1
    (year) => {
      let streak = 0;
      return {
        points: year.points.map(
          // reset after at least 0.01 inches of measurable rain
          (point) => {
            streak = point.value < 0.254 ? streak + 1 : 0;
            return { ...point, value: streak };
          },
        ),
        year: year.year,
      };
    },
  );
}

// compare each daily mean with the historical calendar-day average
function buildTemperatureAnomalySeries(
  state: DashboardState,
  timezone: string,
): readonly TrendYearSeries[] {
  const source = buildTrendYearSeries(state.trends, "temperatureC", timezone);
  const currentYear = trendCurrentYear(state, source);
  const historical = source.filter(
    // exclude the incomplete current year from the climate baseline
    (year) => year.year !== currentYear,
  );
  const baselineSource = historical.length === 0 ? source : historical;
  const baseline = new Map<string, { count: number; total: number }>();

  // collect each historical calendar-day mean
  for (const year of baselineSource) {
    // add every observed historical day
    for (const point of year.points) {
      const day = baseline.get(point.dayKey) ?? { count: 0, total: 0 };
      day.count += 1;
      day.total += point.value;
      baseline.set(point.dayKey, day);
    }
  }

  return source.map(
    // subtract one shared climate baseline from every year
    (year) => ({
      points: year.points.flatMap(
        // retain only days with a historical comparison
        (point) => {
          const day = baseline.get(point.dayKey);
          return day === undefined
            ? []
            : [{ ...point, value: point.value - day.total / day.count }];
        },
      ),
      year: year.year,
    }),
  );
}

// summarize daily prevailing directions into sixteen compass sectors
function buildTrendWindRoseSectors(
  state: DashboardState,
): readonly TrendWindRoseSector[] {
  const currentYear = trendCurrentYear(state, []);
  const timezone = state.selectedSite?.timezone ?? "UTC";
  const currentCounts = Array.from({ length: WIND_ROSE_DIRECTIONS.length }, () => 0);
  const historicalCounts = Array.from({ length: WIND_ROSE_DIRECTIONS.length }, () => 0);

  // count each finite daily prevailing direction
  for (const point of state.trends) {
    const direction = point.metrics.windDirectionDegrees;

    // omit missing daily directions
    if (direction === null || !Number.isFinite(direction)) {
      continue;
    }

    const normalized = ((direction % 360) + 360) % 360;
    const sector = Math.round(normalized / 22.5) % WIND_ROSE_DIRECTIONS.length;
    const year = formatWallClockParts(new Date(point.validAt), timezone).year;

    // separate the partial current year from complete historical years
    if (year === currentYear) {
      currentCounts[sector] = (currentCounts[sector] ?? 0) + 1;
    } else {
      historicalCounts[sector] = (historicalCounts[sector] ?? 0) + 1;
    }
  }

  const currentTotal = currentCounts.reduce(
    // total the current-year direction samples
    (total, count) => total + count,
    0,
  );
  const historicalTotal = historicalCounts.reduce(
    // total the historical direction samples
    (total, count) => total + count,
    0,
  );

  return WIND_ROSE_DIRECTIONS.map(
    // convert one sector count into comparable percentages
    (label, index) => ({
      currentPercent: currentTotal === 0
        ? 0
        : ((currentCounts[index] ?? 0) / currentTotal) * 100,
      historicalPercent: historicalTotal === 0
        ? 0
        : ((historicalCounts[index] ?? 0) / historicalTotal) * 100,
      label,
    }),
  );
}

// render one annular wind-rose sector
function renderTrendWindRoseSector(
  index: number,
  percent: number,
  maximumPercent: number,
  className: string,
): string {
  // omit empty directions from the visual layer
  if (percent <= 0) {
    return "";
  }

  const innerRadius = 12;
  const outerRadius = innerRadius + (percent / maximumPercent) * 98;
  const startAngle = index * 22.5 - 99;
  const endAngle = index * 22.5 - 81;
  const innerStart = trendPolarPoint(160, 160, innerRadius, startAngle);
  const outerStart = trendPolarPoint(160, 160, outerRadius, startAngle);
  const outerEnd = trendPolarPoint(160, 160, outerRadius, endAngle);
  const innerEnd = trendPolarPoint(160, 160, innerRadius, endAngle);
  return `<path data-wind-rose-sector="${WIND_ROSE_DIRECTIONS[index] ?? ""}" data-wind-rose-percent="${percent.toFixed(4)}" class="trend-wind-rose-sector ${className}" d="M ${innerStart.x.toFixed(2)} ${innerStart.y.toFixed(2)} L ${outerStart.x.toFixed(2)} ${outerStart.y.toFixed(2)} A ${outerRadius.toFixed(2)} ${outerRadius.toFixed(2)} 0 0 1 ${outerEnd.x.toFixed(2)} ${outerEnd.y.toFixed(2)} L ${innerEnd.x.toFixed(2)} ${innerEnd.y.toFixed(2)} A ${innerRadius.toFixed(2)} ${innerRadius.toFixed(2)} 0 0 0 ${innerStart.x.toFixed(2)} ${innerStart.y.toFixed(2)} Z"/>`;
}

// project one polar coordinate into the shared wind-rose view box
function trendPolarPoint(
  centerX: number,
  centerY: number,
  radius: number,
  degrees: number,
): Readonly<{ x: number; y: number }> {
  const radians = (degrees * Math.PI) / 180;
  return {
    x: centerX + Math.cos(radians) * radius,
    y: centerY + Math.sin(radians) * radius,
  };
}

// smooth every calendar year with one centered moving average
function smoothTrendYearSeries(
  series: readonly TrendYearSeries[],
  windowDays: number,
): readonly TrendYearSeries[] {
  const radius = Math.floor(Math.max(1, windowDays) / 2);
  const tolerance = radius / 366;
  return series.map(
    // smooth one year without crossing the calendar boundary
    (year) => ({
      points: year.points.map(
        // average one centered calendar-day neighborhood
        (point) => {
          const neighbors = year.points.filter(
            // exclude samples outside the fixed rolling window
            (candidate) => Math.abs(candidate.x - point.x) <= tolerance,
          );
          const value = neighbors.reduce(
            // total one nearby sample
            (total, neighbor) => total + neighbor.value,
            0,
          ) / neighbors.length;
          return { ...point, value };
        },
      ),
      year: year.year,
    }),
  );
}

// calculate historical median, quartiles, and range per calendar day
function buildTrendAggregateSeries(series: readonly TrendYearSeries[]): readonly TrendAggregateSample[] {
  const days = new Map<string, { readonly values: number[]; readonly x: number }>();

  // collect each historical year into matching month-day buckets
  for (const year of series) {
    // collect every observed day
    for (const point of year.points) {
      const day = days.get(point.dayKey) ?? { values: [], x: point.x };
      day.values.push(point.value);
      days.set(point.dayKey, day);
    }
  }

  return [...days.entries()].sort(
    // preserve the calendar order
    ([_leftKey, left], [_rightKey, right]) => left.x - right.x,
  ).map(
    // summarize one historical calendar day
    ([dayKey, day]) => {
      const values = [...day.values].sort(
        // order one day's measurements for percentile interpolation
        (left, right) => left - right,
      );
      return {
        dayKey,
        lowerQuartile: trendPercentile(values, 0.25),
        maximum: values.at(-1) ?? 0,
        median: trendPercentile(values, 0.5),
        minimum: values[0] ?? 0,
        upperQuartile: trendPercentile(values, 0.75),
        x: day.x,
      };
    },
  );
}

// interpolate one percentile from an ordered finite sample
function trendPercentile(values: readonly number[], percentile: number): number {
  // preserve an impossible empty bucket honestly
  if (values.length === 0) {
    return 0;
  }

  const position = Math.max(0, Math.min(1, percentile)) * (values.length - 1);
  const lower = values[Math.floor(position)] ?? values[0] ?? 0;
  const upper = values[Math.ceil(position)] ?? values.at(-1) ?? lower;
  return lower + (upper - lower) * (position - Math.floor(position));
}

// describe the visible lines used by the shared trend scrubber
function buildTrendCrosshairSeries(
  series: readonly TrendYearSeries[],
  aggregate: readonly TrendAggregateSample[],
  displayMode: TrendDisplayMode,
  selectedYear: number | null,
  currentYear: number,
): readonly TrendCrosshairSeries[] {
  const indexedSeries = series.map(
    // retain stable line colors after visibility filtering
    (year, index) => ({ index, year }),
  );
  const currentSeries = indexedSeries.find(
    // locate the permanent current-year comparison
    (entry) => entry.year.year === currentYear,
  );

  // show the aggregate and current year together by default
  if (displayMode === "aggregate") {
    return [
      {
        colorClass: "trend-aggregate-color",
        key: "median",
        label: "Median",
        points: aggregate.map(
          // expose the median through the shared date scrubber
          (point) => ({ dayKey: point.dayKey, value: point.median, x: point.x }),
        ),
      },
      ...(currentSeries === undefined
        ? []
        : [{
            colorClass: "trend-current-year-color",
            key: String(currentYear),
            label: String(currentYear),
            points: currentSeries.year.points,
          }]),
    ];
  }

  const visibleSeries = selectedYear === null
    ? indexedSeries
    : indexedSeries.filter(
        // retain the selection and permanent current-year comparison
        (entry) => entry.year.year === selectedYear || entry.year.year === currentYear,
      );
  return visibleSeries.map(
    // expose every visible year through the shared date scrubber
    ({ index, year }) => ({
      colorClass: year.year === currentYear
        ? "trend-current-year-color"
        : `trend-year-color-${String(index % TREND_YEAR_COLORS.length)}`,
      key: String(year.year),
      label: String(year.year),
      points: year.points,
    }),
  );
}

// fit one vertical scale to every currently visible series
function trendVisibleDomain(
  series: readonly TrendYearSeries[],
  aggregate: readonly TrendAggregateSample[],
  displayMode: TrendDisplayMode,
  selectedYear: number | null,
  currentYear: number,
  option: TrendChartOption,
  units: UnitPreferences,
): Readonly<{ maximum: number; minimum: number }> {
  const visibleYears = displayMode === "all"
    ? selectedYear === null
      ? series
      : series.filter(
          // retain the selection and permanent current-year comparison
          (year) => year.year === selectedYear || year.year === currentYear,
        )
    : series.filter(
        // fit the permanent current-year comparison
        (year) => year.year === currentYear,
      );
  const values = [
    ...(option.includeZero === true ? [0] : []),
    ...aggregate.flatMap(
      // include both visible historical range lines
      (point) => [point.minimum, point.maximum],
    ),
    ...visibleYears.flatMap(
      // include every visible individual yearly line
      (year) => year.points.map(
        // retain one canonical measurement
        (point) => point.value,
      ),
    ),
  ].filter(
    // reject impossible chart coordinates
    (value) => Number.isFinite(value),
  );

  // preserve a usable empty scale
  if (values.length === 0) {
    return { maximum: option.maximum ?? 1, minimum: option.minimum ?? 0 };
  }

  const ordered = [...values].sort(
    // order values for the constant-domain fallback median
    (left, right) => left - right,
  );
  const median = trendPercentile(ordered, 0.5);
  let minimum = trendCanonicalValue(
    Math.floor(trendDisplayValue(ordered[0] ?? median, option.format, units)),
    option.format,
    units,
  );
  let maximum = trendCanonicalValue(
    Math.ceil(trendDisplayValue(ordered.at(-1) ?? median, option.format, units)),
    option.format,
    units,
  );

  // preserve natural bounds after display-unit rounding
  if (option.minimum !== undefined) {
    minimum = Math.max(option.minimum, minimum);
  }

  // preserve natural bounds after display-unit rounding
  if (option.maximum !== undefined) {
    maximum = Math.min(option.maximum, maximum);
  }

  // recover from a fully clamped or constant scale
  if (maximum <= minimum) {
    const displayMedian = trendDisplayValue(median, option.format, units);
    minimum = trendCanonicalValue(Math.floor(displayMedian) - 1, option.format, units);
    maximum = trendCanonicalValue(Math.ceil(displayMedian) + 1, option.format, units);

    // preserve one natural lower boundary after fallback expansion
    if (option.minimum !== undefined) {
      minimum = Math.max(option.minimum, minimum);
    }

    // preserve one natural upper boundary after fallback expansion
    if (option.maximum !== undefined) {
      maximum = Math.min(option.maximum, maximum);
    }
  }

  return { maximum, minimum };
}

// convert one canonical trend value into the configured consumer unit
function trendDisplayValue(
  value: number,
  format: TrendChartFormat,
  units: UnitPreferences,
): number {
  // convert one monotonic display scale
  switch (format) {
    case "count":
      return value;
    case "degreeDays":
    case "temperatureDelta":
      return units.temperature === "fahrenheit" ? (value * 9) / 5 : value;
    case "humidity":
      return value;
    case "temperature":
      return units.temperature === "fahrenheit" ? (value * 9) / 5 + 32 : value;
    case "windSpeed":
      switch (units.windSpeed) {
        case "miles_per_hour":
          return value * 2.236_936_292_1;
        case "kilometers_per_hour":
          return value * 3.6;
        case "meters_per_second":
          return value;
      }
    case "precipitation":
      return units.precipitation === "inches" ? value / 25.4 : value;
    case "pressure":
      switch (units.pressure) {
        case "atmosphere_percent":
          return ((value / 1_013.25) - 1) * 100;
        case "inches_of_mercury":
          return value * 0.029_529_983_1;
        case "hectopascals":
          return value;
      }
    case "waterLevel":
      return units.waterLevel === "feet" ? value * 3.280_839_895 : value;
  }
}

// convert one consumer-unit boundary back into canonical storage units
function trendCanonicalValue(
  value: number,
  format: TrendChartFormat,
  units: UnitPreferences,
): number {
  // invert one monotonic display scale
  switch (format) {
    case "count":
      return value;
    case "degreeDays":
    case "temperatureDelta":
      return units.temperature === "fahrenheit" ? (value * 5) / 9 : value;
    case "humidity":
      return value;
    case "temperature":
      return units.temperature === "fahrenheit" ? ((value - 32) * 5) / 9 : value;
    case "windSpeed":
      switch (units.windSpeed) {
        case "miles_per_hour":
          return value / 2.236_936_292_1;
        case "kilometers_per_hour":
          return value / 3.6;
        case "meters_per_second":
          return value;
      }
    case "precipitation":
      return units.precipitation === "inches" ? value * 25.4 : value;
    case "pressure":
      switch (units.pressure) {
        case "atmosphere_percent":
          return (value / 100 + 1) * 1_013.25;
        case "inches_of_mercury":
          return value / 0.029_529_983_1;
        case "hectopascals":
          return value;
      }
    case "waterLevel":
      return units.waterLevel === "feet" ? value / 3.280_839_895 : value;
  }
}

// format one trend value with its consumer unit
function formatTrendMeasurement(
  value: number | null,
  format: TrendChartFormat,
  units: UnitPreferences,
): FormattedMeasurement {
  // format whole-day streaks and cumulative counts
  if (format === "count") {
    return formatFixedMeasurement(value, "days", 0);
  }

  // label accumulated heat as gdd rather than a calendar-day count
  if (format === "degreeDays") {
    const displayValue = value === null
      ? null
      : trendDisplayValue(value, format, units);
    return formatFixedMeasurement(displayValue, "GDD", 0);
  }

  // format temperature differences without an absolute-temperature offset
  if (format === "temperatureDelta") {
    const displayValue = value === null
      ? null
      : trendDisplayValue(value, format, units);
    return formatFixedMeasurement(
      displayValue,
      units.temperature === "fahrenheit" ? "°F" : "°C",
      0,
    );
  }

  // format normalized humidity without a configurable unit
  if (format === "humidity") {
    return formatFixedMeasurement(value, "%", 0);
  }

  // retain useful precision for small daily rain totals
  if (format === "precipitation") {
    return formatMeasurement(value, format, units);
  }

  // preserve the product-wide pressure precision
  if (format === "pressure") {
    return formatMeasurement(value, format, units, 1);
  }

  // match homepage whole-number temperature presentation
  if (format === "temperature") {
    return formatMeasurement(value, format, units, 0);
  }

  return formatMeasurement(value, format, units, 1);
}

// render the initial shared date and every visible line intersection
function renderTrendCrosshair(
  series: readonly TrendCrosshairSeries[],
  position: number,
  option: TrendChartOption,
  units: UnitPreferences,
): string {
  const date = trendCalendarDate(position);
  const newestFirst = [...series].reverse();
  const summaries = newestFirst.map(
    // render one visible line at the selected calendar position
    (entry) => {
      const measurement = formatTrendMeasurement(
        interpolateTrendValue(entry.points, position),
        option.format,
        units,
      );
      const compact = compactMeasurement(measurement) ?? "—";
      return `<span class="trend-crosshair-value"><i class="${entry.colorClass}" aria-hidden="true"></i><strong>${escapeHtml(entry.label)}</strong><output data-trend-crosshair-value="${escapeHtml(entry.key)}">${escapeHtml(compact)}</output></span>`;
    },
  );
  const ariaValueText = `${date.label}. ${newestFirst.map(
    // summarize one visible line for assistive technology
    (entry) => {
      const measurement = formatTrendMeasurement(
        interpolateTrendValue(entry.points, position),
        option.format,
        units,
      );
      return `${entry.label} ${compactMeasurement(measurement) ?? "unavailable"}`;
    },
  ).join(". ")}`;

  return `
    <div class="trend-crosshair-line" aria-hidden="true"></div>
    <div
      class="trend-crosshair-slider"
      data-trend-crosshair-slider
      role="slider"
      tabindex="0"
      aria-label="Annual trend date scrubber"
      aria-valuemin="0"
      aria-valuemax="365"
      aria-valuenow="${String(Math.round(position * 365))}"
      aria-valuetext="${escapeHtml(ariaValueText)}"
    ></div>
    <time class="trend-crosshair-date-pill" data-trend-crosshair-date datetime="${date.key}" aria-hidden="true">${date.label}</time>
    <div class="trend-crosshair-summary" aria-hidden="true">
      <div class="trend-crosshair-values">${summaries.join("")}</div>
    </div>
  `;
}

// resolve the latest populated day in the current calendar year
function trendInitialPosition(
  state: DashboardState,
  series: readonly TrendYearSeries[],
): number {
  const currentYear = trendCurrentYear(state, series);
  const currentSeries = series.find(
    // locate the current calendar-year line
    (year) => year.year === currentYear,
  ) ?? series.at(-1);
  return currentSeries?.points.at(-1)?.x ?? 0;
}

// map today's site-local date onto the shared annual axis
function trendTodayPosition(state: DashboardState): number {
  const instant = new Date(state.trendGeneratedAt ?? Date.now());
  const parts = formatWallClockParts(instant, state.selectedSite?.timezone ?? "UTC");
  return trendCalendarPosition(parts.month, parts.day);
}

// interpolate one yearly line at a shared calendar date
function interpolateTrendValue(
  points: readonly TrendCalendarSample[],
  position: number,
): number | null {
  const first = points[0];
  const last = points.at(-1);
  const tolerance = 1 / (366 * 24 * 60);

  // reject dates beyond one partial year's coverage
  if (
    first === undefined ||
    last === undefined ||
    position < first.x - tolerance ||
    position > last.x + tolerance
  ) {
    return null;
  }

  const boundedPosition = Math.max(first.x, Math.min(last.x, position));

  // locate one enclosing pair
  for (let index = 0; index < points.length; index += 1) {
    const upper = points[index];

    // skip an impossible sparse entry
    if (upper === undefined) {
      continue;
    }

    // return one exact daily sample
    if (upper.x === boundedPosition || index === 0) {
      if (upper.x === boundedPosition) {
        return upper.value;
      }

      continue;
    }

    // interpolate after reaching the upper daily sample
    if (upper.x > boundedPosition) {
      const lower = points[index - 1];

      // preserve one malformed sparse series honestly
      if (lower === undefined || upper.x === lower.x) {
        return null;
      }

      const ratio = (boundedPosition - lower.x) / (upper.x - lower.x);
      return lower.value + (upper.value - lower.value) * ratio;
    }
  }

  return last.value;
}

// convert a shared calendar position into a stable display date
function trendCalendarDate(position: number): Readonly<{ key: string; label: string }> {
  const bounded = Math.max(0, Math.min(1, position));
  const start = Date.UTC(2000, 0, 1);
  const day = Math.max(0, Math.min(365, Math.round(bounded * 366)));
  const date = new Date(start + day * 24 * 60 * 60 * 1_000);
  return {
    key: date.toISOString().slice(0, 10),
    label: new Intl.DateTimeFormat("en-US", {
      day: "numeric",
      month: "short",
      timeZone: "UTC",
    }).format(date),
  };
}

// place one crosshair on the padded SVG plot
function trendChartPercentage(position: number): number {
  return (trendChartX(Math.max(0, Math.min(1, position))) / TREND_CHART_WIDTH) * 100;
}

// render one closed historical interquartile band
function renderTrendAggregateBandPath(
  aggregate: readonly TrendAggregateSample[],
  minimum: number,
  span: number,
): string {
  // omit an area without enough horizontal extent
  if (aggregate.length < 2) {
    return "";
  }

  const upper = aggregate.map(
    // project one upper quartile edge
    (point) => `${trendChartX(point.x).toFixed(2)},${trendChartY(point.upperQuartile, minimum, span).toFixed(2)}`,
  );
  const lower = [...aggregate].reverse().map(
    // project one lower quartile edge
    (point) => `${trendChartX(point.x).toFixed(2)},${trendChartY(point.lowerQuartile, minimum, span).toFixed(2)}`,
  );
  return `M ${upper.join(" L ")} L ${lower.join(" L ")} Z`;
}

// render one aggregate statistic as a point list
function renderTrendAggregateLinePoints(
  aggregate: readonly TrendAggregateSample[],
  statistic: "maximum" | "median" | "minimum",
  minimum: number,
  span: number,
): string {
  return aggregate.map(
    // project one daily aggregate point
    (point) => `${trendChartX(point.x).toFixed(2)},${trendChartY(point[statistic], minimum, span).toFixed(2)}`,
  ).join(" ");
}

// render one yearly line point list
function renderTrendLinePoints(
  points: readonly TrendCalendarSample[],
  minimum: number,
  span: number,
): string {
  return points.map(
    // project one daily point
    (point) => `${trendChartX(point.x).toFixed(2)},${trendChartY(point.value, minimum, span).toFixed(2)}`,
  ).join(" ");
}

// render month markers across one fixed calendar-year axis
function renderTrendMonthGrid(): string {
  return `
    <line x1="${TREND_CHART_PADDING_LEFT}" y1="${TREND_CHART_HEIGHT - TREND_CHART_PADDING_BOTTOM}" x2="${TREND_CHART_WIDTH - TREND_CHART_PADDING_RIGHT}" y2="${TREND_CHART_HEIGHT - TREND_CHART_PADDING_BOTTOM}" class="trend-chart-axis"/>
    <line x1="${TREND_CHART_PADDING_LEFT}" y1="${TREND_CHART_PADDING_TOP}" x2="${TREND_CHART_PADDING_LEFT}" y2="${TREND_CHART_HEIGHT - TREND_CHART_PADDING_BOTTOM}" class="trend-chart-axis"/>
    ${Array.from({ length: 4 },
      // draw one horizontal guide for each non-bottom vertical tick
      (_value, index) => {
        const y = TREND_CHART_PADDING_TOP +
          (index * (TREND_CHART_HEIGHT - TREND_CHART_PADDING_TOP - TREND_CHART_PADDING_BOTTOM)) / 4;
        return `<line x1="${TREND_CHART_PADDING_LEFT}" y1="${y.toFixed(2)}" x2="${TREND_CHART_WIDTH - TREND_CHART_PADDING_RIGHT}" y2="${y.toFixed(2)}" class="trend-y-grid-line"/>`;
      },
    ).join("")}
    ${TREND_MONTH_LABELS.map(
      // mark one calendar month
      (_label, index) => {
        const x = trendChartX(trendCalendarPosition(index + 1, 1));
        return `<line x1="${x.toFixed(2)}" y1="${TREND_CHART_PADDING_TOP}" x2="${x.toFixed(2)}" y2="${TREND_CHART_HEIGHT - TREND_CHART_PADDING_BOTTOM}" class="trend-month-line"/>`;
      },
    ).join("")}
  `;
}

// render responsive month labels outside the stretched SVG
function renderTrendMonthAxis(): string {
  return `
    <div class="trend-month-axis" aria-hidden="true">
      ${TREND_MONTH_LABELS.map(
        // label one calendar month
        (label) => `<span class="trend-month-label">${label}</span>`,
      ).join("")}
    </div>
  `;
}

// resolve the highlighted current site year
function trendCurrentYear(state: DashboardState, series: readonly TrendYearSeries[]): number {
  // use the response clock when available
  if (typeof state.trendGeneratedAt === "string") {
    return formatWallClockParts(
      new Date(state.trendGeneratedAt),
      state.selectedSite?.timezone ?? "UTC",
    ).year;
  }

  return series.at(-1)?.year ?? 0;
}

// map one month-day onto a leap-safe shared year
function trendCalendarPosition(month: number, day: number): number {
  const start = Date.UTC(2000, 0, 1);
  const end = Date.UTC(2001, 0, 1);
  return (Date.UTC(2000, month - 1, day) - start) / (end - start);
}

// project one normalized horizontal coordinate
function trendChartX(position: number): number {
  return TREND_CHART_PADDING_LEFT + position * (TREND_CHART_WIDTH - TREND_CHART_PADDING_LEFT - TREND_CHART_PADDING_RIGHT);
}

// project one measurement onto the shared vertical scale
function trendChartY(value: number, minimum: number, span: number): number {
  return TREND_CHART_HEIGHT - TREND_CHART_PADDING_BOTTOM -
    ((value - minimum) / span) * (TREND_CHART_HEIGHT - TREND_CHART_PADDING_TOP - TREND_CHART_PADDING_BOTTOM);
}

// select one stable year color
function trendYearColor(index: number): string {
  return TREND_YEAR_COLORS[index % TREND_YEAR_COLORS.length] ?? TREND_YEAR_COLORS[0];
}

const STATION_MAP_HEIGHT = 520;
const STATION_MAP_TILE_SIZE = 256;
const STATION_MAP_WIDTH = 640;
const STATION_MAP_ZOOM = 13;
const PROPERTY_MAP_HEIGHT = 400;
const PROPERTY_MAP_WIDTH = 640;
const PROPERTY_MAP_ZOOM = 17;
const PROPERTY_SATELLITE_IMAGE_SERVICE = "https://imagery.nationalmap.gov/arcgis/rest/services/USGSNAIPImagery/ImageServer/exportImage";
const WEB_MERCATOR_LIMIT_METERS = 20_037_508.342_789_244;
const PROPERTY_SENSOR_ICON_OPTIONS: readonly Readonly<{
  icon: PropertySensorIcon;
  label: string;
  material: MaterialIconName;
}>[] = [
  { icon: "temperature", label: "Temperature", material: "device_thermostat" },
  { icon: "wind", label: "Wind", material: "air" },
  { icon: "rain", label: "Rain", material: "rainy" },
  { icon: "air-quality", label: "Air quality", material: "masks" },
];

interface MapViewport {
  readonly left: number;
  readonly top: number;
  readonly zoom: number;
}

// render the soil-only farm overview for eligible viewers
function renderAdminSoilMoistureMap(state: DashboardState): string {
  // keep delayed optional positions distinct from unavailable positions
  if (state.loading || state.propertySensorLayoutLoading) {
    return renderPropertyMapSkeleton(true);
  }

  const site = state.selectedSite;

  // wait for the fixed site before projecting sensor coordinates
  if (site === null) {
    return "";
  }

  const sensors = propertySensorSnapshots(state).filter(
    // retain only probes with a usable soil moisture reading
    (sensor) => Number.isFinite(sensor.readings.soilMoisturePercent),
  );
  const layoutByKey = new Map(
    (state.propertySensorLayout ?? []).map(
      // index each persisted sensor location
      (entry) => [entry.sensorKey, entry],
    ),
  );
  const placed = sensors.flatMap(
    // pair only probes with persisted map coordinates
    (sensor) => {
      const layout = layoutByKey.get(sensor.key);
      return layout === undefined ? [] : [{ layout, sensor }];
    },
  );
  const viewport = createFixedMapViewport(
    site.latitude,
    site.longitude,
    PROPERTY_MAP_ZOOM,
    PROPERTY_MAP_WIDTH,
    PROPERTY_MAP_HEIGHT,
  );
  const markerOffsets = propertySensorMarkerOffsets(placed, viewport);
  const markers = placed.map(
    // render one visible percentage at every placed soil probe
    ({ layout, sensor }) => renderSoilMoistureMarker(
      layout,
      sensor,
      viewport,
      markerOffsets.get(sensor.key) ?? { x: 0, y: 0 },
    ),
  ).join("");

  // distinguish unavailable positions from an empty saved layout
  return `
    <section class="admin-soil-map-panel" data-admin-soil-map aria-labelledby="admin-soil-map-heading">
      <div class="section-heading">
        <div><p class="eyebrow">Ballydídean property</p><h2 id="admin-soil-map-heading">Soil moisture</h2></div>
        <span class="property-map-count">${String(placed.length)} reporting</span>
      </div>
      <div class="property-map admin-soil-map">
        <div class="property-map-canvas">
          <svg class="property-map-svg" data-property-interactive-map data-property-map-width="${String(PROPERTY_MAP_WIDTH)}" data-property-map-height="${String(PROPERTY_MAP_HEIGHT)}" viewBox="0 0 ${PROPERTY_MAP_WIDTH} ${PROPERTY_MAP_HEIGHT}" role="group" aria-label="Current soil moisture across the Ballydídean property">
            <g data-property-map-world>
              <g class="map-tile-layer" aria-hidden="true">${renderPropertyMapTiles(state.propertyMapLayer, viewport)}</g>
            </g>
            <g class="property-map-overlay">
              ${markers}
            </g>
          </svg>
          ${renderPropertyMapLayerControls(state.propertyMapLayer, viewport)}
          ${renderPropertyMapZoomControls()}
        </div>
        ${renderPropertyMapAttribution(state.propertyMapLayer)}
      </div>
      ${state.propertySensorLayout === null
        ? `<p class="empty-panel">Soil moisture sensor positions are unavailable.</p>`
        : placed.length === 0
          ? `<p class="empty-panel">No soil moisture sensors have been placed yet.</p>`
          : ""}
      ${state.propertySensorLayout !== null && sensors.length > placed.length
        ? `<p class="property-map-note">${String(sensors.length - placed.length)} soil moisture sensor${sensors.length - placed.length === 1 ? " still needs" : "s still need"} a position in Admin.</p>`
        : ""}
    </section>
  `;
}

// render one soil probe as a fixed-size percentage marker
function renderSoilMoistureMarker(
  layout: PropertySensorLayout,
  sensor: PropertySensorSnapshot,
  viewport: MapViewport,
  offset: Readonly<{ x: number; y: number }>,
): string {
  const point = projectMapPoint(layout.latitude, layout.longitude, viewport);
  const moisture = formatNumber(sensor.readings.soilMoisturePercent ?? 0, 0);
  return `
    <g class="soil-moisture-marker" data-soil-moisture-sensor="${escapeHtml(sensor.key)}" data-property-map-anchor data-property-map-x="${point.x.toFixed(2)}" data-property-map-y="${point.y.toFixed(2)}" transform="translate(${point.x.toFixed(2)} ${point.y.toFixed(2)})" role="img" aria-label="${escapeHtml(layout.displayName)}: ${escapeHtml(moisture)}% soil moisture">
      <line class="soil-moisture-marker-leader" x1="0" y1="0" x2="${offset.x.toFixed(2)}" y2="${offset.y.toFixed(2)}"/>
      <circle class="soil-moisture-marker-anchor" r="3"/>
      <g class="soil-moisture-marker-head" transform="translate(${offset.x.toFixed(2)} ${offset.y.toFixed(2)})">
        <circle r="19"/>
        <text x="0" y="4">${escapeHtml(moisture)}%</text>
      </g>
      <title>${escapeHtml(layout.displayName)} · ${escapeHtml(moisture)}% soil moisture</title>
    </g>
  `;
}

// render the farm-scale EcoWitt sensor geography first
function renderPropertySensorMap(state: DashboardState): string {
  // reserve both the map and sensor list until their joint read settles
  if (state.loading) {
    return renderPropertyMapSkeleton();
  }

  const site = state.selectedSite;

  // reserve the property map until current data arrives
  if (site === null) {
    return "";
  }

  const sensors = propertySensorSnapshots(state);
  const layoutByKey = new Map(
    (state.propertySensorLayout ?? []).map(
      // index each server-wide sensor placement
      (entry) => [entry.sensorKey, entry],
    ),
  );
  const placed = sensors.flatMap((sensor) => {
    const layout = layoutByKey.get(sensor.key);
    return layout === undefined ? [] : [{ layout, sensor }];
  });
  const viewport = createFixedMapViewport(
    site.latitude,
    site.longitude,
    PROPERTY_MAP_ZOOM,
    PROPERTY_MAP_WIDTH,
    PROPERTY_MAP_HEIGHT,
  );
  const markerOffsets = propertySensorMarkerOffsets(placed, viewport);
  const markers = placed.map(
    // render each configured first-party sensor
    ({ layout, sensor }) => renderPropertySensorMarker(
      layout,
      sensor,
      viewport,
      state.units,
      state.selectedPropertySensorKey === sensor.key,
      markerOffsets.get(sensor.key) ?? { x: 0, y: 0 },
    ),
  ).join("");
  const sensorRows = placed.map(
    // render every configured sensor row
    ({ layout, sensor }) => renderPropertySensorListItem(
      layout,
      sensor,
      state.units,
      state.selectedPropertySensorKey === sensor.key,
    ),
  ).join("");

  return `
    <section class="panel property-map-panel" aria-labelledby="property-map-heading">
      <div class="section-heading">
        <div><p class="eyebrow">Ballydídean property</p><h2 id="property-map-heading">Property sensors</h2></div>
        <span class="property-map-count">${String(placed.length)} placed</span>
      </div>
      <div class="property-map-layout">
        <div class="property-map">
          <div class="property-map-canvas">
            <svg class="property-map-svg" data-property-interactive-map data-property-map-width="${String(PROPERTY_MAP_WIDTH)}" data-property-map-height="${String(PROPERTY_MAP_HEIGHT)}" viewBox="0 0 ${PROPERTY_MAP_WIDTH} ${PROPERTY_MAP_HEIGHT}" role="group" aria-label="EcoWitt sensors placed across the Ballydídean property">
              <g data-property-map-world>
                <g class="map-tile-layer" aria-hidden="true">${renderPropertyMapTiles(state.propertyMapLayer, viewport)}</g>
              </g>
              <g class="property-map-overlay">
                <g data-property-map-anchor data-property-map-x="${(PROPERTY_MAP_WIDTH / 2).toFixed(2)}" data-property-map-y="${(PROPERTY_MAP_HEIGHT / 2).toFixed(2)}" aria-hidden="true"><circle r="8" class="farm-marker"/></g>
                ${markers}
              </g>
            </svg>
            ${renderPropertyMapLayerControls(state.propertyMapLayer, viewport)}
            ${renderPropertyMapZoomControls()}
          </div>
          ${renderPropertyMapAttribution(state.propertyMapLayer)}
        </div>
        <div class="property-sensor-list-shell">
          ${placed.length === 0
            ? `<p class="empty-panel">No property sensors have been placed yet.</p>`
            : `<ol class="property-sensor-list" aria-label="Placed property sensors">${sensorRows}</ol>`}
        </div>
      </div>
      ${sensors.length > placed.length
        ? `<p class="property-map-note">${String(sensors.length - placed.length)} reporting sensor${sensors.length - placed.length === 1 ? " still needs" : "s still need"} a position in Admin.</p>`
        : ""}
    </section>
  `;
}

// reserve the existing map frame without requesting placeholder tiles
function renderPropertyMapSkeleton(soil = false): string {
  const headingId = soil ? "admin-soil-map-heading" : "property-map-heading";
  return `
    <section class="${soil ? "admin-soil-map-panel" : "panel property-map-panel"} skeleton-region"${soil ? " data-admin-soil-map" : ""} aria-labelledby="${headingId}" aria-busy="true">
      <div class="section-heading">
        <div><p class="eyebrow">Ballydídean property</p><h2 id="${headingId}">${soil ? "Soil moisture" : "Property sensors"}</h2></div>
      </div>
      <div class="${soil ? "admin-soil-map" : "property-map-layout"}">
        <div class="property-map skeleton-map" aria-hidden="true">
          <div class="property-map-canvas"><span class="skeleton-map-shape"></span></div>
          <p class="map-attribution"><span class="skeleton-attribution">Map attribution</span></p>
        </div>
        ${soil ? "" : renderSkeletonFields()}
      </div>
    </section>
  `;
}

// reserve short lists and form fields without fake interactive controls
function renderSkeletonFields(): string {
  return `<div class="skeleton-fields" aria-hidden="true">${Array.from({ length: 3 },
    // preserve a readable row-sized loading surface
    () => '<span class="skeleton-line skeleton-field"></span>',
  ).join("")}</div>`;
}

// space dense property markers while retaining exact anchor lines
function propertySensorMarkerOffsets(
  placed: readonly Readonly<{
    layout: PropertySensorLayout;
    sensor: PropertySensorSnapshot;
  }>[],
  viewport: MapViewport,
): ReadonlyMap<string, Readonly<{ x: number; y: number }>> {
  const spacing = 38;
  const candidates: Array<Readonly<{ x: number; y: number }>> = [{ x: 0, y: 0 }];

  // build deterministic square rings around each true position
  for (let ring = 1; ring <= 4; ring += 1) {
    // inspect every candidate row
    for (let row = -ring; row <= ring; row += 1) {
      // inspect every candidate column
      for (let column = -ring; column <= ring; column += 1) {
        // retain only the current square perimeter
        if (Math.abs(row) !== ring && Math.abs(column) !== ring) {
          continue;
        }

        candidates.push({ x: column * spacing, y: row * spacing });
      }
    }
  }

  const occupied: Array<Readonly<{ x: number; y: number }>> = [];
  const offsets = new Map<string, Readonly<{ x: number; y: number }>>();

  // assign the nearest collision-free visible offset
  for (const { layout, sensor } of placed) {
    const anchor = projectMapPoint(layout.latitude, layout.longitude, viewport);
    const offset = candidates.find(
      // retain one in-bounds point clear of earlier markers
      (candidate) => {
        const point = { x: anchor.x + candidate.x, y: anchor.y + candidate.y };

        // keep every marker head inside the original map
        if (
          point.x < spacing ||
          point.x > PROPERTY_MAP_WIDTH - spacing ||
          point.y < spacing ||
          point.y > PROPERTY_MAP_HEIGHT - spacing
        ) {
          return false;
        }

        return occupied.every(
          // preserve one complete marker diameter
          (prior) => Math.hypot(point.x - prior.x, point.y - prior.y) >= spacing,
        );
      },
    ) ?? { x: 0, y: 0 };
    offsets.set(sensor.key, offset);
    occupied.push({ x: anchor.x + offset.x, y: anchor.y + offset.y });
  }

  return offsets;
}

const FORECAST_ADJUSTMENT_SCORECARD_STATE_LABELS = {
  evidence: {
    as_issued: "As issued",
    prospective_receipt: "Prospective receipt",
    retrospective_counterfactual: "Retrospective counterfactual",
    development: "Development",
  },
  support: {
    sufficient: "Sufficient support",
    insufficient: "Insufficient support",
    invalid: "Invalid support",
  },
  comparison: {
    unscored: "Unscored",
    better: "Better",
    mixed: "Mixed",
    worse: "Worse",
  },
  qualification: {
    development_only: "Development only",
    counterfactual_only: "Counterfactual only",
    pending_support: "Pending support",
    supported: "Supported",
    rejected: "Rejected",
  },
  serving: {
    authorized_active: "Authorized active",
    admin_disabled: "Admin disabled",
    fail_raw: "Failing raw",
    pending_review: "Pending review",
  },
  recommendation: {
    retain: "Retain",
    review_candidate: "Review candidate",
    review_disable: "Review disabling",
    none: "No recommendation",
  },
} as const;

// format one scorecard unit
function forecastAdjustmentScorecardUnit(unit: ForecastAdjustmentScorecardMetric["unit"]): string {
  return unit === "celsius" ? "°C" : unit === "meters_per_second" ? "m/s" : "mm/h";
}

// format one aggregate metric without inventing missing evidence
function formatForecastAdjustmentScorecardMetric(
  value: number | null,
  unit: ForecastAdjustmentScorecardMetric["unit"],
): string {
  return value === null ? "—" : `${formatNumber(value, 2)} ${forecastAdjustmentScorecardUnit(unit)}`;
}

// format one scorecard percentage
function formatForecastAdjustmentScorecardPercent(value: number | null): string {
  return value === null ? "—" : `${formatNumber(value, 1)}%`;
}

// shorten one immutable digest while retaining its complete accessible identity
function renderForecastAdjustmentScorecardHash(value: string | null): string {
  return value === null
    ? '<span class="adjustment-scorecard-unavailable">Unavailable</span>'
    : `<code title="${escapeHtml(value)}">${escapeHtml(value.slice(0, 12))}…</code>`;
}

// render bounded reason counts
function renderForecastAdjustmentScorecardReasons(reasons: Readonly<Record<string, number>>): string {
  const entries = Object.entries(reasons);

  // state honest absence instead of omitting the diagnostic
  if (entries.length === 0) {
    return "None";
  }

  return entries.map(
    // render one safe code as an operator-readable count
    ([code, count]) => `${escapeHtml(code.replaceAll("_", " "))} (${String(count)})`,
  ).join(", ");
}

// render raw and adjusted aggregate errors together
function renderForecastAdjustmentScorecardMetrics(metrics: ForecastAdjustmentScorecardMetric): string {
  const interval = metrics.skillInterval95 === null
    ? "—"
    : `${formatNumber(metrics.skillInterval95.lower, 1)}% to ${formatNumber(metrics.skillInterval95.upper, 1)}%`;
  return `
    <div class="adjustment-scorecard-table-scroll">
      <table class="adjustment-scorecard-metrics">
        <thead><tr><th scope="col">Metric</th><th scope="col">Raw</th><th scope="col">Adjusted</th><th scope="col">Change</th></tr></thead>
        <tbody>
          <tr><th scope="row">MAE</th><td>${formatForecastAdjustmentScorecardMetric(metrics.rawMae, metrics.unit)}</td><td>${formatForecastAdjustmentScorecardMetric(metrics.adjustedMae, metrics.unit)}</td><td>${formatForecastAdjustmentScorecardMetric(metrics.deltaMae, metrics.unit)}</td></tr>
          <tr><th scope="row">Bias</th><td>${formatForecastAdjustmentScorecardMetric(metrics.rawBias, metrics.unit)}</td><td>${formatForecastAdjustmentScorecardMetric(metrics.adjustedBias, metrics.unit)}</td><td>—</td></tr>
          <tr><th scope="row">RMSE</th><td>${formatForecastAdjustmentScorecardMetric(metrics.rawRmse, metrics.unit)}</td><td>${formatForecastAdjustmentScorecardMetric(metrics.adjustedRmse, metrics.unit)}</td><td>—</td></tr>
          <tr><th scope="row">95th percentile error</th><td>${formatForecastAdjustmentScorecardMetric(metrics.rawP95, metrics.unit)}</td><td>${formatForecastAdjustmentScorecardMetric(metrics.adjustedP95, metrics.unit)}</td><td>—</td></tr>
        </tbody>
      </table>
    </div>
    <p class="adjustment-scorecard-skill"><strong>MAE skill</strong> ${formatForecastAdjustmentScorecardPercent(metrics.skillPercent)} <span>95% interval ${interval}</span></p>
  `;
}

// render one card's evidence support
function renderForecastAdjustmentScorecardSupport(support: ForecastAdjustmentScorecardSupport): string {
  return `
    <dl class="adjustment-scorecard-support">
      <div><dt>Rows</dt><dd>${String(support.rowCount)} / ${String(support.targetRowCount)}</dd></div>
      <div><dt>Valid hours</dt><dd>${String(support.validHourCount)}</dd></div>
      <div><dt>Vintages</dt><dd>${String(support.vintageCount)}</dd></div>
      <div><dt>Dates</dt><dd>${String(support.dateCount)}</dd></div>
      <div><dt>Events</dt><dd>${String(support.eventCount)}</dd></div>
      <div><dt>Effective weight</dt><dd>${formatNumber(support.effectiveWeightSum, 2)}</dd></div>
      <div><dt>Wet rows</dt><dd>${String(support.wetRowCount)}</dd></div>
      <div><dt>Wet dates</dt><dd>${String(support.wetDateCount)}</dd></div>
      <div><dt>Gaps</dt><dd>${String(support.gapCount)}</dd></div>
      <div><dt>Fallbacks</dt><dd>${String(support.fallbackCount)}</dd></div>
      <div><dt>Excluded</dt><dd>${String(support.excludedCount)}</dd></div>
    </dl>
    <p class="adjustment-scorecard-reasons"><strong>Fallback reasons</strong> ${renderForecastAdjustmentScorecardReasons(support.fallbackReasons)}</p>
    <p class="adjustment-scorecard-reasons"><strong>Exclusion reasons</strong> ${renderForecastAdjustmentScorecardReasons(support.exclusionReasons)}</p>
  `;
}

// render bounded scorecard slices
function renderForecastAdjustmentScorecardSlices(slices: readonly ForecastAdjustmentScorecardSlice[]): string {
  // state honest slice absence
  if (slices.length === 0) {
    return '<p class="empty-panel">No slice metrics are available.</p>';
  }

  const rows = slices.map(
    // render one aggregate slice
    (slice) => `
      <tr>
        <th scope="row">${escapeHtml(slice.dimension)}: ${escapeHtml(slice.label)}</th>
        <td>${String(slice.rowCount)}</td>
        <td>${formatForecastAdjustmentScorecardMetric(slice.metrics.rawMae, slice.metrics.unit)}</td>
        <td>${formatForecastAdjustmentScorecardMetric(slice.metrics.adjustedMae, slice.metrics.unit)}</td>
        <td>${formatForecastAdjustmentScorecardPercent(slice.metrics.skillPercent)}</td>
      </tr>
    `,
  ).join("");
  return `<div class="adjustment-scorecard-table-scroll"><table class="adjustment-scorecard-metrics"><thead><tr><th scope="col">Slice</th><th scope="col">Rows</th><th scope="col">Raw MAE</th><th scope="col">Adjusted MAE</th><th scope="col">Skill</th></tr></thead><tbody>${rows}</tbody></table></div>`;
}

// render one nonqualification best-match diagnostic
function renderForecastAdjustmentBestMatchDiagnostic(
  diagnostic: ForecastAdjustmentBestMatchDiagnostic | null,
): string {
  // preserve honest absence when matching rows were not recorded
  if (diagnostic === null) {
    return '<p class="adjustment-scorecard-note"><strong>Best Match diagnostic</strong> Unavailable for a same-paired subcohort.</p>';
  }

  return `
    <div class="adjustment-scorecard-diagnostic">
      <h4>Best Match diagnostic</h4>
      <p>Nonqualification comparison on ${String(diagnostic.rowCount)} rows across ${String(diagnostic.dateCount)} dates.</p>
      <dl>
        <div><dt>Best Match raw MAE</dt><dd>${formatForecastAdjustmentScorecardMetric(diagnostic.bestMatchRawMae, diagnostic.unit)}</dd></div>
        <div><dt>Source raw MAE</dt><dd>${formatForecastAdjustmentScorecardMetric(diagnostic.sourceRawMae, diagnostic.unit)}</dd></div>
        <div><dt>Source adjusted MAE</dt><dd>${formatForecastAdjustmentScorecardMetric(diagnostic.sourceAdjustedMae, diagnostic.unit)}</dd></div>
      </dl>
    </div>
  `;
}

// render rain-only verification diagnostics
function renderForecastAdjustmentRainDiagnostics(
  diagnostics: ForecastAdjustmentRainDiagnostics | null,
): string {
  // omit rain diagnostics from temperature and wind cards
  if (diagnostics === null) {
    return "";
  }

  const thresholdRows = diagnostics.thresholds.map(
    // render one fixed event threshold
    (threshold) => `
      <tr>
        <th scope="row">${formatNumber(threshold.thresholdMmPerHour, 1)} mm/h</th>
        <td>${threshold.rawBrier === null ? "—" : formatNumber(threshold.rawBrier, 3)}</td>
        <td>${threshold.adjustedBrier === null ? "—" : formatNumber(threshold.adjustedBrier, 3)}</td>
        <td>${String(threshold.hits)} / ${String(threshold.misses)} / ${String(threshold.falseAlarms)}</td>
        <td>${formatForecastAdjustmentScorecardPercent(threshold.pod === null ? null : threshold.pod * 100)}</td>
        <td>${formatForecastAdjustmentScorecardPercent(threshold.far === null ? null : threshold.far * 100)}</td>
        <td>${formatForecastAdjustmentScorecardPercent(threshold.csi === null ? null : threshold.csi * 100)}</td>
      </tr>
    `,
  ).join("");
  const accumulationRows = diagnostics.accumulations.map(
    // render one fixed accumulation window
    (accumulation) => `
      <tr><th scope="row">${String(accumulation.hours)} hours</th><td>${String(accumulation.completeWindows)}</td><td>${accumulation.rawMae === null ? "—" : `${formatNumber(accumulation.rawMae, 2)} mm`}</td><td>${accumulation.adjustedMae === null ? "—" : `${formatNumber(accumulation.adjustedMae, 2)} mm`}</td></tr>
    `,
  ).join("");
  const reliability = diagnostics.thresholds.map(
    // render complete deciles for one threshold
    (threshold) => `
      <section class="adjustment-scorecard-reliability">
        <h5>${formatNumber(threshold.thresholdMmPerHour, 1)} mm/h</h5>
        <ol>${threshold.reliability.map(
          // preserve every empty or populated reliability bin
          (bin, index) => `<li><span>${String(index * 10)}–${String((index + 1) * 10)}%</span><span>${String(bin.count)} rows</span><span>predicted ${formatForecastAdjustmentScorecardPercent(bin.meanProbability === null ? null : bin.meanProbability * 100)}</span><span>observed ${formatForecastAdjustmentScorecardPercent(bin.observedFrequency === null ? null : bin.observedFrequency * 100)}</span></li>`,
        ).join("")}</ol>
      </section>
    `,
  ).join("");
  return `
    <details class="adjustment-scorecard-details adjustment-scorecard-rain">
      <summary>Rain diagnostics</summary>
      <dl class="adjustment-scorecard-support">
        <div><dt>Annual volume ratio</dt><dd>${diagnostics.annualBalancedVolumeRatio === null ? "—" : formatNumber(diagnostics.annualBalancedVolumeRatio, 3)}</dd></div>
        <div><dt>Winter volume ratio</dt><dd>${diagnostics.winterBalancedVolumeRatio === null ? "—" : formatNumber(diagnostics.winterBalancedVolumeRatio, 3)}</dd></div>
        <div><dt>Wet raw MAE</dt><dd>${formatForecastAdjustmentScorecardMetric(diagnostics.wetRawMae, "millimeters_per_hour")}</dd></div>
        <div><dt>Wet adjusted MAE</dt><dd>${formatForecastAdjustmentScorecardMetric(diagnostics.wetAdjustedMae, "millimeters_per_hour")}</dd></div>
        <div><dt>Heavy raw MAE</dt><dd>${formatForecastAdjustmentScorecardMetric(diagnostics.heavyRawMae, "millimeters_per_hour")}</dd></div>
        <div><dt>Heavy adjusted MAE</dt><dd>${formatForecastAdjustmentScorecardMetric(diagnostics.heavyAdjustedMae, "millimeters_per_hour")}</dd></div>
        <div><dt>Probability order violations</dt><dd>${String(diagnostics.probabilityOrderViolationCount)}</dd></div>
      </dl>
      <h4>Event thresholds</h4>
      <div class="adjustment-scorecard-table-scroll"><table class="adjustment-scorecard-metrics"><thead><tr><th scope="col">Threshold</th><th scope="col">Raw Brier</th><th scope="col">Adjusted Brier</th><th scope="col">Hits / misses / false alarms</th><th scope="col">POD</th><th scope="col">FAR</th><th scope="col">CSI</th></tr></thead><tbody>${thresholdRows}</tbody></table></div>
      <h4>Accumulations</h4>
      <div class="adjustment-scorecard-table-scroll"><table class="adjustment-scorecard-metrics"><thead><tr><th scope="col">Window</th><th scope="col">Complete</th><th scope="col">Raw MAE</th><th scope="col">Adjusted MAE</th></tr></thead><tbody>${accumulationRows}</tbody></table></div>
      <details class="adjustment-scorecard-details"><summary>Reliability bins</summary>${reliability}</details>
    </details>
  `;
}

// render one complete family review card
function renderForecastAdjustmentScorecardFamily(
  card: ForecastAdjustmentScorecardFamily,
  reportSha256: string,
  timezone: string,
): string {
  const title = card.family === "temperature" ? "Temperature" : card.family === "wind" ? "Wind" : "Rain";
  return `
    <article class="adjustment-scorecard-card" data-adjustment-family="${card.family}">
      <div class="adjustment-scorecard-card-heading">
        <div><p class="eyebrow">${escapeHtml(FORECAST_ADJUSTMENT_SCORECARD_STATE_LABELS.recommendation[card.recommendation])}</p><h3>${title}</h3></div>
        <span class="adjustment-scorecard-comparison" data-status="${card.comparisonState}">${escapeHtml(FORECAST_ADJUSTMENT_SCORECARD_STATE_LABELS.comparison[card.comparisonState])}</span>
      </div>
      <div class="adjustment-scorecard-statuses" aria-label="${title} review states">
        <span data-status="${card.supportState}">${escapeHtml(FORECAST_ADJUSTMENT_SCORECARD_STATE_LABELS.support[card.supportState])}</span>
        <span data-status="${card.qualificationState}">${escapeHtml(FORECAST_ADJUSTMENT_SCORECARD_STATE_LABELS.qualification[card.qualificationState])}</span>
        <span data-status="${card.servingState}">${escapeHtml(FORECAST_ADJUSTMENT_SCORECARD_STATE_LABELS.serving[card.servingState])}</span>
        <span data-status="${card.evidenceClass}">${escapeHtml(FORECAST_ADJUSTMENT_SCORECARD_STATE_LABELS.evidence[card.evidenceClass])}</span>
      </div>
      <dl class="adjustment-scorecard-identities">
        <div><dt>Serving identity</dt><dd>${renderForecastAdjustmentScorecardHash(card.servingIdentitySha256)}</dd></div>
        <div><dt>Family report</dt><dd>${renderForecastAdjustmentScorecardHash(reportSha256)}</dd></div>
        <div><dt>Evidence cutoff</dt><dd>${card.evidenceCutoffAt === null ? "Unavailable" : formatInstant(card.evidenceCutoffAt, timezone)}</dd></div>
      </dl>
      ${renderForecastAdjustmentScorecardSupport(card.support)}
      ${renderForecastAdjustmentScorecardMetrics(card.metrics)}
      ${card.family === "wind" ? '<p class="adjustment-scorecard-note"><strong>Best Match diagnostic</strong> Best Match is the primary wind baseline.</p>' : renderForecastAdjustmentBestMatchDiagnostic(card.bestMatchDiagnostic)}
      <details class="adjustment-scorecard-details"><summary>Slice metrics (${String(card.slices.length)})</summary>${renderForecastAdjustmentScorecardSlices(card.slices)}</details>
      ${renderForecastAdjustmentRainDiagnostics(card.rainDiagnostics)}
    </article>
  `;
}

// format one closed machine state for operator display
function formatForecastAdjustmentScorecardState(value: string): string {
  return value.replaceAll("_", " ").replace(/^./u,
    // capitalize only the first display character
    (first) => first.toUpperCase());
}

// render fixed v2 warning classes without accepting report prose
function renderForecastAdjustmentScorecardWarnings(
  warnings: ForecastAdjustmentScorecardV2["warnings"],
): string {
  const entries = (["source", "fallback", "gauge", "capture", "capacity"] as const).flatMap(
    // pair each warning code with its fixed category
    (warningClass) => warnings[warningClass].map(
      // render one validated safe warning code
      (warning) => `<li><strong>${formatForecastAdjustmentScorecardState(warningClass)}</strong> ${escapeHtml(warning.replaceAll("_", " "))}</li>`,
    ),
  );

  // state the verified absence of projected warnings
  if (entries.length === 0) {
    return '<p class="adjustment-scorecard-note">No source, fallback, gauge, capture or capacity warnings are projected.</p>';
  }
  return `<ul class="adjustment-scorecard-reasons">${entries.join("")}</ul>`;
}

// render the bounded sanitized automatic action history
function renderForecastAdjustmentScorecardHistory(
  history: readonly ForecastAdjustmentScorecardHistoryEntry[],
  timezone: string,
): string {
  // preserve honest history absence
  if (history.length === 0) {
    return '<p class="empty-panel">No automatic policy actions or attempts are available.</p>';
  }
  const rows = history.map(
    // render one immutable action reference without private result data
    (entry) => `<tr>
      <td>${formatInstant(entry.occurredAt, timezone)}</td>
      <td>${escapeHtml(formatForecastAdjustmentScorecardState(entry.policyDecision))}</td>
      <td>${escapeHtml(formatForecastAdjustmentScorecardState(entry.actionState))}</td>
      <td>${renderForecastAdjustmentScorecardHash(entry.actionProjectionSha256)}</td>
      <td>${renderForecastAdjustmentScorecardHash(entry.releaseManifestSha256)}</td>
    </tr>`,
  ).join("");
  return `<div class="adjustment-scorecard-table-scroll"><table class="adjustment-scorecard-metrics">
    <thead><tr><th scope="col">Time</th><th scope="col">Decision</th><th scope="col">Action</th><th scope="col">Projection</th><th scope="col">Release</th></tr></thead>
    <tbody>${rows}</tbody>
  </table></div>`;
}

// render v2 automatic policy state and immutable lineage
function renderForecastAdjustmentScorecardPolicy(
  scorecard: ForecastAdjustmentScorecardV2,
  timezone: string,
): string {
  const identityRows = FORECAST_ADJUSTMENT_SCORECARD_FAMILIES.map(
    // render one family's current safe identity projection
    (family) => `<tr>
      <th scope="row">${escapeHtml(formatForecastAdjustmentScorecardState(family))}</th>
      <td>${renderForecastAdjustmentScorecardHash(scorecard.identities.active[family])}</td>
      <td>${renderForecastAdjustmentScorecardHash(scorecard.identities.shadow[family])}</td>
      <td>${renderForecastAdjustmentScorecardHash(scorecard.identities.prior[family])}</td>
      <td>${renderForecastAdjustmentScorecardHash(scorecard.identities.raw[family])}</td>
    </tr>`,
  ).join("");
  return `
    <dl class="adjustment-scorecard-publication">
      <div><dt>Policy decision</dt><dd>${escapeHtml(formatForecastAdjustmentScorecardState(scorecard.policyDecision))}</dd></div>
      <div><dt>Action state</dt><dd>${escapeHtml(formatForecastAdjustmentScorecardState(scorecard.actionState))}</dd></div>
      <div><dt>Attempt</dt><dd>${escapeHtml(formatForecastAdjustmentScorecardState(scorecard.job.attemptState))}</dd></div>
      <div><dt>Success</dt><dd>${escapeHtml(formatForecastAdjustmentScorecardState(scorecard.job.successState))}</dd></div>
      <div><dt>Due</dt><dd>${escapeHtml(formatForecastAdjustmentScorecardState(scorecard.job.dueState))}</dd></div>
      <div><dt>Backlog</dt><dd>${escapeHtml(formatForecastAdjustmentScorecardState(scorecard.job.backlogState))}</dd></div>
      <div><dt>Operator state</dt><dd>${escapeHtml(formatForecastAdjustmentScorecardState(scorecard.job.operatorState))}</dd></div>
      <div><dt>Confirmation epochs</dt><dd>${String(scorecard.progress.confirmationCompletedEpochs)} / ${String(scorecard.progress.confirmationRequiredEpochs)}</dd></div>
      <div><dt>Rollback epochs</dt><dd>${String(scorecard.progress.rollbackCompletedEpochs)} / ${String(scorecard.progress.rollbackRequiredEpochs)}</dd></div>
      <div><dt>Rain capture expiry</dt><dd>${formatInstant(scorecard.progress.rainCaptureExpiresAt, timezone)}</dd></div>
      <div><dt>Action projection</dt><dd>${renderForecastAdjustmentScorecardHash(scorecard.actionProjectionSha256)}</dd></div>
      <div><dt>Action lineage</dt><dd>${renderForecastAdjustmentScorecardHash(scorecard.actionLineage.actionSha256)}</dd></div>
      <div><dt>Attempt identity</dt><dd>${renderForecastAdjustmentScorecardHash(scorecard.actionLineage.attemptSha256)}</dd></div>
      <div><dt>Previous action</dt><dd>${renderForecastAdjustmentScorecardHash(scorecard.actionLineage.predecessorActionSha256)}</dd></div>
      <div><dt>Release manifest</dt><dd>${renderForecastAdjustmentScorecardHash(scorecard.actionLineage.releaseManifestSha256)}</dd></div>
      <div><dt>Input manifest</dt><dd>${renderForecastAdjustmentScorecardHash(scorecard.inputs.inputManifestSha256)}</dd></div>
      <div><dt>Frontier</dt><dd>${renderForecastAdjustmentScorecardHash(scorecard.inputs.frontierSha256)}</dd></div>
      <div><dt>Daily report</dt><dd>${renderForecastAdjustmentScorecardHash(scorecard.inputs.reportSha256)}</dd></div>
    </dl>
    <h3>Model identities</h3>
    <div class="adjustment-scorecard-table-scroll"><table class="adjustment-scorecard-metrics">
      <thead><tr><th scope="col">Family</th><th scope="col">Active</th><th scope="col">Shadow</th><th scope="col">Prior</th><th scope="col">Raw</th></tr></thead>
      <tbody>${identityRows}</tbody>
    </table></div>
    <h3>Warnings</h3>
    ${renderForecastAdjustmentScorecardWarnings(scorecard.warnings)}
    <details class="adjustment-scorecard-details"><summary>Automatic action history (${String(scorecard.history.length)})</summary>${renderForecastAdjustmentScorecardHistory(scorecard.history, timezone)}</details>
  `;
}

// render the authenticated administrator scorecard
function renderForecastAdjustmentScorecard(state: DashboardState): string {
  const loadState = state.adminAdjustmentScorecardState ?? "unavailable";
  const timezone = state.selectedSite?.timezone ?? PRODUCT_SITE.timezone;

  // reserve all three family cards during the independent protected read
  if (loadState === "loading" && state.loading) {
    return `
      <section class="panel adjustment-scorecard skeleton-region" aria-labelledby="adjustment-scorecard-heading" aria-busy="true">
        <div class="section-heading"><div><p class="eyebrow">Policy status</p><h2 id="adjustment-scorecard-heading">Adjustment performance</h2></div></div>
        <p class="adjustment-scorecard-guardrail"><strong>Loading protected policy evidence.</strong> Scorecards are reporting projections, not action authority.</p>
        <div class="adjustment-scorecard-grid">${Array.from({ length: 3 },
          // reserve one stable family card
          () => `<div class="adjustment-scorecard-card">${renderSkeletonFields()}</div>`,
        ).join("")}</div>
      </section>
    `;
  }

  // distinguish an expired protected session
  if (loadState === "unauthorized") {
    return `
      <section class="panel adjustment-scorecard" aria-labelledby="adjustment-scorecard-heading" data-scorecard-state="unauthorized">
        <div class="section-heading"><div><p class="eyebrow">Policy status</p><h2 id="adjustment-scorecard-heading">Adjustment performance</h2></div></div>
        <p class="adjustment-scorecard-guardrail"><strong>Protected report unavailable.</strong> Scorecards are reporting projections, not action authority.</p>
        <p class="empty-panel">Administrator session is unavailable. Sign in again to review performance.</p>
      </section>
    `;
  }

  const scorecard = state.adminAdjustmentScorecard ?? null;

  // collapse absent, stale or corrupt publications into one safe state
  if (loadState !== "ready" || scorecard === null) {
    return `
      <section class="panel adjustment-scorecard" aria-labelledby="adjustment-scorecard-heading" data-scorecard-state="unavailable">
        <div class="section-heading"><div><p class="eyebrow">Policy status</p><h2 id="adjustment-scorecard-heading">Adjustment performance</h2></div></div>
        <p class="adjustment-scorecard-guardrail"><strong>No validated policy report.</strong> A missing, stale or corrupt report cannot support an action.</p>
        <p class="empty-panel">No validated scorecard is available.</p>
      </section>
    `;
  }

  const cards = FORECAST_ADJUSTMENT_SCORECARD_FAMILIES.map(
    // render all existing model tracks without action controls
    (family) => renderForecastAdjustmentScorecardFamily(
      scorecard.families[family],
      scorecard.inputs.reportSha256s[family],
      timezone,
    ),
  ).join("");
  const isV2 = scorecard.contractVersion === "forecast-adjustment-scorecard/v2";
  const publicationState = state.adminAdjustmentScorecardPublicationState ??
    (isV2 ? "current" : "legacy_display");
  const eyebrow = isV2 ? "Automatic policy" : "Legacy report";
  const guardrail = publicationState === "pending_unapplied" && isV2
    ? "<strong>Pending and unapplied.</strong> This sanitized projection has not replaced the legacy current report and cannot itself authorize or execute changes."
    : isV2
      ? `<strong>${escapeHtml(formatForecastAdjustmentScorecardState(scorecard.policyDecision))} · ${escapeHtml(formatForecastAdjustmentScorecardState(scorecard.actionState))}.</strong> This sanitized scorecard reports immutable policy lineage; it does not itself authorize or execute changes.`
    : "<strong>Historical display only.</strong> Legacy v1 scorecards cannot authorize actions or serving changes.";
  return `
    <section class="panel adjustment-scorecard" aria-labelledby="adjustment-scorecard-heading" data-scorecard-state="ready" data-scorecard-publication-state="${publicationState}">
      <div class="section-heading"><div><p class="eyebrow">${eyebrow}</p><h2 id="adjustment-scorecard-heading">Adjustment performance</h2></div></div>
      <p class="adjustment-scorecard-guardrail">${guardrail}</p>
      <dl class="adjustment-scorecard-publication">
        <div><dt>Evaluation dates</dt><dd>${escapeHtml(scorecard.inputs.localDateFrom)} through ${escapeHtml(scorecard.inputs.localDateTo)}</dd></div>
        <div><dt>Target cutoff</dt><dd>${formatInstant(scorecard.inputs.targetCutoffAt, timezone)}</dd></div>
        <div><dt>Generated</dt><dd>${formatInstant(scorecard.generatedAt, timezone)}</dd></div>
        <div><dt>Valid through</dt><dd>${formatInstant(scorecard.validThrough, timezone)}</dd></div>
        <div><dt>Source revision</dt><dd><code>${escapeHtml(scorecard.inputs.sourceRevision)}</code></dd></div>
        <div><dt>Evidence manifest</dt><dd>${renderForecastAdjustmentScorecardHash(scorecard.inputs.adjustmentEvidenceManifestSha256)}</dd></div>
        <div><dt>Evidence watermark</dt><dd>${renderForecastAdjustmentScorecardHash(scorecard.inputs.adjustmentEvidenceWatermarkSha256)}</dd></div>
        <div><dt>Training manifest</dt><dd>${renderForecastAdjustmentScorecardHash(scorecard.inputs.forecastTrainingManifestSha256)}</dd></div>
      </dl>
      ${isV2 ? renderForecastAdjustmentScorecardPolicy(scorecard, timezone) : ""}
      <div class="adjustment-scorecard-grid">${cards}</div>
    </section>
  `;
}

// render three persisted forecast controls
function renderForecastAdjustmentAdmin(state: DashboardState): string {
  const settings = state.forecastAdjustmentSettings ?? null;
  // avoid presenting unknown adjustment settings as switched off
  if (settings === null && state.loading) {
    return `
      <section class="panel forecast-adjustment-admin skeleton-region" aria-labelledby="forecast-adjustment-admin-heading" aria-busy="true">
        <div class="section-heading"><div><p class="eyebrow">Administration</p><h2 id="forecast-adjustment-admin-heading">Forecast adjustments</h2></div></div>
        ${renderSkeletonFields()}
      </section>
    `;
  }

  const disabled = settings === null || state.adminAdjustmentSettingsSaving;
  const message = state.adminAdjustmentSettingsMessage ?? null;

  return `
    <section class="panel forecast-adjustment-admin" aria-labelledby="forecast-adjustment-admin-heading">
      <div class="section-heading">
        <div><p class="eyebrow">Administration</p><h2 id="forecast-adjustment-admin-heading">Forecast adjustments</h2></div>
      </div>
      <p class="property-admin-intro">Choose which adjustments are available in the forecast. Visitors can still switch between adjusted and raw values.</p>
      <form data-admin-forecast-adjustments>
        <label><input type="checkbox" name="temperature"${settings?.temperature ? " checked" : ""}${disabled ? " disabled" : ""}><span>Temperature</span></label>
        <label><input type="checkbox" name="wind"${settings?.wind ? " checked" : ""}${disabled ? " disabled" : ""}><span>Wind</span></label>
        <label><input type="checkbox" name="rain"${settings?.rain ? " checked" : ""}${disabled ? " disabled" : ""}><span>Rain</span></label>
        <div class="forecast-adjustment-admin-actions">
          <button type="submit"${disabled ? " disabled" : ""}>Save adjustments</button>
          <span role="status" aria-live="polite">${state.adminAdjustmentSettingsSaving ? '<span class="skeleton-line skeleton-action" aria-hidden="true"></span><span class="sr-only">Saving forecast adjustments…</span>' : message === null ? "" : escapeHtml(message)}</span>
        </div>
      </form>
    </section>
  `;
}

// render the protected name and position editor
function renderPropertySensorAdmin(state: DashboardState): string {
  const site = state.selectedSite ?? PRODUCT_SITE;
  const sensors = propertySensorSnapshots(state);

  // reserve the editor until both sensor channels and positions are known
  if (state.loading && (sensors.length === 0 || state.propertySensorLayout === null)) {
    return `
      <section class="panel property-admin skeleton-region" aria-labelledby="property-admin-heading" aria-busy="true">
        ${renderPropertyAdminHeading()}
        <div class="property-admin-layout">
          ${renderSkeletonFields()}
          <div class="property-admin-editor">
            ${renderSkeletonFields()}
            <div class="property-admin-map skeleton-map" aria-hidden="true"><span class="skeleton-map-shape"></span></div>
          </div>
        </div>
      </section>
    `;
  }

  // explain the first ingestion wait honestly
  if (sensors.length === 0) {
    return `
      <section class="panel property-admin" aria-labelledby="property-admin-heading">
        ${renderPropertyAdminHeading()}
        <p class="empty-panel">No EcoWitt sensor channels are reporting yet.</p>
      </section>
    `;
  }

  const selectedKey = state.selectedPropertySensorKey ?? sensors[0]?.key ?? "";
  const selected = sensors.find(
    // retain the selected reporting sensor
    (sensor) => sensor.key === selectedKey,
  ) ?? sensors[0];

  // preserve the checked non-empty sensor boundary
  if (selected === undefined) {
    return "";
  }

  const layout = (state.propertySensorLayout ?? []).find(
    // load the selected persisted placement
    (entry) => entry.sensorKey === selected.key,
  );
  const latitude = layout?.latitude ?? site.latitude;
  const longitude = layout?.longitude ?? site.longitude;
  const displayName = layout?.displayName ?? defaultPropertySensorName(selected);
  const icon = layout?.icon ?? defaultPropertySensorIcon(selected);
  const viewport = createFixedMapViewport(
    site.latitude,
    site.longitude,
    PROPERTY_MAP_ZOOM,
    PROPERTY_MAP_WIDTH,
    PROPERTY_MAP_HEIGHT,
  );
  const point = projectMapPoint(latitude, longitude, viewport);
  const sensorRows = sensors.map(
    // render one selectable hardware channel
    (sensor) => {
      const saved = (state.propertySensorLayout ?? []).find(
        // resolve the visible persisted name
        (entry) => entry.sensorKey === sensor.key,
      );
      const sensorIcon = saved?.icon ?? defaultPropertySensorIcon(sensor);
      return `
        <li>
          <button type="button" data-property-sensor-select="${escapeHtml(sensor.key)}" aria-pressed="${String(sensor.key === selected.key)}">
            <span class="property-admin-sensor-icon">${renderMaterialIcon(propertySensorMaterialIcon(sensorIcon))}</span>
            <span><strong>${escapeHtml(saved?.displayName ?? defaultPropertySensorName(sensor))}</strong><small>${escapeHtml(sensor.model)}${sensor.channel === null ? "" : ` · channel ${String(sensor.channel)}`} · ${saved === undefined ? "needs placement" : "placed"}</small></span>
          </button>
        </li>
      `;
    },
  ).join("");

  return `
    <section class="panel property-admin" aria-labelledby="property-admin-heading">
      ${renderPropertyAdminHeading()}
      <p class="property-admin-intro">Select a reporting EcoWitt sensor, give it a useful name, then tap its physical location on the map.</p>
      <div class="property-admin-layout">
        <ol class="property-admin-sensors">${sensorRows}</ol>
        <div class="property-admin-editor">
          <form data-property-sensor-form data-sensor-key="${escapeHtml(selected.key)}">
            <label><span>Display name</span><input name="displayName" maxlength="80" required value="${escapeHtml(displayName)}"></label>
            ${renderPropertySensorIconPicker(icon)}
            <div class="property-coordinate-fields">
              <label><span>Latitude</span><input name="latitude" type="number" min="-85" max="85" step="0.000001" required value="${latitude.toFixed(6)}"></label>
              <label><span>Longitude</span><input name="longitude" type="number" min="-180" max="180" step="0.000001" required value="${longitude.toFixed(6)}"></label>
            </div>
            <div class="property-admin-map">
              <svg data-property-position-map data-property-interactive-map data-property-map-width="${String(PROPERTY_MAP_WIDTH)}" data-property-map-height="${String(PROPERTY_MAP_HEIGHT)}" data-viewport-left="${viewport.left.toFixed(6)}" data-viewport-top="${viewport.top.toFixed(6)}" data-viewport-zoom="${String(viewport.zoom)}" viewBox="0 0 ${PROPERTY_MAP_WIDTH} ${PROPERTY_MAP_HEIGHT}" role="application" aria-label="Tap to place ${escapeHtml(displayName)}">
                <g data-property-map-world>
                  <g class="map-tile-layer" aria-hidden="true">${renderPropertyMapTiles(state.propertyMapLayer, viewport)}</g>
                </g>
                <g data-property-map-anchor data-property-map-x="${point.x.toFixed(2)}" data-property-map-y="${point.y.toFixed(2)}" data-property-position-marker transform="translate(${point.x.toFixed(2)} ${point.y.toFixed(2)})" class="property-position-marker" aria-hidden="true">
                  <path class="property-position-marker-pin" d="M0 0C-2.7-4.4-14-17.2-14-27A14 14 0 1 1 14-27C14-17.2 2.7-4.4 0 0Z"/>
                  <circle class="property-position-marker-core" cy="-27" r="9"/>
                  <text data-property-position-marker-icon class="property-position-marker-icon" x="0" y="-22">${propertySensorMaterialIcon(icon)}</text>
                </g>
              </svg>
              ${renderPropertyMapLayerControls(state.propertyMapLayer, viewport)}
              ${renderPropertyMapZoomControls()}
            </div>
            ${renderPropertyMapAttribution(state.propertyMapLayer)}
            <div class="property-admin-actions"><span role="status" aria-live="polite">${state.loading ? '<span class="skeleton-line skeleton-action" aria-hidden="true"></span><span class="sr-only">Saving sensor…</span>' : layout === undefined ? "Not placed" : `Updated ${formatInstant(layout.updatedAt, site.timezone)}`}</span><button type="submit"${state.loading ? " disabled" : ""}>${renderSaveIcon()} Save sensor</button></div>
          </form>
        </div>
      </div>
    </section>
  `;
}

// render the authenticated admin heading and logout action
function renderPropertyAdminHeading(): string {
  return `
    <div class="section-heading">
      <div><p class="eyebrow">Administration</p><h2 id="property-admin-heading">Property sensors</h2></div>
      <form class="admin-logout-form" action="/admin/logout" method="post"><button type="submit">${renderMaterialIcon("logout")}<span>Log out</span></button></form>
    </div>
  `;
}

// render one compact illustrated map-icon selector
function renderPropertySensorIconPicker(selectedIcon: PropertySensorIcon): string {
  return `
    <fieldset class="property-icon-picker">
      <legend>Map icon</legend>
      <div class="property-icon-options">
        ${PROPERTY_SENSOR_ICON_OPTIONS.map(
          // render every supported sensor category
          (option) => `
            <label>
              <input type="radio" name="icon" value="${option.icon}" aria-label="${escapeHtml(option.label)}"${option.icon === selectedIcon ? " checked" : ""}>
              <span>${renderMaterialIcon(option.material)}<small>${escapeHtml(option.label)}</small></span>
            </label>
          `,
        ).join("")}
      </div>
    </fieldset>
  `;
}

// choose a useful initial icon from the sensor's reported measurements
function defaultPropertySensorIcon(sensor: PropertySensorSnapshot): PropertySensorIcon {
  const readings = sensor.readings;

  // prioritize dedicated particulate sensors
  if (readings.pm25MicrogramsPerCubicMeter !== undefined) {
    return "air-quality";
  }

  // represent multi-sensor weather arrays by wind
  if (readings.windSpeedMps !== undefined || readings.windGustMps !== undefined) {
    return "wind";
  }

  // represent dedicated rain gauges by precipitation
  if (
    readings.precipitationRateMmPerHour !== undefined ||
    readings.dailyPrecipitationMm !== undefined
  ) {
    return "rain";
  }

  return "temperature";
}

// map one persisted category onto the bundled Material glyph
function propertySensorMaterialIcon(icon: PropertySensorIcon): MaterialIconName {
  return PROPERTY_SENSOR_ICON_OPTIONS.find(
    // resolve the exact reviewed category
    (option) => option.icon === icon,
  )?.material ?? "device_thermostat";
}

// accept only a supported persisted sensor category
function isPropertySensorIcon(value: unknown): value is PropertySensorIcon {
  return typeof value === "string" && PROPERTY_SENSOR_ICON_OPTIONS.some(
    // match one reviewed category
    (option) => option.icon === value,
  );
}

// collect each latest EcoWitt hardware snapshot once
function propertySensorSnapshots(state: DashboardState): readonly PropertySensorSnapshot[] {
  const sensors = new Map<string, PropertySensorSnapshot>();

  // inspect current records in response priority order
  for (const record of state.current) {
    // retain every first occurrence by stable hardware key
    for (const sensor of record.metadata.provider?.propertySensors ?? []) {
      // preserve the newest response occurrence
      if (!sensors.has(sensor.key)) {
        sensors.set(sensor.key, sensor);
      }
    }
  }

  return [...sensors.values()];
}

// render one map-positioned sensor label and primary reading
function renderPropertySensorMarker(
  layout: PropertySensorLayout,
  sensor: PropertySensorSnapshot,
  viewport: MapViewport,
  units: UnitPreferences,
  selected: boolean,
  offset: Readonly<{ x: number; y: number }>,
): string {
  const point = projectMapPoint(layout.latitude, layout.longitude, viewport);
  const reading = primaryPropertySensorReading(sensor, units);
  const icon = layout.icon ?? defaultPropertySensorIcon(sensor);
  return `
    <a class="property-sensor-marker${selected ? " selected" : ""}" href="#property-sensor-details-${escapeHtml(sensor.key)}" data-property-sensor-view="${escapeHtml(sensor.key)}" data-property-map-anchor data-property-map-x="${point.x.toFixed(2)}" data-property-map-y="${point.y.toFixed(2)}" transform="translate(${point.x.toFixed(2)} ${point.y.toFixed(2)})" aria-label="Show details for ${escapeHtml(layout.displayName)}" aria-expanded="${String(selected)}" aria-controls="property-sensor-details-${escapeHtml(sensor.key)}">
      <line class="property-sensor-marker-leader" x1="0" y1="0" x2="${offset.x.toFixed(2)}" y2="${offset.y.toFixed(2)}"/>
      <circle class="property-sensor-marker-anchor" r="3"/>
      <g class="property-sensor-marker-head" transform="translate(${offset.x.toFixed(2)} ${offset.y.toFixed(2)})">
        <circle class="property-sensor-marker-hit" r="18"/>
        <circle class="property-sensor-marker-dot" r="14"/>
        <text x="0" y="5" class="property-sensor-marker-icon">${propertySensorMaterialIcon(icon)}</text>
      </g>
      <title>${escapeHtml(layout.displayName)} · ${escapeHtml(reading)}</title>
    </a>
  `;
}

// render one selectable property sensor list row
function renderPropertySensorListItem(
  layout: PropertySensorLayout,
  sensor: PropertySensorSnapshot,
  units: UnitPreferences,
  selected: boolean,
): string {
  const icon = layout.icon ?? defaultPropertySensorIcon(sensor);
  const details = selected ? renderPropertySensorDetails(layout, sensor, units) : "";
  return `
    <li class="property-sensor-item${selected ? " selected" : ""}">
      <button class="property-sensor-select" type="button" data-property-sensor-view="${escapeHtml(sensor.key)}" aria-expanded="${String(selected)}" aria-controls="property-sensor-details-${escapeHtml(sensor.key)}">
        <span class="property-sensor-list-icon">${renderMaterialIcon(propertySensorMaterialIcon(icon))}</span>
        <span class="property-sensor-label"><strong>${escapeHtml(layout.displayName)}</strong><span>${escapeHtml(primaryPropertySensorReading(sensor, units))}</span></span>
      </button>
      ${details}
    </li>
  `;
}

// render every available reading and hardware detail
function renderPropertySensorDetails(
  layout: PropertySensorLayout,
  sensor: PropertySensorSnapshot,
  units: UnitPreferences,
): string {
  const readings = propertySensorReadingLabels(sensor, units);
  return `
    <div class="property-sensor-details" id="property-sensor-details-${escapeHtml(sensor.key)}" data-property-sensor-details="${escapeHtml(sensor.key)}" role="region" aria-live="polite" aria-label="Details for ${escapeHtml(layout.displayName)}">
      <div class="property-sensor-readings">
        ${readings.map(
          // render every current sensor measurement
          (reading) => `<span>${escapeHtml(reading)}</span>`,
        ).join("")}
      </div>
      <p class="property-sensor-meta"><strong>EcoWitt ${escapeHtml(sensor.model)}</strong>${sensor.channel === null ? "" : ` · channel ${String(sensor.channel)}`} · ${escapeHtml(sensor.key)}</p>
      <p class="property-sensor-meta"><strong>Position</strong> ${layout.latitude.toFixed(6)}, ${layout.longitude.toFixed(6)}</p>
    </div>
  `;
}

// name one unconfigured sensor from its hardware identity
function defaultPropertySensorName(sensor: PropertySensorSnapshot): string {
  return `${sensor.model}${sensor.channel === null ? "" : ` channel ${String(sensor.channel)}`}`;
}

// choose one compact reading for a map label
function primaryPropertySensorReading(
  sensor: PropertySensorSnapshot,
  units: UnitPreferences,
): string {
  const soilMoisture = sensor.readings.soilMoisturePercent;

  // prioritize soil moisture for dedicated soil probes
  if (soilMoisture !== undefined) {
    return `Moisture ${formatNumber(soilMoisture)} %`;
  }

  return propertySensorReadingLabels(sensor, units)[0] ?? "Reporting";
}

// format every known sensor reading consistently
function propertySensorReadingLabels(
  sensor: PropertySensorSnapshot,
  units: UnitPreferences,
): readonly string[] {
  const labels: string[] = [];
  const readings = sensor.readings;
  // format one configurable measurement
  const pushMeasurement = (
    key: string,
    label: string,
    kind: keyof UnitPreferences,
  ): void => {
    const value = readings[key];

    // omit unavailable provider readings
    if (value === undefined) {
      return;
    }

    const formatted = formatMeasurement(value, kind, units);
    labels.push(`${label} ${formatted.value}${formatted.unit.length === 0 ? "" : ` ${formatted.unit}`}`);
  };
  // format one fixed-unit measurement
  const pushFixed = (key: string, label: string, unit: string): void => {
    const value = readings[key];

    // omit unavailable provider readings
    if (value === undefined) {
      return;
    }

    labels.push(`${label} ${formatNumber(value)}${unit.length === 0 ? "" : ` ${unit}`}`);
  };
  pushMeasurement("temperatureC", "Temp", "temperature");
  pushFixed("relativeHumidityPercent", "Humidity", "%");
  pushFixed("soilMoisturePercent", "Moisture", "%");
  pushFixed("soilElectricalConductivityMicrosiemensPerCm", "EC", "µS/cm");
  pushMeasurement("windSpeedMps", "Wind", "windSpeed");
  pushMeasurement("windGustMps", "Gust", "windSpeed");
  const rainRate = readings.precipitationRateMmPerHour;

  // format rain rate with the precipitation preference
  if (rainRate !== undefined) {
    const formatted = formatPrecipitationRate(rainRate, units);
    labels.push(`Rain ${formatted.value} ${formatted.unit}`);
  }
  pushMeasurement("dailyPrecipitationMm", "Accumulation", "precipitation");
  pushMeasurement("pressureHpa", "Pressure", "pressure");
  pushFixed("pm25MicrogramsPerCubicMeter", "PM2.5", "");
  pushFixed("uvIndex", "UV", "");
  pushFixed("solarRadiationWm2", "Solar", "W/m²");
  pushMeasurement("blackGlobeTemperatureC", "Globe", "temperature");
  pushMeasurement("wetBulbGlobeTemperatureC", "WBGT", "temperature");
  pushFixed("windDirectionDegrees", "Direction", "°");
  return labels;
}

// render tiled nearby station geography and latest readings
function renderStationMap(state: DashboardState): string {
  // reserve map geometry until station readings are ready
  if (state.loading) {
    return renderStationMapSkeleton();
  }

  const site = state.selectedSite;

  // wait for site geometry
  if (site === null) {
    return "";
  }

  const stations = site.stations.filter(
    // map physical public stations only
    (station) =>
      station.kind === "physical" &&
      station.sources.some((source) => source.kind === "physical_sensor") &&
      Number.isFinite(station.latitude) &&
      Number.isFinite(station.longitude),
  );

  // render an honest station catalog state
  if (stations.length === 0) {
    return `
      <section class="panel" aria-labelledby="map-heading">
        <div class="section-heading"><div><p class="eyebrow">Local network</p><h2 id="map-heading">Nearby station map</h2></div></div>
        <p class="empty-panel">Nearby station coordinates are not available yet.</p>
      </section>
    `;
  }

  const viewport = createMapViewport([
    { latitude: site.latitude, longitude: site.longitude },
    ...stations,
  ]);
  const sitePoint = projectMapPoint(site.latitude, site.longitude, viewport);
  const stationHitAreas = stations.map(
    // render broad touch targets below every visible marker
    (station) => renderStationHitArea(station, viewport),
  ).join("");
  const stationMarkers = stations.map(
    // render each physical station marker
    (station, index) => renderStationMarker(station, index, viewport, state),
  ).join("");
  const stationRows = stations.map(
    // render each physical station label
    (station, index) => renderStationListItem(station, index, site, state),
  ).join("");

  return `
    <section class="panel station-map-panel" aria-labelledby="map-heading">
      <div class="section-heading">
        <div><p class="eyebrow">Local network</p><h2 id="map-heading">Nearby station map</h2></div>
      </div>
      <div class="station-map-layout">
        <div class="station-map">
          <div class="station-map-canvas" role="group" aria-label="Map of nearby public weather stations with ${escapeHtml(mapLayerLabel(state.mapLayer).toLowerCase())} tiles">
            <svg class="station-map-svg" viewBox="0 0 ${STATION_MAP_WIDTH} ${STATION_MAP_HEIGHT}" role="group" aria-label="Nearby weather station markers">
              <g class="map-tile-layer" aria-hidden="true">
                ${renderMapTiles(state.mapLayer, viewport)}
              </g>
              <g class="station-map-overlay">
                <text x="610" y="28" class="map-north" aria-hidden="true">N</text>
                <path d="M616 56V34M610 42l6-8 6 8" class="map-north-arrow" aria-hidden="true"/>
                <circle cx="${sitePoint.x.toFixed(2)}" cy="${sitePoint.y.toFixed(2)}" r="10" class="farm-marker" aria-hidden="true"/>
                ${stationHitAreas}
                ${stationMarkers}
              </g>
            </svg>
            ${renderMapLayerControls(state, viewport)}
          </div>
          ${renderMapAttribution(state.mapLayer)}
        </div>
        <ol class="nearby-station-list">
          ${stationRows}
        </ol>
      </div>
    </section>
  `;
}

// reserve the complete station map layout
function renderStationMapSkeleton(): string {
  return `
    <section class="panel station-map-panel skeleton-region" aria-labelledby="map-heading" aria-busy="true">
      <div class="section-heading">
        <div><p class="eyebrow">Local network</p><h2 id="map-heading">Nearby station map</h2></div>
      </div>
      <div class="station-map-layout">
        <div class="station-map skeleton-map" aria-hidden="true">
          <div class="station-map-canvas"><span class="skeleton-map-shape"></span></div>
          <p class="map-attribution"><span class="skeleton-attribution">© OpenStreetMap contributors</span></p>
        </div>
        <ol class="nearby-station-list skeleton-station-list" aria-hidden="true">
          ${Array.from({ length: 11 },
            // reserve the visible station rows
            (_, index) => `
              <li><button class="nearby-station-select" type="button" disabled><span class="station-number">${String(index + 1)}</span><span class="station-label"><strong class="skeleton-station-name">Station name</strong><span class="station-reading skeleton-station-reading">00°F · 0.0 mi</span></span></button></li>
            `,
          ).join("")}
        </ol>
      </div>
    </section>
  `;
}

// render actual map tiles as an overlaid style picker
function renderMapLayerControls(
  state: DashboardState,
  viewport: MapViewport,
): string {
  return `
    <div class="map-style-controls" role="group" aria-label="Map style">
      ${(["roads", "topo", "satellite"] as const).map(
        // illustrate each reviewed base layer
        (layer) => {
          const sourceZoom = mapLayerSourceZoom(layer, viewport.zoom);
          const sourceScale = 2 ** (viewport.zoom - sourceZoom);
          const previewColumn = Math.floor(
            (viewport.left + STATION_MAP_WIDTH / 2) / (STATION_MAP_TILE_SIZE * sourceScale),
          );
          const previewRow = Math.floor(
            (viewport.top + STATION_MAP_HEIGHT / 2) / (STATION_MAP_TILE_SIZE * sourceScale),
          );
          return `
            <button class="map-style-button" type="button" data-map-layer="${layer}" aria-pressed="${String(state.mapLayer === layer)}">
              <img src="${escapeHtml(mapTileUrl(layer, sourceZoom, previewColumn, previewRow))}" alt="" width="96" height="64" loading="lazy" referrerpolicy="origin">
              <span>${mapLayerLabel(layer)}</span>
            </button>
          `;
        },
      ).join("")}
    </div>
  `;
}

// render farm-scale tile choices inside the bounded viewport
function renderPropertyMapLayerControls(
  selectedLayer: MapLayer,
  viewport: MapViewport,
): string {
  return `
    <div class="map-style-controls property-map-style-controls" role="group" aria-label="Property map style">
      ${(["roads", "topo", "satellite"] as const).map(
        // illustrate each property base layer
        (layer) => {
          const sourceZoom = mapLayerSourceZoom(layer, viewport.zoom);
          const sourceScale = 2 ** (viewport.zoom - sourceZoom);
          const previewColumn = Math.floor(
            (viewport.left + PROPERTY_MAP_WIDTH / 2) / (STATION_MAP_TILE_SIZE * sourceScale),
          );
          const previewRow = Math.floor(
            (viewport.top + PROPERTY_MAP_HEIGHT / 2) / (STATION_MAP_TILE_SIZE * sourceScale),
          );
          const previewUrl = layer === "satellite"
            ? propertySatelliteImageUrl(viewport)
            : mapTileUrl(layer, sourceZoom, previewColumn, previewRow);
          return `
            <button class="map-style-button" type="button" data-property-map-layer="${layer}" aria-pressed="${String(selectedLayer === layer)}">
              <img src="${escapeHtml(previewUrl)}" alt="" width="96" height="64" loading="lazy" referrerpolicy="origin">
              <span>${mapLayerLabel(layer)}</span>
            </button>
          `;
        },
      ).join("")}
    </div>
  `;
}

// render dependency-free property zoom controls
function renderPropertyMapZoomControls(): string {
  return `
    <div class="property-map-zoom-controls" role="group" aria-label="Property map zoom">
      <button type="button" data-property-map-zoom="in" aria-label="Zoom in">+</button>
      <button type="button" data-property-map-zoom="out" aria-label="Zoom out">−</button>
      <button type="button" data-property-map-zoom="reset" aria-label="Reset map">1×</button>
    </div>
  `;
}

// create one fixed Web Mercator viewport around every marker
function createMapViewport(
  coordinates: readonly Readonly<{ latitude: number; longitude: number }>[],
): MapViewport {
  const points = coordinates.map(
    // project each checked coordinate once
    (coordinate) => webMercatorPoint(
      coordinate.latitude,
      coordinate.longitude,
      STATION_MAP_ZOOM,
    ),
  );
  const xCoordinates = points.map(
    // collect horizontal pixels
    (point) => point.x,
  );
  const yCoordinates = points.map(
    // collect vertical pixels
    (point) => point.y,
  );
  const minimumX = Math.min(...xCoordinates);
  const maximumX = Math.max(...xCoordinates);
  const minimumY = Math.min(...yCoordinates);
  const maximumY = Math.max(...yCoordinates);
  return {
    left: (minimumX + maximumX - STATION_MAP_WIDTH) / 2,
    top: (minimumY + maximumY - STATION_MAP_HEIGHT) / 2,
    zoom: STATION_MAP_ZOOM,
  };
}

// create one fixed-size Web Mercator viewport around a center point
function createFixedMapViewport(
  latitude: number,
  longitude: number,
  zoom: number,
  width: number,
  height: number,
): MapViewport {
  const center = webMercatorPoint(latitude, longitude, zoom);
  return {
    left: center.x - width / 2,
    top: center.y - height / 2,
    zoom,
  };
}

// project WGS84 station coordinates into the tile viewport
function projectMapPoint(
  latitude: number,
  longitude: number,
  viewport: MapViewport,
): Readonly<{ x: number; y: number }> {
  const point = webMercatorPoint(latitude, longitude, viewport.zoom);
  return {
    x: point.x - viewport.left,
    y: point.y - viewport.top,
  };
}

// project one coordinate into global Web Mercator pixels
function webMercatorPoint(
  latitude: number,
  longitude: number,
  zoom: number,
): Readonly<{ x: number; y: number }> {
  const boundedLatitude = Math.max(-85.051_129, Math.min(85.051_129, latitude));
  const worldSize = STATION_MAP_TILE_SIZE * 2 ** zoom;
  const latitudeRadians = boundedLatitude * Math.PI / 180;
  return {
    x: (longitude + 180) / 360 * worldSize,
    y: (1 - Math.asinh(Math.tan(latitudeRadians)) / Math.PI) / 2 * worldSize,
  };
}

// invert one viewport pixel into WGS84 coordinates
function inverseMapPoint(
  x: number,
  y: number,
  viewport: MapViewport,
): Readonly<{ latitude: number; longitude: number }> {
  const worldSize = STATION_MAP_TILE_SIZE * 2 ** viewport.zoom;
  const globalX = viewport.left + x;
  const globalY = viewport.top + y;
  return {
    latitude: Math.atan(Math.sinh(Math.PI * (1 - 2 * globalY / worldSize))) * 180 / Math.PI,
    longitude: globalX / worldSize * 360 - 180,
  };
}

// convert one global map pixel into Web Mercator meters
function webMercatorPixelMeters(
  x: number,
  y: number,
  zoom: number,
): Readonly<{ x: number; y: number }> {
  const worldSize = STATION_MAP_TILE_SIZE * 2 ** zoom;
  const span = WEB_MERCATOR_LIMIT_METERS * 2;
  return {
    x: x / worldSize * span - WEB_MERCATOR_LIMIT_METERS,
    y: WEB_MERCATOR_LIMIT_METERS - y / worldSize * span,
  };
}

// request one retina-resolution state aerial image for the fixed farm extent
function propertySatelliteImageUrl(viewport: MapViewport): string {
  const upperLeft = webMercatorPixelMeters(viewport.left, viewport.top, viewport.zoom);
  const lowerRight = webMercatorPixelMeters(
    viewport.left + PROPERTY_MAP_WIDTH,
    viewport.top + PROPERTY_MAP_HEIGHT,
    viewport.zoom,
  );
  const parameters = new URLSearchParams({
    bbox: `${String(upperLeft.x)},${String(lowerRight.y)},${String(lowerRight.x)},${String(upperLeft.y)}`,
    bboxSR: "3857",
    f: "image",
    format: "jpg",
    imageSR: "3857",
    interpolation: "RSP_BilinearInterpolation",
    size: `${String(PROPERTY_MAP_WIDTH * 2)},${String(PROPERTY_MAP_HEIGHT * 2)}`,
  });
  return `${PROPERTY_SATELLITE_IMAGE_SERVICE}?${parameters.toString()}`;
}

// render the higher-resolution farm aerial or a normal tiled layer
function renderPropertyMapTiles(layer: MapLayer, viewport: MapViewport): string {
  // use one cacheable high-density image for the bounded farm viewport
  if (layer === "satellite") {
    return `<image href="${escapeHtml(propertySatelliteImageUrl(viewport))}" x="0" y="0" width="${String(PROPERTY_MAP_WIDTH)}" height="${String(PROPERTY_MAP_HEIGHT)}" preserveAspectRatio="none" referrerpolicy="origin"/>`;
  }

  return renderMapTiles(layer, viewport, PROPERTY_MAP_WIDTH, PROPERTY_MAP_HEIGHT);
}

// cap federal raster tiles at their highest populated cache level
function mapLayerSourceZoom(layer: MapLayer, viewportZoom: number): number {
  return layer === "roads" ? viewportZoom : Math.min(viewportZoom, 16);
}

// render only the visible tiles for the selected base layer
function renderMapTiles(
  layer: MapLayer,
  viewport: MapViewport,
  width: number = STATION_MAP_WIDTH,
  height: number = STATION_MAP_HEIGHT,
): string {
  const sourceZoom = mapLayerSourceZoom(layer, viewport.zoom);
  const sourceScale = 2 ** (viewport.zoom - sourceZoom);
  const renderedTileSize = STATION_MAP_TILE_SIZE * sourceScale;
  const firstColumn = Math.floor(viewport.left / renderedTileSize);
  const lastColumn = Math.floor(
    (viewport.left + width) / renderedTileSize,
  );
  const firstRow = Math.floor(viewport.top / renderedTileSize);
  const lastRow = Math.floor(
    (viewport.top + height) / renderedTileSize,
  );
  let tiles = "";

  // render each visible tile row
  for (let row = firstRow; row <= lastRow; row += 1) {
    // render each visible tile column
    for (let column = firstColumn; column <= lastColumn; column += 1) {
      const left = column * renderedTileSize - viewport.left;
      const top = row * renderedTileSize - viewport.top;
      tiles += `<image href="${escapeHtml(mapTileUrl(layer, sourceZoom, column, row))}" x="${left.toFixed(2)}" y="${top.toFixed(2)}" width="${renderedTileSize}" height="${renderedTileSize}" preserveAspectRatio="none" referrerpolicy="origin"/>`;
    }
  }

  return tiles;
}

// build one reviewed provider tile URL
function mapTileUrl(
  layer: MapLayer,
  zoom: number,
  column: number,
  row: number,
): string {
  // use the OSM XYZ order for labeled roads
  if (layer === "roads") {
    return `https://tile.openstreetmap.org/${String(zoom)}/${String(column)}/${String(row)}.png`;
  }

  const service = layer === "topo" ? "USGSTopo" : "USGSImageryOnly";
  return `https://basemap.nationalmap.gov/arcgis/rest/services/${service}/MapServer/tile/${String(zoom)}/${String(row)}/${String(column)}`;
}

// render visible attribution for the selected tile provider
function renderMapAttribution(layer: MapLayer): string {
  // retain the OSM copyright link with OSM tiles
  if (layer === "roads") {
    return '<p class="map-attribution">© <a href="https://www.openstreetmap.org/copyright" rel="noreferrer">OpenStreetMap contributors</a></p>';
  }

  return '<p class="map-attribution">Map services and data available from <a href="https://www.usgs.gov/programs/national-geospatial-program/national-map" rel="noreferrer">U.S. Geological Survey, National Geospatial Program</a>.</p>';
}

// credit the farm-specific NAIP aerial separately from cached federal tiles
function renderPropertyMapAttribution(layer: MapLayer): string {
  // retain the ordinary provider credit for roads and topo
  if (layer !== "satellite") {
    return renderMapAttribution(layer);
  }

  return '<p class="map-attribution">USGS and USDA NAIP aerial imagery from <a href="https://imagery.nationalmap.gov/arcgis/rest/services/USGSNAIPImagery/ImageServer" rel="noreferrer">The National Map</a>.</p>';
}

// label one selectable map base layer
function mapLayerLabel(layer: MapLayer): string {
  // expand the aerial layer for consumers
  if (layer === "satellite") {
    return "Satellite";
  }

  return layer === "roads" ? "Roads" : "Topo";
}

// render one background touch target below all visible markers
function renderStationHitArea(
  station: WeatherSite["stations"][number],
  viewport: MapViewport,
): string {
  const point = projectMapPoint(station.latitude, station.longitude, viewport);

  return `
    <a class="station-marker-hit-target" href="#station-current-${escapeHtml(station.slug)}" transform="translate(${point.x.toFixed(2)} ${point.y.toFixed(2)})" data-station-select="${escapeHtml(station.slug)}" tabindex="-1" aria-hidden="true">
      <circle class="station-marker-hit" r="41"/>
    </a>
  `;
}

// render one numbered map marker
function renderStationMarker(
  station: WeatherSite["stations"][number],
  index: number,
  viewport: MapViewport,
  state: DashboardState,
): string {
  const point = projectMapPoint(station.latitude, station.longitude, viewport);
  const reading = stationReading(station.slug, state);
  const selected = state.selectedStationSlug === station.slug;

  return `
    <a class="station-marker${selected ? " selected" : ""}" href="#station-current-${escapeHtml(station.slug)}" transform="translate(${point.x.toFixed(2)} ${point.y.toFixed(2)})" data-station-select="${escapeHtml(station.slug)}" aria-label="Show current conditions for ${escapeHtml(station.name)}" aria-expanded="${String(selected)}" aria-controls="station-current-${escapeHtml(station.slug)}">
      <circle class="station-marker-dot" r="12"/>
      <text text-anchor="middle" dominant-baseline="central">${String(index + 1)}</text>
      <title>${escapeHtml(station.name)} · ${escapeHtml(reading)}</title>
    </a>
  `;
}

// render one map legend row
function renderStationListItem(
  station: WeatherSite["stations"][number],
  index: number,
  site: WeatherSite,
  state: DashboardState,
): string {
  const distance = distanceMiles(
    site.latitude,
    site.longitude,
    station.latitude,
    station.longitude,
  );
  const selected = state.selectedStationSlug === station.slug;
  const current = selected ? renderCompactStationCurrent(station, state) : "";

  return `
    <li class="nearby-station-item${selected ? " selected" : ""}">
      <button class="nearby-station-select" type="button" data-station-select="${escapeHtml(station.slug)}" aria-expanded="${String(selected)}" aria-controls="station-current-${escapeHtml(station.slug)}">
        <span class="station-number">${String(index + 1)}</span>
        <span class="station-label"><strong>${escapeHtml(station.name)}</strong><span class="station-reading">${escapeHtml(stationReading(station.slug, state))} · ${formatNumber(distance)} mi</span></span>
      </button>
      ${current}
    </li>
  `;
}

interface CompactStationMetric {
  readonly label: string;
  readonly value: string | null;
}

// render one station-only current snapshot
function renderCompactStationCurrent(
  station: WeatherSite["stations"][number],
  state: DashboardState,
): string {
  const platform = station.sources.find(
    // identify the station's physical platform
    (source) => source.kind === "physical_sensor",
  )?.providerName ?? "Unknown platform";
  const record = currentStationRecord(station.slug, state.current);

  // retain stations without a current sample
  if (record === undefined) {
    return `
      <div class="station-current" id="station-current-${escapeHtml(station.slug)}" data-station-current="${escapeHtml(station.slug)}" role="region" aria-live="polite" aria-label="Current conditions for ${escapeHtml(station.name)}">
        <p class="station-current-empty">No current station reading is available.</p>
        <p class="station-current-meta"><strong>Platform</strong> ${escapeHtml(platform)}</p>
      </div>
    `;
  }

  const metrics: readonly CompactStationMetric[] = [
    {
      label: "Temp",
      value: compactMeasurement(formatMeasurement(record.metrics.temperatureC, "temperature", state.units)),
    },
    {
      label: "Humidity",
      value: compactMeasurement(formatFixedMeasurement(record.metrics.relativeHumidityPercent, "%")),
    },
    {
      label: "Wind",
      value: compactStationWind(record, state.units),
    },
    {
      label: "Rain",
      value: compactMeasurement(formatPrecipitationRate(record.metrics.precipitationRateMmPerHour, state.units)),
    },
    {
      label: "Pressure",
      value: compactMeasurement(formatMeasurement(record.metrics.pressureHpa, "pressure", state.units)),
    },
    {
      label: "PM2.5",
      value: compactMeasurement(formatFixedMeasurement(record.metrics.pm25MicrogramsPerCubicMeter, "µg/m³")),
    },
    {
      label: "UV",
      value: compactMeasurement(formatFixedMeasurement(record.metrics.uvIndex, "")),
    },
  ];
  const available = metrics.filter(
    // omit unavailable station measurements
    (metric): metric is Readonly<{ label: string; value: string }> => metric.value !== null,
  );

  return `
    <div class="station-current" id="station-current-${escapeHtml(station.slug)}" data-station-current="${escapeHtml(station.slug)}" role="region" aria-live="polite" aria-label="Current conditions for ${escapeHtml(station.name)}">
      <div class="station-current-metrics">
        ${available.map(
          // render every station-only measurement
          (metric) => `<span><strong>${escapeHtml(metric.label)}</strong> ${escapeHtml(metric.value)}</span>`,
        ).join("")}
      </div>
      <p class="station-current-meta"><strong>Platform</strong> ${escapeHtml(platform)} · ${escapeHtml(record.freshness.label)} · ${formatInstant(record.validAt, state.selectedSite?.timezone)}</p>
    </div>
  `;
}

// find one station's latest physical record
function currentStationRecord(
  stationSlug: string,
  records: readonly WeatherRecord[],
): WeatherRecord | undefined {
  return records.find(
    // isolate one physical station only
    (record) =>
      record.provenance.stationSlug === stationSlug &&
      record.provenance.sourceKind === "physical_sensor",
  );
}

// collapse one formatted measurement into compact text
function compactMeasurement(measurement: FormattedMeasurement): string | null {
  // omit unavailable measurements
  if (measurement.value === "—") {
    return null;
  }

  return `${measurement.value}${measurement.unit.length === 0 ? "" : ` ${measurement.unit}`}`;
}

// combine station wind and gust into one compact value
function compactStationWind(
  record: WeatherRecord,
  units: UnitPreferences,
): string | null {
  const speed = compactMeasurement(formatMeasurement(record.metrics.windSpeedMps, "windSpeed", units));
  const gust = compactMeasurement(formatMeasurement(record.metrics.windGustMps, "windSpeed", units));

  // render both available wind readings
  if (speed !== null && gust !== null) {
    return `${speed} · gust ${gust}`;
  }

  // retain one available wind reading
  if (speed !== null) {
    return speed;
  }

  return gust === null ? null : `Gust ${gust}`;
}

// format the best current station reading
function stationReading(stationSlug: string, state: DashboardState): string {
  const record = state.current.find(
    // match one station's latest normalized row
    (candidate) => candidate.provenance.stationSlug === stationSlug,
  );

  // retain stations without a current sample
  if (record === undefined) {
    return "Awaiting current data";
  }

  // prefer temperature for weather stations
  if (record.metrics.temperatureC !== null) {
    const value = formatMeasurement(
      record.metrics.temperatureC,
      "temperature",
      state.units,
    );
    return `${value.value}${value.unit}`;
  }

  // fall back to particulate concentration
  if (record.metrics.pm25MicrogramsPerCubicMeter !== null) {
    return `${formatNumber(record.metrics.pm25MicrogramsPerCubicMeter)} µg/m³ PM2.5`;
  }

  return "Current sample available";
}

// calculate a short local great-circle distance
function distanceMiles(
  latitudeA: number,
  longitudeA: number,
  latitudeB: number,
  longitudeB: number,
): number {
  const radians = Math.PI / 180;
  const deltaLatitude = (latitudeB - latitudeA) * radians;
  const deltaLongitude = (longitudeB - longitudeA) * radians;
  const left = Math.sin(deltaLatitude / 2) ** 2;
  const right = Math.cos(latitudeA * radians) *
    Math.cos(latitudeB * radians) *
    Math.sin(deltaLongitude / 2) ** 2;
  return 3_958.8 * 2 * Math.atan2(Math.sqrt(left + right), Math.sqrt(1 - left - right));
}

// keep homepage temperature labels responsive during loading and after
function renderConditionLabel(label: string): string {
  return label === "Temperature"
    ? 'Temp<span class="condition-label-suffix">erature</span>'
    : escapeHtml(label);
}

// render one friendly current-condition card
function renderConditionCard(options: ConditionCardOptions): string {
  // show a distinct secondary comparison only when supplied
  const comparison = options.secondary?.comparison === undefined
    ? ""
    : `<div class="condition-secondary-comparison"><span>${escapeHtml(options.secondary.comparison.label)}</span>${renderConditionMeasurement(options.secondary.comparison.measurement)}</div>`;
  // keep related readings inside one visual card
  const secondary = options.secondary === undefined
    ? ""
    : `
      <div class="condition-secondary${comparison.length === 0 ? "" : " condition-secondary-paired"}">
        <span class="condition-secondary-divider">${escapeHtml(options.secondary.label)}</span>
        ${renderConditionMeasurement(options.secondary.measurement)}
        ${comparison}
      </div>
    `;
  // omit details promoted into a secondary statistic
  const detail = options.band.detail.length === 0
    ? ""
    : `<p class="condition-detail">${escapeHtml(options.band.detail)}</p>`;

  return `
    <article class="condition-card ${escapeHtml(options.className)}" data-condition="${escapeHtml(options.label.toLowerCase().replaceAll(" ", "-"))}">
      ${renderConditionColor(options.band.color)}
      <div class="condition-card-content">
        <div class="condition-card-heading">
          <span class="condition-label">${renderMaterialIcon(options.icon, options.adjusted)}<span>${renderConditionLabel(options.label)}</span></span>
          ${renderConditionStatus(options.band)}
        </div>
        <div class="condition-body${options.secondary === undefined ? "" : " condition-body-secondary"}">
          <div class="condition-live">
            <div class="condition-primary">${renderConditionMeasurement(options.measurement)}</div>
            ${secondary}
          </div>
          ${renderConditionForecast(options.forecast)}
        </div>
        ${detail}
      </div>
    </article>
  `;
}

// render one threshold-colored forecast summary
function renderConditionForecast(forecast: ForecastCardValue): string {
  return `
    <div class="condition-forecast">
      <span class="condition-forecast-readings">
        ${forecast.readings.map(
          // render each forecast statistic
          (reading) => `<span class="condition-forecast-reading condition-forecast-tone-${reading.tone ?? "neutral"}"><span class="condition-forecast-label">${escapeHtml(reading.label)}</span> ${renderForecastMeasurement(reading.measurement)}</span>`,
        ).join("")}
      </span>
    </div>
  `;
}

// render one compact forecast value with a subordinate unit
function renderForecastMeasurement(measurement: FormattedMeasurement): string {
  // omit empty unit furniture
  if (measurement.unit.length === 0) {
    return `<strong>${escapeHtml(measurement.value)}</strong>`;
  }

  const separator = measurement.unit.startsWith("°") || measurement.unit === "%"
    ? ""
    : " ";
  return `<strong>${escapeHtml(measurement.value)}${separator}<small>${escapeHtml(measurement.unit)}</small></strong>`;
}

// render CSP-safe color behind one condition card
function renderConditionColor(color: string): string {
  return `
    <svg class="condition-color" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true" focusable="false">
      <rect width="100" height="100" fill="${escapeHtml(color)}" opacity="0.18"/>
    </svg>
  `;
}

// render one threshold-colored status pill
function renderConditionStatus(band: ConditionBand): string {
  const textClass = conditionStatusTextClass(band.color);
  return `
    <span class="condition-status ${textClass}">
      <svg class="condition-status-color" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true" focusable="false">
        <rect width="100" height="100" fill="${escapeHtml(band.color)}"/>
      </svg>
      <span>${escapeHtml(band.label)}</span>
    </span>
  `;
}

// choose readable text over one threshold color
function conditionStatusTextClass(color: string): string {
  const channels = /^rgb\((\d+), (\d+), (\d+)\)$/u.exec(color);
  const luminance = channels === null
    ? 0
    : linearizeColorChannel(channels[1]) * 0.2126 +
      linearizeColorChannel(channels[2]) * 0.7152 +
      linearizeColorChannel(channels[3]) * 0.0722;
  return luminance > 0.179 ? "condition-status-dark" : "condition-status-light";
}

// convert one color channel to relative luminance
function linearizeColorChannel(channel: string | undefined): number {
  const normalized = Number(channel ?? 0) / 255;
  return normalized <= 0.04045
    ? normalized / 12.92
    : ((normalized + 0.055) / 1.055) ** 2.4;
}

// render one decorative symbol with an optional active-adjustment marker
function renderMaterialIcon(name: MaterialIconName, adjusted = false): string {
  return `<span class="material-symbols-rounded${adjusted ? " forecast-adjusted-icon" : ""}" aria-hidden="true">${name}</span>`;
}

// render the Material save shape without a font dependency
function renderSaveIcon(): string {
  return '<svg class="material-inline-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M17 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V7l-4-4Zm2 16H5V5h11.17L19 7.83V19ZM12 18a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM6 6h9v4H6V6Z"/></svg>';
}

// render one large value with its configured unit
function renderConditionMeasurement(measurement: FormattedMeasurement): string {
  // omit empty unit furniture
  if (measurement.unit.length === 0) {
    return `<strong>${escapeHtml(measurement.value)}</strong>`;
  }

  return `<strong>${escapeHtml(measurement.value)}<small>${escapeHtml(measurement.unit)}</small></strong>`;
}

// format one meteorological bearing as a compass direction
function formatWindDirection(degrees: number | null): string | null {
  // omit unavailable or malformed bearings
  if (degrees === null || !Number.isFinite(degrees)) {
    return null;
  }

  return WIND_CARDINAL_DIRECTIONS[
    Math.round(degrees / (360 / WIND_CARDINAL_DIRECTIONS.length)) % WIND_CARDINAL_DIRECTIONS.length
  ]!;
}

// classify temperature comfort and interpolate the requested color scale
export function temperatureBand(valueC: number | null): ConditionBand {
  // preserve unavailable temperature honestly
  if (valueC === null) {
    return unavailableBand("Temperature reading unavailable");
  }

  const valueF = valueC * 9 / 5 + 32;
  const color = valueF <= 60
    ? interpolateColor([56, 120, 197], [67, 151, 86], (valueF - 32) / 28)
    : interpolateColor([67, 151, 86], [207, 67, 55], (valueF - 60) / 20);

  // label freezing conditions
  if (valueF <= 32) {
    return { color, detail: "At or below the freezing point", label: "Freezing" };
  }

  // use blue forecast text below the shared 55f cold threshold
  if (valueF < 55) {
    return { color, detail: "Cool outdoor conditions", label: "Chilly" };
  }

  // label cool conditions
  if (valueF < 60) {
    return { color, detail: "Approaching the comfort range", label: "Cool" };
  }

  // label comfortable conditions
  if (valueF <= 70) {
    return { color, detail: "Comfortable outdoor temperature", label: "Comfortable" };
  }

  // label warm conditions
  if (valueF <= 80) {
    return { color, detail: "Warm outdoor conditions", label: "Warm" };
  }

  return { color, detail: "Hot outdoor conditions", label: "Hot" };
}

// classify wind using the stronger sustained or gust speed
export function windBand(
  speedMps: number | null,
  gustMps: number | null,
  units: UnitPreferences = DEFAULT_UNIT_PREFERENCES,
): ConditionBand {
  const available = [speedMps, gustMps].filter((value): value is number => value !== null);

  // preserve unavailable wind honestly
  if (available.length === 0) {
    return unavailableBand("Wind reading unavailable");
  }

  const valueMph = Math.max(...available) * 2.236_936_292_1;
  const peak = formatMeasurement(Math.max(...available), "windSpeed", units, 0);
  const color = interpolateColor(
    [67, 151, 86],
    [207, 67, 55],
    (valueMph - 10) / 40,
  );

  // label calm air
  if (valueMph <= 3) {
    return { color, detail: `Peak reading ${peak.value} ${peak.unit}`, label: "Calm" };
  }

  // retain green through ten miles per hour
  if (valueMph < 10) {
    return { color, detail: `Peak reading ${peak.value} ${peak.unit}`, label: "Light" };
  }

  // label a noticeable breeze
  if (valueMph < 20) {
    return { color, detail: `Peak reading ${peak.value} ${peak.unit}`, label: "Breezy" };
  }

  // label stronger wind
  if (valueMph < 35) {
    return { color, detail: `Peak reading ${peak.value} ${peak.unit}`, label: "Windy" };
  }

  // label very strong wind
  if (valueMph < 50) {
    return { color, detail: `Peak reading ${peak.value} ${peak.unit}`, label: "Very windy" };
  }

  return { color, detail: `Peak reading ${peak.value} ${peak.unit}`, label: "Dangerous" };
}

// classify PM2.5 with current EPA health-category breakpoints
export function airQualityBand(value: number | null): ConditionBand {
  // preserve unavailable particulate data honestly
  if (value === null) {
    return unavailableBand("PM2.5 reading unavailable");
  }

  // label the good PM2.5 range
  if (value <= 9) {
    return { color: "rgb(0, 146, 63)", detail: "PM2.5 health range", label: "Good" };
  }

  // label the moderate PM2.5 range
  if (value <= 35.4) {
    return { color: "rgb(230, 181, 25)", detail: "PM2.5 health range", label: "Moderate" };
  }

  // label sensitive-group risk
  if (value <= 55.4) {
    return { color: "rgb(239, 126, 31)", detail: "PM2.5 health range", label: "Sensitive groups" };
  }

  // label unhealthy particulate levels
  if (value <= 125.4) {
    return { color: "rgb(207, 67, 55)", detail: "PM2.5 health range", label: "Unhealthy" };
  }

  // label very unhealthy particulate levels
  if (value <= 225.4) {
    return { color: "rgb(124, 81, 116)", detail: "PM2.5 health range", label: "Very unhealthy" };
  }

  return { color: "rgb(114, 30, 52)", detail: "PM2.5 health range", label: "Hazardous" };
}

// classify UV exposure with the EPA scale
export function uvBand(value: number | null): ConditionBand {
  // preserve unavailable UV data honestly
  if (value === null) {
    return unavailableBand("UV reading unavailable");
  }

  // label low UV exposure
  if (value <= 2) {
    return { color: "rgb(0, 146, 63)", detail: "Minimal sun protection needed", label: "Low" };
  }

  // label moderate UV exposure
  if (value <= 5) {
    return { color: "rgb(230, 181, 25)", detail: "Sun protection recommended", label: "Moderate" };
  }

  // label high UV exposure
  if (value <= 7) {
    return { color: "rgb(239, 126, 31)", detail: "Reduce midday exposure", label: "High" };
  }

  // label very high UV exposure
  if (value <= 10) {
    return { color: "rgb(207, 67, 55)", detail: "Extra sun protection needed", label: "Very high" };
  }

  return { color: "rgb(124, 81, 116)", detail: "Avoid unprotected sun exposure", label: "Extreme" };
}

// classify pressure around the standard sea-level range
export function pressureBand(
  valueHpa: number | null,
  units: UnitPreferences = DEFAULT_UNIT_PREFERENCES,
): ConditionBand {
  // preserve unavailable pressure honestly
  if (valueHpa === null) {
    return unavailableBand("Pressure reading unavailable");
  }

  const low = formatMeasurement(1_009, "pressure", units);
  const high = formatMeasurement(1_022.7, "pressure", units);

  // label pressure below the normal display band
  if (valueHpa < 1_009) {
    return { color: "rgb(56, 120, 197)", detail: `Below ${low.value} ${low.unit}`, label: "Low" };
  }

  // label pressure above the normal display band
  if (valueHpa > 1_022.7) {
    return { color: "rgb(207, 67, 55)", detail: `Above ${high.value} ${high.unit}`, label: "High" };
  }

  return { color: "rgb(67, 151, 86)", detail: `${low.value}–${high.value} ${low.unit}`, label: "Normal" };
}

// color humidity discomfort only in hot air rather than treating damp cold air as oppressive
export function humidityBand(value: number | null, temperatureC: number | null): ConditionBand {
  // preserve unavailable or invalid humidity honestly
  if (value === null || !Number.isFinite(value) || value < 0 || value > 100) {
    return unavailableBand("Humidity reading unavailable");
  }

  // avoid inferring comfort without paired air temperature
  if (temperatureC === null || !Number.isFinite(temperatureC)) {
    return unavailableBand("Air temperature unavailable for humidity comfort");
  }

  // use an 80-degree fahrenheit display cutoff rather than a medical heat-risk threshold
  if (temperatureC < (80 - 32) * 5 / 9) {
    return { color: "rgb(67, 151, 86)", detail: "No heat-related humidity discomfort", label: "Comfortable" };
  }

  // keep dry hot air free of humidity warnings
  if (value <= 60) {
    return { color: "rgb(67, 151, 86)", detail: "Humidity below the muggy range", label: "Comfortable" };
  }

  // flag the first humid comfort band
  if (value <= 70) {
    return { color: "rgb(230, 181, 25)", detail: "Humid air adds to the heat", label: "Humid" };
  }

  // flag uncomfortable humidity
  if (value <= 80) {
    return { color: "rgb(239, 126, 31)", detail: "Uncomfortably hot and humid", label: "Very humid" };
  }

  return { color: "rgb(207, 67, 55)", detail: "Oppressively hot and humid", label: "Very humid" };
}

// classify modeled cloud cover using the dashboard's three display bands
export function cloudBand(value: number | null): ConditionBand {
  // keep missing model values distinct from clear skies
  if (value === null) {
    return unavailableBand("");
  }

  // show nearly cloud-free skies as clear
  if (value <= 10) {
    return { color: "rgb(105, 133, 155)", detail: "", label: "Clear" };
  }

  // keep up to half-covered skies in the light band
  if (value <= 50) {
    return { color: "rgb(105, 133, 155)", detail: "", label: "Light" };
  }

  return { color: "rgb(105, 133, 155)", detail: "", label: "Heavy" };
}

// describe the current precipitation rate
function rainBand(valueMmPerHour: number | null): ConditionBand {
  // preserve unavailable rain-rate data honestly
  if (valueMmPerHour === null) {
    return unavailableBand("Rain-rate reading unavailable");
  }

  // label dry conditions
  if (valueMmPerHour === 0) {
    return { color: "rgb(84, 84, 80)", detail: "No rain detected", label: "Dry" };
  }

  // label light rain
  if (valueMmPerHour <= 2.5) {
    return { color: "rgb(56, 120, 197)", detail: "Light hourly rainfall", label: "Light rain" };
  }

  // label moderate rain
  if (valueMmPerHour <= 7.62) {
    return { color: "rgb(56, 120, 197)", detail: "Steady hourly rainfall", label: "Rain" };
  }

  return { color: "rgb(124, 81, 116)", detail: "Heavy hourly rainfall", label: "Heavy rain" };
}

// provide one neutral unavailable-data presentation
function unavailableBand(detail: string): ConditionBand {
  return { color: "rgb(136, 136, 130)", detail, label: "Unavailable" };
}

// interpolate and clamp one RGB color
function interpolateColor(start: RgbColor, end: RgbColor, progress: number): string {
  const boundedProgress = Math.max(0, Math.min(1, progress));
  const channels = [
    Math.round(start[0] + (end[0] - start[0]) * boundedProgress),
    Math.round(start[1] + (end[1] - start[1]) * boundedProgress),
    Math.round(start[2] + (end[2] - start[2]) * boundedProgress),
  ];
  return `rgb(${channels.join(", ")})`;
}

// map one discrete condition color to its CSP-safe text tone
function forecastToneForBand(value: number | null, band: ConditionBand): ForecastTone {
  // keep unavailable colors neutral
  if (value === null) {
    return "neutral";
  }

  // retain the approved discrete threshold palette
  switch (band.color) {
    case "rgb(56, 120, 197)": return "blue";
    case "rgb(114, 30, 52)": return "burgundy";
    case "rgb(200, 183, 68)": return "gold";
    case "rgb(136, 136, 130)":
    case "rgb(84, 84, 80)": return "gray";
    case "rgb(0, 146, 63)":
    case "rgb(67, 151, 86)": return "green";
    case "rgb(239, 126, 31)": return "orange";
    case "rgb(124, 81, 116)": return "purple";
    case "rgb(207, 67, 55)": return "red";
    case "rgb(230, 181, 25)": return "yellow";
    default: return "neutral";
  }
}

// classify forecast temperature with its semantic threshold color
function forecastTemperatureTone(valueC: number | null): ForecastTone {
  // retain the requested temperature bands
  switch (temperatureBand(valueC).label) {
    case "Freezing":
    case "Chilly": return "blue";
    case "Cool":
    case "Comfortable": return "green";
    case "Warm": return "orange";
    case "Hot": return "red";
    default: return "neutral";
  }
}

// classify forecast wind with its semantic threshold color
function forecastWindTone(valueMps: number | null, units: UnitPreferences): ForecastTone {
  // retain the requested wind bands
  switch (windBand(valueMps, null, units).label) {
    case "Calm":
    case "Light": return "green";
    case "Breezy": return "yellow";
    case "Windy": return "orange";
    case "Very windy": return "red";
    case "Dangerous": return "purple";
    default: return "neutral";
  }
}

// render filters and paginated history
function renderHistory(state: DashboardState): string {
  return `
    <section class="panel" aria-labelledby="history-heading">
      <div class="section-heading">
        <div><p class="eyebrow">Past conditions</p><h2 id="history-heading">Weather history</h2></div>
        <span class="page-label">Page ${String(state.page + 1)}</span>
      </div>
      ${renderHistoryFilters(state)}
      <div class="table-scroll">
        <table>
          <caption class="sr-only">Filterable weather history for ${escapeHtml(state.selectedSite?.name ?? "the selected site")}</caption>
          <thead><tr><th scope="col">Valid time</th><th scope="col">Temperature (${escapeHtml(formatMeasurement(0, "temperature", state.units).unit)})</th><th scope="col">Humidity (%)</th><th scope="col">Wind (${escapeHtml(formatMeasurement(0, "windSpeed", state.units).unit)})</th><th scope="col">Precipitation (${escapeHtml(formatMeasurement(0, "precipitation", state.units).unit)})</th><th scope="col">Source and provenance</th></tr></thead>
          <tbody>${renderHistoryRows(state)}</tbody>
        </table>
      </div>
      ${renderHistoryCards(state)}
      <nav class="pagination" aria-label="History pages">
        <button type="button" data-page="previous"${state.page === 0 || state.loading ? " disabled" : ""}>Previous</button>
        <button type="button" data-page="next"${state.nextCursor === null || state.loading ? " disabled" : ""}>Next</button>
      </nav>
    </section>
  `;
}

// render history filter controls
function renderHistoryFilters(state: DashboardState): string {
  let stationOptions = `<option value="">All stations</option>`;
  let sourceOptions = `<option value="">All sources</option>`;
  const timezone = state.selectedSite?.timezone ?? "UTC";
  const activeFilterCount = Object.values(state.filters).filter(
    // count configured filter values
    (value) => value !== undefined,
  ).length;

  // collect the selected site's sources
  for (const station of state.selectedSite?.stations ?? []) {
    const stationSelected = station.slug === state.filters.stationSlug ? " selected" : "";
    stationOptions += `<option value="${escapeHtml(station.slug)}"${stationSelected}>${escapeHtml(station.name)}</option>`;

    // render every source option
    for (const source of station.sources) {
      const selected = source.id === state.filters.sourceId ? " selected" : "";
      sourceOptions += `<option value="${escapeHtml(source.id)}"${selected}>${escapeHtml(source.provenanceLabel)}</option>`;
    }
  }

  return `
    <details class="history-filter-disclosure" data-history-filter-disclosure data-filter-active="${String(activeFilterCount > 0)}">
      <summary>Filters${activeFilterCount === 0 ? "" : ` · ${String(activeFilterCount)} active`}</summary>
      <form class="filters" data-history-filters>
        <label><span>Station</span><select name="stationSlug">${stationOptions}</select></label>
        <label><span>Source</span><select name="sourceId">${sourceOptions}</select></label>
        <label><span>Provenance</span><select name="sourceKind">
          <option value="">All kinds</option>
          <option value="model_current"${state.filters.sourceKind === "model_current" ? " selected" : ""}>Model current</option>
          <option value="reanalysis"${state.filters.sourceKind === "reanalysis" ? " selected" : ""}>Historical reanalysis</option>
        </select></label>
        <label><span>From</span><input name="from" type="datetime-local" value="${escapeHtml(toLocalInput(state.filters.from, timezone))}"></label>
        <label><span>To</span><input name="to" type="datetime-local" value="${escapeHtml(toLocalInput(state.filters.to, timezone))}"></label>
        <button type="submit"${state.loading ? " disabled" : ""}>Apply filters</button>
      </form>
    </details>
  `;
}

// render history table rows
function renderHistoryRows(state: DashboardState): string {
  // reserve the requested page while filters or pagination load
  if (state.loading) {
    return Array.from({ length: 25 },
      // preserve every final history row
      () => `
        <tr class="skeleton-history-row" aria-hidden="true">
          ${Array.from({ length: 6 },
            // reserve each final history cell
            () => '<td><span class="skeleton-line skeleton-table-value"></span></td>',
          ).join("")}
        </tr>
      `,
    ).join("");
  }

  // render an honest empty result after the request settles
  if (state.history.length === 0) {
    return `<tr><td colspan="6" class="empty">No records match these filters.</td></tr>`;
  }

  let rows = "";

  // render every visible record
  for (const record of state.history) {
    rows += `
      <tr>
        <td><time datetime="${escapeHtml(record.validAt)}">${formatInstant(record.validAt, state.selectedSite?.timezone)}</time></td>
        <td>${formatMeasurementCell(record.metrics.temperatureC, "temperature", state.units)}</td>
        <td>${formatMetricCell(record.metrics.relativeHumidityPercent, "%")}</td>
        <td>${formatMeasurementCell(record.metrics.windSpeedMps, "windSpeed", state.units)}</td>
        <td>${formatMeasurementCell(record.metrics.precipitationMm, "precipitation", state.units)}</td>
        <td><span class="source-kind">${escapeHtml(record.provenance.label)}</span><small>${escapeHtml(record.provenance.sourceKey)}</small></td>
      </tr>
    `;
  }

  return rows;
}

// render compact phone history records
function renderHistoryCards(state: DashboardState): string {
  // reserve the requested mobile page while records load
  if (state.loading) {
    return `
      <ol class="history-cards skeleton-history-cards" aria-label="Loading weather history" aria-busy="true">
        ${Array.from({ length: 25 },
          // preserve every final mobile history card
          () => `
            <li aria-hidden="true">
              <article class="history-card skeleton-history-card">
                <div class="history-card-primary"><span class="skeleton-line skeleton-history-time"></span><span class="skeleton-line skeleton-history-temperature"></span></div>
                <div class="history-card-metrics"><span class="skeleton-line skeleton-history-metric"></span><span class="skeleton-line skeleton-history-metric"></span><span class="skeleton-line skeleton-history-metric"></span></div>
                <span class="skeleton-line skeleton-history-source"></span>
              </article>
            </li>
          `,
        ).join("")}
      </ol>
    `;
  }

  // render an honest empty result after the request settles
  if (state.history.length === 0) {
    return '<p class="history-cards-empty">No records match these filters.</p>';
  }

  return `
    <ol class="history-cards" aria-label="Compact weather history">
      ${state.history.map(
        // render each mobile history record
        (record) => `
          <li>
            <article class="history-card">
              <div class="history-card-primary">
                <time datetime="${escapeHtml(record.validAt)}">${formatInstant(record.validAt, state.selectedSite?.timezone)}</time>
                <strong>${formatMeasurementCell(record.metrics.temperatureC, "temperature", state.units)}</strong>
              </div>
              <div class="history-card-metrics">
                <span><strong>Humidity</strong> ${formatMetricCell(record.metrics.relativeHumidityPercent, "%")}</span>
                <span><strong>Wind</strong> ${formatMeasurementCell(record.metrics.windSpeedMps, "windSpeed", state.units)}</span>
                <span><strong>Rain</strong> ${formatMeasurementCell(record.metrics.precipitationMm, "precipitation", state.units)}</span>
              </div>
              <details><summary>Source</summary><span>${escapeHtml(record.provenance.label)}</span><small>${escapeHtml(record.provenance.sourceKey)}</small></details>
            </article>
          </li>
        `,
      ).join("")}
    </ol>
  `;
}

// connect rendered controls to controller actions
function bindDashboardControls(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  bindForecastAdjustmentToggle(root, controller);
  bindUnitSettings(root, controller);
  bindForecastCharts(root, controller);
  bindTrendMetricControl(root, controller);
  bindTrendViewControls(root, controller);
  bindTrendExtremeControls(root, controller);
  bindTrendYearControls(root, controller);
  bindTrendCrosshair(root, controller);
  bindMapControls(root, controller);
  bindPropertyMapControls(root, controller);
  bindPropertySensorMap(root, controller);
  bindPropertySensorAdmin(root, controller);
  bindForecastAdjustmentAdmin(root, controller);
  const filterDisclosure = root.querySelector<HTMLDetailsElement>("[data-history-filter-disclosure]");

  // expand filters on wide screens or when active
  if (filterDisclosure !== null) {
    const compact = window.matchMedia("(max-width: 42rem)").matches;
    filterDisclosure.open = !compact || filterDisclosure.dataset.filterActive === "true";
  }

  const form = root.querySelector<HTMLFormElement>("[data-history-filters]");

  // wire filter submission
  if (form !== null) {
    // parse one filter form
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      const data = new FormData(form);
      const timezone = controller.state.selectedSite?.timezone ?? "UTC";
      let from: string | undefined;
      let to: string | undefined;

      try {
        from = toInstant(data.get("from"), timezone);
        to = toInstant(data.get("to"), timezone);
      } catch (error) {
        // report site-time validation only
        if (error instanceof RangeError) {
          controller.reportInvalidHistoryWallClock();
          return;
        }

        throw error;
      }

      void controller.setFilters({
        ...optionalFilter("from", from),
        ...optionalFilter("sourceId", readFormValue(data.get("sourceId"))),
        ...optionalFilter(
          "sourceKind",
          readFormValue(data.get("sourceKind")) as SiteSource["kind"] | undefined,
        ),
        ...optionalFilter("stationSlug", readFormValue(data.get("stationSlug"))),
        ...optionalFilter("to", to),
      });
    });
  }

  const previous = root.querySelector<HTMLButtonElement>("[data-page='previous']");

  // wire the previous page
  if (previous !== null) {
    // navigate backward
    previous.addEventListener("click", () => {
      void controller.previousPage();
    });
  }

  const next = root.querySelector<HTMLButtonElement>("[data-page='next']");

  // wire the next page
  if (next !== null) {
    // navigate forward
    next.addEventListener("click", () => {
      void controller.nextPage();
    });
  }
}

// bind the protected forecast adjustment form
function bindForecastAdjustmentAdmin(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  const form = root.querySelector<HTMLFormElement>("[data-admin-forecast-adjustments]");

  // skip non-admin pages and unavailable settings
  if (form === null || controller.state.forecastAdjustmentSettings === null) {
    return;
  }

  // submit one complete set of independent switches
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const data = new FormData(form);
    void controller.saveForecastAdjustmentSettings({
      version: 1,
      temperature: data.has("temperature"),
      wind: data.has("wind"),
      rain: data.has("rain"),
    });
  });
}

// connect the shared forecast adjustment switch
function bindForecastAdjustmentToggle(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  const toggle = root.querySelector<HTMLButtonElement>("[data-forecast-adjustment-toggle]");

  // skip routes without forecast controls
  if (toggle === null) {
    return;
  }

  // switch the persisted display source
  toggle.addEventListener("click", () => {
    controller.toggleForecastAdjustmentMode();
  });
}

// keep map markers fixed-size while their coordinate anchors move
function positionPropertyMapAnchors(map: SVGSVGElement): void {
  const scale = Number(map.dataset.propertyMapScale ?? 1);
  const translateX = Number(map.dataset.propertyMapTranslateX ?? 0);
  const translateY = Number(map.dataset.propertyMapTranslateY ?? 0);

  // position every map marker from its original viewport coordinate
  for (const anchor of map.querySelectorAll<SVGGraphicsElement>("[data-property-map-anchor]")) {
    const x = Number(anchor.dataset.propertyMapX);
    const y = Number(anchor.dataset.propertyMapY);

    // reject incomplete rendered marker coordinates
    if (!Number.isFinite(x) || !Number.isFinite(y)) {
      continue;
    }

    anchor.setAttribute(
      "transform",
      `translate(${(translateX + x * scale).toFixed(3)} ${(translateY + y * scale).toFixed(3)})`,
    );
  }
}

// connect property layers and bounded pan-and-zoom behavior
function bindPropertyMapControls(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  // wire every farm-scale tile layer
  for (const button of root.querySelectorAll<HTMLButtonElement>("[data-property-map-layer]")) {
    // render the selected property tile source
    button.addEventListener("click", () => {
      const layer = button.dataset.propertyMapLayer;

      // reject impossible rendered values
      if (layer !== "roads" && layer !== "topo" && layer !== "satellite") {
        return;
      }

      controller.setPropertyMapLayer(layer);
    });
  }

  // bind each public or admin property viewport
  for (const map of root.querySelectorAll<SVGSVGElement>("[data-property-interactive-map]")) {
    const world = map.querySelector<SVGGElement>("[data-property-map-world]");
    const shell = map.parentElement;
    const width = Number(map.dataset.propertyMapWidth);
    const height = Number(map.dataset.propertyMapHeight);

    // reject incomplete rendered map contracts
    if (
      world === null ||
      shell === null ||
      !Number.isFinite(width) ||
      !Number.isFinite(height) ||
      width <= 0 ||
      height <= 0
    ) {
      continue;
    }

    let scale = 1;
    let translateX = 0;
    let translateY = 0;
    let gesture: null | {
      moved: boolean;
      pointerId: number;
      startClientX: number;
      startClientY: number;
      startTranslateX: number;
      startTranslateY: number;
    } = null;

    // clamp one translation inside the original farm extent
    const clampTranslation = (value: number, dimension: number): number =>
      Math.min(0, Math.max(dimension - dimension * scale, value));

    // publish one transform to the SVG and placement editor
    const applyTransform = (): void => {
      translateX = clampTranslation(translateX, width);
      translateY = clampTranslation(translateY, height);
      world.setAttribute(
        "transform",
        `translate(${translateX.toFixed(3)} ${translateY.toFixed(3)}) scale(${scale.toFixed(4)})`,
      );
      map.dataset.propertyMapScale = String(scale);
      map.dataset.propertyMapTranslateX = String(translateX);
      map.dataset.propertyMapTranslateY = String(translateY);
      positionPropertyMapAnchors(map);
      shell.querySelector<HTMLButtonElement>('[data-property-map-zoom="out"]')
        ?.toggleAttribute("disabled", scale <= 1);
      shell.querySelector<HTMLButtonElement>('[data-property-map-zoom="reset"]')
        ?.toggleAttribute("disabled", scale <= 1);
    };

    // convert one browser point into the fixed SVG viewport
    const viewportPoint = (
      clientX: number,
      clientY: number,
    ): Readonly<{ x: number; y: number }> | null => {
      const bounds = map.getBoundingClientRect();

      // reject hidden or collapsed maps
      if (bounds.width <= 0 || bounds.height <= 0) {
        return null;
      }

      return {
        x: (clientX - bounds.left) / bounds.width * width,
        y: (clientY - bounds.top) / bounds.height * height,
      };
    };

    // zoom around one stable viewport point
    const zoomAt = (nextScale: number, x: number, y: number): void => {
      const boundedScale = Math.max(1, Math.min(4, nextScale));
      const mapX = (x - translateX) / scale;
      const mapY = (y - translateY) / scale;
      scale = boundedScale;
      translateX = x - mapX * scale;
      translateY = y - mapY * scale;
      applyTransform();
    };

    // zoom with a mouse wheel or trackpad
    map.addEventListener("wheel", (event) => {
      event.preventDefault();
      const point = viewportPoint(event.clientX, event.clientY);

      // wait for a measurable map
      if (point === null) {
        return;
      }

      zoomAt(scale * (event.deltaY < 0 ? 1.25 : 0.8), point.x, point.y);
    }, { passive: false });

    // start one bounded pointer pan
    map.addEventListener("pointerdown", (event) => {
      gesture = {
        moved: false,
        pointerId: event.pointerId,
        startClientX: event.clientX,
        startClientY: event.clientY,
        startTranslateX: translateX,
        startTranslateY: translateY,
      };
      map.setPointerCapture(event.pointerId);
      map.classList.add("property-map-dragging");
    });

    // pan the zoomed content without leaving the fixed extent
    map.addEventListener("pointermove", (event) => {
      // require the active pointer
      if (gesture === null || gesture.pointerId !== event.pointerId) {
        return;
      }

      const bounds = map.getBoundingClientRect();

      // reject hidden maps during layout changes
      if (bounds.width <= 0 || bounds.height <= 0) {
        return;
      }

      const deltaX = (event.clientX - gesture.startClientX) / bounds.width * width;
      const deltaY = (event.clientY - gesture.startClientY) / bounds.height * height;
      gesture.moved ||= Math.hypot(deltaX, deltaY) > 3;
      translateX = gesture.startTranslateX + deltaX;
      translateY = gesture.startTranslateY + deltaY;
      applyTransform();
    });

    // finish one pointer pan and suppress its synthetic placement click
    const finishPan = (event: PointerEvent): void => {
      // require the active pointer
      if (gesture === null || gesture.pointerId !== event.pointerId) {
        return;
      }

      map.dataset.propertyMapSuppressClick = String(gesture.moved);
      gesture = null;
      map.classList.remove("property-map-dragging");

      // release only a retained pointer capture
      if (map.hasPointerCapture(event.pointerId)) {
        map.releasePointerCapture(event.pointerId);
      }
    };
    map.addEventListener("pointerup", finishPan);
    map.addEventListener("pointercancel", finishPan);

    // wire explicit mobile-friendly zoom controls
    for (const button of shell.querySelectorAll<HTMLButtonElement>("[data-property-map-zoom]")) {
      // apply one requested zoom operation
      button.addEventListener("click", () => {
        const operation = button.dataset.propertyMapZoom;

        // restore the exact original property bounds
        if (operation === "reset") {
          scale = 1;
          translateX = 0;
          translateY = 0;
          applyTransform();
          return;
        }

        const nextScale = operation === "in" ? scale * 1.5 : scale / 1.5;
        zoomAt(nextScale, width / 2, height / 2);
      });
    }

    applyTransform();
  }
}

// synchronize one list or map control with its matching peers
function bindRelatedMapHighlight(
  root: HTMLElement,
  control: HTMLElement,
  attribute: "data-property-sensor-view" | "data-station-select",
  identity: string,
): void {
  // toggle every rendered peer together
  const togglePeers = (active: boolean): void => {
    // update each matching marker and list control
    for (const peer of root.querySelectorAll<HTMLElement>(`[${attribute}="${CSS.escape(identity)}"]`)) {
      peer.classList.toggle("related-hover", active);
    }
  };

  // highlight peers from pointer navigation
  control.addEventListener("pointerenter", () => togglePeers(true));
  control.addEventListener("pointerleave", () => togglePeers(false));
  // highlight peers from keyboard navigation
  control.addEventListener("focus", () => togglePeers(true));
  control.addEventListener("blur", () => togglePeers(false));
}

// connect the public property sensor map and list
function bindPropertySensorMap(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  // wire every matching property marker and list control
  for (const control of root.querySelectorAll<HTMLElement>("[data-property-sensor-view]")) {
    const sensorKey = control.dataset.propertySensorView;

    // reject incomplete rendered identities
    if (sensorKey === undefined) {
      continue;
    }

    bindRelatedMapHighlight(root, control, "data-property-sensor-view", sensorKey);

    // keep marker clicks separate from map panning
    if (control.classList.contains("property-sensor-marker")) {
      control.addEventListener("pointerdown", (event) => event.stopPropagation());
    }

    // reveal one sensor's complete reading set
    control.addEventListener("click", (event) => {
      event.preventDefault();
      controller.setSelectedPropertySensor(sensorKey);

      // reveal list details selected from the map
      if (control.classList.contains("property-sensor-marker")) {
        root.querySelector<HTMLElement>(`[data-property-sensor-details="${CSS.escape(sensorKey)}"]`)
          ?.scrollIntoView({ block: "nearest" });
      }
    });
  }
}

// connect the protected property sensor editor
function bindPropertySensorAdmin(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  // wire each reporting sensor selector
  for (const button of root.querySelectorAll<HTMLButtonElement>("[data-property-sensor-select]")) {
    // open one sensor editor
    button.addEventListener("click", () => {
      const sensorKey = button.dataset.propertySensorSelect;

      // reject incomplete rendered controls
      if (sensorKey === undefined) {
        return;
      }

      controller.setSelectedPropertySensor(sensorKey);
    });
  }

  const form = root.querySelector<HTMLFormElement>("[data-property-sensor-form]");
  const map = root.querySelector<SVGSVGElement>("[data-property-position-map]");

  // skip every non-admin route and incomplete render
  if (form === null || map === null) {
    return;
  }

  const latitudeInput = form.elements.namedItem("latitude");
  const longitudeInput = form.elements.namedItem("longitude");
  const nameInput = form.elements.namedItem("displayName");
  const viewport: MapViewport = {
    left: Number(map.dataset.viewportLeft),
    top: Number(map.dataset.viewportTop),
    zoom: Number(map.dataset.viewportZoom),
  };
  const updateMarker = (): void => {
    const latitude = latitudeInput instanceof HTMLInputElement
      ? latitudeInput.valueAsNumber
      : Number.NaN;
    const longitude = longitudeInput instanceof HTMLInputElement
      ? longitudeInput.valueAsNumber
      : Number.NaN;

    // preserve the last valid marker while fields are incomplete
    if (!Number.isFinite(latitude) || !Number.isFinite(longitude)) {
      return;
    }

    const point = projectMapPoint(latitude, longitude, viewport);
    const marker = map.querySelector<SVGGElement>("[data-property-position-marker]");

    // update the anchor before applying the current map transform
    if (marker !== null) {
      marker.dataset.propertyMapX = point.x.toFixed(2);
      marker.dataset.propertyMapY = point.y.toFixed(2);
      positionPropertyMapAnchors(map);
    }
  };

  // place the selected sensor at the tapped map point
  map.addEventListener("click", (event) => {
    const bounds = map.getBoundingClientRect();

    // suppress the placement click emitted after a pan
    if (map.dataset.propertyMapSuppressClick === "true") {
      delete map.dataset.propertyMapSuppressClick;
      return;
    }

    // reject a hidden or collapsed map
    if (bounds.width <= 0 || bounds.height <= 0) {
      return;
    }

    delete map.dataset.propertyMapSuppressClick;
    const displayX = (event.clientX - bounds.left) / bounds.width * PROPERTY_MAP_WIDTH;
    const displayY = (event.clientY - bounds.top) / bounds.height * PROPERTY_MAP_HEIGHT;
    const scale = Number(map.dataset.propertyMapScale ?? 1);
    const translateX = Number(map.dataset.propertyMapTranslateX ?? 0);
    const translateY = Number(map.dataset.propertyMapTranslateY ?? 0);
    const x = (displayX - translateX) / scale;
    const y = (displayY - translateY) / scale;
    const coordinate = inverseMapPoint(x, y, viewport);

    // update both explicit coordinate fields
    if (latitudeInput instanceof HTMLInputElement && longitudeInput instanceof HTMLInputElement) {
      latitudeInput.value = coordinate.latitude.toFixed(6);
      longitudeInput.value = coordinate.longitude.toFixed(6);
      updateMarker();
    }
  });

  // keep manual coordinate edits visible on the map
  if (latitudeInput instanceof HTMLInputElement && longitudeInput instanceof HTMLInputElement) {
    latitudeInput.addEventListener("input", updateMarker);
    longitudeInput.addEventListener("input", updateMarker);
  }

  // preview every map-icon selection immediately
  for (const input of form.querySelectorAll<HTMLInputElement>('input[name="icon"]')) {
    // reflect one checked category in the map pin
    input.addEventListener("change", () => {
      const markerIcon = map.querySelector<SVGTextElement>("[data-property-position-marker-icon]");

      // ignore unchecked or invalid rendered controls
      if (!input.checked || !isPropertySensorIcon(input.value) || markerIcon === null) {
        return;
      }

      markerIcon.textContent = propertySensorMaterialIcon(input.value);
    });
  }

  // persist one validated sensor layout
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const sensorKey = form.dataset.sensorKey;
    const icon = new FormData(form).get("icon");

    // reject incomplete rendered form contracts
    if (
      sensorKey === undefined ||
      !(nameInput instanceof HTMLInputElement) ||
      !(latitudeInput instanceof HTMLInputElement) ||
      !(longitudeInput instanceof HTMLInputElement) ||
      !isPropertySensorIcon(icon)
    ) {
      return;
    }

    void controller.savePropertySensorLayout(
      sensorKey,
      nameInput.value,
      icon,
      latitudeInput.valueAsNumber,
      longitudeInput.valueAsNumber,
    );
  });
}

// connect the synchronized forecast crosshair
function bindForecastCharts(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  bindForecastRangeControls(root, controller);
  const mapBinding = bindForecastWeatherMap(root);
  const grid = root.querySelector<HTMLElement>("[data-forecast-charts]");

  // require a rendered forecast stack
  if (grid === null) {
    return;
  }

  const parsedTimes = JSON.parse(grid.dataset.forecastTimes ?? "[]") as unknown;

  // require the rendered time contract
  if (!Array.isArray(parsedTimes) || parsedTimes.some((value) => typeof value !== "string")) {
    return;
  }

  const times = parsedTimes as string[];

  // require at least one forecast instant
  if (times.length === 0) {
    return;
  }

  const chartStates = [...grid.querySelectorAll<HTMLElement>("[data-forecast-chart]")].map(
    // cache one chart's rendered series contract
    (element) => ({
      element,
      format: element.dataset.forecastFormat as ForecastChartFormat,
      maximum: Number(element.dataset.forecastMax ?? 1),
      minimum: Number(element.dataset.forecastMin ?? 0),
      outputs: [...element.querySelectorAll<HTMLElement>("[data-forecast-value]")],
      series: JSON.parse(element.dataset.forecastSeries ?? "[]") as ForecastChartSeries[],
      value: element.querySelector<HTMLElement>(".forecast-chart-value"),
    }),
  );
  const shell = grid.closest<HTMLElement>(".forecast-chart-shell") ?? grid;
  const time = shell.querySelector<HTMLTimeElement>("[data-forecast-crosshair-time]");
  const timezone = controller.state.selectedSite?.timezone;
  let position = Math.max(0, Math.min(times.length - 1, Number(grid.dataset.forecastInitialIndex ?? 0)));
  let gesture: null | {
    horizontal: boolean;
    moved: boolean;
    pointerId: number;
    surface: HTMLElement | SVGSVGElement;
    startX: number;
    startY: number;
  } = null;
  const currentPosition = Math.max(0, Math.min(times.length - 1, Number(grid.dataset.forecastCurrentPosition ?? 0)));
  const currentRatio = times.length === 1 ? 0 : currentPosition / (times.length - 1);
  shell.style.setProperty("--forecast-current-time-position", `${String(currentRatio * 100)}%`);
  shell.classList.toggle("forecast-current-start", currentRatio < 0.1);
  shell.classList.toggle("forecast-current-end", currentRatio > 0.9);

  // render one continuous crosshair position
  const updatePosition = (nextPosition: number, immediateMap = false): void => {
    position = Math.max(0, Math.min(times.length - 1, nextPosition));
    const ratio = times.length === 1 ? 0 : position / (times.length - 1);
    const selectedIndex = Math.max(0, Math.min(times.length - 1, Math.round(position)));
    const selectedTime = interpolateForecastInstant(times, position);
    const summaries: string[] = [];
    grid.dataset.forecastSelectedPosition = String(position);
    shell.style.setProperty("--forecast-crosshair-position", `${String(ratio * 100)}%`);
    shell.classList.toggle("forecast-crosshair-start", ratio < 0.1);
    shell.classList.toggle("forecast-crosshair-end", ratio > 0.9);
    grid.setAttribute("aria-valuenow", String(selectedIndex));

    // update the shared clock
    if (time !== null) {
      time.dateTime = selectedTime;
      time.textContent = formatForecastHour(
        selectedTime,
        timezone,
        controller.state.forecastDays,
      );
    }

    // update every chart intersection
    for (const chart of chartStates) {
      const values = chart.series.map(
        // interpolate every line at the crosshair
        (series) => interpolateForecastValue(
          series.values,
          Math.min(Math.max(0, series.values.length - 1), position),
          chart.format !== "pressureChange",
        ),
      );
      const chartSummary: string[] = [];

      // update each displayed line value
      for (const [index, output] of chart.outputs.entries()) {
        const measurement = formatForecastChartValue(values[index] ?? null, chart.format, controller.state.units);
        const compact = compactMeasurement(measurement) ?? "—";
        output.textContent = compact;
        chartSummary.push(`${chart.series[index]?.label ?? "Value"} ${compact}`);
      }

      const edge = forecastValueLabelEdge(values, chart.minimum, chart.maximum);

      // retain the data-driven edge before checking heading clearance
      if (chart.value !== null) {
        chart.value.dataset.forecastValueEdge = edge;
      }

      const label = chart.element.querySelector("h3")?.textContent?.trim() ?? "Forecast";
      summaries.push(`${label}: ${chartSummary.join(", ")}`);
    }
    fitForecastValueLabels(grid);

    const clock = formatForecastHour(
      selectedTime,
      timezone,
      controller.state.forecastDays,
    );
    grid.setAttribute("aria-valuetext", `${clock}. ${summaries.join(". ")}`);
    mapBinding?.updateTime(selectedTime, immediateMap);
  };

  // convert one horizontal pointer coordinate to the shared index
  const positionFromPointer = (
    clientX: number,
    surface: HTMLElement | SVGSVGElement,
  ): number => {
    const bounds = surface.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (clientX - bounds.left) / Math.max(1, bounds.width)));
    return ratio * (times.length - 1);
  };

  const scrubSurfaces: readonly (HTMLElement | SVGSVGElement)[] = mapBinding === null
    ? [grid]
    : [grid, mapBinding.scrubSurface];

  // register one typed pointer event across HTML and SVG surfaces
  const addPointerListener = (
    surface: HTMLElement | SVGSVGElement,
    type: "pointercancel" | "pointerdown" | "pointermove" | "pointerup",
    listener: (event: PointerEvent) => void,
  ): void => {
    surface.addEventListener(type, listener as EventListener);
  };

  // connect every surface to the shared chart and map time
  for (const surface of scrubSurfaces) {
    // begin one undecided scrub or scroll gesture
    addPointerListener(surface, "pointerdown", (event) => {
      gesture = {
        horizontal: false,
        moved: false,
        pointerId: event.pointerId,
        surface,
        startX: event.clientX,
        startY: event.clientY,
      };
      grid.focus({ preventScroll: true });
    });

    // follow horizontal scrubbing without blocking vertical scrolling
    addPointerListener(surface, "pointermove", (event) => {
      // ignore hover and unrelated pointers
      if (
        gesture === null ||
        gesture.pointerId !== event.pointerId ||
        gesture.surface !== surface
      ) {
        return;
      }

      const deltaX = event.clientX - gesture.startX;
      const deltaY = event.clientY - gesture.startY;

      // wait for an intentional gesture
      if (Math.hypot(deltaX, deltaY) < 6) {
        return;
      }

      gesture.moved = true;

      // leave vertical motion to normal page scrolling
      if (!gesture.horizontal && Math.abs(deltaY) > Math.abs(deltaX)) {
        return;
      }

      gesture.horizontal = true;
      event.preventDefault();

      // capture only an established horizontal scrub
      if (!surface.hasPointerCapture(event.pointerId)) {
        try {
          surface.setPointerCapture(event.pointerId);
        } catch {
          // retain synthetic and legacy pointer support
        }
      }

      updatePosition(positionFromPointer(event.clientX, surface));
    });

    // finish one pointer gesture
    const finishGesture = (event: PointerEvent, selectTap: boolean): void => {
      // ignore unrelated pointer endings
      if (
        gesture === null ||
        gesture.pointerId !== event.pointerId ||
        gesture.surface !== surface
      ) {
        return;
      }

      // move a stationary tap directly to its thumb position
      if (selectTap && !gesture.moved) {
        updatePosition(positionFromPointer(event.clientX, surface), true);
      } else {
        // settle one completed scrub
        updatePosition(position, true);
      }

      // release an active browser capture
      if (surface.hasPointerCapture(event.pointerId)) {
        surface.releasePointerCapture(event.pointerId);
      }

      gesture = null;
    };
    addPointerListener(surface, "pointerup", (event) => {
      // complete a horizontal scrub or stationary tap
      finishGesture(event, true);
    });
    addPointerListener(surface, "pointercancel", (event) => {
      // preserve the crosshair during browser scrolling
      finishGesture(event, false);
    });
  }

  // provide precise keyboard scrubbing
  grid.addEventListener("keydown", (event) => {
    let nextPosition: number | null = null;

    // map horizontal navigation keys to hourly steps
    switch (event.key) {
      case "ArrowLeft":
        nextPosition = position - 1;
        break;
      case "ArrowRight":
        nextPosition = position + 1;
        break;
      case "Home":
        nextPosition = 0;
        break;
      case "End":
        nextPosition = times.length - 1;
        break;
    }

    // retain unrelated keyboard behavior
    if (nextPosition === null) {
      return;
    }

    event.preventDefault();
    updatePosition(nextPosition, true);
  });

  updatePosition(position, true);
}

// connect the Today-only Xweather timeline and layers
function bindForecastWeatherMap(root: HTMLElement): ForecastWeatherMapBinding | null {
  const map = root.querySelector<HTMLElement>("[data-forecast-weather-map]");

  // require the Today map
  if (map === null) {
    return null;
  }

  const canvas = map.querySelector<HTMLElement>(".forecast-map-canvas");
  const scrubSurface = map.querySelector<SVGSVGElement>("[data-forecast-map-scrubber]");
  const legend = map.querySelector<HTMLElement>("[data-forecast-map-legend]");
  const selectionPhase = map.querySelector<HTMLElement>("[data-forecast-map-selection-phase]");
  const selectionPhaseLabel = map.querySelector<HTMLElement>("[data-forecast-map-selection-phase-label]");
  const cacheProgress = map.querySelector<HTMLElement>("[data-forecast-map-cache-progress]");
  const cacheProgressLabel = map.querySelector<HTMLElement>("[data-forecast-map-cache-label]");
  const cacheProgressPercent = map.querySelector<HTMLElement>("[data-forecast-map-cache-percent]");
  const cacheProgressBar = map.querySelector<HTMLProgressElement>("[data-forecast-map-cache-bar]");
  const cacheProgressCount = map.querySelector<HTMLElement>("[data-forecast-map-cache-count]");
  const loading = map.querySelector<HTMLElement>("[data-forecast-map-loading]");
  const error = map.querySelector<HTMLElement>("[data-forecast-map-error]");
  const tiles = [...map.querySelectorAll<SVGImageElement>("[data-forecast-map-tile]")];
  const startMs = Number(map.dataset.forecastMapStart);
  const nowMs = Number(map.dataset.forecastMapNow);
  const endMs = Number(map.dataset.forecastMapEnd);
  const stepMs = Number(map.dataset.forecastMapStep);
  const initialMs = Number(map.dataset.forecastMapSelected);
  const timezone = map.dataset.forecastMapTimezone ?? "UTC";

  // require the rendered map contract
  if (
    !Number.isFinite(startMs) ||
    !Number.isFinite(nowMs) ||
    !Number.isFinite(endMs) ||
    !Number.isFinite(stepMs) ||
    !Number.isFinite(initialMs) ||
    endMs <= startMs ||
    stepMs <= 0 ||
    scrubSurface === null ||
    tiles.length === 0
  ) {
    return null;
  }

  const renderedFrame = tiles[0];
  const frameViewport = {
    height: Number(renderedFrame?.dataset.mapHeight),
    latitude: Number(renderedFrame?.dataset.mapLatitude),
    longitude: Number(renderedFrame?.dataset.mapLongitude),
    width: Number(renderedFrame?.dataset.mapWidth),
    zoom: Number(renderedFrame?.dataset.mapZoom),
  };

  // require one exact static-frame viewport
  if (
    tiles.length !== 1 ||
    !Number.isSafeInteger(frameViewport.height) ||
    !Number.isFinite(frameViewport.latitude) ||
    !Number.isFinite(frameViewport.longitude) ||
    !Number.isSafeInteger(frameViewport.width) ||
    !Number.isSafeInteger(frameViewport.zoom)
  ) {
    return null;
  }

  let layer = parseForecastMapLayer(map.dataset.forecastMapLayer) ?? "radar";
  let selectedMs = Math.max(startMs, Math.min(endMs, initialMs));
  let generation = 0;
  let updateTimer: ReturnType<typeof setTimeout> | null = null;
  let cacheTimer: ReturnType<typeof setTimeout> | null = null;
  let cacheGeneration = 0;
  const cachedImages = new Map<string, HTMLImageElement>();
  const cachedDisplayUrls = new Map<string, string>();
  const cachedUrls = new Set<string>();
  const tilePromises = new Map<string, Promise<boolean>>();
  const framePromises = new Map<string, Promise<boolean>>();
  const readyFrames = new Map<ForecastMapLayer, Set<number>>();
  const storageTilePromises = new Map<string, Promise<boolean>>();
  const storageFramePromises = new Map<string, Promise<boolean>>();
  const storedFrames = new Map<ForecastMapLayer, Set<number>>();
  let cacheTargets = new Set<number>();
  const clientTileCache = "caches" in window
    ? window.caches.open(FORECAST_MAP_CLIENT_CACHE_NAME).catch(
      // degrade gracefully when storage is denied
      () => null,
    )
    : Promise.resolve(null);

  // read one bounded selected map time
  const selectedTime = (): number => selectedMs;

  // align one instant to the provider frame grid
  const frameTime = (value: number): number => {
    const bounded = Math.max(startMs, Math.min(endMs, value));
    return startMs + Math.floor((bounded - startMs) / stepMs) * stepMs;
  };

  // align forecast requests to Xweather's hourly products
  const providerFrameTime = (value: number): number =>
    value <= nowMs
      ? value
      : Math.round(value / FORECAST_MAP_FORECAST_INTERVAL_MS) * FORECAST_MAP_FORECAST_INTERVAL_MS;

  // build every visible URL for one map frame
  const frameUrls = (frameMs: number, targetLayer: ForecastMapLayer): readonly string[] => {
    const phase: ForecastMapPhase = frameMs <= nowMs ? "history" : "forecast";
    const validTime = xweatherValidTime(new Date(providerFrameTime(frameMs)).toISOString());
    const url = xweatherFrameUrl(
      phase,
      targetLayer,
      validTime,
      frameViewport.zoom,
      frameViewport.width,
      frameViewport.height,
      frameViewport.latitude,
      frameViewport.longitude,
    );
    return [url];
  };

  // build one stable persistent-cache key
  const clientTileCacheKey = (target: string): string => {
    const url = new URL(target, window.location.href);
    url.hash = "";
    url.search = "";
    return url.href;
  };

  // identify one mutable forecast tile URL
  const isForecastTileUrl = (target: string): boolean =>
    new URL(target, window.location.href).pathname.includes("/maps/xweather/forecast/");

  // read one usable persistent tile response
  const readClientTileResponse = async (target: string): Promise<Response | null> => {
    const cache = await clientTileCache;

    // skip unavailable persistent browser storage
    if (cache === null) {
      return null;
    }

    try {
      const key = clientTileCacheKey(target);
      const response = await cache.match(key);

      // miss one uncached tile
      if (response === undefined) {
        return null;
      }

      // reuse immutable historical tiles indefinitely
      if (!isForecastTileUrl(target)) {
        return response;
      }

      const cachedAt = Number(response.headers.get(FORECAST_MAP_CLIENT_CACHE_TIMESTAMP_HEADER));

      // reuse one still-current forecast tile
      if (
        Number.isFinite(cachedAt) &&
        cachedAt > 0 &&
        Date.now() - cachedAt < FORECAST_MAP_CLIENT_FORECAST_FRESHNESS_MS
      ) {
        return response;
      }

      await cache.delete(key);
      return null;
    } catch {
      // fall back to the same-origin proxy
      return null;
    }
  };

  // persist one freshly fetched tile response
  const writeClientTile = async (
    target: string,
    blob: Blob,
    contentType: string,
    cachedAt: number,
  ): Promise<void> => {
    const cache = await clientTileCache;

    // skip unavailable persistent browser storage
    if (cache === null) {
      return;
    }

    try {
      const headers = new Headers({
        "Content-Type": contentType,
        [FORECAST_MAP_CLIENT_CACHE_TIMESTAMP_HEADER]: String(cachedAt),
      });
      await cache.put(
        clientTileCacheKey(target),
        new Response(blob, { headers, status: 200 }),
      );
    } catch {
      // retain the decoded in-memory fallback
    }
  };

  // fetch one tile through the same-origin server cache
  const fetchTile = async (target: string): Promise<Blob> => {
    const response = await fetch(target, {
      cache: isForecastTileUrl(target) ? "no-store" : "force-cache",
      headers: { Accept: "image/png,image/*;q=0.8" },
    });

    // reject provider and proxy failures
    if (!response.ok) {
      throw new Error(`weather tile request failed with ${String(response.status)}`);
    }

    const contentType = response.headers.get("Content-Type") ?? "image/png";

    // reject a non-image proxy response
    if (!contentType.toLowerCase().startsWith("image/")) {
      throw new Error("weather tile response was not an image");
    }

    const blob = await response.blob();

    // reject one empty tile payload
    if (blob.size === 0) {
      throw new Error("weather tile response was empty");
    }

    const serverAgeSeconds = Number(response.headers.get("X-Weather-Tile-Age"));
    const cachedAt = Number.isFinite(serverAgeSeconds) && serverAgeSeconds >= 0
      ? Date.now() - serverAgeSeconds * 1_000
      : Date.now();
    await writeClientTile(target, blob, contentType, cachedAt);
    return blob;
  };

  // load one persistent or remote tile payload
  const loadTile = async (target: string): Promise<Blob> => {
    const response = await readClientTileResponse(target);
    return response === null ? await fetchTile(target) : await response.blob();
  };

  // retain one compressed raster behind a reusable object URL
  const prepareDisplayUrl = (target: string, blob: Blob): string => {
    const existing = cachedDisplayUrls.get(target);

    // reuse one already-prepared browser payload
    if (existing !== undefined) {
      return existing;
    }

    const displayUrl = URL.createObjectURL(blob);
    cachedDisplayUrls.set(target, displayUrl);
    return displayUrl;
  };

  // report the active layer's background cache progress
  const updateCacheProgress = (targetLayer: ForecastMapLayer, state = "loading"): void => {
    // ignore stale layer progress
    if (targetLayer !== layer) {
      return;
    }

    const layerFrames = storedFrames.get(targetLayer) ?? new Set<number>();
    const ready = [...cacheTargets].filter(
      // count only the current sliding cache window
      (frame) => layerFrames.has(frame),
    ).length;
    const totalFrames = cacheTargets.size;
    const complete = totalFrames > 0 && ready === totalFrames;
    const nextState = complete ? "complete" : state;
    const percentage = totalFrames === 0 ? 100 : Math.round((ready / totalFrames) * 100);
    const layerLabel = FORECAST_MAP_LAYERS.find(
      // match the cache run's weather layer
      (option) => option.key === targetLayer,
    )?.label ?? "Weather";
    map.dataset.forecastMapCacheReady = String(ready);
    map.dataset.forecastMapCacheTotal = String(totalFrames);
    map.dataset.forecastMapCacheState = nextState;

    // synchronize the visible cache progress overlay
    if (cacheProgress !== null) {
      cacheProgress.hidden = complete;
      cacheProgress.dataset.forecastMapCacheState = nextState;
    }

    // describe the active layer cache run
    if (cacheProgressLabel !== null) {
      cacheProgressLabel.textContent = nextState === "partial"
        ? `${layerLabel} cache paused`
        : `Caching ${layerLabel}`;
    }

    // show one stable whole-number completion value
    if (cacheProgressPercent !== null) {
      cacheProgressPercent.textContent = `${String(percentage)}%`;
    }

    // expose native progress semantics
    if (cacheProgressBar !== null) {
      cacheProgressBar.max = totalFrames;
      cacheProgressBar.value = ready;
      cacheProgressBar.setAttribute("aria-label", `Cached ${layerLabel} map frames`);
    }

    // show the exact frame count beneath the bar
    if (cacheProgressCount !== null) {
      cacheProgressCount.textContent = `${String(ready)} of ${String(totalFrames)} nearby frames ready`;
    }
  };

  // persist one tile without decoding it into image memory
  const cacheTile = (target: string): Promise<boolean> => {
    const decoded = tilePromises.get(target);

    // reuse one selected-frame request
    if (decoded !== undefined) {
      return decoded;
    }

    const pending = storageTilePromises.get(target);

    // share one in-flight persistent write
    if (pending !== undefined) {
      return pending;
    }

    const request = (async (): Promise<boolean> => {
      try {
        const cached = await readClientTileResponse(target);

        // reuse one durable browser response without decoding it
        if (cached !== null) {
          prepareDisplayUrl(target, await cached.blob());
          return true;
        }

        prepareDisplayUrl(target, await fetchTile(target));
        return true;
      } catch {
        storageTilePromises.delete(target);
        return false;
      }
    })();
    storageTilePromises.set(target, request);
    return request;
  };

  // retain one decoded same-origin tile in browser storage
  const preloadTile = (target: string): Promise<boolean> => {
    // reuse one completed tile
    if (cachedUrls.has(target)) {
      return Promise.resolve(true);
    }

    const pending = tilePromises.get(target);

    // share one in-flight tile request
    if (pending !== undefined) {
      return pending;
    }

    const request = (async (): Promise<boolean> => {
      try {
        const storagePending = storageTilePromises.get(target);

        // finish one background write before reading it
        if (storagePending !== undefined && !await storagePending) {
          return false;
        }

        const displayUrl = cachedDisplayUrls.get(target) ?? prepareDisplayUrl(target, await loadTile(target));
        const image = new Image();
        image.decoding = "async";
        cachedImages.set(target, image);
        const decoded = await new Promise<boolean>((resolve) => {
          // retain one successfully decoded tile
          image.onload = () => resolve(true);
          // reject one corrupt image payload
          image.onerror = () => resolve(false);
          image.src = displayUrl;
        });

        // release one corrupt cached tile
        if (!decoded) {
          URL.revokeObjectURL(displayUrl);
          cachedImages.delete(target);
          cachedDisplayUrls.delete(target);
          tilePromises.delete(target);
          return false;
        }

        cachedUrls.add(target);
        return true;
      } catch {
        cachedImages.delete(target);
        cachedDisplayUrls.delete(target);
        tilePromises.delete(target);
        return false;
      }
    })();
    tilePromises.set(target, request);
    return request;
  };

  // cache one complete frame without decoding its raster
  const cacheFrame = (frameMs: number, targetLayer: ForecastMapLayer): Promise<boolean> => {
    const boundedFrame = frameTime(frameMs);
    const key = `${targetLayer}:${String(boundedFrame)}`;
    const existing = storageFramePromises.get(key);

    // share one in-flight or completed persistent frame
    if (existing !== undefined) {
      return existing;
    }

    const request = Promise.all(frameUrls(boundedFrame, targetLayer).map(cacheTile)).then(
      // retain only complete persistent frames
      (results) => {
        const complete = results.every(Boolean);

        // record one fully persisted frame
        if (complete) {
          const layerFrames = storedFrames.get(targetLayer) ?? new Set<number>();
          layerFrames.add(boundedFrame);
          storedFrames.set(targetLayer, layerFrames);
          const layerReadyFrames = readyFrames.get(targetLayer) ?? new Set<number>();
          layerReadyFrames.add(boundedFrame);
          readyFrames.set(targetLayer, layerReadyFrames);
        } else {
          // allow one incomplete frame to retry
          storageFramePromises.delete(key);
        }

        return complete;
      },
    );
    storageFramePromises.set(key, request);
    return request;
  };

  // cache every visible tile for one provider frame
  const preloadFrame = (frameMs: number, targetLayer: ForecastMapLayer): Promise<boolean> => {
    const boundedFrame = frameTime(frameMs);
    const key = `${targetLayer}:${String(boundedFrame)}`;
    const existing = framePromises.get(key);

    // share one in-flight or completed frame
    if (existing !== undefined) {
      return existing;
    }

    const request = Promise.all(frameUrls(boundedFrame, targetLayer).map(preloadTile)).then(
      // retain only complete frames for instant swaps
      (results) => {
        const complete = results.every(Boolean);

        // record one complete decoded frame
        if (complete) {
          const layerFrames = readyFrames.get(targetLayer) ?? new Set<number>();
          layerFrames.add(boundedFrame);
          readyFrames.set(targetLayer, layerFrames);
        } else {
          // allow one incomplete frame to retry
          framePromises.delete(key);
        }

        return complete;
      },
    );
    framePromises.set(key, request);
    return request;
  };

  // determine whether one frame can swap without network delay
  const frameIsReady = (frameMs: number, targetLayer: ForecastMapLayer): boolean =>
    readyFrames.get(targetLayer)?.has(frameTime(frameMs)) === true;

  // describe the selected frame without loading tiles
  const updateDescription = (): void => {
    const selectedMs = selectedTime();
    const selectedInstant = new Date(selectedMs).toISOString();
    const phase: ForecastMapPhase = selectedMs <= nowMs ? "history" : "forecast";
    const phaseLabel = phase === "history" ? "Observed" : "Forecast";
    const layerLabel = FORECAST_MAP_LAYERS.find(
      // match the selected weather layer
      (option) => option.key === layer,
    )?.label ?? "Weather";
    const clock = formatForecastMapTime(selectedInstant, timezone);

    map.dataset.forecastMapPhase = phase;
    map.dataset.forecastMapSelected = String(selectedMs);

    // label the shared selector inside the map
    if (selectionPhase !== null) {
      selectionPhase.dataset.forecastMapSelectionPhase = phase;
    }

    // show the selected map phase in plain language
    if (selectionPhaseLabel !== null) {
      selectionPhaseLabel.textContent = phase === "history" ? "Historical" : "Forecast";
    }

    // synchronize the active overlay legend
    if (legend !== null) {
      const presentation = forecastMapLegend(layer, phase);
      legend.dataset.forecastMapLegendLayer = layer;
      legend.dataset.forecastMapLegendPhase = phase;
      legend.setAttribute("aria-label", `${presentation.title} color legend`);
      legend.innerHTML = renderForecastMapLegendContent(presentation);
    }

    // keep the map's accessible name current
    if (canvas !== null) {
      canvas.setAttribute("aria-label", `${phaseLabel} ${layerLabel.toLowerCase()} near Ballydídean at ${clock}`);
    }
  };

  // display one fully cached frame without partial tile flashes
  const applyFrame = (frameMs: number, targetLayer: ForecastMapLayer): void => {
    const targets = frameUrls(frameMs, targetLayer);
    map.dataset.forecastMapLayer = targetLayer;
    map.setAttribute("aria-busy", "false");

    // hide the in-place loading veil
    if (loading !== null) {
      loading.hidden = true;
    }

    // clear one prior transient tile error
    if (error !== null) {
      error.hidden = true;
    }

    // swap every decoded tile in one visual frame
    for (const [index, tile] of tiles.entries()) {
      const target = targets[index] ?? "";
      tile.dataset.mapTileUrl = target;
      tile.setAttribute("href", cachedDisplayUrls.get(target) ?? target);
    }
  };

  // load one bounded set of same-origin tiles
  const updateTiles = (): void => {
    generation += 1;
    const requestGeneration = generation;
    const selectedFrame = frameTime(selectedTime());
    const targetLayer = layer;
    map.dataset.forecastMapLayer = layer;

    // swap one already-decoded frame immediately
    if (frameIsReady(selectedFrame, targetLayer)) {
      applyFrame(selectedFrame, targetLayer);
      return;
    }

    map.setAttribute("aria-busy", "true");

    // show one in-place loading veil
    if (loading !== null) {
      loading.hidden = false;
    }

    // hide one prior transient tile error
    if (error !== null) {
      error.hidden = true;
    }

    void preloadFrame(selectedFrame, targetLayer).then(
      // commit only the latest complete selected frame
      (complete) => {
        // ignore a stale, detached, or superseded frame
        if (
          !map.isConnected ||
          requestGeneration !== generation ||
          targetLayer !== layer ||
          selectedFrame !== frameTime(selectedTime())
        ) {
          return;
        }

        // retain the previous complete image after a provider failure
        if (!complete) {
          map.setAttribute("aria-busy", "false");

          // clear the in-place loading veil
          if (loading !== null) {
            loading.hidden = true;
          }

          // surface one compact provider failure
          if (error !== null) {
            error.hidden = false;
          }
          return;
        }

        applyFrame(selectedFrame, targetLayer);
        updateCacheProgress(targetLayer);
      },
    );
  };

  // debounce only uncached raster refreshes while scrubbing
  const scheduleTiles = (immediate: boolean): void => {
    // replace one pending scrub refresh
    if (updateTimer !== null) {
      clearTimeout(updateTimer);
      updateTimer = null;
    }

    // load settled or cached changes without delay
    if (immediate || frameIsReady(selectedTime(), layer)) {
      updateTiles();
      return;
    }

    updateTimer = setTimeout(() => {
      updateTimer = null;
      updateTiles();
    }, 80);
  };

  // select one small window around the current frame
  const nearbyFrames = (): readonly number[] => {
    const frames: number[] = [];
    const selectedFrame = frameTime(selectedTime());

    // retain three frames on either side of the selection
    for (let offset = -3; offset <= 3; offset += 1) {
      const frame = selectedFrame + offset * stepMs;

      // keep the cache window inside Today
      if (frame >= startMs && frame <= endMs) {
        frames.push(frame);
      }
    }

    return frames;
  };

  // warm one small window around the current selection
  const warmWindow = async (
    targetLayer: ForecastMapLayer,
    requestGeneration: number,
  ): Promise<void> => {
    const frames = [...cacheTargets];
    let cursor = 0;
    updateCacheProgress(targetLayer);

    // cache one bounded stream of complete frames
    const worker = async (): Promise<void> => {
      // continue while this map and layer own the cache run
      while (
        cursor < frames.length &&
        map.isConnected &&
        requestGeneration === cacheGeneration &&
        targetLayer === layer
      ) {
        const frame = frames[cursor];
        cursor += 1;

        // retain the complete enumerated frame contract
        if (frame === undefined) {
          continue;
        }

        let complete = await cacheFrame(frame, targetLayer);

        // retry one transient incomplete frame once
        if (
          !complete &&
          map.isConnected &&
          requestGeneration === cacheGeneration &&
          targetLayer === layer
        ) {
          complete = await cacheFrame(frame, targetLayer);
        }

        updateCacheProgress(targetLayer, complete ? "loading" : "partial");
      }
    };

    await Promise.all([worker(), worker(), worker(), worker(), worker(), worker(), worker(), worker()]);

    // finalize only the active cache run
    if (
      map.isConnected &&
      requestGeneration === cacheGeneration &&
      targetLayer === layer
    ) {
      const layerFrames = storedFrames.get(targetLayer) ?? new Set<number>();
      const ready = [...cacheTargets].filter(
        // count only frames requested by this cache run
        (frame) => layerFrames.has(frame),
      ).length;
      updateCacheProgress(targetLayer, ready === cacheTargets.size ? "complete" : "partial");
    }
  };

  // schedule one cancellable nearby-frame cache
  const scheduleWindowCache = (delay: number): void => {
    cacheGeneration += 1;
    const requestGeneration = cacheGeneration;
    cacheTargets = new Set(nearbyFrames());

    // replace one pending layer cache
    if (cacheTimer !== null) {
      clearTimeout(cacheTimer);
    }

    updateCacheProgress(layer);
    cacheTimer = setTimeout(() => {
      cacheTimer = null;
      void warmWindow(layer, requestGeneration);
    }, delay);
  };

  // follow one shared chart-selected instant
  const updateTime = (value: string, immediate: boolean): void => {
    const requestedMs = new Date(value).getTime();

    // ignore an invalid shared clock
    if (!Number.isFinite(requestedMs)) {
      return;
    }

    const boundedMs = Math.max(startMs, Math.min(endMs, requestedMs));
    selectedMs = startMs + Math.floor((boundedMs - startMs) / stepMs) * stepMs;
    updateDescription();
    scheduleTiles(immediate);
    scheduleWindowCache(immediate ? 100 : 350);
  };

  // wire every weather-layer choice
  for (const button of map.querySelectorAll<HTMLButtonElement>("[data-forecast-map-layer]")) {
    button.addEventListener("click", () => {
      const selectedLayer = parseForecastMapLayer(button.dataset.forecastMapLayer);

      // reject impossible rendered layers
      if (selectedLayer === null || selectedLayer === layer) {
        return;
      }

      layer = selectedLayer;

      // reflect one selected map layer
      for (const option of map.querySelectorAll<HTMLButtonElement>("[data-forecast-map-layer]")) {
        option.setAttribute("aria-pressed", String(option === button));
      }

      updateDescription();
      scheduleTiles(true);
      scheduleWindowCache(250);
    });
  }

  updateDescription();
  scheduleWindowCache(750);
  return { scrubSurface, updateTime };
}

// validate one rendered forecast-map layer
function parseForecastMapLayer(value: string | undefined): ForecastMapLayer | null {
  // accept only the fixed public layer set
  if (
    value === "clouds" ||
    value === "precipitation" ||
    value === "radar" ||
    value === "wind"
  ) {
    return value;
  }

  return null;
}

// connect the reviewed forecast horizon buttons
function bindForecastRangeControls(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  // wire every rendered forecast horizon
  for (const button of root.querySelectorAll<HTMLButtonElement>("[data-forecast-days]")) {
    // load one selected forecast range
    button.addEventListener("click", () => {
      const days = Number(button.dataset.forecastDays);

      // reject impossible rendered values
      if (days !== 1 && days !== 5 && days !== 10) {
        return;
      }

      void controller.setForecastDays(days);
    });
  }
}

// interpolate one chart line at a fractional forecast index
export function interpolateForecastValue(
  values: readonly (number | null)[],
  position: number,
  allowPartial = true,
): number | null {
  // preserve an empty line honestly
  if (values.length === 0) {
    return null;
  }

  const bounded = Math.max(0, Math.min(values.length - 1, position));
  const lowerIndex = Math.floor(bounded);
  const upperIndex = Math.ceil(bounded);
  const lower = values[lowerIndex] ?? null;
  const upper = values[upperIndex] ?? null;

  // avoid implying a complete pressure window beside an unavailable endpoint
  if (lower === null || upper === null) {
    return allowPartial ? lower ?? upper : null;
  }

  // return the exact stored value
  if (lowerIndex === upperIndex) {
    return lower;
  }

  return lower + (upper - lower) * (bounded - lowerIndex);
}

// interpolate one timestamp at a fractional forecast index
export function interpolateForecastInstant(
  times: readonly string[],
  position: number,
): string {
  // preserve an empty clock honestly
  if (times.length === 0) {
    return "";
  }

  const bounded = Math.max(0, Math.min(times.length - 1, position));
  const lowerIndex = Math.floor(bounded);
  const upperIndex = Math.ceil(bounded);
  const lower = new Date(times[lowerIndex] ?? times[0] ?? "").getTime();
  const upper = new Date(times[upperIndex] ?? times.at(-1) ?? "").getTime();
  const instant = lower + (upper - lower) * (bounded - lowerIndex);
  return new Date(instant).toISOString();
}

// connect the dependency-free map layer controls
function bindMapControls(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  // wire every reviewed tile layer
  for (const button of root.querySelectorAll<HTMLButtonElement>("[data-map-layer]")) {
    // render the selected tile source
    button.addEventListener("click", () => {
      const layer = button.dataset.mapLayer;

      // reject impossible rendered values
      if (layer !== "roads" && layer !== "topo" && layer !== "satellite") {
        return;
      }

      controller.setMapLayer(layer);
    });
  }

  // wire every map and list station selector
  for (const control of root.querySelectorAll<HTMLElement>("[data-station-select]")) {
    const stationSlug = control.dataset.stationSelect;

    // reject incomplete rendered values
    if (stationSlug === undefined) {
      continue;
    }

    bindRelatedMapHighlight(root, control, "data-station-select", stationSlug);

    // reveal one station-only snapshot
    control.addEventListener("click", (event) => {
      event.preventDefault();
      controller.setSelectedStation(stationSlug);

      // reveal list details selected from the map
      if (control.classList.contains("station-marker")) {
        root.querySelector<HTMLElement>(`[data-station-current="${CSS.escape(stationSlug)}"]`)
          ?.scrollIntoView({ block: "nearest" });
      }
    });
  }
}

// connect the single-chart trend title flyover
function bindTrendMetricControl(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  const control = root.querySelector<HTMLElement>("[data-trend-metric-control]");

  // skip every non-trends route
  if (control === null) {
    return;
  }

  const trigger = control.querySelector<HTMLButtonElement>("[data-trend-metric-trigger]");
  const flyover = control.querySelector<HTMLElement>(".trend-metric-flyover");
  const options = [...control.querySelectorAll<HTMLButtonElement>("[data-trend-metric-option]")];

  // require the complete title control
  if (trigger === null || flyover === null) {
    return;
  }

  // synchronize the custom flyover state
  const setOpen = (open: boolean): void => {
    trigger.setAttribute("aria-expanded", String(open));
    flyover.hidden = !open;
  };

  // toggle the flyover from the title
  trigger.addEventListener("click", () => {
    setOpen(trigger.getAttribute("aria-expanded") !== "true");
  });

  // bind every reviewed metric choice
  for (const option of options) {
    option.addEventListener("click", () => {
      const metric = option.dataset.trendMetricOption;

      // reject impossible rendered values
      if (metric === undefined || !isTrendChartMetric(metric)) {
        return;
      }

      setOpen(false);
      controller.setSelectedTrendMetric(metric);
    });
  }

  // close the flyover from the keyboard
  control.addEventListener("keydown", (event) => {
    // preserve ordinary title and option keys
    if (event.key !== "Escape" || trigger.getAttribute("aria-expanded") !== "true") {
      return;
    }

    event.preventDefault();
    setOpen(false);
    trigger.focus();
  });
}

// validate one rendered trend measurement key
function isTrendChartMetric(value: string): value is TrendChartMetric {
  return TREND_CHART_OPTIONS.some(
    // match one reviewed chart option
    (option) => option.metric === value,
  );
}

// connect aggregate and daily-detail trend controls
function bindTrendViewControls(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  const mode = root.querySelector<HTMLButtonElement>("[data-trend-mode-toggle]");
  const detail = root.querySelector<HTMLButtonElement>("[data-trend-detail-toggle]");

  // switch between aggregate and individual lines
  mode?.addEventListener("click", () => {
    controller.toggleTrendDisplayMode();
  });

  // switch between the rolling overview and fixed daily canvas
  detail?.addEventListener("click", () => {
    controller.toggleTrendDetail();
  });
}

// connect the extreme-day family and threshold controls
function bindTrendExtremeControls(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  const kind = root.querySelector<HTMLSelectElement>("[data-trend-extreme-kind]");
  const threshold = root.querySelector<HTMLInputElement>("[data-trend-extreme-threshold]");

  // switch threshold families with one reviewed default
  kind?.addEventListener("change", () => {
    // reject an impossible rendered family
    if (!isTrendExtremeKind(kind.value)) {
      return;
    }

    controller.setTrendExtremeKind(kind.value);
  });

  // apply one preferred-unit threshold at the chart boundary
  threshold?.addEventListener("change", () => {
    const value = Number(threshold.value);
    const configuration = trendExtremeConfiguration(
      normalizeTrendExtremeKind(controller.state.trendExtremeKind),
    );

    // reject empty or malformed number input
    if (!Number.isFinite(value)) {
      return;
    }

    controller.setTrendExtremeThreshold(
      trendCanonicalValue(value, configuration.format, controller.state.units),
    );
  });
}

// validate one rendered extreme-day family
function isTrendExtremeKind(value: string): value is TrendExtremeKind {
  return value === "cold" || value === "heat" || value === "rain" || value === "wind";
}

// connect yearly trend lines to visual emphasis
function bindTrendYearControls(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  const controls = root.querySelectorAll<HTMLElement | SVGElement>("[data-trend-year-select]");

  // bind every visible line and its forgiving tap target
  for (const control of controls) {
    const year = Number(control.dataset.trendYearSelect);

    // reject malformed rendered years
    if (!Number.isInteger(year)) {
      continue;
    }

    // emphasize one selected year
    const selectYear = (): void => {
      controller.setSelectedTrendYear(year);
    };
    control.addEventListener("click", selectYear);

    // add keyboard semantics only to SVG line buttons
    if (control instanceof SVGElement) {
      control.addEventListener("keydown", (event) => {
        // support the SVG button from a keyboard
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          selectYear();
        }
      });
    }
  }
}

// connect one shared calendar-date scrubber to the visible trend lines
function bindTrendCrosshair(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  const surface = root.querySelector<HTMLElement>("[data-trend-scrub-surface]");
  const chart = surface?.closest<HTMLElement>("[data-trend-chart]") ?? null;
  const viewport = surface?.closest<HTMLElement>(".trend-chart-viewport") ?? null;
  const slider = surface?.querySelector<HTMLElement>("[data-trend-crosshair-slider]") ?? null;
  // target the data canvas instead of a control glyph
  const svg = surface?.querySelector<SVGSVGElement>(":scope > svg") ?? null;
  const metric = chart?.dataset.trendChart;
  const displayMode = chart?.dataset.trendDisplayMode;
  const detail = chart?.dataset.trendDetail;

  // require one complete rendered trend contract
  if (
    surface === null ||
    chart === null ||
    viewport === null ||
    slider === null ||
    svg === null ||
    metric === undefined ||
    !isTrendChartMetric(metric) ||
    (displayMode !== "aggregate" && displayMode !== "all") ||
    (detail !== "daily" && detail !== "rolling")
  ) {
    return;
  }

  const option = TREND_CHART_OPTIONS.find(
    // resolve the rendered measurement configuration
    (candidate) => candidate.metric === metric,
  );

  // reject one impossible reviewed metric
  if (option === undefined) {
    return;
  }

  const rawSeries = buildTrendChartYearSeries(controller.state, option);
  const currentYear = trendCurrentYear(controller.state, rawSeries);
  const series = detail === "rolling" && option.supportsDetail !== false
    ? smoothTrendYearSeries(rawSeries, TREND_ROLLING_WINDOW_DAYS)
    : rawSeries;
  const currentSeries = series.find(
    // locate the permanent current-year comparison
    (year) => year.year === currentYear,
  );
  const historicalSeries = series.filter(
    // exclude the incomplete current year from historical statistics
    (year) => year.year !== currentYear,
  );
  const aggregate = buildTrendAggregateSeries(
    // retain a useful fallback before a second calendar year exists
    historicalSeries.length === 0 ? series : historicalSeries,
  );
  const renderedSelectedYear = Number(chart.dataset.selectedTrendYear);
  const selectedYear = Number.isInteger(renderedSelectedYear) ? renderedSelectedYear : null;
  const visibleSeries = buildTrendCrosshairSeries(
    series,
    aggregate,
    displayMode,
    selectedYear,
    currentYear,
  );
  const legendSamples: readonly Readonly<{ readonly value: number; readonly x: number }>[] = [
    ...aggregate.flatMap(
      // retain every visible historical band edge
      (point) => [
        { value: point.minimum, x: point.x },
        { value: point.lowerQuartile, x: point.x },
        { value: point.upperQuartile, x: point.x },
        { value: point.maximum, x: point.x },
      ],
    ),
    ...visibleSeries.flatMap(
      // retain every visible comparison line
      (entry) => entry.points,
    ),
  ];
  const newestFirst = [...visibleSeries].reverse();
  const outputs = new Map(
    [...surface.querySelectorAll<HTMLOutputElement>("[data-trend-crosshair-value]")].flatMap(
      // index every rendered line output
      (output) => {
        const key = output.dataset.trendCrosshairValue;
        return key === undefined ? [] : [[key, output] as const];
      },
    ),
  );
  const date = surface.querySelector<HTMLTimeElement>("[data-trend-crosshair-date]");
  const legend = chart.querySelector<HTMLElement>(".trend-chart-legend");
  const summary = surface.querySelector<HTMLElement>(".trend-crosshair-summary");
  const maximum = Number(chart.dataset.trendMaximum);
  const minimum = Number(chart.dataset.trendMinimum);

  // require the rendered comparison card
  if (
    legend === null ||
    summary === null ||
    !Number.isFinite(maximum) ||
    !Number.isFinite(minimum)
  ) {
    return;
  }

  const span = maximum === minimum ? 1 : maximum - minimum;

  let position = Math.max(0, Math.min(1, Number(surface.dataset.trendInitialPosition ?? 0)));
  const expanded = detail === "daily";
  let gesture: null | {
    mode: "pan" | "scrub";
    moved: boolean;
    pointerId: number;
    startScrollLeft: number;
    startScrollTop: number;
    startX: number;
    startY: number;
    targetWasYear: boolean;
  } = null;
  let suppressYearClick = false;

  // convert one screen coordinate through the rotated SVG transform
  const positionFromPointer = (clientX: number, clientY: number): number => {
    const matrix = svg.getScreenCTM();

    // fall back to an unrotated bounding box
    if (matrix === null) {
      const bounds = svg.getBoundingClientRect();
      return Math.max(0, Math.min(1, (clientX - bounds.left) / Math.max(1, bounds.width)));
    }

    try {
      const point = svg.createSVGPoint();
      point.x = clientX;
      point.y = clientY;
      const local = point.matrixTransform(matrix.inverse());
      const plotWidth = TREND_CHART_WIDTH - TREND_CHART_PADDING_LEFT - TREND_CHART_PADDING_RIGHT;
      return Math.max(0, Math.min(1, (local.x - TREND_CHART_PADDING_LEFT) / plotWidth));
    } catch {
      // preserve pointer input during a transient browser transform
      return position;
    }
  };

  // detect the rotated phone chart interaction axis
  const isMobileTrend = (): boolean => window.matchMedia("(max-width: 42rem)").matches;

  // move the legend to the quieter visible data edge
  const updateLegendPlacement = (): void => {
    const mobile = isMobileTrend();
    const visibleStart = expanded
      ? mobile
        ? viewport.scrollTop
        : viewport.scrollLeft
      : 0;
    const visibleWidth = expanded
      ? mobile
        ? viewport.clientHeight
        : viewport.clientWidth
      : surface.clientWidth;
    const legendHeight = legend.offsetHeight;
    const legendLeft = visibleStart + visibleWidth / 2 - legend.offsetWidth / 2;
    const legendRight = legendLeft + legend.offsetWidth;
    const rootFontSize = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
    const edgeGap = rootFontSize * (mobile ? 0.4 : 0.55);
    const topPosition = surface.clientHeight * 0.15 + edgeGap;
    const bottomPosition = surface.clientHeight * 0.88 - edgeGap - legendHeight;

    // score plotted samples behind one candidate legend band
    const overlapScore = (candidateTop: number): number => legendSamples.reduce(
      // total direct and nearby data overlap
      (score, point) => {
        const pointX = (trendChartX(point.x) / TREND_CHART_WIDTH) * surface.clientWidth;
        const pointY = (trendChartY(point.value, minimum, span) / TREND_CHART_HEIGHT) *
          surface.clientHeight;
        const withinLegendWidth = pointX >= legendLeft && pointX <= legendRight;
        const verticalDistance = Math.max(
          candidateTop - pointY,
          0,
          pointY - (candidateTop + legendHeight),
        );
        const proximity = Math.max(0, 1 - verticalDistance / Math.max(1, legendHeight));
        return score + (withinLegendWidth ? proximity : 0);
      },
      0,
    );

    const topOverlap = overlapScore(topPosition);
    const bottomOverlap = overlapScore(bottomPosition);
    const placement = topOverlap < bottomOverlap ? "top" : "bottom";
    legend.classList.toggle("trend-chart-legend-top", placement === "top");
    legend.dataset.trendLegendPlacement = placement;
  };

  // place the comparison flag at the current-year intersection
  const updateSummaryPosition = (): void => {
    const linePosition = (trendChartPercentage(position) / 100) * surface.clientWidth;
    const viewportEnd = isMobileTrend()
      ? viewport.scrollTop + viewport.clientHeight
      : viewport.scrollLeft + viewport.clientWidth;
    const rightSpace = viewportEnd - linePosition;
    const currentValue = currentSeries === undefined
      ? null
      : interpolateTrendValue(currentSeries.points, position);
    const plotTop = (TREND_CHART_PADDING_TOP / TREND_CHART_HEIGHT) * surface.clientHeight;
    const plotBottom = ((TREND_CHART_HEIGHT - TREND_CHART_PADDING_BOTTOM) / TREND_CHART_HEIGHT) *
      surface.clientHeight;
    const summaryHeight = summary.offsetHeight;
    const targetTop = currentValue === null
      ? plotTop
      : (trendChartY(currentValue, minimum, span) / TREND_CHART_HEIGHT) * surface.clientHeight -
        summaryHeight / 2;
    const maximumTop = Math.max(plotTop, plotBottom - summaryHeight);
    summary.classList.toggle("trend-crosshair-summary-left", rightSpace < summary.offsetWidth + 8);
    summary.style.top = `${Math.max(plotTop, Math.min(maximumTop, targetTop)).toFixed(2)}px`;
  };

  // align every floating chart overlay after viewport movement
  const updateOverlayPositions = (): void => {
    updateLegendPlacement();
    updateSummaryPosition();
  };

  // update the line, date, and every visible value without rerendering
  const updatePosition = (nextPosition: number): void => {
    position = Math.max(0, Math.min(1, nextPosition));
    const percentage = trendChartPercentage(position);
    const selectedDate = trendCalendarDate(position);
    const summaries: string[] = [];
    surface.style.setProperty("--trend-crosshair-position", `${percentage.toFixed(4)}%`);
    surface.classList.toggle("trend-crosshair-start", position < 0.1);
    surface.classList.toggle("trend-crosshair-end", position > 0.9);
    slider.setAttribute("aria-valuenow", String(Math.round(position * 365)));

    // update the visible shared calendar date
    if (date !== null) {
      date.dateTime = selectedDate.key;
      date.textContent = selectedDate.label;
    }

    // update every visible line intersection
    for (const entry of newestFirst) {
      const measurement = formatTrendMeasurement(
        interpolateTrendValue(entry.points, position),
        option.format,
        controller.state.units,
      );
      const compact = compactMeasurement(measurement) ?? "—";
      outputs.get(entry.key)?.replaceChildren(compact);
      summaries.push(`${entry.label} ${compact === "—" ? "unavailable" : compact}`);
    }

    slider.setAttribute("aria-valuetext", `${selectedDate.label}. ${summaries.join(". ")}`);
    updateSummaryPosition();
  };

  // begin one calendar scrub or fixed-detail pan
  surface.addEventListener("pointerdown", (event) => {
    const target = event.target instanceof Element ? event.target : null;

    // retain native chart controls
    if (target?.closest("[data-trend-metric-control], [data-trend-mode-toggle], [data-trend-detail-toggle], .trend-chart-legend") !== null) {
      return;
    }

    const targetWasYear = target?.closest("[data-trend-year-select]") !== null;
    gesture = {
      mode: expanded && target?.closest("[data-trend-crosshair-slider]") === null && !targetWasYear
        ? "pan"
        : "scrub",
      moved: false,
      pointerId: event.pointerId,
      startScrollLeft: viewport.scrollLeft,
      startScrollTop: viewport.scrollTop,
      startX: event.clientX,
      startY: event.clientY,
      targetWasYear,
    };
    slider.focus({ preventScroll: true });
  });

  // follow one deliberate swipe along the rendered chart axis
  surface.addEventListener("pointermove", (event) => {
    // ignore hover and unrelated pointers
    if (gesture === null || gesture.pointerId !== event.pointerId) {
      return;
    }

    // wait for intentional pointer travel
    if (Math.hypot(event.clientX - gesture.startX, event.clientY - gesture.startY) < 6) {
      return;
    }

    gesture.moved = true;
    event.preventDefault();

    // capture only an established chart gesture
    if (!surface.hasPointerCapture(event.pointerId)) {
      try {
        surface.setPointerCapture(event.pointerId);
      } catch {
        // retain synthetic pointer support
      }
    }

    // pan the fixed daily canvas instead of moving its selected date
    if (gesture.mode === "pan") {
      // pan along the rotated phone's annual axis
      if (isMobileTrend()) {
        viewport.scrollTop = gesture.startScrollTop - (event.clientY - gesture.startY);
      } else {
        // pan the expanded desktop chart horizontally
        viewport.scrollLeft = gesture.startScrollLeft - (event.clientX - gesture.startX);
      }

      updateOverlayPositions();
      return;
    }

    updatePosition(positionFromPointer(event.clientX, event.clientY));
  });

  // finish one swipe or stationary date selection
  const finishGesture = (event: PointerEvent, selectTap: boolean): void => {
    // ignore unrelated pointer endings
    if (gesture === null || gesture.pointerId !== event.pointerId) {
      return;
    }

    suppressYearClick = gesture.moved && gesture.targetWasYear;

    // move a stationary expanded-chart tap without interrupting panning
    if (
      selectTap &&
      gesture.mode === "pan" &&
      !gesture.moved &&
      !gesture.targetWasYear
    ) {
      updatePosition(positionFromPointer(event.clientX, event.clientY));
    }

    // preserve stationary yearly-line selection during date scrubbing
    if (
      selectTap &&
      gesture.mode === "scrub" &&
      (!gesture.targetWasYear || gesture.moved)
    ) {
      updatePosition(positionFromPointer(event.clientX, event.clientY));
    }

    // release one active browser capture
    if (surface.hasPointerCapture(event.pointerId)) {
      surface.releasePointerCapture(event.pointerId);
    }

    gesture = null;
  };
  surface.addEventListener("pointerup", (event) => {
    // complete one date selection
    finishGesture(event, true);
  });
  surface.addEventListener("pointercancel", (event) => {
    // retain the latest date during cancellation
    finishGesture(event, false);
  });

  // suppress a synthetic yearly-line click after a swipe
  surface.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;

    // preserve stationary line selection and unrelated clicks
    if (!suppressYearClick || target?.closest("[data-trend-year-select]") === null) {
      suppressYearClick = false;
      return;
    }

    event.preventDefault();
    event.stopImmediatePropagation();
    suppressYearClick = false;
  }, true);

  // pan only the fixed daily canvas with a wheel
  surface.addEventListener("wheel", (event) => {
    // retain ordinary page and browser zoom behavior in overview mode
    if (!expanded || event.ctrlKey) {
      return;
    }

    const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY)
      ? event.deltaX
      : event.deltaY;

    // preserve zero-delta wheel events
    if (delta === 0) {
      return;
    }

    event.preventDefault();

    // scroll the rotated phone chart along its annual axis
    if (isMobileTrend()) {
      viewport.scrollTop += delta;
    } else {
      // scroll the desktop chart along its annual axis
      viewport.scrollLeft += delta;
    }

    updateOverlayPositions();
  }, { passive: false });

  // provide exact daily keyboard steps
  slider.addEventListener("keydown", (event) => {
    let nextPosition: number | null = null;

    // map horizontal navigation to one calendar day
    switch (event.key) {
      case "ArrowLeft":
        nextPosition = position - 1 / 365;
        break;
      case "ArrowRight":
        nextPosition = position + 1 / 365;
        break;
      case "Home":
        nextPosition = 0;
        break;
      case "End":
        nextPosition = 1;
        break;
    }

    // preserve unrelated keyboard controls
    if (nextPosition === null) {
      return;
    }

    event.preventDefault();
    updatePosition(nextPosition);
  });

  viewport.addEventListener("scroll", updateOverlayPositions, { passive: true });
  window.addEventListener("resize", updateOverlayPositions);
  const todayPosition = Math.max(0, Math.min(1, Number(surface.dataset.trendTodayPosition ?? 0)));
  surface.style.setProperty("--trend-today-position", `${trendChartPercentage(todayPosition).toFixed(4)}%`);
  updatePosition(position);
  updateLegendPlacement();

  // center the selected day when opening the fixed daily canvas
  if (expanded) {
    window.requestAnimationFrame(() => {
      const linePosition = (trendChartPercentage(position) / 100) * surface.clientWidth;

      // center along the rotated phone's annual axis
      if (isMobileTrend()) {
        viewport.scrollTop = Math.max(0, linePosition - viewport.clientHeight / 2);
      } else {
        // center along the desktop annual axis
        viewport.scrollLeft = Math.max(0, linePosition - viewport.clientWidth / 2);
      }

      updateOverlayPositions();
    });
  }
}

// connect the unit settings page to browser preferences
function bindUnitSettings(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  const form = root.querySelector<HTMLFormElement>("[data-unit-settings-form]");

  // skip every non-settings route
  if (form === null) {
    return;
  }

  // persist one complete preference form
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const data = new FormData(form);
    controller.setUnitPreferences({
      precipitation: data.get("precipitation"),
      pressure: data.get("pressure"),
      temperature: data.get("temperature"),
      waterLevel: data.get("waterLevel"),
      windSpeed: data.get("windSpeed"),
    });
  });
}

// create an exact optional filter property
function optionalFilter<Key extends keyof HistoryFilters>(
  key: Key,
  value: HistoryFilters[Key],
): Partial<HistoryFilters> {
  // omit empty filter values
  if (value === undefined) {
    return {};
  }

  return { [key]: value };
}

// require the fixed product location from route responses
function requireProductSite(site: WeatherSite): WeatherSite {
  // reject a mismatched route payload
  if (site.slug !== PRODUCT_SITE.slug) {
    throw new Error("Ballydídean weather is not available");
  }

  return site;
}

// read a non-empty form string
function readFormValue(value: FormDataEntryValue | null): string | undefined {
  // reject files and empty strings
  if (typeof value !== "string" || value.length === 0) {
    return undefined;
  }

  return value;
}

// convert a site wall clock input to UTC
function toInstant(
  value: FormDataEntryValue | null,
  timezone: string,
): string | undefined {
  const text = readFormValue(value);

  // preserve an empty input
  if (text === undefined) {
    return undefined;
  }

  return fromSiteWallClock(text, timezone);
}

// format a value and unit for table cells
function formatMetricCell(value: number | null, unit: string): string {
  // render missing values consistently
  if (value === null) {
    return "—";
  }

  return `${escapeHtml(formatNumber(value))} ${escapeHtml(unit)}`;
}

// format one configurable measurement table cell
function formatMeasurementCell(
  value: number | null,
  kind: keyof UnitPreferences,
  preferences: UnitPreferences,
): string {
  const measurement = formatMeasurement(value, kind, preferences);

  // omit the unit for unavailable measurements
  if (measurement.unit.length === 0) {
    return measurement.value;
  }

  return `${escapeHtml(measurement.value)} ${escapeHtml(measurement.unit)}`;
}

// format one measurement with a fixed unit
function formatFixedMeasurement(
  value: number | null,
  unit: string,
  maximumFractionDigits = 1,
): FormattedMeasurement {
  // preserve unavailable measurements
  if (value === null) {
    return { unit: "", value: "—" };
  }

  return { unit, value: formatNumber(value, maximumFractionDigits) };
}

// find the first available current metric
function findMetric(
  records: readonly WeatherRecord[],
  metric: WeatherMetricKey,
): number | null {
  const record = records.find(
    // retain one non-null normalized value
    (candidate) => candidate.metrics[metric] !== null,
  );
  return record?.metrics[metric] ?? null;
}

// resolve the safe prominent value for one metric
export function forecastMetricValue(
  record: WeatherRecord,
  metric: WeatherMetricKey,
  useAdjustments = true,
): number | null {
  const decision = record.adjustment;
  const rainDecision = record.rainAdjustment;
  const temperatureDecision = record.temperatureAdjustment;

  // use only locally generated sunlight projections without expanding governed model metrics
  if (useAdjustments && metric === "cloudCoverPercent") {
    return solarCloudValues.get(record) ?? record.metrics.cloudCoverPercent;
  }

  // reuse the hourly rain amount as its hourly rate
  if (
    useAdjustments &&
    (metric === "precipitationMm" || metric === "precipitationRateMmPerHour") &&
    rainDecision?.state === "active"
  ) {
    return rainDecision.correctedPrecipitationMm;
  }

  // use only explicit ECMWF temperature output when opted in
  if (
    useAdjustments &&
    metric === "temperatureC" &&
    temperatureDecision?.state === "active"
  ) {
    return temperatureDecision.correctedTemperatureC;
  }

  // use only an explicitly applied active value
  if (
    useAdjustments &&
    decision?.state === "active" &&
    FORECAST_ADJUSTMENT_METRIC_KEYS.has(metric as ForecastAdjustmentMetric) &&
    decision.appliedMetrics.includes(metric as ForecastAdjustmentMetric)
  ) {
    return decision.adjustedMetrics[metric as ForecastAdjustmentMetric] ?? record.metrics[metric];
  }

  // locally paired experiments never override a usable qualified server output
  if (useAdjustments) {
    const bias = localForecastBiases.get(record)?.[metric as LocalForecastBiasMetric];
    // forecast-only uv policies never alter humidity particulate or qualified families
    if (metric === "uvIndex") {
      const metadata = bias === undefined ? undefined : localForecastPolicies.get(bias);
      return metadata === undefined ? record.metrics[metric] : projectForecastLocalBias(record.metrics[metric], record.validAt, bias ?? null, metadata.policy);
    }
    return projectLocalForecastBias(record.metrics[metric], record.validAt, bias ?? null);
  }

  return record.metrics[metric];
}

// distinguish actual numeric corrections from equal clamped or unavailable outputs
function adjustmentValuesDiffer(selected: number | null, raw: number | null): boolean {
  return selected !== null && raw !== null && Number.isFinite(selected) && Number.isFinite(raw) && selected !== raw;
}

// compare displayed summaries without marking unavailable values or color-only changes
function adjustmentMeasurementsDiffer(selected: FormattedMeasurement, raw: FormattedMeasurement): boolean {
  return selected.value !== "—" && raw.value !== "—" && (selected.value !== raw.value || selected.unit !== raw.unit);
}

// restrict homepage markers to the summary values actually displayed on the card
function forecastReadingsDiffer(selected: ForecastCardValue, raw: ForecastCardValue): boolean {
  return selected.readings.some(
    // align daily maxima minima and totals without trusting adjustment metadata
    (reading, index) => raw.readings[index] !== undefined && adjustmentMeasurementsDiffer(reading.measurement, raw.readings[index]!.measurement),
  );
}

// find the largest available metric value
function maximumMetric(
  records: readonly WeatherRecord[],
  metric: WeatherMetricKey,
  useAdjustments = false,
): number | null {
  const values = records.flatMap((record) => {
    const value = forecastMetricValue(record, metric, useAdjustments);
    return value === null ? [] : [value];
  });
  return values.length === 0 ? null : Math.max(...values);
}

// find the smallest available metric value
function minimumMetric(
  records: readonly WeatherRecord[],
  metric: WeatherMetricKey,
  useAdjustments = false,
): number | null {
  const values = records.flatMap((record) => {
    const value = forecastMetricValue(record, metric, useAdjustments);
    return value === null ? [] : [value];
  });
  return values.length === 0 ? null : Math.min(...values);
}

// retain forecast hours inside one site-local calendar day
export function forecastForSiteDay(
  records: readonly WeatherRecord[],
  reference: string,
  timezone: string,
): readonly WeatherRecord[] {
  return forecastForSiteDays(records, reference, timezone, 1);
}

// retain forecast hours inside reviewed site-local calendar days
export function forecastForSiteDays(
  records: readonly WeatherRecord[],
  reference: string,
  timezone: string,
  days: ForecastDays,
): readonly WeatherRecord[] {
  const referenceParts = formatWallClockParts(new Date(reference), timezone);
  const targetDates = new Set<string>();

  // collect every requested local calendar date
  for (let index = 0; index < days; index += 1) {
    const date = new Date(Date.UTC(
      referenceParts.year,
      referenceParts.month - 1,
      referenceParts.day + index,
    ));
    targetDates.add(date.toISOString().slice(0, 10));
  }

  return records.filter(
    // exclude every prior and following local date
    (record) => targetDates.has(forecastSiteDateKey(record.validAt, timezone)),
  );
}

// format one site-local date key
function forecastSiteDateKey(value: string, timezone: string): string {
  const parts = formatWallClockParts(new Date(value), timezone);
  return [parts.year, parts.month, parts.day]
    .map(
      // retain stable two-digit calendar fields
      (part, index) => index === 0 ? String(part) : String(part).padStart(2, "0"),
    )
    .join("-");
}

// format one high and low forecast pair
function forecastRange(
  records: readonly WeatherRecord[],
  metric: WeatherMetricKey,
  kind: keyof UnitPreferences,
  units: UnitPreferences,
  maximumFractionDigits: number,
  classify: (value: number | null) => ConditionBand,
  useAdjustments: boolean,
): ForecastCardValue {
  const maximumValue = maximumMetric(records, metric, useAdjustments);
  const minimumValue = minimumMetric(records, metric, useAdjustments);
  const maximum = formatMeasurement(maximumValue, kind, units, maximumFractionDigits);
  const minimum = formatMeasurement(minimumValue, kind, units, maximumFractionDigits);
  return {
    readings: [
      {
        label: "Max",
        measurement: maximum,
        tone: forecastRangeTone(maximumValue, kind, classify),
      },
      {
        label: "Min",
        measurement: minimum,
        tone: forecastRangeTone(minimumValue, kind, classify),
      },
    ],
  };
}

// pair forecast ranges with apparent temperature first
function forecastTemperature(
  records: readonly WeatherRecord[],
  units: UnitPreferences,
  useAdjustments: boolean,
): ForecastCardValue {
  const air = forecastRange(
    records,
    "temperatureC",
    "temperature",
    units,
    0,
    temperatureBand,
    useAdjustments,
  );
  const apparent = forecastRange(
    records,
    "apparentTemperatureC",
    "temperature",
    units,
    0,
    temperatureBand,
    useAdjustments,
  );
  return { readings: [...apparent.readings, ...air.readings] };
}

// select one range tone without mixing metric palettes
function forecastRangeTone(
  value: number | null,
  kind: keyof UnitPreferences,
  classify: (value: number | null) => ConditionBand,
): ForecastTone {
  // preserve semantic temperature bands over interpolated colors
  if (kind === "temperature") {
    return forecastTemperatureTone(value);
  }

  return forecastToneForBand(value, classify(value));
}

// format forecast wind and gust maxima
function forecastWind(
  records: readonly WeatherRecord[],
  units: UnitPreferences,
  useAdjustments: boolean,
): ForecastCardValue {
  const windValue = maximumMetric(records, "windSpeedMps", useAdjustments);
  const gustValue = maximumMetric(records, "windGustMps", useAdjustments);
  const wind = formatMeasurement(windValue, "windSpeed", units, 0);
  const gust = formatMeasurement(gustValue, "windSpeed", units, 0);
  return {
    readings: [
      { label: "Max", measurement: wind, tone: forecastWindTone(windValue, units) },
      { label: "Max", measurement: gust, tone: forecastWindTone(gustValue, units) },
    ],
  };
}

// format forecast rain rate and day-end accumulation
function forecastRain(
  records: readonly WeatherRecord[],
  units: UnitPreferences,
  useAdjustments: boolean,
): ForecastCardValue {
  const maximumRate = maximumMetric(records, "precipitationRateMmPerHour", useAdjustments);
  const accumulation = totalMetric(records, "precipitationMm", useAdjustments);
  return {
    readings: [
      {
        label: "Max",
        measurement: formatPrecipitationRate(maximumRate, units),
        tone: forecastToneForBand(maximumRate, rainBand(maximumRate)),
      },
      {
        label: "Total",
        measurement: formatPrecipitationAccumulation(accumulation, units),
      },
    ],
  };
}

// total one normalized metric
function totalMetric(
  records: readonly WeatherRecord[],
  metric: WeatherMetricKey,
  useAdjustments: boolean,
): number | null {
  const values = records.flatMap((record) => {
    const value = forecastMetricValue(record, metric, useAdjustments);
    return value === null ? [] : [value];
  });
  return values.length === 0
    ? null
    : values.reduce(
      // add one interval accumulation
      (total, value) => total + value,
      0,
    );
}

// color maximum humidity using only temperatures recorded at that same humidity
function forecastHumidity(records: readonly WeatherRecord[], useAdjustments: boolean): ForecastCardValue {
  const value = maximumMetric(records, "relativeHumidityPercent", useAdjustments);
  const matchingHours = records.filter(
    // exclude unrelated daily temperature peaks
    (record) => value !== null && forecastMetricValue(record, "relativeHumidityPercent", useAdjustments) === value,
  );
  const temperatureC = maximumMetric(matchingHours, "temperatureC", useAdjustments);
  return {
    readings: [{
      label: "Max",
      measurement: formatFixedMeasurement(value, "%", 0),
      tone: forecastToneForBand(value, humidityBand(value, temperatureC)),
    }],
  };
}

// format one fixed-unit forecast maximum
function forecastMaximumFixed(
  records: readonly WeatherRecord[],
  metric: WeatherMetricKey,
  unit: string,
  maximumFractionDigits: number,
  classify: (value: number | null) => ConditionBand,
  useAdjustments: boolean,
): ForecastCardValue {
  const value = maximumMetric(records, metric, useAdjustments);
  const measurement = formatFixedMeasurement(
    value,
    unit,
    maximumFractionDigits,
  );
  return {
    readings: [{ label: "Max", measurement, tone: forecastToneForBand(value, classify(value)) }],
  };
}

// format one daily precipitation accumulation
function formatPrecipitationAccumulation(
  value: number | null,
  units: UnitPreferences,
): FormattedMeasurement {
  // preserve useful small-rain precision
  const maximumFractionDigits = units.precipitation === "inches" ? 2 : 1;
  return formatMeasurement(
    value,
    "precipitation",
    units,
    maximumFractionDigits,
  );
}

// format one hourly precipitation rate preference
function formatPrecipitationRate(
  value: number | null,
  units: UnitPreferences,
): FormattedMeasurement {
  const measurement = formatMeasurement(value, "precipitation", units);
  return {
    unit: measurement.unit.length === 0 ? "" : `${measurement.unit}/h`,
    value: measurement.value,
  };
}

// format one forecast clock time in the site timezone
function formatForecastTime(value: string, timezone?: string): string {
  return new Intl.DateTimeFormat("en-US", {
    hour: "numeric",
    minute: "2-digit",
    timeZone: timezone ?? "UTC",
  }).format(new Date(value));
}

// format compact metric precision
function formatNumber(value: number, maximumFractionDigits = 1): string {
  return new Intl.NumberFormat("en-US", { maximumFractionDigits }).format(value);
}

// format an instant in the site timezone
function formatInstant(value: string, timezone?: string): string {
  return new Intl.DateTimeFormat("en-US", {
    dateStyle: "medium",
    timeStyle: "short",
    timeZone: timezone ?? "UTC",
  }).format(new Date(value));
}

// format one compact forecast hour
function formatForecastHour(
  value: string,
  timezone: string | undefined,
  days: ForecastDays,
): string {
  return new Intl.DateTimeFormat("en-US", {
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    month: "short",
    timeZone: timezone ?? "UTC",
    weekday: days === 1 ? undefined : "short",
  }).format(new Date(value));
}

// format one compact multi-day panel label
function formatForecastDayPanel(value: string, timezone?: string): string {
  const instant = new Date(value);
  const day = new Intl.DateTimeFormat("en-US", {
    day: "numeric",
    timeZone: timezone ?? "UTC",
  }).format(instant);
  const weekday = new Intl.DateTimeFormat("en-US", {
    timeZone: timezone ?? "UTC",
    weekday: "short",
  }).format(instant);
  return `${weekday} ${day}`;
}

// format one compact axis hour
function formatForecastAxisHour(value: string, timezone?: string): string {
  return new Intl.DateTimeFormat("en-US", {
    hour: "numeric",
    timeZone: timezone ?? "UTC",
  }).format(new Date(value));
}

// format one compact multi-day axis date
function formatForecastAxisDate(value: string, timezone?: string): string {
  return new Intl.DateTimeFormat("en-US", {
    day: "numeric",
    month: "short",
    timeZone: timezone ?? "UTC",
  }).format(new Date(value));
}

// render an ISO value for a site input
function toLocalInput(value: string | undefined, timezone: string): string {
  // preserve an empty filter
  if (value === undefined) {
    return "";
  }

  return toSiteWallClock(value, timezone);
}

interface WallClockParts {
  readonly day: number;
  readonly hour: number;
  readonly minute: number;
  readonly month: number;
  readonly year: number;
}

// convert one site wall clock to UTC
export function fromSiteWallClock(value: string, timezone: string): string {
  const requested = parseWallClock(value);
  const requestedEpoch = wallClockEpoch(requested);
  const requestedWallClock = formatWallClock(requested);
  const matches = new Set<number>();

  // collect nearby timezone offsets
  for (
    let delta = -WALL_CLOCK_OFFSET_WINDOW_MS;
    delta <= WALL_CLOCK_OFFSET_WINDOW_MS;
    delta += WALL_CLOCK_OFFSET_SAMPLE_MS
  ) {
    const sampleEpoch = requestedEpoch + delta;
    const represented = formatWallClockParts(new Date(sampleEpoch), timezone);
    const offset = wallClockEpoch(represented) - sampleEpoch;
    const candidateEpoch = requestedEpoch - offset;
    const candidate = new Date(candidateEpoch);

    // retain exact round-trip matches
    if (
      Number.isFinite(candidateEpoch) &&
      formatWallClock(formatWallClockParts(candidate, timezone)) ===
        requestedWallClock
    ) {
      matches.add(candidateEpoch);
    }
  }

  // require one unambiguous instant
  if (matches.size !== 1) {
    throw new RangeError("history wall clock is not valid in the site timezone");
  }

  const candidateEpoch = [...matches][0]!;
  return new Date(candidateEpoch).toISOString();
}

// convert one UTC instant to a site wall clock
export function toSiteWallClock(value: string, timezone: string): string {
  const instant = new Date(value);

  // reject invalid stored instants
  if (!Number.isFinite(instant.getTime())) {
    throw new RangeError("history instant must be valid");
  }

  return formatWallClock(formatWallClockParts(instant, timezone));
}

// parse one minute-precision wall clock
function parseWallClock(value: string): WallClockParts {
  const match =
    /^(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})T(?<hour>\d{2}):(?<minute>\d{2})$/u.exec(
      value,
    );

  // require the browser datetime shape
  if (match?.groups === undefined) {
    throw new RangeError("history wall clock must use YYYY-MM-DDTHH:mm");
  }

  const parts = {
    day: Number(match.groups.day),
    hour: Number(match.groups.hour),
    minute: Number(match.groups.minute),
    month: Number(match.groups.month),
    year: Number(match.groups.year),
  };
  const normalized = new Date(wallClockEpoch(parts));

  // reject normalized calendar overflow
  if (
    normalized.getUTCFullYear() !== parts.year ||
    normalized.getUTCMonth() + 1 !== parts.month ||
    normalized.getUTCDate() !== parts.day ||
    normalized.getUTCHours() !== parts.hour ||
    normalized.getUTCMinutes() !== parts.minute
  ) {
    throw new RangeError("history wall clock must be a valid calendar value");
  }

  return parts;
}

// format one instant in a site timezone
function formatWallClockParts(
  instant: Date,
  timezone: string,
): WallClockParts {
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

  // collect named calendar parts
  for (const part of formatter.formatToParts(instant)) {
    // ignore locale punctuation
    if (part.type !== "literal") {
      values.set(part.type, part.value);
    }
  }

  return {
    day: requireWallClockPart(values, "day"),
    hour: requireWallClockPart(values, "hour"),
    minute: requireWallClockPart(values, "minute"),
    month: requireWallClockPart(values, "month"),
    year: requireWallClockPart(values, "year"),
  };
}

// require one formatted calendar part
function requireWallClockPart(
  values: ReadonlyMap<string, string>,
  name: string,
): number {
  const value = values.get(name);

  // fail closed on incomplete formatting
  if (value === undefined) {
    throw new RangeError(`site timezone omitted ${name}`);
  }

  return Number(value);
}

// compare one minute-precision wall clock
function formatWallClock(parts: WallClockParts): string {
  return `${String(parts.year).padStart(4, "0")}-${String(parts.month).padStart(2, "0")}-${String(parts.day).padStart(2, "0")}T${String(parts.hour).padStart(2, "0")}:${String(parts.minute).padStart(2, "0")}`;
}

// project wall clock fields onto a UTC epoch
function wallClockEpoch(parts: WallClockParts): number {
  return Date.UTC(
    parts.year,
    parts.month - 1,
    parts.day,
    parts.hour,
    parts.minute,
  );
}

// escape untrusted text for HTML contexts
function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

// load one protected scorecard without affecting other administrator panels
async function getAdminForecastAdjustmentScorecard(
  fetcher: typeof fetch,
  url: string,
): Promise<Readonly<{
  publicationState: ForecastAdjustmentScorecardPublicationState | null;
  scorecard: ForecastAdjustmentScorecard | null;
  state: Exclude<ForecastAdjustmentScorecardLoadState, "loading">;
}>> {
  try {
    const response = await fetcher(url, {
      cache: "no-store",
      credentials: "same-origin",
      headers: { accept: "application/json" },
    });

    // distinguish an expired administrator session without exposing server detail
    if (response.status === 401 || response.status === 403) {
      return { publicationState: null, scorecard: null, state: "unauthorized" };
    }

    // collapse missing, stale and corrupt publications into one safe state
    if (!response.ok) {
      return { publicationState: null, scorecard: null, state: "unavailable" };
    }

    const body = forecastAdjustmentObject(await response.json());

    // reject response-envelope additions that could smuggle private fields
    if (body === null ||
      !hasExactForecastAdjustmentKeys(body, new Set(["data", "publicationState"]))) {
      return { publicationState: null, scorecard: null, state: "unavailable" };
    }
    const publicationState = body.publicationState;
    const scorecard = parseForecastAdjustmentScorecard(body.data);
    const validPublicationState = publicationState === "current" ||
      publicationState === "pending_unapplied" || publicationState === "legacy_display";
    const generationMatches = scorecard !== null && (
      scorecard.contractVersion === "forecast-adjustment-scorecard/v1"
        ? publicationState === "legacy_display"
        : publicationState === "current" || publicationState === "pending_unapplied"
    );
    return !validPublicationState || !generationMatches
      ? { publicationState: null, scorecard: null, state: "unavailable" }
      : { publicationState, scorecard, state: "ready" };
  } catch {
    return { publicationState: null, scorecard: null, state: "unavailable" };
  }
}

// load one JSON contract
async function getJson<ResponseBody>(
  fetcher: typeof fetch,
  url: string,
): Promise<ResponseBody> {
  const response = await fetcher(url, { headers: { accept: "application/json" } });

  // expose a bounded browser error
  if (!response.ok) {
    throw new Error(`Weather request failed with status ${String(response.status)}`);
  }

  return (await response.json()) as ResponseBody;
}

// read one bounded uncached location claim without browser credentials
async function getHomeNetworkViewerContext(fetcher: typeof fetch, apiBaseUrl: string): Promise<boolean> {
  const abort = new AbortController();
  let timeout: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<boolean>(
    // fail closed after the bounded deadline
    (resolve) => {
      // deny a stalled private-context request
      timeout = setTimeout(() => {
        abort.abort();
        resolve(false);
      }, 4_000);
    },
  );

  try {
    return await Promise.race([
      fetcher(buildViewerContextUrl(apiBaseUrl), {
        cache: "no-store",
        credentials: "omit",
        headers: { accept: "application/json" },
        signal: abort.signal,
      }).then(
        // accept only a successful boolean claim
        async (response) => {
          // deny HTTP errors and malformed positive claims
          if (!response.ok) {
            return false;
          }

          const body: unknown = await response.json();
          return forecastAdjustmentObject(forecastAdjustmentObject(body)?.data)?.homeNetwork === true;
        },
      ).catch(
        // deny failed requests or invalid JSON
        () => false,
      ),
      deadline,
    ]);
  } finally {
    clearTimeout(timeout);
    abort.abort();
  }
}

// load only the compact recorded projection rather than paging archives in every browser
async function getForecastObservations(fetcher: typeof fetch, apiBaseUrl: string, siteSlug: string): Promise<readonly ForecastObservationPoint[]> {
  const abort = new AbortController();
  const timer = setTimeout(() => abort.abort(), 35_000);
  try {
    const response = await fetcher(`${normalizeBaseUrl(apiBaseUrl)}/sites/${encodeURIComponent(siteSlug)}/forecast-observations`, {
      headers: { accept: "application/json" }, signal: abort.signal,
    });
    // an optional history failure never removes the separately loaded future forecast
    if (!response.ok) {
      throw new Error("Recorded forecast history is unavailable");
    }
    return parseForecastObservationResponse(await response.json());
  } finally {
    clearTimeout(timer);
  }
}

// fetch only the compact fixed-site daylight evidence without exposing archive geometry
async function getSolarCloudEvidence(fetcher: typeof fetch, apiBaseUrl: string, site: WeatherSite): Promise<SolarCloudEvidence | null> {
  const abort = new AbortController();
  const timer = setTimeout(() => abort.abort(), 35_000);
  try {
    const response = await fetcher(`${normalizeBaseUrl(apiBaseUrl)}/sites/${encodeURIComponent(site.slug)}/solar-cloud-evidence`, {
      headers: { accept: "application/json" }, signal: abort.signal,
    });
    // optional failure never removes separately loaded weather or renews retained evidence
    if (!response.ok) {
      throw new Error("Solar cloud evidence is unavailable");
    }
    return parseSolarCloudEvidenceResponse(await response.json(), site);
  } finally {
    clearTimeout(timer);
  }
}

// load and sanitize one forecast response
async function getForecastJson(
  fetcher: typeof fetch,
  url: string,
): Promise<ForecastRecordsResponse> {
  return parseForecastRecordsResponse(await getJson<unknown>(fetcher, url));
}

// write one authenticated JSON contract
async function putJson<ResponseBody>(
  fetcher: typeof fetch,
  url: string,
  body: Readonly<Record<string, unknown>>,
): Promise<ResponseBody> {
  const response = await fetcher(url, {
    body: JSON.stringify(body),
    credentials: "same-origin",
    headers: {
      accept: "application/json",
      "content-type": "application/json",
    },
    method: "PUT",
  });

  // expose a bounded browser error
  if (!response.ok) {
    throw new Error(`Weather request failed with status ${String(response.status)}`);
  }

  return (await response.json()) as ResponseBody;
}

// normalize an optional API base URL
function normalizeBaseUrl(value: string): string {
  return value.replace(/\/$/u, "");
}
