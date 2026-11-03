import { createHash } from "node:crypto";
import { lstat, readFile, realpath } from "node:fs/promises";

import type { JsonValue } from "@weather/domain";

import { canonicalJsonBytes } from "./candidate.js";
import type {
  AdjustmentRevisionProjectionDocument,
} from "./maintenance-revision-projection.js";
import type { MaintenanceShadowSourceProjection } from "./maintenance-shadow-values.js";

export const ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH =
  "/run/weather/adjustment-capture-epoch.json" as const;
export const ADJUSTMENT_REVISION_CAPTURE_EPOCH_VERSION =
  "adjustment-revision-capture-epoch-witness/v1" as const;

const GENESIS_FRONTIER_SHA256 = "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af";
// enumerate reviewed control-history identities
const CAPTURE_EPOCH_IDENTITIES: ReadonlyArray<{
  readonly controlPlaneSha256?: string;
  readonly controlPlaneVersion: "14" | "17";
  readonly databaseMigrationHistorySha256: string;
}> = Object.freeze([
  { controlPlaneVersion: "14",
    databaseMigrationHistorySha256: "c683c4f937c7f02b00f6ab49f75268eead81d2a221a8a9a23e38f9e4802a11b0" },
  { controlPlaneVersion: "14",
    databaseMigrationHistorySha256: "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424" },
  { controlPlaneSha256: "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb",
    controlPlaneVersion: "17",
    databaseMigrationHistorySha256: "107ce430f1ac9478d54d94646da7fa4a54cbe7112db13efd7244d8894d0defdf" },
]);
const HASH = /^[a-f0-9]{64}$/u;
const COMMIT = /^[a-f0-9]{40}$/u;
const IMAGE_DIGEST = /^sha256:[a-f0-9]{64}$/u;
const INSTANT = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;
const RELEASE = /^\d{4}\.\d{2}\.\d{2}-[1-9]\d{0,2}$/u;
const WITNESS_KEYS = ["activationKind", "archiveCommitOrdinal", "catalogFrontierSha256", "contractVersion",
  "controlPlaneSha256", "controlPlaneVersion", "databaseMigrationHistorySha256", "epochAt",
  "servingSnapshotSha256", "sourceCommit", "sourceRelease", "sourceServerImageDigest", "sourceWebImageDigest",
  "witnessSha256"] as const;

export interface AdjustmentRevisionCaptureEpochWitness {
  readonly activationKind: "inert_v14_pre_activation";
  readonly archiveCommitOrdinal: "0";
  readonly catalogFrontierSha256: string;
  readonly contractVersion: typeof ADJUSTMENT_REVISION_CAPTURE_EPOCH_VERSION;
  readonly controlPlaneSha256: string;
  readonly controlPlaneVersion: "14" | "17";
  readonly databaseMigrationHistorySha256: string;
  readonly epochAt: string;
  readonly servingSnapshotSha256: string;
  readonly sourceCommit: string;
  readonly sourceRelease: string;
  readonly sourceServerImageDigest: string;
  readonly sourceWebImageDigest: string;
  readonly witnessSha256: string;
}

// load the one root-installed future-only capture boundary
export async function loadAdjustmentRevisionCaptureEpochWitness(): Promise<AdjustmentRevisionCaptureEpochWitness> {
  await requireRootReadOnlyWitness();
  const bytes = await readFile(ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH);
  return verifyAdjustmentRevisionCaptureEpochWitness(bytes);
}

