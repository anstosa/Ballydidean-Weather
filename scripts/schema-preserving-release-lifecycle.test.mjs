import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "..");
const sourcePath = join(repoRoot, "scripts/schema-preserving-release.sh");
const modulePath = join(repoRoot, "scripts/schema-preserving-release.mjs");

// copy the real operator while redirecting only its fixed production paths
async function fixture() {
  const root = await mkdtemp(join(tmpdir(), "weather-schema-preserving-"));
  const scripts = join(root, "scripts");
  const state = join(root, "deploy/state");
  const releases = join(root, "deploy/releases");
  const data = join(root, "var/lib/weather/xweather");
  await Promise.all([
    mkdir(scripts, { recursive: true }),
    mkdir(state, { recursive: true }),
    mkdir(releases, { recursive: true }),
    mkdir(data, { recursive: true }),
  ]);
  const operator = join(scripts, "schema-preserving-release.sh");
  const rewritten = (await readFile(sourcePath, "utf8"))
    .replace("/opt/weather/current/deploy/state/schema-preserving-release-v1", join(state, "schema-preserving-release-v1"))
    .replace("/opt/weather/current/deploy/scripts/update.sh", join(root, "deploy/scripts/update.sh"))
    .replaceAll("/opt/weather/current/deploy/state", state)
    .replaceAll("/opt/weather/current/deploy/releases", releases)
    .replace("/var/lib/weather/xweather/forecast-adjustment-settings.json", join(data, "forecast-adjustment-settings.json"));
  await writeFile(operator, rewritten, { mode: 0o755 });
  await writeFile(join(scripts, "schema-preserving-release.mjs"), await readFile(modulePath), { mode: 0o644 });
  return { root, operator, state, releases, spRoot: join(state, "schema-preserving-release-v1") };
}

// source the copied operator and execute one isolated scenario
function runBash(context, body, args = []) {
  return spawnSync("bash", ["-c", `source "$1"\n${body}`, "schema-preserving-test", context.operator, ...args], {
    encoding: "utf8",
    env: process.env,
  });
}

// provide safe boundaries and fatal sentinels for every mutation class
const guards = String.raw`
log=$2
root=$3
die() { printf 'die:%s\n' "$*" >>"$log"; exit 70; }
forbidden() { printf 'FORBIDDEN:%s\n' "\${FUNCNAME[1]}" >>"$log"; exit 91; }
backup_release_database() { forbidden; }
create_database_fixture_clone() { forbidden; }
run_migration() { forbidden; }
apply_runtime_database_acl() { forbidden; }
start_postgres() { forbidden; }
validate_release() { [[ "$1" =~ ^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[0-9]+$ ]] || die 'bad release'; }
sp_private() { printf 'private:%s\n' "$1" >>"$log"; [[ -f "$1" ]] || die 'unsafe input'; }
sp_sha() { printf '%064d\n' 1; }
sp_field() {
  case "$2" in
    operatorSha256|validatorSha256) printf '%064d\n' 1 ;;
    serverImage) printf 'server-image\n' ;;
    webImage) printf 'web-image\n' ;;
    serverImageId) printf 'sha256:%064d\n' 2 ;;
    webImageId) printf 'sha256:%064d\n' 3 ;;
    targetCommit) printf '%040d\n' 4 ;;
    *) command node "$sp_module" field "$1" "$2" ;;
  esac
}
node() {
  if [[ "$*" == *validate-publication* ]]; then return 0; fi
  if [[ "$1" == --input-type=module ]]; then
    shift 2
    cat >/dev/null
    if (($# == 7)); then
      printf '{"sourceRelease":"%s","targetRelease":"%s","previousRelease":"%s","stageSha256":"%s","stageRelease":"%s","protectedSnapshot":"%s","protectedSha256":"%s"}\n' "$@"
    else
      printf '{}\n'
    fi
    return 0
  fi
  command node "$@"
}
env_value() {
  case "$2" in
    WEATHER_RELEASE) basename "$1" .env ;;
    POSTGRES_IMAGE) printf 'postgres-image\n' ;;
    CLOUDFLARED_IMAGE) printf 'cloudflare-image\n' ;;
    WEATHER_DATABASE_NAME) printf 'weather\n' ;;
    WEATHER_SERVER_IMAGE) printf 'server-image\n' ;;
    WEATHER_WEB_IMAGE) printf 'web-image\n' ;;
    *) printf 'fixed\n' ;;
  esac
}
release_env() { printf '%s/deploy/releases/%s.env\n' "$root" "$1"; }
install() { local destination; destination=$(printf '%s\n' "$@" | tail -n1); mkdir -p "$destination"; chmod 700 "$destination"; printf 'mkdir:%s\n' "$destination" >>"$log"; }
write_release_env() { printf 'WEATHER_RELEASE=%s\n' "$3" >"$2"; chmod 600 "$2"; }
sp_publish() { cat >"$1"; chmod 600 "$1"; printf 'publish:%s\n' "$1" >>"$log"; }
sp_capacity() { printf 'capacity\n' >>"$log"; }
sp_image() { printf 'image:%s\n' "$1" >>"$log"; }
sp_snapshot() { printf 'snapshot:%s\n' "$1" >>"$log"; printf protected >"$2"; chmod 600 "$2"; }
sp_unchanged() { printf 'unchanged:%s\n' "$1" >>"$log"; }
docker() {
  printf 'docker:%s\n' "$*" >>"$log"
  if [[ "$1 $2" == 'image inspect' && "$*" == *revision* ]]; then printf '%040d\n' 4; fi
}
compose() { printf 'compose:%s\n' "$*" >>"$log"; }
`;

