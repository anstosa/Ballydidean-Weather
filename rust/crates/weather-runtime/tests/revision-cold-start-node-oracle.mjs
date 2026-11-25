import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { runAdjustmentArchiveCommand } from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const COLD_DIRECTORIES = [
  "revision-commit-receipts",
  "revision-frontier-successors",
  "revision-projections",
  "shadow-revision-capsules",
  "rain-control-states",
  "rain-control-state-stage-receipts",
];

// encode the recursively sorted predecessor document with its retained LF
function canonicalBytes(value) {
  const canonical = (entry) => {
    if (Array.isArray(entry)) {
      return entry.map(canonical);
    }
    if (entry !== null && typeof entry === "object") {
      return Object.fromEntries(Object.keys(entry).sort().map((key) => [key, canonical(entry[key])]));
    }
    return entry;
  };
  return Buffer.from(`${JSON.stringify(canonical(value))}\n`);
}

// compute one lowercase SHA-256 identity from exact bytes
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// emit one tab-safe oracle record
function emit(parts) {
  process.stdout.write(`${parts.join("\t")}\n`);
}

// retain opaque bytes without involving the Rust side in JSON parsing
function b64(bytes) {
  return Buffer.from(bytes).toString("base64");
}

// form one genuine database receipt for the nonzero snapshot
function receipt(index, projectionKind, predecessorFrontierSha256) {
  const value = {
    archiveCommitOrdinal: String(index),
    archiveCommittedAt: `2026-10-08T00:4${index - 1}:00.000Z`,
    contractVersion: "adjustment-revision-commit-receipt/v1",
    frontierSha256: "",
    predecessorFrontierSha256,
    projectionIdentitySha256: String(index).repeat(64),
    projectionKind,
    projectionSha256: String(index).repeat(64),
    receiptSha256: "",
    stageReceiptSha256: String(index + 5).repeat(64),
  };
  value.receiptSha256 = sha256(Buffer.from([
    value.contractVersion,
    value.archiveCommitOrdinal,
    value.archiveCommittedAt,
    value.projectionKind,
    value.projectionIdentitySha256,
    value.projectionSha256,
    value.stageReceiptSha256,
    value.predecessorFrontierSha256,
  ].join("\n")));
  value.frontierSha256 = sha256(Buffer.from([
    "adjustment-revision-frontier/v1",
    value.predecessorFrontierSha256,
    value.archiveCommitOrdinal,
    value.receiptSha256,
  ].join("\n")));
  return value;
}

// bind all ordered receipt addresses to one snapshot identity
function addressSnapshot(snapshot) {
  snapshot.snapshotSha256 = sha256(Buffer.from([
    snapshot.contractVersion,
    snapshot.cutoffAt,
    snapshot.archiveCommitOrdinal,
    snapshot.frontierSha256,
    snapshot.entries.map((entry) => entry.receipt.receiptSha256).join("\n"),
  ].join("\n")));
  return snapshot;
}

// construct one exact database envelope accepted by the deployed command
function envelope(payload) {
  return {
    databaseManifest: {},
    payload,
    transaction: {
      created_at_utc: payload.cutoffAt,
      idle_in_transaction_session_timeout: "30s",
      isolation_level: "repeatable read",
      lock_timeout: "5s",
      read_only: "on",
      statement_timeout: "5min",
    },
  };
}

// materialize the command's unsigned and self-addressed response documents
function startMaterial(snapshot) {
  const unsigned = {
    contractVersion: "adjustment-revision-cold-transfer-start/v1",
    servingSnapshot: snapshot,
    watermarkArchiveCommitOrdinal: snapshot.archiveCommitOrdinal,
    watermarkFrontierSha256: snapshot.frontierSha256,
  };
  const preimage = canonicalBytes(unsigned);
  const output = canonicalBytes({ ...unsigned, startSha256: sha256(preimage) });
  return { output, preimage };
}

