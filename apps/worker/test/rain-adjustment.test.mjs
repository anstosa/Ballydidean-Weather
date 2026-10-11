import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";
import {
  createRainAdjustmentEvaluation,
  createRainAdjustmentPersistenceTarget,
  createRainAdjustmentRevisionArchiver,
  createRainAdjustmentRun,
  buildRainFixedGaugeTargetProjection,
  publishRainAdjustment,
  publishRainAdjustmentMaintenanceShadow,
  rainForecastProfile,
  rainStationHours,
  stageAdjustmentRevisionWithBackpressure,
  stageRainControlStateWithBackpressure,
} from "../dist/rain-adjustment.js";
import { publishRainFixedGaugeTarget } from "../dist/rain-fixed-gauge-target-producer.js";
import {
  canonicalJsonBytes,
  createRainMaintenanceControlState,
  encodeRainMaintenanceControlState,
  parseAdjustmentRainGateControlProjection,
  RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256,
  createMaintenanceShadowServingAuthority,
  parseAdjustmentRainGateFeatureProjection,
  adjustmentRainFixedGaugeTargetActual,
  adjustmentRainFixedGaugeTargetLogicalKeySha256,
  parseAdjustmentRainFixedGaugeTargetProjection,
  parseMaintenanceShadowComparator,
  parseMaintenanceShadowSourceProjection,
  parseMaintenanceShadowValues,
} from "../../../packages/forecast-adjustment/dist/index.js";
import { RAIN_COLLECTION_STATIONS, weatherRecordContent } from "../../../packages/domain/dist/index.js";
import {
  RAIN_HURDLE_WIND_ARTIFACT_JSON,
  RAIN_HURDLE_WIND_ARTIFACT_SHA256,
} from "../../../packages/forecast-adjustment/dist/rain-hurdle-wind-artifact.js";

const decisionAt = "2026-09-14T08:00:00.000Z";
const captureEpoch = Object.freeze({ epochAt: "2000-01-01T00:00:00.000Z" });

// remove incumbent-only provenance from one inactive portable artifact
function portableRainArtifact() {
  const { nativeModelSha256: _native, provenanceSha256: _provenance, ...artifact } =
    JSON.parse(RAIN_HURDLE_WIND_ARTIFACT_JSON);
  return artifact;
}

// preserve exact minute intervals in an overlapping two-hour provider window
function stationCapture({ missing = -1, received = "2026-09-14T07:03:00.000Z", amount = 0.01 } = {}) {
  const start = Date.parse("2026-09-14T05:00:00.000Z");
  const obs = [];
  // emit one complete minute-by-minute response without daily counter substitution
  for (let minute = 1; minute <= 120; minute += 1) {
    // leave one requested gap explicit
    if (minute === missing) continue;
    obs.push([(start + minute * 60_000) / 1000, 0, 1, 2, 180, 3, 1000, 12, 80,
      0, 0, 0, amount, 0, 0, 0, 2.7, 1, 0, 0, 0, 0]);
  }
  const body = Buffer.from(JSON.stringify({ status: { status_code: 0 }, type: "obs_st", device_id: 175727, obs }));
  return { claimId: "gauge", kind: "station", stationId: 64255, runInitializedAt: null,
    windowStart: "2026-09-14T05:00:01.000Z", windowEndExclusive: "2026-09-14T07:00:01.000Z",
    completedAt: received, bodySha256: createHash("sha256").update(body).digest("hex"), body };
}

// construct one fixed-station raw response spanning a complete target hour
function targetStationCapture(station, {
  claimId = `target-${station.locationId}`,
  completedAt = "2026-09-14T07:10:00.000Z",
  missing = -1,
  padding = 0,
} = {}) {
  const start = Date.parse("2026-09-14T05:59:00.000Z");
  const obs = [];
  // retain five minutes beyond the requested hour for backward endpoint tests
  for (let minute = 1; minute <= 65; minute += 1) {
    if (minute === missing) continue;
    obs.push([(start + minute * 60_000) / 1000, 0, 1, 2, 180, 3, 1000, 12, 80,
      0, 0, 0, 0.01, 0, 0, 0, 2.7, 1, 0, 0, 0, 0]);
  }
  const body = Buffer.from(JSON.stringify({
    device_id: station.deviceId,
    obs,
    padding: "x".repeat(padding),
    status: { status_code: 0 },
    type: "obs_st",
  }));
  return {
    body,
    bodySha256: createHash("sha256").update(body).digest("hex"),
    claimId,
    completedAt,
    kind: "station",
    runInitializedAt: null,
    stationId: station.locationId,
    windowEndExclusive: "2026-09-14T07:05:00.000Z",
    windowStart: "2026-09-14T05:59:00.000Z",
  };
}

// provision the dedicated database source identity for every fixed rain gauge
function targetSources() {
  return RAIN_COLLECTION_STATIONS.map((station) => ({
    sourceId: `rain-target-tempest-${station.locationId}`,
    stationId: station.locationId,
  }));
}

// represent the owner-provisioned immutable source catalog
function targetSourceCatalog() {
  return {
    catalogSha256: "a".repeat(64),
    contractVersion: "adjustment-rain-fixed-gauge-target-source-catalog/v1",
    runContractSha256: "b".repeat(64),
    sources: targetSources().map((source) => ({
      locationId: source.stationId,
      sourceConfigFingerprint: "c".repeat(64),
      sourceId: source.sourceId,
      sourceKey: source.sourceId,
    })),
  };
}

// construct a source-shaped profile independent of the native model fixtures
function forecastCapture() {
  const runInitializedAt = "2026-09-14T00:00:00.000Z";
  const hourly = { time: [], temperature_2m: [], relative_humidity_2m: [], cloud_cover: [],
    surface_pressure: [], wind_speed_10m: [], wind_direction_10m: [], precipitation: [] };
  // retain all 49 provider timestamps including initialization
  for (let lead = 0; lead <= 48; lead += 1) {
    hourly.time.push(new Date(Date.parse(runInitializedAt) + lead * 3_600_000).toISOString().slice(0, 16));
    hourly.temperature_2m.push(12); hourly.relative_humidity_2m.push(80);
    hourly.cloud_cover.push(90); hourly.surface_pressure.push(1000);
    hourly.wind_speed_10m.push(4); hourly.wind_direction_10m.push(180);
    hourly.precipitation.push(0.5);
  }
  const body = Buffer.from(JSON.stringify({ hourly, utc_offset_seconds: 0 }));
  return { claimId: "forecast", kind: "forecast", stationId: null, runInitializedAt,
    windowStart: null, windowEndExclusive: null, completedAt: "2026-09-14T06:01:00.000Z",
    bodySha256: createHash("sha256").update(body).digest("hex"), body };
}

// freeze one rain registration independently of serving activation
function maintenanceRegistration(artifactSha256 = RAIN_HURDLE_WIND_ARTIFACT_SHA256) {
  return {
    artifactSha256, candidateSha256: "2".repeat(64),
    cohortSha256: "3".repeat(64), family: "rain",
    intervalEndAt: "2026-10-01T00:00:00.000Z",
    intervalStartAt: "2026-09-01T00:00:00.000Z",
    policySha256: "4".repeat(64), registrationSha256: "5".repeat(64),
    reservedKeySha256: "6".repeat(64), siteKey: "ballydidean",
    sourceSha256: "7".repeat(64), targetCutoffAt: "2026-10-08T00:00:00.000Z",
    terminalAt: "2026-10-09T00:00:00.000Z",
  };
}

