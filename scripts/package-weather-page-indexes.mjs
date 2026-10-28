import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { gunzipSync } from "node:zlib";
import { pathToFileURL } from "node:url";

export const WEATHER_PAGE_INDEX_RELEASE_SCHEMA = "weather-page-index-release/v1";
export const WEATHER_PAGE_INDEX_RESTORE_SCHEMA =
  "weather-page-index-restore-context/v1";
export const WEATHER_PAGE_INDEX_MANIFEST_SCHEMA = "weather-page-indexes/v2";
export const WEATHER_PAGE_INDEX_REPLACEMENT_SCHEMA =
  "weather-page-index-replacement/v1";

const PACKAGE_SCHEMA = "weather-page-index-package/v1";
const B1_REPLACEMENT_SHA256 =
  "ac165391de09e1490a5832108fd54bba2dda97bf67a6225377c2a2e8e5e91c82";
const RELEASE_PATTERN = /^(\d{4})\.(\d{2})\.(\d{2})-([1-9]\d?)$/u;
const SHA1_PATTERN = /^[a-f0-9]{40}$/u;
const MAX_MEMBER_BYTES = 256 * 1024;
const MAX_PACKAGE_DECODED_BYTES = 768 * 1024;
const MAX_PACKAGE_ARCHIVE_BYTES = 1024 * 1024;
const MAX_RESTORE_DECODED_BYTES = 64 * 1024 * 1024;
const MAX_RESTORE_FILES = 2048;
const MAX_RESTORE_ARCHIVE_BYTES = 64 * 1024 * 1024;
const GIT_MAX_BUFFER = 96 * 1024 * 1024;
const SOURCE_REPOSITORY = "anstosa/weather";
const RENAMED_SOURCE_REPOSITORY = Object.freeze({
  historical: SOURCE_REPOSITORY,
  canonical: "anstosa/Ballydidean-Weather",
  id: 1_342_404_160,
});

const RESTORE_MIGRATIONS = Object.freeze([
  "packages/database/migrations/0001_initial_weather.sql",
  "packages/database/migrations/0002_worker_migration_readiness.sql",
  "packages/database/migrations/0003_ecowitt_measurements.sql",
  "packages/database/migrations/0004_tempest_metadata.sql",
  "packages/database/migrations/0005_source_supersession.sql",
  "packages/database/migrations/0006_station_coordinates.sql",
  "packages/database/migrations/0007_tide_sources.sql",
  "packages/database/migrations/0008_ecowitt_property_sensors.sql",
  "packages/database/migrations/0009_forecast_anchor_records.sql",
  "packages/database/migrations/0010_forecast_training_export.sql",
  "packages/database/migrations/0011_forecast_runtime_provenance.sql",
  "packages/database/migrations/0012_hide_archive_only_forecasts_from_live_reads.sql",
  "packages/database/migrations/0013_ecmwf_temperature_canary.sql",
  "packages/database/migrations/0014_rain_collection.sql",
  "packages/database/migrations/0015_rain_station_access.sql",
  "packages/database/migrations/0016_rain_adjustment.sql",
  "packages/database/migrations/0017_adjustment_evaluation_export.sql",
  "packages/database/migrations/0018_adjustment_maintenance_v2.sql",
]);

const RESTORE_CRITICAL_PATHS = Object.freeze({
  restore: "deploy/scripts/restore.sh",
  common: "deploy/scripts/common.sh",
  compose: "deploy/compose.yaml",
  acl: "deploy/postgres/runtime-acl-v2.sql",
});

export const FROZEN_RESTORE_IDENTITY = Object.freeze({
  repository: SOURCE_REPOSITORY,
  tag: "2026.10.09-1",
  commit: "56b327d9c750946f6f6963b6fe1fa5c9bba791ca",
  tree: "ecd9599ebf4907c67b912c742eb8fc3d8e6ee8df",
  fileCount: 813,
  decodedBytes: 17_238_740,
  migrations: RESTORE_MIGRATIONS,
});

// hash immutable bytes
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// serialize stable documents
export function serializeCanonicalJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

// sort object keys recursively
export function stableValue(value) {
  // preserve array order
  if (Array.isArray(value)) {
    return value.map((entry) => stableValue(entry));
  }

  // sort plain object fields
  if (value !== null && typeof value === "object") {
    const result = {};

    // normalize each field
    for (const key of Object.keys(value).sort()) {
      result[key] = stableValue(value[key]);
    }
    return result;
  }
  return value;
}

// serialize canonical hash input
export function stableJson(value) {
  return JSON.stringify(stableValue(value));
}

