import { createHash } from "node:crypto";

export const LOCAL_FORECAST_CORRECTION_PANEL_VERSION = "weather-local-correction-panel/v1";
export const LOCAL_FORECAST_CORRECTION_ISSUE_VERSION = "weather-local-correction-issue/v1";
export const LOCAL_FORECAST_CORRECTION_LABELS_VERSION = "weather-local-correction-labels/v1";
export const LOCAL_FORECAST_CORRECTION_MULTIPLICITY = 5;
export const LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS = Object.freeze([
  Object.freeze({ cellId: "uvIndex/farm/farm", issueAnchorTier: "farm", metric: "uvIndex", targetLabelTier: "farm" }),
  Object.freeze({ cellId: "uvIndex/local_mean/local_mean", issueAnchorTier: "local_mean", metric: "uvIndex", targetLabelTier: "local_mean" }),
  Object.freeze({ cellId: "pressureChange3hHpa/farm/farm", issueAnchorTier: "farm", metric: "pressureChange3hHpa", targetLabelTier: "farm" }),
  Object.freeze({ cellId: "pressureChange3hHpa/local_mean/local_mean", issueAnchorTier: "local_mean", metric: "pressureChange3hHpa", targetLabelTier: "local_mean" }),
  Object.freeze({ cellId: "cloudCoverPercent/ws90/ws90_proxy", issueAnchorTier: "ws90", metric: "cloudCoverPercent", targetLabelTier: "ws90_proxy" }),
]);
export const LOCAL_FORECAST_CORRECTION_CANDIDATE_GRID = Object.freeze([
  Object.freeze({ horizonHours: 3, strength: 0 }),
  ...[3, 6, 12, 24].flatMap(
    // enumerate the predeclared positive grid in deterministic order
    (horizonHours) => [0.25, 0.5, 0.75, 1].map(
      // freeze every candidate independently
      (strength) => Object.freeze({ horizonHours, strength }),
    ),
  ),
]);

const DAY_MS = 24 * 3_600_000;
const HASH = /^[a-f0-9]{64}$/u;
const INSTANT = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;
const LOCAL_DATE = /^(\d{4})-(\d{2})-(\d{2})$/u;
const ISSUE_RECEIPT_KINDS = ["current", "forecast", "solarCloudEvidence"];
const POLICY_HORIZONS = new Set([3, 6, 12, 24]);
const POLICY_STRENGTHS = new Set([0, 0.25, 0.5, 0.75, 1]);

// encode stable json bytes for hashes and immutable evidence
export function canonicalJsonBytes(value) {
  return Buffer.from(`${canonicalJson(value)}\n`);
}

// hash stable evidence bytes
export function sha256Hex(value) {
  return createHash("sha256").update(value).digest("hex");
}

// hash one issue without its self-referential digest
export function issueSnapshotDigest(issue) {
  return sha256Hex(canonicalJsonBytes(withoutField(issue, "normalizedDigest")));
}

// hash one label seal without its self-referential digest
export function labelSealDigest(labels) {
  return sha256Hex(canonicalJsonBytes(withoutField(labels, "normalizedDigest")));
}

// hash every frozen panel field without its self-referential digest
export function panelManifestDigest(panel) {
  return sha256Hex(canonicalJsonBytes(withoutField(panel, "panelDigest")));
}

// hash the exact five frozen policy choices
export function localForecastCorrectionPolicyHash(cells) {
  validatePanelCells(cells);
  return sha256Hex(canonicalJsonBytes(cells));
}

// validate the exact discovered physical source tuples frozen into one panel
export function validateLocalForecastSourceCatalog(catalog) {
  requireArray(catalog, "sourceCatalog", 1, 128);
  const identities = [];
  for (const source of catalog) {
    // retain every public source identity component used by inference
    requireExactKeys(
      source,
      ["providerKey", "sourceId", "sourceKey", "stationSlug"],
      "source catalog entry",
    );
    for (const [key, value] of Object.entries(source)) {
      // catalog tuple fields remain nonblank public identifiers
      requireNonemptyString(value, `source catalog ${key}`);
    }
    identities.push(source.sourceId);
  }
  const canonical = [...new Set(identities)].toSorted();
  // source tuples are unique and ordered by their public source id
  if (canonical.length !== catalog.length || canonical.some((sourceId, index) =>
    sourceId !== catalog[index].sourceId)) {
    throw new TypeError("source catalog is not canonical");
  }
  return catalog;
}

