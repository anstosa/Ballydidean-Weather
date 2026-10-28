import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  B1_MANIFEST_IDENTITIES,
  B1_PROTOCOL,
  B1_RECOVERY_IDENTITIES,
  B1_REPORT_CONTRACT_VERSION,
  CANDIDATE_INDEX_NAME,
  FROZEN_COMMIT,
  FROZEN_TREE,
  PARTIAL_CANDIDATE_INDEX_SQL,
  PARTIAL_CANDIDATE_CANONICAL_DEFINITION,
  calculateSyntheticDistribution,
  candidateCanonicalDefinitionFor,
  candidateSqlFor,
  captureB1ResourcePreflight,
  compareDeployedQuerySuites,
  deriveB1AdmissionReport,
  deriveMatchedCrossoverAdmission,
  evaluateB1CatalogTransition,
  extractCapturedForecastQuery,
  extractCapturedQueries,
  loadB1RecoveryAuthorization,
  nearestRankP95,
  normalizeAdmissionResourcePeaks,
  parseRehearsalArguments,
  parseContainerResourceOutput,
  parseFilesystemFloorOutput,
  runRehearsal,
  summarizePlan,
  validateB1RecoveryAuthorizationDocument,
} from "./rehearse-weather-page-indexes.mjs";
import { validateIndexManifest, validateRehearsalReport } from "./weather-page-indexes.mjs";

// build one synthetic measured crossover stratum
function crossoverStratum(stratum, p95Ratio) {
  const durationsMs = Array.from({ length: 200 }, () => 10);
  const outcomes = Array.from(
    { length: 200 },
    // reproduce one exact scheduled batch outcome
    function batchOutcome() {
      return { forecast: 240, physical: 50, tide_observation: 24, tide_prediction: 24 };
    },
  );
  const outcomeSha256 = createHash("sha256").update(JSON.stringify(outcomes)).digest("hex");
  const baseline = {
    batchCount: 200,
    batchMix: { forecast: 240, physical: 50, tideObservation: 24, tidePrediction: 24 },
    countOutcomeSha256: outcomeSha256,
    durationsMs,
    generatedWalBytes: 100,
    outcomeSha256,
    outcomes,
    p95Ms: 10,
    persistedOutcomeSha256: "b".repeat(64),
    relationBytesDelta: 10,
  };
  const candidate = {
    ...baseline,
    durationsMs: durationsMs.map(
      // scale every observed candidate duration equally
      function scaleDuration(duration) {
        return duration * p95Ratio;
      },
    ),
    p95Ms: 10 * p95Ratio,
  };
  return {
    countOutcomesEqual: true,
    measurements: { baseline, candidate },
    outcomesEqual: true,
    pass: p95Ratio <= 1.10,
    persistedOutcomesEqual: true,
    p95Ratio,
    stratum,
    warmups: { baseline, candidate },
  };
}

// build one independently judged balanced crossover run
function crossoverRun(run, physicalRatio, forecastRatio, mixedRatio) {
  return {
    backgroundAfter: {},
    backgroundBefore: {},
    backgroundInvalid: false,
    order: run % 2 === 1 ? "baseline-candidate" : "candidate-baseline",
    pass: true,
    run,
    strata: [
      crossoverStratum("physical-null-50", physicalRatio),
      crossoverStratum("forecast-nonnull-240", forecastRatio),
      crossoverStratum("mixed-forecast240-tide24-24-physical50", mixedRatio),
    ],
  };
}

