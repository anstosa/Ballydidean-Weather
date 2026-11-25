/** One immutable projection identity retained in a page envelope. */
export interface ForecastArchiveProjection {
  readonly channel: "organic" | "scheduler_request";
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly identitySha256: string;
}

/** The predecessor-bound header shared by the online page and transfer acknowledgement. */
export interface ForecastArchivePageHeaderV1 {
  readonly contractVersion: "adjustment-cycle-page/v1";
  /** Wire: `capture/YYYY-MM-DDT{00|06|12|18}:35:00.000Z`. */
  readonly dueKey: string;
  /** Wire: unsigned uint64 decimal text without leading zeroes except for `0`. */
  readonly generation: string;
  /** Wire: safe integer in the range 0..=18. */
  readonly pageIndex: number;
  /** Wire: nullable lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly predecessorPageSha256: string | null;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly payloadSha256: string;
  /** Wire: nonnegative safe-integer byte offset. */
  readonly payloadOffset: number;
}

/** One current online page disclosed by `archive-next`. */
export interface ArchiveTransferPageEnvelopeV1 {
  readonly contractVersion: "adjustment-archive-transfer/v1";
  readonly kind: "page";
  readonly header: ForecastArchivePageHeaderV1;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly pageSha256: string;
  /** Wire: standard padded base64 text. */
  readonly payloadBase64: string;
  readonly projections: readonly ForecastArchiveProjection[];
}

/** The stable marker returned when no final or page is pending. */
export interface ArchiveTransferIdleEnvelopeV1 {
  readonly contractVersion: "adjustment-archive-transfer/v1";
  readonly kind: "idle";
}

/** One immutable page-protocol acknowledgement persisted before slot release. */
export interface ForecastArchivePageAcknowledgementV1 {
  /** Wire: canonical UTC ISO-8601 text with millisecond precision. */
  readonly acknowledgedAt: string;
  readonly contractVersion: "adjustment-cycle-page-ack/v1";
  /** Wire: `capture/YYYY-MM-DDT{00|06|12|18}:35:00.000Z`. */
  readonly dueKey: string;
  /** Wire: unsigned uint64 decimal text without leading zeroes except for `0`. */
  readonly generation: string;
  /** Wire: safe integer in the range 0..=18. */
  readonly pageIndex: number;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly pageSha256: string;
}

/** The workstation checkpoint retained before one page slot is released. */
export interface ForecastArchiveTransferAcknowledgementV1 {
  /** Wire: canonical UTC ISO-8601 text with millisecond precision. */
  readonly acknowledgedAt: string;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly checkpointSha256: string;
  readonly contractVersion: "adjustment-archive-transfer-ack/v1";
  readonly header: ForecastArchivePageHeaderV1;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly pageSha256: string;
  /** Wire: safe integer in the range 1..=262144. */
  readonly payloadLength: number;
  readonly projections: readonly ForecastArchiveProjection[];
}

/** The first verified cold graph bound to one immutable final manifest. */
export interface ForecastArchiveFinalAcknowledgementV1 {
  /** Wire: canonical UTC ISO-8601 text with millisecond precision. */
  readonly acknowledgedAt: string;
  readonly contractVersion: "adjustment-archive-final-ack/v1";
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly graphSha256: string;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly manifestSha256: string;
}

/** One explicitly declared producer population root. */
export interface ForecastArchiveRequiredInputRoot {
  /** Wire: safe integer 0..=8192; zero requires the SHA-256 root of canonical `[]\n`. */
  readonly count: number;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly rootSha256: string;
}

/** Every mandatory producer class closed by a sealed final manifest. */
export interface ForecastArchiveRequiredInputs {
  readonly actual_best_match: ForecastArchiveRequiredInputRoot;
  readonly native_source: ForecastArchiveRequiredInputRoot;
  readonly rain_gate_input: ForecastArchiveRequiredInputRoot;
  /** Wire: this population has at least one member. */
  readonly serving_evidence: ForecastArchiveRequiredInputRoot;
  /** Wire: count equals `shadow_metadata.count`. */
  readonly shadow_body: ForecastArchiveRequiredInputRoot;
  readonly shadow_metadata: ForecastArchiveRequiredInputRoot;
  readonly target_revision: ForecastArchiveRequiredInputRoot;
}

