import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { chmod, link, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";

import {
  inspectAdjustmentMaintenanceCaptureReadiness,
  verifyInstalledAdjustmentRunnerClosure,
} from "./adjustment_capture_readiness.mjs";
import { ADJUSTMENT_FIT_PUBLIC_CODE_ALLOWLIST } from "./adjustment_fit_inputs.mjs";

const SOURCE_COMMIT = "1".repeat(40);
const HASH_A = "a".repeat(64);
const HASH_B = "b".repeat(64);
const SERVER_IMAGE = `sha256:${"c".repeat(64)}`;
const CONTROL_SHA256 = "d".repeat(64);
const EPOCH_AT = "2026-10-08T12:00:00.000Z";
const SNAPSHOT_AT = "2026-10-08T12:00:01.000Z";
const WITNESS_SHA256 = "e".repeat(64);
const repoRoot = resolve(import.meta.dirname, "../..");
const migrationRoot = join(repoRoot, "packages/database/migrations");
const migrationNames = readdirSync(migrationRoot)
  .filter((name) => name.endsWith(".sql")).sort();
const migrationChecksums = migrationNames.map((name) =>
  sha256(readFileSync(join(migrationRoot, name))));
const rollingMigrationNames = migrationNames.slice(0, 21);
const rollingMigrationChecksums = migrationChecksums.slice(0, 21);
const rollingHistorySha256 = sha256(Buffer.from(rollingMigrationNames.map(
  // serialize the retained capture ledger
  (name, index) => `${name}:${rollingMigrationChecksums[index]}`,
).join("\n")));
const catalogHistorySha256 = sha256(Buffer.from(migrationNames.map(
  // serialize the additive live catalog ledger
  (name, index) => `${name}:${migrationChecksums[index]}`,
).join("\n")));

// hash one fixture member
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// create one immutable manifest-bound release fixture
async function createReleaseFixture(options = {}) {
  const root = await mkdtemp(join(tmpdir(), "adjustment-capture-readiness-"));
  const releaseRoot = join(root, "releases", SOURCE_COMMIT);
  const members = new Map([
    ["deploy/scripts/member.mjs", Buffer.from("export const member = true;\n")],
    ["scripts/research/job.mjs", Buffer.from("export const job = true;\n")],
    ...[...new Set(Object.values(ADJUSTMENT_FIT_PUBLIC_CODE_ALLOWLIST).flat().map(
      // materialize every required fitter source as a distinct fixture byte string
      ({ source }) => source,
    ))].filter(
      // omit only one explicitly selected regression member
      (path) => path !== options.omitFitSource,
    ).map((path) => [path, Buffer.from(`fixture:${path}\n`)]),
  ]);
  const directories = new Set([releaseRoot]);
  await mkdir(releaseRoot, { mode: 0o700, recursive: true });

  // materialize each reviewed file before freezing directories
  for (const [path, bytes] of members) {
    const absolute = join(releaseRoot, path);
    await mkdir(dirname(absolute), { mode: 0o700, recursive: true });
    let directory = dirname(absolute);
    // retain every created release directory for exact mode transitions
    while (directory.startsWith(`${releaseRoot}/`)) {
      directories.add(directory);
      directory = dirname(directory);
    }
    await writeFile(absolute, bytes, { mode: 0o400 });
    await chmod(absolute, 0o400);
  }
  const manifestBytes = Buffer.from([...members]
    .map(([path, bytes]) => `${sha256(bytes)}  ${path}\n`)
    .join(""));
  await writeFile(join(releaseRoot, "MANIFEST.sha256"), manifestBytes, { mode: 0o400 });
  await chmod(join(releaseRoot, "MANIFEST.sha256"), 0o400);

  // freeze every release directory exactly like the installer
  for (const path of [...directories].sort((left, right) => right.length - left.length)) {
    await chmod(path, 0o500);
  }
  return { directories, manifestBytes, memberCount: members.size, releaseRoot, root };
}

// thaw fixture directories only for disposable cleanup
async function cleanupReleaseFixture(fixture) {
  // restore owner-write access from parent to child
  for (const path of [...fixture.directories].sort((left, right) => left.length - right.length)) {
    await chmod(path, 0o700).catch(() => undefined);
  }
  await rm(fixture.root, { force: true, recursive: true });
}

// create one exact database manifest fixture
function databaseManifest(names = migrationNames, checksums = migrationChecksums) {
  return {
    contract_version: "adjustment-evaluation-export-manifest/v1",
    migration_checksums: [...checksums],
    migration_history_sha256: sha256(Buffer.from(names.map(
      // serialize the exact selected ledger
      (name, index) => `${name}:${checksums[index]}`,
    ).join("\n"))),
    migration_names: [...names],
    query_contract_sha256:
      "c860039c72818a9b813ed9f9d93f5d8e115f4144e99d698734e9159ca3457bd4",
    query_contract_version: "adjustment-evaluation-export-query/v1",
    row_schema_sha256:
      "c21782034130d003e4af48fada4c8f6545f6f1c4dff550f92d0523c7d96d20e2",
    schema_migration: "0017_adjustment_evaluation_export.sql",
    site_key: "ballydidean",
    site_timezone: "America/Los_Angeles",
  };
}

// create one complete injected readiness authority set
function readinessPorts(overrides = {}) {
  const witness = {
    catalogFrontierSha256: HASH_A,
    controlPlaneSha256: CONTROL_SHA256,
    controlPlaneVersion: "14",
    databaseMigrationHistorySha256: rollingHistorySha256,
    epochAt: EPOCH_AT,
    servingSnapshotSha256: HASH_B,
    sourceCommit: SOURCE_COMMIT,
    sourceRelease: "2026.10.08-1",
    sourceServerImageDigest: SERVER_IMAGE,
    witnessSha256: WITNESS_SHA256,
  };
  return {
    fetchDatabaseLedger: async () => ({
      contractVersion: "adjustment-database-ledger/v3",
      databaseManifest: databaseManifest(),
      snapshotAt: SNAPSHOT_AT,
    }),
    fetchEpochSnapshot: async () => ({
      archiveCommitOrdinal: "0",
      cutoffAt: EPOCH_AT,
      entries: [],
      entryCount: 0,
      frontierSha256: HASH_A,
      snapshotSha256: HASH_B,
    }),
    fetchEpochWitness: async () => witness,
    fetchFamilyCurrent: async (family) => ({
      commit: SOURCE_COMMIT,
      family,
      release: witness.sourceRelease,
      sourceServerImageDigest: SERVER_IMAGE,
    }),
    fetchFamilyLineage: async () => ({
      contractVersion: "adjustment-family-release-lineage/v2",
      controlSha256: CONTROL_SHA256,
      controlVersion: "14",
      currentActionSha256: null,
      currentCommit: SOURCE_COMMIT,
      currentRelease: witness.sourceRelease,
      epochAncestorCommit: SOURCE_COMMIT,
      epochWitnessSha256: WITNESS_SHA256,
      state: "verified_epoch_descendant",
      verifiedAt: SNAPSHOT_AT,
    }),
    inspectLocalReadiness: async () => ({
      contractVersion: "adjustment-archive-local-readiness/v1",
      ready: true,
      reason: "ready",
      status: {
        allocatedBytes: "0",
        fileCount: 0,
        incomingCount: 0,
        objectCount: 0,
        taskInodes: "0",
      },
    }),
    inspectFitRuntime: async () => ({
      contractVersion: "adjustment-fit-runtime-readiness/v1",
    }),
    verifyInstalledClosure: async () => ({
      manifestSha256: HASH_B,
      memberCount: 2,
      sourceCommit: SOURCE_COMMIT,
    }),
    ...overrides,
  };
}

// prove one exact installed release and its manifest identity
test("installed capture release verifier accepts only the frozen complete closure", async () => {
  const fixture = await createReleaseFixture();

  try {
    const result = await verifyInstalledAdjustmentRunnerClosure({
      releaseRoot: fixture.releaseRoot,
    });
    assert.deepEqual(result, {
      manifestSha256: sha256(fixture.manifestBytes),
      memberCount: fixture.memberCount,
      sourceCommit: SOURCE_COMMIT,
    });
  } finally {
    await cleanupReleaseFixture(fixture);
  }
});

// require the manifest to contain the complete installed fitter closure
test("installed capture release verifier rejects an incomplete fitter closure", async () => {
  const omitted = ADJUSTMENT_FIT_PUBLIC_CODE_ALLOWLIST.wind.find(
    // select the generated wind entrypoint rather than an arbitrary source
    ({ destination }) => destination.endsWith("forecast-adjustment-wind-refresh-cli.js"),
  ).source;
  const fixture = await createReleaseFixture({ omitFitSource: omitted });

  try {
    await assert.rejects(
      verifyInstalledAdjustmentRunnerClosure({ releaseRoot: fixture.releaseRoot }),
      /fitter closure is incomplete/u,
    );
  } finally {
    await cleanupReleaseFixture(fixture);
  }
});

// reject mutable, unlisted, or multiply-linked installed bytes
test("installed capture release verifier rejects closure drift", async (context) => {
  // exercise each distinct immutable-closure failure
  for (const mutation of ["content", "unlisted", "hardlink"]) {
    await context.test(mutation, async () => {
      const fixture = await createReleaseFixture();

      try {
        await chmod(fixture.releaseRoot, 0o700);

        // apply only the selected release mutation
        if (mutation === "content") {
          const path = join(fixture.releaseRoot, "deploy/scripts/member.mjs");
          await chmod(path, 0o600);
          await writeFile(path, "drift\n");
          await chmod(path, 0o400);
        } else if (mutation === "unlisted") {
          await writeFile(join(fixture.releaseRoot, "foreign.mjs"), "foreign\n", { mode: 0o400 });
          await chmod(join(fixture.releaseRoot, "foreign.mjs"), 0o400);
        } else {
          await link(
            join(fixture.releaseRoot, "deploy/scripts/member.mjs"),
            join(fixture.releaseRoot, "member-link.mjs"),
          );
        }
        await chmod(fixture.releaseRoot, 0o500);
        await assert.rejects(
          verifyInstalledAdjustmentRunnerClosure({ releaseRoot: fixture.releaseRoot }),
          /installed release/u,
        );
      } finally {
        await cleanupReleaseFixture(fixture);
      }
    });
  }
});

// require all authenticated authorities before declaring capture ready
test("capture readiness crossbinds installed, local, epoch, database, and live source proofs", async () => {
  const result = await inspectAdjustmentMaintenanceCaptureReadiness(readinessPorts());
  assert.deepEqual(result, {
    archiveStatus: {
      allocatedBytes: "0",
      fileCount: 0,
      incomingCount: 0,
      objectCount: 0,
      taskInodes: "0",
    },
    contractVersion: "adjustment-maintenance-capture-readiness/v1",
    databaseMigrationHistorySha256: catalogHistorySha256,
    ready: true,
    reason: "ready",
    releaseManifestSha256: HASH_B,
    sourceCommit: SOURCE_COMMIT,
    sourceRelease: "2026.10.08-1",
  });
});

// retain the exact 0021 live pairing while admitting only its exact 0022 successor
test("capture readiness accepts only witness-21 with live-21 or live-22", async () => {
  const rolling = await inspectAdjustmentMaintenanceCaptureReadiness(readinessPorts({
    fetchDatabaseLedger: async () => ({
      contractVersion: "adjustment-database-ledger/v3",
      databaseManifest: databaseManifest(rollingMigrationNames, rollingMigrationChecksums),
      snapshotAt: SNAPSHOT_AT,
    }),
  }));
  assert.equal(rolling.ready, true);
  assert.equal(rolling.databaseMigrationHistorySha256, rollingHistorySha256);

  const catalogWitness = await inspectAdjustmentMaintenanceCaptureReadiness(readinessPorts({
    fetchEpochWitness: async () => ({
      ...(await readinessPorts().fetchEpochWitness()),
      databaseMigrationHistorySha256: catalogHistorySha256,
    }),
  }));
  assert.equal(catalogWitness.ready, false);
  assert.equal(catalogWitness.reason, "capture_epoch_mismatch");
});

// allow a genuine family-only successor while retaining the epoch runner closure
test("capture readiness accepts an authenticated epoch-descendant current release", async () => {
  const successorCommit = "2".repeat(40);
  const successorRelease = "2026.10.09-1";
  const result = await inspectAdjustmentMaintenanceCaptureReadiness(readinessPorts({
    fetchFamilyCurrent: async (family) => ({
      commit: successorCommit,
      family,
      release: successorRelease,
      sourceServerImageDigest: `sha256:${"f".repeat(64)}`,
    }),
    fetchFamilyLineage: async () => ({
      contractVersion: "adjustment-family-release-lineage/v2",
      controlSha256: CONTROL_SHA256,
      controlVersion: "14",
      currentActionSha256: "9".repeat(64),
      currentCommit: successorCommit,
      currentRelease: successorRelease,
      epochAncestorCommit: SOURCE_COMMIT,
      epochWitnessSha256: WITNESS_SHA256,
      state: "verified_epoch_descendant",
      verifiedAt: SNAPSHOT_AT,
    }),
  }));
  assert.equal(result.ready, true);
  assert.equal(result.sourceCommit, SOURCE_COMMIT);
  assert.equal(result.sourceRelease, successorRelease);
});

// fail closed on each independent readiness authority drift
test("capture readiness refuses incomplete or mismatched authorities", async (context) => {
  const cases = [{
    name: "fit runtime",
    options: readinessPorts({
      inspectFitRuntime: async () => {
        throw new Error("missing numerical runtime");
      },
    }),
    reason: "fit_runtime_unavailable",
  }, {
    name: "local archive",
    options: readinessPorts({
      inspectLocalReadiness: async () => ({
        contractVersion: "adjustment-archive-local-readiness/v1",
        ready: false,
        reason: "physical_capacity_refused",
        status: null,
      }),
    }),
    reason: "local_archive_unready",
  }, {
    name: "legacy ledger",
    options: readinessPorts({
      fetchDatabaseLedger: async () => ({
        contractVersion: "adjustment-database-ledger/v3",
        databaseManifest: { migration_history_sha256: HASH_A, migration_names: [] },
        snapshotAt: SNAPSHOT_AT,
      }),
    }),
    reason: "database_ledger_unavailable",
  }, {
    name: "unreviewed ledger suffix",
    options: readinessPorts({
      fetchDatabaseLedger: async () => {
        const manifest = databaseManifest();
        manifest.migration_names.push("0023_unreviewed.sql");
        manifest.migration_checksums.push("f".repeat(64));
        manifest.migration_history_sha256 = sha256(Buffer.from(
          manifest.migration_names.map(
            // hash the unsupported complete suffix
            (name, index) => `${name}:${manifest.migration_checksums[index]}`,
          ).join("\n"),
        ));
        return {
          contractVersion: "adjustment-database-ledger/v3",
          databaseManifest: manifest,
          snapshotAt: SNAPSHOT_AT,
        };
      },
    }),
    reason: "database_ledger_unavailable",
  }, {
    name: "zero snapshot",
    options: readinessPorts({
      fetchEpochSnapshot: async () => ({
        archiveCommitOrdinal: "1",
        cutoffAt: EPOCH_AT,
        entries: [],
        entryCount: 0,
        frontierSha256: HASH_A,
        snapshotSha256: HASH_B,
      }),
    }),
    reason: "capture_epoch_mismatch",
  }, {
    name: "lineage unavailable",
    options: readinessPorts({
      fetchFamilyLineage: async () => {
        throw new Error("lineage unavailable");
      },
    }),
    reason: "current_lineage_unavailable",
  }, {
    name: "live source",
    options: readinessPorts({
      fetchFamilyCurrent: async (family) => ({
        commit: family === "rain" ? "2".repeat(40) : SOURCE_COMMIT,
        family,
        release: "2026.10.08-1",
        sourceServerImageDigest: SERVER_IMAGE,
      }),
    }),
    reason: "current_release_mismatch",
  }, {
    name: "unrelated lineage",
    options: readinessPorts({
      fetchFamilyLineage: async () => ({
        contractVersion: "adjustment-family-release-lineage/v2",
        controlSha256: CONTROL_SHA256,
        controlVersion: "14",
        currentActionSha256: "9".repeat(64),
        currentCommit: "2".repeat(40),
        currentRelease: "2026.10.09-1",
        epochAncestorCommit: "3".repeat(40),
        epochWitnessSha256: WITNESS_SHA256,
        state: "verified_epoch_descendant",
        verifiedAt: SNAPSHOT_AT,
      }),
    }),
    reason: "current_release_mismatch",
  }];

  // isolate every refusal reason from the remaining valid proofs
  for (const fixture of cases) {
    await context.test(fixture.name, async () => {
      const result = await inspectAdjustmentMaintenanceCaptureReadiness(fixture.options);
      assert.equal(result.ready, false);
      assert.equal(result.reason, fixture.reason);
    });
  }
});
