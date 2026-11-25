import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  buildInstalledAdjustmentRainControlReferenceCatalog,
  buildInstalledAdjustmentTargetCatalog,
} from "../../../../deploy/scripts/adjustment-evaluation-package.mjs";

const installedAt = "2026-11-24T12:34:56.789Z";
const createdAt = "2026-11-24T00:00:00.000Z";
const reportCreatedAt = "2026-11-23T00:00:00.000Z";
const validThrough = "2026-11-30T00:00:00.000Z";
const imageCommit = "a".repeat(40);
const hashRecords = new Map();
const documents = {};
const observedCases = [];
const memberRows = [];

// preserve array order and recursively sort object keys
function canonical(value) {
  // retain exact array order
  if (Array.isArray(value)) return value.map(canonical);
  // use production javascript key ordering
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, canonical(value[key])]),
    );
  }
  return value;
}

// encode canonical json without its record delimiter
function canonicalText(value) {
  return JSON.stringify(canonical(value));
}

// encode one canonical member with its single lf delimiter
function canonicalBytes(value) {
  return Buffer.from(`${canonicalText(value)}\n`, "utf8");
}

// hash and retain every original byte preimage
function sha256(value) {
  const bytes = Buffer.from(value);
  const digest = createHash("sha256").update(bytes).digest("hex");
  hashRecords.set(bytes.toString("hex"), digest);
  return digest;
}

// derive a stable distinct fixture identity
function identity(label) {
  return sha256(Buffer.from(`family-target-catalog-node-oracle/${label}`, "utf8"));
}

// retain one named canonical document
function document(name, value) {
  documents[name] = canonical(value);
  return documents[name];
}

const settings = document("settings", {
  rain: { enabled: true },
  temperature: { enabled: true },
  wind: { enabled: true },
});
const settingsBytes = canonicalBytes(settings);
const settingsSha256 = sha256(settingsBytes);
const environment = document("environment", {
  WEATHER_CONTROL_PLANE_SHA256: identity("environment/control-plane"),
  WEATHER_RELEASE: "2026.11.24-1",
  WEATHER_SERVER_IMAGE: `ghcr.io/anstosa/weather-server@sha256:${"b".repeat(64)}`,
});

// construct one complete model action document
function modelAction(label, actionKind, family, overrides = {}) {
  const rules = {
    promote: ["qualified", "qualified_candidate", true],
    raw: ["regressed", "policy_raw", false],
    rollback_prior: ["regressed", "qualified_prior", true],
    shadow: ["pending", "development_candidate", true],
  };
  const [policyDecision, reason, hasCandidate] = rules[actionKind];
  const action = {
    actionKind,
    candidateGraphSha256: hasCandidate ? identity(`${label}/candidate-graph`) : null,
    candidateSha256: hasCandidate ? identity(`${label}/candidate`) : null,
    contractVersion: "forecast-adjustment-model-action/v1",
    createdAt,
    expectedInstalledReceiptSha256: null,
    expectedSettingsSha256: settingsSha256,
    expectedSourceCommit: "c".repeat(40),
    expectedSourceRelease: "2026.11.23-1",
    family,
    fencingToken: "17",
    fullMemberRootSha256: actionKind === "shadow" ? null : identity(`${label}/member-root`),
    lifecycleHeadSha256: identity(`${label}/lifecycle-head`),
    policyDecision,
    policyReportSha256: identity(`${label}/policy-report`),
    predecessorActionSha256: null,
    reason,
    reportCreatedAt,
    siteKey: "ballydidean",
    validThrough,
    ...overrides,
  };
  return document(label, action);
}

