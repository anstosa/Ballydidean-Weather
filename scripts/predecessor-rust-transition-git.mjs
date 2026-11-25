#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  APP_QUERY_TRANSITION,
} from "./schema-preserving-release.mjs";
import {
  WP8B_TARGET_ADDED_FILES,
  WP8B_TARGET_ONLY_ROOTS,
  sha256,
  treeSha256,
  validateReviewedTransitionDeclaration,
} from "./predecessor-rust-transition.mjs";

export const GIT_ADAPTER_VERSION = "weather-predecessor-rust-git-adapter/v1";
export const TARGET_ADDED_FILES = WP8B_TARGET_ADDED_FILES;
export const TARGET_ONLY_ROOTS = WP8B_TARGET_ONLY_ROOTS;

const COMMIT_SHA = /^[a-f0-9]{40}$/u;
const SAFE_PATH = /^[\x20-\x7e]{1,512}$/u;
const WILDCARD = /[*?\[\]{}]/u;
const MAX_GIT_BYTES = 16 * 1024 * 1024;
const MAX_MEMBERS = 100000;
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
const DECLARED_PROTECTED_PREFIXES = [
  "apps/api/",
  "apps/web/",
  "apps/worker/",
  "packages/",
  "rust/",
  "scripts/",
];
const DECLARED_PROTECTED_FILES = new Set([
  ".github/runtime-generation",
]);
const REQUIRED_PROTECTED_PATHS = [
  "Dockerfile",
  "deploy/scripts/common.sh",
  "deploy/scripts/update.sh",
  "package-lock.json",
  "packages/database/migrations/0020_adjustment_revision_frontier.sql",
  "packages/database/migrations/0023_forecast_product_read_index.sql",
  "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
];
const REVIEWED_EXISTING_TRANSITIONS = [APP_QUERY_TRANSITION];

// normalize adapter failures
function fail(message) {
  throw new Error(message);
}

// validate one exact commit id
function validateCommit(commit, label) {
  // reject refs and abbreviated ids
  if (typeof commit !== "string" || !COMMIT_SHA.test(commit)) {
    fail(`${label} must be an exact lowercase commit SHA`);
  }
  return commit;
}

// validate one committed path
function validatePath(path) {
  // reject noncanonical object names
  if (
    typeof path !== "string"
    || !SAFE_PATH.test(path)
    || path.startsWith("/")
    || path.endsWith("/")
    || path.includes("\\")
    || WILDCARD.test(path)
    || path.split("/").some((part) => part === "" || part === "." || part === "..")
  ) {
    fail("Git tree contains a noncanonical path");
  }
  return path;
}

// recognize the fixed protected surface
function protectedPath(path) {
  return PROTECTED_FILES.has(path)
    || PROTECTED_PREFIXES.some((prefix) => path.startsWith(prefix));
}

// recognize the fixed declared-transition source surface
function declaredProtectedPath(path) {
  return protectedPath(path)
    || DECLARED_PROTECTED_FILES.has(path)
    || DECLARED_PROTECTED_PREFIXES.some((prefix) => path.startsWith(prefix));
}

// recognize one fixed target root member
function targetRoot(path) {
  return TARGET_ONLY_ROOTS.find((root) => path.startsWith(`${root}/`));
}

// recognize the fixed target surface
function targetPath(path, declaration = null) {
  return protectedPath(path)
    || (declaration !== null && declaredProtectedPath(path))
    || TARGET_ADDED_FILES.includes(path)
    || targetRoot(path) !== undefined
    || declaration?.addedFiles.some((member) => member.path === path)
    || declaration?.changedMembers.some((member) => member.path === path);
}

// run one fixed Git object command
function runGitCommand(args, cwd) {
  try {
    return execFileSync("git", args, {
      cwd,
      encoding: null,
      maxBuffer: MAX_GIT_BYTES,
      shell: false,
    });
  } catch {
    fail(`git ${args[0] ?? "object read"} failed`);
  }
}

// require bounded command bytes
function commandBytes(value, label) {
  // reject injected or oversized output
  if (!Buffer.isBuffer(value) || value.length > MAX_GIT_BYTES) {
    fail(`${label} output is invalid or oversized`);
  }
  return value;
}

// resolve one exact commit object
function resolveCommit(commit, runGit) {
  const resolved = commandBytes(
    runGit(["rev-parse", "--verify", `${commit}^{commit}`]),
    "git rev-parse",
  ).toString("utf8").trim();
  // reject replacement or indirection
  if (resolved !== commit) {
    fail("Git did not resolve the requested exact commit");
  }
}

