#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  closeSync,
  constants,
  fstatSync,
  lstatSync,
  openSync,
  readFileSync,
  readdirSync,
  realpathSync,
} from "node:fs";
import { execFileSync } from "node:child_process";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

const INPUT_VERSION = "weather-rust-runtime-image-material-input/v1";
const PROFILE = "release";
const TARGET = "aarch64-unknown-linux-gnu";
const MAX_FILE_BYTES = 512 * 1024 * 1024;
const MAX_TOTAL_BYTES = 2 * 1024 * 1024 * 1024;
const MAX_MEMBERS = 10000;
const MAX_GIT_OUTPUT_BYTES = 64 * 1024 * 1024;
const COMMIT_SHA = /^[a-f0-9]{40}$/u;
const AARCH64_ELF = Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1]);

const BINARY_SPECS = Object.freeze([
  Object.freeze({
    build: "bin/weather-api",
    image: "server",
    path: "/opt/weather/bin/weather-api",
    source: "rust/crates/weather-runtime/src/bin/weather-api.rs",
  }),
  Object.freeze({
    build: "bin/weather-http-health",
    image: "server",
    path: "/opt/weather/bin/weather-api-health",
    source: "rust/crates/weather-runtime/src/bin/weather-http-health.rs",
  }),
  Object.freeze({
    build: "bin/weather-migrate",
    image: "server",
    path: "/opt/weather/bin/weather-migrate",
    source: "rust/apps/weather-migrate/src/main.rs",
  }),
  Object.freeze({
    build: "bin/weather-worker",
    image: "server",
    path: "/opt/weather/bin/weather-worker",
    source: "rust/crates/weather-runtime/src/bin/weather-worker.rs",
  }),
  Object.freeze({
    build: "bin/weather-worker-health",
    image: "server",
    path: "/opt/weather/bin/weather-worker-health",
    source: "rust/crates/weather-runtime/src/bin/weather-worker-health.rs",
  }),
  Object.freeze({
    build: "bin/weather-web",
    image: "web",
    path: "/opt/weather/bin/weather-web",
    source: "rust/crates/weather-runtime/src/bin/weather-web.rs",
  }),
  Object.freeze({
    build: "bin/weather-http-health",
    image: "web",
    path: "/opt/weather/bin/weather-web-health",
    source: "rust/crates/weather-runtime/src/bin/weather-http-health.rs",
  }),
]);

const SOURCE_TREES = Object.freeze([
  Object.freeze({
    image: "server",
    role: "migration",
    runtime: "/opt/weather/packages/database/migrations",
    source: "packages/database/migrations",
  }),
  Object.freeze({
    image: "server",
    role: "config",
    runtime: "/opt/weather/config/ecowitt",
    source: "config/ecowitt",
  }),
  Object.freeze({
    image: "server",
    role: "model",
    runtime: "/opt/weather/config/forecast-adjustments",
    source: "config/forecast-adjustments",
  }),
  Object.freeze({
    image: "server",
    role: "config",
    runtime: "/opt/weather/config/public-stations",
    source: "config/public-stations",
  }),
  Object.freeze({
    image: "server",
    role: "config",
    runtime: "/opt/weather/config/sites",
    source: "config/sites",
  }),
  Object.freeze({
    image: "server",
    role: "config",
    runtime: "/opt/weather/config/tempest",
    source: "config/tempest",
  }),
  Object.freeze({
    image: "server",
    role: "config",
    runtime: "/opt/weather/config/tides",
    source: "config/tides",
  }),
  Object.freeze({
    image: "web",
    role: "static",
    runtime: "/opt/weather/apps/web/public",
    source: "apps/web/public",
  }),
]);

const SOURCE_FILES = Object.freeze([
  Object.freeze({
    image: "server",
    role: "config",
    runtime: "/opt/weather/config/runtime-targets.json",
    source: "config/runtime-targets.json",
  }),
  Object.freeze({
    image: "web",
    role: "config",
    runtime: "/opt/weather/edge-config/ballydidean.json",
    source: "config/sites/ballydidean.json",
  }),
]);

