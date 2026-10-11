import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { lstat, mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";
import { gunzipSync } from "node:zlib";

import {
  applyForecastAdjustment,
  createForecastAdjustmentWindCanaryRuntimeLoaderForRoot,
  createFixedRainGaugeTarget,
  createRegionalPhysicalStationTarget,
  evaluateForecastAdjustmentPerformance,
  evaluateForecastAdjustmentRainDiagnostics,
  localCalendarFeaturesFor,
  predictRainHurdleWindPerformance,
  prepareForecastAdjustmentPerformancePairs,
  scoreBalancedForecastAdjustmentPairs,
  type ForecastAdjustmentPerformancePair,
} from "@weather/forecast-adjustment";
import {
  forecastLeadBandFor,
  type CanonicalWeatherMetrics,
  type ForecastObservationStationKey,
} from "@weather/domain";
import type { RainAdjustmentCapture } from "@weather/database";

import { rainForecastProfile, rainStationHours } from "./rain-adjustment.js";

const execFileAsync = promisify(execFile);
const REPOSITORY_ROOT = resolve(fileURLToPath(new URL("../../..", import.meta.url)));
const HASH_PATTERN = /^[a-f0-9]{64}$/u;
const GIT_REVISION_PATTERN = /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/u;
const REPORT_VERSION = "forecast-adjustment-performance-report/v1";

type JsonObject = Record<string, unknown>;

export interface ForecastAdjustmentPerformancePackageInputs {
  readonly adjustmentEvidenceManifestSha256: string;
  readonly adjustmentEvidenceWatermarkSha256: string;
  readonly forecastTrainingManifestSha256: string;
  readonly localDateFrom: string;
  readonly localDateTo: string;
  readonly targetCutoffAt: string;
}

export interface VerifiedForecastAdjustmentPerformancePackages {
  readonly adjustmentManifest: JsonObject;
  readonly adjustmentRows: readonly JsonObject[];
  readonly bodies: Readonly<Record<string, Buffer>>;
  readonly edgeRows: readonly {
    readonly bundleIdentities: JsonObject;
    readonly edgeReceiptIdentitySha256: string;
    readonly firstEdgeCommittedAt: string;
    readonly objectSha256: string;
    readonly row: JsonObject;
    readonly rowIndex: number;
    readonly settingsSha256: string;
    readonly sourceReceiptAt: string;
  }[];
  readonly forecastManifest: JsonObject;
  readonly forecastRows: readonly JsonObject[];
  readonly inputs: ForecastAdjustmentPerformancePackageInputs;
}

export interface TemperaturePerformanceDataRow {
  readonly actualTemperatureC: number | null;
  readonly bestMatchRawTemperatureC: number | null;
  readonly cohort: "ecmwf_single_run_hindcast";
  readonly key: string;
  readonly modelCycle: string;
  readonly modelLeadHours: number;
  readonly operationalHorizonHours: number;
  readonly rawRelativeHumidityPercent: number | null;
  readonly rawTemperatureC: number;
  readonly rawWindSpeedMps: number | null;
  readonly recordedRuntimeResult: null | {
    readonly applied: boolean;
    readonly predictionTemperatureC: number;
    readonly reasonCode: string | null;
    readonly scoredPairMetadata: Omit<
      ForecastAdjustmentPerformancePair,
      "adjustedPrediction" | "rawPrediction" | "target"
    >;
    readonly servingBundleSha256: string;
  };
  readonly recentErrorState: unknown;
  readonly runInitializedAt: string;
  readonly scoredPairMetadata: Omit<
    ForecastAdjustmentPerformancePair,
    "adjustedPrediction" | "rawPrediction" | "target"
  >;
  readonly scoreEligible: boolean;
  readonly targetIdentity: string | null;
  readonly targetMaxReceiptAt: string | null;
  readonly validAt: string;
}

export interface TemperaturePerformanceData {
  readonly actualServingFallbacks: readonly JsonObject[];
  readonly inputs: ForecastAdjustmentPerformancePackageInputs;
  readonly rows: readonly TemperaturePerformanceDataRow[];
}

export interface ForecastAdjustmentPerformanceCliDependencies {
  readonly loadPackages?: typeof loadVerifiedPerformancePackages;
  readonly now?: () => Date;
  readonly verifyForecastPackage?: (root: string) => Promise<string>;
  readonly writeOutput?: (value: string) => void;
}

interface PerformanceFamilyReport {
  readonly contractVersion: typeof REPORT_VERSION;
  readonly developmentReference?: JsonObject;
  readonly family: JsonObject;
  readonly generatedAt: string;
  readonly inputs: ForecastAdjustmentPerformancePackageInputs;
  readonly pairReviews?: readonly JsonObject[];
  readonly siteKey: "ballydidean";
  readonly sourceRevision: string;
}

// require one plain JSON object
function object(value: unknown, description: string): JsonObject {
  // reject arrays and special scalar values
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${description} must be an object`);
  }

  return value as JsonObject;
}

// require one string field
function text(value: unknown, description: string): string {
  // reject absent or empty identifiers
  if (typeof value !== "string" || value.length === 0) {
    throw new TypeError(`${description} must be a nonempty string`);
  }

  return value;
}

// require one finite number field
function finite(value: unknown, description: string): number {
  // reject null and nonfinite numeric values
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new TypeError(`${description} must be finite`);
  }

  return value;
}

// retain only finite optional numbers
function nullableFinite(value: unknown, description: string): number | null {
  // preserve explicit missing values
  if (value === null) {
    return null;
  }

  return finite(value, description);
}

// hash immutable bytes for report and target identities
function sha256(value: string | Buffer): string {
  return createHash("sha256").update(value).digest("hex");
}

// normalize JSON recursively for deterministic private reports
function canonicalize(value: unknown): unknown {
  // preserve arrays in their semantic order
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }

  // sort plain object keys recursively
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value as JsonObject).sort().map((key) =>
      [key, canonicalize((value as JsonObject)[key])],
    ));
  }

  // reject nonfinite JSON numbers before publication
  if (typeof value === "number" && !Number.isFinite(value)) {
    throw new TypeError("performance report contains a nonfinite number");
  }

  return value;
}

// serialize one deterministic JSON artifact
function canonicalJson(value: unknown): string {
  return `${JSON.stringify(canonicalize(value))}\n`;
}

// normalize verified timestamp spelling while retaining original package byte identities
export function normalizeVerifiedPerformanceTimestamps(value: unknown, key = ""): unknown {
  // preserve nested closed arrays in their declared order
  if (Array.isArray(value)) {
    return value.map((entry) => normalizeVerifiedPerformanceTimestamps(entry));
  }

  // normalize only fields already verified by the package contract
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([name, entry]) =>
      [name, normalizeVerifiedPerformanceTimestamps(entry, name)]));
  }

  // keep local dates, source identifiers and non-timestamp strings unchanged
  if (typeof value !== "string" ||
    !/(?:At|AtUtc|_at|windowStart|windowEndExclusive)$/u.test(key) ||
    !/^\d{4}-\d{2}-\d{2}T/u.test(value)) {
    return value;
  }

  const parsed = Date.parse(value);

  // stop malformed chronology instead of dropping its evidence
  if (!Number.isFinite(parsed)) {
    throw new RangeError("verified performance timestamp is invalid");
  }

  const fraction = /\.(\d+)(?:Z|[+-])/u.exec(value)?.[1] ?? "";
  const availability = /(?:ReceivedAt|ReceiptAt|received_at|createdAtUtc)$/u.test(key);
  const fractionalRemainder = /[1-9]/u.test(fraction.slice(3));
  return new Date(parsed + Number(availability && fractionalRemainder)).toISOString();
}

// run the existing closed forecast-package verifier
async function verifyForecastTrainingPackage(root: string): Promise<string> {
  const result = await execFileAsync(process.execPath, [
    join(REPOSITORY_ROOT, "deploy/scripts/forecast-training-package.mjs"),
    "verify",
    root,
  ], { cwd: REPOSITORY_ROOT, maxBuffer: 1024 * 1024 });
  const digest = result.stdout.trim();

  // require the verifier's sole content address
  if (!HASH_PATTERN.test(digest)) {
    throw new Error("forecast package verifier returned an invalid digest");
  }

  return digest;
}

// decode members only after the package's own verifier succeeds
async function readVerifiedForecastRows(
  packageRoot: string,
  manifest: JsonObject,
): Promise<readonly JsonObject[]> {
  const members = manifest.members;

  // require the verifier-bound manifest member list
  if (!Array.isArray(members)) {
    throw new TypeError("forecast package members are unavailable");
  }

  const rows: JsonObject[] = [];

  // read only immutable declared JSONL members
  for (const memberValue of members) {
    const member = object(memberValue, "forecast package member");
    const path = text(member.path, "forecast package member path");
    const compressed = await readFile(join(packageRoot, path));

    // rebind materialized bytes to the verified member descriptor
    if (compressed.byteLength !== finite(member.sizeBytes, "forecast package member size") ||
      sha256(compressed) !== text(member.sha256, "forecast package member hash")) {
      throw new Error("forecast package member changed after verification");
    }

    const plaintext = gunzipSync(compressed).toString("utf8");

    // decode every nonempty canonical member row
    for (const line of plaintext.trimEnd().split("\n")) {
      // preserve the verifier-proven nonempty member invariant
      if (line.length > 0) {
        rows.push(object(normalizeVerifiedPerformanceTimestamps(JSON.parse(line)), "forecast package row"));
      }
    }
  }

  return rows;
}

// decode and join immutable edge objects with first-commit receipts
function readEdgeRows(
  edgeEvidence: Readonly<Record<string, Buffer>>,
): VerifiedForecastAdjustmentPerformancePackages["edgeRows"] {
  const receipts: JsonObject[] = [];
  const objects = new Map<string, JsonObject>();

  // decode only package-verifier-bound edge members
  for (const [path, bytes] of Object.entries(edgeEvidence)) {
    // retain receipt bytes as plain JSON
    if (path.includes("/receipts/")) {
      const receipt = object(JSON.parse(bytes.toString("utf8")), "edge receipt");
      receipts.push(receipt);
    // retain bounded object bytes after deterministic decompression
    } else if (path.includes("/objects/")) {
      const objectValue = object(
        JSON.parse(gunzipSync(bytes, { maxOutputLength: 512 * 1024 }).toString("utf8")),
        "edge object",
      );
      const match = /sha256-([a-f0-9]{64})\.json\.gz$/u.exec(path);

      // bind decoded content to its package pathname
      if (match === null) {
        throw new Error("edge object pathname is invalid");
      }

      objects.set(match[1]!, objectValue);
    }
  }

  const rows: VerifiedForecastAdjustmentPerformancePackages["edgeRows"][number][] = [];

  // join each exclusive receipt to its exact ordered object rows
  for (const receipt of receipts) {
    const objectSha256 = text(receipt.objectSha256, "edge receipt object hash");
    const objectValue = objects.get(objectSha256);

    // reject missing object pairs after package verification
    if (objectValue === undefined || !Array.isArray(objectValue.rows)) {
      throw new Error("edge receipt object is unavailable");
    }

    const availability = object(receipt.availability, "edge receipt availability");

    // require aligned availability indexes and timestamps
    if (!Array.isArray(availability.timestamps) ||
      !Array.isArray(availability.rowTimestampIndexes) ||
      availability.rowTimestampIndexes.length !== objectValue.rows.length) {
      throw new Error("edge receipt availability is invalid");
    }

    // retain one causal availability timestamp per ordered row
    for (let index = 0; index < objectValue.rows.length; index += 1) {
      const timestampIndex = availability.rowTimestampIndexes[index];

      // reject an invalid row-to-timestamp reference
      if (!Number.isSafeInteger(timestampIndex) ||
        timestampIndex < 0 || timestampIndex >= availability.timestamps.length) {
        throw new Error("edge receipt availability index is invalid");
      }

      rows.push({
        bundleIdentities: object(objectValue.bundleIdentities, "bundle identities"),
        edgeReceiptIdentitySha256: text(
          receipt.edgeReceiptIdentitySha256,
          "edge receipt identity",
        ),
        firstEdgeCommittedAt: text(
          receipt.firstEdgeCommittedAt,
          "edge first commit",
        ),
        objectSha256,
        row: object(objectValue.rows[index], "edge row"),
        rowIndex: index,
        settingsSha256: text(objectValue.settingsSha256, "edge settings hash"),
        sourceReceiptAt: text(
          availability.timestamps[timestampIndex],
          "edge source receipt",
        ),
      });
    }
  }

  return rows;
}

// retain only global snapshot rows whose valid hour belongs to one package block
function edgeRowsForPackageInterval(
  rows: VerifiedForecastAdjustmentPerformancePackages["edgeRows"],
  fromLocalDate: string,
  toLocalDate: string,
): VerifiedForecastAdjustmentPerformancePackages["edgeRows"] {
  return rows.filter((edge) => {
    const record = object(edge.row.record, "edge record");
    const localDate = localCalendarFeaturesFor(
      text(record.validAt, "edge record validAt"),
    ).localDate;
    return localDate >= fromLocalDate && localDate <= toLocalDate;
  });
}

// verify and load the two same-date file packages without database access
async function loadVerifiedPerformancePackagePair(
  input: {
    readonly adjustmentPackage: string;
    readonly forecastPackage: string;
  },
  dependencies: Pick<ForecastAdjustmentPerformanceCliDependencies, "verifyForecastPackage"> = {},
): Promise<VerifiedForecastAdjustmentPerformancePackages> {
  const forecastPackage = resolve(input.forecastPackage);
  const adjustmentPackage = resolve(input.adjustmentPackage);
  const verifyForecastPackage = dependencies.verifyForecastPackage ??
    verifyForecastTrainingPackage;
  const forecastManifestBytes = await readFile(join(forecastPackage, "manifest.json"));
  const forecastManifestSha256 = await verifyForecastPackage(forecastPackage);

  // rebind the verified digest to the exact bytes read for scoring
  if (sha256(forecastManifestBytes) !== forecastManifestSha256) {
    throw new Error("forecast package changed after verification");
  }

  const forecastManifest = object(
    JSON.parse(forecastManifestBytes.toString("utf8")),
    "forecast package manifest",
  );
  const adjustmentModuleUrl = pathToFileURL(join(
    REPOSITORY_ROOT,
    "deploy/scripts/adjustment-evaluation-package.mjs",
  )).href;
  const adjustmentModule = await import(adjustmentModuleUrl) as {
    readonly loadVerifiedAdjustmentEvaluationPackage: (
      root: string,
    ) => Promise<{
      readonly bodies: Readonly<Record<string, Buffer>>;
      readonly edgeEvidence: Readonly<Record<string, Buffer>>;
      readonly manifest: JsonObject;
      readonly manifestSha256: string;
      readonly rows: readonly JsonObject[];
    }>;
  };
  const adjustment = await adjustmentModule
    .loadVerifiedAdjustmentEvaluationPackage(adjustmentPackage);

  // reject mismatched date/site packages before opening score populations
  if (
    forecastManifest.siteKey !== "ballydidean" ||
    adjustment.manifest.siteKey !== "ballydidean" ||
    forecastManifest.fromLocalDate !== adjustment.manifest.fromLocalDate ||
    forecastManifest.toLocalDate !== adjustment.manifest.toLocalDate
  ) {
    throw new Error("forecast and adjustment packages cover different cohorts");
  }

  const edgeEvidence = object(
    adjustment.manifest.edgeEvidence,
    "adjustment edge evidence",
  );
  return {
    adjustmentManifest: adjustment.manifest,
    adjustmentRows: adjustment.rows.map((row) => object(
      normalizeVerifiedPerformanceTimestamps(row), "adjustment package row",
    )),
    bodies: adjustment.bodies,
    edgeRows: readEdgeRows(adjustment.edgeEvidence),
    forecastManifest,
    forecastRows: await readVerifiedForecastRows(forecastPackage, forecastManifest),
    inputs: {
      adjustmentEvidenceManifestSha256: adjustment.manifestSha256,
      adjustmentEvidenceWatermarkSha256: text(
        edgeEvidence.watermarkSha256,
        "adjustment watermark",
      ),
      forecastTrainingManifestSha256: forecastManifestSha256,
      localDateFrom: text(forecastManifest.fromLocalDate, "forecast from date"),
      localDateTo: text(forecastManifest.toLocalDate, "forecast to date"),
      targetCutoffAt: text(normalizeVerifiedPerformanceTimestamps(forecastManifest.createdAtUtc, "createdAtUtc"), "forecast target cutoff"),
    },
  };
}

// compose already verified package pairs into one immutable local cohort
export function composeVerifiedPerformancePackagePairs(
  inputPairs: readonly VerifiedForecastAdjustmentPerformancePackages[],
): VerifiedForecastAdjustmentPerformancePackages {
  const pairs = [...inputPairs].sort((left, right) =>
    left.inputs.localDateFrom.localeCompare(right.inputs.localDateFrom));

  // reject empty success before reading any composition identities
  if (pairs.length === 0) {
    throw new Error("performance package composition requires package pairs");
  }

  // reject overlapping or reordered local-date cohorts
  for (let index = 1; index < pairs.length; index += 1) {
    const previous = pairs[index - 1]!;
    const current = pairs[index]!;

    // keep target and adjustment snapshots in disjoint blocks
    if (previous.inputs.localDateTo >= current.inputs.localDateFrom) {
      throw new Error("performance package date intervals overlap");
    }
  }

  // preserve single-pair hashes without synthetic composition identities
  if (pairs.length === 1) {
    const pair = pairs[0]!;
    return {
      ...pair,
      edgeRows: edgeRowsForPackageInterval(
        pair.edgeRows,
        pair.inputs.localDateFrom,
        pair.inputs.localDateTo,
      ),
    };
  }

  const identities = pairs.map((pair) => pair.inputs);
  const bodies: Record<string, Buffer> = {};
  const edgeRows = new Map<
    string,
    VerifiedForecastAdjustmentPerformancePackages["edgeRows"][number]
  >();
  const globalEdgeRows = new Map<
    string,
    VerifiedForecastAdjustmentPerformancePackages["edgeRows"][number]
  >();

  // merge only content-addressed body paths with identical bytes
  for (const pair of pairs) {
    // retain every verified body member under its content address
    for (const [path, bytes] of Object.entries(pair.bodies)) {
      const existing = bodies[path];

      // reject a cross-package path collision
      if (existing !== undefined && !existing.equals(bytes)) {
        throw new Error("performance package body identity collision");
      }

      bodies[path] = bytes;
    }

    // validate duplicate receipt rows even when outside this package's date block
    for (const edge of pair.edgeRows) {
      const identity = `${edge.edgeReceiptIdentitySha256}:${String(edge.rowIndex)}`;
      const existing = globalEdgeRows.get(identity);

      // stop if one receipt-row identity maps to different scoring content
      if (existing !== undefined && canonicalJson(existing) !== canonicalJson(edge)) {
        throw new Error("performance edge receipt row identity collision");
      }

      globalEdgeRows.set(identity, existing ?? edge);
    }

    const boundedEdgeRows = edgeRowsForPackageInterval(
      pair.edgeRows,
      pair.inputs.localDateFrom,
      pair.inputs.localDateTo,
    );

    // retain one conservative copy of each in-block global receipt row
    for (const edge of boundedEdgeRows) {
      const identity = `${edge.edgeReceiptIdentitySha256}:${String(edge.rowIndex)}`;
      edgeRows.set(identity, edgeRows.get(identity) ?? edge);
    }
  }

  return {
    adjustmentManifest: {
      contractVersion: "adjustment-evaluation-export-composition/v1",
      packages: pairs.map((pair) => ({
        fromLocalDate: pair.inputs.localDateFrom,
        manifestSha256: pair.inputs.adjustmentEvidenceManifestSha256,
        toLocalDate: pair.inputs.localDateTo,
      })),
    },
    adjustmentRows: pairs.flatMap((pair) => pair.adjustmentRows),
    bodies,
    edgeRows: [...edgeRows.values()].sort((left, right) =>
      left.edgeReceiptIdentitySha256.localeCompare(right.edgeReceiptIdentitySha256) ||
      left.rowIndex - right.rowIndex),
    forecastManifest: {
      contractVersion: "forecast-training-export-composition/v1",
      packages: pairs.map((pair) => ({
        fromLocalDate: pair.inputs.localDateFrom,
        manifestSha256: pair.inputs.forecastTrainingManifestSha256,
        toLocalDate: pair.inputs.localDateTo,
      })),
    },
    forecastRows: pairs.flatMap((pair) => pair.forecastRows),
    inputs: {
      adjustmentEvidenceManifestSha256: sha256(canonicalJson(
        identities.map((identity) => identity.adjustmentEvidenceManifestSha256),
      )),
      adjustmentEvidenceWatermarkSha256: sha256(canonicalJson(
        identities.map((identity) => identity.adjustmentEvidenceWatermarkSha256),
      )),
      forecastTrainingManifestSha256: sha256(canonicalJson(
        identities.map((identity) => identity.forecastTrainingManifestSha256),
      )),
      localDateFrom: pairs[0]!.inputs.localDateFrom,
      localDateTo: pairs.at(-1)!.inputs.localDateTo,
      targetCutoffAt: pairs.map((pair) => pair.inputs.targetCutoffAt).sort().at(-1)!,
    },
  };
}

// verify and compose one or more disjoint file-only package pairs
export async function loadVerifiedPerformancePackages(
  input: {
    readonly adjustmentPackage: string | readonly string[];
    readonly forecastPackage: string | readonly string[];
  },
  dependencies: Pick<ForecastAdjustmentPerformanceCliDependencies, "verifyForecastPackage"> = {},
): Promise<VerifiedForecastAdjustmentPerformancePackages> {
  const forecastPackages = typeof input.forecastPackage === "string"
    ? [input.forecastPackage] : [...input.forecastPackage];
  const adjustmentPackages = typeof input.adjustmentPackage === "string"
    ? [input.adjustmentPackage] : [...input.adjustmentPackage];

  // require one matched adjustment package for every target package
  if (forecastPackages.length === 0 ||
    forecastPackages.length !== adjustmentPackages.length) {
    throw new Error("performance package composition requires matched package pairs");
  }

  const pairs: VerifiedForecastAdjustmentPerformancePackages[] = [];

  // verify each bounded pair independently before composition
  for (let index = 0; index < forecastPackages.length; index += 1) {
    pairs.push(await loadVerifiedPerformancePackagePair({
      adjustmentPackage: adjustmentPackages[index]!,
      forecastPackage: forecastPackages[index]!,
    }, dependencies));
  }

  return composeVerifiedPerformancePackagePairs(pairs);
}

// bind independent source-hour revision evidence without replacing selected target values
function targetRevisionEvidence(
  adjustmentRows: readonly JsonObject[],
  targetRows: readonly JsonObject[],
  validAt: string,
): readonly JsonObject[] {
  const end = Date.parse(validAt);
  const start = end - 3_600_000;

  return adjustmentRows.filter((row) => {
    // retain only verified physical observation revision aggregates
    if (row.record_kind !== "target_revision_diagnostic") {
      return false;
    }

    const payload = object(row.payload, "target revision evidence");
    const hour = Date.parse(text(payload.validAt, "target revision hour"));

    // cover instantaneous boundary neighbors and the preceding gust hour conservatively
    return hour >= start && hour <= end && targetRows.some((target) =>
      target.physical_station_key === payload.physicalStationKey &&
      Array.isArray(target.source_keys) && target.source_keys.includes(payload.sourceKey) &&
      Array.isArray(target.source_config_fingerprints) &&
      target.source_config_fingerprints.includes(payload.sourceConfigFingerprint));
  }).map((row) => object(row.payload, "target revision evidence"))
    .sort((left, right) => canonicalJson(left).localeCompare(canonicalJson(right)));
}

// derive one regional temperature target and immutable target identity
function temperatureTarget(
  forecastRows: readonly JsonObject[],
  validAt: string,
  adjustmentRows: readonly JsonObject[],
): {
  readonly maxReceiptAt: string | null;
  readonly target: number | null;
  readonly targetIdentity: string | null;
} {
  const rows = forecastRows.filter((row) =>
    row.record_kind === "station_hour" &&
    row.valid_at === validAt &&
    row.physical_station_key !== "ballydidean-ecowitt" &&
    row.temperature_c !== null);
  const values = rows.map((row) => ({
    physicalStationKey: text(
      row.physical_station_key,
      "temperature physical station",
    ) as ForecastObservationStationKey,
    value: finite(row.temperature_c, "temperature target"),
  }));
  const target = createRegionalPhysicalStationTarget(values);

  // preserve explicit target coverage gaps
  if (target === null) {
    return { maxReceiptAt: null, target: null, targetIdentity: null };
  }

  const receipts = rows.flatMap((row) =>
    typeof row.received_at === "string" ? [row.received_at] : []);
  const revisionEvidence = targetRevisionEvidence(adjustmentRows, rows, validAt);

  // keep revised label availability at the conservative captured last receipt
  for (const evidence of revisionEvidence) {
    receipts.push(text(evidence.maxSourceReceiptAt, "target maximum source receipt"));
  }
  const identity = rows.map((row) => ({
    contentHashes: row.content_hashes,
    physicalStationKey: row.physical_station_key,
    receivedAt: row.received_at,
    sourceKeys: row.source_keys,
    validAt: row.valid_at,
  })).sort((left, right) =>
    String(left.physicalStationKey).localeCompare(String(right.physicalStationKey)));
  return {
    maxReceiptAt: receipts.sort().at(-1) ?? null,
    target: target.value,
    targetIdentity: sha256(canonicalJson({ selectedTarget: identity, revisionEvidence })),
  };
}

// derive one regional scalar target for a supported forecast-training metric
function regionalTarget(
  forecastRows: readonly JsonObject[],
  validAt: string,
  metric: "wind_gust_mps" | "wind_speed_mps",
  adjustmentRows: readonly JsonObject[],
): { readonly identity: string; readonly value: number } | null {
  const rows = forecastRows.filter((row) =>
    row.record_kind === "station_hour" && row.valid_at === validAt &&
    row.physical_station_key !== "ballydidean-ecowitt" &&
    row[metric] !== null);
  const target = createRegionalPhysicalStationTarget(rows.map((row) => ({
    physicalStationKey: text(
      row.physical_station_key,
      "wind physical station",
    ) as ForecastObservationStationKey,
    value: finite(row[metric], "wind target"),
  })));

  // preserve regional coverage gaps without substituting Ecowitt
  if (target === null) {
    return null;
  }

  return {
    identity: sha256(canonicalJson({ revisionEvidence: targetRevisionEvidence(adjustmentRows, rows, validAt),
      selectedTarget: rows.map((row) => ({
      contentHashes: row.content_hashes,
      physicalStationKey: row.physical_station_key,
      receivedAt: row.received_at,
      sourceKeys: row.source_keys,
      validAt: row.valid_at,
    })).sort((left, right) =>
      String(left.physicalStationKey).localeCompare(String(right.physicalStationKey))) })),
    value: target.value,
  };
}

// build a complete canonical metrics object for native wind replay
function windMetrics(row: JsonObject): CanonicalWeatherMetrics {
  return {
    apparentTemperatureC: null,
    blackGlobeTemperatureC: null,
    cloudCoverPercent: null,
    pm25MicrogramsPerCubicMeter: null,
    precipitationMm: null,
    precipitationRateMmPerHour: null,
    pressureHpa: null,
    relativeHumidityPercent: nullableFinite(row.relative_humidity_percent, "raw humidity"),
    soilElectricalConductivityMicrosiemensPerCm: null,
    soilMoisturePercent: null,
    solarRadiationWm2: null,
    temperatureC: nullableFinite(row.temperature_c, "raw temperature"),
    uvIndex: null,
    waterLevelM: null,
    windDirectionDegrees: nullableFinite(row.wind_direction_degrees, "raw direction"),
    windGustMps: nullableFinite(row.wind_gust_mps, "raw gust"),
    windSpeedMps: nullableFinite(row.wind_speed_mps, "raw speed"),
    wetBulbGlobeTemperatureC: null,
  };
}

// build an all-null scorecard metric with an explicit unit
function unavailableMetric(unit: string): JsonObject {
  return {
    adjustedBias: null,
    adjustedMae: null,
    adjustedP95: null,
    adjustedRmse: null,
    deltaMae: null,
    rawBias: null,
    rawMae: null,
    rawP95: null,
    rawRmse: null,
    skillInterval95: null,
    skillPercent: null,
    unit,
  };
}

// project detailed paired metrics into the closed scorecard aggregate
function scorecardMetric(
  evaluation: ReturnType<typeof evaluateForecastAdjustmentPerformance> | null,
  unit: string,
): JsonObject {
  const metrics = evaluation?.metrics;

  // preserve unavailable metrics below support gates
  if (metrics === null || metrics === undefined) {
    return unavailableMetric(unit);
  }

  const bootstrap = evaluation?.bootstrap ?? null;

  return {
    adjustedBias: metrics.adjusted.bias,
    adjustedMae: metrics.adjusted.mae,
    adjustedP95: metrics.adjusted.p95AbsoluteError,
    adjustedRmse: metrics.adjusted.rmse,
    deltaMae: metrics.delta.mae,
    rawBias: metrics.raw.bias,
    rawMae: metrics.raw.mae,
    rawP95: metrics.raw.p95AbsoluteError,
    rawRmse: metrics.raw.rmse,
    skillInterval95: bootstrap === null ? null : {
      lower: bootstrap.lowerSkill * 100,
      upper: bootstrap.upperSkill * 100,
    },
    skillPercent: metrics.skill * 100,
    unit,
  };
}

// score one descriptive slice without repeating the aggregate bootstrap
function descriptiveSliceMetric(
  rows: readonly ForecastAdjustmentPerformancePair[],
  minimumRows: number,
  unit: string,
): JsonObject {
  const dateCount = new Set(rows.map((row) => row.localDate)).size;

  // preserve unsupported slices as explicit null metrics
  if (rows.length < minimumRows || dateCount < 7) {
    return unavailableMetric(unit);
  }

  const metrics = scoreBalancedForecastAdjustmentPairs(rows).metrics;
  return {
    adjustedBias: metrics.adjusted.bias,
    adjustedMae: metrics.adjusted.mae,
    adjustedP95: metrics.adjusted.p95AbsoluteError,
    adjustedRmse: metrics.adjusted.rmse,
    deltaMae: metrics.delta.mae,
    rawBias: metrics.raw.bias,
    rawMae: metrics.raw.mae,
    rawP95: metrics.raw.p95AbsoluteError,
    rawRmse: metrics.raw.rmse,
    skillInterval95: null,
    skillPercent: metrics.skill * 100,
    unit,
  };
}

// build fixed local-calendar descriptive slices for one disjoint population
function calendarSlices(
  rows: readonly ForecastAdjustmentPerformancePair[],
  minimumRows: number,
  unit: string,
): readonly JsonObject[] {
  const dimensions = ["month", "season", "daypart"] as const;
  const slices: JsonObject[] = [];

  // partition each row independently under the pinned local calendar
  for (const dimension of dimensions) {
    const groups = new Map<string, ForecastAdjustmentPerformancePair[]>();

    // retain one row in exactly one label for this calendar dimension
    for (const row of rows) {
      const calendar = localCalendarFeaturesFor(row.validAt);
      const label = dimension === "month"
        ? String(calendar.month).padStart(2, "0")
        : calendar[dimension];
      const group = groups.get(label) ?? [];
      group.push(row);
      groups.set(label, group);
    }

    // emit stable labels without manufacturing absent calendar groups
    for (const [label, group] of [...groups.entries()].sort()) {
      slices.push({
        dimension,
        label,
        metrics: descriptiveSliceMetric(group, minimumRows, unit),
        rowCount: group.length,
      });
    }
  }

  return slices;
}

// count explicit excluded reasons in stable lexical order
function exclusionCount(prepared: ReturnType<
  typeof prepareForecastAdjustmentPerformancePairs
>): number {
  return Object.values(prepared.exclusions).reduce((sum, count) => sum + count, 0);
}

// build one closed support aggregate from prepared paired rows
function scorecardSupport(
  prepared: ReturnType<typeof prepareForecastAdjustmentPerformancePairs>,
  inputRowCount: number,
): JsonObject {
  const dates = new Set(prepared.rows.map((row) => row.localDate));
  const validHours = new Set(prepared.rows.map((row) => row.validAt));
  const vintages = new Set(prepared.rows.map((row) => row.vintageKey));
  return {
    dateCount: dates.size,
    effectiveWeightSum: prepared.rows.length === 0 ? 0 : 1,
    eventCount: prepared.rows.length,
    excludedCount: exclusionCount(prepared),
    exclusionReasons: { ...prepared.exclusions, ...prepared.diagnostics },
    fallbackCount: prepared.fallbackCount,
    fallbackReasons: prepared.fallbackCount === 0
      ? {} : { raw_fallback: prepared.fallbackCount },
    gapCount: inputRowCount - prepared.rows.length +
      prepared.diagnostics.provenance_incomplete,
    rowCount: inputRowCount,
    targetRowCount: prepared.rows.length,
    validHourCount: validHours.size,
    vintageCount: vintages.size,
    wetDateCount: 0,
    wetRowCount: 0,
  };
}

// evaluate one possibly empty population without manufacturing success
function optionalEvaluation(
  rows: readonly ForecastAdjustmentPerformancePair[],
  minimumRows: number,
  minimumDates = 7,
): ReturnType<typeof evaluateForecastAdjustmentPerformance> | null {
  // preserve empty evidence as explicit invalid/unscored card state
  if (rows.length === 0) {
    return null;
  }

  return evaluateForecastAdjustmentPerformance(rows, {
    minimumDates,
    minimumRows,
    servingState: "pending_review",
  });
}

// resolve the exact source revision recorded by every family report
async function sourceRevision(
  values: ReadonlyMap<string, readonly string[]>,
): Promise<string> {
  const supplied = values.get("--source-revision");

  // permit an explicit immutable CI revision
  if (supplied !== undefined) {
    const revision = oneOption(values, "--source-revision");

    // require one full Git object identity rather than an abbreviation
    if (!GIT_REVISION_PATTERN.test(revision)) {
      throw new Error("source revision is invalid");
    }

    return revision;
  }

  const result = await execFileAsync("git", ["rev-parse", "HEAD"], {
    cwd: REPOSITORY_ROOT,
    maxBuffer: 1024,
  });
  const revision = result.stdout.trim();

  // reject detached or unexpected revision output
  if (!GIT_REVISION_PATTERN.test(revision)) {
    throw new Error("source revision is invalid");
  }

  return revision;
}

// build all thirteen active wind/gust pair populations on the live-v4 baseline
async function createWindReport(
  packages: VerifiedForecastAdjustmentPerformancePackages,
  revision: string,
  now: Date,
): Promise<PerformanceFamilyReport> {
  const loader = createForecastAdjustmentWindCanaryRuntimeLoaderForRoot(
    join(REPOSITORY_ROOT, "config/forecast-adjustments"),
    { now: () => now.toISOString() },
  );
  const runtime = await loader.load();

  // require the frozen active bundle rather than inventing a candidate runtime
  if (runtime.state !== "active") {
    throw new Error(`active wind runtime is unavailable: ${runtime.reasonCode}`);
  }

  // score the exact mask carried by either qualified runtime grammar
  const enabled = "maintenanceBundleSha256" in runtime.bundle
    ? runtime.bundle.candidate.enabledMetricBands
    : runtime.bundle.authorization.enabledMetricBands;
  const counterfactualByBand = new Map<string, ForecastAdjustmentPerformancePair[]>(
    enabled.map((pair) => [`${pair.metric}:${pair.leadBand}`, []]),
  );
  const issuedByBand = new Map<string, ForecastAdjustmentPerformancePair[]>(
    enabled.map((pair) => [`${pair.metric}:${pair.leadBand}`, []]),
  );
  const enabledWindMetrics = new Set(enabled.map((pair) => pair.metric));
  const issuedNonActiveReasons = { disabled: 0, not_applicable: 0 };
  let issuedNonActiveCount = 0;
  const forecasts = packages.forecastRows.filter((row) =>
    row.record_kind === "legacy_v4_retrieval_snapshot");

  // replay the active bundle against each exact live-v4 raw row
  for (const row of forecasts) {
    const validAt = text(row.valid_at, "wind validAt");
    const targetLeadHours = finite(row.target_lead_hours, "wind target lead");
    const decision = applyForecastAdjustment(runtime, {
      evaluatedAt: now.toISOString(),
      metrics: windMetrics(row),
      rawForecastProvenance: {
        adapterVersion: text(
          (row.adapter_contracts as unknown[])[0],
          "wind adapter version",
        ),
        cohort: "legacy_v4_retrieval_snapshot",
        contractEpoch: text(row.contract_epoch, "wind contract epoch"),
        dataset: text(row.dataset, "wind dataset"),
        referenceAt: text(row.reference_at, "wind referenceAt"),
        referenceKind: "retrieval_snapshot",
        sourceConfigFingerprint: text(
          (row.source_config_fingerprints as unknown[])[0],
          "wind source fingerprint",
        ),
        sourceKey: text((row.source_keys as unknown[])[0], "wind source key"),
        targetLeadHours,
        upstreamModel: text(row.upstream_model, "wind upstream model"),
        validAt,
      },
    });
    const leadBand = decision.state === "active" ? decision.leadBand : null;

    // score speed and gust independently on the same Best Match row
    for (const definition of enabled) {
      // exclude direction if a future runtime widens the canary type
      if (definition.metric !== "windSpeedMps" &&
        definition.metric !== "windGustMps") {
        continue;
      }

      // retain only the row's exact enabled lead band
      if (definition.leadBand !== leadBand) {
        continue;
      }

      const metricField = definition.metric === "windSpeedMps"
        ? "wind_speed_mps" : "wind_gust_mps";
      const raw = row[metricField];

      // preserve missing source values as explicit non-events
      if (raw === null) {
        continue;
      }

      const target = regionalTarget(packages.forecastRows, validAt, metricField, packages.adjustmentRows);
      const adjusted = decision.state === "active"
        ? decision.adjustedMetrics[definition.metric]
        : undefined;
      const pairKey = `${definition.metric}:${definition.leadBand}`;
      const pairs = counterfactualByBand.get(pairKey);

      // retain only the frozen enabled thirteen-pair set
      if (pairs === undefined) {
        throw new Error("wind pair is outside the active mask");
      }

      pairs.push({
        adjustedPrediction: adjusted ?? finite(raw, "wind raw value"),
        evidenceClass: "retrospective_counterfactual",
        fallback: adjusted === undefined,
        firstEdgeCommittedAt: null,
        horizonHours: targetLeadHours,
        key: `${pairKey}:${text(row.reference_at, "wind referenceAt")}:${validAt}`,
        localDate: localCalendarFeaturesFor(validAt).localDate,
        provenanceComplete: true,
        rawPrediction: finite(raw, "wind raw value"),
        rowIdentity: sha256(canonicalJson({
          contentHashes: row.content_hashes,
          definition,
          referenceAt: row.reference_at,
          validAt,
        })),
        sourceReceiptAt: null,
        target: target?.value ?? null,
        targetKey: target?.identity ?? `missing:${validAt}:${definition.metric}`,
        validAt,
        vintageKey: text(row.reference_at, "wind referenceAt"),
      });
    }
  }

  // build the disjoint receipt-backed cohort from the same Best Match baseline
  for (const edge of packages.edgeRows) {
    const bundles = object(edge.bundleIdentities.wind, "edge wind bundle");

    // retain only rows served by the exact active bundle under review
    if (bundles.activeBundle !== runtime.bundle.bundleSha256) {
      continue;
    }

    const record = object(edge.row.record, "edge wind record");
    const adjustment = object(edge.row.windAdjustment, "edge wind adjustment");
    const raw = object(edge.row.raw, "edge wind raw metrics");
    const adjusted = object(adjustment.adjustedMetrics, "edge adjusted wind metrics");
    const validAt = text(record.validAt, "edge wind validAt");
    const appliedMetrics = adjustment.appliedMetrics;
    const state = text(adjustment.state, "edge wind state");

    // reject malformed serving details before descriptive scoring
    if (!Number.isFinite(Date.parse(validAt)) || !Array.isArray(appliedMetrics)) {
      throw new Error("edge wind adjustment identity is invalid");
    }

    // reject unknown states instead of treating them as raw fallbacks
    if (state !== "active" && state !== "disabled" && state !== "not_applicable") {
      throw new Error("edge wind adjustment state is invalid");
    }

    // count fail-raw decisions as unscored fallback gaps without inventing a band
    if (state !== "active") {
      // count each captured enabled raw metric once
      for (const metric of ["windSpeedMps", "windGustMps"] as const) {
        // ignore metrics outside the exact active mask or absent from this row
        if (!enabledWindMetrics.has(metric) || raw[metric] === null ||
          raw[metric] === undefined) {
          continue;
        }

        finite(raw[metric], "edge raw wind value");
        issuedNonActiveReasons[state] += 1;
        issuedNonActiveCount += 1;
      }
      continue;
    }

    const productRunAt = text(record.productRunAt, "edge wind product run");
    const continuousHorizonHours =
      (Date.parse(validAt) - Date.parse(productRunAt)) / 3_600_000;

    // require a finite positive runtime lead only for an active decision
    if (!Number.isFinite(continuousHorizonHours) || continuousHorizonHours <= 0) {
      throw new Error("edge wind adjustment identity is invalid");
    }

    const horizonHours = Math.ceil(continuousHorizonHours);
    const leadBand = text(adjustment.leadBand, "edge wind lead band");

    // bind the captured band to the runtime's ceiling-based lead policy
    if (leadBand !== forecastLeadBandFor(horizonHours)) {
      throw new Error("edge wind lead band is invalid");
    }

    // score each enabled metric independently on its captured raw row
    for (const definition of enabled) {
      // retain only the captured row's exact metric and lead band
      if (definition.leadBand !== leadBand ||
        (definition.metric !== "windSpeedMps" &&
          definition.metric !== "windGustMps")) {
        continue;
      }

      const rawValue = raw[definition.metric];

      // preserve a missing captured source metric as a non-event
      if (rawValue === null || rawValue === undefined) {
        continue;
      }

      const target = regionalTarget(
        packages.forecastRows,
        validAt,
        definition.metric === "windSpeedMps"
          ? "wind_speed_mps" : "wind_gust_mps",
        packages.adjustmentRows,
      );
      const wasApplied = appliedMetrics.includes(definition.metric);
      const adjustedValue = wasApplied
        ? finite(adjusted[definition.metric], "edge adjusted wind value")
        : finite(rawValue, "edge raw wind value");
      const pairKey = `${definition.metric}:${definition.leadBand}`;
      issuedByBand.get(pairKey)!.push({
        adjustedPrediction: adjustedValue,
        evidenceClass: "as_issued",
        fallback: !wasApplied,
        firstEdgeCommittedAt: edge.firstEdgeCommittedAt,
        horizonHours,
        key: `${pairKey}:${productRunAt}:${validAt}`,
        localDate: localCalendarFeaturesFor(validAt).localDate,
        provenanceComplete: edge.row.provenanceComplete === true,
        rawPrediction: finite(rawValue, "edge raw wind value"),
        rowIdentity: sha256(canonicalJson({
          activeBundle: bundles.activeBundle,
          definition,
          recordId: record.id,
          recordRevision: record.revisionCount,
          settingsSha256: edge.settingsSha256,
        })),
        sourceReceiptAt: edge.sourceReceiptAt,
        target: target?.value ?? null,
        targetKey: target?.identity ?? `missing:${validAt}:${definition.metric}`,
        validAt,
        vintageKey: productRunAt,
      });
    }
  }

  const issuedRowCount = [...issuedByBand.values()].reduce(
    (sum, pairs) => sum + pairs.length,
    0,
  );
  const issuedInputRowCount = issuedRowCount + issuedNonActiveCount;
  const evidenceClass = issuedInputRowCount > 0
    ? "as_issued" : "retrospective_counterfactual";
  const byBand = issuedInputRowCount > 0 ? issuedByBand : counterfactualByBand;

  const allPreparedRows: ForecastAdjustmentPerformancePair[] = [];
  const pairReviews: JsonObject[] = [];
  const slices: JsonObject[] = [];
  let inputRows = 0;
  const combinedExclusions: Record<string, number> = {};
  let fallbackCount = 0;
  let excludedCount = 0;

  // evaluate all thirteen pair populations independently
  for (const [key, pairs] of [...byBand.entries()].sort()) {
    const prepared = prepareForecastAdjustmentPerformancePairs(pairs);
    const evaluation = optionalEvaluation(prepared.rows, 30);
    allPreparedRows.push(...prepared.rows);
    inputRows += pairs.length;
    fallbackCount += prepared.fallbackCount;
    excludedCount += exclusionCount(prepared);

    // sum bounded exclusion reasons across pair reports
    for (const [reason, count] of Object.entries(prepared.exclusions)) {
      combinedExclusions[reason] = (combinedExclusions[reason] ?? 0) + count;
    }

    // retain descriptive-only incomplete provenance counts
    for (const [reason, count] of Object.entries(prepared.diagnostics)) {
      combinedExclusions[reason] = (combinedExclusions[reason] ?? 0) + count;
    }

    slices.push({
      dimension: "horizon",
      label: key.toLowerCase().replaceAll(":", "-"),
      metrics: descriptiveSliceMetric(
        prepared.rows,
        30,
        "meters_per_second",
      ),
      rowCount: prepared.rows.length,
    });
    const comparisonState = evaluation?.comparisonState ?? "unscored";
    const qualificationState = evaluation?.qualificationState ??
      (evidenceClass === "as_issued" ? "pending_support" : "counterfactual_only");
    pairReviews.push({
      comparisonState,
      dateCount: new Set(prepared.rows.map((pair) => pair.localDate)).size,
      metrics: scorecardMetric(evaluation, "meters_per_second"),
      pairIdentity: key,
      qualificationState,
      recommendation: comparisonState === "worse"
        ? "review_disable"
        : evidenceClass === "as_issued" && qualificationState === "supported"
          ? "retain"
          : "review_candidate",
      rowCount: prepared.rows.length,
      supportState: evaluation?.supportState ?? "insufficient",
    });
  }

  inputRows += issuedNonActiveCount;
  fallbackCount += issuedNonActiveCount;
  excludedCount += issuedNonActiveCount;

  // preserve each fail-raw state as an explicit exclusion reason
  for (const [state, count] of Object.entries(issuedNonActiveReasons)) {
    // omit absent states instead of manufacturing zero-valued diagnostics
    if (count > 0) {
      combinedExclusions[state] = count;
    }
  }

  const everyPairSupported = pairReviews.every((review) =>
    review.supportState === "sufficient");
  const anyPairWorse = pairReviews.some((review) =>
    review.comparisonState === "worse");
  const anyPairUnscored = pairReviews.some((review) =>
    review.comparisonState === "unscored");
  const anyPairMixed = pairReviews.some((review) =>
    review.comparisonState === "mixed");
  const everyPairQualified = pairReviews.every((review) =>
    review.qualificationState === "supported");
  const anyPairRejected = pairReviews.some((review) =>
    review.qualificationState === "rejected");
  const familyComparison = anyPairWorse
    ? "worse"
    : anyPairUnscored
      ? "unscored"
      : anyPairMixed ? "mixed" : "better";
  const familyQualification = evidenceClass === "retrospective_counterfactual"
    ? "counterfactual_only"
    : anyPairRejected
      ? "rejected"
      : everyPairQualified ? "supported" : "pending_support";
  const support = scorecardSupport(
    prepareForecastAdjustmentPerformancePairs(allPreparedRows),
    inputRows,
  );
  support.exclusionReasons = combinedExclusions;
  support.excludedCount = excludedCount;
  support.fallbackCount = fallbackCount;
  support.fallbackReasons = fallbackCount === 0 ? {} : { raw_fallback: fallbackCount };
  return {
    contractVersion: REPORT_VERSION,
    family: {
      bestMatchDiagnostic: null,
      comparisonState: familyComparison,
      evidenceClass,
      evidenceCutoffAt: packages.inputs.targetCutoffAt,
      family: "wind",
      metrics: everyPairSupported
        ? descriptiveSliceMetric(allPreparedRows, 1, "meters_per_second")
        : unavailableMetric("meters_per_second"),
      qualificationState: familyQualification,
      rainDiagnostics: null,
      recommendation: anyPairWorse
        ? "review_disable" : "review_candidate",
      servingIdentitySha256: runtime.bundle.bundleSha256,
      servingState: "authorized_active",
      slices: [...slices, ...calendarSlices(
        allPreparedRows,
        30,
        "meters_per_second",
      )],
      support,
      supportState: everyPairSupported ? "sufficient" : "insufficient",
    },
    generatedAt: now.toISOString(),
    inputs: packages.inputs,
    pairReviews,
    siteKey: "ballydidean",
    sourceRevision: revision,
  };
}

// reconstruct verified private rain captures from package rows and body members
function rainCaptures(
  packages: VerifiedForecastAdjustmentPerformancePackages,
): readonly RainAdjustmentCapture[] {
  const claims = new Map<string, JsonObject>();

  // index every immutable capture claim
  for (const row of packages.adjustmentRows) {
    // retain only rain claim records
    if (row.record_kind === "rain_claim") {
      const payload = object(row.payload, "rain claim payload");
      claims.set(String(payload.id), payload);
    }
  }

  const captures: RainAdjustmentCapture[] = [];

  // join each valid receipt to its verified compressed provider body
  for (const row of packages.adjustmentRows) {
    // skip non-receipt or invalid provider outcomes
    if (row.record_kind !== "rain_receipt") {
      continue;
    }

    const payload = object(row.payload, "rain receipt payload");

    // retain only parser-validated provider responses
    if (payload.outcome !== "valid") {
      continue;
    }

    const claim = claims.get(String(payload.claimId));
    const memberPath = text(row.body_member_path, "rain body member path");
    const compressed = packages.bodies[memberPath];

    // reject broken claim/body package joins
    if (claim === undefined || compressed === undefined) {
      throw new Error("rain receipt is missing its claim or body");
    }

    const body = gunzipSync(compressed, { maxOutputLength: 2_000_000 });
    const bodySha256 = text(payload.bodySha256, "rain body hash");

    // recheck provider bytes before replay
    if (sha256(body) !== bodySha256) {
      throw new Error("rain provider body hash differs after verification");
    }

    const kind = text(claim.kind, "rain claim kind");

    // reject a capture kind outside the frozen collector
    if (kind !== "forecast" && kind !== "station") {
      throw new Error("rain claim kind is invalid");
    }

    captures.push({
      body,
      bodySha256,
      claimId: text(payload.claimId, "rain claim id"),
      completedAt: text(payload.completedAt, "rain receipt completion"),
      kind,
      runInitializedAt: claim.runInitializedAt === null
        ? null : text(claim.runInitializedAt, "rain run initialization"),
      stationId: claim.stationId === null
        ? null : finite(claim.stationId, "rain station id"),
      windowEndExclusive: claim.windowEndExclusive === null
        ? null : text(claim.windowEndExclusive, "rain window end"),
      windowStart: claim.windowStart === null
        ? null : text(claim.windowStart, "rain window start"),
    });
  }

  return captures;
}

// derive one fixed-gauge preceding-hour rain target from verified intervals
function rainTarget(
  captures: readonly RainAdjustmentCapture[],
  validAt: string,
): {
  readonly maxReceiptAt: string | null;
  readonly target: number | null;
  readonly targetIdentity: string | null;
} {
  const decisionAt = new Date(Date.parse(validAt) + 3_600_000).toISOString();
  const stationRows = rainStationHours(captures, decisionAt).filter(
    (row) => row.hourAt === validAt,
  );
  const target = createFixedRainGaugeTarget(stationRows.map((row) => ({
    precipitationMm: row.precipitationMm,
    stationId: row.stationId,
  })));
  return {
    maxReceiptAt: stationRows.map((row) => row.receivedAt).sort().at(-1) ?? null,
    target: target.precipitationMm,
    targetIdentity: target.complete
      ? sha256(canonicalJson({
        rows: stationRows.map((row) => ({
          precipitationMm: row.precipitationMm,
          receivedAt: row.receivedAt,
          stationId: row.stationId,
        })).sort((left, right) => left.stationId - right.stationId),
        validAt,
      }))
      : null,
  };
}

// preserve the closed rain diagnostics shape when no probability rows qualify
function unavailableRainDiagnostics(): JsonObject {
  return {
    accumulations: ([6, 12, 23] as const).map((hours) => ({
      adjustedMae: null,
      completeWindows: 0,
      hours,
      rawMae: null,
    })),
    annualBalancedVolumeRatio: null,
    heavyAdjustedMae: null,
    heavyRawMae: null,
    probabilityOrderViolationCount: 0,
    thresholds: ([0.1, 1, 2.5] as const).map((thresholdMmPerHour) => ({
      adjustedBrier: null,
      csi: null,
      falseAlarms: 0,
      far: null,
      hits: 0,
      misses: 0,
      pod: null,
      rawBrier: null,
      reliability: Array.from({ length: 10 }, () => ({
        count: 0,
        meanProbability: null,
        observedFrequency: null,
      })),
      thresholdMmPerHour,
    })),
    wetAdjustedMae: null,
    wetRawMae: null,
    winterBalancedVolumeRatio: null,
  };
}

// score the separately matched Best Match rain amount diagnostic
function rainBestMatchDiagnostic(
  packages: VerifiedForecastAdjustmentPerformancePackages,
  captures: readonly RainAdjustmentCapture[],
): JsonObject | null {
  const sourcePairs: ForecastAdjustmentPerformancePair[] = [];
  const bestMatchPairs: ForecastAdjustmentPerformancePair[] = [];

  // retain only rows containing all three amounts on one captured issuance
  for (const edge of packages.edgeRows) {
    const adjustment = object(edge.row.rainAdjustment, "edge rain adjustment");
    const sourceValue = adjustment.sourceForecast;

    // preserve honest absence when no model-source forecast was recorded
    if (sourceValue === null) {
      continue;
    }

    const source = object(sourceValue, "edge rain source forecast");
    const corrected = adjustment.correctedPrecipitationMm;
    const bestMatch = adjustment.rawBestMatchPrecipitationMm;

    // require the complete same-paired diagnostic subcohort
    if (corrected === null || bestMatch === null) {
      continue;
    }

    const validAt = text(source.validAt, "edge rain source validAt");
    const horizonHours = finite(source.modelLeadHours, "edge rain model lead") - 8;

    // retain only the model's positive operational horizon
    if (!Number.isInteger(horizonHours) || horizonHours < 1) {
      continue;
    }

    const target = rainTarget(captures, validAt);
    const base: Omit<
      ForecastAdjustmentPerformancePair,
      "adjustedPrediction" | "rawPrediction"
    > = {
      evidenceClass: "as_issued",
      fallback: false,
      firstEdgeCommittedAt: edge.firstEdgeCommittedAt,
      horizonHours,
      key: `${text(source.runInitializedAt, "edge rain run")}:${validAt}`,
      localDate: localCalendarFeaturesFor(validAt).localDate,
      provenanceComplete: edge.row.provenanceComplete === true,
      rowIdentity: sha256(canonicalJson({
        bundle: edge.bundleIdentities.rain,
        record: edge.row.record,
        settingsSha256: edge.settingsSha256,
      })),
      sourceReceiptAt: edge.sourceReceiptAt,
      target: target.target,
      targetKey: target.targetIdentity === null
        ? `missing:${validAt}`
        : target.targetIdentity,
      validAt,
      vintageKey: text(source.runInitializedAt, "edge rain run"),
    };
    sourcePairs.push({
      ...base,
      adjustedPrediction: finite(corrected, "edge corrected rain"),
      rawPrediction: finite(source.rawPrecipitationMm, "edge source rain"),
    });
    bestMatchPairs.push({
      ...base,
      adjustedPrediction: finite(corrected, "edge corrected rain"),
      rawPrediction: finite(bestMatch, "edge Best Match rain"),
    });
  }

  const preparedSource = prepareForecastAdjustmentPerformancePairs(sourcePairs);
  const preparedBestMatch = prepareForecastAdjustmentPerformancePairs(bestMatchPairs);

  // avoid fabricating a diagnostic from unequal or empty pairs
  if (preparedSource.rows.length === 0 ||
    preparedSource.rows.length !== preparedBestMatch.rows.length) {
    return null;
  }

  const source = scoreBalancedForecastAdjustmentPairs(preparedSource.rows).metrics;
  const bestMatchMetrics = scoreBalancedForecastAdjustmentPairs(
    preparedBestMatch.rows,
  ).metrics;
  return {
    bestMatchRawMae: bestMatchMetrics.raw.mae,
    dateCount: new Set(preparedSource.rows.map((row) => row.localDate)).size,
    rowCount: preparedSource.rows.length,
    sourceAdjustedMae: source.adjusted.mae,
    sourceRawMae: source.raw.mae,
    unit: "millimeters_per_hour",
  };
}

// replay saved deterministic rain runs through the exact TypeScript model
function createRainReport(
  packages: VerifiedForecastAdjustmentPerformancePackages,
  revision: string,
  now: Date,
): PerformanceFamilyReport {
  const captures = rainCaptures(packages);
  const amountPairs: ForecastAdjustmentPerformancePair[] = [];
  const probabilityPairs: Parameters<
    typeof evaluateForecastAdjustmentRainDiagnostics
  >[0][number][] = [];
  const issuedAmountPairs: ForecastAdjustmentPerformancePair[] = [];
  const issuedProbabilityPairs: typeof probabilityPairs[number][] = [];
  const prospectiveAmountPairs: ForecastAdjustmentPerformancePair[] = [];
  const prospectiveProbabilityPairs: typeof probabilityPairs[number][] = [];
  let servingIdentitySha256: string | null = null;

  // replay every stored rain run without refitting or provider access
  for (const row of packages.adjustmentRows) {
    // retain only immutable saved adjustment runs
    if (row.record_kind !== "rain_adjustment_run") {
      continue;
    }

    const stored = object(row.payload, "rain adjustment payload");
    const runInitializedAt = text(stored.runInitializedAt, "rain run initialization");
    const decisionAt = text(stored.decisionAt, "rain decisionAt");
    const runCaptures = captures.filter((capture) =>
      capture.kind === "station" ||
      (capture.runInitializedAt !== null &&
        [0, 6, 12].includes((Date.parse(runInitializedAt) -
          Date.parse(capture.runInitializedAt)) / 3_600_000)));
    const forecasts = runCaptures.filter((capture) => capture.kind === "forecast")
      .sort((left, right) => Date.parse(right.runInitializedAt!) -
        Date.parse(left.runInitializedAt!));
    const current = forecasts.find((capture) =>
      capture.claimId === stored.forecastClaimId &&
      capture.runInitializedAt === runInitializedAt);

    // reject a stored output without its exact source capture
    if (current === undefined) {
      throw new Error("rain adjustment run is missing its source forecast");
    }

    const replay = predictRainHurdleWindPerformance({
      currentRun: rainForecastProfile(current),
      nowUtc: decisionAt,
      priorRuns: forecasts.filter((capture) => capture !== current)
        .map(rainForecastProfile),
      stationHours: rainStationHours(runCaptures, decisionAt),
    });
    const storedHours = stored.hours;

    // require an exact stored-hour array before probability diagnostics
    if (!Array.isArray(storedHours) || storedHours.length !== replay.hours.length) {
      throw new Error("rain stored amount parity failed");
    }

    const modelSha256 = text(stored.modelSha256, "rain model hash");

    // prevent different deterministic models from sharing one report card
    if (servingIdentitySha256 !== null && servingIdentitySha256 !== modelSha256) {
      throw new Error("rain evaluation contains mixed model identities");
    }

    servingIdentitySha256 = modelSha256;

    // compare and score every exact replayed amount
    for (let index = 0; index < replay.hours.length; index += 1) {
      const replayHour = replay.hours[index]!;
      const storedHour = object(storedHours[index], "stored rain hour");
      const amountParity =
        storedHour.validAt === replayHour.validAt &&
        storedHour.modelLeadHours === replayHour.modelLeadHours &&
        Object.is(storedHour.rawPrecipitationMm, replayHour.rawPrecipitationMm) &&
        Object.is(
          storedHour.correctedPrecipitationMm,
          replayHour.correctedPrecipitationMm,
        ) &&
        storedHour.applied === replayHour.applied &&
        storedHour.reasonCode === replayHour.reasonCode;

      // stop all probability reporting on the first amount mismatch
      if (!amountParity) {
        throw new Error("rain stored amount parity failed");
      }

      const target = rainTarget(captures, replayHour.validAt);
      const key = `${runInitializedAt}:${String(replayHour.modelLeadHours)}`;
      const pair: ForecastAdjustmentPerformancePair = {
        adjustedPrediction: replayHour.correctedPrecipitationMm,
        evidenceClass: "development",
        fallback: !replayHour.applied,
        firstEdgeCommittedAt: null,
        horizonHours: replayHour.modelLeadHours - 8,
        key,
        localDate: localCalendarFeaturesFor(replayHour.validAt).localDate,
        provenanceComplete: true,
        rawPrediction: replayHour.rawPrecipitationMm,
        rowIdentity: text(row.record_revision_identity, "rain run identity") +
          `:${String(replayHour.modelLeadHours)}`,
        sourceReceiptAt: null,
        target: target.target,
        targetKey: target.targetIdentity === null
          ? `missing:${replayHour.validAt}`
          : target.targetIdentity,
        validAt: replayHour.validAt,
        vintageKey: runInitializedAt,
      };
      amountPairs.push(pair);

      // calculate Brier only from named binary heads on applied parity rows
      if (replayHour.occurrenceProbabilities !== null) {
        probabilityPairs.push({
          ...pair,
          adjustedProbability: replayHour.occurrenceProbabilities,
          amountParity,
          runKey: runInitializedAt,
          targetTilingComplete: target.target !== null,
        });
      }

      // join captured response rows to this exact native source run and hour
      for (const edge of packages.edgeRows) {
        const adjustment = object(edge.row.rainAdjustment, "edge rain adjustment");
        const sourceValue = adjustment.sourceForecast;

        // exclude public rows without the exact model-source forecast
        if (sourceValue === null) {
          continue;
        }

        const source = object(sourceValue, "edge rain source forecast");

        // retain only this replayed run, lead and valid hour
        if (source.runInitializedAt !== runInitializedAt ||
          source.validAt !== replayHour.validAt ||
          source.modelLeadHours !== replayHour.modelLeadHours) {
          continue;
        }

        // stop an exact source identity mapped to different raw bytes
        if (!Object.is(source.rawPrecipitationMm, replayHour.rawPrecipitationMm)) {
          throw new Error("rain captured source amount parity failed");
        }

        const bundles = object(edge.bundleIdentities.rain, "edge rain bundle");
        const exactServingBundle = adjustment.bundleSha256 === modelSha256 &&
          bundles.activeBundle === modelSha256;
        const capturedApplied = adjustment.state === "active";
        const capturedAmount = capturedApplied
          ? finite(adjustment.correctedPrecipitationMm, "edge corrected rain")
          : replayHour.rawPrecipitationMm;
        const capturedReason = adjustment.reasonCode === null
          ? null : text(adjustment.reasonCode, "edge rain reason");
        const capturedParity =
          Object.is(capturedAmount, replayHour.correctedPrecipitationMm) &&
          capturedApplied === replayHour.applied &&
          capturedReason === replayHour.reasonCode;

        // fail closed when the claimed active bundle differs from native replay
        if (exactServingBundle && !capturedParity) {
          throw new Error("rain captured amount applied reason parity failed");
        }

        const evidenceClass = exactServingBundle
          ? "as_issued" : "prospective_receipt";
        const freshPair: ForecastAdjustmentPerformancePair = {
          adjustedPrediction: exactServingBundle
            ? capturedAmount : replayHour.correctedPrecipitationMm,
          evidenceClass,
          fallback: exactServingBundle ? !capturedApplied : !replayHour.applied,
          firstEdgeCommittedAt: edge.firstEdgeCommittedAt,
          horizonHours: replayHour.modelLeadHours - 8,
          key: `${runInitializedAt}:${String(replayHour.modelLeadHours)}`,
          localDate: localCalendarFeaturesFor(replayHour.validAt).localDate,
          provenanceComplete: edge.row.provenanceComplete === true,
          rawPrediction: finite(
            source.rawPrecipitationMm,
            "edge source rain amount",
          ),
          rowIdentity: sha256(canonicalJson({
            bundle: edge.bundleIdentities.rain,
            modelSha256,
            record: edge.row.record,
            settingsSha256: edge.settingsSha256,
          })),
          sourceReceiptAt: text(
            source.firstReceivedAt,
            "edge rain source receipt",
          ),
          target: target.target,
          targetKey: target.targetIdentity === null
            ? `missing:${replayHour.validAt}`
            : target.targetIdentity,
          validAt: replayHour.validAt,
          vintageKey: runInitializedAt,
        };
        const freshAmounts = exactServingBundle
          ? issuedAmountPairs : prospectiveAmountPairs;
        const freshProbabilities = exactServingBundle
          ? issuedProbabilityPairs : prospectiveProbabilityPairs;
        freshAmounts.push(freshPair);

        // expose binary heads only when the exact amount replay remained valid
        if (replayHour.occurrenceProbabilities !== null) {
          freshProbabilities.push({
            ...freshPair,
            adjustedProbability: replayHour.occurrenceProbabilities,
            amountParity: exactServingBundle ? capturedParity : amountParity,
            runKey: runInitializedAt,
            targetTilingComplete: target.target !== null,
          });
        }
      }
    }
  }

  const evidenceClass = issuedAmountPairs.length > 0
    ? "as_issued"
    : prospectiveAmountPairs.length > 0
      ? "prospective_receipt" : "development";
  const selectedAmountPairs = evidenceClass === "as_issued"
    ? issuedAmountPairs
    : evidenceClass === "prospective_receipt"
      ? prospectiveAmountPairs : amountPairs;
  const selectedProbabilityPairs = evidenceClass === "as_issued"
    ? issuedProbabilityPairs
    : evidenceClass === "prospective_receipt"
      ? prospectiveProbabilityPairs : probabilityPairs;
  const prepared = prepareForecastAdjustmentPerformancePairs(selectedAmountPairs);
  const wetRows = prepared.rows.filter((pair) => (pair.target ?? 0) >= 0.1);
  const wetDates = new Set(wetRows.map((pair) => pair.localDate));
  const baseEvaluation = optionalEvaluation(prepared.rows, 1_000, 180);
  const rainSupportSufficient = baseEvaluation?.supportState === "sufficient" &&
    wetRows.length >= 100 && wetDates.size >= 20;
  const diagnosticRows = prepareForecastAdjustmentPerformancePairs(
    selectedProbabilityPairs.filter((pair) => pair.targetTilingComplete),
  ).rows;
  const diagnostics = diagnosticRows.length === 0
    ? unavailableRainDiagnostics()
    : evaluateForecastAdjustmentRainDiagnostics(selectedProbabilityPairs);
  const support = scorecardSupport(prepared, selectedAmountPairs.length);
  support.wetDateCount = wetDates.size;
  support.wetRowCount = wetRows.length;
  return {
    contractVersion: REPORT_VERSION,
    family: {
      bestMatchDiagnostic: rainBestMatchDiagnostic(packages, captures),
      comparisonState: rainSupportSufficient
        ? baseEvaluation!.comparisonState : "unscored",
      evidenceClass,
      evidenceCutoffAt: packages.inputs.targetCutoffAt,
      family: "rain",
      metrics: rainSupportSufficient
        ? scorecardMetric(baseEvaluation, "millimeters_per_hour")
        : unavailableMetric("millimeters_per_hour"),
      qualificationState: rainSupportSufficient
        ? baseEvaluation!.qualificationState
        : evidenceClass === "development" ? "development_only" : "pending_support",
      rainDiagnostics: diagnostics,
      recommendation: baseEvaluation?.comparisonState === "worse"
        ? "review_disable" : "review_candidate",
      servingIdentitySha256,
      servingState: "authorized_active",
      slices: calendarSlices(
        prepared.rows,
        1_000,
        "millimeters_per_hour",
      ),
      support,
      supportState: rainSupportSufficient ? "sufficient" : "insufficient",
    },
    generatedAt: now.toISOString(),
    inputs: packages.inputs,
    developmentReference: {
      evidenceClass: "development",
      metrics: descriptiveSliceMetric(
        prepareForecastAdjustmentPerformancePairs(amountPairs).rows,
        1,
        "millimeters_per_hour",
      ),
      rowCount: amountPairs.length,
    },
    siteKey: "ballydidean",
    sourceRevision: revision,
  };
}

// load native-temperature fit material and target joins from verified packages
export async function loadTemperaturePerformanceData(
  input: {
    readonly adjustmentPackage: string | readonly string[];
    readonly forecastPackage: string | readonly string[];
  },
  dependencies: Pick<
    ForecastAdjustmentPerformanceCliDependencies,
    "loadPackages" | "verifyForecastPackage"
  > = {},
): Promise<TemperaturePerformanceData> {
  const loadPackages = dependencies.loadPackages ?? loadVerifiedPerformancePackages;
  const packages = await loadPackages(input, dependencies);
  const runs = new Map<string, JsonObject>();

  // index immutable ECMWF run state for each exported hour
  for (const row of packages.adjustmentRows) {
    // retain only temperature run records
    if (row.record_kind === "temperature_run") {
      const payload = object(row.payload, "temperature run payload");
      runs.set(String(payload.id), payload);
    }
  }

  const rows: TemperaturePerformanceDataRow[] = [];

  // join each immutable temperature hour to targets and optional edge issuance
  for (const row of packages.adjustmentRows) {
    // skip non-temperature-hour records
    if (row.record_kind !== "temperature_hour") {
      continue;
    }

    const payload = object(row.payload, "temperature hour payload");
    const run = runs.get(String(payload.runId));

    // reject incomplete run/hour package joins
    if (run === undefined) {
      throw new Error("temperature hour is missing its immutable run");
    }

    const validAt = text(payload.validAt, "temperature validAt");
    const runInitializedAt = text(run.runInitializedAt, "temperature runInitializedAt");
    const modelLeadHours = finite(payload.modelLeadHours, "temperature modelLeadHours");
    const target = temperatureTarget(packages.forecastRows, validAt, packages.adjustmentRows);
    const issuance = packages.edgeRows.find((edge) => {
      const adjustment = object(edge.row.temperatureAdjustment, "temperature adjustment");
      const source = adjustment.sourceForecast;

      // accept only exact captured bytes for this exported ECMWF run and hour
      if (source === null) {
        return false;
      }

      const captured = object(source, "temperature source forecast");
      return captured.runInitializedAt === runInitializedAt &&
        captured.validAt === validAt &&
        captured.providerResponseSha256 === run.providerResponseSha256 &&
        captured.modelCycle === run.modelCycle &&
        captured.modelLeadHours === modelLeadHours &&
        captured.upstreamModel === run.upstreamModel &&
        Object.is(captured.rawTemperatureC, payload.rawTemperatureC) &&
        Object.is(
          captured.rawRelativeHumidityPercent,
          payload.rawRelativeHumidityPercent,
        ) &&
        Object.is(captured.rawWindSpeedMps, payload.rawWindSpeedMps);
    });
    const edgeRow = issuance?.row;
    const provenanceComplete = edgeRow?.provenanceComplete === true;
    const temperatureAdjustment = edgeRow === undefined
      ? null
      : object(edgeRow.temperatureAdjustment, "edge temperature adjustment");
    const temperatureBundles = issuance === undefined
      ? null
      : object(issuance.bundleIdentities.temperature, "edge temperature bundle");
    const rowIdentity = issuance === undefined
      ? text(row.record_revision_identity, "temperature row identity")
      : sha256(canonicalJson({
        bundle: issuance.bundleIdentities.temperature,
        recordId: text(object(edgeRow!.record, "edge record").id, "edge record id"),
        recordRevision: object(edgeRow!.record, "edge record").revisionCount,
        settingsSha256: issuance.settingsSha256,
      }));
    const evidenceClass = issuance === undefined
      ? "retrospective_counterfactual" : "prospective_receipt";
    const sourceReceiptAt = issuance === undefined
      ? text(row.first_received_at, "temperature source receipt")
      : text(
        object(
          temperatureAdjustment!.sourceForecast,
          "edge temperature source forecast",
        ).firstReceivedAt,
        "edge temperature source receipt",
      );
    const scoredPairMetadata = {
      evidenceClass,
      fallback: false,
      firstEdgeCommittedAt: issuance?.firstEdgeCommittedAt ?? null,
      horizonHours: modelLeadHours - 6,
      key: text(row.record_revision_identity, "temperature hour key"),
      localDate: text(row.local_date, "temperature local date"),
      provenanceComplete: issuance === undefined ? true : provenanceComplete,
      rowIdentity,
      sourceReceiptAt,
      targetKey: target.targetIdentity ?? `missing:${validAt}`,
      validAt,
      vintageKey: runInitializedAt,
    } as const;
    const capturedBundle = temperatureAdjustment?.bundleSha256;
    const activeBundle = temperatureBundles?.activeBundle;
    const capturedState = temperatureAdjustment?.state;
    const capturedApplied = capturedState === "active" &&
      temperatureAdjustment?.correctedTemperatureC !== null;
    const recordedRuntimeResult =
      typeof capturedBundle === "string" && capturedBundle === activeBundle &&
      capturedApplied
        ? {
          applied: true,
          predictionTemperatureC: finite(
            temperatureAdjustment!.correctedTemperatureC,
            "issued temperature prediction",
          ),
          reasonCode: temperatureAdjustment!.reasonCode === null
            ? null
            : text(
              temperatureAdjustment!.reasonCode,
              "issued temperature reason",
            ),
          scoredPairMetadata: {
            ...scoredPairMetadata,
            evidenceClass: "as_issued" as const,
            fallback: false,
          },
          servingBundleSha256: capturedBundle,
        }
        : null;
    rows.push({
      actualTemperatureC: target.target,
      bestMatchRawTemperatureC: temperatureAdjustment === null
        ? null
        : nullableFinite(
          temperatureAdjustment.rawBestMatchTemperatureC,
          "edge Best Match temperature",
        ),
      cohort: "ecmwf_single_run_hindcast",
      key: text(row.record_revision_identity, "temperature hour key"),
      modelCycle: text(run.modelCycle, "temperature modelCycle"),
      modelLeadHours,
      operationalHorizonHours: modelLeadHours - 6,
      rawRelativeHumidityPercent: nullableFinite(
        payload.rawRelativeHumidityPercent,
        "temperature raw humidity",
      ),
      rawTemperatureC: finite(payload.rawTemperatureC, "temperature raw amount"),
      rawWindSpeedMps: nullableFinite(
        payload.rawWindSpeedMps,
        "temperature raw wind",
      ),
      recordedRuntimeResult,
      recentErrorState: run.recentErrorState,
      runInitializedAt,
      scoredPairMetadata,
      scoreEligible: modelLeadHours >= 7 && modelLeadHours <= 18,
      targetIdentity: target.targetIdentity,
      targetMaxReceiptAt: target.maxReceiptAt,
      validAt,
    });
  }

  const fallbackByIdentity = new Map<string, JsonObject>();

  // preserve captured public fallbacks separately from the pure ECMWF comparator
  for (const edge of packages.edgeRows) {
    const adjustment = object(edge.row.temperatureAdjustment, "captured temperature decision");

    // active ECMWF corrections already belong to the matched primary population
    if (adjustment.state === "active") {
      continue;
    }

    const record = object(edge.row.record, "captured temperature record");
    const bundle = object(edge.bundleIdentities.temperature, "captured temperature bundle");
    const identity = sha256(canonicalJson({ record, settingsSha256: edge.settingsSha256, bundle }));
    const captured = {
      baseline: "actual_serving_best_match_fallback",
      bundleIdentities: edge.bundleIdentities,
      capturedBundleSha256: adjustment.bundleSha256,
      evidenceClass: "as_issued",
      firstEdgeCommittedAt: edge.firstEdgeCommittedAt,
      provenanceComplete: edge.row.provenanceComplete === true,
      rawBestMatchTemperatureC: adjustment.rawBestMatchTemperatureC,
      reasonCode: adjustment.reasonCode,
      record,
      rowIdentity: identity,
      servingIdentitySha256: bundle.activeBundle,
      settingsSha256: edge.settingsSha256,
      sourceReceiptAt: edge.sourceReceiptAt,
      state: adjustment.state,
      validAt: record.validAt,
    };
    const previous = fallbackByIdentity.get(identity);

    // coalesce response windows at their earliest whole committed receipt
    if (previous === undefined || edge.firstEdgeCommittedAt < String(previous.firstEdgeCommittedAt)) {
      fallbackByIdentity.set(identity, captured);
    }
  }

  return { actualServingFallbacks: [...fallbackByIdentity.values()], inputs: packages.inputs, rows };
}

// parse exact named options while allowing repeatable evaluation packages
function parseOptions(arguments_: readonly string[]): {
  readonly command: string;
  readonly values: ReadonlyMap<string, readonly string[]>;
} {
  const command = arguments_[0];

  // require one supported subcommand name
  if (command === undefined) {
    throw new Error("forecast adjustment performance command is required");
  }

  const values = new Map<string, string[]>();

  // collect option/value pairs without positional path ambiguity
  for (let index = 1; index < arguments_.length; index += 2) {
    const option = arguments_[index];
    const value = arguments_[index + 1];

    // reject flags without exactly one following value
    if (option === undefined || value === undefined || !option.startsWith("--")) {
      throw new Error("forecast adjustment performance options are invalid");
    }

    const entries = values.get(option) ?? [];
    entries.push(value);
    values.set(option, entries);
  }

  return { command, values };
}

// require one nonrepeatable CLI option
function oneOption(
  values: ReadonlyMap<string, readonly string[]>,
  option: string,
): string {
  const entries = values.get(option);

  // reject absent or repeated singleton options
  if (entries?.length !== 1) {
    throw new Error(`${option} is required exactly once`);
  }

  return entries[0]!;
}

// import the shared closed scorecard validator without duplicating its schema
async function scorecardContract(): Promise<{
  readonly parseForecastAdjustmentScorecard: (
    input: string | Buffer,
    options?: { readonly now?: string },
  ) => unknown;
  readonly validateForecastAdjustmentScorecard: (
    value: unknown,
    options?: { readonly now?: string },
  ) => unknown;
}> {
  return await import(pathToFileURL(join(
    REPOSITORY_ROOT,
    "deploy/scripts/forecast-adjustment-scorecard-contract.mjs",
  )).href) as Awaited<ReturnType<typeof scorecardContract>>;
}

// require owned private output ancestry outside every serving and settings path
export async function requirePrivatePerformanceOutput(path: string): Promise<string> {
  const output = resolve(path);
  const roots = [join(REPOSITORY_ROOT, ".weather-data"), join(homedir(), ".weather/research-work")];
  const root = roots.find((candidate) => output.startsWith(`${candidate}${sep}`));

  // deny new files near serving registries even when they would not overwrite one
  if (root === undefined) {
    throw new RangeError("performance output is outside private research storage");
  }

  // reject linked base ancestry before creating even a private directory
  if (await realpath(dirname(root)) !== dirname(root)) {
    throw new RangeError("performance output requires canonical ancestry");
  }

  await mkdir(root, { mode: 0o700 }).catch((error: unknown) => {
    // accept only an already established root for subsequent metadata checks
    if ((error as NodeJS.ErrnoException).code !== "EEXIST") {
      throw error;
    }
  });
  let parent = root;
  const components = relative(root, dirname(output)).split(sep).filter(Boolean);

  // reject links and shared ancestors before exclusive report publication
  for (let index = 0; index <= components.length; index += 1) {
    const metadata = await lstat(parent);

    // retain the private owned directory boundary at every level
    if (!metadata.isDirectory() || metadata.isSymbolicLink() ||
      metadata.uid !== process.getuid?.() || (metadata.mode & 0o077) !== 0) {
      throw new RangeError("performance output requires private owned ancestry");
    }

    // create only one child after its parent passed the private boundary
    if (index < components.length) {
      parent = join(parent, components[index]!);
      await mkdir(parent, { mode: 0o700 }).catch((error: unknown) => {
        // defer existing-node validation to the next loop iteration
        if ((error as NodeJS.ErrnoException).code !== "EEXIST") {
          throw error;
        }
      });
    }
  }

  return output;
}

// write one new private result without replacing an earlier report
async function writePrivateResult(path: string, value: unknown): Promise<void> {
  const output = await requirePrivatePerformanceOutput(path);
  await writeFile(output, canonicalJson(value), { flag: "wx", mode: 0o600 });
}

// read and validate one family report wrapper
async function readFamilyReport(path: string, expectedFamily: string): Promise<{
  readonly bytes: Buffer;
  readonly report: JsonObject;
}> {
  const bytes = await readFile(resolve(path));
  const report = object(JSON.parse(bytes.toString("utf8")), `${expectedFamily} report`);
  const family = object(report.family, `${expectedFamily} report family`);

  // reject stale, cross-family or incomplete wrappers
  if (report.contractVersion !== REPORT_VERSION || report.siteKey !== "ballydidean" ||
    family.family !== expectedFamily) {
    throw new Error(`${expectedFamily} performance report identity is invalid`);
  }

  return { bytes, report };
}

// require all family reports to bind the same verified cohort
function commonReportInputs(reports: readonly JsonObject[]): {
  readonly inputs: JsonObject;
  readonly sourceRevision: string;
} {
  const first = reports[0];

  // retain the compiler-proven three-report input
  if (first === undefined) {
    throw new Error("family reports are unavailable");
  }

  const inputs = object(first.inputs, "family report inputs");
  const sourceRevision = text(first.sourceRevision, "family source revision");
  const canonicalInputs = canonicalJson(inputs);

  // reject mixed package snapshots and stale source revisions
  for (const report of reports) {
    if (canonicalJson(object(report.inputs, "family report inputs")) !== canonicalInputs ||
      report.sourceRevision !== sourceRevision) {
      throw new Error("family reports do not bind the same input snapshot");
    }
  }

  return { inputs, sourceRevision };
}

// bind one or more disjoint publication manifest pairs to scorer identities
async function publicationInputs(
  values: ReadonlyMap<string, readonly string[]>,
): Promise<ForecastAdjustmentPerformancePackageInputs> {
  const forecastPaths = values.get("--forecast-manifest") ?? [];
  const adjustmentPaths = values.get("--adjustment-manifest") ?? [];

  // require one same-date evidence manifest for each target manifest
  if (forecastPaths.length === 0 || forecastPaths.length !== adjustmentPaths.length) {
    throw new Error("publication requires matched forecast and adjustment manifests");
  }

  const pairs: {
    readonly adjustmentHash: string;
    readonly forecastHash: string;
    readonly from: string;
    readonly targetCutoffAt: string;
    readonly to: string;
    readonly watermark: string;
  }[] = [];

  // read every exact manifest byte sequence used by the family reports
  for (let index = 0; index < forecastPaths.length; index += 1) {
    const forecastBytes = await readFile(resolve(forecastPaths[index]!));
    const adjustmentBytes = await readFile(resolve(adjustmentPaths[index]!));
    const forecast = object(
      JSON.parse(forecastBytes.toString("utf8")),
      "scorecard forecast manifest",
    );
    const adjustment = object(
      JSON.parse(adjustmentBytes.toString("utf8")),
      "scorecard adjustment manifest",
    );
    const adjustmentEdge = object(
      adjustment.edgeEvidence,
      "scorecard adjustment edge evidence",
    );
    const from = text(forecast.fromLocalDate, "scorecard from date");
    const to = text(forecast.toLocalDate, "scorecard to date");

    // reject cross-cohort manifest pairings before composition
    if (adjustment.fromLocalDate !== from || adjustment.toLocalDate !== to) {
      throw new Error("publication manifests cover different cohorts");
    }

    pairs.push({
      adjustmentHash: sha256(adjustmentBytes),
      forecastHash: sha256(forecastBytes),
      from,
      targetCutoffAt: text(normalizeVerifiedPerformanceTimestamps(forecast.createdAtUtc, "createdAtUtc"), "scorecard target cutoff"),
      to,
      watermark: text(
        adjustmentEdge.watermarkSha256,
        "scorecard adjustment watermark",
      ),
    });
  }

  pairs.sort((left, right) => left.from.localeCompare(right.from));

  // require disjoint local-date blocks for bounded composition
  for (let index = 1; index < pairs.length; index += 1) {
    if (pairs[index - 1]!.to >= pairs[index]!.from) {
      throw new Error("publication manifest date intervals overlap");
    }
  }

  const first = pairs[0]!;

  // preserve exact package identities for the ordinary single-block report
  if (pairs.length === 1) {
    return {
      adjustmentEvidenceManifestSha256: first.adjustmentHash,
      adjustmentEvidenceWatermarkSha256: first.watermark,
      forecastTrainingManifestSha256: first.forecastHash,
      localDateFrom: first.from,
      localDateTo: first.to,
      targetCutoffAt: first.targetCutoffAt,
    };
  }

  return {
    adjustmentEvidenceManifestSha256: sha256(canonicalJson(
      pairs.map((pair) => pair.adjustmentHash),
    )),
    adjustmentEvidenceWatermarkSha256: sha256(canonicalJson(
      pairs.map((pair) => pair.watermark),
    )),
    forecastTrainingManifestSha256: sha256(canonicalJson(
      pairs.map((pair) => pair.forecastHash),
    )),
    localDateFrom: first.from,
    localDateTo: pairs.at(-1)!.to,
    targetCutoffAt: pairs.map((pair) => pair.targetCutoffAt).sort().at(-1)!,
  };
}

// aggregate three already-sanitized immutable family reports
async function createScorecard(
  values: ReadonlyMap<string, readonly string[]>,
  now: Date,
): Promise<JsonObject> {
  const temperature = await readFamilyReport(
    oneOption(values, "--temperature-report"),
    "temperature",
  );
  const wind = await readFamilyReport(oneOption(values, "--wind-report"), "wind");
  const rain = await readFamilyReport(oneOption(values, "--rain-report"), "rain");
  const reports = [temperature.report, wind.report, rain.report];
  const common = commonReportInputs(reports);
  const expectedInputs = await publicationInputs(values);

  // reject stale predeploy or mismatched same-date family reports
  if (canonicalJson(common.inputs) !== canonicalJson(expectedInputs)) {
    throw new Error("family reports are stale or do not match publication manifests");
  }

  const scorecard = {
    automaticActivationEligible: false,
    contractVersion: "forecast-adjustment-scorecard/v1",
    families: {
      rain: rain.report.family,
      temperature: temperature.report.family,
      wind: wind.report.family,
    },
    generatedAt: now.toISOString(),
    inputs: {
      ...common.inputs,
      reportSha256s: {
        rain: sha256(rain.bytes),
        temperature: sha256(temperature.bytes),
        wind: sha256(wind.bytes),
      },
      sourceRevision: common.sourceRevision,
    },
    operatorApprovalRequired: true,
    servingChanged: false,
    siteKey: "ballydidean",
    validThrough: new Date(now.getTime() + 7 * 86_400_000).toISOString(),
  };
  const contract = await scorecardContract();
  contract.validateForecastAdjustmentScorecard(scorecard, { now: now.toISOString() });
  return scorecard;
}

// run one bounded file-only performance command
export async function runForecastAdjustmentPerformanceCli(
  arguments_: readonly string[] = process.argv.slice(2),
  dependencies: ForecastAdjustmentPerformanceCliDependencies = {},
): Promise<0> {
  const parsed = parseOptions(arguments_);
  const now = dependencies.now?.() ?? new Date();

  // run the retained/live-v4 wind requalification without serving mutations
  if (parsed.command === "wind-requalify") {
    const forecastPackages = parsed.values.get("--forecast-package") ?? [];
    const adjustmentPackages = parsed.values.get("--adjustment-package") ??
      parsed.values.get("--evaluation-packages") ?? [];

    // require matched verified package pairs for causal target binding
    if (forecastPackages.length === 0 ||
      forecastPackages.length !== adjustmentPackages.length) {
      throw new Error("wind requalification requires matched forecast and adjustment packages");
    }

    const loadPackages = dependencies.loadPackages ?? loadVerifiedPerformancePackages;
    const packages = await loadPackages({
      adjustmentPackage: adjustmentPackages,
      forecastPackage: forecastPackages,
    }, dependencies);
    const report = await createWindReport(
      packages,
      await sourceRevision(parsed.values),
      now,
    );
    await writePrivateResult(oneOption(parsed.values, "--output"), report);
    return 0;
  }

  // replay fixed rain evidence with amount parity before probability metrics
  if (parsed.command === "rain-evaluate") {
    const forecastPackages = parsed.values.get("--forecast-package") ?? [];
    const adjustmentPackages = parsed.values.get("--adjustment-package") ??
      parsed.values.get("--evaluation-packages") ?? [];

    // require both same-date package families for target scoring
    if (forecastPackages.length === 0 ||
      forecastPackages.length !== adjustmentPackages.length) {
      throw new Error("rain evaluation requires matched forecast and adjustment packages");
    }

    const loadPackages = dependencies.loadPackages ?? loadVerifiedPerformancePackages;
    const packages = await loadPackages({
      adjustmentPackage: adjustmentPackages,
      forecastPackage: forecastPackages,
    }, dependencies);
    const report = createRainReport(
      packages,
      await sourceRevision(parsed.values),
      now,
    );
    await writePrivateResult(oneOption(parsed.values, "--output"), report);
    return 0;
  }

  // expose the verified native temperature loader to the Python fitter bridge
  if (parsed.command === "temperature-data") {
    const data = await loadTemperaturePerformanceData({
      adjustmentPackage: parsed.values.get("--adjustment-package") ?? [],
      forecastPackage: parsed.values.get("--forecast-package") ?? [],
    }, dependencies);
    const serialized = canonicalJson(data);
    (dependencies.writeOutput ?? process.stdout.write.bind(process.stdout))(serialized);
    return 0;
  }

  // aggregate only already-sanitized family reports
  if (parsed.command === "scorecard") {
    const scorecard = await createScorecard(parsed.values, now);
    await writePrivateResult(oneOption(parsed.values, "--output"), scorecard);
    return 0;
  }

  // validate one scorecard through the exact edge/installer contract
  if (parsed.command === "verify-scorecard") {
    const path = oneOption(parsed.values, "--input");
    const bytes = await readFile(resolve(path));
    const contract = await scorecardContract();
    contract.parseForecastAdjustmentScorecard(bytes, { now: now.toISOString() });
    (dependencies.writeOutput ?? process.stdout.write.bind(process.stdout))(
      `${sha256(bytes)}\n`,
    );
    return 0;
  }

  throw new Error(`unsupported forecast adjustment performance command: ${parsed.command}`);
}

const isEntrypoint = process.argv[1] !== undefined &&
  pathToFileURL(process.argv[1]).href === import.meta.url;

// keep imports file-only and side-effect free
async function main(): Promise<void> {
  try {
    await runForecastAdjustmentPerformanceCli();
  } catch (error) {
    const message = error instanceof Error ? error.message : "unknown error";
    process.stderr.write(`forecast adjustment performance failed: ${message}\n`);
    process.exitCode = 1;
  }
}

// run only through the explicit worker command
if (isEntrypoint) {
  void main();
}
