import { createHash } from "node:crypto";

import {
  createNormalizedWeatherRecord,
  normalizeMetricValue,
  validateTimeZone,
  type JsonValue,
  type NormalizedWeatherRecord,
} from "@weather/domain";

import {
  ProviderFailure,
  type CurrentProviderOperation,
  type ProviderAttribution,
  type ProviderBatch,
  type ProviderFetchOptions,
  type ProviderRequestPlan,
} from "./contract.js";
import { fetchJsonWithRetry } from "./http.js";

export const ECOWITT_ATTRIBUTION: ProviderAttribution = {
  label: "Weather data by Ecowitt",
  url: "https://www.ecowitt.com/",
};
export const ECOWITT_LOCAL_LIVE_ADAPTER_VERSION = "ecowitt-local-live/v1";

const ECOWITT_LIVE_PATH = "/get_livedata_info";
const ECOWITT_NETWORK_PATH = "/get_network_info";
const PRIVATE_IPV4 = /^(?:10\.(?:\d{1,3}\.){2}\d{1,3}|192\.168\.(?:\d{1,3}\.)\d{1,3}|172\.(?:1[6-9]|2\d|3[01])\.(?:\d{1,3}\.)\d{1,3})$/u;

export interface EcowittLiveRequest {
  readonly expectedMac: string;
  readonly gatewayHost: string;
  readonly model: string;
  readonly previousCursor: Readonly<Record<string, JsonValue>> | null;
  readonly sourceId: string;
  readonly timezone: string;
}

export type EcowittLiveOperation = CurrentProviderOperation<EcowittLiveRequest>;

interface ParsedMeasurement {
  readonly unit: string;
  readonly value: number;
}

interface RainSelection {
  readonly group: readonly unknown[];
  readonly kind: "piezo" | "traditional";
}

export interface EcowittSensorReadings {
  readonly batteryExternalPower?: number;
  readonly batteryLevel?: number;
  readonly batteryLow?: number;
  readonly blackGlobeTemperatureC?: number;
  readonly dailyPrecipitationMm?: number;
  readonly pm25MicrogramsPerCubicMeter?: number;
  readonly precipitationRateMmPerHour?: number;
  readonly pressureHpa?: number;
  readonly relativeHumidityPercent?: number;
  readonly soilElectricalConductivityMicrosiemensPerCm?: number;
  readonly soilMoisturePercent?: number;
  readonly solarRadiationWm2?: number;
  readonly temperatureC?: number;
  readonly uvIndex?: number;
  readonly wetBulbGlobeTemperatureC?: number;
  readonly windDirectionDegrees?: number;
  readonly windGustMps?: number;
  readonly windSpeedMps?: number;
}

export interface EcowittSensorSnapshot {
  readonly channel: number | null;
  readonly key: string;
  readonly model: string;
  readonly readings: EcowittSensorReadings;
}

// build the LAN-only live-data request
export function buildEcowittLiveRequest(
  input: EcowittLiveRequest,
): ProviderRequestPlan {
  const validated = validateEcowittRequest(input);

  return {
    adapterVersion: ECOWITT_LOCAL_LIVE_ADAPTER_VERSION,
    capability: "current",
    sourceKind: "physical_sensor",
    url: new URL(ECOWITT_LIVE_PATH, `http://${validated.gatewayHost}`),
  };
}

// fetch and normalize one local gateway snapshot
export async function fetchEcowittLive(
  input: EcowittLiveRequest,
  options: ProviderFetchOptions = {},
): Promise<ProviderBatch> {
  const validated = validateEcowittRequest(input);
  const plan = buildEcowittLiveRequest(validated);
  const identityUrl = new URL(
    ECOWITT_NETWORK_PATH,
    `http://${validated.gatewayHost}`,
  );
  const identity = await fetchJsonWithRetry(identityUrl, {
    maxBodyBytes: 32_768,
    timeoutMs: 5_000,
    ...options,
  });
  requireGatewayIdentity(identity.payload, validated.expectedMac);
  const response = await fetchJsonWithRetry(plan.url, {
    maxBodyBytes: 262_144,
    timeoutMs: 5_000,
    ...options,
  });
  const receivedAt = (options.now ?? defaultNow)().toISOString();
  const record = normalizeEcowittLivePayload(
    response.payload,
    validated,
    receivedAt,
  );
  const rain = selectRainGroup(requireObject(response.payload, "Ecowitt live response"));
  const dailyTotalMm = rain === null
    ? null
    : rainAccumulation(rain.group, "0x10", "daily rain");
  const localDay = siteDate(receivedAt, validated.timezone);

  return {
    attempts: identity.attempts + response.attempts,
    checksum: createHash("sha256")
      .update(identity.checksum)
      .update(response.checksum)
      .digest("hex"),
    providerCursor: {
      ...(dailyTotalMm === null ? {} : { rain_daily_total_mm: dailyTotalMm }),
      rain_day: localDay,
      valid_at: record.validAt,
    },
    records: [record],
    responseMetadata: ecowittResponseMetadata(
      response.payload,
      validated.expectedMac,
      rain?.kind ?? null,
    ),
  };
}

