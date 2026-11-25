//! Closed native archive registry for successful cold-page members and shared ancestry.

use weather_contracts::JsonValue;
use weather_edge::{
    ArchiveWireCodec, NativeRevisionColdGroupCodec, PRIVATE_REVISION_BATCH_MAXIMUM_RECEIPTS,
    PRIVATE_REVISION_BATCH_PUBLISH_RECEIPT_MAXIMUM_BYTES,
    PRIVATE_REVISION_PUBLISH_RECEIPT_MAXIMUM_BYTES, PrivateRevisionReceiptCodec,
    PrivateShadowReceiptCodec,
};

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_comparator_authority::admit_shadow_comparator_authority;
use crate::api_shadow_wire_validation::{
    ShadowFamily, binary64_in, due_key, hash, instant, ordered_keys, rows, unsigned,
};
use crate::maintenance_anchor_status::{hash_field, nullable_hash_field, uint64_field};
use crate::maintenance_canonical_json::{exact_keys, field, text};
use crate::native_maintenance_cas_pack::{
    NativeMaintenanceCasMemberValidator, NativeMaintenanceCasStreamSession,
    NativeMaintenanceCasStreamValidator,
};
use crate::native_maintenance_cold_remote_documents::verify_native_revision_cold_start_document;
use crate::native_maintenance_gap_archive_validators::{
    NativeMaintenanceGapArchiveValidationSession, NativeMaintenanceGapArchiveValidators,
};
use crate::native_maintenance_revision_cold_drain::ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES;
use crate::{MaintenanceCanonicalJson, NativeModelEvidenceValues, NativeWirePrimitives};

const CHECKPOINT_KIND: &str = "adjustment-revision-cold-page-checkpoint/v1";
const START_KIND: &str = "adjustment-revision-cold-transfer-start/v1";
const COMMIT_KIND: &str = "adjustment-revision-commit-receipt/v1";
const SUCCESSOR_KIND: &str = "adjustment-revision-successor/v1";
const PUBLISH_KIND: &str = "adjustment-revision-publish-receipt/v1";
const BATCH_PUBLISH_KIND: &str = "adjustment-revision-batch-publish-receipt/v2";
const SHADOW_CAPSULE_V1: &str = "adjustment-shadow-revision-capsule/v1";
const SHADOW_CAPSULE_V2: &str = "adjustment-shadow-revision-capsule/v2";
const SHADOW_PUBLISH_V1: &str = "adjustment-shadow-publish-receipt/v1";
const SHADOW_PUBLISH_V2: &str = "adjustment-shadow-publish-receipt/v2";
const COMPARATOR_KIND: &str = "adjustment-shadow-incumbent-comparator/v1";
const COLD_METADATA_MAXIMUM_BYTES: usize = 4 * 1_024;
const SHADOW_CAPSULE_MAXIMUM_BYTES: usize = 2 * 1_024 * 1_024;
const SHADOW_COMPARATOR_MAXIMUM_BYTES: usize = 512 * 1_024;

