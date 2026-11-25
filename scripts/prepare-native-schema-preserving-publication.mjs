#!/usr/bin/env node

import { createHash } from "node:crypto";
import { lstatSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  RUNTIME_IMAGE_MATERIAL_INPUT_VERSION,
  RUNTIME_IMAGE_MATERIAL_INSPECTION_CLASS,
  RUNTIME_IMAGE_MATERIAL_VERSION,
  buildRustRuntimeImageMaterial,
} from "../deploy/scripts/rust-runtime-image-material.mjs";
import {
  DECLARED_TRANSITION_CONTRACT_VERSION,
  NATIVE_BOOTSTRAP_APPROVAL_VERSION,
  NATIVE_FAMILY_PUBLICATION_CONTRACT_VERSION,
  validateNativeFamilyPublication,
  NATIVE_PUBLICATION_CONTRACT_VERSION as INSTALLED_NATIVE_PUBLICATION_CONTRACT_VERSION,
  RUNTIME_GENERATION_NODE,
  RUNTIME_GENERATION_RUST,
  canonicalSha256sumManifest,
  sha256,
  validateNativeBootstrapApproval,
  validateNativePublication,
  validateReleaseName,
} from "./schema-preserving-release.mjs";

export const NATIVE_PUBLICATION_REQUEST_VERSION =
  "weather-native-schema-preserving-publication-request/v1";
export const NATIVE_PUBLICATION_CONTRACT_VERSION =
  INSTALLED_NATIVE_PUBLICATION_CONTRACT_VERSION;
export const BOOTSTRAP_APPROVAL_ARTIFACT_VERSION =
  NATIVE_BOOTSTRAP_APPROVAL_VERSION;
export const NATIVE_PUBLICATION_STAGE_GATE =
  "requires-live-retained-installation-receipt-verification";
export const NATIVE_IMAGE_PUBLICATION_PROOF_VERSION =
  "weather-native-image-publication-proof/v1";

const MAX_REQUEST_BYTES = 16 * 1024 * 1024;
const OPERATOR_BUNDLE_CONTRACT_VERSION = "weather-operator-bundle-manifest/v1";
const SHA256 = /^[a-f0-9]{64}$/u;
const COMMIT_SHA = /^[a-f0-9]{40}$/u;
const IMAGE_ID = /^sha256:[a-f0-9]{64}$/u;
const SERVER_IMAGE = /^ghcr\.io\/anstosa\/weather-server@sha256:[a-f0-9]{64}$/u;
const WEB_IMAGE = /^ghcr\.io\/anstosa\/weather-web@sha256:[a-f0-9]{64}$/u;
const ACTION_RUN_URL =
  /^https:\/\/github\.com\/anstosa\/Ballydidean-Weather\/actions\/runs\/([1-9]\d*)$/u;
const REQUEST_KEYS = [
  "bootstrapArtifact",
  "imagePublicationProof",
  "releaseEvidence",
  "requestVersion",
  "sourceGeneration",
  "targetGeneration",
];
const BOOTSTRAP_KEYS = [
  "approvalClass",
  "approvedTransitionSha256",
  "artifactVersion",
  "targetOperatorBundleSha256",
  "transition",
];
const RELEASE_EVIDENCE_KEYS = [
  "check",
  "operatorSha256",
  "publish",
  "serverImage",
  "serverImageId",
  "targetCommit",
  "targetRelease",
  "validatorSha256",
  "webImage",
  "webImageId",
];
const CHECK_KEYS = [
  "conclusion",
  "databaseId",
  "event",
  "headSha",
  "name",
  "status",
  "url",
];
const PUBLISH_KEYS = [...CHECK_KEYS, "headBranch"];
const MATERIAL_KEYS = [
  "artifactClass",
  "attestsBuiltImage",
  "boundary",
  "build",
  "contractVersion",
  "images",
  "inspection",
  "platform",
  "source",
];
const INSPECTION_KEYS = [
  "extractedFileBytes",
  "provesBuildIdentity",
  "provesElfArchitecture",
  "provesImageIdentity",
  "provesPlatform",
  "provesSourceIdentity",
];
const IMAGE_PUBLICATION_PROOF_KEYS = [
  "contractVersion",
  "generation",
  "imageMaterial",
  "imageMaterialSha256",
  "publishedImages",
  "release",
  "sourceCommit",
];
const PUBLISHED_IMAGE_KEYS = [
  "configId",
  "materialManifestSha256",
  "name",
  "reference",
];

// normalize preparation failures
function fail(message) {
  throw new Error(message);
}

