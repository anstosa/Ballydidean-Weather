//! Typed feature-v2 projection coverage for settled worker rain serving rows.

use super::feature_document;
use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    MaintenanceReceiptScalar,
};
use weather_contracts::{
    ForecastAdjustmentMaintenanceRuntimePackage, JsonValue, RainHurdleWindFeatureProjection,
    RainHurdleWindFeatureRow, RainHurdleWindPortableArtifact, RainWindPerformanceResult,
    RainWindPredictionInput, RainWindRunProfile,
};
use weather_database::Sha256Digest;
use weather_worker::{
    RainAdjustmentEvaluation, RainAdjustmentHour, RainAdjustmentPersistedRevision,
    RainAdjustmentRun,
};

const RUN_AT: &str = "fixture-run";
const DECISION_AT: &str = "fixture-decision";
const GENERATED_AT: &str = "fixture-generated";
const HOUR_MILLISECONDS: i64 = 3_600_000;
const ROW_COUNT: usize = 23;
const FEATURE_COUNT: usize = 107;

struct Values;

impl ApiMaintenanceReceiptCanonicalValues for Values {
    /// Map only the synthetic ordered run and lead clocks used by this typed fixture.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        if value == RUN_AT {
            return Some(0);
        }
        // retain each lead number in the clock spelling without implementing a date parser
        value
            .strip_prefix("fixture-valid-")?
            .parse::<i64>()
            .ok()?
            .checked_mul(HOUR_MILLISECONDS)
    }

    /// Document construction does not hash bytes before the owning encoder runs.
    fn sha256(&self, _: &[u8]) -> Option<Sha256Digest> {
        unreachable!("feature document construction must not hash bytes")
    }

    /// Document construction does not derive the separate logical-key digest.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!("feature document construction must not hash the logical key")
    }
}

impl ApiMaintenanceShadowEvidenceValues for Values {
    /// This typed fixture never parses JSON bytes.
    fn decode_shadow_json(&self, _: &[u8]) -> Option<JsonValue> {
        unreachable!("feature document fixture starts from typed values")
    }

    /// This test inspects the constructed document before serialization.
    fn encode_shadow_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!("feature document fixture does not serialize")
    }

    /// Feature revision construction has no comparator base64 input.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!("feature document fixture has no comparator")
    }

    /// Feature revision construction does not request sorted JSON.
    fn encode_shadow_sorted_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!("feature document fixture does not sort JSON")
    }

    /// Feature revision construction does not decode maintenance packages.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        unreachable!("feature document fixture has no maintenance package")
    }

    /// Feature revision construction does not decode portable artifacts.
    fn decode_shadow_rain_artifact(&self, _: &JsonValue) -> Option<RainHurdleWindPortableArtifact> {
        unreachable!("feature document fixture has no portable artifact")
    }

    /// General wind compatibility is outside the rain feature document.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        unreachable!("feature document fixture has no general wind value")
    }
}

/// Prove the source/key/lead order and exact Float32-to-binary64/null projection.
#[test]
fn builds_ordered_feature_v2_rows_and_refuses_a_persisted_row_mismatch() {
    let evaluation = evaluation();
    let persisted = persisted(&evaluation.run);
    let document = feature_document(&Values, &evaluation, &persisted)
        .expect("matching settled serving rows build a feature revision");

    assert_eq!(
        object_keys(field(&document, "logicalKey")),
        ["inputSha256", "modelSha256", "runInitializedAt",]
    );
    assert_eq!(
        object_keys(field(&document, "source")),
        [
            "adapterVersion",
            "contractEpoch",
            "dataset",
            "providerKey",
            "sourceConfigFingerprint",
            "sourceId",
            "sourceKey",
            "sourceKind",
            "upstreamModel",
        ]
    );
    let JsonValue::Array(rows) = field(&document, "rows") else {
        panic!("feature rows must remain an ordered array")
    };
    assert_eq!(rows.len(), ROW_COUNT);
    for (index, row) in rows.iter().enumerate() {
        let lead = 9 + index;
        assert_eq!(
            field(row, "modelLeadHours"),
            &JsonValue::Number(lead as f64)
        );
        assert_eq!(text(field(row, "validAt")), format!("fixture-valid-{lead}"));
    }
    let JsonValue::Array(features) = field(&rows[0], "features64") else {
        panic!("feature cells must remain an ordered array")
    };
    assert_eq!(features.len(), FEATURE_COUNT);
    assert_eq!(features[0], JsonValue::Null);
    assert_eq!(
        text(&features[1]),
        format!("{:016x}", f64::from(0.1_f32).to_bits())
    );

    let mut mismatched = persisted;
    mismatched.hours[0].raw_precipitation_mm += 1.0;
    assert!(feature_document(&Values, &evaluation, &mismatched).is_none());
}