const CHECKPOINT_FIELDS: &[&str] = &[
    "afterArchiveCommitOrdinal",
    "afterFrontierSha256",
    "contractVersion",
    "entries",
    "eof",
    "memberRootSha256",
    "nextArchiveCommitOrdinal",
    "nextFrontierSha256",
    "pageSha256",
    "previousPageSha256",
    "startMemberSha256",
    "startSha256",
    "watermarkArchiveCommitOrdinal",
    "watermarkFrontierSha256",
];
const CHECKPOINT_COMMON_HASH_FIELDS: &[&str] = &[
    "payloadIdentitySha256",
    "publicationIdentitySha256",
    "publicationMemberSha256",
    "stageReceiptMemberSha256",
    "stageReceiptSha256",
];
const CHECKPOINT_BATCH_FIELDS: &[&str] = &[
    "payloadIdentitySha256",
    "payloadKind",
    "publicationDisposition",
    "publicationIdentitySha256",
    "publicationMemberSha256",
    "receiptMemberSha256s",
    "receiptSha256s",
    "stageReceiptMemberSha256",
    "stageReceiptSha256",
    "successorMemberSha256s",
    "successorSha256s",
];
const CHECKPOINT_SINGLE_FIELDS: &[&str] = &[
    "payloadIdentitySha256",
    "payloadKind",
    "publicationDisposition",
    "publicationIdentitySha256",
    "publicationMemberSha256",
    "receiptMemberSha256",
    "receiptSha256",
    "stageReceiptMemberSha256",
    "stageReceiptSha256",
    "successorMemberSha256",
    "successorSha256",
];
const CHECKPOINT_COMPARATOR_FIELDS: &[&str] = &[
    "comparatorMemberSha256",
    "payloadIdentitySha256",
    "payloadKind",
    "publicationDisposition",
    "publicationIdentitySha256",
    "publicationMemberSha256",
    "receiptMemberSha256",
    "receiptSha256",
    "stageReceiptMemberSha256",
    "stageReceiptSha256",
    "successorMemberSha256",
    "successorSha256",
];
const CHECKPOINT_RAIN_FIELDS: &[&str] = &[
    "payloadIdentitySha256",
    "payloadKind",
    "publicationDisposition",
    "publicationIdentitySha256",
    "publicationMemberSha256",
    "rainControlStateIdentitySha256",
    "rainControlStateSha256",
    "rainControlStateStageReceiptMemberSha256",
    "rainControlStateStageReceiptSha256",
    "receiptMemberSha256",
    "receiptSha256",
    "stageReceiptMemberSha256",
    "stageReceiptSha256",
    "successorMemberSha256",
    "successorSha256",
];
const SINGLE_PAYLOAD_KINDS: &[&str] = &[
    "adjustment-revision-projection/v1",
    "adjustment-rain-gate-feature-projection/v2",
    "adjustment-rain-gate-control-projection/v3",
    "adjustment-rain-fixed-gauge-target-projection/v1",
    "adjustment-temperature-native-source-projection/v2",
    SHADOW_CAPSULE_V1,
    SHADOW_CAPSULE_V2,
];
const BATCH_PAYLOAD_KINDS: &[&str] = &[
    "adjustment-revision-batch-projection/v2",
    "adjustment-rain-fixed-gauge-target-projection/v1",
];

const COMPARATOR_FIELDS: &[&str] = &[
    "contractVersion",
    "family",
    "registrationSha256",
    "candidateSha256",
    "sourceSha256",
    "dueKey",
    "issuedAt",
    "sourceProjectionSha256",
    "predictionBodySha256",
    "servingAuthority",
    "rowCount",
    "rows",
];
const TEMPERATURE_COMPARATOR_ROW_FIELDS: &[&str] = &[
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "incumbentTemperatureC64",
    "applied",
    "reasonCode",
];
const WIND_COMPARATOR_ROW_FIELDS: &[&str] = &[
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "incumbentSpeedMps64",
    "incumbentGustMps64",
    "speedApplied",
    "gustApplied",
    "reasonCode",
];
const RAIN_COMPARATOR_ROW_FIELDS: &[&str] = &[
    "validAt",
    "leadHours",
    "sourceRowSha256",
    "incumbentPrecipitationMm64",
    "occurrenceProbability64",
    "atLeast1_0Probability64",
    "atLeast2_5Probability64",
    "positiveAmountMm64",
    "applied",
    "reasonCode",
];
const TEMPERATURE_REASONS: &[&str] = &[
    "bundle_invalid",
    "bundle_missing",
    "canary_expired",
    "canary_killed",
    "registry_inactive",
    "registry_invalid",
    "policy_raw",
    "unsupported",
    "missing_source_forecast",
    "outside_operational_window",
    "source_identity_mismatch",
    "source_not_available",
    "source_stale",
    "source_time_mismatch",
];
const WIND_REASONS: &[&str] = &[
    "adjustment_error",
    "bundle_invalid",
    "bundle_missing",
    "canary_expired",
    "canary_killed",
    "coefficient_missing",
    "direction_calm",
    "identity_mismatch",
    "metric_not_enabled",
    "metric_out_of_bounds",
    "policy_raw",
    "registry_inactive",
    "registry_invalid",
    "runtime_fingerprint_mismatch",
    "unsupported_lead",
    "wrong_cohort",
];
const RAIN_REASONS: &[&str] = &["phase_unsupported", "policy_raw", "prediction_invalid"];