// require one physical record or compact sample to match its frozen source tuple
export function validateLocalForecastPhysicalSource(value, catalog, name = "physical source") {
  validateLocalForecastSourceCatalog(catalog);
  requireObject(value, name);
  const expected = catalog.find(
    // resolve only by the frozen public source id before comparing the full tuple
    (source) => source.sourceId === value.sourceId,
  );
  // substituted keys providers or stations cannot inherit a frozen source id
  if (expected === undefined || expected.sourceKey !== value.sourceKey ||
      expected.providerKey !== value.providerKey || expected.stationSlug !== value.stationSlug) {
    throw new RangeError(`${name} differs from the frozen source catalog`);
  }
  return value;
}

// require the designated farm live source for compact ws90 evidence
export function validateLocalForecastWs90Source(value, catalog) {
  validateLocalForecastPhysicalSource(value, catalog, "ws90 physical source");
  const designated = catalog.filter(
    // resolve the unique discovered id for the canonical farm live tuple
    (source) => source.providerKey === "ecowitt-local" &&
      source.stationSlug === "ballydidean-ecowitt" &&
      source.sourceKey === "ecowitt-88f15505d89f-local-live-v1",
  );
  // another valid catalog member cannot substitute for the designated ws90 gateway
  if (designated.length !== 1 || value.sourceId !== designated[0].sourceId) {
    throw new RangeError("ws90 physical source is not the canonical farm live source");
  }
  return value;
}

// derive one real local calendar day in utc
export function localDateBounds(localDate, timeZone) {
  const [year, month, day] = parseLocalDate(localDate);
  requireTimeZone(timeZone);
  const start = localMidnightUtc(year, month, day, timeZone);
  const next = new Date(Date.UTC(year, month - 1, day + 1));
  const end = localMidnightUtc(
    next.getUTCFullYear(),
    next.getUTCMonth() + 1,
    next.getUTCDate(),
    timeZone,
  );
  return Object.freeze({ endAt: new Date(end).toISOString(), startAt: new Date(start).toISOString() });
}

// create the immutable pre-outcome panel contract
export function createLocalForecastCorrectionPanel(input) {
  requireExactKeys(input, [
    "candidateFrozenAt", "captureMinute", "cells", "claims", "codeHash", "createdAt",
    "developmentLatestAt", "earliestValidationInputAt", "inputHash", "policyHash",
    "servingPolicyState", "site", "sourceAllowlist", "sourceCatalog", "sourceCatalogReceipt",
    "targetDateCount", "targetStartLocalDate",
  ], "panel input");
  requireInteger(input.targetDateCount, "targetDateCount", 14, 14);
  requireInteger(input.captureMinute, "captureMinute", 0, 59);
  const site = validateSite(input.site);
  const targetDates = consecutiveLocalDates(input.targetStartLocalDate, input.targetDateCount);
  const first = localDateBounds(targetDates[0], site.timeZone);
  const last = localDateBounds(targetDates.at(-1), site.timeZone);
  const warmupStartAt = new Date(Date.parse(first.startAt) - DAY_MS).toISOString();
  const issueSchedule = hourlyIssueSchedule(warmupStartAt, last.endAt, input.captureMinute);
  const labelSchedule = targetDates.map(
    // freeze each one-shot label window before outcomes exist
    (targetDate) => createLabelSchedule(targetDate, site.timeZone),
  );
  const unsigned = {
    ...input,
    candidateGridHash: sha256Hex(canonicalJsonBytes(LOCAL_FORECAST_CORRECTION_CANDIDATE_GRID)),
    contractVersion: LOCAL_FORECAST_CORRECTION_PANEL_VERSION,
    issueSchedule,
    labelSchedule,
    multiplicity: LOCAL_FORECAST_CORRECTION_MULTIPLICITY,
    targetDates,
    targetEndAt: last.endAt,
    targetStartAt: first.startAt,
    warmupStartAt,
  };
  const panel = { ...unsigned, panelDigest: panelManifestDigest(unsigned) };
  validateLocalForecastCorrectionPanel(panel);
  return deepFreeze(panel);
}