// bind the test registration to one installed action, bundle and root receipt
function maintenanceCandidate(
  artifact = portableRainArtifact(),
  registration = maintenanceRegistration(createHash("sha256").update(canonicalJsonBytes(artifact)).digest("hex")),
) {
  return {
    action: { candidateGraphSha256: "a".repeat(64), candidateSha256: registration.candidateSha256 },
    bundle: artifact,
    catalogSha256: "b".repeat(64), family: "rain",
    receipt: { actionSha256: "c".repeat(64), bundleSha256: registration.artifactSha256,
      candidateGraphSha256: "a".repeat(64), candidateSha256: registration.candidateSha256,
      contractVersion: "adjustment-installed-candidate-receipt/v1",
      controlPlaneSha256: "d".repeat(64), deployedCommit: "1".repeat(40),
      deployedImageDigest: `sha256:${"e".repeat(64)}`, deployedRelease: "2026.09.14-1",
      deployedSettingsSha256: "f".repeat(64), fencingToken: "1",
      installedAt: "2026-09-13T23:55:00.000Z",
      registrationSha256: registration.registrationSha256,
      sourceSha256: registration.sourceSha256 },
    registration,
  };
}

// bind one monthly control state and fixed ordinal runtime to a rolling candidate
function maintenanceControlCandidate(epochWitnessSha256, scheduleContractSha256) {
  const ordinalArtifact = { ...portableRainArtifact(), modelMonth: "2026-09" };
  const ordinalArtifactBytes = Buffer.from(canonicalJsonBytes(ordinalArtifact));
  const ordinalArtifactSha256 = createHash("sha256").update(ordinalArtifactBytes).digest("hex");
  const state = createRainMaintenanceControlState({
    calibrationEndAt: "2026-08-25T00:00:00.000Z",
    calibrationStartAt: "2026-05-27T00:00:00.000Z",
    contractVersion: "rain-maintenance-control-state/v1",
    epochWitnessSha256,
    generatedAt: "2026-08-26T00:00:00.000Z",
    legacyCalibrationStartAt: "2026-07-11T00:00:00.000Z",
    legacyRawScale: 1.1,
    modelMonth: "2026-09",
    ordinalArtifactSha256,
    recentFallbackReason: "recent_calibration",
    recentRawScale: 1.2,
    recentSupported: true,
    recipeSha256: RAIN_MAINTENANCE_CONTROL_RECIPE_SHA256,
    sameWindowRawScale: 1.15,
    scheduleContractSha256,
    sourceMemberRootSha256: "8".repeat(64),
    sourceReceiptRootSha256: "9".repeat(64),
    support: {
      calibrationDates: 90, calibrationHours: 2_000, calibrationRows: 2_000,
      calibrationWetDates: 20, calibrationWetHours: 200,
      effectiveDates64: "404e000000000000", effectiveWetDates64: "4008000000000000",
      legacyCalibrationRows: 500, legacyTrainingRows: 2_000, legacyTrainingWetRows: 200,
      trainingDates: 200, trainingHours: 4_000, trainingRows: 4_000,
      trainingWetDates: 30, trainingWetHours: 300,
    },
    trainingMaximumValidAt: "2026-05-19T23:00:00.000Z",
  });
  const controlStateBytes = encodeRainMaintenanceControlState(state);
  const base = maintenanceCandidate();
  return {
    ...base,
    controlStateBytes,
    controlStateSha256: createHash("sha256").update(controlStateBytes).digest("hex"),
    ordinalArtifactBytes,
    ordinalArtifactSha256,
    registration: {
      ...base.registration,
      epochWitnessSha256,
      predecessorRegistrationSha256: null,
      scheduleContractSha256,
    },
  };
}

// preserve the exact compiled incumbent artifact and active registry selection
function rainIncumbentRuntime() {
  const receiptBytes = Buffer.from(`${JSON.stringify({
    activeArtifact: { artifactSha256: RAIN_HURDLE_WIND_ARTIFACT_SHA256 },
    contractVersion: "forecast-adjustment-rain-runtime-registry/v1",
    rawReason: null,
    siteKey: "ballydidean",
  })}\n`);
  return {
    artifactSha256: RAIN_HURDLE_WIND_ARTIFACT_SHA256,
    comparatorAuthority: createMaintenanceShadowServingAuthority({
      artifactBytes: Buffer.from(RAIN_HURDLE_WIND_ARTIFACT_JSON),
      artifactIdentitySha256: RAIN_HURDLE_WIND_ARTIFACT_SHA256,
      authorityKind: "legacy_active",
      family: "rain",
      receiptBytes,
    }),
    reasonCode: null,
    state: "active",
  };
}

// preserve an explicit root-selected raw registry as incumbent authority
function rainRawIncumbentRuntime() {
  const receiptBytes = Buffer.from(`${JSON.stringify({
    activeArtifact: null,
    contractVersion: "forecast-adjustment-rain-runtime-registry/v1",
    rawReason: "policy_raw",
    siteKey: "ballydidean",
  })}\n`);
  return {
    artifactSha256: null,
    comparatorAuthority: createMaintenanceShadowServingAuthority({
      artifactBytes: null,
      artifactIdentitySha256: null,
      authorityKind: "policy_raw",
      family: "rain",
      receiptBytes,
    }),
    reasonCode: "policy_raw",
    state: "disabled",
  };
}

// emulate only the rain registration and compact append database boundaries
function maintenanceDatabase(events) {
  return {
    async query(sql, values) {
      if (sql.includes("weather_register_adjustment_shadow_v2")) {
        events.push("register");
        return { rows: [{ value: { inserted: false,
          registrationSha256: maintenanceRegistration().registrationSha256 } }] };
      }
      if (sql.includes("weather_append_adjustment_rain_shadow_v2")) {
        events.push("append");
        const metadata = JSON.parse(values[0]);
        return { rows: [{ value: { committedAt: "2026-09-14T08:06:00.000Z",
          inserted: true, predictionSha256: metadata.predictionSha256,
          revisionReceipt: {
            archiveCommitOrdinal: "1",
            archiveCommittedAt: "2026-09-14T08:06:00.000Z",
            contractVersion: "adjustment-revision-commit-receipt/v1",
            frontierSha256: "a".repeat(64),
            predecessorFrontierSha256: "b".repeat(64),
            projectionIdentitySha256: metadata.sourceReceiptSha256,
            projectionKind: "shadow_prediction",
            projectionSha256: metadata.inputSha256,
            receiptSha256: "c".repeat(64),
            stageReceiptSha256: metadata.stageReceiptSha256,
          } } }] };
      }
      throw new Error(`unexpected query: ${sql}`);
    },
  };
}

