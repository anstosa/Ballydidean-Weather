import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runAdjustmentArchiveCommand } from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const hash = "a".repeat(64);
const graphAt = "2026-10-08T02:00:00.000Z";
const publishedAt = "2026-10-08T02:01:00.000Z";
const finalizedAt = "2026-10-08T02:02:00.000Z";

// encode fixture fields in the original canonical key order
function canonical(value) {
  // keep arrays in their original order
  if (Array.isArray(value)) return value.map(canonical);
  // recursively sort object keys before retaining scalar values
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  }
  return value;
}

// retain the exact one-lf fixture framing
function bytes(value) {
  return Buffer.from(`${JSON.stringify(canonical(value))}\n`);
}

// populate the original named hash fields without inventing different identities
function hashes(names) {
  return Object.fromEntries(names.map((name) => [name, hash]));
}

const v2 = {
  ...hashes(["archiveObjectSha256", "catalogRootSha256", "catalogWatermarkSha256",
    "controlSha256", "frontierRootSha256", "graphManifestSha256",
    "lifecycleLedgerRootSha256", "manifestSha256", "workstationJournalHeadSha256"]),
  actionSha256: null, catalogGeneration: "1", contractVersion: "adjustment-maintenance-anchor/v2",
  controlVersion: 14, ctfState: "transferred", dueKey: "capture/2026-10-08T00:35:00.000Z",
  frontierCount: 1, fullGraphVerifiedAt: graphAt, fullMemberRootSha256: null,
  generation: "1", predecessorAnchorSha256: null, publishedAt, reportSha256: null,
  revisionSnapshotRootSha256: null, sequence: "0", sourceCommit: "b".repeat(40),
};
const v2Final = {
  ...v2,
  ...hashes(["actionSha256", "fullMemberRootSha256", "reportSha256",
    "revisionSnapshotRootSha256", "burnSha256", "catalogRevisionReceiptRootSha256",
    "finalizationProofSha256", "inputSealSha256", "requiredInputRootSha256",
    "retirementSetRootSha256"]),
  archiveCommitOrdinal: "1", confirmationChunkCount: 24, ctfState: "finalized", finalizedAt,
};
const v3 = {
  ...hashes(["actionSha256", "burnSha256", "candidateArtifactRootSha256",
    "candidateReportSha256", "captureEpochWitnessSha256", "confirmationAccessSha256",
    "controlSha256", "custodyCheckpointSha256", "frontierSha256", "fullMemberRootSha256",
    "graphManifestSha256", "inputSealSha256", "lifecycleLedgerRootSha256", "pageSha256",
    "policyReportSha256", "requiredInputRootSha256", "workstationJournalHeadSha256"]),
  archiveCommitOrdinal: "0", confirmationChunkCount: 24,
  contractVersion: "adjustment-maintenance-anchor/v3", controlVersion: "17",
  ctfState: "transferred", dueKey: [`confirmation/rain/${hash}`], family: "rain",
  fullGraphVerifiedAt: graphAt, predecessorAnchorSha256: null, publishedAt,
  sequence: "0", sourceCommit: "b".repeat(40),
};
const v3Final = {
  ...v3, ctfState: "finalized", finalizationProofSha256: hash, finalizedAt,
};
const anchors = {
  v2, v2Final, v3, v3Final,
  v2Malformed: { ...v2, frontierCount: 0 },
  v3Malformed: { ...v3, sequence: "1" },
};

// emit the decoded fixture values and hashes for the independent Rust codec fixture
function emitAnchor(name, value) {
  const contents = bytes(value);
  console.log(["anchor", name, contents.toString().trimEnd(),
    createHash("sha256").update(contents).digest("hex")].join("\t"));
  // retain the exact typed values used by the source validator
  for (const [key, item] of Object.entries(value)) {
    const kind = item === null ? "null" : Array.isArray(item) ? "array" : typeof item;
    console.log(["field", name, key, kind, item === null ? "-" : String(item)].join("\t"));
  }
}

// observe actual Node status from disposable private directories without mutating them
async function observe(root, name, present, slots, extra = false) {
  const evidence = join(root, name);
  await mkdir(evidence);
  const directory = join(evidence, "maintenance-anchors");
  // create only the fixture's selected private root and fixed slots
  if (present) {
    await mkdir(directory, { mode: 0o700 });
    await chmod(directory, 0o700);
    // preserve the exact selected slot material
    for (const [index, slot] of ["current", "pending", "previous"].entries()) {
      // leave genuinely absent slots uncreated
      if (slots[index] !== "-") {
        const path = join(directory, `${slot}.json`);
        await writeFile(path, bytes(anchors[slots[index]]));
        await chmod(path, 0o600);
      }
    }
    // retain one ordinary unexpected directory entry fixture
    if (extra) await writeFile(join(directory, "unexpected.txt"), "fixture");
  }
  let result;
  try {
    const chunks = [];
    await runAdjustmentArchiveCommand(["maintenance-anchor-status-v2"], {
      root: evidence,
      stdout: { write: (chunk) => chunks.push(Buffer.from(chunk)) },
    });
    result = Buffer.concat(chunks).toString().trimEnd();
  } catch {
    result = "error";
  }
  // reject any accidental write, recovery or rotation by the original status call
  if (present) {
    const names = await readdir(directory);
    assert.equal(names.length, slots.filter((slot) => slot !== "-").length + Number(extra));
    // compare the retained bytes after that read-only command
    for (const [index, slot] of ["current", "pending", "previous"].entries()) {
      // skip genuine absence
      if (slots[index] !== "-") {
        assert.deepEqual(await readFile(join(directory, `${slot}.json`)), bytes(anchors[slots[index]]));
      }
    }
  }
  console.log(["case", name, present ? "present" : "absent", ...slots, extra ? "extra" : "-", result].join("\t"));
}

for (const [name, value] of Object.entries(anchors)) emitAnchor(name, value);
for (const clock of [graphAt, publishedAt, finalizedAt]) console.log(["clock", clock, Date.parse(clock)].join("\t"));
const root = await mkdtemp(join(tmpdir(), "weather-anchor-status-oracle-"));
try {
  await observe(root, "absent", false, ["-", "-", "-"]);
  await observe(root, "empty", true, ["-", "-", "-"]);
  await observe(root, "v2_current", true, ["v2", "-", "-"]);
  await observe(root, "mixed", true, ["v3Final", "v3", "v2Final"]);
  await observe(root, "pending_only", true, ["-", "v3", "-"]);
  await observe(root, "v2_final", true, ["v2Final", "-", "v2"]);
  await observe(root, "malformed_v2", true, ["v2Malformed", "-", "-"]);
  await observe(root, "malformed_v3", true, ["v3Malformed", "-", "-"]);
  await observe(root, "unknown_entry", true, ["-", "-", "-"], true);
} finally {
  await rm(root, { recursive: true, force: true });
}
