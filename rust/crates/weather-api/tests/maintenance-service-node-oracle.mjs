import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { Readable } from "node:stream";

import {
  createAdjustmentMaintenanceInternalServer,
} from "../../../../apps/api/dist/adjustment-maintenance-shadow.js";
import {
  readAdjustmentEcmwfTemperatureRevisionAdmission,
  readAdjustmentForecastAnchorRevisionAdmission,
  readAdjustmentRainGateRevisionAdmission,
} from "../../../../packages/database/dist/index.js";
import {
  createMaintenanceShadowPredictionMetadata,
  createMaintenanceShadowSourceIdentity,
  createRainMaintenanceControlState,
  encodeAdjustmentRevisionProjection,
  encodeMaintenanceBinary64,
  encodeMaintenanceShadowSourceProjection,
  encodeMaintenanceShadowValues,
  encodeRainMaintenanceControlState,
  RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256,
} from "../../../../packages/forecast-adjustment/dist/index.js";

const captureEpoch = {
  epochAt: "2000-01-01T00:00:00.000Z",
  witnessSha256: "3".repeat(64),
};
const queries = [];
const captures = [];
const gaps = [];
const publicationEvents = [];
const revisionGaps = [];
const rainControlStateGaps = [];
let authoritativeRevisionReceipt;
let authoritativeShadowReceipt;
let retainedFixedGaugeGap;
let revisionReadFailed = false;
let revisionPublishFailed = false;
let shadowBodyCommitted = true;
const publicationQueries = new Map();

// hash one exact fixture value
function hash(value) {
  return createHash("sha256").update(value).digest("hex");
}

// build one canonical future-only rain maintenance control state
function rainControlStateBytes() {
  return encodeRainMaintenanceControlState(createRainMaintenanceControlState({
    calibrationEndAt: "2026-08-25T00:00:00.000Z",
    calibrationStartAt: "2026-05-27T00:00:00.000Z",
    contractVersion: "rain-maintenance-control-state/v1",
    epochWitnessSha256: captureEpoch.witnessSha256,
    generatedAt: "2026-08-26T00:00:00.000Z",
    legacyCalibrationStartAt: "2026-07-11T00:00:00.000Z",
    legacyRawScale: 1.1,
    modelMonth: "2026-09",
    ordinalArtifactSha256: "4".repeat(64),
    recentFallbackReason: "recent_calibration",
    recentRawScale: 1.2,
    recentSupported: true,
    recipeSha256: RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256,
    sameWindowRawScale: 1.15,
    scheduleContractSha256: "5".repeat(64),
    sourceMemberRootSha256: "6".repeat(64),
    sourceReceiptRootSha256: "7".repeat(64),
    support: {
      calibrationDates: 90,
      calibrationHours: 2000,
      calibrationRows: 2000,
      calibrationWetDates: 20,
      calibrationWetHours: 200,
      effectiveDates64: "404e000000000000",
      effectiveWetDates64: "4008000000000000",
      legacyCalibrationRows: 500,
      legacyTrainingRows: 2000,
      legacyTrainingWetRows: 200,
      trainingDates: 200,
      trainingHours: 4000,
      trainingRows: 4000,
      trainingWetDates: 30,
      trainingWetHours: 300,
    },
    trainingMaximumValidAt: "2026-05-19T23:00:00.000Z",
  }));
}