// emulate the worker-to-api relay with exact durable receipt binding
function maintenanceClient(events, { failStage = false } = {}) {
  return {
    async recordGap(input) { events.push(`gap:${input.reason}`); },
    async stage(sourceBytes, bodyBytes, comparatorBytes) {
      events.push("stage");
      if (failStage) throw new Error("archive unavailable");
      const source = parseMaintenanceShadowSourceProjection(sourceBytes);
      const body = parseMaintenanceShadowValues(bodyBytes);
      const comparator = parseMaintenanceShadowComparator(comparatorBytes);
      events.push({ body, comparator, source });
      const unsigned = { contractVersion: "adjustment-shadow-stage-receipt/v2", durable: true,
        durableAt: "2026-09-14T08:05:30.000Z", dueKey: body.dueKey,
        predictionBodySha256: createHash("sha256").update(bodyBytes).digest("hex"),
        registrationSha256: body.registrationSha256,
        sourceProjectionSha256: createHash("sha256").update(sourceBytes).digest("hex"),
        comparatorSha256: createHash("sha256").update(comparatorBytes).digest("hex") };
      return { ...unsigned, stageReceiptSha256: createHash("sha256")
        .update(`${JSON.stringify(unsigned)}\n`).digest("hex") };
    },
    async publish(_source, _body, _comparator, predictionCommittedAt) {
      assert.equal(predictionCommittedAt, "2026-09-14T08:06:00.000Z");
      events.push("publish");
    },
  };
}

// complete hours require all reported intervals and retain actual receipt availability
test("rain station aggregation deduplicates overlaps without filling interval gaps", () => {
  const first = stationCapture();
  const later = stationCapture({ received: "2026-09-14T07:10:00.000Z" });
  const full = rainStationHours([first, later], decisionAt);
  const hour = full.find((item) => item.hourAt === "2026-09-14T07:00:00.000Z");
  assert.ok(Math.abs(hour.precipitationMm - 0.6) < 1e-12);
  assert.equal(hour.receivedAt, first.completedAt);
  assert.equal(hour.temperatureC, 12);
  assert.equal(full.some((item) => item.stationId !== 64255), false);
  const missing = rainStationHours([stationCapture({ missing: 90 })], decisionAt);
  assert.equal(missing.find((item) => item.hourAt === hour.hourAt).precipitationMm, null);
});

// future reports cannot repair a historical decision and conflicts cannot be cherry-picked
test("rain aggregation excludes late receipts and rejects contradictory gauge revisions", () => {
  assert.deepEqual(rainStationHours([stationCapture({ received: "2026-09-14T08:01:00.000Z" })], decisionAt), []);
  assert.throws(() => rainStationHours([stationCapture(), stationCapture({ amount: 0.02 })], decisionAt), /conflicting/u);
});

test("fixed-gauge target archives raw bodies once and native weather content", () => {
  const captures = RAIN_COLLECTION_STATIONS.map((station) => targetStationCapture(station));
  captures.push({
    ...captures[0],
    claimId: "target-64255-overlap",
    completedAt: "2026-09-14T07:11:00.000Z",
  });
  const built = buildRainFixedGaugeTargetProjection({
    captures,
    sources: targetSources(),
    targetCutoffAt: "2026-09-14T08:00:00.000Z",
    validAt: "2026-09-14T07:00:00.000Z",
  });
  assert.equal(built.state, "complete");
  const parsed = parseAdjustmentRainFixedGaugeTargetProjection(built.bytes);
  assert.equal(parsed.captureBodies.length, 12);
  assert.equal(parsed.captureBodies.find((body) => body.bodySha256 === captures[0].bodySha256).claims.length, 2);
  assert.equal(parsed.rows.length, 12);
  assert.equal(parsed.rows[0].intervals.length, 60);
  assert.equal(parsed.rows[0].normalizedRecord.metrics.precipitationMm, 0.6000000000000003);
  assert.equal(parsed.rows[0].storedContentSha256,
    createHash("sha256").update(weatherRecordContent(parsed.rows[0].normalizedRecord)).digest("hex"));
  const actual = adjustmentRainFixedGaugeTargetActual(parsed);
  assert.equal(actual.gaugeCount, 12);
  assert.equal(actual.target, parsed.rows[0].normalizedRecord.metrics.precipitationMm);
  assert.equal(actual.firstEdgeCommittedAt, "2026-09-14T07:11:00.000Z");
  assert.match(adjustmentRainFixedGaugeTargetLogicalKeySha256({
    sourceIds: parsed.rows.map((row) => row.normalizedRecord.sourceId),
    validAt: parsed.validAt,
  }), /^[a-f0-9]{64}$/u);

  const tampered = JSON.parse(built.bytes.toString("utf8"));
  tampered.captureBodies[0].bodyBase64 = `${tampered.captureBodies[0].bodyBase64.slice(0, -4)}AAAA`;
  assert.throws(() => parseAdjustmentRainFixedGaugeTargetProjection(
    Buffer.from(canonicalJsonBytes(tampered)),
  ), /body identity/u);
});

test("fixed-gauge target preserves gaps and refuses oversized raw custody", () => {
  const missing = RAIN_COLLECTION_STATIONS.map((station, index) =>
    targetStationCapture(station, { missing: index === 0 ? 30 : -1 }));
  const built = buildRainFixedGaugeTargetProjection({
    captures: missing,
    sources: targetSources(),
    targetCutoffAt: "2026-09-14T08:00:00.000Z",
    validAt: "2026-09-14T07:00:00.000Z",
  });
  assert.equal(built.state, "complete");
  const parsed = parseAdjustmentRainFixedGaugeTargetProjection(built.bytes);
  assert.equal(parsed.rows[0].normalizedRecord.metrics.precipitationMm, null);
  assert.deepEqual(parsed.rows[0].intervals, []);
  assert.equal(adjustmentRainFixedGaugeTargetActual(parsed).gaugeCount, 11);

  const oversized = RAIN_COLLECTION_STATIONS.map((station) =>
    targetStationCapture(station, { padding: 300_000 }));
  assert.deepEqual(buildRainFixedGaugeTargetProjection({
    captures: oversized,
    sources: targetSources(),
    targetCutoffAt: "2026-09-14T08:00:00.000Z",
    validAt: "2026-09-14T07:00:00.000Z",
  }), {
    logicalHourAt: "2026-09-14T07:00:00.000Z",
    reason: "target_source_oversized",
    state: "gap",
  });
});

