import type { ApiSite } from "./sites.js";

export interface ApiDailyPrecipitation {
  readonly accumulationMm: number;
  readonly source: {
    readonly sourceId: string;
    readonly stationSlug: string;
  };
  readonly validThrough: string;
}

// retain an explicit null when daily precipitation is unavailable
export interface ApiDailyPrecipitationResponse {
  readonly data: ApiDailyPrecipitation | null;
  readonly generatedAt: string;
  readonly site: ApiSite;
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

// describe the site tide records
export interface ApiTidesResponse {
  readonly data: readonly ApiTideRecord[];
  readonly generatedAt: string;
  readonly site: ApiSite;
}