// build one raw full-scale crossover report
function rawCrossoverReport() {
  const before = {
    container: { dataBytes: 1_000, inodes: 100, tempBytes: 10, walBytes: 200 },
    database: {
      all_index_bytes: 300,
      heap_bytes: 8_000,
      rows: 12_513_302,
      sampled_row_bytes: 703,
      total_bytes: 13_000,
    },
  };
  const build = {
    after: before,
    before,
    elapsedMs: 1_000,
    finishedAt: "2026-10-28T18:00:00.000Z",
    generatedWalBytes: 50,
    highWater: { candidateIndexBytes: 400, dataBytes: 1_100, inodes: 102, tempBytes: 12, walBytes: 230 },
    startedAt: "2026-10-28T17:59:00.000Z",
  };
  const cancellation = {
    before,
    errorCode: "57014",
    generatedWalBytes: 20,
    highWater: { candidateIndexBytes: 40, dataBytes: 1_050, inodes: 101, tempBytes: 11, walBytes: 210 },
    objectAbsentAfterCleanup: true,
    pass: true,
  };
  return {
    build,
    cancellation,
    candidate: {
      canonicalDefinition: PARTIAL_CANDIDATE_CANONICAL_DEFINITION,
      canonicalDefinitionMatches: true,
      sql: PARTIAL_CANDIDATE_INDEX_SQL,
    },
    contractVersion: "weather-page-index-matched-crossover/v1",
    fixture: {
      candidateVolume: "wpi-candidate-volume",
      distribution: calculateSyntheticDistribution(12_513_302, 347_520),
      image: { id: `sha256:${"c".repeat(64)}`, reference: "postgres:17.10-bookworm" },
      postgresMajor: 17,
      retainedCandidate: true,
      serverVersionNum: "170010",
      syntheticOnly: true,
    },
    frozenSource: {
      commit: FROZEN_COMMIT,
      migrationHistorySha256: "d".repeat(64),
      tag: "2026.10.09-1",
      tree: FROZEN_TREE,
    },
    pass: true,
    plan: { pass: true },
    protocol: { contractVersion: "weather-page-index-matched-crossover-protocol/v3" },
    runs: [
      crossoverRun(1, 1.01, 1.02, 1.03),
      crossoverRun(2, 1.05, 1.01, 1.04),
      crossoverRun(3, 1.02, 1.06, 1.09),
      crossoverRun(4, 1.03, 1.04, 1.05),
    ],
    semantics: { equal: true },
  };
}

// build one observed B1 build or cancellation receipt
function b1ResourceReceipt(indexBytes, walGeneratedBytes, elapsedMs, cancelled = false) {
  return {
    before: { container: { dataBytes: 1_000, inodes: 100, tempBytes: 10, walBytes: 200 } },
    ...(cancelled ? {} : { elapsedMs }),
    errorCode: cancelled ? "57014" : null,
    finishedAt: "2026-10-28T22:00:00.000Z",
    generatedWalBytes: walGeneratedBytes,
    highWater: {
      candidateIndexBytes: indexBytes,
      dataBytes: 1_100,
      inodes: 102,
      tempBytes: 14,
      walBytes: 240,
    },
    objectAbsentAfterCleanup: cancelled,
    pass: true,
    startedAt: "2026-10-28T21:59:00.000Z",
  };
}

// build one complete raw B1 admission fixture
function rawB1Report() {
  // create one semantic catalog tuple
  function index(name, definition, oid) {
    return { definition, name, oid, ready: true, valid: true };
  }

  const predecessor = index("weather_records_source_valid_idx", "source-valid", 5);
  const originalCatalog = [
    index("weather_records_pkey", "pkey", 1),
    index("weather_records_identity_key", "identity", 2),
    index("weather_records_kind_valid_idx", "kind", 3),
    index("weather_records_current_idx", "current", 4),
    predecessor,
  ];
  const finalCatalog = [
    ...originalCatalog.filter(
      // remove only the predecessor from the final catalog
      function removePredecessor(item) {
        return item.name !== predecessor.name;
      },
    ),
    index(CANDIDATE_INDEX_NAME, PARTIAL_CANDIDATE_CANONICAL_DEFINITION, 6),
  ];
  const restoredCatalog = [
    ...finalCatalog.filter(
      // remove only the target from the restored catalog
      function removeTarget(item) {
        return item.name !== CANDIDATE_INDEX_NAME;
      },
    ),
    index(predecessor.name, predecessor.definition, 7),
  ];
  return {
    contractVersion: B1_REPORT_CONTRACT_VERSION,
    fixture: {
      distribution: calculateSyntheticDistribution(12_513_302, 347_520),
      image: { id: `sha256:${"c".repeat(64)}`, reference: "postgres:17.10-bookworm" },
      overlay: {
        counts: { forecastObservation: 6, physical: 292, tideObservation: 264, tidePrediction: 264 },
      },
      postgresMajor: 17,
      rowShapes: [{ row_bytes: 703, source_kind: "physical_sensor" }],
      serverVersionNum: "170010",
    },
    frozenSource: {
      commit: FROZEN_COMMIT,
      migrationHistorySha256: "d".repeat(64),
      tag: "2026.10.09-1",
      tree: FROZEN_TREE,
    },
    manifestFileSha256: B1_MANIFEST_IDENTITIES.fileSha256,
    manifestSha256: B1_MANIFEST_IDENTITIES.replacementSha256,
    pass: true,
    preTreatment: { baselineCatalog: originalCatalog },
    protocol: B1_PROTOCOL,
    protocolSha256: B1_RECOVERY_IDENTITIES.originalProtocolSha256,
    queryStates: { maintainedOneX: { pass: true } },
    recoveryAuthorization: {
      contractVersion: "weather-page-index-b1-recovery-authorization/v1",
      sha256: "e".repeat(64),
    },
    replacement: {
      baseResources: {
        all_index_bytes: 1_000,
        heap_bytes: 8_000,
        rows: 12_514_128,
        sampled_row_bytes: 703,
        total_bytes: 13_000,
      },
      finalCatalog,
      targetCancelled: b1ResourceReceipt(20, 30, 800, true),
      targetValid: b1ResourceReceipt(400, 50, 1_000),
    },
    rollback: {
      predecessorCancelled: b1ResourceReceipt(30, 40, 900, true),
      predecessorValid: b1ResourceReceipt(500, 60, 1_200),
      restoredCatalog,
    },
    runs: [
      crossoverRun(1, 1.01, 1.02, 1.03),
      crossoverRun(2, 1.05, 1.01, 1.04),
      crossoverRun(3, 1.02, 1.06, 1.09),
      crossoverRun(4, 1.03, 1.04, 1.05),
    ],
    scale: { baseForecastRows: 347_520, fourXForecastRows: 1_390_080 },
  };
}

