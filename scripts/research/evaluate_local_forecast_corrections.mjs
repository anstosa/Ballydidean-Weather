#!/usr/bin/env node

import { lstat, mkdir, open, readFile, readdir, realpath } from "node:fs/promises";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  canonicalJsonBytes,
  localDateBounds,
  LOCAL_FORECAST_CORRECTION_CANDIDATE_GRID,
  LOCAL_FORECAST_CORRECTION_MULTIPLICITY,
  LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS,
  sha256Hex,
  validateLocalForecastCorrectionPanel,
  validateLocalForecastIssueSnapshot,
  validateLocalForecastLabelSeal,
  validateLocalForecastPhysicalSource,
  validateLocalForecastWs90Source,
} from "./local_forecast_correction_contract.mjs";

export const LOCAL_FORECAST_CORRECTION_EVALUATION_VERSION =
  "weather-local-correction-evaluation/v1";
export const LOCAL_FORECAST_CORRECTION_RETROSPECTIVE_VERSION =
  "weather-local-correction-retrospective-development/v1";

const HOUR_MS = 3_600_000;
const MINUTE_MS = 60_000;
const POINT_RADIUS_MS = 15 * MINUTE_MS;
const MAX_PHYSICAL_GAP_MS = 30 * MINUTE_MS;
const MAX_EVIDENCE_BYTES = 128 * 1_024 * 1_024;
const MAX_ISSUE_BYTES = 2 * 1_024 * 1_024;
const MAX_LABEL_BYTES = 24 * 1_024 * 1_024;
const FARM_STATION = "ballydidean-ecowitt";
const POLICY_FLOORS = Object.freeze({
  cloudCoverPercent: 1,
  pressureChange3hHpa: 0.03,
  uvIndex: 0.03,
});
const METRIC_BOUNDS = Object.freeze({
  pressureHpa: Object.freeze({ maximum: 1_200, minimum: 800 }),
  solarRadiationWm2: Object.freeze({ maximum: 2_500, minimum: 0 }),
  uvIndex: Object.freeze({ maximum: 20, minimum: 0 }),
});
const ZERO_POLICY = Object.freeze({ horizonHours: 3, strength: 0 });
const FIXED_GRID_KEYS = new Set(LOCAL_FORECAST_CORRECTION_CANDIDATE_GRID.map(policyKey));
const repositoryRoot = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const privateEvidenceRoot = resolve(repositoryRoot, ".weather-data");

// hash the capture evaluator contract and compiled inference closure
export async function computeLocalForecastCorrectionCodeHash() {
  const paths = [
    ...await compiledBrowserModulePaths(resolve(repositoryRoot, "apps/web/dist")),
    "scripts/research/capture_local_forecast_corrections.mjs",
    "scripts/research/evaluate_local_forecast_corrections.mjs",
    "scripts/research/local_forecast_correction_contract.mjs",
  ].toSorted();
  const members = [];
  for (const path of paths) {
    // bind every executable or contract member by exact bytes
    const bytes = await readFile(resolve(repositoryRoot, path));
    members.push({ path, sha256: sha256Hex(bytes) });
  }
  return sha256Hex(canonicalJsonBytes(members));
}

// build a deterministic point label from one homogeneous physical tier
export function pointLabel(rows, metric, targetAt, targetTier) {
  const target = parseInstant(targetAt, "targetAt");
  const resolved = resolveSourceSeries(rows, metric);
  // quarantine contradictory immutable revisions
  if (resolved.status === "unsupported") {
    return unsupported(resolved.reason);
  }
  const values = [];
  for (const source of resolved.sources.values()) {
    // interpolate only within one source identity
    const value = interpolateSamples(source.samples, target);
    if (value !== null) {
      values.push({ ...source, value });
    }
  }
  return aggregateTier(values, targetTier);
}

// build an exact three-hour pressure tendency from complete source-local spans
export function pressureTendencyLabel(rows, targetAt, targetTier) {
  const target = parseInstant(targetAt, "targetAt");
  const resolved = resolveSourceSeries(rows, "pressureHpa");
  // quarantine contradictory immutable revisions
  if (resolved.status === "unsupported") {
    return unsupported(resolved.reason);
  }
  const start = target - 3 * HOUR_MS;
  const values = [];
  let incomplete = false;
  for (const source of resolved.sources.values()) {
    // require both endpoints and continuous physical sampling
    const first = interpolateSamples(source.samples, start);
    const last = interpolateSamples(source.samples, target);
    if (first === null || last === null || !completeCoverage(source.samples, start, target)) {
      incomplete = true;
      continue;
    }
    values.push({ ...source, value: last - first });
  }
  const aggregated = aggregateTier(values, targetTier);
  // distinguish a broken span from a wholly absent tier
  if (aggregated.status === "unsupported" && incomplete) {
    return unsupported("incomplete_pressure_span");
  }
  return aggregated;
}

// build nominal and two complete one-hour UV interpretations
export function uvSensitivityLabels(rows, targetAt, targetTier) {
  const target = parseInstant(targetAt, "targetAt");
  const resolved = resolveSourceSeries(rows, "uvIndex");
  // propagate one conflicting revision to every interpretation
  if (resolved.status === "unsupported") {
    const missing = unsupported(resolved.reason);
    return Object.freeze({ following: missing, nominal: missing, preceding: missing });
  }
  const nominalValues = [];
  const precedingValues = [];
  const followingValues = [];
  for (const source of resolved.sources.values()) {
    // keep every interpretation within the same physical source
    const nominal = interpolateSamples(source.samples, target);
    const preceding = sourceWindowMean(source.samples, target - HOUR_MS, target);
    const following = sourceWindowMean(source.samples, target, target + HOUR_MS);
    if (nominal !== null) {
      nominalValues.push({ ...source, value: nominal });
    }
    if (preceding !== null) {
      precedingValues.push({ ...source, value: preceding });
    }
    if (following !== null) {
      followingValues.push({ ...source, value: following });
    }
  }
  return Object.freeze({
    following: aggregateTier(followingValues, targetTier),
    nominal: aggregateTier(nominalValues, targetTier),
    preceding: aggregateTier(precedingValues, targetTier),
  });
}

// build the fixed ws90 cloud proxy before point interpolation
export function cloudProxyLabel(rows, targetAt, targetTier, site, radiationCloudProxy) {
  // prohibit nearby or modeled cloud labels by construction
  if (targetTier !== "ws90_proxy") {
    return unsupported("inadmissible_cloud_tier");
  }
  // require the actual shared nonlinear estimator
  if (typeof radiationCloudProxy !== "function") {
    throw new TypeError("radiationCloudProxy is required");
  }
  const target = parseInstant(targetAt, "targetAt");
  const resolved = resolveSourceSeries(rows, "solarRadiationWm2", (row, value) =>
    radiationCloudProxy(site.latitude, site.longitude, row.validAt, value));
  // quarantine contradictory immutable revisions
  if (resolved.status === "unsupported") {
    return unsupported(resolved.reason);
  }
  const values = [];
  for (const source of resolved.sources.values()) {
    // interpolate already-transformed same-time proxy points
    const value = interpolateSamples(source.samples, target);
    if (value !== null) {
      values.push({ ...source, value });
    }
  }
  return aggregateTier(values, targetTier);
}

// score paired rows with equal date hour and distinct-vintage weights
export function scorePairedRows(rows, labelKey = "nominal") {
  const paired = deduplicatePairs(rows).filter(
    // retain only identical-mask finite raw adjusted and target values
    (row) => Number.isFinite(row.raw) && Number.isFinite(row.adjusted) &&
      Number.isFinite(row.labels?.[labelKey]),
  );
  // preserve an explicit unsupported score rather than NaN
  if (paired.length === 0) {
    return Object.freeze({
      absoluteGain: null,
      adjustedMae: null,
      percentGain: null,
      rawMae: null,
      support: Object.freeze({ changedRows: 0, dates: 0, rows: 0, targetHours: 0, vintages: 0 }),
    });
  }
  const rawMae = hierarchicalMean(paired, (row) => Math.abs(row.raw - row.labels[labelKey]));
  const adjustedMae = hierarchicalMean(paired, (row) => Math.abs(row.adjusted - row.labels[labelKey]));
  const absoluteGain = rawMae - adjustedMae;
  return Object.freeze({
    absoluteGain,
    adjustedMae,
    percentGain: rawMae === 0 ? null : 100 * absoluteGain / rawMae,
    rawMae,
    support: Object.freeze({
      changedRows: paired.filter(
        // count exact runtime numerical changes only
        (row) => row.raw !== row.adjusted,
      ).length,
      dates: new Set(paired.map((row) => row.localDate)).size,
      rows: paired.length,
      targetHours: new Set(paired.map((row) => `${row.localDate}/${row.targetAt}`)).size,
      vintages: new Set(paired.map((row) => `${row.localDate}/${row.targetAt}/${row.vintageId}`)).size,
    }),
  });
}

// separate the predeclared homogeneous primary mask from cross-tier diagnostics
export function partitionPrimaryRows(rows, cell) {
  const primary = [];
  const crossTier = [];
  for (const row of rows) {
    // target outcomes can never select a different issue-anchor regime
    if (row.issueAnchorTier === cell.issueAnchorTier &&
        row.targetLabelTier === cell.targetLabelTier) {
      primary.push(row);
    } else {
      crossTier.push(row);
    }
  }
  return Object.freeze({ crossTier: Object.freeze(crossTier), primary: Object.freeze(primary) });
}

