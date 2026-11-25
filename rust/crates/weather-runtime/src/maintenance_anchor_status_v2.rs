//! validate the original v2 maintenance anchor contract

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::maintenance_anchor_status::{
    MaintenanceAnchorState, MaintenanceAnchorStatusJson, anchor_state, clock_field,
    exact_anchor_keys, hash_field, nullable_hash_field, regex_text, uint64_field,
    valid_control_version, valid_source_commit,
};
use crate::maintenance_canonical_json::{field, text};

const CONTRACT_VERSION: &str = "adjustment-maintenance-anchor/v2";
const BASE_KEYS: &[&str] = &[
    "actionSha256",
    "archiveObjectSha256",
    "catalogGeneration",
    "catalogRootSha256",
    "catalogWatermarkSha256",
    "contractVersion",
    "controlSha256",
    "controlVersion",
    "ctfState",
    "dueKey",
    "frontierCount",
    "frontierRootSha256",
    "fullGraphVerifiedAt",
    "fullMemberRootSha256",
    "generation",
    "graphManifestSha256",
    "lifecycleLedgerRootSha256",
    "manifestSha256",
    "predecessorAnchorSha256",
    "publishedAt",
    "reportSha256",
    "revisionSnapshotRootSha256",
    "sequence",
    "sourceCommit",
    "workstationJournalHeadSha256",
];
const FINALIZED_KEYS: &[&str] = &[
    "archiveCommitOrdinal",
    "burnSha256",
    "catalogRevisionReceiptRootSha256",
    "confirmationChunkCount",
    "finalizationProofSha256",
    "finalizedAt",
    "inputSealSha256",
    "requiredInputRootSha256",
    "retirementSetRootSha256",
];
const REQUIRED_HASH_FIELDS: &[&str] = &[
    "archiveObjectSha256",
    "catalogRootSha256",
    "catalogWatermarkSha256",
    "controlSha256",
    "frontierRootSha256",
    "graphManifestSha256",
    "lifecycleLedgerRootSha256",
    "manifestSha256",
    "workstationJournalHeadSha256",
];
const NULLABLE_HASH_FIELDS: &[&str] = &[
    "actionSha256",
    "fullMemberRootSha256",
    "predecessorAnchorSha256",
    "reportSha256",
    "revisionSnapshotRootSha256",
];
const FINALIZED_HASH_FIELDS: &[&str] = &[
    "burnSha256",
    "catalogRevisionReceiptRootSha256",
    "finalizationProofSha256",
    "inputSealSha256",
    "requiredInputRootSha256",
    "retirementSetRootSha256",
];
const FINALIZED_REQUIRED_PROJECTION_FIELDS: &[&str] = &[
    "actionSha256",
    "fullMemberRootSha256",
    "reportSha256",
    "revisionSnapshotRootSha256",
];

/// Admit the complete original v2 anchor contract after canonical decoding.
pub(crate) fn validate_v2_anchor(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<MaintenanceAnchorState> {
    let state = anchor_state(value)?;
    let control_version = regex_text(value, "controlVersion", codec)?;
    let source_commit = regex_text(value, "sourceCommit", codec)?;
    // close the original contract and regex-coerced identity fields
    if !exact_anchor_keys(value, BASE_KEYS, FINALIZED_KEYS, state)
        || text(value, "contractVersion")? != CONTRACT_VERSION
        || !valid_control_version(&control_version)
        || !valid_source_commit(&source_commit)
        || !valid_scheduler_due_key(&text(value, "dueKey")?)
    {
        return None;
    }

    // retain all three decimal uint64 identities
    uint64_field(value, "sequence")?;
    uint64_field(value, "generation")?;
    uint64_field(value, "catalogGeneration")?;
    let full_graph_verified_at = clock_field(value, "fullGraphVerifiedAt", time)?;
    let published_at = clock_field(value, "publishedAt", time)?;

    // require every transfer root and every nullable projection spelling
    if !REQUIRED_HASH_FIELDS
        .iter()
        .all(|key| hash_field(value, key))
        || !NULLABLE_HASH_FIELDS
            .iter()
            .all(|key| nullable_hash_field(value, key))
    {
        return None;
    }

    // validate the additive finalized-only roots and causal clock
    if state == MaintenanceAnchorState::Finalized {
        uint64_field(value, "archiveCommitOrdinal")?;
        let finalized_at = clock_field(value, "finalizedAt", time)?;
        if !FINALIZED_HASH_FIELDS
            .iter()
            .all(|key| hash_field(value, key))
            || !FINALIZED_REQUIRED_PROJECTION_FIELDS
                .iter()
                .all(|key| hash_field(value, key))
            || !matches!(
                field(value, "confirmationChunkCount"),
                Some(JsonValue::Number(count)) if *count == 24.0 || *count == 27.0
            )
            || finalized_at < published_at
        {
            return None;
        }
    }

    // keep the frontier bounded and preserve graph-before-publication order
    if !matches!(
        field(value, "frontierCount"),
        Some(JsonValue::Number(count))
            if count.is_finite()
                && count.fract() == 0.0
                && (1.0..=9_500_000.0).contains(count)
    ) || full_graph_verified_at > published_at
    {
        return None;
    }
    Some(state)
}

/// Match the original scheduler grammar without calendar normalization.
pub(crate) fn valid_scheduler_due_key(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix("capture/") else {
        return false;
    };
    let shape = b"dddd-dd-ddTdd:35:00.000Z";
    // accept digits only in the regex variable positions
    suffix.len() == shape.len()
        && shape
            .iter()
            .zip(suffix.bytes())
            .all(|(&expected, observed)| {
                // preserve each literal separator
                if expected == b'd' {
                    observed.is_ascii_digit()
                } else {
                    observed == expected
                }
            })
        && matches!(&suffix.as_bytes()[11..13], b"00" | b"06" | b"12" | b"18")
}