// normalize the gateway's canonical outdoor sensor set
export function normalizeEcowittLivePayload(
  payload: unknown,
  input: EcowittLiveRequest,
  receivedAt: string,
): NormalizedWeatherRecord {
  const validated = validateEcowittRequest(input);
  const root = requireObject(payload, "Ecowitt live response");
  const common = requireArray(root.common_list, "common_list");
  const indoor = optionalArray(root.wh25, "wh25");
  const soil = optionalArray(root.ch_ec, "ch_ec");
  const particulate = optionalArray(root.ch_pm25, "ch_pm25");
  const rain = selectRainGroup(root);
  const dailyTotalMm = rain === null
    ? null
    : rainAccumulation(rain.group, "0x10", "daily rain");
  const localDay = siteDate(receivedAt, validated.timezone);
  const precipitationMm = rainDelta(
    dailyTotalMm,
    localDay,
    validated.previousCursor,
  );
  const relativePressure = indoor.length === 0
    ? null
    : measurementFromField(indoor[0], "rel", "relative pressure");
  const primarySoil = findChannel(soil, "1");
  const primaryParticulate = findChannel(particulate, "1");

  return createNormalizedWeatherRecord({
    metadata: {
      device: {
        model: validated.model,
        serial: validated.expectedMac,
        vendor: "Ecowitt",
      },
      model: null,
      provider: {
        dataset: "get_livedata_info",
        device_id: validated.expectedMac,
        property_sensors: normalizeEcowittSensorSnapshots(payload) as unknown as JsonValue,
      },
      quality: {
        sampling: "local_gateway_poll",
      },
      upstreamTimezone: validated.timezone,
    },
    metrics: {
      apparentTemperatureC: temperatureMetric(
        findCommon(common, "4") ?? findCommon(common, "3"),
        "apparent temperature",
        "apparentTemperatureC",
      ),
      blackGlobeTemperatureC: temperatureMetric(
        findCommon(common, "0xA1"),
        "black globe temperature",
        "blackGlobeTemperatureC",
      ),
      cloudCoverPercent: null,
      pm25MicrogramsPerCubicMeter: particulateMetric(primaryParticulate),
      precipitationMm,
      precipitationRateMmPerHour: rain === null
        ? null
        : rainRate(rain.group, "0x0E", "rain rate"),
      pressureHpa: pressureMetric(relativePressure),
      relativeHumidityPercent: percentMetric(
        measurementFromEntry(findCommon(common, "0x07"), "outdoor humidity"),
        "relativeHumidityPercent",
      ),
      soilElectricalConductivityMicrosiemensPerCm: soilEcMetric(primarySoil),
      soilMoisturePercent: percentMetric(
        measurementFromField(primarySoil, "humidity", "soil moisture"),
        "soilMoisturePercent",
      ),
      solarRadiationWm2: solarMetric(
        measurementFromEntry(findCommon(common, "0x15"), "solar radiation"),
      ),
      temperatureC: temperatureMetric(
        findCommon(common, "0x02"),
        "outdoor temperature",
        "temperatureC",
      ),
      uvIndex: unitlessMetric(
        measurementFromEntry(findCommon(common, "0x17"), "UV index"),
        "uvIndex",
      ),
      waterLevelM: null,
      wetBulbGlobeTemperatureC: temperatureMetric(
        findCommon(common, "0xA2"),
        "wet bulb globe temperature",
        "wetBulbGlobeTemperatureC",
      ),
      windDirectionDegrees: directionMetric(
        measurementFromEntry(findCommon(common, "0x0A"), "wind direction"),
      ),
      windGustMps: windMetric(
        measurementFromEntry(findCommon(common, "0x0C"), "wind gust"),
        "windGustMps",
      ),
      windSpeedMps: windMetric(
        measurementFromEntry(findCommon(common, "0x0B"), "wind speed"),
        "windSpeedMps",
      ),
    },
    productRunAt: null,
    receivedAt,
    sourceId: validated.sourceId,
    sourceKind: "physical_sensor",
    validAt: receivedAt,
  });
}