// apply the exact future issue-lead boundary
export function eligibleFutureTarget(evaluatedAt, targetAt) {
  const evaluated = parseInstant(evaluatedAt, "evaluatedAt");
  const target = parseInstant(targetAt, "targetAt");
  const lead = target - evaluated;
  return lead > 0 && lead <= 24 * HOUR_MS;
}

// select a development candidate with the frozen one-percent tie rule
export function chooseDevelopmentPolicy(candidateScores) {
  // require at least one reproducible candidate score
  if (!Array.isArray(candidateScores) || candidateScores.length === 0) {
    throw new RangeError("development candidate scores are absent");
  }
  for (const candidate of candidateScores) {
    // reject an expanded or malformed policy search
    if (candidate === null || typeof candidate !== "object" ||
        !FIXED_GRID_KEYS.has(policyKey(candidate.policy)) ||
        !Number.isFinite(candidate.mae) || candidate.mae < 0) {
      throw new TypeError("development candidate is outside the fixed grid");
    }
  }
  const minimum = Math.min(...candidateScores.map(
    // compare the exact date-balanced development errors
    (candidate) => candidate.mae,
  ));
  const tied = candidateScores.filter(
    // admit no more than one percent relative error from the minimum
    (candidate) => minimum === 0 ? candidate.mae === 0 : candidate.mae <= minimum * 1.01,
  ).toSorted(
    // prefer shortest horizon then weakest strength
    (left, right) => left.policy.horizonHours - right.policy.horizonHours ||
      left.policy.strength - right.policy.strength,
  );
  return Object.freeze({ ...tied[0].policy });
}

// compute the seeded two-local-date moving-block lower gain bound
export function bootstrapLowerGainBound(
  rows,
  targetDates,
  { replicates = 10_000, seed = 20_261_014 } = {},
) {
  // retain the exact predeclared bootstrap size in production
  if (!Number.isInteger(replicates) || replicates !== 10_000) {
    throw new RangeError("bootstrap requires 10000 replicates");
  }
  // preserve the complete fourteen-date calendar including outages
  if (!Array.isArray(targetDates) || targetDates.length !== 14 || new Set(targetDates).size !== 14) {
    throw new RangeError("bootstrap requires fourteen distinct target dates");
  }
  const byDate = new Map(targetDates.map(
    // retain empty outage dates in the resampling frame
    (date) => [date, deduplicatePairs(rows.filter((row) => row.localDate === date))],
  ));
  const random = seededRandom(seed);
  const gains = [];
  for (let replicate = 0; replicate < replicates; replicate += 1) {
    // resample adjacent two-date blocks onto fourteen labeled slots
    const sampled = [];
    let slot = 0;
    while (slot < targetDates.length) {
      // sample one circular adjacent block
      const start = Math.floor(random() * targetDates.length);
      for (let offset = 0; offset < 2 && slot < targetDates.length; offset += 1) {
        // keep repeated dates as distinct balanced bootstrap slots
        const date = targetDates[(start + offset) % targetDates.length];
        for (const row of byDate.get(date)) {
          // relabel only the bootstrap hierarchy identity
          sampled.push({ ...row, localDate: `${slot}/${date}` });
        }
        slot += 1;
      }
    }
    const score = scorePairedRows(sampled, "nominal");
    // exclude empty resamples without manufacturing a zero gain
    if (score.absoluteGain !== null) {
      gains.push(score.absoluteGain);
    }
  }
  // require enough valid draws for a meaningful lower percentile
  if (gains.length < Math.ceil(replicates * 0.99)) {
    return Object.freeze({
      blockLengthDates: 2,
      calendarDates: targetDates.length,
      lowerBound: null,
      replicates,
      validReplicates: gains.length,
    });
  }
  gains.sort((left, right) => left - right);
  const percentileIndex = Math.floor((gains.length - 1) * 0.01);
  return Object.freeze({
    blockLengthDates: 2,
    calendarDates: targetDates.length,
    lowerBound: gains[percentileIndex],
    replicates,
    validReplicates: gains.length,
  });
}

// apply every predeclared support gain uncertainty and no-harm gate
export function evaluateCellDecision({ cell, rows, targetDates }) {
  const primaryRows = partitionPrimaryRows(rows, cell).primary;
  const reasons = [];
  // a development-selected zero remains regional without pretending it failed validation
  if (cell.policy.strength === 0) {
    return Object.freeze({
      bootstrap: null,
      buckets: Object.freeze([]),
      policy: cell.policy,
      reasons: Object.freeze(["zero_policy"]),
      score: scorePairedRows(primaryRows, "nominal"),
      status: "unsupported",
      uvSensitivity: null,
    });
  }
  const score = scorePairedRows(primaryRows, "nominal");
  const minimumHours = cell.metric === "pressureChange3hHpa" ? 100 : 60;
  const qualifyingScore = cell.metric === "uvIndex"
    ? scorePairedRows(primaryRows.filter(
      // count the declared geometry-qualified daylight support separately
      (row) => row.daylight,
    ), "nominal")
    : score;
  // require the primary homogeneous panel support
  if (score.support.dates < 10 || qualifyingScore.support.targetHours < minimumHours) {
    reasons.push("primary_support");
  }
  // zero baseline error cannot produce a percentage improvement claim
  if (score.rawMae === 0) {
    reasons.push("zero_raw_mae");
  }
  // require both relative and metric-specific absolute gain floors
  if (score.percentGain === null || score.percentGain + 1e-12 < 5 ||
      score.absoluteGain === null || score.absoluteGain + 1e-12 < POLICY_FLOORS[cell.metric]) {
    reasons.push("gain_threshold");
  }
  const bootstrap = bootstrapLowerGainBound(primaryRows, targetDates);
  // use the fixed bonferroni lower first percentile for M five
  if (bootstrap.lowerBound === null || bootstrap.lowerBound <= 0) {
    reasons.push(bootstrap.lowerBound === null ? "bootstrap_support" : "bootstrap_lower_bound");
  }
  const affected = deduplicatePairs(primaryRows).filter(
    // retain only exact numerical changes in the candidate window
    (row) => row.raw !== row.adjusted,
  );
  const affectedScore = scorePairedRows(affected, "nominal");
  // require a supported strictly positive affected-window aggregate
  if (affectedScore.support.dates < 8 || affectedScore.support.targetHours < 20 ||
      affectedScore.absoluteGain === null) {
    reasons.push("affected_window_support");
  } else if (affectedScore.absoluteGain <= 0) {
    reasons.push("affected_window_gain");
  }
  // forbid any numerical change at or beyond the frozen horizon
  if (deduplicatePairs(primaryRows).some(
    // inspect observation age rather than issue lead
    (row) => row.observationAgeHours >= cell.policy.horizonHours && row.raw !== row.adjusted,
  )) {
    reasons.push("outside_horizon_changed");
  }
  const buckets = evaluateAffectedBuckets(primaryRows);
  // every changed age bucket must be supported and nonharmful
  if (buckets.some((bucket) => bucket.status === "unsupported")) {
    reasons.push("affected_bucket_support");
  }
  // supported harmful buckets reject the frozen candidate
  if (buckets.some((bucket) => bucket.status === "regional")) {
    reasons.push("affected_bucket_harm");
  }
  // matched whole-day score must not conceal later harm
  if (score.absoluteGain === null || score.absoluteGain < 0) {
    reasons.push("whole_24h_harm");
  }
  const uvSensitivity = cell.metric === "uvIndex" ? evaluateUvSensitivity(primaryRows) : null;
  // both timing interpretations need their own supported nonharmful panels
  if (uvSensitivity !== null && !uvSensitivity.supported) {
    reasons.push("uv_sensitivity_support");
  }
  // supported but harmful sensitivity panels reject UV
  if (uvSensitivity !== null && uvSensitivity.supported && !uvSensitivity.nonharmful) {
    reasons.push("uv_sensitivity_harm");
  }
  const unsupportedReasons = new Set([
    "affected_bucket_support", "affected_window_support", "primary_support",
    "bootstrap_support", "uv_sensitivity_support",
  ]);
  const status = reasons.length === 0 ? "pass" : reasons.some(
    // incomplete support is unsupported rather than a tested rejection
    (reason) => unsupportedReasons.has(reason),
  ) ? "unsupported" : "regional";
  return Object.freeze({
    affectedScore,
    bootstrap,
    buckets,
    issueLeadUnderOneHour: scorePairedRows(primaryRows.filter(
      // report the easily omitted short issue-lead interval separately
      (row) => row.issueLeadHours > 0 && row.issueLeadHours <= 1,
    ), "nominal"),
    policy: cell.policy,
    qualifyingSupport: qualifyingScore.support,
    reasons: Object.freeze([...new Set(reasons)]),
    score,
    status,
    uvSensitivity,
  });
}

