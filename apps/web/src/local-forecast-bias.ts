import { forecastCorrectionWeight, type ForecastCorrectionPolicy } from "./forecast-correction-policy.js";
import { clearSkyRadiation } from "./solar-cloud.js";
import type { SolarCloudSample } from "./solar-cloud.js";

export type LocalForecastBiasMetric =
  | "relativeHumidityPercent"
  | "pm25MicrogramsPerCubicMeter"
  | "uvIndex"
  | "pressureChange3hHpa";

export interface LocalForecastSample extends SolarCloudSample {
  readonly forecastProduct?: boolean;
}

export interface LocalForecastBias {
  readonly contractVersion: "local-forecast-bias-experiment/v1";
  readonly metric: LocalForecastBiasMetric;
  readonly adjustmentType: "additive" | "multiplicative";
  readonly adjustmentValue: number;
  readonly observedAt: string;
  readonly evaluatedAt: string;
  readonly expiresAt: string;
  readonly observationRecordId: string;
  readonly observationSourceId: string;
  readonly regionalRecordId: string;
  readonly regionalSourceId: string;
  readonly regionalValidAt: string;
  readonly observedValue: number;
  readonly regionalValue: number;
}

export interface LocalForecastBiasInput {
  readonly metric: LocalForecastBiasMetric;
  readonly now: string;
  readonly observed: LocalForecastSample | null;
  readonly regional: LocalForecastSample | null;
  readonly latitude: number;
  readonly longitude: number;
}

interface MetricSpec {
  readonly adjustmentType: "additive" | "multiplicative";
  readonly minimum: number;
  readonly maximum: number;
  readonly minimumAdjustment: number;
  readonly maximumAdjustment: number;
}

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const MAX_OBSERVED_AGE_MS = 15 * MINUTE_MS;
const MAX_REGIONAL_AGE_MS = 60 * MINUTE_MS;
const MAX_SAMPLE_PAIR_GAP_MS = 60 * MINUTE_MS;
const MIN_SOLAR_ELEVATION_DEGREES = 15;
const MIN_CLEAR_SKY_RADIATION_WM2 = 200;
const EXPLICIT_TIMESTAMP = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,3}))?(Z|[+-](\d{2}):(\d{2}))$/u;

// define conservative physical and adjustment limits for each experiment
const METRIC_SPECS: Readonly<Record<LocalForecastBiasMetric, MetricSpec>> = {
  relativeHumidityPercent: {
    adjustmentType: "additive",
    maximum: 100,
    maximumAdjustment: 20,
    minimum: 0,
    minimumAdjustment: -20,
  },
  pm25MicrogramsPerCubicMeter: {
    adjustmentType: "additive",
    maximum: 1000,
    maximumAdjustment: 50,
    minimum: 0,
    minimumAdjustment: -50,
  },
  uvIndex: {
    adjustmentType: "multiplicative",
    maximum: 20,
    maximumAdjustment: 1.5,
    minimum: 0,
    minimumAdjustment: 0.2,
  },
  pressureChange3hHpa: {
    adjustmentType: "additive",
    maximum: 20,
    maximumAdjustment: 3,
    minimum: -20,
    minimumAdjustment: -3,
  },
};

