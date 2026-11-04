#!/usr/bin/env node

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  CONTRACT_VERSION,
  SOURCE_COMMIT,
  validatePublication,
  validateReleaseName,
  writeJsonCreateOnce,
} from "./schema-preserving-release.mjs";

const REPOSITORY = "anstosa/Ballydidean-Weather";
const MAX_BYTES = 8 * 1024 * 1024;
const PROTECTED_PREFIXES = [
  ".github/workflows/", "config/", "deploy/", "packages/database/", "packages/forecast-adjustment/",
];
const PROTECTED_FILES = new Set([".dockerignore", "Dockerfile", "package-lock.json", "package.json"]);
const BUNDLE_FILES = [
  ["operatorSha256", "scripts/schema-preserving-release.sh"],
  ["validatorSha256", "scripts/schema-preserving-release.mjs"],
];

// run one bounded local command
function runCommand(command, args, { cwd } = {}) {
  try {
    return execFileSync(command, args, { cwd, encoding: null, maxBuffer: MAX_BYTES });
  } catch {
    // keep failures secret-free
    throw new Error(`${command} ${args[0] ?? ""} failed`);
  }
}

// parse bounded command json
function commandJson(bytes, label) {
  // reject oversized evidence
  if (!Buffer.isBuffer(bytes) || bytes.length === 0 || bytes.length > MAX_BYTES) {
    throw new Error(`${label} output is out of bounds`);
  }
  try {
    return JSON.parse(bytes.toString("utf8"));
  } catch {
    throw new Error(`${label} output is not JSON`);
  }
}

// hash exact bytes
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// recognize the protected source surface
function protectedPath(path) {
  return PROTECTED_FILES.has(path) || PROTECTED_PREFIXES.some((prefix) => path.startsWith(prefix));
}

// resolve an exact remote tag commit
function remoteTagCommit(release, run, cwd) {
  let object = commandJson(run("gh", [
    "api", "--method", "GET", `repos/${REPOSITORY}/git/ref/tags/${encodeURIComponent(release)}`,
  ], { cwd }), "remote tag").object;
  // peel bounded annotated tags
  for (let depth = 0; depth < 4 && object?.type === "tag"; depth += 1) {
    object = commandJson(run("gh", [
      "api", "--method", "GET", `repos/${REPOSITORY}/git/tags/${object.sha}`,
    ], { cwd }), "remote annotated tag").object;
  }
  // require a commit target
  if (object?.type !== "commit" || !/^[a-f0-9]{40}$/u.test(object.sha ?? "")) {
    throw new Error("remote tag does not resolve to a commit");
  }
  return object.sha;
}

// collect content hashes from one git tree
function protectedTree(commit, run, cwd) {
  const listing = run("git", ["ls-tree", "-r", "-z", "--name-only", commit, "--"], { cwd });
  // require nul-terminated output
  if (!Buffer.isBuffer(listing) || listing.length === 0 || listing.at(-1) !== 0) {
    throw new Error("git tree listing is malformed");
  }
  const paths = listing.toString("utf8").split("\0").slice(0, -1).filter(protectedPath).sort();
  // reject empty or duplicate trees
  if (paths.length === 0 || paths.some((path, index) => index > 0 && path === paths[index - 1])) {
    throw new Error("protected git tree is empty or duplicated");
  }
  return paths.map((path) => ({
    path,
    sha256: sha256(run("git", ["show", `${commit}:${path}`], { cwd })),
  }));
}

// select one exact workflow run
function workflowRun(workflow, expected, run, cwd) {
  const document = commandJson(run("gh", [
    "run", "list", "--repo", REPOSITORY, "--commit", expected.headSha, "--event", "push",
    "--workflow", workflow, "--limit", "100",
    "--json", "conclusion,databaseId,event,headBranch,headSha,name,status,url",
  ], { cwd }), `${expected.name} runs`);
  const matches = Array.isArray(document) ? document.filter((candidate) => (
    candidate?.name === expected.name && candidate.headSha === expected.headSha
    && candidate.status === "completed" && candidate.conclusion === "success"
    && candidate.event === "push"
    && (expected.headBranch === undefined || candidate.headBranch === expected.headBranch)
  )) : [];
  // reject missing or ambiguous runs
  if (matches.length !== 1) {
    throw new Error(`${expected.name} evidence is missing or ambiguous`);
  }
  const selected = matches[0];
  const normalized = {
    conclusion: selected.conclusion,
    databaseId: selected.databaseId,
    event: selected.event,
    headSha: selected.headSha,
    name: selected.name,
    status: selected.status,
    url: selected.url,
  };
  // retain only the publish tag
  if (expected.headBranch !== undefined) {
    normalized.headBranch = selected.headBranch;
  }
  return normalized;
}

// prove the image boundary step succeeded
function validatePublishJob(runId, run, cwd) {
  const document = commandJson(run("gh", [
    "run", "view", String(runId), "--repo", REPOSITORY, "--json", "jobs",
  ], { cwd }), "publish jobs");
  const steps = Array.isArray(document.jobs)
    ? document.jobs.flatMap((job) => Array.isArray(job.steps) ? job.steps : [])
    : [];
  const matches = steps.filter((step) => step?.name === "Verify exact production image boundary");
  // require one successful boundary step
  if (matches.length !== 1 || matches[0].status !== "completed" || matches[0].conclusion !== "success") {
    throw new Error("published image boundary was not proven");
  }
}

