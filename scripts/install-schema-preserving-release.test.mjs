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
function runInstaller(fixture, fault = "", upgrade = false) {
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
UPGRADE=\${11}
PREVIOUS_SHA=\${12}
spi_control_digest() { printf '${controlSha}\\n'; }
# isolate external runtime reads while exercising the actual handoff filesystem operations
spi_runtime_check() {
  printf 'runtime-verified:%s\\n' "$4" >>"$FIXTURE_DEPLOY/runtime.log"
  # fail only the post-switch protected-state comparison
  if [[ "$FAULT" == infrastructure && "$4" == unchanged && ! -e "$FAULT_MARKER" ]]; then : >"$FAULT_MARKER"; return 1; fi
  # create only the inert fixture snapshot
  if [[ "$4" == snapshot ]]; then printf protected >"$3"; fi
}
docker() { printf 'true|healthy\\n'; }
systemctl() {
  if [[ "$1" == daemon-reload ]]; then
    if [[ "$FAULT" == reload && ! -e "$FAULT_MARKER" ]]; then : >"$FAULT_MARKER"; return 1; fi
    # model completing a switch that was persisted before systemd reloaded
    if [[ "$FAULT" == pending ]]; then : >"$FAULT_MARKER"; fi
    printf 'reload\\n' >>"$FIXTURE_DEPLOY/reloads.log"
    return
  fi
  local property="\${3#--property=}" path argv dropin="$FIXTURE_UNITS/weather-compose.service.d/10-schema-preserving-release.conf"
  # fail vector inspection only after the new bundle is exposed
  if [[ "$FAULT" == show && -e "$dropin" && ! -e "$FAULT_MARKER" ]] && grep -Fq "$FIXTURE_SHA" "$dropin"; then : >"$FAULT_MARKER"; return 1; fi
  # model systemd's normalized duration property
  if [[ "$property" == TimeoutStartUSec ]]; then
    if [[ -e "$dropin" ]]; then value=10min; else value=5min; fi
    printf '%s\\n' "$value" | tee -a "$FIXTURE_DEPLOY/timeouts.log"
    return
  fi
  case "$property" in
    ExecStart|ExecReload)
      if [[ -e "$dropin" ]]; then
        path=$(awk '/^ExecStart=.+ system-recover$/ {sub(/^ExecStart=/, ""); sub(/ system-recover$/, ""); print}' "$dropin"); argv="$path system-recover"
        # expose the predecessor cache until the pending reload completes
        if [[ "$FAULT" == pending && ! -e "$FAULT_MARKER" ]]; then path="$FIXTURE_OPERATORS/$PREVIOUS_SHA/schema-preserving-release.sh"; argv="$path system-recover"; fi
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
  if [[ "$UPGRADE" == true ]]; then
    [[ -f "$FIXTURE_DEPLOY/state/schema-preserving-release-v1/installations/$FIXTURE_SHA.json" ]] || return 91
  else
    [[ -f "$FIXTURE_DEPLOY/state/schema-preserving-release-v1/installation.json" ]] || return 91
  fi
  spi_publish_dropin_real "$@" || return 1
  [[ "$FAULT" != publish ]]
}
eval "$(declare -f spi_publish_bundle | sed '1s/spi_publish_bundle/spi_publish_bundle_real/')"
# inject an inert bundle publication failure
spi_publish_bundle() { spi_publish_bundle_real "$@" || return 1; [[ "$FAULT" != bundle ]]; }
eval "$(declare -f spi_publish_receipt | sed '1s/spi_publish_receipt/spi_publish_receipt_real/')"
# inject a receipt publication failure before authority changes
spi_publish_receipt() { spi_publish_receipt_real "$@" || return 1; [[ "$FAULT" != receipt ]]; }
# choose the explicitly requested lifecycle path
if [[ "$UPGRADE" == true ]]; then
  spi_upgrade "$3" "$6" "\${12}" "$2" "$4" "$5"
else
  spi_install "$3" "$6" "$2" "$4" "$5"
