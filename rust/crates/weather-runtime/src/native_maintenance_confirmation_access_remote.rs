//! Exact native owner confirmation access request and response across the existing fixed transport.

use std::io;

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceConfirmationCustody,
    NativeMaintenanceDatabaseLedgerReader, NativeRegistrationLifecycleMetadata,
    NativeWirePrimitives, adjustment_rolling_local_date_at,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_confirmation_access_journal::lifecycle_context,
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
    validate_native_maintenance_archive_registration,
};

const REQUEST_VERSION: &str = "adjustment-confirmation-access-burn-request/v3";
const RESULT_VERSION: &str = "adjustment-confirmation-access-burn-result/v3";
const COMMAND: &str = "adjustment-confirmation-access-burn-v3";
const REGISTRATION_HASH_FIELDS: &[&str] = &[
    "candidateSha256",
    "cohortLineageSha256",
    "gateManifestSha256",
    "inputHeadSha256",
    "registrationSha256",
    "reservedKeySha256",
    "sourceLineageSha256",
];
const CONTEXT_FIELDS: &[&str] = &[
    "accessState",
    "actionIdentitySha256",
    "actionState",
    "candidateKind",
    "candidateReportSha256",
    "candidateSha256",
    "cohortLineageSha256",
    "contractVersion",
    "family",
    "firstTargetAt",
    "gateManifestSha256",
    "inputHeadSha256",
    "intervalEndExclusiveLocalDate",
    "intervalStartLocalDate",
    "registrationSha256",
    "reservedKeySha256",
    "sourceLineageSha256",
    "terminalAccessAt",
];
const NATIVE_FIELDS: &[&str] = &[
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

/// Exact authenticated inputs to the existing owner-only confirmation burn operation.
pub struct NativeMaintenanceConfirmationAccessInput<'a> {
    pub custody: &'a NativeMaintenanceConfirmationCustody,
    pub eligible_prediction_set_sha256: &'a str,
    pub full_graph_verified_at: &'a str,
    pub graph_manifest_sha256: &'a str,
    pub confirmation_registration: &'a JsonValue,
    pub epoch_witness_bytes: &'a [u8],
    pub local_burn: &'a JsonValue,
    pub metadata: &'a NativeRegistrationLifecycleMetadata,
    pub shadow_registration: &'a JsonValue,
    pub source_commit: &'a str,
}

/// Canonical source burn request and its content-addressed operand.
pub struct NativeMaintenanceConfirmationAccessRequest {
    value: JsonValue,
    bytes: Vec<u8>,
    request_sha256: String,
}

