import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";
import {
  buildInstalledAdjustmentRainControlReferenceCatalog,
  executeAdjustmentFamilyRelease,
  reconcileAdjustmentFamilySettings,
  validateAdjustmentFamilyAction,
  validateAdjustmentFamilyChangedPaths,
  validateAdjustmentFamilyBootstrapLiveAuthority,
  validateAdjustmentRainControlReferenceAction,
  validateAdjustmentRainControlReferenceBinding,
  validateAdjustmentRainControlReferenceChangedPaths,
  verifyAdjustmentFamilyGitRelease,
  verifyAdjustmentFullV14GitRelease,
  verifyAdjustmentInertV14GitRelease,
} from "../scripts/adjustment-evaluation-package.mjs";
import {
  buildAdjustmentModelAction,
  buildAdjustmentRainControlReferenceAction,
} from "../../scripts/research/adjustment_model_release.mjs";

// construct real publisher bytes for independent privilege-boundary verification
function fixture(overrides = {}) {
  const built = buildAdjustmentModelAction({
    actionKind: "promote", candidateGraphSha256: "a".repeat(64), candidateSha256: "b".repeat(64),
    contractVersion: "forecast-adjustment-model-action/v1", createdAt: "2027-01-10T08:00:00.000Z",
    expectedSettingsSha256: "c".repeat(64), expectedSourceCommit: "1".repeat(40),
    expectedInstalledReceiptSha256: null,
    expectedSourceRelease: "2027.01.09-1", family: "temperature", fencingToken: "1",
    fullMemberRootSha256: "d".repeat(64), lifecycleHeadSha256: "e".repeat(64),
    policyDecision: "qualified", policyReportSha256: "f".repeat(64), predecessorActionSha256: null,
    reason: "qualified_candidate", reportCreatedAt: "2027-01-10T08:00:00.000Z", siteKey: "ballydidean",
    validThrough: "2027-01-17T08:00:00.000Z", ...overrides,
  });
  return { ...built, expected: { actionSha256: built.actionSha256, family: built.action.family,
    now: "2027-01-10T09:00:00.000Z", reportSha256: built.action.policyReportSha256,
    sourceCommit: built.action.expectedSourceCommit, sourceRelease: built.action.expectedSourceRelease } };
}

// construct exact control-reference bytes for the privileged boundary
function controlFixture(overrides = {}) {
  const built = buildAdjustmentRainControlReferenceAction({
    actionKind: "control_reference",
    contractVersion: "forecast-adjustment-rain-control-reference-action/v1",
    controlStateSha256: "1".repeat(64),
    createdAt: "2027-01-25T01:00:00.000Z",
    dueMonth: "2027-02",
    expectedCatalogReceiptSha256: null,
    expectedSettingsSha256: "2".repeat(64),
    expectedSourceCommit: "3".repeat(40),
    expectedSourceRelease: "2027.01.24-1",
    family: "rain",
    fencingToken: "1",
    graphManifestSha256: "4".repeat(64),
    ordinalArtifactSha256: "5".repeat(64),
    predecessorActionSha256: null,
    reason: "premonth_reference",
    sourceMemberRootSha256: "6".repeat(64),
    sourceReceiptRootSha256: "7".repeat(64),
    validThrough: "2027-02-01T00:00:00.000Z",
    ...overrides,
  });
  return {
    ...built,
    expected: {
      actionSha256: built.actionSha256,
      now: "2027-01-25T02:00:00.000Z",
      sourceCommit: built.action.expectedSourceCommit,
      sourceRelease: built.action.expectedSourceRelease,
    },
  };
}