/// Exact successful-cold, gap, and genesis archive member registry.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeMaintenanceColdArchiveValidators;

impl NativeMaintenanceColdArchiveValidators {
    /// Construct the value-free closed registry.
    pub const fn new() -> Self {
        Self
    }
}

impl NativeMaintenanceCasMemberValidator for NativeMaintenanceColdArchiveValidators {
    /// Validate one exact successful-cold member or an explicitly shared ancestry kind.
    fn validate(&mut self, kind: &str, payload: &[u8]) -> bool {
        // select only a registered successful-cold kind before considering shared ancestry
        if cold_member_maximum(kind).is_some() {
            return validate_cold_member(kind, payload);
        }
        // delegate only kinds explicitly emitted by gap or genesis segments
        if delegated_gap_or_genesis_kind(kind) {
            return NativeMaintenanceGapArchiveValidators::new().validate(kind, payload);
        }
        false
    }
}

impl NativeMaintenanceCasStreamValidator for NativeMaintenanceColdArchiveValidators {
    type Session = NativeMaintenanceColdArchiveValidationSession;

    /// Open one bounded validator only for a registered cold or shared ancestry kind.
    fn begin(&mut self, kind: &str, length: u64) -> Option<Self::Session> {
        // retain successful-cold bytes only beneath their source-specific ceiling
        if let Some((kind, maximum)) = cold_member_maximum(kind) {
            // refuse an oversized advertised body before allocating its buffer
            if length > maximum as u64 {
                return None;
            }
            return Some(NativeMaintenanceColdArchiveValidationSession {
                inner: NativeMaintenanceColdArchiveValidationInner::Cold {
                    bytes: Vec::with_capacity(usize::try_from(length).ok()?),
                    kind,
                    maximum,
                },
            });
        }
        // prevent the broader shared validator from becoming an unknown-kind fallback
        if !delegated_gap_or_genesis_kind(kind) {
            return None;
        }
        let session = NativeMaintenanceGapArchiveValidators::new().begin(kind, length)?;
        Some(NativeMaintenanceColdArchiveValidationSession {
            inner: NativeMaintenanceColdArchiveValidationInner::Delegated(session),
        })
    }
}

/// One public stream session retaining the selected closed validator class.
pub struct NativeMaintenanceColdArchiveValidationSession {
    inner: NativeMaintenanceColdArchiveValidationInner,
}

enum NativeMaintenanceColdArchiveValidationInner {
    Cold {
        bytes: Vec<u8>,
        kind: &'static str,
        maximum: usize,
    },
    Delegated(NativeMaintenanceGapArchiveValidationSession),
}

impl NativeMaintenanceCasStreamSession for NativeMaintenanceColdArchiveValidationSession {
    /// Retain one chunk without crossing the selected member ceiling.
    fn write(&mut self, bytes: &[u8]) -> bool {
        match &mut self.inner {
            NativeMaintenanceColdArchiveValidationInner::Cold {
                bytes: retained,
                maximum,
                ..
            } => {
                let Some(next) = retained.len().checked_add(bytes.len()) else {
                    return false;
                };
                // reject stream growth before extending the retained validator input
                if next > *maximum {
                    return false;
                }
                retained.extend_from_slice(bytes);
                true
            }
            NativeMaintenanceColdArchiveValidationInner::Delegated(session) => session.write(bytes),
        }
    }

