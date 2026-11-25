import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  access,
  chmod,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runAdjustmentArchiveCommand } from
  "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const clocks = [
  "2026-10-08T00:35:00.000Z",
  "2026-10-08T00:36:00.000Z",
  "2026-10-08T00:37:00.000Z",
];
const controlSha256 =
  "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb";
const controlVersion = "17";
const fixedDirectories = [
  "rain-control-state-stage-receipts",
  "rain-control-states",
  "revision-commit-receipts",
  "revision-custody-acknowledgements",
  "revision-frontier-successors",
  "revision-gaps",
  "revision-projections",
  "revision-publish-receipts",
  "revision-spool-locks",
  "revision-stage-receipts",
  "shadow-revision-capsules",
  "shadow-revision-gaps",
  "shadow-revision-publish-receipts",
];
const hashPreimages = new Map();

// recursively preserve the production canonical member order
function canonical(value) {
  // preserve array order while canonicalizing every member
  if (Array.isArray(value)) return value.map(canonical);
  // sort every object by its exact utf-16 key order
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, canonical(value[key])]),
    );
  }
  return value;
}

// retain the exact canonical value without its record delimiter
function canonicalText(value) {
  return JSON.stringify(canonical(value));
}

// retain the original single-lf canonical file framing
function canonicalBytes(value) {
  return Buffer.from(`${canonicalText(value)}\n`, "utf8");
}

// compute one original lowercase content identity and retain its exact preimage
function sha256(value) {
  const bytes = Buffer.isBuffer(value) ? Buffer.from(value) : Buffer.from(value);
  const digest = createHash("sha256").update(bytes).digest("hex");
  hashPreimages.set(bytes.toString("hex"), digest);
  return digest;
}

// create stable distinct fixture identities without aliases
function identity(label) {
  return sha256(Buffer.from(`custody-anchor-install-oracle/${label}`, "utf8"));
}

// construct one exact root-authenticated witness with its canonical self address
function captureEpochWitness() {
  const unsigned = {
    activationKind: "inert_v14_pre_activation",
    archiveCommitOrdinal: "0",
    catalogFrontierSha256:
      "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af",
    contractVersion: "adjustment-revision-capture-epoch-witness/v1",
    controlPlaneSha256: controlSha256,
    controlPlaneVersion: controlVersion,
    databaseMigrationHistorySha256:
      "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf",
    epochAt: clocks[0],
    servingSnapshotSha256: identity("witness/serving-snapshot"),
    sourceCommit: "a".repeat(40),
    sourceRelease: "2026.10.08-1",
    sourceServerImageDigest: `sha256:${"b".repeat(64)}`,
    sourceWebImageDigest: `sha256:${"c".repeat(64)}`,
  };
  return { ...unsigned, witnessSha256: sha256(canonicalBytes(unsigned)) };
}

const witness = captureEpochWitness();
const witnessSha256 = sha256(canonicalBytes(witness));

// retain four stable sorted retired-file identities in each acknowledgement
function retirementEntries(label) {
  return [
    "revision_commit_receipt",
    "revision_frontier_successor",
    "revision_projection",
    "revision_stage_receipt",
  ].map((kind) => ({
    fileSha256: identity(`${label}/${kind}/file`),
    identitySha256: identity(`${label}/${kind}/identity`),
    kind,
  }));
}

// create one exact V1 or V2 custody acknowledgement and its canonical self address
function acknowledgement(label, version, previous = null, overrides = {}) {
  const identityField = version === 2 ? "custodyCheckpointSha256" : "graphManifestSha256";
  const unsigned = {
    acknowledgedAt: clocks[0],
    afterArchiveCommitOrdinal: "0",
    afterFrontierSha256: identity(`${label}/after-frontier`),
    authority: "cold_custody_only",
    contractVersion: `adjustment-revision-custody-acknowledgement/v${version}`,
    [identityField]: identity(`${label}/checkpoint`),
    memberRootSha256: identity(`${label}/member-root`),
    nextArchiveCommitOrdinal: "1",
    nextFrontierSha256: identity(`${label}/next-frontier`),
    pageSha256: identity(`${label}/page`),
    previousAcknowledgementSha256: previous?.acknowledgementSha256 ?? null,
    previousPageSha256: identity(`${label}/previous-page`),
    retirementEntries: retirementEntries(label),
    startMemberSha256: null,
    startSha256: identity(`${label}/start`),
    watermarkArchiveCommitOrdinal: "1",
    watermarkFrontierSha256: identity(`${label}/watermark-frontier`),
    ...overrides,
  };
  return {
    ...unsigned,
    acknowledgementSha256: sha256(canonicalBytes(unsigned)),
  };
}

