import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { access, mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

import {
  createMaintenanceShadowPredictionMetadata,
  parseAdjustmentRevisionProjectionDocument,
  parseMaintenanceShadowComparator,
  parseMaintenanceShadowSourceProjection,
  parseMaintenanceShadowValues,
  validateMaintenanceShadowComparatorBinding,
} from "../../../../apps/web/src/adjustment-maintenance-contract.mjs";
import {
  AdjustmentRevisionArchiveStore,
  createAdjustmentRevisionArchiveHandler,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";
import { qualifiedWindPackage } from "./qualified-wind-package-fixture.mjs";

const HASH = "a".repeat(64);
const SECOND_HASH = "b".repeat(64);
const STAGE_AT = "2026-10-08T03:00:00.000Z";
const SOURCE_KEYS = ["contractVersion", "family", "registrationSha256", "candidateSha256",
  "sourceSha256", "dueKey", "issuedAt", "rowCount", "rows"];
const SOURCE_ROW_KEYS = {
  temperature: ["validAt", "leadHours", "modelLeadHours", "referenceAt", "receivedAt",
    "sourceSha256", "contentSha256", "adapterVersion", "dataset", "providerKey",
    "providerResponseSha256", "modelCycle", "upstreamModel", "rawTemperatureC64",
    "rawRelativeHumidityPercent64", "rawWindSpeedMps64", "bestMatchContentSha256",
    "bestMatchProductRunAt", "bestMatchSourceId", "bestMatchTemperatureC64"],
  wind: ["validAt", "leadHours", "modelLeadHours", "referenceAt", "receivedAt", "sourceSha256",
    "contentSha256", "adapterVersion", "contractEpoch", "dataset", "providerKey", "sourceKey",
    "sourceConfigFingerprint", "upstreamModel", "revisionCount", "windSpeedMps64", "windGustMps64"],
  rain: ["validAt", "leadHours", "modelLeadHours", "referenceAt", "receivedAt", "sourceSha256",
    "contentSha256", "adapterVersion", "contractEpoch", "dataset", "providerKey", "sourceKey",
    "sourceConfigFingerprint", "upstreamModel", "revisionCount", "precipitationMm64"],
};
const BODY_KEYS = ["contractVersion", "family", "registrationSha256", "candidateSha256",
  "sourceSha256", "dueKey", "issuedAt", "sourceReceiptSha256", "inputSha256", "rowCount", "rows"];
const BODY_ROW_KEYS = {
  temperature: ["validAt", "leadHours", "sourceRowSha256", "candidateTemperatureC64", "wouldApply", "fallbackCode"],
  wind: ["validAt", "leadHours", "sourceRowSha256", "candidateSpeedMps64", "candidateGustMps64", "speedWouldApply", "gustWouldApply"],
  rain: ["validAt", "leadHours", "sourceRowSha256", "occurrenceProbability64", "positiveAmountMm64",
    "candidatePrecipitationMm64", "atLeast1_0Probability64", "atLeast2_5Probability64", "wouldApply", "fallbackCode"],
};
const COMPARATOR_KEYS = ["contractVersion", "family", "registrationSha256", "candidateSha256",
  "sourceSha256", "dueKey", "issuedAt", "sourceProjectionSha256", "predictionBodySha256",
  "servingAuthority", "rowCount", "rows"];
const AUTHORITY_KEYS = ["artifactBase64", "artifactIdentitySha256", "artifactMemberSha256",
  "authorityKind", "receiptBase64", "receiptMemberSha256"];
const COMPARATOR_ROW_KEYS = {
  temperature: ["validAt", "leadHours", "sourceRowSha256", "incumbentTemperatureC64", "applied", "reasonCode"],
  wind: ["validAt", "leadHours", "sourceRowSha256", "incumbentSpeedMps64",
    "incumbentGustMps64", "speedApplied", "gustApplied", "reasonCode"],
  rain: ["validAt", "leadHours", "sourceRowSha256", "incumbentPrecipitationMm64",
    "occurrenceProbability64", "atLeast1_0Probability64", "atLeast2_5Probability64",
    "positiveAmountMm64", "applied", "reasonCode"],
};

// hash one exact fixture member
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// encode recursively sorted archive metadata with its required final newline
function canonicalBytes(value) {
  const normalize = (entry) => {
    if (entry === null || typeof entry !== "object") return entry;
    if (Array.isArray(entry)) return entry.map(normalize);
    return Object.fromEntries(Object.keys(entry).sort().map((key) => [key, normalize(entry[key])]));
  };
  return Buffer.from(`${JSON.stringify(normalize(value))}\n`);
}

// encode one finite IEEE-754 value in the original fixed lowercase representation
function binary64(value) {
  const bytes = Buffer.alloc(8);
  bytes.writeDoubleBE(value);
  return bytes.toString("hex");
}

// retain one object's exact producer field order
function ordered(value, keys) {
  return Object.fromEntries(keys.map((key) => [key, value[key]]));
}

// encode the exact closed source grammar without requiring built TypeScript output
function encodeSource(value) {
  const keys = value.family === "rain"
    ? [...SOURCE_KEYS.slice(0, 7), "causalInputs", ...SOURCE_KEYS.slice(7)]
    : value.family === "temperature"
      ? [...SOURCE_KEYS.slice(0, 7), "recentErrorState", ...SOURCE_KEYS.slice(7)]
      : SOURCE_KEYS;
  const encoded = ordered(value, keys);
  encoded.rows = value.rows.map((row) => ordered(row, SOURCE_ROW_KEYS[value.family]));
  if (value.family === "rain") {
    const causal = value.causalInputs;
    encoded.causalInputs = ordered(causal,
      ["contractVersion", "captureSet", "currentRun", "priorRuns", "stationHours"]);
    encoded.causalInputs.captureSet = causal.captureSet.map((capture) => ordered(capture,
      ["claimId", "kind", "stationId", "runInitializedAt", "windowStart",
        "windowEndExclusive", "completedAt", "bodySha256"]));
    encoded.causalInputs.currentRun = ordered(causal.currentRun,
      ["runInitializedAt", "completedAt", "contentSha256", "hours"]);
    encoded.causalInputs.currentRun.hours = causal.currentRun.hours.map((hour) => ordered(hour,
      ["leadHours", "precipitationMm64", "temperatureC64", "relativeHumidityPercent64",
        "cloudCoverPercent64", "pressureHpa64", "windSpeedMps64", "windDirectionDegrees64"]));
  }
  return Buffer.from(`${JSON.stringify(encoded)}\n`);
}

// derive the exact source addresses consumed by both candidate and comparator members
function sourceIdentity(sourceBytes) {
  const source = JSON.parse(sourceBytes);
  const inputSha256 = sha256(sourceBytes);
  const sourceRowSha256 = source.rows.map((row) => sha256(Buffer.from(
    `adjustment-shadow-source-row/v1\n${source.family}\n${JSON.stringify(ordered(row, SOURCE_ROW_KEYS[source.family]))}\n`,
  )));
  const sourceReceiptSha256 = sha256(Buffer.from([
    "adjustment-shadow-source-receipt/v1", source.registrationSha256, source.candidateSha256,
    source.sourceSha256, source.dueKey, source.issuedAt, source.rows[0].validAt,
    source.rows.at(-1).validAt, String(source.rowCount), inputSha256,
  ].join("\n")));
  return { inputSha256, sourceReceiptSha256, sourceRowSha256 };
}

// encode the exact family-specific candidate body order
function encodeBody(value) {
  const encoded = ordered(value, BODY_KEYS);
  encoded.rows = value.rows.map((row) => ordered(row, BODY_ROW_KEYS[value.family]));
  return Buffer.from(`${JSON.stringify(encoded)}\n`);
}

// create one actual raw serving authority from canonical receipt bytes
function rawServingAuthority(receipt) {
  const receiptBytes = canonicalBytes(receipt);
  return { artifactBase64: null, artifactIdentitySha256: null, artifactMemberSha256: null,
    authorityKind: "policy_raw", receiptBase64: receiptBytes.toString("base64"),
    receiptMemberSha256: sha256(receiptBytes) };
}

// retain the actual qualified serving receipt and both immutable artifact addresses
function qualifiedServingAuthority(artifactBytes, identity) {
  const receiptBytes = canonicalBytes({
    actionKind: "promote", actionSha256: HASH, bundleSha256: identity, candidateGraphSha256: HASH,
    candidateSha256: HASH, contractVersion: "adjustment-installed-candidate-receipt/v2",
    controlPlaneSha256: HASH, deployedCommit: "a".repeat(40), deployedImageDigest: `sha256:${HASH}`,
    deployedRelease: "fixture", deployedSettingsSha256: HASH, fencingToken: "1", fullMemberRootSha256: HASH,
    installedAt: STAGE_AT, lifecycleHeadSha256: HASH, policyDecision: "qualified", policyReportSha256: HASH,
    registrationSha256: HASH, sourceSha256: HASH,
  });
  return { artifactBase64: artifactBytes.toString("base64"), artifactIdentitySha256: identity,
    artifactMemberSha256: sha256(artifactBytes), authorityKind: "maintenance_qualified",
    receiptBase64: receiptBytes.toString("base64"), receiptMemberSha256: sha256(receiptBytes) };
}

// build an original temperature package with both trained coefficient vectors and strength bands
function temperaturePackage() {
  const band = { alpha: 0.65, supported: true, trainingCutoffUtc: "2026-09-30T00:00:00.000Z" };
  const unsigned = {
    candidateSha256: HASH, contractVersion: "forecast-adjustment-maintenance-runtime-package/v1",
    dueMonth: "2026-10", family: "temperature", model: {
      adaptiveCoefficients: Array(49).fill(0.125), cohort: "ecmwf_single_run_hindcast",
      contractVersion: "temperature-permanent-model/v1", directCoefficients: Array(35).fill(-0.25),
      effectiveFrom: "2026-10-01T00:00:00.000Z", latestTrainingValidAt: "2026-09-29T00:00:00.000Z",
      learnedStrengthContractVersion: "temperature-winner-extensions-research/v1", scope: "assumed_delay6_next12",
      strengthBands: { "1-6": band, "7-12": { ...band, alpha: 0.8 } }, supported: true,
      trainingCutoffUtc: "2026-09-30T00:00:00.000Z",
    }, source: { adapterVersion: "open-meteo-ecmwf-single-run/v1", cohort: "ecmwf_single_run_hindcast",
      dataset: "single_run", maximumReceiptAgeHours: 12, providerKey: "open-meteo", scope: "assumed_delay6_next12",
      sourceDelayHours: 6, upstreamModel: "ecmwf_ifs" },
  };
  return { ...unsigned, bundleSha256: sha256(canonicalBytes(unsigned)) };
}

// use the original feature catalog and a compact valid model with 160 leaf trees per head
async function rainPackage(version) {
  const source = await readFile(new URL("../../../../packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts", import.meta.url), "utf8");
  const literal = source.match(/export const RAIN_HURDLE_WIND_ARTIFACT_JSON = (".*") as const;/u)?.[1];
  assert.ok(literal, "original feature catalog literal");
  const { featureNames } = JSON.parse(JSON.parse(literal));
  const head = (objective) => ({ objective, baseScore: 0.5,
    trees: Array.from({ length: 160 }, () => [[0], [0.125], [-1], [-1], [0]]) });
  return { contractVersion: `rain-hurdle-wind-runtime/${version}`, modelMonth: "2026-10", featureNames,
    rules: [{ threshold: 0.1, cutoff: null }, { threshold: 1, cutoff: 0.25 }, { threshold: 2.5, cutoff: 0.5 }],
    categoryScales: [0.75, 1, 1.25], heads: { "0.1": head("binary:logistic"), "1.0": head("binary:logistic"),
      "2.5": head("binary:logistic"), amount: head("reg:gamma") },
    ...(version === "v2" ? { projectionId: "R3_spring_wet_logit_plus_0_20" } : {}) };
}

// encode the exact comparator top-level, authority, and row order
function encodeComparator(value) {
  const encoded = ordered(value, COMPARATOR_KEYS);
  encoded.servingAuthority = ordered(value.servingAuthority, AUTHORITY_KEYS);
  encoded.rows = value.rows.map((row) => ordered(row, COMPARATOR_ROW_KEYS[value.family]));
  return Buffer.from(`${JSON.stringify(encoded)}\n`);
}

// build one real source and prediction pair through the canonical TypeScript producer
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
    if (family === "temperature") {
      return {
        adapterVersion: "open-meteo-ecmwf-single-run/v1", bestMatchContentSha256: HASH,
        bestMatchProductRunAt: common.referenceAt, bestMatchSourceId: "7",
        bestMatchTemperatureC64: binary64(13), contentSha256: common.contentSha256,
        dataset: "single_run", leadHours, modelCycle: "50r1", modelLeadHours: leadHours + 6,
        providerKey: "open-meteo", providerResponseSha256: HASH,
        rawRelativeHumidityPercent64: binary64(80),
        rawTemperatureC64: binary64(12),
        rawWindSpeedMps64: binary64(5), receivedAt: issuedAt,
        referenceAt: common.referenceAt, sourceSha256: HASH, upstreamModel: "ecmwf_ifs", validAt,
      };
    }
    if (family === "wind") {
      return { ...common, windGustMps64: binary64(12),
        windSpeedMps64: binary64(8) };
    }
    return { ...common, precipitationMm64: binary64(2) };
  });
  const sourceValue = {
    contractVersion: "adjustment-shadow-source-projection/v1", family,
    registrationSha256: HASH, candidateSha256: HASH, sourceSha256: HASH,
    dueKey: `capture/${issuedAt}`, issuedAt, rowCount, rows,
  };
  if (family === "temperature") {
    sourceValue.recentErrorState = {
      b24C: 0.2, b72C: 0.1, cohort: "ecmwf_single_run_hindcast", localDates: 3, mad72C: 0.5,
      maximumSourceRunInitializedAt: "9999-11-01T06:00:00.000Z",
      maximumSourceValidAt: "9999-11-01T17:00:00.000Z", n24: 12, n72: 36,
      sourceKeys: ["source-1"], supported: true,
      targetRunInitializedAt: "9999-11-01T12:35:00.000Z",
      windowEndValidAt: "9999-11-01T17:00:00.000Z",
    };
  }
  if (family === "rain") {
    sourceValue.causalInputs = {
      contractVersion: "adjustment-shadow-rain-causal-inputs/v1",
      captureSet: [{ bodySha256: HASH, claimId: "rain-current", completedAt: issuedAt, kind: "forecast",
        runInitializedAt: "9999-11-01T12:35:00.000Z", stationId: null,
        windowEndExclusive: null, windowStart: null }],
      currentRun: {
        completedAt: issuedAt, contentSha256: HASH,
        hours: Array.from({ length: 48 }, (_, index) => ({
          cloudCoverPercent64: binary64(50), leadHours: index + 1,
          precipitationMm64: binary64(2),
          pressureHpa64: binary64(1000),
          relativeHumidityPercent64: binary64(80),
          temperatureC64: binary64(12),
          windDirectionDegrees64: binary64(180),
          windSpeedMps64: binary64(5),
        })),
        runInitializedAt: "9999-11-01T12:35:00.000Z",
      },
      priorRuns: [], stationHours: [],
    };
  }
  const sourceBytes = encodeSource(sourceValue);
  const identity = sourceIdentity(sourceBytes);
  const bodyRows = rows.map((row, index) => {
    const common = { leadHours: index + 1, sourceRowSha256: identity.sourceRowSha256[index],
      validAt: row.validAt };
    if (family === "temperature") {
      return { ...common, candidateTemperatureC64: binary64(10),
        fallbackCode: "none", wouldApply: true };
    }
    if (family === "wind") {
      return { ...common,
        candidateGustMps64: index >= 48 && index < 72 ? null : binary64(12),
        candidateSpeedMps64: binary64(8),
        gustWouldApply: index < 48 || index >= 72, speedWouldApply: true };
    }
    return { ...common, atLeast1_0Probability64: binary64(0.4),
      atLeast2_5Probability64: binary64(0.2),
      candidatePrecipitationMm64: binary64(2), fallbackCode: "none",
      occurrenceProbability64: binary64(0.6),
      positiveAmountMm64: binary64(2), wouldApply: true };
  });
  const bodyValue = {
    candidateSha256: HASH, contractVersion: "adjustment-shadow-prediction-values/v3",
    dueKey: sourceValue.dueKey, family, inputSha256: identity.inputSha256, issuedAt,
    registrationSha256: HASH, rowCount, rows: bodyRows,
    sourceReceiptSha256: identity.sourceReceiptSha256, sourceSha256: HASH,
  };
  const bodyBytes = encodeBody(bodyValue);
  return { bodyBytes, sourceBytes, sourceValue };
}