// build one closed recovery authorization document
function recoveryAuthorizationDocument(currentSources) {
  return {
    authorization: {
      candidate: B1_PROTOCOL.candidate,
      measurementProtocol: B1_PROTOCOL,
      originalMeasurementProtocolSha256: B1_RECOVERY_IDENTITIES.originalProtocolSha256,
      retainAllTimingsAndFailures: true,
      soleInstrumentationRecoveryRun: true,
    },
    capturedAt: "2026-10-28T22:30:00.000Z",
    contractVersion: "weather-page-index-b1-recovery-authorization/v1",
    identities: {
      criticReviewSha256: B1_RECOVERY_IDENTITIES.criticReviewSha256,
      designSha256: B1_PROTOCOL.sourceDesignSha256,
      helperSha256: B1_RECOVERY_IDENTITIES.helperSha256,
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
    },
    preflight: {
      capturedAt: "2026-10-28T22:29:00.000Z",
      containerResource: {
        availableBytes: 300_000_000_000,
        availableInodes: 50_000_000,
        dataBytes: 40_000_000,
        inodes: 1_400,
        tempBytes: 0,
        walBytes: 16_000_000,
      },
      contractVersion: "weather-page-index-b1-resource-preflight/v1",
      image: {
        digests: [`postgres@sha256:${"c".repeat(64)}`],
        id: `sha256:${"d".repeat(64)}`,
        reference: "postgres:17.10-bookworm",
      },
      passed: true,
      serverVersionNum: "170010",
    },
  };
}

