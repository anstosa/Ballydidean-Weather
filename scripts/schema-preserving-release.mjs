#!/usr/bin/env node

import { createHash, randomBytes } from "node:crypto";
import {
  closeSync,
  constants as fsConstants,
  fchmodSync,
  fstatSync,
  fsyncSync,
  linkSync,
  lstatSync,
  openSync,
  readFileSync,
  realpathSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, isAbsolute, resolve } from "node:path";
import { pathToFileURL } from "node:url";

export const CONTRACT_VERSION = "weather-schema-preserving-publication/v2";
export const LEGACY_CONTRACT_VERSION = "weather-schema-preserving-publication/v1";
export const NATIVE_PUBLICATION_CONTRACT_VERSION = "weather-schema-preserving-publication/v3";
export const NATIVE_FAMILY_PUBLICATION_CONTRACT_VERSION = "weather-schema-preserving-family-publication/v1";
export const NATIVE_DESCENDANT_PUBLICATION_CONTRACT_VERSION =
  "weather-schema-preserving-native-descendant-publication/v1";
export const NATIVE_DASHBOARD_PUBLICATION_CONTRACT_VERSION =
  "weather-native-dashboard-publication/v1";
export const NATIVE_BOOTSTRAP_APPROVAL_VERSION = "weather-predecessor-rust-transition-approval/v1";
export const NATIVE_BOOTSTRAP_PROOF_VERSION = "weather-predecessor-native-bootstrap-proof/v1";
const NATIVE_STAGE_GATE = "requires-live-retained-installation-receipt-verification";
// authorize only the reviewed AQI application query bytes without widening schema authority
export const APP_QUERY_TRANSITION = Object.freeze({
  path: "packages/database/src/repositories.ts",
  sourceSha256: "073b2308f7deb663a0bdefefc75930dedaa1b4abc6bc40b4fda04524b1cb600c",
  targetSha256: "2715e7641fde21303ffcbf47a0cdf0157cc9f32f11211b8a66bad11f1e7aee59",
});
export const RUNTIME_GENERATION_CONTRACT_VERSION = "weather-runtime-generation/v1";
export const RUNTIME_GENERATION_NODE = "node-v1";
export const RUNTIME_GENERATION_RUST = "rust-v1";
export const RUNTIME_GENERATION_LABEL = "org.weather.runtime.generation";
export const STAGE_CONTRACT_VERSION = "weather-schema-preserving-stage/v1";
export const RUNTIME_STAGE_CONTRACT_VERSION = "weather-schema-preserving-stage/v2";
export const SOURCE_RELEASE = "2026.11.03-3";
export const SOURCE_COMMIT = "7e045d5bc69969e9247440ce3b70744e1f9ef6c4";
export const SOURCE_ENV_SHA256 = "7727f844ca07fdd7fbd7a717976628d5e4f860285d5fe527e990f13632eea72b";
export const CONTROL_VERSION = 17;
export const CONTROL_SHA256 = "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb";
export const UPDATE_SHA256 = "ea1185e04e0da2439c856b5b6989ce4f87f087a63ff7bafa1781c577cf42b8a0";
export const HISTORY_SHA256 = "1cfd6251eeb66d443e36d661299c9e49e73465fab55e5dab2cfa4ee86dd36ad2";
export const sourceRelease = SOURCE_RELEASE;
export const sourceCommit = SOURCE_COMMIT;
export const sourceEnvSha = SOURCE_ENV_SHA256;
export const controlVersion = CONTROL_VERSION;
export const controlSha = CONTROL_SHA256;
export const updateSha = UPDATE_SHA256;
export const historySha = HISTORY_SHA256;

const MAX_JSON_BYTES = 1024 * 1024;
const SHA256 = /^[a-f0-9]{64}$/u;
const IMAGE_ID = /^sha256:[a-f0-9]{64}$/u;
const COMMIT_SHA = /^[a-f0-9]{40}$/u;
const RELEASE_NAME = /^(\d{4})\.(\d{2})\.(\d{2})-([1-9]\d?)$/u;
const FIELD_KEY = /^[A-Za-z][A-Za-z0-9]{0,63}$/u;
const SAFE_FIELD_STRING = /^[^\u0000-\u001f\u007f]{1,4096}$/u;
const SAFE_PATH = /^[\x20-\x7e]{1,512}$/u;
const SERVER_IMAGE = /^ghcr\.io\/anstosa\/weather-server@sha256:[a-f0-9]{64}$/u;
const WEB_IMAGE = /^ghcr\.io\/anstosa\/weather-web@sha256:[a-f0-9]{64}$/u;
const ACTION_RUN_URL = /^https:\/\/github\.com\/anstosa\/Ballydidean-Weather\/actions\/runs\/([1-9]\d*)$/u;
const PROTECTED_PREFIXES = [
  ".github/workflows/",
  "config/",
  "deploy/",
  "packages/database/",
  "packages/forecast-adjustment/",
];
const PROTECTED_FILES = new Set([
  ".dockerignore",
  "Dockerfile",
  "package-lock.json",
  "package.json",
]);
const REQUIRED_TREE_PATHS = [
  "Dockerfile",
  "deploy/scripts/common.sh",
  "deploy/scripts/update.sh",
  "package-lock.json",
  "packages/database/migrations/0020_adjustment_revision_frontier.sql",
  "packages/database/migrations/0023_forecast_product_read_index.sql",
  "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
];
const PUBLICATION_KEYS = [
  "check",
  "contractVersion",
  "operatorSha256",
  "publish",
  "serverImage",
  "serverImageId",
  "sourceTree",
  "targetCommit",
  "targetRelease",
  "targetTree",
  "validatorSha256",
  "webImage",
  "webImageId",
];
const STAGE_KEYS = [
  "contractVersion",
  "protectedSha256",
  "publicationSha256",
  "sourceEnvironmentSha256",
  "sourceRelease",
  "targetEnvironmentSha256",
  "targetRelease",
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
const TREE_MEMBER_KEYS = ["path", "sha256"];

// reject malformed contract values
function fail(message) {
  throw new Error(message);
}

// require a plain object
function requireObject(value, label) {
  // reject non-objects
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

// enforce a closed object shape
function requireExactKeys(value, expected, label) {
  requireObject(value, label);
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  // reject shape drift
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    fail(`${label} has unexpected fields`);
  }
}

// validate a calendar date
export function validateDate(value) {
  // require fixed syntax
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/u.test(value)) {
    fail("date must use YYYY-MM-DD");
  }
  const [year, month, day] = value.split("-").map(Number);
  const date = new Date(Date.UTC(year, month - 1, day));
  // reject calendar rollover
  if (
    date.getUTCFullYear() !== year
    || date.getUTCMonth() !== month - 1
    || date.getUTCDate() !== day
  ) {
    fail("date must be a real calendar date");
  }
  return value;
}

// validate an immutable release name
export function validateReleaseName(value) {
  const match = typeof value === "string" ? RELEASE_NAME.exec(value) : null;
  // require fixed release syntax
  if (match === null) {
    fail("release must use YYYY.MM.DD-N");
  }
  validateDate(`${match[1]}-${match[2]}-${match[3]}`);
  return value;
}

// compare ordered release names
function compareReleases(left, right) {
  const leftMatch = RELEASE_NAME.exec(validateReleaseName(left));
  const rightMatch = RELEASE_NAME.exec(validateReleaseName(right));
  // compare release components
  for (let index = 1; index <= 4; index += 1) {
    const difference = Number(leftMatch[index]) - Number(rightMatch[index]);
    // stop at first difference
    if (difference !== 0) {
      return difference;
    }
  }
  return 0;
}

// recognize protected repository paths
function isProtectedPath(value) {
  // accept exact root files
  if (PROTECTED_FILES.has(value)) {
    return true;
  }
  return PROTECTED_PREFIXES.some((prefix) => value.startsWith(prefix));
}

// validate one protected tree
function validateTree(tree, label) {
  // bound tree membership
  if (!Array.isArray(tree) || tree.length === 0 || tree.length > 10000) {
    fail(`${label} must be a nonempty bounded array`);
  }
  let previous = "";
  const paths = new Set();
  // inspect every member
  for (const [index, member] of tree.entries()) {
    requireExactKeys(member, TREE_MEMBER_KEYS, `${label}[${index}]`);
    // reject unsafe paths
    if (
      typeof member.path !== "string"
      || !SAFE_PATH.test(member.path)
      || member.path.startsWith("/")
      || member.path.includes("\\")
      || member.path.split("/").some((part) => part === "" || part === "." || part === "..")
      || !isProtectedPath(member.path)
    ) {
      fail(`${label}[${index}].path is not a canonical protected path`);
    }
    // enforce canonical order
    if (previous !== "" && previous >= member.path) {
      fail(`${label} must be sorted with unique paths`);
    }
    // require content identity
    if (typeof member.sha256 !== "string" || !SHA256.test(member.sha256)) {
      fail(`${label}[${index}].sha256 is invalid`);
    }
    previous = member.path;
    paths.add(member.path);
  }
  // require safety sentinels
  for (const requiredPath of REQUIRED_TREE_PATHS) {
    // reject partial trees
    if (!paths.has(requiredPath)) {
      fail(`${label} is missing required path ${requiredPath}`);
    }
  }
  return tree;
}