// validate the closed b1 replacement payload
export function validateB1IndexManifest(manifest) {
  const fields = ["admission", "operation", "replacement", "restoreContext", "schema"];

  // require only the frozen top-level capabilities
  if (
    manifest === null ||
    Array.isArray(manifest) ||
    typeof manifest !== "object" ||
    JSON.stringify(Object.keys(manifest).sort()) !== JSON.stringify(fields) ||
    manifest.schema !== WEATHER_PAGE_INDEX_MANIFEST_SCHEMA ||
    manifest.replacement?.schema !== WEATHER_PAGE_INDEX_REPLACEMENT_SCHEMA
  ) {
    throw new Error("tagged index manifest is not the closed B1 replacement");
  }
  const replacementDigest = sha256(Buffer.from(stableJson(manifest.replacement)));

  // bind every replacement object name order and definition
  if (replacementDigest !== B1_REPLACEMENT_SHA256) {
    throw new Error("tagged B1 replacement manifest differs");
  }
  return manifest;
}

// run one bounded command
function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd,
    encoding: options.encoding,
    env: options.env ?? process.env,
    maxBuffer: options.maxBuffer ?? GIT_MAX_BUFFER,
  });

  // surface command failure
  if (result.status !== 0) {
    const detail = Buffer.isBuffer(result.stderr)
      ? result.stderr.toString("utf8").trim()
      : String(result.stderr ?? "").trim();
    throw new Error(`${basename(command)} failed${detail ? `: ${detail}` : ""}`);
  }
  return result.stdout;
}

// read one git result
function gitText(repositoryPath, args) {
  return String(run("git", ["-C", repositoryPath, ...args], {
    encoding: "utf8",
  })).trim();
}

// read one git object
function gitBytes(repositoryPath, args) {
  return Buffer.from(run("git", ["-C", repositoryPath, ...args]));
}

// reject unsafe archive paths
function requireSafePath(path, nested) {
  // require canonical utf8 posix paths
  if (
    typeof path !== "string" ||
    path.length === 0 ||
    path.startsWith("/") ||
    path.includes("\\") ||
    path.includes("\0") ||
    path.includes("\uFFFD") ||
    /[\u0000-\u001f\u007f]/u.test(path) ||
    path.split("/").some((part) => part === "" || part === "." || part === "..") ||
    (!nested && path.includes("/"))
  ) {
    throw new Error(`unsafe archive path: ${JSON.stringify(path)}`);
  }
}

// parse one tar octal field
function parseTarOctal(header, start, length, label) {
  const value = header.subarray(start, start + length).toString("ascii")
    .replace(/\0.*$/u, "")
    .trim();

  // reject base-256 and malformed fields
  if (!/^[0-7]+$/u.test(value)) {
    throw new Error(`invalid tar ${label}`);
  }
  return Number.parseInt(value, 8);
}

// parse one ustar string
function parseTarString(header, start, length) {
  const bytes = header.subarray(start, start + length);
  const end = bytes.indexOf(0);
  return bytes.subarray(0, end === -1 ? bytes.length : end).toString("utf8");
}

// inspect finite ustar bytes
export function inspectUstarArchive(archiveBytes, {
  expectedNames,
  allowNested = false,
  maximumEntries,
  maximumMemberBytes,
  maximumDecodedBytes,
} = {}) {
  const tar = gunzipSync(archiveBytes);
  const entries = [];
  const names = new Set();
  let offset = 0;
  let zeroBlocks = 0;
  let decodedBytes = 0;

  // walk raw headers
  while (offset + 512 <= tar.length) {
    const header = tar.subarray(offset, offset + 512);
    const allZero = header.every((byte) => byte === 0);

    // stop only at a zero trailer
    if (allZero) {
      zeroBlocks += 1;
      offset += 512;
      continue;
    }

    // reject data after the trailer
    if (zeroBlocks > 0) {
      throw new Error("tar contains data after its zero trailer");
    }
    const expectedChecksum = parseTarOctal(header, 148, 8, "checksum");
    let actualChecksum = 0;

    // calculate checksum bytes
    for (let index = 0; index < header.length; index += 1) {
      actualChecksum += index >= 148 && index < 156 ? 32 : header[index];
    }

    // reject corrupt headers
    if (actualChecksum !== expectedChecksum) {
      throw new Error("tar header checksum differs");
    }
    const magic = header.subarray(257, 263).toString("binary");

    // require ustar without extensions
    if (magic !== "ustar\0" && magic !== "ustar ") {
      throw new Error("archive is not ustar");
    }
    const type = header[156];

    // reject links and extension headers
    if (type !== 0 && type !== 48) {
      throw new Error(`tar member type is not regular: ${String.fromCharCode(type)}`);
    }
    const name = parseTarString(header, 0, 100);
    const prefix = parseTarString(header, 345, 155);
    const path = prefix ? `${prefix}/${name}` : name;
    requireSafePath(path, allowNested);

    // reject duplicate members
    if (names.has(path)) {
      throw new Error(`duplicate tar member: ${path}`);
    }
    names.add(path);
    const size = parseTarOctal(header, 124, 12, "size");
    const mode = parseTarOctal(header, 100, 8, "mode");

    // enforce member bounds
    if (maximumMemberBytes !== undefined && size > maximumMemberBytes) {
      throw new Error(`tar member exceeds its size limit: ${path}`);
    }
    const contentOffset = offset + 512;
    const nextOffset = contentOffset + Math.ceil(size / 512) * 512;

    // reject truncated members
    if (nextOffset > tar.length) {
      throw new Error(`truncated tar member: ${path}`);
    }
    const content = tar.subarray(contentOffset, contentOffset + size);
    decodedBytes += size;
    entries.push({
      name: path,
      size,
      mode: mode.toString(8).padStart(4, "0"),
      digest: sha256(content),
      content,
    });
    offset = nextOffset;
  }

  // require the conventional two-block trailer
  if (zeroBlocks < 2 || offset !== tar.length) {
    throw new Error("tar trailer is incomplete");
  }

  // enforce aggregate bounds
  if (maximumEntries !== undefined && entries.length > maximumEntries) {
    throw new Error("tar contains too many members");
  }
  if (maximumDecodedBytes !== undefined && decodedBytes > maximumDecodedBytes) {
    throw new Error("tar decoded bytes exceed their limit");
  }

  // require exact member order and names
  if (expectedNames !== undefined) {
    const actualNames = entries.map((entry) => entry.name);

    // reject missing or extra entries
    if (JSON.stringify(actualNames) !== JSON.stringify(expectedNames)) {
      throw new Error(`tar members differ: ${actualNames.join(",")}`);
    }
  }
  return { entries, decodedBytes, tarBytes: tar.length };
}

