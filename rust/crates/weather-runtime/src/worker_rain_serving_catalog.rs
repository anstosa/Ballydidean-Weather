//! Admit root-qualified active model catalogs and public serving members.

use weather_contracts::{AdjustmentShadowFamily, JsonValue};
use weather_database::ApiMaintenanceRegistrationVersion;

use crate::api_maintenance_candidate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateError, exact_keys, family_name, field,
    text, valid_hash,
};
use crate::api_maintenance_candidate_catalog::{
    parse_installed_candidate_receipt, select_installed_catalog_entry,
    validate_installed_registration,
};

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
const REGISTRY_KEYS: &[&str] = &["activePackage", "contractVersion", "rawReason", "siteKey"];
const REGISTRY_PACKAGE_KEYS: &[&str] =
    &["actionSha256", "artifactSha256", "candidateSha256", "path"];

/// Root-qualified serving identities retained after catalog and registration admission.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WorkerRainServingCatalog<'a> {
    pub raw_receipt: &'a JsonValue,
    pub registration: &'a JsonValue,
    pub registration_version: ApiMaintenanceRegistrationVersion,
    pub action_sha256: String,
    pub candidate_graph_sha256: String,
    pub candidate_sha256: String,
    pub bundle_sha256: String,
    pub source_sha256: String,
    pub deployed_settings_sha256: String,
    pub fencing_token: String,
    pub full_member_root_sha256: String,
    pub lifecycle_head_sha256: String,
    pub policy_report_sha256: String,
}

/// Select one family's sole qualified active slot and bind its database registration.
pub(crate) fn select_qualified_serving_catalog<'a>(
    catalog: &'a JsonValue,
    family: AdjustmentShadowFamily,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<Option<WorkerRainServingCatalog<'a>>, ApiMaintenanceCandidateError> {
    let Some(entry) = select_installed_catalog_entry(catalog, family_name(family), "active")?
    else {
        return Ok(None);
    };
    let raw_receipt = field(entry, "receipt").ok_or(ApiMaintenanceCandidateError::Catalog)?;
    let receipt = parse_installed_candidate_receipt(raw_receipt, codec)?;
    // require the post-confirmation authority rather than a shadow or raw lifecycle receipt
    if receipt.action_kind.as_deref() != Some("promote")
        || receipt.policy_decision.as_deref() != Some("qualified")
    {
        return Err(ApiMaintenanceCandidateError::Catalog);
    }
    let full_member_root_sha256 = receipt
        .full_member_root_sha256
        .clone()
        .ok_or(ApiMaintenanceCandidateError::Catalog)?;
    let lifecycle_head_sha256 = receipt
        .lifecycle_head_sha256
        .clone()
        .ok_or(ApiMaintenanceCandidateError::Catalog)?;
    let policy_report_sha256 = receipt
        .policy_report_sha256
        .clone()
        .ok_or(ApiMaintenanceCandidateError::Catalog)?;
    let registration =
        field(entry, "registration").ok_or(ApiMaintenanceCandidateError::Registration)?;
    let registration_version =
        validate_installed_registration(registration, family, &receipt, codec)?;
    Ok(Some(WorkerRainServingCatalog {
        raw_receipt,
        registration,
        registration_version,
        action_sha256: receipt.action_sha256,
        candidate_graph_sha256: receipt.candidate_graph_sha256,
        candidate_sha256: receipt.candidate_sha256,
        bundle_sha256: receipt.bundle_sha256,
        source_sha256: receipt.source_sha256,
        deployed_settings_sha256: receipt.deployed_settings_sha256,
        fencing_token: receipt.fencing_token,
        full_member_root_sha256,
        lifecycle_head_sha256,
        policy_report_sha256,
    }))
}

/// Preserve the rain-specific selection surface for its existing worker.
pub(crate) fn select_worker_rain_serving_catalog<'a>(
    catalog: &'a JsonValue,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<Option<WorkerRainServingCatalog<'a>>, ApiMaintenanceCandidateError> {
    select_qualified_serving_catalog(catalog, AdjustmentShadowFamily::Rain, codec)
}

/// Validate one family's compiled registry and independently addressed promote action.
pub(crate) fn validate_qualified_serving_members(
    codec: &mut impl ApiMaintenanceCandidateCodec,
    selected: &WorkerRainServingCatalog<'_>,
    family: AdjustmentShadowFamily,
    registry_bytes: &[u8],
    action_bytes: &[u8],
) -> Result<(), ApiMaintenanceCandidateError> {
    let registry = codec
        .parse_canonical_object(registry_bytes)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    let action = codec
        .parse_canonical_object(action_bytes)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    validate_registry(&registry, selected, family)?;
    validate_action(&action, action_bytes, selected, family, codec)
}

/// Preserve the rain-specific member-validation surface for its existing worker.
pub(crate) fn validate_worker_rain_serving_members(
    codec: &mut impl ApiMaintenanceCandidateCodec,
    selected: &WorkerRainServingCatalog<'_>,
    registry_bytes: &[u8],
    action_bytes: &[u8],
) -> Result<(), ApiMaintenanceCandidateError> {
    validate_qualified_serving_members(
        codec,
        selected,
        AdjustmentShadowFamily::Rain,
        registry_bytes,
        action_bytes,
    )
}

