import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  classifyCommitRange,
  classifyPaths,
  parseNameStatus,
  verifyRequiredJobs,
} from "./ci-changes.mjs";

const allGates = {
  fullTests: true,
  integration: true,
  deployIntegration: true,
  fullBrowser: true,
  nativeAndroid: true,
};
const baselineOnly = {
  fullTests: false,
  integration: false,
  deployIntegration: false,
  fullBrowser: false,
  nativeAndroid: false,
};

// isolate one top-level workflow job
function workflowJob(workflow, name) {
  const marker = `  ${name}:\n`;
  const start = workflow.indexOf(marker);
  assert.notEqual(start, -1, `missing workflow job ${name}`);
  const remainder = workflow.slice(start + marker.length);
  const nextJob = remainder.search(/^  [a-z0-9-]+:\n/mu);

  // keep the final job through end of file
  if (nextJob === -1) {
    return remainder;
  }
  return remainder.slice(0, nextJob);
}

// retain both rename paths and complete status records
test("rename classification includes source and destination", () => {
  assert.deepEqual(
    parseNameStatus(Buffer.from("R100\0docs/old.md\0deploy/new.sh\0")),
    ["docs/old.md", "deploy/new.sh"],
  );
  assert.deepEqual(
    parseNameStatus(Buffer.from("D\0apps/web/src/old.ts\0")),
    ["apps/web/src/old.ts"],
  );
  assert.equal(parseNameStatus(Buffer.from("M\0docs/truncated.md")), null);
});

// unknown inputs cannot narrow coverage
test("unknown statuses and paths fail to full coverage", () => {
  assert.equal(parseNameStatus(Buffer.from("Z\0README.md\0")), null);
  assert.equal(parseNameStatus(Buffer.from([77, 0, 255, 0])), null);
  assert.deepEqual(classifyPaths(["new/unknown.txt"]), allGates);
});

// documentation still receives mandatory gates
test("documentation retains core smoke without expensive suites", () => {
  assert.deepEqual(classifyPaths(["docs/operations/example.md"]), baselineOnly);
});

// UI changes keep browser coverage without unrelated rehearsals
test("UI changes keep fast units and integration defaults but require full browser", () => {
  assert.deepEqual(classifyPaths(["apps/web/src/index.ts", "apps/web/src/styles.css"]), {
    fullTests: false,
    integration: false,
    deployIntegration: false,
    fullBrowser: true,
    nativeAndroid: true,
  });
});

// platform-only changes select only their matching native lane
test("native platform paths preserve the mandatory linux baseline", () => {
  assert.deepEqual(classifyPaths(["mobile/android/app/src/main/Main.kt"]), {
    ...baselineOnly,
    nativeAndroid: true,
  });
  assert.deepEqual(classifyPaths(["mobile/ios/WeatherApp/App.swift"]), allGates);
});

// shared contracts and renamed platform paths retain every affected native lane
test("shared mobile paths and renames select conservatively", () => {
  assert.deepEqual(classifyPaths(["mobile/shared/widget-forecast-v1.schema.json"]), {
    ...baselineOnly,
    nativeAndroid: true,
  });
  assert.deepEqual(classifyPaths(["scripts/widget-fixtures.mjs"]), {
    ...baselineOnly,
    nativeAndroid: true,
  });
  assert.deepEqual(classifyPaths(["mobile/scripts/native_https_fixture.py"]), {
    ...baselineOnly,
    nativeAndroid: true,
  });
  const renamed = parseNameStatus(Buffer.from(
    "R100\0mobile/android/Old.kt\0mobile/ios/New.swift\0",
  ));
  assert.deepEqual(classifyPaths(renamed), allGates);
});

// transitive boundaries select their dependent suites
test("UI, model, backend, and deployment changes select dependent suites", () => {
  assert.equal(classifyPaths(["apps/web/src/styles.css"]).fullBrowser, true);
  assert.equal(classifyPaths(["scripts/research/rain_fit.py"]).fullTests, true);
  assert.equal(classifyPaths(["packages/forecast-adjustment/src/main.ts"]).integration, true);
  assert.equal(classifyPaths(["apps/api/src/main.ts"]).integration, true);
  assert.equal(classifyPaths(["packages/domain/src/main.ts"]).deployIntegration, true);
  assert.equal(classifyPaths(["packages/database/migrations/0017.sql"]).deployIntegration, true);
  assert.equal(classifyPaths(["packages/providers/src/index.ts"]).deployIntegration, true);
  assert.equal(classifyPaths(["apps/worker/src/index.ts"]).fullTests, true);
  assert.equal(classifyPaths(["deploy/compose.yaml"]).deployIntegration, true);
  assert.equal(classifyPaths([".github/workflows/check.yml"]).deployIntegration, true);
  assert.equal(classifyPaths(["package-lock.json"]).deployIntegration, true);
  assert.equal(classifyPaths(["deploy/compose.yaml"]).nativeAndroid, true);
  assert.equal(classifyPaths(["config/sites/ballydidean.json"]).nativeAndroid, true);
});

