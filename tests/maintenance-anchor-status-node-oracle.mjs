import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmod,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  runAdjustmentArchiveCommand,
} from "../deploy/scripts/adjustment-evidence-store.mjs";

const SLOT_NAMES = ["current", "pending", "previous"];

// recursively reproduce the original command's canonical object ordering
function canonicalValue(value) {
  // retain array order while sorting any objects nested inside it
  if (Array.isArray(value)) {
    return value.map(canonicalValue);
  }
  // sort only ordinary JSON objects
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value)
      .sort(([left], [right]) => left.localeCompare(right, "en"))
      .map(([key, item]) => [key, canonicalValue(item)]));
  }
  return value;
}

// encode one recursively sorted LF-terminated document
function canonicalBytes(value) {
  return Buffer.from(`${JSON.stringify(canonicalValue(value))}\n`);
}

// hash exact fixture or output bytes with Node's original primitive
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// construct one complete legacy transferred anchor
function legacyTransferred() {
  return {
    actionSha256: null,
    archiveObjectSha256: "1".repeat(64),
    catalogGeneration: "1",
    catalogRootSha256: "2".repeat(64),
    catalogWatermarkSha256: "3".repeat(64),
    contractVersion: "adjustment-maintenance-anchor/v2",
    controlSha256: "4".repeat(64),
    controlVersion: "13",
    ctfState: "transferred",
    dueKey: "capture/2026-10-08T00:35:00.000Z",
    frontierCount: 7,
    frontierRootSha256: "5".repeat(64),
    fullGraphVerifiedAt: "2026-10-08T00:40:00.000Z",
    fullMemberRootSha256: null,
    generation: "1",
    graphManifestSha256: "6".repeat(64),
    lifecycleLedgerRootSha256: "7".repeat(64),
    manifestSha256: "8".repeat(64),
    predecessorAnchorSha256: null,
    publishedAt: "2026-10-08T00:41:00.000Z",
    reportSha256: null,
    revisionSnapshotRootSha256: null,
    sequence: "0",
    sourceCommit: "9".repeat(40),
    workstationJournalHeadSha256: "a".repeat(64),
  };
}

// extend the legacy anchor with its complete finalized schema
function legacyFinalized() {
  return {
    ...legacyTransferred(),
    actionSha256: "b".repeat(64),
    archiveCommitOrdinal: "12",
    burnSha256: "c".repeat(64),
    catalogRevisionReceiptRootSha256: "d".repeat(64),
    confirmationChunkCount: 24,
    ctfState: "finalized",
    finalizationProofSha256: "e".repeat(64),
    finalizedAt: "2026-10-08T00:42:00.000Z",
    fullMemberRootSha256: "f".repeat(64),
    inputSealSha256: "0".repeat(64),
    reportSha256: "1".repeat(64),
    requiredInputRootSha256: "2".repeat(64),
    retirementSetRootSha256: "3".repeat(64),
    revisionSnapshotRootSha256: "4".repeat(64),
  };
}

// construct one complete future-only transferred anchor
function futureTransferred() {
  return {
    actionSha256: "1".repeat(64),
    archiveCommitOrdinal: "24",
    burnSha256: "2".repeat(64),
    candidateArtifactRootSha256: "3".repeat(64),
    candidateReportSha256: "4".repeat(64),
    captureEpochWitnessSha256: "5".repeat(64),
    confirmationAccessSha256: "6".repeat(64),
    confirmationChunkCount: 27,
    contractVersion: "adjustment-maintenance-anchor/v3",
    controlSha256: "7".repeat(64),
    controlVersion: 14,
    ctfState: "transferred",
    custodyCheckpointSha256: "8".repeat(64),
    dueKey: `confirmation/wind/${"9".repeat(64)}`,
    family: "wind",
    frontierSha256: "a".repeat(64),
    fullGraphVerifiedAt: "2026-10-08T02:12:00.000Z",
    fullMemberRootSha256: "b".repeat(64),
    graphManifestSha256: "c".repeat(64),
    inputSealSha256: "d".repeat(64),
    lifecycleLedgerRootSha256: "e".repeat(64),
    pageSha256: "f".repeat(64),
    policyReportSha256: "0".repeat(64),
    predecessorAnchorSha256: null,
    publishedAt: "2026-10-08T02:13:00.000Z",
    requiredInputRootSha256: "1".repeat(64),
    sequence: "0",
    sourceCommit: "2".repeat(40),
    workstationJournalHeadSha256: "3".repeat(64),
  };
}

