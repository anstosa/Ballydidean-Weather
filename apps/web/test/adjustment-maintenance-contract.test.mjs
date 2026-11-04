import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

import { buildRainFixedGaugeTargetProjection } from "../../worker/dist/rain-fixed-gauge-target.js";
import { RAIN_COLLECTION_STATIONS, weatherRecordContent } from "../../../packages/domain/dist/index.js";
import * as canonical from "../../../packages/forecast-adjustment/dist/index.js";
import * as publicContract from "../src/adjustment-maintenance-contract.mjs";

const root = resolve(import.meta.dirname, "../../..");
const HASH = "a".repeat(64);
const SECOND_HASH = "b".repeat(64);
const expectedExports = [
  "createMaintenanceShadowPredictionMetadata",
  "parseAdjustmentRainFixedGaugeTargetProjection",
  "parseAdjustmentRevisionProjectionDocument",
  "parseMaintenanceShadowComparator",
  "parseMaintenanceShadowSourceProjection",
  "parseMaintenanceShadowValues",
  "parseRainMaintenanceControlState",
  "validateMaintenanceShadowComparatorBinding",
];
const canonicalSourceHashes = {
  "packages/domain/dist/forecast-adjustment.js": "86380326785f8dd9c02b9f2ff5079ef019cdf23083d6ba6cd9262408c59ae6ae",
  "packages/domain/dist/forecast-anchor-record.js": "5732fe71d7da28fa1769cc71c3c9891dc11d1dd5de09b4652baacbb3baff3cf6",
  "packages/domain/dist/provenance.js": "137f9021a48045b8a1fca34890f93c28bfd48c16f8def11849d4f72bb8441d94",
  "packages/domain/dist/rain-collection.js": "6e512dc937f987e6299d82bb229980cc2e8fec9d2d579bab867015dee7847087",
  "packages/domain/dist/weather-record.js": "0aae2a9f51c93913c90e6ade7eaac12a85e68f3fed1f0f89aef17c70c9badcf1",
  "packages/forecast-adjustment/src/algorithm-v1.ts": "7f4f6f49cdc2e195a653e222809a6e3a46f68ceb46999e3ca81a87de287dace7",
  "packages/forecast-adjustment/src/calendar.ts": "014aaeddadee2c47b47169f5e6a7a292f70a53193292381b6a22406d995052da",
  "packages/forecast-adjustment/src/candidate.ts": "4bdfbef5165786aca995e929b513872f8f30d3d87248fc7f07046d7ee9e8672e",
  "packages/forecast-adjustment/src/evaluate.ts": "20480348e34a2a6f9972404e9fe02c9112c4eba5c3d800c818f6f731b083f74a",
  "packages/forecast-adjustment/src/maintenance-capture-epoch.ts": "7e850738195947bab7bf4733935edd8429e341dd93cd61fa1169956975f3050c",
  "packages/forecast-adjustment/src/maintenance-policy.ts": "5df440a43655c1525e32849aec772989ee032b70e21f26f3e849658928a8e8c5",
  "packages/forecast-adjustment/src/maintenance-revision-projection.ts": "66f2718173500154de3ec255adc4a84c2186f6eca5e1259cc7bea3b0900f2ffd",
  "packages/forecast-adjustment/src/maintenance-runtime-package.ts": "bf65ff19151cdae04682cb3dc39975703c08296286e37bafed8c1273b78bab6b",
  "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts": "cfa9b19d9ce591f4d020e6a52aa38a63e0e66f934ad12a7a4c608a691801004b",
  "packages/forecast-adjustment/src/maintenance-shadow-comparator.ts": "ee540e11de8238311d04d7ac82bb5f841718c31c41b4bb3b4d239bc176d376e8",
  "packages/forecast-adjustment/src/maintenance-shadow-values.ts": "28553231d0f63df45e9de2c8d425f7ca2bed8b14782c660c0fd57a19ac7717d3",
  "packages/forecast-adjustment/src/performance-scorecard.ts": "8ad4f569cb93c9f03bf2fa1c374265636111fb6eefd4dd08cdb9d0b4a6415d5b",
  "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts": "4cd1cc873d72b67c19e79e820c0d8e73bf4786f487ab391e45410fbae0adc7ca",
  "packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts": "8aa8972711e2bdb757bd6bf91ab2468a2d409b7b924393f6fb1e93daec5592ba",
  "packages/forecast-adjustment/src/rain-hurdle-wind.ts": "4d456196c4ee047a9d037cac9ade47b2f62b50b389df071f097bf451d40e0e2c",
  "packages/forecast-adjustment/src/rain-maintenance-controls.ts": "5ee3fca8132c8e8aef4a93163ba11cb3fb14890567005e3e35690a6dabf02aaf",
  "packages/forecast-adjustment/src/runtime-bundle.ts": "9f83b96a2e71c6f270e157d5d6a3d857d43bc6f0aed5ff435806fb3cc031fd5b",
  "packages/forecast-adjustment/src/runtime-loader.ts": "2e32282c0d3768d7d1261ed5659bb86d961f712e0dd7e396e660a991e9d454f3",
  "packages/forecast-adjustment/src/temperature-canary.ts": "c128eb33f59b4bbfca9f1ab1aa3717b4c1d1e4a489dd10c5650aca7e1b5096c5",
  "packages/forecast-adjustment/src/temperature-mos-runtime.ts": "33b6dbda2ebd96d4a350a3730defeaf6078bc32cf5d5f57cd58c20c4ed7a1586",
  "packages/forecast-adjustment/src/wind-canary.ts": "b7262b749c6883713274b2a59a2d7db664c86d84c81bb6938ce831ed8f4a386d",
};