const acknowledgementV2 = acknowledgement("ack-v2", 2);
const acknowledgementV1 = acknowledgement("ack-v1", 1);
const acknowledgementMixed = acknowledgement("ack-mixed-v2", 2, acknowledgementV1);
const acknowledgementLaterV1 = acknowledgement("ack-later-v1", 1, acknowledgementV2);

// project the acknowledgement fields copied into either action-anchor family
function acknowledgementFields(value) {
  return {
    archiveCommitOrdinal: value.nextArchiveCommitOrdinal,
    custodyAcknowledgedAt: value.acknowledgedAt,
    custodyAcknowledgementSha256: value.acknowledgementSha256,
    custodyCheckpointSha256: value.custodyCheckpointSha256,
    frontierSha256: value.nextFrontierSha256,
    memberRootSha256: value.memberRootSha256,
    pageSha256: value.pageSha256,
    startMemberSha256: value.startMemberSha256,
    startSha256: value.startSha256,
  };
}

// create one fully bound valid development custody anchor
function developmentAnchor(label, acknowledgementValue, overrides = {}) {
  return {
    actionKind: "shadow",
    actionSha256: identity(`${label}/action`),
    artifactSha256: identity(`${label}/artifact`),
    candidateGraphSha256: identity(`${label}/candidate-graph`),
    candidateSha256: identity(`${label}/candidate`),
    captureEpochWitnessSha256: witnessSha256,
    contractVersion: "adjustment-development-custody-anchor/v1",
    controlSha256,
    controlVersion,
    developmentGraphSha256: identity(`${label}/graph`),
    dueKey: "monthly/temperature/2026-10",
    family: "temperature",
    fullGraphVerifiedAt: clocks[1],
    inputHeadSha256: identity(`${label}/input-head`),
    lifecycleLedgerRootSha256: identity(`${label}/lifecycle`),
    policyReportSha256: identity(`${label}/policy`),
    predecessorAnchorSha256: null,
    registrationSha256: identity(`${label}/registration`),
    sequence: "0",
    sourceCommit: "d".repeat(40),
    sourceSha256: identity(`${label}/source`),
    ...acknowledgementFields(acknowledgementValue),
    ...overrides,
  };
}

// create one fully bound valid rain control-reference custody anchor
function rainAnchor(label, acknowledgementValue, overrides = {}) {
  return {
    actionKind: "control_reference",
    actionSha256: identity(`${label}/action`),
    captureEpochWitnessSha256: witnessSha256,
    contractVersion: "adjustment-rain-control-custody-anchor/v1",
    controlSha256,
    controlStateSha256: identity(`${label}/control-state`),
    controlVersion,
    dueMonth: "2026-10",
    fencingToken: "1",
    fullGraphVerifiedAt: clocks[1],
    graphManifestSha256: identity(`${label}/graph-manifest`),
    ordinalArtifactSha256: identity(`${label}/ordinal-artifact`),
    predecessorAnchorSha256: null,
    sequence: "0",
    sourceCommit: "e".repeat(40),
    sourceMemberRootSha256: identity(`${label}/source-member-root`),
    sourceReceiptRootSha256: identity(`${label}/source-receipt-root`),
    workstationJournalHeadSha256: identity(`${label}/journal`),
    ...acknowledgementFields(acknowledgementValue),
    ...overrides,
  };
}

const development = developmentAnchor("development", acknowledgementV2);
const developmentSha256 = sha256(canonicalBytes(development));
const developmentNext = developmentAnchor("development-next", acknowledgementV2, {
  predecessorAnchorSha256: developmentSha256,
  sequence: "1",
});
const developmentMixed = developmentAnchor("development-mixed", acknowledgementMixed);
const developmentWrongCustody = developmentAnchor("development-wrong", acknowledgementV2, {
  custodyCheckpointSha256: identity("development/wrong-custody"),
});
const developmentWrongWitness = developmentAnchor("development-witness", acknowledgementV2, {
  captureEpochWitnessSha256: identity("development/wrong-witness"),
});
const rain = rainAnchor("rain", acknowledgementV2);
const rainSha256 = sha256(canonicalBytes(rain));
const rainNext = rainAnchor("rain-next", acknowledgementV2, {
  predecessorAnchorSha256: rainSha256,
  sequence: "1",
});
const rainWrongControl = rainAnchor("rain-control", acknowledgementV2, {
  controlSha256: identity("rain/wrong-control"),
});

