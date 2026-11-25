import assert from "node:assert/strict";
import { chmodSync, readFileSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import {
  RUNTIME_IMAGE_MATERIAL_INSPECTION_CLASS,
  buildRustRuntimeImageMaterial,
} from "../deploy/scripts/rust-runtime-image-material.mjs";
import {
  BOOTSTRAP_APPROVAL_ARTIFACT_VERSION,
  NATIVE_PUBLICATION_CONTRACT_VERSION,
  NATIVE_PUBLICATION_REQUEST_VERSION,
  NATIVE_PUBLICATION_STAGE_GATE,
  canonicalJsonBytes,
  prepareNativeSchemaPreservingPublication,
  prepareNativeFamilyPublication,
  validateNativeSchemaPreservingPublication,
} from "./prepare-native-schema-preserving-publication.mjs";
import {
  DECLARED_TRANSITION_CONTRACT_VERSION,
  NATIVE_BOOTSTRAP_PROOF_VERSION,
  buildReviewedTransitionDeclaration,
  canonicalSha256sumManifest,
  nativeCanonicalSha256,
  sha256,
  validateNativeBootstrapProof,
  validatePublication,
} from "./schema-preserving-release.mjs";

const SCRIPT = new URL("./prepare-native-schema-preserving-publication.mjs", import.meta.url)
  .pathname;
const MATERIAL_FIXTURE = new URL(
  "../deploy/scripts/rust-runtime-image-material.fixture.json",
  import.meta.url,
);
const TRANSITION_FIXTURE = new URL("./predecessor-rust-transition.fixture.json", import.meta.url);
const TARGET_COMMIT = "b".repeat(40);
const OPERATOR_SHA256 = "e".repeat(64);
const VALIDATOR_SHA256 = "f".repeat(64);
const OPERATOR_BUNDLE_CONTRACT_VERSION = "weather-operator-bundle-manifest/v1";
const INSTALLED_RECEIPT_SHA256 = "9".repeat(64);

// derive the exact target operator manifest used by the preparation request
function operatorBundle() {
  const members = [
    { path: "schema-preserving-release.mjs", sha256: VALIDATOR_SHA256 },
    { path: "schema-preserving-release.sh", sha256: OPERATOR_SHA256 },
  ];
  const sha256sum = canonicalSha256sumManifest(members);
  return {
    contractVersion: OPERATOR_BUNDLE_CONTRACT_VERSION,
    manifestSha256: sha256(Buffer.from(sha256sum, "utf8")),
    members,
    sha256sum,
  };
}

// build one inspected-material fixture from the existing closed material grammar
function inspectedMaterial() {
  const input = JSON.parse(readFileSync(MATERIAL_FIXTURE, "utf8"));
  input.source.commit = TARGET_COMMIT;
  const material = buildRustRuntimeImageMaterial(input);
  material.artifactClass = RUNTIME_IMAGE_MATERIAL_INSPECTION_CLASS;
  material.inspection = {
    extractedFileBytes: true,
    provesBuildIdentity: false,
    provesElfArchitecture: true,
    provesImageIdentity: false,
    provesPlatform: false,
    provesSourceIdentity: false,
  };
  return material;
}

// build one finite transition with a real declared retained-member change and target operator bytes
function finiteTransition() {
  const transition = JSON.parse(readFileSync(TRANSITION_FIXTURE, "utf8"));
  const predecessorDocker = transition.predecessorMembers.find(
    // select the fixed retained member
    (member) => member.path === "Dockerfile",
  );
  const targetDocker = transition.targetMembers.find(
    // select the fixed target member
    (member) => member.path === "Dockerfile",
  );
  targetDocker.sha256 = "2".repeat(64);
  transition.changedMembers = [{
    path: "Dockerfile",
    predecessorSha256: predecessorDocker.sha256,
    targetSha256: targetDocker.sha256,
  }];
  transition.contractVersion = DECLARED_TRANSITION_CONTRACT_VERSION;
  transition.reviewedDeclaration = buildReviewedTransitionDeclaration({
    addedFiles: [],
    changedMembers: structuredClone(transition.changedMembers),
    predecessorCommit: transition.predecessor.commit,
    targetCommit: transition.target.commit,
  });
  const operatorMembers = [
    { path: "scripts/schema-preserving-release.mjs", sha256: VALIDATOR_SHA256 },
    { path: "scripts/schema-preserving-release.sh", sha256: OPERATOR_SHA256 },
  ];
  transition.predecessorMembers.push(...structuredClone(operatorMembers));
  transition.targetMembers.push(...structuredClone(operatorMembers));
  transition.predecessorMembers.sort(
    // retain canonical finite member order
    (left, right) => left.path < right.path ? -1 : 1,
  );
  transition.targetMembers.sort(
    // retain canonical finite member order
    (left, right) => left.path < right.path ? -1 : 1,
  );
  return transition;
}

// build one complete pure preparation request
function requestFixture() {
  const transition = finiteTransition();
  const transitionSha256 = sha256(canonicalJsonBytes(transition));
  const bundle = operatorBundle();
  const imageMaterial = inspectedMaterial();
  const releaseEvidence = {
    check: {
      conclusion: "success",
      databaseId: 101,
      event: "push",
      headSha: TARGET_COMMIT,
      name: "Check",
      status: "completed",
      url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/101",
    },
    operatorSha256: OPERATOR_SHA256,
    publish: {
      conclusion: "success",
      databaseId: 102,
      event: "push",
      headBranch: transition.target.release,
      headSha: TARGET_COMMIT,
      name: "Publish production images",
      status: "completed",
      url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/102",
    },
    serverImage: `ghcr.io/anstosa/weather-server@sha256:${"a".repeat(64)}`,
    serverImageId: `sha256:${"c".repeat(64)}`,
    targetCommit: TARGET_COMMIT,
    targetRelease: transition.target.release,
    validatorSha256: VALIDATOR_SHA256,
    webImage: `ghcr.io/anstosa/weather-web@sha256:${"d".repeat(64)}`,
    webImageId: `sha256:${"e".repeat(64)}`,
  };
  return {
    bootstrapArtifact: {
      approvalClass: "operator-reviewed-finite-transition",
      approvedTransitionSha256: transitionSha256,
      artifactVersion: BOOTSTRAP_APPROVAL_ARTIFACT_VERSION,
      targetOperatorBundleSha256: bundle.manifestSha256,
      transition,
    },
    imagePublicationProof: {
      contractVersion: "weather-native-image-publication-proof/v1",
      generation: "rust-v1",
      imageMaterial,
      imageMaterialSha256: sha256(canonicalJsonBytes(imageMaterial)),
      publishedImages: [
        {
          configId: releaseEvidence.serverImageId,
          materialManifestSha256: imageMaterial.images[0].materialManifestSha256,
          name: "server",
          reference: releaseEvidence.serverImage,
        },
        {
          configId: releaseEvidence.webImageId,
          materialManifestSha256: imageMaterial.images[1].materialManifestSha256,
          name: "web",
          reference: releaseEvidence.webImage,
        },
      ],
      release: releaseEvidence.targetRelease,
      sourceCommit: releaseEvidence.targetCommit,
    },
    releaseEvidence,
    requestVersion: NATIVE_PUBLICATION_REQUEST_VERSION,
    sourceGeneration: "node-v1",
    targetGeneration: "rust-v1",
  };
}

// write one isolated private receipt accepted by the installed operator CLI
function privateJson(directory, name, value) {
  const path = join(directory, name);
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  chmodSync(path, 0o600);
  return path;
}

// build one live-bootstrap proof fixture against a distinct installed target receipt
function bootstrapProof(publication) {
  const predecessor = publication.transition.predecessor;
  return {
    approvalSha256: nativeCanonicalSha256({ ...publication.bootstrapApproval, transition: publication.transition }),
    contractVersion: NATIVE_BOOTSTRAP_PROOF_VERSION,
    controlSha256: predecessor.controlSha256,
    predecessorBundleSha256: predecessor.bundleSha256,
    predecessorDropInSha256: predecessor.recoveryDropInSha256,
    predecessorInstallationReceiptSha256: predecessor.installationReceiptSha256,
    protectedSnapshotSha256: "b".repeat(64),
    sourceCommit: predecessor.commit,
    sourceEnvironmentSha256: predecessor.sourceEnvironmentSha256,
    sourceRelease: predecessor.release,
    targetBundleSha256: publication.operatorBundle.manifestSha256,
    targetCommit: publication.targetCommit,
    targetInstallationReceiptSha256: INSTALLED_RECEIPT_SHA256,
    targetRelease: publication.targetRelease,
    transitionSha256: publication.transitionSha256,
  };
}

// stage one family-only native descendant using the same inspected image material
test("prepares a family descendant bound to the exact native source and reciprocal release", () => {
  const fixture = requestFixture();
  const request = {
    family: "rain",
    familyActionSha256: "a".repeat(64),
    familyRole: "target",
    imagePublicationProof: fixture.imagePublicationProof,
    pairedRelease: "2099.01.01-3",
    releaseEvidence: fixture.releaseEvidence,
    requestVersion: "weather-native-family-publication-request/v1",
    sourceCommit: "c".repeat(40),
    sourcePublicationSha256: "b".repeat(64),
    sourceRelease: fixture.bootstrapArtifact.transition.predecessor.release,
  };
  const publication = prepareNativeFamilyPublication(request);
  assert.equal(publication.contractVersion, "weather-schema-preserving-family-publication/v1");
  assert.equal(publication.sourceGeneration, "rust-v1");
  assert.equal(validatePublication(publication, publication.targetRelease), publication);
  assert.equal(publication.familyActionSha256, request.familyActionSha256);
  assert.equal(publication.pairedRelease, request.pairedRelease);
});

// prepare a closed v3 document while leaving the live installed authority unresolved
test("prepares exact native publication material without granting live stage authority", () => {
  const request = requestFixture();
  const publication = prepareNativeSchemaPreservingPublication(request);
  assert.equal(publication.contractVersion, NATIVE_PUBLICATION_CONTRACT_VERSION);
  assert.equal(validateNativeSchemaPreservingPublication(publication), publication);
  assert.equal(validatePublication(publication, publication.targetRelease), publication);
  assert.equal(publication.sourceGeneration, "node-v1");
  assert.equal(publication.targetGeneration, "rust-v1");
  assert.equal(publication.targetCommit, TARGET_COMMIT);
  assert.equal(publication.targetRelease, request.bootstrapArtifact.transition.target.release);
  assert.equal(publication.approvedTransitionSha256, request.bootstrapArtifact.approvedTransitionSha256);
  assert.equal(publication.transitionSha256, request.bootstrapArtifact.approvedTransitionSha256);
  assert.equal(publication.operatorBundle.manifestSha256, operatorBundle().manifestSha256);
  assert.match(publication.materialManifestSha256, /^[a-f0-9]{64}$/u);
  assert.match(publication.imagePublicationProofSha256, /^[a-f0-9]{64}$/u);
  assert.equal(publication.liveAuthorityEstablished, false);
  assert.equal(publication.eligibleForLiveStage, false);
  assert.equal(publication.stageGate, NATIVE_PUBLICATION_STAGE_GATE);
  assert.equal(
    publication.requiredInstallationReceiptSha256,
    request.bootstrapArtifact.transition.predecessor.installationReceiptSha256,
  );
});

// prove compatibility with the installed validator and its isolated operator commands
test("installed validator accepts coordinates and later distinct live bootstrap proof", () => {
  const request = requestFixture();
  const publication = prepareNativeSchemaPreservingPublication(request);
  const proof = bootstrapProof(publication);
  assert.notEqual(
    proof.targetInstallationReceiptSha256,
    proof.predecessorInstallationReceiptSha256,
  );
  assert.equal(
    validateNativeBootstrapProof(
      proof,
      publication,
      INSTALLED_RECEIPT_SHA256,
      publication.operatorBundle.manifestSha256,
    ),
    proof,
  );

  const directory = mkdtempSync(join(tmpdir(), "native-publication-installed-validator-"));
  const installedScript = new URL("./schema-preserving-release.mjs", import.meta.url).pathname;
  try {
    const publicationPath = privateJson(directory, "publication.json", publication);
    const approvalPath = privateJson(directory, "approval.json", request.bootstrapArtifact);
    const proofPath = privateJson(directory, "proof.json", proof);
    const validate = spawnSync(process.execPath, [
      installedScript,
      "validate-publication",
      publicationPath,
      publication.targetRelease,
    ], { encoding: "utf8" });
    assert.equal(validate.status, 0, validate.stderr);
    assert.deepEqual(JSON.parse(validate.stdout), publication);

    const coordinates = spawnSync(process.execPath, [
      installedScript,
      "native-bootstrap-coordinates",
      approvalPath,
    ], { encoding: "utf8" });
    assert.equal(coordinates.status, 0, coordinates.stderr);
    assert.deepEqual(coordinates.stdout.trimEnd().split("\t"), [
      publication.transitionSha256,
      publication.sourceRelease,
      publication.transition.predecessor.commit,
      publication.transition.predecessor.sourceEnvironmentSha256,
      publication.transition.predecessor.controlSha256,
      publication.transition.predecessor.bundleSha256,
      publication.requiredInstallationReceiptSha256,
      publication.transition.predecessor.recoveryDropInSha256,
      publication.targetRelease,
      publication.targetCommit,
      publication.operatorBundle.manifestSha256,
      proof.approvalSha256,
    ]);

    const proofValidation = spawnSync(process.execPath, [
      installedScript,
      "validate-native-bootstrap-proof",
      proofPath,
      publicationPath,
      INSTALLED_RECEIPT_SHA256,
      publication.operatorBundle.manifestSha256,
    ], { encoding: "utf8" });
    assert.equal(proofValidation.status, 0, proofValidation.stderr);
    assert.equal(proofValidation.stdout, "");
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});

// keep CLI preparation byte-equivalent to the pure interface
test("CLI emits the same deterministic v3 preparation", () => {
  const request = requestFixture();
  const directory = mkdtempSync(join(tmpdir(), "native-publication-fixture-"));
  const path = join(directory, "request.json");
  try {
    writeFileSync(path, `${JSON.stringify(request)}\n`);
    const result = spawnSync(process.execPath, [SCRIPT, path], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), prepareNativeSchemaPreservingPublication(request));
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});

// reject the predecessor preparer's deliberately ineligible offline artifact
test("rejects offline-only and self-asserted live bootstrap artifacts", () => {
  const offline = requestFixture();
  offline.bootstrapArtifact = {
    artifactVersion: "weather-predecessor-rust-offline-bootstrap-artifact/v1",
    eligibility: "offline-only-ineligible",
    eligibleForLiveInstallation: false,
    liveAuthorityVerified: false,
    transition: offline.bootstrapArtifact.transition,
  };
  assert.throws(
    () => prepareNativeSchemaPreservingPublication(offline),
    /unexpected fields/u,
  );

  const synthetic = requestFixture();
  synthetic.bootstrapArtifact.liveAuthorityVerified = true;
  assert.throws(
    () => prepareNativeSchemaPreservingPublication(synthetic),
    /unexpected fields/u,
  );

  const undeclared = requestFixture();
  undeclared.bootstrapArtifact.transition.contractVersion =
    "weather-predecessor-rust-transition/v1";
  delete undeclared.bootstrapArtifact.transition.reviewedDeclaration;
  undeclared.bootstrapArtifact.approvedTransitionSha256 = sha256(
    canonicalJsonBytes(undeclared.bootstrapArtifact.transition),
  );
  assert.throws(
    () => prepareNativeSchemaPreservingPublication(undeclared),
    /not an approved finite transition/u,
  );
});

// refuse drift across workflow, material, transition and operator identities
test("rejects any identity drift across the pure publication evidence", () => {
  const failedCheck = requestFixture();
  failedCheck.releaseEvidence.check.conclusion = "failure";
  assert.throws(
    () => prepareNativeSchemaPreservingPublication(failedCheck),
    /exact successful target push/u,
  );

  const offlineMaterial = requestFixture();
  offlineMaterial.imagePublicationProof.imageMaterial.artifactClass =
    "offline-image-material-preflight";
  delete offlineMaterial.imagePublicationProof.imageMaterial.inspection;
  assert.throws(
    () => prepareNativeSchemaPreservingPublication(offlineMaterial),
    /unexpected fields|not an extracted native inspection/u,
  );

  const wrongMaterialIdentity = requestFixture();
  wrongMaterialIdentity.imagePublicationProof.imageMaterialSha256 = "0".repeat(64);
  assert.throws(
    () => prepareNativeSchemaPreservingPublication(wrongMaterialIdentity),
    /material identity differs/u,
  );

  const wrongPublishedConfig = requestFixture();
  wrongPublishedConfig.imagePublicationProof.publishedImages[0].configId =
    `sha256:${"9".repeat(64)}`;
  assert.throws(
    () => prepareNativeSchemaPreservingPublication(wrongPublishedConfig),
    /publishedImages\[0\] differs/u,
  );

  const wrongApproval = requestFixture();
  wrongApproval.bootstrapArtifact.approvedTransitionSha256 = "0".repeat(64);
  assert.throws(
    () => prepareNativeSchemaPreservingPublication(wrongApproval),
    /approval identit(?:y|ies) differ/u,
  );

  const missingOperator = requestFixture();
  missingOperator.bootstrapArtifact.transition.targetMembers =
    missingOperator.bootstrapArtifact.transition.targetMembers.filter(
      // remove only the target validator identity
      (member) => member.path !== "scripts/schema-preserving-release.mjs",
    );
  missingOperator.bootstrapArtifact.approvedTransitionSha256 = sha256(
    canonicalJsonBytes(missingOperator.bootstrapArtifact.transition),
  );
  assert.throws(
    () => prepareNativeSchemaPreservingPublication(missingOperator),
    /missing from targetMembers|absent from the finite transition/u,
  );
});

// reject mutation of the explicit unresolved live gate after preparation
test("v3 validator rejects a synthetic live-stage promotion", () => {
  const publication = prepareNativeSchemaPreservingPublication(requestFixture());
  const promoted = structuredClone(publication);
  promoted.eligibleForLiveStage = true;
  promoted.liveAuthorityEstablished = true;
  assert.throws(
    () => validateNativeSchemaPreservingPublication(promoted),
    /native publication identity differs/u,
  );

  const reboundImage = structuredClone(publication);
  reboundImage.imagePublicationProof.publishedImages[0].configId =
    `sha256:${"8".repeat(64)}`;
  assert.throws(
    () => validateNativeSchemaPreservingPublication(reboundImage),
    /native image publication material differs/u,
  );
});