    /// Finish through the exact validator selected when the stream opened.
    fn finish(self) -> bool {
        match self.inner {
            NativeMaintenanceColdArchiveValidationInner::Cold { bytes, kind, .. } => {
                validate_cold_member(kind, &bytes)
            }
            NativeMaintenanceColdArchiveValidationInner::Delegated(session) => session.finish(),
        }
    }
}

/// Validate one successful-cold member through its owning typed native codec.
fn validate_cold_member(kind: &str, payload: &[u8]) -> bool {
    let wire = ArchiveWireCodec::new(NativeWirePrimitives::new());
    // keep every registered kind on its complete schema and identity validator
    match kind {
        CHECKPOINT_KIND => validate_cold_checkpoint(payload).is_some(),
        START_KIND => verify_native_revision_cold_start_document(
            payload,
            &mut NativeWirePrimitives::new(),
            &NativeWirePrimitives::new(),
        )
        .is_ok(),
        COMMIT_KIND => wire.admitted_commit_receipt(payload).is_some(),
        SUCCESSOR_KIND => wire.admitted_successor(payload).is_some(),
        PUBLISH_KIND => {
            PrivateRevisionReceiptCodec::decode_publish_receipt(&wire, payload).is_some()
        }
        BATCH_PUBLISH_KIND => {
            PrivateRevisionReceiptCodec::decode_batch_publish_receipt(&wire, payload).is_some()
        }
        SHADOW_CAPSULE_V1 | SHADOW_CAPSULE_V2 => {
            PrivateShadowReceiptCodec::decode_capsule_document(&wire, payload).is_some()
        }
        SHADOW_PUBLISH_V1 | SHADOW_PUBLISH_V2 => {
            PrivateShadowReceiptCodec::decode_publish_receipt(&wire, payload).is_some()
        }
        COMPARATOR_KIND => validate_comparator_member(payload).is_some(),
        _ => false,
    }
}

/// Return one exact byte ceiling only for a successful-cold member kind.
fn cold_member_maximum(kind: &str) -> Option<(&'static str, usize)> {
    // keep member-specific ceilings aligned with the original retained stores
    match kind {
        CHECKPOINT_KIND => Some((
            CHECKPOINT_KIND,
            ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES,
        )),
        START_KIND => Some((START_KIND, ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES)),
        COMMIT_KIND => Some((COMMIT_KIND, COLD_METADATA_MAXIMUM_BYTES)),
        SUCCESSOR_KIND => Some((SUCCESSOR_KIND, COLD_METADATA_MAXIMUM_BYTES)),
        PUBLISH_KIND => Some((PUBLISH_KIND, PRIVATE_REVISION_PUBLISH_RECEIPT_MAXIMUM_BYTES)),
        BATCH_PUBLISH_KIND => Some((
            BATCH_PUBLISH_KIND,
            PRIVATE_REVISION_BATCH_PUBLISH_RECEIPT_MAXIMUM_BYTES,
        )),
        SHADOW_CAPSULE_V1 => Some((SHADOW_CAPSULE_V1, SHADOW_CAPSULE_MAXIMUM_BYTES)),
        SHADOW_CAPSULE_V2 => Some((SHADOW_CAPSULE_V2, SHADOW_CAPSULE_MAXIMUM_BYTES)),
        SHADOW_PUBLISH_V1 => Some((
            SHADOW_PUBLISH_V1,
            PRIVATE_REVISION_PUBLISH_RECEIPT_MAXIMUM_BYTES,
        )),
        SHADOW_PUBLISH_V2 => Some((
            SHADOW_PUBLISH_V2,
            PRIVATE_REVISION_PUBLISH_RECEIPT_MAXIMUM_BYTES,
        )),
        COMPARATOR_KIND => Some((COMPARATOR_KIND, SHADOW_COMPARATOR_MAXIMUM_BYTES)),
        _ => None,
    }
}

