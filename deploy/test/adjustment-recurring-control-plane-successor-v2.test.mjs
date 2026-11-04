import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmod,
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rename,
  rm,
  stat,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test, { after } from "node:test";

const repoRoot = resolve(import.meta.dirname, "../..");
const repositoryInstaller = join(repoRoot, "scripts/install-adjustment-recurring-control-plane-successor-v2.sh");
const repositoryLibrary = join(repoRoot, "scripts/install-adjustment-recurring-control-plane.sh");
const frozenLibrarySha256 = "f3df96877b938b67ab0c8eac497b2ee07c9b78db77e25896002ef51c9928fcd9";
const predecessorCommit = "238f41e36e2491f8ae09370d7740e59ca8ac4cc2";
const predecessorMigrateCommit = "461730a566e2ca6672a179c60607615af2c94913";
const changedPaths = ["deploy/scripts/update.sh"];
const legacyDigest = "af80d5752590bf47f5b893ee69f04215132e80210993f4a8c78d8562d2ea9a52";
const predecessorDigest = "e089e5c092fb336d0f8130cbf45320ed013a55af3b10957f2fd95a03265a2ed2";
const bridgeBytes = `WEATHER_V14_BRIDGE_CONTRACT_VERSION=weather-v14-compatibility-bridge/v1
WEATHER_V14_BRIDGE_SOURCE_RELEASE=2026.10.09-1
WEATHER_V14_BRIDGE_TARGET_RELEASE=2026.10.09-2
WEATHER_V14_BRIDGE_TARGET_COMMIT=f6daa89d135661eb60dd5479c5d0d6c23e7198a9
WEATHER_V14_BRIDGE_CONTROL_SHA256=${legacyDigest}
WEATHER_V14_BRIDGE_SOURCE_HISTORY_SHA256=e4a73765078fba8235bf94753c799c209d59f737dde6f38d1a1a458e3164a82e
WEATHER_V14_BRIDGE_SETTINGS_INODE=45826:526937
WEATHER_V14_BRIDGE_SETTINGS_SHA256=9d3911129378ef3b4e2c0a310d04900d0aefaeb7394a215314e9665518eb1275
WEATHER_V14_BRIDGE_SOURCE_ENV_SHA256=4d34c1d82ce5b02a0911cf0371ddb9c9be56c2bf5d9096f84a590d75c4b4f724
WEATHER_V14_BRIDGE_TARGET_ENV_SHA256=97368a14e368dcf7fd3f3f179cd222df2f848695e1cd993abf8892e69c30d4e7
WEATHER_V14_BRIDGE_PREVIOUS_RELEASE=2026.10.07-3
`;
const targetEnvironment = `WEATHER_RELEASE=2026.10.09-2
WEATHER_SERVER_IMAGE=ghcr.io/anstosa/weather-server@sha256:587dd0d34ed19086986a8081f7889fb10af2146d0d3f9ba46914389a787e5437
WEATHER_WEB_IMAGE=ghcr.io/anstosa/weather-web@sha256:34d824b03c5f84289b8f38028e751f183a953565fc929260c09b07c9e64f15ba
POSTGRES_IMAGE=postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
CLOUDFLARED_IMAGE=cloudflare/cloudflared@sha256:a85d5a3d6f22cb3c7e78b2f0d05b0f0daeb72566e9426f656c60b357b7b89c95
WEATHER_DATABASE_NAME=weather
WEATHER_POSTGRES_DIR=/var/lib/weather/postgres
WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=0
WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=0
WEATHER_CONTROL_PLANE_SHA256=${legacyDigest}
WEATHER_CONTROL_PLANE_VERSION=15
`;
const stagedEnvironment = `WEATHER_RELEASE=2026.10.09-3
WEATHER_SERVER_IMAGE=ghcr.io/anstosa/weather-server@sha256:5ebb1c9686e71053a56958ffe100436897082b7c41b3d072793303e73f508a64
WEATHER_WEB_IMAGE=ghcr.io/anstosa/weather-web@sha256:b3ceecc4a5c14e1bac420f80702d46b99ae7a302d68cd1b7d6b69cb20ec22d8b
POSTGRES_IMAGE=postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
CLOUDFLARED_IMAGE=cloudflare/cloudflared@sha256:a85d5a3d6f22cb3c7e78b2f0d05b0f0daeb72566e9426f656c60b357b7b89c95
WEATHER_DATABASE_NAME=weather
WEATHER_POSTGRES_DIR=/var/lib/weather/postgres
WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=0
WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=0
WEATHER_CONTROL_PLANE_SHA256=${predecessorDigest}
WEATHER_CONTROL_PLANE_VERSION=16
`;
const v16Handoff = `{"bridgeStateSha256":"c944a152f3129d7bf3bf3b2dcd2b4d230ca58c64719360e22351eb8043779c62","candidateIdentityManifestSha256":"157281ebd1ebc35691a34e085722a72ea3a530ca9b6cc75832d77ab6d868be5e","contractVersion":"weather-control-plane-handoff/v16-v1","installedAt":"2026-11-03T00:38:21Z","predecessorIdentityManifestSha256":"d1f64bf6c03fa35d198d6e9edc58b9d9ac4225e829e2c41e68761fe5ca827536","receiptSha256":"f1acdcf00a21e7937b639d9d41910d695c5589175083eb5c3ba97f07066e7bfa","sourceControlSha256":"af80d5752590bf47f5b893ee69f04215132e80210993f4a8c78d8562d2ea9a52","sourceControlVersion":15,"sourceRelease":"2026.10.09-2","successorControlSha256":"e089e5c092fb336d0f8130cbf45320ed013a55af3b10957f2fd95a03265a2ed2","successorControlVersion":16}\n`;