const documents = {
  acknowledgementMixed,
  acknowledgementLaterV1,
  acknowledgementV1,
  acknowledgementV2,
  development,
  developmentInstallation: {
    anchorSha256: developmentSha256,
    contractVersion: "adjustment-development-custody-anchor-installation/v1",
    state: "installed",
  },
  developmentMixed,
  developmentMixedInstallation: {
    anchorSha256: sha256(canonicalBytes(developmentMixed)),
    contractVersion: "adjustment-development-custody-anchor-installation/v1",
    state: "installed",
  },
  developmentNext,
  developmentNextInstallation: {
    anchorSha256: sha256(canonicalBytes(developmentNext)),
    contractVersion: "adjustment-development-custody-anchor-installation/v1",
    state: "installed",
  },
  developmentWrongCustody,
  developmentWrongWitness,
  rain,
  rainInstallation: {
    anchorSha256: rainSha256,
    contractVersion: "adjustment-rain-control-custody-anchor-installation/v1",
    state: "installed",
  },
  rainNext,
  rainNextInstallation: {
    anchorSha256: sha256(canonicalBytes(rainNext)),
    contractVersion: "adjustment-rain-control-custody-anchor-installation/v1",
    state: "installed",
  },
  rainWrongControl,
  witness,
};

// expose every decoded fixture node for the independent Rust fixture codec
function emitNode(name, path, value) {
  // expose an array before its indexed children
  if (Array.isArray(value)) {
    console.log(["node", name, path, "array", "-"].join("\t"));
    value.forEach((item, index) => emitNode(name, `${path}/${index}`, item));
    return;
  }
  // expose an object before its canonically ordered members
  if (value !== null && typeof value === "object") {
    console.log(["node", name, path, "object", "-"].join("\t"));
    Object.entries(canonical(value)).forEach(
      ([key, item]) => emitNode(name, `${path}/${key}`, item),
    );
    return;
  }
  const kind = value === null ? "null" : typeof value;
  console.log(["node", name, path, kind, value === null ? "null" : String(value)].join("\t"));
}

// expose one exact canonical document and its complete typed tree
function emitDocument(name, value) {
  console.log(["doc", name, canonicalText(value)].join("\t"));
  emitNode(name, "$", canonical(value));
}

// distinguish genuine absence without creating the selected fixed path
async function exists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

// create one exact private file from a reusable fixture document
async function writeDocument(path, name) {
  if (name === "-") return;
  await writeFile(path, canonicalBytes(documents[name]), { mode: 0o600 });
  await chmod(path, 0o600);
}

// initialize every fixed archive directory required by the real installation command
async function initializeEvidenceRoot(root, fixture) {
  await mkdir(root, { mode: 0o700 });
  await chmod(root, 0o700);
  for (const directory of fixedDirectories) {
    // leave one exact required child absent for the selected refusal fixture
    if (directory === fixture.missingDir) continue;
    const path = join(root, directory);
    await mkdir(path, { mode: 0o700 });
    await chmod(path, 0o700);
  }
  const acknowledgements = join(root, "revision-custody-acknowledgements");
  await writeDocument(join(acknowledgements, "current.json"), fixture.ackCurrent);
  await writeDocument(join(acknowledgements, "previous.json"), fixture.ackPrevious);
  const anchors = join(root, fixture.action === "dev"
    ? "development-custody-anchors"
    : "rain-control-custody-anchors");
  if (fixture.beforeRoot === "present") {
    await mkdir(anchors, { mode: 0o700 });
    await chmod(anchors, 0o700);
    await writeDocument(join(anchors, "current.json"), fixture.current);
    await writeDocument(join(anchors, "pending.json"), fixture.pending);
    await writeDocument(join(anchors, "previous.json"), fixture.previous);
  }
  return anchors;
}

