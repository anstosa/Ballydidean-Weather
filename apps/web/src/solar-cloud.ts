import { forecastCorrectionWeight, type ForecastCorrectionPolicy } from "./forecast-correction-policy.js";

export type SolarCloudFreshnessStatus = "fresh" | "delayed" | "stale";

export interface SolarCloudSample {
  readonly value: number | null;
  readonly validAt: string;
  readonly receivedAt: string;
  readonly freshnessStatus: SolarCloudFreshnessStatus;
  readonly recordId: string;
  readonly sourceId: string;
}

export interface SolarCloudBias {
  readonly contractVersion: "solar-cloud-experiment/v1";
  readonly observedAt: string;
  readonly evaluatedAt: string;
  readonly expiresAt: string;
  readonly observationRecordId: string;
  readonly observationSourceId: string;
  readonly regionalRecordId: string;
  readonly regionalSourceId: string;
  readonly regionalValidAt: string;
  readonly observedRadiationWm2: number;
  readonly clearSkyRadiationWm2: number;
  readonly solarElevationDegrees: number;
  readonly rawCloudCoverPercent: number;
  readonly estimatedCloudCoverPercent: number;
  readonly biasPercentPoints: number;
}

export interface ClearSkyRadiation {
  readonly solarElevationDegrees: number;
  readonly irradianceWm2: number;
}

export interface SolarCloudBiasInput {
  readonly latitude: number;
  readonly longitude: number;
  readonly now: string;
  readonly ws90: SolarCloudSample | null;
  readonly regional: SolarCloudSample | null;
}

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const MAX_WS90_AGE_MS = 5 * MINUTE_MS;
const MAX_REGIONAL_AGE_MS = 20 * MINUTE_MS;
const MAX_SAMPLE_PAIR_GAP_MS = 20 * MINUTE_MS;
const MIN_SOLAR_ELEVATION_DEGREES = 15;
const MIN_CLEAR_SKY_RADIATION_WM2 = 200;
const CLEAR_SKY_TOLERANCE = 0.85;
const CLOUD_ATTENUATION_RANGE = 0.65;
const MAX_RADIATION_RATIO = 1.5;
const MAX_BIAS_PERCENT_POINTS = 80;
const EXPLICIT_TIMESTAMP = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,3}))?(Z|[+-](\d{2}):(\d{2}))$/u;

// estimate geometric sun position with NOAA equations and Haurwitz clear-sky irradiance
export function clearSkyRadiation(
  latitude: number,
  longitude: number,
  at: string,
): ClearSkyRadiation | null {
  const timestamp = parseExplicitTimestamp(at);
  // reject invalid coordinates and ambiguous wall clocks
  if (
    timestamp === null ||
    !Number.isFinite(latitude) || latitude < -90 || latitude > 90 ||
    !Number.isFinite(longitude) || longitude < -180 || longitude > 180
  ) {
    return null;
  }

  // equations follow NOAA's solar calculation details
  // https://gml.noaa.gov/grad/solcalc/calcdetails.html
  const julianDay = timestamp / DAY_MS + 2_440_587.5;
  const julianCentury = (julianDay - 2_451_545) / 36_525;
  const geometricMeanLongitude = normalizeDegrees(
    280.46646 + julianCentury * (36_000.76983 + julianCentury * 0.0003032),
  );
  const geometricMeanAnomaly = 357.52911 + julianCentury * (35_999.05029 - 0.0001537 * julianCentury);
  const orbitalEccentricity = 0.016708634 - julianCentury * (0.000042037 + 0.0000001267 * julianCentury);
  const anomalyRadians = degreesToRadians(geometricMeanAnomaly);
  const equationOfCenter =
    Math.sin(anomalyRadians) * (1.914602 - julianCentury * (0.004817 + 0.000014 * julianCentury)) +
    Math.sin(2 * anomalyRadians) * (0.019993 - 0.000101 * julianCentury) +
    Math.sin(3 * anomalyRadians) * 0.000289;
  const trueLongitude = geometricMeanLongitude + equationOfCenter;
  const apparentLongitude = trueLongitude - 0.00569 - 0.00478 * Math.sin(
    degreesToRadians(125.04 - 1934.136 * julianCentury),
  );
  const meanObliquity = 23 + (
    26 + (21.448 - julianCentury * (46.815 + julianCentury * (0.00059 - julianCentury * 0.001813))) / 60
  ) / 60;
  const correctedObliquity = meanObliquity + 0.00256 * Math.cos(
    degreesToRadians(125.04 - 1934.136 * julianCentury),
  );
  const obliquityRadians = degreesToRadians(correctedObliquity);
  const apparentLongitudeRadians = degreesToRadians(apparentLongitude);
  const solarDeclination = Math.asin(
    Math.sin(obliquityRadians) * Math.sin(apparentLongitudeRadians),
  );
  const equationVariable = Math.tan(obliquityRadians / 2) ** 2;
  const longitudeRadians = degreesToRadians(geometricMeanLongitude);
  const equationOfTime = 4 * radiansToDegrees(
    equationVariable * Math.sin(2 * longitudeRadians) -
    2 * orbitalEccentricity * Math.sin(anomalyRadians) +
    4 * orbitalEccentricity * equationVariable * Math.sin(anomalyRadians) * Math.cos(2 * longitudeRadians) -
    0.5 * equationVariable ** 2 * Math.sin(4 * longitudeRadians) -
    1.25 * orbitalEccentricity ** 2 * Math.sin(2 * anomalyRadians),
  );
  const instant = new Date(timestamp);
  const utcMinutes = instant.getUTCHours() * 60 + instant.getUTCMinutes() +
    instant.getUTCSeconds() / 60 + instant.getUTCMilliseconds() / 60_000;
  const trueSolarMinutes = normalizeMinutes(utcMinutes + equationOfTime + 4 * longitude);
  const hourAngle = degreesToRadians(trueSolarMinutes / 4 - 180);
  const latitudeRadians = degreesToRadians(latitude);
  const cosineZenith = clamp(
    Math.sin(latitudeRadians) * Math.sin(solarDeclination) +
      Math.cos(latitudeRadians) * Math.cos(solarDeclination) * Math.cos(hourAngle),
    -1,
    1,
  );
  const solarElevationDegrees = 90 - radiansToDegrees(Math.acos(cosineZenith));

  // use the Haurwitz clear-sky model only while the sun is above the horizon
  // https://github.com/pvlib/pvlib-python/blob/main/pvlib/clearsky.py
  if (cosineZenith <= 0) {
    return { irradianceWm2: 0, solarElevationDegrees };
  }

  return {
    irradianceWm2: 1098 * cosineZenith * Math.exp(-0.059 / cosineZenith),
    solarElevationDegrees,
  };
}

