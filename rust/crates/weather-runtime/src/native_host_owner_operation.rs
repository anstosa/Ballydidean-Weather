//! Root-authorized native owner-operation SQL and exact result projection.

use std::fmt::{self, Display, Formatter};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::capture_epoch_witness::verify_operator_capture_epoch_witness;
use crate::family_target_catalog::catalog_entries;
use crate::family_target_catalog_validation::validate_family_target_catalog;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_cold_remote_documents::validate_custody_acknowledgement_v2;
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    MaintenanceCanonicalJson, NativeFamilyReleaseCurrent, NativeMaintenanceConfirmationAccessInput,
    NativeMaintenanceConfirmationCustody, NativeRegistrationLifecycleMetadata,
    NativeWirePrimitives, build_native_maintenance_confirmation_access_request,
};

const MAXIMUM_DOCUMENT_BYTES: usize = 128 * 1_024;
const ACCESS_REQUEST_VERSION: &str = "adjustment-confirmation-access-burn-request/v3";
const ACCESS_RESULT_VERSION: &str = "adjustment-confirmation-access-burn-result/v3";
const TERMINAL_RESULT_VERSION: &str = "adjustment-shadow-terminal-record-result/v3";
const RETIREMENT_RESULT_VERSION: &str = "adjustment-shadow-terminal-retirement-result/v3";
const TERMINAL_RECORD_VERSION: &str = "adjustment-shadow-terminal-record/v3";
const ACCESS_REQUEST_FIELDS: &[&str] = &[
    "acknowledgement",
    "archive",
    "confirmationRegistration",
    "contractVersion",
    "epochWitnessSha256",
    "journalHeadSha256",
    "localBurn",
    "metadata",
    "shadowRegistration",
    "sourceCommit",
];
const NATIVE_ACCESS_FIELDS: &[&str] = &[
    "accessSha256",
    "accessedAt",
    "eligiblePredictionSetSha256",
    "expectedKeySetSha256",
    "gateManifestSha256",
    "journalHeadSha256",
    "maintenanceAnchorSha256",
    "metadataRootSha256",
    "registrationSha256",
    "revisionCatalogWatermarkSha256",
    "targetComparatorSnapshotRootSha256",
    "targetCutoffAt",
];
const TERMINAL_RECORD_FIELDS: &[&str] = &[
    "accessSha256",
    "actionCompletedAt",
    "actionDisposition",
    "actionSha256",
    "candidateSha256",
    "contractVersion",
    "family",
    "finalizedMetadataRootSha256",
    "finalizedPredictionCount",
    "maintenanceAnchorSha256",
    "metadataGeneration",
    "recordedAt",
    "reconciliationSha256",
    "registrationSha256",
    "reservedKeySha256",
    "sourceSha256",
    "terminalMemberSha256",
    "terminalResultSha256",
];

/// The three original result projection grammars exposed by owner-operation-result-v3.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHostOwnerOperationKind {
    Access,
    Terminal,
    Retire,
}

/// Bounded refusal categories without reflecting retained host material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHostOwnerOperationError {
    Size,
    Canonical,
    Request,
    Authority,
    Result,
    Sql,
}

impl Display for NativeHostOwnerOperationError {
    /// Render one stable owner-operation failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Size => "native owner operation document is too large",
            Self::Canonical => "native owner operation document is invalid",
            Self::Request => "native owner operation request differs",
            Self::Authority => "native owner operation authority differs",
            Self::Result => "native owner operation result differs",
            Self::Sql => "native owner operation SQL projection failed",
        })
    }
}

impl std::error::Error for NativeHostOwnerOperationError {}

/// Independently read root authorities needed by the pre-value access burn.
pub struct NativeHostOwnerAccessAuthority<'a> {
    pub capture_epoch_witness_bytes: &'a [u8],
    pub current: &'a NativeFamilyReleaseCurrent,
    pub installed_catalog: &'a JsonValue,
    pub actual_acknowledgement: &'a JsonValue,
    pub now_milliseconds: i64,
    pub time: &'a dyn CurrentTimeResolver,
}

/// Exact canonical request identity and fixed SQL emitted after host authority checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeHostOwnerAccessSql {
    pub request_sha256: String,
    pub sql: String,
}

/// Admitted original owner result and its canonical LF-terminated projection.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeHostOwnerOperationResult {
    pub value: JsonValue,
    pub canonical_bytes: Vec<u8>,
}

