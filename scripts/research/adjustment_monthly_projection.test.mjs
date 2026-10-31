import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";

import {
  FORECAST_OBSERVATION_SOURCE_LINEAGES,
} from "./adjustment-maintenance-runtime/node_modules/@weather/domain/dist/forecast-adjustment.js";
import {
  encodeAdjustmentRevisionProjection,
  encodeAdjustmentTemperatureNativeSourceProjection,
} from "./adjustment-maintenance-runtime/forecast/maintenance-revision-projection.js";
import {
  encodeMaintenanceBinary64,
} from "./adjustment-maintenance-runtime/forecast/maintenance-shadow-values.js";
import {
  fitWindMaintenanceDevelopment,
} from "../../packages/forecast-adjustment/dist/evidence.js";
import {
  buildForecastAdjustmentMaintenanceFitParityInputs,
} from "./adjustment_maintenance_runtime_adapter.mjs";
import {
  buildAdjustmentMonthlyFitAssembly,
  validateAdjustmentMonthlyFitAssembly,
} from "./adjustment_monthly_projection.mjs";
import {
  adjustmentSha256,
  canonicalJsonBytes,
} from "./adjustment_plaintext_archive.mjs";

const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);
const lineage = FORECAST_OBSERVATION_SOURCE_LINEAGES[0];

// construct one valid frozen future-only witness
function epochWitness() {
  const epochAt = "2026-10-10T08:00:00.000Z";
  const frontier = adjustmentSha256(Buffer.from(
    "adjustment-revision-frontier/v1\n0\n",
  ));
  const unsigned = {
    activationKind: "inert_v14_pre_activation",
    archiveCommitOrdinal: "0",
    catalogFrontierSha256: frontier,
    contractVersion: "adjustment-revision-capture-epoch-witness/v1",
    controlPlaneSha256: HASH_A,
    controlPlaneVersion: "14",
    databaseMigrationHistorySha256:
      "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
    epochAt,
    servingSnapshotSha256: adjustmentSha256(Buffer.from([
      "adjustment-revision-serving-snapshot/v1", epochAt, "0", frontier, "",
    ].join("\n"))),
    sourceCommit: "1".repeat(40),
    sourceRelease: "2026.10.10-1",
    sourceServerImageDigest: `sha256:${HASH_A}`,
    sourceWebImageDigest: `sha256:${HASH_B}`,
  };
  return { ...unsigned, witnessSha256: adjustmentSha256(canonicalJsonBytes(unsigned)) };
}

// construct one canonical physical target source
function targetSource(selectedLineage = lineage, sourceId = "1") {
  return {
    adapterVersion: selectedLineage.adapterContract,
    contractEpoch: "physical-station-hourly/v1",
    dataset: "physical_observation",
    providerKey: "ambient",
    sourceConfigFingerprint: selectedLineage.checkedFingerprint,
    sourceId,
    sourceKey: selectedLineage.sourceKey,
    sourceKind: "physical_sensor",
    upstreamModel: null,
  };
}

// construct one canonical best-match forecast source
function forecastSource() {
  return {
    adapterVersion: "open-meteo-forecast-v4",
    contractEpoch: "forecast-v4",
    dataset: "best_match",
    providerKey: "open-meteo",
    sourceConfigFingerprint: HASH_A,
    sourceId: "2",
    sourceKey: "open-meteo-forecast-v4",
    sourceKind: "forecast",
    upstreamModel: "best_match",
  };
}

