import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";
import test from "node:test";
import { GIT_ADAPTER_VERSION } from "./predecessor-rust-transition-git.mjs";
import {
  OFFLINE_BOOTSTRAP_ARTIFACT_VERSION,
  OFFLINE_BOOTSTRAP_DECLARED_ARTIFACT_VERSION,
  OFFLINE_BOOTSTRAP_DECLARED_REQUEST_VERSION,
  OFFLINE_BOOTSTRAP_LIVE_GATE,
  OFFLINE_BOOTSTRAP_REQUEST_VERSION,
  decodeOfflineBootstrapRequest,
  prepareOfflineBootstrap,
  prepareOfflineBootstrapFixture,
} from "./predecessor-rust-transition-prepare.mjs";
import {
  buildReviewedTransitionDeclaration,
  buildTwoFileBundleManifest,
  sha256,
  treeSha256,
} from "./predecessor-rust-transition.mjs";

const TRANSITION_FIXTURE = new URL("./predecessor-rust-transition.fixture.json", import.meta.url);
const INSTALLER = new URL("./install-schema-preserving-release.sh", import.meta.url);
const OPERATOR_MODULE = new URL("./schema-preserving-release.mjs", import.meta.url);
const OPERATOR_SHELL = new URL("./schema-preserving-release.sh", import.meta.url);
const PREPARE_CLI = new URL("./predecessor-rust-transition-prepare.mjs", import.meta.url);
const TRANSITION_CLI = new URL("./predecessor-rust-transition.mjs", import.meta.url);

// build exact offline byte and Git evidence
function offlineFixture() {
  const transition = JSON.parse(readFileSync(TRANSITION_FIXTURE, "utf8"));
  const bundleFiles = [
    {
      bytes: Buffer.from("fixture validator bytes\n"),
      path: "scripts/schema-preserving-release.mjs",
    },
    {
      bytes: Buffer.from("fixture operator bytes\n"),
      path: "scripts/schema-preserving-release.sh",
    },
  ];
  const operatorBundle = buildTwoFileBundleManifest(bundleFiles.map((file) => ({
    bytes: file.bytes,
    path: basename(file.path),
  })));
  const controlBytes = Buffer.from("fixture Node predecessor control bytes\n");
  const installationReceiptBytes = Buffer.from("fixture private installation receipt\n");
  const recoveryDropInBytes = Buffer.from("fixture recovery drop-in\n");
  const sourceEnvironmentBytes = Buffer.from("FIXTURE_NODE_PREDECESSOR_ENVIRONMENT=exact\n");
  const input = {
    bundleFiles,
    controlBytes,
    installationReceiptBytes,
    predecessor: {
      ...transition.predecessor,
      bundleSha256: operatorBundle.manifestSha256,
      controlSha256: sha256(controlBytes),
      installationReceiptSha256: sha256(installationReceiptBytes),
      recoveryDropInSha256: sha256(recoveryDropInBytes),
      sourceEnvironmentSha256: sha256(sourceEnvironmentBytes),
    },
    recoveryDropInBytes,
    requestVersion: OFFLINE_BOOTSTRAP_REQUEST_VERSION,
    sourceEnvironmentBytes,
    target: transition.target,
  };
  const evidence = {
    adapterVersion: GIT_ADAPTER_VERSION,
    addedFiles: transition.addedFiles,
    changedMembers: transition.changedMembers,
    evidenceMode: "injected-test-fixture",
    predecessorCommit: transition.predecessor.commit,
    predecessorMembers: transition.predecessorMembers,
    targetCommit: transition.target.commit,
    targetMembers: transition.targetMembers,
    targetOnlyRoots: transition.targetOnlyRoots,
  };
  return { evidence, input, operatorBundle };
}

// run one isolated command and retain its ordinary diagnostic on failure
function run(command, args, cwd = undefined) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

// write one disposable repository member with all parent directories
function writeRepositoryMember(repository, path) {
  const destination = join(repository, path);
  mkdirSync(dirname(destination), { recursive: true });
  writeFileSync(destination, `${path}\n`);
}