/// Authenticate one access request against independent host state and emit the fixed transaction.
pub fn build_native_host_owner_access_sql(
    request_bytes: &[u8],
    expected_sha256: &str,
    authority: NativeHostOwnerAccessAuthority<'_>,
) -> Result<NativeHostOwnerAccessSql, NativeHostOwnerOperationError> {
    use NativeHostOwnerOperationError as Error;

    // bound and canonically admit every caller byte before reading nested fields
    if !(2..=MAXIMUM_DOCUMENT_BYTES).contains(&request_bytes.len()) {
        return Err(Error::Size);
    }
    let mut wire = NativeWirePrimitives;
    let request = wire
        .parse_canonical_object(request_bytes)
        .ok_or(Error::Canonical)?;
    let request_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, request_bytes));
    // bind the fixed forced-command operand to every canonical request byte
    if !valid_hash(expected_sha256)
        || request_sha256 != expected_sha256
        || !exact_keys(&request, ACCESS_REQUEST_FIELDS)
        || text(&request, "contractVersion").as_deref() != Some(ACCESS_REQUEST_VERSION)
    {
        return Err(Error::Request);
    }

    let metadata_value = field(&request, "metadata").ok_or(Error::Request)?;
    let metadata = NativeRegistrationLifecycleMetadata {
        finalized_prediction_count: u32_integer(metadata_value, "finalizedPredictionCount")
            .ok_or(Error::Request)?,
        generation: u32_integer(metadata_value, "generation").ok_or(Error::Request)?,
        last_anchor_sha256: nullable_text(metadata_value, "lastAnchorSha256")
            .ok_or(Error::Request)?,
        root_sha256: text(metadata_value, "rootSha256").ok_or(Error::Request)?,
        through_at: nullable_text(metadata_value, "throughAt").ok_or(Error::Request)?,
    };
    let acknowledgement = field(&request, "acknowledgement")
        .ok_or(Error::Request)?
        .clone();
    let custody = NativeMaintenanceConfirmationCustody {
        acknowledgement,
        journal_head_sha256: text(&request, "journalHeadSha256").ok_or(Error::Request)?,
    };
    let archive = field(&request, "archive").ok_or(Error::Request)?;
    let confirmation_registration =
        field(&request, "confirmationRegistration").ok_or(Error::Request)?;
    let local_burn = field(&request, "localBurn").ok_or(Error::Request)?;
    let shadow_registration = field(&request, "shadowRegistration").ok_or(Error::Request)?;
    let source_commit = text(&request, "sourceCommit").ok_or(Error::Request)?;
    // reuse the complete native workstation request validator without granting it host authority
    build_native_maintenance_confirmation_access_request(
        NativeMaintenanceConfirmationAccessInput {
            custody: &custody,
            eligible_prediction_set_sha256: &text(archive, "eligiblePredictionSetSha256")
                .ok_or(Error::Request)?,
            full_graph_verified_at: &text(archive, "fullGraphVerifiedAt").ok_or(Error::Request)?,
            graph_manifest_sha256: &text(archive, "graphManifestSha256").ok_or(Error::Request)?,
            confirmation_registration,
            epoch_witness_bytes: authority.capture_epoch_witness_bytes,
            local_burn,
            metadata: &metadata,
            shadow_registration,
            source_commit: &source_commit,
        },
    )
    .ok_or(Error::Request)?;

    let admitted_witness = verify_operator_capture_epoch_witness(
        authority.capture_epoch_witness_bytes,
        &mut wire,
        authority.time,
    )
    .map_err(|_| Error::Authority)?;
    let witness_document_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &wire,
        authority.capture_epoch_witness_bytes,
    ));
    validate_family_target_catalog(authority.installed_catalog, &mut wire, authority.time)
        .map_err(|_| Error::Authority)?;
    let catalog_bytes = wire
        .encode_canonical_json(authority.installed_catalog)
        .ok_or(Error::Authority)?;
    let catalog_sha256 =
        format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, &catalog_bytes));
    validate_custody_acknowledgement_v2(
        authority.actual_acknowledgement,
        &mut wire,
        authority.time,
    )
    .map_err(|_| Error::Authority)?;
    let family = text(shadow_registration, "family").ok_or(Error::Request)?;
    let catalog_registration = catalog_entries(authority.installed_catalog)
        .map_err(|_| Error::Authority)?
        .iter()
        .find(|entry| {
            // select only the explicit source shadow slot for this exact family
            text(entry, "family").as_deref() == Some(family.as_str())
                && text(entry, "slot").as_deref() == Some("shadow")
        })
        .and_then(|entry| field(entry, "registration"))
        .ok_or(Error::Authority)?;
    let graph_verified_at = instant(archive, "fullGraphVerifiedAt").ok_or(Error::Request)?;
    let accessed_at = instant(local_burn, "accessedAt").ok_or(Error::Request)?;
    let actual_acknowledgement_bytes = wire
        .encode_canonical_json(authority.actual_acknowledgement)
        .ok_or(Error::Authority)?;
    let request_acknowledgement_bytes = wire
        .encode_canonical_json(&custody.acknowledgement)
        .ok_or(Error::Request)?;
    let catalog_registration_bytes = wire
        .encode_canonical_json(catalog_registration)
        .ok_or(Error::Authority)?;
    let request_registration_bytes = wire
        .encode_canonical_json(shadow_registration)
        .ok_or(Error::Request)?;
    // require current release, catalog, epoch, custody, and wall clock independently of the request
    if authority.current.family != family
        || authority.current.commit != source_commit
        || authority.current.catalog_sha256 != catalog_sha256
        || text(&request, "epochWitnessSha256").as_deref() != Some(witness_document_sha256.as_str())
        || text(shadow_registration, "epochWitnessSha256").as_deref()
            != Some(admitted_witness.witness.witnessSha256.as_str())
        || actual_acknowledgement_bytes != request_acknowledgement_bytes
        || catalog_registration_bytes != request_registration_bytes
        || graph_verified_at > authority.now_milliseconds
        || accessed_at > authority.now_milliseconds
    {
        return Err(Error::Authority);
    }

    Ok(NativeHostOwnerAccessSql {
        request_sha256,
        sql: access_burn_sql(&request, expected_sha256).ok_or(Error::Sql)?,
    })
}