// normalize every property sensor group without merging repeated measurements
export function normalizeEcowittSensorSnapshots(
  payload: unknown,
): readonly EcowittSensorSnapshot[] {
  const root = requireObject(payload, "Ecowitt live response");
  const common = requireArray(root.common_list, "common_list");
  const indoor = optionalArray(root.wh25, "wh25");
  const rain = selectRainGroup(root);
  const piezoRain = optionalArray(root.piezoRain, "piezoRain");
  const traditionalRain = optionalArray(root.rain, "rain");
  const sensors: EcowittSensorSnapshot[] = [];
  const gateway = indoor[0];

  // retain gateway environmental readings
  if (gateway !== undefined) {
    sensors.push({
      channel: null,
      key: "gateway",
      model: "GW3000",
      readings: compactSensorReadings({
        pressureHpa: pressureMetric(
          measurementFromField(gateway, "rel", "relative pressure"),
        ),
        relativeHumidityPercent: percentMetric(
          measurementFromField(gateway, "inhumi", "gateway humidity"),
          "relativeHumidityPercent",
        ),
        temperatureC: temperatureFieldMetric(
          gateway,
          "intemp",
          "gateway temperature",
          "temperatureC",
        ),
      }),
    });
  }

  const ws90Readings: Record<keyof EcowittSensorReadings, number | null> = {
    batteryExternalPower: null,
    batteryLevel: optionalBatteryValue(findById(piezoRain, "0x13"), 5),
    batteryLow: null,
    blackGlobeTemperatureC: null,
    dailyPrecipitationMm: rain?.kind === "piezo"
      ? rainAccumulation(rain.group, "0x10", "daily rain")
      : null,
    pm25MicrogramsPerCubicMeter: null,
    precipitationRateMmPerHour: rain?.kind === "piezo"
      ? rainRate(rain.group, "0x0E", "rain rate")
      : null,
    pressureHpa: null,
    relativeHumidityPercent: percentMetric(
      measurementFromEntry(findCommon(common, "0x07"), "outdoor humidity"),
      "relativeHumidityPercent",
    ),
    soilElectricalConductivityMicrosiemensPerCm: null,
    soilMoisturePercent: null,
    solarRadiationWm2: solarMetric(
      measurementFromEntry(findCommon(common, "0x15"), "solar radiation"),
    ),
    temperatureC: temperatureMetric(
      findCommon(common, "0x02"),
      "outdoor temperature",
      "temperatureC",
    ),
    uvIndex: unitlessMetric(
      measurementFromEntry(findCommon(common, "0x17"), "UV index"),
      "uvIndex",
    ),
    wetBulbGlobeTemperatureC: null,
    windDirectionDegrees: directionMetric(
      measurementFromEntry(findCommon(common, "0x0A"), "wind direction"),
    ),
    windGustMps: windMetric(
      measurementFromEntry(findCommon(common, "0x0C"), "wind gust"),
      "windGustMps",
    ),
    windSpeedMps: windMetric(
      measurementFromEntry(findCommon(common, "0x0B"), "wind speed"),
      "windSpeedMps",
    ),
  };
  sensors.push({
    channel: null,
    key: "weather-array",
    model: "WS90",
    readings: compactSensorReadings(ws90Readings),
  });

  const globeReadings = compactSensorReadings({
    batteryLevel: optionalBatteryValue(findCommon(common, "0xA1"), 5),
    blackGlobeTemperatureC: temperatureMetric(
      findCommon(common, "0xA1"),
      "black globe temperature",
      "blackGlobeTemperatureC",
    ),
    wetBulbGlobeTemperatureC: temperatureMetric(
      findCommon(common, "0xA2"),
      "wet bulb globe temperature",
      "wetBulbGlobeTemperatureC",
    ),
  });

  // retain the installed heat-stress sensor when it is reporting
  if (Object.keys(globeReadings).length > 0) {
    sensors.push({
      channel: null,
      key: "black-globe",
      model: "WN38",
      readings: globeReadings,
    });
  }

  // retain the dedicated tipping-bucket gauge separately
  if (rain?.kind === "traditional") {
    sensors.push({
      channel: null,
      key: "rain-gauge",
      model: "WH40H",
      readings: compactSensorReadings({
        batteryLevel: optionalBatteryValue(
          findById(traditionalRain, "0x13"),
          5,
        ),
        dailyPrecipitationMm: rainAccumulation(
          rain.group,
          "0x10",
          "daily rain",
        ),
        precipitationRateMmPerHour: rainRate(
          rain.group,
          "0x0E",
          "rain rate",
        ),
      }),
    });
  }

  // retain every multi-channel temperature sensor
  for (const entry of optionalArray(root.ch_aisle, "ch_aisle")) {
    const channel = sensorChannel(entry, "temperature sensor");
    sensors.push({
      channel,
      key: `temperature-${String(channel)}`,
      model: channel === 5 ? "WN30" : "WN31",
      readings: compactSensorReadings({
        batteryLow: optionalBatteryValue(entry, 1),
        relativeHumidityPercent: percentMetric(
          measurementFromField(entry, "humidity", "channel humidity"),
          "relativeHumidityPercent",
        ),
        temperatureC: temperatureFieldMetric(
          entry,
          "temp",
          "channel temperature",
          "temperatureC",
        ),
      }),
    });
  }

  // retain every soil channel as an independent property sensor
  for (const entry of optionalArray(root.ch_ec, "ch_ec")) {
    const channel = sensorChannel(entry, "soil sensor");
    sensors.push({
      channel,
      key: `soil-${String(channel)}`,
      model: "WH52",
      readings: compactSensorReadings({
        batteryLevel: optionalBatteryValue(entry, 5),
        soilElectricalConductivityMicrosiemensPerCm: soilEcMetric(entry),
        soilMoisturePercent: percentMetric(
          measurementFromField(entry, "humidity", "soil moisture"),
          "soilMoisturePercent",
        ),
        temperatureC: temperatureFieldMetric(
          entry,
          "temp",
          "soil temperature",
          "temperatureC",
        ),
      }),
    });
  }

  // retain every particulate channel when installed
  for (const entry of optionalArray(root.ch_pm25, "ch_pm25")) {
    const channel = sensorChannel(entry, "air quality sensor");
    sensors.push({
      channel,
      key: `air-quality-${String(channel)}`,
      model: "WH41",
      readings: compactSensorReadings({
        batteryExternalPower: optionalBatteryExternalPower(entry),
        batteryLevel: optionalBatteryValue(entry, 5),
        pm25MicrogramsPerCubicMeter: particulateMetric(entry),
      }),
    });
  }

  return sensors;
}