/// Independently verified native access and the exact local burn identity.
pub struct NativeMaintenanceConfirmationAccessResult {
    pub local_access_sha256: String,
    pub native_access: JsonValue,
    pub native_access_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceConfirmationAccessRemoteError {
    Request,
    Transport,
    Response,
}

/// Fixed typed operation port; never accepts a caller-selected transport command.
pub trait NativeMaintenanceConfirmationAccessPort {
    /// Submit one canonical source request under its exact immutable identity.
    fn burn_confirmation_access(
        &mut self,
        request_sha256: &str,
        bytes: &[u8],
    ) -> io::Result<Vec<u8>>;
}
impl NativeMaintenanceConfirmationAccessPort for NativeMaintenanceDatabaseLedgerReader {
    /// Use the existing verified fixed owner transport with the original operation and operand.
    fn burn_confirmation_access(
        &mut self,
        request_sha256: &str,
        bytes: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.request_document(COMMAND, &[request_sha256], Some(bytes))
    }
}

/// Build and cross-bind the exact local/remote registration domains and custody proof.
pub fn build_native_maintenance_confirmation_access_request(
    input: NativeMaintenanceConfirmationAccessInput<'_>,
) -> Option<NativeMaintenanceConfirmationAccessRequest> {
    let wire = NativeWirePrimitives;
    validate_registration(input.confirmation_registration)?;
    validate_local_burn(input.local_burn, input.confirmation_registration)?;
    let shadow_bytes = wire.encode_sorted_json(input.shadow_registration)?;
    validate_native_maintenance_archive_registration(
        "adjustment-shadow-registration/v3",
        &shadow_bytes,
    )?;
    crate::native_maintenance_cold_remote_documents::validate_custody_acknowledgement_v2(
        &input.custody.acknowledgement,
        &mut NativeWirePrimitives,
        &wire,
    )
    .ok()?;
    let metadata = input.metadata;
    let last_anchor = metadata.last_anchor_sha256.as_deref()?;
    let through_at = metadata.through_at.as_deref()?;
    wire.canonical_utc_milliseconds(through_at)?;
    // require the original fully finalized database metadata and immutable source identities
    if metadata.generation == 0
        || metadata.finalized_prediction_count == 0
        || ![
            last_anchor,
            &metadata.root_sha256,
            input.eligible_prediction_set_sha256,
            input.graph_manifest_sha256,
            &input.custody.journal_head_sha256,
        ]
        .into_iter()
        .all(valid_hash)
        || input.source_commit.len() != 40
        || !input
            .source_commit
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return None;
    }
    let verified_at = wire.canonical_utc_milliseconds(input.full_graph_verified_at)?;
    let acknowledged_at =
        wire.canonical_utc_milliseconds(&text(&input.custody.acknowledgement, "acknowledgedAt")?)?;
    if verified_at < acknowledged_at {
        return None;
    }
    let registration = input.confirmation_registration;
    let shadow = input.shadow_registration;
    for (local, remote) in [
        ("family", "family"),
        ("candidateSha256", "candidateSha256"),
        ("reservedKeySha256", "reservedKeySha256"),
        ("firstTargetAt", "intervalStartAt"),
        ("terminalAccessAt", "terminalAt"),
    ] {
        if text(registration, local)? != text(shadow, remote)? {
            return None;
        }
    }
    if text(input.local_burn, "expectedKeySetSha256")? != text(shadow, "reservedKeySha256")?
        || text(input.local_burn, "targetCutoffAt")? != text(shadow, "targetCutoffAt")?
        || text(shadow, "targetCutoffAt")? != text(shadow, "terminalAt")?
        || text(input.local_burn, "gateManifestSha256")?
            != text(registration, "gateManifestSha256")?
        || text(registration, "intervalStartLocalDate")?
            != adjustment_rolling_local_date_at(&text(shadow, "intervalStartAt")?).ok()?
        || text(registration, "intervalEndExclusiveLocalDate")?
            != adjustment_rolling_local_date_at(&text(shadow, "intervalEndAt")?).ok()?
    {
        return None;
    }
    // retain the exact full canonical witness member hash rather than the witness's internal identity
    let _ = crate::capture_epoch_witness::verify_operator_capture_epoch_witness(
        input.epoch_witness_bytes,
        &mut NativeWirePrimitives,
        &wire,
    )
    .ok()?;
    let epoch_hash = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &wire,
        input.epoch_witness_bytes,
    ));
    let value = object(vec![
        ("acknowledgement", input.custody.acknowledgement.clone()),
        (
            "archive",
            object(vec![
                (
                    "eligiblePredictionSetSha256",
                    string(input.eligible_prediction_set_sha256),
                ),
                ("fullGraphVerifiedAt", string(input.full_graph_verified_at)),
                ("graphManifestSha256", string(input.graph_manifest_sha256)),
            ]),
        ),
        ("confirmationRegistration", registration.clone()),
        ("contractVersion", string(REQUEST_VERSION)),
        ("epochWitnessSha256", string(&epoch_hash)),
        (
            "journalHeadSha256",
            string(&input.custody.journal_head_sha256),
        ),
        ("localBurn", input.local_burn.clone()),
        (
            "metadata",
            object(vec![
                (
                    "finalizedPredictionCount",
                    JsonValue::Number(metadata.finalized_prediction_count as f64),
                ),
                ("generation", JsonValue::Number(metadata.generation as f64)),
                ("lastAnchorSha256", string(last_anchor)),
                ("rootSha256", string(&metadata.root_sha256)),
                ("throughAt", string(through_at)),
            ]),
        ),
        ("shadowRegistration", shadow.clone()),
        ("sourceCommit", string(input.source_commit)),
    ]);
    let bytes = wire.encode_sorted_json(&value)?;
    let request_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, &bytes));
    Some(NativeMaintenanceConfirmationAccessRequest {
        value,
        bytes,
        request_sha256,
    })
}