// require two bounded workflow artifacts
function validateArtifacts(runId, run, cwd) {
  const document = commandJson(run("gh", [
    "api", "--method", "GET", `repos/${REPOSITORY}/actions/runs/${runId}/artifacts?per_page=100`,
  ], { cwd }), "publish artifacts");
  // inspect each required artifact
  for (const name of ["server-image-digest", "web-image-digest"]) {
    const matches = Array.isArray(document.artifacts)
      ? document.artifacts.filter((artifact) => artifact?.name === name)
      : [];
    // reject missing expired or oversized artifacts
    if (matches.length !== 1 || matches[0].expired !== false
      || !Number.isSafeInteger(matches[0].size_in_bytes)
      || matches[0].size_in_bytes <= 0 || matches[0].size_in_bytes > 4096) {
      throw new Error(`${name} artifact is unavailable or oversized`);
    }
  }
}

// read one downloaded image reference
function artifactReference(directory, target) {
  const name = `${target}-image-digest.env`;
  const entries = readdirSync(directory, { withFileTypes: true });
  // require exactly one regular artifact
  if (entries.length !== 1 || entries[0].name !== name || !entries[0].isFile()) {
    throw new Error(`${target} artifact shape is invalid`);
  }
  const path = join(directory, name);
  const details = lstatSync(path);
  const bytes = readFileSync(path);
  // bound artifact metadata
  if (!details.isFile() || details.isSymbolicLink() || details.nlink !== 1 || bytes.length > 512) {
    throw new Error(`${target} artifact metadata is invalid`);
  }
  const variable = `WEATHER_${target.toUpperCase()}_IMAGE`;
  const pattern = new RegExp(
    `^${variable}=(ghcr\\.io/anstosa/weather-${target}@sha256:[a-f0-9]{64})\\n?$`,
    "u",
  );
  const match = pattern.exec(bytes.toString("utf8"));
  // require one exact environment field
  if (match === null) {
    throw new Error(`${target} artifact reference is invalid`);
  }
  return match[1];
}

// inspect one remote image config
function imageConfig(reference, run, cwd) {
  const manifest = commandJson(run("docker", [
    "buildx", "imagetools", "inspect", reference, "--raw",
  ], { cwd }), "remote image manifest");
  // require an immutable config identity
  if (!/^sha256:[a-f0-9]{64}$/u.test(manifest?.config?.digest ?? "")) {
    throw new Error("remote image config digest is invalid");
  }
  return manifest.config.digest;
}

// prepare one reproducible publication proof
export function preparePublication(release, output, options = {}) {
  validateReleaseName(release);
  const cwd = resolve(options.cwd ?? process.cwd());
  const run = options.run ?? runCommand;
  const localTag = run("git", ["rev-parse", "--verify", `refs/tags/${release}^{commit}`], { cwd })
    .toString("utf8").trim();
  const head = run("git", ["rev-parse", "HEAD"], { cwd }).toString("utf8").trim();
  const remoteTag = remoteTagCommit(release, run, cwd);
  // require one local and remote target
  if (!/^[a-f0-9]{40}$/u.test(localTag) || localTag !== head || remoteTag !== localTag) {
    throw new Error("local tag, remote tag and HEAD do not identify one commit");
  }
  const check = workflowRun("Check", { headSha: head, name: "Check" }, run, cwd);
  const publish = workflowRun("publish-images.yml", {
    headBranch: release, headSha: head, name: "Publish production images",
  }, run, cwd);
  validatePublishJob(publish.databaseId, run, cwd);
  validateArtifacts(publish.databaseId, run, cwd);
  const bundle = {};
  // bind committed auxiliary bytes
  for (const [field, path] of BUNDLE_FILES) {
    const committed = run("git", ["show", `${head}:${path}`], { cwd });
    // reject local bundle drift
    if (!Buffer.isBuffer(committed) || !committed.equals(readFileSync(resolve(cwd, path)))) {
      throw new Error(`${path} differs from the target commit`);
    }
    bundle[field] = sha256(committed);
  }
  const ownedTemporary = mkdtempSync(join(tmpdir(), "weather-schema-publication-"));
  try {
    const references = {};
    // download only the two digest artifacts
    for (const target of ["server", "web"]) {
      const directory = join(ownedTemporary, target);
      mkdirSync(directory, { mode: 0o700 });
      run("gh", [
        "run", "download", String(publish.databaseId), "--repo", REPOSITORY,
        "--name", `${target}-image-digest`, "--dir", directory,
      ], { cwd });
      references[target] = artifactReference(directory, target);
    }
    const sourceTree = protectedTree(SOURCE_COMMIT, run, cwd);
    const publication = {
      check,
      contractVersion: CONTRACT_VERSION,
      ...bundle,
      publish,
      serverImage: references.server,
      serverImageId: imageConfig(references.server, run, cwd),
      sourceTree,
      targetCommit: head,
      targetRelease: release,
      targetTree: protectedTree(head, run, cwd),
      webImage: references.web,
      webImageId: imageConfig(references.web, run, cwd),
    };
    validatePublication(publication, release);
    writeJsonCreateOnce(resolve(cwd, output), publication);
    return publication;
  } finally {
    // delete only the owned temporary
    rmSync(ownedTemporary, { force: true, recursive: true });
  }
}

// execute only as the entry point
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const [release, output] = process.argv.slice(2);
    // require fixed cli arity
    if (process.argv.length !== 4) {
      throw new Error("usage: prepare-schema-preserving-publication.mjs RELEASE OUTPUT");
    }
    preparePublication(release, output);
  } catch (error) {
    process.stderr.write(`prepare-schema-preserving-publication: ${error.message}\n`);
    process.exitCode = 1;
  }
}
