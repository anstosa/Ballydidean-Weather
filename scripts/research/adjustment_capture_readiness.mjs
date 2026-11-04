import { createHash } from "node:crypto";
import { lstat, readFile, readdir, realpath } from "node:fs/promises";
import { basename, dirname, join, relative, resolve, sep } from "node:path";

import { validateAdjustmentV14RollingDatabaseManifest } from
  "../../deploy/scripts/adjustment-evaluation-package.mjs";
import { inspectAdjustmentArchiveLocalReadiness } from "./adjustment_archive_job.mjs";
import { ADJUSTMENT_FIT_PUBLIC_CODE_ALLOWLIST } from "./adjustment_fit_inputs.mjs";
import { captureAdjustmentFitRuntimeReadiness } from "./adjustment_fit_sandbox.mjs";
import {
  fetchAdjustmentFamilyReleaseCurrent,
  fetchAdjustmentFamilyReleaseLineage,
  fetchAdjustmentRevisionCaptureEpochSnapshot,
  fetchAdjustmentRevisionCaptureEpochWitness,
} from "./adjustment_maintenance_controller.mjs";

const MANIFEST_MAXIMUM_BYTES = 64 * 1_024;
const SOURCE_COMMIT_PATTERN = /^[a-f0-9]{40}$/u;
const SHA256_PATTERN = /^[a-f0-9]{64}$/u;
const RELEASE_PATTERN = /^\d{4}\.\d{2}\.\d{2}-[1-9]\d?$/u;
const INSTANT_PATTERN = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;
const FAMILIES = Object.freeze(["temperature", "wind", "rain"]);
const CAPTURE_EPOCH_MIGRATION_HISTORY_SHA256 =
  "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";
const FIT_SOURCE_FILES = Object.freeze([...new Set(
  Object.values(ADJUSTMENT_FIT_PUBLIC_CODE_ALLOWLIST).flat().map(
    // retain every family source once
    ({ source }) => source,
  ),
)]);

// hash one exact installed manifest
function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// reject caller-selected or linked release roots
async function requireInstalledReleaseRoot(path) {
  const releaseRoot = resolve(path);
  const details = await lstat(releaseRoot);

  // bind the directory name to one immutable source commit
  if (!details.isDirectory() || details.isSymbolicLink() ||
    basename(dirname(releaseRoot)) !== "releases" ||
    !SOURCE_COMMIT_PATTERN.test(basename(releaseRoot)) ||
    await realpath(releaseRoot) !== releaseRoot || (details.mode & 0o777) !== 0o500) {
    throw new Error("installed release root is invalid");
  }
  return { details, releaseRoot, sourceCommit: basename(releaseRoot) };
}

// parse one bounded exact installed manifest
function parseInstalledManifest(bytes) {
  // require one newline-terminated nonempty manifest
  if (!Buffer.isBuffer(bytes) || bytes.length === 0 ||
    bytes.length > MANIFEST_MAXIMUM_BYTES || bytes.at(-1) !== 0x0a) {
    throw new Error("installed release manifest is invalid");
  }
  const entries = new Map();

  // validate every closed relative file binding
  for (const line of bytes.toString("utf8").slice(0, -1).split("\n")) {
    const match = /^([a-f0-9]{64})  ([a-zA-Z0-9][a-zA-Z0-9._@/+\-]*)$/u.exec(line);

    // prohibit duplicate, absolute, or traversal paths
    if (match === null || match[2].startsWith("/") ||
      match[2].split("/").some((segment) => segment === ".." || segment === ".") ||
      entries.has(match[2])) {
      throw new Error("installed release manifest member is invalid");
    }
    entries.set(match[2], match[1]);
  }

  // prohibit an empty reviewed closure
  if (entries.size === 0) {
    throw new Error("installed release manifest is empty");
  }
  // require every fitter source in the reviewed installed closure
  if (FIT_SOURCE_FILES.some((path) => !entries.has(path))) {
    throw new Error("installed release fitter closure is incomplete");
  }
  return entries;
}

