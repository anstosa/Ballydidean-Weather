import assert from "node:assert/strict";
import test from "node:test";

import {
  LOCAL_FORECAST_CORRECTION_CANDIDATE_GRID,
  LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS,
  createLocalForecastCorrectionPanel,
  issueSnapshotDigest,
  labelSealDigest,
  localDateBounds,
  localForecastCorrectionPolicyHash,
  validateLocalForecastIssueSnapshot,
  validateLocalForecastLabelSeal,
  validateLocalForecastCorrectionPanel,
} from "./local_forecast_correction_contract.mjs";

const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);
const HASH_C = "c".repeat(64);

// create one minimal normalized weather record
function record(overrides = {}) {
  return {
    freshness: { status: "fresh" },
    id: "record-1",
    metadata: { upstream: { model: "best_match" } },
    metrics: { pressureHpa: 1000, solarRadiationWm2: 500, uvIndex: 4 },
    productRunAt: "2026-10-31T06:00:00.000Z",
    provenance: {
      providerKey: "open-meteo",
      sourceId: "source-1",
      sourceKind: "forecast",
      stationSlug: "open-meteo-virtual",
    },
    receivedAt: "2026-10-31T06:05:00.000Z",
    validAt: "2026-10-31T07:00:00.000Z",
    ...overrides,
  };
}

// create one valid frozen panel
function panelInput(overrides = {}) {
  const candidateFrozenAt = overrides.candidateFrozenAt ?? "2026-10-31T03:34:59.999Z";
  const cells = LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS.map((cell) => ({
    ...cell,
    policy: { horizonHours: 3, strength: 0 },
  }));
  return {
    candidateFrozenAt,
    captureMinute: 5,
    cells,
    claims: {
      candidateOutput: "prospective_causal_counterfactual",
      inputProduct: "as_available_captured_regional_anchor",
      servingOutput: "regional_zero",
    },
    codeHash: HASH_A,
    createdAt: overrides.createdAt ?? candidateFrozenAt,
    developmentLatestAt: "2026-10-30T00:35:00.000Z",
    earliestValidationInputAt: "2026-10-31T03:35:00.000Z",
    inputHash: HASH_B,
    policyHash: localForecastCorrectionPolicyHash(cells),
    servingPolicyState: "zero",
    site: {
      latitude: 48.5,
      longitude: -122.6,
      siteKey: "ballydidean",
      timeZone: "America/Los_Angeles",
    },
    sourceAllowlist: ["farm", "ws90"],
    sourceCatalog: [
      { providerKey: "ecowitt-local", sourceId: "farm", sourceKey: "farm-live-v1", stationSlug: "ballydidean-ecowitt" },
      { providerKey: "ecowitt-local", sourceId: "ws90", sourceKey: "ws90-live-v1", stationSlug: "ballydidean-ecowitt" },
    ],
    sourceCatalogReceipt: overrides.sourceCatalogReceipt ?? {
      envelopeSha256: HASH_C,
      receivedAt: overrides.createdAt ?? candidateFrozenAt,
    },
    targetDateCount: 14,
    targetStartLocalDate: "2026-11-01",
    ...overrides,
  };
}

// build one valid issue snapshot with a complete context
function issueInput() {
  const pressureContext = Array.from({ length: 6 }, (_unused, index) => record({
    id: `context-${index}`,
    validAt: new Date(Date.parse("2026-10-31T07:00:00.000Z") + index * 3_600_000).toISOString(),
  }));
  const unsigned = {
    contractVersion: "weather-local-correction-issue/v1",
    current: [record({ id: "current", productRunAt: null })],
    evaluatedAt: "2026-10-31T06:05:00.000Z",
    forecast: [record()],
    normalizedDigest: "",
    panelDigest: createLocalForecastCorrectionPanel(panelInput()).panelDigest,
    pressureContext,
    receipts: [
      { envelopeSha256: HASH_A, generatedAt: null, kind: "current", receivedAt: "2026-10-31T06:01:00.000Z" },
      { envelopeSha256: HASH_B, generatedAt: null, kind: "forecast", receivedAt: "2026-10-31T06:03:00.000Z" },
      { envelopeSha256: HASH_C, generatedAt: null, kind: "solarCloudEvidence", receivedAt: "2026-10-31T06:05:00.000Z" },
    ],
    scheduledAt: "2026-10-31T06:05:00.000Z",
    site: panelInput().site,
    snapshotId: "2026-10-31T06-05-00-000Z",
    solarCloudEvidence: null,
  };
  return { ...unsigned, normalizedDigest: issueSnapshotDigest(unsigned) };
}

