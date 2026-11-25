import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  authorizeAdjustmentFamilyAction,
  buildInstalledAdjustmentRainControlReferenceCatalog,
  buildInstalledAdjustmentTargetCatalog,
  validateAdjustmentDevelopmentActionBinding,
  validateAdjustmentRainControlReferenceBinding,
} from "../../../../deploy/scripts/adjustment-evaluation-package.mjs";
import { AdjustmentDevelopmentCustodyAnchorStore } from
  "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const installedAt = "2026-11-24T12:34:56.789Z";
const createdAt = "2026-11-24T00:00:00.000Z";
const reportCreatedAt = "2026-11-23T00:00:00.000Z";
const validThrough = "2026-11-30T00:00:00.000Z";
const imageCommit = "a".repeat(40);
const controlSha256 =
  "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb";
const hashRecords = new Map();
const documents = {};
const memberRows = [];
const caseRows = [];
const materialRows = [];

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

// retain every original sha256 preimage
function sha256(value) {
  const bytes = Buffer.from(value);
  const digest = createHash("sha256").update(bytes).digest("hex");
  hashRecords.set(bytes.toString("hex"), digest);
  return digest;
}

// derive one stable distinct fixture identity
function identity(label) {
  return sha256(Buffer.from(`family-target-preflight-node-oracle/${label}`, "utf8"));
}

// retain one named canonical document
function document(name, value) {
  documents[name] = canonical(value);
  return documents[name];
}

// construct one valid capture epoch witness with its canonical self address
function captureEpochWitness() {
  const unsigned = {
    activationKind: "inert_v14_pre_activation",
    archiveCommitOrdinal: "0",
    catalogFrontierSha256:
      "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af",
    contractVersion: "adjustment-revision-capture-epoch-witness/v1",
    controlPlaneSha256: controlSha256,
    controlPlaneVersion: "17",
    databaseMigrationHistorySha256:
      "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf",
    epochAt: "2026-10-08T00:35:00.000Z",
    servingSnapshotSha256: identity("witness/serving-snapshot"),
    sourceCommit: "b".repeat(40),
    sourceRelease: "2026.10.08-1",
    sourceServerImageDigest: `sha256:${"c".repeat(64)}`,
    sourceWebImageDigest: `sha256:${"d".repeat(64)}`,
  };
  return { ...unsigned, witnessSha256: sha256(canonicalBytes(unsigned)) };
}

const witness = document("witness", captureEpochWitness());
const witnessDocumentSha256 = sha256(canonicalBytes(witness));
const settings = document("settings", {
  rain: { enabled: true },
  temperature: { enabled: true },
});
const settingsBytes = canonicalBytes(settings);
const settingsSha256 = sha256(settingsBytes);
const environment = document("environment", {
  WEATHER_CONTROL_PLANE_SHA256: identity("environment/control-plane"),
  WEATHER_RELEASE: "2026.11.24-1",
  WEATHER_SERVER_IMAGE: `ghcr.io/anstosa/weather-server@sha256:${"e".repeat(64)}`,
});
const targetEnvironment = document("release_environment_target", {
  ...environment,
  WEATHER_WEB_IMAGE: `ghcr.io/anstosa/weather-web@sha256:${"1".repeat(64)}`,
});
const compensationEnvironment = document("release_environment_compensation", {
  WEATHER_CONTROL_PLANE_SHA256: identity("compensation/control-plane"),
  WEATHER_RELEASE: "2026.11.23-1",
  WEATHER_SERVER_IMAGE: `ghcr.io/anstosa/weather-server@sha256:${"2".repeat(64)}`,
  WEATHER_WEB_IMAGE: `ghcr.io/anstosa/weather-web@sha256:${"3".repeat(64)}`,
});
const sourceRegistry = document("release_registry_source_temperature", {
  active: "baseline-temperature",
});
const targetRegistry = document("release_registry_target_temperature", {
  active: "candidate-temperature",
});
const source = document("source_empty", {
  contractVersion: "adjustment-installed-candidate-catalog/v2",
  entries: [],
});