// construct one installed model slot for source-catalog fixtures
function modelEntry(label, family, slot, overrides = {}) {
  const artifactSha256 = identity(`${label}/artifact`);
  const candidateSha256 = identity(`${label}/candidate`);
  const receipt = {
    actionKind: slot === "active" ? "promote" : "shadow",
    actionSha256: identity(`${label}/action`),
    bundleSha256: artifactSha256,
    candidateGraphSha256: identity(`${label}/candidate-graph`),
    candidateSha256,
    contractVersion: "adjustment-installed-candidate-receipt/v2",
    controlPlaneSha256: environment.WEATHER_CONTROL_PLANE_SHA256,
    deployedCommit: "d".repeat(40),
    deployedImageDigest: `sha256:${"e".repeat(64)}`,
    deployedRelease: "2026.11.22-1",
    deployedSettingsSha256: settingsSha256,
    fencingToken: "16",
    fullMemberRootSha256: slot === "active" ? identity(`${label}/member-root`) : null,
    installedAt: "2026-11-22T12:34:56.789Z",
    lifecycleHeadSha256: identity(`${label}/lifecycle-head`),
    policyDecision: slot === "active" ? "qualified" : "pending",
    policyReportSha256: identity(`${label}/policy-report`),
    registrationSha256: identity(`${label}/registration`),
    sourceSha256: identity(`${label}/source`),
  };
  const registration = {
    artifactSha256,
    candidateSha256,
    registrationSha256: receipt.registrationSha256,
    sourceSha256: receipt.sourceSha256,
  };
  return {
    family,
    receipt,
    registration,
    slot,
    ...overrides,
  };
}

// construct one legacy v1 shadow entry without a slot
function legacyEntry(label, family) {
  const { slot: _slot, ...entry } = modelEntry(label, family, "shadow");
  return entry;
}

// construct one fully valid installed control receipt
function controlReceipt(label, overrides = {}) {
  return {
    actionKind: "control_reference",
    actionSha256: identity(`${label}/action`),
    contractVersion: "adjustment-installed-rain-control-reference-receipt/v1",
    controlPlaneSha256: environment.WEATHER_CONTROL_PLANE_SHA256,
    controlStateSha256: identity(`${label}/control-state`),
    custodyAnchorSha256: identity(`${label}/custody-anchor`),
    deployedCommit: "f".repeat(40),
    deployedImageDigest: `sha256:${"1".repeat(64)}`,
    deployedRelease: "2026.11.21-1",
    deployedSettingsSha256: settingsSha256,
    dueMonth: "2026-12",
    fencingToken: "15",
    graphManifestSha256: identity(`${label}/graph-manifest`),
    installedAt: "2026-11-21T12:34:56.789Z",
    ordinalArtifactSha256: identity(`${label}/ordinal-artifact`),
    sourceMemberRootSha256: identity(`${label}/source-member-root`),
    sourceReceiptRootSha256: identity(`${label}/source-receipt-root`),
    ...overrides,
  };
}

// construct one complete control-reference action
function rainAction(label, expectedCatalogReceiptSha256, controlStateSha256,
  ordinalArtifactSha256, overrides = {}) {
  return document(label, {
    actionKind: "control_reference",
    contractVersion: "forecast-adjustment-rain-control-reference-action/v1",
    controlStateSha256,
    createdAt,
    dueMonth: "2026-12",
    expectedCatalogReceiptSha256,
    expectedSettingsSha256: settingsSha256,
    expectedSourceCommit: "c".repeat(40),
    expectedSourceRelease: "2026.11.23-1",
    family: "rain",
    fencingToken: "17",
    graphManifestSha256: identity(`${label}/graph-manifest`),
    ordinalArtifactSha256,
    predecessorActionSha256: null,
    reason: "premonth_reference",
    sourceMemberRootSha256: identity(`${label}/source-member-root`),
    sourceReceiptRootSha256: identity(`${label}/source-receipt-root`),
    validThrough: "2026-12-01T00:00:00.000Z",
    ...overrides,
  });
}

