//! Exact validation for the future-only v3 maintenance anchor contract.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::{
    maintenance_anchor_status::{
        MaintenanceAnchorState, MaintenanceAnchorStatusJson, anchor_state, clock_field,
        exact_anchor_keys, hash_field, nullable_hash_field, regex_text, uint64_field,
        valid_control_version, valid_source_commit,
    },
    maintenance_canonical_json::{field, text, valid_hash},
};

const BASE_KEYS: &[&str] = &[
    "actionSha256",
    "archiveCommitOrdinal",
    "burnSha256",
    "candidateArtifactRootSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "confirmationAccessSha256",
    "confirmationChunkCount",
    "contractVersion",
    "controlSha256",
    "controlVersion",
    "ctfState",
    "custodyCheckpointSha256",
    "dueKey",
    "family",
    "frontierSha256",
    "fullGraphVerifiedAt",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "inputSealSha256",
    "lifecycleLedgerRootSha256",
    "pageSha256",
    "policyReportSha256",
    "predecessorAnchorSha256",
    "publishedAt",
    "requiredInputRootSha256",
    "sequence",
    "sourceCommit",
    "workstationJournalHeadSha256",
];

const FINALIZED_KEYS: &[&str] = &["finalizationProofSha256", "finalizedAt"];

const REQUIRED_HASH_FIELDS: &[&str] = &[
    "actionSha256",
    "burnSha256",
    "candidateArtifactRootSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "confirmationAccessSha256",
    "controlSha256",
    "custodyCheckpointSha256",
    "frontierSha256",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "inputSealSha256",
    "lifecycleLedgerRootSha256",
    "pageSha256",
    "policyReportSha256",
    "requiredInputRootSha256",
    "workstationJournalHeadSha256",
];

/// validate one transferred or finalized future-only maintenance anchor
pub(crate) fn validate_v3_anchor(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<MaintenanceAnchorState> {
    let state = anchor_state(value)?;
    let exact_keys = exact_anchor_keys(value, BASE_KEYS, FINALIZED_KEYS, state);
    let family = text(value, "family")?;
    // bind the family chunk contract
    let expected_chunk_count = match family.as_str() {
        "rain" => 24.0,
        "temperature" | "wind" => 27.0,
        _ => return None,
    };
    let due_key = regex_text(value, "dueKey", codec)?;
    let mut due_parts = due_key.split('/');
    let due_prefix = due_parts.next()?;
    let due_family = due_parts.next()?;
    let due_hash = due_parts.next()?;
    let due_key_is_valid = due_prefix == "confirmation"
        && due_family == family
        && valid_hash(due_hash)
        && due_parts.next().is_none();
    let control_version = regex_text(value, "controlVersion", codec)?;
    let source_commit = regex_text(value, "sourceCommit", codec)?;
    let _archive_commit_ordinal = uint64_field(value, "archiveCommitOrdinal")?;
    let sequence = uint64_field(value, "sequence")?;
    let full_graph_verified_at = clock_field(value, "fullGraphVerifiedAt", time)?;
    let published_at = clock_field(value, "publishedAt", time)?;
    // validate every required authority hash
    let required_hashes_are_valid = REQUIRED_HASH_FIELDS
        .iter()
        .all(|field_name| hash_field(value, field_name));
    let predecessor_is_null = matches!(
        field(value, "predecessorAnchorSha256"),
        Some(JsonValue::Null)
    );
    let predecessor_is_valid = nullable_hash_field(value, "predecessorAnchorSha256")
        && ((sequence == 0) == predecessor_is_null);
    // enforce only additive finalized fields
    let finalized_fields_are_valid = match state {
        MaintenanceAnchorState::Transferred => true,
        MaintenanceAnchorState::Finalized => {
            let finalized_at = clock_field(value, "finalizedAt", time)?;
            hash_field(value, "finalizationProofSha256") && finalized_at >= published_at
        }
    };

    (exact_keys
        && text(value, "contractVersion").as_deref() == Some("adjustment-maintenance-anchor/v3")
        && matches!(
            field(value, "confirmationChunkCount"),
            Some(JsonValue::Number(count)) if *count == expected_chunk_count
        )
        && due_key_is_valid
        && valid_control_version(&control_version)
        && valid_source_commit(&source_commit)
        && required_hashes_are_valid
        && predecessor_is_valid
        && full_graph_verified_at <= published_at
        && finalized_fields_are_valid)
        .then_some(state)
}