// hash one exact source member
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// construct the common forecast source lineage
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

// construct one exact normalized weather row
function weatherRow(validAt = "2026-10-08T01:00:00.000Z") {
  return {
    apparentTemperatureC64: canonical.encodeMaintenanceBinary64(10),
    blackGlobeTemperatureC64: null,
    cloudCoverPercent64: canonical.encodeMaintenanceBinary64(50),
    contentSha256: HASH,
    pm25MicrogramsPerCubicMeter64: null,
    precipitationMm64: canonical.encodeMaintenanceBinary64(0),
    precipitationRateMmPerHour64: null,
    pressureHpa64: canonical.encodeMaintenanceBinary64(1010),
    relativeHumidityPercent64: canonical.encodeMaintenanceBinary64(80),
    soilElectricalConductivityMicrosiemensPerCm64: null,
    soilMoisturePercent64: null,
    solarRadiationWm264: null,
    temperatureC64: canonical.encodeMaintenanceBinary64(11),
    uvIndex64: null,
    validAt,
    waterLevelM64: null,
    wetBulbGlobeTemperatureC64: null,
    windDirectionDegrees64: canonical.encodeMaintenanceBinary64(180),
    windGustMps64: canonical.encodeMaintenanceBinary64(8),
    windSpeedMps64: canonical.encodeMaintenanceBinary64(5),
  };
}

// construct one complete immutable Tempest capture body
function fixedGaugeCapture(station) {
  const start = Date.parse("2026-09-14T05:59:00.000Z");
  const obs = [];
  // preserve every minute required by the fixed backward tiling recipe
  for (let minute = 1; minute <= 65; minute += 1) {
    obs.push([(start + minute * 60_000) / 1_000, 0, 1, 2, 180, 3, 1_000, 12, 80,
      0, 0, 0, 0.01, 0, 0, 0, 2.7, 1, 0, 0, 0, 0]);
  }
  const body = Buffer.from(JSON.stringify({
    device_id: station.deviceId,
    obs,
    status: { status_code: 0 },
    type: "obs_st",
  }));
  return {
    body,
    bodySha256: sha256(body),
    claimId: `target-${station.locationId}`,
    completedAt: "2026-09-14T07:10:00.000Z",
    kind: "station",
    runInitializedAt: null,
    stationId: station.locationId,
    windowEndExclusive: "2026-09-14T07:05:00.000Z",
    windowStart: "2026-09-14T05:59:00.000Z",
  };
}

// build one genuine twelve-gauge target document
function fixedGaugeTargetBytes() {
  const built = buildRainFixedGaugeTargetProjection({
    captures: RAIN_COLLECTION_STATIONS.map(fixedGaugeCapture),
    sources: RAIN_COLLECTION_STATIONS.map((station, index) => ({
      sourceId: String(1_000 + index),
      stationId: station.locationId,
    })),
    targetCutoffAt: "2026-09-14T07:10:00.000Z",
    validAt: "2026-09-14T07:00:00.000Z",
  });
  assert.equal(built.state, "complete");
  return built.bytes;
}

// construct one valid single-row revision
function revisionProjection() {
  return {
    contractVersion: "adjustment-revision-projection/v1",
    family: "shared",
    logicalKey: {
      productRunAt: "2026-10-08T00:00:00.000Z",
      sourceId: "7",
      sourceKind: "forecast",
      validAt: "2026-10-08T01:00:00.000Z",
    },
    logicalReceivedAt: "2026-10-08T00:05:00.000Z",
    projectionKind: "actual_best_match",
    rows: [weatherRow()],
    source: source(),
    storedContentSha256: HASH,
  };
}