// build one canonical current Best Match revision projection
function revisionProjection() {
  return encodeAdjustmentRevisionProjection({
    contractVersion: "adjustment-revision-projection/v1",
    family: "shared",
    logicalKey: {
      productRunAt: "2026-09-14T00:00:00.000Z",
      sourceId: "7",
      sourceKind: "forecast",
      validAt: "2026-09-14T01:00:00.000Z",
    },
    logicalReceivedAt: "2026-09-14T00:10:00.000Z",
    projectionKind: "actual_best_match",
    rows: [{
      apparentTemperatureC64: encodeMaintenanceBinary64(10),
      blackGlobeTemperatureC64: null,
      cloudCoverPercent64: encodeMaintenanceBinary64(50),
      contentSha256: hash("revision-content"),
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
      validAt: "2026-09-14T01:00:00.000Z",
      waterLevelM64: null,
      wetBulbGlobeTemperatureC64: null,
      windDirectionDegrees64: encodeMaintenanceBinary64(180),
      windGustMps64: encodeMaintenanceBinary64(8),
      windSpeedMps64: encodeMaintenanceBinary64(5),
    }],
    source: {
      adapterVersion: "open-meteo/v1",
      contractEpoch: "weather-record/v4",
      dataset: "best_match",
      providerKey: "open-meteo",
      sourceConfigFingerprint: "fingerprint-v1",
      sourceId: "7",
      sourceKey: "forecast-ballydidean",
      sourceKind: "forecast",
      upstreamModel: "best_match",
    },
    storedContentSha256: hash("revision-content"),
  });
}

// create one canonical revision stage receipt
function revisionStageReceipt(projection) {
  const identity = hash(projection);
  const unsigned = {
    contractVersion: "adjustment-revision-stage-receipt/v1",
    durable: true,
    durableAt: "2026-09-14T00:11:00.000Z",
    projectionIdentitySha256: identity,
    projectionKind: "actual_best_match",
    projectionSha256: identity,
  };
  return { ...unsigned, stageReceiptSha256: hash(`${JSON.stringify(unsigned)}\n`) };
}

// create one canonical revision publish receipt
function revisionPublishReceipt(receipt) {
  const unsigned = {
    committed: true,
    committedAt: "2026-09-14T00:13:00.000Z",
    contractVersion: "adjustment-revision-publish-receipt/v1",
    projectionIdentitySha256: receipt.projectionIdentitySha256,
    projectionKind: receipt.projectionKind,
    projectionSha256: receipt.projectionSha256,
    revisionReceiptSha256: receipt.receiptSha256,
  };
  return { ...unsigned, publishReceiptSha256: hash(`${JSON.stringify(unsigned)}\n`) };
}

// build one complete canonical wind source and prediction body
function ordinaryShadowMaterial() {
  const registrationSha256 = "1".repeat(64);
  const candidateSha256 = "2".repeat(64);
  const sourceSha256 = "3".repeat(64);
  const dueKey = "capture/2026-09-14T00:35:00.000Z";
  const issuedAt = "2026-09-14T00:35:00.000Z";
  const referenceAt = "2026-09-14T00:00:00.000Z";
  // preserve the full fixed wind horizon in source order
  const sourceRows = Array.from({ length: 168 }, (_, index) => {
    const leadHours = index + 1;
    return {
      validAt: new Date(Date.parse(referenceAt) + leadHours * 3_600_000).toISOString(),
      leadHours,
      modelLeadHours: leadHours,
      referenceAt,
      receivedAt: "2026-09-14T00:10:00.000Z",
      sourceSha256,
      contentSha256: hash(`wind-content-${leadHours}`),
      adapterVersion: "open-meteo-forecast-daily/v4",
      contractEpoch: "v4",
      dataset: "forecast",
      providerKey: "open-meteo",
      sourceKey: "open-meteo-forecast-v4",
      sourceConfigFingerprint: "4".repeat(64),
      upstreamModel: "best_match",
      revisionCount: 0,
      windSpeedMps64: encodeMaintenanceBinary64(2),
      windGustMps64: encodeMaintenanceBinary64(3),
    };
  });
  const sourceProjection = encodeMaintenanceShadowSourceProjection({
    contractVersion: "adjustment-shadow-source-projection/v1",
    family: "wind",
    registrationSha256,
    candidateSha256,
    sourceSha256,
    dueKey,
    issuedAt,
    rowCount: sourceRows.length,
    rows: sourceRows,
  });
  const identity = createMaintenanceShadowSourceIdentity(sourceProjection);
  // preserve the paired disabled-gust band while binding every source row
  const body = encodeMaintenanceShadowValues({
    contractVersion: "adjustment-shadow-prediction-values/v3",
    family: "wind",
    registrationSha256,
    candidateSha256,
    sourceSha256,
    dueKey,
    issuedAt,
    sourceReceiptSha256: identity.sourceReceiptSha256,
    inputSha256: identity.inputSha256,
    rowCount: sourceRows.length,
    rows: sourceRows.map((row, index) => ({
      validAt: row.validAt,
      leadHours: row.leadHours,
      sourceRowSha256: identity.sourceRowSha256[index],
      candidateSpeedMps64: encodeMaintenanceBinary64(1),
      candidateGustMps64: index + 1 >= 49 && index + 1 <= 72
        ? null
        : encodeMaintenanceBinary64(2),
      speedWouldApply: true,
      gustWouldApply: !(index + 1 >= 49 && index + 1 <= 72),
    })),
  }, sourceProjection);
  return { body, sourceProjection };
}