// read a transcript only after the command has created it
async function transcript(path) {
  return readFile(path, "utf8");
}

test("stage rejects bad publication, source, and ledger before mutation", async () => {
  // prove each independent gate precedes staging side effects
  for (const failure of ["publication", "source", "ledger"]) {
    const context = await fixture();
    const log = join(context.root, `${failure}.log`);
    const publication = join(context.root, "publication.json");
    await writeFile(publication, "{}\n", { mode: 0o600 });
    const body = `${guards}\n${String.raw`
failure=$4; publication=$5
sp_private() { printf 'private:%s\n' "$1" >>"$log"; [[ "$failure" != publication ]] || die 'bad publication'; }
sp_source() { printf source-check >>"$log"; [[ "$failure" != source ]] || die 'bad source'; release_env 2026.11.03-3; }
sp_health() { printf health-check >>"$log"; }
sp_database() { printf ledger-check >>"$log"; [[ "$failure" != ledger ]] || die 'bad ledger'; }
sp_stage 2026.11.04-1 "$publication" "$(sp_sha "$publication")"
`}`;
    try {
      const result = runBash(context, body, [log, context.root, failure, publication]);
      assert.notEqual(result.status, 0, `${failure} unexpectedly succeeded`);
      const output = await transcript(log);
      assert.doesNotMatch(output, /mkdir:|docker:|snapshot:|compose:|FORBIDDEN:/u);
    } finally {
      await rm(context.root, { force: true, recursive: true });
    }
  }
});

test("stage remains private and read-only across source and target readiness", async () => {
  const context = await fixture();
  const log = join(context.root, "stage.log");
  const publication = join(context.root, "publication.json");
  await writeFile(publication, "{}\n", { mode: 0o600 });
  await mkdir(context.spRoot, { mode: 0o700 });
  const body = `${guards}\n${String.raw`
source_env="$root/deploy/releases/2026.11.03-3.env"
printf source >"$source_env"
sp_source() { printf '%s\n' "$source_env"; }
sp_health() { printf 'health:%s\n' "$1" >>"$log"; }
sp_database() { printf 'database:%s\n' "$1" >>"$log"; }
sp_readiness() { printf 'readiness:%s:readonly\n' "$1" >>"$log"; }
sp_stage 2026.11.04-1 "$4" "$(sp_sha "$4")"
stat -c 'stage-mode:%a' "$sp_root/2026.11.04-1" >>"$log"
`}`;
  try {
    const result = runBash(context, body, [log, context.root, publication]);
    assert.equal(result.status, 0, result.stderr);
    const output = await transcript(log);
    assert.match(output, /readiness:.*2026\.11\.03-3\.env:readonly/u);
    assert.match(output, /readiness:.*environment\.env:readonly/u);
    assert.match(output, /docker:pull server-image[\s\S]*docker:pull web-image/u);
    assert.match(output, /stage-mode:700/u);
    assert.equal((output.match(/database:/gu) ?? []).length, 2);
    assert.equal((output.match(/unchanged:/gu) ?? []).length, 1);
    assert.doesNotMatch(output, /docker:pull (?:postgres|cloudflare)|FORBIDDEN:/u);
  } finally {
    await rm(context.root, { force: true, recursive: true });
  }
});

// create the files used by forward and reverse transaction tests
async function transactionFiles(context) {
  const directory = join(context.spRoot, "2026.11.04-1");
  await mkdir(directory, { recursive: true, mode: 0o700 });
  await writeFile(join(directory, "stage.json"), '{"sourceRelease":"2026.11.03-3"}\n', { mode: 0o600 });
  await writeFile(join(directory, "protected.json"), "protected\n", { mode: 0o600 });
  await writeFile(join(context.releases, "2026.11.03-3.env"), "source\n", { mode: 0o600 });
  await writeFile(join(context.releases, "2026.11.04-1.env"), "target\n", { mode: 0o600 });
  return directory;
}

