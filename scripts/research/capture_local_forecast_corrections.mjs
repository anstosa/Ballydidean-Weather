#!/usr/bin/env node

import {
  lstat,
  mkdir,
  mkdtemp,
  open,
  readFile,
  readdir,
  realpath,
  rename,
  rm,
  unlink,
} from "node:fs/promises";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  canonicalJsonBytes,
  createLocalForecastCorrectionPanel,
  issueSnapshotDigest,
  labelSealDigest,
  localForecastCorrectionPolicyHash,
  sha256Hex,
  validateLocalForecastCorrectionPanel,
  validateLocalForecastIssueSnapshot,
  validateLocalForecastLabelSeal,
  validateLocalForecastPhysicalSource,
  validateLocalForecastSourceCatalog,
  validateLocalForecastWs90Source,
} from "./local_forecast_correction_contract.mjs";
import { computeLocalForecastCorrectionCodeHash } from "./evaluate_local_forecast_corrections.mjs";

export const LOCAL_FORECAST_CORRECTION_CAPTURE_VERSION =
  "weather-local-correction-capture/v1";
export const LOCAL_FORECAST_CORRECTION_RETROSPECTIVE_HISTORY_VERSION =
  "weather-local-correction-retrospective-history/v1";

const HOUR_MS = 3_600_000;
const MINUTE_MS = 60_000;
const RESPONSE_BYTES = 2 * 1_024 * 1_024;
const HISTORY_PAGE_BYTES = 1 * 1_024 * 1_024;
const ISSUE_MEMBER_BYTES = 2 * 1_024 * 1_024;
const LABEL_MEMBER_BYTES = 24 * 1_024 * 1_024;
const TOTAL_EVIDENCE_BYTES = 128 * 1_024 * 1_024;
const ISSUE_START_GRACE_MS = 5 * MINUTE_MS;
const LABEL_DEADLINE_MS = 5 * MINUTE_MS;
const REQUEST_TIMEOUT_MS = 30_000;
const TRANSPORT_RETRIES = 2;
const HISTORY_LIMIT = 250;
const HISTORY_PAGE_LIMIT = 80;
const HISTORY_ROW_LIMIT = 20_000;
const RETROSPECTIVE_WINDOW_MS = 36 * HOUR_MS;
const RUNTIME_TARGETS_BYTES = 64 * 1_024;
const FARM_LIVE_SOURCE = "ecowitt-88f15505d89f-local-live-v1";
const HASH = /^[a-f0-9]{64}$/u;
const INSTANT = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;
const LOCAL_DATE = /^\d{4}-\d{2}-\d{2}$/u;
const SOURCE_ID = /^[1-9]\d*$/u;
const repositoryRoot = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const privateEvidenceRoot = resolve(repositoryRoot, ".weather-data");
const runtimeTargetsPath = resolve(repositoryRoot, "config/runtime-targets.json");

const METRIC_RANGES = Object.freeze({
  apparentTemperatureC: [-120, 100],
  blackGlobeTemperatureC: [-120, 150],
  cloudCoverPercent: [0, 100],
  pm25MicrogramsPerCubicMeter: [0, 5_000],
  precipitationMm: [0, 2_000],
  precipitationRateMmPerHour: [0, 2_000],
  pressureHpa: [800, 1_200],
  relativeHumidityPercent: [0, 100],
  soilElectricalConductivityMicrosiemensPerCm: [0, 100_000],
  soilMoisturePercent: [0, 100],
  solarRadiationWm2: [0, 2_500],
  temperatureC: [-100, 100],
  uvIndex: [0, 20],
  wetBulbGlobeTemperatureC: [-120, 150],
  windDirectionDegrees: [0, 360],
  windGustMps: [0, 200],
  windSpeedMps: [0, 200],
});

// bind a panel to one exact https api base site and source set
export function localForecastCaptureInputHash(baseUrl, site, sourceAllowlist) {
  const normalizedBaseUrl = normalizeBaseUrl(baseUrl);
  const normalizedSite = validateCaptureSite(site);
  validateSourceAllowlist(sourceAllowlist);
  return sha256Hex(canonicalJsonBytes({
    baseUrl: normalizedBaseUrl,
    site: normalizedSite,
    sourceAllowlist,
  }));
}

// derive the only production cli api base from checked runtime configuration
export function configuredRemoteCaptureBaseUrl(configuration) {
  requireObject(configuration, "runtime targets");
  requireObject(configuration.remote, "remote runtime target");
  requireObject(configuration.remote.tunnel, "remote tunnel target");
  const configured = requireString(
    configuration.remote.tunnel.origin,
    "remote tunnel origin",
    1,
    2_048,
  );
  let origin;
  try {
    origin = new URL(configured);
  } catch {
    throw new TypeError("remote tunnel origin is invalid");
  }
  // require one credential-free https origin before adding the fixed api path
  if (origin.protocol !== "https:" || origin.username !== "" || origin.password !== "" ||
      origin.pathname !== "/" || origin.search !== "" || origin.hash !== "") {
    throw new RangeError("remote tunnel origin must be one https origin");
  }
  return normalizeBaseUrl(new URL("/api/v1", origin).href);
}

// select the fixed public physical source catalog from one sites envelope
export function discoverLocalForecastSources(envelope, expectedSite) {
  requireObject(envelope, "sites envelope");
  requireArray(envelope.data, "sites", 1, 128);
  const site = envelope.data.find(
    // bind discovery to the requested configured site
    (candidate) => candidate?.slug === expectedSite.siteKey,
  );
  // fail when the configured site is absent or changed
  if (site === undefined) {
    throw new TypeError("configured site is absent from discovery");
  }
  validateDiscoveredSite(site, expectedSite);
  const sources = [];
  for (const station of site.stations) {
    // accept only complete public station entries
    requireObject(station, "site station");
    requireString(station.slug, "station slug", 1, 256);
    requireArray(station.sources, "station sources", 0, 256);
    for (const source of station.sources) {
      // freeze physical public inputs and categorically exclude derived or legacy farm feeds
      if (source?.kind !== "physical_sensor" || !SOURCE_ID.test(source.id ?? "") ||
          typeof source.key !== "string" || source.key.startsWith("rain-target-") ||
          source.key.includes("derived") ||
          (station.slug === "ballydidean-ecowitt" && source.key !== FARM_LIVE_SOURCE)) {
        continue;
      }
      sources.push(Object.freeze({
        providerKey: requireString(source.providerKey, "source providerKey", 1, 128),
        sourceId: source.id,
        sourceKey: source.key,
        stationSlug: station.slug,
      }));
    }
  }
  const ordered = sources.toSorted((left, right) => left.sourceId.localeCompare(right.sourceId));
  // source ids must be complete and unambiguous
  if (ordered.length === 0 || new Set(ordered.map((source) => source.sourceId)).size !== ordered.length) {
    throw new RangeError("eligible physical source catalog is empty or duplicated");
  }
  return Object.freeze({
    site: Object.freeze({ ...expectedSite }),
    sourceAllowlist: Object.freeze(ordered.map((source) => source.sourceId)),
    sources: Object.freeze(ordered),
  });
}

// initialize one pre-outcome panel after exactly one public source discovery
export async function initializeLocalForecastCorrectionPanel({
  baseUrl,
  clock = defaultClock,
  fetcher = fetch,
  input,
  sleep = defaultSleep,
}) {
  const baseInputKeys = [
    "captureMinute", "cells", "claims", "developmentLatestAt", "earliestValidationInputAt",
    "servingPolicyState", "site", "targetDateCount", "targetStartLocalDate",
  ];
  requireExactKeys(
    input,
    Object.hasOwn(input, "candidateFrozenAt")
      ? [...baseInputKeys, "candidateFrozenAt"]
      : baseInputKeys,
    "capture panel input",
  );
  const normalizedBaseUrl = normalizeBaseUrl(baseUrl);
  const discovery = await fetchBoundedPublicJson(`${normalizedBaseUrl}/sites`, {
    baseUrl: normalizedBaseUrl,
    clock,
    fetcher,
    maximumBytes: RESPONSE_BYTES,
    sleep,
  });
  const catalog = discoverLocalForecastSources(discovery.value, input.site);
  const cells = input.cells;
  const codeHash = await computeLocalForecastCorrectionCodeHash();
  const createdAt = new Date(clockNow(clock)).toISOString();
  // a supplied freeze assertion must equal actual manifest creation rather than backdate it
  if (input.candidateFrozenAt !== undefined && input.candidateFrozenAt !== createdAt) {
    throw new RangeError("candidate freeze assertion differs from manifest creation");
  }
  const { candidateFrozenAt: _candidateFrozenAt, ...panelInput } = input;
  return createLocalForecastCorrectionPanel({
    ...panelInput,
    candidateFrozenAt: createdAt,
    codeHash,
    createdAt,
    inputHash: localForecastCaptureInputHash(
      normalizedBaseUrl,
      catalog.site,
      catalog.sourceAllowlist,
    ),
    policyHash: localForecastCorrectionPolicyHash(cells),
    sourceAllowlist: catalog.sourceAllowlist,
    sourceCatalog: catalog.sources,
    sourceCatalogReceipt: {
      envelopeSha256: discovery.envelopeSha256,
      receivedAt: discovery.receivedAt,
    },
  });
}

