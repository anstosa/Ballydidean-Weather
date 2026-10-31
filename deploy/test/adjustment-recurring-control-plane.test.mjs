import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmod,
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rename,
  rm,
  stat,
  symlink,
  truncate,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "../..");
const installer = join(repoRoot, "scripts/install-adjustment-recurring-control-plane.sh");
const identityPacketPath = join(
  repoRoot,
  "deploy/test/fixtures/adjustment-v15-predecessor-identities.json",
);
const baselineCommit = "2026.10.14-1";
const recordedDigest = "603eb8f488ba78be3d7ecf76b0d587346d1c36432255d390d86d2b768c8fecba";
const predecessorDigest = "08b25ce2c80baadbc3f94c8e57a694387853c3a9bab06364c7e7906e5e04de40";
const predecessorRelease = "2026.10.09-1";
const changedPaths = [
  "deploy/postgres/runtime-acl-v2.sql",
  "deploy/scripts/adjustment-evaluation-package.mjs",
  "deploy/scripts/adjustment-evidence-store.mjs",
  "deploy/scripts/backup.sh",
  "deploy/scripts/common.sh",
  "deploy/scripts/public-stations-backfill.sh",
  "deploy/scripts/tempest-backfill.sh",
  "deploy/scripts/tide-backfill.sh",
  "deploy/scripts/update.sh",
  "deploy/scripts/web-server.mjs",
];
const partialPaths = [
  "deploy/postgres/runtime-acl-v2.sql",
  "deploy/scripts/adjustment-evaluation-package.mjs",
  "deploy/scripts/adjustment-evidence-store.mjs",
  "deploy/scripts/common.sh",
];

const identityPacketBytes = await readFile(identityPacketPath);
assert.equal(
  createHash("sha256").update(identityPacketBytes).digest("hex"),
  "49509e8b4acbe529f4f4e57eb20e8c873a26cba44e70cb8b7852650bbf758b53",
  "reviewed predecessor fixture must retain its frozen identity",
);
const identityPacket = JSON.parse(identityPacketBytes);
const identities = Object.entries(identityPacket)
  .map(([absolutePath, identity]) => ({
    ...identity,
    relativePath: absolutePath.replace("/opt/weather/current/", ""),
  }))
  .sort((left, right) => left.relativePath.localeCompare(right.relativePath));

// hash one file without loading it into test assertions
async function fileHash(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

// read one frozen predecessor file through git
function gitShowBaseline(relativePath) {
  const commit = relativePath === "deploy/scripts/web-server.mjs"
    ? "6db596783035597792878c400004fb464bbb9739"
    : baselineCommit;
  return execFileSync("git", ["show", `${commit}:${relativePath}`], {
    cwd: repoRoot,
  });
}

// materialize the exact reviewed predecessor from git-show bytes
async function writeBaselineTree(root) {
  // reproduce every reviewed identity without reading dirty worktree bytes
  for (const identity of identities) {
    const target = join(root, identity.relativePath);
    const bytes = gitShowBaseline(identity.relativePath);
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, bytes, { mode: identity.mode });
    await chmod(target, identity.mode);
    assert.equal(await fileHash(target), identity.sha256);
  }
}

// alter one reviewed path while retaining its file syntax
async function changeCandidatePath(candidate, relativePath) {
  const path = join(candidate, relativePath);
  const original = await readFile(path, "utf8");
  let updated;
  // bind update to the exact v15 successor
  if (relativePath === "deploy/scripts/update.sh") {
    updated = original
      .replace("control_plane_version=14", "control_plane_version=15");
  } else if (relativePath.endsWith(".sql")) {
    updated = `${original}\n-- maintenance-v15-fixture\n`;
  } else if (relativePath.endsWith(".mjs")) {
    updated = `${original}\n// maintenance-v15-fixture\n`;
  } else {
    updated = `${original}\n# maintenance-v15-fixture\n`;
  }
  await writeFile(path, updated);
}

