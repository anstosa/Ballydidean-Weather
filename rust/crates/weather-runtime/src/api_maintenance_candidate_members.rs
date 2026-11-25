//! validate the source-image shadow action and candidate-addressed projections

use weather_contracts::{AdjustmentShadowFamily, JsonValue};

use crate::api_maintenance_candidate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateError, exact_keys, family_name, field,
    text, valid_hash,
};
use crate::api_maintenance_candidate_catalog::ApiShadowCatalogEntry;
use crate::api_maintenance_candidate_files::ApiMaintenanceCandidateMembers;

const ACTION_KEYS: &[&str] = &[
    "actionKind",
    "candidateGraphSha256",
    "candidateSha256",
    "contractVersion",
    "createdAt",
    "expectedInstalledReceiptSha256",
    "expectedSettingsSha256",
    "expectedSourceCommit",
    "expectedSourceRelease",
    "family",
    "fencingToken",
    "fullMemberRootSha256",
    "lifecycleHeadSha256",
    "policyDecision",
    "policyReportSha256",
    "predecessorActionSha256",
    "reason",
    "reportCreatedAt",
    "siteKey",
    "validThrough",
];
const PROJECTION_KEYS: &[&str] = &[
    "actionSha256",
    "artifactSha256",
    "bundleSha256",
    "candidateGraphSha256",
    "candidateSha256",
    "contractVersion",
    "family",
    "paritySha256",
    "registration",
    "siteKey",
];
const PARITY_KEYS: &[&str] = &[
    "candidateSha256",
    "contractVersion",
    "family",
    "retainedInputSha256",
    "retainedNativeOutputSha256",
    "retainedPackagedOutputSha256",
    "syntheticInputSha256",
    "syntheticNativeOutputSha256",
    "syntheticPackagedOutputSha256",
];
const PARITY_IDENTITY_KEYS: &[&str] = &[
    "retainedInputSha256",
    "retainedNativeOutputSha256",
    "retainedPackagedOutputSha256",
    "syntheticInputSha256",
    "syntheticNativeOutputSha256",
    "syntheticPackagedOutputSha256",
];