// copy one complete allowlisted weather dto at a frozen cutoff
export function normalizeLocalForecastWeatherRecord(value, cutoffAt, {
  availableAt = cutoffAt,
  current = false,
} = {}) {
  const cutoff = requireInstant(cutoffAt, "weather cutoff");
  const available = requireInstant(availableAt, "weather availability cutoff");
  requireObject(value, "weather record");
  const validAt = requireInstant(value.validAt, "weather validAt");
  const receivedAt = requireInstant(value.receivedAt, "weather receivedAt");
  // records unavailable at the assembled cutoff cannot be retained
  if (receivedAt > cutoff || receivedAt > available) {
    throw new RangeError("weather record is unavailable at cutoff");
  }
  requireObject(value.provenance, "weather provenance");
  requireObject(value.metrics, "weather metrics");
  const sourceKind = requireEnum(value.provenance.sourceKind, [
    "forecast", "model_current", "physical_sensor", "reanalysis",
    "tide_observation", "tide_prediction",
  ], "source kind");
  // physical readings cannot arrive before their observation time
  if (sourceKind === "physical_sensor" && receivedAt < validAt) {
    throw new RangeError("physical weather record precedes validAt");
  }
  // current product rows cannot describe a future observation
  if (current && validAt > cutoff) {
    throw new RangeError("current weather record is in the future");
  }
  let productRunAt = null;
  // bind forecast vintages to inputs available by the cutoff
  if (value.productRunAt !== null) {
    productRunAt = requireCanonicalInstant(value.productRunAt, "productRunAt");
    if (Date.parse(productRunAt) > cutoff || Date.parse(productRunAt) > available) {
      throw new RangeError("forecast product run is unavailable at cutoff");
    }
  }
  // preserve the public record-kind product identity invariant
  if ((sourceKind === "physical_sensor" && productRunAt !== null) ||
      (sourceKind === "forecast" && productRunAt === null)) {
    throw new RangeError("weather product run differs from its source kind");
  }
  const metrics = {};
  for (const [metric, [minimum, maximum]] of Object.entries(METRIC_RANGES)) {
    // preserve dto shape while removing private property-only soil observations
    metrics[metric] = metric === "soilElectricalConductivityMicrosiemensPerCm" ||
      metric === "soilMoisturePercent"
      ? null
      : requireNullableNumber(value.metrics[metric], metric, minimum, maximum);
  }
  const metadata = normalizeMetadata(value.metadata);
  const ageSeconds = Math.max(0, Math.floor((cutoff - validAt) / 1_000));
  const freshness = ageSeconds <= 1_800
    ? { ageSeconds, label: "Weather value is current", status: "fresh" }
    : ageSeconds <= 7_200
      ? { ageSeconds, label: "Weather value is delayed", status: "delayed" }
      : { ageSeconds, label: "Weather value may be stale", status: "stale" };
  const normalized = {
    freshness,
    id: requireString(value.id, "weather id", 1, 512),
    metadata,
    metrics,
    productRunAt,
    provenance: {
      attribution: normalizeAttribution(value.provenance.attribution),
      label: requireString(value.provenance.label, "provenance label", 1, 256),
      providerKey: requireString(value.provenance.providerKey, "providerKey", 1, 128),
      sourceId: requireString(value.provenance.sourceId, "sourceId", 1, 512),
      sourceKey: requireString(value.provenance.sourceKey, "sourceKey", 1, 256),
      sourceKind,
      stationSlug: requireString(value.provenance.stationSlug, "stationSlug", 1, 256),
    },
    receivedAt: new Date(receivedAt).toISOString(),
    revisionCount: requireInteger(value.revisionCount, "revisionCount", 0, 1_000_000),
    validAt: new Date(validAt).toISOString(),
  };
  // current rows retain the source-aligned pressure tendency field
  if (current) {
    normalized.pressureChange3hHpa = requireNullableNumber(
      value.pressureChange3hHpa ?? null,
      "pressureChange3hHpa",
      -400,
      400,
    );
  }
  return Object.freeze(normalized);
}

// fetch one same-origin bounded json envelope with transport-only retries
export async function fetchBoundedPublicJson(url, {
  baseUrl,
  clock = defaultClock,
  deadlineAt = null,
  fetcher = fetch,
  maximumBytes,
  sleep = defaultSleep,
  timeoutMs = REQUEST_TIMEOUT_MS,
}) {
  const normalizedBaseUrl = normalizeBaseUrl(baseUrl);
  const requested = new URL(url);
  const configured = new URL(normalizedBaseUrl);
  // every request must stay on the configured https origin and api prefix
  if (requested.origin !== configured.origin ||
      !requested.pathname.startsWith(`${configured.pathname.replace(/\/$/u, "")}/`)) {
    throw new RangeError("public request escaped the configured origin");
  }
  requireInteger(maximumBytes, "maximumBytes", 1, RESPONSE_BYTES);
  let lastError = null;
  for (let attempt = 0; attempt <= TRANSPORT_RETRIES; attempt += 1) {
    // keep every retry inside the declared terminal deadline
    const now = clockNow(clock);
    if (deadlineAt !== null && now >= deadlineAt) {
      throw new RangeError("public request deadline expired");
    }
    const availableMs = deadlineAt === null ? timeoutMs : Math.min(timeoutMs, deadlineAt - now);
    try {
      const completed = await fetchWithTimeout(
        fetcher,
        requested.href,
        availableMs,
        async (response) => {
          // manual redirect handling prohibits origin or path replacement
          if (response.redirected || (response.status >= 300 && response.status < 400)) {
            throw new TerminalCaptureError("public request redirect is forbidden");
          }
          // client errors are terminal while server errors are bounded transport failures
          if (!response.ok) {
            if (response.status >= 500) {
              throw new RetryableCaptureError(`public request failed with status ${String(response.status)}`);
            }
            throw new TerminalCaptureError(`public request failed with status ${String(response.status)}`);
          }
          const responseUrl = response.url;
          // native fetch exposes the final response url for an additional origin check
          if (responseUrl !== "" && new URL(responseUrl).origin !== configured.origin) {
            throw new TerminalCaptureError("public response escaped the configured origin");
          }
          const contentType = response.headers.get("content-type") ?? "";
          // accept only the public json response contract
          if (!contentType.toLowerCase().startsWith("application/json")) {
            throw new TerminalCaptureError("public response content type is invalid");
          }
          const bytes = await readBoundedResponse(response, maximumBytes);
          let value;
          try {
            value = JSON.parse(bytes.toString("utf8"));
          } catch {
            throw new TerminalCaptureError("public response json is invalid");
          }
          return { bytes, value };
        },
      );
      return Object.freeze({
        byteLength: completed.bytes.length,
        envelopeSha256: sha256Hex(completed.bytes),
        receivedAt: new Date(clockNow(clock)).toISOString(),
        requestSha256: sha256Hex(Buffer.from(requested.href)),
        value: completed.value,
      });
    } catch (error) {
      // retry only network timeout body or server transport failures
      const retryable = error instanceof RetryableCaptureError || isNetworkError(error);
      if (!retryable || attempt === TRANSPORT_RETRIES) {
        throw error;
      }
      lastError = error;
      await sleep(250);
    }
  }
  throw lastError ?? new Error("public request failed");
}

// create and validate one private evidence directory without symlink traversal
export async function prepareLocalForecastCaptureRoot(path) {
  const absolute = resolve(path);
  const location = relative(privateEvidenceRoot, absolute);
  // capture evidence cannot escape the ignored private root
  if (location === ".." || location.startsWith(`..${sep}`)) {
    throw new RangeError("capture root must be inside .weather-data");
  }
  await ensurePrivateDirectory(privateEvidenceRoot);
  let cursor = privateEvidenceRoot;
  for (const segment of location === "" ? [] : location.split(sep)) {
    // create and verify each exact private ancestor independently
    cursor = join(cursor, segment);
    await ensurePrivateDirectory(cursor);
  }
  return absolute;
}