/** The immutable combined producer closure required by final manifest v2. */
export interface ForecastArchiveInputSealV1 {
  /** Wire: safe integer in the range 1..=4096. */
  readonly acknowledgedProjectionCount: number;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly acknowledgedProjectionRootSha256: string;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly acknowledgementRootSha256: string;
  readonly contractVersion: "adjustment-cycle-input-seal/v1";
  /** Wire: `capture/YYYY-MM-DDT{00|06|12|18}:35:00.000Z`. */
  readonly dueKey: string;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly evidenceGapRootSha256: string;
  /** Wire: unsigned uint64 decimal text without leading zeroes except for `0`. */
  readonly generation: string;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly pageRootSha256: string;
  readonly requiredInputs: ForecastArchiveRequiredInputs;
  /** Wire: canonical UTC ISO-8601 text with millisecond precision. */
  readonly sealedAt: string;
}

/** One ordered payload range retained without duplicating its page bytes. */
export interface ForecastArchiveFinalPage {
  /** Wire: safe integer in the range 0..=18 and equal to the array index. */
  readonly pageIndex: number;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly pageSha256: string;
  /** Wire: nullable lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly predecessorPageSha256: string | null;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly payloadSha256: string;
  /** Wire: nonnegative safe-integer byte offset. */
  readonly payloadOffset: number;
  /** Wire: safe integer in the range 1..=262144. */
  readonly payloadLength: number;
}

/** The legacy immutable final page-chain closure without an explicit producer seal. */
export interface ForecastArchiveFinalManifestV1 {
  readonly contractVersion: "adjustment-cycle-final-manifest/v1";
  /** Wire: `capture/YYYY-MM-DDT{00|06|12|18}:35:00.000Z`. */
  readonly dueKey: string;
  /** Wire: unsigned uint64 decimal text without leading zeroes except for `0`. */
  readonly generation: string;
  /** Wire: canonical UTC ISO-8601 text with millisecond precision. */
  readonly finalizedAt: string;
  /** Wire: safe integer in the range 1..=19. */
  readonly pageCount: number;
  /** Wire: safe integer in the range 1..=4947968. */
  readonly totalPayloadBytes: number;
  readonly pages: readonly ForecastArchiveFinalPage[];
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly pageRootSha256: string;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly acknowledgementRootSha256: string;
  /** Wire: nonnegative safe integer. */
  readonly acknowledgedProjectionCount: number;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly acknowledgedProjectionRootSha256: string;
  /** Wire: nonnegative safe integer. */
  readonly evidenceGapCount: number;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly evidenceGapRootSha256: string;
}

/** The sealed immutable final page-chain closure used by the retained runtime. */
export interface ForecastArchiveFinalManifestV2 {
  readonly contractVersion: "adjustment-cycle-final-manifest/v2";
  /** Wire: `capture/YYYY-MM-DDT{00|06|12|18}:35:00.000Z`. */
  readonly dueKey: string;
  /** Wire: unsigned uint64 decimal text without leading zeroes except for `0`. */
  readonly generation: string;
  /** Wire: canonical UTC ISO-8601 text with millisecond precision. */
  readonly finalizedAt: string;
  /** Wire: safe integer in the range 1..=19. */
  readonly pageCount: number;
  /** Wire: safe integer in the range 1..=4947968. */
  readonly totalPayloadBytes: number;
  readonly pages: readonly ForecastArchiveFinalPage[];
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly pageRootSha256: string;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly acknowledgementRootSha256: string;
  /** Wire: safe integer in the range 1..=4096. */
  readonly acknowledgedProjectionCount: number;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly acknowledgedProjectionRootSha256: string;
  /** Wire: nonnegative safe integer. */
  readonly evidenceGapCount: number;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly evidenceGapRootSha256: string;
  readonly inputSeal: ForecastArchiveInputSealV1;
}

/** Either deployed immutable final manifest schema. */
export type ForecastArchiveFinalManifest =
  | ForecastArchiveFinalManifestV1
  | ForecastArchiveFinalManifestV2;

/** One genuine final disclosed before any later online page. */
export interface ArchiveTransferFinalEnvelopeV1 {
  readonly contractVersion: "adjustment-archive-transfer/v1";
  readonly kind: "final";
  readonly manifest: ForecastArchiveFinalManifest;
  /** Wire: lowercase SHA-256 with exactly 64 ASCII hexadecimal characters. */
  readonly manifestSha256: string;
}

/** The exact discriminated response returned by `archive-next`. */
export type ArchiveTransferEnvelope =
  | ArchiveTransferFinalEnvelopeV1
  | ArchiveTransferPageEnvelopeV1
  | ArchiveTransferIdleEnvelopeV1;
