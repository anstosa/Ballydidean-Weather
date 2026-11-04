import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  chmod,
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
import { join, resolve } from "node:path";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "..");
const installer = join(repoRoot, "scripts/install-schema-preserving-release.sh");
const controlSha = "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb";
const release = "2026.11.03-3";

// hash one fixture payload
function hash(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// build one complete disposable installation boundary
async function createFixture(extraBundleFile = false) {
  const root = await mkdtemp(join(tmpdir(), "weather-schema-preserving-installer-"));
  const bundle = join(root, "bundle");
  const deploy = join(root, "current/deploy");
  const operators = join(root, "operators/schema-preserving-release");
  const units = join(root, "units");
  const state = join(deploy, "state");
  const releases = join(deploy, "releases");
  await Promise.all([
    mkdir(bundle, { mode: 0o700 }),
    mkdir(join(deploy, "scripts"), { recursive: true }),
    mkdir(join(deploy, "systemd"), { recursive: true }),
    mkdir(state, { recursive: true }),
    mkdir(releases, { recursive: true }),
    mkdir(resolve(operators, ".."), { recursive: true, mode: 0o755 }),
    mkdir(units, { mode: 0o755 }),
  ]);
  await chmod(bundle, 0o700);
  await chmod(resolve(operators, ".."), 0o755);
  await chmod(units, 0o755);
  const shellBytes = "#!/usr/bin/env bash\nprintf helper\\n\n";
  const moduleBytes = "export const helper = true;\n";
  await writeFile(join(bundle, "schema-preserving-release.sh"), shellBytes, { mode: 0o755 });
  await writeFile(join(bundle, "schema-preserving-release.mjs"), moduleBytes, { mode: 0o644 });
  await chmod(join(bundle, "schema-preserving-release.sh"), 0o755);
  await chmod(join(bundle, "schema-preserving-release.mjs"), 0o644);
  // introduce one closed-manifest violation when requested
  if (extraBundleFile) {
    await writeFile(join(bundle, "extra"), "unexpected\n", { mode: 0o600 });
  }
  const updater = `#!/usr/bin/env bash
deploy_dir="$(cd "$(dirname "\${BASH_SOURCE[0]}")/.." && pwd)"
state_dir="$deploy_dir/state"
releases_dir="$deploy_dir/releases"
control_plane_version=17
control_plane_digest() { printf '${controlSha}\\n'; }
acquire_release_transaction_lock() { printf 'lock\\n' >>"$deploy_dir/lock.log"; }
read_release_state() { cat "$1"; }
normal_release_source_env() { printf '%s/${release}.env\\n' "$releases_dir"; }
compose() {
  case "\${*: -1}" in
    postgres) printf '%064d\\n' 1 ;;
    api) printf '%064d\\n' 2 ;;
    worker) printf '%064d\\n' 3 ;;
    web) printf '%064d\\n' 4 ;;
    cloudflared) printf '%064d\\n' 5 ;;
    *) return 1 ;;
  esac
}
`;
  const updaterPath = join(deploy, "scripts/update.sh");
  await writeFile(updaterPath, updater, { mode: 0o755 });
  await chmod(updaterPath, 0o755);
  const unitBytes = `[Service]
ExecStart=/old/update.sh recover
ExecReload=/old/update.sh recover
ExecStop=/old/compose stop
`;
  await writeFile(join(deploy, "systemd/weather-compose.service"), unitBytes, { mode: 0o644 });
  await writeFile(join(units, "weather-compose.service"), unitBytes, { mode: 0o644 });
  await chmod(join(deploy, "systemd/weather-compose.service"), 0o644);
  await chmod(join(units, "weather-compose.service"), 0o644);
  const environmentBytes = `WEATHER_RELEASE=${release}\n`;
  await Promise.all([
    writeFile(join(state, "current-release"), `${release}\n`, { mode: 0o600 }),
    writeFile(join(state, "schema-release"), `${release}\n`, { mode: 0o600 }),
    writeFile(join(releases, `${release}.env`), environmentBytes, { mode: 0o600 }),
  ]);
  await symlink(`../releases/${release}.env`, join(state, "active.env"));
  const manifest = `${hash(moduleBytes)}  schema-preserving-release.mjs\n${hash(shellBytes)}  schema-preserving-release.sh\n`;
  return {
    bundle,
    bundleSha: hash(manifest),
    baseUnitSha: hash(unitBytes),
    deploy,
    environmentSha: hash(environmentBytes),
    operators,
    root,
    units,
    updaterSha: hash(updater),
  };
}

// invoke one sourced installation with fake systemd and Docker commands
function runInstaller(fixture, fault = "") {
  const command = `source "$1"
spi_expected_update_sha=$7
FAULT=$8
spi_source_env_sha=$9
spi_expected_base_sha=\${10}
FAULT_MARKER="$2/fault-marker"
FIXTURE_DEPLOY=$2
FIXTURE_OPERATORS=$4
FIXTURE_UNITS=$5
FIXTURE_SHA=$6
spi_control_digest() { printf '${controlSha}\\n'; }
docker() { printf 'true|healthy\\n'; }
systemctl() {
  if [[ "$1" == daemon-reload ]]; then
    if [[ "$FAULT" == reload && ! -e "$FAULT_MARKER" ]]; then : >"$FAULT_MARKER"; return 1; fi
    printf 'reload\\n' >>"$FIXTURE_DEPLOY/reloads.log"
    return
  fi
  local property="\${3#--property=}" path argv dropin="$FIXTURE_UNITS/weather-compose.service.d/10-schema-preserving-release.conf"
  if [[ "$FAULT" == show && -e "$dropin" && ! -e "$FAULT_MARKER" ]]; then : >"$FAULT_MARKER"; return 1; fi
  # model systemd's normalized duration property
  if [[ "$property" == TimeoutStartUSec ]]; then
    if [[ -e "$dropin" ]]; then value=10min; else value=5min; fi
    printf '%s\\n' "$value" | tee -a "$FIXTURE_DEPLOY/timeouts.log"
    return
  fi
  case "$property" in
    ExecStart|ExecReload)
      if [[ -e "$dropin" ]]; then
        path="$FIXTURE_OPERATORS/$FIXTURE_SHA/schema-preserving-release.sh"; argv="$path system-recover"
      else path=/old/update.sh; argv='/old/update.sh recover'; fi ;;
    ExecStop) path=/old/compose; argv='/old/compose stop' ;;
  esac
  printf '{ path=%s ; argv[]=%s ; ignore_errors=no ; }\\n' "$path" "$argv"
}
systemd-analyze() {
  if [[ "$FAULT" == verify && ! -e "$FAULT_MARKER" ]]; then : >"$FAULT_MARKER"; return 1; fi
  return 0
}
eval "$(declare -f spi_publish_dropin | sed '1s/spi_publish_dropin/spi_publish_dropin_real/')"
spi_publish_dropin() {
  # prove recovery authority exists before systemd publication begins
  [[ -f "$FIXTURE_DEPLOY/state/schema-preserving-release-v1/installation.json" ]] || return 91
  spi_publish_dropin_real "$@" || return 1
  [[ "$FAULT" != publish ]]
}
spi_install "$3" "$6" "$2" "$4" "$5"`;
  return spawnSync("bash", [
    "-c",
    command,
    "schema-preserving-installer-test",
    installer,
    fixture.deploy,
    fixture.bundle,
    fixture.operators,
    fixture.units,
    fixture.bundleSha,
    fixture.updaterSha,
    fault,
    fixture.environmentSha,
    fixture.baseUnitSha,
  ], { encoding: "utf8" });
}

// remove one disposable fixture
async function cleanup(fixture) {
  await rm(fixture.root, { force: true, recursive: true });
}

test("bundle hash uses the exact sorted sha256sum manifest", async () => {
  const fixture = await createFixture();
  try {
    const result = spawnSync("bash", ["-c", 'source "$1"; spi_bundle_sha "$2"',
      "bundle-hash-test", installer, fixture.bundle], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim(), fixture.bundleSha);
  } finally {
    await cleanup(fixture);
  }
});

test("bundle validation rejects unknown files and unsafe metadata", async () => {
  const extra = await createFixture(true);
  const unsafe = await createFixture();
  try {
    let result = spawnSync("bash", ["-c", 'source "$1"; spi_bundle_sha "$2"',
      "bundle-extra-test", installer, extra.bundle], { encoding: "utf8" });
    assert.notEqual(result.status, 0);
    await chmod(join(unsafe.bundle, "schema-preserving-release.sh"), 0o775);
    result = spawnSync("bash", ["-c", 'source "$1"; spi_bundle_sha "$2"',
      "bundle-mode-test", installer, unsafe.bundle], { encoding: "utf8" });
    assert.notEqual(result.status, 0);
  } finally {
    await Promise.all([cleanup(extra), cleanup(unsafe)]);
  }
});

test("installer publishes an immutable bundle, exact drop-in and private receipt", async () => {
  const fixture = await createFixture();
  try {
    const result = runInstaller(fixture);
    assert.equal(result.status, 0, result.stderr);
    const installed = join(fixture.operators, fixture.bundleSha);
    assert.deepEqual((await readdir(installed)).sort(),
      ["schema-preserving-release.mjs", "schema-preserving-release.sh"]);
    const dropin = await readFile(join(fixture.units,
      "weather-compose.service.d/10-schema-preserving-release.conf"), "utf8");
    assert.equal(dropin, `[Service]\nExecStart=\nExecStart=${installed}/schema-preserving-release.sh system-recover\nExecReload=\nExecReload=${installed}/schema-preserving-release.sh system-recover\nTimeoutStartSec=600\n`);
    const receiptPath = join(fixture.deploy,
      "state/schema-preserving-release-v1/installation.json");
    const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
    assert.equal(receipt.bundleSha256, fixture.bundleSha);
    assert.equal(receipt.execStop, "/old/compose|/old/compose stop");
    assert.equal(receipt.timeoutStartSec, 600);
    assert.equal((await stat(receiptPath)).mode & 0o777, 0o600);
    assert.equal((await readFile(join(fixture.deploy, "timeouts.log"), "utf8")).trim().split("\n").at(-1),
      "10min");
    const retry = runInstaller(fixture);
    assert.equal(retry.status, 0, retry.stderr);
  } finally {
    await cleanup(fixture);
  }
});

test("installer rejects bundle hash drift, substitutions and unknown drop-ins", async () => {
  const badBundleHash = await createFixture();
  const badUpdater = await createFixture();
  const incompleteTarget = await createFixture();
  const unknownDropin = await createFixture();
  try {
    badBundleHash.bundleSha = "0".repeat(64);
    let result = runInstaller(badBundleHash);
    assert.notEqual(result.status, 0);
    await writeFile(join(badUpdater.deploy, "scripts/update.sh"), "substituted\n", { mode: 0o755 });
    result = runInstaller(badUpdater);
    assert.notEqual(result.status, 0);
    const incomplete = join(incompleteTarget.operators, incompleteTarget.bundleSha);
    await mkdir(incomplete, { recursive: true, mode: 0o700 });
    await writeFile(join(incomplete, "schema-preserving-release.sh"), "partial\n", { mode: 0o755 });
    result = runInstaller(incompleteTarget);
    assert.notEqual(result.status, 0);
    const directory = join(unknownDropin.units, "weather-compose.service.d");
    await mkdir(directory);
    await writeFile(join(directory, "foreign.conf"), "[Service]\n", { mode: 0o644 });
    result = runInstaller(unknownDropin);
    assert.notEqual(result.status, 0);
  } finally {
    await Promise.all([
      cleanup(badBundleHash),
      cleanup(badUpdater),
      cleanup(incompleteTarget),
      cleanup(unknownDropin),
    ]);
  }
});

for (const fault of ["publish", "reload", "verify", "show"]) {
  // prove each post-publication failure restores only invocation-owned state
  test(`installer rolls back an invocation-owned drop-in after ${fault} failure`, async () => {
    const fixture = await createFixture();
    try {
      const result = runInstaller(fixture, fault);
      assert.notEqual(result.status, 0);
      const dropin = join(fixture.units,
        "weather-compose.service.d/10-schema-preserving-release.conf");
      await assert.rejects(readFile(dropin));
      await assert.doesNotReject(stat(join(fixture.operators, fixture.bundleSha)));
      const receiptPath = join(fixture.deploy,
        "state/schema-preserving-release-v1/installation.json");
      const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
      assert.equal(receipt.bundleSha256, fixture.bundleSha);
      assert.equal(receipt.timeoutStartSec, 600);
      const timeouts = (await readFile(join(fixture.deploy, "timeouts.log"), "utf8")).trim().split("\n");
      assert.equal(timeouts.at(-1), "5min");
      // retry from retained inert authority must finish the installation
      const retry = runInstaller(fixture);
      assert.equal(retry.status, 0, retry.stderr);
      await assert.doesNotReject(readFile(dropin));
    } finally {
      await cleanup(fixture);
    }
  });
}