// capture one scheduled issue through exactly three sequential public products
export async function captureLocalForecastIssue({
  baseUrl,
  clock = defaultClock,
  fetcher = fetch,
  panel,
  root,
  scheduledAt,
  sleep = defaultSleep,
}) {
  validateLocalForecastCorrectionPanel(panel);
  verifyPanelOrigin(panel, baseUrl);
  const captureRoot = await prepareCaptureLayout(root);
  const schedule = requireCanonicalInstant(scheduledAt, "scheduledAt");
  // prohibit added or shifted issue attempts
  if (!panel.issueSchedule.includes(schedule)) {
    throw new RangeError("issue time is outside the frozen schedule");
  }
  const snapshotId = issueSnapshotId(schedule);
  const finalPath = join(captureRoot, "issues", `${snapshotId}.json`);
  const existing = await readOptionalJson(finalPath, ISSUE_MEMBER_BYTES);
  // identical existing publication is the only idempotent repeat
  if (existing !== null) {
    validateLocalForecastIssueSnapshot(existing);
    // idempotent reuse is valid only for the exact frozen panel
    if (existing.panelDigest !== panel.panelDigest) {
      throw new RangeError("existing issue differs from the frozen panel");
    }
    return existing;
  }
  const claimPath = join(captureRoot, "attempts", "issues", `${snapshotId}.claim.json`);
  const failurePath = join(captureRoot, "attempts", "issues", `${snapshotId}.failed.json`);
  const priorClaim = await readOptionalJson(claimPath, 64 * 1_024);
  // an existing exact claim permanently burns this scheduled issue without another read
  if (priorClaim !== null) {
    validateIssueAttemptClaim(priorClaim, panel, schedule, snapshotId);
    const priorFailure = await readOptionalJson(failurePath, 64 * 1_024);
    // only a durable terminal witness lets a resumed finite run continue this slot
    if (priorFailure !== null) {
      validateIssueFailureWitness(priorFailure, panel, schedule, snapshotId);
      throw new IssueSlotCaptureError(
        new RangeError(`capture attempt is terminal: ${priorFailure.reasonCode}`),
        priorFailure,
      );
    }
    throw new RangeError("capture attempt is already burned");
  }
  const startedAt = new Date(clockNow(clock)).toISOString();
  // future scheduled attempts cannot be burned before they become due
  if (Date.parse(startedAt) < Date.parse(schedule)) {
    throw new RangeError("issue attempt is not due");
  }
  await writeExclusiveJson(claimPath, {
    contractVersion: LOCAL_FORECAST_CORRECTION_CAPTURE_VERSION,
    panelDigest: panel.panelDigest,
    scheduledAt: schedule,
    snapshotId,
    startedAt,
    type: "issue_attempt",
  });
  // late starts are recorded as gaps without reading a favorable replacement vintage
  if (Date.parse(startedAt) > Date.parse(schedule) + ISSUE_START_GRACE_MS) {
    const witness = await writeIssueFailure(
      captureRoot,
      snapshotId,
      panel.panelDigest,
      schedule,
      startedAt,
      "missed_schedule",
    );
    validateIssueFailureWitness(witness, panel, schedule, snapshotId);
    throw new IssueSlotCaptureError(new RangeError("scheduled issue was missed"), witness);
  }
  let stage = null;
  let evidencePhase = false;
  try {
    stage = await createStage(captureRoot, `issue-${snapshotId}-`);
    evidencePhase = true;
    const normalizedBaseUrl = normalizeBaseUrl(baseUrl);
    const siteSlug = encodeURIComponent(panel.site.siteKey);
    const currentResponse = await fetchBoundedPublicJson(
      `${normalizedBaseUrl}/sites/${siteSlug}/current`,
      { baseUrl: normalizedBaseUrl, clock, fetcher, maximumBytes: RESPONSE_BYTES, sleep },
    );
    const forecastResponse = await fetchBoundedPublicJson(
      `${normalizedBaseUrl}/sites/${siteSlug}/forecast?days=5`,
      { baseUrl: normalizedBaseUrl, clock, fetcher, maximumBytes: RESPONSE_BYTES, sleep },
    );
    const solarResponse = await fetchBoundedPublicJson(
      `${normalizedBaseUrl}/sites/${siteSlug}/solar-cloud-evidence`,
      { baseUrl: normalizedBaseUrl, clock, fetcher, maximumBytes: RESPONSE_BYTES, sleep },
    );
    const evaluatedAt = maximumInstant([
      currentResponse.receivedAt,
      forecastResponse.receivedAt,
      solarResponse.receivedAt,
    ]);
    const currentEnvelope = normalizeWeatherEnvelope(
      currentResponse.value,
      panel.site,
      evaluatedAt,
      {
        availableAt: currentResponse.receivedAt,
        current: true,
        maximumRows: 128,
        sourceCatalog: panel.sourceCatalog,
      },
    );
    const forecastEnvelope = normalizeWeatherEnvelope(
      forecastResponse.value,
      panel.site,
      evaluatedAt,
      { availableAt: forecastResponse.receivedAt, current: false, maximumRows: 400 },
    );
    const solarCloudEvidence = normalizeSolarCloudEnvelope(
      solarResponse.value,
      evaluatedAt,
      solarResponse.receivedAt,
      panel.sourceCatalog,
    );
    const forecast = forecastEnvelope.rows.filter((row) => {
      // retain only the pressure lookback and the exact next-twenty-four-hour horizon
      const valid = Date.parse(row.validAt);
      const evaluated = Date.parse(evaluatedAt);
      return row.provenance.sourceKind === "forecast" &&
        valid >= evaluated - 3 * HOUR_MS && valid <= evaluated + 24 * HOUR_MS;
    });
    const pressureContext = normalizePressureContext(
      forecastResponse.value.pressureContext,
      evaluatedAt,
      forecastResponse.receivedAt,
    );
    const receipts = [
      issueReceipt("current", currentResponse, currentEnvelope.generatedAt),
      issueReceipt("forecast", forecastResponse, forecastEnvelope.generatedAt),
      issueReceipt("solarCloudEvidence", solarResponse, solarCloudEvidence.generatedAt),
    ];
    const unsigned = {
      contractVersion: "weather-local-correction-issue/v1",
      current: currentEnvelope.rows,
      evaluatedAt,
      forecast,
      normalizedDigest: "",
      panelDigest: panel.panelDigest,
      pressureContext,
      receipts,
      scheduledAt: schedule,
      site: panel.site,
      snapshotId,
      solarCloudEvidence: solarCloudEvidence.evidence,
    };
    const snapshot = { ...unsigned, normalizedDigest: issueSnapshotDigest(unsigned) };
    validateLocalForecastIssueSnapshot(snapshot);
    evidencePhase = false;
    await writeStageJson(stage, "current.json", currentEnvelope.rows);
    await writeStageJson(stage, "forecast.json", { forecast, pressureContext });
    await writeStageJson(stage, "solar-cloud-evidence.json", solarCloudEvidence.evidence);
    await writeStageJson(stage, "snapshot.json", snapshot);
    await publishStagedJson(
      captureRoot,
      join(stage, "snapshot.json"),
      finalPath,
      ISSUE_MEMBER_BYTES,
    );
    return snapshot;
  } catch (error) {
    // structural publication filesystem and programmer errors remain fatal to the finite run
    if (!evidencePhase || !isExpectedIssueEvidenceError(error)) {
      throw error;
    }
    const witness = await writeIssueFailure(
      captureRoot,
      snapshotId,
      panel.panelDigest,
      schedule,
      startedAt,
      captureReason(error),
    );
    validateIssueFailureWitness(witness, panel, schedule, snapshotId);
    throw new IssueSlotCaptureError(error, witness);
  } finally {
    // remove only this attempt's proven private staging directory
    if (stage !== null) {
      await rm(stage, { force: true, recursive: true });
    }
  }
}

// seal one target date exactly once from bounded source-specific history pages
export async function sealLocalForecastLabels({
  baseUrl,
  clock = defaultClock,
  fetcher = fetch,
  panel,
  root,
  sleep = defaultSleep,
  targetDate,
}) {
  validateLocalForecastCorrectionPanel(panel);
  verifyPanelOrigin(panel, baseUrl);
  const captureRoot = await prepareCaptureLayout(root);
  // target dates must be one of the fourteen predeclared members
  if (typeof targetDate !== "string" || !LOCAL_DATE.test(targetDate)) {
    throw new TypeError("target date is invalid");
  }
  const schedule = panel.labelSchedule.find(
    // bind the attempt to its exact frozen history window
    (candidate) => candidate.targetDate === targetDate,
  );
  if (schedule === undefined) {
    throw new RangeError("target date is outside the frozen panel");
  }
  const finalPath = join(captureRoot, "labels", `${targetDate}.json`);
  const existing = await readOptionalJson(finalPath, LABEL_MEMBER_BYTES);
  // a terminal complete or unsupported seal is immutable
  if (existing !== null) {
    validateLocalForecastLabelSeal(existing);
    // immutable reuse cannot cross a different panel freeze
    if (existing.panelDigest !== panel.panelDigest) {
      throw new RangeError("existing labels differ from the frozen panel");
    }
    return existing;
  }
  const claimPath = join(captureRoot, "attempts", "labels", `${targetDate}.claim.json`);
  const startedAt = new Date(clockNow(clock)).toISOString();
  const deadlineAt = Date.parse(schedule.sealAt) + LABEL_DEADLINE_MS;
  const priorClaim = await readOptionalJson(claimPath, 64 * 1_024);
  // an exact burned claim recovers only after its query deadline without new reads
  if (priorClaim !== null) {
    validateLabelAttemptClaim(priorClaim, panel, schedule);
    if (Date.parse(startedAt) >= deadlineAt) {
      return publishUnsupportedLabels(
        captureRoot,
        finalPath,
        panel,
        schedule,
        [],
        "burned_claim_recovered_after_deadline",
      );
    }
    throw new RangeError("label capture attempt is already burned");
  }
  // do not burn future one-shot attempts before their declared cutoff
  if (Date.parse(startedAt) < Date.parse(schedule.sealAt)) {
    throw new RangeError("label seal is not due");
  }
  await writeExclusiveJson(claimPath, {
    contractVersion: LOCAL_FORECAST_CORRECTION_CAPTURE_VERSION,
    panelDigest: panel.panelDigest,
    sealAt: schedule.sealAt,
    startedAt,
    targetDate,
    type: "label_attempt",
  });
  // a late invocation becomes terminal unsupported without reading outcomes
  if (Date.parse(startedAt) >= deadlineAt) {
    return publishUnsupportedLabels(
      captureRoot,
      finalPath,
      panel,
      schedule,
      [],
      "seal_deadline_missed",
    );
  }
  let stage = null;
  const rows = [];
  const receipts = [];
  let pageNumber = 0;
  let decodedBytes = 0;
  try {
    stage = await createStage(captureRoot, `label-${targetDate}-`);
    const normalizedBaseUrl = normalizeBaseUrl(baseUrl);
    const siteSlug = encodeURIComponent(panel.site.siteKey);
    for (const sourceId of panel.sourceAllowlist) {
      // page each frozen physical source independently
      let cursor = null;
      const seenCursors = new Set();
      while (true) {
        // the whole multi-source query must finish inside one five-minute deadline
        if (clockNow(clock) >= deadlineAt) {
          throw new RangeError("label query deadline expired");
        }
        pageNumber += 1;
        if (pageNumber > HISTORY_PAGE_LIMIT) {
          throw new RangeError("label page limit exceeded");
        }
        const url = historyUrl(
          normalizedBaseUrl,
          siteSlug,
          sourceId,
          schedule.from,
          schedule.to,
          cursor,
        );
        const response = await fetchBoundedPublicJson(url, {
          baseUrl: normalizedBaseUrl,
          clock,
          deadlineAt,
          fetcher,
          maximumBytes: HISTORY_PAGE_BYTES,
          sleep,
        });
        const page = normalizeHistoryPage(
          response.value,
          panel.site,
          sourceId,
          schedule,
          panel.sourceCatalog,
        );
        // each successful page must complete inside the seal attempt after generation
        if (Date.parse(page.generatedAt) > Date.parse(response.receivedAt) ||
            Date.parse(response.receivedAt) < Date.parse(schedule.sealAt) ||
            Date.parse(response.receivedAt) > deadlineAt) {
          throw new RangeError("label page receipt timing is invalid");
        }
        decodedBytes += response.byteLength;
        if (decodedBytes > LABEL_MEMBER_BYTES) {
          throw new RangeError("label decoded byte limit exceeded");
        }
        for (const row of page.rows) {
          // exclude a latest public revision received after the frozen cutoff without reconstruction
          if (Date.parse(row.receivedAt) <= Date.parse(schedule.sealAt)) {
            rows.push(row);
          }
        }
        if (rows.length > HISTORY_ROW_LIMIT) {
          throw new RangeError("label row limit exceeded");
        }
        receipts.push({
          envelopeSha256: response.envelopeSha256,
          generatedAt: page.generatedAt,
          page: pageNumber,
          receivedAt: response.receivedAt,
          requestSha256: response.requestSha256,
          sourceId,
        });
        await writeStageJson(stage, `page-${String(pageNumber).padStart(2, "0")}.json`, {
          receipt: receipts.at(-1),
          rows: page.rows,
        });
        // a null cursor closes this exact source window
        if (page.nextCursor === null) {
          break;
        }
        if (seenCursors.has(page.nextCursor)) {
          throw new RangeError("label cursor did not progress");
        }
        seenCursors.add(page.nextCursor);
        cursor = page.nextCursor;
      }
    }
    const orderedRows = rows.toSorted(compareWeatherRows);
    const unsigned = {
      contractVersion: "weather-local-correction-labels/v1",
      from: schedule.from,
      normalizedDigest: "",
      panelDigest: panel.panelDigest,
      receipts,
      rows: orderedRows,
      sealAt: schedule.sealAt,
      sourceAllowlist: panel.sourceAllowlist,
      status: "complete",
      targetDate,
      to: schedule.to,
    };
    const labels = { ...unsigned, normalizedDigest: labelSealDigest(unsigned) };
    validateLocalForecastLabelSeal(labels);
    await writeStageJson(stage, "labels.json", labels);
    await publishStagedJson(
      captureRoot,
      join(stage, "labels.json"),
      finalPath,
      LABEL_MEMBER_BYTES,
    );
    return labels;
  } catch (error) {
    return publishUnsupportedLabels(
      captureRoot,
      finalPath,
      panel,
      schedule,
      receipts,
      captureReason(error),
    );
  } finally {
    // remove only this attempt's proven private staging directory
    if (stage !== null) {
      await rm(stage, { force: true, recursive: true });
    }
  }
}