// return every retained fixed anchor slot exactly as a reusable document identity
async function requireAnchorState(directory, names, label) {
  const slots = ["current", "pending", "previous"];
  const expectedNames = slots.filter((_, index) => names[index] !== "-")
    .map((slot) => `${slot}.json`).sort();
  if (expectedNames.length === 0 && !await exists(directory)) return;
  assert.deepEqual((await readdir(directory)).sort(), expectedNames, label);
  for (const [index, slot] of slots.entries()) {
    if (names[index] !== "-") {
      assert.deepEqual(
        await readFile(join(directory, `${slot}.json`)),
        canonicalBytes(documents[names[index]]),
        label,
      );
    }
  }
}

// invoke one actual original install command and assert its entire resulting anchor state
async function observe(root, fixture) {
  const evidence = join(root, fixture.name);
  const anchorDirectory = await initializeEvidenceRoot(evidence, fixture);
  const inputBytes = canonicalBytes(documents[fixture.input]);
  const chunks = [];
  let failed = false;
  try {
    await runAdjustmentArchiveCommand([
      fixture.action === "dev"
        ? "development-custody-anchor-install-v1"
        : "rain-control-custody-anchor-install-v1",
      fixture.argument,
    ], {
      expectedControlSha256: fixture.expectedControlSha256,
      expectedControlVersion: fixture.expectedControlVersion,
      readCaptureEpochWitness: async () => documents[fixture.witness],
      root: evidence,
      stdinBytes: inputBytes,
      stdout: { write: (chunk) => chunks.push(Buffer.from(chunk)) },
    });
  } catch {
    failed = true;
  }
  const output = Buffer.concat(chunks);
  if (fixture.expected === "error") {
    assert.equal(failed, true, fixture.name);
    assert.equal(output.length, 0, fixture.name);
  } else {
    assert.equal(failed, false, fixture.name);
    assert.deepEqual(output, canonicalBytes(documents[fixture.expected]), fixture.name);
  }
  assert.equal(await exists(anchorDirectory), fixture.afterRoot === "present", fixture.name);
  await requireAnchorState(anchorDirectory, fixture.after, fixture.name);
  // installation never changes either retained custody acknowledgement
  const acknowledgements = join(evidence, "revision-custody-acknowledgements");
  for (const [slot, name] of [["current", fixture.ackCurrent], ["previous", fixture.ackPrevious]]) {
    if (name !== "-") {
      assert.deepEqual(
        await readFile(join(acknowledgements, `${slot}.json`)),
        canonicalBytes(documents[name]),
        fixture.name,
      );
    }
  }
  console.log([
    "case", fixture.name, fixture.action, fixture.expectedControlSha256,
    fixture.expectedControlVersion, fixture.argument, fixture.input, fixture.witness,
    fixture.ackCurrent, fixture.ackPrevious, fixture.beforeRoot, fixture.missingDir,
    fixture.current, fixture.pending, fixture.previous, fixture.expected,
    fixture.afterRoot, ...fixture.after,
  ].join("\t"));
}