// construct every remaining v1 database projection shape
function remainingRevisionV1Projections() {
  const actual = revisionProjection();
  const target = structuredClone(actual);
  target.logicalKey.productRunAt = null;
  target.logicalKey.sourceKind = "physical_sensor";
  target.projectionKind = "target_revision";
  target.source.sourceKind = "physical_sensor";
  target.source.upstreamModel = null;
  const anchor = {
    ...structuredClone(actual),
    logicalKey: {
      contentSha256: null,
      leadHours: 24,
      providerResponseSha256: null,
      runInitializedAt: null,
      siteId: null,
      sourceId: "7",
      sourceType: "forecast_anchor",
      validAt: "2026-10-08T01:00:00.000Z",
    },
    projectionKind: "native_source",
    rows: [{
      apparentTemperatureC64: canonical.encodeMaintenanceBinary64(10),
      cloudCoverPercent64: canonical.encodeMaintenanceBinary64(50),
      contentSha256: HASH,
      leadHours: 24,
      precipitationMm64: canonical.encodeMaintenanceBinary64(0),
      pressureHpa64: canonical.encodeMaintenanceBinary64(1_010),
      relativeHumidityPercent64: canonical.encodeMaintenanceBinary64(80),
      temperatureC64: canonical.encodeMaintenanceBinary64(11),
      validAt: "2026-10-08T01:00:00.000Z",
      windDirectionDegrees64: canonical.encodeMaintenanceBinary64(180),
      windGustMps64: canonical.encodeMaintenanceBinary64(8),
      windSpeedMps64: canonical.encodeMaintenanceBinary64(5),
    }],
  };
  const temperature = {
    ...structuredClone(actual),
    family: "temperature",
    logicalKey: {
      contentSha256: HASH,
      leadHours: null,
      providerResponseSha256: SECOND_HASH,
      runInitializedAt: "2026-10-08T00:00:00.000Z",
      siteId: "3",
      sourceId: null,
      sourceType: "ecmwf_temperature_run",
      validAt: null,
    },
    projectionKind: "native_source",
    rows: Array.from({ length: 18 }, (_, index) => ({
      bestMatchContentSha256: index < 6 ? null : SECOND_HASH,
      bestMatchProductRunAt: index < 6 ? null : "2026-10-08T00:00:00.000Z",
      bestMatchSourceId: index < 6 ? null : "7",
      bestMatchTemperatureC64: index < 6 ? null : canonical.encodeMaintenanceBinary64(10),
      contentSha256: sha256(`temperature-${index}`),
      modelCycle: "50r1",
      modelLeadHours: index + 1,
      rawRelativeHumidityPercent64: canonical.encodeMaintenanceBinary64(80),
      rawTemperatureC64: canonical.encodeMaintenanceBinary64(11),
      rawWindSpeedMps64: canonical.encodeMaintenanceBinary64(5),
      validAt: new Date(Date.parse("2026-10-08T01:00:00.000Z") + index * 3_600_000).toISOString(),
    })),
    source: {
      ...source(),
      adapterVersion: "open-meteo-ecmwf-single-run/v1",
      contractEpoch: "temperature-canary/v1",
      dataset: "single_run",
      upstreamModel: "ecmwf_ifs",
    },
  };
  const rain = {
    contractVersion: "adjustment-revision-projection/v1",
    family: "rain",
    logicalKey: {
      inputSha256: HASH,
      modelSha256: SECOND_HASH,
      runInitializedAt: "2026-10-08T00:00:00.000Z",
    },
    logicalReceivedAt: "2026-10-08T00:05:00.000Z",
    projectionKind: "rain_gate_input",
    rows: Array.from({ length: 23 }, (_, index) => ({
      applied: index % 2 === 0,
      correctedPrecipitationMm64: canonical.encodeMaintenanceBinary64(1),
      modelLeadHours: index + 1,
      rawPrecipitationMm64: canonical.encodeMaintenanceBinary64(2),
      reasonCode: "candidate_applied",
      validAt: new Date(Date.parse("2026-10-08T01:00:00.000Z") + index * 3_600_000).toISOString(),
    })),
    source: Object.fromEntries(Object.keys(source()).map((key) => [key, null])),
    storedContentSha256: HASH,
  };
  return { anchor, rain, target, temperature };
}

// construct one source-decision temperature v2 projection
function temperatureNativeV2Projection() {
  const { temperature } = remainingRevisionV1Projections();
  const recentErrorState = {
    b24C: 0.2,
    b72C: 0.1,
    cohort: "ecmwf_single_run_hindcast",
    localDates: 3,
    mad72C: 0.5,
    maximumSourceRunInitializedAt: "2026-10-07T12:00:00.000Z",
    maximumSourceValidAt: "2026-10-07T17:00:00.000Z",
    n24: 6,
    n72: 24,
    sourceKeys: Array.from({ length: 24 }, (_, index) =>
      `2026-10-07T00:00:00.000Z/${new Date(Date.parse("2026-10-07T00:00:00.000Z") +
        index * 3_600_000).toISOString()}`),
    supported: true,
    targetRunInitializedAt: temperature.logicalKey.runInitializedAt,
    windowEndValidAt: "2026-10-07T17:00:00.000Z",
  };
  return {
    ...temperature,
    contractVersion: "adjustment-temperature-native-source-projection/v2",
    recentErrorState,
    recentErrorStateSha256: sha256(JSON.stringify(recentErrorState)),
  };
}

// construct one exact rain feature v2 projection
function rainFeatureV2Projection() {
  const runInitializedAt = "2026-10-08T00:00:00.000Z";
  return {
    contractVersion: "adjustment-rain-gate-feature-projection/v2",
    family: "rain",
    logicalKey: { inputSha256: HASH, modelSha256: SECOND_HASH, runInitializedAt },
    logicalReceivedAt: "2026-10-08T08:05:00.000Z",
    projectionKind: "rain_gate_input",
    rows: Array.from({ length: 23 }, (_unused, index) => ({
      features64: Array.from({ length: 107 }, (_feature, featureIndex) =>
        featureIndex === 17 ? null : canonical.encodeMaintenanceBinary64(index + featureIndex / 100)),
      modelLeadHours: index + 9,
      rawPrecipitationMm64: canonical.encodeMaintenanceBinary64(0.5),
      rawTargetHourTemperatureC64: canonical.encodeMaintenanceBinary64(10),
      validAt: new Date(Date.parse(runInitializedAt) + (index + 9) * 3_600_000).toISOString(),
    })),
    source: {
      adapterVersion: "rain-hurdle-wind-features/v1",
      contractEpoch: "rain-prospective-capture/v1",
      dataset: "ecmwf_ifs",
      providerKey: "open-meteo-single-runs",
      sourceConfigFingerprint: canonical.ADJUSTMENT_RAIN_SOURCE_CONFIG_FINGERPRINT,
      sourceId: "forecast-claim-1",
      sourceKey: "rain-prospective-forecast",
      sourceKind: "forecast",
      upstreamModel: "ecmwf_ifs",
    },
    storedContentSha256: sha256("stored-rain-jsonb"),
  };
}

