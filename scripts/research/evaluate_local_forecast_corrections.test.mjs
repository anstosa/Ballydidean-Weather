import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, symlink, unlink, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

import {
  bootstrapLowerGainBound,
  buildPendingLocalForecastCorrectionReport,
  chooseDevelopmentPolicy,
  classifyPanelDecision,
  cloudProxyLabel,
  collectCellPairs,
  compiledBrowserModulePaths,
  countEvidenceFiles,
  evaluateCellDecision,
  evaluateLocalForecastCorrections,
  evaluateRetrospectiveDevelopment,
  eligibleFutureTarget,
  partitionPrimaryRows,
  pointLabel,
  pressureTendencyLabel,
  readJsonFile,
  scorePairedRows,
  uvSensitivityLabels,
  validateIssuePhysicalSources,
  validateLabelPhysicalSources,
  writePrivateReport,
} from "./evaluate_local_forecast_corrections.mjs";
import {
  createLocalForecastCorrectionPanel,
  LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS,
  localForecastCorrectionPolicyHash,
} from "./local_forecast_correction_contract.mjs";
import { radiationCloudProxy } from "../../apps/web/dist/solar-cloud.js";

const HOUR_MS = 3_600_000;
const TARGET = "2026-06-21T20:00:00.000Z";

// create one normalized physical record for label fixtures
function record(sourceId, stationSlug, validAt, metrics, overrides = {}) {
  return {
    freshness: { status: "stale" },
    id: `${sourceId}/${validAt}/${overrides.suffix ?? "a"}`,
    metadata: { upstream: { model: null } },
    metrics: {
      pressureHpa: null,
      solarRadiationWm2: null,
      uvIndex: null,
      ...metrics,
    },
    productRunAt: null,
    provenance: {
      providerKey: stationSlug === "ballydidean-ecowitt" ? "ecowitt-local" : "weatherflow-tempest",
      sourceId,
      sourceKind: "physical_sensor",
      stationSlug,
    },
    receivedAt: overrides.receivedAt ?? validAt,
    validAt,
  };
}

// shift one fixture instant
function at(hours, minutes = 0) {
  return new Date(Date.parse(TARGET) + hours * HOUR_MS + minutes * 60_000).toISOString();
}

// create one paired validation row
function pair(localDate, hourIndex, overrides = {}) {
  const targetAt = new Date(Date.parse(`${localDate}T08:00:00.000Z`) + hourIndex * HOUR_MS).toISOString();
  return {
    adjusted: 0.95,
    daylight: true,
    evaluationAt: new Date(Date.parse(targetAt) - 2 * HOUR_MS).toISOString(),
    issueLeadHours: 2,
    issueAnchorTier: "farm",
    labels: { following: 0, nominal: 0, preceding: 0 },
    localDate,
    observationAgeHours: 2,
    raw: 1,
    targetAt,
    targetLabelTier: "farm",
    vintageId: `run-${hourIndex % 2}`,
    ...overrides,
  };
}

// create one frozen zero-policy panel without outcome members
function pendingPanel() {
  const cells = LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS.map((cell) => ({
    ...cell,
    policy: { horizonHours: 3, strength: 0 },
  }));
  return createLocalForecastCorrectionPanel({
    candidateFrozenAt: "2026-08-31T03:34:59.999Z",
    captureMinute: 5,
    cells,
    claims: {
      candidateOutput: "prospective_causal_counterfactual",
      inputProduct: "as_available_captured_regional_anchor",
      servingOutput: "regional_zero",
    },
    codeHash: "a".repeat(64),
    createdAt: "2026-08-31T03:34:59.999Z",
    developmentLatestAt: "2026-08-30T00:35:00.000Z",
    earliestValidationInputAt: "2026-08-31T03:35:00.000Z",
    inputHash: "b".repeat(64),
    policyHash: localForecastCorrectionPolicyHash(cells),
    servingPolicyState: "zero",
    site: { latitude: 48, longitude: -122, siteKey: "ballydidean", timeZone: "America/Los_Angeles" },
    sourceAllowlist: ["farm", "neighbor"],
    sourceCatalog: [
      { providerKey: "ecowitt-local", sourceId: "farm", sourceKey: "ecowitt-88f15505d89f-local-live-v1", stationSlug: "ballydidean-ecowitt" },
      { providerKey: "weatherflow-tempest", sourceId: "neighbor", sourceKey: "neighbor-live-v1", stationSlug: "tempest-neighbor" },
    ],
    sourceCatalogReceipt: {
      envelopeSha256: "c".repeat(64),
      receivedAt: "2026-08-31T03:34:59.999Z",
    },
    targetDateCount: 14,
    targetStartLocalDate: "2026-09-01",
  });
}

