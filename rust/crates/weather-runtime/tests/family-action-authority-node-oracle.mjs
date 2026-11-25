import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, lstat, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from
  "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { authorizeAdjustmentFamilyAction } from
  "../../../../deploy/scripts/adjustment-evaluation-package.mjs";
import {
  AdjustmentDevelopmentCustodyAnchorStore,
  AdjustmentMaintenanceAnchorStore,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const graphAt = "2026-10-08T00:35:00.000Z";
const publishedAt = "2026-10-08T00:36:00.000Z";
const finalizedAt = "2026-10-08T00:37:00.000Z";
const hashRecords = new Map();

// preserve the production canonical member order recursively
function canonical(value) {
  // retain array order while canonicalizing every member
  if (Array.isArray(value)) return value.map(canonical);
  // order every object by its exact javascript key comparison
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, canonical(value[key])]),
    );
  }
  return value;
}

// encode one canonical document without its file delimiter
function canonicalText(value) {
  return JSON.stringify(canonical(value));
}

// encode one canonical private file with its required single newline
function canonicalBytes(value) {
  return Buffer.from(`${canonicalText(value)}\n`, "utf8");
}

// retain every original sha256 preimage consumed by the fixture
function trackedSha256(value) {
  const bytes = Buffer.from(value);
  const digest = createHash("sha256").update(bytes).digest("hex");
  hashRecords.set(bytes.toString("hex"), digest);
  return digest;
}

// create one stable distinct fixture identity from a public label
function identity(label) {
  return trackedSha256(Buffer.from(`family-action-authority-oracle/${label}`, "utf8"));
}

// populate a set of named identities without aliases
function hashes(prefix, names) {
  return Object.fromEntries(names.map((name) => [name, identity(`${prefix}/${name}`)]));
}

// build one fully valid development custody authority anchor
function developmentAnchor() {
  return {
    ...hashes("development", [
      "actionSha256", "artifactSha256", "candidateGraphSha256", "candidateSha256",
      "captureEpochWitnessSha256", "controlSha256", "custodyAcknowledgementSha256",
      "custodyCheckpointSha256", "developmentGraphSha256", "frontierSha256",
      "inputHeadSha256", "lifecycleLedgerRootSha256", "memberRootSha256",
      "pageSha256", "policyReportSha256", "registrationSha256", "sourceSha256",
      "startSha256",
    ]),
    actionKind: "shadow",
    archiveCommitOrdinal: "1",
    contractVersion: "adjustment-development-custody-anchor/v1",
    controlVersion: "17",
    custodyAcknowledgedAt: graphAt,
    dueKey: "monthly/temperature/2026-10",
    family: "temperature",
    fullGraphVerifiedAt: publishedAt,
    predecessorAnchorSha256: null,
    sequence: "0",
    sourceCommit: "b".repeat(40),
    startMemberSha256: null,
  };
}

// build one fully valid rain control-reference custody authority anchor
function rainAnchor() {
  return {
    ...hashes("rain", [
      "actionSha256", "captureEpochWitnessSha256", "controlSha256",
      "controlStateSha256", "custodyAcknowledgementSha256", "custodyCheckpointSha256",
      "frontierSha256", "graphManifestSha256", "memberRootSha256",
      "ordinalArtifactSha256", "pageSha256", "sourceMemberRootSha256",
      "sourceReceiptRootSha256", "startSha256", "workstationJournalHeadSha256",
    ]),
    actionKind: "control_reference",
    archiveCommitOrdinal: "1",
    contractVersion: "adjustment-rain-control-custody-anchor/v1",
    controlVersion: "17",
    custodyAcknowledgedAt: graphAt,
    dueMonth: "2026-10",
    fencingToken: "1",
    fullGraphVerifiedAt: publishedAt,
    predecessorAnchorSha256: null,
    sequence: "0",
    sourceCommit: "c".repeat(40),
    startMemberSha256: null,
  };
}

// build one valid finalized future-only maintenance authority anchor
function v3FinalizedAnchor() {
  return {
    ...hashes("maintenance-v3", [
      "actionSha256", "burnSha256", "candidateArtifactRootSha256",
      "candidateReportSha256", "captureEpochWitnessSha256",
      "confirmationAccessSha256", "controlSha256", "custodyCheckpointSha256",
      "finalizationProofSha256", "frontierSha256", "fullMemberRootSha256",
      "graphManifestSha256", "inputSealSha256", "lifecycleLedgerRootSha256",
      "pageSha256", "policyReportSha256", "requiredInputRootSha256",
      "workstationJournalHeadSha256",
    ]),
    archiveCommitOrdinal: "1",
    confirmationChunkCount: 24,
    contractVersion: "adjustment-maintenance-anchor/v3",
    controlVersion: "17",
    ctfState: "finalized",
    dueKey: `confirmation/rain/${identity("maintenance-v3/due")}`,
    family: "rain",
    finalizedAt,
    fullGraphVerifiedAt: graphAt,
    predecessorAnchorSha256: null,
    publishedAt,
    sequence: "0",
    sourceCommit: "d".repeat(40),
  };
}