// validate one exact frozen panel manifest
export function validateLocalForecastCorrectionPanel(panel) {
  requireExactKeys(panel, [
    "candidateFrozenAt", "candidateGridHash", "captureMinute", "cells", "claims", "codeHash",
    "contractVersion", "createdAt", "developmentLatestAt", "earliestValidationInputAt", "inputHash",
    "issueSchedule", "labelSchedule", "multiplicity", "panelDigest", "policyHash",
    "servingPolicyState", "site", "sourceAllowlist", "sourceCatalog", "sourceCatalogReceipt", "targetDateCount",
    "targetDates", "targetEndAt", "targetStartAt", "targetStartLocalDate", "warmupStartAt",
  ], "panel");
  // bind every manifest to the only supported contract generation
  if (panel.contractVersion !== LOCAL_FORECAST_CORRECTION_PANEL_VERSION) {
    throw new TypeError("panel contract version is invalid");
  }
  requireInteger(panel.targetDateCount, "targetDateCount", 14, 14);
  requireInteger(panel.captureMinute, "captureMinute", 0, 59);
  requireInteger(panel.multiplicity, "multiplicity", 5, 5);
  requireHash(panel.codeHash, "codeHash");
  requireHash(panel.inputHash, "inputHash");
  requireHash(panel.policyHash, "policyHash");
  requireHash(panel.candidateGridHash, "candidateGridHash");
  requireHash(panel.panelDigest, "panelDigest");
  const expectedGridHash = sha256Hex(canonicalJsonBytes(LOCAL_FORECAST_CORRECTION_CANDIDATE_GRID));
  // reject a policy search changed after planning
  if (panel.candidateGridHash !== expectedGridHash) {
    throw new TypeError("candidate grid hash is invalid");
  }
  const site = validateSite(panel.site);
  validateStringSet(panel.sourceAllowlist, "sourceAllowlist", 1, 128);
  validateLocalForecastSourceCatalog(panel.sourceCatalog);
  requireCanonicalEquality(
    panel.sourceAllowlist,
    panel.sourceCatalog.map(
      // bind the id convenience list to the complete frozen source tuples
      (source) => source.sourceId,
    ),
    "source allowlist",
  );
  const targetDates = consecutiveLocalDates(panel.targetStartLocalDate, panel.targetDateCount);
  requireCanonicalEquality(panel.targetDates, targetDates, "target dates");
  const first = localDateBounds(targetDates[0], site.timeZone);
  const last = localDateBounds(targetDates.at(-1), site.timeZone);
  const expectedWarmup = new Date(Date.parse(first.startAt) - DAY_MS).toISOString();
  // reject shifted or shortened panel timing
  if (panel.targetStartAt !== first.startAt || panel.targetEndAt !== last.endAt || panel.warmupStartAt !== expectedWarmup) {
    throw new TypeError("panel target bounds are invalid");
  }
  const issueSchedule = hourlyIssueSchedule(expectedWarmup, last.endAt, panel.captureMinute);
  requireCanonicalEquality(panel.issueSchedule, issueSchedule, "issue schedule");
  const labelSchedule = targetDates.map(
    // reproduce every declared seal without reading outcomes
    (targetDate) => createLabelSchedule(targetDate, site.timeZone),
  );
  requireCanonicalEquality(panel.labelSchedule, labelSchedule, "label schedule");
  validatePanelTiming(panel);
  validatePanelCells(panel.cells);
  // bind the policy digest to the five exact cell choices
  if (panel.policyHash !== localForecastCorrectionPolicyHash(panel.cells)) {
    throw new TypeError("policy hash is invalid");
  }
  validatePanelClaims(panel);
  // reject any post-freeze policy timing input or claim substitution
  if (panel.panelDigest !== panelManifestDigest(panel)) {
    throw new TypeError("panel digest differs");
  }
  return panel;
}

// validate one normalized issue snapshot
export function validateLocalForecastIssueSnapshot(issue) {
  requireExactKeys(issue, [
    "contractVersion", "current", "evaluatedAt", "forecast", "normalizedDigest",
    "panelDigest", "pressureContext", "receipts", "scheduledAt", "site", "snapshotId",
    "solarCloudEvidence",
  ], "issue snapshot");
  // bind issue bytes to the approved capture contract
  if (issue.contractVersion !== LOCAL_FORECAST_CORRECTION_ISSUE_VERSION) {
    throw new TypeError("issue contract version is invalid");
  }
  requireNonemptyString(issue.snapshotId, "snapshotId");
  requireHash(issue.panelDigest, "panelDigest");
  const scheduledAt = requireInstant(issue.scheduledAt, "scheduledAt");
  const evaluatedAt = requireInstant(issue.evaluatedAt, "evaluatedAt");
  // prohibit snapshots assembled before their declared capture instant
  if (evaluatedAt < scheduledAt) {
    throw new RangeError("issue evaluation precedes its schedule");
  }
  validateSite(issue.site);
  requireArray(issue.current, "current", 0, 128);
  requireArray(issue.forecast, "forecast", 0, 400);
  requireArray(issue.pressureContext, "pressureContext", 0, 6);
  // accept only absent or complete six-row pressure context
  if (issue.pressureContext.length !== 0 && issue.pressureContext.length !== 6) {
    throw new RangeError("issue pressure context must be absent or six rows");
  }
  for (const row of [...issue.current, ...issue.forecast, ...issue.pressureContext]) {
    // validate only the normalized dto boundary needed by the evaluator
    validateWeatherRecord(row, evaluatedAt);
  }
  validatePressureContext(issue.pressureContext);
  validateIssueReceipts(issue.receipts, evaluatedAt);
  // retain only compact normalized solar evidence
  if (issue.solarCloudEvidence !== null) {
    requireObject(issue.solarCloudEvidence, "solarCloudEvidence");
  }
  requireHash(issue.normalizedDigest, "normalizedDigest");
  // reject bytes changed after atomic publication
  if (issue.normalizedDigest !== issueSnapshotDigest(issue)) {
    throw new TypeError("issue normalized digest differs");
  }
  return issue;
}