// construct one closed weather row
function weatherRow(validAt, contentSha256) {
  return {
    apparentTemperatureC64: encodeMaintenanceBinary64(9),
    blackGlobeTemperatureC64: null,
    cloudCoverPercent64: encodeMaintenanceBinary64(50),
    contentSha256,
    pm25MicrogramsPerCubicMeter64: null,
    precipitationMm64: encodeMaintenanceBinary64(0),
    precipitationRateMmPerHour64: null,
    pressureHpa64: encodeMaintenanceBinary64(1_010),
    relativeHumidityPercent64: encodeMaintenanceBinary64(80),
    soilElectricalConductivityMicrosiemensPerCm64: null,
    soilMoisturePercent64: null,
    solarRadiationWm264: null,
    temperatureC64: encodeMaintenanceBinary64(10),
    uvIndex64: null,
    validAt,
    waterLevelM64: null,
    wetBulbGlobeTemperatureC64: null,
    windDirectionDegrees64: encodeMaintenanceBinary64(180),
    windGustMps64: encodeMaintenanceBinary64(8),
    windSpeedMps64: encodeMaintenanceBinary64(5),
  };
}

// construct one database-backed historical member
function member(document, row, ordinal, options = {}) {
  const payloadBytes = document.contractVersion ===
    "adjustment-temperature-native-source-projection/v2"
    ? encodeAdjustmentTemperatureNativeSourceProjection(document)
    : encodeAdjustmentRevisionProjection(document);
  const payloadIdentitySha256 = adjustmentSha256(payloadBytes);
  const rowIndex = document.rows.indexOf(row);
  const memberSha256 = options.memberSha256 ?? adjustmentSha256(
    Buffer.from(`member-${ordinal}-${rowIndex}`));
  return Object.freeze({
    document,
    graphManifestSha256: HASH_A,
    memberSha256,
    payloadBytes,
    payloadIdentitySha256,
    projectionKind: document.projectionKind,
    receipt: {
      archiveCommitOrdinal: String(ordinal),
      archiveCommittedAt: options.archiveCommittedAt ??
        new Date(Date.parse(row.validAt) + 3_600_000).toISOString(),
      contractVersion: "adjustment-revision-commit-receipt/v1",
      frontierSha256: HASH_A,
      predecessorFrontierSha256: HASH_B,
      projectionIdentitySha256: payloadIdentitySha256,
      projectionKind: document.projectionKind,
      projectionSha256: payloadIdentitySha256,
      receiptSha256: adjustmentSha256(Buffer.from(`receipt-${ordinal}`)),
      stageReceiptSha256: adjustmentSha256(Buffer.from(`stage-${ordinal}`)),
    },
    row,
    rowIndex,
  });
}

// construct one exact historical projection boundary
function historicalProjection(family, dueMonth, cutoffAt, projectionMembers,
  shadowMembers = []) {
  return Object.freeze({
    classCounts: {
      actual_best_match: projectionMembers.filter(
        (value) => value.projectionKind === "actual_best_match").length,
      native_source: projectionMembers.filter(
        (value) => value.projectionKind === "native_source").length,
      rain_gate_input: 0,
      target_revision: projectionMembers.filter(
        (value) => value.projectionKind === "target_revision").length,
    },
    contractVersion: "adjustment-revision-fit-projection/v2",
    cutoffAt,
    dueMonth,
    epochWitnessSha256: epochWitness().witnessSha256,
    family,
    historyRootSha256: adjustmentSha256(Buffer.from("history")),
    memberRootSha256: adjustmentSha256(Buffer.from(`members-${family}`)),
    projectionMembers: Object.freeze(projectionMembers),
    rowCount: projectionMembers.length + shadowMembers.length,
    shadowMembers: Object.freeze(shadowMembers),
  });
}

// construct one exact empty source-decision state
function emptyTemperatureState(runInitializedAt) {
  return {
    b24C: null,
    b72C: null,
    cohort: "ecmwf_single_run_hindcast",
    localDates: 0,
    mad72C: null,
    maximumSourceRunInitializedAt: null,
    maximumSourceValidAt: null,
    n24: 0,
    n72: 0,
    sourceKeys: [],
    supported: false,
    targetRunInitializedAt: runInitializedAt,
    windowEndValidAt: new Date(Date.parse(runInitializedAt) - 7 * 3_600_000).toISOString(),
  };
}