// classify terminal panel evidence without calling incomplete work a failure
export function classifyPanelDecision({ cellReports, completeTargetDates, targetDateCount }) {
  // missing one-shot seals leave positive validation pending
  if (completeTargetDates < targetDateCount) {
    return "regional_pending_or_unsupported";
  }
  // classification accounts for every frozen cell while ignoring zero-policy outcomes
  if (!Array.isArray(cellReports) || cellReports.length !== LOCAL_FORECAST_CORRECTION_MULTIPLICITY) {
    throw new RangeError("panel cell decisions are incomplete");
  }
  const positive = cellReports.filter(
    // zero policies are neither failed nor unresolved positive candidates
    (report) => report.policy.strength > 0,
  );
  // no positive candidate means unsupported rather than a tested rejection
  if (positive.length === 0) {
    return "regional_pending_or_unsupported";
  }
  // any passing positive candidate still needs the separate exact release commit
  if (positive.some((report) => report.decision.status === "pass")) {
    return "validated_candidate_pending_release";
  }
  // one unresolved positive candidate prevents an all-tested rejection claim
  if (positive.some((report) => report.decision.status !== "regional")) {
    return "regional_pending_or_unsupported";
  }
  // every positive frozen candidate has been tested and rejected
  return "regional_tested_no_winner";
}

// evaluate captured public inputs through the compiled browser inference path
export async function evaluateLocalForecastCorrections({
  clockAt,
  issues,
  labelSeals,
  panel,
  phase = "validation",
  runtime = null,
}) {
  validateLocalForecastCorrectionPanel(panel);
  const clock = parseInstant(clockAt, "clockAt");
  // keep prospective evaluation exclusively on the sealed validation path
  if (phase !== "validation") {
    throw new TypeError("prospective evaluation phase must be validation");
  }
  const codeHash = await computeLocalForecastCorrectionCodeHash();
  // reject changed evaluator capture or compiled runtime bytes after freeze
  if (panel.codeHash !== codeHash) {
    throw new TypeError("panel code hash differs from executable closure");
  }
  // inspect no outcome members before the complete panel terminal deadline
  if (!Array.isArray(issues) || !Array.isArray(labelSeals) ||
      issues.length > panel.issueSchedule.length || labelSeals.length > panel.targetDateCount) {
    throw new RangeError("evidence member count is invalid");
  }
  const finalSealDeadline = Math.max(...panel.labelSchedule.map(
    // include the approved five-minute one-shot query deadline
    (seal) => Date.parse(seal.sealAt) + 5 * MINUTE_MS,
  ));
  if (labelSeals.length < panel.targetDateCount || clock < finalSealDeadline) {
    return buildPendingLocalForecastCorrectionReport(panel, issues.length, labelSeals.length);
  }
  const browser = runtime ?? await loadCompiledRuntime();
  const normalizedIssues = validateIssuesAgainstPanel(issues, panel);
  const normalizedLabels = validateLabelsAgainstPanel(labelSeals, panel);
  // a nominally full directory cannot silently score duplicate terminal dates
  if (normalizedLabels.length !== panel.targetDateCount) {
    throw new RangeError("terminal label seals are incomplete");
  }
  const labelByDate = new Map(normalizedLabels.map(
    // retain one immutable seal per target date
    (labels) => [labels.targetDate, labels],
  ));
  const terminalLabelDates = normalizedLabels.length;
  const evidenceInputHash = sha256Hex(canonicalJsonBytes({
    issues: normalizedIssues.map(
      // bind the aggregate report without exposing raw source metadata
      (issue) => issue.normalizedDigest,
    ).sort(),
    labels: normalizedLabels.map(
      // bind every one-shot label member
      (labels) => labels.normalizedDigest,
    ).sort(),
    panelDigest: panel.panelDigest,
  }));
  const complete = terminalLabelDates === panel.targetDateCount;
  const cellReports = [];
  for (const cell of panel.cells) {
    // preserve each predeclared homogeneous comparison independently
    const collection = collectCellPairs(normalizedIssues, labelByDate, panel, cell, browser);
    const zeroCollection = cell.policy.strength === 0 ? collection : collectCellPairs(
      normalizedIssues,
      labelByDate,
      panel,
      { ...cell, policy: ZERO_POLICY },
      browser,
    );
    // candidate arithmetic cannot change support masks or target weights
    if (collection.maskHash !== zeroCollection.maskHash) {
      throw new TypeError("candidate mask differs from zero policy");
    }
    const decision = complete
      ? evaluateCellDecision({ cell, rows: collection.rows, targetDates: panel.targetDates })
      : Object.freeze({
        reasons: Object.freeze(["panel_incomplete"]),
        score: scorePairedRows(collection.rows, "nominal"),
        status: "pending",
      });
    cellReports.push(Object.freeze({
      cellId: cell.cellId,
      counts: collection.counts,
      crossTierDiagnostics: collection.crossTierDiagnostics,
      decision,
      issueAnchorTier: cell.issueAnchorTier,
      metric: cell.metric,
      maskHash: collection.maskHash,
      policy: cell.policy,
      targetLabelTier: cell.targetLabelTier,
    }));
  }
  const state = classifyPanelDecision({
    cellReports,
    completeTargetDates: terminalLabelDates,
    targetDateCount: panel.targetDateCount,
  });
  return Object.freeze({
    cells: Object.freeze(cellReports),
    claims: panel.claims,
    contractVersion: LOCAL_FORECAST_CORRECTION_EVALUATION_VERSION,
    coverage: Object.freeze({
      capturedIssues: normalizedIssues.length,
      completeLabelDates: normalizedLabels.filter(
        // count only successfully sealed physical history windows
        (labels) => labels.status === "complete",
      ).length,
      expectedIssues: panel.issueSchedule.length,
      expectedLabelDates: panel.targetDateCount,
      missedIssues: panel.issueSchedule.length - normalizedIssues.length,
      terminalLabelDates,
      unsupportedLabelDates: normalizedLabels.filter(
        // retain terminal unsupported attempts without scoring partial rows
        (labels) => labels.status === "unsupported",
      ).length,
    }),
    evidenceInputHash,
    frozenIdentity: Object.freeze({
      candidateFrozenAt: panel.candidateFrozenAt,
      codeHash: panel.codeHash,
      developmentLatestAt: panel.developmentLatestAt,
      earliestValidationInputAt: panel.earliestValidationInputAt,
      inputHash: panel.inputHash,
      panelDigest: panel.panelDigest,
      policyHash: panel.policyHash,
      targetEndAt: panel.targetEndAt,
      targetStartAt: panel.targetStartAt,
      warmupStartAt: panel.warmupStartAt,
    }),
    multiplicity: 5,
    phase,
    positiveValidationPending: state === "regional_pending_or_unsupported",
    servingPolicyState: "zero",
    state,
  });
}

// replay bounded opened history as development counterfactuals only
export async function evaluateRetrospectiveDevelopment({
  historyRows,
  issues,
  retrievedAt,
  runtime = null,
  site,
}) {
  const retrieved = parseInstant(retrievedAt, "retrievedAt");
  validateRetrospectiveSite(site);
  // bound opened legacy inputs without assigning validation semantics
  if (!Array.isArray(issues) || issues.length > 375 || !Array.isArray(historyRows) ||
      historyRows.length > 20_000) {
    throw new RangeError("retrospective development input limit exceeded");
  }
  const causalIssues = issues.map(
    // remove rows that were not available by each assumed issue time
    (issue) => normalizeRetrospectiveIssue(issue, retrieved, site),
  );
  const causalHistory = historyRows.filter(
    // public history labels are only the latest revisions visible by retrieval
    (row) => recordAvailableAt(row, retrieved),
  );
  const targetDates = [...new Set(causalIssues.flatMap((issue) => issue.forecast.flatMap((row) =>
    // retain only actual next-twenty-four-hour development targets
    eligibleFutureTarget(issue.evaluatedAt, row.validAt)
      ? [localDateAt(row.validAt, site.timeZone)]
      : [],
  )))].toSorted();
  // development replay remains finite and cannot impersonate the fixed validation calendar
  if (targetDates.length === 0 || targetDates.length > 14) {
    throw new RangeError("retrospective development target dates are absent or exceed fourteen");
  }
  const labelSchedule = targetDates.map((targetDate) => {
    // reproduce the same pressure and sensitivity halo without a one-shot seal claim
    const bounds = localDateBounds(targetDate, site.timeZone);
    return {
      from: new Date(Date.parse(bounds.startAt) - 3.5 * HOUR_MS).toISOString(),
      targetDate,
      to: new Date(Date.parse(bounds.endAt) + HOUR_MS).toISOString(),
    };
  });
  const panel = { labelSchedule, site };
  const labelByDate = new Map(targetDates.map(
    // every target reads the same bounded latest-revision development corpus
    (targetDate) => [targetDate, { rows: causalHistory, status: "complete" }],
  ));
  const codeHash = await computeLocalForecastCorrectionCodeHash();
  const browser = runtime ?? await loadCompiledRuntime();
  const cells = [];
  for (const cell of LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS) {
    // run all seventeen choices through the identical browser projection path
    const candidateScores = [];
    let zeroMaskHash = null;
    for (const policy of LOCAL_FORECAST_CORRECTION_CANDIDATE_GRID) {
      // preserve policy-independent target masks throughout development selection
      const candidate = { ...cell, policy };
      const collection = collectCellPairs(causalIssues, labelByDate, panel, candidate, browser);
      if (zeroMaskHash === null) {
        zeroMaskHash = collection.maskHash;
      } else if (collection.maskHash !== zeroMaskHash) {
        throw new TypeError("retrospective candidate mask differs from zero policy");
      }
      const score = scorePairedRows(collection.rows, "nominal");
      candidateScores.push(Object.freeze({ policy, score }));
    }
    const selectable = candidateScores.flatMap(
      // only finite supported errors can participate in the frozen tie rule
      (candidate) => candidate.score.adjustedMae === null
        ? []
        : [{ mae: candidate.score.adjustedMae, policy: candidate.policy }],
    );
    cells.push(Object.freeze({
      candidateScores: Object.freeze(candidateScores),
      cellId: cell.cellId,
      issueAnchorTier: cell.issueAnchorTier,
      metric: cell.metric,
      selectedPolicy: selectable.length === 0 ? ZERO_POLICY : chooseDevelopmentPolicy(selectable),
      targetLabelTier: cell.targetLabelTier,
    }));
  }
  return Object.freeze({
    cells: Object.freeze(cells),
    claims: Object.freeze({
      candidateOutput: "retrospective_development_counterfactual",
      inputProduct: "public_latest_revision_at_retrieval",
      servingOutput: "regional_zero",
    }),
    codeHash,
    contractVersion: LOCAL_FORECAST_CORRECTION_RETROSPECTIVE_VERSION,
    inputHash: sha256Hex(canonicalJsonBytes({ historyRows: causalHistory, issues: causalIssues, retrievedAt, site })),
    limitations: Object.freeze([
      "public_receivedAt_is_latest_not_first_receipt",
      "prior_revisions_before_retrieval_are_not_reconstructable",
      "assumed_issue_times_are_not_http_receipts",
      "development_only_not_qualification",
    ]),
    retrievedAt,
    servingPolicyState: "zero",
    state: "retrospective_development_only",
    support: Object.freeze({
      historyRows: causalHistory.length,
      issues: causalIssues.length,
      targetDates: targetDates.length,
    }),
  });
}

