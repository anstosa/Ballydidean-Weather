use std::cmp::Ordering;

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use super::{
    TemperaturePerformanceTargetError, TemperaturePerformanceTargetValues,
    derive_temperature_performance_target,
};
use crate::forecast_training_package_row::ForecastTrainingPackageRowTime;
use crate::maintenance_canonical_json::{MaintenanceCanonicalJson, field, text};

const VALID_AT: &str = "2026-09-14T07:00:00.000Z";
const TARGET_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct FixtureValues {
    encoded: Vec<JsonValue>,
    sha_inputs: Vec<Vec<u8>>,
}

impl MaintenanceCanonicalJson for FixtureValues {
    /// reject unused parsing in the target fixture
    fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
        None
    }

    /// capture evidence and assign a deterministic locale-sort material
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        self.encoded.push(value.clone());
        // sort revision payloads by their fixture source key
        if let Some(source_key) = text(value, "sourceKey") {
            return Some(source_key.into_bytes());
        }
        Some(b"identity\n".to_vec())
    }

    /// return one admitted digest for the final canonical identity only
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.sha_inputs.push(bytes.to_vec());
        (bytes == b"identity\n").then(|| Sha256Digest::admitted(TARGET_HASH.to_owned()))
    }
}

impl ForecastTrainingPackageRowTime for FixtureValues {
    /// parse the bounded target and revision fixture instants
    fn utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        // retain exact millisecond positions around the target interval
        match value {
            "2026-09-14T05:59:00.000Z" => Some(-60_000),
            "2026-09-14T06:00:00.000Z" => Some(0),
            "2026-09-14T06:30:00.000Z" => Some(1_800_000),
            VALID_AT => Some(3_600_000),
            _ => None,
        }
    }

    /// leave unused formatting outside the target slice
    fn canonical_utc(&self, _: i64) -> Option<String> {
        None
    }

    /// leave unused calendar projection outside the target slice
    fn los_angeles_local_date(&self, _: i64) -> Option<String> {
        None
    }
}

impl TemperaturePerformanceTargetValues for FixtureValues {
    /// use lexical fixture ordering for the injected javascript locale effect
    fn compare_js_locale(&self, left: &str, right: &str) -> Ordering {
        left.cmp(right)
    }
}

/// preserve weighted selection, revision bounds, metadata, and revised receipt max
#[test]
fn target_uses_weighted_station_value_and_canonical_revision_identity() {
    let forecast_rows = vec![
        station_row(
            "tempest-64255",
            30.0,
            Some("2026-09-14T07:03:00.000Z"),
            "tempest-64255-observations-v2",
            "fp-tempest",
        ),
        station_row(
            "ambient-maxweather",
            10.0,
            Some("2026-09-14T07:01:00.000Z"),
            "ambient-maxweather-observations-v1",
            "fp-maxweather",
        ),
        station_row(
            "ambient-merlin",
            20.0,
            None,
            "ambient-merlin-observations-v1",
            "fp-merlin",
        ),
    ];
    let adjustment_rows = vec![
        revision_row(
            "tempest-64255",
            "tempest-64255-observations-v2",
            "fp-tempest",
            VALID_AT,
            "2026-09-14T07:12:00.000Z",
        ),
        revision_row(
            "ambient-merlin",
            "ambient-merlin-observations-v1",
            "fp-merlin",
            "2026-09-14T06:00:00.000Z",
            "2026-09-14T07:10:00.000Z",
        ),
        revision_row(
            "ambient-maxweather",
            "ambient-maxweather-observations-v1",
            "fp-maxweather",
            "2026-09-14T05:59:00.000Z",
            "2026-09-14T09:00:00.000Z",
        ),
        revision_row(
            "ambient-maxweather",
            "ambient-maxweather-observations-v1",
            "wrong-fingerprint",
            "2026-09-14T06:30:00.000Z",
            "2026-09-14T10:00:00.000Z",
        ),
        revision_row(
            "ambient-maxweather",
            "ambient-maxweather-observations-v1",
            "fp-maxweather",
            "not-a-date",
            "2026-09-14T11:00:00.000Z",
        ),
    ];
    let mut values = FixtureValues::default();

    let target = derive_temperature_performance_target(
        &forecast_rows,
        VALID_AT,
        &adjustment_rows,
        &mut values,
    )
    .expect("ordinary weighted target should derive");

    assert_eq!(target.target, Some(20.0));
    assert_eq!(
        target.max_receipt_at.as_deref(),
        Some("2026-09-14T07:12:00.000Z")
    );
    assert_eq!(target.target_identity.as_deref(), Some(TARGET_HASH));
    assert_eq!(values.sha_inputs, vec![b"identity\n".to_vec()]);
    let preimage = values
        .encoded
        .last()
        .expect("complete target should encode an identity");
    let JsonValue::Array(selected) =
        field(preimage, "selectedTarget").expect("identity should include selected targets")
    else {
        panic!("selected target should be an array");
    };
    assert_eq!(
        selected
            .iter()
            .map(|row| text(row, "physicalStationKey").expect("station key should remain text"))
            .collect::<Vec<_>>(),
        vec!["ambient-maxweather", "ambient-merlin", "tempest-64255"]
    );
    assert!(field(&selected[1], "receivedAt").is_none());
    let JsonValue::Array(revisions) =
        field(preimage, "revisionEvidence").expect("identity should include revision evidence")
    else {
        panic!("revision evidence should be an array");
    };
    assert_eq!(revisions.len(), 2);
    assert_eq!(
        revisions
            .iter()
            .map(|row| text(row, "sourceKey").expect("source key should remain text"))
            .collect::<Vec<_>>(),
        vec![
            "ambient-merlin-observations-v1",
            "tempest-64255-observations-v2"
        ]
    );
}

