use weather_contracts::{
    FieldPresence, JsonValue, NormalizedWeatherRecord, Nullable, ScheduledCheckpointState,
    SourceKind,
};
use weather_database::{AdmittedIngestionError, SerializedJson, Sha256Digest};
use weather_runtime::{
    OpenMeteoRuntimeValues, RuntimePersistenceOperation, RuntimeTimeValue,
    ScheduledDatabaseAdapter, SupplementalPublicStationDatabaseAdapter,
};
use weather_worker::{
    NetatmoMeasurementBody, NetatmoMeasurementRow, NetatmoMeasurementSeries,
    NetatmoMeasurementStatus, NetatmoMeasurementValues, OpenMeteoCursor, PublicStationNumber,
    PurpleAirDecodedHistory, PurpleAirDecodedRow, ScheduledWindow, SupplementalPublicStationInputs,
    SupplementalPublicStationProduct, TaskFailure, WeatherUndergroundMetricObservation,
    WeatherUndergroundObservation, WeatherUndergroundQcStatus,
    normalize_supplemental_public_station_batch,
};

const START_MS: i64 = 1_787_576_400_000;
const END_MS: i64 = 1_787_580_000_000;
const RECEIVED_AT_MS: i64 = 1_787_583_600_000;
const START: &str = "2026-08-24T13:00:00.000Z";
const RECEIVED_AT: &str = "2026-08-24T15:00:00.000Z";
const CONTENT_HASH: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Default)]
struct FixtureValues {
    hash_inputs: Vec<NormalizedWeatherRecord>,
    trace: Vec<&'static str>,
}

impl OpenMeteoRuntimeValues for FixtureValues {
    /// Map only the two public-station fixture instants.
    fn utc_instant(
        &mut self,
        milliseconds: i64,
        value: RuntimeTimeValue,
    ) -> Result<String, TaskFailure> {
        let instant = match (milliseconds, value) {
            (START_MS, RuntimeTimeValue::ValidAt) => {
                self.trace.push("utc-valid");
                START
            }
            (RECEIVED_AT_MS, RuntimeTimeValue::ReceivedAt) => {
                self.trace.push("utc-received");
                RECEIVED_AT
            }
            _ => panic!("unexpected public-station instant: {milliseconds} {value:?}"),
        };
        Ok(instant.to_owned())
    }

    /// Reject unused timestamp decoding in this projection-only fixture.
    fn epoch_milliseconds(
        &mut self,
        instant: &str,
        value: RuntimeTimeValue,
    ) -> Result<i64, TaskFailure> {
        panic!("unexpected public-station epoch decode: {instant} {value:?}")
    }