// construct one closed-shape stage receipt for validator boundary tests
function stageReceipt(overrides = {}) {
  return {
    contractVersion: "weather-schema-preserving-stage/v1",
    sourceRelease: "2026.11.03-3",
    targetRelease: "2026.11.04-1",
    sourceEnvironmentSha256: "1".repeat(64),
    targetEnvironmentSha256: "2".repeat(64),
    publicationSha256: "3".repeat(64),
    protectedSha256: "4".repeat(64),
    ...overrides,
  };
}

test("stage validation rejects malformed target and ready receipts before side effects", async () => {
  const cases = [
    { name: "target-contract", action: "activate", receipt: stageReceipt({ contractVersion: "wrong" }) },
    { name: "ready-target", action: "source", receipt: stageReceipt({ targetRelease: "2026.11.05-1" }) },
  ];
  // exercise both staged activation and committed ancestry validation
  for (const scenario of cases) {
    const context = await fixture();
    const log = join(context.root, `${scenario.name}.log`);
    const directory = join(context.spRoot, "2026.11.04-1");
    const environment = join(context.releases, "2026.11.04-1.env");
    await mkdir(directory, { recursive: true, mode: 0o700 });
    await writeFile(join(directory, scenario.action === "activate" ? "stage.json" : "ready.json"), `${JSON.stringify(scenario.receipt)}\n`, { mode: 0o600 });
    await writeFile(join(directory, "publication.json"), "{}\n", { mode: 0o600 });
    await writeFile(environment, "target\n", { mode: 0o600 });
    await symlink(environment, join(context.state, "active.env"));
    const body = `${guards}\n${String.raw`
action=$4
sp_environment() { printf 'environment:%s\n' "$1" >>"$log"; release_env "$1"; }
sp_image() { printf 'UNSAFE-IMAGE\n' >>"$log"; }
sp_transaction() { printf 'UNSAFE-TRANSACTION\n' >>"$log"; }
read_release_state() { [[ "$1" == *schema-release ]] && printf '2026.11.03-3\n' || printf '2026.11.04-1\n'; }
if [[ "$action" == activate ]]; then sp_source() { release_env 2026.11.03-3; }; sp_activate 2026.11.04-1; else sp_source; fi
`}`;
    try {
      const result = runBash(context, body, [log, context.root, scenario.action]);
      assert.notEqual(result.status, 0, `${scenario.name} receipt unexpectedly accepted`);
      const output = await transcript(log);
      assert.doesNotMatch(output, /UNSAFE-IMAGE|UNSAFE-TRANSACTION/u);
      assert.equal((output.match(/environment:/gu) ?? []).length, scenario.action === "source" ? 1 : 0);
    } finally {
      await rm(context.root, { force: true, recursive: true });
    }
  }
});

test("activation switches api and worker before web and commits after snapshots", async () => {
  const context = await fixture();
  const log = join(context.root, "activate.log");
  await transactionFiles(context);
  const body = `${guards}\n${String.raw`
sp_source() { release_env 2026.11.03-3; }
sp_target() { :; }
sp_environment() { release_env "$1"; }
sp_database() { printf 'database:%s\n' "$1" >>"$log"; }
sp_health() { printf 'health:%s\n' "$1" >>"$log"; }
read_release_state() { printf '2026.11.03-2\n'; }
record_release_success() { printf 'marker:%s:%s\n' "$1" "$2" >>"$log"; }
sp_activate 2026.11.04-1
`}`;
  try {
    const result = runBash(context, body, [log, context.root]);
    assert.equal(result.status, 0, result.stderr);
    const events = (await transcript(log)).trim().split("\n");
    const apiWorker = events.findIndex((line) => /compose:.*up .*api worker$/u.test(line));
    const web = events.findIndex((line) => /compose:.*up .*web$/u.test(line));
    const targetDatabase = events.findIndex((line) => /database:.*2026\.11\.04-1/u.test(line));
    const finalSnapshot = events.findLastIndex((line) => /unchanged:.*2026\.11\.04-1/u.test(line));
    const marker = events.findIndex((line) => line.startsWith("marker:"));
    assert.ok(apiWorker >= 0 && apiWorker < web);
    assert.ok(web < targetDatabase && targetDatabase < finalSnapshot && finalSnapshot < marker);
    assert.doesNotMatch(events.join("\n"), /schema-release|FORBIDDEN:/u);
  } finally {
    await rm(context.root, { force: true, recursive: true });
  }
});