// build one native-v2 first-candidate fixture without any shadow capsule
function temperatureFirstCandidateFixture(options = {}) {
  const runInitializedAt = "2026-10-12T08:00:00.000Z";
  const nativeDecisionAt = "2026-10-12T14:00:00.000Z";
  const cutoffAt = "2026-11-24T08:00:00.000Z";
  const recentErrorState = {
    ...emptyTemperatureState(runInitializedAt),
    ...options.recentErrorState,
  };
  const selectedLineages = [...new Map(FORECAST_OBSERVATION_SOURCE_LINEAGES
    .filter((candidate) => candidate.acceptedEndExclusive === null)
    .map((candidate) => [candidate.physicalStationKey, candidate])).values()].slice(0, 5);
  const projectionMembers = [];
  const nativeRows = [];
  let ordinal = 1;

  // retain all eighteen native hours while binding only the eligible twelve comparators
  for (let lead = 1; lead <= 18; lead += 1) {
    const validAt = new Date(Date.parse(runInitializedAt) + lead * 3_600_000).toISOString();
    const rawContentSha256 = adjustmentSha256(Buffer.from(`native-${lead}`));
    const bestContentSha256 = adjustmentSha256(Buffer.from(`best-${lead}`));
    const eligible = lead >= 7;
    nativeRows.push({
      bestMatchContentSha256: eligible ? bestContentSha256 : null,
      bestMatchProductRunAt: eligible ? "2026-10-12T12:00:00.000Z" : null,
      bestMatchSourceId: eligible ? String(options.forgedLead === lead ? 999 : 2) : null,
      bestMatchTemperatureC64: eligible ? encodeMaintenanceBinary64(10) : null,
      contentSha256: rawContentSha256,
      modelCycle: "50r1",
      modelLeadHours: lead,
      rawRelativeHumidityPercent64: encodeMaintenanceBinary64(80),
      rawTemperatureC64: encodeMaintenanceBinary64(11),
      rawWindSpeedMps64: encodeMaintenanceBinary64(5),
      validAt,
    });
    // archive exact best-match and physical targets only for serving leads
    if (eligible) {
      const bestRow = weatherRow(validAt, bestContentSha256);
      const bestDocument = {
        contractVersion: "adjustment-revision-projection/v1",
        family: "shared",
        logicalKey: {
          productRunAt: "2026-10-12T12:00:00.000Z",
          sourceId: "2",
          sourceKind: "forecast",
          validAt,
        },
        logicalReceivedAt: "2026-10-12T13:00:00.000Z",
        projectionKind: "actual_best_match",
        rows: [bestRow],
        source: forecastSource(),
        storedContentSha256: bestContentSha256,
      };
      projectionMembers.push(member(bestDocument, bestRow, ordinal++, {
        archiveCommittedAt: options.bestMatchReceiptAt ??
          "2026-10-12T13:30:00.000Z",
      }));
      // retain the same frozen physical network used by the serving state recipe
      for (const [index, selected] of selectedLineages.entries()) {
        const targetContentSha256 = adjustmentSha256(
          Buffer.from(`target-${lead}-${index}`));
        const targetRow = weatherRow(validAt, targetContentSha256);
        const targetDocument = {
          contractVersion: "adjustment-revision-projection/v1",
          family: "shared",
          logicalKey: {
            productRunAt: null,
            sourceId: String(index + 10),
            sourceKind: "physical_sensor",
            validAt,
          },
          logicalReceivedAt: new Date(Date.parse(validAt) + 10 * 60_000).toISOString(),
          projectionKind: "target_revision",
          rows: [targetRow],
          source: targetSource(selected, String(index + 10)),
          storedContentSha256: targetContentSha256,
        };
        projectionMembers.push(member(targetDocument, targetRow, ordinal++, {
          archiveCommittedAt: options.targetReceiptAt ??
            new Date(Date.parse(validAt) + 30 * 60_000).toISOString(),
        }));
      }
    }
  }
  const nativeDocument = {
    contractVersion: "adjustment-temperature-native-source-projection/v2",
    family: "temperature",
    logicalKey: {
      contentSha256: HASH_B,
      leadHours: null,
      providerResponseSha256: HASH_A,
      runInitializedAt,
      siteId: "1",
      sourceId: null,
      sourceType: "ecmwf_temperature_run",
      validAt: null,
    },
    logicalReceivedAt: nativeDecisionAt,
    projectionKind: "native_source",
    recentErrorState,
    recentErrorStateSha256: createHash("sha256")
      .update(JSON.stringify(recentErrorState)).digest("hex"),
    rows: nativeRows,
    source: {
      ...forecastSource(),
      adapterVersion: "open-meteo-ecmwf-single-run/v1",
      contractEpoch: "ecmwf-temperature-canary/v1",
      dataset: "single_run",
      sourceId: "site:1",
      sourceKey: "open-meteo-ecmwf-single-run:ballydidean",
      upstreamModel: "ecmwf_ifs",
    },
    storedContentSha256: HASH_B,
  };
  // bind every selected native row to the one actual run receipt
  for (const row of nativeRows) {
    projectionMembers.push(member(nativeDocument, row, ordinal, {
      archiveCommittedAt: "2026-10-12T14:05:00.000Z",
    }));
  }
  return {
    input: {
      dueMonth: "2026-12",
      epochWitness: epochWitness(),
      family: "temperature",
      historicalProjection: historicalProjection("temperature", "2026-12",
        cutoffAt, projectionMembers),
      incumbent: {
        artifactBytes: Buffer.from("compiled"), artifactIdentitySha256: HASH_A,
        authorityKind: "maintenance_qualified", candidate: null,
        comparatorAuthorityBytes: Buffer.from("authority"), family: "temperature",
        model: { contractVersion: "temperature-permanent-model/v1" }, reasonCode: null,
        state: "active",
      },
    },
    nativeDecisionAt,
  };
}

