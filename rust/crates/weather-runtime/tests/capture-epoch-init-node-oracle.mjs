import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  unlink,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  writeAdjustmentRevisionCaptureEpochWitness,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "../../../..");
const migrationRoot = join(repoRoot, "packages/database/migrations");
const genesis = sha256(Buffer.from("adjustment-revision-frontier/v1\n0\n"));

// hash only original fixture bytes in Node rather than recreating SHA-256 in Rust
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// retain a JSON.parse-compatible stdin spelling for the Rust command fixture
function databaseBytes(value) {
  return Buffer.from(`${JSON.stringify(value)}\n`);
}

// construct the exact empty serving snapshot accepted by the original writer
function servingSnapshot(cutoffAt) {
  const snapshot = {
    archiveCommitOrdinal: "0",
    contractVersion: "adjustment-revision-serving-snapshot/v1",
    cutoffAt,
    entries: [],
    entryCount: 0,
    frontierSha256: genesis,
    snapshotSha256: "",
  };
  const preimage = Buffer.from([
    snapshot.contractVersion,
    snapshot.cutoffAt,
    "0",
    genesis,
    "",
  ].join("\n"));
  snapshot.snapshotSha256 = sha256(preimage);
  return { preimage, snapshot };
}

// construct the exact manifest surface around one reviewed migration ledger
function databaseManifest(names, checksums, history) {
  return {
    contract_version: "adjustment-evaluation-export-manifest/v1",
    migration_checksums: checksums,
    migration_history_sha256: history,
    migration_names: names,
    query_contract_sha256: "1".repeat(64),
    query_contract_version: "adjustment-evaluation-export-query/v1",
    row_schema_sha256: "2".repeat(64),
    schema_migration: "0017_adjustment_evaluation_export.sql",
    site_key: "ballydidean",
    site_timezone: "America/Los_Angeles",
  };
}

// preserve the database-owned read-only transaction declaration
function transaction(createdAtUtc) {
  return {
    created_at_utc: createdAtUtc,
    idle_in_transaction_session_timeout: "30s",
    isolation_level: "repeatable read",
    lock_timeout: "5s",
    read_only: "on",
    statement_timeout: "5min",
  };
}

// construct one direct-call envelope without invoking a privileged command path
function envelope(manifest, payload, createdAtUtc) {
  return {
    databaseManifest: manifest,
    payload,
    transaction: transaction(createdAtUtc),
  };
}

// emit tab-safe exact bytes for the Rust fixture parser
function row(...values) {
  process.stdout.write(`${values.join("\t")}\n`);
}

// emit one complete scalar object and both canonical hash inputs
function emitMaterial(name, bytes, preimage, digest) {
  const value = JSON.parse(bytes);
  row("material", name, bytes.toString("hex"));
  // export every scalar without making Rust parse production JSON
  for (const [key, field] of Object.entries(value)) {
    // identify the only two non-string snapshot fields explicitly
    if (Array.isArray(field)) {
      row("field", name, key, "array-empty", "");
    } else if (typeof field === "number") {
      row("field", name, key, "number", String(field));
    } else {
      row("field", name, key, "string", field);
    }
  }
  row("hash", name, preimage.toString("hex"), digest);
}

// emit the exact input object ingredients needed by the fixture JSON decoder
function emitDatabase(name, value, ledgerPreimage) {
  row("database", name, databaseBytes(value).toString("hex"));
  row("database-meta", name,
    value.databaseManifest.migration_history_sha256,
    value.transaction.created_at_utc,
    value.payload === null ? "null" : "snapshot");
  // preserve every actual migration identity in database order
  for (let index = 0; index < value.databaseManifest.migration_names.length; index += 1) {
    row("migration", name,
      String(index),
      value.databaseManifest.migration_names[index],
      value.databaseManifest.migration_checksums[index]);
  }
  row("hash", `ledger_${name}`, ledgerPreimage.toString("hex"),
    value.databaseManifest.migration_history_sha256);
}

// emit one immutable release identity as observed by the Node call
function emitDeployment(name, deployment) {
  // retain the six public target fields without translating their values
  for (const [key, value] of Object.entries(deployment)) {
    row("deployment", name, key, value);
  }
}

// derive the unsigned witness bytes from the actual durable canonical document
function witnessPreimage(bytes) {
  const value = JSON.parse(bytes);
  const digest = value.witnessSha256;
  delete value.witnessSha256;
  return { digest, preimage: Buffer.from(`${JSON.stringify(value)}\n`) };
}

