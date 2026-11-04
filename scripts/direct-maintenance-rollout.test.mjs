import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "..");
const operator = join(repoRoot, "scripts/direct-maintenance-rollout.sh");

// run one sourced operator scenario
function runBash(body, argumentsList = [], environment = {}) {
  return spawnSync("bash", ["-c", `source "$1"\n${body}`, "direct-rollout-test", operator,
    ...argumentsList], {
    encoding: "utf8",
    env: { ...process.env, ...environment },
  });
}

test("operator keeps the direct migration narrow and does not fabricate rehearsal state", async () => {
  const source = await readFile(operator, "utf8");
  assert.match(source, /--atomic-direct-18-23-index-600s/u);
  assert.doesNotMatch(source, /--atomic-direct-18-23(?:\s|$)/u);
  assert.doesNotMatch(source, /normal-stage-receipt|historical-v14-18-21-rehearsal-v1/u);
  assert.doesNotMatch(source, /docker volume create|pg_basebackup|scripts\/backup\.sh/u);
  assert.match(source, /if \[\[ -z "\$ids" \]\]; then\s+return 0/u);
  assert.match(source, /direct_revised_23_sha=7e48acb14710a625/u);
  assert.match(source, /direct_original_backup_receipt_sha=c17966f78167775b/u);
  assert.match(source, /direct_retained_cap=20401094656/u);
  assert.match(source, /direct_peak_cap=23622320128/u);
});

