import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, mkdir, mkdtemp, readFile, readdir, symlink, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { Readable } from "node:stream";
import test from "node:test";
import { gzipSync } from "node:zlib";

import { RAIN_COLLECTION_STATIONS } from "@weather/domain";

import { buildAdjustmentEvaluationPackage } from
  "../../../deploy/scripts/adjustment-evaluation-package.mjs";

import {
  composeVerifiedPerformancePackagePairs,
  loadVerifiedPerformancePackages,
  loadTemperaturePerformanceData,
  normalizeVerifiedPerformanceTimestamps,
  requirePrivatePerformanceOutput,
  runForecastAdjustmentPerformanceCli,
} from "../dist/forecast-adjustment-performance-cli.js";
import { createRainAdjustmentRun } from "../dist/rain-adjustment.js";

const HASH = "a".repeat(64);
const SOURCE_REVISION = "b".repeat(64);
const NOW = new Date("2026-10-07T12:00:00.000Z");
const WIND_PAIRS = [
  ["windGustMps", "001-024", 12],
  ["windGustMps", "025-048", 36],
  ["windGustMps", "073-096", 84],
  ["windGustMps", "097-120", 108],
  ["windGustMps", "121-144", 132],
  ["windGustMps", "145-168", 156],
  ["windSpeedMps", "001-024", 12],
  ["windSpeedMps", "025-048", 36],
  ["windSpeedMps", "049-072", 60],
  ["windSpeedMps", "073-096", 84],
  ["windSpeedMps", "097-120", 108],
  ["windSpeedMps", "121-144", 132],
  ["windSpeedMps", "145-168", 156],
];

// create owned fixture output under the same bounded research root
async function privateTestDirectory(prefix) {
  const root = resolve(import.meta.dirname, "../../../.weather-data");
  await mkdir(root, { recursive: true, mode: 0o700 });
  await chmod(root, 0o700);
  return await mkdtemp(join(root, prefix));
}

// hash exact fixture bytes
function fixtureSha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// build the smallest real adjustment package accepted by the paired loader
async function buildEmptyAdjustmentPackage(root, packageRoot, localDate) {
  const migrationRoot = resolve(import.meta.dirname, "../../../packages/database/migrations");
  const migrationNames = (await readdir(migrationRoot))
    // retain the ordered database ledger only
    .filter((name) => name.endsWith(".sql"))
    .sort();
  const migrationChecksums = await Promise.all(migrationNames.map(
    // bind every migration fixture byte exactly
    async (name) => fixtureSha256(await readFile(join(migrationRoot, name))),
  ));
  const migrationHistorySha256 = fixtureSha256(Buffer.from(migrationNames.map(
    // serialize the ledger like the package contract
    (name, index) => `${name}:${migrationChecksums[index]}`,
  ).join("\n")));
  const snapshotPath = join(root, "empty-edge-snapshot.json");
  await writeFile(snapshotPath, `${JSON.stringify({
    contractVersion: "forecast-adjustment-evidence-snapshot/v1",
    entries: [],
    frozenAt: "2026-10-07T12:00:00.000Z",
    watermarkSha256: fixtureSha256(Buffer.from("[]")),
  })}\n`);
  const manifestRecord = {
    payload: {
      contract_version: "adjustment-evaluation-export-manifest/v1",
      migration_checksums: migrationChecksums,
      migration_history_sha256: migrationHistorySha256,
      migration_names: migrationNames,
      query_contract_sha256:
        "c860039c72818a9b813ed9f9d93f5d8e115f4144e99d698734e9159ca3457bd4",
      query_contract_version: "adjustment-evaluation-export-query/v1",
      row_schema_sha256:
        "c21782034130d003e4af48fada4c8f6545f6f1c4dff550f92d0523c7d96d20e2",
      schema_migration: "0017_adjustment_evaluation_export.sql",
      site_key: "ballydidean",
      site_timezone: "America/Los_Angeles",
    },
    record_type: "manifest",
    transaction: {
      created_at_utc: "2026-10-07T12:00:00.000000Z",
      from_local_date: localDate,
      idle_in_transaction_session_timeout: "30s",
      isolation_level: "repeatable read",
      lock_timeout: "5s",
      read_only: "on",
      statement_timeout: "5min",
      to_local_date: localDate,
    },
  };
  await buildAdjustmentEvaluationPackage({
    edgeSnapshotPath: snapshotPath,
    fromLocalDate: localDate,
    input: Readable.from([`${JSON.stringify(manifestRecord)}\n`]),
    packageRoot,
    toLocalDate: localDate,
  });
}

// prohibit file-only commands from writing into serving or linked directories
test("performance outputs reject serving paths and symbolic ancestry", async () => {
  const root = await privateTestDirectory("performance-private-");
  const link = join(root, "linked");
  await symlink(resolve(import.meta.dirname, "../../../config"), link);
  await assert.rejects(requirePrivatePerformanceOutput(join(link, "new.json")), /private owned/u);
  await assert.rejects(requirePrivatePerformanceOutput(resolve(import.meta.dirname,
    "../../../config/forecast-adjustments/unapproved.json")), /outside private/u);
});

// bind equivalent database timestamp forms without understating label availability
test("verified package timestamps share canonical instants and conservative receipts", () => {
  assert.deepEqual(normalizeVerifiedPerformanceTimestamps({
    valid_at: "2026-09-30T10:00:00.000000Z",
    validAt: "2026-09-30T10:00:00+00:00",
    firstReceivedAt: "2026-09-30T10:00:00.123456+00:00",
    localDate: "2026-09-30",
    sourceKey: "unchanged",
  }), {
    valid_at: "2026-09-30T10:00:00.000Z",
    validAt: "2026-09-30T10:00:00.000Z",
    firstReceivedAt: "2026-09-30T10:00:00.124Z",
    localDate: "2026-09-30",
    sourceKey: "unchanged",
  });
});