// construct the exact target-image shadow projection
function shadowProjection(label, action, actionSha256) {
  const artifactSha256 = identity(`${label}/artifact`);
  return document(label, {
    actionSha256,
    artifactSha256,
    bundleSha256: artifactSha256,
    candidateGraphSha256: action.candidateGraphSha256,
    candidateSha256: action.candidateSha256,
    contractVersion: "forecast-adjustment-shadow-catalog-projection/v1",
    family: action.family,
    paritySha256: identity(`${label}/parity`),
    registration: {
      artifactSha256,
      candidateSha256: action.candidateSha256,
      registrationSha256: identity(`${label}/registration`),
      sourceSha256: identity(`${label}/source`),
    },
    siteKey: "ballydidean",
  });
}

// construct the exact target-image rain selector
function rainSelector(label, action, actionSha256, overrides = {}) {
  return document(label, {
    actionSha256,
    contractVersion: "forecast-adjustment-rain-control-reference-registry/v1",
    controlStatePath:
      `rain-maintenance-control-states/sha256-${action.controlStateSha256}.json`,
    controlStateSha256: action.controlStateSha256,
    dueMonth: action.dueMonth,
    ordinalArtifactPath:
      `rain-runtime-artifacts/sha256-${action.ordinalArtifactSha256}.json`,
    ordinalArtifactSha256: action.ordinalArtifactSha256,
    siteKey: "ballydidean",
    ...overrides,
  });
}

// expose every decoded fixture node through the closed protocol
function emitNode(name, path, value) {
  // recurse over ordered arrays
  if (Array.isArray(value)) {
    console.log(["node", name, path, "array", "-"].join("\t"));
    value.forEach((child, index) => emitNode(name, `${path}/${index}`, child));
    return;
  }
  // recurse over canonical object members
  if (value !== null && typeof value === "object") {
    console.log(["node", name, path, "object", "-"].join("\t"));
    // visit the exact canonical key order
    for (const [key, child] of Object.entries(canonical(value))) {
      emitNode(name, `${path}/${key}`, child);
    }
    return;
  }
  console.log(["node", name, path, value === null ? "null" : typeof value,
    value === null ? "null" : String(value)].join("\t"));
}

// provide one synchronous bounded in-memory image reader
function imageReader(caseName, members) {
  return (path, maximumBytes) => {
    const documentName = members.get(path) ?? "-";
    memberRows.push(["member", caseName, path, String(maximumBytes), documentName]);
    // preserve ordinary missing-member refusal
    if (documentName === "-") throw new Error("fixture image member is unavailable");
    const bytes = canonicalBytes(documents[documentName]);
    assert.ok(bytes.length <= maximumBytes, `${caseName}: ${path}`);
    return bytes;
  };
}

// execute one actual builder case and retain its oracle rows
async function observe(fixture) {
  const action = documents[fixture.actionDoc];
  const source = documents[fixture.sourceDoc];
  const actionSha256 = fixture.actionSha256 ?? sha256(canonicalBytes(action));
  const readsBefore = memberRows.length;
  let clockCalls = 0;
  let result;
  let failure;
  const input = {
    action,
    actionSha256,
    clock: () => {
      // the original clock is sampled only after all selected image reads
      assert.equal(memberRows.length - readsBefore, fixture.expectedReads, fixture.name);
      clockCalls += 1;
      return installedAt;
    },
    environment,
    imageCommit,
    imageRead: imageReader(fixture.name, fixture.members ?? new Map()),
    settingsBytes,
    source,
  };
  // include only the control builder's custody identity
  if (fixture.custodySha256 !== undefined) input.custodyAnchorSha256 = fixture.custodySha256;
  try {
    result = fixture.kind === "rain"
      ? buildInstalledAdjustmentRainControlReferenceCatalog(input)
      : await buildInstalledAdjustmentTargetCatalog({ ...input, family: fixture.family });
  } catch (error) {
    failure = error;
  }
  // validate exact refusal without inventing an output
  if (fixture.error !== undefined) {
    assert.equal(result, undefined, fixture.name);
    assert.equal(failure?.message, fixture.error, fixture.name);
    assert.equal(clockCalls, fixture.clockCalls ?? 0, fixture.name);
  } else {
    assert.equal(failure, undefined, `${fixture.name}: ${failure}`);
    assert.equal(clockCalls, fixture.clockCalls ?? 1, fixture.name);
    document(fixture.expectedDoc, result);
  }
  assert.equal(memberRows.length - readsBefore, fixture.expectedReads, fixture.name);
  observedCases.push([
    "case", fixture.name, fixture.actionDoc, actionSha256, fixture.family,
    fixture.sourceDoc, "environment", imageCommit, "settings", installedAt,
    fixture.custodySha256 ?? "-", fixture.error === undefined ? fixture.expectedDoc : "error",
    String(clockCalls),
  ]);
}

