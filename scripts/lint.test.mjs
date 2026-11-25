import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { promisify } from "node:util";

const executeFile = promisify(execFile);
const repoRoot = resolve(import.meta.dirname, "..");
const lintScript = join(repoRoot, "scripts/lint.mjs");

// include native source while excluding generated native output trees
test("lint scans authored native text without walking build artifacts", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-native-lint-"));

  try {
    await writeFile(join(root, "Weather.kt"), "val weather = true\n");
    await writeFile(join(root, "GenerateBrandAssets.java"), "final class GenerateBrandAssets {}\n");

    // populate every ignored native output boundary with invalid text
    for (const directory of [".gradle", "build", "host-evidence"]) {
      const generated = join(root, directory);
      await mkdir(generated);
      await writeFile(join(generated, "Generated.kt"), "val generated = true   \n");
    }

    const result = await executeFile(process.execPath, [lintScript, root], { cwd: repoRoot });
    assert.match(result.stdout, /Linted 2 files\./u);

    await writeFile(join(root, "Widget.kt"), "val widget = true   \n");
    await assert.rejects(
      executeFile(process.execPath, [lintScript, root], { cwd: repoRoot }),
      // require the authored native file to fail text lint
      (error) => `${error.stdout}${error.stderr}`.includes("Widget.kt:1: trailing whitespace"),
    );
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});

// keep the canonical contract package out of runtime module graphs
test("lint requires type-only syntax at canonical contract package boundaries", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-contract-lint-"));

  try {
    // exercise both the API consumer and the canonical forecast type source
    for (const [directory, dependency, type] of [
      ["apps/api/src", "@weather/contracts", "ApiSite"],
      ["packages/contracts/src", "@weather/forecast-adjustment", "ForecastTemperatureCanaryDecisionV1"],
    ]) {
      const sourceDirectory = join(root, directory);
      const source = join(sourceDirectory, "index.ts");
      await mkdir(sourceDirectory, { recursive: true });
      await writeFile(
        source,
        `import type { ${type} } from "${dependency}";\nexport type { ${type} } from "${dependency}";\n`,
      );
      const accepted = await executeFile(process.execPath, [lintScript, directory], {
        cwd: root,
      });
      assert.match(accepted.stdout, /Linted 1 files\./u);

      const rejectedSources = [
        {
          diagnostic: `project import ${dependency} must use import type`,
          source: `import { ${type} } from "${dependency}";\n`,
        },
        {
          diagnostic: `project export ${dependency} must use export type`,
          source: `export { ${type} } from "${dependency}";\n`,
        },
        {
          diagnostic: `project import ${dependency} must be static and type-only`,
          source: `const contracts = import("${dependency}");\n`,
        },
      ];

      // keep every value-bearing form out of the emitted module graph
      for (const rejected of rejectedSources) {
        await writeFile(source, rejected.source);
        await assert.rejects(
          executeFile(process.execPath, [lintScript, directory], { cwd: root }),
          // require the boundary-specific diagnostic
          (error) => `${error.stdout}${error.stderr}`.includes(rejected.diagnostic),
        );
      }
    }
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});
