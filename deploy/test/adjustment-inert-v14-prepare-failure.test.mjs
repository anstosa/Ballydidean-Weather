import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  readlink,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const update = resolve(import.meta.dirname, "../scripts/update.sh");

// execute the real preparer through the production bridge boundary
async function runRealPreparationFailure(failure, existingTarget = false) {
  const root = await mkdtemp(join(tmpdir(), "weather-v14-real-prepare-"));
  const state = join(root, "state");
  const releases = join(root, "releases");
  const transcript = join(root, "transcript");
  const success = join(root, "success");
  const source = join(releases, "2026.10.09-1.env");
  const target = join(releases, "2026.10.09-2.env");
  await mkdir(state);
  await mkdir(releases);
  await writeFile(join(state, "current-release"), "2026.10.09-1\n", { mode: 0o600 });
  await writeFile(join(state, "schema-release"), "2026.10.09-1\n", { mode: 0o600 });
  await writeFile(join(state, "previous-release"), "2026.10.08-1\n", { mode: 0o600 });
  await symlink("../releases/2026.10.09-1.env", join(state, "active.env"));
  await writeFile(source, "WEATHER_DATABASE_NAME=weather\n", { mode: 0o600 });
  // preserve one exact pre-existing target branch fixture
  if (existingTarget) {
    await writeFile(target, "existing-target\n", { mode: 0o600 });
  }
  const script = `source "$1"
state_dir=$2/state
releases_dir=$2/releases
v14_compatibility_bridge_state=$state_dir/v14-compatibility-bridge-v1
transcript=$3
failure=$4
success=$5
literal_capacity_calls=0
# record the exact injected boundary
checkpoint() { printf '%s\n' "$1" >>"$transcript"; return 1; }
# resolve only fixture release environments
release_env() { printf '%s/%s.env\n' "$releases_dir" "$1"; }
validate_release_env() { :; }
require_control_plane_compatibility() { :; }
require_v14_bridge_active_release() { :; }
require_deployment_secrets() { :; }
# return deterministic proof and settings values
node() {
  # select only the three production node call shapes
  case "$*" in
    *verify-v14-compatibility-bridge-release*) printf '%s\n' '{"targetCommit":"f6daa89d135661eb60dd5479c5d0d6c23e7198a9"}' ;;
    *--input-type*) printf '%s\n' f6daa89d135661eb60dd5479c5d0d6c23e7198a9 ;;
    *) printf '%s\n' '1:2 ${"a".repeat(64)}' ;;
  esac
}
# return fixed source values without evaluating fixture bytes
env_value() {
  # bind each requested source field
  case "$2" in
    WEATHER_DATABASE_NAME) printf '%s\n' weather ;;
    WEATHER_SERVER_IMAGE) printf '%s\n' 'ghcr.io/anstosa/weather-server@sha256:${"1".repeat(64)}' ;;
    WEATHER_WEB_IMAGE) printf '%s\n' 'ghcr.io/anstosa/weather-web@sha256:${"2".repeat(64)}' ;;
    POSTGRES_IMAGE) printf '%s\n' 'postgres@sha256:${"3".repeat(64)}' ;;
    CLOUDFLARED_IMAGE) printf '%s\n' 'cloudflare/cloudflared@sha256:${"4".repeat(64)}' ;;
  esac
}
image_repository() { printf '%s\n' ghcr.io/anstosa/weather; }
resolve_arm64_image() { printf '%s\n' 'ghcr.io/anstosa/weather@sha256:${"5".repeat(64)}'; }
# render only one inert target candidate
write_release_env() { printf 'rendered-target\n' >"$2"; }
require_v14_compatibility_bridge_release_pair() { :; }
require_v14_compatibility_bridge_migration_ledger() { :; }
validate_image_reference() { :; }
cleanup_obsolete_weather_images() { :; }
require_image_pull_capacity_floor() { :; }
require_normal_release_resource_gate() { :; }
# fail the selected literal capacity sample
require_literal_inert_v14_release_capacity() {
  literal_capacity_calls=$((literal_capacity_calls + 1))
  # reject only the selected pre- or post-pull sample
  case "$failure:$literal_capacity_calls" in
    capacity-initial:1|capacity-post:2) checkpoint "$failure" ;;
  esac
}
# emulate only config inspection and image pulling
compose() {
  # preserve exact compose subcommand behavior
  case "$*" in
    "config --quiet") : ;;
    "config --images")
      printf '%s\n' \
        'ghcr.io/anstosa/weather-server@sha256:${"1".repeat(64)}' \
        'ghcr.io/anstosa/weather-web@sha256:${"2".repeat(64)}' \
        'postgres@sha256:${"3".repeat(64)}' \
        'cloudflare/cloudflared@sha256:${"4".repeat(64)}'
      ;;
    pull)
      # reject only the selected pull
      [[ "$failure" != compose-pull ]] || checkpoint compose-pull
      ;;
  esac
}
migration_history_sha256() { printf '%064d\n' 1; }
# expose every forbidden runtime or publication boundary
publish_v14_compatibility_bridge_state() { printf 'publish\n' >>"$transcript"; }
start_v14_compatibility_bridge_runtime() { printf 'runtime\n' >>"$transcript"; }
write_private_state() { printf 'state:%s\n' "\${1##*/}" >>"$transcript"; }
write_active_symlink() { printf 'active\n' >>"$transcript"; }
recover_v14_compatibility_bridge() { printf 'recover\n' >>"$transcript"; }
v14_compatibility_bridge_release 2026.10.09-2
printf 'success\n' >"$success"`;
  const result = spawnSync("bash", ["-c", script, "v14-real-prepare", update, root,
    transcript, failure, success], { encoding: "utf8" });
  return { releases, result, root, source, state, success, target, transcript };
}

