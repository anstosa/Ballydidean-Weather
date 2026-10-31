import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const backup = resolve(import.meta.dirname, "../scripts/backup.sh");

// run one backup with isolated producer and encryptor ports
async function runBackup({ bytes, cap }) {
  const root = await mkdtemp(join(tmpdir(), "weather-backup-cap-"));
  const bin = join(root, "bin");
  const output = join(root, "output");
  const envFile = join(root, "weather.env");
  await mkdir(bin);
  await mkdir(output);
  await writeFile(envFile, "WEATHER_DATABASE_NAME=weather\n", { mode: 0o600 });
  await writeFile(join(bin, "docker"), `#!/usr/bin/env bash
set -euo pipefail
head --bytes="${bytes}" /dev/zero
exit "\${PRODUCER_STATUS:-0}"
`);
  await writeFile(join(bin, "age"), `#!/usr/bin/env bash
set -euo pipefail
output=
while (($# > 0)); do
  case "$1" in
    --recipient) shift 2 ;;
    --output) output=$2; shift 2 ;;
    *) exit 2 ;;
  esac
done
if [[ -n "$output" ]]; then cat >"$output"; else cat; fi
exit "\${ENCRYPTOR_STATUS:-0}"
`);
  await writeFile(join(bin, "dd"), `#!/usr/bin/env bash
/usr/bin/dd "$@"
exit "\${DD_STATUS:-0}"
`);
  await writeFile(join(bin, "od"), `#!/usr/bin/env bash
/usr/bin/od "$@"
exit "\${OD_STATUS:-0}"
`);
  await writeFile(join(bin, "sync"), "#!/usr/bin/env bash\nexit 0\n");
  // expose every controlled pipeline boundary as one executable fixture
  for (const name of ["age", "dd", "docker", "od", "sync"]) {
    await chmod(join(bin, name), 0o755);
  }
  const argumentsList = ["--recipient", `age1${"q".repeat(58)}`, "--output-dir", output,
    "--env-file", envFile];
  // add only the explicitly requested bounded mode
  if (cap !== undefined) {
    argumentsList.push("--max-output-bytes", String(cap));
  }
  const result = spawnSync("bash", [backup, ...argumentsList], {
    encoding: "utf8",
    env: { ...process.env, BASH_ENV: "/dev/null", PATH: `${bin}:${process.env.PATH}` },
  });
  return { output, result, root };
}

test("bounded backup publishes output at the exact ceiling", async () => {
  const fixture = await runBackup({ bytes: 4096, cap: 4096 });
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    const names = await readdir(fixture.output);
    assert.equal(names.filter((name) => name.endsWith(".dump.age")).length, 1);
    assert.equal(names.filter((name) => name.endsWith(".sha256")).length, 1);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("bounded backup rejects one byte over without disturbing prior backups", async () => {
  const fixture = await runBackup({ bytes: 4097, cap: 4096 });
  try {
    await writeFile(join(fixture.output, "weather-retained.dump.age"), "retained\n");
    const retry = spawnSync("bash", [backup, "--recipient", `age1${"q".repeat(58)}`,
      "--output-dir", fixture.output, "--env-file", join(fixture.root, "weather.env"),
      "--max-output-bytes", "4096"], {
      encoding: "utf8",
      env: { ...process.env, BASH_ENV: "/dev/null",
        PATH: `${join(fixture.root, "bin")}:${process.env.PATH}` },
    });
    assert.notEqual(retry.status, 0);
    assert.match(retry.stderr, /bounded encrypted backup failed/u);
    assert.equal(await readFile(join(fixture.output, "weather-retained.dump.age"), "utf8"),
      "retained\n");
    assert.deepEqual((await readdir(fixture.output)).sort(), ["weather-retained.dump.age"]);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("default backup mode remains uncapped", async () => {
  const fixture = await runBackup({ bytes: 8192 });
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.equal((await readdir(fixture.output)).filter((name) => name.endsWith(".dump.age")).length, 1);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});

test("bounded backup propagates producer and encryptor failures", async () => {
  // exercise both upstream pipeline failures without publishing partial output
  for (const variable of ["PRODUCER_STATUS", "ENCRYPTOR_STATUS"]) {
    const fixture = await runBackup({ bytes: 2048, cap: 4096 });
    try {
      await rm(fixture.output, { force: true, recursive: true });
      await mkdir(fixture.output);
      const failed = spawnSync("bash", [backup, "--recipient", `age1${"q".repeat(58)}`,
        "--output-dir", fixture.output, "--env-file", join(fixture.root, "weather.env"),
        "--max-output-bytes", "4096"], {
        encoding: "utf8",
        env: { ...process.env, BASH_ENV: "/dev/null", [variable]: "17",
          PATH: `${join(fixture.root, "bin")}:${process.env.PATH}` },
      });
      assert.notEqual(failed.status, 0);
      assert.deepEqual(await readdir(fixture.output), []);
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  }
});

test("bounded backup rejects writer and excess-probe failures at producer EOF", async () => {
  // keep exact-ceiling output unpublished when either final writer check fails
  for (const variable of ["DD_STATUS", "OD_STATUS"]) {
    const fixture = await runBackup({ bytes: 4096, cap: 4096 });
    try {
      await rm(fixture.output, { force: true, recursive: true });
      await mkdir(fixture.output);
      const failed = spawnSync("bash", [backup, "--recipient", `age1${"q".repeat(58)}`,
        "--output-dir", fixture.output, "--env-file", join(fixture.root, "weather.env"),
        "--max-output-bytes", "4096"], {
        encoding: "utf8",
        env: { ...process.env, BASH_ENV: "/dev/null", [variable]: "19",
          PATH: `${join(fixture.root, "bin")}:${process.env.PATH}` },
      });
      assert.notEqual(failed.status, 0);
      assert.match(failed.stderr, /bounded encrypted backup failed/u);
      assert.deepEqual(await readdir(fixture.output), []);
    } finally {
      await rm(fixture.root, { force: true, recursive: true });
    }
  }
});

test("bounded backup signal cleanup removes only invocation-owned partials", async () => {
  const fixture = await runBackup({ bytes: 1024, cap: 4096 });
  try {
    await rm(fixture.output, { force: true, recursive: true });
    await mkdir(fixture.output);
    await writeFile(join(fixture.output, "weather-retained.dump.age"), "retained\n");
    await writeFile(join(fixture.root, "bin", "docker"), `#!/usr/bin/env bash
set -euo pipefail
head --bytes=1024 /dev/zero
sleep 30
`);
    const child = spawn("bash", [backup, "--recipient", `age1${"q".repeat(58)}`,
      "--output-dir", fixture.output, "--env-file", join(fixture.root, "weather.env"),
      "--max-output-bytes", "4096"], {
      env: { ...process.env, BASH_ENV: "/dev/null",
        PATH: `${join(fixture.root, "bin")}:${process.env.PATH}` },
      stdio: "ignore",
    });
    await new Promise((resolveWait) => setTimeout(resolveWait, 250));
    child.kill("SIGTERM");
    await once(child, "exit");
    assert.deepEqual((await readdir(fixture.output)).sort(), ["weather-retained.dump.age"]);
  } finally {
    await rm(fixture.root, { force: true, recursive: true });
  }
});