// validate one immutable one-shot label seal
export function validateLocalForecastLabelSeal(labels) {
  const baseKeys = [
    "contractVersion", "from", "normalizedDigest", "panelDigest", "receipts", "rows", "sealAt",
    "sourceAllowlist", "status", "targetDate", "to",
  ];
  const keys = labels?.status === "unsupported" ? [...baseKeys, "reasonCode"] : baseKeys;
  requireExactKeys(labels, keys, "label seal");
  // bind label bytes to the approved public-history contract
  if (labels.contractVersion !== LOCAL_FORECAST_CORRECTION_LABELS_VERSION) {
    throw new TypeError("label contract version is invalid");
  }
  parseLocalDate(labels.targetDate);
  requireHash(labels.panelDigest, "panelDigest");
  const from = requireInstant(labels.from, "from");
  const to = requireInstant(labels.to, "to");
  const sealAt = requireInstant(labels.sealAt, "sealAt");
  // reject reversed or oversized label ranges
  if (to <= from || to - from > 36 * 3_600_000) {
    throw new RangeError("label window is invalid");
  }
  // require the one declared terminal state
  if (labels.status !== "complete" && labels.status !== "unsupported") {
    throw new TypeError("label status is invalid");
  }
  requireArray(labels.rows, "rows", 0, 20_000);
  validateStringSet(labels.sourceAllowlist, "sourceAllowlist", 1, 128);
  // a successful one-shot seal must retain transport evidence
  if (labels.status === "complete" && labels.receipts.length === 0) {
    throw new RangeError("complete label seal has no receipt");
  }
  // unsupported windows cannot retain a favorable partial score
  if (labels.status === "unsupported" && labels.rows.length !== 0) {
    throw new RangeError("unsupported label seal must not retain rows");
  }
  // retain a stable bounded reason for unsupported seals
  if (labels.status === "unsupported") {
    requireNonemptyString(labels.reasonCode, "reasonCode");
  }
  for (const row of labels.rows) {
    // reject late revisions and halo escapes before scoring
    validateWeatherRecord(row, sealAt);
    const validAt = requireInstant(row.validAt, "label validAt");
    const receivedAt = requireInstant(row.receivedAt, "label receivedAt");
    // retain only physical sources frozen before the one-shot query
    if (!labels.sourceAllowlist.includes(row.provenance.sourceId)) {
      throw new RangeError("label source is outside the allowlist");
    }
    // preserve only records inside the declared union context
    if (validAt < from || validAt >= to) {
      throw new RangeError("label window excludes a row");
    }
    // never reconstruct a revision received after the frozen cutoff
    if (receivedAt > sealAt) {
      throw new RangeError("label cutoff excludes a row");
    }
  }
  validateLabelReceipts(labels.receipts, sealAt, labels.sourceAllowlist, labels.status);
  requireHash(labels.normalizedDigest, "normalizedDigest");
  // reject bytes changed after the one-shot seal
  if (labels.normalizedDigest !== labelSealDigest(labels)) {
    throw new TypeError("label normalized digest differs");
  }
  return labels;
}