// collect one cell's paired runtime and physical target rows
export function collectCellPairs(issues, labelByDate, panel, cell, runtime) {
  const rows = [];
  const crossRows = [];
  const counts = {
    crossTierExcluded: 0,
    duplicateVintageExcluded: 0,
    matched: 0,
    missingAnchor: 0,
    missingLabel: 0,
    missingPrediction: 0,
  };
  for (const issue of issues) {
    // invoke the actual browser wrapper for one frozen cell policy
    const projected = projectIssue(issue, cell, runtime);
    const eligible = eligibleForecastRows(issue, panel);
    let rawPressure = null;
    let adjustedPressure = null;
    if (cell.metric === "pressureChange3hHpa") {
      // use the shared first-three-hour context seam for raw and adjusted values
      rawPressure = runtime.forecastPressureSeries(
        issue.forecast,
        eligible.map((entry) => entry.record),
        issue.pressureContext,
        false,
      );
      adjustedPressure = runtime.forecastPressureSeries(
        projected.forecast,
        eligible.map((entry) => projected.forecast[entry.index]),
        issue.pressureContext,
        true,
      );
    }
    for (let position = 0; position < eligible.length; position += 1) {
      // retain one paired target from the identical forecast index
      const entry = eligible[position];
      const rawRecord = entry.record;
      const adjustedRecord = projected.forecast[entry.index];
      const raw = cell.metric === "pressureChange3hHpa"
        ? rawPressure[position]
        : runtime.forecastMetricValue(rawRecord, cell.metric, false);
      const adjusted = cell.metric === "pressureChange3hHpa"
        ? adjustedPressure[position]
        : runtime.forecastMetricValue(adjustedRecord, cell.metric, true);
      // missing runtime values cannot enter either side's mask
      if (!Number.isFinite(raw) || !Number.isFinite(adjusted)) {
        counts.missingPrediction += 1;
        continue;
      }
      const anchor = runtime.forecastCorrectionAnchor(adjustedRecord, cell.metric);
      const indexedTier = runtime.forecastCorrectionAnchorTier(adjustedRecord, cell.metric);
      // require both trusted runtime index helpers to agree
      if (anchor === null || indexedTier === null || anchor.tier !== indexedTier) {
        counts.missingAnchor += 1;
        continue;
      }
      const labels = labelByDate.get(entry.localDate);
      // incomplete history attempts supply no favorable partial labels
      if (labels === undefined || labels.status !== "complete") {
        counts.missingLabel += 1;
        continue;
      }
      const target = independentLabelsForTarget(
        labels.rows,
        cell.metric,
        rawRecord.validAt,
        panel.site,
        runtime,
      );
      // require a supported nominal physical target
      if (target === null) {
        counts.missingLabel += 1;
        continue;
      }
      const { labelSet, targetTier } = target;
      const observedAt = parseInstant(anchor.observedAt, "runtime anchor observedAt");
      const targetAt = Date.parse(rawRecord.validAt);
      const evaluationAt = Date.parse(issue.evaluatedAt);
      const sky = cell.metric === "uvIndex"
        ? runtime.clearSkyRadiation(panel.site.latitude, panel.site.longitude, rawRecord.validAt)
        : null;
      const paired = {
        adjusted,
        daylight: sky !== null && sky.solarElevationDegrees >= 15 && sky.irradianceWm2 >= 200,
        evaluationAt: issue.evaluatedAt,
        issueLeadHours: (targetAt - evaluationAt) / HOUR_MS,
        issueAnchorTier: anchor.tier,
        labels: {
          following: labelSet.following.status === "supported" ? labelSet.following.value : null,
          nominal: labelSet.nominal.value,
          preceding: labelSet.preceding.status === "supported" ? labelSet.preceding.value : null,
        },
        localDate: entry.localDate,
        observationAgeHours: (targetAt - observedAt) / HOUR_MS,
        raw,
        targetAt: rawRecord.validAt,
        targetLabelTier: targetTier,
        vintageId: `${rawRecord.provenance.sourceId}/${rawRecord.productRunAt ?? ""}`,
      };
      // retain cross-tier errors as diagnostics without qualification weight
      if (anchor.tier !== cell.issueAnchorTier || targetTier !== cell.targetLabelTier) {
        counts.crossTierExcluded += 1;
        crossRows.push(paired);
        continue;
      }
      rows.push(paired);
      counts.matched += 1;
    }
  }
  const deduplicated = deduplicatePairs(rows);
  counts.duplicateVintageExcluded = rows.length - deduplicated.length;
  counts.matched = deduplicated.length;
  return Object.freeze({
    counts: Object.freeze(counts),
    crossTierDiagnostics: scorePairedRows(crossRows, "nominal"),
    maskHash: pairedMaskHash(deduplicated),
    rows: Object.freeze(deduplicated),
  });
}

// apply one cell policy through both browser wrapper families
function projectIssue(issue, cell, runtime) {
  const policies = zeroPolicyTable();
  policies[cell.metric][cell.issueAnchorTier] = cell.policy;
  const state = {
    current: issue.current,
    forecast: issue.forecast,
    forecastAdjustmentMode: "adjusted",
    selectedSite: {
      latitude: issue.site.latitude,
      longitude: issue.site.longitude,
      name: issue.site.siteKey,
      slug: issue.site.siteKey,
      stations: [],
      timezone: issue.site.timeZone,
    },
    solarCloudEvidence: issue.solarCloudEvidence,
  };
  const now = new Date(issue.evaluatedAt);
  const cloud = runtime.withSolarCloudAdjustment(state, now, policies);
  return runtime.withLocalForecastAdjustments(cloud, now, policies);
}

// construct explicit zero defaults without trusting serialized policy input
function zeroPolicyTable() {
  return {
    cloudCoverPercent: { ws90: ZERO_POLICY },
    pressureChange3hHpa: { farm: ZERO_POLICY, local_mean: ZERO_POLICY },
    uvIndex: { farm: ZERO_POLICY, local_mean: ZERO_POLICY },
  };
}

// select future next-twenty-four-hour forecast records in panel dates
function eligibleForecastRows(issue, panel) {
  const evaluationAt = Date.parse(issue.evaluatedAt);
  return issue.forecast.map(
    // retain original index for weakmap-backed render clones
    (record, index) => ({ index, record }),
  ).filter((entry) => {
    // require an actual captured forecast product and future target
    const targetAt = Date.parse(entry.record.validAt);
    return entry.record.provenance.sourceKind === "forecast" &&
      eligibleFutureTarget(issue.evaluatedAt, entry.record.validAt) &&
      targetDateForInstant(targetAt, panel) !== null;
  }).map((entry) => ({
    ...entry,
    localDate: targetDateForInstant(Date.parse(entry.record.validAt), panel),
  })).toSorted(
    // preserve visible chronological positions for the pressure context seam
    (left, right) => Date.parse(left.record.validAt) - Date.parse(right.record.validAt),
  );
}

// map one utc target hour into the fixed local-date panel
function targetDateForInstant(instant, panel) {
  for (const schedule of panel.labelSchedule) {
    // score only the target date and never its label halos
    const from = Date.parse(schedule.from) + 3.5 * HOUR_MS;
    const to = Date.parse(schedule.to) - HOUR_MS;
    if (instant >= from && instant < to) {
      return schedule.targetDate;
    }
  }
  return null;
}

