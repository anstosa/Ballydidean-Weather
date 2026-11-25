//! Verification of the retained legacy temperature canary bundle.

use weather_contracts::JsonValue;

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_legacy_values::{
    boolean_field, canonical_clock, canonical_object_hash_matches, hash_field, integer_field,
    number_field, text_equals, validate_text_field,
};
use crate::maintenance_canonical_json::{exact_keys, field, text};

const BUNDLE_KEYS: &[&str] = &[
    "artifactKind",
    "authorization",
    "bundleSha256",
    "contractVersion",
    "evidence",
    "model",
    "runtimeFingerprint",
    "servedForecastIdentity",
    "siteKey",
    "timezone",
    "trainingForecastIdentity",
];
const AUTHORIZATION_KEYS: &[&str] = &[
    "activatedAt",
    "authorizationReason",
    "authorizationSha256",
    "authorized",
    "authorizedAt",
    "authorizedBy",
    "expiresAt",
];
const PERMANENT_AUTHORIZATION_KEYS: &[&str] = &[
    "activatedAt",
    "artifactKind",
    "authorizationReason",
    "authorizationSha256",
    "authorized",
    "authorizedAt",
    "authorizedBy",
    "contractVersion",
    "expiresAt",
    "permanent",
];
const EVIDENCE_KEYS: &[&str] = &[
    "modelSourceSha256",
    "researchSummarySha256",
    "retentionManifestSha256",
    "strengthSourceSha256",
];
const RUNTIME_FINGERPRINT_KEYS: &[&str] = &["icuVersion", "tzdataVersion"];
const SERVED_IDENTITY_KEYS: &[&str] = &[
    "adapterVersion",
    "dataset",
    "maximumReceiptAgeHours",
    "providerKey",
    "sourceDelayHours",
    "upstreamModel",
];
const TRAINING_IDENTITY_KEYS: &[&str] = &["cohort", "scope"];
const MODEL_KEYS: &[&str] = &[
    "adaptiveCoefficients",
    "cohort",
    "contractVersion",
    "directCoefficients",
    "learnedStrengthContractVersion",
    "month",
    "scope",
    "strengthBands",
    "supported",
    "trainingCutoffUtc",
];
const PERMANENT_MODEL_KEYS: &[&str] = &[
    "adaptiveCoefficients",
    "cohort",
    "contractVersion",
    "directCoefficients",
    "effectiveFrom",
    "latestTrainingValidAt",
    "learnedStrengthContractVersion",
    "scope",
    "strengthBands",
    "supported",
    "trainingCutoffUtc",
];
const STRENGTH_KEYS: &[&str] = &["alpha", "supported", "trainingCutoffUtc"];
const STRENGTH_BAND_KEYS: &[&str] = &["1-6", "7-12"];
const V1_BUNDLE: &str = "forecast-adjustment-temperature-canary-bundle/v1";
const V2_BUNDLE: &str = "forecast-adjustment-temperature-canary-bundle/v2";
const MONTHLY_MODEL: &str = "temperature-shortlead-models-research/v1";
const PERMANENT_MODEL: &str = "temperature-permanent-model/v1";
const LEARNED_STRENGTH: &str = "temperature-winner-extensions-research/v1";
const PERMANENT_AUTHORIZATION: &str = "forecast-adjustment-temperature-canary-authorization/v2";
const MAXIMUM_AUTHORIZATION_DURATION_MS: i64 = 14 * 24 * 60 * 60 * 1_000;
const SEVEN_HOURS_MS: i64 = 7 * 3_600_000;