// keep bootstrap provenance immutable while admitting current valid operator intent
test("family bootstrap admits a later valid settings snapshot", async () => {
  const settingsA = Buffer.from('{"rain":true,"temperature":true,"version":1,"wind":true}\n');
  const settingsB = Buffer.from('{"rain":true,"temperature":false,"version":1,"wind":true}\n');
  const catalogBytes = Buffer.from('{"contractVersion":"adjustment-installed-candidate-catalog/v2","entries":[]}\n');
  const environment = {
    WEATHER_SERVER_IMAGE: `ghcr.io/anstosa/weather-server@sha256:${"1".repeat(64)}`,
    WEATHER_WEB_IMAGE: `ghcr.io/anstosa/weather-web@sha256:${"2".repeat(64)}`,
  };
  const active = {
    bootstrappedAt: "2026-10-09T12:00:00.000Z",
    catalogSha256: createHash("sha256").update(catalogBytes).digest("hex"),
    commit: "3".repeat(40),
    contractVersion: "adjustment-family-release-bootstrap-current/v1",
    imageDigestsSha256: createHash("sha256").update(Buffer.from(`${JSON.stringify([
      { digest: environment.WEATHER_SERVER_IMAGE.split("@")[1], runtime: "server" },
      { digest: environment.WEATHER_WEB_IMAGE.split("@")[1], runtime: "web" },
    ])}\n`)).digest("hex"),
    release: "2026.10.09-3",
    serverImage: environment.WEATHER_SERVER_IMAGE,
    settingsSha256: createHash("sha256").update(settingsA).digest("hex"),
    state: "inert_v14_bootstrap",
    webImage: environment.WEATHER_WEB_IMAGE,
  };
  assert.equal(
    validateAdjustmentFamilyBootstrapLiveAuthority(active, environment, catalogBytes, settingsB),
    active.commit,
  );
  assert.throws(() => validateAdjustmentFamilyBootstrapLiveAuthority(
    active,
    environment,
    catalogBytes,
    Buffer.from('{"temperature":"invalid"}\n'),
  ));
  const built = fixture({ expectedSettingsSha256: createHash("sha256").update(settingsB).digest("hex") });
  const input = {
    actionSha256: built.actionSha256,
    compensatingRelease: "2026.10.10-2",
    expectedCurrentRelease: active.release,
    expectedSettingsSha256: built.action.expectedSettingsSha256,
    expectedSourceRelease: active.release,
    family: "temperature",
    fencingToken: "1",
    reportSha256: built.action.policyReportSha256,
    targetRelease: "2026.10.10-1",
  };
  const result = await executeAdjustmentFamilyRelease(input, {
    acknowledge: async () => assert.fail("disabled family must not mutate"),
    acknowledgeOperatorOff: async (record) => record,
    applyImages: async () => assert.fail("disabled family must not apply"),
    clock: () => "2026-10-09T13:00:00.000Z",
    inspectCurrent: async () => ({ commit: active.commit, installedReceiptSha256: null,
      maximumFence: "0", release: active.release, settingsBytes: settingsB }),
    markApplying: async () => assert.fail("disabled family must not apply"),
    prepare: async () => assert.fail("disabled family must not prepare"),
    readPrepared: async () => null,
    recordFailure: async () => assert.fail("disabled family must not fail"),
    verifyLive: async () => assert.fail("disabled family must not verify"),
    verifyProof: async () => assert.fail("disabled family must not prove"),
    withReleaseLock: async (callback) => await callback(),
  });
  assert.equal(result.state, "operator_off_unapplied");
  assert.equal(result.sourceSettingsSha256, input.expectedSettingsSha256);
});

// exact action, family, source and report identities cannot drift independently
test("root family action independently validates publisher bytes and closed freshness", () => {
  const built = fixture();
  assert.deepEqual(validateAdjustmentFamilyAction(built.bytes, built.expected), built.action);
  assert.throws(() => validateAdjustmentFamilyAction(Buffer.from(JSON.stringify(built.action)), built.expected));
  assert.throws(() => validateAdjustmentFamilyAction(built.bytes, { ...built.expected, family: "wind" }));
  assert.throws(() => validateAdjustmentFamilyAction(built.bytes, { ...built.expected, sourceCommit: "2".repeat(40) }));
  assert.throws(() => validateAdjustmentFamilyAction(built.bytes, { ...built.expected, now: "2027-01-17T08:00:00.001Z" }), /expired/u);
  const recovery = fixture({ actionKind: "compensate_incumbent", reason: "failed_promotion",
    predecessorActionSha256: "9".repeat(64), createdAt: "2027-01-18T08:00:00.000Z" });
  assert.deepEqual(validateAdjustmentFamilyAction(recovery.bytes,
    { ...recovery.expected, now: "2027-01-18T09:00:00.000Z" }), recovery.action);
});

test("root rain control action and diff remain disjoint from serving changes", () => {
  const built = controlFixture();
  assert.deepEqual(
    validateAdjustmentRainControlReferenceAction(built.bytes, built.expected),
    built.action,
  );
  const files = [
    { filename: built.path, status: "added" },
    {
      filename: "config/forecast-adjustments/ballydidean/rain-maintenance-control-states/" +
        `sha256-${built.action.controlStateSha256}.json`,
      status: "added",
    },
    {
      filename: "config/forecast-adjustments/ballydidean/rain-runtime-artifacts/" +
        `sha256-${built.action.ordinalArtifactSha256}.json`,
      status: "added",
    },
    {
      filename: "config/forecast-adjustments/ballydidean-rain-control-reference.json",
      status: "modified",
    },
  ];
  assert.equal(validateAdjustmentRainControlReferenceChangedPaths(built.action, files), true);
  assert.throws(() => validateAdjustmentRainControlReferenceChangedPaths(built.action, [
    ...files,
    {
      filename: "config/forecast-adjustments/ballydidean-rain-runtime.json",
      status: "modified",
    },
  ]), /not closed/u);
  const compensation = controlFixture({
    actionKind: "compensate_control_reference",
    predecessorActionSha256: built.actionSha256,
    reason: "failed_control_reference",
  });
  assert.equal(validateAdjustmentRainControlReferenceChangedPaths(compensation.action, [
    { filename: compensation.path, status: "added" },
    {
      filename: "config/forecast-adjustments/ballydidean-rain-control-reference.json",
      status: "removed",
    },
  ]), true);
  assert.deepEqual(validateAdjustmentRainControlReferenceAction(
    compensation.bytes,
    { ...compensation.expected, now: "2027-02-02T00:00:00.000Z" },
  ), compensation.action);
});

