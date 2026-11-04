export type PrecipitationUnit = "inches" | "millimeters";
export type PressureUnit = "atmosphere_percent" | "hectopascals" | "inches_of_mercury";
export type TemperatureUnit = "celsius" | "fahrenheit";
export type WaterLevelUnit = "feet" | "meters";
export type WindSpeedUnit = "kilometers_per_hour" | "meters_per_second" | "miles_per_hour";

export interface UnitPreferences {
  readonly precipitation: PrecipitationUnit;
  readonly pressure: PressureUnit;
  readonly temperature: TemperatureUnit;
  readonly waterLevel: WaterLevelUnit;
  readonly windSpeed: WindSpeedUnit;
}

export interface FormattedMeasurement {
  readonly unit: string;
  readonly value: string;
}

// retain hourly concentrations rather than a vendor-supplied index
export interface AirQualityHourlyContext {
  readonly hourlyPm25: readonly (number | null)[];
  readonly through: string;
}

const PM25_AQI_BREAKPOINTS = [
  [0, 9, 0, 50],
  [9.1, 35.4, 51, 100],
  [35.5, 55.4, 101, 150],
  [55.5, 125.4, 151, 200],
  [125.5, 225.4, 201, 300],
  [225.5, 325.4, 301, 500],
] as const;

// convert an averaged concentration using the EPA's 2024 PM2.5 breakpoints
export function pm25ToAqi(value: number | null | undefined): number | null {
  // reject missing and invalid concentrations without inventing clean air
  if (value === null || value === undefined || !Number.isFinite(value) || value < 0) {
    return null;
  }
  // remove binary arithmetic noise without rounding meaningful decimal places
  const concentration = Math.floor((value + Number.EPSILON * Math.max(1, value) * 4) * 10) / 10;
  const [low, high, indexLow, indexHigh] = PM25_AQI_BREAKPOINTS.find(
    // use the last segment beyond 500 rather than clamping hazardous readings
    ([, maximum]) => concentration <= maximum,
  ) ?? PM25_AQI_BREAKPOINTS[5];
  return Math.round(indexLow + (indexHigh - indexLow) * (concentration - low) / (high - low));
}

// calculate the EPA 12-hour PM NowCast before converting its concentration to AQI
export function pm25NowCastAqi(context: AirQualityHourlyContext | null | undefined, now = Date.now()): number | null {
  // fail closed when the hourly history has not been supplied
  if (context === null || context === undefined || !Array.isArray(context.hourlyPm25) || typeof context.through !== "string") {
    return null;
  }
  const hour = 3_600_000;
  const expected = Math.floor(now / hour) * hour - hour;
  const through = Date.parse(context.through);
  const shift = (expected - through) / hour;
  // reject future nonhourly and expired contexts
  if (!Number.isInteger(shift) || shift < 0 || shift > 11) {
    return null;
  }
  const values = Array.from({ length: 12 },
    // preserve missing hours and their wall-clock ages
    (_, age) => age < shift ? null : context.hourlyPm25[age - shift] ?? null,
  ).map(
    // discard invalid history without shifting older samples forward
    (value) => value !== null && Number.isFinite(value) && value >= 0 ? value : null,
  );
  // require two actual observations in the latest three completed hours
  if (values.slice(0, 3).filter((value) => value !== null).length < 2) {
    return null;
  }
  const valid = values.filter((value): value is number => value !== null);
  const maximum = Math.max(...valid);
  const weight = maximum === 0 ? 1 : Math.max(0.5, Math.min(...valid) / maximum);
  let numerator = 0;
  let denominator = 0;
  // omit missing terms but retain the age exponent
  for (const [age, concentration] of values.entries()) {
    // never give missing hours a zero concentration or a denominator vote
    if (concentration !== null) {
      const factor = weight ** age;
      numerator += concentration * factor;
      denominator += factor;
    }
  }
  return pm25ToAqi(numerator / denominator);
}

const standardOffsetCache = new Map<string, number>();