assert.equal(createHash("sha256").update(bridgeBytes).digest("hex"),
  "c944a152f3129d7bf3bf3b2dcd2b4d230ca58c64719360e22351eb8043779c62");
assert.equal(createHash("sha256").update(targetEnvironment).digest("hex"),
  "97368a14e368dcf7fd3f3f179cd222df2f848695e1cd993abf8892e69c30d4e7");
assert.equal(createHash("sha256").update(stagedEnvironment).digest("hex"),
  "f4bd73a18649894a25d9b4f54784bb5eccb099b4226dd5aa7df50f09906dc63b");
assert.equal(createHash("sha256").update(v16Handoff).digest("hex"),
  "63daabc14561306402d6130d8d644dc7025148f837b40f34791f00b3106bef2e");

// hash one fixture file
async function fileHash(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

// create one private same-directory script pair
async function createPrivateInstallerFixture(sourceInstaller, sourceLibrary) {
  const root = await mkdtemp(join(tmpdir(), "weather-successor-scripts-"));
  const scripts = join(root, "scripts");
  const installerPath = join(scripts, "install-adjustment-recurring-control-plane-successor-v2.sh");
  const libraryPath = join(scripts, "install-adjustment-recurring-control-plane.sh");
  try {
    await mkdir(scripts, { mode: 0o700 });
    await cp(sourceInstaller, installerPath);
    await cp(sourceLibrary, libraryPath);
    await chmod(installerPath, 0o700);
    await chmod(libraryPath, 0o700);
    const libraryStat = await stat(libraryPath);
    assert.equal(await fileHash(installerPath), await fileHash(sourceInstaller));
    assert.equal(await fileHash(libraryPath), frozenLibrarySha256);
    assert.equal(libraryStat.mode & 0o777, 0o700);
    assert.equal(libraryStat.nlink, 1);
    return { installer: installerPath, library: libraryPath, root };
  } catch (error) {
    // clean a rejected fixture
    await rm(root, { force: true, recursive: true });
    throw error;
  }
}

const privateInstallerFixture = await createPrivateInstallerFixture(repositoryInstaller, repositoryLibrary);
const installer = privateInstallerFixture.installer;

// dispose the shared private scripts after every focused test
after(async () => {
  await rm(privateInstallerFixture.root, { force: true, recursive: true });
});

// read the frozen identity rows from the reviewed installer packet
function predecessorIdentities() {
  const result = spawnSync("bash", ["-c", 'source "$1"; predecessor_identity_manifest',
    "successor-predecessor-identities", installer], { cwd: repoRoot, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim().split("\n").map((line) => {
    const [sha256, uid, gid, mode, relativePath] = line.split(" ");
    return { gid, mode, relativePath, sha256, uid };
  });
}

// run one semantic evidence identity calculation
function evidenceIdentity(path, extra = "") {
  return spawnSync("bash", ["-c", `source "$1"
${extra}
successor_evidence_tree_identity "$2"`, "successor-evidence-identity", installer, path],
  { cwd: repoRoot, encoding: "utf8" });
}

// materialize the exact E089 predecessor from committed git objects
async function writePredecessorTree(root) {
  const identities = predecessorIdentities();
  assert.equal(identities.length, 42);
  // reproduce every reviewed byte without ignored rollout evidence
  for (const identity of identities) {
    const commit = identity.relativePath === "deploy/scripts/migrate.mjs"
      ? predecessorMigrateCommit
      : predecessorCommit;
    const bytes = execFileSync("git", ["show", `${commit}:${identity.relativePath}`], { cwd: repoRoot });
    const target = join(root, identity.relativePath);
    assert.equal(createHash("sha256").update(bytes).digest("hex"), identity.sha256,
      identity.relativePath);
    assert.equal(identity.uid, "0");
    assert.equal(identity.gid, "0");
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, bytes, { mode: Number.parseInt(identity.mode, 8) });
    await chmod(target, Number.parseInt(identity.mode, 8));
  }
}

// build one exact E089 successor fixture
async function createFixture() {
  const root = await mkdtemp(join(tmpdir(), "weather-control-successor-v2-"));
  const installed = join(root, "installed");
  const candidate = join(root, "candidate");
  const runtime = join(root, "runtime");
  const backups = join(runtime, "control-plane-backups");
  const archive = join(root, "candidate.tar");
  // create fixed roots before extracting control bytes
  for (const path of [installed, candidate, runtime, backups]) {
    await mkdir(path, { recursive: true, mode: 0o700 });
  }
  await writePredecessorTree(installed);
  await cp(join(installed, "deploy"), join(candidate, "deploy"), { recursive: true });
  // assemble the candidate from the reviewed control
  for (const relative of changedPaths) {
    await writeFile(join(candidate, relative), await readFile(join(repoRoot, relative)));
  }
  execFileSync("tar", ["-cf", archive, "-C", candidate, "deploy"]);
  const archiveSha256 = await fileHash(archive);
  const digest = spawnSync("bash", ["-c", 'source "$1"; control_digest "$2"',
    "successor-digest", installer, candidate], { encoding: "utf8" });
  assert.equal(digest.status, 0, digest.stderr);

  const state = join(installed, "deploy/state");
  const releases = join(installed, "deploy/releases");
  const xweather = join(runtime, "xweather");
  await mkdir(state);
  await mkdir(releases);
  await mkdir(join(xweather, "adjustment-evidence"), { recursive: true });
  await mkdir(join(xweather, "adjustment-release-state"), { recursive: true });
  await writeFile(join(xweather, "adjustment-evidence/current.json"), "evidence\n", { mode: 0o600 });
  await writeFile(join(xweather, "adjustment-release-state/current.json"), "release\n", { mode: 0o600 });
  await writeFile(join(xweather, "forecast-adjustment-settings.json"), "fixture-settings\n", { mode: 0o600 });
  await writeFile(join(releases, "2026.10.09-1.env"), "source-fixture\n", { mode: 0o600 });
  await writeFile(join(releases, "2026.10.09-2.env"), targetEnvironment, { mode: 0o600 });
  await writeFile(join(releases, "2026.10.09-3.env"), stagedEnvironment, { mode: 0o600 });
  await writeFile(join(state, "v14-compatibility-bridge-v1"), bridgeBytes, { mode: 0o600 });
  await writeFile(join(state, "control-plane-handoff-v16-v1.json"), v16Handoff, { mode: 0o600 });
  const failureEvidence = join(state, "historical-v14-rehearsal-failure-nxIGVo");
  await mkdir(failureEvidence, { mode: 0o700 });
  await writeFile(join(failureEvidence, "failure.log"), "preserved failure\n", { mode: 0o600 });
  for (const [name, value] of [["current-release", "2026.10.09-2\n"],
    ["schema-release", "2026.10.09-2\n"], ["previous-release", "2026.10.09-1\n"]]) {
    await writeFile(join(state, name), value, { mode: 0o600 });
  }
  await symlink("../releases/2026.10.09-2.env", join(state, "active.env"));
  // model the exact three task-owned retained backups
  for (const name of ["v15-08b25ce2c80b.W53hmfJ4", "v15-08b25ce2c80b.Ips8xyrd",
    "successor-af80d5752590.42gfsNcs"]) {
    await mkdir(join(backups, name), { mode: 0o700 });
    await writeFile(join(backups, name, "retained"), "retained\n", { mode: 0o600 });
  }
  return { archive, archiveSha256, backups, candidate, candidateDigest: digest.stdout.trim(),
    installed, root, runtime, state };
}

// execute the sourced installer against a fixture
function runInstaller(fixture, failureStep = 0, extra = "") {
  const script = `source "$1"
successor_require_quiet_weather() { :; }
successor_verify_source_environment_identity() { [[ -f "$1" && ! -L "$1" ]]; }
successor_verify_settings_identity() { [[ -f "$1" && ! -L "$1" ]]; }
maintenance_filesystem_sample() { printf '%s %s\\n' 999999999999 999999999; }
${extra}
install_successor_control_plane_v2 "$2" "$3" "$4" "$5" "$6" "$7" "$8" "$(id -u)" "$(id -g)" "$(id -u)" "$(id -g)"`;
  return spawnSync("bash", ["-c", script, "successor-install", installer, fixture.installed,
    fixture.backups, fixture.runtime, fixture.archive, fixture.archiveSha256,
    fixture.candidateDigest, String(failureStep)], { encoding: "utf8" });
}

// execute the distinct successor recovery path
function runRecovery(fixture, backup, extra = "") {
  const script = `source "$1"
successor_require_quiet_weather() { :; }
successor_verify_source_environment_identity() { [[ -f "$1" && ! -L "$1" ]]; }
successor_verify_settings_identity() { [[ -f "$1" && ! -L "$1" ]]; }
${extra}
recover_successor_control_plane_v2 "$2" "$3" "$4" "$5" "$(id -u)" "$(id -g)" "$(id -u)" "$(id -g)"`;
  return spawnSync("bash", ["-c", script, "successor-recover", installer, fixture.installed,
    fixture.backups, fixture.runtime, backup], { encoding: "utf8" });
}

// locate the retained successor transaction
async function successorBackup(fixture) {
  const name = (await readdir(fixture.backups)).find((entry) => entry.startsWith("successor-e089e5c092fb."));
  assert.ok(name);
  return join(fixture.backups, name);
}

// compare the installed controls to exact E089 bytes
async function assertPredecessor(fixture) {
  const result = spawnSync("bash", ["-c",
    'source "$1"; successor_verify_predecessor_identities "$2" "$(id -u)" "$(id -g)"',
    "successor-predecessor", installer, fixture.installed], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
}

// capture evidence bytes that must never change
async function preservedEvidence(fixture) {
  return Promise.all([
    readFile(join(fixture.installed, "deploy/releases/2026.10.09-1.env")),
    readFile(join(fixture.installed, "deploy/releases/2026.10.09-2.env")),
    readFile(join(fixture.installed, "deploy/releases/2026.10.09-3.env")),
    readFile(join(fixture.state, "v14-compatibility-bridge-v1")),
    readFile(join(fixture.state, "control-plane-handoff-v16-v1.json")),
    readFile(join(fixture.state, "historical-v14-rehearsal-failure-nxIGVo/failure.log")),
    readFile(join(fixture.runtime, "xweather/adjustment-evidence/current.json")),
    readFile(join(fixture.runtime, "xweather/adjustment-release-state/current.json")),
  ]);
}

// hold one flock until the caller terminates it
function holdLock(path) {
  const child = spawn("bash", ["-c", 'exec 9<"$1"; flock -x 9; printf ready; sleep 60',
    "successor-lock", path], { stdio: ["ignore", "pipe", "ignore"] });
  return new Promise((resolveReady) => child.stdout.once("data", () => resolveReady(child)));
}

// release one held lock
async function releaseLock(child) {
  const exit = new Promise((resolveExit) => child.once("exit", resolveExit));
  child.kill("SIGTERM");
  await exit;
}

// convert git executable modes into the private production identity
test("successor tests isolate a Git-like 0755 script checkout", async () => {
  const gitRoot = await mkdtemp(join(tmpdir(), "weather-successor-git-modes-"));
  const gitScripts = join(gitRoot, "scripts");
  let privateFixture;
  try {
    await mkdir(gitScripts, { mode: 0o700 });
    const gitInstaller = join(gitScripts, "install-adjustment-recurring-control-plane-successor-v2.sh");
    const gitLibrary = join(gitScripts, "install-adjustment-recurring-control-plane.sh");
    await cp(repositoryInstaller, gitInstaller);
    await cp(repositoryLibrary, gitLibrary);
    await chmod(gitInstaller, 0o755);
    await chmod(gitLibrary, 0o755);
    assert.equal((await stat(gitLibrary)).mode & 0o777, 0o755);
    privateFixture = await createPrivateInstallerFixture(gitInstaller, gitLibrary);
    assert.equal((await stat(privateFixture.library)).mode & 0o777, 0o700);
    const result = spawnSync("bash", ["-c", 'source "$1"; predecessor_identity_manifest',
      "successor-private-mode", privateFixture.installer], { cwd: repoRoot, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim().split("\n").length, 42);
  } finally {
    // remove both bounded script fixtures
    if (privateFixture) await rm(privateFixture.root, { force: true, recursive: true });
    await rm(gitRoot, { force: true, recursive: true });
  }
});

// ignore copied directory allocation while retaining semantic evidence identity
test("successor evidence identity survives directory churn and rejects semantic changes", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-successor-evidence-"));
  const source = join(root, "source");
  const copied = join(root, "copied");
  const nested = join(source, "nested");
  try {
    await mkdir(nested, { recursive: true, mode: 0o700 });
    // create and remove entries to model ext4 directory slack
    for (let index = 0; index < 40; index += 1) {
      await writeFile(join(nested, `long-evidence-entry-${String(index).padStart(3, "0")}`),
        `value-${index}\n`, { mode: 0o600 });
    }
    // retain only ten logical files after physical churn
    for (let index = 10; index < 40; index += 1) {
      await rm(join(nested, `long-evidence-entry-${String(index).padStart(3, "0")}`));
    }
    await cp(source, copied, { recursive: true, preserveTimestamps: true });
    const comparison = spawnSync("bash", ["-c", `source "$1"
source_tree=$2
stat() {
  if [[ "$1" == -c && "$2" == %s && -d "$3" ]]; then
    if [[ "$3" == "$source_tree"* ]]; then printf '36864\\n'; else printf '4096\\n'; fi
  else
    command stat "$@"
  fi
}
printf '%s\\n' "$(runtime_state_identity "$2")" "$(runtime_state_identity "$3")" "$(successor_evidence_tree_identity "$2")" "$(successor_evidence_tree_identity "$3")"`,
    "successor-evidence-copy", installer, source, copied], { cwd: repoRoot, encoding: "utf8" });
    assert.equal(comparison.status, 0, comparison.stderr);
    const [oldSource, oldCopy, semanticSource, semanticCopy] = comparison.stdout.trim().split("\n");
    assert.notEqual(oldSource, oldCopy);
    assert.equal(semanticSource, semanticCopy);

    const baseline = evidenceIdentity(source);
    assert.equal(baseline.status, 0, baseline.stderr);
    const retained = join(nested, "long-evidence-entry-000");
    const retainedBytes = await readFile(retained);
    const retainedMode = (await stat(retained)).mode & 0o777;

    await writeFile(join(nested, "extra-file"), "extra\n", { mode: 0o600 });
    assert.notEqual(evidenceIdentity(source).stdout, baseline.stdout);
    await rm(join(nested, "extra-file"));

    await writeFile(retained, Buffer.from("changed\n"));
    assert.notEqual(evidenceIdentity(source).stdout, baseline.stdout);
    await writeFile(retained, retainedBytes);

    await chmod(retained, 0o640);
    assert.notEqual(evidenceIdentity(source).stdout, baseline.stdout);
    await chmod(retained, retainedMode);

    const renamed = join(nested, "renamed-evidence-entry");
    await rename(retained, renamed);
    assert.notEqual(evidenceIdentity(source).stdout, baseline.stdout);
    await rename(renamed, retained);

    const linked = join(nested, "linked-evidence-entry");
    await symlink("long-evidence-entry-000", linked);
    const linkedResult = evidenceIdentity(source);
    assert.notEqual(linkedResult.status, 0, `${linkedResult.stdout}\n${linkedResult.stderr}`);
    assert.match(linkedResult.stderr, /unsafe entry/u);
    await rm(linked);

    const empty = join(root, "empty");
    await mkdir(empty, { mode: 0o700 });
    const findFailure = evidenceIdentity(empty, "find() { return 23; }");
    assert.notEqual(findFailure.status, 0);
    assert.equal(findFailure.stdout, "");
    assert.match(findFailure.stderr, /evidence state enumeration failed/u);

    const concurrent = evidenceIdentity(source,
      "successor_evidence_identity_probe() { printf 'concurrent\\n' >>\"$1/nested/long-evidence-entry-000\"; }");
    assert.notEqual(concurrent.status, 0);
    assert.equal(concurrent.stdout, "");
    assert.match(concurrent.stderr, /changed during semantic identity/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// bind the embedded predecessor and helper contract
test("successor embeds the exact E089 packet and one-path order", async () => {
  assert.ok((await readFile(installer, "utf8")).startsWith("#!/bin/bash\n"));
  const result = spawnSync("bash", ["-c",
    'source "$1"; predecessor_identity_manifest; printf "%s\\n" --changes--; successor_changed_path_manifest',
    "successor-manifest", installer], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const [manifest, changes] = result.stdout.split("--changes--\n");
  assert.equal(manifest.trim().split("\n").length, 42);
  assert.equal(createHash("sha256").update(manifest).digest("hex"),
    "157281ebd1ebc35691a34e085722a72ea3a530ca9b6cc75832d77ab6d868be5e");
  assert.deepEqual(changes.trim().split("\n"), changedPaths);
});

// validate the complete candidate assembled from current reviewed controls
test("successor accepts the real v16 handoff constants and 42-file candidate", async () => {
  const fixture = await createFixture();
  try {
    const updater = await readFile(join(repoRoot, "deploy/scripts/update.sh"), "utf8");
    assert.ok(updater.split("\n").includes("control_plane_version=17"));
    // bind the candidate byte to the live reviewed file
    for (const relative of changedPaths) {
      assert.equal(await fileHash(join(fixture.candidate, relative)), await fileHash(join(repoRoot, relative)));
    }
    const result = spawnSync("bash", ["-c",
      'source "$1"; successor_verify_candidate "$2" "$3"',
      "successor-current-candidate", installer, fixture.candidate, fixture.candidateDigest],
    { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// reject staging outside the measured runtime filesystem before allocation
test("successor rejects a distinct staging device without mutation", async () => {
  const fixture = await createFixture();
  try {
    const before = await preservedEvidence(fixture);
    const archiveBefore = await fileHash(fixture.archive);
    const result = runInstaller(fixture, 0, `successor_test_runtime="$4"
stat() {
  if [[ "$1" == -c && "$2" == %d && "$3" == "\${TMPDIR:-/tmp}" ]]; then
    printf '101\\n'
  elif [[ "$1" == -c && "$2" == %d && "$3" == "$successor_test_runtime" ]]; then
    printf '202\\n'
  else
    command stat "$@"
  fi
}
require_maintenance_capacity() { printf 'capacity-called\\n' >&2; return 1; }`);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /staging filesystem differs/u);
    assert.doesNotMatch(result.stderr, /capacity-called/u);
    await assertPredecessor(fixture);
    assert.deepEqual(await preservedEvidence(fixture), before);
    assert.equal(await fileHash(fixture.archive), archiveBefore);
    assert.equal((await readdir(fixture.backups)).filter((entry) => entry.startsWith("successor-e089e5c092fb.")).length, 0);
    await assert.rejects(stat(join(fixture.state, "control-plane-handoff-v17-v1.json")), /ENOENT/u);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// install only the exact successor and publish one canonical receipt
test("successor installs one control last and retains sealed recovery", async () => {
  const fixture = await createFixture();
  try {
    const before = await preservedEvidence(fixture);
    const result = runInstaller(fixture);
    assert.equal(result.status, 0, result.stderr);
    for (const relative of changedPaths) {
      assert.equal(await fileHash(join(fixture.installed, relative)), await fileHash(join(fixture.candidate, relative)));
    }
    assert.deepEqual(await preservedEvidence(fixture), before);
    const receiptBytes = await readFile(join(fixture.state, "control-plane-handoff-v17-v1.json"), "utf8");
    assert.ok(receiptBytes.endsWith("\n"));
    const receipt = JSON.parse(receiptBytes);
    assert.deepEqual(Object.keys(receipt).sort(), ["bridgeStateSha256", "candidateIdentityManifestSha256",
      "contractVersion", "installedAt", "predecessorHandoffFileSha256",
      "predecessorHandoffReceiptSha256", "predecessorIdentityManifestSha256", "receiptSha256",
      "sourceControlSha256", "sourceControlVersion", "sourceEnvironmentSha256", "sourceRelease",
      "successorControlSha256", "successorControlVersion"]);
    assert.equal(receipt.sourceRelease, "2026.10.09-3");
    assert.equal(receipt.contractVersion, "weather-control-plane-handoff/v17-v1");
    assert.equal(receipt.sourceControlVersion, 16);
    assert.equal(receipt.sourceControlSha256, predecessorDigest);
    assert.equal(receipt.sourceEnvironmentSha256,
      "f4bd73a18649894a25d9b4f54784bb5eccb099b4226dd5aa7df50f09906dc63b");
    assert.equal(receipt.predecessorHandoffFileSha256,
      "63daabc14561306402d6130d8d644dc7025148f837b40f34791f00b3106bef2e");
    assert.equal(receipt.predecessorHandoffReceiptSha256,
      "f1acdcf00a21e7937b639d9d41910d695c5589175083eb5c3ba97f07066e7bfa");
    assert.equal(receipt.predecessorIdentityManifestSha256,
      "157281ebd1ebc35691a34e085722a72ea3a530ca9b6cc75832d77ab6d868be5e");
    assert.equal(receipt.successorControlVersion, 17);
    assert.equal(receipt.successorControlSha256, fixture.candidateDigest);
    const receiptSha256 = receipt.receiptSha256;
    delete receipt.receiptSha256;
    assert.equal(receiptSha256, createHash("sha256").update(`${JSON.stringify(receipt)}\n`).digest("hex"));
    const backup = await successorBackup(fixture);
    const candidateIdentities = await readFile(join(backup, "successor-identities.txt"), "utf8");
    assert.equal(receipt.candidateIdentityManifestSha256,
      createHash("sha256").update(candidateIdentities).digest("hex"));
    assert.equal(candidateIdentities.trim().split("\n").length, 42);
    assert.ok(candidateIdentities.trim().split("\n").every((line) => /^[a-f0-9]{64} 0 0 0[0-7]{3} /u.test(line)));
    assert.match(await readFile(join(backup, "sealed-transaction.manifest"), "utf8"),
      /adjustment-maintenance-control-successor-recovery\/v2/u);
    assert.equal((await readdir(join(backup, "control/deploy/scripts"))).join("\n"), "update.sh");
    const physicalBytes = Number(execFileSync("du", ["-sx", "--block-size=1", backup],
      { encoding: "utf8" }).trim().split(/\s+/u)[0]);
    const allocation = spawnSync("bash", ["-c",
      'source "$1"; successor_recovery_allocation "$2" "$3"', "successor-allocation", installer,
      fixture.installed, fixture.runtime], { encoding: "utf8" });
    assert.equal(allocation.status, 0, allocation.stderr);
    const upperBoundBytes = Number(allocation.stdout.trim().split(" ")[0]);
    assert.ok(physicalBytes <= upperBoundBytes, `${physicalBytes} > ${upperBoundBytes}`);
    assert.ok(upperBoundBytes <= 5_853_184, String(upperBoundBytes));
    assert.ok(physicalBytes <= 5_853_184, String(physicalBytes));
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// restore E089 after each replacement and receipt publication failure
test("successor failure probes restore E089 and preserve evidence", async () => {
  // exercise backup, replacement and publication failures
  for (let step = 1; step <= 3; step += 1) {
    const fixture = await createFixture();
    try {
      const before = await preservedEvidence(fixture);
      const result = runInstaller(fixture, step);
      assert.notEqual(result.status, 0, `step ${step} passed`);
      assert.match(result.stderr, /failure probe/u);
      await assertPredecessor(fixture);
      assert.deepEqual(await preservedEvidence(fixture), before);
      await assert.rejects(stat(join(fixture.state, "control-plane-handoff-v17-v1.json")), /ENOENT/u);
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  }
});

// retain an authoritative receipt when preserved evidence advances
test("successor rollback preserves advanced evidence and the published receipt", async () => {
  const fixture = await createFixture();
  try {
    const result = runInstaller(fixture, 3, `successor_failure_probe() {
  printf 'advanced\\n' >>"$2/xweather/adjustment-evidence/current.json"
}`);
    assert.equal(result.status, 2, result.stderr);
    await assertPredecessor(fixture);
    assert.equal(await readFile(join(fixture.runtime, "xweather/adjustment-evidence/current.json"), "utf8"),
      "evidence\nadvanced\n");
    const receipt = join(fixture.state, "control-plane-handoff-v17-v1.json");
    assert.equal((await stat(receipt)).isFile(), true);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// reject source evidence drift across the backup copy
test("successor backup refuses concurrent evidence mutation before transaction", async () => {
  const fixture = await createFixture();
  try {
    const result = runInstaller(fixture, 0, `successor_evidence_copy_probe() {
  if [[ "$2" == 0 ]]; then printf 'advanced\\n' >>"$1/current.json"; fi
}`);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /evidence source changed during backup/u);
    await assertPredecessor(fixture);
    assert.equal(await readFile(join(fixture.runtime, "xweather/adjustment-evidence/current.json"), "utf8"),
      "evidence\nadvanced\n");
    assert.equal((await readdir(fixture.backups)).filter((entry) => entry.startsWith("successor-e089e5c092fb.")).length, 0);
    await assert.rejects(stat(join(fixture.state, "control-plane-handoff-v17-v1.json")), /ENOENT/u);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// reject evidence enumeration failure before creating a recovery backup
test("successor fails closed when evidence find fails", async () => {
  const fixture = await createFixture();
  try {
    const before = await preservedEvidence(fixture);
    const result = runInstaller(fixture, 0, `successor_test_runtime="$4"
find() {
  if [[ "$1" == -P && ( "$2" == "$successor_test_runtime/xweather/adjustment-evidence" ||
    "$2" == "$successor_test_runtime/xweather/adjustment-release-state" ) ]]; then
    return 23
  fi
  command find "$@"
}`);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /evidence state enumeration failed|recovery source enumeration failed/u);
    await assertPredecessor(fixture);
    assert.deepEqual(await preservedEvidence(fixture), before);
    assert.equal((await readdir(fixture.backups)).filter((entry) => entry.startsWith("successor-e089e5c092fb.")).length, 0);
    await assert.rejects(stat(join(fixture.state, "control-plane-handoff-v17-v1.json")), /ENOENT/u);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// reject future-artifact enumeration failure before mutation
test("successor fails closed when future-artifact enumeration fails", async () => {
  const fixture = await createFixture();
  try {
    const before = await preservedEvidence(fixture);
    const result = runInstaller(fixture, 0, `successor_test_installed="$2"
find() {
  # fail only the release-artifact authority read
  if [[ "$1" == -P && "$2" == "$successor_test_installed/deploy/releases" ]]; then
    return 23
  fi
  command find "$@"
}`);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /artifact enumeration failed/u);
    await assertPredecessor(fixture);
    assert.deepEqual(await preservedEvidence(fixture), before);
    assert.equal((await readdir(fixture.backups)).filter((entry) =>
      entry.startsWith("successor-e089e5c092fb.")).length, 0);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// remove only the incomplete successor backup after a sealing failure
test("successor sealing failure cleans its partial backup without state rollback", async () => {
  const fixture = await createFixture();
  try {
    const before = await preservedEvidence(fixture);
    const result = runInstaller(fixture, 0, "successor_seal_backup() { return 1; }");
    assert.notEqual(result.status, 0);
    await assertPredecessor(fixture);
    assert.deepEqual(await preservedEvidence(fixture), before);
    assert.equal((await readdir(fixture.backups)).filter((entry) => entry.startsWith("successor-e089e5c092fb.")).length, 0);
    await assert.rejects(stat(join(fixture.state, "control-plane-handoff-v17-v1.json")), /ENOENT/u);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// recover only through the distinct successor interface
test("successor explicit recovery restores E089 without evidence rollback", async () => {
  const fixture = await createFixture();
  try {
    assert.equal(runInstaller(fixture).status, 0);
    const before = await preservedEvidence(fixture);
    const recovered = runRecovery(fixture, await successorBackup(fixture));
    assert.equal(recovered.status, 0, recovered.stderr);
    await assertPredecessor(fixture);
    assert.deepEqual(await preservedEvidence(fixture), before);
    await assert.rejects(stat(join(fixture.state, "control-plane-handoff-v17-v1.json")), /ENOENT/u);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// reject immutable handoff and staged-source substitutions before mutation
test("successor rejects wrong v16 handoff and staged environment identities", async () => {
  for (const relative of ["deploy/state/control-plane-handoff-v16-v1.json",
    "deploy/releases/2026.10.09-3.env"]) {
    const fixture = await createFixture();
    try {
      const before = await preservedEvidence(fixture);
      await writeFile(join(fixture.installed, relative), "substituted\n", { mode: 0o600 });
      const result = runInstaller(fixture);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /identity differs/u);
      await assertPredecessor(fixture);
      await assert.rejects(stat(join(fixture.state, "control-plane-handoff-v17-v1.json")), /ENOENT/u);
      const after = await preservedEvidence(fixture);
      const changedIndex = relative.includes("handoff") ? 4 : 2;
      // preserve every unrelated immutable source byte
      for (let index = 0; index < before.length; index += 1) {
        // skip only the intentional substitution
        if (index !== changedIndex) assert.deepEqual(after[index], before[index]);
      }
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  }
});

// reject predecessor metadata substitution before backup
test("successor rejects predecessor mode substitution", async () => {
  const fixture = await createFixture();
  try {
    await chmod(join(fixture.installed, "deploy/scripts/update.sh"), 0o600);
    const result = runInstaller(fixture);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /predecessor identity differs/u);
    assert.equal((await readdir(fixture.backups)).filter((entry) =>
      entry.startsWith("successor-e089e5c092fb.")).length, 0);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// reject a substituted frozen low-level library before sourcing it
test("successor rejects frozen library substitution", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-successor-library-substitution-"));
  const scripts = join(root, "scripts");
  try {
    await mkdir(scripts, { mode: 0o700 });
    const copiedInstaller = join(scripts, "install-adjustment-recurring-control-plane-successor-v2.sh");
    const copiedLibrary = join(scripts, "install-adjustment-recurring-control-plane.sh");
    await cp(repositoryInstaller, copiedInstaller);
    await cp(repositoryLibrary, copiedLibrary);
    await chmod(copiedInstaller, 0o700);
    await chmod(copiedLibrary, 0o700);
    await writeFile(copiedLibrary, "# substituted\n", { flag: "a" });
    const result = spawnSync("bash", ["-c", 'source "$1"', "successor-library", copiedInstaller],
      { encoding: "utf8" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /library identity differs/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// reject transaction overlap before backup or replacement
test("successor requires both state and backup locks", async () => {
  for (const lockKind of ["state", "backup"]) {
    const fixture = await createFixture();
    let holder;
    try {
      const lockPath = lockKind === "state" ? fixture.state
        : join(fixture.backups, ".adjustment-maintenance-control-install.lock");
      if (lockKind === "backup") await writeFile(lockPath, "", { mode: 0o600 });
      holder = await holdLock(lockPath);
      const result = runInstaller(fixture);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /transaction is in flight|installer is in flight/u);
      await assertPredecessor(fixture);
      assert.equal((await readdir(fixture.backups)).filter((entry) => entry.startsWith("successor-e089e5c092fb.")).length, 0);
    } finally {
      if (holder) await releaseLock(holder);
      await rm(fixture.root, { force: true, recursive: true });
    }
  }
});

// reject journals, future artifacts and receipt collisions before mutation
test("successor rejects active or future release artifacts", async () => {
  const scenarios = [
    ["state", "normal-activation-v1.env"],
    ["state", ".v14-compatibility-bridge-v1.pending"],
    ["releases", "2026.10.09-3.normal-stage-receipt.json"],
    ["state", "control-plane-handoff-v17-v1.json"],
  ];
  for (const [rootName, name] of scenarios) {
    const fixture = await createFixture();
    try {
      const root = rootName === "state" ? fixture.state : join(fixture.installed, "deploy/releases");
      await writeFile(join(root, name), "collision\n", { mode: 0o600 });
      const result = runInstaller(fixture);
      assert.notEqual(result.status, 0);
      await assertPredecessor(fixture);
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  }
});

// enforce the task-owned sixteen MiB ledger exactly
test("successor task recovery ledger admits equality and rejects one byte over", () => {
  for (const [existing, next, accepted] of [
    [10_924_032, 16_777_216 - 10_924_032, true],
    [10_924_032, 16_777_217 - 10_924_032, false],
  ]) {
    const result = spawnSync("bash", ["-c",
      'source "$1"; successor_require_task_recovery_capacity "$2" "$3"',
      "successor-capacity", installer, String(existing), String(next)], { encoding: "utf8" });
    assert.equal(result.status === 0, accepted, result.stderr);
  }
});

// refuse unreviewed or incomplete candidate changes
test("successor candidate rejects incomplete, substituted and extra changes", async () => {
  // reject absent, substituted and foreign changes
  for (const mutation of ["incomplete", "substitution", "extra"]) {
    const fixture = await createFixture();
    try {
      // restore the predecessor byte
      if (mutation === "incomplete") {
        await cp(join(fixture.installed, changedPaths[0]), join(fixture.candidate, changedPaths[0]));
      // substitute the reviewed byte
      } else if (mutation === "substitution") {
        await writeFile(join(fixture.candidate, changedPaths[0]), "# substituted\n", { flag: "a" });
      // add one foreign change
      } else {
        await writeFile(join(fixture.candidate, "deploy/scripts/status.sh"), "foreign\n");
      }
      execFileSync("tar", ["-cf", fixture.archive, "-C", fixture.candidate, "deploy"]);
      fixture.archiveSha256 = await fileHash(fixture.archive);
      const digest = spawnSync("bash", ["-c", 'source "$1"; control_digest "$2"',
        "successor-mutant-digest", installer, fixture.candidate], { encoding: "utf8" });
      fixture.candidateDigest = digest.stdout.trim();
      const result = runInstaller(fixture);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /exactly one|identity differs|unreviewed/u);
      await assertPredecessor(fixture);
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  }
});