const activeTemperature = modelEntry("active-temperature", "temperature", "active");
const pendingWind = modelEntry("pending-wind", "wind", "shadow");
const priorControl = controlReceipt("prior-control");
const priorControlSha256 = sha256(canonicalBytes(priorControl));
const controlEntry = { family: "rain", receipt: priorControl, registration: null,
  slot: "control" };

const sourceShadowActive = document("source_shadow_active", {
  contractVersion: "adjustment-installed-candidate-catalog/v2",
  entries: [activeTemperature],
});
const sourcePromote = document("source_promote", {
  contractVersion: "adjustment-installed-candidate-catalog/v2",
  entries: [modelEntry("promote-shadow", "wind", "shadow")],
});
const sourceRawShadow = document("source_raw_shadow", {
  contractVersion: "adjustment-installed-candidate-catalog/v2",
  entries: [pendingWind],
});
const sourceLegacy = document("source_legacy", {
  contractVersion: "adjustment-installed-candidate-catalog/v1",
  entries: [legacyEntry("legacy-wind", "wind")],
});
const sourceWithControl = document("source_with_control", {
  contractVersion: "adjustment-installed-candidate-catalog/v3",
  entries: [controlEntry, activeTemperature],
});
const sourceEmpty = document("source_empty", {
  contractVersion: "adjustment-installed-candidate-catalog/v2",
  entries: [],
});
const sourceRainReplacement = document("source_rain_replacement", {
  contractVersion: "adjustment-installed-candidate-catalog/v3",
  entries: [activeTemperature, controlEntry],
});

const shadowAction = modelAction("action_shadow_preserve_active", "shadow", "temperature");
const shadowActionSha256 = sha256(canonicalBytes(shadowAction));
shadowProjection("member_shadow_preserve_active", shadowAction, shadowActionSha256);

const promoteSourceEntry = sourcePromote.entries[0];
const promoteAction = modelAction("action_promote", "promote", "wind", {
  candidateGraphSha256: promoteSourceEntry.receipt.candidateGraphSha256,
  candidateSha256: promoteSourceEntry.receipt.candidateSha256,
  expectedInstalledReceiptSha256: sha256(canonicalBytes(promoteSourceEntry.receipt)),
});
const rawAction = modelAction("action_raw", "raw", "wind");
const legacyAction = modelAction("action_legacy_raw", "raw", "rain");
const controlShadowAction = modelAction("action_shadow_preserve_control", "shadow", "temperature");
const controlShadowActionSha256 = sha256(canonicalBytes(controlShadowAction));
shadowProjection("member_shadow_preserve_control", controlShadowAction,
  controlShadowActionSha256);
const rollbackAction = modelAction("action_rollback", "rollback_prior", "rain");

const rainState = document("member_rain_state", {
  contractVersion: "rain-maintenance-control-state/v1",
  dueMonth: "2026-12",
  ordinal: "31",
});
const rainArtifact = document("member_rain_artifact", {
  contractVersion: "rain-runtime-artifact/v1",
  dueMonth: "2026-12",
  values: [0.1, 0.2, 0.3],
});
const rainStateSha256 = sha256(canonicalBytes(rainState));
const rainArtifactSha256 = sha256(canonicalBytes(rainArtifact));