// retain only causal feature rows from one assumed retrospective issue
function normalizeRetrospectiveIssue(issue, retrievedAt, site) {
  if (issue === null || typeof issue !== "object" || Array.isArray(issue) ||
      !Array.isArray(issue.current) || !Array.isArray(issue.forecast) ||
      !Array.isArray(issue.pressureContext) || issue.current.length > 128 ||
      issue.forecast.length > 400 ||
      (issue.pressureContext.length !== 0 && issue.pressureContext.length !== 6)) {
    throw new TypeError("retrospective issue is invalid");
  }
  const evaluatedAt = parseInstant(issue.evaluatedAt, "retrospective evaluatedAt");
  // assumed issues cannot postdate the public retrieval
  if (evaluatedAt > retrievedAt) {
    throw new RangeError("retrospective issue postdates retrieval");
  }
  const available = (row) => recordAvailableAt(row, evaluatedAt);
  const current = issue.current.filter(
    // remove features whose latest public receipt was not yet available
    (row) => available(row) && Date.parse(row.validAt) <= evaluatedAt,
  );
  const forecast = issue.forecast.filter(
    // retain only forecast products published by the assumed issue time
    (row) => available(row) && row?.provenance?.sourceKind === "forecast" &&
      typeof row.productRunAt === "string" && Date.parse(row.productRunAt) <= evaluatedAt,
  );
  const pressureContext = issue.pressureContext.every(
    // incomplete or later context fails to the same absent runtime fallback
    (row) => available(row) && row?.provenance?.sourceKind === "forecast" &&
      typeof row.productRunAt === "string" && Date.parse(row.productRunAt) <= evaluatedAt,
  ) ? issue.pressureContext : [];
  return Object.freeze({
    current: Object.freeze(current),
    evaluatedAt: issue.evaluatedAt,
    forecast: Object.freeze(forecast),
    pressureContext: Object.freeze(pressureContext),
    site,
    solarCloudEvidence: retrospectiveSolarEvidence(issue.solarCloudEvidence, evaluatedAt),
  });
}

// test one record against an explicit retrospective availability cutoff
function recordAvailableAt(row, cutoffAt) {
  const receivedAt = Date.parse(row?.receivedAt);
  const validAt = Date.parse(row?.validAt);
  return Number.isFinite(receivedAt) && Number.isFinite(validAt) && receivedAt <= cutoffAt &&
    (row?.provenance?.sourceKind !== "physical_sensor" || receivedAt >= validAt);
}

// retain compact solar evidence only when both inputs were available
function retrospectiveSolarEvidence(value, cutoffAt) {
  // absence is the honest raw fallback
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  const evaluatedAt = Date.parse(value.evaluatedAt);
  const ws90 = Date.parse(value.ws90?.receivedAt);
  const regional = Date.parse(value.regional?.receivedAt);
  return Number.isFinite(evaluatedAt) && Number.isFinite(ws90) && Number.isFinite(regional) &&
    evaluatedAt <= cutoffAt && ws90 <= cutoffAt && regional <= cutoffAt
    ? value
    : null;
}

// derive one local calendar label for a utc target instant
function localDateAt(value, timeZone) {
  const instant = parseInstant(value, "target validAt");
  const parts = Object.fromEntries(new Intl.DateTimeFormat("en-CA", {
    day: "2-digit",
    month: "2-digit",
    timeZone,
    year: "numeric",
  }).formatToParts(new Date(instant)).map(
    // retain only named local calendar fields
    (part) => [part.type, part.value],
  ));
  return `${parts.year}-${parts.month}-${parts.day}`;
}

// validate one retrospective site without accepting capture manifest claims
function validateRetrospectiveSite(site) {
  if (site === null || typeof site !== "object" || Array.isArray(site) ||
      typeof site.siteKey !== "string" || site.siteKey === "" ||
      typeof site.timeZone !== "string" || !Number.isFinite(site.latitude) ||
      !Number.isFinite(site.longitude) || site.latitude < -90 || site.latitude > 90 ||
      site.longitude < -180 || site.longitude > 180) {
    throw new TypeError("retrospective site is invalid");
  }
  try {
    new Intl.DateTimeFormat("en", { timeZone: site.timeZone }).format(0);
  } catch {
    throw new RangeError("retrospective site timezone is invalid");
  }
}

// build metric-specific labels without chart-hour aggregation
function labelsForTarget(rows, metric, targetAt, targetTier, site, runtime) {
  // UV carries all three explicitly separate timing interpretations
  if (metric === "uvIndex") {
    return uvSensitivityLabels(rows, targetAt, targetTier);
  }
  // pressure is an exact endpoint tendency
  if (metric === "pressureChange3hHpa") {
    const nominal = pressureTendencyLabel(rows, targetAt, targetTier);
    return Object.freeze({ following: unsupported("not_applicable"), nominal, preceding: unsupported("not_applicable") });
  }
  // clouds use only the fixed pointwise ws90 radiation proxy
  const nominal = cloudProxyLabel(rows, targetAt, targetTier, site, runtime.radiationCloudProxy);
  return Object.freeze({ following: unsupported("not_applicable"), nominal, preceding: unsupported("not_applicable") });
}

// select target truth independently with fixed farm-first physical precedence
function independentLabelsForTarget(rows, metric, targetAt, site, runtime) {
  // clouds have only the predeclared on-site proxy target
  if (metric === "cloudCoverPercent") {
    const labelSet = labelsForTarget(rows, metric, targetAt, "ws90_proxy", site, runtime);
    return labelSet.nominal.status === "supported"
      ? Object.freeze({ labelSet, targetTier: "ws90_proxy" })
      : null;
  }
  const farm = labelsForTarget(rows, metric, targetAt, "farm", site, runtime);
  // physical farm truth takes precedence independently of the issue anchor
  if (farm.nominal.status === "supported") {
    return Object.freeze({ labelSet: farm, targetTier: "farm" });
  }
  const local = labelsForTarget(rows, metric, targetAt, "local_mean", site, runtime);
  return local.nominal.status === "supported"
    ? Object.freeze({ labelSet: local, targetTier: "local_mean" })
    : null;
}

// evaluate every nonzero observation-age bucket independently
function evaluateAffectedBuckets(rows) {
  const definitions = [
    ["(0,3]", 0, 3],
    ["(3,6]", 3, 6],
    ["(6,12]", 6, 12],
    ["(12,24]", 12, 24],
  ];
  return Object.freeze(definitions.map(([name, lower, upper]) => {
    // isolate only rows numerically changed in this age interval
    const bucketRows = rows.filter((row) => row.observationAgeHours > lower &&
      row.observationAgeHours <= upper && row.raw !== row.adjusted);
    const score = scorePairedRows(bucketRows, "nominal");
    const active = bucketRows.length > 0;
    const supported = score.support.dates >= 8 && score.support.targetHours >= 20 &&
      score.absoluteGain !== null;
    const status = !active ? "unchanged" : !supported ? "unsupported" :
      score.absoluteGain >= 0 ? "pass" : "regional";
    return Object.freeze({ ageHours: name, score, status });
  }));
}

// require both UV timing sensitivities to remain supported and nonharmful
function evaluateUvSensitivity(rows) {
  const panels = {};
  let passing = true;
  for (const labelKey of ["preceding", "following"]) {
    // score each declared alignment without selecting the favorable one
    const score = scorePairedRows(rows, labelKey);
    const daylight = scorePairedRows(rows.filter((row) => row.daylight), labelKey);
    const affected = scorePairedRows(rows.filter((row) => row.raw !== row.adjusted), labelKey);
    const supported = score.support.dates >= 10 && score.support.targetHours >= 60 &&
      daylight.support.dates >= 10 && daylight.support.targetHours >= 60 &&
      affected.support.dates >= 8 && affected.support.targetHours >= 20;
    const nonharmful = score.absoluteGain !== null && score.absoluteGain >= 0 &&
      daylight.absoluteGain !== null && daylight.absoluteGain >= 0 &&
      affected.absoluteGain !== null && affected.absoluteGain >= 0;
    // fail closed on incomplete or harmful timing interpretations
    if (!supported || !nonharmful) {
      passing = false;
    }
    panels[labelKey] = Object.freeze({ affected, daylight, score, supported });
  }
  const supported = Object.values(panels).every((panel) => panel.supported);
  const nonharmful = supported && passing;
  return Object.freeze({
    ...panels,
    nonharmful,
    status: supported && nonharmful ? "pass" : supported ? "regional" : "unsupported",
    supported,
  });
}

// hash exact target masks and labels without predictions
function pairedMaskHash(rows) {
  return sha256Hex(canonicalJsonBytes(rows.map((row) => ({
    daylight: row.daylight,
    issueAnchorTier: row.issueAnchorTier,
    issueLeadHours: row.issueLeadHours,
    labels: row.labels,
    localDate: row.localDate,
    observationAgeHours: row.observationAgeHours,
    targetAt: row.targetAt,
    targetLabelTier: row.targetLabelTier,
    vintageId: row.vintageId,
  }))));
}