// bind an actual policy-raw comparator to one exact source and body pair
function boundComparatorFixture(family, qualifiedAuthority = null) {
  const fixture = shadowFixture(family);
  const rawReceipt = {
    activePackage: null,
    contractVersion: `forecast-adjustment-${family}-maintenance-registry/v1`,
    rawReason: "policy_raw",
    siteKey: "ballydidean",
  };
  const servingAuthority = qualifiedAuthority ?? rawServingAuthority(rawReceipt);
  const identity = sourceIdentity(fixture.sourceBytes);
  const rows = fixture.sourceValue.rows.map((row, index) => {
    const common = { validAt: row.validAt, leadHours: index + 1,
      sourceRowSha256: identity.sourceRowSha256[index] };
    if (family === "wind") {
      return { ...common, incumbentSpeedMps64: binary64(5),
        incumbentGustMps64: binary64(8), speedApplied: false,
        gustApplied: false, reasonCode: "policy_raw" };
    }
    if (family === "temperature") {
      return { ...common, incumbentTemperatureC64: binary64(12), applied: true, reasonCode: null };
    }
    return { ...common, incumbentPrecipitationMm64: binary64(0),
      occurrenceProbability64: binary64(0),
      atLeast1_0Probability64: binary64(0),
      atLeast2_5Probability64: binary64(0),
      positiveAmountMm64: binary64(0), applied: false,
      reasonCode: "policy_raw" };
  });
  const comparator = {
    contractVersion: "adjustment-shadow-incumbent-comparator/v1", family,
    registrationSha256: fixture.sourceValue.registrationSha256,
    candidateSha256: fixture.sourceValue.candidateSha256,
    sourceSha256: fixture.sourceValue.sourceSha256, dueKey: fixture.sourceValue.dueKey,
    issuedAt: fixture.sourceValue.issuedAt, sourceProjectionSha256: sha256(fixture.sourceBytes),
    predictionBodySha256: sha256(fixture.bodyBytes), servingAuthority, rowCount: rows.length, rows,
  };
  const comparatorBytes = encodeComparator(comparator);
  return { ...fixture, comparator, comparatorBytes };
}