// require one forward commit ancestry
function requireDescendant(predecessorCommit, targetCommit, runGit) {
  const output = commandBytes(
    runGit(["merge-base", "--is-ancestor", predecessorCommit, targetCommit]),
    "git merge-base",
  );
  // reject unexpected command output
  if (output.length !== 0) {
    fail("Git ancestry check produced unexpected output");
  }
}

// list every path in one commit tree
function listTree(commit, runGit) {
  const listing = commandBytes(
    runGit(["ls-tree", "-r", "-z", "--name-only", commit, "--"]),
    "git ls-tree",
  );
  // require one nul-terminated listing
  if (listing.length === 0 || listing.at(-1) !== 0) {
    fail("Git tree listing is empty or malformed");
  }
  const paths = listing.toString("utf8").split("\0").slice(0, -1);
  // bound the full tree before filtering
  if (paths.length === 0 || paths.length > MAX_MEMBERS) {
    fail("Git tree membership is out of bounds");
  }
  let previous = "";
  // validate canonical Git order
  for (const path of paths) {
    validatePath(path);
    // reject duplicate or unsorted tree output
    if (previous !== "" && previous >= path) {
      fail("Git tree paths are not sorted and unique");
    }
    previous = path;
  }
  return paths;
}

// hash one exact committed blob
function memberFromGit(commit, path, runGit) {
  const bytes = commandBytes(
    runGit(["cat-file", "blob", `${commit}:${path}`]),
    "git cat-file",
  );
  return { path, sha256: sha256(bytes) };
}

// collect one fixed committed surface
function collectMembers(commit, paths, predicate, runGit) {
  const members = [];
  // hash every selected object byte sequence
  for (const path of paths) {
    // skip material outside the fixed surface
    if (!predicate(path)) {
      continue;
    }
    members.push(memberFromGit(commit, path, runGit));
  }
  // reject empty or oversized selections
  if (members.length === 0 || members.length > MAX_MEMBERS) {
    fail("selected Git tree membership is out of bounds");
  }
  return members;
}

// require all prior protected sentinels
function requireProtectedSentinels(members, label) {
  const paths = new Set(members.map((member) => member.path));
  // inspect every retained sentinel
  for (const requiredPath of REQUIRED_PROTECTED_PATHS) {
    // reject partial protected surfaces
    if (!paths.has(requiredPath)) {
      fail(`${label} is missing required protected path ${requiredPath}`);
    }
  }
}

// require target additions to be source-absent
function requireSourceAbsence(paths, declaration = null) {
  // inspect exact added files
  for (const path of TARGET_ADDED_FILES) {
    // reject predecessor contamination
    if (paths.includes(path)) {
      fail(`predecessor already contains target-only file ${path}`);
    }
  }
  // inspect exact added roots
  for (const root of TARGET_ONLY_ROOTS) {
    // reject predecessor root contamination
    if (paths.some((path) => path.startsWith(`${root}/`))) {
      fail(`predecessor already contains target-only root ${root}`);
    }
  }
  // inspect every explicit finite reviewed addition
  for (const member of declaration?.addedFiles ?? []) {
    if (paths.includes(member.path)) {
      fail(`predecessor already contains reviewed addition ${member.path}`);
    }
  }
}

// expand every fixed target-only root
function expandTargetRoots(targetMembers) {
  const roots = [];
  // build each exact sorted root expansion
  for (const root of TARGET_ONLY_ROOTS) {
    const members = targetMembers.filter((member) => member.path.startsWith(`${root}/`));
    // reject incomplete named roots
    if (members.length === 0) {
      fail(`target is missing target-only root ${root}`);
    }
    roots.push({ members, root, treeSha256: treeSha256(members) });
  }
  return roots;
}

// select exact target-only files
function selectAddedFiles(targetMembers, declaration = null) {
  const byPath = new Map(targetMembers.map((member) => [member.path, member]));
  const addedFiles = [];
  // bind every fixed added file
  for (const path of TARGET_ADDED_FILES) {
    const member = byPath.get(path);
    // reject missing named files
    if (member === undefined) {
      fail(`target is missing target-only file ${path}`);
    }
    addedFiles.push(member);
  }
  // bind every additional reviewed file to the exact target object digest
  for (const reviewed of declaration?.addedFiles ?? []) {
    const member = byPath.get(reviewed.path);
    if (member === undefined || member.sha256 !== reviewed.sha256) {
      fail(`target reviewed addition ${reviewed.path} differs from its declaration`);
    }
    addedFiles.push(member);
  }
  return addedFiles.sort((left, right) => left.path < right.path ? -1 : left.path > right.path ? 1 : 0);
}

