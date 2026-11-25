import type {
  ForecastAdjustmentDecision,
  ForecastAdjustmentMetric,
} from "@weather/domain";
import type {
  ForecastAdjustmentWindQualifiedMaintenanceDecision,
  ForecastTemperatureCanaryDecisionV1,
} from "@weather/forecast-adjustment";

import type { ApiSite } from "./sites.js";
import type { ApiWeatherRecord } from "./weather-records.js";

export type ForecastDays = 1 | 5 | 10;

// describe the rain status included in the forecast response
export interface ApiRainAdjustmentRuntime {
  readonly state: "active" | "disabled";
  readonly activeBundle: string | null;
  readonly reasonCode: "policy_raw" | "registry_invalid" | "model_unavailable" | null;
  readonly loadedAt: string;
  readonly source: null | {
    readonly runInitializedAt: string;
    readonly firstReceivedAt: string;
    readonly decisionAt: string;
    readonly hourCount: number;
  };
}

// describe one rain adjustment decision attached to a forecast hour
export interface ApiRainAdjustmentDecision {
  readonly contractVersion: "forecast-rain-adjustment-decision/v1";
  readonly state: "active" | "disabled" | "raw_fallback";
  readonly reasonCode: string | null;
  readonly bundleSha256: string | null;
  readonly correctedPrecipitationMm: number | null;
  readonly rawBestMatchPrecipitationMm: number | null;
  readonly sourceForecast: null | {
    readonly runInitializedAt: string;
    readonly firstReceivedAt: string;
    readonly validAt: string;
    readonly rawPrecipitationMm: number;
    readonly modelLeadHours: number;
    readonly decisionAt: string;
    readonly upstreamModel: "ecmwf_ifs";
    readonly providerKey: "open-meteo";
  };
}

export type ApiForecastAdjustmentDecision =
  | ForecastAdjustmentDecision
  | ForecastAdjustmentWindQualifiedMaintenanceDecision;

// extend only forecast rows with their three independent decisions
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
  readonly reasonCode:
    | "bundle_invalid"
    | "bundle_missing"
    | "registry_inactive"
    | "registry_invalid"
    | "canary_expired"
    | "canary_killed"
    | "policy_raw"
    | null;
  readonly state: "active" | "disabled";
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
  readonly reasonCode:
    | "bundle_invalid"
    | "bundle_missing"
    | "canary_expired"
    | "canary_killed"
    | "registry_inactive"
    | "registry_invalid"
    | "policy_raw"
    | null;
  readonly source: null | {
    readonly adaptiveReady: boolean;
    readonly firstReceivedAt: string;
    readonly hourCount: number;
    readonly latestRunInitializedAt: string;
    readonly stateReason: string | null;
    readonly stateStatus: "supported" | "cold" | "insufficient" | "invalid";
  };
  readonly state: "active" | "disabled";
}

// describe the complete forecast response before browser normalization
export interface ApiForecastResponse {
  readonly adjustmentRuntime: ApiForecastAdjustmentRuntime;
  readonly airQualityContext: readonly ApiForecastWeatherRecord[];
  readonly rainAdjustmentRuntime: ApiRainAdjustmentRuntime;
  readonly data: readonly ApiForecastWeatherRecord[];
  readonly days: ForecastDays;
  readonly generatedAt: string;
  readonly pressureContext: readonly ApiWeatherRecord[];
  readonly site: ApiSite;
  readonly temperatureAdjustmentRuntime: ApiForecastTemperatureAdjustmentRuntime;
}