test("rain control catalog v3 adds only the custody-bound control slot", () => {
  const settingsBytes = Buffer.from('{"rain":true,"temperature":true,"version":1,"wind":true}\n');
  const stateBytes = Buffer.from('{"state":"fixture"}\n');
  const artifactBytes = Buffer.from('{"artifact":"fixture"}\n');
  const built = controlFixture({
    controlStateSha256: createHash("sha256").update(stateBytes).digest("hex"),
    expectedSettingsSha256: createHash("sha256").update(settingsBytes).digest("hex"),
    ordinalArtifactSha256: createHash("sha256").update(artifactBytes).digest("hex"),
  });
  const selector = Buffer.from(`${JSON.stringify({
    actionSha256: built.actionSha256,
    contractVersion: "forecast-adjustment-rain-control-reference-registry/v1",
    controlStatePath: `rain-maintenance-control-states/sha256-${built.action.controlStateSha256}.json`,
    controlStateSha256: built.action.controlStateSha256,
    dueMonth: built.action.dueMonth,
    ordinalArtifactPath: `rain-runtime-artifacts/sha256-${built.action.ordinalArtifactSha256}.json`,
    ordinalArtifactSha256: built.action.ordinalArtifactSha256,
    siteKey: "ballydidean",
  })}\n`);
  const catalog = buildInstalledAdjustmentRainControlReferenceCatalog({
    action: built.action,
    actionSha256: built.actionSha256,
    clock: () => "2027-01-25T02:00:00.000Z",
    custodyAnchorSha256: "8".repeat(64),
    environment: {
      WEATHER_CONTROL_PLANE_SHA256: "9".repeat(64),
      WEATHER_RELEASE: "2027.01.25-1",
      WEATHER_SERVER_IMAGE: `ghcr.io/anstosa/weather-server@sha256:${"a".repeat(64)}`,
    },
    imageCommit: "b".repeat(40),
    imageRead: (path) => path.endsWith("ballydidean-rain-control-reference.json")
      ? selector
      : path.includes("control-states") ? stateBytes : artifactBytes,
    settingsBytes,
    source: { contractVersion: "adjustment-installed-candidate-catalog/v2", entries: [] },
  });
  assert.equal(catalog.contractVersion, "adjustment-installed-candidate-catalog/v3");
  assert.equal(catalog.entries.length, 1);
  assert.equal(catalog.entries[0].slot, "control");
  assert.equal(catalog.entries[0].registration, null);
  assert.equal(catalog.entries[0].receipt.custodyAnchorSha256, "8".repeat(64));
  assert.throws(() => buildInstalledAdjustmentRainControlReferenceCatalog({
    action: { ...built.action, expectedCatalogReceiptSha256: "c".repeat(64) },
    actionSha256: built.actionSha256,
    clock: () => "2027-01-25T02:00:00.000Z",
    custodyAnchorSha256: "8".repeat(64),
    environment: {
      WEATHER_CONTROL_PLANE_SHA256: "9".repeat(64),
      WEATHER_RELEASE: "2027.01.25-1",
      WEATHER_SERVER_IMAGE: `ghcr.io/anstosa/weather-server@sha256:${"a".repeat(64)}`,
    },
    imageCommit: "b".repeat(40),
    imageRead: () => selector,
    settingsBytes,
    source: { contractVersion: "adjustment-installed-candidate-catalog/v2", entries: [] },
  }), /baseline differs/u);
});

