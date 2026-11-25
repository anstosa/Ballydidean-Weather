import { spawnSync } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = resolve(import.meta.dirname, "..");
const unitGroups = [
  ["scripts", "scripts", ".test.mjs"],
  ["api", "apps/api/test", ".test.mjs"],
  ["web", "apps/web/test", ".test.mjs"],
  ["worker", "apps/worker/test", ".test.mjs"],
  ["database", "packages/database/test", ".unit.test.mjs"],
  ["domain", "packages/domain/test", ".test.mjs"],
  ["forecast-adjustment", "packages/forecast-adjustment/test", ".test.mjs"],
  ["providers", "packages/providers/test", ".test.mjs"],
  ["research", "scripts/research", ".test.mjs"],
];
const integrationGroups = [
  ["database", "packages/database/test", ".integration.test.mjs"],
  ["worker", "apps/worker/test", ".integration.test.mjs"],
  ["api", "apps/api/test", ".integration.mjs"],
];

export const fastExcludedFiles = Object.freeze([
  "packages/forecast-adjustment/test/full-history-research.test.mjs",
  "packages/forecast-adjustment/test/retained-pipeline.test.mjs",
  "packages/forecast-adjustment/test/temperature-analog-research.test.mjs",
  "packages/forecast-adjustment/test/temperature-analog-stress-validation.test.mjs",
  "packages/forecast-adjustment/test/temperature-analog-validation.test.mjs",
  "packages/forecast-adjustment/test/temperature-lead-research.test.mjs",
  "packages/forecast-adjustment/test/temperature-nowcast-research.test.mjs",
  "packages/forecast-adjustment/test/temperature-weather-research.test.mjs",
  "packages/forecast-adjustment/test/wind-canary-retained.test.mjs",
  "scripts/research/acquire_rain_stations.test.mjs",
  "scripts/research/build_humidity_dataset.test.mjs",
]);

// preserve the approved retired r2 source-proof suites for standalone historical use
export const retiredR2TestFiles = Object.freeze([
  "scripts/backend-clock-catalog-report.test.mjs",
  "scripts/backend-clock-occurrences.test.mjs",
  "scripts/backend-nonmodule-source-analysis.test.mjs",
  "scripts/backend-operation-source-graph.test.mjs",
  "scripts/backend-reference-time-source-proof.test.mjs",
  "scripts/check-backend-reference-time-source.test.mjs",
]);

export const smokeNames = Object.freeze([
  "public navigation updates the URL and restores routed content from history",
  "anonymous home-network viewers see indoor and soil panels only while allowed",
  "admin login and logout work inside an iframe",
  "admin forecast switches persist independently while the public cloud toggle remains",
]);

// discover the same unit files selected by standalone workspace scripts
export async function collectTestPlan(root, groups = unitGroups) {
  const directory = root instanceof URL ? fileURLToPath(root) : root;
  const plan = [];

  // preserve the existing sequential workspace boundaries
  for (const [name, path, suffix] of groups) {
    const entries = await readdir(join(directory, path));
    const files = entries
      .filter((entry) => entry.endsWith(suffix))
      .sort()
      .map((entry) => join(path, entry));

    // fail when a renamed suite would silently disappear
    if (files.length === 0) {
      throw new Error(`no tests found for ${name}`);
    }

    plan.push({ files, name });
  }

  return plan;
}

// apply the approved retirement without hiding any unnamed suite
export function partitionRetiredR2Tests(plan) {
  const retiredNames = new Set(retiredR2TestFiles);
  const found = new Set();
  const active = [];
  const retired = [];

  // partition every discovered path without wildcard exclusions
  for (const group of plan) {
    const activeFiles = [];
    const retiredFiles = [];

    // retain each retired suite in an explicit historical plan
    for (const file of group.files) {
      if (retiredNames.has(file)) {
        found.add(file);
        retiredFiles.push(file);
      } else {
        activeFiles.push(file);
      }
    }

    // preserve nonempty active workspace groups
    if (activeFiles.length > 0) {
      active.push({ ...group, files: activeFiles });
    }
    // preserve nonempty retired groups for standalone selection
    if (retiredFiles.length > 0) {
      retired.push({ ...group, files: retiredFiles });
    }
  }

  // reject stale names instead of silently changing the retired boundary
  for (const file of retiredR2TestFiles) {
    if (!found.has(file)) {
      throw new Error(`retired R2 test missing from discovered plan: ${file}`);
    }
  }

  return { active, retired };
}

// omit only offline research and retained replay work from routine commits
export function filterFastTests(plan) {
  const excluded = new Set(fastExcludedFiles);
  const found = new Set();
  const filtered = plan.map((group) => ({
    ...group,
    files: group.files.filter((file) => {
      // record each explicit exclusion
      if (excluded.has(file)) {
        found.add(file);
        return false;
      }

      return true;
    }),
  }));

  // reject stale exclusions rather than silently drifting coverage
  for (const file of excluded) {
    // require every named offline suite in the full plan
    if (!found.has(file)) {
      throw new Error(`fast test exclusion missing from full plan: ${file}`);
    }
  }

  return filtered.filter((group) => group.files.length > 0);
}