// enumerate one immutable regular-file closure without following links
async function inspectInstalledReleaseTree(input) {
  const discovered = new Set();
  const pending = [input.releaseRoot];

  // inspect every directory and direct child exactly once
  while (pending.length > 0) {
    const directory = pending.pop();
    const directoryDetails = await lstat(directory);

    // keep the complete release on one owner-controlled filesystem
    if (!directoryDetails.isDirectory() || directoryDetails.isSymbolicLink() ||
      directoryDetails.uid !== input.uid || directoryDetails.gid !== input.gid ||
      directoryDetails.dev !== input.dev || (directoryDetails.mode & 0o777) !== 0o500 ||
      await realpath(directory) !== directory) {
      throw new Error("installed release directory is invalid");
    }
    const children = await readdir(directory, { withFileTypes: true });

    // reject every link and collect only literal files or directories
    for (const child of children) {
      const path = join(directory, child.name);
      const relativePath = relative(input.releaseRoot, path).split(sep).join("/");

      // recurse only through literal directories
      if (child.isDirectory()) {
        pending.push(path);
      } else if (child.isFile()) {
        discovered.add(relativePath);
      } else {
        throw new Error("installed release member type is invalid");
      }
    }
  }
  return discovered;
}

// prove the copied immutable release against its complete manifest
export async function verifyInstalledAdjustmentRunnerClosure(options = {}) {
  // permit only an isolated release-root override for regression tests
  if (options === null || typeof options !== "object" || Array.isArray(options) ||
    Object.keys(options).some((key) => key !== "releaseRoot")) {
    throw new TypeError("installed release verification options are invalid");
  }
  const defaultRoot = resolve(import.meta.dirname, "../..");
  const authority = await requireInstalledReleaseRoot(options.releaseRoot ?? defaultRoot);
  const manifestPath = join(authority.releaseRoot, "MANIFEST.sha256");
  const manifestDetails = await lstat(manifestPath);

  // require one owner-only immutable manifest on the release filesystem
  if (!manifestDetails.isFile() || manifestDetails.isSymbolicLink() ||
    manifestDetails.uid !== authority.details.uid ||
    manifestDetails.gid !== authority.details.gid ||
    manifestDetails.dev !== authority.details.dev || manifestDetails.nlink !== 1 ||
    (manifestDetails.mode & 0o777) !== 0o400 || await realpath(manifestPath) !== manifestPath) {
    throw new Error("installed release manifest authority is invalid");
  }
  const manifestBytes = await readFile(manifestPath);
  const entries = parseInstalledManifest(manifestBytes);
  const discovered = await inspectInstalledReleaseTree({
    dev: authority.details.dev,
    gid: authority.details.gid,
    releaseRoot: authority.releaseRoot,
    uid: authority.details.uid,
  });
  discovered.delete("MANIFEST.sha256");

  // require the manifest to enumerate every installed runtime byte
  if (discovered.size !== entries.size ||
    [...discovered].some((path) => !entries.has(path))) {
    throw new Error("installed release closure differs from manifest");
  }

  // bind every member to its immutable manifest digest
  for (const [path, expectedSha256] of entries) {
    const absolute = join(authority.releaseRoot, path);
    const details = await lstat(absolute);

    // prohibit links, aliases, foreign files, and writable bytes
    if (!details.isFile() || details.isSymbolicLink() ||
      details.uid !== authority.details.uid || details.gid !== authority.details.gid ||
      details.dev !== authority.details.dev || details.nlink !== 1 ||
      (details.mode & 0o777) !== 0o400 || await realpath(absolute) !== absolute ||
      sha256(await readFile(absolute)) !== expectedSha256) {
      throw new Error("installed release member differs from manifest");
    }
  }
  return Object.freeze({
    manifestSha256: sha256(manifestBytes),
    memberCount: entries.size,
    sourceCommit: authority.sourceCommit,
  });
}

