use super::*;

struct FixtureTime;

impl ForecastTrainingPackageRowTime for FixtureTime {
    /// Parse only the deterministic fixture instants.
    fn utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        // retain exact fixture identities without introducing a date codec
        match value {
            "2026-10-01T12:00:00.000Z" => Some(0),
            "2026-10-01T12:01:00.000Z" => Some(60_000),
            _ => None,
        }
    }

    /// Format only the deterministic fixture epochs.
    fn canonical_utc(&self, epoch_ms: i64) -> Option<String> {
        // preserve the canonical millisecond spellings used by the row fixture
        match epoch_ms {
            0 => Some("2026-10-01T12:00:00.000Z".to_owned()),
            60_000 => Some("2026-10-01T12:01:00.000Z".to_owned()),
            _ => None,
        }
    }

    /// Project both fixture instants into the same Ballydidean date.
    fn los_angeles_local_date(&self, epoch_ms: i64) -> Option<String> {
        // admit only epochs already owned by this fixture
        if matches!(epoch_ms, 0 | 60_000) {
            Some("2026-10-01".to_owned())
        } else {
            None
        }
    }
}

/// Encode one fixture string through the generated UTF-16 JSON representation.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Encode one fixture array.
fn array(values: Vec<JsonValue>) -> JsonValue {
    JsonValue::Array(values)
}

/// Build one explicit missing station-hour grid row.
fn station_gap_row() -> JsonValue {
    JsonValue::Object(vec![
        ("adapter_contracts".encode_utf16().collect(), array(vec![])),
        (
            "collision_count".encode_utf16().collect(),
            JsonValue::Number(0.0),
        ),
        ("content_hashes".encode_utf16().collect(), array(vec![])),
        (
            "contract_epoch".encode_utf16().collect(),
            string("physical-station-hourly/v1"),
        ),
        ("dataset".encode_utf16().collect(), JsonValue::Null),
        (
            "exclusion_reason_codes".encode_utf16().collect(),
            array(vec![string("station_coverage_insufficient")]),
        ),
        ("ingestion_run_ids".encode_utf16().collect(), array(vec![])),
        (
            "physical_station_key".encode_utf16().collect(),
            string("ambient-merlin"),
        ),
        (
            "provider_family".encode_utf16().collect(),
            string("ambient"),
        ),
        (
            "record_kind".encode_utf16().collect(),
            string("station_hour"),
        ),
        ("received_at".encode_utf16().collect(), JsonValue::Null),
        ("reference_at".encode_utf16().collect(), JsonValue::Null),
        ("reference_kind".encode_utf16().collect(), JsonValue::Null),
        (
            "relative_humidity_percent".encode_utf16().collect(),
            JsonValue::Null,
        ),
        ("site_key".encode_utf16().collect(), string("ballydidean")),
        (
            "source_config_fingerprints".encode_utf16().collect(),
            array(vec![]),
        ),
        ("source_keys".encode_utf16().collect(), array(vec![])),
        (
            "target_lead_hours".encode_utf16().collect(),
            JsonValue::Null,
        ),
        ("temperature_c".encode_utf16().collect(), JsonValue::Null),
        ("upstream_model".encode_utf16().collect(), JsonValue::Null),
        (
            "valid_at".encode_utf16().collect(),
            string("2026-10-01T12:00:00.000Z"),
        ),
        (
            "wind_direction_degrees".encode_utf16().collect(),
            JsonValue::Null,
        ),
        ("wind_gust_mps".encode_utf16().collect(), JsonValue::Null),
        ("wind_speed_mps".encode_utf16().collect(), JsonValue::Null),
    ])
}

/// Replace one known fixture field without changing its closed shape.
fn set_field(row: &mut JsonValue, name: &str, value: JsonValue) {
    let JsonValue::Object(fields) = row else {
        panic!("fixture row must be an object");
    };
    let key: Vec<u16> = name.encode_utf16().collect();
    let field = fields
        .iter_mut()
        .find(|(candidate, _)| *candidate == key)
        .expect("fixture field must exist");
    field.1 = value;
}