// create the smallest real Git-object transition admitted by the fixed adapter
function createGitTransitionRepository(root) {
  const repository = join(root, "repository");
  mkdirSync(repository, { mode: 0o700 });
  run("git", ["init", "--quiet", "--object-format=sha1"], repository);
  run("git", ["config", "user.name", "fixture"], repository);
  run("git", ["config", "user.email", "fixture@example.invalid"], repository);
  const retained = [
    "Dockerfile",
    "deploy/scripts/common.sh",
    "deploy/scripts/update.sh",
    "package-lock.json",
    "packages/database/migrations/0020_adjustment_revision_frontier.sql",
    "packages/database/migrations/0023_forecast_product_read_index.sql",
    "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
  ];
  // populate the complete predecessor sentinel surface
  for (const path of retained) {
    writeRepositoryMember(repository, path);
  }
  run("git", ["add", "--all"], repository);
  run("git", [
    "-c", "commit.gpgsign=false",
    "-c", "core.hooksPath=/dev/null",
    "commit", "--quiet", "-m", "predecessor",
  ], repository);
  const predecessorCommit = run("git", ["rev-parse", "HEAD"], repository);
  const additions = [
    "config/backend-runtime-inventory.json",
    "rust/Cargo.lock",
    "rust/Cargo.toml",
    "rust/rust-toolchain.toml",
    "scripts/check-backend-runtime-inventory.mjs",
    "scripts/generate-rust-contracts.mjs",
    "packages/contracts/src/api.ts",
    "rust/apps/weather-migrate/src/lib.rs",
    "rust/crates/weather-adjustment/src/lib.rs",
    "rust/crates/weather-contracts/src/lib.rs",
    "rust/crates/weather-database/src/lib.rs",
    "rust/crates/weather-domain/src/lib.rs",
  ];
  // populate every fixed target-only file and root
  for (const path of additions) {
    writeRepositoryMember(repository, path);
  }
  run("git", ["add", "--all"], repository);
  run("git", [
    "-c", "commit.gpgsign=false",
    "-c", "core.hooksPath=/dev/null",
    "commit", "--quiet", "-m", "target",
  ], repository);
  const targetCommit = run("git", ["rev-parse", "HEAD"], repository);
  return { predecessorCommit, repository, targetCommit };
}

// produce only an offline ineligible artifact from exact evidence
test("offline preparer binds bytes commit and source set before strict validation", () => {
  const fixture = offlineFixture();
  const artifact = prepareOfflineBootstrapFixture(fixture.input, fixture.evidence);
  assert.equal(artifact.artifactVersion, OFFLINE_BOOTSTRAP_ARTIFACT_VERSION);
  assert.equal(artifact.eligibility, "offline-only-ineligible");
  assert.equal(artifact.eligibleForLiveInstallation, false);
  assert.equal(artifact.liveGate, OFFLINE_BOOTSTRAP_LIVE_GATE);
  assert.equal(artifact.liveAuthorityVerified, false);
  assert.equal(artifact.gitEvidenceMode, "injected-test-fixture");
  assert.deepEqual(artifact.operatorBundle, fixture.operatorBundle);
  assert.equal(artifact.controlSha256, sha256(fixture.input.controlBytes));
  assert.equal(
    artifact.sourceEnvironmentSha256,
    sha256(fixture.input.sourceEnvironmentBytes),
  );
  assert.equal(
    artifact.predecessorSourceTreeSha256,
    treeSha256(fixture.evidence.predecessorMembers),
  );
  assert.equal(artifact.transition.predecessor.commit, fixture.evidence.predecessorCommit);
  assert.deepEqual(artifact.transition.predecessorMembers, fixture.evidence.predecessorMembers);
});

// retain explicit review material while remaining categorically offline-ineligible
test("declared offline preparer binds content-addressed extension without live authority", () => {
  const fixture = offlineFixture();
  const added = {
    path: ".github/workflows/native-images.yml",
    sha256: "a".repeat(64),
  };
  const declaration = buildReviewedTransitionDeclaration({
    addedFiles: [added],
    changedMembers: [],
    predecessorCommit: fixture.input.predecessor.commit,
    targetCommit: fixture.input.target.commit,
  });
  fixture.input.requestVersion = OFFLINE_BOOTSTRAP_DECLARED_REQUEST_VERSION;
  fixture.input.reviewedDeclaration = declaration;
  fixture.evidence.reviewedDeclaration = declaration;
  fixture.evidence.addedFiles.push(added);
  fixture.evidence.addedFiles.sort((left, right) => left.path < right.path ? -1 : 1);
  fixture.evidence.targetMembers.push(added);
  fixture.evidence.targetMembers.sort((left, right) => left.path < right.path ? -1 : 1);
  const artifact = prepareOfflineBootstrapFixture(fixture.input, fixture.evidence);
  assert.equal(artifact.artifactVersion, OFFLINE_BOOTSTRAP_DECLARED_ARTIFACT_VERSION);
  assert.equal(artifact.eligibility, "offline-only-ineligible");
  assert.equal(artifact.eligibleForLiveInstallation, false);
  assert.equal(artifact.liveAuthorityVerified, false);
  assert.deepEqual(artifact.transition.reviewedDeclaration, declaration);

  const mismatched = offlineFixture();
  mismatched.input.requestVersion = OFFLINE_BOOTSTRAP_DECLARED_REQUEST_VERSION;
  mismatched.input.reviewedDeclaration = declaration;
  assert.throws(
    () => prepareOfflineBootstrapFixture(mismatched.input, mismatched.evidence),
    /reviewed declaration/u,
  );
});