// parse only bounded explicit rehearsal sizes
test("rehearsal arguments bind tiny current-scale and full-heap modes", () => {
  assert.deepEqual(parseRehearsalArguments([]), {
    b1Replacement: false,
    candidate: "full",
    forecastRows: 2_400,
    matchedCrossover: false,
    output: null,
    protocolSha256: null,
    recoveryAuthorization: null,
    recoveryAuthorizationSha256: null,
    replacementManifestSha256: null,
    retain: false,
    seedVolume: null,
    totalRows: null,
  });
  assert.deepEqual(parseRehearsalArguments(["--rows12513302", "--retain"]), {
    b1Replacement: false,
    candidate: "full",
    forecastRows: 347_520,
    matchedCrossover: false,
    output: null,
    protocolSha256: null,
    recoveryAuthorization: null,
    recoveryAuthorizationSha256: null,
    replacementManifestSha256: null,
    retain: true,
    seedVolume: null,
    totalRows: 12_513_302,
  });
  // reject partial forecast products
  assert.throws(() => parseRehearsalArguments(["--forecast-rows", "481"]), /multiple of 240/u);
  // reject unknown production-affecting controls
  assert.throws(() => parseRehearsalArguments(["--host", "blueberry"]), /unknown argument/u);
  assert.equal(
    parseRehearsalArguments(["--candidate", "partial-product-run"]).candidate,
    "partial-product-run",
  );
  assert.equal(candidateSqlFor("partial-product-run"), PARTIAL_CANDIDATE_INDEX_SQL);
  assert.equal(
    candidateCanonicalDefinitionFor("partial-product-run"),
    PARTIAL_CANDIDATE_CANONICAL_DEFINITION,
  );
  // reject unreviewed candidate shapes
  assert.throws(() => parseRehearsalArguments(["--candidate", "source-only"]), /candidate must be/u);
  assert.equal(parseRehearsalArguments([
    "--candidate", "partial-product-run", "--matched-crossover", "--rows12513302",
  ]).matchedCrossover, true);
  assert.equal(parseRehearsalArguments([
    "--candidate", "partial-product-run", "--matched-crossover", "--rows12513302",
    "--seed-volume", "weather-wpi-crossover-candidate-3343083-847d8e84",
  ]).seedVolume, "weather-wpi-crossover-candidate-3343083-847d8e84");
  // reject production or arbitrary volume recovery
  assert.throws(
    () => parseRehearsalArguments([
      "--candidate", "partial-product-run", "--matched-crossover", "--rows12513302",
      "--seed-volume", "weather-production-data",
    ]),
    /exact disposable crossover volume/u,
  );
  const b1 = parseRehearsalArguments([
    "--b1-replacement", "--rows12513302",
    "--seed-volume", "weather-wpi-crossover-candidate-3343083-847d8e84",
    "--replacement-manifest-sha256", B1_MANIFEST_IDENTITIES.replacementSha256,
    "--protocol-sha256", B1_RECOVERY_IDENTITIES.originalProtocolSha256,
    "--recovery-authorization", "/tmp/b1-recovery-authorization.json",
    "--recovery-authorization-sha256", "e".repeat(64),
  ]);
  assert.equal(b1.b1Replacement, true);
  assert.equal(b1.candidate, "partial-product-run");
  assert.equal(b1.retain, true);
  assert.equal(B1_REPORT_CONTRACT_VERSION, "weather-page-index-b1-rehearsal/v1");
  assert.equal(B1_PROTOCOL.measurement.runs, 4);
  assert.equal(B1_PROTOCOL.measurement.measuredBatchesPerStratum, 200);
  assert.equal(B1_PROTOCOL.manifest, B1_MANIFEST_IDENTITIES);
  assert.equal(b1.protocolSha256, B1_RECOVERY_IDENTITIES.originalProtocolSha256);
  assert.equal(b1.recoveryAuthorization, "/tmp/b1-recovery-authorization.json");
  // reject an arbitrary protocol digest without reviewed recovery provenance
  assert.throws(() => parseRehearsalArguments([
    "--b1-replacement", "--rows12513302",
    "--seed-volume", "weather-wpi-crossover-candidate-3343083-847d8e84",
    "--replacement-manifest-sha256", B1_MANIFEST_IDENTITIES.replacementSha256,
    "--protocol-sha256", "e".repeat(64),
    "--recovery-authorization", "/tmp/b1-recovery-authorization.json",
    "--recovery-authorization-sha256", "f".repeat(64),
  ]), /approved clean 12\.5M-row borrowed seed/u);
  // reject superseded or scope-confused manifest digests
  assert.throws(() => parseRehearsalArguments([
    "--b1-replacement", "--rows12513302",
    "--seed-volume", "weather-wpi-crossover-candidate-3343083-847d8e84",
    "--replacement-manifest-sha256", "5d188345008ed0f263e39b9aa1594a85baf72b69b0195187064e8a9914476164",
    "--protocol-sha256", B1_RECOVERY_IDENTITIES.originalProtocolSha256,
    "--recovery-authorization", "/tmp/b1-recovery-authorization.json",
    "--recovery-authorization-sha256", "e".repeat(64),
  ]), /approved clean 12\.5M-row borrowed seed/u);
});

// preserve the reviewed full-heap distribution exactly
test("synthetic distribution keeps the fixed forecast corpus and exact total", () => {
  const distribution = calculateSyntheticDistribution(12_513_302, 347_520);

  assert.equal(distribution.total, 12_513_302);
  assert.equal(distribution.forecast, 347_520);
  assert.equal(distribution.physical, Math.floor(12_513_302 * 0.932));
  assert.equal(distribution.tide, Math.floor(12_513_302 * 0.0355));
  assert.equal(
    distribution.physical + distribution.tide + distribution.forecast + distribution.modelAndReanalysis,
    distribution.total,
  );
});

