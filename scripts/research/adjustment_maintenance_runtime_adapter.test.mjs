import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { access, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

import { ADJUSTMENT_FIT_RUNTIME_GENERATED_FILES } from "./adjustment_maintenance_runtime_manifest.mjs";

import {
  ADJUSTMENT_MAINTENANCE_RUNTIME_ADAPTER_GENERATED_FILES,
  ADJUSTMENT_MAINTENANCE_RUNTIME_ADAPTER_SOURCE_FILES,
  buildForecastAdjustmentMaintenanceFitParityInputs,
  buildForecastAdjustmentMaintenanceParityInputs,
  buildForecastAdjustmentMaintenancePortableCandidate,
  evaluateForecastAdjustmentMaintenanceNativeCandidate,
  evaluateForecastAdjustmentMaintenancePackagedCandidate,
  evaluateForecastAdjustmentMaintenancePolicy,
  forecastAdjustmentRainMaintenanceSourceIdentitySha256,
  forecastAdjustmentTemperatureMaintenanceSourceIdentitySha256,
  forecastAdjustmentWindMaintenanceSourceIdentitySha256,
  loadInstalledForecastAdjustmentMaintenanceShadowCandidate,
  parseForecastAdjustmentMaintenanceShadowCapsule,
  projectForecastAdjustmentMaintenanceShadowNativeSource,
  projectForecastAdjustmentMaintenanceTemperatureTargets,
} from "./adjustment_maintenance_runtime_adapter.mjs";

import {
  FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
  FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1,
  canonicalJsonBytes,
  canonicalSha256,
  createMaintenanceShadowServingAuthority,
  createMaintenanceShadowPredictionMetadata,
  createMaintenanceShadowSourceIdentity,
  createDevelopmentReport,
  createForecastAdjustmentCandidate,
  encodeMaintenanceBinary64,
  encodeAdjustmentRevisionProjection,
  encodeMaintenanceShadowSourceProjection,
  encodeMaintenanceShadowComparator,
  encodeMaintenanceShadowValues,
  evaluateDevelopmentLosoFold,
  forecastAdjustmentRainMaintenanceSourceIdentitySha256 as packageRainSourceIdentity,
  forecastAdjustmentTemperatureMaintenanceSourceIdentitySha256 as packageTemperatureSourceIdentity,
  runtimeCalendarFingerprint,
  MAINTENANCE_SHADOW_SOURCE_VERSION,
  MAINTENANCE_SHADOW_VALUES_VERSION,
} from "../../packages/forecast-adjustment/dist/index.js";
import {
  RAIN_HURDLE_WIND_ARTIFACT_JSON,
  RAIN_HURDLE_WIND_ARTIFACT_SHA256,
} from "../../packages/forecast-adjustment/dist/rain-hurdle-wind-artifact.js";
import {
  FORECAST_OBSERVATION_SOURCE_LINEAGES,
} from "../../packages/domain/dist/index.js";

// hash exact fixture bytes
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

const LEAD_BANDS = Object.freeze([
  "001-024", "025-048", "049-072", "073-096", "097-120", "121-144", "145-168",
]);
const INVALID_FIT_BYTES = Buffer.from("{}\n");

// build one genuine selected temperature fit from the retained permanent model
async function temperatureFitBytes() {
  const incumbent = JSON.parse(await readFile(
    new URL("../../config/forecast-adjustments/ballydidean/temperature-canary-bundles/" +
      "sha256-4d4e229b42823e53d2db062ec18c625bb2d2378a8a46d641fa95fabb59501b0e.json", import.meta.url),
    "utf8",
  ));
  return Buffer.from(canonicalJsonBytes({
    arms: {},
    confirmationOpened: false,
    contractVersion: "temperature-maintenance-fit/v2",
    developmentIncumbentMae: 1,
    developmentRawMae: 2,
    dueMonth: "2026-10",
    model: incumbent.model,
    selectedArm: "month_start_expanding",
    servingChanged: false,
    state: "development_candidate",
  }));
}

// create one complete causal temperature evaluator input
function temperatureInput() {
  return {
    evaluatedAt: "2026-10-08T06:10:00.000Z",
    rawBestMatchTemperatureC: 17.2,
    recentErrorState: {
      b24C: 0.8,
      b72C: 0.3,
      cohort: "ecmwf_single_run_hindcast",
      localDates: 3,
      mad72C: 0.4,
      maximumSourceRunInitializedAt: "2026-10-07T00:00:00.000Z",
      maximumSourceValidAt: "2026-10-07T17:00:00.000Z",
      n24: 12,
      n72: 50,
      sourceKeys: Array.from({ length: 50 }, (_unused, index) => `source-${index}`),
      supported: true,
      targetRunInitializedAt: "2026-10-08T00:00:00.000Z",
      windowEndValidAt: "2026-10-07T17:00:00.000Z",
    },
    sourceForecast: {
      adapterVersion: "open-meteo-ecmwf-single-run/v1",
      dataset: "single_run",
      firstReceivedAt: "2026-10-08T06:05:00.000Z",
      modelCycle: "50r1",
      modelLeadHours: 7,
      providerKey: "open-meteo",
      providerResponseSha256: "e".repeat(64),
      rawRelativeHumidityPercent: 78,
      rawTemperatureC: 16.4,
      rawWindSpeedMps: 3.2,
      runInitializedAt: "2026-10-08T00:00:00.000Z",
      upstreamModel: "ecmwf_ifs",
      validAt: "2026-10-08T07:00:00.000Z",
    },
    validAt: "2026-10-08T07:00:00.000Z",
  };
}

// create the exact thirteen supported wind metric bands
function windPairs() {
  return [
    ...LEAD_BANDS.map((leadBand) => ({ leadBand, metric: "windSpeedMps" })),
    ...LEAD_BANDS.filter(
      // omit the unsupported middle gust band
      (leadBand) => leadBand !== "049-072",
    ).map((leadBand) => ({ leadBand, metric: "windGustMps" })),
  ];
}

// create one verified selected wind fit
function windFitBytes() {
  const pairs = windPairs();
  const stationScores = [
    ["ambient-maxweather", "ambient"],
    ["ballydidean-ecowitt", "ecowitt"],
    ["netatmo-nearby", "netatmo"],
    ["tempest-126537", "tempest"],
    ["tempest-168853", "tempest"],
  ].map(([physicalStationKey, providerFamily]) => ({
    adjustedLoss: 9,
    eventCount: 100,
    physicalStationKey,
    pointSkill: 0.1,
    providerFamily,
    rawLoss: 10,
    remainingNetworkScoreEvents: 100,
    scoreMatches: 100,
    trainingMatches: 500,
  }));
  const developmentReport = createDevelopmentReport({
    enabledMetricBands: pairs,
    folds: pairs.flatMap(
      // retain all five independent station folds per metric band
      (metricBand) => [1, 2, 3, 4, 5].map((fold) => evaluateDevelopmentLosoFold({
        auxiliaryModelSha256s: ["1", "2", "3", "4", "5"].map((value) => value.repeat(64)),
        bootstrapLowerBound: 0.01,
        fold,
        materialHarmSliceKeys: [],
        metricBand,
        stationScores,
      })),
    ),
  });
  const candidate = createForecastAdjustmentCandidate({
    coefficients: pairs.map((pair) => ({
      coefficient: 0.25,
      daypart: null,
      effectiveEventCount: 200,
      leadBand: pair.leadBand,
      level: 1,
      metric: pair.metric,
      month: null,
      season: null,
    })),
    developmentReportSha256: developmentReport.developmentReportSha256,
    enabledMetricBands: pairs,
    evaluationEpochId: "maintenance-2026-10",
    exportManifestSha256: canonicalSha256({ contractVersion: "fixture/v1" }),
    finalTrainingCutoff: "2026-09-01T06:59:59.999Z",
    forecastIdentity: FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
    runtimeFingerprint: runtimeCalendarFingerprint(),
    trainingEnvelopes: pairs.map((pair) => ({
      leadBand: pair.leadBand,
      maximum: 40,
      metric: pair.metric,
      minimum: 0,
    })),
    trainingProvenance: FORECAST_ADJUSTMENT_CANONICAL_TRAINING_PROVENANCE_V1,
  });
  return Buffer.from(canonicalJsonBytes({
    candidate,
    confirmationOpened: false,
    contractVersion: "wind-maintenance-fit/v2",
    developmentReport,
    dueMonth: "2026-10",
    state: "development_candidate",
  }));
}

// create one live Best Match wind input
function windInput() {
  const metrics = Object.fromEntries([
    "apparentTemperatureC", "blackGlobeTemperatureC", "cloudCoverPercent", "pm25MicrogramsPerCubicMeter",
    "precipitationMm", "precipitationRateMmPerHour", "pressureHpa", "relativeHumidityPercent",
    "soilElectricalConductivityMicrosiemensPerCm", "soilMoisturePercent", "solarRadiationWm2", "temperatureC",
    "uvIndex", "waterLevelM", "wetBulbGlobeTemperatureC", "windDirectionDegrees", "windGustMps", "windSpeedMps",
  ].map((metric) => [metric, null]));
  metrics.windGustMps = 8;
  metrics.windSpeedMps = 5;
  return {
    evaluatedAt: "2026-10-08T00:00:00.000Z",
    metrics,
    rawForecastProvenance: {
      ...FORECAST_ADJUSTMENT_CANONICAL_FORECAST_IDENTITY_V1,
      referenceAt: "2026-10-08T00:00:00.000Z",
      targetLeadHours: 1,
      validAt: "2026-10-08T01:00:00.000Z",
    },
  };
}

// construct one selected rain fit from the retained portable artifact
async function rainFitBytes({
  contractVersion = "rain-maintenance-fit/v2",
  projectionId = "retained-native-parity",
} = {}) {
  const source = await readFile(
    new URL("../../packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts", import.meta.url),
    "utf8",
  );
  const match = source.match(/RAIN_HURDLE_WIND_ARTIFACT_JSON = (.*) as const;\n$/su);

  // require the generated retained artifact assignment
  if (match === null) {
    throw new TypeError("retained rain artifact source is invalid");
  }
  const runtime = JSON.parse(JSON.parse(match[1]));
  const categories = Object.fromEntries(runtime.categoryScales.map(
    // restore only the serving category scale projection
    (scale, index) => [String(index + 1), { scale }],
  ));
  return Buffer.from(canonicalJsonBytes({
    artifact: {
      calibration: {
        categories,
        contractVersion: "rain-hurdle-calibration/v1",
        rules: runtime.rules,
      },
      contractVersion: "rain-maintenance-artifact/v2",
      featureNames: runtime.featureNames,
      heads: runtime.heads,
      modelMonth: runtime.modelMonth,
      projectionId,
    },
    confirmationOpened: false,
    contractVersion,
    reason: "development_gate_passer",
    state: "development_candidate",
  }));
}

// construct one complete rain source projection with exact binary64 causal rows
function rainSourceProjection() {
  const hash = "f".repeat(64);
  const runInitializedAt = "9999-11-01T12:00:00.000Z";
  const completedAt = "9999-11-01T18:01:00.000Z";
  const issuedAt = "9999-11-01T20:35:00.000Z";
  const contentSha256 = createHash("sha256").update("rain-current").digest("hex");
  const hours = Array.from({ length: 48 }, (_unused, index) => ({
    cloudCoverPercent64: encodeMaintenanceBinary64(50),
    leadHours: index + 1,
    precipitationMm64: encodeMaintenanceBinary64(2),
    pressureHpa64: encodeMaintenanceBinary64(1_000),
    relativeHumidityPercent64: encodeMaintenanceBinary64(80),
    temperatureC64: encodeMaintenanceBinary64(12),
    windDirectionDegrees64: encodeMaintenanceBinary64(180),
    windSpeedMps64: encodeMaintenanceBinary64(5),
  }));
  return {
    candidateSha256: hash,
    causalInputs: {
      captureSet: [{ bodySha256: contentSha256, claimId: "rain-current", completedAt,
        kind: "forecast", runInitializedAt, stationId: null, windowEndExclusive: null,
        windowStart: null }],
      contractVersion: "adjustment-shadow-rain-causal-inputs/v1",
      currentRun: { completedAt, contentSha256, hours, runInitializedAt },
      priorRuns: [],
      stationHours: [],
    },
    contractVersion: MAINTENANCE_SHADOW_SOURCE_VERSION,
    dueKey: "capture/9999-11-01T18:35:00.000Z",
    family: "rain",
    issuedAt,
    registrationSha256: hash,
    rowCount: 23,
    rows: Array.from({ length: 23 }, (_unused, index) => {
      const modelLeadHours = index + 9;
      return {
        adapterVersion: "open-meteo-rain-capture/v1",
        contentSha256,
        contractEpoch: "rain-prospective-capture/v1",
        dataset: "ecmwf_ifs",
        leadHours: index + 1,
        modelLeadHours,
        precipitationMm64: encodeMaintenanceBinary64(2),
        providerKey: "open-meteo-single-runs",
        receivedAt: completedAt,
        referenceAt: runInitializedAt,
        revisionCount: 0,
        sourceConfigFingerprint: createHash("sha256")
          .update("rain-prospective-open-meteo/v1\necmwf_ifs\n49\n").digest("hex"),
        sourceKey: "rain-prospective-forecast",
        sourceSha256: hash,
        upstreamModel: "ecmwf_ifs",
        validAt: new Date(Date.parse(runInitializedAt) + modelLeadHours * 3_600_000).toISOString(),
      };
    }),
    sourceSha256: hash,
  };
}

// build one exact capsule around a validated rain source and body
function rainCapsuleBytes(version = "v1") {
  const source = rainSourceProjection();
  const sourceBytes = encodeMaintenanceShadowSourceProjection(source);
  const identity = createMaintenanceShadowSourceIdentity(sourceBytes);
  const body = {
    candidateSha256: source.candidateSha256,
    contractVersion: MAINTENANCE_SHADOW_VALUES_VERSION,
    dueKey: source.dueKey,
    family: "rain",
    inputSha256: identity.inputSha256,
    issuedAt: source.issuedAt,
    registrationSha256: source.registrationSha256,
    rowCount: source.rowCount,
    rows: source.rows.map((row, index) => ({
      atLeast1_0Probability64: encodeMaintenanceBinary64(0.4),
      atLeast2_5Probability64: encodeMaintenanceBinary64(0.2),
      candidatePrecipitationMm64: encodeMaintenanceBinary64(2),
      fallbackCode: "none",
      leadHours: row.leadHours,
      occurrenceProbability64: encodeMaintenanceBinary64(0.6),
      positiveAmountMm64: encodeMaintenanceBinary64(2),
      sourceRowSha256: identity.sourceRowSha256[index],
      validAt: row.validAt,
      wouldApply: true,
    })),
    sourceReceiptSha256: identity.sourceReceiptSha256,
    sourceSha256: source.sourceSha256,
  };
  const bodyBytes = encodeMaintenanceShadowValues(body, sourceBytes);
  const metadata = createMaintenanceShadowPredictionMetadata(bodyBytes, sourceBytes);
  const comparatorAuthority = createMaintenanceShadowServingAuthority({
    artifactBytes: Buffer.from(RAIN_HURDLE_WIND_ARTIFACT_JSON),
    artifactIdentitySha256: RAIN_HURDLE_WIND_ARTIFACT_SHA256,
    authorityKind: "legacy_active",
    family: "rain",
    receiptBytes: Buffer.from(canonicalJsonBytes({
      activeArtifact: { artifactSha256: RAIN_HURDLE_WIND_ARTIFACT_SHA256 },
      contractVersion: "forecast-adjustment-rain-runtime-registry/v1",
      rawReason: null,
      siteKey: "ballydidean",
    })),
  });
  const comparatorBytes = version === "v2"
    ? encodeMaintenanceShadowComparator({
        contractVersion: "adjustment-shadow-incumbent-comparator/v1",
        family: "rain",
        registrationSha256: source.registrationSha256,
        candidateSha256: source.candidateSha256,
        sourceSha256: source.sourceSha256,
        dueKey: source.dueKey,
        issuedAt: source.issuedAt,
        sourceProjectionSha256: createHash("sha256").update(sourceBytes).digest("hex"),
        predictionBodySha256: metadata.predictionBodySha256,
        servingAuthority: comparatorAuthority,
        rowCount: source.rowCount,
        rows: source.rows.map((row, index) => ({
          validAt: row.validAt,
          leadHours: row.leadHours,
          sourceRowSha256: identity.sourceRowSha256[index],
          incumbentPrecipitationMm64: encodeMaintenanceBinary64(2),
          occurrenceProbability64: encodeMaintenanceBinary64(0.6),
          atLeast1_0Probability64: encodeMaintenanceBinary64(0.4),
          atLeast2_5Probability64: encodeMaintenanceBinary64(0.2),
          positiveAmountMm64: encodeMaintenanceBinary64(2),
          applied: true,
          reasonCode: null,
        })),
      }, sourceBytes, bodyBytes)
    : null;
  const stageUnsigned = {
    contractVersion: `adjustment-shadow-stage-receipt/${version}`,
    durable: true,
    durableAt: "9999-11-01T20:35:01.000Z",
    dueKey: metadata.dueKey,
    predictionBodySha256: metadata.predictionBodySha256,
    registrationSha256: metadata.registrationSha256,
    sourceProjectionSha256: createHash("sha256").update(sourceBytes).digest("hex"),
    ...(comparatorBytes === null
      ? {}
      : { comparatorSha256: createHash("sha256").update(comparatorBytes).digest("hex") }),
  };
  const stageReceipt = {
    ...stageUnsigned,
    stageReceiptSha256: createHash("sha256")
      .update(Buffer.from(`${JSON.stringify(stageUnsigned)}\n`)).digest("hex"),
  };
  const predecessorFrontierSha256 = createHash("sha256")
    .update("adjustment-revision-frontier/v1\n0\n").digest("hex");
  const revisionReceipt = {
    archiveCommitOrdinal: "1",
    archiveCommittedAt: "9999-11-01T20:35:02.000Z",
    contractVersion: "adjustment-revision-commit-receipt/v1",
    frontierSha256: "",
    predecessorFrontierSha256,
    projectionIdentitySha256: metadata.sourceReceiptSha256,
    projectionKind: "shadow_prediction",
    projectionSha256: metadata.inputSha256,
    receiptSha256: "",
    stageReceiptSha256: stageReceipt.stageReceiptSha256,
  };
  revisionReceipt.receiptSha256 = createHash("sha256").update([
    revisionReceipt.contractVersion, revisionReceipt.archiveCommitOrdinal,
    revisionReceipt.archiveCommittedAt, revisionReceipt.projectionKind,
    revisionReceipt.projectionIdentitySha256, revisionReceipt.projectionSha256,
    revisionReceipt.stageReceiptSha256, revisionReceipt.predecessorFrontierSha256,
  ].join("\n")).digest("hex");
  revisionReceipt.frontierSha256 = createHash("sha256").update([
    "adjustment-revision-frontier/v1", predecessorFrontierSha256,
    revisionReceipt.archiveCommitOrdinal, revisionReceipt.receiptSha256,
  ].join("\n")).digest("hex");
  return Buffer.from(canonicalJsonBytes({
    bodyBase64: bodyBytes.toString("base64"),
    ...(comparatorBytes === null
      ? {}
      : {
          comparatorBase64: comparatorBytes.toString("base64"),
          comparatorSha256: stageUnsigned.comparatorSha256,
        }),
    contractVersion: `adjustment-shadow-revision-capsule/${version}`,
    metadata,
    ...(comparatorBytes === null
      ? {}
      : { predictionCommittedAt: "9999-11-01T20:35:01.500Z" }),
    revisionReceipt,
    sourceProjectionBase64: sourceBytes.toString("base64"),
    sourceProjectionSha256: stageUnsigned.sourceProjectionSha256,
    stageReceipt,
  }));
}

// build one parser-authenticated physical temperature revision
function targetRevisionBytes(sourceKey, sourceId, temperatureC) {
  const lineage = FORECAST_OBSERVATION_SOURCE_LINEAGES.find(
    // select one reviewed fixture lineage
    (candidate) => candidate.sourceKey === sourceKey,
  );
  const contentSha256 = createHash("sha256").update(`${sourceKey}\n`).digest("hex");
  const validAt = "2026-10-08T01:00:00.000Z";
  return encodeAdjustmentRevisionProjection({
    contractVersion: "adjustment-revision-projection/v1",
    family: "shared",
    logicalKey: {
      productRunAt: null,
      sourceId,
      sourceKind: "physical_sensor",
      validAt,
    },
    logicalReceivedAt: "2026-10-08T01:05:00.000Z",
    projectionKind: "target_revision",
    rows: [{
      apparentTemperatureC64: encodeMaintenanceBinary64(temperatureC),
      blackGlobeTemperatureC64: null,
      cloudCoverPercent64: null,
      contentSha256,
      pm25MicrogramsPerCubicMeter64: null,
      precipitationMm64: null,
      precipitationRateMmPerHour64: null,
      pressureHpa64: null,
      relativeHumidityPercent64: null,
      soilElectricalConductivityMicrosiemensPerCm64: null,
      soilMoisturePercent64: null,
      solarRadiationWm264: null,
      temperatureC64: encodeMaintenanceBinary64(temperatureC),
      uvIndex64: null,
      validAt,
      waterLevelM64: null,
      wetBulbGlobeTemperatureC64: null,
      windDirectionDegrees64: null,
      windGustMps64: null,
      windSpeedMps64: null,
    }],
    source: {
      adapterVersion: lineage.adapterContract,
      contractEpoch: "physical-observation/v1",
      dataset: "physical_observation",
      providerKey: lineage.physicalStationKey,
      sourceConfigFingerprint: lineage.checkedFingerprint,
      sourceId,
      sourceKey,
      sourceKind: "physical_sensor",
      upstreamModel: null,
    },
    storedContentSha256: contentSha256,
  });
}

test("portable adapter packages and independently replays temperature fit bytes", async () => {
  const candidateBytes = await temperatureFitBytes();
  const packaged = buildForecastAdjustmentMaintenancePortableCandidate({
    candidateBytes,
    family: "temperature",
  });
  const input = temperatureInput();
  const nativeOutput = evaluateForecastAdjustmentMaintenanceNativeCandidate({
    candidateBytes,
    family: "temperature",
    input,
  });
  const packagedOutput = evaluateForecastAdjustmentMaintenancePackagedCandidate({
    artifactBytes: packaged.artifactBytes,
    family: "temperature",
    input,
  });
  assert.deepEqual(nativeOutput, packagedOutput);
  assert.equal(JSON.parse(nativeOutput).state, "active");
  assert.equal(packaged.artifactSha256, JSON.parse(packaged.artifactBytes).bundleSha256);
  assert.equal(packaged.sourceIdentitySha256, packageTemperatureSourceIdentity());
});

test("portable adapter packages and independently replays wind fit bytes", () => {
  const candidateBytes = windFitBytes();
  const packaged = buildForecastAdjustmentMaintenancePortableCandidate({
    candidateBytes,
    family: "wind",
  });
  const input = windInput();
  const nativeOutput = evaluateForecastAdjustmentMaintenanceNativeCandidate({
    candidateBytes,
    family: "wind",
    input,
  });
  const packagedOutput = evaluateForecastAdjustmentMaintenancePackagedCandidate({
    artifactBytes: packaged.artifactBytes,
    family: "wind",
    input,
  });
  assert.deepEqual(nativeOutput, packagedOutput);
  assert.equal(JSON.parse(nativeOutput).state, "active");
  assert.equal(
    forecastAdjustmentWindMaintenanceSourceIdentitySha256(candidateBytes),
    packaged.sourceIdentitySha256,
  );
});

test("portable adapter derives first-candidate temperature and wind parity from fit rows", () => {
  const temperature = temperatureInput();
  const temperatureParity = buildForecastAdjustmentMaintenanceFitParityInputs({
    family: "temperature",
    fitInput: {
      contractVersion: "temperature-maintenance-fit-input/v2",
      developmentRows: [],
      trainingRows: [{
        adapterVersion: temperature.sourceForecast.adapterVersion,
        firstReceivedAt: temperature.sourceForecast.firstReceivedAt,
        key: "temperature-row",
        modelCycle: temperature.sourceForecast.modelCycle,
        modelLeadHours: temperature.sourceForecast.modelLeadHours,
        providerResponseSha256: temperature.sourceForecast.providerResponseSha256,
        rawBestMatchTemperatureC: temperature.rawBestMatchTemperatureC,
        rawRelativeHumidityPercent: temperature.sourceForecast.rawRelativeHumidityPercent,
        rawTemperatureC: temperature.sourceForecast.rawTemperatureC,
        rawWindSpeedMps: temperature.sourceForecast.rawWindSpeedMps,
        runInitializedAt: temperature.sourceForecast.runInitializedAt,
        sourceReceiptAt: temperature.evaluatedAt,
        state: temperature.recentErrorState,
        validAt: temperature.validAt,
      }],
    },
  });
  assert.deepEqual(temperatureParity.retainedInput, temperature);
  const wind = windInput();
  const windParity = buildForecastAdjustmentMaintenanceFitParityInputs({
    family: "wind",
    fitInput: {
      contractVersion: "wind-maintenance-fit-input/v2",
      rows: [{
        adapterContracts: [wind.rawForecastProvenance.adapterVersion],
        contentHashes: ["f".repeat(64)],
        contractEpoch: wind.rawForecastProvenance.contractEpoch,
        dataset: wind.rawForecastProvenance.dataset,
        metrics: {
          windGustMps: wind.metrics.windGustMps,
          windSpeedMps: wind.metrics.windSpeedMps,
        },
        receivedAt: wind.evaluatedAt,
        recordKind: "legacy_v4_retrieval_snapshot",
        referenceAt: wind.rawForecastProvenance.referenceAt,
        sourceConfigFingerprints: [wind.rawForecastProvenance.sourceConfigFingerprint],
        sourceKeys: [wind.rawForecastProvenance.sourceKey],
        targetLeadHours: wind.rawForecastProvenance.targetLeadHours,
        upstreamModel: wind.rawForecastProvenance.upstreamModel,
        validAt: wind.rawForecastProvenance.validAt,
      }],
    },
  });
  assert.deepEqual(windParity.retainedInput, wind);
  assert.notEqual(temperatureParity.retainedInputSha256, temperatureParity.syntheticInputSha256);
  assert.notEqual(windParity.retainedInputSha256, windParity.syntheticInputSha256);
});

test("portable adapter independently replays exact rain causal inputs", async () => {
  const candidateBytes = await rainFitBytes({
    contractVersion: "rain-maintenance-fit/v3",
    projectionId: "R1_winter_scale_0_90",
  });
  const packaged = buildForecastAdjustmentMaintenancePortableCandidate({
    candidateBytes,
    family: "rain",
  });
  const input = rainSourceProjection();
  const nativeOutput = evaluateForecastAdjustmentMaintenanceNativeCandidate({
    candidateBytes,
    family: "rain",
    input,
  });
  const packagedOutput = evaluateForecastAdjustmentMaintenancePackagedCandidate({
    artifactBytes: packaged.artifactBytes,
    family: "rain",
    input,
  });
  assert.deepEqual(nativeOutput, packagedOutput);
  assert.equal(JSON.parse(nativeOutput).hours.length, 23);
  assert.equal(packaged.sourceIdentitySha256, packageRainSourceIdentity());
});

test("portable adapter parses and cross-binds one complete shadow capsule", () => {
  const parsed = parseForecastAdjustmentMaintenanceShadowCapsule({
    capsuleBytes: rainCapsuleBytes(),
  });
  assert.equal(parsed.body.family, "rain");
  assert.equal(parsed.source.family, "rain");
  assert.equal(parsed.revisionReceipt.projectionIdentitySha256,
    parsed.sourceIdentity.sourceReceiptSha256);
  const parsedV2 = parseForecastAdjustmentMaintenanceShadowCapsule({
    capsuleBytes: rainCapsuleBytes("v2"),
  });
  assert.equal(parsedV2.comparator.family, "rain");
  assert.equal(parsedV2.comparator.servingAuthority.authorityKind, "legacy_active");
  assert.throws(() => parseForecastAdjustmentMaintenanceShadowCapsule({
    capsuleBytes: Buffer.concat([rainCapsuleBytes(), Buffer.from("\n")]),
  }), /canonical|bytes/u);
});

test("portable adapter exposes the exact rain native source with its shadow receipt", () => {
  const projected = projectForecastAdjustmentMaintenanceShadowNativeSource({
    capsuleBytes: rainCapsuleBytes("v2"),
  });
  assert.equal(projected.family, "rain");
  assert.equal(projected.captureClaims.length, 1);
  assert.equal(projected.captureClaims[0].claimId, "rain-current");
  assert.equal(projected.sourceRows.length, 23);
  assert.equal(projected.revisionReceipt.projectionKind, "shadow_prediction");
  assert.equal(projected.revisionReceipt.projectionIdentitySha256,
    projected.sourceReceiptSha256);
  assert.equal(projected.revisionReceipt.projectionSha256,
    projected.sourceProjectionSha256);
  assert.equal(sha256(projected.sourceBytes), projected.sourceProjectionSha256);
});

test("portable adapter derives distinct rain parity inputs from an authenticated capsule", async () => {
  const inputs = buildForecastAdjustmentMaintenanceParityInputs({
    capsuleBytes: rainCapsuleBytes("v2"),
  });
  const candidateBytes = await rainFitBytes({
    contractVersion: "rain-maintenance-fit/v3",
    projectionId: "R1_winter_scale_0_90",
  });
  const packaged = buildForecastAdjustmentMaintenancePortableCandidate({
    candidateBytes,
    family: "rain",
  });
  assert.notEqual(inputs.retainedInputSha256, inputs.syntheticInputSha256);
  // prove both authenticated-derived fixtures traverse independent candidate decoders
  for (const input of [inputs.retainedInput, inputs.syntheticInput]) {
    const native = evaluateForecastAdjustmentMaintenanceNativeCandidate({
      candidateBytes,
      family: "rain",
      input,
    });
    const portable = evaluateForecastAdjustmentMaintenancePackagedCandidate({
      artifactBytes: packaged.artifactBytes,
      family: "rain",
      input,
    });
    assert.deepEqual(native, portable);
  }
});

test("portable adapter derives first-candidate rain parity from archived feature rows", async () => {
  const featureNames = JSON.parse(RAIN_HURDLE_WIND_ARTIFACT_JSON).featureNames;
  const features = Array.from({ length: 107 }, (_unused, index) => index === 5 ? 1 : null);
  const inputs = buildForecastAdjustmentMaintenanceFitParityInputs({
    family: "rain",
    fitInput: {
      contractVersion: "rain-maintenance-fit-input/v3",
      developmentRows: [],
      featureNames,
      trainingRows: [{ features, key: "rain-row", validAt: "2026-10-08T01:00:00.000Z" }],
    },
  });
  const candidateBytes = await rainFitBytes({
    contractVersion: "rain-maintenance-fit/v3",
    projectionId: "R1_winter_scale_0_90",
  });
  const packaged = buildForecastAdjustmentMaintenancePortableCandidate({
    candidateBytes,
    family: "rain",
  });
  assert.equal(inputs.retainedInput.contractVersion,
    "rain-maintenance-feature-parity-input/v2");
  assert.equal(inputs.retainedInput.validAt, "2026-10-08T01:00:00.000Z");
  assert.notEqual(inputs.retainedInputSha256, inputs.syntheticInputSha256);
  // prove pre-candidate feature parity without a shadow capsule
  for (const input of [inputs.retainedInput, inputs.syntheticInput]) {
    assert.deepEqual(
      evaluateForecastAdjustmentMaintenanceNativeCandidate({
        candidateBytes,
        family: "rain",
        input,
      }),
      evaluateForecastAdjustmentMaintenancePackagedCandidate({
        artifactBytes: packaged.artifactBytes,
        family: "rain",
        input,
      }),
    );
  }
});

// compare disjoint raw-fit and portable evaluators across selected v3 arms
test("portable adapter independently preserves rain v3 arm and guard semantics", async () => {
  const featureNames = JSON.parse(RAIN_HURDLE_WIND_ARTIFACT_JSON).featureNames;
  const input = {
    contractVersion: "rain-maintenance-feature-parity-input/v2",
    featureNames,
    features: Array.from({ length: 107 }, (_unused, index) => index === 5 ? 0.5 : null),
    validAt: "2027-04-15T12:00:00.000Z",
  };
  const artifactSha256s = new Set();
  // cover base, seasonal, nesting, and heavy-blend arms independently
  for (const projectionId of [
    "R0_exact_refit",
    "R3_spring_wet_logit_plus_0_20",
    "R5_nested_cumulative_min",
    "R6_heavy_raw_blend_0_25",
  ]) {
    const candidateBytes = await rainFitBytes({
      contractVersion: "rain-maintenance-fit/v3",
      projectionId,
    });
    const packaged = buildForecastAdjustmentMaintenancePortableCandidate({
      candidateBytes,
      family: "rain",
    });
    artifactSha256s.add(packaged.artifactSha256);
    assert.deepEqual(
      evaluateForecastAdjustmentMaintenanceNativeCandidate({
        candidateBytes,
        family: "rain",
        input,
      }),
      evaluateForecastAdjustmentMaintenancePackagedCandidate({
        artifactBytes: packaged.artifactBytes,
        family: "rain",
        input,
      }),
    );
  }
  assert.equal(artifactSha256s.size, 4);
  const heavyCandidate = await rainFitBytes({
    contractVersion: "rain-maintenance-fit/v3",
    projectionId: "R6_heavy_raw_blend_0_25",
  });
  const heavyPackage = buildForecastAdjustmentMaintenancePortableCandidate({
    candidateBytes: heavyCandidate,
    family: "rain",
  });
  const heavyInput = structuredClone(input);
  heavyInput.features[5] = 50;
  const heavy = JSON.parse(evaluateForecastAdjustmentMaintenancePackagedCandidate({
    artifactBytes: heavyPackage.artifactBytes,
    family: "rain",
    input: heavyInput,
  }).toString("utf8"));
  assert.equal(heavy.correctedPrecipitationMm, 50);
});

test("portable adapter derives network temperature targets from exact archived lineages", () => {
  const projected = projectForecastAdjustmentMaintenanceTemperatureTargets({
    projectionBytes: [
      targetRevisionBytes("ambient-merlin-observations-v1", "1", 12),
      targetRevisionBytes("ecowitt-88f15505d89f-local-live-v1", "2", 10),
      targetRevisionBytes("tempest-38270-observations-v2", "3", 14),
    ],
  });
  assert.equal(projected.stationRows.length, 3);
  assert.equal(projected.temperatureTargets.length, 1);
  assert.equal(projected.temperatureTargets[0].stationCount, 3);
  assert.match(projected.temperatureTargets[0].networkTemperatureC64, /^[a-f0-9]{16}$/u);
  assert.equal(projected.temperatureTargets[0].normalizedWeights.length, 3);
  assert.throws(() => projectForecastAdjustmentMaintenanceTemperatureTargets({
    projectionBytes: [
      targetRevisionBytes("ambient-merlin-observations-v1", "1", 12),
      targetRevisionBytes("ambient-merlin-observations-v1", "2", 13),
    ],
  }), /duplicated/u);
});

test("portable adapter exposes exact source identities and a closed installed manifest", async () => {
  assert.equal(typeof loadInstalledForecastAdjustmentMaintenanceShadowCandidate, "function");
  assert.equal(forecastAdjustmentTemperatureMaintenanceSourceIdentitySha256(),
    packageTemperatureSourceIdentity());
  assert.equal(forecastAdjustmentRainMaintenanceSourceIdentitySha256(), packageRainSourceIdentity());
  assert.throws(() => forecastAdjustmentWindMaintenanceSourceIdentitySha256(
    INVALID_FIT_BYTES,
  ));
  const destinations = ADJUSTMENT_MAINTENANCE_RUNTIME_ADAPTER_SOURCE_FILES.map(
    // project every exact installed destination once
    (entry) => entry.destination,
  );
  assert.equal(new Set(destinations).size, destinations.length);
  await Promise.all(ADJUSTMENT_MAINTENANCE_RUNTIME_ADAPTER_SOURCE_FILES.map(
    // prove each frozen source closure member exists in the checkout
    async (entry) => await access(resolve(import.meta.dirname, "../..", entry.source)),
  ));
  assert.equal(ADJUSTMENT_MAINTENANCE_RUNTIME_ADAPTER_GENERATED_FILES.length, 33);
  assert.equal(ADJUSTMENT_MAINTENANCE_RUNTIME_ADAPTER_SOURCE_FILES.length, 40);
  assert.equal(ADJUSTMENT_FIT_RUNTIME_GENERATED_FILES.length, 12);
  // enforce the installed adapter and fit-only compiler snapshots together
  for (const entry of [
    ...ADJUSTMENT_MAINTENANCE_RUNTIME_ADAPTER_GENERATED_FILES,
    ...ADJUSTMENT_FIT_RUNTIME_GENERATED_FILES,
  ]) {
    assert.deepEqual(
      await readFile(resolve(import.meta.dirname, "../..", entry.snapshot)),
      await readFile(resolve(import.meta.dirname, "../..", entry.generatedSource)),
      `${entry.snapshot} differs from its generated source`,
    );
  }
});

test("portable adapter rejects policy mode and epoch ambiguity", () => {
  assert.throws(() => evaluateForecastAdjustmentMaintenancePolicy({
    epoch: null,
    family: "temperature",
    kind: "regression",
    rows: [],
  }), /kind or epoch/u);
});
