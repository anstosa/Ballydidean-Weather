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

// compute one original lowercase content identity
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// create stable distinct fixture identities without aliases
function identity(label) {
  return sha256(Buffer.from(`custody-anchor-current-oracle/${label}`, "utf8"));
}

// create one fully bound valid development custody anchor
function developmentAnchor(overrides = {}) {
  return {
    actionKind: "shadow",
    actionSha256: identity("development/action"),
    archiveCommitOrdinal: "1",
    artifactSha256: identity("development/artifact"),
    candidateGraphSha256: identity("development/candidate-graph"),
    candidateSha256: identity("development/candidate"),
    captureEpochWitnessSha256: identity("development/epoch"),
    contractVersion: "adjustment-development-custody-anchor/v1",
    controlSha256: identity("development/control"),
    controlVersion: "17",
    custodyAcknowledgedAt: clocks[0],
    custodyAcknowledgementSha256: identity("development/custody-ack"),
    custodyCheckpointSha256: identity("development/custody-checkpoint"),
    developmentGraphSha256: identity("development/graph"),
    dueKey: "monthly/temperature/2026-10",
    family: "temperature",
    frontierSha256: identity("development/frontier"),
    fullGraphVerifiedAt: clocks[1],
    inputHeadSha256: identity("development/input-head"),
    lifecycleLedgerRootSha256: identity("development/lifecycle"),
    memberRootSha256: identity("development/member-root"),
    pageSha256: identity("development/page"),
    policyReportSha256: identity("development/policy"),
    predecessorAnchorSha256: null,
    registrationSha256: identity("development/registration"),
    sequence: "0",
    sourceCommit: "b".repeat(40),
    sourceSha256: identity("development/source"),
    startMemberSha256: null,
    startSha256: identity("development/start"),
    ...overrides,
  };
}

// create one fully bound valid rain control-reference custody anchor
function rainAnchor(overrides = {}) {
  return {
    actionKind: "control_reference",
    actionSha256: identity("rain/action"),
    archiveCommitOrdinal: "1",
    captureEpochWitnessSha256: identity("rain/epoch"),
    contractVersion: "adjustment-rain-control-custody-anchor/v1",
    controlSha256: identity("rain/control"),
    controlStateSha256: identity("rain/control-state"),
    controlVersion: "17",
    custodyAcknowledgedAt: clocks[0],
    custodyAcknowledgementSha256: identity("rain/custody-ack"),
    custodyCheckpointSha256: identity("rain/custody-checkpoint"),
    dueMonth: "2026-10",
    fencingToken: "1",
    frontierSha256: identity("rain/frontier"),
    fullGraphVerifiedAt: clocks[1],
    graphManifestSha256: identity("rain/graph-manifest"),
    memberRootSha256: identity("rain/member-root"),
    ordinalArtifactSha256: identity("rain/ordinal-artifact"),
    pageSha256: identity("rain/page"),
    predecessorAnchorSha256: null,
    sequence: "0",
    sourceCommit: "c".repeat(40),
    sourceMemberRootSha256: identity("rain/source-member-root"),
    sourceReceiptRootSha256: identity("rain/source-receipt-root"),
    startMemberSha256: null,
    startSha256: identity("rain/start"),
    workstationJournalHeadSha256: identity("rain/journal"),
    ...overrides,
  };
}

const development = developmentAnchor();
const developmentCoerced = developmentAnchor({
  controlVersion: 17,
  dueKey: ["monthly/temperature/2026-10"],
});
const rain = rainAnchor();
const rainCoerced = rainAnchor({
  controlVersion: 17,
  dueMonth: ["2026-10"],
});
const documents = {
  development,
  developmentCoerced,
  developmentInvalidPredecessor: developmentAnchor({ sequence: "1" }),
  developmentInvalidTime: developmentAnchor({ fullGraphVerifiedAt: clocks[0],
    custodyAcknowledgedAt: clocks[1] }),
  developmentMalformed: developmentAnchor({ actionKind: "rain" }),
  rain,
  rainCoerced,
  rainInvalidPredecessor: rainAnchor({ sequence: "1" }),
  rainInvalidTime: rainAnchor({ fullGraphVerifiedAt: clocks[0],
    custodyAcknowledgedAt: clocks[1] }),
  rainMalformed: rainAnchor({ dueMonth: "2026-13" }),
  null: null,
};

// add the exact path-free current projection for every successful current anchor
// bind every successful anchor to its exact canonical current identity
for (const name of ["development", "developmentCoerced", "rain", "rainCoerced"]) {
  const bytes = canonicalBytes(documents[name]);
  documents[`${name}Current`] = {
    anchor: documents[name],
    anchorSha256: sha256(bytes),
  };
}

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
  emitNode(name, ".", canonical(value));
}

// distinguish genuine absence without creating the queried path
async function exists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

// populate only one selected fixed private slot
async function writeSlot(directory, slot, documentName) {
  // leave intentional absence untouched
  if (documentName === "-") return;
  const path = join(directory, `${slot}.json`);
  await writeFile(path, canonicalBytes(documents[documentName]), { mode: 0o600 });
  await chmod(path, 0o600);
}