// build one complete label seal
function labelInput() {
  const unsigned = {
    contractVersion: "weather-local-correction-labels/v1",
    from: "2026-11-01T03:30:00.000Z",
    normalizedDigest: "",
    panelDigest: createLocalForecastCorrectionPanel(panelInput()).panelDigest,
    receipts: [
      {
        envelopeSha256: HASH_A,
        generatedAt: "2026-11-02T14:00:00.000Z",
        page: 1,
        receivedAt: "2026-11-02T14:04:00.000Z",
        requestSha256: HASH_B,
        sourceId: "farm",
      },
      {
        envelopeSha256: HASH_B,
        generatedAt: "2026-11-02T14:00:00.000Z",
        page: 2,
        receivedAt: "2026-11-02T14:04:00.000Z",
        requestSha256: HASH_C,
        sourceId: "ws90",
      },
    ],
    rows: [record({
      id: "label",
      productRunAt: null,
      provenance: { providerKey: "ecowitt-local", sourceId: "ws90", sourceKind: "physical_sensor", stationSlug: "ballydidean-ecowitt" },
      receivedAt: "2026-11-01T20:01:00.000Z",
      validAt: "2026-11-01T20:00:00.000Z",
    })],
    sealAt: "2026-11-02T14:00:00.000Z",
    sourceAllowlist: ["farm", "ws90"],
    status: "complete",
    targetDate: "2026-11-01",
    to: "2026-11-02T09:00:00.000Z",
  };
  return { ...unsigned, normalizedDigest: labelSealDigest(unsigned) };
}

test("candidate grid is the fixed zero plus sixteen policy choices", () => {
  assert.equal(LOCAL_FORECAST_CORRECTION_CANDIDATE_GRID.length, 17);
  assert.deepEqual(LOCAL_FORECAST_CORRECTION_CANDIDATE_GRID[0], { horizonHours: 3, strength: 0 });
  assert.deepEqual(LOCAL_FORECAST_CORRECTION_CANDIDATE_GRID.at(-1), { horizonHours: 24, strength: 1 });
});

test("panel derives DST-safe target bounds schedules and fixed five cells", () => {
  const panel = createLocalForecastCorrectionPanel(panelInput());
  assert.equal(panel.targetStartAt, "2026-11-01T07:00:00.000Z");
  assert.equal(panel.targetEndAt, "2026-11-15T08:00:00.000Z");
  assert.equal(panel.warmupStartAt, "2026-10-31T07:00:00.000Z");
  assert.equal(panel.targetDates.length, 14);
  assert.equal(panel.issueSchedule.length, 361);
  assert.equal(panel.labelSchedule[0].from, "2026-11-01T03:30:00.000Z");
  assert.equal(panel.labelSchedule[0].sealAt, "2026-11-02T14:00:00.000Z");
  assert.equal(panel.cells.length, 5);
  assert.match(panel.panelDigest, /^[a-f0-9]{64}$/u);
  assert.equal(validateLocalForecastCorrectionPanel(panel), panel);
  assert.throws(() => createLocalForecastCorrectionPanel(panelInput({
    sourceCatalog: panelInput().sourceCatalog.map((source, index) => index === 0
      ? { ...source, sourceId: "substituted" }
      : source),
  })), /source allowlist/u);
  const changedCells = panel.cells.map((cell, index) => index === 0
    ? { ...cell, policy: { horizonHours: 3, strength: 0.25 } }
    : cell);
  assert.throws(() => validateLocalForecastCorrectionPanel({
    ...panel,
    cells: changedCells,
    policyHash: localForecastCorrectionPolicyHash(changedCells),
  }), /panel digest/u);
});

test("local date bounds retain real 23 and 25 hour dates", () => {
  const spring = localDateBounds("2026-03-08", "America/Los_Angeles");
  const fall = localDateBounds("2026-11-01", "America/Los_Angeles");
  assert.equal(Date.parse(spring.endAt) - Date.parse(spring.startAt), 23 * 3_600_000);
  assert.equal(Date.parse(fall.endAt) - Date.parse(fall.startAt), 25 * 3_600_000);
});

