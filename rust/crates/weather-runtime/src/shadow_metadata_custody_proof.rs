//! Exact admission for compact shadow metadata custody proofs.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::{
    maintenance_anchor_status::{
        MaintenanceAnchorStatusJson, clock_field, hash_field, regex_text, uint64_field,
    },
    maintenance_anchor_status_v2::valid_scheduler_due_key,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
};

const CONTRACT_VERSION: &str = "adjustment-shadow-metadata-custody-proof/v1";
const AUTHORITY: &str = "shadow_metadata_custody_only";
const REVISION_PROJECTION_MAXIMUM_BYTES: f64 = (3_500 * 1_024) as f64;
const PROOF_KEYS: &[&str] = &[
    "acknowledgementSha256",
    "archiveCommitOrdinal",
    "authority",
    "contractVersion",
    "custodyCheckpointSha256",
    "entries",
    "frontierSha256",
    "memberRootSha256",
    "pageSha256",
    "preparedAt",
    "proofSha256",
];
const PROOF_HASH_FIELDS: &[&str] = &[
    "acknowledgementSha256",
    "custodyCheckpointSha256",
    "frontierSha256",
    "memberRootSha256",
    "pageSha256",
    "proofSha256",
];
const ENTRY_KEYS: &[&str] = &[
    "archiveCommitOrdinal",
    "archiveCommittedAt",
    "bodyByteCount",
    "capsuleSha256",
    "dueKey",
    "frontierSha256",
    "inputSha256",
    "issuedAt",
    "maxValidAt",
    "minValidAt",
    "predictionBodySha256",
    "predictionCommittedAt",
    "predictionSchemaSha256",
    "predictionSha256",
    "predecessorFrontierSha256",
    "receiptSha256",
    "registrationSha256",
    "rowCount",
    "sourceReceiptSha256",
    "stageReceiptSha256",
];
const ENTRY_HASH_FIELDS: &[&str] = &[
    "capsuleSha256",
    "frontierSha256",
    "inputSha256",
    "predictionBodySha256",
    "predictionSchemaSha256",
    "predecessorFrontierSha256",
    "receiptSha256",
    "sourceReceiptSha256",
    "stageReceiptSha256",
];

/// validate one compact shadow metadata proof
pub(crate) fn validate_shadow_custody_proof(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<()> {
    // close the proof contract before reading members
    exact_keys(value, PROOF_KEYS).then_some(())?;
    (text(value, "contractVersion").as_deref() == Some(CONTRACT_VERSION)
        && text(value, "authority").as_deref() == Some(AUTHORITY))
    .then_some(())?;
    uint64_field(value, "archiveCommitOrdinal")?;
    clock_field(value, "preparedAt", time)?;
    // admit every exact proof digest
    PROOF_HASH_FIELDS
        .iter()
        .all(|key| hash_field(value, key))
        .then_some(())?;
    // retain only the original one-or-two entry bound
    let JsonValue::Array(entries) = field(value, "entries")? else {
        return None;
    };
    (1..=2).contains(&entries.len()).then_some(())?;
    // validate rows before checking their exact lexical wire order
    let identities = entries
        .iter()
        .map(|entry| validate_shadow_custody_entry(entry, codec, time))
        .collect::<Option<Vec<_>>>()?;
    identities
        .windows(2)
        .all(|pair| pair[0] <= pair[1])
        .then_some(())?;
    crate::shadow_metadata_custody::canonical_self_address(value, "proofSha256", codec)
}

/// validate one capsule-derived compact row identity
fn validate_shadow_custody_entry(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<(String, String)> {
    // reject extensions before resolving any typed field
    exact_keys(value, ENTRY_KEYS).then_some(())?;
    uint64_field(value, "archiveCommitOrdinal")?;
    let archive_committed_at = clock_field(value, "archiveCommittedAt", time)?;
    let issued_at = clock_field(value, "issuedAt", time)?;
    clock_field(value, "maxValidAt", time)?;
    clock_field(value, "minValidAt", time)?;
    let prediction_committed_at = clock_field(value, "predictionCommittedAt", time)?;
    // admit all non-ordering row digests
    ENTRY_HASH_FIELDS
        .iter()
        .all(|key| hash_field(value, key))
        .then_some(())?;
    let registration_sha256 = text(value, "registrationSha256")?;
    let prediction_sha256 = text(value, "predictionSha256")?;
    (valid_hash(&registration_sha256) && valid_hash(&prediction_sha256)).then_some(())?;
    let due_key = regex_text(value, "dueKey", codec)?;
    // preserve the original numeric and causal bounds only
    (bounded_integer_field(
        value,
        "bodyByteCount",
        2.0,
        REVISION_PROJECTION_MAXIMUM_BYTES,
    ) && bounded_integer_field(value, "rowCount", 1.0, 168.0)
        && valid_scheduler_due_key(&due_key)
        && issued_at <= prediction_committed_at
        && prediction_committed_at <= archive_committed_at)
        .then_some((registration_sha256, prediction_sha256))
}

/// admit one finite integral JSON number in the closed bound
fn bounded_integer_field(value: &JsonValue, key: &str, minimum: f64, maximum: f64) -> bool {
    matches!(
        field(value, key),
        Some(JsonValue::Number(number))
            if number.is_finite()
                && number.fract() == 0.0
                && (minimum..=maximum).contains(number)
    )
}
