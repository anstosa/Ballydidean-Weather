import { RAIN_COLLECTION_STATIONS } from "../adjustment-maintenance-runtime/node_modules/@weather/domain/dist/rain-collection.js";
import { encodeAdjustmentRainGateFeatureProjection } from "../adjustment-maintenance-runtime/forecast/maintenance-revision-projection.js";
import { encodeMaintenanceBinary64 } from "../adjustment-maintenance-runtime/forecast/maintenance-shadow-values.js";
import { buildRainFixedGaugeTargetProjection } from "../../../apps/worker/dist/rain-adjustment.js";
import { adjustmentSha256, canonicalJsonBytes } from "../adjustment_plaintext_archive.mjs";

const A = "a".repeat(64);
const B = "b".repeat(64);
const validAt = "2026-10-12T07:00:00.000Z";

// represent an authenticated synthetic epoch without production authority
export function rainHistoryEpochWitness() {
  const epochAt = "2026-10-10T08:00:00.000Z";
  const frontier = adjustmentSha256(Buffer.from("adjustment-revision-frontier/v1\n0\n"));
  const unsigned = { activationKind: "inert_v14_pre_activation", archiveCommitOrdinal: "0",
    catalogFrontierSha256: frontier, contractVersion: "adjustment-revision-capture-epoch-witness/v1",
    controlPlaneSha256: A, controlPlaneVersion: "14",
    databaseMigrationHistorySha256: "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
    epochAt, servingSnapshotSha256: adjustmentSha256(Buffer.from([
      "adjustment-revision-serving-snapshot/v1", epochAt, "0", frontier, ""].join("\n"))),
    sourceCommit: "1".repeat(40), sourceRelease: "2026.10.10-1",
    sourceServerImageDigest: `sha256:${A}`, sourceWebImageDigest: `sha256:${B}` };
  return { ...unsigned, witnessSha256: adjustmentSha256(canonicalJsonBytes(unsigned)) };
}

// build complete genuine raw-body target bytes through the production minute aggregator
export function rainHistoryTargetOccurrence({ completedAt = "2026-10-12T07:10:00.000Z",
  ordinal = 2, receiptAt = "2026-10-12T07:15:00.000Z" } = {}) {
  const captures = RAIN_COLLECTION_STATIONS.map((station) => {
    const start = Date.parse("2026-10-12T05:59:00.000Z");
    const obs = Array.from({ length: 65 }, (_, index) => [
      (start + (index + 1) * 60_000) / 1_000, 0, 1, 2, 180, 3, 1_000, 12, 80,
      0, 0, 0, .01, 0, 0, 0, 2.7, 1, 0, 0, 0, 0,
    ]);
    const body = Buffer.from(JSON.stringify({ device_id: station.deviceId, obs,
      status: { status_code: 0 }, type: "obs_st" }));
    return { body, bodySha256: adjustmentSha256(body), claimId: `target-${station.locationId}`,
      completedAt, kind: "station", runInitializedAt: null,
      stationId: station.locationId, windowEndExclusive: "2026-10-12T07:05:00.000Z",
      windowStart: "2026-10-12T05:59:00.000Z" };
  });
  const built = buildRainFixedGaugeTargetProjection({ captures,
    sources: RAIN_COLLECTION_STATIONS.map((station) => ({
      sourceId: `rain-target-tempest-${station.locationId}`, stationId: station.locationId })),
    targetCutoffAt: "2026-10-12T08:00:00.000Z", validAt });
  // a fixture cannot silently use an unsupported target in place of actual bytes
  if (built.state !== "complete") throw new Error("rain target fixture is incomplete");
  return occurrence(built.bytes, ordinal, receiptAt, 12);
}

// build the actual frozen 107-feature prefit projection for the target hour
export function rainHistoryFeatureOccurrence({ logicalReceivedAt = "2026-10-11T20:00:00.000Z",
  receiptAt = "2026-10-11T20:01:00.000Z", signedZero = false } = {}) {
  const runInitializedAt = "2026-10-11T12:00:00.000Z";
  const document = { contractVersion: "adjustment-rain-gate-feature-projection/v2", family: "rain",
    logicalKey: { inputSha256: A, modelSha256: B, runInitializedAt },
    logicalReceivedAt, projectionKind: "rain_gate_input",
    rows: Array.from({ length: 23 }, (_, index) => ({
      features64: Array.from({ length: 107 }, (_unused, position) =>
        encodeMaintenanceBinary64(signedZero && position === 20 ? -0 : 1)),
      modelLeadHours: index + 9, rawPrecipitationMm64: encodeMaintenanceBinary64(.5),
      rawTargetHourTemperatureC64: encodeMaintenanceBinary64(10),
      validAt: new Date(Date.parse(runInitializedAt) + (index + 9) * 3_600_000).toISOString(),
    })), source: { adapterVersion: "rain-hurdle-wind-features/v1", contractEpoch: "rain-prospective-capture/v1",
      dataset: "ecmwf_ifs", providerKey: "open-meteo-single-runs",
      sourceConfigFingerprint: "96e4365f74a0fb8694944172b73a65c29d89a68ddf86b1b0a50db38bb39d52aa",
      sourceId: "actual-claim", sourceKey: "rain-prospective-forecast", sourceKind: "forecast",
      upstreamModel: "ecmwf_ifs" }, storedContentSha256: A };
  return occurrence(encodeAdjustmentRainGateFeatureProjection(document), 1,
    receiptAt, 1);
}

// represent a verified synthetic archive occurrence with exact grouped receipt semantics
function occurrence(payloadBytes, ordinal, archiveCommittedAt, count) {
  const document = JSON.parse(payloadBytes.toString("utf8"));
  const payloadIdentitySha256 = adjustmentSha256(payloadBytes);
  const receipts = Array.from({ length: count }, (_, index) => {
    const unsigned = { archiveCommitOrdinal: String(ordinal + index), archiveCommittedAt,
      contractVersion: "adjustment-revision-commit-receipt/v1", frontierSha256: A,
      predecessorFrontierSha256: B, projectionIdentitySha256: payloadIdentitySha256,
      projectionKind: document.projectionKind, projectionSha256: payloadIdentitySha256,
      stageReceiptSha256: adjustmentSha256(Buffer.from(`stage-${ordinal}`)) };
    return { ...unsigned, receiptSha256: adjustmentSha256(canonicalJsonBytes(unsigned)) };
  });
  return { graphManifestSha256: adjustmentSha256(Buffer.from(`graph-${ordinal}`)),
    pageSha256: adjustmentSha256(Buffer.from(`page-${ordinal}`)), payloadBytes,
    payloadIdentitySha256, payloadKind: document.contractVersion, publicationDisposition: "published", receipts };
}

// retain exact occurrences without assigning real production archive authority
export function rainHistory(occurrences = [rainHistoryFeatureOccurrence(), rainHistoryTargetOccurrence()]) {
  return { catalog: {}, contractVersion: "adjustment-revision-historical-archive-index/v1",
    historyRootSha256: A, occurrences, pages: [], receiptCount: occurrences.reduce(
      (count, item) => count + item.receipts.length, 0) };
}