/// Bind one compiled active-package registry to the selected installed receipt.
fn validate_registry(
    registry: &JsonValue,
    selected: &WorkerRainServingCatalog<'_>,
    family: AdjustmentShadowFamily,
) -> Result<(), ApiMaintenanceCandidateError> {
    let active = field(registry, "activePackage").ok_or(ApiMaintenanceCandidateError::Package)?;
    let (contract_version, artifact_directory) = serving_family_contract(family);
    let expected_path = format!(
        "{artifact_directory}/sha256-{}.json",
        selected.bundle_sha256
    );
    // require the sole qualified package and prohibit a raw fallback reason beside it
    if !exact_keys(registry, REGISTRY_KEYS)
        || !exact_keys(active, REGISTRY_PACKAGE_KEYS)
        || !text_is(registry, "contractVersion", contract_version)
        || !text_is(registry, "siteKey", "ballydidean")
        || !matches!(field(registry, "rawReason"), Some(JsonValue::Null))
        || !text_is(active, "actionSha256", &selected.action_sha256)
        || !text_is(active, "artifactSha256", &selected.bundle_sha256)
        || !text_is(active, "candidateSha256", &selected.candidate_sha256)
        || !text_is(active, "path", &expected_path)
    {
        return Err(ApiMaintenanceCandidateError::Package);
    }
    Ok(())
}

/// Bind one exact qualified promote action to every installed lifecycle identity.
fn validate_action(
    action: &JsonValue,
    bytes: &[u8],
    selected: &WorkerRainServingCatalog<'_>,
    family: AdjustmentShadowFamily,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<(), ApiMaintenanceCandidateError> {
    // close the action shape and independently admit its bounded clocks and ancestry hashes
    if !exact_keys(action, ACTION_KEYS)
        || !canonical_clock(action, "createdAt", codec)
        || !canonical_clock(action, "reportCreatedAt", codec)
        || !canonical_clock(action, "validThrough", codec)
        || !required_hash(action, "predecessorActionSha256")
        || !required_hash(action, "expectedInstalledReceiptSha256")
    {
        return Err(ApiMaintenanceCandidateError::Action);
    }
    let action_sha256 = codec
        .sha256(bytes)
        .ok_or(ApiMaintenanceCandidateError::Canonical)?;
    // match Node's qualified action cross-bind without granting authority to unbound source labels
    if action_sha256.as_str() != selected.action_sha256
        || !text_is(
            action,
            "contractVersion",
            "forecast-adjustment-model-action/v1",
        )
        || !text_is(action, "actionKind", "promote")
        || !text_is(action, "policyDecision", "qualified")
        || !text_is(action, "reason", "qualified_candidate")
        || !text_is(action, "siteKey", "ballydidean")
        || !text_is(action, "family", family_name(family))
        || !text_is(
            action,
            "candidateGraphSha256",
            &selected.candidate_graph_sha256,
        )
        || !text_is(action, "candidateSha256", &selected.candidate_sha256)
        || !text_is(
            action,
            "fullMemberRootSha256",
            &selected.full_member_root_sha256,
        )
        || !text_is(
            action,
            "lifecycleHeadSha256",
            &selected.lifecycle_head_sha256,
        )
        || !text_is(action, "policyReportSha256", &selected.policy_report_sha256)
        || !text_is(
            action,
            "expectedSettingsSha256",
            &selected.deployed_settings_sha256,
        )
        || !text_is(action, "fencingToken", &selected.fencing_token)
    {
        return Err(ApiMaintenanceCandidateError::Action);
    }
    Ok(())
}

/// Map one generated family to its fixed serving-registry contract and artifact directory.
const fn serving_family_contract(family: AdjustmentShadowFamily) -> (&'static str, &'static str) {
    match family {
        AdjustmentShadowFamily::Temperature => (
            "forecast-adjustment-temperature-maintenance-registry/v1",
            "temperature-canary-bundles",
        ),
        AdjustmentShadowFamily::Wind => (
            "forecast-adjustment-wind-maintenance-registry/v1",
            "wind-canary-bundles",
        ),
        AdjustmentShadowFamily::Rain => (
            "forecast-adjustment-rain-maintenance-registry/v1",
            "rain-runtime-artifacts",
        ),
    }
}

/// Check one required canonical UTC action clock through the approved owner.
fn canonical_clock(
    value: &JsonValue,
    key: &str,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> bool {
    text(value, key).is_some_and(|clock| codec.is_canonical_utc_instant(&clock))
}

/// Check one required lowercase SHA-256 action field.
fn required_hash(value: &JsonValue, key: &str) -> bool {
    text(value, key).is_some_and(|identity| valid_hash(&identity))
}

/// Compare one required string field without allocating a second expected value.
fn text_is(value: &JsonValue, key: &str, expected: &str) -> bool {
    text(value, key).as_deref() == Some(expected)
}