test("warm-up is exactly twenty-four hours before target start across DST", () => {
  const spring = createLocalForecastCorrectionPanel(panelInput({
    developmentLatestAt: "2026-03-06T00:35:00.000Z",
    earliestValidationInputAt: "2026-03-08T03:35:00.000Z",
    candidateFrozenAt: "2026-03-08T03:34:59.999Z",
    targetStartLocalDate: "2026-03-09",
  }));
  const fall = createLocalForecastCorrectionPanel(panelInput({
    developmentLatestAt: "2026-10-30T01:35:00.000Z",
    earliestValidationInputAt: "2026-11-01T04:35:00.000Z",
    candidateFrozenAt: "2026-11-01T04:34:59.999Z",
    targetStartLocalDate: "2026-11-02",
  }));
  assert.equal(Date.parse(spring.targetStartAt) - Date.parse(spring.warmupStartAt), 24 * 3_600_000);
  assert.equal(spring.warmupStartAt, "2026-03-08T07:00:00.000Z");
  assert.equal(Date.parse(fall.targetStartAt) - Date.parse(fall.warmupStartAt), 24 * 3_600_000);
  assert.equal(fall.warmupStartAt, "2026-11-01T08:00:00.000Z");
});

test("panel rejects a short embargo changed comparison count and false output claim", () => {
  assert.throws(() => createLocalForecastCorrectionPanel(panelInput({
    developmentLatestAt: "2026-10-30T00:35:00.001Z",
  })), /27-hour embargo/u);
  assert.throws(() => createLocalForecastCorrectionPanel(panelInput({
    earliestValidationInputAt: "2026-10-31T03:35:00.001Z",
  })), /differs from the schedule/u);
  assert.throws(() => createLocalForecastCorrectionPanel(panelInput({
    candidateFrozenAt: "2026-10-30T00:34:59.999Z",
  })), /candidate freeze/u);
  assert.throws(() => createLocalForecastCorrectionPanel(panelInput({ cells: [] })), /cells/u);
  assert.throws(() => createLocalForecastCorrectionPanel(panelInput({
    claims: { ...panelInput().claims, candidateOutput: "as_issued_adjusted" },
  })), /candidate output claim/u);
});

test("issue snapshot validates final receipt timing digest and six-row context", () => {
  const issue = issueInput();
  assert.equal(validateLocalForecastIssueSnapshot(issue), issue);
  assert.throws(() => validateLocalForecastIssueSnapshot({ ...issue, scheduledAt: "2026-10-31T06:05:00.001Z" }), /precedes its schedule/u);
  assert.throws(() => validateLocalForecastIssueSnapshot({ ...issue, pressureContext: issue.pressureContext.slice(1) }), /pressure context/u);
  assert.throws(() => validateLocalForecastIssueSnapshot({
    ...issue,
    pressureContext: issue.pressureContext.map((row, index) => index === 2
      ? { ...row, metrics: { ...row.metrics, pressureHpa: null } }
      : row),
  }), /pressure context/u);
  assert.throws(() => validateLocalForecastIssueSnapshot({ ...issue, evaluatedAt: "2026-10-31T06:04:00.000Z" }), /precedes its schedule|unavailable|evaluation receipt/u);
  assert.throws(() => validateLocalForecastIssueSnapshot({ ...issue, normalizedDigest: HASH_A }), /normalized digest/u);
});

test("label seal enforces cutoff range and immutable digest", () => {
  const labels = labelInput();
  assert.equal(validateLocalForecastLabelSeal(labels), labels);
  const noReceipt = { ...labels, receipts: [] };
  noReceipt.normalizedDigest = labelSealDigest(noReceipt);
  assert.throws(() => validateLocalForecastLabelSeal(noReceipt), /has no receipt/u);
  const late = { ...labels, rows: [{ ...labels.rows[0], receivedAt: "2026-11-03T14:00:00.001Z" }] };
  late.normalizedDigest = labelSealDigest(late);
  assert.throws(() => validateLocalForecastLabelSeal(late), /unavailable|label cutoff/u);
  const halo = { ...labels, rows: [{ ...labels.rows[0], receivedAt: labels.to, validAt: labels.to }] };
  halo.normalizedDigest = labelSealDigest(halo);
  assert.throws(() => validateLocalForecastLabelSeal(halo), /label window/u);
  const expiredAttempt = {
    ...labels,
    receipts: [{ ...labels.receipts[0], receivedAt: "2026-11-02T14:05:00.001Z" }],
  };
  expiredAttempt.normalizedDigest = labelSealDigest(expiredAttempt);
  assert.throws(() => validateLocalForecastLabelSeal(expiredAttempt), /receipt sequence/u);
  const foreign = {
    ...labels,
    rows: [{
      ...labels.rows[0],
      provenance: { ...labels.rows[0].provenance, sourceId: "foreign" },
    }],
  };
  foreign.normalizedDigest = labelSealDigest(foreign);
  assert.throws(() => validateLocalForecastLabelSeal(foreign), /allowlist/u);
});