test("family transaction authorizes a disjoint custody-only rain control pair", async () => {
  const settingsBytes = Buffer.from('{"version":1,"temperature":true,"wind":true,"rain":true}\n');
  const settingsSha256 = createHash("sha256").update(settingsBytes).digest("hex");
  const target = controlFixture({ expectedSettingsSha256: settingsSha256 });
  const targetCommit = "4".repeat(40);
  const compensation = controlFixture({
    actionKind: "compensate_control_reference",
    createdAt: "2027-01-25T01:30:00.000Z",
    expectedSettingsSha256: settingsSha256,
    expectedSourceCommit: targetCommit,
    expectedSourceRelease: "2027.01.25-1",
    predecessorActionSha256: target.actionSha256,
    reason: "failed_control_reference",
  });
  const input = {
    actionSha256: target.actionSha256,
    compensatingRelease: "2027.01.25-2",
    expectedCurrentRelease: target.action.expectedSourceRelease,
    expectedSettingsSha256: settingsSha256,
    expectedSourceRelease: target.action.expectedSourceRelease,
    family: "rain",
    fencingToken: "1",
    reportSha256: target.action.graphManifestSha256,
    targetRelease: "2027.01.25-1",
  };
  const authority = {
    actionKind: "control_reference",
    actionSha256: target.actionSha256,
    contractVersion: "adjustment-rain-control-custody-anchor/v1",
    controlStateSha256: target.action.controlStateSha256,
    dueMonth: target.action.dueMonth,
    fencingToken: target.action.fencingToken,
    graphManifestSha256: target.action.graphManifestSha256,
    ordinalArtifactSha256: target.action.ordinalArtifactSha256,
    sourceCommit: target.action.expectedSourceCommit,
    sourceMemberRootSha256: target.action.sourceMemberRootSha256,
    sourceReceiptRootSha256: target.action.sourceReceiptRootSha256,
  };
  const proof = {
    actionBytes: target.bytes,
    authority,
    capacity: {
      actionSha256: input.actionSha256,
      compensationScope: "family-only-new-release-compensation",
      contractVersion: "adjustment-family-release-capacity/v1",
      family: "rain",
      imageDigests: ["source", "target", "compensating"].flatMap(
        // bind the six reserved image identities
        (role, index) => ["server", "web"].map((runtime, offset) => ({
          digest: `sha256:${["a", "b", "c", "d", "e", "f"][index * 2 + offset].repeat(64)}`,
          role,
          runtime,
        })),
      ),
      measuredAt: "2027-01-25T02:00:00.000Z",
      retirementCreditBytes: 0,
      sourceRelease: input.expectedSourceRelease,
      state: "capacity_ready",
    },
    compensation: {
      actionBytes: compensation.bytes,
      actionSha256: compensation.actionSha256,
      git: { sourceCommit: targetCommit, targetRelease: input.compensatingRelease },
    },
    familyIdentitySha256: "8".repeat(64),
    git: {
      sourceCommit: target.action.expectedSourceCommit,
      targetCommit,
      targetRelease: input.targetRelease,
    },
    imageDigestsSha256: "9".repeat(64),
    targetCommit,
  };
  let record = null;
  const result = await executeAdjustmentFamilyRelease(input, {
    acknowledge: async (next) => next,
    applyImages: async () => undefined,
    clock: () => "2027-01-25T02:00:00.000Z",
    inspectCurrent: async () => ({
      commit: target.action.expectedSourceCommit,
      installedReceiptSha256BySlot: { active: null, control: null, shadow: null },
      maximumFence: "0",
      release: input.expectedSourceRelease,
      settingsBytes,
    }),
    markApplying: async () => undefined,
    prepare: async (next) => { record = next; },
    readPrepared: async () => record,
    recordFailure: async () => undefined,
    verifyLive: async () => ({
      commit: targetCommit,
      family: "rain",
      familyIdentitySha256: proof.familyIdentitySha256,
      imageDigestsSha256: proof.imageDigestsSha256,
      release: input.targetRelease,
      settingsBytes,
    }),
    verifyProof: async () => proof,
    withReleaseLock: async (operation) => await operation(),
  });
  assert.equal(result.state, "acknowledged");
  const catalog = { entries: [{ family: "rain", receipt: {
    actionSha256: target.actionSha256,
    controlStateSha256: target.action.controlStateSha256,
    custodyAnchorSha256: createHash("sha256").update(`${JSON.stringify(authority)}\n`).digest("hex"),
    graphManifestSha256: target.action.graphManifestSha256,
    ordinalArtifactSha256: target.action.ordinalArtifactSha256,
    sourceMemberRootSha256: target.action.sourceMemberRootSha256,
    sourceReceiptRootSha256: target.action.sourceReceiptRootSha256,
  }, registration: null, slot: "control" }] };
  assert.equal(validateAdjustmentRainControlReferenceBinding(
    target.action,
    target.actionSha256,
    authority,
    catalog,
  ), true);
});

