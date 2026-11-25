//! native validation for the sanitized automatic scorecard projection.

use weather_contracts::JsonValue;

use crate::forecast_scorecard_contract::{
    validate_families, validate_input_fields, validate_validity, wire,
};
use crate::forecast_scorecard_fields::{
    ScorecardClock, ScorecardContractError, member, require_array, require_count, require_enum,
    require_equal, require_instant, require_keys, require_match, require_nullable_sha256,
    require_object, require_sha256,
};

const FAMILIES: &[&str] = &["temperature", "wind", "rain"];
const POLICY_DECISIONS: &[&str] = &["pending", "qualified", "failed", "regressed"];
const ACTION_STATES: &[&str] = &[
    "none",
    "shadow_pending",
    "confirmation_registered",
    "pending_support",
    "release_pending",
    "deploying",
    "active",
    "failed",
    "expired_unapplied",
    "deployed_operator_off",
    "rolled_back_prior",
    "raw",
];
const ATTEMPT_STATES: &[&str] = &[
    "not_due",
    "due",
    "running",
    "completed",
    "failed",
    "blocked",
];
const SUCCESS_STATES: &[&str] = &["none", "succeeded", "failed"];
const DUE_STATES: &[&str] = &["not_due", "due", "overdue", "expired"];
const BACKLOG_STATES: &[&str] = &["clear", "present", "blocked"];
const OPERATOR_STATES: &[&str] = &["enabled", "disabled"];
const WARNING_CLASSES: &[&str] = &["source", "fallback", "gauge", "capture", "capacity"];
const IDENTITY_CLASSES: &[&str] = &["active", "prior", "raw", "shadow"];

/// validate the sanitized automatic policy projection.
pub(crate) fn validate_scorecard_v2(
    value: &JsonValue,
    now_ms: Option<i64>,
    clock: &impl ScorecardClock,
) -> Result<(), ScorecardContractError> {
    require_keys(
        value,
        &[
            "actionLineage",
            "actionProjectionSha256",
            "actionState",
            "contractVersion",
            "families",
            "generatedAt",
            "history",
            "identities",
            "inputs",
            "job",
            "policyDecision",
            "progress",
            "siteKey",
            "validThrough",
            "warnings",
        ],
        "scorecard",
    )?;
    require_equal(
        member(value, "contractVersion"),
        &wire("forecast-adjustment-scorecard/v2"),
        "contractVersion",
    )?;
    require_equal(member(value, "siteKey"), &wire("ballydidean"), "siteKey")?;
    validate_validity(
        member(value, "generatedAt"),
        member(value, "validThrough"),
        now_ms,
        true,
        clock,
    )?;
    require_enum(
        member(value, "policyDecision"),
        POLICY_DECISIONS,
        "policyDecision",
    )?;
    require_enum(member(value, "actionState"), ACTION_STATES, "actionState")?;
    require_sha256(
        member(value, "actionProjectionSha256"),
        "actionProjectionSha256",
    )?;
    validate_inputs_v2(member(value, "inputs"), clock)?;
    validate_action_lineage(member(value, "actionLineage"))?;
    validate_job(member(value, "job"))?;
    validate_warnings(member(value, "warnings"))?;
    validate_identities(member(value, "identities"))?;
    validate_progress(member(value, "progress"), clock)?;
    let latest_history = validate_history(member(value, "history"), clock)?;

    // bind the newest immutable projection
    if let Some(latest) = latest_history {
        let lineage = member(value, "actionLineage");
        // reject any current projection mismatch
        if member(latest, "actionProjectionSha256") != member(value, "actionProjectionSha256")
            || member(latest, "actionState") != member(value, "actionState")
            || member(latest, "policyDecision") != member(value, "policyDecision")
            || member(latest, "attemptSha256") != member(lineage, "attemptSha256")
            || member(latest, "releaseManifestSha256") != member(lineage, "releaseManifestSha256")
        {
            return Err(ScorecardContractError::new(
                "latest scorecard history does not match the current projection",
            ));
        }
    }
    validate_families(member(value, "families"), clock)
}

