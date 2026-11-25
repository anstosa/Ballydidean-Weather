//! Positive typed-decoder coverage using the compiled predecessor projection encoders.

use std::{path::Path, process::Command};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use weather_contracts::{
    AdjustmentRevisionProjectionDocument, AdjustmentRevisionProjectionKind, Nullable,
};

use super::{ApiMaintenanceRevisionTransportDocument, decode_native_api_revision_document};

/// Compile valid single, grouped, and rich native-temperature bytes with the predecessor encoder.
fn predecessor_projection_bytes() -> Vec<Vec<u8>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(3).unwrap();
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURES])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "predecessor fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<Vec<String>>(&output.stdout)
        .unwrap()
        .into_iter()
        .map(|encoded| STANDARD.decode(encoded).unwrap())
        .collect()
}

/// Retain every generated pointer field and richer state value from canonical predecessor bytes.
#[test]
fn decoder_retains_canonical_single_grouped_and_temperature_documents() {
    let [single, grouped, temperature] = predecessor_projection_bytes()
        .try_into()
        .expect("three predecessor fixtures");

    let ApiMaintenanceRevisionTransportDocument::Standard(single) =
        decode_native_api_revision_document(&single).unwrap()
    else {
        panic!("single fixture must remain a standard document");
    };
    let AdjustmentRevisionProjectionDocument::Single(single) = single.as_ref() else {
        panic!("single fixture must retain its generated variant");
    };
    assert_eq!(
        single.projectionKind,
        AdjustmentRevisionProjectionKind::ActualBestMatch
    );
    assert_eq!(single.rows.len(), 1);
    assert_eq!(single.logicalKey.len(), 4);
    assert_eq!(single.source.len(), 9);
    assert_eq!(single.storedContentSha256, "a".repeat(64));

    let ApiMaintenanceRevisionTransportDocument::Standard(grouped) =
        decode_native_api_revision_document(&grouped).unwrap()
    else {
        panic!("grouped fixture must remain a standard document");
    };
    let AdjustmentRevisionProjectionDocument::Batch(grouped) = grouped.as_ref() else {
        panic!("grouped fixture must retain its generated variant");
    };
    assert_eq!(grouped.rows.len(), 168);
    assert_eq!(grouped.logicalKey.len(), 4);
    assert_eq!(grouped.source.len(), 9);
    assert_eq!(
        grouped.storedContentSha256,
        "b6ff6137add9d1b78bf4d76f6dad3f2dcd29b83bd4a66c3d209125a8ad75b79d"
    );

    let ApiMaintenanceRevisionTransportDocument::Standard(temperature) =
        decode_native_api_revision_document(&temperature).unwrap()
    else {
        panic!("temperature fixture must remain a standard document");
    };
    let AdjustmentRevisionProjectionDocument::NativeTemperature(temperature) = temperature.as_ref()
    else {
        panic!("temperature fixture must retain its generated variant");
    };
    assert_eq!(temperature.rows.len(), 18);
    assert_eq!(temperature.recentErrorState.localDates, 3);
    assert_eq!(temperature.recentErrorState.sourceKeys.len(), 24);
    assert_eq!(temperature.recentErrorState.b24C, Nullable::Value(0.2));
    assert_eq!(temperature.recentErrorState.b72C, Nullable::Value(0.1));
    assert_eq!(temperature.recentErrorState.mad72C, Nullable::Value(0.5));
    assert!(temperature.recentErrorState.supported);
    assert_eq!(
        temperature.recentErrorState.targetRunInitializedAt,
        "2026-10-08T00:00:00.000Z"
    );
}

const NODE_FIXTURES: &str = r#"
import { createHash } from "node:crypto";
import {
  encodeAdjustmentRevisionBatchProjection,
  encodeAdjustmentRevisionProjection,
  encodeAdjustmentTemperatureNativeSourceProjection,
  encodeMaintenanceBinary64,
} from "./packages/forecast-adjustment/dist/index.js";

const HASH = "a".repeat(64);
const SECOND_HASH = "b".repeat(64);

// construct the predecessor's complete non-rain lineage
const source = () => ({
  adapterVersion: "open-meteo/v4",
  contractEpoch: "forecast/v4",
  dataset: "best_match",
  providerKey: "open-meteo",
  sourceConfigFingerprint: "open-meteo-forecast/v4",
  sourceId: "7",
  sourceKey: "open-meteo-forecast",
  sourceKind: "forecast",
  upstreamModel: "best_match",
});

