#!/usr/bin/env node

import { lstatSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  NATIVE_DASHBOARD_PUBLICATION_CONTRACT_VERSION,
  validateNativeDashboardPublication,
} from "./schema-preserving-release.mjs";
import {
  validateImagePublicationProof,
  validateReleaseEvidence,
} from "./prepare-native-schema-preserving-publication.mjs";

export const NATIVE_DASHBOARD_PUBLICATION_REQUEST_VERSION =
  "weather-native-dashboard-publication-request/v1";
export {
  NATIVE_DASHBOARD_PUBLICATION_CONTRACT_VERSION,
  validateNativeDashboardPublication,
};

const MAX_REQUEST_BYTES = 16 * 1024 * 1024;
const REQUEST_KEYS = [
  "coreCommit", "corePublicationSha256", "coreRelease", "coreTree",
  "imagePublicationProof", "releaseEvidence", "requestVersion", "sourceCommit",
  "sourcePublicationSha256", "sourceRelease", "sourceWebImage", "sourceWebImageId",
  "targetTree",
];

// normalize pure dashboard publication failures
function fail(message) {
  throw new Error(message);
}

// require one plain object
function requireObject(value, label) {
  // reject arrays and custom prototypes
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    fail(`${label} must be an object`);
  }
  const prototype = Object.getPrototypeOf(value);
  // reject non-json object prototypes
  if (prototype !== Object.prototype && prototype !== null) {
    fail(`${label} must be a plain object`);
  }
}

// close one object shape before using any identity
function requireExactKeys(value, expected, label) {
  requireObject(value, label);
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  // reject missing and additional fields
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    fail(`${label} has unexpected fields`);
  }
}

// prepare one web-only publication from independent workflow and image evidence
export function prepareNativeDashboardPublication(input) {
  requireExactKeys(input, REQUEST_KEYS, "native dashboard publication request");
  // keep this request disjoint from unified and family publication authority
  if (input.requestVersion !== NATIVE_DASHBOARD_PUBLICATION_REQUEST_VERSION) {
    fail("native dashboard publication request version differs");
  }
  const evidence = validateReleaseEvidence(input.releaseEvidence);
  const { imagePublicationProofSha256, materialManifestSha256 } =
    validateImagePublicationProof(input.imagePublicationProof, evidence);
  return validateNativeDashboardPublication({
    check: structuredClone(evidence.check),
    contractVersion: NATIVE_DASHBOARD_PUBLICATION_CONTRACT_VERSION,
    coreCommit: input.coreCommit,
    corePublicationSha256: input.corePublicationSha256,
    coreRelease: input.coreRelease,
    coreTree: structuredClone(input.coreTree),
    imagePublicationProof: structuredClone(input.imagePublicationProof),
    imagePublicationProofSha256,
    materialManifestSha256,
    publish: structuredClone(evidence.publish),
    sourceCommit: input.sourceCommit,
    sourcePublicationSha256: input.sourcePublicationSha256,
    sourceRelease: input.sourceRelease,
    sourceWebImage: input.sourceWebImage,
    sourceWebImageId: input.sourceWebImageId,
    targetCommit: evidence.targetCommit,
    targetRelease: evidence.targetRelease,
    targetTree: structuredClone(input.targetTree),
    targetWebImage: evidence.webImage,
    targetWebImageId: evidence.webImageId,
  }, evidence.targetRelease, input.coreRelease, input.sourceRelease);
}

// read one bounded regular json file without following a final link
function readJson(path) {
  const absolute = resolve(path);
  const details = lstatSync(absolute);
  // reject empty linked non-files and oversized documents
  if (!details.isFile() || details.isSymbolicLink() || details.size === 0 ||
    details.size > MAX_REQUEST_BYTES) {
    fail("native dashboard publication input must be a bounded regular file");
  }
  return JSON.parse(readFileSync(absolute, "utf8"));
}

// emit one safe scalar field for the installed shell owner
function field(value, key) {
  // expose only direct safe scalar fields
  if (!/^[A-Za-z][A-Za-z0-9]{0,63}$/u.test(key) || !Object.hasOwn(value, key)) {
    fail("native dashboard publication field differs");
  }
  const selected = value[key];
  // retain only bounded printable strings
  if (typeof selected !== "string" || !/^[\x20-\x7e]{1,4096}$/u.test(selected)) {
    fail("native dashboard publication field is not a safe string");
  }
  return selected;
}

// run the pure preparation validation and field interface
function main(argv) {
  const [command, ...args] = argv;
  // prepare one request by default
  if (argv.length === 1) {
    process.stdout.write(`${JSON.stringify(prepareNativeDashboardPublication(readJson(argv[0])), null, 2)}\n`);
    return;
  }
  // validate one installed publication against all caller coordinates
  if (command === "validate" && args.length === 4) {
    validateNativeDashboardPublication(readJson(args[0]), args[1], args[2], args[3]);
    return;
  }
  // read one safe installed scalar
  if (command === "field" && args.length === 2) {
    process.stdout.write(`${field(readJson(args[0]), args[1])}\n`);
    return;
  }
  fail("usage: prepare-native-dashboard-publication.mjs REQUEST.json | validate FILE TARGET CORE SOURCE | field FILE KEY");
}

// execute only when selected as the command entry point
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`prepare-native-dashboard-publication: ${error.message}\n`);
    process.exitCode = 1;
  }
}