// select only the deployed source-level MAX query
test("query capture parser rejects substitutes and selects exact MAX shape", () => {
  const content = `QUERY_CAPTURE=${JSON.stringify([
    { text: "SELECT 1", values: [] },
    {
      text: "SELECT candidate.* WHERE candidate.source_kind = 'forecast' AND candidate.product_run_at = (SELECT MAX(product.product_run_at))",
      values: ["ballydidean"],
    },
  ])}`;
  const query = extractCapturedForecastQuery(content);

  assert.match(query.text, /MAX\(product\.product_run_at\)/u);
  assert.deepEqual(query.values, ["ballydidean"]);
  assert.throws(() => extractCapturedForecastQuery("QUERY_CAPTURE=[]"), /MAX query is missing/u);
});

// bind B1 to all six exact deployed query shapes
test("B1 query capture requires all six deployed SQL statements", () => {
  const queries = [
    { text: "SELECT candidate.* MAX(product.product_run_at) candidate.source_kind = 'forecast'", values: ["forecast"] },
    { text: "SELECT pressure_before.pressure_hpa ORDER BY candidate.valid_at DESC", values: ["current"] },
    { text: "WITH nearest_source AS (SELECT 1) SELECT SUM(wr.precipitation_mm)", values: ["rain"] },
    { text: "SELECT context_run.product_run_at HAVING COUNT(*) = 6", values: ["observations"] },
    { text: "SELECT wr.water_level_m FROM weather_records wr", values: ["tides"] },
    { text: "SELECT candidate.* WHERE s.source_kind = $2 LIMIT $5", values: ["history"] },
  ];
  const parsed = extractCapturedQueries(`QUERY_CAPTURE=${JSON.stringify(queries)}`);

  assert.equal(parsed.length, 6);
  assert.deepEqual(parsed.map(
    // retain the frozen positional query identity
    function selectQueryId(query) {
      return query.id;
    },
  ), ["forecast", "current", "daily-precipitation", "forecast-observations", "tides", "history"]);
  // reject a missing deployed statement
  assert.throws(
    () => extractCapturedQueries(`QUERY_CAPTURE=${JSON.stringify(queries.slice(0, 5))}`),
    /six deployed queries/u,
  );
});

// preserve every protected index across replacement and compensation
test("B1 catalog transition removes only source-valid and restores it semantically", () => {
  // build one closed synthetic catalog tuple
  const index = (name, definition, oid) => ({ definition, name, oid, ready: true, valid: true });
  const original = [
    index("weather_records_pkey", "pkey", 1),
    index("weather_records_identity_key", "identity", 2),
    index("weather_records_kind_valid_idx", "kind", 3),
    index("weather_records_current_idx", "current", 4),
    index("weather_records_source_valid_idx", "source-valid", 5),
  ];
  const final = [
    ...original.filter(
      // omit only the approved predecessor
      function retainProtected(item) {
        return item.name !== "weather_records_source_valid_idx";
      },
    ),
    index(CANDIDATE_INDEX_NAME, PARTIAL_CANDIDATE_CANONICAL_DEFINITION, 6),
  ];
  const restored = [
    ...final.filter(
      // remove only the operation-created target
      function removeTarget(item) {
        return item.name !== CANDIDATE_INDEX_NAME;
      },
    ),
    index("weather_records_source_valid_idx", "source-valid", 7),
  ];
  const result = evaluateB1CatalogTransition(original, final, restored);

  assert.equal(result.finalPass, true);
  assert.equal(result.restoredPass, true);
  assert.deepEqual(result.changedFinalNames, [CANDIDATE_INDEX_NAME, "weather_records_source_valid_idx"]);
});

// separate clone equality from final-catalog plan admission
test("B1 pre-treatment comparison preserves semantics without applying the target-only MAX bound", () => {
  const observation = {
    hash: "a".repeat(64),
    id: "forecast",
    plan: {
      candidateProductSelectionRows: 0,
      weatherRecordScanRows: 347_766,
      weatherRecordSequentialScan: false,
    },
    responseBytes: 286_929,
    rows: 240,
  };

  assert.equal(compareDeployedQuerySuites([observation], [observation], false).pass, true);
  assert.equal(compareDeployedQuerySuites([observation], [observation], true).pass, false);
});

// calculate write p95 using nearest rank
test("write p95 uses nearest rank without interpolation", () => {
  assert.equal(nearestRankP95([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20]), 19);
  // reject missing evidence
  assert.throws(() => nearestRankP95([]), /at least one/u);
});