// immutable refs and latest exact-SHA workflow attempts are independent evidence
test("root family Git proof rejects hidden changes, incomplete results and stale Check success", async () => {
  const built = fixture();
  const target = "2".repeat(40);
  const repository = {
    fork: false,
    full_name: "anstosa/Ballydidean-Weather",
    id: 1_342_404_160,
  };
  // create one complete scoped successful workflow run
  const run = (filename, id, created_at, updated_at) => ({ id, run_attempt: 1, created_at, updated_at,
    head_sha: target, event: "push", repository: { ...repository }, head_repository: { ...repository },
    path: `.github/workflows/${filename}`, status: "completed", conclusion: "success" });
  const responses = new Map([
    [`git/ref/tags/${built.action.expectedSourceRelease}`, { object: { type: "commit", sha: "1".repeat(40) } }],
    ["git/ref/tags/2027.01.10-1", { object: { type: "commit", sha: target } }],
    [`compare/${"1".repeat(40)}...${target}?per_page=100`, { status: "ahead", ahead_by: 1, behind_by: 0,
      total_commits: 1, base_commit: { sha: "1".repeat(40) }, commits: [{ sha: target, parents: [{ sha: "1".repeat(40) }] }],
      files: [{ filename: built.path, status: "added" },
        { filename: "config/forecast-adjustments/ballydidean-temperature-canary.json", status: "modified" }] }],
    [`actions/workflows/check.yml/runs?head_sha=${target}&event=push&per_page=100`, {
      total_count: 1, workflow_runs: [run("check.yml", 1, "2027-01-10T08:01:00Z", "2027-01-10T08:10:00Z")] }],
    [`actions/workflows/publish-images.yml/runs?head_sha=${target}&event=push&per_page=100`, {
      total_count: 1, workflow_runs: [run("publish-images.yml", 2, "2027-01-10T08:11:00Z", "2027-01-10T08:20:00Z")] }],
  ]);
  // return immutable fixture metadata through the same closed production paths
  const readJson = async (path) => structuredClone(responses.get(path));
  const request = { action: built.action, targetRelease: "2027.01.10-1", readJson };
  assert.equal((await verifyAdjustmentFamilyGitRelease(request)).targetCommit, target);
  const check = responses.get(`actions/workflows/check.yml/runs?head_sha=${target}&event=push&per_page=100`);
  check.workflow_runs[0].repository.full_name = "anstosa/weather";
  await assert.rejects(verifyAdjustmentFamilyGitRelease(request), /identity differs/u);
  check.workflow_runs[0].repository.full_name = repository.full_name;
  check.workflow_runs[0].head_repository.fork = true;
  await assert.rejects(verifyAdjustmentFamilyGitRelease(request), /identity differs/u);
  check.workflow_runs[0].head_repository.fork = false;
  check.workflow_runs[0].head_repository.id += 1;
  await assert.rejects(verifyAdjustmentFamilyGitRelease(request), /identity differs/u);
  check.workflow_runs[0].head_repository.id = repository.id;
  check.workflow_runs.push({ ...run("check.yml", 3, "2027-01-10T08:12:00Z", "2027-01-10T08:21:00Z"), conclusion: "failure" });
  check.total_count = 2;
  await assert.rejects(verifyAdjustmentFamilyGitRelease(request), /not successful/u);
  check.workflow_runs.pop();
  await assert.rejects(verifyAdjustmentFamilyGitRelease(request), /incomplete/u);
  check.total_count = 1;
  const compare = responses.get(`compare/${"1".repeat(40)}...${target}?per_page=100`);
  compare.files.push({ filename: "apps/api/src/main.ts", status: "modified" });
  await assert.rejects(verifyAdjustmentFamilyGitRelease(request), /not closed/u);
});

// an inactive shadow must never overwrite the serving incumbent registry
test("root family diff refuses shadow activation and unrelated deployment changes", () => {
  const shadow = fixture({ actionKind: "shadow", policyDecision: "pending",
    reason: "development_candidate", fullMemberRootSha256: null });
  const additions = [{ filename: shadow.path, status: "added" }, {
    filename: `config/forecast-adjustments/ballydidean/temperature-canary-bundles/sha256-${shadow.action.candidateSha256}.json`,
    status: "added",
  }];
  assert.equal(validateAdjustmentFamilyChangedPaths(shadow.action, additions), true);
  assert.throws(() => validateAdjustmentFamilyChangedPaths(shadow.action, [...additions,
    { filename: "config/forecast-adjustments/ballydidean-temperature-canary.json", status: "modified" }]));
  assert.throws(() => validateAdjustmentFamilyChangedPaths(shadow.action, [...additions,
    { filename: "deploy/compose.yaml", status: "modified" }]));
  const promotion = fixture();
  assert.equal(validateAdjustmentFamilyChangedPaths(promotion.action, [
    { filename: promotion.path, status: "added" },
    { filename: "config/forecast-adjustments/ballydidean-temperature-canary.json", status: "modified" },
  ]), true);
  assert.throws(() => validateAdjustmentFamilyChangedPaths(promotion.action, [
    { filename: promotion.path, status: "added" },
    { filename: "config/forecast-adjustments/ballydidean-temperature-canary.json", status: "added" },
  ]));
  assert.throws(() => validateAdjustmentFamilyChangedPaths(promotion.action, [
    { filename: promotion.path, status: "added" },
    { filename: "config/forecast-adjustments/ballydidean-wind-canary.json", status: "modified" },
  ]));

  const rainShadow = fixture({ actionKind: "shadow", candidateSha256: "8".repeat(64),
    family: "rain", fullMemberRootSha256: null, policyDecision: "pending",
    reason: "development_candidate" });
  const rainArtifact =
    `config/forecast-adjustments/ballydidean/rain-runtime-artifacts/sha256-${"6".repeat(64)}.json`;
  const rainOrdinalArtifact =
    `config/forecast-adjustments/ballydidean/rain-runtime-artifacts/sha256-${"7".repeat(64)}.json`;
  const rainState =
    `config/forecast-adjustments/ballydidean/rain-maintenance-control-states/sha256-${"5".repeat(64)}.json`;
  const rainCommon = [
    { filename: rainShadow.path, status: "added" },
    { filename: rainArtifact, status: "added" },
    { filename: `config/forecast-adjustments/ballydidean/rain-model-packages/sha256-${rainShadow.action.candidateSha256}.json`, status: "added" },
    { filename: `config/forecast-adjustments/ballydidean/model-parity/rain/sha256-${rainShadow.action.candidateSha256}.json`, status: "added" },
    { filename: `config/forecast-adjustments/ballydidean/shadow-catalog/rain/sha256-${rainShadow.action.candidateSha256}.json`, status: "added" },
  ];
  assert.equal(validateAdjustmentFamilyChangedPaths(rainShadow.action, rainCommon), true);
  assert.equal(validateAdjustmentFamilyChangedPaths(rainShadow.action, [
    ...rainCommon,
    { filename: rainOrdinalArtifact, status: "added" },
    { filename: rainState, status: "added" },
  ]), true);
  assert.throws(() => validateAdjustmentFamilyChangedPaths(rainShadow.action, [
    ...rainCommon,
    { filename: rainOrdinalArtifact, status: "added" },
  ]));
  assert.throws(() => validateAdjustmentFamilyChangedPaths(rainShadow.action, [
    ...rainCommon,
    { filename: rainState, status: "added" },
    { filename: rainState.replace("5", "4"), status: "added" },
  ]));
});