/// Submit exactly one local burn request and admit the complete genuine owner response.
pub fn burn_native_maintenance_confirmation_access(
    request: &NativeMaintenanceConfirmationAccessRequest,
    remote: &mut impl NativeMaintenanceConfirmationAccessPort,
) -> Result<NativeMaintenanceConfirmationAccessResult, NativeMaintenanceConfirmationAccessRemoteError>
{
    use NativeMaintenanceConfirmationAccessRemoteError as Error;
    let wire = NativeWirePrimitives;
    if wire.encode_sorted_json(&request.value).as_deref() != Some(&request.bytes)
        || format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, &request.bytes))
            != request.request_sha256
    {
        return Err(Error::Request);
    }
    let bytes = remote
        .burn_confirmation_access(&request.request_sha256, &request.bytes)
        .map_err(|_| Error::Transport)?;
    let response = NativeWirePrimitives
        .parse_canonical_object(&bytes)
        .ok_or(Error::Response)?;
    validate_result(&request.value, &response).ok_or(Error::Response)
}

/// Recompute every local preregistration identity before permitting a native access request.
fn validate_registration(value: &JsonValue) -> Option<()> {
    if !exact_keys(value, CONTEXT_FIELDS)
        || text(value, "contractVersion")? != "forecast-adjustment-lifecycle-ledger/v2"
        || text(value, "accessState")? != "registered"
        || text(value, "actionState")? != "none"
        || !matches!(field(value, "candidateReportSha256"), Some(JsonValue::Null))
        || !matches!(field(value, "actionIdentitySha256"), Some(JsonValue::Null))
    {
        return None;
    }
    let expected_candidate = match text(value, "family")?.as_str() {
        "temperature" => "temperature-delayed-mos/v1",
        "wind" => "wind-robust-hierarchical-median/v1",
        "rain" => "rain-hurdle-wind-occurrence-amount/v1",
        _ => return None,
    };
    if text(value, "candidateKind")? != expected_candidate {
        return None;
    }
    for key in REGISTRATION_HASH_FIELDS {
        if !valid_hash(&text(value, key)?) {
            return None;
        }
    }
    let wire = NativeWirePrimitives;
    for key in ["firstTargetAt", "terminalAccessAt"] {
        wire.canonical_utc_milliseconds(&text(value, key)?)?;
    }
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let unsigned = JsonValue::Object(
        fields
            .iter()
            .filter(|(key, _)| {
                ![
                    "accessState",
                    "actionState",
                    "candidateReportSha256",
                    "actionIdentitySha256",
                    "registrationSha256",
                ]
                .iter()
                .any(|field| key.iter().copied().eq(field.encode_utf16()))
            })
            .cloned()
            .collect(),
    );
    (digest_value(&unsigned)? == text(value, "registrationSha256")?).then_some(())
}

/// Admit the source local burn preimage and its immutable registration fields.
fn validate_local_burn(value: &JsonValue, registration: &JsonValue) -> Option<()> {
    let keys = CONTEXT_FIELDS
        .iter()
        .copied()
        .chain([
            "accessSha256",
            "accessedAt",
            "expectedKeySetSha256",
            "revisionCatalogWatermarkSha256",
            "targetComparatorSnapshotRootSha256",
            "targetCutoffAt",
        ])
        .collect::<Vec<_>>();
    if !exact_keys(value, &keys) {
        return None;
    }
    let context = lifecycle_context(registration, "burned")?;
    let JsonValue::Object(fields) = &context else {
        return None;
    };
    // compare every original source lifecycle field before advancing access state
    for (key, expected) in fields {
        if field(value, &String::from_utf16(key).ok()?)? != expected {
            return None;
        }
    }
    for name in [
        "accessSha256",
        "expectedKeySetSha256",
        "revisionCatalogWatermarkSha256",
        "targetComparatorSnapshotRootSha256",
    ] {
        if !valid_hash(&text(value, name)?) {
            return None;
        }
    }
    for name in ["accessedAt", "targetCutoffAt"] {
        NativeWirePrimitives.canonical_utc_milliseconds(&text(value, name)?)?;
    }
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let unsigned = JsonValue::Object(
        fields
            .iter()
            .filter(|(key, _)| !key.iter().copied().eq("accessSha256".encode_utf16()))
            .cloned()
            .collect(),
    );
    (digest_value(&unsigned)? == text(value, "accessSha256")?).then_some(())
}