// retrieve one bounded opened-history corpus for nonqualifying development replay
export async function captureRetrospectiveDevelopmentHistory({
  baseUrl,
  clock = defaultClock,
  fetcher = fetch,
  from,
  site,
  sleep = defaultSleep,
  sourceCatalog,
  to,
}) {
  const normalizedBaseUrl = normalizeBaseUrl(baseUrl);
  const normalizedSite = validateCaptureSite(site);
  validateLocalForecastSourceCatalog(sourceCatalog);
  const sourceAllowlist = sourceCatalog.map(
    // query the exact public ids from the complete discovered source tuples
    (source) => source.sourceId,
  );
  validateSourceAllowlist(sourceAllowlist);
  const fromAt = requireInstant(from, "development history from");
  const toAt = requireInstant(to, "development history to");
  // development downloads stay within one explicitly opened thirty-six-hour window
  if (toAt <= fromAt || toAt - fromAt > RETROSPECTIVE_WINDOW_MS) {
    throw new RangeError("development history window exceeds thirty-six hours");
  }
  const deadlineAt = clockNow(clock) + LABEL_DEADLINE_MS;
  const siteSlug = encodeURIComponent(normalizedSite.siteKey);
  const rows = [];
  const receipts = [];
  let decodedBytes = 0;
  let pageNumber = 0;
  for (const sourceId of sourceAllowlist) {
    // page each declared physical source once inside the bounded retrieval
    let cursor = null;
    const seenCursors = new Set();
    while (true) {
      // one opened-history retrieval cannot run beyond its five-minute bound
      if (clockNow(clock) >= deadlineAt) {
        throw new RangeError("development history query deadline expired");
      }
      pageNumber += 1;
      if (pageNumber > HISTORY_PAGE_LIMIT) {
        throw new RangeError("development history page limit exceeded");
      }
      const response = await fetchBoundedPublicJson(
        historyUrl(normalizedBaseUrl, siteSlug, sourceId, from, to, cursor),
        {
          baseUrl: normalizedBaseUrl,
          clock,
          deadlineAt,
          fetcher,
          maximumBytes: HISTORY_PAGE_BYTES,
          sleep,
        },
      );
      const page = normalizeHistoryPage(
        response.value,
        normalizedSite,
        sourceId,
        { from, sealAt: response.receivedAt, to },
        sourceCatalog,
      );
      // opened history cannot claim a revision timestamp after its actual response
      if (page.rows.length !== response.value.data.length ||
          Date.parse(page.generatedAt) > Date.parse(response.receivedAt)) {
        throw new RangeError("development history timing is invalid");
      }
      decodedBytes += response.byteLength;
      if (decodedBytes > LABEL_MEMBER_BYTES) {
        throw new RangeError("development history decoded byte limit exceeded");
      }
      rows.push(...page.rows);
      if (rows.length > HISTORY_ROW_LIMIT) {
        throw new RangeError("development history row limit exceeded");
      }
      receipts.push(Object.freeze({
        envelopeSha256: response.envelopeSha256,
        generatedAt: page.generatedAt,
        page: pageNumber,
        receivedAt: response.receivedAt,
        requestSha256: response.requestSha256,
        sourceId,
      }));
      // a null cursor closes this source at the actual retrieval vintage
      if (page.nextCursor === null) {
        break;
      }
      if (seenCursors.has(page.nextCursor)) {
        throw new RangeError("development history cursor did not progress");
      }
      seenCursors.add(page.nextCursor);
      cursor = page.nextCursor;
    }
  }
  const retrievedAt = maximumInstant(receipts.map(
    // expose the final real response completion rather than a synthetic seal
    (receipt) => receipt.receivedAt,
  ));
  return Object.freeze({
    contractVersion: LOCAL_FORECAST_CORRECTION_RETROSPECTIVE_HISTORY_VERSION,
    from,
    historyRows: Object.freeze(rows.toSorted(compareWeatherRows)),
    limitations: Object.freeze([
      "public_history_exposes_latest_revision_at_retrieval",
      "prior_revisions_are_not_reconstructable",
      "development_only_not_qualification",
    ]),
    receipts: Object.freeze(receipts),
    retrievedAt,
    site: Object.freeze(normalizedSite),
    sourceAllowlist: Object.freeze([...sourceAllowlist]),
    sourceCatalog: Object.freeze(sourceCatalog.map(
      // retain a detached immutable tuple projection in the helper result
      (source) => Object.freeze({ ...source }),
    )),
    to,
  });
}

// run only the finite frozen issue and label event schedule
export async function runFiniteLocalForecastCapture({
  baseUrl,
  captureIssue = captureLocalForecastIssue,
  clock = defaultClock,
  fetcher = fetch,
  panel,
  root,
  sealLabels = sealLocalForecastLabels,
  sleep = defaultSleep,
}) {
  validateLocalForecastCorrectionPanel(panel);
  verifyPanelOrigin(panel, baseUrl);
  const events = [
    ...panel.issueSchedule.map(
      // represent every scheduled issue attempt once
      (scheduledAt) => ({ at: scheduledAt, kind: "issue", value: scheduledAt }),
    ),
    ...panel.labelSchedule.map(
      // represent every one-shot label seal once
      (schedule) => ({ at: schedule.sealAt, kind: "label", value: schedule.targetDate }),
    ),
  ].toSorted((left, right) => left.at.localeCompare(right.at) || left.kind.localeCompare(right.kind));
  const result = {
    issueFailed: 0,
    issueFailureReasons: {},
    issuePublished: 0,
    labelComplete: 0,
    labelUnsupported: 0,
  };
  for (const event of events) {
    // wait in at most hourly sleeps and never poll faster than the capture cadence
    while (clockNow(clock) < Date.parse(event.at)) {
      await sleep(Math.min(HOUR_MS, Date.parse(event.at) - clockNow(clock)));
    }
    if (event.kind === "issue") {
      try {
        const issue = await captureIssue({
          baseUrl, clock, fetcher, panel, root, scheduledAt: event.value, sleep,
        });
        // count only immutable publications or verified idempotent members
        if (issue !== null) {
          result.issuePublished += 1;
        }
      } catch (error) {
        // continue only after an expected slot failure proves its durable exact witness
        if (!(error instanceof IssueSlotCaptureError)) {
          throw error;
        }
        validateIssueFailureWitness(
          error.witness,
          panel,
          event.value,
          issueSnapshotId(event.value),
        );
        result.issueFailed += 1;
        const reason = boundedReason(error.witness.reasonCode);
        result.issueFailureReasons[reason] = (result.issueFailureReasons[reason] ?? 0) + 1;
      }
    } else {
      const labels = await sealLabels({
        baseUrl, clock, fetcher, panel, root, sleep, targetDate: event.value,
      });
      // terminal unsupported attempts remain honest panel members
      if (labels.status === "complete") {
        result.labelComplete += 1;
      } else {
        result.labelUnsupported += 1;
      }
    }
  }
  return Object.freeze({
    ...result,
    issueFailureReasons: Object.freeze({ ...result.issueFailureReasons }),
  });
}

