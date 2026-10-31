import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "../..");
const predecessorDigest = "603eb8f488ba78be3d7ecf76b0d587346d1c36432255d390d86d2b768c8fecba";
const installedDigest = "a".repeat(64);

// test the production handoff function without starting a release
test("v15 accepts only its exact installed contract or pinned v13 predecessor", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-v13-handoff-"));
  const envFile = join(directory, "release.env");
  const harness = `
source "$1"
# isolate the exact installed digest
control_plane_digest() { printf '%s\\n' "$INSTALLED_DIGEST"; }
require_control_plane_compatibility "$2"
`;

  // run one immutable release identity against the real shell gate
  const check = async (version, digest, release) => {
    await writeFile(envFile, [
      `WEATHER_CONTROL_PLANE_VERSION=${version}`,
      `WEATHER_CONTROL_PLANE_SHA256=${digest}`,
      `WEATHER_RELEASE=${release}`,
      "",
    ].join("\n"));
    return spawnSync("bash", [
      "-c", harness, "v13-handoff", join(root, "deploy/scripts/update.sh"), envFile,
    ], { encoding: "utf8", env: { ...process.env, INSTALLED_DIGEST: installedDigest } });
  };

  try {
    const installed = await check(15, installedDigest, "2026.10.09-2");
    assert.equal(installed.status, 0, installed.stderr);
    const full = await check(15, installedDigest, "2026.10.09-3");
    assert.equal(full.status, 0, full.stderr);
    const predecessor = await check(13, predecessorDigest, "2026.10.09-1");
    assert.equal(predecessor.status, 0, predecessor.stderr);

    // refuse incorrect release labels, versions and immutable byte identities
    for (const [version, digest, release] of [
      [12, predecessorDigest, "2026.10.07-2"],
      [11, predecessorDigest, "2026.10.07-3"],
      [13, predecessorDigest, "2026.10.07-3"],
      [12, installedDigest, "2026.10.07-3"],
      [15, "b".repeat(64), "2026.10.09-2"],
      [12, "5685ead4440468ecdb58402fbe14e76ce63ed03a018b9b4724fa24ffef4eb3c8", "2026.10.07-2"],
    ]) {
      const result = await check(version, digest, release);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /unsupported/u);
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// exercise exact forced grammar with a nonprivileged sudo stand-in
test("v13 forced dispatch forwards only canonical bounded v2 operands", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-v13-dispatch-"));
  const bin = join(directory, "bin");
  const log = join(directory, "arguments");
  await mkdir(bin);
  await writeFile(join(bin, "sudo"), "#!/usr/bin/env bash\nprintf '%s\\0' \"$@\" >\"$DISPATCH_LOG\"\n");
  await chmod(join(bin, "sudo"), 0o755);
  const sha = "c".repeat(64);
  const access = "d".repeat(64);

  // invoke only the forced dispatcher against local executable stubs
  const dispatch = (command) => spawnSync("bash", [
    join(root, "deploy/scripts/ssh-dispatch.sh"),
  ], {
    encoding: "utf8",
    env: {
      ...process.env,
      DISPATCH_LOG: log,
      PATH: `${bin}:${process.env.PATH}`,
      SSH_ORIGINAL_COMMAND: command,
    },
  });

  try {
    // preserve canonical v1 commands alongside each exact v2 surface
    for (const command of [
      "adjustment-maintenance-anchor-status-v2",
      "adjustment-development-custody-anchor-current-v1",
      "adjustment-rain-control-custody-anchor-current-v1",
      "adjustment-unsupported-terminal-proof-current-v1",
      "adjustment-future-input-seal-current-v2",
      "adjustment-maintenance-anchor-current-v3",
      "adjustment-registration-lifecycle-status-v4",
      "adjustment-shadow-metadata-custody-status-v1",
      `adjustment-shadow-metadata-custody-finalize-v1 ${sha}`,
      `adjustment-confirmation-access-burn-v3 ${sha}`,
      `adjustment-shadow-terminal-record-v3 ${sha}`,
      `adjustment-shadow-terminal-retire-v3 ${sha}`,
      `adjustment-shadow-unsupported-terminal-record-v1 ${sha}`,
      `adjustment-shadow-unsupported-terminal-retire-v1 ${sha}`,
      `adjustment-development-custody-anchor-install-v1 ${sha}`,
      `adjustment-rain-control-custody-anchor-install-v1 ${sha}`,
      `adjustment-unsupported-terminal-proof-install-v1 ${sha}`,
      `adjustment-future-input-seal-install-v2 ${sha}`,
      `adjustment-maintenance-anchor-install-v2 ${sha}`,
      `adjustment-maintenance-anchor-install-v3 ${sha}`,
      `adjustment-maintenance-anchor-finalize-v2 ${sha}`,
      `adjustment-maintenance-anchor-finalize-v3 ${sha}`,
      `install-adjustment-scorecard ${sha}`,
      `install-adjustment-scorecard-v2 ${sha}`,
      "adjustment-evaluation-export-v2 2026-10-01 2026-10-08",
      `adjustment-confirmation-availability-v2 ${sha}`,
      `adjustment-confirmation-export-v2 ${sha} ${access} 0`,
      `adjustment-confirmation-export-v2 ${sha} ${access} 26`,
    ]) {
      const result = dispatch(command);
      assert.equal(result.status, 0, result.stderr);
      assert.deepEqual((await readFile(log, "utf8")).split("\0").slice(0, -1), [
        "-n", "/usr/local/sbin/weather-remote-ops", ...command.split(" "),
      ]);
    }

    // reject alternate framing, hashes, indices and shell payloads
    for (const command of [
      "adjustment-maintenance-anchor-status-v2 /tmp/input",
      "adjustment-development-custody-anchor-current-v1 extra",
      "adjustment-rain-control-custody-anchor-current-v1 extra",
      "adjustment-unsupported-terminal-proof-current-v1 extra",
      "adjustment-future-input-seal-current-v2 extra",
      "adjustment-maintenance-anchor-current-v3 extra",
      "adjustment-registration-lifecycle-status-v4 extra",
      "adjustment-shadow-metadata-custody-status-v1 extra",
      `adjustment-shadow-metadata-custody-finalize-v1 ${sha.toUpperCase()}`,
      `adjustment-shadow-metadata-custody-finalize-v1 ${sha} extra`,
      `adjustment-confirmation-access-burn-v3 ${sha.toUpperCase()}`,
      `adjustment-confirmation-access-burn-v3 ${sha} extra`,
      `adjustment-shadow-terminal-record-v3 ${sha.toUpperCase()}`,
      `adjustment-shadow-terminal-record-v3 ${sha} extra`,
      `adjustment-shadow-terminal-retire-v3 ${sha.toUpperCase()}`,
      `adjustment-shadow-terminal-retire-v3 ${sha} extra`,
      `adjustment-shadow-unsupported-terminal-record-v1 ${sha.toUpperCase()}`,
      `adjustment-shadow-unsupported-terminal-record-v1 ${sha} extra`,
      `adjustment-shadow-unsupported-terminal-retire-v1 ${sha.toUpperCase()}`,
      `adjustment-shadow-unsupported-terminal-retire-v1 ${sha} extra`,
      `adjustment-development-custody-anchor-install-v1 ${sha.toUpperCase()}`,
      `adjustment-development-custody-anchor-install-v1 ${sha} extra`,
      `adjustment-rain-control-custody-anchor-install-v1 ${sha.toUpperCase()}`,
      `adjustment-rain-control-custody-anchor-install-v1 ${sha} extra`,
      `adjustment-unsupported-terminal-proof-install-v1 ${sha.toUpperCase()}`,
      `adjustment-unsupported-terminal-proof-install-v1 ${sha} extra`,
      `adjustment-future-input-seal-install-v2 ${sha.toUpperCase()}`,
      `adjustment-future-input-seal-install-v2 ${sha} extra`,
      `adjustment-maintenance-anchor-install-v2 ${sha.toUpperCase()}`,
      `adjustment-maintenance-anchor-install-v2 ${sha} extra`,
      `adjustment-maintenance-anchor-install-v3 ${sha.toUpperCase()}`,
      `adjustment-maintenance-anchor-install-v3 ${sha} extra`,
      `adjustment-maintenance-anchor-finalize-v2 ${sha.toUpperCase()}`,
      `adjustment-maintenance-anchor-finalize-v3 ${sha.toUpperCase()}`,
      `adjustment-maintenance-anchor-finalize-v3 ${sha} extra`,
      `install-adjustment-scorecard-v2 ${sha} extra`,
      `install-adjustment-scorecard-v2 ${sha.toUpperCase()}`,
      `adjustment-confirmation-export-v2 ${sha} ${access} 27`,
      `adjustment-confirmation-export-v2 ${sha} ${access} 00`,
      `adjustment-confirmation-export-v2 ${sha} ${access} -1`,
      `adjustment-confirmation-availability-v2 ${sha}; id`,
      `adjustment-confirmation-availability-v2 ${sha}\nstatus`,
      "adjustment-evaluation-export-v2 2026-10-01 2026-10-08 /tmp/output",
      "install-adjustment-scorecard-v3 " + sha,
    ]) {
      await rm(log, { force: true });
      const result = dispatch(command);
      assert.equal(result.status, 126);
      await assert.rejects(readFile(log), { code: "ENOENT" });
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// keep forced deployment settings on the immutable active source environment
test("v13 derives source settings from retained state and rejects bootstrap substitutes", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-v13-source-"));
  await mkdir(join(directory, "state"));
  await mkdir(join(directory, "releases"));
  const source = join(directory, "releases/2026.10.07-3.env");
  await writeFile(source, "WEATHER_RELEASE=2026.10.07-3\n", { mode: 0o600 });
  await writeFile(join(directory, "state/current-release"), "2026.10.07-3\n", { mode: 0o600 });
  const harness = `source "$1"
state_dir="$2/state"
releases_dir="$2/releases"
require_retained_maintenance_source "$3"`;
  // run the real source gate without Docker or service mutation
  const check = (path) => spawnSync("bash", [
    "-c", harness, "v13-source", join(root, "deploy/scripts/update.sh"), directory, path,
  ], { encoding: "utf8" });
  try {
    const accepted = check(source);
    assert.equal(accepted.status, 0, accepted.stderr);
    assert.notEqual(check(join(directory, ".env")).status, 0);
    await writeFile(join(directory, "state/current-release"), "2026.10.07-2\n");
    assert.notEqual(check(source).status, 0);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

// reject another release while one actor holds the shared state inode
test("v13 lifecycle lock serializes release actors without creating a lock file", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-v13-lock-"));
  const holder = spawn("bash", [
    "-c", 'exec 9<"$1"; flock --exclusive 9; printf ready; read -r ignored || :',
    "release-lock-holder", directory,
  ], { stdio: ["pipe", "pipe", "pipe"] });
  const harness = 'source "$1"; state_dir="$2"; acquire_release_transaction_lock';
  // execute only the production locking helper
  const check = () => spawnSync("bash", [
    "-c", harness, "v13-lock", join(root, "deploy/scripts/update.sh"), directory,
  ], { encoding: "utf8" });
  try {
    await once(holder.stdout, "data");
    const refused = check();
    assert.notEqual(refused.status, 0);
    assert.match(refused.stderr, /another Weather release transaction/u);
    const exited = once(holder, "exit");
    holder.stdin.end();
    await exited;
    const admitted = check();
    assert.equal(admitted.status, 0, admitted.stderr);
  } finally {
    holder.stdin.end();
    await rm(directory, { recursive: true, force: true });
  }
});
