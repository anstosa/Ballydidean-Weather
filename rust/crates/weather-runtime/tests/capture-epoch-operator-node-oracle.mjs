import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { runAdjustmentArchiveCommand } from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const GENESIS_FRONTIER_SHA256 = createHash("sha256")
  .update("adjustment-revision-frontier/v1\n0\n")
  .digest("hex");
const ROLLING_0021_MIGRATION_HISTORY_SHA256 =
  "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";
const EARLY_V14_MIGRATION_HISTORY_SHA256 =
  "c683c4f937c7f02b00f6ab49f75268eead81d2a221a8a9a23e38f9e4802a11b0";

// encode the already sorted fixture objects exactly like the original command
function canonicalBytes(value) {
  return Buffer.from(`${JSON.stringify(value)}\n`);
}

// compute one lowercase content identity from exact predecessor bytes
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// form the exact original snapshot address preimage
function servingSnapshotPreimage(snapshot) {
  return Buffer.from([
    snapshot.contractVersion,
    snapshot.cutoffAt,
    snapshot.archiveCommitOrdinal,
    snapshot.frontierSha256,
    "",
  ].join("\n"));
}

// construct one valid empty serving snapshot at an independent cutoff
function servingSnapshot(cutoffAt) {
  const unsigned = {
    archiveCommitOrdinal: "0",
    contractVersion: "adjustment-revision-serving-snapshot/v1",
    cutoffAt,
    entries: [],
    entryCount: 0,
    frontierSha256: GENESIS_FRONTIER_SHA256,
  };
  const snapshotSha256 = sha256(servingSnapshotPreimage(unsigned));
  return { ...unsigned, snapshotSha256 };
}

// construct one valid v14 witness bound to the reviewed 0021 ledger
function captureWitness(snapshot, databaseMigrationHistorySha256) {
  const unsigned = {
    activationKind: "inert_v14_pre_activation",
    archiveCommitOrdinal: "0",
    catalogFrontierSha256: GENESIS_FRONTIER_SHA256,
    contractVersion: "adjustment-revision-capture-epoch-witness/v1",
    controlPlaneSha256: "1".repeat(64),
    controlPlaneVersion: "14",
    databaseMigrationHistorySha256,
    epochAt: snapshot.cutoffAt,
    servingSnapshotSha256: snapshot.snapshotSha256,
    sourceCommit: "2".repeat(40),
    sourceRelease: "2026.10.08-1",
    sourceServerImageDigest: `sha256:${"3".repeat(64)}`,
    sourceWebImageDigest: `sha256:${"4".repeat(64)}`,
  };
  return { ...unsigned, witnessSha256: sha256(canonicalBytes(unsigned)) };
}

// write one current-user authority file with the exact predecessor mode
async function writeAuthority(path, bytes) {
  await writeFile(path, bytes, { mode: 0o644 });
  await chmod(path, 0o644);
}

// capture one command outcome without letting an error write to process stdout
async function observe(name, argv, options) {
  const output = [];
  let failure = null;
  try {
    await runAdjustmentArchiveCommand(argv, {
      ...options,
      stdout: { write: (bytes) => output.push(Buffer.from(bytes)) },
    });
  } catch (error) {
    failure = error;
  }
  return {
    error: failure === null ? "" : Buffer.from(String(failure.message)).toString("base64"),
    name,
    output: Buffer.concat(output).toString("base64"),
    status: failure === null ? "ok" : "error",
  };
}

// emit one tab-safe result with opaque exact bytes
function emit(parts) {
  process.stdout.write(`${parts.join("\t")}\n`);
}

// expose one compact command JSON line without its transport newline
function jsonLine(bytes) {
  assert.equal(bytes.at(-1), 0x0a);
  return bytes.subarray(0, -1).toString("utf8");
}

// expose original string scalars without asking Rust to parse fixture JSON
function emitStringFields(name, value) {
  // retain the predecessor object's insertion order
  for (const [key, field] of Object.entries(value)) {
    // omit arrays and numbers while retaining every exact string
    if (typeof field === "string") {
      emit(["field", name, key, field]);
    }
  }
}