/// validate immutable report and frontier inputs.
fn validate_inputs_v2(
    value: &JsonValue,
    clock: &impl ScorecardClock,
) -> Result<(), ScorecardContractError> {
    require_object(value, "inputs")?;
    require_keys(
        value,
        &[
            "adjustmentEvidenceManifestSha256",
            "adjustmentEvidenceWatermarkSha256",
            "forecastTrainingManifestSha256",
            "frontierSha256",
            "inputManifestSha256",
            "localDateFrom",
            "localDateTo",
            "reportSha256",
            "reportSha256s",
            "sourceRevision",
            "targetCutoffAt",
        ],
        "inputs",
    )?;
    validate_input_fields(value, clock)?;
    require_sha256(member(value, "frontierSha256"), "inputs.frontierSha256")?;
    require_sha256(
        member(value, "inputManifestSha256"),
        "inputs.inputManifestSha256",
    )?;
    require_sha256(member(value, "reportSha256"), "inputs.reportSha256")
}

/// validate immutable action lineage.
fn validate_action_lineage(value: &JsonValue) -> Result<(), ScorecardContractError> {
    require_object(value, "actionLineage")?;
    require_keys(
        value,
        &[
            "actionSha256",
            "attemptSha256",
            "predecessorActionSha256",
            "releaseManifestSha256",
            "sourceRevision",
        ],
        "actionLineage",
    )?;
    require_sha256(member(value, "actionSha256"), "actionLineage.actionSha256")?;
    require_sha256(
        member(value, "attemptSha256"),
        "actionLineage.attemptSha256",
    )?;
    require_nullable_sha256(
        member(value, "predecessorActionSha256"),
        "actionLineage.predecessorActionSha256",
    )?;
    require_nullable_sha256(
        member(value, "releaseManifestSha256"),
        "actionLineage.releaseManifestSha256",
    )?;
    require_match(
        member(value, "sourceRevision"),
        "actionLineage.sourceRevision",
        is_revision,
    )
}

/// validate one bounded daily attempt projection.
fn validate_job(value: &JsonValue) -> Result<(), ScorecardContractError> {
    require_object(value, "job")?;
    require_keys(
        value,
        &[
            "attemptState",
            "backlogState",
            "dueState",
            "operatorState",
            "successState",
        ],
        "job",
    )?;
    require_enum(
        member(value, "attemptState"),
        ATTEMPT_STATES,
        "job.attemptState",
    )?;
    require_enum(
        member(value, "backlogState"),
        BACKLOG_STATES,
        "job.backlogState",
    )?;
    require_enum(member(value, "dueState"), DUE_STATES, "job.dueState")?;
    require_enum(
        member(value, "operatorState"),
        OPERATOR_STATES,
        "job.operatorState",
    )?;
    require_enum(
        member(value, "successState"),
        SUCCESS_STATES,
        "job.successState",
    )
}

/// validate fixed warning categories containing codes only.
fn validate_warnings(value: &JsonValue) -> Result<(), ScorecardContractError> {
    require_object(value, "warnings")?;
    require_keys(value, WARNING_CLASSES, "warnings")?;

    // bound each warning class
    for warning_class in WARNING_CLASSES {
        let path = format!("warnings.{warning_class}");
        let warnings = require_array(member(value, warning_class), 16, &path, None)?;

        // reject private warning values
        for (index, warning) in warnings.iter().enumerate() {
            require_match(warning, &format!("{path}[{index}]"), is_safe_label)?;
        }
    }
    Ok(())
}

/// validate active, shadow, prior and raw identities.
fn validate_identities(value: &JsonValue) -> Result<(), ScorecardContractError> {
    require_object(value, "identities")?;
    require_keys(value, IDENTITY_CLASSES, "identities")?;

    // bind each identity class
    for identity_class in IDENTITY_CLASSES {
        let identities = member(value, identity_class);
        let path = format!("identities.{identity_class}");
        require_object(identities, &path)?;
        require_keys(identities, FAMILIES, &path)?;

        // validate every family identity
        for family in FAMILIES {
            let family_path = format!("{path}.{family}");
            // require the raw fallback identity
            if *identity_class == "raw" {
                require_sha256(member(identities, family), &family_path)?;
            } else {
                require_nullable_sha256(member(identities, family), &family_path)?;
            }
        }
    }
    Ok(())
}