/// Verify one complete legacy temperature runtime bundle against its content identity.
pub(crate) fn verify_legacy_temperature_bundle(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
    identity: &str,
) -> Option<()> {
    exact_keys(doc, BUNDLE_KEYS).then_some(())?;
    // bind the transfer-canary bundle to its fixed site and runtime identity
    if !text_equals(doc, "artifactKind", "ecmwf_temperature_transfer_canary")
        || !text_equals(doc, "siteKey", "ballydidean")
        || !text_equals(doc, "timezone", "America/Los_Angeles")
    {
        return None;
    }
    let bundle_sha256 = hash_field(doc, "bundleSha256")?;
    // retain the caller's exact selected bundle identity
    if bundle_sha256 != identity {
        return None;
    }

    let authorization = field(doc, "authorization")?;
    let model = field(doc, "model")?;
    let contract_version = text(doc, "contractVersion")?;
    // keep finite receipts monthly and permanent receipts static
    match contract_version.as_str() {
        V1_BUNDLE => {
            validate_authorization(values, authorization)?;
            text_equals(model, "contractVersion", MONTHLY_MODEL).then_some(())?;
        }
        V2_BUNDLE => {
            validate_permanent_authorization(values, authorization)?;
            text_equals(model, "contractVersion", PERMANENT_MODEL).then_some(())?;
        }
        _ => return None,
    }

    validate_evidence(field(doc, "evidence")?)?;
    validate_runtime_fingerprint(field(doc, "runtimeFingerprint")?)?;
    validate_served_identity(field(doc, "servedForecastIdentity")?)?;
    validate_training_identity(field(doc, "trainingForecastIdentity")?)?;
    validate_sanitized_model(values, model)?;
    canonical_object_hash_matches(values, doc, "bundleSha256", &bundle_sha256)
}

/// Validate the four immutable evidence member identities.
fn validate_evidence(node: &JsonValue) -> Option<()> {
    exact_keys(node, EVIDENCE_KEYS).then_some(())?;
    // require every retained evidence member to be one lowercase content digest
    EVIDENCE_KEYS
        .iter()
        .all(|key| hash_field(node, key).is_some())
        .then_some(())
}

/// Validate the two bounded runtime calendar labels.
fn validate_runtime_fingerprint(node: &JsonValue) -> Option<()> {
    exact_keys(node, RUNTIME_FINGERPRINT_KEYS).then_some(())?;
    validate_text_field(node, "icuVersion")?;
    validate_text_field(node, "tzdataVersion")
}

/// Validate the exact live single-run serving identity.
fn validate_served_identity(node: &JsonValue) -> Option<()> {
    exact_keys(node, SERVED_IDENTITY_KEYS).then_some(())?;
    let maximum_receipt_age = integer_field(node, "maximumReceiptAgeHours")?;
    // retain the source delay and bounded receipt lifetime used by the live sidecar
    (text_equals(node, "dataset", "single_run")
        && text_equals(node, "adapterVersion", "open-meteo-ecmwf-single-run/v1")
        && text_equals(node, "providerKey", "open-meteo")
        && text_equals(node, "upstreamModel", "ecmwf_ifs")
        && number_field(node, "sourceDelayHours")? == 6.0
        && (6..=12).contains(&maximum_receipt_age))
    .then_some(())
}

/// Validate the disclosed hindcast-to-live transfer boundary.
fn validate_training_identity(node: &JsonValue) -> Option<()> {
    exact_keys(node, TRAINING_IDENTITY_KEYS).then_some(())?;
    (text_equals(node, "cohort", "ecmwf_single_run_hindcast")
        && text_equals(node, "scope", "assumed_delay6_next12"))
    .then_some(())
}

/// Validate the exact sanitized monthly or permanent model.
fn validate_sanitized_model(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    model: &JsonValue,
) -> Option<()> {
    let contract_version = text(model, "contractVersion")?;
    let permanent = contract_version == PERMANENT_MODEL;
    exact_keys(
        model,
        if permanent {
            PERMANENT_MODEL_KEYS
        } else {
            MODEL_KEYS
        },
    )
    .then_some(())?;
    // close the supported delayed model identity before reading fitted material
    if !matches!(contract_version.as_str(), MONTHLY_MODEL | PERMANENT_MODEL)
        || !text_equals(model, "learnedStrengthContractVersion", LEARNED_STRENGTH)
        || !text_equals(model, "cohort", "ecmwf_single_run_hindcast")
        || !text_equals(model, "scope", "assumed_delay6_next12")
        || !boolean_field(model, "supported")?
    {
        return None;
    }
    let training_cutoff_text = text(model, "trainingCutoffUtc")?;
    let training_cutoff = values.canonical_utc_epoch_milliseconds(&training_cutoff_text)?;

    // preserve monthly identity or bind the permanent model to its causal fit
    if permanent {
        let effective_from = canonical_clock(values, model, "effectiveFrom")?;
        let latest_training = canonical_clock(values, model, "latestTrainingValidAt")?;
        if latest_training.checked_add(SEVEN_HOURS_MS)? > training_cutoff
            || training_cutoff > effective_from
        {
            return None;
        }
    } else if !valid_model_month(&text(model, "month")?) {
        return None;
    }

    coefficient_vector(model, "directCoefficients", 35)?;
    coefficient_vector(model, "adaptiveCoefficients", 49)?;
    let strength_bands = field(model, "strengthBands")?;
    exact_keys(strength_bands, STRENGTH_BAND_KEYS).then_some(())?;
    // bind both learned bands to the same supported winner and cutoff spelling
    STRENGTH_BAND_KEYS
        .iter()
        .all(|key| validate_strength_band(strength_bands, key, &training_cutoff_text).is_some())
        .then_some(())
}