// create one self-consistent rain action and selector family per case
function prepareRain(label, expectedReceiptSha256, selectorOverrides = {}, actionOverrides = {}) {
  const action = rainAction(`action_${label}`, expectedReceiptSha256, rainStateSha256,
    rainArtifactSha256, actionOverrides);
  const actionSha256 = sha256(canonicalBytes(action));
  rainSelector(`member_${label}_selector`, action, actionSha256, selectorOverrides);
  return { action, actionSha256 };
}

const rainInitial = prepareRain("rain_initial", null);
const rainLegacy = prepareRain("rain_legacy_upgrade_control", null);
const rainReplacement = prepareRain("rain_replacement", priorControlSha256);
const rainWrongBaseline = prepareRain("rain_wrong_baseline", identity("wrong-prior-receipt"));
const rainSelectorMismatch = prepareRain("rain_selector_mismatch", null, {
  dueMonth: "2027-01",
});
const mismatchedState = document("member_rain_state_mismatch", {
  contractVersion: "rain-maintenance-control-state/v1",
  dueMonth: "2026-12",
  ordinal: "32",
});
const rainStateMismatch = prepareRain("rain_state_mismatch", null);

const shadowPath = `config/forecast-adjustments/ballydidean/shadow-catalog/temperature/sha256-${shadowAction.candidateSha256}.json`;
const controlShadowPath = `config/forecast-adjustments/ballydidean/shadow-catalog/temperature/sha256-${controlShadowAction.candidateSha256}.json`;
const selectorPath = "config/forecast-adjustments/ballydidean-rain-control-reference.json";
const statePath = `config/forecast-adjustments/ballydidean/rain-maintenance-control-states/sha256-${rainStateSha256}.json`;
const artifactPath = `config/forecast-adjustments/ballydidean/rain-runtime-artifacts/sha256-${rainArtifactSha256}.json`;