test("fixed-gauge target stages before one ordered twelve-receipt publication", async () => {
  const events = [];
  const captures = RAIN_COLLECTION_STATIONS.map((station) => targetStationCapture(station));
  let staged;
  let group;
  const result = await publishRainFixedGaugeTarget({}, captureEpoch, {
    async publishRevisionBatch(bytes, stageReceipt, receipts) {
      events.push("publish");
      assert.equal(Buffer.compare(Buffer.from(bytes), staged), 0);
      assert.equal(stageReceipt.stageReceiptSha256, group.stageReceiptSha256);
      assert.equal(receipts.length, 12);
    },
    async readRainFixedGaugeTargetGap() {
      return { contractVersion: "adjustment-rain-fixed-gauge-target-gap-status/v1", state: "absent" };
    },
    async recordRainFixedGaugeTargetGap() { assert.fail("unexpected oversized gap"); },
    async recordRevisionGap() { assert.fail("unexpected revision gap"); },
    async stageRevision(bytes) {
      events.push("stage");
      staged = Buffer.from(bytes);
      const identity = createHash("sha256").update(bytes).digest("hex");
      const unsigned = {
        contractVersion: "adjustment-revision-stage-receipt/v1",
        durable: true,
        durableAt: "2026-09-14T07:12:00.000Z",
        projectionIdentitySha256: identity,
        projectionKind: "target_revision",
        projectionSha256: identity,
      };
      return { ...unsigned,
        stageReceiptSha256: createHash("sha256").update(`${JSON.stringify(unsigned)}\n`).digest("hex") };
    },
  }, {
    repository: {
      async commitRevisionGroup(_pool, input) {
        events.push("commit");
        group = input;
        return {
          revisionReceipts: Array.from({ length: 12 }, (_unused, index) => ({
            archiveCommitOrdinal: String(index + 31),
            archiveCommittedAt: "2026-09-14T07:12:01.000Z",
            contractVersion: "adjustment-revision-commit-receipt/v1",
            frontierSha256: createHash("sha256").update(`target-frontier-${index}`).digest("hex"),
            predecessorFrontierSha256: index === 0
              ? "a".repeat(64)
              : createHash("sha256").update(`target-frontier-${index - 1}`).digest("hex"),
            projectionIdentitySha256: input.projectionIdentitySha256,
            projectionKind: "target_revision",
            projectionSha256: input.projectionSha256,
            receiptSha256: createHash("sha256").update(`target-receipt-${index}`).digest("hex"),
            stageReceiptSha256: input.stageReceiptSha256,
          })),
          state: "committed",
        };
      },
      async markRevisionGroupGap() { assert.fail("unexpected group gap"); },
      async readCapturesForHour() { return captures; },
      async readPendingHour() { return "2026-09-14T07:00:00.000Z"; },
      async readSourceCatalog() { return targetSourceCatalog(); },
    },
  });
  assert.deepEqual(result, { revisionCount: 12, state: "published" });
  assert.deepEqual(events, ["stage", "commit", "publish"]);
});

test("fixed-gauge target persists one grouped stage gap without ordinals", async () => {
  const events = [];
  const captures = RAIN_COLLECTION_STATIONS.map((station) => targetStationCapture(station));
  const result = await publishRainFixedGaugeTarget({}, captureEpoch, {
    async publishRevisionBatch() { assert.fail("unexpected publication"); },
    async readRainFixedGaugeTargetGap() {
      return { contractVersion: "adjustment-rain-fixed-gauge-target-gap-status/v1", state: "absent" };
    },
    async recordRainFixedGaugeTargetGap() { assert.fail("unexpected oversized gap"); },
    async recordRevisionGap(gap) { events.push(`gap:${gap.reason}`); },
    async stageRevision() { throw new Error("archive unavailable"); },
  }, {
    repository: {
      async commitRevisionGroup(_pool, input) {
        events.push(`commit:${String(input.stageReceiptSha256)}`);
        return {
          gap: {
            logicalKeySha256: input.logicalKeySha256,
            projectionIdentitySha256: input.projectionIdentitySha256,
            projectionKind: "target_revision",
            projectionSha256: input.projectionSha256,
            reason: "archive_stage_failed",
          },
          state: "gap",
        };
      },
      async markRevisionGroupGap() { assert.fail("unexpected group gap"); },
      async readCapturesForHour() { return captures; },
      async readPendingHour() { return "2026-09-14T07:00:00.000Z"; },
      async readSourceCatalog() { return targetSourceCatalog(); },
    },
  });
  assert.deepEqual(result, { reason: "archive_stage_failed", state: "gap" });
  assert.deepEqual(events, ["commit:null", "gap:archive_stage_failed"]);
});

test("fixed-gauge target reports oversize without a revision body or ordinal", async () => {
  const captures = RAIN_COLLECTION_STATIONS.map((station) =>
    targetStationCapture(station, { padding: 300_000 }));
  let reported;
  const result = await publishRainFixedGaugeTarget({}, captureEpoch, {
    async publishRevisionBatch() { assert.fail("unexpected publication"); },
    async readRainFixedGaugeTargetGap() {
      return { contractVersion: "adjustment-rain-fixed-gauge-target-gap-status/v1", state: "absent" };
    },
    async recordRainFixedGaugeTargetGap(input) {
      reported = input;
      const unsigned = {
        contractVersion: "adjustment-rain-fixed-gauge-target-gap/v1",
        gapAt: "2026-09-14T07:12:00.000Z",
        logicalHourAt: input.logicalHourAt,
        logicalKeySha256: input.logicalKeySha256,
        qualificationDisposition: "forever_unqualified",
        reason: input.reason,
      };
      return {
        ...unsigned,
        gapSha256: createHash("sha256").update(`${JSON.stringify(unsigned)}\n`).digest("hex"),
      };
    },
    async recordRevisionGap() { assert.fail("unexpected revision gap"); },
    async stageRevision() { assert.fail("oversized body was staged"); },
  }, {
    repository: {
      async commitRevisionGroup() { assert.fail("oversized body reached the database"); },
      async markRevisionGroupGap() { assert.fail("unexpected group gap"); },
      async readCapturesForHour() { return captures; },
      async readPendingHour() { return "2026-09-14T07:00:00.000Z"; },
      async readSourceCatalog() { return targetSourceCatalog(); },
    },
  });
  assert.deepEqual(result, { reason: "target_source_oversized", state: "gap" });
  assert.deepEqual(reported, {
    logicalHourAt: "2026-09-14T07:00:00.000Z",
    logicalKeySha256: adjustmentRainFixedGaugeTargetLogicalKeySha256({
      sourceIds: targetSources().map((source) => source.sourceId),
      validAt: "2026-09-14T07:00:00.000Z",
    }),
    reason: "target_source_oversized",
  });
});

test("fixed-gauge target skips raw body reads after a durable oversize gap", async () => {
  const logicalHourAt = "2026-09-14T07:00:00.000Z";
  const logicalKeySha256 = adjustmentRainFixedGaugeTargetLogicalKeySha256({
    sourceIds: targetSources().map((source) => source.sourceId),
    validAt: logicalHourAt,
  });
  const unsigned = {
    contractVersion: "adjustment-rain-fixed-gauge-target-gap/v1",
    gapAt: "2026-09-14T07:12:00.000Z",
    logicalHourAt,
    logicalKeySha256,
    qualificationDisposition: "forever_unqualified",
    reason: "target_source_oversized",
  };
  const gap = {
    ...unsigned,
    gapSha256: createHash("sha256").update(`${JSON.stringify(unsigned)}\n`).digest("hex"),
  };
  const result = await publishRainFixedGaugeTarget({}, captureEpoch, {
    async publishRevisionBatch() { assert.fail("unexpected publication"); },
    async readRainFixedGaugeTargetGap() {
      return {
        contractVersion: "adjustment-rain-fixed-gauge-target-gap-status/v1",
        gap,
        state: "present",
      };
    },
    async recordRainFixedGaugeTargetGap() { assert.fail("gap was already durable"); },
    async recordRevisionGap() { assert.fail("unexpected revision gap"); },
    async stageRevision() { assert.fail("durable gap reached staging"); },
  }, {
    repository: {
      async commitRevisionGroup() { assert.fail("durable gap reached the database"); },
      async markRevisionGroupGap() { assert.fail("unexpected group gap"); },
      async readCapturesForHour() { assert.fail("durable gap inflated raw bodies"); },
      async readPendingHour() { return logicalHourAt; },
      async readSourceCatalog() { return targetSourceCatalog(); },
    },
  });
  assert.deepEqual(result, { reason: "target_source_oversized", state: "gap" });
});