// extend the future-only anchor with its additive finalization fields
function futureFinalized() {
  return {
    ...futureTransferred(),
    ctfState: "finalized",
    finalizationProofSha256: "4".repeat(64),
    finalizedAt: "2026-10-08T02:14:00.000Z",
  };
}

// emit one tab-safe fixture row
function row(...values) {
  process.stdout.write(`${values.join("\t")}\n`);
}

// expose exact canonical bytes and Node-decoded scalar fields
function emitMaterial(name, bytes) {
  const value = JSON.parse(bytes);
  row("material", name, bytes.toString("hex"));
  // export every scalar without adding a Rust JSON parser
  for (const [key, field] of Object.entries(value)) {
    // preserve null distinctly from strings and numbers
    if (field === null) {
      row("field", name, key, "null", "");
    } else if (typeof field === "string") {
      row("field", name, key, "string", Buffer.from(field).toString("hex"));
    } else if (typeof field === "number") {
      row("field", name, key, "number", String(field));
    } else {
      throw new TypeError(`unsupported fixture field ${key}`);
    }
  }
  row("hash", name, sha256(bytes));
}

// create one independent evidence root and its optional fixed slot directory
async function fixtureRoot(parent, name, slots = null, extras = {}) {
  const evidenceRoot = join(parent, name);
  const anchorRoot = join(evidenceRoot, "maintenance-anchors");
  await mkdir(evidenceRoot, { mode: 0o700 });
  // leave the future root wholly absent for the genesis case
  if (slots === null) {
    return { anchorRoot, evidenceRoot };
  }
  await mkdir(anchorRoot, { mode: 0o700 });
  await chmod(anchorRoot, 0o700);
  // install only the supplied fixed slots with the original private mode
  for (const [slot, bytes] of Object.entries(slots)) {
    assert.ok(SLOT_NAMES.includes(slot));
    await writeFile(join(anchorRoot, `${slot}.json`), bytes, { mode: 0o600 });
    await chmod(join(anchorRoot, `${slot}.json`), 0o600);
  }
  // add only explicit ordinary foreign entries for closed-root testing
  for (const [nameValue, bytes] of Object.entries(extras)) {
    await writeFile(join(anchorRoot, nameValue), bytes, { mode: 0o600 });
    await chmod(join(anchorRoot, nameValue), 0o600);
  }
  return { anchorRoot, evidenceRoot };
}

// capture names, modes and bytes while ignoring read-only access-time changes
async function fixtureState(anchorRoot) {
  let names;
  // distinguish a genuinely absent root from an initialized empty root
  try {
    names = (await readdir(anchorRoot)).sort();
  } catch (error) {
    // accept only absence in this test-only observation
    if (error?.code === "ENOENT") {
      return null;
    }
    throw error;
  }
  const rootDetails = await lstat(anchorRoot);
  const entries = [];
  // retain each exact file payload around the supposedly read-only command
  for (const name of names) {
    const path = join(anchorRoot, name);
    const details = await lstat(path);
    entries.push({
      bytes: (await readFile(path)).toString("hex"),
      mode: details.mode & 0o777,
      name,
      size: details.size,
    });
  }
  return { entries, mode: rootDetails.mode & 0o777 };
}

// invoke the unchanged command and prove that it did not alter its fixture
async function observe(name, argv, root) {
  const before = await fixtureState(root.anchorRoot);
  const output = [];
  let failure = null;
  // capture failures without allowing diagnostics onto stdout
  try {
    await runAdjustmentArchiveCommand(argv, {
      root: root.evidenceRoot,
      stdout: { write: (bytes) => output.push(Buffer.from(bytes)) },
    });
  } catch (error) {
    failure = error;
  }
  assert.deepEqual(await fixtureState(root.anchorRoot), before);
  return {
    error: failure === null ? "" : String(failure.message),
    name,
    output: Buffer.concat(output),
    status: failure === null ? "ok" : "error",
  };
}

