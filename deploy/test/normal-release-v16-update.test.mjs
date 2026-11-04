import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "../..");
const updatePath = join(root, "deploy/scripts/update.sh");
const predecessorDigest = "af80d5752590bf47f5b893ee69f04215132e80210993f4a8c78d8562d2ea9a52";
const stagedDigest = "e089e5c092fb336d0f8130cbf45320ed013a55af3b10957f2fd95a03265a2ed2";
const historicalDigest = "603eb8f488ba78be3d7ecf76b0d587346d1c36432255d390d86d2b768c8fecba";

const sourceEnvironment = `WEATHER_RELEASE=2026.10.09-2
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
const bridgeState = `WEATHER_V14_BRIDGE_CONTRACT_VERSION=weather-v14-compatibility-bridge/v1
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

// select one named shell function body
function shellFunction(source, name) {
  const start = source.indexOf(`${name}() {`);
  assert.notEqual(start, -1, `missing ${name}`);
  const next = source.indexOf("\n}\n\n#", start);
  assert.notEqual(next, -1, `unterminated ${name}`);
  return source.slice(start, next + 3);
}

test("v17 routes AF80 and E089 only through the exact chained handoff", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-v16-handoff-"));
  const envFile = join(directory, "release.env");
  const harness = `
source "$1"
# inject the installed digest for router isolation
control_plane_digest() { printf '%s\\n' "$INSTALLED_DIGEST"; }
# inject the handoff decision for router isolation
require_v17_control_plane_handoff() { [[ "$HANDOFF_OK" == true ]]; }
require_control_plane_compatibility "$2"
`;

  // run one release identity through the real compatibility router
  const check = async (version, digest, release, handoffOk = false) => {
    await writeFile(envFile, [
      `WEATHER_CONTROL_PLANE_VERSION=${version}`,
      `WEATHER_CONTROL_PLANE_SHA256=${digest}`,
      `WEATHER_RELEASE=${release}`,
      "",
    ].join("\n"));
    return spawnSync("bash", ["-c", harness, "v16-handoff", updatePath, envFile], {
      encoding: "utf8",
      env: {
        ...process.env,
        HANDOFF_OK: String(handoffOk),
        INSTALLED_DIGEST: "a".repeat(64),
      },
    });
  };

  try {
    assert.equal((await check(17, "a".repeat(64), "2026.10.09-4")).status, 0);
    assert.equal((await check(15, predecessorDigest, "2026.10.09-2", true)).status, 0);
    assert.notEqual((await check(15, predecessorDigest, "2026.10.09-2", false)).status, 0);
    assert.notEqual((await check(15, predecessorDigest, "2026.10.09-3", true)).status, 0);
    assert.equal((await check(16, stagedDigest, "2026.10.09-3", true)).status, 0);
    assert.notEqual((await check(16, stagedDigest, "2026.10.09-3", false)).status, 0);
    assert.notEqual((await check(16, stagedDigest, "2026.10.09-2", true)).status, 0);
    assert.equal((await check(13, historicalDigest, "2026.10.09-1")).status, 0);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("v16 handoff validator executes one complete sealed fixture", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-v16-handoff-real-"));
  const deploy = join(directory, "deploy");
  const state = join(deploy, "state");
  const releases = join(deploy, "releases");
  const sourceEnv = join(releases, "2026.10.09-2.env");
  const bridge = join(state, "v14-compatibility-bridge-v1");
  const receipt = join(state, "control-plane-handoff-v16-v1.json");
  const harness = `
source "$1"
deploy_dir=$2
repo_root=$3
state_dir="$deploy_dir/state"
releases_dir="$deploy_dir/releases"
v14_compatibility_bridge_state="$state_dir/v14-compatibility-bridge-v1"
v16_control_plane_handoff="$state_dir/control-plane-handoff-v16-v1.json"
FIXTURE_ROOT=$3
stat() {
  # model production root ownership while retaining real fixture modes and sizes
  if [[ "$1" == -c && "$3" == "$FIXTURE_ROOT/"* ]]; then
    case "$2" in
      %u:%g:%a:%h) printf '0:0:%s:1\\n' "$(command stat -c %a "$3")" ;;
      %u:%g:%h) printf '0:0:1\\n' ;;
      *) command stat "$@" ;;
    esac
  else
    command stat "$@"
  fi
}
node --input-type=module - "$v16_control_plane_handoff" <<'NODE'
import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
const [path] = process.argv.slice(2);
const body = {
  bridgeStateSha256: "c944a152f3129d7bf3bf3b2dcd2b4d230ca58c64719360e22351eb8043779c62",
  candidateIdentityManifestSha256: "157281ebd1ebc35691a34e085722a72ea3a530ca9b6cc75832d77ab6d868be5e",
  contractVersion: "weather-control-plane-handoff/v16-v1",
  installedAt: "2026-11-03T00:38:21Z",
  predecessorIdentityManifestSha256: "d1f64bf6c03fa35d198d6e9edc58b9d9ac4225e829e2c41e68761fe5ca827536",
  sourceControlSha256: "af80d5752590bf47f5b893ee69f04215132e80210993f4a8c78d8562d2ea9a52",
  sourceControlVersion: 15,
  sourceRelease: "2026.10.09-2",
  successorControlSha256: "e089e5c092fb336d0f8130cbf45320ed013a55af3b10957f2fd95a03265a2ed2",
  successorControlVersion: 16,
};
const canonical = (value) => Array.isArray(value)
  ? \`[\${value.map(canonical).join(",")}]\`
  : value !== null && typeof value === "object"
    ? \`{\${Object.keys(value).sort().map((key) => \`\${JSON.stringify(key)}:\${canonical(value[key])}\`).join(",")}}\`
    : JSON.stringify(value);
const closed = { ...body,
  receiptSha256: createHash("sha256").update(\`\${canonical(body)}\\n\`).digest("hex") };
writeFileSync(path, \`\${canonical(closed)}\\n\`, { mode: 0o600 });
NODE
require_v16_control_plane_handoff "$4"
`;

  try {
    // create every fixed receipt root
    for (const name of ["state", "releases"]) {
      await mkdir(join(deploy, name), { recursive: true });
    }
    await writeFile(sourceEnv, sourceEnvironment, { mode: 0o600 });
    await writeFile(bridge, bridgeState, { mode: 0o600 });
    const result = spawnSync("bash", ["-c", harness, "v16-real", updatePath, deploy,
      directory, sourceEnv], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("v17 handoff verifier chains the immutable v16 receipt", async () => {
  const source = await readFile(updatePath, "utf8");
  const v16Verifier = shellFunction(source, "require_v16_control_plane_handoff");
  const v17Verifier = shellFunction(source, "require_v17_control_plane_handoff");
  assert.match(source, /^control_plane_version=17$/mu);
  assert.match(source, /^recurring_previous_release=2026\.10\.09-1$/mu);
  assert.match(source, new RegExp(`^recurring_previous_control_plane_sha256=${historicalDigest}$`, "mu"));
  assert.match(source, /^v16_handoff_source_release=2026\.10\.09-2$/mu);
  assert.match(source, /^v16_handoff_source_control_version=15$/mu);
  assert.match(source, new RegExp(`^v16_handoff_source_control_sha256=${predecessorDigest}$`, "mu"));
  assert.match(v16Verifier, /weather-control-plane-handoff\/v16-v1/u);
  assert.match(source, /d1f64bf6c03fa35d198d6e9edc58b9d9ac4225e829e2c41e68761fe5ca827536/u);
  assert.match(source, /97368a14e368dcf7fd3f3f179cd222df2f848695e1cd993abf8892e69c30d4e7/u);
  assert.match(source, /c944a152f3129d7bf3bf3b2dcd2b4d230ca58c64719360e22351eb8043779c62/u);
  assert.match(source, /63daabc14561306402d6130d8d644dc7025148f837b40f34791f00b3106bef2e/u);
  assert.match(source, /f1acdcf00a21e7937b639d9d41910d695c5589175083eb5c3ba97f07066e7bfa/u);
  assert.match(source, /f4bd73a18649894a25d9b4f54784bb5eccb099b4226dd5aa7df50f09906dc63b/u);
  assert.match(v16Verifier, /candidateIdentityManifestSha256/u);
  assert.match(v16Verifier, /receiptSha256/u);
  assert.match(v16Verifier, /raw\.at\(-1\) !== 0x0a/u);
  assert.match(v17Verifier, /require_v16_control_plane_handoff/u);
  assert.match(v17Verifier, /weather-control-plane-handoff\/v17-v1/u);
  assert.match(v17Verifier, /predecessorHandoffFileSha256/u);
  assert.match(v17Verifier, /sourceEnvironmentSha256/u);
  assert.match(v17Verifier, /successorControlVersion !== 17/u);
});

test("normal stage proves images before live SQL and closes all three phases", async () => {
  const source = await readFile(updatePath, "utf8");
  const writer = shellFunction(source, "write_normal_stage_capacity_input");
  const stage = source.slice(source.indexOf("  stage)"), source.indexOf("  activate)"));
  assert.ok(writer.indexOf("captured_at=") < writer.indexOf("migration_history_sha256"));
  assert.match(writer, /imageProofSha256/u);
  const proof = stage.indexOf("require_normal_stage_image_proof");
  const pre = stage.indexOf("pre_pull");
  const pull = stage.indexOf("compose pull");
  const post = stage.indexOf("post_pull");
  const clone = stage.indexOf("verify_previous_image_compatibility");
  const cleanup = stage.indexOf("post_cleanup");
  const qualification = stage.indexOf("write_normal_stage_qualification_receipt");
  assert.ok(proof >= 0 && proof < pre && pre < pull && pull < post && post < clone &&
    clone < cleanup && cleanup < qualification);
  assert.match(source, /normal-stage-qualification\/v2/u);
  assert.match(source, /prePullReceipt: pre/u);
  assert.match(source, /postPullReceipt: post/u);
  assert.match(source, /postCleanupReceipt: cleanup/u);
  assert.match(source, /verifyNormalCapacityReceipt/u);
});

test("normal activation reuses the pre ceiling and records only delta diagnostics", async () => {
  const source = await readFile(updatePath, "utf8");
  const writer = shellFunction(source, "write_normal_activation_capacity_input");
  const pair = shellFunction(source, "require_normal_activation_capacity_pair");
  assert.ok(writer.indexOf("captured_at=") < writer.indexOf("migration_history_sha256"));
  assert.match(writer, /preBackupReceiptSha256/u);
  assert.match(writer, /actualBackupAllocatedBytes/u);
  assert.match(writer, /boundedBackupCeilingBytes/u);
  assert.match(pair, /verifyNormalCapacityReceipt\(JSON\.parse/u);
  assert.match(pair, /validateNormalCapacityReceipt\(JSON\.parse\(readFileSync\(postPath\)\), new Date\(\)\)/u);
  assert.match(pair, /preBackupReceiptSha256/u);
  assert.match(pair, /allocatedBytes/u);
  assert.doesNotMatch(pair, /consumedBytes > Number\(allocatedBytes\) \+ 8_192/u);
  assert.doesNotMatch(pair, /consumedInodes > 2/u);
  assert.match(pair, /diagnostic/u);
});

// execute only the staged publication lifecycle against bounded fixture artifacts
async function runStagePublicationFixture(directory, failLate) {
  const deploy = join(directory, "deploy");
  const releases = join(deploy, "releases");
  const state = join(deploy, "state");
  const sourceEnv = join(releases, "2026.10.09-2.env");
  await mkdir(releases, { recursive: true });
  await mkdir(state, { recursive: true });
  await writeFile(sourceEnv, sourceEnvironment, { mode: 0o600 });
  await writeFile(join(directory, "unrelated"), "preserve\n", { mode: 0o600 });
  const harness = `
source "$1"
deploy_dir=$2
repo_root=$3
releases_dir="$deploy_dir/releases"
state_dir="$deploy_dir/state"
SOURCE_ENV=$4
FAIL_LATE=$5
# select the fixture source environment
normal_release_source_env() { printf '%s\\n' "$SOURCE_ENV"; }
# isolate filesystem publication from locking
acquire_release_transaction_lock() { :; }
# model an absent dashboard override
require_no_dashboard_override() { :; }
# use only fixture-provided commands
require_command() { :; }
# retain one fixed active source release
read_release_state() { printf '%s\\n' 2026.10.09-2; }
# isolate publication from resource evidence
require_normal_release_resource_gate() { :; }
# isolate publication from secret fixtures
require_deployment_secrets() { :; }
# retain every fixture image
cleanup_obsolete_weather_images() { :; }
# isolate publication from host capacity
require_image_pull_capacity_floor() { :; }
# resolve deterministic pinned fixture images
resolve_arm64_image() {
  case "$1" in
    *weather-server*) printf 'ghcr.io/anstosa/weather-server@sha256:%064d\\n' 1 ;;
    *weather-web*) printf 'ghcr.io/anstosa/weather-web@sha256:%064d\\n' 2 ;;
    *) printf '%s\\n' "$1" ;;
  esac
}
# write one target environment for the transaction
write_release_env() {
  cat >"$2" <<ENV
WEATHER_RELEASE=$3
WEATHER_SERVER_IMAGE=$4
WEATHER_WEB_IMAGE=$5
POSTGRES_IMAGE=$6
CLOUDFLARED_IMAGE=$7
WEATHER_DATABASE_NAME=weather
WEATHER_POSTGRES_DIR=/var/lib/weather/postgres
WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=0
WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=0
WEATHER_CONTROL_PLANE_SHA256=$(printf '%064d' 9)
WEATHER_CONTROL_PLANE_VERSION=16
ENV
  chmod 600 "$2"
}
# model only config and pull operations
compose() {
  # emit the four rendered images only for the image listing call
  if [[ " $* " == *" config --images "* ]]; then
    env_value "$WEATHER_ENV_FILE" WEATHER_SERVER_IMAGE
    env_value "$WEATHER_ENV_FILE" WEATHER_WEB_IMAGE
    env_value "$WEATHER_ENV_FILE" POSTGRES_IMAGE
    env_value "$WEATHER_ENV_FILE" CLOUDFLARED_IMAGE
  fi
}
# emit one bounded proof artifact
require_normal_stage_image_proof() {
  printf '{"proofSha256":"%064d"}\\n' 7 >"$4"
}
# emit one phase-specific capacity pair
require_normal_stage_capacity() {
  printf '{"phase":"%s"}\\n' "$4" >"$5"
  printf '{"phase":"%s","receiptSha256":"%064d"}\\n' "$4" 8 >"$6"
}
# emit retained authorization and samples
verify_previous_image_compatibility() {
  printf 'authorization\\n' >"$3"
  printf '{"samples":[1,2]}\\n' >"$5"
}
# publish one fixture authorization
publish_migration_authorization() { mv "$1" "$2"; }
# embed the three phase receipts
write_normal_stage_qualification_receipt() {
  node --input-type=module - "$3" "$4" "$5" "$7" <<'NODE'
import { readFileSync, writeFileSync } from "node:fs";
const [prePath, postPath, cleanupPath, output] = process.argv.slice(2);
const value = {
  contractVersion: "normal-stage-qualification/v2",
  postCleanupReceipt: JSON.parse(readFileSync(cleanupPath)),
  postPullReceipt: JSON.parse(readFileSync(postPath)),
  prePullReceipt: JSON.parse(readFileSync(prePath)),
};
writeFileSync(output, \`\${JSON.stringify(value)}\\n\`);
NODE
}
# enforce publication order and metadata
read_normal_stage_qualification() {
  local release_name proof samples qualification path
  release_name=$(env_value "$2" WEATHER_RELEASE)
  proof="$releases_dir/$release_name.normal-stage-image-proof.json"
  samples="$releases_dir/$release_name.normal-stage-samples.json"
  qualification=$3
  # require all evidence aliases closed before the environment commit point
  # validate every published evidence file
  for path in "$proof" "$samples" "$qualification"; do
    [[ -f "$path" && ! -L "$path" && "$(stat -c '%h' "$path")" == 1 &&
      "$(tail -c 1 "$path" | od -An -tu1 | tr -d ' ')" == 10 ]] || return 1
  done
  [[ ! -e "$releases_dir/$release_name.env" ]] || return 1
  # expose the intentional failure only after all publication checks
  if [[ "$FAIL_LATE" == true ]]; then
    printf 'forced late qualification failure\\n' >&2
    return 1
  fi
}
main stage 2026.11.02-1
`;
  return spawnSync("bash", ["-c", harness, "stage-publication", updatePath, deploy,
    directory, sourceEnv, String(failLate)], { encoding: "utf8" });
}

test("normal stage publishes single-link evidence before committing the environment", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-stage-publication-"));
  const release = "2026.11.02-1";
  const releases = join(directory, "deploy/releases");
  try {
    const result = await runStagePublicationFixture(directory, false);
    assert.equal(result.status, 0, result.stderr);
    const proof = join(releases, `${release}.normal-stage-image-proof.json`);
    const samples = join(releases, `${release}.normal-stage-samples.json`);
    const qualification = join(releases, `${release}.normal-stage-receipt.json`);
    // validate every committed transaction artifact
    for (const path of [proof, samples, qualification, join(releases, `${release}.env`)]) {
      assert.equal((await stat(path)).nlink, 1);
      assert.ok((await readFile(path)).toString("utf8").endsWith("\n"));
    }
    const value = JSON.parse(await readFile(qualification, "utf8"));
    assert.equal(value.prePullReceipt.phase, "pre_pull");
    assert.equal(value.postPullReceipt.phase, "post_pull");
    assert.equal(value.postCleanupReceipt.phase, "post_cleanup");
    assert.equal(await readFile(join(directory, "unrelated"), "utf8"), "preserve\n");
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("normal stage late failure removes only its unpublished transaction", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-stage-publication-fail-"));
  const release = "2026.11.02-1";
  const releases = join(directory, "deploy/releases");
  try {
    const result = await runStagePublicationFixture(directory, true);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /forced late qualification failure/u);
    // require every transaction publication to be absent
    for (const suffix of ["normal-stage-image-proof.json", "normal-stage-samples.json",
      "normal-stage-receipt.json", "env", "migration-authorization"]) {
      await assert.rejects(stat(join(releases, `${release}.${suffix}`)), /ENOENT/u);
    }
    assert.equal(await readFile(join(directory, "unrelated"), "utf8"), "preserve\n");
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});