/// Admit only the complete explicit gap and genesis registries used by graph ancestry.
fn delegated_gap_or_genesis_kind(kind: &str) -> bool {
    matches!(
        kind,
        "adjustment-revision-gap-page-checkpoint/v1"
            | "adjustment-revision-gap/v1"
            | "adjustment-rain-control-state-gap/v1"
            | "adjustment-shadow-gap/v1"
            | "adjustment-revision-stage-receipt/v1"
            | "adjustment-rain-control-state-stage-receipt/v1"
            | "adjustment-shadow-stage-receipt/v1"
            | "adjustment-shadow-stage-receipt/v2"
            | "adjustment-revision-projection/v1"
            | "adjustment-revision-batch-projection/v2"
            | "adjustment-rain-gate-feature-projection/v2"
            | "adjustment-rain-gate-control-projection/v3"
            | "adjustment-rain-fixed-gauge-target-projection/v1"
            | "adjustment-temperature-native-source-projection/v2"
            | "rain-maintenance-control-state/v1"
            | "adjustment-shadow-revision-stage/v1"
            | "adjustment-shadow-revision-stage/v2"
            | "adjustment-revision-capture-epoch-witness/v1"
            | "adjustment-revision-serving-snapshot/v1"
    )
}

/// Validate one complete cold checkpoint and its exact member-root projection.
fn validate_cold_checkpoint(bytes: &[u8]) -> Option<()> {
    // bound the checkpoint before invoking the canonical JSON owner
    if !(2..=ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES).contains(&bytes.len()) {
        return None;
    }
    let mut codec = NativeWirePrimitives::new();
    let value = codec.parse_canonical_object(bytes)?;
    let JsonValue::Array(entries) = field(&value, "entries")? else {
        return None;
    };
    // require the closed top-level grammar and fixed two-entry population
    if !exact_keys(&value, CHECKPOINT_FIELDS)
        || text(&value, "contractVersion").as_deref() != Some(CHECKPOINT_KIND)
        || entries.len() > 2
        || !matches!(field(&value, "eof"), Some(JsonValue::Boolean(_)))
        || [
            "afterArchiveCommitOrdinal",
            "nextArchiveCommitOrdinal",
            "watermarkArchiveCommitOrdinal",
        ]
        .into_iter()
        .any(|name| uint64_field(&value, name).is_none())
        || [
            "afterFrontierSha256",
            "memberRootSha256",
            "nextFrontierSha256",
            "pageSha256",
            "previousPageSha256",
            "startSha256",
            "watermarkFrontierSha256",
        ]
        .into_iter()
        .any(|name| !hash_field(&value, name))
        || !nullable_hash_field(&value, "startMemberSha256")
    {
        return None;
    }
    let mut roots = Vec::with_capacity(entries.len());
    // validate every closed singular or grouped entry and project its member tuple
    for entry in entries {
        roots.push(validate_checkpoint_entry(entry)?);
    }
    let start_member = field(&value, "startMemberSha256")?.clone();
    let root_value = JsonValue::Object(vec![
        json_field("entries", JsonValue::Array(roots)),
        json_field("startMemberSha256", start_member),
    ]);
    let root_bytes = codec.encode_canonical_json(&root_value)?;
    let root_sha256 = codec.sha256(&root_bytes)?;
    let first_page = text(&value, "previousPageSha256")? == text(&value, "startSha256")?;
    let has_start = !matches!(field(&value, "startMemberSha256"), Some(JsonValue::Null));
    // bind the compact root and first-page-only start member without cross-member fallback
    (text(&value, "memberRootSha256").as_deref() == Some(root_sha256.as_str())
        && first_page == has_start)
        .then_some(())
}