// construct one complete pending development action
function developmentAction(label) {
  return document(`action_${label}`, {
    actionKind: "shadow",
    candidateGraphSha256: identity(`${label}/candidate-graph`),
    candidateSha256: identity(`${label}/candidate`),
    contractVersion: "forecast-adjustment-model-action/v1",
    createdAt,
    expectedInstalledReceiptSha256: null,
    expectedSettingsSha256: settingsSha256,
    expectedSourceCommit: "f".repeat(40),
    expectedSourceRelease: "2026.11.23-1",
    family: "temperature",
    fencingToken: "17",
    fullMemberRootSha256: null,
    lifecycleHeadSha256: identity(`${label}/lifecycle`),
    policyDecision: "pending",
    policyReportSha256: identity(`${label}/policy`),
    predecessorActionSha256: null,
    reason: "development_candidate",
    reportCreatedAt,
    siteKey: "ballydidean",
    validThrough,
  });
}

// construct one complete development custody authority anchor
function developmentAnchor(label, action, actionSha256) {
  return document(`anchor_${label}`, {
    actionKind: "shadow",
    actionSha256,
    archiveCommitOrdinal: "1",
    artifactSha256: identity(`${label}/artifact`),
    candidateGraphSha256: action.candidateGraphSha256,
    candidateSha256: action.candidateSha256,
    captureEpochWitnessSha256: witnessDocumentSha256,
    contractVersion: "adjustment-development-custody-anchor/v1",
    controlSha256,
    controlVersion: "17",
    custodyAcknowledgedAt: "2026-10-08T00:35:00.000Z",
    custodyAcknowledgementSha256: identity(`${label}/custody-acknowledgement`),
    custodyCheckpointSha256: identity(`${label}/custody-checkpoint`),
    developmentGraphSha256: identity(`${label}/development-graph`),
    dueKey: "monthly/temperature/2026-11",
    family: action.family,
    frontierSha256: identity(`${label}/frontier`),
    fullGraphVerifiedAt: "2026-10-08T00:36:00.000Z",
    inputHeadSha256: identity(`${label}/input-head`),
    lifecycleLedgerRootSha256: action.lifecycleHeadSha256,
    memberRootSha256: identity(`${label}/member-root`),
    pageSha256: identity(`${label}/page`),
    policyReportSha256: action.policyReportSha256,
    predecessorAnchorSha256: null,
    registrationSha256: identity(`${label}/registration`),
    sequence: "0",
    sourceCommit: action.expectedSourceCommit,
    sourceSha256: identity(`${label}/source`),
    startMemberSha256: null,
    startSha256: identity(`${label}/start`),
  });
}

// construct one target-image shadow projection
function shadowProjection(label, action, actionSha256, anchor, overrides = {}) {
  return document(`member_${label}_projection`, {
    actionSha256,
    artifactSha256: anchor.artifactSha256,
    bundleSha256: anchor.artifactSha256,
    candidateGraphSha256: action.candidateGraphSha256,
    candidateSha256: action.candidateSha256,
    contractVersion: "forecast-adjustment-shadow-catalog-projection/v1",
    family: action.family,
    paritySha256: identity(`${label}/parity`),
    registration: {
      artifactSha256: anchor.artifactSha256,
      candidateSha256: action.candidateSha256,
      epochWitnessSha256: witness.witnessSha256,
      registrationSha256: anchor.registrationSha256,
      sourceSha256: anchor.sourceSha256,
      ...overrides,
    },
    siteKey: "ballydidean",
  });
}

// construct one complete rain control-reference action
function rainAction(label, overrides = {}) {
  return document(`action_${label}`, {
    actionKind: "control_reference",
    contractVersion: "forecast-adjustment-rain-control-reference-action/v1",
    controlStateSha256: rainStateSha256,
    createdAt,
    dueMonth: "2026-12",
    expectedCatalogReceiptSha256: null,
    expectedSettingsSha256: settingsSha256,
    expectedSourceCommit: "9".repeat(40),
    expectedSourceRelease: "2026.11.23-1",
    family: "rain",
    fencingToken: "17",
    graphManifestSha256: identity(`${label}/graph-manifest`),
    ordinalArtifactSha256: rainArtifactSha256,
    predecessorActionSha256: null,
    reason: "premonth_reference",
    sourceMemberRootSha256: identity(`${label}/source-member-root`),
    sourceReceiptRootSha256: identity(`${label}/source-receipt-root`),
    validThrough: "2026-12-01T00:00:00.000Z",
    ...overrides,
  });
}