// preserve protected bytes except one content-addressed application query transition
export function validateProtectedTrees(sourceTree, targetTree, contractVersion = CONTRACT_VERSION) {
  // keep retained legacy publications strictly byte-identical
  if (contractVersion !== CONTRACT_VERSION && contractVersion !== LEGACY_CONTRACT_VERSION) {
    fail("contractVersion is invalid");
  }
  validateTree(sourceTree, "sourceTree");
  validateTree(targetTree, "targetTree");
  // reject membership drift
  if (sourceTree.length !== targetTree.length) {
    fail("protected tree length drifted");
  }
  // compare exact members
  for (let index = 0; index < sourceTree.length; index += 1) {
    const source = sourceTree[index];
    const target = targetTree[index];
    const reviewedQuery = contractVersion === CONTRACT_VERSION
      && source.path === APP_QUERY_TRANSITION.path
      && target.path === APP_QUERY_TRANSITION.path
      && source.sha256 === APP_QUERY_TRANSITION.sourceSha256
      && target.sha256 === APP_QUERY_TRANSITION.targetSha256;
    // reject membership drift and every unreviewed byte change
    if (
      sourceTree[index].path !== targetTree[index].path
      || (sourceTree[index].sha256 !== targetTree[index].sha256 && !reviewedQuery)
    ) {
      fail(`protected tree drifted at ${sourceTree[index].path}`);
    }
  }
  return true;
}

// validate one workflow record
function validateWorkflow(value, expected, targetCommit, targetRelease) {
  requireExactKeys(value, expected.keys, expected.label);
  // require numeric identity
  if (!Number.isSafeInteger(value.databaseId) || value.databaseId <= 0) {
    fail(`${expected.label}.databaseId is invalid`);
  }
  // require exact workflow
  if (value.name !== expected.name) {
    fail(`${expected.label}.name is invalid`);
  }
  // bind the target commit
  if (value.headSha !== targetCommit) {
    fail(`${expected.label}.headSha does not match targetCommit`);
  }
  // require successful push
  if (value.status !== "completed" || value.conclusion !== "success" || value.event !== "push") {
    fail(`${expected.label} must be a successful completed push`);
  }
  const urlMatch = typeof value.url === "string" ? ACTION_RUN_URL.exec(value.url) : null;
  // bind canonical run url
  if (urlMatch === null || Number(urlMatch[1]) !== value.databaseId) {
    fail(`${expected.label}.url is invalid`);
  }
  // bind the release tag
  if (expected.label === "publish" && value.headBranch !== targetRelease) {
    fail("publish.headBranch does not match targetRelease");
  }
}

// validate the publication proof
export function validatePublication(value, expectedTargetRelease) {
  // retain an ordinary native descendant with exact protected-tree equality
  if (value?.contractVersion === NATIVE_DESCENDANT_PUBLICATION_CONTRACT_VERSION) {
    return validateNativeDescendantPublication(value, expectedTargetRelease);
  }
  // retain an independent closed family-only descendant receipt
  if (value?.contractVersion === NATIVE_FAMILY_PUBLICATION_CONTRACT_VERSION) {
    return validateNativeFamilyPublication(value, expectedTargetRelease);
  }
  // keep the finite native transition separate from the historic equality contracts
  if (value?.contractVersion === NATIVE_PUBLICATION_CONTRACT_VERSION) {
    return validateNativePublication(value, expectedTargetRelease);
  }
  requireExactKeys(value, PUBLICATION_KEYS, "publication");
  // bind the contract version
  if (value.contractVersion !== CONTRACT_VERSION && value.contractVersion !== LEGACY_CONTRACT_VERSION) {
    fail("contractVersion is invalid");
  }
  validateReleaseName(value.targetRelease);
  // prevent anchor reuse
  if (compareReleases(value.targetRelease, SOURCE_RELEASE) <= 0) {
    fail("targetRelease must follow sourceRelease");
  }
  // bind caller intent
  if (expectedTargetRelease !== undefined) {
    validateReleaseName(expectedTargetRelease);
    // require the requested tag
    if (value.targetRelease !== expectedTargetRelease) {
      fail("targetRelease does not match the requested release");
    }
  }
  // require a new exact commit
  if (!COMMIT_SHA.test(value.targetCommit ?? "") || value.targetCommit === SOURCE_COMMIT) {
    fail("targetCommit is invalid");
  }
  // bind the server image
  if (!SERVER_IMAGE.test(value.serverImage ?? "") || !IMAGE_ID.test(value.serverImageId ?? "")) {
    fail("server image identity is invalid");
  }
  // bind the web image
  if (!WEB_IMAGE.test(value.webImage ?? "") || !IMAGE_ID.test(value.webImageId ?? "")) {
    fail("web image identity is invalid");
  }
  // reject reused identities
  if (value.serverImage === value.webImage || value.serverImageId === value.webImageId) {
    fail("server and web image identities must differ");
  }
  // bind the operator bundle
  if (!SHA256.test(value.operatorSha256 ?? "") || !SHA256.test(value.validatorSha256 ?? "")) {
    fail("bundle sha256 is invalid");
  }
  validateWorkflow(value.check, {
    keys: CHECK_KEYS,
    label: "check",
    name: "Check",
  }, value.targetCommit, value.targetRelease);
  validateWorkflow(value.publish, {
    keys: PUBLISH_KEYS,
    label: "publish",
    name: "Publish production images",
  }, value.targetCommit, value.targetRelease);
  // require distinct workflow runs
  if (value.check.databaseId === value.publish.databaseId) {
    fail("check and publish runs must differ");
  }
  validateProtectedTrees(value.sourceTree, value.targetTree, value.contractVersion);
  return value;
}

// hash canonical sorted JSON consistently with the offline transition preparer
export function nativeCanonicalSha256(value) {
  // sort object keys while preserving ordered finite member lists
  const canonical = (member) => {
    if (Array.isArray(member)) {
      return member.map(canonical);
    }
    if (member !== null && typeof member === "object") {
      return Object.fromEntries(Object.keys(member).sort().map((key) => [key, canonical(member[key])]));
    }
    return member;
  };
  return createHash("sha256").update(`${JSON.stringify(canonical(value))}\n`).digest("hex");
}

// derive the exact installed two-file target from finite target tree bytes
function nativeTargetBundle(transition) {
  const target = new Map(transition.targetMembers.map((member) => [member.path, member.sha256]));
  const members = ["schema-preserving-release.mjs", "schema-preserving-release.sh"].map((path) => {
    const sha256 = target.get(`scripts/${path}`);
    if (!SHA256.test(sha256 ?? "")) {
      fail("native target operator member missing");
    }
    return { path, sha256 };
  });
  const sha256sum = members.map(({ path, sha256 }) => `${sha256}  ${path}\n`).join("");
  return {
    contractVersion: "weather-operator-bundle-manifest/v1",
    members,
    sha256sum,
    manifestSha256: createHash("sha256").update(sha256sum).digest("hex"),
  };
}

// validate reviewed finite identity without treating review metadata as live authority
export function validateNativeBootstrapApproval(approval) {
  requireExactKeys(approval, [
    "approvalClass", "approvedTransitionSha256", "artifactVersion", "targetOperatorBundleSha256", "transition",
  ], "native bootstrap approval");
  if (approval.artifactVersion !== NATIVE_BOOTSTRAP_APPROVAL_VERSION
    || approval.approvalClass !== "operator-reviewed-finite-transition"
    || approval.transition?.contractVersion !== DECLARED_TRANSITION_CONTRACT_VERSION) {
    fail("native bootstrap approval class differs");
  }
  validatePredecessorTransition(approval.transition);
  const transition = approval.transition;
  const bundle = nativeTargetBundle(transition);
  if (approval.approvedTransitionSha256 !== nativeCanonicalSha256(transition)
    || approval.targetOperatorBundleSha256 !== bundle.manifestSha256) {
    fail("native bootstrap approval identity differs");
  }
  return { transition, bundle };
}

