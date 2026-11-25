//! Exact value-free maintenance attempt report and immutable archive material.

use crate::{
    NativeMaintenanceArchiveSegment, NativeMaintenanceArchiveSegmentMember, NativeWirePrimitives,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_retirement_files_v2::format_digest,
};
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

pub const NATIVE_MAINTENANCE_ATTEMPT_VERSION: &str = "forecast-adjustment-maintenance-attempt/v2";
pub const NATIVE_MAINTENANCE_ATTEMPT_MAXIMUM_BYTES: usize = 128 * 1024;
const KEYS: &[&str] = &[
    "actionEligible",
    "attemptIdentitySha256",
    "candidateGraphSha256",
    "candidateSha256",
    "contractVersion",
    "dueKey",
    "family",
    "fitReceiptSha256",
    "inputHeadSha256",
    "lifecycleHeadSha256",
    "mode",
    "originalCutoffAt",
    "reason",
    "reservedConfirmationExposed",
    "semanticInputSha256",
    "servingChanged",
    "state",
];

/// Verified immutable attempt report and its independent source identity.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceAttemptReport {
    pub value: JsonValue,
    pub canonical_bytes: Vec<u8>,
    pub report_sha256: String,
}

/// Admit the exact source archive contract without granting action authority.
pub fn verify_native_maintenance_attempt_report(
    bytes: &[u8],
) -> Option<NativeMaintenanceAttemptReport> {
    let wire = NativeWirePrimitives;
    // reject an unbounded or noncanonical stored document
    if bytes.is_empty() || bytes.len() > NATIVE_MAINTENANCE_ATTEMPT_MAXIMUM_BYTES {
        return None;
    }
    let value = wire.decode_json(bytes)?;
    if wire.encode_sorted_json(&value)?.as_slice() != bytes
        || !exact_keys(&value, KEYS)
        || text(&value, "contractVersion")? != NATIVE_MAINTENANCE_ATTEMPT_VERSION
    {
        return None;
    }
    let due = text(&value, "dueKey")?;
    let mode = text(&value, "mode")?;
    let family = nullable_text(&value, "family")?;
    let cutoff = text(&value, "originalCutoffAt")?;
    let reason = text(&value, "reason")?;
    let state = text(&value, "state")?;
    let action = boolean(&value, "actionEligible")?;
    let changed = boolean(&value, "servingChanged")?;
    // preserve the exact original closed due spelling and time grammar
    if !valid_due(&due)
        || !matches!(mode.as_str(), "daily" | "monthly")
        || family
            .as_deref()
            .is_some_and(|name| !matches!(name, "temperature" | "wind" | "rain"))
        || (mode == "monthly") != family.is_some()
        || wire.format_utc_milliseconds(wire.parse_explicit_utc_milliseconds(&cutoff)?)? != cutoff
        || boolean(&value, "reservedConfirmationExposed")?
        || (reason == "daily_candidate_promoted") != action
        || changed && !action
    {
        return None;
    }
    // admit all exact immutable addresses and nullable evidence coordinates
    for key in [
        "attemptIdentitySha256",
        "inputHeadSha256",
        "lifecycleHeadSha256",
    ] {
        if !valid_hash(&text(&value, key)?) {
            return None;
        }
    }
    let candidate_graph = nullable_hash(&value, "candidateGraphSha256")?;
    let candidate = nullable_hash(&value, "candidateSha256")?;
    let fit = nullable_hash(&value, "fitReceiptSha256")?;
    let semantic = nullable_hash(&value, "semanticInputSha256")?;
    if candidate_graph.is_some() != candidate.is_some() {
        return None;
    }
    let candidate = candidate.is_some();
    let fit = fit.is_some();
    let semantic = semantic.is_some();
    let monthly = mode == "monthly";
    // bind each source reason to the exact state and evidence-presence tuple
    let valid = match reason.as_str() {
        "development_candidate_archived" => {
            state == "completed" && candidate && fit && semantic && monthly
        }
        "control_reference_archived" => {
            state == "completed"
                && !candidate
                && !fit
                && semantic
                && monthly
                && family.as_deref() == Some("rain")
                && due.starts_with("control-reference/rain/")
        }
        "candidate_archive_unavailable" => {
            state == "blocked" && !candidate && fit && semantic && monthly
        }
        "fit_failed" => state == "failed" && !candidate && !fit && semantic && monthly,
        "candidate_parity_failed" => state == "failed" && !candidate && fit && semantic && monthly,
        "monthly_no_candidate" => state == "completed" && !candidate && !fit && semantic && monthly,
        "daily_candidate_rejected"
        | "daily_no_registered_candidate"
        | "daily_support_failed"
        | "daily_candidate_promoted" => {
            state == "completed" && !candidate && !fit && semantic && !monthly
        }
        "daily_history_pending" | "daily_promotion_pending" => {
            state == "blocked" && !candidate && !fit && semantic && !monthly
        }
        "daily_evaluation_failed" => {
            state == "failed" && !candidate && !fit && semantic && !monthly
        }
        "family_slot_busy" | "registration_schedule_unavailable" => {
            state == "blocked" && !candidate && !fit && semantic && monthly
        }
        "dependency_failed"
        | "history_unavailable"
        | "semantic_catalog_unavailable"
        | "semantic_input_blocked" => state == "blocked" && !candidate && !fit && !semantic,
        _ => false,
    };
    // retain the exact bytes and their independent archive member address
    valid.then(|| NativeMaintenanceAttemptReport {
        value,
        canonical_bytes: bytes.to_vec(),
        report_sha256: format_digest(wire.sha256(bytes)),
    })
}