// construct one exact rain control v3 projection
function rainControlV3Projection() {
  const feature = rainFeatureV2Projection();
  const persistenceTarget = canonical.createRainMaintenancePersistenceTarget({
    decisionAt: "2026-10-08T08:00:00.000Z",
    rows: RAIN_COLLECTION_STATIONS.map((station, index) => ({
      captureMembers: [{
        bodySha256: sha256(`capture-${index}`),
        claimId: `station-${station.locationId}`,
        completedAt: "2026-10-08T07:05:00.000Z",
      }],
      precipitationMm: 0.75,
      receivedAt: "2026-10-08T07:05:00.000Z",
      stationId: station.locationId,
    })),
  });
  return {
    ...feature,
    contractVersion: "adjustment-rain-gate-control-projection/v3",
    ordinalArtifactSha256: "c".repeat(64),
    persistenceTarget,
    rows: feature.rows.map((row) => ({
      ...row,
      incumbentArtifactIdentitySha256: HASH,
      incumbentPrediction64: canonical.encodeMaintenanceBinary64(0.6),
      incumbentProbability: {
        atLeast0_1: canonical.encodeMaintenanceBinary64(0.7),
        atLeast1_0: canonical.encodeMaintenanceBinary64(0.2),
        atLeast2_5: canonical.encodeMaintenanceBinary64(0.1),
      },
      incumbentReceiptMemberSha256: SECOND_HASH,
      nativeSourceProbability: {
        atLeast0_1: canonical.encodeMaintenanceBinary64(1),
        atLeast1_0: canonical.encodeMaintenanceBinary64(0),
        atLeast2_5: canonical.encodeMaintenanceBinary64(0),
      },
      persistencePrediction64: canonical.encodeMaintenanceBinary64(0.75),
      persistenceReason: "causal_target",
      persistenceTargetMemberSha256: persistenceTarget.targetMemberSha256,
      recentVolumeScalePrediction64: canonical.encodeMaintenanceBinary64(0.8),
      sameWindowVolumeScalePrediction64: canonical.encodeMaintenanceBinary64(0.7),
      sourceRowSha256: canonical.adjustmentRainGateFeatureRowSha256(feature.logicalKey, row),
      unchangedOrdinalPrediction64: canonical.encodeMaintenanceBinary64(0.6),
      volumeScalePrediction64: canonical.encodeMaintenanceBinary64(0.625),
    })),
    stateSha256: "d".repeat(64),
    stateStageReceiptSha256: "e".repeat(64),
    storedContentSha256: "f".repeat(64),
  };
}

// construct one complete rain control state
function controlState(recentSupported) {
  const value = {
    calibrationEndAt: "2026-07-25T00:00:00.000Z",
    calibrationStartAt: "2026-04-26T00:00:00.000Z",
    contractVersion: "rain-maintenance-control-state/v1",
    epochWitnessSha256: "1".repeat(64),
    generatedAt: "2026-07-26T00:00:00.000Z",
    legacyCalibrationStartAt: "2026-06-10T00:00:00.000Z",
    legacyRawScale64: canonical.encodeMaintenanceBinary64(1.25),
    modelMonth: "2026-08",
    ordinalArtifactSha256: "2".repeat(64),
    recentFallbackReason: recentSupported ? "recent_calibration" : "same_window_volume_scale",
    recentRawScale64: canonical.encodeMaintenanceBinary64(recentSupported ? 1.5 : 1.4),
    recentSupported,
    recipeSha256: canonical.RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256,
    sameWindowRawScale64: canonical.encodeMaintenanceBinary64(1.4),
    scheduleContractSha256: "3".repeat(64),
    sourceMemberRootSha256: "4".repeat(64),
    sourceReceiptRootSha256: "5".repeat(64),
    stateSha256: "",
    support: {
      calibrationDates: recentSupported ? 90 : 1,
      calibrationHours: recentSupported ? 1500 : 1,
      calibrationRows: recentSupported ? 2000 : 1,
      calibrationWetDates: recentSupported ? 20 : 0,
      calibrationWetHours: recentSupported ? 100 : 0,
      effectiveDates64: canonical.encodeMaintenanceBinary64(recentSupported ? 40 : 1),
      effectiveWetDates64: canonical.encodeMaintenanceBinary64(recentSupported ? 5 : 0),
      legacyCalibrationRows: recentSupported ? 1000 : 1,
      legacyTrainingRows: recentSupported ? 5000 : 1,
      legacyTrainingWetRows: recentSupported ? 500 : 0,
      trainingDates: recentSupported ? 300 : 1,
      trainingHours: recentSupported ? 5000 : 1,
      trainingRows: recentSupported ? 6000 : 1,
      trainingWetDates: recentSupported ? 50 : 0,
      trainingWetHours: recentSupported ? 500 : 0,
    },
    trainingMaximumValidAt: "2026-04-18T23:00:00.000Z",
  };
  value.stateSha256 = canonical.canonicalObjectSha256(value, "stateSha256");
  return value;
}

// compare one parser's accepted value and rejection behavior
function assertParserParity(publicParser, canonicalParser, validBytes, tamperedBytes) {
  assert.deepEqual(publicParser(validBytes), canonicalParser(validBytes));
  assert.throws(() => canonicalParser(tamperedBytes));
  assert.throws(() => publicParser(tamperedBytes));
}