test("evaluator binds current ws90 and label physical rows to frozen source tuples", () => {
  const manifest = pendingPanel();
  const farm = {
    ...manifest.sourceCatalog[0],
    sourceKind: "physical_sensor",
  };
  const regional = {
    providerKey: "open-meteo",
    sourceId: "regional-public",
    sourceKey: "open-meteo-current-v1",
    sourceKind: "model_current",
    stationSlug: "open-meteo-virtual",
  };
  const issue = {
    current: [{ provenance: farm }, { provenance: regional }],
    solarCloudEvidence: { regional, ws90: farm },
  };
  assert.doesNotThrow(() => validateIssuePhysicalSources(issue, manifest.sourceCatalog));
  assert.doesNotThrow(() => validateLabelPhysicalSources({ rows: [{ provenance: farm }] }, manifest.sourceCatalog));
  const neighbor = {
    ...manifest.sourceCatalog[1],
    sourceKind: "physical_sensor",
  };
  assert.throws(
    () => validateIssuePhysicalSources({ ...issue, solarCloudEvidence: { regional, ws90: neighbor } }, manifest.sourceCatalog),
    /canonical farm live source/u,
  );
  for (const [field, value] of [
    ["sourceKey", "ecowitt-legacy-v1"],
    ["providerKey", "substituted-provider"],
    ["stationSlug", "substituted-station"],
  ]) {
    // every tuple component is immutable after discovery
    const substituted = { ...farm, [field]: value };
    assert.throws(
      () => validateIssuePhysicalSources({ ...issue, current: [{ provenance: substituted }] }, manifest.sourceCatalog),
      /frozen source catalog/u,
    );
    assert.throws(
      () => validateIssuePhysicalSources({ ...issue, solarCloudEvidence: { regional, ws90: substituted } }, manifest.sourceCatalog),
      /frozen source catalog/u,
    );
    assert.throws(
      () => validateLabelPhysicalSources({ rows: [{ provenance: substituted }] }, manifest.sourceCatalog),
      /frozen source catalog/u,
    );
  }
});

test("point labels prefer exact values and interpolate same-source brackets deterministically", () => {
  const rows = [
    record("farm", "ballydidean-ecowitt", at(0, -10), { uvIndex: 4 }),
    record("farm", "ballydidean-ecowitt", at(0, 10), { uvIndex: 8 }),
  ];
  assert.equal(pointLabel(rows.toReversed(), "uvIndex", TARGET, "farm").value, 6);
  const exact = record("farm", "ballydidean-ecowitt", TARGET, { uvIndex: 7 });
  assert.equal(pointLabel([...rows, exact], "uvIndex", TARGET, "farm").value, 7);
  assert.equal(pointLabel(rows.slice(0, 1), "uvIndex", TARGET, "farm").status, "unsupported");
  const mixed = [
    record("a", "station-a", at(0, -10), { uvIndex: 4 }),
    record("b", "station-b", at(0, 10), { uvIndex: 8 }),
  ];
  assert.equal(pointLabel(mixed, "uvIndex", TARGET, "local_mean").status, "unsupported");
  const offsetRows = [
    record("farm", "ballydidean-ecowitt", "2026-06-21T12:50:00-07:00", { uvIndex: 4 }),
    record("farm", "ballydidean-ecowitt", "2026-06-21T13:10:00-07:00", { uvIndex: 8 }),
  ];
  assert.equal(pointLabel(offsetRows, "uvIndex", TARGET, "farm").value, 6);
  const boundary = [
    record("farm", "ballydidean-ecowitt", at(0, -15), { uvIndex: 2 }),
    record("farm", "ballydidean-ecowitt", at(0, 15), { uvIndex: 8 }),
  ];
  assert.equal(pointLabel(boundary, "uvIndex", TARGET, "farm").value, 5);
  const outside = { ...boundary[0], validAt: "2026-06-21T19:44:59.999Z", receivedAt: "2026-06-21T19:44:59.999Z" };
  assert.equal(pointLabel([outside, boundary[1]], "uvIndex", TARGET, "farm").status, "unsupported");
});

