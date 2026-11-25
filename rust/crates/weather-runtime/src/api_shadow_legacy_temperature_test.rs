//! Compact ordinary coverage for a permanent legacy temperature bundle.

use weather_contracts::{
    ForecastAdjustmentMaintenanceRuntimePackage, JsonValue, RainHurdleWindPortableArtifact,
};
use weather_database::Sha256Digest;

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_legacy_temperature::verify_legacy_temperature_bundle;
use crate::{ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar};

const AUTHORIZATION_SHA256: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BUNDLE_SHA256: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const EVIDENCE_SHA256: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const AUTHORIZED_AT: &str = "2026-10-01T00:00:00.000Z";
const ACTIVATED_AT: &str = "2026-10-01T01:00:00.000Z";
const TRAINING_CUTOFF: &str = "2026-10-01T07:00:00.000Z";
const EFFECTIVE_FROM: &str = "2026-11-01T00:00:00.000Z";

struct Oracle;

impl ApiMaintenanceReceiptCanonicalValues for Oracle {
    /// Resolve only the causal fixture clocks.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        match value {
            AUTHORIZED_AT => Some(0),
            ACTIVATED_AT => Some(3_600_000),
            TRAINING_CUTOFF => Some(7 * 3_600_000),
            EFFECTIVE_FROM => Some(31 * 24 * 3_600_000),
            _ => None,
        }
    }

    /// Return the two deterministic canonical object identities.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        match bytes {
            b"authorization-preimage" => Some(Sha256Digest::admitted(AUTHORIZATION_SHA256)),
            b"bundle-preimage" => Some(Sha256Digest::admitted(BUNDLE_SHA256)),
            _ => None,
        }
    }

    /// Legacy bundle verification does not encode receipt field lists.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!("legacy temperature verification must not encode receipts")
    }
}

impl ApiMaintenanceShadowEvidenceValues for Oracle {
    /// The fixture starts from an already decoded bundle.
    fn decode_shadow_json(&self, _: &[u8]) -> Option<JsonValue> {
        unreachable!("legacy temperature verification must not parse raw JSON")
    }

    /// Ordered wire encoding is outside the decoded bundle verifier.
    fn encode_shadow_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!("legacy temperature verification must not encode ordered JSON")
    }

    /// Embedded base64 belongs to the outer authority verifier.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!("legacy temperature verification must not decode base64")
    }

    /// Map only the two exact self-addressed Node-style preimages.
    fn encode_shadow_sorted_json(&self, doc: &JsonValue) -> Option<Vec<u8>> {
        // recognize the authorization with only its self address removed
        if doc == &without_field(&authorization(), "authorizationSha256") {
            return Some(b"authorization-preimage".to_vec());
        }
        let valid_bundle = without_field(&bundle(), "bundleSha256");
        let boundary_bundle =
            without_field(&bundle_with_model(model(ACTIVATED_AT)), "bundleSha256");
        // let the boundary fixture reach causal validation independently of hashing
        if doc == &valid_bundle || doc == &boundary_bundle {
            return Some(b"bundle-preimage".to_vec());
        }
        None
    }

    /// Maintenance packages are disjoint from a legacy canary bundle.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        unreachable!("legacy temperature verification must not decode a maintenance package")
    }

    /// Portable rain models are disjoint from a temperature canary bundle.
    fn decode_shadow_rain_artifact(&self, _: &JsonValue) -> Option<RainHurdleWindPortableArtifact> {
        unreachable!("legacy temperature verification must not decode rain")
    }

    /// This fixture never examines a general wind runtime string.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        unreachable!()
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

/// Remove one self-address member while retaining every other field.
fn without_field(value: &JsonValue, key: &str) -> JsonValue {
    let JsonValue::Object(fields) = value else {
        panic!("fixture value must be an object");
    };
    JsonValue::Object(
        fields
            .iter()
            .filter(|(name, _)| !name.iter().copied().eq(key.encode_utf16()))
            .cloned()
            .collect(),
    )
}