// validate the pre-outcome timing and embargo
function validatePanelTiming(panel) {
  const frozenAt = requireInstant(panel.candidateFrozenAt, "candidateFrozenAt");
  const createdAt = requireInstant(panel.createdAt, "createdAt");
  const developmentLatestAt = requireInstant(panel.developmentLatestAt, "developmentLatestAt");
  const earliestValidationInputAt = requireInstant(panel.earliestValidationInputAt, "earliestValidationInputAt");
  const warmupStartAt = requireInstant(panel.warmupStartAt, "warmupStartAt");
  requireExactKeys(
    panel.sourceCatalogReceipt,
    ["envelopeSha256", "receivedAt"],
    "source catalog receipt",
  );
  requireHash(panel.sourceCatalogReceipt.envelopeSha256, "source catalog envelopeSha256");
  const catalogReceivedAt = requireInstant(
    panel.sourceCatalogReceipt.receivedAt,
    "source catalog receivedAt",
  );
  const expectedEarliestInputAt = Date.parse(panel.issueSchedule[0]) - 3.5 * 3_600_000;
  // bind the embargo boundary to the first pressure-label halo input
  if (earliestValidationInputAt !== expectedEarliestInputAt) {
    throw new RangeError("earliest validation input differs from the schedule");
  }
  // the policy freeze is the actual completed discovery and manifest creation instant
  if (createdAt !== frozenAt || catalogReceivedAt > createdAt || createdAt > earliestValidationInputAt) {
    throw new RangeError("candidate freeze is not the manifest creation receipt");
  }
  // freeze before the first validation anchor or issue input
  if (frozenAt < developmentLatestAt || frozenAt > earliestValidationInputAt || earliestValidationInputAt > warmupStartAt) {
    throw new RangeError("candidate freeze is after validation input");
  }
  // separate every development endpoint from validation by at least twenty-seven hours
  if (earliestValidationInputAt - developmentLatestAt < 27 * 3_600_000) {
    throw new RangeError("panel violates the 27-hour embargo");
  }
}

// validate the five immutable source-tier comparisons
function validatePanelCells(cells) {
  requireArray(cells, "cells", 5, 5);
  for (let index = 0; index < LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS.length; index += 1) {
    // preserve exact predeclared cell order and target regime
    const expected = LOCAL_FORECAST_CORRECTION_PRIMARY_CELLS[index];
    const actual = cells[index];
    requireExactKeys(actual, ["cellId", "issueAnchorTier", "metric", "policy", "targetLabelTier"], `cell ${index}`);
    // prohibit cross-tier or post-outcome comparison replacement
    if (actual.cellId !== expected.cellId || actual.metric !== expected.metric ||
        actual.issueAnchorTier !== expected.issueAnchorTier || actual.targetLabelTier !== expected.targetLabelTier) {
      throw new TypeError("panel primary cells differ");
    }
    validatePolicy(actual.policy);
  }
}

// validate honest evidence and serving claims
function validatePanelClaims(panel) {
  requireExactKeys(panel.claims, ["candidateOutput", "inputProduct", "servingOutput"], "claims");
  // describe only the bounded public inputs actually captured
  if (panel.claims.inputProduct !== "as_available_captured_regional_anchor") {
    throw new TypeError("input product claim is invalid");
  }
  // counterfactual candidates cannot become adjusted issuance by report text
  if (panel.claims.candidateOutput !== "prospective_causal_counterfactual") {
    throw new TypeError("candidate output claim is invalid");
  }
  // the first delivery contract permits only compiled-zero serving
  if (panel.servingPolicyState !== "zero" || panel.claims.servingOutput !== "regional_zero") {
    throw new TypeError("serving output claim is invalid");
  }
}

// validate one closed candidate policy
function validatePolicy(policy) {
  requireExactKeys(policy, ["horizonHours", "strength"], "policy");
  // retain only the fixed grid values
  if (!POLICY_HORIZONS.has(policy.horizonHours) || !POLICY_STRENGTHS.has(policy.strength)) {
    throw new TypeError("candidate policy is outside the fixed grid");
  }
  // canonicalize the zero policy to one identity
  if (policy.strength === 0 && policy.horizonHours !== 3) {
    throw new TypeError("zero policy identity is invalid");
  }
}

// validate normalized site geometry
function validateSite(site) {
  requireExactKeys(site, ["latitude", "longitude", "siteKey", "timeZone"], "site");
  requireNonemptyString(site.siteKey, "siteKey");
  requireTimeZone(site.timeZone);
  // keep geometry within physical bounds
  if (!Number.isFinite(site.latitude) || site.latitude < -90 || site.latitude > 90 ||
      !Number.isFinite(site.longitude) || site.longitude < -180 || site.longitude > 180) {
    throw new RangeError("site geometry is invalid");
  }
  return site;
}