// resolve a release date
export function requireReleaseTag(release) {
  const match = RELEASE_PATTERN.exec(release ?? "");

  // reject unsupported tag classes
  if (match === null) {
    throw new Error("release tag must use YYYY.MM.DD-N");
  }
  const [, year, month, day] = match;
  const normalized = new Date(`${year}-${month}-${day}T00:00:00Z`);

  // reject normalized invalid dates
  if (
    Number.isNaN(normalized.valueOf()) ||
    normalized.toISOString().slice(0, 10) !== `${year}-${month}-${day}`
  ) {
    throw new Error("release tag contains an invalid date");
  }
  return release;
}

// resolve one annotated tag
function resolveTagIdentity(repositoryPath, tag) {
  const ref = `refs/tags/${tag}`;
  const tagObject = gitText(repositoryPath, ["rev-parse", "--verify", ref]);

  // require an immutable annotated tag
  if (!SHA1_PATTERN.test(tagObject) || gitText(repositoryPath, ["cat-file", "-t", ref]) !== "tag") {
    throw new Error(`release tag is not annotated: ${tag}`);
  }
  const commit = gitText(repositoryPath, ["rev-parse", "--verify", `${ref}^{commit}`]);
  const tree = gitText(repositoryPath, ["rev-parse", "--verify", `${commit}^{tree}`]);

  // reject malformed git identities
  if (!SHA1_PATTERN.test(commit) || !SHA1_PATTERN.test(tree)) {
    throw new Error(`release tag identity is invalid: ${tag}`);
  }
  return { tag, tagObject, commit, tree };
}

