//! Validate one bounded retain-only metadata custody preparation.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::maintenance_anchor_status::{MaintenanceAnchorStatusJson, clock_field, hash_field};
use crate::maintenance_canonical_json::{exact_keys, field, text};
use crate::shadow_metadata_custody::canonical_self_address;

pub(crate) const CONTRACT_VERSION: &str = "adjustment-shadow-metadata-custody-preparation/v1";
const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const PREPARATION_KEYS: &[&str] = &[
    "authority",
    "contractVersion",
    "finalizations",
    "preparedAt",
    "preparationSha256",
    "proofSha256",
];
const FINALIZATION_KEYS: &[&str] = &[
    "coldCommitSha256",
    "expectedPreviousMetadataRootSha256",
    "finalDisposition",
    "fromCommittedAt",
    "generation",
    "maintenanceAnchorSha256",
    "metadataManifestSha256",
    "newMetadataRootSha256",
    "registrationSha256",
    "rowCount",
    "throughCommittedAt",
];
const FINALIZATION_HASH_KEYS: &[&str] = &[
    "coldCommitSha256",
    "expectedPreviousMetadataRootSha256",
    "maintenanceAnchorSha256",
    "metadataManifestSha256",
    "newMetadataRootSha256",
    "registrationSha256",
];

// read one positive javascript-safe integer without coercion
pub(crate) fn positive_safe_integer(value: &JsonValue, key: &str) -> Option<u64> {
    // reject nonnumeric json values without coercion
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    ((1.0..=MAXIMUM_SAFE_INTEGER).contains(number) && number.fract() == 0.0)
        .then_some(*number as u64)
}

// validate one exact retain-only metadata finalization
pub(crate) fn validate_finalization(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<String> {
    exact_keys(value, FINALIZATION_KEYS).then_some(())?;
    (text(value, "finalDisposition")?.as_str() == "retain").then_some(())?;
    let generation = positive_safe_integer(value, "generation")?;
    let row_count = positive_safe_integer(value, "rowCount")?;
    (row_count <= 2).then_some(())?;
    let from_committed_at = clock_field(value, "fromCommittedAt", time)?;
    let through_committed_at = clock_field(value, "throughCommittedAt", time)?;
    // validate every database and cold-custody identity
    FINALIZATION_HASH_KEYS
        .iter()
        .all(|key| hash_field(value, key))
        .then_some(())?;

    let previous_root = text(value, "expectedPreviousMetadataRootSha256")?;
    let manifest = text(value, "metadataManifestSha256")?;
    let through = text(value, "throughCommittedAt")?;
    let cold_commit = text(value, "coldCommitSha256")?;
    let maintenance_anchor = text(value, "maintenanceAnchorSha256")?;
    let registration = text(value, "registrationSha256")?;
    let expected_root = codec.sha256(
        format!(
            "{previous_root}\n{manifest}\n{generation}\n{row_count}\n{through}\n{cold_commit}\n{maintenance_anchor}"
        )
        .as_bytes(),
    )?;
    (from_committed_at <= through_committed_at
        && text(value, "newMetadataRootSha256")?.as_str() == expected_root.as_str())
    .then_some(registration)
}

// validate one durable preparation before owner mutation
pub(crate) fn validate_shadow_custody_preparation(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Option<()> {
    exact_keys(value, PREPARATION_KEYS).then_some(())?;
    (text(value, "authority")?.as_str() == "shadow_metadata_transfer"
        && text(value, "contractVersion")?.as_str() == CONTRACT_VERSION)
        .then_some(())?;
    clock_field(value, "preparedAt", time)?;
    (hash_field(value, "proofSha256") && hash_field(value, "preparationSha256")).then_some(())?;
    // require the original bounded finalization array
    let JsonValue::Array(finalizations) = field(value, "finalizations")? else {
        return None;
    };
    ((1..=2).contains(&finalizations.len())).then_some(())?;
    // validate every closed native function argument
    let registrations = finalizations
        .iter()
        .map(|finalization| validate_finalization(finalization, codec, time))
        .collect::<Option<Vec<_>>>()?;
    // enforce strict registration order and uniqueness
    registrations
        .windows(2)
        .all(|pair| pair[0] < pair[1])
        .then_some(())?;
    canonical_self_address(value, "preparationSha256", codec)
}
