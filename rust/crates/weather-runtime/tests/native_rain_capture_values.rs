//! Use native retained-provider body decoding and normalization, with Node only as the oracle.

use std::{fs, path::PathBuf, process::Command};
use weather_contracts::{
    JsonValue, Nullable, RainAdjustmentCapture, RainAdjustmentCaptureKindEnum,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_runtime::{
    NativeRainCaptureValues, NativeWirePrimitives, decode_native_tempest_payload,
    project_worker_rain_adjustment_inputs,
};
use weather_worker::{
    RainAdjustmentCaptureKind, TempestObservationRequest, normalize_tempest_observations,
};

/// Borrow one exact Node oracle property.
fn field<'a>(value: &'a JsonValue, key: &str) -> &'a JsonValue {
    let JsonValue::Object(fields) = value else {
        panic!("oracle object")
    };
    fields
        .iter()
        .find_map(|(units, value)| {
            units
                .iter()
                .copied()
                .eq(key.encode_utf16())
                .then_some(value)
        })
        .unwrap()
}

/// Preserve the normalized nullable metric representation.
fn metric(value: Option<f64>) -> JsonValue {
    value.map_or(JsonValue::Null, JsonValue::Number)
}

/// Preserve generated nullable forecast values.
fn nullable(value: Nullable<f64>) -> JsonValue {
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => JsonValue::Number(value),
    }
}

/// Verify all retained lead cells and real Tempest normalization through native code.
#[test]
fn native_rain_capture_values_match_node_profiles_and_observations() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let oracle =
        Command::new("node")
            .arg(root.join(
                "rust/crates/weather-runtime/tests/native-rain-capture-values-node-oracle.mjs",
            ))
            .output()
            .unwrap();
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let expected = NativeWirePrimitives.decode_json(&oracle.stdout).unwrap();
    let bytes =
        fs::read(root.join("packages/providers/test/fixtures/tempest/observations.json")).unwrap();
    let envelope = decode_native_tempest_payload(&bytes).unwrap();
    let request = TempestObservationRequest {
        device_id: 470937,
        end_exclusive_ms: 1787191200000,
        location_id: 203055,
        serial: "ST-00198967".to_owned(),
        source_id: "source-tempest".to_owned(),
        start_ms: 1787184000000,
        timezone: "America/Los_Angeles".to_owned(),
    };
    let received = NativeWirePrimitives
        .canonical_utc_milliseconds("2026-08-20T12:00:00.000Z")
        .unwrap();
    let actual = normalize_tempest_observations(envelope, &request, received, 1).unwrap();
    let rows = actual
        .records
        .iter()
        .map(|row| {
            JsonValue::Array(vec![
                JsonValue::Number(row.valid_at_ms as f64),
                metric(row.metrics.temperature_c),
                metric(row.metrics.precipitation_mm),
                metric(row.metrics.precipitation_rate_mm_per_hour),
                metric(row.metrics.wind_speed_mps),
                metric(row.metrics.wind_direction_degrees),
                metric(row.metrics.uv_index),
                metric(row.provider.report_interval_minutes),
            ])
        })
        .collect();
    assert_eq!(&JsonValue::Array(rows), field(&expected, "tempest"));
    let JsonValue::String(body) = field(&expected, "body") else {
        panic!("forecast body")
    };
    let capture = RainAdjustmentCapture {
        claimId: "fixture-forecast".to_owned(),
        kind: RainAdjustmentCaptureKindEnum::Forecast,
        stationId: Nullable::Null,
        runInitializedAt: Nullable::Value("2026-09-14T00:00:00.000Z".to_owned()),
        windowStart: Nullable::Null,
        windowEndExclusive: Nullable::Null,
        completedAt: "2026-09-14T01:00:00.000Z".to_owned(),
        bodySha256: "a".repeat(64),
        body: String::from_utf16(body).unwrap().into_bytes(),
    };
    let projected =
        project_worker_rain_adjustment_inputs(&mut NativeRainCaptureValues, vec![capture]).unwrap();
    let RainAdjustmentCaptureKind::Forecast { profile, .. } = &projected.captures[0].kind else {
        panic!("forecast projection")
    };
    let rows = profile
        .hours
        .iter()
        .map(|hour| {
            JsonValue::Array(vec![
                JsonValue::Number(hour.leadHours),
                nullable(hour.precipitationMm.clone()),
                nullable(hour.temperatureC.clone()),
                nullable(hour.relativeHumidityPercent.clone()),
                nullable(hour.cloudCoverPercent.clone()),
                nullable(hour.pressureHpa.clone()),
                nullable(hour.windSpeedMps.clone()),
                nullable(hour.windDirectionDegrees.clone()),
            ])
        })
        .collect();
    assert_eq!(&JsonValue::Array(rows), field(&expected, "forecast"));
}