// build an outcome-free pending report before any label member is inspected
export function buildPendingLocalForecastCorrectionReport(panel, issueMemberCount, labelMemberCount) {
  validateLocalForecastCorrectionPanel(panel);
  // retain counts only inside the frozen schedule ceilings
  if (!Number.isInteger(issueMemberCount) || issueMemberCount < 0 ||
      issueMemberCount > panel.issueSchedule.length || !Number.isInteger(labelMemberCount) ||
      labelMemberCount < 0 || labelMemberCount > panel.targetDateCount) {
    throw new RangeError("pending evidence counts are invalid");
  }
  return Object.freeze({
    cells: Object.freeze([]),
    claims: panel.claims,
    contractVersion: LOCAL_FORECAST_CORRECTION_EVALUATION_VERSION,
    coverage: Object.freeze({
      expectedIssues: panel.issueSchedule.length,
      expectedLabelDates: panel.targetDateCount,
      issueMemberCount,
      labelMemberCount,
    }),
    evidenceInputHash: null,
    frozenIdentity: Object.freeze({
      candidateFrozenAt: panel.candidateFrozenAt,
      codeHash: panel.codeHash,
      developmentLatestAt: panel.developmentLatestAt,
      earliestValidationInputAt: panel.earliestValidationInputAt,
      inputHash: panel.inputHash,
      panelDigest: panel.panelDigest,
      policyHash: panel.policyHash,
      targetEndAt: panel.targetEndAt,
      targetStartAt: panel.targetStartAt,
      warmupStartAt: panel.warmupStartAt,
    }),
    multiplicity: 5,
    phase: "validation",
    positiveValidationPending: true,
    servingPolicyState: "zero",
    state: "regional_pending_or_unsupported",
  });
}

// average errors through date hour and vintage hierarchy
function hierarchicalMean(rows, value) {
  const byDate = groupBy(rows, (row) => row.localDate);
  const dateMeans = [];
  for (const dateRows of byDate.values()) {
    // give every utc target hour one equal vote within its date
    const byHour = groupBy(dateRows, (row) => row.targetAt);
    const hourMeans = [];
    for (const hourRows of byHour.values()) {
      // give every distinct retrieval vintage one equal vote within its hour
      hourMeans.push(mean(hourRows.map(value)));
    }
    dateMeans.push(mean(hourMeans));
  }
  return mean(dateMeans);
}

// deduplicate repeated captures of one product vintage deterministically
function deduplicatePairs(rows) {
  const selected = new Map();
  for (const row of rows) {
    // use target date hour and retrieval identity as the paired key
    const key = `${row.localDate}/${row.targetAt}/${row.vintageId}`;
    const current = selected.get(key);
    // keep the earliest actual receipt of a repeated unchanged product vintage
    if (current === undefined || Date.parse(row.evaluationAt) < Date.parse(current.evaluationAt)) {
      selected.set(key, row);
    }
  }
  return [...selected.values()].toSorted(
    // stabilize downstream hashes and reports
    (left, right) => left.localDate.localeCompare(right.localDate) ||
      left.targetAt.localeCompare(right.targetAt) || left.vintageId.localeCompare(right.vintageId),
  );
}

// resolve newest revisions into independent source-local sample series
function resolveSourceSeries(rows, metric, transform = (_row, value) => value) {
  const bounds = METRIC_BOUNDS[metric];
  // reject evaluator-internal unknown metrics
  if (bounds === undefined) {
    throw new TypeError("label metric is invalid");
  }
  const revisions = new Map();
  for (const row of rows) {
    // physical readings are the only independent targets
    if (row?.provenance?.sourceKind !== "physical_sensor") {
      continue;
    }
    const value = row.metrics?.[metric];
    // reject missing and physically invalid scalar readings
    if (!Number.isFinite(value) || value < bounds.minimum || value > bounds.maximum) {
      continue;
    }
    const validAt = Date.parse(row.validAt);
    const receivedAt = Date.parse(row.receivedAt);
    // reject malformed revision timing
    if (!Number.isFinite(validAt) || !Number.isFinite(receivedAt) || receivedAt < validAt) {
      continue;
    }
    const sourceId = row.provenance.sourceId;
    const key = `${sourceId}/${row.validAt}`;
    const existing = revisions.get(key) ?? [];
    existing.push({ receivedAt, row, value });
    revisions.set(key, existing);
  }
  const sources = new Map();
  for (const revisionRows of revisions.values()) {
    // resolve newest reception then stable record identity
    revisionRows.sort((left, right) => right.receivedAt - left.receivedAt ||
      right.row.id.localeCompare(left.row.id));
    const selected = revisionRows[0];
    const contradictions = revisionRows.filter(
      // identify impossible duplicate identity/time values
      (candidate) => candidate.receivedAt === selected.receivedAt &&
        candidate.row.id === selected.row.id && candidate.value !== selected.value,
    );
    // quarantine the dependent label rather than selecting favorable bytes
    if (contradictions.length > 0) {
      return { reason: "conflicting_revision", status: "unsupported" };
    }
    const transformed = transform(selected.row, selected.value);
    // nonlinear proxy gates may reject one physical sample
    if (!Number.isFinite(transformed)) {
      continue;
    }
    const sourceId = selected.row.provenance.sourceId;
    const source = sources.get(sourceId) ?? {
      providerKey: selected.row.provenance.providerKey,
      samples: [],
      sourceId,
      stationSlug: selected.row.provenance.stationSlug,
    };
    source.samples.push({ at: Date.parse(selected.row.validAt), value: transformed });
    sources.set(sourceId, source);
  }
  for (const source of sources.values()) {
    // make input order irrelevant
    source.samples.sort((left, right) => left.at - right.at);
  }
  return { sources, status: "supported" };
}

// interpolate one exact or tightly bracketed source-local point
function interpolateSamples(samples, target) {
  const exact = samples.find(
    // exact physical points always outrank interpolation
    (sample) => sample.at === target,
  );
  // preserve exact measured zero values
  if (exact !== undefined) {
    return exact.value;
  }
  const before = samples.filter(
    // retain only earlier candidates
    (sample) => sample.at < target,
  ).at(-1);
  const after = samples.find(
    // retain the earliest later candidate
    (sample) => sample.at > target,
  );
  // require both endpoints within fifteen minutes
  if (before === undefined || after === undefined ||
      target - before.at > POINT_RADIUS_MS || after.at - target > POINT_RADIUS_MS) {
    return null;
  }
  const weight = (target - before.at) / (after.at - before.at);
  return before.value + (after.value - before.value) * weight;
}

// integrate one complete source-local hour with trapezoidal weights
function sourceWindowMean(samples, from, to) {
  const first = interpolateSamples(samples, from);
  const last = interpolateSamples(samples, to);
  // require exact or bounded endpoints plus complete interior coverage
  if (first === null || last === null || !completeCoverage(samples, from, to)) {
    return null;
  }
  const points = [
    { at: from, value: first },
    ...samples.filter(
      // retain strict interior points once
      (sample) => sample.at > from && sample.at < to,
    ),
    { at: to, value: last },
  ];
  let area = 0;
  for (let index = 1; index < points.length; index += 1) {
    // integrate the linear segment between adjacent physical points
    const left = points[index - 1];
    const right = points[index];
    area += (right.at - left.at) * (left.value + right.value) / 2;
  }
  return area / (to - from);
}

// require samples across a span without bridging a physical outage
function completeCoverage(samples, from, to) {
  const relevant = samples.filter(
    // include only the endpoint interpolation halos
    (sample) => sample.at >= from - POINT_RADIUS_MS && sample.at <= to + POINT_RADIUS_MS,
  );
  // require bracketing support at both ends
  if (relevant.length === 0 || relevant[0].at > from || relevant.at(-1).at < to) {
    return false;
  }
  for (let index = 1; index < relevant.length; index += 1) {
    // reject every physical gap over thirty minutes
    if (relevant[index].at - relevant[index - 1].at > MAX_PHYSICAL_GAP_MS) {
      return false;
    }
  }
  return true;
}

// aggregate source-local estimates into one declared station tier
function aggregateTier(values, targetTier) {
  const admissible = values.filter((value) => {
    // farm and cloud labels remain tied to the configured ws90 gateway
    if (targetTier === "farm" || targetTier === "ws90_proxy") {
      return value.providerKey === "ecowitt-local" && value.stationSlug === FARM_STATION;
    }
    // local means categorically exclude the farm station
    if (targetTier === "local_mean") {
      return value.stationSlug !== FARM_STATION;
    }
    return false;
  });
  // missing matching tiers never fall back to regional truth
  if (admissible.length === 0) {
    return unsupported("missing_matching_tier");
  }
  const byStation = groupBy(admissible, (value) => value.stationSlug);
  const stationValues = [...byStation.values()].map(
    // give each physical station one equal vote despite multiple source ids
    (station) => mean(station.map((value) => value.value)),
  );
  return Object.freeze({
    sourceCount: admissible.length,
    stationCount: stationValues.length,
    status: "supported",
    tier: targetTier,
    value: mean(stationValues),
  });
}

// create one immutable unsupported label result
function unsupported(reason) {
  return Object.freeze({ reason, status: "unsupported", value: null });
}