test("target health failure restores source with the current marker unchanged", async () => {
  const context = await fixture();
  const log = join(context.root, "health-failure.log");
  await transactionFiles(context);
  const body = `${guards}\n${String.raw`
sp_source() { release_env 2026.11.03-3; }
sp_target() { :; }
sp_environment() { release_env "$1"; }
sp_database() { :; }
read_release_state() { [[ "$1" == *current-release ]] && printf '2026.11.03-3\n' || printf '2026.11.03-2\n'; }
write_active_symlink() { printf 'active:%s\n' "$1" >>"$log"; }
write_private_state() { printf 'previous:%s\n' "$2" >>"$log"; }
record_release_success() { printf 'UNSAFE-MARKER\n' >>"$log"; }
sp_health() { if [[ "$1" == *2026.11.04-1.env ]]; then printf 'target-health-failed\n' >>"$log"; return 12; fi; printf 'source-health-ok\n' >>"$log"; }
sp_activate 2026.11.04-1
`}`;
  try {
    const result = runBash(context, body, [log, context.root]);
    assert.notEqual(result.status, 0);
    const output = await transcript(log);
    assert.match(output, /target-health-failed[\s\S]*source-health-ok[\s\S]*active:2026\.11\.03-3/u);
    assert.doesNotMatch(output, /UNSAFE-MARKER|FORBIDDEN:/u);
  } finally {
    await rm(context.root, { force: true, recursive: true });
  }
});

test("nested recovery marker failure aborts and retains the journal", async () => {
  const context = await fixture();
  const log = join(context.root, "nested-recovery-failure.log");
  await transactionFiles(context);
  const body = `${guards}\n${String.raw`
sp_source() { release_env 2026.11.03-3; }
sp_target() { :; }
sp_environment() { release_env "$1"; }
sp_database() { :; }
read_release_state() { [[ "$1" == *current-release ]] && printf '2026.11.03-3\n' || printf '2026.11.03-2\n'; }
write_active_symlink() { printf 'active-failed:%s\n' "$1" >>"$log"; return 22; }
write_private_state() { printf 'UNSAFE-PREVIOUS\n' >>"$log"; }
record_release_success() { printf 'UNSAFE-SUCCESS\n' >>"$log"; }
sp_health() { [[ "$1" != *2026.11.04-1.env ]] || return 12; }
sp_activate 2026.11.04-1
`}`;
  try {
    const result = runBash(context, body, [log, context.root]);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /recovery requires operator attention/u);
    assert.match(await transcript(log), /active-failed:2026\.11\.03-3/u);
    assert.doesNotMatch(await transcript(log), /UNSAFE-PREVIOUS|UNSAFE-SUCCESS/u);
    assert.match(await readFile(join(context.spRoot, "journal.json"), "utf8"), /sourceRelease/u);
  } finally {
    await rm(context.root, { force: true, recursive: true });
  }
});

test("reconcile follows valid commit points and rejects an unrelated pair", async () => {
  const cases = [
    { current: "2026.11.03-3", source: "2026.11.03-3", succeeds: true },
    { current: "2026.11.04-1", source: "2026.11.03-3", succeeds: true },
    { current: "2026.11.03-2", source: "2026.11.03-2", succeeds: false },
  ];
  // exercise both commit points and one unrelated journal pair
  for (const { current, source, succeeds } of cases) {
    const context = await fixture();
    const log = join(context.root, `recover-${current}.log`);
    const directory = await transactionFiles(context);
    await writeFile(join(context.spRoot, "journal.json"), JSON.stringify({
      sourceRelease: source,
      targetRelease: "2026.11.04-1",
      previousRelease: "2026.11.03-2",
      stageSha256: "1".padStart(64, "0"),
      stageRelease: "2026.11.04-1",
      protectedSnapshot: join(directory, "protected.json"),
      protectedSha256: "1".padStart(64, "0"),
    }), { mode: 0o600 });
    const body = `${guards}\n${String.raw`
sp_target() { :; }
sp_environment() { release_env "$1"; }
sp_database() { :; }
sp_switch() { printf 'switch:%s\n' "$1" >>"$log"; }
current_marker=$4
read_release_state() { printf '%s\n' "$current_marker"; }
write_active_symlink() { printf 'active:%s\n' "$1" >>"$log"; }
write_private_state() { printf 'previous:%s\n' "$2" >>"$log"; }
sp_reconcile
`}`;
    try {
      const result = runBash(context, body, [log, context.root, current]);
      // reject a journal that is not the staged forward or reverse pair
      if (!succeeds) {
        assert.notEqual(result.status, 0);
        assert.doesNotMatch(await transcript(log), /switch:/u);
        continue;
      }
      assert.equal(result.status, 0, result.stderr);
      const output = await transcript(log);
      assert.match(output, new RegExp(`switch:.*${current.replaceAll(".", "\\.")}`));
      assert.equal(output.includes("previous:2026.11.03-2"), current === "2026.11.03-3");
    } finally {
      await rm(context.root, { force: true, recursive: true });
    }
  }
});