// normalize one current or forecast response envelope
function normalizeWeatherEnvelope(value, expectedSite, cutoffAt, {
  availableAt,
  current,
  maximumRows,
  sourceCatalog = null,
}) {
  requireObject(value, "weather envelope");
  validateDiscoveredSite(value.site, expectedSite);
  const generatedAt = requireCanonicalInstant(value.generatedAt, "weather generatedAt");
  // an envelope generated after assembly is not causal input
  if (Date.parse(generatedAt) > Date.parse(cutoffAt)) {
    throw new RangeError("weather envelope is generated after cutoff");
  }
  requireArray(value.data, "weather data", 0, maximumRows);
  const rows = value.data.flatMap((row) => {
    // sanitize every row through the same complete dto projection
    const normalized = normalizeLocalForecastWeatherRecord(row, cutoffAt, { availableAt, current });
    // every retained physical anchor must match its complete discovered source tuple
    if (current && normalized.provenance.sourceKind === "physical_sensor") {
      const knownSource = sourceCatalog.some(
        // unlisted legacy physical rows are ignored before browser inference
        (source) => source.sourceId === normalized.provenance.sourceId,
      );
      if (!knownSource) {
        return [];
      }
      validateLocalForecastPhysicalSource(
        normalized.provenance,
        sourceCatalog,
        "current physical source",
      );
    }
    return current && isPropertyOnlyCurrentRecord(row, normalized) ? [] : [normalized];
  });
  return Object.freeze({ generatedAt, rows: Object.freeze(rows) });
}

// identify private property-only current rows after the complete dto projection
function isPropertyOnlyCurrentRecord(original, normalized) {
  const propertyEvidence = original.metrics?.soilElectricalConductivityMicrosiemensPerCm != null ||
    original.metrics?.soilMoisturePercent != null ||
    (Array.isArray(original.metadata?.provider?.propertySensors) &&
      original.metadata.provider.propertySensors.length > 0);
  // retain every row needed by the actual uv pressure ws90 humidity or pm inference paths
  const inferenceEvidence = [
    normalized.metrics.uvIndex,
    normalized.metrics.pressureHpa,
    normalized.metrics.solarRadiationWm2,
    normalized.metrics.relativeHumidityPercent,
    normalized.metrics.pm25MicrogramsPerCubicMeter,
    normalized.pressureChange3hHpa,
  ].some((value) => value !== null && value !== undefined);
  return propertyEvidence && !inferenceEvidence;
}

// normalize an absent or exact six-row forecast pressure context
function normalizePressureContext(value, cutoffAt, availableAt) {
  // incomplete public context is the explicit absent fallback
  if (!Array.isArray(value) || value.length !== 6) {
    return Object.freeze([]);
  }
  let rows;
  try {
    rows = value.map(
      // remove unrelated adjustment metadata from every context row
      (row) => normalizeLocalForecastWeatherRecord(row, cutoffAt, { availableAt }),
    ).toSorted((left, right) => left.validAt.localeCompare(right.validAt));
  } catch {
    return Object.freeze([]);
  }
  const first = rows[0];
  const valid = first !== undefined && first.provenance.sourceKind === "forecast" &&
    first.productRunAt !== null && rows.every(
      // require exact same-source same-run hourly pressure geometry
      (row, index) => row.provenance.sourceKind === "forecast" &&
        row.provenance.sourceId === first.provenance.sourceId &&
        row.productRunAt === first.productRunAt && row.metrics.pressureHpa !== null &&
        Date.parse(row.validAt) === Date.parse(first.validAt) + index * HOUR_MS,
    );
  return Object.freeze(valid ? rows : []);
}

// normalize the compact queryless daylight evidence envelope
function normalizeSolarCloudEnvelope(value, cutoffAt, availableAt, sourceCatalog) {
  requireObject(value, "solar cloud envelope");
  const generatedAt = requireCanonicalInstant(value.generatedAt, "solar generatedAt");
  // generation cannot postdate the assembled issue
  if (Date.parse(generatedAt) > Date.parse(cutoffAt) ||
      value.contractVersion !== "solar-cloud-evidence/v1") {
    throw new RangeError("solar cloud envelope is invalid");
  }
  // null is a complete explicit raw fallback
  if (value.evidence === null) {
    return Object.freeze({ evidence: null, generatedAt });
  }
  requireObject(value.evidence, "solar cloud evidence");
  const evaluatedAt = requireCanonicalInstant(value.evidence.evaluatedAt, "solar evaluatedAt");
  const ws90 = normalizeSolarSample(value.evidence.ws90, "physical_sensor", availableAt);
  const regional = normalizeSolarSample(value.evidence.regional, "model_current", availableAt);
  validateLocalForecastWs90Source(ws90, sourceCatalog);
  // the retained pair evaluation is its final source receipt
  if (Date.parse(evaluatedAt) !== Math.max(Date.parse(ws90.receivedAt), Date.parse(regional.receivedAt)) ||
      Date.parse(evaluatedAt) > Date.parse(cutoffAt)) {
    throw new RangeError("solar cloud evidence timing is invalid");
  }
  return Object.freeze({
    evidence: Object.freeze({ evaluatedAt, regional, ws90 }),
    generatedAt,
  });
}

// normalize one compact retained solar input sample
function normalizeSolarSample(value, sourceKind, cutoffAt) {
  requireObject(value, "solar sample");
  const validAt = requireCanonicalInstant(value.validAt, "solar validAt");
  const receivedAt = requireCanonicalInstant(value.receivedAt, "solar receivedAt");
  // retained samples must already exist by the issue cutoff
  if (Date.parse(receivedAt) > Date.parse(cutoffAt) || Date.parse(receivedAt) < Date.parse(validAt)) {
    throw new RangeError("solar sample timing is invalid");
  }
  return Object.freeze({
    providerKey: requireString(value.providerKey, "solar providerKey", 1, 128),
    receivedAt,
    recordId: requireString(value.recordId, "solar recordId", 1, 512),
    sourceId: requireString(value.sourceId, "solar sourceId", 1, 512),
    sourceKey: requireString(value.sourceKey, "solar sourceKey", 1, 256),
    sourceKind: requireEnum(value.sourceKind, [sourceKind], "solar sourceKind"),
    stationSlug: requireString(value.stationSlug, "solar stationSlug", 1, 256),
    validAt,
    value: requireNumber(value.value, "solar value", -1_000, 3_000),
  });
}

// normalize one source-specific public history page
function normalizeHistoryPage(value, expectedSite, sourceId, schedule, sourceCatalog) {
  const sourceAllowlist = sourceCatalog.map(
    // derive exact query ids from the complete frozen physical catalog
    (source) => source.sourceId,
  );
  requireObject(value, "history page");
  validateDiscoveredSite(value.site, expectedSite);
  const generatedAt = requireCanonicalInstant(value.generatedAt, "history generatedAt");
  requireArray(value.data, "history data", 0, HISTORY_LIMIT);
  requireObject(value.page, "history page metadata");
  // the response must echo the exact requested public page limit
  if (value.page.limit !== HISTORY_LIMIT ||
      (value.page.nextCursor !== null &&
        (typeof value.page.nextCursor !== "string" || value.page.nextCursor.length === 0 ||
          value.page.nextCursor.length > 4_096))) {
    throw new RangeError("history pagination is invalid");
  }
  const rows = value.data.flatMap((row) => {
    // audit query identity before excluding a late latest revision
    requireObject(row, "history row");
    requireObject(row.provenance, "history row provenance");
    const validAt = requireCanonicalInstant(row.validAt, "history validAt");
    const receivedAt = requireCanonicalInstant(row.receivedAt, "history receivedAt");
    if (row.provenance.sourceKind !== "physical_sensor" ||
        row.provenance.sourceId !== sourceId || !sourceAllowlist.includes(sourceId) ||
        Date.parse(validAt) < Date.parse(schedule.from) ||
        Date.parse(validAt) >= Date.parse(schedule.to)) {
      throw new RangeError("history row differs from its declared query");
    }
    // the public dto exposes only the latest revision so late values cannot reconstruct an earlier one
    const normalized = normalizeLocalForecastWeatherRecord(
      row,
      Date.parse(receivedAt) > Date.parse(schedule.sealAt) ? receivedAt : schedule.sealAt,
    );
    validateLocalForecastPhysicalSource(
      normalized.provenance,
      sourceCatalog,
      "history physical source",
    );
    if (Date.parse(receivedAt) > Date.parse(schedule.sealAt)) {
      return [];
    }
    return [normalized];
  });
  for (const row of rows) {
    // source-specific queries cannot return another tier or a halo escape
    if (row.provenance.sourceKind !== "physical_sensor" ||
        row.provenance.sourceId !== sourceId || !sourceAllowlist.includes(sourceId) ||
        Date.parse(row.validAt) < Date.parse(schedule.from) ||
        Date.parse(row.validAt) >= Date.parse(schedule.to)) {
      throw new RangeError("history row differs from its declared query");
    }
  }
  return Object.freeze({
    generatedAt,
    nextCursor: value.page.nextCursor,
    rows: Object.freeze(rows),
  });
}

// preserve one exact product response receipt
function issueReceipt(kind, response, generatedAt) {
  // product generation must precede the completed response receipt
  if (Date.parse(generatedAt) > Date.parse(response.receivedAt)) {
    throw new RangeError("issue response receipt precedes generation");
  }
  return Object.freeze({
    envelopeSha256: response.envelopeSha256,
    generatedAt,
    kind,
    receivedAt: response.receivedAt,
  });
}

// publish one unsupported terminal label attempt without partial rows
async function publishUnsupportedLabels(
  captureRoot,
  finalPath,
  panel,
  schedule,
  receipts,
  reasonCode,
) {
  const safeReceipts = receipts.filter(
    // retain only receipts completed inside the declared attempt deadline
    (receipt) => Date.parse(receipt.receivedAt) >= Date.parse(schedule.sealAt) &&
      Date.parse(receipt.receivedAt) <= Date.parse(schedule.sealAt) + LABEL_DEADLINE_MS,
  );
  const unsigned = {
    contractVersion: "weather-local-correction-labels/v1",
    from: schedule.from,
    normalizedDigest: "",
    panelDigest: panel.panelDigest,
    reasonCode: boundedReason(reasonCode),
    receipts: safeReceipts,
    rows: [],
    sealAt: schedule.sealAt,
    sourceAllowlist: panel.sourceAllowlist,
    status: "unsupported",
    targetDate: schedule.targetDate,
    to: schedule.to,
  };
  const labels = { ...unsigned, normalizedDigest: labelSealDigest(unsigned) };
  validateLocalForecastLabelSeal(labels);
  await publishJson(captureRoot, finalPath, labels, LABEL_MEMBER_BYTES);
  return labels;
}

