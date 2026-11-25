#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  closeSync,
  constants,
  fstatSync,
  lstatSync,
  openSync,
  opendirSync,
  readFileSync,
  readSync,
  realpathSync,
} from "node:fs";
import { resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

export const RUNTIME_IMAGE_MATERIAL_INPUT_VERSION =
  "weather-rust-runtime-image-material-input/v1";
export const RUNTIME_IMAGE_MATERIAL_VERSION = "weather-rust-runtime-image-material/v1";
export const RUNTIME_IMAGE_MATERIAL_CLASS = "offline-image-material-preflight";
export const RUNTIME_IMAGE_MATERIAL_INSPECTION_CLASS =
  "offline-extracted-image-material-inspection";

const MAX_INPUT_BYTES = 8 * 1024 * 1024;
const MAX_MATERIAL_FILE_BYTES = 512 * 1024 * 1024;
const MAX_MATERIAL_TOTAL_BYTES = 2 * 1024 * 1024 * 1024;
const MAX_MEMBERS = 10000;
const MAX_TREE_DEPTH = 64;
const MAX_TREE_NODES = MAX_MEMBERS * 4;
const HASH_BUFFER_BYTES = 64 * 1024;
const ELF64_HEADER_BYTES = 64;
const ELF64_PROGRAM_HEADER_BYTES = 56;
const ELF64_LE_IDENT = Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1]);
const EM_AARCH64 = 183;
const ELF_EXECUTABLE_TYPES = new Set([2, 3]);
const COMMIT_SHA = /^[a-f0-9]{40}$/u;
const SHA256 = /^[a-f0-9]{64}$/u;
const SAFE_PATH = /^[\x20-\x7e]{1,512}$/u;
const MEMBER_KEYS = [
  "buildSha256",
  "path",
  "role",
  "runtimeSha256",
  "sourcePath",
  "sourceSha256",
];
const INPUT_KEYS = ["actual", "build", "expected", "inputVersion", "source"];
const INSPECTION_INPUT_KEYS = ["build", "expected", "inputVersion", "source"];
const IMAGE_SET_KEYS = ["server", "web"];
const SOURCE_KEYS = ["commit", "treeSha256"];
const BUILD_KEYS = ["identitySha256", "profile", "target"];
const ROLES = new Set(["binary", "config", "migration", "model", "static"]);
const IMAGE_ROLES = Object.freeze({
  server: Object.freeze(["binary", "config", "migration", "model"]),
  web: Object.freeze(["binary", "config", "static"]),
});
const IMAGE_BINARIES = Object.freeze({
  server: Object.freeze([
    "/opt/weather/bin/weather-api",
    "/opt/weather/bin/weather-api-health",
    "/opt/weather/bin/weather-migrate",
    "/opt/weather/bin/weather-worker",
    "/opt/weather/bin/weather-worker-health",
  ]),
  web: Object.freeze(["/opt/weather/bin/weather-web", "/opt/weather/bin/weather-web-health"]),
});
const IMAGE_COMMANDS = Object.freeze({
  server: Object.freeze([
    Object.freeze({ argv: Object.freeze(["/opt/weather/bin/weather-api"]), name: "api" }),
    Object.freeze({ argv: Object.freeze(["/opt/weather/bin/weather-api-health", "api"]), name: "api-health" }),
    Object.freeze({ argv: Object.freeze(["/opt/weather/bin/weather-migrate"]), name: "migrate" }),
    Object.freeze({ argv: Object.freeze(["/opt/weather/bin/weather-worker"]), name: "worker" }),
    Object.freeze({ argv: Object.freeze(["/opt/weather/bin/weather-worker-health"]), name: "worker-health" }),
  ]),
  web: Object.freeze([
    Object.freeze({ argv: Object.freeze(["/opt/weather/bin/weather-web"]), name: "web" }),
    Object.freeze({ argv: Object.freeze(["/opt/weather/bin/weather-web-health", "web"]), name: "web-health" }),
  ]),
});

// normalize manifest failures
function fail(message) {
  throw new Error(message);
}

// hash exact canonical bytes
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
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