// expose the decoded fixture without adding a JSON dependency to Rust tests
function emitSnapshot(name, snapshot) {
  emit([
    "snapshot", name, snapshot.archiveCommitOrdinal, snapshot.cutoffAt,
    String(snapshot.entryCount), snapshot.frontierSha256, snapshot.snapshotSha256,
  ]);
  snapshot.entries.forEach((entry, index) => {
    emit(["entry", name, String(index), entry.logicalReceivedAt, entry.relation]);
    emit([
      "receipt", name, String(index), entry.receipt.archiveCommitOrdinal,
      entry.receipt.archiveCommittedAt, entry.receipt.contractVersion,
      entry.receipt.frontierSha256, entry.receipt.predecessorFrontierSha256,
      entry.receipt.projectionIdentitySha256, entry.receipt.projectionKind,
      entry.receipt.projectionSha256, entry.receipt.receiptSha256,
      entry.receipt.stageReceiptSha256,
    ]);
  });
  const material = startMaterial(snapshot);
  const snapshotBytes = canonicalBytes(snapshot);
  emit(["encoding", name, b64(snapshotBytes), b64(material.preimage), b64(material.output)]);
  const snapshotPreimage = Buffer.from([
    snapshot.contractVersion,
    snapshot.cutoffAt,
    snapshot.archiveCommitOrdinal,
    snapshot.frontierSha256,
    snapshot.entries.map((entry) => entry.receipt.receiptSha256).join("\n"),
  ].join("\n"));
  emit(["hash", b64(snapshotPreimage), snapshot.snapshotSha256]);
  emit(["hash", b64(material.preimage), sha256(material.preimage)]);
  emit(["clock", snapshot.cutoffAt, String(Date.parse(snapshot.cutoffAt))]);
  for (const entry of snapshot.entries) {
    emit(["clock", entry.logicalReceivedAt, String(Date.parse(entry.logicalReceivedAt))]);
    emit([
      "clock", entry.receipt.archiveCommittedAt,
      String(Date.parse(entry.receipt.archiveCommittedAt)),
    ]);
  }
}

// capture one command result without writing through process stdout
async function observe(name, argv, input, root) {
  const output = [];
  let failure = null;
  try {
    await runAdjustmentArchiveCommand(argv, {
      root,
      stdinBytes: input,
      stdout: { write: (bytes) => output.push(Buffer.from(bytes)) },
    });
  } catch (error) {
    failure = error;
  }
  emit([
    "case", name, failure === null ? "ok" : "error", b64(input),
    b64(Buffer.concat(output)), failure === null ? "" : b64(String(failure.message)),
  ]);
}

const genesis = sha256(Buffer.from("adjustment-revision-frontier/v1\n0\n"));
const receipts = [];
let predecessor = genesis;
for (const [index, kind] of [
  "actual_best_match", "native_source", "rain_gate_input", "target_revision",
].entries()) {
  const value = receipt(index + 1, kind, predecessor);
  receipts.push(value);
  predecessor = value.frontierSha256;
}
const nonzero = addressSnapshot({
  archiveCommitOrdinal: "4",
  contractVersion: "adjustment-revision-serving-snapshot/v1",
  cutoffAt: "2026-10-08T00:45:00.000Z",
  entries: receipts.map((value, index) => ({
    logicalReceivedAt: `2026-10-08T00:3${index}:00.000Z`,
    receipt: value,
    relation: index === 0 ? "weather_records" : index === 1
      ? "forecast_anchor_records" : index === 2
        ? "rain_adjustment_runs" : "weather_records",
  })),
  entryCount: 4,
  frontierSha256: predecessor,
  snapshotSha256: "",
});
const empty = addressSnapshot({
  archiveCommitOrdinal: "0",
  contractVersion: "adjustment-revision-serving-snapshot/v1",
  cutoffAt: "2026-10-08T00:45:00.000Z",
  entries: [],
  entryCount: 0,
  frontierSha256: genesis,
  snapshotSha256: "",
});
const invalidRelation = structuredClone(nonzero);
invalidRelation.entries[0].relation = "forecast_anchor_records";
const invalidGenesis = addressSnapshot({ ...structuredClone(empty), frontierSha256: "f".repeat(64) });

emitSnapshot("nonzero", nonzero);
emitSnapshot("empty", empty);
emitSnapshot("invalid_relation", invalidRelation);
emitSnapshot("invalid_genesis", invalidGenesis);

const root = await mkdtemp(join(tmpdir(), "weather-revision-cold-start-"));
try {
  await chmod(root, 0o700);
  for (const directory of COLD_DIRECTORIES) {
    await mkdir(join(root, directory), { mode: 0o700 });
    await chmod(join(root, directory), 0o700);
  }
  const nonzeroInput = Buffer.from(JSON.stringify(envelope(nonzero)));
  const emptyInput = Buffer.from(JSON.stringify(envelope(empty)));
  const invalidRelationInput = Buffer.from(JSON.stringify(envelope(invalidRelation)));
  const invalidGenesisInput = Buffer.from(JSON.stringify(envelope(invalidGenesis)));
  await observe("nonzero", ["revision-cold-start-v1"], nonzeroInput, root);
  await observe("empty", ["revision-cold-start-v1"], emptyInput, root);
  await observe(
    "invalid_relation", ["revision-cold-start-v1"], invalidRelationInput, root,
  );
  await observe(
    "invalid_genesis", ["revision-cold-start-v1"], invalidGenesisInput, root,
  );
  await observe(
    "arity", ["revision-cold-start-v1", "unexpected"], nonzeroInput, root,
  );
  await rm(join(root, COLD_DIRECTORIES.at(-1)), { recursive: true });
  await observe("missing_directory", ["revision-cold-start-v1"], emptyInput, root);
} finally {
  await rm(root, { force: true, recursive: true });
}

assert.equal(receipts.length, 4);