// construct one complete rain control custody authority anchor
function rainAnchor(label, action, actionSha256) {
  return document(`anchor_${label}`, {
    actionKind: "control_reference",
    actionSha256,
    archiveCommitOrdinal: "1",
    captureEpochWitnessSha256: witnessDocumentSha256,
    contractVersion: "adjustment-rain-control-custody-anchor/v1",
    controlSha256,
    controlStateSha256: action.controlStateSha256,
    controlVersion: "17",
    custodyAcknowledgedAt: "2026-10-08T00:35:00.000Z",
    custodyAcknowledgementSha256: identity(`${label}/custody-acknowledgement`),
    custodyCheckpointSha256: identity(`${label}/custody-checkpoint`),
    dueMonth: "2026-12",
    fencingToken: action.fencingToken,
    frontierSha256: identity(`${label}/frontier`),
    fullGraphVerifiedAt: "2026-10-08T00:36:00.000Z",
    graphManifestSha256: action.graphManifestSha256,
    memberRootSha256: identity(`${label}/member-root`),
    ordinalArtifactSha256: action.ordinalArtifactSha256,
    pageSha256: identity(`${label}/page`),
    predecessorAnchorSha256: null,
    sequence: "0",
    sourceCommit: action.expectedSourceCommit,
    sourceMemberRootSha256: action.sourceMemberRootSha256,
    sourceReceiptRootSha256: action.sourceReceiptRootSha256,
    startMemberSha256: null,
    startSha256: identity(`${label}/start`),
    workstationJournalHeadSha256: identity(`${label}/journal`),
  });
}

// construct the target-image rain selector
function rainSelector(label, action, actionSha256) {
  return document(`member_${label}_selector`, {
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
  });
}

// reproduce the original private installed-slot receipt identity formula
function installedReceiptSha256(catalog, family, slot) {
  const entry = catalog.entries.find((candidate) => candidate.family === family &&
    (catalog.contractVersion === "adjustment-installed-candidate-catalog/v1"
      ? slot === "shadow"
      : candidate.slot === slot));
  return entry === undefined ? null : sha256(canonicalBytes(entry.receipt));
}

// retain the exact source-derived canonical family proof and its digest
function releaseFamilyIdentity(name, family, registry, catalog) {
  const proof = document(name, {
    activeInstalledReceiptSha256: installedReceiptSha256(catalog, family, "active"),
    controlInstalledReceiptSha256: family === "rain"
      ? installedReceiptSha256(catalog, family, "control")
      : null,
    family,
    registrySha256: sha256(canonicalBytes(registry)),
    shadowInstalledReceiptSha256: installedReceiptSha256(catalog, family, "shadow"),
  });
  return sha256(canonicalBytes(proof));
}

// retain the exact source-derived canonical immutable-image proof and its digest
function releaseImageIdentity(name, selectedEnvironment) {
  const proof = document(name, [
    {
      digest: selectedEnvironment.WEATHER_SERVER_IMAGE.split("@")[1],
      runtime: "server",
    },
    {
      digest: selectedEnvironment.WEATHER_WEB_IMAGE.split("@")[1],
      runtime: "web",
    },
  ]);
  return sha256(canonicalBytes(proof));
}

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

const fixtures = [];

// retain one complete development composition fixture
function prepareDevelopment(name, registrationOverrides = {}, expectedDoc = null) {
  const action = developmentAction(name);
  const actionSha256 = sha256(canonicalBytes(action));
  const anchor = developmentAnchor(name, action, actionSha256);
  const projection = shadowProjection(name, action, actionSha256, anchor,
    registrationOverrides);
  const path = `config/forecast-adjustments/ballydidean/shadow-catalog/${action.family}/sha256-${action.candidateSha256}.json`;
  fixtures.push({
    actionDoc: `action_${name}`,
    actionSha256,
    anchorDoc: `anchor_${name}`,
    anchorKind: "dev",
    expectedDoc: expectedDoc ?? "error",
    expectedError: expectedDoc === null
      ? "development custody target image binding differs" : null,
    expectedReads: 1,
    family: action.family,
    members: new Map([[path, `member_${name}_projection`]]),
    name,
    projection,
    sourceDoc: "source_empty",
    witnessDoc: "witness",
  });
}

