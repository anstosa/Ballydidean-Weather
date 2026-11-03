import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmod, mkdtemp, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");
const helper = "scripts/direct-maintenance-compatibility.sh";

// read the unchanged bounded fixture through bash itself
function originalDefinition() {
  return execFileSync("bash", ["-c", "source deploy/scripts/update.sh; declare -f verify_bounded_maintenance_source_compatibility"], {
    cwd: root,
    encoding: "utf8",
  });
}

// adapt public callback text without loading production state
function adapt(definition) {
  return spawnSync("bash", ["-c", `source ${helper}; direct_compatibility_definition`], {
    cwd: root,
    input: definition,
    encoding: "utf8",
  });
}

// preserve every existing source-runtime and rejection probe
test("direct compatibility changes exactly four bounded fixture callbacks", () => {
  const original = originalDefinition();
  const result = adapt(original);
  assert.equal(result.status, 0, result.stderr);
  const expected = original
    .replace("verify_bounded_maintenance_source_compatibility ()", "verify_direct_source_compatibility ()")
    .replace("require_fixed_v14_source_identity", "require_direct_source_identity")
    .replace("require_fixed_v14_migration_ledger", "require_direct_target_ledger")
    .replace("WEATHER_ENV_FILE=$target_env compose run --rm --no-deps --env WEATHER_DATABASE_NAME=\"$candidate\" migration;", "direct_fixture_migrate \"$target_env\" \"$candidate\";");
  assert.equal(result.stdout, expected);
  assert.match(result.stdout, /fixture_budget_bytes=\$\(\(64 \* 1024 \* 1024\)\)/u);
  assert.match(result.stdout, /source worker did not persist the deterministic forecast/u);
  assert.match(result.stdout, /source worker accepted unproven migration history/u);
  assert.match(result.stdout, /source API accepted invalid migration authorization/u);
  assert.match(result.stdout, /source API compatibility failed/u);
  assert.equal(spawnSync("bash", ["-n"], { input: result.stdout }).status, 0);
});

// reject drift before evaluating any adapted shell
test("direct compatibility refuses missing or repeated callback anchors", () => {
  const original = originalDefinition();
  // cover every distinct substitution boundary
  for (const anchor of [
    "verify_bounded_maintenance_source_compatibility ()",
    "require_fixed_v14_source_identity",
    "require_fixed_v14_migration_ledger",
    "WEATHER_ENV_FILE=$target_env compose run --rm --no-deps --env WEATHER_DATABASE_NAME=\"$candidate\" migration;",
  ]) {
    assert.notEqual(adapt(original.replace(anchor, "changed_callback")).status, 0);
    assert.notEqual(adapt(`${original}\n${anchor}\n`).status, 0);
  }
});

// prove the fixture runs the real prefix and alternate migration
test("direct fixture callback uses an empty schema 18 before the repaired atomic tail", () => {
  const output = execFileSync("bash", ["-c", `
source ${helper}
# record commands instead of starting any service
compose() { printf 'CALL\\0%s\\0' "$WEATHER_ENV_FILE"; printf '%s\\0' "$@"; }
direct_fixture_migrate /target.env weather_v14_compat_123_456
`], { cwd: root, encoding: "utf8" });
  const calls = output.split("CALL\0").slice(1);
  assert.equal(calls.length, 2);
  assert.match(calls[0], /names\.slice\(0, 18\)/u);
  assert.match(calls[0], /await runMigrations\(pool, prefix\)/u);
  assert.match(calls[1], /--atomic-direct-18-23/u);
  assert.match(calls[1], /WEATHER_DATABASE_STATEMENT_TIMEOUT_MS=300000/u);
  // reject a full-size clone in either fixture phase
  for (const call of calls) {
    assert.match(call, /WEATHER_DATABASE_NAME=weather_v14_compat_123_456/u);
    assert.doesNotMatch(call, /pg_dump|pg_restore|CREATE DATABASE.*TEMPLATE weather/u);
  }
});