// require the configured gateway at the target IP
function requireGatewayIdentity(payload: unknown, expectedMac: string): void {
  const root = requireObject(payload, "Ecowitt network response");
  const actualMac = requireString(root.mac, "network.mac", 17).toUpperCase();

  // reject address reuse by a different gateway
  if (actualMac !== expectedMac) {
    throw invalidPayload("Ecowitt gateway MAC does not match configuration");
  }
}

// validate one immutable local gateway request
function validateEcowittRequest(input: EcowittLiveRequest): EcowittLiveRequest {
  const gatewayHost = requireString(input.gatewayHost, "gatewayHost", 64);

  // constrain polling to a literal private LAN address
  if (!PRIVATE_IPV4.test(gatewayHost) || !validIpv4Octets(gatewayHost)) {
    throw new RangeError("Ecowitt gatewayHost must be a private IPv4 address");
  }

  const expectedMac = requireString(input.expectedMac, "expectedMac", 17).toUpperCase();

  // require one canonical hardware identity
  if (!/^(?:[0-9A-F]{2}:){5}[0-9A-F]{2}$/u.test(expectedMac)) {
    throw new RangeError("Ecowitt expectedMac must use canonical colon notation");
  }

  return {
    expectedMac,
    gatewayHost,
    model: requireString(input.model, "model", 128),
    previousCursor: input.previousCursor,
    sourceId: requireString(input.sourceId, "sourceId", 128),
    timezone: validateTimeZone(input.timezone),
  };
}

