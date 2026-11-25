import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  collectTestPlan,
  fastExcludedFiles,
  filterFastTests,
  partitionRetiredR2Tests,
  retiredR2TestFiles,
  assertSmokePassCount,
  smokeNames,
  validateSmokeNames,
} from "./test-workspaces.mjs";

const root = new URL("..", import.meta.url);

// flatten the selected paths for partition checks
function paths(plan) {
  return plan.flatMap((group) => group.files);
}

// keep every existing workspace and script unit suite in the full pass
test("compiled full tests include every unit suite and research script", async () => {
  const plan = await collectTestPlan(root);
  const selected = paths(plan);

  assert.equal(new Set(selected).size, selected.length);
  assert.ok(selected.includes("scripts/build-workspaces.test.mjs"));
  assert.ok(selected.includes("apps/worker/test/worker.integration.test.mjs"));
  assert.ok(selected.includes("packages/database/test/database.unit.test.mjs"));
  assert.ok(selected.includes("scripts/research/build_humidity_dataset.test.mjs"));
  assert.deepEqual(plan.map((group) => group.name), [
    "scripts",
    "api",
    "web",
    "worker",
    "database",
    "domain",
    "forecast-adjustment",
    "providers",
    "research",
  ]);
});

// keep the approved retired source-proof boundary exact and reviewable
test("compiled execution separates only the exact retired R2 source-proof family", async () => {
  const discovered = await collectTestPlan(root);
  const { active, retired } = partitionRetiredR2Tests(discovered);
  const discoveredPaths = paths(discovered);
  const activePaths = paths(active);
  const retiredPaths = paths(retired);

  assert.deepEqual(retiredPaths, retiredR2TestFiles);
  assert.deepEqual(
    discoveredPaths.filter((file) => !activePaths.includes(file)),
    retiredR2TestFiles,
  );
  assert.equal(activePaths.length + retiredPaths.length, discoveredPaths.length);
  assert.equal(new Set([...activePaths, ...retiredPaths]).size, discoveredPaths.length);
  assert.ok(activePaths.includes("scripts/backend-performance-direct-evaluator.test.mjs"));
  assert.ok(activePaths.includes("scripts/backend-host-observation.test.mjs"));
  assert.ok(activePaths.includes("scripts/check-backend-runtime-inventory.test.mjs"));
  assert.ok(activePaths.includes("scripts/check-rust-source-quality.test.mjs"));
  assert.ok(activePaths.includes("scripts/generate-rust-contracts.test.mjs"));
});

// make the fast exclusions explicit and prove no other suite disappears
test("fast tests omit only named offline research and retained replay suites", async () => {
  const { active } = partitionRetiredR2Tests(await collectTestPlan(root));
  const full = paths(active);
  const fast = paths(filterFastTests(active));
  const removed = full.filter((file) => !fast.includes(file));

  assert.deepEqual(new Set(removed), new Set(fastExcludedFiles));
  assert.ok(fast.includes("packages/forecast-adjustment/test/runtime-loader.test.mjs"));
  assert.ok(fast.includes("packages/forecast-adjustment/test/temperature-canary.test.mjs"));
  assert.ok(fast.includes("apps/api/test/rain-adjustment.test.mjs"));
  assert.ok(fast.includes("apps/worker/test/rain-adjustment.test.mjs"));
});

// fail smoke selection before a zero-test pass can occur
test("browser smoke names all exist and reject missing tests", async () => {
  const source = await readFile(new URL("../apps/web/test/web.e2e.mjs", import.meta.url), "utf8");

  assert.equal(smokeNames.length, 4);
  assert.doesNotThrow(() => validateSmokeNames(source, smokeNames));
  assert.throws(() => validateSmokeNames(source, ["missing smoke test"]), /missing smoke test/u);
});

// reject a zero-test or skipped-only browser gate
test("browser smoke requires four actual passing TAP tests", () => {
  assert.doesNotThrow(() => assertSmokePassCount("1..22\n# pass 4\n# fail 0\n# skipped 18\n"));
  assert.throws(() => assertSmokePassCount("1..22\n# pass 0\n# fail 0\n# skipped 22\n"), /expected 4 passing/u);
  assert.throws(() => assertSmokePassCount("1..22\n# pass 3\n# fail 0\n# skipped 19\n"), /expected 4 passing/u);
  assert.throws(() => assertSmokePassCount("1..22\n# pass 4\n# fail 1\n"), /expected 4 passing/u);
  assert.throws(() => assertSmokePassCount("# pass 4\n# fail 0\n# pass 0\n# fail 0\n"), /expected 4 passing/u);
});