/// Recompute the owner access preimage and bind it to the request's separate local identity.
fn validate_result(
    request: &JsonValue,
    result: &JsonValue,
) -> Option<NativeMaintenanceConfirmationAccessResult> {
    if !exact_keys(
        result,
        &[
            "contractVersion",
            "localAccessSha256",
            "nativeAccess",
            "nativeAccessSha256",
            "state",
        ],
    ) || text(result, "contractVersion")? != RESULT_VERSION
        || text(result, "state")? != "burned"
    {
        return None;
    }
    let native = field(result, "nativeAccess")?;
    if !exact_keys(native, NATIVE_FIELDS) {
        return None;
    }
    for key in NATIVE_FIELDS
        .iter()
        .copied()
        .filter(|key| key.ends_with("Sha256"))
    {
        if !valid_hash(&text(native, key)?) {
            return None;
        }
    }
    for key in ["accessedAt", "targetCutoffAt"] {
        NativeWirePrimitives.canonical_utc_milliseconds(&text(native, key)?)?;
    }
    let preimage = [
        "adjustment-confirmation-access/v2".to_owned(),
        text(native, "registrationSha256")?,
        text(native, "journalHeadSha256")?,
        text(native, "maintenanceAnchorSha256")?,
        text(native, "gateManifestSha256")?,
        text(native, "eligiblePredictionSetSha256")?,
        text(native, "expectedKeySetSha256")?,
        text(native, "metadataRootSha256")?,
        text(native, "targetComparatorSnapshotRootSha256")?,
        text(native, "revisionCatalogWatermarkSha256")?,
        text(native, "targetCutoffAt")?,
    ]
    .join("\n");
    let native_access_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        preimage.as_bytes(),
    ));
    let local_access_sha256 = text(result, "localAccessSha256")?;
    let local = field(request, "localBurn")?;
    let shadow = field(request, "shadowRegistration")?;
    let archive = field(request, "archive")?;
    // require the exact source response bindings without confusing the two access hashes
    if native_access_sha256 != text(result, "nativeAccessSha256")?
        || native_access_sha256 != text(native, "accessSha256")?
        || local_access_sha256 != text(local, "accessSha256")?
        || !valid_hash(&local_access_sha256)
        || text(native, "registrationSha256")? != text(shadow, "registrationSha256")?
        || text(native, "journalHeadSha256")? != text(request, "journalHeadSha256")?
        || text(native, "eligiblePredictionSetSha256")?
            != text(archive, "eligiblePredictionSetSha256")?
    {
        return None;
    }
    for key in [
        "expectedKeySetSha256",
        "gateManifestSha256",
        "revisionCatalogWatermarkSha256",
        "targetComparatorSnapshotRootSha256",
        "targetCutoffAt",
    ] {
        if text(native, key)? != text(local, key)? {
            return None;
        }
    }
    Some(NativeMaintenanceConfirmationAccessResult {
        local_access_sha256,
        native_access: native.clone(),
        native_access_sha256,
    })
}

/// Hash the original LF-framed canonical JSON.
fn digest_value(value: &JsonValue) -> Option<String> {
    let wire = NativeWirePrimitives;
    Some(format_digest(RevisionArchiveCanonicalCodec::sha256(
        &wire,
        &wire.encode_sorted_json(value)?,
    )))
}