// verify canonical witness bytes without conferring installed-file authority
export function verifyAdjustmentRevisionCaptureEpochWitness(
  bytes: Uint8Array,
): AdjustmentRevisionCaptureEpochWitness {
  const canonical = Buffer.from(bytes);
  // bound parsing before accepting any root file contents
  if (canonical.byteLength < 2 || canonical.byteLength > 16 * 1_024) {
    throw new RangeError("adjustment capture epoch witness size is invalid");
  }
  const value = JSON.parse(canonical.toString("utf8")) as JsonValue;
  // reject alternate bytes, arrays and extension fields
  if (!record(value) || Object.keys(value).sort().join("\n") !== [...WITNESS_KEYS].sort().join("\n") ||
      canonicalJsonBytes(value) !== canonical.toString("utf8")) {
    throw new RangeError("adjustment capture epoch witness fields differ");
  }
  const witness = value as unknown as AdjustmentRevisionCaptureEpochWitness;
  const { witnessSha256: _witnessSha256, ...unsigned } = witness;
  // match one closed identity tuple
  const identityMatches = CAPTURE_EPOCH_IDENTITIES.some((identity) =>
    identity.controlPlaneVersion === witness.controlPlaneVersion &&
    identity.databaseMigrationHistorySha256 === witness.databaseMigrationHistorySha256 &&
    (identity.controlPlaneSha256 === undefined ||
      identity.controlPlaneSha256 === witness.controlPlaneSha256));
  // bind the actual zero frontier and reviewed control-history pair
  if (witness.contractVersion !== ADJUSTMENT_REVISION_CAPTURE_EPOCH_VERSION ||
      witness.activationKind !== "inert_v14_pre_activation" || witness.archiveCommitOrdinal !== "0" ||
      witness.catalogFrontierSha256 !== GENESIS_FRONTIER_SHA256 || !identityMatches || !validInstant(witness.epochAt) ||
      !COMMIT.test(witness.sourceCommit) || !RELEASE.test(witness.sourceRelease) ||
      !IMAGE_DIGEST.test(witness.sourceServerImageDigest) || !IMAGE_DIGEST.test(witness.sourceWebImageDigest) ||
      !HASH.test(witness.controlPlaneSha256) || !HASH.test(witness.servingSnapshotSha256) ||
      !HASH.test(witness.witnessSha256) || sha256(canonicalJsonBytes(unsigned as JsonValue)) !== witness.witnessSha256) {
    throw new RangeError("adjustment capture epoch witness identity differs");
  }
  return Object.freeze({ ...witness });
}

// reject old reference, initialization, observation or receipt clocks
export function adjustmentRevisionClockIsAfterCaptureEpoch(
  witness: AdjustmentRevisionCaptureEpochWitness,
  value: string,
): boolean {
  return validInstant(value) && Date.parse(value) >= Date.parse(witness.epochAt);
}

// require every source clock consumed by a shadow evaluation to be future-only
export function requireMaintenanceShadowSourceAfterCaptureEpoch(
  witness: AdjustmentRevisionCaptureEpochWitness,
  source: MaintenanceShadowSourceProjection,
): void {
  const clocks: unknown[] = source.rows.flatMap((row) => [row.referenceAt, row.receivedAt]);
  // preserve every exact temperature comparator initialization
  if (source.family === "temperature") {
    clocks.push(...source.rows.map((row) => row.bestMatchProductRunAt));
  }
  // include the complete rolling temperature window rather than its latest edge alone
  if (source.family === "temperature" && source.recentErrorState !== undefined) {
    clocks.push(source.recentErrorState.targetRunInitializedAt);
    clocks.push(new Date(Date.parse(source.recentErrorState.windowEndValidAt) - 71 * 3_600_000).toISOString());
    // preserve the honest empty cold-state nulls
    if (source.recentErrorState.maximumSourceRunInitializedAt !== null) {
      clocks.push(source.recentErrorState.maximumSourceRunInitializedAt);
    }
    // preserve the honest empty cold-state nulls
    if (source.recentErrorState.maximumSourceValidAt !== null) {
      clocks.push(source.recentErrorState.maximumSourceValidAt);
    }
  }
  // bind every rain capture, model run and station observation clock
  if (source.family === "rain" && source.causalInputs !== undefined) {
    for (const capture of source.causalInputs.captureSet) {
      clocks.push(capture.completedAt);
      // retain the mutually exclusive forecast or station source clocks
      if (capture.kind === "forecast") {
        clocks.push(capture.runInitializedAt);
      } else {
        clocks.push(capture.windowStart, capture.windowEndExclusive);
      }
    }
    for (const run of [source.causalInputs.currentRun, ...source.causalInputs.priorRuns]) {
      clocks.push(run.runInitializedAt, run.completedAt);
    }
    for (const station of source.causalInputs.stationHours) {
      clocks.push(station.hourAt, station.receivedAt);
    }
  }
  // refuse an old checkpoint, backfill or causal feature before staging
  if (clocks.some((clock) => typeof clock !== "string" ||
      !adjustmentRevisionClockIsAfterCaptureEpoch(witness, clock))) {
    throw new RangeError("maintenance shadow source predates capture epoch");
  }
}