/// Stable attempt identity over the exact original cutoff and due input head.
pub fn native_maintenance_attempt_identity(
    due: &str,
    input_head: &str,
    cutoff: &str,
) -> Option<String> {
    let wire = NativeWirePrimitives;
    if !valid_due(due)
        || !valid_hash(input_head)
        || wire.format_utc_milliseconds(wire.parse_explicit_utc_milliseconds(cutoff)?)? != cutoff
    {
        return None;
    }
    let value = object(vec![
        (
            "contractVersion",
            string("forecast-adjustment-maintenance-attempt-identity/v1"),
        ),
        ("dueKey", string(due)),
        ("inputHeadSha256", string(input_head)),
        ("originalCutoffAt", string(cutoff)),
    ]);
    Some(format_digest(
        wire.sha256(&wire.encode_sorted_json(&value)?),
    ))
}

/// Deterministic controller lease run id using the current source journal head.
pub fn native_maintenance_controller_run_id(due: &str, head: &str) -> Option<String> {
    if !valid_due(due) || !valid_hash(head) {
        return None;
    }
    let digest = format_digest(NativeWirePrimitives.sha256(format!("{due}\n{head}\n").as_bytes()));
    Some(format!("maintenance-{}", &digest[..32]))
}

/// Original attempt report fields assembled from genuine retained outcome evidence.
pub struct NativeMaintenanceAttemptMaterial<'a> {
    pub due_key: &'a str,
    pub mode: &'a str,
    pub family: Option<&'a str>,
    pub original_cutoff_at: &'a str,
    pub input_head_sha256: &'a str,
    pub lifecycle_head_sha256: &'a str,
    pub reason: &'a str,
    pub state: &'a str,
    pub candidate_graph_sha256: Option<&'a str>,
    pub candidate_sha256: Option<&'a str>,
    pub fit_receipt_sha256: Option<&'a str>,
    pub semantic_input_sha256: Option<&'a str>,
    pub action_eligible: bool,
    pub serving_changed: bool,
}