/// Build one populated fixed-lead Previous Runs row from the closed fixture shape.
fn fixed_lead_row() -> JsonValue {
    let mut row = station_gap_row();
    set_field(
        &mut row,
        "adapter_contracts",
        array(vec![string("previous-runs-hourly/v1")]),
    );
    set_field(
        &mut row,
        "content_hashes",
        array(vec![string(&"a".repeat(64))]),
    );
    set_field(
        &mut row,
        "contract_epoch",
        string("open-meteo-previous-runs-best-match/2026-09"),
    );
    set_field(&mut row, "dataset", string("previous_runs"));
    set_field(&mut row, "exclusion_reason_codes", array(vec![]));
    set_field(
        &mut row,
        "ingestion_run_ids",
        array(vec![JsonValue::Number(7.0)]),
    );
    set_field(&mut row, "physical_station_key", JsonValue::Null);
    set_field(&mut row, "provider_family", JsonValue::Null);
    set_field(&mut row, "record_kind", string("fixed_lead_anchor"));
    set_field(&mut row, "received_at", string("2026-10-01T12:01:00.000Z"));
    set_field(&mut row, "reference_kind", string("fixed_lead_anchor"));
    set_field(
        &mut row,
        "source_config_fingerprints",
        array(vec![string(
            "3a311d67d08aa3f9dedc2dbb8382d4cf11f945439d50c328a93874fc0a44538e",
        )]),
    );
    set_field(
        &mut row,
        "source_keys",
        array(vec![string("open-meteo-previous-runs-v1")]),
    );
    set_field(&mut row, "target_lead_hours", JsonValue::Number(24.0));
    set_field(&mut row, "temperature_c", JsonValue::Number(12.5));
    set_field(&mut row, "upstream_model", string("best_match"));
    row
}

#[test]
/// Admit the predecessor's explicit provenance-free station grid row.
fn admits_explicit_station_gap_fixture() {
    let admitted = validate_forecast_training_package_row(
        &FixtureTime,
        station_gap_row(),
        "2026-10-01",
        "2026-10-01",
    )
    .expect("station gap fixture must remain admitted");

    assert_eq!(admitted.valid_at, "2026-10-01T12:00:00.000Z");
    assert_eq!(admitted.valid_at_epoch_ms, 0);
    assert_eq!(admitted.local_date, "2026-10-01");
    assert_eq!(
        admitted.record_kind,
        ForecastTrainingRecordKind::StationHour
    );
    assert_eq!(admitted.station_key.as_deref(), Some("ambient-merlin"));
    assert_eq!(
        admitted.member_path,
        "members/2026-10-01/station-hour/ambient-merlin.jsonl.gz"
    );
    assert!(admitted.source_identities.is_empty());
    assert!(!admitted.metrics.any_present());
}

#[test]
/// Reject an empty grid row whose required gap diagnostic disappeared.
fn rejects_undiagnosed_station_gap_fixture() {
    let mut row = station_gap_row();
    let JsonValue::Object(fields) = &mut row else {
        panic!("fixture row must be an object");
    };
    let exclusion_key: Vec<u16> = "exclusion_reason_codes".encode_utf16().collect();
    let field = fields
        .iter_mut()
        .find(|(key, _)| *key == exclusion_key)
        .expect("fixture exclusion field");
    field.1 = array(vec![]);

    assert_eq!(
        validate_forecast_training_package_row(&FixtureTime, row, "2026-10-01", "2026-10-01"),
        Err(ForecastTrainingPackageRowError::StationGap)
    );
}

#[test]
/// Admit the exact fixed-lead forecast grammar and its canonical member path.
fn admits_fixed_lead_forecast_fixture() {
    let admitted = validate_forecast_training_package_row(
        &FixtureTime,
        fixed_lead_row(),
        "2026-10-01",
        "2026-10-01",
    )
    .expect("fixed-lead fixture must remain admitted");

    assert_eq!(
        admitted.record_kind,
        ForecastTrainingRecordKind::FixedLeadAnchor
    );
    assert_eq!(admitted.station_key, None);
    assert_eq!(
        admitted.member_path,
        "members/2026-10-01/fixed-lead-anchor/forecast.jsonl.gz"
    );
    assert_eq!(
        admitted.source_identities,
        vec![FORECAST_TRAINING_SOURCE_IDENTITIES[5]]
    );
    assert_eq!(admitted.metrics.temperature_c, Some(12.5));
}