// reject another database or another completed history
test("direct compatibility ledger callback is closed to its fixture and revised hash", () => {
  const command = `source ${helper};
direct_source_environment=/source.env;
# retain a bounded diagnostic failure
die() { printf '%s\\n' "$1" >&2; return 1; }
# return only the supplied public test ledger
migration_history_sha256() { printf '%s' "$TEST_HISTORY"; }
require_direct_target_ledger "$1" "$2"`;
  const environment = { ...process.env, TEST_HISTORY: "3fa9ed85218278aa3b20537d9fbe9b88ef1adee7d7b2f80fde832c45d9cb4554" };
  assert.equal(spawnSync("bash", ["-c", command, "test", "/source.env", "weather_v14_compat_123_456"], { cwd: root, env: environment }).status, 0);
  assert.notEqual(spawnSync("bash", ["-c", command, "test", "/source.env", "weather"], { cwd: root, env: environment }).status, 0);
  assert.notEqual(spawnSync("bash", ["-c", command, "test", "/other.env", "weather_v14_compat_123_456"], { cwd: root, env: environment }).status, 0);
  assert.notEqual(spawnSync("bash", ["-c", command, "test", "/source.env", "weather_v14_compat_123_456"], { cwd: root, env: { ...environment, TEST_HISTORY: "0".repeat(64) } }).status, 0);
});

// cover failed publication rollback without touching any service
test("direct compatibility removes only its exact uncommitted authorization", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-direct-compatibility-"));
  const source = join(directory, "source.env");
  const target = join(directory, "target.env");
  const authorization = join(directory, "authorization");
  const proof = join(directory, "proof.json");
  const bytes = "private-test-authorization\n";
  const digest = createHash("sha256").update(bytes).digest("hex");

  try {
    await writeFile(source, "source\n");
    await writeFile(target, "target\n");
    await writeFile(authorization, bytes, { mode: 0o600 });
    let details = await stat(authorization);
    let identity = `${String(details.dev)}:${String(details.ino)}`;
    // fail before the final proof exists
    const failed = spawnSync("bash", ["-c", `source ${helper}; direct_compatibility_publish_proof "$@"`, "test", source, target, authorization, join(directory, "missing", "proof.json")], { cwd: root });
    assert.notEqual(failed.status, 0);
    const cleaned = spawnSync("bash", ["-c", `source ${helper}; direct_compatibility_unpublish "$@"`, "test", authorization, proof, identity, digest], { cwd: root });
    assert.equal(cleaned.status, 0);
    await assert.rejects(readFile(authorization), { code: "ENOENT" });
    // retry only after exact owned rollback completed
    await writeFile(authorization, bytes, { mode: 0o600 });
    const succeeded = spawnSync("bash", ["-c", `source ${helper}; direct_compatibility_publish_proof "$@"`, "test", source, target, authorization, proof], { cwd: root });
    assert.equal(succeeded.status, 0, succeeded.stderr?.toString());
    const value = JSON.parse(await readFile(proof, "utf8"));
    assert.equal(value.authorizationSha256, digest);
    assert.equal(value.state, "compatibility_ready");
    assert.equal((await stat(proof)).nlink, 1);
    assert.equal((await stat(proof)).mode & 0o777, 0o600);
    details = await stat(authorization);
    identity = `${String(details.dev)}:${String(details.ino)}`;
    // retain committed authorization even during later cleanup
    assert.equal(spawnSync("bash", ["-c", `source ${helper}; direct_compatibility_unpublish "$@"`, "test", authorization, proof, identity, digest], { cwd: root }).status, 0);
    assert.equal(await readFile(authorization, "utf8"), bytes);
    await rm(proof);
    await rename(authorization, `${authorization}.retained`);
    await writeFile(authorization, bytes, { mode: 0o600 });
    await chmod(authorization, 0o600);
    // preserve a different inode even when its bytes match
    assert.notEqual(spawnSync("bash", ["-c", `source ${helper}; direct_compatibility_unpublish "$@"`, "test", authorization, proof, identity, digest], { cwd: root }).status, 0);
    assert.equal(await readFile(authorization, "utf8"), bytes);
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});