prepareDevelopment("development_success", {}, "expected_development_success");
prepareDevelopment("development_registration_refusal", {
  registrationSha256: identity("development-registration-refusal/wrong-registration"),
});
prepareDevelopment("development_epoch_refusal", {
  epochWitnessSha256: identity("development-epoch-refusal/wrong-epoch"),
});

// retain one complete rain composition fixture
function prepareRain(name, actionOverrides = {}, expectedDoc = null) {
  const action = rainAction(name, actionOverrides);
  const actionSha256 = sha256(canonicalBytes(action));
  const anchor = rainAnchor(name, action, actionSha256);
  const selector = rainSelector(name, action, actionSha256);
  const selectorPath = "config/forecast-adjustments/ballydidean-rain-control-reference.json";
  const statePath = `config/forecast-adjustments/ballydidean/rain-maintenance-control-states/sha256-${rainStateSha256}.json`;
  const artifactPath = `config/forecast-adjustments/ballydidean/rain-runtime-artifacts/sha256-${rainArtifactSha256}.json`;
  fixtures.push({
    actionDoc: `action_${name}`,
    actionSha256,
    anchorDoc: `anchor_${name}`,
    anchorKind: "rain",
    expectedDoc: expectedDoc ?? "error",
    expectedError: expectedDoc === null
      ? "adjustment rain control custody authorization differs" : null,
    expectedReads: expectedDoc === null ? 0 : 3,
    family: "rain",
    members: new Map([
      [selectorPath, `member_${name}_selector`],
      [statePath, "member_rain_state"],
      [artifactPath, "member_rain_artifact"],
    ]),
    name,
    selector,
    sourceDoc: "source_empty",
    witnessDoc: "-",
  });
}

prepareRain("rain_success", {}, "expected_rain_success");
prepareRain("rain_authority_refusal", { dueMonth: "2027-01" });

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

// invoke the original wrapper where ownership permits and its original store otherwise
async function authorize(action, actionSha256, kind, root) {
  // mapped root satisfies the wrapper's fixed root-owner boundary
  if (kind === "rain" || process.getuid() === 0) {
    return authorizeAdjustmentFamilyAction(action, actionSha256, root);
  }
  const current = JSON.parse(await readFile(
    join(root, "development-custody-anchors/current.json"), "utf8"));
  const store = new AdjustmentDevelopmentCustodyAnchorStore({ root });
  return store.authorizeDevelopmentShadowAction({
    actionKind: action.actionKind,
    actionSha256,
    artifactSha256: current.artifactSha256,
    candidateGraphSha256: action.candidateGraphSha256,
    candidateSha256: action.candidateSha256,
    family: action.family,
    lifecycleLedgerRootSha256: action.lifecycleHeadSha256,
    policyReportSha256: action.policyReportSha256,
    registrationSha256: current.registrationSha256,
    sourceCommit: action.expectedSourceCommit,
  });
}

// install one fixture-owned current authority anchor
async function installAnchor(root, kind, anchor) {
  const directory = join(root, kind === "dev"
    ? "development-custody-anchors" : "rain-control-custody-anchors");
  await mkdir(directory, { mode: 0o700 });
  await chmod(directory, 0o700);
  const path = join(directory, "current.json");
  await writeFile(path, canonicalBytes(anchor), { mode: 0o600 });
  await chmod(path, 0o600);
}

