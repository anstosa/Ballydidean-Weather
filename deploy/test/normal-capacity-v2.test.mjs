import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, link, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

import {
  BLUEBERRY_NEXT_CAPTURE_BYTES,
  BLUEBERRY_PROTECTED_FREE_BYTES,
  collectNormalActivationCapacity,
  collectNormalStageCapacity,
  collectNormalStageImageCapacity,
  collectNormalStageImageProof,
  evaluateNormalActivationCapacity,
  evaluateNormalStageCapacity,
  validateNormalCapacityReceipt,
  validateNormalStageImageProof,
  verifyNormalCapacityReceipt,
} from "../scripts/adjustment-evaluation-package.mjs";

const repoRoot = resolve(import.meta.dirname, "../..");
const sourceLedgerSha256 =
  "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";
const targetLedgerSha256 =
  "c8b43b714eb555027ea7439a813831ab7326ad951b800362f35b2343330da81f";

// hash one exact fixture payload
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// encode one recursively canonical fixture
function canonical(value) {
  // preserve array order while sorting object keys
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value).sort().map(
      // encode each named member canonically
      (key) => `${JSON.stringify(key)}:${canonical(value[key])}`,
    ).join(",")}}`;
  }
  return JSON.stringify(value);
}

// create one fully verified remote-image fixture
function remoteImage(runtime, marker) {
  const diffId = `sha256:${marker.repeat(64)}`;
  const configBytes = canonical({
    architecture: "arm64",
    os: "linux",
    rootfs: { diff_ids: [diffId], type: "layers" },
  });
  const configDigest = `sha256:${sha256(configBytes)}`;
  const blobDigest = `sha256:${String(Number(marker) + 4).repeat(64)}`;
  const manifestBytes = canonical({
    config: {
      digest: configDigest,
      mediaType: "application/vnd.oci.image.config.v1+json",
      size: Buffer.byteLength(configBytes),
    },
    layers: [{
      digest: blobDigest,
      mediaType: "application/vnd.oci.image.layer.v1.tar+gzip",
      size: 4_096,
    }],
    mediaType: "application/vnd.oci.image.manifest.v1+json",
    schemaVersion: 2,
  });
  const reference = `ghcr.io/anstosa/weather-${runtime}@sha256:${sha256(manifestBytes)}`;
  return {
    configBytes,
    manifestBytes,
    reference,
    unpackedLayers: [{ allocatedBytes: 8_192, blobDigest, entryInodes: 3 }],
  };
}

// create one exact proof request and its remote fixtures
function proofFixture() {
  const roles = [
    ["source", "server", "1"],
    ["source", "web", "2"],
    ["target", "server", "3"],
    ["target", "web", "4"],
  ];
  const remote = roles.map(([, runtime, marker]) => remoteImage(runtime, marker));
  return {
    remote: new Map(remote.map((image) => [image.reference, image])),
    request: {
      imageReferences: roles.map(([role, runtime], index) => ({
        reference: remote[index].reference,
        role,
        runtime,
      })),
      operationId: "normal-release-v2-test",
      sourceEnvSha256: "5".repeat(64),
      sourceRelease: "2026.10.09-3",
      targetEnvSha256: "7".repeat(64),
      targetRelease: "2026.10.29-1",
    },
  };
}

// create the common immutable release identity
function releaseIdentity() {
  return {
    operationId: "normal-release-v2-test",
    postgresComposeProject: "weather",
    postgresContainerId: "c".repeat(64),
    sourceDatabaseOid: 16_384,
    sourceEnvSha256: "5".repeat(64),
    sourceLedgerSha256,
    sourceRelease: "2026.10.09-3",
    sourceSchemaVersion: 21,
    systemIdentifier: "7558888888888888888",
    targetEnvSha256: "7".repeat(64),
    targetLedgerSha256,
    targetRelease: "2026.10.29-1",
    targetSchemaVersion: 23,
    weatherPostgresDir: "/var/lib/weather/postgres",
  };
}

// create one complete stage evaluator input
function stageInput(proof, overrides = {}) {
  return {
    ...releaseIdentity(),
    activeDatabaseBytes: 1_000_000,
    captureReserveBytes: BLUEBERRY_NEXT_CAPTURE_BYTES,
    capturedAt: "2026-10-29T00:10:00.000Z",
    fixtureInodes: 4_096,
    freeBytes: 4_000_000_000,
    freeInodes: 100_000,
    imageProofSha256: proof.proofSha256,
    imagePullPeakBytes: 159_862_784,
    imageReferences: proof.imageReferences,
    inodeFloor: 32_768,
    largestSourceIndexBytes: 500_000,
    maxWalBytes: 10_000_000,
    newImageInodes: 4_000,
    persistentImageGrowthBytes: 90_000_000,
    phase: "pre_pull",
    postgresContainerImageId: `sha256:${"6".repeat(64)}`,
    postgresContainerImageReference: `postgres@sha256:${"8".repeat(64)}`,
    postgresDataDirectory: "/var/lib/postgresql/data",
    postgresDataFilesystemDevice: 2_048,
    protectedFloorBytes: BLUEBERRY_PROTECTED_FREE_BYTES,
    retirementCreditBytes: 0,
    scratchBytes: 268_435_456,
    sourceDatabaseFileCount: 10_000,
    weatherPostgresDirFilesystemDevice: 2_048,
    ...overrides,
  };
}

// remove evaluator-owned live stage measurements
function stageCollectionInput(proof, overrides = {}) {
  const {
    freeBytes: _freeBytes,
    freeInodes: _freeInodes,
    imagePullPeakBytes: _imagePullPeakBytes,
    newImageInodes: _newImageInodes,
    persistentImageGrowthBytes: _persistentImageGrowthBytes,
    postgresContainerImageId: _postgresContainerImageId,
    postgresContainerImageReference: _postgresContainerImageReference,
    postgresDataDirectory: _postgresDataDirectory,
    postgresDataFilesystemDevice: _postgresDataFilesystemDevice,
    weatherPostgresDirFilesystemDevice: _weatherPostgresDirFilesystemDevice,
    ...input
  } = stageInput(proof, overrides);
  return input;
}

// create one injected filesystem observation
function filesystemFacts() {
  return {
    freeBytes: 4_000_000_000,
    freeInodes: 100_000,
    postgresContainerImageId: `sha256:${"6".repeat(64)}`,
    postgresContainerImageReference: `postgres@sha256:${"8".repeat(64)}`,
    postgresDataDirectory: "/var/lib/postgresql/data",
    postgresDataFilesystemDevice: 2_048,
    weatherPostgresDirFilesystemDevice: 2_048,
  };
}

// create one complete activation evaluator input
function activationInput(overrides = {}) {
  const liveDatabaseBytes = 1_000_000;
  return {
    ...releaseIdentity(),
    actualBackupBytes: 0,
    backupHeadroomBytes: 1_073_741_824,
    boundedBackupCeilingBytes: liveDatabaseBytes + 1_073_741_824,
    captureReserveBytes: BLUEBERRY_NEXT_CAPTURE_BYTES,
    capturedAt: "2026-10-29T00:00:00.000Z",
    fixtureInodes: 4_096,
    freeBytes: 4_000_000_000,
    freeInodes: 100_000,
    inodeFloor: 32_768,
    liveDatabaseBytes,
    measuredMigrationPeakBytes: 50_000_000,
    measuredMigrationPeakInodes: 2_000,
    phase: "pre_backup",
    postgresContainerImageId: `sha256:${"6".repeat(64)}`,
    postgresContainerImageReference: `postgres@sha256:${"8".repeat(64)}`,
    postgresDataDirectory: "/var/lib/postgresql/data",
    postgresDataFilesystemDevice: 2_048,
    protectedFloorBytes: BLUEBERRY_PROTECTED_FREE_BYTES,
    retirementCreditBytes: 0,
    stageReceiptSha256: "9".repeat(64),
    weatherPostgresDirFilesystemDevice: 2_048,
    ...overrides,
  };
}

// remove evaluator-owned activation filesystem measurements
function activationCollectionInput(overrides = {}) {
  const {
    freeBytes: _freeBytes,
    freeInodes: _freeInodes,
    postgresContainerImageId: _postgresContainerImageId,
    postgresContainerImageReference: _postgresContainerImageReference,
    postgresDataDirectory: _postgresDataDirectory,
    postgresDataFilesystemDevice: _postgresDataFilesystemDevice,
    weatherPostgresDirFilesystemDevice: _weatherPostgresDirFilesystemDevice,
    ...input
  } = activationInput(overrides);
  return input;
}

test("immutable image proof can precede a fresh live admission by more than five minutes", async () => {
  const { remote, request } = proofFixture();
  let now = new Date("2026-10-29T00:00:00.000Z");
  const proof = await collectNormalStageImageProof(request, {
    clock: () => now,
    collectRemote: async (reference) => {
      now = new Date(now.getTime() + 2 * 60 * 1_000);
      return remote.get(reference);
    },
  });
  assert.equal(proof.verifiedAt, "2026-10-29T00:08:00.000Z");
  assert.equal(validateNormalStageImageProof(proof, request), proof);
  const receipt = await collectNormalStageCapacity(stageCollectionInput(proof), {
    clock: () => new Date("2026-10-29T00:11:00.000Z"),
    collectFilesystem: async () => filesystemFacts(),
    collectImages: async () => ({
      imagePullPeakBytes: 159_862_784,
      newImageInodes: 4_000,
      persistentImageGrowthBytes: 90_000_000,
    }),
    imageProof: proof,
  });
  assert.equal(receipt.state, "capacity_ready");
  assert.equal(receipt.evaluatedAt, "2026-10-29T00:11:00.000Z");
  await assert.rejects(collectNormalStageCapacity(stageCollectionInput(proof), {
    clock: () => new Date("2026-10-29T00:15:00.001Z"),
    collectFilesystem: async () => filesystemFacts(),
    collectImages: async () => ({
      imagePullPeakBytes: 159_862_784,
      newImageInodes: 4_000,
      persistentImageGrowthBytes: 90_000_000,
    }),
    imageProof: proof,
  }), /stale/u);
});

test("image proof rejects hash, reference, and transaction drift before live collection", async () => {
  const { remote, request } = proofFixture();
  const proof = await collectNormalStageImageProof(request, {
    clock: () => new Date("2026-10-29T00:08:00.000Z"),
    collectRemote: async (reference) => remote.get(reference),
  });
  assert.throws(() => validateNormalStageImageProof({
    ...proof,
    proofSha256: "f".repeat(64),
  }, request), /proof.*differs/u);
  const changedReference = structuredClone(proof);
  changedReference.imageReferences[0].reference = proof.imageReferences[1].reference;
  assert.throws(() => validateNormalStageImageProof(changedReference, request), /reference|differs/u);
  let collected = false;
  await assert.rejects(collectNormalStageCapacity(stageCollectionInput(proof, {
    targetEnvSha256: "a".repeat(64),
  }), {
    clock: () => new Date("2026-10-29T00:11:00.000Z"),
    collectFilesystem: async () => {
      collected = true;
      return filesystemFacts();
    },
    imageProof: proof,
  }), /proof.*identity/u);
  assert.equal(collected, false);
});

test("local image admission requires target images after pull", async () => {
  const { remote, request } = proofFixture();
  const proof = await collectNormalStageImageProof(request, {
    clock: () => new Date("2026-10-29T00:08:00.000Z"),
    collectRemote: async (reference) => remote.get(reference),
  });
  const inspectLocal = (_reference, image) => image.role === "source"
    ? [{ allocatedBytes: 8_192, chainId: image.layers[0].chainId }]
    : [];
  await collectNormalStageImageCapacity(stageCollectionInput(proof), proof, { inspectLocal });
  await assert.rejects(collectNormalStageImageCapacity(stageCollectionInput(proof, {
    phase: "post_pull",
  }), proof, { inspectLocal }), /target image is not locally measurable/u);
  await assert.rejects(collectNormalStageImageCapacity(stageCollectionInput(proof, {
    phase: "post_cleanup",
  }), proof, { inspectLocal }), /target image is not locally measurable/u);
});

test("post-cleanup admits only the protected byte and inode floors", async () => {
  const { remote, request } = proofFixture();
  const proof = await collectNormalStageImageProof(request, {
    clock: () => new Date("2026-10-29T00:08:00.000Z"),
    collectRemote: async (reference) => remote.get(reference),
  });
  const ready = evaluateNormalStageCapacity(stageInput(proof, {
    freeBytes: BLUEBERRY_PROTECTED_FREE_BYTES + BLUEBERRY_NEXT_CAPTURE_BYTES,
    freeInodes: 32_768,
    phase: "post_cleanup",
  }), new Date("2026-10-29T00:10:00.000Z"));
  assert.equal(ready.requiredBytes, BLUEBERRY_PROTECTED_FREE_BYTES + BLUEBERRY_NEXT_CAPTURE_BYTES);
  assert.equal(ready.requiredInodes, 32_768);
  assert.equal(ready.state, "capacity_ready");
  assert.deepEqual(evaluateNormalStageCapacity(stageInput(proof, {
    freeBytes: ready.requiredBytes - 1,
    freeInodes: ready.requiredInodes - 1,
    phase: "post_cleanup",
  }), new Date("2026-10-29T00:10:00.000Z")).reasons,
  ["insufficient_free_bytes", "insufficient_free_inodes"]);
  assert.throws(() => evaluateNormalStageCapacity(stageInput(proof, {
    phase: "retired",
  }), new Date("2026-10-29T00:10:00.000Z")), /constants/u);
});

test("historical receipts replay after freshness expires without becoming live authority", async () => {
  const { remote, request } = proofFixture();
  const proof = await collectNormalStageImageProof(request, {
    clock: () => new Date("2026-10-29T00:08:00.000Z"),
    collectRemote: async (reference) => remote.get(reference),
  });
  const receipt = evaluateNormalStageCapacity(stageInput(proof),
    new Date("2026-10-29T00:10:00.000Z"));
  assert.deepEqual(verifyNormalCapacityReceipt(receipt), receipt);
  assert.throws(() => validateNormalCapacityReceipt(receipt,
    new Date("2026-10-29T00:15:00.001Z")), /stale/u);
  assert.throws(() => verifyNormalCapacityReceipt({
    ...receipt,
    schemaVersion: "normal-capacity-v1",
  }), /schema/u);
});

test("post-backup retains the pre-backup ceiling and predecessor identity", async () => {
  const pre = evaluateNormalActivationCapacity(activationInput(),
    new Date("2026-10-29T00:01:00.000Z"));
  const postFields = {
    actualBackupAllocatedBytes: 500_006_912,
    actualBackupBytes: 500_000_000,
    boundedBackupCeilingBytes: pre.boundedBackupCeilingBytes,
    capturedAt: "2026-10-29T00:10:00.000Z",
    liveDatabaseBytes: 1_100_000,
    phase: "post_backup",
    preBackupReceiptSha256: pre.receiptSha256,
  };
  const post = await collectNormalActivationCapacity(
    activationCollectionInput(postFields), {
      clock: () => new Date("2026-10-29T00:11:00.000Z"),
      collectFilesystem: async () => filesystemFacts(),
      preBackupReceipt: pre,
    },
  );
  assert.equal(post.boundedBackupCeilingBytes, pre.boundedBackupCeilingBytes);
  assert.equal(post.requiredBytes, BLUEBERRY_PROTECTED_FREE_BYTES +
    BLUEBERRY_NEXT_CAPTURE_BYTES + pre.boundedBackupCeilingBytes -
    postFields.actualBackupAllocatedBytes + 50_000_000);
  assert.equal(post.state, "capacity_ready");
  let collected = false;
  await assert.rejects(collectNormalActivationCapacity(
    activationCollectionInput({
      ...postFields,
      boundedBackupCeilingBytes: 1_100_000 + 1_073_741_824,
    }), {
      clock: () => new Date("2026-10-29T00:11:00.000Z"),
      collectFilesystem: async () => {
        collected = true;
        return filesystemFacts();
      },
      preBackupReceipt: pre,
    },
  ), /pre-backup.*differs/u);
  assert.equal(collected, false);
  await assert.rejects(collectNormalActivationCapacity(
    activationCollectionInput({
      ...postFields,
      preBackupReceiptSha256: "f".repeat(64),
    }), {
      clock: () => new Date("2026-10-29T00:11:00.000Z"),
      collectFilesystem: async () => filesystemFacts(),
      preBackupReceipt: pre,
    },
  ), /pre-backup.*differs/u);
  collected = false;
  await assert.rejects(collectNormalActivationCapacity(
    activationCollectionInput({ phase: "unknown" }), {
      clock: () => new Date("2026-10-29T00:01:00.000Z"),
      collectFilesystem: async () => {
        collected = true;
        return filesystemFacts();
      },
    },
  ), /phase is invalid/u);
  assert.equal(collected, false);
});

test("capacity CLI proof files must be canonical private single-link files", async () => {
  const { remote, request } = proofFixture();
  const proof = await collectNormalStageImageProof(request, {
    clock: () => new Date("2026-10-29T00:08:00.000Z"),
    collectRemote: async (reference) => remote.get(reference),
  });
  const root = await mkdtemp(join(tmpdir(), "normal-capacity-v2-"));
  const inputPath = join(root, "input.json");
  const proofPath = join(root, "proof.json");
  const linkedProofPath = join(root, "proof-link.json");
  const hardLinkedProofPath = join(root, "proof-hard-link.json");
  const script = join(repoRoot, "deploy/scripts/adjustment-evaluation-package.mjs");

  try {
    await writeFile(inputPath, `${canonical(stageCollectionInput(proof))}\n`, { mode: 0o600 });
    await writeFile(proofPath, JSON.stringify(proof, null, 2), { mode: 0o600 });
    const noncanonical = spawnSync(process.execPath,
      [script, "normal-stage-capacity", inputPath, proofPath], { encoding: "utf8" });
    assert.equal(noncanonical.status, 1);
    assert.match(noncanonical.stderr, /not canonical/u);
    await writeFile(proofPath, `${canonical(proof)}\n`, { mode: 0o600 });
    await chmod(proofPath, 0o640);
    const publicFile = spawnSync(process.execPath,
      [script, "normal-stage-capacity", inputPath, proofPath], { encoding: "utf8" });
    assert.equal(publicFile.status, 1);
    assert.match(publicFile.stderr, /unsafe/u);
    await chmod(proofPath, 0o600);
    await symlink(proofPath, linkedProofPath);
    const linked = spawnSync(process.execPath,
      [script, "normal-stage-capacity", inputPath, linkedProofPath], { encoding: "utf8" });
    assert.equal(linked.status, 1);
    await link(proofPath, hardLinkedProofPath);
    const hardLinked = spawnSync(process.execPath,
      [script, "normal-stage-capacity", inputPath, hardLinkedProofPath], { encoding: "utf8" });
    assert.equal(hardLinked.status, 1);
    assert.match(hardLinked.stderr, /unsafe/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});