// require one digest
function requireSha256(value, label) {
  // reject unpinned identities
  if (typeof value !== "string" || !SHA256.test(value)) {
    fail(`${label} must be a lowercase SHA-256 digest`);
  }
  return value;
}

// require one canonical path
function requirePath(value, label, absolute) {
  // reject noncanonical paths
  if (
    typeof value !== "string"
    || !SAFE_PATH.test(value)
    || value.includes("\\")
    || value.endsWith("/")
    || value.split("/").some((part, index) => (
      (index > 0 || !absolute) && (part === "" || part === "." || part === "..")
    ))
    || (absolute ? !value.startsWith("/opt/weather/") : value.startsWith("/"))
  ) {
    fail(`${label} must be a canonical ${absolute ? "runtime" : "source"} path`);
  }
  return value;
}

// compare canonical member paths
function compareMembers(left, right) {
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

// validate and sort one supplied member set
function normalizeMembers(value, image, label) {
  // bound supplied membership
  if (!Array.isArray(value) || value.length === 0 || value.length > MAX_MEMBERS) {
    fail(`${label} must be a nonempty bounded member array`);
  }
  const allowedRoles = new Set(IMAGE_ROLES[image]);
  const members = value.map((member, index) => {
    requireExactKeys(member, MEMBER_KEYS, `${label}[${index}]`);
    requirePath(member.path, `${label}[${index}].path`, true);
    requirePath(member.sourcePath, `${label}[${index}].sourcePath`, false);
    // bind one declared role
    if (typeof member.role !== "string" || !ROLES.has(member.role)) {
      fail(`${label}[${index}].role is invalid`);
    }
    // keep each image inside its declared role boundary
    if (!allowedRoles.has(member.role)) {
      fail(`${label}[${index}].role is outside the ${image} boundary`);
    }
    // preserve the edge-only disjoint startup site configuration
    if (image === "web" && member.role === "config"
      && (member.path !== "/opt/weather/edge-config/ballydidean.json"
        || member.sourcePath !== "config/sites/ballydidean.json")) {
      fail(`${label}[${index}].role is outside the web configuration boundary`);
    }
    requireSha256(member.runtimeSha256, `${label}[${index}].runtimeSha256`);
    requireSha256(member.sourceSha256, `${label}[${index}].sourceSha256`);
    requireSha256(member.buildSha256, `${label}[${index}].buildSha256`);
    return structuredClone(member);
  }).sort(compareMembers);
  // reject duplicate runtime paths
  if (members.some((member, index) => index > 0 && member.path === members[index - 1].path)) {
    fail(`${label} must contain unique runtime paths`);
  }
  return members;
}

// encode one deterministic material preimage
function materialPreimage(members) {
  return `${members.map((member) => JSON.stringify(member)).join("\n")}\n`;
}

// compare actual and expected complete membership
function requireExactMembers(actual, expected, image) {
  // reject missing extra or changed metadata
  if (
    actual.length !== expected.length
    || actual.some((member, index) => (
      JSON.stringify(member) !== JSON.stringify(expected[index])
    ))
  ) {
    fail(`${image} actual members do not exactly match expected members`);
  }
}

// require the accepted binary split
function validateBinaries(members, image) {
  const binaries = members.filter((member) => member.role === "binary").map((member) => member.path);
  const expected = IMAGE_BINARIES[image];
  // reject missing extra or reordered binaries
  if (
    binaries.length !== expected.length
    || binaries.some((path, index) => path !== expected[index])
  ) {
    fail(`${image} binaries do not match the accepted native command set`);
  }
}

// require every declared role to have material
function validateRoles(members, image) {
  const roles = new Set(members.map((member) => member.role));
  // inspect the exact image role contract
  for (const role of IMAGE_ROLES[image]) {
    // reject incomplete role material
    if (!roles.has(role)) {
      fail(`${image} is missing required ${role} material`);
    }
  }
}

// validate source and build identities
function validateIdentities(source, build) {
  requireExactKeys(source, SOURCE_KEYS, "source");
  requireExactKeys(build, BUILD_KEYS, "build");
  // bind one exact source commit
  if (typeof source.commit !== "string" || !COMMIT_SHA.test(source.commit)) {
    fail("source.commit must be an exact lowercase commit SHA");
  }
  requireSha256(source.treeSha256, "source.treeSha256");
  requireSha256(build.identitySha256, "build.identitySha256");
  // bind the accepted native build identity
  if (build.profile !== "release" || build.target !== "aarch64-unknown-linux-gnu") {
    fail("build must use the release aarch64-unknown-linux-gnu target");
  }
}

// validate shared expected input
function validateExpectedInput(input, allowMissingActual) {
  requireObject(input, "input");
  const hasActual = Object.hasOwn(input, "actual");
  // admit the inspection-only shape
  if (allowMissingActual && !hasActual) {
    requireExactKeys(input, INSPECTION_INPUT_KEYS, "input");
  } else {
    requireExactKeys(input, INPUT_KEYS, "input");
  }
  // bind the input contract
  if (input.inputVersion !== RUNTIME_IMAGE_MATERIAL_INPUT_VERSION) {
    fail("inputVersion is invalid");
  }
  requireExactKeys(input.expected, IMAGE_SET_KEYS, "expected");
  validateIdentities(input.source, input.build);
}

// build one image material record
function buildImageRecord(image, actual, expected) {
  const actualMembers = normalizeMembers(actual, image, `actual.${image}`);
  const expectedMembers = normalizeMembers(expected, image, `expected.${image}`);
  requireExactMembers(actualMembers, expectedMembers, image);
  validateBinaries(actualMembers, image);
  validateRoles(actualMembers, image);
  const preimage = materialPreimage(actualMembers);
  return {
    commands: IMAGE_COMMANDS[image].map((command) => ({
      argv: [...command.argv],
      name: command.name,
    })),
    materialManifestSha256: sha256(Buffer.from(preimage, "utf8")),
    members: actualMembers,
    name: image,
    roles: [...IMAGE_ROLES[image]],
  };
}

// require disjoint application material
function validateBoundary(server, web) {
  const serverPaths = new Set(server.members.map((member) => member.path));
  // inspect every web path
  for (const member of web.members) {
    // reject cross-image application paths
    if (serverPaths.has(member.path)) {
      fail(`runtime path ${member.path} appears in both image boundaries`);
    }
  }
  const serverBinaries = new Set(
    server.members.filter((member) => member.role === "binary").map((member) => member.path),
  );
  // inspect every web binary
  for (const member of web.members.filter((candidate) => candidate.role === "binary")) {
    // reject cross-image binary membership
    if (serverBinaries.has(member.path)) {
      fail(`binary ${member.path} appears in both image boundaries`);
    }
  }
}

// assemble one validated manifest
function buildMaterialManifest(input, actual, artifactClass, inspection = null) {
  const server = buildImageRecord("server", actual.server, input.expected.server);
  const web = buildImageRecord("web", actual.web, input.expected.web);
  validateBoundary(server, web);
  const manifest = {
    artifactClass,
    attestsBuiltImage: false,
    boundary: {
      serverRoles: [...IMAGE_ROLES.server],
      sharedApplicationPaths: [],
      webRoles: [...IMAGE_ROLES.web],
    },
    build: structuredClone(input.build),
    contractVersion: RUNTIME_IMAGE_MATERIAL_VERSION,
    images: [server, web],
    platform: {
      architecture: "arm64",
      os: "linux",
    },
    source: structuredClone(input.source),
  };
  // state the inspection proof ceiling
  if (inspection !== null) {
    manifest.inspection = inspection;
  }
  return manifest;
}

// prepare one deterministic offline material manifest
export function buildRustRuntimeImageMaterial(input) {
  validateExpectedInput(input, false);
  requireExactKeys(input.actual, IMAGE_SET_KEYS, "actual");
  return buildMaterialManifest(input, input.actual, RUNTIME_IMAGE_MATERIAL_CLASS);
}

// inspect one canonical extracted root
function canonicalInspectionRoot(value, label) {
  // require an explicit path
  if (typeof value !== "string" || value.length === 0) {
    fail(`${label} must be a nonempty directory path`);
  }
  const absolute = resolve(value);
  // reject an unbounded filesystem root
  if (absolute === resolve(sep)) {
    fail(`${label} must not be the filesystem root`);
  }
  let details;
  // normalize inaccessible roots
  try {
    details = lstatSync(absolute);
  } catch {
    fail(`${label} cannot be inspected`);
  }
  // reject links and non-directories
  if (!details.isDirectory() || details.isSymbolicLink()) {
    fail(`${label} must be a real directory`);
  }
  let canonical;
  // resolve every ancestor once
  try {
    canonical = realpathSync(absolute);
  } catch {
    fail(`${label} cannot be resolved`);
  }
  // reject an aliased root
  if (canonical !== absolute) {
    fail(`${label} must not traverse symbolic links`);
  }
  return canonical;
}

// detect shared inspection trees
function rootsOverlap(left, right) {
  return left === right
    || left.startsWith(`${right}${sep}`)
    || right.startsWith(`${left}${sep}`);
}

// admit two disjoint extracted roots
function validateInspectionRoots(roots) {
  requireExactKeys(roots, IMAGE_SET_KEYS, "roots");
  const server = canonicalInspectionRoot(roots.server, "server root");
  const web = canonicalInspectionRoot(roots.web, "web root");
  // reject aliases and nesting
  if (rootsOverlap(server, web)) {
    fail("server and web roots must be distinct disjoint real directories");
  }
  return { server, web };
}

// compare one descriptor identity
function sameFileIdentity(left, right) {
  return left.dev === right.dev
    && left.ino === right.ino
    && left.mode === right.mode
    && left.size === right.size
    && left.mtimeMs === right.mtimeMs
    && left.ctimeMs === right.ctimeMs;
}

// hash one stable regular file and retain the binary header from that same read
function hashExtractedFile(path, runtimePath, listed, budget, binary) {
  // reject unsafe declared sizes
  if (
    !Number.isSafeInteger(listed.size)
    || listed.size < 0
    || listed.size > MAX_MATERIAL_FILE_BYTES
  ) {
    fail(`${runtimePath} exceeds the extracted file byte limit`);
  }
  // bound the complete inspected material
  if (budget.totalBytes + listed.size > MAX_MATERIAL_TOTAL_BYTES) {
    fail("extracted material exceeds the total byte limit");
  }
  let descriptor;
  // open without following the final entry
  try {
    descriptor = openSync(
      path,
      constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
    );
  } catch {
    fail(`${runtimePath} cannot be opened without following links`);
  }
  const hash = createHash("sha256");
  const buffer = budget.buffer;
  const elfHeader = binary ? Buffer.alloc(Math.min(ELF64_HEADER_BYTES, listed.size)) : null;
  let observedBytes = 0;
  const maximumObservedBytes = listed.size + 1;
  // inspect through the admitted descriptor
  try {
    const opened = fstatSync(descriptor);
    // bind path and descriptor identity
    if (!opened.isFile() || !sameFileIdentity(listed, opened)) {
      fail(`${runtimePath} changed before inspection`);
    }
    // stream bounded file bytes
    while (observedBytes < maximumObservedBytes) {
      const remaining = maximumObservedBytes - observedBytes;
      const count = readSync(descriptor, buffer, 0, Math.min(buffer.length, remaining), null);
      // stop at descriptor eof
      if (count === 0) {
        break;
      }
      // retain at most one elf header even if the descriptor returns short reads
      if (elfHeader !== null && observedBytes < elfHeader.length) {
        buffer.copy(elfHeader, observedBytes, 0, Math.min(count, elfHeader.length - observedBytes));
      }
      observedBytes += count;
      // reject aggregate growth during inspection
      if (budget.totalBytes + observedBytes > MAX_MATERIAL_TOTAL_BYTES) {
        fail("extracted material exceeds the total byte limit");
      }
      hash.update(buffer.subarray(0, count));
    }
    const completed = fstatSync(descriptor);
    // require one stable complete read
    if (observedBytes !== listed.size || !sameFileIdentity(opened, completed)) {
      fail(`${runtimePath} changed during inspection`);
    }
  } finally {
    closeSync(descriptor);
  }
  budget.totalBytes += observedBytes;
  return {
    elfHeader,
    mode: listed.mode,
    runtimeSha256: hash.digest("hex"),
    size: observedBytes,
  };
}

// admit only the supported little-endian arm64 executable header shapes
function requireArm64ElfExecutable(member) {
  const { elfHeader, mode, path, size } = member;
  // prove executable mode separately from binary header identity
  if ((mode & 0o111) === 0) {
    fail(`${path} must be executable`);
  }
  // check the fixed elf64 ident and machine fields before reading offsets
  if (
    elfHeader === null
    || elfHeader.length !== ELF64_HEADER_BYTES
    || !elfHeader.subarray(0, ELF64_LE_IDENT.length).equals(ELF64_LE_IDENT)
    || elfHeader.readUInt16LE(18) !== EM_AARCH64
    || elfHeader.readUInt32LE(20) !== 1
    || elfHeader.readUInt16LE(52) !== ELF64_HEADER_BYTES
  ) {
    fail(`${path} must have a little-endian AArch64 ELF64 header`);
  }
  // admit both ordinary executables and pie with an entry point
  if (!ELF_EXECUTABLE_TYPES.has(elfHeader.readUInt16LE(16)) || elfHeader.readBigUInt64LE(24) === 0n) {
    fail(`${path} must have an AArch64 ELF executable type and entry point`);
  }
  const programOffset = elfHeader.readBigUInt64LE(32);
  const programCount = elfHeader.readUInt16LE(56);
  const programEnd = programOffset + BigInt(programCount * ELF64_PROGRAM_HEADER_BYTES);
  // require the supported program header table to lie inside the hashed bytes
  if (
    elfHeader.readUInt16LE(54) !== ELF64_PROGRAM_HEADER_BYTES
    || programCount === 0
    || programCount === 0xffff
    || programOffset < BigInt(ELF64_HEADER_BYTES)
    || programEnd > BigInt(size)
  ) {
    fail(`${path} must have an in-file ELF64 program header table`);
  }
}

// walk one bounded extracted tree
function inspectExtractedDirectory(directory, relative, image, state, budget, depth) {
  // bound nested traversal
  if (depth > MAX_TREE_DEPTH) {
    fail(`${image} extracted root exceeds the directory depth limit`);
  }
  let handle;
  // open one bounded directory cursor
  try {
    handle = opendirSync(directory);
  } catch {
    fail(`${image} extracted directory cannot be read`);
  }
  // inspect every entry without materializing the directory
  try {
    while (true) {
      let entry;
      // advance one directory entry
      try {
        entry = handle.readSync();
      } catch {
        fail(`${image} extracted directory cannot be read`);
      }
      // stop at directory eof
      if (entry === null) {
        break;
      }
      state.nodeCount += 1;
      // bound every encountered node
      if (state.nodeCount > MAX_TREE_NODES) {
        fail(`${image} extracted root exceeds the traversal node limit`);
      }
      const childRelative = relative === "" ? entry.name : `${relative}/${entry.name}`;
      const runtimePath = requirePath(
        `/opt/weather/${childRelative}`,
        `${image} extracted path`,
        true,
      );
      const child = resolve(directory, entry.name);
      let details;
      // normalize entry races and access failures
      try {
        details = lstatSync(child);
      } catch {
        fail(`${runtimePath} cannot be inspected`);
      }
      // recurse only into real directories
      if (details.isDirectory() && !details.isSymbolicLink()) {
        let canonical;
        // detect directory replacement
        try {
          canonical = realpathSync(child);
        } catch {
          fail(`${runtimePath} cannot be resolved`);
        }
        // reject directory aliases
        if (canonical !== child) {
          fail(`${runtimePath} must not traverse symbolic links`);
        }
        inspectExtractedDirectory(child, childRelative, image, state, budget, depth + 1);
        continue;
      }
      // reject links and special files
      if (!details.isFile() || details.isSymbolicLink()) {
        fail(`${runtimePath} must be a regular file`);
      }
      state.fileCount += 1;
      // bound extracted membership
      if (state.fileCount > MAX_MEMBERS) {
        fail(`${image} extracted root exceeds the file count limit`);
      }
      state.files.push({
        path: runtimePath,
        ...hashExtractedFile(child, runtimePath, details, budget, IMAGE_BINARIES[image].includes(runtimePath)),
      });
    }
  } finally {
    handle.closeSync();
  }
}

// derive one extracted file inventory
function inspectExtractedRoot(root, image, budget) {
  const state = { fileCount: 0, files: [], nodeCount: 1 };
  inspectExtractedDirectory(root, "", image, state, budget, 0);
  // require material rather than an empty tree
  if (state.files.length === 0) {
    fail(`${image} extracted root must contain material files`);
  }
  return state.files.sort(compareMembers);
}

// bind inspected bytes to expected provenance
function bindInspectedMembers(inspected, expected, image) {
  const expectedMembers = normalizeMembers(expected, image, `expected.${image}`);
  const remaining = new Map(expectedMembers.map((member) => [member.path, member]));
  const observed = new Set();
  // bind every observed member once
  for (const member of inspected) {
    // reject duplicate derived paths
    if (observed.has(member.path)) {
      fail(`${image} extracted root contains duplicate path ${member.path}`);
    }
    observed.add(member.path);
    const expectedMember = remaining.get(member.path);
    // reject unexpected derived material
    if (expectedMember === undefined) {
      fail(`${image} extracted root contains unexpected file ${member.path}`);
    }
    // reject changed runtime bytes
    if (member.runtimeSha256 !== expectedMember.runtimeSha256) {
      fail(`${image} extracted file ${member.path} runtime digest does not match expected`);
    }
    // bind executable identity only after exact on-disk bytes match
    if (expectedMember.role === "binary") {
      requireArm64ElfExecutable(member);
    }
    remaining.delete(member.path);
  }
  // reject the first deterministic missing member
  if (remaining.size > 0) {
    const missing = remaining.keys().next().value;
    fail(`${image} extracted root is missing ${missing}`);
  }
  // copy provenance only after byte parity
  return expectedMembers.map((member) => structuredClone(member));
}

// inspect extracted application roots
export function inspectRustRuntimeImageMaterial(input, roots) {
  validateExpectedInput(input, true);
  const canonicalRoots = validateInspectionRoots(roots);
  const budget = { buffer: Buffer.allocUnsafe(HASH_BUFFER_BYTES), totalBytes: 0 };
  const serverFiles = inspectExtractedRoot(canonicalRoots.server, "server", budget);
  const webFiles = inspectExtractedRoot(canonicalRoots.web, "web", budget);
  const actual = {
    server: bindInspectedMembers(serverFiles, input.expected.server, "server"),
    web: bindInspectedMembers(webFiles, input.expected.web, "web"),
  };
  return buildMaterialManifest(
    input,
    actual,
    RUNTIME_IMAGE_MATERIAL_INSPECTION_CLASS,
    {
      extractedFileBytes: true,
      provesBuildIdentity: false,
      provesElfArchitecture: true,
      provesImageIdentity: false,
      provesPlatform: false,
      provesSourceIdentity: false,
    },
  );
}

// read one bounded local fixture
function readInput(path) {
  const absolute = resolve(path);
  const details = lstatSync(absolute);
  // reject links non-files and oversized fixtures
  if (
    !details.isFile()
    || details.isSymbolicLink()
    || details.size === 0
    || details.size > MAX_INPUT_BYTES
  ) {
    fail("input must be a bounded regular file");
  }
  return JSON.parse(readFileSync(absolute, "utf8"));
}

// run the offline preflight interface
function main(argv) {
  // run the extracted-tree inspection mode
  if (argv[0] === "--inspect") {
    // require one fixture and two roots
    if (argv.length !== 4) {
      fail("usage: rust-runtime-image-material.mjs --inspect INPUT.json SERVER_ROOT WEB_ROOT");
    }
    const manifest = inspectRustRuntimeImageMaterial(readInput(argv[1]), {
      server: argv[2],
      web: argv[3],
    });
    process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
    return;
  }
  // preserve the manifest-only interface
  if (argv.length !== 1) {
    fail("usage: rust-runtime-image-material.mjs INPUT.json");
  }
  const manifest = buildRustRuntimeImageMaterial(readInput(argv[0]));
  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
}

// execute only as the entry point
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`rust-runtime-image-material: ${error.message}\n`);
    process.exitCode = 1;
  }
}
