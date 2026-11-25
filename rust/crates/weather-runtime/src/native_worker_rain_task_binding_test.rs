//! Positive differentials for the native rain task values against compiled predecessor owners.

use std::{fs, path::Path, process::Command};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use weather_contracts::{
    AdjustmentRainFixedGaugeTargetCaptureBody, AdjustmentRainFixedGaugeTargetCaptureMember,
    AdjustmentRainFixedGaugeTargetClaim, AdjustmentRainFixedGaugeTargetProjection,
    AdjustmentRainFixedGaugeTargetProjectionContractVersionLiteral,
    AdjustmentRainFixedGaugeTargetProjectionFamilyLiteral,
    AdjustmentRainFixedGaugeTargetProjectionProjectionKindLiteral,
    AdjustmentRainFixedGaugeTargetRow, CanonicalWeatherMetrics, DeviceMetadata, FieldPresence,
    JsonValue, NormalizedWeatherRecord, Nullable, SourceKind, WeatherRecordMetadata,
};
use weather_edge::ArchiveWirePrimitives;
use weather_worker::{RAIN_COLLECTION_STATIONS, RainAdjustmentFeatureAdmission};

use super::*;

const COMPLETED_AT: &str = "2026-09-14T08:05:00.000Z";
const VALID_AT: &str = "2026-09-14T08:00:00.000Z";
const WINDOW_END: &str = "2026-09-14T09:00:00.000Z";
const WINDOW_START: &str = "2026-09-14T07:00:00.000Z";

struct PredecessorOracle {
    calendar: [f64; 4],
    content_hashes: Vec<String>,
    projection_base64: String,
    projection_valid_at: String,
}

/// Run the compiled TypeScript owners over the same retained provider source body.
fn predecessor_oracle(root: &Path) -> PredecessorOracle {
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_ORACLE])
        .current_dir(root)
        .output()
        .expect("run compiled rain predecessor oracle");
    assert!(
        output.status.success(),
        "compiled rain predecessor oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("decode compiled rain predecessor oracle");
    PredecessorOracle {
        calendar: serde_json::from_value(value["calendar"].clone())
            .expect("decode calendar fields"),
        content_hashes: serde_json::from_value(value["contentHashes"].clone())
            .expect("decode content hashes"),
        projection_base64: value["projectionBase64"].as_str().unwrap().to_owned(),
        projection_valid_at: value["projectionValidAt"].as_str().unwrap().to_owned(),
    }
}

/// Build the exact incomplete fixed-gauge record used by both differential owners.
fn target_record(index: usize) -> NormalizedWeatherRecord {
    let station = RAIN_COLLECTION_STATIONS[index];
    NormalizedWeatherRecord {
        metadata: WeatherRecordMetadata {
            device: Nullable::Value(DeviceMetadata {
                model: FieldPresence::Value("Tempest".to_owned()),
                serial: FieldPresence::Value(station.serial.to_owned()),
                vendor: FieldPresence::Value("WeatherFlow".to_owned()),
            }),
            model: Nullable::Null,
            provider: Nullable::Value(vec![
                json_member("dataset", json_text("rain_fixed_gauge_hourly")),
                json_member("device_id", JsonValue::Number(station.device_id as f64)),
                json_member("location_id", JsonValue::Number(station.location_id as f64)),
                json_member("report_interval_minutes", JsonValue::Number(60.0)),
            ]),
            quality: Nullable::Value(vec![
                json_member("sampling", json_text("backward_exact_interval_tiling")),
                json_member("status", json_text("incomplete_interval_coverage")),
            ]),
            upstreamTimezone: "America/Los_Angeles".to_owned(),
        },
        metrics: CanonicalWeatherMetrics {
            apparentTemperatureC: Nullable::Null,
            blackGlobeTemperatureC: Nullable::Null,
            cloudCoverPercent: Nullable::Null,
            pm25MicrogramsPerCubicMeter: Nullable::Null,
            precipitationMm: Nullable::Null,
            precipitationRateMmPerHour: Nullable::Null,
            pressureHpa: Nullable::Null,
            relativeHumidityPercent: Nullable::Null,
            soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
            soilMoisturePercent: Nullable::Null,
            solarRadiationWm2: Nullable::Null,
            temperatureC: Nullable::Null,
            uvIndex: Nullable::Null,
            waterLevelM: Nullable::Null,
            wetBulbGlobeTemperatureC: Nullable::Null,
            windDirectionDegrees: Nullable::Null,
            windGustMps: Nullable::Null,
            windSpeedMps: Nullable::Null,
        },
        productRunAt: Nullable::Null,
        receivedAt: COMPLETED_AT.to_owned(),
        sourceId: format!("source-{index:02}"),
        sourceKind: SourceKind::PhysicalSensor,
        validAt: VALID_AT.to_owned(),
    }
}

