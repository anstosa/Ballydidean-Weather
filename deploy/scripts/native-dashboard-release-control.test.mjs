import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const CONTROLLER = fileURLToPath(
  new URL("./native-dashboard-release-control.sh", import.meta.url),
);
const CORE = "2099.01.01-1";
const TARGET = "2099.01.01-2";
const CORE_COMMIT = "a".repeat(40);
const TARGET_COMMIT = "b".repeat(40);
const CORE_IMAGE = `ghcr.io/anstosa/weather-web@sha256:${"1".repeat(64)}`;
const TARGET_IMAGE = `ghcr.io/anstosa/weather-web@sha256:${"2".repeat(64)}`;
const CORE_IMAGE_ID = `sha256:${"3".repeat(64)}`;
const TARGET_IMAGE_ID = `sha256:${"4".repeat(64)}`;

// hash one fixture file with the same bytes as the installed owner
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// prove the actual controller stages, activates and rolls back only the web service
test("stages activates and rolls back one fixed native dashboard publication", async () => {
  const root = await mkdtemp(join(tmpdir(), "native-dashboard-controller-"));
  const publicationRoot = join(root, "publications");
  const publicationDirectory = join(publicationRoot, "by-release", TARGET);
  const state = join(root, "state");
  const releases = join(state, "releases");
  const stages = join(root, "stages");
  const log = join(root, "operations");
  const controller = join(root, "native-dashboard-release-control.sh");
  const publication = join(publicationDirectory, "publication.json");
  try {
    await mkdir(publicationDirectory, { recursive: true, mode: 0o700 });
    await mkdir(releases, { recursive: true, mode: 0o700 });
    await mkdir(stages, { mode: 0o700 });
    const publicationBytes = Buffer.from('{"contractVersion":"fixture"}\n');
    const publicationSha = sha256(publicationBytes);
    await writeFile(publication, publicationBytes, { mode: 0o600 });
    await writeFile(
      join(publicationDirectory, "publication.sha256"),
      `${publicationSha}  publication.json\n`,
      { mode: 0o600 },
    );
    const corePublication = join(stages, CORE, "publication.json");
    await mkdir(join(stages, CORE), { mode: 0o700 });
    await writeFile(corePublication, '{"contractVersion":"core"}\n', { mode: 0o600 });
    await writeFile(
      join(releases, `${CORE}.env`),
      `WEATHER_RELEASE=${CORE}\nWEATHER_RUNTIME_GENERATION=rust-v1\n` +
        `WEATHER_SERVER_IMAGE=ghcr.io/anstosa/weather-server@sha256:${"5".repeat(64)}\n` +
        `WEATHER_WEB_IMAGE=${CORE_IMAGE}\n`,
      { mode: 0o600 },
    );
    await writeFile(join(state, "current-release"), `${CORE}\n`, { mode: 0o600 });
    const source = (await readFile(CONTROLLER, "utf8")).replace(
      "/opt/weather/operators/native-dashboard-publications",
      publicationRoot,
    );
    await writeFile(controller, source, { mode: 0o600 });
    const body = String.raw`
set -euo pipefail
state_dir=$2
sp_root=$3
log=$4
sp_module=/fixture/schema-preserving-release.mjs
compose_file=/fixture/compose.yml
dashboard_release_journal="$state_dir/dashboard-release-v1.env"
fixture_core=$5
fixture_target=$6
fixture_core_commit=$7
fixture_target_commit=$8
fixture_core_image=$9
shift 9
fixture_target_image=$1
fixture_core_image_id=$2
fixture_target_image_id=$3
fixture_publication_sha=$4
fail_pull=false
fail_capacity=false
native_evidence_binary=capacity_fixture
# retain canonical disposable paths
realpath() { printf '%s\n' "$3"; }
# project root-owned directory metadata
stat() { printf '0:0:700\n'; }
# create only private disposable directories without requiring host root ownership
install() { local destination; for destination in "$@"; do :; done; mkdir -p "$destination"; chmod 700 "$destination"; }
# admit regular private fixture files
sp_private() { [[ -f "$1" && ! -L "$1" ]]; }
# hash exact retained bytes
sp_sha() { sha256sum -- "$1" | awk '{print $1}'; }
# expose closed publication fields and the compose override
node() {
  if [[ "$2" == field ]]; then
    case "$4" in
      coreRelease|sourceRelease) printf '%s\n' "$fixture_core" ;;
      corePublicationSha256|sourcePublicationSha256) sp_sha "$sp_root/$fixture_core/publication.json" ;;
      coreCommit|sourceCommit) printf '%s\n' "$fixture_core_commit" ;;
      targetCommit) printf '%s\n' "$fixture_target_commit" ;;
      sourceWebImage) printf '%s\n' "$fixture_core_image" ;;
      sourceWebImageId) printf '%s\n' "$fixture_core_image_id" ;;
      targetWebImage) printf '%s\n' "$fixture_target_image" ;;
      targetWebImageId) printf '%s\n' "$fixture_target_image_id" ;;
      measuredAt) date -u +%Y-%m-%dT%H:%M:%S.000Z ;;
      *) return 91 ;;
    esac
  elif [[ "$2" == runtime-compose-override ]]; then
    printf '{}\n'
  elif [[ "$2" == validate-dashboard-publication ]]; then
    return 0
  elif [[ "$2" == validate-dashboard-capacity ]]; then
    [[ -s "$3" ]]
  else
    return 92
  fi
}
# record the exact installed capacity command and its measured admission outcome
capacity_fixture() {
  [[ "$1" == dashboard-release-capacity && "$#" == 6 && "$3" == "$fixture_core" &&
    "$5" == "$fixture_core_image" && "$6" == "$fixture_target_image" ]]
  printf 'native-capacity:%s\n' "$*" >>"$log"
  printf '{}\n'
  if [[ "$fail_capacity" == true ]]; then return 3; fi
}
# expose the ordinary core publication commit and immutable web image
sp_field() {
  case "$2" in
    targetCommit) printf '%s\n' "$fixture_core_commit" ;;
    webImage) printf '%s\n' "$fixture_core_image" ;;
    webImageId) printf '%s\n' "$fixture_core_image_id" ;;
    *) return 1 ;;
  esac
}
# expose and validate the current native core environment
sp_source() { printf '%s/releases/%s.env\n' "$state_dir" "$fixture_core"; }
sp_runtime_generation() { printf 'rust-v1\n'; }
env_value() { sed -n "s/^$2=//p" "$1"; }
read_optional_release_state() { [[ -f "$1" ]] && tr -d '\n' <"$1" || true; }
read_release_state() { tr -d '\n' <"$1"; }
validate_release() { [[ "$1" =~ ^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[1-9][0-9]?$ ]]; }
release_env() { printf '%s/releases/%s.env\n' "$state_dir" "$1"; }
dashboard_release_env() { printf '%s/releases/%s.web.env\n' "$state_dir" "$1"; }
validate_dashboard_release_env() {
  [[ "$(env_value "$1" WEATHER_RELEASE)" == "$2" &&
    "$(env_value "$1" WEATHER_CORE_RELEASE)" == "$3" ]]
}
write_dashboard_release_env() {
  printf 'WEATHER_RELEASE=%s\nWEATHER_CORE_RELEASE=%s\nWEATHER_RUNTIME_GENERATION=rust-v1\nWEATHER_WEB_IMAGE=%s\n' \
    "$3" "$(env_value "$1" WEATHER_RELEASE)" "$4" >"$2"
  chmod 600 "$2"
}
# record only web compose operations and return one exact web container identity
compose() {
  if [[ "$*" == *"ps -q web" ]]; then
    [[ "$WEATHER_ENV_FILE" == *"$fixture_target.web.env" ]] && printf 'd%.0s' {1..64} || printf 'c%.0s' {1..64}
    printf '\n'
    return
  elif [[ "$*" == *"ps -a -q api" ]]; then
    printf 'a%.0s' {1..64}; printf '\n'
    return
  elif [[ "$*" == *"ps -a -q worker" ]]; then
    printf 'b%.0s' {1..64}; printf '\n'
    return
  fi
  printf 'compose:%s:%s\n' "$WEATHER_ENV_FILE" "$*" >>"$log"
  # simulate one ordinary interrupted pull before any marker changes
  if [[ "$*" == *'pull web' && "$fail_pull" == true ]]; then return 94; fi
}
docker() {
  local image
  # select the final image or container operand
  for image in "$@"; do :; done
  if [[ "$1" == image ]]; then
    if [[ "$*" == *"org.weather.runtime.generation"* ]]; then
      printf 'rust-v1|%s\n' "$fixture_target_commit"
    else
      [[ "$image" == "$fixture_core_image" ]] && printf '%s\n' "$fixture_core_image_id" || printf '%s\n' "$fixture_target_image_id"
    fi
  elif [[ "$image" == d* ]]; then
    printf '%s|%s|true|healthy\n' "$fixture_target_image" "$fixture_target_image_id"
  elif [[ "$image" == a* || "$image" == b* ]]; then
    printf '%s|sha256:%064d|ghcr.io/anstosa/weather-server@sha256:%064d|0|0|0|0|0|0\n' \
      "$image" 6 7
  else
    printf '%s|%s|true|healthy\n' "$fixture_core_image" "$fixture_core_image_id"
  fi
}
sp_snapshot() { printf '{"snapshot":"core"}\n' >"$2"; chmod 600 "$2"; printf 'snapshot\n' >>"$log"; }
sp_capacity() { printf 'capacity\n' >>"$log"; }
sp_unchanged() { printf 'unchanged:%s\n' "$2" >>"$log"; }
sp_database() { printf 'database:%s\n' "$1" >>"$log"; }
sp_image() { printf 'image:%s:%s\n' "$1" "$2" >>"$log"; [[ "$2" == "$fixture_core_image_id" || "$2" == "$fixture_target_image_id" ]]; }
write_dashboard_release_journal() {
  printf 'journal:%s:%s:%s:%s\n' "$1" "$2" "$3" "$4" >>"$log"
  : >"$dashboard_release_journal"
  chmod 600 "$dashboard_release_journal"
}
remove_dashboard_state() { rm -f -- "$1"; }
record_dashboard_release_success() {
  if [[ "$1" == "$3" ]]; then
    rm -f -- "$state_dir/active-web.env" "$state_dir/current-web-release" "$state_dir/previous-web-release"
  else
    printf '%s\n' "$2" >"$state_dir/previous-web-release"
    ln -sfn "../releases/$1.web.env" "$state_dir/active-web.env"
    printf '%s\n' "$1" >"$state_dir/current-web-release"
  fi
}
restore_dashboard_marker_snapshot() { return 93; }
printf '%s\n' "$fixture_core" >"$state_dir/schema-release"
ln -s "../releases/$fixture_core.env" "$state_dir/active.env"
fail_capacity=true
if native_dashboard_stage "$fixture_target" "$fixture_core" "$fixture_core"; then exit 96; fi
[[ ! -e "$sp_root/native-dashboard/$fixture_target" ]]
fail_capacity=false
fail_pull=true
if native_dashboard_stage "$fixture_target" "$fixture_core" "$fixture_core"; then exit 95; fi
[[ ! -e "$sp_root/native-dashboard/$fixture_target" ]]
fail_pull=false
native_dashboard_stage "$fixture_target" "$fixture_core" "$fixture_core"
native_dashboard_activate "$fixture_target"
[[ "$(read_release_state "$state_dir/current-web-release")" == "$fixture_target" ]]
native_dashboard_release "$fixture_target" "$fixture_core" "$fixture_core"
native_dashboard_system_recover
native_dashboard_rollback core
[[ ! -e "$state_dir/current-web-release" && ! -e "$state_dir/active-web.env" ]]
`;
    const result = spawnSync(
      "bash",
      [
        "-c",
        `source "$1"\n${body}`,
        "native-dashboard-controller-test",
        controller,
        state,
        stages,
        log,
        CORE,
        TARGET,
        CORE_COMMIT,
        TARGET_COMMIT,
        CORE_IMAGE,
        TARGET_IMAGE,
        CORE_IMAGE_ID,
        TARGET_IMAGE_ID,
        publicationSha,
      ],
      { encoding: "utf8" },
    );
    assert.equal(result.status, 0, result.stderr);
    const operations = await readFile(log, "utf8");
    assert.equal(operations.match(/^compose:.*\bpull web$/gmu)?.length, 2);
    assert.equal(operations.match(/^compose:.*\bup -d --no-deps --no-build --pull never --wait web$/gmu)?.length, 3);
    assert.equal(operations.match(/^journal:release:/gmu)?.length, 1);
    assert.equal(operations.match(/^journal:rollback:/gmu)?.length, 1);
    assert.equal(operations.match(/^native-capacity:/gmu)?.length, 4);
    assert.equal(operations.match(/^database:/gmu)?.length, 10);
    assert.doesNotMatch(operations, /(?:up|pull).*\b(?:api|worker)\b/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// classify each ordered marker-write and compensation boundary from the real recovery owner
test("dashboard recovery recognizes interrupted core and non-core marker writes", async () => {
  const root = await mkdtemp(join(tmpdir(), "native-dashboard-markers-"));
  try {
    const body = String.raw`
set -euo pipefail
state_dir=$2
sp_root="$state_dir/stages"
dashboard_release_journal="$state_dir/journal.env"
mkdir -p "$sp_root/native-dashboard"
pending="$sp_root/native-dashboard/pending-protected.json"
printf '{}\n' >"$pending"
sp_private() { [[ -f "$1" && ! -L "$1" ]]; }
sp_sha() { sha256sum -- "$1" | awk '{print $1}'; }
env_value() { sed -n "s/^$2=//p" "$1"; }
read_optional_release_state() { [[ ! -f "$1" ]] || tr -d '\n' <"$1"; }
validate_dashboard_release_journal() { [[ -s "$dashboard_release_journal" ]]; }
fixture_core=2099.01.01-1
fixture_source=2099.01.01-2
fixture_target=2099.01.01-3
# create one journal with the exact original marker tuple
journal() {
  printf '%s\n' "WEATHER_DASHBOARD_CORE_RELEASE=$fixture_core" \
    "WEATHER_DASHBOARD_SOURCE_RELEASE=$fixture_source" "WEATHER_DASHBOARD_TARGET_RELEASE=$1" \
    "WEATHER_DASHBOARD_ORIGINAL_CURRENT=$fixture_source" "WEATHER_DASHBOARD_ORIGINAL_PREVIOUS=$fixture_core" \
    "WEATHER_DASHBOARD_CORE_SNAPSHOT_SHA256=$(sp_sha "$pending")" >"$dashboard_release_journal"
}
# materialize only the disposable current/previous/link boundary
markers() {
  rm -f -- "$state_dir/current-web-release" "$state_dir/previous-web-release" "$state_dir/active-web.env"
  if [[ "$1" != none ]]; then printf '%s\n' "$1" >"$state_dir/current-web-release"; fi
  if [[ "$2" != none ]]; then printf '%s\n' "$2" >"$state_dir/previous-web-release"; fi
  if [[ "$3" != none ]]; then ln -s "../releases/$3.web.env" "$state_dir/active-web.env"; fi
}
journal "$fixture_target"
markers "$fixture_source" "$fixture_core" "$fixture_source"
native_dashboard_recovery_boundary
markers "$fixture_source" "$fixture_source" "$fixture_target"
native_dashboard_recovery_boundary
markers "$fixture_target" "$fixture_source" "$fixture_target"
native_dashboard_recovery_boundary
markers "$fixture_target" "$fixture_source" "$fixture_source"
native_dashboard_recovery_boundary
journal "$fixture_core"
markers "$fixture_source" none none
native_dashboard_recovery_boundary
markers "$fixture_source" none "$fixture_source"
native_dashboard_recovery_boundary
markers none none "$fixture_source"
native_dashboard_recovery_boundary
markers none none none
native_dashboard_recovery_boundary
`;
    const result = spawnSync("bash", ["-c", `source "$1"\n${body}`, "native-dashboard-markers", CONTROLLER, root], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, "original\noriginal\ncommitted\noriginal\noriginal\noriginal\noriginal\ncommitted\n");
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});
