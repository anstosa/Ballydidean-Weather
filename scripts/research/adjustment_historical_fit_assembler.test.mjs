import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";

import {
  encodeAdjustmentRevisionProjection,
  encodeAdjustmentTemperatureNativeSourceProjection,
} from "./adjustment-maintenance-runtime/forecast/maintenance-revision-projection.js";
import {
  encodeMaintenanceBinary64,
} from "./adjustment-maintenance-runtime/forecast/maintenance-shadow-values.js";
import {
  buildAdjustmentHistoricalFitProjection,
} from "./adjustment_historical_fit_assembler.mjs";
import {
  adjustmentSha256,
  canonicalJsonBytes,
} from "./adjustment_plaintext_archive.mjs";

const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);

// build one exact future-only epoch witness
function epochWitness() {
  const cutoffAt = "2026-10-10T08:00:00.000Z";
  const frontierSha256 = adjustmentSha256(Buffer.from(
    "adjustment-revision-frontier/v1\n0\n",
  ));
  const snapshotSha256 = adjustmentSha256(Buffer.from([
    "adjustment-revision-serving-snapshot/v1", cutoffAt, "0", frontierSha256, "",
  ].join("\n")));
  const unsigned = {
    activationKind: "inert_v14_pre_activation",
    archiveCommitOrdinal: "0",
    catalogFrontierSha256: frontierSha256,
    contractVersion: "adjustment-revision-capture-epoch-witness/v1",
    controlPlaneSha256: HASH_A,
    controlPlaneVersion: "14",
    databaseMigrationHistorySha256:
      "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
    epochAt: cutoffAt,
    servingSnapshotSha256: snapshotSha256,
    sourceCommit: "1".repeat(40),
    sourceRelease: "2026.10.10-1",
    sourceServerImageDigest: `sha256:${HASH_A}`,
    sourceWebImageDigest: `sha256:${HASH_B}`,
  };
  return { ...unsigned, witnessSha256: adjustmentSha256(canonicalJsonBytes(unsigned)) };
}

// construct one complete non-rain source lineage
function source() {
  return {
    adapterVersion: "open-meteo/v4",
    contractEpoch: "forecast/v4",
    dataset: "best_match",
    providerKey: "open-meteo",
    sourceConfigFingerprint: "open-meteo-forecast/v4",
    sourceId: "7",
    sourceKey: "open-meteo-forecast",
    sourceKind: "forecast",
    upstreamModel: "best_match",
  };
}

// construct one normalized weather row
function weatherRow(validAt) {
  return {
    apparentTemperatureC64: encodeMaintenanceBinary64(10),
    blackGlobeTemperatureC64: null,
    cloudCoverPercent64: encodeMaintenanceBinary64(50),
    contentSha256: HASH_A,
    pm25MicrogramsPerCubicMeter64: null,
    precipitationMm64: encodeMaintenanceBinary64(0),
    precipitationRateMmPerHour64: null,
    pressureHpa64: encodeMaintenanceBinary64(1_010),
    relativeHumidityPercent64: encodeMaintenanceBinary64(80),
    soilElectricalConductivityMicrosiemensPerCm64: null,
    soilMoisturePercent64: null,
    solarRadiationWm264: null,
    temperatureC64: encodeMaintenanceBinary64(11),
    uvIndex64: null,
    validAt,
    waterLevelM64: null,
    wetBulbGlobeTemperatureC64: null,
    windDirectionDegrees64: encodeMaintenanceBinary64(180),
    windGustMps64: encodeMaintenanceBinary64(8),
    windSpeedMps64: encodeMaintenanceBinary64(5),
  };
}

// construct authentic temperature-relevant projection documents
function temperatureDocuments(validAt = "2026-10-11T09:00:00.000Z") {
  const actual = {
    contractVersion: "adjustment-revision-projection/v1",
    family: "shared",
    logicalKey: {
      productRunAt: "2026-10-11T08:00:00.000Z",
      sourceId: "7",
      sourceKind: "forecast",
      validAt,
    },
    logicalReceivedAt: "2026-10-11T08:05:00.000Z",
    projectionKind: "actual_best_match",
    rows: [weatherRow(validAt)],
    source: source(),
    storedContentSha256: HASH_A,
  };
  const target = structuredClone(actual);
  target.logicalKey.productRunAt = null;
  target.logicalKey.sourceKind = "physical_sensor";
  target.projectionKind = "target_revision";
  target.source.sourceKind = "physical_sensor";
  target.source.upstreamModel = null;
  const temperature = {
    ...structuredClone(actual),
    contractVersion: "adjustment-temperature-native-source-projection/v2",
    family: "temperature",
    logicalKey: {
      contentSha256: HASH_A,
      leadHours: null,
      providerResponseSha256: HASH_B,
      runInitializedAt: "2026-10-11T08:00:00.000Z",
      siteId: "3",
      sourceId: null,
      sourceType: "ecmwf_temperature_run",
      validAt: null,
    },
    projectionKind: "native_source",
    rows: Array.from({ length: 18 }, (_, index) => ({
      contentSha256: createHash("sha256").update(`temperature-${index}`).digest("hex"),
      modelLeadHours: index + 1,
      modelCycle: "50r1",
      bestMatchContentSha256: index < 6 ? null : HASH_A,
      bestMatchProductRunAt: index < 6 ? null : "2026-10-11T08:00:00.000Z",
      bestMatchSourceId: index < 6 ? null : "7",
      bestMatchTemperatureC64: index < 6 ? null : encodeMaintenanceBinary64(11),
      rawRelativeHumidityPercent64: encodeMaintenanceBinary64(80),
      rawTemperatureC64: encodeMaintenanceBinary64(11),
      rawWindSpeedMps64: encodeMaintenanceBinary64(5),
      validAt: new Date(Date.parse("2026-10-11T09:00:00.000Z") +
        index * 3_600_000).toISOString(),
    })),
    source: {
      ...source(),
      adapterVersion: "open-meteo-ecmwf-single-run/v1",
      contractEpoch: "temperature-canary/v1",
      dataset: "single_run",
      upstreamModel: "ecmwf_ifs",
    },
  };
  // preserve the database fixed-order empty source-decision state and hash
  temperature.recentErrorState = {
    b24C: null, b72C: null, cohort: "ecmwf_single_run_hindcast", localDates: 0,
    mad72C: null, maximumSourceRunInitializedAt: null, maximumSourceValidAt: null,
    n24: 0, n72: 0, sourceKeys: [], supported: false,
    targetRunInitializedAt: "2026-10-11T08:00:00.000Z",
    windowEndValidAt: "2026-10-11T01:00:00.000Z",
  };
  temperature.recentErrorStateSha256 = createHash("sha256")
    .update(JSON.stringify(temperature.recentErrorState)).digest("hex");
  return [actual, target, temperature];
}