// invoke one original current command and assert the complete namespace remains unchanged
async function observe(root, fixture) {
  const evidence = join(root, fixture.name);
  const directoryName = fixture.action === "dev"
    ? "development-custody-anchors"
    : "rain-control-custody-anchors";
  const directory = join(evidence, directoryName);
  await mkdir(evidence, { mode: 0o700 });
  await chmod(evidence, 0o700);
  // create only the fixture-selected private anchor root
  if (fixture.present) {
    await mkdir(directory, { mode: 0o700 });
    await chmod(directory, 0o700);
    await writeSlot(directory, "current", fixture.current);
    await writeSlot(directory, "pending", fixture.pending);
    await writeSlot(directory, "previous", fixture.previous);
  }
  const before = new Map();
  // retain every original namespace byte for the post-read assertion
  if (fixture.present) {
    // read each fixed fixture entry before invoking the command
    for (const name of await readdir(directory)) {
      before.set(name, await readFile(join(directory, name)));
    }
  }
  const chunks = [];
  let failed = false;
  try {
    const action = fixture.action === "dev"
      ? "development-custody-anchor-current-v1"
      : "rain-control-custody-anchor-current-v1";
    await runAdjustmentArchiveCommand([action], {
      root: evidence,
      stdout: { write: (chunk) => chunks.push(Buffer.from(chunk)) },
    });
  } catch {
    failed = true;
  }
  const output = Buffer.concat(chunks);
  // compare either the exact source refusal or exact canonical stdout
  if (fixture.expected === "error") {
    assert.equal(failed, true, fixture.name);
    assert.equal(output.length, 0, fixture.name);
  } else {
    assert.equal(failed, false, fixture.name);
    assert.deepEqual(output, canonicalBytes(documents[fixture.expected]), fixture.name);
  }
  // forbid directory creation, recovery, rotation, or any byte change
  if (!fixture.present) {
    assert.equal(await exists(directory), false, fixture.name);
  } else {
    const afterNames = (await readdir(directory)).sort();
    assert.deepEqual(afterNames, [...before.keys()].sort(), fixture.name);
    // compare every retained entry rather than only the selected current slot
    for (const [name, bytes] of before) {
      assert.deepEqual(await readFile(join(directory, name)), bytes, fixture.name);
    }
  }
  console.log([
    "case", fixture.name, fixture.action, fixture.present ? "present" : "absent",
    fixture.current, fixture.pending, fixture.previous, fixture.expected,
    fixture.current, fixture.pending, fixture.previous,
  ].join("\t"));
}

const cases = [
  { name: "development_absent", action: "dev", present: false, current: "-",
    pending: "-", previous: "-", expected: "null" },
  { name: "development_empty", action: "dev", present: true, current: "-",
    pending: "-", previous: "-", expected: "null" },
  { name: "development_pending_only", action: "dev", present: true, current: "-",
    pending: "development", previous: "-", expected: "null" },
  { name: "development_current", action: "dev", present: true,
    current: "development", pending: "-", previous: "-",
    expected: "developmentCurrent" },
  { name: "development_coerced", action: "dev", present: true,
    current: "developmentCoerced", pending: "-", previous: "development",
    expected: "developmentCoercedCurrent" },
  { name: "development_invalid_predecessor", action: "dev", present: true,
    current: "developmentInvalidPredecessor", pending: "-", previous: "-", expected: "error" },
  { name: "development_invalid_time", action: "dev", present: true,
    current: "developmentInvalidTime", pending: "-", previous: "-", expected: "error" },
  { name: "development_malformed", action: "dev", present: true,
    current: "developmentMalformed", pending: "-", previous: "-", expected: "error" },
  { name: "rain_absent", action: "rain", present: false, current: "-",
    pending: "-", previous: "-", expected: "null" },
  { name: "rain_empty", action: "rain", present: true, current: "-",
    pending: "-", previous: "-", expected: "null" },
  { name: "rain_pending_only", action: "rain", present: true, current: "-",
    pending: "rain", previous: "-", expected: "null" },
  { name: "rain_current", action: "rain", present: true, current: "rain",
    pending: "-", previous: "-", expected: "rainCurrent" },
  { name: "rain_coerced", action: "rain", present: true, current: "rainCoerced",
    pending: "-", previous: "rain", expected: "rainCoercedCurrent" },
  { name: "rain_invalid_predecessor", action: "rain", present: true,
    current: "rainInvalidPredecessor", pending: "-", previous: "-", expected: "error" },
  { name: "rain_invalid_time", action: "rain", present: true,
    current: "rainInvalidTime", pending: "-", previous: "-", expected: "error" },
  { name: "rain_malformed", action: "rain", present: true,
    current: "rainMalformed", pending: "-", previous: "-", expected: "error" },
];

// expose every reusable fixture before its cases
for (const [name, value] of Object.entries(documents)) emitDocument(name, value);
// retain every original current identity preimage for the independent Rust codec
for (const name of ["development", "developmentCoerced", "rain", "rainCoerced"]) {
  const preimage = canonicalBytes(documents[name]);
  console.log(["hash", preimage.toString("hex"), sha256(preimage)].join("\t"));
}
// expose exact javascript clock resolution for every fixture instant
for (const clock of clocks) console.log(["clock", clock, Date.parse(clock)].join("\t"));

const root = await mkdtemp(join(tmpdir(), "weather-custody-anchor-current-oracle-"));
try {
  // isolate every actual command invocation in its own private fixture root
  for (const fixture of cases) await observe(root, fixture);
} finally {
  await rm(root, { recursive: true, force: true });
}