// normalize one generator failure
function fail(message) {
  throw new Error(message);
}

// hash exact material bytes
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// compare paths with the manifest's bytewise order
function comparePath(left, right) {
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

// resolve one real directory without accepting an alias
function exactRoot(value, label) {
  // require one explicit absolute or relative root
  if (typeof value !== "string" || value.length === 0) {
    fail(`${label} must be a nonempty path`);
  }
  const root = resolve(value);
  let details;
  // normalize inaccessible roots
  try {
    details = lstatSync(root);
  } catch {
    fail(`${label} is unavailable`);
  }
  // prohibit links and non-directories at the authority boundary
  if (!details.isDirectory() || details.isSymbolicLink() || realpathSync(root) !== root) {
    fail(`${label} must be a real canonical directory`);
  }
  return root;
}

// keep one child path inside its admitted root
function contained(root, child, label) {
  const path = resolve(root, child);
  const suffix = relative(root, path);
  // reject absolute traversal or a path that leaves the root
  if (suffix === "" || isAbsolute(suffix) || suffix === ".." || suffix.startsWith(`..${sep}`)) {
    fail(`${label} escapes its root`);
  }
  return path;
}

// read one bounded stable regular file without following its final entry
function readStableFile(root, child, budget, label, executable = false) {
  const path = contained(root, child, label);
  let descriptor;
  // open only one non-link file
  try {
    descriptor = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  } catch {
    fail(`${label} cannot be opened`);
  }
  try {
    const before = fstatSync(descriptor);
    // close file type size and aggregate allocation before reading
    if (!before.isFile()
      || before.size === 0
      || before.size > MAX_FILE_BYTES
      || budget.bytes + before.size > MAX_TOTAL_BYTES
      || (executable && (before.mode & 0o111) === 0)) {
      fail(`${label} is not an admitted bounded${executable ? " executable" : ""} file`);
    }
    const bytes = readFileSync(descriptor);
    const after = fstatSync(descriptor);
    // reject replacement or mutation across the read
    if (bytes.length !== before.size
      || before.dev !== after.dev
      || before.ino !== after.ino
      || before.size !== after.size
      || before.mtimeMs !== after.mtimeMs
      || before.ctimeMs !== after.ctimeMs) {
      fail(`${label} changed during generation`);
    }
    budget.bytes += bytes.length;
    return bytes;
  } finally {
    closeSync(descriptor);
  }
}

// execute one bounded Git query against the explicit source root
function git(sourceRoot, operands, encoding = null) {
  try {
    return execFileSync("/usr/bin/git", ["-C", sourceRoot, ...operands], {
      encoding,
      env: { HOME: "/nonexistent", LANG: "C", LC_ALL: "C", PATH: "/usr/bin:/bin" },
      maxBuffer: MAX_GIT_OUTPUT_BYTES,
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch {
    fail(`git ${operands[0]} failed for the source checkout`);
  }
}

// admit one exact clean checkout and derive its canonical Git tree inventory digest
function sourceIdentity(sourceRoot, sourceCommit) {
  // close the externally supplied immutable commit before invoking Git
  if (typeof sourceCommit !== "string" || !COMMIT_SHA.test(sourceCommit)) {
    fail("source commit must be a lowercase 40-character commit SHA");
  }
  const top = git(sourceRoot, ["rev-parse", "--show-toplevel"], "utf8").trim();
  const head = git(sourceRoot, ["rev-parse", "HEAD"], "utf8").trim();
  const status = git(sourceRoot, ["status", "--porcelain=v1", "--untracked-files=all"], "utf8");
  // require the named root, commit and complete non-ignored checkout to agree
  if (resolve(top) !== sourceRoot || head !== sourceCommit || status !== "") {
    fail("source root must be an exact clean checkout of source commit");
  }
  const tree = git(sourceRoot, ["ls-tree", "-r", "-z", "--full-tree", sourceCommit]);
  // refuse an empty or unexpectedly unbounded repository inventory
  if (!Buffer.isBuffer(tree) || tree.length === 0 || tree.length > MAX_GIT_OUTPUT_BYTES) {
    fail("source tree inventory is invalid");
  }
  return { commit: sourceCommit, treeSha256: sha256(tree) };
}

// validate one exported native executable before using its digest as expected runtime material
function requireAarch64Elf(bytes, label) {
  // inspect only the fixed ELF identity fields required before image inspection
  if (bytes.length < 64
    || !bytes.subarray(0, AARCH64_ELF.length).equals(AARCH64_ELF)
    || bytes.readUInt16LE(18) !== 183
    || bytes.readUInt32LE(20) !== 1
    || ![2, 3].includes(bytes.readUInt16LE(16))) {
    fail(`${label} must be an AArch64 ELF64 executable`);
  }
}

// walk one fixed source or build directory without following links
function walkFiles(root, child, budget, label) {
  const start = contained(root, child, label);
  let startDetails;
  // normalize a missing fixed tree
  try {
    startDetails = lstatSync(start);
  } catch {
    fail(`${label} is unavailable`);
  }
  // require an ordinary directory rather than an aliased tree
  if (!startDetails.isDirectory() || startDetails.isSymbolicLink() || realpathSync(start) !== start) {
    fail(`${label} must be a real directory`);
  }
  const files = [];
  const visit = (directory, prefix) => {
    const entries = readdirSync(directory, { withFileTypes: true })
      .sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0);
    // inspect every fixed-tree member once
    for (const entry of entries) {
      const relativePath = prefix === "" ? entry.name : `${prefix}/${entry.name}`;
      const absolute = contained(start, relativePath, label);
      // recurse only through real directories
      if (entry.isDirectory() && !entry.isSymbolicLink()) {
        if (realpathSync(absolute) !== absolute) {
          fail(`${label} contains an aliased directory`);
        }
        visit(absolute, relativePath);
        continue;
      }
      // reject links and special members before reading
      if (!entry.isFile() || entry.isSymbolicLink()) {
        fail(`${label} contains a non-regular member`);
      }
      const bytes = readStableFile(start, relativePath, budget, `${label}/${relativePath}`);
      files.push({ bytes, relativePath });
      // bound membership while traversing rather than after allocation
      if (files.length > MAX_MEMBERS) {
        fail(`${label} exceeds the member limit`);
      }
    }
  };
  visit(start, "");
  // every copied Docker tree must contain actual material
  if (files.length === 0) {
    fail(`${label} must contain files`);
  }
  return files;
}

// freeze one expected member with its distinct source build and runtime identities
function member(path, role, sourcePath, sourceSha256, buildSha256, runtimeSha256) {
  return { buildSha256, path, role, runtimeSha256, sourcePath, sourceSha256 };
}

// hash one exact tracked source file and bind it to the named Git object
function sourceFile(sourceRoot, sourceCommit, sourcePath, budget) {
  const bytes = readStableFile(sourceRoot, sourcePath, budget, `source ${sourcePath}`);
  const retained = git(sourceRoot, ["show", `${sourceCommit}:${sourcePath}`]);
  // require the Docker-context bytes to equal the immutable Git member
  if (!Buffer.isBuffer(retained) || !bytes.equals(retained)) {
    fail(`source ${sourcePath} differs from source commit`);
  }
  return bytes;
}

// require the independent export to contain only the fixed build products
function validateBuildExport(buildRoot) {
  const top = readdirSync(buildRoot).sort();
  const binaries = readdirSync(contained(buildRoot, "bin", "build bin")).sort();
  const expectedBinaries = [...new Set(BINARY_SPECS.map((spec) => spec.build.slice(4)))].sort();
  // reject extra export roots or binary products that could widen image membership
  if (JSON.stringify(top) !== JSON.stringify(["bin", "web-dist"])
    || JSON.stringify(binaries) !== JSON.stringify(expectedBinaries)) {
    fail("build export membership differs from the fixed native build stage");
  }
}

// build the exact existing inspector input from independent build products and one source checkout
export function buildNativeRuntimeImageMaterialInput({ sourceCommit, sourceRoot, buildRoot }) {
  const source = exactRoot(sourceRoot, "source root");
  const build = exactRoot(buildRoot, "build root");
  // preserve independent authority rather than accepting a nested checkout/build export
  if (source === build || source.startsWith(`${build}${sep}`) || build.startsWith(`${source}${sep}`)) {
    fail("source and build roots must be disjoint");
  }
  const identity = sourceIdentity(source, sourceCommit);
  validateBuildExport(build);
  const budget = { bytes: 0 };
  const expected = { server: [], web: [] };
  // bind each accepted native command to its independent build bytes and tracked source entry
  for (const spec of BINARY_SPECS) {
    const buildBytes = readStableFile(build, spec.build, budget, `build ${spec.build}`, true);
    requireAarch64Elf(buildBytes, `build ${spec.build}`);
    const sourceBytes = sourceFile(source, sourceCommit, spec.source, budget);
    const buildSha256 = sha256(buildBytes);
    expected[spec.image].push(member(
      spec.path,
      "binary",
      spec.source,
      sha256(sourceBytes),
      buildSha256,
      buildSha256,
    ));
  }
  // project every exact source tree copied by the native runtime stages
  for (const spec of SOURCE_TREES) {
    const files = walkFiles(source, spec.source, budget, `source ${spec.source}`);
    // retain every recursively copied source member
    for (const file of files) {
      const sourcePath = `${spec.source}/${file.relativePath}`;
      const retained = git(source, ["show", `${sourceCommit}:${sourcePath}`]);
      // prohibit untracked or changed material inside a copied tree
      if (!Buffer.isBuffer(retained) || !file.bytes.equals(retained)) {
        fail(`source ${sourcePath} differs from source commit`);
      }
      const digest = sha256(file.bytes);
      expected[spec.image].push(member(
        `${spec.runtime}/${file.relativePath}`,
        spec.role,
        sourcePath,
        digest,
        digest,
        digest,
      ));
    }
  }
  // project fixed singleton source members with their Docker runtime renames
  for (const spec of SOURCE_FILES) {
    const bytes = sourceFile(source, sourceCommit, spec.source, budget);
    const digest = sha256(bytes);
    expected[spec.image].push(member(
      spec.runtime,
      spec.role,
      spec.source,
      digest,
      digest,
      digest,
    ));
  }
  const webDist = walkFiles(build, "web-dist", budget, "build web-dist");
  // bind generated static output to the independent Node build-stage export
  for (const file of webDist) {
    const digest = sha256(file.bytes);
    expected.web.push(member(
      `/opt/weather/apps/web/dist/${file.relativePath}`,
      "static",
      `apps/web/dist/${file.relativePath}`,
      digest,
      digest,
      digest,
    ));
  }
  expected.server.sort(comparePath);
  expected.web.sort(comparePath);
  // close total expected membership before constructing the build identity
  if (expected.server.length + expected.web.length > MAX_MEMBERS) {
    fail("expected runtime material exceeds the member limit");
  }
  const buildPreimage = Buffer.from(`${JSON.stringify({
    expected,
    profile: PROFILE,
    source: identity,
    target: TARGET,
  })}\n`, "utf8");
  return {
    build: {
      identitySha256: sha256(buildPreimage),
      profile: PROFILE,
      target: TARGET,
    },
    expected,
    inputVersion: INPUT_VERSION,
    source: identity,
  };
}

// execute the deterministic stdout-only generator interface
function main(argv) {
  // require the immutable source identity and two disjoint material roots
  if (argv.length !== 3) {
    fail("usage: build-native-runtime-image-material.mjs SOURCE_COMMIT SOURCE_ROOT BUILD_ROOT");
  }
  const input = buildNativeRuntimeImageMaterialInput({
    sourceCommit: argv[0],
    sourceRoot: argv[1],
    buildRoot: argv[2],
  });
  process.stdout.write(`${JSON.stringify(input, null, 2)}\n`);
}

// run only when selected as the entry point
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`build-native-runtime-image-material: ${error.message}\n`);
    process.exitCode = 1;
  }
}