// run the exact deployed web producer admission boundary
function validateShadowStage(input) {
  const source = Buffer.from(input.sourceProjection);
  const body = Buffer.from(input.body);
  parseMaintenanceShadowSourceProjection(source);
  parseMaintenanceShadowValues(body);
  if (input.comparator !== undefined) {
    const comparator = parseMaintenanceShadowComparator(Buffer.from(input.comparator));
    validateMaintenanceShadowComparatorBinding(comparator, source, body);
  }
  const expected = createMaintenanceShadowPredictionMetadata(body, source);
  const fields = [
    "bodyByteCount", "candidateSha256", "dueKey", "inputSha256", "issuedAt",
    "maxValidAt", "minValidAt", "predictionBodySha256", "predictionSchemaSha256",
    "predictionSha256", "registrationSha256", "rowCount", "sourceReceiptSha256", "sourceSha256",
  ];
  if (fields.some((field) => input.metadata[field] !== expected[field])) {
    throw new Error("adjustment shadow capsule metadata differs");
  }
}

// run the same deployed capsule validator over values decoded by the archive owner
function validateShadowCapsule(capsule) {
  validateShadowStage({
    body: Buffer.from(capsule.bodyBase64, "base64"),
    ...(capsule.contractVersion === "adjustment-shadow-revision-capsule/v2"
      ? { comparator: Buffer.from(capsule.comparatorBase64, "base64") }
      : {}),
    metadata: capsule.metadata,
    sourceProjection: Buffer.from(capsule.sourceProjectionBase64, "base64"),
  });
}