// validate one burned label claim against the exact frozen panel and schedule
function validateLabelAttemptClaim(claim, panel, schedule) {
  requireExactKeys(claim, [
    "contractVersion", "panelDigest", "sealAt", "startedAt", "targetDate", "type",
  ], "label attempt claim");
  requireCanonicalInstant(claim.startedAt, "label claim startedAt");
  // another panel schedule or claim kind cannot authorize recovery publication
  if (claim.contractVersion !== LOCAL_FORECAST_CORRECTION_CAPTURE_VERSION ||
      claim.panelDigest !== panel.panelDigest || claim.sealAt !== schedule.sealAt ||
      claim.targetDate !== schedule.targetDate || claim.type !== "label_attempt" ||
      Date.parse(claim.startedAt) < Date.parse(schedule.sealAt)) {
    throw new RangeError("label attempt claim differs from the frozen panel");
  }
  return claim;
}

// validate one burned issue claim against the exact frozen panel and schedule
function validateIssueAttemptClaim(claim, panel, scheduledAt, snapshotId) {
  requireExactKeys(claim, [
    "contractVersion", "panelDigest", "scheduledAt", "snapshotId", "startedAt", "type",
  ], "issue attempt claim");
  requireCanonicalInstant(claim.startedAt, "issue claim startedAt");
  // another panel schedule or claim kind cannot occupy this issue identity
  if (claim.contractVersion !== LOCAL_FORECAST_CORRECTION_CAPTURE_VERSION ||
      claim.panelDigest !== panel.panelDigest || claim.scheduledAt !== scheduledAt ||
      claim.snapshotId !== snapshotId || claim.type !== "issue_attempt" ||
      Date.parse(claim.startedAt) < Date.parse(scheduledAt)) {
    throw new RangeError("issue attempt claim differs from the frozen panel");
  }
  return claim;
}

// identify bounded transport or public-evidence failures eligible for slot continuation
function isExpectedIssueEvidenceError(error) {
  return error instanceof RetryableCaptureError || error instanceof TerminalCaptureError ||
    error instanceof RangeError || error instanceof TypeError;
}

// validate one durable issue failure witness before the finite runner continues
function validateIssueFailureWitness(failure, panel, scheduledAt, snapshotId) {
  requireExactKeys(failure, [
    "contractVersion", "panelDigest", "reasonCode", "scheduledAt", "snapshotId",
    "startedAt", "type",
  ], "issue failure witness");
  requireCanonicalInstant(failure.startedAt, "issue failure startedAt");
  requireString(failure.reasonCode, "issue failure reasonCode", 1, 128);
  // only this exact frozen panel and scheduled slot can authorize continuation
  if (failure.contractVersion !== LOCAL_FORECAST_CORRECTION_CAPTURE_VERSION ||
      failure.panelDigest !== panel.panelDigest || failure.scheduledAt !== scheduledAt ||
      failure.snapshotId !== snapshotId || failure.type !== "issue_failure" ||
      Date.parse(failure.startedAt) < Date.parse(scheduledAt)) {
    throw new RangeError("issue failure witness differs from the frozen panel");
  }
  return failure;
}

// record one burned issue slot without exposing provider response content
async function writeIssueFailure(
  captureRoot,
  snapshotId,
  panelDigest,
  scheduledAt,
  startedAt,
  reasonCode,
) {
  const path = join(captureRoot, "attempts", "issues", `${snapshotId}.failed.json`);
  const existing = await readOptionalJson(path, 64 * 1_024);
  // never replace a prior terminal gap witness
  if (existing !== null) {
    return existing;
  }
  const failure = {
    contractVersion: LOCAL_FORECAST_CORRECTION_CAPTURE_VERSION,
    panelDigest,
    reasonCode: boundedReason(reasonCode),
    scheduledAt,
    snapshotId,
    startedAt,
    type: "issue_failure",
  };
  await writeExclusiveJson(path, failure);
  return failure;
}

// construct one source-specific opaque-cursor history url
function historyUrl(baseUrl, siteSlug, sourceId, from, to, cursor) {
  const url = new URL(`${baseUrl}/sites/${siteSlug}/history`);
  url.searchParams.set("from", from);
  url.searchParams.set("limit", String(HISTORY_LIMIT));
  url.searchParams.set("source", sourceId);
  url.searchParams.set("sourceKind", "physical_sensor");
  url.searchParams.set("to", to);
  // carry only a server-issued opaque cursor
  if (cursor !== null) {
    url.searchParams.set("cursor", cursor);
  }
  return url.href;
}

// normalize safe public metadata while removing serial grid and property sensors
function normalizeMetadata(value) {
  requireObject(value, "weather metadata");
  const device = value.device === null ? null : {
    model: requireNullableString(value.device?.model, "device model", 128),
    serial: null,
    vendor: requireNullableString(value.device?.vendor, "device vendor", 128),
  };
  const provider = value.provider === null ? null : {
    dataset: requireNullableString(value.provider?.dataset, "provider dataset", 256),
    elevationM: requireNullableNumber(value.provider?.elevationM, "provider elevation", -1_000, 10_000),
    gridCell: null,
    propertySensors: null,
  };
  const quality = value.quality === null ? null : {
    confidencePercent: requireNullableNumber(value.quality?.confidencePercent, "quality confidence", 0, 100),
    flags: normalizeStringList(value.quality?.flags, "quality flags", 20, 128),
    interpolation: requireNullableString(value.quality?.interpolation, "quality interpolation", 128),
    status: requireNullableString(value.quality?.status, "quality status", 128),
  };
  requireObject(value.upstream, "upstream metadata");
  return Object.freeze({
    device: device === null ? null : Object.freeze(device),
    provider: provider === null ? null : Object.freeze(provider),
    quality: quality === null ? null : Object.freeze(quality),
    upstream: Object.freeze({
      model: requireNullableString(value.upstream.model, "upstream model", 128),
      timezone: requireString(value.upstream.timezone, "upstream timezone", 1, 128),
    }),
  });
}

// normalize one public attribution pair
function normalizeAttribution(value) {
  requireObject(value, "attribution");
  const url = requireString(value.url, "attribution url", 1, 2_048);
  // attribution links remain public http or https metadata only
  if (!/^https?:\/\//u.test(url)) {
    throw new TypeError("attribution url is invalid");
  }
  return Object.freeze({
    label: requireString(value.label, "attribution label", 1, 256),
    url,
  });
}

// fetch one response under a hard wall-clock abort
async function fetchWithTimeout(fetcher, url, timeoutMs, consumeResponse) {
  // a nonpositive remaining deadline is already terminal
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new RetryableCaptureError("public request timeout expired");
  }
  const abort = new AbortController();
  let timeout;
  let response = null;
  try {
    const deadline = new Promise((_resolve, reject) => {
      // abort headers and body then reject even when an injected stream ignores its signal
      timeout = setTimeout(() => {
        abort.abort();
        response?.body?.cancel().catch(() => {});
        reject(new RetryableCaptureError("public request timed out"));
      }, timeoutMs);
    });
    const request = (async () => {
      try {
        response = await fetcher(url, {
          headers: { accept: "application/json" },
          redirect: "manual",
          signal: abort.signal,
        });
      } catch (error) {
        throw new RetryableCaptureError(
          error instanceof Error ? error.message : "public request transport failed",
        );
      }
      return consumeResponse(response);
    })();
    return await Promise.race([
      request,
      deadline,
    ]);
  } finally {
    clearTimeout(timeout);
  }
}

// read a response stream without allocating beyond its decoded byte ceiling
async function readBoundedResponse(response, maximumBytes) {
  const declared = response.headers.get("content-length");
  // reject declared oversize bodies before reading them
  if (declared !== null && (!/^\d+$/u.test(declared) || Number(declared) > maximumBytes)) {
    throw new TerminalCaptureError("public response byte limit exceeded");
  }
  // fallback responses without a stream remain bounded after decoding
  if (response.body === null) {
    const bytes = Buffer.from(await response.arrayBuffer());
    if (bytes.length === 0 || bytes.length > maximumBytes) {
      throw new TerminalCaptureError("public response byte limit exceeded");
    }
    return bytes;
  }
  const reader = response.body.getReader();
  const chunks = [];
  let length = 0;
  while (true) {
    // advance the body one bounded chunk at a time
    const part = await reader.read();
    if (part.done) {
      break;
    }
    length += part.value.byteLength;
    if (length > maximumBytes) {
      await reader.cancel().catch(() => {});
      throw new TerminalCaptureError("public response byte limit exceeded");
    }
    chunks.push(Buffer.from(part.value));
  }
  // empty public envelopes are invalid
  if (length === 0) {
    throw new TerminalCaptureError("public response byte limit exceeded");
  }
  return Buffer.concat(chunks, length);
}

// create all fixed private capture subdirectories
async function prepareCaptureLayout(root) {
  const captureRoot = await prepareLocalForecastCaptureRoot(root);
  for (const path of [
    join(captureRoot, "attempts"),
    join(captureRoot, "attempts", "issues"),
    join(captureRoot, "attempts", "labels"),
    join(captureRoot, "issues"),
    join(captureRoot, "labels"),
    join(captureRoot, "tmp"),
  ]) {
    // materialize only the closed capture layout
    await prepareLocalForecastCaptureRoot(path);
  }
  return captureRoot;
}

// create one exclusive per-attempt staging directory
async function createStage(captureRoot, prefix) {
  const temporaryRoot = join(captureRoot, "tmp");
  await prepareLocalForecastCaptureRoot(temporaryRoot);
  const stage = await mkdtemp(join(temporaryRoot, prefix));
  await verifyPrivateDirectory(stage);
  return stage;
}

// write one normalized staging member durably and exclusively
async function writeStageJson(stage, name, value) {
  const path = join(stage, name);
  await writeExclusiveJson(path, value);
  return path;
}