// build one valid legacy transferred maintenance authority anchor
function v2TransferredAnchor() {
  return {
    ...hashes("maintenance-v2-transferred", [
      "archiveObjectSha256", "catalogRootSha256", "catalogWatermarkSha256",
      "controlSha256", "frontierRootSha256", "graphManifestSha256",
      "lifecycleLedgerRootSha256", "manifestSha256", "workstationJournalHeadSha256",
    ]),
    actionSha256: null,
    catalogGeneration: "1",
    contractVersion: "adjustment-maintenance-anchor/v2",
    controlVersion: 14,
    ctfState: "transferred",
    dueKey: "capture/2026-10-08T00:35:00.000Z",
    frontierCount: 1,
    fullGraphVerifiedAt: graphAt,
    fullMemberRootSha256: null,
    generation: "1",
    predecessorAnchorSha256: null,
    publishedAt,
    reportSha256: null,
    revisionSnapshotRootSha256: null,
    sequence: "0",
    sourceCommit: "e".repeat(40),
  };
}

// build one valid legacy finalized maintenance authority anchor
function v2FinalizedAnchor() {
  return {
    ...v2TransferredAnchor(),
    ...hashes("maintenance-v2-finalized", [
      "actionSha256", "burnSha256", "catalogRevisionReceiptRootSha256",
      "finalizationProofSha256", "fullMemberRootSha256", "inputSealSha256",
      "reportSha256", "requiredInputRootSha256", "retirementSetRootSha256",
      "revisionSnapshotRootSha256",
    ]),
    archiveCommitOrdinal: "1",
    confirmationChunkCount: 27,
    ctfState: "finalized",
    finalizedAt,
  };
}

const development = developmentAnchor();
const rain = rainAnchor();
const maintenanceV3Finalized = v3FinalizedAnchor();
const maintenanceV2Transferred = v2TransferredAnchor();
const maintenanceV2Finalized = v2FinalizedAnchor();
const documents = {
  development,
  rain,
  maintenanceV3Finalized,
  maintenanceV2Transferred,
  maintenanceV2Finalized,
  actionDevelopment: {
    actionKind: "shadow", candidateGraphSha256: development.candidateGraphSha256,
    candidateSha256: development.candidateSha256,
    expectedSourceCommit: development.sourceCommit, family: development.family,
    lifecycleHeadSha256: development.lifecycleLedgerRootSha256,
    policyReportSha256: development.policyReportSha256,
  },
  actionDevelopmentMismatch: {
    actionKind: "shadow", candidateGraphSha256: development.candidateGraphSha256,
    candidateSha256: identity("development/wrong-candidate"),
    expectedSourceCommit: development.sourceCommit, family: development.family,
    lifecycleHeadSha256: development.lifecycleLedgerRootSha256,
    policyReportSha256: development.policyReportSha256,
  },
  actionRain: {
    actionKind: rain.actionKind,
    contractVersion: "forecast-adjustment-rain-control-reference-action/v1",
    controlStateSha256: rain.controlStateSha256, dueMonth: rain.dueMonth,
    expectedSourceCommit: rain.sourceCommit, fencingToken: rain.fencingToken,
    graphManifestSha256: rain.graphManifestSha256,
    ordinalArtifactSha256: rain.ordinalArtifactSha256,
    sourceMemberRootSha256: rain.sourceMemberRootSha256,
    sourceReceiptRootSha256: rain.sourceReceiptRootSha256,
  },
  actionRainMismatch: {
    actionKind: rain.actionKind,
    contractVersion: "forecast-adjustment-rain-control-reference-action/v1",
    controlStateSha256: rain.controlStateSha256, dueMonth: "2026-11",
    expectedSourceCommit: rain.sourceCommit, fencingToken: rain.fencingToken,
    graphManifestSha256: rain.graphManifestSha256,
    ordinalArtifactSha256: rain.ordinalArtifactSha256,
    sourceMemberRootSha256: rain.sourceMemberRootSha256,
    sourceReceiptRootSha256: rain.sourceReceiptRootSha256,
  },
  actionV3Finalized: {
    actionKind: "promote", expectedSourceCommit: maintenanceV3Finalized.sourceCommit,
    fullMemberRootSha256: maintenanceV3Finalized.fullMemberRootSha256,
    lifecycleHeadSha256: maintenanceV3Finalized.lifecycleLedgerRootSha256,
    policyReportSha256: maintenanceV3Finalized.policyReportSha256,
  },
  actionV3Mismatch: {
    actionKind: "promote", expectedSourceCommit: maintenanceV3Finalized.sourceCommit,
    fullMemberRootSha256: maintenanceV3Finalized.fullMemberRootSha256,
    lifecycleHeadSha256: identity("maintenance-v3/wrong-lifecycle"),
    policyReportSha256: maintenanceV3Finalized.policyReportSha256,
  },
  actionV2Finalized: {
    actionKind: "promote", expectedSourceCommit: maintenanceV2Finalized.sourceCommit,
    fullMemberRootSha256: maintenanceV2Finalized.fullMemberRootSha256,
    lifecycleHeadSha256: maintenanceV2Finalized.lifecycleLedgerRootSha256,
    policyReportSha256: maintenanceV2Finalized.reportSha256,
  },
  actionV2FinalizedMismatch: {
    actionKind: "promote", expectedSourceCommit: maintenanceV2Finalized.sourceCommit,
    fullMemberRootSha256: maintenanceV2Finalized.fullMemberRootSha256,
    lifecycleHeadSha256: identity("maintenance-v2-finalized/wrong-lifecycle"),
    policyReportSha256: maintenanceV2Finalized.reportSha256,
  },
  actionV2TransferredCompensate: {
    actionKind: "compensate_shadow",
    expectedSourceCommit: maintenanceV2Transferred.sourceCommit,
  },
  actionV2TransferredMismatch: {
    actionKind: "compensate_shadow", expectedSourceCommit: "f".repeat(40),
  },
};