test("future target eligibility includes every positive lead through exactly twenty-four hours", () => {
  const evaluatedAt = "2026-06-21T20:00:00.000Z";
  assert.equal(eligibleFutureTarget(evaluatedAt, evaluatedAt), false);
  assert.equal(eligibleFutureTarget(evaluatedAt, "2026-06-21T20:00:00.001Z"), true);
  assert.equal(eligibleFutureTarget(evaluatedAt, "2026-06-22T20:00:00.000Z"), true);
  assert.equal(eligibleFutureTarget(evaluatedAt, "2026-06-22T20:00:00.001Z"), false);
});

test("point labels use newest frozen revisions and quarantine contradictory equal identities", () => {
  const old = record("farm", "ballydidean-ecowitt", TARGET, { uvIndex: 3 }, {
    receivedAt: at(0, 1), suffix: "same",
  });
  const revised = { ...old, metrics: { ...old.metrics, uvIndex: 5 }, receivedAt: at(0, 2) };
  assert.equal(pointLabel([revised, old], "uvIndex", TARGET, "farm").value, 5);
  const conflict = { ...revised, metrics: { ...revised.metrics, uvIndex: 9 } };
  assert.equal(pointLabel([revised, conflict], "uvIndex", TARGET, "farm").reason, "conflicting_revision");
});

test("pressure target uses exact three-hour source-local endpoints and complete coverage", () => {
  const farm = Array.from({ length: 7 }, (_unused, index) => record(
    "farm", "ballydidean-ecowitt", at(-3 + index / 2),
    { pressureHpa: 1000 + 2 * index / 2 },
  ));
  assert.equal(pressureTendencyLabel(farm, TARGET, "farm").value, 6);
  const shiftedQuadratic = farm.map((row, index) => ({
    ...row,
    metrics: { ...row.metrics, pressureHpa: 1000 + (index / 2) ** 2 },
  }));
  assert.equal(pressureTendencyLabel(shiftedQuadratic, TARGET, "farm").value, 9);
  assert.equal(pressureTendencyLabel(farm.filter((_row, index) => index !== 3), TARGET, "farm").reason, "incomplete_pressure_span");
  assert.equal(pressureTendencyLabel(farm.map((row, index) => index === 6 ? {
    ...row,
    provenance: { ...row.provenance, sourceId: "replacement" },
  } : row), TARGET, "farm").status, "unsupported");
  const invalid = farm.map((row, index) => index === 3
    ? { ...row, metrics: { ...row.metrics, pressureHpa: 1_300 } }
    : row);
  assert.equal(pressureTendencyLabel(invalid, TARGET, "farm").reason, "incomplete_pressure_span");
});

test("pressure local mean averages tendencies rather than absolute station datums", () => {
  const rows = [];
  for (let index = 0; index < 7; index += 1) {
    // build two complete half-hour station series
    rows.push(record("a", "station-a", at(-3 + index / 2), { pressureHpa: 980 + index }));
    rows.push(record("b", "station-b", at(-3 + index / 2), { pressureHpa: 1030 + 2 * index }));
  }
  assert.equal(pressureTendencyLabel(rows, TARGET, "local_mean").value, 9);
});