// reject every mismatched predecessor byte identity
test("offline preparer rejects every predecessor raw byte mismatch", () => {
  const bundleMismatch = offlineFixture();
  bundleMismatch.input.bundleFiles[0].bytes = Buffer.from("mismatched validator bytes\n");
  assert.throws(
    () => prepareOfflineBootstrapFixture(bundleMismatch.input, bundleMismatch.evidence),
    /bundleSha256 does not match the supplied bundle bytes/u,
  );

  const controlMismatch = offlineFixture();
  controlMismatch.input.controlBytes = Buffer.from("mismatched control bytes\n");
  assert.throws(
    () => prepareOfflineBootstrapFixture(controlMismatch.input, controlMismatch.evidence),
    /controlSha256 does not match the supplied control bytes/u,
  );

  const receiptMismatch = offlineFixture();
  receiptMismatch.input.installationReceiptBytes = Buffer.from("mismatched receipt bytes\n");
  assert.throws(
    () => prepareOfflineBootstrapFixture(receiptMismatch.input, receiptMismatch.evidence),
    /installationReceiptSha256 does not match the supplied receipt bytes/u,
  );

  const dropInMismatch = offlineFixture();
  dropInMismatch.input.recoveryDropInBytes = Buffer.from("mismatched drop-in bytes\n");
  assert.throws(
    () => prepareOfflineBootstrapFixture(dropInMismatch.input, dropInMismatch.evidence),
    /recoveryDropInSha256 does not match the supplied drop-in bytes/u,
  );

  const environmentMismatch = offlineFixture();
  environmentMismatch.input.sourceEnvironmentBytes = Buffer.from("MISMATCHED_ENVIRONMENT=true\n");
  assert.throws(
    () => prepareOfflineBootstrapFixture(environmentMismatch.input, environmentMismatch.evidence),
    /sourceEnvironmentSha256 does not match the supplied environment bytes/u,
  );
});

// reject Git evidence for a different predecessor identity
test("offline preparer binds exact commits and strict predecessor members", () => {
  const commitMismatch = offlineFixture();
  commitMismatch.evidence.predecessorCommit = "c".repeat(40);
  assert.throws(
    () => prepareOfflineBootstrapFixture(commitMismatch.input, commitMismatch.evidence),
    /does not match the pinned commits/u,
  );

  const memberMismatch = offlineFixture();
  memberMismatch.evidence.predecessorMembers[0].sha256 = "f".repeat(64);
  assert.throws(
    () => prepareOfflineBootstrapFixture(memberMismatch.input, memberMismatch.evidence),
    /changed without an exact declaration/u,
  );
});

// keep injected evidence visibly separate from Git object evidence
test("offline fixture preparer rejects evidence mode substitution", () => {
  const fixture = offlineFixture();
  fixture.evidence.evidenceMode = "git-object-database";
  assert.throws(
    () => prepareOfflineBootstrapFixture(fixture.input, fixture.evidence),
    /must use injected-test-fixture/u,
  );
});

// decode canonical CLI evidence without changing its bytes
test("offline request decoder preserves exact base64 evidence", () => {
  const fixture = offlineFixture();
  const cliRequest = {
    bundleFiles: fixture.input.bundleFiles.map((file) => ({
      bytesBase64: file.bytes.toString("base64"),
      path: file.path,
    })),
    controlBase64: fixture.input.controlBytes.toString("base64"),
    installationReceiptBase64: fixture.input.installationReceiptBytes.toString("base64"),
    predecessor: fixture.input.predecessor,
    recoveryDropInBase64: fixture.input.recoveryDropInBytes.toString("base64"),
    requestVersion: fixture.input.requestVersion,
    sourceEnvironmentBase64: fixture.input.sourceEnvironmentBytes.toString("base64"),
    target: fixture.input.target,
  };
  const decoded = decodeOfflineBootstrapRequest(cliRequest);
  assert.deepEqual(decoded, fixture.input);
  const artifact = prepareOfflineBootstrapFixture(decoded, fixture.evidence);
  assert.equal(artifact.eligibleForLiveInstallation, false);
});

