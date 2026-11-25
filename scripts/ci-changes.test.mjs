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
  nativeRustArm64: true,
};
const baselineOnly = {
  fullTests: false,
  integration: false,
  deployIntegration: false,
  fullBrowser: false,
  nativeAndroid: false,
  nativeRustArm64: false,
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
    nativeRustArm64: true,
  });
});

// platform-only changes select only their matching native lane
test("native platform paths preserve the mandatory linux baseline", () => {
  assert.deepEqual(classifyPaths(["mobile/android/app/src/main/Main.kt"]), {
    ...baselineOnly,
    nativeAndroid: true,
  });
  assert.deepEqual(classifyPaths(["mobile/ios/WeatherApp/App.swift"]), {
    ...allGates,
    nativeRustArm64: false,
  });
  assert.deepEqual(
    classifyPaths(["mobile/ios/WeatherApp/App.swift", "rust/crates/weather-api/src/lib.rs"]),
    allGates,
  );
  assert.deepEqual(
    classifyPaths(["rust/crates/weather-api/src/lib.rs", "mobile/ios/WeatherApp/App.swift"]),
    allGates,
  );
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
  assert.deepEqual(classifyPaths(renamed), {
    ...allGates,
    nativeRustArm64: false,
  });
});

// transitive boundaries select their dependent suites
test("UI, model, backend, and deployment changes select dependent suites", () => {
  assert.equal(classifyPaths(["apps/web/src/styles.css"]).fullBrowser, true);
  assert.equal(classifyPaths(["apps/web/src/styles.css"]).nativeRustArm64, true);
  assert.equal(classifyPaths(["scripts/research/rain_fit.py"]).fullTests, true);
  assert.equal(classifyPaths(["packages/forecast-adjustment/src/main.ts"]).integration, true);
  assert.equal(
    classifyPaths(["packages/forecast-adjustment/src/main.ts"]).nativeRustArm64,
    true,
  );
  assert.equal(classifyPaths(["apps/api/src/main.ts"]).integration, true);
  assert.equal(classifyPaths(["apps/api/src/main.ts"]).nativeRustArm64, true);
  assert.equal(classifyPaths(["packages/domain/src/main.ts"]).deployIntegration, true);
  assert.equal(classifyPaths(["packages/database/migrations/0017.sql"]).deployIntegration, true);
  assert.equal(classifyPaths(["packages/providers/src/index.ts"]).deployIntegration, true);
  assert.equal(classifyPaths(["apps/worker/src/index.ts"]).fullTests, true);
  assert.equal(classifyPaths(["apps/worker/src/index.ts"]).nativeRustArm64, true);
  assert.equal(classifyPaths(["deploy/compose.yaml"]).deployIntegration, true);
  assert.equal(classifyPaths(["deploy/compose.yaml"]).nativeRustArm64, true);
  assert.equal(classifyPaths([".github/workflows/check.yml"]).deployIntegration, true);
  assert.equal(classifyPaths([".github/workflows/check.yml"]).nativeRustArm64, true);
  assert.equal(classifyPaths(["package-lock.json"]).deployIntegration, true);
  assert.equal(classifyPaths(["deploy/compose.yaml"]).nativeAndroid, true);
  assert.equal(classifyPaths(["config/sites/ballydidean.json"]).nativeAndroid, true);
  assert.equal(classifyPaths(["rust/crates/weather-api/src/lib.rs"]).nativeRustArm64, true);
  assert.equal(classifyPaths(["packages/contracts/src/index.ts"]).nativeRustArm64, true);
  assert.equal(classifyPaths(["scripts/generate-rust-contracts.mjs"]).nativeRustArm64, true);
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
      nativeRustArm64: String(unverified.nativeRustArm64),
    },
    {
      changeSelection: "success",
      nativeAndroid: "skipped",
      nativeRustArm64: "skipped",
      qualityGates: "success",
    },
  ), /selected but finished as skipped/u);
});