/// Build one complete twelve-row projection around an actual retained provider fixture body.
fn native_projection(body: &[u8]) -> (AdjustmentRainFixedGaugeTargetProjection, Vec<String>) {
    let evidence = NativeModelEvidenceValues::new();
    let body_sha256 = ApiMaintenanceReceiptCanonicalValues::sha256(&evidence, body)
        .expect("fixture body digest")
        .as_str()
        .to_owned();
    let body_base64 = ArchiveWirePrimitives::encode_standard_base64(&evidence, body)
        .expect("fixture body base64");
    let claims = RAIN_COLLECTION_STATIONS
        .iter()
        .enumerate()
        .map(|(index, station)| AdjustmentRainFixedGaugeTargetClaim {
            claimId: format!("claim-{index:02}"),
            completedAt: COMPLETED_AT.to_owned(),
            stationId: station.location_id,
            windowEndExclusive: WINDOW_END.to_owned(),
            windowStart: WINDOW_START.to_owned(),
        })
        .collect();
    let mut content_hashes = Vec::with_capacity(RAIN_COLLECTION_STATIONS.len());
    let rows = RAIN_COLLECTION_STATIONS
        .iter()
        .enumerate()
        .map(|(index, station)| {
            let normalized_record = target_record(index);
            let stored_content_sha256 = fixed_gauge_record_content_sha256(&normalized_record)
                .expect("fixed-gauge content digest")
                .as_str()
                .to_owned();
            content_hashes.push(stored_content_sha256.clone());
            AdjustmentRainFixedGaugeTargetRow {
                captureMembers: vec![AdjustmentRainFixedGaugeTargetCaptureMember {
                    bodySha256: body_sha256.clone(),
                    claimId: format!("claim-{index:02}"),
                    completedAt: COMPLETED_AT.to_owned(),
                }],
                intervals: Vec::new(),
                normalizedRecord: normalized_record,
                stationId: station.location_id,
                storedContentSha256: stored_content_sha256,
            }
        })
        .collect();
    (
        AdjustmentRainFixedGaugeTargetProjection {
            captureBodies: vec![AdjustmentRainFixedGaugeTargetCaptureBody {
                bodyBase64: body_base64,
                bodySha256: body_sha256,
                claims,
            }],
            contractVersion: AdjustmentRainFixedGaugeTargetProjectionContractVersionLiteral,
            family: AdjustmentRainFixedGaugeTargetProjectionFamilyLiteral,
            logicalReceivedAt: COMPLETED_AT.to_owned(),
            projectionKind: AdjustmentRainFixedGaugeTargetProjectionProjectionKindLiteral,
            rows,
            validAt: VALID_AT.to_owned(),
        },
        content_hashes,
    )
}

/// Compare actual source-body custody, content hashes, canonical bytes, and UTC harmonics.
#[test]
fn native_rain_task_values_match_compiled_predecessor() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(3).expect("workspace root");
    let body = fs::read(root.join("packages/providers/test/fixtures/tempest/observations.json"))
        .expect("retained Tempest fixture");
    let oracle = predecessor_oracle(root);
    let (projection, content_hashes) = native_projection(&body);
    let encoded = NativeWorkerRainProjectionValues::new()
        .encode_fixed_gauge_projection(&projection)
        .expect("native fixed-gauge projection");
    assert_eq!(content_hashes, oracle.content_hashes);
    assert_eq!(encoded, STANDARD.decode(oracle.projection_base64).unwrap());

    let valid_at_ms = NativeWirePrimitives
        .canonical_utc_milliseconds(&oracle.projection_valid_at)
        .expect("oracle projection instant");
    let calendar = NativeRainAdjustmentFeatureAdmission
        .calendar(valid_at_ms)
        .expect("native rain UTC features");
    let actual = [
        calendar.annual_sin,
        calendar.annual_cos,
        calendar.daily_sin,
        calendar.daily_cos,
    ];
    // model input narrows these harmonics to float32 before tree evaluation
    assert_eq!(
        actual.map(|value| value as f32),
        oracle.calendar.map(|value| value as f32)
    );
}

/// Construct one UTF-16 JSON string for generated metadata fixtures.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Construct one UTF-16 JSON object member for generated metadata fixtures.
fn json_member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

