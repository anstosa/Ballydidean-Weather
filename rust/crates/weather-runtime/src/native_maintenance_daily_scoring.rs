//! Exact candidate report and semantic receipt projection after admitted daily evidence.

use crate::{
    NativeMaintenanceCaptureFamily, NativeMaintenanceDailyEvaluation, NativeMaintenanceDue,
    NativeMaintenanceDueMode, NativeMaintenancePolicyKind, NativePredecessorModelCalendar,
    NativeWirePrimitives, VerifiedNativeMaintenanceCaptureEpochWitness,
    eval_native_maintenance_policy,
    maintenance_canonical_json::{text, valid_hash},
    native_maintenance_policy_comparison::{decode_native_maintenance_policy_row, object, string},
    native_maintenance_retirement_files_v2::format_digest,
    verify_native_maintenance_due,
};
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

/// Exact commitments from the verified registration, access and completed confirmation manifest.
pub struct NativeMaintenanceDailyScoringMaterial<'a> {
    pub catalog_input_manifest_sha256: &'a str,
    pub due: &'a NativeMaintenanceDue,
    pub witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    pub family: NativeMaintenanceCaptureFamily,
    pub candidate_sha256: &'a str,
    pub confirmation_registration_sha256: &'a str,
    pub registration_sha256: &'a str,
    pub full_member_root_sha256: &'a str,
    pub target_cutoff_at: &'a str,
    pub rows: &'a [JsonValue],
}

/// Evaluate admitted confirmation rows under the original promotion policy and produce exact report bytes.
pub fn finalize_native_maintenance_daily_scoring(
    input: NativeMaintenanceDailyScoringMaterial<'_>,
) -> Option<NativeMaintenanceDailyEvaluation> {
    let wire = NativeWirePrimitives;
    // retain only exact original registered/root commitments and canonical due time
    if input.due.mode != NativeMaintenanceDueMode::Daily
        || !verify_native_maintenance_due(input.due)
        || [
            input.catalog_input_manifest_sha256,
            &input.witness.witness_sha256,
            input.candidate_sha256,
            input.confirmation_registration_sha256,
            input.registration_sha256,
            input.full_member_root_sha256,
        ]
        .iter()
        .any(|value| !valid_hash(value))
    {
        return None;
    }
    wire.canonical_utc_milliseconds(input.target_cutoff_at)?;
    let calendar = NativePredecessorModelCalendar::new()?;
    let row_hashes = input
        .rows
        .iter()
        .map(|value| {
            let row = decode_native_maintenance_policy_row(value, &calendar)?;
            // require the independently identified original evaluation row
            (row.family == input.family && valid_hash(&row.row_sha256))
                .then(|| string(&row.row_sha256))
        })
        .collect::<Option<Vec<_>>>()?;
    let evaluation_rows_sha256 =
        format_digest(wire.sha256(&wire.encode_sorted_json(&JsonValue::Array(row_hashes))?));
    let policy = eval_native_maintenance_policy(
        input.family,
        NativeMaintenancePolicyKind::Promotion,
        input.rows,
        None,
    )?;
    let policy_bytes = wire.encode_sorted_json(&policy)?;
    let policy_report_sha256 = format_digest(wire.sha256(&policy_bytes));
    let candidate_report = object(vec![
        ("candidateSha256", string(input.candidate_sha256)),
        (
            "confirmationRegistrationSha256",
            string(input.confirmation_registration_sha256),
        ),
        (
            "contractVersion",
            string("adjustment-maintenance-candidate-report/v1"),
        ),
        ("evaluationRowsSha256", string(&evaluation_rows_sha256)),
        ("family", string(input.family.wire())),
        (
            "fullMemberRootSha256",
            string(input.full_member_root_sha256),
        ),
        ("policyReportSha256", string(&policy_report_sha256)),
        ("registrationSha256", string(input.registration_sha256)),
        ("targetCutoffAt", string(input.target_cutoff_at)),
    ]);
    let candidate_report_bytes = wire.encode_sorted_json(&candidate_report)?;
    let candidate_report_sha256 = format_digest(wire.sha256(&candidate_report_bytes));
    let due = object(vec![
        ("dueKey", string(&input.due.due_key)),
        ("family", JsonValue::Null),
        ("mode", string("daily")),
        ("originalCutoffAt", string(&input.due.original_cutoff_at)),
        ("scope", string("daily")),
    ]);
    let semantic_material = object(vec![
        (
            "catalogInputManifestSha256",
            string(input.catalog_input_manifest_sha256),
        ),
        ("candidateReportSha256", string(&candidate_report_sha256)),
        ("due", due),
        ("epochWitnessSha256", string(&input.witness.witness_sha256)),
        (
            "fullMemberRootSha256",
            string(input.full_member_root_sha256),
        ),
    ]);
    let semantic_input_sha256 =
        format_digest(wire.sha256(&wire.encode_sorted_json(&semantic_material)?));
    let policy_state = text(&policy, "state")?;
    let policy_action = text(&policy, "action")?;
    let report = object(vec![
        (
            "candidateReportBase64",
            string(&wire.encode_standard_base64(&candidate_report_bytes)?),
        ),
        ("candidateReportSha256", string(&candidate_report_sha256)),
        (
            "contractVersion",
            string("adjustment-maintenance-daily-evaluation/v1"),
        ),
        ("family", string(input.family.wire())),
        (
            "fullMemberRootSha256",
            string(input.full_member_root_sha256),
        ),
        ("policy", policy),
        (
            "policyBytesBase64",
            string(&wire.encode_standard_base64(&policy_bytes)?),
        ),
        ("policyReportSha256", string(&policy_report_sha256)),
        ("rowCount", JsonValue::Number(input.rows.len() as f64)),
        ("semanticInputSha256", string(&semantic_input_sha256)),
        ("state", string("evaluated")),
    ]);
    // accept only the exact source promotion policy state/action combinations
    match (policy_state.as_str(), policy_action.as_str()) {
        ("pass", "promote") => Some(NativeMaintenanceDailyEvaluation::Promote {
            semantic_input_sha256,
            value: report,
        }),
        ("fail", "retain") => Some(NativeMaintenanceDailyEvaluation::Reject {
            semantic_input_sha256,
            value: report,
        }),
        ("pending", "pending") => Some(NativeMaintenanceDailyEvaluation::SupportFailed {
            semantic_input_sha256,
            value: report,
        }),
        _ => None,
    }
}