// validate one evaluator-facing weather record projection
function validateWeatherRecord(row, cutoffAt) {
  requireObject(row, "weather record");
  requireNonemptyString(row.id, "weather record id");
  const validAt = requireInstant(row.validAt, "weather record validAt");
  const receivedAt = requireInstant(row.receivedAt, "weather record receivedAt");
  requireObject(row.metrics, "weather record metrics");
  requireObject(row.provenance, "weather record provenance");
  requireNonemptyString(row.provenance.providerKey, "providerKey");
  requireNonemptyString(row.provenance.sourceId, "sourceId");
  requireNonemptyString(row.provenance.sourceKind, "sourceKind");
  requireNonemptyString(row.provenance.stationSlug, "stationSlug");
  // ensure no record arrived after its assembled snapshot or seal
  if (receivedAt > cutoffAt) {
    throw new RangeError("weather record is unavailable at cutoff");
  }
  // physical sensors cannot arrive before the reading they claim
  if (row.provenance.sourceKind === "physical_sensor" && receivedAt < validAt) {
    throw new RangeError("physical weather record arrived before valid time");
  }
  // validate retrieval-vintage timing when present
  if (row.productRunAt !== undefined && row.productRunAt !== null) {
    const productRunAt = requireInstant(row.productRunAt, "productRunAt");
    // future retrieval identities are not causal inputs
    if (productRunAt > cutoffAt) {
      throw new RangeError("forecast retrieval is unavailable at cutoff");
    }
  }
}

// validate complete same-source same-run hourly pressure context
function validatePressureContext(rows) {
  // absence is an explicitly unsupported fallback
  if (rows.length === 0) {
    return;
  }
  const first = rows[0];
  const sourceId = first.provenance.sourceId;
  const productRunAt = first.productRunAt;
  // context requires one explicit retrieval vintage
  if (first.provenance.sourceKind !== "forecast" || typeof productRunAt !== "string") {
    throw new TypeError("issue pressure context product run is absent");
  }
  for (let index = 0; index < rows.length; index += 1) {
    // prohibit mixed sources runs or gaps
    const row = rows[index];
    if (row.provenance.sourceKind !== "forecast" || row.provenance.sourceId !== sourceId ||
        row.productRunAt !== productRunAt || !Number.isFinite(row.metrics.pressureHpa) ||
        row.metrics.pressureHpa < 800 || row.metrics.pressureHpa > 1_200 ||
        Date.parse(row.validAt) !== Date.parse(first.validAt) + index * 3_600_000) {
      throw new RangeError("issue pressure context is not contiguous same-run data");
    }
  }
}

// validate the three bounded product receipts
function validateIssueReceipts(receipts, evaluatedAt) {
  requireArray(receipts, "issue receipts", 3, 3);
  const seen = new Set();
  let lastReceiptAt = Number.NEGATIVE_INFINITY;
  for (const receipt of receipts) {
    // retain exact public product identities only
    requireExactKeys(receipt, ["envelopeSha256", "generatedAt", "kind", "receivedAt"], "issue receipt");
    requireHash(receipt.envelopeSha256, "envelopeSha256");
    const receivedAt = requireInstant(receipt.receivedAt, "receipt receivedAt");
    // generated time is optional because every endpoint may not expose it
    if (receipt.generatedAt !== null) {
      const generatedAt = requireInstant(receipt.generatedAt, "receipt generatedAt");
      // a response cannot be generated after its completed receipt
      if (generatedAt > receivedAt) {
        throw new RangeError("issue receipt precedes its generation");
      }
    }
    // reject repeated or private product categories
    if (!ISSUE_RECEIPT_KINDS.includes(receipt.kind) || seen.has(receipt.kind)) {
      throw new TypeError("issue receipt kind is invalid");
    }
    seen.add(receipt.kind);
    lastReceiptAt = Math.max(lastReceiptAt, receivedAt);
  }
  // the final public response receipt is the fixed evaluation instant
  if (lastReceiptAt !== evaluatedAt) {
    throw new RangeError("issue evaluation receipt is not final");
  }
}

// validate bounded public-history page receipts
function validateLabelReceipts(receipts, sealAt, sourceAllowlist, status) {
  requireArray(receipts, "label receipts", 0, 80);
  let priorPage = 0;
  let priorReceiptAt = sealAt;
  let priorSourceIndex = 0;
  const seenSources = new Set();
  for (const receipt of receipts) {
    // retain only pagination identity and timing
    requireExactKeys(receipt, [
      "envelopeSha256", "generatedAt", "page", "receivedAt", "requestSha256", "sourceId",
    ], "label receipt");
    requireHash(receipt.envelopeSha256, "envelopeSha256");
    requireHash(receipt.requestSha256, "requestSha256");
    requireNonemptyString(receipt.sourceId, "label receipt sourceId");
    const sourceIndex = sourceAllowlist.indexOf(receipt.sourceId);
    requireInteger(receipt.page, "label receipt page", 1, 80);
    const generatedAt = requireInstant(receipt.generatedAt, "label receipt generatedAt");
    const receivedAt = requireInstant(receipt.receivedAt, "label receipt receivedAt");
    // pages must finish monotonically inside the five-minute seal attempt
    if (sourceIndex < 0 || sourceIndex < priorSourceIndex ||
        receipt.page !== priorPage + 1 || receivedAt < priorReceiptAt ||
        generatedAt > receivedAt || receivedAt < sealAt || receivedAt > sealAt + 5 * 60_000) {
      throw new RangeError("label receipt sequence is invalid");
    }
    seenSources.add(receipt.sourceId);
    priorPage = receipt.page;
    priorReceiptAt = receivedAt;
    priorSourceIndex = sourceIndex;
  }
  // a complete seal must prove every frozen source query ran
  if (status === "complete" && seenSources.size !== sourceAllowlist.length) {
    throw new RangeError("complete label seal omits a source receipt");
  }
}

