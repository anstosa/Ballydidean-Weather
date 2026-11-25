//! Typed row coverage for the original v3 rain-control projection.

use super::{control_row, object, probability, string};
use crate::WorkerRainControlValues;
use weather_contracts::{
    JsonValue, MaintenanceShadowComparatorAuthorityKind, MaintenanceShadowServingAuthority,
    Nullable, RainMaintenancePersistenceTargetReasonEnum, RainWindPerformanceHour,
    RainWindPerformanceHourOccurrenceProbabilities,
};

const VALID_AT: &str = "2026-11-24T09:00:00.000Z";
const SOURCE_ROW_SHA256: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const RECEIPT_SHA256: &str = "2222222222222222222222222222222222222222222222222222222222222222";

/// Preserve policy-raw amount/probability fallback and the selected source-row identity.
#[test]
fn control_row_binds_raw_fallback_probabilities_and_source_identity() {
    let native = RainWindPerformanceHourOccurrenceProbabilities {
        atLeast0_1: 1.0,
        atLeast1_0: 1.0,
        atLeast2_5: 0.0,
    };
    let row = control_row(
        feature_row(),
        &RainWindPerformanceHour {
            validAt: VALID_AT.to_owned(),
            modelLeadHours: 9.0,
            rawPrecipitationMm: 1.25,
            correctedPrecipitationMm: 0.75,
            applied: true,
            reasonCode: Nullable::Null,
            occurrenceProbabilities: Nullable::Value(
                RainWindPerformanceHourOccurrenceProbabilities {
                    atLeast0_1: 0.8,
                    atLeast1_0: 0.4,
                    atLeast2_5: 0.1,
                },
            ),
            positiveAmountMm: Nullable::Value(0.75),
        },
        WorkerRainControlValues {
            native_source_probability: native,
            persistence_prediction: 1.25,
            persistence_reason: RainMaintenancePersistenceTargetReasonEnum::RawFallbackUnavailable,
            recent_volume_scale_prediction: 1.1,
            same_window_volume_scale_prediction: 1.2,
            unchanged_ordinal_prediction: 0.9,
            volume_scale_prediction: 1.3,
        },
        &MaintenanceShadowServingAuthority {
            artifactBase64: Nullable::Null,
            artifactIdentitySha256: Nullable::Null,
            artifactMemberSha256: Nullable::Null,
            authorityKind: MaintenanceShadowComparatorAuthorityKind::PolicyRaw,
            receiptBase64: "receipt".to_owned(),
            receiptMemberSha256: RECEIPT_SHA256.to_owned(),
        },
        "3".repeat(64).as_str(),
        SOURCE_ROW_SHA256,
    )
    .expect("physical fallback controls form one v3 row");

    assert_eq!(
        text(field(&row, "persistenceReason")),
        "raw_fallback_unavailable"
    );
    assert_eq!(
        field(&row, "persistenceTargetMemberSha256"),
        &JsonValue::Null
    );
    assert_eq!(text(field(&row, "sourceRowSha256")), SOURCE_ROW_SHA256);
    assert_eq!(
        text(field(&row, "incumbentPrediction64")),
        format!("{:016x}", 1.25_f64.to_bits())
    );
    assert_eq!(
        field(&row, "incumbentProbability"),
        field(&row, "nativeSourceProbability")
    );
    assert_eq!(
        text(field(&row, "volumeScalePrediction64")),
        format!("{:016x}", 1.3_f64.to_bits())
    );
}

/// Reject cumulative probabilities that increase at a heavier threshold.
#[test]
fn probability_rejects_nonnested_heads() {
    assert!(
        probability(&RainWindPerformanceHourOccurrenceProbabilities {
            atLeast0_1: 0.4,
            atLeast1_0: 0.5,
            atLeast2_5: 0.1,
        })
        .is_none()
    );
}

/// Build one already admitted feature-v2 wire row.
fn feature_row() -> JsonValue {
    object([
        (
            "features64",
            JsonValue::Array(vec![string("0000000000000000")]),
        ),
        ("modelLeadHours", JsonValue::Number(9.0)),
        (
            "rawPrecipitationMm64",
            string(&format!("{:016x}", 1.25_f64.to_bits())),
        ),
        (
            "rawTargetHourTemperatureC64",
            string(&format!("{:016x}", 12.5_f64.to_bits())),
        ),
        ("validAt", string(VALID_AT)),
    ])
}

/// Borrow one named member from an ordered dependency-free object.
fn field<'a>(value: &'a JsonValue, name: &str) -> &'a JsonValue {
    let JsonValue::Object(fields) = value else {
        panic!("fixture value must remain an object")
    };
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
        .expect("fixture field must remain present")
}

/// Decode one test-only UTF-16 JSON string.
fn text(value: &JsonValue) -> String {
    let JsonValue::String(value) = value else {
        panic!("fixture value must remain a string")
    };
    String::from_utf16(value).unwrap()
}