// write one exclusive private json member and flush its parent
async function writeExclusiveJson(path, value) {
  await prepareLocalForecastCaptureRoot(dirname(path));
  const bytes = canonicalJsonBytes(value);
  let handle;
  try {
    handle = await open(path, "wx", 0o600);
    await handle.writeFile(bytes);
    await handle.sync();
  } catch (error) {
    // an existing attempt claim permanently burns its slot
    if (error?.code === "EEXIST") {
      throw new RangeError("capture attempt is already burned");
    }
    throw error;
  } finally {
    await handle?.close();
  }
  await syncDirectory(dirname(path));
  return bytes.length;
}

// publish one value through an exclusive normalized staging member
async function publishJson(captureRoot, finalPath, value, maximumBytes) {
  const stage = await createStage(captureRoot, "publish-");
  try {
    const stagedPath = join(stage, "member.json");
    await writeStageJson(stage, "member.json", value);
    await publishStagedJson(captureRoot, stagedPath, finalPath, maximumBytes);
  } finally {
    await rm(stage, { force: true, recursive: true });
  }
}

// atomically publish one staged member under the exclusive capture lock
async function publishStagedJson(captureRoot, stagedPath, finalPath, maximumBytes) {
  const staged = await lstat(stagedPath);
  // only bounded regular normalized members may be published
  if (!staged.isFile() || staged.isSymbolicLink() || staged.size === 0 || staged.size > maximumBytes) {
    throw new RangeError("normalized evidence member byte limit exceeded");
  }
  const lockPath = join(captureRoot, ".publication.lock");
  let lock;
  try {
    lock = await open(lockPath, "wx", 0o600);
    await lock.writeFile(canonicalJsonBytes({
      contractVersion: LOCAL_FORECAST_CORRECTION_CAPTURE_VERSION,
      pid: process.pid,
    }));
    await lock.sync();
  } catch (error) {
    await lock?.close();
    // never break or replace a foreign publication lock
    if (error?.code === "EEXIST") {
      throw new RangeError("capture publication lock is held");
    }
    throw error;
  }
  try {
    await prepareLocalForecastCaptureRoot(dirname(finalPath));
    const existing = await readOptionalBytes(finalPath, maximumBytes);
    // an identical existing member is idempotent while conflicts never overwrite
    if (existing !== null) {
      const stagedBytes = await readFile(stagedPath);
      if (!existing.equals(stagedBytes)) {
        throw new RangeError("immutable capture member conflicts");
      }
      return;
    }
    const admitted = await normalizedEvidenceBytes(captureRoot);
    if (admitted + staged.size > TOTAL_EVIDENCE_BYTES) {
      throw new RangeError("normalized evidence budget exhausted");
    }
    await rename(stagedPath, finalPath);
    await syncDirectory(dirname(finalPath));
  } finally {
    await lock.close();
    await unlink(lockPath);
    await syncDirectory(captureRoot);
  }
}

// count only immutable panel issue and label evidence toward admission
async function normalizedEvidenceBytes(captureRoot) {
  let total = 0;
  for (const path of [
    join(captureRoot, "panel.json"),
    join(captureRoot, "issues"),
    join(captureRoot, "labels"),
  ]) {
    // absent members contribute no admitted evidence
    const metadata = await lstat(path).catch((error) => error?.code === "ENOENT" ? null : Promise.reject(error));
    if (metadata === null) {
      continue;
    }
    if (metadata.isSymbolicLink()) {
      throw new RangeError("normalized evidence contains a symbolic link");
    }
    if (metadata.isFile()) {
      total += metadata.size;
      continue;
    }
    if (!metadata.isDirectory()) {
      throw new RangeError("normalized evidence contains a special file");
    }
    const entries = await readdir(path, { withFileTypes: true });
    for (const entry of entries) {
      // immutable evidence directories accept immediate json members only
      if (!entry.isFile() || !entry.name.endsWith(".json")) {
        throw new RangeError("normalized evidence directory contains a foreign member");
      }
      const member = await lstat(join(path, entry.name));
      if (member.isSymbolicLink() || !member.isFile()) {
        throw new RangeError("normalized evidence member is invalid");
      }
      total += member.size;
    }
  }
  return total;
}

// read one optional immutable json member without following a symlink
async function readOptionalJson(path, maximumBytes) {
  const bytes = await readOptionalBytes(path, maximumBytes);
  // preserve an absent member distinctly
  if (bytes === null) {
    return null;
  }
  try {
    return JSON.parse(bytes.toString("utf8"));
  } catch {
    throw new TypeError("existing capture member is invalid json");
  }
}

// read one optional bounded regular file
async function readOptionalBytes(path, maximumBytes) {
  const metadata = await lstat(path).catch((error) => error?.code === "ENOENT" ? null : Promise.reject(error));
  // missing is the only nonerror absence
  if (metadata === null) {
    return null;
  }
  if (metadata.isSymbolicLink() || !metadata.isFile() || metadata.size === 0 || metadata.size > maximumBytes) {
    throw new RangeError("existing capture member is invalid");
  }
  return readFile(path);
}

// create or verify one exact real private directory
async function ensurePrivateDirectory(path) {
  await mkdir(path, { mode: 0o700, recursive: true });
  await verifyPrivateDirectory(path);
}

// reject symlink or canonical-path drift for one directory
async function verifyPrivateDirectory(path) {
  const metadata = await lstat(path);
  const canonical = await realpath(path);
  // capture never traverses symlinked or noncanonical ancestors
  if (metadata.isSymbolicLink() || !metadata.isDirectory() || canonical !== resolve(path)) {
    throw new RangeError("capture path contains a symbolic link or non-directory");
  }
}

// flush one directory after exclusive publication changes
async function syncDirectory(path) {
  const handle = await open(path, "r");
  try {
    await handle.sync();
  } finally {
    await handle.close();
  }
}

// validate the configured panel site projection
function validateCaptureSite(site) {
  requireExactKeys(site, ["latitude", "longitude", "siteKey", "timeZone"], "capture site");
  const normalized = {
    latitude: requireNumber(site.latitude, "site latitude", -90, 90),
    longitude: requireNumber(site.longitude, "site longitude", -180, 180),
    siteKey: requireString(site.siteKey, "site key", 1, 128),
    timeZone: requireString(site.timeZone, "site timeZone", 1, 128),
  };
  try {
    new Intl.DateTimeFormat("en", { timeZone: normalized.timeZone }).format(0);
  } catch {
    throw new RangeError("site timeZone is invalid");
  }
  return normalized;
}

// validate a discovered api site against frozen panel geometry
function validateDiscoveredSite(site, expectedSite) {
  requireObject(site, "discovered site");
  const expected = validateCaptureSite(expectedSite);
  // reject site key timezone or geometry drift
  if (site.slug !== expected.siteKey || site.timezone !== expected.timeZone ||
      site.latitude !== expected.latitude || site.longitude !== expected.longitude) {
    throw new RangeError("discovered site differs from the panel");
  }
  requireArray(site.stations, "site stations", 0, 512);
  return site;
}

// verify the command base url remains bound to the frozen input hash
function verifyPanelOrigin(panel, baseUrl) {
  const expected = localForecastCaptureInputHash(baseUrl, panel.site, panel.sourceAllowlist);
  // another api origin cannot write into this panel
  if (panel.inputHash !== expected) {
    throw new RangeError("capture origin differs from the frozen panel");
  }
}

// validate one canonical sorted public source id set
function validateSourceAllowlist(value) {
  requireArray(value, "source allowlist", 1, 128);
  const sorted = [...new Set(value)].toSorted();
  // history source ids are positive public ids in canonical order
  if (sorted.length !== value.length || sorted.some((entry, index) =>
    entry !== value[index] || !SOURCE_ID.test(entry))) {
    throw new TypeError("source allowlist is invalid");
  }
}

// normalize one strict https api base without credentials query or fragment
function normalizeBaseUrl(value) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new TypeError("capture base url is invalid");
  }
  // the off-host collector never sends credentials or follows another protocol
  if (parsed.protocol !== "https:" || parsed.username !== "" || parsed.password !== "" ||
      parsed.search !== "" || parsed.hash !== "" || parsed.pathname === "/" ||
      parsed.pathname.endsWith("/")) {
    throw new RangeError("capture base url must be one https api prefix");
  }
  return parsed.href.replace(/\/$/u, "");
}

// create one stable issue snapshot identifier from its frozen schedule
function issueSnapshotId(scheduledAt) {
  return scheduledAt.replace(/[:.]/gu, "-");
}

// choose the latest actual receipt time
function maximumInstant(values) {
  return new Date(Math.max(...values.map(
    // parse every receipt through the canonical instant boundary
    (value) => requireInstant(value, "receipt instant"),
  ))).toISOString();
}

// compare physical rows by time source receipt and identity
function compareWeatherRows(left, right) {
  return left.validAt.localeCompare(right.validAt) ||
    left.provenance.sourceId.localeCompare(right.provenance.sourceId) ||
    left.receivedAt.localeCompare(right.receivedAt) || left.id.localeCompare(right.id);
}

// reduce one internal failure to a bounded nonsecret terminal reason
function captureReason(error) {
  const message = error instanceof Error ? error.message : "capture_failed";
  return boundedReason(message.toLowerCase().replace(/[^a-z0-9]+/gu, "_").replace(/^_+|_+$/gu, ""));
}

// retain one bounded stable terminal reason code
function boundedReason(value) {
  const normalized = typeof value === "string" ? value.slice(0, 128) : "capture_failed";
  return normalized === "" ? "capture_failed" : normalized;
}

// identify native network failures without retrying validation failures
function isNetworkError(error) {
  return error instanceof TypeError && !(error instanceof TerminalCaptureError);
}