// match one reviewed existing-member transition
function reviewedTransition(path, predecessorSha256, targetSha256, declaration = null) {
  const declared = declaration?.changedMembers.find((transition) => transition.path === path);
  // an explicit declaration for this path must match even when a historical fixed review exists
  if (declared !== undefined) {
    return declared.predecessorSha256 === predecessorSha256
      && declared.targetSha256 === targetSha256
      ? declared
      : undefined;
  }
  return REVIEWED_EXISTING_TRANSITIONS.find((transition) => (
    transition.path === path
    && transition.sourceSha256 === predecessorSha256
    && transition.targetSha256 === targetSha256
  ));
}

// compare the fixed existing protected surface
function compareProtectedMembers(predecessorMembers, targetMembers, declaration = null) {
  const target = new Map(targetMembers.map((member) => [member.path, member.sha256]));
  const changedMembers = [];
  const remainingReviewed = new Set(
    (declaration?.changedMembers ?? []).map((member) => member.path),
  );
  // inspect every predecessor member
  for (const member of predecessorMembers) {
    const targetSha256 = target.get(member.path);
    // reject protected deletion
    if (targetSha256 === undefined) {
      fail(`target removed protected member ${member.path}`);
    }
    // keep exact retained bytes implicit
    if (targetSha256 === member.sha256) {
      continue;
    }
    // reject every unreviewed existing-member change
    if (reviewedTransition(member.path, member.sha256, targetSha256, declaration) === undefined) {
      fail(`target changed protected member ${member.path} outside a fixed reviewed transition`);
    }
    changedMembers.push({
      path: member.path,
      predecessorSha256: member.sha256,
      targetSha256,
    });
    remainingReviewed.delete(member.path);
  }
  // require every explicit changed member to exist and change exactly once
  if (remainingReviewed.size !== 0) {
    fail(`reviewed changed member ${remainingReviewed.values().next().value} is absent`);
  }
  return changedMembers;
}

// reject target protected additions outside fixed declarations
function requireNoExtraProtectedAdditions(predecessorMembers, targetMembers, declaration = null) {
  const predecessor = new Set(predecessorMembers.map((member) => member.path));
  // inspect every target member
  for (const member of targetMembers) {
    // skip retained paths and exact additions
    if (
      predecessor.has(member.path)
      || TARGET_ADDED_FILES.includes(member.path)
      || targetRoot(member.path) !== undefined
      || declaration?.addedFiles.some((reviewed) => reviewed.path === member.path)
    ) {
      continue;
    }
    fail(`target added undeclared protected member ${member.path}`);
  }
}

// derive one fixed transition from Git object bytes
function deriveTransition(
  predecessorCommit,
  targetCommit,
  runGit,
  evidenceMode,
  declaration = null,
) {
  validateCommit(predecessorCommit, "predecessorCommit");
  validateCommit(targetCommit, "targetCommit");
  // require distinct object identities
  if (predecessorCommit === targetCommit) {
    fail("predecessorCommit and targetCommit must differ");
  }
  // validate an optional review document before reading any commit objects
  if (declaration !== null) {
    validateReviewedTransitionDeclaration(declaration);
    if (declaration.predecessorCommit !== predecessorCommit
      || declaration.targetCommit !== targetCommit) {
      fail("reviewed declaration does not match requested commits");
    }
  }
  resolveCommit(predecessorCommit, runGit);
  resolveCommit(targetCommit, runGit);
  requireDescendant(predecessorCommit, targetCommit, runGit);
  const predecessorPaths = listTree(predecessorCommit, runGit);
  const targetPaths = listTree(targetCommit, runGit);
  requireSourceAbsence(predecessorPaths, declaration);
  const predecessorMembers = collectMembers(
    predecessorCommit,
    predecessorPaths,
    (path) => protectedPath(path)
      || (declaration !== null && declaredProtectedPath(path))
      || declaration?.changedMembers.some((member) => member.path === path),
    runGit,
  );
  const targetMembers = collectMembers(
    targetCommit,
    targetPaths,
    (path) => targetPath(path, declaration),
    runGit,
  );
  requireProtectedSentinels(predecessorMembers, "predecessorMembers");
  requireProtectedSentinels(targetMembers, "targetMembers");
  requireNoExtraProtectedAdditions(predecessorMembers, targetMembers, declaration);
  const changedMembers = compareProtectedMembers(predecessorMembers, targetMembers, declaration);
  const addedFiles = selectAddedFiles(targetMembers, declaration);
  const targetOnlyRoots = expandTargetRoots(targetMembers);
  const evidence = {
    adapterVersion: GIT_ADAPTER_VERSION,
    addedFiles,
    changedMembers,
    evidenceMode,
    predecessorCommit,
    predecessorMembers,
    targetCommit,
    targetMembers,
    targetOnlyRoots,
  };
  // preserve the reviewed baseline evidence shape exactly unless extension is explicit
  if (declaration !== null) {
    evidence.reviewedDeclaration = structuredClone(declaration);
  }
  return evidence;
}