// assemble a first temperature candidate from native v2 without prior shadows
test("monthly temperature assembly replays native v2 without a shadow candidate", () => {
  const fixture = temperatureFirstCandidateFixture();
  const result = buildAdjustmentMonthlyFitAssembly(fixture.input);
  assert.equal(validateAdjustmentMonthlyFitAssembly(result), result);
  assert.equal(fixture.input.historicalProjection.shadowMembers.length, 0);
  assert.equal(result.fitInput.developmentRows.length, 12);
  assert.equal(result.fitInput.trainingRows.length, 0);
  assert.equal(result.fitInput.developmentRows[0].adapterVersion,
    "open-meteo-ecmwf-single-run/v1");
  assert.equal(result.fitInput.developmentRows[0].firstReceivedAt,
    fixture.nativeDecisionAt);
  assert.equal(result.fitInput.developmentRows[0].providerResponseSha256, HASH_A);
  assert.equal(result.fitInput.developmentRows[0].rawBestMatchTemperatureC, 10);
  assert.equal(result.fitInput.developmentRows[0].actualTemperatureC, 10);
  assert.deepEqual(result.fitInput.developmentRows[0].state,
    emptyTemperatureState("2026-10-12T08:00:00.000Z"));
  const parity = buildForecastAdjustmentMaintenanceFitParityInputs({
    family: "temperature",
    fitInput: result.fitInput,
  });
  assert.equal(parity.retainedInput.sourceForecast.firstReceivedAt,
    fixture.nativeDecisionAt);
  assert.equal(parity.retainedInput.rawBestMatchTemperatureC, 10);
});

// reject a comparator that matches content but not the frozen source tuple
test("monthly temperature assembly rejects a forged best-match tuple", () => {
  assert.throws(() => buildAdjustmentMonthlyFitAssembly(
    temperatureFirstCandidateFixture({ forgedLead: 7 }).input),
  /projection is incomplete/u);
});