// derive one database receipt from the original receipt and frontier preimages
function revisionReceipt(stageReceipt, metadata) {
  const predecessorFrontierSha256 = sha256("adjustment-revision-frontier/v1\n0\n");
  const receipt = {
    archiveCommitOrdinal: "1",
    archiveCommittedAt: "9999-11-01T18:37:00.000Z",
    contractVersion: "adjustment-revision-commit-receipt/v1",
    frontierSha256: "",
    predecessorFrontierSha256,
    projectionIdentitySha256: metadata.sourceReceiptSha256,
    projectionKind: "shadow_prediction",
    projectionSha256: metadata.inputSha256,
    receiptSha256: "",
    stageReceiptSha256: stageReceipt.stageReceiptSha256,
  };
  receipt.receiptSha256 = sha256([
    receipt.contractVersion, receipt.archiveCommitOrdinal, receipt.archiveCommittedAt,
    receipt.projectionKind, receipt.projectionIdentitySha256, receipt.projectionSha256,
    receipt.stageReceiptSha256, receipt.predecessorFrontierSha256,
  ].join("\n"));
  receipt.frontierSha256 = sha256([
    "adjustment-revision-frontier/v1", receipt.predecessorFrontierSha256,
    receipt.archiveCommitOrdinal, receipt.receiptSha256,
  ].join("\n"));
  return receipt;
}

