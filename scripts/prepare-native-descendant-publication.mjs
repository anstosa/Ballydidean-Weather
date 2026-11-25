#!/usr/bin/env node

import { lstatSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  NATIVE_DESCENDANT_PUBLICATION_CONTRACT_VERSION,
  RUNTIME_GENERATION_RUST,
  validateNativeDescendantPublication,
} from "./schema-preserving-release.mjs";
import {
  validateImagePublicationProof,
  validateReleaseEvidence,
} from "./prepare-native-schema-preserving-publication.mjs";

export const NATIVE_DESCENDANT_PUBLICATION_REQUEST_VERSION =
  "weather-native-descendant-publication-request/v1";

const MAX_REQUEST_BYTES = 16 * 1024 * 1024;
const REQUEST_KEYS = [
  "imagePublicationProof",
  "releaseEvidence",
  "requestVersion",
  "sourceCommit",
  "sourcePublicationSha256",
  "sourceRelease",
  "sourceTree",
  "targetTree",
];

// normalize preparation failures without exposing command output
function fail(message) {
  throw new Error(message);
}

// require one plain request object
function requireObject(value, label) {
  // reject arrays and custom request objects
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    fail(`${label} must be an object`);
  }
  const prototype = Object.getPrototypeOf(value);
  // reject custom prototypes
  if (prototype !== Object.prototype && prototype !== null) {
    fail(`${label} must be a plain object`);
  }
}

// close the request shape before projecting any authority
function requireExactKeys(value, expected, label) {
  requireObject(value, label);
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  // reject missing or additional request fields
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    fail(`${label} has unexpected fields`);
  }
}

// prepare one pure ordinary native descendant from independent workflow and image evidence
export function prepareNativeDescendantPublication(input) {
  requireExactKeys(input, REQUEST_KEYS, "native descendant publication request");
  // keep ordinary descendants separate from bootstrap and family publications
  if (input.requestVersion !== NATIVE_DESCENDANT_PUBLICATION_REQUEST_VERSION) {
    fail("native descendant publication request version differs");
  }
  const releaseEvidence = validateReleaseEvidence(input.releaseEvidence);
  const { imagePublicationProofSha256, materialManifestSha256 } =
    validateImagePublicationProof(input.imagePublicationProof, releaseEvidence);
  return validateNativeDescendantPublication({
    ...structuredClone(releaseEvidence),
    contractVersion: NATIVE_DESCENDANT_PUBLICATION_CONTRACT_VERSION,
    imagePublicationProof: structuredClone(input.imagePublicationProof),
    imagePublicationProofSha256,
    materialManifestSha256,
    sourceCommit: input.sourceCommit,
    sourceGeneration: RUNTIME_GENERATION_RUST,
    sourcePublicationSha256: input.sourcePublicationSha256,
    sourceRelease: input.sourceRelease,
    sourceTree: structuredClone(input.sourceTree),
    targetGeneration: RUNTIME_GENERATION_RUST,
    targetTree: structuredClone(input.targetTree),
  }, releaseEvidence.targetRelease);
}

// read one bounded regular request without following a final link
function readRequest(path) {
  const absolute = resolve(path);
  const details = lstatSync(absolute);
  // reject empty linked non-files and oversized requests
  if (!details.isFile() || details.isSymbolicLink() || details.size === 0 ||
    details.size > MAX_REQUEST_BYTES) {
    fail("native descendant publication request must be a bounded regular file");
  }
  return JSON.parse(readFileSync(absolute, "utf8"));
}

// run the pure stdout-only preparation interface
function main(argv) {
  // accept exactly one request path
  if (argv.length !== 1) {
    fail("usage: prepare-native-descendant-publication.mjs REQUEST.json");
  }
  const output = prepareNativeDescendantPublication(readRequest(argv[0]));
  process.stdout.write(`${JSON.stringify(output, null, 2)}\n`);
}

// execute only when selected as the command entry point
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`prepare-native-descendant-publication: ${error.message}\n`);
    process.exitCode = 1;
  }
}