// expose candidate access and reject heap scans in plan summaries
test("plan summary records candidate rows buffers and heap scans", () => {
  const summary = summarizePlan({
    "Execution Time": 1.5,
    "Planning Time": 0.5,
    Plan: {
      "Node Type": "Nested Loop",
      "Shared Hit Blocks": 12,
      "Shared Read Blocks": 3,
      Plans: [
        {
          "Actual Loops": 1,
          "Actual Rows": 1,
          "Index Name": CANDIDATE_INDEX_NAME,
          "Index Cond": "((source_id = s.id) AND (product_run_at IS NOT NULL))",
          "Node Type": "Index Only Scan",
          "Rows Removed by Filter": 0,
        },
        {
          "Node Type": "Seq Scan",
          "Relation Name": "weather_records",
        },
      ],
    },
  });

  assert.equal(summary.candidateExaminedRows, 1);
  assert.equal(summary.candidateProductSelectionRows, 1);
  assert.equal(summary.candidateResultRows, 0);
  assert.deepEqual(summary.indexNames, [CANDIDATE_INDEX_NAME]);
  assert.equal(summary.weatherRecordSequentialScan, true);
  assert.equal(summary.sharedHitBlocks, 12);

  const partialSummary = summarizePlan({
    Plan: {
      "Node Type": "Result",
      Plans: [{
        "Actual Loops": 1,
        "Actual Rows": 1,
        "Index Cond": "(source_id = s.id)",
        "Index Name": CANDIDATE_INDEX_NAME,
        "Node Type": "Index Only Scan",
        "Scan Direction": "Backward",
      }],
    },
  });
  assert.equal(partialSummary.candidateProductSelectionRows, 1);
  assert.equal(partialSummary.candidateResultRows, 0);
});

// convert raw container totals into admission increments
test("admission resource normalization subtracts just-before-build totals", () => {
  const normalized = normalizeAdmissionResourcePeaks({
    admission: {
      failedBuild: { resourcePeaks: { indexBytesHighWater: 999 } },
      validBuild: { finishedAt: "2026-10-28T18:00:00.000Z" },
    },
    cancellation: {
      before: { container: { dataBytes: 500, inodes: 20, tempBytes: 1, walBytes: 100 } },
      generatedWalBytes: 7,
      highWater: { candidateIndexBytes: 11, dataBytes: 530, inodes: 22, tempBytes: 4, walBytes: 115 },
    },
    resources: {
      build: {
        before: { container: { dataBytes: 1_000, inodes: 30, tempBytes: 2, walBytes: 200 } },
        highWater: { candidateIndexBytes: 50, dataBytes: 1_080, inodes: 31, tempBytes: 2, walBytes: 225 },
      },
    },
  });

  assert.equal(normalized.capturedAt, "2026-10-28T18:00:00.000Z");
  assert.equal(normalized.admission.validBuild.indexBytesHighWater, 50);
  assert.equal(normalized.admission.validBuild.inodesHighWater, 1);
  assert.equal(normalized.admission.validBuild.walRetainedBytesHighWater, 25);
  assert.deepEqual(normalized.admission.failedBuild.resourcePeaks, {
    indexBytesHighWater: 11,
    inodesHighWater: 2,
    tempBytesHighWater: 3,
    walGeneratedBytes: 7,
    walRetainedBytesHighWater: 15,
  });
});

// require both byte and inode floor witnesses
test("filesystem floor parser rejects the missing-inode shape from the failed B1 run", () => {
  assert.deepEqual(parseFilesystemFloorOutput("324872167424 52078738\n"), {
    availableBytes: 324_872_167_424,
    availableInodes: 52_078_738,
  });
  assert.throws(
    () => parseFilesystemFloorOutput("324872167424\n"),
    /filesystem floor observation is incomplete/u,
  );
});

// require every container resource counter before lifecycle evidence is accepted
test("container resource parser requires six finite safe nonnegative integers", () => {
  assert.deepEqual(
    parseContainerResourceOutput("15283974913 1073754112 0 1464 309880655872 52078738\n"),
    {
      availableBytes: 309_880_655_872,
      availableInodes: 52_078_738,
      dataBytes: 15_283_974_913,
      inodes: 1_464,
      tempBytes: 0,
      walBytes: 1_073_754_112,
    },
  );
  // reject the exact five-field failure shape
  assert.throws(
    () => parseContainerResourceOutput("15283974913 1073754112 0 1464 309880655872\n"),
    /container resource observation is incomplete/u,
  );
  // reject unsafe or nonfinite counters
  assert.throws(
    () => parseContainerResourceOutput("1 2 3 4 5 NaN\n"),
    /container resource observation is incomplete/u,
  );
});