/// Build one permanent authorization in the original closed shape.
fn authorization() -> JsonValue {
    object(vec![
        ("activatedAt", string(ACTIVATED_AT)),
        (
            "artifactKind",
            string("ecmwf_temperature_transfer_canary_authorization"),
        ),
        ("authorizationReason", string("approved permanent model")),
        ("authorizationSha256", string(AUTHORIZATION_SHA256)),
        ("authorized", JsonValue::Boolean(true)),
        ("authorizedAt", string(AUTHORIZED_AT)),
        ("authorizedBy", string("operator")),
        (
            "contractVersion",
            string("forecast-adjustment-temperature-canary-authorization/v2"),
        ),
        ("expiresAt", JsonValue::Null),
        ("permanent", JsonValue::Boolean(true)),
    ])
}

/// Build one learned strength band tied to the permanent fit cutoff.
fn strength_band() -> JsonValue {
    object(vec![
        ("alpha", JsonValue::Number(1.0)),
        ("supported", JsonValue::Boolean(true)),
        ("trainingCutoffUtc", string(TRAINING_CUTOFF)),
    ])
}

/// Build one schema-complete permanent temperature model.
fn model(latest_training_valid_at: &str) -> JsonValue {
    object(vec![
        (
            "adaptiveCoefficients",
            JsonValue::Array(vec![JsonValue::Number(0.0); 49]),
        ),
        ("cohort", string("ecmwf_single_run_hindcast")),
        ("contractVersion", string("temperature-permanent-model/v1")),
        (
            "directCoefficients",
            JsonValue::Array(vec![JsonValue::Number(0.0); 35]),
        ),
        ("effectiveFrom", string(EFFECTIVE_FROM)),
        ("latestTrainingValidAt", string(latest_training_valid_at)),
        (
            "learnedStrengthContractVersion",
            string("temperature-winner-extensions-research/v1"),
        ),
        ("scope", string("assumed_delay6_next12")),
        (
            "strengthBands",
            object(vec![("1-6", strength_band()), ("7-12", strength_band())]),
        ),
        ("supported", JsonValue::Boolean(true)),
        ("trainingCutoffUtc", string(TRAINING_CUTOFF)),
    ])
}

/// Build one complete permanent v2 legacy bundle.
fn bundle() -> JsonValue {
    bundle_with_model(model(AUTHORIZED_AT))
}

/// Bind one supplied model into the complete legacy envelope.
fn bundle_with_model(model: JsonValue) -> JsonValue {
    object(vec![
        ("artifactKind", string("ecmwf_temperature_transfer_canary")),
        ("authorization", authorization()),
        ("bundleSha256", string(BUNDLE_SHA256)),
        (
            "contractVersion",
            string("forecast-adjustment-temperature-canary-bundle/v2"),
        ),
        (
            "evidence",
            object(vec![
                ("modelSourceSha256", string(EVIDENCE_SHA256)),
                ("researchSummarySha256", string(EVIDENCE_SHA256)),
                ("retentionManifestSha256", string(EVIDENCE_SHA256)),
                ("strengthSourceSha256", string(EVIDENCE_SHA256)),
            ]),
        ),
        ("model", model),
        (
            "runtimeFingerprint",
            object(vec![
                ("icuVersion", string("icu-76")),
                ("tzdataVersion", string("2026a")),
            ]),
        ),
        (
            "servedForecastIdentity",
            object(vec![
                ("adapterVersion", string("open-meteo-ecmwf-single-run/v1")),
                ("dataset", string("single_run")),
                ("maximumReceiptAgeHours", JsonValue::Number(12.0)),
                ("providerKey", string("open-meteo")),
                ("sourceDelayHours", JsonValue::Number(6.0)),
                ("upstreamModel", string("ecmwf_ifs")),
            ]),
        ),
        ("siteKey", string("ballydidean")),
        ("timezone", string("America/Los_Angeles")),
        (
            "trainingForecastIdentity",
            object(vec![
                ("cohort", string("ecmwf_single_run_hindcast")),
                ("scope", string("assumed_delay6_next12")),
            ]),
        ),
    ])
}

#[test]
fn admits_one_self_addressed_permanent_v2_legacy_bundle() {
    assert_eq!(
        verify_legacy_temperature_bundle(&Oracle, &bundle(), BUNDLE_SHA256),
        Some(())
    );
}

#[test]
fn rejects_a_permanent_model_without_the_seven_hour_training_gap() {
    let too_recent = bundle_with_model(model(ACTIVATED_AT));
    assert_eq!(
        verify_legacy_temperature_bundle(&Oracle, &too_recent, BUNDLE_SHA256),
        None
    );
}
