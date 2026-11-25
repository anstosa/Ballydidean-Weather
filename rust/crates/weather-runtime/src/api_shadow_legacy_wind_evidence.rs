//! Verification of retained wind canary transfer evidence and authority.

use weather_contracts::JsonValue;

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_legacy_values::{
    boolean_field, canonical_clock, canonical_fields_equal, canonical_object_hash_matches,
    hash_field, integer_field, number_field, text_equals, validate_text_field,
};
use crate::maintenance_canonical_json::{exact_keys, field};

const TRANSFER_REPORT_KEYS: &[&str] = &[
    "artifactKind",
    "bridgeEndExclusive",
    "bridgeEvaluations",
    "bridgeStartInclusive",
    "candidateArtifactSha256",
    "contractVersion",
    "enabledMetricBands",
    "passed",
    "servedForecastIdentity",
    "trainingForecastIdentity",
    "transferReportSha256",
];
const AUTHORIZATION_KEYS: &[&str] = &[
    "activatedAt",
    "artifactKind",
    "authorizationReason",
    "authorizationSha256",
    "authorized",
    "authorizedAt",
    "authorizedBy",
    "candidateArtifactSha256",
    "contractVersion",
    "enabledMetricBands",
    "expiresAt",
    "transferReportSha256",
];
const PERMANENT_AUTHORIZATION_KEYS: &[&str] = &[
    "activatedAt",
    "artifactKind",
    "authorizationReason",
    "authorizationSha256",
    "authorized",
    "authorizedAt",
    "authorizedBy",
    "candidateArtifactSha256",
    "contractVersion",
    "enabledMetricBands",
    "expiresAt",
    "permanent",
    "transferReportSha256",
];
const BRIDGE_EVALUATION_KEYS: &[&str] = &["metricBand", "network"];
const METRIC_BAND_KEYS: &[&str] = &["leadBand", "metric"];
const BRIDGE_SCORE_KEYS: &[&str] = &["adjustedLoss", "eventCount", "rawLoss", "skill"];
const REPORT_VERSION: &str = "forecast-adjustment-wind-canary-transfer-report/v1";
const AUTHORIZATION_VERSION: &str = "forecast-adjustment-wind-canary-authorization/v1";
const PERMANENT_AUTHORIZATION_VERSION: &str = "forecast-adjustment-wind-canary-authorization/v2";
const MAXIMUM_AUTHORIZATION_DURATION_MS: i64 = 14 * 24 * 60 * 60 * 1_000;
const MAXIMUM_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// Verify one wind bridge report and its finite or permanent operator authority.
pub(crate) fn verify_wind_canary_evidence(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    authorization: &JsonValue,
    report: &JsonValue,
    candidate: &JsonValue,
    permanent: bool,
) -> Option<()> {
    let report_sha256 = verify_transfer_report(values, report, candidate)?;
    verify_authorization(
        values,
        authorization,
        report,
        candidate,
        &report_sha256,
        permanent,
    )
}

/// Verify the exact passing transfer report without repeating candidate admission.
fn verify_transfer_report(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    report: &JsonValue,
    candidate: &JsonValue,
) -> Option<String> {
    exact_keys(report, TRANSFER_REPORT_KEYS).then_some(())?;
    let candidate_sha256 = hash_field(candidate, "candidateArtifactSha256")?;
    // require the separate passing report identity and candidate address
    if !text_equals(
        report,
        "artifactKind",
        "wind_transfer_canary_transfer_report",
    ) || !text_equals(report, "contractVersion", REPORT_VERSION)
        || !boolean_field(report, "passed")?
        || hash_field(report, "candidateArtifactSha256")? != candidate_sha256
    {
        return None;
    }
    let report_sha256 = hash_field(report, "transferReportSha256")?;
    let bridge_start = canonical_clock(values, report, "bridgeStartInclusive")?;
    let bridge_end = canonical_clock(values, report, "bridgeEndExclusive")?;
    let final_training_cutoff = canonical_clock(values, candidate, "finalTrainingCutoff")?;
    // require a nonempty bridge window strictly after the fitted training material
    if bridge_start >= bridge_end || final_training_cutoff >= bridge_start {
        return None;
    }

    canonical_fields_equal(
        values,
        field(report, "enabledMetricBands")?,
        field(candidate, "enabledMetricBands")?,
    )?;
    canonical_fields_equal(
        values,
        field(report, "trainingForecastIdentity")?,
        field(candidate, "trainingForecastIdentity")?,
    )?;
    canonical_fields_equal(
        values,
        field(report, "servedForecastIdentity")?,
        field(candidate, "servedForecastIdentity")?,
    )?;
    validate_bridge_evaluations(values, report, candidate)?;
    canonical_object_hash_matches(values, report, "transferReportSha256", &report_sha256)?;
    Some(report_sha256)
}