// aggregate accepts only exact success or explicitly unselected skips
test("Check aggregate rejects missing failed and malformed native evidence", () => {
  const requiredResults = {
    changeSelection: "success",
    nativeAndroid: "success",
    nativeRustArm64: "success",
    qualityGates: "success",
  };
  assert.equal(verifyRequiredJobs(
    { nativeAndroid: "true", nativeRustArm64: "true" },
    requiredResults,
  ), true);
  assert.equal(verifyRequiredJobs(
    { nativeAndroid: "false", nativeRustArm64: "false" },
    { ...requiredResults, nativeAndroid: "skipped", nativeRustArm64: "skipped" },
  ), true);

  // reject every missing mandatory or selected result
  for (const [selection, results, pattern] of [
    [
      { nativeAndroid: "true", nativeRustArm64: "true" },
      { ...requiredResults, changeSelection: "failure" },
      /change selection/u,
    ],
    [
      { nativeAndroid: "true", nativeRustArm64: "true" },
      { ...requiredResults, qualityGates: "cancelled" },
      /quality gates/u,
    ],
    [
      { nativeAndroid: "true", nativeRustArm64: "true" },
      { ...requiredResults, nativeAndroid: "skipped" },
      /selected but finished as skipped/u,
    ],
    [
      { nativeAndroid: "false", nativeRustArm64: "false" },
      { ...requiredResults, nativeAndroid: "success" },
      /not selected but finished as success/u,
    ],
    [
      { nativeAndroid: "", nativeRustArm64: "true" },
      requiredResults,
      /selection is missing or invalid/u,
    ],
    [
      { nativeAndroid: "true", nativeRustArm64: "true" },
      { ...requiredResults, nativeAndroid: "" },
      /selected but finished as missing/u,
    ],
    [
      { nativeAndroid: "true", nativeRustArm64: "true" },
      { ...requiredResults, nativeRustArm64: "skipped" },
      /native Rust ARM64 was selected but finished as skipped/u,
    ],
    [
      { nativeAndroid: "true", nativeRustArm64: "false" },
      requiredResults,
      /native Rust ARM64 was not selected but finished as success/u,
    ],
    [
      { nativeAndroid: "true", nativeRustArm64: "" },
      requiredResults,
      /native Rust ARM64 selection is missing or invalid/u,
    ],
    [
      { nativeAndroid: "true", nativeRustArm64: "true" },
      { ...requiredResults, nativeRustArm64: "" },
      /native Rust ARM64 was selected but finished as missing/u,
    ],
  ]) {
    assert.throws(() => verifyRequiredJobs(selection, results), pattern);
  }
});

// run the declared runtime generations before compiling the selected commit
test("Check gates compilation on the current runtime inventory", async () => {
  const workflow = await readFile(new URL("../.github/workflows/check.yml", import.meta.url), "utf8");
  const quality = workflowJob(workflow, "quality-gates");
  const name = "      - name: Verify the current runtime inventory\n";
  const start = quality.indexOf(name);
  const install = quality.indexOf("        run: npm ci\n");
  const build = quality.indexOf("        run: npm run build\n");
  assert.ok(install !== -1 && install < start && start < build);
  const step = quality.slice(start).split("\n      - ", 1)[0];
  assert.deepEqual(step.trimEnd().split("\n").slice(1), [
    "        shell: bash",
    "        run: |",
    "          set -euo pipefail",
    "          npm run runtime-inventory:check -- --generation node-v1 --phase baseline",
    "          npm run runtime-inventory:check -- --generation node-v1 --phase rollback-drill",
    "          npm run runtime-inventory:check -- --generation mixed --phase pre-cutover",
    "          npm run runtime-inventory:check -- --generation rust-v1 --phase pre-cutover",
  ]);
});

// workflow wiring keeps native lanes parallel and the aggregate fail closed
test("Check workflow binds selected native jobs to the exact commit", async () => {
  const workflow = await readFile(new URL("../.github/workflows/check.yml", import.meta.url), "utf8");
  const selection = workflowJob(workflow, "change-selection");
  const quality = workflowJob(workflow, "quality-gates");
  const rust = workflowJob(workflow, "native-rust-arm64");
  const android = workflowJob(workflow, "native-android");
  const aggregate = workflowJob(workflow, "check-complete");
  assert.match(selection, /native_android:/u);
  assert.match(selection, /native_rust_arm64:/u);
  assert.match(rust, /needs: \[change-selection, quality-gates\]/u);
  assert.match(rust, /if: needs\.change-selection\.outputs\.native_rust_arm64 == 'true'/u);
  assert.match(rust, /runs-on: ubuntu-24\.04-arm/u);
  assert.match(quality, /cargo fetch --locked[\s\S]*cargo test --workspace --all-targets --locked --offline/u);
  assert.match(rust, /cargo fetch --locked --target aarch64-unknown-linux-gnu[\s\S]*cargo test --locked --offline --target aarch64-unknown-linux-gnu/u);
  assert.match(quality, /cargo test --workspace --all-targets --locked --offline --features weather-runtime\/node-zlib-candidate/u);
  assert.match(rust, /cargo test --locked --offline --target aarch64-unknown-linux-gnu -p weather-runtime --features node-zlib-candidate --lib native_node_gzip_candidate/u);
  assert.match(rust, /run_native_suite weather-runtime native_cold_custody_command private_archive_native_http_transport_matches_node/u);
  assert.match(rust, /run_native_suite weather-runtime native_cold_custody_command native_evidence_command_emits_and_reenters_actual_ack/u);
  assert.match(rust, /run_native_suite weather-runtime native_evidence_read_commands/u);
  assert.match(rust, /run_native_suite weather-runtime native_evidence_gap_commands/u);
  assert.match(rust, /run_native_suite weather-runtime revision_cold_start native_evidence_cold_start_matches_node_database_envelopes/u);
  assert.match(rust, /run_native_suite weather-runtime shadow_metadata_custody/u);
  assert.match(rust, /run_native_suite weather-runtime native_edge_http_transport/u);
  assert.match(rust, /run_native_suite weather-runtime native_edge_process_http/u);
  assert.match(rust, /run_native_suite weather-runtime native_worker_process/u);
  assert.match(rust, /run_native_suite weather-runtime native_worker_health/u);
  assert.match(rust, /cargo test --locked --offline --target aarch64-unknown-linux-gnu -p weather-runtime --bin weather-http-health/u);
  assert.match(rust, /run_native_suite weather-runtime native_worker_catalogs/u);
  assert.match(rust, /run_native_suite weather-runtime native_rain_capture_values/u);
  assert.match(
    rust,
    /run_native_lib_suite weather-runtime native_postgres -- --skip native_postgres_real_roundtrip_test::native_pool_roundtrips_typed_values_against_postgres_16\n/u,
  );
  assert.match(rust, /run_native_lib_suite weather-runtime native_outbound_http_test/u);
  assert.match(rust, /run_native_lib_suite weather-runtime native_open_meteo_transport::tests/u);
  assert.match(rust, /run_native_suite weather-runtime native_wire_primitives/u);
  assert.match(rust, /run_native_suite weather-runtime api_repository native_/u);
  assert.doesNotMatch(rust, /cargo test[^\n]*(?:--workspace|--all-targets)/u);
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
  assert.match(
    aggregate,
    /CI_NATIVE_RUST_ARM64: \$\{\{ needs\.change-selection\.outputs\.native_rust_arm64 \}\}/u,
  );
  assert.match(
    aggregate,
    /CI_NATIVE_RUST_ARM64_RESULT: \$\{\{ needs\.native-rust-arm64\.result \}\}/u,
  );
  assert.equal((workflow.match(/ref: \$\{\{ github\.sha \}\}/gu) ?? []).length, 5);
  assert.doesNotMatch(workflow, /continue-on-error:/u);
  assert.doesNotMatch(workflow, /native[-_]ios|CI_NATIVE_IOS/u);
});