// use local standard time year-round so every EPA daily block contains 24 hours
export function airQualityDayKey(instant: string, timezone: string): string {
  const date = new Date(instant);
  const year = date.getUTCFullYear();
  const cacheKey = `${timezone}/${String(year)}`;
  let offset = standardOffsetCache.get(cacheKey);
  // choose the standard offset rather than the daylight-saving offset
  if (offset === undefined) {
    const formatter = new Intl.DateTimeFormat("en-US", { timeZone: timezone, timeZoneName: "longOffset" });
    offset = Math.min(...[0, 6].map(
      // northern and southern hemisphere winter offsets are both covered
      (month) => {
        const label = formatter.formatToParts(new Date(Date.UTC(year, month, 15))).find((part) => part.type === "timeZoneName")?.value ?? "GMT";
        const match = /^GMT([+-])(\d{2}):(\d{2})$/u.exec(label);
        return match === null ? 0 : (match[1] === "-" ? -1 : 1) * (Number(match[2]) * 60 + Number(match[3]));
      },
    ));
    standardOffsetCache.set(cacheKey, offset);
  }
  return new Date(date.getTime() + offset * 60_000).toISOString().slice(0, 10);
}

// forecast AQI represents a complete predicted daily PM2.5 mean not an hourly index
export function dailyForecastAqi(
  samples: readonly { readonly validAt: string; readonly concentration: number | null }[],
  timezone: string,
): ReadonlyMap<string, number> {
  const days = new Map<string, Map<number, number>>();
  // deduplicate absolute hourly slots before averaging
  for (const sample of samples) {
    const timestamp = Date.parse(sample.validAt);
    const value = sample.concentration;
    // withhold invalid or nonhourly forecast samples
    if (!Number.isFinite(timestamp) || timestamp % 3_600_000 !== 0 || value === null || !Number.isFinite(value) || value < 0) {
      continue;
    }
    const key = airQualityDayKey(sample.validAt, timezone);
    const hours = days.get(key) ?? new Map<number, number>();
    hours.set(timestamp, value);
    days.set(key, hours);
  }
  const result = new Map<string, number>();
  // do not extrapolate partial forecast days
  for (const [key, hours] of days) {
    // require all 24 predicted hours instead of applying observation completeness rules
    if (hours.size === 24) {
      const mean = [...hours.values()].reduce((sum, value) => sum + value, 0) / 24;
      result.set(key, pm25ToAqi(mean)!);
    }
  }
  return result;
}

export interface UnitPreferenceStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export const UNIT_PREFERENCE_STORAGE_KEY = "weather.unit-preferences.v1";

export const DEFAULT_UNIT_PREFERENCES: UnitPreferences = {
  precipitation: "inches",
  pressure: "atmosphere_percent",
  temperature: "fahrenheit",
  waterLevel: "feet",
  windSpeed: "miles_per_hour",
};

const PRECIPITATION_UNITS = new Set<PrecipitationUnit>(["inches", "millimeters"]);
const PRESSURE_UNITS = new Set<PressureUnit>([
  "atmosphere_percent",
  "hectopascals",
  "inches_of_mercury",
]);
const TEMPERATURE_UNITS = new Set<TemperatureUnit>(["celsius", "fahrenheit"]);
const WATER_LEVEL_UNITS = new Set<WaterLevelUnit>(["feet", "meters"]);
const WIND_SPEED_UNITS = new Set<WindSpeedUnit>([
  "kilometers_per_hour",
  "meters_per_second",
  "miles_per_hour",
]);

// load one validated browser preference record
export function loadUnitPreferences(
  storage: UnitPreferenceStorage | null,
): UnitPreferences {
  // retain defaults without browser storage
  if (storage === null) {
    return DEFAULT_UNIT_PREFERENCES;
  }

  try {
    const serialized = storage.getItem(UNIT_PREFERENCE_STORAGE_KEY);

    // retain defaults before first customization
    if (serialized === null) {
      return DEFAULT_UNIT_PREFERENCES;
    }

    return normalizeUnitPreferences(JSON.parse(serialized) as unknown);
  } catch {
    return DEFAULT_UNIT_PREFERENCES;
  }
}

// persist one normalized browser preference record
export function persistUnitPreferences(
  storage: UnitPreferenceStorage | null,
  preferences: UnitPreferences,
): void {
  // skip unavailable browser storage
  if (storage === null) {
    return;
  }

  try {
    storage.setItem(UNIT_PREFERENCE_STORAGE_KEY, JSON.stringify(preferences));
  } catch {
    // retain in-memory preferences when storage is unavailable
  }
}