// bind recovery authorization to exact sources protocol history and real preflight
test("B1 recovery authorization rejects stale source or incomplete resource identities", () => {
  const currentSources = {
    criticReviewSha256: B1_RECOVERY_IDENTITIES.criticReviewSha256,
    helperSha256: B1_RECOVERY_IDENTITIES.helperSha256,
    rehearsalSourceSha256: "a".repeat(64),
    rehearsalTestSha256: "b".repeat(64),
  };
  const document = recoveryAuthorizationDocument(currentSources);

  assert.equal(validateB1RecoveryAuthorizationDocument(document, currentSources), document);
  assert.throws(
    () => validateB1RecoveryAuthorizationDocument(document, {
      ...currentSources,
      rehearsalSourceSha256: "c".repeat(64),
    }),
    /stale or incomplete/u,
  );
  const incomplete = structuredClone(document);
  delete incomplete.preflight.containerResource.availableInodes;
  assert.throws(
    () => validateB1RecoveryAuthorizationDocument(incomplete, currentSources),
    /stale or incomplete/u,
  );
});

// bind the CLI loader to private immutable authorization bytes and current sources
test("B1 recovery authorization loader rejects a caller-supplied digest substitution", async () => {
  const directory = await mkdtemp(join(tmpdir(), "weather-b1-authorization-test-"));
  const path = join(directory, "authorization.json");
  const currentSources = {
    criticReviewSha256: B1_RECOVERY_IDENTITIES.criticReviewSha256,
    helperSha256: B1_RECOVERY_IDENTITIES.helperSha256,
    rehearsalSourceSha256: createHash("sha256").update(
      readFileSync("scripts/rehearse-weather-page-indexes.mjs"),
    ).digest("hex"),
    rehearsalTestSha256: createHash("sha256").update(
      readFileSync("scripts/rehearse-weather-page-indexes.test.mjs"),
    ).digest("hex"),
  };
  const bytes = Buffer.from(`${JSON.stringify(recoveryAuthorizationDocument(currentSources), null, 2)}\n`);
  await writeFile(path, bytes, { mode: 0o600 });

  // always remove the private test authorization
  try {
    await assert.rejects(
      loadB1RecoveryAuthorization({
        recoveryAuthorization: path,
        recoveryAuthorizationSha256: "f".repeat(64),
      }),
      /digest mismatch/u,
    );
  } finally {
    await rm(directory, { force: true, recursive: true });
  }
});

// bind lifecycle admission to every independently passing crossover run
test("matched crossover admission selects worst run ratios without pooling", () => {
  const report = deriveMatchedCrossoverAdmission(
    rawCrossoverReport(),
    [{ row_bytes: 703, source_kind: "physical_sensor" }],
    "e".repeat(64),
  );

  assert.equal(report.contractVersion, "weather-page-index-rehearsal/v1");
  assert.equal(report.admission.contractVersion, "weather-page-index-rehearsal-admission/v1");
  assert.equal(report.capturedAt, report.admission.validBuild.finishedAt);
  assert.equal(report.pass, true);
  assert.equal(report.writes.p95Ratio, 1.05);
  assert.equal(report.writes.forecastP95Ratio, 1.06);
  assert.equal(report.admission.burstReplay.p95Ratio, 1.09);
  assert.equal(report.admission.base.rows, 12_513_302);
  assert.equal(report.admission.postgres.serverVersionNum, "170010");
  assert.equal(report.crossoverEvidence.digest, "e".repeat(64));
  assert.deepEqual(Object.keys(report.admission.failedBuild.resourcePeaks).sort(), [
    "indexBytesHighWater",
    "inodesHighWater",
    "tempBytesHighWater",
    "walGeneratedBytes",
    "walRetainedBytesHighWater",
  ]);
});

