#!/usr/bin/env node

import { lstatSync, readFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  canonicalSha256sumManifest,
  sha256,
  validatePredecessorTransition,
} from "./schema-preserving-release.mjs";

export {
  DECLARED_TRANSITION_CONTRACT_VERSION,
  REVIEWED_DECLARATION_CONTRACT_VERSION,
  TRANSITION_CONTRACT_VERSION,
  TRANSITION_PUBLICATION_LANE,
  WP8B_TARGET_ADDED_FILES,
  WP8B_TARGET_ONLY_ROOTS,
  buildReviewedTransitionDeclaration,
  canonicalSha256sumManifest,
  reviewedTransitionDeclarationSha256,
  sha256,
  treeSha256,
  validatePredecessorTransition,
  validateReviewedTransitionDeclaration,
} from "./schema-preserving-release.mjs";

export const OPERATOR_BUNDLE_CONTRACT_VERSION = "weather-operator-bundle-manifest/v1";
export const INSTALLED_OPERATOR_BUNDLE_NAMES = Object.freeze([
  "schema-preserving-release.mjs",
  "schema-preserving-release.sh",
]);

const MAX_DOCUMENT_BYTES = 8 * 1024 * 1024;
const SAFE_BUNDLE_PATH = /^[\x20-\x7e]{1,512}$/u;
const BUNDLE_WILDCARD = /[*?\[\]{}]/u;

// normalize wrapper and bundle failures
function fail(message) {
  throw new Error(message);
}

// require one plain bundle descriptor
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

// enforce one closed wrapper shape
function requireExactKeys(value, expected, label) {
  requireObject(value, label);
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  // reject shape drift
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    fail(`${label} has unexpected fields`);
  }
}

// require one finite canonical bundle path
function requirePath(value, label) {
  // reject traversal aliases and wildcard membership
  if (typeof value !== "string"
    || !SAFE_BUNDLE_PATH.test(value)
    || value.startsWith("/")
    || value.endsWith("/")
    || value.includes("\\")
    || BUNDLE_WILDCARD.test(value)
    || value.split("/").some((part) => part === "" || part === "." || part === "..")) {
    fail(`${label} must be a canonical finite path without wildcards`);
  }
  return value;
}

// compare bundle paths bytewise
function comparePaths(left, right) {
  // preserve bytewise ordering
  if (left.path < right.path) {
    return -1;
  }
  // preserve bytewise ordering
  if (left.path > right.path) {
    return 1;
  }
  return 0;
}

// build one exact two-file operator bundle manifest
export function buildTwoFileBundleManifest(files) {
  // require exactly two named bundle inputs
  if (!Array.isArray(files) || files.length !== 2) {
    fail("operator bundle must contain exactly two files");
  }
  const members = files.map((file, index) => {
    requireExactKeys(file, ["bytes", "path"], `bundle file[${index}]`);
    const path = requirePath(file.path, `bundle file[${index}].path`);
    // require exact byte buffers
    if (!Buffer.isBuffer(file.bytes)) {
      fail(`bundle file[${index}].bytes must be a Buffer`);
    }
    return { path, sha256: sha256(file.bytes) };
  }).sort(comparePaths);
  // reject duplicate bundle paths
  if (members[0].path === members[1].path) {
    fail("operator bundle paths must be unique");
  }
  const sha256sum = canonicalSha256sumManifest(members);
  return {
    contractVersion: OPERATOR_BUNDLE_CONTRACT_VERSION,
    manifestSha256: sha256(Buffer.from(sha256sum, "utf8")),
    members,
    sha256sum,
  };
}

// bind the exact two names retained by the installed recovery helper
export function buildInstalledOperatorBundleManifest(files) {
  const manifest = buildTwoFileBundleManifest(files);
  // prohibit a generic two-file hash from posing as the installed bundle identity
  if (manifest.members.some((member, index) =>
    member.path !== INSTALLED_OPERATOR_BUNDLE_NAMES[index]
  )) {
    fail("bundleFiles do not match the installed operator bundle names");
  }
  return manifest;
}

// read one bounded local wrapper input
function readBoundedFile(path) {
  const absolute = resolve(path);
  const details = lstatSync(absolute);
  // reject links non-files and oversized documents
  if (!details.isFile() || details.isSymbolicLink() || details.size > MAX_DOCUMENT_BYTES) {
    fail(`${basename(path)} is not a bounded regular file`);
  }
  return readFileSync(absolute);
}

// run the stable offline wrapper interface
function main(argv) {
  const [command, ...args] = argv;
  // validate one checked finite transition
  if (command === "validate" && args.length === 1) {
    const document = JSON.parse(readBoundedFile(args[0]).toString("utf8"));
    validatePredecessorTransition(document);
    process.stdout.write(`${JSON.stringify(document, null, 2)}\n`);
    return;
  }
  // build one deterministic two-file bundle manifest
  if (command === "bundle-manifest" && args.length === 2) {
    const files = args.map(
      // installation identity is independent of the staging directory
      (path) => ({ bytes: readBoundedFile(path), path: basename(path) }),
    );
    process.stdout.write(`${JSON.stringify(buildInstalledOperatorBundleManifest(files), null, 2)}\n`);
    return;
  }
  fail("usage: predecessor-rust-transition.mjs validate DOCUMENT | bundle-manifest FILE FILE");
}

// execute only as the wrapper entry point
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`predecessor-rust-transition: ${error.message}\n`);
    process.exitCode = 1;
  }
}