// create one canonical ordinary stage receipt
function shadowStageReceipt(input) {
  const unsigned = {
    contractVersion: "adjustment-shadow-stage-receipt/v1",
    durable: true,
    durableAt: "2026-09-14T00:36:00.000Z",
    dueKey: input.metadata.dueKey,
    predictionBodySha256: input.metadata.predictionBodySha256,
    registrationSha256: input.metadata.registrationSha256,
    sourceProjectionSha256: input.sourceProjectionSha256,
  };
  return { ...unsigned, stageReceiptSha256: hash(`${JSON.stringify(unsigned)}\n`) };
}

// create one canonical ordinary publish receipt
function shadowPublishReceipt(input) {
  const unsigned = {
    committed: true,
    committedAt: "2026-09-14T00:40:00.000Z",
    contractVersion: "adjustment-shadow-publish-receipt/v1",
    predictionSha256: input.metadata.predictionSha256,
  };
  return { ...unsigned, publishReceiptSha256: hash(`${JSON.stringify(unsigned)}\n`) };
}
const queryable = {
  // return one family-bound value-free schedule slot
  async query(sql, values) {
    // expose the authoritative current revision receipt on publication
    if (sql.includes("adjustment_weather_revision_admission_v1")) {
      publicationQueries.set("revision", { sql, values });
      publicationEvents.push("revision-admit");
      // simulate a transient database read failure
      if (revisionReadFailed) {
        throw new Error("fixture pointer read unavailable");
      }
      return { rows: [{ value: authoritativeRevisionReceipt }] };
    }
    // record the permanent live disposition before cold gap archival
    if (sql.includes("weather_mark_adjustment_revision_gap_v1")) {
      publicationQueries.set("revision-gap", { sql, values });
      const [relation, encodedKey, encodedGap] = values;
      assert.equal(relation, "weather_record");
      const key = JSON.parse(encodedKey);
      const gap = JSON.parse(encodedGap);
      publicationEvents.push(`revision-mark:${gap.reason}`);
      return { rows: [{ value: {
        ...gap,
        contractVersion: "adjustment-revision-gap-marker/v1",
        storedContentSha256: key.storedContentSha256,
      } }] };
    }
    // expose exact ordinary body and revision admissions before publication
    if (sql.includes("adjustment_shadow_body_admission_v2")) {
      publicationQueries.set("shadow-body", { sql, values });
      publicationEvents.push("shadow-admit");
      return { rows: [{ admitted: shadowBodyCommitted }] };
    }
    if (sql.includes("adjustment_shadow_revision_admission_v1")) {
      publicationQueries.set("shadow-receipt", { sql, values });
      publicationEvents.push("shadow-revision-admit");
      return { rows: [{ value: authoritativeShadowReceipt }] };
    }
    queries.push({ sql, values });
    return {
      rows: [{
        value: {
          contractVersion: "adjustment-shadow-registration-slot/v3",
          epochWitnessSha256: "d".repeat(64),
          family: values[0],
          horizonEndAt: "2028-08-26T07:00:00.000Z",
          registrationSha256: null,
          scheduleContractSha256: "e".repeat(64),
          state: "free",
          terminalAt: null,
        },
      }],
    };
  },
};
const relay = {
  // capture the ordinary value-free gap callback
  async recordGap(gap) {
    gaps.push(gap);
  },
  // capture one value-free permanent revision gap
  async recordRevisionGap(gap) {
    revisionGaps.push(gap);
    publicationEvents.push(`revision-archive-gap:${gap.reason}`);
  },
  // retain one canonical future-only rain control state
  async stageRainControlState(state) {
    publicationEvents.push("rain-control-state-stage");
    const unsigned = {
      contractVersion: "adjustment-rain-control-state-stage-receipt/v1",
      durable: true,
      durableAt: "2026-09-14T00:36:00.000Z",
      stateSha256: hash(state),
    };
    return {
      ...unsigned,
      stageReceiptSha256: hash(`${JSON.stringify(unsigned)}\n`),
    };
  },
  // capture one value-free rain control-state gap
  async recordRainControlStateGap(gap) {
    rainControlStateGaps.push(gap);
  },
  // retain one permanent fixed-gauge target disposition
  async recordRainFixedGaugeTargetGap(gap) {
    const unsigned = {
      contractVersion: "adjustment-rain-fixed-gauge-target-gap/v1",
      gapAt: "2026-09-14T01:00:01.000Z",
      logicalHourAt: gap.logicalHourAt,
      logicalKeySha256: gap.logicalKeySha256,
      qualificationDisposition: "forever_unqualified",
      reason: gap.reason,
    };
    retainedFixedGaugeGap = {
      ...unsigned,
      gapSha256: hash(`${JSON.stringify(unsigned)}\n`),
    };
    return retainedFixedGaugeGap;
  },
  // read the retained permanent fixed-gauge disposition
  async readRainFixedGaugeTargetGap(key) {
    assert.equal(retainedFixedGaugeGap.logicalHourAt, key.logicalHourAt);
    assert.equal(retainedFixedGaugeGap.logicalKeySha256, key.logicalKeySha256);
    return {
      contractVersion: "adjustment-rain-fixed-gauge-target-gap-status/v1",
      gap: retainedFixedGaugeGap,
      state: "present",
    };
  },
  // retain exact ordinary bytes before database admission
  async stage(input) {
    publicationEvents.push("shadow-stage");
    return shadowStageReceipt(input);
  },
  // publish ordinary bytes only after both database admission reads
  async publish(input) {
    publicationEvents.push("shadow-publish");
    assert.deepEqual(input.revisionReceipt, authoritativeShadowReceipt);
    return shadowPublishReceipt(input);
  },
  // retain exact revision bytes before the database admission phase
  async stageRevision(projection) {
    publicationEvents.push("revision-stage");
    return revisionStageReceipt(projection);
  },
  // publish only the database-authenticated revision receipt
  async publishRevision(input) {
    publicationEvents.push("revision-publish");
    assert.deepEqual(input.revisionReceipt, authoritativeRevisionReceipt);
    // simulate an ordinary archive write failure
    if (revisionPublishFailed) {
      throw new Error("fixture archive temporarily unavailable");
    }
    return revisionPublishReceipt(input.revisionReceipt);
  },
};
const server = createAdjustmentMaintenanceInternalServer({
  captureEpoch,
  // capture the fixed admitted scheduler request
  async capture(request) {
    captures.push(request);
  },
  queryable,
  relay,
});

