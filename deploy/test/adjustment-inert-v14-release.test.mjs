import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmod, link, mkdir, mkdtemp, readFile, readdir, rename, rm, stat, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import {
  bootstrapAdjustmentFamilyReleaseCurrent,
  readAdjustmentInertV14SettingsSnapshot,
  selectAdjustmentInertV14RestorationSchema,
  verifyAdjustmentInertV14BootstrapCurrent,
} from "../scripts/adjustment-evaluation-package.mjs";

const update = resolve(import.meta.dirname, "../scripts/update.sh");
const sourceValues = {
  WEATHER_RELEASE: "2026.10.09-1",
  WEATHER_CONTROL_PLANE_VERSION: "13",
  WEATHER_CONTROL_PLANE_SHA256: "603eb8f488ba78be3d7ecf76b0d587346d1c36432255d390d86d2b768c8fecba",
  WEATHER_SERVER_IMAGE: "ghcr.io/anstosa/weather-server@sha256:fb140b46d6eaea463ba2d10dc74303eac515135a37c21ddb746cdce744fd23ab",
  WEATHER_WEB_IMAGE: "ghcr.io/anstosa/weather-web@sha256:fdcb2d10da4c9ed5ec8651bafa96c9d2b240b66b85db85619b909d6e144e2d7b",
};

// write only the exact literal identity fields exercised by the source gate
async function writeEnvironment(path, values) {
  await writeFile(path, Object.entries(values).map(
    // retain literal environment keys and values without shell evaluation
    ([key, value]) => `${key}=${value}\n`,
  ).join(""), { mode: 0o600 });
}

// run one isolated bridge transaction with deterministic failure injection
async function runCompatibilityBridge(failure = "none") {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-bridge-transaction-"));
  const state = join(root, "state");
  const releases = join(root, "releases");
  const transcript = join(root, "transcript");
  await mkdir(state);
  await mkdir(releases);
  await writeFile(join(state, "current-release"), "2026.10.09-1\n", { mode: 0o600 });
  await writeFile(join(state, "schema-release"), "2026.10.09-1\n", { mode: 0o600 });
  await writeFile(join(state, "previous-release"), "2026.10.08-1\n", { mode: 0o600 });
  await symlink("../releases/2026.10.09-1.env", join(state, "active.env"));
  await writeFile(join(releases, "2026.10.09-1.env"), "WEATHER_DATABASE_NAME=weather\n");
  await writeFile(join(releases, "2026.10.09-2.env"), "WEATHER_DATABASE_NAME=weather\n");
  const script = `source "$1"
state_dir=$2/state
releases_dir=$2/releases
v14_compatibility_bridge_state=$state_dir/v14-compatibility-bridge-v1
transcript=$3
failure=$4
pair_calls=0
ledger_calls=0
image_calls=0
resource_calls=0
capacity_calls=0
# record one deterministic injected boundary
checkpoint() {
  printf '%s\\n' "$1" >>"$transcript"
  [[ "$failure" != "$1" && !( "$failure" == recover && ( "$1" == start || "$1" == recover ) ) ]]
}
# resolve only the two fixed fixture environments
release_env() { printf '%s/%s.env\\n' "$releases_dir" "$1"; }
validate_release_env() { :; }
require_control_plane_compatibility() { :; }
require_deployment_secrets() { :; }
# emit fixed CI and settings results without external access
node() {
  if [[ "$*" == *verify-v14-compatibility-bridge-release* ]]; then
    printf '%s\\n' '{"targetCommit":"f6daa89d135661eb60dd5479c5d0d6c23e7198a9"}'
  elif [[ "$*" == --input-type* ]]; then
    printf '%s\\n' f6daa89d135661eb60dd5479c5d0d6c23e7198a9
  else
    printf '%s\\n' '1:2 ${"a".repeat(64)}'
  fi
}
# return the already rendered bridge environment
prepare_v14_compatibility_bridge_release() { checkpoint prepare >&2; printf '%s/%s.env\\n' "$releases_dir" "$1"; }
# fail only the second pair validation
require_v14_compatibility_bridge_release_pair() {
  pair_calls=$((pair_calls + 1))
  [[ "$pair_calls" != 2 ]] || checkpoint pair-post
}
# fail only the second exact-ledger validation
require_v14_compatibility_bridge_migration_ledger() {
  ledger_calls=$((ledger_calls + 1))
  [[ "$ledger_calls" != 2 ]] || checkpoint ledger-post
}
# return one source hash and optionally drift after target start
migration_history_sha256() {
  if [[ "$1" == *2026.10.09-2.env && "$failure" == history-post ]]; then
    printf '%064d\\n' 2
  else
    printf '%064d\\n' 1
  fi
}
# meter both pre- and post-start gates
require_image_pull_capacity_floor() { image_calls=$((image_calls + 1)); [[ "$image_calls" != 2 ]] || checkpoint image-post; }
require_normal_release_resource_gate() { resource_calls=$((resource_calls + 1)); [[ "$resource_calls" != 2 ]] || checkpoint resource-post; }
require_literal_inert_v14_release_capacity() { capacity_calls=$((capacity_calls + 1)); [[ "$capacity_calls" != 2 ]] || checkpoint capacity-post; }
# persist only a visible deterministic intent marker
publish_v14_compatibility_bridge_state() { checkpoint publish; printf 'intent\\n' >"$v14_compatibility_bridge_state"; }
start_v14_compatibility_bridge_runtime() { checkpoint start; }
# publish one marker with its injected boundary
write_private_state() {
  local name=\${1##*/}
  checkpoint "$name"
  printf '%s\\n' "$2" >"$1"
  chmod 600 "$1"
}
# publish the fixture active link with its injected boundary
write_active_symlink() {
  checkpoint active
  rm -f "$state_dir/active.env"
  ln -s "../releases/$1.env" "$state_dir/active.env"
}
# restore exact source state for every post-intent failure
recover_v14_compatibility_bridge() {
  checkpoint recover || return 1
  printf '2026.10.08-1\\n' >"$state_dir/previous-release"
  printf '2026.10.09-1\\n' >"$state_dir/schema-release"
  rm -f "$state_dir/active.env"
  ln -s '../releases/2026.10.09-1.env' "$state_dir/active.env"
  printf '2026.10.09-1\\n' >"$state_dir/current-release"
  rm -f "$v14_compatibility_bridge_state"
}
v14_compatibility_bridge_release 2026.10.09-2`;
  const result = spawnSync("bash", ["-c", script, "bridge-test", update, root, transcript, failure], {
    encoding: "utf8",
  });
  return { result, root, state, transcript };
}