// preserve expensive gates for the new evidence and research boundaries
test("adjustment scorecard paths cannot narrow relevant coverage", () => {
  // retain all platform checks at shared deployment and migration boundaries
  for (const path of [
    "deploy/scripts/adjustment-evaluation-export.sh",
    "deploy/scripts/adjustment-evidence-store.mjs",
    "packages/database/migrations/0017_adjustment_evaluation_export.sql",
  ]) {
    assert.deepEqual(classifyPaths([path]), allGates);
  }
  // retain backend and model checks for numerical and paired scoring work
  for (const path of [
    "packages/forecast-adjustment/src/performance-scorecard.ts",
    "apps/worker/src/forecast-adjustment-performance-cli.ts",
    "scripts/research/temperature_refresh.py",
  ]) {
    const selected = classifyPaths([path]);
    assert.equal(selected.fullTests, true);
    assert.equal(selected.integration, !path.startsWith("scripts/research/"));
    assert.equal(selected.deployIntegration, false);
  }
});

// untrusted ancestry must replay all gates
test("missing or failed baseline Check forces full inherited coverage", () => {
  const base = "a".repeat(40);
  const head = "b".repeat(40);
  const unverified = classifyCommitRange("push", base, head, false);
  assert.deepEqual(unverified, allGates);
  assert.deepEqual(classifyCommitRange("push", base, head, true), allGates);
  assert.deepEqual(classifyCommitRange("schedule", "", ""), allGates);
  assert.deepEqual(classifyCommitRange("workflow_dispatch", "", ""), allGates);

  // unverified baselines cannot excuse skipped native jobs
  assert.throws(() => verifyRequiredJobs(
    {
      nativeAndroid: String(unverified.nativeAndroid),
    },
    {
      changeSelection: "success",
      nativeAndroid: "skipped",
      qualityGates: "success",
    },
  ), /selected but finished as skipped/u);
});

// aggregate accepts only exact success or explicitly unselected skips
test("Check aggregate rejects missing failed and malformed native evidence", () => {
  const requiredResults = {
    changeSelection: "success",
    nativeAndroid: "success",
    qualityGates: "success",
  };
  assert.equal(verifyRequiredJobs(
    { nativeAndroid: "true" },
    requiredResults,
  ), true);
  assert.equal(verifyRequiredJobs(
    { nativeAndroid: "false" },
    { ...requiredResults, nativeAndroid: "skipped" },
  ), true);

  // reject every missing mandatory or selected result
  for (const [selection, results, pattern] of [
    [
      { nativeAndroid: "true" },
      { ...requiredResults, changeSelection: "failure" },
      /change selection/u,
    ],
    [
      { nativeAndroid: "true" },
      { ...requiredResults, qualityGates: "cancelled" },
      /quality gates/u,
    ],
    [
      { nativeAndroid: "true" },
      { ...requiredResults, nativeAndroid: "skipped" },
      /selected but finished as skipped/u,
    ],
    [
      { nativeAndroid: "false" },
      { ...requiredResults, nativeAndroid: "success" },
      /not selected but finished as success/u,
    ],
    [
      { nativeAndroid: "" },
      requiredResults,
      /selection is missing or invalid/u,
    ],
    [
      { nativeAndroid: "true" },
      { ...requiredResults, nativeAndroid: "" },
      /selected but finished as missing/u,
    ],
  ]) {
    assert.throws(() => verifyRequiredJobs(selection, results), pattern);
  }
});

// workflow wiring keeps native lanes parallel and the aggregate fail closed
test("Check workflow binds selected native jobs to the exact commit", async () => {
  const workflow = await readFile(new URL("../.github/workflows/check.yml", import.meta.url), "utf8");
  const selection = workflowJob(workflow, "change-selection");
  const quality = workflowJob(workflow, "quality-gates");
  const android = workflowJob(workflow, "native-android");
  const aggregate = workflowJob(workflow, "check-complete");
  assert.match(selection, /native_android:/u);
  assert.match(android, /needs: change-selection/u);
  assert.match(android, /if: needs\.change-selection\.outputs\.native_android == 'true'/u);
  assert.match(
    android,
    /android-actions\/setup-android@9fc6c4e9069bf8d3d10b2204b1fb8f6ef7065407[\s\S]*packages: platform-tools/u,
  );
  assert.match(android, /'platforms;android-37\.0'/u);
  assert.doesNotMatch(android, /'platforms;android-37'/u);
  assert.match(android, /apt-get install --yes libpulse0/u);
  assert.match(android, /GenerateBrandAssets\.java --check/u);
  assert.match(android, /with-native-https-fixture\.sh[\s\S]*run-hosted-shell-tests\.sh/u);
  assert.match(android, /android-webview\/TEST-widgetPhone\.xml/u);
  assert.match(android, /"tests": "29"/u);
  assert.match(android, /managedDevice\/debug\/widgetPhone\/TEST-widgetPhone\.xml/u);
  assert.match(quality, /python3 mobile\/scripts\/native_https_fixture_test\.py/u);
  assert.match(aggregate, /if: always\(\)[\s\S]*CI_CHANGES_MODE: aggregate/u);
  assert.equal((workflow.match(/ref: \$\{\{ github\.sha \}\}/gu) ?? []).length, 4);
  assert.doesNotMatch(workflow, /continue-on-error:/u);
  assert.doesNotMatch(workflow, /native[-_]ios|CI_NATIVE_IOS/u);
});
