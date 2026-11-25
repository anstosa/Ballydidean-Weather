//! Validate and select one inactive API maintenance candidate from the installed catalog.

use weather_contracts::{AdjustmentShadowFamily, JsonValue};
use weather_database::ApiMaintenanceRegistrationVersion;

use crate::api_maintenance_candidate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateError, exact_keys, family_name, field,
    text, valid_hash,
};

const CATALOG_KEYS: &[&str] = &["contractVersion", "entries"];
const ENTRY_V1_KEYS: &[&str] = &["family", "receipt", "registration"];
const ENTRY_V2_KEYS: &[&str] = &["family", "receipt", "registration", "slot"];
const RECEIPT_V1_KEYS: &[&str] = &[
    "contractVersion",
    "actionSha256",
    "registrationSha256",
    "candidateGraphSha256",
    "candidateSha256",
    "bundleSha256",
    "sourceSha256",
    "deployedCommit",
    "deployedRelease",
    "deployedImageDigest",
    "deployedSettingsSha256",
    "controlPlaneSha256",
    "fencingToken",
    "installedAt",
];
const RECEIPT_V2_KEYS: &[&str] = &[
    "contractVersion",
    "actionSha256",
    "registrationSha256",
    "candidateGraphSha256",
    "candidateSha256",
    "bundleSha256",
    "sourceSha256",
    "deployedCommit",
    "deployedRelease",
    "deployedImageDigest",
    "deployedSettingsSha256",
    "controlPlaneSha256",
    "fencingToken",
    "installedAt",
    "actionKind",
    "fullMemberRootSha256",
    "lifecycleHeadSha256",
    "policyDecision",
    "policyReportSha256",
];
const REGISTRATION_V2_KEYS: &[&str] = &[
    "artifactSha256",
    "candidateSha256",
    "cohortSha256",
    "family",
    "intervalEndAt",
    "intervalStartAt",
    "policySha256",
    "registrationSha256",
    "reservedKeySha256",
    "siteKey",
    "sourceSha256",
    "targetCutoffAt",
    "terminalAt",
];
const REGISTRATION_V3_KEYS: &[&str] = &[
    "artifactSha256",
    "candidateSha256",
    "cohortSha256",
    "family",
    "intervalEndAt",
    "intervalStartAt",
    "policySha256",
    "registrationSha256",
    "reservedKeySha256",
    "siteKey",
    "sourceSha256",
    "targetCutoffAt",
    "terminalAt",
    "epochWitnessSha256",
    "predecessorRegistrationSha256",
    "scheduleContractSha256",
];
const REGISTRATION_V3_ONLY_KEYS: &[&str] = &[
    "epochWitnessSha256",
    "predecessorRegistrationSha256",
    "scheduleContractSha256",
];

/// The retained v2 lifecycle identities after inactive authority admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApiShadowLifecycle {
    pub lifecycle_head_sha256: String,
    pub policy_report_sha256: String,
}

/// The installed action receipt fields needed by later member validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApiShadowInstalledReceipt {
    pub action_sha256: String,
    pub registration_sha256: String,
    pub candidate_graph_sha256: String,
    pub candidate_sha256: String,
    pub bundle_sha256: String,
    pub source_sha256: String,
    pub deployed_settings_sha256: String,
    pub fencing_token: String,
    pub lifecycle: Option<ApiShadowLifecycle>,
}

/// Every closed installed-candidate receipt field shared by shadow and active loaders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedInstalledCandidateReceipt {
    pub action_sha256: String,
    pub registration_sha256: String,
    pub candidate_graph_sha256: String,
    pub candidate_sha256: String,
    pub bundle_sha256: String,
    pub source_sha256: String,
    pub deployed_settings_sha256: String,
    pub fencing_token: String,
    pub action_kind: Option<String>,
    pub full_member_root_sha256: Option<String>,
    pub lifecycle_head_sha256: Option<String>,
    pub policy_decision: Option<String>,
    pub policy_report_sha256: Option<String>,
}

/// One selected catalog entry with its original registration object retained for PostgreSQL.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ApiShadowCatalogEntry<'a> {
    pub registration: &'a JsonValue,
    pub registration_version: ApiMaintenanceRegistrationVersion,
    pub receipt: ApiShadowInstalledReceipt,
}