/// Validate one checkpoint entry and project only the fields hashed into its root.
fn validate_checkpoint_entry(value: &JsonValue) -> Option<JsonValue> {
    let payload_kind = text(value, "payloadKind")?;
    let batch = field(value, "receiptMemberSha256s").is_some();
    let comparator = payload_kind == SHADOW_CAPSULE_V2;
    let rain = payload_kind == "adjustment-rain-gate-control-projection/v3";
    // select the one exact key population implied by the payload kind
    let expected_fields = if batch {
        CHECKPOINT_BATCH_FIELDS
    } else if comparator {
        CHECKPOINT_COMPARATOR_FIELDS
    } else if rain {
        CHECKPOINT_RAIN_FIELDS
    } else {
        CHECKPOINT_SINGLE_FIELDS
    };
    // reject extensions and malformed identities shared by every checkpoint entry
    if !exact_keys(value, expected_fields)
        || CHECKPOINT_COMMON_HASH_FIELDS
            .iter()
            .any(|name| !hash_field(value, name))
        || !matches!(
            text(value, "publicationDisposition")?.as_str(),
            "published" | "committed_unpublished_gap"
        )
    {
        return None;
    }
    // keep grouped and singular member grammars disjoint
    if batch {
        validate_batch_checkpoint_entry(value, &payload_kind)?;
    } else {
        validate_single_checkpoint_entry(value, &payload_kind, comparator, rain)?;
    }
    let mut root = Vec::new();
    // retain the additive comparator member only for the v2 capsule
    if comparator {
        root.push(json_field(
            "comparatorMemberSha256",
            string(&text(value, "comparatorMemberSha256")?),
        ));
    }
    root.extend([
        json_field(
            "payloadMemberSha256",
            string(&text(value, "payloadIdentitySha256")?),
        ),
        json_field(
            "publicationDisposition",
            string(&text(value, "publicationDisposition")?),
        ),
        json_field(
            "publicationIdentitySha256",
            string(&text(value, "publicationIdentitySha256")?),
        ),
        json_field(
            "publicationMemberSha256",
            string(&text(value, "publicationMemberSha256")?),
        ),
    ]);
    // retain both detached rain-control member addresses only on that payload kind
    if rain {
        root.extend([
            json_field(
                "rainControlStateMemberSha256",
                string(&text(value, "rainControlStateIdentitySha256")?),
            ),
            json_field(
                "rainControlStateStageReceiptMemberSha256",
                string(&text(value, "rainControlStateStageReceiptMemberSha256")?),
            ),
        ]);
    }
    // preserve the singular or grouped receipt and successor representation exactly
    if batch {
        root.extend([
            json_field(
                "receiptMemberSha256s",
                field(value, "receiptMemberSha256s")?.clone(),
            ),
            json_field(
                "stageReceiptMemberSha256",
                string(&text(value, "stageReceiptMemberSha256")?),
            ),
            json_field(
                "successorMemberSha256s",
                field(value, "successorMemberSha256s")?.clone(),
            ),
        ]);
    } else {
        root.extend([
            json_field(
                "receiptMemberSha256",
                string(&text(value, "receiptMemberSha256")?),
            ),
            json_field(
                "stageReceiptMemberSha256",
                string(&text(value, "stageReceiptMemberSha256")?),
            ),
            json_field(
                "successorMemberSha256",
                string(&text(value, "successorMemberSha256")?),
            ),
        ]);
    }
    Some(JsonValue::Object(root))
}