// require one plain object
function requireObject(value, label) {
  // reject arrays and custom objects
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    fail(`${label} must be an object`);
  }
  const prototype = Object.getPrototypeOf(value);
  // reject custom prototypes
  if (prototype !== Object.prototype && prototype !== null) {
    fail(`${label} must be a plain object`);
  }
  return value;
}

// enforce one closed object shape
function requireExactKeys(value, expected, label) {
  requireObject(value, label);
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  // reject shape drift
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    fail(`${label} has unexpected fields`);
  }
}

// require one lowercase digest
function requireSha256(value, label) {
  // reject mutable or malformed identities
  if (typeof value !== "string" || !SHA256.test(value)) {
    fail(`${label} must be a lowercase SHA-256 digest`);
  }
  return value;
}

// recursively normalize one JSON value for identity hashing
function canonicalJsonValue(value, label = "canonical JSON") {
  // preserve the JSON scalar domain
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  // preserve only finite JSON numbers
  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      fail(`${label} contains a non-finite number`);
    }
    return value;
  }
  // preserve array order while normalizing members
  if (Array.isArray(value)) {
    return value.map((member, index) => canonicalJsonValue(member, `${label}[${index}]`));
  }
  requireObject(value, label);
  const normalized = {};
  // sort object keys bytewise before hashing
  for (const key of Object.keys(value).sort()) {
    normalized[key] = canonicalJsonValue(value[key], `${label}.${key}`);
  }
  return normalized;
}

// encode one deterministic canonical JSON document
export function canonicalJsonBytes(value) {
  return Buffer.from(`${JSON.stringify(canonicalJsonValue(value))}\n`, "utf8");
}

// compare two admitted JSON values independent of insertion order
function requireCanonicalEqual(actual, expected, label) {
  // reject any semantic drift
  if (!canonicalJsonBytes(actual).equals(canonicalJsonBytes(expected))) {
    fail(`${label} differs`);
  }
}

// derive the exact target two-file operator bundle from the retained publication fields
function targetOperatorBundle(publication) {
  const members = [
    { path: "schema-preserving-release.mjs", sha256: publication.validatorSha256 },
    { path: "schema-preserving-release.sh", sha256: publication.operatorSha256 },
  ];
  const sha256sum = canonicalSha256sumManifest(members);
  return {
    contractVersion: OPERATOR_BUNDLE_CONTRACT_VERSION,
    manifestSha256: sha256(Buffer.from(sha256sum, "utf8")),
    members,
    sha256sum,
  };
}

// validate one independently extracted native material inspection
function validateImageMaterial(value, targetCommit) {
  requireExactKeys(value, MATERIAL_KEYS, "image material");
  // require the inspector result rather than its explicitly offline preflight sibling
  if (
    value.artifactClass !== RUNTIME_IMAGE_MATERIAL_INSPECTION_CLASS
    || value.attestsBuiltImage !== false
    || value.contractVersion !== RUNTIME_IMAGE_MATERIAL_VERSION
  ) {
    fail("image material is not an extracted native inspection");
  }
  requireExactKeys(value.inspection, INSPECTION_KEYS, "image material inspection");
  // require the two facts the extracted-tree inspector actually proves
  if (
    value.inspection.extractedFileBytes !== true
    || value.inspection.provesElfArchitecture !== true
    || value.inspection.provesBuildIdentity !== false
    || value.inspection.provesImageIdentity !== false
    || value.inspection.provesPlatform !== false
    || value.inspection.provesSourceIdentity !== false
  ) {
    fail("image material inspection proof ceiling differs");
  }
  // require canonical server then web material before reconstruction
  if (
    !Array.isArray(value.images)
    || value.images.length !== 2
    || value.images[0]?.name !== "server"
    || value.images[1]?.name !== "web"
  ) {
    fail("image material must contain canonical server and web records");
  }
  // bind material production to the exact publication commit
  if (value.source?.commit !== targetCommit) {
    fail("image material source commit does not match targetCommit");
  }
  const input = {
    actual: {
      server: value.images[0].members,
      web: value.images[1].members,
    },
    build: value.build,
    expected: {
      server: value.images[0].members,
      web: value.images[1].members,
    },
    inputVersion: RUNTIME_IMAGE_MATERIAL_INPUT_VERSION,
    source: value.source,
  };
  const reconstructed = buildRustRuntimeImageMaterial(input);
  requireCanonicalEqual(value.boundary, reconstructed.boundary, "image material boundary");
  requireCanonicalEqual(value.build, reconstructed.build, "image material build");
  requireCanonicalEqual(value.images, reconstructed.images, "image material images");
  requireCanonicalEqual(value.platform, reconstructed.platform, "image material platform");
  requireCanonicalEqual(value.source, reconstructed.source, "image material source");
  return sha256(canonicalJsonBytes(value));
}