// reject forecast member replacement after the package verifier returns
test("verified forecast rows rebind exact member bytes before decoding", async () => {
  const root = await privateTestDirectory("performance-member-rebind-");
  const adjustmentPackage = join(root, "adjustment");
  const forecastPackage = join(root, "forecast");
  const memberPath = "members/2026-10-01/station-hour/fixture.jsonl.gz";
  const memberAbsolutePath = join(forecastPackage, memberPath);
  const originalBytes = gzipSync(Buffer.from('{"record_kind":"station_hour","value":1}\n'), {
    level: 9,
    mtime: 0,
  });
  const replacementBytes = gzipSync(Buffer.from('{"record_kind":"station_hour","value":2}\n'), {
    level: 9,
    mtime: 0,
  });
  assert.equal(replacementBytes.byteLength, originalBytes.byteLength);
  await buildEmptyAdjustmentPackage(root, adjustmentPackage, "2026-10-01");
  await mkdir(join(forecastPackage, "members/2026-10-01/station-hour"), { recursive: true });
  await writeFile(memberAbsolutePath, originalBytes);
  const manifestBytes = Buffer.from(`${JSON.stringify({
    createdAtUtc: "2026-10-07T12:00:00.000Z",
    fromLocalDate: "2026-10-01",
    members: [{
      path: memberPath,
      sha256: fixtureSha256(originalBytes),
      sizeBytes: originalBytes.byteLength,
    }],
    siteKey: "ballydidean",
    toLocalDate: "2026-10-01",
  })}\n`);
  await writeFile(join(forecastPackage, "manifest.json"), manifestBytes);

  await assert.rejects(loadVerifiedPerformancePackages({
    adjustmentPackage,
    forecastPackage,
  }, {
    // replace one same-size valid gzip member after verification
    verifyForecastPackage: async () => {
      await writeFile(memberAbsolutePath, replacementBytes);
      return fixtureSha256(manifestBytes);
    },
  }), /forecast package member changed after verification/u);
});