/// Select and cross-bind one exact inactive shadow slot from the installed catalog.
pub(crate) fn select_api_shadow_catalog<'a>(
    catalog: &'a JsonValue,
    family: AdjustmentShadowFamily,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<Option<ApiShadowCatalogEntry<'a>>, ApiMaintenanceCandidateError> {
    let Some(entry) = select_installed_catalog_entry(catalog, family_name(family), "shadow")?
    else {
        return Ok(None);
    };
    let parsed_receipt = parse_installed_candidate_receipt(
        field(entry, "receipt").ok_or(ApiMaintenanceCandidateError::Catalog)?,
        codec,
    )?;
    let receipt = validate_shadow_receipt(&parsed_receipt)?;
    let registration =
        field(entry, "registration").ok_or(ApiMaintenanceCandidateError::Registration)?;
    let registration_version =
        validate_installed_registration(registration, family, &parsed_receipt, codec)?;
    Ok(Some(ApiShadowCatalogEntry {
        registration,
        registration_version,
        receipt,
    }))
}

/// Validate every catalog slot and borrow the one requested exact family role.
pub(crate) fn select_installed_catalog_entry<'a>(
    catalog: &'a JsonValue,
    selected_family: &str,
    selected_slot: &str,
) -> Result<Option<&'a JsonValue>, ApiMaintenanceCandidateError> {
    // reject extension fields before interpreting the catalog version
    if !exact_keys(catalog, CATALOG_KEYS) {
        return Err(ApiMaintenanceCandidateError::Catalog);
    }
    let version = text(catalog, "contractVersion").ok_or(ApiMaintenanceCandidateError::Catalog)?;
    let maximum = match version.as_str() {
        "adjustment-installed-candidate-catalog/v1" => 3,
        "adjustment-installed-candidate-catalog/v2" => 6,
        "adjustment-installed-candidate-catalog/v3" => 7,
        _ => return Err(ApiMaintenanceCandidateError::Catalog),
    };
    let JsonValue::Array(entries) =
        field(catalog, "entries").ok_or(ApiMaintenanceCandidateError::Catalog)?
    else {
        return Err(ApiMaintenanceCandidateError::Catalog);
    };
    // retain the source catalog's bounded cardinality contract
    if entries.len() > maximum {
        return Err(ApiMaintenanceCandidateError::Catalog);
    }

    let mut identities = Vec::with_capacity(entries.len());
    let mut selected = None;
    // validate every disjoint slot before selecting the requested family
    for entry in entries {
        let expected_keys = if version == "adjustment-installed-candidate-catalog/v1" {
            ENTRY_V1_KEYS
        } else {
            ENTRY_V2_KEYS
        };
        // reject malformed unrelated entries instead of selecting around them
        if !exact_keys(entry, expected_keys) {
            return Err(ApiMaintenanceCandidateError::Catalog);
        }
        let entry_family = text(entry, "family").ok_or(ApiMaintenanceCandidateError::Catalog)?;
        let entry_slot = if version == "adjustment-installed-candidate-catalog/v1" {
            "shadow".to_owned()
        } else {
            text(entry, "slot").ok_or(ApiMaintenanceCandidateError::Catalog)?
        };
        let receipt = field(entry, "receipt").ok_or(ApiMaintenanceCandidateError::Catalog)?;
        let receipt_version =
            text(receipt, "contractVersion").ok_or(ApiMaintenanceCandidateError::Catalog)?;
        let control_entry =
            version == "adjustment-installed-candidate-catalog/v3" && entry_slot == "control";
        let candidate_receipt_version = if version == "adjustment-installed-candidate-catalog/v1" {
            "adjustment-installed-candidate-receipt/v1"
        } else {
            "adjustment-installed-candidate-receipt/v2"
        };
        let valid_slot = if control_entry {
            entry_family == "rain"
                && matches!(field(entry, "registration"), Some(JsonValue::Null))
                && receipt_version == "adjustment-installed-rain-control-reference-receipt/v1"
        } else {
            matches!(entry_family.as_str(), "temperature" | "wind" | "rain")
                && matches!(entry_slot.as_str(), "active" | "shadow")
                && receipt_version == candidate_receipt_version
        };
        // close catalog version, family, slot and receipt-version substitution
        if !valid_slot {
            return Err(ApiMaintenanceCandidateError::Catalog);
        }

        let identity = format!("{entry_family}:{entry_slot}");
        // require every family-slot identity to be unique
        if identities.iter().any(|existing| existing == &identity) {
            return Err(ApiMaintenanceCandidateError::Catalog);
        }
        identities.push(identity);
        // retain only the exact family role requested by the caller
        if entry_family == selected_family && entry_slot == selected_slot {
            selected = Some(entry);
        }
    }
    Ok(selected)
}