// check smoke names against the browser suite before running Docker
export function validateSmokeNames(source, names) {
  // prevent an empty or mistyped selection from passing
  for (const name of names) {
    // require an exact top-level test declaration
    if (!source.includes(`test("${name}"`)) {
      throw new Error(`missing smoke test: ${name}`);
    }
  }
}

// require the four selected browser journeys to actually pass
export function assertSmokePassCount(output) {
  const passes = [...output.matchAll(/^# pass (\d+)$/gmu)];
  const failures = [...output.matchAll(/^# fail (\d+)$/gmu)];

  // reject skipped-only or incomplete TAP output
  if (
    passes.length !== 1 ||
    failures.length !== 1 ||
    passes[0]?.[1] !== String(smokeNames.length) ||
    failures[0]?.[1] !== "0"
  ) {
    throw new Error(`expected ${smokeNames.length} passing browser smoke tests`);
  }
}

// escape names before passing the anchored browser test filter
function escapeRegularExpression(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

// run one already-compiled test group without rebuilding dependencies
function runNodeTests(files, serial = false) {
  const argumentsList = ["--test"];

  // serialize real database integration files
  if (serial) {
    argumentsList.push("--test-concurrency=1");
  }

  argumentsList.push(...files);
  const result = spawnSync(process.execPath, argumentsList, {
    cwd: repositoryRoot,
    stdio: "inherit",
  });

  // report one failing group without executing downstream groups
  if (result.error !== undefined) {
    throw result.error;
  }

  // preserve nonzero exits and termination signals
  if (result.status !== 0) {
    throw new Error(`test group failed: ${result.signal ?? result.status}`);
  }
}

// run the fixture browser suite with existing pinned Playwright tooling
async function runBrowserTests(smoke) {
  const files = (await readdir(join(repositoryRoot, "apps/web/test")))
    .filter((entry) => entry.endsWith(".e2e.mjs"))
    .sort()
    .map((entry) => join("apps/web/test", entry));

  // reject a missing browser suite
  if (files.length === 0) {
    throw new Error("no browser tests found");
  }

  const argumentsList = ["--test"];

  // select only four existing journeys for routine commits
  if (smoke) {
    const sources = await Promise.all(files.map((file) => readFile(join(repositoryRoot, file), "utf8")));
    validateSmokeNames(sources.join("\n"), smokeNames);
    const expression = `^(?:${smokeNames.map(escapeRegularExpression).join("|")})$`;
    argumentsList.push("--test-reporter=tap", `--test-name-pattern=${expression}`);
  }

  argumentsList.push(...files);
  const result = spawnSync("docker", [
    "run", "--rm", "--init", "--ipc=host", "--network", "host",
    "--volume", `${repositoryRoot}:/work`, "--workdir", "/work",
    "mcr.microsoft.com/playwright:v1.62.1-noble", "node", ...argumentsList,
  ], {
    cwd: repositoryRoot,
    encoding: smoke ? "utf8" : undefined,
    maxBuffer: 10 * 1024 * 1024,
    stdio: smoke ? ["inherit", "pipe", "pipe"] : "inherit",
  });

  // relay captured TAP before evaluating the smoke count
  if (smoke) {
    process.stdout.write(result.stdout ?? "");
    process.stderr.write(result.stderr ?? "");
  }

  // report an unavailable Docker engine or browser failure
  if (result.error !== undefined) {
    throw result.error;
  }

  // fail rather than accepting an incomplete smoke pass
  if (result.status !== 0) {
    throw new Error(`browser tests failed: ${result.signal ?? result.status}`);
  }

  // Docker can exit successfully even when every selected test skips
  if (smoke) {
    assertSmokePassCount(result.stdout ?? "");
  }
}

// dispatch an explicit compiled-test profile
async function main(profile) {
  // keep browser runs out of the Node unit dispatcher
  if (profile === "e2e" || profile === "e2e-smoke") {
    await runBrowserTests(profile === "e2e-smoke");
    return;
  }

  // reject a misspelled profile before selecting any tests
  if (!["full", "fast", "integration"].includes(profile)) {
    throw new Error(`unknown compiled test profile: ${profile}`);
  }

  const groups = profile === "integration" ? integrationGroups : unitGroups;
  const fullPlan = await collectTestPlan(repositoryRoot, groups);
  const { active } = profile === "integration"
    ? { active: fullPlan }
    : partitionRetiredR2Tests(fullPlan);
  const plan = profile === "fast" ? filterFastTests(active) : active;

  // retain the previous sequential group execution order
  for (const group of plan) {
    runNodeTests(group.files, profile === "integration");
  }
}

// run only when invoked through a root package command
if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main(process.argv[2]);
}