// invoke the compiled request listener without opening a socket
async function invoke(target, method, url, value, rawBody) {
  const body = rawBody ?? (value === undefined ? undefined : JSON.stringify(value));
  const request = Readable.from(body === undefined ? [] : [body]);
  request.method = method;
  request.url = url;
  return await new Promise((resolve) => {
    const response = {
      // retain the exact status and headers chosen by the handler
      writeHead(status, headers) {
        this.status = status;
        this.headers = headers;
      },
      // finish one bounded JSON response
      end(payload) {
        resolve({
          body: JSON.parse(String(payload)),
          headers: this.headers,
          status: this.status,
        });
      },
    };
    target.emit("request", request, response);
  });
}

const schedule = await invoke(
  server,
  "GET",
  "/internal/adjustment-maintenance/schedule-slots",
);
assert.equal(schedule.status, 200);
assert.deepEqual(schedule.body.map((slot) => slot.family), [
  "temperature",
  "wind",
  "rain",
]);
assert.deepEqual(queries.map((query) => query.values), [
  ["temperature"],
  ["wind"],
  ["rain"],
]);

const capture = {
  dueKey: "capture/2028-08-26T06:35:00.000Z",
  issuedAt: "2028-08-26T06:35:00.000Z",
};
const accepted = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/shadow/capture",
  capture,
);
assert.deepEqual(accepted, {
  body: { accepted: true },
  headers: {
    "content-length": 17,
    "content-type": "application/json",
  },
  status: 202,
});
assert.deepEqual(captures, [capture]);