test("rollback takes a fresh protected snapshot before using the retained pair", async () => {
  const context = await fixture();
  const log = join(context.root, "rollback.log");
  const directory = await transactionFiles(context);
  const body = `${guards}\n${String.raw`
sp_source() { release_env 2026.11.04-1; }
sp_environment() { release_env "$1"; }
read_release_state() { printf '2026.11.03-3\n'; }
sp_target() { :; }
sp_database() { printf 'database:%s\n' "$1" >>"$log"; }
sp_transaction() { printf 'transaction:%s:%s:%s\n' "$1" "$2" "$3" >>"$log"; }
sp_rollback
`}`;
  try {
    const result = runBash(context, body, [log, context.root]);
    assert.equal(result.status, 0, result.stderr);
    const output = await transcript(log);
    assert.match(output, /database:.*2026\.11\.04-1\.env[\s\S]*snapshot:.*2026\.11\.04-1\.env/u);
    assert.match(output, /transaction:2026\.11\.03-3:2026\.11\.04-1:.*rollback-protected-[0-9]+-[0-9]+\.json/u);
    assert.doesNotMatch(output, /FORBIDDEN:/u);
    assert.equal((await readFile(join(directory, "stage.json"), "utf8")).includes("2026.11.03-3"), true);
  } finally {
    await rm(context.root, { force: true, recursive: true });
  }
});

test("retire quarantines partial stages before, after, and orphaned at environment publication", async () => {
  const scenarios = [
    { name: "before", hasStage: true, targetPublished: false },
    { name: "after", hasStage: true, targetPublished: true },
    { name: "orphan", hasStage: false, targetPublished: true },
  ];
  // verify every interrupted stage publication namespace
  for (const scenario of scenarios) {
    const context = await fixture();
    const log = join(context.root, `retire-${scenario.name}.log`);
    const release = "2026.11.04-1";
    const directory = join(context.spRoot, release);
    // create stage-local evidence only when publication reached that namespace
    if (scenario.hasStage) {
      await mkdir(directory, { recursive: true, mode: 0o700 });
      await writeFile(join(directory, "publication.json"), "{}\n", { mode: 0o600 });
      await writeFile(join(directory, "protected.json"), "protected\n", { mode: 0o600 });
      if (!scenario.targetPublished) await writeFile(join(directory, "environment.env"), "target\n", { mode: 0o600 });
    }
    if (scenario.targetPublished) await writeFile(join(context.releases, `${release}.env`), "target\n", { mode: 0o600 });
    const body = `${guards}\n${String.raw`
sp_source() { :; }
read_release_state() { [[ "$1" == *current-release ]] && printf '2026.11.03-3\n' || printf '2026.11.03-2\n'; }
sp_environment() { printf 'environment-validated:%s\n' "$1" >>"$log"; release_env "$1"; }
stat() { if [[ "$1 $2" == '-c %u:%g:%a' ]]; then printf '0:0:700\n'; else command stat "$@"; fi; }
sp_retire_stage 2026.11.04-1
[[ ! -e "$sp_root/2026.11.04-1" && ! -e "$(release_env 2026.11.04-1)" ]] || exit 88
printf 'namespace-clear\n' >>"$log"
find "$sp_root/retired" -mindepth 2 -maxdepth 2 -printf 'retired:%f\n' | sort >>"$log"
`}`;
    try {
      const result = runBash(context, body, [log, context.root]);
      assert.equal(result.status, 0, result.stderr);
      const output = await transcript(log);
      assert.match(output, /namespace-clear/u);
      assert.equal(output.includes("retired:stage"), scenario.hasStage);
      assert.equal(output.includes("retired:environment.env"), scenario.targetPublished);
      assert.equal(output.includes("environment-validated:2026.11.04-1"), scenario.targetPublished);
      assert.doesNotMatch(output, /docker:|FORBIDDEN:/u);
    } finally {
      await rm(context.root, { force: true, recursive: true });
    }
  }
});