// replace invalid or stale preference fields with US defaults
export function normalizeUnitPreferences(value: unknown): UnitPreferences {
  const record = isRecord(value) ? value : {};
  return {
    precipitation: readPreference(
      record.precipitation,
      PRECIPITATION_UNITS,
      DEFAULT_UNIT_PREFERENCES.precipitation,
    ),
    pressure: readPreference(
      record.pressure,
      PRESSURE_UNITS,
      DEFAULT_UNIT_PREFERENCES.pressure,
    ),
    temperature: readPreference(
      record.temperature,
      TEMPERATURE_UNITS,
      DEFAULT_UNIT_PREFERENCES.temperature,
    ),
    waterLevel: readPreference(
      record.waterLevel,
      WATER_LEVEL_UNITS,
      DEFAULT_UNIT_PREFERENCES.waterLevel,
    ),
    windSpeed: readPreference(
      record.windSpeed,
      WIND_SPEED_UNITS,
      DEFAULT_UNIT_PREFERENCES.windSpeed,
    ),
  };
}

// format one canonical database value in the preferred display unit
export function formatMeasurement(
  value: number | null,
  kind: keyof UnitPreferences,
  preferences: UnitPreferences,
  maximumFractionDigits?: number,
): FormattedMeasurement {
  // preserve unavailable measurements
  if (value === null) {
    return { unit: "", value: "—" };
  }

  // convert from canonical metric storage
  switch (kind) {
    case "temperature":
      return preferences.temperature === "fahrenheit"
        ? formatValue((value * 9) / 5 + 32, "°F", maximumFractionDigits ?? 1)
        : formatValue(value, "°C", maximumFractionDigits ?? 1);
    case "windSpeed":
      switch (preferences.windSpeed) {
        case "miles_per_hour":
          return formatValue(value * 2.236_936_292_1, "mph", maximumFractionDigits ?? 1);
        case "kilometers_per_hour":
          return formatValue(value * 3.6, "km/h", maximumFractionDigits ?? 1);
        case "meters_per_second":
          return formatValue(value, "m/s", maximumFractionDigits ?? 1);
      }
    case "precipitation":
      return preferences.precipitation === "inches"
        ? formatValue(value / 25.4, "in", maximumFractionDigits ?? 2)
        : formatValue(value, "mm", maximumFractionDigits ?? 1);
    case "pressure":
      switch (preferences.pressure) {
        case "atmosphere_percent":
          return formatAtmosphereDeviation(value);
        case "inches_of_mercury":
          return formatValue(value * 0.029_529_983_1, "inHg", 1, 1);
        case "hectopascals":
          return formatValue(value, "hPa", 1, 1);
      }
    case "waterLevel":
      return preferences.waterLevel === "feet"
        ? formatValue(value * 3.280_839_895, "ft", maximumFractionDigits ?? 1)
        : formatValue(value, "m", maximumFractionDigits ?? 2);
  }
}

// expose browser storage without breaking restricted contexts
export function browserUnitPreferenceStorage(): UnitPreferenceStorage | null {
  try {
    // require the browser storage surface
    if (!("localStorage" in globalThis)) {
      return null;
    }

    return globalThis.localStorage;
  } catch {
    return null;
  }
}

// select one allowlisted stored preference
function readPreference<Value extends string>(
  value: unknown,
  allowed: ReadonlySet<Value>,
  fallback: Value,
): Value {
  // reject malformed or unsupported values
  if (typeof value !== "string" || !allowed.has(value as Value)) {
    return fallback;
  }

  return value as Value;
}

// identify a JSON object safely
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

// format one converted numeric value
function formatValue(
  value: number,
  unit: string,
  maximumFractionDigits: number,
  minimumFractionDigits = 0,
): FormattedMeasurement {
  return {
    unit,
    value: new Intl.NumberFormat("en-US", {
      maximumFractionDigits,
      minimumFractionDigits,
    }).format(value),
  };
}

// format pressure as a fixed one-decimal deviation from one atmosphere
function formatAtmosphereDeviation(valueHpa: number): FormattedMeasurement {
  const deviationPercent = ((valueHpa / 1_013.25) - 1) * 100;
  return {
    unit: "%",
    value: new Intl.NumberFormat("en-US", {
      maximumFractionDigits: 1,
      minimumFractionDigits: 1,
      signDisplay: "always",
    }).format(deviationPercent),
  };
}