// report whether one retired stage remains addressable
async function pathExists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

// exercise stage, publication, rejected blind retry, restage, and convergent retry
async function writePublicationFixture(parent, name, fixture, comparatorBytes = null) {
  const root = join(parent, `node-publication-${name}`);
  await mkdir(root, { mode: 0o700 });
  const store = new AdjustmentRevisionArchiveStore({
    now: () => new Date(STAGE_AT),
    parseProjection: parseAdjustmentRevisionProjectionDocument,
    root,
    validateShadowCapsule,
    validateShadowStage,
  });
  const handler = createAdjustmentRevisionArchiveHandler({ store });
  const metadata = createMaintenanceShadowPredictionMetadata(
    fixture.bodyBytes, fixture.sourceBytes,
  );
  const stageInput = {
    bodyBase64: fixture.bodyBytes.toString("base64"),
    ...(comparatorBytes === null ? {} : { comparatorBase64: comparatorBytes.toString("base64") }),
    metadata,
    sourceProjectionBase64: fixture.sourceBytes.toString("base64"),
    sourceProjectionSha256: sha256(fixture.sourceBytes),
  };
  const stageReceipt = await handler(
    "/internal/adjustment-maintenance/archive/stage", stageInput,
  );
  const receipt = revisionReceipt(stageReceipt, metadata);
  const publishInput = comparatorBytes === null
    ? {
        metadata,
        revisionReceipt: receipt,
        sourceProjectionSha256: stageInput.sourceProjectionSha256,
        stageReceiptSha256: stageReceipt.stageReceiptSha256,
      }
    : {
        ...stageInput,
        predictionCommittedAt: "9999-11-01T18:36:00.000Z",
        revisionReceipt: receipt,
        stageReceipt,
      };
  const publication = await handler(
    "/internal/adjustment-maintenance/archive/publish", publishInput,
  );
  const stagePath = join(
    root, "shadow-revision-stages", `sha256-${metadata.predictionSha256}.json`,
  );
  assert.equal(await pathExists(stagePath), false, `${name} first publication retires stage`);
  let blindRetryRejected = false;
  try {
    await handler("/internal/adjustment-maintenance/archive/publish", publishInput);
  } catch {
    blindRetryRejected = true;
  }
  assert.equal(blindRetryRejected, true, `${name} blind publication retry`);
  const restaged = await handler(
    "/internal/adjustment-maintenance/archive/stage", stageInput,
  );
  assert.deepEqual(restaged, stageReceipt, `${name} frozen restage receipt`);
  const retried = await handler(
    "/internal/adjustment-maintenance/archive/publish", publishInput,
  );
  assert.deepEqual(retried, publication, `${name} publication retry receipt`);
  assert.equal(await pathExists(stagePath), false, `${name} retry retires stage`);
  await writeFile(join(parent, `publication-${name}-stage.request.json`), JSON.stringify(stageInput));
  await writeFile(join(parent, `publication-${name}-stage.response.json`), canonicalBytes(stageReceipt));
  await writeFile(join(parent, `publication-${name}-publish.request.json`), JSON.stringify(publishInput));
  await writeFile(join(parent, `publication-${name}-publish.response.json`), canonicalBytes(publication));
  return `${name}\t${comparatorBytes === null ? "v1" : "v2"}`;
}