const NODE_ORACLE: &str = r#"
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import {
  RAIN_COLLECTION_STATIONS,
  createNormalizedWeatherRecord,
  weatherRecordContent,
} from "./packages/domain/dist/index.js";
import {
  encodeAdjustmentRainFixedGaugeTargetProjection,
  projectRainHurdleWindFeatures,
} from "./packages/forecast-adjustment/dist/index.js";

const completedAt = "2026-09-14T08:05:00.000Z";
const validAt = "2026-09-14T08:00:00.000Z";
const windowEndExclusive = "2026-09-14T09:00:00.000Z";
const windowStart = "2026-09-14T07:00:00.000Z";
const body = readFileSync("./packages/providers/test/fixtures/tempest/observations.json");
const bodySha256 = createHash("sha256").update(body).digest("hex");

// preserve the generated complete metric key set while marking this source hour incomplete
const metrics = () => ({
  apparentTemperatureC: null,
  blackGlobeTemperatureC: null,
  cloudCoverPercent: null,
  pm25MicrogramsPerCubicMeter: null,
  precipitationMm: null,
  precipitationRateMmPerHour: null,
  pressureHpa: null,
  relativeHumidityPercent: null,
  soilElectricalConductivityMicrosiemensPerCm: null,
  soilMoisturePercent: null,
  solarRadiationWm2: null,
  temperatureC: null,
  uvIndex: null,
  waterLevelM: null,
  wetBulbGlobeTemperatureC: null,
  windDirectionDegrees: null,
  windGustMps: null,
  windSpeedMps: null,
});

// derive each normalized target through the predecessor domain constructor
const rows = RAIN_COLLECTION_STATIONS.map((station, index) => {
  const normalizedRecord = createNormalizedWeatherRecord({
    metadata: {
      device: { model: "Tempest", serial: station.serial, vendor: "WeatherFlow" },
      model: null,
      provider: {
        dataset: "rain_fixed_gauge_hourly",
        device_id: station.deviceId,
        location_id: station.locationId,
        report_interval_minutes: 60,
      },
      quality: {
        sampling: "backward_exact_interval_tiling",
        status: "incomplete_interval_coverage",
      },
      upstreamTimezone: "America/Los_Angeles",
    },
    metrics: metrics(),
    productRunAt: null,
    receivedAt: completedAt,
    sourceId: `source-${String(index).padStart(2, "0")}`,
    sourceKind: "physical_sensor",
    validAt,
  });
  return {
    captureMembers: [{
      bodySha256,
      claimId: `claim-${String(index).padStart(2, "0")}`,
      completedAt,
    }],
    intervals: [],
    normalizedRecord,
    stationId: station.locationId,
    storedContentSha256: createHash("sha256").update(weatherRecordContent(normalizedRecord)).digest("hex"),
  };
});

const projection = {
  captureBodies: [{
    bodyBase64: body.toString("base64"),
    bodySha256,
    claims: RAIN_COLLECTION_STATIONS.map((station, index) => ({
      claimId: `claim-${String(index).padStart(2, "0")}`,
      completedAt,
      stationId: station.locationId,
      windowEndExclusive,
      windowStart,
    })),
  }],
  contractVersion: "adjustment-rain-fixed-gauge-target-projection/v1",
  family: "rain",
  logicalReceivedAt: completedAt,
  projectionKind: "target_revision",
  rows,
  validAt,
};

// execute the compiled serving feature builder to expose its four UTC harmonics
const currentRun = {
  runInitializedAt: "2026-09-14T00:00:00.000Z",
  completedAt: "2026-09-14T01:00:00.000Z",
  hours: Array.from({ length: 48 }, (_unused, index) => ({
    leadHours: index + 1,
    precipitationMm: 0,
    temperatureC: 10,
    relativeHumidityPercent: 80,
    cloudCoverPercent: 50,
    pressureHpa: 1010,
    windSpeedMps: 2,
    windDirectionDegrees: 180,
  })),
};
const featureProjection = projectRainHurdleWindFeatures({
  currentRun,
  priorRuns: [],
  stationHours: [],
  nowUtc: "2026-09-14T08:00:00.000Z",
});
const first = featureProjection.rows[0];
process.stdout.write(JSON.stringify({
  calendar: Array.from(first.features.slice(1, 5)),
  contentHashes: rows.map((row) => row.storedContentSha256),
  projectionBase64: encodeAdjustmentRainFixedGaugeTargetProjection(projection).toString("base64"),
  projectionValidAt: first.validAt,
}));
"#;
