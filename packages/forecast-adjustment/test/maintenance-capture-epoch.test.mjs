import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";

import {
  adjustmentRevisionClockIsAfterCaptureEpoch,
  canonicalJsonBytes,
  requireAdjustmentRevisionProjectionAfterCaptureEpoch,
  requireMaintenanceShadowSourceAfterCaptureEpoch,
  verifyAdjustmentRevisionCaptureEpochWitness,
} from "../dist/index.js";

// construct one self-addressed zero-frontier witness
function witnessBytes(overrides = {}) {
  const unsigned = {
    activationKind: "inert_v14_pre_activation",
    archiveCommitOrdinal: "0",
    catalogFrontierSha256: "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af",
    contractVersion: "adjustment-revision-capture-epoch-witness/v1",
    controlPlaneSha256: "1".repeat(64),
    controlPlaneVersion: "14",
    databaseMigrationHistorySha256: "c683c4f937c7f02b00f6ab49f75268eead81d2a221a8a9a23e38f9e4802a11b0",
    epochAt: "2026-10-08T16:00:00.000Z",
    servingSnapshotSha256: "2".repeat(64),
    sourceCommit: "3".repeat(40),
    sourceRelease: "2026.10.08-1",
    sourceServerImageDigest: `sha256:${"4".repeat(64)}`,
    sourceWebImageDigest: `sha256:${"5".repeat(64)}`,
    ...overrides,
  };
  const witnessSha256 = createHash("sha256").update(canonicalJsonBytes(unsigned)).digest("hex");
  return Buffer.from(canonicalJsonBytes({ ...unsigned, witnessSha256 }));
}

// accept only clocks at or after the authenticated future-only epoch
test("capture epoch witness binds zero frontier and future-only clocks", () => {
  const witness = verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes());
  const v21Witness = verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes({
    databaseMigrationHistorySha256: "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
  }));
  assert.equal(witness.archiveCommitOrdinal, "0");
  assert.equal(
    v21Witness.databaseMigrationHistorySha256,
    "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",
  );
  assert.equal(adjustmentRevisionClockIsAfterCaptureEpoch(witness, witness.epochAt), true);
  assert.equal(adjustmentRevisionClockIsAfterCaptureEpoch(
    witness,
    "2026-10-08T15:59:59.999Z",
  ), false);
});

// accept only the exact current control and migration identity
test("capture epoch witness accepts the reviewed version 17 identity", () => {
  const witness = verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes({
    controlPlaneSha256: "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
    controlPlaneVersion: "17",
    databaseMigrationHistorySha256: "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf",
  }));
  assert.equal(witness.controlPlaneVersion, "17");
  assert.equal(
    witness.databaseMigrationHistorySha256,
    "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf",
  );
});

// bind the approved index timeout to its exact database manifest
test("capture epoch witness accepts only the reviewed index 600s identity", () => {
  const identity = {
    controlPlaneSha256: "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
    controlPlaneVersion: "17",
    databaseMigrationHistorySha256: "3aec6846d009a2bb2cee02279dceb9bd2d3572dc1b3a83eed2ce9a1724311345",
  };
  assert.equal(verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes(identity))
    .databaseMigrationHistorySha256, identity.databaseMigrationHistorySha256);
  assert.throws(() => verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes({
    ...identity,
    databaseMigrationHistorySha256: "1cfd6251eeb66d443e36d661299c9e49e73465fab55e5dab2cfa4ee86dd36ad2",
  })), /identity differs/u);
  assert.throws(() => verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes({
    ...identity,
    controlPlaneSha256: "7".repeat(64),
  })), /identity differs/u);
});

// reject a rehashed nonzero or incomplete database boundary
test("capture epoch witness rejects alternate frontier and history claims", () => {
  assert.throws(
    () => verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes({ archiveCommitOrdinal: "1" })),
    /identity differs/u,
  );
  assert.throws(
    () => verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes({ databaseMigrationHistorySha256: "6".repeat(64) })),
    /identity differs/u,
  );
  assert.throws(
    () => verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes({
      controlPlaneSha256: "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
      databaseMigrationHistorySha256: "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf",
    })),
    /identity differs/u,
  );
  assert.throws(
    () => verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes({
      controlPlaneSha256: "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
      controlPlaneVersion: "17",
    })),
    /identity differs/u,
  );
  assert.throws(
    () => verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes({
      controlPlaneVersion: "17",
      databaseMigrationHistorySha256: "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf",
    })),
    /identity differs/u,
  );
  assert.throws(
    () => verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes({
      controlPlaneSha256: "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
      controlPlaneVersion: "17",
      databaseMigrationHistorySha256: "3fa9ed85218278aa3b20537d9fbe9b88ef1adee7d7b2f80fde832c45d9cb4554",
    })),
    /identity differs/u,
  );
});

// reject old checkpoint rows even when staging begins after activation
test("capture epoch gate rejects old serving and shadow source clocks", () => {
  const witness = verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes());
  const shadow = {
    family: "wind",
    rows: [{
      receivedAt: "2026-10-08T16:00:01.000Z",
      referenceAt: "2026-10-08T15:00:00.000Z",
    }],
  };
  assert.throws(
    () => requireMaintenanceShadowSourceAfterCaptureEpoch(witness, shadow),
    /predates capture epoch/u,
  );
  const projection = {
    logicalKey: { productRunAt: null, runInitializedAt: null, validAt: "2026-10-08T15:00:00.000Z" },
    logicalReceivedAt: "2026-10-08T16:00:01.000Z",
    projectionKind: "target_revision",
  };
  assert.throws(
    () => requireAdjustmentRevisionProjectionAfterCaptureEpoch(witness, projection),
    /predates capture epoch/u,
  );
  const temperatureProjection = {
    contractVersion: "adjustment-temperature-native-source-projection/v2",
    logicalKey: { runInitializedAt: "2026-10-08T16:00:00.000Z" },
    logicalReceivedAt: "2026-10-08T16:00:01.000Z",
    projectionKind: "native_source",
    rows: [{ bestMatchProductRunAt: "2026-10-08T15:00:00.000Z" }],
  };
  assert.throws(
    () => requireAdjustmentRevisionProjectionAfterCaptureEpoch(witness, temperatureProjection),
    /comparator predates/u,
  );
});

// comparator initialization remains future-only even with new source receipts
test("temperature shadow epoch gate rejects pre-epoch comparator initialization", () => {
  const witness = verifyAdjustmentRevisionCaptureEpochWitness(witnessBytes());
  const shadow = {
    family: "temperature",
    rows: [{
      bestMatchProductRunAt: "2026-10-08T15:59:59.999Z",
      receivedAt: "2026-10-08T16:00:01.000Z",
      referenceAt: "2026-10-08T16:00:00.000Z",
    }],
  };
  assert.throws(() => requireMaintenanceShadowSourceAfterCaptureEpoch(witness, shadow),
    /predates capture epoch/u);
  shadow.rows[0].bestMatchProductRunAt = witness.epochAt;
  assert.doesNotThrow(() => requireMaintenanceShadowSourceAfterCaptureEpoch(witness, shadow));
});