// emit a full command result and its independently decoded public projection
function emitResult(result) {
  row("result", result.name, result.status, result.output.toString("hex"),
    Buffer.from(result.error).toString("hex"));
  // failures carry no public status projection
  if (result.status !== "ok") {
    return;
  }
  const status = JSON.parse(result.output);
  row("status", result.name, status.privacyState, status.rootState,
    status.schemaReadiness);
  // export every fixed slot projection in public order
  for (const slot of SLOT_NAMES) {
    const value = status.slots[slot];
    row("slot", result.name, slot, value.state,
      value.sha256 ?? "-", value.sizeBytes ?? "-");
  }
}

const legacyTransferredBytes = canonicalBytes(legacyTransferred());
const legacyFinalizedBytes = canonicalBytes(legacyFinalized());
const futureTransferredBytes = canonicalBytes(futureTransferred());
const futureFinalizedBytes = canonicalBytes(futureFinalized());
const malformedBytes = canonicalBytes({ ...legacyTransferred(), frontierCount: 0 });
const materials = {
  future_finalized: futureFinalizedBytes,
  future_transferred: futureTransferredBytes,
  legacy_finalized: legacyFinalizedBytes,
  legacy_transferred: legacyTransferredBytes,
  malformed: malformedBytes,
};

// export every anchor once for the dependency-free Rust fixture codec
for (const [name, bytes] of Object.entries(materials)) {
  emitMaterial(name, bytes);
}
const clocks = new Set(Object.values(materials).flatMap(
  (bytes) => Object.entries(JSON.parse(bytes))
    .filter(([key, value]) => key.endsWith("At") && typeof value === "string")
    .map(([, value]) => value),
));
// bind each original instant spelling to Node's millisecond ordering
for (const clock of clocks) {
  row("clock", Buffer.from(clock).toString("hex"), String(Date.parse(clock)));
}

const root = await mkdtemp(join(tmpdir(), "weather-maintenance-anchor-status-oracle-"));
try {
  const absent = await fixtureRoot(root, "absent");
  const empty = await fixtureRoot(root, "empty", {});
  const legacy = await fixtureRoot(root, "legacy", { current: legacyTransferredBytes });
  const mixed = await fixtureRoot(root, "mixed", {
    current: legacyFinalizedBytes,
    pending: futureTransferredBytes,
    previous: legacyTransferredBytes,
  });
  const pendingOnly = await fixtureRoot(root, "pending-only", {
    pending: futureTransferredBytes,
  });
  const future = await fixtureRoot(root, "future", { current: futureFinalizedBytes });
  const malformed = await fixtureRoot(root, "malformed", { current: malformedBytes });
  const unknown = await fixtureRoot(root, "unknown", {}, {
    "unexpected.json": Buffer.from("{}\n"),
  });
  const results = [
    await observe("absent_root", ["maintenance-anchor-status-v2"], absent),
    await observe("empty_root", ["maintenance-anchor-status-v2"], empty),
    await observe("legacy_transferred", ["maintenance-anchor-status-v2"], legacy),
    await observe("mixed_slots", ["maintenance-anchor-status-v2"], mixed),
    await observe("pending_only", ["maintenance-anchor-status-v2"], pendingOnly),
    await observe("future_finalized", ["maintenance-anchor-status-v2"], future),
    await observe("malformed_schema", ["maintenance-anchor-status-v2"], malformed),
    await observe("unknown_entry", ["maintenance-anchor-status-v2"], unknown),
    await observe("arguments", ["maintenance-anchor-status-v2", "extra"], absent),
  ];
  assert.deepEqual(results.map(({ name, status }) => [name, status]), [
    ["absent_root", "ok"],
    ["empty_root", "ok"],
    ["legacy_transferred", "ok"],
    ["mixed_slots", "ok"],
    ["pending_only", "ok"],
    ["future_finalized", "ok"],
    ["malformed_schema", "error"],
    ["unknown_entry", "error"],
    ["arguments", "error"],
  ]);
  assert.equal(results[6].error, "maintenance anchor fields are invalid");
  assert.equal(results[7].error, "maintenance anchor root contains an unknown entry");
  assert.equal(results[8].error, "maintenance-anchor-status-v2 takes no arguments");
  // retain only command and projection evidence after every assertion passes
  for (const result of results) {
    emitResult(result);
  }
} finally {
  await rm(root, { force: true, recursive: true });
}