test("retire refuses symlinked or foreign quarantine roots", async () => {
  // prove an existing quarantine namespace is never followed or claimed
  for (const kind of ["symlink", "foreign"]) {
    const context = await fixture();
    const log = join(context.root, `retire-root-${kind}.log`);
    const directory = join(context.spRoot, "2026.11.04-1");
    const retired = join(context.spRoot, "retired");
    await mkdir(directory, { recursive: true, mode: 0o700 });
    await writeFile(join(directory, "publication.json"), "{}\n", { mode: 0o600 });
    if (kind === "symlink") {
      const outside = join(context.root, "outside");
      await mkdir(outside, { mode: 0o700 });
      await symlink(outside, retired);
    } else {
      await mkdir(retired, { mode: 0o755 });
    }
    const body = `${guards}\n${String.raw`
kind=$4
sp_source() { :; }
read_release_state() { [[ "$1" == *current-release ]] && printf '2026.11.03-3\n' || printf '2026.11.03-2\n'; }
stat() { if [[ "$1 $2" == '-c %u:%g:%a' ]]; then [[ "$kind" == foreign && "$3" == "$sp_root/retired" ]] && printf '1000:1000:755\n' || printf '0:0:700\n'; else command stat "$@"; fi; }
sp_retire_stage 2026.11.04-1
`}`;
    try {
      const result = runBash(context, body, [log, context.root, kind]);
      assert.notEqual(result.status, 0, `${kind} quarantine unexpectedly accepted`);
      assert.match(await transcript(log), /stage quarantine root differs/u);
      assert.deepEqual(await readdir(directory), ["publication.json"]);
    } finally {
      await rm(context.root, { force: true, recursive: true });
    }
  }
});

test("retire refuses retained, committed, journaled, and unexpected stages", async () => {
  const cases = [
    { name: "current", release: "2026.11.04-1", current: "2026.11.04-1", previous: "2026.11.03-2", error: /retained or active/u },
    { name: "previous", release: "2026.11.04-1", current: "2026.11.03-3", previous: "2026.11.04-1", error: /retained or active/u },
    { name: "anchor", release: "2026.11.03-3", current: "2026.11.03-2", previous: "2026.11.03-1", error: /retained or active/u },
    { name: "ready", release: "2026.11.04-1", current: "2026.11.03-3", previous: "2026.11.03-2", error: /committed application evidence/u },
    { name: "journal", release: "2026.11.04-1", current: "2026.11.03-3", previous: "2026.11.03-2", error: /retained or active/u },
    { name: "unexpected", release: "2026.11.04-1", current: "2026.11.03-3", previous: "2026.11.03-2", error: /unexpected object/u },
  ];
  // preserve every namespace that is not an uncommitted recognized partial stage
  for (const scenario of cases) {
    const context = await fixture();
    const log = join(context.root, `retire-${scenario.name}.log`);
    const directory = join(context.spRoot, scenario.release);
    await mkdir(directory, { recursive: true, mode: 0o700 });
    if (scenario.name === "ready") await writeFile(join(directory, "ready.json"), "{}\n", { mode: 0o600 });
    if (scenario.name === "unexpected") await writeFile(join(directory, "surprise"), "x\n", { mode: 0o600 });
    if (scenario.name === "journal") await writeFile(join(context.spRoot, "journal.json"), "{}\n", { mode: 0o600 });
    const body = `${guards}\n${String.raw`
marker_current=$4; marker_previous=$5; release=$6
sp_source() { :; }
read_release_state() { [[ "$1" == *current-release ]] && printf '%s\n' "$marker_current" || printf '%s\n' "$marker_previous"; }
stat() { if [[ "$1 $2" == '-c %u:%g:%a' ]]; then printf '0:0:700\n'; else command stat "$@"; fi; }
sp_retire_stage "$release"
`}`;
    try {
      const result = runBash(context, body, [log, context.root, scenario.current, scenario.previous, scenario.release]);
      assert.notEqual(result.status, 0, `${scenario.name} unexpectedly retired`);
      assert.match(await transcript(log), scenario.error);
      assert.ok(Array.isArray(await readdir(directory)));
    } finally {
      await rm(context.root, { force: true, recursive: true });
    }
  }
});