// require every preparation failure to remain prepublication
test("real compatibility preparer cannot publish after required preparation failures", async () => {
  const cases = [
    ["capacity-initial", false],
    ["compose-pull", false],
    ["capacity-post", false],
    ["capacity-post", true],
  ];
  // verify every new and existing target failure boundary
  for (const [failure, existingTarget] of cases) {
    const scenario = await runRealPreparationFailure(failure, existingTarget);
    // inspect the isolated failure result
    try {
      assert.notEqual(scenario.result.status, 0, failure);
      assert.equal(scenario.result.stdout, "", failure);
      await assert.rejects(readFile(scenario.success), /ENOENT/u, failure);
      const transcript = await readFile(scenario.transcript, "utf8");
      assert.match(transcript, new RegExp(`^${failure}\\n$`, "u"), failure);
      assert.doesNotMatch(transcript, /publish|runtime|state:|active|success/u, failure);
      assert.equal(await readFile(join(scenario.state, "current-release"), "utf8"), "2026.10.09-1\n", failure);
      assert.equal(await readFile(join(scenario.state, "schema-release"), "utf8"), "2026.10.09-1\n", failure);
      assert.equal(await readlink(join(scenario.state, "active.env")), "../releases/2026.10.09-1.env", failure);
      assert.equal(await readFile(scenario.source, "utf8"), "WEATHER_DATABASE_NAME=weather\n", failure);
      // retain or reject the target exactly as it began
      if (existingTarget) {
        assert.equal(await readFile(scenario.target, "utf8"), "existing-target\n", failure);
      } else {
        await assert.rejects(lstat(scenario.target), /ENOENT/u, failure);
      }
      // remove only the owned incomplete render
      assert.deepEqual(
        (await readdir(scenario.releases)).filter((name) => name.includes(".partial")),
        [],
        `${failure}: ${scenario.result.stderr}`,
      );
    } finally {
      // remove the isolated fixture
      await rm(scenario.root, { force: true, recursive: true });
    }
  }
});
