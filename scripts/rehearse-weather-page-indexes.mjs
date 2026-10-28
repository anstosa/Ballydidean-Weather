import { execFile } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import {
  chmod,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { isDeepStrictEqual, promisify } from "node:util";
import { pathToFileURL } from "node:url";

import { runMigrations } from "../packages/database/dist/index.js";
import {
  createTestPool,
  prepareRuntimeRoles,
  startPostgres,
  stopPostgres,
} from "../packages/database/test/postgres-harness.mjs";

const executeFile = promisify(execFile);
const crossoverPostgresImage = "postgres:17.10-bookworm";

export const REHEARSAL_CONTRACT_VERSION = "weather-page-index-rehearsal/v1";
export const ADMISSION_CONTRACT_VERSION = "weather-page-index-rehearsal-admission/v1";
export const FROZEN_TAG = "2026.10.09-1";
export const FROZEN_COMMIT = "56b327d9c750946f6f6963b6fe1fa5c9bba791ca";
export const FROZEN_TREE = "ecd9599ebf4907c67b912c742eb8fc3d8e6ee8df";
export const CANDIDATE_INDEX_NAME = "weather_records_source_product_run_idx";
export const CANDIDATE_INDEX_SQL = `CREATE INDEX CONCURRENTLY ${CANDIDATE_INDEX_NAME} ON public.weather_records USING btree (source_id, product_run_at)`;
export const PARTIAL_CANDIDATE_INDEX_SQL = `${CANDIDATE_INDEX_SQL} WHERE product_run_at IS NOT NULL`;
export const FULL_CANDIDATE_CANONICAL_DEFINITION = `CREATE INDEX ${CANDIDATE_INDEX_NAME} ON public.weather_records USING btree (source_id, product_run_at)`;
export const PARTIAL_CANDIDATE_CANONICAL_DEFINITION = `${FULL_CANDIDATE_CANONICAL_DEFINITION} WHERE (product_run_at IS NOT NULL)`;
export const B1_REPORT_CONTRACT_VERSION = "weather-page-index-b1-rehearsal/v1";
export const PREDECESSOR_INDEX_NAME = "weather_records_source_valid_idx";
export const PREDECESSOR_INDEX_SQL = `CREATE INDEX CONCURRENTLY ${PREDECESSOR_INDEX_NAME} ON public.weather_records USING btree (source_id, valid_at DESC, id DESC)`;
export const PREDECESSOR_CANONICAL_DEFINITION = `CREATE INDEX ${PREDECESSOR_INDEX_NAME} ON public.weather_records USING btree (source_id, valid_at DESC, id DESC)`;
export const B1_MANIFEST_IDENTITIES = Object.freeze({
  coveringSemanticSha256: "3abe29ae6b2fad0191b6580c204654c93c50129d09fce9701925eb8aa619c970",
  fileSha256: "88f3210bee6012336a4c5370b46a69c5226cd2eaf6c2381f72c14fdbfc960cbc",
  predecessorSemanticSha256: "177252506026176f90f0381755720ed8bf1a4dc9f6530d2fffe234eca70c7436",
  protectedSemanticSha256: "5edcd5c257288c412de3df29cef2333235cc5c1531b39f7601b666541349f994",
  replacementSha256: "ac165391de09e1490a5832108fd54bba2dda97bf67a6225377c2a2e8e5e91c82",
  targetSemanticSha256: "69a6cba73e6dbb3e93a598aa7f0b37a48f5aab3dda462b6074e8b5072a2ed247",
});
export const B1_RECOVERY_IDENTITIES = Object.freeze({
  criticReviewSha256: "643ac04f12f2299c497ff87953b3bfa36dd883fcf438ac3735aa8e196d0a4488",
  helperSha256: "8b11e3f7dbc9b2c0de73f602ccf7e065e2279295bd6f323060c30ae0121ce2b5",
  originalProtocolSha256: "97acd0290f7a974439cefd005146b67f9fdcfaa09d7238c2138061ecc8460879",
  priorClassificationSha256: "9c3ff8df8c9bce0787c30b16362897949000e8b2d513a80307ad063d134722d7",
  priorJudgmentSha256: "406f12c4d55082894d7a7abca8bdd76490822e89afce67e00be5e366c4a94e2f",
  priorReportSha256: "fe6ef5ead2e24e63dcd086c728bd448d952a6d0ece454fffa085fc9382e71337",
  seedInspectionSha256: "b39f541319b192e01d7fe62d3d7ef657367c6f62eb47eb79992dbc356849e0ed",
});
const cancellationIndexName = "weather_records_source_product_run_cancel_idx";

const queryCapturePath = resolve(".weather-data/db-page-optimization/query-capture.txt");
const b1DesignPath = resolve(".weather-data/db-page-optimization/candidate-b-design.md");
const replacementManifestPath = resolve("scripts/weather-page-indexes.json");
const migrationPrefix = "packages/database/migrations/";
const indexLockKey = "8032416683782917";
const fixedAsOf = "2026-10-28T07:00:00.000Z";
const forecastRowsPerRun = 240;
const fullScaleForecastRows = 347_520;
const defaultForecastRows = 2_400;
const defaultFullHeapRows = 12_513_302;
const seedBatchRows = 50_000;
const seedBatchRuns = 100;
const writeBatchCount = 20;
const writeRowsPerBatch = 50;

export const MATCHED_CROSSOVER_PROTOCOL = Object.freeze({
  backgroundInvalidation: Object.freeze({
    invalidateOnActiveAutovacuum: true,
    invalidateOnCheckpointCounterChange: true,
    invalidateOnProgressRows: true,
    recordBgwriterCounters: true,
  }),
  candidate: "partial-product-run",
  commonPreparation: Object.freeze([
    "vacuum-analyze-weather-records",
    "checkpoint",
    "restart-both-containers",
    "exact-query-cache-preparation",
    "checkpoint",
    "fixed-five-second-quiet-period",
  ]),
  contractVersion: "weather-page-index-matched-crossover-protocol/v3",
  fixture: Object.freeze({
    cloneMethod: "offline-clean-stopped-named-volume-copy",
    distinctPostgres17Volumes: 2,
    frozenMigrationCount: 18,
    fullHeapRows: defaultFullHeapRows,
    productionData: false,
  }),
  measurement: Object.freeze({
    measuredBatchesPerStratum: 200,
    orderByRun: Object.freeze(["baseline-candidate", "candidate-baseline", "baseline-candidate", "candidate-baseline"]),
    runs: 4,
    strata: Object.freeze([
      "physical-null-50",
      "forecast-nonnull-240",
      "mixed-forecast240-tide24-24-physical50",
    ]),
    warmupBatchesPerStratum: 5,
  }),
  outOfTimingValidation: Object.freeze({
    contentHashGuard: true,
    normalization: "stored-source-kind-metrics-nulls-provenance",
    scanBoundary: "weather-record-primary-key-pre-post-insert-range",
    timedInsertUnchanged: true,
  }),
  pass: Object.freeze({
    everyRunEveryStratumP95RatioMaximum: 1.10,
    noPooledRescue: true,
    noRetries: true,
    outcomesMustMatch: true,
  }),
  preTreatmentEquality: Object.freeze([
    "rows",
    "distribution",
    "semantic-hash",
    "catalog-hash",
    "statistics-hash",
    "settings-hash",
  ]),
  supersedes: "weather-page-index-matched-crossover-protocol/v2 out-of-timing persisted-validation scan boundary only",
});

export const B1_PROTOCOL = Object.freeze({
  candidate: "partial-product-run-replaces-source-valid",
  catalog: Object.freeze({
    covering: "weather_records_current_idx",
    dependencyColumns: Object.freeze({
      covering: Object.freeze(["source_id", "valid_at", "id", "last_received_at", "source_kind"]),
      predecessor: Object.freeze(["source_id", "valid_at", "id"]),
      protected: Object.freeze({
        weather_records_identity_key: Object.freeze([]),
        weather_records_kind_valid_idx: Object.freeze(["source_kind", "valid_at", "id"]),
        weather_records_pkey: Object.freeze([]),
      }),
      target: Object.freeze(["source_id", "product_run_at", "product_run_at"]),
    }),
    predecessor: PREDECESSOR_INDEX_NAME,
    protected: Object.freeze([
      "weather_records_pkey",
      "weather_records_identity_key",
      "weather_records_kind_valid_idx",
    ]),
    target: CANDIDATE_INDEX_NAME,
  }),
  checkpointPolicy: "count-all-workload-induced-checkpoints-without-exclusion-or-retry",
  contractVersion: "weather-page-index-b1-protocol/v1",
  deployedQueries: Object.freeze([
    Object.freeze({ id: "forecast", maximumWeatherRecordScanRows: 600 }),
    Object.freeze({ id: "current", maximumWeatherRecordScanRows: 200 }),
    Object.freeze({ id: "daily-precipitation", maximumWeatherRecordScanRows: 2_000 }),
    Object.freeze({ id: "forecast-observations", maximumWeatherRecordScanRows: 6_000 }),
    Object.freeze({ id: "tides", maximumWeatherRecordScanRows: 2_000 }),
    Object.freeze({ id: "history", maximumWeatherRecordScanRows: 2_000 }),
  ]),
  fixture: Object.freeze({
    borrowedSeed: "weather-wpi-crossover-candidate-3343083-847d8e84",
    cloneMethod: "offline-clean-stopped-named-volume-copy",
    frozenMigrationCount: 18,
    fullHeapRows: defaultFullHeapRows,
    productionData: false,
  }),
  forecastScale: Object.freeze({ baseRows: fullScaleForecastRows, fourXRows: fullScaleForecastRows * 4 }),
  measurement: MATCHED_CROSSOVER_PROTOCOL.measurement,
  manifest: B1_MANIFEST_IDENTITIES,
  overlay: Object.freeze({
    forecastObservationRows: 6,
    physicalRows: 292,
    tideObservationRows: 264,
    tidePredictionRows: 264,
    totalRows: 826,
    windowEndExclusive: "2026-11-07T18:00:00.000Z",
    windowStart: "2026-10-27T17:40:00.000Z",
  }),
  pass: MATCHED_CROSSOVER_PROTOCOL.pass,
  resourceFloors: Object.freeze({ availableBytes: 5 * 1024 * 1024 * 1024, availableInodes: 100_000 }),
  rollback: "cancel-predecessor-rebuild-then-rebuild-predecessor-before-dropping-owned-target",
  semantics: Object.freeze(["maintained-1x", "recent-4x", "maintained-4x", "post-write-recent", "restored-original"]),
  sourceDesignSha256: "dc24340bdc9917826cb7d896a32d47c25b67e287a36235b5e49f4eb384b36cdf",
  sourceQueryCaptureSha256: "54ed571c624410bb3b16f733801623c270f6449b5d1a994667572920f0de159e",
});

// recognize plain records only
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

// require one positive bounded integer
function positiveInteger(value, label, maximum = 20_000_000) {
  const parsed = Number(value);

  // reject unsafe sizes
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || parsed > maximum) {
    throw new Error(`${label} must be an integer from 1 through ${String(maximum)}`);
  }

  return parsed;
}

// parse the bounded disposable rehearsal arguments
export function parseRehearsalArguments(argumentsList) {
  let b1Replacement = false;
  let candidate = "full";
  let forecastRows = defaultForecastRows;
  let forecastRowsExplicit = false;
  let matchedCrossover = false;
  let output = null;
  let protocolSha256 = null;
  let recoveryAuthorization = null;
  let recoveryAuthorizationSha256 = null;
  let replacementManifestSha256 = null;
  let retain = false;
  let seedVolume = null;
  let totalRows = null;

  // parse every explicit argument
  for (let index = 0; index < argumentsList.length; index += 1) {
    const argument = argumentsList[index];

    // select the approved final-catalog replacement protocol
    if (argument === "--b1-replacement") {
      b1Replacement = true;
      candidate = "partial-product-run";
      retain = true;
      continue;
    }

    // select the frozen matched crossover protocol
    if (argument === "--matched-crossover") {
      matchedCrossover = true;
      continue;
    }

    // select one explicitly reviewed candidate refinement
    if (argument === "--candidate") {
      candidate = String(argumentsList[index + 1] ?? "");
      index += 1;

      // reject unreviewed index shapes
      if (candidate !== "full" && candidate !== "partial-product-run") {
        throw new Error("candidate must be full or partial-product-run");
      }
      continue;
    }

    // select the reviewed current-scale forecast cardinality
    if (argument === "--forecast-rows") {
      forecastRows = positiveInteger(argumentsList[index + 1], "forecast rows", 2_000_000);
      forecastRowsExplicit = true;
      index += 1;
      continue;
    }
    // select one full synthetic heap cardinality
    if (argument === "--rows") {
      totalRows = positiveInteger(argumentsList[index + 1], "total rows");
      index += 1;
      continue;
    }
    // accept the operator shorthand from the reviewed plan
    if (argument === "--rows12513302") {
      totalRows = defaultFullHeapRows;
      continue;
    }
    // select one private report path
    if (argument === "--output") {
      output = resolve(String(argumentsList[index + 1] ?? ""));
      index += 1;
      continue;
    }
    // bind B1 to the exact reviewed replacement manifest
    if (argument === "--replacement-manifest-sha256") {
      replacementManifestSha256 = String(argumentsList[index + 1] ?? "");
      index += 1;
      continue;
    }
    // bind B1 measurements to the pre-frozen protocol artifact
    if (argument === "--protocol-sha256") {
      protocolSha256 = String(argumentsList[index + 1] ?? "");
      index += 1;
      continue;
    }
    // bind B1 to one reviewed private recovery authorization
    if (argument === "--recovery-authorization") {
      recoveryAuthorization = resolve(String(argumentsList[index + 1] ?? ""));
      index += 1;
      continue;
    }
    // bind B1 to the exact recovery authorization bytes
    if (argument === "--recovery-authorization-sha256") {
      recoveryAuthorizationSha256 = String(argumentsList[index + 1] ?? "");
      index += 1;
      continue;
    }
    // retain only this disposable fixture
    if (argument === "--retain") {
      retain = true;
      continue;
    }
    // reuse only one exact stopped disposable crossover seed
    if (argument === "--seed-volume") {
      seedVolume = String(argumentsList[index + 1] ?? "");
      index += 1;

      // reject production or ambiguous Docker volume identities
      if (!/^weather-wpi-crossover-(?:seed|candidate)-[a-zA-Z0-9.-]+$/u.test(seedVolume)) {
        throw new Error("seed volume must be an exact disposable crossover volume");
      }
      continue;
    }
    throw new Error(`unknown argument ${String(argument)}`);
  }

  // bind full-heap runs to the reviewed forecast scale by default
  if (totalRows !== null && !forecastRowsExplicit) {
    forecastRows = fullScaleForecastRows;
  }
  // require complete ten-day products
  if (forecastRows < forecastRowsPerRun * 2 || forecastRows % forecastRowsPerRun !== 0) {
    throw new Error(`forecast rows must be a multiple of ${String(forecastRowsPerRun)} and at least ${String(forecastRowsPerRun * 2)}`);
  }
  // ensure the requested base heap can contain the forecast corpus
  if (totalRows !== null && totalRows < forecastRows) {
    throw new Error("total rows must not be smaller than forecast rows");
  }

  // bind crossover runs to the full reviewed fixture
  if (matchedCrossover && (candidate !== "partial-product-run" || totalRows !== defaultFullHeapRows || forecastRows !== fullScaleForecastRows)) {
    throw new Error("matched crossover requires partial-product-run with --rows12513302 and 347520 forecast rows");
  }
  // limit stopped-volume recovery to the exact matched protocol
  if (seedVolume !== null && !matchedCrossover && !b1Replacement) {
    throw new Error("seed volume reuse requires matched crossover mode");
  }
  // bind B1 to the exact approved clean full-scale borrowed seed
  if (
    b1Replacement &&
    (
      matchedCrossover ||
      candidate !== "partial-product-run" ||
      totalRows !== defaultFullHeapRows ||
      forecastRows !== fullScaleForecastRows ||
      seedVolume !== B1_PROTOCOL.fixture.borrowedSeed ||
      replacementManifestSha256 !== B1_MANIFEST_IDENTITIES.replacementSha256 ||
      protocolSha256 !== B1_RECOVERY_IDENTITIES.originalProtocolSha256 ||
      recoveryAuthorization === null ||
      !/^[a-f0-9]{64}$/u.test(recoveryAuthorizationSha256 ?? "")
    )
  ) {
    throw new Error("B1 replacement requires the approved clean 12.5M-row borrowed seed");
  }

  return {
    b1Replacement,
    candidate,
    forecastRows,
    matchedCrossover,
    output,
    protocolSha256,
    recoveryAuthorization,
    recoveryAuthorizationSha256,
    replacementManifestSha256,
    retain,
    seedVolume,
    totalRows,
  };
}

// calculate the reviewed approximate production mix
export function calculateSyntheticDistribution(totalRows, forecastRows) {
  // omit optional padding from tiny rehearsals
  if (totalRows === null) {
    return {
      forecast: forecastRows,
      modelAndReanalysis: 0,
      physical: 0,
      tide: 0,
      total: forecastRows,
    };
  }

  const physical = Math.floor(totalRows * 0.932);
  const tide = Math.floor(totalRows * 0.0355);
  const modelAndReanalysis = totalRows - physical - tide - forecastRows;

  // reject a mix that cannot preserve the fixed forecast corpus
  if (modelAndReanalysis < 0) {
    throw new Error("total rows cannot satisfy the reviewed physical, tide, and forecast mix");
  }

  return { forecast: forecastRows, modelAndReanalysis, physical, tide, total: totalRows };
}

// find the exact deployed newest-product query
export function extractCapturedForecastQuery(content) {
  const match = /^QUERY_CAPTURE=(.+)$/mu.exec(content);

  // require one captured JSON payload
  if (match?.[1] === undefined) {
    throw new Error("QUERY_CAPTURE JSON is missing");
  }

  const captures = JSON.parse(match[1]);

  // reject malformed capture payloads
  if (!Array.isArray(captures)) {
    throw new Error("QUERY_CAPTURE must be an array");
  }

  // inspect the captured deployed queries
  for (const capture of captures) {
    // select only the source-level MAX forecast query
    if (
      isRecord(capture) &&
      typeof capture.text === "string" &&
      capture.text.includes("SELECT MAX(product.product_run_at)") &&
      capture.text.includes("candidate.source_kind = 'forecast'") &&
      Array.isArray(capture.values)
    ) {
      return { text: capture.text, values: capture.values };
    }
  }

  throw new Error("captured deployed forecast MAX query is missing");
}

// load the six exact deployed SQL statements in frozen positional order
export function extractCapturedQueries(content) {
  const match = /^QUERY_CAPTURE=(.+)$/mu.exec(content);

  // require one captured JSON payload
  if (match?.[1] === undefined) {
    throw new Error("QUERY_CAPTURE JSON is missing");
  }

  const captures = JSON.parse(match[1]);
  const identities = [
    { id: "forecast", markers: ["MAX(product.product_run_at)", "candidate.source_kind = 'forecast'"] },
    { id: "current", markers: ["pressure_before.pressure_hpa", "ORDER BY candidate.valid_at DESC"] },
    { id: "daily-precipitation", markers: ["WITH nearest_source AS", "SUM(wr.precipitation_mm)"] },
    { id: "forecast-observations", markers: ["context_run.product_run_at", "HAVING COUNT(*) = 6"] },
    { id: "tides", markers: ["wr.water_level_m", "FROM weather_records wr"] },
    { id: "history", markers: ["s.source_kind = $2", "LIMIT $5"] },
  ];

  // reject missing or substituted deployed statements
  if (!Array.isArray(captures) || captures.length !== identities.length) {
    throw new Error("QUERY_CAPTURE must contain all six deployed queries");
  }

  return captures.map(
    // bind each exact positional statement to its reviewed identity
    function bindCapturedQuery(capture, index) {
      const identity = identities[index];

      // reject malformed or signature-mismatched SQL
      if (
        !isRecord(capture) ||
        typeof capture.text !== "string" ||
        !Array.isArray(capture.values) ||
        !identity.markers.every(
          // require every reviewed query marker
          function containsMarker(marker) {
            return capture.text.includes(marker);
          },
        )
      ) {
        throw new Error(`captured deployed ${identity.id} query is missing or changed`);
      }
      return { id: identity.id, text: capture.text, values: capture.values };
    },
  );
}

// compare the approved final catalog and semantic compensation state
export function evaluateB1CatalogTransition(original, final, restored) {
  const originalByName = new Map(original.map(
    // index one original catalog tuple
    function indexOriginal(index) {
      return [index.name, index];
    },
  ));
  const finalByName = new Map(final.map(
    // index one final catalog tuple
    function indexFinal(index) {
      return [index.name, index];
    },
  ));
  const restoredByName = new Map(restored.map(
    // index one restored catalog tuple
    function indexRestored(index) {
      return [index.name, index];
    },
  ));
  const changedFinalNames = [...new Set([
    ...original.map(
      // retain every original name for the symmetric difference
      function originalName(index) {
        return index.name;
      },
    ),
    ...final.map(
      // retain every final name for the symmetric difference
      function finalName(index) {
        return index.name;
      },
    ),
  ])].filter(
    // retain only added or removed names
    function nameChanged(name) {
      return !originalByName.has(name) || !finalByName.has(name);
    },
  ).sort();
  const finalPass = changedFinalNames.length === 2 &&
    changedFinalNames.includes(CANDIDATE_INDEX_NAME) &&
    changedFinalNames.includes(PREDECESSOR_INDEX_NAME) &&
    finalByName.get(CANDIDATE_INDEX_NAME)?.definition === PARTIAL_CANDIDATE_CANONICAL_DEFINITION &&
    finalByName.get(CANDIDATE_INDEX_NAME)?.valid === true &&
    finalByName.get(CANDIDATE_INDEX_NAME)?.ready === true &&
    [...originalByName.entries()].every(
      // preserve every original tuple except the approved predecessor
      function originalPreserved([name, index]) {
        return name === PREDECESSOR_INDEX_NAME || isDeepStrictEqual(finalByName.get(name), index);
      },
    );
  const restoredPass = !restoredByName.has(CANDIDATE_INDEX_NAME) &&
    restoredByName.size === originalByName.size &&
    [...originalByName.entries()].every(
      // permit only the compensated predecessor OID to change
      function originalRestored([name, index]) {
        const restoredIndex = restoredByName.get(name);

        // compare the recreated predecessor semantically without its old OID
        if (name === PREDECESSOR_INDEX_NAME && restoredIndex !== undefined) {
          const { oid: originalOid, ...originalSemantic } = index;
          const { oid: restoredOid, ...restoredSemantic } = restoredIndex;
          return originalOid !== restoredOid && isDeepStrictEqual(restoredSemantic, originalSemantic);
        }
        return isDeepStrictEqual(restoredIndex, index);
      },
    );

  return { changedFinalNames, finalPass, restoredPass };
}

// calculate one nearest-rank p95
export function nearestRankP95(values) {
  // require a nonempty duration set
  if (!Array.isArray(values) || values.length === 0) {
    throw new Error("p95 requires at least one value");
  }

  const sorted = [...values];

  // reject invalid durations
  for (const value of sorted) {
    // preserve millisecond units
    if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
      throw new Error("p95 values must be finite nonnegative milliseconds");
    }
  }

  sorted.sort(compareNumbers);
  return sorted[Math.ceil(sorted.length * 0.95) - 1];
}

// order numeric durations
function compareNumbers(left, right) {
  return left - right;
}

// hash one deterministic JSON projection
function hashValue(value) {
  return createHash("sha256").update(JSON.stringify(value)).digest("hex");
}