/// Project one genuine database owner result into the predecessor's canonical response bytes.
pub fn project_native_host_owner_operation_result(
    kind: NativeHostOwnerOperationKind,
    bytes: &[u8],
) -> Result<NativeHostOwnerOperationResult, NativeHostOwnerOperationError> {
    use NativeHostOwnerOperationError as Error;

    // preserve the predecessor's bounded JSON input before decoding database output
    if bytes.is_empty() || bytes.len() > MAXIMUM_DOCUMENT_BYTES {
        return Err(Error::Size);
    }
    let wire = NativeWirePrimitives;
    let value = wire.decode_json(bytes).ok_or(Error::Canonical)?;
    // dispatch only the three closed original projection grammars
    match kind {
        NativeHostOwnerOperationKind::Access => validate_access_result(&value),
        NativeHostOwnerOperationKind::Terminal => validate_terminal_result(&value),
        NativeHostOwnerOperationKind::Retire => validate_retirement_result(&value),
    }
    .ok_or(Error::Result)?;
    let canonical_bytes = wire.encode_sorted_json(&value).ok_or(Error::Canonical)?;
    Ok(NativeHostOwnerOperationResult {
        value,
        canonical_bytes,
    })
}

/// Validate one access result including its server-derived identity preimage.
fn validate_access_result(value: &JsonValue) -> Option<()> {
    if !exact_keys(
        value,
        &[
            "contractVersion",
            "localAccessSha256",
            "nativeAccess",
            "nativeAccessSha256",
            "state",
        ],
    ) || text(value, "contractVersion")? != ACCESS_RESULT_VERSION
        || text(value, "state")? != "burned"
        || !valid_hash(&text(value, "localAccessSha256")?)
    {
        return None;
    }
    let native = field(value, "nativeAccess")?;
    validate_native_access(native)?;
    let native_access_sha256 = text(native, "accessSha256")?;
    (text(value, "nativeAccessSha256")? == native_access_sha256).then_some(())
}

