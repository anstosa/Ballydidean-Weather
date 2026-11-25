//! Compact ordinary binding coverage for decoded incumbent comparator documents.

use weather_contracts::{JsonValue, MaintenanceShadowPredictionMetadata};

use crate::api_shadow_comparator_validation::validate_shadow_comparator_document;

const REGISTRATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CANDIDATE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SOURCE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const SOURCE_PROJECTION: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PREDICTION_BODY: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const ROW: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const DUE_KEY: &str = "capture/2026-11-01T00:35:00.000Z";
const ISSUED_AT: &str = "2026-11-01T00:35:00.000Z";
const VALID_AT: &str = "2026-11-01T01:35:00.000Z";
const VALUE: &str = "4028000000000000";
const PROBABILITY: &str = "3fc999999999999a";

#[derive(Clone, Copy)]
enum Family {
    Temperature,
    Wind,
    Rain,
}

impl Family {
    /// Return the original family spelling.
    const fn name(self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::Wind => "wind",
            Self::Rain => "rain",
        }
    }
}

/// Build one ordered dependency-free JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one dependency-free JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build the already admitted prediction row consumed only for binding.
fn body_row(family: Family) -> JsonValue {
    match family {
        Family::Temperature => object(vec![
            ("validAt", string(VALID_AT)),
            ("leadHours", JsonValue::Number(1.0)),
            ("sourceRowSha256", string(ROW)),
            ("candidateTemperatureC64", string(VALUE)),
            ("wouldApply", JsonValue::Boolean(false)),
            ("fallbackCode", string("model_unavailable")),
        ]),
        Family::Wind => object(vec![
            ("validAt", string(VALID_AT)),
            ("leadHours", JsonValue::Number(1.0)),
            ("sourceRowSha256", string(ROW)),
            ("candidateSpeedMps64", string(VALUE)),
            ("candidateGustMps64", string(VALUE)),
            ("speedWouldApply", JsonValue::Boolean(false)),
            ("gustWouldApply", JsonValue::Boolean(false)),
        ]),
        Family::Rain => object(vec![
            ("validAt", string(VALID_AT)),
            ("leadHours", JsonValue::Number(1.0)),
            ("sourceRowSha256", string(ROW)),
            ("occurrenceProbability64", string(PROBABILITY)),
            ("positiveAmountMm64", string(VALUE)),
            ("candidatePrecipitationMm64", string(VALUE)),
            ("atLeast1_0Probability64", string(PROBABILITY)),
            ("atLeast2_5Probability64", string(PROBABILITY)),
            ("wouldApply", JsonValue::Boolean(false)),
            ("fallbackCode", string("model_unavailable")),
        ]),
    }
}

/// Build one trusted prediction body with the common binding fields.
fn body(family: Family) -> JsonValue {
    object(vec![
        (
            "contractVersion",
            string("adjustment-shadow-prediction-values/v3"),
        ),
        ("family", string(family.name())),
        ("registrationSha256", string(REGISTRATION)),
        ("candidateSha256", string(CANDIDATE)),
        ("sourceSha256", string(SOURCE)),
        ("dueKey", string(DUE_KEY)),
        ("issuedAt", string(ISSUED_AT)),
        ("sourceReceiptSha256", string(ROW)),
        ("inputSha256", string(SOURCE_PROJECTION)),
        ("rowCount", JsonValue::Number(1.0)),
        ("rows", JsonValue::Array(vec![body_row(family)])),
    ])
}

/// Build one family comparator row in the original TypeScript key order.
fn comparator_row(family: Family) -> JsonValue {
    match family {
        Family::Temperature => object(vec![
            ("validAt", string(VALID_AT)),
            ("leadHours", JsonValue::Number(1.0)),
            ("sourceRowSha256", string(ROW)),
            ("incumbentTemperatureC64", string(VALUE)),
            ("applied", JsonValue::Boolean(false)),
            ("reasonCode", string("bundle_missing")),
        ]),
        Family::Wind => object(vec![
            ("validAt", string(VALID_AT)),
            ("leadHours", JsonValue::Number(1.0)),
            ("sourceRowSha256", string(ROW)),
            ("incumbentSpeedMps64", string(VALUE)),
            ("incumbentGustMps64", string(VALUE)),
            ("speedApplied", JsonValue::Boolean(false)),
            ("gustApplied", JsonValue::Boolean(false)),
            ("reasonCode", string("metric_not_enabled")),
        ]),
        Family::Rain => object(vec![
            ("validAt", string(VALID_AT)),
            ("leadHours", JsonValue::Number(1.0)),
            ("sourceRowSha256", string(ROW)),
            ("incumbentPrecipitationMm64", string(VALUE)),
            ("occurrenceProbability64", string(PROBABILITY)),
            ("atLeast1_0Probability64", string(PROBABILITY)),
            ("atLeast2_5Probability64", string(PROBABILITY)),
            ("positiveAmountMm64", string(VALUE)),
            ("applied", JsonValue::Boolean(false)),
            ("reasonCode", string("policy_raw")),
        ]),
    }
}