// bind the publisher's extracted native material to the exact published image coordinates
function validateNativeImagePublication(value) {
  const proof = value.imagePublicationProof;
  requireExactKeys(proof, [
    "contractVersion", "generation", "imageMaterial", "imageMaterialSha256", "publishedImages", "release", "sourceCommit",
  ], "native image publication proof");
  const material = proof.imageMaterial;
  requireExactKeys(material, [
    "artifactClass", "attestsBuiltImage", "boundary", "build", "contractVersion", "images", "inspection", "platform", "source",
  ], "native image material");
  requireExactKeys(material.platform, ["architecture", "os"], "native image platform");
  requireExactKeys(material.inspection, [
    "extractedFileBytes", "provesElfArchitecture", "provesBuildIdentity", "provesImageIdentity",
    "provesPlatform", "provesSourceIdentity",
  ], "native image inspection");
  if (proof.contractVersion !== "weather-native-image-publication-proof/v1" || proof.generation !== RUNTIME_GENERATION_RUST
    || proof.release !== value.targetRelease || proof.sourceCommit !== value.targetCommit
    || material.contractVersion !== "weather-rust-runtime-image-material/v1"
    || material.artifactClass !== "offline-extracted-image-material-inspection" || material.attestsBuiltImage !== false
    || material.source?.commit !== value.targetCommit || material.platform.os !== "linux" || material.platform.architecture !== "arm64"
    || material.build?.profile !== "release" || material.build?.target !== "aarch64-unknown-linux-gnu"
    || material.inspection.extractedFileBytes !== true || material.inspection.provesElfArchitecture !== true
    || material.inspection.provesBuildIdentity !== false || material.inspection.provesImageIdentity !== false
    || material.inspection.provesPlatform !== false || material.inspection.provesSourceIdentity !== false
    || !Array.isArray(material.images) || material.images.length !== 2
    || !Array.isArray(proof.publishedImages) || proof.publishedImages.length !== 2
    || proof.imageMaterialSha256 !== nativeCanonicalSha256(material)
    || value.materialManifestSha256 !== proof.imageMaterialSha256
    || value.imagePublicationProofSha256 !== nativeCanonicalSha256(proof)) {
    fail("native image publication material differs");
  }
  // preserve the exact independent image-to-material associations
  for (const [index, name] of ["server", "web"].entries()) {
    const record = material.images[index];
    const image = proof.publishedImages[index];
    requireExactKeys(record, ["commands", "materialManifestSha256", "members", "name", "roles"], "native image material record");
    requireExactKeys(image, ["configId", "materialManifestSha256", "name", "reference"], "published image record");
    if (record.name !== name || image.name !== name
      || !SHA256.test(record.materialManifestSha256 ?? "")
      || image.materialManifestSha256 !== record.materialManifestSha256
      || image.reference !== value[`${name}Image`] || image.configId !== value[`${name}ImageId`]) {
      fail("native published image coordinates differ");
    }
  }
}

// preserve every protected snapshot member except the independently verified family catalog replacement
export function validateNativeFamilyRecoverySnapshot(previous, current, catalogPath, expectedCatalogSha256) {
  requireExactKeys(previous, ["files", "infrastructure"], "previous protected snapshot");
  requireExactKeys(current, ["files", "infrastructure"], "current protected snapshot");
  // require exact snapshot members and the unchanged retained database and infrastructure
  if (previous.infrastructure !== current.infrastructure
    || typeof catalogPath !== "string"
    || !catalogPath.endsWith("/adjustment-candidate-catalog.json")
    || !SHA256.test(expectedCatalogSha256)
    || !Array.isArray(previous.files) || previous.files.length !== 5
    || !Array.isArray(current.files) || current.files.length !== previous.files.length) {
    fail("family recovery protected snapshot differs");
  }
  let catalogCount = 0;
  // compare every fixed protected file and permit one atomic catalog replacement only
  for (const [index, old] of previous.files.entries()) {
    const actual = current.files[index];
    requireExactKeys(old, ["inode", "mode", "owner", "path", "sha256"], "previous protected member");
    requireExactKeys(actual, ["inode", "mode", "owner", "path", "sha256"], "current protected member");
    if (actual.path !== old.path || actual.owner !== old.owner || actual.mode !== old.mode) {
      fail("family recovery protected member differs");
    }
    // only the independently authenticated catalog bytes and its atomic replacement inode may vary
    if (old.path === catalogPath) {
      catalogCount += 1;
      if (actual.sha256 !== expectedCatalogSha256) {
        fail("family recovery catalog identity differs");
      }
    } else if (actual.inode !== old.inode || actual.sha256 !== old.sha256) {
      fail("family recovery retained state differs");
    }
  }
  if (catalogCount !== 1) {
    fail("family recovery catalog member differs");
  }
  return true;
}

// validate exact native publication without self-authorizing staging
export function validateNativePublication(value, expectedTargetRelease) {
  requireExactKeys(value, [
    "approvedTransitionSha256", "bootstrapApproval", "check", "contractVersion",
    "eligibleForLiveStage", "imagePublicationProof", "imagePublicationProofSha256", "liveAuthorityEstablished", "materialManifestSha256", "operatorBundle",
    "operatorSha256", "publish", "requiredInstallationReceiptSha256", "serverImage", "serverImageId",
    "sourceGeneration", "sourceRelease", "stageGate", "targetCommit", "targetGeneration", "targetRelease",
    "transition", "transitionSha256", "validatorSha256", "webImage", "webImageId",
  ], "native publication");
  requireExactKeys(value.bootstrapApproval, ["approvalClass", "approvedTransitionSha256", "artifactVersion", "targetOperatorBundleSha256"], "bootstrap approval header");
  const { transition, bundle } = validateNativeBootstrapApproval({
    ...value.bootstrapApproval, transition: value.transition,
  });
  if (value.contractVersion !== NATIVE_PUBLICATION_CONTRACT_VERSION
    || value.eligibleForLiveStage !== false || value.liveAuthorityEstablished !== false
    || value.stageGate !== NATIVE_STAGE_GATE
    || value.sourceGeneration !== RUNTIME_GENERATION_NODE || value.targetGeneration !== RUNTIME_GENERATION_RUST
    || value.sourceRelease !== transition.predecessor.release || value.targetRelease !== transition.target.release
    || value.targetCommit !== transition.target.commit
    || value.requiredInstallationReceiptSha256 !== transition.predecessor.installationReceiptSha256
    || value.transitionSha256 !== nativeCanonicalSha256(transition) || value.approvedTransitionSha256 !== value.transitionSha256
    || nativeCanonicalSha256(value.operatorBundle) !== nativeCanonicalSha256(bundle)
    || value.validatorSha256 !== bundle.members[0].sha256 || value.operatorSha256 !== bundle.members[1].sha256
    || !SHA256.test(value.materialManifestSha256 ?? "")
    || !SERVER_IMAGE.test(value.serverImage ?? "") || !WEB_IMAGE.test(value.webImage ?? "")
    || !IMAGE_ID.test(value.serverImageId ?? "") || !IMAGE_ID.test(value.webImageId ?? "")
    || value.serverImageId === value.webImageId
    || (expectedTargetRelease !== undefined && value.targetRelease !== expectedTargetRelease)) {
    fail("native publication identity differs");
  }
  validateNativeImagePublication(value);
  validateWorkflow(value.check, { keys: CHECK_KEYS, label: "check", name: "Check" },
    value.targetCommit, value.targetRelease);
  validateWorkflow(value.publish, { keys: PUBLISH_KEYS, label: "publish", name: "Publish production images" },
    value.targetCommit, value.targetRelease);
  if (value.check.databaseId === value.publish.databaseId) {
    fail("native workflow runs must differ");
  }
  return value;
}

// admit only an immutable Rust descendant designated for the separate family transaction
export function validateNativeFamilyPublication(value, expectedTargetRelease) {
  requireExactKeys(value, [
    "check", "contractVersion", "family", "familyActionSha256",
    "familyRole", "imagePublicationProof", "imagePublicationProofSha256",
    "materialManifestSha256", "operatorSha256", "pairedRelease",
    "publish", "serverImage", "serverImageId", "sourceCommit",
    "sourceGeneration", "sourcePublicationSha256", "sourceRelease",
    "targetCommit", "targetGeneration", "targetRelease", "validatorSha256",
    "webImage", "webImageId",
  ], "native family publication");
  // require the exact original source and two distinct future native release identities
  if (value.contractVersion !== NATIVE_FAMILY_PUBLICATION_CONTRACT_VERSION
    || value.sourceGeneration !== RUNTIME_GENERATION_RUST
    || value.targetGeneration !== RUNTIME_GENERATION_RUST
    || !["temperature", "wind", "rain"].includes(value.family)
    || !["target", "compensation"].includes(value.familyRole)
    || !SHA256.test(value.familyActionSha256 ?? "")
    || !SHA256.test(value.sourcePublicationSha256 ?? "")
    || !SHA256.test(value.operatorSha256 ?? "")
    || !SHA256.test(value.validatorSha256 ?? "")
    || !SHA256.test(value.materialManifestSha256 ?? "")
    || !COMMIT_SHA.test(value.sourceCommit ?? "")
    || !COMMIT_SHA.test(value.targetCommit ?? "")
    || value.sourceCommit === value.targetCommit
    || compareReleases(value.sourceRelease, value.targetRelease) >= 0
    || compareReleases(value.sourceRelease, value.pairedRelease) >= 0
    || value.targetRelease === value.pairedRelease
    || (value.familyRole === "target" && compareReleases(value.targetRelease, value.pairedRelease) >= 0)
    || (value.familyRole === "compensation" && compareReleases(value.targetRelease, value.pairedRelease) <= 0)
    || (expectedTargetRelease !== undefined && value.targetRelease !== expectedTargetRelease)
    || !SERVER_IMAGE.test(value.serverImage ?? "") || !WEB_IMAGE.test(value.webImage ?? "")
    || !IMAGE_ID.test(value.serverImageId ?? "") || !IMAGE_ID.test(value.webImageId ?? "")
    || value.serverImageId === value.webImageId) {
    fail("native family publication identity differs");
  }
  validateNativeImagePublication(value);
  validateWorkflow(value.check, { keys: CHECK_KEYS, label: "check", name: "Check" },
    value.targetCommit, value.targetRelease);
  validateWorkflow(value.publish, { keys: PUBLISH_KEYS, label: "publish", name: "Publish production images" },
    value.targetCommit, value.targetRelease);
  // retain distinct workflow identities and never let a publication authorize activation by itself
  if (value.check.databaseId === value.publish.databaseId) {
    fail("native family workflow runs must differ");
  }
  return value;
}