// parse one github remote
function resolveRepositoryName(repositoryPath) {
  const remote = gitText(repositoryPath, ["remote", "get-url", "origin"]);
  const match = /^(?:git@github\.com:|https:\/\/github\.com\/)([A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+?)(?:\.git)?$/u.exec(remote);

  // reject non-github provenance
  if (match === null) {
    throw new Error("origin is not an exact GitHub repository");
  }
  return match[1];
}

// parse one gh response
function ghJson(endpoint) {
  const stdout = run("gh", ["api", endpoint], { encoding: "utf8" });
  return JSON.parse(String(stdout));
}

// bind one exact github repository or reviewed rename
export function resolvePublishedRepositoryIdentity(historical, document) {
  const frozenSource = historical === RENAMED_SOURCE_REPOSITORY.historical;
  const direct =
    document?.full_name === historical &&
    (!frozenSource || document?.id === RENAMED_SOURCE_REPOSITORY.id);
  const renamed =
    frozenSource &&
    document?.full_name === RENAMED_SOURCE_REPOSITORY.canonical &&
    document?.id === RENAMED_SOURCE_REPOSITORY.id;

  // reject forks malformed identities and arbitrary redirects
  if (
    typeof historical !== "string" ||
    historical.length === 0 ||
    !Number.isSafeInteger(document?.id) ||
    document.id <= 0 ||
    document.fork !== false ||
    (!direct && !renamed)
  ) {
    throw new Error("GitHub repository identity differs");
  }
  return {
    repository: historical,
    canonicalRepository: document.full_name,
    repositoryId: document.id,
  };
}

// select exact workflow evidence
export function selectPublishedWorkflowRun(document, {
  repository,
  commit,
  tag,
  workflow,
}) {
  // reject truncated evidence
  if (
    !Number.isInteger(document?.total_count) ||
    document.total_count > 100 ||
    !Array.isArray(document.workflow_runs) ||
    document.workflow_runs.length !== document.total_count
  ) {
    throw new Error(`${workflow} lookup returned incomplete evidence`);
  }
  const candidates = document.workflow_runs.filter((runRecord) =>
    runRecord.event === "push" &&
    runRecord.head_sha === commit &&
    runRecord.head_repository?.id === repository.id &&
    runRecord.head_repository?.full_name === repository.full_name);

  // require one exact run
  if (candidates.length === 0) {
    throw new Error(`${workflow} has no exact repository/commit push`);
  }
  const expectedPath = `.github/workflows/${workflow}.yml`;

  // reject malformed exact evidence
  if (candidates.some((runRecord) => {
    const referenceClass = workflow === "check" ? "heads" : "tags";
    const expectedReferencePath =
      `${expectedPath}@refs/${referenceClass}/${runRecord.head_branch}`;
    return !Number.isSafeInteger(runRecord.id) ||
      runRecord.id <= 0 ||
      !Number.isSafeInteger(runRecord.run_attempt) ||
      runRecord.run_attempt <= 0 ||
      !Number.isFinite(Date.parse(runRecord.created_at ?? "")) ||
      runRecord.repository?.id !== repository.id ||
      runRecord.repository?.full_name !== repository.full_name ||
      typeof runRecord.head_branch !== "string" ||
      runRecord.head_branch.length === 0 ||
      (runRecord.path !== expectedPath && runRecord.path !== expectedReferencePath);
  })) {
    throw new Error(`${workflow} returned malformed exact evidence`);
  }
  candidates.sort((left, right) =>
    Date.parse(right.created_at) - Date.parse(left.created_at) ||
    right.id - left.id);
  const latest = candidates[0];

  // require the expected branch identity
  if (workflow === "check" && latest.head_branch === tag) {
    throw new Error("Check evidence is a tag push, not a branch push");
  }
  if (workflow === "publish-images" && latest.head_branch !== tag) {
    throw new Error("publication evidence does not match the release tag");
  }

  // require completed success
  if (latest.status !== "completed" || latest.conclusion !== "success") {
    throw new Error(`latest exact ${workflow} run is not successful`);
  }
  return latest;
}

// verify remote release publication
export async function verifyPublishedRelease({
  repositoryPath,
  repository,
  tag,
  tagObject,
  commit,
  tree,
  restoreIdentity,
}) {
  const remoteLines = gitText(repositoryPath, [
    "ls-remote",
    "--tags",
    "origin",
    `refs/tags/${tag}`,
    `refs/tags/${tag}^{}`,
    `refs/tags/${restoreIdentity.tag}`,
    `refs/tags/${restoreIdentity.tag}^{}`,
  ]).split("\n");
  const remoteRefs = new Map(remoteLines.map((line) => {
    const [object, ref] = line.split(/\s+/u);
    return [ref, object];
  }));

  // require unmoved release and restore tags
  if (
    remoteRefs.get(`refs/tags/${tag}`) !== tagObject ||
    remoteRefs.get(`refs/tags/${tag}^{}`) !== commit ||
    remoteRefs.get(`refs/tags/${restoreIdentity.tag}^{}`) !== restoreIdentity.commit
  ) {
    throw new Error("remote tag identity differs from local Git objects");
  }
  const repositoryDocument = ghJson(`repos/${repository}`);
  const repositoryIdentity = resolvePublishedRepositoryIdentity(
    repository,
    repositoryDocument,
  );
  const encodedCommit = encodeURIComponent(commit);
  const checkDocument = ghJson(
    `repos/${repositoryIdentity.canonicalRepository}/actions/workflows/check.yml/runs?head_sha=${encodedCommit}&event=push&per_page=100`,
  );
  const publicationDocument = ghJson(
    `repos/${repositoryIdentity.canonicalRepository}/actions/workflows/publish-images.yml/runs?head_sha=${encodedCommit}&event=push&branch=${encodeURIComponent(tag)}&per_page=100`,
  );
  const lookup = {
    repository: {
      id: repositoryIdentity.repositoryId,
      full_name: repositoryIdentity.canonicalRepository,
    },
    commit,
    tag,
  };
  const check = selectPublishedWorkflowRun(checkDocument, {
    ...lookup,
    workflow: "check",
  });
  const publication = selectPublishedWorkflowRun(publicationDocument, {
    ...lookup,
    workflow: "publish-images",
  });
  return {
    ...repositoryIdentity,
    tag,
    commit,
    tree,
    tagObject,
    checkRunId: check.id,
    checkRunAttempt: check.run_attempt,
    publicationRunId: publication.id,
    publicationRunAttempt: publication.run_attempt,
  };
}

// parse the complete git tree
function collectGitTree(repositoryPath, commit) {
  const raw = gitBytes(repositoryPath, ["ls-tree", "-rz", "--full-tree", commit]);
  const records = raw.toString("utf8").split("\0");
  records.pop();
  const entries = [];

  // resolve every immutable blob
  for (const record of records) {
    const match = /^(\d{6}) ([a-z]+) ([a-f0-9]{40})\t(.+)$/u.exec(record);

    // reject malformed tree output
    if (match === null) {
      throw new Error("Git tree record is malformed");
    }
    const [, mode, type, blob, path] = match;
    requireSafePath(path, true);

    // reject links, submodules and special modes
    if (type !== "blob" || (mode !== "100644" && mode !== "100755")) {
      throw new Error(`Git tree contains a non-regular file: ${path}`);
    }
    const content = gitBytes(repositoryPath, ["cat-file", "blob", blob]);
    entries.push({
      path,
      mode,
      blob,
      digest: sha256(content),
      size: content.length,
      content,
    });
  }
  entries.sort((left, right) =>
    Buffer.compare(Buffer.from(left.path), Buffer.from(right.path)));
  return entries;
}

// read one tagged regular file
function readTaggedFile(repositoryPath, commit, path) {
  const record = gitText(repositoryPath, ["ls-tree", commit, "--", path]);
  const match = /^(100644|100755) blob ([a-f0-9]{40})\t(.+)$/u.exec(record);

  // require one exact regular source file
  if (match === null || match[3] !== path) {
    throw new Error(`tagged source file is unavailable: ${path}`);
  }
  const content = gitBytes(repositoryPath, ["cat-file", "blob", match[2]]);
  return {
    path,
    mode: match[1],
    blob: match[2],
    digest: sha256(content),
    size: content.length,
    content,
  };
}

// materialize regular git files
function materializeTree(root, entries) {
  // write every bounded source file
  for (const entry of entries) {
    const destination = join(root, entry.path);
    mkdirSync(dirname(destination), { recursive: true, mode: 0o700 });
    writeFileSync(destination, entry.content, { flag: "wx", mode: 0o600 });
    chmodSync(destination, entry.mode === "100755" ? 0o755 : 0o644);
  }
}

// build one deterministic archive
function createUstarArchive(sourceRoot, paths, archivePath) {
  const listPath = join(dirname(archivePath), `${basename(archivePath)}.files`);
  writeFileSync(listPath, `${paths.join("\n")}\n`, { flag: "wx", mode: 0o600 });
  run("tar", [
    "--format=ustar",
    "--sort=name",
    "--mtime=@0",
    "--owner=0",
    "--group=0",
    "--numeric-owner",
    "--no-recursion",
    "--verbatim-files-from",
    "--use-compress-program=gzip -n",
    "-C",
    sourceRoot,
    "-cf",
    archivePath,
    "--files-from",
    listPath,
  ], {
    env: { ...process.env, LC_ALL: "C", SOURCE_DATE_EPOCH: "0" },
  });
}

// require exact frozen restore bytes
function requireRestoreIdentity(identity, tagIdentity, entries, repository) {
  // reject a moved frozen tag
  if (
    tagIdentity.commit !== identity.commit ||
    tagIdentity.tree !== identity.tree ||
    identity.repository !== repository
  ) {
    throw new Error("frozen restore tag identity differs");
  }
  const decodedBytes = entries.reduce((total, entry) => total + entry.size, 0);

  // enforce declared tree bounds
  if (entries.length !== identity.fileCount || decodedBytes !== identity.decodedBytes) {
    throw new Error("frozen restore tree count or bytes differ");
  }
  const migrations = entries
    .map((entry) => entry.path)
    .filter((path) => /^packages\/database\/migrations\/\d{4}.*\.sql$/u.test(path));

  // require the exact ordered legacy migrations
  if (JSON.stringify(migrations) !== JSON.stringify(identity.migrations)) {
    throw new Error("frozen restore migrations differ");
  }
  return decodedBytes;
}

// describe critical restore files
function criticalRestoreFiles(files, migrations) {
  const byPath = new Map(files.map((entry) => [entry.path, entry]));

  // retain only finite critical fields
  const criticalEntry = (path) => {
    const entry = byPath.get(path);

    // require each critical dependency
    if (entry === undefined) {
      throw new Error(`restore critical file is missing: ${path}`);
    }
    return { path: entry.path, digest: entry.digest };
  };
  return {
    restore: criticalEntry(RESTORE_CRITICAL_PATHS.restore),
    common: criticalEntry(RESTORE_CRITICAL_PATHS.common),
    compose: criticalEntry(RESTORE_CRITICAL_PATHS.compose),
    acl: criticalEntry(RESTORE_CRITICAL_PATHS.acl),
    migrations: migrations.map(criticalEntry),
  };
}

// validate the tagged restore template
function requireRestoreTemplate(template, identity) {
  const criticalPaths = Object.values(RESTORE_CRITICAL_PATHS);

  // require frozen source and absolute bounds
  if (
    template === null ||
    Array.isArray(template) ||
    typeof template !== "object" ||
    template.schema !== WEATHER_PAGE_INDEX_RESTORE_SCHEMA ||
    template.repository !== identity.repository ||
    template.tag !== identity.tag ||
    template.commit !== identity.commit ||
    template.tree !== identity.tree ||
    template.fileCount !== identity.fileCount ||
    template.decodedBytes !== identity.decodedBytes ||
    template.maximumFileCount !== MAX_RESTORE_FILES ||
    template.maximumDecodedBytes !== MAX_RESTORE_DECODED_BYTES ||
    JSON.stringify(template.criticalPaths) !== JSON.stringify(criticalPaths) ||
    JSON.stringify(template.migrations) !== JSON.stringify(identity.migrations)
  ) {
    throw new Error("tagged restoreContext template differs from the frozen source");
  }
}

// write an immutable output
function writeImmutable(path, bytes, mode = 0o600) {
  const content = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes);

  // retain only byte-identical prior output
  if (existsSync(path)) {
    const existing = readFileSync(path);

    // reject output replacement
    if (!existing.equals(content)) {
      throw new Error(`output already exists with different bytes: ${path}`);
    }
    return;
  }
  writeFileSync(path, content, { flag: "wx", mode });
  chmodSync(path, mode);
}