// package one exact public control candidate
async function refreshArchive(fixture) {
  execFileSync("tar", [
    "-cf",
    fixture.archive,
    "-C",
    fixture.candidate,
    "deploy/scripts",
    "deploy/postgres",
    "deploy/systemd",
    "deploy/sudoers",
    "deploy/compose.yaml",
  ]);
  fixture.archiveSha256 = await fileHash(fixture.archive);
  const digest = spawnSync("bash", [
    "-c",
    'source "$1"; control_digest "$2"',
    "maintenance-v15-digest",
    installer,
    fixture.candidate,
  ], { cwd: repoRoot, encoding: "utf8" });
  assert.equal(digest.status, 0, digest.stderr);
  fixture.candidateDigest = digest.stdout.trim();
}

// create one full or partial v15 installation fixture
async function createFixture({ partial = false, actualCandidate = false } = {}) {
  const root = await mkdtemp(join(tmpdir(), "weather-maintenance-v15-"));
  const baseline = join(root, "baseline");
  const installed = join(root, "installed");
  const candidate = join(root, "candidate");
  const runtime = join(root, "runtime");
  const backups = join(runtime, "control-plane-backups");
  const archive = join(root, "candidate.tar");
  await Promise.all([
    mkdir(baseline),
    mkdir(installed),
    mkdir(candidate),
    mkdir(runtime),
  ]);
  await writeBaselineTree(baseline);
  await cp(join(baseline, "deploy"), join(installed, "deploy"), { recursive: true });
  await cp(join(baseline, "deploy"), join(candidate, "deploy"), { recursive: true });

  // overlay either the actual partial worktree slice or all reviewed changes
  for (const relativePath of partial ? partialPaths : changedPaths) {
    if (partial || actualCandidate) {
      await cp(join(repoRoot, relativePath), join(candidate, relativePath));
      const identity = identities.find((entry) => entry.relativePath === relativePath);
      await chmod(join(candidate, relativePath), identity.mode);
    } else {
      await changeCandidatePath(candidate, relativePath);
    }
  }

  await Promise.all([
    mkdir(join(installed, "deploy/state")),
    mkdir(join(installed, "deploy/releases")),
    mkdir(join(runtime, "xweather/adjustment-evidence"), { recursive: true }),
    mkdir(join(runtime, "xweather/adjustment-release-state"), { recursive: true }),
  ]);
  const releaseBytes = [
    `WEATHER_RELEASE=${predecessorRelease}`,
    `WEATHER_CONTROL_PLANE_SHA256=${recordedDigest}`,
    "WEATHER_CONTROL_PLANE_VERSION=13",
    "WEATHER_SERVER_IMAGE=registry.example/weather-server@sha256:baseline",
    "WEATHER_WEB_IMAGE=registry.example/weather-web@sha256:baseline",
    "",
  ].join("\n");
  await Promise.all([
    writeFile(
      join(installed, "deploy/state/current-release"),
      `${predecessorRelease}\n`,
      { mode: 0o600 },
    ),
    writeFile(
      join(installed, `deploy/releases/${predecessorRelease}.env`),
      releaseBytes,
      { mode: 0o600 },
    ),
    writeFile(
      join(runtime, "xweather/adjustment-evidence/current.json"),
      "evidence-before\n",
      { mode: 0o600 },
    ),
    writeFile(
      join(runtime, "xweather/adjustment-release-state/current.json"),
      "release-before\n",
      { mode: 0o600 },
    ),
  ]);
  const fixture = {
    archive,
    archiveSha256: "",
    backups,
    baseline,
    candidate,
    candidateDigest: "",
    installed,
    root,
    runtime,
  };
  await refreshArchive(fixture);
  return fixture;
}

// execute the sourced installer against a disposable deployment
function runInstaller(fixture, failureStep = 0, { concurrentCapture = false, corruptBackup = false } = {}) {
  const hook = concurrentCapture
    ? `installation_failure_probe() {
  printf 'committed-live-capture\\n' >"$2/xweather/adjustment-evidence/live-capture.json"
  chmod 600 "$2/xweather/adjustment-evidence/live-capture.json"
}`
    : corruptBackup
      ? `installation_failure_probe() {
  local backup
  backup=$(find "$2/control-plane-backups" -maxdepth 1 -type d -name 'v15-*' -print -quit)
  printf 'untrusted-backup\\n' >>"$backup/control/deploy/scripts/common.sh"
}`
      : "installation_failure_probe() { :; }";
  return spawnSync("bash", [
    "-c",
    `source "$1"
require_quiet_weather() { :; }
${hook}
install_v15_control_plane "$2" "$3" "$4" "$5" "$6" "$7" "$8" "$9" "\${10}" "\${11}" "\${12}"`,
    "maintenance-v15-install",
    installer,
    fixture.installed,
    fixture.backups,
    fixture.runtime,
    fixture.archive,
    fixture.archiveSha256,
    fixture.candidateDigest,
    String(failureStep),
    String(process.getuid()),
    String(process.getgid()),
    String(process.getuid()),
    String(process.getgid()),
  ], { cwd: repoRoot, encoding: "utf8" });
}