test("UV sensitivity panels use complete preceding and following source-local means", () => {
  const rows = Array.from({ length: 5 }, (_unused, index) => record(
    "farm", "ballydidean-ecowitt", at(-1 + index / 2), { uvIndex: 2 + index },
  ));
  const labels = uvSensitivityLabels(rows, TARGET, "farm");
  assert.equal(labels.nominal.value, 4);
  assert.equal(labels.preceding.value, 3);
  assert.equal(labels.following.value, 5);
  assert.equal(uvSensitivityLabels(rows.filter((_row, index) => index !== 1), TARGET, "farm").preceding.status, "unsupported");
});

test("cloud proxy is transformed pointwise before target interpolation", () => {
  const rows = [
    record("ws90", "ballydidean-ecowitt", at(0, -10), { solarRadiationWm2: 2 }),
    record("ws90", "ballydidean-ecowitt", at(0, 10), { solarRadiationWm2: 4 }),
  ];
  const label = cloudProxyLabel(rows, TARGET, "ws90_proxy", { latitude: 48, longitude: -122 },
    // make nonlinear ordering independently visible
    (_latitude, _longitude, _at, radiation) => radiation ** 2);
  assert.equal(label.value, 10);
  assert.notEqual(label.value, 3 ** 2);
  assert.equal(cloudProxyLabel(rows, TARGET, "local_mean", { latitude: 48, longitude: -122 }, () => 0).status, "unsupported");
});

test("cloud labels reuse the exact compiled ws90 proxy", () => {
  const rows = [record("ws90", "ballydidean-ecowitt", TARGET, { solarRadiationWm2: 500 })];
  const site = { latitude: 47.950429954185445, longitude: -122.42797012608193 };
  const expected = radiationCloudProxy(site.latitude, site.longitude, TARGET, 500);
  assert.notEqual(expected, null);
  assert.equal(cloudProxyLabel(rows, TARGET, "ws90_proxy", site, radiationCloudProxy).value, expected);
});

test("paired scoring weights date then hour then distinct retrieval vintage", () => {
  const rows = [
    pair("2026-06-01", 0, { adjusted: 0, raw: 1, vintageId: "a" }),
    pair("2026-06-01", 0, { adjusted: 2, raw: 3, vintageId: "b" }),
    pair("2026-06-01", 1, { adjusted: 4, raw: 5, vintageId: "a" }),
    pair("2026-06-02", 0, { adjusted: 0, raw: 1, vintageId: "a" }),
    pair("2026-06-02", 0, { adjusted: 100, raw: 100, vintageId: "a", evaluationAt: "2026-06-02T07:00:00.000Z" }),
  ];
  const score = scorePairedRows(rows, "nominal");
  assert.equal(score.rawMae, 2.25);
  assert.equal(score.adjustedMae, 1.25);
  assert.equal(score.absoluteGain, 1);
  assert.equal(score.percentGain, 100 / 2.25);
  assert.deepEqual(score.support, { changedRows: 4, dates: 2, rows: 4, targetHours: 3, vintages: 4 });
});

test("development selection uses the fixed grid with shortest then weakest one-percent tie break", () => {
  const choice = chooseDevelopmentPolicy([
    { mae: 1, policy: { horizonHours: 3, strength: 0 } },
    { mae: 0.9, policy: { horizonHours: 12, strength: 1 } },
    { mae: 0.908, policy: { horizonHours: 3, strength: 0.5 } },
    { mae: 0.907, policy: { horizonHours: 3, strength: 0.25 } },
  ]);
  assert.deepEqual(choice, { horizonHours: 3, strength: 0.25 });
  assert.throws(() => chooseDevelopmentPolicy([{ mae: 0, policy: { horizonHours: 5, strength: 0.5 } }]), /fixed grid/u);
});

test("prospective evaluator rejects the removed development alias before reading outcomes", async () => {
  await assert.rejects(() => evaluateLocalForecastCorrections({
    clockAt: "2026-10-01T00:00:00.000Z",
    issues: [],
    labelSeals: [],
    panel: pendingPanel(),
    phase: "development",
  }), /must be validation/u);
});

