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
import { GIT_ADAPTER_VERSION } from "./predecessor-rust-transition-git.mjs";
import {
  OFFLINE_BOOTSTRAP_ARTIFACT_VERSION,
  OFFLINE_BOOTSTRAP_LIVE_GATE,
  OFFLINE_BOOTSTRAP_REQUEST_VERSION,
  prepareOfflineBootstrapFixture,
} from "./predecessor-rust-transition-prepare.mjs";

const repoRoot = resolve(import.meta.dirname, "..");
const installer = join(repoRoot, "scripts/install-schema-preserving-release.sh");
const controlSha = "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb";
const release = "2026.11.03-3";

// hash one fixture payload
function hash(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// build one complete disposable installation boundary
async function createFixture(extraBundleFile = false, operatorShellBytes = null) {
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
  const shellBytes = operatorShellBytes ?? "#!/usr/bin/env bash\nprintf helper\\n\n";
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

// bind every effect-relevant transition identity before the disposable controller runs
function bindPreparedFixtureTransition(artifact, input, transition, fixture) {
  assert.deepEqual(Object.keys(artifact).sort(), [
    "artifactVersion",
    "controlSha256",
    "eligibility",
    "eligibleForLiveInstallation",
    "gitEvidenceMode",
    "installationReceiptSha256",
    "liveAuthorityVerified",
    "liveGate",
    "operatorBundle",
    "predecessorSourceTreeSha256",
    "recoveryDropInSha256",
    "sourceEnvironmentSha256",
    "targetTreeSha256",
    "transition",
  ]);
  assert.equal(artifact.artifactVersion, OFFLINE_BOOTSTRAP_ARTIFACT_VERSION);
  assert.equal(artifact.eligibility, "offline-only-ineligible");
  assert.equal(artifact.eligibleForLiveInstallation, false);
  assert.equal(artifact.liveAuthorityVerified, false);
  assert.equal(artifact.liveGate, OFFLINE_BOOTSTRAP_LIVE_GATE);
  assert.equal(artifact.gitEvidenceMode, "injected-test-fixture");
  assert.deepEqual(artifact.transition.predecessor, input.predecessor);
  assert.deepEqual(artifact.transition.target, input.target);
  assert.equal(artifact.transition.contractVersion, transition.contractVersion);
  assert.equal(artifact.transition.publicationLane, transition.publicationLane);
  assert.equal(artifact.controlSha256, input.predecessor.controlSha256);
  assert.equal(
    artifact.installationReceiptSha256,
    input.predecessor.installationReceiptSha256,
  );
  assert.equal(artifact.recoveryDropInSha256, input.predecessor.recoveryDropInSha256);
  assert.equal(
    artifact.sourceEnvironmentSha256,
    input.predecessor.sourceEnvironmentSha256,
  );
  assert.equal(artifact.operatorBundle.manifestSha256, input.predecessor.bundleSha256);
  assert.deepEqual(
    artifact.operatorBundle.members.map((member) => member.path),
    ["schema-preserving-release.mjs", "schema-preserving-release.sh"],
  );
  assert.equal(input.predecessor.release, "2026.11.04-3");
  assert.equal(input.target.release, "2026.11.04-4");
  return { ...fixture, previousSha: artifact.operatorBundle.manifestSha256 };
}

// execute the installed controller's real system-recover dispatch with isolated host effects
function runFixtureSystemRecovery(
  fixture,
  bundleSha,
  receiptPath,
  receiptSha,
  dropInSha,
  logPath,
) {
  const helper = join(
    fixture.operators,
    bundleSha,
    "schema-preserving-release.sh",
  );
  const command = `source "$1"
state_dir="$2/state"
releases_dir="$2/releases"
deploy_dir="$2"
compose_file="$2/compose.yaml"
sp_journal="$state_dir/schema-preserving-release-v1/journal.json"
FIXTURE_OPERATORS="$3"
FIXTURE_UNITS="$4"
EXPECTED_BUNDLE="$5"
EXPECTED_RECEIPT="$6"
EXPECTED_RECEIPT_SHA="$7"
EXPECTED_DROPIN_SHA="$8"
RECOVERY_LOG="$9"
POSTGRES_ID=$(printf '%064d' 1)
CLOUDFLARED_ID=$(printf '%064d' 5)
: >"$RECOVERY_LOG"
# preserve the installed controller identity while replacing privileged host admission
sp_load() {
  local manifest= file mode=644 dropin="$FIXTURE_UNITS/weather-compose.service.d/10-schema-preserving-release.conf"
  [[ "$sp_bundle" == "$FIXTURE_OPERATORS/$EXPECTED_BUNDLE" ]]
  # bind both exact installed helper files
  for file in schema-preserving-release.mjs schema-preserving-release.sh; do
    mode=644
    [[ "$file" != *.sh ]] || mode=755
    [[ -f "$sp_bundle/$file" && ! -L "$sp_bundle/$file" && "$(stat -c '%a:%h' "$sp_bundle/$file")" == "$mode:1" ]]
    manifest+="$(sha256sum -- "$sp_bundle/$file" | awk '{print $1}')  $file"$'\\n'
  done
  [[ "$(printf '%s' "$manifest" | sha256sum | awk '{print $1}')" == "$EXPECTED_BUNDLE" ]]
  [[ "$(sha256sum -- "$EXPECTED_RECEIPT" | awk '{print $1}')" == "$EXPECTED_RECEIPT_SHA" ]]
  [[ "$(sha256sum -- "$dropin" | awk '{print $1}')" == "$EXPECTED_DROPIN_SHA" ]]
  grep -Fxq "ExecStart=$sp_bundle/schema-preserving-release.sh system-recover" "$dropin"
  printf 'load:%s\\n' "$EXPECTED_BUNDLE" >>"$RECOVERY_LOG"
}
# read one fixture marker without the production updater
read_release_state() { cat "$1"; }
# retain the ordinary core-only recovery fixture
dashboard_override_present() { return 1; }
# select only the fixture's committed application environment
sp_environment() {
  [[ "$1" == 2026.11.04-3 ]]
  printf '%s/2026.11.04-3.env\\n' "$releases_dir"
}
# preserve the committed-source equality check
sp_source() { sp_environment "$(read_release_state "$state_dir/current-release")"; }
# record the retained database check boundary
sp_database() { printf 'database:%s\\n' "$1" >>"$RECOVERY_LOG"; }
# record the application switch boundary without creating containers
sp_switch() { printf 'switch:%s\\n' "$1" >>"$RECOVERY_LOG"; }
# expose only the two retained infrastructure image references
env_value() {
  case "$2" in
    POSTGRES_IMAGE) printf 'postgres@sha256:%064d\\n' 3 ;;
    CLOUDFLARED_IMAGE) printf 'cloudflared@sha256:%064d\\n' 4 ;;
    WEATHER_RELEASE) printf '2026.11.04-3\\n' ;;
    *) return 1 ;;
  esac
}
# resolve only existing retained container identities
compose() {
  local service="\${!#}"
  [[ " $* " == *' ps '* ]]
  case "$service" in
    postgres) printf '%s\\n' "$POSTGRES_ID" ;;
    cloudflared) printf '%s\\n' "$CLOUDFLARED_ID" ;;
    *) return 1 ;;
  esac
}
# isolate image identity, health, and retained start operations
docker() {
  local image expected_id service container="\${4:-}"
  # return one exact image identity
  if [[ "$1:$2" == image:inspect ]]; then
    image="$5"
    case "$image" in
      postgres@*) printf 'sha256:%064d\\n' 3 ;;
      cloudflared@*) printf 'sha256:%064d\\n' 4 ;;
      *) return 1 ;;
    esac
    return
  fi
  # return one existing container identity or health state
  if [[ "$1" == inspect && "$2" == --format ]]; then
    # bind the complete project and service descriptor
    if [[ "$3" == *'.Config.Image'* ]]; then
      if [[ "$container" == "$POSTGRES_ID" ]]; then image="postgres@sha256:$(printf '%064d' 3)"; expected_id="sha256:$(printf '%064d' 3)"; service=postgres;
      elif [[ "$container" == "$CLOUDFLARED_ID" ]]; then image="cloudflared@sha256:$(printf '%064d' 4)"; expected_id="sha256:$(printf '%064d' 4)"; service=cloudflared;
      else return 1; fi
      printf '%s|%s|%s|weather|%s\\n' "$container" "$image" "$expected_id" "$service"
    else
      [[ "$container" == "$POSTGRES_ID" || "$container" == "$CLOUDFLARED_ID" ]]
      printf 'health:%s\\n' "$container" >>"$RECOVERY_LOG"
      printf 'true|healthy\\n'
    fi
    return
  fi
  # admit starts only for the two already identified containers
  if [[ "$1" == start && ( "$2" == "$POSTGRES_ID" || "$2" == "$CLOUDFLARED_ID" ) ]]; then
    printf 'start:%s\\n' "$2" >>"$RECOVERY_LOG"
    return
  fi
  printf 'forbidden-docker:%s\\n' "$*" >>"$RECOVERY_LOG"
  return 1
}
# reject unplanned host controller effects
systemctl() { printf 'forbidden-systemctl:%s\\n' "$*" >>"$RECOVERY_LOG"; return 1; }
# reject privilege changes inside the fixture
sudo() { printf 'forbidden-sudo:%s\\n' "$*" >>"$RECOVERY_LOG"; return 1; }
# fail if the real health loop ever needs to wait
sleep() { return 92; }
sp_main system-recover`;
  return spawnSync("bash", [
    "-c",
    command,
    "schema-preserving-recovery-test",
    helper,
    fixture.deploy,
    fixture.operators,
    fixture.units,
    bundleSha,
    receiptPath,
    receiptSha,
    dropInSha,
    logPath,
  ], { encoding: "utf8" });
}

// describe the only retained-host effects allowed from either recovery authority
function recoveryEffects(bundleSha, deploy) {
  return [
    `load:${bundleSha}`,
    `start:${"1".padStart(64, "0")}`,
    `start:${"5".padStart(64, "0")}`,
    `health:${"1".padStart(64, "0")}`,
    `health:${"5".padStart(64, "0")}`,
    `database:${join(deploy, "releases/2026.11.04-3.env")}`,
    `switch:${join(deploy, "releases/2026.11.04-3.env")}`,
    `health:${"1".padStart(64, "0")}`,
    `health:${"5".padStart(64, "0")}`,
  ];
}

// retain an installed predecessor while preparing a different immutable bundle
async function createUpgradeFixture(validatorOnly = false, realOperator = false) {
  const originalOperator = realOperator
    ? await readFile(join(repoRoot, "scripts/schema-preserving-release.sh"), "utf8")
    : null;
  const fixture = await createFixture(false, originalOperator);
  const bootstrap = runInstaller(fixture);
  assert.equal(bootstrap.status, 0, bootstrap.stderr);
  fixture.previousSha = fixture.bundleSha;
  fixture.oldDropin = await readFile(join(fixture.units, "weather-compose.service.d/10-schema-preserving-release.conf"), "utf8");
  fixture.oldReceipt = await readFile(join(fixture.deploy, "state/schema-preserving-release-v1/installation.json"), "utf8");
  fixture.bundle = join(fixture.root, "repaired-bundle");
  await mkdir(fixture.bundle, { mode: 0o700 });
  const operator = realOperator
    ? `${await readFile(join(fixture.operators, fixture.previousSha, "schema-preserving-release.sh"), "utf8")}\n# distinct fixture target bundle\n`
    : validatorOnly
    ? await readFile(join(fixture.operators, fixture.previousSha, "schema-preserving-release.sh"), "utf8")
    : "#!/usr/bin/env bash\n# repaired fixture helper\n";
  const validator = validatorOnly ? "export const helper = 'reviewed-query';\n" : "export const helper = true;\n";
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

// join preparation to the original installer using the same predecessor bytes
test("prepared predecessor identity restores the exact installed authority after reload fails", async () => {
  const fixture = await createUpgradeFixture(false, true);
  try {
    const transition = JSON.parse(await readFile(
      join(repoRoot, "scripts/predecessor-rust-transition.fixture.json"), "utf8",
    ));
    const installed = join(fixture.operators, fixture.previousSha);
    const bundleFiles = await Promise.all(["mjs", "sh"].map(
      // preserve the actual installed predecessor bytes as repository-named inputs
      async (extension) => ({
        bytes: await readFile(join(installed, `schema-preserving-release.${extension}`)),
        path: `scripts/schema-preserving-release.${extension}`,
      }),
    ));
    const controlBytes = await readFile(join(fixture.deploy, "scripts/update.sh"));
    const sourceEnvironmentBytes = await readFile(join(fixture.deploy, "state/active.env"));
    const installationReceiptBytes = Buffer.from(fixture.oldReceipt);
    const recoveryDropInBytes = Buffer.from(fixture.oldDropin);
    const preparationInput = {
      bundleFiles,
      controlBytes,
      installationReceiptBytes,
      predecessor: {
        ...transition.predecessor,
        bundleSha256: JSON.parse(fixture.oldReceipt).bundleSha256,
        controlSha256: hash(controlBytes),
        installationReceiptSha256: hash(installationReceiptBytes),
        recoveryDropInSha256: hash(recoveryDropInBytes),
        release: "2026.11.04-3",
        sourceEnvironmentSha256: hash(sourceEnvironmentBytes),
      },
      recoveryDropInBytes,
      requestVersion: OFFLINE_BOOTSTRAP_REQUEST_VERSION,
      sourceEnvironmentBytes,
      target: { ...transition.target, release: "2026.11.04-4" },
    };
    const transitionEvidence = {
      ...transition,
      adapterVersion: GIT_ADAPTER_VERSION,
      evidenceMode: "injected-test-fixture",
      predecessorCommit: transition.predecessor.commit,
      targetCommit: transition.target.commit,
    };
    const artifact = prepareOfflineBootstrapFixture(preparationInput, transitionEvidence);
    const prepared = bindPreparedFixtureTransition(
      artifact,
      preparationInput,
      transition,
      fixture,
    );
    const failed = runInstaller(prepared, "reload", true);
    assert.notEqual(failed.status, 0);
    const dropIn = join(fixture.units, "weather-compose.service.d/10-schema-preserving-release.conf");
    const receipts = join(fixture.deploy, "state/schema-preserving-release-v1");
    assert.equal(await readFile(dropIn, "utf8"), fixture.oldDropin);
    const predecessorReceipt = join(receipts, "installation.json");
    assert.equal(await readFile(predecessorReceipt, "utf8"), fixture.oldReceipt);
    await assert.doesNotReject(stat(join(receipts, `installations/${fixture.bundleSha}.json`)));

    const predecessorRecoveryLog = join(fixture.root, "predecessor-recovery.log");
    const recoveredPredecessor = runFixtureSystemRecovery(
      prepared,
      fixture.previousSha,
      predecessorReceipt,
      hash(fixture.oldReceipt),
      hash(fixture.oldDropin),
      predecessorRecoveryLog,
    );
    assert.equal(recoveredPredecessor.status, 0, recoveredPredecessor.stderr);
    assert.deepEqual(
      (await readFile(predecessorRecoveryLog, "utf8")).trim().split("\n"),
      recoveryEffects(fixture.previousSha, fixture.deploy),
    );
    assert.equal(await readFile(dropIn, "utf8"), fixture.oldDropin);
    assert.equal(await readFile(predecessorReceipt, "utf8"), fixture.oldReceipt);

    const retry = runInstaller(prepared, "", true);
    assert.equal(retry.status, 0, retry.stderr);
    const targetReceipt = join(receipts, `installations/${fixture.bundleSha}.json`);
    const targetReceiptBytes = await readFile(targetReceipt);
    const targetDropInBytes = await readFile(dropIn);
    const targetRecoveryLog = join(fixture.root, "target-recovery.log");
    const recoveredTarget = runFixtureSystemRecovery(
      prepared,
      fixture.bundleSha,
      targetReceipt,
      hash(targetReceiptBytes),
      hash(targetDropInBytes),
      targetRecoveryLog,
    );
    assert.equal(recoveredTarget.status, 0, recoveredTarget.stderr);
    assert.deepEqual(
      (await readFile(targetRecoveryLog, "utf8")).trim().split("\n"),
      recoveryEffects(fixture.bundleSha, fixture.deploy),
    );
    assert.doesNotMatch(await readFile(targetRecoveryLog, "utf8"), /create|publish|stage/u);
    assert.deepEqual(await readFile(dropIn), targetDropInBytes);
    assert.deepEqual(await readFile(targetReceipt), targetReceiptBytes);
  } finally {
    await cleanup(fixture);
  }
});

// validator-only repairs still require a distinct bundle and retain rollback authority
test("validator-only upgrade rotates authority without changing the operator shell", async () => {
  const fixture = await createUpgradeFixture(true);
  try {
    assert.notEqual(fixture.bundleSha, fixture.previousSha);
    const result = runInstaller(fixture, "", true);
    assert.equal(result.status, 0, result.stderr);
    const previous = join(fixture.operators, fixture.previousSha);
    const current = join(fixture.operators, fixture.bundleSha);
    assert.equal(await readFile(join(current, "schema-preserving-release.sh"), "utf8"), await readFile(join(previous, "schema-preserving-release.sh"), "utf8"));
    assert.notEqual(await readFile(join(current, "schema-preserving-release.mjs"), "utf8"), await readFile(join(previous, "schema-preserving-release.mjs"), "utf8"));
    assert.equal(await readFile(join(fixture.deploy, "state/schema-preserving-release-v1/installation.json"), "utf8"), fixture.oldReceipt);
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
