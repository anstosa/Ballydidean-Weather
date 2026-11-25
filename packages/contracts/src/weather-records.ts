import type { SourceKind } from "@weather/domain";

import type { ApiSite } from "./sites.js";

export interface ApiAirQualityHourlyContext {
  readonly hourlyPm25: readonly (number | null)[];
  readonly through: string;
}

export interface ApiPropertySensorSnapshot {
  readonly airQuality?: ApiAirQualityHourlyContext | null;
  readonly channel: number | null;
  readonly key: string;
  readonly model: string;
  readonly readings: Readonly<Record<string, number>>;
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
  readonly airQuality: ApiAirQualityHourlyContext | null;
  readonly pressureChange3hHpa: number | null;
}

// describe current weather at the selected site
export interface ApiCurrentWeatherResponse {
  readonly data: readonly ApiCurrentWeatherRecord[];
  readonly generatedAt: string;
  readonly site: ApiSite;
}

// describe a history page with a nullable continuation cursor
export interface ApiHistoryResponse {
  readonly data: readonly ApiWeatherRecord[];
  readonly generatedAt: string;
  readonly page: {
    readonly limit: number;
    readonly nextCursor: string | null;
  };
  readonly site: ApiSite;
}