// run the real generic recovery classifier with inert host-operation ports
async function runGenericRecovery(current, schema, receipt = true) {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-generic-recovery-"));
  const state = join(root, "state");
  const releases = join(root, "releases");
  const transcript = join(root, "transcript");
  await mkdir(state);
  await mkdir(releases);
  await writeFile(join(state, "current-release"), `${current}\n`, { mode: 0o600 });
  await writeFile(join(state, "schema-release"), `${schema}\n`, { mode: 0o600 });
  // retain authority only in scenarios that claim a bridge or full release
  if (receipt) {
    await writeFile(join(state, "v14-compatibility-bridge-v1"), "receipt\n", { mode: 0o600 });
  }
  const script = `source "$1"
state_dir=$2/state
releases_dir=$2/releases
v14_compatibility_bridge_state=$state_dir/v14-compatibility-bridge-v1
transcript=$3
acquire_release_transaction_lock() { :; }
require_command() { :; }
require_v14_compatibility_bridge_receipt() { printf 'bridge-receipt\\n' >>"$transcript"; }
require_committed_v14_compatibility_bridge() { printf 'committed-bridge\\n' >>"$transcript"; }
require_v14_source2_full_schema_authority() { printf 'bridge-receipt\\n' >>"$transcript"; }
require_fixed_v14_migration_ledger() { :; }
require_full_v14_current_authority() { :; }
release_env() { printf '%s/%s.env\\n' "$releases_dir" "$1"; }
validate_release_env() { :; }
require_control_plane_compatibility() { :; }
require_deployment_secrets() { :; }
restore_images() { printf 'restore:%s:%s\\n' "$2" "$3" >>"$transcript"; }
restore_v14_compatibility_bridge_images() { printf 'restore:%s:%s\\n' "$2" "$3" >>"$transcript"; }
write_active_symlink() { printf 'active:%s\\n' "$1" >>"$transcript"; }
main recover`;
  const result = spawnSync("bash", ["-c", script, "v14-generic-recovery", update, root,
    transcript], { encoding: "utf8" });
  return { result, root, transcript };
}