/// preserve both minimum-count and nearest-three coverage gaps
#[test]
fn target_returns_missing_before_revision_or_canonical_work() {
    let malformed_revision = object(vec![
        ("record_kind", string("target_revision_diagnostic")),
        ("payload", JsonValue::Null),
    ]);
    let mut values = FixtureValues::default();
    let two_stations = vec![
        station_row(
            "ambient-merlin",
            10.0,
            Some("2026-09-14T07:01:00.000Z"),
            "ambient-merlin-observations-v1",
            "fp-merlin",
        ),
        station_row(
            "tempest-64255",
            11.0,
            Some("2026-09-14T07:02:00.000Z"),
            "tempest-64255-observations-v2",
            "fp-tempest",
        ),
    ];

    let target = derive_temperature_performance_target(
        &two_stations,
        VALID_AT,
        std::slice::from_ref(&malformed_revision),
        &mut values,
    )
    .expect("two stations should be an ordinary coverage gap");

    assert_eq!(target.target, None);
    assert_eq!(target.max_receipt_at, None);
    assert_eq!(target.target_identity, None);
    assert!(values.encoded.is_empty());

    let far_stations = vec![
        station_row(
            "ambient-maxweather",
            10.0,
            None,
            "ambient-maxweather-observations-v1",
            "fp-maxweather",
        ),
        station_row(
            "netatmo-nearby",
            11.0,
            None,
            "netatmo-nearby-observations-v1",
            "fp-netatmo",
        ),
        station_row(
            "tempest-126537",
            12.0,
            None,
            "tempest-126537-observations-v2",
            "fp-tempest",
        ),
    ];
    let target = derive_temperature_performance_target(
        &far_stations,
        VALID_AT,
        &[malformed_revision],
        &mut values,
    )
    .expect("three far stations should still be an ordinary coverage gap");

    assert_eq!(target.target, None);
    assert!(values.encoded.is_empty());
}

/// retain ordinary duplicate and finite-value rejection order
#[test]
fn target_rejects_duplicate_and_nonfinite_station_values() {
    let duplicate = vec![
        station_row("ambient-merlin", 10.0, None, "source-a", "fp-a"),
        station_row("ambient-merlin", 11.0, None, "source-b", "fp-b"),
        station_row("tempest-64255", 12.0, None, "source-c", "fp-c"),
    ];
    let mut values = FixtureValues::default();

    let error = derive_temperature_performance_target(&duplicate, VALID_AT, &[], &mut values)
        .expect_err("duplicate physical stations should be rejected");

    assert_eq!(error, TemperaturePerformanceTargetError::DuplicatedStation);

    let nonfinite = vec![
        station_row("ambient-merlin", f64::NAN, None, "source-a", "fp-a"),
        station_row("ambient-maxweather", 11.0, None, "source-b", "fp-b"),
        station_row("tempest-64255", 12.0, None, "source-c", "fp-c"),
    ];
    let error = derive_temperature_performance_target(&nonfinite, VALID_AT, &[], &mut values)
        .expect_err("nonfinite station values should be rejected");

    assert_eq!(
        error,
        TemperaturePerformanceTargetError::InvalidStationValue
    );
}

/// build one selected station-hour row with canonical identity material
fn station_row(
    station_key: &str,
    temperature_c: f64,
    received_at: Option<&str>,
    source_key: &str,
    fingerprint: &str,
) -> JsonValue {
    let mut fields = vec![
        ("record_kind", string("station_hour")),
        ("valid_at", string(VALID_AT)),
        ("physical_station_key", string(station_key)),
        ("temperature_c", JsonValue::Number(temperature_c)),
        ("content_hashes", array(&["hash-a", "hash-b"])),
        ("source_keys", array(&[source_key])),
        ("source_config_fingerprints", array(&[fingerprint])),
    ];
    // model the predecessor's optional receipt property
    if let Some(received_at) = received_at {
        fields.push(("received_at", string(received_at)));
    }
    object(fields)
}

/// build one target revision diagnostic row
fn revision_row(
    station_key: &str,
    source_key: &str,
    fingerprint: &str,
    valid_at: &str,
    max_receipt_at: &str,
) -> JsonValue {
    object(vec![
        ("record_kind", string("target_revision_diagnostic")),
        (
            "payload",
            object(vec![
                ("physicalStationKey", string(station_key)),
                ("sourceKey", string(source_key)),
                ("sourceConfigFingerprint", string(fingerprint)),
                ("validAt", string(valid_at)),
                ("maxSourceReceiptAt", string(max_receipt_at)),
            ]),
        ),
    ])
}

/// build one string array
fn array(values: &[&str]) -> JsonValue {
    JsonValue::Array(values.iter().map(|value| string(value)).collect())
}

/// build one utf-sixteen json string
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// build one json object from utf-eight field names
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}