    /// Assert each exact ordered object before returning its fixture serialization.
    fn serialize_json_object(
        &mut self,
        value: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        let (trace, encoded) = match value {
            value if value == purpleair_provider().as_slice() => (
                "json-provider",
                r#"{"dataset":"public-map-history","device_id":"32489","report_interval_minutes":2}"#,
            ),
            value if value == purpleair_quality().as_slice() => (
                "json-quality",
                r#"{"flags":["dual_channel_average","uncorrected_sensor_enclosure_temperature"],"sampling":"every_two_minute_provider_observation"}"#,
            ),
            value if value == [(utf16("valid_at"), json_string(START))] => {
                ("json-cursor", r#"{"valid_at":"2026-08-24T13:00:00.000Z"}"#)
            }
            value if value == purpleair_response().as_slice() => (
                "json-purpleair-response",
                r#"{"normalized_record_count":1,"provider":"purpleair","raw_observation_count":2,"resolution_seconds":120}"#,
            ),
            value if value == wunderground_response().as_slice() => (
                "json-wunderground-response",
                r#"{"normalized_record_count":2,"provider":"weather-underground","raw_observation_count":4}"#,
            ),
            value if value == netatmo_response().as_slice() => (
                "json-netatmo-response",
                r#"{"measurement_request_count":4,"normalized_record_count":1,"provider":"netatmo-weathermap"}"#,
            ),
            value => panic!("unexpected public-station JSON object: {value:?}"),
        };
        self.trace.push(trace);
        Ok(SerializedJson::admitted(encoded))
    }

    /// Reject unrelated request metadata serialization.
    fn request_metadata_json(&mut self, endpoint: &str) -> Result<SerializedJson, TaskFailure> {
        panic!("unexpected public-station endpoint: {endpoint}")
    }

    /// Reject the unrelated Open-Meteo cursor boundary.
    fn provider_cursor_json(
        &mut self,
        cursor: Option<&OpenMeteoCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        panic!("unexpected Open-Meteo cursor: {cursor:?}")
    }

    /// Reject checkpoint decoding in this projection-only fixture.
    fn checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<OpenMeteoCursor>, TaskFailure> {
        panic!("unexpected public-station checkpoint: {checkpoint:?}")
    }

    /// Retain the exact generated row presented to the content-hash codec.
    fn weather_record_content_hash(
        &mut self,
        record: &NormalizedWeatherRecord,
    ) -> Result<Sha256Digest, TaskFailure> {
        self.trace.push("content-hash");
        self.hash_inputs.push(record.clone());
        Ok(Sha256Digest::admitted(CONTENT_HASH))
    }

    /// Reject unrelated digest admission in this projection-only fixture.
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure> {
        panic!("unexpected public-station digest admission: {value}")
    }

    /// Reject failure projection in this successful fixture.
    fn ingestion_error(
        &mut self,
        failure: &TaskFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        panic!("unexpected public-station ingestion failure: {failure:?}")
    }

    /// Reject persistence classification in this adapter-only fixture.
    fn persistence_failure(
        &mut self,
        operation: RuntimePersistenceOperation,
        message: &str,
    ) -> TaskFailure {
        panic!("unexpected public-station persistence failure: {operation:?} {message}")
    }
}

/// Cross normalized provider batches into the exact scheduled database projection.
#[test]
fn projects_supplemental_provider_evidence_without_losing_original_keys() {
    let window = fixture_window();
    let purpleair = purpleair_batch(window);
    let wunderground = wunderground_batch(window);
    let netatmo = netatmo_batch(window);
    assert_eq!(purpleair.response_metadata, purpleair_metadata());
    assert_eq!(wunderground.response_metadata, wunderground_metadata());
    assert_eq!(netatmo.response_metadata, netatmo_metadata());

    let [record] = purpleair.records.as_slice() else {
        panic!("PurpleAir fixture must retain one normalized row");
    };
    let mut values = FixtureValues::default();
    let prepared = <SupplementalPublicStationDatabaseAdapter as ScheduledDatabaseAdapter<
        FixtureValues,
    >>::project_record(&mut values, record.clone())
    .expect("PurpleAir row should project");
    let cursor = <SupplementalPublicStationDatabaseAdapter as ScheduledDatabaseAdapter<
        FixtureValues,
    >>::provider_cursor_json(&mut values, &purpleair.provider_cursor)
    .expect("PurpleAir cursor should serialize");
    let purpleair_response_json = response_json(&mut values, &purpleair.response_metadata);
    let wunderground_response_json = response_json(&mut values, &wunderground.response_metadata);
    let netatmo_response_json = response_json(&mut values, &netatmo.response_metadata);

    assert_eq!(prepared.record, values.hash_inputs[0]);
    assert_eq!(prepared.record.sourceId, "source-public");
    assert_eq!(prepared.record.sourceKind, SourceKind::PhysicalSensor);
    assert_eq!(prepared.record.receivedAt, RECEIVED_AT);
    assert_eq!(prepared.record.validAt, START);
    assert_eq!(prepared.record.productRunAt, Nullable::Null);
    assert_eq!(
        prepared.record.metadata.provider,
        Nullable::Value(purpleair_provider())
    );
    assert_eq!(
        prepared.record.metadata.quality,
        Nullable::Value(purpleair_quality())
    );
    assert_eq!(
        prepared.record.metadata.upstreamTimezone,
        "America/Los_Angeles"
    );
    let Nullable::Value(device) = &prepared.record.metadata.device else {
        panic!("public station device metadata must be present");
    };
    assert_eq!(device.model, FieldPresence::Value("test-model".to_owned()));
    assert_eq!(
        device.serial,
        FieldPresence::Value("test-serial".to_owned())
    );
    assert_eq!(device.vendor, FieldPresence::Value("PurpleAir".to_owned()));
    assert_eq!(
        prepared.record.metrics.pm25MicrogramsPerCubicMeter,
        Nullable::Value(11.0)
    );
    assert_eq!(
        prepared.record.metrics.relativeHumidityPercent,
        Nullable::Value(51.0)
    );
    assert_eq!(
        prepared.record.metrics.pressureHpa,
        Nullable::Value(1_001.0)
    );
    assert_eq!(prepared.record.metrics.temperatureC, Nullable::Value(20.0));
    assert_eq!(prepared.storage.content_hash.as_str(), CONTENT_HASH);
    assert_eq!(
        prepared
            .storage
            .provider_metadata_json
            .as_ref()
            .map(SerializedJson::as_str),
        Some(r#"{"dataset":"public-map-history","device_id":"32489","report_interval_minutes":2}"#),
    );
    assert_eq!(
        prepared
            .storage
            .quality_metadata_json
            .as_ref()
            .map(SerializedJson::as_str),
        Some(
            r#"{"flags":["dual_channel_average","uncorrected_sensor_enclosure_temperature"],"sampling":"every_two_minute_provider_observation"}"#
        ),
    );
    assert_eq!(
        cursor.as_ref().map(SerializedJson::as_str),
        Some(r#"{"valid_at":"2026-08-24T13:00:00.000Z"}"#),
    );
    assert_eq!(
        purpleair_response_json.as_str(),
        r#"{"normalized_record_count":1,"provider":"purpleair","raw_observation_count":2,"resolution_seconds":120}"#,
    );
    assert_eq!(
        wunderground_response_json.as_str(),
        r#"{"normalized_record_count":2,"provider":"weather-underground","raw_observation_count":4}"#,
    );
    assert_eq!(
        netatmo_response_json.as_str(),
        r#"{"measurement_request_count":4,"normalized_record_count":1,"provider":"netatmo-weathermap"}"#,
    );
    assert_eq!(
        values.trace,
        [
            "json-provider",
            "json-quality",
            "utc-received",
            "utc-valid",
            "content-hash",
            "utc-valid",
            "json-cursor",
            "json-purpleair-response",
            "json-wunderground-response",
            "json-netatmo-response",
        ],
    );
}

/// Normalize the archived PurpleAir fixture through the common batch owner.
fn purpleair_batch(window: ScheduledWindow) -> weather_worker::SupplementalPublicStationBatch {
    let first = PurpleAirDecodedRow {
        row_width_valid: true,
        sensor_index: Some(32_489),
        timestamp_ms: Some(START_MS),
        humidity_a: PublicStationNumber::Number(50.0),
        humidity_b: PublicStationNumber::Number(52.0),
        pm25_atm_a: PublicStationNumber::Number(10.0),
        pm25_atm_b: PublicStationNumber::Number(12.0),
        pressure_a: PublicStationNumber::Number(1_000.0),
        pressure_b: PublicStationNumber::Number(1_002.0),
        temperature_a: PublicStationNumber::Number(68.0),
        temperature_b: PublicStationNumber::Missing,
    };
    let history = PurpleAirDecodedHistory {
        header: weather_worker::PurpleAirDecodedHeader::Valid,
        rows: vec![
            first.clone(),
            PurpleAirDecodedRow {
                timestamp_ms: Some(END_MS),
                ..first
            },
        ],
    };
    normalize_supplemental_public_station_batch(
        fixture_inputs(window),
        SupplementalPublicStationProduct::PurpleAir {
            sensor_index: 32_489,
            history: &history,
        },
        RECEIVED_AT_MS,
        2,
        "purpleair-response-digest".to_owned(),
    )
    .expect("PurpleAir Node fixture should normalize")
}

/// Normalize both local-day Weather Underground payloads from the Node fixture.
fn wunderground_batch(window: ScheduledWindow) -> weather_worker::SupplementalPublicStationBatch {
    let days = [
        vec![
            wunderground_row(1_787_576_399, 10.0),
            wunderground_row(1_787_576_400, 11.0),
        ],
        vec![
            wunderground_row(1_787_579_999, 12.0),
            wunderground_row(1_787_580_000, 13.0),
        ],
    ];
    normalize_supplemental_public_station_batch(
        fixture_inputs(window),
        SupplementalPublicStationProduct::WeatherUnderground {
            station_id: "KWACLINT112",
            days: &days,
        },
        RECEIVED_AT_MS,
        2,
        "wunderground-response-digest".to_owned(),
    )
    .expect("Weather Underground Node fixture should normalize")
}

/// Normalize the four Netatmo module series from the Node fixture.
fn netatmo_batch(window: ScheduledWindow) -> weather_worker::SupplementalPublicStationBatch {
    let series = [
        netatmo_series(1_787_576_617, &[12.7, 96.0]),
        netatmo_series(1_787_576_661, &[1_018.5]),
        netatmo_series(1_787_576_655, &[0.2]),
        netatmo_series(1_787_576_655, &[3.0, -1.0, 5.0, 15.0]),
    ];
    normalize_supplemental_public_station_batch(
        fixture_inputs(window),
        SupplementalPublicStationProduct::Netatmo {
            device_id: "70:ee:50:71:3f:c4",
            modules: &series,
        },
        RECEIVED_AT_MS,
        5,
        "netatmo-response-digest".to_owned(),
    )
    .expect("Netatmo Node fixture should normalize")
}

/// Build one typed Weather Underground fixture row.
fn wunderground_row(epoch_seconds: i64, temperature_c: f64) -> WeatherUndergroundObservation {
    WeatherUndergroundObservation {
        epoch_seconds,
        humidity_avg_percent: PublicStationNumber::Number(85.0),
        metric: Some(WeatherUndergroundMetricObservation {
            precip_rate_mm_per_hour: PublicStationNumber::Number(0.2),
            pressure_max_hpa: PublicStationNumber::Number(1_020.0),
            pressure_min_hpa: PublicStationNumber::Number(1_019.0),
            temperature_avg_c: PublicStationNumber::Number(temperature_c),
            wind_gust_high_kph: PublicStationNumber::Number(18.0),
            wind_speed_avg_kph: PublicStationNumber::Number(9.0),
        }),
        qc_status: WeatherUndergroundQcStatus::AdmittedNumber("1".to_owned()),
        solar_radiation_high_wm2: PublicStationNumber::Number(50.0),
        station_id: "KWACLINT112".to_owned(),
        uv_high: PublicStationNumber::Number(2.0),
        wind_direction_avg_degrees: PublicStationNumber::Number(180.0),
    }
}

/// Build one successful typed Netatmo module series.
fn netatmo_series(epoch_seconds: i64, values: &[f64]) -> NetatmoMeasurementSeries {
    NetatmoMeasurementSeries {
        status: NetatmoMeasurementStatus::Ok,
        body: NetatmoMeasurementBody::Rows(vec![NetatmoMeasurementRow {
            epoch_seconds,
            values: NetatmoMeasurementValues::Numbers(
                values
                    .iter()
                    .copied()
                    .map(PublicStationNumber::Number)
                    .collect(),
            ),
        }]),
    }
}

/// Serialize one actual response-metadata projection.
fn response_json(
    values: &mut FixtureValues,
    metadata: &weather_worker::SupplementalPublicStationResponseMetadata,
) -> SerializedJson {
    <SupplementalPublicStationDatabaseAdapter as ScheduledDatabaseAdapter<FixtureValues>>::
        response_metadata_json(values, metadata)
        .expect("response metadata should serialize")
        .expect("supplemental response metadata is always present")
}

/// Return the common Node public-station fixture inputs.
const fn fixture_inputs(window: ScheduledWindow) -> SupplementalPublicStationInputs<'static> {
    SupplementalPublicStationInputs {
        model: Some("test-model"),
        serial: Some("test-serial"),
        source_id: "source-public",
        timezone: "America/Los_Angeles",
        window,
    }
}

/// Return the exact one-hour Node fixture range.
const fn fixture_window() -> ScheduledWindow {
    ScheduledWindow {
        start_ms: START_MS,
        end_exclusive_ms: END_MS,
    }
}

/// Return ordered PurpleAir provider provenance.
fn purpleair_provider() -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        entry("dataset", json_string("public-map-history")),
        entry("device_id", json_string("32489")),
        entry("report_interval_minutes", JsonValue::Number(2.0)),
    ]
}