// only complete and equal-or-more-restrictive persisted switches may reconcile
test("operator-off wins without deployment writing or enabling any setting", () => {
  const settings = { version: 1, temperature: true, wind: true, rain: false };
  assert.equal(reconcileAdjustmentFamilySettings(settings, settings, "temperature"), "active");
  assert.equal(reconcileAdjustmentFamilySettings(settings,
    { ...settings, temperature: false }, "temperature"), "deployed_operator_off");
  assert.throws(() => reconcileAdjustmentFamilySettings(settings,
    { ...settings, rain: true }, "temperature"), /less restrictive/u);
  assert.throws(() => reconcileAdjustmentFamilySettings(settings, null, "temperature"));
  assert.throws(() => reconcileAdjustmentFamilySettings(settings,
    { ...settings, temperature: false, approval: true }, "temperature"));
});

// exercise durable root ordering and categorical recovery instead of a rollback flag
test("family transaction preserves settings and records failure before family-only compensation", async () => {
  const { createHash } = await import("node:crypto");
  const { executeAdjustmentFamilyRelease } = await import("../scripts/adjustment-evaluation-package.mjs");
  const settingsBytes = Buffer.from('{"version":1,"temperature":true,"wind":true,"rain":false}\n');
  const settingsSha = createHash("sha256").update(settingsBytes).digest("hex");
  const built = fixture({ expectedSettingsSha256: settingsSha });
  const targetCommit = "2".repeat(40);
  const compensation = fixture({ actionKind: "compensate_incumbent", reason: "failed_promotion",
    predecessorActionSha256: built.actionSha256, expectedSourceCommit: targetCommit,
    expectedSourceRelease: "2027.01.10-1", expectedSettingsSha256: settingsSha });
  const input = { actionSha256: built.actionSha256, compensatingRelease: "2027.01.10-2",
    expectedCurrentRelease: built.action.expectedSourceRelease, expectedSettingsSha256: settingsSha,
    expectedSourceRelease: built.action.expectedSourceRelease, family: "temperature", fencingToken: "1",
    reportSha256: built.action.policyReportSha256, targetRelease: "2027.01.10-1" };
  const proof = { actionBytes: built.bytes, targetCommit,
    git: { sourceCommit: built.action.expectedSourceCommit, targetCommit, targetRelease: input.targetRelease },
    familyIdentitySha256: "5".repeat(64), imageDigestsSha256: "6".repeat(64),
    compensation: { actionBytes: compensation.bytes, actionSha256: compensation.actionSha256,
      git: { sourceCommit: targetCommit, targetRelease: input.compensatingRelease } },
    authority: { actionSha256: input.actionSha256, reportSha256: input.reportSha256,
      fullMemberRootSha256: built.action.fullMemberRootSha256, lifecycleLedgerRootSha256: built.action.lifecycleHeadSha256,
      sourceCommit: built.action.expectedSourceCommit, ctfState: "finalized" },
    capacity: { contractVersion: "adjustment-family-release-capacity/v1", state: "capacity_ready",
      actionSha256: input.actionSha256, family: input.family, sourceRelease: input.expectedSourceRelease,
      compensationScope: "family-only-new-release-compensation", measuredAt: "2027-01-10T09:00:00.000Z",
      retirementCreditBytes: 0, imageDigests: ["source", "target", "compensating"].flatMap(
        // bind all six literal image roles in the root transaction fixture
        (role, index) => ["server", "web"].map(
          (runtime, offset) => ({ role, runtime, digest: `sha256:${String(index * 2 + offset + 1).repeat(64)}` }),
        ),
      ) } };
  let record = null;
  const calls = [];
  let failApply = true;
  let failMark = false;
  let now = "2027-01-10T09:00:00.000Z";
  const ports = {
    // emulate the actual held root release lock without concurrent callbacks
    withReleaseLock: async (callback) => await callback(),
    clock: () => now,
    readPrepared: async () => record,
    inspectCurrent: async () => ({ settingsBytes, release: input.expectedSourceRelease,
      commit: built.action.expectedSourceCommit, maximumFence: "0", installedReceiptSha256: null }),
    verifyProof: async () => proof,
    prepare: async (next) => {
      calls.push("prepare");

      // a crash retry must reuse the first durable preparation clock
      if (record?.state === "prepared") assert.equal(next.createdAt, record.createdAt);
      record = next;
    },
    markApplying: async (next) => {
      calls.push("applying");

      // simulate a pre-mutation process crash after durable preparation
      if (failMark) throw new Error("applying checkpoint failed");
      record = { ...next, state: "applying" };
    },
    applyImages: async () => { calls.push("apply"); if (failApply) throw new Error("target health failed"); },
    recordFailure: async (next) => { calls.push("failure"); record = next; },
    verifyLive: async () => { calls.push("verify"); return { settingsBytes: Buffer.from(
      '{"version":1,"temperature":false,"wind":true,"rain":false}\n'), release: input.targetRelease,
      commit: targetCommit, family: input.family, familyIdentitySha256: proof.familyIdentitySha256,
      imageDigestsSha256: proof.imageDigestsSha256 }; },
    acknowledge: async (next) => { calls.push("ack"); record = next; return next; },
  };
  await assert.rejects(executeAdjustmentFamilyRelease(input, ports), /target health failed/u);
  assert.deepEqual(calls, ["prepare", "applying", "apply", "failure"]);
  assert.equal(record.state, "compensation_required");
  assert.equal((await executeAdjustmentFamilyRelease(input, ports)).state, "compensation_required");
  assert.deepEqual(calls, ["prepare", "applying", "apply", "failure"]);
  await assert.rejects(executeAdjustmentFamilyRelease({ ...input, fencingToken: "2" }, ports), /collision/u);
  record = null;
  failApply = false;
  failMark = true;
  calls.length = 0;
  await assert.rejects(executeAdjustmentFamilyRelease(input, ports), /checkpoint failed/u);
  assert.equal(record.state, "prepared");
  assert.equal(record.createdAt, "2027-01-10T09:00:00.000Z");
  now = "2027-01-10T09:05:00.000Z";
  failMark = false;
  const retried = await executeAdjustmentFamilyRelease(input, ports);
  assert.equal(retried.state, "acknowledged");
  assert.equal(retried.createdAt, "2027-01-10T09:00:00.000Z");
  assert.deepEqual(calls, ["prepare", "applying", "prepare", "applying", "apply", "verify", "ack"]);
  record = null;
  now = "2027-01-10T09:00:00.000Z";
  calls.length = 0;
  const result = await executeAdjustmentFamilyRelease(input, ports);
  assert.equal(result.outcome, "deployed_operator_off");
  assert.deepEqual(calls, ["prepare", "applying", "apply", "verify", "ack"]);
  assert.equal((await executeAdjustmentFamilyRelease(input, ports)).state, "acknowledged");
  // future-only F carries the policy report under its own additive field name
  proof.authority = { ...proof.authority,
    contractVersion: "adjustment-maintenance-anchor/v3", policyReportSha256: input.reportSha256 };
  delete proof.authority.reportSha256;
  record = null;
  assert.equal((await executeAdjustmentFamilyRelease(input, ports)).state, "acknowledged");
  proof.authority.policyReportSha256 = "7".repeat(64);
  record = null;
  await assert.rejects(executeAdjustmentFamilyRelease(input, ports), /anchor authority/u);
  proof.authority.policyReportSha256 = input.reportSha256;
  proof.authority.ctfState = "transferred";
  record = null;
  calls.length = 0;
  await assert.rejects(executeAdjustmentFamilyRelease(input, ports), /anchor authority/u);
  assert.deepEqual(calls, []);
});