// wrap exact projection bytes in one verified archive occurrence
function occurrence(document, ordinal) {
  const payloadBytes = document.contractVersion === "adjustment-temperature-native-source-projection/v2"
    ? encodeAdjustmentTemperatureNativeSourceProjection(document)
    : encodeAdjustmentRevisionProjection(document);
  const payloadIdentitySha256 = adjustmentSha256(payloadBytes);
  return Object.freeze({
    graphManifestSha256: adjustmentSha256(Buffer.from(`graph-${ordinal}`)),
    pageSha256: adjustmentSha256(Buffer.from(`page-${ordinal}`)),
    payloadBytes,
    payloadIdentitySha256,
    payloadKind: document.contractVersion,
    publicationDisposition: "published",
    receipts: Object.freeze([{
      archiveCommitOrdinal: String(ordinal),
      archiveCommittedAt: `2026-10-12T08:0${ordinal}:00.000Z`,
      contractVersion: "adjustment-revision-commit-receipt/v1",
      frontierSha256: HASH_A,
      predecessorFrontierSha256: HASH_B,
      projectionIdentitySha256: payloadIdentitySha256,
      projectionKind: document.projectionKind,
      projectionSha256: payloadIdentitySha256,
      receiptSha256: adjustmentSha256(Buffer.from(`receipt-${ordinal}`)),
      stageReceiptSha256: adjustmentSha256(Buffer.from(`stage-${ordinal}`)),
    }]),
  });
}

// construct one already-verified historical archive boundary
function history(documents) {
  return Object.freeze({
    catalog: {},
    contractVersion: "adjustment-revision-historical-archive-index/v1",
    historyRootSha256: adjustmentSha256(Buffer.from("history")),
    occurrences: Object.freeze(documents.map(
      // preserve the supplied server order
      (document, index) => occurrence(document, index + 1),
    )),
    pages: Object.freeze([]),
    receiptCount: documents.length,
  });
}

test("fit projection retains authentic post-epoch temperature rows", () => {
  const result = buildAdjustmentHistoricalFitProjection({
    epochWitness: epochWitness(),
    family: "temperature",
    fitMonth: "2026-12",
    history: history(temperatureDocuments()),
  });
  assert.equal(result.contractVersion, "adjustment-revision-fit-projection/v2");
  assert.equal(result.classCounts.actual_best_match, 1);
  assert.equal(result.classCounts.native_source, 18);
  assert.equal(result.classCounts.target_revision, 1);
  assert.equal(result.rowCount, 20);
  assert.equal(result.projectionMembers.every((member) =>
    member.receipt.archiveCommittedAt >= epochWitness().epochAt), true);
  assert.match(result.memberRootSha256, /^[a-f0-9]{64}$/u);
  const nativeMembers = result.projectionMembers.filter((member) => member.projectionKind === "native_source");
  assert.equal(nativeMembers.length, 18);
  // retain one owned body per grouped document instead of copying it for every numerical row
  assert.equal(nativeMembers.every((member) => member.payloadBytes === nativeMembers[0].payloadBytes), true);
});

test("fit projection rejects a pre-epoch causal row", () => {
  const documents = temperatureDocuments("2026-10-09T09:00:00.000Z");
  documents[0].logicalKey.productRunAt = "2026-10-09T08:00:00.000Z";
  documents[0].logicalReceivedAt = "2026-10-09T08:05:00.000Z";
  assert.throws(() => buildAdjustmentHistoricalFitProjection({
    epochWitness: epochWitness(),
    family: "temperature",
    fitMonth: "2026-12",
    history: history(documents),
  }), /predates its epoch/u);
});

// pre-epoch comparator runs cannot enter a future-only native population
test("fit projection rejects a pre-epoch native comparator initialization", () => {
  const documents = temperatureDocuments();
  documents[2].rows[6].bestMatchProductRunAt = "2026-10-09T08:00:00.000Z";
  assert.throws(() => buildAdjustmentHistoricalFitProjection({
    epochWitness: epochWitness(),
    family: "temperature",
    fitMonth: "2026-12",
    history: history(documents),
  }), /predates its epoch/u);
});