test("rain persistence target retains every overlapping causal capture", () => {
  const first = stationCapture();
  const later = stationCapture({ received: "2026-09-14T07:10:00.000Z" });
  const evaluation = createRainAdjustmentEvaluation(
    [forecastCapture(), later, first],
    "2026-09-14T08:05:00.000Z",
  );
  const target = createRainAdjustmentPersistenceTarget(evaluation);
  assert.equal(target.contractVersion, "rain-maintenance-persistence-target/v1");
  assert.equal(target.rows.length, 12);
  assert.deepEqual(target.rows[0].captureMembers.map((capture) => capture.completedAt),
    [first.completedAt, later.completedAt]);
  assert.equal(target.rows[0].receivedAt, first.completedAt);
  assert.equal(target.reason, "raw_fallback_unavailable");
  assert.equal(target.prediction64, null);
});

// source identity must survive provider decoding before any portable inference
test("rain profile and genuine model output preserve initialized hourly scope", () => {
  const capture = forecastCapture();
  const profile = rainForecastProfile(capture);
  assert.equal(profile.hours.length, 48);
  assert.equal(profile.hours[0].leadHours, 1);
  const run = createRainAdjustmentRun([capture, stationCapture()], "2026-09-14T08:05:00.000Z");
  assert.equal(run.decisionAt, decisionAt);
  assert.equal(run.hours.length, 23);
  assert.equal(run.hours[0].modelLeadHours, 9);
  assert.equal(run.hours.at(-1).modelLeadHours, 31);
  assert.equal(run.hours.every((hour) => hour.applied && Number.isFinite(hour.correctedPrecipitationMm)), true);
  assert.equal(run.hours.some((hour) => hour.correctedPrecipitationMm !== hour.rawPrecipitationMm), true);
  assert.equal(JSON.stringify(run).includes('"body"'), false);
  assert.equal(createRainAdjustmentRun([], decisionAt), null);
  assert.throws(() => createRainAdjustmentRun([capture], "2026-09-14T07:59:00.000Z"), /matured/u);
  const bad = JSON.parse(capture.body);
  bad.hourly.time[9] = "2026-09-14T10:00";
  assert.throws(() => rainForecastProfile({ ...capture, body: Buffer.from(JSON.stringify(bad)) }), /hour mismatch/u);
});

// disabled runtime must stop before storage reads or model inference
test("disabled rain runtime does not query pending captures", async () => {
  const pool = {
    // reject any accidental database access
    async query() {
      assert.fail("disabled rain runtime queried captures");
    },
  };
  assert.equal(await publishRainAdjustment(
    pool,
    new Date(decisionAt),
    { artifactSha256: null, reasonCode: "policy_raw", state: "disabled" },
  ), false);
});

test("rain maintenance preserves native probabilities and stages before compact append", async () => {
  const captures = [forecastCapture(), stationCapture()];
  const evaluation = createRainAdjustmentEvaluation(captures, "2026-09-14T08:05:00.000Z");
  const events = [];
  const result = await publishRainAdjustmentMaintenanceShadow(
    maintenanceDatabase(events),
    evaluation,
    "2026-09-14T08:05:00.000Z",
    { candidate: maintenanceCandidate(), captureEpoch, client: maintenanceClient(events) },
    rainIncumbentRuntime(),
  );
  assert.equal(result.status, "published");
  assert.deepEqual(events.filter((event) => typeof event === "string"),
    ["stage", "register", "append", "publish"]);
  const captured = events.find((event) => typeof event === "object");
  assert.equal(captured.source.rows.length, 23);
  assert.equal(captured.source.rows[0].contentSha256, forecastCapture().bodySha256);
  assert.equal(captured.source.rows[0].receivedAt, forecastCapture().completedAt);
  assert.equal(captured.source.rows[0].modelLeadHours, 9);
  assert.equal(captured.body.rows.length, 23);
  assert.equal(captured.body.rows.every((row) => typeof row.occurrenceProbability64 === "string"), true);
  assert.equal(captured.body.rows.some((row) => row.occurrenceProbability64 !== "0000000000000000"), true);
});

test("rain maintenance archives explicit policy-raw incumbent rows", async () => {
  const evaluation = createRainAdjustmentEvaluation(
    [forecastCapture(), stationCapture()],
    "2026-09-14T08:05:00.000Z",
  );
  const events = [];
  const result = await publishRainAdjustmentMaintenanceShadow(
    maintenanceDatabase(events),
    evaluation,
    "2026-09-14T08:05:00.000Z",
    { candidate: maintenanceCandidate(), captureEpoch, client: maintenanceClient(events) },
    rainRawIncumbentRuntime(),
  );
  const captured = events.find((event) => typeof event === "object");
  assert.equal(result.status, "published");
  assert.equal(captured.comparator.servingAuthority.authorityKind, "policy_raw");
  assert.equal(captured.comparator.rows.every((row) =>
    row.applied === false && row.reasonCode === "policy_raw"), true);
  assert.equal(captured.comparator.rows[0].incumbentPrecipitationMm64,
    captured.source.rows[0].precipitationMm64);
});

// evaluate the root-installed inactive artifact rather than replaying the served model
test("rain maintenance evaluates the installed candidate artifact on the active causal inputs", async () => {
  const evaluation = createRainAdjustmentEvaluation(
    [forecastCapture(), stationCapture()],
    "2026-09-14T08:05:00.000Z",
  );
  const artifact = {
    ...portableRainArtifact(),
    categoryScales: [1, 1, 1],
    modelMonth: "2027-01",
  };
  const events = [];
  const result = await publishRainAdjustmentMaintenanceShadow(
    maintenanceDatabase(events),
    evaluation,
    "2026-09-14T08:05:00.000Z",
    { candidate: maintenanceCandidate(artifact), captureEpoch, client: maintenanceClient(events) },
    rainIncumbentRuntime(),
  );
  const captured = events.find((event) => typeof event === "object");
  const candidateAmounts = captured.body.rows.map((row) =>
    Buffer.from(row.candidatePrecipitationMm64, "hex").readDoubleBE());
  assert.equal(result.status, "published");
  assert.ok(candidateAmounts.some((amount, index) =>
    amount !== evaluation.performance.hours[index].correctedPrecipitationMm));
});