// execute one complete authority, target and final-binding composition
async function observe(root, fixture) {
  const caseRoot = join(root, fixture.name);
  await mkdir(caseRoot, { mode: 0o700 });
  await chmod(caseRoot, 0o700);
  const action = documents[fixture.actionDoc];
  const anchor = documents[fixture.anchorDoc];
  const sourceValue = documents[fixture.sourceDoc];
  await installAnchor(caseRoot, fixture.anchorKind, anchor);
  let clockCalls = 0;
  let witnessReads = 0;
  let result;
  let authority;
  let failure;
  const readsBefore = memberRows.length;
  try {
    authority = await authorize(action, fixture.actionSha256,
      fixture.anchorKind, caseRoot);
    const input = {
      action,
      actionSha256: fixture.actionSha256,
      clock: () => {
        clockCalls += 1;
        return installedAt;
      },
      environment,
      imageCommit,
      imageRead: imageReader(fixture.name, fixture.members),
      settingsBytes,
      source: sourceValue,
    };
    // supply custody only to its disjoint rain builder
    if (fixture.anchorKind === "rain") {
      input.custodyAnchorSha256 = sha256(canonicalBytes(authority));
      result = buildInstalledAdjustmentRainControlReferenceCatalog(input);
      validateAdjustmentRainControlReferenceBinding(
        action, fixture.actionSha256, authority, result,
      );
    } else {
      result = await buildInstalledAdjustmentTargetCatalog({
        ...input,
        family: fixture.family,
      });
      witnessReads += 1;
      validateAdjustmentDevelopmentActionBinding(
        action, fixture.actionSha256, authority, result, witness,
      );
    }
  } catch (error) {
    failure = error;
  }
  // preserve only the expected success document or exact staged refusal
  if (fixture.expectedDoc === "error") {
    assert.equal(result === undefined || fixture.anchorKind === "dev", true, fixture.name);
    assert.equal(failure?.message, fixture.expectedError, fixture.name);
  } else {
    assert.equal(failure, undefined, `${fixture.name}: ${failure}`);
    document(fixture.expectedDoc, result);
    // add one complete material fixture without changing the accepted case row
    if (fixture.name === "development_success") {
      const authorityDoc = `expected_${fixture.name}_authority`;
      const targetFamilyProof = `expected_${fixture.name}_target_family_proof`;
      const compensationFamilyProof =
        `expected_${fixture.name}_compensation_family_proof`;
      const targetImageProof = `expected_${fixture.name}_target_image_proof`;
      const compensationImageProof =
        `expected_${fixture.name}_compensation_image_proof`;
      document(authorityDoc, authority);
      materialRows.push([
        "material", fixture.name, "release_environment_target",
        "release_environment_compensation", "release_registry_source_temperature",
        "release_registry_target_temperature", "release_registry_source_temperature",
        authorityDoc, targetFamilyProof,
        releaseFamilyIdentity(targetFamilyProof, fixture.family, targetRegistry, result),
        compensationFamilyProof,
        releaseFamilyIdentity(compensationFamilyProof, fixture.family, sourceRegistry, sourceValue),
        targetImageProof,
        releaseImageIdentity(targetImageProof, targetEnvironment),
        compensationImageProof,
        releaseImageIdentity(compensationImageProof, compensationEnvironment),
      ]);
    }
  }
  assert.equal(memberRows.length - readsBefore, fixture.expectedReads, fixture.name);
  assert.equal(clockCalls, fixture.expectedReads === 0 ? 0 : 1, fixture.name);
  assert.equal(witnessReads, fixture.anchorKind === "dev" ? 1 : 0, fixture.name);
  caseRows.push([
    "case", fixture.name, fixture.actionDoc, fixture.actionSha256, fixture.family,
    fixture.sourceDoc, "environment", imageCommit, "settings", installedAt,
    fixture.witnessDoc, fixture.anchorDoc, fixture.anchorKind, fixture.expectedDoc,
    String(clockCalls), String(witnessReads),
  ]);
}

const root = await mkdtemp(join(tmpdir(), "weather-family-target-preflight-node-oracle-"));
try {
  // preserve deterministic case and member-read ordering
  for (const fixture of fixtures) await observe(root, fixture);
} finally {
  await rm(root, { recursive: true, force: true });
}

// emit every canonical fixture document and typed node
for (const [name, value] of Object.entries(documents)) {
  console.log(["doc", name, canonicalText(value)].join("\t"));
  emitNode(name, "$", value);
  sha256(canonicalBytes(value));
}
// emit every deduplicated original hash preimage
for (const [preimage, digest] of hashRecords) {
  console.log(["hash", preimage, digest].join("\t"));
}
// emit every deterministic fixture clock
for (const clock of [createdAt, reportCreatedAt, validThrough, installedAt,
  "2026-10-08T00:35:00.000Z", "2026-10-08T00:36:00.000Z",
  "2026-12-01T00:00:00.000Z"]) {
  console.log(["clock", clock, String(Date.parse(clock))].join("\t"));
}
// emit the fixed seventeen-field composition protocol
for (const row of caseRows) console.log(row.join("\t"));
// emit the additive full-material fixture separately from the stable case protocol
for (const row of materialRows) console.log(row.join("\t"));
// emit only actual ordered image reads
for (const row of memberRows) console.log(row.join("\t"));