// reject out-of-range dotted-quad components
function validIpv4Octets(value: string): boolean {
  return value.split(".").every(
    // require decimal octets only
    (part) => /^\d{1,3}$/u.test(part) && Number(part) <= 255,
  );
}

// prefer the dedicated tipping-bucket gauge when installed
function selectRainGroup(root: Record<string, unknown>): RainSelection | null {
  const traditional = optionalArray(root.rain, "rain");

  // select a populated WH40 group first
  if (traditional.length > 0) {
    return { group: traditional, kind: "traditional" };
  }

  const piezo = optionalArray(root.piezoRain, "piezoRain");
  return piezo.length === 0 ? null : { group: piezo, kind: "piezo" };
}

// derive one interval accumulation from the durable daily counter cursor
function rainDelta(
  currentTotalMm: number | null,
  localDay: string,
  cursor: Readonly<Record<string, JsonValue>> | null,
): number | null {
  // preserve missing gauge totals
  if (currentTotalMm === null) {
    return null;
  }

  const previousTotal = cursor?.rain_daily_total_mm;
  const previousDay = cursor?.rain_day;

  // seed the first interval with today's gateway total
  if (typeof previousTotal !== "number" || typeof previousDay !== "string") {
    return normalizeMetricValue("precipitationMm", currentTotalMm, "millimeter");
  }

  // seed the first interval of a new local day
  if (previousDay !== localDay) {
    return normalizeMetricValue("precipitationMm", currentTotalMm, "millimeter");
  }

  // avoid double counting after an unexpected same-day counter reset
  if (currentTotalMm < previousTotal) {
    return null;
  }

  return normalizeMetricValue(
    "precipitationMm",
    currentTotalMm - previousTotal,
    "millimeter",
  );
}

// parse one rain accumulation field
function rainAccumulation(
  group: readonly unknown[],
  id: string,
  field: string,
): number | null {
  const measurement = measurementFromEntry(findById(group, id), field);

  // preserve missing rain counters
  if (measurement === null) {
    return null;
  }

  const unit = normalizedUnit(measurement.unit);

  // normalize metric rain totals
  if (unit === "mm") {
    return normalizeMetricValue("precipitationMm", measurement.value, "millimeter");
  }

  // normalize imperial rain totals
  if (unit === "in") {
    return normalizeMetricValue("precipitationMm", measurement.value, "inch");
  }

  throw invalidPayload(`${field} has an unsupported unit`);
}

// parse one rain-rate field
function rainRate(
  group: readonly unknown[],
  id: string,
  field: string,
): number | null {
  const measurement = measurementFromEntry(findById(group, id), field);

  // preserve missing rain rates
  if (measurement === null) {
    return null;
  }

  const unit = normalizedUnit(measurement.unit);

  // normalize metric rates
  if (unit === "mm/hr") {
    return normalizeMetricValue(
      "precipitationRateMmPerHour",
      measurement.value,
      "millimeter_per_hour",
    );
  }

  // normalize imperial rates
  if (unit === "in/hr") {
    return normalizeMetricValue(
      "precipitationRateMmPerHour",
      measurement.value,
      "inch_per_hour",
    );
  }

  throw invalidPayload(`${field} has an unsupported unit`);
}

// normalize one temperature entry
function temperatureMetric(
  entry: unknown,
  field: string,
  metric:
    | "apparentTemperatureC"
    | "blackGlobeTemperatureC"
    | "temperatureC"
    | "wetBulbGlobeTemperatureC",
): number | null {
  const measurement = measurementFromEntry(entry, field);

  return temperatureMeasurementMetric(measurement, field, metric);
}

// normalize one nested temperature field
function temperatureFieldMetric(
  entry: unknown,
  key: string,
  field: string,
  metric:
    | "apparentTemperatureC"
    | "blackGlobeTemperatureC"
    | "temperatureC"
    | "wetBulbGlobeTemperatureC",
): number | null {
  const measurement = measurementFromField(entry, key, field);

  return temperatureMeasurementMetric(measurement, field, metric);
}