// hash one immutable source artifact
async function hashSourceFile(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

// validate one frozen B1 recovery authorization document
export function validateB1RecoveryAuthorizationDocument(document, currentSources) {
  const expectedIdentity = {
    criticReviewSha256: currentSources.criticReviewSha256,
    designSha256: B1_PROTOCOL.sourceDesignSha256,
    helperSha256: currentSources.helperSha256,
    manifestFileSha256: B1_MANIFEST_IDENTITIES.fileSha256,
    priorClassificationSha256: B1_RECOVERY_IDENTITIES.priorClassificationSha256,
    priorJudgmentSha256: B1_RECOVERY_IDENTITIES.priorJudgmentSha256,
    priorProtocolSha256: B1_RECOVERY_IDENTITIES.originalProtocolSha256,
    priorReportSha256: B1_RECOVERY_IDENTITIES.priorReportSha256,
    queryCaptureSha256: B1_PROTOCOL.sourceQueryCaptureSha256,
    rehearsalSourceSha256: currentSources.rehearsalSourceSha256,
    rehearsalTestSha256: currentSources.rehearsalTestSha256,
    replacementManifestSha256: B1_MANIFEST_IDENTITIES.replacementSha256,
    seedInspectionSha256: B1_RECOVERY_IDENTITIES.seedInspectionSha256,
  };
  const authorization = document?.authorization;
  const preflight = document?.preflight;
  const resource = preflight?.containerResource;
  const image = preflight?.image;

  // reject any open or stale recovery authorization shape
  if (
    document?.contractVersion !== "weather-page-index-b1-recovery-authorization/v1" ||
    !isDeepStrictEqual(Object.keys(document ?? {}).sort(), [
      "authorization",
      "capturedAt",
      "contractVersion",
      "identities",
      "preflight",
    ]) ||
    !Number.isFinite(Date.parse(document?.capturedAt)) ||
    !isDeepStrictEqual(document?.identities, expectedIdentity) ||
    !isDeepStrictEqual(Object.keys(authorization ?? {}).sort(), [
      "candidate",
      "measurementProtocol",
      "originalMeasurementProtocolSha256",
      "retainAllTimingsAndFailures",
      "soleInstrumentationRecoveryRun",
    ]) ||
    authorization?.candidate !== B1_PROTOCOL.candidate ||
    currentSources.criticReviewSha256 !== B1_RECOVERY_IDENTITIES.criticReviewSha256 ||
    currentSources.helperSha256 !== B1_RECOVERY_IDENTITIES.helperSha256 ||
    authorization?.originalMeasurementProtocolSha256 !== B1_RECOVERY_IDENTITIES.originalProtocolSha256 ||
    authorization?.retainAllTimingsAndFailures !== true ||
    authorization?.soleInstrumentationRecoveryRun !== true ||
    !isDeepStrictEqual(authorization?.measurementProtocol, B1_PROTOCOL) ||
    preflight?.contractVersion !== "weather-page-index-b1-resource-preflight/v1" ||
    preflight?.passed !== true ||
    preflight?.serverVersionNum !== "170010" ||
    !isDeepStrictEqual(Object.keys(preflight ?? {}).sort(), [
      "capturedAt",
      "containerResource",
      "contractVersion",
      "image",
      "passed",
      "serverVersionNum",
    ]) ||
    !Number.isFinite(Date.parse(preflight?.capturedAt)) ||
    !/^sha256:[a-f0-9]{64}$/u.test(image?.id ?? "") ||
    image?.reference !== crossoverPostgresImage ||
    !Array.isArray(image?.digests) ||
    image.digests.length < 1 ||
    image.digests.some(
      // reject one nonimmutable image digest
      function invalidImageDigest(digest) {
        return !/^postgres@sha256:[a-f0-9]{64}$/u.test(digest);
      },
    ) ||
    !isRecord(resource) ||
    !isDeepStrictEqual(Object.keys(resource).sort(), [
      "availableBytes",
      "availableInodes",
      "dataBytes",
      "inodes",
      "tempBytes",
      "walBytes",
    ]) ||
    Object.values(resource).some(
      // reject one invalid real-container observation
      function invalidResource(value) {
        return !Number.isSafeInteger(value) || value < 0;
      },
    )
  ) {
    throw new Error("B1 recovery authorization is stale or incomplete");
  }
  return document;
}

// load one private hash-bound B1 recovery authorization
export async function loadB1RecoveryAuthorization(options) {
  const metadata = await lstat(options.recoveryAuthorization);

  // reject links public modes foreign owners and oversized authorization files
  if (
    metadata.isSymbolicLink() ||
    !metadata.isFile() ||
    metadata.size < 2 ||
    metadata.size > 1024 * 1024 ||
    (metadata.mode & 0o077) !== 0 ||
    (typeof process.getuid === "function" && metadata.uid !== process.getuid())
  ) {
    throw new Error("B1 recovery authorization must be private owned and link-free");
  }
  const bytes = await readFile(options.recoveryAuthorization);
  const sha256 = createHash("sha256").update(bytes).digest("hex");

  // bind the CLI declaration to the exact reviewed authorization bytes
  if (sha256 !== options.recoveryAuthorizationSha256) {
    throw new Error("B1 recovery authorization digest mismatch");
  }
  let document;

  try {
    document = JSON.parse(bytes);
  } catch (error) {
    throw new Error("B1 recovery authorization JSON is invalid", { cause: error });
  }
  const currentSources = {
    criticReviewSha256: await hashSourceFile(resolve(".weather-data/db-page-optimization/b1-verification-recovery-review.md")),
    helperSha256: await hashSourceFile(resolve("scripts/weather-page-indexes.mjs")),
    rehearsalSourceSha256: await hashSourceFile(resolve("scripts/rehearse-weather-page-indexes.mjs")),
    rehearsalTestSha256: await hashSourceFile(resolve("scripts/rehearse-weather-page-indexes.test.mjs")),
  };
  validateB1RecoveryAuthorizationDocument(document, currentSources);
  return { document, path: options.recoveryAuthorization, sha256 };
}

// delay one bounded monitor iteration
async function delay(milliseconds) {
  await new Promise(function wait(resolvePromise) {
    setTimeout(resolvePromise, milliseconds);
  });
}

// read one Git command as text
async function gitText(argumentsList) {
  const { stdout } = await executeFile("git", argumentsList, {
    cwd: resolve("."),
    maxBuffer: 4 * 1024 * 1024,
    timeout: 30_000,
  });
  return stdout;
}

// materialize exact frozen migrations into a private directory
async function materializeFrozenMigrations(root) {
  const commit = (await gitText(["rev-parse", `${FROZEN_TAG}^{commit}`])).trim();
  const tree = (await gitText(["rev-parse", `${FROZEN_TAG}^{tree}`])).trim();

  // reject moved or unexpected release identities
  if (commit !== FROZEN_COMMIT || tree !== FROZEN_TREE) {
    throw new Error("frozen migration tag identity mismatch");
  }

  const names = (await gitText(["ls-tree", "-r", "--name-only", FROZEN_TAG, "--", migrationPrefix]))
    .trim()
    .split("\n")
    .filter(selectFrozenMigration)
    .sort();

  // require the exact ordered 0018 prefix
  if (names.length !== 18 || !names[0]?.includes("0001_") || !names[17]?.includes("0018_")) {
    throw new Error("frozen tag does not contain the exact 0001 through 0018 migration set");
  }

  const migrationDirectory = join(root, "migrations");
  await mkdir(migrationDirectory, { mode: 0o700 });
  const hashes = [];

  // write each exact Git blob privately
  for (const name of names) {
    const bytes = await gitText(["show", `${FROZEN_TAG}:${name}`]);
    const basename = name.slice(migrationPrefix.length);
    await writeFile(join(migrationDirectory, basename), bytes, { mode: 0o600 });
    hashes.push({ name: basename, sha256: createHash("sha256").update(bytes).digest("hex") });
  }

  return {
    commit,
    directory: migrationDirectory,
    hashes,
    historySha256: hashValue(hashes),
    tag: FROZEN_TAG,
    tree,
  };
}

// select only migrations through 0018
function selectFrozenMigration(path) {
  const match = /\/(\d{4})_[^/]+\.sql$/u.exec(path);
  return match !== null && Number(match[1]) <= 18;
}

// create one deterministic fingerprint
function fingerprint(label) {
  return createHash("sha256").update(`synthetic:${label}`).digest("hex");
}

// insert one source and its completed ingestion run
async function insertSource(client, {
  active,
  capabilities,
  kind,
  providerId,
  sourceKey,
  stationId,
}) {
  const sourceFingerprint = fingerprint(sourceKey);
  const sourceResult = await client.query(`
    INSERT INTO sources (
      station_id,
      provider_id,
      source_key,
      source_kind,
      material_provider_config,
      source_config_fingerprint,
      capabilities,
      cadence_seconds,
      active
    ) VALUES ($1, $2, $3, $4, '{}'::jsonb, $5, $6::jsonb, 300, $7)
    RETURNING id
  `, [stationId, providerId, sourceKey, kind, sourceFingerprint, JSON.stringify(capabilities), active]);
  const id = sourceResult.rows[0].id;
  const runResult = await client.query(`
    INSERT INTO ingestion_runs (
      source_id,
      mode,
      requested_start,
      requested_end_exclusive,
      source_config_fingerprint,
      adapter_version,
      deadline_at,
      completed_at,
      state
    ) VALUES (
      $1,
      'scheduled',
      '2000-01-01T00:00:00Z',
      '2030-01-01T00:00:00Z',
      $2,
      'synthetic-rehearsal/v1',
      clock_timestamp() + interval '1 hour',
      clock_timestamp(),
      'succeeded'
    ) RETURNING id
  `, [id, sourceFingerprint]);

  return { id, kind, runId: runResult.rows[0].id, sourceKey };
}

// create exact foreign-key-safe synthetic source metadata
async function bootstrapSyntheticCatalog(client) {
  const siteResult = await client.query(`
    INSERT INTO sites (slug, display_name, latitude, longitude, timezone, active)
    VALUES ('ballydidean', 'Synthetic Ballydidean', 48.0, -122.0, 'UTC', true)
    RETURNING id
  `);
  const providerResult = await client.query(`
    INSERT INTO providers (provider_key, display_name, attribution_label, attribution_url, active)
    VALUES ('synthetic-rehearsal', 'Synthetic rehearsal', 'Synthetic only', 'https://invalid.example', true)
    RETURNING id
  `);
  const siteId = siteResult.rows[0].id;
  const providerId = providerResult.rows[0].id;

  // create one station for every independently mixed source
  async function station(slug, active, stationKind = "virtual") {
    const result = await client.query(`
      INSERT INTO stations (
        site_id,
        slug,
        display_name,
        station_kind,
        latitude,
        longitude,
        active
      ) VALUES ($1, $2, $3, $4, 48.0, -122.0, $5)
      RETURNING id
    `, [siteId, slug, `Synthetic ${slug}`, stationKind, active]);
    return result.rows[0].id;
  }

  const forecast = await insertSource(client, {
    active: true,
    capabilities: ["forecast"],
    kind: "forecast",
    providerId,
    sourceKey: "open-meteo-forecast-v4",
    stationId: await station("forecast", true),
  });
  const physical = [];

  // create twenty physical histories with only five active/current
  for (let index = 0; index < 20; index += 1) {
    const active = index < 5;
    physical.push(await insertSource(client, {
      active,
      capabilities: ["current", "historical"],
      kind: "physical_sensor",
      providerId,
      sourceKey: `physical-${String(index + 1)}`,
      stationId: await station(`physical-${String(index + 1)}`, active, "physical"),
    }));
  }

  const model = await insertSource(client, {
    active: true,
    capabilities: ["current", "historical"],
    kind: "model_current",
    providerId,
    sourceKey: "model-current",
    stationId: await station("model-current", true),
  });
  const reanalysis = await insertSource(client, {
    active: true,
    capabilities: ["historical"],
    kind: "reanalysis",
    providerId,
    sourceKey: "reanalysis",
    stationId: await station("reanalysis", true),
  });
  const tideObservation = await insertSource(client, {
    active: true,
    capabilities: ["historical"],
    kind: "tide_observation",
    providerId,
    sourceKey: "tide-observation",
    stationId: await station("tide-observation", true),
  });
  const tidePrediction = await insertSource(client, {
    active: true,
    capabilities: ["forecast"],
    kind: "tide_prediction",
    providerId,
    sourceKey: "tide-prediction",
    stationId: await station("tide-prediction", true),
  });

  return { forecast, model, physical, reanalysis, tideObservation, tidePrediction };
}

// load the exact synthetic source identities from a recovered seed
async function loadSyntheticCatalog(client) {
  const result = await client.query(`
    SELECT
      source.id,
      source.source_key,
      source.source_kind,
      (
        SELECT run.id
        FROM ingestion_runs run
        WHERE run.source_id = source.id
        ORDER BY run.id
        LIMIT 1
      ) AS run_id
    FROM sources source
    JOIN providers provider ON provider.id = source.provider_id
    WHERE provider.provider_key = 'synthetic-rehearsal'
    ORDER BY source.source_key
  `);
  const byKey = new Map(result.rows.map(
    // index one recovered synthetic source
    function indexSource(source) {
      return [source.source_key, {
        id: source.id,
        kind: source.source_kind,
        runId: source.run_id,
        sourceKey: source.source_key,
      }];
    },
  ));
  const physical = [];

  // restore every exact physical source in numeric order
  for (let index = 1; index <= 20; index += 1) {
    physical.push(byKey.get(`physical-${String(index)}`));
  }
  const catalog = {
    forecast: byKey.get("open-meteo-forecast-v4"),
    model: byKey.get("model-current"),
    physical,
    reanalysis: byKey.get("reanalysis"),
    tideObservation: byKey.get("tide-observation"),
    tidePrediction: byKey.get("tide-prediction"),
  };

  // refuse a recovered volume without every expected fixture identity
  if (Object.values(catalog).some(
    // detect missing scalar or physical source identities
    function sourceMissing(source) {
      return Array.isArray(source)
        ? source.some(
            // detect one missing physical source
            function physicalMissing(item) {
              return item === undefined;
            },
          )
        : source === undefined;
    },
  )) {
    throw new Error("recovered crossover seed catalog is incomplete");
  }

  return catalog;
}

// qualify one cloned borrowed seed before any treatment
async function qualifyRecoveredCrossoverSeed(client, distribution) {
  const result = await client.query(`
    SELECT json_build_object(
      'serverVersionNum', current_setting('server_version_num'),
      'databaseName', current_database(),
      'rows', (SELECT count(*)::integer FROM weather_records),
      'forecast', (SELECT count(*)::integer FROM weather_records WHERE source_kind = 'forecast'),
      'physical', (SELECT count(*)::integer FROM weather_records WHERE source_kind = 'physical_sensor'),
      'tide', (SELECT count(*)::integer FROM weather_records WHERE source_kind IN ('tide_observation', 'tide_prediction')),
      'modelAndReanalysis', (SELECT count(*)::integer FROM weather_records WHERE source_kind IN ('model_current', 'reanalysis')),
      'migrationCount', (SELECT count(*)::integer FROM schema_migrations),
      'lastMigration', (SELECT name FROM schema_migrations ORDER BY name DESC LIMIT 1),
      'migrationLedger', (SELECT json_agg(json_build_object('name', name, 'checksum', checksum) ORDER BY name) FROM schema_migrations),
      'providerKeys', (SELECT json_agg(provider_key ORDER BY provider_key) FROM providers),
      'siteSlugs', (SELECT json_agg(slug ORDER BY slug) FROM sites),
      'sourceCount', (SELECT count(*)::integer FROM sources),
      'weatherProviderKeys', (
        SELECT json_agg(provider_key ORDER BY provider_key)
        FROM (
          SELECT DISTINCT provider.provider_key
          FROM weather_records record
          JOIN sources source ON source.id = record.source_id
          JOIN providers provider ON provider.id = source.provider_id
        ) providers
      ),
      'candidateIndex', to_regclass('public.${CANDIDATE_INDEX_NAME}'),
      'ownerRole', (SELECT rolname FROM pg_roles WHERE rolname = 'weather_owner'),
      'apiRole', (SELECT rolname FROM pg_roles WHERE rolname = 'weather_api')
    ) AS observation
  `);
  const observation = result.rows[0].observation;
  const pass = observation.serverVersionNum === "170010" &&
    observation.databaseName === "weather_test" &&
    Number(observation.rows) === distribution.total &&
    Number(observation.forecast) === distribution.forecast &&
    Number(observation.physical) === distribution.physical &&
    Number(observation.tide) === distribution.tide &&
    Number(observation.modelAndReanalysis) === distribution.modelAndReanalysis &&
    observation.migrationCount === 18 &&
    String(observation.lastMigration).startsWith("0018_") &&
    isDeepStrictEqual(observation.providerKeys, ["synthetic-rehearsal"]) &&
    isDeepStrictEqual(observation.siteSlugs, ["ballydidean"]) &&
    observation.sourceCount === 25 &&
    isDeepStrictEqual(observation.weatherProviderKeys, ["synthetic-rehearsal"]) &&
    observation.candidateIndex === null &&
    observation.ownerRole === "weather_owner" &&
    observation.apiRole === "weather_api";

  // refuse a borrowed clone before any candidate treatment
  if (!pass) {
    throw new Error("recovered crossover seed is not the exact synthetic fixture");
  }

  return { observation, pass, sha256: hashValue(observation) };
}

// seed one bounded range of complete forecast products
async function seedForecastRuns(client, source, startRun, endRunExclusive, progress) {
  let inserted = 0;

  // insert bounded product-run batches
  for (let batchStart = startRun; batchStart < endRunExclusive; batchStart += seedBatchRuns) {
    const batchEnd = Math.min(endRunExclusive, batchStart + seedBatchRuns);
    const result = await client.query(`
      INSERT INTO weather_records (
        source_id,
        source_kind,
        valid_at,
        product_run_at,
        first_ingestion_run_id,
        last_ingestion_run_id,
        first_received_at,
        last_received_at,
        upstream_timezone,
        upstream_model,
        quality_metadata,
        provider_metadata,
        temperature_c,
        precipitation_mm,
        wind_speed_mps,
        pressure_hpa,
        relative_humidity_percent,
        cloud_cover_percent,
        content_hash
      )
      SELECT
        $1,
        'forecast',
        $2::timestamptz + hour_index * interval '1 hour',
        $2::timestamptz - run_index * interval '6 hours',
        $3::bigint,
        $3::bigint,
        $2::timestamptz - run_index * interval '6 hours' + interval '5 minutes',
        $2::timestamptz - run_index * interval '6 hours' + interval '5 minutes',
        'UTC',
        'best_match',
        jsonb_build_object('status', repeat('q', 57)),
        jsonb_build_object('request_id', repeat('f', 48)),
        10 + (hour_index % 10),
        (hour_index % 5)::double precision / 10,
        2 + (hour_index % 7)::double precision / 10,
        1000 + (hour_index % 20)::double precision / 10,
        70,
        (hour_index % 100),
        md5(format('forecast:%s:%s', run_index, hour_index)) ||
          md5(format('forecast-extra:%s:%s', run_index, hour_index))
      FROM generate_series($4::integer, $5::integer - 1) AS run_index
      CROSS JOIN generate_series(0, $6::integer - 1) AS hour_index
    `, [source.id, fixedAsOf, source.runId, batchStart, batchEnd, forecastRowsPerRun]);
    inserted += result.rowCount;
    progress("forecast", inserted);
  }

  return inserted;
}

// seed bounded nonforecast padding with realistic metadata sizes
async function seedPaddingKind(client, sources, kind, count, progress) {
  let inserted = 0;
  const sourceIds = sources.map(selectSourceId);
  const runIds = sources.map(selectRunId);

  // insert bounded historical batches
  for (let batchStart = 0; batchStart < count; batchStart += seedBatchRows) {
    const batchEnd = Math.min(count, batchStart + seedBatchRows);
    const result = await client.query(`
      INSERT INTO weather_records (
        source_id,
        source_kind,
        valid_at,
        product_run_at,
        first_ingestion_run_id,
        last_ingestion_run_id,
        first_received_at,
        last_received_at,
        upstream_timezone,
        device_vendor,
        device_model,
        device_serial,
        quality_metadata,
        provider_metadata,
        temperature_c,
        apparent_temperature_c,
        precipitation_mm,
        wind_speed_mps,
        wind_gust_mps,
        pressure_hpa,
        relative_humidity_percent,
        cloud_cover_percent,
        wind_direction_degrees,
        solar_radiation_wm2,
        uv_index,
        water_level_m,
        content_hash
      )
      SELECT
        ($1::bigint[])[1 + (row_number % cardinality($1::bigint[]))::integer],
        $3::text,
        '2000-01-01T00:00:00Z'::timestamptz +
          (row_number / cardinality($1::bigint[])) * interval '1 minute' +
          (row_number % cardinality($1::bigint[])) * interval '1 second',
        NULL,
        ($2::bigint[])[1 + (row_number % cardinality($2::bigint[]))::integer],
        ($2::bigint[])[1 + (row_number % cardinality($2::bigint[]))::integer],
        '2000-01-01T00:00:00Z'::timestamptz + row_number * interval '1 second',
        '2000-01-01T00:00:00Z'::timestamptz + row_number * interval '1 second',
        'UTC',
        CASE WHEN $3::text = 'physical_sensor' THEN repeat('v', 16) ELSE NULL END,
        CASE WHEN $3::text = 'physical_sensor' THEN repeat('m', 16) ELSE NULL END,
        CASE WHEN $3::text = 'physical_sensor' THEN repeat('s', 16) ELSE NULL END,
        jsonb_build_object('status', repeat('q', 57)),
        CASE
          WHEN $3::text IN ('tide_observation', 'tide_prediction')
            THEN jsonb_build_object('prediction_type', $3::text, 'request_id', repeat('t', 94))
          ELSE jsonb_build_object('request_id', repeat('p', 306))
        END,
        CASE WHEN $3::text IN ('tide_observation', 'tide_prediction') THEN NULL ELSE 10 END,
        CASE WHEN $3::text = 'physical_sensor' THEN 9 ELSE NULL END,
        CASE WHEN $3::text = 'physical_sensor' THEN 0.1 ELSE NULL END,
        CASE WHEN $3::text IN ('tide_observation', 'tide_prediction') THEN NULL ELSE 2 END,
        CASE WHEN $3::text = 'physical_sensor' THEN 3 ELSE NULL END,
        CASE WHEN $3::text IN ('tide_observation', 'tide_prediction') THEN NULL ELSE 1000 END,
        CASE WHEN $3::text IN ('tide_observation', 'tide_prediction') THEN NULL ELSE 70 END,
        CASE WHEN $3::text IN ('model_current', 'reanalysis') THEN 50 ELSE NULL END,
        CASE WHEN $3::text = 'physical_sensor' THEN 180 ELSE NULL END,
        CASE WHEN $3::text = 'physical_sensor' THEN 500 ELSE NULL END,
        CASE WHEN $3::text = 'physical_sensor' THEN 4 ELSE NULL END,
        CASE WHEN $3::text IN ('tide_observation', 'tide_prediction') THEN (row_number % 300)::double precision / 100 ELSE NULL END,
        md5(format('%s:%s', $3::text, row_number)) || md5(format('%s-extra:%s', $3::text, row_number))
      FROM generate_series($4::bigint, $5::bigint - 1) AS row_number
    `, [sourceIds, runIds, kind, batchStart, batchEnd]);
    inserted += result.rowCount;
    progress(kind, inserted);
  }

  return inserted;
}

// select one source id for SQL arrays
function selectSourceId(source) {
  return source.id;
}

// select one ingestion run id for SQL arrays
function selectRunId(source) {
  return source.runId;
}

// seed the optional production-shaped base heap
async function seedSyntheticPadding(client, catalog, distribution, progress) {
  // skip padding for tiny forecast-only rehearsals
  if (distribution.total === distribution.forecast) {
    return;
  }

  await seedPaddingKind(client, catalog.physical, "physical_sensor", distribution.physical, progress);
  const tideObservationRows = Math.floor(distribution.tide / 2);
  await seedPaddingKind(client, [catalog.tideObservation], "tide_observation", tideObservationRows, progress);
  await seedPaddingKind(client, [catalog.tidePrediction], "tide_prediction", distribution.tide - tideObservationRows, progress);
  const modelRows = Math.floor(distribution.modelAndReanalysis / 2);
  await seedPaddingKind(client, [catalog.model], "model_current", modelRows, progress);
  await seedPaddingKind(client, [catalog.reanalysis], "reanalysis", distribution.modelAndReanalysis - modelRows, progress);
}

// add one bounded representative overlay for all deployed query cases
async function seedB1QueryOverlay(client, catalog) {
  const physical = await client.query(`
    INSERT INTO weather_records (
      source_id, source_kind, valid_at, product_run_at,
      first_ingestion_run_id, last_ingestion_run_id,
      first_received_at, last_received_at, upstream_timezone,
      quality_metadata, provider_metadata,
      temperature_c, precipitation_mm, pressure_hpa, content_hash
    )
    SELECT
      $1, 'physical_sensor',
      '2026-10-27T17:40:00Z'::timestamptz + row_index * interval '5 minutes',
      NULL, $2, $2,
      '2026-10-27T17:40:30Z'::timestamptz + row_index * interval '5 minutes',
      '2026-10-27T17:40:30Z'::timestamptz + row_index * interval '5 minutes',
      'UTC',
      jsonb_build_object('status', 'b1-overlay'),
      jsonb_build_object('request_id', format('b1-physical-%s', row_index)),
      12 + row_index::double precision / 100,
      CASE WHEN row_index >= 160 THEN 0.1 ELSE 0 END,
      1000 + row_index::double precision / 100,
      md5(format('b1-overlay-physical:%s', row_index)) ||
        md5(format('b1-overlay-physical-extra:%s', row_index))
    FROM generate_series(0, 291) AS row_index
  `, [catalog.physical[0].id, catalog.physical[0].runId]);
  const forecast = await client.query(`
    INSERT INTO weather_records (
      source_id, source_kind, valid_at, product_run_at,
      first_ingestion_run_id, last_ingestion_run_id,
      first_received_at, last_received_at, upstream_timezone, upstream_model,
      quality_metadata, provider_metadata, temperature_c, pressure_hpa, content_hash
    )
    SELECT
      $1, 'forecast',
      '2026-10-28T04:00:00Z'::timestamptz + hour_index * interval '1 hour',
      '2026-10-28T06:00:00Z'::timestamptz,
      $2, $2,
      '2026-10-28T06:05:00Z'::timestamptz,
      '2026-10-28T06:05:00Z'::timestamptz,
      'UTC', 'best_match',
      jsonb_build_object('status', 'b1-overlay'),
      jsonb_build_object('request_id', format('b1-forecast-observation-%s', hour_index)),
      10 + hour_index,
      1000 + hour_index::double precision / 10,
      md5(format('b1-overlay-forecast:%s', hour_index)) ||
        md5(format('b1-overlay-forecast-extra:%s', hour_index))
    FROM generate_series(0, 5) AS hour_index
  `, [catalog.forecast.id, catalog.forecast.runId]);
  const tideObservation = await client.query(`
    INSERT INTO weather_records (
      source_id, source_kind, valid_at, product_run_at,
      first_ingestion_run_id, last_ingestion_run_id,
      first_received_at, last_received_at, upstream_timezone,
      quality_metadata, provider_metadata, water_level_m, content_hash
    )
    SELECT
      $1, 'tide_observation',
      '2026-10-27T18:00:00Z'::timestamptz + hour_index * interval '1 hour',
      NULL, $2, $2, clock_timestamp(), clock_timestamp(), 'UTC',
      jsonb_build_object('status', 'b1-overlay'),
      jsonb_build_object('prediction_type', 'observed', 'request_id', format('b1-tide-observed-%s', hour_index)),
      hour_index::double precision / 100,
      md5(format('b1-overlay-tide-observed:%s', hour_index)) ||
        md5(format('b1-overlay-tide-observed-extra:%s', hour_index))
    FROM generate_series(0, 263) AS hour_index
  `, [catalog.tideObservation.id, catalog.tideObservation.runId]);
  const tidePrediction = await client.query(`
    INSERT INTO weather_records (
      source_id, source_kind, valid_at, product_run_at,
      first_ingestion_run_id, last_ingestion_run_id,
      first_received_at, last_received_at, upstream_timezone,
      quality_metadata, provider_metadata, water_level_m, content_hash
    )
    SELECT
      $1, 'tide_prediction',
      '2026-10-27T18:00:00Z'::timestamptz + hour_index * interval '1 hour',
      NULL, $2, $2, clock_timestamp(), clock_timestamp(), 'UTC',
      jsonb_build_object('status', 'b1-overlay'),
      jsonb_build_object('prediction_type', 'predicted', 'request_id', format('b1-tide-predicted-%s', hour_index)),
      hour_index::double precision / 100 + 0.5,
      md5(format('b1-overlay-tide-predicted:%s', hour_index)) ||
        md5(format('b1-overlay-tide-predicted-extra:%s', hour_index))
    FROM generate_series(0, 263) AS hour_index
  `, [catalog.tidePrediction.id, catalog.tidePrediction.runId]);
  const counts = {
    forecastObservation: forecast.rowCount,
    physical: physical.rowCount,
    tideObservation: tideObservation.rowCount,
    tidePrediction: tidePrediction.rowCount,
  };
  return { counts, sha256: hashValue(counts), totalRows: Object.values(counts).reduce(
    // sum every declared overlay case
    function sumRows(total, rows) {
      return total + rows;
    },
    0,
  ) };
}

// explain the exact deployed query with JSON buffers
async function explainCapturedQuery(client, capturedQuery) {
  const result = await client.query(
    `EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON, TIMING OFF, SUMMARY ON) ${capturedQuery.text}`,
    capturedQuery.values,
  );
  return result.rows[0]["QUERY PLAN"][0];
}

// collect plan nodes recursively
function collectPlanNodes(plan, target = []) {
  // retain one valid plan node
  if (!isRecord(plan)) {
    return target;
  }

  target.push(plan);
  const children = Array.isArray(plan.Plans) ? plan.Plans : [];

  // visit every child plan
  for (const child of children) {
    collectPlanNodes(child, target);
  }

  return target;
}

// summarize the growth-sensitive plan evidence
export function summarizePlan(explain) {
  const root = explain?.Plan;
  const nodes = collectPlanNodes(root);
  const indexNames = [];
  let candidateExaminedRows = 0;
  let candidateHeapFetches = 0;
  let candidateProductSelectionRows = 0;
  let candidateResultRows = 0;
  let candidateSharedBlocks = 0;
  let weatherRecordSequentialScan = false;

  // inspect every plan node
  for (const node of nodes) {
    // retain index identity
    if (typeof node["Index Name"] === "string") {
      indexNames.push(node["Index Name"]);
    }
    // count rows touched by the candidate index
    if (node["Index Name"] === CANDIDATE_INDEX_NAME) {
      const actualRows = Number(node["Actual Rows"] ?? 0);
      const loops = Number(node["Actual Loops"] ?? 1);
      const removed = Number(node["Rows Removed by Filter"] ?? 0);
      const examined = (actualRows + removed) * loops;
      candidateExaminedRows += examined;
      candidateHeapFetches += Number(node["Heap Fetches"] ?? 0) * loops;
      candidateSharedBlocks += (
        Number(node["Shared Hit Blocks"] ?? 0) +
        Number(node["Shared Read Blocks"] ?? 0)
      ) * loops;

      const productSelection =
        String(node["Index Cond"] ?? "").includes("product_run_at IS NOT NULL") ||
        (node["Node Type"] === "Index Only Scan" && node["Scan Direction"] === "Backward");

      // distinguish the early-stop MAX probe from fixed-horizon result rows
      if (productSelection) {
        candidateProductSelectionRows += examined;
      } else {
        candidateResultRows += examined;
      }
    }
    // reject a full weather-record heap scan
    if (node["Node Type"] === "Seq Scan" && node["Relation Name"] === "weather_records") {
      weatherRecordSequentialScan = true;
    }
  }

  return {
    candidateExaminedRows,
    candidateHeapFetches,
    candidateProductSelectionRows,
    candidateResultRows,
    candidateSharedBlocks,
    executionMs: Number(explain?.["Execution Time"] ?? 0),
    indexNames: [...new Set(indexNames)].sort(),
    planningMs: Number(explain?.["Planning Time"] ?? 0),
    sharedHitBlocks: Number(root?.["Shared Hit Blocks"] ?? 0),
    sharedReadBlocks: Number(root?.["Shared Read Blocks"] ?? 0),
    weatherRecordSequentialScan,
  };
}

// execute the exact query and retain only its semantic hash
async function querySemantics(client, capturedQuery) {
  const result = await client.query(capturedQuery.text, capturedQuery.values);
  return {
    hash: hashValue(result.rows),
    rows: result.rowCount,
  };
}

// capture every existing weather-record index definition
async function catalogSnapshot(client) {
  const result = await client.query(`
    SELECT
      index_class.relname AS name,
      pg_get_indexdef(index_class.oid) AS definition,
      index_state.indisvalid AS valid,
      index_state.indisready AS ready,
      index_state.indisunique AS unique_index
    FROM pg_index index_state
    JOIN pg_class table_class ON table_class.oid = index_state.indrelid
    JOIN pg_namespace namespace ON namespace.oid = table_class.relnamespace
    JOIN pg_class index_class ON index_class.oid = index_state.indexrelid
    WHERE namespace.nspname = 'public'
      AND table_class.relname = 'weather_records'
    ORDER BY index_class.relname
  `);
  return result.rows;
}

// capture complete semantic and physical index identities
async function fullIndexCatalogSnapshot(client) {
  const result = await client.query(`
    SELECT
      index_class.oid::bigint AS oid,
      index_class.relname AS name,
      pg_get_indexdef(index_class.oid) AS definition,
      access_method.amname AS access_method,
      owner_role.rolname AS owner,
      COALESCE(tablespace.spcname, 'pg_default') AS tablespace,
      COALESCE(index_class.reloptions, ARRAY[]::text[]) AS reloptions,
      index_state.indisvalid AS valid,
      index_state.indisready AS ready,
      index_state.indislive AS live,
      index_state.indisunique AS unique_index,
      index_state.indisprimary AS primary_index,
      index_state.indisexclusion AS exclusion_index,
      index_state.indnullsnotdistinct AS nulls_not_distinct,
      index_state.indnkeyatts AS key_attributes,
      index_state.indnatts AS total_attributes,
      index_state.indkey::text AS attribute_numbers,
      index_state.indcollation::text AS collations,
      index_state.indclass::text AS opclasses,
      index_state.indoption::text AS options,
      pg_get_expr(index_state.indpred, index_state.indrelid) AS predicate,
      pg_get_expr(index_state.indexprs, index_state.indrelid) AS expressions,
      constraint_state.conname AS constraint_name,
      constraint_state.contype AS constraint_type,
      COALESCE((
        SELECT jsonb_agg(
          jsonb_build_object(
            'dependencyType', dependency.deptype,
            'referencedClass', referenced_class.relname,
            'referencedNamespace', referenced_namespace.nspname,
            'referencedSubId', dependency.refobjsubid
          )
          ORDER BY dependency.deptype, referenced_namespace.nspname, referenced_class.relname, dependency.refobjsubid
        )
        FROM pg_depend dependency
        LEFT JOIN pg_class referenced_class ON referenced_class.oid = dependency.refobjid
        LEFT JOIN pg_namespace referenced_namespace ON referenced_namespace.oid = referenced_class.relnamespace
        WHERE dependency.classid = 'pg_class'::regclass
          AND dependency.objid = index_class.oid
      ), '[]'::jsonb) AS dependencies
      , COALESCE((
        SELECT jsonb_agg(attribute.attname ORDER BY dependency.ctid)
        FROM pg_depend dependency
        JOIN pg_attribute attribute
          ON attribute.attrelid = dependency.refobjid
         AND attribute.attnum = dependency.refobjsubid
        WHERE dependency.classid = 'pg_class'::regclass
          AND dependency.objid = index_class.oid
          AND dependency.refobjid = table_class.oid
          AND dependency.refobjsubid > 0
      ), '[]'::jsonb) AS dependency_columns
    FROM pg_index index_state
    JOIN pg_class table_class ON table_class.oid = index_state.indrelid
    JOIN pg_namespace namespace ON namespace.oid = table_class.relnamespace
    JOIN pg_class index_class ON index_class.oid = index_state.indexrelid
    JOIN pg_am access_method ON access_method.oid = index_class.relam
    JOIN pg_roles owner_role ON owner_role.oid = index_class.relowner
    LEFT JOIN pg_tablespace tablespace ON tablespace.oid = index_class.reltablespace
    LEFT JOIN pg_constraint constraint_state ON constraint_state.conindid = index_class.oid
    WHERE namespace.nspname = 'public'
      AND table_class.relname = 'weather_records'
    ORDER BY index_class.relname
  `);
  return result.rows;
}

// omit cluster-local OIDs from one semantic catalog fingerprint
function semanticIndexCatalog(catalog) {
  return catalog.map(
    // retain every semantic catalog attribute
    function omitOid(index) {
      const { oid, ...semantic } = index;
      return semantic;
    },
  );
}

// capture one DDL-invariant weather-record data fingerprint
async function weatherRecordDataFingerprint(client) {
  const result = await client.query(`
    SELECT
      count(*)::bigint AS rows,
      min(id)::bigint AS minimum_id,
      max(id)::bigint AS maximum_id,
      sum(id)::numeric::text AS id_sum,
      bit_xor(hashtextextended(content_hash::text, 0))::text AS content_xor,
      sum(hashtextextended(content_hash::text, 1)::numeric)::text AS content_sum,
      pg_relation_filenode('public.weather_records'::regclass)::bigint AS relation_filenode,
      pg_relation_size('public.weather_records'::regclass)::bigint AS heap_bytes,
      (
        SELECT jsonb_object_agg(source_kind, rows ORDER BY source_kind)
        FROM (
          SELECT source_kind, count(*)::bigint AS rows
          FROM weather_records
          GROUP BY source_kind
        ) distribution
      ) AS distribution
    FROM weather_records
  `);
  const fingerprint = result.rows[0];
  return { ...fingerprint, sha256: hashValue(fingerprint) };
}

// summarize bounded weather-record work for one deployed plan
function summarizeDeployedPlan(explain) {
  const nodes = collectPlanNodes(explain?.Plan);
  const indexNames = [];
  let weatherRecordScanRows = 0;
  let weatherRecordSequentialScan = false;

  // inspect every weather-record access node
  for (const node of nodes) {
    const indexName = node["Index Name"];
    const relationName = node["Relation Name"];

    // retain every weather-record index identity
    if (typeof indexName === "string" && indexName.startsWith("weather_records_")) {
      indexNames.push(indexName);
    }
    // count rows at physical weather-record access nodes
    if (relationName === "weather_records" || (typeof indexName === "string" && indexName.startsWith("weather_records_"))) {
      weatherRecordScanRows += (
        Number(node["Actual Rows"] ?? 0) + Number(node["Rows Removed by Filter"] ?? 0)
      ) * Number(node["Actual Loops"] ?? 1);
    }
    // reject full archive scans
    if (relationName === "weather_records" && node["Node Type"] === "Seq Scan") {
      weatherRecordSequentialScan = true;
    }
  }

  return {
    indexNames: [...new Set(indexNames)].sort(),
    weatherRecordScanRows,
    weatherRecordSequentialScan,
  };
}

// execute all six exact deployed queries and their observed plans
export async function evaluateDeployedQueries(client, queries) {
  const observations = [];

  // retain exact ordered semantics and plans for every deployed query
  for (const query of queries) {
    const result = await client.query(query.text, query.values);
    const explain = await explainCapturedQuery(client, query);
    const rowsJson = JSON.stringify(result.rows);
    observations.push({
      explain,
      hash: hashValue(result.rows),
      id: query.id,
      plan: {
        ...summarizeDeployedPlan(explain),
        ...summarizePlan(explain),
      },
      responseBytes: Buffer.byteLength(rowsJson),
      rows: result.rowCount,
    });
  }

  return observations;
}

// compare complete deployed semantics and bounded final-catalog plans
export function compareDeployedQuerySuites(baseline, candidate, requireCandidatePlanBounds = true) {
  const observations = baseline.map(
    // compare one fixed positional deployed query
    function compareQuery(reference, index) {
      const measured = candidate[index];
      const bound = B1_PROTOCOL.deployedQueries[index];
      const semanticsEqual = measured?.id === reference.id &&
        measured.hash === reference.hash &&
        measured.rows === reference.rows &&
        measured.responseBytes === reference.responseBytes;
      const planPass = !requireCandidatePlanBounds || (
        measured?.rows > 0 &&
        measured.plan.weatherRecordSequentialScan === false &&
        measured.plan.weatherRecordScanRows <= bound.maximumWeatherRecordScanRows &&
        (measured.id !== "forecast" || measured.plan.candidateProductSelectionRows <= 5)
      );
      return {
        baseline: reference,
        candidate: measured,
        id: reference.id,
        maximumWeatherRecordScanRows: bound.maximumWeatherRecordScanRows,
        pass: semanticsEqual && planPass && reference.rows > 0,
        planPass,
        semanticsEqual,
      };
    },
  );
  return {
    observations,
    pass: observations.every(
      // require every exact deployed query independently
      function observationPassed(observation) {
        return observation.pass;
      },
    ),
  };
}

// omit only the package candidate from a catalog comparison
function withoutCandidate(catalog) {
  return catalog.filter(function retainOriginal(index) {
    return index.name !== CANDIDATE_INDEX_NAME;
  });
}

// capture database size and WAL counters
async function databaseResourceSnapshot(client) {
  const result = await client.query(`
    SELECT
      pg_current_wal_lsn()::text AS wal_lsn,
      pg_relation_size('public.weather_records')::bigint AS heap_bytes,
      pg_indexes_size('public.weather_records')::bigint AS all_index_bytes,
      pg_total_relation_size('public.weather_records')::bigint AS total_bytes,
      COALESCE(pg_relation_size(to_regclass('public.${CANDIDATE_INDEX_NAME}')), 0)::bigint AS candidate_index_bytes,
      statistics.temp_bytes::bigint AS temp_bytes,
      (SELECT count(*)::bigint FROM weather_records) AS rows,
      (SELECT avg(pg_column_size(sample))::double precision FROM (
        SELECT record AS sample
        FROM weather_records record
        TABLESAMPLE SYSTEM (0.1)
        LIMIT 10000
      ) sampled) AS sampled_row_bytes
    FROM pg_stat_database statistics
    WHERE statistics.datname = current_database()
  `);
  return result.rows[0];
}

// capture lightweight counters during the active index build
async function buildDatabaseResourceSnapshot(client, indexName = CANDIDATE_INDEX_NAME) {
  const result = await client.query(`
    SELECT
      pg_current_wal_lsn()::text AS wal_lsn,
      COALESCE(pg_relation_size(to_regclass($1)), 0)::bigint AS candidate_index_bytes,
      statistics.temp_bytes::bigint AS temp_bytes
    FROM pg_stat_database statistics
    WHERE statistics.datname = current_database()
  `, [`public.${indexName}`]);
  return result.rows[0];
}

// capture disposable container byte and inode state
async function containerResourceSnapshot(server) {
  const script = `
    set -eu
    data_bytes=$(du -sb "$PGDATA" | cut -f1)
    wal_bytes=$(du -sb "$PGDATA/pg_wal" | cut -f1)
    temp_bytes=$(find "$PGDATA" -xdev -type f -path '*/pgsql_tmp/*' -printf '%s\n' | awk '{ total += $1 } END { print total + 0 }')
    inodes=$(find "$PGDATA" -xdev -type f | wc -l)
    set -- $(df -B1 --output=avail,iavail "$PGDATA" | tail -1)
    available_bytes=$1
    available_inodes=$2
    printf '%s %s %s %s %s %s\n' "$data_bytes" "$wal_bytes" "$temp_bytes" "$inodes" "$available_bytes" "$available_inodes"
  `;
  const { stdout } = await executeFile("docker", ["exec", server.name, "sh", "-c", script], {
    maxBuffer: 1024 * 1024,
    timeout: 30_000,
  });
  return parseContainerResourceOutput(stdout);
}

// parse one closed six-field container resource observation
export function parseContainerResourceOutput(output) {
  const values = String(output).trim().split(/\s+/u).map(Number);

  // reject missing noninteger or unsafe resource evidence
  if (values.length !== 6 || values.some(
    // detect one invalid resource counter
    function invalidValue(value) {
      return !Number.isSafeInteger(value) || value < 0;
    },
  )) {
    throw new Error("container resource observation is incomplete");
  }
  const [dataBytes, walBytes, tempBytes, inodes, availableBytes, availableInodes] = values;
  return { availableBytes, availableInodes, dataBytes, inodes, tempBytes, walBytes };
}

// capture the generated WAL delta
async function walDelta(client, beforeLsn) {
  const result = await client.query(
    "SELECT pg_wal_lsn_diff(pg_current_wal_lsn(), $1::pg_lsn)::bigint AS bytes",
    [beforeLsn],
  );
  return Number(result.rows[0].bytes);
}

// capture write-path WAL and relation bytes without scanning the heap
async function writeResourceSnapshot(client) {
  const result = await client.query(`
    SELECT
      pg_current_wal_lsn()::text AS wal_lsn,
      pg_total_relation_size('public.weather_records')::bigint AS total_bytes
  `);
  return result.rows[0];
}

// reproduce one synthetic stored content hash
function syntheticContentHash(primary, secondary) {
  return createHash("md5").update(primary).digest("hex") +
    createHash("md5").update(secondary).digest("hex");
}

// hash only persisted row counts from one replay
function countOutcomeHash(outcomes) {
  return hashValue(outcomes.map(
    // omit content digests from the raw count guard
    function retainCounts(outcome) {
      return Object.fromEntries(Object.entries(outcome).filter(
        // retain integer count fields only
        function isCountEntry(entry) {
          return Number.isInteger(entry[1]);
        },
      ));
    },
  ));
}

// hash normalized stored values outside the timed insert boundary
async function persistedOutcomeDigest(client, contentHashes, idBounds) {
  const result = await client.query(`
    SELECT
      count(*)::integer AS rows,
      md5(string_agg(normalized::text, '' ORDER BY normalized::text)) AS normalized_md5
    FROM (
      SELECT
        content_hash,
        jsonb_build_object(
          'sourceKind', source_kind,
          'productRunPresent', product_run_at IS NOT NULL,
          'upstreamTimezone', upstream_timezone,
          'upstreamModel', upstream_model,
          'deviceVendor', device_vendor,
          'deviceModel', device_model,
          'deviceSerial', device_serial,
          'qualityMetadata', quality_metadata,
          'providerMetadata', provider_metadata,
          'temperatureC', temperature_c,
          'apparentTemperatureC', apparent_temperature_c,
          'precipitationMm', precipitation_mm,
          'windSpeedMps', wind_speed_mps,
          'windGustMps', wind_gust_mps,
          'pressureHpa', pressure_hpa,
          'relativeHumidityPercent', relative_humidity_percent,
          'cloudCoverPercent', cloud_cover_percent,
          'windDirectionDegrees', wind_direction_degrees,
          'blackGlobeTemperatureC', black_globe_temperature_c,
          'pm25', pm25_micrograms_per_cubic_meter,
          'precipitationRate', precipitation_rate_mm_per_hour,
          'soilElectricalConductivity', soil_electrical_conductivity_us_cm,
          'soilMoisturePercent', soil_moisture_percent,
          'solarRadiation', solar_radiation_wm2,
          'uvIndex', uv_index,
          'wetBulbGlobeTemperatureC', wet_bulb_globe_temperature_c,
          'waterLevelM', water_level_m
        ) AS normalized
      FROM weather_records
      WHERE id > $2::bigint
        AND id <= $3::bigint
        AND content_hash = ANY($1::bpchar[])
    ) persisted
  `, [contentHashes, idBounds.firstExclusive, idBounds.lastInclusive]);
  const outcome = {
    normalizedMd5: result.rows[0].normalized_md5,
    rows: result.rows[0].rows,
  };
  return { ...outcome, idBounds, sha256: hashValue(outcome) };
}

// capture one indexed weather-record primary-key high-water mark
async function weatherRecordIdHighWater(client) {
  const result = await client.query("SELECT COALESCE(max(id), 0)::bigint AS id FROM weather_records");
  return result.rows[0].id;
}

// merge one resource sample into high-water evidence
function retainHighWater(highWater, sample) {
  highWater.candidateIndexBytes = Math.max(highWater.candidateIndexBytes, Number(sample.database.candidate_index_bytes));
  highWater.dataBytes = Math.max(highWater.dataBytes, sample.container.dataBytes);
  highWater.inodes = Math.max(highWater.inodes, sample.container.inodes);
  highWater.tempBytes = Math.max(highWater.tempBytes, sample.container.tempBytes);
  highWater.walBytes = Math.max(highWater.walBytes, sample.container.walBytes);
}

// calculate one nonnegative resource increment
function resourceIncrement(highWater, before) {
  return Math.max(0, Number(highWater) - Number(before));
}

// normalize legacy raw high-water totals into admission increments
export function normalizeAdmissionResourcePeaks(report) {
  const normalized = structuredClone(report);
  const validBuild = normalized.admission?.validBuild;
  const validRaw = normalized.resources?.build?.highWater;
  const validBefore = normalized.resources?.build?.before?.container;
  const failedBuild = normalized.admission?.failedBuild;
  const failedRaw = normalized.cancellation?.highWater;
  const failedBefore = normalized.cancellation?.before?.container;

  // normalize one complete valid-build resource record
  if (isRecord(validBuild) && isRecord(validRaw) && isRecord(validBefore)) {
    validBuild.indexBytesHighWater = Number(validRaw.candidateIndexBytes);
    validBuild.inodesHighWater = resourceIncrement(validRaw.inodes, validBefore.inodes);
    validBuild.tempBytesHighWater = resourceIncrement(validRaw.tempBytes, validBefore.tempBytes);
    validBuild.walRetainedBytesHighWater = resourceIncrement(validRaw.walBytes, validBefore.walBytes);
    validBuild.resourceDiagnostics = {
      dataBytesHighWater: resourceIncrement(validRaw.dataBytes, validBefore.dataBytes),
      rawHighWater: validRaw,
    };
    delete validBuild.dataBytesHighWater;
    delete validBuild.rawHighWater;
  }

  // normalize one complete failed-build resource record
  if (
    isRecord(failedBuild) &&
    isRecord(failedBuild.resourcePeaks) &&
    isRecord(failedRaw) &&
    isRecord(failedBefore)
  ) {
    failedBuild.resourcePeaks = {
      indexBytesHighWater: Number(failedRaw.candidateIndexBytes),
      inodesHighWater: resourceIncrement(failedRaw.inodes, failedBefore.inodes),
      tempBytesHighWater: resourceIncrement(failedRaw.tempBytes, failedBefore.tempBytes),
      walGeneratedBytes: Number(normalized.cancellation.generatedWalBytes),
      walRetainedBytesHighWater: resourceIncrement(failedRaw.walBytes, failedBefore.walBytes),
    };
    failedBuild.resourceDiagnostics = {
      dataBytesHighWater: resourceIncrement(failedRaw.dataBytes, failedBefore.dataBytes),
      rawHighWater: failedRaw,
    };
  }

  // bind report capture time to the valid build receipt
  if (isRecord(validBuild) && typeof validBuild.finishedAt === "string") {
    normalized.capturedAt = validBuild.finishedAt;
  }

  return normalized;
}

// sample build resources until the build ends
async function monitorIndexBuild(server, client, state, evidence, indexName = CANDIDATE_INDEX_NAME) {
  // poll only while the owned build is active
  while (state.active) {
    const database = await buildDatabaseResourceSnapshot(client, indexName);
    const container = await containerResourceSnapshot(server);
    const progressResult = await client.query(`
      SELECT phase, blocks_total, blocks_done, tuples_total, tuples_done
      FROM pg_stat_progress_create_index
      WHERE index_relid = to_regclass($1)
    `, [`public.${indexName}`]);
    const sample = {
      at: new Date().toISOString(),
      container,
      database,
      progress: progressResult.rows[0] ?? null,
    };
    evidence.samples += 1;
    retainHighWater(evidence.highWater, sample);

    // retain bounded progress samples
    if (evidence.progress.length < 100 && sample.progress !== null) {
      evidence.progress.push(sample.progress);
    }
    await delay(250);
  }
}

// prove concurrent DDL fails inside a transaction
async function rehearseTransactionFailure(client) {
  let code = null;
  let message = null;

  await client.query("BEGIN");
  try {
    await client.query("CREATE INDEX CONCURRENTLY weather_records_rehearsal_forbidden_idx ON weather_records (source_id)");
  } catch (error) {
    code = error.code ?? null;
    message = error.message ?? String(error);
  } finally {
    await client.query("ROLLBACK");
  }

  const object = await client.query("SELECT to_regclass('public.weather_records_rehearsal_forbidden_idx')::text AS name");
  return {
    errorCode: code,
    errorMessage: message,
    objectAbsent: object.rows[0].name === null,
    pass: code === "25001" && object.rows[0].name === null,
  };
}

// select the exact reviewed candidate ddl
export function candidateSqlFor(candidate) {
  // select the physical-write-excluding refinement explicitly
  if (candidate === "partial-product-run") {
    return PARTIAL_CANDIDATE_INDEX_SQL;
  }
  // retain the original full candidate by default
  if (candidate === "full") {
    return CANDIDATE_INDEX_SQL;
  }
  throw new Error(`unknown candidate ${String(candidate)}`);
}

// select the exact PostgreSQL 17 catalog definition
export function candidateCanonicalDefinitionFor(candidate) {
  // select the reviewed partial predicate spelling
  if (candidate === "partial-product-run") {
    return PARTIAL_CANDIDATE_CANONICAL_DEFINITION;
  }
  // select the original full-index spelling
  if (candidate === "full") {
    return FULL_CANDIDATE_CANONICAL_DEFINITION;
  }
  throw new Error(`unknown candidate ${String(candidate)}`);
}

// build the candidate under the held migration advisory lock
async function buildCandidateIndex(server, ownerClient, monitorClient, candidateSql) {
  await ownerClient.query("SET maintenance_work_mem = '64MB'");
  await ownerClient.query("SET max_parallel_maintenance_workers = 0");
  await ownerClient.query("SET lock_timeout = '5s'");
  await ownerClient.query("SET statement_timeout = '45min'");
  await ownerClient.query("SELECT pg_advisory_lock($1::bigint)", [indexLockKey]);
  const beforeDatabase = await databaseResourceSnapshot(monitorClient);
  const beforeContainer = await containerResourceSnapshot(server);
  const evidence = {
    highWater: {
      candidateIndexBytes: Number(beforeDatabase.candidate_index_bytes),
      dataBytes: beforeContainer.dataBytes,
      inodes: beforeContainer.inodes,
      tempBytes: beforeContainer.tempBytes,
      walBytes: beforeContainer.walBytes,
    },
    progress: [],
    samples: 0,
  };
  const state = { active: true };
  const monitorPromise = monitorIndexBuild(server, monitorClient, state, evidence);
  const startedAt = new Date().toISOString();
  const started = performance.now();

  try {
    await ownerClient.query(candidateSql);
  } finally {
    state.active = false;

    // release the lock even when monitoring fails
    try {
      await monitorPromise;
    } finally {
      await ownerClient.query("SELECT pg_advisory_unlock($1::bigint)", [indexLockKey]);
    }
  }

  const elapsedMs = performance.now() - started;
  const finishedAt = new Date().toISOString();
  const afterDatabase = await databaseResourceSnapshot(monitorClient);
  const afterContainer = await containerResourceSnapshot(server);
  retainHighWater(evidence.highWater, { container: afterContainer, database: afterDatabase });

  return {
    after: { container: afterContainer, database: afterDatabase },
    before: { container: beforeContainer, database: beforeDatabase },
    elapsedMs,
    finishedAt,
    generatedWalBytes: await walDelta(monitorClient, beforeDatabase.wal_lsn),
    highWater: evidence.highWater,
    progress: evidence.progress,
    samples: evidence.samples,
    startedAt,
    tempBytesDelta: Number(afterDatabase.temp_bytes) - Number(beforeDatabase.temp_bytes),
  };
}

// build one exact B1 index while its caller retains the advisory lock
async function buildObservedIndex(server, ownerClient, monitorClient, indexName, sql) {
  await ownerClient.query("SET maintenance_work_mem = '64MB'");
  await ownerClient.query("SET max_parallel_maintenance_workers = 0");
  await ownerClient.query("SET lock_timeout = '5s'");
  await ownerClient.query("SET statement_timeout = '45min'");
  const beforeDatabase = await buildDatabaseResourceSnapshot(monitorClient, indexName);
  const beforeContainer = await containerResourceSnapshot(server);
  const evidence = {
    highWater: {
      candidateIndexBytes: Number(beforeDatabase.candidate_index_bytes),
      dataBytes: beforeContainer.dataBytes,
      inodes: beforeContainer.inodes,
      tempBytes: beforeContainer.tempBytes,
      walBytes: beforeContainer.walBytes,
    },
    progress: [],
    samples: 0,
  };
  const state = { active: true };
  const monitorPromise = monitorIndexBuild(server, monitorClient, state, evidence, indexName);
  const startedAt = new Date().toISOString();
  const started = performance.now();

  try {
    await ownerClient.query(sql);
  } finally {
    state.active = false;
    await monitorPromise;
  }

  const elapsedMs = performance.now() - started;
  const finishedAt = new Date().toISOString();
  const afterDatabase = await buildDatabaseResourceSnapshot(monitorClient, indexName);
  const afterContainer = await containerResourceSnapshot(server);
  retainHighWater(evidence.highWater, { container: afterContainer, database: afterDatabase });
  return {
    after: { container: afterContainer, database: afterDatabase },
    before: { container: beforeContainer, database: beforeDatabase },
    elapsedMs,
    finishedAt,
    generatedWalBytes: await walDelta(monitorClient, beforeDatabase.wal_lsn),
    highWater: evidence.highWater,
    indexName,
    progress: evidence.progress,
    samples: evidence.samples,
    startedAt,
    tempBytesDelta: Number(afterDatabase.temp_bytes) - Number(beforeDatabase.temp_bytes),
  };
}

// cancel and clean one exact temporary B1 build under the retained lock
async function rehearseObservedCancellation(server, ownerClient, monitorClient, indexName, sql) {
  await ownerClient.query("SET maintenance_work_mem = '64MB'");
  await ownerClient.query("SET max_parallel_maintenance_workers = 0");
  await ownerClient.query("SET lock_timeout = '5s'");
  await ownerClient.query("SET statement_timeout = '45min'");
  const beforeDatabase = await buildDatabaseResourceSnapshot(monitorClient, indexName);
  const beforeContainer = await containerResourceSnapshot(server);
  const evidence = {
    highWater: {
      candidateIndexBytes: 0,
      dataBytes: beforeContainer.dataBytes,
      inodes: beforeContainer.inodes,
      tempBytes: beforeContainer.tempBytes,
      walBytes: beforeContainer.walBytes,
    },
    progress: [],
    samples: 0,
  };
  const state = { active: true };
  const monitorPromise = monitorIndexBuild(server, monitorClient, state, evidence, indexName);
  const backend = await ownerClient.query("SELECT pg_backend_pid() AS pid, backend_start FROM pg_stat_activity WHERE pid = pg_backend_pid()");
  const backendIdentity = backend.rows[0];
  const startedAt = new Date().toISOString();

  // normalize unexpected successful completion
  function buildSucceeded() {
    return { errorCode: null, errorMessage: null, unexpectedlyCompleted: true };
  }

  // retain one exact cancellation failure
  function buildFailed(error) {
    return {
      errorCode: error.code ?? null,
      errorMessage: error.message ?? String(error),
      unexpectedlyCompleted: false,
    };
  }

  const buildPromise = ownerClient.query(sql).then(buildSucceeded, buildFailed);
  await delay(100);
  const cancelResult = await monitorClient.query("SELECT pg_cancel_backend($1::integer) AS cancelled", [backendIdentity.pid]);
  const outcome = await buildPromise;
  state.active = false;
  await monitorPromise;
  const retained = await monitorClient.query("SELECT to_regclass($1)::text AS name", [`public.${indexName}`]);
  const retainedName = retained.rows[0].name;

  // capture a retained invalid object before exact cleanup
  if (retainedName !== null) {
    const retainedDatabase = await buildDatabaseResourceSnapshot(monitorClient, indexName);
    const retainedContainer = await containerResourceSnapshot(server);
    retainHighWater(evidence.highWater, { container: retainedContainer, database: retainedDatabase });
    await ownerClient.query(`DROP INDEX CONCURRENTLY public.${indexName}`);
  }

  const afterDatabase = await buildDatabaseResourceSnapshot(monitorClient, indexName);
  const afterContainer = await containerResourceSnapshot(server);
  const remaining = await monitorClient.query("SELECT to_regclass($1)::text AS name", [`public.${indexName}`]);
  retainHighWater(evidence.highWater, { container: afterContainer, database: afterDatabase });
  return {
    after: { container: afterContainer, database: afterDatabase },
    backendIdentity,
    before: { container: beforeContainer, database: beforeDatabase },
    cancelRequested: cancelResult.rows[0].cancelled === true,
    errorCode: outcome.errorCode,
    errorMessage: outcome.errorMessage,
    finishedAt: new Date().toISOString(),
    generatedWalBytes: await walDelta(monitorClient, beforeDatabase.wal_lsn),
    highWater: evidence.highWater,
    indexName,
    objectAbsentAfterCleanup: remaining.rows[0].name === null,
    pass: cancelResult.rows[0].cancelled === true &&
      outcome.errorCode === "57014" &&
      outcome.unexpectedlyCompleted === false &&
      remaining.rows[0].name === null,
    progress: evidence.progress,
    retainedObjectBeforeCleanup: retainedName,
    samples: evidence.samples,
    startedAt,
    tempBytesDelta: Number(afterDatabase.temp_bytes) - Number(beforeDatabase.temp_bytes),
    unexpectedlyCompleted: outcome.unexpectedlyCompleted,
  };
}

// measure one exact concurrent index drop
async function observeConcurrentDrop(server, ownerClient, monitorClient, indexName) {
  const beforeDatabase = await databaseResourceSnapshot(monitorClient);
  const beforeContainer = await containerResourceSnapshot(server);
  const startedAt = new Date().toISOString();
  const started = performance.now();
  await ownerClient.query(`DROP INDEX CONCURRENTLY public.${indexName}`);
  const elapsedMs = performance.now() - started;
  const afterDatabase = await databaseResourceSnapshot(monitorClient);
  const afterContainer = await containerResourceSnapshot(server);
  return {
    after: { container: afterContainer, database: afterDatabase },
    before: { container: beforeContainer, database: beforeDatabase },
    elapsedMs,
    finishedAt: new Date().toISOString(),
    generatedWalBytes: await walDelta(monitorClient, beforeDatabase.wal_lsn),
    indexName,
    startedAt,
  };
}

// apply the create-first approved B1 final catalog treatment
async function applyB1Treatment(server, connections, originalCatalog, beforeData) {
  const targetCancellationName = "weather_records_source_product_run_b1_cancel_idx";
  const targetCancellationSql = `CREATE INDEX CONCURRENTLY ${targetCancellationName} ON public.weather_records USING btree (source_id, product_run_at) WHERE product_run_at IS NOT NULL`;
  const baseResources = await databaseResourceSnapshot(connections.monitorClient);
  await connections.ownerClient.query("SELECT pg_advisory_lock($1::bigint)", [indexLockKey]);
  let targetCancelled;
  let targetValid;
  let predecessorDrop;

  try {
    targetCancelled = await rehearseObservedCancellation(
      server,
      connections.ownerClient,
      connections.monitorClient,
      targetCancellationName,
      targetCancellationSql,
    );
    targetValid = await buildObservedIndex(
      server,
      connections.ownerClient,
      connections.monitorClient,
      CANDIDATE_INDEX_NAME,
      PARTIAL_CANDIDATE_INDEX_SQL,
    );
    const afterTarget = await fullIndexCatalogSnapshot(connections.ownerClient);
    const target = afterTarget.find(findCandidateIndex);
    const predecessor = afterTarget.find(
      // locate the exact predecessor before its approved drop
      function findPredecessor(index) {
        return index.name === PREDECESSOR_INDEX_NAME;
      },
    );

    // refuse source removal before exact target and predecessor verification
    if (
      target?.definition !== PARTIAL_CANDIDATE_CANONICAL_DEFINITION ||
      target.valid !== true ||
      target.ready !== true ||
      target.live !== true ||
      predecessor?.definition !== PREDECESSOR_CANONICAL_DEFINITION ||
      predecessor.valid !== true ||
      predecessor.ready !== true ||
      predecessor.live !== true
    ) {
      throw new Error("B1 target or predecessor verification failed before source drop");
    }
    predecessorDrop = await observeConcurrentDrop(
      server,
      connections.ownerClient,
      connections.monitorClient,
      PREDECESSOR_INDEX_NAME,
    );
  } finally {
    await connections.ownerClient.query("SELECT pg_advisory_unlock($1::bigint)", [indexLockKey]);
  }

  const finalCatalog = await fullIndexCatalogSnapshot(connections.ownerClient);
  const afterData = await weatherRecordDataFingerprint(connections.ownerClient);
  const changedNames = [...new Set([
    ...originalCatalog.map(
      // retain original catalog names
      function originalName(index) {
        return index.name;
      },
    ),
    ...finalCatalog.map(
      // retain final catalog names
      function finalName(index) {
        return index.name;
      },
    ),
  ])].filter(
    // retain only the approved symmetric difference
    function changedName(name) {
      const inOriginal = originalCatalog.some(
        // find one original name
        function originalHas(index) {
          return index.name === name;
        },
      );
      const inFinal = finalCatalog.some(
        // find one final name
        function finalHas(index) {
          return index.name === name;
        },
      );
      return inOriginal !== inFinal;
    },
  ).sort();
  const expectedChanges = [CANDIDATE_INDEX_NAME, PREDECESSOR_INDEX_NAME].sort();
  const protectedPass = originalCatalog.every(
    // preserve every catalog tuple other than the approved predecessor
    function originalPreserved(index) {
      return index.name === PREDECESSOR_INDEX_NAME || finalCatalog.some(
        // match one exact protected tuple
        function matchesFinal(candidate) {
          return isDeepStrictEqual(candidate, index);
        },
      );
    },
  );
  const pass = targetCancelled.pass &&
    isDeepStrictEqual(changedNames, expectedChanges) &&
    protectedPass &&
    beforeData.sha256 === afterData.sha256;
  return {
    afterData,
    baseResources,
    beforeData,
    changedNames,
    finalCatalog,
    finalCatalogSemanticSha256: hashValue(semanticIndexCatalog(finalCatalog)),
    finalCatalogSha256: hashValue(finalCatalog),
    pass,
    predecessorDrop,
    protectedPass,
    targetCancelled,
    targetValid,
  };
}

// compensate a final-copy clone source-first and prove original semantics
async function rehearseB1Rollback(server, connections, originalCatalog, finalCatalog, expectedData, expectedQueries, queries) {
  const predecessorCancellationName = "weather_records_source_valid_b1_cancel_idx";
  const predecessorCancellationSql = `CREATE INDEX CONCURRENTLY ${predecessorCancellationName} ON public.weather_records USING btree (source_id, valid_at DESC, id DESC)`;
  await connections.ownerClient.query("SELECT pg_advisory_lock($1::bigint)", [indexLockKey]);
  let predecessorCancelled;
  let predecessorValid;
  let targetDrop;

  try {
    predecessorCancelled = await rehearseObservedCancellation(
      server,
      connections.ownerClient,
      connections.monitorClient,
      predecessorCancellationName,
      predecessorCancellationSql,
    );
    predecessorValid = await buildObservedIndex(
      server,
      connections.ownerClient,
      connections.monitorClient,
      PREDECESSOR_INDEX_NAME,
      PREDECESSOR_INDEX_SQL,
    );
    const compensatedCatalog = await fullIndexCatalogSnapshot(connections.ownerClient);
    const predecessor = compensatedCatalog.find(
      // locate the recreated predecessor
      function findPredecessor(index) {
        return index.name === PREDECESSOR_INDEX_NAME;
      },
    );

    // preserve the target until the predecessor is exact and valid
    if (
      predecessor?.definition !== PREDECESSOR_CANONICAL_DEFINITION ||
      predecessor.valid !== true ||
      predecessor.ready !== true ||
      predecessor.live !== true
    ) {
      throw new Error("B1 predecessor compensation is incomplete");
    }
    targetDrop = await observeConcurrentDrop(
      server,
      connections.ownerClient,
      connections.monitorClient,
      CANDIDATE_INDEX_NAME,
    );
  } finally {
    await connections.ownerClient.query("SELECT pg_advisory_unlock($1::bigint)", [indexLockKey]);
  }

  const restoredCatalog = await fullIndexCatalogSnapshot(connections.ownerClient);
  const restoredData = await weatherRecordDataFingerprint(connections.ownerClient);
  const restoredQueries = await evaluateDeployedQueries(connections.apiClient, queries);
  const queryComparison = compareDeployedQuerySuites(expectedQueries, restoredQueries, false);
  const catalogTransition = evaluateB1CatalogTransition(originalCatalog, finalCatalog, restoredCatalog);
  const dataEqual = expectedData.sha256 === restoredData.sha256;
  return {
    catalogTransition,
    dataEqual,
    pass: predecessorCancelled.pass &&
      catalogTransition.restoredPass &&
      dataEqual &&
      queryComparison.pass,
    predecessorCancelled,
    predecessorValid,
    queryComparison,
    restoredCatalog,
    restoredCatalogSemanticSha256: hashValue(semanticIndexCatalog(restoredCatalog)),
    restoredCatalogSha256: hashValue(restoredCatalog),
    restoredData,
    restoredQueries,
    targetDrop,
  };
}

// cancel and clean one exact owned disposable index build
async function rehearseOwnedCancellation(server, ownerClient, monitorClient, candidate = "full") {
  await ownerClient.query("SET maintenance_work_mem = '64MB'");
  await ownerClient.query("SET max_parallel_maintenance_workers = 0");
  await ownerClient.query("SET lock_timeout = '5s'");
  await ownerClient.query("SET statement_timeout = '45min'");
  await ownerClient.query("SELECT pg_advisory_lock($1::bigint)", [indexLockKey]);
  const beforeDatabase = await databaseResourceSnapshot(monitorClient);
  const beforeContainer = await containerResourceSnapshot(server);
  const evidence = {
    highWater: {
      candidateIndexBytes: 0,
      dataBytes: beforeContainer.dataBytes,
      inodes: beforeContainer.inodes,
      tempBytes: beforeContainer.tempBytes,
      walBytes: beforeContainer.walBytes,
    },
    progress: [],
    samples: 0,
  };
  const state = { active: true };
  const monitorPromise = monitorIndexBuild(server, monitorClient, state, evidence, cancellationIndexName);
  const backend = await ownerClient.query("SELECT pg_backend_pid() AS pid, backend_start FROM pg_stat_activity WHERE pid = pg_backend_pid()");
  const backendIdentity = backend.rows[0];
  const startedAt = new Date().toISOString();

  // normalize successful completion as a failed cancellation attempt
  function buildSucceeded() {
    return { errorCode: null, errorMessage: null, unexpectedlyCompleted: true };
  }

  // retain the exact owned cancellation error
  function buildFailed(error) {
    return {
      errorCode: error.code ?? null,
      errorMessage: error.message ?? String(error),
      unexpectedlyCompleted: false,
    };
  }

  const buildPromise = ownerClient.query(
    `CREATE INDEX CONCURRENTLY ${cancellationIndexName} ON public.weather_records USING btree (source_id, product_run_at)${candidate === "partial-product-run" ? " WHERE product_run_at IS NOT NULL" : ""}`,
  ).then(buildSucceeded, buildFailed);
  await delay(100);
  const cancelResult = await monitorClient.query("SELECT pg_cancel_backend($1::integer) AS cancelled", [backendIdentity.pid]);
  const outcome = await buildPromise;
  state.active = false;

  // release monitoring and the advisory lock after the owned backend stops
  try {
    await monitorPromise;
  } finally {
    await ownerClient.query("SELECT pg_advisory_unlock($1::bigint)", [indexLockKey]);
  }

  const retained = await monitorClient.query(
    "SELECT to_regclass($1)::text AS name",
    [`public.${cancellationIndexName}`],
  );
  const retainedName = retained.rows[0].name;

  // capture the retained invalid object before owned cleanup
  if (retainedName !== null) {
    const retainedDatabase = await buildDatabaseResourceSnapshot(monitorClient, cancellationIndexName);
    const retainedContainer = await containerResourceSnapshot(server);
    retainHighWater(evidence.highWater, {
      container: retainedContainer,
      database: retainedDatabase,
    });
  }

  // remove only the exact disposable object from this rehearsal
  if (retainedName !== null) {
    await ownerClient.query(`DROP INDEX CONCURRENTLY public.${cancellationIndexName}`);
  }

  const afterDatabase = await databaseResourceSnapshot(monitorClient);
  const afterContainer = await containerResourceSnapshot(server);
  const remaining = await monitorClient.query(
    "SELECT to_regclass($1)::text AS name",
    [`public.${cancellationIndexName}`],
  );
  const finishedAt = new Date().toISOString();
  retainHighWater(evidence.highWater, {
    container: afterContainer,
    database: { candidate_index_bytes: 0 },
  });

  return {
    after: { container: afterContainer, database: afterDatabase },
    backendIdentity,
    before: { container: beforeContainer, database: beforeDatabase },
    cancelRequested: cancelResult.rows[0].cancelled === true,
    errorCode: outcome.errorCode,
    errorMessage: outcome.errorMessage,
    finishedAt,
    generatedWalBytes: await walDelta(monitorClient, beforeDatabase.wal_lsn),
    highWater: evidence.highWater,
    objectAbsentAfterCleanup: remaining.rows[0].name === null,
    pass:
      cancelResult.rows[0].cancelled === true &&
      outcome.errorCode === "57014" &&
      outcome.unexpectedlyCompleted === false &&
      remaining.rows[0].name === null,
    progress: evidence.progress,
    retainedObjectBeforeCleanup: retainedName,
    samples: evidence.samples,
    startedAt,
    tempBytesDelta: Number(afterDatabase.temp_bytes) - Number(beforeDatabase.temp_bytes),
    unexpectedlyCompleted: outcome.unexpectedlyCompleted,
  };
}

// measure matched physical ingestion batches
async function measureWriteBatches(client, source, phase, phaseOffsetDays, batchCount = writeBatchCount) {
  const durations = [];
  const outcomes = [];
  const contentHashes = [];
  const contentPhase = phase;
  const firstExclusive = await weatherRecordIdHighWater(client);

  // measure every matched batch independently
  for (let batch = 0; batch < batchCount; batch += 1) {
    const started = performance.now();
    const result = await client.query(`
      INSERT INTO weather_records (
        source_id,
        source_kind,
        valid_at,
        product_run_at,
        first_ingestion_run_id,
        last_ingestion_run_id,
        first_received_at,
        last_received_at,
        upstream_timezone,
        quality_metadata,
        provider_metadata,
        temperature_c,
        precipitation_mm,
        pressure_hpa,
        content_hash
      )
      SELECT
        $1,
        'physical_sensor',
        '2026-10-29T00:00:00Z'::timestamptz + $2 * interval '1 day' +
          ($3 * $4 + row_index) * interval '1 second',
        NULL,
        $5,
        $5,
        '2026-10-29T00:00:00Z'::timestamptz + $2 * interval '1 day' +
          ($3 * $4 + row_index) * interval '1 second',
        '2026-10-29T00:00:00Z'::timestamptz + $2 * interval '1 day' +
          ($3 * $4 + row_index) * interval '1 second',
        'UTC',
        jsonb_build_object('status', repeat('q', 57)),
        jsonb_build_object('request_id', repeat('p', 306)),
        12,
        0.1,
        1000,
        md5(format('%s:%s:%s', $6::text, $3, row_index)) ||
          md5(format('%s-extra:%s:%s', $6::text, $3, row_index))
      FROM generate_series(0, $4::integer - 1) AS row_index
    `, [source.id, phaseOffsetDays, batch, writeRowsPerBatch, source.runId, contentPhase]);
    durations.push(performance.now() - started);
    outcomes.push({ physical: result.rowCount });

    // reproduce every persisted row identity outside timing
    for (let row = 0; row < writeRowsPerBatch; row += 1) {
      contentHashes.push(syntheticContentHash(
        `${contentPhase}:${String(batch)}:${String(row)}`,
        `${contentPhase}-extra:${String(batch)}:${String(row)}`,
      ));
    }
  }

  const lastInclusive = await weatherRecordIdHighWater(client);
  const persistedOutcome = await persistedOutcomeDigest(client, contentHashes, { firstExclusive, lastInclusive });
  const countOutcomeSha256 = countOutcomeHash(outcomes);

  return {
    batches: batchCount,
    countOutcomeSha256,
    durationsMs: durations,
    outcomeSha256: countOutcomeSha256,
    outcomes,
    persistedOutcome,
    persistedOutcomeSha256: persistedOutcome.sha256,
    p95Ms: nearestRankP95(durations),
    rowsPerBatch: writeRowsPerBatch,
  };
}

// measure one matched nonnull forecast ingestion replay
async function measureForecastWriteBatches(client, source, phase, productRunStart, batchCount) {
  const durations = [];
  const outcomes = [];
  const contentHashes = [];
  const before = await writeResourceSnapshot(client);
  const contentPhase = phase;
  const firstExclusive = await weatherRecordIdHighWater(client);

  // measure each complete ten-day product independently
  for (let batch = 0; batch < batchCount; batch += 1) {
    const started = performance.now();
    const result = await client.query(`
      WITH inserted AS (
        INSERT INTO weather_records (
          source_id, source_kind, valid_at, product_run_at,
          first_ingestion_run_id, last_ingestion_run_id,
          first_received_at, last_received_at, upstream_timezone, upstream_model,
          quality_metadata, provider_metadata, temperature_c, pressure_hpa, content_hash
        )
        SELECT
          $1, 'forecast', $6::timestamptz + hour_index * interval '1 hour',
          $6::timestamptz - ($4::integer + $5::integer) * interval '6 hours',
          $2, $2, clock_timestamp(), clock_timestamp(), 'UTC', 'best_match',
          jsonb_build_object('status', repeat('q', 57)),
          jsonb_build_object('request_id', repeat('f', 48)),
          10, 1000,
          md5(format('forecast-write:%s:%s:%s', $3::text, $5, hour_index)) ||
            md5(format('forecast-write-extra:%s:%s:%s', $3::text, $5, hour_index))
        FROM generate_series(0, 239) AS hour_index
        RETURNING 1
      )
      SELECT count(*)::integer AS forecast FROM inserted
    `, [source.id, source.runId, contentPhase, productRunStart, batch, fixedAsOf]);
    durations.push(performance.now() - started);
    outcomes.push(result.rows[0]);

    // reproduce every persisted forecast row identity outside timing
    for (let hour = 0; hour < forecastRowsPerRun; hour += 1) {
      contentHashes.push(syntheticContentHash(
        `forecast-write:${contentPhase}:${String(batch)}:${String(hour)}`,
        `forecast-write-extra:${contentPhase}:${String(batch)}:${String(hour)}`,
      ));
    }
  }

  const after = await writeResourceSnapshot(client);
  const lastInclusive = await weatherRecordIdHighWater(client);
  const persistedOutcome = await persistedOutcomeDigest(client, contentHashes, { firstExclusive, lastInclusive });
  const countOutcomeSha256 = countOutcomeHash(outcomes);
  return {
    batchCount,
    batchMix: { forecast: forecastRowsPerRun },
    countOutcomeSha256,
    durationsMs: durations,
    generatedWalBytes: await walDelta(client, before.wal_lsn),
    outcomeSha256: countOutcomeSha256,
    outcomes,
    persistedOutcome,
    persistedOutcomeSha256: persistedOutcome.sha256,
    p95Ms: nearestRankP95(durations),
    relationBytesDelta: Number(after.total_bytes) - Number(before.total_bytes),
  };
}

// represent an intentionally unmeasured tiny-fixture replay
function unmeasuredForecastWriteReplay() {
  return {
    batchCount: 0,
    batchMix: { forecast: forecastRowsPerRun },
    durationsMs: [],
    generatedWalBytes: 0,
    outcomeSha256: hashValue([]),
    outcomes: [],
    p95Ms: null,
    relationBytesDelta: 0,
  };
}

// measure one matched forecast tide and physical replay mix
export async function measureMixedIngestionBatches(client, catalog, phase, productRunStart, batchCount, timeOffsetYears = phase === "baseline" ? 0 : 1) {
  const durations = [];
  const outcomes = [];
  const contentHashes = [];
  const before = await writeResourceSnapshot(client);
  const contentPhase = phase;
  const firstExclusive = await weatherRecordIdHighWater(client);

  // measure each complete scheduled-event mix
  for (let batch = 0; batch < batchCount; batch += 1) {
    const started = performance.now();
    const result = await client.query(`
      WITH forecast_rows AS (
        INSERT INTO weather_records (
          source_id, source_kind, valid_at, product_run_at,
          first_ingestion_run_id, last_ingestion_run_id,
          first_received_at, last_received_at, upstream_timezone, upstream_model,
          quality_metadata, provider_metadata, temperature_c, pressure_hpa, content_hash
        )
        SELECT
          $1, 'forecast', $11::timestamptz + hour_index * interval '1 hour',
          $11::timestamptz - ($12::integer + $13::integer) * interval '6 hours',
          $2, $2, clock_timestamp(), clock_timestamp(), 'UTC', 'best_match',
          jsonb_build_object('status', repeat('q', 57)),
          jsonb_build_object('request_id', repeat('f', 48)),
          10, 1000,
          md5(format('mixed-forecast:%s:%s:%s', $10::text, $13, hour_index)) ||
            md5(format('mixed-forecast-extra:%s:%s:%s', $10::text, $13, hour_index))
        FROM generate_series(0, 239) AS hour_index
        RETURNING 1
      ),
      tide_observation_rows AS (
        INSERT INTO weather_records (
          source_id, source_kind, valid_at, product_run_at,
          first_ingestion_run_id, last_ingestion_run_id,
          first_received_at, last_received_at, upstream_timezone,
          quality_metadata, provider_metadata, water_level_m, content_hash
        )
        SELECT
          $3, 'tide_observation',
          '2010-01-01T00:00:00Z'::timestamptz + $9::integer * interval '1 year' +
            ($13::integer * 24 + row_index) * interval '1 hour',
          NULL, $4, $4, clock_timestamp(), clock_timestamp(), 'UTC',
          jsonb_build_object('status', repeat('q', 57)),
          jsonb_build_object('prediction_type', 'observed', 'request_id', repeat('t', 94)),
          row_index::double precision / 100,
          md5(format('mixed-tide-observed:%s:%s:%s', $10::text, $13, row_index)) ||
            md5(format('mixed-tide-observed-extra:%s:%s:%s', $10::text, $13, row_index))
        FROM generate_series(0, 23) AS row_index
        RETURNING 1
      ),
      tide_prediction_rows AS (
        INSERT INTO weather_records (
          source_id, source_kind, valid_at, product_run_at,
          first_ingestion_run_id, last_ingestion_run_id,
          first_received_at, last_received_at, upstream_timezone,
          quality_metadata, provider_metadata, water_level_m, content_hash
        )
        SELECT
          $5, 'tide_prediction',
          '2010-01-01T00:00:00Z'::timestamptz + $9::integer * interval '1 year' +
            ($13::integer * 24 + row_index) * interval '1 hour',
          NULL, $6, $6, clock_timestamp(), clock_timestamp(), 'UTC',
          jsonb_build_object('status', repeat('q', 57)),
          jsonb_build_object('prediction_type', 'predicted', 'request_id', repeat('t', 94)),
          row_index::double precision / 100,
          md5(format('mixed-tide-predicted:%s:%s:%s', $10::text, $13, row_index)) ||
            md5(format('mixed-tide-predicted-extra:%s:%s:%s', $10::text, $13, row_index))
        FROM generate_series(0, 23) AS row_index
        RETURNING 1
      ),
      physical_rows AS (
        INSERT INTO weather_records (
          source_id, source_kind, valid_at, product_run_at,
          first_ingestion_run_id, last_ingestion_run_id,
          first_received_at, last_received_at, upstream_timezone,
          quality_metadata, provider_metadata, temperature_c, pressure_hpa, content_hash
        )
        SELECT
          $7, 'physical_sensor',
          '2010-01-01T00:00:00Z'::timestamptz + $9::integer * interval '1 year' +
            ($13::integer * 50 + row_index) * interval '1 minute',
          NULL, $8, $8, clock_timestamp(), clock_timestamp(), 'UTC',
          jsonb_build_object('status', repeat('q', 57)),
          jsonb_build_object('request_id', repeat('p', 306)),
          12, 1000,
          md5(format('mixed-physical:%s:%s:%s', $10::text, $13, row_index)) ||
            md5(format('mixed-physical-extra:%s:%s:%s', $10::text, $13, row_index))
        FROM generate_series(0, 49) AS row_index
        RETURNING 1
      )
      SELECT
        (SELECT count(*) FROM forecast_rows)::integer AS forecast,
        (SELECT count(*) FROM tide_observation_rows)::integer AS tide_observation,
        (SELECT count(*) FROM tide_prediction_rows)::integer AS tide_prediction,
        (SELECT count(*) FROM physical_rows)::integer AS physical
    `, [
      catalog.forecast.id,
      catalog.forecast.runId,
      catalog.tideObservation.id,
      catalog.tideObservation.runId,
      catalog.tidePrediction.id,
      catalog.tidePrediction.runId,
      catalog.physical[0].id,
      catalog.physical[0].runId,
      timeOffsetYears,
      contentPhase,
      fixedAsOf,
      productRunStart,
      batch,
    ]);
    durations.push(performance.now() - started);
    outcomes.push(result.rows[0]);

    // reproduce every persisted forecast row identity outside timing
    for (let hour = 0; hour < forecastRowsPerRun; hour += 1) {
      contentHashes.push(syntheticContentHash(
        `mixed-forecast:${contentPhase}:${String(batch)}:${String(hour)}`,
        `mixed-forecast-extra:${contentPhase}:${String(batch)}:${String(hour)}`,
      ));
    }
    // reproduce both tide source identities outside timing
    for (let row = 0; row < 24; row += 1) {
      contentHashes.push(syntheticContentHash(
        `mixed-tide-observed:${contentPhase}:${String(batch)}:${String(row)}`,
        `mixed-tide-observed-extra:${contentPhase}:${String(batch)}:${String(row)}`,
      ));
      contentHashes.push(syntheticContentHash(
        `mixed-tide-predicted:${contentPhase}:${String(batch)}:${String(row)}`,
        `mixed-tide-predicted-extra:${contentPhase}:${String(batch)}:${String(row)}`,
      ));
    }
    // reproduce every persisted physical row identity outside timing
    for (let row = 0; row < writeRowsPerBatch; row += 1) {
      contentHashes.push(syntheticContentHash(
        `mixed-physical:${contentPhase}:${String(batch)}:${String(row)}`,
        `mixed-physical-extra:${contentPhase}:${String(batch)}:${String(row)}`,
      ));
    }
  }

  const after = await writeResourceSnapshot(client);
  const lastInclusive = await weatherRecordIdHighWater(client);
  const persistedOutcome = await persistedOutcomeDigest(client, contentHashes, { firstExclusive, lastInclusive });
  const countOutcomeSha256 = countOutcomeHash(outcomes);
  return {
    batchCount,
    batchMix: {
      forecast: 240,
      physical: 50,
      tideObservation: 24,
      tidePrediction: 24,
    },
    countOutcomeSha256,
    durationsMs: durations,
    generatedWalBytes: await walDelta(client, before.wal_lsn),
    outcomeSha256: countOutcomeSha256,
    outcomes,
    persistedOutcome,
    persistedOutcomeSha256: persistedOutcome.sha256,
    p95Ms: nearestRankP95(durations),
    relationBytesDelta: Number(after.total_bytes) - Number(before.total_bytes),
  };
}

// derive one stable synthetic ingestion driver identity
export function syntheticIngestionDriverId({ cadenceMs, eventCount, fixtureSha256, startedAt }) {
  // require a complete bounded driver declaration
  if (
    !Number.isInteger(cadenceMs) || cadenceMs < 1 ||
    !Number.isInteger(eventCount) || eventCount < 1 ||
    !/^[a-f0-9]{64}$/u.test(fixtureSha256 ?? "") ||
    !Number.isFinite(Date.parse(startedAt))
  ) {
    throw new Error("synthetic ingestion driver identity input is invalid");
  }

  return createHash("sha256").update([
    "synthetic-normal-ingestion/v1",
    fixtureSha256,
    startedAt,
    String(cadenceMs),
    String(eventCount),
  ].join("\n")).digest("hex");
}

// capture one migration ledger hash
async function migrationLedger(client) {
  const result = await client.query("SELECT name, checksum FROM schema_migrations ORDER BY name");
  return { entries: result.rowCount, sha256: hashValue(result.rows) };
}

// capture sampled row and metadata sizes by source kind
async function rowShapeSnapshot(client) {
  const shapes = [];

  // sample every production-shaped source kind independently
  for (const kind of [
    "physical_sensor",
    "forecast",
    "tide_observation",
    "tide_prediction",
    "model_current",
    "reanalysis",
  ]) {
    const result = await client.query(`
      SELECT
        $1::text AS source_kind,
        avg(pg_column_size(sample))::double precision AS row_bytes,
        avg(octet_length((sample).provider_metadata::text))::double precision AS provider_metadata_bytes,
        avg(octet_length((sample).quality_metadata::text))::double precision AS quality_metadata_bytes
      FROM (
        SELECT record AS sample
        FROM weather_records record
        WHERE record.source_kind = $1
        ORDER BY record.valid_at ASC, record.id ASC
        LIMIT 1000
      ) sampled
    `, [kind]);
    shapes.push(result.rows[0]);
  }

  return shapes;
}

// write progress without exposing synthetic row contents
function defaultProgress(kind, inserted) {
  process.stderr.write(`seed ${kind}: ${String(inserted)} rows\n`);
}

// create one collision-resistant disposable docker identity
function disposableDockerName(label) {
  return `weather-${label}-${String(process.pid)}-${randomBytes(4).toString("hex")}`;
}

// create one private named PostgreSQL volume
async function createPostgresVolume(label) {
  const name = disposableDockerName(label);
  await executeFile("docker", ["volume", "create", name], { timeout: 30_000 });
  return name;
}

// wait for one volume-backed PostgreSQL server
async function waitForVolumePostgres(server) {
  let lastError = null;

  // retry only the bounded startup interval
  for (let attempt = 0; attempt < 90; attempt += 1) {
    const pool = createTestPool(server);

    try {
      await pool.query("SELECT 1");
      await pool.end();
      return;
    } catch (error) {
      lastError = error;
      await pool.end().catch(ignoreCleanupError);
      await delay(500);
    }
  }

  throw new Error("volume-backed PostgreSQL did not become ready", { cause: lastError });
}

// start one PostgreSQL 17 container on an exact named volume
async function startVolumePostgres(volume, label) {
  const name = disposableDockerName(label);
  await executeFile("docker", [
    "run",
    "--detach",
    "--rm",
    "--name",
    name,
    "--env",
    "POSTGRES_PASSWORD=postgres-test",
    "--env",
    "POSTGRES_DB=weather_test",
    "--publish",
    "127.0.0.1::5432",
    "--mount",
    `type=volume,source=${volume},target=/var/lib/postgresql/data`,
    crossoverPostgresImage,
  ], { timeout: 180_000 });

  try {
    const inspected = await executeFile("docker", [
      "inspect",
      "--format",
      "{{(index (index .NetworkSettings.Ports \"5432/tcp\") 0).HostPort}}",
      name,
    ], { timeout: 10_000 });
    const server = {
      host: "127.0.0.1",
      name,
      password: "postgres-test",
      port: Number(inspected.stdout.trim()),
      user: "postgres",
      volume,
    };
    await waitForVolumePostgres(server);
    return server;
  } catch (error) {
    await executeFile("docker", ["rm", "--force", name], { timeout: 30_000 }).catch(ignoreCleanupError);
    throw error;
  }
}

// stop one PostgreSQL container cleanly before volume operations
async function stopVolumePostgres(server) {
  // ignore an already-stopped cleanup target
  if (server === null) {
    return;
  }
  await executeFile("docker", ["stop", "--time", "60", server.name], { timeout: 90_000 });
}

// clone one cleanly stopped PostgreSQL volume offline
async function cloneStoppedPostgresVolume(source, destination) {
  await executeFile("docker", [
    "run",
    "--rm",
    "--mount",
    `type=volume,source=${source},target=/from,readonly`,
    "--mount",
    `type=volume,source=${destination},target=/to`,
    crossoverPostgresImage,
    "sh",
    "-ceu",
    "cp -a /from/. /to/",
  ], { maxBuffer: 1024 * 1024, timeout: 45 * 60_000 });
}

// verify one recovered synthetic seed is present and cleanly stopped
async function inspectStoppedCrossoverSeed(volume) {
  const attached = await executeFile("docker", [
    "ps",
    "--all",
    "--quiet",
    "--filter",
    `volume=${volume}`,
  ], { timeout: 30_000 });

  // refuse a live or stopped container attachment during offline cloning
  if (attached.stdout.trim() !== "") {
    throw new Error("recovered crossover seed volume is attached to a container");
  }

  const inspected = await executeFile("docker", [
    "volume",
    "inspect",
    "--format",
    "{{json .Name}}\t{{json .Driver}}\t{{json .CreatedAt}}",
    volume,
  ], { timeout: 30_000 });
  const [name, driver, createdAt] = inspected.stdout.trim().split("\t").map(
    // decode one Docker JSON-formatted identity field
    function parseIdentity(field) {
      return JSON.parse(field);
    },
  );

  // bind recovery to the requested local named volume
  if (name !== volume || driver !== "local" || typeof createdAt !== "string" || !Number.isFinite(Date.parse(createdAt))) {
    throw new Error("recovered crossover seed identity is invalid");
  }

  return { createdAt, driver, name };
}

// remove one exact disposable volume
async function removePostgresVolume(volume) {
  // ignore an absent cleanup target
  if (volume === null) {
    return;
  }
  // retry bounded container-removal propagation
  for (let attempt = 0; attempt < 5; attempt += 1) {
    try {
      await executeFile("docker", ["volume", "rm", volume], { timeout: 60_000 });
      return;
    } catch {
      await delay(500);
    }
  }
}

// capture one immutable PostgreSQL image identity
async function crossoverPostgresImageIdentity() {
  const inspected = await executeFile("docker", [
    "image",
    "inspect",
    "--format",
    "{{json .Id}}\t{{json .RepoDigests}}",
    crossoverPostgresImage,
  ], { timeout: 30_000 });
  const [idJson, digestsJson] = inspected.stdout.trim().split("\t");
  const id = JSON.parse(idJson);
  const digests = JSON.parse(digestsJson);

  // require the pulled immutable content digest
  if (!/^sha256:[a-f0-9]{64}$/u.test(id) || !Array.isArray(digests) || digests.length === 0) {
    throw new Error("crossover PostgreSQL image identity is incomplete");
  }

  return { digests, id, reference: crossoverPostgresImage };
}

// capture equality-critical pre-treatment fixture identity
async function crossoverFixtureIdentity(catalogClient, queryClient, capturedQuery) {
  const rows = await catalogClient.query(`
    SELECT source_kind, count(*)::bigint AS rows
    FROM weather_records
    GROUP BY source_kind
    ORDER BY source_kind
  `);
  const statistics = await catalogClient.query(`
    SELECT attname, null_frac, n_distinct, correlation
    FROM pg_stats
    WHERE schemaname = 'public' AND tablename = 'weather_records'
    ORDER BY attname
  `);
  const settings = await catalogClient.query(`
    SELECT name, setting, unit
    FROM pg_settings
    WHERE name = ANY($1::text[])
    ORDER BY name
  `, [[
    "block_size",
    "effective_cache_size",
    "maintenance_work_mem",
    "max_parallel_maintenance_workers",
    "random_page_cost",
    "seq_page_cost",
    "server_version_num",
    "shared_buffers",
    "synchronous_commit",
    "wal_segment_size",
    "work_mem",
  ]]);
  const catalog = await catalogSnapshot(catalogClient);
  const semantics = await querySemantics(queryClient, capturedQuery);
  return {
    catalogSha256: hashValue(catalog),
    distribution: rows.rows,
    distributionSha256: hashValue(rows.rows),
    rows: rows.rows.reduce(
      // sum exact source-kind counts
      function sumRows(total, row) {
        return total + Number(row.rows);
      },
      0,
    ),
    semanticHash: semantics.hash,
    semanticRows: semantics.rows,
    settings: settings.rows,
    settingsSha256: hashValue(settings.rows),
    statisticsSha256: hashValue(statistics.rows),
  };
}

// capture objective background-work overlap evidence
async function crossoverBackgroundSnapshot(client) {
  const result = await client.query(`
    SELECT json_build_object(
      'observedAt', clock_timestamp(),
      'walLsn', pg_current_wal_lsn(),
      'activeAutovacuum', (
        SELECT count(*)::integer
        FROM pg_stat_activity
        WHERE backend_type = 'autovacuum worker' AND state = 'active'
      ),
      'progressRows',
        (SELECT count(*)::integer FROM pg_stat_progress_vacuum) +
        (SELECT count(*)::integer FROM pg_stat_progress_analyze) +
        (SELECT count(*)::integer FROM pg_stat_progress_create_index),
      'foreignDdl', (
        SELECT count(*)::integer
        FROM pg_stat_activity
        WHERE pid <> pg_backend_pid()
          AND state = 'active'
          AND query ~* '(create|drop|reindex)[[:space:]]+(unique[[:space:]]+)?index'
      ),
      'bgwriter', (SELECT to_jsonb(stats) - 'stats_reset' FROM pg_stat_bgwriter stats),
      'checkpointer', (SELECT to_jsonb(stats) - 'stats_reset' FROM pg_stat_checkpointer stats),
      'wal', (SELECT to_jsonb(stats) - 'stats_reset' FROM pg_stat_wal stats),
      'io', (
        SELECT jsonb_agg(to_jsonb(stats) - 'stats_reset' ORDER BY backend_type, object, context)
        FROM pg_stat_io stats
      )
    ) AS observation
  `);
  return result.rows[0].observation;
}

// capture one cheap filesystem resource-floor snapshot
async function filesystemFloorSnapshot(server) {
  const script = `
    set -eu
    df -B1 --output=avail,iavail "$PGDATA" | tail -1
  `;
  const result = await executeFile("docker", ["exec", server.name, "sh", "-c", script], {
    maxBuffer: 1024 * 1024,
    timeout: 30_000,
  });
  return parseFilesystemFloorOutput(result.stdout);
}

// parse one closed filesystem-floor observation
export function parseFilesystemFloorOutput(output) {
  const values = String(output).trim().split(/\s+/u).map(Number);

  // reject missing or nonnumeric floor evidence
  if (values.length !== 2 || values.some(
    // detect one invalid resource value
    function invalidValue(value) {
      return !Number.isSafeInteger(value) || value < 0;
    },
  )) {
    throw new Error("filesystem floor observation is incomplete");
  }
  return { availableBytes: values[0], availableInodes: values[1] };
}

// read one version-tolerant checkpoint counter total
function checkpointCounter(snapshot) {
  const checkpointer = snapshot?.checkpointer ?? {};
  return Number(
    checkpointer.num_timed ?? checkpointer.checkpoints_timed ?? 0,
  ) + Number(
    checkpointer.num_requested ?? checkpointer.checkpoints_req ?? 0,
  );
}

// decide objective crossover invalidation from observed background work
function crossoverBackgroundInvalid(before, after) {
  return Number(before?.activeAutovacuum ?? 0) > 0 ||
    Number(after?.activeAutovacuum ?? 0) > 0 ||
    Number(before?.progressRows ?? 0) > 0 ||
    Number(after?.progressRows ?? 0) > 0 ||
    checkpointCounter(after) !== checkpointCounter(before);
}

// prepare one fixture before the symmetric restart
async function prepareCrossoverForRestart(ownerClient, adminClient) {
  await ownerClient.query("VACUUM (ANALYZE) weather_records");
  await adminClient.query("CHECKPOINT");
}

// prepare one restarted fixture through the same exact query sequence
async function prepareCrossoverAfterRestart(adminClient, apiClient, capturedQuery) {
  // execute the same exact query cache preparation count
  for (let attempt = 0; attempt < 3; attempt += 1) {
    await apiClient.query(capturedQuery.text, capturedQuery.values);
  }

  await adminClient.query("CHECKPOINT");
}

// measure one declared crossover stratum
async function measureCrossoverStratum(client, catalog, stratum, run, warmup) {
  const count = warmup
    ? MATCHED_CROSSOVER_PROTOCOL.measurement.warmupBatchesPerStratum
    : MATCHED_CROSSOVER_PROTOCOL.measurement.measuredBatchesPerStratum;
  const phase = `crossover-run-${String(run + 1)}-${stratum}-${warmup ? "warmup" : "measured"}`;
  const lane = warmup ? 0 : 1;

  // isolate physical NULL writes in one matched time window
  if (stratum === "physical-null-50") {
    return await measureWriteBatches(
      client,
      catalog.physical[0],
      phase,
      100 + run * 10 + lane,
      count,
    );
  }
  // isolate forecast nonNULL product writes
  if (stratum === "forecast-nonnull-240") {
    return await measureForecastWriteBatches(
      client,
      catalog.forecast,
      phase,
      10_000 + run * 1_000 + lane * 100,
      count,
    );
  }
  // replay the exact mixed scheduled-event envelope
  if (stratum === "mixed-forecast240-tide24-24-physical50") {
    return await measureMixedIngestionBatches(
      client,
      catalog,
      phase,
      20_000 + run * 1_000 + lane * 100,
      count,
      10 + run * 2 + lane,
    );
  }
  throw new Error(`unknown crossover stratum ${String(stratum)}`);
}

// open exact owner API and monitor connections for one crossover fixture
async function openCrossoverConnections(server) {
  const adminPool = createTestPool(server);
  const ownerPool = createTestPool(server, "weather_test", "weather_owner", "owner-test");
  const apiPool = createTestPool(server, "weather_test", "weather_api", "api-test");
  const adminClient = await adminPool.connect();
  const ownerClient = await ownerPool.connect();
  const monitorClient = await ownerPool.connect();
  const apiClient = await apiPool.connect();
  await adminClient.query("SET statement_timeout = '45min'");
  await ownerClient.query("SET statement_timeout = '45min'");
  await monitorClient.query("SET statement_timeout = '45min'");
  await apiClient.query("SET statement_timeout = '45min'");
  return { adminClient, adminPool, apiClient, apiPool, monitorClient, ownerClient, ownerPool };
}

// close every crossover connection before a clean stop
async function closeCrossoverConnections(connections) {
  // ignore an unopened connection set
  if (connections === null) {
    return;
  }
  connections.adminClient.release();
  connections.apiClient.release();
  connections.monitorClient.release();
  connections.ownerClient.release();
  await connections.apiPool.end();
  await connections.adminPool.end();
  await connections.ownerPool.end();
}

// execute the exact balanced four-run measurement sequence
async function executeCrossoverRuns(baselineConnections, candidateConnections, catalog) {
  const runs = [];
  const connectionsByName = {
    baseline: baselineConnections,
    candidate: candidateConnections,
  };

  // execute every predeclared run exactly once
  for (let run = 0; run < MATCHED_CROSSOVER_PROTOCOL.measurement.runs; run += 1) {
    const order = MATCHED_CROSSOVER_PROTOCOL.measurement.orderByRun[run];
    const names = order === "baseline-candidate"
      ? ["baseline", "candidate"]
      : ["candidate", "baseline"];
    const backgroundBefore = {
      baseline: await crossoverBackgroundSnapshot(baselineConnections.monitorClient),
      candidate: await crossoverBackgroundSnapshot(candidateConnections.monitorClient),
    };
    const strata = [];

    // measure every declared stratum in the frozen order
    for (const stratum of MATCHED_CROSSOVER_PROTOCOL.measurement.strata) {
      const warmups = {};
      const measurements = {};

      // execute the balanced participant order for this run
      for (const name of names) {
        warmups[name] = await measureCrossoverStratum(
          connectionsByName[name].ownerClient,
          catalog,
          stratum,
          run,
          true,
        );
        measurements[name] = await measureCrossoverStratum(
          connectionsByName[name].ownerClient,
          catalog,
          stratum,
          run,
          false,
        );
      }

      const p95Ratio = measurements.candidate.p95Ms / measurements.baseline.p95Ms;
      const countOutcomesEqual = measurements.candidate.countOutcomeSha256 === measurements.baseline.countOutcomeSha256;
      const persistedOutcomesEqual = measurements.candidate.persistedOutcomeSha256 === measurements.baseline.persistedOutcomeSha256;
      const outcomesEqual = countOutcomesEqual && persistedOutcomesEqual;
      strata.push({
        countOutcomesEqual,
        measurements,
        outcomesEqual,
        persistedOutcomesEqual,
        p95Ratio,
        pass: p95Ratio <= 1.10 && outcomesEqual,
        stratum,
        warmups,
      });
    }

    const backgroundAfter = {
      baseline: await crossoverBackgroundSnapshot(baselineConnections.monitorClient),
      candidate: await crossoverBackgroundSnapshot(candidateConnections.monitorClient),
    };
    const backgroundInvalid = crossoverBackgroundInvalid(backgroundBefore.baseline, backgroundAfter.baseline) ||
      crossoverBackgroundInvalid(backgroundBefore.candidate, backgroundAfter.candidate);
    runs.push({
      backgroundAfter,
      backgroundBefore,
      backgroundInvalid,
      order,
      pass: !backgroundInvalid && strata.every(
        // require every stratum independently
        function stratumPassed(stratum) {
          return stratum.pass;
        },
      ),
      run: run + 1,
      strata,
    });
  }

  return runs;
}

// identify only prohibited measurement interference
function b1CounterInterference(snapshot) {
  return Number(snapshot?.activeAutovacuum ?? 0) > 0 ||
    Number(snapshot?.progressRows ?? 0) > 0 ||
    Number(snapshot?.foreignDdl ?? 0) > 0;
}

// require one frozen catalog identity throughout measured writes
async function catalogSemanticSha256(client) {
  return hashValue(semanticIndexCatalog(await fullIndexCatalogSnapshot(client)));
}

// execute four balanced B1 runs while retaining every workload counter
async function executeB1Runs(
  baselineServer,
  candidateServer,
  baselineConnections,
  candidateConnections,
  catalog,
  expectedCatalogs,
) {
  const runs = [];
  const connectionsByName = { baseline: baselineConnections, candidate: candidateConnections };

  // execute every predeclared balanced run exactly once
  for (let run = 0; run < B1_PROTOCOL.measurement.runs; run += 1) {
    const order = B1_PROTOCOL.measurement.orderByRun[run];
    const names = order === "baseline-candidate" ? ["baseline", "candidate"] : ["candidate", "baseline"];
    const resourceBefore = {
      baseline: await filesystemFloorSnapshot(baselineServer),
      candidate: await filesystemFloorSnapshot(candidateServer),
    };
    const strata = [];

    // measure all three fixed write strata without exclusions
    for (const stratum of B1_PROTOCOL.measurement.strata) {
      const countersBefore = {
        baseline: await crossoverBackgroundSnapshot(baselineConnections.monitorClient),
        candidate: await crossoverBackgroundSnapshot(candidateConnections.monitorClient),
      };
      const warmups = {};
      const measurements = {};

      // execute the frozen participant order for this stratum
      for (const name of names) {
        warmups[name] = await measureCrossoverStratum(
          connectionsByName[name].ownerClient,
          catalog,
          stratum,
          run,
          true,
        );
        measurements[name] = await measureCrossoverStratum(
          connectionsByName[name].ownerClient,
          catalog,
          stratum,
          run,
          false,
        );
      }

      const countersAfter = {
        baseline: await crossoverBackgroundSnapshot(baselineConnections.monitorClient),
        candidate: await crossoverBackgroundSnapshot(candidateConnections.monitorClient),
      };
      const p95Ratio = measurements.candidate.p95Ms / measurements.baseline.p95Ms;
      const countOutcomesEqual = measurements.candidate.countOutcomeSha256 === measurements.baseline.countOutcomeSha256;
      const persistedOutcomesEqual = measurements.candidate.persistedOutcomeSha256 === measurements.baseline.persistedOutcomeSha256;
      const outcomesEqual = countOutcomesEqual && persistedOutcomesEqual;
      const interference = Object.values(countersBefore).some(b1CounterInterference) ||
        Object.values(countersAfter).some(b1CounterInterference);
      strata.push({
        countOutcomesEqual,
        countersAfter,
        countersBefore,
        interference,
        measurements,
        outcomesEqual,
        pass: !interference && p95Ratio <= 1.10 && outcomesEqual,
        persistedOutcomesEqual,
        p95Ratio,
        stratum,
        warmups,
      });
    }

    const resourceAfter = {
      baseline: await filesystemFloorSnapshot(baselineServer),
      candidate: await filesystemFloorSnapshot(candidateServer),
    };
    const catalogAfter = {
      baseline: await catalogSemanticSha256(baselineConnections.ownerClient),
      candidate: await catalogSemanticSha256(candidateConnections.ownerClient),
    };
    const catalogStable = catalogAfter.baseline === expectedCatalogs.baseline &&
      catalogAfter.candidate === expectedCatalogs.candidate;
    const resourceFloorPass = [resourceBefore, resourceAfter].every(
      // require both participants above frozen resource floors
      function resourcePairPassed(pair) {
        return Object.values(pair).every(
          // check one participant resource floor
          function resourcePassed(resource) {
            return resource.availableBytes >= B1_PROTOCOL.resourceFloors.availableBytes &&
              resource.availableInodes >= B1_PROTOCOL.resourceFloors.availableInodes;
          },
        );
      },
    );
    runs.push({
      catalogAfter,
      catalogStable,
      order,
      pass: catalogStable && resourceFloorPass && strata.every(
        // require every stratum independently
        function stratumPassed(stratum) {
          return stratum.pass;
        },
      ),
      resourceAfter,
      resourceBefore,
      resourceFloorPass,
      run: run + 1,
      strata,
    });
  }

  return runs;
}

// execute the frozen two-volume matched crossover experiment
export async function runMatchedCrossover(options, hooks = {}) {
  const progress = hooks.progress ?? defaultProgress;
  const privateRoot = await mkdtemp(join(tmpdir(), "weather-page-index-crossover-"));
  await chmod(privateRoot, 0o700);
  const distribution = calculateSyntheticDistribution(options.totalRows, options.forecastRows);
  const ownsSeedVolume = options.seedVolume === null || options.seedVolume === undefined;
  let seedVolume = null;
  let baselineVolume = null;
  let candidateVolume = null;
  let seedServer = null;
  let baselineServer = null;
  let candidateServer = null;
  let baselineConnections = null;
  let candidateConnections = null;
  let catalog = null;
  let recoveredSeed = null;
  let recoveredSeedQualification = null;

  try {
    const migrations = await materializeFrozenMigrations(privateRoot);
    const image = await crossoverPostgresImageIdentity();
    const capturedQuery = extractCapturedForecastQuery(await readFile(queryCapturePath, "utf8"));

    // seed one new full synthetic volume only when recovery is absent
    if (ownsSeedVolume) {
      seedVolume = await createPostgresVolume("wpi-crossover-seed");
      seedServer = await startVolumePostgres(seedVolume, "wpi-crossover-seed");
      const seedPool = createTestPool(seedServer);
      await prepareRuntimeRoles(seedPool);
      const seedOwnerPool = createTestPool(seedServer, "weather_test", "weather_owner", "owner-test");
      await runMigrations(seedOwnerPool, migrations.directory, {
        lockTimeoutMs: 5_000,
        statementTimeoutMs: 120_000,
      });
      const seedClient = await seedOwnerPool.connect();

      try {
        await seedClient.query("SET statement_timeout = '45min'");
        await seedClient.query("SET synchronous_commit = off");
        catalog = await bootstrapSyntheticCatalog(seedClient);
        await seedForecastRuns(
          seedClient,
          catalog.forecast,
          0,
          options.forecastRows / forecastRowsPerRun,
          progress,
        );
        await seedSyntheticPadding(seedClient, catalog, distribution, progress);
        await seedClient.query("RESET synchronous_commit");
        await seedClient.query("VACUUM (ANALYZE) weather_records");
        await seedPool.query("CHECKPOINT");
      } finally {
        seedClient.release();
        await seedOwnerPool.end();
        await seedPool.end();
      }

      await stopVolumePostgres(seedServer);
      seedServer = null;
    } else {
      seedVolume = options.seedVolume;
      recoveredSeed = await inspectStoppedCrossoverSeed(seedVolume);
    }

    baselineVolume = await createPostgresVolume("wpi-crossover-baseline");
    candidateVolume = await createPostgresVolume("wpi-crossover-candidate");
    await cloneStoppedPostgresVolume(seedVolume, baselineVolume);
    await cloneStoppedPostgresVolume(seedVolume, candidateVolume);
    // remove only a seed owned by this successful invocation
    if (ownsSeedVolume) {
      await removePostgresVolume(seedVolume);
      seedVolume = null;
    }
    baselineServer = await startVolumePostgres(baselineVolume, "wpi-crossover-baseline");
    candidateServer = await startVolumePostgres(candidateVolume, "wpi-crossover-candidate");
    baselineConnections = await openCrossoverConnections(baselineServer);
    candidateConnections = await openCrossoverConnections(candidateServer);
    catalog ??= await loadSyntheticCatalog(baselineConnections.ownerClient);

    // qualify only a caller-supplied borrowed seed clone before treatment
    if (!ownsSeedVolume) {
      recoveredSeedQualification = await qualifyRecoveredCrossoverSeed(
        baselineConnections.ownerClient,
        distribution,
      );
    }
    const baselineIdentity = await crossoverFixtureIdentity(
      baselineConnections.ownerClient,
      baselineConnections.apiClient,
      capturedQuery,
    );
    const candidateIdentity = await crossoverFixtureIdentity(
      candidateConnections.ownerClient,
      candidateConnections.apiClient,
      capturedQuery,
    );
    const preTreatmentEqual = isDeepStrictEqual(baselineIdentity, candidateIdentity);

    // refuse treatment when the two restored fixtures are not exact matches
    if (!preTreatmentEqual) {
      throw new Error("matched crossover fixtures differ before treatment");
    }

    const build = await buildCandidateIndex(
      candidateServer,
      candidateConnections.ownerClient,
      candidateConnections.monitorClient,
      PARTIAL_CANDIDATE_INDEX_SQL,
    );
    const candidateCatalogAfterBuild = await catalogSnapshot(candidateConnections.ownerClient);
    const candidateIndex = candidateCatalogAfterBuild.find(findCandidateIndex) ?? null;
    const canonicalDefinitionMatches = candidateIndex?.definition === PARTIAL_CANDIDATE_CANONICAL_DEFINITION;
    await prepareCrossoverForRestart(baselineConnections.ownerClient, baselineConnections.adminClient);
    await prepareCrossoverForRestart(candidateConnections.ownerClient, candidateConnections.adminClient);
    await closeCrossoverConnections(baselineConnections);
    await closeCrossoverConnections(candidateConnections);
    baselineConnections = null;
    candidateConnections = null;
    await stopVolumePostgres(baselineServer);
    await stopVolumePostgres(candidateServer);
    baselineServer = null;
    candidateServer = null;
    baselineServer = await startVolumePostgres(baselineVolume, "wpi-crossover-baseline-measure");
    candidateServer = await startVolumePostgres(candidateVolume, "wpi-crossover-candidate-measure");
    baselineConnections = await openCrossoverConnections(baselineServer);
    candidateConnections = await openCrossoverConnections(candidateServer);
    await prepareCrossoverAfterRestart(
      baselineConnections.adminClient,
      baselineConnections.apiClient,
      capturedQuery,
    );
    await prepareCrossoverAfterRestart(
      candidateConnections.adminClient,
      candidateConnections.apiClient,
      capturedQuery,
    );
    await delay(5_000);

    const runs = await executeCrossoverRuns(
      baselineConnections,
      candidateConnections,
      catalog,
    );
    const baselineSemantics = await querySemantics(baselineConnections.apiClient, capturedQuery);
    const candidateSemantics = await querySemantics(candidateConnections.apiClient, capturedQuery);
    const candidatePlan = await explainCapturedQuery(candidateConnections.apiClient, capturedQuery);
    const candidatePlanSummary = summarizePlan(candidatePlan);
    const cancellation = await rehearseOwnedCancellation(
      candidateServer,
      candidateConnections.ownerClient,
      candidateConnections.monitorClient,
      "partial-product-run",
    );
    const planPass = candidatePlanSummary.indexNames.includes(CANDIDATE_INDEX_NAME) &&
      !candidatePlanSummary.weatherRecordSequentialScan &&
      candidatePlanSummary.candidateProductSelectionRows <= 5 &&
      candidatePlanSummary.candidateResultRows <= forecastRowsPerRun;
    const semanticsEqual = baselineSemantics.hash === candidateSemantics.hash &&
      baselineSemantics.rows === candidateSemantics.rows;
    const pass = preTreatmentEqual &&
      baselineIdentity.settings.some(
        // require exact PostgreSQL 17.10 server identity
        function isExactServerVersion(setting) {
          return setting.name === "server_version_num" && setting.setting === "170010";
        },
      ) &&
      canonicalDefinitionMatches &&
      semanticsEqual &&
      planPass &&
      cancellation.pass &&
      runs.every(
        // require all four balanced runs without pooled rescue
        function runPassed(run) {
          return run.pass;
        },
      );

    return {
      build,
      cancellation,
      candidate: {
        canonicalDefinition: PARTIAL_CANDIDATE_CANONICAL_DEFINITION,
        canonicalDefinitionMatches,
        catalog: candidateIndex,
        sql: PARTIAL_CANDIDATE_INDEX_SQL,
      },
      capturedAt: new Date().toISOString(),
      contractVersion: "weather-page-index-matched-crossover/v1",
      fixture: {
        baselineVolume,
        candidateVolume,
        distribution,
        image,
        postgresMajor: 17,
        recoveredSeed,
        recoveredSeedQualification,
        retainedCandidate: options.retain === true,
        serverVersionNum: baselineIdentity.settings.find(
          // select the recorded exact server version setting
          function findServerVersion(setting) {
            return setting.name === "server_version_num";
          },
        )?.setting ?? null,
        syntheticOnly: true,
      },
      frozenSource: {
        commit: migrations.commit,
        migrationHistorySha256: migrations.historySha256,
        tag: migrations.tag,
        tree: migrations.tree,
      },
      pass,
      plan: { explain: candidatePlan, pass: planPass, summary: candidatePlanSummary },
      preTreatment: {
        baseline: baselineIdentity,
        candidate: candidateIdentity,
        equal: preTreatmentEqual,
      },
      protocol: MATCHED_CROSSOVER_PROTOCOL,
      runs,
      semantics: {
        baseline: baselineSemantics,
        candidate: candidateSemantics,
        equal: semanticsEqual,
      },
    };
  } finally {
    await closeCrossoverConnections(baselineConnections).catch(ignoreCleanupError);
    await closeCrossoverConnections(candidateConnections).catch(ignoreCleanupError);

    // stop both measurement containers before any volume handoff
    if (baselineServer !== null) {
      await stopVolumePostgres(baselineServer).catch(ignoreCleanupError);
    }
    // stop the candidate before retaining or removing its volume
    if (candidateServer !== null) {
      await stopVolumePostgres(candidateServer).catch(ignoreCleanupError);
    }
    // stop an interrupted seed before volume cleanup
    if (seedServer !== null) {
      await stopVolumePostgres(seedServer).catch(ignoreCleanupError);
    }
    // preserve a borrowed clean seed for failure recovery and audit
    if (ownsSeedVolume) {
      await removePostgresVolume(seedVolume);
    }
    await removePostgresVolume(baselineVolume);

    // preserve only the explicitly requested stopped candidate volume
    if (options.retain !== true) {
      await removePostgresVolume(candidateVolume);
    }
    await rm(privateRoot, { force: true, recursive: true });
  }
}

// execute the approved B1 final-catalog replacement rehearsal
export async function runB1Rehearsal(options, hooks = {}) {
  const progress = hooks.progress ?? defaultProgress;
  const recoveryAuthorization = await loadB1RecoveryAuthorization(options);
  const privateRoot = await mkdtemp(join(tmpdir(), "weather-page-index-b1-"));
  await chmod(privateRoot, 0o700);
  const baseDistribution = calculateSyntheticDistribution(options.totalRows, options.forecastRows);
  let overlayVolume = null;
  let baselineVolume = null;
  let finalVolume = null;
  let rollbackVolume = null;
  let overlayServer = null;
  let baselineServer = null;
  let finalServer = null;
  let rollbackServer = null;
  let overlayConnections = null;
  let baselineConnections = null;
  let finalConnections = null;
  let rollbackConnections = null;
  let completed = false;

  try {
    const [designSha256, manifestFileSha256, queryCaptureBytes] = await Promise.all([
      hashSourceFile(b1DesignPath),
      hashSourceFile(replacementManifestPath),
      readFile(queryCapturePath, "utf8"),
    ]);
    const queryCaptureSha256 = createHash("sha256").update(queryCaptureBytes).digest("hex");

    // refuse stale protocol or manifest inputs before any disposable mutation
    if (
      designSha256 !== B1_PROTOCOL.sourceDesignSha256 ||
      manifestFileSha256 !== B1_MANIFEST_IDENTITIES.fileSha256 ||
      options.replacementManifestSha256 !== B1_MANIFEST_IDENTITIES.replacementSha256 ||
      queryCaptureSha256 !== B1_PROTOCOL.sourceQueryCaptureSha256
    ) {
      throw new Error("B1 frozen protocol source identity mismatch");
    }

    const migrations = await materializeFrozenMigrations(privateRoot);
    const image = await crossoverPostgresImageIdentity();

    // require the full run to use the exact preflighted PostgreSQL image
    if (!isDeepStrictEqual(image, recoveryAuthorization.document.preflight.image)) {
      throw new Error("B1 recovery PostgreSQL image differs from preflight");
    }
    const queries = extractCapturedQueries(queryCaptureBytes);
    const borrowedSeed = await inspectStoppedCrossoverSeed(options.seedVolume);
    overlayVolume = await createPostgresVolume("wpi-b1-overlay");
    await cloneStoppedPostgresVolume(options.seedVolume, overlayVolume);
    overlayServer = await startVolumePostgres(overlayVolume, "wpi-b1-overlay");
    overlayConnections = await openCrossoverConnections(overlayServer);
    const seedQualification = await qualifyRecoveredCrossoverSeed(
      overlayConnections.ownerClient,
      baseDistribution,
    );
    const catalog = await loadSyntheticCatalog(overlayConnections.ownerClient);
    const overlay = await seedB1QueryOverlay(overlayConnections.ownerClient, catalog);

    // require the prospective overlay to match its frozen declaration
    if (
      overlay.totalRows !== B1_PROTOCOL.overlay.totalRows ||
      overlay.counts.physical !== B1_PROTOCOL.overlay.physicalRows ||
      overlay.counts.forecastObservation !== B1_PROTOCOL.overlay.forecastObservationRows ||
      overlay.counts.tideObservation !== B1_PROTOCOL.overlay.tideObservationRows ||
      overlay.counts.tidePrediction !== B1_PROTOCOL.overlay.tidePredictionRows
    ) {
      throw new Error("B1 representative overlay differs from the frozen protocol");
    }

    await overlayConnections.ownerClient.query("VACUUM (ANALYZE) weather_records");
    await overlayConnections.adminClient.query("CHECKPOINT");
    const overlayQueries = await evaluateDeployedQueries(overlayConnections.apiClient, queries);

    // refuse an empty deployed query shortcut before participant cloning
    if (overlayQueries.some(
      // detect one unrepresented deployed query
      function queryIsEmpty(query) {
        return query.rows < 1 || query.responseBytes < 3;
      },
    )) {
      throw new Error("B1 overlay does not represent every deployed query");
    }

    const overlayData = await weatherRecordDataFingerprint(overlayConnections.ownerClient);
    const overlayCatalog = await fullIndexCatalogSnapshot(overlayConnections.ownerClient);
    const rowShapes = await rowShapeSnapshot(overlayConnections.ownerClient);
    await closeCrossoverConnections(overlayConnections);
    overlayConnections = null;
    await stopVolumePostgres(overlayServer);
    overlayServer = null;
    baselineVolume = await createPostgresVolume("wpi-b1-baseline");
    finalVolume = await createPostgresVolume("wpi-b1-final");
    await cloneStoppedPostgresVolume(overlayVolume, baselineVolume);
    await cloneStoppedPostgresVolume(overlayVolume, finalVolume);
    await removePostgresVolume(overlayVolume);
    overlayVolume = null;
    baselineServer = await startVolumePostgres(baselineVolume, "wpi-b1-baseline");
    finalServer = await startVolumePostgres(finalVolume, "wpi-b1-final");
    baselineConnections = await openCrossoverConnections(baselineServer);
    finalConnections = await openCrossoverConnections(finalServer);
    const baselineOriginalCatalog = await fullIndexCatalogSnapshot(baselineConnections.ownerClient);
    const finalOriginalCatalog = await fullIndexCatalogSnapshot(finalConnections.ownerClient);
    const baselineOriginalData = await weatherRecordDataFingerprint(baselineConnections.ownerClient);
    const finalOriginalData = await weatherRecordDataFingerprint(finalConnections.ownerClient);
    const baselineOriginalQueries = await evaluateDeployedQueries(baselineConnections.apiClient, queries);
    const finalOriginalQueries = await evaluateDeployedQueries(finalConnections.apiClient, queries);
    const preTreatmentQueries = compareDeployedQuerySuites(
      baselineOriginalQueries,
      finalOriginalQueries,
      false,
    );
    const preTreatmentPass = isDeepStrictEqual(baselineOriginalCatalog, finalOriginalCatalog) &&
      baselineOriginalData.sha256 === finalOriginalData.sha256 &&
      preTreatmentQueries.pass;

    // refuse treatment unless both offline clones are exact
    if (!preTreatmentPass) {
      throw new Error("B1 participant fixtures differ before treatment");
    }

    const treatment = await applyB1Treatment(
      finalServer,
      finalConnections,
      finalOriginalCatalog,
      finalOriginalData,
    );
    const maintainedOneX = compareDeployedQuerySuites(
      await evaluateDeployedQueries(baselineConnections.apiClient, queries),
      await evaluateDeployedQueries(finalConnections.apiClient, queries),
    );
    const forecastRunCount = options.forecastRows / forecastRowsPerRun;
    await baselineConnections.ownerClient.query("SET synchronous_commit = off");
    await finalConnections.ownerClient.query("SET synchronous_commit = off");
    await seedForecastRuns(
      baselineConnections.ownerClient,
      catalog.forecast,
      forecastRunCount,
      forecastRunCount * 4,
      progress,
    );
    await seedForecastRuns(
      finalConnections.ownerClient,
      catalog.forecast,
      forecastRunCount,
      forecastRunCount * 4,
      progress,
    );
    await baselineConnections.ownerClient.query("RESET synchronous_commit");
    await finalConnections.ownerClient.query("RESET synchronous_commit");
    const recentFourX = compareDeployedQuerySuites(
      await evaluateDeployedQueries(baselineConnections.apiClient, queries),
      await evaluateDeployedQueries(finalConnections.apiClient, queries),
    );
    const fourXBaselineData = await weatherRecordDataFingerprint(baselineConnections.ownerClient);
    const fourXFinalData = await weatherRecordDataFingerprint(finalConnections.ownerClient);
    await prepareCrossoverForRestart(baselineConnections.ownerClient, baselineConnections.adminClient);
    await prepareCrossoverForRestart(finalConnections.ownerClient, finalConnections.adminClient);
    await closeCrossoverConnections(baselineConnections);
    await closeCrossoverConnections(finalConnections);
    baselineConnections = null;
    finalConnections = null;
    await stopVolumePostgres(baselineServer);
    await stopVolumePostgres(finalServer);
    baselineServer = null;
    finalServer = null;
    baselineServer = await startVolumePostgres(baselineVolume, "wpi-b1-baseline-measure");
    finalServer = await startVolumePostgres(finalVolume, "wpi-b1-final-measure");
    baselineConnections = await openCrossoverConnections(baselineServer);
    finalConnections = await openCrossoverConnections(finalServer);

    // prepare both restarted participants through all exact deployed queries
    for (const query of queries) {
      await baselineConnections.apiClient.query(query.text, query.values);
      await finalConnections.apiClient.query(query.text, query.values);
    }
    await baselineConnections.adminClient.query("CHECKPOINT");
    await finalConnections.adminClient.query("CHECKPOINT");
    await delay(5_000);
    const maintainedFourXBaseline = await evaluateDeployedQueries(baselineConnections.apiClient, queries);
    const maintainedFourXFinal = await evaluateDeployedQueries(finalConnections.apiClient, queries);
    const maintainedFourX = compareDeployedQuerySuites(maintainedFourXBaseline, maintainedFourXFinal);
    const expectedCatalogs = {
      baseline: hashValue(semanticIndexCatalog(await fullIndexCatalogSnapshot(baselineConnections.ownerClient))),
      candidate: hashValue(semanticIndexCatalog(await fullIndexCatalogSnapshot(finalConnections.ownerClient))),
    };
    const runs = await executeB1Runs(
      baselineServer,
      finalServer,
      baselineConnections,
      finalConnections,
      catalog,
      expectedCatalogs,
    );
    const postWriteBaselineQueries = await evaluateDeployedQueries(baselineConnections.apiClient, queries);
    const postWriteFinalQueries = await evaluateDeployedQueries(finalConnections.apiClient, queries);
    const postWriteRecent = compareDeployedQuerySuites(postWriteBaselineQueries, postWriteFinalQueries);
    const postWriteBaselineData = await weatherRecordDataFingerprint(baselineConnections.ownerClient);
    const postWriteFinalData = await weatherRecordDataFingerprint(finalConnections.ownerClient);
    const finalCatalogBeforeHandoff = await fullIndexCatalogSnapshot(finalConnections.ownerClient);
    await closeCrossoverConnections(baselineConnections);
    await closeCrossoverConnections(finalConnections);
    baselineConnections = null;
    finalConnections = null;
    await stopVolumePostgres(baselineServer);
    await stopVolumePostgres(finalServer);
    baselineServer = null;
    finalServer = null;
    rollbackVolume = await createPostgresVolume("wpi-b1-rollback");
    await cloneStoppedPostgresVolume(finalVolume, rollbackVolume);
    rollbackServer = await startVolumePostgres(rollbackVolume, "wpi-b1-rollback");
    rollbackConnections = await openCrossoverConnections(rollbackServer);
    const rollback = await rehearseB1Rollback(
      rollbackServer,
      rollbackConnections,
      baselineOriginalCatalog,
      finalCatalogBeforeHandoff,
      postWriteFinalData,
      postWriteFinalQueries,
      queries,
    );
    await closeCrossoverConnections(rollbackConnections);
    rollbackConnections = null;
    await stopVolumePostgres(rollbackServer);
    rollbackServer = null;
    await removePostgresVolume(rollbackVolume);
    rollbackVolume = null;
    const allRunsPass = runs.every(
      // require all twelve independent write ratios
      function runPassed(run) {
        return run.pass;
      },
    );
    const allQueryStatesPass = maintainedOneX.pass && recentFourX.pass && maintainedFourX.pass && postWriteRecent.pass;
    const dataEqual = fourXBaselineData.sha256 === fourXFinalData.sha256 &&
      postWriteBaselineData.sha256 === postWriteFinalData.sha256;
    const pass = treatment.pass &&
      allRunsPass &&
      allQueryStatesPass &&
      dataEqual &&
      rollback.pass;
    completed = true;

    return {
      capturedAt: new Date().toISOString(),
      contractVersion: B1_REPORT_CONTRACT_VERSION,
      fixture: {
        baselineVolume,
        borrowedSeed,
        distribution: baseDistribution,
        finalVolume,
        image,
        overlay,
        overlayCatalogSha256: hashValue(overlayCatalog),
        overlayData,
        overlayQueries,
        postgresMajor: 17,
        retainedFinal: options.retain === true,
        rowShapes,
        seedQualification,
        serverVersionNum: "170010",
        syntheticOnly: true,
      },
      frozenSource: {
        commit: migrations.commit,
        migrationHistorySha256: migrations.historySha256,
        tag: migrations.tag,
        tree: migrations.tree,
      },
      manifestSha256: options.replacementManifestSha256,
      manifestFileSha256,
      pass,
      preTreatment: {
        baselineCatalog: baselineOriginalCatalog,
        baselineData: baselineOriginalData,
        equal: preTreatmentPass,
        finalCatalog: finalOriginalCatalog,
        finalData: finalOriginalData,
        queries: preTreatmentQueries,
      },
      protocol: B1_PROTOCOL,
      protocolSha256: options.protocolSha256,
      queryCaptureSha256,
      recoveryAuthorization: {
        contractVersion: recoveryAuthorization.document.contractVersion,
        path: recoveryAuthorization.path,
        preflight: recoveryAuthorization.document.preflight,
        sha256: recoveryAuthorization.sha256,
      },
      queryStates: {
        maintainedFourX,
        maintainedOneX,
        postWriteRecent,
        recentFourX,
      },
      replacement: treatment,
      rollback,
      runs,
      scale: {
        baseForecastRows: options.forecastRows,
        fourXForecastRows: options.forecastRows * 4,
      },
      verification: { allQueryStatesPass, allRunsPass, dataEqual },
    };
  } finally {
    await closeCrossoverConnections(overlayConnections).catch(ignoreCleanupError);
    await closeCrossoverConnections(baselineConnections).catch(ignoreCleanupError);
    await closeCrossoverConnections(finalConnections).catch(ignoreCleanupError);
    await closeCrossoverConnections(rollbackConnections).catch(ignoreCleanupError);

    // stop every owned disposable server before volume cleanup
    for (const server of [overlayServer, baselineServer, finalServer, rollbackServer]) {
      // stop only one opened server
      if (server !== null) {
        await stopVolumePostgres(server).catch(ignoreCleanupError);
      }
    }

    await removePostgresVolume(overlayVolume);
    await removePostgresVolume(baselineVolume);
    await removePostgresVolume(rollbackVolume);

    // preserve only the stopped final-catalog copy for HTTP handoff
    if (options.retain !== true || !completed) {
      await removePostgresVolume(finalVolume);
    }
    await rm(privateRoot, { force: true, recursive: true });
  }
}

// select the independently measured worst-ratio stratum
function worstCrossoverStratum(crossover, stratumName) {
  const measured = crossover.runs.map(
    // retain one exact stratum from every balanced run
    function selectRunStratum(run) {
      return run.strata.find(
        // match the frozen stratum identity
        function matchesStratum(stratum) {
          return stratum.stratum === stratumName;
        },
      );
    },
  );

  // refuse incomplete run evidence rather than pooling around it
  if (measured.some(
    // identify an absent run stratum
    function isMissing(stratum) {
      return stratum === undefined;
    },
  )) {
    throw new Error(`matched crossover is missing ${stratumName}`);
  }

  return measured.reduce(
    // retain the highest independently measured candidate ratio
    function selectWorst(worst, stratum) {
      return stratum.p95Ratio > worst.p95Ratio ? stratum : worst;
    },
  );
}

// normalize one raw build into incremental admission peaks
function crossoverBuildPeaks(build) {
  return {
    after: build.after,
    before: build.before,
    elapsedMs: build.elapsedMs,
    finishedAt: build.finishedAt,
    indexBytesHighWater: Number(build.highWater.candidateIndexBytes),
    inodesHighWater: resourceIncrement(build.highWater.inodes, build.before.container.inodes),
    resourceDiagnostics: {
      dataBytesHighWater: resourceIncrement(build.highWater.dataBytes, build.before.container.dataBytes),
      rawHighWater: build.highWater,
    },
    startedAt: build.startedAt,
    tempBytesHighWater: resourceIncrement(build.highWater.tempBytes, build.before.container.tempBytes),
    walGeneratedBytes: Number(build.generatedWalBytes),
    walRetainedBytesHighWater: resourceIncrement(build.highWater.walBytes, build.before.container.walBytes),
  };
}

// normalize one cancelled build into the exact lifecycle peak schema
function crossoverFailedBuild(cancellation) {
  const resourcePeaks = cancellation.pass
    ? {
        indexBytesHighWater: Number(cancellation.highWater.candidateIndexBytes),
        inodesHighWater: resourceIncrement(cancellation.highWater.inodes, cancellation.before.container.inodes),
        tempBytesHighWater: resourceIncrement(cancellation.highWater.tempBytes, cancellation.before.container.tempBytes),
        walGeneratedBytes: Number(cancellation.generatedWalBytes),
        walRetainedBytesHighWater: resourceIncrement(cancellation.highWater.walBytes, cancellation.before.container.walBytes),
      }
    : null;

  return {
    errorCode: cancellation.errorCode,
    kind: "owned-backend-cancellation",
    objectAbsent: cancellation.objectAbsentAfterCleanup,
    reason: cancellation.pass ? null : "owned cancellation did not produce a complete resource peak",
    resourceDiagnostics: cancellation.pass
      ? {
          dataBytesHighWater: resourceIncrement(cancellation.highWater.dataBytes, cancellation.before.container.dataBytes),
          rawHighWater: cancellation.highWater,
        }
      : null,
    resourcePeaks,
  };
}

// derive one lifecycle rehearsal without pooling balanced crossover runs
export function deriveMatchedCrossoverAdmission(crossover, rowShapes, crossoverDigest) {
  // require one complete raw crossover and its separate immutable artifact digest
  if (
    crossover?.contractVersion !== "weather-page-index-matched-crossover/v1" ||
    !Array.isArray(crossover.runs) ||
    crossover.runs.length !== MATCHED_CROSSOVER_PROTOCOL.measurement.runs ||
    !Array.isArray(rowShapes) ||
    rowShapes.length === 0 ||
    !/^[a-f0-9]{64}$/u.test(crossoverDigest)
  ) {
    throw new Error("matched crossover admission inputs are incomplete");
  }

  const physical = worstCrossoverStratum(crossover, "physical-null-50");
  const forecast = worstCrossoverStratum(crossover, "forecast-nonnull-240");
  const mixed = worstCrossoverStratum(crossover, "mixed-forecast240-tide24-24-physical50");
  const build = crossover.build;
  const cancellation = crossover.cancellation;
  const validBuild = crossoverBuildPeaks(build);
  const failedBuild = crossoverFailedBuild(cancellation);
  const allRunsPass = crossover.runs.every(
    // require every frozen run and stratum independently
    function runPassed(run) {
      return run.pass === true && run.strata.every(
        // require every non-pooled stratum result
        function stratumPassed(stratum) {
          return stratum.pass === true;
        },
      );
    },
  );
  const writesPass = allRunsPass && physical.pass === true;
  const sourceIdentity = {
    commit: crossover.frozenSource.commit,
    migrationHistorySha256: crossover.frozenSource.migrationHistorySha256,
    tag: crossover.frozenSource.tag,
    tree: crossover.frozenSource.tree,
  };
  const admission = {
    base: {
      allIndexBytes: Number(build.before.database.all_index_bytes),
      heapBytes: Number(build.before.database.heap_bytes),
      rowShapes,
      rows: Number(build.before.database.rows),
      sampledRowBytes: Number(build.before.database.sampled_row_bytes),
      totalBytes: Number(build.before.database.total_bytes),
    },
    burstReplay: {
      baseline: mixed.measurements.baseline,
      candidate: mixed.measurements.candidate,
      measured: true,
      outcomesEqual: mixed.outcomesEqual,
      p95Ratio: mixed.p95Ratio,
      pass: allRunsPass && mixed.pass === true,
    },
    contractVersion: ADMISSION_CONTRACT_VERSION,
    distribution: crossover.fixture.distribution,
    failedBuild,
    forecastReplay: {
      baseline: forecast.measurements.baseline,
      candidate: forecast.measurements.candidate,
      measured: true,
      outcomesEqual: forecast.outcomesEqual,
      p95Ratio: forecast.p95Ratio,
      pass: allRunsPass && forecast.pass === true,
    },
    index: {
      canonicalDefinition: crossover.candidate.canonicalDefinition,
      canonicalDefinitionMatches: crossover.candidate.canonicalDefinitionMatches,
      lockTimeoutMs: 5_000,
      maintenanceWorkMemBytes: 64 * 1024 * 1024,
      maxParallelMaintenanceWorkers: 0,
      name: CANDIDATE_INDEX_NAME,
      sql: crossover.candidate.sql,
      statementTimeoutMs: 45 * 60 * 1_000,
    },
    ingestionProducer: {
      reportPath: null,
      required: true,
      sha256: null,
      status: "pending-separate-producer",
    },
    postgres: {
      image: crossover.fixture.image,
      major: crossover.fixture.postgresMajor,
      serverVersionNum: crossover.fixture.serverVersionNum,
    },
    sourceIdentity,
    validBuild,
  };

  return {
    admission,
    candidate: crossover.candidate,
    cancellation,
    capturedAt: validBuild.finishedAt,
    contractVersion: REHEARSAL_CONTRACT_VERSION,
    crossoverEvidence: {
      contractVersion: crossover.contractVersion,
      digest: crossoverDigest,
      protocolVersion: crossover.protocol.contractVersion,
    },
    fixture: {
      candidateVolume: crossover.fixture.candidateVolume,
      postgresMajor: crossover.fixture.postgresMajor,
      retained: crossover.fixture.retainedCandidate,
      serverVersionNum: crossover.fixture.serverVersionNum,
      syntheticOnly: crossover.fixture.syntheticOnly,
    },
    frozenSource: sourceIdentity,
    pass: crossover.pass === true && allRunsPass && writesPass,
    plans: { candidate: crossover.plan },
    resources: { build, rowShapes },
    scale: {
      pass: crossover.pass === true,
      qualified: crossover.fixture.distribution.total === defaultFullHeapRows,
      rows: crossover.fixture.distribution.total,
    },
    semantics: crossover.semantics,
    writes: {
      baseline: physical.measurements.baseline,
      candidate: physical.measurements.candidate,
      forecastBaseline: forecast.measurements.baseline,
      forecastCandidate: forecast.measurements.candidate,
      forecastOutcomesEqual: forecast.outcomesEqual,
      forecastP95Ratio: forecast.p95Ratio,
      forecastPass: allRunsPass && forecast.pass === true,
      mixedBaseline: mixed.measurements.baseline,
      mixedCandidate: mixed.measurements.candidate,
      mixedOutcomesEqual: mixed.outcomesEqual,
      mixedP95Ratio: mixed.p95Ratio,
      mixedPass: allRunsPass && mixed.pass === true,
      outcomesEqual: physical.outcomesEqual,
      pass: writesPass,
      p95Ratio: physical.p95Ratio,
      threshold: 1.10,
    },
  };
}

// normalize one build or cancellation into incremental resource peaks
function b1ResourcePeaks(receipt) {
  return {
    indexBytesHighWater: Number(receipt.highWater.candidateIndexBytes),
    inodesHighWater: resourceIncrement(receipt.highWater.inodes, receipt.before.container.inodes),
    tempBytesHighWater: resourceIncrement(receipt.highWater.tempBytes, receipt.before.container.tempBytes),
    walGeneratedBytes: Number(receipt.generatedWalBytes),
    walRetainedBytesHighWater: resourceIncrement(receipt.highWater.walBytes, receipt.before.container.walBytes),
  };
}

// calculate one build receipt duration without losing cancelled builds
function b1ReceiptElapsedMs(receipt) {
  const measured = Number(receipt.elapsedMs);

  // prefer one directly measured positive duration
  if (Number.isFinite(measured) && measured > 0) {
    return measured;
  }
  const timestampDuration = Date.parse(receipt.finishedAt) - Date.parse(receipt.startedAt);

  // reject an incomplete fallback timing identity
  if (!Number.isFinite(timestampDuration) || timestampDuration <= 0) {
    throw new Error("B1 build receipt duration is incomplete");
  }
  return timestampDuration;
}

// combine two replacement build windows conservatively component by component
function combineB1ResourcePeaks(targetValid, targetCancelled, predecessorValid, predecessorCancelled) {
  const fields = [
    "indexBytesHighWater",
    "inodesHighWater",
    "tempBytesHighWater",
    "walGeneratedBytes",
    "walRetainedBytesHighWater",
  ];
  return Object.fromEntries(fields.map(
    // sum each phase's worse valid or cancelled observation
    function combineField(field) {
      return [
        field,
        Math.max(targetValid[field], targetCancelled[field]) +
          Math.max(predecessorValid[field], predecessorCancelled[field]),
      ];
    },
  ));
}

// select one exact catalog tuple by name
function b1Index(catalog, name) {
  return catalog.find(
    // locate the requested immutable catalog identity
    function indexMatches(index) {
      return index.name === name;
    },
  );
}

// derive the lifecycle-compatible B1 admission report
export function deriveB1AdmissionReport(raw, rawDigest) {
  // require one complete immutable raw B1 report
  if (
    raw?.contractVersion !== B1_REPORT_CONTRACT_VERSION ||
    !/^[a-f0-9]{64}$/u.test(rawDigest) ||
    raw.manifestSha256 !== B1_MANIFEST_IDENTITIES.replacementSha256 ||
    raw.manifestFileSha256 !== B1_MANIFEST_IDENTITIES.fileSha256 ||
    raw.protocolSha256 !== B1_RECOVERY_IDENTITIES.originalProtocolSha256 ||
    raw.recoveryAuthorization?.contractVersion !== "weather-page-index-b1-recovery-authorization/v1" ||
    !/^[a-f0-9]{64}$/u.test(raw.recoveryAuthorization?.sha256 ?? "")
  ) {
    throw new Error("B1 admission inputs are incomplete");
  }

  const targetValidReceipt = raw.replacement.targetValid;
  const targetCancelledReceipt = raw.replacement.targetCancelled;
  const predecessorValidReceipt = raw.rollback.predecessorValid;
  const predecessorCancelledReceipt = raw.rollback.predecessorCancelled;
  const replacementResources = {
    predecessorCancelled: b1ResourcePeaks(predecessorCancelledReceipt),
    predecessorValid: b1ResourcePeaks(predecessorValidReceipt),
    targetCancelled: b1ResourcePeaks(targetCancelledReceipt),
    targetValid: b1ResourcePeaks(targetValidReceipt),
  };
  const combinedPeaks = combineB1ResourcePeaks(
    replacementResources.targetValid,
    replacementResources.targetCancelled,
    replacementResources.predecessorValid,
    replacementResources.predecessorCancelled,
  );
  const physical = worstCrossoverStratum(raw, "physical-null-50");
  const forecast = worstCrossoverStratum(raw, "forecast-nonnull-240");
  const mixed = worstCrossoverStratum(raw, "mixed-forecast240-tide24-24-physical50");
  const allRunsPass = raw.runs.every(
    // require all twelve non-pooled ratios
    function runPassed(run) {
      return run.pass === true;
    },
  );
  const originalCatalog = raw.preTreatment.baselineCatalog;
  const finalCatalog = raw.replacement.finalCatalog;
  const restoredCatalog = raw.rollback.restoredCatalog;
  const originalSemanticCatalog = semanticIndexCatalog(originalCatalog);
  const finalSemanticCatalog = semanticIndexCatalog(finalCatalog);
  const restoredSemanticCatalog = semanticIndexCatalog(restoredCatalog);
  const protectedSemantic = B1_PROTOCOL.catalog.protected.map(
    // bind each protected catalog tuple into one manifest identity
    function protectedIdentity(name) {
      return b1Index(originalSemanticCatalog, name);
    },
  );
  const replacement = {
    actualCoveringCatalogSha256: hashValue(b1Index(originalSemanticCatalog, B1_PROTOCOL.catalog.covering)),
    actualPredecessorCatalogSha256: hashValue(b1Index(originalSemanticCatalog, PREDECESSOR_INDEX_NAME)),
    actualProtectedCatalogSha256: hashValue(protectedSemantic),
    actualTargetCatalogSha256: hashValue(b1Index(finalSemanticCatalog, CANDIDATE_INDEX_NAME)),
    covering: B1_MANIFEST_IDENTITIES.coveringSemanticSha256,
    finalCatalogFullSha256: hashValue(finalCatalog),
    finalCatalogSha256: hashValue(finalSemanticCatalog),
    manifestSha256: raw.manifestSha256,
    manifestFileSha256: raw.manifestFileSha256,
    originalCatalogFullSha256: hashValue(originalCatalog),
    originalCatalogSha256: hashValue(originalSemanticCatalog),
    predecessor: B1_MANIFEST_IDENTITIES.predecessorSemanticSha256,
    protected: B1_MANIFEST_IDENTITIES.protectedSemanticSha256,
    restoredCatalogFullSha256: hashValue(restoredCatalog),
    restoredCatalogSha256: hashValue(restoredSemanticCatalog),
    target: B1_MANIFEST_IDENTITIES.targetSemanticSha256,
  };
  const actualDistribution = {
    forecast: raw.fixture.distribution.forecast + raw.fixture.overlay.counts.forecastObservation,
    modelAndReanalysis: raw.fixture.distribution.modelAndReanalysis,
    physical: raw.fixture.distribution.physical + raw.fixture.overlay.counts.physical,
    tide: raw.fixture.distribution.tide +
      raw.fixture.overlay.counts.tideObservation +
      raw.fixture.overlay.counts.tidePrediction,
    total: Number(raw.replacement.baseResources.rows),
  };
  const predecessorCancelled = replacementResources.predecessorCancelled;
  const validElapsedMs = Math.max(
    b1ReceiptElapsedMs(targetValidReceipt),
    b1ReceiptElapsedMs(targetCancelledReceipt),
  ) + Math.max(
    b1ReceiptElapsedMs(predecessorValidReceipt),
    b1ReceiptElapsedMs(predecessorCancelledReceipt),
  );
  const admission = {
    base: {
      allIndexBytes: Number(raw.replacement.baseResources.all_index_bytes),
      heapBytes: Number(raw.replacement.baseResources.heap_bytes),
      rowShapes: raw.fixture.rowShapes,
      rows: Number(raw.replacement.baseResources.rows),
      sampledRowBytes: Number(raw.replacement.baseResources.sampled_row_bytes),
      totalBytes: Number(raw.replacement.baseResources.total_bytes),
    },
    burstReplay: {
      baseline: mixed.measurements.baseline,
      candidate: mixed.measurements.candidate,
      measured: true,
      outcomesEqual: mixed.outcomesEqual,
      p95Ratio: mixed.p95Ratio,
      pass: allRunsPass && mixed.pass === true,
    },
    contractVersion: ADMISSION_CONTRACT_VERSION,
    distribution: actualDistribution,
    failedBuild: {
      errorCode: predecessorCancelledReceipt.errorCode,
      kind: "owned-backend-cancellation",
      objectAbsent: predecessorCancelledReceipt.objectAbsentAfterCleanup,
      reason: predecessorCancelledReceipt.pass ? null : "predecessor cancellation failed",
      resourceDiagnostics: predecessorCancelledReceipt.highWater,
      resourcePeaks: predecessorCancelled,
    },
    forecastReplay: {
      baseline: forecast.measurements.baseline,
      candidate: forecast.measurements.candidate,
      measured: true,
      outcomesEqual: forecast.outcomesEqual,
      p95Ratio: forecast.p95Ratio,
      pass: allRunsPass && forecast.pass === true,
    },
    index: {
      canonicalDefinition: PARTIAL_CANDIDATE_CANONICAL_DEFINITION,
      canonicalDefinitionMatches: true,
      lockTimeoutMs: 5_000,
      maintenanceWorkMemBytes: 64 * 1024 * 1024,
      maxParallelMaintenanceWorkers: 0,
      name: CANDIDATE_INDEX_NAME,
      sql: PARTIAL_CANDIDATE_INDEX_SQL,
      statementTimeoutMs: 45 * 60 * 1_000,
    },
    ingestionProducer: {
      reportPath: null,
      required: true,
      sha256: null,
      status: "pending-separate-producer",
    },
    postgres: {
      image: raw.fixture.image,
      major: raw.fixture.postgresMajor,
      serverVersionNum: raw.fixture.serverVersionNum,
    },
    replacement,
    replacementResources,
    sourceIdentity: raw.frozenSource,
    validBuild: {
      ...combinedPeaks,
      admissionDurationCeilingSeconds: 5_400,
      elapsedMs: validElapsedMs,
      finishedAt: predecessorValidReceipt.finishedAt,
      startedAt: targetCancelledReceipt.startedAt,
    },
  };
  const writesPass = allRunsPass && physical.pass === true;
  return {
    admission,
    candidate: raw.replacement,
    capturedAt: predecessorValidReceipt.finishedAt,
    contractVersion: REHEARSAL_CONTRACT_VERSION,
    frozenSource: raw.frozenSource,
    pass: raw.pass === true && writesPass,
    rawB1Evidence: {
      contractVersion: raw.contractVersion,
      digest: rawDigest,
      protocolSha256: raw.protocolSha256,
      protocolVersion: raw.protocol.contractVersion,
      recoveryAuthorizationSha256: raw.recoveryAuthorization.sha256,
    },
    replacement,
    resources: {
      replacementReceipts: {
        predecessorCancelled: predecessorCancelledReceipt,
        predecessorValid: predecessorValidReceipt,
        targetCancelled: targetCancelledReceipt,
        targetValid: targetValidReceipt,
      },
      replacementResources,
    },
    scale: raw.scale,
    semantics: raw.queryStates,
    writes: {
      baseline: physical.measurements.baseline,
      candidate: physical.measurements.candidate,
      forecastBaseline: forecast.measurements.baseline,
      forecastCandidate: forecast.measurements.candidate,
      forecastOutcomesEqual: forecast.outcomesEqual,
      forecastP95Ratio: forecast.p95Ratio,
      forecastPass: allRunsPass && forecast.pass === true,
      mixedBaseline: mixed.measurements.baseline,
      mixedCandidate: mixed.measurements.candidate,
      mixedOutcomesEqual: mixed.outcomesEqual,
      mixedP95Ratio: mixed.p95Ratio,
      mixedPass: allRunsPass && mixed.pass === true,
      outcomesEqual: physical.outcomesEqual,
      pass: writesPass,
      p95Ratio: physical.p95Ratio,
      threshold: 1.10,
    },
  };
}

// measure retained synthetic row shapes through one clean temporary start
export async function captureRetainedCrossoverRowShapes(crossover) {
  const volume = crossover?.fixture?.candidateVolume;

  // require the explicit stopped retained fixture identity
  if (
    crossover?.contractVersion !== "weather-page-index-matched-crossover/v1" ||
    crossover?.fixture?.retainedCandidate !== true ||
    typeof volume !== "string" ||
    volume.length === 0
  ) {
    throw new Error("retained crossover candidate volume is missing");
  }

  const server = await startVolumePostgres(volume, "wpi-crossover-row-shapes");
  const pool = createTestPool(server, "weather_test", "weather_owner", "owner-test");

  try {
    return await rowShapeSnapshot(pool);
  } finally {
    await pool.end().catch(ignoreCleanupError);
    await stopVolumePostgres(server).catch(ignoreCleanupError);
  }
}

// capture one bounded exact PostgreSQL 17.10 resource-sampler preflight
export async function captureB1ResourcePreflight() {
  const volume = await createPostgresVolume("wpi-b1-resource-preflight");
  let server = null;
  let pool = null;

  try {
    server = await startVolumePostgres(volume, "wpi-b1-resource-preflight");
    pool = createTestPool(server);
    const version = await pool.query("SELECT current_setting('server_version_num') AS server_version_num");
    const serverVersionNum = version.rows[0].server_version_num;
    const containerResource = await containerResourceSnapshot(server);
    const image = await crossoverPostgresImageIdentity();
    const passed = serverVersionNum === "170010" &&
      containerResource.availableBytes > 0 &&
      containerResource.availableInodes > 0;

    // refuse a partial exact-version preflight
    if (!passed) {
      throw new Error("B1 exact PostgreSQL resource preflight failed");
    }
    return {
      capturedAt: new Date().toISOString(),
      containerResource,
      contractVersion: "weather-page-index-b1-resource-preflight/v1",
      image,
      passed,
      serverVersionNum,
    };
  } finally {
    // close only an opened preflight pool
    if (pool !== null) {
      await pool.end().catch(ignoreCleanupError);
    }

    // stop only an opened exact-version preflight server
    if (server !== null) {
      await stopVolumePostgres(server).catch(ignoreCleanupError);
    }
    await removePostgresVolume(volume);
  }
}

// execute the disposable PostgreSQL 17 rehearsal
export async function runRehearsal(options, hooks = {}) {
  const distribution = calculateSyntheticDistribution(options.totalRows ?? null, options.forecastRows);
  const candidateSql = candidateSqlFor(options.candidate ?? "full");
  const progress = hooks.progress ?? defaultProgress;
  const privateRoot = await mkdtemp(join(tmpdir(), "weather-page-index-rehearsal-"));
  await chmod(privateRoot, 0o700);
  let server = null;
  let postgresPool = null;
  let ownerPool = null;
  let apiPool = null;
  let retainedFixture = null;

  try {
    const migrations = await materializeFrozenMigrations(privateRoot);
    const capturedQuery = extractCapturedForecastQuery(await readFile(queryCapturePath, "utf8"));
    server = await startPostgres(17, "page-index-rehearsal");
    postgresPool = createTestPool(server);
    await prepareRuntimeRoles(postgresPool);
    ownerPool = createTestPool(server, "weather_test", "weather_owner", "owner-test");
    await runMigrations(ownerPool, migrations.directory, {
      lockTimeoutMs: 5_000,
      statementTimeoutMs: 120_000,
    });
    apiPool = createTestPool(server, "weather_test", "weather_api", "api-test");
    const ownerClient = await ownerPool.connect();
    const apiClient = await apiPool.connect();
    const monitorClient = await ownerPool.connect();

    try {
      await ownerClient.query("SET statement_timeout = '45min'");
      await ownerClient.query("SET synchronous_commit = off");
      const catalog = await bootstrapSyntheticCatalog(ownerClient);
      const forecastRunCount = options.forecastRows / forecastRowsPerRun;
      const fourXRunCount = forecastRunCount * 4;
      const mixedBatchCount = Math.min(writeBatchCount, forecastRunCount - 1);
      const scaleQualified = options.forecastRows >= fullScaleForecastRows;
      const forecastWriteBatchCount = scaleQualified ? writeBatchCount : 0;
      const baselineForecastWriteStart = forecastRunCount - mixedBatchCount - forecastWriteBatchCount;
      const candidateForecastWriteStart = fourXRunCount - mixedBatchCount - forecastWriteBatchCount;
      await seedForecastRuns(ownerClient, catalog.forecast, 1, baselineForecastWriteStart, progress);
      await seedSyntheticPadding(ownerClient, catalog, distribution, progress);
      await ownerClient.query("RESET synchronous_commit");
      await ownerClient.query("VACUUM (ANALYZE) weather_records");
      const ledgerBefore = await migrationLedger(ownerClient);
      const catalogBefore = await catalogSnapshot(ownerClient);
      const baselineMaintainedPlan = await explainCapturedQuery(apiClient, capturedQuery);
      const baselineWrites = await measureWriteBatches(ownerClient, catalog.physical[0], "baseline", 0);
      const baselineForecastWrites = forecastWriteBatchCount > 0
        ? await measureForecastWriteBatches(
            ownerClient,
            catalog.forecast,
            "baseline",
            baselineForecastWriteStart,
            forecastWriteBatchCount,
          )
        : unmeasuredForecastWriteReplay();
      const baselineMixedWrites = await measureMixedIngestionBatches(
        ownerClient,
        catalog,
        "baseline",
        forecastRunCount - mixedBatchCount,
        mixedBatchCount,
      );
      await seedForecastRuns(ownerClient, catalog.forecast, 0, 1, progress);
      const recentSemantics = await querySemantics(apiClient, capturedQuery);
      const baselineRecentPlan = await explainCapturedQuery(apiClient, capturedQuery);
      const failure = await rehearseTransactionFailure(ownerClient);
      const build = await buildCandidateIndex(server, ownerClient, monitorClient, candidateSql);
      const candidateRecentPlan = await explainCapturedQuery(apiClient, capturedQuery);
      const candidateRecentSemantics = await querySemantics(apiClient, capturedQuery);
      await ownerClient.query("VACUUM (ANALYZE) weather_records");
      const candidateMaintainedPlan = await explainCapturedQuery(apiClient, capturedQuery);
      const candidateMaintainedSemantics = await querySemantics(apiClient, capturedQuery);
      const candidateWrites = await measureWriteBatches(ownerClient, catalog.physical[0], "candidate", 1);
      const candidateForecastWrites = forecastWriteBatchCount > 0
        ? await measureForecastWriteBatches(
            ownerClient,
            catalog.forecast,
            "candidate",
            candidateForecastWriteStart,
            forecastWriteBatchCount,
          )
        : unmeasuredForecastWriteReplay();
      const candidateMixedWrites = await measureMixedIngestionBatches(
        ownerClient,
        catalog,
        "candidate",
        fourXRunCount - mixedBatchCount,
        mixedBatchCount,
      );
      const candidateAfterWritePlan = await explainCapturedQuery(apiClient, capturedQuery);
      await ownerClient.query("SET synchronous_commit = off");
      await seedForecastRuns(
        ownerClient,
        catalog.forecast,
        forecastRunCount,
        candidateForecastWriteStart,
        progress,
      );
      await ownerClient.query("RESET synchronous_commit");
      const fourXPlan = await explainCapturedQuery(apiClient, capturedQuery);
      const fourXSemantics = await querySemantics(apiClient, capturedQuery);
      const cancellation = await rehearseOwnedCancellation(server, ownerClient, monitorClient, options.candidate ?? "full");
      const catalogAfter = await catalogSnapshot(ownerClient);
      const ledgerAfter = await migrationLedger(ownerClient);
      const finalResources = await databaseResourceSnapshot(ownerClient);
      const rowShapes = await rowShapeSnapshot(ownerClient);
      const candidateCatalog = catalogAfter.find(findCandidateIndex) ?? null;
      const candidateRecentSummary = summarizePlan(candidateRecentPlan);
      const fourXSummary = summarizePlan(fourXPlan);
      const writeRatio = candidateWrites.p95Ms / baselineWrites.p95Ms;
      const writeOutcomesEqual = candidateWrites.persistedOutcomeSha256 === baselineWrites.persistedOutcomeSha256;
      const forecastWriteRatio = scaleQualified
        ? candidateForecastWrites.p95Ms / baselineForecastWrites.p95Ms
        : 1;
      const forecastOutcomesEqual =
        baselineForecastWrites.outcomeSha256 === candidateForecastWrites.outcomeSha256 &&
        baselineForecastWrites.persistedOutcomeSha256 === candidateForecastWrites.persistedOutcomeSha256;
      const mixedWriteRatio = candidateMixedWrites.p95Ms / baselineMixedWrites.p95Ms;
      const mixedOutcomesEqual =
        baselineMixedWrites.outcomeSha256 === candidateMixedWrites.outcomeSha256 &&
        baselineMixedWrites.persistedOutcomeSha256 === candidateMixedWrites.persistedOutcomeSha256;
      const semanticsEqual =
        recentSemantics.hash === candidateRecentSemantics.hash &&
        recentSemantics.hash === candidateMaintainedSemantics.hash &&
        recentSemantics.hash === fourXSemantics.hash &&
        recentSemantics.rows === forecastRowsPerRun;
      const catalogsEqual = hashValue(catalogBefore) === hashValue(withoutCandidate(catalogAfter));
      const ledgerEqual = ledgerBefore.sha256 === ledgerAfter.sha256 && ledgerBefore.entries === 18;
      const scalePass = !scaleQualified || (
        candidateRecentSummary.indexNames.includes(CANDIDATE_INDEX_NAME) &&
        fourXSummary.indexNames.includes(CANDIDATE_INDEX_NAME) &&
        !candidateRecentSummary.weatherRecordSequentialScan &&
        !fourXSummary.weatherRecordSequentialScan &&
        candidateRecentSummary.candidateProductSelectionRows <= 5 &&
        fourXSummary.candidateProductSelectionRows <= 5 &&
        candidateRecentSummary.candidateResultRows <= forecastRowsPerRun &&
        fourXSummary.candidateResultRows <= forecastRowsPerRun
      );
      const writePass = writeRatio <= 1.10 && writeOutcomesEqual;
      const mixedWritePass = mixedWriteRatio <= 1.10 && mixedOutcomesEqual;
      const forecastWritePass = !scaleQualified || (forecastWriteRatio <= 1.10 && forecastOutcomesEqual);
      const canonicalDefinition = candidateCanonicalDefinitionFor(options.candidate ?? "full");
      const canonicalDefinitionMatches = candidateCatalog?.definition === canonicalDefinition;
      const pass =
        failure.pass &&
        semanticsEqual &&
        catalogsEqual &&
        ledgerEqual &&
        candidateCatalog?.valid === true &&
        candidateCatalog?.ready === true &&
        canonicalDefinitionMatches &&
        writePass &&
        forecastWritePass &&
        (!scaleQualified || mixedWritePass) &&
        scalePass &&
        (!scaleQualified || cancellation.pass);

      // retain only this disposable fixture when explicitly requested
      if (options.retain === true) {
        retainedFixture = {
          database: "weather_test",
          host: server.host,
          name: server.name,
          password: server.password,
          port: server.port,
          user: server.user,
        };
      }

      const frozenSource = {
        commit: migrations.commit,
        hashes: migrations.hashes,
        historySha256: migrations.historySha256,
        tag: migrations.tag,
        tree: migrations.tree,
      };
      const admission = {
        base: {
          allIndexBytes: Number(build.before.database.all_index_bytes),
          heapBytes: Number(build.before.database.heap_bytes),
          rowShapes,
          rows: Number(build.before.database.rows),
          sampledRowBytes: Number(build.before.database.sampled_row_bytes),
          totalBytes: Number(build.before.database.total_bytes),
        },
        contractVersion: ADMISSION_CONTRACT_VERSION,
        distribution,
        burstReplay: {
          baseline: baselineMixedWrites,
          candidate: candidateMixedWrites,
          measured: scaleQualified,
          outcomesEqual: mixedOutcomesEqual,
          p95Ratio: mixedWriteRatio,
          pass: mixedWritePass,
        },
        forecastReplay: {
          baseline: baselineForecastWrites,
          candidate: candidateForecastWrites,
          measured: scaleQualified,
          outcomesEqual: forecastOutcomesEqual,
          p95Ratio: forecastWriteRatio,
          pass: forecastWritePass,
        },
        failedBuild: {
          errorCode: cancellation.errorCode,
          kind: "owned-backend-cancellation",
          objectAbsent: cancellation.objectAbsentAfterCleanup,
          reason: cancellation.pass
            ? null
            : "owned cancellation did not produce a complete resource peak",
          resourceDiagnostics: cancellation.pass
            ? {
                dataBytesHighWater: resourceIncrement(
                  cancellation.highWater.dataBytes,
                  cancellation.before.container.dataBytes,
                ),
                rawHighWater: cancellation.highWater,
              }
            : null,
          resourcePeaks: cancellation.pass
            ? {
                indexBytesHighWater: cancellation.highWater.candidateIndexBytes,
                inodesHighWater: resourceIncrement(
                  cancellation.highWater.inodes,
                  cancellation.before.container.inodes,
                ),
                tempBytesHighWater: resourceIncrement(
                  cancellation.highWater.tempBytes,
                  cancellation.before.container.tempBytes,
                ),
                walGeneratedBytes: cancellation.generatedWalBytes,
                walRetainedBytesHighWater: resourceIncrement(
                  cancellation.highWater.walBytes,
                  cancellation.before.container.walBytes,
                ),
              }
            : null,
        },
        index: {
          canonicalDefinition,
          canonicalDefinitionMatches,
          lockTimeoutMs: 5_000,
          maintenanceWorkMemBytes: 64 * 1024 * 1024,
          maxParallelMaintenanceWorkers: 0,
          name: CANDIDATE_INDEX_NAME,
          sql: candidateSql,
          statementTimeoutMs: 45 * 60 * 1_000,
        },
        ingestionProducer: {
          reportPath: null,
          required: true,
          sha256: null,
          status: "pending-separate-producer",
        },
        postgres: { major: 17 },
        sourceIdentity: {
          commit: migrations.commit,
          migrationHistorySha256: migrations.historySha256,
          tag: migrations.tag,
          tree: migrations.tree,
        },
        validBuild: {
          after: build.after,
          before: build.before,
          elapsedMs: build.elapsedMs,
          finishedAt: build.finishedAt,
          indexBytesHighWater: build.highWater.candidateIndexBytes,
          inodesHighWater: resourceIncrement(
            build.highWater.inodes,
            build.before.container.inodes,
          ),
          resourceDiagnostics: {
            dataBytesHighWater: resourceIncrement(
              build.highWater.dataBytes,
              build.before.container.dataBytes,
            ),
            rawHighWater: build.highWater,
          },
          startedAt: build.startedAt,
          tempBytesHighWater: resourceIncrement(
            build.highWater.tempBytes,
            build.before.container.tempBytes,
          ),
          walGeneratedBytes: build.generatedWalBytes,
          walRetainedBytesHighWater: resourceIncrement(
            build.highWater.walBytes,
            build.before.container.walBytes,
          ),
        },
      };

      return {
        admission,
        candidate: {
          catalog: candidateCatalog,
          canonicalDefinition,
          canonicalDefinitionMatches,
          sql: candidateSql,
        },
        cancellation,
        catalogPreserved: catalogsEqual,
        capturedAt: build.finishedAt,
        contractVersion: REHEARSAL_CONTRACT_VERSION,
        distribution,
        failure,
        fixture: {
          postgresMajor: 17,
          retained: retainedFixture,
          syntheticOnly: true,
        },
        frozenSource,
        ledgerPreserved: ledgerEqual,
        pass,
        plans: {
          baselineMaintained: { explain: baselineMaintainedPlan, summary: summarizePlan(baselineMaintainedPlan) },
          baselineRecentWrites: { explain: baselineRecentPlan, summary: summarizePlan(baselineRecentPlan) },
          candidateAfterWrite: { explain: candidateAfterWritePlan, summary: summarizePlan(candidateAfterWritePlan) },
          candidateMaintained: { explain: candidateMaintainedPlan, summary: summarizePlan(candidateMaintainedPlan) },
          candidateRecentWrites: { explain: candidateRecentPlan, summary: candidateRecentSummary },
          fourXForecast: { explain: fourXPlan, summary: fourXSummary },
        },
        resources: {
          build,
          final: finalResources,
          rowShapes,
        },
        scale: {
          baseForecastRows: options.forecastRows,
          fourXForecastRows: options.forecastRows * 4,
          pass: scalePass,
          qualified: scaleQualified,
        },
        semantics: {
          candidateMaintained: candidateMaintainedSemantics,
          candidateRecent: candidateRecentSemantics,
          equal: semanticsEqual,
          fourX: fourXSemantics,
          recentBaseline: recentSemantics,
        },
        writes: {
          baseline: baselineWrites,
          candidate: candidateWrites,
          forecastBaseline: baselineForecastWrites,
          forecastCandidate: candidateForecastWrites,
          forecastOutcomesEqual,
          forecastP95Ratio: forecastWriteRatio,
          forecastPass: forecastWritePass,
          mixedBaseline: baselineMixedWrites,
          mixedCandidate: candidateMixedWrites,
          mixedOutcomesEqual,
          mixedP95Ratio: mixedWriteRatio,
          mixedPass: mixedWritePass,
          outcomesEqual: writeOutcomesEqual,
          pass: writePass,
          p95Ratio: writeRatio,
          threshold: 1.10,
        },
      };
    } finally {
      monitorClient.release();
      apiClient.release();
      ownerClient.release();
    }
  } finally {
    // close every pool before container cleanup
    if (apiPool !== null) {
      await apiPool.end().catch(ignoreCleanupError);
    }
    // close the owner pool before container cleanup
    if (ownerPool !== null) {
      await ownerPool.end().catch(ignoreCleanupError);
    }
    // close the bootstrap pool before container cleanup
    if (postgresPool !== null) {
      await postgresPool.end().catch(ignoreCleanupError);
    }
    // delete the disposable container unless retained explicitly
    if (server !== null && options.retain !== true) {
      await stopPostgres(server);
    }
    await rm(privateRoot, { force: true, recursive: true });
  }
}

// find the exact candidate catalog row
function findCandidateIndex(index) {
  return index.name === CANDIDATE_INDEX_NAME;
}

// ignore best-effort cleanup failures
function ignoreCleanupError() {
  return undefined;
}

// prepare one private report parent directory
async function preparePrivateOutput(path) {
  const directory = dirname(path);

  // create only the requested report directory
  await mkdir(directory, { mode: 0o700, recursive: true });
  const metadata = await lstat(directory);

  // reject links public modes and foreign ownership
  if (
    metadata.isSymbolicLink() ||
    !metadata.isDirectory() ||
    (metadata.mode & 0o077) !== 0 ||
    (typeof process.getuid === "function" && metadata.uid !== process.getuid())
  ) {
    throw new Error("rehearsal output directory must be private owned and link-free");
  }
}

// write one no-overwrite private report
async function writePrivateReport(path, report) {
  await preparePrivateOutput(path);
  const bytes = `${JSON.stringify(report, null, 2)}\n`;
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  await writeFile(path, bytes, {
    flag: "wx",
    mode: 0o600,
  });
  await writeFile(`${path}.sha256`, `${sha256}  ${path.split("/").at(-1)}\n`, {
    flag: "wx",
    mode: 0o600,
  });
  return sha256;
}

// reject an existing output before starting a retained fixture
async function assertNewOutputPath(path) {
  await preparePrivateOutput(path);

  // reject either pre-existing report artifact
  for (const candidate of [path, `${path}.sha256`]) {
    try {
      await lstat(candidate);
    } catch (error) {
      // continue only when this artifact is absent
      if (error?.code === "ENOENT") {
        continue;
      }
      throw error;
    }
    throw new Error("rehearsal output already exists");
  }
}

// execute the disposable rehearsal CLI
export async function runCli(argumentsList = process.argv.slice(2)) {
  const options = parseRehearsalArguments(argumentsList);

  // require a private evidence destination
  if (options.output === null || options.output === resolve("")) {
    throw new Error("--output <private-report.json> is required");
  }

  await assertNewOutputPath(options.output);
  const admissionOutput = `${options.output}.admission.json`;

  // reserve both immutable B1 artifacts before expensive work
  if (options.b1Replacement) {
    await assertNewOutputPath(admissionOutput);
  }

  const report = options.b1Replacement
    ? await runB1Rehearsal(options)
    : options.matchedCrossover
      ? await runMatchedCrossover(options)
      : await runRehearsal(options);
  let reportSha256;
  let admissionSha256 = null;

  try {
    reportSha256 = await writePrivateReport(options.output, report);
    // bind lifecycle admission to the immutable raw B1 report
    if (options.b1Replacement) {
      admissionSha256 = await writePrivateReport(
        admissionOutput,
        deriveB1AdmissionReport(report, reportSha256),
      );
    }
  } catch (error) {
    // avoid retaining a crossover volume without its private descriptor
    if (options.matchedCrossover && report.fixture.retainedCandidate === true) {
      await removePostgresVolume(report.fixture.candidateVolume);
    }
    // avoid retaining a B1 final copy without both private descriptors
    if (options.b1Replacement && report.fixture.retainedFinal === true) {
      await removePostgresVolume(report.fixture.finalVolume);
    }
    // avoid retaining a fixture without its private descriptor
    if (!options.matchedCrossover && report.fixture.retained !== null) {
      await stopPostgres({ name: report.fixture.retained.name });
    }
    throw error;
  }
  process.stdout.write(`${options.output}\nsha256=${reportSha256}\n`);
  // report the separately hash-bound lifecycle artifact
  if (admissionSha256 !== null) {
    process.stdout.write(`${admissionOutput}\nadmissionSha256=${admissionSha256}\n`);
  }
  return report.pass ? 0 : 1;
}

// run only when invoked as a script
if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    process.exitCode = await runCli();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 2;
  }
}