/// validate bounded confirmation and rollback progress.
fn validate_progress(
    value: &JsonValue,
    clock: &impl ScorecardClock,
) -> Result<(), ScorecardContractError> {
    require_object(value, "progress")?;
    require_keys(
        value,
        &[
            "confirmationCompletedEpochs",
            "confirmationRequiredEpochs",
            "rainCaptureExpiresAt",
            "rollbackCompletedEpochs",
            "rollbackRequiredEpochs",
        ],
        "progress",
    )?;
    let confirmation_completed = require_count(
        member(value, "confirmationCompletedEpochs"),
        "progress.confirmationCompletedEpochs",
    )?;
    let confirmation_required = require_count(
        member(value, "confirmationRequiredEpochs"),
        "progress.confirmationRequiredEpochs",
    )?;
    let rollback_completed = require_count(
        member(value, "rollbackCompletedEpochs"),
        "progress.rollbackCompletedEpochs",
    )?;
    let rollback_required = require_count(
        member(value, "rollbackRequiredEpochs"),
        "progress.rollbackRequiredEpochs",
    )?;
    require_instant(
        member(value, "rainCaptureExpiresAt"),
        "progress.rainCaptureExpiresAt",
        clock,
    )?;

    // reject impossible progress projections
    if confirmation_completed > confirmation_required || rollback_completed > rollback_required {
        return Err(ScorecardContractError::new(
            "scorecard epoch progress is invalid",
        ));
    }
    Ok(())
}

/// validate the bounded immutable action history.
fn validate_history<'a>(
    value: &'a JsonValue,
    clock: &impl ScorecardClock,
) -> Result<Option<&'a JsonValue>, ScorecardContractError> {
    let history = require_array(value, 64, "history", None)?;
    let mut previous_time = i64::MIN;

    // validate each immutable history reference
    for (index, entry) in history.iter().enumerate() {
        let path = format!("history[{index}]");
        require_object(entry, &path)?;
        require_keys(
            entry,
            &[
                "actionLineageSha256",
                "actionProjectionSha256",
                "actionState",
                "attemptSha256",
                "occurredAt",
                "policyDecision",
                "releaseManifestSha256",
            ],
            &path,
        )?;
        require_sha256(
            member(entry, "actionLineageSha256"),
            &format!("{path}.actionLineageSha256"),
        )?;
        require_sha256(
            member(entry, "actionProjectionSha256"),
            &format!("{path}.actionProjectionSha256"),
        )?;
        require_enum(
            member(entry, "actionState"),
            ACTION_STATES,
            &format!("{path}.actionState"),
        )?;
        require_sha256(
            member(entry, "attemptSha256"),
            &format!("{path}.attemptSha256"),
        )?;
        let occurred_time = require_instant(
            member(entry, "occurredAt"),
            &format!("{path}.occurredAt"),
            clock,
        )?;
        require_enum(
            member(entry, "policyDecision"),
            POLICY_DECISIONS,
            &format!("{path}.policyDecision"),
        )?;
        require_nullable_sha256(
            member(entry, "releaseManifestSha256"),
            &format!("{path}.releaseManifestSha256"),
        )?;

        // require chronological projection order
        if occurred_time < previous_time {
            return Err(ScorecardContractError::new(
                "scorecard history is not chronological",
            ));
        }
        previous_time = occurred_time;
    }
    Ok(history.last())
}

/// match a git or immutable revision digest.
fn is_revision(value: &str) -> bool {
    // accept only complete lowercase hexadecimal bytes
    matches!(value.len(), 40 | 64)
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

/// match one bounded safe warning label.
fn is_safe_label(value: &str) -> bool {
    let bytes = value.as_bytes();
    // require the leading label character
    if !(1..=80).contains(&bytes.len())
        || !(bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
    {
        return false;
    }
    // admit only the retained continuation characters
    bytes[1..].iter().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(*byte, b'_' | b'.' | b':' | b'+' | b'-')
    })
}

// include the ordinary full-document fixture
#[cfg(test)]
#[path = "forecast_scorecard_v2_test.rs"]
mod tests;