// construct one closed normalized weather revision row
const weatherRow = (validAt) => ({
  apparentTemperatureC64: encodeMaintenanceBinary64(10),
  blackGlobeTemperatureC64: null,
  cloudCoverPercent64: encodeMaintenanceBinary64(50),
  contentSha256: HASH,
  pm25MicrogramsPerCubicMeter64: null,
  precipitationMm64: encodeMaintenanceBinary64(0),
  precipitationRateMmPerHour64: null,
  pressureHpa64: encodeMaintenanceBinary64(1010),
  relativeHumidityPercent64: encodeMaintenanceBinary64(80),
  soilElectricalConductivityMicrosiemensPerCm64: null,
  soilMoisturePercent64: null,
  solarRadiationWm264: null,
  temperatureC64: encodeMaintenanceBinary64(11),
  uvIndex64: null,
  validAt,
  waterLevelM64: null,
  wetBulbGlobeTemperatureC64: null,
  windDirectionDegrees64: encodeMaintenanceBinary64(180),
  windGustMps64: encodeMaintenanceBinary64(8),
  windSpeedMps64: encodeMaintenanceBinary64(5),
});

const single = {
  contractVersion: "adjustment-revision-projection/v1",
  family: "shared",
  logicalKey: {
    productRunAt: "2026-10-08T00:00:00.000Z",
    sourceId: "7",
    sourceKind: "forecast",
    validAt: "2026-10-08T01:00:00.000Z",
  },
  logicalReceivedAt: "2026-10-08T00:05:00.000Z",
  projectionKind: "actual_best_match",
  rows: [weatherRow("2026-10-08T01:00:00.000Z")],
  source: source(),
  storedContentSha256: HASH,
};

const grouped = {
  ...single,
  contractVersion: "adjustment-revision-batch-projection/v2",
  family: "wind",
  rows: Array.from({ length: 168 }, (_unused, index) => ({
    ...weatherRow(new Date(Date.parse("2026-10-08T01:00:00.000Z") + index * 3600000).toISOString()),
    contentSha256: createHash("sha256").update(`weather-${index}`).digest("hex"),
  })),
  storedContentSha256: createHash("sha256").update("weather-0").digest("hex"),
};

const temperatureBase = {
  ...single,
  family: "temperature",
  logicalKey: {
    contentSha256: HASH,
    leadHours: null,
    providerResponseSha256: SECOND_HASH,
    runInitializedAt: "2026-10-08T00:00:00.000Z",
    siteId: "3",
    sourceId: null,
    sourceType: "ecmwf_temperature_run",
    validAt: null,
  },
  projectionKind: "native_source",
  rows: Array.from({ length: 18 }, (_unused, index) => ({
    bestMatchContentSha256: index < 6 ? null : SECOND_HASH,
    bestMatchProductRunAt: index < 6 ? null : "2026-10-08T00:00:00.000Z",
    bestMatchSourceId: index < 6 ? null : "7",
    bestMatchTemperatureC64: index < 6 ? null : encodeMaintenanceBinary64(10),
    contentSha256: createHash("sha256").update(`temperature-${index}`).digest("hex"),
    modelCycle: "50r1",
    modelLeadHours: index + 1,
    rawRelativeHumidityPercent64: encodeMaintenanceBinary64(80),
    rawTemperatureC64: encodeMaintenanceBinary64(11),
    rawWindSpeedMps64: encodeMaintenanceBinary64(5),
    validAt: new Date(Date.parse("2026-10-08T01:00:00.000Z") + index * 3600000).toISOString(),
  })),
  source: {
    ...source(),
    adapterVersion: "open-meteo-ecmwf-single-run/v1",
    contractEpoch: "temperature-canary/v1",
    dataset: "single_run",
    upstreamModel: "ecmwf_ifs",
  },
};
const recentErrorState = {
  b24C: 0.2,
  b72C: 0.1,
  cohort: "ecmwf_single_run_hindcast",
  localDates: 3,
  mad72C: 0.5,
  maximumSourceRunInitializedAt: "2026-10-07T12:00:00.000Z",
  maximumSourceValidAt: "2026-10-07T17:00:00.000Z",
  n24: 6,
  n72: 24,
  sourceKeys: Array.from({ length: 24 }, (_unused, index) =>
    `2026-10-07T00:00:00.000Z/${new Date(Date.parse("2026-10-07T00:00:00.000Z") + index * 3600000).toISOString()}`),
  supported: true,
  targetRunInitializedAt: temperatureBase.logicalKey.runInitializedAt,
  windowEndValidAt: "2026-10-07T17:00:00.000Z",
};
const temperature = {
  ...temperatureBase,
  contractVersion: "adjustment-temperature-native-source-projection/v2",
  recentErrorState,
  recentErrorStateSha256: createHash("sha256").update(JSON.stringify(recentErrorState)).digest("hex"),
};

const encoded = [
  encodeAdjustmentRevisionProjection(single),
  encodeAdjustmentRevisionBatchProjection(grouped),
  encodeAdjustmentTemperatureNativeSourceProjection(temperature),
].map((bytes) => bytes.toString("base64"));
process.stdout.write(JSON.stringify(encoded));
"#;