// require one canonical millisecond instant
function requireInstant(value, name) {
  // reject normalized or invalid timestamp substitutions
  if (typeof value !== "string" || !INSTANT_PATTERN.test(value) ||
    new Date(value).toISOString() !== value) {
    throw new TypeError(`${name} is invalid`);
  }
  return value;
}

// load the controller's typed full-ledger reader after its validator is available
async function fetchProductionDatabaseLedger() {
  const controller = await import("./adjustment_maintenance_controller.mjs");

  // refuse a deployment whose reviewed controller lacks the closed reader
  if (typeof controller.fetchAdjustmentMaintenanceDatabaseLedger !== "function") {
    throw new Error("database ledger reader is unavailable");
  }
  return await controller.fetchAdjustmentMaintenanceDatabaseLedger();
}

// close the helper's injectable read-only dependency surface
function captureReadinessPorts(options) {
  // permit only fixed read-only port substitutions for isolated tests
  if (options === null || typeof options !== "object" || Array.isArray(options) ||
    Object.keys(options).some((key) => ![
      "fetchDatabaseLedger", "fetchEpochSnapshot", "fetchEpochWitness",
      "fetchFamilyCurrent", "fetchFamilyLineage", "inspectFitRuntime", "inspectLocalReadiness",
      "verifyInstalledClosure",
    ].includes(key))) {
    throw new TypeError("capture readiness options are invalid");
  }
  const ports = {
    fetchDatabaseLedger: options.fetchDatabaseLedger ?? fetchProductionDatabaseLedger,
    fetchEpochSnapshot: options.fetchEpochSnapshot ?? fetchAdjustmentRevisionCaptureEpochSnapshot,
    fetchEpochWitness: options.fetchEpochWitness ?? fetchAdjustmentRevisionCaptureEpochWitness,
    fetchFamilyCurrent: options.fetchFamilyCurrent ?? fetchAdjustmentFamilyReleaseCurrent,
    fetchFamilyLineage: options.fetchFamilyLineage ?? fetchAdjustmentFamilyReleaseLineage,
    inspectFitRuntime: options.inspectFitRuntime ?? captureAdjustmentFitRuntimeReadiness,
    inspectLocalReadiness: options.inspectLocalReadiness ?? inspectAdjustmentArchiveLocalReadiness,
    verifyInstalledClosure: options.verifyInstalledClosure ??
      verifyInstalledAdjustmentRunnerClosure,
  };

  // reject non-callable boundary substitutions
  if (Object.values(ports).some((port) => typeof port !== "function")) {
    throw new TypeError("capture readiness ports are invalid");
  }
  return ports;
}

// emit one path-free bounded readiness result
function captureReadinessResult(input) {
  return Object.freeze({
    archiveStatus: input.archiveStatus,
    contractVersion: "adjustment-maintenance-capture-readiness/v1",
    databaseMigrationHistorySha256: input.databaseMigrationHistorySha256,
    ready: input.reason === "ready",
    reason: input.reason,
    releaseManifestSha256: input.releaseManifestSha256,
    sourceCommit: input.sourceCommit,
    sourceRelease: input.sourceRelease,
  });
}

