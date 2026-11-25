#!/usr/bin/env node

import { lstatSync, readFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  GIT_ADAPTER_VERSION,
  deriveDeclaredGitTransitionMembers,
  deriveGitTransitionMembers,
} from "./predecessor-rust-transition-git.mjs";
import {
  DECLARED_TRANSITION_CONTRACT_VERSION,
  INSTALLED_OPERATOR_BUNDLE_NAMES,
  TRANSITION_CONTRACT_VERSION,
  TRANSITION_PUBLICATION_LANE,
  buildInstalledOperatorBundleManifest,
  sha256,
  treeSha256,
  validatePredecessorTransition,
  validateReviewedTransitionDeclaration,
} from "./predecessor-rust-transition.mjs";

export const OFFLINE_BOOTSTRAP_REQUEST_VERSION =
  "weather-predecessor-rust-offline-bootstrap-request/v1";
export const OFFLINE_BOOTSTRAP_DECLARED_REQUEST_VERSION =
  "weather-predecessor-rust-offline-bootstrap-request/v2";
export const OFFLINE_BOOTSTRAP_ARTIFACT_VERSION =
  "weather-predecessor-rust-offline-bootstrap-artifact/v1";
export const OFFLINE_BOOTSTRAP_DECLARED_ARTIFACT_VERSION =
  "weather-predecessor-rust-offline-bootstrap-artifact/v2";
export const OFFLINE_BOOTSTRAP_LIVE_GATE = "unresolved-current-predecessor-authority";

const MAX_EVIDENCE_BYTES = 8 * 1024 * 1024;
const SHA256 = /^[a-f0-9]{64}$/u;
const BUNDLE_PATHS = INSTALLED_OPERATOR_BUNDLE_NAMES.map((name) => `scripts/${name}`);
const INPUT_KEYS = [
  "bundleFiles",
  "controlBytes",
  "installationReceiptBytes",
  "predecessor",
  "recoveryDropInBytes",
  "requestVersion",
  "sourceEnvironmentBytes",
  "target",
];
const DECLARED_INPUT_KEYS = [...INPUT_KEYS, "reviewedDeclaration"];
const CLI_INPUT_KEYS = [
  "bundleFiles",
  "controlBase64",
  "installationReceiptBase64",
  "predecessor",
  "recoveryDropInBase64",
  "requestVersion",
  "sourceEnvironmentBase64",
  "target",
];
const DECLARED_CLI_INPUT_KEYS = [...CLI_INPUT_KEYS, "reviewedDeclaration"];
const PREDECESSOR_KEYS = [
  "bundleSha256",
  "commit",
  "controlSha256",
  "installationReceiptSha256",
  "recoveryDropInSha256",
  "release",
  "sourceEnvironmentSha256",
];
const TARGET_KEYS = ["commit", "release"];
const BUFFER_BUNDLE_KEYS = ["bytes", "path"];
const CLI_BUNDLE_KEYS = ["bytesBase64", "path"];

// normalize preparation failures
function fail(message) {
  throw new Error(message);
}