// reject noncanonical encoded evidence
test("offline request decoder rejects malformed byte evidence", () => {
  const fixture = offlineFixture();
  const request = {
    bundleFiles: fixture.input.bundleFiles.map((file) => ({
      bytesBase64: file.bytes.toString("base64"),
      path: file.path,
    })),
    controlBase64: fixture.input.controlBytes.toString("base64"),
    installationReceiptBase64: "not-base64",
    predecessor: fixture.input.predecessor,
    recoveryDropInBase64: fixture.input.recoveryDropInBytes.toString("base64"),
    requestVersion: fixture.input.requestVersion,
    sourceEnvironmentBase64: fixture.input.sourceEnvironmentBytes.toString("base64"),
    target: fixture.input.target,
  };
  assert.throws(
    () => decodeOfflineBootstrapRequest(request),
    /installationReceiptBase64 must be canonical base64/u,
  );
});

// compare the preparer and public CLI to the independent installer bundle identity
test("offline preparer emits the installer identity for repository operator bytes", () => {
  const root = mkdtempSync(join(tmpdir(), "weather-predecessor-rust-"));
  try {
    chmodSync(root, 0o700);
    const bundle = join(root, "bundle");
    mkdirSync(bundle, { mode: 0o700 });
    chmodSync(bundle, 0o700);
    const moduleBytes = readFileSync(OPERATOR_MODULE);
    const shellBytes = readFileSync(OPERATOR_SHELL);
    const modulePath = join(bundle, "schema-preserving-release.mjs");
    const shellPath = join(bundle, "schema-preserving-release.sh");
    writeFileSync(modulePath, moduleBytes, { mode: 0o644 });
    writeFileSync(shellPath, shellBytes, { mode: 0o755 });
    chmodSync(modulePath, 0o644);
    chmodSync(shellPath, 0o755);
    const installerBundleSha256 = run("bash", [
      "-c",
      'source "$1"; spi_bundle_sha "$2"',
      "predecessor-bundle-fixture",
      INSTALLER.pathname,
      bundle,
    ]);
    const git = createGitTransitionRepository(root);
    const fixture = offlineFixture();
    fixture.input.bundleFiles = [
      { bytes: moduleBytes, path: "scripts/schema-preserving-release.mjs" },
      { bytes: shellBytes, path: "scripts/schema-preserving-release.sh" },
    ];
    fixture.input.predecessor.bundleSha256 = installerBundleSha256;
    fixture.input.predecessor.commit = git.predecessorCommit;
    fixture.input.target.commit = git.targetCommit;
    const requestPath = join(root, "request.json");
    const request = {
      bundleFiles: fixture.input.bundleFiles.map((file) => ({
        bytesBase64: file.bytes.toString("base64"),
        path: file.path,
      })),
      controlBase64: fixture.input.controlBytes.toString("base64"),
      installationReceiptBase64: fixture.input.installationReceiptBytes.toString("base64"),
      predecessor: fixture.input.predecessor,
      recoveryDropInBase64: fixture.input.recoveryDropInBytes.toString("base64"),
      requestVersion: fixture.input.requestVersion,
      sourceEnvironmentBase64: fixture.input.sourceEnvironmentBytes.toString("base64"),
      target: fixture.input.target,
    };
    writeFileSync(requestPath, `${JSON.stringify(request)}\n`, { mode: 0o600 });
    chmodSync(requestPath, 0o600);

    const artifact = prepareOfflineBootstrap(fixture.input, git.repository);
    const cliArtifact = JSON.parse(run(
      process.execPath,
      [PREPARE_CLI.pathname, requestPath],
      git.repository,
    ));
    const expected = buildTwoFileBundleManifest([
      { bytes: moduleBytes, path: "schema-preserving-release.mjs" },
      { bytes: shellBytes, path: "schema-preserving-release.sh" },
    ]);
    const cliBundle = JSON.parse(run(process.execPath, [
      TRANSITION_CLI.pathname,
      "bundle-manifest",
      modulePath,
      shellPath,
    ]));
    assert.equal(artifact.operatorBundle.manifestSha256, installerBundleSha256);
    assert.deepEqual(artifact.operatorBundle, expected);
    assert.deepEqual(cliArtifact, artifact);
    assert.deepEqual(cliBundle, expected);
    assert.deepEqual(
      artifact.operatorBundle.members.map((member) => member.path),
      ["schema-preserving-release.mjs", "schema-preserving-release.sh"],
    );
    assert.equal(artifact.gitEvidenceMode, "git-object-database");
    assert.equal(artifact.eligibility, "offline-only-ineligible");
    assert.equal(artifact.eligibleForLiveInstallation, false);
    assert.equal(artifact.liveGate, OFFLINE_BOOTSTRAP_LIVE_GATE);
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});