// the separate inactive handoff cannot reuse either another source or the family scope
test("compatibility bridge and full source gates remain disjoint", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-inert-v14-source-"));
  const source = join(root, "source.env");
  const target = join(root, "target.env");
  // invoke the real closed Bash identity gate without Docker or host mutation
  const verify = () => spawnSync("bash", ["-c",
    'source "$1"; require_v14_compatibility_bridge_source_identity "$2" "$3"',
    "v14-source-test", update, target, source], { encoding: "utf8" });
  try {
    await writeEnvironment(source, sourceValues);
    await writeEnvironment(target, { WEATHER_RELEASE: "2026.10.09-2", WEATHER_CONTROL_PLANE_VERSION: "15" });
    assert.equal(verify().status, 0);
    // every independently pinned predecessor field rejects drift
    for (const [key, value] of Object.entries(sourceValues)) {
      await writeEnvironment(source, { ...sourceValues, [key]: `${value}x` });
      assert.notEqual(verify().status, 0, key);
    }
    await writeEnvironment(source, sourceValues);
    await writeEnvironment(target, { WEATHER_RELEASE: "2026.10.09-1", WEATHER_CONTROL_PLANE_VERSION: "15" });
    assert.notEqual(verify().status, 0);
    await writeEnvironment(target, { WEATHER_RELEASE: "2026.10.09-2", WEATHER_CONTROL_PLANE_VERSION: "13" });
    assert.notEqual(verify().status, 0);
    const bridgeServer = `ghcr.io/anstosa/weather-server@sha256:${"1".repeat(64)}`;
    const bridgeWeb = `ghcr.io/anstosa/weather-web@sha256:${"2".repeat(64)}`;
    const control = "3".repeat(64);
    await writeEnvironment(source, {
      WEATHER_CONTROL_PLANE_SHA256: control,
      WEATHER_CONTROL_PLANE_VERSION: "15",
      WEATHER_RELEASE: "2026.10.09-2",
      WEATHER_SERVER_IMAGE: bridgeServer,
      WEATHER_WEB_IMAGE: bridgeWeb,
    });
    await writeEnvironment(target, { WEATHER_RELEASE: "2026.10.09-3", WEATHER_CONTROL_PLANE_VERSION: "15" });
    const full = spawnSync("bash", ["-c", `source "$1"
full_v14_source_server_image=$4
full_v14_source_web_image=$5
expected_control=$6
# return the isolated installed control identity
control_plane_digest() { printf '%s\\n' "$expected_control"; }
require_fixed_v14_source_identity "$2" "$3"`,
    "v14-full-source-test", update, target, source, bridgeServer, bridgeWeb, control], { encoding: "utf8" });
    assert.equal(full.status, 0, full.stderr);
    const bytes = await readFile(update, "utf8");
    assert.match(bytes, /v14-compatibility-bridge\)/u);
    assert.match(bytes, /inert-v14\)/u);
    assert.match(bytes, /verify_fixed_v14_source_compatibility/u);
    assert.match(bytes, /require_literal_inert_v14_release_capacity/u);
    assert.match(bytes, /discard-failed-inert-v14-current/u);
    assert.match(bytes, /bootstrap-inert-v14-current/u);
    const compatibility = bytes.split("v14_compatibility_bridge_release() (")[1]
      .split("# prepare one direct release without clone-based staging")[0];
    assert.doesNotMatch(compatibility, /compose run --rm migration|apply_runtime_database_acl|revision-capture-epoch|bootstrap-inert-v14-current|require_inert_v14_empty_catalog/u);
    assert.match(compatibility, /require_v14_compatibility_bridge_migration_ledger/u);
    const fullRelease = bytes.split("inert_v14_release() (")[1].split("# activate one forward release")[0];
    assert.ok(fullRelease.indexOf("require_committed_v14_compatibility_bridge") <
      fullRelease.indexOf("verify-full-v14-release"));
    assert.ok(fullRelease.indexOf("--revision-capture-epoch-init-v1") < fullRelease.indexOf('start_exact_release "$target"'));
    assert.ok(fullRelease.indexOf("--revision-capture-epoch-init-v1") > fullRelease.indexOf('verify_runtime_database_acl "$target"'));
    assert.match(fullRelease, /migration \\\n\s+node deploy\/scripts\/migrate\.mjs --atomic-maintenance-v14/u);
    assert.ok(fullRelease.indexOf("inert-v14-settings-snapshot") < fullRelease.indexOf('start_exact_release "$target"'));
    assert.ok(fullRelease.indexOf("verify-inert-v14-settings-snapshot") > fullRelease.indexOf('start_exact_release "$target"'));
    assert.ok(fullRelease.indexOf("bootstrap-inert-v14-current") <
      fullRelease.indexOf('record_release_success "$release" "$current"'));
    const authorizationPublication = bytes.split("publish_migration_authorization() {")[1]
      .split("# hash one exact ordered migration ledger")[0];
    const authorizationLink = authorizationPublication.indexOf('ln "$source" "$target"');
    assert.ok(authorizationPublication.indexOf('sync -f "$source"') < authorizationLink);
    assert.ok(authorizationPublication.indexOf('sync -f "$releases_dir"', authorizationLink) >
      authorizationLink);

  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// the full handoff cannot begin without the durable committed bridge authority