const cases = [
  { name: "development_first", action: "dev", input: "development",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "-", pending: "-",
    previous: "-", expected: "developmentInstallation",
    after: ["development", "-", "-"] },
  { name: "development_retry", action: "dev", input: "development",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "development", pending: "-",
    previous: "-", expected: "developmentInstallation",
    after: ["development", "-", "-"] },
  { name: "development_pending_retry", action: "dev", input: "development",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "-", pending: "development",
    previous: "-", expected: "developmentInstallation",
    after: ["development", "-", "-"] },
  { name: "development_successor", action: "dev", input: "developmentNext",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "development", pending: "-",
    previous: "-", expected: "developmentNextInstallation",
    after: ["developmentNext", "-", "development"] },
  { name: "development_pending_collision", action: "dev", input: "development",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "development", pending: "developmentNext",
    previous: "-", expected: "error", after: ["development", "developmentNext", "-"] },
  { name: "development_missing_ack", action: "dev", input: "development",
    ackCurrent: "-", ackPrevious: "-", current: "-", pending: "-", previous: "-",
    expected: "error", after: ["-", "-", "-"] },
  { name: "development_missing_required_directory", action: "dev", input: "development",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "-", pending: "-",
    previous: "-", expected: "error", missingDir: "revision-gaps",
    after: ["-", "-", "-"] },
  { name: "development_v1_only", action: "dev", input: "development",
    ackCurrent: "acknowledgementV1", ackPrevious: "-", current: "-", pending: "-",
    previous: "-", expected: "error", after: ["-", "-", "-"] },
  { name: "development_mixed_history", action: "dev", input: "developmentMixed",
    ackCurrent: "acknowledgementMixed", ackPrevious: "acknowledgementV1", current: "-",
    pending: "-", previous: "-", expected: "developmentMixedInstallation",
    after: ["developmentMixed", "-", "-"] },
  { name: "development_broken_ack_predecessor", action: "dev", input: "developmentMixed",
    ackCurrent: "acknowledgementMixed", ackPrevious: "-", current: "-", pending: "-",
    previous: "-", expected: "error", after: ["-", "-", "-"] },
  { name: "development_custody_mismatch", action: "dev", input: "developmentWrongCustody",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "-", pending: "-",
    previous: "-", expected: "error", after: ["-", "-", "-"] },
  { name: "development_witness_mismatch", action: "dev", input: "developmentWrongWitness",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "-", pending: "-",
    previous: "-", expected: "error", after: ["-", "-", "-"] },
  { name: "rain_first", action: "rain", input: "rain", ackCurrent: "acknowledgementV2",
    ackPrevious: "-", current: "-", pending: "-", previous: "-",
    expected: "rainInstallation", beforeRoot: "present", after: ["rain", "-", "-"] },
  { name: "rain_retry_with_pending", action: "rain", input: "rain",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "rain", pending: "rain",
    previous: "-", expected: "rainInstallation", after: ["rain", "-", "-"] },
  { name: "rain_pending_retry", action: "rain", input: "rain",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "-", pending: "rain",
    previous: "-", expected: "rainInstallation", after: ["rain", "-", "-"] },
  { name: "rain_successor", action: "rain", input: "rainNext",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "rain", pending: "-",
    previous: "-", expected: "rainNextInstallation", after: ["rainNext", "-", "rain"] },
  { name: "rain_pending_collision", action: "rain", input: "rain",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "rain", pending: "rainNext",
    previous: "-", expected: "error", after: ["rain", "rainNext", "-"] },
  { name: "rain_wrong_control", action: "rain", input: "rainWrongControl",
    ackCurrent: "acknowledgementV2", ackPrevious: "-", current: "-", pending: "-",
    previous: "-", expected: "error", after: ["-", "-", "-"] },
  { name: "rain_missing_ack", action: "rain", input: "rain",
    ackCurrent: "-", ackPrevious: "-", current: "-", pending: "-", previous: "-",
    expected: "error", after: ["-", "-", "-"] },
  { name: "rain_v1_only", action: "rain", input: "rain",
    ackCurrent: "acknowledgementV1", ackPrevious: "-", current: "-", pending: "-",
    previous: "-", expected: "error", after: ["-", "-", "-"] },
  { name: "rain_selects_v2_previous", action: "rain", input: "rain",
    ackCurrent: "acknowledgementLaterV1", ackPrevious: "acknowledgementV2", current: "-",
    pending: "-", previous: "-", expected: "rainInstallation",
    after: ["rain", "-", "-"] },
].map((fixture) => ({
  ...fixture,
  beforeRoot: fixture.beforeRoot ??
    ([fixture.current, fixture.pending, fixture.previous].some((name) => name !== "-")
      ? "present" : "absent"),
  missingDir: fixture.missingDir ?? "-",
  argument: sha256(canonicalBytes(documents[fixture.input])),
  afterRoot: fixture.expected !== "error" || fixture.beforeRoot === "present" ||
    [fixture.current, fixture.pending, fixture.previous].some((name) => name !== "-")
    ? "present" : "absent",
  expectedControlSha256: controlSha256,
  expectedControlVersion: controlVersion,
  witness: "witness",
}));

// expose every reusable fixture before its cases
for (const [name, value] of Object.entries(documents)) emitDocument(name, value);
// expose every exact fixture-owned content preimage needed by the Rust codec
for (const [preimage, digest] of hashPreimages) console.log(["hash", preimage, digest].join("\t"));
// expose exact javascript clock resolution for every fixture instant
for (const clock of clocks) console.log(["clock", clock, Date.parse(clock)].join("\t"));

const root = await mkdtemp(join(tmpdir(), "weather-custody-anchor-install-oracle-"));
try {
  // isolate every actual command invocation in its own private fixture root
  for (const fixture of cases) await observe(root, fixture);
} finally {
  await rm(root, { recursive: true, force: true });
}
