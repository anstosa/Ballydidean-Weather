//! Construct the original policy-authorized promotion action from the burned complete member.

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeFamilyReleaseCurrent, NativeMaintenanceDailyEvaluation,
    NativeMaintenanceDailyTerminalValues, NativeMaintenanceInstalledTerminalMaterial,
    NativeMaintenanceReleaseLease, NativeWirePrimitives,
    maintenance_canonical_json::{field, text, valid_hash},
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
};

/// Exact original promotion authority and the complete evaluated report bytes.
pub struct NativeMaintenanceQualifiedAction {
    pub action: JsonValue,
    pub action_sha256: String,
    pub candidate_report_bytes: Vec<u8>,
    pub policy_bytes: Vec<u8>,
    pub policy_report_sha256: String,
    pub next_confirmation_eligible_at: String,
    pub semantic_input_sha256: String,
}

/// Retain only actual complete, policy-qualified, fenced source promotion material.
pub fn build_native_maintenance_qualified_action(
    material: &NativeMaintenanceInstalledTerminalMaterial,
    evaluation: &NativeMaintenanceDailyEvaluation,
    current: &NativeFamilyReleaseCurrent,
    lease: &NativeMaintenanceReleaseLease,
) -> Option<NativeMaintenanceQualifiedAction> {
    let NativeMaintenanceDailyEvaluation::Promote {
        semantic_input_sha256,
        value,
    } = evaluation
    else {
        return None;
    };
    let NativeMaintenanceDailyTerminalValues::Complete { family, .. } = &material.assembly else {
        return None;
    };
    let shadow = &material.access.shadow_registration.registration;
    let candidate = text(shadow, "candidateSha256")?;
    let full_root = text(&material.full_manifest, "fullMemberRootSha256")?;
    let policy = field(value, "policy")?;
    let policy_report_sha256 = text(value, "policyReportSha256")?;
    let candidate_report_sha256 = text(value, "candidateReportSha256")?;
    let lifecycle_head = text(&material.access.native_access, "journalHeadSha256")?;
    let created = text(&material.access.native_access, "accessedAt")?;
    let wire = NativeWirePrimitives;
    // admit only the same actual passing evaluation and retained complete member under the release scope
    if current.family != family.wire()
        || text(value, "state")?.as_str() != "evaluated"
        || text(policy, "state")?.as_str() != "pass"
        || text(policy, "action")?.as_str() != "promote"
        || text(value, "family")?.as_str() != family.wire()
        || text(value, "fullMemberRootSha256")? != full_root
        || lease.due_key != format!("confirmation/{}/{candidate}", family.wire())
        || ![
            candidate.as_str(),
            &full_root,
            semantic_input_sha256,
            &policy_report_sha256,
            &candidate_report_sha256,
            &lifecycle_head,
            &material.candidate.candidate_graph_sha256,
            &current.settings_sha256,
        ]
        .into_iter()
        .all(valid_hash)
        || lease.run_id != format!("release-promote-{}", &candidate[..32])
    {
        return None;
    }
    let policy_bytes = wire.decode_standard_base64(&text(value, "policyBytesBase64")?)?;
    let candidate_report_bytes =
        wire.decode_standard_base64(&text(value, "candidateReportBase64")?)?;
    if format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, &policy_bytes))
        != policy_report_sha256
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &wire,
            &candidate_report_bytes,
        )) != candidate_report_sha256
    {
        return None;
    }
    let created_ms = wire.canonical_utc_milliseconds(&created)?;
    let next = wire.format_utc_milliseconds(created_ms.checked_add(7 * 86_400_000)?)?;
    let action = object(vec![
        ("actionKind", string("promote")),
        (
            "candidateGraphSha256",
            string(&material.candidate.candidate_graph_sha256),
        ),
        ("candidateSha256", string(&candidate)),
        (
            "contractVersion",
            string("forecast-adjustment-model-action/v1"),
        ),
        ("createdAt", string(&created)),
        (
            "expectedInstalledReceiptSha256",
            current
                .shadow_installed_receipt_sha256
                .as_deref()
                .map(string)
                .unwrap_or(JsonValue::Null),
        ),
        ("expectedSettingsSha256", string(&current.settings_sha256)),
        ("expectedSourceCommit", string(&current.commit)),
        ("expectedSourceRelease", string(&current.release)),
        ("family", string(family.wire())),
        ("fencingToken", string(&lease.fencing_token)),
        ("fullMemberRootSha256", string(&full_root)),
        ("lifecycleHeadSha256", string(&lifecycle_head)),
        ("policyDecision", string("qualified")),
        ("policyReportSha256", string(&policy_report_sha256)),
        ("predecessorActionSha256", JsonValue::Null),
        ("reason", string("qualified_candidate")),
        ("reportCreatedAt", string(&created)),
        ("siteKey", string("ballydidean")),
        ("validThrough", string(&next)),
    ]);
    let action_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &wire,
        &wire.encode_sorted_json(&action)?,
    ));
    Some(NativeMaintenanceQualifiedAction {
        action,
        action_sha256,
        candidate_report_bytes,
        policy_bytes,
        policy_report_sha256,
        next_confirmation_eligible_at: next,
        semantic_input_sha256: semantic_input_sha256.clone(),
    })
}