// share the fixed pointwise ws90 proxy with independent label construction
export function radiationCloudProxy(latitude: number, longitude: number, at: string, radiation: number | null): number | null {
  const sky = clearSkyRadiation(latitude, longitude, at);
  // retain every original physical daylight and enhancement gate
  if (sky === null || radiation === null || !Number.isFinite(radiation) || radiation <= 5 || radiation > 2500 ||
    sky.solarElevationDegrees < MIN_SOLAR_ELEVATION_DEGREES || sky.irradianceWm2 < MIN_CLEAR_SKY_RADIATION_WM2 ||
    radiation / sky.irradianceWm2 > MAX_RADIATION_RATIO) {
    return null;
  }
  const obscuration = clamp(
    (1 - Math.min(1, radiation / (CLEAR_SKY_TOLERANCE * sky.irradianceWm2))) / CLOUD_ATTENUATION_RANGE,
    0,
    1,
  );
  return 100 * Math.sqrt(obscuration);
}

// create one short-lived experimental cloud-cover bias from paired current readings
export function createSolarCloudBias(input: SolarCloudBiasInput): SolarCloudBias | null {
  const now = parseExplicitTimestamp(input.now);
  const ws90 = input.ws90;
  const regional = input.regional;
  // require both explicitly sourced readings and one valid evaluation time
  if (now === null || ws90 === null || regional === null) {
    return null;
  }

  const ws90ValidAt = parseExplicitTimestamp(ws90.validAt);
  const ws90ReceivedAt = parseExplicitTimestamp(ws90.receivedAt);
  const regionalValidAt = parseExplicitTimestamp(regional.validAt);
  const regionalReceivedAt = parseExplicitTimestamp(regional.receivedAt);
  // reject malformed provenance timestamps
  if (
    ws90ValidAt === null || ws90ReceivedAt === null ||
    regionalValidAt === null || regionalReceivedAt === null
  ) {
    return null;
  }

  // reject stale delayed future or causally invalid samples
  if (
    ws90.freshnessStatus !== "fresh" || regional.freshnessStatus !== "fresh" ||
    ws90ValidAt > now || ws90ReceivedAt > now || regionalValidAt > now || regionalReceivedAt > now ||
    ws90ReceivedAt < ws90ValidAt || regionalReceivedAt < regionalValidAt ||
    now - ws90ValidAt > MAX_WS90_AGE_MS || now - ws90ReceivedAt > MAX_WS90_AGE_MS ||
    now - regionalValidAt > MAX_REGIONAL_AGE_MS || now - regionalReceivedAt > MAX_REGIONAL_AGE_MS ||
    Math.abs(ws90ValidAt - regionalValidAt) > MAX_SAMPLE_PAIR_GAP_MS
  ) {
    return null;
  }

  // require complete identifiers and bounded physical inputs
  if (
    ws90.recordId.trim() === "" || ws90.sourceId.trim() === "" ||
    regional.recordId.trim() === "" || regional.sourceId.trim() === "" ||
    ws90.value === null || !Number.isFinite(ws90.value) || ws90.value <= 5 || ws90.value > 2500 ||
    regional.value === null || !Number.isFinite(regional.value) || regional.value < 0 || regional.value > 100
  ) {
    return null;
  }

  const observedSky = clearSkyRadiation(input.latitude, input.longitude, ws90.validAt);
  const evaluatedSky = clearSkyRadiation(input.latitude, input.longitude, input.now);
  // fail raw at night low sun or weak expected radiation
  if (
    observedSky === null || evaluatedSky === null ||
    observedSky.solarElevationDegrees < MIN_SOLAR_ELEVATION_DEGREES ||
    evaluatedSky.solarElevationDegrees < MIN_SOLAR_ELEVATION_DEGREES ||
    observedSky.irradianceWm2 < MIN_CLEAR_SKY_RADIATION_WM2 ||
    evaluatedSky.irradianceWm2 < MIN_CLEAR_SKY_RADIATION_WM2
  ) {
    return null;
  }

  const estimatedCloudCoverPercent = radiationCloudProxy(input.latitude, input.longitude, ws90.validAt, ws90.value);
  // the exact point proxy rejects suspicious radiation enhancement
  if (estimatedCloudCoverPercent === null) {
    return null;
  }
  const biasPercentPoints = clamp(
    estimatedCloudCoverPercent - regional.value,
    -MAX_BIAS_PERCENT_POINTS,
    MAX_BIAS_PERCENT_POINTS,
  );

  return {
    contractVersion: "solar-cloud-experiment/v1",
    observedAt: ws90.validAt,
    evaluatedAt: input.now,
    expiresAt: new Date(ws90ValidAt + DAY_MS).toISOString(),
    observationRecordId: ws90.recordId,
    observationSourceId: ws90.sourceId,
    regionalRecordId: regional.recordId,
    regionalSourceId: regional.sourceId,
    regionalValidAt: regional.validAt,
    observedRadiationWm2: ws90.value,
    clearSkyRadiationWm2: observedSky.irradianceWm2,
    solarElevationDegrees: observedSky.solarElevationDegrees,
    rawCloudCoverPercent: regional.value,
    estimatedCloudCoverPercent,
    biasPercentPoints,
  };
}