// require one plain object
function requireObject(value, label) {
  // reject arrays and custom values
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

// require bounded exact bytes
function requireBytes(value, label) {
  // reject missing empty or oversized evidence
  if (!Buffer.isBuffer(value) || value.length === 0 || value.length > MAX_EVIDENCE_BYTES) {
    fail(`${label} must be nonempty bounded bytes`);
  }
  return value;
}

// decode canonical base64 evidence
function decodeBase64(value, label) {
  // reject malformed encodings
  if (
    typeof value !== "string"
    || value.length === 0
    || value.length > MAX_EVIDENCE_BYTES * 2
    || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(value)
  ) {
    fail(`${label} must be canonical base64`);
  }
  const bytes = Buffer.from(value, "base64");
  // reject alternate encodings
  if (bytes.toString("base64") !== value) {
    fail(`${label} must be canonical base64`);
  }
  return requireBytes(bytes, label);
}

// validate the pinned identity shapes
function validateIdentities(predecessor, target) {
  requireExactKeys(predecessor, PREDECESSOR_KEYS, "predecessor");
  requireExactKeys(target, TARGET_KEYS, "target");
  // require every pinned predecessor digest
  for (const key of PREDECESSOR_KEYS.filter((key) => key.endsWith("Sha256"))) {
    // reject malformed identity hashes before Git reads
    if (typeof predecessor[key] !== "string" || !SHA256.test(predecessor[key])) {
      fail(`predecessor.${key} must be a lowercase SHA-256 digest`);
    }
  }
}

// validate and hash the supplied byte evidence
function validateByteEvidence(input) {
  const declared = input?.requestVersion === OFFLINE_BOOTSTRAP_DECLARED_REQUEST_VERSION;
  requireExactKeys(input, declared ? DECLARED_INPUT_KEYS : INPUT_KEYS, "offline bootstrap input");
  // bind one reviewed baseline or explicit declared request version
  if (!declared && input.requestVersion !== OFFLINE_BOOTSTRAP_REQUEST_VERSION) {
    fail("offline bootstrap requestVersion is invalid");
  }
  validateIdentities(input.predecessor, input.target);
  // crossbind the optional finite declaration before any Git reads
  if (declared) {
    validateReviewedTransitionDeclaration(input.reviewedDeclaration);
    if (input.reviewedDeclaration.predecessorCommit !== input.predecessor.commit
      || input.reviewedDeclaration.targetCommit !== input.target.commit) {
      fail("reviewed declaration does not match bootstrap commits");
    }
  }
  // require the exact two bundle files
  if (!Array.isArray(input.bundleFiles) || input.bundleFiles.length !== BUNDLE_PATHS.length) {
    fail("bundleFiles must contain the exact two-file operator bundle");
  }
  const bundleFiles = input.bundleFiles.map((file, index) => {
    requireExactKeys(file, BUFFER_BUNDLE_KEYS, `bundleFiles[${index}]`);
    // admit only the two named repository inputs
    if (!BUNDLE_PATHS.includes(file.path)) {
      fail("bundleFiles do not match the exact offline operator bundle paths");
    }
    return {
      bytes: requireBytes(file.bytes, `bundleFiles[${index}].bytes`),
      // the installer hashes names relative to its two-file bundle directory
      path: basename(file.path),
    };
  });
  const operatorBundle = buildInstalledOperatorBundleManifest(bundleFiles);
  // bind the predecessor bundle digest
  if (operatorBundle.manifestSha256 !== input.predecessor.bundleSha256) {
    fail("predecessor.bundleSha256 does not match the supplied bundle bytes");
  }
  const controlBytes = requireBytes(input.controlBytes, "controlBytes");
  const installationReceiptBytes = requireBytes(
    input.installationReceiptBytes,
    "installationReceiptBytes",
  );
  const recoveryDropInBytes = requireBytes(input.recoveryDropInBytes, "recoveryDropInBytes");
  const sourceEnvironmentBytes = requireBytes(
    input.sourceEnvironmentBytes,
    "sourceEnvironmentBytes",
  );
  const controlSha256 = sha256(controlBytes);
  const installationReceiptSha256 = sha256(installationReceiptBytes);
  const recoveryDropInSha256 = sha256(recoveryDropInBytes);
  const sourceEnvironmentSha256 = sha256(sourceEnvironmentBytes);
  // bind the exact predecessor control bytes
  if (controlSha256 !== input.predecessor.controlSha256) {
    fail("predecessor.controlSha256 does not match the supplied control bytes");
  }
  // bind the exact installation receipt bytes
  if (installationReceiptSha256 !== input.predecessor.installationReceiptSha256) {
    fail("predecessor.installationReceiptSha256 does not match the supplied receipt bytes");
  }
  // bind the exact recovery drop-in bytes
  if (recoveryDropInSha256 !== input.predecessor.recoveryDropInSha256) {
    fail("predecessor.recoveryDropInSha256 does not match the supplied drop-in bytes");
  }
  // bind the exact source environment bytes
  if (sourceEnvironmentSha256 !== input.predecessor.sourceEnvironmentSha256) {
    fail("predecessor.sourceEnvironmentSha256 does not match the supplied environment bytes");
  }
  return {
    controlSha256,
    installationReceiptSha256,
    operatorBundle,
    recoveryDropInSha256,
    sourceEnvironmentSha256,
  };
}

// require one adapter-produced Git evidence shape
function validateGitEvidence(evidence, input, expectedMode) {
  requireObject(evidence, "Git transition evidence");
  // bind the adapter version
  if (evidence.adapterVersion !== GIT_ADAPTER_VERSION) {
    fail("Git transition evidence adapterVersion is invalid");
  }
  // prevent fixture evidence from posing as production reads
  if (evidence.evidenceMode !== expectedMode) {
    fail(`Git transition evidence must use ${expectedMode}`);
  }
  // bind both exact commits
  if (
    evidence.predecessorCommit !== input.predecessor.commit
    || evidence.targetCommit !== input.target.commit
  ) {
    fail("Git transition evidence does not match the pinned commits");
  }
  const declared = input.requestVersion === OFFLINE_BOOTSTRAP_DECLARED_REQUEST_VERSION;
  // bind declared evidence to the exact content-addressed request without accepting adapter drift
  if (declared) {
    validateReviewedTransitionDeclaration(evidence.reviewedDeclaration);
    if (evidence.reviewedDeclaration.declarationSha256
      !== input.reviewedDeclaration.declarationSha256) {
      fail("Git transition evidence does not match the reviewed declaration");
    }
  }
  // preserve the baseline evidence shape without an undeclared review extension
  if (!declared && Object.hasOwn(evidence, "reviewedDeclaration")) {
    fail("Git transition evidence contains an undeclared review extension");
  }
}

// prepare one explicitly ineligible artifact
function prepareFromEvidence(input, evidence, expectedMode, byteEvidence = validateByteEvidence(input)) {
  validateGitEvidence(evidence, input, expectedMode);
  const declared = input.requestVersion === OFFLINE_BOOTSTRAP_DECLARED_REQUEST_VERSION;
  const transition = {
    addedFiles: evidence.addedFiles,
    changedMembers: evidence.changedMembers,
    contractVersion: declared
      ? DECLARED_TRANSITION_CONTRACT_VERSION
      : TRANSITION_CONTRACT_VERSION,
    predecessor: structuredClone(input.predecessor),
    predecessorMembers: evidence.predecessorMembers,
    publicationLane: TRANSITION_PUBLICATION_LANE,
    target: structuredClone(input.target),
    targetMembers: evidence.targetMembers,
    targetOnlyRoots: evidence.targetOnlyRoots,
  };
  // retain the declaration itself only in the explicit v2 transition
  if (declared) {
    transition.reviewedDeclaration = structuredClone(input.reviewedDeclaration);
  }
  validatePredecessorTransition(transition);
  return {
    artifactVersion: declared
      ? OFFLINE_BOOTSTRAP_DECLARED_ARTIFACT_VERSION
      : OFFLINE_BOOTSTRAP_ARTIFACT_VERSION,
    controlSha256: byteEvidence.controlSha256,
    eligibility: "offline-only-ineligible",
    eligibleForLiveInstallation: false,
    gitEvidenceMode: evidence.evidenceMode,
    installationReceiptSha256: byteEvidence.installationReceiptSha256,
    liveAuthorityVerified: false,
    liveGate: OFFLINE_BOOTSTRAP_LIVE_GATE,
    operatorBundle: byteEvidence.operatorBundle,
    predecessorSourceTreeSha256: treeSha256(transition.predecessorMembers),
    recoveryDropInSha256: byteEvidence.recoveryDropInSha256,
    sourceEnvironmentSha256: byteEvidence.sourceEnvironmentSha256,
    targetTreeSha256: treeSha256(transition.targetMembers),
    transition,
  };
}

// prepare from committed local Git objects only
export function prepareOfflineBootstrap(input, cwd = process.cwd()) {
  const byteEvidence = validateByteEvidence(input);
  const evidence = input.requestVersion === OFFLINE_BOOTSTRAP_DECLARED_REQUEST_VERSION
    ? deriveDeclaredGitTransitionMembers(
      input.predecessor.commit,
      input.target.commit,
      input.reviewedDeclaration,
      cwd,
    )
    : deriveGitTransitionMembers(
      input.predecessor.commit,
      input.target.commit,
      cwd,
    );
  return prepareFromEvidence(input, evidence, "git-object-database", byteEvidence);
}

// prepare an explicitly fixture-only artifact
export function prepareOfflineBootstrapFixture(input, evidence) {
  return prepareFromEvidence(input, evidence, "injected-test-fixture");
}

// decode one versioned CLI request
export function decodeOfflineBootstrapRequest(value) {
  const declared = value?.requestVersion === OFFLINE_BOOTSTRAP_DECLARED_REQUEST_VERSION;
  requireExactKeys(
    value,
    declared ? DECLARED_CLI_INPUT_KEYS : CLI_INPUT_KEYS,
    "offline bootstrap CLI request",
  );
  // require exact bundle evidence descriptors
  if (!Array.isArray(value.bundleFiles) || value.bundleFiles.length !== BUNDLE_PATHS.length) {
    fail("bundleFiles must contain the exact two-file operator bundle");
  }
  const bundleFiles = value.bundleFiles.map((file, index) => {
    requireExactKeys(file, CLI_BUNDLE_KEYS, `bundleFiles[${index}]`);
    return {
      bytes: decodeBase64(file.bytesBase64, `bundleFiles[${index}].bytesBase64`),
      path: file.path,
    };
  });
  const decoded = {
    bundleFiles,
    controlBytes: decodeBase64(value.controlBase64, "controlBase64"),
    installationReceiptBytes: decodeBase64(
      value.installationReceiptBase64,
      "installationReceiptBase64",
    ),
    predecessor: value.predecessor,
    recoveryDropInBytes: decodeBase64(
      value.recoveryDropInBase64,
      "recoveryDropInBase64",
    ),
    requestVersion: value.requestVersion,
    sourceEnvironmentBytes: decodeBase64(
      value.sourceEnvironmentBase64,
      "sourceEnvironmentBase64",
    ),
    target: value.target,
  };
  // preserve the explicit declaration only for the v2 request shape
  if (declared) {
    decoded.reviewedDeclaration = structuredClone(value.reviewedDeclaration);
  }
  return decoded;
}

// read one bounded request document
function readRequest(path) {
  const absolute = resolve(path);
  const details = lstatSync(absolute);
  // reject links non-files and oversized requests
  if (
    !details.isFile()
    || details.isSymbolicLink()
    || details.size === 0
    || details.size > MAX_EVIDENCE_BYTES
  ) {
    fail("offline bootstrap request must be a bounded regular file");
  }
  return JSON.parse(readFileSync(absolute, "utf8"));
}

// run the offline-only preparation interface
function main(argv) {
  // require one explicit request document
  if (argv.length !== 1) {
    fail("usage: predecessor-rust-transition-prepare.mjs REQUEST.json");
  }
  const input = decodeOfflineBootstrapRequest(readRequest(argv[0]));
  const artifact = prepareOfflineBootstrap(input);
  process.stdout.write(`${JSON.stringify(artifact, null, 2)}\n`);
}

// execute only as the entry point
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`predecessor-rust-transition-prepare: ${error.message}\n`);
    process.exitCode = 1;
  }
}