// snapshot the fixed protected surface from Git
export function snapshotGitCommit(commit, cwd = process.cwd()) {
  validateCommit(commit, "commit");
  const repository = resolve(cwd);
  // bind the runner to one repository
  function runGit(args) {
    return runGitCommand(args, repository);
  }
  resolveCommit(commit, runGit);
  const paths = listTree(commit, runGit);
  const members = collectMembers(commit, paths, protectedPath, runGit);
  requireProtectedSentinels(members, "members");
  return {
    adapterVersion: GIT_ADAPTER_VERSION,
    commit,
    evidenceMode: "git-object-database",
    members,
  };
}

// derive a production Git-object transition
export function deriveGitTransitionMembers(predecessorCommit, targetCommit, cwd = process.cwd()) {
  const repository = resolve(cwd);
  // bind the runner to one repository
  function runGit(args) {
    return runGitCommand(args, repository);
  }
  return deriveTransition(
    predecessorCommit,
    targetCommit,
    runGit,
    "git-object-database",
  );
}

// derive an extended transition only from one explicit content-addressed review declaration
export function deriveDeclaredGitTransitionMembers(
  predecessorCommit,
  targetCommit,
  declaration,
  cwd = process.cwd(),
) {
  const repository = resolve(cwd);
  // bind the runner to one repository
  function runGit(args) {
    return runGitCommand(args, repository);
  }
  return deriveTransition(
    predecessorCommit,
    targetCommit,
    runGit,
    "git-object-database",
    declaration,
  );
}

// derive isolated injected fixture evidence
export function deriveGitTransitionFixture(predecessorCommit, targetCommit, fixtureRunGit) {
  // require an explicit fixture runner
  if (typeof fixtureRunGit !== "function") {
    fail("fixtureRunGit must be a function");
  }
  // mark every injected read as fixture-only
  function runGit(args) {
    return fixtureRunGit([...args]);
  }
  return deriveTransition(predecessorCommit, targetCommit, runGit, "injected-test-fixture");
}

// derive isolated declared fixture evidence without granting it production authority
export function deriveDeclaredGitTransitionFixture(
  predecessorCommit,
  targetCommit,
  declaration,
  fixtureRunGit,
) {
  // require an explicit fixture runner
  if (typeof fixtureRunGit !== "function") {
    fail("fixtureRunGit must be a function");
  }
  // keep injected reads visibly fixture-only
  function runGit(args) {
    return fixtureRunGit([...args]);
  }
  return deriveTransition(
    predecessorCommit,
    targetCommit,
    runGit,
    "injected-test-fixture",
    declaration,
  );
}

// run the read-only adapter interface
function main(argv) {
  const [command, ...args] = argv;
  // snapshot one exact commit
  if (command === "snapshot" && args.length === 1) {
    process.stdout.write(`${JSON.stringify(snapshotGitCommit(args[0]), null, 2)}\n`);
    return;
  }
  // compare two exact commits
  if (command === "transition" && args.length === 2) {
    process.stdout.write(`${JSON.stringify(deriveGitTransitionMembers(args[0], args[1]), null, 2)}\n`);
    return;
  }
  fail("usage: predecessor-rust-transition-git.mjs snapshot COMMIT | transition PREDECESSOR TARGET");
}

// execute only as the entry point
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`predecessor-rust-transition-git: ${error.message}\n`);
    process.exitCode = 1;
  }
}