test("rain maintenance crash retry and proof failures never bypass durable staging", async () => {
  const evaluation = createRainAdjustmentEvaluation(
    [forecastCapture(), stationCapture()],
    "2026-09-14T08:05:00.000Z",
  );
  const failedEvents = [];
  const failed = await publishRainAdjustmentMaintenanceShadow(
    maintenanceDatabase(failedEvents), evaluation, "2026-09-14T08:05:00.000Z",
    { candidate: maintenanceCandidate(), captureEpoch,
      client: maintenanceClient(failedEvents, { failStage: true }) },
    rainIncumbentRuntime(),
  );
  assert.deepEqual(failed, { reason: "archive_stage_failed", status: "gap" });
  assert.deepEqual(failedEvents, ["stage", "gap:archive_stage_failed"]);

  const retryEvents = [];
  assert.equal((await publishRainAdjustmentMaintenanceShadow(
    maintenanceDatabase(retryEvents), evaluation, "2026-09-14T08:05:00.000Z",
    { candidate: maintenanceCandidate(), captureEpoch, client: maintenanceClient(retryEvents) },
    rainIncumbentRuntime(),
  )).status, "published");
  assert.deepEqual(retryEvents.filter((event) => typeof event === "string"),
    ["stage", "register", "append", "publish"]);

  const proofEvents = [];
  const rejected = await publishRainAdjustmentMaintenanceShadow(
    maintenanceDatabase(proofEvents), evaluation, "2026-09-14T08:05:00.000Z",
    { candidate: { ...maintenanceCandidate(), catalogSha256: "not-a-hash" }, captureEpoch,
      client: maintenanceClient(proofEvents) },
    rainIncumbentRuntime(),
  );
  assert.deepEqual(rejected, { reason: "candidate_unavailable", status: "gap" });
  assert.deepEqual(proofEvents, ["gap:candidate_unavailable"]);
});

test("rain gate revision stages before the transactional bind and publishes after handoff", async () => {
  const evaluation = createRainAdjustmentEvaluation(
    [forecastCapture(), stationCapture()],
    "2026-09-14T08:05:00.000Z",
  );
  const stored = {
    generatedAt: "2026-09-14T08:05:10.000Z",
    hours: evaluation.run.hours,
    inputSha256: evaluation.run.inputSha256,
    modelSha256: evaluation.run.modelSha256,
    runInitializedAt: evaluation.run.runInitializedAt,
    storedContentSha256: createHash("sha256").update("stored-rain-jsonb").digest("hex"),
  };
  const events = [];
  let bindFailure = false;
  let recoveredGap;
  let stageAttempts = 0;
  let stageReceipt;
  let stagedProjection;
  let transactionAborted = false;
  const client = {
    // publish only a successfully bound rain revision
    async publishRevision(projection, receivedStage, revisionReceipt) {
      // prohibit publication after a failed database bind
      if (bindFailure) assert.fail("failed rain bind was published");
      events.push("publish-revision");
      assert.equal(receivedStage, stageReceipt);
      assert.equal(revisionReceipt.stageReceiptSha256, stageReceipt.stageReceiptSha256);
      assert.ok(Buffer.from(projection).length > 0);
    },
    // retain the staged rain body's terminal disposition
    async recordRevisionGap(gap) {
      events.push(`revision-gap:${gap.reason}`);
      recoveredGap = gap;
    },
    async stageRevision(projection) {
      events.push("stage-revision");
      stageAttempts += 1;
      stagedProjection = parseAdjustmentRainGateFeatureProjection(projection);
      // retain the gate run until one simulated full spool drains
      if (stageAttempts === 1) {
        const error = new Error("full");
        error.code = "adjustment_revision_spool_refused";
        throw error;
      }
      const identity = createHash("sha256").update(projection).digest("hex");
      const unsigned = {
        contractVersion: "adjustment-revision-stage-receipt/v1",
        durable: true,
        durableAt: "2026-09-14T08:05:11.000Z",
        projectionIdentitySha256: identity,
        projectionKind: "rain_gate_input",
        projectionSha256: identity,
      };
      stageReceipt = { ...unsigned,
        stageReceiptSha256: createHash("sha256").update(`${JSON.stringify(unsigned)}\n`).digest("hex") };
      return stageReceipt;
    },
  };
  const queryable = {
    // route only the exact rain bind transaction statements
    async query(sql, values) {
      const statement = sql.trim();
      // reject every operation while the rain bind error is unrecovered
      if (transactionAborted && statement !==
          "ROLLBACK TO SAVEPOINT rain_adjustment_revision_bind") {
        throw new Error("current transaction is aborted");
      }
      // open the bounded rain bind recovery scope
      if (statement === "SAVEPOINT rain_adjustment_revision_bind") {
        events.push("bind-savepoint");
        return { rows: [] };
      }
      // bind the staged rain body inside the savepoint
      if (statement.includes("weather_bind_rain_gate_revision_v1")) {
        events.push("bind-revision");
        // emulate one SQL error that aborts the transaction
        if (bindFailure) {
          transactionAborted = true;
          throw new Error("rain bind failed");
        }
        const pointer = JSON.parse(values[0]);
        return { rows: [{ value: {
          contractVersion: "adjustment-revision-row-receipt/v1",
          revisionReceipt: {
            archiveCommitOrdinal: "12",
            archiveCommittedAt: "2026-09-14T08:05:12.000Z",
            contractVersion: "adjustment-revision-commit-receipt/v1",
            frontierSha256: "a".repeat(64),
            predecessorFrontierSha256: "b".repeat(64),
            projectionIdentitySha256: pointer.projectionIdentitySha256,
            projectionKind: "rain_gate_input",
            projectionSha256: pointer.projectionSha256,
            receiptSha256: "c".repeat(64),
            stageReceiptSha256: pointer.stageReceiptSha256,
          },
        } }] };
      }
      // restore queryability before writing the terminal marker
      if (statement === "ROLLBACK TO SAVEPOINT rain_adjustment_revision_bind") {
        events.push("bind-rollback");
        transactionAborted = false;
        return { rows: [] };
      }
      // release the successful rain bind scope
      if (statement === "RELEASE SAVEPOINT rain_adjustment_revision_bind") {
        events.push("bind-release");
        return { rows: [] };
      }
      // persist the terminal disposition only after recovery
      if (statement.includes("weather_mark_adjustment_revision_gap_v1")) {
        const gap = JSON.parse(values[2]);
        events.push("mark-gap");
        return { rows: [{ value: { ...gap,
          contractVersion: "adjustment-revision-gap-marker/v1",
          storedContentSha256: stored.storedContentSha256 } }] };
      }
      throw new Error(`unexpected rain revision query: ${statement}`);
    },
  };
  const publication = await createRainAdjustmentRevisionArchiver(
    queryable,
    client,
    captureEpoch,
    evaluation,
    // release the simulated spool without delaying the focused test
    async () => { events.push("stage-wait"); },
  )(queryable, stored);
  assert.deepEqual(events, ["stage-revision", "stage-wait", "stage-revision", "bind-savepoint",
    "bind-revision", "bind-release"]);
  assert.equal(stagedProjection.contractVersion, "adjustment-rain-gate-feature-projection/v2");
  assert.equal(stagedProjection.source.sourceId, forecastCapture().claimId);
  assert.equal(stagedProjection.rows.length, 23);
  assert.equal(stagedProjection.rows.every((row) => row.features64.length === 107), true);
  assert.equal(stagedProjection.rows[0].modelLeadHours, 9);
  assert.equal(Buffer.from(stagedProjection.rows[0].rawPrecipitationMm64, "hex").readDoubleBE(),
    evaluation.run.hours[0].rawPrecipitationMm);
  assert.equal(Buffer.from(stagedProjection.rows[0].rawTargetHourTemperatureC64, "hex").readDoubleBE(),
    12);
  await publication.publish();
  assert.deepEqual(events, ["stage-revision", "stage-wait", "stage-revision", "bind-savepoint",
    "bind-revision", "bind-release", "publish-revision"]);

  events.length = 0;
  bindFailure = true;
  const recoveryPublication = await createRainAdjustmentRevisionArchiver(
    queryable,
    client,
    captureEpoch,
    evaluation,
  )(queryable, stored);
  await recoveryPublication.publish();
  assert.deepEqual(events, ["stage-revision", "bind-savepoint", "bind-revision",
    "bind-rollback", "bind-release", "mark-gap", "revision-gap:database_bind_failed"]);
  assert.deepEqual(recoveredGap, {
    logicalKeySha256: recoveredGap.logicalKeySha256,
    projectionIdentitySha256: stageReceipt.projectionIdentitySha256,
    projectionKind: "rain_gate_input",
    projectionSha256: stageReceipt.projectionIdentitySha256,
    reason: "database_bind_failed",
  });
});