/// Recompute one native access row exactly like validateOwnerNativeAccess.
fn validate_native_access(value: &JsonValue) -> Option<()> {
    if !exact_keys(value, NATIVE_ACCESS_FIELDS) {
        return None;
    }
    // validate every fixed identity before constructing its LF-free preimage
    for key in NATIVE_ACCESS_FIELDS
        .iter()
        .copied()
        .filter(|key| key.ends_with("Sha256"))
    {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    instant(value, "accessedAt")?;
    instant(value, "targetCutoffAt")?;
    let preimage = [
        "adjustment-confirmation-access/v2".to_owned(),
        text(value, "registrationSha256")?,
        text(value, "journalHeadSha256")?,
        text(value, "maintenanceAnchorSha256")?,
        text(value, "gateManifestSha256")?,
        text(value, "eligiblePredictionSetSha256")?,
        text(value, "expectedKeySetSha256")?,
        text(value, "metadataRootSha256")?,
        text(value, "targetComparatorSnapshotRootSha256")?,
        text(value, "revisionCatalogWatermarkSha256")?,
        text(value, "targetCutoffAt")?,
    ]
    .join("\n");
    let expected = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        preimage.as_bytes(),
    ));
    (text(value, "accessSha256")? == expected).then_some(())
}

/// Validate the complete terminal result and its nested immutable row.
fn validate_terminal_result(value: &JsonValue) -> Option<()> {
    if !exact_keys(
        value,
        &[
            "contractVersion",
            "reconciliationSha256",
            "registrationSha256",
            "state",
            "terminalRecord",
        ],
    ) || text(value, "contractVersion")? != TERMINAL_RESULT_VERSION
        || text(value, "state")? != "recorded"
    {
        return None;
    }
    let reconciliation = text(value, "reconciliationSha256")?;
    let registration = text(value, "registrationSha256")?;
    if !valid_hash(&reconciliation) || !valid_hash(&registration) {
        return None;
    }
    let record = field(value, "terminalRecord")?;
    validate_terminal_record(record)?;
    (text(record, "reconciliationSha256")? == reconciliation
        && text(record, "registrationSha256")? == registration)
        .then_some(())
}

/// Validate one owner-projected terminal row without normalizing any field.
fn validate_terminal_record(value: &JsonValue) -> Option<()> {
    if !exact_keys(value, TERMINAL_RECORD_FIELDS)
        || text(value, "contractVersion")? != TERMINAL_RECORD_VERSION
        || !matches!(
            text(value, "family")?.as_str(),
            "temperature" | "wind" | "rain"
        )
        || !matches!(
            text(value, "actionDisposition")?.as_str(),
            "operator_off_compensated"
                | "promoted_operator_off_unapplied"
                | "promoted_verified"
                | "promotion_failed_compensated"
                | "rejected_no_action"
                | "resource_refused_no_action"
                | "support_failed_no_action"
        )
        || positive_safe_integer(value, "metadataGeneration").is_none()
        || positive_safe_integer(value, "finalizedPredictionCount").is_none()
    {
        return None;
    }
    // require all terminal identities and source clocks before returning the row
    for key in TERMINAL_RECORD_FIELDS
        .iter()
        .copied()
        .filter(|key| key.ends_with("Sha256"))
    {
        valid_hash(&text(value, key)?).then_some(())?;
    }
    instant(value, "actionCompletedAt")?;
    instant(value, "recordedAt")?;
    Some(())
}

/// Validate one exact terminal retirement acknowledgement.
fn validate_retirement_result(value: &JsonValue) -> Option<()> {
    if !exact_keys(
        value,
        &[
            "contractVersion",
            "reconciliationSha256",
            "registrationSha256",
            "state",
        ],
    ) || text(value, "contractVersion")? != RETIREMENT_RESULT_VERSION
        || text(value, "state")? != "retired"
    {
        return None;
    }
    (valid_hash(&text(value, "reconciliationSha256")?)
        && valid_hash(&text(value, "registrationSha256")?))
    .then_some(())
}

