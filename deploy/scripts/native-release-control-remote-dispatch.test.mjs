import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const dispatcher = fileURLToPath(
  new URL("./native-release-control-remote-dispatch.sh", import.meta.url),
);
const release = "2026.11.25-1";

// Source the actual dispatcher with only its external owner functions replaced.
function run(body, args = [], source = dispatcher) {
  return spawnSync(
    "bash",
    ["-c", `source "$1"\n${body}`, "native-release-control-test", source, ...args],
    { encoding: "utf8" },
  );
}

// Prove every original release-control verb is intercepted before legacy dispatch.
test("classifies the complete release-control surface", () => {
  const body = String.raw`
# verify every declared release-control verb
for action in rollback recover status preflight stage activate yolo dashboard-release dashboard-rollback dashboard-rollback-core; do
  native_release_control_handles "$action" || exit 20
done
native_release_control_handles forecast-training-export && exit 21
true
`;
  const result = run(body);
  assert.equal(result.status, 0, result.stderr);
});

// Prove fixed publication bytes drive retry-safe native staging and yolo activation.
test("stages and activates an independently installed fixed publication", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-native-publication-owner-"));
  const publicationRoot = join(root, "native-publications");
  const releaseRoot = join(publicationRoot, "by-release", release);
  const stageRoot = join(root, "stages");
  const log = join(root, "operations");
  const source = join(root, "native-release-control-remote-dispatch.sh");
  const publication = join(releaseRoot, "publication.json");
  try {
    await mkdir(releaseRoot, { recursive: true, mode: 0o700 });
    await mkdir(stageRoot, { mode: 0o700 });
    const bytes = Buffer.from('{"contractVersion":"fixture"}\n');
    const digest = createHash("sha256").update(bytes).digest("hex");
    await writeFile(publication, bytes, { mode: 0o600 });
    await writeFile(
      join(releaseRoot, "publication.sha256"),
      `${digest}  publication.json\n`,
      { mode: 0o600 },
    );
    const rewritten = (await readFile(dispatcher, "utf8")).replace(
      "/opt/weather/operators/native-publications",
      publicationRoot,
    );
    await writeFile(source, rewritten, { mode: 0o600 });
    const body = String.raw`
log=$2
sp_root=$3
expected_release=$4
active_release=2099.01.01-1
publication_contract=weather-schema-preserving-native-descendant-publication/v1
sp_module=/fixture/schema-preserving-release.mjs
# accept only the selected fixture release
validate_release() { [[ "$1" == "$expected_release" ]]; }
# keep fixed paths canonical in the disposable namespace
realpath() { printf '%s\n' "$3"; }
# project root-owned private directory metadata
stat() { printf '0:0:700\n'; }
# admit only fixture regular files
sp_private() { [[ -f "$1" && ! -L "$1" ]]; }
# hash exact installed fixture bytes
sp_sha() { sha256sum -- "$1" | awk '{print $1}'; }
# expose the fixed ordinary descendant contract
sp_field() { [[ "$2" == contractVersion ]] && printf '%s\n' "$publication_contract"; }
# record installed validator invocation
node() { printf 'validate:%s:%s:%s\n' "$2" "$3" "$4" >>"$log"; }
# retain the first exact staged publication
sp_stage() {
  printf 'stage:%s:%s:%s\n' "$1" "$2" "$3" >>"$log"
  mkdir -p "$sp_root/$1"
  cp -- "$2" "$sp_root/$1/publication.json"
  : >"$sp_root/$1/stage.json"
}
# record retained stage admission
sp_target() { printf 'target:%s\n' "$1" >>"$log"; }
# expose one current source environment
sp_source() { printf '/fixture/source.env\n'; }
# expose the selected active release
env_value() { printf '%s\n' "$active_release"; }
# record active-release retry health
sp_health() { printf 'health:%s\n' "$1" >>"$log"; }
# record exact native activation
sp_activate() { printf 'activate:%s\n' "$1" >>"$log"; }
native_release_control_dispatch stage "$expected_release"
native_release_control_dispatch stage "$expected_release"
native_release_control_dispatch yolo "$expected_release"
active_release=$expected_release
native_release_control_dispatch yolo "$expected_release"
publication_contract=weather-schema-preserving-publication/v2
native_release_control_dispatch stage "$expected_release"
[[ "$?" == 1 ]] || exit 41
`;
    const result = run(body, [log, stageRoot, release], source);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(
      result.stderr,
      "error: native release-control publication differs: ordinary descendant contract\n",
    );
    const events = await readFile(log, "utf8");
    assert.equal(events.match(/^stage:/gmu)?.length, 1, events);
    assert.equal(events.match(/^target:/gmu)?.length, 2, events);
    assert.equal(events.match(/^validate:/gmu)?.length, 3, events);
    assert.equal(events.match(/^activate:/gmu)?.length, 1, events);
    assert.match(events, /health:\/fixture\/source\.env/u);
    assert.doesNotMatch(events, /legacy/u);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// Prove concrete native controls preserve the exact auxiliary and host owners.
test("dispatches rollback recovery activation status and preflight to exact owners", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-native-release-control-"));
  const scripts = join(root, "scripts");
  const log = join(root, "operations");
  try {
    await mkdir(scripts);
    await writeFile(
      join(scripts, "status.sh"),
      `#!/usr/bin/env bash\nprintf 'status:%s\\n' "$#" >>"${log}"\n`,
    );
    await writeFile(
      join(scripts, "preflight-capacity.sh"),
      `#!/usr/bin/env bash\nprintf 'preflight:%s\\n' "$*" >>"${log}"\n`,
    );
    await Promise.all([
      chmod(join(scripts, "status.sh"), 0o755),
      chmod(join(scripts, "preflight-capacity.sh"), 0o755),
    ]);
    const body = String.raw`
deploy_dir=$2
log=$3
expected_release=$4
sp_rollback() { printf 'rollback\n' >>"$log"; }
sp_system_recover() { printf 'recover\n' >>"$log"; }
sp_source() { printf 'source\n' >>"$log"; printf '/fixture/release.env\n'; }
sp_activate() { printf 'activate:%s\n' "$1" >>"$log"; }
validate_release() { [[ "$1" == "$expected_release" ]]; }
native_release_control_dispatch rollback
native_release_control_dispatch recover
native_release_control_dispatch status
native_release_control_dispatch preflight
native_release_control_dispatch activate "$4"
`;
    const result = run(body, [root, log, release]);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(
      await readFile(log, "utf8"),
      "rollback\nrecover\nsource\nstatus:0\n" +
        "preflight:--sample-seconds 900 --json /var/lib/weather/preflight-v13-resource.json\n" +
        `activate:${release}\n`,
    );
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// Prove dashboard operations select only the native web-only controller.
test("dispatches dashboard mutations to the native controller", () => {
  const body = String.raw`
expected_release=$2
validate_release() { [[ "$1" == "$expected_release" ]]; }
native_dashboard_release() { printf 'release:%s:%s:%s\n' "$1" "$2" "$3"; }
native_dashboard_rollback() { printf 'rollback:%s\n' "$1"; }
native_release_control_dispatch dashboard-release "$2" "$2" "$2"
# check dashboard rollback operations
native_release_control_dispatch dashboard-rollback
native_release_control_dispatch dashboard-rollback-core
`;
  const result = run(body, [release]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    result.stdout,
    `release:${release}:${release}:${release}\nrollback:previous\nrollback:core\n`,
  );
  assert.equal(result.stderr, "");
});

// Prove malformed and unrelated calls remain distinguishable to the gateway.
test("retains invalid-argument and unrelated-operation return classes", () => {
  const body = String.raw`
sp_rollback() { printf 'rollback-called\n'; exit 90; }
native_release_control_dispatch activate invalid-release
printf 'invalid=%s\n' "$?"
native_release_control_dispatch rollback unexpected
printf 'rollback-invalid=%s\n' "$?"
native_release_control_dispatch unrelated
printf 'unrelated=%s\n' "$?"
`;
  const result = run(body);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, "invalid=2\nrollback-invalid=2\nunrelated=64\n");
  assert.match(result.stderr, /invalid native release-control arguments/u);
});