test("private archive validation accepts an explicit bound above 64 KiB", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-direct-large-private-"));
  const archive = join(root, "archive.dump.age");
  await writeFile(archive, Buffer.alloc(128 * 1024, 1), { mode: 0o600 });
  const body = String.raw`
die() { printf 'error: %s\n' "$*" >&2; return 1; }
direct_require_private "$2" 200000
if direct_require_private "$2"; then
  printf 'default bound unexpectedly accepted archive\n' >&2
  exit 9
fi
`;
  try {
    const result = runBash(body, [archive]);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

test("activation stops and quiesces both writers before ledger classification", () => {
  const body = String.raw`
log=$2
releases_dir=/opt/weather/current/deploy/releases
direct_begin() { :; }
direct_validate_identity() { :; }
direct_validate_compatibility() { :; }
direct_require_source() { printf '/source.env\n'; }
direct_target_environment() { printf '/target.env\n'; }
direct_require_target() { :; }
migration_authorization() { printf '/authorization\n'; }
validate_migration_authorization() { :; }
direct_backup_receipt() { printf '/backup.json\n'; }
direct_state_root() { dirname "$log"; }
direct_validate_backup() { :; }
sha256sum() { printf '%064d  %s\n' 0 "$1"; }
direct_validate_activation() { :; }
require_normal_release_resource_gate() { :; }
direct_require_free() { :; }
require_deployment_secrets() { :; }
env_value() { printf 'weather\n'; }
require_v14_compatibility_bridge_migration_ledger() { :; }
migration_history_sha256() { printf '%064d\n' 1; }
capture_supported_writer_container() { printf '%s-id\n' "$2"; }
direct_journal() { printf 'journal:%s\n' "$1" >>"$log"; }
stop_supported_writer_container() { printf 'stop:%s\n' "$2" >>"$log"; }
require_supported_writers_quiesced() { printf 'quiesce\n' >>"$log"; }
start_postgres() { printf 'postgres\n' >>"$log"; }
direct_require_database_quiesced() { printf 'owner-quiesce\n' >>"$log"; }
direct_run_migration() { DIRECT_MIGRATION_EXIT=0; printf 'migration\n' >>"$log"; }
direct_classify() { printf 'classify\n' >>"$log"; printf 'target\n'; }
direct_finish_target() { printf 'finish\n' >>"$log"; }
direct_activate id "$(printf '%064d' 0)" /opt/weather/current/deploy/releases/2026.11.03-3.direct-compatibility.json auth "$(printf '%064d' 2)"
`;
  const root = spawnSync("mktemp", ["-d"], { encoding: "utf8" }).stdout.trim();
  const log = join(root, "order.log");
  try {
    const result = runBash(body, [log]);
    assert.equal(result.status, 0, result.stderr);
    const order = spawnSync("cat", [log], { encoding: "utf8" }).stdout.trim().split("\n");
    assert.deepEqual(order.slice(1, 5), ["stop:api", "stop:worker", "quiesce", "postgres"]);
    assert.ok(order.lastIndexOf("quiesce") < order.indexOf("owner-quiesce"));
    assert.ok(order.indexOf("owner-quiesce") < order.indexOf("migration"));
    assert.ok(order.indexOf("migration") < order.indexOf("classify"));
    assert.ok(order.indexOf("classify") < order.indexOf("finish"));
  } finally {
    spawnSync("rm", ["-rf", root]);
  }
});

test("an unknown ledger fails closed after quiescence", () => {
  const body = String.raw`
log=$2
releases_dir=/opt/weather/current/deploy/releases
direct_begin() { :; }
direct_validate_identity() { :; }
direct_validate_compatibility() { :; }
direct_require_source() { printf '/source.env\n'; }
direct_target_environment() { printf '/target.env\n'; }
direct_require_target() { :; }
migration_authorization() { printf '/authorization\n'; }
validate_migration_authorization() { :; }
direct_backup_receipt() { printf '/backup.json\n'; }
direct_state_root() { dirname "$log"; }
direct_validate_backup() { :; }
sha256sum() { printf '%064d  %s\n' 0 "$1"; }
direct_validate_activation() { :; }
require_normal_release_resource_gate() { :; }
direct_require_free() { :; }
require_deployment_secrets() { :; }
env_value() { printf 'weather\n'; }
require_v14_compatibility_bridge_migration_ledger() { :; }
migration_history_sha256() { printf '%064d\n' 1; }
capture_supported_writer_container() { printf '%s-id\n' "$2"; }
direct_journal() { printf 'journal:%s\n' "$1" >>"$log"; }
stop_supported_writer_container() { printf 'stop:%s\n' "$2" >>"$log"; }
require_supported_writers_quiesced() { printf 'quiesce\n' >>"$log"; }
start_postgres() { :; }
direct_require_database_quiesced() { printf 'owner-quiesce\n' >>"$log"; }
direct_run_migration() { DIRECT_MIGRATION_EXIT=1; printf 'migration\n' >>"$log"; }
direct_classify() { printf 'classify\n' >>"$log"; printf 'unknown\n'; }
direct_finish_target() { printf 'UNSAFE_FINISH\n' >>"$log"; }
direct_exit_recovery() { printf 'recovery-preserved\n' >>"$log"; }
die() { printf 'error: %s\n' "$*" >&2; return 1; }
direct_activate id "$(printf '%064d' 0)" /opt/weather/current/deploy/releases/2026.11.03-3.direct-compatibility.json auth "$(printf '%064d' 2)"
`;
  const root = spawnSync("mktemp", ["-d"], { encoding: "utf8" }).stdout.trim();
  const log = join(root, "unknown.log");
  try {
    const result = runBash(body, [log]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /ledger is unknown/u);
    const events = spawnSync("cat", [log], { encoding: "utf8" }).stdout;
    assert.match(events, /quiesce[\s\S]*migration[\s\S]*classify/u);
    assert.doesNotMatch(events, /UNSAFE_FINISH/u);
  } finally {
    spawnSync("rm", ["-rf", root]);
  }
});

test("a nonzero migration container cannot publish committed target markers", () => {
  const body = String.raw`
log=$2
releases_dir=/opt/weather/current/deploy/releases
direct_begin() { :; }
direct_validate_identity() { :; }
direct_validate_compatibility() { :; }
direct_require_source() { printf '/source.env\n'; }
direct_target_environment() { printf '/target.env\n'; }
direct_require_target() { :; }
migration_authorization() { printf '/authorization\n'; }
validate_migration_authorization() { :; }
direct_backup_receipt() { printf '/backup.json\n'; }
direct_state_root() { dirname "$log"; }
direct_validate_backup() { :; }
sha256sum() { printf '%064d  %s\n' 0 "$1"; }
direct_validate_activation() { :; }
require_normal_release_resource_gate() { :; }
direct_require_free() { :; }
require_deployment_secrets() { :; }
env_value() { printf 'weather\n'; }
require_v14_compatibility_bridge_migration_ledger() { :; }
migration_history_sha256() { printf '%064d\n' 1; }
capture_supported_writer_container() { printf '%s-id\n' "$2"; }
direct_journal() { printf 'journal:%s\n' "$1" >>"$log"; }
stop_supported_writer_container() { printf 'stop:%s\n' "$2" >>"$log"; }
require_supported_writers_quiesced() { printf 'quiesce\n' >>"$log"; }
start_postgres() { :; }
direct_require_database_quiesced() { printf 'owner-quiesce\n' >>"$log"; }
direct_run_migration() { DIRECT_MIGRATION_EXIT=9; printf 'migration:9\n' >>"$log"; }
direct_classify() { printf 'target\n'; }
direct_finish_target() { printf 'UNSAFE_TARGET_MARKERS\n' >>"$log"; }
direct_exit_recovery() { printf 'recovery-required\n' >>"$log"; }
die() { printf 'error: %s\n' "$*" >&2; return 1; }
direct_activate id "$(printf '%064d' 0)" /opt/weather/current/deploy/releases/2026.11.03-3.direct-compatibility.json auth "$(printf '%064d' 2)"
`;
  const root = spawnSync("mktemp", ["-d"], { encoding: "utf8" }).stdout.trim();
  const log = join(root, "nonzero.log");
  try {
    const result = runBash(body, [log]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /failed after committing/u);
    const events = spawnSync("cat", [log], { encoding: "utf8" }).stdout;
    assert.match(events, /migration:9/u);
    assert.match(events, /recovery-required/u);
    assert.doesNotMatch(events, /UNSAFE_TARGET_MARKERS/u);
  } finally {
    spawnSync("rm", ["-rf", root]);
  }
});

test("committed target recovery uses the proven pre-epoch runtime view", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-direct-pre-epoch-recovery-"));
  const log = join(root, "recovery.log");
  const body = String.raw`
log=$2
state_dir=$3
direct_target_env=/target.env
direct_source_env=/source.env
direct_source_history=$(printf '%064d' 1)
direct_compatibility_proof=/proof.json
DIRECT_MIGRATION_EXIT=9
direct_quiesce_migration() { printf 'quiesced\n' >>"$log"; }
start_postgres() { printf 'postgres\n' >>"$log"; }
require_supported_writers_quiesced() { printf 'writers-quiesced\n' >>"$log"; }
direct_require_database_quiesced() { printf 'database-quiesced\n' >>"$log"; }
direct_classify() { printf 'target\n'; }
direct_validate_compatibility() { printf 'compatibility:%s\n' "$1" >>"$log"; }
migration_authorization() { printf '/authorization\n'; }
validate_migration_authorization() { printf 'authorization:%s:%s:%s\n' "$1" "$2" "$3" >>"$log"; }
env_value() { printf 'weather\n'; }
apply_runtime_database_acl() { printf 'acl-apply:%s:%s\n' "$1" "$2" >>"$log"; }
verify_runtime_database_acl() { printf 'acl-verify:%s:%s\n' "$1" "$2" >>"$log"; }
with_restore_migration_authorization() {
  printf 'override:%s:%s:%s:%s:%s\n' "$1" "$2" "$3" "$4" "$5" >>"$log"
  shift 3
  "$@"
}
start_v14_compatibility_bridge_runtime() { printf 'pre-epoch-runtime:%s\n' "$1" >>"$log"; }
restore_images() { printf 'UNSAFE_ORDINARY_RESTORE\n' >>"$log"; return 1; }
direct_journal() { printf 'journal:%s\n' "$1" >>"$log"; }
write_private_state() { printf 'marker:%s:%s\n' "$(basename "$1")" "$2" >>"$log"; }
write_active_symlink() { printf 'marker:active:%s\n' "$1" >>"$log"; }
direct_recover_inner
`;
  try {
    const result = runBash(body, [log, root]);
    assert.equal(result.status, 0, result.stderr);
    const events = await readFile(log, "utf8");
    assert.match(events,
      /authorization:\/authorization:2026\.10\.09-2:2026\.11\.03-3/u);
    assert.match(events,
      /override:\/authorization:2026\.10\.09-2:2026\.11\.03-3:start_v14_compatibility_bridge_runtime:\/source\.env/u);
    assert.match(events, /acl-apply:\/target\.env:weather/u);
    assert.match(events, /pre-epoch-runtime:\/source\.env/u);
    assert.doesNotMatch(events, /UNSAFE_ORDINARY_RESTORE/u);
    await assert.rejects(readFile(join(root, "adjustment-capture-epoch.json")),
      { code: "ENOENT" });
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

test("activation never overwrites a preserved journal", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-direct-journal-test-"));
  const journal = join(root, "activation-journal.json");
  const original = "preserved-running-journal\n";
  await writeFile(journal, original, { mode: 0o600 });
  const body = String.raw`
root=$2
releases_dir=/opt/weather/current/deploy/releases
direct_begin() { :; }
direct_validate_identity() { :; }
direct_validate_compatibility() { :; }
direct_require_source() { printf '/source.env\n'; }
direct_target_environment() { printf '/target.env\n'; }
direct_require_target() { :; }
migration_authorization() { printf '/authorization\n'; }
validate_migration_authorization() { :; }
direct_backup_receipt() { printf '/backup.json\n'; }
direct_validate_backup() { :; }
direct_state_root() { printf '%s\n' "$root"; }
sha256sum() { printf '%064d  %s\n' 0 "$1"; }
direct_validate_activation() { :; }
require_normal_release_resource_gate() { :; }
direct_require_free() { :; }
require_deployment_secrets() { :; }
env_value() { printf 'weather\n'; }
require_v14_compatibility_bridge_migration_ledger() { :; }
migration_history_sha256() { printf '%064d\n' 1; }
direct_journal() { printf 'OVERWRITTEN\n' >"$root/activation-journal.json"; }
die() { printf 'error: %s\n' "$*" >&2; return 1; }
direct_activate id "$(printf '%064d' 0)" /opt/weather/current/deploy/releases/2026.11.03-3.direct-compatibility.json auth "$(printf '%064d' 2)"
`;
  try {
    const result = runBash(body, [root]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /journal already exists/u);
    assert.equal(await readFile(journal, "utf8"), original);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

test("recovery does not classify or restart when migration quiescence fails", () => {
  const body = String.raw`
log=$2
direct_target_env=/target.env
direct_source_env=/source.env
direct_source_history=$(printf '%064d' 1)
direct_quiesce_migration() { printf 'quiesce-failed\n' >>"$log"; return 1; }
start_postgres() { printf 'UNSAFE_POSTGRES\n' >>"$log"; }
direct_classify() { printf 'UNSAFE_CLASSIFY\n' >>"$log"; }
restore_images() { printf 'UNSAFE_RESTART\n' >>"$log"; }
direct_recover_inner
`;
  const root = spawnSync("mktemp", ["-d"], { encoding: "utf8" }).stdout.trim();
  const log = join(root, "recovery.log");
  try {
    const result = runBash(body, [log]);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /could not be quiesced/u);
    const events = spawnSync("cat", [log], { encoding: "utf8" }).stdout;
    assert.equal(events, "quiesce-failed\n");
  } finally {
    spawnSync("rm", ["-rf", root]);
  }
});

test("a verified backup adoption prevents a second backup invocation", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-direct-backup-test-"));
  const receipt = join(root, "backup-receipt.json");
  const invoked = join(root, "invoked");
  await writeFile(receipt, "{}\n", { mode: 0o600 });
  const body = String.raw`
fixture_receipt=$2
invoked=$3
direct_begin() { :; }
direct_validate_identity() { :; }
direct_require_source() { printf '/source.env\n'; }
direct_target_environment() { printf '/target.env\n'; }
direct_require_target() { :; }
direct_state_root() { dirname "$fixture_receipt"; }
direct_backup_receipt() { printf '%s\n' "$fixture_receipt"; }
direct_validate_backup() { printf 'validated\n'; }
direct_backup id "$(printf '%064d' 0)"
`;
  try {
    const result = runBash(body, [receipt, invoked]);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Reused verified backup adoption receipt/u);
    const probe = spawnSync("test", ["-e", invoked]);
    assert.notEqual(probe.status, 0);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

test("backup phase adopts the retained publication without invoking a dump", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-direct-backup-adoption-"));
  const state = join(root, "state");
  const originalRoot = join(state, "direct-maintenance-2026.11.03-2");
  const targetRoot = join(state, "direct-maintenance-2026.11.03-3");
  const original = join(originalRoot, "backup-receipt.json");
  const receipt = join(targetRoot, "backup-receipt.json");
  await Promise.all([
    mkdir(originalRoot, { recursive: true }),
    mkdir(targetRoot, { recursive: true }),
  ]);
  await writeFile(original, `${JSON.stringify({
    allocatedBytes: 842_956_957,
    archive: "/var/lib/weather/backups/weather-retained.dump.age",
    archiveBytes: 842_950_000,
    archiveSha256: "a".repeat(64),
    checksum: "/var/lib/weather/backups/weather-retained.dump.age.sha256",
  })}\n`, { mode: 0o600 });
  const body = String.raw`
state_dir=$2
target_root=$3
direct_begin() { :; }
direct_validate_identity() { :; }
direct_require_source() { printf '/source.env\n'; }
direct_target_environment() { printf '/target.env\n'; }
direct_require_target() { :; }
direct_state_root() { printf '%s\n' "$target_root"; }
direct_validate_original_backup() { :; }
require_normal_release_resource_gate() { :; }
direct_validate_backup() {
  node --input-type=module - "$1" <<'NODE'
import { readFileSync } from "node:fs";
const value = JSON.parse(readFileSync(process.argv[2], "utf8"));
if (value.contractVersion !== "weather-direct-maintenance-backup-adoption/v1" ||
  value.targetRelease !== "2026.11.03-3" || value.state !== "adopted") process.exit(9);
NODE
}
direct_backup id "$(printf '%064d' 9)"
`;
  try {
    const result = runBash(body, [state, targetRoot]);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /without a new publication/u);
    const adopted = JSON.parse(await readFile(receipt, "utf8"));
    assert.equal(adopted.originalBackupReceipt, original);
    assert.equal(adopted.originalIdentitySha256,
      "ee25fc43f34c4926ebd3bd577a00d783108f3caa2434094eaf7ecf5697c1f754");
    assert.equal(adopted.archive, "/var/lib/weather/backups/weather-retained.dump.age");
    assert.equal(adopted.archiveSha256, "a".repeat(64));
    assert.equal(adopted.allocatedBytes, 842_956_957);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

test("target health and authority precede the final release markers", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-direct-markers-test-"));
  const deploy = join(root, "deploy");
  const bin = join(root, "bin");
  const log = join(root, "order.log");
  await mkdir(join(deploy, "scripts"), { recursive: true });
  await mkdir(bin);
  await writeFile(join(deploy, "scripts/adjustment-evaluation-export.sh"),
    `#!/usr/bin/env bash\nprintf 'epoch:%s:%s\\n' "$WEATHER_ENV_FILE" "$2" >>${JSON.stringify(log)}\n`);
  await chmod(join(deploy, "scripts/adjustment-evaluation-export.sh"), 0o755);
  await writeFile(join(bin, "node"), `#!/usr/bin/env bash
if [[ "$*" == *inert-v14-settings-snapshot* ]]; then printf '1:1 %064d\\n' 1; fi
exit 0
`);
  await chmod(join(bin, "node"), 0o755);
  const body = String.raw`
log=$2
deploy_dir=$3
state_dir=$4
node() {
  if [[ "$*" == *inert-v14-settings-snapshot* ]]; then printf '1:1 %064d\n' 1; fi
}
env_value() {
  case "$2" in
    WEATHER_DATABASE_NAME) printf 'weather\n' ;;
    WEATHER_SERVER_IMAGE) printf 'server@sha256:%064d\n' 1 ;;
    WEATHER_WEB_IMAGE) printf 'web@sha256:%064d\n' 2 ;;
  esac
}
apply_runtime_database_acl() { printf 'acl-apply\n' >>"$log"; }
verify_runtime_database_acl() { printf 'acl-verify\n' >>"$log"; }
direct_initialize_capture_epoch() { printf 'epoch:%s:%s\n' "$2" "$direct_target_release" >>"$log"; }
direct_initialize_gauges() { printf 'gauges\n' >>"$log"; }
require_inert_v14_empty_catalog() { printf '{}\n' >"$state_dir/adjustment-candidate-catalog.json"; printf 'catalog\n' >>"$log"; }
direct_json() { printf '%040d\n' 3; }
start_exact_release() { printf 'health\n' >>"$log"; }
direct_classify() { printf 'target\n'; }
direct_journal() { printf 'journal:%s\n' "$1" >>"$log"; }
write_private_state() { printf 'marker:%s\n' "$(basename "$1")" >>"$log"; }
write_active_symlink() { printf 'marker:active.env\n' >>"$log"; }
DIRECT_MIGRATION_EXIT=0
direct_finish_target id target
`;
  try {
    const result = runBash(body, [log, deploy, root], { PATH: `${bin}:${process.env.PATH}` });
    assert.equal(result.status, 0, result.stderr);
    const events = (await readFile(log, "utf8")).trim().split("\n");
    assert.ok(events.includes("epoch:target:2026.11.03-3"));
    const health = events.indexOf("health");
    const journal = events.indexOf("journal:target_healthy");
    assert.ok(health >= 0 && journal > health);
    assert.deepEqual(events.slice(-4), [
      "marker:schema-release",
      "marker:previous-release",
      "marker:active.env",
      "marker:current-release",
    ]);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});