const gap = {
  dueKey: capture.dueKey,
  family: "wind",
  reason: "candidate_unavailable",
  registrationSha256: "a".repeat(64),
};
const recorded = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/shadow/gap",
  gap,
);
assert.equal(recorded.status, 200);
assert.deepEqual(recorded.body, { recorded: true });
assert.deepEqual(gaps, [gap]);

const revisionGap = {
  logicalKeySha256: "b".repeat(64),
  projectionIdentitySha256: null,
  projectionKind: "actual_best_match",
  projectionSha256: null,
  reason: "archive_stage_failed",
};
const recordedRevisionGap = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/revision/gap",
  revisionGap,
);
assert.equal(recordedRevisionGap.status, 200);
assert.deepEqual(recordedRevisionGap.body, { recorded: true });
assert.deepEqual(revisionGaps, [revisionGap]);
publicationEvents.length = 0;

const rainControlState = rainControlStateBytes();
const stagedRainControlState = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/rain-control-state/stage",
  { stateBase64: rainControlState.toString("base64") },
);
assert.equal(stagedRainControlState.status, 200);
assert.equal(
  stagedRainControlState.body.stateSha256,
  hash(rainControlState),
);
assert.deepEqual(publicationEvents, ["rain-control-state-stage"]);
publicationEvents.length = 0;

const rainControlStateGap = {
  reason: "projection_stage_failed",
  stateSha256: stagedRainControlState.body.stateSha256,
  stageReceiptSha256: stagedRainControlState.body.stageReceiptSha256,
};
const recordedRainControlStateGap = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/rain-control-state/gap",
  rainControlStateGap,
);
assert.equal(recordedRainControlStateGap.status, 200);
assert.deepEqual(recordedRainControlStateGap.body, { recorded: true });
assert.deepEqual(rainControlStateGaps, [rainControlStateGap]);

const fixedGaugeTargetGap = {
  logicalHourAt: "2026-09-14T01:00:00.000Z",
  logicalKeySha256: "c".repeat(64),
  reason: "target_source_oversized",
};
const recordedFixedGaugeTargetGap = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap",
  fixedGaugeTargetGap,
);
assert.equal(recordedFixedGaugeTargetGap.status, 200);
assert.equal(
  recordedFixedGaugeTargetGap.body.qualificationDisposition,
  "forever_unqualified",
);
assert.deepEqual(recordedFixedGaugeTargetGap.body, retainedFixedGaugeGap);

const fixedGaugeTargetStatus = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status",
  {
    logicalHourAt: fixedGaugeTargetGap.logicalHourAt,
    logicalKeySha256: fixedGaugeTargetGap.logicalKeySha256,
  },
);
assert.equal(fixedGaugeTargetStatus.status, 200);
assert.equal(fixedGaugeTargetStatus.body.state, "present");
assert.deepEqual(fixedGaugeTargetStatus.body.gap, retainedFixedGaugeGap);