// build one closed unavailable aggregate
function metric(unit) {
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

// build one closed support population
function support() {
  return {
    dateCount: 0,
    effectiveWeightSum: 0,
    eventCount: 0,
    excludedCount: 0,
    exclusionReasons: {},
    fallbackCount: 0,
    fallbackReasons: {},
    gapCount: 0,
    rowCount: 0,
    targetRowCount: 0,
    validHourCount: 0,
    vintageCount: 0,
    wetDateCount: 0,
    wetRowCount: 0,
  };
}

// build fixed empty rain diagnostics
function rainDiagnostics() {
  return {
    accumulations: [6, 12, 23].map((hours) => ({
      adjustedMae: null,
      completeWindows: 0,
      hours,
      rawMae: null,
    })),
    annualBalancedVolumeRatio: null,
    heavyAdjustedMae: null,
    heavyRawMae: null,
    probabilityOrderViolationCount: 0,
    thresholds: [0.1, 1, 2.5].map((thresholdMmPerHour) => ({
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

// build one validator-compatible family card
function card(family) {
  const units = {
    rain: "millimeters_per_hour",
    temperature: "celsius",
    wind: "meters_per_second",
  };
  return {
    bestMatchDiagnostic: null,
    comparisonState: "unscored",
    evidenceClass: "development",
    evidenceCutoffAt: "2026-10-07T10:00:00.000Z",
    family,
    metrics: metric(units[family]),
    qualificationState: "development_only",
    rainDiagnostics: family === "rain" ? rainDiagnostics() : null,
    recommendation: "none",
    servingIdentitySha256: null,
    servingState: "pending_review",
    slices: [],
    support: support(),
    supportState: "insufficient",
  };
}

// build one global edge-freeze row for composition tests
function edgeRow(identity, validAt, rawTemperatureC = 10) {
  return {
    bundleIdentities: {},
    edgeReceiptIdentitySha256: identity,
    firstEdgeCommittedAt: "2026-10-07T10:00:00.000Z",
    objectSha256: HASH,
    row: {
      provenanceComplete: false,
      raw: { temperatureC: rawTemperatureC },
      record: { id: identity, revisionCount: 1, validAt },
    },
    rowIndex: 0,
    settingsSha256: HASH,
    sourceReceiptAt: "2026-10-07T09:00:00.000Z",
  };
}

// build one already verified package-pair result
function verifiedPair(from, to, edges, suffix) {
  return {
    adjustmentManifest: {},
    adjustmentRows: [],
    bodies: {},
    edgeRows: edges,
    forecastManifest: {},
    forecastRows: [],
    inputs: {
      adjustmentEvidenceManifestSha256: suffix.repeat(64),
      adjustmentEvidenceWatermarkSha256: suffix.repeat(64),
      forecastTrainingManifestSha256: suffix.repeat(64),
      localDateFrom: from,
      localDateTo: to,
      targetCutoffAt: `${to}T23:59:59.000Z`,
    },
  };
}

// build causal wind edge rows and matching physical targets
function issuedWindCohort(bundle, definitions, repetitions, adjustedDelta) {
  const edgeRows = [];
  const stationRows = [];
  let sequence = 0;

  // materialize every requested metric-band independently
  for (const [metric, leadBand, horizonHours] of definitions) {
    // retain the requested support for this exact pair
    for (let repetition = 0; repetition < repetitions; repetition += 1) {
      const day = sequence % 7;
      const hour = Math.floor(sequence / 7);
      const validAt = new Date(Date.parse("2026-10-08T12:00:00.000Z") +
        day * 86_400_000 + hour * 3_600_000).toISOString();
      const productRunAt = new Date(Date.parse(validAt) -
        horizonHours * 3_600_000).toISOString();
      const rawValue = metric === "windGustMps" ? 8 : 5;
      edgeRows.push({
        bundleIdentities: { wind: { activeBundle: bundle } },
        edgeReceiptIdentitySha256: String(sequence + 1).padStart(64, "0"),
        firstEdgeCommittedAt: new Date(Date.parse(validAt) - 3_600_000).toISOString(),
        objectSha256: HASH,
        row: {
          provenanceComplete: true,
          raw: { [metric]: rawValue },
          record: {
            id: `wind-${String(sequence)}`,
            productRunAt,
            revisionCount: 1,
            validAt,
          },
          windAdjustment: {
            adjustedMetrics: { [metric]: rawValue + adjustedDelta },
            appliedMetrics: [metric],
            leadBand,
            state: "active",
          },
        },
        rowIndex: 0,
        settingsSha256: HASH,
        sourceReceiptAt: new Date(Date.parse(validAt) - 7_200_000).toISOString(),
      });

      // retain three deduplicated physical targets for each valid hour
      for (const physical_station_key of [
        "ambient-merlin",
        "tempest-64255",
        "tempest-38270",
      ]) {
        stationRows.push({
          content_hashes: [HASH],
          physical_station_key,
          received_at: new Date(Date.parse(validAt) + 3_600_000).toISOString(),
          record_kind: "station_hour",
          source_config_fingerprints: [HASH],
          source_keys: [`${physical_station_key}-source`],
          valid_at: validAt,
          wind_gust_mps: 8,
          wind_speed_mps: 5,
        });
      }
      sequence += 1;
    }
  }

  return { edgeRows, stationRows };
}

// build one exact forecast provider capture for rain replay
function rainForecastCapture() {
  const runInitializedAt = "2026-09-14T00:00:00.000Z";
  const hourly = {
    cloud_cover: [],
    precipitation: [],
    relative_humidity_2m: [],
    surface_pressure: [],
    temperature_2m: [],
    time: [],
    wind_direction_10m: [],
    wind_speed_10m: [],
  };

  // retain the provider's initialization plus forty-eight lead hours
  for (let lead = 0; lead <= 48; lead += 1) {
    hourly.time.push(new Date(Date.parse(runInitializedAt) +
      lead * 3_600_000).toISOString().slice(0, 16));
    hourly.temperature_2m.push(12);
    hourly.relative_humidity_2m.push(80);
    hourly.cloud_cover.push(90);
    hourly.surface_pressure.push(1000);
    hourly.wind_speed_10m.push(4);
    hourly.wind_direction_10m.push(180);
    hourly.precipitation.push(0.5);
  }

  const body = Buffer.from(JSON.stringify({ hourly, utc_offset_seconds: 0 }));
  return {
    body,
    bodySha256: createHash("sha256").update(body).digest("hex"),
    claimId: "rain-forecast",
    completedAt: "2026-09-14T06:01:00.000Z",
    kind: "forecast",
    runInitializedAt,
    stationId: null,
    windowEndExclusive: null,
    windowStart: null,
  };
}

// build one complete two-hour fixed-gauge provider capture
function rainStationCapture(station) {
  const startedAt = Date.parse("2026-09-14T08:00:00.000Z");
  const observations = [];

  // tile both target hours with exact one-minute intervals
  for (let minute = 1; minute <= 120; minute += 1) {
    observations.push([
      (startedAt + minute * 60_000) / 1000,
      0, 1, 2, 180, 3, 1000, 12, 80, 0, 0, 0, 0.01, 0, 0, 0, 2.7, 1,
      0, 0, 0, 0,
    ]);
  }

  const body = Buffer.from(JSON.stringify({
    device_id: station.deviceId,
    obs: observations,
    status: { status_code: 0 },
    type: "obs_st",
  }));
  return {
    body,
    bodySha256: createHash("sha256").update(body).digest("hex"),
    claimId: `rain-station-${String(station.locationId)}`,
    completedAt: "2026-09-14T10:00:00.000Z",
    kind: "station",
    runInitializedAt: null,
    stationId: station.locationId,
    windowEndExclusive: "2026-09-14T10:00:01.000Z",
    windowStart: "2026-09-14T08:00:01.000Z",
  };
}

// convert exact provider captures into verified-package records
function rainCaptureRows(captures) {
  const rows = [];
  const bodies = {};

  // bind every claim, receipt and compressed body member
  for (const capture of captures) {
    const memberPath = `bodies/${capture.bodySha256}.json.gz`;
    rows.push({
      payload: {
        id: capture.claimId,
        kind: capture.kind,
        runInitializedAt: capture.runInitializedAt,
        stationId: capture.stationId,
        windowEndExclusive: capture.windowEndExclusive,
        windowStart: capture.windowStart,
      },
      record_kind: "rain_claim",
    }, {
      body_member_path: memberPath,
      payload: {
        bodySha256: capture.bodySha256,
        claimId: capture.claimId,
        completedAt: capture.completedAt,
        outcome: "valid",
      },
      record_kind: "rain_receipt",
    });
    bodies[memberPath] = gzipSync(capture.body);
  }

  return { bodies, rows };
}

test("package composition scopes global edge freezes and rejects duplicate collisions", () => {
  const first = edgeRow("1".repeat(64), "2026-10-07T12:00:00.000Z");
  const second = edgeRow("2".repeat(64), "2026-10-08T12:00:00.000Z");
  const outside = edgeRow("3".repeat(64), "2026-10-09T12:00:00.000Z");
  const pair1 = verifiedPair(
    "2026-10-07",
    "2026-10-07",
    [first, second, outside],
    "1",
  );
  const pair2 = verifiedPair(
    "2026-10-08",
    "2026-10-08",
    [first, second, outside],
    "2",
  );
  const composed = composeVerifiedPerformancePackagePairs([pair2, pair1]);
  assert.deepEqual(
    composed.edgeRows.map((row) => row.edgeReceiptIdentitySha256),
    ["1".repeat(64), "2".repeat(64)],
  );

  const collision = {
    ...first,
    row: {
      ...first.row,
      raw: { temperatureC: 11 },
    },
  };
  assert.throws(() => composeVerifiedPerformancePackagePairs([
    pair1,
    verifiedPair("2026-10-08", "2026-10-08", [collision, second], "2"),
  ]), /edge receipt row identity collision/);
});

test("temperature loader preserves native inputs and matched Best Match diagnostic", async () => {
  const validAt = "2026-10-08T13:00:00.000Z";
  const runInitializedAt = "2026-10-08T06:00:00.000Z";
  const stationRows = [
    ["ambient-merlin", 10],
    ["tempest-64255", 11],
    ["tempest-38270", 12],
    ["ballydidean-ecowitt", 99],
  ].map(([physical_station_key, temperature_c]) => ({
    content_hashes: [HASH],
    physical_station_key,
    received_at: "2026-10-08T13:00:00.000Z",
    record_kind: "station_hour",
    source_config_fingerprints: [HASH],
    source_keys: [`${physical_station_key}-source`],
    temperature_c,
    valid_at: validAt,
  }));
  const packages = {
    adjustmentManifest: {},
    adjustmentRows: [{
      payload: {
        id: "run-1",
        modelCycle: "06z",
        providerResponseSha256: HASH,
        recentErrorState: { bias: 0 },
        runInitializedAt,
        upstreamModel: "ecmwf_ifs025",
      },
      record_kind: "temperature_run",
    }, {
      first_received_at: "2026-10-08T06:05:00.000Z",
      local_date: "2026-10-08",
      payload: {
        modelLeadHours: 7,
        rawRelativeHumidityPercent: 80,
        rawTemperatureC: 9,
        rawWindSpeedMps: 2,
        runId: "run-1",
        validAt,
      },
      record_kind: "temperature_hour",
      record_revision_identity: "temperature-hour-1",
    }],
    bodies: {},
    edgeRows: [{
      bundleIdentities: {
        temperature: { activeBundle: HASH },
      },
      edgeReceiptIdentitySha256: HASH,
      firstEdgeCommittedAt: "2026-10-08T06:10:00.000Z",
      objectSha256: HASH,
      row: {
        provenanceComplete: false,
        record: { id: "edge-row", revisionCount: 1, validAt },
        temperatureAdjustment: {
          bundleSha256: HASH,
          correctedTemperatureC: 9.5,
          rawBestMatchTemperatureC: 8,
          reasonCode: null,
          sourceForecast: {
            firstReceivedAt: "2026-10-08T06:05:00.000Z",
            modelCycle: "06z",
            modelLeadHours: 7,
            providerResponseSha256: HASH,
            rawRelativeHumidityPercent: 80,
            rawTemperatureC: 9,
            rawWindSpeedMps: 2,
            runInitializedAt,
            upstreamModel: "ecmwf_ifs025",
            validAt,
          },
          state: "active",
        },
      },
      rowIndex: 0,
      settingsSha256: HASH,
      sourceReceiptAt: "2026-10-08T06:05:00.000Z",
    }],
    forecastManifest: {},
    forecastRows: stationRows,
    inputs: {
      adjustmentEvidenceManifestSha256: HASH,
      adjustmentEvidenceWatermarkSha256: HASH,
      forecastTrainingManifestSha256: HASH,
      localDateFrom: "2026-10-08",
      localDateTo: "2026-10-08",
      targetCutoffAt: "2026-10-08T14:00:00.000Z",
    },
  };
  const data = await loadTemperaturePerformanceData({
    adjustmentPackage: "adjustment",
    forecastPackage: "forecast",
  }, { loadPackages: async () => packages });
  assert.equal(data.rows.length, 1);
  assert.equal(data.rows[0].bestMatchRawTemperatureC, 8);
  assert.equal(data.rows[0].actualTemperatureC, 11);
  assert.equal(data.rows[0].scoreEligible, true);
  assert.equal(data.rows[0].scoredPairMetadata.evidenceClass, "prospective_receipt");
  assert.equal(data.rows[0].scoredPairMetadata.provenanceComplete, false);
  assert.equal(data.rows[0].recordedRuntimeResult.applied, true);
  assert.equal(data.rows[0].recordedRuntimeResult.predictionTemperatureC, 9.5);
  assert.equal(
    data.rows[0].recordedRuntimeResult.scoredPairMetadata.evidenceClass,
    "as_issued",
  );
  assert.deepEqual(data.actualServingFallbacks, []);

  // preserve nonactive public-serving decisions outside the ECMWF comparator
  const activeEdge = packages.edgeRows[0];
  const sourceForecast = activeEdge.row.temperatureAdjustment.sourceForecast;
  const fallbackLate = {
    ...activeEdge,
    edgeReceiptIdentitySha256: "1".repeat(64),
    firstEdgeCommittedAt: "2026-10-08T06:20:00.000Z",
    row: {
      ...activeEdge.row,
      record: { id: "fallback-edge", revisionCount: 1, validAt },
      temperatureAdjustment: {
        bundleSha256: HASH,
        correctedTemperatureC: null,
        rawBestMatchTemperatureC: 8.25,
        reasonCode: "prediction_invalid",
        sourceForecast,
        state: "raw_fallback",
      },
    },
    sourceReceiptAt: "2026-10-08T06:09:00.000Z",
  };
  const fallbackEarly = {
    ...fallbackLate,
    edgeReceiptIdentitySha256: "2".repeat(64),
    firstEdgeCommittedAt: "2026-10-08T06:15:00.000Z",
    sourceReceiptAt: "2026-10-08T06:05:00.000Z",
  };
  const disabled = {
    ...activeEdge,
    edgeReceiptIdentitySha256: "3".repeat(64),
    firstEdgeCommittedAt: "2026-10-08T06:12:00.000Z",
    row: {
      ...activeEdge.row,
      record: { id: "disabled-edge", revisionCount: 1, validAt },
      temperatureAdjustment: {
        bundleSha256: null,
        correctedTemperatureC: null,
        rawBestMatchTemperatureC: 8.5,
        reasonCode: "canary_expired",
        sourceForecast: null,
        state: "disabled",
      },
    },
    sourceReceiptAt: "2026-10-08T06:02:00.000Z",
  };
  const otherBundle = {
    ...fallbackEarly,
    bundleIdentities: { temperature: { activeBundle: "c".repeat(64) } },
    edgeReceiptIdentitySha256: "4".repeat(64),
    row: {
      ...fallbackEarly.row,
      temperatureAdjustment: {
        ...fallbackEarly.row.temperatureAdjustment,
        bundleSha256: "c".repeat(64),
        rawBestMatchTemperatureC: 7.75,
        reasonCode: "source_identity_mismatch",
      },
    },
  };
  const fallbackData = await loadTemperaturePerformanceData({
    adjustmentPackage: "adjustment",
    forecastPackage: "forecast",
  }, {
    loadPackages: async () => ({
      ...packages,
      edgeRows: [fallbackLate, fallbackEarly, disabled, otherBundle],
    }),
  });
  assert.equal(fallbackData.rows[0].recordedRuntimeResult, null);
  assert.equal(
    fallbackData.rows[0].scoredPairMetadata.evidenceClass,
    "prospective_receipt",
  );
  assert.equal(fallbackData.rows[0].bestMatchRawTemperatureC, 8.25);
  assert.equal(fallbackData.actualServingFallbacks.length, 3);
  const rawFallback = fallbackData.actualServingFallbacks.find((item) =>
    item.state === "raw_fallback" && item.servingIdentitySha256 === HASH);
  assert.ok(rawFallback);
  assert.deepEqual(rawFallback, {
    baseline: "actual_serving_best_match_fallback",
    bundleIdentities: fallbackEarly.bundleIdentities,
    capturedBundleSha256: HASH,
    evidenceClass: "as_issued",
    firstEdgeCommittedAt: "2026-10-08T06:15:00.000Z",
    provenanceComplete: false,
    rawBestMatchTemperatureC: 8.25,
    reasonCode: "prediction_invalid",
    record: fallbackEarly.row.record,
    rowIdentity: rawFallback.rowIdentity,
    servingIdentitySha256: HASH,
    settingsSha256: HASH,
    sourceReceiptAt: "2026-10-08T06:05:00.000Z",
    state: "raw_fallback",
    validAt,
  });
  const disabledFallback = fallbackData.actualServingFallbacks.find((item) =>
    item.state === "disabled");
  assert.ok(disabledFallback);
  assert.equal(disabledFallback.rawBestMatchTemperatureC, 8.5);
  assert.equal(disabledFallback.reasonCode, "canary_expired");
  assert.equal(disabledFallback.capturedBundleSha256, null);
  assert.deepEqual(disabledFallback.record, disabled.row.record);
  assert.equal(disabledFallback.validAt, validAt);
  const otherBundleFallback = fallbackData.actualServingFallbacks.find((item) =>
    item.servingIdentitySha256 === "c".repeat(64));
  assert.ok(otherBundleFallback);
  assert.equal(otherBundleFallback.rawBestMatchTemperatureC, 7.75);
  assert.equal(otherBundleFallback.reasonCode, "source_identity_mismatch");
  assert.notEqual(otherBundleFallback.rowIdentity, rawFallback.rowIdentity);

  // bind later independent revision receipts without substituting target values
  packages.adjustmentRows.push({
    record_kind: "target_revision_diagnostic",
    payload: {
      contributorRevisionSha256: "c".repeat(64),
      maxSourceReceiptAt: "2026-10-08T14:00:00.000Z",
      physicalStationKey: "ambient-merlin",
      sourceKey: "ambient-merlin-source",
      sourceConfigFingerprint: HASH,
      validAt,
    },
  });
  const revised = await loadTemperaturePerformanceData({
    adjustmentPackage: "adjustment", forecastPackage: "forecast",
  }, { loadPackages: async () => packages });
  assert.equal(revised.rows[0].actualTemperatureC, data.rows[0].actualTemperatureC);
  assert.equal(revised.rows[0].targetMaxReceiptAt, "2026-10-08T14:00:00.000Z");
  assert.notEqual(revised.rows[0].targetIdentity, data.rows[0].targetIdentity);
});

test("wind command evaluates all thirteen active pairs without changing serving", async () => {
  const root = await privateTestDirectory("weather-performance-wind-");
  const output = join(root, "wind.json");
  const validAt = "2026-10-07T19:00:00.000Z";
  const referenceAt = "2026-10-07T18:00:00.000Z";
  const forecast = {
    adapter_contracts: ["forecast-daily/v4"],
    content_hashes: [HASH],
    contract_epoch: "legacy-v4/9d26d9c46dcaacc422c28e854327b11cd710625e092110786010f0687a100d83",
    dataset: "forecast",
    record_kind: "legacy_v4_retrieval_snapshot",
    reference_at: referenceAt,
    relative_humidity_percent: 70,
    source_config_fingerprints: ["ceb83ac4ba3ddc421a31043794ad450a859ecc31643506f93f64a28feb15e5b4"],
    source_keys: ["open-meteo-forecast-v4"],
    target_lead_hours: 1,
    temperature_c: 10,
    upstream_model: "best_match",
    valid_at: validAt,
    wind_direction_degrees: 180,
    wind_gust_mps: 8,
    wind_speed_mps: 5,
  };
  const stations = [
    ["ambient-merlin", 4],
    ["tempest-64255", 5],
    ["tempest-38270", 6],
  ].map(([physical_station_key, value]) => ({
    content_hashes: [HASH],
    physical_station_key,
    received_at: "2026-10-07T20:00:00.000Z",
    record_kind: "station_hour",
    source_keys: [`${physical_station_key}-source`],
    valid_at: validAt,
    wind_gust_mps: value + 2,
    wind_speed_mps: value,
  }));
  const packages = {
    adjustmentManifest: {},
    adjustmentRows: [],
    bodies: {},
    edgeRows: [],
    forecastManifest: {},
    forecastRows: [forecast, ...stations],
    inputs: {
      adjustmentEvidenceManifestSha256: HASH,
      adjustmentEvidenceWatermarkSha256: HASH,
      forecastTrainingManifestSha256: HASH,
      localDateFrom: "2026-10-07",
      localDateTo: "2026-10-07",
      targetCutoffAt: "2026-10-07T10:00:00.000Z",
    },
  };
  await runForecastAdjustmentPerformanceCli([
    "wind-requalify",
    "--forecast-package", "forecast",
    "--adjustment-package", "adjustment",
    "--output", output,
  ], {
    loadPackages: async () => packages,
    now: () => NOW,
  });
  const report = JSON.parse(await readFile(output, "utf8"));
  assert.equal(report.family.family, "wind");
  assert.match(report.sourceRevision, /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/u);
  assert.equal(report.family.slices.length, 13);
  assert.ok(report.family.slices.some((slice) =>
    slice.label === "windgustmps-121-144"));
  assert.equal(report.family.qualificationState, "counterfactual_only");
  assert.equal(report.family.servingState, "authorized_active");
  assert.equal(report.family.bestMatchDiagnostic, null);
  assert.equal(report.pairReviews.length, 13);
  assert.ok(report.pairReviews.every((review) =>
    review.supportState === "insufficient"));
  assert.equal(report.family.supportState, "insufficient");

  await assert.rejects(() => runForecastAdjustmentPerformanceCli([
    "wind-requalify",
    "--forecast-package", "forecast",
    "--adjustment-package", "adjustment",
    "--source-revision", "abc1234",
    "--output", join(root, "invalid-revision.json"),
  ], {
    loadPackages: async () => packages,
    now: () => NOW,
  }), /source revision is invalid/u);

  const issuedOutput = join(root, "wind-issued.json");
  const issuedPackages = {
    ...packages,
    edgeRows: [{
      bundleIdentities: {
        wind: { activeBundle: report.family.servingIdentitySha256 },
      },
      edgeReceiptIdentitySha256: HASH,
      firstEdgeCommittedAt: "2026-10-07T18:30:00.000Z",
      objectSha256: HASH,
      row: {
        provenanceComplete: false,
        raw: { windGustMps: 8, windSpeedMps: 5 },
        record: {
          id: "issued-row",
          productRunAt: "2026-10-07T17:07:05.934Z",
          revisionCount: 1,
          validAt,
        },
        windAdjustment: {
          adjustedMetrics: { windGustMps: 7.5, windSpeedMps: 4.5 },
          appliedMetrics: ["windGustMps", "windSpeedMps"],
          leadBand: "001-024",
          state: "active",
        },
      },
      rowIndex: 0,
      settingsSha256: HASH,
      sourceReceiptAt: "2026-10-07T18:20:00.000Z",
    }, {
      bundleIdentities: {
        wind: { activeBundle: report.family.servingIdentitySha256 },
      },
      edgeReceiptIdentitySha256: "c".repeat(64),
      firstEdgeCommittedAt: "2026-10-07T18:31:00.000Z",
      objectSha256: "d".repeat(64),
      row: {
        provenanceComplete: true,
        raw: { windGustMps: 8, windSpeedMps: 5 },
        record: {
          id: "not-applicable-row",
          productRunAt: "2026-10-07T17:07:05.934Z",
          revisionCount: 1,
          validAt: "2026-10-07T17:00:00.000Z",
        },
        windAdjustment: {
          adjustedMetrics: {},
          appliedMetrics: [],
          leadBand: null,
          reasonCode: "unsupported_lead",
          state: "not_applicable",
        },
      },
      rowIndex: 0,
      settingsSha256: "e".repeat(64),
      sourceReceiptAt: "2026-10-07T18:21:00.000Z",
    }],
  };
  await runForecastAdjustmentPerformanceCli([
    "wind-requalify",
    "--forecast-package", "forecast",
    "--adjustment-package", "adjustment",
    "--source-revision", SOURCE_REVISION,
    "--output", issuedOutput,
  ], {
    loadPackages: async () => issuedPackages,
    now: () => NOW,
  });
  const issuedReport = JSON.parse(await readFile(issuedOutput, "utf8"));
  assert.equal(issuedReport.family.evidenceClass, "as_issued");
  assert.equal(issuedReport.family.qualificationState, "pending_support");
  assert.equal(issuedReport.family.support.exclusionReasons.provenance_incomplete, 2);
  assert.equal(issuedReport.family.support.exclusionReasons.not_applicable, 2);
  assert.equal(issuedReport.family.support.excludedCount, 2);
  assert.equal(issuedReport.family.support.fallbackCount, 2);
  assert.deepEqual(issuedReport.family.support.fallbackReasons, { raw_fallback: 2 });
  assert.equal(issuedReport.family.support.gapCount, 4);
  assert.equal(issuedReport.family.support.rowCount, 4);
  assert.equal(issuedReport.family.support.targetRowCount, 2);

  const nonActiveOutput = join(root, "wind-only-not-applicable.json");
  await runForecastAdjustmentPerformanceCli([
    "wind-requalify",
    "--forecast-package", "forecast",
    "--adjustment-package", "adjustment",
    "--source-revision", SOURCE_REVISION,
    "--output", nonActiveOutput,
  ], {
    loadPackages: async () => ({
      ...issuedPackages,
      edgeRows: issuedPackages.edgeRows.slice(1),
    }),
    now: () => NOW,
  });
  const nonActiveReport = JSON.parse(await readFile(nonActiveOutput, "utf8"));
  assert.equal(nonActiveReport.family.evidenceClass, "as_issued");
  assert.equal(nonActiveReport.family.support.rowCount, 2);
  assert.equal(nonActiveReport.family.support.targetRowCount, 0);
  assert.ok(nonActiveReport.pairReviews.every((review) => review.rowCount === 0));

  const invalidActivePackages = structuredClone(issuedPackages);
  invalidActivePackages.edgeRows[0].row.windAdjustment.leadBand = null;
  await assert.rejects(() => runForecastAdjustmentPerformanceCli([
    "wind-requalify",
    "--forecast-package", "forecast",
    "--adjustment-package", "adjustment",
    "--source-revision", SOURCE_REVISION,
    "--output", join(root, "wind-invalid-active-band.json"),
  ], {
    loadPackages: async () => invalidActivePackages,
    now: () => NOW,
  }), /edge wind lead band must be a nonempty string/u);

  invalidActivePackages.edgeRows[0].row.windAdjustment.leadBand = "999-999";
  await assert.rejects(() => runForecastAdjustmentPerformanceCli([
    "wind-requalify",
    "--forecast-package", "forecast",
    "--adjustment-package", "adjustment",
    "--source-revision", SOURCE_REVISION,
    "--output", join(root, "wind-invalid-active-band-name.json"),
  ], {
    loadPackages: async () => invalidActivePackages,
    now: () => NOW,
  }), /edge wind lead band is invalid/u);

  const diluted = issuedWindCohort(
    report.family.servingIdentitySha256,
    WIND_PAIRS,
    3,
    0,
  );
  const dilutedOutput = join(root, "wind-diluted.json");
  await runForecastAdjustmentPerformanceCli([
    "wind-requalify",
    "--forecast-package", "forecast",
    "--adjustment-package", "adjustment",
    "--source-revision", SOURCE_REVISION,
    "--output", dilutedOutput,
  ], {
    loadPackages: async () => ({
      ...packages,
      edgeRows: diluted.edgeRows,
      forecastRows: diluted.stationRows,
    }),
    now: () => NOW,
  });
  const dilutedReport = JSON.parse(await readFile(dilutedOutput, "utf8"));
  assert.equal(dilutedReport.family.support.targetRowCount, 39);
  assert.equal(dilutedReport.family.supportState, "insufficient");
  assert.ok(dilutedReport.pairReviews.every((review) =>
    review.supportState === "insufficient"));

  const weak = issuedWindCohort(
    report.family.servingIdentitySha256,
    [["windGustMps", "121-144", 132]],
    30,
    5,
  );
  const weakOutput = join(root, "wind-weak-harm.json");
  await runForecastAdjustmentPerformanceCli([
    "wind-requalify",
    "--forecast-package", "forecast",
    "--adjustment-package", "adjustment",
    "--source-revision", SOURCE_REVISION,
    "--output", weakOutput,
  ], {
    loadPackages: async () => ({
      ...packages,
      edgeRows: weak.edgeRows,
      forecastRows: weak.stationRows,
    }),
    now: () => NOW,
  });
  const weakReport = JSON.parse(await readFile(weakOutput, "utf8"));
  const weakPair = weakReport.pairReviews.find((review) =>
    review.pairIdentity === "windGustMps:121-144");
  assert.equal(weakPair.comparisonState, "worse");
  assert.equal(weakPair.recommendation, "review_disable");
  assert.equal(weakReport.family.comparisonState, "worse");
  assert.equal(weakReport.family.recommendation, "review_disable");
});

test("rain command completes honestly when verified package has no mature rows", async () => {
  const root = await privateTestDirectory("weather-performance-rain-");
  const output = join(root, "rain.json");
  const packages = {
    adjustmentManifest: {},
    adjustmentRows: [],
    bodies: {},
    edgeRows: [],
    forecastManifest: {},
    forecastRows: [],
    inputs: {
      adjustmentEvidenceManifestSha256: HASH,
      adjustmentEvidenceWatermarkSha256: HASH,
      forecastTrainingManifestSha256: HASH,
      localDateFrom: "2026-10-07",
      localDateTo: "2026-10-07",
      targetCutoffAt: "2026-10-07T10:00:00.000Z",
    },
  };
  await runForecastAdjustmentPerformanceCli([
    "rain-evaluate",
    "--forecast-package", "forecast",
    "--adjustment-package", "adjustment",
    "--source-revision", SOURCE_REVISION,
    "--output", output,
  ], {
    loadPackages: async () => packages,
    now: () => NOW,
  });
  const report = JSON.parse(await readFile(output, "utf8"));
  assert.equal(report.family.supportState, "insufficient");
  assert.equal(report.family.comparisonState, "unscored");
  assert.equal(report.family.rainDiagnostics.thresholds.length, 3);
});

test("rain command separates receipt-backed issuance from development replay", async () => {
  const root = await privateTestDirectory("weather-performance-rain-issued-");
  const output = join(root, "rain-issued.json");
  const forecastCapture = rainForecastCapture();
  const run = createRainAdjustmentRun(
    [forecastCapture],
    "2026-09-14T08:05:00.000Z",
  );
  assert.notEqual(run, null);
  const stationCaptures = RAIN_COLLECTION_STATIONS.map(rainStationCapture);
  const captured = rainCaptureRows([forecastCapture, ...stationCaptures]);
  const edgeRows = run.hours.slice(0, 3).map((hour, index) => ({
    bundleIdentities: { rain: { activeBundle: run.modelSha256 } },
    edgeReceiptIdentitySha256: String(index + 1).padStart(64, "1"),
    firstEdgeCommittedAt: index === 1
      ? "2026-09-14T10:30:00.000Z"
      : new Date(Date.parse(hour.validAt) - 30 * 60_000).toISOString(),
    objectSha256: HASH,
    row: {
      provenanceComplete: true,
      rainAdjustment: {
        bundleSha256: run.modelSha256,
        correctedPrecipitationMm: hour.correctedPrecipitationMm,
        rawBestMatchPrecipitationMm: null,
        reasonCode: hour.reasonCode,
        sourceForecast: {
          firstReceivedAt: forecastCapture.completedAt,
          modelLeadHours: hour.modelLeadHours,
          rawPrecipitationMm: hour.rawPrecipitationMm,
          runInitializedAt: run.runInitializedAt,
          validAt: hour.validAt,
        },
        state: hour.applied ? "active" : "raw_fallback",
      },
      record: { id: `rain-edge-${String(index)}`, revisionCount: 1 },
    },
    rowIndex: index,
    settingsSha256: HASH,
    sourceReceiptAt: forecastCapture.completedAt,
  }));
  const packages = {
    adjustmentManifest: {},
    adjustmentRows: [...captured.rows, {
      payload: run,
      record_kind: "rain_adjustment_run",
      record_revision_identity: "rain-run-1",
    }],
    bodies: captured.bodies,
    edgeRows,
    forecastManifest: {},
    forecastRows: [],
    inputs: {
      adjustmentEvidenceManifestSha256: HASH,
      adjustmentEvidenceWatermarkSha256: HASH,
      forecastTrainingManifestSha256: HASH,
      localDateFrom: "2026-09-14",
      localDateTo: "2026-09-14",
      targetCutoffAt: "2026-09-15T00:00:00.000Z",
    },
  };
  await runForecastAdjustmentPerformanceCli([
    "rain-evaluate",
    "--forecast-package", "forecast",
    "--adjustment-package", "adjustment",
    "--source-revision", SOURCE_REVISION,
    "--output", output,
  ], {
    loadPackages: async () => packages,
    now: () => NOW,
  });
  const report = JSON.parse(await readFile(output, "utf8"));
  assert.equal(report.family.evidenceClass, "as_issued");
  assert.equal(report.family.qualificationState, "pending_support");
  assert.equal(report.family.supportState, "insufficient");
  assert.equal(report.family.support.rowCount, 3);
  assert.equal(report.family.support.targetRowCount, 1);
  assert.equal(report.family.support.exclusionReasons.issued_after_valid, 1);
  assert.equal(report.family.support.exclusionReasons.missing_target, 1);
  assert.equal(report.developmentReference.evidenceClass, "development");
  assert.equal(report.developmentReference.rowCount, 23);
  assert.equal(report.family.rainDiagnostics.thresholds[0].reliability
    .reduce((sum, bin) => sum + bin.count, 0), 1);
});

test("scorecard rejects stale reports and verifies exact current manifests", async () => {
  const root = await privateTestDirectory("weather-performance-scorecard-");
  const forecastManifest = {
    createdAtUtc: "2026-10-07T10:00:00.000Z",
    fromLocalDate: "2026-10-07",
    toLocalDate: "2026-10-07",
  };
  const adjustmentManifest = {
    edgeEvidence: { watermarkSha256: HASH },
    fromLocalDate: "2026-10-07",
    toLocalDate: "2026-10-07",
  };
  const forecastPath = join(root, "forecast-manifest.json");
  const adjustmentPath = join(root, "adjustment-manifest.json");
  const forecastBytes = `${JSON.stringify(forecastManifest)}\n`;
  const adjustmentBytes = `${JSON.stringify(adjustmentManifest)}\n`;
  await writeFile(forecastPath, forecastBytes);
  await writeFile(adjustmentPath, adjustmentBytes);
  const { createHash } = await import("node:crypto");
  const digest = (value) => createHash("sha256").update(value).digest("hex");
  const inputs = {
    adjustmentEvidenceManifestSha256: digest(adjustmentBytes),
    adjustmentEvidenceWatermarkSha256: HASH,
    forecastTrainingManifestSha256: digest(forecastBytes),
    localDateFrom: "2026-10-07",
    localDateTo: "2026-10-07",
    targetCutoffAt: "2026-10-07T10:00:00.000Z",
  };
  const paths = {};
  for (const family of ["temperature", "wind", "rain"]) {
    const path = join(root, `${family}.json`);
    await writeFile(path, `${JSON.stringify({
      contractVersion: "forecast-adjustment-performance-report/v1",
      family: card(family),
      generatedAt: NOW.toISOString(),
      inputs,
      siteKey: "ballydidean",
      sourceRevision: SOURCE_REVISION,
    })}\n`);
    paths[family] = path;
  }
  const output = join(root, "scorecard.json");
  await runForecastAdjustmentPerformanceCli([
    "scorecard",
    "--forecast-manifest", forecastPath,
    "--adjustment-manifest", adjustmentPath,
    "--temperature-report", paths.temperature,
    "--wind-report", paths.wind,
    "--rain-report", paths.rain,
    "--output", output,
  ], { now: () => NOW });
  const scorecard = JSON.parse(await readFile(output, "utf8"));
  assert.equal(scorecard.servingChanged, false);
  assert.equal(scorecard.operatorApprovalRequired, true);

  let verified = "";
  await runForecastAdjustmentPerformanceCli([
    "verify-scorecard", "--input", output,
  ], { now: () => NOW, writeOutput: (value) => { verified += value; } });
  assert.match(verified, /^[a-f0-9]{64}\n$/u);

  const forecastManifest2 = {
    createdAtUtc: "2026-10-08T10:00:00.000Z",
    fromLocalDate: "2026-10-08",
    toLocalDate: "2026-10-08",
  };
  const adjustmentManifest2 = {
    edgeEvidence: { watermarkSha256: HASH },
    fromLocalDate: "2026-10-08",
    toLocalDate: "2026-10-08",
  };
  const forecastPath2 = join(root, "forecast-manifest-2.json");
  const adjustmentPath2 = join(root, "adjustment-manifest-2.json");
  const forecastBytes2 = `${JSON.stringify(forecastManifest2)}\n`;
  const adjustmentBytes2 = `${JSON.stringify(adjustmentManifest2)}\n`;
  await writeFile(forecastPath2, forecastBytes2);
  await writeFile(adjustmentPath2, adjustmentBytes2);
  const composedInputs = {
    adjustmentEvidenceManifestSha256: digest(`${JSON.stringify([
      digest(adjustmentBytes), digest(adjustmentBytes2),
    ])}\n`),
    adjustmentEvidenceWatermarkSha256: digest(`${JSON.stringify([HASH, HASH])}\n`),
    forecastTrainingManifestSha256: digest(`${JSON.stringify([
      digest(forecastBytes), digest(forecastBytes2),
    ])}\n`),
    localDateFrom: "2026-10-07",
    localDateTo: "2026-10-08",
    targetCutoffAt: "2026-10-08T10:00:00.000Z",
  };
  const composedPaths = {};

  // write one immutable family wrapper for the composed cohort
  for (const family of ["temperature", "wind", "rain"]) {
    const path = join(root, `${family}-composed.json`);
    await writeFile(path, `${JSON.stringify({
      contractVersion: "forecast-adjustment-performance-report/v1",
      family: card(family),
      generatedAt: NOW.toISOString(),
      inputs: composedInputs,
      siteKey: "ballydidean",
      sourceRevision: SOURCE_REVISION,
    })}\n`);
    composedPaths[family] = path;
  }

  const composedOutput = join(root, "scorecard-composed.json");
  await runForecastAdjustmentPerformanceCli([
    "scorecard",
    "--forecast-manifest", forecastPath,
    "--adjustment-manifest", adjustmentPath,
    "--forecast-manifest", forecastPath2,
    "--adjustment-manifest", adjustmentPath2,
    "--temperature-report", composedPaths.temperature,
    "--wind-report", composedPaths.wind,
    "--rain-report", composedPaths.rain,
    "--output", composedOutput,
  ], { now: () => NOW });
  const composed = JSON.parse(await readFile(composedOutput, "utf8"));
  assert.equal(composed.inputs.localDateFrom, "2026-10-07");
  assert.equal(composed.inputs.localDateTo, "2026-10-08");

  const stale = JSON.parse(await readFile(paths.wind, "utf8"));
  stale.inputs.forecastTrainingManifestSha256 = HASH;
  const stalePath = join(root, "wind-stale.json");
  await writeFile(stalePath, `${JSON.stringify(stale)}\n`);
  await assert.rejects(() => runForecastAdjustmentPerformanceCli([
    "scorecard",
    "--forecast-manifest", forecastPath,
    "--adjustment-manifest", adjustmentPath,
    "--temperature-report", paths.temperature,
    "--wind-report", stalePath,
    "--rain-report", paths.rain,
    "--output", join(root, "stale-scorecard.json"),
  ], { now: () => NOW }), /same input snapshot|stale/);
});