// prove every read-only prerequisite without initializing capture state
export async function inspectAdjustmentMaintenanceCaptureReadiness(options = {}) {
  const ports = captureReadinessPorts(options);
  const result = {
    archiveStatus: null,
    databaseMigrationHistorySha256: null,
    reason: "installed_release_unavailable",
    releaseManifestSha256: null,
    sourceCommit: null,
    sourceRelease: null,
  };
  let closure;

  try {
    closure = await ports.verifyInstalledClosure();
  } catch {
    return captureReadinessResult(result);
  }
  result.releaseManifestSha256 = closure.manifestSha256;
  result.sourceCommit = closure.sourceCommit;

  // prove the pinned interpreter and numerical imports before activation
  try {
    await ports.inspectFitRuntime();
  } catch {
    result.reason = "fit_runtime_unavailable";
    return captureReadinessResult(result);
  }
  let local;

  try {
    local = await ports.inspectLocalReadiness();
  } catch {
    result.reason = "local_archive_unready";
    return captureReadinessResult(result);
  }
  result.archiveStatus = local.status ?? null;

  // refuse every local floor or finite-count failure
  if (local.contractVersion !== "adjustment-archive-local-readiness/v1" ||
    local.ready !== true || local.reason !== "ready" || local.status === null) {
    result.reason = "local_archive_unready";
    return captureReadinessResult(result);
  }
  let database;

  try {
    database = await ports.fetchDatabaseLedger();
  } catch {
    result.reason = "database_ledger_unavailable";
    return captureReadinessResult(result);
  }
  let manifest;

  // recheck one exact supported live ledger before using its clock
  try {
    manifest = validateAdjustmentV14RollingDatabaseManifest(database?.databaseManifest);
  } catch {
    result.reason = "database_ledger_unavailable";
    return captureReadinessResult(result);
  }
  if (database?.contractVersion !== "adjustment-database-ledger/v3" ||
    !SHA256_PATTERN.test(manifest.migration_history_sha256)) {
    result.reason = "database_ledger_unavailable";
    return captureReadinessResult(result);
  }
  result.databaseMigrationHistorySha256 = manifest.migration_history_sha256;
  requireInstant(database.snapshotAt, "database snapshotAt");
  let witness;
  let snapshot;

  try {
    witness = await ports.fetchEpochWitness();
    snapshot = await ports.fetchEpochSnapshot();
  } catch {
    result.reason = "capture_epoch_unavailable";
    return captureReadinessResult(result);
  }

  // bind the retained 0021 zero proof to the exact 0021 or 0022 live ledger
  if (witness.databaseMigrationHistorySha256 !== CAPTURE_EPOCH_MIGRATION_HISTORY_SHA256 ||
    snapshot.archiveCommitOrdinal !== "0" || snapshot.entryCount !== 0 ||
    !Array.isArray(snapshot.entries) || snapshot.entries.length !== 0 ||
    snapshot.frontierSha256 !== witness.catalogFrontierSha256 ||
    snapshot.snapshotSha256 !== witness.servingSnapshotSha256 ||
    snapshot.cutoffAt !== witness.epochAt ||
    Date.parse(database.snapshotAt) < Date.parse(witness.epochAt)) {
    result.reason = "capture_epoch_mismatch";
    return captureReadinessResult(result);
  }
  const currents = [];

  try {
    // authenticate every actual family source through its fixed forced verb
    for (const family of FAMILIES) {
      currents.push(await ports.fetchFamilyCurrent(family));
    }
  } catch {
    result.reason = "current_release_unavailable";
    return captureReadinessResult(result);
  }
  const [first] = currents;
  let lineage;

  try {
    lineage = await ports.fetchFamilyLineage();
  } catch {
    result.reason = "current_lineage_unavailable";
    return captureReadinessResult(result);
  }

  // bind one common live release to its transitive root-owned epoch ancestry
  if (first === undefined || !SOURCE_COMMIT_PATTERN.test(first.commit ?? "") ||
    !RELEASE_PATTERN.test(first.release ?? "") ||
    currents.some((current, index) => current.family !== FAMILIES[index] ||
      current.commit !== first.commit || current.release !== first.release ||
      current.sourceServerImageDigest !== first.sourceServerImageDigest) ||
    lineage.currentCommit !== first.commit || lineage.currentRelease !== first.release ||
    lineage.epochWitnessSha256 !== witness.witnessSha256 ||
    lineage.epochAncestorCommit !== witness.sourceCommit ||
    lineage.controlVersion !== witness.controlPlaneVersion ||
    lineage.controlSha256 !== witness.controlPlaneSha256 ||
    Date.parse(lineage.verifiedAt) < Date.parse(witness.epochAt) ||
    closure.sourceCommit !== witness.sourceCommit) {
    result.reason = "current_release_mismatch";
    return captureReadinessResult(result);
  }
  result.reason = "ready";
  result.sourceRelease = first.release;
  return captureReadinessResult(result);
}