/// Validate one exact learned strength band.
fn validate_strength_band(bands: &JsonValue, key: &str, cutoff: &str) -> Option<()> {
    let band = field(bands, key)?;
    exact_keys(band, STRENGTH_KEYS).then_some(())?;
    (boolean_field(band, "supported")?
        && number_field(band, "alpha")? == 1.0
        && text(band, "trainingCutoffUtc")? == cutoff)
        .then_some(())
}

/// Validate one exact finite coefficient vector.
fn coefficient_vector(model: &JsonValue, key: &str, width: usize) -> Option<()> {
    let JsonValue::Array(coefficients) = field(model, key)? else {
        return None;
    };
    // reject absent, extra, or nonfinite fitted model content
    (coefficients.len() == width
        && coefficients
            .iter()
            .all(|value| matches!(value, JsonValue::Number(number) if number.is_finite())))
    .then_some(())
}

/// Validate one finite-duration authorization receipt.
fn validate_authorization(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    authorization: &JsonValue,
) -> Option<()> {
    exact_keys(authorization, AUTHORIZATION_KEYS).then_some(())?;
    let authorization_sha256 = hash_field(authorization, "authorizationSha256")?;
    validate_text_field(authorization, "authorizedBy")?;
    validate_text_field(authorization, "authorizationReason")?;
    let authorized_at = canonical_clock(values, authorization, "authorizedAt")?;
    let activated_at = canonical_clock(values, authorization, "activatedAt")?;
    let expires_at = canonical_clock(values, authorization, "expiresAt")?;
    // require explicit authority beginning no later than activation for at most fourteen days
    if !boolean_field(authorization, "authorized")?
        || authorized_at > activated_at
        || expires_at <= activated_at
        || expires_at.checked_sub(activated_at)? > MAXIMUM_AUTHORIZATION_DURATION_MS
    {
        return None;
    }
    canonical_object_hash_matches(
        values,
        authorization,
        "authorizationSha256",
        &authorization_sha256,
    )
}

/// Validate one permanent authorization receipt.
fn validate_permanent_authorization(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    authorization: &JsonValue,
) -> Option<()> {
    exact_keys(authorization, PERMANENT_AUTHORIZATION_KEYS).then_some(())?;
    let authorization_sha256 = hash_field(authorization, "authorizationSha256")?;
    validate_text_field(authorization, "authorizedBy")?;
    validate_text_field(authorization, "authorizationReason")?;
    let authorized_at = canonical_clock(values, authorization, "authorizedAt")?;
    let activated_at = canonical_clock(values, authorization, "activatedAt")?;
    // require explicit permanent authority before activation with no expiry
    if !text_equals(
        authorization,
        "artifactKind",
        "ecmwf_temperature_transfer_canary_authorization",
    ) || !text_equals(authorization, "contractVersion", PERMANENT_AUTHORIZATION)
        || !boolean_field(authorization, "authorized")?
        || !boolean_field(authorization, "permanent")?
        || !matches!(field(authorization, "expiresAt"), Some(JsonValue::Null))
        || authorized_at > activated_at
    {
        return None;
    }
    canonical_object_hash_matches(
        values,
        authorization,
        "authorizationSha256",
        &authorization_sha256,
    )
}

/// Match the source's four-digit year and two-digit monthly material identity.
fn valid_model_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..].iter().all(u8::is_ascii_digit)
}