// derive every predeclared issue instant
function hourlyIssueSchedule(fromAt, toAt, minute) {
  const from = requireInstant(fromAt, "issue schedule from");
  const to = requireInstant(toAt, "issue schedule to");
  const hour = Math.floor(from / 3_600_000) * 3_600_000;
  let next = hour + minute * 60_000;
  // advance a capture minute already behind the interval
  if (next < from) {
    next += 3_600_000;
  }
  const schedule = [];
  for (let at = next; at < to; at += 3_600_000) {
    // retain every real utc hour through the target interval
    schedule.push(new Date(at).toISOString());
  }
  // fail closed before the conservative dst ceiling
  if (schedule.length > 375) {
    throw new RangeError("issue schedule exceeds 375 captures");
  }
  return schedule;
}

// derive one target date's fixed history seal
function createLabelSchedule(targetDate, timeZone) {
  const bounds = localDateBounds(targetDate, timeZone);
  const start = Date.parse(bounds.startAt);
  const end = Date.parse(bounds.endAt);
  return Object.freeze({
    from: new Date(start - 3.5 * 3_600_000).toISOString(),
    sealAt: new Date(end + 6 * 3_600_000).toISOString(),
    targetDate,
    to: new Date(end + 3_600_000).toISOString(),
  });
}

// list consecutive calendar labels without assuming twenty-four-hour utc days
function consecutiveLocalDates(start, count) {
  const dates = [];
  for (let offset = 0; offset < count; offset += 1) {
    // shift calendar fields rather than elapsed milliseconds
    dates.push(shiftLocalDate(start, offset));
  }
  return dates;
}

// shift one gregorian date by whole calendar days
function shiftLocalDate(localDate, days) {
  const [year, month, day] = parseLocalDate(localDate);
  const shifted = new Date(Date.UTC(year, month - 1, day + days));
  return `${shifted.getUTCFullYear().toString().padStart(4, "0")}-${(shifted.getUTCMonth() + 1).toString().padStart(2, "0")}-${shifted.getUTCDate().toString().padStart(2, "0")}`;
}

// resolve a local midnight across timezone offset changes
function localMidnightUtc(year, month, day, timeZone) {
  const desired = Date.UTC(year, month - 1, day);
  let instant = desired;
  for (let attempt = 0; attempt < 4; attempt += 1) {
    // converge the formatted local wall time onto midnight
    const local = zonedParts(instant, timeZone);
    const represented = Date.UTC(local.year, local.month - 1, local.day, local.hour, local.minute, local.second);
    instant += desired - represented;
  }
  const resolved = zonedParts(instant, timeZone);
  // reject zones whose calendar did not resolve to a real midnight
  if (resolved.year !== year || resolved.month !== month || resolved.day !== day ||
      resolved.hour !== 0 || resolved.minute !== 0 || resolved.second !== 0) {
    throw new RangeError("local date midnight is not resolvable");
  }
  return instant;
}

// format one utc instant into local calendar parts
function zonedParts(instant, timeZone) {
  const formatter = new Intl.DateTimeFormat("en-CA", {
    day: "2-digit",
    hour: "2-digit",
    hourCycle: "h23",
    minute: "2-digit",
    month: "2-digit",
    second: "2-digit",
    timeZone,
    year: "numeric",
  });
  const values = Object.fromEntries(formatter.formatToParts(new Date(instant)).map(
    // retain only named formatter fields
    (part) => [part.type, part.value],
  ));
  return {
    day: Number(values.day),
    hour: Number(values.hour),
    minute: Number(values.minute),
    month: Number(values.month),
    second: Number(values.second),
    year: Number(values.year),
  };
}