const root = await mkdtemp(join(tmpdir(), "weather-capture-epoch-oracle-"));
try {
  const witnessPath = join(root, "adjustment-capture-epoch.json");
  const snapshotPath = join(root, "adjustment-capture-epoch-snapshot.json");
  const snapshotA = servingSnapshot("2026-10-08T16:00:00.000Z");
  const snapshotB = servingSnapshot("2026-10-08T16:00:01.000Z");
  const snapshotBadClock = servingSnapshot("2026-02-30T16:00:00.000Z");
  const snapshotExtended = servingSnapshot("+010000-01-01T00:00:00.000Z");
  const witness = captureWitness(snapshotA, ROLLING_0021_MIGRATION_HISTORY_SHA256);
  const earlyWitness = captureWitness(snapshotA, EARLY_V14_MIGRATION_HISTORY_SHA256);
  const witnessBadClock = captureWitness(
    snapshotBadClock,
    ROLLING_0021_MIGRATION_HISTORY_SHA256,
  );
  const witnessExtended = captureWitness(
    snapshotExtended,
    ROLLING_0021_MIGRATION_HISTORY_SHA256,
  );
  const witnessBytes = canonicalBytes(witness);
  const earlyWitnessBytes = canonicalBytes(earlyWitness);
  const witnessBadClockBytes = canonicalBytes(witnessBadClock);
  const witnessExtendedBytes = canonicalBytes(witnessExtended);
  const snapshotABytes = canonicalBytes(snapshotA);
  const snapshotBBytes = canonicalBytes(snapshotB);
  const snapshotBadClockBytes = canonicalBytes(snapshotBadClock);
  const snapshotExtendedBytes = canonicalBytes(snapshotExtended);
  const authority = {
    captureEpochExpectedGid: process.getegid(),
    captureEpochExpectedUid: process.geteuid(),
    captureEpochPath: witnessPath,
    captureEpochSnapshotPath: snapshotPath,
  };

  await writeAuthority(snapshotPath, snapshotABytes);
  const snapshotOnly = await observe(
    "snapshot_without_witness",
    ["revision-capture-epoch-snapshot-read-v1"],
    authority,
  );
  assert.deepEqual(snapshotOnly, {
    error: "",
    name: "snapshot_without_witness",
    output: snapshotABytes.toString("base64"),
    status: "ok",
  });

  await writeAuthority(witnessPath, witnessBytes);
  const paired = await observe(
    "paired_witness",
    ["revision-capture-epoch-read-v1"],
    authority,
  );
  assert.deepEqual(paired, {
    error: "",
    name: "paired_witness",
    output: witnessBytes.toString("base64"),
    status: "ok",
  });

  await writeAuthority(snapshotPath, snapshotBBytes);
  const mismatch = await observe(
    "mismatched_pair",
    ["revision-capture-epoch-read-v1"],
    authority,
  );
  assert.equal(mismatch.status, "error");
  assert.equal(mismatch.output, "");
  assert.equal(
    Buffer.from(mismatch.error, "base64").toString("utf8"),
    "revision capture epoch snapshot differs from witness",
  );

  const arity = await observe(
    "snapshot_arity",
    ["revision-capture-epoch-snapshot-read-v1", "unexpected"],
    authority,
  );
  assert.equal(arity.status, "error");
  assert.equal(arity.output, "");
  assert.equal(
    Buffer.from(arity.error, "base64").toString("utf8"),
    "revision-capture-epoch-snapshot-read-v1 takes no arguments",
  );

  await writeAuthority(snapshotPath, snapshotABytes);
  await writeAuthority(witnessPath, earlyWitnessBytes);
  const earlyV14 = await observe(
    "early_v14_witness",
    ["revision-capture-epoch-read-v1"],
    authority,
  );
  assert.equal(earlyV14.status, "error");
  assert.equal(earlyV14.output, "");
  assert.equal(
    Buffer.from(earlyV14.error, "base64").toString("utf8"),
    "adjustment revision capture epoch witness is invalid",
  );

  await writeAuthority(snapshotPath, snapshotBadClockBytes);
  const badSnapshotClock = await observe(
    "snapshot_bad_clock",
    ["revision-capture-epoch-snapshot-read-v1"],
    authority,
  );
  assert.equal(badSnapshotClock.status, "error");
  assert.equal(badSnapshotClock.output, "");

  await writeAuthority(witnessPath, witnessBadClockBytes);
  const badWitnessClock = await observe(
    "witness_bad_clock",
    ["revision-capture-epoch-read-v1"],
    authority,
  );
  assert.equal(badWitnessClock.status, "error");
  assert.equal(badWitnessClock.output, "");

  await writeAuthority(snapshotPath, snapshotExtendedBytes);
  const extendedSnapshot = await observe(
    "snapshot_extended",
    ["revision-capture-epoch-snapshot-read-v1"],
    authority,
  );
  assert.deepEqual(extendedSnapshot, {
    error: "",
    name: "snapshot_extended",
    output: snapshotExtendedBytes.toString("base64"),
    status: "ok",
  });

  await writeAuthority(witnessPath, witnessExtendedBytes);
  const extendedWitness = await observe(
    "witness_extended",
    ["revision-capture-epoch-read-v1"],
    authority,
  );
  assert.deepEqual(extendedWitness, {
    error: "",
    name: "witness_extended",
    output: witnessExtendedBytes.toString("base64"),
    status: "ok",
  });

  emit(["material", "witness", jsonLine(witnessBytes)]);
  emit(["material", "witness_early_v14", jsonLine(earlyWitnessBytes)]);
  emit(["material", "snapshot_a", jsonLine(snapshotABytes)]);
  emit(["material", "snapshot_b", jsonLine(snapshotBBytes)]);
  emit(["material", "snapshot_bad_clock", jsonLine(snapshotBadClockBytes)]);
  emit(["material", "witness_bad_clock", jsonLine(witnessBadClockBytes)]);
  emit(["material", "snapshot_extended", jsonLine(snapshotExtendedBytes)]);
  emit(["material", "witness_extended", jsonLine(witnessExtendedBytes)]);
  emitStringFields("witness", witness);
  emitStringFields("witness_early_v14", earlyWitness);
  emitStringFields("snapshot_a", snapshotA);
  emitStringFields("snapshot_b", snapshotB);
  emitStringFields("snapshot_bad_clock", snapshotBadClock);
  emitStringFields("witness_bad_clock", witnessBadClock);
  emitStringFields("snapshot_extended", snapshotExtended);
  emitStringFields("witness_extended", witnessExtended);
  const { witnessSha256, ...witnessUnsigned } = witness;
  const { witnessSha256: earlyWitnessSha256, ...earlyWitnessUnsigned } = earlyWitness;
  const { witnessSha256: badClockWitnessSha256, ...badClockWitnessUnsigned } =
    witnessBadClock;
  const { witnessSha256: extendedWitnessSha256, ...extendedWitnessUnsigned } =
    witnessExtended;
  emit(["hash", "witness", canonicalBytes(witnessUnsigned).toString("hex"), witnessSha256]);
  emit([
    "hash",
    "witness_early_v14",
    canonicalBytes(earlyWitnessUnsigned).toString("hex"),
    earlyWitnessSha256,
  ]);
  emit([
    "hash",
    "snapshot_a",
    servingSnapshotPreimage(snapshotA).toString("hex"),
    snapshotA.snapshotSha256,
  ]);
  emit([
    "hash",
    "snapshot_b",
    servingSnapshotPreimage(snapshotB).toString("hex"),
    snapshotB.snapshotSha256,
  ]);
  emit([
    "hash",
    "snapshot_bad_clock",
    servingSnapshotPreimage(snapshotBadClock).toString("hex"),
    snapshotBadClock.snapshotSha256,
  ]);
  emit([
    "hash",
    "witness_bad_clock",
    canonicalBytes(badClockWitnessUnsigned).toString("hex"),
    badClockWitnessSha256,
  ]);
  emit([
    "hash",
    "snapshot_extended",
    servingSnapshotPreimage(snapshotExtended).toString("hex"),
    snapshotExtended.snapshotSha256,
  ]);
  emit([
    "hash",
    "witness_extended",
    canonicalBytes(extendedWitnessUnsigned).toString("hex"),
    extendedWitnessSha256,
  ]);
  emit(["clock", snapshotA.cutoffAt, String(Date.parse(snapshotA.cutoffAt))]);
  emit(["clock", snapshotB.cutoffAt, String(Date.parse(snapshotB.cutoffAt))]);
  emit([
    "clock",
    snapshotExtended.cutoffAt,
    String(Date.parse(snapshotExtended.cutoffAt)),
  ]);
  // preserve the fixed case order for the Rust fixture reader
  for (const result of [
    snapshotOnly,
    paired,
    mismatch,
    arity,
    earlyV14,
    badSnapshotClock,
    badWitnessClock,
    extendedSnapshot,
    extendedWitness,
  ]) {
    const output = result.output === ""
      ? ""
      : jsonLine(Buffer.from(result.output, "base64"));
    const error = result.error === ""
      ? ""
      : Buffer.from(result.error, "base64").toString("utf8");
    emit(["result", result.name, result.status, output, error]);
  }
} finally {
  await rm(root, { force: true, recursive: true });
}