// require verifier output identity
function requirePublicationEvidence(evidence, expected) {
  let repositoryIdentity;

  // validate injected repository evidence with the same rename policy
  try {
    repositoryIdentity = resolvePublishedRepositoryIdentity(
      expected.repository,
      {
        id: evidence?.repositoryId,
        full_name: evidence?.canonicalRepository,
        fork: false,
      },
    );
  } catch {
    // collapse invalid aliases into one verifier failure
    throw new Error("release verifier did not bind complete publication evidence");
  }

  // reject incomplete verifier evidence
  if (
    evidence === null ||
    typeof evidence !== "object" ||
    evidence.repository !== repositoryIdentity.repository ||
    evidence.canonicalRepository !== repositoryIdentity.canonicalRepository ||
    evidence.repositoryId !== repositoryIdentity.repositoryId ||
    evidence.tag !== expected.tag ||
    evidence.commit !== expected.commit ||
    evidence.tree !== expected.tree ||
    evidence.tagObject !== expected.tagObject ||
    !Number.isSafeInteger(evidence.checkRunId) ||
    evidence.checkRunId <= 0 ||
    !Number.isSafeInteger(evidence.publicationRunId) ||
    evidence.publicationRunId <= 0
  ) {
    throw new Error("release verifier did not bind complete publication evidence");
  }
}