// validate one local calendar date
function parseLocalDate(value) {
  const match = typeof value === "string" ? LOCAL_DATE.exec(value) : null;
  // reject ambiguous or normalized calendar dates
  if (match === null) {
    throw new TypeError("local date is invalid");
  }
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const parsed = new Date(Date.UTC(year, month - 1, day));
  // require the exact requested gregorian fields
  if (parsed.getUTCFullYear() !== year || parsed.getUTCMonth() + 1 !== month || parsed.getUTCDate() !== day) {
    throw new RangeError("local date is invalid");
  }
  return [year, month, day];
}

// validate one installed iana timezone
function requireTimeZone(value) {
  requireNonemptyString(value, "timeZone");
  try {
    new Intl.DateTimeFormat("en", { timeZone: value }).format(0);
  } catch {
    throw new RangeError("timeZone is invalid");
  }
}

// require one canonical utc instant
function requireInstant(value, name) {
  // reject offsets and sub-millisecond variants in immutable evidence
  if (typeof value !== "string" || !INSTANT.test(value) || new Date(value).toISOString() !== value) {
    throw new TypeError(`${name} is invalid`);
  }
  return Date.parse(value);
}

// require one lowercase sha256 digest
function requireHash(value, name) {
  // reject aliases and truncated evidence identities
  if (typeof value !== "string" || !HASH.test(value)) {
    throw new TypeError(`${name} is invalid`);
  }
}

// require one bounded integer
function requireInteger(value, name, minimum, maximum) {
  // reject fractional and out-of-contract counts
  if (!Number.isInteger(value) || value < minimum || value > maximum) {
    throw new RangeError(`${name} is invalid`);
  }
}

// require one bounded array
function requireArray(value, name, minimum, maximum) {
  // reject absent or resource-unbounded collections
  if (!Array.isArray(value) || value.length < minimum || value.length > maximum) {
    throw new RangeError(`${name} is invalid`);
  }
}

// require one sorted duplicate-free identifier allowlist
function validateStringSet(value, name, minimum, maximum) {
  requireArray(value, name, minimum, maximum);
  for (const entry of value) {
    // reject blank or nonstring source identities
    requireNonemptyString(entry, name);
  }
  const canonical = [...new Set(value)].toSorted();
  // prevent duplicates and order-dependent evidence hashes
  if (canonical.length !== value.length || canonical.some((entry, index) => entry !== value[index])) {
    throw new TypeError(`${name} is not canonical`);
  }
}

// require one nonarray object
function requireObject(value, name) {
  // reject null arrays and primitives
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} is invalid`);
  }
  return value;
}

// require one stable identifier
function requireNonemptyString(value, name) {
  // reject missing whitespace-only identifiers
  if (typeof value !== "string" || value.trim() === "") {
    throw new TypeError(`${name} is invalid`);
  }
}

// reject hidden fields at every durable contract boundary
function requireExactKeys(value, keys, name) {
  requireObject(value, name);
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  // reject both missing and silently widened fields
  if (canonicalJson(actual) !== canonicalJson(expected)) {
    throw new TypeError(`${name} fields are invalid`);
  }
}

// compare two stable json projections
function requireCanonicalEquality(actual, expected, name) {
  // reject reordered duplicated or changed schedule values
  if (canonicalJson(actual) !== canonicalJson(expected)) {
    throw new TypeError(`${name} differ from the contract`);
  }
}

// omit one self-referential digest field
function withoutField(value, field) {
  requireObject(value, "digest input");
  return Object.fromEntries(Object.entries(value).filter(
    // exclude only the named digest field
    ([key]) => key !== field,
  ));
}

// serialize json with recursively sorted object keys
function canonicalJson(value) {
  // preserve json scalar semantics
  if (value === null || typeof value !== "object") {
    const scalar = JSON.stringify(value);
    // reject values json would silently omit
    if (scalar === undefined) {
      throw new TypeError("canonical json value is unsupported");
    }
    return scalar;
  }
  // retain array order as evidence order
  if (Array.isArray(value)) {
    return `[${value.map(
      // encode every member recursively
      (entry) => canonicalJson(entry),
    ).join(",")}]`;
  }
  const entries = Object.entries(value).sort(([left], [right]) => left.localeCompare(right));
  return `{${entries.map(
    // encode every object key and value explicitly
    ([key, entry]) => `${JSON.stringify(key)}:${canonicalJson(entry)}`,
  ).join(",")}}`;
}

// freeze immutable manifest output recursively
function deepFreeze(value) {
  // primitives require no ownership wrapper
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) {
    return value;
  }
  for (const entry of Object.values(value)) {
    // freeze child collections before their parent
    deepFreeze(entry);
  }
  return Object.freeze(value);
}
