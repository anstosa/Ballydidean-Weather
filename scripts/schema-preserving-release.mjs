#!/usr/bin/env node

import { randomBytes } from "node:crypto";
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
// authorize only the reviewed AQI application query bytes without widening schema authority
export const APP_QUERY_TRANSITION = Object.freeze({
  path: "packages/database/src/repositories.ts",
  sourceSha256: "073b2308f7deb663a0bdefefc75930dedaa1b4abc6bc40b4fda04524b1cb600c",
  targetSha256: "2a62ecf8a093c3161f838194a7c761dae4506b32a4ae0ab83e23bbfc246dd832",
});
export const STAGE_CONTRACT_VERSION = "weather-schema-preserving-stage/v1";
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

// validate one immutable stage receipt
export function validateStageReceipt(value, expectedTargetRelease) {
  requireExactKeys(value, STAGE_KEYS, "stage receipt");
  // bind the stage contract
  if (value.contractVersion !== STAGE_CONTRACT_VERSION) {
    fail("stage receipt contractVersion is invalid");
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
  // validate stage receipts
  if (command === "validate-stage" && args.length === 2) {
    const receipt = readPrivateJsonReceipt(args[0]);
    validateStageReceipt(receipt.value, args[1]);
    process.stdout.write(receipt.bytes);
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
  fail("usage: schema-preserving-release.mjs validate-publication FILE TARGET_RELEASE | validate-stage FILE TARGET_RELEASE | field FILE KEY | write-json OUTPUT");
}

// execute only as the entry point
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  // report bounded cli failures
  main(process.argv.slice(2)).catch((error) => {
    process.stderr.write(`schema-preserving-release: ${error.message}\n`);
    process.exitCode = 1;
  });
}