// bootstrap is exact CI-backed code delivery rather than a model qualification action
test("inactive v14 Git proof binds the actual v13 predecessor and refuses stale CI", async () => {
  const source = "56b327d9c750946f6f6963b6fe1fa5c9bba791ca";
  const target = "f6daa89d135661eb60dd5479c5d0d6c23e7198a9";
  const repository = {
    fork: false,
    full_name: "anstosa/Ballydidean-Weather",
    id: 1_342_404_160,
  };
  // construct exact successful same-repository workflow metadata
  const run = (filename, id, created_at, updated_at) => ({ id, run_attempt: 1, created_at, updated_at,
    head_sha: target, event: "push", repository: { ...repository }, head_repository: { ...repository },
    path: `.github/workflows/${filename}`, status: "completed", conclusion: "success" });
  const responses = new Map([
    ["git/ref/tags/2026.10.09-1", { object: { type: "commit", sha: source } }],
    ["git/ref/tags/2026.10.09-2", { object: { type: "commit", sha: target } }],
    [`compare/${source}...${target}?per_page=100`, {
      ahead_by: 1, base_commit: { sha: source }, behind_by: 0,
      commits: [{ sha: target, parents: [{ sha: source }] }], status: "ahead", total_commits: 1,
    }],
    [`actions/workflows/check.yml/runs?head_sha=${target}&event=push&per_page=100`, {
      total_count: 1, workflow_runs: [run("check.yml", 1, "2026-10-09T08:00:00Z", "2026-10-09T08:10:00Z")] }],
    [`actions/workflows/publish-images.yml/runs?head_sha=${target}&event=push&per_page=100`, {
      total_count: 1, workflow_runs: [run("publish-images.yml", 2, "2026-10-09T08:11:00Z", "2026-10-09T08:20:00Z")] }],
  ]);
  // return only the immutable fixture requested by the public proof
  const readJson = async (path) => structuredClone(responses.get(path));
  assert.equal((await verifyAdjustmentInertV14GitRelease("2026.10.09-2", readJson)).targetCommit, target);
  const ancestry = responses.get(`compare/${source}...${target}?per_page=100`);
  ancestry.commits[0].parents[0].sha = "f".repeat(40);
  await assert.rejects(verifyAdjustmentInertV14GitRelease("2026.10.09-2", readJson), /ancestry/u);
  ancestry.commits[0].parents[0].sha = source;
  await assert.rejects(verifyAdjustmentInertV14GitRelease("2026.10.09-1", readJson));
  responses.get("git/ref/tags/2026.10.09-1").object.sha = "3".repeat(40);
  await assert.rejects(verifyAdjustmentInertV14GitRelease("2026.10.09-2", readJson), /source differs/u);
  responses.get("git/ref/tags/2026.10.09-1").object.sha = source;
  const check = responses.get(`actions/workflows/check.yml/runs?head_sha=${target}&event=push&per_page=100`);
  check.workflow_runs[0].updated_at = "2026-10-09T08:12:00Z";
  await assert.rejects(verifyAdjustmentInertV14GitRelease("2026.10.09-2", readJson), /preceded/u);
  check.workflow_runs[0].updated_at = "2026-10-09T08:10:00Z";
  check.workflow_runs[0].conclusion = "failure";
  await assert.rejects(verifyAdjustmentInertV14GitRelease("2026.10.09-2", readJson), /not successful/u);
});