// execute explicit recovery through the fixed backup root
function runRecovery(fixture, backup) {
  return spawnSync("bash", [
    "-c",
    `source "$1"
require_quiet_weather() { :; }
recover_v15_control_plane "$2" "$3" "$4" "$5" "$6" "$7" "$8" "$9"`,
    "maintenance-v15-recover",
    installer,
    fixture.installed,
    fixture.backups,
    fixture.runtime,
    backup,
    String(process.getuid()),
    String(process.getgid()),
    String(process.getuid()),
    String(process.getgid()),
  ], { cwd: repoRoot, encoding: "utf8" });
}

// find the one retained sealed transaction directory
async function retainedBackup(fixture) {
  const backupName = (await readdir(fixture.backups))
    .find((name) => name.startsWith("v15-"));
  assert.ok(backupName);
  return join(fixture.backups, backupName);
}

// hold the fixed installer lock in one separate process
function holdTransactionLock(fixture) {
  const ready = join(fixture.root, `lock-ready-${Date.now()}`);
  const lock = join(fixture.backups, ".adjustment-maintenance-control-install.lock");
  const holder = spawn("bash", [
    "-c",
    'exec 9>"$1"; flock 9; printf ready >"$2"; sleep 60',
    "maintenance-v15-lock-holder",
    lock,
    ready,
  ], { cwd: repoRoot, stdio: "ignore" });
  execFileSync("bash", [
    "-c",
    'for _ in {1..100}; do [[ -f "$1" ]] && exit 0; sleep 0.05; done; exit 1',
    "maintenance-v15-lock-wait",
    ready,
  ]);
  return holder;
}

// stop one lock holder and wait for release
async function stopLockHolder(holder) {
  const exited = new Promise((resolveExit) => holder.once("exit", resolveExit));
  holder.kill("SIGTERM");
  await exited;
}

// require the fixture to retain the exact predecessor
function assertPredecessor(fixture) {
  const verified = spawnSync("bash", [
    "-c",
    'source "$1"; verify_predecessor_identities "$2" "$3" "$4"',
    "maintenance-v15-predecessor",
    installer,
    fixture.installed,
    String(process.getuid()),
    String(process.getgid()),
  ], { cwd: repoRoot, encoding: "utf8" });
  assert.equal(verified.status, 0, verified.stderr);
}

// require both affected runtime roots to retain their original bytes
async function assertRuntimeState(fixture) {
  assert.equal(
    await readFile(join(fixture.runtime, "xweather/adjustment-evidence/current.json"), "utf8"),
    "evidence-before\n",
  );
  assert.equal(
    await readFile(
      join(fixture.runtime, "xweather/adjustment-release-state/current.json"),
      "utf8",
    ),
    "release-before\n",
  );
}

test("v15 embeds the frozen 42-path predecessor packet and 10-path order", () => {
  const manifest = execFileSync("bash", [
    "-c",
    "source \"$1\"; predecessor_identity_manifest; printf '%s\\n' --changes--; maintenance_changed_path_manifest",
    "maintenance-v15-manifest",
    installer,
  ], { cwd: repoRoot, encoding: "utf8" });
  const [identityText, changeText] = manifest.split("--changes--\n");
  const embedded = identityText.trim().split("\n");
  assert.equal(embedded.length, 42);
  // compare every embedded identity to the frozen packet
  for (const line of embedded) {
    const [sha256, uid, gid, mode, relativePath] = line.split(" ");
    const expected = identityPacket[`/opt/weather/current/${relativePath}`];
    assert.deepEqual(
      { gid: Number(gid), mode: Number.parseInt(mode, 8), sha256, uid: Number(uid) },
      expected,
    );
  }
  assert.deepEqual(changeText.trim().split("\n"), changedPaths);
});