/// Project the exact original value-free report and verify its complete source grammar.
pub fn build_native_maintenance_attempt_report(
    input: NativeMaintenanceAttemptMaterial<'_>,
) -> Option<NativeMaintenanceAttemptReport> {
    let attempt_identity = native_maintenance_attempt_identity(
        input.due_key,
        input.input_head_sha256,
        input.original_cutoff_at,
    )?;
    let nullable = |value: Option<&str>| value.map_or(JsonValue::Null, string);
    let report = object(vec![
        ("actionEligible", JsonValue::Boolean(input.action_eligible)),
        ("attemptIdentitySha256", string(&attempt_identity)),
        (
            "candidateGraphSha256",
            nullable(input.candidate_graph_sha256),
        ),
        ("candidateSha256", nullable(input.candidate_sha256)),
        (
            "contractVersion",
            string(NATIVE_MAINTENANCE_ATTEMPT_VERSION),
        ),
        ("dueKey", string(input.due_key)),
        ("family", nullable(input.family)),
        ("fitReceiptSha256", nullable(input.fit_receipt_sha256)),
        ("inputHeadSha256", string(input.input_head_sha256)),
        ("lifecycleHeadSha256", string(input.lifecycle_head_sha256)),
        ("mode", string(input.mode)),
        ("originalCutoffAt", string(input.original_cutoff_at)),
        ("reason", string(input.reason)),
        ("reservedConfirmationExposed", JsonValue::Boolean(false)),
        ("semanticInputSha256", nullable(input.semantic_input_sha256)),
        ("servingChanged", JsonValue::Boolean(input.serving_changed)),
        ("state", string(input.state)),
    ]);
    verify_native_maintenance_attempt_report(&NativeWirePrimitives.encode_sorted_json(&report)?)
}

/// Build exactly the one-member segment used by the report publication owner.
pub fn native_maintenance_attempt_archive_segment(
    report: &NativeMaintenanceAttemptReport,
) -> Option<NativeMaintenanceArchiveSegment> {
    let verified = verify_native_maintenance_attempt_report(&report.canonical_bytes)?;
    // do not trust a caller-reconstructed report address
    if &verified != report {
        return None;
    }
    Some(NativeMaintenanceArchiveSegment {
        members: vec![NativeMaintenanceArchiveSegmentMember {
            kind: NATIVE_MAINTENANCE_ATTEMPT_VERSION.to_owned(),
            identity_sha256: NativeWirePrimitives.sha256(&report.canonical_bytes),
            payload: report.canonical_bytes.clone(),
        }],
        cross_links: Vec::new(),
    })
}

// require the source's closed due grammar without inventing calendar eligibility
fn valid_due(value: &str) -> bool {
    let parts = value.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["daily", date] => calendar_shape(date, true),
        ["monthly", "temperature" | "wind" | "rain", month]
        | ["control-reference", "rain", month] => calendar_shape(month, false),
        _ => false,
    }
}

// preserve the predecessor's fixed ASCII calendar shape
fn calendar_shape(value: &str, day: bool) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != if day { 10 } else { 7 } {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| {
        if index == 4 || day && index == 7 {
            *byte == b'-'
        } else {
            byte.is_ascii_digit()
        }
    })
}

// read a source boolean without accepting string or numeric aliases
fn boolean(value: &JsonValue, key: &str) -> Option<bool> {
    match field(value, key)? {
        JsonValue::Boolean(v) => Some(*v),
        _ => None,
    }
}

// read a source nullable string
fn nullable_text(value: &JsonValue, key: &str) -> Option<Option<String>> {
    if *field(value, key)? == JsonValue::Null {
        Some(None)
    } else {
        Some(Some(text(value, key)?))
    }
}

// read a source nullable content address
fn nullable_hash(value: &JsonValue, key: &str) -> Option<Option<String>> {
    let v = nullable_text(value, key)?;
    v.as_ref().is_none_or(|s| valid_hash(s)).then_some(v)
}

// construct one exact source canonical object
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(k, v)| (k.encode_utf16().collect(), v))
            .collect(),
    )
}

// encode one generated UTF-16 contract string
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_attempt_report_test.rs"]
mod tests;