/// Render the exact source access-burn SQL after all request and host admission.
fn access_burn_sql(request: &JsonValue, request_sha256: &str) -> Option<String> {
    let archive = field(request, "archive")?;
    let burn = field(request, "localBurn")?;
    let metadata = field(request, "metadata")?;
    let shadow = field(request, "shadowRegistration")?;
    let shadow_literal = canonical_literal(shadow)?;
    let shadow_sql = concat!(
        "'artifactSha256', window_row.artifact_sha256, ",
        "'candidateSha256', window_row.candidate_sha256, ",
        "'cohortSha256', window_row.cohort_sha256, ",
        "'epochWitnessSha256', window_row.epoch_witness_sha256, ",
        "'family', window_row.family, ",
        "'intervalEndAt', to_char(window_row.interval_end_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), ",
        "'intervalStartAt', to_char(window_row.interval_start_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), ",
        "'policySha256', window_row.policy_sha256, ",
        "'predecessorRegistrationSha256', window_row.predecessor_registration_sha256, ",
        "'registrationSha256', window_row.registration_sha256, ",
        "'reservedKeySha256', window_row.reserved_key_sha256, ",
        "'scheduleContractSha256', window_row.schedule_contract_sha256, ",
        "'siteKey', window_row.site_key, ",
        "'sourceSha256', window_row.source_sha256, ",
        "'targetCutoffAt', to_char(window_row.target_cutoff_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), ",
        "'terminalAt', to_char(window_row.terminal_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')"
    );
    let native_sql = concat!(
        "'accessSha256', access.access_sha256, ",
        "'accessedAt', to_char(access.accessed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), ",
        "'eligiblePredictionSetSha256', access.eligible_prediction_set_sha256, ",
        "'expectedKeySetSha256', access.expected_key_set_sha256, ",
        "'gateManifestSha256', access.gate_manifest_sha256, ",
        "'journalHeadSha256', access.journal_head_sha256, ",
        "'maintenanceAnchorSha256', access.maintenance_anchor_sha256, ",
        "'metadataRootSha256', access.metadata_root_sha256, ",
        "'registrationSha256', access.registration_sha256, ",
        "'revisionCatalogWatermarkSha256', access.revision_catalog_watermark_sha256, ",
        "'targetComparatorSnapshotRootSha256', access.target_comparator_snapshot_root_sha256, ",
        "'targetCutoffAt', to_char(access.target_cutoff_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')"
    );
    let registration_sha256 = text(shadow, "registrationSha256")?;
    let eligible_prediction_set_sha256 = text(archive, "eligiblePredictionSetSha256")?;
    let journal_head_sha256 = text(request, "journalHeadSha256")?;
    let gate_manifest_sha256 = text(burn, "gateManifestSha256")?;
    let expected_key_set_sha256 = text(burn, "expectedKeySetSha256")?;
    let target_comparator_snapshot_root_sha256 = text(burn, "targetComparatorSnapshotRootSha256")?;
    let revision_catalog_watermark_sha256 = text(burn, "revisionCatalogWatermarkSha256")?;
    let target_cutoff_at = text(burn, "targetCutoffAt")?;
    let local_access_sha256 = text(burn, "accessSha256")?;
    let metadata_generation = u32_integer(metadata, "generation")?;
    let finalized_prediction_count = u32_integer(metadata, "finalizedPredictionCount")?;
    let metadata_root_sha256 = text(metadata, "rootSha256")?;
    let last_anchor_sha256 = text(metadata, "lastAnchorSha256")?;
    let through_at = text(metadata, "throughAt")?;
    Some(format!(
        "BEGIN;\n\
SET LOCAL statement_timeout = '30s';\n\
SET LOCAL lock_timeout = '5s';\n\
SET LOCAL weather.adjustment_maintenance_anchor_kind = 'confirmation_accessed';\n\
SET LOCAL weather.adjustment_maintenance_anchor_sha256 = '{request_sha256}';\n\
SET LOCAL weather.adjustment_eligible_prediction_set_sha256 = '{eligible_prediction_set_sha256}';\n\
DO $burn$\n\
DECLARE registration adjustment_shadow_registrations_v2%ROWTYPE; argument jsonb; metadata_root text; access_identity text;\n\
BEGIN\n\
  PERFORM pg_advisory_xact_lock(hashtextextended('{registration_sha256}', 0));\n\
  SELECT * INTO STRICT registration FROM adjustment_shadow_registrations_v2\n\
    WHERE registration_sha256 = '{registration_sha256}' FOR UPDATE;\n\
  -- bind all frozen registration fields and the exact cold accumulator before access\n\
  IF NOT EXISTS (SELECT 1 FROM adjustment_shadow_registration_windows_v3 window_row\n\
    WHERE window_row.registration_sha256 = registration.registration_sha256\n\
      AND jsonb_build_object({shadow_sql}) = $shadow${shadow_literal}$shadow$::jsonb)\n\
    OR registration.metadata_generation <> {metadata_generation}\n\
    OR registration.finalized_prediction_count <> {finalized_prediction_count}\n\
    OR registration.finalized_metadata_root_sha256 <> '{metadata_root_sha256}'\n\
    OR registration.last_finalization_anchor_sha256 <> '{last_anchor_sha256}'\n\
    OR registration.finalized_through_at <> '{through_at}'::timestamptz\n\
    OR EXISTS (SELECT 1 FROM adjustment_shadow_predictions_v2 prediction\n\
      WHERE prediction.registration_sha256 = registration.registration_sha256) THEN\n\
    RAISE EXCEPTION 'confirmation burn actual metadata differs';\n\
  END IF;\n\
  metadata_root := encode(sha256(convert_to(registration.finalized_metadata_root_sha256 || E'\\n'\n\
    || encode(sha256(convert_to('[]', 'UTF8')), 'hex') || E'\\n'\n\
    || registration.finalized_prediction_count::text || E'\\n0\\n' || registration.metadata_generation::text, 'UTF8')), 'hex');\n\
  argument := jsonb_build_object('registrationSha256', registration.registration_sha256,\n\
    'journalHeadSha256', '{journal_head_sha256}', 'maintenanceAnchorSha256', '{request_sha256}',\n\
    'gateManifestSha256', '{gate_manifest_sha256}', 'eligiblePredictionSetSha256', '{eligible_prediction_set_sha256}',\n\
    'expectedKeySetSha256', '{expected_key_set_sha256}', 'metadataRootSha256', metadata_root,\n\
    'targetComparatorSnapshotRootSha256', '{target_comparator_snapshot_root_sha256}',\n\
    'revisionCatalogWatermarkSha256', '{revision_catalog_watermark_sha256}', 'targetCutoffAt', '{target_cutoff_at}');\n\
  access_identity := encode(sha256(convert_to('adjustment-confirmation-access/v2' || E'\\n'\n\
    || registration.registration_sha256 || E'\\n{journal_head_sha256}\\n{request_sha256}\\n{gate_manifest_sha256}\\n{eligible_prediction_set_sha256}\\n{expected_key_set_sha256}\\n'\n\
    || metadata_root || E'\\n{target_comparator_snapshot_root_sha256}\\n{revision_catalog_watermark_sha256}\\n{target_cutoff_at}', 'UTF8')), 'hex');\n\
  argument := argument || jsonb_build_object('accessSha256', access_identity);\n\
  PERFORM weather_record_adjustment_confirmation_access_v2(argument);\n\
END;\n\
$burn$;\n\
SELECT json_build_object('contractVersion', 'adjustment-confirmation-access-burn-result/v3',\n\
  'localAccessSha256', '{local_access_sha256}', 'nativeAccess', jsonb_build_object({native_sql}),\n\
  'nativeAccessSha256', access.access_sha256, 'state', 'burned')\n\
FROM adjustment_confirmation_accesses_v2 access WHERE access.registration_sha256 = '{registration_sha256}';\n\
COMMIT;\n"
    ))
}