test("v15 accepts exactly 10 changed paths and retains a complete recovery backup", async () => {
  const fixture = await createFixture();
  try {
    const result = runInstaller(fixture);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Installed inert maintenance v15 control plane/u);
    const installedDigest = execFileSync("bash", [
      "-c",
      'source "$1"; control_digest "$2"',
      "maintenance-v15-installed-digest",
      installer,
      fixture.installed,
    ], { cwd: repoRoot, encoding: "utf8" }).trim();
    assert.equal(installedDigest, fixture.candidateDigest);
    assert.notEqual(installedDigest, predecessorDigest);
    await assertRuntimeState(fixture);
    const backupNames = (await readdir(fixture.backups))
      .filter((name) => name.startsWith("v15-"));
    assert.equal(backupNames.length, 1);
    const backup = join(fixture.backups, backupNames[0]);
    const manifestLines = (
      await readFile(join(backup, "v15-identities.txt"), "utf8")
    ).trim().split("\n");
    assert.equal(manifestLines.length, 42);
    assert.equal(
      await readFile(join(backup, "v15-control.sha256"), "utf8"),
      `${fixture.candidateDigest}\n`,
    );
    const seal = await readFile(join(backup, "sealed-transaction.manifest"), "utf8");
    assert.match(seal, /contractVersion=adjustment-maintenance-control-recovery\/v2/u);
    assert.match(seal, new RegExp(`predecessorControlSha256=${predecessorDigest}`, "u"));
    assert.match(seal, new RegExp(`recordedSourceControlSha256=${recordedDigest}`, "u"));
    assert.match(seal, /state0Identity=[a-f0-9]{64}/u);
    assert.match(seal, /state1Identity=[a-f0-9]{64}/u);
    assert.ok((await stat(backup)).isDirectory());
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("v15 installs the actual complete worktree candidate and recovers its predecessor", async () => {
  const fixture = await createFixture({ actualCandidate: true });
  try {
    const result = runInstaller(fixture);
    assert.equal(result.status, 0, result.stderr);
    // verify every actual candidate byte rather than synthetic fixture comments
    for (const relativePath of changedPaths) {
      assert.equal(
        await fileHash(join(fixture.installed, relativePath)),
        await fileHash(join(fixture.candidate, relativePath)),
      );
    }
    await assertRuntimeState(fixture);
    const recovered = runRecovery(fixture, await retainedBackup(fixture));
    assert.equal(recovered.status, 0, recovered.stderr);
    assertPredecessor(fixture);
    await assertRuntimeState(fixture);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("v15 restores all identities and preserves untouched state after every injected backup or replacement failure", async () => {
  const fixture = await createFixture();
  try {
    // cover both state-backup probes and every one of ten replacements
    for (let failureStep = 1; failureStep <= 12; failureStep += 1) {
      const result = runInstaller(fixture, failureStep);
      assert.notEqual(result.status, 0, `failure step ${failureStep} unexpectedly passed`);
      assert.match(result.stderr, /failure probe/u);
      assertPredecessor(fixture);
      await assertRuntimeState(fixture);
    }
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// never erase a response-finish capture when reverting inert control files
test("v15 failed replacement preserves a concurrent committed live capture", async () => {
  const fixture = await createFixture();
  try {
    const result = runInstaller(fixture, 3, { concurrentCapture: true });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /live runtime state advanced; preserved without rollback/u);
    assertPredecessor(fixture);
    await assertRuntimeState(fixture);
    assert.equal(
      await readFile(join(fixture.runtime, "xweather/adjustment-evidence/live-capture.json"), "utf8"),
      "committed-live-capture\n",
    );
    assert.ok(await retainedBackup(fixture));
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// retained snapshots never authorize deleting later live evidence
test("v15 explicit recovery preserves live captures added after installation", async () => {
  const fixture = await createFixture();
  try {
    const installed = runInstaller(fixture);
    assert.equal(installed.status, 0, installed.stderr);
    const liveCapture = join(fixture.runtime, "xweather/adjustment-evidence/live-capture.json");
    await writeFile(liveCapture, "committed-live-capture\n", { mode: 0o600 });
    const recovered = runRecovery(fixture, await retainedBackup(fixture));
    assert.notEqual(recovered.status, 0);
    assert.match(recovered.stderr, /live runtime state advanced; preserved without rollback/u);
    assert.doesNotMatch(recovered.stdout, /Recovered maintenance predecessor control plane/u);
    assertPredecessor(fixture);
    await assertRuntimeState(fixture);
    assert.equal(await readFile(liveCapture, "utf8"), "committed-live-capture\n");
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// automatic recovery must reject a damaged seal before any restoration write
test("v15 automatic rollback fails closed before writes when its sealed backup changes", async () => {
  const fixture = await createFixture();
  try {
    const result = runInstaller(fixture, 3, { corruptBackup: true });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /predecessor identity differs|sealed recovery/u);
    // only the first legitimate installation replacement may remain
    for (const identity of identities) {
      const expectedHash = identity.relativePath === changedPaths[0]
        ? await fileHash(join(fixture.candidate, identity.relativePath))
        : identity.sha256;
      assert.equal(await fileHash(join(fixture.installed, identity.relativePath)), expectedHash);
    }
    await assertRuntimeState(fixture);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("v15 retained backup supports explicit complete recovery", async () => {
  const fixture = await createFixture();
  try {
    const installed = runInstaller(fixture);
    assert.equal(installed.status, 0, installed.stderr);
    const recovered = runRecovery(fixture, await retainedBackup(fixture));
    assert.equal(recovered.status, 0, recovered.stderr);
    assertPredecessor(fixture);
    await assertRuntimeState(fixture);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// removing v15 controls must not orphan an active or interrupted dashboard
test("v15 control recovery requires dashboard rollback before any restoration", async () => {
  const fixture = await createFixture();
  try {
    assert.equal(runInstaller(fixture).status, 0);
    const backup = await retainedBackup(fixture);
    const installedBytes = await readFile(join(fixture.installed, "deploy/scripts/update.sh"));
    // every dashboard authority marker independently blocks control recovery
    for (const marker of ["current-web-release", "previous-web-release", "active-web.env", "dashboard-web-transaction.env"]) {
      const path = join(fixture.installed, "deploy/state", marker);
      await writeFile(path, "2026.10.09-4\n", { mode: 0o600 });
      const recovered = runRecovery(fixture, backup);
      assert.notEqual(recovered.status, 0);
      assert.match(recovered.stderr, /dashboard rollback is required/u);
      assert.deepEqual(await readFile(join(fixture.installed, "deploy/scripts/update.sh")), installedBytes);
      await rm(path);
    }
    assert.equal(runRecovery(fixture, backup).status, 0);
    assertPredecessor(fixture);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("v15 refuses a four-path partial candidate without mutation", async () => {
  const fixture = await createFixture({ partial: true });
  try {
    const result = runInstaller(fixture);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /must change exactly 10 reviewed paths; found 4/u);
    assertPredecessor(fixture);
    await assertRuntimeState(fixture);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("v15 refuses predecessor drift before creating a recovery backup", async () => {
  const fixture = await createFixture();
  try {
    await writeFile(
      join(fixture.installed, "deploy/scripts/common.sh"),
      "drifted\n",
    );
    const result = runInstaller(fixture);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /predecessor identity differs: deploy\/scripts\/common\.sh/u);
    await assert.rejects(readdir(fixture.backups));
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("v15 final quiet-window recheck preserves foreign control drift without replacement", async () => {
  const fixture = await createFixture();
  try {
    const result = spawnSync("bash", [
      "-c",
      `source "$1"
test_destination=$2
require_quiet_weather() {
  # inject an unrelated write after sealing but before replacement
  if [[ -n "\${backup:-}" && -f "$backup/sealed-transaction.manifest" ]]; then
    printf 'drifted\\n' >"$test_destination/deploy/scripts/common.sh"
  fi
}
install_v15_control_plane "$2" "$3" "$4" "$5" "$6" "$7" 0 "$8" "$9" "$8" "$9"`,
      "maintenance-v15-reread",
      installer,
      fixture.installed,
      fixture.backups,
      fixture.runtime,
      fixture.archive,
      fixture.archiveSha256,
      fixture.candidateDigest,
      String(process.getuid()),
      String(process.getgid()),
    ], { cwd: repoRoot, encoding: "utf8" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /predecessor identity differs: deploy\/scripts\/common\.sh/u);
    assert.equal(
      await readFile(join(fixture.installed, "deploy/scripts/common.sh"), "utf8"),
      "drifted\n",
    );
    // no other control member may be replaced or adopted after foreign drift
    for (const identity of identities.filter((entry) => entry.relativePath !== "deploy/scripts/common.sh")) {
      assert.equal(await fileHash(join(fixture.installed, identity.relativePath)), identity.sha256);
    }
    assert.doesNotMatch(result.stdout, /Installed inert maintenance v15 control plane/u);
    await assertRuntimeState(fixture);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("v15 refuses unreviewed control changes and added privileged paths", async (context) => {
  await context.test("unreviewed existing path", async () => {
    const fixture = await createFixture();
    try {
      await writeFile(
        join(fixture.candidate, "deploy/scripts/weather-admin-store.mjs"),
        "// unreviewed\n",
        { flag: "a" },
      );
      await refreshArchive(fixture);
      const result = runInstaller(fixture);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /changes an unreviewed path/u);
      assertPredecessor(fixture);
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  });

  await context.test("new privileged path", async () => {
    const fixture = await createFixture();
    try {
      await writeFile(
        join(fixture.candidate, "deploy/scripts/new-maintenance-helper.sh"),
        "#!/usr/bin/env bash\n",
        { mode: 0o755 },
      );
      await refreshArchive(fixture);
      const result = runInstaller(fixture);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /closed control surface/u);
      assertPredecessor(fixture);
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  });
});

test("v15 refuses a recovery source over four GiB", async () => {
  const fixture = await createFixture();
  try {
    const oversized = join(fixture.runtime, "xweather/adjustment-evidence/oversized.bin");
    await writeFile(oversized, "");
    await truncate(oversized, (4 * 1024 * 1024 * 1024) + 1);
    const result = runInstaller(fixture);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /recovery source exceeds four GiB/u);
    assertPredecessor(fixture);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("v15 rejects hidden control links, fifos, and empty directories", async (context) => {
  const scenarios = [
    {
      name: "symlink",
      // add one hidden link beneath the installed control tree
      mutate: async (fixture) => await symlink(
        "/tmp/never-follow-weather-control",
        join(fixture.installed, "deploy/scripts/hidden-link"),
      ),
    },
    {
      name: "fifo",
      // add one hidden fifo beneath the installed control tree
      mutate: async (fixture) => execFileSync(
        "mkfifo",
        [join(fixture.installed, "deploy/scripts/hidden-fifo")],
      ),
    },
    {
      name: "empty directory",
      // add one hidden directory beneath the installed control tree
      mutate: async (fixture) => await mkdir(
        join(fixture.installed, "deploy/scripts/hidden-directory"),
      ),
    },
  ];
  // exercise each filesystem object independently
  for (const scenario of scenarios) {
    await context.test(scenario.name, async () => {
      const fixture = await createFixture();
      try {
        await scenario.mutate(fixture);
        const result = runInstaller(fixture);
        assert.notEqual(result.status, 0);
        assert.match(result.stderr, /control surface contains an unreviewed entry/u);
        await assertRuntimeState(fixture);
      } finally {
        await rm(fixture.root, { force: true, recursive: true });
      }
    });
  }
});

test("v15 rejects linked control and runtime ancestors", async (context) => {
  await context.test("linked control parent", async () => {
    const fixture = await createFixture();
    try {
      const escaped = join(fixture.root, "escaped-scripts");
      await rename(join(fixture.installed, "deploy/scripts"), escaped);
      await symlink(escaped, join(fixture.installed, "deploy/scripts"));
      const result = runInstaller(fixture);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /deploy\/scripts is missing, linked or noncanonical/u);
      assert.equal(await fileHash(join(escaped, "common.sh")), identityPacket[
        "/opt/weather/current/deploy/scripts/common.sh"
      ].sha256);
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  });

  await context.test("linked runtime parent", async () => {
    const fixture = await createFixture();
    try {
      const escaped = join(fixture.root, "escaped-xweather");
      await rename(join(fixture.runtime, "xweather"), escaped);
      await symlink(escaped, join(fixture.runtime, "xweather"));
      const result = runInstaller(fixture);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /xweather state parent is missing, linked or noncanonical/u);
      assertPredecessor(fixture);
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  });
});

test("v15 recovery refuses ancestry drift before any mutation", async () => {
  const fixture = await createFixture();
  try {
    const installed = runInstaller(fixture);
    assert.equal(installed.status, 0, installed.stderr);
    const candidateDigestBefore = execFileSync("bash", [
      "-c",
      'source "$1"; control_digest "$2"',
      "maintenance-v15-before-ancestry-refusal",
      installer,
      fixture.installed,
    ], { cwd: repoRoot, encoding: "utf8" }).trim();
    const escaped = join(fixture.root, "escaped-evidence");
    await rename(join(fixture.runtime, "xweather/adjustment-evidence"), escaped);
    await symlink(escaped, join(fixture.runtime, "xweather/adjustment-evidence"));
    const recovered = runRecovery(fixture, await retainedBackup(fixture));
    assert.notEqual(recovered.status, 0);
    assert.match(
      recovered.stderr,
      /adjustment evidence state root is missing, linked or noncanonical/u,
    );
    const candidateDigestAfter = execFileSync("bash", [
      "-c",
      'source "$1"; control_digest "$2"',
      "maintenance-v15-after-ancestry-refusal",
      installer,
      fixture.installed,
    ], { cwd: repoRoot, encoding: "utf8" }).trim();
    assert.equal(candidateDigestAfter, candidateDigestBefore);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("v15 recovery accepts only a complete fixed-root sealed transaction", async (context) => {
  const fixture = await createFixture();
  try {
    const installed = runInstaller(fixture);
    assert.equal(installed.status, 0, installed.stderr);
    const backup = await retainedBackup(fixture);
    const candidateDigestBefore = execFileSync("bash", [
      "-c",
      'source "$1"; control_digest "$2"',
      "maintenance-v15-seal-before",
      installer,
      fixture.installed,
    ], { cwd: repoRoot, encoding: "utf8" }).trim();

    await context.test("caller-imported path", async () => {
      const imported = join(fixture.root, "imported-backup");
      await cp(backup, imported, { recursive: true });
      const result = runRecovery(fixture, imported);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /not a fixed-root transaction/u);
    });

    await context.test("copied direct child", async () => {
      const copied = join(
        fixture.backups,
        `v15-${predecessorDigest.slice(0, 12)}.ZYXWVUTS`,
      );
      await cp(backup, copied, { recursive: true });
      const result = runRecovery(fixture, copied);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /sealed recovery transaction manifest differs/u);
    });

    await context.test("old v14 transaction name", async () => {
      const oldTransaction = join(
        fixture.backups,
        `v14-${predecessorDigest.slice(0, 12)}.OLDV14TX`,
      );
      await cp(backup, oldTransaction, { recursive: true });
      const result = runRecovery(fixture, oldTransaction);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /not a fixed-root transaction/u);
    });

    await context.test("linked direct child", async () => {
      const linked = join(
        fixture.backups,
        `v15-${predecessorDigest.slice(0, 12)}.ABCDEFGH`,
      );
      await symlink(backup, linked);
      const result = runRecovery(fixture, linked);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /recovery transaction is missing, linked or noncanonical/u);
    });

    await context.test("incomplete direct child", async () => {
      const incomplete = join(
        fixture.backups,
        `v15-${predecessorDigest.slice(0, 12)}.IJKLMNO1`,
      );
      await cp(backup, incomplete, { recursive: true });
      await rm(join(incomplete, "state/1.identity"));
      const result = runRecovery(fixture, incomplete);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /sealed recovery member is missing or linked/u);
    });

    await context.test("oversized direct child", async () => {
      const oversized = join(
        fixture.backups,
        `v15-${predecessorDigest.slice(0, 12)}.PQRSTUVW`,
      );
      await cp(backup, oversized, { recursive: true });
      const oversizedMember = join(oversized, "oversized.bin");
      await writeFile(oversizedMember, "");
      await truncate(oversizedMember, (4 * 1024 * 1024 * 1024) + 1);
      const result = runRecovery(fixture, oversized);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /recovery transaction exceeds four GiB/u);
    });

    const candidateDigestAfter = execFileSync("bash", [
      "-c",
      'source "$1"; control_digest "$2"',
      "maintenance-v15-seal-after",
      installer,
      fixture.installed,
    ], { cwd: repoRoot, encoding: "utf8" }).trim();
    assert.equal(candidateDigestAfter, candidateDigestBefore);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("v15 install and recovery refuse a concurrently held transaction lock", async () => {
  const fixture = await createFixture();
  try {
    const installed = runInstaller(fixture);
    assert.equal(installed.status, 0, installed.stderr);
    const backup = await retainedBackup(fixture);
    let holder = holdTransactionLock(fixture);
    try {
      const recovery = runRecovery(fixture, backup);
      assert.notEqual(recovery.status, 0);
      assert.match(recovery.stderr, /another maintenance control transaction is in flight/u);
    } finally {
      await stopLockHolder(holder);
    }
    const recovered = runRecovery(fixture, backup);
    assert.equal(recovered.status, 0, recovered.stderr);
    holder = holdTransactionLock(fixture);
    try {
      const secondInstall = runInstaller(fixture);
      assert.notEqual(secondInstall.status, 0);
      assert.match(secondInstall.stderr, /another maintenance control installer is in flight/u);
      assertPredecessor(fixture);
    } finally {
      await stopLockHolder(holder);
    }
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

// enforce a backup allocation peak without reducing the protected floor
test("v15 backup admission holds exact byte and inode floors", () => {
  const harness = `source "$1"
maintenance_filesystem_sample() { printf '%s\\n' "$SAMPLE_BYTES $SAMPLE_INODES"; }
require_maintenance_capacity /unused 1048576 128`;
  // admit equality but never one byte or inode less
  for (const [bytes, inodes, accepted] of [
    [2_035_204_096, 32_896, true],
    [2_035_204_095, 32_896, false],
    [2_035_204_096, 32_895, false],
  ]) {
    const result = spawnSync("bash", ["-c", harness, "v15-capacity", installer], {
      encoding: "utf8",
      env: { ...process.env, SAMPLE_BYTES: String(bytes), SAMPLE_INODES: String(inodes) },
    });
    assert.equal(result.status === 0, accepted, result.stderr);
  }
});

// actual forced-command executables have an independent pinned pair transaction
test("v15 SSH mirrors replace only exact actual predecessors and refuse drift", async () => {
  const fixture = await createFixture();
  const mirrors = join(fixture.root, "mirrors");
  const dispatcher = join(mirrors, "dispatcher");
  const operations = join(mirrors, "operations");
  try {
    await mkdir(mirrors, { mode: 0o700 });
    await mkdir(fixture.backups, { mode: 0o700 });
    await writeFile(dispatcher, gitShowBaseline("deploy/scripts/ssh-dispatch.sh"), { mode: 0o755 });
    await writeFile(operations, gitShowBaseline("deploy/scripts/remote-ops.sh"), { mode: 0o755 });
    await chmod(dispatcher, 0o755);
    await chmod(operations, 0o755);
    // invoke only the bounded pair helper with isolated owner-controlled paths
    const install = () => spawnSync("bash", ["-c",
      'source "$1"; install_v15_ssh_mirrors "$2" "$3" "$4" "$5" "$6" "$(id -u)" "$(id -g)"',
      "v15-mirror-test", installer, fixture.candidate, fixture.backups, dispatcher,
      operations, fixture.candidateDigest], { cwd: repoRoot, encoding: "utf8" });
    const replaced = install();
    assert.equal(replaced.status, 0, replaced.stderr);
    assert.equal(await fileHash(dispatcher), await fileHash(join(fixture.candidate, "deploy/scripts/ssh-dispatch.sh")));
    assert.equal(await fileHash(operations), await fileHash(join(fixture.candidate, "deploy/scripts/remote-ops.sh")));
    const retry = install();
    assert.equal(retry.status, 0, retry.stderr);
    assert.match(retry.stdout, /already match/u);
    await writeFile(dispatcher, "#!/usr/bin/env bash\n# foreign drift\n");
    const foreignBytes = await readFile(dispatcher);
    assert.notEqual(install().status, 0);
    assert.deepEqual(await readFile(dispatcher), foreignBytes);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});