test("rain control revision durably stages state before scoring and database bind", async () => {
  const evaluation = createRainAdjustmentEvaluation(
    [forecastCapture(), stationCapture()],
    "2026-09-14T08:05:00.000Z",
  );
  const epochWitnessSha256 = "7".repeat(64);
  const scheduleContractSha256 = "6".repeat(64);
  const candidate = maintenanceControlCandidate(epochWitnessSha256, scheduleContractSha256);
  const stored = {
    generatedAt: "2026-09-14T08:05:10.000Z",
    hours: evaluation.run.hours,
    inputSha256: evaluation.run.inputSha256,
    modelSha256: evaluation.run.modelSha256,
    runInitializedAt: evaluation.run.runInitializedAt,
    storedContentSha256: createHash("sha256").update("stored-rain-controls").digest("hex"),
  };
  const events = [];
  let projection;
  let stateAttempts = 0;
  const client = {
    async publishRevision() { events.push("publish-revision"); },
    async recordRevisionGap(gap) { events.push(`revision-gap:${gap.reason}`); },
    async stageRainControlState(stateBytes) {
      events.push("stage-state");
      assert.deepEqual(Buffer.from(stateBytes), Buffer.from(candidate.controlStateBytes));
      stateAttempts += 1;
      // release the simulated two-slot spool after one bounded wait
      if (stateAttempts === 1) {
        const error = new Error("full");
        error.code = "adjustment_revision_spool_refused";
        throw error;
      }
      const unsigned = {
        contractVersion: "adjustment-rain-control-state-stage-receipt/v1",
        durable: true,
        durableAt: "2026-09-14T08:05:09.000Z",
        stateSha256: candidate.controlStateSha256,
      };
      return { ...unsigned, stageReceiptSha256: createHash("sha256")
        .update(`${JSON.stringify(unsigned)}\n`).digest("hex") };
    },
    async stageRevision(bytes) {
      events.push("stage-revision");
      projection = parseAdjustmentRainGateControlProjection(bytes);
      const identity = createHash("sha256").update(bytes).digest("hex");
      const unsigned = {
        contractVersion: "adjustment-revision-stage-receipt/v1",
        durable: true,
        durableAt: "2026-09-14T08:05:11.000Z",
        projectionIdentitySha256: identity,
        projectionKind: "rain_gate_input",
        projectionSha256: identity,
      };
      return { ...unsigned, stageReceiptSha256: createHash("sha256")
        .update(`${JSON.stringify(unsigned)}\n`).digest("hex") };
    },
  };
  const queryable = {
    // route only the exact control bind transaction statements
    async query(sql, values) {
      const statement = sql.trim();
      // open the bounded control bind recovery scope
      if (statement === "SAVEPOINT rain_adjustment_revision_bind") {
        events.push("bind-savepoint");
        return { rows: [] };
      }
      // bind the staged control body inside the savepoint
      if (statement.includes("weather_bind_rain_gate_revision_v1")) {
        events.push("bind-revision");
        const pointer = JSON.parse(values[0]);
        return { rows: [{ value: {
          contractVersion: "adjustment-revision-row-receipt/v1",
          revisionReceipt: {
            archiveCommitOrdinal: "13",
            archiveCommittedAt: "2026-09-14T08:05:12.000Z",
            contractVersion: "adjustment-revision-commit-receipt/v1",
            frontierSha256: "a".repeat(64),
            predecessorFrontierSha256: "b".repeat(64),
            projectionIdentitySha256: pointer.projectionIdentitySha256,
            projectionKind: "rain_gate_input",
            projectionSha256: pointer.projectionSha256,
            receiptSha256: "c".repeat(64),
            stageReceiptSha256: pointer.stageReceiptSha256,
          },
        } }] };
      }
      // release the successful control bind scope
      if (statement === "RELEASE SAVEPOINT rain_adjustment_revision_bind") {
        events.push("bind-release");
        return { rows: [] };
      }
      throw new Error(`unexpected rain control revision query: ${statement}`);
    },
  };
  const publication = await createRainAdjustmentRevisionArchiver(
    queryable,
    client,
    { epochAt: "2000-01-01T00:00:00.000Z", witnessSha256: epochWitnessSha256 },
    evaluation,
    async () => { events.push("state-wait"); },
    candidate,
    rainIncumbentRuntime(),
  )(queryable, stored);
  assert.deepEqual(events, ["stage-state", "state-wait", "stage-state", "stage-revision",
    "bind-savepoint", "bind-revision", "bind-release"]);
  assert.equal(projection.contractVersion, "adjustment-rain-gate-control-projection/v3");
  assert.equal(projection.stateSha256, candidate.controlStateSha256);
  assert.equal(projection.ordinalArtifactSha256, candidate.ordinalArtifactSha256);
  assert.equal(projection.rows.length, 23);
  assert.equal(projection.rows[0].features64.length, 107);
  assert.equal(projection.persistenceTarget.reason, "raw_fallback_unavailable");
  assert.equal(projection.rows[0].persistenceReason, "raw_fallback_unavailable");
  assert.equal(projection.rows[0].incumbentReceiptMemberSha256,
    rainIncumbentRuntime().comparatorAuthority.receiptMemberSha256);
  await publication.publish();
  assert.deepEqual(events, ["stage-state", "state-wait", "stage-state", "stage-revision",
    "bind-savepoint", "bind-revision", "bind-release", "publish-revision"]);
});