// read one injected clock as a finite unix millisecond value
function clockNow(clock) {
  const value = typeof clock?.now === "function" ? clock.now() : Number.NaN;
  const milliseconds = value instanceof Date ? value.getTime() : value;
  // capture timing cannot fall back to an implicit wall clock
  if (!Number.isFinite(milliseconds)) {
    throw new TypeError("capture clock is invalid");
  }
  return milliseconds;
}

// provide the real capture clock
const defaultClock = Object.freeze({ now: () => Date.now() });

// sleep one nonnegative bounded interval
function defaultSleep(milliseconds) {
  // callers cannot create negative or unbounded timers
  if (!Number.isFinite(milliseconds) || milliseconds < 0 || milliseconds > HOUR_MS) {
    throw new RangeError("capture sleep is invalid");
  }
  return new Promise((resolveSleep) => setTimeout(resolveSleep, milliseconds));
}

// require one nonarray object
function requireObject(value, name) {
  // reject null arrays and primitives
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} is invalid`);
  }
  return value;
}

// require one bounded array
function requireArray(value, name, minimum, maximum) {
  // reject missing or resource-unbounded collections
  if (!Array.isArray(value) || value.length < minimum || value.length > maximum) {
    throw new RangeError(`${name} is invalid`);
  }
  return value;
}

// require one exact closed object field set
function requireExactKeys(value, keys, name) {
  requireObject(value, name);
  const actual = Object.keys(value).toSorted();
  const expected = [...keys].toSorted();
  // reject missing or silently widened capture configuration
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new TypeError(`${name} fields are invalid`);
  }
}

// require one bounded nonblank string
function requireString(value, name, minimum, maximum) {
  // reject control-bearing or oversized public strings
  if (typeof value !== "string" || value.trim().length < minimum || value.length > maximum ||
      /[\u0000-\u001f\u007f]/u.test(value)) {
    throw new TypeError(`${name} is invalid`);
  }
  return value;
}

// require one bounded optional string
function requireNullableString(value, name, maximum) {
  return value === null ? null : requireString(value, name, 1, maximum);
}

// require one member of a closed literal set
function requireEnum(value, allowed, name) {
  // reject unknown public enumerations
  if (!allowed.includes(value)) {
    throw new TypeError(`${name} is invalid`);
  }
  return value;
}

// require one finite bounded number
function requireNumber(value, name, minimum, maximum) {
  // reject nan infinity and out-of-domain values
  if (!Number.isFinite(value) || value < minimum || value > maximum) {
    throw new RangeError(`${name} is invalid`);
  }
  return value;
}

// require one bounded optional number
function requireNullableNumber(value, name, minimum, maximum) {
  return value === null ? null : requireNumber(value, name, minimum, maximum);
}

// require one bounded integer
function requireInteger(value, name, minimum, maximum) {
  // reject fractional and out-of-contract counts
  if (!Number.isInteger(value) || value < minimum || value > maximum) {
    throw new RangeError(`${name} is invalid`);
  }
  return value;
}

// require one canonical utc instant string
function requireCanonicalInstant(value, name) {
  requireInstant(value, name);
  return value;
}

// parse one canonical utc instant
function requireInstant(value, name) {
  // reject offsets and precision aliases in immutable evidence
  if (typeof value !== "string" || !INSTANT.test(value) || new Date(value).toISOString() !== value) {
    throw new TypeError(`${name} is invalid`);
  }
  return Date.parse(value);
}

// normalize one bounded optional public string list
function normalizeStringList(value, name, maximumItems, maximumLength) {
  // preserve an explicit absent quality list
  if (value === null) {
    return null;
  }
  requireArray(value, name, 0, maximumItems);
  return Object.freeze(value.map(
    // retain only bounded strings in original public order
    (entry) => requireString(entry, name, 1, maximumLength),
  ));
}

// mark retryable transport failures separately from evidence validation
class RetryableCaptureError extends Error {}

// mark terminal public contract failures
class TerminalCaptureError extends Error {}

// mark one expected per-slot capture failure with its durable terminal witness
class IssueSlotCaptureError extends Error {
  // retain only the validated bounded witness needed by the finite runner
  constructor(error, witness) {
    super(error instanceof Error ? error.message : "issue capture failed");
    this.name = "IssueSlotCaptureError";
    this.witness = witness;
  }
}

// parse one closed command line into string options
function parseArguments(argumentsList) {
  const [command, ...members] = argumentsList;
  // accept only the finite capture lifecycle commands
  if (![
    "init-panel", "capture-issue", "seal-label", "run-finite",
  ].includes(command)) {
    throw new TypeError("capture command is invalid");
  }
  const options = {};
  for (let index = 0; index < members.length; index += 2) {
    // require one value for every named option
    const key = members[index];
    const value = members[index + 1];
    if (typeof key !== "string" || !key.startsWith("--") || value === undefined) {
      throw new TypeError("capture option is invalid");
    }
    const name = key.slice(2);
    if (Object.hasOwn(options, name)) {
      throw new TypeError(`duplicate capture option: ${key}`);
    }
    options[name] = value;
  }
  const required = command === "init-panel"
    ? ["input", "origin", "root"]
    : command === "capture-issue"
      ? ["origin", "panel", "root", "scheduled-at"]
      : command === "seal-label"
        ? ["origin", "panel", "root", "target-date"]
        : ["origin", "panel", "root"];
  const allowed = new Set(required);
  for (const name of Object.keys(options)) {
    // reject every unreviewed cli field
    if (!allowed.has(name)) {
      throw new TypeError(`unknown capture option: --${name}`);
    }
  }
  for (const name of required) {
    // reject implicit paths origins and schedule identities
    if (typeof options[name] !== "string" || options[name] === "") {
      throw new TypeError(`missing capture option: --${name}`);
    }
  }
  return { command, options };
}

// read one bounded private configuration or panel file
async function readPrivateJson(path, maximumBytes) {
  const absolute = resolve(path);
  const location = relative(privateEvidenceRoot, absolute);
  // private capture inputs cannot be read through repository-visible paths
  if (location === "" || location === ".." || location.startsWith(`..${sep}`)) {
    throw new RangeError("capture input must be inside .weather-data");
  }
  await verifyPrivateAncestors(dirname(absolute));
  const bytes = await readOptionalBytes(absolute, maximumBytes);
  if (bytes === null) {
    throw new RangeError("capture input is absent");
  }
  try {
    return JSON.parse(bytes.toString("utf8"));
  } catch {
    throw new TypeError("capture input is invalid json");
  }
}

// verify every existing private input ancestor without creating it
async function verifyPrivateAncestors(path) {
  const absolute = resolve(path);
  const location = relative(privateEvidenceRoot, absolute);
  // input ancestors must stay inside the private evidence root
  if (location === ".." || location.startsWith(`..${sep}`)) {
    throw new RangeError("capture input escaped .weather-data");
  }
  let cursor = privateEvidenceRoot;
  await verifyPrivateDirectory(cursor);
  for (const segment of location === "" ? [] : location.split(sep)) {
    // reject every symlinked input ancestor
    cursor = join(cursor, segment);
    await verifyPrivateDirectory(cursor);
  }
}

// load the checked remote target and reject oversized or malformed configuration
async function readConfiguredCliBaseUrl() {
  const bytes = await readFile(runtimeTargetsPath);
  // runtime target configuration remains a small reviewed repository member
  if (bytes.length === 0 || bytes.length > RUNTIME_TARGETS_BYTES) {
    throw new RangeError("runtime targets configuration is invalid");
  }
  let configuration;
  try {
    configuration = JSON.parse(bytes.toString("utf8"));
  } catch {
    throw new TypeError("runtime targets configuration is invalid json");
  }
  return configuredRemoteCaptureBaseUrl(configuration);
}

// execute one explicit finite capture command
async function main() {
  const { command, options } = parseArguments(process.argv.slice(2));
  const configuredBaseUrl = await readConfiguredCliBaseUrl();
  // the operational cli cannot be redirected to another https host or prefix
  if (normalizeBaseUrl(options.origin) !== configuredBaseUrl) {
    throw new RangeError("capture origin differs from configured remote runtime target");
  }
  if (command === "init-panel") {
    const root = await prepareCaptureLayout(options.root);
    const input = await readPrivateJson(options.input, RESPONSE_BYTES);
    const panel = await initializeLocalForecastCorrectionPanel({
      baseUrl: configuredBaseUrl,
      input,
    });
    const path = join(root, "panel.json");
    await publishJson(root, path, panel, RESPONSE_BYTES);
    process.stdout.write(`${JSON.stringify({ command, panel: path, state: "initialized" })}\n`);
    return;
  }
  const panel = await readPrivateJson(options.panel, RESPONSE_BYTES);
  validateLocalForecastCorrectionPanel(panel);
  if (command === "capture-issue") {
    const issue = await captureLocalForecastIssue({
      baseUrl: configuredBaseUrl,
      panel,
      root: options.root,
      scheduledAt: options["scheduled-at"],
    });
    process.stdout.write(`${JSON.stringify({ command, snapshotId: issue.snapshotId, state: "published" })}\n`);
    return;
  }
  if (command === "seal-label") {
    const labels = await sealLocalForecastLabels({
      baseUrl: configuredBaseUrl,
      panel,
      root: options.root,
      targetDate: options["target-date"],
    });
    process.stdout.write(`${JSON.stringify({ command, state: labels.status, targetDate: labels.targetDate })}\n`);
    return;
  }
  const result = await runFiniteLocalForecastCapture({
    baseUrl: configuredBaseUrl,
    panel,
    root: options.root,
  });
  process.stdout.write(`${JSON.stringify({ command, result, state: "finished" })}\n`);
}

const invokedPath = process.argv[1] === undefined ? null : pathToFileURL(resolve(process.argv[1])).href;
// execute only when invoked as the standalone finite collector
if (invokedPath === import.meta.url) {
  main().catch((error) => {
    // emit one bounded failure without response bodies or record metadata
    process.stderr.write(`${error instanceof Error ? boundedReason(error.message) : "capture_failed"}\n`);
    process.exitCode = 1;
  });
}