// group values by one deterministic key
function groupBy(values, key) {
  const groups = new Map();
  for (const value of values) {
    // preserve encounter order within each group
    const identity = key(value);
    const group = groups.get(identity) ?? [];
    group.push(value);
    groups.set(identity, group);
  }
  return groups;
}

// average a nonempty finite collection
function mean(values) {
  // reject empty internal reductions
  if (values.length === 0) {
    throw new RangeError("mean requires values");
  }
  return values.reduce(
    // divide while summing to reduce overflow risk
    (total, value) => total + value / values.length,
    0,
  );
}

// create one stable seeded pseudo-random stream
function seededRandom(seed) {
  // normalize any integer seed to uint32
  if (!Number.isInteger(seed)) {
    throw new TypeError("bootstrap seed is invalid");
  }
  let state = seed >>> 0;
  return () => {
    // use mulberry32 for deterministic local resampling
    state += 0x6d2b79f5;
    let value = state;
    value = Math.imul(value ^ value >>> 15, value | 1);
    value ^= value + Math.imul(value ^ value >>> 7, value | 61);
    return ((value ^ value >>> 14) >>> 0) / 4_294_967_296;
  };
}

// encode one policy identity
function policyKey(policy) {
  return policy === null || typeof policy !== "object"
    ? "invalid"
    : `${policy.strength}/${policy.horizonHours}`;
}

// parse one canonical evidence instant
function parseInstant(value, name) {
  const parsed = typeof value === "string" ? Date.parse(value) : Number.NaN;
  // reject malformed and noncanonical timestamps
  if (!Number.isFinite(parsed) || new Date(parsed).toISOString() !== value) {
    throw new TypeError(`${name} is invalid`);
  }
  return parsed;
}

// validate and deduplicate issue members against the frozen schedule
function validateIssuesAgainstPanel(issues, panel) {
  // enforce the bounded finite issue population
  if (!Array.isArray(issues) || issues.length > panel.issueSchedule.length) {
    throw new RangeError("issue evidence count is invalid");
  }
  const schedule = new Set(panel.issueSchedule);
  const selected = new Map();
  for (const issue of issues) {
    // validate immutable member bytes before panel binding
    validateLocalForecastIssueSnapshot(issue);
    validateIssuePhysicalSources(issue, panel.sourceCatalog);
    if (issue.panelDigest !== panel.panelDigest || !schedule.has(issue.scheduledAt) ||
        canonicalSite(issue.site) !== canonicalSite(panel.site)) {
      throw new TypeError("issue differs from panel");
    }
    const existing = selected.get(issue.scheduledAt);
    // identical publication is idempotent but conflicting bytes fail closed
    if (existing !== undefined && existing.normalizedDigest !== issue.normalizedDigest) {
      throw new TypeError("issue snapshot conflicts");
    }
    selected.set(issue.scheduledAt, issue);
  }
  return [...selected.values()].toSorted(
    // preserve declared capture order
    (left, right) => left.scheduledAt.localeCompare(right.scheduledAt),
  );
}

// validate and deduplicate label members against one-shot schedules
function validateLabelsAgainstPanel(labelSeals, panel) {
  // enforce at most one terminal attempt per target date
  if (!Array.isArray(labelSeals) || labelSeals.length > panel.targetDateCount) {
    throw new RangeError("label evidence count is invalid");
  }
  const schedules = new Map(panel.labelSchedule.map(
    // index exact declared one-shot timing
    (schedule) => [schedule.targetDate, schedule],
  ));
  const selected = new Map();
  for (const labels of labelSeals) {
    // validate immutable member bytes before panel binding
    validateLocalForecastLabelSeal(labels);
    validateLabelPhysicalSources(labels, panel.sourceCatalog);
    const expected = schedules.get(labels.targetDate);
    if (labels.panelDigest !== panel.panelDigest || expected === undefined ||
        labels.from !== expected.from || labels.to !== expected.to ||
        labels.sealAt !== expected.sealAt ||
        JSON.stringify(labels.sourceAllowlist) !== JSON.stringify(panel.sourceAllowlist)) {
      throw new TypeError("label seal differs from panel");
    }
    const existing = selected.get(labels.targetDate);
    // identical publication is idempotent but conflicting bytes fail closed
    if (existing !== undefined && existing.normalizedDigest !== labels.normalizedDigest) {
      throw new TypeError("label seal conflicts");
    }
    selected.set(labels.targetDate, labels);
  }
  return [...selected.values()].toSorted(
    // preserve calendar order including unsupported attempts
    (left, right) => left.targetDate.localeCompare(right.targetDate),
  );
}

// bind physical target rows to complete frozen source tuples
export function validateLabelPhysicalSources(labels, sourceCatalog) {
  for (const row of labels.rows) {
    // every retained label row must preserve the discovered physical identity
    validateLocalForecastPhysicalSource(
      row.provenance,
      sourceCatalog,
      "label physical source",
    );
  }
}

// bind current anchors and compact ws90 evidence to complete frozen source tuples
export function validateIssuePhysicalSources(issue, sourceCatalog) {
  for (const row of issue.current) {
    // regional current products remain outside the physical source allowlist
    if (row.provenance.sourceKind === "physical_sensor") {
      validateLocalForecastPhysicalSource(
        row.provenance,
        sourceCatalog,
        "current physical source",
      );
    }
  }
  // absent solar evidence remains the explicit raw fallback
  if (issue.solarCloudEvidence === null) {
    return;
  }
  const { regional, ws90 } = issue.solarCloudEvidence;
  // only the ws90 sample is physical while regional model current stays public and unlisted
  if (ws90?.sourceKind !== "physical_sensor" || regional?.sourceKind !== "model_current") {
    throw new TypeError("solar cloud evidence source kinds are invalid");
  }
  validateLocalForecastWs90Source(ws90, sourceCatalog);
}

// compare compact normalized site identities
function canonicalSite(site) {
  return JSON.stringify([site.siteKey, site.timeZone, site.latitude, site.longitude]);
}

// load only the actual compiled browser inference functions
async function loadCompiledRuntime() {
  const dashboard = await import("../../apps/web/dist/index.js");
  const solar = await import("../../apps/web/dist/solar-cloud.js");
  const runtime = {
    clearSkyRadiation: solar.clearSkyRadiation,
    forecastCorrectionAnchor: dashboard.forecastCorrectionAnchor,
    forecastCorrectionAnchorTier: dashboard.forecastCorrectionAnchorTier,
    forecastMetricValue: dashboard.forecastMetricValue,
    forecastPressureSeries: dashboard.forecastPressureSeries,
    radiationCloudProxy: solar.radiationCloudProxy,
    withLocalForecastAdjustments: dashboard.withLocalForecastAdjustments,
    withSolarCloudAdjustment: dashboard.withSolarCloudAdjustment,
  };
  for (const [name, value] of Object.entries(runtime)) {
    // reject stale builds instead of silently using a research approximation
    if (typeof value !== "function") {
      throw new TypeError(`compiled browser export is missing: ${name}`);
    }
  }
  return Object.freeze(runtime);
}

// enumerate the complete compiled browser javascript closure conservatively
export async function compiledBrowserModulePaths(root) {
  const absoluteRoot = resolve(root);
  const rootMetadata = await lstat(absoluteRoot);
  const canonicalRoot = await realpath(absoluteRoot);
  // hashing and later imports cannot traverse a linked special or noncanonical root
  if (rootMetadata.isSymbolicLink() || !rootMetadata.isDirectory() ||
      canonicalRoot !== absoluteRoot) {
    throw new RangeError("compiled browser root is not one canonical directory");
  }
  const paths = [];
  const entries = await readdir(absoluteRoot);
  for (const entry of entries) {
    // reject links and special members instead of silently omitting importable paths
    const absolute = join(absoluteRoot, entry);
    const metadata = await lstat(absolute);
    if (metadata.isSymbolicLink()) {
      throw new RangeError("compiled browser closure contains a symbolic link");
    }
    if (metadata.isDirectory()) {
      paths.push(...await compiledBrowserModulePaths(absolute));
    } else if (metadata.isFile() && (entry.endsWith(".js") || entry.endsWith(".mjs"))) {
      paths.push(relative(repositoryRoot, absolute));
    } else if (!metadata.isFile()) {
      throw new RangeError("compiled browser closure contains a special member");
    }
  }
  return paths;
}

// read one bounded json file
export async function readJsonFile(path, maximumBytes, name) {
  const absolute = resolve(path);
  await verifyPrivateEvidenceDirectory(dirname(absolute), false);
  const metadata = await lstat(absolute);
  // reject links special files and oversized members before reading bytes
  if (metadata.isSymbolicLink() || !metadata.isFile() || metadata.size === 0 ||
      metadata.size > maximumBytes) {
    throw new RangeError(`${name} byte limit exceeded`);
  }
  const bytes = await readFile(absolute);
  // reject a member changed after the pre-read size check
  if (bytes.length !== metadata.size || bytes.length > maximumBytes) {
    throw new RangeError(`${name} changed during read`);
  }
  let value;
  try {
    value = JSON.parse(bytes.toString("utf8"));
  } catch {
    throw new TypeError(`${name} is not valid json`);
  }
  return { bytes: bytes.length, value };
}