// bind B1 admission to the final manifest and four component envelopes
test("B1 admission uses dependency-complete manifest identities and semantic catalog restoration", () => {
  const report = deriveB1AdmissionReport(rawB1Report(), "f".repeat(64));
  const manifest = validateIndexManifest(JSON.parse(readFileSync("scripts/weather-page-indexes.json", "utf8")));
  const replacement = report.admission.replacement;
  const resources = report.admission.replacementResources;

  assert.equal(replacement.target, B1_MANIFEST_IDENTITIES.targetSemanticSha256);
  assert.equal(replacement.predecessor, B1_MANIFEST_IDENTITIES.predecessorSemanticSha256);
  assert.equal(replacement.covering, B1_MANIFEST_IDENTITIES.coveringSemanticSha256);
  assert.equal(replacement.protected, B1_MANIFEST_IDENTITIES.protectedSemanticSha256);
  assert.notEqual(replacement.originalCatalogSha256, replacement.finalCatalogSha256);
  assert.equal(replacement.restoredCatalogSha256, replacement.originalCatalogSha256);
  assert.deepEqual(Object.keys(resources).sort(), [
    "predecessorCancelled",
    "predecessorValid",
    "targetCancelled",
    "targetValid",
  ]);
  assert.deepEqual(Object.keys(resources.targetValid).sort(), [
    "indexBytesHighWater",
    "inodesHighWater",
    "tempBytesHighWater",
    "walGeneratedBytes",
    "walRetainedBytesHighWater",
  ]);
  assert.equal(report.admission.validBuild.indexBytesHighWater, 900);
  assert.equal(report.admission.validBuild.elapsedMs, 120_000);
  assert.equal(report.admission.failedBuild.errorCode, "57014");
  assert.equal(report.pass, true);
  assert.equal(validateRehearsalReport(report, manifest), report);
});

// run the exact PostgreSQL 17.10 six-field sampler only in its explicit lane
test("B1 resource preflight captures byte and inode witnesses from exact PostgreSQL 17.10", {
  skip: process.env.WEATHER_RUN_DB_PAGE_REHEARSAL !== "1",
  timeout: 60_000,
}, async () => {
  const preflight = await captureB1ResourcePreflight();

  assert.equal(preflight.contractVersion, "weather-page-index-b1-resource-preflight/v1");
  assert.equal(preflight.serverVersionNum, "170010");
  assert.equal(preflight.image.reference, "postgres:17.10-bookworm");
  assert.equal(preflight.passed, true);
  assert.equal(preflight.containerResource.availableBytes > 0, true);
  assert.equal(preflight.containerResource.availableInodes > 0, true);
  assert.deepEqual(Object.keys(preflight.containerResource).sort(), [
    "availableBytes",
    "availableInodes",
    "dataBytes",
    "inodes",
    "tempBytes",
    "walBytes",
  ]);
});

// run a real tiny PostgreSQL 17 rehearsal only in its explicit lane
test("tiny PostgreSQL 17 rehearsal preserves semantics and catalogs", {
  skip: process.env.WEATHER_RUN_DB_PAGE_REHEARSAL !== "1",
  timeout: 180_000,
}, async () => {
  const report = await runRehearsal({
    candidate: "full",
    forecastRows: 480,
    retain: false,
    totalRows: null,
  }, {
    // suppress bounded fixture progress in unit output
    progress() {},
  });

  assert.equal(report.fixture.postgresMajor, 17);
  assert.equal(report.fixture.syntheticOnly, true);
  assert.equal(report.failure.pass, true);
  assert.equal(report.admission.contractVersion, "weather-page-index-rehearsal-admission/v1");
  assert.equal(report.admission.ingestionProducer.status, "pending-separate-producer");
  assert.equal(report.admission.validBuild.walGeneratedBytes >= 0, true);
  assert.equal(report.capturedAt, report.admission.validBuild.finishedAt);
  assert.equal(report.writes.mixedOutcomesEqual, true);
  assert.deepEqual(report.writes.mixedBaseline.batchMix, {
    forecast: 240,
    physical: 50,
    tideObservation: 24,
    tidePrediction: 24,
  });
  assert.equal(report.catalogPreserved, true);
  assert.equal(report.ledgerPreserved, true);
  assert.equal(report.semantics.equal, true);
  assert.equal(report.semantics.recentBaseline.rows, 240);
  assert.equal(typeof report.writes.pass, "boolean");
  assert.equal(report.scale.qualified, false);
  const containerResource = report.resources.build.before.container;
  assert.deepEqual(Object.keys(containerResource).sort(), [
    "availableBytes",
    "availableInodes",
    "dataBytes",
    "inodes",
    "tempBytes",
    "walBytes",
  ]);
  assert.equal(Object.values(containerResource).every(
    // require every real PostgreSQL container counter
    function validContainerResource(value) {
      return Number.isSafeInteger(value) && value >= 0;
    },
  ), true);
  assert.equal(containerResource.availableBytes > 0, true);
  assert.equal(containerResource.availableInodes > 0, true);
});