/// Validate ordered metric coverage and every positive live-v4 score.
fn validate_bridge_evaluations(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    report: &JsonValue,
    candidate: &JsonValue,
) -> Option<()> {
    let JsonValue::Array(evaluations) = field(report, "bridgeEvaluations")? else {
        return None;
    };
    let metric_bands = evaluations
        .iter()
        .map(|evaluation| field(evaluation, "metricBand").cloned())
        .collect::<Option<Vec<_>>>()?;
    canonical_fields_equal(
        values,
        &JsonValue::Array(metric_bands),
        field(candidate, "enabledMetricBands")?,
    )?;
    // close each bridge row and retain only positive finite evidence
    evaluations
        .iter()
        .all(|evaluation| validate_bridge_evaluation(evaluation).is_some())
        .then_some(())
}

/// Validate one closed bridge evaluation and metric-band identity.
fn validate_bridge_evaluation(evaluation: &JsonValue) -> Option<()> {
    exact_keys(evaluation, BRIDGE_EVALUATION_KEYS).then_some(())?;
    let metric_band = field(evaluation, "metricBand")?;
    exact_keys(metric_band, METRIC_BAND_KEYS).then_some(())?;
    validate_bridge_score(field(evaluation, "network")?)
}

/// Validate one immutable positive bridge score with Node's binary64 tolerance.
fn validate_bridge_score(score: &JsonValue) -> Option<()> {
    exact_keys(score, BRIDGE_SCORE_KEYS).then_some(())?;
    let adjusted_loss = number_field(score, "adjustedLoss")?;
    let event_count = integer_field(score, "eventCount")?;
    let raw_loss = number_field(score, "rawLoss")?;
    let skill = number_field(score, "skill")?;
    let derived_skill = if raw_loss == 0.0 {
        if adjusted_loss == 0.0 {
            0.0
        } else {
            f64::NEG_INFINITY
        }
    } else {
        (raw_loss - adjusted_loss) / raw_loss
    };
    // preserve Number.isSafeInteger and the exact positive-score tolerance
    ((-MAXIMUM_SAFE_INTEGER..=MAXIMUM_SAFE_INTEGER).contains(&event_count)
        && event_count >= 30
        && adjusted_loss >= 0.0
        && raw_loss > 0.0
        && skill > 0.0
        && (derived_skill - skill).abs() <= f64::EPSILON * 16.0)
        .then_some(())
}

/// Verify one finite or permanent authorization linked to the accepted report.
fn verify_authorization(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    authorization: &JsonValue,
    report: &JsonValue,
    candidate: &JsonValue,
    report_sha256: &str,
    permanent: bool,
) -> Option<()> {
    exact_keys(
        authorization,
        if permanent {
            PERMANENT_AUTHORIZATION_KEYS
        } else {
            AUTHORIZATION_KEYS
        },
    )
    .then_some(())?;
    let expected_version = if permanent {
        PERMANENT_AUTHORIZATION_VERSION
    } else {
        AUTHORIZATION_VERSION
    };
    let candidate_sha256 = hash_field(candidate, "candidateArtifactSha256")?;
    // require one immutable authority linked to both accepted evidence artifacts
    if !text_equals(
        authorization,
        "artifactKind",
        "wind_transfer_canary_authorization",
    ) || !text_equals(authorization, "contractVersion", expected_version)
        || !boolean_field(authorization, "authorized")?
        || hash_field(authorization, "candidateArtifactSha256")? != candidate_sha256
        || hash_field(authorization, "transferReportSha256")? != report_sha256
    {
        return None;
    }
    canonical_fields_equal(
        values,
        field(authorization, "enabledMetricBands")?,
        field(candidate, "enabledMetricBands")?,
    )?;

    let authorization_sha256 = hash_field(authorization, "authorizationSha256")?;
    validate_text_field(authorization, "authorizationReason")?;
    validate_text_field(authorization, "authorizedBy")?;
    let authorized_at = canonical_clock(values, authorization, "authorizedAt")?;
    let activated_at = canonical_clock(values, authorization, "activatedAt")?;
    let bridge_end = canonical_clock(values, report, "bridgeEndExclusive")?;
    // choose the generation-specific authority window without weakening common chronology
    if permanent {
        if !boolean_field(authorization, "permanent")?
            || !matches!(field(authorization, "expiresAt"), Some(JsonValue::Null))
            || authorized_at < bridge_end
            || activated_at < authorized_at
        {
            return None;
        }
    } else {
        let expires_at = canonical_clock(values, authorization, "expiresAt")?;
        if authorized_at < bridge_end
            || activated_at < authorized_at
            || expires_at <= activated_at
            || expires_at.checked_sub(activated_at)? > MAXIMUM_AUTHORIZATION_DURATION_MS
        {
            return None;
        }
    }
    canonical_object_hash_matches(
        values,
        authorization,
        "authorizationSha256",
        &authorization_sha256,
    )
}