test("retained infrastructure identity failures occur before either container starts", async () => {
  // cover missing containers plus configuration and cached-image identity drift
  for (const scenario of ["missing", "wrong-config", "wrong-image-id"]) {
    const context = await fixture();
    const log = join(context.root, `infrastructure-${scenario}.log`);
    const body = `${guards}\n${String.raw`
scenario=$4
postgres_id=$(printf 'a%.0s' {1..64}); cloudflared_id=$(printf 'b%.0s' {1..64})
postgres_image_id=sha256:$(printf 'c%.0s' {1..64}); cloudflared_image_id=sha256:$(printf 'd%.0s' {1..64})
compose() {
  printf 'compose:%s\n' "$*" >>"$log"
  [[ "$4" != postgres ]] || printf '%s\n' "$postgres_id"
  [[ "$4" != cloudflared || "$scenario" == missing ]] || printf '%s\n' "$cloudflared_id"
}
docker() {
  printf 'docker:%s\n' "$*" >>"$log"
  if [[ "$1 $2" == 'image inspect' ]]; then [[ "$5" == postgres-image ]] && printf '%s\n' "$postgres_image_id" || printf '%s\n' "$cloudflared_image_id"; return; fi
  if [[ "$1" == inspect && "$4" == "$postgres_id" ]]; then printf '%s|postgres-image|%s|weather|postgres\n' "$postgres_id" "$postgres_image_id"; return; fi
  if [[ "$1" == inspect && "$4" == "$cloudflared_id" ]]; then
    [[ "$scenario" != wrong-config ]] || { printf '%s|wrong-image|%s|weather|cloudflared\n' "$cloudflared_id" "$cloudflared_image_id"; return; }
    printf '%s|cloudflare-image|sha256:%064d|weather|cloudflared\n' "$cloudflared_id" 9
  fi
}
sp_start_infrastructure /fixture.env
`}`;
    try {
      const result = runBash(context, body, [log, context.root, scenario]);
      assert.notEqual(result.status, 0, `${scenario} infrastructure unexpectedly accepted`);
      assert.doesNotMatch(await transcript(log), /docker:start/u);
    } finally {
      await rm(context.root, { force: true, recursive: true });
    }
  }
});

test("retained infrastructure can become healthy after the former ninety-second bound", async () => {
  const context = await fixture();
  const log = join(context.root, "infrastructure-delayed-health.log");
  const healthCount = join(context.root, "cloudflared-health-count");
  await writeFile(healthCount, "");
  const body = `${guards}\n${String.raw`
health_count=$4
postgres_id=$(printf 'a%.0s' {1..64}); cloudflared_id=$(printf 'b%.0s' {1..64})
postgres_image_id=sha256:$(printf 'c%.0s' {1..64}); cloudflared_image_id=sha256:$(printf 'd%.0s' {1..64})
compose() { [[ "$4" == postgres ]] && printf '%s\n' "$postgres_id" || printf '%s\n' "$cloudflared_id"; }
docker() {
  if [[ "$1 $2" == 'image inspect' ]]; then [[ "$5" == postgres-image ]] && printf '%s\n' "$postgres_image_id" || printf '%s\n' "$cloudflared_image_id"; return; fi
  if [[ "$1" == start ]]; then printf 'start:%s\n' "$2" >>"$log"; return; fi
  if [[ "$1" == inspect && "$3" == *State.Running* ]]; then
    [[ "$4" == "$postgres_id" ]] && { printf 'true|healthy\n'; return; }
    printf x >>"$health_count"
    (($(wc -c <"$health_count") >= 47)) && printf 'true|healthy\n' || printf 'true|starting\n'
    return
  fi
  [[ "$4" == "$postgres_id" ]] && printf '%s|postgres-image|%s|weather|postgres\n' "$postgres_id" "$postgres_image_id" || printf '%s|cloudflare-image|%s|weather|cloudflared\n' "$cloudflared_id" "$cloudflared_image_id"
}
sleep() { printf 'sleep:%s\n' "$1" >>"$log"; }
sp_start_infrastructure /fixture.env
`}`;
  try {
    const result = runBash(context, body, [log, context.root, healthCount]);
    assert.equal(result.status, 0, result.stderr);
    const output = await transcript(log);
    assert.equal((output.match(/sleep:2/gu) ?? []).length, 46);
    assert.match(output, /start:a{64}[\s\S]*start:b{64}/u);
    assert.doesNotMatch(output, /compose:up/u);
    assert.match(await readFile(context.operator, "utf8"), /for \(\(attempt=0; attempt<100; attempt\+\+\)\)/u);
  } finally {
    await rm(context.root, { force: true, recursive: true });
  }
});