test("rain control projection failure abandons only the exact staged state", async () => {
  const evaluation = createRainAdjustmentEvaluation(
    [forecastCapture(), stationCapture()],
    "2026-09-14T08:05:00.000Z",
  );
  const candidate = maintenanceControlCandidate("7".repeat(64), "6".repeat(64));
  const stored = {
    generatedAt: "2026-09-14T08:05:10.000Z",
    hours: evaluation.run.hours,
    inputSha256: evaluation.run.inputSha256,
    modelSha256: evaluation.run.modelSha256,
    runInitializedAt: evaluation.run.runInitializedAt,
    storedContentSha256: createHash("sha256").update("stored-rain-gap").digest("hex"),
  };
  const events = [];
  const unsigned = {
    contractVersion: "adjustment-rain-control-state-stage-receipt/v1",
    durable: true,
    durableAt: "2026-09-14T08:05:09.000Z",
    stateSha256: candidate.controlStateSha256,
  };
  const stateReceipt = { ...unsigned, stageReceiptSha256: createHash("sha256")
    .update(`${JSON.stringify(unsigned)}\n`).digest("hex") };
  const client = {
    async recordRainControlStateGap(gap) { events.push(["state-gap", gap]); },
    async recordRevisionGap(gap) { events.push(["revision-gap", gap.reason]); },
    async stageRainControlState() {
      events.push(["stage-state"]);
      return stateReceipt;
    },
    async stageRevision() {
      events.push(["stage-revision"]);
      throw new Error("projection refused");
    },
  };
  const queryable = {
    async query(sql, values) {
      assert.match(sql, /weather_mark_adjustment_revision_gap_v1/u);
      events.push(["mark-gap", JSON.parse(values[2]).reason]);
      return { rows: [{ value: {
        ...JSON.parse(values[2]),
        contractVersion: "adjustment-revision-gap-marker/v1",
        storedContentSha256: stored.storedContentSha256,
      } }] };
    },
  };
  const publication = await createRainAdjustmentRevisionArchiver(
    queryable,
    client,
    { epochAt: "2000-01-01T00:00:00.000Z", witnessSha256: "7".repeat(64) },
    evaluation,
    undefined,
    candidate,
    rainIncumbentRuntime(),
  )(queryable, stored);
  assert.deepEqual(events.slice(0, 4), [
    ["stage-state"],
    ["stage-revision"],
    ["state-gap", {
      reason: "projection_stage_failed",
      stageReceiptSha256: stateReceipt.stageReceiptSha256,
      stateSha256: candidate.controlStateSha256,
    }],
    ["mark-gap", "archive_stage_failed"],
  ]);
  await publication.publish();
  assert.deepEqual(events.at(-1), ["revision-gap", "archive_stage_failed"]);
});

test("rain feature revision refuses a pre-epoch causal capture before staging", async () => {
  const evaluation = createRainAdjustmentEvaluation(
    [forecastCapture(), stationCapture()],
    "2026-09-14T08:05:00.000Z",
  );
  const storedContentSha256 = createHash("sha256").update("stored-rain-jsonb").digest("hex");
  const stored = {
    generatedAt: "2026-09-14T08:05:10.000Z",
    hours: evaluation.run.hours,
    inputSha256: evaluation.run.inputSha256,
    modelSha256: evaluation.run.modelSha256,
    runInitializedAt: evaluation.run.runInitializedAt,
    storedContentSha256,
  };
  const events = [];
  const client = {
    async recordRevisionGap(gap) { events.push(`revision-gap:${gap.reason}`); },
    async stageRevision() { assert.fail("pre-epoch feature body was staged"); },
  };
  const queryable = {
    async query(sql, values) {
      assert.match(sql, /weather_mark_adjustment_revision_gap_v1/u);
      const gap = JSON.parse(values[2]);
      events.push("mark-gap");
      return { rows: [{ value: { ...gap,
        contractVersion: "adjustment-revision-gap-marker/v1", storedContentSha256 } }] };
    },
  };
  const publication = await createRainAdjustmentRevisionArchiver(
    queryable,
    client,
    { epochAt: "2026-09-14T05:30:00.000Z" },
    evaluation,
  )(queryable, stored);
  await publication.publish();
  assert.deepEqual(events, ["mark-gap", "revision-gap:archive_stage_failed"]);
});

test("revision staging waits only for bounded spool capacity", async () => {
  const pauses = [];
  let attempts = 0;
  const client = {
    // release the simulated two-slot spool on the third poll
    async stageRevision() {
      attempts += 1;
      // preserve the private capacity code until the simulated drain
      if (attempts < 3) {
        const error = new Error("full");
        error.code = "adjustment_revision_spool_refused";
        throw error;
      }
      return { contractVersion: "adjustment-revision-stage-receipt/v1" };
    },
  };
  const receipt = await stageAdjustmentRevisionWithBackpressure(
    client,
    Buffer.from("projection"),
    // record fixed polling without delaying the unit test
    async (milliseconds) => { pauses.push(milliseconds); },
  );
  assert.equal(receipt.contractVersion, "adjustment-revision-stage-receipt/v1");
  assert.equal(attempts, 3);
  assert.deepEqual(pauses, [5_000, 5_000]);

  let genericAttempts = 0;
  // refuse generic archive errors without retry or delay
  await assert.rejects(() => stageAdjustmentRevisionWithBackpressure(
    {
      async stageRevision() {
        genericAttempts += 1;
        throw new Error("invalid body");
      },
    },
    Buffer.from("projection"),
    async () => { throw new Error("unexpected pause"); },
  ), /invalid body/u);
  assert.equal(genericAttempts, 1);
});

test("rain control state staging shares bounded spool backpressure", async () => {
  const pauses = [];
  let attempts = 0;
  const receipt = await stageRainControlStateWithBackpressure(
    {
      // release the simulated two-slot spool on the third poll
      async stageRainControlState() {
        attempts += 1;
        // preserve the private capacity code until the simulated drain
        if (attempts < 3) {
          const error = new Error("full");
          error.code = "adjustment_revision_spool_refused";
          throw error;
        }
        return { contractVersion: "adjustment-rain-control-state-stage-receipt/v1" };
      },
    },
    Buffer.from("state"),
    // record fixed polling without delaying the unit test
    async (milliseconds) => { pauses.push(milliseconds); },
  );
  assert.equal(receipt.contractVersion, "adjustment-rain-control-state-stage-receipt/v1");
  assert.equal(attempts, 3);
  assert.deepEqual(pauses, [5_000, 5_000]);

  let genericAttempts = 0;
  // refuse generic state errors without retry or delay
  await assert.rejects(() => stageRainControlStateWithBackpressure(
    {
      async stageRainControlState() {
        genericAttempts += 1;
        throw new Error("invalid state");
      },
    },
    Buffer.from("state"),
    async () => { throw new Error("unexpected pause"); },
  ), /invalid state/u);
  assert.equal(genericAttempts, 1);
});

test("revision staging stops after thirty seconds of capacity polling", async () => {
  const pauses = [];
  let attempts = 0;
  await assert.rejects(() => stageAdjustmentRevisionWithBackpressure(
    {
      // keep the simulated spool full for every bounded attempt
      async stageRevision() {
        attempts += 1;
        const error = new Error("full");
        error.code = "adjustment_revision_spool_refused";
        throw error;
      },
    },
    Buffer.from("projection"),
    // record the six fixed waits without sleeping
    async (milliseconds) => { pauses.push(milliseconds); },
  ), (error) => error.code === "adjustment_revision_spool_refused");
  assert.equal(attempts, 7);
  assert.deepEqual(pauses, [5_000, 5_000, 5_000, 5_000, 5_000, 5_000]);
});