// write one complete typed stage input without duplicating byte payloads in JSON
async function writeInput(parent, name, fixture, comparatorBytes = null, metadata = null) {
  const actualMetadata = metadata ?? createMaintenanceShadowPredictionMetadata(
    fixture.bodyBytes, fixture.sourceBytes,
  );
  await writeFile(join(parent, `${name}-source.json`), fixture.sourceBytes);
  await writeFile(join(parent, `${name}-body.json`), fixture.bodyBytes);
  if (comparatorBytes !== null) await writeFile(join(parent, `${name}-comparator.json`), comparatorBytes);
  await writeFile(join(parent, `${name}-input.json`), canonicalBytes({
    metadata: actualMetadata,
    sourceProjectionSha256: sha256(fixture.sourceBytes),
  }));
  return actualMetadata;
}

const parent = process.argv[2];
const temperature = shadowFixture("temperature");
const wind = boundComparatorFixture("wind");
const rain = boundComparatorFixture("rain");
const tempArtifact = temperaturePackage();
const tempQualified = boundComparatorFixture("temperature", qualifiedServingAuthority(canonicalBytes(tempArtifact), tempArtifact.bundleSha256));
const windArtifact = qualifiedWindPackage();
const windQualified = boundComparatorFixture("wind", qualifiedServingAuthority(canonicalBytes(windArtifact), windArtifact.bundleSha256));
const rainV1Artifact = canonicalBytes(await rainPackage("v1"));
const rainV2Artifact = canonicalBytes(await rainPackage("v2"));
const rainV1Qualified = boundComparatorFixture("rain", qualifiedServingAuthority(rainV1Artifact, sha256(rainV1Artifact)));
const rainV2Qualified = boundComparatorFixture("rain", qualifiedServingAuthority(rainV2Artifact, sha256(rainV2Artifact)));
const shortTemperature = temperaturePackage();
delete shortTemperature.bundleSha256;
shortTemperature.model.adaptiveCoefficients.pop();
const shortTemperatureArtifact = { ...shortTemperature, bundleSha256: sha256(canonicalBytes(shortTemperature)) };
const shortTemperatureQualified = boundComparatorFixture("temperature", qualifiedServingAuthority(
  canonicalBytes(shortTemperatureArtifact), shortTemperatureArtifact.bundleSha256,
));
const incompleteRain = await rainPackage("v2");
incompleteRain.heads.amount.trees.pop();
const incompleteRainArtifact = canonicalBytes(incompleteRain);
const incompleteRainQualified = boundComparatorFixture("rain", qualifiedServingAuthority(
  incompleteRainArtifact, sha256(incompleteRainArtifact),
));
const temperatureMetadata = createMaintenanceShadowPredictionMetadata(
  temperature.bodyBytes, temperature.sourceBytes,
);
const windMetadata = createMaintenanceShadowPredictionMetadata(wind.bodyBytes, wind.sourceBytes);
const cases = [
  { accepted: true, fixture: temperature, name: "temperature-v1" },
  { accepted: true, comparatorBytes: wind.comparatorBytes, fixture: wind, name: "wind-v2" },
  { accepted: true, comparatorBytes: rain.comparatorBytes, fixture: rain, name: "rain-v2" },
  { accepted: true, comparatorBytes: tempQualified.comparatorBytes, fixture: tempQualified, name: "temperature-qualified" },
  { accepted: true, comparatorBytes: windQualified.comparatorBytes, fixture: windQualified, name: "wind-qualified" },
  { accepted: true, comparatorBytes: rainV1Qualified.comparatorBytes, fixture: rainV1Qualified, name: "rain-qualified-v1-artifact" },
  { accepted: true, comparatorBytes: rainV2Qualified.comparatorBytes, fixture: rainV2Qualified, name: "rain-qualified-v2-artifact" },
  { accepted: false, comparatorBytes: shortTemperatureQualified.comparatorBytes, fixture: shortTemperatureQualified, name: "temperature-qualified-short-vector" },
  { accepted: false, comparatorBytes: incompleteRainQualified.comparatorBytes, fixture: incompleteRainQualified, name: "rain-qualified-short-head" },
  { accepted: false, fixture: { ...temperature,
    bodyBytes: Buffer.concat([temperature.bodyBytes, Buffer.from("\n")]) },
    metadata: temperatureMetadata, name: "temperature-extra-lf" },
  { accepted: false, comparatorBytes: wind.comparatorBytes, fixture: wind,
    metadata: { ...windMetadata, rowCount: windMetadata.rowCount + 1 }, name: "wind-metadata-row-count" },
  { accepted: false, comparatorBytes: Buffer.from(JSON.stringify({ ...rain.comparator,
    sourceProjectionSha256: SECOND_HASH }) + "\n"), fixture: rain, name: "rain-comparator-binding" },
];
const manifest = [];
for (const entry of cases) {
  const metadata = await writeInput(
    parent, entry.name, entry.fixture, entry.comparatorBytes ?? null, entry.metadata ?? null,
  );
  const root = join(parent, `node-${entry.name}`);
  await mkdir(root, { mode: 0o700 });
  const store = new AdjustmentRevisionArchiveStore({
    now: () => new Date(STAGE_AT),
    parseProjection: parseAdjustmentRevisionProjectionDocument,
    root,
    validateShadowStage,
  });
  const input = {
    bodyBase64: entry.fixture.bodyBytes.toString("base64"),
    ...(entry.comparatorBytes === undefined ? {} : {
      comparatorBase64: entry.comparatorBytes.toString("base64"),
    }),
    metadata,
    sourceProjectionBase64: entry.fixture.sourceBytes.toString("base64"),
    sourceProjectionSha256: sha256(entry.fixture.sourceBytes),
  };
  await writeFile(
    join(parent, `${entry.name}-wire-request.json`),
    Buffer.from(`${JSON.stringify(input)}\n`),
  );
  let accepted = false;
  try {
    const receipt = await store.stageShadowRevision(input);
    accepted = true;
    await writeFile(join(parent, `${entry.name}-receipt.json`), canonicalBytes(receipt));
    await writeFile(join(parent, `${entry.name}-stage.json`), await readFile(join(
      root, "shadow-revision-stages", `sha256-${metadata.predictionSha256}.json`,
    )));
  } catch (error) {
    if (entry.accepted) throw error;
  }
  assert.equal(accepted, entry.accepted, `${entry.name} Node admission`);
  manifest.push(`${entry.name}\t${entry.accepted ? "yes" : "no"}\t${entry.comparatorBytes === undefined ? "v1" : "v2"}`);
}
await writeFile(join(parent, "shadow-cases.tsv"), `${manifest.join("\n")}\n`);

// emit publication fixtures only for the raw native publication differential
if (process.argv[3] === "publication") {
  const publications = [
    await writePublicationFixture(parent, "temperature-v1", temperature),
    await writePublicationFixture(parent, "wind-v2", wind, wind.comparatorBytes),
  ];
  await writeFile(join(parent, "shadow-publication-cases.tsv"), `${publications.join("\n")}\n`);
}