const fixtures = [
  {
    actionDoc: "action_shadow_preserve_active", actionSha256: shadowActionSha256,
    expectedDoc: "expected_shadow_preserve_active", expectedReads: 1,
    family: "temperature", kind: "target", name: "shadow_preserve_active",
    sourceDoc: "source_shadow_active", members: new Map([
      [shadowPath, "member_shadow_preserve_active"],
    ]),
  },
  {
    actionDoc: "action_promote", expectedDoc: "expected_promote", expectedReads: 0,
    family: "wind", kind: "target", name: "promote_shadow",
    sourceDoc: "source_promote",
  },
  {
    actionDoc: "action_raw", clockCalls: 0, expectedDoc: "expected_raw_retain_shadow",
    expectedReads: 0, family: "wind", kind: "target", name: "raw_retain_shadow",
    sourceDoc: "source_raw_shadow",
  },
  {
    actionDoc: "action_legacy_raw", clockCalls: 0,
    expectedDoc: "expected_legacy_shadow_upgrade", expectedReads: 0, family: "rain",
    kind: "target", name: "legacy_shadow_upgrade", sourceDoc: "source_legacy",
  },
  {
    actionDoc: "action_shadow_preserve_control", actionSha256: controlShadowActionSha256,
    expectedDoc: "expected_target_preserve_control", expectedReads: 1,
    family: "temperature", kind: "target", name: "target_preserve_control",
    sourceDoc: "source_with_control", members: new Map([
      [controlShadowPath, "member_shadow_preserve_control"],
    ]),
  },
  {
    actionDoc: "action_rollback", clockCalls: 0,
    error: "prior qualified installed receipt is unavailable", expectedReads: 0,
    family: "rain", kind: "target", name: "rollback_refusal", sourceDoc: "source_empty",
  },
  {
    actionDoc: "action_rain_initial", actionSha256: rainInitial.actionSha256,
    custodySha256: identity("rain-initial/custody"), expectedDoc: "expected_rain_initial",
    expectedReads: 3, family: "rain", kind: "rain", name: "rain_initial",
    sourceDoc: "source_empty", members: new Map([
      [selectorPath, "member_rain_initial_selector"],
      [statePath, "member_rain_state"],
      [artifactPath, "member_rain_artifact"],
    ]),
  },
  {
    actionDoc: "action_rain_legacy_upgrade_control", actionSha256: rainLegacy.actionSha256,
    custodySha256: identity("rain-legacy/custody"),
    expectedDoc: "expected_rain_legacy_upgrade_control", expectedReads: 3,
    family: "rain", kind: "rain", name: "rain_legacy_upgrade_control",
    sourceDoc: "source_legacy", members: new Map([
      [selectorPath, "member_rain_legacy_upgrade_control_selector"],
      [statePath, "member_rain_state"],
      [artifactPath, "member_rain_artifact"],
    ]),
  },
  {
    actionDoc: "action_rain_replacement", actionSha256: rainReplacement.actionSha256,
    custodySha256: identity("rain-replacement/custody"),
    expectedDoc: "expected_rain_replacement", expectedReads: 3,
    family: "rain", kind: "rain", name: "rain_replacement_baseline",
    sourceDoc: "source_rain_replacement", members: new Map([
      [selectorPath, "member_rain_replacement_selector"],
      [statePath, "member_rain_state"],
      [artifactPath, "member_rain_artifact"],
    ]),
  },
  {
    actionDoc: "action_rain_wrong_baseline", actionSha256: rainWrongBaseline.actionSha256,
    clockCalls: 0, custodySha256: identity("rain-wrong-baseline/custody"),
    error: "installed rain control catalog baseline differs", expectedReads: 0,
    family: "rain", kind: "rain", name: "rain_replacement_baseline_refusal",
    sourceDoc: "source_rain_replacement",
  },
  {
    actionDoc: "action_rain_selector_mismatch",
    actionSha256: rainSelectorMismatch.actionSha256, clockCalls: 0,
    custodySha256: identity("rain-selector-mismatch/custody"),
    error: "installed rain control selector differs", expectedReads: 1,
    family: "rain", kind: "rain", name: "rain_selector_mismatch",
    sourceDoc: "source_empty", members: new Map([
      [selectorPath, "member_rain_selector_mismatch_selector"],
    ]),
  },
  {
    actionDoc: "action_rain_state_mismatch", actionSha256: rainStateMismatch.actionSha256,
    clockCalls: 0, custodySha256: identity("rain-state-mismatch/custody"),
    error: "installed rain control selector differs", expectedReads: 2,
    family: "rain", kind: "rain", name: "rain_state_digest_mismatch",
    sourceDoc: "source_empty", members: new Map([
      [selectorPath, "member_rain_state_mismatch_selector"],
      [statePath, "member_rain_state_mismatch"],
    ]),
  },
];

// execute fixtures sequentially to retain read order
for (const fixture of fixtures) await observe(fixture);

// emit every canonical fixture document and typed node
for (const [name, value] of Object.entries(documents)) {
  console.log(["doc", name, canonicalText(value)].join("\t"));
  emitNode(name, "$", value);
  sha256(canonicalBytes(value));
}
// emit deduplicated original hash preimages
for (const [preimage, digest] of hashRecords) {
  console.log(["hash", preimage, digest].join("\t"));
}
// emit every deterministic fixture clock
for (const clock of [createdAt, reportCreatedAt, validThrough, installedAt,
  "2026-12-01T00:00:00.000Z", "2026-11-21T12:34:56.789Z", "2026-11-22T12:34:56.789Z"]) {
  console.log(["clock", clock, String(Date.parse(clock))].join("\t"));
}
// emit the fixed thirteen-field case protocol including original clock call count
for (const row of observedCases) console.log(row.join("\t"));
// emit only actual ordered image reads
for (const row of memberRows) console.log(row.join("\t"));
