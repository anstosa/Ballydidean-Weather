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
  rm,
  stat,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test, { after } from "node:test";

const repoRoot = resolve(import.meta.dirname, "../..");
const repositoryInstaller = join(repoRoot, "scripts/install-adjustment-recurring-control-plane-successor-v1.sh");
const repositoryLibrary = join(repoRoot, "scripts/install-adjustment-recurring-control-plane.sh");
const frozenLibrarySha256 = "f3df96877b938b67ab0c8eac497b2ee07c9b78db77e25896002ef51c9928fcd9";
const af80BaseCommit = "f7ac5369379a6f7f93d03ffbe09ae50e1cfb4f54";
const af80MaintenanceCommit = "d308ff4072ff623df6ef37ca6bc16cab5a6d4412";
const af80MaintenancePaths = new Set([
  "deploy/postgres/runtime-acl-v2.sql",
  "deploy/scripts/adjustment-evaluation-package.mjs",
  "deploy/scripts/adjustment-evidence-store.mjs",
  "deploy/scripts/backup.sh",
  "deploy/scripts/common.sh",
  "deploy/scripts/public-stations-backfill.sh",
  "deploy/scripts/tempest-backfill.sh",
  "deploy/scripts/tide-backfill.sh",
  "deploy/scripts/update.sh",
  "deploy/scripts/web-server.mjs",
]);
const changedPaths = [
  "deploy/scripts/adjustment-evaluation-package.mjs",
  "deploy/scripts/common.sh",
  "deploy/scripts/update.sh",
];
const predecessorDigest = "af80d5752590bf47f5b893ee69f04215132e80210993f4a8c78d8562d2ea9a52";
const bridgeBytes = `WEATHER_V14_BRIDGE_CONTRACT_VERSION=weather-v14-compatibility-bridge/v1
WEATHER_V14_BRIDGE_SOURCE_RELEASE=2026.10.09-1
WEATHER_V14_BRIDGE_TARGET_RELEASE=2026.10.09-2
WEATHER_V14_BRIDGE_TARGET_COMMIT=f6daa89d135661eb60dd5479c5d0d6c23e7198a9
WEATHER_V14_BRIDGE_CONTROL_SHA256=${predecessorDigest}
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
WEATHER_CONTROL_PLANE_SHA256=${predecessorDigest}
WEATHER_CONTROL_PLANE_VERSION=15
`;

assert.equal(createHash("sha256").update(bridgeBytes).digest("hex"),
  "c944a152f3129d7bf3bf3b2dcd2b4d230ca58c64719360e22351eb8043779c62");
assert.equal(createHash("sha256").update(targetEnvironment).digest("hex"),
  "97368a14e368dcf7fd3f3f179cd222df2f848695e1cd993abf8892e69c30d4e7");