// read one finite directory of immutable json members
async function readEvidenceDirectory(path, maximumFiles, maximumFileBytes, validator, name) {
  const absolute = await verifyPrivateEvidenceDirectory(path, false);
  const entries = (await readdir(absolute, { withFileTypes: true })).toSorted(
    // retain stable evidence traversal after closed-member validation
    (left, right) => left.name.localeCompare(right.name),
  );
  for (const entry of entries) {
    // outcome directories accept only immediate regular json members
    if (!entry.isFile() || !entry.name.endsWith(".json")) {
      throw new RangeError(`${name} directory contains a foreign member`);
    }
  }
  // reject an unbounded directory before opening members
  if (entries.length > maximumFiles) {
    throw new RangeError(`${name} file limit exceeded`);
  }
  const values = [];
  let bytes = 0;
  for (const entry of entries) {
    // read only immediate regular json members
    const member = await readJsonFile(join(absolute, entry.name), maximumFileBytes, name);
    bytes += member.bytes;
    // reject the total normalized evidence ceiling before scoring
    if (bytes > MAX_EVIDENCE_BYTES) {
      throw new RangeError("normalized evidence byte limit exceeded");
    }
    validator(member.value);
    values.push(member.value);
  }
  return { bytes, values };
}

// count immediate regular json evidence members without opening outcomes
export async function countEvidenceFiles(path, maximumFiles, name) {
  const absolute = await verifyPrivateEvidenceDirectory(path, false);
  const entries = await readdir(absolute, { withFileTypes: true });
  for (const entry of entries) {
    // pending coverage cannot hide foreign directories links or non-json members
    if (!entry.isFile() || !entry.name.endsWith(".json")) {
      throw new RangeError(`${name} directory contains a foreign member`);
    }
  }
  // reject an unbounded directory before pending-state reporting
  if (entries.length > maximumFiles) {
    throw new RangeError(`${name} file limit exceeded`);
  }
  return entries.length;
}

// verify or create one canonical private evidence directory chain
async function verifyPrivateEvidenceDirectory(path, create) {
  const absolute = resolve(path);
  const location = relative(privateEvidenceRoot, absolute);
  // evaluator inputs and outputs remain beneath the ignored private root
  if (location === ".." || location.startsWith(`..${sep}`)) {
    throw new RangeError("evaluator path escaped .weather-data");
  }
  let cursor = privateEvidenceRoot;
  const segments = location === "" ? [] : location.split(sep);
  for (const segment of ["", ...segments]) {
    // create only output ancestors while preserving exact canonical paths
    if (segment !== "") {
      cursor = join(cursor, segment);
    }
    if (create) {
      await mkdir(cursor, { mode: 0o700, recursive: true });
    }
    const metadata = await lstat(cursor);
    const canonical = await realpath(cursor);
    if (metadata.isSymbolicLink() || !metadata.isDirectory() || canonical !== resolve(cursor)) {
      throw new RangeError("evaluator path contains a symbolic link or non-directory");
    }
  }
  return absolute;
}

// parse the closed evaluator command line
function parseArguments(argumentsList) {
  const options = { phase: "validation" };
  for (let index = 0; index < argumentsList.length; index += 2) {
    // require one value for every named option
    const key = argumentsList[index];
    const value = argumentsList[index + 1];
    if (value === undefined) {
      throw new TypeError(`missing value for ${key}`);
    }
    if (key === "--panel") {
      options.panel = value;
    } else if (key === "--issues") {
      options.issues = value;
    } else if (key === "--labels") {
      options.labels = value;
    } else if (key === "--output") {
      options.output = value;
    } else if (key === "--clock") {
      options.clockAt = value;
    } else if (key === "--phase") {
      options.phase = value;
    } else {
      throw new TypeError(`unknown argument: ${key}`);
    }
  }
  for (const required of ["panel", "issues", "labels", "output", "clockAt"]) {
    // reject implicit dates paths and durations
    if (typeof options[required] !== "string" || options[required] === "") {
      throw new TypeError(`missing --${required === "clockAt" ? "clock" : required}`);
    }
  }
  return options;
}

// parse the separate nonqualifying retrospective development command
function parseRetrospectiveArguments(argumentsList) {
  const options = {};
  for (let index = 0; index < argumentsList.length; index += 2) {
    // require one value for each closed retrospective option
    const key = argumentsList[index];
    const value = argumentsList[index + 1];
    if ((key !== "--input" && key !== "--output") || value === undefined) {
      throw new TypeError("retrospective development argument is invalid");
    }
    options[key.slice(2)] = value;
  }
  // both private input and aggregate output are mandatory
  if (typeof options.input !== "string" || options.input === "" ||
      typeof options.output !== "string" || options.output === "") {
    throw new TypeError("retrospective development input and output are required");
  }
  return options;
}

// write one aggregate-only report into ignored private evidence
export async function writePrivateReport(path, report) {
  const absolute = resolve(path);
  const location = relative(privateEvidenceRoot, absolute);
  // prohibit repository or path-escape publication of private evidence
  if (location === "" || location === ".." || location.startsWith(`..${sep}`) ||
      absolute === privateEvidenceRoot) {
    throw new RangeError("output must be inside .weather-data");
  }
  await verifyPrivateEvidenceDirectory(dirname(absolute), true);
  const handle = await open(absolute, "wx", 0o600);
  try {
    // publish only deterministic aggregate report bytes
    await handle.writeFile(canonicalJsonBytes(report));
    await handle.sync();
  } finally {
    await handle.close();
  }
}

// run the bounded standalone evaluator
async function main() {
  // keep retrospective opened-history development separate from prospective validation
  if (process.argv[2] === "retrospective-development") {
    const options = parseRetrospectiveArguments(process.argv.slice(3));
    const member = await readJsonFile(options.input, MAX_EVIDENCE_BYTES, "retrospective development input");
    const input = member.value;
    if (input?.contractVersion !== LOCAL_FORECAST_CORRECTION_RETROSPECTIVE_VERSION) {
      throw new TypeError("retrospective development contract version is invalid");
    }
    const report = await evaluateRetrospectiveDevelopment({
      historyRows: input.historyRows,
      issues: input.issues,
      retrievedAt: input.retrievedAt,
      site: input.site,
    });
    await writePrivateReport(options.output, report);
    process.stdout.write(`${JSON.stringify({ output: options.output, state: report.state })}\n`);
    return;
  }
  const options = parseArguments(process.argv.slice(2));
  // reject the removed prospective development alias before opening outcome files
  if (options.phase !== "validation") {
    throw new TypeError("prospective evaluation phase must be validation");
  }
  const panelMember = await readJsonFile(options.panel, MAX_ISSUE_BYTES, "panel");
  validateLocalForecastCorrectionPanel(panelMember.value);
  const issueFileCount = await countEvidenceFiles(
    options.issues,
    panelMember.value.issueSchedule.length,
    "issue",
  );
  const labelFileCount = await countEvidenceFiles(
    options.labels,
    panelMember.value.targetDateCount,
    "labels",
  );
  const finalSealDeadline = Math.max(...panelMember.value.labelSchedule.map(
    // preserve the complete five-minute terminal attempt
    (seal) => Date.parse(seal.sealAt) + 5 * MINUTE_MS,
  ));
  // report pending coverage without opening any forecast or label outcomes
  if (labelFileCount < panelMember.value.targetDateCount ||
      parseInstant(options.clockAt, "clockAt") < finalSealDeadline) {
    const report = await evaluateLocalForecastCorrections({
      clockAt: options.clockAt,
      issues: Array(issueFileCount).fill(null),
      labelSeals: Array(labelFileCount).fill(null),
      panel: panelMember.value,
      phase: options.phase,
    });
    await writePrivateReport(options.output, report);
    process.stdout.write(`${JSON.stringify({ output: options.output, state: report.state })}\n`);
    return;
  }
  const issues = await readEvidenceDirectory(
    options.issues,
    panelMember.value.issueSchedule.length,
    MAX_ISSUE_BYTES,
    validateLocalForecastIssueSnapshot,
    "issue",
  );
  const labels = await readEvidenceDirectory(
    options.labels,
    panelMember.value.targetDateCount,
    MAX_LABEL_BYTES,
    validateLocalForecastLabelSeal,
    "labels",
  );
  // enforce the shared normalized evidence budget across both directories
  if (issues.bytes + labels.bytes > MAX_EVIDENCE_BYTES) {
    throw new RangeError("normalized evidence byte limit exceeded");
  }
  const report = await evaluateLocalForecastCorrections({
    clockAt: options.clockAt,
    issues: issues.values,
    labelSeals: labels.values,
    panel: panelMember.value,
    phase: options.phase,
  });
  await writePrivateReport(options.output, report);
  process.stdout.write(`${JSON.stringify({ output: options.output, state: report.state })}\n`);
}

const invokedPath = process.argv[1] === undefined ? null : pathToFileURL(resolve(process.argv[1])).href;
// execute only when invoked as the standalone cli
if (invokedPath === import.meta.url) {
  main().catch((error) => {
    // emit one bounded error without raw evidence bodies
    process.stderr.write(`${error instanceof Error ? error.message : "evaluation failed"}\n`);
    process.exitCode = 1;
  });
}