test("system recovery boots retained infrastructure then the application trio", async () => {
  const context = await fixture();
  const log = join(context.root, "system-recover.log");
  await mkdir(context.spRoot, { mode: 0o700 });
  const environment = join(context.releases, "2026.11.03-3.env");
  await writeFile(environment, "source\n", { mode: 0o600 });
  await symlink(environment, join(context.state, "active.env"));
  const body = `${guards}\n${String.raw`
sp_environment() { release_env "$1"; }
read_release_state() { printf '2026.11.03-3\n'; }
sp_source() { release_env 2026.11.03-3; }
sp_start_infrastructure() { printf 'retained-start:%s\n' "$1" >>"$log"; }
sp_database() { printf 'database-readonly\n' >>"$log"; }
sp_health() { printf 'app-health\n' >>"$log"; }
docker() { printf 'docker:%s\n' "$*" >>"$log"; printf 'true|healthy\n'; }
compose() { printf 'compose:%s\n' "$*" >>"$log"; [[ "$1" != ps ]] || printf 'container\n'; }
sp_system_recover
`}`;
  try {
    const result = runBash(context, body, [log, context.root]);
    assert.equal(result.status, 0, result.stderr);
    const output = await transcript(log);
    assert.match(output, /retained-start:.*2026\.11\.03-3\.env[\s\S]*database-readonly[\s\S]*compose:up -d --no-deps --no-build --pull never --wait api worker[\s\S]*compose:up -d --no-deps --no-build --pull never --wait web/u);
    assert.doesNotMatch(output, /compose:up .*postgres|compose:up .*cloudflared/u);
    assert.doesNotMatch(output, /FORBIDDEN:|volume|clone|migrat/u);
  } finally {
    await rm(context.root, { force: true, recursive: true });
  }
});

test("wrong schema, source, or preexisting journal fails before side effects", async () => {
  // verify authority failures precede container or stage creation
  for (const failure of ["schema", "source", "journal"]) {
    const context = await fixture();
    const log = join(context.root, `${failure}.log`);
    await mkdir(context.spRoot, { mode: 0o700 });
    if (failure === "journal") await writeFile(join(context.spRoot, "journal.json"), "{}\n", { mode: 0o600 });
    const publication = join(context.root, "publication.json");
    await writeFile(publication, "{}\n", { mode: 0o600 });
    const body = `${guards}\n${String.raw`
failure=$4; publication=$5
sp_environment() { release_env "$1"; }
read_release_state() { if [[ "$1" == *schema-release && "$failure" == schema ]]; then printf wrong; else printf '2026.11.03-3'; fi; printf '\n'; }
sp_source() { [[ "$failure" != source ]] || die 'bad source'; release_env 2026.11.03-3; }
if [[ "$failure" == journal ]]; then sp_stage 2026.11.04-1 "$publication" "$(sp_sha "$publication")"; else sp_system_recover; fi
`}`;
    try {
      const result = runBash(context, body, [log, context.root, failure, publication]);
      assert.notEqual(result.status, 0);
      const output = await transcript(log);
      assert.doesNotMatch(output, /compose:|docker:|mkdir:|FORBIDDEN:/u);
    } finally {
      await rm(context.root, { force: true, recursive: true });
    }
  }
});

test("database verifier uses read-only SQL and adapts only the index checksum", async () => {
  const context = await fixture();
  const log = join(context.root, "database.log");
  const sql = join(context.root, "database.sql");
  const body = String.raw`
log=$2; sql=$3
die() { printf 'die:%s\n' "$*" >&2; exit 70; }
env_value() { printf 'weather\n'; }
compose() { cat >"$sql"; printf 't\n'; }
verify_forecast_product_read_index() { : e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a; }
verify_runtime_database_acl() { declare -f verify_forecast_product_read_index >>"$log"; }
before=$(declare -f verify_forecast_product_read_index)
sp_database /fixture.env
after=$(declare -f verify_forecast_product_read_index)
[[ "$before" == "$after" ]] || die 'original verifier changed'
`;
  try {
    const result = runBash(context, body, [log, sql]);
    assert.equal(result.status, 0, result.stderr);
    const query = await readFile(sql, "utf8");
    assert.match(query, /BEGIN READ ONLY;/u);
    assert.match(query, /total=23 AND digest='1cfd6251/u);
    assert.equal((query.match(/adjustment_revision_receipt_check/gu) ?? []).length, 4);
    assert.match(query, /NOT EXISTS[\s\S]*constraint_row\.contype<>'c'[\s\S]*constraint_row\.convalidated/u);
    assert.match(query, /weather_adjustment_revision_receipt_valid_v1\(adjustment_revision_receipt\)/u);
    assert.doesNotMatch(query, /\b(?:ALTER|CREATE|DELETE|DROP|INSERT|TRUNCATE|UPDATE)\b/iu);
    const output = await transcript(log);
    assert.match(output, /7e48acb14710a625cdcbb796bf1bf49ab4e00bf1aeb9bb763ac86410e08663e0/u);
    assert.doesNotMatch(output, /e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a/u);
  } finally {
    await rm(context.root, { force: true, recursive: true });
  }
});