test("full v14 refuses before the compatibility bridge is committed", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-full-prebridge-"));
  const result = spawnSync("bash", ["-c", `source "$1"
state_dir=$2
v14_compatibility_bridge_state=$state_dir/v14-compatibility-bridge-v1
require_committed_v14_compatibility_bridge`, "v14-full-prebridge", update, root], {
    encoding: "utf8",
  });
  try {
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /required file not found/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// bridge health strips only the unavailable future-authority mounts
test("compatibility bridge uses no catalog or epoch bind mount", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-bridge-compose-"));
  const state = join(root, "state");
  const environment = join(root, "bridge.env");
  const transcript = join(root, "transcript");
  await mkdir(state);
  await writeEnvironment(environment, {
    CLOUDFLARED_IMAGE: `cloudflare/cloudflared@sha256:${"1".repeat(64)}`,
    POSTGRES_IMAGE: `postgres@sha256:${"2".repeat(64)}`,
    WEATHER_CONTROL_PLANE_SHA256: "3".repeat(64),
    WEATHER_CONTROL_PLANE_VERSION: "15",
    WEATHER_DATABASE_NAME: "weather",
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH: "1",
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH: "0",
    WEATHER_POSTGRES_DIR: "/var/lib/weather/postgres",
    WEATHER_RELEASE: "2026.10.09-2",
    WEATHER_SERVER_IMAGE: `ghcr.io/anstosa/weather-server@sha256:${"4".repeat(64)}`,
    WEATHER_WEB_IMAGE: `ghcr.io/anstosa/weather-web@sha256:${"5".repeat(64)}`,
  });
  const script = `source "$1"
state_dir=$2
environment=$3
transcript=$4
docker() {
  # execute only the read-only merged-config proof
  if [[ "$*" == *" config --format json"* ]]; then
    command docker "$@"
    return
  fi
  printf '%s\\n' "$*" >"$transcript"
}
start_v14_compatibility_bridge_runtime "$environment"`;
  const result = spawnSync("bash", ["-c", script, "v14-bridge-compose", update,
    state, environment, transcript], { encoding: "utf8" });
  try {
    assert.equal(result.status, 0, result.stderr);
    assert.match(await readFile(transcript, "utf8"),
      /up -d --no-deps --remove-orphans --wait api worker web cloudflared/u);
    assert.deepEqual(await readdir(state), []);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// bridge receipt publication reconciles both fixed crash windows without authority loss
test("compatibility bridge receipt survives pending and linked crash states", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-bridge-receipt-"));
  const source = join(root, "source.env");
  const target = join(root, "target.env");
  const final = join(root, "v14-compatibility-bridge-v1");
  const pending = join(root, ".v14-compatibility-bridge-v1.pending");
  await writeFile(source, "source\n", { mode: 0o600 });
  await writeFile(target, "target\n", { mode: 0o600 });
  const script = `source "$1"
state_dir=$2
v14_compatibility_bridge_state=$state_dir/v14-compatibility-bridge-v1
v14_compatibility_bridge_pending=$state_dir/.v14-compatibility-bridge-v1.pending
control_plane_digest() { printf '%064d\\n' 7; }
publish_v14_compatibility_bridge_state "$3" "$4" "$(printf '%064d' 1)" \
  '1:2' "$(printf '%064d' 2)" absent
# simulate a kill after the final directory entry is durable but before pending unlink
ln "$v14_compatibility_bridge_state" "$v14_compatibility_bridge_pending"
require_v14_compatibility_bridge_state
# simulate a kill after durable pending rename but before final link
mv "$v14_compatibility_bridge_state" "$v14_compatibility_bridge_pending"
publish_v14_compatibility_bridge_state "$3" "$4" "$(printf '%064d' 1)" \
  '1:2' "$(printf '%064d' 2)" absent`;
  const result = spawnSync("bash", ["-c", script, "v14-bridge-receipt", update,
    root, source, target], { encoding: "utf8" });
  try {
    assert.equal(result.status, 0, result.stderr);
    assert.equal((await stat(final)).nlink, 1);
    await assert.rejects(stat(pending), /ENOENT/u);
    const updateBytes = await readFile(update, "utf8");
    const publication = updateBytes.split("publish_v14_compatibility_bridge_state() {")[1]
      .split("# write the fixed pre-epoch compose override")[0];
    const finalLink = publication.lastIndexOf('ln "$v14_compatibility_bridge_pending"');
    assert.ok(publication.indexOf('sync -f "$temporary"') < finalLink);
    assert.ok(publication.indexOf('sync -f "$state_dir"', finalLink) > finalLink);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// bridge success commits current last without any schema or model operation
test("compatibility bridge commits only after post-start invariants pass", async () => {
  const scenario = await runCompatibilityBridge();
  try {
    assert.equal(scenario.result.status, 0, scenario.result.stderr);
    assert.equal(await readFile(join(scenario.state, "previous-release"), "utf8"), "2026.10.09-1\n");
    assert.equal(await readFile(join(scenario.state, "schema-release"), "utf8"), "2026.10.09-2\n");
    assert.equal(await readFile(join(scenario.state, "current-release"), "utf8"), "2026.10.09-2\n");
    const transcript = await readFile(scenario.transcript, "utf8");
    assert.match(transcript, /start[\s\S]*pair-post[\s\S]*ledger-post[\s\S]*image-post[\s\S]*resource-post[\s\S]*capacity-post/u);
    assert.match(transcript, /previous-release[\s\S]*schema-release[\s\S]*active[\s\S]*current-release\n$/u);
    assert.doesNotMatch(transcript, /recover/u);
  } finally {
    await rm(scenario.root, { force: true, recursive: true });
  }
});

// every post-intent boundary restores exact source markers and images
test("compatibility bridge compensates every precommit mutation failure", async () => {
  const failures = [
    "start", "pair-post", "ledger-post", "history-post", "image-post", "resource-post",
    "capacity-post", "previous-release", "schema-release", "active", "current-release",
  ];
  // exercise each independently injected boundary
  for (const failure of failures) {
    const scenario = await runCompatibilityBridge(failure);
    try {
      assert.notEqual(scenario.result.status, 0, failure);
      assert.match(await readFile(scenario.transcript, "utf8"), /recover/u, failure);
      assert.equal(await readFile(join(scenario.state, "previous-release"), "utf8"), "2026.10.08-1\n", failure);
      assert.equal(await readFile(join(scenario.state, "schema-release"), "utf8"), "2026.10.09-1\n", failure);
      assert.equal(await readFile(join(scenario.state, "current-release"), "utf8"), "2026.10.09-1\n", failure);
    } finally {
      await rm(scenario.root, { force: true, recursive: true });
    }
  }
});

// a failed source restoration remains a hard transaction failure
test("compatibility bridge reports recovery failure", async () => {
  const scenario = await runCompatibilityBridge("recover");
  try {
    assert.notEqual(scenario.result.status, 0);
    assert.match(scenario.result.stderr, /restoration both failed/u);
  } finally {
    await rm(scenario.root, { force: true, recursive: true });
  }
});

// refuse to reinterpret an advanced full-release tuple as a source-one bridge retry
test("compatibility bridge never rewinds advanced release markers", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-advanced-bridge-"));
  const state = join(root, "state");
  const transcript = join(root, "transcript");
  await mkdir(state);
  await writeFile(join(state, "v14-compatibility-bridge-v1"), "retained\n", { mode: 0o600 });
  const script = `source "$1"
state_dir=$2
v14_compatibility_bridge_state=$state_dir/v14-compatibility-bridge-v1
transcript=$3
recover_v14_compatibility_bridge() { printf 'source-one-recovery\\n' >>"$transcript"; }
require_committed_v14_compatibility_bridge() { printf 'committed\\n' >>"$transcript"; }
v14_compatibility_bridge_release 2026.10.09-2`;
  try {
    // reject both a compensated full schema and a committed full release
    for (const [current, schema] of [
      ["2026.10.09-2", "2026.10.09-3"],
      ["2026.10.09-3", "2026.10.09-3"],
    ]) {
      await writeFile(join(state, "current-release"), `${current}\n`, { mode: 0o600 });
      await writeFile(join(state, "schema-release"), `${schema}\n`, { mode: 0o600 });
      await rm(transcript, { force: true });
      const result = spawnSync("bash", ["-c", script, "advanced-bridge", update, state,
        transcript], { encoding: "utf8" });
      assert.notEqual(result.status, 0, `${current}:${schema}`);
      assert.match(result.stderr, /cannot rewind an advanced release state/u);
      await assert.rejects(readFile(transcript, "utf8"), /ENOENT/u);
    }
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// admit only pristine schema 18 or an authenticated compensated schema 21 retry
test("full v14 source authority distinguishes pristine and compensated bridge states", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-full-source-authority-"));
  const state = join(root, "state");
  const transcript = join(root, "transcript");
  await mkdir(state);
  const script = `source "$1"
state_dir=$2
transcript=$3
require_v14_bridge_active_release() { :; }
require_committed_v14_compatibility_bridge() { printf 'schema18\\n' >>"$transcript"; }
require_v14_source2_full_schema_authority() { printf 'schema21\\n' >>"$transcript"; }
require_v14_full_source_authority /tmp/source.env`;
  try {
    await writeFile(join(state, "current-release"), "2026.10.09-2\n", { mode: 0o600 });
    // route each exact schema marker to its disjoint authority
    for (const [schema, expected] of [
      ["2026.10.09-2", "schema18\n"],
      ["2026.10.09-3", "schema21\n"],
    ]) {
      await writeFile(join(state, "schema-release"), `${schema}\n`, { mode: 0o600 });
      await rm(transcript, { force: true });
      const result = spawnSync("bash", ["-c", script, "full-source-authority", update, state,
        transcript], { encoding: "utf8" });
      assert.equal(result.status, 0, result.stderr);
      assert.equal(await readFile(transcript, "utf8"), expected);
    }
    await writeFile(join(state, "current-release"), "2026.10.09-3\n", { mode: 0o600 });
    const rejected = spawnSync("bash", ["-c", script, "full-source-authority", update, state,
      transcript], { encoding: "utf8" });
    assert.notEqual(rejected.status, 0);
    assert.match(rejected.stderr, /requires the exact compatibility bridge runtime/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// preserve source-two schema authorization across a failed and retried full handoff
test("full v14 compensated retry restores schema 21 before succeeding", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-compensated-retry-"));
  const state = join(root, "state");
  const releases = join(root, "releases");
  const scripts = join(root, "deploy", "scripts");
  const transcript = join(root, "transcript");
  const source = join(releases, "2026.10.09-2.env");
  const target = join(releases, "2026.10.09-3.env");
  const history = "6".repeat(64);
  const environment = {
    CLOUDFLARED_IMAGE: `cloudflare/cloudflared@sha256:${"1".repeat(64)}`,
    POSTGRES_IMAGE: `postgres@sha256:${"2".repeat(64)}`,
    WEATHER_CONTROL_PLANE_SHA256: "3".repeat(64),
    WEATHER_CONTROL_PLANE_VERSION: "15",
    WEATHER_DATABASE_NAME: "weather",
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH: "1",
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH: "0",
    WEATHER_POSTGRES_DIR: "/var/lib/weather/postgres",
    WEATHER_SERVER_IMAGE: `ghcr.io/anstosa/weather-server@sha256:${"4".repeat(64)}`,
    WEATHER_WEB_IMAGE: `ghcr.io/anstosa/weather-web@sha256:${"5".repeat(64)}`,
  };
  await mkdir(state);
  await mkdir(releases);
  await mkdir(scripts, { recursive: true });
  await writeEnvironment(source, { ...environment, WEATHER_RELEASE: "2026.10.09-2" });
  await writeEnvironment(target, { ...environment, WEATHER_RELEASE: "2026.10.09-3",
    WEATHER_SERVER_IMAGE: `ghcr.io/anstosa/weather-server@sha256:${"7".repeat(64)}`,
    WEATHER_WEB_IMAGE: `ghcr.io/anstosa/weather-web@sha256:${"8".repeat(64)}` });
  await writeFile(join(releases, "2026.10.09-3.migration-authorization"), [
    "WEATHER_MIGRATION_AUTHORIZATION_VERSION=1",
    "WEATHER_MIGRATION_AUTHORIZATION_RELEASE=2026.10.09-2",
    "WEATHER_MIGRATION_AUTHORIZATION_SCHEMA_RELEASE=2026.10.09-3",
    `WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256=${history}`,
    "",
  ].join("\n"), { mode: 0o600 });
  await writeFile(join(state, "current-release"), "2026.10.09-2\n", { mode: 0o600 });
  await writeFile(join(state, "schema-release"), "2026.10.09-3\n", { mode: 0o600 });
  await writeFile(join(state, "previous-release"), "2026.10.09-1\n", { mode: 0o600 });
  await writeFile(join(state, "adjustment-candidate-catalog.json"), "catalog\n", { mode: 0o600 });
  await symlink("../releases/2026.10.09-2.env", join(state, "active.env"));
  await symlink(resolve(import.meta.dirname, "../scripts/adjustment-evaluation-package.mjs"),
    join(scripts, "adjustment-evaluation-package.mjs"));
  await writeFile(join(scripts, "adjustment-evaluation-export.sh"), "#!/bin/sh\nexit 0\n", {
    mode: 0o700,
  });
  const script = `source "$1"
state_dir=$2/state
releases_dir=$2/releases
deploy_dir=$2/deploy
real_evaluator=\${1%/*}/adjustment-evaluation-package.mjs
transcript=$3
failure=$4
fixture_history=$5
eval "$(declare -f restore_v14_compatibility_bridge_images | \
  sed '1s/restore_v14_compatibility_bridge_images/restore_v14_compatibility_bridge_images_actual/')"
restore_v14_compatibility_bridge_images() {
  # retain the real helper while recording its exact recovery tuple
  printf 'restore-args:%s:%s:%s\\n' "$1" "$2" "$3" >>"$transcript"
  restore_v14_compatibility_bridge_images_actual "$@"
}
require_v14_bridge_active_release() { :; }
require_v14_source2_full_schema_authority() {
  # authenticate the exact persisted source-two schema authorization
  validate_migration_authorization "$(migration_authorization "$full_v14_release")" \
    "$v14_compatibility_bridge_release" "$full_v14_release"
}
validate_release_env() { :; }
require_control_plane_compatibility() { :; }
require_deployment_secrets() { :; }
prepare_inert_v14_release() { printf '%s/%s.env\\n' "$releases_dir" "$1"; }
require_fixed_v14_source_identity() { :; }
require_inert_v14_empty_catalog() { :; }
require_normal_release_resource_gate() { :; }
require_historical_v14_rehearsal_receipt() { :; }
require_literal_full_v14_release_capacity() { :; }
capture_supported_writer_container() {
  # provide one exact writer identity
  printf '%064d\\n' 0
}
stop_supported_writer_container() {
  # isolate writer shutdown from retry coverage
  :
}
require_supported_writers_quiesced() {
  # isolate writer absence from retry coverage
  :
}
migration_history_sha256() { printf '%s\\n' "$fixture_history"; }
start_postgres() { printf 'postgres:%s\\n' "$1" >>"$transcript"; }
compose() { printf 'migration:%s\\n' "$*" >>"$transcript"; }
apply_runtime_database_acl() { printf 'apply-acl\\n' >>"$transcript"; }
verify_runtime_database_acl() { printf 'verify-acl\\n' >>"$transcript"; }
initialize_adjustment_rain_fixed_gauge_target_sources_v1() { printf 'rain-sources\\n' >>"$transcript"; }
start_exact_release() {
  # inject only the target health boundary after schema mutation
  printf 'target:%s\\n' "$1" >>"$transcript"
  [[ "$failure" != target ]]
}
prepare_xweather_usage_directory() { :; }
start_v14_compatibility_bridge_runtime() {
  # prove recovery receives the persisted schema-21 authorization
  printf 'restore:%s:%s:%s\\n' "$1" \
    "\${WEATHER_MIGRATION_AUTHORIZATION_RELEASE-unset}" \
    "\${WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256-unset}" >>"$transcript"
}
node() {
  # isolate network and protected settings calls while retaining the real selector
  if [[ "$*" == *verify-full-v14-release* ]]; then
    printf '%s\\n' '{"targetCommit":"9999999999999999999999999999999999999999"}'
  elif [[ "$*" == *inert-v14-settings-snapshot* ]]; then
    printf '%s\\n' '11:13 ${"a".repeat(64)}'
  elif [[ "$*" == *verify-inert-v14-settings-snapshot* ]]; then
    :
  elif [[ "$*" == *bootstrap-inert-v14-current* ]]; then
    printf '%s\\n' '{}'
  elif [[ "$*" == *discard-failed-inert-v14-current* ]]; then
    printf '%s\\n' absent
  elif [[ "$*" == *inert-v14-restoration-schema* ]]; then
    command node "$real_evaluator" "\${@:2}"
  else
    command node "$@"
  fi
}
inert_v14_release 2026.10.09-3 "$2/releases/2026.10.09-2.env"`;
  try {
    const failed = spawnSync("bash", ["-c", script, "full-retry", update, root, transcript,
      "target", history], { encoding: "utf8" });
    assert.notEqual(failed.status, 0);
    assert.equal(await readFile(join(state, "current-release"), "utf8"), "2026.10.09-2\n");
    assert.equal(await readFile(join(state, "schema-release"), "utf8"), "2026.10.09-3\n");
    assert.match(await readFile(transcript, "utf8"),
      new RegExp(`restore:.*2026\\.10\\.09-2:${history}`, "u"), failed.stderr);
    const succeeded = spawnSync("bash", ["-c", script, "full-retry", update, root, transcript,
      "none", history], { encoding: "utf8" });
    assert.equal(succeeded.status, 0, succeeded.stderr);
    assert.equal(await readFile(join(state, "current-release"), "utf8"), "2026.10.09-3\n");
    assert.equal(await readFile(join(state, "schema-release"), "utf8"), "2026.10.09-3\n");
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// generic recovery preserves full-schema authority and never falls back to source one
test("generic recovery classifies committed bridge and full release tuples", async () => {
  const scenarios = [
    { current: "2026.10.09-3", schema: "2026.10.09-3",
      transcript: "bridge-receipt\nrestore:2026.10.09-3:2026.10.09-3\nactive:2026.10.09-3\n" },
    { current: "2026.10.09-2", schema: "2026.10.09-3",
      transcript: "bridge-receipt\nrestore:2026.10.09-2:2026.10.09-3\nactive:2026.10.09-2\n" },
  ];
  // verify both complete-full and source-two-over-full-schema recovery
  for (const scenario of scenarios) {
    const result = await runGenericRecovery(scenario.current, scenario.schema);
    try {
      assert.equal(result.result.status, 0, result.result.stderr);
      assert.equal(await readFile(result.transcript, "utf8"), scenario.transcript);
    } finally {
      await rm(result.root, { force: true, recursive: true });
    }
  }
  const unknown = await runGenericRecovery("2026.10.09-3", "2026.10.09-2");
  try {
    assert.notEqual(unknown.result.status, 0);
    assert.match(unknown.result.stderr, /marker tuple is unsupported/u);
    await assert.rejects(readFile(unknown.transcript, "utf8"), /ENOENT/u);
  } finally {
    await rm(unknown.root, { force: true, recursive: true });
  }
  const missing = await runGenericRecovery("2026.10.09-3", "2026.10.09-3", false);
  try {
    assert.notEqual(missing.result.status, 0);
    assert.match(missing.result.stderr, /requires the retained compatibility bridge receipt/u);
    await assert.rejects(readFile(missing.transcript, "utf8"), /ENOENT/u);
  } finally {
    await rm(missing.root, { force: true, recursive: true });
  }
});

// recover an exact full migration that crashed before publishing its schema marker
test("generic recovery repairs an authorized full ledger with source-two health", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-full-marker-crash-"));
  const state = join(root, "state");
  const releases = join(root, "releases");
  const transcript = join(root, "transcript");
  const environment = join(releases, "2026.10.09-2.env");
  const authorization = join(releases, "2026.10.09-3.migration-authorization");
  const history = "6".repeat(64);
  const control = "7".repeat(64);
  await mkdir(state);
  await mkdir(releases);
  await writeFile(join(state, "current-release"), "2026.10.09-2\n", { mode: 0o600 });
  await writeFile(join(state, "schema-release"), "2026.10.09-2\n", { mode: 0o600 });
  await writeFile(join(state, "v14-compatibility-bridge-v1"),
    `WEATHER_V14_BRIDGE_SOURCE_HISTORY_SHA256=${"5".repeat(64)}\n`, { mode: 0o600 });
  await writeEnvironment(environment, {
    CLOUDFLARED_IMAGE: `cloudflare/cloudflared@sha256:${"1".repeat(64)}`,
    POSTGRES_IMAGE: `postgres@sha256:${"2".repeat(64)}`,
    WEATHER_CONTROL_PLANE_SHA256: control,
    WEATHER_CONTROL_PLANE_VERSION: "15",
    WEATHER_DATABASE_NAME: "weather",
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH: "1",
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH: "0",
    WEATHER_POSTGRES_DIR: "/var/lib/weather/postgres",
    WEATHER_RELEASE: "2026.10.09-2",
    WEATHER_SERVER_IMAGE: `ghcr.io/anstosa/weather-server@sha256:${"3".repeat(64)}`,
    WEATHER_WEB_IMAGE: `ghcr.io/anstosa/weather-web@sha256:${"4".repeat(64)}`,
  });
  await writeFile(authorization, [
    "WEATHER_MIGRATION_AUTHORIZATION_VERSION=1",
    "WEATHER_MIGRATION_AUTHORIZATION_RELEASE=2026.10.09-2",
    "WEATHER_MIGRATION_AUTHORIZATION_SCHEMA_RELEASE=2026.10.09-3",
    `WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256=${history}`,
    "",
  ].join("\n"), { mode: 0o600 });
  const script = `source "$1"
state_dir=$2/state
releases_dir=$2/releases
deploy_dir=$2
v14_compatibility_bridge_state=$state_dir/v14-compatibility-bridge-v1
v14_compatibility_bridge_pending=$state_dir/.v14-compatibility-bridge-v1.pending
transcript=$3
fixture_history=$4
expected_control=$5
control_plane_digest() { printf '%s\\n' "$expected_control"; }
require_v14_compatibility_bridge_receipt() { :; }
require_v14_compatibility_bridge_settings() { :; }
require_fixed_v14_migration_ledger() { :; }
migration_history_sha256() { printf '%s\\n' "$fixture_history"; }
require_deployment_secrets() { :; }
prepare_xweather_usage_directory() { :; }
start_postgres() { printf 'postgres\\n' >>"$transcript"; }
apply_runtime_database_acl() { printf 'apply-acl\\n' >>"$transcript"; }
verify_runtime_database_acl() { printf 'verify-acl\\n' >>"$transcript"; }
start_v14_compatibility_bridge_runtime() {
  printf 'runtime:%s:%s\\n' "$WEATHER_MIGRATION_AUTHORIZATION_RELEASE" \
    "$WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256" >>"$transcript"
}
main recover`;
  const result = spawnSync("bash", ["-c", script, "v14-full-marker-crash", update,
    root, transcript, history, control], { encoding: "utf8" });
  try {
    assert.equal(result.status, 0, result.stderr);
    assert.equal(await readFile(join(state, "schema-release"), "utf8"), "2026.10.09-3\n");
    assert.equal(await readFile(join(state, "current-release"), "utf8"), "2026.10.09-2\n");
    assert.match(await readFile(transcript, "utf8"),
      new RegExp(`postgres[\\s\\S]*apply-acl[\\s\\S]*verify-acl[\\s\\S]*runtime:2026\\.10\\.09-2:${history}`, "u"));
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// freeze operator intent from the real descriptor without accepting alternate writers
test("inactive v14 settings snapshot binds bytes, inode, ownership and no aliases", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-inert-v14-settings-"));
  const path = join(root, "settings.json");
  const options = { path, owners: [process.getuid()] };
  const initial = '{"version":1,"temperature":true,"wind":true,"rain":false}\n';
  try {
    await writeFile(path, initial, { mode: 0o600 });
    const before = await readAdjustmentInertV14SettingsSnapshot(options);
    assert.match(before.identity, /^[0-9]+:[0-9]+$/u);
    assert.match(before.sha256, /^[a-f0-9]{64}$/u);
    assert.deepEqual(await readAdjustmentInertV14SettingsSnapshot(options), before);
    await assert.rejects(readAdjustmentInertV14SettingsSnapshot({ path, owners: [] }), /unsafe/u);
    await chmod(path, 0o644);
    await assert.rejects(readAdjustmentInertV14SettingsSnapshot(options), /unsafe/u);
    await chmod(path, 0o600);
    await symlink(path, join(root, "alias.json"));
    await assert.rejects(readAdjustmentInertV14SettingsSnapshot({ ...options, path: join(root, "alias.json") }));
    await link(path, join(root, "hardlink.json"));
    await assert.rejects(readAdjustmentInertV14SettingsSnapshot(options), /unsafe/u);
    await rm(join(root, "hardlink.json"));
    await writeFile(path, initial.replace('"temperature":true', '"temperature":false'));
    const changed = await readAdjustmentInertV14SettingsSnapshot(options);
    assert.equal(changed.identity, before.identity);
    assert.notEqual(changed.sha256, before.sha256);
    await writeFile(join(root, "replacement.json"), initial, { mode: 0o600 });
    await rename(join(root, "replacement.json"), path);
    const replaced = await readAdjustmentInertV14SettingsSnapshot(options);
    assert.equal(replaced.sha256, before.sha256);
    assert.notEqual(replaced.identity, before.identity);
    await writeFile(path, initial.replace('"rain":false', '"rain":false,"approval":true'));
    await assert.rejects(readAdjustmentInertV14SettingsSnapshot(options));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// compensation selects authorization only after observing the real complete ledger
test("inactive v14 restoration refuses partial history and uses source or target exactly", () => {
  const input = { actualHistorySha256: "1".repeat(64), sourceHistorySha256: "1".repeat(64),
    sourceRelease: "2026.10.09-2", sourceSchemaRelease: "2026.10.09-2",
    targetHistorySha256: "2".repeat(64), targetRelease: "2026.10.09-3" };
  assert.equal(selectAdjustmentInertV14RestorationSchema(input), input.sourceRelease);
  assert.equal(selectAdjustmentInertV14RestorationSchema({ ...input,
    actualHistorySha256: input.targetHistorySha256 }), input.targetRelease);
  assert.equal(selectAdjustmentInertV14RestorationSchema({ ...input,
    actualHistorySha256: input.targetHistorySha256,
    sourceHistorySha256: input.targetHistorySha256,
    sourceSchemaRelease: input.targetRelease }), input.targetRelease);
  assert.throws(() => selectAdjustmentInertV14RestorationSchema({ ...input,
    actualHistorySha256: "3".repeat(64) }), /not recognized/u);
  assert.throws(() => selectAdjustmentInertV14RestorationSchema({ ...input,
    actualHistorySha256: "" }), /invalid/u);
});

// a committed full marker requires the exact precommitted root current authority
test("full v14 current verification never creates missing bootstrap authority", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-current-authority-"));
  const input = {
    catalogSha256: "1".repeat(64),
    release: "2026.10.09-3",
    serverImage: `ghcr.io/anstosa/weather-server@sha256:${"2".repeat(64)}`,
    webImage: `ghcr.io/anstosa/weather-web@sha256:${"4".repeat(64)}`,
  };
  try {
    await assert.rejects(verifyAdjustmentInertV14BootstrapCurrent(input, { root }));
    await bootstrapAdjustmentFamilyReleaseCurrent({ ...input, commit: "5".repeat(40),
      settingsSha256: "3".repeat(64) }, {
      clock: () => "2026-10-09T12:00:00.000Z",
      root,
    });
    const verified = await verifyAdjustmentInertV14BootstrapCurrent(input, { root });
    assert.equal(verified.release, input.release);
    assert.equal(verified.settingsSha256, "3".repeat(64));
    await assert.rejects(verifyAdjustmentInertV14BootstrapCurrent({ ...input,
      catalogSha256: "6".repeat(64) }, { root }), /differs/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// keep mutable operator settings separate from the retained commit-time authority
test("full v14 recovery accepts a valid live settings change", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-current-settings-change-"));
  const transcript = join(root, "transcript");
  const script = `source "$1"
state_dir=$2
deploy_dir=/opt/weather/current/deploy
transcript=$2/transcript
full_v14_release=2026.10.09-3
require_file() { :; }
sha256sum() { printf '%s  %s\\n' '${"1".repeat(64)}' "$1"; }
env_value() {
  # return only the immutable image identities used by root-current verification
  if [[ "$2" == WEATHER_SERVER_IMAGE ]]; then
    printf '%s\\n' 'ghcr.io/anstosa/weather-server@sha256:${"2".repeat(64)}'
  else
    printf '%s\\n' 'ghcr.io/anstosa/weather-web@sha256:${"3".repeat(64)}'
  fi
}
node() {
  # model a legitimate post-bootstrap settings update with a different live hash
  if [[ "$*" == *inert-v14-settings-snapshot* ]]; then
    printf '%s\\n' '17:29 ${"4".repeat(64)}'
  else
    printf '%s\\n' "$*" >"$transcript"
  fi
}
require_full_v14_current_authority "$2/release.env"`;
  try {
    await writeFile(join(root, "adjustment-candidate-catalog.json"), "catalog\n");
    const result = spawnSync("bash", ["-c", script, "settings-change", update, root], {
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    const invocation = await readFile(transcript, "utf8");
    assert.match(invocation, /verify-inert-v14-current 2026\.10\.09-3/u);
    assert.doesNotMatch(invocation, new RegExp("4{64}", "u"));
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});