// admit one ordinary Rust descendant without bootstrap or family mutation authority
export function validateNativeDescendantPublication(value, expectedTargetRelease) {
  requireExactKeys(value, [
    "check", "contractVersion", "imagePublicationProof", "imagePublicationProofSha256",
    "materialManifestSha256", "operatorSha256", "publish", "serverImage", "serverImageId",
    "sourceCommit", "sourceGeneration", "sourcePublicationSha256", "sourceRelease", "sourceTree",
    "targetCommit", "targetGeneration", "targetRelease", "targetTree", "validatorSha256",
    "webImage", "webImageId",
  ], "native descendant publication");
  // require a direct immutable Rust successor and complete image identities
  if (value.contractVersion !== NATIVE_DESCENDANT_PUBLICATION_CONTRACT_VERSION
    || value.sourceGeneration !== RUNTIME_GENERATION_RUST
    || value.targetGeneration !== RUNTIME_GENERATION_RUST
    || !SHA256.test(value.sourcePublicationSha256 ?? "")
    || !SHA256.test(value.operatorSha256 ?? "")
    || !SHA256.test(value.validatorSha256 ?? "")
    || !SHA256.test(value.materialManifestSha256 ?? "")
    || !COMMIT_SHA.test(value.sourceCommit ?? "")
    || !COMMIT_SHA.test(value.targetCommit ?? "")
    || value.sourceCommit === value.targetCommit
    || compareReleases(value.sourceRelease, value.targetRelease) >= 0
    || (expectedTargetRelease !== undefined && value.targetRelease !== expectedTargetRelease)
    || !SERVER_IMAGE.test(value.serverImage ?? "")
    || !WEB_IMAGE.test(value.webImage ?? "")
    || !IMAGE_ID.test(value.serverImageId ?? "")
    || !IMAGE_ID.test(value.webImageId ?? "")
    || value.serverImageId === value.webImageId) {
    fail("native descendant publication identity differs");
  }
  validateNativeImagePublication(value);
  validateWorkflow(value.check, { keys: CHECK_KEYS, label: "check", name: "Check" },
    value.targetCommit, value.targetRelease);
  validateWorkflow(value.publish, { keys: PUBLISH_KEYS, label: "publish", name: "Publish production images" },
    value.targetCommit, value.targetRelease);
  // require independent workflow evidence and byte-identical protected repository membership
  if (value.check.databaseId === value.publish.databaseId) {
    fail("native descendant workflow runs must differ");
  }
  validateProtectedTrees(value.sourceTree, value.targetTree, LEGACY_CONTRACT_VERSION);
  return value;
}

// admit one web-only native successor without granting core activation authority
export function validateNativeDashboardPublication(
  value,
  expectedTargetRelease,
  expectedCoreRelease,
  expectedSourceRelease,
) {
  requireExactKeys(value, [
    "check", "contractVersion", "coreCommit", "corePublicationSha256", "coreRelease",
    "coreTree", "imagePublicationProof", "imagePublicationProofSha256",
    "materialManifestSha256", "publish", "sourceCommit", "sourcePublicationSha256",
    "sourceRelease", "sourceWebImage", "sourceWebImageId", "targetCommit", "targetRelease",
    "targetTree", "targetWebImage", "targetWebImageId",
  ], "native dashboard publication");
  // bind a strict core-to-source-to-target chain and two distinct web identities
  if (value.contractVersion !== NATIVE_DASHBOARD_PUBLICATION_CONTRACT_VERSION
    || !COMMIT_SHA.test(value.coreCommit ?? "") || !COMMIT_SHA.test(value.sourceCommit ?? "")
    || !COMMIT_SHA.test(value.targetCommit ?? "")
    || !SHA256.test(value.corePublicationSha256 ?? "")
    || !SHA256.test(value.sourcePublicationSha256 ?? "")
    || !SHA256.test(value.imagePublicationProofSha256 ?? "")
    || !SHA256.test(value.materialManifestSha256 ?? "")
    || !SERVER_IMAGE.test(value.imagePublicationProof?.publishedImages?.[0]?.reference ?? "")
    || !IMAGE_ID.test(value.imagePublicationProof?.publishedImages?.[0]?.configId ?? "")
    || !WEB_IMAGE.test(value.sourceWebImage ?? "") || !IMAGE_ID.test(value.sourceWebImageId ?? "")
    || !WEB_IMAGE.test(value.targetWebImage ?? "") || !IMAGE_ID.test(value.targetWebImageId ?? "")
    || compareReleases(value.coreRelease, value.sourceRelease) > 0
    || compareReleases(value.sourceRelease, value.targetRelease) >= 0
    || value.sourceCommit === value.targetCommit
    || value.sourceWebImage === value.targetWebImage
    || value.sourceWebImageId === value.targetWebImageId
    || (value.sourceRelease === value.coreRelease
      && (value.sourceCommit !== value.coreCommit
        || value.sourcePublicationSha256 !== value.corePublicationSha256))
    || (expectedTargetRelease !== undefined && value.targetRelease !== expectedTargetRelease)
    || (expectedCoreRelease !== undefined && value.coreRelease !== expectedCoreRelease)
    || (expectedSourceRelease !== undefined && value.sourceRelease !== expectedSourceRelease)) {
    fail("native dashboard publication identity differs");
  }
  const publishedServer = value.imagePublicationProof?.publishedImages?.[0];
  // validate the complete independently inspected image proof while selecting only its web image
  validateNativeImagePublication({
    ...value,
    serverImage: publishedServer?.reference,
    serverImageId: publishedServer?.configId,
    webImage: value.targetWebImage,
    webImageId: value.targetWebImageId,
  });
  validateWorkflow(value.check, { keys: CHECK_KEYS, label: "check", name: "Check" },
    value.targetCommit, value.targetRelease);
  validateWorkflow(value.publish, { keys: PUBLISH_KEYS, label: "publish", name: "Publish production images" },
    value.targetCommit, value.targetRelease);
  // retain independent workflow evidence and byte-identical protected core membership
  if (value.check.databaseId === value.publish.databaseId) {
    fail("native dashboard workflow runs must differ");
  }
  validateProtectedTrees(value.coreTree, value.targetTree, LEGACY_CONTRACT_VERSION);
  return value;
}

// admit the measured six literal roles without substituting server or compensation images
export function validateNativeDashboardCapacity(value, publication, core, server, sourceWeb, targetWeb) {
  validateReleaseName(core);
  if (!SHA256.test(publication ?? "") || !SERVER_IMAGE.test(server ?? "") ||
      !WEB_IMAGE.test(sourceWeb ?? "") || !WEB_IMAGE.test(targetWeb ?? "") || sourceWeb === targetWeb) {
    fail("native dashboard capacity coordinates differ");
  }
  requireExactKeys(value, [
    "publicationSha256", "availableFreeBytes", "captureReservedBytes", "compatibilityFixtureBytes",
    "compatibilityFixtureInodes", "compensationScope", "compressedPeakBytes", "contractVersion",
    "futureStateBytes", "imageDigests", "measuredAt", "newLayerInodes", "persistentGrowthBytes",
    "persistentOwnedBytes", "protectedFreeBytes", "pullPeakGrowthBytes", "releaseTransactionPeakGrowthBytes",
    "reasons", "requiredFreeBytes", "requiredFreeInodes", "retainedControlBytes", "retirementCreditBytes",
    "sourceRelease", "state",
  ], "native dashboard capacity");
  // retain the source formula's ready decision and measure time without future retirement credit
  const measured = Date.parse(value.measuredAt);
  if (value.contractVersion !== "dashboard-web-release-capacity/v1" ||
      value.compensationScope !== "fixed-core-web-only-source-restore" || value.publicationSha256 !== publication ||
      value.sourceRelease !== core || value.state !== "capacity_ready" || value.retirementCreditBytes !== 0 ||
      !Array.isArray(value.reasons) || value.reasons.length !== 0 || !Number.isFinite(measured) ||
      new Date(measured).toISOString() !== value.measuredAt) {
    fail("native dashboard capacity decision differs");
  }
  // verify all retained numeric capacity quantities remain safe and nonnegative
  for (const [key, amount] of Object.entries(value)) {
    if (/(?:Bytes|Inodes)$/u.test(key) && (!Number.isSafeInteger(amount) || amount < 0)) {
      fail(`native dashboard capacity ${key} differs`);
    }
  }
  const expected = [
    ["source", "server", server], ["source", "web", sourceWeb],
    ["target", "server", server], ["target", "web", targetWeb],
    ["compensating", "server", server], ["compensating", "web", sourceWeb],
  ];
  if (!Array.isArray(value.imageDigests) || value.imageDigests.length !== expected.length) {
    fail("native dashboard capacity image roles differ");
  }
  // retain the exact ordered image identity for every physical role
  for (const [index, [role, runtime, image]] of expected.entries()) {
    const observed = value.imageDigests[index];
    requireExactKeys(observed, ["digest", "role", "runtime"], "native dashboard capacity image");
    if (observed.role !== role || observed.runtime !== runtime || observed.digest !== image.split("@")[1]) {
      fail("native dashboard capacity image identity differs");
    }
  }
  return value;
}

