import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, stat, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "../..");
const updateScript = join(repoRoot, "deploy/scripts/update.sh");

// run one isolated release function
function runBash(source, argumentsList = [], options = {}) {
  return spawnSync("bash", ["-c", source, "weather-update-test", updateScript, ...argumentsList], {
    cwd: repoRoot,
    encoding: "utf8",
    ...options,
  });
}

// enforce exact semantic tables plus the narrow bootstrap sequence allowance
test("preexisting data verifier accepts only monotonic bootstrap sequence drift", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-semantic-verifier-"));
  const contract = join(directory, "contract");
  const before = join(directory, "before");
  const after = join(directory, "after");
  const hash = "a".repeat(64);
  const tableHash = "b".repeat(64);
  const contractBody = `preexisting-data/v1\n` +
    `T\tsources\tid,updated_at\tid\t${hash}\tid\tsources_pkey\n` +
    `S\tsources_id_seq\t${hash}\ttrue\n` +
    `S\tweather_records_id_seq\t${hash}\tfalse\n`;
  const beforeBody = `preexisting-data-snapshot/v1\n` +
    `T\tsources\t1\t${tableHash}\n` +
    `S\tsources_id_seq\t${hash}\ttrue\t7\tt\n` +
    `S\tweather_records_id_seq\t${hash}\tfalse\t11\tt\n`;

  try {
    await writeFile(contract, contractBody);
    await writeFile(before, beforeBody);
    await writeFile(after, beforeBody.replace("\t7\tt\n", "\t12\tt\n"));
    const accepted = runBash(
      'source "$1"; verify_preexisting_data_snapshots "$2" "$3" "$4"',
      [contract, before, after],
    );
    assert.equal(accepted.status, 0, accepted.stderr);

    // reject table mutation, sequence reset and non-bootstrap advance independently
    const rejectedBodies = [
      beforeBody.replace(tableHash, "c".repeat(64)),
      beforeBody.replace("\t7\tt\n", "\t6\tt\n"),
      beforeBody.replace("\t11\tt\n", "\t12\tt\n"),
    ];
    for (const rejectedBody of rejectedBodies) {
      await writeFile(after, rejectedBody);
      const rejected = runBash(
        'source "$1"; verify_preexisting_data_snapshots "$2" "$3" "$4"',
        [contract, before, after],
      );
      assert.notEqual(rejected.status, 0);
    }
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// keep both forward proof paths on the same streaming semantic contract
test("normal and historical rehearsals use the preexisting semantic stream", async () => {
  const update = await readFile(updateScript, "utf8");
  const compatibility = update
    .split("verify_previous_image_compatibility() (")[1]
    .split("\n)\n\n# reconcile retained PostgreSQL")[0];
  const historical = update
    .split("historical_v14_18_21_rehearsal() (")[1]
    .split("\n)\n\n# require one measured full-size ARM64 proof")[0];

  for (const proof of [compatibility, historical]) {
    assert.match(proof, /write_preexisting_data_contract/u);
    assert.match(proof, /write_preexisting_data_snapshot/u);
    assert.match(proof, /verify_preexisting_data_snapshots/u);
  }
  assert.doesNotMatch(compatibility, /--data-only[\s\S]*sha256sum/u);
  assert.doesNotMatch(historical, /historical_application_data_sha256/u);
});

// reject partial or empty discovery output when the PostgreSQL producer fails
test("preexisting contract rejects failed discovery producers", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-semantic-discovery-"));
  const contract = join(directory, "contract");

  try {
    await writeFile(contract, "", { mode: 0o600 });
    const partialTables = runBash(
      `source "$1"
compose() { printf 'sites\n'; return 1; }
write_preexisting_data_contract ignored weather_test "$2"`,
      [contract],
    );
    assert.notEqual(partialTables.status, 0);
    assert.match(partialTables.stderr, /table discovery failed/u);

    const failedSequences = runBash(
      `source "$1"
compose() {
  if [[ "$*" == *"relkind IN ('r','p')"* ]]; then printf 'sites\n'; return 0; fi
  if [[ "$*" == *"SELECT attribute.attname"* ]]; then printf 'id\nupdated_at\n'; return 0; fi
  if [[ "$*" == *"SELECT attribute.attnum"* ]]; then printf 'metadata\n'; return 0; fi
  if [[ "$*" == *"FROM pg_index primary_index"* ]]; then printf 'id\tsites_pkey\n'; return 0; fi
  if [[ "$*" == *"relation.relkind = 'S'"* ]]; then return 1; fi
  return 64
}
write_preexisting_data_contract ignored weather_test "$2"`,
      [contract],
    );
    assert.notEqual(failedSequences.status, 0);
    assert.match(failedSequences.stderr, /sequence discovery failed/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// propagate a failed COPY producer even when the streaming hash consumer reaches EOF
test("preexisting snapshot rejects a partial failed row stream", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-semantic-stream-"));
  const contract = join(directory, "contract");
  const snapshot = join(directory, "snapshot");
  const metadata = "1|id|bigint|t|||\n";
  const metadataSha256 = createHash("sha256").update(metadata).digest("hex");

  try {
    await writeFile(
      contract,
      `preexisting-data/v1\nT\tsites\tid\tid\t${metadataSha256}\tid\tsites_pkey\n`,
      { mode: 0o600 },
    );
    await writeFile(snapshot, "", { mode: 0o600 });
    const result = runBash(
      `source "$1"
compose() {
  if [[ "$*" == *"SELECT attribute.attnum"* ]]; then printf '1|id|bigint|t|||\n'; return 0; fi
  if [[ "$*" == *"FROM pg_index primary_index"* ]]; then printf 'id\tsites_pkey\n'; return 0; fi
  if [[ "$*" == *"EXPLAIN (COSTS OFF)"* ]]; then printf 'Index Scan using sites_pkey on sites\n'; return 0; fi
  if [[ "$*" == *"COPY (SELECT"* ]]; then printf '[1]\n'; return 1; fi
  return 64
}
write_preexisting_data_snapshot ignored weather_test "$2" "$3"`,
      [contract, snapshot],
    );
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /row stream failed/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// retain bounded private diagnostics before failed qualification cleanup
test("release qualification failures publish private evidence packets", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-release-failure-"));
  const sample = join(directory, "samples");

  try {
    await writeFile(sample, "0 1 2 3\n", { mode: 0o600 });
    const result = runBash(
      'source "$1"; state_dir="$2"; retain_release_failure_evidence normal-stage 1 "$3"',
      [directory, sample],
    );
    assert.equal(result.status, 0, result.stderr);
    const names = await readdir(directory);
    const packet = names.find((name) => name.startsWith("normal-stage-failure-"));
    assert.ok(packet);
    assert.equal((await stat(join(directory, packet))).mode & 0o777, 0o700);
    assert.equal((await stat(join(directory, packet, "manifest"))).mode & 0o777, 0o600);
    assert.equal((await stat(join(directory, packet, "samples"))).mode & 0o777, 0o600);
    assert.equal(await readFile(join(directory, packet, "samples"), "utf8"), "0 1 2 3\n");
    const update = await readFile(updateScript, "utf8");
    assert.match(update, /retain_release_failure_evidence normal-stage/u);
    assert.match(update, /retain_release_failure_evidence historical-v14-rehearsal/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify locale-independent control identity
test("control-plane digest is locale independent", () => {
  const command = 'source "$1"; control_plane_digest';
  const cLocale = spawnSync("bash", ["-c", command, "weather-update-test", updateScript], {
    cwd: repoRoot,
    encoding: "utf8",
    env: { ...process.env, LC_ALL: "C" },
  });
  const forwardedLocale = spawnSync("bash", ["-c", command, "weather-update-test", updateScript], {
    cwd: repoRoot,
    encoding: "utf8",
    env: { ...process.env, LC_ALL: "en_US.UTF-8" },
  });
  assert.equal(cLocale.status, 0, cLocale.stderr);
  assert.equal(forwardedLocale.status, 0, forwardedLocale.stderr);
  assert.equal(forwardedLocale.stdout.trim(), cLocale.stdout.trim());
});

// make release markers durable before callers cross their commit boundary
test("private state and active symlink sync their final directory entries", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-durable-release-state-"));
  const state = join(directory, "state");
  const releases = join(directory, "releases");
  const transcript = join(directory, "transcript");
  try {
    await mkdir(state);
    await mkdir(releases);
    await writeFile(join(releases, "2026.10.09-2.env"), "fixture\n");
    const result = runBash(
      `source "$1"
deploy_dir=$2
transcript=$3
sync() { printf 'sync:%s\\n' "$*" >>"$transcript"; }
mv() { printf 'mv:%s\\n' "$*" >>"$transcript"; command mv "$@"; }
rmdir() { printf 'rmdir:%s\\n' "$*" >>"$transcript"; command rmdir "$@"; }
write_private_state "$deploy_dir/state/schema-release" 2026.10.09-2
write_active_symlink 2026.10.09-2`,
      [directory, transcript],
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(await readFile(join(state, "schema-release"), "utf8"), "2026.10.09-2\n");
    assert.equal((await readFile(join(state, "active.env"), "utf8")), "fixture\n");
    const operations = await readFile(transcript, "utf8");
    assert.match(operations,
      /sync:-f .*schema-release\.[^\n]+\nmv:.*schema-release\.[^\n]+ .*schema-release\nsync:-f .*\/state\n/u);
    assert.match(operations,
      /mv:-Tf .*\/active\.env .*\/state\/active\.env\nsync:-f .*\/state\nrmdir:.*\.active\.env\.[^\n]+\nsync:-f .*\/state\n/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// preserve the one-operand owner bridge dispatch after main shifts the action
test("metadata custody finalization forwards one exact proof identity", () => {
  const proofSha256 = "a".repeat(64);
  const accepted = runBash(
    `source "$1"
finalize_adjustment_shadow_metadata_custody_v1() { printf '%s\\n' "$1"; }
main adjustment-shadow-metadata-custody-finalize-v1 "$2"`,
    [proofSha256],
  );
  assert.equal(accepted.status, 0, accepted.stderr);
  assert.equal(accepted.stdout, `${proofSha256}\n`);

  // reject missing or surplus proof operands before the owner bridge
  for (const argumentsList of [[], [proofSha256, proofSha256]]) {
    const rejected = runBash(
      `source "$1"
finalize_adjustment_shadow_metadata_custody_v1() { printf 'unexpected\\n'; }
main adjustment-shadow-metadata-custody-finalize-v1 "\${@:2}"`,
      argumentsList,
    );
    assert.notEqual(rejected.status, 0);
    assert.match(rejected.stderr, /requires one identity/u);
    assert.equal(rejected.stdout, "");
  }
});

// preserve foreign transaction-root metadata instead of repairing it
test("family transaction root rejects existing ownership and mode drift", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-family-state-root-"));
  const root = join(directory, "adjustment-release-state");

  try {
    await mkdir(root, { mode: 0o755 });
    const before = await stat(root);
    const result = runBash(
      'source "$1"; ensure_adjustment_family_private_state_root "$2"',
      [root],
    );
    const after = await stat(root);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /adjustment family transaction root is unsafe/u);
    assert.deepEqual(
      { gid: after.gid, mode: after.mode & 0o777, uid: after.uid },
      { gid: before.gid, mode: before.mode & 0o777, uid: before.uid },
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify retained credential handoff
test("restore recreates PostgreSQL before starting runtime images", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-postgres-restore-"));
  const transcript = join(directory, "transcript");

  try {
    const result = runBash(
      `source "$1"
transcript=$2
compose() { printf '%s:%s\n' "$WEATHER_ENV_FILE" "$*" >>"$transcript"; }
prepare_xweather_usage_directory() { :; }
env_value() {
  if [[ "$2" == WEATHER_DATABASE_NAME ]]; then printf 'weather_test\n'; else awk -F= -v key="$2" '$1 == key { print substr($0, length(key) + 2) }' "$1"; fi
}
apply_runtime_database_acl() { :; }
verify_runtime_database_acl() { :; }
restore_images /releases/current.env`,
      [transcript],
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(
      await readFile(transcript, "utf8"),
      "/releases/current.env:up -d --no-deps --force-recreate --wait postgres\n" +
        "/releases/current.env:up -d --no-deps --wait api worker web cloudflared\n",
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify private immutable authorization state
test("migration authorization is private exact and release-bound", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-migration-authorization-"));
  const releases = join(directory, "releases");
  const stagedAuthorization = join(releases, ".2026.08.22-2.authorization.partial");
  const replacementAuthorization = join(releases, ".2026.08.22-2.replacement.partial");
  const authorization = join(releases, "2026.08.22-2.migration-authorization");

  try {
    await mkdir(releases);
    const result = runBash(
      `source "$1"
releases_dir=$2
write_migration_authorization "$3" "2026.08.22-1" "2026.08.22-2" "${"a".repeat(64)}"
publish_migration_authorization "$3" "$4"
validate_migration_authorization "$4" "2026.08.22-1" "2026.08.22-2"`,
      [releases, stagedAuthorization, authorization],
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal((await stat(authorization)).mode & 0o777, 0o600);
    assert.equal(
      await readFile(authorization, "utf8"),
      `WEATHER_MIGRATION_AUTHORIZATION_VERSION=1\n` +
        `WEATHER_MIGRATION_AUTHORIZATION_RELEASE=2026.08.22-1\n` +
        `WEATHER_MIGRATION_AUTHORIZATION_SCHEMA_RELEASE=2026.08.22-2\n` +
        `WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256=${"a".repeat(64)}\n`,
    );
    const replacement = runBash(
      `source "$1"
releases_dir=$2
write_migration_authorization "$3" "2026.08.22-1" "2026.08.22-2" "${"b".repeat(64)}"
publish_migration_authorization "$3" "$4"`,
      [releases, replacementAuthorization, authorization],
    );
    assert.notEqual(replacement.status, 0);
    assert.match(replacement.stderr, /migration authorization already exists/u);
    assert.match(await readFile(authorization, "utf8"), new RegExp(`${"a".repeat(64)}\\n$`, "u"));
    assert.match(
      await readFile(replacementAuthorization, "utf8"),
      new RegExp(`${"b".repeat(64)}\\n$`, "u"),
    );
    await writeFile(authorization, `${await readFile(authorization, "utf8")}UNKNOWN=value\n`, {
      mode: 0o600,
    });
    const rejected = runBash(
      'source "$1"; releases_dir="$2"; validate_migration_authorization "$3" "2026.08.22-1" "2026.08.22-2"',
      [releases, authorization],
    );
    assert.notEqual(rejected.status, 0);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("restore paths replace hostile inherited authorization with exact persisted values", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-authorized-restore-"));
  const releases = join(directory, "releases");
  const authorization = join(releases, "2026.08.22-2.migration-authorization");
  const v14Authorization = join(releases, "2026.10.09-3.migration-authorization");
  const transcript = join(directory, "transcript");

  try {
    await mkdir(releases);
    const result = runBash(
      `source "$1"
releases_dir=$2
transcript=$3
write_migration_authorization "$4" "2026.08.22-1" "2026.08.22-2" "${"a".repeat(64)}"
write_migration_authorization "$5" "2026.10.09-2" "2026.10.09-3" "${"b".repeat(64)}"
start_postgres() { :; }
compose() { printf '%s|%s\n' "\${WEATHER_MIGRATION_AUTHORIZATION_RELEASE-unset}" "\${WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256-unset}" >>"$transcript"; }
start_v14_compatibility_bridge_runtime() { printf '%s|%s\n' "\${WEATHER_MIGRATION_AUTHORIZATION_RELEASE-unset}" "\${WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256-unset}" >>"$transcript"; }
prepare_xweather_usage_directory() { :; }
env_value() {
  if [[ "$2" == WEATHER_DATABASE_NAME ]]; then printf 'weather_test\n'; else awk -F= -v key="$2" '$1 == key { print substr($0, length(key) + 2) }' "$1"; fi
}
apply_runtime_database_acl() { :; }
verify_runtime_database_acl() { :; }
export WEATHER_MIGRATION_AUTHORIZATION_RELEASE=untrusted
export WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256=untrusted
restore_images /releases/previous.env 2026.08.22-1 2026.08.22-2
export WEATHER_MIGRATION_AUTHORIZATION_RELEASE=untrusted
export WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256=untrusted
restore_images /releases/current.env 2026.08.22-2 2026.08.22-2
export WEATHER_MIGRATION_AUTHORIZATION_RELEASE=untrusted
export WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256=untrusted
restore_v14_compatibility_bridge_images /releases/bridge.env 2026.10.09-2 2026.10.09-2
export WEATHER_MIGRATION_AUTHORIZATION_RELEASE=untrusted
export WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256=untrusted
restore_v14_compatibility_bridge_images /releases/bridge.env 2026.10.09-2 2026.10.09-3`,
      [releases, transcript, authorization, v14Authorization],
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(
      await readFile(transcript, "utf8"),
      `2026.08.22-1|${"a".repeat(64)}\nunset|unset\nunset|unset\n` +
        `2026.10.09-2|${"b".repeat(64)}\n`,
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("rollback switches images and commits state without forward operations", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-rollback-"));
  const transcript = join(directory, "transcript");

  try {
    const result = runBash(
      `source "$1"
state_dir=/unused
transcript=$2
read_release_state() { [[ "$1" == */current-release ]] && printf '2026.08.22-2\\n' || printf '2026.08.22-1\\n'; }
read_optional_release_state() { printf '2026.08.22-2\\n'; }
release_env() { printf '/releases/%s.env\\n' "$1"; }
validate_release_env() { :; }
require_deployment_secrets() { :; }
require_control_plane_compatibility() { :; }
restore_images() { printf 'restore:%s:%s:%s\\n' "$1" "$2" "$3" >>"$transcript"; }
record_release_success() { printf 'record:%s:%s\\n' "$1" "$2" >>"$transcript"; }
rollback_release`,
      [transcript],
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(
      await readFile(transcript, "utf8"),
      "restore:/releases/2026.08.22-1.env:2026.08.22-1:2026.08.22-2\n" +
        "record:2026.08.22-1:2026.08.22-2\n",
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("failed rollback restores current images without committing state", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-rollback-failure-"));
  const transcript = join(directory, "transcript");

  try {
    const result = runBash(
      `source "$1"
state_dir=/unused
transcript=$2
attempt=0
read_release_state() { [[ "$1" == */current-release ]] && printf '2026.08.22-2\\n' || printf '2026.08.22-1\\n'; }
read_optional_release_state() { printf '2026.08.22-2\\n'; }
release_env() { printf '/releases/%s.env\\n' "$1"; }
validate_release_env() { :; }
require_deployment_secrets() { :; }
require_control_plane_compatibility() { :; }
restore_images() { attempt=$((attempt + 1)); printf 'restore:%s:%s:%s\\n' "$1" "$2" "$3" >>"$transcript"; ((attempt > 1)); }
record_release_success() { printf 'unexpected-record\\n' >>"$transcript"; }
rollback_release`,
      [transcript],
    );
    assert.notEqual(result.status, 0);
    assert.equal(
      await readFile(transcript, "utf8"),
      "restore:/releases/2026.08.22-1.env:2026.08.22-1:2026.08.22-2\n" +
        "restore:/releases/2026.08.22-2.env:2026.08.22-2:2026.08.22-2\n",
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("failed activation quiesces its owned migrator before exact-ledger recovery", () => {
  const update = readFileSync(join(repoRoot, "deploy/scripts/update.sh"), "utf8");
  const activation = update.split("start_release() (")[1].split("\n)\n\n# roll back images")[0];
  const schemaIntent = activation.indexOf('write_private_state "$state_dir/schema-release" "$target"');
  const migration = activation.indexOf("compose run --detach");
  const failedHealth = activation.indexOf("if ! start_exact_release");
  const quiesce = activation.indexOf('quiesce_normal_activation_migration "$activation_env"');
  const classify = activation.indexOf("restore_normal_activation_after_quiescence");
  assert.equal(schemaIntent < migration, true);
  assert.equal(migration < failedHealth, true);
  assert.equal(quiesce < classify, true);
  assert.match(activation, /write_normal_activation_journal[\s\S]*prepared[\s\S]*running/u);
});

test("normal activation recovery classifies only exact source or target ledgers", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-normal-ledger-recovery-"));
  const transcript = join(directory, "transcript");
  const sourceLedger = "adb2aade2b019eb0e7a4df5b5bda38017ecbe143613df79cd873f67659068c93";
  const targetLedger = "3f9a9dbccbd89c39b9f06c4a82dcdfdb535e3b6e7477ca02859774a17fc2dc51";
  const run = (ledger) => runBash(
    `source "$1"
state_dir=$2
transcript=$3
env_value() { printf 'weather_test\\n'; }
migration_history_sha256() { printf '%s\\n' "$HISTORY"; }
migration_authorization() { printf '/authorization/%s\\n' "$1"; }
validate_migration_authorization() { printf 'authorize:%s:%s\\n' "$2" "$3" >>"$transcript"; }
write_private_state() { printf 'marker:%s\\n' "$2" >>"$transcript"; }
restore_images() { printf 'restore:%s:%s\\n' "$2" "$3" >>"$transcript"; }
restore_normal_activation_after_quiescence /source.env 2026.10.29-1 2026.10.29-1 2026.10.29-2`,
    [directory, transcript],
    { env: { ...process.env, HISTORY: ledger } },
  );
  try {
    // cover both failure classes that must retain the complete source ledger
    for (const scenario of ["pre-sql", "transactional"]) {
      await writeFile(transcript, "");
      const result = run(sourceLedger);
      assert.equal(result.status, 0, `${scenario}: ${result.stderr}`);
      assert.equal(await readFile(transcript, "utf8"),
        "marker:2026.10.29-1\nrestore:2026.10.29-1:2026.10.29-1\n");
    }
    // cover both post-commit failure classes that must retain target authority
    for (const scenario of ["post-ledger-bootstrap", "acl"]) {
      await writeFile(transcript, "");
      const result = run(targetLedger);
      assert.equal(result.status, 0, `${scenario}: ${result.stderr}`);
      assert.equal(await readFile(transcript, "utf8"),
        "authorize:2026.10.29-1:2026.10.29-2\n" +
        "marker:2026.10.29-2\nrestore:2026.10.29-1:2026.10.29-2\n");
    }
    await writeFile(transcript, "");
    const rejected = run("f".repeat(64));
    assert.notEqual(rejected.status, 0);
    assert.equal(await readFile(transcript, "utf8"), "");
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("historical rehearsal admission uses the measured scan envelope instead of mutable size equality", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-historical-envelope-"));
  const state = join(directory, "state");
  const postgres = join(directory, "postgres");
  const source = join(directory, "source.env");
  const target = join(directory, "target.env");
  const receiptPath = join(state, "receipt.json");
  const samplesPath = join(state, "samples.json");
  const canonical = (value) => Array.isArray(value)
    ? `[${value.map(canonical).join(",")}]`
    : value !== null && typeof value === "object"
      ? `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`
      : JSON.stringify(value);
  try {
    await mkdir(join(postgres, "base/16384"), { recursive: true });
    await mkdir(state);
    await writeFile(join(postgres, "base/16384/1"), "fixture");
    const sourceServer = `ghcr.io/example/server@sha256:${"1".repeat(64)}`;
    const sourceWeb = `ghcr.io/example/web@sha256:${"2".repeat(64)}`;
    const targetServer = `ghcr.io/example/server@sha256:${"3".repeat(64)}`;
    const targetWeb = `ghcr.io/example/web@sha256:${"4".repeat(64)}`;
    await writeFile(source, [
      "WEATHER_DATABASE_NAME=weather",
      `WEATHER_POSTGRES_DIR=${postgres}`,
      `WEATHER_SERVER_IMAGE=${sourceServer}`,
      `WEATHER_WEB_IMAGE=${sourceWeb}`,
      "",
    ].join("\n"));
    await writeFile(target, [
      "WEATHER_DATABASE_NAME=weather",
      `WEATHER_POSTGRES_DIR=${postgres}`,
      `WEATHER_SERVER_IMAGE=${targetServer}`,
      `WEATHER_WEB_IMAGE=${targetWeb}`,
      "",
    ].join("\n"));
    const retainedBody = {
      contractVersion: "historical-v14-capacity-samples/v1",
      samples: [
        { capturedAtMilliseconds: Date.now() - 2_000, freeBytes: 5_000_000_000, freeInodes: 100_000, sequence: 0, tempBytes: 0 },
        { capturedAtMilliseconds: Date.now() - 1_000, freeBytes: 4_900_000_000, freeInodes: 99_000, sequence: 1, tempBytes: 1_000 },
      ],
    };
    const retained = { ...retainedBody,
      receiptSha256: createHash("sha256").update(canonical(retainedBody)).digest("hex") };
    const retainedBytes = Buffer.from(canonical(retained));
    await writeFile(samplesPath, retainedBytes, { mode: 0o600 });
    const requiredFreeBytes = 2_030_043_136 + 4_112_384 + 100_000_000 +
      10_000_000 + 2 * 100_000_000 + 268_435_456;
    const body = {
      appDataEqual: true,
      architecture: "aarch64",
      candidateAppDataSha256: "a".repeat(64),
      candidateScanHeapBytes: 1_000,
      candidateScanRowCount: 100,
      capacitySampleCount: 2,
      capacitySamplesReceiptSha256: retained.receiptSha256,
      capacitySamplesSha256: createHash("sha256").update(retainedBytes).digest("hex"),
      capturedAt: new Date().toISOString(),
      cleanupConfirmed: true,
      contractVersion: "historical-v14-18-21-rehearsal/v1",
      databaseBytes: 100_000_000,
      databaseOid: 16_384,
      durationMilliseconds: 100_000,
      freeBytesBefore: 5_000_000_000,
      freeInodesBefore: 100_000,
      largestSourceIndexBytes: 10_000_000,
      lockTimeoutMilliseconds: 10_000,
      maxWalBytes: 100_000_000,
      memoryLimitBytes: 512 * 1024 * 1024,
      minimumFreeBytesObserved: 4_900_000_000,
      minimumFreeInodesObserved: 99_000,
      peakGrowthBytes: 100_000_000,
      peakGrowthInodes: 1_000,
      peakTempBytes: 1_000,
      peakWalBytes: 100_000,
      requiredFreeBytes,
      requiredFreeInodes: 32_768 + 1 + 4_096,
      sourceDatabaseFileCount: 1,
      sourceEnvSha256: createHash("sha256").update(await readFile(source)).digest("hex"),
      sourceLedgerSha256: "e4a73765078fba8235bf94753c799c209d59f737dde6f38d1a1a458e3164a82e",
      sourceRelease: "2026.10.09-2",
      sourceServerImage: sourceServer,
      sourceWebImage: sourceWeb,
      statementTimeoutMilliseconds: 300_000,
      systemIdentifier: "123456789",
      targetEnvSha256: createHash("sha256").update(await readFile(target)).digest("hex"),
      targetLedgerSha256: "adb2aade2b019eb0e7a4df5b5bda38017ecbe143613df79cd873f67659068c93",
      targetRelease: "2026.10.09-3",
      targetServerImage: targetServer,
      targetWebImage: targetWeb,
      weatherRecordBytes: 99_000_000,
      weatherRecordCount: 90,
    };
    await writeFile(receiptPath, canonical({ ...body,
      receiptSha256: createHash("sha256").update(canonical(body)).digest("hex"),
    }), { mode: 0o600 });
    const run = (liveRows, liveHeapBytes) => runBash(
      `source "$1"
state_dir=$2
compose() { printf '123456789 16384 110000000 91 100000000 %s %s 10000000 100000000\\n' "$LIVE_ROWS" "$LIVE_HEAP_BYTES"; }
migration_history_sha256() { printf 'e4a73765078fba8235bf94753c799c209d59f737dde6f38d1a1a458e3164a82e\\n'; }
require_historical_v14_rehearsal_receipt "$3" "$4" "$5" "$6"`,
      [state, source, target, receiptPath, samplesPath],
      { env: { ...process.env, LIVE_ROWS: String(liveRows), LIVE_HEAP_BYTES: String(liveHeapBytes) } },
    );
    // 100s measured duration admits at most 1.5x the candidate scan workload
    const exact = run(150, 1_500);
    assert.equal(exact.status, 0, exact.stderr);
    for (const [rows, heapBytes] of [[151, 1_500], [150, 1_501]]) {
      const rejected = run(rows, heapBytes);
      assert.notEqual(rejected.status, 0);
      assert.match(rejected.stderr, /differs from the live handoff/u);
    }
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("recovery restores the active runtime against retained schema state", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-recovery-authorization-"));
  const transcript = join(directory, "transcript");

  try {
    await mkdir(join(directory, "state"));
    const result = runBash(
      `source "$1"
state_dir=$2/state
transcript=$3
read_release_state() { printf '2026.08.22-1\n'; }
read_optional_release_state() { printf '2026.08.22-2\n'; }
release_env() { printf '/releases/%s.env\n' "$1"; }
validate_release_env() { :; }
require_control_plane_compatibility() { :; }
require_command() { :; }
require_deployment_secrets() { :; }
restore_images() { printf 'restore:%s:%s:%s\n' "$1" "$2" "$3" >>"$transcript"; }
write_active_symlink() { printf 'active:%s\n' "$1" >>"$transcript"; }
main recover`,
      [directory, transcript],
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(
      await readFile(transcript, "utf8"),
      "restore:/releases/2026.08.22-1.env:2026.08.22-1:2026.08.22-2\n" +
        "active:2026.08.22-1\n",
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("crash recovery quiesces the journaled migrator before ledger classification", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-normal-crash-recovery-"));
  const state = join(directory, "state");
  const transcript = join(directory, "transcript");
  try {
    await mkdir(state);
    await writeFile(join(state, "normal-activation-v1.env"), "journal\n", { mode: 0o600 });
    const result = runBash(
      `source "$1"
state_dir=$2
normal_activation_journal=$2/normal-activation-v1.env
dashboard_release_journal=$2/dashboard.env
v14_compatibility_bridge_state=$2/v14.env
transcript=$3
acquire_release_transaction_lock() { :; }
require_command() { :; }
dashboard_override_present() { return 1; }
read_release_state() { printf '2026.10.29-1\\n'; }
read_optional_release_state() { printf '2026.10.29-2\\n'; }
read_normal_activation_journal() { printf '2026.10.29-1 2026.10.29-2 migration absent prepared\\n'; }
release_env() { printf '/releases/%s.env\\n' "$1"; }
validate_release_env() { :; }
require_control_plane_compatibility() { :; }
require_deployment_secrets() { :; }
quiesce_normal_activation_migration() { printf 'quiesce\\n' >>"$transcript"; }
start_postgres() { printf 'postgres\\n' >>"$transcript"; }
restore_normal_activation_after_quiescence() { printf 'classify:%s:%s:%s\\n' "$2" "$3" "$4" >>"$transcript"; }
write_active_symlink() { printf 'active:%s\\n' "$1" >>"$transcript"; }
sync() { :; }
main recover`,
      [state, transcript],
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(await readFile(transcript, "utf8"),
      "quiesce\npostgres\nclassify:2026.10.29-1:2026.10.29-1:2026.10.29-2\n" +
      "active:2026.10.29-1\n");
    await assert.rejects(readFile(join(state, "normal-activation-v1.env")));
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("normal schema-22 recovery precedes the retained v14 bridge classifier", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-normal-v14-receipt-recovery-"));
  const state = join(directory, "state");
  const releases = join(directory, "releases");
  const transcript = join(directory, "transcript");
  const target = "2026.10.29-2";
  try {
    await mkdir(state);
    await mkdir(releases);
    await writeFile(join(state, "v14-compatibility-bridge-v1"), "retained\n");
    await writeFile(join(releases, `${target}.normal-stage-receipt.json`), "{}", { mode: 0o600 });
    const result = runBash(
      `source "$1"
state_dir=$2
releases_dir=$3
normal_activation_journal=$2/normal-activation-v1.env
dashboard_release_journal=$2/dashboard.env
v14_compatibility_bridge_state=$2/v14-compatibility-bridge-v1
transcript=$4
acquire_release_transaction_lock() { :; }
require_command() { :; }
dashboard_override_present() { return 1; }
read_release_state() { printf '2026.10.29-2\\n'; }
read_optional_release_state() { printf '2026.10.29-2\\n'; }
require_deployment_secrets() { :; }
recover_committed_normal_release() { printf 'normal:%s:%s\\n' "$1" "$2" >>"$transcript"; }
require_v14_compatibility_bridge_receipt() { printf 'unexpected-v14\\n' >>"$transcript"; return 1; }
main recover`,
      [state, releases, transcript],
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(await readFile(transcript, "utf8"), `normal:${target}:${target}\n`);
    assert.equal(await readFile(join(state, "v14-compatibility-bridge-v1"), "utf8"), "retained\n");
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("capacity monitor completion rejects early death, nonzero exit, and sequence gaps", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-capacity-monitor-"));
  const harness = `source "$1"
samples=$2
completion=$3
mode=$4
case "$mode" in
  success|nonzero|gap)
    if [[ "$mode" == gap ]]; then
      printf '0 1 100 100\\n2 2 90 90\\n' >"$samples"
      next=2
    else
      printf '0 1 100 100\\n' >"$samples"
      next=1
    fi
    (
      trap 'printf "%s %s\\n" "$BASHPID" "$next" >"$completion"; if [[ "$mode" == nonzero ]]; then exit 7; else exit 0; fi' TERM
      while :; do sleep 0.1; done
    ) &
    monitor=$!
    sleep 0.2
    ;;
  early)
    printf '0 1 100 100\\n' >"$samples"
    (exit 0) &
    monitor=$!
    sleep 0.1
    ;;
esac
if stop_owned_capacity_monitor "$monitor" "$completion" "$samples"; then
  printf 'accepted:%s\\n' "$capacity_monitor_next_sequence"
else
  printf 'rejected\\n'
fi`;
  try {
    for (const [mode, expected] of [
      ["success", "accepted:1\n"],
      ["early", "rejected\n"],
      ["nonzero", "rejected\n"],
      ["gap", "rejected\n"],
    ]) {
      const samples = join(directory, `${mode}.samples`);
      const completion = join(directory, `${mode}.completion`);
      const result = runBash(harness, [samples, completion, mode]);
      assert.equal(result.status, 0, `${mode}: ${result.stderr}`);
      assert.equal(result.stdout, expected, mode);
    }
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// prove stale activation preserves retained recovery state
test("stale target activation rejects before mutation", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-reactivation-authorization-"));
  const scripts = join(directory, "scripts");
  const state = join(directory, "state");
  const transcript = join(directory, "transcript");
  const error = join(directory, "error");

  try {
    await mkdir(scripts, { recursive: true });
    await mkdir(state, { recursive: true });
    await writeFile(
      join(scripts, "backup.sh"),
      '#!/usr/bin/env bash\nprintf \'backup\\n\' >>"$WEATHER_TEST_TRANSCRIPT"\n',
    );
    await chmod(join(scripts, "backup.sh"), 0o700);
    await writeFile(join(state, "current-release"), "2026.08.22-1\n", { mode: 0o600 });
    await writeFile(join(state, "schema-release"), "2026.08.22-3\n", { mode: 0o600 });
    const result = runBash(
      `source "$1"
deploy_dir=$2
state_dir=$3
transcript=$4
error=$5
export WEATHER_TEST_TRANSCRIPT=$transcript
# resolve fixture release paths
release_env() { printf '/releases/%s.env\\n' "$1"; }
# accept fixture release metadata
validate_release_env() { :; }
# accept fixture control planes
require_control_plane_compatibility() { :; }
# record fixture capacity checks
require_capacity_gate() { printf 'capacity\\n' >>"$transcript"; }
# record fixture secret checks
require_deployment_secrets() { printf 'secrets\\n' >>"$transcript"; }
# accept fixture command checks
require_command() { :; }
# record fixture database starts
start_postgres() { printf 'postgres\\n' >>"$transcript"; }
# record fixture compose calls
compose() { printf 'compose:%s\\n' "$*" >>"$transcript"; }
# publish fixture state
write_private_state() { printf 'state:%s:%s\\n' "$1" "$2" >>"$transcript"; printf '%s\\n' "$2" >"$1"; }
# record fixture release starts
start_exact_release() { printf 'start:%s\\n' "$1" >>"$transcript"; }
# record fixture release commits
record_release_success() { printf 'record:%s:%s\\n' "$1" "$2" >>"$transcript"; }
# resolve fixture authorizations
migration_authorization() { printf 'authorization:%s\\n' "$1" >>"$transcript"; printf '/releases/%s.migration-authorization\\n' "$1"; }
# record fixture authorization checks
validate_migration_authorization() { printf 'validate-authorization:%s:%s:%s\\n' "$1" "$2" "$3" >>"$transcript"; }
# record fixture image restores
restore_images() { printf 'restore:%s:%s:%s\\n' "$1" "$2" "$3" >>"$transcript"; }
# record fixture active links
write_active_symlink() { printf 'active:%s\\n' "$1" >>"$transcript"; }
# require fail-closed stale activation
if (main activate 2026.08.22-2) 2>"$error"; then
  exit 64
fi
main recover`,
      [directory, state, transcript, error],
    );
    assert.equal(result.status, 0, result.stderr);
    assert.match(
      await readFile(error, "utf8"),
      /normal activation requires one exact forward staged release/u,
    );
    assert.equal(await readFile(join(state, "schema-release"), "utf8"), "2026.08.22-3\n");
    assert.equal(
      await readFile(transcript, "utf8"),
      "secrets\n" +
        "restore:/releases/2026.08.22-1.env:2026.08.22-1:2026.08.22-3\n" +
        "active:2026.08.22-1\n",
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("persistent authorization is written only after API and worker compatibility", async () => {
  const update = await readFile(updateScript, "utf8");
  const compatibility = update
    .split("verify_previous_image_compatibility() (")[1]
    .split("\n)\n\n# reconcile retained PostgreSQL")[0];
  const previousHistory = compatibility.indexOf(
    'previous_history_sha256=$(migration_history_sha256',
  );
  const currentHistory = compatibility.indexOf(
    'history_sha256=$(migration_history_sha256',
  );
  const authorizedWorker = compatibility.indexOf(
    "worker node apps/worker/dist/worker.js --once",
  );
  const workerGate = compatibility.indexOf("previous worker compatibility failed");
  const schemaChangeGate = compatibility.indexOf(
    'if [[ "$migrations_changed" == true ]]',
    workerGate,
  );
  const unprovenWorkerHealth = compatibility.indexOf(
    "worker node apps/worker/dist/health.js",
  );
  const workerRejection = compatibility.indexOf(
    "previous worker accepted unproven migration history",
  );
  const apiRejection = compatibility.indexOf(
    "previous API accepted invalid migration authorization",
  );
  const apiGate = compatibility.indexOf("previous API compatibility failed");
  const publication = compatibility.lastIndexOf("write_migration_authorization");

  assert.equal(previousHistory >= 0, true);
  assert.equal(currentHistory > previousHistory, true);
  assert.equal(authorizedWorker > currentHistory, true);
  assert.equal(workerGate > authorizedWorker, true);
  assert.equal(schemaChangeGate > workerGate, true);
  assert.equal(unprovenWorkerHealth > schemaChangeGate, true);
  assert.equal(workerRejection > unprovenWorkerHealth, true);
  assert.equal(apiRejection > workerGate, true);
  assert.equal(apiGate > apiRejection, true);
  assert.equal(publication > apiGate, true);
  const stage = update.split("  stage)\n")[1].split("  activate)\n")[0];
  assert.match(stage, /published_authorization/u);
  assert.match(stage, /trap 'rm -f[^']*published_authorization/u);
});

test("inert v14 provisions fixed rain target sources before producer activation", async () => {
  const update = await readFile(updateScript, "utf8");
  const activation = update
    .split("inert_v14_release() (")[1]
    .split("\n)\n\n# activate one forward release")[0];
  const epoch = activation.indexOf("--revision-capture-epoch-init-v1");
  const sources = activation.indexOf(
    "initialize_adjustment_rain_fixed_gauge_target_sources_v1 \"$target\"",
  );
  const start = activation.indexOf("start_exact_release \"$target\"");

  assert.equal(epoch >= 0, true);
  assert.equal(sources > epoch, true);
  assert.equal(start > sources, true);
  assert.match(update, /adjustment-rain-fixed-gauge-target-sources-initialize-v1\)/u);
  assert.match(update, /rain target source initialization takes no arguments/u);
});

// pin the backward-compatible migration boundary
test("compatibility clone removes private evidence before replaying migrations through 0021", async () => {
  const update = await readFile(updateScript, "utf8");
  const compatibility = update
    .split("verify_previous_image_compatibility() (")[1]
    .split("\n)\n\n# reconcile retained PostgreSQL administrator authority")[0];
  assert.match(compatibility, /DROP TABLE IF EXISTS adjustment_revision_frontier_v1/u);
  assert.match(compatibility, /DROP FUNCTION IF EXISTS weather_append_adjustment_shadow_v4/u);
  assert.match(compatibility, /ALTER TABLE weather_records DROP COLUMN IF EXISTS adjustment_revision_receipt/u);
  assert.match(compatibility, /0020_adjustment_revision_frontier\.sql/u);
  assert.match(compatibility, /0021_adjustment_rolling_registration\.sql/u);
  assert.match(compatibility, /compatibility database absence check failed/u);
  assert.match(compatibility, /compatibility task name already exists/u);
  assert.match(compatibility,
    /require_owned_compatibility_task_container[\s\S]*Config\.Labels\.com\.docker\.compose\.project[\s\S]*Config\.Image/u);
  assert.ok(compatibility.indexOf("ALTER TABLE ecmwf_temperature_canary_runs DROP COLUMN") <
    compatibility.indexOf("DROP FUNCTION IF EXISTS weather_adjustment_revision_receipt_valid_v1"));
  const maintenanceTablesDrop = compatibility.indexOf("DROP TABLE IF EXISTS adjustment_shadow_predictions_v2");
  const maintenanceFunctionDrop = compatibility.indexOf("DROP FUNCTION IF EXISTS weather_guard_adjustment_shadow_registration_v2()");
  const exportManifestDrop = compatibility.indexOf("DROP VIEW IF EXISTS adjustment_evaluation_export_manifest_v1");
  const exportRowsDrop = compatibility.indexOf("DROP VIEW IF EXISTS adjustment_evaluation_export_rows_v1");
  const viewDrop = compatibility.indexOf("DROP VIEW IF EXISTS rain_collection_status_v1");
  const modelDrop = compatibility.indexOf("DROP TABLE IF EXISTS rain_adjustment_runs");
  const modelGuardDrop = compatibility.indexOf("DROP FUNCTION IF EXISTS weather_guard_rain_adjustment_run()");
  const receiptDrop = compatibility.indexOf("DROP TABLE IF EXISTS rain_capture_receipts");
  const claimDrop = compatibility.indexOf("DROP TABLE IF EXISTS rain_capture_claims");
  const claimGuardDrop = compatibility.indexOf("DROP FUNCTION IF EXISTS weather_guard_rain_capture_claim()");
  const receiptGuardDrop = compatibility.indexOf("DROP FUNCTION IF EXISTS weather_guard_rain_capture_receipt()");
  const mutationGuardDrop = compatibility.indexOf("DROP FUNCTION IF EXISTS weather_reject_rain_capture_mutation()");
  const ledgerReset = compatibility.indexOf("'0017_adjustment_evaluation_export.sql', '0018_adjustment_maintenance_v2.sql', '0019_adjustment_maintenance_recurring.sql', '0020_adjustment_revision_frontier.sql', '0021_adjustment_rolling_registration.sql')");
  const replay = compatibility.indexOf(
    'run_owned_compatibility_container "$target_env" "${candidate}_migration" migration --no-deps',
  );
  const replayProof = compatibility.indexOf("rain_collection_migration_count=$(WEATHER_ENV_FILE=");
  const stationReplayProof = compatibility.indexOf("station_access_migration_count=$(WEATHER_ENV_FILE=");
  const modelReplayProof = compatibility.indexOf("rain_adjustment_migration_count=$(WEATHER_ENV_FILE=");
  const exportReplayProof = compatibility.indexOf("adjustment_export_migration_count=$(WEATHER_ENV_FILE=");
  const maintenanceReplayProof = compatibility.indexOf("adjustment_maintenance_migration_count=$(WEATHER_ENV_FILE=");

  assert.equal(maintenanceTablesDrop >= 0 && maintenanceTablesDrop < maintenanceFunctionDrop, true);
  assert.equal(maintenanceFunctionDrop < exportManifestDrop, true);
  assert.equal(exportManifestDrop >= 0 && exportManifestDrop < exportRowsDrop, true);
  assert.equal(exportRowsDrop < viewDrop, true);
  assert.equal(viewDrop >= 0 && viewDrop < modelDrop, true);
  assert.equal(modelDrop < modelGuardDrop && modelGuardDrop < receiptDrop, true);
  assert.equal(receiptDrop < claimDrop, true);
  assert.equal(claimDrop < claimGuardDrop && claimGuardDrop < receiptGuardDrop, true);
  assert.equal(receiptGuardDrop < mutationGuardDrop, true);
  assert.equal(mutationGuardDrop < ledgerReset && ledgerReset < replay, true);
  assert.equal(replay < replayProof, true);
  assert.equal(replayProof < stationReplayProof, true);
  assert.equal(stationReplayProof < modelReplayProof, true);
  assert.equal(modelReplayProof < exportReplayProof, true);
  assert.equal(exportReplayProof < maintenanceReplayProof, true);
  assert.match(compatibility, /baseline_schema_state" == "8:::::::"/u);
  assert.match(compatibility, /rain collection migration is missing from the compatibility database/u);
  assert.match(compatibility, /rain station access migration is missing from the compatibility database/u);
  assert.match(compatibility, /rain adjustment migration is missing from the compatibility database/u);
  assert.match(compatibility, /adjustment evaluation export migration is missing from the compatibility database/u);
  assert.match(compatibility, /adjustment maintenance migration is missing from the compatibility database/u);
});

// verify current strictness and the exact retained legacy format
test("release environment validator requires exact current control metadata", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-release-env-"));
  const valid = join(directory, "valid.env");
  const digest = `sha256:${"a".repeat(64)}`;
  const content = [
    "WEATHER_RELEASE=2026.08.22-1",
    `WEATHER_SERVER_IMAGE=registry.example/weather-server@${digest}`,
    `WEATHER_WEB_IMAGE=registry.example/weather-web@${digest}`,
    `POSTGRES_IMAGE=postgres@${digest}`,
    `CLOUDFLARED_IMAGE=cloudflare/cloudflared@${digest}`,
    "WEATHER_DATABASE_NAME=weather",
    "WEATHER_POSTGRES_DIR=/var/lib/weather/postgres",
    "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=1",
    "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=1",
    `WEATHER_CONTROL_PLANE_SHA256=${"b".repeat(64)}`,
    "WEATHER_CONTROL_PLANE_VERSION=2",
  ].join("\n");

  try {
    await writeFile(valid, `${content}\n`, { mode: 0o600 });
    const accepted = runBash('source "$1"; validate_release_env "$2" "2026.08.22-1"', [valid]);
    assert.equal(accepted.status, 0, accepted.stderr);
    const allowlistedLegacy = content
      .split("\n")
      .filter(
        // retain legacy fields
        (line) =>
          !line.startsWith("WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=") &&
          !line.startsWith("WEATHER_CONTROL_PLANE_"),
      )
      .concat(
        "WEATHER_CONTROL_PLANE_SHA256=c4d74581b84505e065fdec63447dfdded1d14221e459777a88e37729275f33b5",
        "WEATHER_CONTROL_PLANE_VERSION=6",
      )
      .join("\n");
    await writeFile(valid, `${allowlistedLegacy}\n`, { mode: 0o600 });
    const acceptedLegacy = runBash(
      'source "$1"; validate_release_env "$2" "2026.08.22-1"',
      [valid],
    );
    assert.equal(acceptedLegacy.status, 0, acceptedLegacy.stderr);
    const separatedLegacy = allowlistedLegacy.replace(
      "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=1",
      "\nWEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=1",
    );
    await writeFile(valid, `${separatedLegacy}\n`, { mode: 0o600 });
    const acceptedSeparator = runBash(
      'source "$1"; validate_release_env "$2" "2026.08.22-1"',
      [valid],
    );
    assert.equal(acceptedSeparator.status, 0, acceptedSeparator.stderr);
    assert.equal(await readFile(valid, "utf8"), `${separatedLegacy}\n`);

    // reject all broader legacy parsing exceptions
    for (const rejectedContent of [
      `${content}\n\n`,
      `${separatedLegacy.replace(/c4d745/u, "04d745")}\n`,
      `${separatedLegacy}\n \n`,
      `${separatedLegacy}\n# comment\n`,
      `${separatedLegacy}\nUNKNOWN_STATE=value\n`,
      `${separatedLegacy}\nWEATHER_RELEASE=2026.08.22-1\n`,
    ]) {
      await writeFile(valid, rejectedContent, { mode: 0o600 });
      const rejectedSeparator = runBash(
        'source "$1"; validate_release_env "$2" "2026.08.22-1"',
        [valid],
      );
      assert.notEqual(rejectedSeparator.status, 0);
    }
    await writeFile(valid, `${allowlistedLegacy.replace(/c4d745/u, "04d745")}\n`, {
      mode: 0o600,
    });
    const rejectedUnlistedLegacy = runBash(
      'source "$1"; validate_release_env "$2" "2026.08.22-1"',
      [valid],
    );
    assert.notEqual(rejectedUnlistedLegacy.status, 0);
    // reject missing control metadata
    const legacyContent = content
      .split("\n")
      .filter((line) => !line.startsWith("WEATHER_CONTROL_PLANE_"))
      .join("\n");
    await writeFile(valid, `${legacyContent}\n`, { mode: 0o600 });
    const rejectedLegacy = runBash('source "$1"; validate_release_env "$2" "2026.08.22-1"', [valid]);
    assert.notEqual(rejectedLegacy.status, 0);
    await writeFile(valid, `${content}\nWEATHER_RELEASE=2026.08.22-1\n`, { mode: 0o600 });
    const duplicate = runBash('source "$1"; validate_release_env "$2" "2026.08.22-1"', [valid]);
    assert.notEqual(duplicate.status, 0);
    await writeFile(valid, `${content}\nUNKNOWN_STATE=value\n`, { mode: 0o600 });
    const unknown = runBash('source "$1"; validate_release_env "$2" "2026.08.22-1"', [valid]);
    assert.notEqual(unknown.status, 0);
    await writeFile(
      valid,
      `${content.replace("WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=1", "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=true")}\n`,
      { mode: 0o600 },
    );
    const invalidKillSwitch = runBash(
      'source "$1"; validate_release_env "$2" "2026.08.22-1"',
      [valid],
    );
    assert.notEqual(invalidKillSwitch.status, 0);
    assert.match(invalidKillSwitch.stderr, /kill switch must be 0 or 1/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify kill-switch release persistence
test("release environment writer persists or safely defaults the wind canary kill switch", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-release-wind-canary-"));
  const source = join(directory, "source.env");
  const target = join(directory, "target.env");
  const digest = `sha256:${"a".repeat(64)}`;
  const sourceContent = [
    "WEATHER_DATABASE_NAME=weather",
    "WEATHER_POSTGRES_DIR=/var/lib/weather/postgres",
  ].join("\n");

  try {
    await writeFile(
      source,
      `${sourceContent}\nWEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=1\nWEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=0\n`,
    );
    const persisted = runBash(
      'source "$1"; write_release_env "$2" "$3" "2026.08.22-1" "$4" "$5" "$6" "$7"',
      [
        source,
        target,
        `registry.example/weather-server@${digest}`,
        `registry.example/weather-web@${digest}`,
        `postgres@${digest}`,
        `cloudflare/cloudflared@${digest}`,
      ],
    );
    assert.equal(persisted.status, 0, persisted.stderr);
    assert.match(
      await readFile(target, "utf8"),
      /^WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=1$/mu,
    );
    assert.match(
      await readFile(target, "utf8"),
      /^WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=0$/mu,
    );

    await writeFile(source, `${sourceContent}\n`);
    const defaulted = runBash(
      'source "$1"; write_release_env "$2" "$3" "2026.08.22-1" "$4" "$5" "$6" "$7"',
      [
        source,
        target,
        `registry.example/weather-server@${digest}`,
        `registry.example/weather-web@${digest}`,
        `postgres@${digest}`,
        `cloudflare/cloudflared@${digest}`,
      ],
    );
    assert.equal(defaulted.status, 0, defaulted.stderr);
    assert.match(
      await readFile(target, "utf8"),
      /^WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=0$/mu,
    );
    assert.match(
      await readFile(target, "utf8"),
      /^WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=1$/mu,
    );

    await writeFile(
      source,
      `${sourceContent}\nWEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=disabled\n`,
    );
    const malformed = runBash(
      'source "$1"; write_release_env "$2" "$3" "2026.08.22-1" "$4" "$5" "$6" "$7"',
      [
        source,
        target,
        `registry.example/weather-server@${digest}`,
        `registry.example/weather-web@${digest}`,
        `postgres@${digest}`,
        `cloudflare/cloudflared@${digest}`,
      ],
    );
    assert.notEqual(malformed.status, 0);
    assert.match(malformed.stderr, /kill switch must be 0 or 1/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("deployment secret validation rejects shared database credentials", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-secret-separation-"));
  const secrets = join(directory, "secrets");

  try {
    await mkdir(secrets);
    const values = {
      cloudflare_tunnel_token: "tunnel",
      weather_api_password: "api",
      weather_migration_owner_password: "owner",
      weather_postgres_admin_password: "owner",
      weather_postgres_api_password: "api",
      weather_postgres_ingest_password: "ingest",
      weather_postgres_owner_password: "owner",
      weather_postgres_training_export_password: "api",
      weather_tempest_api_key: "tempest",
      weather_worker_ingest_password: "ingest",
      weather_xweather_client_id: "xweather-id",
      weather_xweather_client_secret: "xweather-secret",
    };

    // provision every required source
    await Promise.all(
      Object.entries(values).map(([name, value]) => writeFile(join(secrets, name), `${value}\n`)),
    );
    const result = runBash(
      'source "$1"; deploy_dir="$2"; require_secret_source() { :; }; require_deployment_secrets',
      [directory],
    );
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /administrator and owner passwords must differ/u);
    await writeFile(join(secrets, "weather_postgres_admin_password"), "admin\n");
    const sharedExport = runBash(
      'source "$1"; deploy_dir="$2"; require_secret_source() { :; }; require_deployment_secrets',
      [directory],
    );
    assert.notEqual(sharedExport.status, 0);
    assert.match(
      sharedExport.stderr,
      /training export password must differ from every runtime password/u,
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("release environment validator rejects non-canonical PostgreSQL paths", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-release-path-"));
  const candidate = join(directory, "candidate.env");
  const digest = `sha256:${"a".repeat(64)}`;
  const controlPlane = "b".repeat(64);

  try {
    // reject every alias and escape
    for (const postgresDirectory of [
      "/var/lib/weather/postgres/../escape",
      "/var/lib/weather//postgres",
      "/var/lib/weather/postgres/.",
      "/var/lib/weather-postgres",
    ]) {
      const content = [
        "WEATHER_RELEASE=2026.08.22-1",
        `WEATHER_SERVER_IMAGE=registry.example/weather-server@${digest}`,
        `WEATHER_WEB_IMAGE=registry.example/weather-web@${digest}`,
        `POSTGRES_IMAGE=postgres@${digest}`,
        `CLOUDFLARED_IMAGE=cloudflare/cloudflared@${digest}`,
        "WEATHER_DATABASE_NAME=weather",
        `WEATHER_POSTGRES_DIR=${postgresDirectory}`,
        `WEATHER_CONTROL_PLANE_SHA256=${controlPlane}`,
        "WEATHER_CONTROL_PLANE_VERSION=2",
      ].join("\n");
      await writeFile(candidate, `${content}\n`, { mode: 0o600 });
      const result = runBash('source "$1"; validate_release_env "$2" "2026.08.22-1"', [candidate]);
      assert.notEqual(result.status, 0, postgresDirectory);
    }
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("secret validation rejects a symlink outside the Weather secret root", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-secret-path-"));
  const deployDirectory = join(directory, "deploy");
  const outside = join(directory, "outside");
  const secret = join(outside, "weather_api_password");

  try {
    await mkdir(deployDirectory);
    await mkdir(outside);
    await writeFile(secret, "secret\n");
    await chmod(secret, 0o400);
    await symlink(outside, join(deployDirectory, "secrets"));
    const result = runBash(
      'source "$1"; deploy_dir="$2"; require_secret_source "$3" "$(id -u)" "$(id -g)"',
      [deployDirectory, join(deployDirectory, "secrets/weather_api_password")],
    );
    assert.notEqual(result.status, 0);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

test("normal activation cleanup removes private evidence after exact-ledger restoration", () => {
  const update = readFileSync(join(repoRoot, "deploy/scripts/update.sh"), "utf8");
  const activation = update.split("start_release() (")[1].split("\n)\n\n# roll back images")[0];
  const cleanup = activation.split("cleanup_normal_activation() {")[1].split("\n  }")[0];
  assert.match(cleanup, /rm -f -- "\$pre_capacity_input" "\$pre_capacity_receipt"/u);
  assert.match(cleanup, /quiesce_normal_activation_migration[\s\S]*restore_normal_activation_after_quiescence/u);
  assert.match(cleanup, /rm -f -- "\$normal_activation_journal"/u);
  assert.match(activation, /--max-output-bytes "\$backup_ceiling"/u);
  assert.equal(
    activation.lastIndexOf('write_private_state "$state_dir/schema-release" "$target"') <
      activation.indexOf("compose run --detach"),
    true,
  );
});

test("release operations accept only current v15 and the exact reviewed predecessor", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-control-plane-"));
  const release = join(directory, "release.env");

  try {
    const digest = runBash('source "$1"; control_plane_digest').stdout.trim();
    await writeFile(
      release,
      [
        "WEATHER_RELEASE=2026.10.08-1",
        `WEATHER_CONTROL_PLANE_SHA256=${digest}`,
        "WEATHER_CONTROL_PLANE_VERSION=15",
        "",
      ].join("\n"),
    );
    const currentAccepted = runBash(
      'source "$1"; require_control_plane_compatibility "$2"',
      [release],
    );
    assert.equal(currentAccepted.status, 0, currentAccepted.stderr);

    await writeFile(
      release,
      [
        "WEATHER_RELEASE=2026.10.09-1",
        "WEATHER_CONTROL_PLANE_SHA256=603eb8f488ba78be3d7ecf76b0d587346d1c36432255d390d86d2b768c8fecba",
        "WEATHER_CONTROL_PLANE_VERSION=13",
        "",
      ].join("\n"),
    );
    const predecessorAccepted = runBash(
      'source "$1"; require_control_plane_compatibility "$2"',
      [release],
    );
    assert.equal(predecessorAccepted.status, 0, predecessorAccepted.stderr);

    const rejectedIdentities = [
      [
        "2026.10.07-2",
        "16d871c7aebb3a34097af219fd5c76a93b3ff1be3af521643dbf3a4d2041c61d",
        "12",
      ],
      [
        "2026.10.07-3",
        "5685ead4440468ecdb58402fbe14e76ce63ed03a018b9b4724fa24ffef4eb3c8",
        "12",
      ],
      [
        "2026.10.07-3",
        "ae098ea591868b9f0d093815fa94d05082af96ece7e9f6fd62e1d903bb8179eb",
        "11",
      ],
      ["2026.10.08-1", digest, "12"],
      ["2026.10.08-1", "a".repeat(64), "13"],
    ];

    // reject every relabelled or obsolete identity
    for (const [candidateRelease, candidateDigest, candidateVersion] of rejectedIdentities) {
      await writeFile(
        release,
        [
          `WEATHER_RELEASE=${candidateRelease}`,
          `WEATHER_CONTROL_PLANE_SHA256=${candidateDigest}`,
          `WEATHER_CONTROL_PLANE_VERSION=${candidateVersion}`,
          "",
        ].join("\n"),
      );
      const rejected = runBash(
        'source "$1"; require_control_plane_compatibility "$2"',
        [release],
      );
      assert.notEqual(rejected.status, 0);
      assert.match(
        rejected.stderr,
        /unsupported without an exact versioned allowlisted handoff/u,
      );
    }

    await writeFile(release, "WEATHER_CONTROL_PLANE_VERSION=15\n");
    const incomplete = runBash(
      'source "$1"; require_control_plane_compatibility "$2"',
      [release],
    );
    assert.notEqual(incomplete.status, 0);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// prove stage rejects before mutation
test("stage rejects caller-selected predecessor arguments before mutation", () => {
  const update = join(repoRoot, "deploy/scripts/update.sh");
  const result = runBash(
    'source "$1"; acquire_release_transaction_lock() { :; }; main stage 2026.08.22-2 --from /tmp/source.env',
    [],
  );
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /stage requires exactly one release/u);
  const stage = readFileSync(update, "utf8").split("  stage)\n")[1].split("  activate)\n")[0];
  assert.doesNotMatch(stage, /--from|retained_maintenance_source_env/u);
  assert.match(stage, /source_env=\$\(normal_release_source_env\)/u);
});

test("activation gates exact target and live source identities before mutation", () => {
  const update = readFileSync(join(repoRoot, "deploy/scripts/update.sh"), "utf8");
  const activation = update.split("start_release() (")[1].split("\n)\n\n# roll back images")[0];
  const targetGate = activation.indexOf('require_control_plane_compatibility "$activation_env"');
  const sourceGate = activation.indexOf("current_env=$(normal_release_source_env)");
  const stageReceipt = activation.indexOf("read_normal_stage_qualification");
  const firstMutation = activation.lastIndexOf('start_postgres "$current_env"');
  assert.equal(targetGate < sourceGate && sourceGate < stageReceipt, true);
  assert.equal(stageReceipt < firstMutation, true);
  assert.doesNotMatch(activation.slice(stageReceipt, firstMutation), /compose run --detach/u);
});

// prove rollback and recovery gate every release
test("rollback and recovery reject unsupported control planes before mutation", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-rollback-control-gate-"));

  try {
    await mkdir(join(directory, "state"));
    // reject either rollback boundary
    for (const rejectedRelease of ["2026.08.22-2", "2026.08.22-1"]) {
      const transcript = join(directory, `${rejectedRelease}.transcript`);
      const result = runBash(
        `source "$1"
state_dir=$2/state
transcript=$3
rejected_release=$4
read_release_state() {
  case "$1" in
    */current-release) printf '2026.08.22-2\n' ;;
    */previous-release) printf '2026.08.22-1\n' ;;
  esac
}
release_env() { printf '/releases/%s.env\n' "$1"; }
validate_release_env() { :; }
require_control_plane_compatibility() {
  printf 'gate:%s\n' "$1" >>"$transcript"
  [[ "$1" != *"$rejected_release.env" ]]
}
require_deployment_secrets() { printf 'secrets\n' >>"$transcript"; }
restore_images() { printf 'restore:%s\n' "$1" >>"$transcript"; }
record_release_success() { printf 'record:%s\n' "$1" >>"$transcript"; }
rollback_release`,
        [directory, transcript, rejectedRelease],
      );
      assert.notEqual(result.status, 0, rejectedRelease);
      const output = await readFile(transcript, "utf8");
      assert.match(output, new RegExp(`gate:.*${rejectedRelease.replaceAll(".", "\\.")}\\.env`, "u"));
      assert.doesNotMatch(output, /secrets|restore:|record:/u);
    }

    const recoverTranscript = join(directory, "recover.transcript");
    const recover = runBash(
      `source "$1"
state_dir=$2/state
transcript=$3
read_release_state() { printf '2026.08.22-2\n'; }
release_env() { printf '/releases/%s.env\n' "$1"; }
validate_release_env() { :; }
require_control_plane_compatibility() { printf 'gate:%s\n' "$1" >>"$transcript"; return 1; }
require_deployment_secrets() { printf 'secrets\n' >>"$transcript"; }
restore_images() { printf 'restore:%s\n' "$1" >>"$transcript"; }
write_active_symlink() { printf 'active:%s\n' "$1" >>"$transcript"; }
main recover`,
      [directory, recoverTranscript],
    );
    assert.notEqual(recover.status, 0);
    assert.match(await readFile(recoverTranscript, "utf8"), /^gate:.*2026\.08\.22-2\.env\n$/u);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// verify independent switches and fail-closed temperature defaults
test("release switches preserve independent values and reject ambiguous temperature declarations", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-temperature-switch-"));
  const source = join(directory, "source.env");
  try {
    // cover both independent settings and malformed input
    for (const [content, expected, pass] of [
      ["", "1", true],
      ["WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=0\n", "1", true],
      ["WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=0\nWEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=1\n", "0", true],
      ["WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=1\n", "1", true],
      ["WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=true\n", "", false],
      ["WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=0\nWEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=1\n", "", false],
      ["WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH =0\n", "", false],
    ]) {
      await writeFile(source, content);
      const result = runBash('source "$1"; source_canary_kill_switch "$2" TEMPERATURE 1', [source]);
      assert.equal(result.status === 0, pass, result.stderr);
      assert.equal(result.stdout.trim(), expected);
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// reject incompatible direct-deploy inputs before any mutable operation
test("direct deployment gates both retained target and active control identities", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-yolo-control-"));
  const releases = join(directory, "releases");
  const state = join(directory, "state");
  const transcript = join(directory, "transcript");
  const source = join(directory, "source.env");
  const currentRelease = "2026.10.07-3";
  const targetRelease = "2026.10.08-1";
  const current = join(releases, `${currentRelease}.env`);
  const target = join(releases, `${targetRelease}.env`);
  const imageDigest = `sha256:${"a".repeat(64)}`;

  // build one structurally complete release identity
  function releaseContent(release, controlVersion, controlDigest) {
    return [
      `WEATHER_RELEASE=${release}`,
      `WEATHER_SERVER_IMAGE=ghcr.io/anstosa/weather-server@${imageDigest}`,
      `WEATHER_WEB_IMAGE=ghcr.io/anstosa/weather-web@${imageDigest}`,
      `POSTGRES_IMAGE=postgres@${imageDigest}`,
      `CLOUDFLARED_IMAGE=cloudflare/cloudflared@${imageDigest}`,
      "WEATHER_DATABASE_NAME=weather",
      "WEATHER_POSTGRES_DIR=/var/lib/weather/postgres",
      "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=0",
      "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH=1",
      `WEATHER_CONTROL_PLANE_SHA256=${controlDigest}`,
      `WEATHER_CONTROL_PLANE_VERSION=${controlVersion}`,
      "",
    ].join("\n");
  }

  try {
    await mkdir(releases);
    await mkdir(state);
    await writeFile(join(state, "current-release"), `${currentRelease}\n`, { mode: 0o600 });
    const predecessor = releaseContent(
      currentRelease,
      12,
      "16d871c7aebb3a34097af219fd5c76a93b3ff1be3af521643dbf3a4d2041c61d",
    );
    await writeFile(current, predecessor);
    await writeFile(source, predecessor);
    await writeFile(target, releaseContent(targetRelease, 13, "b".repeat(64)));
    await writeFile(transcript, "");

    const targetRejected = runBash(
      `source "$1"
releases_dir=$2
state_dir=$3
transcript=$4
require_deployment_secrets() { printf 'secrets\n' >>"$transcript"; }
cleanup_obsolete_weather_images() { printf 'cleanup\n' >>"$transcript"; }
compose() { printf 'compose:%s\n' "$*" >>"$transcript"; }
yolo_release "$5" "$6"`,
      [releases, state, transcript, targetRelease, source],
    );
    assert.notEqual(targetRejected.status, 0);
    assert.match(targetRejected.stderr, /control-plane identity is unsupported/u);
    assert.equal(await readFile(transcript, "utf8"), "");

    await writeFile(transcript, "");
    const invalidPredecessor = releaseContent(currentRelease, 12, "c".repeat(64));
    await writeFile(current, invalidPredecessor);
    await writeFile(source, invalidPredecessor);
    const activeRejected = runBash(
      `source "$1"
releases_dir=$2
state_dir=$3
transcript=$4
require_deployment_secrets() { printf 'secrets\n' >>"$transcript"; }
cleanup_obsolete_weather_images() { printf 'cleanup\n' >>"$transcript"; }
compose() { printf 'compose:%s\n' "$*" >>"$transcript"; }
yolo_release "$5" "$6"`,
      [releases, state, transcript, targetRelease, source],
    );
    assert.notEqual(activeRejected.status, 0);
    assert.match(activeRejected.stderr, /control-plane identity is unsupported/u);
    assert.equal(await readFile(transcript, "utf8"), "");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