/// Validate one grouped checkpoint entry and all four parallel identity arrays.
fn validate_batch_checkpoint_entry(value: &JsonValue, payload_kind: &str) -> Option<()> {
    // restrict grouped bodies to the two source payload contracts
    if !BATCH_PAYLOAD_KINDS.contains(&payload_kind) {
        return None;
    }
    let arrays = [
        hash_array(value, "receiptMemberSha256s")?,
        hash_array(value, "receiptSha256s")?,
        hash_array(value, "successorMemberSha256s")?,
        hash_array(value, "successorSha256s")?,
    ];
    let count = arrays[0].len();
    // preserve bounded equal populations and exact successor byte identities
    if !(1..=PRIVATE_REVISION_BATCH_MAXIMUM_RECEIPTS).contains(&count)
        || arrays.iter().any(|items| items.len() != count)
        || arrays[2] != arrays[3]
    {
        return None;
    }
    Some(())
}

/// Validate one singular checkpoint entry and its additive kind-specific hashes.
fn validate_single_checkpoint_entry(
    value: &JsonValue,
    payload_kind: &str,
    comparator: bool,
    rain: bool,
) -> Option<()> {
    // reject unregistered payload contracts before reading singular member identities
    if !SINGLE_PAYLOAD_KINDS.contains(&payload_kind)
        || [
            "receiptMemberSha256",
            "receiptSha256",
            "successorMemberSha256",
            "successorSha256",
        ]
        .into_iter()
        .any(|name| !hash_field(value, name))
        || text(value, "successorMemberSha256")? != text(value, "successorSha256")?
    {
        return None;
    }
    // require the comparator address only on the additive v2 shadow capsule
    if comparator && !hash_field(value, "comparatorMemberSha256") {
        return None;
    }
    // require all detached public control state addresses as one inseparable group
    if rain
        && [
            "rainControlStateIdentitySha256",
            "rainControlStateSha256",
            "rainControlStateStageReceiptMemberSha256",
            "rainControlStateStageReceiptSha256",
        ]
        .into_iter()
        .any(|name| !hash_field(value, name))
    {
        return None;
    }
    Some(())
}

/// Borrow one nonempty bounded array containing only lowercase digest strings.
fn hash_array<'a>(value: &'a JsonValue, name: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(value, name)? else {
        return None;
    };
    values
        .iter()
        .all(|value| matches!(value, JsonValue::String(units) if valid_utf16_hash(units)))
        .then_some(values)
}

/// Validate one exact standalone comparator member retained from a v2 capsule.
fn validate_comparator_member(bytes: &[u8]) -> Option<()> {
    // retain the producer's ordered newline framing rather than accepting sorted-json aliases
    if !(2..=SHADOW_COMPARATOR_MAXIMUM_BYTES).contains(&bytes.len()) || !bytes.ends_with(b"\n") {
        return None;
    }
    let values = NativeModelEvidenceValues::new();
    let document = values.decode_shadow_json(bytes)?;
    let encoded = values.encode_shadow_ordered_json(&document)?;
    // require the producer's exact ordered bytes and closed top-level grammar
    if bytes.strip_suffix(b"\n") != Some(encoded.as_slice())
        || ordered_keys(&document, COMPARATOR_FIELDS).is_none()
        || text(&document, "contractVersion")?.as_str() != COMPARATOR_KIND
    {
        return None;
    }
    let family = ShadowFamily::read(&document)?;
    // require every standalone header identity, cadence and clock admitted by the producer
    if [
        "registrationSha256",
        "candidateSha256",
        "sourceSha256",
        "sourceProjectionSha256",
        "predictionBodySha256",
    ]
    .into_iter()
    .any(|name| hash(&document, name).is_none())
        || due_key(&values, &document).is_none()
        || instant(&values, &document, "issuedAt").is_none()
    {
        return None;
    }
    let comparator_rows = rows(&document, "rows")?;
    // preserve the family geometry independently of the capsule cross-member binding
    if unsigned(&document, "rowCount")? != u64::try_from(comparator_rows.len()).ok()?
        || comparator_rows.len() != family.row_count()
    {
        return None;
    }
    // validate every family-specific row without inferring its paired prediction body
    for row in comparator_rows {
        validate_comparator_row(family, row, &values)?;
    }
    let authority = field(&document, "servingAuthority")?;
    admit_shadow_comparator_authority(&values, authority, family)
}