/// Build one evaluation with the frozen 9-through-31 lead sequence.
fn evaluation() -> RainAdjustmentEvaluation {
    let feature_rows = (9..=31).map(feature_row).collect::<Vec<_>>();
    let serving_hours = feature_rows
        .iter()
        .map(|row| RainAdjustmentHour {
            applied: true,
            corrected_precipitation_mm: row.rawPrecipitationMm,
            model_lead_hours: row.modelLeadHours,
            raw_precipitation_mm: row.rawPrecipitationMm,
            reason: None,
            valid_at: row.validAt.clone(),
        })
        .collect::<Vec<_>>();
    RainAdjustmentEvaluation {
        causal_input: RainWindPredictionInput {
            currentRun: RainWindRunProfile {
                runInitializedAt: RUN_AT.to_owned(),
                completedAt: GENERATED_AT.to_owned(),
                hours: Vec::new(),
            },
            priorRuns: Vec::new(),
            stationHours: Vec::new(),
            nowUtc: DECISION_AT.to_owned(),
        },
        captures: Vec::new(),
        current_claim_id: "fixture-claim".to_owned(),
        feature_projection: RainHurdleWindFeatureProjection {
            decisionAt: DECISION_AT.to_owned(),
            rows: feature_rows,
            runInitializedAt: RUN_AT.to_owned(),
        },
        incumbent_performance: RainWindPerformanceResult {
            modelSha256: "9".repeat(64),
            modelMonth: "2026-11".to_owned(),
            decisionAt: DECISION_AT.to_owned(),
            hours: Vec::new(),
        },
        run: RainAdjustmentRun {
            decision_at: DECISION_AT.to_owned(),
            first_received_at: GENERATED_AT.to_owned(),
            forecast_claim_id: "fixture-claim".to_owned(),
            generated_at: GENERATED_AT.to_owned(),
            hours: serving_hours,
            input_sha256: "1".repeat(64),
            model_sha256: "2".repeat(64),
            run_initialized_at: RUN_AT.to_owned(),
        },
    }
}

/// Build one 107-cell row with missing and exactly promoted Float32 values.
fn feature_row(lead: usize) -> RainHurdleWindFeatureRow {
    let mut features = vec![0.0_f32; FEATURE_COUNT];
    features[0] = f32::NAN;
    features[1] = 0.1_f32;
    RainHurdleWindFeatureRow {
        features,
        modelLeadHours: lead as f64,
        rawPrecipitationMm: lead as f64 / 10.0,
        rawTargetHourTemperatureC: 12.5,
        validAt: format!("fixture-valid-{lead}"),
    }
}

/// Mirror the exact PostgreSQL-authored serving row for revision binding.
fn persisted(run: &RainAdjustmentRun) -> RainAdjustmentPersistedRevision {
    RainAdjustmentPersistedRevision {
        generated_at: GENERATED_AT.to_owned(),
        hours: run.hours.clone(),
        input_sha256: run.input_sha256.clone(),
        model_sha256: run.model_sha256.clone(),
        run_initialized_at: run.run_initialized_at.clone(),
        stored_content_sha256: "3".repeat(64),
    }
}

/// Borrow one named field from an ordered dependency-free object.
fn field<'a>(value: &'a JsonValue, name: &str) -> &'a JsonValue {
    let JsonValue::Object(fields) = value else {
        panic!("fixture value must remain an object")
    };
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
        .expect("fixture field must remain present")
}

/// Return ordered field spellings without normalizing their order.
fn object_keys(value: &JsonValue) -> Vec<String> {
    let JsonValue::Object(fields) = value else {
        panic!("fixture value must remain an object")
    };
    fields
        .iter()
        .map(|(key, _)| String::from_utf16(key).unwrap())
        .collect()
}

/// Decode one test-only UTF-16 JSON string value.
fn text(value: &JsonValue) -> String {
    let JsonValue::String(value) = value else {
        panic!("fixture value must remain a string")
    };
    String::from_utf16(value).unwrap()
}