// normalize one parsed temperature measurement
function temperatureMeasurementMetric(
  measurement: ParsedMeasurement | null,
  field: string,
  metric:
    | "apparentTemperatureC"
    | "blackGlobeTemperatureC"
    | "temperatureC"
    | "wetBulbGlobeTemperatureC",
): number | null {

  // preserve absent sensor readings
  if (measurement === null) {
    return null;
  }

  const unit = normalizedUnit(measurement.unit);

  // normalize Celsius values
  if (unit === "c") {
    return normalizeMetricValue(metric, measurement.value, "c");
  }

  // normalize Fahrenheit values
  if (unit === "f") {
    return normalizeMetricValue(metric, measurement.value, "f");
  }

  throw invalidPayload(`${field} has an unsupported unit`);
}

// omit unavailable readings from one compact sensor snapshot
function compactSensorReadings(
  input: Readonly<
    Partial<Record<keyof EcowittSensorReadings, number | null>>
  >,
): EcowittSensorReadings {
  const output: Partial<Record<keyof EcowittSensorReadings, number>> = {};

  // retain only live finite values
  for (const [key, value] of Object.entries(input) as Array<
    [keyof EcowittSensorReadings, number | null | undefined]
  >) {
    // omit missing readings
    if (value !== null && value !== undefined && Number.isFinite(value)) {
      output[key] = value;
    }
  }

  return output;
}

// normalize relative pressure
function pressureMetric(measurement: ParsedMeasurement | null): number | null {
  // preserve absent pressure readings
  if (measurement === null) {
    return null;
  }

  const unit = normalizedUnit(measurement.unit);

  // normalize hectopascals
  if (unit === "hpa") {
    return normalizeMetricValue("pressureHpa", measurement.value, "hectopascal");
  }

  // normalize kilopascals
  if (unit === "kpa") {
    return normalizeMetricValue("pressureHpa", measurement.value * 1_000, "pascal");
  }

  // normalize inches of mercury
  if (unit === "inhg") {
    return normalizeMetricValue("pressureHpa", measurement.value, "inch_of_mercury");
  }

  throw invalidPayload("relative pressure has an unsupported unit");
}

// normalize one percentage reading
function percentMetric(
  measurement: ParsedMeasurement | null,
  metric: "relativeHumidityPercent" | "soilMoisturePercent",
): number | null {
  // preserve absent percentage readings
  if (measurement === null) {
    return null;
  }

  // require the gateway's explicit percent unit
  if (normalizedUnit(measurement.unit) !== "%") {
    throw invalidPayload(`${metric} has an unsupported unit`);
  }

  return normalizeMetricValue(metric, measurement.value, "percent");
}

// normalize one wind reading
function windMetric(
  measurement: ParsedMeasurement | null,
  metric: "windGustMps" | "windSpeedMps",
): number | null {
  // preserve absent wind readings
  if (measurement === null) {
    return null;
  }

  const unit = normalizedUnit(measurement.unit);

  // normalize metric wind
  if (unit === "m/s") {
    return normalizeMetricValue(metric, measurement.value, "meter_per_second");
  }

  // normalize consumer imperial wind
  if (unit === "mph") {
    return normalizeMetricValue(metric, measurement.value, "mile_per_hour");
  }

  // normalize alternate metric wind
  if (unit === "km/h") {
    return normalizeMetricValue(metric, measurement.value, "kilometer_per_hour");
  }

  // normalize nautical wind
  if (unit === "kn" || unit === "knot" || unit === "knots") {
    return normalizeMetricValue(metric, measurement.value, "knot");
  }

  throw invalidPayload(`${metric} has an unsupported unit`);
}

// normalize one direction reading
function directionMetric(measurement: ParsedMeasurement | null): number | null {
  // preserve absent directions
  if (measurement === null) {
    return null;
  }

  // require unitless degrees from the gateway
  if (measurement.unit.length > 0) {
    throw invalidPayload("wind direction has an unsupported unit");
  }

  return normalizeMetricValue(
    "windDirectionDegrees",
    measurement.value === 360 ? 0 : measurement.value,
    "degree",
  );
}