/// Validate one comparator row's fixed fields, clocks, identities, values, and reason.
fn validate_comparator_row(
    family: ShadowFamily,
    row: &JsonValue,
    values: &NativeModelEvidenceValues,
) -> Option<()> {
    // retain the common source row coordinate before family-specific result admission
    if hash(row, "sourceRowSha256").is_none()
        || instant(values, row, "validAt").is_none()
        || unsigned(row, "leadHours").is_none()
    {
        return None;
    }
    match family {
        ShadowFamily::Temperature => {
            ordered_keys(row, TEMPERATURE_COMPARATOR_ROW_FIELDS)?;
            binary64_in(row, "incumbentTemperatureC64", -f64::MAX, f64::MAX, false)?;
            boolean(row, "applied")?;
            reason(row, "reasonCode", TEMPERATURE_REASONS)
        }
        ShadowFamily::Wind => {
            ordered_keys(row, WIND_COMPARATOR_ROW_FIELDS)?;
            binary64_in(row, "incumbentSpeedMps64", -f64::MAX, f64::MAX, false)?;
            binary64_in(row, "incumbentGustMps64", -f64::MAX, f64::MAX, true)?;
            boolean(row, "speedApplied")?;
            boolean(row, "gustApplied")?;
            reason(row, "reasonCode", WIND_REASONS)
        }
        ShadowFamily::Rain => validate_rain_comparator_row(row),
    }
}

/// Validate one rain comparator row including its applied completeness rule.
fn validate_rain_comparator_row(row: &JsonValue) -> Option<()> {
    ordered_keys(row, RAIN_COMPARATOR_ROW_FIELDS)?;
    binary64_in(
        row,
        "incumbentPrecipitationMm64",
        -f64::MAX,
        f64::MAX,
        false,
    )?;
    // preserve explicit unavailable probability outputs when the result is not applied
    for key in [
        "occurrenceProbability64",
        "atLeast1_0Probability64",
        "atLeast2_5Probability64",
    ] {
        binary64_in(row, key, 0.0, 1.0, true)?;
    }
    binary64_in(row, "positiveAmountMm64", -f64::MAX, f64::MAX, true)?;
    let applied = boolean(row, "applied")?;
    reason(row, "reasonCode", RAIN_REASONS)?;
    // require all modeled probability and amount outputs on an applied result
    if applied
        && [
            "occurrenceProbability64",
            "atLeast1_0Probability64",
            "atLeast2_5Probability64",
            "positiveAmountMm64",
        ]
        .into_iter()
        .any(|key| matches!(field(row, key), Some(JsonValue::Null)))
    {
        return None;
    }
    Some(())
}

/// Read one required JSON boolean without scalar coercion.
fn boolean(value: &JsonValue, name: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(value, name)? else {
        return None;
    };
    Some(*value)
}

/// Admit one nullable or closed family-specific comparator reason.
fn reason(value: &JsonValue, name: &str, admitted: &[&str]) -> Option<()> {
    match field(value, name)? {
        JsonValue::Null => Some(()),
        JsonValue::String(units)
            if admitted
                .iter()
                .any(|reason| units.iter().copied().eq(reason.encode_utf16())) =>
        {
            Some(())
        }
        _ => None,
    }
}

/// Recognize one lowercase digest directly from decoded UTF-16 code units.
fn valid_utf16_hash(value: &[u16]) -> bool {
    value.len() == 64
        && value.iter().all(|unit| {
            (u16::from(b'0')..=u16::from(b'9')).contains(unit)
                || (u16::from(b'a')..=u16::from(b'f')).contains(unit)
        })
}

/// Construct one canonical JSON object member for root reprojection.
fn json_field(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Construct one UTF-16 JSON string without another parser.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_cold_archive_validators_test.rs"]
mod tests;