const cases = [
  ["development_match", "actionDevelopment", "dev", "development", "development"],
  ["development_mismatch", "actionDevelopmentMismatch", "dev", "development", "error"],
  ["development_absent", "actionDevelopment", "dev", "-", "error"],
  ["rain_match", "actionRain", "rain", "rain", "rain"],
  ["rain_mismatch", "actionRainMismatch", "rain", "rain", "error"],
  ["rain_absent", "actionRain", "rain", "-", "error"],
  ["future_finalized_match", "actionV3Finalized", "maintenance", "maintenanceV3Finalized", "maintenanceV3Finalized"],
  ["future_finalized_mismatch", "actionV3Mismatch", "maintenance", "maintenanceV3Finalized", "error"],
  ["legacy_finalized_match", "actionV2Finalized", "maintenance", "maintenanceV2Finalized", "maintenanceV2Finalized"],
  ["legacy_finalized_mismatch", "actionV2FinalizedMismatch", "maintenance", "maintenanceV2Finalized", "error"],
  ["legacy_transferred_compensate", "actionV2TransferredCompensate", "maintenance", "maintenanceV2Transferred", "maintenanceV2Transferred"],
  ["legacy_transferred_mismatch", "actionV2TransferredMismatch", "maintenance", "maintenanceV2Transferred", "error"],
  ["maintenance_absent", "actionV3Finalized", "maintenance", "-", "error"],
];
const directories = {
  dev: "development-custody-anchors",
  rain: "rain-control-custody-anchors",
  maintenance: "maintenance-anchors",
};

// expose every decoded fixture node through the closed fixture protocol
function emitNode(name, path, value) {
  if (Array.isArray(value)) {
    console.log(["node", name, path, "array", "-"].join("\t"));
    value.forEach((child, index) => emitNode(name, `${path}/${index}`, child));
    return;
  }
  if (value !== null && typeof value === "object") {
    console.log(["node", name, path, "object", "-"].join("\t"));
    // visit the exact canonical key order
    for (const [key, child] of Object.entries(canonical(value))) {
      emitNode(name, `${path}/${key}`, child);
    }
    return;
  }
  console.log(["node", name, path, value === null ? "null" : typeof value,
    value === null ? "null" : String(value)].join("\t"));
}

// snapshot the complete disposable directory and retained file identities
async function snapshot(root) {
  const entries = [];
  // walk only the fixture-owned bounded tree
  for (const name of (await readdir(root)).sort()) {
    const path = join(root, name);
    const details = await lstat(path);
    entries.push([name, details.mode & 0o777, details.isDirectory()
      ? await snapshot(path) : (await readFile(path)).toString("hex")]);
  }
  return entries;
}