test("public maintenance contract has a closed builtin-only boundary", async () => {
  assert.deepEqual(Object.keys(publicContract).sort(), expectedExports);
  const bytes = await readFile(new URL("../src/adjustment-maintenance-contract.mjs", import.meta.url));
  const text = bytes.toString("utf8");
  const imports = [...text.matchAll(/^import .* from ["']([^"']+)["'];$/gmu)].map((match) => match[1]);
  assert.ok(imports.length > 0);
  assert.deepEqual([...new Set(imports)].sort(), ["node:crypto", "node:util"]);
  assert.doesNotMatch(
    text,
    /RAIN_HURDLE_WIND_ARTIFACT_JSON|"heads":\{"0\.1"|function (?:fit|predict|evaluate)|process\.env|homedir|MODEL_EVIDENCE_ROOT|HOLDOUT_LEDGER_PATH/u,
  );
  assert.ok(bytes.byteLength < 256 * 1024);
});

test("public maintenance contract source provenance is explicit", async () => {
  const bytes = await readFile(new URL("../src/adjustment-maintenance-contract.mjs", import.meta.url));
  const sourceMembers = [...bytes.toString("utf8").matchAll(/^\/\/ (packages\/[^\r\n]+)$/gmu)]
    .map((match) => match[1]);
  assert.deepEqual([...new Set(sourceMembers)].sort(), Object.keys(canonicalSourceHashes).sort());
  for (const [path, expected] of Object.entries(canonicalSourceHashes)) {
    assert.equal(sha256(await readFile(resolve(root, path))), expected, path);
  }
});

test("revision parsers retain canonical v1 and grouped v2 behavior", () => {
  const single = revisionProjection();
  const singleBytes = canonical.encodeAdjustmentRevisionProjection(single);
  assertParserParity(
    publicContract.parseAdjustmentRevisionProjectionDocument,
    canonical.parseAdjustmentRevisionProjectionDocument,
    singleBytes,
    Buffer.concat([singleBytes, Buffer.from("\n")]),
  );
  const grouped = {
    ...single,
    contractVersion: "adjustment-revision-batch-projection/v2",
    family: "wind",
    rows: [weatherRow(), weatherRow("2026-10-08T02:00:00.000Z")],
  };
  const groupedBytes = canonical.encodeAdjustmentRevisionBatchProjection(grouped);
  assertParserParity(
    publicContract.parseAdjustmentRevisionProjectionDocument,
    canonical.parseAdjustmentRevisionProjectionDocument,
    groupedBytes,
    Buffer.from(groupedBytes.toString("utf8").replace('"family":"wind"', '"family":"rain"')),
  );
});

test("revision dispatcher retains every canonical projection shape", () => {
  const { anchor, rain, target, temperature } = remainingRevisionV1Projections();
  const v1Cases = [
    {
      name: "target_revision",
      tampered: { ...target, rows: [{ ...target.rows[0], contentSha256: SECOND_HASH }] },
      value: target,
    },
    {
      name: "forecast_anchor",
      tampered: { ...anchor, rows: [{ ...anchor.rows[0], contentSha256: SECOND_HASH }] },
      value: anchor,
    },
    {
      name: "temperature_native",
      tampered: {
        ...temperature,
        rows: temperature.rows.map((row, index) => index === 6
          ? { ...row, bestMatchSourceId: null }
          : row),
      },
      value: temperature,
    },
    {
      name: "rain_gate",
      tampered: {
        ...rain,
        source: { ...rain.source, providerKey: "open-meteo" },
      },
      value: rain,
    },
  ];
  // compare every distinct v1 logical-key and row grammar
  for (const { name, tampered, value } of v1Cases) {
    const bytes = canonical.encodeAdjustmentRevisionProjection(value);
    assert.deepEqual(
      publicContract.parseAdjustmentRevisionProjectionDocument(bytes),
      canonical.parseAdjustmentRevisionProjectionDocument(bytes),
    );
    const tamperedBytes = Buffer.from(canonical.canonicalJsonBytes(tampered));
    assert.throws(() => canonical.parseAdjustmentRevisionProjectionDocument(tamperedBytes), name);
    assert.throws(() => publicContract.parseAdjustmentRevisionProjectionDocument(tamperedBytes), name);
  }

  const groupedTarget = {
    ...target,
    contractVersion: "adjustment-revision-batch-projection/v2",
    family: "shared",
    rows: [
      { ...target.rows[0], contentSha256: sha256("target-1") },
      { ...target.rows[0], contentSha256: sha256("target-2"), validAt: "2026-10-08T02:00:00.000Z" },
    ],
    storedContentSha256: sha256("target-1"),
  };
  const groupedTargetBytes = canonical.encodeAdjustmentRevisionBatchProjection(groupedTarget);
  assert.deepEqual(
    publicContract.parseAdjustmentRevisionProjectionDocument(groupedTargetBytes),
    canonical.parseAdjustmentRevisionProjectionDocument(groupedTargetBytes),
  );
  const groupedTargetTamper = {
    ...groupedTarget,
    logicalKey: { ...groupedTarget.logicalKey, productRunAt: "2026-10-08T00:00:00.000Z" },
  };
  const groupedTargetTamperBytes = Buffer.from(canonical.canonicalJsonBytes(groupedTargetTamper));
  assert.throws(() => canonical.parseAdjustmentRevisionProjectionDocument(groupedTargetTamperBytes));
  assert.throws(() => publicContract.parseAdjustmentRevisionProjectionDocument(groupedTargetTamperBytes));

  const temperatureV2 = temperatureNativeV2Projection();
  const temperatureV2Bytes = canonical.encodeAdjustmentTemperatureNativeSourceProjection(temperatureV2);
  assert.deepEqual(
    publicContract.parseAdjustmentRevisionProjectionDocument(temperatureV2Bytes),
    canonical.parseAdjustmentRevisionProjectionDocument(temperatureV2Bytes),
  );
  const changedState = {
    ...temperatureV2.recentErrorState,
    targetRunInitializedAt: "2026-10-09T00:00:00.000Z",
  };
  const temperatureV2Tamper = {
    ...temperatureV2,
    recentErrorState: changedState,
    recentErrorStateSha256: sha256(JSON.stringify(changedState)),
  };
  const temperatureV2TamperBytes = Buffer.from(canonical.canonicalJsonBytes(temperatureV2Tamper));
  assert.throws(() => canonical.parseAdjustmentRevisionProjectionDocument(temperatureV2TamperBytes));
  assert.throws(() => publicContract.parseAdjustmentRevisionProjectionDocument(temperatureV2TamperBytes));

  const featureV2 = rainFeatureV2Projection();
  const featureV2Bytes = canonical.encodeAdjustmentRainGateFeatureProjection(featureV2);
  assert.deepEqual(
    publicContract.parseAdjustmentRevisionProjectionDocument(featureV2Bytes),
    canonical.parseAdjustmentRevisionProjectionDocument(featureV2Bytes),
  );
  const featureV2Tamper = {
    ...featureV2,
    source: { ...featureV2.source, sourceConfigFingerprint: HASH },
  };
  const featureV2TamperBytes = Buffer.from(canonical.canonicalJsonBytes(featureV2Tamper));
  assert.throws(() => canonical.parseAdjustmentRevisionProjectionDocument(featureV2TamperBytes));
  assert.throws(() => publicContract.parseAdjustmentRevisionProjectionDocument(featureV2TamperBytes));

  const controlV3 = rainControlV3Projection();
  const controlV3Bytes = canonical.encodeAdjustmentRainGateControlProjection(controlV3);
  assert.deepEqual(
    publicContract.parseAdjustmentRevisionProjectionDocument(controlV3Bytes),
    canonical.parseAdjustmentRevisionProjectionDocument(controlV3Bytes),
  );
  const controlV3Tamper = {
    ...controlV3,
    rows: controlV3.rows.map((row, index) => index === 0
      ? {
          ...row,
          nativeSourceProbability: {
            ...row.nativeSourceProbability,
            atLeast1_0: canonical.encodeMaintenanceBinary64(1),
          },
        }
      : row),
  };
  const controlV3TamperBytes = Buffer.from(canonical.canonicalJsonBytes(controlV3Tamper));
  assert.throws(() => canonical.parseAdjustmentRevisionProjectionDocument(controlV3TamperBytes));
  assert.throws(() => publicContract.parseAdjustmentRevisionProjectionDocument(controlV3TamperBytes));
});

test("rain control parser matches supported and unsupported canonical states", () => {
  for (const recentSupported of [true, false]) {
    const bytes = canonical.encodeRainMaintenanceControlState(controlState(recentSupported));
    assertParserParity(
      publicContract.parseRainMaintenanceControlState,
      canonical.parseRainMaintenanceControlState,
      bytes,
      Buffer.concat([bytes, Buffer.from("\n")]),
    );
  }
});

test("fixed-gauge parser retains canonical raw-capture replay", () => {
  const bytes = fixedGaugeTargetBytes();
  assert.deepEqual(
    publicContract.parseAdjustmentRainFixedGaugeTargetProjection(bytes),
    canonical.parseAdjustmentRainFixedGaugeTargetProjection(bytes),
  );
  const tampered = JSON.parse(bytes.toString("utf8"));
  tampered.rows[0].normalizedRecord.metrics.precipitationMm += 1;
  tampered.rows[0].storedContentSha256 = sha256(
    weatherRecordContent(tampered.rows[0].normalizedRecord),
  );
  const tamperedBytes = Buffer.from(canonical.canonicalJsonBytes(tampered));
  assert.throws(() => canonical.parseAdjustmentRainFixedGaugeTargetProjection(tamperedBytes));
  assert.throws(() => publicContract.parseAdjustmentRainFixedGaugeTargetProjection(tamperedBytes));
});

// construct exact family source and value bytes
function shadowFixture(family) {
  const issuedAt = "9999-11-01T18:35:00.000Z";
  const rowCount = { rain: 23, temperature: 12, wind: 168 }[family];
  const rows = Array.from({ length: rowCount }, (_, index) => {
    const leadHours = index + 1;
    const validAt = new Date(Date.parse(issuedAt) + leadHours * 3_600_000).toISOString();
    const common = {
      adapterVersion: "open-meteo/v4", contentSha256: sha256(`${family}-${index}`),
      contractEpoch: "forecast/v4", dataset: "best_match", leadHours, modelLeadHours: leadHours + 6,
      providerKey: "open-meteo", receivedAt: issuedAt, referenceAt: "9999-11-01T12:35:00.000Z",
      revisionCount: 0, sourceConfigFingerprint: "open-meteo-forecast/v4", sourceKey: "open-meteo-forecast",
      sourceSha256: HASH, upstreamModel: "best_match", validAt,
    };
    // retain each family-specific native row
    if (family === "temperature") {
      return { adapterVersion: "open-meteo-ecmwf-single-run/v1", bestMatchContentSha256: HASH,
        bestMatchProductRunAt: common.referenceAt, bestMatchSourceId: "7",
        bestMatchTemperatureC64: canonical.encodeMaintenanceBinary64(13), contentSha256: common.contentSha256,
        dataset: "single_run", leadHours, modelCycle: "50r1", modelLeadHours: leadHours + 6,
        providerKey: "open-meteo", providerResponseSha256: HASH, rawRelativeHumidityPercent64: canonical.encodeMaintenanceBinary64(80),
        rawTemperatureC64: canonical.encodeMaintenanceBinary64(12), rawWindSpeedMps64: canonical.encodeMaintenanceBinary64(5),
        receivedAt: issuedAt, referenceAt: common.referenceAt, sourceSha256: HASH, upstreamModel: "ecmwf_ifs", validAt };
    }
    // retain wind native metrics
    if (family === "wind") {
      return { ...common, windGustMps64: canonical.encodeMaintenanceBinary64(12),
        windSpeedMps64: canonical.encodeMaintenanceBinary64(8) };
    }
    return { ...common, precipitationMm64: canonical.encodeMaintenanceBinary64(2) };
  });
  const sourceValue = { contractVersion: "adjustment-shadow-source-projection/v1", family,
    registrationSha256: HASH, candidateSha256: HASH, sourceSha256: HASH, dueKey: `capture/${issuedAt}`,
    issuedAt, rowCount, rows };
  // retain the exact temperature causal state
  if (family === "temperature") {
    sourceValue.recentErrorState = { b24C: 0.2, b72C: 0.1, cohort: "ecmwf_single_run_hindcast", localDates: 3,
      mad72C: 0.5, maximumSourceRunInitializedAt: "9999-11-01T06:00:00.000Z",
      maximumSourceValidAt: "9999-11-01T17:00:00.000Z", n24: 12, n72: 36, sourceKeys: ["source-1"],
      supported: true, targetRunInitializedAt: "9999-11-01T12:35:00.000Z",
      windowEndValidAt: "9999-11-01T17:00:00.000Z" };
  }
  // retain the exact rain causal graph
  if (family === "rain") {
    sourceValue.causalInputs = { contractVersion: "adjustment-shadow-rain-causal-inputs/v1",
      captureSet: [{ bodySha256: HASH, claimId: "rain-current", completedAt: issuedAt, kind: "forecast",
        runInitializedAt: "9999-11-01T12:35:00.000Z", stationId: null, windowEndExclusive: null, windowStart: null }],
      currentRun: { completedAt: issuedAt, contentSha256: HASH,
        hours: Array.from({ length: 48 }, (_, index) => ({ cloudCoverPercent64: canonical.encodeMaintenanceBinary64(50),
          leadHours: index + 1, precipitationMm64: canonical.encodeMaintenanceBinary64(2),
          pressureHpa64: canonical.encodeMaintenanceBinary64(1000), relativeHumidityPercent64: canonical.encodeMaintenanceBinary64(80),
          temperatureC64: canonical.encodeMaintenanceBinary64(12), windDirectionDegrees64: canonical.encodeMaintenanceBinary64(180),
          windSpeedMps64: canonical.encodeMaintenanceBinary64(5) })), runInitializedAt: "9999-11-01T12:35:00.000Z" },
      priorRuns: [], stationHours: [] };
  }
  const sourceBytes = canonical.encodeMaintenanceShadowSourceProjection(sourceValue);
  const identity = canonical.createMaintenanceShadowSourceIdentity(sourceBytes);
  const bodyRows = rows.map((row, index) => {
    const common = { leadHours: index + 1, sourceRowSha256: identity.sourceRowSha256[index], validAt: row.validAt };
    // retain the exact family body shape
    if (family === "temperature") return { ...common, candidateTemperatureC64: canonical.encodeMaintenanceBinary64(10), fallbackCode: "none", wouldApply: true };
    if (family === "wind") return { ...common, candidateGustMps64: index >= 48 && index < 72 ? null : canonical.encodeMaintenanceBinary64(12), candidateSpeedMps64: canonical.encodeMaintenanceBinary64(8), gustWouldApply: index < 48 || index >= 72, speedWouldApply: true };
    return { ...common, atLeast1_0Probability64: canonical.encodeMaintenanceBinary64(.4), atLeast2_5Probability64: canonical.encodeMaintenanceBinary64(.2), candidatePrecipitationMm64: canonical.encodeMaintenanceBinary64(2), fallbackCode: "none", occurrenceProbability64: canonical.encodeMaintenanceBinary64(.6), positiveAmountMm64: canonical.encodeMaintenanceBinary64(2), wouldApply: true };
  });
  const bodyValue = { candidateSha256: HASH, contractVersion: "adjustment-shadow-prediction-values/v3",
    dueKey: sourceValue.dueKey, family, inputSha256: identity.inputSha256, issuedAt, registrationSha256: HASH,
    rowCount, rows: bodyRows, sourceReceiptSha256: identity.sourceReceiptSha256, sourceSha256: HASH };
  return { bodyBytes: canonical.encodeMaintenanceShadowValues(bodyValue, sourceBytes), bodyValue, sourceBytes, sourceValue };
}

// build one raw-policy comparator bound to the exact shadow members
function boundComparatorFixture(family) {
  const fixture = shadowFixture(family);
  const rawReceipt = {
    activePackage: null,
    contractVersion: `forecast-adjustment-${family}-maintenance-registry/v1`,
    rawReason: "policy_raw",
    siteKey: "ballydidean",
  };
  const servingAuthority = canonical.createMaintenanceShadowServingAuthority({
    artifactBytes: null,
    artifactIdentitySha256: null,
    authorityKind: "policy_raw",
    family,
    receiptBytes: canonical.canonicalJsonBytes(rawReceipt),
  });
  const identity = canonical.createMaintenanceShadowSourceIdentity(fixture.sourceBytes);
  const rows = fixture.sourceValue.rows.map((row, index) => {
    const common = {
      validAt: row.validAt,
      leadHours: index + 1,
      sourceRowSha256: identity.sourceRowSha256[index],
    };
    // retain each family-specific incumbent result shape
    if (family === "temperature") {
      return {
        ...common,
        incumbentTemperatureC64: canonical.encodeMaintenanceBinary64(10),
        applied: false,
        reasonCode: "policy_raw",
      };
    }
    // retain both wind decision channels
    if (family === "wind") {
      return {
        ...common,
        incumbentSpeedMps64: canonical.encodeMaintenanceBinary64(5),
        incumbentGustMps64: canonical.encodeMaintenanceBinary64(8),
        speedApplied: false,
        gustApplied: false,
        reasonCode: "policy_raw",
      };
    }
    return {
      ...common,
      incumbentPrecipitationMm64: canonical.encodeMaintenanceBinary64(0),
      occurrenceProbability64: canonical.encodeMaintenanceBinary64(0),
      atLeast1_0Probability64: canonical.encodeMaintenanceBinary64(0),
      atLeast2_5Probability64: canonical.encodeMaintenanceBinary64(0),
      positiveAmountMm64: canonical.encodeMaintenanceBinary64(0),
      applied: false,
      reasonCode: "policy_raw",
    };
  });
  const comparator = {
    contractVersion: "adjustment-shadow-incumbent-comparator/v1",
    family,
    registrationSha256: fixture.sourceValue.registrationSha256,
    candidateSha256: fixture.sourceValue.candidateSha256,
    sourceSha256: fixture.sourceValue.sourceSha256,
    dueKey: fixture.sourceValue.dueKey,
    issuedAt: fixture.sourceValue.issuedAt,
    sourceProjectionSha256: sha256(fixture.sourceBytes),
    predictionBodySha256: sha256(fixture.bodyBytes),
    servingAuthority,
    rowCount: rows.length,
    rows,
  };
  const bytes = canonical.encodeMaintenanceShadowComparator(
    comparator,
    fixture.sourceBytes,
    fixture.bodyBytes,
  );
  return { ...fixture, bytes, comparator };
}

test("shadow metadata and source/value parsers remain byte-identical", () => {
  // compare every family grammar
  for (const family of ["temperature", "wind", "rain"]) {
    const fixture = shadowFixture(family);
    assert.deepEqual(publicContract.parseMaintenanceShadowSourceProjection(fixture.sourceBytes), fixture.sourceValue);
    assert.deepEqual(publicContract.parseMaintenanceShadowValues(fixture.bodyBytes), fixture.bodyValue);
    assert.deepEqual(publicContract.createMaintenanceShadowPredictionMetadata(fixture.bodyBytes, fixture.sourceBytes),
      canonical.createMaintenanceShadowPredictionMetadata(fixture.bodyBytes, fixture.sourceBytes));
    assert.throws(() => publicContract.parseMaintenanceShadowValues(Buffer.concat([fixture.bodyBytes, Buffer.from("\n")])));
  }
});

test("shadow comparator binding remains exact for every family", () => {
  // validate the full source/body relationship for every family
  for (const family of ["temperature", "wind", "rain"]) {
    const fixture = boundComparatorFixture(family);
    const canonicalComparator = canonical.parseMaintenanceShadowComparator(fixture.bytes);
    const publicComparator = publicContract.parseMaintenanceShadowComparator(fixture.bytes);
    canonical.validateMaintenanceShadowComparatorBinding(
      canonicalComparator,
      fixture.sourceBytes,
      fixture.bodyBytes,
    );
    publicContract.validateMaintenanceShadowComparatorBinding(
      publicComparator,
      fixture.sourceBytes,
      fixture.bodyBytes,
    );

    const tampered = { ...canonicalComparator, sourceProjectionSha256: SECOND_HASH };
    const tamperedBytes = Buffer.from(JSON.stringify(tampered) + "\n");
    const parsedCanonicalTamper = canonical.parseMaintenanceShadowComparator(tamperedBytes);
    const parsedPublicTamper = publicContract.parseMaintenanceShadowComparator(tamperedBytes);
    assert.throws(() => canonical.validateMaintenanceShadowComparatorBinding(
      parsedCanonicalTamper,
      fixture.sourceBytes,
      fixture.bodyBytes,
    ));
    assert.throws(() => publicContract.validateMaintenanceShadowComparatorBinding(
      parsedPublicTamper,
      fixture.sourceBytes,
      fixture.bodyBytes,
    ));
  }
});