// fade one valid experimental bias to zero over its fixed observation horizon
export function projectSolarCloudCover(
  rawCover: number | null,
  targetAt: string,
  bias: SolarCloudBias | null,
): number | null {
  const times = solarProjectionTimes(rawCover, targetAt, bias);
  // rejected metadata preserves the original current projector behavior
  if (times === null || rawCover === null || bias === null) {
    return rawCover;
  }
  const { target, observed } = times;
  const weight = Math.max(0, 1 - (target - observed) / DAY_MS);
  return clamp(rawCover + bias.biasPercentPoints * weight, 0, 100);
}

// shorten only forecast persistence while retaining current cloud semantics
export function projectForecastCloudCover(
  rawCover: number | null,
  targetAt: string,
  bias: SolarCloudBias | null,
  policy: ForecastCorrectionPolicy,
): number | null {
  const times = solarProjectionTimes(rawCover, targetAt, bias);
  // no unvalidated or unavailable forecast value can be manufactured
  if (times === null || rawCover === null || bias === null) {
    return rawCover;
  }
  const weight = forecastCorrectionWeight(times.target, times.observed, policy);
  return weight === 0 ? rawCover : clamp(rawCover + bias.biasPercentPoints * weight, 0, 100);
}

// retain the legacy cloud projector guards without changing current outputs
function solarProjectionTimes(rawCover: number | null, targetAt: string, bias: SolarCloudBias | null):
  { readonly target: number; readonly observed: number } | null {
  // preserve unavailable or invalid normalized inputs
  if (rawCover === null || !Number.isFinite(rawCover) || rawCover < 0 || rawCover > 100 || bias === null) {
    return null;
  }

  const target = parseExplicitTimestamp(targetAt);
  const observed = parseExplicitTimestamp(bias.observedAt);
  const evaluated = parseExplicitTimestamp(bias.evaluatedAt);
  const expires = parseExplicitTimestamp(bias.expiresAt);
  // preserve raw outside a coherent forward projection window
  if (
    bias.contractVersion !== "solar-cloud-experiment/v1" ||
    target === null || observed === null || evaluated === null || expires === null ||
    expires !== observed + DAY_MS || evaluated < observed || evaluated >= expires ||
    target < evaluated || target >= expires ||
    !Number.isFinite(bias.biasPercentPoints) ||
    Math.abs(bias.biasPercentPoints) > MAX_BIAS_PERCENT_POINTS
  ) {
    return null;
  }

  return { target, observed };
}

// parse one valid ISO timestamp carrying an explicit offset
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
  // reject normalized overflow dates before Date parsing
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

// wrap degrees into one full revolution
function normalizeDegrees(value: number): number {
  return ((value % 360) + 360) % 360;
}

// wrap clock minutes into one solar day
function normalizeMinutes(value: number): number {
  return ((value % 1440) + 1440) % 1440;
}

// convert angular degrees to radians
function degreesToRadians(value: number): number {
  return value * Math.PI / 180;
}

// convert angular radians to degrees
function radiansToDegrees(value: number): number {
  return value * 180 / Math.PI;
}

// constrain one finite scalar inclusively
function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
