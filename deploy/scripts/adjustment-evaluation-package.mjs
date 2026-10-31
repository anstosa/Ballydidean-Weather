#!/usr/bin/env node

import { execFileSync, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  constants,
  createReadStream,
  createWriteStream,
  fstatSync,
  lstatSync,
  readFileSync,
  readdirSync,
  realpathSync,
  statfsSync,
  statSync,
} from "node:fs";
import {
  copyFile,
  lstat,
  mkdir,
  open,
  readFile,
  realpath,
  rename,
  rm,
  unlink,
  writeFile,
} from "node:fs/promises";
import { basename, dirname, join, relative, resolve, sep } from "node:path";
import { once } from "node:events";
import { PassThrough, Readable, Transform } from "node:stream";
import { StringDecoder } from "node:string_decoder";
import { pipeline } from "node:stream/promises";
import { createGunzip, createGzip, gunzipSync, gzipSync } from "node:zlib";

import {
  AdjustmentShadowMetadataCustodyProofStore,
  AdjustmentUnsupportedTerminalProofStore,
  readAdjustmentRevisionCaptureEpochWitness,
  validateAdjustmentMaintenanceFinalizationProofV3,
  validateAdjustmentRevisionColdCustodyAcknowledgementV2,
  validateAdjustmentRevisionCaptureEpochWitness,
  validateAdjustmentUnsupportedTerminalProofV1,
  validateAdjustmentShadowMetadataCustodyPreparation,
  validateAdjustmentShadowMetadataCustodyProof,
  validateAdjustmentShadowMetadataCustodyStatus,
} from "./adjustment-evidence-store.mjs";

const PACKAGE_VERSION = "adjustment-evaluation-export-package/v1";
const DATABASE_MANIFEST_VERSION = "adjustment-evaluation-export-manifest/v1";
const EDGE_OBJECT_VERSION = "forecast-adjustment-evidence-object/v1";
const EDGE_RECEIPT_VERSION = "forecast-adjustment-edge-receipt/v1";
const EDGE_SNAPSHOT_VERSION = "forecast-adjustment-evidence-snapshot/v1";
const EDGE_WINDOWS = new Set(["days=1", "days=5", "days=10", "overnight"]);
const MAX_DAYS = 14;
const MAX_ROWS = 8_192;
const MAX_COMPRESSED_BODY_BYTES = 32 * 1024 * 1024;
const MAX_CANONICAL_BYTES = 48 * 1024 * 1024;
const MAX_ARCHIVE_BYTES = 48 * 1024 * 1024;
const MAX_TEMP_BYTES = 64 * 1024 * 1024;
const MAX_OTHER_TEMP_BYTES = 16 * 1024 * 1024;
const MAX_DATABASE_RECORD_BYTES = 4 * 1024 * 1024;
const MAX_RATIO = 64;
const TAR_BLOCK_BYTES = 512;
const EXPECTED_ROW_SCHEMA_SHA256 =
  "c21782034130d003e4af48fada4c8f6545f6f1c4dff550f92d0523c7d96d20e2";
const EXPECTED_QUERY_CONTRACT_SHA256 =
  "c860039c72818a9b813ed9f9d93f5d8e115f4144e99d698734e9159ca3457bd4";
const HASH_PATTERN = /^[a-f0-9]{64}$/u;
const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/u;
const WEATHER_GITHUB_REPOSITORY = Object.freeze({
  fullName: "anstosa/Ballydidean-Weather",
  id: 1_342_404_160,
});
const EXPECTED_MIGRATION_NAMES = [
  "0001_initial_weather.sql",
  "0002_worker_migration_readiness.sql",
  "0003_ecowitt_measurements.sql",
  "0004_tempest_metadata.sql",
  "0005_source_supersession.sql",
  "0006_station_coordinates.sql",
  "0007_tide_sources.sql",
  "0008_ecowitt_property_sensors.sql",
  "0009_forecast_anchor_records.sql",
  "0010_forecast_training_export.sql",
  "0011_forecast_runtime_provenance.sql",
  "0012_hide_archive_only_forecasts_from_live_reads.sql",
  "0013_ecmwf_temperature_canary.sql",
  "0014_rain_collection.sql",
  "0015_rain_station_access.sql",
  "0016_rain_adjustment.sql",
  "0017_adjustment_evaluation_export.sql",
];
const EXPECTED_MIGRATION_CHECKSUMS = [
  "f4264482606a3476b4b54e3089e66c900f0ad5104ea678a9bf17e5e5ba385a62",
  "954731c9ddb791c85defc3bffd887a3d30e9ff9fb5c9dac96e050ac52a796f03",
  "250e9b49176fda732b23f797a6dbce6878ca93818fa1bf6a408b5d6611ee5a48",
  "5487c90a4e70f8e049f9ed48c3ee3f025eb9f1908ed04a1febfb736c22f30d34",
  "462be10d1a35419b52068d09ae5254134e0168fed0ada66f957c87d290c8e6f1",
  "33f3fc5b1b475add0e862b45488a4a3482435df0ae674599005bafd37c3886a8",
  "4e2ed3037ea82011e947e48102149a6ea6456e5fb9f504436f7ec35f6e191390",
  "3934568dcdbb840f5026a09bab676bede7ad7d2571c1d7415efdabb902dd8b81",
  "e144415b6ac8e19338cc4b53d7daeffbf3d32650af880ab7acb809c6f71ded20",
  "2590423b7787b0e5e5668bfa4af21db775b8926540b1d2e339d6656aa3f27dc8",
  "ebf5789636095982e6010a868cdfdd4d7449ca79fb1057adf3b8dd8f49a337dc",
  "663ed323104bb70f4d0e735c3b546fb111038be4d0fee83e0c76c018cf0da15c",
  "e05f0397529108641ad9b9eb5381ac968f35449514fe4d8ed0ac502c9d5bf3ee",
  "2a311e2effcc975442c1011c627c125521cc4396323b32f0d0c4f92ea1ccba04",
  "2f560ccec001246a3caa5d27476900b2694dfb7cc0701b35181265ef1fb78822",
  "9fa5659c032dc21fdf82dca693fa962d8b5212d1192ae35c4cfd04bef94be5f9",
  "6f18210453a18f0deefe95a0b70657cc4c4dd3fc20d0bab72f847ee04958ab90",
];
const EXPECTED_MIGRATION_HISTORY_SHA256 = createHash("sha256").update(
  EXPECTED_MIGRATION_NAMES.map((name, index) =>
    `${name}:${EXPECTED_MIGRATION_CHECKSUMS[index]}`).join("\n"),
).digest("hex");
const EXPECTED_COMPLETE_MIGRATION_NAMES = [
  ...EXPECTED_MIGRATION_NAMES,
  "0018_adjustment_maintenance_v2.sql",
];
const EXPECTED_COMPLETE_MIGRATION_CHECKSUMS = [
  ...EXPECTED_MIGRATION_CHECKSUMS,
  "c13d2c2c39096887712ae97f0b863f8a7ab52074ca320575f8c4c1f9b72a50a3",
];
const EXPECTED_COMPLETE_MIGRATION_HISTORY_SHA256 = createHash("sha256").update(
  EXPECTED_COMPLETE_MIGRATION_NAMES.map(
    // serialize the exact additive ledger in database order
    (name, index) => `${name}:${EXPECTED_COMPLETE_MIGRATION_CHECKSUMS[index]}`,
  ).join("\n"),
).digest("hex");
const EXPECTED_RECURRING_MIGRATION_NAMES = [
  ...EXPECTED_COMPLETE_MIGRATION_NAMES,
  "0019_adjustment_maintenance_recurring.sql",
];
const EXPECTED_RECURRING_MIGRATION_CHECKSUMS = [
  ...EXPECTED_COMPLETE_MIGRATION_CHECKSUMS,
  "f2d9fa34a041449443741963833de798d696eb3c6096c6bd02f81769619dfd44",
];
const EXPECTED_RECURRING_MIGRATION_HISTORY_SHA256 = createHash("sha256").update(
  EXPECTED_RECURRING_MIGRATION_NAMES.map(
    // bind the additive recurring contract without admitting arbitrary extra migrations
    (name, index) => `${name}:${EXPECTED_RECURRING_MIGRATION_CHECKSUMS[index]}`,
  ).join("\n"),
).digest("hex");
const EXPECTED_FRONTIER_MIGRATION_NAMES = [
  ...EXPECTED_RECURRING_MIGRATION_NAMES,
  "0020_adjustment_revision_frontier.sql",
];
const EXPECTED_FRONTIER_MIGRATION_CHECKSUMS = [
  ...EXPECTED_RECURRING_MIGRATION_CHECKSUMS,
  "56027b4f2cb2c3c83e746f14dac753b75d573753934898fa7a52f78faa56499c",
];
const EXPECTED_FRONTIER_MIGRATION_HISTORY_SHA256 = createHash("sha256").update(
  EXPECTED_FRONTIER_MIGRATION_NAMES.map(
    // bind the frozen server-issued ordinal and pointer contract
    (name, index) => `${name}:${EXPECTED_FRONTIER_MIGRATION_CHECKSUMS[index]}`,
  ).join("\n"),
).digest("hex");
const EXPECTED_ROLLING_MIGRATION_NAMES = [
  ...EXPECTED_FRONTIER_MIGRATION_NAMES,
  "0021_adjustment_rolling_registration.sql",
];
const EXPECTED_ROLLING_MIGRATION_CHECKSUMS = [
  ...EXPECTED_FRONTIER_MIGRATION_CHECKSUMS,
  "ca29db99377001fca2e2e4268fd80d1cbe7b876f71f2d5ba04e575712cb9f13b",
];
const EXPECTED_ROLLING_MIGRATION_HISTORY_SHA256 = createHash("sha256").update(
  EXPECTED_ROLLING_MIGRATION_NAMES.map(
    // bind the additive rolling schedule without reinterpreting legacy registrations
    (name, index) => `${name}:${EXPECTED_ROLLING_MIGRATION_CHECKSUMS[index]}`,
  ).join("\n"),
).digest("hex");
const EXPECTED_CATALOG_MIGRATION_NAMES = [
  ...EXPECTED_ROLLING_MIGRATION_NAMES,
  "0022_forecast_product_catalog.sql",
];
const EXPECTED_CATALOG_MIGRATION_CHECKSUMS = [
  ...EXPECTED_ROLLING_MIGRATION_CHECKSUMS,
  "8d04170868414cd17e78cde2a4d6dd8144dfd606cba4e171686523f045cb77d2",
];
const EXPECTED_CATALOG_MIGRATION_HISTORY_SHA256 = createHash("sha256").update(
  EXPECTED_CATALOG_MIGRATION_NAMES.map(
    // bind the additive product catalog without changing the retained epoch
    (name, index) => `${name}:${EXPECTED_CATALOG_MIGRATION_CHECKSUMS[index]}`,
  ).join("\n"),
).digest("hex");
const EXPECTED_INDEX_MIGRATION_NAMES = [
  ...EXPECTED_CATALOG_MIGRATION_NAMES,
  "0023_forecast_product_read_index.sql",
];
const EXPECTED_INDEX_MIGRATION_CHECKSUMS = [
  ...EXPECTED_CATALOG_MIGRATION_CHECKSUMS,
  "e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a",
];
const EXPECTED_INDEX_MIGRATION_HISTORY_SHA256 = createHash("sha256").update(
  EXPECTED_INDEX_MIGRATION_NAMES.map(
    // bind the additive forecast read index without changing earlier ledgers
    (name, index) => `${name}:${EXPECTED_INDEX_MIGRATION_CHECKSUMS[index]}`,
  ).join("\n"),
).digest("hex");
const EXPECTED_MIGRATION_LEDGERS = [
  {
    checksums: EXPECTED_MIGRATION_CHECKSUMS,
    historySha256: EXPECTED_MIGRATION_HISTORY_SHA256,
    names: EXPECTED_MIGRATION_NAMES,
  },
  {
    checksums: EXPECTED_COMPLETE_MIGRATION_CHECKSUMS,
    historySha256: EXPECTED_COMPLETE_MIGRATION_HISTORY_SHA256,
    names: EXPECTED_COMPLETE_MIGRATION_NAMES,
  },
  {
    checksums: EXPECTED_RECURRING_MIGRATION_CHECKSUMS,
    historySha256: EXPECTED_RECURRING_MIGRATION_HISTORY_SHA256,
    names: EXPECTED_RECURRING_MIGRATION_NAMES,
  },
  {
    checksums: EXPECTED_FRONTIER_MIGRATION_CHECKSUMS,
    historySha256: EXPECTED_FRONTIER_MIGRATION_HISTORY_SHA256,
    names: EXPECTED_FRONTIER_MIGRATION_NAMES,
  },
  {
    checksums: EXPECTED_ROLLING_MIGRATION_CHECKSUMS,
    historySha256: EXPECTED_ROLLING_MIGRATION_HISTORY_SHA256,
    names: EXPECTED_ROLLING_MIGRATION_NAMES,
  },
  {
    checksums: EXPECTED_CATALOG_MIGRATION_CHECKSUMS,
    historySha256: EXPECTED_CATALOG_MIGRATION_HISTORY_SHA256,
    names: EXPECTED_CATALOG_MIGRATION_NAMES,
  },
  {
    checksums: EXPECTED_INDEX_MIGRATION_CHECKSUMS,
    historySha256: EXPECTED_INDEX_MIGRATION_HISTORY_SHA256,
    names: EXPECTED_INDEX_MIGRATION_NAMES,
  },
];
const RAIN_FIXED_GAUGE_TARGET_SOURCE_CATALOG_VERSION =
  "adjustment-rain-fixed-gauge-target-source-catalog/v1";
const RAIN_FIXED_GAUGE_TARGET_SOURCE_VERSION = "rain-fixed-gauge-target-source/v1";
const RAIN_FIXED_GAUGE_TARGET_RUN_VERSION = "adjustment-rain-fixed-gauge-target-run/v1";
const RAIN_FIXED_GAUGE_TARGET_ADAPTER_VERSION = "rain-fixed-gauge-target/v1";
const RAIN_FIXED_GAUGE_TARGET_STATIONS = Object.freeze([
  { locationId: 64255, deviceId: 175727, serial: "ST-00054713", latitude: 47.95008, longitude: -122.43982 },
  { locationId: 225947, deviceId: 1239187, serial: "ST-00212830", latitude: 47.94215, longitude: -122.42542 },
  { locationId: 38270, deviceId: 115866, serial: "ST-00157152", latitude: 47.95293, longitude: -122.41414 },
  { locationId: 168853, deviceId: 401592, serial: "ST-00170845", latitude: 47.95498, longitude: -122.44074 },
  { locationId: 126537, deviceId: 313016, serial: "ST-00134621", latitude: 47.9582, longitude: -122.44274 },
  { locationId: 201058, deviceId: 466938, serial: "ST-00194085", latitude: 47.96244, longitude: -122.43369 },
  { locationId: 203055, deviceId: 470937, serial: "ST-00198967", latitude: 47.96505, longitude: -122.4241 },
  { locationId: 66270, deviceId: 180230, serial: "ST-00173167", latitude: 47.93134, longitude: -122.42912 },
  { locationId: 34768, deviceId: 107388, serial: "ST-00020495", latitude: 47.91752, longitude: -122.41112 },
  { locationId: 88159, deviceId: 230560, serial: "ST-00094734", latitude: 47.91563, longitude: -122.41845 },
  { locationId: 126197, deviceId: 312302, serial: "ST-00129187", latitude: 47.91413, longitude: -122.41471 },
  { locationId: 27140, deviceId: 87271, serial: "ST-00000360", latitude: 47.98707, longitude: -122.46295 },
]);
const ROW_KINDS = new Set([
  "rain_adjustment_run",
  "rain_claim",
  "rain_receipt",
  "target_revision_diagnostic",
  "temperature_hour",
  "temperature_run",
]);
const PAYLOAD_KEYS = new Map([
  ["temperature_run", [
    "adapterVersion",
    "contentHash",
    "firstReceivedAt",
    "id",
    "lastReceivedAt",
    "modelCycle",
    "providerResponseSha256",
    "recentErrorState",
    "recentErrorStateSha256",
    "revisionCount",
    "runInitializedAt",
    "stateReason",
    "stateStatus",
    "upstreamModel",
  ]],
  ["temperature_hour", [
    "contentHash",
    "modelLeadHours",
    "rawRelativeHumidityPercent",
    "rawTemperatureC",
    "rawWindSpeedMps",
    "runId",
    "validAt",
  ]],
  ["rain_claim", [
    "attempt",
    "claimedAt",
    "id",
    "kind",
    "policy",
    "policySha256",
    "release",
    "runInitializedAt",
    "slotKey",
    "stationId",
    "windowEndExclusive",
    "windowStart",
  ]],
  ["rain_receipt", [
    "availableByDecision",
    "bodyBytes",
    "bodySha256",
    "claimId",
    "completedAt",
    "compressedBytes",
    "errorCode",
    "httpStatus",
    "metadata",
    "outcome",
    "parserVersion",
    "recordedAt",
    "rowCount",
    "startedAt",
  ]],
  ["rain_adjustment_run", [
    "decisionAt",
    "firstReceivedAt",
    "forecastClaimId",
    "generatedAt",
    "hours",
    "inputSha256",
    "modelSha256",
    "runInitializedAt",
  ]],
  ["target_revision_diagnostic", [
    "adapterContract",
    "contributorContentHashesSha256",
    "contributorRevisionSha256",
    "firstReceivedAt",
    "maxRevisionCount",
    "maxSourceReceiptAt",
    "physicalStationKey",
    "providerKey",
    "recordCount",
    "revisedRecordCount",
    "sourceConfigFingerprint",
    "sourceId",
    "sourceKey",
    "sourceKeys",
    "validAt",
  ]],
]);

export const ADJUSTMENT_RELEASE_CAPACITY_VERSION = "adjustment-release-capacity/v3";
export const ADJUSTMENT_RELEASE_INVENTORY_VERSION = "adjustment-release-inventory/v1";
export const ADJUSTMENT_FAMILY_RELEASE_INVENTORY_VERSION = "adjustment-family-release-inventory/v1";
export const ADJUSTMENT_FAMILY_RELEASE_CAPACITY_VERSION = "adjustment-family-release-capacity/v1";
export const ADJUSTMENT_FAMILY_RELEASE_COMPENSATION_SCOPE = "family-only-new-release-compensation";
export const ADJUSTMENT_INERT_V14_CAPACITY_VERSION = "adjustment-inert-v14-release-capacity/v1";
const ADJUSTMENT_INERT_V14_SOURCE_RELEASE = "2026.10.09-1";
const ADJUSTMENT_INERT_V14_COMPENSATION_SCOPE = "fixed-inert-v14-whole-release-source-restore";
const ADJUSTMENT_INERT_V14_SOURCE_REFERENCES = new Map([
  ["server", "ghcr.io/anstosa/weather-server@sha256:fb140b46d6eaea463ba2d10dc74303eac515135a37c21ddb746cdce744fd23ab"],
  ["web", "ghcr.io/anstosa/weather-web@sha256:fdcb2d10da4c9ed5ec8651bafa96c9d2b240b66b85db85619b909d6e144e2d7b"],
]);
export const ADJUSTMENT_FULL_V14_CAPACITY_VERSION = "adjustment-full-v14-release-capacity/v1";
const ADJUSTMENT_FULL_V14_SOURCE_RELEASE = "2026.10.09-2";
const ADJUSTMENT_FULL_V14_TARGET_RELEASE = "2026.10.09-3";
const ADJUSTMENT_FULL_V14_SOURCE_COMMIT = "f6daa89d135661eb60dd5479c5d0d6c23e7198a9";
const ADJUSTMENT_FULL_V14_COMPENSATION_SCOPE = "fixed-full-v14-whole-release-source-restore";
const ADJUSTMENT_FULL_V14_SOURCE_REFERENCES = new Map([
  ["server", "ghcr.io/anstosa/weather-server@sha256:587dd0d34ed19086986a8081f7889fb10af2146d0d3f9ba46914389a787e5437"],
  ["web", "ghcr.io/anstosa/weather-web@sha256:34d824b03c5f84289b8f38028e751f183a953565fc929260c09b07c9e64f15ba"],
]);
export const DASHBOARD_WEB_RELEASE_CAPACITY_VERSION = "dashboard-web-release-capacity/v1";
export const NORMAL_RELEASE_CAPACITY_SCHEMA_VERSION = "normal-capacity-v1";
const DASHBOARD_WEB_RELEASE_INVENTORY_VERSION = "dashboard-web-release-inventory/v1";
const DASHBOARD_WEB_RELEASE_COMPENSATION_SCOPE = "fixed-core-web-only-source-restore";
const DASHBOARD_WEB_RELEASE_CORE_COMMIT = "56b327d9c750946f6f6963b6fe1fa5c9bba791ca";
export const BLUEBERRY_PROTECTED_FREE_BYTES = 2_030_043_136;
export const BLUEBERRY_NEXT_CAPTURE_BYTES = 4_112_384;
export const ADJUSTMENT_RUNTIME_PACKAGE_MAX_BYTES = 8 * 1_024 * 1_024;
export const ADJUSTMENT_RELEASE_CONTROL_MAX_BYTES = 4 * 1_024 * 1_024 * 1_024;
export const ADJUSTMENT_RELEASE_FUTURE_STATE_BYTES = 1 * 1_024 * 1_024;
export const ADJUSTMENT_RELEASE_ENGINE_METADATA_BYTES = 64 * 1_024 * 1_024;
export const ADJUSTMENT_RELEASE_PULL_SCRATCH_BYTES = 64 * 1_024 * 1_024;
export const ADJUSTMENT_RELEASE_COMPATIBILITY_FIXTURE_BYTES = 64 * 1_024 * 1_024;
export const ADJUSTMENT_RELEASE_COMPATIBILITY_FIXTURE_INODES = 4_096;
export const ADJUSTMENT_RELEASE_ENGINE_METADATA_INODES = 4_096;
export const ADJUSTMENT_RELEASE_CONTROL_GROWTH_INODES = 128;
const ADJUSTMENT_RELEASE_MAXIMUM_COMPRESSED_IMAGE_BYTES = 2 * 1_024 * 1_024 * 1_024;
const ADJUSTMENT_RELEASE_MAXIMUM_UNPACKED_LAYER_BYTES = 4 * 1_024 * 1_024 * 1_024;
const ADJUSTMENT_RELEASE_MAXIMUM_LAYER_INODES = 1_000_000;
const ADJUSTMENT_RELEASE_BLOCK_BYTES = 4_096;
const ADJUSTMENT_RELEASE_ROLES = ["source", "target", "compensating"];
const ADJUSTMENT_RELEASE_RUNTIMES = ["server", "web"];
const ADJUSTMENT_RELEASE_SOURCE_RELEASE = "2026.10.07-3";
const ADJUSTMENT_RELEASE_COMPENSATION_SCOPE =
  "fixed-inert-v13-whole-release-source-restore";
const ADJUSTMENT_RELEASE_SOURCE_REFERENCES = new Map([
  ["server", "ghcr.io/anstosa/weather-server@sha256:d0688756c33875940f67fbb782d9e67a2405a811d2aca40bd155f6ceffa71a74"],
  ["web", "ghcr.io/anstosa/weather-web@sha256:739d063cd911bcd7c6637082e356889ef60ac7a76caabc737070703fdf83746f"],
]);
const ADJUSTMENT_RELEASE_REFERENCE =
  /^ghcr\.io\/anstosa\/weather-(server|web)@sha256:([a-f0-9]{64})$/u;
const NORMAL_CAPACITY_OPERATION_ID_PATTERN = /^[a-z0-9][a-z0-9-]{0,127}$/u;
const NORMAL_CAPACITY_SYSTEM_IDENTIFIER_PATTERN = /^[1-9][0-9]{0,19}$/u;
const NORMAL_CAPACITY_CONTAINER_ID_PATTERN = /^[a-f0-9]{64}$/u;
const NORMAL_CAPACITY_COMPOSE_PROJECT_PATTERN = /^[a-z0-9][a-z0-9_-]{0,62}$/u;
const NORMAL_CAPACITY_MAXIMUM_AGE_MILLISECONDS = 5 * 60 * 1_000;
const NORMAL_CAPACITY_MAXIMUM_FUTURE_MILLISECONDS = 30 * 1_000;
const NORMAL_CAPACITY_SCRATCH_BYTES = 256 * 1_024 * 1_024;
const NORMAL_CAPACITY_INODE_FLOOR = 32_768;
const NORMAL_CAPACITY_FIXTURE_INODES = 4_096;
const NORMAL_CAPACITY_BACKUP_HEADROOM_BYTES = 1_024 * 1_024 * 1_024;
const NORMAL_CAPACITY_SOURCE_LEDGER_SHA256 =
  "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";
const NORMAL_CAPACITY_TARGET_LEDGER_SHA256 =
  "c8b43b714eb555027ea7439a813831ab7326ad951b800362f35b2343330da81f";
const ADJUSTMENT_RELEASE_DIGEST = /^sha256:[a-f0-9]{64}$/u;
const ADJUSTMENT_FAMILY_RELEASE_ROOT =
  "/opt/weather/current/deploy/state/adjustment-release-state";
const ADJUSTMENT_FAMILY_CATALOG_PATH =
  "/opt/weather/current/deploy/state/adjustment-candidate-catalog.json";
const ADJUSTMENT_FAMILY_SETTINGS_PATH =
  "/var/lib/weather/xweather/forecast-adjustment-settings.json";
const ADJUSTMENT_FAMILY_BASELINE_RELEASE = "2026.10.09-1";
const ADJUSTMENT_FAMILY_BASELINE_COMMIT =
  "56b327d9c750946f6f6963b6fe1fa5c9bba791ca";
const ADJUSTMENT_FAMILY_TRANSACTION_VERSION =
  "adjustment-family-release-transaction/v1";
const ADJUSTMENT_FAMILY_OPERATOR_OFF_TRANSACTION_VERSION =
  "adjustment-family-operator-off-transaction/v1";
const ADJUSTMENT_FAMILY_CURRENT_VERSION =
  "adjustment-family-release-current/v1";
const ADJUSTMENT_FAMILY_BOOTSTRAP_CURRENT_VERSION =
  "adjustment-family-release-bootstrap-current/v1";
const ADJUSTMENT_FAMILY_RELEASE_LINEAGE_VERSION =
  "adjustment-family-release-lineage/v2";
const ADJUSTMENT_FAMILY_CATALOG_VERSION =
  "adjustment-installed-candidate-catalog/v1";
const ADJUSTMENT_FAMILY_CATALOG_V2_VERSION =
  "adjustment-installed-candidate-catalog/v2";
const ADJUSTMENT_FAMILY_CATALOG_V3_VERSION =
  "adjustment-installed-candidate-catalog/v3";
const ADJUSTMENT_FAMILY_MAXIMUM_STATE_BYTES = 512 * 1_024;
const ADJUSTMENT_FAMILY_LINEAGE_MAXIMUM_TRANSACTIONS = 384;
const ADJUSTMENT_FAMILY_MAXIMUM_CATALOG_BYTES = 256 * 1_024;
const ADJUSTMENT_FAMILY_LOCK_ROOT = "/opt/weather/current/deploy/state";
const ADJUSTMENT_FAMILY_REGISTRIES = new Map([
  ["temperature", "config/forecast-adjustments/ballydidean-temperature-canary.json"],
  ["wind", "config/forecast-adjustments/ballydidean-wind-canary.json"],
  ["rain", "config/forecast-adjustments/ballydidean-rain-runtime.json"],
]);

// round a capacity value to host filesystem blocks
function adjustmentReleaseAllocated(bytes) {
  // unknown or inexact values cannot authorize a release
  if (!Number.isSafeInteger(bytes) || bytes < 0 ||
    bytes > Number.MAX_SAFE_INTEGER - ADJUSTMENT_RELEASE_BLOCK_BYTES) {
    throw new RangeError("invalid measured release bytes");
  }
  return Math.ceil(bytes / ADJUSTMENT_RELEASE_BLOCK_BYTES) *
    ADJUSTMENT_RELEASE_BLOCK_BYTES;
}

// preserve exact aggregate accounting
function adjustmentReleaseSum(...values) {
  let total = 0;

  // reject precision loss in every bucket
  for (const value of values) {
    total += value;
    if (!Number.isSafeInteger(total)) {
      throw new RangeError("release capacity accounting overflow");
    }
  }
  return total;
}

// freeze Docker parent-chain identities
export function dockerChainIdentity(diffIds) {
  // one diff-id beneath a different parent is separate storage
  if (!Array.isArray(diffIds) || diffIds.length === 0 ||
    diffIds.some((value) => !ADJUSTMENT_RELEASE_DIGEST.test(value))) {
    throw new RangeError("invalid docker layer chain");
  }
  let identity = diffIds[0];

  // match Docker's recursive parent-plus-diff identity
  for (const diffId of diffIds.slice(1)) {
    identity = `sha256:${createHash("sha256").update(`${identity} ${diffId}`).digest("hex")}`;
  }
  return identity;
}

// bind raw OCI manifests and configs to one closed release inventory
export function collectAdjustmentReleaseCapacityInventory(record) {
  return collectReleaseCapacityInventory(record, "v13");
}

// admit only the inactive code handoff from the already reviewed v13 release
export function evaluateAdjustmentInertV14ReleaseCapacity(record) {
  // this distinct contract never authorizes a model or another predecessor
  if (record.sourceRelease !== ADJUSTMENT_INERT_V14_SOURCE_RELEASE ||
    record.compensationScope !== ADJUSTMENT_INERT_V14_COMPENSATION_SCOPE ||
    record.version !== "adjustment-inert-v14-release-inventory/v1") {
    throw new Error("inert v14 capacity scope is invalid");
  }
  const result = evaluateAdjustmentReleaseCapacity(collectReleaseCapacityInventory(record, "v14"));
  return Object.freeze({ ...result, compensationScope: ADJUSTMENT_INERT_V14_COMPENSATION_SCOPE,
    contractVersion: ADJUSTMENT_INERT_V14_CAPACITY_VERSION, sourceRelease: ADJUSTMENT_INERT_V14_SOURCE_RELEASE });
}

// admit only the full maintenance handoff from the published compatibility bridge
export function evaluateAdjustmentFullV14ReleaseCapacity(record) {
  // keep the second handoff disjoint from the original source-one recovery scope
  if (record.sourceRelease !== ADJUSTMENT_FULL_V14_SOURCE_RELEASE ||
    record.compensationScope !== ADJUSTMENT_FULL_V14_COMPENSATION_SCOPE ||
    record.version !== "adjustment-full-v14-release-inventory/v1") {
    throw new Error("full v14 capacity scope is invalid");
  }
  const result = evaluateAdjustmentReleaseCapacity(
    collectReleaseCapacityInventory(record, "full-v14"),
  );
  return Object.freeze({
    ...result,
    compensationScope: ADJUSTMENT_FULL_V14_COMPENSATION_SCOPE,
    contractVersion: ADJUSTMENT_FULL_V14_CAPACITY_VERSION,
    sourceRelease: ADJUSTMENT_FULL_V14_SOURCE_RELEASE,
  });
}

// meter an independent dashboard without granting model or schema authority
export function evaluateDashboardWebReleaseCapacity(record) {
  // retain the one authenticated core and exact source-web restoration
  if (record.version !== DASHBOARD_WEB_RELEASE_INVENTORY_VERSION ||
    record.sourceRelease !== ADJUSTMENT_INERT_V14_SOURCE_RELEASE ||
    record.compensationScope !== DASHBOARD_WEB_RELEASE_COMPENSATION_SCOPE ||
    !Array.isArray(record.images) || record.images.length !== 6) {
    throw new Error("dashboard web capacity scope is invalid");
  }
  const roles = new Map(record.images.map(
    // distinguish every literal ownership role before physical accounting
    (image) => [`${image.role}/${image.runtime}`, image.reference],
  ));
  const sourceWeb = roles.get("source/web");
  const targetWeb = roles.get("target/web");

  // every server role remains the exact deployed core image
  for (const role of ADJUSTMENT_RELEASE_ROLES) {
    // web-only compensation cannot substitute another server
    if (roles.get(`${role}/server`) !== ADJUSTMENT_INERT_V14_SOURCE_REFERENCES.get("server")) {
      throw new Error("dashboard web release must preserve the exact core server");
    }
  }
  // require distinct pinned web bytes and literal source restoration
  if (roles.size !== 6 || ADJUSTMENT_RELEASE_REFERENCE.exec(sourceWeb)?.[1] !== "web" ||
    ADJUSTMENT_RELEASE_REFERENCE.exec(targetWeb)?.[1] !== "web" ||
    targetWeb === sourceWeb || roles.get("compensating/web") !== sourceWeb) {
    throw new Error("dashboard web compensation must restore the exact source web");
  }
  const result = evaluateAdjustmentReleaseCapacity(collectReleaseCapacityInventory(record, "dashboard-web"));
  return Object.freeze({ ...result,
    compensationScope: DASHBOARD_WEB_RELEASE_COMPENSATION_SCOPE,
    contractVersion: DASHBOARD_WEB_RELEASE_CAPACITY_VERSION,
    sourceRelease: ADJUSTMENT_INERT_V14_SOURCE_RELEASE });
}

// bind a distinct family transaction to literal new compensation images
export function evaluateAdjustmentFamilyReleaseCapacity(record) {
  requireExactKeys(record, ["actionSha256", "compensationScope", "family", "freeBytes",
    "freeInodes", "images", "inventory", "measuredAt", "retainedControlBytes",
    "runtimePackageBytes", "sourceRelease", "version"], "family release capacity collection");

  // refuse caller-shaped scopes or mutable source aliases
  if (record.version !== ADJUSTMENT_FAMILY_RELEASE_INVENTORY_VERSION ||
    record.compensationScope !== ADJUSTMENT_FAMILY_RELEASE_COMPENSATION_SCOPE ||
    !/^[a-f0-9]{64}$/u.test(record.actionSha256) ||
    !["rain", "temperature", "wind"].includes(record.family) ||
    typeof record.sourceRelease !== "string" ||
    !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d*$/u.test(record.sourceRelease)) {
    throw new Error("family release capacity scope is invalid");
  }
  const { actionSha256, family, ...inventory } = record;
  const result = evaluateAdjustmentReleaseCapacity(collectReleaseCapacityInventory(inventory, "family"));
  return Object.freeze({ ...result, actionSha256, family,
    compensationScope: ADJUSTMENT_FAMILY_RELEASE_COMPENSATION_SCOPE,
    contractVersion: ADJUSTMENT_FAMILY_RELEASE_CAPACITY_VERSION,
    sourceRelease: record.sourceRelease });
}

// share measured OCI accounting without broadening either public scope
function collectReleaseCapacityInventory(record, scope) {
  const expectedKeys = [
    "compensationScope",
    "freeBytes",
    "freeInodes",
    "images",
    "inventory",
    "measuredAt",
    "retainedControlBytes",
    "runtimePackageBytes",
    "sourceRelease",
    "version",
  ];
  requireExactKeys(record, expectedKeys, "release capacity collection");

  // admit only the fixed inert v13 whole-release recovery transaction
  if ((scope === "v13" && (record.version !== ADJUSTMENT_RELEASE_INVENTORY_VERSION ||
    record.sourceRelease !== ADJUSTMENT_RELEASE_SOURCE_RELEASE ||
    record.compensationScope !== ADJUSTMENT_RELEASE_COMPENSATION_SCOPE)) ||
    !Array.isArray(record.images) || record.images.length !== 6 ||
    !Array.isArray(record.inventory) || record.inventory.length > 256) {
    throw new Error("release capacity collection scope is invalid");
  }
  const inventory = record.inventory.map((entry) => {
    requireExactKeys(entry, ["allocatedBytes", "chainId"], "allocated Docker layer");
    if (!ADJUSTMENT_RELEASE_DIGEST.test(entry.chainId) ||
      adjustmentReleaseAllocated(entry.allocatedBytes) !== entry.allocatedBytes) {
      throw new Error("allocated Docker layer is invalid");
    }
    return { allocatedBytes: entry.allocatedBytes, chainId: entry.chainId };
  });
  const inventoryChains = new Set(inventory.map(({ chainId }) => chainId));

  // reject duplicate physical inventory claims
  if (inventoryChains.size !== inventory.length) {
    throw new Error("allocated Docker layer is duplicated");
  }
  const images = record.images.map((entry) => {
    requireExactKeys(entry, [
      "configBytes",
      "manifestBytes",
      "reference",
      "role",
      "runtime",
      "unpackedLayers",
    ], "release image collection");
    const reference = ADJUSTMENT_RELEASE_REFERENCE.exec(entry.reference);

    // bind each fixed repository to its runtime and one immutable manifest
    if (reference === null || reference[1] !== entry.runtime ||
      !ADJUSTMENT_RELEASE_ROLES.includes(entry.role) ||
      !ADJUSTMENT_RELEASE_RUNTIMES.includes(entry.runtime) ||
      typeof entry.manifestBytes !== "string" ||
      typeof entry.configBytes !== "string" ||
      Buffer.byteLength(entry.manifestBytes) > 1_048_576 ||
      Buffer.byteLength(entry.configBytes) > 4_194_304 ||
      !Array.isArray(entry.unpackedLayers) || entry.unpackedLayers.length > 128) {
      throw new Error("release image collection is invalid");
    }
    const manifestDigest = `sha256:${createHash("sha256").update(entry.manifestBytes).digest("hex")}`;

    // verify the exact raw manifest against the pinned reference
    if (manifestDigest !== `sha256:${reference[2]}`) {
      throw new Error("release image manifest digest differs");
    }
    const manifest = JSON.parse(entry.manifestBytes);
    const config = JSON.parse(entry.configBytes);
    requireExactKeys(manifest, ["config", "layers", "mediaType", "schemaVersion"], "OCI manifest");

    // require the closed single-platform OCI manifest layout
    if (manifest.schemaVersion !== 2 ||
      manifest.mediaType !== "application/vnd.oci.image.manifest.v1+json" ||
      !Array.isArray(manifest.layers) || manifest.layers.length < 1 ||
      manifest.layers.length > 128 ||
      typeof manifest.config !== "object" || manifest.config === null ||
      manifest.config.mediaType !== "application/vnd.oci.image.config.v1+json" ||
      !ADJUSTMENT_RELEASE_DIGEST.test(manifest.config.digest) ||
      manifest.config.size !== Buffer.byteLength(entry.configBytes) ||
      `sha256:${createHash("sha256").update(entry.configBytes).digest("hex")}` !==
        manifest.config.digest ||
      config.architecture !== "arm64" || config.os !== "linux" ||
      config.rootfs?.type !== "layers" || !Array.isArray(config.rootfs.diff_ids) ||
      config.rootfs.diff_ids.length !== manifest.layers.length ||
      entry.unpackedLayers.length !== manifest.layers.length) {
      throw new Error("release image OCI/config identity is invalid");
    }
    const diffIds = [];
    const layers = manifest.layers.map((layer, index) => {
      requireExactKeys(layer, ["digest", "mediaType", "size"], "OCI layer descriptor");
      const unpacked = entry.unpackedLayers[index];
      requireExactKeys(unpacked, ["allocatedBytes", "blobDigest", "entryInodes"],
        "unpacked OCI layer measurement");

      // accept only measured gzip layers bound to their exact descriptor
      if (layer.mediaType !== "application/vnd.oci.image.layer.v1.tar+gzip" ||
        !ADJUSTMENT_RELEASE_DIGEST.test(layer.digest) ||
        !Number.isSafeInteger(layer.size) || layer.size < 1 ||
        unpacked.blobDigest !== layer.digest ||
        adjustmentReleaseAllocated(unpacked.allocatedBytes) !== unpacked.allocatedBytes ||
        !Number.isSafeInteger(unpacked.entryInodes) || unpacked.entryInodes < 1) {
        throw new Error("release image layer measurement is invalid");
      }
      const diffId = config.rootfs.diff_ids[index];
      diffIds.push(diffId);
      return {
        blobDigest: layer.digest,
        chainId: dockerChainIdentity(diffIds),
        compressedBytes: layer.size,
        diffId,
        entryInodes: unpacked.entryInodes,
        unpackedBytes: unpacked.allocatedBytes,
      };
    });
    return {
      digest: manifestDigest,
      layers,
      metadataBytes: adjustmentReleaseSum(
        adjustmentReleaseAllocated(Buffer.byteLength(entry.manifestBytes)),
        adjustmentReleaseAllocated(Buffer.byteLength(entry.configBytes)),
      ),
      reference: entry.reference,
      role: entry.role,
      runtime: entry.runtime,
    };
  });
  const roles = new Map(images.map((image) => [`${image.role}/${image.runtime}`, image]));

  // require all six roles and literal source reuse only in this fixed rollback scope
  for (const role of ADJUSTMENT_RELEASE_ROLES) {
    for (const runtime of ADJUSTMENT_RELEASE_RUNTIMES) {
      if (!roles.has(`${role}/${runtime}`)) {
        throw new Error("release image roles are incomplete");
      }
    }
  }
  for (const runtime of ADJUSTMENT_RELEASE_RUNTIMES) {
    const sourceReference = roles.get(`source/${runtime}`).reference;

    // bind the one reviewed predecessor and its whole-release restoration images
    if (scope === "v13" && (sourceReference !== ADJUSTMENT_RELEASE_SOURCE_REFERENCES.get(runtime) ||
      sourceReference !== roles.get(`compensating/${runtime}`).reference)) {
      throw new Error("fixed inert v13 compensation must restore the exact reviewed source image");
    }
    // bind the distinct inactive v14 handoff to the exact already deployed images
    if (scope === "v14" && (sourceReference !== ADJUSTMENT_INERT_V14_SOURCE_REFERENCES.get(runtime) ||
      sourceReference !== roles.get(`compensating/${runtime}`).reference)) {
      throw new Error("fixed inert v14 compensation must restore its exact reviewed source image");
    }
    // bind the full handoff to the separately published bridge images
    if (scope === "full-v14" &&
      (sourceReference !== ADJUSTMENT_FULL_V14_SOURCE_REFERENCES.get(runtime) ||
        sourceReference !== roles.get(`compensating/${runtime}`).reference)) {
      throw new Error("full v14 compensation must restore its exact reviewed bridge image");
    }
  }
  // whole-release source restoration is not a family compensating release
  if (scope === "family" && roles.get("source/server").reference === roles.get("compensating/server").reference) {
    throw new Error("family compensation requires a new immutable server image");
  }
  return {
    engineMetadataBytes: ADJUSTMENT_RELEASE_ENGINE_METADATA_BYTES,
    freeBytes: record.freeBytes,
    freeInodes: record.freeInodes,
    futureStateBytes: ADJUSTMENT_RELEASE_FUTURE_STATE_BYTES,
    images: images.map(({ reference: _reference, ...image }) => image),
    inventory,
    maximumOwnedBytes: ADJUSTMENT_RELEASE_CONTROL_MAX_BYTES,
    measuredAt: record.measuredAt,
    nextStateInodes: 1_004,
    pullScratchBytes: ADJUSTMENT_RELEASE_PULL_SCRATCH_BYTES,
    retainedControlBytes: record.retainedControlBytes,
    runtimePackageBytes: record.runtimePackageBytes,
  };
}

// account persistent source, target and exact fixed-scope compensation ownership
export function evaluateAdjustmentReleaseCapacity(input) {
  const keys = ["engineMetadataBytes", "freeBytes", "freeInodes", "futureStateBytes",
    "images", "inventory", "maximumOwnedBytes", "measuredAt", "nextStateInodes",
    "pullScratchBytes", "retainedControlBytes", "runtimePackageBytes"];
  requireExactKeys(input, keys, "release capacity inventory");
  adjustmentReleaseAllocated(input.freeBytes);
  const available = Math.floor(input.freeBytes / ADJUSTMENT_RELEASE_BLOCK_BYTES) *
    ADJUSTMENT_RELEASE_BLOCK_BYTES;
  adjustmentReleaseAllocated(input.engineMetadataBytes);
  adjustmentReleaseAllocated(input.futureStateBytes);
  adjustmentReleaseAllocated(input.pullScratchBytes);
  adjustmentReleaseAllocated(input.retainedControlBytes);
  adjustmentReleaseAllocated(input.runtimePackageBytes);
  adjustmentReleaseAllocated(input.maximumOwnedBytes);
  const observationMilliseconds = Date.parse(input.measuredAt);

  // bind a canonical observation and all integer inode measurements
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(input.measuredAt) ||
    !Number.isFinite(observationMilliseconds) ||
    new Date(observationMilliseconds).toISOString() !== input.measuredAt ||
    input.futureStateBytes !== ADJUSTMENT_RELEASE_FUTURE_STATE_BYTES ||
    !Number.isSafeInteger(input.freeInodes) || !Number.isSafeInteger(input.nextStateInodes) ||
    input.freeInodes < 0 || input.nextStateInodes < 0 ||
    !Array.isArray(input.inventory) || !Array.isArray(input.images) || input.images.length !== 6) {
    throw new RangeError("invalid exact release inventory");
  }
  const inventory = new Map();

  // retain literal current allocated layer measurements
  for (const layer of input.inventory) {
    requireExactKeys(layer, ["allocatedBytes", "chainId"], "allocated Docker layer");
    if (!ADJUSTMENT_RELEASE_DIGEST.test(layer.chainId) || inventory.has(layer.chainId) ||
      adjustmentReleaseAllocated(layer.allocatedBytes) !== layer.allocatedBytes) {
      throw new RangeError("invalid allocated docker inventory");
    }
    inventory.set(layer.chainId, layer.allocatedBytes);
  }
  const owned = new Map();
  const layerInodes = new Map();
  const newCompressed = new Map();
  const metadata = new Map();
  const manifests = new Map();
  const roles = new Set();

  // require all literal images to coexist through restart and compensation
  for (const image of input.images) {
    requireExactKeys(image, ["digest", "layers", "metadataBytes", "role", "runtime"],
      "release capacity image");
    if (!ADJUSTMENT_RELEASE_DIGEST.test(image.digest) ||
      !ADJUSTMENT_RELEASE_ROLES.includes(image.role) ||
      !ADJUSTMENT_RELEASE_RUNTIMES.includes(image.runtime) ||
      roles.has(`${image.role}/${image.runtime}`) ||
      !Array.isArray(image.layers) || image.layers.length === 0) {
      throw new RangeError("release images must contain both runtimes for source, target and compensation");
    }
    roles.add(`${image.role}/${image.runtime}`);
    const previousMetadata = metadata.get(image.digest);
    const bytes = adjustmentReleaseAllocated(image.metadataBytes);
    if (previousMetadata !== undefined && previousMetadata !== bytes) {
      throw new RangeError("image metadata measurement differs");
    }
    metadata.set(image.digest, bytes);
    const diffIds = [];

    // deduplicate only complete parent chains
    for (const layer of image.layers) {
      requireExactKeys(layer, ["blobDigest", "chainId", "compressedBytes", "diffId",
        "entryInodes", "unpackedBytes"], "release capacity layer");
      diffIds.push(layer.diffId);
      const chainId = dockerChainIdentity(diffIds);
      if (!ADJUSTMENT_RELEASE_DIGEST.test(layer.blobDigest) || layer.chainId !== chainId ||
        !Number.isSafeInteger(layer.entryInodes) || layer.entryInodes < 1) {
        throw new RangeError("image layer identity differs");
      }
      const unpacked = adjustmentReleaseAllocated(layer.unpackedBytes);
      const prior = owned.get(chainId);
      if (prior !== undefined && prior !== unpacked) {
        throw new RangeError("shared layer measurement differs");
      }
      owned.set(chainId, unpacked);
      const priorInodes = layerInodes.get(chainId);
      if (priorInodes !== undefined && priorInodes !== layer.entryInodes) {
        throw new RangeError("shared layer inode measurement differs");
      }
      layerInodes.set(chainId, layer.entryInodes);
      const compressed = adjustmentReleaseAllocated(layer.compressedBytes);

      // only absent chains consume pull growth
      if (!inventory.has(chainId)) {
        const oldCompressed = newCompressed.get(layer.blobDigest);
        if (oldCompressed !== undefined && oldCompressed !== compressed) {
          throw new RangeError("compressed blob measurement differs");
        }
        newCompressed.set(layer.blobDigest, compressed);
      }
    }
    const manifest = JSON.stringify(image.layers.map(
      // bind all immutable and bounded layer facts
      ({ blobDigest, chainId, compressedBytes, diffId, entryInodes, unpackedBytes }) =>
        [blobDigest, chainId, compressedBytes, diffId, entryInodes, unpackedBytes],
    ));
    if (manifests.has(image.digest) && manifests.get(image.digest) !== manifest) {
      throw new RangeError("immutable image manifest measurement differs");
    }
    manifests.set(image.digest, manifest);
  }
  let retainedLayerBytes = 0;
  let newLayerBytes = 0;
  let newLayerInodes = 0;

  // use actual existing blocks and measured absent-chain bounds
  for (const [chainId, bound] of owned) {
    const measured = inventory.get(chainId);
    retainedLayerBytes = adjustmentReleaseSum(retainedLayerBytes, measured ?? bound);
    if (measured === undefined) {
      newLayerBytes = adjustmentReleaseSum(newLayerBytes, bound);
      newLayerInodes = adjustmentReleaseSum(newLayerInodes, layerInodes.get(chainId));
    }
  }
  const metadataBytes = adjustmentReleaseSum(...metadata.values());
  const packageBytes = adjustmentReleaseAllocated(input.runtimePackageBytes);
  const futureStateBytes = adjustmentReleaseAllocated(input.futureStateBytes);
  const retainedControlBytes = adjustmentReleaseAllocated(input.retainedControlBytes);
  const persistentOwnedBytes = adjustmentReleaseSum(retainedLayerBytes, metadataBytes,
    adjustmentReleaseAllocated(input.engineMetadataBytes), packageBytes,
    retainedControlBytes, futureStateBytes);
  const persistentGrowthBytes = adjustmentReleaseSum(newLayerBytes, metadataBytes,
    adjustmentReleaseAllocated(input.engineMetadataBytes), packageBytes, futureStateBytes);
  const compressedPeakBytes = adjustmentReleaseSum(...newCompressed.values());
  const pullPeakGrowthBytes = adjustmentReleaseSum(persistentGrowthBytes,
    compressedPeakBytes, adjustmentReleaseAllocated(input.pullScratchBytes));
  const compatibilityPeakGrowthBytes = adjustmentReleaseSum(
    persistentGrowthBytes, ADJUSTMENT_RELEASE_COMPATIBILITY_FIXTURE_BYTES);
  const releaseTransactionPeakGrowthBytes = Math.max(
    pullPeakGrowthBytes, compatibilityPeakGrowthBytes);
  const requiredFreeBytes = adjustmentReleaseSum(BLUEBERRY_PROTECTED_FREE_BYTES,
    BLUEBERRY_NEXT_CAPTURE_BYTES, releaseTransactionPeakGrowthBytes);
  const requiredFreeInodes = adjustmentReleaseSum(32_768, input.nextStateInodes,
    newLayerInodes, ADJUSTMENT_RELEASE_ENGINE_METADATA_INODES,
    ADJUSTMENT_RELEASE_CONTROL_GROWTH_INODES,
    ADJUSTMENT_RELEASE_COMPATIBILITY_FIXTURE_INODES);
  const reasons = [];

  // retain both byte and inode floors after the next complete capture
  if (available < requiredFreeBytes) reasons.push("protected_floor_or_transaction_peak");
  if (input.freeInodes < requiredFreeInodes) reasons.push("inode_floor");
  if (persistentOwnedBytes > input.maximumOwnedBytes) reasons.push("persistent_image_ownership");
  if (input.runtimePackageBytes > ADJUSTMENT_RUNTIME_PACKAGE_MAX_BYTES) {
    reasons.push("runtime_package_ceiling");
  }
  return Object.freeze({
    availableFreeBytes: available,
    captureReservedBytes: BLUEBERRY_NEXT_CAPTURE_BYTES,
    compatibilityFixtureBytes: ADJUSTMENT_RELEASE_COMPATIBILITY_FIXTURE_BYTES,
    compatibilityFixtureInodes: ADJUSTMENT_RELEASE_COMPATIBILITY_FIXTURE_INODES,
    compensationScope: ADJUSTMENT_RELEASE_COMPENSATION_SCOPE,
    compressedPeakBytes,
    contractVersion: ADJUSTMENT_RELEASE_CAPACITY_VERSION,
    futureStateBytes,
    imageDigests: input.images.map(({ role, runtime, digest }) => ({ digest, role, runtime })),
    measuredAt: input.measuredAt,
    newLayerInodes,
    persistentGrowthBytes,
    persistentOwnedBytes,
    protectedFreeBytes: BLUEBERRY_PROTECTED_FREE_BYTES,
    pullPeakGrowthBytes,
    releaseTransactionPeakGrowthBytes,
    reasons,
    requiredFreeBytes,
    requiredFreeInodes,
    retainedControlBytes,
    retirementCreditBytes: 0,
    sourceRelease: ADJUSTMENT_RELEASE_SOURCE_RELEASE,
    state: reasons.length === 0 ? "capacity_ready" : "capacity_blocked",
  });
}

// require one exact normal-release integer measurement
function normalCapacityInteger(value, name, minimum = 0) {
  // prohibit negative, fractional, or inexact capacity authority
  if (!Number.isSafeInteger(value) || value < minimum) {
    throw new RangeError(`${name} is invalid`);
  }
  return value;
}

// require one exact normal-release instant
function normalCapacityInstant(value, name) {
  const milliseconds = Date.parse(value);

  // prohibit normalized or non-millisecond clocks
  if (typeof value !== "string" ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(value) ||
    !Number.isFinite(milliseconds) || new Date(milliseconds).toISOString() !== value) {
    throw new TypeError(`${name} is invalid`);
  }
  return milliseconds;
}

// validate immutable release identity shared by every capacity boundary
function validateNormalCapacityReleaseIdentity(input) {
  // bind only the exact schema-21 to schema-23 successor
  if (!NORMAL_CAPACITY_OPERATION_ID_PATTERN.test(input.operationId) ||
    typeof input.sourceRelease !== "string" ||
    !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d*$/u.test(input.sourceRelease) ||
    typeof input.targetRelease !== "string" ||
    !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d*$/u.test(input.targetRelease) ||
    input.sourceRelease === input.targetRelease ||
    !HASH_PATTERN.test(input.sourceEnvSha256) ||
    !HASH_PATTERN.test(input.targetEnvSha256) ||
    input.sourceLedgerSha256 !== NORMAL_CAPACITY_SOURCE_LEDGER_SHA256 ||
    input.targetLedgerSha256 !== NORMAL_CAPACITY_TARGET_LEDGER_SHA256 ||
    !NORMAL_CAPACITY_SYSTEM_IDENTIFIER_PATTERN.test(input.systemIdentifier) ||
    BigInt(input.systemIdentifier) > 18_446_744_073_709_551_615n ||
    normalCapacityInteger(input.sourceDatabaseOid, "sourceDatabaseOid", 1) > 0xffff_ffff ||
    input.sourceSchemaVersion !== 21 || input.targetSchemaVersion !== 23 ||
    !NORMAL_CAPACITY_CONTAINER_ID_PATTERN.test(input.postgresContainerId) ||
    !NORMAL_CAPACITY_COMPOSE_PROJECT_PATTERN.test(input.postgresComposeProject)) {
    throw new Error("normal release capacity identity is invalid");
  }

  // require the literal canonical host mount source before inspection
  if (typeof input.weatherPostgresDir !== "string" || input.weatherPostgresDir === "/" ||
    resolve(input.weatherPostgresDir) !== input.weatherPostgresDir) {
    throw new Error("weatherPostgresDir is invalid");
  }
}

// validate measured filesystem identity shared by completed receipts
function validateNormalCapacityIdentity(input) {
  validateNormalCapacityReleaseIdentity(input);

  // bind the actual SQL directory and inspected immutable container image
  if (typeof input.postgresDataDirectory !== "string" ||
    input.postgresDataDirectory === "/" || resolve(input.postgresDataDirectory) !==
      input.postgresDataDirectory ||
    !ADJUSTMENT_RELEASE_DIGEST.test(input.postgresContainerImageId) ||
    !/^[^@\s]+@sha256:[a-f0-9]{64}$/u.test(input.postgresContainerImageReference)) {
    throw new Error("normal release PostgreSQL identity is invalid");
  }
  normalCapacityInteger(input.weatherPostgresDirFilesystemDevice,
    "weatherPostgresDirFilesystemDevice", 1);
  normalCapacityInteger(input.postgresDataFilesystemDevice,
    "postgresDataFilesystemDevice", 1);
}

// bind four immutable source and target images to their repository roles
function validateNormalStageImageReferences(imageReferences) {
  if (!Array.isArray(imageReferences) || imageReferences.length !== 4) {
    throw new Error("normal stage image references are invalid");
  }
  const images = new Map();

  // reject aliases, role duplication, or repository/runtime mismatch
  for (const image of imageReferences) {
    requireExactKeys(image, ["reference", "role", "runtime"],
      "normal stage image reference");
    const parsed = ADJUSTMENT_RELEASE_REFERENCE.exec(image.reference);
    const role = `${image.role}/${image.runtime}`;

    // require one immutable image for every source and target runtime
    if (!["source", "target"].includes(image.role) ||
      !ADJUSTMENT_RELEASE_RUNTIMES.includes(image.runtime) || parsed?.[1] !== image.runtime ||
      images.has(role)) {
      throw new Error("normal stage image references are invalid");
    }
    images.set(role, image.reference);
  }
  return ["source/server", "source/web", "target/server", "target/web"].map((role) => {
    const reference = images.get(role);

    // require the complete four-role matrix
    if (reference === undefined) throw new Error("normal stage image references are invalid");
    return reference;
  });
}

// require one evaluator-owned capacity clock
function normalCapacityTrustedInstant(value) {
  // accept only a finite real Date supplied by the caller's trusted boundary
  if (!(value instanceof Date) || !Number.isFinite(value.getTime())) {
    throw new TypeError("trusted normal capacity clock is invalid");
  }
  return value.toISOString();
}

// validate one observation against the evaluator-owned current time
function validateNormalCapacityClock(input, trustedNow) {
  const capturedAt = normalCapacityInstant(input.capturedAt, "capturedAt");
  const evaluatedAtValue = normalCapacityTrustedInstant(trustedNow);
  const evaluatedAt = Date.parse(evaluatedAtValue);
  const age = evaluatedAt - capturedAt;

  // refuse stale observations or capture clocks later than evaluation
  if (age > NORMAL_CAPACITY_MAXIMUM_AGE_MILLISECONDS || age < 0) {
    throw new Error("normal release capacity observation is stale");
  }
  return evaluatedAtValue;
}

// close one canonical normal-release receipt
function closeNormalCapacityReceipt(
  input,
  evaluatedAt,
  operation,
  requiredBytes,
  requiredInodes,
  reasons,
) {
  const receipt = {
    ...input,
    evaluatedAt,
    operation,
    reasons: [...reasons].sort(),
    requiredBytes,
    requiredInodes,
    schemaVersion: NORMAL_RELEASE_CAPACITY_SCHEMA_VERSION,
    state: reasons.length === 0 ? "capacity_ready" : "capacity_blocked",
  };
  return Object.freeze({
    ...receipt,
    receiptSha256: sha256(Buffer.from(canonicalJson(receipt))),
  });
}

// evaluate the measured full-copy normal release stage envelope
export function evaluateNormalStageCapacity(input, trustedNow = new Date()) {
  const keys = [
    "activeDatabaseBytes", "captureReserveBytes", "capturedAt",
    "fixtureInodes", "freeBytes", "freeInodes", "imagePullPeakBytes",
    "imageReferences", "inodeFloor", "largestSourceIndexBytes", "maxWalBytes",
    "newImageInodes", "operationId", "persistentImageGrowthBytes", "phase",
    "postgresComposeProject", "postgresContainerId", "postgresContainerImageId",
    "postgresContainerImageReference", "postgresDataDirectory",
    "postgresDataFilesystemDevice", "protectedFloorBytes",
    "retirementCreditBytes", "scratchBytes", "sourceDatabaseFileCount",
    "sourceDatabaseOid", "sourceEnvSha256", "sourceLedgerSha256", "sourceRelease",
    "sourceSchemaVersion", "systemIdentifier", "targetEnvSha256", "targetLedgerSha256",
    "targetRelease", "targetSchemaVersion", "weatherPostgresDir",
    "weatherPostgresDirFilesystemDevice",
  ];
  requireExactKeys(input, keys, "normal stage capacity input");
  validateNormalCapacityIdentity(input);
  const evaluatedAt = validateNormalCapacityClock(input, trustedNow);

  // require the reviewed fixed reserves and zero speculative deletion credit
  if (!["pre_pull", "post_pull"].includes(input.phase) ||
    input.protectedFloorBytes !== BLUEBERRY_PROTECTED_FREE_BYTES ||
    input.captureReserveBytes !== BLUEBERRY_NEXT_CAPTURE_BYTES ||
    input.scratchBytes !== NORMAL_CAPACITY_SCRATCH_BYTES ||
    input.inodeFloor !== NORMAL_CAPACITY_INODE_FLOOR ||
    input.fixtureInodes !== NORMAL_CAPACITY_FIXTURE_INODES ||
    input.retirementCreditBytes !== 0) {
    throw new Error("normal stage capacity constants are invalid");
  }
  const measuredNames = ["freeBytes", "freeInodes", "imagePullPeakBytes",
    "persistentImageGrowthBytes", "activeDatabaseBytes", "largestSourceIndexBytes",
    "maxWalBytes", "newImageInodes", "sourceDatabaseFileCount"];

  // require every physical fact to be an exact nonnegative integer
  for (const name of measuredNames) {
    normalCapacityInteger(input[name], name, name === "activeDatabaseBytes" ||
      name === "sourceDatabaseFileCount" ? 1 : 0);
  }
  validateNormalStageImageReferences(input.imageReferences);
  const migrationPeakBytes = adjustmentReleaseSum(
    input.activeDatabaseBytes,
    input.largestSourceIndexBytes,
    adjustmentReleaseSum(input.maxWalBytes, input.maxWalBytes),
    input.scratchBytes,
  );
  const transactionPeakBytes = input.phase === "pre_pull"
    ? Math.max(input.imagePullPeakBytes, adjustmentReleaseSum(
      input.persistentImageGrowthBytes,
      migrationPeakBytes,
    ))
    : migrationPeakBytes;
  const requiredBytes = adjustmentReleaseSum(
    input.protectedFloorBytes,
    input.captureReserveBytes,
    transactionPeakBytes,
  );
  const requiredInodes = adjustmentReleaseSum(
    input.inodeFloor,
    input.phase === "pre_pull" ? input.newImageInodes : 0,
    input.sourceDatabaseFileCount,
    input.fixtureInodes,
  );
  const reasons = [];

  // preserve both the byte and inode envelope on the database filesystem
  if (input.weatherPostgresDirFilesystemDevice !== input.postgresDataFilesystemDevice) {
    reasons.push("postgres_filesystem_mismatch");
  }
  if (input.freeBytes < requiredBytes) reasons.push("insufficient_free_bytes");
  if (input.freeInodes < requiredInodes) reasons.push("insufficient_free_inodes");
  return closeNormalCapacityReceipt(
    input,
    evaluatedAt,
    "stage",
    requiredBytes,
    requiredInodes,
    reasons,
  );
}

// evaluate the bounded-backup normal release activation envelope
export function evaluateNormalActivationCapacity(input, trustedNow = new Date()) {
  const keys = [
    "actualBackupBytes", "backupHeadroomBytes", "boundedBackupCeilingBytes",
    "captureReserveBytes", "capturedAt", "fixtureInodes", "freeBytes",
    "freeInodes", "inodeFloor", "liveDatabaseBytes", "measuredMigrationPeakBytes",
    "measuredMigrationPeakInodes", "operationId", "phase", "postgresDataDirectory",
    "postgresComposeProject", "postgresContainerId", "postgresContainerImageId",
    "postgresContainerImageReference", "postgresDataFilesystemDevice",
    "protectedFloorBytes", "retirementCreditBytes",
    "sourceDatabaseOid", "sourceEnvSha256", "sourceLedgerSha256", "sourceRelease",
    "sourceSchemaVersion", "stageReceiptSha256", "systemIdentifier", "targetEnvSha256",
    "targetLedgerSha256", "targetRelease", "targetSchemaVersion", "weatherPostgresDir",
    "weatherPostgresDirFilesystemDevice",
  ];
  requireExactKeys(input, keys, "normal activation capacity input");
  validateNormalCapacityIdentity(input);
  const evaluatedAt = validateNormalCapacityClock(input, trustedNow);

  // retain the stage authority and fixed backup/inode envelopes
  if (!["pre_backup", "post_backup"].includes(input.phase) ||
    !HASH_PATTERN.test(input.stageReceiptSha256) ||
    input.protectedFloorBytes !== BLUEBERRY_PROTECTED_FREE_BYTES ||
    input.captureReserveBytes !== BLUEBERRY_NEXT_CAPTURE_BYTES ||
    input.backupHeadroomBytes !== NORMAL_CAPACITY_BACKUP_HEADROOM_BYTES ||
    input.inodeFloor !== NORMAL_CAPACITY_INODE_FLOOR ||
    input.fixtureInodes !== NORMAL_CAPACITY_FIXTURE_INODES ||
    input.retirementCreditBytes !== 0) {
    throw new Error("normal activation capacity constants are invalid");
  }
  for (const name of ["actualBackupBytes", "boundedBackupCeilingBytes", "freeBytes",
    "freeInodes", "liveDatabaseBytes", "measuredMigrationPeakBytes",
    "measuredMigrationPeakInodes"]) {
    normalCapacityInteger(input[name], name, name === "liveDatabaseBytes" ? 1 : 0);
  }
  const expectedBackupCeiling = adjustmentReleaseSum(
    input.liveDatabaseBytes,
    input.backupHeadroomBytes,
  );

  // prohibit a lowered ceiling or a fabricated pre-backup allocation
  if (input.boundedBackupCeilingBytes !== expectedBackupCeiling ||
    (input.phase === "pre_backup" && input.actualBackupBytes !== 0)) {
    throw new Error("normal activation backup envelope is invalid");
  }
  const backupCeilingExceeded = input.actualBackupBytes > input.boundedBackupCeilingBytes;
  let remainingBackupBytes = input.boundedBackupCeilingBytes;

  // reserve only the unconsumed backup ceiling after the measured backup exists
  if (input.phase === "post_backup") {
    remainingBackupBytes = backupCeilingExceeded
      ? 0
      : input.boundedBackupCeilingBytes - input.actualBackupBytes;
  }
  const requiredBytes = adjustmentReleaseSum(
    input.protectedFloorBytes,
    input.captureReserveBytes,
    remainingBackupBytes,
    input.measuredMigrationPeakBytes,
  );
  const requiredInodes = adjustmentReleaseSum(
    input.inodeFloor,
    input.measuredMigrationPeakInodes,
    input.fixtureInodes,
  );
  const reasons = [];

  // preserve the complete admitted backup and measured migration peak together
  if (input.weatherPostgresDirFilesystemDevice !== input.postgresDataFilesystemDevice) {
    reasons.push("postgres_filesystem_mismatch");
  }
  if (backupCeilingExceeded) {
    reasons.push("bounded_backup_ceiling_exceeded");
  }
  if (input.freeBytes < requiredBytes) reasons.push("insufficient_free_bytes");
  if (input.freeInodes < requiredInodes) reasons.push("insufficient_free_inodes");
  return closeNormalCapacityReceipt(
    input,
    evaluatedAt,
    "activation",
    requiredBytes,
    requiredInodes,
    reasons,
  );
}

// revalidate one stored normal capacity receipt and every echoed input fact
export function validateNormalCapacityReceipt(value, trustedNow = new Date()) {
  const receipt = requireObject(value, "normal capacity receipt");
  const input = { ...receipt };
  const evaluatedAt = normalCapacityInstant(receipt.evaluatedAt, "evaluatedAt");
  const trustedAt = Date.parse(normalCapacityTrustedInstant(trustedNow));
  const receiptAge = trustedAt - evaluatedAt;

  // reject a stored evaluation outside the real caller-owned current-time window
  if (receiptAge > NORMAL_CAPACITY_MAXIMUM_AGE_MILLISECONDS ||
    receiptAge < -NORMAL_CAPACITY_MAXIMUM_FUTURE_MILLISECONDS) {
    throw new Error("normal capacity receipt is stale");
  }

  // remove only the derived receipt fields before exact input validation
  for (const name of ["evaluatedAt", "operation", "reasons", "receiptSha256", "requiredBytes",
    "requiredInodes", "schemaVersion", "state"]) {
    delete input[name];
  }
  const expected = receipt.operation === "stage"
    ? evaluateNormalStageCapacity(input, new Date(evaluatedAt))
    : receipt.operation === "activation"
      ? evaluateNormalActivationCapacity(input, new Date(evaluatedAt))
      : null;

  // reject every unknown operation, changed fact, formula, reason, state, or digest
  if (expected === null || canonicalJson(receipt) !== canonicalJson(expected)) {
    throw new Error("normal capacity receipt differs");
  }
  return expected;
}

// read one bounded PostgreSQL data_directory from the exact running container
function collectNormalPostgresDataDirectory(containerId, execute) {
  let output;

  // ask PostgreSQL itself instead of accepting a caller-shaped container path
  try {
    output = execute("docker", ["exec", containerId, "psql", "--no-psqlrc",
      "--set=ON_ERROR_STOP=1", "--username", "postgres", "--dbname", "postgres",
      "--tuples-only", "--no-align", "--command", "SHOW data_directory"], {
      encoding: "utf8",
      maxBuffer: 16_384,
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch {
    throw new Error("normal capacity PostgreSQL data directory inspection failed");
  }
  const lines = output.split(/\r?\n/u).filter((line) => line.length > 0);

  // require one literal canonical absolute directory
  if (Buffer.byteLength(output) > 16_384 || lines.length !== 1 || lines[0] === "/" ||
    resolve(lines[0]) !== lines[0]) {
    throw new Error("normal capacity PostgreSQL data directory is invalid");
  }
  return lines[0];
}

// measure the host filesystem backing the exact running PostgreSQL container
export function collectNormalDatabaseFilesystemCapacity(input, options = {}) {
  let output;
  const execute = options.execute ?? execFileSync;
  const realpathHost = options.realpath ?? realpathSync;
  const statFilesystem = options.statFilesystem ?? statfsSync;
  const statHost = options.stat ?? statSync;

  // inspect only the exact Compose-selected container identity
  try {
    output = execute("docker", ["inspect", "--format", "{{json .}}",
      input.postgresContainerId], {
      encoding: "utf8",
      maxBuffer: 1_048_576,
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch {
    throw new Error("normal capacity PostgreSQL container inspection failed");
  }
  if (Buffer.byteLength(output) > 1_048_576) {
    throw new Error("normal capacity PostgreSQL container inspection exceeds its bound");
  }
  const inspected = JSON.parse(output);
  const dataDirectory = collectNormalPostgresDataDirectory(input.postgresContainerId, execute);

  // prove the exact running Compose service and immutable container image
  if (inspected.Id !== input.postgresContainerId || inspected.State?.Running !== true ||
    inspected.Config?.Labels?.["com.docker.compose.project"] !== input.postgresComposeProject ||
    inspected.Config?.Labels?.["com.docker.compose.service"] !== "postgres" ||
    !ADJUSTMENT_RELEASE_DIGEST.test(inspected.Image) ||
    !/^[^@\s]+@sha256:[a-f0-9]{64}$/u.test(inspected.Config?.Image) ||
    !Array.isArray(inspected.Mounts)) {
    throw new Error("normal capacity PostgreSQL container identity is invalid");
  }
  const matchingMounts = inspected.Mounts.filter((mount) =>
    dataDirectory === mount.Destination || dataDirectory.startsWith(`${mount.Destination}/`));

  // reject nested mount shadowing and require the reviewed writable bind source
  if (matchingMounts.length !== 1 || matchingMounts[0].Type !== "bind" ||
    matchingMounts[0].RW !== true || matchingMounts[0].Source !== input.weatherPostgresDir ||
    realpathHost(matchingMounts[0].Source) !== input.weatherPostgresDir) {
    throw new Error("normal capacity PostgreSQL bind mount is invalid");
  }
  const mount = matchingMounts[0];
  const suffix = relative(mount.Destination, dataDirectory);
  const hostDataDirectory = resolve(mount.Source, suffix);
  const canonicalHostDataDirectory = realpathHost(hostDataDirectory);

  // keep the SQL-selected directory inside the canonical reviewed host source
  if (suffix.startsWith(`..${sep}`) || suffix === ".." ||
    (canonicalHostDataDirectory !== input.weatherPostgresDir &&
      !canonicalHostDataDirectory.startsWith(`${input.weatherPostgresDir}${sep}`))) {
    throw new Error("normal capacity PostgreSQL data directory escapes its bind mount");
  }
  const mountStatus = statHost(input.weatherPostgresDir, { bigint: true });
  const dataStatus = statHost(canonicalHostDataDirectory, { bigint: true });
  const filesystem = statFilesystem(canonicalHostDataDirectory, { bigint: true });
  const freeBytes = filesystem.bavail * filesystem.bsize;

  // retain exact host facts and detect a nested host filesystem boundary
  if (!mountStatus.isDirectory() || !dataStatus.isDirectory() || mountStatus.dev < 1n ||
    dataStatus.dev < 1n || mountStatus.dev > BigInt(Number.MAX_SAFE_INTEGER) ||
    dataStatus.dev > BigInt(Number.MAX_SAFE_INTEGER) ||
    freeBytes > BigInt(Number.MAX_SAFE_INTEGER) ||
    filesystem.ffree > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error("normal capacity PostgreSQL filesystem exceeds exact measurement bounds");
  }
  return {
    freeBytes: Number(freeBytes),
    freeInodes: Number(filesystem.ffree),
    postgresContainerImageId: inspected.Image,
    postgresContainerImageReference: inspected.Config.Image,
    postgresDataDirectory: dataDirectory,
    postgresDataFilesystemDevice: Number(dataStatus.dev),
    weatherPostgresDirFilesystemDevice: Number(mountStatus.dev),
  };
}

// derive image growth from exact registry manifests and local Docker layers
async function collectNormalStageImageCapacity(input) {
  const [sourceServer, sourceWeb, targetServer, targetWeb] =
    validateNormalStageImageReferences(input.imageReferences);
  const record = await collectLiteralReleaseCapacity([
    sourceServer, sourceWeb, targetServer, targetWeb, sourceServer, sourceWeb,
  ]);
  const inventory = collectReleaseCapacityInventory({
    ...record,
    compensationScope: "normal-schema-release-source-restore",
    sourceRelease: input.sourceRelease,
    version: "normal-stage-release-inventory/v1",
  }, "normal");
  const measured = evaluateAdjustmentReleaseCapacity(inventory);
  return {
    imagePullPeakBytes: measured.pullPeakGrowthBytes,
    newImageInodes: adjustmentReleaseSum(
      measured.newLayerInodes,
      ADJUSTMENT_RELEASE_ENGINE_METADATA_INODES,
    ),
    persistentImageGrowthBytes: measured.persistentGrowthBytes,
  };
}

// collect authoritative stage facts before applying the pure reviewed formula
export async function collectNormalStageCapacity(input, options = {}) {
  const keys = [
    "activeDatabaseBytes", "captureReserveBytes", "capturedAt", "fixtureInodes",
    "imageReferences", "inodeFloor", "largestSourceIndexBytes", "maxWalBytes",
    "operationId", "phase", "postgresComposeProject", "postgresContainerId",
    "protectedFloorBytes", "retirementCreditBytes", "scratchBytes",
    "sourceDatabaseFileCount", "sourceDatabaseOid", "sourceEnvSha256",
    "sourceLedgerSha256", "sourceRelease", "sourceSchemaVersion", "systemIdentifier",
    "targetEnvSha256", "targetLedgerSha256", "targetRelease", "targetSchemaVersion",
    "weatherPostgresDir",
  ];
  requireExactKeys(input, keys, "normal stage capacity collection input");
  const trustedNow = options.trustedNow ?? new Date();
  validateNormalCapacityReleaseIdentity(input);
  validateNormalCapacityClock(input, trustedNow);
  validateNormalStageImageReferences(input.imageReferences);
  const collectFilesystem = options.collectFilesystem ?? collectNormalDatabaseFilesystemCapacity;
  const collectImages = options.collectImages ?? collectNormalStageImageCapacity;
  const filesystem = await collectFilesystem(input);
  const images = await collectImages(input);
  requireExactKeys(filesystem, ["freeBytes", "freeInodes", "postgresContainerImageId",
    "postgresContainerImageReference", "postgresDataDirectory",
    "postgresDataFilesystemDevice", "weatherPostgresDirFilesystemDevice"],
  "normal stage filesystem collection");
  requireExactKeys(images, ["imagePullPeakBytes", "newImageInodes",
    "persistentImageGrowthBytes"], "normal stage image collection");
  return evaluateNormalStageCapacity({ ...input, ...filesystem, ...images }, trustedNow);
}

// collect authoritative activation facts before applying the pure reviewed formula
export async function collectNormalActivationCapacity(input, options = {}) {
  const keys = [
    "actualBackupBytes", "backupHeadroomBytes", "boundedBackupCeilingBytes",
    "captureReserveBytes", "capturedAt", "fixtureInodes", "inodeFloor",
    "liveDatabaseBytes", "measuredMigrationPeakBytes", "measuredMigrationPeakInodes",
    "operationId", "phase", "postgresComposeProject", "postgresContainerId",
    "protectedFloorBytes", "retirementCreditBytes", "sourceDatabaseOid",
    "sourceEnvSha256", "sourceLedgerSha256", "sourceRelease", "sourceSchemaVersion",
    "stageReceiptSha256", "systemIdentifier", "targetEnvSha256", "targetLedgerSha256",
    "targetRelease", "targetSchemaVersion", "weatherPostgresDir",
  ];
  requireExactKeys(input, keys, "normal activation capacity collection input");
  const trustedNow = options.trustedNow ?? new Date();
  validateNormalCapacityReleaseIdentity(input);
  validateNormalCapacityClock(input, trustedNow);
  const collectFilesystem = options.collectFilesystem ?? collectNormalDatabaseFilesystemCapacity;
  const filesystem = await collectFilesystem(input);
  requireExactKeys(filesystem, ["freeBytes", "freeInodes", "postgresContainerImageId",
    "postgresContainerImageReference", "postgresDataDirectory",
    "postgresDataFilesystemDevice", "weatherPostgresDirFilesystemDevice"],
  "normal activation filesystem collection");
  return evaluateNormalActivationCapacity({ ...input, ...filesystem }, trustedNow);
}

const MODEL_ACTION_CONTRACT_VERSION = "forecast-adjustment-model-action/v1";
const RAIN_CONTROL_REFERENCE_ACTION_CONTRACT_VERSION =
  "forecast-adjustment-rain-control-reference-action/v1";
const MODEL_ACTION_KEYS = ["actionKind", "candidateGraphSha256", "candidateSha256",
  "contractVersion", "createdAt", "expectedSettingsSha256", "expectedSourceCommit",
  "expectedInstalledReceiptSha256",
  "expectedSourceRelease", "family", "fencingToken", "fullMemberRootSha256",
  "lifecycleHeadSha256", "policyDecision", "policyReportSha256", "predecessorActionSha256",
  "reason", "reportCreatedAt", "siteKey", "validThrough"];
const MODEL_ACTION_RULES = new Map([
  ["shadow", ["pending", "development_candidate", true, false]],
  ["promote", ["qualified", "qualified_candidate", true, false]],
  ["rollback_prior", ["regressed", "qualified_prior", true, false]],
  ["raw", ["regressed", "policy_raw", false, false]],
  ["compensate_incumbent", [null, "failed_promotion", true, true]],
  ["compensate_raw", [null, "invalid_incumbent", false, true]],
  ["compensate_shadow", ["pending", "development_shadow_failure", true, true]],
]);
const RAIN_CONTROL_REFERENCE_ACTION_KEYS = ["actionKind", "contractVersion", "createdAt",
  "dueMonth", "expectedCatalogReceiptSha256", "expectedSettingsSha256",
  "expectedSourceCommit", "expectedSourceRelease", "family", "fencingToken",
  "graphManifestSha256", "ordinalArtifactSha256", "predecessorActionSha256", "reason",
  "controlStateSha256", "sourceMemberRootSha256", "sourceReceiptRootSha256", "validThrough"];

// independently close the root action grammar and immutable policy freshness
export function validateAdjustmentFamilyAction(bytes, expected) {
  requireExactKeys(expected, ["actionSha256", "family", "now", "reportSha256",
    "sourceCommit", "sourceRelease"], "family action expectations");

  // accept only bounded exact bytes extracted from the pinned target image
  if (!Buffer.isBuffer(bytes) || bytes.length < 1 || bytes.length > 16_384) {
    throw new Error("family action bytes are invalid");
  }
  const action = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(action, MODEL_ACTION_KEYS, "family action");
  const rule = MODEL_ACTION_RULES.get(action.actionKind);
  const digest = createHash("sha256").update(bytes).digest("hex");

  // require canonical framing and exact command-to-image identities
  if (!bytes.equals(Buffer.from(canonicalJson(action))) || digest !== expected.actionSha256 ||
    action.contractVersion !== MODEL_ACTION_CONTRACT_VERSION || action.siteKey !== "ballydidean" ||
    !["rain", "temperature", "wind"].includes(action.family) || action.family !== expected.family ||
    action.expectedSourceRelease !== expected.sourceRelease ||
    action.expectedSourceCommit !== expected.sourceCommit ||
    action.policyReportSha256 !== expected.reportSha256 || rule === undefined ||
    action.reason !== rule[1] ||
    (rule[0] === null ? !["qualified", "regressed"].includes(action.policyDecision) :
      action.policyDecision !== rule[0])) {
    throw new Error("family action identity differs");
  }

  // refuse malformed roots and caller-selected aliases
  for (const key of ["expectedSettingsSha256", "lifecycleHeadSha256", "policyReportSha256"]) {
    if (!/^[a-f0-9]{64}$/u.test(action[key])) throw new Error("family action root is invalid");
  }
  for (const key of ["candidateGraphSha256", "candidateSha256", "fullMemberRootSha256", "expectedInstalledReceiptSha256",
    "predecessorActionSha256"]) {
    if (action[key] !== null && !/^[a-f0-9]{64}$/u.test(action[key])) {
      throw new Error("family action optional root is invalid");
    }
  }
  if (!/^[a-f0-9]{40}$/u.test(action.expectedSourceCommit) ||
    !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d*$/u.test(action.expectedSourceRelease) ||
    typeof action.fencingToken !== "string" || !/^(?:0|[1-9]\d{0,19})$/u.test(action.fencingToken) ||
    BigInt(action.fencingToken) > 18_446_744_073_709_551_615n ||
    rule[2] !== (action.candidateSha256 !== null && action.candidateGraphSha256 !== null) ||
    (action.candidateSha256 === null) !== (action.candidateGraphSha256 === null) ||
    (["shadow", "compensate_shadow"].includes(action.actionKind)) !== (action.fullMemberRootSha256 === null) ||
    rule[3] !== (action.predecessorActionSha256 !== null)) {
    throw new Error("family action bindings are invalid");
  }
  const clocks = [action.createdAt, action.reportCreatedAt, action.validThrough, expected.now];

  // reject normalized or noncanonical clocks before freshness arithmetic
  if (clocks.some((clock) => typeof clock !== "string" ||
    !Number.isFinite(Date.parse(clock)) || new Date(clock).toISOString() !== clock)) {
    throw new Error("family action clock is invalid");
  }
  if (Date.parse(action.validThrough) - Date.parse(action.reportCreatedAt) !== 7 * 86_400_000 ||
    Date.parse(action.createdAt) < Date.parse(action.reportCreatedAt) ||
    Date.parse(expected.now) < Date.parse(action.createdAt) ||
    (!rule[3] && Date.parse(expected.now) > Date.parse(action.validThrough))) {
    throw new Error("family action is expired or clock-untrusted");
  }
  return action;
}

// independently close the custody-only pre-month rain control action grammar
export function validateAdjustmentRainControlReferenceAction(bytes, expected) {
  requireExactKeys(expected, ["actionSha256", "now", "sourceCommit", "sourceRelease"],
    "rain control action expectations");
  if (!Buffer.isBuffer(bytes) || bytes.length < 1 || bytes.length > 16_384) {
    throw new Error("rain control action bytes are invalid");
  }
  const action = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(action, RAIN_CONTROL_REFERENCE_ACTION_KEYS, "rain control action");
  const digest = createHash("sha256").update(bytes).digest("hex");
  const compensation = action.actionKind === "compensate_control_reference";

  // require exact image bytes and the disjoint control action literals
  if (!bytes.equals(Buffer.from(canonicalJson(action))) || digest !== expected.actionSha256 ||
    action.contractVersion !== RAIN_CONTROL_REFERENCE_ACTION_CONTRACT_VERSION ||
    action.family !== "rain" ||
    !["control_reference", "compensate_control_reference"].includes(action.actionKind) ||
    action.reason !== (compensation ? "failed_control_reference" : "premonth_reference") ||
    action.expectedSourceCommit !== expected.sourceCommit ||
    action.expectedSourceRelease !== expected.sourceRelease ||
    !/^\d{4}-(?:0[1-9]|1[0-2])$/u.test(action.dueMonth)) {
    throw new Error("rain control action identity differs");
  }
  for (const key of ["expectedSettingsSha256", "graphManifestSha256",
    "ordinalArtifactSha256", "controlStateSha256", "sourceMemberRootSha256",
    "sourceReceiptRootSha256"]) {
    if (!/^[a-f0-9]{64}$/u.test(action[key])) {
      throw new Error("rain control action root is invalid");
    }
  }
  for (const key of ["expectedCatalogReceiptSha256", "predecessorActionSha256"]) {
    if (action[key] !== null && !/^[a-f0-9]{64}$/u.test(action[key])) {
      throw new Error("rain control action optional root is invalid");
    }
  }
  const clocks = [action.createdAt, action.validThrough, expected.now];
  const monthStart = Date.parse(`${action.dueMonth}-01T00:00:00.000Z`);
  const createdAt = Date.parse(action.createdAt);

  // bind the literal pre-month window and permit expired compensation only as recovery
  if (!/^[a-f0-9]{40}$/u.test(action.expectedSourceCommit) ||
    !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d*$/u.test(action.expectedSourceRelease) ||
    typeof action.fencingToken !== "string" || !/^[1-9]\d{0,19}$/u.test(action.fencingToken) ||
    BigInt(action.fencingToken) > 18_446_744_073_709_551_615n ||
    clocks.some((clock) => typeof clock !== "string" || !Number.isFinite(Date.parse(clock)) ||
      new Date(clock).toISOString() !== clock) ||
    action.validThrough !== new Date(monthStart).toISOString() ||
    createdAt < monthStart - 7 * 86_400_000 || createdAt >= monthStart ||
    Date.parse(expected.now) < createdAt ||
    (!compensation && Date.parse(expected.now) > monthStart) ||
    compensation !== (action.predecessorActionSha256 !== null)) {
    throw new Error("rain control action is expired or clock-untrusted");
  }
  return action;
}

// verify public immutable refs and exact CI before privileged family publication
export async function verifyAdjustmentFamilyGitRelease({ action, targetRelease, readJson }) {
  // close both tags and the trusted public repository before any request
  if (!/^\d{4}\.\d{2}\.\d{2}-[1-9]\d*$/u.test(targetRelease) ||
    targetRelease === action.expectedSourceRelease || typeof readJson !== "function") {
    throw new Error("family Git release identity is invalid");
  }
  const sourceCommit = await resolvePublicWeatherTag(action.expectedSourceRelease, readJson);
  const targetCommit = await resolvePublicWeatherTag(targetRelease, readJson);

  // source identity must match the deployed release action boundary
  if (sourceCommit !== action.expectedSourceCommit || sourceCommit === targetCommit) {
    throw new Error("family Git source differs");
  }
  const compare = await readJson(`compare/${sourceCommit}...${targetCommit}?per_page=100`);

  // one exact parent prevents hidden commits or truncated compare output
  if (compare.status !== "ahead" || compare.ahead_by !== 1 || compare.behind_by !== 0 ||
    compare.total_commits !== 1 || compare.base_commit?.sha !== sourceCommit ||
    !Array.isArray(compare.commits) || compare.commits.length !== 1 ||
    compare.commits[0]?.sha !== targetCommit || compare.commits[0]?.parents?.length !== 1 ||
    compare.commits[0].parents[0].sha !== sourceCommit) {
    throw new Error("family Git ancestry is not exact");
  }
  // apply the disjoint file matrix selected by the validated action contract
  if (action.contractVersion === RAIN_CONTROL_REFERENCE_ACTION_CONTRACT_VERSION) {
    validateAdjustmentRainControlReferenceChangedPaths(action, compare.files);
  } else {
    validateAdjustmentFamilyChangedPaths(action, compare.files);
  }
  const check = await readJson(`actions/workflows/check.yml/runs?head_sha=${targetCommit}&event=push&per_page=100`);
  const publish = await readJson(`actions/workflows/publish-images.yml/runs?head_sha=${targetCommit}&event=push&per_page=100`);
  const checkRun = requireLatestWeatherWorkflow(check, targetCommit, "check.yml");
  const publishRun = requireLatestWeatherWorkflow(publish, targetCommit, "publish-images.yml");

  // immutable tag publication must follow the authoritative exact-commit branch check
  if (Date.parse(checkRun.updated_at) > Date.parse(publishRun.created_at) ||
    !Number.isFinite(Date.parse(checkRun.updated_at))) {
    throw new Error("family release was published before exact Check success");
  }
  return Object.freeze({ checkRunId: checkRun.id, publishRunId: publishRun.id,
    sourceCommit, targetCommit, targetRelease });
}

// prove one exact direct-child public release after Check and image publication
async function verifyAdjustmentDirectChildRelease({ expectedSourceCommit, expectedTargetCommit = null,
  label, sourceRelease, targetRelease }, readJson) {
  requireFamilyRelease(sourceRelease, `${label} source release`);
  requireFamilyRelease(targetRelease, `${label} target release`);
  // reject aliases, retries against the source and alternate readers
  if (targetRelease === sourceRelease || typeof readJson !== "function") {
    throw new Error(`${label} target differs from the supported handoff`);
  }
  const sourceCommit = await resolvePublicWeatherTag(sourceRelease, readJson);
  const targetCommit = await resolvePublicWeatherTag(targetRelease, readJson);
  // reject a re-tagged source or bridge before considering workflow evidence
  if (sourceCommit !== expectedSourceCommit) {
    throw new Error(`${label} Git source differs`);
  }
  if (targetCommit === sourceCommit ||
    (expectedTargetCommit !== null && targetCommit !== expectedTargetCommit)) {
    throw new Error(`${label} Git target differs`);
  }
  const compare = await readJson(`compare/${sourceCommit}...${targetCommit}?per_page=100`);
  // require this handoff to be the next reviewed commit rather than an unrelated green tag
  if (compare?.status !== "ahead" || compare.ahead_by !== 1 || compare.behind_by !== 0 ||
    compare.total_commits !== 1 || compare.base_commit?.sha !== sourceCommit ||
    !Array.isArray(compare.commits) || compare.commits.length !== 1 ||
    compare.commits[0]?.sha !== targetCommit || compare.commits[0]?.parents?.length !== 1 ||
    compare.commits[0].parents[0].sha !== sourceCommit) {
    throw new Error(`${label} Git ancestry is not exact`);
  }
  const check = await readJson(`actions/workflows/check.yml/runs?head_sha=${targetCommit}&event=push&per_page=100`);
  const publish = await readJson(`actions/workflows/publish-images.yml/runs?head_sha=${targetCommit}&event=push&per_page=100`);
  const checkRun = requireLatestWeatherWorkflow(check, targetCommit, "check.yml");
  const publishRun = requireLatestWeatherWorkflow(publish, targetCommit, "publish-images.yml");
  // the exact branch check must finish before the immutable release publish
  if (!Number.isFinite(Date.parse(checkRun.updated_at)) ||
    Date.parse(checkRun.updated_at) > Date.parse(publishRun.created_at)) {
    throw new Error(`${label} release preceded exact Check success`);
  }
  return Object.freeze({ checkRunId: checkRun.id, publishRunId: publishRun.id,
    sourceCommit, targetCommit, targetRelease });
}

// prove the compatibility-only bridge from the retained v13 release
export async function verifyAdjustmentV14CompatibilityBridgeGitRelease(
  targetRelease,
  readJson = readPublicWeatherGitJson,
) {
  return await verifyAdjustmentDirectChildRelease({
    expectedSourceCommit: "56b327d9c750946f6f6963b6fe1fa5c9bba791ca",
    expectedTargetCommit: ADJUSTMENT_FULL_V14_SOURCE_COMMIT,
    label: "v14 compatibility bridge",
    sourceRelease: ADJUSTMENT_INERT_V14_SOURCE_RELEASE,
    targetRelease,
  }, readJson);
}

// preserve the original export for the now-explicit bridge proof
export async function verifyAdjustmentInertV14GitRelease(
  targetRelease,
  readJson = readPublicWeatherGitJson,
) {
  return await verifyAdjustmentV14CompatibilityBridgeGitRelease(targetRelease, readJson);
}

// prove the full maintenance release is the direct child of the bridge
export async function verifyAdjustmentFullV14GitRelease(
  targetRelease,
  readJson = readPublicWeatherGitJson,
) {
  if (targetRelease !== ADJUSTMENT_FULL_V14_TARGET_RELEASE) {
    throw new Error("full v14 target differs from the supported handoff");
  }
  return await verifyAdjustmentDirectChildRelease({
    expectedSourceCommit: ADJUSTMENT_FULL_V14_SOURCE_COMMIT,
    label: "full v14",
    sourceRelease: ADJUSTMENT_FULL_V14_SOURCE_RELEASE,
    targetRelease,
  }, readJson);
}

// prove a separately published dashboard descendant of the unchanged core
export async function verifyDashboardWebGitRelease(
  targetRelease,
  coreRelease,
  readJson = readPublicWeatherGitJson,
) {
  requireFamilyRelease(targetRelease, "dashboard target release");
  // reserve the immutable maintenance releases for their original operations
  if (coreRelease !== ADJUSTMENT_INERT_V14_SOURCE_RELEASE ||
    [ADJUSTMENT_FULL_V14_SOURCE_RELEASE, ADJUSTMENT_FULL_V14_TARGET_RELEASE].includes(targetRelease) ||
    typeof readJson !== "function") {
    throw new Error("dashboard publication core or target is unsupported");
  }
  const sourceCommit = await resolvePublicWeatherTag(coreRelease, readJson);
  const targetCommit = await resolvePublicWeatherTag(targetRelease, readJson);
  // a moved core tag cannot authorize new dashboard bytes
  if (sourceCommit !== DASHBOARD_WEB_RELEASE_CORE_COMMIT ||
    targetRelease !== coreRelease && targetCommit === sourceCommit) {
    throw new Error("dashboard publication Git identity differs");
  }
  // core rollback uses its original publication without inventing a descendant
  if (targetRelease !== coreRelease) {
    const compare = await readJson(`compare/${sourceCommit}...${targetCommit}?per_page=1`);
    // the independent dashboard must descend from the retained core
    if (compare?.status !== "ahead" || compare.behind_by !== 0 ||
      !Number.isSafeInteger(compare.ahead_by) || compare.ahead_by < 1 ||
      compare.base_commit?.sha !== sourceCommit) {
      throw new Error("dashboard publication Git ancestry differs");
    }
  }
  const check = await readJson(`actions/workflows/check.yml/runs?head_sha=${targetCommit}&event=push&per_page=100`);
  const publish = await readJson(`actions/workflows/publish-images.yml/runs?head_sha=${targetCommit}&event=push&per_page=100`);
  const checkRun = requireLatestWeatherWorkflow(check, targetCommit, "check.yml");
  const publishRun = requireLatestWeatherWorkflow(publish, targetCommit, "publish-images.yml");
  // match this tag and preserve Check-before-Publish ordering
  if (publishRun.head_branch !== targetRelease ||
    !Number.isFinite(Date.parse(checkRun.updated_at)) ||
    Date.parse(checkRun.updated_at) > Date.parse(publishRun.created_at)) {
    throw new Error("dashboard publication preceded Check or names another tag");
  }
  return Object.freeze({ checkRunId: checkRun.id,
    contractVersion: "dashboard-web-release-proof/v1", coreRelease,
    publishRunId: publishRun.id, sourceCommit, state: "publication_ready",
    targetCommit, targetRelease });
}

// freeze one validated operator-settings inode before starting the new web process
export async function readAdjustmentInertV14SettingsSnapshot(options = {}) {
  const path = options.path ?? ADJUSTMENT_FAMILY_SETTINGS_PATH;
  const owners = options.owners ?? [0, 10_002];
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const before = await handle.stat();
    // reject aliases and unexpected settings writers before reading their bytes
    if (!before.isFile() || before.nlink !== 1 || !owners.includes(before.uid) ||
      before.gid !== before.uid || (before.mode & 0o777) !== 0o600 ||
      before.size < 1 || before.size > 4_096 || await realpath(path) !== path) {
      throw new Error("inert v14 operator settings file is unsafe");
    }
    const bytes = await handle.readFile();
    const after = await handle.stat();
    const linked = await lstat(path);
    // reject in-place edits and path replacement during the descriptor-bound read
    if (!linked.isFile() || linked.isSymbolicLink() || before.dev !== linked.dev ||
      before.ino !== linked.ino || before.dev !== after.dev || before.ino !== after.ino ||
      before.size !== after.size || before.mtimeMs !== after.mtimeMs ||
      before.ctimeMs !== after.ctimeMs) {
      throw new Error("inert v14 operator settings identity changed");
    }
    const settings = JSON.parse(bytes.toString("utf8"));
    reconcileAdjustmentFamilySettings(settings, settings, "temperature");
    return Object.freeze({ identity: `${before.dev}:${before.ino}`, sha256: sha256(bytes) });
  } finally {
    await handle.close();
  }
}

// select the schema authorization only from an actually observed complete database ledger
export function selectAdjustmentInertV14RestorationSchema(input) {
  requireExactKeys(input, ["actualHistorySha256", "sourceHistorySha256", "sourceRelease",
    "sourceSchemaRelease", "targetHistorySha256", "targetRelease"],
  "inert v14 restoration schema");
  requireFamilyRelease(input.sourceRelease, "restoration source release");
  requireFamilyRelease(input.sourceSchemaRelease, "restoration source schema release");
  requireFamilyRelease(input.targetRelease, "restoration target release");
  for (const field of ["actualHistorySha256", "sourceHistorySha256", "targetHistorySha256"]) {
    // deny unavailable or malformed ledger observations before source apps start
    if (!HASH_PATTERN.test(input[field])) throw new Error("restoration schema history is invalid");
  }
  // preserve the authenticated starting marker when retry source and target ledgers coincide
  if (input.actualHistorySha256 === input.sourceHistorySha256 &&
    input.actualHistorySha256 === input.targetHistorySha256) {
    if (input.sourceSchemaRelease !== input.sourceRelease &&
      input.sourceSchemaRelease !== input.targetRelease) {
      throw new Error("restoration source schema release is unsupported");
    }
    return input.sourceSchemaRelease;
  }
  // original schema never receives authorization for a migration that did not complete
  if (input.actualHistorySha256 === input.sourceHistorySha256) return input.sourceRelease;
  if (input.actualHistorySha256 === input.targetHistorySha256) return input.targetRelease;
  throw new Error("inert v14 restoration database ledger is not recognized");
}

// resolve one public tag with a bounded annotated-tag chain
async function resolvePublicWeatherTag(release, readJson) {
  let ref = await readJson(`git/ref/tags/${release}`);

  // follow only canonical SHA-tag objects within the fixed repository
  for (let index = 0; index < 3; index += 1) {
    if (!/^[a-f0-9]{40}$/u.test(ref.object?.sha)) {
      throw new Error("Weather tag object is invalid");
    }
    if (ref.object.type === "commit") return ref.object.sha;
    if (ref.object.type !== "tag") throw new Error("Weather tag is not a commit");
    ref = await readJson(`git/tags/${ref.object.sha}`);
  }
  throw new Error("Weather tag chain is too deep");
}

// select the latest complete same-repository push run without stale-success fallback
function requireLatestWeatherWorkflow(document, commit, filename) {
  // incomplete pages could conceal a failed newer attempt
  if (!Number.isSafeInteger(document.total_count) || document.total_count < 1 ||
    document.total_count > 100 || !Array.isArray(document.workflow_runs) ||
    document.workflow_runs.length !== document.total_count) {
    throw new Error("Weather workflow evidence is incomplete");
  }
  const runs = document.workflow_runs.filter((run) => run.head_sha === commit && run.event === "push" &&
    run.repository?.id === WEATHER_GITHUB_REPOSITORY.id &&
    run.repository?.full_name === WEATHER_GITHUB_REPOSITORY.fullName &&
    run.repository?.fork === false &&
    run.head_repository?.id === WEATHER_GITHUB_REPOSITORY.id &&
    run.head_repository?.full_name === WEATHER_GITHUB_REPOSITORY.fullName &&
    run.head_repository?.fork === false);

  // malformed scoped evidence cannot reveal an older successful run
  if (runs.length === 0 || runs.some((run) =>
    run.path !== `.github/workflows/${filename}` || !Number.isSafeInteger(run.id) || run.id < 1 ||
    !Number.isSafeInteger(run.run_attempt) || run.run_attempt < 1 ||
    !Number.isFinite(Date.parse(run.created_at)))) {
    throw new Error("Weather workflow identity differs");
  }
  runs.sort((left, right) => Date.parse(right.created_at) - Date.parse(left.created_at) || right.id - left.id);
  const run = runs[0];

  // queued, cancelled and failed latest runs all block publication
  if (run.status !== "completed" || run.conclusion !== "success") {
    throw new Error("latest exact-commit Weather workflow is not successful");
  }
  return run;
}

// read only bounded public GitHub metadata without a new credential or redirect
async function readPublicWeatherGitJson(suffix) {
  const url = new URL(`/repos/${WEATHER_GITHUB_REPOSITORY.fullName}/${suffix}`,
    "https://api.github.com");
  const response = await fetch(url, { redirect: "error", signal: AbortSignal.timeout(15_000),
    headers: { Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2026-03-10" } });

  // network failure and rate limits are blockers rather than authority
  if (!response.ok) throw new Error(`Weather Git lookup failed with HTTP ${response.status}`);
  return JSON.parse(await readBoundedRegistryResponse(response, 4_194_304, "Weather Git metadata"));
}

// prevent shadow publication or compensation from smuggling another family change
export function validateAdjustmentFamilyChangedPaths(action, files) {
  const registry = `config/forecast-adjustments/ballydidean-${action.family}-${
    action.family === "rain" ? "runtime" : "canary"}.json`;
  const root = "config/forecast-adjustments/ballydidean/";
  const actionPath = `${root}actions/sha256-${createHash("sha256")
    .update(canonicalJson(action)).digest("hex")}.json`;
  const allowed = new Set([actionPath]);

  // inactive shadows alone may add candidate-addressed immutable evidence
  if (action.actionKind === "shadow") {
    allowed.add(`${root}model-parity/${action.family}/sha256-${action.candidateSha256}.json`);
    allowed.add(`${root}shadow-catalog/${action.family}/sha256-${action.candidateSha256}.json`);
    const directory = action.family === "rain"
      ? "rain-runtime-artifacts"
      : `${action.family}-canary-bundles`;
    const pattern = new RegExp(`^${root}${directory}/sha256-[a-f0-9]{64}\\.json$`, "u");
    const artifacts = Array.isArray(files)
      ? files.filter((file) => pattern.test(file.filename))
      : [];

    // admit one serving artifact plus rain's distinct immutable r0 control artifact
    if (artifacts.length < 1 || artifacts.length > (action.family === "rain" ? 2 : 1)) {
      throw new Error("family release diff is not closed");
    }
    for (const artifact of artifacts) {
      allowed.add(artifact.filename);
    }
    if (action.family === "rain") {
      allowed.add(`${root}rain-model-packages/sha256-${action.candidateSha256}.json`);
      const states = Array.isArray(files)
        ? files.filter((file) => new RegExp(
            `^${root}rain-maintenance-control-states/sha256-[a-f0-9]{64}\\.json$`,
            "u",
          ).test(file.filename))
        : [];

      // keep legacy v1 and control-state v2 shadow matrices disjoint
      if (states.length > 1 || artifacts.length === 2 && states.length !== 1) {
        throw new Error("family release diff is not closed");
      }
      if (states.length === 1) {
        allowed.add(states[0].filename);
      }
    }
  }
  if (!["shadow", "compensate_shadow"].includes(action.actionKind)) allowed.add(registry);

  // rain serving changes execute only through the generated compiled source
  if (action.family === "rain" &&
    ["promote", "rollback_prior", "compensate_incumbent"].includes(action.actionKind)) {
    allowed.add("packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts");
  }

  // a bounded complete diff may not delete, rename or alter immutable evidence
  if (!Array.isArray(files) || files.length < 1 || files.length > 10 ||
    new Set(files.map((file) => file.filename)).size !== files.length ||
    !files.some((file) => file.filename === actionPath) || files.some(
      (file) => !allowed.has(file.filename) || !["added", "modified"].includes(file.status) ||
        (file.filename === registry ? file.status !== "modified" : file.status !== "added"),
    ) || (!["shadow", "compensate_shadow"].includes(action.actionKind) &&
      !files.some((file) => file.filename === registry))) {
    throw new Error("family release diff is not closed");
  }
  return true;
}

// prevent a control reference from changing any serving or candidate selector
export function validateAdjustmentRainControlReferenceChangedPaths(action, files) {
  const actionPath = `config/forecast-adjustments/ballydidean/actions/sha256-${createHash("sha256")
    .update(canonicalJson(action)).digest("hex")}.json`;
  const selectorPath = "config/forecast-adjustments/ballydidean-rain-control-reference.json";
  const statePath = "config/forecast-adjustments/ballydidean/rain-maintenance-control-states/" +
    `sha256-${action.controlStateSha256}.json`;
  const artifactPath = "config/forecast-adjustments/ballydidean/rain-runtime-artifacts/" +
    `sha256-${action.ordinalArtifactSha256}.json`;
  const compensation = action.actionKind === "compensate_control_reference";
  const expected = compensation
    ? new Map([[actionPath, new Set(["added"])],
        [selectorPath, new Set(["modified", "removed"])]] )
    : new Map([[actionPath, new Set(["added"])], [statePath, new Set(["added"])],
        [artifactPath, new Set(["added"])],
        [selectorPath, new Set(["added", "modified"])] ]);

  // require every and only the action-specific family-control path
  if (!Array.isArray(files) || files.length !== expected.size ||
    new Set(files.map((file) => file.filename)).size !== files.length ||
    files.some((file) => !expected.get(file.filename)?.has(file.status)) ||
    [...expected.keys()].some((path) => !files.some((file) => file.filename === path))) {
    throw new Error("rain control release diff is not closed");
  }
  return true;
}

// preserve operator intent without ever writing settings during deployment
export function reconcileAdjustmentFamilySettings(before, after, family) {
  // both snapshots must be complete persisted switch documents
  for (const settings of [before, after]) {
    requireExactKeys(settings, ["version", "temperature", "wind", "rain"], "adjustment settings");
    if (settings.version !== 1 || ["temperature", "wind", "rain"].some(
      (key) => typeof settings[key] !== "boolean",
    )) throw new Error("adjustment settings are unavailable");
  }
  if (!["temperature", "wind", "rain"].includes(family)) {
    throw new Error("adjustment settings family is invalid");
  }
  // restrictive concurrent operator changes remain effective
  if (["temperature", "wind", "rain"].some((key) => !before[key] && after[key])) {
    throw new Error("adjustment settings became less restrictive");
  }
  return after[family] ? "active" : "deployed_operator_off";
}

// run one fenced family action without ever invoking whole-release restoration
export async function executeAdjustmentFamilyRelease(input, ports) {
  requireExactKeys(input, ["actionSha256", "compensatingRelease", "expectedCurrentRelease",
    "expectedSettingsSha256", "expectedSourceRelease", "family", "fencingToken",
    "reportSha256", "targetRelease"], "family transaction");
  const required = ["acknowledge", "applyImages", "clock", "inspectCurrent", "markApplying",
    "prepare", "readPrepared", "recordFailure", "verifyLive", "verifyProof", "withReleaseLock"];

  // require every real privilege and persistence boundary before mutation
  if (ports === null || typeof ports !== "object" || required.some(
    (key) => typeof ports[key] !== "function",
  )) throw new Error("family transaction ports are unavailable");
  if (!["rain", "temperature", "wind"].includes(input.family) ||
    [input.actionSha256, input.reportSha256, input.expectedSettingsSha256].some(
      (value) => typeof value !== "string" || !/^[a-f0-9]{64}$/u.test(value),
    ) || typeof input.fencingToken !== "string" || !/^[1-9]\d{0,19}$/u.test(input.fencingToken) ||
    BigInt(input.fencingToken) > 0xffff_ffff_ffff_ffffn ||
    [input.targetRelease, input.compensatingRelease, input.expectedCurrentRelease,
      input.expectedSourceRelease].some((value) => typeof value !== "string" ||
        !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d*$/u.test(value)) ||
    input.targetRelease === input.compensatingRelease ||
    input.targetRelease === input.expectedCurrentRelease ||
    input.expectedCurrentRelease !== input.expectedSourceRelease) {
    throw new Error("family transaction tokens are invalid");
  }
  return await ports.withReleaseLock(async () => {
    const prior = await ports.readPrepared(input.actionSha256);

    // retries retain exact identity and never repeat an acknowledged mutation
    if (prior !== null) {
      if (canonicalJson(prior.input) !== canonicalJson(input)) {
        throw new Error("family transaction identity collision");
      }
      // finish or replay one durable no-mutation operator-off acknowledgement
      if (prior.state === "operator_off_unapplied") {
        if (typeof ports.acknowledgeOperatorOff !== "function") {
          throw new Error("family operator-off acknowledgement port is unavailable");
        }
        return await ports.acknowledgeOperatorOff(prior);
      }
      if (prior.state === "acknowledged") return prior;
      if (["applying", "compensation_required"].includes(prior.state)) {
        return Object.freeze({ ...prior, state: "compensation_required" });
      }
    }
    const current = await ports.inspectCurrent();
    const beforeBytes = Buffer.from(current.settingsBytes);
    const before = JSON.parse(beforeBytes);
    reconcileAdjustmentFamilySettings(before, before, input.family);

    // operator intent and the exact source are checked again under the root lock
    if (current.release !== input.expectedCurrentRelease ||
      createHash("sha256").update(beforeBytes).digest("hex") !== input.expectedSettingsSha256 ||
      typeof current.maximumFence !== "string" || !/^(?:0|[1-9]\d*)$/u.test(current.maximumFence) ||
      (prior === null && BigInt(input.fencingToken) <= BigInt(current.maximumFence))) {
      throw new Error("family source, settings or fencing compare-and-swap failed");
    }
    // persist the exact source and fence without running any model mutation
    if (!before[input.family]) {
      if (typeof ports.acknowledgeOperatorOff !== "function" ||
        typeof current.commit !== "string" || !/^[a-f0-9]{40}$/u.test(current.commit)) {
        throw new Error("family operator-off acknowledgement port is unavailable");
      }
      return await ports.acknowledgeOperatorOff({
        completedAt: ports.clock(),
        input,
        sourceCommit: current.commit,
        sourceSettingsSha256: input.expectedSettingsSha256,
        state: "operator_off_unapplied",
      });
    }
    const now = ports.clock();
    const proof = await ports.verifyProof(input, current);
    const actionContract = parseCanonicalFamilyJson(
      proof.actionBytes,
      "family target action dispatch",
    ).contractVersion;
    const controlReference = actionContract === RAIN_CONTROL_REFERENCE_ACTION_CONTRACT_VERSION;
    const action = controlReference
      ? validateAdjustmentRainControlReferenceAction(proof.actionBytes, {
          actionSha256: input.actionSha256,
          now,
          sourceCommit: current.commit,
          sourceRelease: current.release,
        })
      : validateAdjustmentFamilyAction(proof.actionBytes, {
          actionSha256: input.actionSha256, family: input.family, now,
          reportSha256: input.reportSha256, sourceCommit: current.commit,
          sourceRelease: current.release,
        });

    // the command fence and setting snapshot must be embedded in the pinned action
    const installedSlot = controlReference
      ? "control"
      : ["shadow", "promote", "compensate_shadow"].includes(action.actionKind)
        ? "shadow"
        : "active";
    const currentInstalledReceipt = current.installedReceiptSha256BySlot === undefined
      ? installedSlot === "shadow" ? (current.installedReceiptSha256 ?? null) : null
      : current.installedReceiptSha256BySlot[installedSlot];
    const expectedInstalledReceipt = controlReference
      ? action.expectedCatalogReceiptSha256
      : action.expectedInstalledReceiptSha256;
    if (action.fencingToken !== input.fencingToken ||
      action.expectedSettingsSha256 !== input.expectedSettingsSha256 ||
      expectedInstalledReceipt !== currentInstalledReceipt ||
      (controlReference && input.reportSha256 !== action.graphManifestSha256) ||
      proof.targetCommit !== proof.git.targetCommit || proof.git.sourceCommit !== current.commit ||
      proof.git.targetRelease !== input.targetRelease) {
      throw new Error("family action proof differs");
    }
    const capacity = proof.capacity;
    const age = Date.parse(now) - Date.parse(capacity?.measuredAt);

    // literal source, target and compensation ownership remains fully reserved
    if (capacity?.contractVersion !== ADJUSTMENT_FAMILY_RELEASE_CAPACITY_VERSION ||
      capacity.state !== "capacity_ready" || capacity.actionSha256 !== input.actionSha256 ||
      capacity.family !== input.family || capacity.sourceRelease !== current.release ||
      capacity.compensationScope !== ADJUSTMENT_FAMILY_RELEASE_COMPENSATION_SCOPE ||
      !Number.isFinite(age) || age < 0 || age > 900_000 ||
      !Array.isArray(capacity.imageDigests) || capacity.imageDigests.length !== 6 ||
      capacity.imageDigests.some((image) => !ADJUSTMENT_RELEASE_DIGEST.test(image.digest) ||
        !ADJUSTMENT_RELEASE_ROLES.includes(image.role) || !ADJUSTMENT_RELEASE_RUNTIMES.includes(image.runtime)) ||
      new Set(capacity.imageDigests.map((image) => `${image.role}/${image.runtime}`)).size !== 6 ||
      capacity.retirementCreditBytes !== 0) {
      throw new Error("family capacity proof is missing or stale");
    }
    const compensation = controlReference
      ? validateAdjustmentRainControlReferenceAction(proof.compensation.actionBytes, {
          actionSha256: proof.compensation.actionSha256,
          now,
          sourceCommit: proof.targetCommit,
          sourceRelease: input.targetRelease,
        })
      : validateAdjustmentFamilyAction(proof.compensation.actionBytes, {
          actionSha256: proof.compensation.actionSha256, family: input.family, now,
          reportSha256: input.reportSha256, sourceCommit: proof.targetCommit,
          sourceRelease: input.targetRelease,
        });

    // reserve a real new release with inherited evidence and the exact failed predecessor
    if (!compensation.actionKind.startsWith("compensate_") ||
      compensation.predecessorActionSha256 !== input.actionSha256 ||
      compensation.fencingToken !== input.fencingToken ||
      compensation.validThrough !== action.validThrough ||
      (controlReference
        ? compensation.expectedCatalogReceiptSha256 !== action.expectedCatalogReceiptSha256 ||
          compensation.graphManifestSha256 !== action.graphManifestSha256 ||
          compensation.controlStateSha256 !== action.controlStateSha256 ||
          compensation.ordinalArtifactSha256 !== action.ordinalArtifactSha256 ||
          compensation.sourceMemberRootSha256 !== action.sourceMemberRootSha256 ||
          compensation.sourceReceiptRootSha256 !== action.sourceReceiptRootSha256 ||
          compensation.dueMonth !== action.dueMonth
        : compensation.policyDecision !== action.policyDecision ||
          compensation.fullMemberRootSha256 !== action.fullMemberRootSha256 ||
          compensation.reportCreatedAt !== action.reportCreatedAt ||
          compensation.expectedInstalledReceiptSha256 !== action.expectedInstalledReceiptSha256) ||
      proof.compensation.git.sourceCommit !== proof.targetCommit ||
      proof.compensation.git.targetRelease !== input.compensatingRelease) {
      throw new Error("family compensation lineage differs");
    }
    const authority = proof.authority;
    const controlAuthority = authority.contractVersion ===
      "adjustment-rain-control-custody-anchor/v1";
    const developmentAuthority = authority.contractVersion ===
      "adjustment-development-custody-anchor/v1";
    // preserve the disjoint report identity field in each authenticated anchor version
    const authorityReportSha256 = controlAuthority
      ? authority.graphManifestSha256
      : developmentAuthority ||
      authority.contractVersion === "adjustment-maintenance-anchor/v3"
      ? authority.policyReportSha256 : authority.reportSha256;

    // only an exact authenticated anchor may authorize the workstation policy projection
    if (authority.actionSha256 !== input.actionSha256 || authorityReportSha256 !== input.reportSha256 ||
      (!controlAuthority && !developmentAuthority &&
        authority.fullMemberRootSha256 !== action.fullMemberRootSha256) ||
      (controlAuthority && (!controlReference || authority.controlStateSha256 !== action.controlStateSha256 ||
        authority.ordinalArtifactSha256 !== action.ordinalArtifactSha256 ||
        authority.sourceMemberRootSha256 !== action.sourceMemberRootSha256 ||
        authority.sourceReceiptRootSha256 !== action.sourceReceiptRootSha256 ||
        authority.fencingToken !== action.fencingToken || authority.dueMonth !== action.dueMonth)) ||
      (developmentAuthority && (action.actionKind !== "shadow" || authority.actionKind !== "shadow" ||
        action.fullMemberRootSha256 !== null || action.policyDecision !== "pending" ||
        authority.candidateGraphSha256 !== action.candidateGraphSha256 ||
        authority.candidateSha256 !== action.candidateSha256 || authority.family !== action.family)) ||
      (!controlAuthority && authority.lifecycleLedgerRootSha256 !== action.lifecycleHeadSha256) ||
      authority.sourceCommit !== current.commit ||
      (!controlAuthority && !["shadow", "compensate_shadow"].includes(action.actionKind) &&
        authority.ctfState !== "finalized")) {
      throw new Error("family action anchor authority differs");
    }
    const record = { action, createdAt: prior?.createdAt ?? now, input, sourceCommit: current.commit,
      sourceSettingsSha256: input.expectedSettingsSha256, state: "prepared",
      targetCommit: proof.targetCommit };
    await ports.prepare(record, proof);

    // persist applying before the first service or runtime-catalog mutation
    await ports.markApplying(record);
    try {
      await ports.applyImages(record, proof);
      const live = await ports.verifyLive(record, proof);
      const outcome = reconcileAdjustmentFamilySettings(before, JSON.parse(live.settingsBytes), input.family);

      // health, actual image identity and physical family identity are independent checks
      if (live.release !== input.targetRelease || live.commit !== proof.targetCommit ||
        live.family !== input.family || live.familyIdentitySha256 !== proof.familyIdentitySha256 ||
        live.imageDigestsSha256 !== proof.imageDigestsSha256) {
        throw new Error("family live deployment verification differs");
      }
      return await ports.acknowledge({ ...record, outcome, state: "acknowledged",
        verifiedAt: ports.clock() }, live);
    } catch (error) {
      // preserve the exact failed action for a new family-only compensating release
      await ports.recordFailure({ ...record, state: "compensation_required",
        failedAt: ports.clock() });
      throw error;
    }
  });
}

// validate one durable nonserving operator-off transaction record
function validateAdjustmentFamilyOperatorOffRecord(record, expectedActionSha256 = null) {
  requireExactKeys(record, [
    "completedAt", "input", "sourceCommit", "sourceSettingsSha256", "state",
  ], "family operator-off record");
  requireExactKeys(record.input, [
    "actionSha256", "compensatingRelease", "expectedCurrentRelease",
    "expectedSettingsSha256", "expectedSourceRelease", "family", "fencingToken",
    "reportSha256", "targetRelease",
  ], "family operator-off input");
  const input = record.input;
  const releases = [input.compensatingRelease, input.expectedCurrentRelease,
    input.expectedSourceRelease, input.targetRelease];
  // bind one exact source observation and command identity without serving authority
  if (record.state !== "operator_off_unapplied" ||
    expectedActionSha256 !== null && input.actionSha256 !== expectedActionSha256 ||
    !OWNER_CONFIRMATION_FAMILIES.has(input.family) ||
    !/^[a-f0-9]{40}$/u.test(record.sourceCommit) ||
    !/^[1-9]\d{0,19}$/u.test(input.fencingToken) ||
    BigInt(input.fencingToken) > 0xffff_ffff_ffff_ffffn ||
    releases.some((value) => typeof value !== "string" ||
      !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d*$/u.test(value)) ||
    input.expectedCurrentRelease !== input.expectedSourceRelease ||
    input.targetRelease === input.expectedCurrentRelease ||
    input.targetRelease === input.compensatingRelease ||
    record.sourceSettingsSha256 !== input.expectedSettingsSha256) {
    throw new Error("family operator-off record differs");
  }
  // reject every caller-selected identity outside its closed hash domain
  for (const value of [input.actionSha256, input.expectedSettingsSha256,
    input.reportSha256, record.sourceSettingsSha256]) {
    requireFamilySha256(value, "family operator-off identity");
  }
  validateV2Instant(record.completedAt, "family operator-off completion clock");
  return record;
}

// project the exact categorical operator-off receipt from its durable input
function adjustmentFamilyOperatorOffStatus(record) {
  validateAdjustmentFamilyOperatorOffRecord(record);
  return Object.freeze({
    actionSha256: record.input.actionSha256,
    compensatingRelease: record.input.compensatingRelease,
    compensationState: "absent",
    contractVersion: "adjustment-family-release-status/v1",
    family: record.input.family,
    fencingToken: record.input.fencingToken,
    outcome: "operator_off_unapplied",
    state: "operator_off_unapplied",
    targetRelease: record.input.targetRelease,
  });
}

// create durable root ports around one explicit host mutation boundary
export function createAdjustmentFamilyReleasePorts(options = {}) {
  requireExactKeys(options, ["clock", "host", "root"], "family release port options");
  const host = options.host;

  // require every host observation and mutation adapter up front
  if (host === null || typeof host !== "object" || [
    "applyTarget", "compensate", "inspectCurrent", "verifyLive", "verifyProof",
  ].some((name) => typeof host[name] !== "function") ||
    typeof options.clock !== "function" || typeof options.root !== "string" ||
    !options.root.startsWith("/")) {
    throw new TypeError("family release host ports are invalid");
  }
  const root = resolve(options.root);
  const transactionsRoot = join(root, "transactions");
  const maximumFencePath = join(root, "maximum-fence");
  const currentPath = join(root, "current.json");
  let loadedDocument = null;

  // initialize only the fixed private transaction directories
  async function initialize() {
    await ensureAdjustmentFamilyPrivateDirectory(root);
    await ensureAdjustmentFamilyPrivateDirectory(transactionsRoot);
  }

  // locate one content-addressed transaction record
  function transactionPath(actionSha256) {
    requireFamilySha256(actionSha256, "family action SHA256");
    return join(transactionsRoot, `sha256-${actionSha256}.json`);
  }

  // read one durable transaction document without following links
  async function readDocument(actionSha256, ensureRoot = true) {
    // mutation paths establish the private root; status remains read only
    if (ensureRoot) await initialize();
    const bytes = await readAdjustmentFamilyPrivateFile(
      transactionPath(actionSha256),
      ADJUSTMENT_FAMILY_MAXIMUM_STATE_BYTES,
      true,
    );
    if (bytes === null) return null;
    const value = parseCanonicalFamilyJson(bytes, "family transaction");
    // preserve the disjoint no-mutation receipt without fake proof or compensation
    if (value.contractVersion === ADJUSTMENT_FAMILY_OPERATOR_OFF_TRANSACTION_VERSION) {
      requireExactKeys(value, ["contractVersion", "record"],
        "family operator-off transaction");
      validateAdjustmentFamilyOperatorOffRecord(value.record, actionSha256);
      return value;
    }
    requireExactKeys(value, [
      "compensation", "contractVersion", "proof", "record",
    ], "family transaction");

    // reject state from another action or an unsupported writer
    if (value.contractVersion !== ADJUSTMENT_FAMILY_TRANSACTION_VERSION ||
      value.record?.input?.actionSha256 !== actionSha256 ||
      !["absent", "failed", "verified"].includes(value.compensation?.state)) {
      throw new Error("family transaction state is invalid");
    }
    return value;
  }

  // replace one already-created transaction after exact identity comparison
  async function updateDocument(actionSha256, transform) {
    const current = await readDocument(actionSha256);
    if (current === null) throw new Error("family transaction is unavailable");
    const next = transform(structuredClone(current));
    await writeAdjustmentFamilyAtomic(transactionPath(actionSha256), canonicalFamilyBytes(next), 0o600);
    return next;
  }

  // read the monotonic fence or its fixed genesis
  async function readMaximumFence() {
    await initialize();
    const bytes = await readAdjustmentFamilyPrivateFile(maximumFencePath, 32, true);
    if (bytes === null) return "0";
    const value = bytes.toString("ascii");

    // accept only one canonical uint64 line
    if (!/^(?:0|[1-9]\d*)\n$/u.test(value) ||
      BigInt(value.trim()) > 0xffff_ffff_ffff_ffffn) {
      throw new Error("family release maximum fence is invalid");
    }
    return value.trim();
  }

  // advance one fence only after its recoverable transaction exists
  async function writeMaximumFence(value) {
    const current = await readMaximumFence();

    // permit an exact retry but never a regression
    if (BigInt(value) < BigInt(current)) {
      throw new Error("family release fence regressed");
    }
    if (value !== current) {
      await writeAdjustmentFamilyAtomic(maximumFencePath, Buffer.from(`${value}\n`), 0o600);
    }
  }

  // refuse a second action while another family transaction remains unfinished
  async function requireAvailableTransactionSlot(actionSha256) {
    const names = readdirSync(transactionsRoot).sort();

    // bound retained journal traversal and reject foreign entries
    if (names.length > 4_096 || names.some((name) =>
      !/^sha256-[a-f0-9]{64}\.json$/u.test(name))) {
      throw new Error("family transaction root contains an unknown entry");
    }
    for (const name of names) {
      const identity = name.slice(7, -5);

      // inspect only other immutable action records
      if (identity === actionSha256) continue;
      const document = await readDocument(identity);

      // release the single action slot only after a durable acknowledgement
      if (!new Set(["acknowledged", "operator_off_unapplied"])
        .has(document.record.state) && document.compensation?.state !== "verified") {
        throw new Error("another family transaction is unfinished");
      }
    }
  }

  // persist the root acknowledgement used as future source-commit authority
  async function writeCurrent(record, live, state) {
    const value = {
      acknowledgedAt: record.verifiedAt ?? record.failedAt ?? options.clock(),
      actionSha256: state === "acknowledged"
        ? record.input.actionSha256
        : live.actionSha256,
      commit: live.commit,
      contractVersion: ADJUSTMENT_FAMILY_CURRENT_VERSION,
      family: record.input.family,
      imageDigestsSha256: live.imageDigestsSha256,
      release: live.release,
      state,
    };
    await writeAdjustmentFamilyAtomic(currentPath, canonicalFamilyBytes(value), 0o600);
  }

  // compensate one prepared mutation and retain its categorical result
  async function compensatePrepared(record, proof) {
    try {
      const live = await host.compensate(record, proof);
      await writeCurrent(record, live, "compensated");
      await updateDocument(record.input.actionSha256, (document) => ({
        ...document,
        compensation: {
          actionSha256: live.actionSha256,
          release: live.release,
          state: "verified",
          verifiedAt: options.clock(),
        },
      }));
      return live;
    } catch (error) {
      await updateDocument(record.input.actionSha256, (document) => ({
        ...document,
        compensation: {
          actionSha256: document.proof.compensationActionSha256,
          release: record.input.compensatingRelease,
          state: "failed",
          verifiedAt: null,
        },
      })).catch(() => undefined);
      throw error;
    }
  }

  return Object.freeze({
    // the caller holds the inherited kernel lock across this callback
    withReleaseLock: async (callback) => await callback(),
    clock: options.clock,
    // return only the executor-owned record projection
    readPrepared: async (actionSha256) => {
      loadedDocument = await readDocument(actionSha256);
      return loadedDocument?.record ?? null;
    },
    // combine host state with the durable monotonic fence
    inspectCurrent: async () => ({
      ...await host.inspectCurrent(),
      maximumFence: await readMaximumFence(),
    }),
    verifyProof: async (input, current) => {
      // a prepared retry remains valid only while it owns the global fence
      if (loadedDocument?.record.state === "prepared" &&
        BigInt(input.fencingToken) < BigInt(current.maximumFence)) {
        throw new Error("family prepared transaction fence is obsolete");
      }
      return await host.verifyProof(input, current, loadedDocument?.record ?? null);
    },
    // durably retain one exact no-mutation status before exposing it to the caller
    acknowledgeOperatorOff: async (record) => {
      validateAdjustmentFamilyOperatorOffRecord(record);
      await initialize();
      const path = transactionPath(record.input.actionSha256);
      const existing = await readDocument(record.input.actionSha256);
      const document = {
        contractVersion: ADJUSTMENT_FAMILY_OPERATOR_OFF_TRANSACTION_VERSION,
        record,
      };

      // create once or converge an exact response-lost retry
      if (existing === null) {
        await requireAvailableTransactionSlot(record.input.actionSha256);
        await writeAdjustmentFamilyExclusive(path, canonicalFamilyBytes(document), 0o600);
      } else if (!canonicalFamilyBytes(existing).equals(canonicalFamilyBytes(document))) {
        throw new Error("family operator-off transaction collision");
      }
      await writeMaximumFence(record.input.fencingToken);
      return Object.freeze(record);
    },
    // create one immutable transaction before advancing its fence
    prepare: async (record, proof) => {
      await initialize();
      const path = transactionPath(record.input.actionSha256);
      const existing = await readDocument(record.input.actionSha256);
      const document = {
        compensation: {
          actionSha256: proof.durable.compensationActionSha256,
          release: record.input.compensatingRelease,
          state: "absent",
          verifiedAt: null,
        },
        contractVersion: ADJUSTMENT_FAMILY_TRANSACTION_VERSION,
        proof: proof.durable,
        record,
      };

      // reuse only byte-identical prepared evidence
      if (existing === null) {
        await requireAvailableTransactionSlot(record.input.actionSha256);
        await writeAdjustmentFamilyExclusive(path, canonicalFamilyBytes(document), 0o600);
      } else if (!canonicalFamilyBytes(existing).equals(canonicalFamilyBytes(document))) {
        throw new Error("family prepared transaction collision");
      }
      await writeMaximumFence(record.input.fencingToken);
      return record;
    },
    // publish applying before the first catalog or service mutation
    markApplying: async (record) => {
      await updateDocument(record.input.actionSha256, (document) => ({
        ...document,
        record: { ...record, state: "applying" },
      }));
    },
    // restore only the precomputed family compensation on target failure
    applyImages: async (record, proof) => {
      try {
        return await host.applyTarget(record, proof);
      } catch (error) {
        await compensatePrepared(record, proof).catch(() => undefined);
        throw error;
      }
    },
    // compensate a target that started but failed independent live verification
    verifyLive: async (record, proof) => {
      try {
        return await host.verifyLive(record, proof);
      } catch (error) {
        await compensatePrepared(record, proof).catch(() => undefined);
        throw error;
      }
    },
    // retain failure without erasing a verified compensation result
    recordFailure: async (record) => {
      const current = await readDocument(record.input.actionSha256);

      // cover acknowledgement and other post-mutation failures with the same compensation
      if (current === null) throw new Error("family transaction is unavailable");
      if (current.compensation.state !== "verified") {
        await compensatePrepared(record, { durable: current.proof }).catch(() => undefined);
      }
      await updateDocument(record.input.actionSha256, (document) => ({
        ...document,
        record,
      }));
    },
    // acknowledge only after live verification and publish future source authority
    acknowledge: async (record, live) => {
      await writeCurrent(record, live, "acknowledged");
      await updateDocument(record.input.actionSha256, (document) => ({
        ...document,
        record,
      }));
      return record;
    },
    // reconcile an applying or failed transaction from durable compensation evidence
    reconcileCompensation: async (actionSha256) => {
      const document = await readDocument(actionSha256, false);
      if (document === null) throw new Error("family transaction is unavailable");
      if (document.compensation.state === "verified") return document.compensation;
      return await compensatePrepared(document.record, { durable: document.proof });
    },
    // expose only a bounded categorical transaction projection
    status: async (actionSha256) => {
      const document = await readDocument(actionSha256, false);
      if (document === null) {
        return Object.freeze({ actionSha256, contractVersion: "adjustment-family-release-status/v1",
          state: "absent" });
      }
      // project the exact disjoint nonserving receipt without synthetic proof fields
      if (document.contractVersion === ADJUSTMENT_FAMILY_OPERATOR_OFF_TRANSACTION_VERSION) {
        return adjustmentFamilyOperatorOffStatus(document.record);
      }
      return Object.freeze({
        actionSha256,
        compensatingRelease: document.record.input.compensatingRelease,
        compensationState: document.compensation.state,
        contractVersion: "adjustment-family-release-status/v1",
        family: document.record.input.family,
        fencingToken: document.record.input.fencingToken,
        outcome: document.record.outcome ?? null,
        state: document.record.state,
        targetRelease: document.record.input.targetRelease,
      });
    },
  });
}

// publish the one-time inert v14 source authority without inventing an action
export async function bootstrapAdjustmentFamilyReleaseCurrent(input, options = {}) {
  requireExactKeys(input, [
    "catalogSha256", "commit", "release", "serverImage", "settingsSha256", "webImage",
  ], "family release bootstrap authority");
  requireObject(options, "family release bootstrap options");

  // keep optional test seams closed to the clock and private root
  if (Object.keys(options).some((key) => !["clock", "root"].includes(key))) {
    throw new TypeError("family release bootstrap options are invalid");
  }
  const clock = options.clock ?? (() => new Date().toISOString());
  const root = resolve(options.root ?? ADJUSTMENT_FAMILY_RELEASE_ROOT);

  // accept only closed immutable identities from the independently verified bridge
  if (typeof clock !== "function" || !root.startsWith("/") ||
    !/^[a-f0-9]{40}$/u.test(input.commit) ||
    !ADJUSTMENT_RELEASE_REFERENCE.test(input.serverImage) ||
    !ADJUSTMENT_RELEASE_REFERENCE.test(input.webImage) ||
    input.serverImage.includes("weather-web") || input.webImage.includes("weather-server")) {
    throw new TypeError("family release bootstrap authority is invalid");
  }
  requireFamilyRelease(input.release, "family bootstrap release");
  requireFamilySha256(input.catalogSha256, "family bootstrap catalog SHA256");
  requireFamilySha256(input.settingsSha256, "family bootstrap settings SHA256");
  const environment = {
    WEATHER_SERVER_IMAGE: input.serverImage,
    WEATHER_WEB_IMAGE: input.webImage,
  };

  const imageDigestsSha256 = adjustmentFamilyImageIdentity(environment);
  const bootstrappedAt = clock();

  // retain one canonical millisecond UTC creation time
  if (typeof bootstrappedAt !== "string" ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(bootstrappedAt)) {
    throw new TypeError("family bootstrap time is invalid");
  }
  const value = {
    bootstrappedAt,
    catalogSha256: input.catalogSha256,
    commit: input.commit,
    contractVersion: ADJUSTMENT_FAMILY_BOOTSTRAP_CURRENT_VERSION,
    imageDigestsSha256,
    release: input.release,
    serverImage: input.serverImage,
    settingsSha256: input.settingsSha256,
    state: "inert_v14_bootstrap",
    webImage: input.webImage,
  };
  await ensureAdjustmentFamilyPrivateDirectory(root);
  const path = join(root, "current.json");
  const existingBytes = await readAdjustmentFamilyPrivateFile(path, 8 * 1_024, true);

  // allow only an exact identity retry of this one-time bridge authority
  if (existingBytes !== null) {
    const existing = parseCanonicalFamilyJson(existingBytes, "family current authority");
    const identityKeys = [
      "catalogSha256", "commit", "contractVersion", "imageDigestsSha256", "release",
      "serverImage", "state", "webImage",
    ];
    // retain commit-time settings evidence without freezing later valid operator changes
    if (identityKeys.some((key) => existing[key] !== value[key])) {
      throw new Error("family release bootstrap authority already exists");
    }
    return Object.freeze(existing);
  }
  await writeAdjustmentFamilyExclusive(path, canonicalFamilyBytes(value), 0o600);
  return Object.freeze(value);
}

// verify the retained inert bootstrap without creating missing authority
export async function verifyAdjustmentInertV14BootstrapCurrent(input, options = {}) {
  requireExactKeys(input, ["catalogSha256", "release", "serverImage", "webImage"],
    "inert v14 bootstrap verification");
  requireExactKeys(options, Object.keys(options).length === 0 ? [] : ["root"],
    "inert v14 bootstrap verification options");
  requireFamilyRelease(input.release, "inert v14 bootstrap release");
  requireFamilySha256(input.catalogSha256, "inert v14 bootstrap catalog");
  const root = resolve(options.root ?? ADJUSTMENT_FAMILY_RELEASE_ROOT);
  const bytes = await readAdjustmentFamilyPrivateFile(join(root, "current.json"), 8 * 1_024, false);
  const current = parseCanonicalFamilyJson(bytes, "inert v14 bootstrap authority");
  requireExactKeys(current, ["bootstrappedAt", "catalogSha256", "commit", "contractVersion",
    "imageDigestsSha256", "release", "serverImage", "settingsSha256", "state", "webImage"],
  "inert v14 bootstrap authority");
  requireFamilySha256(current.settingsSha256, "inert v14 bootstrap settings");
  const imageIdentity = adjustmentFamilyImageIdentity({ WEATHER_SERVER_IMAGE: input.serverImage,
    WEATHER_WEB_IMAGE: input.webImage });
  // accept only the exact retained authority created before the release commit
  if (current.contractVersion !== ADJUSTMENT_FAMILY_BOOTSTRAP_CURRENT_VERSION ||
    current.state !== "inert_v14_bootstrap" || current.imageDigestsSha256 !== imageIdentity ||
    !/^[a-f0-9]{40}$/u.test(current.commit) ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(current.bootstrappedAt) ||
    Object.keys(input).some((key) => current[key] !== input[key])) {
    throw new Error("inert v14 bootstrap authority differs");
  }
  return Object.freeze(current);
}

// discard only the exact failed inactive bootstrap after source restoration
export async function discardFailedAdjustmentInertV14Bootstrap(input, options = {}) {
  requireExactKeys(input, ["catalogSha256", "commit", "release", "serverImage", "settingsSha256", "webImage"],
    "failed inert v14 bootstrap identity");
  requireExactKeys(options, Object.keys(options).length === 0 ? [] : ["root"],
    "failed inert v14 bootstrap options");
  requireFamilyRelease(input.release, "failed inert v14 release");
  requireFamilySha256(input.catalogSha256, "failed inert v14 catalog");
  requireFamilySha256(input.settingsSha256, "failed inert v14 settings");
  // reject nonliteral source proof inputs before any file operation
  if (!/^[a-f0-9]{40}$/u.test(input.commit) ||
    ADJUSTMENT_RELEASE_REFERENCE.exec(input.serverImage)?.[1] !== "server" ||
    ADJUSTMENT_RELEASE_REFERENCE.exec(input.webImage)?.[1] !== "web") {
    throw new TypeError("failed inert v14 bootstrap inputs are invalid");
  }
  const root = resolve(options.root ?? ADJUSTMENT_FAMILY_RELEASE_ROOT);
  await ensureAdjustmentFamilyPrivateDirectory(root);
  const path = join(root, "current.json");
  const bytes = await readAdjustmentFamilyPrivateFile(path, 8 * 1_024, true);
  // an interrupted publication may leave no authority to remove
  if (bytes === null) return "absent";
  const current = parseCanonicalFamilyJson(bytes, "failed inert v14 bootstrap authority");
  requireExactKeys(current, ["bootstrappedAt", "catalogSha256", "commit", "contractVersion",
    "imageDigestsSha256", "release", "serverImage", "settingsSha256", "state", "webImage"],
  "failed inert v14 bootstrap authority");
  const imageIdentity = adjustmentFamilyImageIdentity({ WEATHER_SERVER_IMAGE: input.serverImage,
    WEATHER_WEB_IMAGE: input.webImage });
  const immutableIdentityKeys = ["catalogSha256", "commit", "release", "serverImage", "webImage"];
  // a model action or another bridge authority is never disposable bootstrap state
  if (current.contractVersion !== ADJUSTMENT_FAMILY_BOOTSTRAP_CURRENT_VERSION ||
    current.state !== "inert_v14_bootstrap" || current.imageDigestsSha256 !== imageIdentity ||
    immutableIdentityKeys.some((key) => current[key] !== input[key])) {
    throw new Error("failed inert v14 bootstrap authority differs");
  }
  const rechecked = await readAdjustmentFamilyPrivateFile(path, 8 * 1_024, false);
  // recheck exact immutable bytes while the caller holds the global release lock
  if (!bytes.equals(rechecked)) throw new Error("failed inert v14 bootstrap changed before removal");
  await unlink(path);
  await fsyncAdjustmentFamilyDirectory(root);
  return "discarded_exact_failed_bootstrap";
}

// require one lowercase content identity
function requireFamilySha256(value, label) {
  if (typeof value !== "string" || !/^[a-f0-9]{64}$/u.test(value)) {
    throw new TypeError(`${label} is invalid`);
  }
  return value;
}

// encode one canonical newline-framed family document
function canonicalFamilyBytes(value) {
  return Buffer.from(canonicalJson(value));
}

// parse one canonical newline-framed family document
function parseCanonicalFamilyJson(bytes, label) {
  let value;

  // reject malformed json without normalization
  try {
    value = JSON.parse(bytes.toString("utf8"));
  } catch {
    throw new TypeError(`${label} JSON is invalid`);
  }
  if (value === null || typeof value !== "object" || Array.isArray(value) ||
    !canonicalFamilyBytes(value).equals(bytes)) {
    throw new TypeError(`${label} is not canonical`);
  }
  return value;
}

// create or verify one owner-private fixed directory
async function ensureAdjustmentFamilyPrivateDirectory(path) {
  await mkdir(path, { mode: 0o700, recursive: true });
  const details = await realpath(path);
  const status = lstatSync(path);

  // reject links, foreign owners and permissive transaction roots
  if (details !== path || !status.isDirectory() || status.isSymbolicLink() ||
    status.uid !== process.getuid() || status.gid !== process.getgid() ||
    (status.mode & 0o777) !== 0o700) {
    throw new Error("family release state root is unsafe");
  }
}

// read one bounded owner-private regular file without links
async function readAdjustmentFamilyPrivateFile(path, maximumBytes, optional) {
  let handle;

  // distinguish only genuine absence when the caller permits it
  try {
    handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  } catch (error) {
    if (optional && error?.code === "ENOENT") return null;
    throw error;
  }
  try {
    const before = await handle.stat();

    // require one private single-link file owned by this privileged identity
    if (!before.isFile() || before.nlink !== 1 || before.uid !== process.getuid() ||
      before.gid !== process.getgid() || (before.mode & 0o777) !== 0o600 ||
      before.size < 1 || before.size > maximumBytes) {
      throw new Error("family release state file is unsafe");
    }
    const bytes = await handle.readFile();
    const after = await handle.stat();

    // reject replacement or mutation during the bounded read
    if (before.dev !== after.dev || before.ino !== after.ino || before.size !== after.size ||
      before.mtimeMs !== after.mtimeMs) {
      throw new Error("family release state file changed during read");
    }
    return bytes;
  } finally {
    await handle.close();
  }
}

// publish one new private file and fsync its parent
async function writeAdjustmentFamilyExclusive(path, bytes, mode) {
  const handle = await open(
    path,
    constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | constants.O_NOFOLLOW,
    mode,
  );
  try {
    await handle.writeFile(bytes);
    await handle.sync();
  } finally {
    await handle.close();
  }
  await fsyncAdjustmentFamilyDirectory(dirname(path));
}

// replace one private state file atomically
async function writeAdjustmentFamilyAtomic(path, bytes, mode) {
  const temporary = join(dirname(path), `.family-${randomUUID()}.tmp`);
  let handle = await open(
    temporary,
    constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | constants.O_NOFOLLOW,
    mode,
  );
  try {
    await handle.writeFile(bytes);
    await handle.sync();
    await handle.close();
    handle = null;
    await rename(temporary, path);
    await fsyncAdjustmentFamilyDirectory(dirname(path));
  } catch (error) {
    // close and remove only this unique temporary file
    if (handle !== null) await handle.close().catch(() => undefined);
    await unlink(temporary).catch(() => undefined);
    throw error;
  }
}

// make one directory entry update durable
async function fsyncAdjustmentFamilyDirectory(path) {
  const handle = await open(path, constants.O_RDONLY | constants.O_DIRECTORY);
  try {
    await handle.sync();
  } finally {
    await handle.close();
  }
}

// create the fixed-root production transaction ports
export function createProductionAdjustmentFamilyReleasePorts(options = {}) {
  const allowed = new Set(["clock", "evidenceRoot", "family", "fetchImpl", "lockGuard", "paths"]);

  // reject caller-created production adapters or hidden path knobs
  if (options === null || typeof options !== "object" || Array.isArray(options) ||
    Object.keys(options).some((key) => !allowed.has(key))) {
    throw new TypeError("production family release options are invalid");
  }
  const startedAt = new Date().toISOString();
  const clock = options.clock ?? (() => startedAt);
  const paths = {
    catalog: ADJUSTMENT_FAMILY_CATALOG_PATH,
    compose: resolve(dirname(import.meta.filename), "../compose.yaml"),
    currentRelease: "/opt/weather/current/deploy/state/current-release",
    evidenceRoot: options.evidenceRoot ?? "/var/lib/weather/xweather/adjustment-evidence",
    releases: "/opt/weather/current/deploy/releases",
    root: ADJUSTMENT_FAMILY_RELEASE_ROOT,
    settings: ADJUSTMENT_FAMILY_SETTINGS_PATH,
    update: resolve(dirname(import.meta.filename), "update.sh"),
    ...(options.paths ?? {}),
  };

  // require absolute fixed-root replacements only for isolated regression tests
  if (Object.values(paths).some((path) => typeof path !== "string" || !path.startsWith("/")) ||
    typeof clock !== "function") {
    throw new TypeError("production family release paths are invalid");
  }
  const fetchImpl = options.fetchImpl ?? fetch;
  const host = createProductionAdjustmentFamilyHost({ clock, family: options.family ?? null,
    fetchImpl, paths });
  const durable = createAdjustmentFamilyReleasePorts({ clock, host, root: paths.root });
  const lockGuard = options.lockGuard ?? (() => requireInheritedAdjustmentFamilyLock());

  // validate the inherited lock immediately before every transaction callback
  return Object.freeze({
    ...durable,
    withReleaseLock: async (callback) => {
      await lockGuard();
      return await callback();
    },
  });
}

// run one fixed nine-operand production transaction
export async function runAdjustmentFamilyReleaseCommand(argumentsList, options = {}) {
  // retain the same exact positional contract across ssh, bash and node
  if (!Array.isArray(argumentsList) || argumentsList.length !== 9) {
    throw new TypeError("family release requires exactly nine arguments");
  }
  const input = {
    targetRelease: argumentsList[0],
    compensatingRelease: argumentsList[1],
    expectedCurrentRelease: argumentsList[2],
    expectedSourceRelease: argumentsList[3],
    expectedSettingsSha256: argumentsList[4],
    family: argumentsList[5],
    actionSha256: argumentsList[6],
    reportSha256: argumentsList[7],
    fencingToken: argumentsList[8],
  };
  const ports = options.ports ?? createProductionAdjustmentFamilyReleasePorts({ family: input.family });
  const result = await executeAdjustmentFamilyRelease(input, ports);

  // expose only the exact durable no-mutation acknowledgement
  if (result.state === "operator_off_unapplied") {
    const status = await ports.status(input.actionSha256);
    const expected = adjustmentFamilyOperatorOffStatus(result);
    if (canonicalJson(status) !== canonicalJson(expected)) {
      throw new Error("family operator-off status differs");
    }
    return status;
  }

  // finish crash or target-failure reconciliation using only prepared evidence
  if (result.state === "compensation_required") {
    await ports.reconcileCompensation(input.actionSha256);
  }
  return await ports.status(input.actionSha256);
}

// project one fixed-root transaction without changing it
export async function readAdjustmentFamilyReleaseStatus(actionSha256, options = {}) {
  requireFamilySha256(actionSha256, "family action SHA256");
  const ports = options.ports ?? createProductionAdjustmentFamilyReleasePorts({
    lockGuard: () => undefined,
  });
  return await ports.status(actionSha256);
}

// project the exact live family source authority without exposing host paths
export async function readAdjustmentFamilyReleaseCurrent(family, options = {}) {
  if (!["temperature", "wind", "rain"].includes(family) || options === null ||
    typeof options !== "object" || Array.isArray(options) ||
    Object.keys(options).some((key) => key !== "host") ||
    (options.host !== undefined && typeof options.host?.inspectCurrent !== "function")) {
    throw new TypeError("family current authority request is invalid");
  }
  let host = options.host;

  // construct only the fixed production observation boundary by default
  if (host === undefined) {
    const paths = {
      catalog: ADJUSTMENT_FAMILY_CATALOG_PATH,
      compose: resolve(dirname(import.meta.filename), "../compose.yaml"),
      currentRelease: "/opt/weather/current/deploy/state/current-release",
      evidenceRoot: "/var/lib/weather/xweather/adjustment-evidence",
      releases: "/opt/weather/current/deploy/releases",
      root: ADJUSTMENT_FAMILY_RELEASE_ROOT,
      settings: ADJUSTMENT_FAMILY_SETTINGS_PATH,
      update: resolve(dirname(import.meta.filename), "update.sh"),
    };
    host = createProductionAdjustmentFamilyHost({
      clock: () => new Date().toISOString(),
      family,
      fetchImpl: fetch,
      paths,
    });
  }
  const current = await host.inspectCurrent();
  const active = current.installedReceiptSha256BySlot?.active ?? null;
  const shadow = current.installedReceiptSha256BySlot?.shadow ??
    current.installedReceiptSha256 ?? null;
  const control = family === "rain"
    ? current.installedReceiptSha256BySlot?.control ?? null
    : null;
  if (typeof current.commit !== "string" || !/^[a-f0-9]{40}$/u.test(current.commit)) {
    throw new Error("family current commit is invalid");
  }
  requireFamilyRelease(current.release, "family current release");
  for (const [value, label] of [
    [active, "family current active receipt"],
    [control, "family current control receipt"],
    [shadow, "family current shadow receipt"],
  ]) {
    if (value !== null) requireFamilySha256(value, label);
  }
  const settingsSha256 = createHash("sha256").update(current.settingsBytes).digest("hex");
  const catalogSha256 = createHash("sha256").update(current.catalogBytes).digest("hex");
  const sourceServerImageDigest = current.environment.WEATHER_SERVER_IMAGE.split("@")[1];
  requireFamilySha256(sourceServerImageDigest.slice("sha256:".length),
    "family current server image digest");
  return Object.freeze({
    activeInstalledReceiptSha256: active,
    catalogSha256,
    commit: current.commit,
    controlInstalledReceiptSha256: control,
    contractVersion: "adjustment-family-release-current-status/v1",
    family,
    release: current.release,
    settingsSha256,
    shadowInstalledReceiptSha256: shadow,
    sourceServerImageDigest,
  });
}

// bind the inherited descriptor to the existing release-state directory lock
function requireInheritedAdjustmentFamilyLock() {
  const text = process.env.WEATHER_RELEASE_LOCK_FD;

  // reject absent, alternate or unbounded descriptor encodings
  if (typeof text !== "string" || !/^[1-9]\d{0,2}$/u.test(text)) {
    throw new Error("family release lock is unavailable");
  }
  const descriptor = Number(text);
  const held = fstatSync(descriptor, { bigint: true });
  const root = statSync(ADJUSTMENT_FAMILY_LOCK_ROOT, { bigint: true });

  // require the exact fixed directory inode opened by update.sh
  if (!held.isDirectory() || held.dev !== root.dev || held.ino !== root.ino) {
    throw new Error("family release lock identity differs");
  }
}

// authenticate one bootstrap against immutable release state and valid live settings
export function validateAdjustmentFamilyBootstrapLiveAuthority(
  active,
  environment,
  catalogBytes,
  settingsBytes,
) {
  const current = validateAdjustmentFamilyCurrentDocument(active);
  let settings;

  // require the exact byte-oriented host observation contract
  if (!Buffer.isBuffer(catalogBytes) || !Buffer.isBuffer(settingsBytes)) {
    throw new TypeError("family release live authority bytes are invalid");
  }
  // reject malformed live settings without binding them to the commit-time snapshot
  try {
    settings = JSON.parse(Buffer.from(settingsBytes));
  } catch {
    throw new Error("family release live settings are invalid");
  }
  reconcileAdjustmentFamilySettings(settings, settings, "temperature");
  if (current.contractVersion !== ADJUSTMENT_FAMILY_BOOTSTRAP_CURRENT_VERSION ||
    current.serverImage !== environment.WEATHER_SERVER_IMAGE ||
    current.webImage !== environment.WEATHER_WEB_IMAGE ||
    current.imageDigestsSha256 !== adjustmentFamilyImageIdentity(environment) ||
    current.catalogSha256 !== createHash("sha256").update(catalogBytes).digest("hex")) {
    throw new Error("family release bootstrap live authority differs");
  }
  return current.commit;
}

// create the real host observation, proof and image-switch boundary
function createProductionAdjustmentFamilyHost({ clock, family, fetchImpl, paths }) {
  // inspect the exact running source and its root-owned state
  async function inspectCurrent() {
    const release = (await readHostFile(paths.currentRelease, 128, 0o600, [0]))
      .toString("ascii").trim();
    requireFamilyRelease(release, "current release");
    const environment = await readFamilyReleaseEnvironment(paths, release);
    await verifyRunningAdjustmentImages(paths, environment);
    const settingsBytes = await readHostFile(paths.settings, 4_096, 0o600, [0, 10_002]);
    const catalogBytes = await readHostFile(
      paths.catalog,
      ADJUSTMENT_FAMILY_MAXIMUM_CATALOG_BYTES,
      0o644,
      [0],
    );
    const catalog = validateInstalledAdjustmentCatalog(catalogBytes);
    await verifyInstalledAdjustmentCatalog(environment.WEATHER_SERVER_IMAGE, paths.catalog);
    const active = await readAdjustmentFamilyCurrent(paths.root, true);
    let commit;

    // seed only the frozen predecessor or an explicit inert-v14 bridge authority
    if (release === ADJUSTMENT_FAMILY_BASELINE_RELEASE) {
      // validate mutable settings independently for the retained baseline
      const settings = JSON.parse(settingsBytes);
      reconcileAdjustmentFamilySettings(settings, settings, "temperature");
      if (environment.WEATHER_SERVER_IMAGE !== ADJUSTMENT_INERT_V14_SOURCE_REFERENCES.get("server") ||
        environment.WEATHER_WEB_IMAGE !== ADJUSTMENT_INERT_V14_SOURCE_REFERENCES.get("web")) {
        throw new Error("family release baseline image identity differs");
      }
      commit = ADJUSTMENT_FAMILY_BASELINE_COMMIT;
    } else {
      // later source authority comes only from a root-published live authority
      if (active === null || active.release !== release) {
        throw new Error("family release current acknowledgement is unavailable");
      }
      if (active.imageDigestsSha256 !== adjustmentFamilyImageIdentity(environment)) {
        throw new Error("family release current image acknowledgement differs");
      }

      // authenticate the bootstrap without freezing later valid operator settings
      if (active.contractVersion === ADJUSTMENT_FAMILY_BOOTSTRAP_CURRENT_VERSION) {
        validateAdjustmentFamilyBootstrapLiveAuthority(
          active,
          environment,
          catalogBytes,
          settingsBytes,
        );
      } else {
        const settings = JSON.parse(settingsBytes);
        reconcileAdjustmentFamilySettings(settings, settings, "temperature");
      }
      commit = active.commit;
    }
    return {
      catalogBytes,
      commit,
      environment,
      installedReceiptSha256: installedFamilyReceiptSha256(catalog, family),
      installedReceiptSha256BySlot: {
        active: installedFamilyReceiptSha256(catalog, family, "active"),
        control: family === "rain"
          ? installedFamilyReceiptSha256(catalog, family, "control")
          : null,
        shadow: installedFamilyReceiptSha256(catalog, family, "shadow"),
      },
      release,
      settingsBytes,
    };
  }

  // assemble independent Git, image, anchor, capacity and catalog proof
  async function verifyProof(input, current, priorRecord) {
    const targetEnvironment = await readFamilyReleaseEnvironment(paths, input.targetRelease);
    const compensationEnvironment = await readFamilyReleaseEnvironment(
      paths,
      input.compensatingRelease,
    );
    const readJson = async (suffix) => await readPublicWeatherGitJsonWithFetch(suffix, fetchImpl);
    const targetActionPath = `config/forecast-adjustments/ballydidean/actions/sha256-${input.actionSha256}.json`;
    const targetGitBytes = await readPublicWeatherRepositoryFile(
      current.commit,
      input.targetRelease,
      targetActionPath,
      readJson,
    );
    const targetAction = parseCanonicalFamilyJson(targetGitBytes, "family target action");
    const git = await verifyAdjustmentFamilyGitRelease({
      action: targetAction,
      readJson,
      targetRelease: input.targetRelease,
    });
    const targetImageBytes = readAdjustmentImageFile(
      targetEnvironment.WEATHER_SERVER_IMAGE,
      targetActionPath,
      32 * 1_024,
    );

    // require the published target image to contain the verified repository bytes
    if (!targetImageBytes.equals(targetGitBytes)) {
      throw new Error("family target image action differs");
    }
    const compensationCommit = await resolvePublicWeatherTag(input.compensatingRelease, readJson);
    const compensationCompare = await readJson(
      `compare/${git.targetCommit}...${compensationCommit}?per_page=100`,
    );
    const compensationActionFiles = Array.isArray(compensationCompare.files)
      ? compensationCompare.files.filter((file) =>
          /^config\/forecast-adjustments\/ballydidean\/actions\/sha256-[a-f0-9]{64}\.json$/u
            .test(file.filename) && file.status === "added")
      : [];

    // discover exactly one new immutable compensation action from the closed diff
    if (compensationActionFiles.length !== 1) {
      throw new Error("family compensation action is unavailable");
    }
    const compensationPath = compensationActionFiles[0].filename;
    const compensationActionSha256 = /sha256-([a-f0-9]{64})\.json$/u
      .exec(compensationPath)?.[1];
    requireFamilySha256(compensationActionSha256, "family compensation action SHA256");
    const compensationGitBytes = await readPublicWeatherRepositoryFile(
      git.targetCommit,
      input.compensatingRelease,
      compensationPath,
      readJson,
    );
    const compensationAction = parseCanonicalFamilyJson(
      compensationGitBytes,
      "family compensation action",
    );
    const compensationGit = await verifyAdjustmentFamilyGitRelease({
      action: compensationAction,
      readJson,
      targetRelease: input.compensatingRelease,
    });
    const compensationImageBytes = readAdjustmentImageFile(
      compensationEnvironment.WEATHER_SERVER_IMAGE,
      compensationPath,
      32 * 1_024,
    );

    // require the prebuilt compensation image to contain its verified action
    if (!compensationImageBytes.equals(compensationGitBytes)) {
      throw new Error("family compensation image action differs");
    }
    const capacity = await readAdjustmentFamilyCapacity(paths.root, input.actionSha256);
    const authority = await authorizeAdjustmentFamilyAction(
      targetAction,
      input.actionSha256,
      paths.evidenceRoot,
    );
    const baselineCatalog = validateInstalledAdjustmentCatalog(current.catalogBytes);
    const controlReference = targetAction.contractVersion ===
      RAIN_CONTROL_REFERENCE_ACTION_CONTRACT_VERSION;
    const targetCatalog = controlReference
      ? buildInstalledAdjustmentRainControlReferenceCatalog({
          action: targetAction,
          actionSha256: input.actionSha256,
          clock: () => priorRecord?.createdAt ?? clock(),
          custodyAnchorSha256: createHash("sha256")
            .update(canonicalFamilyBytes(authority)).digest("hex"),
          environment: targetEnvironment,
          imageCommit: git.targetCommit,
          imageRead: (path, maximumBytes) => readAdjustmentImageFile(
            targetEnvironment.WEATHER_SERVER_IMAGE,
            path,
            maximumBytes,
          ),
          settingsBytes: current.settingsBytes,
          source: baselineCatalog,
        })
      : await buildInstalledAdjustmentTargetCatalog({
          action: targetAction,
          actionSha256: input.actionSha256,
          clock: () => priorRecord?.createdAt ?? clock(),
          environment: targetEnvironment,
          family: input.family,
          imageCommit: git.targetCommit,
          imageRead: (path, maximumBytes) => readAdjustmentImageFile(
            targetEnvironment.WEATHER_SERVER_IMAGE,
            path,
            maximumBytes,
          ),
          settingsBytes: current.settingsBytes,
          source: baselineCatalog,
        });
    // bind control custody to the exact target image members before mutation
    if (controlReference) {
      validateAdjustmentRainControlReferenceBinding(
        targetAction,
        input.actionSha256,
        authority,
        targetCatalog,
      );
    }
    // bind non-qualification custody to the independently read target image registration
    if (authority.contractVersion === "adjustment-development-custody-anchor/v1") {
      validateAdjustmentDevelopmentActionBinding(targetAction, input.actionSha256,
        authority, targetCatalog, await readAdjustmentRevisionCaptureEpochWitness());
    }
    const targetCatalogBytes = canonicalFamilyBytes(targetCatalog);
    await verifyCatalogBytesWithImage(
      targetEnvironment.WEATHER_SERVER_IMAGE,
      targetCatalogBytes,
      paths.root,
    );
    await verifyCatalogBytesWithImage(
      compensationEnvironment.WEATHER_SERVER_IMAGE,
      current.catalogBytes,
      paths.root,
    );
    const sourceRegistryBytes = readAdjustmentImageFile(
      current.environment.WEATHER_SERVER_IMAGE,
      ADJUSTMENT_FAMILY_REGISTRIES.get(input.family),
      ADJUSTMENT_RUNTIME_PACKAGE_MAX_BYTES,
    );
    const targetRegistryBytes = readAdjustmentImageFile(
      targetEnvironment.WEATHER_SERVER_IMAGE,
      ADJUSTMENT_FAMILY_REGISTRIES.get(input.family),
      ADJUSTMENT_RUNTIME_PACKAGE_MAX_BYTES,
    );
    const compensationRegistryBytes = readAdjustmentImageFile(
      compensationEnvironment.WEATHER_SERVER_IMAGE,
      ADJUSTMENT_FAMILY_REGISTRIES.get(input.family),
      ADJUSTMENT_RUNTIME_PACKAGE_MAX_BYTES,
    );

    // family compensation must restore exact pre-action serving bytes
    if (!compensationRegistryBytes.equals(sourceRegistryBytes)) {
      throw new Error("family compensation serving baseline differs");
    }
    const familyIdentitySha256 = adjustmentFamilyIdentity(
      input.family,
      targetRegistryBytes,
      targetCatalog,
    );
    const compensationFamilyIdentitySha256 = adjustmentFamilyIdentity(
      input.family,
      compensationRegistryBytes,
      baselineCatalog,
    );
    const imageDigestsSha256 = adjustmentFamilyImageIdentity(targetEnvironment);
    const compensationImageDigestsSha256 = adjustmentFamilyImageIdentity(
      compensationEnvironment,
    );
    return {
      actionBytes: targetImageBytes,
      authority,
      capacity,
      compensation: {
        actionBytes: compensationImageBytes,
        actionSha256: compensationActionSha256,
        git: compensationGit,
      },
      durable: {
        baselineCatalogBase64: current.catalogBytes.toString("base64"),
        compensationActionSha256,
        compensationCommit,
        compensationFamilyIdentitySha256,
        compensationImageDigestsSha256,
        sourceSettingsBase64: current.settingsBytes.toString("base64"),
        targetCatalogBase64: targetCatalogBytes.toString("base64"),
        targetCommit: git.targetCommit,
        targetFamilyIdentitySha256: familyIdentitySha256,
        targetImageDigestsSha256: imageDigestsSha256,
      },
      familyIdentitySha256,
      git,
      imageDigestsSha256,
      targetCommit: git.targetCommit,
    };
  }

  // install the target catalog before recreating its exact application images
  async function applyTarget(record, proof) {
    const bytes = decodeDurableCatalog(proof.durable.targetCatalogBase64);
    const environment = await readFamilyReleaseEnvironment(paths, record.input.targetRelease);
    const catalog = validateInstalledAdjustmentCatalog(bytes);
    const registry = readAdjustmentImageFile(
      environment.WEATHER_SERVER_IMAGE,
      ADJUSTMENT_FAMILY_REGISTRIES.get(record.input.family),
      ADJUSTMENT_RUNTIME_PACKAGE_MAX_BYTES,
    );

    // rebind the immutable target immediately before the first catalog mutation
    if (adjustmentFamilyImageIdentity(environment) !== proof.durable.targetImageDigestsSha256 ||
      adjustmentFamilyIdentity(record.input.family, registry, catalog) !==
        proof.durable.targetFamilyIdentitySha256) {
      throw new Error("family target changed after preparation");
    }
    await verifyCatalogBytesWithImage(environment.WEATHER_SERVER_IMAGE, bytes, paths.root);
    await installAdjustmentFamilyCatalog(paths.catalog, bytes);
    runAdjustmentFamilyUpdate(paths.update, record.input.targetRelease,
      record.input.expectedCurrentRelease, record.input.expectedCurrentRelease);
  }

  // verify the target release, settings, images and family bytes independently
  async function verifyLive(record, proof) {
    return await verifyAdjustmentFamilyLive({
      actionSha256: record.input.actionSha256,
      catalogBase64: proof.durable.targetCatalogBase64,
      commit: proof.durable.targetCommit,
      expectedFamilyIdentitySha256: proof.durable.targetFamilyIdentitySha256,
      expectedImageDigestsSha256: proof.durable.targetImageDigestsSha256,
      family: record.input.family,
      paths,
      release: record.input.targetRelease,
    });
  }

  // restore the exact catalog baseline and start only the new compensation release
  async function compensate(record, proof) {
    const durable = proof.durable;
    const bytes = decodeDurableCatalog(durable.baselineCatalogBase64);
    const environment = await readFamilyReleaseEnvironment(
      paths,
      record.input.compensatingRelease,
    );
    const catalog = validateInstalledAdjustmentCatalog(bytes);
    const registry = readAdjustmentImageFile(
      environment.WEATHER_SERVER_IMAGE,
      ADJUSTMENT_FAMILY_REGISTRIES.get(record.input.family),
      ADJUSTMENT_RUNTIME_PACKAGE_MAX_BYTES,
    );

    // rebind the prebuilt compensation before restoring any catalog bytes
    if (adjustmentFamilyImageIdentity(environment) !== durable.compensationImageDigestsSha256 ||
      adjustmentFamilyIdentity(record.input.family, registry, catalog) !==
        durable.compensationFamilyIdentitySha256) {
      throw new Error("family compensation changed after preparation");
    }
    await verifyCatalogBytesWithImage(environment.WEATHER_SERVER_IMAGE, bytes, paths.root);
    await installAdjustmentFamilyCatalog(paths.catalog, bytes);
    runAdjustmentFamilyUpdate(paths.update, record.input.compensatingRelease,
      record.input.expectedCurrentRelease, record.input.targetRelease);
    const live = await verifyAdjustmentFamilyLive({
      actionSha256: durable.compensationActionSha256,
      catalogBase64: durable.baselineCatalogBase64,
      commit: durable.compensationCommit,
      expectedFamilyIdentitySha256: durable.compensationFamilyIdentitySha256,
      expectedImageDigestsSha256: durable.compensationImageDigestsSha256,
      family: record.input.family,
      paths,
      release: record.input.compensatingRelease,
    });
    const beforeSettings = Buffer.from(durable.sourceSettingsBase64, "base64");

    // compensation preserves any concurrent restrictive operator change
    if (createHash("sha256").update(beforeSettings).digest("hex") !==
      record.input.expectedSettingsSha256) {
      throw new Error("family compensation source settings differ");
    }
    reconcileAdjustmentFamilySettings(
      JSON.parse(beforeSettings),
      JSON.parse(live.settingsBytes),
      record.input.family,
    );
    return live;
  }
  return Object.freeze({ applyTarget, compensate, inspectCurrent, verifyLive, verifyProof });
}

// require one immutable Weather release tag
function requireFamilyRelease(value, label) {
  if (typeof value !== "string" || !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d{0,2}$/u.test(value)) {
    throw new TypeError(`${label} is invalid`);
  }
  return value;
}

// read one fixed host file with exact ownership and mode
async function readHostFile(path, maximumBytes, mode, owners) {
  let handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const before = await handle.stat();

    // reject links, aliases, unexpected writers and oversized state
    if (!before.isFile() || before.nlink !== 1 || !owners.includes(before.uid) ||
      before.gid !== before.uid || (before.mode & 0o777) !== mode || before.size < 1 ||
      before.size > maximumBytes || await realpath(path) !== path) {
      throw new Error("family release host file is unsafe");
    }
    const bytes = await handle.readFile();
    const after = await handle.stat();

    // reject replacement or mutation during the bounded read
    if (before.dev !== after.dev || before.ino !== after.ino || before.size !== after.size ||
      before.mtimeMs !== after.mtimeMs) {
      throw new Error("family release host file changed during read");
    }
    return bytes;
  } finally {
    await handle.close();
  }
}

// parse one exact release environment without evaluating shell syntax
async function readFamilyReleaseEnvironment(paths, release) {
  requireFamilyRelease(release, "release environment identity");
  const bytes = await readHostFile(join(paths.releases, `${release}.env`), 16 * 1_024, 0o600, [0]);
  const lines = bytes.toString("utf8").split("\n");

  // retain exactly one final newline and eleven assignments
  if (lines.pop() !== "" || lines.length !== 11 || lines.some((line) =>
    !/^[A-Z][A-Z0-9_]*=[^\r\n]+$/u.test(line))) {
    throw new Error("family release environment format differs");
  }
  const entries = lines.map((line) => {
    const separator = line.indexOf("=");
    return [line.slice(0, separator), line.slice(separator + 1)];
  });
  const environment = Object.fromEntries(entries);
  const keys = [
    "CLOUDFLARED_IMAGE", "POSTGRES_IMAGE", "WEATHER_CONTROL_PLANE_SHA256",
    "WEATHER_CONTROL_PLANE_VERSION", "WEATHER_DATABASE_NAME",
    "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH",
    "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH", "WEATHER_POSTGRES_DIR",
    "WEATHER_RELEASE", "WEATHER_SERVER_IMAGE", "WEATHER_WEB_IMAGE",
  ];
  requireExactKeys(environment, keys, "family release environment");

  // bind immutable image, release and control-plane identities
  if (environment.WEATHER_RELEASE !== release ||
    !ADJUSTMENT_RELEASE_REFERENCE.test(environment.WEATHER_SERVER_IMAGE) ||
    !ADJUSTMENT_RELEASE_REFERENCE.test(environment.WEATHER_WEB_IMAGE) ||
    !/^[^@\s]+@sha256:[a-f0-9]{64}$/u.test(environment.POSTGRES_IMAGE) ||
    !/^[^@\s]+@sha256:[a-f0-9]{64}$/u.test(environment.CLOUDFLARED_IMAGE) ||
    !/^[a-f0-9]{64}$/u.test(environment.WEATHER_CONTROL_PLANE_SHA256) ||
    !/^[1-9]\d{0,5}$/u.test(environment.WEATHER_CONTROL_PLANE_VERSION)) {
    throw new Error("family release environment identity differs");
  }
  return Object.freeze(environment);
}

// verify all live application containers use the environment's exact images
async function verifyRunningAdjustmentImages(paths, environment) {
  const expected = new Map([
    ["api", environment.WEATHER_SERVER_IMAGE],
    ["worker", environment.WEATHER_SERVER_IMAGE],
    ["web", environment.WEATHER_WEB_IMAGE],
  ]);

  // inspect each service independently rather than trusting compose state text
  for (const [service, image] of expected) {
    const container = execFileSync("docker", [
      "compose", "--project-name", "weather", "--env-file",
      join(paths.releases, `${environment.WEATHER_RELEASE}.env`), "--file", paths.compose,
      "ps", "--quiet", service,
    ], { encoding: "utf8", maxBuffer: 4_096, stdio: ["ignore", "pipe", "pipe"] }).trim();

    // require one live container identifier and exact configured immutable reference
    if (!/^[a-f0-9]{12,64}$/u.test(container)) {
      throw new Error("family release live container is unavailable");
    }
    const inspected = execFileSync("docker", [
      "inspect", "--format", "{{json .}}", container,
    ], { encoding: "utf8", maxBuffer: 1_048_576, stdio: ["ignore", "pipe", "pipe"] });
    const value = JSON.parse(inspected);

    // require running, healthy application containers on exact image references
    if (value.Config?.Image !== image || value.State?.Running !== true ||
      value.State?.Health?.Status !== "healthy") {
      throw new Error("family release live image or health differs");
    }
  }
}

// read one canonical acknowledged source record
async function readAdjustmentFamilyCurrent(root, optional) {
  const bytes = await readAdjustmentFamilyPrivateFile(
    join(root, "current.json"),
    8 * 1_024,
    optional,
  );
  if (bytes === null) return null;
  const value = parseCanonicalFamilyJson(bytes, "family current acknowledgement");
  return validateAdjustmentFamilyCurrentDocument(value);
}

// validate one exact bootstrap or acknowledged current authority
function validateAdjustmentFamilyCurrentDocument(value) {
  // keep the actionless v14 bridge distinct from family action acknowledgements
  if (value.contractVersion === ADJUSTMENT_FAMILY_BOOTSTRAP_CURRENT_VERSION) {
    requireExactKeys(value, [
      "bootstrappedAt", "catalogSha256", "commit", "contractVersion",
      "imageDigestsSha256", "release", "serverImage", "settingsSha256", "state",
      "webImage",
    ], "family bootstrap current authority");
    if (value.state !== "inert_v14_bootstrap" ||
      !/^[a-f0-9]{40}$/u.test(value.commit) ||
      !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(value.bootstrappedAt) ||
      !ADJUSTMENT_RELEASE_REFERENCE.test(value.serverImage) ||
      !ADJUSTMENT_RELEASE_REFERENCE.test(value.webImage) ||
      value.serverImage.includes("weather-web") || value.webImage.includes("weather-server")) {
      throw new Error("family bootstrap current authority is invalid");
    }
    requireFamilySha256(value.catalogSha256, "family bootstrap catalog SHA256");
    requireFamilySha256(value.imageDigestsSha256, "family bootstrap image SHA256");
    requireFamilySha256(value.settingsSha256, "family bootstrap settings SHA256");
    requireFamilyRelease(value.release, "family bootstrap release");

    // rederive the aggregate rather than trusting a detached bridge hash
    if (adjustmentFamilyImageIdentity({
      WEATHER_SERVER_IMAGE: value.serverImage,
      WEATHER_WEB_IMAGE: value.webImage,
    }) !== value.imageDigestsSha256) {
      throw new Error("family bootstrap current image identity differs");
    }
    return value;
  }
  requireExactKeys(value, [
    "acknowledgedAt", "actionSha256", "commit", "contractVersion", "family",
    "imageDigestsSha256", "release", "state",
  ], "family current acknowledgement");

  // accept only complete acknowledged or verified compensation authorities
  if (value.contractVersion !== ADJUSTMENT_FAMILY_CURRENT_VERSION ||
    !["acknowledged", "compensated"].includes(value.state) ||
    !["temperature", "wind", "rain"].includes(value.family) ||
    !/^[a-f0-9]{40}$/u.test(value.commit) ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(value.acknowledgedAt)) {
    throw new Error("family current acknowledgement is invalid");
  }
  requireFamilySha256(value.actionSha256, "family current action SHA256");
  requireFamilySha256(value.imageDigestsSha256, "family current image SHA256");
  requireFamilyRelease(value.release, "family current release");
  return value;
}

// derive one authorizing root-journal edge or ignore a closed unfinished branch
function adjustmentFamilyLineageEdge(actionSha256, document) {
  requireFamilySha256(actionSha256, "family lineage action SHA256");
  // validate but never authorize ancestry from a no-mutation operator-off branch
  if (document.contractVersion === ADJUSTMENT_FAMILY_OPERATOR_OFF_TRANSACTION_VERSION) {
    requireExactKeys(document, ["contractVersion", "record"],
      "family operator-off lineage transaction");
    validateAdjustmentFamilyOperatorOffRecord(document.record, actionSha256);
    return null;
  }
  requireExactKeys(document, ["compensation", "contractVersion", "proof", "record"],
    "family lineage transaction");
  if (document.contractVersion !== ADJUSTMENT_FAMILY_TRANSACTION_VERSION) {
    throw new Error("family lineage transaction version differs");
  }
  const input = document.record?.input;
  requireExactKeys(input, [
    "actionSha256", "compensatingRelease", "expectedCurrentRelease",
    "expectedSettingsSha256", "expectedSourceRelease", "family", "fencingToken",
    "reportSha256", "targetRelease",
  ], "family lineage transaction input");
  requireExactKeys(document.compensation, [
    "actionSha256", "release", "state", "verifiedAt",
  ], "family lineage compensation");
  requireExactKeys(document.proof, [
    "baselineCatalogBase64", "compensationActionSha256", "compensationCommit",
    "compensationFamilyIdentitySha256", "compensationImageDigestsSha256",
    "sourceSettingsBase64", "targetCatalogBase64", "targetCommit",
    "targetFamilyIdentitySha256", "targetImageDigestsSha256",
  ], "family lineage proof");
  const record = document.record;
  const allowedRecordKeys = new Set([
    "action", "createdAt", "failedAt", "input", "outcome", "sourceCommit",
    "sourceSettingsSha256", "state", "targetCommit", "verifiedAt",
  ]);
  const actionBytes = canonicalFamilyBytes(record?.action);

  // reject alternate record fields before selecting an ancestry edge
  if (record === null || typeof record !== "object" || Array.isArray(record) ||
    Object.keys(record).some((key) => !allowedRecordKeys.has(key)) ||
    record.input.actionSha256 !== actionSha256 ||
    record.action?.expectedSourceCommit !== record.sourceCommit ||
    record.action?.expectedSourceRelease !== input.expectedSourceRelease ||
    record.action?.family !== input.family || input.expectedCurrentRelease !==
      input.expectedSourceRelease || record.sourceSettingsSha256 !==
      input.expectedSettingsSha256 || document.proof.targetCommit !==
      record.targetCommit || document.proof.compensationActionSha256 !==
      document.compensation.actionSha256 || document.compensation.release !==
      input.compensatingRelease) {
    throw new Error("family lineage transaction binding differs");
  }
  validateAdjustmentFamilyAction(actionBytes, {
    actionSha256,
    family: input.family,
    now: record.action.createdAt,
    reportSha256: input.reportSha256,
    sourceCommit: record.sourceCommit,
    sourceRelease: input.expectedSourceRelease,
  });
  for (const commit of [record.sourceCommit, record.targetCommit,
    document.proof.compensationCommit]) {
    // require only immutable Git commit identities in the retained chain
    if (typeof commit !== "string" || !/^[a-f0-9]{40}$/u.test(commit)) {
      throw new Error("family lineage commit is invalid");
    }
  }
  for (const release of [input.expectedSourceRelease, input.targetRelease,
    input.compensatingRelease]) {
    requireFamilyRelease(release, "family lineage release");
  }

  // ignore a closed branch that never established a new current authority
  if (["prepared", "applying"].includes(record.state)) {
    if (document.compensation.state !== "absent" ||
      document.compensation.verifiedAt !== null || record.failedAt !== undefined ||
      record.outcome !== undefined || record.verifiedAt !== undefined) {
      throw new Error("family unfinished lineage branch is invalid");
    }
    return null;
  }

  // select only a completed target acknowledgement
  if (record.state === "acknowledged") {
    if (document.compensation.state !== "absent" ||
      typeof record.verifiedAt !== "string" ||
      !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(record.verifiedAt)) {
      throw new Error("family acknowledged lineage is incomplete");
    }
    return Object.freeze({
      actionSha256,
      childCommit: record.targetCommit,
      childRelease: input.targetRelease,
      parentCommit: record.sourceCommit,
      parentRelease: input.expectedSourceRelease,
    });
  }

  // ignore a failed deployment until its compensation establishes authority
  if (record.state === "compensation_required" &&
    ["absent", "failed"].includes(document.compensation.state)) {
    if (typeof record.failedAt !== "string" ||
      !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(record.failedAt) ||
      document.compensation.verifiedAt !== null || record.outcome !== undefined ||
      record.verifiedAt !== undefined) {
      throw new Error("family failed lineage branch is invalid");
    }
    return null;
  }

  // select only a verified family-only compensation acknowledgement
  if (!["applying", "compensation_required"].includes(record.state) ||
    document.compensation.state !== "verified" ||
    typeof document.compensation.verifiedAt !== "string" ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(
      document.compensation.verifiedAt,
    )) {
    throw new Error("family compensated lineage is incomplete");
  }
  return Object.freeze({
    actionSha256: document.compensation.actionSha256,
    childCommit: document.proof.compensationCommit,
    childRelease: input.compensatingRelease,
    parentCommit: record.sourceCommit,
    parentRelease: input.expectedSourceRelease,
  });
}

// validate and project one bounded transitive epoch ancestry proof
export function projectAdjustmentFamilyReleaseLineage(input) {
  requireExactKeys(input, [
    "controlSha256", "controlVersion", "current", "epochWitness",
    "transactions", "verifiedAt",
  ], "family release lineage input");
  const witness = validateAdjustmentRevisionCaptureEpochWitness(input.epochWitness);
  const current = validateAdjustmentFamilyCurrentDocument(input.current);
  requireFamilySha256(input.controlSha256, "family lineage control SHA256");
  if (input.controlVersion !== "14" || input.controlVersion !==
      witness.controlPlaneVersion || input.controlSha256 !==
      witness.controlPlaneSha256 || typeof input.verifiedAt !== "string" ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(input.verifiedAt) ||
    !Array.isArray(input.transactions) || input.transactions.length >
      ADJUSTMENT_FAMILY_LINEAGE_MAXIMUM_TRANSACTIONS) {
    throw new Error("family release lineage control differs");
  }
  const edges = input.transactions.map((entry) => {
    requireExactKeys(entry, ["actionSha256", "document"],
      "family lineage transaction entry");
    return adjustmentFamilyLineageEdge(entry.actionSha256, entry.document);
  }).filter((edge) => edge !== null);
  let commit = current.commit;
  let release = current.release;
  let currentActionSha256 = null;

  // distinguish the original inert authority from every family action child
  if (current.contractVersion === ADJUSTMENT_FAMILY_BOOTSTRAP_CURRENT_VERSION) {
    if (current.commit !== witness.sourceCommit || current.release !==
      witness.sourceRelease || current.serverImage.split("@")[1] !==
      witness.sourceServerImageDigest || current.webImage.split("@")[1] !==
      witness.sourceWebImageDigest) {
      throw new Error("family lineage bootstrap differs from capture epoch");
    }
  } else {
    currentActionSha256 = current.actionSha256;
    const visited = new Set();
    let depth = 0;

    // walk every acknowledged edge back to the immutable epoch source
    while (commit !== witness.sourceCommit) {
      if (visited.has(commit) || depth >= ADJUSTMENT_FAMILY_LINEAGE_MAXIMUM_TRANSACTIONS) {
        throw new Error("family release lineage contains a cycle");
      }
      visited.add(commit);
      const matches = edges.filter(
        // match both commit and release to prevent cross-release ancestry aliases
        (edge) => edge.childCommit === commit && edge.childRelease === release,
      );

      // require one predecessor and the current root action on the first edge
      if (matches.length !== 1 || (depth === 0 &&
        matches[0].actionSha256 !== current.actionSha256)) {
        throw new Error("family release lineage predecessor is ambiguous");
      }
      commit = matches[0].parentCommit;
      release = matches[0].parentRelease;
      depth += 1;
    }
    if (depth === 0 || release !== witness.sourceRelease) {
      throw new Error("family release lineage epoch ancestor differs");
    }
  }
  return Object.freeze({
    contractVersion: ADJUSTMENT_FAMILY_RELEASE_LINEAGE_VERSION,
    controlSha256: input.controlSha256,
    controlVersion: input.controlVersion,
    currentActionSha256,
    currentCommit: current.commit,
    currentRelease: current.release,
    epochAncestorCommit: witness.sourceCommit,
    epochWitnessSha256: witness.witnessSha256,
    state: "verified_epoch_descendant",
    verifiedAt: input.verifiedAt,
  });
}

// require one existing owner-private lineage directory without creating it
async function requireAdjustmentFamilyLineageDirectory(path) {
  const details = await lstat(path);
  const canonical = await realpath(path);

  // reject links, foreign ownership and permissive journal roots
  if (!details.isDirectory() || details.isSymbolicLink() ||
    details.uid !== process.getuid() || details.gid !== process.getgid() ||
    (details.mode & 0o777) !== 0o700 || canonical !== path) {
    throw new Error("family lineage transaction root is unsafe");
  }
}

// read the complete bounded root-owned family transaction journal
async function readAdjustmentFamilyLineageTransactions(root) {
  const transactionsRoot = join(root, "transactions");
  await requireAdjustmentFamilyLineageDirectory(root);
  await requireAdjustmentFamilyLineageDirectory(transactionsRoot);
  const names = readdirSync(transactionsRoot).sort();

  // refuse unbounded, foreign or multiply named transaction members
  if (names.length > ADJUSTMENT_FAMILY_LINEAGE_MAXIMUM_TRANSACTIONS || names.some(
    (name) => !/^sha256-[a-f0-9]{64}\.json$/u.test(name),
  )) {
    throw new Error("family lineage transaction root contains an unknown entry");
  }
  const entries = [];

  // retain every canonical transaction beside its filename identity
  for (const name of names) {
    const bytes = await readAdjustmentFamilyPrivateFile(
      join(transactionsRoot, name),
      ADJUSTMENT_FAMILY_MAXIMUM_STATE_BYTES,
      false,
    );
    entries.push({
      actionSha256: name.slice(7, -5),
      document: parseCanonicalFamilyJson(bytes, "family lineage transaction"),
    });
  }
  return entries;
}

// read one frozen root-owned lineage observation for proof or ancestor checks
async function readAdjustmentFamilyReleaseLineageInput(options = {}) {
  requireExactKeys(options, Object.keys(options).length === 0 ? [] : ["host"],
    "family release lineage options");
  let host = options.host;

  // construct only fixed production readers without a mutation seam
  if (host === undefined) {
    const paths = {
      catalog: ADJUSTMENT_FAMILY_CATALOG_PATH,
      compose: resolve(dirname(import.meta.filename), "../compose.yaml"),
      currentRelease: "/opt/weather/current/deploy/state/current-release",
      evidenceRoot: "/var/lib/weather/xweather/adjustment-evidence",
      releases: "/opt/weather/current/deploy/releases",
      root: ADJUSTMENT_FAMILY_RELEASE_ROOT,
      settings: ADJUSTMENT_FAMILY_SETTINGS_PATH,
      update: resolve(dirname(import.meta.filename), "update.sh"),
    };
    const liveHost = createProductionAdjustmentFamilyHost({
      clock: () => new Date().toISOString(),
      family: "temperature",
      fetchImpl: fetch,
      paths,
    });
    host = {
      clock: () => new Date().toISOString(),
      inspectLive: async () => await liveHost.inspectCurrent(),
      readCurrent: async () => await readAdjustmentFamilyPrivateFile(
        join(paths.root, "current.json"),
        8 * 1_024,
        false,
      ),
      readTransactions: async () => await readAdjustmentFamilyLineageTransactions(paths.root),
      readWitness: async () => await readAdjustmentRevisionCaptureEpochWitness(),
    };
  }
  const required = ["clock", "inspectLive", "readCurrent", "readTransactions", "readWitness"];

  // close the injectable test surface before observing any authority
  if (host === null || typeof host !== "object" || required.some(
    (name) => typeof host[name] !== "function",
  )) {
    throw new TypeError("family release lineage host is invalid");
  }
  const beforeBytes = await host.readCurrent();
  const current = validateAdjustmentFamilyCurrentDocument(
    parseCanonicalFamilyJson(beforeBytes, "family lineage current authority"),
  );
  const witness = validateAdjustmentRevisionCaptureEpochWitness(await host.readWitness());
  const beforeLive = await host.inspectLive();
  const transactions = await host.readTransactions();
  const afterLive = await host.inspectLive();
  const afterBytes = await host.readCurrent();

  // recheck the current inode bytes and live release around the bounded journal walk
  if (!Buffer.isBuffer(beforeBytes) || !Buffer.isBuffer(afterBytes) ||
    !beforeBytes.equals(afterBytes) || beforeLive?.commit !== current.commit ||
    beforeLive?.release !== current.release || afterLive?.commit !== current.commit ||
    afterLive?.release !== current.release || current.imageDigestsSha256 !==
      adjustmentFamilyImageIdentity(beforeLive.environment ?? {}) ||
    current.imageDigestsSha256 !== adjustmentFamilyImageIdentity(
      afterLive.environment ?? {},
    ) || beforeLive.environment?.WEATHER_CONTROL_PLANE_VERSION !==
      afterLive.environment?.WEATHER_CONTROL_PLANE_VERSION ||
    beforeLive.environment?.WEATHER_CONTROL_PLANE_SHA256 !==
      afterLive.environment?.WEATHER_CONTROL_PLANE_SHA256) {
    throw new Error("family release current authority changed during lineage read");
  }
  const controlVersion = afterLive.environment?.WEATHER_CONTROL_PLANE_VERSION;
  const controlSha256 = afterLive.environment?.WEATHER_CONTROL_PLANE_SHA256;
  return Object.freeze({
    controlSha256,
    controlVersion,
    current,
    epochWitness: witness,
    transactions,
    verifiedAt: host.clock(),
  });
}

// read one transitive root journal proof for capture restart admission
export async function readAdjustmentFamilyReleaseLineage(options = {}) {
  return projectAdjustmentFamilyReleaseLineage(
    await readAdjustmentFamilyReleaseLineageInput(options),
  );
}

// verify one commit occurs on the exact retained terminal epoch ancestry
export function verifyAdjustmentFamilyReleaseAncestorProjection(input, sourceCommit) {
  // require one immutable commit operand before reading the closed journal
  if (typeof sourceCommit !== "string" || !/^[a-f0-9]{40}$/u.test(sourceCommit)) {
    throw new TypeError("family release ancestor commit is invalid");
  }
  const lineage = projectAdjustmentFamilyReleaseLineage(input);
  // retain only genuine terminal authority edges from the closed transactions
  const edges = input.transactions.map((entry) =>
    adjustmentFamilyLineageEdge(entry.actionSha256, entry.document))
    .filter((edge) => edge !== null);
  let commit = lineage.currentCommit;
  let release = lineage.currentRelease;
  let matched = commit === sourceCommit;
  let depth = 0;
  const visited = new Set();

  // walk only the already-validated unique terminal path back to the epoch
  while (commit !== lineage.epochAncestorCommit) {
    // preserve the existing finite path and cycle bound
    if (visited.has(commit) || depth >= ADJUSTMENT_FAMILY_LINEAGE_MAXIMUM_TRANSACTIONS) {
      throw new Error("family release ancestor path contains a cycle");
    }
    visited.add(commit);
    const matches = edges.filter(
      // preserve the same commit and release pair used by the lineage proof
      (edge) => edge.childCommit === commit && edge.childRelease === release,
    );
    // require the same unique predecessor already admitted by the lineage proof
    if (matches.length !== 1) {
      throw new Error("family release ancestor predecessor is ambiguous");
    }
    commit = matches[0].parentCommit;
    release = matches[0].parentRelease;
    matched ||= commit === sourceCommit;
    depth += 1;
  }
  return matched;
}

// verify one commit against a frozen root-owned lineage observation
export async function verifyAdjustmentFamilyReleaseAncestor(sourceCommit, options = {}) {
  return verifyAdjustmentFamilyReleaseAncestorProjection(
    await readAdjustmentFamilyReleaseLineageInput(options),
    sourceCommit,
  );
}

// validate the root-installed catalog envelope and unique family slots
function validateInstalledAdjustmentCatalog(bytes) {
  if (!Buffer.isBuffer(bytes) || bytes.length > ADJUSTMENT_FAMILY_MAXIMUM_CATALOG_BYTES) {
    throw new TypeError("installed adjustment catalog bytes are invalid");
  }
  const value = parseCanonicalFamilyJson(bytes, "installed adjustment catalog");
  requireExactKeys(value, ["contractVersion", "entries"], "installed adjustment catalog");

  const v1 = value.contractVersion === ADJUSTMENT_FAMILY_CATALOG_VERSION;
  const v2 = value.contractVersion === ADJUSTMENT_FAMILY_CATALOG_V2_VERSION;
  const v3 = value.contractVersion === ADJUSTMENT_FAMILY_CATALOG_V3_VERSION;

  // retain legacy shadows and close active, shadow and rain-control slots
  if ((!v1 && !v2 && !v3) || !Array.isArray(value.entries) ||
    value.entries.length > (v1 ? 3 : v2 ? 6 : 7) ||
    value.entries.some((entry) => entry === null || typeof entry !== "object" ||
      Array.isArray(entry) || !["temperature", "wind", "rain"].includes(entry.family) ||
      ((v2 || v3) && !["active", "shadow", ...(v3 ? ["control"] : [])]
        .includes(entry.slot)) ||
      (entry.slot === "control" && (entry.family !== "rain" || entry.registration !== null)) ||
      Object.keys(entry).sort().join("\n") !== (v1
        ? ["family", "receipt", "registration"]
        : ["family", "receipt", "registration", "slot"]).sort().join("\n")) ||
    new Set(value.entries.map((entry) => v1 ? entry.family : `${entry.family}/${entry.slot}`))
      .size !== value.entries.length) {
    throw new Error("installed adjustment catalog is invalid");
  }
  const control = value.entries.find((entry) => entry.slot === "control");

  // validate the sole additive control receipt without conferring model authority
  if (control !== undefined) {
    validateInstalledRainControlReferenceReceipt(control.receipt);
  }
  return value;
}

// validate one custody-only installed rain control receipt
function validateInstalledRainControlReferenceReceipt(value) {
  requireExactKeys(value, ["actionKind", "actionSha256", "contractVersion",
    "controlPlaneSha256", "controlStateSha256", "custodyAnchorSha256", "deployedCommit",
    "deployedImageDigest", "deployedRelease", "deployedSettingsSha256", "dueMonth",
    "fencingToken", "graphManifestSha256", "installedAt", "ordinalArtifactSha256",
    "sourceMemberRootSha256", "sourceReceiptRootSha256"],
  "installed rain control receipt");
  for (const key of ["actionSha256", "controlPlaneSha256", "controlStateSha256",
    "custodyAnchorSha256", "deployedSettingsSha256", "graphManifestSha256",
    "ordinalArtifactSha256", "sourceMemberRootSha256", "sourceReceiptRootSha256"]) {
    requireFamilySha256(value[key], `installed rain control ${key}`);
  }
  if (value.contractVersion !== "adjustment-installed-rain-control-reference-receipt/v1" ||
    value.actionKind !== "control_reference" ||
    !/^\d{4}-(?:0[1-9]|1[0-2])$/u.test(value.dueMonth) ||
    !/^[a-f0-9]{40}$/u.test(value.deployedCommit) ||
    !/^sha256:[a-f0-9]{64}$/u.test(value.deployedImageDigest) ||
    !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d*$/u.test(value.deployedRelease) ||
    !/^[1-9]\d{0,19}$/u.test(value.fencingToken) ||
    BigInt(value.fencingToken) > 18_446_744_073_709_551_615n ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(value.installedAt) ||
    new Date(value.installedAt).toISOString() !== value.installedAt) {
    throw new Error("installed rain control receipt differs");
  }
  return value;
}

// hash the exact installed receipt for one family slot or return explicit absence
function installedFamilyReceiptSha256(catalog, family, slot = "shadow") {
  // status-only callers never inspect a family receipt
  if (!["temperature", "wind", "rain"].includes(family)) {
    throw new TypeError("installed adjustment family is invalid");
  }
  if (!["active", "shadow", "control"].includes(slot) || slot === "control" && family !== "rain") {
    throw new TypeError("installed adjustment slot is invalid");
  }
  const entry = catalog.entries.find((candidate) => candidate.family === family &&
    (catalog.contractVersion === ADJUSTMENT_FAMILY_CATALOG_VERSION
      ? slot === "shadow"
      : candidate.slot === slot));
  if (entry === undefined) return null;
  requireExactKeys(entry, catalog.contractVersion === ADJUSTMENT_FAMILY_CATALOG_VERSION
    ? ["family", "receipt", "registration"]
    : ["family", "receipt", "registration", "slot"], "installed adjustment entry");
  return createHash("sha256").update(canonicalFamilyBytes(entry.receipt)).digest("hex");
}

// fetch bounded public repository metadata through an injected transport
async function readPublicWeatherGitJsonWithFetch(suffix, fetchImpl) {
  const url = new URL(`/repos/${WEATHER_GITHUB_REPOSITORY.fullName}/${suffix}`,
    "https://api.github.com");
  const response = await fetchImpl(url, { redirect: "error", signal: AbortSignal.timeout(15_000),
    headers: { Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2026-03-10" } });

  // treat redirects, throttling and oversized metadata as blockers
  if (!response.ok) throw new Error(`Weather Git lookup failed with HTTP ${response.status}`);
  return JSON.parse(await readBoundedRegistryResponse(response, 4_194_304, "Weather Git metadata"));
}

// read one immutable repository file and bind its public action hash
async function readPublicWeatherRepositoryFile(sourceCommit, release, path, readJson) {
  if (!/^[a-f0-9]{40}$/u.test(sourceCommit)) {
    throw new TypeError("Weather repository source commit is invalid");
  }
  requireFamilyRelease(release, "Weather repository release");
  const encoded = path.split("/").map(encodeURIComponent).join("/");
  const value = await readJson(`contents/${encoded}?ref=${encodeURIComponent(release)}`);

  // accept only one bounded base64 file at the requested immutable path
  if (value?.type !== "file" || value.path !== path || value.encoding !== "base64" ||
    !Number.isSafeInteger(value.size) || value.size < 2 || value.size > 32 * 1_024 ||
    typeof value.content !== "string") {
    throw new Error("Weather repository action file is invalid");
  }
  const bytes = Buffer.from(value.content.replaceAll("\n", ""), "base64");
  const actionSha256 = /sha256-([a-f0-9]{64})\.json$/u.exec(path)?.[1];

  // bind canonical bytes to their public content-addressed filename
  if (bytes.length !== value.size ||
    createHash("sha256").update(bytes).digest("hex") !== actionSha256) {
    throw new Error("Weather repository action identity differs");
  }
  return bytes;
}

// read one bounded file from a CI-verified immutable server image
function readAdjustmentImageFile(image, path, maximumBytes) {
  if (!ADJUSTMENT_RELEASE_REFERENCE.test(image) || typeof path !== "string" ||
    path.startsWith("/") || path.includes("..")) {
    throw new TypeError("family image file request is invalid");
  }
  const output = execFileSync("docker", [
    "run", "--rm", "--network", "none", "--read-only", "--pids-limit", "32",
    "--memory", "128m", "--entrypoint", "/bin/cat", image, `/opt/weather/${path}`,
  ], { encoding: null, maxBuffer: maximumBytes + 1, stdio: ["ignore", "pipe", "pipe"] });

  // reject empty or oversized image members after bounded collection
  if (output.length < 2 || output.length > maximumBytes) {
    throw new Error("family image file exceeds its bound");
  }
  return Buffer.from(output);
}

// read the pre-pull six-image capacity receipt from the fixed transaction root
async function readAdjustmentFamilyCapacity(root, actionSha256) {
  const bytes = await readAdjustmentFamilyPrivateFile(
    join(root, `capacity-sha256-${actionSha256}.json`),
    64 * 1_024,
    false,
  );
  const value = parseCanonicalFamilyJson(bytes, "family release capacity");

  // leave the complete semantic validation to the executor after canonical parsing
  if (value.actionSha256 !== actionSha256) {
    throw new Error("family release capacity action differs");
  }
  return value;
}

// authorize one action through the installed anchor store's exact current slot
export async function authorizeAdjustmentFamilyAction(action, actionSha256, evidenceRoot) {
  // pre-month control custody is disjoint from development and qualified model anchors
  if (action.contractVersion === RAIN_CONTROL_REFERENCE_ACTION_CONTRACT_VERSION) {
    const { AdjustmentRainControlCustodyAnchorStore } =
      await import("./adjustment-evidence-store.mjs");
    const store = new AdjustmentRainControlCustodyAnchorStore({ root: evidenceRoot });
    return await store.authorizeRainControlReferenceAction({
      actionKind: action.actionKind,
      actionSha256,
      controlStateSha256: action.controlStateSha256,
      dueMonth: action.dueMonth,
      fencingToken: action.fencingToken,
      graphManifestSha256: action.graphManifestSha256,
      ordinalArtifactSha256: action.ordinalArtifactSha256,
      sourceCommit: action.expectedSourceCommit,
      sourceMemberRootSha256: action.sourceMemberRootSha256,
      sourceReceiptRootSha256: action.sourceReceiptRootSha256,
    });
  }
  // development authority is distinct from both legacy and future terminal C/T/F
  if (action.actionKind === "shadow") {
    const bytes = await readHostFile(join(evidenceRoot,
      "development-custody-anchors/current.json"), 16 * 1_024, 0o600, [0]);
    const current = parseCanonicalFamilyJson(bytes, "development custody action anchor");
    const { AdjustmentDevelopmentCustodyAnchorStore } = await import("./adjustment-evidence-store.mjs");
    const store = new AdjustmentDevelopmentCustodyAnchorStore({ root: evidenceRoot });
    return await store.authorizeDevelopmentShadowAction({
      actionKind: action.actionKind,
      actionSha256,
      artifactSha256: current.artifactSha256,
      candidateGraphSha256: action.candidateGraphSha256,
      candidateSha256: action.candidateSha256,
      family: action.family,
      lifecycleLedgerRootSha256: action.lifecycleHeadSha256,
      policyReportSha256: action.policyReportSha256,
      registrationSha256: current.registrationSha256,
      sourceCommit: action.expectedSourceCommit,
    });
  }
  const anchorPath = join(evidenceRoot, "maintenance-anchors/current.json");
  const bytes = await readHostFile(anchorPath, 16 * 1_024, 0o600, [0]);
  const current = parseCanonicalFamilyJson(bytes, "maintenance action anchor");
  const { AdjustmentMaintenanceAnchorStore } = await import("./adjustment-evidence-store.mjs");
  const store = new AdjustmentMaintenanceAnchorStore({ root: evidenceRoot });

  // future-only qualification binds the additive F anchor without legacy aliases
  if (current.contractVersion === "adjustment-maintenance-anchor/v3") {
    return await store.authorizeFutureOnlyQualifiedAction({
      actionSha256,
      fullMemberRootSha256: action.fullMemberRootSha256,
      lifecycleLedgerRootSha256: action.lifecycleHeadSha256,
      policyReportSha256: action.policyReportSha256,
      sourceCommit: action.expectedSourceCommit,
    });
  }

  // development shadow actions require transferred graph authority
  if (["shadow", "compensate_shadow"].includes(action.actionKind)) {
    return await store.authorizeShadowCandidate({
      catalogRootSha256: current.catalogRootSha256,
      controlSha256: current.controlSha256,
      controlVersion: current.controlVersion,
      graphManifestSha256: current.graphManifestSha256,
      sourceCommit: action.expectedSourceCommit,
    });
  }
  return await store.authorizeQualifiedAction({
    actionSha256,
    burnSha256: current.burnSha256,
    controlSha256: current.controlSha256,
    controlVersion: current.controlVersion,
    fullMemberRootSha256: action.fullMemberRootSha256,
    lifecycleLedgerRootSha256: action.lifecycleHeadSha256,
    reportSha256: action.policyReportSha256,
    revisionSnapshotRootSha256: current.revisionSnapshotRootSha256,
    sourceCommit: action.expectedSourceCommit,
    workstationJournalHeadSha256: current.workstationJournalHeadSha256,
  });
}

// crossbind one custody-only control action to its installed v3 catalog receipt
export function validateAdjustmentRainControlReferenceBinding(
  action,
  actionSha256,
  authority,
  catalog,
) {
  const entry = catalog.entries.find((item) => item.family === "rain" && item.slot === "control");
  const receipt = entry?.receipt;

  // require action, custody and target catalog to name the same immutable control material
  if (authority.contractVersion !== "adjustment-rain-control-custody-anchor/v1" ||
    authority.actionKind !== "control_reference" || action.actionKind !== "control_reference" ||
    authority.actionSha256 !== actionSha256 || authority.sourceCommit !== action.expectedSourceCommit ||
    authority.fencingToken !== action.fencingToken || authority.dueMonth !== action.dueMonth ||
    authority.graphManifestSha256 !== action.graphManifestSha256 ||
    authority.controlStateSha256 !== action.controlStateSha256 ||
    authority.ordinalArtifactSha256 !== action.ordinalArtifactSha256 ||
    authority.sourceMemberRootSha256 !== action.sourceMemberRootSha256 ||
    authority.sourceReceiptRootSha256 !== action.sourceReceiptRootSha256 || entry === undefined ||
    entry.registration !== null || receipt.actionSha256 !== actionSha256 ||
    receipt.custodyAnchorSha256 !== createHash("sha256")
      .update(canonicalFamilyBytes(authority)).digest("hex") ||
    receipt.controlStateSha256 !== action.controlStateSha256 ||
    receipt.ordinalArtifactSha256 !== action.ordinalArtifactSha256 ||
    receipt.graphManifestSha256 !== action.graphManifestSha256 ||
    receipt.sourceMemberRootSha256 !== action.sourceMemberRootSha256 ||
    receipt.sourceReceiptRootSha256 !== action.sourceReceiptRootSha256) {
    throw new Error("rain control custody target image binding differs");
  }
  return true;
}

// crossbind development custody to actual target image material before mutation
export function validateAdjustmentDevelopmentActionBinding(action, actionSha256, authority, catalog, witnessValue) {
  const witness = validateAdjustmentRevisionCaptureEpochWitness(witnessValue);
  const witnessDocumentSha256 = createHash("sha256")
    .update(canonicalFamilyBytes(witness)).digest("hex");
  const entry = catalog.entries.find((item) => item.family === action.family && item.slot === "shadow");
  // never let pending development custody masquerade as completed confirmation
  if (authority.contractVersion !== "adjustment-development-custody-anchor/v1" ||
    authority.actionKind !== "shadow" || action.actionKind !== "shadow" ||
    action.policyDecision !== "pending" || action.fullMemberRootSha256 !== null ||
    authority.actionSha256 !== actionSha256 || authority.family !== action.family ||
    authority.candidateGraphSha256 !== action.candidateGraphSha256 ||
    authority.candidateSha256 !== action.candidateSha256 ||
    authority.policyReportSha256 !== action.policyReportSha256 ||
    authority.lifecycleLedgerRootSha256 !== action.lifecycleHeadSha256 ||
    authority.sourceCommit !== action.expectedSourceCommit || entry === undefined ||
    entry.receipt.actionSha256 !== actionSha256 ||
    entry.registration.artifactSha256 !== authority.artifactSha256 ||
    entry.receipt.bundleSha256 !== authority.artifactSha256 ||
    entry.registration.registrationSha256 !== authority.registrationSha256 ||
    entry.registration.sourceSha256 !== authority.sourceSha256 ||
    authority.captureEpochWitnessSha256 !== witnessDocumentSha256 ||
    entry.registration.epochWitnessSha256 !== witness.witnessSha256) {
    throw new Error("development custody target image binding differs");
  }
  return true;
}

// derive the target root-installed catalog from one verified action image
export async function buildInstalledAdjustmentTargetCatalog(input) {
  const sourceEntries = input.source.entries.map((entry) => input.source.contractVersion ===
    ADJUSTMENT_FAMILY_CATALOG_VERSION ? { ...entry, slot: "shadow" } : entry);
  const entries = sourceEntries.filter((entry) => entry.family !== input.family ||
    entry.slot === "control");
  const active = sourceEntries.find((entry) =>
    entry.family === input.family && entry.slot === "active");
  const shadow = sourceEntries.find((entry) =>
    entry.family === input.family && entry.slot === "shadow");

  // preserve the serving authority while installing a new inactive candidate
  if (input.action.actionKind === "shadow" && active !== undefined) {
    entries.push(active);
  }

  // raw fallback removes only active authority and retains any pending shadow
  if (input.action.actionKind === "raw" && shadow !== undefined) {
    entries.push(shadow);
  }

  // an inactive shadow installs a distinct pending slot
  if (input.action.actionKind === "shadow") {
    const candidateSha256 = requireFamilySha256(
      input.action.candidateSha256,
      "shadow candidate SHA256",
    );
    const projectionPath = `config/forecast-adjustments/ballydidean/shadow-catalog/${input.family}/sha256-${candidateSha256}.json`;
    const projectionBytes = input.imageRead(projectionPath, 128 * 1_024);
    const projection = parseCanonicalFamilyJson(projectionBytes, "shadow catalog projection");
    requireExactKeys(projection, [
      "actionSha256", "artifactSha256", "bundleSha256", "candidateGraphSha256",
      "candidateSha256", "contractVersion", "family", "paritySha256", "registration",
      "siteKey",
    ], "shadow catalog projection");

    // bind the public projection to this exact action before receipt creation
    if (projection.contractVersion !== "forecast-adjustment-shadow-catalog-projection/v1" ||
      projection.actionSha256 !== input.actionSha256 || projection.family !== input.family ||
      projection.candidateSha256 !== candidateSha256 ||
      projection.candidateGraphSha256 !== input.action.candidateGraphSha256 ||
      projection.siteKey !== "ballydidean") {
      throw new Error("shadow catalog projection differs");
    }
    const receipt = {
      actionKind: "shadow",
      actionSha256: input.actionSha256,
      bundleSha256: projection.bundleSha256,
      candidateGraphSha256: projection.candidateGraphSha256,
      candidateSha256,
      contractVersion: "adjustment-installed-candidate-receipt/v2",
      controlPlaneSha256: input.environment.WEATHER_CONTROL_PLANE_SHA256,
      deployedCommit: input.imageCommit,
      deployedImageDigest: input.environment.WEATHER_SERVER_IMAGE.split("@")[1],
      deployedRelease: input.environment.WEATHER_RELEASE,
      deployedSettingsSha256: createHash("sha256").update(input.settingsBytes).digest("hex"),
      fencingToken: input.action.fencingToken,
      fullMemberRootSha256: null,
      installedAt: input.clock(),
      lifecycleHeadSha256: input.action.lifecycleHeadSha256,
      policyDecision: input.action.policyDecision,
      policyReportSha256: input.action.policyReportSha256,
      registrationSha256: projection.registration?.registrationSha256,
      sourceSha256: projection.registration?.sourceSha256,
    };
    entries.push({ family: input.family, receipt, registration: projection.registration,
      slot: "shadow" });
  }

  // promotion consumes the exact pending slot and installs qualified active authority
  if (input.action.actionKind === "promote") {
    if (shadow === undefined) {
      throw new Error("installed shadow candidate is unavailable");
    }
    requireExactKeys(shadow, ["family", "receipt", "registration", "slot"],
      "installed shadow entry");
    const sourceReceipt = shadow.receipt;

    // require the pending candidate and artifact selected by this qualification
    if (sourceReceipt.contractVersion !== "adjustment-installed-candidate-receipt/v2" ||
      sourceReceipt.actionKind !== "shadow" || sourceReceipt.policyDecision !== "pending" ||
      sourceReceipt.fullMemberRootSha256 !== null ||
      sourceReceipt.candidateSha256 !== input.action.candidateSha256 ||
      sourceReceipt.candidateGraphSha256 !== input.action.candidateGraphSha256 ||
      shadow.registration?.candidateSha256 !== input.action.candidateSha256 ||
      shadow.registration?.artifactSha256 !== sourceReceipt.bundleSha256) {
      throw new Error("installed shadow candidate differs");
    }
    const receipt = {
      actionKind: "promote",
      actionSha256: input.actionSha256,
      bundleSha256: sourceReceipt.bundleSha256,
      candidateGraphSha256: input.action.candidateGraphSha256,
      candidateSha256: input.action.candidateSha256,
      contractVersion: "adjustment-installed-candidate-receipt/v2",
      controlPlaneSha256: input.environment.WEATHER_CONTROL_PLANE_SHA256,
      deployedCommit: input.imageCommit,
      deployedImageDigest: input.environment.WEATHER_SERVER_IMAGE.split("@")[1],
      deployedRelease: input.environment.WEATHER_RELEASE,
      deployedSettingsSha256: createHash("sha256").update(input.settingsBytes).digest("hex"),
      fencingToken: input.action.fencingToken,
      fullMemberRootSha256: input.action.fullMemberRootSha256,
      installedAt: input.clock(),
      lifecycleHeadSha256: input.action.lifecycleHeadSha256,
      policyDecision: input.action.policyDecision,
      policyReportSha256: input.action.policyReportSha256,
      registrationSha256: shadow.registration.registrationSha256,
      sourceSha256: shadow.registration.sourceSha256,
    };
    entries.push({ family: input.family, receipt, registration: shadow.registration,
      slot: "active" });
  }

  // prior rollback needs a retained separately qualified receipt not present in this catalog
  if (input.action.actionKind === "rollback_prior") {
    throw new Error("prior qualified installed receipt is unavailable");
  }
  entries.sort((left, right) => left.family.localeCompare(right.family) ||
    left.slot.localeCompare(right.slot));
  const catalogVersion = input.source.contractVersion === ADJUSTMENT_FAMILY_CATALOG_V3_VERSION ||
    entries.some((entry) => entry.slot === "control")
    ? ADJUSTMENT_FAMILY_CATALOG_V3_VERSION
    : ADJUSTMENT_FAMILY_CATALOG_V2_VERSION;
  return validateInstalledAdjustmentCatalog(canonicalFamilyBytes({
    contractVersion: catalogVersion,
    entries,
  }));
}

// derive one rain control catalog without changing active or shadow model authority
export function buildInstalledAdjustmentRainControlReferenceCatalog(input) {
  requireExactKeys(input, ["action", "actionSha256", "clock", "custodyAnchorSha256",
    "environment", "imageCommit", "imageRead", "settingsBytes", "source"],
  "installed rain control catalog input");
  const action = input.action;
  if (action.contractVersion !== RAIN_CONTROL_REFERENCE_ACTION_CONTRACT_VERSION ||
    action.actionKind !== "control_reference" || action.family !== "rain" ||
    typeof input.clock !== "function" || typeof input.imageRead !== "function") {
    throw new Error("installed rain control catalog action differs");
  }
  requireFamilySha256(input.actionSha256, "installed rain control action");
  requireFamilySha256(input.custodyAnchorSha256, "installed rain control custody anchor");
  const source = validateInstalledAdjustmentCatalog(canonicalFamilyBytes(input.source));
  const previousReceiptSha256 = installedFamilyReceiptSha256(source, "rain", "control");

  // require the action to compare-and-swap the exact prepared control slot baseline
  if (previousReceiptSha256 !== action.expectedCatalogReceiptSha256 ||
    action.expectedSettingsSha256 !==
      createHash("sha256").update(input.settingsBytes).digest("hex")) {
    throw new Error("installed rain control catalog baseline differs");
  }
  const selectorPath = "config/forecast-adjustments/ballydidean-rain-control-reference.json";
  const selectorBytes = input.imageRead(selectorPath, 16 * 1_024);
  const selector = parseCanonicalFamilyJson(selectorBytes, "rain control selector");
  requireExactKeys(selector, ["actionSha256", "contractVersion", "controlStatePath",
    "controlStateSha256", "dueMonth", "ordinalArtifactPath", "ordinalArtifactSha256",
    "siteKey"], "rain control selector");
  const statePath = `rain-maintenance-control-states/sha256-${action.controlStateSha256}.json`;
  const artifactPath = `rain-runtime-artifacts/sha256-${action.ordinalArtifactSha256}.json`;

  // bind target-image selector and content-addressed bytes before installing receipt authority
  if (selector.actionSha256 !== input.actionSha256 ||
    selector.contractVersion !== "forecast-adjustment-rain-control-reference-registry/v1" ||
    selector.siteKey !== "ballydidean" || selector.dueMonth !== action.dueMonth ||
    selector.controlStateSha256 !== action.controlStateSha256 ||
    selector.ordinalArtifactSha256 !== action.ordinalArtifactSha256 ||
    selector.controlStatePath !== statePath || selector.ordinalArtifactPath !== artifactPath ||
    createHash("sha256").update(input.imageRead(
      `config/forecast-adjustments/ballydidean/${statePath}`,
      64 * 1_024,
    )).digest("hex") !== action.controlStateSha256 ||
    createHash("sha256").update(input.imageRead(
      `config/forecast-adjustments/ballydidean/${artifactPath}`,
      ADJUSTMENT_RUNTIME_PACKAGE_MAX_BYTES,
    )).digest("hex") !== action.ordinalArtifactSha256) {
    throw new Error("installed rain control selector differs");
  }
  const receipt = {
    actionKind: "control_reference",
    actionSha256: input.actionSha256,
    contractVersion: "adjustment-installed-rain-control-reference-receipt/v1",
    controlPlaneSha256: input.environment.WEATHER_CONTROL_PLANE_SHA256,
    controlStateSha256: action.controlStateSha256,
    custodyAnchorSha256: input.custodyAnchorSha256,
    deployedCommit: input.imageCommit,
    deployedImageDigest: input.environment.WEATHER_SERVER_IMAGE.split("@")[1],
    deployedRelease: input.environment.WEATHER_RELEASE,
    deployedSettingsSha256: action.expectedSettingsSha256,
    dueMonth: action.dueMonth,
    fencingToken: action.fencingToken,
    graphManifestSha256: action.graphManifestSha256,
    installedAt: input.clock(),
    ordinalArtifactSha256: action.ordinalArtifactSha256,
    sourceMemberRootSha256: action.sourceMemberRootSha256,
    sourceReceiptRootSha256: action.sourceReceiptRootSha256,
  };
  validateInstalledRainControlReferenceReceipt(receipt);
  const entries = source.entries.filter((entry) => entry.slot !== "control").map(
    // upgrade only legacy shadow entries while preserving their exact nested bytes
    (entry) => source.contractVersion === ADJUSTMENT_FAMILY_CATALOG_VERSION
      ? { ...entry, slot: "shadow" }
      : entry,
  );
  entries.push({ family: "rain", receipt, registration: null, slot: "control" });
  entries.sort((left, right) => left.family.localeCompare(right.family) ||
    left.slot.localeCompare(right.slot));
  return validateInstalledAdjustmentCatalog(canonicalFamilyBytes({
    contractVersion: ADJUSTMENT_FAMILY_CATALOG_V3_VERSION,
    entries,
  }));
}

// verify every installed entry through the compiled runtime loader in its image
async function verifyInstalledAdjustmentCatalog(image, catalogPath) {
  const script = [
    'import { loadInstalledMaintenanceServingCandidate, loadInstalledMaintenanceShadowCandidate, loadInstalledRainMaintenanceControlReference } from "@weather/forecast-adjustment";',
    'for (const family of ["temperature","wind","rain"]) {',
    '  await loadInstalledMaintenanceShadowCandidate({ family, sourceRoot: "/opt/weather" });',
    '  await loadInstalledMaintenanceServingCandidate({ family, sourceRoot: "/opt/weather" });',
    '}',
    'await loadInstalledRainMaintenanceControlReference({ sourceRoot: "/opt/weather" });',
  ].join("\n");
  const result = spawnSync("docker", [
    "run", "--rm", "--network", "none", "--read-only", "--pids-limit", "32",
    "--memory", "192m", "--mount",
    `type=bind,src=${catalogPath},dst=/run/weather/adjustment-candidate-catalog.json,readonly`,
    "--entrypoint", "node", image, "--input-type=module", "--eval", script,
  ], { encoding: "utf8", maxBuffer: 64 * 1_024, stdio: ["ignore", "pipe", "pipe"] });

  // accept only a silent successful compiled-loader verification
  if (result.status !== 0 || result.signal !== null || result.stdout !== "" || result.stderr !== "") {
    throw new Error("installed adjustment catalog runtime verification failed");
  }
}

// verify candidate bytes from a private temporary catalog inode
async function verifyCatalogBytesWithImage(image, bytes, root) {
  await ensureAdjustmentFamilyPrivateDirectory(root);
  const path = join(root, `.catalog-${randomUUID()}.json`);
  try {
    await writeAdjustmentFamilyExclusive(path, bytes, 0o644);
    await verifyInstalledAdjustmentCatalog(image, path);
  } finally {
    await unlink(path).catch(() => undefined);
    await fsyncAdjustmentFamilyDirectory(root);
  }
}

// bind serving registry bytes and the installed shadow receipt as one family state
function adjustmentFamilyIdentity(family, registryBytes, catalog) {
  return createHash("sha256").update(canonicalFamilyBytes({
    activeInstalledReceiptSha256: installedFamilyReceiptSha256(catalog, family, "active"),
    controlInstalledReceiptSha256: family === "rain"
      ? installedFamilyReceiptSha256(catalog, family, "control")
      : null,
    family,
    registrySha256: createHash("sha256").update(registryBytes).digest("hex"),
    shadowInstalledReceiptSha256: installedFamilyReceiptSha256(catalog, family, "shadow"),
  })).digest("hex");
}

// bind both immutable application manifests to one live release identity
function adjustmentFamilyImageIdentity(environment) {
  return createHash("sha256").update(canonicalFamilyBytes([
    { digest: environment.WEATHER_SERVER_IMAGE.split("@")[1], runtime: "server" },
    { digest: environment.WEATHER_WEB_IMAGE.split("@")[1], runtime: "web" },
  ])).digest("hex");
}

// decode one bounded canonical catalog retained in the durable journal
function decodeDurableCatalog(value) {
  if (typeof value !== "string" || value.length > ADJUSTMENT_FAMILY_MAXIMUM_CATALOG_BYTES * 2) {
    throw new Error("durable family catalog is invalid");
  }
  const bytes = Buffer.from(value, "base64");
  validateInstalledAdjustmentCatalog(bytes);
  return bytes;
}

// replace the root-owned catalog atomically before container recreation
async function installAdjustmentFamilyCatalog(path, bytes) {
  validateInstalledAdjustmentCatalog(bytes);
  await writeAdjustmentFamilyAtomic(path, bytes, 0o644);
  const installed = await readHostFile(
    path,
    ADJUSTMENT_FAMILY_MAXIMUM_CATALOG_BYTES,
    0o644,
    [0],
  );

  // reject any post-rename substitution before service mutation
  if (!installed.equals(bytes)) {
    throw new Error("installed adjustment catalog differs");
  }
}

// invoke only the inherited-lock image-switch helper
function runAdjustmentFamilyUpdate(updatePath, release, expectedCurrent, alternateCurrent) {
  requireFamilyRelease(release, "family apply release");
  requireFamilyRelease(expectedCurrent, "family apply expected release");
  requireFamilyRelease(alternateCurrent, "family apply alternate release");
  execFileSync(updatePath, [
    "adjustment-family-apply-unlocked", release, expectedCurrent, alternateCurrent,
  ], {
    env: { ...process.env },
    maxBuffer: 64 * 1_024,
    stdio: ["ignore", "pipe", "pipe"],
  });
}

// verify one applied target or compensation against all durable identities
async function verifyAdjustmentFamilyLive(input) {
  const release = (await readHostFile(input.paths.currentRelease, 128, 0o600, [0]))
    .toString("ascii").trim();

  // require the committed release marker before any image-derived success
  if (release !== input.release) {
    throw new Error("family live release differs");
  }
  const environment = await readFamilyReleaseEnvironment(input.paths, release);
  await verifyRunningAdjustmentImages(input.paths, environment);
  const catalogBytes = await readHostFile(
    input.paths.catalog,
    ADJUSTMENT_FAMILY_MAXIMUM_CATALOG_BYTES,
    0o644,
    [0],
  );
  const expectedCatalogBytes = decodeDurableCatalog(input.catalogBase64);

  // require exact root catalog bytes rather than a semantic substitute
  if (!catalogBytes.equals(expectedCatalogBytes)) {
    throw new Error("family live catalog differs");
  }
  await verifyInstalledAdjustmentCatalog(environment.WEATHER_SERVER_IMAGE, input.paths.catalog);
  const catalog = validateInstalledAdjustmentCatalog(catalogBytes);
  const registryBytes = readAdjustmentImageFile(
    environment.WEATHER_SERVER_IMAGE,
    ADJUSTMENT_FAMILY_REGISTRIES.get(input.family),
    ADJUSTMENT_RUNTIME_PACKAGE_MAX_BYTES,
  );
  const familyIdentitySha256 = adjustmentFamilyIdentity(input.family, registryBytes, catalog);
  const imageDigestsSha256 = adjustmentFamilyImageIdentity(environment);

  // bind both physical family state and live image manifests
  if (familyIdentitySha256 !== input.expectedFamilyIdentitySha256 ||
    imageDigestsSha256 !== input.expectedImageDigestsSha256) {
    throw new Error("family live identity differs");
  }
  const settingsBytes = await readHostFile(input.paths.settings, 4_096, 0o600, [0, 10_002]);
  return Object.freeze({
    actionSha256: input.actionSha256,
    commit: input.commit,
    family: input.family,
    familyIdentitySha256,
    imageDigestsSha256,
    release,
    settingsBytes,
  });
}

// normalize canonical JSON
function canonicalize(value) {
  // preserve array order
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }

  // sort object keys
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonicalize(value[key])]),
    );
  }

  return value;
}

// encode canonical JSON
function canonicalJson(value) {
  return `${canonicalJsonValue(value)}\n`;
}

// encode one canonical JSON value without record framing
function canonicalJsonValue(value) {
  return JSON.stringify(canonicalize(value));
}

// pull bounded database records without eager line buffering
async function* databaseRecordLines(input) {
  let parts = [];
  let partBytes = 0;

  // request the next input chunk only after the prior record is consumed
  for await (const inputChunk of input) {
    const chunk = Buffer.isBuffer(inputChunk) ? inputChunk : Buffer.from(inputChunk);
    let start = 0;

    // split complete newline-delimited records inside this chunk
    while (start < chunk.length) {
      const newline = chunk.indexOf(0x0a, start);
      const end = newline === -1 ? chunk.length : newline;
      const part = chunk.subarray(start, end);
      const nextBytes = partBytes + part.length;

      // reject oversize records before retaining the new bytes
      if (nextBytes > MAX_DATABASE_RECORD_BYTES) {
        throw new Error("database export record is empty or oversized");
      }
      // retain only the current bounded record
      if (part.length > 0) {
        parts.push(part);
        partBytes = nextBytes;
      }

      // await another pulled chunk for an incomplete record
      if (newline === -1) {
        break;
      }

      let line = parts.length === 1 ? parts[0] : Buffer.concat(parts, partBytes);

      // match the prior CRLF normalization without altering bare CR bytes
      if (line.length > 0 && line[line.length - 1] === 0x0d) {
        line = line.subarray(0, line.length - 1);
      }
      yield line.toString("utf8");
      parts = [];
      partBytes = 0;
      start = newline + 1;
    }
  }

  // preserve one final non-newline-terminated record
  if (partBytes > 0) {
    const line = parts.length === 1 ? parts[0] : Buffer.concat(parts, partBytes);
    yield line.toString("utf8");
  }
}

// hash exact bytes
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// hash one bounded file without retaining it
async function sha256File(path) {
  const hash = createHash("sha256");

  // update the digest incrementally
  for await (const chunk of createReadStream(path)) {
    hash.update(chunk);
  }

  return hash.digest("hex");
}

// verify compressed plaintext without retaining decompressed bytes
async function validateCompressedBodyStream(input, compressedBytes, expectedBytes, expectedSha256) {
  // retain the database's fixed receipt bounds
  if (compressedBytes > 2_100_000 || expectedBytes > 2_000_000 ||
    expectedBytes > compressedBytes * MAX_RATIO) {
    throw new Error("rain receipt body exceeds its fixed size limit");
  }
  const hash = createHash("sha256");
  const decoder = new StringDecoder("utf8");
  const forbiddenPrivateMaterial =
    /(api[_-]?key|authorization|password|secret|token|https?:\/\/)/iu;
  let inspectionTail = "";
  let plaintextBytes = 0;
  const counter = new Transform({
    // cap decompressed bytes before hashing them
    transform(chunk, encoding, callback) {
      plaintextBytes += chunk.length;
      if (plaintextBytes > expectedBytes || plaintextBytes > compressedBytes * MAX_RATIO) {
        callback(new Error("rain receipt body exceeds its decompression limit"));
        return;
      }
      hash.update(chunk);
      const inspection = `${inspectionTail}${decoder.write(chunk)}`;

      // reject private material without retaining the decoded body
      if (forbiddenPrivateMaterial.test(inspection)) {
        callback(new Error("rain receipt body contains forbidden private material"));
        return;
      }
      inspectionTail = inspection.slice(-16);
      callback();
    },
  });
  await pipeline(input, createGunzip(), counter);

  // bind exact length and plaintext identity
  if (plaintextBytes !== expectedBytes || hash.digest("hex") !== expectedSha256) {
    throw new Error("rain receipt body hash or decompression ratio is invalid");
  }

  // inspect the decoder's final partial character
  if (forbiddenPrivateMaterial.test(`${inspectionTail}${decoder.end()}`)) {
    throw new Error("rain receipt body contains forbidden private material");
  }
}

// verify one in-memory compressed body through the streaming validator
async function validateCompressedBody(body, expectedBytes, expectedSha256) {
  await validateCompressedBodyStream(
    Readable.from(body),
    body.length,
    expectedBytes,
    expectedSha256,
  );
}

// require a plain object
function requireObject(value, description) {
  // reject arrays and null
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${description} must be an object`);
  }

  return value;
}

// require exact object keys
function requireExactKeys(value, expected, description) {
  const actual = Object.keys(requireObject(value, description)).sort();
  const wanted = [...expected].sort();

  // reject schema drift
  if (JSON.stringify(actual) !== JSON.stringify(wanted)) {
    throw new Error(`${description} has unexpected keys`);
  }
}

// parse one canonical calendar date
function parseCalendarDate(value, description) {
  // reject noncanonical or impossible dates
  if (!DATE_PATTERN.test(value) || new Date(`${value}T00:00:00.000Z`).toISOString().slice(0, 10) !== value) {
    throw new Error(`${description} must use YYYY-MM-DD`);
  }

  return Date.parse(`${value}T00:00:00.000Z`);
}

// validate one bounded inclusive interval
function validateDateRange(fromLocalDate, toLocalDate) {
  const from = parseCalendarDate(fromLocalDate, "fromLocalDate");
  const to = parseCalendarDate(toLocalDate, "toLocalDate");
  const days = Math.trunc((to - from) / 86_400_000) + 1;

  // reject reversed and oversized intervals
  if (days < 1 || days > MAX_DAYS) {
    throw new Error(`export range must contain 1 to ${MAX_DAYS} inclusive dates`);
  }
}

// require one regular nonlinked file below a fixed root
async function requireSafeFile(path, root, description) {
  const canonicalRoot = await realpath(root);
  const canonicalParent = await realpath(dirname(path));
  const metadata = lstatSync(path);

  // reject links, special nodes, and path escapes
  if (!metadata.isFile() || metadata.isSymbolicLink() ||
    (canonicalParent !== canonicalRoot && !canonicalParent.startsWith(`${canonicalRoot}${sep}`))) {
    throw new Error(`${description} must be a regular file below its fixed root`);
  }

  return metadata;
}

// write one stream chunk with backpressure
async function writeChunk(stream, chunk) {
  // wait before accepting more input
  if (!stream.write(chunk)) {
    await once(stream, "drain");
  }
}

// close one writable stream
async function finishStream(stream) {
  stream.end();
  await once(stream, "finish");
}

// count real temporary allocation without following links
function allocatedBytesBelow(path) {
  const details = lstatSync(path, { bigint: true });

  // reject linked and special temporary entries
  if (details.isSymbolicLink() || (!details.isDirectory() && !details.isFile())) {
    throw new Error("export temporary root contains an invalid entry");
  }

  let bytes = Number(details.blocks * 512n);

  // include every fixed-root descendant allocation
  if (details.isDirectory()) {
    for (const name of readdirSync(path)) {
      bytes += allocatedBytesBelow(join(path, name));
    }
  }

  return bytes;
}

// encode one portable tar number
function writeTarNumber(header, offset, length, value) {
  const encoded = value.toString(8).padStart(length - 1, "0");

  // reject values that do not fit the reviewed ustar field
  if (encoded.length >= length) {
    throw new Error("package member is too large for its tar header");
  }

  header.write(`${encoded}\0`, offset, length, "ascii");
}

// build one deterministic regular-file ustar header
function tarFileHeader(path, sizeBytes) {
  const archivePath = `./${path}`;
  const header = Buffer.alloc(TAR_BLOCK_BYTES);
  const splitAt = archivePath.lastIndexOf("/");
  const name = Buffer.byteLength(archivePath) <= 100
    ? archivePath
    : archivePath.slice(splitAt + 1);
  const prefix = name === archivePath ? "" : archivePath.slice(0, splitAt);

  // keep all reviewed package paths in portable ustar fields
  if (Buffer.byteLength(name) > 100 || Buffer.byteLength(prefix) > 155) {
    throw new Error("package member path exceeds the tar limit");
  }

  header.write(name, 0, 100, "utf8");
  writeTarNumber(header, 100, 8, 0o600);
  writeTarNumber(header, 108, 8, 0);
  writeTarNumber(header, 116, 8, 0);
  writeTarNumber(header, 124, 12, sizeBytes);
  writeTarNumber(header, 136, 12, 0);
  header.fill(0x20, 148, 156);
  header.write("0", 156, 1, "ascii");
  header.write("ustar", 257, 5, "ascii");
  header[262] = 0;
  header.write("00", 263, 2, "ascii");
  header.write(prefix, 345, 155, "utf8");
  const checksum = header.reduce((sum, byte) => sum + byte, 0);
  const encodedChecksum = checksum.toString(8).padStart(6, "0");
  header.write(`${encodedChecksum}\0 `, 148, 8, "ascii");
  return header;
}

// stream one exact buffer into the archive
async function writeTarBuffer(stream, path, bytes) {
  await writeChunk(stream, tarFileHeader(path, bytes.length));
  await writeChunk(stream, bytes);
  const padding = (TAR_BLOCK_BYTES - (bytes.length % TAR_BLOCK_BYTES)) % TAR_BLOCK_BYTES;

  // align the next member without retaining prior bytes
  if (padding > 0) {
    await writeChunk(stream, Buffer.alloc(padding));
  }
}

// stream one staged file into the archive
async function writeTarFile(stream, path, sourcePath, sizeBytes) {
  await writeChunk(stream, tarFileHeader(path, sizeBytes));

  // forward the bounded file without buffering it again
  for await (const chunk of createReadStream(sourcePath)) {
    await writeChunk(stream, chunk);
  }
  const padding = (TAR_BLOCK_BYTES - (sizeBytes % TAR_BLOCK_BYTES)) % TAR_BLOCK_BYTES;

  // align the next member
  if (padding > 0) {
    await writeChunk(stream, Buffer.alloc(padding));
  }
}

// validate the immutable database manifest record
function validateDatabaseManifest(value) {
  const manifest = requireObject(value, "database manifest");
  requireExactKeys(manifest, [
    "contract_version",
    "migration_checksums",
    "migration_history_sha256",
    "migration_names",
    "query_contract_sha256",
    "query_contract_version",
    "row_schema_sha256",
    "schema_migration",
    "site_key",
    "site_timezone",
  ], "database manifest");
  const recognizedMigrationLedger = Array.isArray(manifest.migration_names) &&
    Array.isArray(manifest.migration_checksums) &&
    EXPECTED_MIGRATION_LEDGERS.some(
      // match only the exact legacy or complete maintenance ledger
      (ledger) => manifest.migration_history_sha256 === ledger.historySha256 &&
        JSON.stringify(manifest.migration_names) === JSON.stringify(ledger.names) &&
        JSON.stringify(manifest.migration_checksums) === JSON.stringify(ledger.checksums),
    );

  // bind both closed ledgers to the unchanged migration 0017 query
  if (manifest.contract_version !== DATABASE_MANIFEST_VERSION ||
    manifest.schema_migration !== "0017_adjustment_evaluation_export.sql" ||
    manifest.query_contract_version !== "adjustment-evaluation-export-query/v1" ||
    manifest.site_key !== "ballydidean" ||
    manifest.site_timezone !== "America/Los_Angeles" ||
    manifest.row_schema_sha256 !== EXPECTED_ROW_SCHEMA_SHA256 ||
    manifest.query_contract_sha256 !== EXPECTED_QUERY_CONTRACT_SHA256 ||
    !HASH_PATTERN.test(manifest.migration_history_sha256) ||
    !recognizedMigrationLedger) {
    throw new Error("database manifest identity is invalid");
  }

  return manifest;
}

// validate the transaction envelope
function validateTransaction(value, fromLocalDate, toLocalDate) {
  const transaction = requireObject(value, "transaction");
  requireExactKeys(transaction, [
    "created_at_utc",
    "from_local_date",
    "idle_in_transaction_session_timeout",
    "isolation_level",
    "lock_timeout",
    "read_only",
    "statement_timeout",
    "to_local_date",
  ], "transaction");

  // require the fixed read-only repeatable-read query settings
  if (transaction.from_local_date !== fromLocalDate ||
    transaction.to_local_date !== toLocalDate ||
    transaction.isolation_level !== "repeatable read" ||
    transaction.read_only !== "on" ||
    transaction.statement_timeout !== "5min" ||
    transaction.lock_timeout !== "5s" ||
    transaction.idle_in_transaction_session_timeout !== "30s" ||
    !Number.isFinite(Date.parse(transaction.created_at_utc))) {
    throw new Error("transaction contract is invalid");
  }

  return transaction;
}

// validate one exported database row
function validateRow(value) {
  const row = requireObject(value, "export row");
  requireExactKeys(row, [
    "compressed_body_base64",
    "content_hash",
    "first_received_at",
    "last_received_at",
    "local_date",
    "payload",
    "record_kind",
    "record_revision_identity",
    "reference_at",
    "revision_count",
    "site_key",
    "source_identity",
    "valid_at",
  ], "export row");

  // reject unexpected sites, row kinds, and identities
  if (row.site_key !== "ballydidean" || !ROW_KINDS.has(row.record_kind) ||
    typeof row.source_identity !== "string" || row.source_identity.length > 256 ||
    typeof row.record_revision_identity !== "string" || row.record_revision_identity.length > 320 ||
    !DATE_PATTERN.test(row.local_date) || !Number.isFinite(Date.parse(row.valid_at)) ||
    (row.reference_at !== null && !Number.isFinite(Date.parse(row.reference_at))) ||
    !Number.isFinite(Date.parse(row.first_received_at)) ||
    !Number.isFinite(Date.parse(row.last_received_at)) ||
    !Number.isInteger(row.revision_count) || row.revision_count < 0 ||
    (row.content_hash !== null && !HASH_PATTERN.test(row.content_hash)) ||
    (row.compressed_body_base64 !== null &&
      (row.record_kind !== "rain_receipt" ||
        typeof row.compressed_body_base64 !== "string"))) {
    throw new Error("export row identity is invalid");
  }

  requireObject(row.payload, "export row payload");
  requireExactKeys(row.payload, PAYLOAD_KEYS.get(row.record_kind), `${row.record_kind} payload`);

  // reject credential, URL, and authorization material even inside bounded JSON fields
  if (/(api[_-]?key|authorization|password|secret|token|https?:\/\/)/iu.test(
    JSON.stringify(row.payload),
  )) {
    throw new Error("export row payload contains forbidden private material");
  }
  return row;
}

// copy a frozen edge snapshot into package members
async function copyEdgeSnapshot(snapshotPath, packageRoot, maximumBytes) {
  const snapshot = JSON.parse(await readFile(snapshotPath, "utf8"));
  requireExactKeys(snapshot, ["contractVersion", "entries", "frozenAt", "watermarkSha256"], "edge snapshot");

  // reject incomplete or unbounded snapshot identity
  if (snapshot.contractVersion !== EDGE_SNAPSHOT_VERSION ||
    !Number.isFinite(Date.parse(snapshot.frozenAt)) ||
    !HASH_PATTERN.test(snapshot.watermarkSha256) ||
    !Array.isArray(snapshot.entries) || snapshot.entries.length > MAX_ROWS) {
    throw new Error("edge snapshot identity is invalid");
  }

  const receiptRoot = join(packageRoot, "members/edge/receipts");
  const objectRoot = join(packageRoot, "members/edge/objects");
  await mkdir(receiptRoot, { mode: 0o700, recursive: true });
  await mkdir(objectRoot, { mode: 0o700, recursive: true });
  const members = [];
  const seenReceipts = new Set();
  const seenObjects = new Set();
  let copiedBytes = 0;

  // copy every already-validated immutable pair exactly once
  for (const entry of snapshot.entries) {
    requireExactKeys(entry, [
      "edgeReceiptIdentitySha256",
      "objectPath",
      "objectSha256",
      "receiptPath",
    ], "edge snapshot entry");
    if (!HASH_PATTERN.test(entry.edgeReceiptIdentitySha256) ||
      !HASH_PATTERN.test(entry.objectSha256) ||
      seenReceipts.has(entry.edgeReceiptIdentitySha256)) {
      throw new Error("edge snapshot entry identity is invalid");
    }
    seenReceipts.add(entry.edgeReceiptIdentitySha256);
    const receiptSource = resolve(entry.receiptPath);
    const objectSource = resolve(entry.objectPath);
    const sourceRoot = resolve(join(dirname(receiptSource), ".."));
    await requireSafeFile(receiptSource, join(sourceRoot, "receipts"), "edge receipt");
    await requireSafeFile(objectSource, join(sourceRoot, "objects"), "edge object");
    const receiptBytes = await readFile(receiptSource);
    const objectBytes = await readFile(objectSource);
    const objectPlaintext = gunzipSync(objectBytes, {
      maxOutputLength: Math.min(512 * 1024 + 1, objectBytes.length * MAX_RATIO + 1),
    });

    // recheck filename and content identity at the package boundary
    if (basename(receiptSource) !== `sha256-${entry.edgeReceiptIdentitySha256}.json` ||
      basename(objectSource) !== `sha256-${entry.objectSha256}.json.gz` ||
      objectPlaintext.length > objectBytes.length * MAX_RATIO ||
      sha256(objectPlaintext) !== entry.objectSha256) {
      throw new Error("edge snapshot bytes changed after freeze");
    }

    const receiptPath = `members/edge/receipts/${basename(receiptSource)}`;
    copiedBytes += receiptBytes.length;
    if (copiedBytes > maximumBytes) {
      throw new Error("edge evidence exceeds the remaining host temporary limit");
    }
    await writeFile(join(packageRoot, receiptPath), receiptBytes, { flag: "wx", mode: 0o600 });
    members.push({ path: receiptPath, sha256: sha256(receiptBytes), sizeBytes: receiptBytes.length });

    // deduplicate objects shared by overlapping receipts
    if (!seenObjects.has(entry.objectSha256)) {
      seenObjects.add(entry.objectSha256);
      const objectPath = `members/edge/objects/${basename(objectSource)}`;
      copiedBytes += objectBytes.length;
      if (copiedBytes > maximumBytes) {
        throw new Error("edge evidence exceeds the remaining host temporary limit");
      }
      await writeFile(join(packageRoot, objectPath), objectBytes, { flag: "wx", mode: 0o600 });
      members.push({ path: objectPath, sha256: sha256(objectBytes), sizeBytes: objectBytes.length });
    }
  }

  return {
    evidence: {
      contractVersion: snapshot.contractVersion,
      frozenAt: snapshot.frozenAt,
      objectCount: seenObjects.size,
      receiptCount: seenReceipts.size,
      watermarkSha256: snapshot.watermarkSha256,
    },
    members,
  };
}

// stream a frozen edge snapshot without staging its object set
async function streamEdgeSnapshot(snapshotPath, archive, maximumPackageBytes) {
  const snapshot = JSON.parse(await readFile(snapshotPath, "utf8"));
  requireExactKeys(snapshot, ["contractVersion", "entries", "frozenAt", "watermarkSha256"], "edge snapshot");

  // reject incomplete or unbounded snapshot identity
  if (snapshot.contractVersion !== EDGE_SNAPSHOT_VERSION ||
    !Number.isFinite(Date.parse(snapshot.frozenAt)) ||
    !HASH_PATTERN.test(snapshot.watermarkSha256) ||
    !Array.isArray(snapshot.entries) || snapshot.entries.length > MAX_ROWS) {
    throw new Error("edge snapshot identity is invalid");
  }

  const members = [];
  const seenReceipts = new Set();
  const seenObjects = new Set();
  let packageBytes = 0;

  // stream every frozen pair while retaining only one pair in memory
  for (const entry of snapshot.entries) {
    requireExactKeys(entry, [
      "edgeReceiptIdentitySha256",
      "objectPath",
      "objectSha256",
      "receiptPath",
    ], "edge snapshot entry");
    if (!HASH_PATTERN.test(entry.edgeReceiptIdentitySha256) ||
      !HASH_PATTERN.test(entry.objectSha256) ||
      seenReceipts.has(entry.edgeReceiptIdentitySha256)) {
      throw new Error("edge snapshot entry identity is invalid");
    }
    seenReceipts.add(entry.edgeReceiptIdentitySha256);
    const receiptSource = resolve(entry.receiptPath);
    const objectSource = resolve(entry.objectPath);
    const sourceRoot = resolve(join(dirname(receiptSource), ".."));
    await requireSafeFile(receiptSource, join(sourceRoot, "receipts"), "edge receipt");
    await requireSafeFile(objectSource, join(sourceRoot, "objects"), "edge object");
    const receiptBytes = await readFile(receiptSource);
    const objectBytes = await readFile(objectSource);
    const objectPlaintext = gunzipSync(objectBytes, {
      maxOutputLength: Math.min(512 * 1024 + 1, objectBytes.length * MAX_RATIO + 1),
    });
    const receipt = JSON.parse(receiptBytes.toString("utf8"));
    const object = JSON.parse(objectPlaintext.toString("utf8"));
    requireExactKeys(receipt, [
      "availability",
      "contractVersion",
      "edgeReceiptIdentitySha256",
      "firstEdgeCommittedAt",
      "objectSha256",
      "siteKey",
      "window",
    ], "edge receipt");
    requireExactKeys(receipt.availability, ["rowTimestampIndexes", "timestamps"], "edge receipt availability");

    // rebind the frozen names, canonical bytes, and receipt-to-object pair
    if (basename(receiptSource) !== `sha256-${entry.edgeReceiptIdentitySha256}.json` ||
      basename(objectSource) !== `sha256-${entry.objectSha256}.json.gz` ||
      receipt.edgeReceiptIdentitySha256 !== entry.edgeReceiptIdentitySha256 ||
      receipt.objectSha256 !== entry.objectSha256 || receipt.siteKey !== "ballydidean" ||
      !Number.isFinite(Date.parse(receipt.firstEdgeCommittedAt)) ||
      !Array.isArray(receipt.availability.timestamps) ||
      !Array.isArray(receipt.availability.rowTimestampIndexes) ||
      !Array.isArray(object.rows) ||
      receipt.availability.rowTimestampIndexes.length !== object.rows.length ||
      receipt.availability.timestamps.some((value) => !Number.isFinite(Date.parse(value))) ||
      receipt.availability.rowTimestampIndexes.some((index) =>
        !Number.isSafeInteger(index) || index < 0 ||
        index >= receipt.availability.timestamps.length) ||
      !Buffer.from(canonicalJson(receipt)).equals(receiptBytes) ||
      !Buffer.from(canonicalJsonValue(object)).equals(objectPlaintext) ||
      !gzipSync(objectPlaintext, { level: 9, mtime: 0 }).equals(objectBytes) ||
      objectPlaintext.length > objectBytes.length * MAX_RATIO ||
      sha256(objectPlaintext) !== entry.objectSha256) {
      throw new Error("edge snapshot bytes changed after freeze");
    }

    const receiptPath = `members/edge/receipts/${basename(receiptSource)}`;
    packageBytes += receiptBytes.length;
    if (packageBytes > maximumPackageBytes) {
      throw new Error("edge evidence exceeds the remaining package limit");
    }
    await writeTarBuffer(archive, receiptPath, receiptBytes);
    members.push({ path: receiptPath, sha256: sha256(receiptBytes), sizeBytes: receiptBytes.length });

    // emit each shared object once
    if (!seenObjects.has(entry.objectSha256)) {
      seenObjects.add(entry.objectSha256);
      const objectPath = `members/edge/objects/${basename(objectSource)}`;
      packageBytes += objectBytes.length;
      if (packageBytes > maximumPackageBytes) {
        throw new Error("edge evidence exceeds the remaining package limit");
      }
      await writeTarBuffer(archive, objectPath, objectBytes);
      members.push({ path: objectPath, sha256: sha256(objectBytes), sizeBytes: objectBytes.length });
    }
  }

  return {
    evidence: {
      contractVersion: snapshot.contractVersion,
      frozenAt: snapshot.frozenAt,
      objectCount: seenObjects.size,
      receiptCount: seenReceipts.size,
      watermarkSha256: snapshot.watermarkSha256,
    },
    members,
    packageBytes,
  };
}

// build one verified package directory from the database stream
export async function buildAdjustmentEvaluationPackage({
  edgeSnapshotPath,
  fromLocalDate,
  input = process.stdin,
  packageRoot,
  toLocalDate,
}) {
  validateDateRange(fromLocalDate, toLocalDate);
  await mkdir(join(packageRoot, "members/bodies"), { mode: 0o700, recursive: true });
  const rowsPath = join(packageRoot, "members/rows.jsonl.gz");
  const rowsOutput = createWriteStream(rowsPath, { flags: "wx", mode: 0o600 });
  const rowsGzip = createGzip({ level: 9, mtime: 0 });
  let rowsCompressedBytes = 0;
  const rowsCap = new Transform({
    // count the only non-body database temporary
    transform(chunk, encoding, callback) {
      rowsCompressedBytes += chunk.length;
      if (rowsCompressedBytes > MAX_OTHER_TEMP_BYTES) {
        callback(new Error("compressed row member exceeds the non-body temporary limit"));
        return;
      }
      callback(null, chunk);
    },
  });
  const rowsPipeline = pipeline(rowsGzip, rowsCap, rowsOutput);
  void rowsPipeline.catch(() => undefined);
  let databaseManifest;
  let transaction;
  let canonicalBytes = 0;
  let compressedBodyBytes = 0;
  let rowCount = 0;
  let revisedRowCount = 0;
  let maxRevisionCount = 0;
  let maxSourceReceiptAt = null;
  const sourceIdentities = new Set();
  const bodyMembers = [];
  const seenBodies = new Set();
  let completed = false;

  try {

  // stream the fixed manifest followed by ordered rows
  for await (const line of databaseRecordLines(input)) {
    // reject blank or individually unbounded records
    if (line.length === 0) {
      throw new Error("database export record is empty or oversized");
    }
    const record = JSON.parse(line);
    requireObject(record, "database export record");

    // accept exactly one leading manifest
    if (record.record_type === "manifest") {
      requireExactKeys(record, ["payload", "record_type", "transaction"], "manifest record");
      if (databaseManifest !== undefined || rowCount !== 0) {
        throw new Error("database manifest must be the first and only manifest");
      }
      databaseManifest = validateDatabaseManifest(record.payload);
      transaction = validateTransaction(record.transaction, fromLocalDate, toLocalDate);
      continue;
    }

    requireExactKeys(record, ["payload", "record_type"], "row record");
    if (record.record_type !== "row" || databaseManifest === undefined) {
      throw new Error("database row is outside its manifest");
    }
    const row = validateRow(record.payload);
    rowCount += 1;

    // stop before accepting a row beyond the reviewed bound
    if (rowCount > MAX_ROWS) {
      throw new Error("database export exceeds the row limit");
    }

    let bodyMemberPath = null;
    // separate retained compressed bodies from canonical row JSON
    if (row.compressed_body_base64 !== null) {
      const body = Buffer.from(row.compressed_body_base64, "base64");
      const declaredBytes = row.payload.compressedBytes;
      const bodySha256 = row.payload.bodySha256;
      const declaredPlaintextBytes = row.payload.bodyBytes;
      if (!Number.isInteger(declaredBytes) || declaredBytes !== body.length ||
        !Number.isInteger(declaredPlaintextBytes) || declaredPlaintextBytes < 0 ||
        !HASH_PATTERN.test(bodySha256) || row.content_hash !== bodySha256) {
        throw new Error("rain receipt body metadata is invalid");
      }
      await validateCompressedBody(body, declaredPlaintextBytes, bodySha256);
      bodyMemberPath = `members/bodies/sha256-${bodySha256}.json.gz`;

      // copy each content-addressed provider body once
      if (!seenBodies.has(bodySha256)) {
        seenBodies.add(bodySha256);
        compressedBodyBytes += body.length;
        if (compressedBodyBytes > MAX_COMPRESSED_BODY_BYTES) {
          throw new Error("retained compressed bodies exceed the package limit");
        }
        await writeFile(join(packageRoot, bodyMemberPath), body, { flag: "wx", mode: 0o600 });
        bodyMembers.push({ path: bodyMemberPath, sha256: sha256(body), sizeBytes: body.length });
      }
    }

    const canonicalRow = {
      ...row,
      body_member_path: bodyMemberPath,
    };
    delete canonicalRow.compressed_body_base64;
    const encoded = canonicalJson(canonicalRow);
    canonicalBytes += Buffer.byteLength(encoded);

    // reject the stream before it can exceed its canonical budget
    if (canonicalBytes > MAX_CANONICAL_BYTES) {
      throw new Error("canonical database stream exceeds the package limit");
    }
    await writeChunk(rowsGzip, encoded);
    sourceIdentities.add(row.source_identity);
    revisedRowCount += row.revision_count > 0 ? 1 : 0;
    maxRevisionCount = Math.max(maxRevisionCount, row.revision_count);
    if (maxSourceReceiptAt === null || row.last_received_at > maxSourceReceiptAt) {
      maxSourceReceiptAt = row.last_received_at;
    }
  }

  // require a complete transaction even when it contains no rows
  if (databaseManifest === undefined || transaction === undefined) {
    throw new Error("database export manifest is missing");
  }
  await finishStream(rowsGzip);
  await rowsPipeline;
  const rowsBytes = await readFile(rowsPath);
  const databaseTemporaryBytes = rowsBytes.length + bodyMembers.reduce(
    (total, member) => total + member.sizeBytes,
    0,
  );
  if (databaseTemporaryBytes > MAX_TEMP_BYTES) {
    throw new Error("database members exceed the host temporary limit");
  }
  const edge = await copyEdgeSnapshot(
    edgeSnapshotPath,
    packageRoot,
    MAX_TEMP_BYTES - databaseTemporaryBytes,
  );
  const members = [
    { path: "members/rows.jsonl.gz", sha256: sha256(rowsBytes), sizeBytes: rowsBytes.length },
    ...bodyMembers,
    ...edge.members,
  ].sort((left, right) => left.path.localeCompare(right.path, "en"));
  const temporaryBytes = members.reduce((total, member) => total + member.sizeBytes, 0);

  // enforce total simultaneous package allocation
  if (temporaryBytes > MAX_TEMP_BYTES) {
    throw new Error("package members exceed the host temporary limit");
  }

  const manifest = {
    canonicalStreamBytes: canonicalBytes,
    compressedBodyBytes,
    contractVersion: PACKAGE_VERSION,
    databaseManifest,
    edgeEvidence: edge.evidence,
    fromLocalDate,
    limits: {
      maxArchiveBytes: MAX_ARCHIVE_BYTES,
      maxCanonicalStreamBytes: MAX_CANONICAL_BYTES,
      maxCompressedBodyBytes: MAX_COMPRESSED_BODY_BYTES,
      maxDays: MAX_DAYS,
      maxDecompressionRatio: MAX_RATIO,
      maxOtherSimultaneousTemporaryBytes: MAX_OTHER_TEMP_BYTES,
      maxRows: MAX_ROWS,
      maxSimultaneousHostTemporaryBytes: MAX_TEMP_BYTES,
    },
    maxSourceReceiptAt,
    members,
    revisionDiagnostics: {
      maxRevisionCount,
      revisedRowCount,
      rowCount,
    },
    siteKey: "ballydidean",
    siteTimezone: "America/Los_Angeles",
    sourceIdentities: [...sourceIdentities].sort(),
    toLocalDate,
    transaction: {
      createdAtUtc: transaction.created_at_utc,
      idleInTransactionSessionTimeout: transaction.idle_in_transaction_session_timeout,
      isolationLevel: transaction.isolation_level,
      lockTimeout: transaction.lock_timeout,
      readOnly: transaction.read_only,
      statementTimeout: transaction.statement_timeout,
    },
  };
  const manifestBytes = canonicalJson(manifest);
  const manifestSha256 = sha256(manifestBytes);
  await writeFile(join(packageRoot, "manifest.json"), manifestBytes, { flag: "wx", mode: 0o600 });
  await writeFile(join(packageRoot, "manifest.sha256"), `${manifestSha256}\n`, {
    flag: "wx",
    mode: 0o600,
  });
  completed = true;
  return manifestSha256;
  } finally {
    // remove every partial package after validation or streaming failure
    if (!completed) {
      rowsGzip.destroy();
      rowsOutput.destroy();
      await rm(packageRoot, { force: true, recursive: true });
    }
  }
}

// stream one verified archive while staging only its compressed row member
export async function streamAdjustmentEvaluationPackageArchive({
  edgeSnapshotPath,
  fromLocalDate,
  input = process.stdin,
  output = process.stdout,
  temporaryRoot,
  toLocalDate,
}) {
  validateDateRange(fromLocalDate, toLocalDate);
  const canonicalTemporaryRoot = await realpath(temporaryRoot);
  const rowsPath = join(canonicalTemporaryRoot, "rows.jsonl.gz");
  const allocationBeforeRows = allocatedBytesBelow(canonicalTemporaryRoot);
  const filesystemBlockBytes = statfsSync(canonicalTemporaryRoot).bsize;

  // leave room for filesystem block rounding
  const rowsTemporaryLimit = MAX_OTHER_TEMP_BYTES - allocationBeforeRows -
    (filesystemBlockBytes - 1);
  if (rowsTemporaryLimit < 0) {
    throw new Error("export non-archive temporary allocation exceeds its limit");
  }

  const rowsOutput = createWriteStream(rowsPath, { flags: "wx", mode: 0o600 });
  const rowsGzip = createGzip({ level: 9, mtime: 0 });
  let rowsCompressedBytes = 0;
  const rowsCap = new Transform({
    // stop the only staged member before the fixed temp budget
    transform(chunk, encoding, callback) {
      rowsCompressedBytes += chunk.length;
      if (rowsCompressedBytes > rowsTemporaryLimit) {
        callback(new Error("compressed row member exceeds the non-archive temporary limit"));
        return;
      }
      callback(null, chunk);
    },
  });
  const rowsPipeline = pipeline(rowsGzip, rowsCap, rowsOutput);
  void rowsPipeline.catch(() => undefined);
  const archiveGzip = createGzip({ level: 9, mtime: 0 });
  let archiveBytes = 0;
  const archiveCap = new Transform({
    // stop egress at the reviewed archive boundary
    transform(chunk, encoding, callback) {
      archiveBytes += chunk.length;
      if (archiveBytes > MAX_ARCHIVE_BYTES) {
        callback(new Error("compressed archive exceeds the package limit"));
        return;
      }
      callback(null, chunk);
    },
  });
  const archivePipeline = pipeline(archiveGzip, archiveCap, output);
  void archivePipeline.catch(() => undefined);
  let databaseManifest;
  let transaction;
  let canonicalBytes = 0;
  let compressedBodyBytes = 0;
  let packageMemberBytes = 0;
  let rowCount = 0;
  let revisedRowCount = 0;
  let maxRevisionCount = 0;
  let maxSourceReceiptAt = null;
  const sourceIdentities = new Set();
  const bodyMembers = [];
  const seenBodies = new Set();
  let completed = false;

  try {
    // consume one manifest followed by bounded ordered rows
    for await (const line of databaseRecordLines(input)) {
      // reject blank or individually unbounded records
      if (line.length === 0) {
        throw new Error("database export record is empty or oversized");
      }
      const record = JSON.parse(line);
      requireObject(record, "database export record");

      // accept exactly one leading manifest
      if (record.record_type === "manifest") {
        requireExactKeys(record, ["payload", "record_type", "transaction"], "manifest record");
        if (databaseManifest !== undefined || rowCount !== 0) {
          throw new Error("database manifest must be the first and only manifest");
        }
        databaseManifest = validateDatabaseManifest(record.payload);
        transaction = validateTransaction(record.transaction, fromLocalDate, toLocalDate);
        continue;
      }

      requireExactKeys(record, ["payload", "record_type"], "row record");
      if (record.record_type !== "row" || databaseManifest === undefined) {
        throw new Error("database row is outside its manifest");
      }
      const row = validateRow(record.payload);
      rowCount += 1;

      // stop before accepting a row beyond the reviewed bound
      if (rowCount > MAX_ROWS) {
        throw new Error("database export exceeds the row limit");
      }

      let bodyMemberPath = null;

      // stream retained bodies directly into the archive
      if (row.compressed_body_base64 !== null) {
        const normalizedBase64 = row.compressed_body_base64.replaceAll("\n", "");
        const body = Buffer.from(normalizedBase64, "base64");
        const declaredBytes = row.payload.compressedBytes;
        const bodySha256 = row.payload.bodySha256;
        const declaredPlaintextBytes = row.payload.bodyBytes;
        if (!Number.isInteger(declaredBytes) || declaredBytes !== body.length ||
          !Number.isInteger(declaredPlaintextBytes) || declaredPlaintextBytes < 0 ||
          !HASH_PATTERN.test(bodySha256) || row.content_hash !== bodySha256 ||
          !/^[A-Za-z0-9+/]*={0,2}$/u.test(normalizedBase64) ||
          body.toString("base64") !== normalizedBase64) {
          throw new Error("rain receipt body metadata is invalid");
        }
        await validateCompressedBody(body, declaredPlaintextBytes, bodySha256);
        bodyMemberPath = `members/bodies/sha256-${bodySha256}.json.gz`;

        // emit each content-addressed provider body once
        if (!seenBodies.has(bodySha256)) {
          seenBodies.add(bodySha256);
          compressedBodyBytes += body.length;
          packageMemberBytes += body.length;
          if (compressedBodyBytes > MAX_COMPRESSED_BODY_BYTES ||
            packageMemberBytes > MAX_TEMP_BYTES) {
            throw new Error("retained compressed bodies exceed the package limit");
          }
          await writeTarBuffer(archiveGzip, bodyMemberPath, body);
          bodyMembers.push({
            path: bodyMemberPath,
            sha256: sha256(body),
            sizeBytes: body.length,
          });
        }
      }

      const canonicalRow = { ...row, body_member_path: bodyMemberPath };
      delete canonicalRow.compressed_body_base64;
      const encoded = canonicalJson(canonicalRow);
      canonicalBytes += Buffer.byteLength(encoded);

      // reject the logical stream before it exceeds its fixed cap
      if (canonicalBytes > MAX_CANONICAL_BYTES) {
        throw new Error("canonical database stream exceeds the package limit");
      }
      await writeChunk(rowsGzip, encoded);
      sourceIdentities.add(row.source_identity);
      revisedRowCount += row.revision_count > 0 ? 1 : 0;
      maxRevisionCount = Math.max(maxRevisionCount, row.revision_count);
      if (maxSourceReceiptAt === null || row.last_received_at > maxSourceReceiptAt) {
        maxSourceReceiptAt = row.last_received_at;
      }
    }

    // require a complete database transaction envelope
    if (databaseManifest === undefined || transaction === undefined) {
      throw new Error("database export manifest is missing");
    }
    await finishStream(rowsGzip);
    await rowsPipeline;
    if (allocatedBytesBelow(canonicalTemporaryRoot) > MAX_OTHER_TEMP_BYTES) {
      throw new Error("export non-archive temporary allocation exceeds its limit");
    }
    const rowsSize = statSync(rowsPath).size;
    packageMemberBytes += rowsSize;
    if (packageMemberBytes > MAX_TEMP_BYTES) {
      throw new Error("database members exceed the package limit");
    }
    const edge = await streamEdgeSnapshot(
      edgeSnapshotPath,
      archiveGzip,
      MAX_TEMP_BYTES - packageMemberBytes,
    );
    packageMemberBytes += edge.packageBytes;
    const rowsMember = {
      path: "members/rows.jsonl.gz",
      sha256: await sha256File(rowsPath),
      sizeBytes: rowsSize,
    };
    await writeTarFile(archiveGzip, rowsMember.path, rowsPath, rowsSize);
    const members = [rowsMember, ...bodyMembers, ...edge.members]
      .sort((left, right) => left.path.localeCompare(right.path, "en"));
    const manifest = {
      canonicalStreamBytes: canonicalBytes,
      compressedBodyBytes,
      contractVersion: PACKAGE_VERSION,
      databaseManifest,
      edgeEvidence: edge.evidence,
      fromLocalDate,
      limits: {
        maxArchiveBytes: MAX_ARCHIVE_BYTES,
        maxCanonicalStreamBytes: MAX_CANONICAL_BYTES,
        maxCompressedBodyBytes: MAX_COMPRESSED_BODY_BYTES,
        maxDays: MAX_DAYS,
        maxDecompressionRatio: MAX_RATIO,
        maxOtherSimultaneousTemporaryBytes: MAX_OTHER_TEMP_BYTES,
        maxRows: MAX_ROWS,
        maxSimultaneousHostTemporaryBytes: MAX_TEMP_BYTES,
      },
      maxSourceReceiptAt,
      members,
      revisionDiagnostics: { maxRevisionCount, revisedRowCount, rowCount },
      siteKey: "ballydidean",
      siteTimezone: "America/Los_Angeles",
      sourceIdentities: [...sourceIdentities].sort(),
      toLocalDate,
      transaction: {
        createdAtUtc: transaction.created_at_utc,
        idleInTransactionSessionTimeout: transaction.idle_in_transaction_session_timeout,
        isolationLevel: transaction.isolation_level,
        lockTimeout: transaction.lock_timeout,
        readOnly: transaction.read_only,
        statementTimeout: transaction.statement_timeout,
      },
    };
    const manifestBytes = Buffer.from(canonicalJson(manifest));
    const manifestSha256 = sha256(manifestBytes);
    const manifestHashBytes = Buffer.from(`${manifestSha256}\n`);

    // include manifest overhead in the extracted-package cap
    if (packageMemberBytes + manifestBytes.length + manifestHashBytes.length > MAX_TEMP_BYTES) {
      throw new Error("package members exceed the extracted package limit");
    }
    await writeTarBuffer(archiveGzip, "manifest.json", manifestBytes);
    await writeTarBuffer(archiveGzip, "manifest.sha256", manifestHashBytes);
    await writeChunk(archiveGzip, Buffer.alloc(TAR_BLOCK_BYTES * 2));
    await finishStream(archiveGzip);
    await archivePipeline;
    completed = true;
    return manifestSha256;
  } finally {
    // close failed pipelines and remove only the staged row member
    if (!completed) {
      rowsGzip.destroy();
      rowsOutput.destroy();
      archiveGzip.destroy();
      archiveCap.destroy();
    }
    await Promise.allSettled([rowsPipeline, archivePipeline]);
    await rm(rowsPath, { force: true });
  }
}

// enumerate every regular package file
function listPackageFiles(root) {
  const files = [];

  // recurse through fixed package directories
  function visit(path) {
    // reject symbolic and special entries
    for (const name of readdirSync(path)) {
      const child = join(path, name);
      const metadata = lstatSync(child);
      if (metadata.isSymbolicLink()) {
        throw new Error("package contains a symbolic link");
      }
      if (metadata.isDirectory()) {
        visit(child);
      } else if (metadata.isFile()) {
        files.push(relative(root, child));
      } else {
        throw new Error("package contains a special file");
      }
    }
  }

  visit(root);
  return files.sort();
}

// read one stable no-follow control file inside a fixed byte bound
async function readNoFollowBoundedFile(path, maximumBytes, description) {
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);

  try {
    const before = await handle.stat({ bigint: true });

    // reject special and oversized control files before allocation
    if (!before.isFile() || before.size > BigInt(maximumBytes)) {
      throw new Error(`${description} is not a bounded regular file`);
    }
    const bytes = Buffer.alloc(Number(before.size));
    let offset = 0;

    // fill only the allocation proven by the opened descriptor
    while (offset < bytes.length) {
      const read = await handle.read(bytes, offset, bytes.length - offset, offset);

      // reject an unexpected early end
      if (read.bytesRead === 0) {
        break;
      }
      offset += read.bytesRead;
    }
    const probe = Buffer.alloc(1);
    const extra = await handle.read(probe, 0, 1, offset);
    const after = await handle.stat({ bigint: true });

    // reject replacement or mutation during the bounded read
    if (offset !== bytes.length || extra.bytesRead !== 0 ||
      before.dev !== after.dev || before.ino !== after.ino ||
      before.size !== after.size || before.mtimeNs !== after.mtimeNs ||
      before.ctimeNs !== after.ctimeNs) {
      throw new Error(`${description} changed while reading`);
    }
    return bytes;
  } finally {
    await handle.close();
  }
}

// validate one package manifest and its fixed contracts
async function readPackageManifest(packageRoot) {
  const manifestBytes = await readNoFollowBoundedFile(
    join(packageRoot, "manifest.json"),
    MAX_OTHER_TEMP_BYTES,
    "package manifest",
  );
  const hashBytes = await readNoFollowBoundedFile(
    join(packageRoot, "manifest.sha256"),
    65,
    "package manifest hash",
  );
  const recordedHash = hashBytes.toString("utf8").trim();
  const manifestHash = sha256(manifestBytes);

  // reject mutable or malformed manifest identity
  if (!HASH_PATTERN.test(recordedHash) || recordedHash !== manifestHash) {
    throw new Error("package manifest hash differs");
  }
  const manifest = JSON.parse(manifestBytes);
  requireExactKeys(manifest, [
    "canonicalStreamBytes",
    "compressedBodyBytes",
    "contractVersion",
    "databaseManifest",
    "edgeEvidence",
    "fromLocalDate",
    "limits",
    "maxSourceReceiptAt",
    "members",
    "revisionDiagnostics",
    "siteKey",
    "siteTimezone",
    "sourceIdentities",
    "toLocalDate",
    "transaction",
  ], "package manifest");

  // bind the immutable package contract
  if (manifest.contractVersion !== PACKAGE_VERSION || manifest.siteKey !== "ballydidean" ||
    manifest.siteTimezone !== "America/Los_Angeles") {
    throw new Error("package manifest contract is invalid");
  }
  requireExactKeys(manifest.limits, [
    "maxArchiveBytes",
    "maxCanonicalStreamBytes",
    "maxCompressedBodyBytes",
    "maxDays",
    "maxDecompressionRatio",
    "maxOtherSimultaneousTemporaryBytes",
    "maxRows",
    "maxSimultaneousHostTemporaryBytes",
  ], "package limits");

  // bind every published resource limit
  if (manifest.limits.maxArchiveBytes !== MAX_ARCHIVE_BYTES ||
    manifest.limits.maxCanonicalStreamBytes !== MAX_CANONICAL_BYTES ||
    manifest.limits.maxCompressedBodyBytes !== MAX_COMPRESSED_BODY_BYTES ||
    manifest.limits.maxDays !== MAX_DAYS ||
    manifest.limits.maxDecompressionRatio !== MAX_RATIO ||
    manifest.limits.maxOtherSimultaneousTemporaryBytes !== MAX_OTHER_TEMP_BYTES ||
    manifest.limits.maxRows !== MAX_ROWS ||
    manifest.limits.maxSimultaneousHostTemporaryBytes !== MAX_TEMP_BYTES ||
    !Number.isInteger(manifest.canonicalStreamBytes) ||
    manifest.canonicalStreamBytes < 0 || manifest.canonicalStreamBytes > MAX_CANONICAL_BYTES ||
    !Number.isInteger(manifest.compressedBodyBytes) ||
    manifest.compressedBodyBytes < 0 ||
    manifest.compressedBodyBytes > MAX_COMPRESSED_BODY_BYTES) {
    throw new Error("package limits are invalid");
  }
  requireExactKeys(manifest.edgeEvidence, [
    "contractVersion",
    "frozenAt",
    "objectCount",
    "receiptCount",
    "watermarkSha256",
  ], "edge evidence");

  // validate the frozen edge summary
  if (manifest.edgeEvidence.contractVersion !== EDGE_SNAPSHOT_VERSION ||
    !Number.isFinite(Date.parse(manifest.edgeEvidence.frozenAt)) ||
    !HASH_PATTERN.test(manifest.edgeEvidence.watermarkSha256) ||
    !Number.isInteger(manifest.edgeEvidence.objectCount) ||
    !Number.isInteger(manifest.edgeEvidence.receiptCount) ||
    manifest.edgeEvidence.objectCount < 0 || manifest.edgeEvidence.receiptCount < 0) {
    throw new Error("edge evidence identity is invalid");
  }
  requireExactKeys(manifest.revisionDiagnostics, [
    "maxRevisionCount",
    "revisedRowCount",
    "rowCount",
  ], "revision diagnostics");

  // validate the bounded row summary
  if (!Number.isInteger(manifest.revisionDiagnostics.maxRevisionCount) ||
    !Number.isInteger(manifest.revisionDiagnostics.revisedRowCount) ||
    !Number.isInteger(manifest.revisionDiagnostics.rowCount) ||
    manifest.revisionDiagnostics.maxRevisionCount < 0 ||
    manifest.revisionDiagnostics.revisedRowCount < 0 ||
    manifest.revisionDiagnostics.rowCount < 0 ||
    manifest.revisionDiagnostics.rowCount > MAX_ROWS ||
    manifest.revisionDiagnostics.revisedRowCount > manifest.revisionDiagnostics.rowCount) {
    throw new Error("revision diagnostics are invalid");
  }
  requireExactKeys(manifest.transaction, [
    "createdAtUtc",
    "idleInTransactionSessionTimeout",
    "isolationLevel",
    "lockTimeout",
    "readOnly",
    "statementTimeout",
  ], "package transaction");

  // validate the exported transaction and bounded identities
  if (!Number.isFinite(Date.parse(manifest.transaction.createdAtUtc)) ||
    manifest.transaction.isolationLevel !== "repeatable read" ||
    manifest.transaction.readOnly !== "on" ||
    manifest.transaction.statementTimeout !== "5min" ||
    manifest.transaction.lockTimeout !== "5s" ||
    manifest.transaction.idleInTransactionSessionTimeout !== "30s" ||
    (manifest.maxSourceReceiptAt !== null &&
      !Number.isFinite(Date.parse(manifest.maxSourceReceiptAt))) ||
    !Array.isArray(manifest.members) || !Array.isArray(manifest.sourceIdentities) ||
    manifest.sourceIdentities.some((identity) =>
      typeof identity !== "string" || identity.length > 256)) {
    throw new Error("package transaction or source identities are invalid");
  }
  validateDateRange(manifest.fromLocalDate, manifest.toLocalDate);
  validateDatabaseManifest(manifest.databaseManifest);
  return { manifest, manifestBytes, manifestHash };
}

// validate declared paths before opening any package member
function validatePackageMemberDeclarations(packageRoot, manifest) {
  const expectedFiles = new Set(["manifest.json", "manifest.sha256"]);
  const membersByPath = new Map();
  let memberBytes = 0;

  // validate every immutable member declaration
  for (const member of manifest.members) {
    requireExactKeys(member, ["path", "sha256", "sizeBytes"], "package member");
    if (member.path !== "members/rows.jsonl.gz" &&
      !/^members\/bodies\/sha256-[a-f0-9]{64}\.json\.gz$/u.test(member.path) &&
      !/^members\/edge\/objects\/sha256-[a-f0-9]{64}\.json\.gz$/u.test(member.path) &&
      !/^members\/edge\/receipts\/sha256-[a-f0-9]{64}\.json$/u.test(member.path)) {
      throw new Error("package member path is invalid");
    }

    // reject duplicate, escaping, and malformed identities
    if (member.path.includes("..") ||
      !HASH_PATTERN.test(member.sha256) || !Number.isInteger(member.sizeBytes) ||
      member.sizeBytes < 0 || expectedFiles.has(member.path)) {
      throw new Error("package member identity is invalid");
    }
    expectedFiles.add(member.path);
    membersByPath.set(member.path, member);
    memberBytes += member.sizeBytes;
  }

  // enforce the complete extracted allocation
  if (memberBytes > MAX_TEMP_BYTES) {
    throw new Error("package member allocation exceeds the host temporary limit");
  }
  const actualFiles = listPackageFiles(packageRoot);

  // reject undeclared members and archive additions
  if (JSON.stringify(actualFiles) !== JSON.stringify([...expectedFiles].sort())) {
    throw new Error("package contains undeclared files");
  }
  return { memberBytes, membersByPath };
}

// consume one stable member while hashing the exact opened byte stream
async function consumeVerifiedMember(packageRoot, member, consume) {
  const path = join(packageRoot, member.path);
  await requireSafeFile(path, packageRoot, "package member");
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  const hash = createHash("sha256");
  let bytes = 0;
  let source;
  let verifier;

  try {
    const before = await handle.stat({ bigint: true });

    // bind the opened regular file before consumption
    if (!before.isFile() || before.size !== BigInt(member.sizeBytes)) {
      throw new Error("package member bytes differ");
    }
    verifier = new Transform({
      // bind byte count and digest before forwarding
      transform(chunk, encoding, callback) {
        bytes += chunk.length;
        if (bytes > member.sizeBytes) {
          callback(new Error("package member bytes differ"));
          return;
        }
        hash.update(chunk);
        callback(null, chunk);
      },
    });
    source = handle.createReadStream({ autoClose: false });
    source.once("error", (error) => verifier.destroy(error));
    source.pipe(verifier);
    const result = await consume(verifier);
    const after = await handle.stat({ bigint: true });

    // reject content changes and identity replacement during consumption
    if (bytes !== member.sizeBytes || hash.digest("hex") !== member.sha256 ||
      before.dev !== after.dev || before.ino !== after.ino ||
      before.size !== after.size || before.mtimeNs !== after.mtimeNs ||
      before.ctimeNs !== after.ctimeNs) {
      throw new Error("package member bytes differ");
    }
    return result;
  } finally {
    source?.destroy();
    verifier?.destroy();
    await handle.close();
  }
}

// read one already-bounded member after binding its stream identity
async function readVerifiedMember(packageRoot, member, maximumBytes) {
  if (member.sizeBytes > maximumBytes) {
    throw new Error("package member exceeds its fixed size limit");
  }
  return consumeVerifiedMember(packageRoot, member, async (stream) => {
    const chunks = [];

    // retain only the caller's fixed member bound
    for await (const chunk of stream) {
      chunks.push(chunk);
    }
    return Buffer.concat(chunks, member.sizeBytes);
  });
}

// drain one member whose identity is sufficient
async function verifyMemberIdentity(packageRoot, member) {
  await consumeVerifiedMember(packageRoot, member, async (stream) => {
    // hash the complete bounded member without retaining it
    for await (const _chunk of stream) {
      // consume only
    }
  });
}

// validate one canonical packaged database row
function validatePackagedRow(line) {
  const packagedRow = JSON.parse(line);
  requireExactKeys(packagedRow, [
    "body_member_path",
    "content_hash",
    "first_received_at",
    "last_received_at",
    "local_date",
    "payload",
    "record_kind",
    "record_revision_identity",
    "reference_at",
    "revision_count",
    "site_key",
    "source_identity",
    "valid_at",
  ], "packaged export row");

  // require the canonical row bytes emitted by the builder
  if (canonicalJson(packagedRow) !== `${line}\n`) {
    throw new Error("packaged export row is not canonical");
  }
  const { body_member_path: bodyMemberPath, ...rawRow } = packagedRow;
  const validated = validateRow({ ...rawRow, compressed_body_base64: null });

  // bind the optional body path to a rain receipt
  if (bodyMemberPath !== null &&
    !/^members\/bodies\/sha256-[a-f0-9]{64}\.json\.gz$/u.test(bodyMemberPath)) {
    throw new Error("packaged export row body path is invalid");
  }
  if ((validated.record_kind === "rain_receipt" &&
    validated.payload.compressedBytes !== null) !== (bodyMemberPath !== null)) {
    throw new Error("packaged export row body binding is invalid");
  }
  return { ...validated, body_member_path: bodyMemberPath };
}

// validate one edge object and return its stable receipt binding
async function verifyEdgeObject(packageRoot, member, expectedSha256) {
  const objectBytes = await readVerifiedMember(packageRoot, member, 16 * 1024);
  const objectPlaintext = gunzipSync(objectBytes, { maxOutputLength: 512 * 1024 + 1 });
  const object = JSON.parse(objectPlaintext.toString("utf8"));
  requireExactKeys(object, [
    "bundleIdentities",
    "contractVersion",
    "rows",
    "settingsSha256",
    "siteKey",
    "window",
  ], "edge object");

  // bind deterministic canonical object bytes and plaintext identity
  if (object.contractVersion !== EDGE_OBJECT_VERSION ||
    !Array.isArray(object.rows) || object.rows.length < 1 || object.rows.length > 240 ||
    !HASH_PATTERN.test(object.settingsSha256) || object.siteKey !== "ballydidean" ||
    !EDGE_WINDOWS.has(object.window) ||
    objectPlaintext.length > objectBytes.length * MAX_RATIO ||
    !Buffer.from(canonicalJsonValue(object)).equals(objectPlaintext) ||
    !gzipSync(objectPlaintext, { level: 9, mtime: 0 }).equals(objectBytes) ||
    sha256(objectPlaintext) !== expectedSha256) {
    throw new Error("edge object bytes differ");
  }
  const stableRows = object.rows.map((row) => {
    requireObject(row, "edge object row");
    requireObject(row.record, "edge object row record");
    requireObject(row.source, "edge object row source");
    return { record: row.record, source: row.source };
  });
  return {
    rowCount: object.rows.length,
    stableIdentitySha256: sha256(canonicalJsonValue({
      bundleIdentities: object.bundleIdentities,
      rows: stableRows,
      settingsSha256: object.settingsSha256,
      siteKey: object.siteKey,
      window: object.window,
    })),
    window: object.window,
  };
}

// stream-verify one package for bounded local publication
export async function verifyAdjustmentEvaluationPackage(packageRoot) {
  const firstManifest = await readPackageManifest(packageRoot);
  const { manifest, manifestHash } = firstManifest;
  const { membersByPath } = validatePackageMemberDeclarations(packageRoot, manifest);
  const rowsMember = membersByPath.get("members/rows.jsonl.gz");

  // require exactly one canonical row stream
  if (rowsMember === undefined) {
    throw new Error("package row member is missing");
  }
  const verifiedMembers = new Set();
  const verifiedBodies = new Map();
  const sourceIdentities = new Set();
  let canonicalBytes = 0;
  let maxRevisionCount = 0;
  let maxSourceReceiptAt = null;
  let revisedRowCount = 0;
  let rowCount = 0;
  let rowsLastByte = null;

  await consumeVerifiedMember(packageRoot, rowsMember, async (compressedRows) => {
    const decodedRows = new PassThrough();
    const decodedCap = new Transform({
      // cap decoded rows before line accumulation
      transform(chunk, encoding, callback) {
        canonicalBytes += chunk.length;
        rowsLastByte = chunk.length === 0 ? rowsLastByte : chunk[chunk.length - 1];
        if (canonicalBytes > MAX_CANONICAL_BYTES ||
          canonicalBytes > rowsMember.sizeBytes * MAX_RATIO) {
          callback(new Error("package row member exceeds the decompression limit"));
          return;
        }
        callback(null, chunk);
      },
    });
    const decompression = pipeline(compressedRows, createGunzip(), decodedCap, decodedRows);
    void decompression.catch(() => undefined);

    try {
      // validate each row and its body before pulling the next row
      for await (const line of databaseRecordLines(decodedRows)) {
        if (line.length === 0) {
          throw new Error("package row member contains an empty record");
        }
        const row = validatePackagedRow(line);
        rowCount += 1;

        // enforce the reviewed row bound during streaming
        if (rowCount > MAX_ROWS) {
          throw new Error("package row count exceeds its limit");
        }
        sourceIdentities.add(row.source_identity);
        revisedRowCount += row.revision_count > 0 ? 1 : 0;
        maxRevisionCount = Math.max(maxRevisionCount, row.revision_count);
        if (maxSourceReceiptAt === null || row.last_received_at > maxSourceReceiptAt) {
          maxSourceReceiptAt = row.last_received_at;
        }

        // validate each distinct retained body once
        if (row.body_member_path !== null) {
          const bodyMember = membersByPath.get(row.body_member_path);
          const binding = `${row.payload.bodyBytes}:${row.payload.bodySha256}:` +
            `${row.payload.compressedBytes}`;
          if (bodyMember === undefined ||
            bodyMember.sizeBytes !== row.payload.compressedBytes ||
            row.body_member_path !==
              `members/bodies/sha256-${row.payload.bodySha256}.json.gz`) {
            throw new Error("packaged export row body member is missing");
          }
          const priorBinding = verifiedBodies.get(row.body_member_path);
          if (priorBinding !== undefined && priorBinding !== binding) {
            throw new Error("packaged export row body binding differs");
          }

          // verify only the first row bound to shared content
          if (priorBinding === undefined) {
            await consumeVerifiedMember(packageRoot, bodyMember, async (bodyStream) => {
              await validateCompressedBodyStream(
                bodyStream,
                bodyMember.sizeBytes,
                row.payload.bodyBytes,
                row.payload.bodySha256,
              );
            });
            verifiedBodies.set(row.body_member_path, binding);
            verifiedMembers.add(row.body_member_path);
          }
        }
      }
      await decompression;
    } finally {
      decodedRows.destroy();
      decodedCap.destroy();
      await Promise.allSettled([decompression]);
    }
  });
  verifiedMembers.add(rowsMember.path);

  // require the exact row stream and aggregate identities
  if ((canonicalBytes > 0 && rowsLastByte !== 0x0a) ||
    canonicalBytes !== manifest.canonicalStreamBytes ||
    rowCount !== manifest.revisionDiagnostics.rowCount ||
    revisedRowCount !== manifest.revisionDiagnostics.revisedRowCount ||
    maxRevisionCount !== manifest.revisionDiagnostics.maxRevisionCount ||
    maxSourceReceiptAt !== manifest.maxSourceReceiptAt ||
    JSON.stringify([...sourceIdentities].sort()) !== JSON.stringify(manifest.sourceIdentities)) {
    throw new Error("package row diagnostics differ");
  }

  const bodyMembers = manifest.members.filter((member) =>
    member.path.startsWith("members/bodies/"));
  const compressedBodyBytes = bodyMembers.reduce((total, member) => total + member.sizeBytes, 0);

  // bind the complete body member allocation
  if (compressedBodyBytes !== manifest.compressedBodyBytes) {
    throw new Error("package compressed body allocation differs");
  }

  // hash any declared body not referenced by a row
  for (const bodyMember of bodyMembers) {
    if (!verifiedMembers.has(bodyMember.path)) {
      await verifyMemberIdentity(packageRoot, bodyMember);
      verifiedMembers.add(bodyMember.path);
    }
  }

  const objectMembers = manifest.members.filter((member) =>
    member.path.startsWith("members/edge/objects/"));
  const receiptMembers = manifest.members.filter((member) =>
    member.path.startsWith("members/edge/receipts/"));

  // bind edge member counts before pair validation
  if (objectMembers.length !== manifest.edgeEvidence.objectCount ||
    receiptMembers.length !== manifest.edgeEvidence.receiptCount) {
    throw new Error("edge evidence member counts differ");
  }
  const validatedObjects = new Map();
  const watermarkPairs = [];

  // validate each receipt and its referenced object pair
  for (const receiptMember of receiptMembers) {
    const receiptBytes = await readVerifiedMember(packageRoot, receiptMember, 16 * 1024);
    const receipt = JSON.parse(receiptBytes.toString("utf8"));
    requireExactKeys(receipt, [
      "availability",
      "contractVersion",
      "edgeReceiptIdentitySha256",
      "firstEdgeCommittedAt",
      "objectSha256",
      "siteKey",
      "window",
    ], "edge receipt");
    requireExactKeys(receipt.availability, [
      "rowTimestampIndexes",
      "timestamps",
    ], "edge receipt availability");
    const receiptIdentity = basename(receiptMember.path).slice(7, -5);
    const objectPath = `members/edge/objects/sha256-${receipt.objectSha256}.json.gz`;
    const objectMember = membersByPath.get(objectPath);

    // bind canonical receipt bytes and pair identity
    if (receipt.contractVersion !== EDGE_RECEIPT_VERSION ||
      receipt.edgeReceiptIdentitySha256 !== receiptIdentity ||
      !HASH_PATTERN.test(receipt.objectSha256) || receipt.siteKey !== "ballydidean" ||
      !EDGE_WINDOWS.has(receipt.window) ||
      !Number.isFinite(Date.parse(receipt.firstEdgeCommittedAt)) ||
      !Array.isArray(receipt.availability.timestamps) ||
      !Array.isArray(receipt.availability.rowTimestampIndexes) ||
      receipt.availability.timestamps.some((value) => !Number.isFinite(Date.parse(value))) ||
      !Buffer.from(canonicalJson(receipt)).equals(receiptBytes) ||
      objectMember === undefined) {
      throw new Error("edge receipt identity is invalid");
    }
    let objectIdentity;

    // validate each shared edge object once
    if (!validatedObjects.has(objectPath)) {
      objectIdentity = await verifyEdgeObject(
        packageRoot,
        objectMember,
        receipt.objectSha256,
      );
      validatedObjects.set(objectPath, objectIdentity);
      verifiedMembers.add(objectPath);
    } else {
      objectIdentity = validatedObjects.get(objectPath);
    }

    // bind stable identity, window, and every availability index
    if (receipt.edgeReceiptIdentitySha256 !== objectIdentity.stableIdentitySha256 ||
      receipt.window !== objectIdentity.window ||
      receipt.availability.rowTimestampIndexes.length !== objectIdentity.rowCount ||
      receipt.availability.rowTimestampIndexes.some((index) =>
        !Number.isSafeInteger(index) || index < 0 ||
        index >= receipt.availability.timestamps.length)) {
      throw new Error("edge receipt availability is invalid");
    }
    watermarkPairs.push({
      edgeReceiptIdentitySha256: receipt.edgeReceiptIdentitySha256,
      objectSha256: receipt.objectSha256,
    });
    verifiedMembers.add(receiptMember.path);
  }

  // rebind the portable freeze watermark from ordered receipt pairs
  if (sha256(canonicalJsonValue(watermarkPairs)) !== manifest.edgeEvidence.watermarkSha256) {
    throw new Error("edge evidence watermark differs");
  }

  // validate any declared object not referenced by a receipt
  for (const objectMember of objectMembers) {
    if (!verifiedMembers.has(objectMember.path)) {
      const expectedSha256 = basename(objectMember.path).slice(7, -8);
      await verifyEdgeObject(packageRoot, objectMember, expectedSha256);
      verifiedMembers.add(objectMember.path);
    }
  }

  // require every declared member to be covered by strict verification
  if (verifiedMembers.size !== manifest.members.length) {
    throw new Error("package contains an unverified member");
  }
  const finalManifest = await readPackageManifest(packageRoot);

  // reject manifest replacement during member verification
  if (finalManifest.manifestHash !== manifestHash ||
    !finalManifest.manifestBytes.equals(firstManifest.manifestBytes)) {
    throw new Error("package manifest changed during verification");
  }
  return { manifest, manifestSha256: manifestHash };
}

// verify and decode one local package directory
export async function loadVerifiedAdjustmentEvaluationPackage(packageRoot) {
  const strict = await verifyAdjustmentEvaluationPackage(packageRoot);
  const { manifest, manifestSha256: manifestHash } = strict;
  const membersByPath = new Map(manifest.members.map((member) => [member.path, member]));
  const rowsMember = manifest.members.find((member) => member.path === "members/rows.jsonl.gz");
  if (rowsMember === undefined) {
    throw new Error("package row member is missing");
  }
  const rowsBytes = await readFile(join(packageRoot, rowsMember.path));

  // rebind the materialized row bytes to the strict manifest
  if (rowsBytes.length !== rowsMember.sizeBytes || sha256(rowsBytes) !== rowsMember.sha256) {
    throw new Error("package member bytes differ");
  }
  const decodedRows = gunzipSync(rowsBytes, {
    maxOutputLength: MAX_CANONICAL_BYTES + 1,
  }).toString("utf8");
  if (Buffer.byteLength(decodedRows) > rowsMember.sizeBytes * MAX_RATIO) {
    throw new Error("package row member exceeds the decompression ratio");
  }
  const rows = decodedRows === ""
    ? []
    : decodedRows.trimEnd().split("\n").map(validatePackagedRow);
  if (rows.length !== manifest.revisionDiagnostics.rowCount) {
    throw new Error("package row count differs");
  }

  const bodies = Object.fromEntries(
    manifest.members
      .filter((member) => member.path.startsWith("members/bodies/"))
      .map((member) => {
        const bytes = readFileSync(join(packageRoot, member.path));

        // rebind each materialized body to the strict manifest
        if (bytes.length !== member.sizeBytes || sha256(bytes) !== member.sha256) {
          throw new Error("package member bytes differ");
        }
        return [member.path, bytes];
      }),
  );
  const edgeEvidence = Object.fromEntries(
    manifest.members
      .filter((member) => member.path.startsWith("members/edge/"))
      .map((member) => {
        const bytes = readFileSync(join(packageRoot, member.path));

        // rebind each materialized edge member to the strict manifest
        if (bytes.length !== member.sizeBytes || sha256(bytes) !== member.sha256) {
          throw new Error("package member bytes differ");
        }
        return [member.path, bytes];
      }),
  );
  if (Object.keys(edgeEvidence).filter((path) => path.includes("/objects/")).length !==
      manifest.edgeEvidence.objectCount ||
    Object.keys(edgeEvidence).filter((path) => path.includes("/receipts/")).length !==
      manifest.edgeEvidence.receiptCount) {
    throw new Error("edge evidence member counts differ");
  }

  // recheck every row-to-body binding against plaintext identity and ratio
  for (const row of rows) {
    if (row.body_member_path === null) {
      continue;
    }
    const body = bodies[row.body_member_path];
    if (body === undefined) {
      throw new Error("packaged export row body member is missing");
    }
    await validateCompressedBody(body, row.payload.bodyBytes, row.payload.bodySha256);
  }

  // recheck every identity after materialization to close mutation races
  const finalStrict = await verifyAdjustmentEvaluationPackage(packageRoot);
  if (finalStrict.manifestSha256 !== manifestHash ||
    membersByPath.size !== manifest.members.length) {
    throw new Error("package identity changed during loading");
  }
  return { bodies, edgeEvidence, manifest, manifestSha256: manifestHash, rows };
}

const V2_STREAM_MAGIC = Buffer.from("weather-adjustment-evaluation-envelope/v2\n");
const V2_ENVELOPE_VERSION = "adjustment-evaluation-export-envelope/v2";
const V2_AVAILABILITY_ENVELOPE_VERSION =
  "adjustment-confirmation-availability-envelope/v2";
const V2_AUTHORIZATION_ENVELOPE_VERSION =
  "adjustment-confirmation-export-authorization-envelope/v2";
const V2_MAXIMUM_HEADER_BYTES = 64 * 1024;
const V2_MAXIMUM_DATABASE_RESPONSE_BYTES = 768 * 1024;
const V2_UTC_MILLISECOND_PATTERN =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;
const V2_DUE_KEY_PATTERN =
  /^capture\/\d{4}-\d{2}-\d{2}T(?:00|06|12|18):35:00\.000Z$/u;
const V2_FAMILIES = new Set(["rain", "temperature", "wind"]);

// require the complete additive maintenance ledger
function validateCompleteMaintenanceDatabaseManifest(value) {
  const manifest = validateDatabaseManifest(value);

  // accept only the complete reviewed function graphs and their exact bytes
  const complete = EXPECTED_MIGRATION_LEDGERS.slice(1).some(
    // exclude legacy display-only and unknown additive ledgers
    (ledger) => JSON.stringify(manifest.migration_names) === JSON.stringify(ledger.names) &&
      JSON.stringify(manifest.migration_checksums) === JSON.stringify(ledger.checksums) &&
      manifest.migration_history_sha256 === ledger.historySha256,
  );
  if (!complete) {
    throw new Error("maintenance database ledger is not exact");
  }
  return manifest;
}

// require the unchanged eighteen-migration bridge ledger
export function validateAdjustmentV13CompatibilityDatabaseManifest(value) {
  const manifest = validateDatabaseManifest(value);
  // accept only the exact deployed maintenance-v2 boundary
  if (JSON.stringify(manifest.migration_names) !==
      JSON.stringify(EXPECTED_COMPLETE_MIGRATION_NAMES) ||
    JSON.stringify(manifest.migration_checksums) !==
      JSON.stringify(EXPECTED_COMPLETE_MIGRATION_CHECKSUMS) ||
    manifest.migration_history_sha256 !== EXPECTED_COMPLETE_MIGRATION_HISTORY_SHA256) {
    throw new Error("v14 compatibility bridge requires the exact complete 0018 ledger");
  }
  return manifest;
}

// require the complete frozen serving-frontier schema for the inactive v14 bridge
export function validateAdjustmentV14DatabaseManifest(value) {
  const manifest = validateCompleteMaintenanceDatabaseManifest(value);
  // legacy or partial additive schemas cannot authorize the new deployment
  if (manifest.migration_names.length !== 20 ||
    manifest.migration_names.at(-1) !== "0020_adjustment_revision_frontier.sql") {
    throw new Error("inert v14 requires the exact complete 0020 ledger");
  }
  return manifest;
}

// require one exact live rolling, catalog or index ledger
export function validateAdjustmentV14RollingDatabaseManifest(value) {
  const manifest = validateCompleteMaintenanceDatabaseManifest(value);
  // admit only the reviewed additive live schemas
  if (![EXPECTED_ROLLING_MIGRATION_HISTORY_SHA256,
    EXPECTED_CATALOG_MIGRATION_HISTORY_SHA256,
    EXPECTED_INDEX_MIGRATION_HISTORY_SHA256].includes(manifest.migration_history_sha256)) {
    throw new Error("rolling v14 requires an exact live ledger");
  }
  return manifest;
}

// require the exact retained 0021 historical handoff ledger
function validateAdjustmentV14RollingActivationDatabaseManifest(value) {
  const manifest = validateCompleteMaintenanceDatabaseManifest(value);
  // reject the later normal-release catalog extension
  if (manifest.migration_history_sha256 !== EXPECTED_ROLLING_MIGRATION_HISTORY_SHA256) {
    throw new Error("rolling v14 requires the exact complete 0021 ledger");
  }
  return manifest;
}

// require the exact indexed catalog ledger for a normal release
export function validateAdjustmentV15CatalogDatabaseManifest(value) {
  const manifest = validateCompleteMaintenanceDatabaseManifest(value);
  // reject the retained capture ledger and every later suffix
  if (manifest.migration_history_sha256 !== EXPECTED_INDEX_MIGRATION_HISTORY_SHA256) {
    throw new Error("normal v15 requires the exact complete 0023 ledger");
  }
  return manifest;
}

// project only a genuine metadata-only database transaction into the closed ledger response
export function projectAdjustmentMaintenanceDatabaseLedgerV3(value) {
  requireExactKeys(requireObject(value, "database ledger envelope"),
    ["databaseManifest", "payload", "transaction"], "database ledger envelope");
  const databaseManifest = validateAdjustmentV14RollingDatabaseManifest(value.databaseManifest);
  const transaction = validateV2ReadOnlyTransaction(value.transaction);
  // private values and function results cannot masquerade as ledger metadata
  if (value.payload !== null) {
    throw new Error("database ledger envelope payload must be null");
  }
  return {
    contractVersion: "adjustment-database-ledger/v3",
    databaseManifest,
    snapshotAt: new Date(transaction.created_at_utc).toISOString(),
  };
}

const ROLLING_SCHEDULE_SHA256 =
  "7c17f5d1a8e8249cd0aa4820638169e51f6edb3433017f50ab4c959e44c62f1f";
const ROLLING_ZONE = "America/Los_Angeles";

// independently rederive the root-authorized bootstrap inside the verified control closure
export function validateAdjustmentRegistrationScheduleBootstrapV3(value, epochWitness) {
  const witness = validateAdjustmentRevisionCaptureEpochWitness(epochWitness);
  requireExactKeys(requireObject(value, "registration schedule bootstrap"), [
    "bootstrapSha256", "contractVersion", "epochAt", "epochWitnessSha256",
    "firstCompleteLocalDate", "horizonEndAt", "scheduleContractSha256",
  ], "registration schedule bootstrap");
  const dateFormatter = new Intl.DateTimeFormat("en-CA", {
    day: "2-digit", month: "2-digit", timeZone: ROLLING_ZONE, year: "numeric",
  });
  const firstCompleteLocalDate = new Date(Date.parse(
    `${dateFormatter.format(new Date(witness.epochAt))}T00:00:00.000Z`,
  ) + 24 * 60 * 60 * 1_000).toISOString().slice(0, 10);
  // include the actual annual development interval before the unchanged future confirmation
  const finalDate = new Date(Date.parse(`${firstCompleteLocalDate}T00:00:00.000Z`) +
    1_053 * 24 * 60 * 60 * 1_000).toISOString().slice(0, 10);
  const wallFormatter = new Intl.DateTimeFormat("en-US", {
    day: "2-digit", hour: "2-digit", hourCycle: "h23", minute: "2-digit",
    month: "2-digit", second: "2-digit", timeZone: ROLLING_ZONE, year: "numeric",
  });
  const target = Date.parse(`${finalDate}T00:00:00.000Z`);
  let guess = target + 8 * 60 * 60 * 1_000;
  // resolve local midnight without assuming a fixed daylight-saving offset
  for (let iteration = 0; iteration < 3; iteration += 1) {
    const parts = Object.fromEntries(wallFormatter.formatToParts(new Date(guess)).map(
      // retain the named wall-clock components
      (part) => [part.type, part.value],
    ));
    guess += target - Date.UTC(Number(parts.year), Number(parts.month) - 1,
      Number(parts.day), Number(parts.hour), Number(parts.minute), Number(parts.second));
  }
  const unsigned = {
    contractVersion: "adjustment-registration-schedule-bootstrap/v3",
    epochAt: witness.epochAt,
    epochWitnessSha256: witness.witnessSha256,
    firstCompleteLocalDate,
    horizonEndAt: new Date(guess).toISOString(),
    scheduleContractSha256: ROLLING_SCHEDULE_SHA256,
  };
  const expected = { ...unsigned, bootstrapSha256: sha256(Buffer.from(canonicalJson(unsigned))) };
  // require the current full rolling schema and every independently derived byte
  if (witness.databaseMigrationHistorySha256 !== EXPECTED_ROLLING_MIGRATION_HISTORY_SHA256 ||
    canonicalJson(value) !== canonicalJson(expected)) {
    throw new Error("registration schedule bootstrap differs from retained epoch");
  }
  return value;
}

const OWNER_CONFIRMATION_FAMILIES = new Map([
  ["temperature", "temperature-delayed-mos/v1"],
  ["wind", "wind-robust-hierarchical-median/v1"],
  ["rain", "rain-hurdle-wind-occurrence-amount/v1"],
]);
const OWNER_CONFIRMATION_CONTEXT_KEYS = [
  "accessState", "actionIdentitySha256", "actionState", "candidateKind",
  "candidateReportSha256", "candidateSha256", "cohortLineageSha256",
  "contractVersion", "family", "firstTargetAt", "gateManifestSha256",
  "inputHeadSha256", "intervalEndExclusiveLocalDate", "intervalStartLocalDate",
  "registrationSha256", "reservedKeySha256", "sourceLineageSha256",
  "terminalAccessAt",
];
// freeze preregistration context across the terminal transition
const OWNER_CONFIRMATION_IMMUTABLE_KEYS = [
  "candidateKind", "candidateSha256", "cohortLineageSha256", "contractVersion", "family",
  "firstTargetAt", "gateManifestSha256", "inputHeadSha256", "intervalEndExclusiveLocalDate",
  "intervalStartLocalDate", "registrationSha256", "reservedKeySha256", "sourceLineageSha256",
  "terminalAccessAt",
];

// validate one pre-value owner access request without deriving database authority
export function validateAdjustmentConfirmationAccessBurnRequestV3(value) {
  requireExactKeys(requireObject(value, "confirmation access burn request"), [
    "acknowledgement", "archive", "confirmationRegistration", "contractVersion",
    "epochWitnessSha256", "journalHeadSha256", "localBurn", "metadata",
    "shadowRegistration", "sourceCommit",
  ], "confirmation access burn request");
  if (value.contractVersion !== "adjustment-confirmation-access-burn-request/v3") {
    throw new Error("confirmation access burn contract differs");
  }
  const acknowledgement = validateAdjustmentRevisionColdCustodyAcknowledgementV2(
    value.acknowledgement,
  );
  const registration = validateOwnerConfirmationRegistration(value.confirmationRegistration);
  const burn = validateOwnerLocalBurn(value.localBurn, registration);
  const shadow = validateOwnerShadowRegistration(value.shadowRegistration);
  requireExactKeys(requireObject(value.archive, "confirmation access archive"), [
    "eligiblePredictionSetSha256", "fullGraphVerifiedAt", "graphManifestSha256",
  ], "confirmation access archive");
  requireExactKeys(requireObject(value.metadata, "confirmation access metadata"), [
    "finalizedPredictionCount", "generation", "lastAnchorSha256", "rootSha256", "throughAt",
  ], "confirmation access metadata");
  for (const identity of [
    value.archive.eligiblePredictionSetSha256, value.archive.graphManifestSha256,
    value.epochWitnessSha256, value.journalHeadSha256, value.metadata.rootSha256,
  ]) {
    requireOwnerHash(identity, "confirmation access identity");
  }
  validateV2Instant(value.archive.fullGraphVerifiedAt, "confirmation graph verified clock");
  validateOwnerMetadata(value.metadata);
  // crossbind the two registration domains without equating their distinct hashes
  if (registration.family !== shadow.family || registration.candidateSha256 !== shadow.candidateSha256 ||
    registration.reservedKeySha256 !== shadow.reservedKeySha256 ||
    burn.expectedKeySetSha256 !== shadow.reservedKeySha256 ||
    burn.targetCutoffAt !== shadow.targetCutoffAt ||
    registration.firstTargetAt !== shadow.intervalStartAt ||
    registration.terminalAccessAt !== shadow.terminalAt ||
    registration.intervalStartLocalDate !== ownerLocalDate(shadow.intervalStartAt) ||
    registration.intervalEndExclusiveLocalDate !== ownerLocalDate(shadow.intervalEndAt) ||
    burn.gateManifestSha256 !== registration.gateManifestSha256 ||
    Date.parse(value.archive.fullGraphVerifiedAt) < Date.parse(acknowledgement.acknowledgedAt) ||
    !/^[a-f0-9]{40}$/u.test(value.sourceCommit)) {
    throw new Error("confirmation access registration binding differs");
  }
  return value;
}

// validate one terminal owner record request after genuine finalization
export function validateAdjustmentShadowTerminalRecordRequestV3(value) {
  requireExactKeys(requireObject(value, "terminal record request"), [
    "action", "actionReceipt", "confirmationRegistration", "contractVersion",
    "finalizationProof", "localResult", "nativeAccess", "previousTombstone",
    "shadowRegistration", "sourceCommit", "terminalGraphManifestSha256",
    "terminalGraphVerifiedAt",
  ], "terminal record request");
  if (value.contractVersion !== "adjustment-shadow-terminal-record-request/v3") {
    throw new Error("terminal record contract differs");
  }
  const proof = validateAdjustmentMaintenanceFinalizationProofV3(value.finalizationProof);
  const registration = validateOwnerConfirmationRegistration(value.confirmationRegistration);
  const shadow = validateOwnerShadowRegistration(value.shadowRegistration);
  const result = validateOwnerLocalResult(value.localResult);
  const nativeAccess = validateOwnerNativeAccess(value.nativeAccess);
  const receipt = validateOwnerTerminalActionReceipt(value.actionReceipt);
  requireOwnerHash(value.terminalGraphManifestSha256, "terminal graph manifest");
  validateV2Instant(value.terminalGraphVerifiedAt, "terminal graph verified clock");
  // use the retained no-action completion clock when no family transaction exists
  const receiptCompletedAt = receipt.contractVersion ===
    "adjustment-terminal-no-action-receipt/v3" ? receipt.completedAt : proof.finalizedAt;
  if (!/^[a-f0-9]{40}$/u.test(value.sourceCommit) ||
    OWNER_CONFIRMATION_IMMUTABLE_KEYS.some((key) => result[key] !== registration[key]) ||
    nativeAccess.registrationSha256 !== shadow.registrationSha256 ||
    registration.family !== shadow.family || result.family !== shadow.family ||
    registration.candidateSha256 !== shadow.candidateSha256 ||
    result.candidateSha256 !== shadow.candidateSha256 ||
    registration.reservedKeySha256 !== shadow.reservedKeySha256 ||
    result.reservedKeySha256 !== shadow.reservedKeySha256 ||
    nativeAccess.expectedKeySetSha256 !== shadow.reservedKeySha256 ||
    nativeAccess.gateManifestSha256 !== registration.gateManifestSha256 ||
    nativeAccess.targetCutoffAt !== shadow.targetCutoffAt ||
    proof.family !== shadow.family || proof.sourceCommit !== value.sourceCommit ||
    proof.confirmationAccessSha256 !== nativeAccess.accessSha256 ||
    proof.fullMemberRootSha256 !== result.fullMemberRootSha256 ||
    proof.policyReportSha256 !== result.candidateReportSha256 ||
    receipt.actionSha256 !== proof.actionSha256 ||
    Date.parse(nativeAccess.accessedAt) < Date.parse(shadow.terminalAt) ||
    Date.parse(proof.fullGraphVerifiedAt) < Date.parse(nativeAccess.accessedAt) ||
    Date.parse(receiptCompletedAt) < Date.parse(proof.finalizedAt) ||
    Date.parse(value.terminalGraphVerifiedAt) < Date.parse(receiptCompletedAt)) {
    throw new Error("terminal record identity differs");
  }
  // promoted results carry exact immutable action bytes; no-action results carry none
  if (result.disposition === "promoted") {
    if (receipt.actionSha256 !== result.actionIdentitySha256 ||
      receipt.contractVersion !== "adjustment-family-release-status/v1" ||
      receipt.family !== value.action?.family ||
      receipt.fencingToken !== value.action?.fencingToken) {
      throw new Error("terminal promoted action differs");
    }
    validateOwnerTerminalModelAction(value.action, receipt.actionSha256, result);
  } else if (value.action !== null || receipt.contractVersion !==
    "adjustment-terminal-no-action-receipt/v3" ||
    receipt.registrationSha256 !== shadow.registrationSha256 ||
    receipt.disposition !== result.disposition ||
    receipt.fullMemberRootSha256 !== result.fullMemberRootSha256 ||
    receipt.policyReportSha256 !== result.candidateReportSha256) {
    throw new Error("terminal no-action receipt differs");
  }
  validateOwnerPreviousTombstone(value.previousTombstone);
  return value;
}

// validate one exact terminal tombstone retirement request
export function validateAdjustmentShadowTerminalRetirementRequestV3(value) {
  requireExactKeys(requireObject(value, "terminal retirement request"), [
    "contractVersion", "finalizationProof", "registrationSha256", "terminalRecord",
    "terminalTombstone",
  ], "terminal retirement request");
  if (value.contractVersion !== "adjustment-shadow-terminal-retirement-request/v3") {
    throw new Error("terminal retirement contract differs");
  }
  requireOwnerHash(value.registrationSha256, "retirement registration");
  const proof = validateAdjustmentMaintenanceFinalizationProofV3(value.finalizationProof);
  const record = validateOwnerTerminalRecord(value.terminalRecord);
  const tombstone = validateOwnerPreviousTombstone(value.terminalTombstone, false);
  // release only the registration represented by the finalized action and reconciliation
  if (record.registrationSha256 !== value.registrationSha256 ||
    tombstone.registrationSha256 !== value.registrationSha256 ||
    tombstone.reconciliationSha256 !== record.reconciliationSha256 ||
    proof.actionSha256 !== record.actionSha256 ||
    proof.confirmationAccessSha256 !== record.accessSha256 ||
    proof.family !== record.family || proof.fullMemberRootSha256 !==
      record.terminalMemberSha256 ||
    proof.transferredAnchorSha256 !== record.maintenanceAnchorSha256) {
    throw new Error("terminal retirement identity differs");
  }
  return value;
}

// validate one unsupported-only terminal record without admitting model mutation
export function validateAdjustmentShadowUnsupportedTerminalRecordRequestV1(value) {
  requireExactKeys(requireObject(value, "unsupported terminal record request"), [
    "actionReceipt", "confirmationRegistration", "contractVersion", "localResult",
    "nativeAccess", "previousTombstone", "shadowRegistration", "sourceCommit",
    "terminalGraphManifestSha256", "terminalGraphVerifiedAt", "unsupportedProof",
  ], "unsupported terminal record request");
  if (value.contractVersion !== "adjustment-shadow-unsupported-terminal-record-request/v1") {
    throw new Error("unsupported terminal record contract differs");
  }
  const proof = validateAdjustmentUnsupportedTerminalProofV1(value.unsupportedProof);
  const registration = validateOwnerConfirmationRegistration(value.confirmationRegistration);
  const shadow = validateOwnerShadowRegistration(value.shadowRegistration);
  const result = validateOwnerLocalResult(value.localResult);
  const nativeAccess = validateOwnerNativeAccess(value.nativeAccess);
  const receipt = validateOwnerTerminalActionReceipt(value.actionReceipt);
  requireOwnerHash(value.terminalGraphManifestSha256, "unsupported terminal graph manifest");
  validateV2Instant(value.terminalGraphVerifiedAt, "unsupported terminal graph verified clock");
  const due = /^confirmation\/(temperature|wind|rain)\/([a-f0-9]{64})$/u.exec(proof.dueKey);

  // crossbind every genuine no-action domain while preserving distinct registration hashes
  if (due === null || due[1] !== shadow.family || due[2] !== shadow.candidateSha256 ||
    !/^[a-f0-9]{40}$/u.test(value.sourceCommit) ||
    OWNER_CONFIRMATION_IMMUTABLE_KEYS.some((key) => result[key] !== registration[key]) ||
    result.disposition !== "support_failed" || result.actionIdentitySha256 !== null ||
    nativeAccess.registrationSha256 !== shadow.registrationSha256 ||
    registration.family !== shadow.family || result.family !== shadow.family ||
    registration.candidateSha256 !== shadow.candidateSha256 ||
    result.candidateSha256 !== shadow.candidateSha256 ||
    registration.reservedKeySha256 !== shadow.reservedKeySha256 ||
    result.reservedKeySha256 !== shadow.reservedKeySha256 ||
    nativeAccess.expectedKeySetSha256 !== shadow.reservedKeySha256 ||
    nativeAccess.gateManifestSha256 !== registration.gateManifestSha256 ||
    nativeAccess.targetCutoffAt !== shadow.targetCutoffAt ||
    proof.family !== shadow.family || proof.sourceCommit !== value.sourceCommit ||
    proof.registrationSha256 !== shadow.registrationSha256 ||
    proof.confirmationAccessSha256 !== nativeAccess.accessSha256 ||
    proof.fullMemberRootSha256 !== result.fullMemberRootSha256 ||
    proof.policyReportSha256 !== result.candidateReportSha256 ||
    proof.graphManifestSha256 !== value.terminalGraphManifestSha256 ||
    receipt.actionSha256 !== proof.actionSha256 ||
    receipt.contractVersion !== "adjustment-terminal-no-action-receipt/v3" ||
    receipt.disposition !== "support_failed" ||
    receipt.registrationSha256 !== shadow.registrationSha256 ||
    receipt.fullMemberRootSha256 !== proof.fullMemberRootSha256 ||
    receipt.policyReportSha256 !== proof.policyReportSha256 ||
    Date.parse(nativeAccess.accessedAt) < Date.parse(shadow.terminalAt) ||
    Date.parse(proof.fullGraphVerifiedAt) < Date.parse(nativeAccess.accessedAt) ||
    Date.parse(receipt.completedAt) < Date.parse(proof.finalizedAt) ||
    Date.parse(value.terminalGraphVerifiedAt) < Date.parse(receipt.completedAt)) {
    throw new Error("unsupported terminal record identity differs");
  }
  validateOwnerPreviousTombstone(value.previousTombstone);
  return value;
}

// validate one unsupported terminal retirement against its retained proof
export function validateAdjustmentShadowUnsupportedTerminalRetirementRequestV1(value) {
  requireExactKeys(requireObject(value, "unsupported terminal retirement request"), [
    "contractVersion", "registrationSha256", "terminalRecord", "terminalTombstone",
    "unsupportedProof",
  ], "unsupported terminal retirement request");
  if (value.contractVersion !== "adjustment-shadow-unsupported-terminal-retirement-request/v1") {
    throw new Error("unsupported terminal retirement contract differs");
  }
  requireOwnerHash(value.registrationSha256, "unsupported retirement registration");
  const proof = validateAdjustmentUnsupportedTerminalProofV1(value.unsupportedProof);
  const record = validateOwnerTerminalRecord(value.terminalRecord);
  const tombstone = validateOwnerPreviousTombstone(value.terminalTombstone, false);
  const proofSha256 = sha256(Buffer.from(canonicalJson(proof)));

  // release only the exact failed member reconciled under the installed unsupported proof
  if (record.registrationSha256 !== value.registrationSha256 ||
    proof.registrationSha256 !== value.registrationSha256 ||
    tombstone.registrationSha256 !== value.registrationSha256 ||
    tombstone.reconciliationSha256 !== record.reconciliationSha256 ||
    proof.actionSha256 !== record.actionSha256 ||
    proof.confirmationAccessSha256 !== record.accessSha256 ||
    proof.family !== record.family || proof.fullMemberRootSha256 !==
      record.terminalMemberSha256 || proofSha256 !== record.maintenanceAnchorSha256) {
    throw new Error("unsupported terminal retirement identity differs");
  }
  return value;
}

// validate one exact local preregistration document
function validateOwnerConfirmationRegistration(value) {
  requireExactKeys(requireObject(value, "local confirmation registration"),
    OWNER_CONFIRMATION_CONTEXT_KEYS, "local confirmation registration");
  const familyKind = OWNER_CONFIRMATION_FAMILIES.get(value.family);
  for (const identity of [
    value.candidateSha256, value.cohortLineageSha256, value.gateManifestSha256,
    value.inputHeadSha256, value.registrationSha256, value.reservedKeySha256,
    value.sourceLineageSha256,
  ]) requireOwnerHash(identity, "local registration identity");
  for (const clock of [value.firstTargetAt, value.terminalAccessAt]) {
    validateV2Instant(clock, "local registration clock");
  }
  const registrationPayload = Object.fromEntries([
    "contractVersion", "family", "candidateKind", "candidateSha256", "cohortLineageSha256",
    "sourceLineageSha256", "reservedKeySha256", "intervalStartLocalDate",
    "intervalEndExclusiveLocalDate", "firstTargetAt", "terminalAccessAt", "gateManifestSha256",
    "inputHeadSha256",
  ].map((key) => [key, value[key]]));
  if (value.contractVersion !== "forecast-adjustment-lifecycle-ledger/v2" ||
    value.candidateKind !== familyKind || value.accessState !== "registered" ||
    value.actionState !== "none" || value.candidateReportSha256 !== null ||
    value.actionIdentitySha256 !== null || !/^20\d{2}-\d{2}-\d{2}$/u.test(value.intervalStartLocalDate) ||
    !/^20\d{2}-\d{2}-\d{2}$/u.test(value.intervalEndExclusiveLocalDate) ||
    sha256(Buffer.from(canonicalJson(registrationPayload))) !== value.registrationSha256) {
    throw new Error("local confirmation registration differs");
  }
  return value;
}

// validate the local burn identity independently of the owner-native access hash
function validateOwnerLocalBurn(value, registration) {
  requireExactKeys(requireObject(value, "local confirmation burn"), [
    ...OWNER_CONFIRMATION_CONTEXT_KEYS, "accessSha256", "accessedAt", "expectedKeySetSha256",
    "revisionCatalogWatermarkSha256", "targetComparatorSnapshotRootSha256", "targetCutoffAt",
  ], "local confirmation burn");
  for (const identity of [value.accessSha256, value.expectedKeySetSha256,
    value.revisionCatalogWatermarkSha256, value.targetComparatorSnapshotRootSha256]) {
    requireOwnerHash(identity, "local burn identity");
  }
  for (const clock of [value.accessedAt, value.targetCutoffAt]) {
    validateV2Instant(clock, "local burn clock");
  }
  const unsigned = { ...value };
  delete unsigned.accessSha256;
  const immutableKeys = ["contractVersion", "family", "candidateKind", "candidateSha256",
    "cohortLineageSha256", "sourceLineageSha256", "reservedKeySha256",
    "intervalStartLocalDate", "intervalEndExclusiveLocalDate", "firstTargetAt",
    "terminalAccessAt", "gateManifestSha256", "inputHeadSha256", "registrationSha256"];
  // preserve every preregistered identity while advancing only access state
  if (immutableKeys.some((key) => value[key] !== registration[key]) ||
    value.accessState !== "burned" || value.actionState !== "none" ||
    value.candidateReportSha256 !== null || value.actionIdentitySha256 !== null ||
    sha256(Buffer.from(canonicalJson(unsigned))) !== value.accessSha256) {
    throw new Error("local confirmation burn differs");
  }
  return value;
}

// validate the database registration identity without requiring the retained witness document
function validateOwnerShadowRegistration(value) {
  const identityKeys = ["siteKey", "family", "candidateSha256", "artifactSha256", "policySha256",
    "cohortSha256", "reservedKeySha256", "sourceSha256", "epochWitnessSha256",
    "scheduleContractSha256", "predecessorRegistrationSha256", "intervalStartAt", "intervalEndAt",
    "targetCutoffAt", "terminalAt"];
  requireExactKeys(requireObject(value, "owner shadow registration"),
    [...identityKeys, "registrationSha256"], "owner shadow registration");
  for (const identity of ["candidateSha256", "artifactSha256", "policySha256", "cohortSha256",
    "reservedKeySha256", "sourceSha256", "epochWitnessSha256", "scheduleContractSha256",
    "registrationSha256"].map((key) => value[key])) requireOwnerHash(identity, "shadow identity");
  if (value.predecessorRegistrationSha256 !== null) {
    requireOwnerHash(value.predecessorRegistrationSha256, "shadow predecessor");
  }
  for (const key of ["intervalStartAt", "intervalEndAt", "targetCutoffAt", "terminalAt"]) {
    validateV2Instant(value[key], "shadow registration clock");
  }
  const preimage = ["adjustment-shadow-registration/v3", ...identityKeys.map(
    // retain the frozen nullable predecessor token
    (key) => value[key] ?? "none",
  )].join("\n") + "\n";
  if (value.siteKey !== "ballydidean" || !OWNER_CONFIRMATION_FAMILIES.has(value.family) ||
    value.targetCutoffAt !== value.terminalAt || sha256(Buffer.from(preimage)) !== value.registrationSha256) {
    throw new Error("owner shadow registration differs");
  }
  return value;
}

// validate one compact database metadata head
function validateOwnerMetadata(value) {
  if (!Number.isSafeInteger(value.generation) || value.generation < 1 ||
    !Number.isSafeInteger(value.finalizedPredictionCount) || value.finalizedPredictionCount < 1 ||
    value.lastAnchorSha256 === null || value.throughAt === null) {
    throw new Error("confirmation access metadata differs");
  }
  requireOwnerHash(value.lastAnchorSha256, "metadata anchor");
  validateV2Instant(value.throughAt, "metadata through clock");
  return value;
}

// validate one local terminal evaluation projection
function validateOwnerLocalResult(value) {
  requireExactKeys(requireObject(value, "local terminal result"), [
    ...OWNER_CONFIRMATION_CONTEXT_KEYS, "disposition", "fullMemberRootSha256",
    "nextConfirmationEligibleAt",
  ], "local terminal result");
  for (const identity of [value.registrationSha256, value.candidateReportSha256,
    value.fullMemberRootSha256]) requireOwnerHash(identity, "local terminal identity");
  validateV2Instant(value.nextConfirmationEligibleAt, "terminal eligibility clock");
  if (!new Set(["promoted", "rejected", "resource_refused", "support_failed"]).has(value.disposition) ||
    value.accessState !== "opened" || value.actionState !==
      (value.disposition === "promoted" ? "action_pending" : "terminal_no_action") ||
    (value.disposition === "promoted") !== HASH_PATTERN.test(value.actionIdentitySha256 ?? "")) {
    throw new Error("local terminal result differs");
  }
  return value;
}

// validate the actual owner-native access document retained in the full graph
function validateOwnerNativeAccess(value) {
  const keys = ["accessSha256", "accessedAt", "eligiblePredictionSetSha256",
    "expectedKeySetSha256", "gateManifestSha256", "journalHeadSha256",
    "maintenanceAnchorSha256", "metadataRootSha256", "registrationSha256",
    "revisionCatalogWatermarkSha256", "targetComparatorSnapshotRootSha256", "targetCutoffAt"];
  requireExactKeys(requireObject(value, "native confirmation access"), keys,
    "native confirmation access");
  for (const key of keys.filter((key) => key.endsWith("Sha256"))) {
    requireOwnerHash(value[key], "native access identity");
  }
  validateV2Instant(value.accessedAt, "native access clock");
  validateV2Instant(value.targetCutoffAt, "native target cutoff");
  const preimage = [
    "adjustment-confirmation-access/v2",
    value.registrationSha256,
    value.journalHeadSha256,
    value.maintenanceAnchorSha256,
    value.gateManifestSha256,
    value.eligiblePredictionSetSha256,
    value.expectedKeySetSha256,
    value.metadataRootSha256,
    value.targetComparatorSnapshotRootSha256,
    value.revisionCatalogWatermarkSha256,
    value.targetCutoffAt,
  ].join("\n");

  // recompute the server trigger identity while retaining its separate clock
  if (sha256(Buffer.from(preimage)) !== value.accessSha256) {
    throw new Error("native confirmation access identity differs");
  }
  return value;
}

// validate a verified root action or explicit finalized no-action receipt
function validateOwnerTerminalActionReceipt(value) {
  requireObject(value, "terminal action receipt");
  if (value.contractVersion === "adjustment-family-release-status/v1") {
    requireExactKeys(value, ["actionSha256", "compensatingRelease", "compensationState",
      "contractVersion", "family", "fencingToken", "outcome", "state", "targetRelease"],
    "terminal action receipt");
    requireOwnerHash(value.actionSha256, "terminal action receipt");
    const operatorOff = value.state === "operator_off_unapplied";
    if (!OWNER_CONFIRMATION_FAMILIES.has(value.family) ||
      !["acknowledged", "compensation_required", "operator_off_unapplied"]
        .includes(value.state) || value.state === "compensation_required" &&
      value.compensationState !== "verified" || operatorOff &&
      (value.compensationState !== "absent" || value.outcome !==
        "operator_off_unapplied")) {
      throw new Error("terminal action receipt differs");
    }
    return value;
  }
  requireExactKeys(value, ["actionSha256", "completedAt", "contractVersion", "disposition",
    "fullMemberRootSha256", "policyReportSha256", "registrationSha256", "state"],
  "terminal no-action receipt");
  for (const key of ["actionSha256", "fullMemberRootSha256", "policyReportSha256",
    "registrationSha256"]) requireOwnerHash(value[key], "terminal no-action identity");
  validateV2Instant(value.completedAt, "terminal no-action clock");
  const identity = {
    contractVersion: "adjustment-terminal-no-action-identity/v3",
    disposition: value.disposition,
    policyReportSha256: value.policyReportSha256,
    registrationSha256: value.registrationSha256,
  };
  if (value.contractVersion !== "adjustment-terminal-no-action-receipt/v3" ||
    value.state !== "verified_no_action" ||
    !["rejected", "resource_refused", "support_failed"].includes(value.disposition) ||
    value.actionSha256 !== sha256(Buffer.from(canonicalJson(identity)))) {
    throw new Error("terminal no-action receipt differs");
  }
  return value;
}

// validate exact promoted action bytes against the immutable action identity
function validateOwnerTerminalModelAction(value, actionSha256, result) {
  const keys = ["actionKind", "candidateGraphSha256", "candidateSha256", "contractVersion",
    "createdAt", "expectedInstalledReceiptSha256", "expectedSettingsSha256",
    "expectedSourceCommit", "expectedSourceRelease", "family", "fencingToken",
    "fullMemberRootSha256", "lifecycleHeadSha256", "policyDecision", "policyReportSha256",
    "predecessorActionSha256", "reason", "reportCreatedAt", "siteKey", "validThrough"];
  requireExactKeys(requireObject(value, "terminal model action"), keys, "terminal model action");
  if (value.actionKind !== "promote" || value.policyDecision !== "qualified" ||
    value.fullMemberRootSha256 !== result.fullMemberRootSha256 ||
    value.policyReportSha256 !== result.candidateReportSha256 ||
    sha256(Buffer.from(canonicalJson(value))) !== actionSha256) {
    throw new Error("terminal model action differs");
  }
  return value;
}

// validate the archived predecessor or current terminal tombstone
function validateOwnerPreviousTombstone(value, nullable = true) {
  if (value === null && nullable) return null;
  requireExactKeys(requireObject(value, "terminal tombstone"), [
    "contractVersion", "reconciliationSha256", "registrationSha256", "terminalResultSha256",
  ], "terminal tombstone");
  for (const key of ["reconciliationSha256", "registrationSha256", "terminalResultSha256"]) {
    requireOwnerHash(value[key], "terminal tombstone identity");
  }
  if (value.contractVersion !== "adjustment-shadow-terminal-tombstone/v3") {
    throw new Error("terminal tombstone contract differs");
  }
  return value;
}

// validate the owner-projected terminal row needed for retirement
function validateOwnerTerminalRecord(value) {
  const keys = ["accessSha256", "actionCompletedAt", "actionDisposition", "actionSha256",
    "candidateSha256", "contractVersion", "family", "finalizedMetadataRootSha256",
    "finalizedPredictionCount", "maintenanceAnchorSha256", "metadataGeneration", "recordedAt",
    "reconciliationSha256", "registrationSha256", "reservedKeySha256", "sourceSha256",
    "terminalMemberSha256", "terminalResultSha256"];
  requireExactKeys(requireObject(value, "terminal record"), keys, "terminal record");
  for (const key of keys.filter((key) => key.endsWith("Sha256"))) {
    requireOwnerHash(value[key], "terminal record identity");
  }
  for (const key of ["actionCompletedAt", "recordedAt"]) {
    validateV2Instant(value[key], "terminal record clock");
  }
  if (value.contractVersion !== "adjustment-shadow-terminal-record/v3" ||
    !OWNER_CONFIRMATION_FAMILIES.has(value.family) ||
    !new Set([
      "operator_off_compensated", "promoted_operator_off_unapplied",
      "promoted_verified", "promotion_failed_compensated", "rejected_no_action",
      "resource_refused_no_action", "support_failed_no_action",
    ]).has(value.actionDisposition) ||
    !Number.isSafeInteger(value.metadataGeneration) || value.metadataGeneration < 1 ||
    !Number.isSafeInteger(value.finalizedPredictionCount) || value.finalizedPredictionCount < 1) {
    throw new Error("terminal record differs");
  }
  return value;
}

// require one full lowercase sha-256 identity
function requireOwnerHash(value, label) {
  if (!HASH_PATTERN.test(value ?? "")) throw new Error(`${label} differs`);
}

// project one canonical local date from an authenticated instant
function ownerLocalDate(value) {
  const parts = Object.fromEntries(new Intl.DateTimeFormat("en-US", {
    day: "2-digit",
    month: "2-digit",
    timeZone: "America/Los_Angeles",
    year: "numeric",
  }).formatToParts(new Date(value)).map(
    // retain the named calendar components
    (part) => [part.type, part.value],
  ));
  return `${parts.year}-${parts.month}-${parts.day}`;
}

// derive the closed dedicated source specifications from the frozen gauge catalog
export function adjustmentRainFixedGaugeTargetSourceSpecifications() {
  return RAIN_FIXED_GAUGE_TARGET_STATIONS.map((station) => {
    const sourceKey = `rain-target-tempest-${String(station.locationId)}`;
    const adapterConfig = {
      contractVersion: RAIN_FIXED_GAUGE_TARGET_SOURCE_VERSION,
      deviceId: station.deviceId,
      locationId: station.locationId,
      projection: "fixed-gauge-hour-target",
    };
    const material = {
      adapterConfig,
      location: {
        latitude: station.latitude,
        longitude: station.longitude,
        siteKey: "ballydidean",
        timezone: "America/Los_Angeles",
      },
      providerKey: "weatherflow-tempest",
      sourceKey,
      sourceKind: "physical_sensor",
      stationKey: sourceKey,
      version: 1,
    };
    return {
      adapterConfig,
      deviceId: station.deviceId,
      latitude: station.latitude,
      locationId: station.locationId,
      longitude: station.longitude,
      serial: station.serial,
      sourceConfigFingerprint: sha256(Buffer.from(canonicalJsonValue(material))),
      sourceKey,
      stationKey: sourceKey,
    };
  });
}

const RAIN_FIXED_GAUGE_TARGET_SOURCE_SPECS =
  adjustmentRainFixedGaugeTargetSourceSpecifications();
export const RAIN_FIXED_GAUGE_TARGET_SOURCE_CATALOG_SHA256 = sha256(Buffer.from(canonicalJson({
  contractVersion: RAIN_FIXED_GAUGE_TARGET_SOURCE_CATALOG_VERSION,
  sources: RAIN_FIXED_GAUGE_TARGET_SOURCE_SPECS,
})));
export const RAIN_FIXED_GAUGE_TARGET_RUN_CONTRACT_SHA256 = sha256(Buffer.from(canonicalJson({
  adapterVersion: RAIN_FIXED_GAUGE_TARGET_ADAPTER_VERSION,
  contractVersion: RAIN_FIXED_GAUGE_TARGET_RUN_VERSION,
  sourceCatalogSha256: RAIN_FIXED_GAUGE_TARGET_SOURCE_CATALOG_SHA256,
})));

// create or verify only the twelve exact source identities without rewriting drift
export function buildAdjustmentRainFixedGaugeTargetSourceInitializationSql() {
  const specs = canonicalJsonValue(RAIN_FIXED_GAUGE_TARGET_SOURCE_SPECS);
  return `BEGIN;
SET LOCAL statement_timeout = '30s';
SET LOCAL lock_timeout = '5s';
INSERT INTO providers (
  provider_key, display_name, attribution_label, attribution_url, active
) VALUES (
  'weatherflow-tempest', 'WeatherFlow Tempest', 'Weather data by Tempest',
  'https://tempestwx.com/', true
) ON CONFLICT (provider_key) DO NOTHING;
DO $rain_target$
DECLARE
  item jsonb;
  provider_row providers%ROWTYPE;
  site_row sites%ROWTYPE;
  source_row sources%ROWTYPE;
  station_row stations%ROWTYPE;
BEGIN
  SELECT * INTO STRICT provider_row FROM providers
  WHERE provider_key = 'weatherflow-tempest' FOR UPDATE;
  IF ROW(provider_row.display_name, provider_row.attribution_label,
      provider_row.attribution_url, provider_row.active)
    IS DISTINCT FROM ROW('WeatherFlow Tempest', 'Weather data by Tempest',
      'https://tempestwx.com/', true) THEN
    RAISE EXCEPTION 'rain target provider differs' USING ERRCODE = '23514';
  END IF;
  SELECT * INTO STRICT site_row FROM sites WHERE slug = 'ballydidean' FOR UPDATE;
  IF site_row.timezone <> 'America/Los_Angeles' OR NOT site_row.active THEN
    RAISE EXCEPTION 'rain target site differs' USING ERRCODE = '23514';
  END IF;
  FOR item IN SELECT value FROM jsonb_array_elements($specs$${specs}$specs$::jsonb) LOOP
    INSERT INTO stations (
      site_id, slug, display_name, station_kind, latitude, longitude,
      vendor, model, serial, active
    ) VALUES (
      site_row.id, item->>'stationKey',
      'Rain target Tempest ' || (item->>'locationId'), 'physical',
      (item->>'latitude')::double precision, (item->>'longitude')::double precision,
      'WeatherFlow', 'Tempest', item->>'serial', true
    ) ON CONFLICT (site_id, slug) DO NOTHING;
    SELECT * INTO STRICT station_row FROM stations
    WHERE site_id = site_row.id AND slug = item->>'stationKey' FOR UPDATE;
    IF ROW(station_row.display_name, station_row.station_kind,
        station_row.latitude, station_row.longitude, station_row.vendor,
        station_row.model, station_row.serial, station_row.active)
      IS DISTINCT FROM ROW('Rain target Tempest ' || (item->>'locationId'),
        'physical', (item->>'latitude')::double precision,
        (item->>'longitude')::double precision, 'WeatherFlow', 'Tempest',
        item->>'serial', true) THEN
      RAISE EXCEPTION 'rain target station differs' USING ERRCODE = '23514';
    END IF;
    INSERT INTO sources (
      station_id, provider_id, source_key, source_kind, material_provider_config,
      source_config_fingerprint, capabilities, cadence_seconds, active
    ) VALUES (
      station_row.id, provider_row.id, item->>'sourceKey', 'physical_sensor',
      item->'adapterConfig', item->>'sourceConfigFingerprint',
      '["historical"]'::jsonb, NULL, true
    ) ON CONFLICT (station_id, source_key) DO NOTHING;
    SELECT * INTO STRICT source_row FROM sources
    WHERE station_id = station_row.id AND source_key = item->>'sourceKey' FOR UPDATE;
    IF ROW(source_row.provider_id, source_row.source_kind,
        source_row.material_provider_config, source_row.source_config_fingerprint,
        source_row.capabilities, source_row.cadence_seconds, source_row.active)
      IS DISTINCT FROM ROW(provider_row.id, 'physical_sensor',
        item->'adapterConfig', (item->>'sourceConfigFingerprint')::char(64),
        '["historical"]'::jsonb, NULL::integer, true) THEN
      RAISE EXCEPTION 'rain target source differs' USING ERRCODE = '23514';
    END IF;
  END LOOP;
END;
$rain_target$;
WITH expected AS (
  SELECT value AS item, ordinality
  FROM jsonb_array_elements($specs$${specs}$specs$::jsonb) WITH ORDINALITY
), retained AS (
  SELECT expected.ordinality, expected.item, source.id::text AS source_id
  FROM expected
  JOIN stations station ON station.slug = expected.item->>'stationKey'
  JOIN sites site ON site.id = station.site_id AND site.slug = 'ballydidean'
  JOIN sources source ON source.station_id = station.id
    AND source.source_key = expected.item->>'sourceKey'
)
SELECT json_build_object(
  'catalogSha256', '${RAIN_FIXED_GAUGE_TARGET_SOURCE_CATALOG_SHA256}',
  'contractVersion', '${RAIN_FIXED_GAUGE_TARGET_SOURCE_CATALOG_VERSION}',
  'runContractSha256', '${RAIN_FIXED_GAUGE_TARGET_RUN_CONTRACT_SHA256}',
  'sources', json_agg(json_build_object(
    'locationId', (item->>'locationId')::integer,
    'sourceConfigFingerprint', item->>'sourceConfigFingerprint',
    'sourceId', source_id,
    'sourceKey', item->>'sourceKey'
  ) ORDER BY ordinality)
) FROM retained;
COMMIT;
`;
}

// validate one actual owner-projected dedicated source catalog
export function validateAdjustmentRainFixedGaugeTargetSourceCatalog(value) {
  requireExactKeys(requireObject(value, "rain target source catalog"), [
    "catalogSha256", "contractVersion", "runContractSha256", "sources",
  ], "rain target source catalog");
  // require the static catalog and run contracts
  if (value.contractVersion !== RAIN_FIXED_GAUGE_TARGET_SOURCE_CATALOG_VERSION ||
    value.catalogSha256 !== RAIN_FIXED_GAUGE_TARGET_SOURCE_CATALOG_SHA256 ||
    value.runContractSha256 !== RAIN_FIXED_GAUGE_TARGET_RUN_CONTRACT_SHA256 ||
    !Array.isArray(value.sources) ||
    value.sources.length !== RAIN_FIXED_GAUGE_TARGET_SOURCE_SPECS.length) {
    throw new Error("rain target source catalog differs");
  }
  value.sources.forEach((source, index) => {
    const expected = RAIN_FIXED_GAUGE_TARGET_SOURCE_SPECS[index];
    requireExactKeys(requireObject(source, "rain target source"), [
      "locationId", "sourceConfigFingerprint", "sourceId", "sourceKey",
    ], "rain target source");
    // reject reordered, aliased or nonnumeric database identities
    if (expected === undefined || source.locationId !== expected.locationId ||
      source.sourceConfigFingerprint !== expected.sourceConfigFingerprint ||
      source.sourceKey !== expected.sourceKey ||
      !/^[1-9][0-9]*$/u.test(source.sourceId)) {
      throw new Error("rain target source identity differs");
    }
  });
  return value;
}

// construct only the closed owner function call after exact stdin and epoch validation
export function buildAdjustmentRegistrationScheduleInitializationSql(input) {
  requireExactKeys(requireObject(input, "registration schedule SQL input"),
    ["bootstrapBytes", "bootstrapSha256", "witness"], "registration schedule SQL input");
  // bound actual bytes before JSON parsing or privileged SQL construction
  if (!Buffer.isBuffer(input.bootstrapBytes) || input.bootstrapBytes.length > 2_048 ||
    !HASH_PATTERN.test(input.bootstrapSha256 ?? "")) {
    throw new Error("registration schedule input bytes differ");
  }
  const bootstrap = validateAdjustmentRegistrationScheduleBootstrapV3(
    JSON.parse(input.bootstrapBytes.toString("utf8")), input.witness,
  );
  // reject alternate JSON encodings at the root write boundary
  if (bootstrap.bootstrapSha256 !== input.bootstrapSha256 ||
    !input.bootstrapBytes.equals(Buffer.from(canonicalJson(bootstrap)))) {
    throw new Error("registration schedule input is not canonical");
  }
  return "BEGIN;\nSET LOCAL statement_timeout = '30s';\nSET LOCAL lock_timeout = '5s';\n" +
    "SELECT weather_initialize_adjustment_registration_schedule_v3(" +
    `$bootstrap$${canonicalJsonValue(bootstrap)}$bootstrap$::jsonb);\nCOMMIT;\n`;
}

// project one actual database schedule head without caller-selected dates or clocks
export function projectAdjustmentRegistrationScheduleStatusV3(value, epochWitness) {
  const ledger = projectAdjustmentMaintenanceDatabaseLedgerV3({ ...value, payload: null });
  requireExactKeys(value, ["databaseManifest", "payload", "transaction"], "schedule status envelope");
  const witness = validateAdjustmentRevisionCaptureEpochWitness(epochWitness);
  const slots = value.payload;
  // require all fixed families in database projection order
  if (!Array.isArray(slots) || slots.length !== 3) {
    throw new Error("registration schedule slots differ");
  }
  for (const [index, family] of ["temperature", "wind", "rain"].entries()) {
    const slot = slots[index];
    requireExactKeys(requireObject(slot, "registration schedule slot"), [
      "contractVersion", "epochWitnessSha256", "family", "horizonEndAt", "registrationSha256",
      "scheduleContractSha256", "state", "terminalAt",
    ], "registration schedule slot");
    validateV2Instant(slot.horizonEndAt, "registration horizon");
    // authenticate every family against the same retained epoch and finite schedule head
    if (slot.contractVersion !== "adjustment-shadow-registration-slot/v3" ||
      slot.family !== family || slot.epochWitnessSha256 !== witness.witnessSha256 ||
      slot.scheduleContractSha256 !== ROLLING_SCHEDULE_SHA256 ||
      slot.horizonEndAt !== slots[0].horizonEndAt ||
      !["free", "busy_v3", "busy_v2_legacy"].includes(slot.state) ||
      (slot.state === "free" ? slot.registrationSha256 !== null || slot.terminalAt !== null
        : !HASH_PATTERN.test(slot.registrationSha256 ?? "") ||
          slot.terminalAt === null)) {
      throw new Error("registration schedule slot identity differs");
    }
    // occupied slots retain their exact terminal clock rather than a fabricated deadline
    if (slot.terminalAt !== null) validateV2Instant(slot.terminalAt, "registration terminal");
  }
  // bind the retained 0021 witness to one reviewed additive live ledger
  if (witness.databaseMigrationHistorySha256 !== EXPECTED_ROLLING_MIGRATION_HISTORY_SHA256 ||
    ![EXPECTED_ROLLING_MIGRATION_HISTORY_SHA256,
      EXPECTED_CATALOG_MIGRATION_HISTORY_SHA256,
      EXPECTED_INDEX_MIGRATION_HISTORY_SHA256].includes(
      ledger.databaseManifest.migration_history_sha256,
    ) ||
    Date.parse(ledger.snapshotAt) < Date.parse(witness.epochAt) ||
    Date.parse(slots[0].horizonEndAt) <= Date.parse(witness.epochAt)) {
    throw new Error("registration schedule database epoch differs");
  }
  return {
    contractVersion: "adjustment-registration-schedule-status/v3",
    epochAt: witness.epochAt,
    epochWitnessSha256: witness.witnessSha256,
    horizonEndAt: slots[0].horizonEndAt,
    scheduleContractSha256: ROLLING_SCHEDULE_SHA256,
    snapshotAt: ledger.snapshotAt,
    slots,
  };
}

// authenticate owner operations against independently read live and retained authorities
async function verifyAdjustmentOwnerRequestAuthorityV3(kind, request) {
  const witness = await readAdjustmentRevisionCaptureEpochWitness();
  const witnessDocumentSha256 = sha256(Buffer.from(canonicalJson(witness)));
  const unsupported = kind === "unsupported-terminal" || kind === "unsupported-retire";
  const retirement = kind === "retire" || kind === "unsupported-retire";
  const family = retirement ? request.terminalRecord.family : request.shadowRegistration.family;
  const current = await readAdjustmentFamilyReleaseCurrent(family);
  const catalog = validateInstalledAdjustmentCatalog(await readHostFile(
    ADJUSTMENT_FAMILY_CATALOG_PATH, ADJUSTMENT_FAMILY_MAXIMUM_CATALOG_BYTES, 0o644, [0],
  ));
  const epochReference = kind === "access" ? request.epochWitnessSha256
    : unsupported ? request.unsupportedProof.captureEpochWitnessSha256
      : request.finalizationProof.captureEpochWitnessSha256;
  // preserve the immutable full-document epoch domain across all later release commits
  if (epochReference !== witnessDocumentSha256 || kind !== "retire" &&
    request.shadowRegistration.epochWitnessSha256 !== witness.witnessSha256) {
    throw new Error("owner request retained epoch differs");
  }
  // access is a pre-value burn authority and cannot claim finalization
  if (kind === "access") {
    const entry = catalog.entries.find((item) => item.family === family && item.slot === "shadow");
    const actualAcknowledgement = validateAdjustmentRevisionColdCustodyAcknowledgementV2(
      parseCanonicalFamilyJson(await readHostFile(
        "/var/lib/weather/xweather/adjustment-evidence/revision-custody-acknowledgements/current.json",
        64 * 1_024, 0o600, [10_002],
      ), "owner actual custody acknowledgement"),
    );
    if (current.commit !== request.sourceCommit || entry === undefined ||
      canonicalJson(entry.registration) !== canonicalJson(request.shadowRegistration) ||
      canonicalJson(actualAcknowledgement) !== canonicalJson(request.acknowledgement) ||
      Date.parse(request.archive.fullGraphVerifiedAt) > Date.now() ||
      Date.parse(request.localBurn.accessedAt) > Date.now()) {
      throw new Error("owner burn current source or custody differs");
    }
    return null;
  }
  // unsupported proof is terminal-only and never reaches the qualified anchor store
  if (unsupported) {
    const proof = request.unsupportedProof;
    const proofSha256 = sha256(Buffer.from(canonicalJson(proof)));
    const retained = await new AdjustmentUnsupportedTerminalProofStore().readCurrent();
    const entry = catalog.entries.find((item) => item.family === family && item.slot === "shadow");
    if (retained === null || retained.proofSha256 !== proofSha256 ||
      canonicalJson(retained.proof) !== canonicalJson(proof) ||
      proof.sourceCommit !== current.commit || retirement === false &&
        (entry === undefined || canonicalJson(entry.registration) !==
          canonicalJson(request.shadowRegistration) ||
          Date.parse(request.actionReceipt.completedAt) > Date.now())) {
      throw new Error("owner request current unsupported proof differs");
    }
    return retirement ? null : request.actionReceipt.completedAt;
  }
  const proof = request.finalizationProof;
  const { AdjustmentMaintenanceAnchorStore } = await import("./adjustment-evidence-store.mjs");
  const anchor = await new AdjustmentMaintenanceAnchorStore().authorizeFutureOnlyQualifiedAction({
    actionSha256: proof.actionSha256, fullMemberRootSha256: proof.fullMemberRootSha256,
    lifecycleLedgerRootSha256: proof.lifecycleLedgerRootSha256,
    policyReportSha256: proof.policyReportSha256, sourceCommit: proof.sourceCommit,
  });
  // require the exact external F proof retained by the root store
  if (anchor.finalizationProofSha256 !== sha256(Buffer.from(canonicalJson(proof)))) {
    throw new Error("owner request current finalization proof differs");
  }
  if (kind === "retire") return null;
  // promoted and compensated actions retain their pre-action source in the root journal
  if (request.localResult.disposition === "promoted") {
    const status = await readAdjustmentFamilyReleaseStatus(proof.actionSha256);
    const document = parseCanonicalFamilyJson(await readAdjustmentFamilyPrivateFile(
      join(ADJUSTMENT_FAMILY_RELEASE_ROOT, "transactions", `sha256-${proof.actionSha256}.json`),
      ADJUSTMENT_FAMILY_MAXIMUM_STATE_BYTES, false,
    ), "owner actual family transaction");
    // authenticate the qualified but unapplied result without claiming ancestry or serving change
    if (status.state === "operator_off_unapplied") {
      requireExactKeys(document, ["contractVersion", "record"],
        "owner operator-off family transaction");
      const record = validateAdjustmentFamilyOperatorOffRecord(
        document.record,
        proof.actionSha256,
      );
      const action = request.action;
      if (document.contractVersion !== ADJUSTMENT_FAMILY_OPERATOR_OFF_TRANSACTION_VERSION ||
        canonicalJson(status) !== canonicalJson(request.actionReceipt) ||
        current.commit !== request.sourceCommit || current.release !==
          action.expectedSourceRelease || record.sourceCommit !== request.sourceCommit ||
        record.input.family !== action.family || record.input.fencingToken !==
          action.fencingToken || record.input.reportSha256 !== proof.policyReportSha256 ||
        record.input.expectedSettingsSha256 !== action.expectedSettingsSha256 ||
        record.input.expectedSourceRelease !== action.expectedSourceRelease ||
        record.input.expectedCurrentRelease !== action.expectedSourceRelease ||
        record.input.actionSha256 !== proof.actionSha256 ||
        Date.parse(record.completedAt) > Date.now()) {
        throw new Error("owner terminal actual operator-off acknowledgement differs");
      }
      return record.completedAt;
    }
    adjustmentFamilyLineageEdge(proof.actionSha256, document);
    if (canonicalJson(status) !== canonicalJson(request.actionReceipt) ||
      canonicalJson(document.record.action) !== canonicalJson(request.action) ||
      document.record.sourceCommit !== request.sourceCommit ||
      document.record.action.fullMemberRootSha256 !== proof.fullMemberRootSha256 ||
      document.record.action.policyReportSha256 !== proof.policyReportSha256) {
      throw new Error("owner terminal actual action acknowledgement differs");
    }
    return document.record.state === "acknowledged" ? document.record.verifiedAt : document.compensation.verifiedAt;
  }
  // no-action dispositions preserve the live incumbent and its source
  if (current.commit !== request.sourceCommit ||
    Date.parse(request.actionReceipt.completedAt) > Date.now()) {
    throw new Error("owner terminal no-action current source differs");
  }
  return request.actionReceipt.completedAt;
}

// project only actual immutable terminal columns into the owner wire contract
function ownerTerminalRowSql(alias = "terminal") {
  const utc = (column) => `to_char(${alias}.${column} AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')`;
  const fields = { accessSha256: `${alias}.confirmation_access_sha256`, actionCompletedAt: utc('action_completed_at'),
    actionDisposition: `${alias}.action_disposition`, actionSha256: `${alias}.action_sha256`,
    candidateSha256: 'registration.candidate_sha256', contractVersion: "'adjustment-shadow-terminal-record/v3'",
    family: `${alias}.family`, finalizedMetadataRootSha256: `${alias}.metadata_root_sha256`,
    finalizedPredictionCount: `${alias}.finalized_prediction_count`, maintenanceAnchorSha256: `${alias}.maintenance_anchor_sha256`,
    metadataGeneration: `${alias}.metadata_generation`, recordedAt: utc('recorded_at'),
    reconciliationSha256: `${alias}.reconciliation_sha256`, registrationSha256: `${alias}.registration_sha256`,
    reservedKeySha256: `${alias}.reserved_key_sha256`, sourceSha256: `${alias}.source_sha256`,
    terminalMemberSha256: `${alias}.terminal_member_sha256`, terminalResultSha256: `${alias}.terminal_result_sha256` };
  return `jsonb_build_object(${Object.entries(fields).map(
    // retain only fixed column expressions and field names
    ([key, column]) => `'${key}', ${column}`,
  ).join(', ')})`;
}

// derive qualified reconciliation from genuine database access and its final proof
export function buildAdjustmentShadowTerminalRecordSqlV3(requestValue, requestSha256, completedAt) {
  const request = validateAdjustmentShadowTerminalRecordRequestV3(requestValue);
  return buildAdjustmentShadowTerminalRecordSql(
    request,
    requestSha256,
    completedAt,
    request.finalizationProof,
    request.finalizationProof.transferredAnchorSha256,
  );
}

// derive failed-member reconciliation without accepting unsupported proof as F
export function buildAdjustmentShadowUnsupportedTerminalRecordSqlV1(
  requestValue,
  requestSha256,
  completedAt,
) {
  const request = validateAdjustmentShadowUnsupportedTerminalRecordRequestV1(requestValue);
  const proof = request.unsupportedProof;
  return buildAdjustmentShadowTerminalRecordSql(
    request,
    requestSha256,
    completedAt,
    proof,
    sha256(Buffer.from(canonicalJson(proof))),
  );
}

// build the shared owner transaction after disjoint authority validation
function buildAdjustmentShadowTerminalRecordSql(
  request,
  requestSha256,
  completedAt,
  proof,
  maintenanceAnchorSha256,
) {
  validateV2Instant(completedAt, "root terminal action completion clock");
  // identify the exact canonical request before deriving privileged SQL
  if (sha256(Buffer.from(canonicalJson(request))) !== requestSha256) {
    throw new Error("terminal record request identity differs");
  }
  const shadow = request.shadowRegistration;
  const native = request.nativeAccess;
  const result = request.localResult;
  const actionDisposition = result.disposition === "promoted"
    ? request.actionReceipt.state === "operator_off_unapplied"
      ? "promoted_operator_off_unapplied"
      : request.actionReceipt.state === "acknowledged"
        ? "promoted_verified"
        : "promotion_failed_compensated"
    : { rejected: "rejected_no_action", resource_refused: "resource_refused_no_action", support_failed: "support_failed_no_action" }[result.disposition];
  const terminalResultSha256 = sha256(Buffer.from(canonicalJson(result)));
  const previous = request.previousTombstone;
  const previousGuard = previous === null
    ? "previous.registration_sha256 IS NOT NULL AND previous.registration_sha256 <> registration.registration_sha256"
    : `(previous.registration_sha256 IS NULL OR previous.registration_sha256 <> '${previous.registrationSha256}'
      OR previous.reconciliation_sha256 <> '${previous.reconciliationSha256}'
      OR previous.terminal_result_sha256 <> '${previous.terminalResultSha256}')
      AND (previous.registration_sha256 IS NULL OR previous.registration_sha256 <> registration.registration_sha256)`;
  const previousSetting = previous === null ? ''
    : `PERFORM set_config('weather.adjustment_previous_terminal_tombstone_sha256', '${previous.reconciliationSha256}', true);`;
  return `BEGIN;
SET LOCAL statement_timeout = '30s';
SET LOCAL lock_timeout = '5s';
SET LOCAL weather.adjustment_maintenance_anchor_kind = 'terminal_action_reconciliation';
SET LOCAL weather.adjustment_maintenance_anchor_sha256 = '${maintenanceAnchorSha256}';
SET LOCAL weather.adjustment_terminal_member_root_sha256 = '${proof.fullMemberRootSha256}';
DO $terminal$
DECLARE registration adjustment_shadow_registrations_v2%ROWTYPE;
  access adjustment_confirmation_accesses_v2%ROWTYPE; previous adjustment_shadow_terminal_results_v2%ROWTYPE;
  reconciliation text; argument jsonb;
BEGIN
  PERFORM pg_advisory_xact_lock(hashtextextended('${shadow.registrationSha256}', 0));
  SELECT * INTO STRICT registration FROM adjustment_shadow_registrations_v2
    WHERE registration_sha256 = '${shadow.registrationSha256}' FOR UPDATE;
  SELECT * INTO STRICT access FROM adjustment_confirmation_accesses_v2
    WHERE registration_sha256 = registration.registration_sha256;
  -- require the real owner access and current registered candidate rather than claimed hashes
  IF registration.family <> '${shadow.family}' OR registration.candidate_sha256 <> '${shadow.candidateSha256}'
    OR registration.artifact_sha256 <> '${shadow.artifactSha256}' OR registration.source_sha256 <> '${shadow.sourceSha256}'
    OR registration.reserved_key_sha256 <> '${shadow.reservedKeySha256}'
    OR access.access_sha256 <> '${native.accessSha256}' OR access.journal_head_sha256 <> '${native.journalHeadSha256}'
    OR access.maintenance_anchor_sha256 <> '${native.maintenanceAnchorSha256}'
    OR access.gate_manifest_sha256 <> '${native.gateManifestSha256}'
    OR access.eligible_prediction_set_sha256 <> '${native.eligiblePredictionSetSha256}'
    OR access.expected_key_set_sha256 <> '${native.expectedKeySetSha256}'
    OR access.metadata_root_sha256 <> '${native.metadataRootSha256}'
    OR access.target_comparator_snapshot_root_sha256 <> '${native.targetComparatorSnapshotRootSha256}'
    OR access.revision_catalog_watermark_sha256 <> '${native.revisionCatalogWatermarkSha256}'
    OR access.target_cutoff_at <> '${native.targetCutoffAt}'::timestamptz
    OR date_trunc('milliseconds', access.accessed_at) <> '${native.accessedAt}'::timestamptz THEN
    RAISE EXCEPTION 'terminal record actual owner access differs';
  END IF;
  SELECT * INTO previous FROM adjustment_shadow_terminal_results_v2 WHERE family = registration.family;
  -- rotate only the exact prior tombstone already retained in the terminal graph
  IF ${previousGuard} THEN RAISE EXCEPTION 'terminal record predecessor differs'; END IF;
  ${previousSetting}
  reconciliation := encode(sha256(convert_to('adjustment-shadow-terminal-reconciliation/v2' || E'\n'
    || registration.registration_sha256 || E'\n' || registration.family || E'\n' || registration.candidate_sha256 || E'\n'
    || registration.source_sha256 || E'\n' || registration.reserved_key_sha256 || E'\n${proof.fullMemberRootSha256}\n${terminalResultSha256}\n'
    || access.access_sha256 || E'\n${proof.actionSha256}\n${actionDisposition}\n${completedAt}\n'
    || registration.finalized_metadata_root_sha256 || E'\n' || registration.metadata_generation::text || E'\n'
    || registration.finalized_prediction_count::text || E'\n${maintenanceAnchorSha256}', 'UTF8')), 'hex');
  argument := jsonb_build_object('actionCompletedAt', '${completedAt}', 'actionDisposition', '${actionDisposition}',
    'actionSha256', '${proof.actionSha256}', 'maintenanceAnchorSha256', '${maintenanceAnchorSha256}',
    'reconciliationSha256', reconciliation, 'registrationSha256', registration.registration_sha256,
    'terminalMemberSha256', '${proof.fullMemberRootSha256}', 'terminalResultSha256', '${terminalResultSha256}');
  PERFORM weather_record_adjustment_shadow_terminal_result_v2(argument);
END;
$terminal$;
SELECT json_build_object('contractVersion', 'adjustment-shadow-terminal-record-result/v3',
  'reconciliationSha256', terminal.reconciliation_sha256, 'registrationSha256', terminal.registration_sha256,
  'state', 'recorded', 'terminalRecord', ${ownerTerminalRowSql()})
FROM adjustment_shadow_terminal_results_v2 terminal JOIN adjustment_shadow_registrations_v2 registration USING (registration_sha256)
WHERE terminal.registration_sha256 = '${shadow.registrationSha256}';
COMMIT;
`;
}

// retire only the actual reconciled terminal row under the authenticated final proof
export function buildAdjustmentShadowTerminalRetirementSqlV3(requestValue, requestSha256) {
  const request = validateAdjustmentShadowTerminalRetirementRequestV3(requestValue);
  return buildAdjustmentShadowTerminalRetirementSql(request, requestSha256);
}

// retire only the actual unsupported failed member and its retained tombstone
export function buildAdjustmentShadowUnsupportedTerminalRetirementSqlV1(
  requestValue,
  requestSha256,
) {
  const request = validateAdjustmentShadowUnsupportedTerminalRetirementRequestV1(requestValue);
  return buildAdjustmentShadowTerminalRetirementSql(request, requestSha256);
}

// build the shared retirement transaction after disjoint proof validation
function buildAdjustmentShadowTerminalRetirementSql(request, requestSha256) {
  // never reinterpret a different canonical request as retirement authority
  if (sha256(Buffer.from(canonicalJson(request))) !== requestSha256) {
    throw new Error("terminal retirement request identity differs");
  }
  const identity = request.registrationSha256;
  const reconciliation = request.terminalRecord.reconciliationSha256;
  const argument = { reconciliationSha256: reconciliation, registrationSha256: identity, retirementAnchorSha256: requestSha256 };
  return `BEGIN;
SET LOCAL statement_timeout = '30s';
SET LOCAL lock_timeout = '5s';
SET LOCAL weather.adjustment_maintenance_anchor_kind = 'terminal_registration_retirement';
SET LOCAL weather.adjustment_maintenance_anchor_sha256 = '${requestSha256}';
DO $retire$
DECLARE actual jsonb; retained_terminal adjustment_shadow_terminal_results_v2%ROWTYPE;
BEGIN
  PERFORM pg_advisory_xact_lock(hashtextextended('${identity}', 0));
  SELECT * INTO STRICT retained_terminal FROM adjustment_shadow_terminal_results_v2 WHERE registration_sha256 = '${identity}';
  -- bind every retained terminal field even after its hot registration was retired
  SELECT ${ownerTerminalRowSql()} INTO actual FROM adjustment_shadow_terminal_results_v2 terminal
    JOIN adjustment_shadow_registration_windows_v3 registration USING (registration_sha256)
    WHERE terminal.registration_sha256 = '${identity}';
  IF actual IS NULL OR actual <> $record$${canonicalJsonValue(request.terminalRecord)}$record$::jsonb THEN
    RAISE EXCEPTION 'terminal retirement actual row differs';
  END IF;
  -- preserve the exact durable tombstone when a completed retirement response was lost
  IF NOT EXISTS (SELECT 1 FROM adjustment_shadow_registrations_v2 WHERE registration_sha256 = '${identity}') THEN
    RETURN;
  END IF;
  PERFORM weather_retire_adjustment_shadow_registration_v2($argument$${canonicalJsonValue(argument)}$argument$::jsonb);
END;
$retire$;
SELECT json_build_object('contractVersion', 'adjustment-shadow-terminal-retirement-result/v3',
  'reconciliationSha256', terminal.reconciliation_sha256, 'registrationSha256', terminal.registration_sha256, 'state', 'retired')
FROM adjustment_shadow_terminal_results_v2 terminal WHERE terminal.registration_sha256 = '${identity}';
COMMIT;
`;
}

// build one owner-native burn without conflating the local journal hash domain
export function buildAdjustmentConfirmationAccessBurnSqlV3(requestValue, requestSha256) {
  const request = validateAdjustmentConfirmationAccessBurnRequestV3(requestValue);
  // require the forced operand to identify every actual request byte
  if (sha256(Buffer.from(canonicalJson(request))) !== requestSha256) {
    throw new Error("confirmation burn request identity differs");
  }
  const shadow = request.shadowRegistration;
  const burn = request.localBurn;
  const metadata = request.metadata;
  // project only fixed native timestamp columns
  const utc = (column) => `to_char(${column} AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')`;
  const shadowFields = { artifactSha256: 'window_row.artifact_sha256', candidateSha256: 'window_row.candidate_sha256',
    cohortSha256: 'window_row.cohort_sha256', epochWitnessSha256: 'window_row.epoch_witness_sha256',
    family: 'window_row.family', intervalEndAt: utc('window_row.interval_end_at'),
    intervalStartAt: utc('window_row.interval_start_at'), policySha256: 'window_row.policy_sha256',
    predecessorRegistrationSha256: 'window_row.predecessor_registration_sha256',
    registrationSha256: 'window_row.registration_sha256', reservedKeySha256: 'window_row.reserved_key_sha256',
    scheduleContractSha256: 'window_row.schedule_contract_sha256', siteKey: 'window_row.site_key',
    sourceSha256: 'window_row.source_sha256', targetCutoffAt: utc('window_row.target_cutoff_at'), terminalAt: utc('window_row.terminal_at') };
  const shadowSql = Object.entries(shadowFields).map(
    // never accept a caller-controlled identifier or SQL fragment
    ([key, column]) => `'${key}', ${column}`,
  ).join(', ');
  const nativeFields = { accessSha256: 'access.access_sha256', accessedAt: utc('access.accessed_at'),
    eligiblePredictionSetSha256: 'access.eligible_prediction_set_sha256', expectedKeySetSha256: 'access.expected_key_set_sha256',
    gateManifestSha256: 'access.gate_manifest_sha256', journalHeadSha256: 'access.journal_head_sha256',
    maintenanceAnchorSha256: 'access.maintenance_anchor_sha256', metadataRootSha256: 'access.metadata_root_sha256',
    registrationSha256: 'access.registration_sha256', revisionCatalogWatermarkSha256: 'access.revision_catalog_watermark_sha256',
    targetComparatorSnapshotRootSha256: 'access.target_comparator_snapshot_root_sha256', targetCutoffAt: utc('access.target_cutoff_at') };
  const nativeSql = Object.entries(nativeFields).map(
    // retain the actual owner access row rather than normalizing a missing result
    ([key, column]) => `'${key}', ${column}`,
  ).join(', ');
  return `BEGIN;
SET LOCAL statement_timeout = '30s';
SET LOCAL lock_timeout = '5s';
SET LOCAL weather.adjustment_maintenance_anchor_kind = 'confirmation_accessed';
SET LOCAL weather.adjustment_maintenance_anchor_sha256 = '${requestSha256}';
SET LOCAL weather.adjustment_eligible_prediction_set_sha256 = '${request.archive.eligiblePredictionSetSha256}';
DO $burn$
DECLARE registration adjustment_shadow_registrations_v2%ROWTYPE; argument jsonb; metadata_root text; access_identity text;
BEGIN
  PERFORM pg_advisory_xact_lock(hashtextextended('${shadow.registrationSha256}', 0));
  SELECT * INTO STRICT registration FROM adjustment_shadow_registrations_v2
    WHERE registration_sha256 = '${shadow.registrationSha256}' FOR UPDATE;
  -- bind all frozen registration fields and the exact cold accumulator before access
  IF NOT EXISTS (SELECT 1 FROM adjustment_shadow_registration_windows_v3 window_row
    WHERE window_row.registration_sha256 = registration.registration_sha256
      AND jsonb_build_object(${shadowSql}) = $shadow$${canonicalJsonValue(shadow)}$shadow$::jsonb)
    OR registration.metadata_generation <> ${metadata.generation}
    OR registration.finalized_prediction_count <> ${metadata.finalizedPredictionCount}
    OR registration.finalized_metadata_root_sha256 <> '${metadata.rootSha256}'
    OR registration.last_finalization_anchor_sha256 <> '${metadata.lastAnchorSha256}'
    OR registration.finalized_through_at <> '${metadata.throughAt}'::timestamptz
    OR EXISTS (SELECT 1 FROM adjustment_shadow_predictions_v2 prediction
      WHERE prediction.registration_sha256 = registration.registration_sha256) THEN
    RAISE EXCEPTION 'confirmation burn actual metadata differs';
  END IF;
  metadata_root := encode(sha256(convert_to(registration.finalized_metadata_root_sha256 || E'\n'
    || encode(sha256(convert_to('[]', 'UTF8')), 'hex') || E'\n'
    || registration.finalized_prediction_count::text || E'\n0\n' || registration.metadata_generation::text, 'UTF8')), 'hex');
  argument := jsonb_build_object('registrationSha256', registration.registration_sha256,
    'journalHeadSha256', '${request.journalHeadSha256}', 'maintenanceAnchorSha256', '${requestSha256}',
    'gateManifestSha256', '${burn.gateManifestSha256}', 'eligiblePredictionSetSha256', '${request.archive.eligiblePredictionSetSha256}',
    'expectedKeySetSha256', '${burn.expectedKeySetSha256}', 'metadataRootSha256', metadata_root,
    'targetComparatorSnapshotRootSha256', '${burn.targetComparatorSnapshotRootSha256}',
    'revisionCatalogWatermarkSha256', '${burn.revisionCatalogWatermarkSha256}', 'targetCutoffAt', '${burn.targetCutoffAt}');
  access_identity := encode(sha256(convert_to('adjustment-confirmation-access/v2' || E'\n'
    || registration.registration_sha256 || E'\n${request.journalHeadSha256}\n${requestSha256}\n${burn.gateManifestSha256}\n${request.archive.eligiblePredictionSetSha256}\n${burn.expectedKeySetSha256}\n'
    || metadata_root || E'\n${burn.targetComparatorSnapshotRootSha256}\n${burn.revisionCatalogWatermarkSha256}\n${burn.targetCutoffAt}', 'UTF8')), 'hex');
  argument := argument || jsonb_build_object('accessSha256', access_identity);
  PERFORM weather_record_adjustment_confirmation_access_v2(argument);
END;
$burn$;
SELECT json_build_object('contractVersion', 'adjustment-confirmation-access-burn-result/v3',
  'localAccessSha256', '${burn.accessSha256}', 'nativeAccess', jsonb_build_object(${nativeSql}),
  'nativeAccessSha256', access.access_sha256, 'state', 'burned')
FROM adjustment_confirmation_accesses_v2 access WHERE access.registration_sha256 = '${shadow.registrationSha256}';
COMMIT;
`;
}

// derive compact custody arguments from actual native rows and native jsonb hashes
export function buildAdjustmentShadowMetadataPreparationSql(proofValue) {
  const proof = validateAdjustmentShadowMetadataCustodyProof(proofValue);
  // reject duplicate proof rows before emitting any owner SQL
  if (new Set(proof.entries.map((entry) => entry.predictionSha256)).size !== proof.entries.length) {
    throw new Error("shadow metadata custody predictions repeat");
  }
  const expected = proof.entries.map(({ capsuleSha256: _capsule, ...entry }) => entry);
  const literal = canonicalJsonValue(expected);
  // format only fixed database columns using the frozen receipt grammar
  const utc = (column) => `to_char(${column} AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')`;
  const compactFields = {
    bodyByteCount: 'prediction.body_byte_count', committedAt: utc('prediction.committed_at'),
    dueKey: 'prediction.due_key', inputSha256: 'prediction.input_sha256', issuedAt: utc('prediction.issued_at'),
    maxValidAt: utc('prediction.max_valid_at'), minValidAt: utc('prediction.min_valid_at'),
    predictionBodySha256: 'prediction.prediction_body_sha256',
    predictionSchemaSha256: 'prediction.prediction_schema_sha256', predictionSha256: 'prediction.prediction_sha256',
    rowCount: 'prediction.row_count', sourceReceiptSha256: 'prediction.source_receipt_sha256',
  };
  const receiptFields = { ...compactFields };
  delete receiptFields.committedAt;
  Object.assign(receiptFields, {
    archiveCommitOrdinal: 'prediction.archive_commit_ordinal::text', archiveCommittedAt: utc('prediction.archive_committed_at'),
    frontierSha256: 'prediction.revision_frontier_sha256', predictionCommittedAt: utc('prediction.committed_at'),
    predecessorFrontierSha256: 'prediction.predecessor_frontier_sha256', receiptSha256: 'prediction.revision_receipt_sha256',
    registrationSha256: 'prediction.registration_sha256', stageReceiptSha256: 'prediction.stage_receipt_sha256',
  });
  // keep every identifier fixed and every operand closed by the proof validator
  const objectSql = (fields) => `jsonb_build_object(${Object.entries(fields).map(
    ([key, column]) => `'${key}', ${column}`,
  ).join(', ')})`;
  return `BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout = '30s';
SET LOCAL lock_timeout = '5s';
DO $verify$
DECLARE expected jsonb := $proof$${literal}$proof$::jsonb; entry jsonb; group_row record; selected_count integer;
BEGIN
  -- require each capsule proof to match the actual compact row and receipt pointers
  FOR entry IN SELECT value FROM jsonb_array_elements(expected) LOOP
    IF NOT EXISTS (SELECT 1 FROM adjustment_shadow_predictions_v2 prediction
      JOIN adjustment_shadow_registration_windows_v3 window_row USING (registration_sha256)
      WHERE prediction.prediction_sha256 = entry->>'predictionSha256'
        AND prediction.revision_contract_epoch = 1
        AND ${objectSql(receiptFields)} = entry) THEN
      RAISE EXCEPTION 'shadow metadata custody row differs';
    END IF;
  END LOOP;
  -- prohibit deleting an unacknowledged sibling sharing a selected time range
  FOR group_row IN SELECT value->>'registrationSha256' AS registration,
    min((value->>'predictionCommittedAt')::timestamptz) AS first_at,
    max((value->>'predictionCommittedAt')::timestamptz) AS last_at, count(*) AS row_count
    FROM jsonb_array_elements(expected) GROUP BY value->>'registrationSha256' LOOP
    SELECT count(*) INTO selected_count FROM adjustment_shadow_predictions_v2 prediction
      WHERE prediction.registration_sha256 = group_row.registration
        AND prediction.committed_at BETWEEN group_row.first_at AND group_row.last_at;
    IF selected_count <> group_row.row_count THEN
      RAISE EXCEPTION 'shadow metadata custody range differs';
    END IF;
  END LOOP;
END;
$verify$;
WITH expected AS (SELECT value FROM jsonb_array_elements($proof$${literal}$proof$::jsonb)),
groups AS (SELECT value->>'registrationSha256' AS registration,
  min((value->>'predictionCommittedAt')::timestamptz) AS first_at,
  max((value->>'predictionCommittedAt')::timestamptz) AS last_at
  FROM expected GROUP BY value->>'registrationSha256'),
selected AS (SELECT groups.registration, groups.first_at, groups.last_at, count(*)::integer AS row_count,
  encode(sha256(convert_to(jsonb_agg(${objectSql(compactFields)} ORDER BY prediction.prediction_sha256)::text, 'UTF8')), 'hex') AS manifest
  FROM groups JOIN adjustment_shadow_predictions_v2 prediction ON prediction.registration_sha256 = groups.registration
    AND prediction.committed_at BETWEEN groups.first_at AND groups.last_at
  GROUP BY groups.registration, groups.first_at, groups.last_at),
arguments AS (SELECT jsonb_build_object(
  'coldCommitSha256', '${proof.custodyCheckpointSha256}',
  'expectedPreviousMetadataRootSha256', registration.finalized_metadata_root_sha256,
  'finalDisposition', 'retain', 'fromCommittedAt', ${utc('selected.first_at')},
  'generation', registration.metadata_generation + 1, 'maintenanceAnchorSha256', '${proof.proofSha256}',
  'metadataManifestSha256', selected.manifest,
  'newMetadataRootSha256', encode(sha256(convert_to(registration.finalized_metadata_root_sha256 || E'\\n'
    || selected.manifest || E'\\n' || (registration.metadata_generation + 1)::text || E'\\n'
    || selected.row_count::text || E'\\n' || ${utc('selected.last_at')} || E'\\n'
    || '${proof.custodyCheckpointSha256}' || E'\\n' || '${proof.proofSha256}', 'UTF8')), 'hex'),
  'registrationSha256', selected.registration, 'rowCount', selected.row_count,
  'throughCommittedAt', ${utc('selected.last_at')}) AS argument
  FROM selected JOIN adjustment_shadow_registrations_v2 registration ON registration.registration_sha256 = selected.registration)
SELECT json_build_object('proofSha256', '${proof.proofSha256}',
  'finalizations', jsonb_agg(argument ORDER BY argument->>'registrationSha256')) FROM arguments;
COMMIT;
`;
}

// execute only the durable retain-only preparation under its authenticated custody proof
export function buildAdjustmentShadowMetadataFinalizationSql(preparationValue) {
  const preparation = validateAdjustmentShadowMetadataCustodyPreparation(preparationValue);
  return `BEGIN;
SET LOCAL statement_timeout = '30s';
SET LOCAL lock_timeout = '5s';
SET LOCAL weather.adjustment_maintenance_anchor_kind = 'shadow_metadata_transfer';
SET LOCAL weather.adjustment_maintenance_anchor_sha256 = '${preparation.proofSha256}';
WITH results AS MATERIALIZED (SELECT argument,
  weather_finalize_adjustment_shadow_metadata_v2(argument) AS result
  FROM jsonb_array_elements($arguments$${canonicalJsonValue(preparation.finalizations)}$arguments$::jsonb) argument)
SELECT json_build_object('preparationSha256', '${preparation.preparationSha256}',
  'proofSha256', '${preparation.proofSha256}', 'results', jsonb_agg(result || jsonb_build_object(
    'metadataManifestSha256', argument->>'metadataManifestSha256',
    'newMetadataRootSha256', argument->>'newMetadataRootSha256') ORDER BY argument->>'registrationSha256'))
FROM results;
COMMIT;
`;
}

// read only three actual hot registrations and their retained rolling predecessors
export function buildAdjustmentRegistrationLifecycleStatusSqlV4() {
  // normalize only fixed database timestamp columns to their public identity grammar
  const utc = (column) => `to_char(${column} AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')`;
  const registrationFields = {
    artifactSha256: 'window_row.artifact_sha256', candidateSha256: 'window_row.candidate_sha256',
    cohortSha256: 'window_row.cohort_sha256', epochWitnessSha256: 'window_row.epoch_witness_sha256',
    family: 'window_row.family', intervalEndAt: utc('window_row.interval_end_at'),
    intervalStartAt: utc('window_row.interval_start_at'), policySha256: 'window_row.policy_sha256',
    predecessorRegistrationSha256: 'window_row.predecessor_registration_sha256',
    registrationSha256: 'window_row.registration_sha256', reservedKeySha256: 'window_row.reserved_key_sha256',
    scheduleContractSha256: 'window_row.schedule_contract_sha256', siteKey: 'window_row.site_key',
    sourceSha256: 'window_row.source_sha256', targetCutoffAt: utc('window_row.target_cutoff_at'),
    terminalAt: utc('window_row.terminal_at'),
  };
  const registration = Object.entries(registrationFields).map(
    // emit no caller-controlled SQL operands or identifiers
    ([key, value]) => `'${key}', ${value}`,
  ).join(',\n');
  return `BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout = '5min';
SET LOCAL lock_timeout = '5s';
SET LOCAL idle_in_transaction_session_timeout = '30s';
WITH families(family, ordinal) AS (VALUES ('temperature', 1), ('wind', 2), ('rain', 3)),
entries AS (
  SELECT families.ordinal, jsonb_build_object(
    'slot', adjustment_shadow_registration_slot_v3(families.family),
    'latestRegistrationSha256', latest.registration_sha256,
    'activeRegistration', CASE WHEN window_row.registration_sha256 IS NULL THEN NULL
      ELSE jsonb_build_object(${registration}) END,
    'metadata', CASE WHEN window_row.registration_sha256 IS NULL THEN NULL ELSE jsonb_build_object(
      'generation', hot.metadata_generation,
      'rootSha256', hot.finalized_metadata_root_sha256,
      'finalizedPredictionCount', hot.finalized_prediction_count,
      'throughAt', ${utc('hot.finalized_through_at')},
      'lastAnchorSha256', hot.last_finalization_anchor_sha256) END,
    'predecessor', CASE WHEN predecessor.registration_sha256 IS NULL THEN NULL ELSE jsonb_build_object(
      'registrationSha256', predecessor.registration_sha256,
      'terminalAt', ${utc('predecessor.terminal_at')},
      'reconciliationSha256', terminal.reconciliation_sha256,
      'sourceSha256', predecessor.source_sha256,
      'reservedKeySha256', predecessor.reserved_key_sha256,
      'epochWitnessSha256', predecessor.epoch_witness_sha256,
      'scheduleContractSha256', predecessor.schedule_contract_sha256) END
  ) AS value FROM families
  LEFT JOIN adjustment_shadow_registrations_v2 hot ON hot.family = families.family
  LEFT JOIN adjustment_shadow_registration_windows_v3 window_row
    ON window_row.registration_sha256 = hot.registration_sha256
  LEFT JOIN LATERAL (SELECT registration_sha256
    FROM adjustment_shadow_registration_windows_v3 history
    WHERE history.family = families.family ORDER BY interval_start_at DESC LIMIT 1) latest ON true
  LEFT JOIN adjustment_shadow_terminal_results_v2 terminal ON terminal.family = families.family
  LEFT JOIN adjustment_shadow_registration_windows_v3 predecessor
    ON predecessor.registration_sha256 = terminal.registration_sha256
    AND predecessor.source_sha256 = terminal.source_sha256
    AND predecessor.reserved_key_sha256 = terminal.reserved_key_sha256
)
SELECT json_build_object(
  'databaseManifest', (SELECT row_to_json(manifest) FROM adjustment_evaluation_export_manifest_v1 manifest),
  'payload', (SELECT json_agg(value ORDER BY ordinal) FROM entries),
  'transaction', json_build_object(
    'created_at_utc', ${utc('transaction_timestamp()')},
    'idle_in_transaction_session_timeout', current_setting('idle_in_transaction_session_timeout'),
    'isolation_level', current_setting('transaction_isolation'),
    'lock_timeout', current_setting('lock_timeout'),
    'read_only', current_setting('transaction_read_only'),
    'statement_timeout', current_setting('statement_timeout')));
COMMIT;
`;
}

// validate actual active geometry and the exact public rolling registration identity
function validateLifecycleRegistrationV3(value, family, witness) {
  const identityFields = ['siteKey', 'family', 'candidateSha256', 'artifactSha256', 'policySha256',
    'cohortSha256', 'reservedKeySha256', 'sourceSha256', 'epochWitnessSha256', 'scheduleContractSha256',
    'predecessorRegistrationSha256', 'intervalStartAt', 'intervalEndAt', 'targetCutoffAt', 'terminalAt'];
  requireExactKeys(requireObject(value, 'active rolling registration'),
    [...identityFields, 'registrationSha256'], 'active rolling registration');
  // require every actual artifact, source and registration identity
  for (const field of ['candidateSha256', 'artifactSha256', 'policySha256', 'cohortSha256',
    'reservedKeySha256', 'sourceSha256', 'epochWitnessSha256', 'scheduleContractSha256', 'registrationSha256']) {
    if (!HASH_PATTERN.test(value[field] ?? '')) throw new Error('active registration identity differs');
  }
  // retain the nullable genesis predecessor without accepting aliases
  if (value.predecessorRegistrationSha256 !== null &&
    !HASH_PATTERN.test(value.predecessorRegistrationSha256 ?? '')) {
    throw new Error('active registration predecessor differs');
  }
  for (const field of ['intervalStartAt', 'intervalEndAt', 'targetCutoffAt', 'terminalAt']) {
    validateV2Instant(value[field], `active registration ${field}`);
  }
  const start = Date.parse(value.intervalStartAt);
  const end = Date.parse(value.intervalEndAt);
  const date = new Intl.DateTimeFormat('en-CA', { year: 'numeric', month: '2-digit', day: '2-digit',
    timeZone: ROLLING_ZONE });
  const hour = new Intl.DateTimeFormat('en-US', { hour: '2-digit', minute: '2-digit', second: '2-digit',
    hourCycle: 'h23', timeZone: ROLLING_ZONE });
  const localDays = (Date.parse(`${date.format(new Date(end))}T00:00:00.000Z`) -
    Date.parse(`${date.format(new Date(start))}T00:00:00.000Z`)) / 86_400_000;
  const preimage = ['adjustment-shadow-registration/v3', ...identityFields.map(
    // preserve the frozen nullable predecessor preimage rather than JSON serialization
    (field) => value[field] ?? 'none',
  )].join('\n') + '\n';
  // reject historical, shortened or non-midnight windows even under a matching version label
  if (value.family !== family || value.siteKey !== 'ballydidean' ||
    value.epochWitnessSha256 !== witness.witnessSha256 ||
    value.scheduleContractSha256 !== ROLLING_SCHEDULE_SHA256 ||
    start <= Date.parse(witness.epochAt) || localDays !== (family === 'rain' ? 334 : 366) ||
    hour.format(new Date(start)) !== '00:00:00' || hour.format(new Date(end)) !== '00:00:00' ||
    value.targetCutoffAt !== value.terminalAt || Date.parse(value.terminalAt) !== end + 7 * 86_400_000 ||
    sha256(Buffer.from(preimage)) !== value.registrationSha256) {
    throw new Error('active rolling registration geometry differs');
  }
  return value;
}

// expose only bounded value-free active and reconciled predecessor metadata
export function projectAdjustmentRegistrationLifecycleStatusV4(value, epochWitness) {
  requireExactKeys(requireObject(value, 'registration lifecycle envelope'),
    ['databaseManifest', 'payload', 'transaction'], 'registration lifecycle envelope');
  const witness = validateAdjustmentRevisionCaptureEpochWitness(epochWitness);
  // require all fixed families before projecting their existing slot contracts
  if (!Array.isArray(value.payload) || value.payload.length !== 3) {
    throw new Error('registration lifecycle entries differ');
  }
  const schedule = projectAdjustmentRegistrationScheduleStatusV3({ ...value,
    payload: value.payload.map((entry) => entry.slot) }, witness);
  const entries = value.payload;
  // verify each active slot and its exact retained predecessor chain independently
  for (const [index, entry] of entries.entries()) {
    requireExactKeys(requireObject(entry, 'registration lifecycle entry'),
      ['slot', 'activeRegistration', 'metadata', 'latestRegistrationSha256', 'predecessor'],
      'registration lifecycle entry');
    const slot = schedule.slots[index];
    if (entry.latestRegistrationSha256 !== null &&
      !HASH_PATTERN.test(entry.latestRegistrationSha256 ?? '')) {
      throw new Error('latest rolling registration identity differs');
    }
    // genesis is distinct from a lost or unreconciled predecessor
    if (entry.predecessor !== null) {
      const predecessor = entry.predecessor;
      requireExactKeys(requireObject(predecessor, 'registration predecessor'),
        ['registrationSha256', 'terminalAt', 'reconciliationSha256', 'sourceSha256',
          'reservedKeySha256', 'epochWitnessSha256', 'scheduleContractSha256'], 'registration predecessor');
      for (const field of ['registrationSha256', 'reconciliationSha256', 'sourceSha256',
        'reservedKeySha256', 'epochWitnessSha256', 'scheduleContractSha256']) {
        if (!HASH_PATTERN.test(predecessor[field] ?? '')) throw new Error('registration predecessor identity differs');
      }
      validateV2Instant(predecessor.terminalAt, 'registration predecessor terminal');
      if (predecessor.epochWitnessSha256 !== witness.witnessSha256 ||
        predecessor.scheduleContractSha256 !== ROLLING_SCHEDULE_SHA256 ||
        Date.parse(predecessor.terminalAt) > Date.parse(schedule.snapshotAt) ||
        Date.parse(predecessor.terminalAt) <= Date.parse(witness.epochAt)) {
        throw new Error('registration predecessor epoch or clock differs');
      }
    }
    // expose geometry only for an actual rolling active slot
    if (slot.state === 'busy_v3') {
      const registration = validateLifecycleRegistrationV3(entry.activeRegistration, slot.family, witness);
      if (registration.registrationSha256 !== slot.registrationSha256 ||
        registration.registrationSha256 !== entry.latestRegistrationSha256 ||
        registration.terminalAt !== slot.terminalAt ||
        (registration.predecessorRegistrationSha256 !== (entry.predecessor?.registrationSha256 ?? null) &&
          entry.predecessor?.registrationSha256 !== registration.registrationSha256)) {
        throw new Error('registration lifecycle active binding differs');
      }
      const metadata = entry.metadata;
      requireExactKeys(requireObject(metadata, 'registration metadata'),
        ['generation', 'rootSha256', 'finalizedPredictionCount', 'throughAt', 'lastAnchorSha256'],
        'registration metadata');
      validateV2Integer(metadata.generation, 0, 2_147_483_647, 'metadata generation');
      validateV2Integer(metadata.finalizedPredictionCount, 0, 2_147_483_647, 'metadata count');
      if (!HASH_PATTERN.test(metadata.rootSha256 ?? '') || (metadata.generation === 0
        ? metadata.finalizedPredictionCount !== 0 || metadata.throughAt !== null || metadata.lastAnchorSha256 !== null ||
          metadata.rootSha256 !== '4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945'
        : metadata.finalizedPredictionCount < 1 || metadata.throughAt === null ||
          !HASH_PATTERN.test(metadata.lastAnchorSha256 ?? ''))) {
        throw new Error('registration metadata identity differs');
      }
      if (metadata.throughAt !== null) {
        validateV2Instant(metadata.throughAt, 'metadata through clock');
        if (Date.parse(metadata.throughAt) > Date.parse(schedule.snapshotAt)) {
          throw new Error('registration metadata clock differs');
        }
      }
    } else {
      if (entry.activeRegistration !== null || entry.metadata !== null ||
        (slot.state === 'free' && entry.latestRegistrationSha256 !== (entry.predecessor?.registrationSha256 ?? null))) {
        throw new Error('registration lifecycle free predecessor differs');
      }
    }
  }
  return { contractVersion: 'adjustment-registration-lifecycle-status/v4',
    epochAt: schedule.epochAt, epochWitnessSha256: schedule.epochWitnessSha256,
    horizonEndAt: schedule.horizonEndAt, scheduleContractSha256: schedule.scheduleContractSha256,
    snapshotAt: schedule.snapshotAt, entries };
}

// collect only a bounded metadata document from the forced command stdin
async function readBoundedScheduleMetadata(maximumBytes) {
  const chunks = [];
  let size = 0;
  // reject oversized streams before parsing any root-bound fields
  for await (const chunk of process.stdin) {
    size += chunk.length;
    if (size > maximumBytes) throw new Error("schedule metadata exceeds byte bound");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

// validate one read-only function transaction
function validateV2ReadOnlyTransaction(value) {
  const transaction = requireObject(value, "maintenance export transaction");
  requireExactKeys(transaction, [
    "created_at_utc",
    "idle_in_transaction_session_timeout",
    "isolation_level",
    "lock_timeout",
    "read_only",
    "statement_timeout",
  ], "maintenance export transaction");

  // require the fixed repeatable-read function boundary
  if (!Number.isFinite(Date.parse(transaction.created_at_utc)) ||
    transaction.idle_in_transaction_session_timeout !== "30s" ||
    transaction.isolation_level !== "repeatable read" ||
    transaction.lock_timeout !== "5s" ||
    transaction.read_only !== "on" ||
    transaction.statement_timeout !== "5min") {
    throw new Error("maintenance export transaction is invalid");
  }
  return transaction;
}

// require one canonical utc millisecond instant
function validateV2Instant(value, description) {
  // reject normalized aliases and invalid instants
  if (typeof value !== "string" || !V2_UTC_MILLISECOND_PATTERN.test(value) ||
    !Number.isFinite(Date.parse(value))) {
    throw new Error(`${description} is invalid`);
  }
}

// require one safe integer inside a closed range
function validateV2Integer(value, minimum, maximum, description) {
  // reject fractional and out-of-range counts
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) {
    throw new Error(`${description} is invalid`);
  }
}

// require one lowercase content identity
function validateV2Hash(value, description) {
  // reject aliases and malformed identities
  if (typeof value !== "string" || !HASH_PATTERN.test(value)) {
    throw new Error(`${description} is invalid`);
  }
}

// validate one function-only value-free availability payload
function validateV2AvailabilityPayload(value, registrationSha256) {
  const payload = requireObject(value, "maintenance availability payload");
  requireExactKeys(payload, [
    "contractVersion",
    "expectedKeySetSha256",
    "family",
    "finalizedMetadataRootSha256",
    "finalizedPredictionCount",
    "finalizedThroughAt",
    "hotPredictionCount",
    "hotPredictions",
    "hotSetRootSha256",
    "intervalEndAt",
    "intervalStartAt",
    "metadataGeneration",
    "missingExpectedDueKeys",
    "missingExpectedDueKeysStatus",
    "registrationSha256",
    "targetCutoffAt",
  ], "maintenance availability payload");

  // require the exact requested value-free contract
  if (payload.contractVersion !== "adjustment-confirmation-availability/v2" ||
    payload.registrationSha256 !== registrationSha256 ||
    !V2_FAMILIES.has(payload.family) ||
    payload.missingExpectedDueKeys !== null ||
    payload.missingExpectedDueKeysStatus !==
      "requires_anchored_cold_reconstruction") {
    throw new Error("maintenance availability contract differs");
  }
  for (const [candidate, description] of [
    [payload.expectedKeySetSha256, "expected key set identity"],
    [payload.finalizedMetadataRootSha256, "finalized metadata root"],
    [payload.hotSetRootSha256, "hot set root"],
  ]) {
    validateV2Hash(candidate, description);
  }
  validateV2Integer(
    payload.finalizedPredictionCount,
    0,
    Number.MAX_SAFE_INTEGER,
    "finalized prediction count",
  );
  validateV2Integer(
    payload.metadataGeneration,
    0,
    Number.MAX_SAFE_INTEGER,
    "metadata generation",
  );
  validateV2Instant(payload.intervalStartAt, "interval start");
  validateV2Instant(payload.intervalEndAt, "interval end");
  validateV2Instant(payload.targetCutoffAt, "target cutoff");

  // validate the nullable finalized clock
  if (payload.finalizedThroughAt !== null) {
    validateV2Instant(payload.finalizedThroughAt, "finalized through time");
  }
  if (!Array.isArray(payload.hotPredictions)) {
    throw new Error("maintenance hot predictions are invalid");
  }
  validateV2Integer(payload.hotPredictionCount, 0, 8_192, "hot prediction count");

  // bind the declared projection count
  if (payload.hotPredictionCount !== payload.hotPredictions.length) {
    throw new Error("maintenance hot prediction count differs");
  }
  let previousIdentity = "";
  for (const predictionValue of payload.hotPredictions) {
    const prediction = requireObject(predictionValue, "maintenance hot prediction");
    requireExactKeys(prediction, [
      "dueKey",
      "maxValidAt",
      "minValidAt",
      "predictionSha256",
      "rowCount",
    ], "maintenance hot prediction");
    validateV2Hash(prediction.predictionSha256, "hot prediction identity");
    validateV2Instant(prediction.minValidAt, "hot minimum valid time");
    validateV2Instant(prediction.maxValidAt, "hot maximum valid time");

    // require scheduler due keys and family row bounds
    const maximumRows = payload.family === "wind" ? 168 :
      payload.family === "rain" ? 23 : 12;
    if (typeof prediction.dueKey !== "string" ||
      !V2_DUE_KEY_PATTERN.test(prediction.dueKey)) {
      throw new Error("maintenance hot prediction due key is invalid");
    }
    validateV2Integer(prediction.rowCount, 1, maximumRows, "hot prediction row count");

    // require the database's canonical order
    if (prediction.predictionSha256 <= previousIdentity) {
      throw new Error("maintenance hot predictions are unordered");
    }
    previousIdentity = prediction.predictionSha256;
  }

  // enforce the function's reviewed response cap
  if (Buffer.byteLength(canonicalJsonValue(payload)) > 512 * 1024) {
    throw new Error("maintenance availability exceeds its limit");
  }
  return payload;
}

// validate one persisted-burn chunk authorization
function validateV2AuthorizationPayload(
  value,
  registrationSha256,
  accessSha256,
  chunkIndex,
) {
  const payload = requireObject(value, "maintenance authorization payload");
  requireExactKeys(payload, [
    "accessSha256",
    "chunkCount",
    "chunkIndex",
    "contractVersion",
    "eligiblePredictionSetSha256",
    "expectedKeySetSha256",
    "family",
    "fromLocalDate",
    "metadataRootSha256",
    "registrationSha256",
    "revisionCatalogWatermarkSha256",
    "targetComparatorSnapshotRootSha256",
    "targetCutoffAt",
    "toLocalDateExclusive",
  ], "maintenance authorization payload");

  // bind the function result to the exact forced operands
  if (payload.contractVersion !==
      "adjustment-confirmation-export-authorization/v2" ||
    payload.registrationSha256 !== registrationSha256 ||
    payload.accessSha256 !== accessSha256 || payload.chunkIndex !== chunkIndex ||
    !V2_FAMILIES.has(payload.family)) {
    throw new Error("maintenance authorization contract differs");
  }
  const expectedChunkCount = payload.family === "rain" ? 24 : 27;
  if (payload.chunkCount !== expectedChunkCount || chunkIndex < 0 ||
    chunkIndex >= expectedChunkCount) {
    throw new Error("maintenance authorization partition differs");
  }
  for (const [candidate, description] of [
    [payload.eligiblePredictionSetSha256, "eligible prediction set identity"],
    [payload.expectedKeySetSha256, "expected key set identity"],
    [payload.metadataRootSha256, "metadata root identity"],
    [payload.revisionCatalogWatermarkSha256, "revision catalog watermark"],
    [payload.targetComparatorSnapshotRootSha256, "target snapshot root"],
  ]) {
    validateV2Hash(candidate, description);
  }
  validateV2Instant(payload.targetCutoffAt, "target cutoff");
  const fromDate = parseCalendarDate(payload.fromLocalDate, "authorization start date");
  const toDateExclusive = parseCalendarDate(
    payload.toLocalDateExclusive,
    "authorization exclusive end date",
  );

  // require one to fourteen america/los_angeles local dates
  const spanDays = Math.round((toDateExclusive - fromDate) / 86_400_000);
  if (spanDays < 1 || spanDays > 14) {
    throw new Error("maintenance authorization dates differ");
  }
  if (Buffer.byteLength(canonicalJsonValue(payload)) > 8 * 1024) {
    throw new Error("maintenance authorization exceeds its limit");
  }
  return payload;
}

// read one bounded database function response
async function readV2DatabaseResponse(input = process.stdin) {
  const chunks = [];
  let bytes = 0;

  // retain only the bounded json response
  for await (const inputChunk of input) {
    const chunk = Buffer.isBuffer(inputChunk) ? inputChunk : Buffer.from(inputChunk);
    bytes += chunk.length;
    if (bytes > V2_MAXIMUM_DATABASE_RESPONSE_BYTES) {
      throw new Error("maintenance database response exceeds its limit");
    }
    chunks.push(chunk);
  }
  const encoded = Buffer.concat(chunks).toString("utf8").trim();
  const response = requireObject(JSON.parse(encoded), "maintenance database response");
  requireExactKeys(response, [
    "databaseManifest",
    "payload",
    "transaction",
  ], "maintenance database response");
  return {
    databaseManifest: validateCompleteMaintenanceDatabaseManifest(response.databaseManifest),
    payload: response.payload,
    transaction: validateV2ReadOnlyTransaction(response.transaction),
  };
}

// frame one value-free availability response
export async function frameAdjustmentConfirmationAvailabilityV2({
  input = process.stdin,
  registrationSha256,
}) {
  validateV2Hash(registrationSha256, "registration identity");
  const response = await readV2DatabaseResponse(input);
  const payload = validateV2AvailabilityPayload(response.payload, registrationSha256);
  const envelope = {
    contractVersion: V2_AVAILABILITY_ENVELOPE_VERSION,
    databaseManifest: response.databaseManifest,
    payload,
    payloadSha256: sha256(canonicalJson(payload)),
    transaction: response.transaction,
  };
  return Buffer.from(canonicalJson(envelope));
}

// frame one post-burn authorization response
export async function frameAdjustmentConfirmationAuthorizationV2({
  accessSha256,
  chunkIndex,
  input = process.stdin,
  registrationSha256,
}) {
  validateV2Hash(registrationSha256, "registration identity");
  validateV2Hash(accessSha256, "access identity");
  validateV2Integer(chunkIndex, 0, 26, "chunk index");
  const response = await readV2DatabaseResponse(input);
  const payload = validateV2AuthorizationPayload(
    response.payload,
    registrationSha256,
    accessSha256,
    chunkIndex,
  );
  const envelope = {
    contractVersion: V2_AUTHORIZATION_ENVELOPE_VERSION,
    databaseManifest: response.databaseManifest,
    payload,
    payloadSha256: sha256(canonicalJson(payload)),
    transaction: response.transaction,
  };
  return Buffer.from(canonicalJson(envelope));
}

// validate one framed function response
function validateV2FunctionEnvelope(value, expectedContractVersion) {
  const envelope = requireObject(value, "maintenance function envelope");
  requireExactKeys(envelope, [
    "contractVersion",
    "databaseManifest",
    "payload",
    "payloadSha256",
    "transaction",
  ], "maintenance function envelope");
  if (envelope.contractVersion !== expectedContractVersion) {
    throw new Error("maintenance function envelope contract differs");
  }
  validateCompleteMaintenanceDatabaseManifest(envelope.databaseManifest);
  validateV2ReadOnlyTransaction(envelope.transaction);
  validateV2Hash(envelope.payloadSha256, "maintenance payload identity");

  // bind the function payload to its canonical bytes
  if (sha256(canonicalJson(envelope.payload)) !== envelope.payloadSha256) {
    throw new Error("maintenance function payload hash differs");
  }
  return envelope;
}

// load one canonical authorization envelope
async function readV2AuthorizationEnvelope(path) {
  const bytes = await readNoFollowBoundedFile(
    path,
    V2_MAXIMUM_DATABASE_RESPONSE_BYTES,
    "maintenance authorization envelope",
  );
  const envelope = validateV2FunctionEnvelope(
    JSON.parse(bytes),
    V2_AUTHORIZATION_ENVELOPE_VERSION,
  );
  const payload = validateV2AuthorizationPayload(
    envelope.payload,
    envelope.payload.registrationSha256,
    envelope.payload.accessSha256,
    envelope.payload.chunkIndex,
  );

  // require canonical stored bytes
  if (!bytes.equals(Buffer.from(canonicalJson(envelope)))) {
    throw new Error("maintenance authorization envelope is not canonical");
  }
  return { bytes, envelope, payload };
}

// read one edge watermark without exporting host paths
async function readV2EdgeWatermark(snapshotPath) {
  const bytes = await readNoFollowBoundedFile(
    snapshotPath,
    4 * 1024 * 1024,
    "maintenance edge snapshot",
  );
  const snapshot = requireObject(JSON.parse(bytes), "maintenance edge snapshot");
  requireExactKeys(snapshot, [
    "contractVersion",
    "entries",
    "frozenAt",
    "watermarkSha256",
  ], "maintenance edge snapshot");
  validateV2Hash(snapshot.watermarkSha256, "edge snapshot watermark");
  if (snapshot.contractVersion !== EDGE_SNAPSHOT_VERSION ||
    !Array.isArray(snapshot.entries) || !Number.isFinite(Date.parse(snapshot.frozenAt))) {
    throw new Error("maintenance edge snapshot contract differs");
  }
  return snapshot.watermarkSha256;
}

// add calendar days without timezone conversion
function addV2CalendarDays(value, days) {
  const parsed = new Date(parseCalendarDate(value, "maintenance local date"));
  parsed.setUTCDate(parsed.getUTCDate() + days);
  return parsed.toISOString().slice(0, 10);
}

// validate one immutable v2 stream header
function validateV2StreamHeader(value) {
  const header = requireObject(value, "maintenance stream header");
  requireExactKeys(header, [
    "authorization",
    "authorizationSha256",
    "contractVersion",
    "dateSemantics",
    "exportKind",
    "fromLocalDate",
    "payloadBytes",
    "payloadSha256",
    "snapshotLineage",
    "sourcePackageContractVersion",
    "toLocalDate",
    "valueAuthority",
  ], "maintenance stream header");
  if (header.contractVersion !== V2_ENVELOPE_VERSION ||
    header.dateSemantics !== "america_los_angeles_inclusive_local_dates" ||
    header.sourcePackageContractVersion !== PACKAGE_VERSION) {
    throw new Error("maintenance stream contract differs");
  }
  validateDateRange(header.fromLocalDate, header.toLocalDate);
  validateV2Hash(header.payloadSha256, "maintenance stream payload identity");
  validateV2Integer(header.payloadBytes, 1, MAX_ARCHIVE_BYTES, "stream payload bytes");
  const lineage = requireObject(header.snapshotLineage, "maintenance snapshot lineage");
  requireExactKeys(lineage, [
    "edgeEvidenceWatermarkSha256",
    "revisionCatalogWatermarkSha256",
    "targetComparatorSnapshotRootSha256",
  ], "maintenance snapshot lineage");
  validateV2Hash(lineage.edgeEvidenceWatermarkSha256, "edge evidence watermark");

  // separate descriptive daily transport from post-burn authority
  if (header.exportKind === "daily_monitoring") {
    if (header.authorization !== null || header.authorizationSha256 !== null ||
      header.valueAuthority !== "none" ||
      lineage.revisionCatalogWatermarkSha256 !== null ||
      lineage.targetComparatorSnapshotRootSha256 !== null) {
      throw new Error("daily maintenance stream has action authority");
    }
  } else if (header.exportKind === "post_burn_confirmation") {
    validateV2Hash(header.authorizationSha256, "authorization envelope identity");
    const authorization = validateV2FunctionEnvelope(
      header.authorization,
      V2_AUTHORIZATION_ENVELOPE_VERSION,
    );
    const payload = validateV2AuthorizationPayload(
      authorization.payload,
      authorization.payload.registrationSha256,
      authorization.payload.accessSha256,
      authorization.payload.chunkIndex,
    );
    if (sha256(canonicalJson(header.authorization)) !== header.authorizationSha256 ||
      header.valueAuthority !== "persisted_burn_authorization" ||
      payload.fromLocalDate !== header.fromLocalDate ||
      addV2CalendarDays(payload.toLocalDateExclusive, -1) !== header.toLocalDate ||
      lineage.revisionCatalogWatermarkSha256 !==
        payload.revisionCatalogWatermarkSha256 ||
      lineage.targetComparatorSnapshotRootSha256 !==
        payload.targetComparatorSnapshotRootSha256) {
      throw new Error("confirmation maintenance stream lineage differs");
    }
  } else {
    throw new Error("maintenance stream kind is invalid");
  }
  return header;
}

// hash one stable no-follow archive descriptor
async function hashV2Archive(path) {
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const before = await handle.stat({ bigint: true });
    if (!before.isFile() || before.size < 1n || before.size > BigInt(MAX_ARCHIVE_BYTES)) {
      throw new Error("maintenance source archive is invalid");
    }
    const hash = createHash("sha256");
    const buffer = Buffer.alloc(256 * 1024);
    let offset = 0;

    // stream the bounded source archive
    while (offset < Number(before.size)) {
      const read = await handle.read(
        buffer,
        0,
        Math.min(buffer.length, Number(before.size) - offset),
        offset,
      );
      if (read.bytesRead === 0) {
        throw new Error("maintenance source archive ended early");
      }
      hash.update(buffer.subarray(0, read.bytesRead));
      offset += read.bytesRead;
    }
    const after = await handle.stat({ bigint: true });
    if (before.dev !== after.dev || before.ino !== after.ino ||
      before.size !== after.size || before.mtimeNs !== after.mtimeNs ||
      before.ctimeNs !== after.ctimeNs) {
      throw new Error("maintenance source archive changed while hashing");
    }
    return {
      metadata: before,
      sha256: hash.digest("hex"),
      sizeBytes: Number(before.size),
    };
  } finally {
    await handle.close();
  }
}

// stream one v2 header followed by one immutable v1 archive
export async function streamAdjustmentEvaluationV2EnvelopeArchive({
  authorizationPath = null,
  edgeSnapshotPath,
  exportKind,
  fromLocalDate,
  output = process.stdout,
  sourceArchivePath,
  toLocalDate,
}) {
  validateDateRange(fromLocalDate, toLocalDate);
  const source = await hashV2Archive(sourceArchivePath);
  const edgeEvidenceWatermarkSha256 = await readV2EdgeWatermark(edgeSnapshotPath);
  let authorization = null;
  let authorizationSha256 = null;
  let valueAuthority = "none";
  let revisionCatalogWatermarkSha256 = null;
  let targetComparatorSnapshotRootSha256 = null;

  // require a persisted burn for confirmation transport
  if (exportKind === "post_burn_confirmation") {
    if (authorizationPath === null) {
      throw new Error("confirmation authorization is missing");
    }
    const authorized = await readV2AuthorizationEnvelope(authorizationPath);
    authorization = authorized.envelope;
    authorizationSha256 = sha256(authorized.bytes);
    valueAuthority = "persisted_burn_authorization";
    revisionCatalogWatermarkSha256 =
      authorized.payload.revisionCatalogWatermarkSha256;
    targetComparatorSnapshotRootSha256 =
      authorized.payload.targetComparatorSnapshotRootSha256;
    if (authorized.payload.fromLocalDate !== fromLocalDate ||
      addV2CalendarDays(authorized.payload.toLocalDateExclusive, -1) !== toLocalDate) {
      throw new Error("confirmation archive dates differ from authorization");
    }
  } else if (exportKind !== "daily_monitoring" || authorizationPath !== null) {
    throw new Error("maintenance export kind is invalid");
  }
  const header = validateV2StreamHeader({
    authorization,
    authorizationSha256,
    contractVersion: V2_ENVELOPE_VERSION,
    dateSemantics: "america_los_angeles_inclusive_local_dates",
    exportKind,
    fromLocalDate,
    payloadBytes: source.sizeBytes,
    payloadSha256: source.sha256,
    snapshotLineage: {
      edgeEvidenceWatermarkSha256,
      revisionCatalogWatermarkSha256,
      targetComparatorSnapshotRootSha256,
    },
    sourcePackageContractVersion: PACKAGE_VERSION,
    toLocalDate,
    valueAuthority,
  });
  const headerBytes = Buffer.from(canonicalJson(header));
  if (headerBytes.length > V2_MAXIMUM_HEADER_BYTES) {
    throw new Error("maintenance stream header exceeds its limit");
  }
  await writeChunk(output, V2_STREAM_MAGIC);
  await writeChunk(output, Buffer.from(`${headerBytes.length.toString(16).padStart(8, "0")}\n`));
  await writeChunk(output, headerBytes);
  const handle = await open(sourceArchivePath, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const before = await handle.stat({ bigint: true });
    if (before.dev !== source.metadata.dev || before.ino !== source.metadata.ino ||
      before.size !== source.metadata.size || before.mtimeNs !== source.metadata.mtimeNs ||
      before.ctimeNs !== source.metadata.ctimeNs) {
      throw new Error("maintenance source archive changed before streaming");
    }
    const hash = createHash("sha256");
    const buffer = Buffer.alloc(256 * 1024);
    let offset = 0;

    // stream only the prehashed source bytes
    while (offset < source.sizeBytes) {
      const read = await handle.read(
        buffer,
        0,
        Math.min(buffer.length, source.sizeBytes - offset),
        offset,
      );
      if (read.bytesRead === 0) {
        throw new Error("maintenance source archive ended early");
      }
      const chunk = buffer.subarray(0, read.bytesRead);
      hash.update(chunk);
      await writeChunk(output, chunk);
      offset += read.bytesRead;
    }
    const after = await handle.stat({ bigint: true });
    if (hash.digest("hex") !== source.sha256 || before.dev !== after.dev ||
      before.ino !== after.ino || before.size !== after.size ||
      before.mtimeNs !== after.mtimeNs || before.ctimeNs !== after.ctimeNs) {
      throw new Error("maintenance source archive changed while streaming");
    }
  } finally {
    await handle.close();
  }
  return header;
}

// read one exact descriptor range
async function readV2DescriptorRange(handle, length, position, description) {
  const bytes = Buffer.alloc(length);
  let offset = 0;

  // fill the proven descriptor range
  while (offset < bytes.length) {
    const read = await handle.read(bytes, offset, bytes.length - offset, position + offset);
    if (read.bytesRead === 0) {
      throw new Error(`${description} ended early`);
    }
    offset += read.bytesRead;
  }
  return bytes;
}

// verify one downloaded v2 stream without extracting its payload
export async function verifyAdjustmentEvaluationV2Stream(framePath) {
  const handle = await open(framePath, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const before = await handle.stat({ bigint: true });
    if (!before.isFile() || before.size < BigInt(V2_STREAM_MAGIC.length + 10) ||
      before.size > BigInt(MAX_ARCHIVE_BYTES + V2_MAXIMUM_HEADER_BYTES + 64)) {
      throw new Error("maintenance stream is not a bounded regular file");
    }
    const magic = await readV2DescriptorRange(
      handle,
      V2_STREAM_MAGIC.length,
      0,
      "maintenance stream magic",
    );
    if (!magic.equals(V2_STREAM_MAGIC)) {
      throw new Error("maintenance stream magic differs");
    }
    const lengthOffset = V2_STREAM_MAGIC.length;
    const encodedLength = await readV2DescriptorRange(
      handle,
      9,
      lengthOffset,
      "maintenance stream header length",
    );
    if (!/^[a-f0-9]{8}\n$/u.test(encodedLength.toString("ascii"))) {
      throw new Error("maintenance stream header length is invalid");
    }
    const headerLength = Number.parseInt(encodedLength.toString("ascii").trim(), 16);
    if (headerLength < 2 || headerLength > V2_MAXIMUM_HEADER_BYTES) {
      throw new Error("maintenance stream header exceeds its limit");
    }
    const headerOffset = lengthOffset + encodedLength.length;
    const headerBytes = await readV2DescriptorRange(
      handle,
      headerLength,
      headerOffset,
      "maintenance stream header",
    );
    const header = validateV2StreamHeader(JSON.parse(headerBytes));
    if (!headerBytes.equals(Buffer.from(canonicalJson(header)))) {
      throw new Error("maintenance stream header is not canonical");
    }
    const payloadOffset = headerOffset + headerLength;
    if (BigInt(payloadOffset + header.payloadBytes) !== before.size) {
      throw new Error("maintenance stream framing differs");
    }
    const hash = createHash("sha256");
    const buffer = Buffer.alloc(256 * 1024);
    let offset = 0;

    // verify the framed v1 archive without buffering it
    while (offset < header.payloadBytes) {
      const read = await handle.read(
        buffer,
        0,
        Math.min(buffer.length, header.payloadBytes - offset),
        payloadOffset + offset,
      );
      if (read.bytesRead === 0) {
        throw new Error("maintenance stream payload ended early");
      }
      hash.update(buffer.subarray(0, read.bytesRead));
      offset += read.bytesRead;
    }
    const after = await handle.stat({ bigint: true });
    if (hash.digest("hex") !== header.payloadSha256 || before.dev !== after.dev ||
      before.ino !== after.ino || before.size !== after.size ||
      before.mtimeNs !== after.mtimeNs || before.ctimeNs !== after.ctimeNs) {
      throw new Error("maintenance stream changed or has an invalid payload");
    }
    return { header, headerBytes, payloadBytes: header.payloadBytes, payloadOffset };
  } finally {
    await handle.close();
  }
}

// verify one normalized v2 package against its v1 payload
export async function verifyAdjustmentEvaluationV2EnvelopePackage({
  envelopePath,
  packageRoot,
}) {
  const envelopeBytes = await readNoFollowBoundedFile(
    envelopePath,
    V2_MAXIMUM_HEADER_BYTES,
    "maintenance stream envelope",
  );
  const header = validateV2StreamHeader(JSON.parse(envelopeBytes));
  if (!envelopeBytes.equals(Buffer.from(canonicalJson(header)))) {
    throw new Error("maintenance stream envelope is not canonical");
  }
  const verified = await verifyAdjustmentEvaluationPackage(packageRoot);
  validateCompleteMaintenanceDatabaseManifest(verified.manifest.databaseManifest);
  if (verified.manifest.fromLocalDate !== header.fromLocalDate ||
    verified.manifest.toLocalDate !== header.toLocalDate ||
    verified.manifest.edgeEvidence.watermarkSha256 !==
      header.snapshotLineage.edgeEvidenceWatermarkSha256) {
    throw new Error("maintenance package lineage differs");
  }
  return {
    envelopeSha256: sha256(envelopeBytes),
    manifestSha256: verified.manifestSha256,
    snapshotLineage: header.snapshotLineage,
  };
}

// verify one downloaded value-free availability envelope
export async function verifyAdjustmentConfirmationAvailabilityV2(path) {
  const bytes = await readNoFollowBoundedFile(
    path,
    V2_MAXIMUM_DATABASE_RESPONSE_BYTES,
    "maintenance availability envelope",
  );
  const envelope = validateV2FunctionEnvelope(
    JSON.parse(bytes),
    V2_AVAILABILITY_ENVELOPE_VERSION,
  );
  validateV2AvailabilityPayload(
    envelope.payload,
    envelope.payload.registrationSha256,
  );
  if (!bytes.equals(Buffer.from(canonicalJson(envelope)))) {
    throw new Error("maintenance availability envelope is not canonical");
  }
  return sha256(bytes);
}

// read one bounded registry response
async function readBoundedRegistryResponse(response, maximumBytes, description) {
  if (response.body === null) {
    throw new Error(`${description} response is empty`);
  }
  const chunks = [];
  let bytes = 0;

  // retain only small manifest, config and token responses
  for await (const chunk of Readable.fromWeb(response.body)) {
    bytes += chunk.length;
    if (bytes > maximumBytes) {
      throw new Error(`${description} response exceeds its bound`);
    }
    chunks.push(chunk);
  }
  return Buffer.concat(chunks, bytes);
}

// validate one closed GHCR signed-blob redirect without forwarding credentials
export function weatherRegistryBlobRedirectUrl(location, repository, digest) {
  if (!/^weather-(server|web)$/u.test(repository) ||
    !ADJUSTMENT_RELEASE_DIGEST.test(digest) || typeof location !== "string" ||
    location.length > 4_096) {
    throw new Error("registry blob redirect identity is invalid");
  }
  const redirected = new URL(location);
  const expectedKeys = [
    "hmac", "se", "sig", "ske", "skoid", "sks", "skt", "sktid",
    "skv", "sp", "spr", "sr", "sv",
  ];
  const actualKeys = [...redirected.searchParams.keys()].sort();
  const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/u;

  // accept one exact HTTPS content host, path, and read-only signed query shape
  if (redirected.protocol !== "https:" || redirected.port !== "" ||
    redirected.username !== "" || redirected.password !== "" ||
    redirected.hostname !== "pkg-containers.githubusercontent.com" ||
    !new RegExp(`^/ghcr(?:[0-9]{1,3}|blobs[0-9]{2})/blobs/${digest}$`, "u")
      .test(redirected.pathname) ||
    redirected.hash !== "" ||
    JSON.stringify(actualKeys) !== JSON.stringify(expectedKeys) ||
    redirected.searchParams.get("sp") !== "r" ||
    redirected.searchParams.get("spr") !== "https" ||
    redirected.searchParams.get("sr") !== "b" ||
    redirected.searchParams.get("sks") !== "b" ||
    !/^\d{4}-\d{2}-\d{2}$/u.test(redirected.searchParams.get("skv") ?? "") ||
    !/^\d{4}-\d{2}-\d{2}$/u.test(redirected.searchParams.get("sv") ?? "") ||
    !uuid.test(redirected.searchParams.get("skoid") ?? "") ||
    !uuid.test(redirected.searchParams.get("sktid") ?? "") ||
    !/^[a-f0-9]{64}$/u.test(redirected.searchParams.get("hmac") ?? "") ||
    !/^[A-Za-z0-9+/=]{16,512}$/u.test(redirected.searchParams.get("sig") ?? "") ||
    !Number.isFinite(Date.parse(redirected.searchParams.get("se") ?? "")) ||
    !Number.isFinite(Date.parse(redirected.searchParams.get("ske") ?? "")) ||
    !Number.isFinite(Date.parse(redirected.searchParams.get("skt") ?? ""))) {
    throw new Error("registry blob redirect is outside the closed policy");
  }
  return redirected.href;
}

// fetch one public GHCR object with a bounded anonymous pull token
async function fetchWeatherRegistryObject(repository, kind, digest, accept) {
  if (!/^weather-(server|web)$/u.test(repository) ||
    !ADJUSTMENT_RELEASE_DIGEST.test(digest) || !["blobs", "manifests"].includes(kind)) {
    throw new Error("registry object identity is invalid");
  }
  const url = `https://ghcr.io/v2/anstosa/${repository}/${kind}/${digest}`;
  const headers = { Accept: accept };
  let response = await fetch(url, {
    headers,
    redirect: "manual",
    signal: AbortSignal.timeout(300_000),
  });

  // obtain only an anonymous token for the fixed public repository
  if (response.status === 401) {
    const challenge = response.headers.get("www-authenticate") ?? "";
    const match = /^Bearer realm="(https:\/\/ghcr\.io\/token)",service="ghcr\.io",scope="(repository:anstosa\/weather-(?:server|web):pull)"$/u.exec(challenge);
    if (match === null || match[2] !== `repository:anstosa/${repository}:pull`) {
      throw new Error("registry authentication challenge is invalid");
    }
    await response.body?.cancel();
    const tokenResponse = await fetch(
      `${match[1]}?service=ghcr.io&scope=${encodeURIComponent(match[2])}`,
      { redirect: "error", signal: AbortSignal.timeout(30_000) },
    );
    if (!tokenResponse.ok) {
      throw new Error("registry token request failed");
    }
    const tokenBytes = await readBoundedRegistryResponse(tokenResponse, 16_384, "registry token");
    const token = JSON.parse(tokenBytes.toString("utf8")).token;
    if (typeof token !== "string" || token.length < 1 || token.length > 8_192) {
      throw new Error("registry token is invalid");
    }
    response = await fetch(url, {
      headers: { ...headers, Authorization: `Bearer ${token}` },
      redirect: "manual",
      signal: AbortSignal.timeout(300_000),
    });
  }

  // follow only the fixed signed GHCR blob handoff without its bearer token
  if (response.status === 307) {
    const redirectUrl = weatherRegistryBlobRedirectUrl(
      response.headers.get("location"),
      repository,
      digest,
    );
    if (kind !== "blobs") {
      throw new Error("registry manifest redirect is unsupported");
    }
    await response.body?.cancel();
    response = await fetch(redirectUrl, {
      headers: { Accept: accept },
      redirect: "error",
      signal: AbortSignal.timeout(300_000),
    });
  }
  if (!response.ok) {
    throw new Error(`registry ${kind} request failed`);
  }
  return response;
}

// parse one portable tar size field
function parseReleaseTarSize(header) {
  const bytes = header.subarray(124, 136);
  const text = bytes.toString("ascii").replace(/\0.*$/u, "").trim();

  // reject base-256 and noncanonical archive sizes
  if (!/^[0-7]{1,11}$/u.test(text)) {
    throw new Error("release layer tar size is invalid");
  }
  const size = Number.parseInt(text, 8);
  if (!Number.isSafeInteger(size) || size < 0) {
    throw new Error("release layer tar size is invalid");
  }
  return size;
}

// decode one bounded canonical tar path
function parseReleaseTarPath(bytes, description) {
  const nul = bytes.indexOf(0);
  const end = nul === -1 ? bytes.length : nul;

  // require zero padding and one exact UTF-8 representation
  if (nul !== -1 && bytes.subarray(nul).some((byte) => byte !== 0)) {
    throw new Error(`${description} padding is invalid`);
  }
  const path = bytes.subarray(0, end).toString("utf8");
  if (!Buffer.from(path, "utf8").equals(bytes.subarray(0, end)) ||
    Buffer.byteLength(path) < 1 || Buffer.byteLength(path) > 4_095 ||
    /[\u0000-\u001f\u007f\\]/u.test(path)) {
    throw new Error(`${description} is invalid`);
  }
  return path;
}

// normalize one extraction path without widening archive semantics
function releaseTarPathParts(path) {
  let normalized = path;

  // discard only explicit current-directory prefixes
  while (normalized.startsWith("./")) normalized = normalized.slice(2);
  if (normalized.endsWith("/")) normalized = normalized.slice(0, -1);
  if (normalized.length === 0 || normalized === ".") return [];
  const parts = normalized.split("/");
  if (normalized.startsWith("/") ||
    parts.some((part) => part.length === 0 || part === "." || part === "..")) {
    throw new Error("release layer tar path is unsafe");
  }
  return parts;
}

// parse one bounded local PAX header and return its path override
function parseReleasePaxPath(bytes) {
  let offset = 0;
  let path = null;
  const allowed = new Set([
    "atime", "ctime", "gid", "gname", "linkpath", "mtime", "path", "uid", "uname",
  ]);

  // validate every length-framed PAX record
  while (offset < bytes.length) {
    const space = bytes.indexOf(0x20, offset);
    if (space < offset + 2 || space > offset + 8) {
      throw new Error("release layer PAX framing is invalid");
    }
    const lengthText = bytes.subarray(offset, space).toString("ascii");
    if (!/^[1-9][0-9]{1,6}$/u.test(lengthText)) {
      throw new Error("release layer PAX length is invalid");
    }
    const length = Number.parseInt(lengthText, 10);
    const end = offset + length;
    if (!Number.isSafeInteger(length) || end > bytes.length ||
      bytes[end - 1] !== 0x0a) {
      throw new Error("release layer PAX record is incomplete");
    }
    const record = bytes.subarray(space + 1, end - 1);
    const equals = record.indexOf(0x3d);
    if (equals < 1) throw new Error("release layer PAX record is invalid");
    const key = record.subarray(0, equals).toString("ascii");
    if (!allowed.has(key) || key === "path" && path !== null) {
      throw new Error("release layer PAX keyword is unsupported");
    }

    // retain only the extraction path override
    if (key === "path") {
      path = parseReleaseTarPath(record.subarray(equals + 1), "release layer PAX path");
    }
    offset = end;
  }
  return path;
}

// verify one POSIX/GNU tar header checksum
function requireReleaseTarChecksum(header) {
  const checksumText = header.subarray(148, 156).toString("ascii")
    .replace(/\0.*$/u, "").trim();
  if (!/^[0-7]{1,7}$/u.test(checksumText)) {
    throw new Error("release layer tar checksum is invalid");
  }
  let checksum = 0;

  // treat the checksum field as spaces per the tar contract
  for (let index = 0; index < header.length; index += 1) {
    checksum += index >= 148 && index < 156 ? 0x20 : header[index];
  }
  if (checksum !== Number.parseInt(checksumText, 8)) {
    throw new Error("release layer tar checksum differs");
  }
}

// stream one gzip layer into a conservative filesystem allocation bound
export async function measureReleaseLayer(response, descriptor, expectedDiffId) {
  if (response.body === null || !Number.isSafeInteger(descriptor.size) || descriptor.size < 1 ||
    !ADJUSTMENT_RELEASE_DIGEST.test(descriptor.digest) ||
    !ADJUSTMENT_RELEASE_DIGEST.test(expectedDiffId)) {
    throw new Error("release layer descriptor is invalid");
  }
  const compressedHash = createHash("sha256");
  const uncompressedHash = createHash("sha256");
  let compressedBytes = 0;
  let uncompressedBytes = 0;
  let entryInodes = 0;
  let allocatedBytes = 0;
  let bodyBytes = 0;
  let bodyContentBytes = 0;
  let bodyKind = null;
  let bodyChunks = [];
  let pendingPath = null;
  let pendingLongLink = false;
  let terminatorBlocks = 0;
  let header = Buffer.alloc(0);

  // charge one materialized entry and every possibly implicit parent directory
  const accountPath = (path, size) => {
    const parts = releaseTarPathParts(path);
    const parentCount = Math.max(0, parts.length - 1);
    const addedInodes = 1 + parentCount;
    entryInodes = adjustmentReleaseSum(entryInodes, addedInodes);
    if (entryInodes > ADJUSTMENT_RELEASE_MAXIMUM_LAYER_INODES) {
      throw new Error("release layer inode count exceeds its bound");
    }
    allocatedBytes = adjustmentReleaseSum(
      allocatedBytes,
      adjustmentReleaseAllocated(size),
      adjustmentReleaseAllocated(Buffer.byteLength(path) + 1),
      parentCount * ADJUSTMENT_RELEASE_BLOCK_BYTES,
    );
    if (allocatedBytes > ADJUSTMENT_RELEASE_MAXIMUM_UNPACKED_LAYER_BYTES) {
      throw new Error("release layer allocation exceeds its bound");
    }
  };

  // apply one captured PAX or GNU long-name body to the following header
  const finishExtensionBody = () => {
    if (bodyKind === null) return;
    const bytes = Buffer.concat(bodyChunks);
    if (bodyKind === "x") {
      pendingPath = parseReleasePaxPath(bytes);
    } else {
      if (bytes.length < 2 || bytes.at(-1) !== 0 ||
        bytes.subarray(0, -1).includes(0)) {
        throw new Error("release layer GNU long value is invalid");
      }
      const value = parseReleaseTarPath(bytes.subarray(0, -1),
        `release layer GNU long ${bodyKind === "L" ? "path" : "link"}`);
      if (bodyKind === "L") pendingPath = value;
      else pendingLongLink = true;
    }
    bodyKind = null;
    bodyChunks = [];
    bodyContentBytes = 0;
  };
  const compressedCounter = new Transform({
    // verify exact compressed descriptor bytes while streaming
    transform(chunk, _encoding, callback) {
      compressedBytes += chunk.length;
      if (compressedBytes > descriptor.size) {
        callback(new Error("release layer compressed size differs"));
        return;
      }
      compressedHash.update(chunk);
      callback(null, chunk);
    },
  });
  const tarCounter = new Transform({
    // retain at most one 512-byte tar header
    transform(chunk, _encoding, callback) {
      uncompressedBytes += chunk.length;
      if (uncompressedBytes > ADJUSTMENT_RELEASE_MAXIMUM_UNPACKED_LAYER_BYTES) {
        callback(new Error("release layer unpacked stream exceeds its bound"));
        return;
      }
      uncompressedHash.update(chunk);
      let offset = 0;
      try {
        while (offset < chunk.length) {
          if (bodyBytes > 0) {
            const consumed = Math.min(bodyBytes, chunk.length - offset);
            const content = Math.min(bodyContentBytes, consumed);
            if (bodyKind !== null && content > 0) {
              bodyChunks.push(chunk.subarray(offset, offset + content));
            }
            bodyContentBytes -= content;
            bodyBytes -= consumed;
            offset += consumed;
            if (bodyBytes === 0) finishExtensionBody();
            continue;
          }
          const needed = 512 - header.length;
          const consumed = Math.min(needed, chunk.length - offset);
          header = Buffer.concat([header, chunk.subarray(offset, offset + consumed)]);
          offset += consumed;
          if (header.length < 512) continue;
          const zero = header.every((byte) => byte === 0);
          if (zero) {
            terminatorBlocks += 1;
            header = Buffer.alloc(0);
            continue;
          }
          if (terminatorBlocks > 0) {
            throw new Error("release layer tar contains data after its terminator");
          }
          requireReleaseTarChecksum(header);
          const size = parseReleaseTarSize(header);
          const storedBytes = Math.ceil(size / 512) * 512;
          const type = header[156] === 0 ? "0" : String.fromCharCode(header[156]);
          const magic = header.subarray(257, 263).toString("binary");
          const version = header.subarray(263, 265).toString("binary");
          if (!((magic === "ustar\0" && version === "00") ||
            (magic === "ustar " && version === " \0"))) {
            throw new Error("release layer tar format is unsupported");
          }
          const name = parseReleaseTarPath(header.subarray(0, 100), "release layer tar name");
          const prefixBytes = header.subarray(345, 500);
          const prefix = prefixBytes.every((byte) => byte === 0) ? null :
            parseReleaseTarPath(prefixBytes, "release layer tar prefix");
          const headerPath = prefix === null ? name : `${prefix}/${name}`;

          // capture only bounded local PAX and GNU long-name extensions
          if (["x", "L", "K"].includes(type)) {
            if (pendingPath !== null || pendingLongLink || size < 1 || size > 16_384) {
              throw new Error("release layer tar extension sequence is unsupported");
            }
            accountPath(headerPath, size);
            bodyKind = type;
            bodyContentBytes = size;
            bodyChunks = [];
          } else {
            if (!["0", "1", "2", "3", "4", "5", "6"].includes(type)) {
              throw new Error("release layer tar entry type is unsupported");
            }
            if (pendingLongLink && !["1", "2"].includes(type)) {
              throw new Error("release layer GNU long link is misplaced");
            }
            accountPath(pendingPath ?? headerPath, size);
            pendingPath = null;
            pendingLongLink = false;
          }
          bodyBytes = storedBytes;
          header = Buffer.alloc(0);
        }
        callback();
      } catch (error) {
        callback(error);
      }
    },
    // require a complete terminated tar stream
    flush(callback) {
      if (terminatorBlocks < 2 || header.length !== 0 || bodyBytes !== 0 ||
        bodyKind !== null || pendingPath !== null || pendingLongLink || entryInodes < 1) {
        callback(new Error("release layer tar framing is incomplete"));
        return;
      }
      callback();
    },
  });
  await pipeline(Readable.fromWeb(response.body), compressedCounter, createGunzip(), tarCounter);

  // verify descriptor and diff-id identities after complete consumption
  if (compressedBytes !== descriptor.size ||
    `sha256:${compressedHash.digest("hex")}` !== descriptor.digest ||
    `sha256:${uncompressedHash.digest("hex")}` !== expectedDiffId ||
    uncompressedBytes < 1) {
    throw new Error("release layer digest or size differs");
  }
  return {
    allocatedBytes: adjustmentReleaseAllocated(allocatedBytes),
    blobDigest: descriptor.digest,
    entryInodes,
  };
}

// inspect one pinned image and measure every literal layer without storing it
async function inspectRemoteReleaseImage(reference, layerMeasurements) {
  const match = ADJUSTMENT_RELEASE_REFERENCE.exec(reference);
  if (match === null) {
    throw new Error("release image reference is not pinned to the fixed repository");
  }
  const repository = `weather-${match[1]}`;
  const manifestDigest = `sha256:${match[2]}`;
  const manifestResponse = await fetchWeatherRegistryObject(
    repository,
    "manifests",
    manifestDigest,
    "application/vnd.oci.image.manifest.v1+json",
  );
  const manifestBytes = await readBoundedRegistryResponse(
    manifestResponse,
    1_048_576,
    "release image manifest",
  );
  if (`sha256:${createHash("sha256").update(manifestBytes).digest("hex")}` !== manifestDigest) {
    throw new Error("release image manifest digest differs");
  }
  const manifest = JSON.parse(manifestBytes.toString("utf8"));
  if (!ADJUSTMENT_RELEASE_DIGEST.test(manifest.config?.digest)) {
    throw new Error("release image config descriptor is invalid");
  }
  const configResponse = await fetchWeatherRegistryObject(
    repository,
    "blobs",
    manifest.config.digest,
    "application/vnd.oci.image.config.v1+json",
  );
  const configBytes = await readBoundedRegistryResponse(
    configResponse,
    4_194_304,
    "release image config",
  );
  if (`sha256:${createHash("sha256").update(configBytes).digest("hex")}` !==
    manifest.config.digest) {
    throw new Error("release image config digest differs");
  }
  const config = JSON.parse(configBytes.toString("utf8"));
  if (!Array.isArray(manifest.layers) || !Array.isArray(config.rootfs?.diff_ids) ||
    manifest.layers.length !== config.rootfs.diff_ids.length) {
    throw new Error("release image layers and config differ");
  }
  const compressedImageBytes = manifest.layers.reduce(
    // reject descriptor overflow and unbounded registry reads
    (total, layer) => adjustmentReleaseSum(total, layer.size),
    0,
  );
  if (compressedImageBytes > ADJUSTMENT_RELEASE_MAXIMUM_COMPRESSED_IMAGE_BYTES) {
    throw new Error("release image compressed layers exceed their bound");
  }
  const unpackedLayers = [];

  // fetch one copy of each content-addressed blob measurement
  for (const [index, descriptor] of manifest.layers.entries()) {
    const measurementKey = `${descriptor.digest}/${config.rootfs.diff_ids[index]}`;
    let measurement = layerMeasurements.get(measurementKey);
    if (measurement === undefined) {
      const layerResponse = await fetchWeatherRegistryObject(
        repository,
        "blobs",
        descriptor.digest,
        "application/vnd.oci.image.layer.v1.tar+gzip",
      );
      measurement = await measureReleaseLayer(
        layerResponse,
        descriptor,
        config.rootfs.diff_ids[index],
      );
      layerMeasurements.set(measurementKey, measurement);
    }
    unpackedLayers.push(measurement);
  }
  return {
    configBytes: configBytes.toString("utf8"),
    manifestBytes: manifestBytes.toString("utf8"),
    reference,
    unpackedLayers,
  };
}

// classify one no-follow Docker layer entry and its actual allocation
export function measureReleasePathEntry(details) {
  const overlayWhiteout = details.isCharacterDevice() && details.rdev === 0n;

  // accept only ordinary entries or the exact overlay2 whiteout representation
  if (!details.isDirectory() && !details.isFile() && !details.isSymbolicLink() &&
    !overlayWhiteout) {
    throw new Error("Docker layer contains an unsupported filesystem entry");
  }
  return {
    bytes: adjustmentReleaseAllocated(Number(details.blocks * 512n)),
    descend: details.isDirectory(),
    inodes: 1,
  };
}

// count actual blocks below one Docker layer without following links
function measureReleasePath(path) {
  const details = lstatSync(path, { bigint: true });
  const entry = measureReleasePathEntry(details);
  let bytes = entry.bytes;
  let inodes = entry.inodes;

  // descend through real directories only
  if (entry.descend) {
    for (const name of readdirSync(path)) {
      const child = measureReleasePath(join(path, name));
      bytes = adjustmentReleaseSum(bytes, child.bytes);
      inodes = adjustmentReleaseSum(inodes, child.inodes);
    }
  }
  return { bytes: adjustmentReleaseAllocated(bytes), inodes };
}

// collect actual local Docker parent-chain allocation for one retained image
function inspectLocalReleaseImage(reference, remoteImage) {
  let output;
  try {
    output = execFileSync("docker", ["image", "inspect", "--format", "{{json .}}", reference], {
      encoding: "utf8",
      maxBuffer: 8 * 1_024 * 1_024,
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch {
    return [];
  }
  if (Buffer.byteLength(output) > 8 * 1_024 * 1_024) {
    throw new Error("local Docker image inspection exceeds its bound");
  }
  const inspected = JSON.parse(output);
  const config = JSON.parse(remoteImage.configBytes);
  const paths = [];
  const lower = inspected.GraphDriver?.Data?.LowerDir;
  const upper = inspected.GraphDriver?.Data?.UpperDir;

  // bind local identity, platform and diff-id sequence to registry config
  if (inspected.Id !== JSON.parse(remoteImage.manifestBytes).config.digest ||
    inspected.Architecture !== "arm64" || inspected.Os !== "linux" ||
    inspected.GraphDriver?.Name !== "overlay2" ||
    JSON.stringify(inspected.RootFS?.Layers) !== JSON.stringify(config.rootfs.diff_ids)) {
    throw new Error("local Docker image identity differs from registry config");
  }
  if (typeof lower === "string" && lower.length > 0) {
    paths.push(...lower.split(":").reverse());
  }
  if (typeof upper === "string" && upper.length > 0) {
    paths.push(upper);
  }
  if (paths.length !== config.rootfs.diff_ids.length ||
    paths.some((path) => !path.startsWith("/var/lib/docker/overlay2/") || !path.endsWith("/diff"))) {
    throw new Error("local Docker layer paths are unavailable or unsupported");
  }
  const inventory = [];
  const diffIds = [];

  // measure each literal parent chain once
  for (const [index, diffId] of config.rootfs.diff_ids.entries()) {
    diffIds.push(diffId);
    const measured = measureReleasePath(paths[index]);
    inventory.push({
      allocatedBytes: measured.bytes,
      chainId: dockerChainIdentity(diffIds),
    });
  }
  return inventory;
}

// measure fixed release/control state without following symlinks
function measureReleaseControlRoot() {
  const root = resolve(dirname(import.meta.filename), "../..");
  return measureReleasePath(root).bytes;
}

// collect the literal fixed-scope release inventory
async function collectFixedInertV13ReleaseCapacity(sourceServer, sourceWeb, targetServer, targetWeb) {
  const record = await collectLiteralReleaseCapacity(
    [sourceServer, sourceWeb, targetServer, targetWeb, sourceServer, sourceWeb],
  );
  return evaluateAdjustmentReleaseCapacity(collectAdjustmentReleaseCapacityInventory({
    ...record,
    compensationScope: ADJUSTMENT_RELEASE_COMPENSATION_SCOPE,
    sourceRelease: ADJUSTMENT_RELEASE_SOURCE_RELEASE,
    version: ADJUSTMENT_RELEASE_INVENTORY_VERSION,
  }));
}

// measure only the fixed inactive current-v13 to v14 code transition
async function collectFixedInertV14ReleaseCapacity(sourceServer, sourceWeb, targetServer, targetWeb) {
  const record = await collectLiteralReleaseCapacity(
    [sourceServer, sourceWeb, targetServer, targetWeb, sourceServer, sourceWeb],
  );
  return evaluateAdjustmentInertV14ReleaseCapacity({ ...record,
    compensationScope: ADJUSTMENT_INERT_V14_COMPENSATION_SCOPE,
    sourceRelease: ADJUSTMENT_INERT_V14_SOURCE_RELEASE,
    version: "adjustment-inert-v14-release-inventory/v1" });
}

// measure only the full maintenance handoff from the published bridge
async function collectFixedFullV14ReleaseCapacity(sourceServer, sourceWeb, targetServer, targetWeb) {
  const record = await collectLiteralReleaseCapacity(
    [sourceServer, sourceWeb, targetServer, targetWeb, sourceServer, sourceWeb],
  );
  return evaluateAdjustmentFullV14ReleaseCapacity({
    ...record,
    compensationScope: ADJUSTMENT_FULL_V14_COMPENSATION_SCOPE,
    sourceRelease: ADJUSTMENT_FULL_V14_SOURCE_RELEASE,
    version: "adjustment-full-v14-release-inventory/v1",
  });
}

// measure dashboard growth while retaining the literal core and source web
async function collectDashboardWebReleaseCapacity(sourceWeb, targetWeb) {
  // reject caller-shaped repositories before network or filesystem inspection
  if (ADJUSTMENT_RELEASE_REFERENCE.exec(sourceWeb)?.[1] !== "web" ||
    ADJUSTMENT_RELEASE_REFERENCE.exec(targetWeb)?.[1] !== "web" || sourceWeb === targetWeb) {
    throw new Error("dashboard web capacity references are invalid");
  }
  const server = ADJUSTMENT_INERT_V14_SOURCE_REFERENCES.get("server");
  const record = await collectLiteralReleaseCapacity([server, sourceWeb, server, targetWeb, server, sourceWeb]);
  return evaluateDashboardWebReleaseCapacity({ ...record,
    compensationScope: DASHBOARD_WEB_RELEASE_COMPENSATION_SCOPE,
    sourceRelease: ADJUSTMENT_INERT_V14_SOURCE_RELEASE,
    version: DASHBOARD_WEB_RELEASE_INVENTORY_VERSION });
}

// measure six literal images for one source-preserving family transaction
async function collectFamilyReleaseCapacity(actionSha256, family, sourceRelease, ...references) {
  // reject malformed scope before registry or host inventory work
  if (!/^[a-f0-9]{64}$/u.test(actionSha256) ||
    !["rain", "temperature", "wind"].includes(family) ||
    !/^\d{4}\.\d{2}\.\d{2}-[1-9]\d*$/u.test(sourceRelease) ||
    references.length !== 6 || references.some(
      // require alternating immutable application repositories
      (reference, index) => ADJUSTMENT_RELEASE_REFERENCE.exec(reference)?.[1] !==
        (index % 2 === 0 ? "server" : "web"),
    )) {
    throw new Error("family release capacity arguments are invalid");
  }
  const record = await collectLiteralReleaseCapacity(references);
  return evaluateAdjustmentFamilyReleaseCapacity({ ...record, actionSha256, family, sourceRelease,
    compensationScope: ADJUSTMENT_FAMILY_RELEASE_COMPENSATION_SCOPE,
    version: ADJUSTMENT_FAMILY_RELEASE_INVENTORY_VERSION });
}

// keep physical OCI measurements independent of compensation policy
async function collectLiteralReleaseCapacity(references) {
  const [sourceServer, sourceWeb, targetServer, targetWeb, compensatingServer, compensatingWeb] = references;
  const layerMeasurements = new Map();
  const remote = new Map();

  // bind all unique source and target image bytes from GHCR
  for (const reference of references) {
    if (!remote.has(reference)) {
      remote.set(reference, await inspectRemoteReleaseImage(reference, layerMeasurements));
    }
  }
  const sourceImages = [remote.get(sourceServer), remote.get(sourceWeb)];
  const targetImages = [remote.get(targetServer), remote.get(targetWeb)];
  const compensatingImages = [remote.get(compensatingServer), remote.get(compensatingWeb)];
  const inventoryByChain = new Map();

  // merge one exact locally allocated image into the physical inventory
  const mergeLocalImage = (image, required) => {
    const local = inspectLocalReleaseImage(image.reference, image);
    if (required && local.length === 0) {
      throw new Error("release source image is not locally measurable");
    }
    for (const layer of local) {
      const old = inventoryByChain.get(layer.chainId);
      if (old !== undefined && old !== layer.allocatedBytes) {
        throw new Error("local Docker layer allocation differs across release images");
      }
      inventoryByChain.set(layer.chainId, layer.allocatedBytes);
    }
  };

  // source must exist; an already-pulled target is measured without baseline reset
  for (const image of sourceImages) mergeLocalImage(image, true);
  for (const image of targetImages) mergeLocalImage(image, false);
  for (const image of compensatingImages) mergeLocalImage(image, false);
  const filesystem = statfsSync("/var/lib/weather", { bigint: true });
  const freeBytesBig = filesystem.bavail * filesystem.bsize;
  if (freeBytesBig > BigInt(Number.MAX_SAFE_INTEGER) || filesystem.ffree > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error("release filesystem capacity exceeds exact integer precision");
  }
  const roleImage = (role, runtime, reference) => ({
    ...structuredClone(remote.get(reference)),
    role,
    runtime,
  });
  const record = {
    freeBytes: Number(freeBytesBig),
    freeInodes: Number(filesystem.ffree),
    images: [
      roleImage("source", "server", sourceServer),
      roleImage("source", "web", sourceWeb),
      roleImage("target", "server", targetServer),
      roleImage("target", "web", targetWeb),
      roleImage("compensating", "server", compensatingServer),
      roleImage("compensating", "web", compensatingWeb),
    ],
    inventory: [...inventoryByChain.entries()].map(([chainId, allocatedBytes]) => ({
      allocatedBytes,
      chainId,
    })).sort((left, right) => left.chainId.localeCompare(right.chainId)),
    measuredAt: new Date().toISOString(),
    retainedControlBytes: measureReleaseControlRoot(),
    runtimePackageBytes: 0,
  };
  return record;
}

// cap an archive stream without host-side publication
async function capArchive(input = process.stdin, output = process.stdout) {
  let bytes = 0;

  // forward only within the reviewed archive limit
  for await (const chunk of input) {
    bytes += chunk.length;
    if (bytes > MAX_ARCHIVE_BYTES) {
      throw new Error("compressed archive exceeds the package limit");
    }
    await writeChunk(output, chunk);
  }
}

// run the narrow package CLI
async function main() {
  const [command, ...argumentsList] = process.argv.slice(2);

  // build from a streamed database transaction
  if (command === "build") {
    if (argumentsList.length !== 4) {
      throw new Error("usage: adjustment-evaluation-package.mjs build PACKAGE_ROOT FROM TO EDGE_SNAPSHOT");
    }
    const [packageRoot, fromLocalDate, toLocalDate, edgeSnapshotPath] = argumentsList;
    const manifest = await buildAdjustmentEvaluationPackage({
      edgeSnapshotPath,
      fromLocalDate,
      packageRoot,
      toLocalDate,
    });
    process.stderr.write(`manifest=${manifest}\n`);
    return;
  }

  // stream the production archive with only one staged row member
  if (command === "stream-archive") {
    if (argumentsList.length !== 4) {
      throw new Error("usage: adjustment-evaluation-package.mjs stream-archive TEMP_ROOT FROM TO EDGE_SNAPSHOT");
    }
    const [temporaryRoot, fromLocalDate, toLocalDate, edgeSnapshotPath] = argumentsList;
    const manifest = await streamAdjustmentEvaluationPackageArchive({
      edgeSnapshotPath,
      fromLocalDate,
      temporaryRoot,
      toLocalDate,
    });
    process.stderr.write(`manifest=${manifest}\n`);
    return;
  }

  // frame one function-only availability response
  if (command === "frame-v2-availability") {
    if (argumentsList.length !== 1) {
      throw new Error("usage: adjustment-evaluation-package.mjs frame-v2-availability REGISTRATION_SHA256");
    }
    const bytes = await frameAdjustmentConfirmationAvailabilityV2({
      registrationSha256: argumentsList[0],
    });
    await writeChunk(process.stdout, bytes);
    return;
  }

  // require exact v2 ledger readiness without adding data authority
  if (command === "verify-v2-readiness") {
    if (argumentsList.length !== 0) {
      throw new Error("usage: adjustment-evaluation-package.mjs verify-v2-readiness");
    }
    const response = await readV2DatabaseResponse();
    if (response.payload !== null) {
      throw new Error("maintenance readiness payload must be null");
    }
    return;
  }

  // frame one persisted-burn authorization response
  if (command === "frame-v2-authorization") {
    if (argumentsList.length !== 3 || !/^(?:0|[1-9][0-9]?)$/u.test(argumentsList[2])) {
      throw new Error("usage: adjustment-evaluation-package.mjs frame-v2-authorization REGISTRATION_SHA256 ACCESS_SHA256 CHUNK_INDEX");
    }
    const bytes = await frameAdjustmentConfirmationAuthorizationV2({
      accessSha256: argumentsList[1],
      chunkIndex: Number(argumentsList[2]),
      registrationSha256: argumentsList[0],
    });
    await writeChunk(process.stdout, bytes);
    return;
  }

  // print the authorized inclusive legacy observation interval
  if (command === "v2-authorization-dates") {
    if (argumentsList.length !== 1) {
      throw new Error("usage: adjustment-evaluation-package.mjs v2-authorization-dates AUTHORIZATION_ENVELOPE");
    }
    const authorization = await readV2AuthorizationEnvelope(argumentsList[0]);
    process.stdout.write(
      `${authorization.payload.fromLocalDate} ${addV2CalendarDays(
        authorization.payload.toLocalDateExclusive,
        -1,
      )}\n`,
    );
    return;
  }

  // stream one prehashed v2 envelope around the unchanged v1 package
  if (command === "stream-v2-envelope") {
    if (argumentsList.length !== 5 && argumentsList.length !== 6) {
      throw new Error("usage: adjustment-evaluation-package.mjs stream-v2-envelope KIND SOURCE_ARCHIVE FROM TO EDGE_SNAPSHOT [AUTHORIZATION_ENVELOPE]");
    }
    const [exportKind, sourceArchivePath, fromLocalDate, toLocalDate,
      edgeSnapshotPath, authorizationPath] = argumentsList;
    await streamAdjustmentEvaluationV2EnvelopeArchive({
      authorizationPath: authorizationPath ?? null,
      edgeSnapshotPath,
      exportKind,
      fromLocalDate,
      sourceArchivePath,
      toLocalDate,
    });
    return;
  }

  // verify one bounded framed stream and publish its canonical header
  if (command === "verify-v2-stream") {
    if (argumentsList.length !== 2) {
      throw new Error("usage: adjustment-evaluation-package.mjs verify-v2-stream FRAME ENVELOPE_OUTPUT");
    }
    const verified = await verifyAdjustmentEvaluationV2Stream(argumentsList[0]);
    await writeFile(argumentsList[1], verified.headerBytes, { flag: "wx", mode: 0o600 });
    process.stdout.write(`${verified.payloadOffset} ${verified.payloadBytes}\n`);
    return;
  }

  // verify one normalized local v2 package
  if (command === "verify-v2-envelope") {
    if (argumentsList.length !== 2) {
      throw new Error("usage: adjustment-evaluation-package.mjs verify-v2-envelope ENVELOPE PACKAGE_ROOT");
    }
    const verified = await verifyAdjustmentEvaluationV2EnvelopePackage({
      envelopePath: argumentsList[0],
      packageRoot: argumentsList[1],
    });
    process.stdout.write(`${verified.envelopeSha256}\n`);
    return;
  }

  // verify one normalized value-free availability envelope
  if (command === "verify-v2-availability") {
    if (argumentsList.length !== 1) {
      throw new Error("usage: adjustment-evaluation-package.mjs verify-v2-availability ENVELOPE");
    }
    process.stdout.write(
      `${await verifyAdjustmentConfirmationAvailabilityV2(argumentsList[0])}\n`,
    );
    return;
  }

  // verify one extracted package
  if (command === "verify") {
    if (argumentsList.length !== 1) {
      throw new Error("usage: adjustment-evaluation-package.mjs verify PACKAGE_ROOT");
    }
    const verified = await verifyAdjustmentEvaluationPackage(argumentsList[0]);
    process.stdout.write(`${verified.manifestSha256}\n`);
    return;
  }

  // evaluate one canonical normal stage or activation capacity document
  if (command === "normal-stage-capacity" || command === "normal-activation-capacity") {
    if (argumentsList.length !== 0) throw new Error(`${command} takes no arguments`);
    const bytes = await readBoundedScheduleMetadata(16 * 1_024);
    const input = JSON.parse(bytes.toString("utf8"));

    // reject noncanonical or multiply framed capacity evidence
    if (!bytes.equals(Buffer.from(canonicalJson(input)))) {
      throw new Error("normal capacity input is not canonical");
    }
    const receipt = command === "normal-stage-capacity"
      ? await collectNormalStageCapacity(input)
      : await collectNormalActivationCapacity(input);
    process.stdout.write(canonicalJson(receipt));

    // preserve the complete refusal receipt while returning a failing gate
    if (receipt.state !== "capacity_ready") process.exitCode = 3;
    return;
  }

  // collect and enforce the fixed inert v13 literal image capacity
  if (command === "release-capacity") {
    if (argumentsList.length !== 4) {
      throw new Error("usage: adjustment-evaluation-package.mjs release-capacity SOURCE_SERVER SOURCE_WEB TARGET_SERVER TARGET_WEB");
    }
    const receipt = await collectFixedInertV13ReleaseCapacity(...argumentsList);
    process.stdout.write(`${canonicalJsonValue(receipt)}\n`);

    // preserve the complete refusal receipt while returning a failing gate
    if (receipt.state !== "capacity_ready") {
      process.exitCode = 3;
    }
    return;
  }

  // meter the separate fixed inactive code handoff without widening v13 recovery
  if (command === "inert-v14-release-capacity") {
    // keep the source and target image argument grammar fixed
    if (argumentsList.length !== 4) {
      throw new Error("usage: adjustment-evaluation-package.mjs inert-v14-release-capacity SOURCE_SERVER SOURCE_WEB TARGET_SERVER TARGET_WEB");
    }
    const receipt = await collectFixedInertV14ReleaseCapacity(...argumentsList);
    process.stdout.write(`${canonicalJsonValue(receipt)}\n`);
    // a complete refusal receipt remains a failing gate
    if (receipt.state !== "capacity_ready") process.exitCode = 3;
    return;
  }

  // meter the full handoff from the separately published bridge
  if (command === "full-v14-release-capacity") {
    // keep the source and target image argument grammar fixed
    if (argumentsList.length !== 4) {
      throw new Error("usage: adjustment-evaluation-package.mjs full-v14-release-capacity SOURCE_SERVER SOURCE_WEB TARGET_SERVER TARGET_WEB");
    }
    const receipt = await collectFixedFullV14ReleaseCapacity(...argumentsList);
    process.stdout.write(`${canonicalJsonValue(receipt)}\n`);
    // a complete refusal receipt remains a failing gate
    if (receipt.state !== "capacity_ready") process.exitCode = 3;
    return;
  }

  // verify only the independent dashboard publication
  if (command === "web-release-proof") {
    // accept identities rather than paths or arbitrary repositories
    if (argumentsList.length !== 2) {
      throw new Error("usage: adjustment-evaluation-package.mjs web-release-proof TARGET_RELEASE EXPECTED_CORE_RELEASE");
    }
    const receipt = await verifyDashboardWebGitRelease(...argumentsList);
    process.stdout.write(`${canonicalJsonValue(receipt)}\n`);
    return;
  }

  // preserve the protected floors for the web-only source restoration lane
  if (command === "web-release-capacity") {
    // retain the exact two-reference grammar
    if (argumentsList.length !== 2) {
      throw new Error("usage: adjustment-evaluation-package.mjs web-release-capacity SOURCE_WEB TARGET_WEB");
    }
    const receipt = await collectDashboardWebReleaseCapacity(...argumentsList);
    process.stdout.write(`${canonicalJsonValue(receipt)}\n`);
    // report the measured refusal without earning admission
    if (receipt.state !== "capacity_ready") process.exitCode = 3;
    return;
  }

  // collect distinct source, target and new family compensation images
  if (command === "family-release-capacity") {
    // keep every privileged argument a closed identity rather than a path
    if (argumentsList.length !== 9) {
      throw new Error("usage: adjustment-evaluation-package.mjs family-release-capacity ACTION_SHA256 FAMILY SOURCE_RELEASE SOURCE_SERVER SOURCE_WEB TARGET_SERVER TARGET_WEB COMPENSATING_SERVER COMPENSATING_WEB");
    }
    const receipt = await collectFamilyReleaseCapacity(...argumentsList);
    process.stdout.write(`${canonicalJsonValue(receipt)}\n`);

    // return the complete refusal receipt without authorizing a partial transaction
    if (receipt.state !== "capacity_ready") process.exitCode = 3;
    return;
  }

  // validate a bounded metadata-only migration manifest
  if (command === "verify-maintenance-ledger-v13-compatibility" ||
    command === "verify-maintenance-ledger-v14" ||
    command === "verify-maintenance-ledger-v14-rolling" ||
    command === "verify-maintenance-ledger-v15-catalog" ||
    command === "project-maintenance-ledger-v3") {
    if (argumentsList.length !== 0) throw new Error(`${command} takes no arguments`);
    const chunks = [];
    let size = 0;
    // cap stdin before parsing any caller-controlled JSON
    for await (const chunk of process.stdin) {
      size += chunk.length;
      if (size > 64 * 1_024) throw new Error("migration manifest exceeds its bound");
      chunks.push(chunk);
    }
    const manifest = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    // keep metadata projection separate from both exact manifest-only verifier domains
    if (command === "project-maintenance-ledger-v3") {
      process.stdout.write(canonicalJson(projectAdjustmentMaintenanceDatabaseLedgerV3(manifest)));
    } else if (command === "verify-maintenance-ledger-v13-compatibility") {
      validateAdjustmentV13CompatibilityDatabaseManifest(manifest);
    } else if (command === "verify-maintenance-ledger-v14-rolling") {
      validateAdjustmentV14RollingActivationDatabaseManifest(manifest);
    } else if (command === "verify-maintenance-ledger-v15-catalog") {
      validateAdjustmentV15CatalogDatabaseManifest(manifest);
    } else {
      validateAdjustmentV14DatabaseManifest(manifest);
    }
    return;
  }

  // construct only the epoch-bound closed database owner call
  if (command === "registration-schedule-initialize-sql-v3") {
    if (argumentsList.length !== 1) throw new Error("schedule initializer requires one hash");
    process.stdout.write(buildAdjustmentRegistrationScheduleInitializationSql({
      bootstrapBytes: await readBoundedScheduleMetadata(2_048),
      bootstrapSha256: argumentsList[0],
      witness: await readAdjustmentRevisionCaptureEpochWitness(),
    }));
    return;
  }

  // validate the fixed owner function result before exposing a successful retry
  if (command === "verify-registration-schedule-initialization-v3") {
    if (argumentsList.length !== 1 || !HASH_PATTERN.test(argumentsList[0])) {
      throw new Error("schedule result requires one hash");
    }
    const result = JSON.parse((await readBoundedScheduleMetadata(4_096)).toString("utf8"));
    requireExactKeys(requireObject(result, "schedule initialization result"),
      ["bootstrapSha256", "initialized", "scheduleContractSha256"], "schedule initialization result");
    // require the authenticated bootstrap rather than an arbitrary successful database result
    if (result.bootstrapSha256 !== argumentsList[0] || typeof result.initialized !== "boolean" ||
      result.scheduleContractSha256 !== ROLLING_SCHEDULE_SHA256) {
      throw new Error("schedule initialization result differs");
    }
    process.stdout.write(canonicalJson(result));
    return;
  }

  // expose one genuine finite schedule head from a read-only transaction
  if (command === "project-registration-schedule-status-v3") {
    if (argumentsList.length !== 0) throw new Error("schedule status takes no arguments");
    const envelope = JSON.parse((await readBoundedScheduleMetadata(64 * 1_024)).toString("utf8"));
    process.stdout.write(canonicalJson(projectAdjustmentRegistrationScheduleStatusV3(
      envelope, await readAdjustmentRevisionCaptureEpochWitness(),
    )));
    return;
  }

  // emit only the fixed create-once rain target source transaction
  if (command === "rain-fixed-gauge-target-sources-sql-v1") {
    if (argumentsList.length !== 0) throw new Error("rain target source SQL takes no arguments");
    process.stdout.write(buildAdjustmentRainFixedGaugeTargetSourceInitializationSql());
    return;
  }

  // close the actual source identities returned by the owner transaction
  if (command === "project-rain-fixed-gauge-target-sources-v1") {
    if (argumentsList.length !== 0) throw new Error("rain target source result takes no arguments");
    const value = JSON.parse((await readBoundedScheduleMetadata(16 * 1_024)).toString("utf8"));
    process.stdout.write(canonicalJson(validateAdjustmentRainFixedGaugeTargetSourceCatalog(value)));
    return;
  }

  // bind the one-time bootstrap to independently verified exact public CI
  if (command === "verify-inert-v14-release" ||
    command === "verify-v14-compatibility-bridge-release") {
    if (argumentsList.length !== 1) throw new Error(`${command} requires one release`);
    process.stdout.write(canonicalJson(
      await verifyAdjustmentV14CompatibilityBridgeGitRelease(argumentsList[0]),
    ));
    return;
  }

  // bind the full release to the published compatibility bridge
  if (command === "verify-full-v14-release") {
    if (argumentsList.length !== 1) throw new Error("verify-full-v14-release requires one release");
    process.stdout.write(canonicalJson(await verifyAdjustmentFullV14GitRelease(argumentsList[0])));
    return;
  }

  // emit only one closed owner operation after independent root authority checks
  if (command === "owner-operation-sql-v3") {
    const [kind, expectedSha256] = argumentsList;
    const validators = { access: validateAdjustmentConfirmationAccessBurnRequestV3,
      terminal: validateAdjustmentShadowTerminalRecordRequestV3,
      retire: validateAdjustmentShadowTerminalRetirementRequestV3,
      "unsupported-terminal": validateAdjustmentShadowUnsupportedTerminalRecordRequestV1,
      "unsupported-retire": validateAdjustmentShadowUnsupportedTerminalRetirementRequestV1 };
    if (argumentsList.length !== 2 || !Object.hasOwn(validators, kind) || !HASH_PATTERN.test(expectedSha256 ?? "")) {
      throw new Error("owner operation requires one closed kind and request identity");
    }
    const bytes = await readBoundedScheduleMetadata(128 * 1_024);
    const request = validators[kind](JSON.parse(bytes.toString("utf8")));
    if (!bytes.equals(Buffer.from(canonicalJson(request))) || sha256(bytes) !== expectedSha256) {
      throw new Error("owner operation canonical request differs");
    }
    const completedAt = await verifyAdjustmentOwnerRequestAuthorityV3(kind, request);
    const sql = kind === "access" ? buildAdjustmentConfirmationAccessBurnSqlV3(request, expectedSha256)
      : kind === "terminal" ? buildAdjustmentShadowTerminalRecordSqlV3(request, expectedSha256, completedAt)
        : kind === "unsupported-terminal"
          ? buildAdjustmentShadowUnsupportedTerminalRecordSqlV1(request, expectedSha256, completedAt)
          : kind === "unsupported-retire"
            ? buildAdjustmentShadowUnsupportedTerminalRetirementSqlV1(request, expectedSha256)
            : buildAdjustmentShadowTerminalRetirementSqlV3(request, expectedSha256);
    process.stdout.write(sql);
    return;
  }

  // close genuine owner query output before returning a canonical metadata response
  if (command === "owner-operation-result-v3") {
    if (argumentsList.length !== 1 || !["access", "terminal", "retire"].includes(argumentsList[0])) {
      throw new Error("owner result requires one closed operation kind");
    }
    const value = requireObject(JSON.parse((await readBoundedScheduleMetadata(128 * 1_024)).toString("utf8")), "owner result");
    const kind = argumentsList[0];
    const keys = kind === "access" ? ["contractVersion", "localAccessSha256", "nativeAccess", "nativeAccessSha256", "state"]
      : kind === "terminal" ? ["contractVersion", "reconciliationSha256", "registrationSha256", "state", "terminalRecord"]
        : ["contractVersion", "reconciliationSha256", "registrationSha256", "state"];
    requireExactKeys(value, keys, "owner result");
    const version = kind === "access" ? "adjustment-confirmation-access-burn-result/v3"
      : `adjustment-shadow-terminal-${kind === "terminal" ? "record" : "retirement"}-result/v3`;
    if (value.contractVersion !== version || value.state !==
      ({ access: "burned", terminal: "recorded", retire: "retired" })[kind]) throw new Error("owner result state differs");
    if (kind === "access") {
      validateOwnerNativeAccess(value.nativeAccess);
      requireOwnerHash(value.localAccessSha256, "owner local burn identity");
      if (value.nativeAccessSha256 !== value.nativeAccess.accessSha256) throw new Error("owner native access identity differs");
    } else {
      requireOwnerHash(value.reconciliationSha256, "owner reconciliation identity");
      requireOwnerHash(value.registrationSha256, "owner registration identity");
      if (kind === "terminal") {
        validateOwnerTerminalRecord(value.terminalRecord);
        if (value.terminalRecord.reconciliationSha256 !== value.reconciliationSha256 ||
          value.terminalRecord.registrationSha256 !== value.registrationSha256) throw new Error("owner terminal result differs");
      }
    }
    process.stdout.write(canonicalJson(value));
    return;
  }

  // canonicalize genuine owner JSON before durable preparation or consumption
  if (["prepare-shadow-metadata-custody-v1", "consume-shadow-metadata-custody-v1"].includes(command)) {
    if (argumentsList.length !== 1 || !HASH_PATTERN.test(argumentsList[0])) {
      throw new Error("shadow metadata operation requires one exact identity");
    }
    const input = JSON.parse((await readBoundedScheduleMetadata(16 * 1_024)).toString("utf8"));
    const store = new AdjustmentShadowMetadataCustodyProofStore();
    if (command === "prepare-shadow-metadata-custody-v1") {
      if (input.proofSha256 !== argumentsList[0]) throw new Error("shadow metadata preparation proof differs");
      process.stdout.write(canonicalJson(await store.prepareFinalizations(input)));
      return;
    }
    if (input.preparationSha256 !== argumentsList[0]) throw new Error("shadow metadata consumption preparation differs");
    process.stdout.write(canonicalJson(await store.consume(input)));
    return;
  }

  // inspect only the exact proof preparation without accepting a file path
  if (["shadow-metadata-preparation-identity-v1", "shadow-metadata-preparation-sql-v1",
    "shadow-metadata-finalization-sql-v1"].includes(command)) {
    if (argumentsList.length !== 1 || !HASH_PATTERN.test(argumentsList[0])) {
      throw new Error("shadow metadata query requires one exact identity");
    }
    const status = validateAdjustmentShadowMetadataCustodyStatus(
      await new AdjustmentShadowMetadataCustodyProofStore().readStatus(),
    );
    if (status.proof === null) throw new Error("shadow metadata custody proof is unavailable");
    if (command === "shadow-metadata-finalization-sql-v1") {
      if (status.preparation?.preparationSha256 !== argumentsList[0]) {
        throw new Error("shadow metadata custody preparation differs");
      }
      process.stdout.write(buildAdjustmentShadowMetadataFinalizationSql(status.preparation));
      return;
    }
    if (status.proof.proofSha256 !== argumentsList[0]) {
      throw new Error("shadow metadata custody proof differs");
    }
    if (command === "shadow-metadata-preparation-identity-v1") {
      process.stdout.write(`${status.preparation?.preparationSha256 ?? "unprepared"}\n`);
      return;
    }
    if (status.preparation !== null) throw new Error("shadow metadata preparation already exists");
    process.stdout.write(buildAdjustmentShadowMetadataPreparationSql(status.proof));
    return;
  }

  // emit only the fixed owner read-only query without accepting SQL or paths
  if (command === "registration-lifecycle-status-sql-v4") {
    if (argumentsList.length !== 0) throw new Error("registration lifecycle query takes no arguments");
    process.stdout.write(buildAdjustmentRegistrationLifecycleStatusSqlV4());
    return;
  }

  // close the actual owner transaction against the retained epoch witness
  if (command === "project-registration-lifecycle-status-v4") {
    if (argumentsList.length !== 0) throw new Error("registration lifecycle projection takes no arguments");
    const bytes = await readBoundedScheduleMetadata(64 * 1_024);
    const witness = await readAdjustmentRevisionCaptureEpochWitness();
    process.stdout.write(canonicalJson(projectAdjustmentRegistrationLifecycleStatusV4(
      JSON.parse(bytes.toString("utf8")), witness)));
    return;
  }

  // return only a validated settings inode and exact-byte identity
  if (command === "inert-v14-settings-snapshot") {
    if (argumentsList.length !== 0) throw new Error("settings snapshot takes no arguments");
    const snapshot = await readAdjustmentInertV14SettingsSnapshot();
    process.stdout.write(`${snapshot.identity} ${snapshot.sha256}\n`);
    return;
  }

  // recheck the frozen inode and bytes immediately before root publication
  if (command === "verify-inert-v14-settings-snapshot") {
    if (argumentsList.length !== 2 || !/^\d+:\d+$/u.test(argumentsList[0]) ||
      !HASH_PATTERN.test(argumentsList[1])) throw new Error("settings snapshot identity is invalid");
    const current = await readAdjustmentInertV14SettingsSnapshot();
    if (current.identity !== argumentsList[0] || current.sha256 !== argumentsList[1]) {
      throw new Error("inert v14 operator settings changed during startup");
    }
    return;
  }

  // derive source restoration authority from the actual database ledger
  if (command === "inert-v14-restoration-schema") {
    if (argumentsList.length !== 6) throw new Error("restoration schema requires six identities");
    const [actualHistorySha256, sourceHistorySha256, sourceRelease, sourceSchemaRelease,
      targetHistorySha256, targetRelease] = argumentsList;
    process.stdout.write(`${selectAdjustmentInertV14RestorationSchema({
      actualHistorySha256, sourceHistorySha256, sourceRelease, sourceSchemaRelease,
      targetHistorySha256, targetRelease,
    })}\n`);
    return;
  }

  // publish only the root-derived literal inactive handoff identities
  if (command === "bootstrap-inert-v14-current") {
    if (argumentsList.length !== 6) throw new Error("bootstrap-inert-v14-current requires six literal identities");
    const [release, commit, serverImage, webImage, catalogSha256, settingsSha256] = argumentsList;
    const settings = await readAdjustmentInertV14SettingsSnapshot();
    // reread the protected settings bytes at the root publication boundary
    if (settings.sha256 !== settingsSha256) throw new Error("bootstrap settings identity differs");
    process.stdout.write(canonicalJson(await bootstrapAdjustmentFamilyReleaseCurrent({
      catalogSha256, commit, release, serverImage, settingsSha256, webImage,
    })));
    return;
  }

  // verify but never create the root current needed by a committed full release
  if (command === "verify-inert-v14-current") {
    if (argumentsList.length !== 4) throw new Error("verify-inert-v14-current requires four identities");
    const [release, serverImage, webImage, catalogSha256] = argumentsList;
    process.stdout.write(canonicalJson(await verifyAdjustmentInertV14BootstrapCurrent({
      catalogSha256, release, serverImage, webImage,
    })));
    return;
  }

  // reconcile only the exact actionless failed bridge after source health restoration
  if (command === "discard-failed-inert-v14-current") {
    if (argumentsList.length !== 6) throw new Error("discard-failed-inert-v14-current requires six identities");
    const [release, commit, serverImage, webImage, catalogSha256, settingsSha256] = argumentsList;
    process.stdout.write(`${await discardFailedAdjustmentInertV14Bootstrap({
      catalogSha256, commit, release, serverImage, settingsSha256, webImage,
    })}\n`);
    return;
  }

  // execute one root-locked fixed nine-operand family transaction
  if (command === "family-release") {
    if (argumentsList.length !== 9) {
      throw new Error("usage: adjustment-evaluation-package.mjs family-release TARGET_RELEASE COMPENSATING_RELEASE EXPECTED_CURRENT_RELEASE EXPECTED_SOURCE_RELEASE EXPECTED_SETTINGS_SHA256 FAMILY ACTION_SHA256 REPORT_SHA256 FENCE");
    }
    const status = await runAdjustmentFamilyReleaseCommand(argumentsList);
    process.stdout.write(canonicalJson(status));
    return;
  }

  // report one sanitized fixed-root family transaction
  if (command === "family-release-status") {
    if (argumentsList.length !== 1) {
      throw new Error("usage: adjustment-evaluation-package.mjs family-release-status ACTION_SHA256");
    }
    const status = await readAdjustmentFamilyReleaseStatus(argumentsList[0]);
    process.stdout.write(canonicalJson(status));
    return;
  }

  // report one sanitized current family source authority
  if (command === "family-release-current") {
    if (argumentsList.length !== 1) {
      throw new Error("usage: adjustment-evaluation-package.mjs family-release-current FAMILY");
    }
    const status = await readAdjustmentFamilyReleaseCurrent(argumentsList[0]);
    process.stdout.write(canonicalJson(status));
    return;
  }

  // report only the transitive root journal proof back to the capture runner
  if (command === "family-release-lineage-v2") {
    if (argumentsList.length !== 0) {
      throw new Error("family-release-lineage-v2 takes no arguments");
    }
    process.stdout.write(canonicalJson(await readAdjustmentFamilyReleaseLineage()));
    return;
  }

  // enforce the compressed archive egress cap
  if (command === "cap-archive") {
    if (argumentsList.length !== 0) {
      throw new Error("usage: adjustment-evaluation-package.mjs cap-archive");
    }
    await capArchive();
    return;
  }

  throw new Error("unknown adjustment evaluation package command");
}

// avoid running during library import
if (process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  main().catch((error) => {
    process.stderr.write(`error: ${error.message}\n`);
    process.exitCode = 1;
  });
}