// bind an immutable proof generated only after successful live predecessor recovery
export function validateNativeBootstrapProof(proof, publication, installationSha256, bundleSha256) {
  validateNativePublication(publication);
  const predecessor = publication.transition.predecessor;
  requireExactKeys(proof, [
    "contractVersion", "transitionSha256", "approvalSha256", "sourceRelease", "sourceCommit",
    "sourceEnvironmentSha256", "predecessorBundleSha256", "predecessorInstallationReceiptSha256",
    "predecessorDropInSha256", "controlSha256", "targetRelease", "targetCommit", "targetBundleSha256",
    "targetInstallationReceiptSha256", "protectedSnapshotSha256",
  ], "native bootstrap proof");
  if (proof.contractVersion !== NATIVE_BOOTSTRAP_PROOF_VERSION
    || proof.transitionSha256 !== publication.transitionSha256
    || proof.sourceRelease !== predecessor.release || proof.sourceCommit !== predecessor.commit
    || proof.sourceEnvironmentSha256 !== predecessor.sourceEnvironmentSha256
    || proof.predecessorBundleSha256 !== predecessor.bundleSha256
    || proof.predecessorInstallationReceiptSha256 !== predecessor.installationReceiptSha256
    || proof.predecessorDropInSha256 !== predecessor.recoveryDropInSha256
    || proof.controlSha256 !== predecessor.controlSha256
    || proof.targetRelease !== publication.targetRelease || proof.targetCommit !== publication.targetCommit
    || proof.targetBundleSha256 !== publication.operatorBundle.manifestSha256
    || proof.targetBundleSha256 !== bundleSha256
    || proof.targetInstallationReceiptSha256 !== installationSha256
    || proof.approvalSha256 !== nativeCanonicalSha256({ ...publication.bootstrapApproval, transition: publication.transition })
    || !SHA256.test(proof.protectedSnapshotSha256 ?? "")) {
    fail("native bootstrap proof differs");
  }
  return proof;
}

// validate one immutable runtime generation bound to the exact publication and digests
export function validateRuntimeGeneration(value, expected) {
  requireExactKeys(value, [
    "contractVersion", "generation", "materialManifestSha256", "publicationSha256",
    "release", "serverImage", "serverImageId", "webImage", "webImageId",
  ], "runtime generation");
  if (value.contractVersion !== RUNTIME_GENERATION_CONTRACT_VERSION
    || value.release !== validateReleaseName(expected.release)
    || value.publicationSha256 !== expected.publicationSha256
    || !SHA256.test(value.publicationSha256)
    || value.serverImage !== expected.serverImage || !SERVER_IMAGE.test(value.serverImage)
    || value.webImage !== expected.webImage || !WEB_IMAGE.test(value.webImage)
    || value.serverImageId !== expected.serverImageId || !IMAGE_ID.test(value.serverImageId)
    || value.webImageId !== expected.webImageId || !IMAGE_ID.test(value.webImageId)) {
    fail("runtime generation identity differs");
  }
  // Rust requires independently inspected native image material; Node retains the original proof
  if ((value.generation === RUNTIME_GENERATION_RUST && !SHA256.test(value.materialManifestSha256 ?? ""))
    || (value.generation === RUNTIME_GENERATION_NODE && value.materialManifestSha256 !== null)
    || ![RUNTIME_GENERATION_NODE, RUNTIME_GENERATION_RUST].includes(value.generation)) {
    fail("runtime generation material differs");
  }
  return value;
}

// use an explicit generation record, never derive release authority from an image label alone
export function validateRuntimeImageMarkers(generation, serverMarker, webMarker) {
  // old immutable Node images predate the optional generation marker
  const nodeMarker = (marker) => marker === "" || marker === "<no value>" || marker === RUNTIME_GENERATION_NODE;
  if (generation === RUNTIME_GENERATION_NODE && nodeMarker(serverMarker) && nodeMarker(webMarker)) {
    return generation;
  }
  if (generation === RUNTIME_GENERATION_RUST
    && serverMarker === RUNTIME_GENERATION_RUST && webMarker === RUNTIME_GENERATION_RUST) {
    return generation;
  }
  fail("runtime image generation differs");
}

// render only the closed native command overlay; the original Node Compose remains byte-identical
export function runtimeGenerationComposeOverride(generation) {
  if (generation === RUNTIME_GENERATION_NODE) {
    return null;
  }
  if (generation !== RUNTIME_GENERATION_RUST) {
    fail("runtime generation is invalid");
  }
  return {
    services: {
      migration: { command: ["/opt/weather/bin/weather-migrate"] },
      api: {
        command: ["/opt/weather/bin/weather-api"],
        healthcheck: { test: ["CMD", "/opt/weather/bin/weather-api-health", "api"] },
      },
      worker: {
        command: ["/opt/weather/bin/weather-worker"],
        healthcheck: { test: ["CMD", "/opt/weather/bin/weather-worker-health"] },
      },
      web: {
        command: ["/opt/weather/bin/weather-web"],
        environment: { WEATHER_SITE_CONFIG_PATH: "/opt/weather/edge-config/ballydidean.json" },
        healthcheck: { test: ["CMD", "/opt/weather/bin/weather-web-health", "web"] },
      },
    },
  };
}

// validate one immutable stage receipt
export function validateStageReceipt(value, expectedTargetRelease) {
  const runtimeStage = value?.contractVersion === RUNTIME_STAGE_CONTRACT_VERSION;
  requireExactKeys(value, runtimeStage ? [...STAGE_KEYS, "runtimeMetadataSha256"] : STAGE_KEYS, "stage receipt");
  // bind the version without widening the predecessor receipt
  if (value.contractVersion !== STAGE_CONTRACT_VERSION && !runtimeStage) {
    fail("stage receipt contractVersion is invalid");
  }
  if (runtimeStage && !SHA256.test(value.runtimeMetadataSha256 ?? "")) {
    fail("stage receipt runtimeMetadataSha256 is invalid");
  }
  validateReleaseName(value.sourceRelease);
  validateReleaseName(value.targetRelease);
  validateReleaseName(expectedTargetRelease);
  // require one forward handoff
  if (value.sourceRelease === value.targetRelease) {
    fail("stage receipt sourceRelease and targetRelease must differ");
  }
  // bind caller intent
  if (value.targetRelease !== expectedTargetRelease) {
    fail("stage receipt targetRelease does not match the requested release");
  }
  // bind all staged bytes
  for (const key of [
    "sourceEnvironmentSha256",
    "targetEnvironmentSha256",
    "publicationSha256",
    "protectedSha256",
  ]) {
    // require an exact digest
    if (typeof value[key] !== "string" || !SHA256.test(value[key])) {
      fail(`stage receipt ${key} is invalid`);
    }
  }
  return value;
}

// validate a private receipt descriptor
function validatePrivateFile(path, details) {
  // require canonical storage
  if (!isAbsolute(path) || resolve(path) !== path || realpathSync(path) !== path) {
    fail("receipt path must be absolute and canonical");
  }
  // reject links and devices
  if (!details.isFile || details.nlink !== 1) {
    fail("receipt must be a single-link regular file");
  }
  // require private mode
  if ((details.mode & 0o777) !== 0o600) {
    fail("receipt mode must be 0600");
  }
  // require current ownership
  if (typeof process.geteuid === "function" && details.uid !== process.geteuid()) {
    fail("receipt owner must match the effective user");
  }
  // bound receipt bytes
  if (details.size <= 1 || details.size > MAX_JSON_BYTES) {
    fail("receipt size is out of bounds");
  }
}

// convert bigint file metadata
function numericFileDetails(details) {
  return {
    isFile: details.isFile(),
    mode: Number(details.mode),
    nlink: Number(details.nlink),
    size: Number(details.size),
    uid: Number(details.uid),
  };
}

