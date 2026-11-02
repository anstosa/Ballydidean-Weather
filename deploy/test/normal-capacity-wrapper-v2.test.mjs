import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmod, link, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const common = resolve(import.meta.dirname, "../scripts/common.sh");

// capture only the file-based package invocation without executing a collector
function invoke(root, command, extra = [], input = join(root, "input.json")) {
  return spawnSync("bash", ["-c", `source "$1"
transcript=$2
# expose the exact package argument vector
node() { printf '%s\\0' "$@" >"$transcript"; }
shift 2
normal_release_capacity_receipt "$@"`, "capacity-wrapper", common,
  join(root, "argv"), command, input, ...extra], { encoding: "utf8" });
}

// create one bounded private input and proof pair
async function fixture() {
  const root = await mkdtemp(join(tmpdir(), "weather-capacity-wrapper-v2-"));
  await writeFile(join(root, "input.json"), "{}\n", { mode: 0o600 });
  await writeFile(join(root, "proof.json"), "{}\n", { mode: 0o600 });
  return root;
}

// require every reviewed domain to forward explicit private file paths
test("normal capacity wrapper forwards v2 request and proof paths", async () => {
  const root = await fixture();
  try {
    // exercise immutable proof and both live formula command shapes
    for (const [command, extra] of [
      ["normal-stage-image-proof", []],
      ["normal-stage-capacity", [join(root, "proof.json")]],
      ["normal-activation-capacity", []],
      ["normal-activation-capacity", [join(root, "proof.json")]],
    ]) {
      const result = invoke(root, command, extra);
      assert.equal(result.status, 0, result.stderr);
      assert.deepEqual((await readFile(join(root, "argv"))).toString().split("\0").filter(Boolean),
        [resolve(import.meta.dirname, "../scripts/adjustment-evaluation-package.mjs"),
          command, join(root, "input.json"), ...extra]);
    }
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// reject missing or extra authority arguments before launching a package command
test("normal capacity wrapper closes command and argument arity", async () => {
  const root = await fixture();
  try {
    // cover unsupported domains and every invalid authority shape
    for (const [command, extra] of [
      ["arbitrary-operation", []],
      ["normal-stage-image-proof", [join(root, "proof.json")]],
      ["normal-stage-capacity", []],
      ["normal-stage-capacity", [join(root, "proof.json"), "unexpected"]],
      ["normal-activation-capacity", [join(root, "proof.json"), "unexpected"]],
    ]) {
      const result = invoke(root, command, extra);
      assert.notEqual(result.status, 0, command);
    }
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// refuse aliases and exposed or oversized request files before collector execution
test("normal capacity wrapper rejects unsafe input metadata", async () => {
  const root = await fixture();
  try {
    const input = join(root, "input.json");
    const alias = join(root, "alias.json");
    await symlink(input, alias);
    assert.notEqual(invoke(root, "normal-stage-image-proof", [], alias).status, 0);
    await chmod(input, 0o640);
    assert.notEqual(invoke(root, "normal-stage-image-proof").status, 0);
    await chmod(input, 0o600);
    await link(input, join(root, "hardlink.json"));
    assert.notEqual(invoke(root, "normal-stage-image-proof").status, 0);
    await rm(join(root, "hardlink.json"));
    await writeFile(input, Buffer.alloc(16_385, 0x20));
    assert.notEqual(invoke(root, "normal-stage-image-proof").status, 0);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});