// create one short-lived local-versus-regional scalar bias
export function createLocalForecastBias(input: LocalForecastBiasInput): LocalForecastBias | null {
  const spec = metricSpec(input.metric);
  const now = parseExplicitTimestamp(input.now);
  const observed = input.observed;
  const regional = input.regional;
  // require one supported metric and two explicit source readings
  if (spec === null || now === null || observed === null || regional === null) {
    return null;
  }

  const observedValidAt = parseExplicitTimestamp(observed.validAt);
  const observedReceivedAt = parseExplicitTimestamp(observed.receivedAt);
  const regionalValidAt = parseExplicitTimestamp(regional.validAt);
  const regionalReceivedAt = parseExplicitTimestamp(regional.receivedAt);
  // reject malformed provenance timestamps
  if (
    observedValidAt === null || observedReceivedAt === null ||
    regionalValidAt === null || regionalReceivedAt === null
  ) {
    return null;
  }

  // accept only fresh causal pairs inside their collection cadences
  if (
    observed.freshnessStatus !== "fresh" || regional.freshnessStatus !== "fresh" ||
    observedValidAt > now || observedReceivedAt > now ||
    regionalValidAt > now || regionalReceivedAt > now ||
    observedReceivedAt < observedValidAt ||
    (regional.forecastProduct !== true && regionalReceivedAt < regionalValidAt) ||
    now - observedValidAt > MAX_OBSERVED_AGE_MS ||
    now - observedReceivedAt > MAX_OBSERVED_AGE_MS ||
    now - regionalValidAt > MAX_REGIONAL_AGE_MS ||
    now - regionalReceivedAt > MAX_REGIONAL_AGE_MS ||
    Math.abs(observedValidAt - regionalValidAt) > MAX_SAMPLE_PAIR_GAP_MS
  ) {
    return null;
  }

  // require complete immutable provenance identifiers
  if (
    observed.recordId.trim() === "" || observed.sourceId.trim() === "" ||
    regional.recordId.trim() === "" || regional.sourceId.trim() === ""
  ) {
    return null;
  }

  // reject unavailable or physically invalid scalar values
  if (
    !isMetricValue(observed.value, spec) ||
    !isMetricValue(regional.value, spec) ||
    (input.metric === "uvIndex" && regional.value < 1)
  ) {
    return null;
  }

  // gate UV ratios on meaningful daylight at observation and evaluation
  if (input.metric === "uvIndex" && !hasMeaningfulDaylight(input, observed.validAt)) {
    return null;
  }

  const adjustmentValue = deriveAdjustment(input.metric, observed.value, regional.value, spec);
  // fail raw when arithmetic cannot produce a bounded correction
  if (adjustmentValue === null) {
    return null;
  }

  return {
    adjustmentType: spec.adjustmentType,
    adjustmentValue,
    contractVersion: "local-forecast-bias-experiment/v1",
    evaluatedAt: input.now,
    expiresAt: new Date(observedValidAt + DAY_MS).toISOString(),
    metric: input.metric,
    observationRecordId: observed.recordId,
    observationSourceId: observed.sourceId,
    observedAt: observed.validAt,
    observedValue: observed.value,
    regionalRecordId: regional.recordId,
    regionalSourceId: regional.sourceId,
    regionalValidAt: regional.validAt,
    regionalValue: regional.value,
  };
}

// project one fixed observation bias without compounding prior projections
export function projectLocalForecastBias(
  raw: number | null,
  targetAt: string,
  bias: LocalForecastBias | null,
): number | null {
  const context = localProjectionContext(raw, targetAt, bias);
  // rejected metadata preserves the original regional value
  if (context === null || raw === null || bias === null) {
    return raw;
  }
  const { spec, target, observed } = context;
  const weight = Math.max(0, 1 - (target - observed) / DAY_MS);
  const projected = spec.adjustmentType === "multiplicative"
    ? raw * (1 + (bias.adjustmentValue - 1) * weight)
    : raw + bias.adjustmentValue * weight;
  // clamp the corrected scalar to its physical presentation range
  return clamp(projected, spec.minimum, spec.maximum);
}

// project only future uv and pressure through one frozen horizon policy
export function projectForecastLocalBias(
  raw: number | null,
  targetAt: string,
  bias: LocalForecastBias | null,
  policy: ForecastCorrectionPolicy,
): number | null {
  const context = localProjectionContext(raw, targetAt, bias);
  // unrelated families retain their legacy entry point and arithmetic
  if (context === null || raw === null || bias === null ||
    (bias.metric !== "uvIndex" && bias.metric !== "pressureChange3hHpa")) {
    return raw;
  }
  const weight = forecastCorrectionWeight(context.target, context.observed, policy);
  // a zero or retired policy does not clamp or alter the raw value
  if (weight === 0) {
    return raw;
  }
  const projected = bias.adjustmentType === "multiplicative"
    ? raw * (1 + (bias.adjustmentValue - 1) * weight)
    : raw + bias.adjustmentValue * weight;
  return clamp(projected, context.spec.minimum, context.spec.maximum);
}