// reject a source-decision state that cannot be replayed from archived causes
test("monthly temperature assembly rejects a forged recent-error state", () => {
  assert.throws(() => buildAdjustmentMonthlyFitAssembly(
    temperatureFirstCandidateFixture({ recentErrorState: {
      localDates: 1,
      maximumSourceRunInitializedAt: "2026-10-11T12:00:00.000Z",
      maximumSourceValidAt: "2026-10-11T17:00:00.000Z",
      n24: 1,
      n72: 1,
      sourceKeys: ["2026-10-11T12:00:00.000Z/2026-10-11T17:00:00.000Z"],
    } }).input), /recent-error state differs/u);
});

// reject physical targets that were not archived before the original cutoff
test("monthly temperature assembly rejects a future target receipt", () => {
  assert.throws(() => buildAdjustmentMonthlyFitAssembly(
    temperatureFirstCandidateFixture({
      targetReceiptAt: "2026-11-24T08:00:00.000Z",
    }).input), /receipt exceeds its cutoff/u);
});

// reject a comparator archived after the frozen native source decision
test("monthly temperature assembly rejects a late best-match receipt", () => {
  assert.throws(() => buildAdjustmentMonthlyFitAssembly(
    temperatureFirstCandidateFixture({
      bestMatchReceiptAt: "2026-10-12T14:30:00.000Z",
    }).input), /best-match receipt is late/u);
});

// build a complete archive-native wind manifest accepted by the unchanged fitter
 test("monthly wind assembly emits a genuine archive manifest v2", async () => {
  const projectionMembers = [];
  const end = Date.parse("2028-01-24T12:00:00.000Z");
  let ordinal = 1;
  // retain one forecast and physical target on every exact epoch date
  for (let index = 0; index < 402; index += 1) {
    const validAt = new Date(end - (401 - index) * 86_400_000).toISOString();
    const targetRow = weatherRow(validAt, adjustmentSha256(Buffer.from(`target-${index}`)));
    const targetDocument = {
      contractVersion: "adjustment-revision-projection/v1",
      family: "shared",
      logicalKey: { productRunAt: null, sourceId: "1",
        sourceKind: "physical_sensor", validAt },
      logicalReceivedAt: new Date(Date.parse(validAt) + 1_800_000).toISOString(),
      projectionKind: "target_revision",
      rows: [targetRow],
      source: targetSource(),
      storedContentSha256: targetRow.contentSha256,
    };
    const forecastRow = weatherRow(validAt,
      adjustmentSha256(Buffer.from(`forecast-${index}`)));
    const forecastDocument = {
      ...targetDocument,
      logicalKey: { productRunAt: new Date(Date.parse(validAt) - 24 * 3_600_000)
        .toISOString(), sourceId: "2", sourceKind: "forecast", validAt },
      projectionKind: "actual_best_match",
      rows: [forecastRow],
      source: forecastSource(),
      storedContentSha256: forecastRow.contentSha256,
    };
    projectionMembers.push(member(targetDocument, targetRow, ordinal++));
    projectionMembers.push(member(forecastDocument, forecastRow, ordinal++));
  }
  const result = buildAdjustmentMonthlyFitAssembly({
    dueMonth: "2028-02",
    epochWitness: epochWitness(),
    family: "wind",
    historicalProjection: historicalProjection("wind", "2028-02",
      "2028-01-25T08:00:00.000Z", projectionMembers),
    incumbent: null,
  });
  assert.equal(result.fitInput.manifest.contractVersion,
    "adjustment-wind-archive-fit-manifest/v2");
  assert.equal(result.fitInput.manifest.members.length, 804);
  assert.equal(result.fitInput.openedMembers.length, 804);
  assert.equal(result.fitInput.rows.length, 804);
  const fitted = await fitWindMaintenanceDevelopment(result.fitInput);
  assert.equal(fitted.state, "insufficient_data");
  assert.match(result.fitInput.snapshotManifestSha256, /^[a-f0-9]{64}$/u);
});