const shadow = ordinaryShadowMaterial();
const stagedShadow = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/shadow/stage",
  {
    bodyBase64: shadow.body.toString("base64"),
    sourceProjectionBase64: shadow.sourceProjection.toString("base64"),
  },
);
assert.equal(stagedShadow.status, 200);
const shadowMetadata = createMaintenanceShadowPredictionMetadata(
  shadow.body,
  shadow.sourceProjection,
);
authoritativeShadowReceipt = {
  archiveCommitOrdinal: "8",
  archiveCommittedAt: "2026-09-14T00:38:00.000Z",
  contractVersion: "adjustment-revision-commit-receipt/v1",
  frontierSha256: hash("shadow-frontier"),
  predecessorFrontierSha256: hash("shadow-predecessor"),
  projectionIdentitySha256: shadowMetadata.sourceReceiptSha256,
  projectionKind: "shadow_prediction",
  projectionSha256: shadowMetadata.inputSha256,
  receiptSha256: hash("shadow-database-receipt"),
  stageReceiptSha256: stagedShadow.body.stageReceiptSha256,
};
const publishedShadow = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/shadow/publish",
  {
    bodyBase64: shadow.body.toString("base64"),
    revisionReceipt: authoritativeShadowReceipt,
    sourceProjectionBase64: shadow.sourceProjection.toString("base64"),
    stageReceipt: stagedShadow.body,
  },
);
assert.equal(publishedShadow.status, 200);
assert.equal(publishedShadow.body.contractVersion, "adjustment-shadow-publish-receipt/v1");
assert.deepEqual(publicationEvents, [
  "shadow-stage",
  "shadow-admit",
  "shadow-revision-admit",
  "shadow-publish",
]);
publicationEvents.length = 0;

const projection = revisionProjection();
const stagedRevision = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/revision/stage",
  { projectionBase64: projection.toString("base64") },
);
assert.equal(stagedRevision.status, 200);
const stageReceipt = revisionStageReceipt(projection);
assert.deepEqual(stagedRevision.body, stageReceipt);
authoritativeRevisionReceipt = {
  archiveCommitOrdinal: "9",
  archiveCommittedAt: "2026-09-14T00:12:00.000Z",
  contractVersion: "adjustment-revision-commit-receipt/v1",
  frontierSha256: hash("frontier"),
  predecessorFrontierSha256: hash("predecessor"),
  projectionIdentitySha256: stageReceipt.projectionIdentitySha256,
  projectionKind: "actual_best_match",
  projectionSha256: stageReceipt.projectionSha256,
  receiptSha256: hash("database-receipt"),
  stageReceiptSha256: stageReceipt.stageReceiptSha256,
};
const publishedRevision = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/revision/publish",
  {
    projectionBase64: projection.toString("base64"),
    revisionReceipt: authoritativeRevisionReceipt,
    stageReceipt,
  },
);
assert.equal(publishedRevision.status, 200);
assert.deepEqual(
  publishedRevision.body,
  revisionPublishReceipt(authoritativeRevisionReceipt),
);
assert.deepEqual(publicationEvents, [
  "revision-stage",
  "revision-admit",
  "revision-publish",
]);

const revisionRequest = {
  projectionBase64: projection.toString("base64"),
  revisionReceipt: authoritativeRevisionReceipt,
  stageReceipt,
};

// retain the causal order of ordinary read or archive publication failures
async function revisionFailureTrace() {
  publicationEvents.length = 0;
  const failed = await invoke(
    server, "POST", "/internal/adjustment-maintenance/revision/publish", revisionRequest,
  );
  assert.equal(failed.status, 409);
  assert.deepEqual(failed.body, { error: "shadow_rejected" });
  return publicationEvents.join(",");
}

revisionReadFailed = true;
const readFailureTrace = await revisionFailureTrace();
revisionReadFailed = false;
assert.equal(readFailureTrace, [
  "revision-admit",
  "revision-mark:database_admission_failed",
  "revision-archive-gap:database_admission_failed",
].join(","));
revisionPublishFailed = true;
const publishFailureTrace = await revisionFailureTrace();
revisionPublishFailed = false;
assert.equal(publishFailureTrace, [
  "revision-admit",
  "revision-publish",
  "revision-mark:archive_publish_failed",
  "revision-archive-gap:archive_publish_failed",
].join(","));
assert.equal(revisionGaps.at(-1).projectionIdentitySha256, stageReceipt.projectionIdentitySha256);