fi`;
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
    String(upgrade),
    fixture.previousSha ?? "",
  ], { encoding: "utf8" });
}

// retain an installed predecessor while preparing a different immutable bundle
async function createUpgradeFixture() {
  const fixture = await createFixture();
  const bootstrap = runInstaller(fixture);
  assert.equal(bootstrap.status, 0, bootstrap.stderr);
  fixture.previousSha = fixture.bundleSha;
  fixture.oldDropin = await readFile(join(fixture.units, "weather-compose.service.d/10-schema-preserving-release.conf"), "utf8");
  fixture.oldReceipt = await readFile(join(fixture.deploy, "state/schema-preserving-release-v1/installation.json"), "utf8");
  fixture.bundle = join(fixture.root, "repaired-bundle");
  await mkdir(fixture.bundle, { mode: 0o700 });
  const operator = "#!/usr/bin/env bash\n# repaired fixture helper\n";
  const validator = "export const helper = true;\n";
  await writeFile(join(fixture.bundle, "schema-preserving-release.sh"), operator, { mode: 0o755 });
  await writeFile(join(fixture.bundle, "schema-preserving-release.mjs"), validator, { mode: 0o644 });
  await chmod(join(fixture.bundle, "schema-preserving-release.sh"), 0o755);
  await chmod(join(fixture.bundle, "schema-preserving-release.mjs"), 0o644);
  fixture.bundleSha = hash(`${hash(validator)}  schema-preserving-release.mjs\n${hash(operator)}  schema-preserving-release.sh\n`);
  const current = "2026.11.04-3";
  await writeFile(join(fixture.deploy, "state/current-release"), `${current}\n`, { mode: 0o600 });
  await writeFile(join(fixture.deploy, `releases/${current}.env`), `WEATHER_RELEASE=${current}\n`, { mode: 0o600 });
  await rm(join(fixture.deploy, "state/active.env"));
  await symlink(`../releases/${current}.env`, join(fixture.deploy, "state/active.env"));
  return fixture;
}

// keep both boot authorities valid on their respective sides of the atomic switch
test("bundle upgrade retains the legacy receipt and publishes new authority before switching", async () => {
  const fixture = await createUpgradeFixture();
  try {
    const result = runInstaller(fixture, "", true);
    assert.equal(result.status, 0, result.stderr);
    const root = join(fixture.deploy, "state/schema-preserving-release-v1");
    assert.equal(await readFile(join(root, "installation.json"), "utf8"), fixture.oldReceipt);
    const receipt = JSON.parse(await readFile(join(root, `installations/${fixture.bundleSha}.json`), "utf8"));
    assert.equal(receipt.bundleSha256, fixture.bundleSha);
    assert.match(await readFile(join(fixture.units, "weather-compose.service.d/10-schema-preserving-release.conf"), "utf8"), new RegExp(fixture.bundleSha, "u"));
    await assert.doesNotReject(stat(join(fixture.operators, fixture.previousSha)));
    assert.equal(await readFile(join(fixture.deploy, "state/current-release"), "utf8"), "2026.11.04-3\n");
    assert.equal(await readFile(join(fixture.deploy, "state/schema-release"), "utf8"), `${release}\n`);
    const retry = runInstaller(fixture, "", true);
    assert.equal(retry.status, 0, retry.stderr);
    const pending = runInstaller(fixture, "pending", true);
    assert.equal(pending.status, 0, pending.stderr);
  } finally {
    await cleanup(fixture);
  }
});

// fail closed before switching an unexpected predecessor or unfinished release
test("bundle upgrade rejects altered predecessor, receipt, markers, schema owner, and active journal", async () => {
  // isolate each independent authority failure
  for (const failure of ["bundle", "receipt", "marker", "schema", "journal"]) {
    const fixture = await createUpgradeFixture();
    try {
      // reject an unexpected predecessor identity
      if (failure === "bundle") fixture.previousSha = "0".repeat(64);
      // reject unrelated receipt bytes
      if (failure === "receipt") await writeFile(join(fixture.deploy, "state/schema-preserving-release-v1/installation.json"), "{}\n");
      // reject changed marker ownership policy
      if (failure === "marker") await chmod(join(fixture.deploy, "state/current-release"), 0o644);
      // retain the immutable schema owner
      if (failure === "schema") await writeFile(join(fixture.deploy, "state/schema-release"), "2026.10.09-2\n");
      // never interrupt an application release transaction
      if (failure === "journal") await writeFile(join(fixture.deploy, "state/schema-preserving-release-v1/journal.json"), "{}\n");
      const result = runInstaller(fixture, "", true);
      assert.notEqual(result.status, 0, failure);
      assert.equal(await readFile(join(fixture.units, "weather-compose.service.d/10-schema-preserving-release.conf"), "utf8"), fixture.oldDropin);
    } finally {
      await cleanup(fixture);
    }
  }
});

// run the real subprocess bridge with an inherited kernel lock and controlled runtime boundaries
test("upgrade runtime checks retain the actual lock and stop after a failed prerequisite", async () => {
  // isolate lock, health and database failure cases without production paths
  for (const failure of ["", "lock", "health", "database", "source-before", "source-after"]) {
    const root = await mkdtemp(join(tmpdir(), "weather-upgrade-runtime-"));
    const state = join(root, "state");
    const other = join(root, "other");
    const snapshot = join(root, "snapshot.json");
    const environment = join(root, "current.env");
    try {
      await mkdir(state);
      await mkdir(other);
      const updater = join(root, "update.sh");
      await writeFile(updater, `state_dir='${state}'\nrequire_no_dashboard_override() { :; }\n`);
      const helper = `sp_update='${updater}'