// require one serving projection's actual source clock to be future-only
export function requireAdjustmentRevisionProjectionAfterCaptureEpoch(
  witness: AdjustmentRevisionCaptureEpochWitness,
  projection: AdjustmentRevisionProjectionDocument,
): void {
  const sourceClock = projection.projectionKind === "actual_best_match"
    ? projection.logicalKey.productRunAt
    : projection.projectionKind === "target_revision"
      ? projection.logicalKey.validAt
      : projection.logicalKey.runInitializedAt;
  // require both the actual provider clock and transaction receipt clock
  if (typeof sourceClock !== "string" ||
      !adjustmentRevisionClockIsAfterCaptureEpoch(witness, sourceClock) ||
      !adjustmentRevisionClockIsAfterCaptureEpoch(witness, projection.logicalReceivedAt)) {
    throw new RangeError("adjustment revision projection predates capture epoch");
  }
  // require every source-decision best-match product to be genuinely post-epoch
  if (projection.contractVersion === "adjustment-temperature-native-source-projection/v2" &&
      projection.rows.some((row) => row.bestMatchProductRunAt !== null &&
        !adjustmentRevisionClockIsAfterCaptureEpoch(witness, String(row.bestMatchProductRunAt)))) {
    throw new RangeError("adjustment temperature comparator predates capture epoch");
  }
}

// require the witness to remain a root-owned read-only file bind
async function requireRootReadOnlyWitness(): Promise<void> {
  const [status, resolved, mountInfo] = await Promise.all([
    lstat(ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH),
    realpath(ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH),
    readFile("/proc/self/mountinfo", "utf8"),
  ]);
  // reject aliases, multiple links, writable modes and non-root ownership
  if (!status.isFile() || status.isSymbolicLink() || status.nlink !== 1 || status.uid !== 0 || status.gid !== 0 ||
      (status.mode & 0o777) !== 0o644 || resolved !== ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH) {
    throw new RangeError("adjustment capture epoch witness authority is invalid");
  }
  const mounted = mountInfo.split("\n").find(
    // select only the exact file mount rather than a parent directory
    (line) => line.split(" ")[4] === ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH,
  );
  // require a literal read-only VFS mount inside the application container
  if (mounted === undefined || !mounted.split(" ")[5]?.split(",").includes("ro")) {
    throw new RangeError("adjustment capture epoch witness mount is not read only");
  }
}

// identify one plain parsed object
function record(value: unknown): value is Record<string, JsonValue> {
  return value !== null && typeof value === "object" && !Array.isArray(value) &&
    [Object.prototype, null].includes(Object.getPrototypeOf(value));
}

// test one canonical millisecond utc instant
function validInstant(value: unknown): value is string {
  return typeof value === "string" && INSTANT.test(value) && Number.isFinite(Date.parse(value)) &&
    new Date(value).toISOString() === value;
}

// hash one exact canonical byte sequence
function sha256(value: string | Uint8Array): string {
  return createHash("sha256").update(value).digest("hex");
}