// validate the existing projection contract once for both pure entry points
function localProjectionContext(
  raw: number | null,
  targetAt: string,
  bias: LocalForecastBias | null,
): { readonly spec: MetricSpec; readonly target: number; readonly observed: number } | null {
  // preserve unavailable input before inspecting optional metadata
  if (raw === null || bias === null) {
    return null;
  }

  const spec = metricSpec(bias.metric);
  // preserve non-finite or physically invalid regional forecast values
  if (spec === null || !isMetricValue(raw, spec)) {
    return null;
  }

  const target = parseExplicitTimestamp(targetAt);
  const observed = parseExplicitTimestamp(bias.observedAt);
  const evaluated = parseExplicitTimestamp(bias.evaluatedAt);
  const expires = parseExplicitTimestamp(bias.expiresAt);
  const regionalValidAt = parseExplicitTimestamp(bias.regionalValidAt);
  // preserve raw outside one coherent forward projection contract
  if (
    bias.contractVersion !== "local-forecast-bias-experiment/v1" ||
    target === null || observed === null || evaluated === null ||
    expires === null || regionalValidAt === null ||
    expires !== observed + DAY_MS || evaluated < observed || evaluated >= expires ||
    regionalValidAt > evaluated || Math.abs(observed - regionalValidAt) > MAX_SAMPLE_PAIR_GAP_MS ||
    target < evaluated || target >= expires ||
    bias.adjustmentType !== spec.adjustmentType ||
    bias.observationRecordId.trim() === "" || bias.observationSourceId.trim() === "" ||
    bias.regionalRecordId.trim() === "" || bias.regionalSourceId.trim() === "" ||
    !isMetricValue(bias.observedValue, spec) || !isMetricValue(bias.regionalValue, spec) ||
    (bias.metric === "uvIndex" && bias.regionalValue < 1)
  ) {
    return null;
  }

  const expectedAdjustment = deriveAdjustment(
    bias.metric,
    bias.observedValue,
    bias.regionalValue,
    spec,
  );
  // reject altered or out-of-bounds adjustment metadata
  if (expectedAdjustment === null || bias.adjustmentValue !== expectedAdjustment) {
    return null;
  }

  return { spec, target, observed };
}

// require useful sunlight for observation-derived UV ratios
function hasMeaningfulDaylight(input: LocalForecastBiasInput, observedAt: string): boolean {
  const observedSky = clearSkyRadiation(input.latitude, input.longitude, observedAt);
  const evaluatedSky = clearSkyRadiation(input.latitude, input.longitude, input.now);
  // reject invalid coordinates darkness low sun and weak expected radiation
  if (observedSky === null || evaluatedSky === null) {
    return false;
  }

  return (
    observedSky.solarElevationDegrees >= MIN_SOLAR_ELEVATION_DEGREES &&
    evaluatedSky.solarElevationDegrees >= MIN_SOLAR_ELEVATION_DEGREES &&
    observedSky.irradianceWm2 >= MIN_CLEAR_SKY_RADIATION_WM2 &&
    evaluatedSky.irradianceWm2 >= MIN_CLEAR_SKY_RADIATION_WM2
  );
}

// derive one bounded additive delta or UV ratio
function deriveAdjustment(
  metric: LocalForecastBiasMetric,
  observed: number,
  regional: number,
  spec: MetricSpec,
): number | null {
  // divide only a qualified nonzero regional UV estimate
  if (metric === "uvIndex") {
    const ratio = observed / regional;
    return Number.isFinite(ratio)
      ? clamp(ratio, spec.minimumAdjustment, spec.maximumAdjustment)
      : null;
  }

  const delta = observed - regional;
  return Number.isFinite(delta)
    ? clamp(delta, spec.minimumAdjustment, spec.maximumAdjustment)
    : null;
}

// resolve one runtime-safe metric specification
function metricSpec(metric: unknown): MetricSpec | null {
  // reject forged metric names from untyped callers or serialized metadata
  if (typeof metric !== "string" || !Object.hasOwn(METRIC_SPECS, metric)) {
    return null;
  }

  return METRIC_SPECS[metric as LocalForecastBiasMetric];
}

// validate one nullable scalar against a metric's physical range
function isMetricValue(value: number | null, spec: MetricSpec): value is number {
  return value !== null && Number.isFinite(value) && value >= spec.minimum && value <= spec.maximum;
}

// parse one real ISO timestamp carrying an explicit offset
function parseExplicitTimestamp(value: string): number | null {
  const match = EXPLICIT_TIMESTAMP.exec(value);
  // reject missing offsets and malformed calendar fields
  if (match === null) {
    return null;
  }

  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const hour = Number(match[4]);
  const minute = Number(match[5]);
  const second = Number(match[6]);
  const offsetHour = match[8] === "Z" ? 0 : Number(match[9]);
  const offsetMinute = match[8] === "Z" ? 0 : Number(match[10]);
  // reject overflow fields before Date normalization
  if (
    month < 1 || month > 12 || day < 1 || day > daysInMonth(year, month) ||
    hour > 23 || minute > 59 || second > 59 || offsetHour > 23 || offsetMinute > 59
  ) {
    return null;
  }

  const timestamp = Date.parse(value);
  return Number.isFinite(timestamp) ? timestamp : null;
}

// count one Gregorian calendar month
function daysInMonth(year: number, month: number): number {
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return days[month - 1] ?? 0;
}

// constrain one finite scalar inclusively
function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