/// Build one comparator document in the frozen top-level key order.
fn comparator(family: Family) -> JsonValue {
    object(vec![
        (
            "contractVersion",
            string("adjustment-shadow-incumbent-comparator/v1"),
        ),
        ("family", string(family.name())),
        ("registrationSha256", string(REGISTRATION)),
        ("candidateSha256", string(CANDIDATE)),
        ("sourceSha256", string(SOURCE)),
        ("dueKey", string(DUE_KEY)),
        ("issuedAt", string(ISSUED_AT)),
        ("sourceProjectionSha256", string(SOURCE_PROJECTION)),
        ("predictionBodySha256", string(PREDICTION_BODY)),
        ("servingAuthority", JsonValue::Null),
        ("rowCount", JsonValue::Number(1.0)),
        ("rows", JsonValue::Array(vec![comparator_row(family)])),
    ])
}

/// Build metadata identities already derived from the admitted body and source.
fn metadata() -> MaintenanceShadowPredictionMetadata {
    MaintenanceShadowPredictionMetadata {
        bodyByteCount: 1,
        candidateSha256: CANDIDATE.to_owned(),
        dueKey: DUE_KEY.to_owned(),
        inputSha256: SOURCE_PROJECTION.to_owned(),
        issuedAt: ISSUED_AT.to_owned(),
        maxValidAt: VALID_AT.to_owned(),
        minValidAt: VALID_AT.to_owned(),
        predictionBodySha256: PREDICTION_BODY.to_owned(),
        predictionSchemaSha256: "1111111111111111111111111111111111111111111111111111111111111111"
            .to_owned(),
        predictionSha256: "2222222222222222222222222222222222222222222222222222222222222222"
            .to_owned(),
        registrationSha256: REGISTRATION.to_owned(),
        rowCount: 1,
        sourceReceiptSha256: ROW.to_owned(),
        sourceSha256: SOURCE.to_owned(),
    }
}

/// Replace one known object member without changing its wire position.
fn replace_field(object: &mut JsonValue, key: &str, replacement: JsonValue) {
    let JsonValue::Object(fields) = object else {
        panic!("fixture value must be an object");
    };
    let value = fields
        .iter_mut()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
        .expect("fixture object must retain the requested field");
    *value = replacement;
}

/// Replace one known comparator row member without changing its wire position.
fn replace_row_field(comparator: &mut JsonValue, key: &str, replacement: JsonValue) {
    let JsonValue::Object(fields) = comparator else {
        panic!("fixture comparator must be an object");
    };
    let JsonValue::Array(rows) = fields
        .iter_mut()
        .find_map(|(name, value)| {
            name.iter()
                .copied()
                .eq("rows".encode_utf16())
                .then_some(value)
        })
        .expect("fixture comparator must retain rows")
    else {
        panic!("fixture comparator rows must be an array");
    };
    replace_field(
        rows.first_mut().expect("fixture must retain one row"),
        key,
        replacement,
    );
}

#[test]
fn all_three_family_comparators_bind_to_the_trusted_body_and_metadata() {
    let metadata = metadata();
    // prove each frozen family row grammar and reason vocabulary independently
    for family in [Family::Temperature, Family::Wind, Family::Rain] {
        assert!(
            validate_shadow_comparator_document(&comparator(family), &body(family), &metadata)
                .is_some()
        );
    }
}

#[test]
fn comparator_rejects_capsule_row_and_reason_substitution() {
    let metadata = metadata();

    let mut temperature = comparator(Family::Temperature);
    replace_row_field(&mut temperature, "sourceRowSha256", string(CANDIDATE));
    assert!(
        validate_shadow_comparator_document(&temperature, &body(Family::Temperature), &metadata,)
            .is_none()
    );

    let mut wind = comparator(Family::Wind);
    replace_field(&mut wind, "predictionBodySha256", string(CANDIDATE));
    assert!(validate_shadow_comparator_document(&wind, &body(Family::Wind), &metadata).is_none());

    let mut rain = comparator(Family::Rain);
    replace_row_field(&mut rain, "reasonCode", string("prediction_missing"));
    assert!(validate_shadow_comparator_document(&rain, &body(Family::Rain), &metadata).is_none());
}
