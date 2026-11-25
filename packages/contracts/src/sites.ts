import type { SourceKind, StationKind } from "@weather/domain";

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

// describe the active site list
export interface ApiSitesResponse {
  readonly data: readonly ApiSite[];
}
