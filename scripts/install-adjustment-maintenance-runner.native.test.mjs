import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmod,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  readlink,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "..");
const installer = join(repoRoot, "scripts/install-adjustment-maintenance-runner.sh");
const sourceCommit = "1".repeat(40);
const executableNames = [
  "weather-maintenance-capture",
  "weather-maintenance-fit",
  "weather-maintenance-wind-fit",
];

// Run one sourced shell helper and return its required single-line identity.
function shellIdentity(expression, ...arguments_) {
  const result = spawnSync("bash", [
    "-c",
    `source "$1"; ${expression}`,
    "native-runner-identity-test",
    installer,
    ...arguments_,
  ], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

// Install one native candidate through disposable private and public roots.
function runNativeInstaller(fixture, binaryDigest = fixture.binaryDigest) {
  const command = `source "$1"
require_source_commit() { :; }
require_private_install_ancestors() { :; }
adjustment_require_public_unit_root() { [[ -d "$1" && ! -L "$1" ]]; }
adjustment_require_public_unit_file() {
  [[ -f "$1" && ! -L "$1" && "$(stat -c '%u:%a' -- "$1")" == "$(id -u):644" ]]
}
adjustment_public_install() { install -m 0644 -- "$1" "$2"; }
adjustment_public_move() { mv -Tf -- "$1" "$2"; }
adjustment_public_remove() { rm -f -- "$1"; }
require_loaded_public_unit() { :; }
require_user_manager_readiness() { :; }
reload_systemd() { :; }
install_adjustment_maintenance_runner "$2" "$3" "$4" "$5" "$6" native "$7" "$8"`;
  return spawnSync("bash", [
    "-c",
    command,
    "native-runner-install-test",
    installer,
    repoRoot,
    sourceCommit,
    fixture.sourceDigest,
    fixture.installRoot,
    fixture.unitRoot,
    fixture.binaryRoot,
    binaryDigest,
  ], {
    encoding: "utf8",
    env: { ...process.env, NATIVE_READINESS_LOG: fixture.readinessLog },
  });
}

// Build one exact three-binary candidate directory and isolated install roots.
async function createFixture() {
  const root = await mkdtemp(join(tmpdir(), "weather-native-runner-installer-"));
  const binaryRoot = join(root, "binaries");
  const installRoot = join(root, "install");
  const unitRoot = join(root, "units");
  const readinessLog = join(root, "readiness.log");
  await Promise.all([
    mkdir(binaryRoot, { mode: 0o700 }),
    mkdir(installRoot, { mode: 0o700 }),
    mkdir(unitRoot, { mode: 0o700 }),
  ]);
  const capture = `#!/usr/bin/env bash
printf '%s\n' "$*" >"\${NATIVE_READINESS_LOG:?}"
[[ "$#" -eq 1 && "$1" == "--readiness" ]]
`;
  await writeFile(join(binaryRoot, executableNames[0]), capture, { mode: 0o700 });
  // materialize both native fitter binaries as distinct digest-bound executables
  for (const name of executableNames.slice(1)) {
    await writeFile(join(binaryRoot, name), `#!/usr/bin/env bash\n# ${name}\nexit 0\n`, {
      mode: 0o700,
    });
  }
  const sourceDigest = shellIdentity(
    'adjustment_runner_manifest_digest "$2" native',
    repoRoot,
  );
  const binaryDigest = shellIdentity(
    'adjustment_native_binary_manifest_digest "$2"',
    binaryRoot,
  );
  return {
    binaryDigest,
    binaryRoot,
    installRoot,
    readinessLog,
    root,
    sourceDigest,
    unitRoot,
  };
}

// Thaw and remove only the disposable immutable release fixture.
async function cleanupFixture(fixture) {
  const thaw = spawnSync("chmod", ["-R", "u+w", fixture.root], { encoding: "utf8" });
  assert.equal(thaw.status, 0, thaw.stderr);
  await rm(fixture.root, { force: true, recursive: true });
}

test("explicit native runner installs digest-bound binaries and native units", async () => {
  const fixture = await createFixture();
  try {
    const result = runNativeInstaller(fixture);
    assert.equal(result.status, 0, result.stderr);
    const release = join(fixture.installRoot, "releases", sourceCommit);
    assert.equal(await readlink(join(fixture.installRoot, "current")), `releases/${sourceCommit}`);
    assert.equal(await readFile(fixture.readinessLog, "utf8"), "--readiness\n");
    const manifest = await readFile(join(release, "MANIFEST.sha256"), "utf8");
    assert.match(manifest, /rust\/crates\/weather-runtime\/src\/bin\/weather-maintenance-capture\.rs$/mu);
    // bind the native installation, readiness, snapshot and invocation owners
    for (const module of [
      "native_maintenance_capture_native_installation",
      "native_maintenance_fit_runtime_readiness",
      "native_maintenance_fit_snapshot",
      "native_maintenance_fit_sandbox",
    ]) {
      assert.match(manifest, new RegExp(`  rust/crates/weather-runtime/src/${module}\\.rs$`, "mu"));
    }
    // native installs no longer carry predecessor Python or JavaScript application sources
    assert.doesNotMatch(manifest, /scripts\/research\/.*\.(?:py|mjs|js)$/mu);
    // require every installed native binary to be manifest-bound and owner-executable only
    for (const name of executableNames) {
      assert.match(manifest, new RegExp(`  bin/${name}$`, "mu"));
      assert.equal((await lstat(join(release, "bin", name))).mode & 0o777, 0o500);
    }
    assert.equal((await lstat(join(release, "MANIFEST.sha256"))).mode & 0o777, 0o400);
    const archive = await readFile(
      join(fixture.unitRoot, "weather-adjustment-archive.service"),
      "utf8",
    );
    const daily = await readFile(
      join(fixture.unitRoot, "weather-adjustment-daily.service"),
      "utf8",
    );
    const monthly = await readFile(
      join(fixture.unitRoot, "weather-adjustment-monthly.service"),
      "utf8",
    );
    assert.match(archive, /^ExecStart=.*\/bin\/weather-maintenance-capture$/mu);
    assert.match(daily, /^ExecStart=.*\/bin\/weather-maintenance-capture --daily$/mu);
    assert.match(monthly, /^ExecStart=.*\/bin\/weather-maintenance-capture --monthly$/mu);
    assert.doesNotMatch(archive, /\/n\/bin\/node/u);
  } finally {
    await cleanupFixture(fixture);
  }
});

test("native runner refuses a different binary digest before publishing current", async () => {
  const fixture = await createFixture();
  try {
    const result = runNativeInstaller(fixture, "0".repeat(64));
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /native binary manifest differs/u);
    await assert.rejects(readlink(join(fixture.installRoot, "current")), { code: "ENOENT" });
  } finally {
    await cleanupFixture(fixture);
  }
});

test("predecessor units remain the default checked-in runner generation", async () => {
  const archive = await readFile(
    join(repoRoot, "scripts/systemd/system/weather-adjustment-archive.service"),
    "utf8",
  );
  assert.match(archive, /^ExecStart=\/home\/ubuntu\/n\/bin\/node /mu);
  const usage = spawnSync("bash", [installer, "--native"], { encoding: "utf8" });
  assert.equal(usage.status, 2);
  assert.match(usage.stderr, /SOURCE_COMMIT \| --native SOURCE_COMMIT BINARY_DIR/u);
});