// read bounded private json bytes
function readPrivateJsonReceipt(path) {
  const initial = lstatSync(path, { bigint: true });
  validatePrivateFile(path, numericFileDetails(initial));
  const fd = openSync(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
  // hold the verified descriptor
  try {
    const details = fstatSync(fd, { bigint: true });
    validatePrivateFile(path, numericFileDetails(details));
    // reject replacement races
    if (initial.dev !== details.dev || initial.ino !== details.ino) {
      fail("receipt changed while opening");
    }
    const bytes = readFileSync(fd);
    // reject size races
    if (bytes.length !== Number(details.size) || bytes.length > MAX_JSON_BYTES) {
      fail("receipt changed while reading");
    }
    const final = fstatSync(fd, { bigint: true });
    // reject mutation races
    if (
      final.dev !== details.dev
      || final.ino !== details.ino
      || final.size !== details.size
      || final.mtimeNs !== details.mtimeNs
      || final.ctimeNs !== details.ctimeNs
    ) {
      fail("receipt changed while reading");
    }
    let value;
    // normalize parse failures
    try {
      value = JSON.parse(bytes.toString("utf8"));
    } catch {
      fail("receipt must contain valid JSON");
    }
    requireObject(value, "receipt");
    return { bytes, value };
  } finally {
    // always close the descriptor
    closeSync(fd);
  }
}

// read a bounded private root object
export function readPrivateJson(path) {
  return readPrivateJsonReceipt(path).value;
}

// read bounded standard input
async function readBoundedStdin() {
  const chunks = [];
  let length = 0;
  // consume bounded chunks
  for await (const chunk of process.stdin) {
    const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    length += bytes.length;
    // stop before allocation growth
    if (length > MAX_JSON_BYTES) {
      fail("stdin JSON exceeds the size limit");
    }
    chunks.push(bytes);
  }
  // reject empty objects
  if (length <= 1) {
    fail("stdin JSON is empty");
  }
  return Buffer.concat(chunks, length);
}

// sync one directory mutation
function syncDirectory(path) {
  const fd = openSync(path, fsConstants.O_RDONLY | fsConstants.O_DIRECTORY);
  // close after sync
  try {
    fsyncSync(fd);
  } finally {
    // always close the directory
    closeSync(fd);
  }
}

// publish canonical json once
export function writeJsonCreateOnce(outputPath, value) {
  requireObject(value, "output JSON");
  // require canonical output
  if (!isAbsolute(outputPath) || resolve(outputPath) !== outputPath) {
    fail("output path must be absolute and canonical");
  }
  const parent = dirname(outputPath);
  const parentDetails = lstatSync(parent);
  // reject linked parents
  if (!parentDetails.isDirectory() || realpathSync(parent) !== parent) {
    fail("output parent must be a canonical directory");
  }
  // reject writable parents
  if ((parentDetails.mode & 0o022) !== 0) {
    fail("output parent must not be group or world writable");
  }
  // require current ownership
  if (typeof process.geteuid === "function" && parentDetails.uid !== process.geteuid()) {
    fail("output parent owner must match the effective user");
  }
  const payload = Buffer.from(`${JSON.stringify(value, null, 2)}\n`, "utf8");
  // bound published bytes
  if (payload.length > MAX_JSON_BYTES) {
    fail("output JSON exceeds the size limit");
  }
  const temporaryPath = `${parent}/.${basename(outputPath)}.${randomBytes(12).toString("hex")}.tmp`;
  let fd;
  // create and link once
  try {
    fd = openSync(
      temporaryPath,
      fsConstants.O_CREAT
        | fsConstants.O_EXCL
        | fsConstants.O_NOFOLLOW
        | fsConstants.O_WRONLY,
      0o600,
    );
    fchmodSync(fd, 0o600);
    writeFileSync(fd, payload);
    fsyncSync(fd);
    closeSync(fd);
    fd = undefined;
    linkSync(temporaryPath, outputPath);
    syncDirectory(parent);
    unlinkSync(temporaryPath);
    syncDirectory(parent);
  } catch (error) {
    // close interrupted writes
    if (fd !== undefined) {
      closeSync(fd);
    }
    // remove only our temporary
    try {
      unlinkSync(temporaryPath);
    } catch (cleanupError) {
      // preserve cleanup failures
      if (cleanupError.code !== "ENOENT") {
        throw new AggregateError([error, cleanupError], "failed to clean temporary JSON");
      }
    }
    throw error;
  }
  return outputPath;
}

// print one safe own scalar
function fieldValue(value, key) {
  // require an own safe key
  if (!FIELD_KEY.test(key ?? "") || !Object.hasOwn(value, key)) {
    fail("field key is invalid or absent");
  }
  const field = value[key];
  // emit safe strings
  if (typeof field === "string") {
    // reject terminal controls
    if (!SAFE_FIELD_STRING.test(field)) {
      fail("string field is unsafe");
    }
    return field;
  }
  // emit safe integers
  if (typeof field === "number" && Number.isSafeInteger(field)) {
    return String(field);
  }
  // emit booleans
  if (typeof field === "boolean") {
    return String(field);
  }
  fail("field must be a safe primitive");
}

// run the bounded command interface
async function main(argv) {
  const [command, ...args] = argv;
  // validate publication proofs
  if (command === "validate-publication" && args.length === 2) {
    const receipt = readPrivateJsonReceipt(args[0]);
    validatePublication(receipt.value, args[1]);
    process.stdout.write(receipt.bytes);
    return;
  }
  // validate one fixed web-only publication against all explicit release coordinates
  if (command === "validate-dashboard-publication" && args.length === 4) {
    const receipt = readPrivateJsonReceipt(args[0]);
    validateNativeDashboardPublication(receipt.value, args[1], args[2], args[3]);
    process.stdout.write(receipt.bytes);
    return;
  }
  // validate a ready native dashboard capacity receipt against its fixed roles
  if (command === "validate-dashboard-capacity" && args.length === 6) {
    const receipt = readPrivateJsonReceipt(args[0]);
    validateNativeDashboardCapacity(receipt.value, ...args.slice(1));
    process.stdout.write(receipt.bytes);
    return;
  }
  // emit the reviewed predecessor and target coordinates for the bootstrap operation
  if (command === "native-bootstrap-coordinates" && args.length === 1) {
    const { value } = readPrivateJsonReceipt(args[0]);
    const { transition, bundle } = validateNativeBootstrapApproval(value);
    const predecessor = transition.predecessor;
    process.stdout.write(`${[
      value.approvedTransitionSha256, predecessor.release, predecessor.commit, predecessor.sourceEnvironmentSha256,
      predecessor.controlSha256, predecessor.bundleSha256, predecessor.installationReceiptSha256,
      predecessor.recoveryDropInSha256, transition.target.release, transition.target.commit,
      bundle.manifestSha256, nativeCanonicalSha256(value),
    ].join("\t")}\n`);
    return;
  }
  // verify proof against the exact immutable publication and installed bundle
  if (command === "validate-native-bootstrap-proof" && args.length === 4) {
    const [proof, publication, installationSha256, bundleSha256] = args;
    validateNativeBootstrapProof(readPrivateJson(proof), readPrivateJson(publication), installationSha256, bundleSha256);
    return;
  }
  // validate stage receipts
  // compare the retained failed-switch snapshot to the authenticated compensation catalog state
  if (command === "validate-family-recovery-snapshot" && args.length === 4) {
    validateNativeFamilyRecoverySnapshot(
      readPrivateJson(args[0]), readPrivateJson(args[1]), args[2], args[3],
    );
    return;
  }
  if (command === "validate-stage" && args.length === 2) {
    const receipt = readPrivateJsonReceipt(args[0]);
    validateStageReceipt(receipt.value, args[1]);
    process.stdout.write(receipt.bytes);
    return;
  }
  // read one immutable generation record against the exact already admitted image coordinates
  if (command === "validate-runtime-generation" && args.length === 7) {
    const [file, release, publicationSha256, serverImage, serverImageId, webImage, webImageId] = args;
    const { value } = readPrivateJsonReceipt(file);
    const validated = validateRuntimeGeneration(value, {
      release, publicationSha256, serverImage, serverImageId, webImage, webImageId,
    });
    process.stdout.write(`${validated.generation}
`);
    return;
  }
  // validate the explicit generation against both actual image markers
  if (command === "validate-runtime-markers" && args.length === 3) {
    process.stdout.write(`${validateRuntimeImageMarkers(...args)}
`);
    return;
  }
  // emit the exact fixed native Compose override from the installed immutable operator
  if (command === "runtime-compose-override" && args.length === 1) {
    const override = runtimeGenerationComposeOverride(args[0]);
    if (override === null) {
      fail("Node generation must retain the unmodified Compose");
    }
    process.stdout.write(`${JSON.stringify(override)}
`);
    return;
  }
  // emit generic receipt fields
  if (command === "field" && args.length === 2) {
    const receipt = readPrivateJsonReceipt(args[0]);
    process.stdout.write(`${fieldValue(receipt.value, args[1])}\n`);
    return;
  }
  // publish generic receipts
  if (command === "write-json" && args.length === 1) {
    const bytes = await readBoundedStdin();
    let value;
    // parse one root object
    try {
      value = JSON.parse(bytes.toString("utf8"));
    } catch {
      fail("stdin must contain valid JSON");
    }
    writeJsonCreateOnce(args[0], value);
    return;
  }
  fail("usage: schema-preserving-release.mjs validate-publication FILE TARGET_RELEASE | validate-dashboard-publication FILE TARGET_RELEASE CORE_RELEASE SOURCE_RELEASE | validate-dashboard-capacity FILE PROOF_SHA CORE_RELEASE SERVER SOURCE_WEB TARGET_WEB | validate-stage FILE TARGET_RELEASE | field FILE KEY | write-json OUTPUT");
}

// finite predecessor-to-rust transition authority retained inside the installed validator bundle
export const TRANSITION_CONTRACT_VERSION = "weather-predecessor-rust-transition/v1";
export const DECLARED_TRANSITION_CONTRACT_VERSION = "weather-predecessor-rust-transition/v2";
export const REVIEWED_DECLARATION_CONTRACT_VERSION =
  "weather-predecessor-rust-reviewed-declaration/v1";
export const TRANSITION_PUBLICATION_LANE = "predecessor-control-bootstrap";
export const WP8B_TARGET_ADDED_FILES = Object.freeze([
  "config/backend-runtime-inventory.json",
  "rust/Cargo.lock",
  "rust/Cargo.toml",
  "rust/rust-toolchain.toml",
  "scripts/check-backend-runtime-inventory.mjs",
  "scripts/generate-rust-contracts.mjs",
]);
export const WP8B_TARGET_ONLY_ROOTS = Object.freeze([
  "packages/contracts",
  "rust/apps/weather-migrate",
  "rust/crates/weather-adjustment",
  "rust/crates/weather-contracts",
  "rust/crates/weather-database",
  "rust/crates/weather-domain",
]);

const TRANSITION_WILDCARD = /[*?\[\]{}]/u;
const TRANSITION_MAX_MEMBERS = 100000;
const TRANSITION_BROAD_ROOTS = new Set([".github", "config", "deploy", "packages", "rust", "scripts"]);
const TRANSITION_MEMBER_KEYS = ["path", "sha256"];
const TRANSITION_CHANGED_MEMBER_KEYS = ["path", "predecessorSha256", "targetSha256"];
const TRANSITION_TARGET_ROOT_KEYS = ["members", "root", "treeSha256"];
const TRANSITION_PREDECESSOR_KEYS = [
  "bundleSha256",
  "commit",
  "controlSha256",
  "installationReceiptSha256",
  "recoveryDropInSha256",
  "release",
  "sourceEnvironmentSha256",
];
const TRANSITION_TARGET_KEYS = ["commit", "release"];
const TRANSITION_DOCUMENT_KEYS = [
  "addedFiles",
  "changedMembers",
  "contractVersion",
  "predecessor",
  "predecessorMembers",
  "publicationLane",
  "target",
  "targetMembers",
  "targetOnlyRoots",
];
const DECLARED_TRANSITION_DOCUMENT_KEYS = [
  ...TRANSITION_DOCUMENT_KEYS,
  "reviewedDeclaration",
];
const REVIEWED_DECLARATION_KEYS = [
  "addedFiles",
  "changedMembers",
  "contractVersion",
  "declarationSha256",
  "predecessorCommit",
  "targetCommit",
];
const REVIEWED_DECLARATION_PAYLOAD_KEYS = REVIEWED_DECLARATION_KEYS.filter(
  (key) => key !== "declarationSha256",
);

// hash exact finite transition bytes
export function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// require one transition digest
function requireTransitionSha256(value, label) {
  // reject unpinned transition identities
  if (typeof value !== "string" || !SHA256.test(value)) {
    fail(`${label} must be a lowercase SHA-256 digest`);
  }
  return value;
}

// require one canonical finite transition path
function requireTransitionPath(value, label) {
  // reject unsafe paths wildcards and traversal
  if (typeof value !== "string"
    || !SAFE_PATH.test(value)
    || value.startsWith("/")
    || value.endsWith("/")
    || value.includes("\\")
    || TRANSITION_WILDCARD.test(value)
    || value.split("/").some((part) => part === "" || part === "." || part === "..")) {
    fail(`${label} must be a canonical finite path without wildcards`);
  }
  return value;
}

// validate one sorted finite member set
function validateTransitionMembers(value, label, allowEmpty = false) {
  // bound complete transition membership
  if (!Array.isArray(value)
    || value.length > TRANSITION_MAX_MEMBERS
    || (!allowEmpty && value.length === 0)) {
    fail(`${label} must be a bounded${allowEmpty ? "" : " nonempty"} array`);
  }
  let previous = "";
  // inspect every exact member
  for (const [index, member] of value.entries()) {
    requireExactKeys(member, TRANSITION_MEMBER_KEYS, `${label}[${index}]`);
    requireTransitionPath(member.path, `${label}[${index}].path`);
    requireTransitionSha256(member.sha256, `${label}[${index}].sha256`);
    // enforce canonical unique path order
    if (previous !== "" && previous >= member.path) {
      fail(`${label} must be sorted with unique paths`);
    }
    previous = member.path;
  }
  return value;
}

// encode one canonical final-newline transition member manifest
export function canonicalSha256sumManifest(members) {
  validateTransitionMembers(members, "manifest members");
  return members.map((member) => `${member.sha256}  ${member.path}\n`).join("");
}

// calculate one finite transition tree digest
export function treeSha256(members) {
  return sha256(Buffer.from(canonicalSha256sumManifest(members), "utf8"));
}

// validate one predecessor transition anchor
function validateTransitionPredecessor(value) {
  requireExactKeys(value, TRANSITION_PREDECESSOR_KEYS, "predecessor");
  validateReleaseName(value.release);
  // require one exact predecessor commit
  if (typeof value.commit !== "string" || !COMMIT_SHA.test(value.commit)) {
    fail("predecessor.commit must be a lowercase commit SHA");
  }
  // bind every predecessor evidence identity
  for (const key of TRANSITION_PREDECESSOR_KEYS.filter((key) => key.endsWith("Sha256"))) {
    requireTransitionSha256(value[key], `predecessor.${key}`);
  }
}

// validate one target transition anchor
function validateTransitionTarget(value, predecessor) {
  requireExactKeys(value, TRANSITION_TARGET_KEYS, "target");
  validateReleaseName(value.release);
  // require one different exact target commit
  if (typeof value.commit !== "string"
    || !COMMIT_SHA.test(value.commit)
    || value.commit === predecessor.commit) {
    fail("target.commit must be a different lowercase commit SHA");
  }
  // require a forward immutable release
  if (compareReleases(value.release, predecessor.release) <= 0) {
    fail("target.release must follow predecessor.release");
  }
}

// validate exact finite changed-member declarations
function validateTransitionChangedMembers(value) {
  // bound changed membership
  if (!Array.isArray(value) || value.length > TRANSITION_MAX_MEMBERS) {
    fail("changedMembers must be a bounded array");
  }
  let previous = "";
  // inspect each exact content transition
  for (const [index, member] of value.entries()) {
    requireExactKeys(member, TRANSITION_CHANGED_MEMBER_KEYS, `changedMembers[${index}]`);
    requireTransitionPath(member.path, `changedMembers[${index}].path`);
    requireTransitionSha256(member.predecessorSha256, `changedMembers[${index}].predecessorSha256`);
    requireTransitionSha256(member.targetSha256, `changedMembers[${index}].targetSha256`);
    // reject no-op duplicate and unsorted changes
    if (member.predecessorSha256 === member.targetSha256 || previous >= member.path) {
      fail("changedMembers must be sorted unique content transitions");
    }
    previous = member.path;
  }
  return value;
}

// encode the reviewed declaration independently of its content address
function reviewedDeclarationPayload(value) {
  return {
    addedFiles: structuredClone(value.addedFiles),
    changedMembers: structuredClone(value.changedMembers),
    contractVersion: value.contractVersion,
    predecessorCommit: value.predecessorCommit,
    targetCommit: value.targetCommit,
  };
}

// calculate one canonical reviewed declaration identity
export function reviewedTransitionDeclarationSha256(value) {
  requireExactKeys(value, REVIEWED_DECLARATION_PAYLOAD_KEYS, "reviewed declaration payload");
  validateTransitionMembers(value.addedFiles, "reviewed declaration addedFiles", true);
  validateTransitionChangedMembers(value.changedMembers);
  // require exact commit and contract identities before hashing
  if (typeof value.predecessorCommit !== "string" || !COMMIT_SHA.test(value.predecessorCommit)
    || typeof value.targetCommit !== "string" || !COMMIT_SHA.test(value.targetCommit)
    || value.predecessorCommit === value.targetCommit
    || value.contractVersion !== REVIEWED_DECLARATION_CONTRACT_VERSION) {
    fail("reviewed declaration commit or contract identity is invalid");
  }
  return sha256(Buffer.from(`${JSON.stringify(reviewedDeclarationPayload(value))}\n`, "utf8"));
}

// validate one finite content-addressed review without inferring approval
export function validateReviewedTransitionDeclaration(value) {
  requireExactKeys(value, REVIEWED_DECLARATION_KEYS, "reviewed declaration");
  const payload = reviewedDeclarationPayload(value);
  const declarationSha256 = reviewedTransitionDeclarationSha256(payload);
  requireTransitionSha256(value.declarationSha256, "reviewed declaration.declarationSha256");
  // bind the supplied address to every reviewed identity
  if (value.declarationSha256 !== declarationSha256) {
    fail("reviewed declaration.declarationSha256 does not match its canonical payload");
  }
  const additions = new Set();
  // prohibit optional declarations from redefining mandatory WP8b material
  for (const member of value.addedFiles) {
    if (WP8B_TARGET_ADDED_FILES.includes(member.path)
      || WP8B_TARGET_ONLY_ROOTS.some((root) => member.path.startsWith(`${root}/`))) {
      fail(`reviewed declaration addition ${member.path} overlaps mandatory WP8b material`);
    }
    additions.add(member.path);
  }
  // keep added and changed declarations disjoint
  for (const member of value.changedMembers) {
    if (additions.has(member.path)) {
      fail(`reviewed declaration path ${member.path} cannot be both added and changed`);
    }
  }
  return value;
}

// construct one declaration from explicit caller-supplied immutable identities
export function buildReviewedTransitionDeclaration(input) {
  requireExactKeys(
    input,
    ["addedFiles", "changedMembers", "predecessorCommit", "targetCommit"],
    "reviewed declaration input",
  );
  const payload = {
    addedFiles: structuredClone(input.addedFiles),
    changedMembers: structuredClone(input.changedMembers),
    contractVersion: REVIEWED_DECLARATION_CONTRACT_VERSION,
    predecessorCommit: input.predecessorCommit,
    targetCommit: input.targetCommit,
  };
  return validateReviewedTransitionDeclaration({
    ...payload,
    declarationSha256: reviewedTransitionDeclarationSha256(payload),
  });
}

// validate exact finite target-only root expansions
function validateTransitionTargetOnlyRoots(value) {
  // bound root count
  if (!Array.isArray(value) || value.length > TRANSITION_MAX_MEMBERS) {
    fail("targetOnlyRoots must be a bounded array");
  }
  let previous = "";
  const roots = [];
  // inspect every exact root expansion
  for (const [index, targetRoot] of value.entries()) {
    requireExactKeys(targetRoot, TRANSITION_TARGET_ROOT_KEYS, `targetOnlyRoots[${index}]`);
    const root = requireTransitionPath(targetRoot.root, `targetOnlyRoots[${index}].root`);
    // reject broad unsorted root declarations
    if (TRANSITION_BROAD_ROOTS.has(root) || !root.includes("/") || previous >= root) {
      fail("targetOnlyRoots must contain sorted scoped roots, not broad prefixes");
    }
    validateTransitionMembers(targetRoot.members, `targetOnlyRoots[${index}].members`);
    requireTransitionSha256(targetRoot.treeSha256, `targetOnlyRoots[${index}].treeSha256`);
    // bind the root digest to its exact final-newline manifest
    if (treeSha256(targetRoot.members) !== targetRoot.treeSha256) {
      fail(`targetOnlyRoots[${index}].treeSha256 does not match its exact members`);
    }
    // retain every member under its named finite root
    for (const member of targetRoot.members) {
      if (!member.path.startsWith(`${root}/`)) {
        fail(`targetOnlyRoots[${index}] contains a member outside ${root}`);
      }
    }
    // prohibit overlapping finite root authority
    for (const otherRoot of roots) {
      if (root.startsWith(`${otherRoot}/`) || otherRoot.startsWith(`${root}/`)) {
        fail("targetOnlyRoots must not overlap");
      }
    }
    roots.push(root);
    previous = root;
  }
  return value;
}

// require one exact ordered declaration path sequence
function requireExactTransitionPaths(actual, expected, label) {
  // reject additions outside the selected reviewed declaration
  if (actual.length !== expected.length
    || actual.some((path, index) => path !== expected[index])) {
    fail(`${label} must exactly match the immutable WP8b declaration`);
  }
}

// index one canonical transition member list
function transitionMemberMap(members) {
  return new Map(members.map((member) => [member.path, member.sha256]));
}

// index every exact transition addition
function transitionDeclaredAdditions(addedFiles, targetOnlyRoots) {
  const additions = new Map();
  // retain standalone additions
  for (const member of addedFiles) {
    additions.set(member.path, member.sha256);
  }
  // retain every finite root member
  for (const targetRoot of targetOnlyRoots) {
    for (const member of targetRoot.members) {
      // prohibit duplicate addition authority
      if (additions.has(member.path)) {
        fail(`target addition ${member.path} is declared more than once`);
      }
      additions.set(member.path, member.sha256);
    }
  }
  return additions;
}

// validate one complete finite predecessor-to-rust transition
export function validatePredecessorTransition(value) {
  const declared = value?.contractVersion === DECLARED_TRANSITION_CONTRACT_VERSION;
  requireExactKeys(
    value,
    declared ? DECLARED_TRANSITION_DOCUMENT_KEYS : TRANSITION_DOCUMENT_KEYS,
    "transition",
  );
  // reject historical and unknown transition contracts
  if (!declared && value.contractVersion !== TRANSITION_CONTRACT_VERSION) {
    fail("transition.contractVersion is invalid");
  }
  // retain the isolated predecessor bootstrap lane
  if (value.publicationLane !== TRANSITION_PUBLICATION_LANE) {
    fail("transition.publicationLane must use predecessor-control-bootstrap");
  }
  validateTransitionPredecessor(value.predecessor);
  validateTransitionTarget(value.target, value.predecessor);
  validateTransitionMembers(value.predecessorMembers, "predecessorMembers");
  validateTransitionMembers(value.targetMembers, "targetMembers");
  validateTransitionMembers(value.addedFiles, "addedFiles", true);
  const changedMembers = validateTransitionChangedMembers(value.changedMembers);
  const targetOnlyRoots = validateTransitionTargetOnlyRoots(value.targetOnlyRoots);
  const reviewedDeclaration = declared
    ? validateReviewedTransitionDeclaration(value.reviewedDeclaration)
    : null;
  // crossbind the optional declaration to the transition commits
  if (reviewedDeclaration !== null
    && (reviewedDeclaration.predecessorCommit !== value.predecessor.commit
      || reviewedDeclaration.targetCommit !== value.target.commit)) {
    fail("reviewed declaration does not match transition commits");
  }
  const expectedAddedPaths = reviewedDeclaration === null
    ? WP8B_TARGET_ADDED_FILES
    : [
      ...WP8B_TARGET_ADDED_FILES,
      ...reviewedDeclaration.addedFiles.map((member) => member.path),
    ].sort();
  requireExactTransitionPaths(
    value.addedFiles.map((member) => member.path),
    expectedAddedPaths,
    "addedFiles",
  );
  requireExactTransitionPaths(
    targetOnlyRoots.map((targetRoot) => targetRoot.root),
    WP8B_TARGET_ONLY_ROOTS,
    "targetOnlyRoots",
  );
  const predecessor = transitionMemberMap(value.predecessorMembers);
  const target = transitionMemberMap(value.targetMembers);
  const changed = new Map(changedMembers.map((member) => [member.path, member]));
  const additions = transitionDeclaredAdditions(value.addedFiles, targetOnlyRoots);
  const reviewedChanges = reviewedDeclaration === null
    ? null
    : new Map(reviewedDeclaration.changedMembers.map((member) => [member.path, member]));
  // require every additional file to retain its reviewed target digest
  if (reviewedDeclaration !== null) {
    const added = transitionMemberMap(value.addedFiles);
    for (const member of reviewedDeclaration.addedFiles) {
      if (added.get(member.path) !== member.sha256) {
        fail(`reviewed declaration addition ${member.path} does not match target bytes`);
      }
    }
  }
  // compare every predecessor member to the complete target tree
  for (const [path, predecessorSha256] of predecessor) {
    const targetSha256 = target.get(path);
    // prohibit predecessor member removal
    if (targetSha256 === undefined) {
      fail(`predecessor member ${path} is missing from targetMembers`);
    }
    const transition = changed.get(path);
    // retain undeclared predecessor bytes exactly
    if (transition === undefined) {
      if (targetSha256 !== predecessorSha256) {
        fail(`predecessor member ${path} changed without an exact declaration`);
      }
      continue;
    }
    // bind both sides of every declared change
    if (transition.predecessorSha256 !== predecessorSha256
      || transition.targetSha256 !== targetSha256) {
      fail(`changed member ${path} does not match predecessor and target bytes`);
    }
    // limit v2 changes to the fixed historical review or exact content-addressed declaration
    if (reviewedChanges !== null) {
      const reviewed = reviewedChanges.get(path);
      const fixed = APP_QUERY_TRANSITION.path === path
        && APP_QUERY_TRANSITION.sourceSha256 === predecessorSha256
        && APP_QUERY_TRANSITION.targetSha256 === targetSha256;
      const reviewedMatches = reviewed !== undefined
        && reviewed.predecessorSha256 === predecessorSha256
        && reviewed.targetSha256 === targetSha256;
      if ((reviewed !== undefined && !reviewedMatches) || (!fixed && !reviewedMatches)) {
        fail(`changed member ${path} is outside the content-addressed review declaration`);
      }
      reviewedChanges.delete(path);
    }
    changed.delete(path);
  }
  // prohibit declarations for members outside the predecessor tree
  if (changed.size !== 0) {
    fail(`changed member ${changed.keys().next().value} is not in the predecessor tree`);
  }
  // require every reviewed change to appear in both complete trees
  if (reviewedChanges !== null && reviewedChanges.size !== 0) {
    fail(`reviewed changed member ${reviewedChanges.keys().next().value} is absent from transition`);
  }
  // compare every target-only member to exact file or expanded-root declarations
  for (const [path, targetSha256] of target) {
    if (predecessor.has(path)) {
      continue;
    }
    const declaredSha256 = additions.get(path);
    // reject extra and byte-mismatched additions
    if (declaredSha256 === undefined || declaredSha256 !== targetSha256) {
      fail(`target-only member ${path} is absent from the exact transition manifest`);
    }
    additions.delete(path);
  }
  // reject declarations absent from the complete target tree
  if (additions.size !== 0) {
    fail(`declared target addition ${additions.keys().next().value} is absent from targetMembers`);
  }
  return value;
}

// execute only as the entry point
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  // report bounded cli failures
  main(process.argv.slice(2)).catch((error) => {
    process.stderr.write(`schema-preserving-release: ${error.message}\n`);
    process.exitCode = 1;
  });
}