// hash one fixture file
async function fileHash(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

// create one private same-directory script pair
async function createPrivateInstallerFixture(sourceInstaller, sourceLibrary) {
  const root = await mkdtemp(join(tmpdir(), "weather-successor-scripts-"));
  const scripts = join(root, "scripts");
  const installerPath = join(scripts, "install-adjustment-recurring-control-plane-successor-v1.sh");
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

// materialize the exact AF80 predecessor from committed git objects
async function writeAf80Tree(root) {
  const identities = predecessorIdentities();
  assert.equal(identities.length, 42);
  // reproduce every reviewed byte without ignored rollout evidence
  for (const identity of identities) {
    const commit = af80MaintenancePaths.has(identity.relativePath)
      ? af80MaintenanceCommit
      : af80BaseCommit;
    const bytes = execFileSync("git", ["show", `${commit}:${identity.relativePath}`], { cwd: repoRoot });
    const target = join(root, identity.relativePath);
    assert.equal(createHash("sha256").update(bytes).digest("hex"), identity.sha256);
    assert.equal(identity.uid, "0");
    assert.equal(identity.gid, "0");
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, bytes, { mode: Number.parseInt(identity.mode, 8) });
    await chmod(target, Number.parseInt(identity.mode, 8));
  }
  assert.equal(identities.filter((identity) => af80MaintenancePaths.has(identity.relativePath)).length, 10);
}

// build one exact AF80 successor fixture
async function createFixture() {
  const root = await mkdtemp(join(tmpdir(), "weather-control-successor-v1-"));
  const installed = join(root, "installed");
  const candidate = join(root, "candidate");
  const runtime = join(root, "runtime");
  const backups = join(runtime, "control-plane-backups");
  const archive = join(root, "candidate.tar");
  // create fixed roots before extracting control bytes
  for (const path of [installed, candidate, runtime, backups]) {
    await mkdir(path, { recursive: true, mode: 0o700 });
  }
  await writeAf80Tree(installed);
  await cp(join(installed, "deploy"), join(candidate, "deploy"), { recursive: true });
  // assemble the candidate from the three real reviewed controls
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
  await writeFile(join(state, "v14-compatibility-bridge-v1"), bridgeBytes, { mode: 0o600 });
  for (const [name, value] of [["current-release", "2026.10.09-2\n"],
    ["schema-release", "2026.10.09-2\n"], ["previous-release", "2026.10.09-1\n"]]) {
    await writeFile(join(state, name), value, { mode: 0o600 });
  }
  await symlink("../releases/2026.10.09-2.env", join(state, "active.env"));
  // model the exact two task-owned retained backups
  for (const name of ["v15-08b25ce2c80b.W53hmfJ4", "v15-08b25ce2c80b.Ips8xyrd"]) {
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
install_successor_control_plane_v1 "$2" "$3" "$4" "$5" "$6" "$7" "$8" "$(id -u)" "$(id -g)" "$(id -u)" "$(id -g)"`;
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
recover_successor_control_plane_v1 "$2" "$3" "$4" "$5" "$(id -u)" "$(id -g)" "$(id -u)" "$(id -g)"`;
  return spawnSync("bash", ["-c", script, "successor-recover", installer, fixture.installed,
    fixture.backups, fixture.runtime, backup], { encoding: "utf8" });
}

// locate the retained successor transaction
async function successorBackup(fixture) {
  const name = (await readdir(fixture.backups)).find((entry) => entry.startsWith("successor-"));
  assert.ok(name);
  return join(fixture.backups, name);
}

// compare the installed controls to exact AF80 bytes
async function assertPredecessor(fixture) {
  const result = spawnSync("bash", ["-c",
    'source "$1"; successor_verify_predecessor_identities "$2" "$(id -u)" "$(id -g)"',
    "successor-predecessor", installer, fixture.installed], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
}

// capture evidence bytes that must never change
async function preservedEvidence(fixture) {
  return Promise.all([
    readFile(join(fixture.installed, "deploy/releases/2026.10.09-2.env")),
    readFile(join(fixture.state, "v14-compatibility-bridge-v1")),
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
    const gitInstaller = join(gitScripts, "install-adjustment-recurring-control-plane-successor-v1.sh");
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

// bind the embedded predecessor and helper contract
test("successor embeds the exact AF80 packet and three-path order", async () => {
  assert.ok((await readFile(installer, "utf8")).startsWith("#!/bin/bash\n"));
  const result = spawnSync("bash", ["-c",
    'source "$1"; predecessor_identity_manifest; printf "%s\\n" --changes--; successor_changed_path_manifest',
    "successor-manifest", installer], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const [manifest, changes] = result.stdout.split("--changes--\n");
  assert.equal(manifest.trim().split("\n").length, 42);
  assert.equal(createHash("sha256").update(manifest).digest("hex"),
    "d1f64bf6c03fa35d198d6e9edc58b9d9ac4225e829e2c41e68761fe5ca827536");
  assert.deepEqual(changes.trim().split("\n"), changedPaths);
});

// validate the complete candidate assembled from current reviewed controls
test("successor accepts the real v16 handoff constants and 42-file candidate", async () => {
  const fixture = await createFixture();
  try {
    const updater = await readFile(join(repoRoot, "deploy/scripts/update.sh"), "utf8");
    // require every historical and handoff marker
    for (const line of [
      "control_plane_version=16",
      "recurring_previous_release=2026.10.09-1",
      "recurring_previous_control_plane_sha256=603eb8f488ba78be3d7ecf76b0d587346d1c36432255d390d86d2b768c8fecba",
      "v16_handoff_source_release=2026.10.09-2",
      "v16_handoff_source_control_version=15",
      `v16_handoff_source_control_sha256=${predecessorDigest}`,
    ]) {
      assert.ok(updater.split("\n").includes(line), line);
    }
    // bind all three candidate bytes to the live reviewed files
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
    assert.equal((await readdir(fixture.backups)).filter((entry) => entry.startsWith("successor-")).length, 0);
    await assert.rejects(stat(join(fixture.state, "control-plane-handoff-v16-v1.json")), /ENOENT/u);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// install only the exact successor and publish one canonical receipt
test("successor installs three controls last and retains sealed recovery", async () => {
  const fixture = await createFixture();
  try {
    const before = await preservedEvidence(fixture);
    const result = runInstaller(fixture);
    assert.equal(result.status, 0, result.stderr);
    for (const relative of changedPaths) {
      assert.equal(await fileHash(join(fixture.installed, relative)), await fileHash(join(fixture.candidate, relative)));
    }
    assert.deepEqual(await preservedEvidence(fixture), before);
    const receiptBytes = await readFile(join(fixture.state, "control-plane-handoff-v16-v1.json"), "utf8");
    assert.ok(receiptBytes.endsWith("\n"));
    const receipt = JSON.parse(receiptBytes);
    assert.deepEqual(Object.keys(receipt).sort(), ["bridgeStateSha256", "candidateIdentityManifestSha256",
      "contractVersion", "installedAt", "predecessorIdentityManifestSha256", "receiptSha256",
      "sourceControlSha256", "sourceControlVersion", "sourceRelease", "successorControlSha256",
      "successorControlVersion"]);
    const receiptSha256 = receipt.receiptSha256;
    delete receipt.receiptSha256;
    assert.equal(receiptSha256, createHash("sha256").update(`${JSON.stringify(receipt)}\n`).digest("hex"));
    const backup = await successorBackup(fixture);
    const candidateIdentities = await readFile(join(backup, "successor-identities.txt"), "utf8");
    assert.equal(candidateIdentities.trim().split("\n").length, 42);
    assert.ok(candidateIdentities.trim().split("\n").every((line) => /^[a-f0-9]{64} 0 0 0[0-7]{3} /u.test(line)));
    assert.match(await readFile(join(backup, "sealed-transaction.manifest"), "utf8"),
      /adjustment-maintenance-control-successor-recovery\/v1/u);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// restore AF80 after each replacement and receipt publication failure
test("successor failure probes restore AF80 and preserve evidence", async () => {
  for (let step = 1; step <= 5; step += 1) {
    const fixture = await createFixture();
    try {
      const before = await preservedEvidence(fixture);
      const result = runInstaller(fixture, step);
      assert.notEqual(result.status, 0, `step ${step} passed`);
      assert.match(result.stderr, /failure probe/u);
      await assertPredecessor(fixture);
      assert.deepEqual(await preservedEvidence(fixture), before);
      await assert.rejects(stat(join(fixture.state, "control-plane-handoff-v16-v1.json")), /ENOENT/u);
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  }
});

// retain an authoritative receipt when preserved evidence advances
test("successor rollback preserves advanced evidence and the published receipt", async () => {
  const fixture = await createFixture();
  try {
    const result = runInstaller(fixture, 5, `successor_failure_probe() {
  printf 'advanced\\n' >>"$2/xweather/adjustment-evidence/current.json"
}`);
    assert.equal(result.status, 2, result.stderr);
    await assertPredecessor(fixture);
    assert.equal(await readFile(join(fixture.runtime, "xweather/adjustment-evidence/current.json"), "utf8"),
      "evidence\nadvanced\n");
    const receipt = join(fixture.state, "control-plane-handoff-v16-v1.json");
    assert.equal((await stat(receipt)).isFile(), true);
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
    assert.equal((await readdir(fixture.backups)).filter((entry) => entry.startsWith("successor-")).length, 0);
    await assert.rejects(stat(join(fixture.state, "control-plane-handoff-v16-v1.json")), /ENOENT/u);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// recover only through the distinct successor interface
test("successor explicit recovery restores AF80 without evidence rollback", async () => {
  const fixture = await createFixture();
  try {
    assert.equal(runInstaller(fixture).status, 0);
    const before = await preservedEvidence(fixture);
    const recovered = runRecovery(fixture, await successorBackup(fixture));
    assert.equal(recovered.status, 0, recovered.stderr);
    await assertPredecessor(fixture);
    assert.deepEqual(await preservedEvidence(fixture), before);
    await assert.rejects(stat(join(fixture.state, "control-plane-handoff-v16-v1.json")), /ENOENT/u);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
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
      assert.equal((await readdir(fixture.backups)).filter((entry) => entry.startsWith("successor-")).length, 0);
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
    ["state", "control-plane-handoff-v16-v1.json"],
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
    [7_094_272, 16_777_216 - 7_094_272, true],
    [7_094_272, 16_777_217 - 7_094_272, false],
  ]) {
    const result = spawnSync("bash", ["-c",
      'source "$1"; successor_require_task_recovery_capacity "$2" "$3"',
      "successor-capacity", installer, String(existing), String(next)], { encoding: "utf8" });
    assert.equal(result.status === 0, accepted, result.stderr);
  }
});

// refuse unreviewed or incomplete candidate changes
test("successor candidate rejects incomplete and extra changes", async () => {
  for (const mutation of ["incomplete", "extra"]) {
    const fixture = await createFixture();
    try {
      if (mutation === "incomplete") {
        await cp(join(fixture.installed, changedPaths[0]), join(fixture.candidate, changedPaths[0]));
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
      assert.match(result.stderr, /exactly three|unreviewed/u);
      await assertPredecessor(fixture);
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  }
});