// record a direct writer result while retaining only bounded diagnostic text on failure
async function outcome(name, operation) {
  try {
    const value = await operation();
    row("outcome", name, "ok", Buffer.from(`${JSON.stringify(value)}\n`).toString("hex"), "");
    return value;
  } catch (error) {
    row("outcome", name, "error", "", Buffer.from(String(error.message)).toString("hex"));
    return null;
  }
}

const root = await mkdtemp(join(tmpdir(), "weather-capture-epoch-init-oracle-"));
try {
  const availableNames = (await readdir(migrationRoot))
    .filter((name) => name.endsWith(".sql"))
    .sort();
  const availableChecksums = [];
  // hash the actual checked-in migrations rather than copying fixture constants
  for (const name of availableNames) {
    availableChecksums.push(sha256(await readFile(join(migrationRoot, name))));
  }

  const rollingCount = availableNames.indexOf(
    "0021_adjustment_rolling_registration.sql",
  ) + 1;
  assert.equal(rollingCount, 21);
  const rollingNames = availableNames.slice(0, rollingCount);
  const rollingChecksums = availableChecksums.slice(0, rollingCount);
  const rollingLedgerPreimage = Buffer.from(rollingNames.map(
    (name, index) => `${name}:${rollingChecksums[index]}`,
  ).join("\n"));
  const rollingHistory = sha256(rollingLedgerPreimage);
  assert.equal(rollingHistory,
    "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424");

  const rollingEpoch = "2026-10-08T02:00:00.000Z";
  const laterEpoch = "2026-10-08T02:01:00.000Z";
  const rollingSnapshot = servingSnapshot(rollingEpoch);
  const rollingManifest = databaseManifest(
    rollingNames,
    rollingChecksums,
    rollingHistory,
  );
  const rollingFirst = envelope(rollingManifest, rollingSnapshot.snapshot, rollingEpoch);
  const rollingRetry = envelope(rollingManifest, null, laterEpoch);
  const rollingDeployment = {
    controlPlaneSha256: "3".repeat(64),
    controlPlaneVersion: "14",
    sourceCommit: "4".repeat(40),
    sourceRelease: "2026.10.08-1",
    sourceServerImageDigest: `sha256:${"5".repeat(64)}`,
    sourceWebImageDigest: `sha256:${"6".repeat(64)}`,
  };
  const rollingPath = join(root, "rolling", "adjustment-capture-epoch.json");
  await mkdir(dirname(rollingPath));
  const first = await outcome("rolling_first", () =>
    writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope: rollingFirst,
      deployment: rollingDeployment,
      path: rollingPath,
    }));
  assert.notEqual(first, null);
  const rollingWitnessBytes = await readFile(rollingPath);
  const rollingSnapshotBytes = await readFile(join(
    dirname(rollingPath),
    "adjustment-capture-epoch-snapshot.json",
  ));
  const rollingWitnessHash = witnessPreimage(rollingWitnessBytes);
  emitMaterial(
    "rolling_snapshot",
    rollingSnapshotBytes,
    rollingSnapshot.preimage,
    rollingSnapshot.snapshot.snapshotSha256,
  );
  emitMaterial(
    "rolling_witness",
    rollingWitnessBytes,
    rollingWitnessHash.preimage,
    rollingWitnessHash.digest,
  );
  emitDatabase("rolling_first", rollingFirst, rollingLedgerPreimage);
  emitDatabase("rolling_retry", rollingRetry, rollingLedgerPreimage);
  emitDeployment("rolling", rollingDeployment);

  const retry = await outcome("rolling_retry", () =>
    writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope: rollingRetry,
      deployment: rollingDeployment,
      path: rollingPath,
    }));
  assert.deepEqual(retry, first);
  assert.deepEqual(await readFile(rollingPath), rollingWitnessBytes);

  const interruptedPath = join(root, "interrupted", "adjustment-capture-epoch.json");
  await mkdir(dirname(interruptedPath));
  await writeAdjustmentRevisionCaptureEpochWitness({
    databaseEnvelope: rollingFirst,
    deployment: rollingDeployment,
    path: interruptedPath,
  });
  await unlink(interruptedPath);
  const interrupted = await outcome("rolling_interrupted_retry", () =>
    writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope: rollingRetry,
      deployment: rollingDeployment,
      path: interruptedPath,
    }));
  assert.deepEqual(interrupted, first);
  assert.deepEqual(await readFile(interruptedPath), rollingWitnessBytes);

  const mismatchedDeployment = {
    ...rollingDeployment,
    sourceCommit: "7".repeat(40),
  };
  assert.equal(await outcome("rolling_mismatch", () =>
    writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope: rollingRetry,
      deployment: mismatchedDeployment,
      path: rollingPath,
    })), null);
  emitDeployment("mismatch", mismatchedDeployment);

  const missingPath = join(root, "missing", "adjustment-capture-epoch.json");
  await mkdir(dirname(missingPath));
  assert.equal(await outcome("rolling_missing_snapshot", () =>
    writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope: rollingRetry,
      deployment: rollingDeployment,
      path: missingPath,
    })), null);

  const staleEnvelope = envelope({
    ...rollingManifest,
    migration_history_sha256: "0".repeat(64),
  }, rollingSnapshot.snapshot, rollingEpoch);
  const stalePath = join(root, "stale", "adjustment-capture-epoch.json");
  await mkdir(dirname(stalePath));
  assert.equal(await outcome("stale_envelope", () =>
    writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope: staleEnvelope,
      deployment: rollingDeployment,
      path: stalePath,
    })), null);
  emitDatabase("stale_envelope", staleEnvelope, rollingLedgerPreimage);

  const missingEnvelope = {};
  assert.equal(await outcome("missing_envelope", () =>
    writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope: missingEnvelope,
      deployment: rollingDeployment,
      path: join(root, "missing-envelope.json"),
    })), null);
  row("database", "missing_envelope", databaseBytes(missingEnvelope).toString("hex"));

  const directChecksums = [...availableChecksums];
  directChecksums[availableNames.indexOf("0020_adjustment_revision_frontier.sql")] =
    "58d50e88fa767212865e222ec02e55283ceac5b00538aa2898b5ebc2873b28f6";
  const directVariants = [
    [
      "direct_a",
      "e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a",
      "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf",
    ],
    [
      "direct_b",
      "7e48acb14710a625cdcbb796bf1bf49ab4e00bf1aeb9bb763ac86410e08663e0",
      "3aec6846d009a2bb2cee02279dceb9bd2d3572dc1b3a83eed2ce9a1724311345",
    ],
  ];
  const directEpoch = "2026-11-03T08:00:00.000Z";
  const directDeployment = {
    controlPlaneSha256: "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
    controlPlaneVersion: "17",
    sourceCommit: "3".repeat(40),
    sourceRelease: "2026.11.03-1",
    sourceServerImageDigest: `sha256:${"4".repeat(64)}`,
    sourceWebImageDigest: `sha256:${"5".repeat(64)}`,
  };
  emitDeployment("direct", directDeployment);
  // exercise both reviewed direct-index checksum variants against real migration names
  for (const [name, indexChecksum, expectedHistory] of directVariants) {
    const checksums = [...directChecksums];
    checksums[availableNames.indexOf("0023_forecast_product_read_index.sql")] = indexChecksum;
    const ledgerPreimage = Buffer.from(availableNames.map(
      (migrationName, index) => `${migrationName}:${checksums[index]}`,
    ).join("\n"));
    assert.equal(sha256(ledgerPreimage), expectedHistory);
    const snapshot = servingSnapshot(directEpoch);
    const database = envelope(
      databaseManifest(availableNames, checksums, expectedHistory),
      snapshot.snapshot,
      directEpoch,
    );
    const path = join(root, name, "adjustment-capture-epoch.json");
    await mkdir(dirname(path));
    const written = await outcome(name, () =>
      writeAdjustmentRevisionCaptureEpochWitness({
        databaseEnvelope: database,
        deployment: directDeployment,
        path,
      }));
    assert.notEqual(written, null);
    const witnessBytes = await readFile(path);
    const snapshotBytes = await readFile(join(
      dirname(path),
      "adjustment-capture-epoch-snapshot.json",
    ));
    const witnessHash = witnessPreimage(witnessBytes);
    emitMaterial(`${name}_snapshot`, snapshotBytes, snapshot.preimage,
      snapshot.snapshot.snapshotSha256);
    emitMaterial(`${name}_witness`, witnessBytes, witnessHash.preimage, witnessHash.digest);
    emitDatabase(name, database, ledgerPreimage);
  }
} finally {
  await rm(root, { force: true, recursive: true });
}