/// validate all canonical source-image members selected by one shadow catalog entry
pub(crate) fn validate_api_shadow_members(
    family: AdjustmentShadowFamily,
    selected: &ApiShadowCatalogEntry<'_>,
    members: &ApiMaintenanceCandidateMembers,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<(), ApiMaintenanceCandidateError> {
    let action = codec
        .parse_canonical_object(&members.action)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    let projection = codec
        .parse_canonical_object(&members.projection)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    let parity = codec
        .parse_canonical_object(&members.parity)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;

    // preserve node's projection and parity admission before action binding
    validate_projection(family, selected, &projection, codec)?;
    validate_parity(
        family,
        selected,
        &projection,
        &parity,
        &members.parity,
        codec,
    )?;
    validate_action(family, selected, &action, &members.action, codec)
}

/// validate the independently addressed pending shadow action
fn validate_action(
    family: AdjustmentShadowFamily,
    selected: &ApiShadowCatalogEntry<'_>,
    action: &JsonValue,
    bytes: &[u8],
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<(), ApiMaintenanceCandidateError> {
    // reject shape drift before inspecting values
    if !exact_keys(action, ACTION_KEYS) {
        return Err(ApiMaintenanceCandidateError::Action);
    }
    // validate every action clock and nullable ancestry identity
    if !canonical_clock(action, "createdAt", codec)
        || !canonical_clock(action, "reportCreatedAt", codec)
        || !canonical_clock(action, "validThrough", codec)
        || !nullable_hash(action, "expectedInstalledReceiptSha256")
        || !nullable_hash(action, "predecessorActionSha256")
    {
        return Err(ApiMaintenanceCandidateError::Action);
    }
    // require every action graph identity to be an exact hash
    for key in [
        "candidateGraphSha256",
        "candidateSha256",
        "expectedSettingsSha256",
        "lifecycleHeadSha256",
        "policyReportSha256",
    ] {
        if !text(action, key).is_some_and(|identity| valid_hash(&identity)) {
            return Err(ApiMaintenanceCandidateError::Action);
        }
    }
    let action_sha256 = codec
        .sha256(bytes)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    // admit only a pending inactive shadow action bound to the selected receipt
    if action_sha256.as_str() != selected.receipt.action_sha256.as_str()
        || !text_is(
            action,
            "contractVersion",
            "forecast-adjustment-model-action/v1",
        )
        || !text_is(action, "actionKind", "shadow")
        || !text_is(action, "policyDecision", "pending")
        || !text_is(action, "reason", "development_candidate")
        || !text_is(action, "siteKey", "ballydidean")
        || !text_is(action, "family", family_name(family))
        || !text_is(
            action,
            "candidateGraphSha256",
            selected.receipt.candidate_graph_sha256.as_str(),
        )
        || !text_is(
            action,
            "candidateSha256",
            selected.receipt.candidate_sha256.as_str(),
        )
        || !matches!(field(action, "fullMemberRootSha256"), Some(JsonValue::Null))
        || !text_is(
            action,
            "expectedSettingsSha256",
            selected.receipt.deployed_settings_sha256.as_str(),
        )
        || !text_is(
            action,
            "fencingToken",
            selected.receipt.fencing_token.as_str(),
        )
    {
        return Err(ApiMaintenanceCandidateError::Action);
    }
    // bind the extra v2 identities after both documents admitted the inactive disposition
    if let Some(lifecycle) = &selected.receipt.lifecycle
        && (!text_is(
            action,
            "lifecycleHeadSha256",
            lifecycle.lifecycle_head_sha256.as_str(),
        ) || !text_is(
            action,
            "policyReportSha256",
            lifecycle.policy_report_sha256.as_str(),
        ))
    {
        return Err(ApiMaintenanceCandidateError::Action);
    }
    Ok(())
}

/// validate the source-image catalog projection against the selected receipt
fn validate_projection(
    family: AdjustmentShadowFamily,
    selected: &ApiShadowCatalogEntry<'_>,
    projection: &JsonValue,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<(), ApiMaintenanceCandidateError> {
    // bind the exact action, graph, artifact, and database registration
    if !exact_keys(projection, PROJECTION_KEYS)
        || !text_is(
            projection,
            "contractVersion",
            "forecast-adjustment-shadow-catalog-projection/v1",
        )
        || !text_is(projection, "siteKey", "ballydidean")
        || !text_is(projection, "family", family_name(family))
        || !text_is(
            projection,
            "actionSha256",
            selected.receipt.action_sha256.as_str(),
        )
        || !text_is(
            projection,
            "candidateGraphSha256",
            selected.receipt.candidate_graph_sha256.as_str(),
        )
        || !text_is(
            projection,
            "candidateSha256",
            selected.receipt.candidate_sha256.as_str(),
        )
        || !text_is(
            projection,
            "artifactSha256",
            selected.receipt.bundle_sha256.as_str(),
        )
        || !text_is(
            projection,
            "bundleSha256",
            selected.receipt.bundle_sha256.as_str(),
        )
        || !text(projection, "paritySha256").is_some_and(|identity| valid_hash(&identity))
    {
        return Err(ApiMaintenanceCandidateError::Projection);
    }
    let projected_registration =
        field(projection, "registration").ok_or(ApiMaintenanceCandidateError::Projection)?;
    let projected_bytes = codec
        .encode_canonical_json(projected_registration)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    let selected_bytes = codec
        .encode_canonical_json(selected.registration)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    // preserve canonical JSON equality instead of relying on object field order
    if projected_bytes != selected_bytes {
        return Err(ApiMaintenanceCandidateError::Projection);
    }
    Ok(())
}

/// validate one exact native-to-packaged parity receipt
fn validate_parity(
    family: AdjustmentShadowFamily,
    selected: &ApiShadowCatalogEntry<'_>,
    projection: &JsonValue,
    parity: &JsonValue,
    bytes: &[u8],
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<(), ApiMaintenanceCandidateError> {
    // validate every retained and synthetic byte identity
    if !exact_keys(parity, PARITY_KEYS) {
        return Err(ApiMaintenanceCandidateError::Parity);
    }
    for key in PARITY_IDENTITY_KEYS {
        if !text(parity, key).is_some_and(|identity| valid_hash(&identity)) {
            return Err(ApiMaintenanceCandidateError::Parity);
        }
    }
    // reject addressing drift before invoking the hashing boundary
    if !text_is(
        parity,
        "contractVersion",
        "forecast-adjustment-model-parity/v1",
    ) || !text_is(
        parity,
        "candidateSha256",
        selected.receipt.candidate_sha256.as_str(),
    ) || !text_is(parity, "family", family_name(family))
    {
        return Err(ApiMaintenanceCandidateError::Parity);
    }
    let parity_sha256 = codec
        .sha256(bytes)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    if text(projection, "paritySha256").as_deref() != Some(parity_sha256.as_str())
        || text(parity, "retainedNativeOutputSha256")
            != text(parity, "retainedPackagedOutputSha256")
        || text(parity, "syntheticNativeOutputSha256")
            != text(parity, "syntheticPackagedOutputSha256")
    {
        return Err(ApiMaintenanceCandidateError::Parity);
    }
    Ok(())
}

/// recognize one nullable lowercase SHA-256 identity
fn nullable_hash(value: &JsonValue, key: &str) -> bool {
    // accept only exact null or one validated string
    matches!(field(value, key), Some(JsonValue::Null))
        || text(value, key).is_some_and(|identity| valid_hash(&identity))
}

/// recognize one canonical UTC clock field
fn canonical_clock(
    value: &JsonValue,
    key: &str,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> bool {
    // keep time grammar owned by the shared candidate boundary
    text(value, key).is_some_and(|clock| codec.is_canonical_utc_instant(&clock))
}

/// compare one required JSON string without lossy conversion
fn text_is(value: &JsonValue, key: &str, expected: &str) -> bool {
    // compare decoded UTF-16 text through the shared helper
    text(value, key).as_deref() == Some(expected)
}