test("private evidence reads reject oversize symlinks and every foreign directory member", async () => {
  const privateRoot = resolve(".weather-data");
  await mkdir(privateRoot, { mode: 0o700, recursive: true });
  const root = await mkdtemp(resolve(privateRoot, "evaluator-safety-"));
  try {
    const clean = resolve(root, "clean");
    await mkdir(clean, { mode: 0o700 });
    const member = resolve(clean, "member.json");
    await writeFile(member, "{}", { mode: 0o600 });
    assert.deepEqual((await readJsonFile(member, 16, "test member")).value, {});
    await writeFile(resolve(clean, "foreign.txt"), "x", { mode: 0o600 });
    await assert.rejects(() => countEvidenceFiles(clean, 2, "test"), /foreign member/u);

    const linkedMembers = resolve(root, "linked-members");
    await mkdir(linkedMembers, { mode: 0o700 });
    await symlink(member, resolve(linkedMembers, "member.json"));
    await assert.rejects(() => countEvidenceFiles(linkedMembers, 2, "test"), /foreign member/u);
    await assert.rejects(() => readJsonFile(resolve(linkedMembers, "member.json"), 16, "test member"), /byte limit/u);

    const oversized = resolve(root, "oversized.json");
    await writeFile(oversized, JSON.stringify({ value: "x".repeat(100) }), { mode: 0o600 });
    await assert.rejects(() => readJsonFile(oversized, 32, "test member"), /byte limit/u);

    const outputTarget = resolve(root, "output-target");
    const outputLink = resolve(root, "output-link");
    await mkdir(outputTarget, { mode: 0o700 });
    await symlink(outputTarget, outputLink, "dir");
    await assert.rejects(() => writePrivateReport(resolve(outputLink, "report.json"), {}), /symbolic link/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

test("compiled closure enumeration rejects imported module and root symlinks", async () => {
  const privateRoot = resolve(".weather-data");
  await mkdir(privateRoot, { mode: 0o700, recursive: true });
  const root = await mkdtemp(resolve(privateRoot, "compiled-closure-"));
  try {
    const compiled = resolve(root, "dist");
    await mkdir(compiled, { mode: 0o700 });
    const modulePath = resolve(compiled, "module.js");
    await writeFile(modulePath, "export const value = 1;\n", { mode: 0o600 });
    assert.equal((await compiledBrowserModulePaths(compiled)).length, 1);
    const moduleLink = resolve(compiled, "imported.js");
    await symlink(modulePath, moduleLink);
    await assert.rejects(() => compiledBrowserModulePaths(compiled), /symbolic link/u);
    await unlink(moduleLink);
    const rootLink = resolve(root, "dist-link");
    await symlink(compiled, rootLink, "dir");
    await assert.rejects(() => compiledBrowserModulePaths(rootLink), /canonical directory/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

test("two-date moving bootstrap is seeded reproducible and preserves outage dates", () => {
  const targetDates = Array.from({ length: 14 }, (_unused, index) => `2026-06-${String(index + 1).padStart(2, "0")}`);
  const rows = targetDates.flatMap((date, index) => index === 5 ? [] : [pair(date, 0, { adjusted: index % 2 === 0 ? 0.5 : 0.8 })]);
  const first = bootstrapLowerGainBound(rows, targetDates, { replicates: 10_000, seed: 7 });
  const second = bootstrapLowerGainBound(rows, targetDates, { replicates: 10_000, seed: 7 });
  assert.equal(first.lowerBound, second.lowerBound);
  assert.equal(first.replicates, 10_000);
  assert.equal(first.calendarDates, 14);
  assert.equal(first.blockLengthDates, 2);
});

test("cell gate passes exact conservative support and rejects harmed affected buckets", () => {
  const dates = Array.from({ length: 14 }, (_unused, index) => `2026-06-${String(index + 1).padStart(2, "0")}`);
  const passing = dates.flatMap((date) => Array.from({ length: 6 }, (_unused, hour) => pair(date, hour)));
  const pass = evaluateCellDecision({
    cell: { cellId: "uvIndex/farm/farm", issueAnchorTier: "farm", metric: "uvIndex", policy: { horizonHours: 3, strength: 0.5 }, targetLabelTier: "farm" },
    rows: passing,
    targetDates: dates,
  });
  assert.equal(pass.status, "pass");
  assert.deepEqual(pass.reasons, []);

  const daylightShort = passing.map((row, index) => ({ ...row, daylight: index < 59 }));
  const unsupported = evaluateCellDecision({
    cell: { cellId: "uvIndex/farm/farm", issueAnchorTier: "farm", metric: "uvIndex", policy: { horizonHours: 3, strength: 0.5 }, targetLabelTier: "farm" },
    rows: daylightShort,
    targetDates: dates,
  });
  assert.equal(unsupported.status, "unsupported");
  assert.ok(unsupported.reasons.includes("primary_support"));

  const harmed = passing.map((row, index) => index % 2 === 0
    ? { ...row, adjusted: 0.5, raw: 2, observationAgeHours: 2 }
    : { ...row, adjusted: 2.1, raw: 2, observationAgeHours: 5 });
  const fail = evaluateCellDecision({
    cell: { cellId: "uvIndex/farm/farm", issueAnchorTier: "farm", metric: "uvIndex", policy: { horizonHours: 6, strength: 0.5 }, targetLabelTier: "farm" },
    rows: harmed,
    targetDates: dates,
  });
  assert.equal(fail.status, "regional");
  assert.ok(fail.reasons.includes("affected_bucket_harm"));
});

test("homogeneous masks cannot be rescued by favorable cross-tier outcomes", () => {
  const dates = Array.from({ length: 14 }, (_unused, index) => `2026-08-${String(index + 1).padStart(2, "0")}`);
  const farmHarm = dates.flatMap((date) => Array.from({ length: 6 }, (_unused, hour) => pair(date, hour, {
    adjusted: 2,
    raw: 1,
    vintageId: `farm-${hour}`,
  })));
  const localBenefit = dates.flatMap((date) => Array.from({ length: 6 }, (_unused, hour) => pair(date, hour, {
    adjusted: 0,
    issueAnchorTier: "local_mean",
    raw: 2,
    targetLabelTier: "local_mean",
    vintageId: `local-${hour}`,
  })));
  const cell = {
    cellId: "uvIndex/farm/farm",
    issueAnchorTier: "farm",
    metric: "uvIndex",
    policy: { horizonHours: 3, strength: 0.5 },
    targetLabelTier: "farm",
  };
  const partition = partitionPrimaryRows([...farmHarm, ...localBenefit], cell);
  assert.equal(partition.primary.length, 84);
  assert.equal(partition.crossTier.length, 84);
  const decision = evaluateCellDecision({ cell, rows: [...farmHarm, ...localBenefit], targetDates: dates });
  assert.equal(decision.status, "regional");
  assert.ok(decision.reasons.includes("gain_threshold"));
});

test("collection chooses target tiers independently with fixed farm-first precedence", () => {
  const manifest = pendingPanel();
  const targetAt = "2026-09-01T10:00:00.000Z";
  const forecast = record("forecast", "open-meteo-virtual", targetAt, { uvIndex: 5 }, {
    receivedAt: "2026-09-01T07:00:00.000Z",
  });
  forecast.productRunAt = "2026-09-01T06:00:00.000Z";
  forecast.provenance = { ...forecast.provenance, providerKey: "open-meteo", sourceKind: "forecast" };
  const issue = {
    current: [],
    evaluatedAt: "2026-09-01T07:00:00.000Z",
    forecast: [forecast],
    pressureContext: [],
    site: manifest.site,
    solarCloudEvidence: null,
  };
  const runtime = (tier) => ({
    clearSkyRadiation: () => ({ irradianceWm2: 500, solarElevationDegrees: 30 }),
    forecastCorrectionAnchor: () => ({ observedAt: "2026-09-01T06:00:00.000Z", tier }),
    forecastCorrectionAnchorTier: () => tier,
    forecastMetricValue: (row) => row.metrics.uvIndex,
    forecastPressureSeries: () => [],
    radiationCloudProxy,
    withLocalForecastAdjustments: (state) => state,
    withSolarCloudAdjustment: (state) => state,
  });
  const localLabels = new Map([["2026-09-01", {
    rows: [record("neighbor", "tempest-neighbor", targetAt, { uvIndex: 4 })],
    status: "complete",
  }]]);
  const farmCell = manifest.cells.find((cell) => cell.cellId === "uvIndex/farm/farm");
  const farmAgainstLocal = collectCellPairs([issue], localLabels, manifest, farmCell, runtime("farm"));
  assert.equal(farmAgainstLocal.counts.matched, 0);
  assert.equal(farmAgainstLocal.counts.crossTierExcluded, 1);

  const farmLabels = new Map([["2026-09-01", {
    rows: [record("farm", "ballydidean-ecowitt", targetAt, { uvIndex: 4 })],
    status: "complete",
  }]]);
  const localCell = manifest.cells.find((cell) => cell.cellId === "uvIndex/local_mean/local_mean");
  const localAgainstFarm = collectCellPairs([issue], farmLabels, manifest, localCell, runtime("local_mean"));
  assert.equal(localAgainstFarm.counts.matched, 0);
  assert.equal(localAgainstFarm.counts.crossTierExcluded, 1);
});

test("retrospective development stays nonqualifying and selects only from the fixed grid", async () => {
  const targetAt = "2026-09-01T10:00:00.000Z";
  const forecast = record("forecast", "open-meteo-virtual", targetAt, { uvIndex: 5 }, {
    receivedAt: "2026-09-01T07:00:00.000Z",
  });
  forecast.productRunAt = "2026-09-01T06:00:00.000Z";
  forecast.provenance = { ...forecast.provenance, providerKey: "open-meteo", sourceKind: "forecast" };
  const runtime = {
    clearSkyRadiation: () => ({ irradianceWm2: 500, solarElevationDegrees: 30 }),
    forecastCorrectionAnchor: (_row, metric) => ({ observedAt: "2026-09-01T06:00:00.000Z", tier: metric === "cloudCoverPercent" ? "ws90" : "farm" }),
    forecastCorrectionAnchorTier: (_row, metric) => metric === "cloudCoverPercent" ? "ws90" : "farm",
    forecastMetricValue: (row, metric) => metric === "cloudCoverPercent" ? row.metrics.cloudCoverPercent : row.metrics.uvIndex,
    forecastPressureSeries: () => [null],
    radiationCloudProxy,
    withLocalForecastAdjustments: (state) => state,
    withSolarCloudAdjustment: (state) => state,
  };
  const report = await evaluateRetrospectiveDevelopment({
    historyRows: [record("farm", "ballydidean-ecowitt", targetAt, { uvIndex: 4 })],
    issues: [{ current: [], evaluatedAt: "2026-09-01T07:00:00.000Z", forecast: [forecast], pressureContext: [], solarCloudEvidence: null }],
    retrievedAt: "2026-09-02T07:00:00.000Z",
    runtime,
    site: { latitude: 48, longitude: -122, siteKey: "ballydidean", timeZone: "America/Los_Angeles" },
  });
  assert.equal(report.state, "retrospective_development_only");
  assert.equal(report.claims.candidateOutput, "retrospective_development_counterfactual");
  assert.equal(report.cells.length, 5);
  assert.ok(report.cells.every((cell) => cell.candidateScores.length === 17));
  assert.equal(Object.hasOwn(report, "qualification"), false);
});

test("pressure support and gain boundaries are enforced at their exact thresholds", () => {
  const dates = Array.from({ length: 14 }, (_unused, index) => `2026-09-${String(index + 1).padStart(2, "0")}`);
  const cell = {
    cellId: "pressureChange3hHpa/farm/farm",
    issueAnchorTier: "farm",
    metric: "pressureChange3hHpa",
    policy: { horizonHours: 3, strength: 0.5 },
    targetLabelTier: "farm",
  };
  const exact = dates.slice(0, 10).flatMap((date) => Array.from({ length: 10 }, (_unused, hour) => pair(date, hour, {
    adjusted: 0.57,
    raw: 0.6,
    vintageId: `pressure-${hour}`,
  })));
  assert.equal(evaluateCellDecision({ cell, rows: exact, targetDates: dates }).status, "pass");
  const belowSupport = evaluateCellDecision({ cell, rows: exact.slice(0, 99), targetDates: dates });
  assert.equal(belowSupport.status, "unsupported");
  assert.ok(belowSupport.reasons.includes("primary_support"));
  const belowGain = exact.map((row) => ({ ...row, adjusted: 0.5701 }));
  const rejected = evaluateCellDecision({ cell, rows: belowGain, targetDates: dates });
  assert.equal(rejected.status, "regional");
  assert.ok(rejected.reasons.includes("gain_threshold"));
  const above = exact.map((row) => ({ ...row, adjusted: 0.56 }));
  assert.equal(evaluateCellDecision({ cell, rows: above, targetDates: dates }).status, "pass");
});

test("zero raw error cannot earn a correction and missing sensitivity stays regional", () => {
  const dates = Array.from({ length: 14 }, (_unused, index) => `2026-07-${String(index + 1).padStart(2, "0")}`);
  const zero = dates.flatMap((date) => Array.from({ length: 6 }, (_unused, hour) => pair(date, hour, {
    adjusted: 0,
    labels: { following: null, nominal: 0, preceding: null },
    raw: 0,
  })));
  const decision = evaluateCellDecision({
    cell: { cellId: "uvIndex/local_mean/local_mean", issueAnchorTier: "local_mean", metric: "uvIndex", policy: { horizonHours: 3, strength: 0.5 }, targetLabelTier: "local_mean" },
    rows: zero.map((row) => ({ ...row, issueAnchorTier: "local_mean", targetLabelTier: "local_mean" })),
    targetDates: dates,
  });
  assert.equal(decision.status, "unsupported");
  assert.ok(decision.reasons.includes("zero_raw_mae"));
  assert.ok(decision.reasons.includes("uv_sensitivity_support"));
});

test("panel classification keeps incomplete evidence pending rather than tested failed", () => {
  const reports = (definitions) => definitions.map(([strength, status]) => ({
    decision: { status },
    policy: { horizonHours: 3, strength },
  }));
  assert.equal(classifyPanelDecision({ cellReports: [], completeTargetDates: 13, targetDateCount: 14 }), "regional_pending_or_unsupported");
  assert.equal(classifyPanelDecision({
    cellReports: reports([[0, "unsupported"], [0.5, "regional"], [0.5, "unsupported"], [0, "unsupported"], [0, "unsupported"]]),
    completeTargetDates: 14,
    targetDateCount: 14,
  }), "regional_pending_or_unsupported");
  assert.equal(classifyPanelDecision({
    cellReports: reports([[0, "unsupported"], [0.5, "pass"], [0.5, "unsupported"], [0, "unsupported"], [0, "unsupported"]]),
    completeTargetDates: 14,
    targetDateCount: 14,
  }), "validated_candidate_pending_release");
  assert.equal(classifyPanelDecision({
    cellReports: reports([[0, "unsupported"], [0, "unsupported"], [0, "unsupported"], [0, "unsupported"], [0, "unsupported"]]),
    completeTargetDates: 14,
    targetDateCount: 14,
  }), "regional_pending_or_unsupported");
  assert.equal(classifyPanelDecision({
    cellReports: reports([[0.25, "regional"], [0.5, "regional"], [0.75, "regional"], [1, "regional"], [0, "unsupported"]]),
    completeTargetDates: 14,
    targetDateCount: 14,
  }), "regional_tested_no_winner");
});

test("pending report exposes coverage only and no partial outcome scores", () => {
  const report = buildPendingLocalForecastCorrectionReport(pendingPanel(), 12, 13);
  assert.equal(report.state, "regional_pending_or_unsupported");
  assert.deepEqual(report.cells, []);
  assert.equal(report.evidenceInputHash, null);
  assert.deepEqual(report.coverage, {
    expectedIssues: report.coverage.expectedIssues,
    expectedLabelDates: 14,
    issueMemberCount: 12,
    labelMemberCount: 13,
  });
  assert.equal(Object.hasOwn(report, "score"), false);
});