// build immutable release artifacts
export async function buildWeatherPageIndexPackage({
  repositoryPath,
  release,
  outputDirectory,
  verifyPublication = verifyPublishedRelease,
  restoreIdentity = FROZEN_RESTORE_IDENTITY,
} = {}) {
  requireReleaseTag(release);
  const repositoryRoot = resolve(repositoryPath ?? process.cwd());
  const outputRoot = resolve(outputDirectory);
  const repository = resolveRepositoryName(repositoryRoot);

  // restrict production provenance
  if (repository !== SOURCE_REPOSITORY && verifyPublication === verifyPublishedRelease) {
    throw new Error(`repository must be ${SOURCE_REPOSITORY}`);
  }
  const releaseIdentity = resolveTagIdentity(repositoryRoot, release);
  const restoreTagIdentity = resolveTagIdentity(repositoryRoot, restoreIdentity.tag);
  const verificationInput = {
    repositoryPath: repositoryRoot,
    repository,
    ...releaseIdentity,
    restoreIdentity,
  };
  const publicationEvidence = await verifyPublication(verificationInput);
  requirePublicationEvidence(publicationEvidence, {
    repository,
    ...releaseIdentity,
  });
  const restoreEntries = collectGitTree(repositoryRoot, restoreIdentity.commit);
  const restoreDecodedBytes = requireRestoreIdentity(
    restoreIdentity,
    restoreTagIdentity,
    restoreEntries,
    repository,
  );

  // enforce absolute restore bounds
  if (
    restoreEntries.length > MAX_RESTORE_FILES ||
    restoreDecodedBytes > MAX_RESTORE_DECODED_BYTES
  ) {
    throw new Error("restore context exceeds its absolute bound");
  }
  mkdirSync(outputRoot, { recursive: true, mode: 0o700 });
  const temporaryRoot = mkdtempSync(join(tmpdir(), "weather-page-index-package-"));

  try {
    const restoreRoot = join(temporaryRoot, "restore");
    mkdirSync(restoreRoot, { mode: 0o700 });
    materializeTree(restoreRoot, restoreEntries);
    const restoreArchiveName = `restore-context-${restoreIdentity.commit.slice(0, 12)}.tar.gz`;
    const restoreArchivePath = join(temporaryRoot, restoreArchiveName);
    createUstarArchive(
      restoreRoot,
      restoreEntries.map((entry) => entry.path),
      restoreArchivePath,
    );
    const restoreArchiveBytes = readFileSync(restoreArchivePath);

    // enforce compressed restore bound
    if (restoreArchiveBytes.length > MAX_RESTORE_ARCHIVE_BYTES) {
      throw new Error("restore context archive exceeds its size limit");
    }
    const inspectedRestore = inspectUstarArchive(restoreArchiveBytes, {
      expectedNames: restoreEntries.map((entry) => entry.path),
      allowNested: true,
      maximumEntries: MAX_RESTORE_FILES,
      maximumDecodedBytes: MAX_RESTORE_DECODED_BYTES,
    });

    // require every archived restore byte
    for (const [index, entry] of restoreEntries.entries()) {
      const archived = inspectedRestore.entries[index];

      // reject archive transformation
      if (
        archived.digest !== entry.digest ||
        archived.size !== entry.size ||
        archived.mode !== (entry.mode === "100755" ? "0755" : "0644")
      ) {
        throw new Error(`restore archive member differs: ${entry.path}`);
      }
    }
    const descriptorFiles = restoreEntries.map(({ path, mode, blob, digest, size }) => ({
      path,
      mode,
      blob,
      digest,
      size,
    }));
    const filesDigest = sha256(Buffer.from(stableJson(descriptorFiles)));
    const restoreExpansion = {
      filesDigest,
      archive: {
        name: restoreArchiveName,
        digest: sha256(restoreArchiveBytes),
        size: restoreArchiveBytes.length,
      },
      files: descriptorFiles,
      critical: criticalRestoreFiles(descriptorFiles, restoreIdentity.migrations),
    };
    const helperSource = readTaggedFile(
      repositoryRoot,
      releaseIdentity.commit,
      "scripts/weather-page-indexes.mjs",
    );
    const indexSource = readTaggedFile(
      repositoryRoot,
      releaseIdentity.commit,
      "scripts/weather-page-indexes.json",
    );
    const installerSource = readTaggedFile(
      repositoryRoot,
      releaseIdentity.commit,
      "scripts/install-weather-page-indexes.sh",
    );

    // require executable installer source
    if (installerSource.mode !== "100755") {
      throw new Error("tagged installer is not executable");
    }
    const indexBase = JSON.parse(indexSource.content.toString("utf8"));

    // reject a malformed index manifest
    if (
      indexBase === null ||
      Array.isArray(indexBase) ||
      typeof indexBase !== "object"
    ) {
      throw new Error("tagged index manifest cannot accept restoreContext");
    }
    validateB1IndexManifest(indexBase);
    requireRestoreTemplate(indexBase.restoreContext, restoreIdentity);
    const restoreDescriptor = {
      ...indexBase.restoreContext,
      ...restoreExpansion,
    };
    const indexBytes = Buffer.from(serializeCanonicalJson({
      ...indexBase,
      restoreContext: restoreDescriptor,
    }));
    const packageFiles = [
      {
        path: "weather-page-indexes.mjs",
        digest: helperSource.digest,
        size: helperSource.size,
        mode: helperSource.mode,
        content: helperSource.content,
      },
      {
        path: "weather-page-indexes.json",
        digest: sha256(indexBytes),
        size: indexBytes.length,
        mode: "100644",
        content: indexBytes,
      },
    ];
    const releaseManifest = {
      schema: WEATHER_PAGE_INDEX_RELEASE_SCHEMA,
      repository,
      tag: release,
      commit: releaseIdentity.commit,
      tree: releaseIdentity.tree,
      files: packageFiles.map(({ path, digest, size, mode }) => ({
        path,
        digest,
        size,
        mode,
      })),
      restoreContext: restoreDescriptor,
    };
    const releaseManifestBytes = Buffer.from(serializeCanonicalJson(releaseManifest));
    const packageMembers = [
      ...packageFiles,
      {
        path: "release-manifest.json",
        digest: sha256(releaseManifestBytes),
        size: releaseManifestBytes.length,
        mode: "100644",
        content: releaseManifestBytes,
      },
    ];

    // recheck the complete generated manifest member
    for (const member of packageMembers) {
      // enforce every production member cap
      if (member.size > MAX_MEMBER_BYTES) {
        throw new Error(`package member exceeds 256KiB: ${member.path}`);
      }
    }
    const packageDecodedBytes = packageMembers.reduce(
      (total, member) => total + member.size,
      0,
    );

    // enforce decoded package cap
    if (packageDecodedBytes > MAX_PACKAGE_DECODED_BYTES) {
      throw new Error("package decoded bytes exceed 768KiB");
    }
    const packageRoot = join(temporaryRoot, "package");
    mkdirSync(packageRoot, { mode: 0o700 });

    // write only three package members
    for (const member of packageMembers) {
      const destination = join(packageRoot, member.path);
      writeFileSync(destination, member.content, { flag: "wx", mode: 0o600 });
      chmodSync(destination, member.mode === "100755" ? 0o755 : 0o644);
    }
    const archiveName = `weather-page-indexes-${release}-${releaseIdentity.commit.slice(0, 12)}.tar.gz`;
    const archivePath = join(temporaryRoot, archiveName);
    const packageNames = packageMembers.map((member) => member.path).sort();
    createUstarArchive(packageRoot, packageNames, archivePath);
    const archiveBytes = readFileSync(archivePath);

    // enforce compressed package cap
    if (archiveBytes.length > MAX_PACKAGE_ARCHIVE_BYTES) {
      throw new Error("package archive exceeds 1MiB");
    }
    const inspectedPackage = inspectUstarArchive(archiveBytes, {
      expectedNames: packageNames,
      maximumEntries: 3,
      maximumMemberBytes: MAX_MEMBER_BYTES,
      maximumDecodedBytes: MAX_PACKAGE_DECODED_BYTES,
    });
    const expectedPackage = new Map(packageMembers.map((member) => [member.path, member]));

    // require exact package bytes and modes
    for (const archived of inspectedPackage.entries) {
      const expected = expectedPackage.get(archived.name);

      // reject archive transformation
      if (
        expected === undefined ||
        archived.digest !== expected.digest ||
        archived.size !== expected.size ||
        archived.mode !== (expected.mode === "100755" ? "0755" : "0644")
      ) {
        throw new Error(`package archive member differs: ${archived.name}`);
      }
    }
    const installerDigest = installerSource.digest;
    const installerName = `install-weather-page-indexes-${release}-${releaseIdentity.commit.slice(0, 12)}-${installerDigest}.sh`;
    const archiveDigest = sha256(archiveBytes);
    const metadataName = `weather-page-indexes-${release}-${releaseIdentity.commit.slice(0, 12)}.metadata.json`;
    const metadata = {
      schema: PACKAGE_SCHEMA,
      repository,
      tag: release,
      commit: releaseIdentity.commit,
      tree: releaseIdentity.tree,
      publication: {
        historicalRepository: publicationEvidence.repository,
        canonicalRepository: publicationEvidence.canonicalRepository,
        repositoryId: publicationEvidence.repositoryId,
        checkRunId: publicationEvidence.checkRunId,
        checkRunAttempt: publicationEvidence.checkRunAttempt ?? 1,
        publicationRunId: publicationEvidence.publicationRunId,
        publicationRunAttempt: publicationEvidence.publicationRunAttempt ?? 1,
      },
      archive: {
        name: archiveName,
        digest: archiveDigest,
        size: archiveBytes.length,
        decodedBytes: packageDecodedBytes,
      },
      restoreContext: restoreDescriptor.archive,
      installer: {
        name: installerName,
        digest: installerDigest,
        size: installerSource.size,
        sourcePath: installerSource.path,
        sourceBlob: installerSource.blob,
      },
    };
    const outputs = [
      [archiveName, archiveBytes, 0o600],
      [`${archiveName}.sha256`, `${archiveDigest}  ${archiveName}\n`, 0o600],
      [restoreArchiveName, restoreArchiveBytes, 0o600],
      [`${restoreArchiveName}.sha256`, `${restoreDescriptor.archive.digest}  ${restoreArchiveName}\n`, 0o600],
      [installerName, installerSource.content, 0o700],
      [`${installerName}.sha256`, `${installerDigest}  ${installerName}\n`, 0o600],
      [metadataName, serializeCanonicalJson(metadata), 0o600],
    ];

    // publish immutable outputs last
    for (const [name, bytes, mode] of outputs) {
      writeImmutable(join(outputRoot, name), bytes, mode);
    }
    return {
      ...metadata,
      outputDirectory: outputRoot,
      metadata: { name: metadataName },
    };
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

// parse the finite cli
function parseArguments(argv) {
  const options = {};

  // consume paired flags
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];

    // reject missing values
    if (value === undefined) {
      throw new Error(`missing value for ${flag}`);
    }

    // bind supported flags
    if (flag === "--release") {
      options.release = value;
    } else if (flag === "--output") {
      options.outputDirectory = value;
    } else {
      throw new Error(`unsupported argument: ${flag}`);
    }
  }

  // require both cli inputs
  if (!options.release || !options.outputDirectory) {
    throw new Error("usage: package-weather-page-indexes.mjs --release TAG --output DIR");
  }
  return options;
}

// run only as a cli
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const result = await buildWeatherPageIndexPackage({
    repositoryPath: process.cwd(),
    ...parseArguments(process.argv.slice(2)),
  });
  process.stdout.write(`${serializeCanonicalJson(result)}`);
}