/// Encode one canonical JSON value without the source document's terminating LF.
fn canonical_literal(value: &JsonValue) -> Option<String> {
    let mut bytes = NativeWirePrimitives.encode_sorted_json(value)?;
    // remove only the canonical codec's mandatory final line feed
    if bytes.pop() != Some(b'\n') {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Parse one canonical instant through the approved native date owner.
fn instant(value: &JsonValue, key: &str) -> Option<i64> {
    NativeWirePrimitives.canonical_utc_milliseconds(&text(value, key)?)
}

/// Admit one exact nonnegative u32 JSON integer.
fn u32_integer(value: &JsonValue, key: &str) -> Option<u32> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    if !number.is_finite()
        || number.fract() != 0.0
        || *number < 0.0
        || *number > f64::from(u32::MAX)
    {
        return None;
    }
    Some(*number as u32)
}

/// Admit one positive JavaScript-safe JSON integer.
fn positive_safe_integer(value: &JsonValue, key: &str) -> Option<u64> {
    const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    if !number.is_finite()
        || number.fract() != 0.0
        || *number < 1.0
        || *number > MAXIMUM_SAFE_INTEGER
    {
        return None;
    }
    Some(*number as u64)
}

/// Preserve a required string or explicit null without a fallback.
fn nullable_text(value: &JsonValue, key: &str) -> Option<Option<String>> {
    match field(value, key)? {
        JsonValue::Null => Some(None),
        JsonValue::String(_) => text(value, key).map(Some),
        _ => None,
    }
}

#[cfg(test)]
#[path = "native_host_owner_operation_test.rs"]
mod tests;