// validate the publisher's independent binding from inspected material to immutable image configs
export function validateImagePublicationProof(value, releaseEvidence) {
  requireExactKeys(value, IMAGE_PUBLICATION_PROOF_KEYS, "image publication proof");
  // bind one exact native release proof to the workflow publication target
  if (
    value.contractVersion !== NATIVE_IMAGE_PUBLICATION_PROOF_VERSION
    || value.generation !== RUNTIME_GENERATION_RUST
    || value.release !== releaseEvidence.targetRelease
    || value.sourceCommit !== releaseEvidence.targetCommit
  ) {
    fail("image publication proof target differs");
  }
  const materialManifestSha256 = validateImageMaterial(
    value.imageMaterial,
    releaseEvidence.targetCommit,
  );
  // bind the nested inspection to its canonical complete identity
  if (value.imageMaterialSha256 !== materialManifestSha256) {
    fail("image publication proof material identity differs");
  }
  // require canonical server then web publication records
  if (!Array.isArray(value.publishedImages) || value.publishedImages.length !== 2) {
    fail("image publication proof must contain server and web records");
  }
  const expected = [
    {
      configId: releaseEvidence.serverImageId,
      materialManifestSha256: value.imageMaterial.images[0].materialManifestSha256,
      name: "server",
      reference: releaseEvidence.serverImage,
    },
    {
      configId: releaseEvidence.webImageId,
      materialManifestSha256: value.imageMaterial.images[1].materialManifestSha256,
      name: "web",
      reference: releaseEvidence.webImage,
    },
  ];
  // validate both exact immutable image-to-material bindings
  for (const [index, image] of value.publishedImages.entries()) {
    requireExactKeys(image, PUBLISHED_IMAGE_KEYS, `publishedImages[${index}]`);
    requireCanonicalEqual(image, expected[index], `publishedImages[${index}]`);
  }
  return {
    imagePublicationProofSha256: sha256(canonicalJsonBytes(value)),
    materialManifestSha256,
  };
}

// validate one already selected workflow receipt without performing any remote operation
function validateWorkflowReceipt(value, expected, targetCommit, targetRelease) {
  requireExactKeys(value, expected.keys, expected.label);
  // require one stable workflow run identity
  if (!Number.isSafeInteger(value.databaseId) || value.databaseId <= 0) {
    fail(`${expected.label}.databaseId is invalid`);
  }
  // bind the exact workflow commit and successful push result
  if (
    value.name !== expected.name
    || value.headSha !== targetCommit
    || value.status !== "completed"
    || value.conclusion !== "success"
    || value.event !== "push"
  ) {
    fail(`${expected.label} must be the exact successful target push`);
  }
  const url = typeof value.url === "string" ? ACTION_RUN_URL.exec(value.url) : null;
  // bind the canonical repository run URL to the numeric identity
  if (url === null || Number(url[1]) !== value.databaseId) {
    fail(`${expected.label}.url is invalid`);
  }
  // bind the publication workflow to the immutable release tag
  if (expected.label === "publish" && value.headBranch !== targetRelease) {
    fail("publish.headBranch does not match targetRelease");
  }
}

// validate immutable workflow, image and operator evidence without v2 protected-tree substitution
export function validateReleaseEvidence(value) {
  requireExactKeys(value, RELEASE_EVIDENCE_KEYS, "release evidence");
  validateReleaseName(value.targetRelease);
  // require one immutable target commit and two disjoint immutable native images
  if (
    typeof value.targetCommit !== "string"
    || !COMMIT_SHA.test(value.targetCommit)
    || !SERVER_IMAGE.test(value.serverImage ?? "")
    || !WEB_IMAGE.test(value.webImage ?? "")
    || !IMAGE_ID.test(value.serverImageId ?? "")
    || !IMAGE_ID.test(value.webImageId ?? "")
    || value.serverImage === value.webImage
    || value.serverImageId === value.webImageId
  ) {
    fail("release evidence immutable target identity is invalid");
  }
  requireSha256(value.operatorSha256, "operatorSha256");
  requireSha256(value.validatorSha256, "validatorSha256");
  validateWorkflowReceipt(value.check, {
    keys: CHECK_KEYS,
    label: "check",
    name: "Check",
  }, value.targetCommit, value.targetRelease);
  validateWorkflowReceipt(value.publish, {
    keys: PUBLISH_KEYS,
    label: "publish",
    name: "Publish production images",
  }, value.targetCommit, value.targetRelease);
  // require independent Check and Publish runs
  if (value.check.databaseId === value.publish.databaseId) {
    fail("check and publish workflow runs must differ");
  }
  return value;
}