sp_control_sha=fixture
sp_journal='${state}/journal.json'
sp_control_digest() { printf fixture; }
# expose both source validation failure shapes without changing its expected text
sp_source() { [[ '${failure}' != source-before ]]; printf '%s\\n' '${environment}'; [[ '${failure}' != source-after ]]; }
sp_health() { [[ '${failure}' != health ]]; }
sp_database() { [[ '${failure}' != database ]]; }
sp_snapshot() { printf protected >"$2"; }
sp_unchanged() { [[ "$(cat "$2")" == protected ]]; }
`;
      await writeFile(join(root, "schema-preserving-release.sh"), helper);
      const result = spawnSync("bash", ["-c", `source "$1"
exec {release_transaction_lock_fd}<"$6"
flock --exclusive --nonblock "$release_transaction_lock_fd"
spi_runtime_check "$2" "$3" "$4" snapshot
`, "runtime-lock-test", installer, root, environment, snapshot, failure, failure === "lock" ? other : state], { encoding: "utf8" });
      // only a valid held lock and successful read-only checks may create the snapshot
      if (failure === "") {
        assert.equal(result.status, 0, result.stderr);
        assert.equal(await readFile(snapshot, "utf8"), "protected");
      } else {
        assert.notEqual(result.status, 0, failure);
        await assert.rejects(readFile(snapshot));
      }
    } finally {
      await rm(root, { force: true, recursive: true });
    }
  }
});

// retain old recovery authority after every failed handoff boundary
for (const fault of ["bundle", "receipt", "publish", "reload", "verify", "show", "infrastructure"]) {
  // inject one bounded failure without touching application state
  test(`bundle upgrade restores the exact predecessor after ${fault} failure`, async () => {
    const fixture = await createUpgradeFixture();
    try {
      const result = runInstaller(fixture, fault, true);
      assert.notEqual(result.status, 0, fault);
      assert.equal(await readFile(join(fixture.units, "weather-compose.service.d/10-schema-preserving-release.conf"), "utf8"), fixture.oldDropin);
      assert.equal(await readFile(join(fixture.deploy, "state/schema-preserving-release-v1/installation.json"), "utf8"), fixture.oldReceipt);
      await assert.doesNotReject(stat(join(fixture.operators, fixture.previousSha)));
      const retry = runInstaller(fixture, "", true);
      assert.equal(retry.status, 0, retry.stderr);
    } finally {
      await cleanup(fixture);
    }
  });
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