// normalize one solar-radiation reading
function solarMetric(measurement: ParsedMeasurement | null): number | null {
  // preserve absent radiation
  if (measurement === null) {
    return null;
  }

  // require watts per square meter
  if (normalizedUnit(measurement.unit) !== "w/m2") {
    throw invalidPayload("solar radiation has an unsupported unit");
  }

  return normalizeMetricValue(
    "solarRadiationWm2",
    measurement.value,
    "watt_per_square_meter",
  );
}

// normalize one unitless metric
function unitlessMetric(
  measurement: ParsedMeasurement | null,
  metric: "uvIndex",
): number | null {
  // preserve absent unitless readings
  if (measurement === null) {
    return null;
  }

  // require an actually unitless value
  if (measurement.unit.length > 0) {
    throw invalidPayload(`${metric} has an unsupported unit`);
  }

  return normalizeMetricValue(metric, measurement.value, "index");
}

// normalize the first configured soil EC channel
function soilEcMetric(entry: unknown): number | null {
  const measurement = measurementFromField(entry, "ec", "soil EC");

  // preserve absent conductivity
  if (measurement === null) {
    return null;
  }

  // require microsiemens per centimeter
  if (normalizedUnit(measurement.unit) !== "us/cm") {
    throw invalidPayload("soil EC has an unsupported unit");
  }

  return normalizeMetricValue(
    "soilElectricalConductivityMicrosiemensPerCm",
    measurement.value,
    "microsiemens_per_centimeter",
  );
}

// normalize the first configured PM2.5 channel
function particulateMetric(entry: unknown): number | null {
  const measurement = measurementFromField(entry, "PM25", "PM2.5");

  // preserve absent particulate readings
  if (measurement === null) {
    return null;
  }

  // accept the gateway's implicit concentration unit
  if (measurement.unit.length > 0 && normalizedUnit(measurement.unit) !== "ug/m3") {
    throw invalidPayload("PM2.5 has an unsupported unit");
  }

  return normalizeMetricValue(
    "pm25MicrogramsPerCubicMeter",
    measurement.value,
    "microgram_per_cubic_meter",
  );
}

// read a common-list entry by stable field ID
function findCommon(group: readonly unknown[], id: string): unknown {
  return findById(group, id);
}

// read any ID-keyed entry
function findById(group: readonly unknown[], id: string): unknown {
  return group.find(
    // retain one matching object only
    (candidate) => isObject(candidate) && candidate.id === id,
  );
}

// read one numbered sensor channel
function findChannel(group: readonly unknown[], channel: string): unknown {
  return group.find(
    // retain one matching channel only
    (candidate) => isObject(candidate) && candidate.channel === channel,
  );
}

// require one bounded numeric channel identity
function sensorChannel(entry: unknown, field: string): number {
  const object = requireObject(entry, field);
  const raw = object.channel;
  const channel = typeof raw === "string" && /^\d{1,2}$/u.test(raw)
    ? Number(raw)
    : raw;

  // reject missing or implausible channel identities
  if (!Number.isSafeInteger(channel) || Number(channel) < 1 || Number(channel) > 32) {
    throw invalidPayload(`${field} channel must be between 1 and 32`);
  }

  return Number(channel);
}

// parse one optional bounded battery state
function optionalBatteryValue(entry: unknown, maximum: 1 | 5): number | null {
  const object = isObject(entry) ? entry : null;
  const raw = object?.battery;
  const value = typeof raw === "string" && /^[0-9]$/u.test(raw)
    ? Number(raw)
    : raw;

  // omit missing or unrecognized provider states
  if (!Number.isSafeInteger(value) || Number(value) < 0 || Number(value) > maximum) {
    return null;
  }

  return Number(value);
}

// detect one externally powered battery sentinel
function optionalBatteryExternalPower(entry: unknown): number | null {
  const object = isObject(entry) ? entry : null;
  const raw = object?.battery;

  // retain only the WH41 external-power sentinel
  if (raw !== 6 && raw !== "6") {
    return null;
  }

  return 1;
}

// parse one standard ID entry
function measurementFromEntry(
  entry: unknown,
  field: string,
): ParsedMeasurement | null {
  return measurementFromField(entry, "val", field);
}