// project one v3 output into its closed workflow and immutable image receipt
function releaseEvidenceFrom(value) {
  return {
    check: value.check,
    operatorSha256: value.operatorSha256,
    publish: value.publish,
    serverImage: value.serverImage,
    serverImageId: value.serverImageId,
    targetCommit: value.targetCommit,
    targetRelease: value.targetRelease,
    validatorSha256: value.validatorSha256,
    webImage: value.webImage,
    webImageId: value.webImageId,
  };
}

// validate one reviewed finite transition without claiming installed live authority
function validateBootstrapArtifact(value, releaseEvidence, operatorBundle) {
  requireExactKeys(value, BOOTSTRAP_KEYS, "bootstrap artifact");
  // reject the offline-only bootstrap artifact and any self-asserted live eligibility flags
  if (
    value.artifactVersion !== BOOTSTRAP_APPROVAL_ARTIFACT_VERSION
    || value.approvalClass !== "operator-reviewed-finite-transition"
    || value.transition?.contractVersion !== DECLARED_TRANSITION_CONTRACT_VERSION
  ) {
    fail("bootstrap artifact is not an approved finite transition");
  }
  const installedApproval = validateNativeBootstrapApproval(value);
  requireCanonicalEqual(
    installedApproval.bundle,
    operatorBundle,
    "installed target operator bundle",
  );
  const transitionSha256 = sha256(canonicalJsonBytes(value.transition));
  requireSha256(value.approvedTransitionSha256, "approvedTransitionSha256");
  requireSha256(value.targetOperatorBundleSha256, "targetOperatorBundleSha256");
  // bind the approval to the exact finite transition and new operator bundle
  if (
    value.approvedTransitionSha256 !== transitionSha256
    || value.targetOperatorBundleSha256 !== installedApproval.bundle.manifestSha256
  ) {
    fail("bootstrap approval identities differ");
  }
  // bind the transition target to the existing exact workflow receipts
  if (
    value.transition.target.commit !== releaseEvidence.targetCommit
    || value.transition.target.release !== releaseEvidence.targetRelease
  ) {
    fail("bootstrap transition target differs from release evidence");
  }
  const targetMembers = new Map(
    // index exact target members for operator byte binding
    value.transition.targetMembers.map((member) => [member.path, member.sha256]),
  );
  // bind the new operator bundle to exact target-commit transition members
  if (
    targetMembers.get("scripts/schema-preserving-release.mjs") !== releaseEvidence.validatorSha256
    || targetMembers.get("scripts/schema-preserving-release.sh") !== releaseEvidence.operatorSha256
  ) {
    fail("target operator bundle is absent from the finite transition");
  }
  return transitionSha256;
}

// validate one complete pure native publication preparation
export function validateNativeSchemaPreservingPublication(value) {
  const installed = validateNativePublication(value, value?.targetRelease);
  // require the declaration-bearing finite authority in addition to the installed common validator
  if (installed.transition.contractVersion !== DECLARED_TRANSITION_CONTRACT_VERSION) {
    fail("native publication requires the reviewed finite transition contract");
  }
  const imageProof = validateImagePublicationProof(
    installed.imagePublicationProof,
    releaseEvidenceFrom(installed),
  );
  // retain full member reconstruction beyond the installed proof's closed coordinate validation
  if (
    installed.materialManifestSha256 !== imageProof.materialManifestSha256
    || installed.imagePublicationProofSha256 !== imageProof.imagePublicationProofSha256
  ) {
    fail("native publication image proof identities differ");
  }
  return installed;
}