/// Return ordered PurpleAir quality provenance.
fn purpleair_quality() -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        entry(
            "flags",
            JsonValue::Array(vec![
                json_string("dual_channel_average"),
                json_string("uncorrected_sensor_enclosure_temperature"),
            ]),
        ),
        entry(
            "sampling",
            json_string("every_two_minute_provider_observation"),
        ),
    ]
}

/// Return the actual normalized PurpleAir response evidence.
fn purpleair_metadata() -> weather_worker::SupplementalPublicStationResponseMetadata {
    weather_worker::SupplementalPublicStationResponseMetadata {
        measurement_request_count: None,
        normalized_record_count: 1,
        provider: "purpleair",
        raw_observation_count: Some(2),
        resolution_seconds: Some(120),
    }
}

/// Return ordered PurpleAir response evidence.
fn purpleair_response() -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        entry("normalized_record_count", JsonValue::Number(1.0)),
        entry("provider", json_string("purpleair")),
        entry("raw_observation_count", JsonValue::Number(2.0)),
        entry("resolution_seconds", JsonValue::Number(120.0)),
    ]
}

/// Return the actual normalized Weather Underground response evidence.
fn wunderground_metadata() -> weather_worker::SupplementalPublicStationResponseMetadata {
    weather_worker::SupplementalPublicStationResponseMetadata {
        measurement_request_count: None,
        normalized_record_count: 2,
        provider: "weather-underground",
        raw_observation_count: Some(4),
        resolution_seconds: None,
    }
}

/// Return ordered Weather Underground response evidence.
fn wunderground_response() -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        entry("normalized_record_count", JsonValue::Number(2.0)),
        entry("provider", json_string("weather-underground")),
        entry("raw_observation_count", JsonValue::Number(4.0)),
    ]
}

/// Return the actual normalized Netatmo response evidence.
fn netatmo_metadata() -> weather_worker::SupplementalPublicStationResponseMetadata {
    weather_worker::SupplementalPublicStationResponseMetadata {
        measurement_request_count: Some(4),
        normalized_record_count: 1,
        provider: "netatmo-weathermap",
        raw_observation_count: None,
        resolution_seconds: None,
    }
}

/// Return ordered Netatmo response evidence.
fn netatmo_response() -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        entry("measurement_request_count", JsonValue::Number(4.0)),
        entry("normalized_record_count", JsonValue::Number(1.0)),
        entry("provider", json_string("netatmo-weathermap")),
    ]
}

/// Build one ordered generated JSON entry.
fn entry(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (utf16(key), value)
}

/// Build one generated JSON string.
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(utf16(value))
}

/// Encode fixture text as generated UTF-16 units.
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}