// parse one provider measurement field
function measurementFromField(
  entry: unknown,
  key: string,
  field: string,
): ParsedMeasurement | null {
  // preserve absent sensor entries
  if (entry === undefined || entry === null) {
    return null;
  }

  const object = requireObject(entry, field);
  const raw = object[key];

  // preserve provider no-data sentinels
  if (
    raw === undefined ||
    raw === null ||
    (typeof raw === "string" && /^(?:none|-{2,}|--\.-)$/iu.test(raw.trim()))
  ) {
    return null;
  }

  // retain raw numeric readings
  if (typeof raw === "number" && Number.isFinite(raw)) {
    return {
      unit: typeof object.unit === "string" ? object.unit.trim() : "",
      value: raw,
    };
  }

  // require a number followed by an optional unit
  if (typeof raw !== "string") {
    throw invalidPayload(`${field} must be a numeric measurement`);
  }

  const match = /^([+-]?(?:\d+(?:\.\d+)?|\.\d+))\s*(.*)$/u.exec(raw.trim());

  // reject malformed measurement strings
  if (match === null || match[1] === undefined) {
    throw invalidPayload(`${field} must be a numeric measurement`);
  }

  const value = Number(match[1]);
  const inlineUnit = match[2]?.trim() ?? "";
  const explicitUnit = typeof object.unit === "string" ? object.unit.trim() : "";

  // reject non-finite parsed values
  if (!Number.isFinite(value)) {
    throw invalidPayload(`${field} must be finite`);
  }

  return { unit: inlineUnit || explicitUnit, value };
}

// expose bounded gateway diagnostics
function ecowittResponseMetadata(
  payload: unknown,
  expectedMac: string,
  rainGauge: RainSelection["kind"] | null,
): Readonly<Record<string, JsonValue>> {
  const root = requireObject(payload, "Ecowitt live response");

  return {
    gateway_mac: expectedMac,
    group_count: Object.keys(root).filter(
      // count array-bearing live-data groups only
      (key) => Array.isArray(root[key]),
    ).length,
    rain_gauge: rainGauge,
    soil_channel_count: optionalArray(root.ch_ec, "ch_ec").length,
    temperature_humidity_channel_count: optionalArray(root.ch_aisle, "ch_aisle").length,
  };
}

// derive one stable site-local calendar date
function siteDate(instant: string, timezone: string): string {
  const parts = new Intl.DateTimeFormat("en-US", {
    day: "2-digit",
    month: "2-digit",
    timeZone: timezone,
    year: "numeric",
  }).formatToParts(new Date(instant));
  const values = new Map(
    parts.map(
      // index each calendar field
      (part) => [part.type, part.value],
    ),
  );
  return `${values.get("year")}-${values.get("month")}-${values.get("day")}`;
}

// normalize provider unit spellings
function normalizedUnit(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/µ/gu, "u")
    .replace(/²/gu, "2")
    .replace(/\s+/gu, "");
}

// read an optional array field
function optionalArray(value: unknown, field: string): readonly unknown[] {
  // preserve absent optional groups
  if (value === undefined) {
    return [];
  }

  return requireArray(value, field);
}

// require one array
function requireArray(value: unknown, field: string): readonly unknown[] {
  // reject malformed groups
  if (!Array.isArray(value)) {
    throw invalidPayload(`${field} must be an array`);
  }

  return value;
}

// test plain object membership
function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

// require one object
function requireObject(value: unknown, field: string): Record<string, unknown> {
  // reject arrays and nulls
  if (!isObject(value)) {
    throw invalidPayload(`${field} must be an object`);
  }

  return value;
}

// require one bounded string
function requireString(value: unknown, field: string, maximum: number): string {
  // reject missing or oversized strings
  if (typeof value !== "string" || value.trim().length === 0 || value.length > maximum) {
    throw new RangeError(`${field} must be a non-empty bounded string`);
  }

  return value;
}

// classify local gateway schema failures
function invalidPayload(message: string): ProviderFailure {
  return new ProviderFailure({
    classification: "invalid_payload",
    code: "ecowitt_invalid_payload",
    message: message.slice(0, 512),
  });
}

// provide a testable clock default
function defaultNow(): Date {
  return new Date();
}