// prepare one side-effect-free native publication from independently supplied evidence
export function prepareNativeSchemaPreservingPublication(input) {
  requireExactKeys(input, REQUEST_KEYS, "native publication request");
  // require the versioned request and explicit one-way generation transition
  if (
    input.requestVersion !== NATIVE_PUBLICATION_REQUEST_VERSION
    || input.sourceGeneration !== RUNTIME_GENERATION_NODE
    || input.targetGeneration !== RUNTIME_GENERATION_RUST
  ) {
    fail("native publication request generation is invalid");
  }
  // retain exact Check and Publish receipt semantics without invoking v2 tree authority
  const releaseEvidence = validateReleaseEvidence(input.releaseEvidence);
  const operatorBundle = targetOperatorBundle(releaseEvidence);
  const transitionSha256 = validateBootstrapArtifact(
    input.bootstrapArtifact,
    releaseEvidence,
    operatorBundle,
  );
  const {
    imagePublicationProofSha256,
    materialManifestSha256,
  } = validateImagePublicationProof(input.imagePublicationProof, releaseEvidence);
  const { transition, ...bootstrapApproval } = input.bootstrapArtifact;
  const output = {
    approvedTransitionSha256: transitionSha256,
    bootstrapApproval: structuredClone(bootstrapApproval),
    check: structuredClone(releaseEvidence.check),
    contractVersion: NATIVE_PUBLICATION_CONTRACT_VERSION,
    eligibleForLiveStage: false,
    imagePublicationProof: structuredClone(input.imagePublicationProof),
    imagePublicationProofSha256,
    liveAuthorityEstablished: false,
    materialManifestSha256,
    operatorBundle,
    operatorSha256: releaseEvidence.operatorSha256,
    publish: structuredClone(releaseEvidence.publish),
    requiredInstallationReceiptSha256: transition.predecessor.installationReceiptSha256,
    serverImage: releaseEvidence.serverImage,
    serverImageId: releaseEvidence.serverImageId,
    sourceGeneration: input.sourceGeneration,
    sourceRelease: transition.predecessor.release,
    stageGate: NATIVE_PUBLICATION_STAGE_GATE,
    targetCommit: releaseEvidence.targetCommit,
    targetGeneration: input.targetGeneration,
    targetRelease: releaseEvidence.targetRelease,
    transition: structuredClone(transition),
    transitionSha256,
    validatorSha256: releaseEvidence.validatorSha256,
    webImage: releaseEvidence.webImage,
    webImageId: releaseEvidence.webImageId,
  };
  return validateNativeSchemaPreservingPublication(output);
}

// prepare an immutable family staging document from native image publication and retained source evidence
export function prepareNativeFamilyPublication(input) {
  requireExactKeys(input, [
    "family", "familyActionSha256", "familyRole", "imagePublicationProof",
    "pairedRelease", "releaseEvidence", "requestVersion", "sourceCommit",
    "sourcePublicationSha256", "sourceRelease",
  ], "native family publication request");
  // keep this command disjoint from the one-time Node-to-Rust bootstrap
  if (input.requestVersion !== "weather-native-family-publication-request/v1") {
    fail("native family publication request version differs");
  }
  const releaseEvidence = validateReleaseEvidence(input.releaseEvidence);
  const { imagePublicationProofSha256, materialManifestSha256 } =
    validateImagePublicationProof(input.imagePublicationProof, releaseEvidence);
  return validateNativeFamilyPublication({
    ...structuredClone(releaseEvidence),
    contractVersion: NATIVE_FAMILY_PUBLICATION_CONTRACT_VERSION,
    family: input.family,
    familyActionSha256: input.familyActionSha256,
    familyRole: input.familyRole,
    imagePublicationProof: structuredClone(input.imagePublicationProof),
    imagePublicationProofSha256,
    materialManifestSha256,
    pairedRelease: input.pairedRelease,
    sourceCommit: input.sourceCommit,
    sourceGeneration: RUNTIME_GENERATION_RUST,
    sourcePublicationSha256: input.sourcePublicationSha256,
    sourceRelease: input.sourceRelease,
    targetGeneration: RUNTIME_GENERATION_RUST,
  });
}

// read one bounded local request without following a final link
function readRequest(path) {
  const absolute = resolve(path);
  const details = lstatSync(absolute);
  // reject empty linked non-files and oversized requests
  if (
    !details.isFile()
    || details.isSymbolicLink()
    || details.size === 0
    || details.size > MAX_REQUEST_BYTES
  ) {
    fail("native publication request must be a bounded regular file");
  }
  return JSON.parse(readFileSync(absolute, "utf8"));
}

// run the pure stdout-only preparation interface
function main(argv) {
  // select the separate descendant preparation only with its exact explicit mode
  const family = argv.length === 2 && argv[0] === "--family";
  if (!family && argv.length !== 1) {
    fail("usage: prepare-native-schema-preserving-publication.mjs [--family] REQUEST.json");
  }
  const input = readRequest(argv[family ? 1 : 0]);
  const output = family ? prepareNativeFamilyPublication(input) : prepareNativeSchemaPreservingPublication(input);
  process.stdout.write(`${JSON.stringify(output, null, 2)}\n`);
}

// execute only when selected as the entry point
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`prepare-native-schema-preserving-publication: ${error.message}\n`);
    process.exitCode = 1;
  }
}