/// Validate one exact installed candidate receipt and retain only later-bound identities.
pub(crate) fn parse_installed_candidate_receipt(
    value: &JsonValue,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<ParsedInstalledCandidateReceipt, ApiMaintenanceCandidateError> {
    let contract_version =
        text(value, "contractVersion").ok_or(ApiMaintenanceCandidateError::Catalog)?;
    let keys = if contract_version == "adjustment-installed-candidate-receipt/v2" {
        RECEIPT_V2_KEYS
    } else {
        RECEIPT_V1_KEYS
    };
    // reject partial v2 extensions and all unknown receipt fields
    if !exact_keys(value, keys) {
        return Err(ApiMaintenanceCandidateError::Catalog);
    }

    let action_sha256 = required_text(value, "actionSha256")?;
    let registration_sha256 = required_text(value, "registrationSha256")?;
    let candidate_graph_sha256 = required_text(value, "candidateGraphSha256")?;
    let candidate_sha256 = required_text(value, "candidateSha256")?;
    let bundle_sha256 = required_text(value, "bundleSha256")?;
    let source_sha256 = required_text(value, "sourceSha256")?;
    let deployed_settings_sha256 = required_text(value, "deployedSettingsSha256")?;
    let control_plane_sha256 = required_text(value, "controlPlaneSha256")?;
    // require every immutable graph and deployment identity
    if [
        &action_sha256,
        &registration_sha256,
        &candidate_graph_sha256,
        &candidate_sha256,
        &bundle_sha256,
        &source_sha256,
        &deployed_settings_sha256,
        &control_plane_sha256,
    ]
    .into_iter()
    .any(|identity| !valid_hash(identity))
    {
        return Err(ApiMaintenanceCandidateError::Catalog);
    }

    let deployed_commit = required_text(value, "deployedCommit")?;
    let deployed_release = required_text(value, "deployedRelease")?;
    let deployed_image_digest = required_text(value, "deployedImageDigest")?;
    let fencing_token = required_text(value, "fencingToken")?;
    let installed_at = required_text(value, "installedAt")?;
    // close receipt version, deployment spellings and the installed clock
    if !matches!(
        contract_version.as_str(),
        "adjustment-installed-candidate-receipt/v1" | "adjustment-installed-candidate-receipt/v2"
    ) || !valid_commit(&deployed_commit)
        || !valid_release(&deployed_release)
        || !valid_image_digest(&deployed_image_digest)
        || !valid_fencing_token(&fencing_token)
        || !codec.is_canonical_utc_instant(&installed_at)
    {
        return Err(ApiMaintenanceCandidateError::Catalog);
    }

    let (
        action_kind,
        full_member_root_sha256,
        lifecycle_head_sha256,
        policy_decision,
        policy_report_sha256,
    ) = if contract_version == "adjustment-installed-candidate-receipt/v2" {
        let lifecycle_head_sha256 = required_text(value, "lifecycleHeadSha256")?;
        let policy_report_sha256 = required_text(value, "policyReportSha256")?;
        let full_member_root_sha256 = match field(value, "fullMemberRootSha256") {
            Some(JsonValue::Null) => None,
            Some(JsonValue::String(value)) => String::from_utf16(value)
                .map(Some)
                .map_err(|_| ApiMaintenanceCandidateError::Catalog)?,
            _ => return Err(ApiMaintenanceCandidateError::Catalog),
        };
        // close all v2 lifecycle identity types before disposition-specific admission
        if !valid_hash(&lifecycle_head_sha256)
            || !valid_hash(&policy_report_sha256)
            || full_member_root_sha256
                .as_deref()
                .is_some_and(|identity| !valid_hash(identity))
        {
            return Err(ApiMaintenanceCandidateError::Catalog);
        }
        (
            Some(required_text(value, "actionKind")?),
            full_member_root_sha256,
            Some(lifecycle_head_sha256),
            Some(required_text(value, "policyDecision")?),
            Some(policy_report_sha256),
        )
    } else {
        (None, None, None, None, None)
    };

    Ok(ParsedInstalledCandidateReceipt {
        action_sha256,
        registration_sha256,
        candidate_graph_sha256,
        candidate_sha256,
        bundle_sha256,
        source_sha256,
        deployed_settings_sha256,
        fencing_token,
        action_kind,
        full_member_root_sha256,
        lifecycle_head_sha256,
        policy_decision,
        policy_report_sha256,
    })
}

/// Restrict one parsed receipt to the existing inactive shadow authority.
fn validate_shadow_receipt(
    receipt: &ParsedInstalledCandidateReceipt,
) -> Result<ApiShadowInstalledReceipt, ApiMaintenanceCandidateError> {
    let lifecycle = match receipt.action_kind.as_deref() {
        None => None,
        Some("shadow")
            if receipt.policy_decision.as_deref() == Some("pending")
                && receipt.full_member_root_sha256.is_none() =>
        {
            Some(ApiShadowLifecycle {
                lifecycle_head_sha256: receipt
                    .lifecycle_head_sha256
                    .clone()
                    .ok_or(ApiMaintenanceCandidateError::Catalog)?,
                policy_report_sha256: receipt
                    .policy_report_sha256
                    .clone()
                    .ok_or(ApiMaintenanceCandidateError::Catalog)?,
            })
        }
        _ => return Err(ApiMaintenanceCandidateError::Catalog),
    };
    Ok(ApiShadowInstalledReceipt {
        action_sha256: receipt.action_sha256.clone(),
        registration_sha256: receipt.registration_sha256.clone(),
        candidate_graph_sha256: receipt.candidate_graph_sha256.clone(),
        candidate_sha256: receipt.candidate_sha256.clone(),
        bundle_sha256: receipt.bundle_sha256.clone(),
        source_sha256: receipt.source_sha256.clone(),
        deployed_settings_sha256: receipt.deployed_settings_sha256.clone(),
        fencing_token: receipt.fencing_token.clone(),
        lifecycle,
    })
}

/// Validate and recompute one exact database registration before cross-binding it.
pub(crate) fn validate_installed_registration(
    value: &JsonValue,
    family: AdjustmentShadowFamily,
    receipt: &ParsedInstalledCandidateReceipt,
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<ApiMaintenanceRegistrationVersion, ApiMaintenanceCandidateError> {
    let is_v3 = REGISTRATION_V3_ONLY_KEYS
        .iter()
        .any(|key| field(value, key).is_some());
    let keys = if is_v3 {
        REGISTRATION_V3_KEYS
    } else {
        REGISTRATION_V2_KEYS
    };
    // force partial v3 extensions into the closed v3 key set
    if !exact_keys(value, keys) {
        return Err(ApiMaintenanceCandidateError::Registration);
    }

    let artifact_sha256 = registration_text(value, "artifactSha256")?;
    let candidate_sha256 = registration_text(value, "candidateSha256")?;
    let cohort_sha256 = registration_text(value, "cohortSha256")?;
    let policy_sha256 = registration_text(value, "policySha256")?;
    let registration_sha256 = registration_text(value, "registrationSha256")?;
    let reserved_key_sha256 = registration_text(value, "reservedKeySha256")?;
    let source_sha256 = registration_text(value, "sourceSha256")?;
    // close every immutable registration identity before recomputation
    if [
        &artifact_sha256,
        &candidate_sha256,
        &cohort_sha256,
        &policy_sha256,
        &registration_sha256,
        &reserved_key_sha256,
        &source_sha256,
    ]
    .into_iter()
    .any(|identity| !valid_hash(identity))
    {
        return Err(ApiMaintenanceCandidateError::Registration);
    }

    let registration_family = registration_text(value, "family")?;
    let site_key = registration_text(value, "siteKey")?;
    let interval_start_at = registration_text(value, "intervalStartAt")?;
    let interval_end_at = registration_text(value, "intervalEndAt")?;
    let target_cutoff_at = registration_text(value, "targetCutoffAt")?;
    let terminal_at = registration_text(value, "terminalAt")?;
    // validate family, site and all four exact database clocks
    if registration_family != family_name(family)
        || site_key != "ballydidean"
        || !codec.is_canonical_utc_instant(&interval_start_at)
        || !codec.is_canonical_utc_instant(&interval_end_at)
        || !codec.is_canonical_utc_instant(&target_cutoff_at)
        || !codec.is_canonical_utc_instant(&terminal_at)
    {
        return Err(ApiMaintenanceCandidateError::Registration);
    }

    let (version, preimage) = if is_v3 {
        let epoch_witness_sha256 = registration_text(value, "epochWitnessSha256")?;
        let schedule_contract_sha256 = registration_text(value, "scheduleContractSha256")?;
        let predecessor = nullable_registration_hash(value, "predecessorRegistrationSha256")?;
        // close every future-only schedule lineage identity
        if !valid_hash(&epoch_witness_sha256)
            || !valid_hash(&schedule_contract_sha256)
            || predecessor
                .as_deref()
                .is_some_and(|value| !valid_hash(value))
        {
            return Err(ApiMaintenanceCandidateError::Registration);
        }
        let predecessor = predecessor.as_deref().unwrap_or("none");
        (
            ApiMaintenanceRegistrationVersion::V3,
            format!(
                "adjustment-shadow-registration/v3\n{site_key}\n{registration_family}\n{candidate_sha256}\n{artifact_sha256}\n{policy_sha256}\n{cohort_sha256}\n{reserved_key_sha256}\n{source_sha256}\n{epoch_witness_sha256}\n{schedule_contract_sha256}\n{predecessor}\n{interval_start_at}\n{interval_end_at}\n{target_cutoff_at}\n{terminal_at}\n"
            ),
        )
    } else {
        (
            ApiMaintenanceRegistrationVersion::V2,
            format!(
                "adjustment-shadow-registration/v2\n{site_key}\n{registration_family}\n{candidate_sha256}\n{artifact_sha256}\n{policy_sha256}\n{cohort_sha256}\n{reserved_key_sha256}\n{source_sha256}\n{interval_start_at}\n{interval_end_at}\n{target_cutoff_at}\n{terminal_at}"
            ),
        )
    };
    let recomputed = codec
        .sha256(preimage.as_bytes())
        .ok_or(ApiMaintenanceCandidateError::Registration)?;
    // bind the declared registration identity to the exact source preimage
    if recomputed.as_str() != registration_sha256 {
        return Err(ApiMaintenanceCandidateError::Registration);
    }
    // reject candidate, source, artifact or family substitution across the receipt
    if registration_sha256 != receipt.registration_sha256
        || candidate_sha256 != receipt.candidate_sha256
        || artifact_sha256 != receipt.bundle_sha256
        || source_sha256 != receipt.source_sha256
    {
        return Err(ApiMaintenanceCandidateError::Registration);
    }
    Ok(version)
}

/// Decode one required receipt string while retaining catalog error ownership.
fn required_text(value: &JsonValue, key: &str) -> Result<String, ApiMaintenanceCandidateError> {
    text(value, key).ok_or(ApiMaintenanceCandidateError::Catalog)
}

/// Decode one required registration string while retaining registration error ownership.
fn registration_text(value: &JsonValue, key: &str) -> Result<String, ApiMaintenanceCandidateError> {
    text(value, key).ok_or(ApiMaintenanceCandidateError::Registration)
}

/// Decode one nullable registration hash without accepting another JSON type.
fn nullable_registration_hash(
    value: &JsonValue,
    key: &str,
) -> Result<Option<String>, ApiMaintenanceCandidateError> {
    match field(value, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(value)) => String::from_utf16(value)
            .map(Some)
            .map_err(|_| ApiMaintenanceCandidateError::Registration),
        _ => Err(ApiMaintenanceCandidateError::Registration),
    }
}

/// Recognize the deployed 40-character lowercase commit identity.
fn valid_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(is_lower_hex)
}

/// Recognize the deployed image digest prefix and lowercase SHA-256 identity.
fn valid_image_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(valid_hash)
}

/// Recognize the source release spelling without interpreting its calendar date.
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 12
        && bytes.get(4) == Some(&b'.')
        && bytes.get(7) == Some(&b'.')
        && bytes.get(10) == Some(&b'-')
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && (1..=3).contains(&bytes[11..].len())
        && bytes[11] != b'0'
        && bytes[11..].iter().all(u8::is_ascii_digit)
}

/// Recognize one positive bounded decimal fencing token.
fn valid_fencing_token(value: &str) -> bool {
    (1..=20).contains(&value.len())
        && value.as_bytes()[0] != b'0'
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Recognize one lowercase hexadecimal byte.
const fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
}