// use the original store projection on ordinary non-root CI; the mapped-root run invokes the exported wrapper
async function ordinaryStore(action, actionSha256, kind, root) {
  if (kind === "rain" || process.getuid() === 0) {
    return authorizeAdjustmentFamilyAction(action, actionSha256, root);
  }
  const current = JSON.parse(await readFile(join(root, directories[kind], "current.json"), "utf8"));
  // reproduce the exact original wrapper key selection before calling its original store
  if (kind === "dev") {
    const store = new AdjustmentDevelopmentCustodyAnchorStore({ root });
    return store.authorizeDevelopmentShadowAction({
      actionKind: action.actionKind, actionSha256, artifactSha256: current.artifactSha256,
      candidateGraphSha256: action.candidateGraphSha256, candidateSha256: action.candidateSha256,
      family: action.family, lifecycleLedgerRootSha256: action.lifecycleHeadSha256,
      policyReportSha256: action.policyReportSha256, registrationSha256: current.registrationSha256,
      sourceCommit: action.expectedSourceCommit,
    });
  }
  const store = new AdjustmentMaintenanceAnchorStore({ root });
  if (current.contractVersion === "adjustment-maintenance-anchor/v3") {
    return store.authorizeFutureOnlyQualifiedAction({
      actionSha256, fullMemberRootSha256: action.fullMemberRootSha256,
      lifecycleLedgerRootSha256: action.lifecycleHeadSha256,
      policyReportSha256: action.policyReportSha256, sourceCommit: action.expectedSourceCommit,
    });
  }
  if (action.actionKind === "compensate_shadow") {
    return store.authorizeShadowCandidate({
      catalogRootSha256: current.catalogRootSha256, controlSha256: current.controlSha256,
      controlVersion: current.controlVersion, graphManifestSha256: current.graphManifestSha256,
      sourceCommit: action.expectedSourceCommit,
    });
  }
  return store.authorizeQualifiedAction({
    actionSha256, burnSha256: current.burnSha256, controlSha256: current.controlSha256,
    controlVersion: current.controlVersion, fullMemberRootSha256: action.fullMemberRootSha256,
    lifecycleLedgerRootSha256: action.lifecycleHeadSha256, reportSha256: action.policyReportSha256,
    revisionSnapshotRootSha256: current.revisionSnapshotRootSha256,
    sourceCommit: action.expectedSourceCommit,
    workstationJournalHeadSha256: current.workstationJournalHeadSha256,
  });
}

// observe one actual Node action against a disposable owned root
async function observe(root, [name, actionName, kind, currentName, expected]) {
  const evidence = join(root, name);
  await mkdir(evidence, { mode: 0o700 });
  await chmod(evidence, 0o700);
  // install only a selected current anchor and preserve genuine absence
  if (currentName !== "-") {
    const anchorRoot = join(evidence, directories[kind]);
    await mkdir(anchorRoot, { mode: 0o700 });
    await chmod(anchorRoot, 0o700);
    const currentPath = join(anchorRoot, "current.json");
    await writeFile(currentPath, canonicalBytes(documents[currentName]), { mode: 0o600 });
    await chmod(currentPath, 0o600);
  }
  const before = await snapshot(evidence);
  const action = documents[actionName];
  const actionSha256 = documents[currentName]?.actionSha256 ??
    (kind === "rain" ? rain.actionSha256 : kind === "dev" ? development.actionSha256
      : maintenanceV3Finalized.actionSha256);
  let result;
  let failure;
  try {
    result = await ordinaryStore(action, actionSha256, kind, evidence);
  } catch (error) {
    failure = error;
  }
  // verify source output or ordinary refusal without inventing authority
  if (expected === "error") {
    assert.ok(failure, name);
  } else {
    assert.equal(failure, undefined, `${name}: ${failure}`);
    assert.deepEqual(canonicalBytes(result), canonicalBytes(documents[expected]), name);
  }
  assert.deepEqual(await snapshot(evidence), before, name);
  console.log(["case", name, actionName, actionSha256, kind, currentName, expected].join("\t"));
}

// emit every supplied document and all original hash and clock preimages
for (const [name, value] of Object.entries(documents)) {
  console.log(["doc", name, canonicalText(value)].join("\t"));
  emitNode(name, "$", value);
  trackedSha256(canonicalBytes(value));
}
for (const [preimage, digest] of hashRecords) console.log(["hash", preimage, digest].join("\t"));
for (const clock of [graphAt, publishedAt, finalizedAt]) console.log(["clock", clock, Date.parse(clock)].join("\t"));

const root = await mkdtemp(join(tmpdir(), "weather-family-action-authority-node-oracle-"));
try {
  // isolate the original authority calls to their own disposable trees
  for (const fixture of cases) await observe(root, fixture);
} finally {
  await rm(root, { recursive: true, force: true });
}
