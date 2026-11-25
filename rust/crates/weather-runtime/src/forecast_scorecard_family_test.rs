use super::*;

#[derive(Debug)]
struct TestClock;

impl ScorecardClock for TestClock {
    /// expose no parsed instants to null-only fixtures.
    fn utc_epoch_milliseconds(&self, _value: &str) -> Option<i64> {
        None
    }

    /// expose no canonical instants to null-only fixtures.
    fn canonical_utc(&self, _epoch_milliseconds: i64) -> Option<String> {
        None
    }
}

/// construct one typed json text value.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// construct one typed json object.
fn object(entries: &[(&str, JsonValue)]) -> JsonValue {
    JsonValue::Object(
        entries
            .iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value.clone()))
            .collect(),
    )
}

/// construct one valid metric object.
fn metric(unit: &str) -> JsonValue {
    object(&[
        ("adjustedBias", JsonValue::Null),
        ("adjustedMae", JsonValue::Number(1.0)),
        ("adjustedP95", JsonValue::Number(2.0)),
        ("adjustedRmse", JsonValue::Number(1.5)),
        ("deltaMae", JsonValue::Number(-0.5)),
        ("rawBias", JsonValue::Null),
        ("rawMae", JsonValue::Number(1.5)),
        ("rawP95", JsonValue::Number(3.0)),
        ("rawRmse", JsonValue::Number(2.0)),
        (
            "skillInterval95",
            object(&[
                ("lower", JsonValue::Number(5.0)),
                ("upper", JsonValue::Number(20.0)),
            ]),
        ),
        ("skillPercent", JsonValue::Number(12.5)),
        ("unit", text(unit)),
    ])
}

/// construct one valid support object.
fn support() -> JsonValue {
    object(&[
        ("dateCount", JsonValue::Number(60.0)),
        ("effectiveWeightSum", JsonValue::Number(60.0)),
        ("eventCount", JsonValue::Number(1_000.0)),
        ("excludedCount", JsonValue::Number(2.0)),
        (
            "exclusionReasons",
            object(&[("missing_target", JsonValue::Number(2.0))]),
        ),
        ("fallbackCount", JsonValue::Number(3.0)),
        (
            "fallbackReasons",
            object(&[("source_stale", JsonValue::Number(3.0))]),
        ),
        ("gapCount", JsonValue::Number(4.0)),
        ("rowCount", JsonValue::Number(1_000.0)),
        ("targetRowCount", JsonValue::Number(1_000.0)),
        ("validHourCount", JsonValue::Number(240.0)),
        ("vintageCount", JsonValue::Number(120.0)),
        ("wetDateCount", JsonValue::Number(20.0)),
        ("wetRowCount", JsonValue::Number(100.0)),
    ])
}

/// construct one valid temperature family card.
fn temperature_family(slices: Vec<JsonValue>) -> JsonValue {
    object(&[
        ("bestMatchDiagnostic", JsonValue::Null),
        ("comparisonState", text("better")),
        ("evidenceClass", text("development")),
        ("evidenceCutoffAt", JsonValue::Null),
        ("family", text("temperature")),
        ("metrics", metric("celsius")),
        ("qualificationState", text("development_only")),
        ("rainDiagnostics", JsonValue::Null),
        ("recommendation", text("retain")),
        ("servingIdentitySha256", JsonValue::Null),
        ("servingState", text("authorized_active")),
        ("slices", JsonValue::Array(slices)),
        ("support", support()),
        ("supportState", text("sufficient")),
    ])
}

/// construct one slice with a selected label.
fn slice(label: &str) -> JsonValue {
    object(&[
        ("dimension", text("horizon")),
        ("label", text(label)),
        ("metrics", metric("celsius")),
        ("rowCount", JsonValue::Number(250.0)),
    ])
}

#[test]
fn accepts_a_complete_temperature_family() {
    let value = temperature_family(vec![slice("001-024")]);

    assert!(validate_family(&value, "temperature", &TestClock).is_ok());
}

#[test]
fn rejects_rain_diagnostics_on_a_temperature_family() {
    let mut value = temperature_family(Vec::new());
    let JsonValue::Object(entries) = &mut value else {
        unreachable!("fixture is an object");
    };
    let rain_diagnostics = entries
        .iter_mut()
        .find(|(key, _)| key == &"rainDiagnostics".encode_utf16().collect::<Vec<_>>())
        .expect("fixture rain diagnostics");
    rain_diagnostics.1 = object(&[]);

    let error = validate_family(&value, "temperature", &TestClock).unwrap_err();

    assert_eq!(
        error.to_string(),
        "families.temperature.rainDiagnostics must be null"
    );
}

#[test]
fn rejects_an_unsafe_slice_label() {
    let value = temperature_family(vec![slice("unsafe label")]);

    assert!(validate_family(&value, "temperature", &TestClock).is_err());
}

#[test]
fn rejects_a_reversed_skill_interval() {
    let value = object(&[
        ("adjustedBias", JsonValue::Null),
        ("adjustedMae", JsonValue::Number(1.0)),
        ("adjustedP95", JsonValue::Number(2.0)),
        ("adjustedRmse", JsonValue::Number(1.5)),
        ("deltaMae", JsonValue::Number(-0.5)),
        ("rawBias", JsonValue::Null),
        ("rawMae", JsonValue::Number(1.5)),
        ("rawP95", JsonValue::Number(3.0)),
        ("rawRmse", JsonValue::Number(2.0)),
        (
            "skillInterval95",
            object(&[
                ("lower", JsonValue::Number(20.0)),
                ("upper", JsonValue::Number(5.0)),
            ]),
        ),
        ("skillPercent", JsonValue::Number(12.5)),
        ("unit", text("celsius")),
    ]);

    let error = validate_metric(&value, "families.temperature.metrics", "celsius").unwrap_err();

    assert_eq!(
        error.to_string(),
        "families.temperature.metrics.skillInterval95 is reversed"
    );
}