publicationEvents.length = 0;
shadowBodyCommitted = false;
const uncommittedShadow = await invoke(server, "POST", "/internal/adjustment-maintenance/shadow/publish", {
  bodyBase64: shadow.body.toString("base64"),
  revisionReceipt: authoritativeShadowReceipt,
  sourceProjectionBase64: shadow.sourceProjection.toString("base64"),
  stageReceipt: stagedShadow.body,
});
assert.equal(uncommittedShadow.status, 409);
assert.deepEqual(publicationEvents, ["shadow-admit"]);
assert.deepEqual(
  publicationQueries.get("shadow-body").values,
  publicationQueries.get("shadow-receipt").values,
);

// capture the other three API-role lookup plans through their actual repository functions
for (const [label, read, key] of [
  ["forecast-anchor", readAdjustmentForecastAnchorRevisionAdmission, {
    leadHours: 24,
    sourceId: "7",
    storedContentSha256: "1".repeat(64),
    validAt: "2026-09-14T01:00:00.000Z",
  }],
  ["rain-gate", readAdjustmentRainGateRevisionAdmission, {
    inputSha256: "2".repeat(64),
    modelSha256: "3".repeat(64),
    runInitializedAt: "2026-09-14T00:00:00.000Z",
    storedContentSha256: "1".repeat(64),
  }],
  ["ecmwf-temperature", readAdjustmentEcmwfTemperatureRevisionAdmission, {
    providerResponseSha256: "4".repeat(64),
    runInitializedAt: "2026-09-14T00:00:00.000Z",
    siteId: "7",
    storedContentSha256: "1".repeat(64),
  }],
]) {
  const selected = await read({
    // assert the exact single JSON body before returning the fixture receipt
    async query(sql, values) {
      assert.deepEqual(values.map(
        // decode each real JSON bind
        (value) => JSON.parse(value),
      ), [key]);
      publicationQueries.set(label, { sql, values });
      return { rows: [{ value: authoritativeRevisionReceipt }] };
    },
  }, key);
  assert.deepEqual(selected, authoritativeRevisionReceipt);
}

// emit only the exact SQL and value-free receipt material compared by Rust
for (const [label, query] of publicationQueries) {
  console.log(`${label}-sql=${query.sql.trim().replace(/\s+/gu, " ")}`);
}
console.log(`shadow-body-binds=${publicationQueries.get("shadow-body").values.join("|")}`);
console.log(`revision-body-sha256=${stageReceipt.projectionIdentitySha256}`);
console.log(`revision-logical-sha256=${revisionGaps.at(-1).logicalKeySha256}`);
console.log(`revision-read-failure=${readFailureTrace}`);
console.log(`revision-publish-failure=${publishFailureTrace}`);

const stale = await invoke(
  server,
  "POST",
  "/internal/adjustment-maintenance/shadow/capture",
  {
    dueKey: "capture/1999-12-31T18:35:00.000Z",
    issuedAt: "1999-12-31T18:35:00.000Z",
  },
);
assert.equal(stale.status, 409);
assert.deepEqual(stale.body, { error: "shadow_rejected" });

const inactive = createAdjustmentMaintenanceInternalServer({
  captureEpoch,
  queryable,
  relay,
});
const inactiveCapture = await invoke(
  inactive,
  "POST",
  "/internal/adjustment-maintenance/shadow/capture",
  capture,
);
assert.equal(inactiveCapture.status, 409);
assert.deepEqual(inactiveCapture.body, { error: "shadow_rejected" });

const notFound = await invoke(
  server,
  "GET",
  "/internal/adjustment-maintenance/shadow/capture",
  undefined,
  "not-json",
);
assert.equal(notFound.status, 404);
assert.deepEqual(notFound.body, { error: "not_found" });

process.stdout.write("maintenance private service Node oracle passed\n");