// full v14 must be the published direct child of the compatibility bridge
test("full v14 Git proof binds the exact bridge predecessor", async () => {
  const source = "f6daa89d135661eb60dd5479c5d0d6c23e7198a9";
  const target = "4".repeat(40);
  const repository = {
    fork: false,
    full_name: "anstosa/Ballydidean-Weather",
    id: 1_342_404_160,
  };
  // construct complete exact-commit workflow results
  const run = (filename, id, created_at, updated_at) => ({ id, run_attempt: 1, created_at, updated_at,
    head_sha: target, event: "push", repository: { ...repository }, head_repository: { ...repository },
    path: `.github/workflows/${filename}`, status: "completed", conclusion: "success" });
  const responses = new Map([
    ["git/ref/tags/2026.10.09-2", { object: { type: "commit", sha: source } }],
    ["git/ref/tags/2026.10.09-3", { object: { type: "commit", sha: target } }],
    [`compare/${source}...${target}?per_page=100`, {
      ahead_by: 1, base_commit: { sha: source }, behind_by: 0,
      commits: [{ sha: target, parents: [{ sha: source }] }], status: "ahead", total_commits: 1,
    }],
    [`actions/workflows/check.yml/runs?head_sha=${target}&event=push&per_page=100`, {
      total_count: 1, workflow_runs: [run("check.yml", 3, "2026-10-09T09:00:00Z", "2026-10-09T09:10:00Z")],
    }],
    [`actions/workflows/publish-images.yml/runs?head_sha=${target}&event=push&per_page=100`, {
      total_count: 1, workflow_runs: [run("publish-images.yml", 4, "2026-10-09T09:11:00Z", "2026-10-09T09:20:00Z")],
    }],
  ]);
  // expose only the requested immutable public document
  const readJson = async (path) => structuredClone(responses.get(path));
  assert.equal((await verifyAdjustmentFullV14GitRelease("2026.10.09-3", readJson)).sourceCommit, source);
  await assert.rejects(verifyAdjustmentFullV14GitRelease("2026.10.09-2", readJson), /target differs/u);
  responses.get("git/ref/tags/2026.10.09-2").object.sha = "5".repeat(40);
  await assert.rejects(verifyAdjustmentFullV14GitRelease("2026.10.09-3", readJson), /source differs/u);
});