// keep native candidate images inside the selected ARM64 gate and inspect loaded bytes without execution
test("Check workflow inspects native ARM64 candidate images against independent material", async () => {
  const workflow = await readFile(new URL("../.github/workflows/check.yml", import.meta.url), "utf8");
  const rust = workflowJob(workflow, "native-rust-arm64");
  assert.match(rust, /runs-on: ubuntu-24\.04-arm[\s\S]*timeout-minutes: 75/u);
  assert.match(rust, /uses: docker\/setup-buildx-action@v3/u);
  assert.match(
    rust,
    /docker buildx build --platform linux\/arm64 --target native-server[\s\S]*--tag weather-native-server:check --load/u,
  );
  assert.match(
    rust,
    /docker buildx build --platform linux\/arm64 --target native-web[\s\S]*--tag weather-native-web:check --load/u,
  );
  assert.equal(
    (rust.match(/--build-arg "WEATHER_SOURCE_REVISION=\$\{GITHUB_SHA\}"/gu) ?? []).length,
    2,
  );
  assert.match(
    rust,
    /docker buildx build --platform linux\/arm64 --target native-runtime-material-build[\s\S]*--output "type=local,dest=\$\{material_root\}"/u,
  );
  assert.match(rust, /docker image inspect weather-native-server:check/u);
  assert.match(rust, /docker image inspect weather-native-web:check/u);
  assert.match(rust, /assert\.equal\(image\.Architecture, "arm64"\)/u);
  assert.match(rust, /assert\.equal\(image\.Os, "linux"\)/u);
  assert.match(rust, /assert\.deepEqual\(image\.Config\.Cmd, command\)/u);
  assert.match(rust, /assert\.deepEqual\(image\.Config\.Env, environment\)/u);
  assert.match(rust, /"org\.opencontainers\.image\.revision": process\.env\.GITHUB_SHA/u);
  assert.match(rust, /"org\.weather\.runtime\.generation": "rust-v1"/u);
  assert.match(rust, /\["\/opt\/weather\/bin\/weather-api"\]/u);
  assert.match(rust, /\["\/opt\/weather\/bin\/weather-web"\]/u);
  assert.match(rust, /docker create weather-native-server:check/u);
  assert.match(rust, /docker create weather-native-web:check/u);
  assert.match(rust, /docker cp "\$\{server_container\}:\/opt\/weather\/\." "\$\{server_root\}\/"/u);
  assert.match(rust, /docker cp "\$\{web_container\}:\/opt\/weather\/\." "\$\{web_root\}\/"/u);
  assert.match(
    rust,
    /node deploy\/scripts\/build-native-runtime-image-material\.mjs[\s\S]*"\$\{GITHUB_SHA\}" "\$\{GITHUB_WORKSPACE\}" "\$\{material_root\}"/u,
  );
  assert.match(
    rust,
    /node deploy\/scripts\/rust-runtime-image-material\.mjs --inspect[\s\S]*"\$\{inspection_root\}\/expected\.json" "\$\{server_root\}" "\$\{web_root\}"/u,
  );
  assert.doesNotMatch(rust, /docker run/u);
  assert.doesNotMatch(rust, /--target (?:server|web)(?:\s|\\)/u);
});
