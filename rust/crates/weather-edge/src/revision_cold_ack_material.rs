//! Derive one cold-custody member root and exact hot-spool retirement set.

use weather_contracts::{FieldPresence, JsonValue};

use crate::private_shadow_archive::{
    PrivateShadowArchiveVersion, PrivateShadowCapsuleDocument, PrivateShadowReceiptCodec,
};
use crate::rain_control_state_archive::RainControlStateWireCodec;
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, lower_hex};
use crate::revision_cold_ack::{
    RevisionColdAckError, RevisionColdAckPage, RevisionColdRetirementEntry,
    RevisionColdRetirementKind,
};
use crate::revision_cold_page::{RevisionColdCursor, RevisionColdPageRequest};
use crate::revision_gap_transfer::RevisionGapSha256;

/// Whether one archived publication was retained or terminated in a durable gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionColdPublicationDisposition {
    Published,
    CommittedUnpublishedGap,
}

impl RevisionColdPublicationDisposition {
    /// Return the exact predecessor wire spelling used by the member-root document.
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::CommittedUnpublishedGap => "committed_unpublished_gap",
        }
    }
}

/// Address class of one admitted publication file made redundant by cold custody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevisionColdPublicationRetirement {
    RevisionPublished {
        projection_identity_sha256: RevisionGapSha256,
    },
    RevisionTerminalGap {
        canonical_address_bytes: Vec<u8>,
    },
    ShadowPublished {
        prediction_sha256: RevisionGapSha256,
    },
    ShadowTerminalGap {
        canonical_address_bytes: Vec<u8>,
    },
}

/// Exact optional rain-control members represented beside one control projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionColdRainControlMaterial {
    pub state_identity_sha256: RevisionGapSha256,
    pub state_sha256: RevisionGapSha256,
    pub stage_receipt_bytes: Vec<u8>,
}

/// One fully admitted cold-page entry with every canonical file-equivalent body retained.
#[derive(Debug, Clone, PartialEq)]
pub struct RevisionColdAckEntryMaterial {
    pub packed_shadow_entry: Option<JsonValue>,
    pub batch: bool,
    pub comparator_member_sha256: Option<RevisionGapSha256>,
    pub first_frontier_sha256: RevisionGapSha256,
    pub first_predecessor_frontier_sha256: RevisionGapSha256,
    pub first_projection_identity_sha256: RevisionGapSha256,
    pub first_receipt_sha256: RevisionGapSha256,
    pub group_bytes: Option<Vec<u8>>,
    pub payload_identity_sha256: RevisionGapSha256,
    pub payload_is_shadow: bool,
    pub payload_is_shadow_v2: bool,
    pub publication_bytes: Vec<u8>,
    pub publication_disposition: RevisionColdPublicationDisposition,
    pub publication_identity_sha256: RevisionGapSha256,
    pub publication_retirement: RevisionColdPublicationRetirement,
    pub rain_control: Option<RevisionColdRainControlMaterial>,
    pub receipt_bytes: Vec<Vec<u8>>,
    pub stage_receipt_bytes: Vec<u8>,
    pub successor_bytes: Vec<Vec<u8>>,
}

/// One page already admitted by the original cold-page canonical decoder.
#[derive(Debug, Clone, PartialEq)]
pub struct RevisionColdAckAdmittedPage {
    pub entries: Vec<RevisionColdAckEntryMaterial>,
    pub next: RevisionColdCursor,
    pub page_sha256: RevisionGapSha256,
    pub request: RevisionColdPageRequest,
}

/// File-equivalent hashes used in the predecessor's member-root preimage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionColdCheckpointEntry {
    pub batch: bool,
    pub comparator_member_sha256: Option<RevisionGapSha256>,
    pub payload_member_sha256: RevisionGapSha256,
    pub publication_disposition: RevisionColdPublicationDisposition,
    pub publication_identity_sha256: RevisionGapSha256,
    pub publication_member_sha256: RevisionGapSha256,
    pub rain_control_state_member_sha256: Option<RevisionGapSha256>,
    pub rain_control_state_stage_receipt_member_sha256: Option<RevisionGapSha256>,
    pub receipt_member_sha256s: Vec<RevisionGapSha256>,
    pub stage_receipt_member_sha256: RevisionGapSha256,
    pub successor_member_sha256s: Vec<RevisionGapSha256>,
}

/// Only the missing byte transport primitive beside the existing canonical JSON owners.
pub trait RevisionColdAckMaterialCodec:
    RainControlStateWireCodec + PrivateShadowReceiptCodec
{
    /// Decode strict canonical padded base64 without accepting alternate spellings.
    fn decode_canonical_base64(&self, value: &str) -> Option<Vec<u8>>;
}

/// Derive all custody material from one already validated, nonempty cold page.
pub fn derive_revision_cold_ack_page(
    page_bytes: &[u8],
    start_member_sha256: Option<RevisionGapSha256>,
    codec: &impl RevisionColdAckMaterialCodec,
) -> Result<RevisionColdAckPage, RevisionColdAckError> {
    let admitted = admit_page(page_bytes, codec).ok_or(RevisionColdAckError::Page)?;
    // custody is permitted only for one actual selected page, never an EOF-only page
    if admitted.entries.is_empty() || admitted.entries.len() > 2 {
        return Err(RevisionColdAckError::Page);
    }
    let first_page = admitted.request.previous == admitted.request.start;
    // retain the start member only at the first page boundary
    if first_page != start_member_sha256.is_some() {
        return Err(RevisionColdAckError::MemberRoot);
    }

    let mut checkpoint_entries = Vec::with_capacity(admitted.entries.len());
    let mut retirements = Vec::new();
    // derive each entry without re-decoding or copying its large payload body
    for entry in &admitted.entries {
        validate_entry_shape(entry)?;
        checkpoint_entries.push(checkpoint_entry(entry, codec));
        append_retirements(entry, codec, &mut retirements)?;
    }
    // match Node's kind then identity ordering before the acknowledgement is encoded
    retirements.sort_by_key(|entry| (entry.kind, entry.identity_sha256));
    if !(4..=10).contains(&retirements.len()) {
        return Err(RevisionColdAckError::Retirement);
    }
    let root_value = member_root_value(&checkpoint_entries, start_member_sha256);
    let root_bytes = codec
        .encode_control_sorted_json(&root_value)
        .ok_or(RevisionColdAckError::Encoding)?;
    let member_root_sha256 = digest(codec, &root_bytes);
    // preserve the v2 proof requirement without imposing it on legacy v1 custody
    let entry_count = admitted.entries.len();
    let packed_shadow_entries = admitted
        .entries
        .iter()
        .all(
            // a legacy shadow capsule cannot produce the packed metadata proof
            |entry| !entry.payload_is_shadow || entry.packed_shadow_entry.is_some(),
        )
        .then(|| {
            admitted
                .entries
                .into_iter()
                .filter_map(|entry| entry.packed_shadow_entry)
                .collect()
        });
    Ok(RevisionColdAckPage {
        packed_shadow_entries,
        request: admitted.request,
        next: admitted.next,
        page_sha256: admitted.page_sha256,
        entry_count,
        member_root_sha256,
        retirement_entries: retirements,
    })
}

/// Parse the canonical page through the existing JSON owner and preserve exact member bytes.
fn admit_page(
    page_bytes: &[u8],
    codec: &impl RevisionColdAckMaterialCodec,
) -> Option<RevisionColdAckAdmittedPage> {
    const PAGE_KEYS: &[&str] = &[
        "afterArchiveCommitOrdinal",
        "afterFrontierSha256",
        "contractVersion",
        "entries",
        "eof",
        "nextArchiveCommitOrdinal",
        "nextFrontierSha256",
        "pageSha256",
        "previousPageSha256",
        "startSha256",
        "watermarkArchiveCommitOrdinal",
        "watermarkFrontierSha256",
    ];
    let value = codec.decode_control_json(page_bytes)?;
    // refuse alternate serialization before deriving any file authority
    if !exact_keys(&value, PAGE_KEYS)
        || text(&value, "contractVersion")? != "adjustment-revision-cold-page/v1"
        || codec.encode_control_sorted_json(&value)?.as_slice() != page_bytes
    {
        return None;
    }
    let entries = array(&value, "entries")?;
    if entries.is_empty() || entries.len() > 2 {
        return None;
    }
    let page_sha256 = hash_field(&value, "pageSha256")?;
    let mut unsigned = object_fields(&value)?.to_vec();
    unsigned.retain(|(name, _)| !name.iter().copied().eq("pageSha256".encode_utf16()));
    let unsigned_bytes = codec.encode_control_sorted_json(&JsonValue::Object(unsigned))?;
    // bind the supplied page identity to every exact canonical member
    if digest(codec, &unsigned_bytes) != page_sha256 {
        return None;
    }
    let request = RevisionColdPageRequest {
        watermark: RevisionColdCursor {
            ordinal: uint64_field(&value, "watermarkArchiveCommitOrdinal")?,
            frontier: hash_field(&value, "watermarkFrontierSha256")?,
        },
        start: hash_field(&value, "startSha256")?,
        after: RevisionColdCursor {
            ordinal: uint64_field(&value, "afterArchiveCommitOrdinal")?,
            frontier: hash_field(&value, "afterFrontierSha256")?,
        },
        previous: hash_field(&value, "previousPageSha256")?,
    };
    let next = RevisionColdCursor {
        ordinal: uint64_field(&value, "nextArchiveCommitOrdinal")?,
        frontier: hash_field(&value, "nextFrontierSha256")?,
    };
    let mut admitted_entries = Vec::with_capacity(entries.len());
    // retain source entry order because it is the receipt-chain order
    for entry in entries {
        admitted_entries.push(admit_entry(entry, codec)?);
    }
    Some(RevisionColdAckAdmittedPage {
        entries: admitted_entries,
        next,
        page_sha256,
        request,
    })
}

/// Decode one already page-validated entry into exact file-equivalent material.
fn admit_entry(
    value: &JsonValue,
    codec: &impl RevisionColdAckMaterialCodec,
) -> Option<RevisionColdAckEntryMaterial> {
    let batch = field(value, "receipts").is_some();
    let receipt_values = if batch {
        array(value, "receipts")?.to_vec()
    } else {
        vec![field(value, "receipt")?.clone()]
    };
    let successor_values = if batch {
        array(value, "successors")?.to_vec()
    } else {
        vec![field(value, "successor")?.clone()]
    };
    if receipt_values.is_empty() || receipt_values.len() != successor_values.len() {
        return None;
    }
    let first = receipt_values.first()?;
    let first_frontier_sha256 = hash_field(first, "frontierSha256")?;
    let first_predecessor_frontier_sha256 = hash_field(first, "predecessorFrontierSha256")?;
    let first_projection_identity = text(first, "projectionIdentitySha256")?;
    let first_projection_identity_sha256 =
        RevisionGapSha256::from_lower_hex(&first_projection_identity)?;
    let first_receipt_sha256 = hash_field(first, "receiptSha256")?;
    let payload = field(value, "payload")?;
    let payload_kind = text(payload, "kind")?;
    let payload_identity_sha256 = hash_field(payload, "identitySha256")?;
    let payload_bytes = codec.decode_canonical_base64(&text(payload, "bytesBase64")?)?;
    // retain the base64 member only when its decoded identity is exact
    if digest(codec, &payload_bytes) != payload_identity_sha256 {
        return None;
    }
    let payload_is_shadow = text(first, "projectionKind")? == "shadow_prediction";
    let mut comparator_member_sha256 = None;
    let mut payload_is_shadow_v2 = false;
    let mut packed_shadow_entry = None;
    // use the existing producer-validating capsule codec for shadow-specific metadata
    if payload_is_shadow {
        let capsule = codec.decode_capsule_document(&payload_bytes)?;
        payload_is_shadow_v2 = capsule.version == PrivateShadowArchiveVersion::V2;
        // derive the compact proof from the already decoded exact capsule once
        if payload_is_shadow_v2 {
            packed_shadow_entry = Some(compact_shadow_metadata_entry(
                &capsule,
                payload_identity_sha256,
            )?);
        }
        comparator_member_sha256 = match capsule.stage_input.comparator {
            FieldPresence::Missing => None,
            FieldPresence::Value(bytes) => Some(digest(codec, &bytes)),
        };
    }

    let publication = field(value, "publication")?;
    let publication_disposition = match text(publication, "disposition")?.as_str() {
        "published" => RevisionColdPublicationDisposition::Published,
        "committed_unpublished_gap" => RevisionColdPublicationDisposition::CommittedUnpublishedGap,
        _ => return None,
    };
    let publication_value = field(publication, "value")?;
    let publication_bytes = canonical_bytes(codec, publication_value)?;
    let publication_identity_sha256 = match publication_disposition {
        RevisionColdPublicationDisposition::Published => {
            hash_field(publication_value, "publishReceiptSha256")?
        }
        RevisionColdPublicationDisposition::CommittedUnpublishedGap => {
            hash_field(publication_value, "gapSha256")?
        }
    };
    let publication_retirement = publication_retirement_material(
        payload_is_shadow,
        publication_disposition,
        first,
        publication_value,
        codec,
    )?;

    let rain_control = if payload_kind == "adjustment-rain-gate-control-projection/v3" {
        let state = field(value, "rainControlState")?;
        let state_bytes = codec.decode_canonical_base64(&text(state, "bytesBase64")?)?;
        let state_identity_sha256 = hash_field(state, "identitySha256")?;
        if digest(codec, &state_bytes) != state_identity_sha256 {
            return None;
        }
        Some(RevisionColdRainControlMaterial {
            state_identity_sha256,
            state_sha256: hash_field(state, "stateSha256")?,
            stage_receipt_bytes: canonical_bytes(
                codec,
                field(value, "rainControlStateStageReceipt")?,
            )?,
        })
    } else {
        None
    };

    let receipt_bytes = canonical_values(codec, &receipt_values)?;
    let successor_bytes = canonical_values(codec, &successor_values)?;
    let group_bytes = if batch {
        Some(canonical_bytes(
            codec,
            &JsonValue::Object(vec![
                json_entry(
                    "contractVersion",
                    json_string("adjustment-revision-batch-commit-group/v2"),
                ),
                json_entry(
                    "projectionIdentitySha256",
                    json_string(&first_projection_identity),
                ),
                json_entry("receipts", JsonValue::Array(receipt_values)),
                json_entry("successors", JsonValue::Array(successor_values)),
            ]),
        )?)
    } else {
        None
    };
    Some(RevisionColdAckEntryMaterial {
        packed_shadow_entry,
        batch,
        comparator_member_sha256,
        first_frontier_sha256,
        first_predecessor_frontier_sha256,
        first_projection_identity_sha256,
        first_receipt_sha256,
        group_bytes,
        payload_identity_sha256,
        payload_is_shadow,
        payload_is_shadow_v2,
        publication_bytes,
        publication_disposition,
        publication_identity_sha256,
        publication_retirement,
        rain_control,
        receipt_bytes,
        stage_receipt_bytes: canonical_bytes(codec, field(value, "stageReceipt")?)?,
        successor_bytes,
    })
}

/// Project the original packed v2 compact proof row from its authenticated capsule.
fn compact_shadow_metadata_entry(
    capsule: &PrivateShadowCapsuleDocument,
    capsule_sha256: RevisionGapSha256,
) -> Option<JsonValue> {
    let receipt = &capsule.revision_receipt;
    let metadata = &capsule.stage_input.metadata;
    let FieldPresence::Value(prediction_committed_at) = &capsule.prediction_committed_at else {
        return None;
    };
    // preserve the exact canonical numeric range of the generated wire contract
    if metadata.bodyByteCount > 9_007_199_254_740_991 || metadata.rowCount > 9_007_199_254_740_991 {
        return None;
    }
    Some(JsonValue::Object(vec![
        json_entry(
            "archiveCommitOrdinal",
            json_string(&receipt.archiveCommitOrdinal),
        ),
        json_entry(
            "archiveCommittedAt",
            json_string(&receipt.archiveCommittedAt),
        ),
        json_entry(
            "bodyByteCount",
            JsonValue::Number(metadata.bodyByteCount as f64),
        ),
        json_entry("capsuleSha256", hash_value(capsule_sha256)),
        json_entry("dueKey", json_string(&metadata.dueKey)),
        json_entry("frontierSha256", json_string(&receipt.frontierSha256)),
        json_entry("inputSha256", json_string(&metadata.inputSha256)),
        json_entry("issuedAt", json_string(&metadata.issuedAt)),
        json_entry("maxValidAt", json_string(&metadata.maxValidAt)),
        json_entry("minValidAt", json_string(&metadata.minValidAt)),
        json_entry(
            "predictionBodySha256",
            json_string(&metadata.predictionBodySha256),
        ),
        json_entry(
            "predictionCommittedAt",
            json_string(prediction_committed_at),
        ),
        json_entry(
            "predictionSchemaSha256",
            json_string(&metadata.predictionSchemaSha256),
        ),
        json_entry("predictionSha256", json_string(&metadata.predictionSha256)),
        json_entry(
            "predecessorFrontierSha256",
            json_string(&receipt.predecessorFrontierSha256),
        ),
        json_entry("receiptSha256", json_string(&receipt.receiptSha256)),
        json_entry(
            "registrationSha256",
            json_string(&metadata.registrationSha256),
        ),
        json_entry("rowCount", JsonValue::Number(metadata.rowCount as f64)),
        json_entry(
            "sourceReceiptSha256",
            json_string(&metadata.sourceReceiptSha256),
        ),
        json_entry(
            "stageReceiptSha256",
            json_string(&receipt.stageReceiptSha256),
        ),
    ]))
}

/// Derive a publication's sole fixed hot-spool address from its admitted value.
fn publication_retirement_material(
    shadow: bool,
    disposition: RevisionColdPublicationDisposition,
    first_receipt: &JsonValue,
    publication: &JsonValue,
    codec: &impl RevisionColdAckMaterialCodec,
) -> Option<RevisionColdPublicationRetirement> {
    let material = match (shadow, disposition) {
        (false, RevisionColdPublicationDisposition::Published) => {
            RevisionColdPublicationRetirement::RevisionPublished {
                projection_identity_sha256: hash_field(first_receipt, "projectionIdentitySha256")?,
            }
        }
        (true, RevisionColdPublicationDisposition::Published) => {
            RevisionColdPublicationRetirement::ShadowPublished {
                prediction_sha256: hash_field(publication, "predictionSha256")?,
            }
        }
        (false, RevisionColdPublicationDisposition::CommittedUnpublishedGap) => {
            let input = field(publication, "input")?;
            let address = JsonValue::Object(vec![
                json_entry(
                    "logicalKeySha256",
                    json_string(&text(input, "logicalKeySha256")?),
                ),
                json_entry(
                    "projectionIdentitySha256",
                    json_string(&text(input, "projectionIdentitySha256")?),
                ),
            ]);
            RevisionColdPublicationRetirement::RevisionTerminalGap {
                canonical_address_bytes: codec.encode_control_sorted_json(&address)?,
            }
        }
        (true, RevisionColdPublicationDisposition::CommittedUnpublishedGap) => {
            let input = field(publication, "input")?;
            let address = JsonValue::Object(vec![
                json_entry("dueKey", json_string(&text(input, "dueKey")?)),
                json_entry(
                    "registrationSha256",
                    json_string(&text(input, "registrationSha256")?),
                ),
            ]);
            RevisionColdPublicationRetirement::ShadowTerminalGap {
                canonical_address_bytes: codec.encode_control_sorted_json(&address)?,
            }
        }
    };
    Some(material)
}

/// Build the exact sorted JSON value whose canonical bytes define the member root.
fn member_root_value(
    entries: &[RevisionColdCheckpointEntry],
    start_member_sha256: Option<RevisionGapSha256>,
) -> JsonValue {
    JsonValue::Object(vec![
        json_entry(
            "entries",
            JsonValue::Array(entries.iter().map(checkpoint_value).collect()),
        ),
        json_entry(
            "startMemberSha256",
            start_member_sha256.map_or(JsonValue::Null, hash_value),
        ),
    ])
}

/// Project one checkpoint entry with singular versus grouped fields kept explicit.
fn checkpoint_value(entry: &RevisionColdCheckpointEntry) -> JsonValue {
    let mut fields = Vec::new();
    // comparator identity exists only for the v2 shadow capsule
    if let Some(value) = entry.comparator_member_sha256 {
        fields.push(json_entry("comparatorMemberSha256", hash_value(value)));
    }
    fields.extend([
        json_entry(
            "payloadMemberSha256",
            hash_value(entry.payload_member_sha256),
        ),
        json_entry(
            "publicationDisposition",
            json_string(entry.publication_disposition.wire_value()),
        ),
        json_entry(
            "publicationIdentitySha256",
            hash_value(entry.publication_identity_sha256),
        ),
        json_entry(
            "publicationMemberSha256",
            hash_value(entry.publication_member_sha256),
        ),
    ]);
    // rain control state and stage members are always paired
    if let Some(value) = entry.rain_control_state_member_sha256 {
        fields.push(json_entry(
            "rainControlStateMemberSha256",
            hash_value(value),
        ));
        fields.push(json_entry(
            "rainControlStateStageReceiptMemberSha256",
            hash_value(
                entry
                    .rain_control_state_stage_receipt_member_sha256
                    .expect("paired admitted rain member"),
            ),
        ));
    }
    if entry.batch {
        fields.push(json_entry(
            "receiptMemberSha256s",
            hash_array(&entry.receipt_member_sha256s),
        ));
    } else {
        fields.push(json_entry(
            "receiptMemberSha256",
            hash_value(entry.receipt_member_sha256s[0]),
        ));
    }
    fields.push(json_entry(
        "stageReceiptMemberSha256",
        hash_value(entry.stage_receipt_member_sha256),
    ));
    if entry.batch {
        fields.push(json_entry(
            "successorMemberSha256s",
            hash_array(&entry.successor_member_sha256s),
        ));
    } else {
        fields.push(json_entry(
            "successorMemberSha256",
            hash_value(entry.successor_member_sha256s[0]),
        ));
    }
    JsonValue::Object(fields)
}

/// Encode a list of exact canonical JSON subdocuments without changing entry order.
fn canonical_values(
    codec: &impl RainControlStateWireCodec,
    values: &[JsonValue],
) -> Option<Vec<Vec<u8>>> {
    values
        .iter()
        .map(|value| canonical_bytes(codec, value))
        .collect()
}

/// Encode one recursively sorted canonical subdocument with its retained line feed.
fn canonical_bytes(codec: &impl RainControlStateWireCodec, value: &JsonValue) -> Option<Vec<u8>> {
    codec.encode_control_sorted_json(value)
}

/// Return one object field list without accepting array or scalar coercion.
fn object_fields(value: &JsonValue) -> Option<&[(Vec<u16>, JsonValue)]> {
    match value {
        JsonValue::Object(fields) => Some(fields),
        _ => None,
    }
}

/// Borrow one exact object member by its unnormalized UTF-16 key.
fn field<'a>(value: &'a JsonValue, name: &str) -> Option<&'a JsonValue> {
    object_fields(value)?
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
}

/// Require the complete closed object key set.
fn exact_keys(value: &JsonValue, names: &[&str]) -> bool {
    let Some(fields) = object_fields(value) else {
        return false;
    };
    fields.len() == names.len() && names.iter().all(|name| field(value, name).is_some())
}

/// Borrow one array-valued object member.
fn array<'a>(value: &'a JsonValue, name: &str) -> Option<&'a [JsonValue]> {
    match field(value, name)? {
        JsonValue::Array(values) => Some(values),
        _ => None,
    }
}

/// Decode one UTF-16 JSON string without replacement characters.
fn text(value: &JsonValue, name: &str) -> Option<String> {
    match field(value, name)? {
        JsonValue::String(value) => String::from_utf16(value).ok(),
        _ => None,
    }
}

/// Decode one canonical lower-hex digest field.
fn hash_field(value: &JsonValue, name: &str) -> Option<RevisionGapSha256> {
    RevisionGapSha256::from_lower_hex(&text(value, name)?)
}

/// Decode one canonical decimal uint64 string.
fn uint64_field(value: &JsonValue, name: &str) -> Option<u64> {
    let value = text(value, name)?;
    let parsed = value.parse::<u64>().ok()?;
    (parsed.to_string() == value).then_some(parsed)
}

/// Construct one generated JSON object member.
fn json_entry(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Construct one generated JSON string without a runtime string codec.
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Construct one lowercase digest JSON string.
fn hash_value(value: RevisionGapSha256) -> JsonValue {
    json_string(&lower_hex(*value.as_bytes()))
}

/// Construct one ordered digest array for a grouped receipt body.
fn hash_array(values: &[RevisionGapSha256]) -> JsonValue {
    JsonValue::Array(values.iter().copied().map(hash_value).collect())
}

/// Reject representation combinations that cannot exist in an admitted Node cold-page entry.
fn validate_entry_shape(entry: &RevisionColdAckEntryMaterial) -> Result<(), RevisionColdAckError> {
    let paired_members =
        !entry.receipt_bytes.is_empty() && entry.receipt_bytes.len() == entry.successor_bytes.len();
    let group_shape = entry.batch == entry.group_bytes.is_some()
        && (entry.batch || entry.receipt_bytes.len() == 1);
    let shadow_shape = entry.payload_is_shadow
        == matches!(
            entry.publication_retirement,
            RevisionColdPublicationRetirement::ShadowPublished { .. }
                | RevisionColdPublicationRetirement::ShadowTerminalGap { .. }
        );
    // comparator membership is additive only for the v2 shadow capsule
    if !paired_members
        || !group_shape
        || !shadow_shape
        || (entry.comparator_member_sha256.is_some() != entry.payload_is_shadow_v2)
        || (entry.payload_is_shadow_v2 && !entry.payload_is_shadow)
    {
        return Err(RevisionColdAckError::Page);
    }
    Ok(())
}

/// Hash exact canonical member bytes into the compact checkpoint representation.
fn checkpoint_entry(
    entry: &RevisionColdAckEntryMaterial,
    codec: &impl RevisionArchiveCanonicalCodec,
) -> RevisionColdCheckpointEntry {
    RevisionColdCheckpointEntry {
        batch: entry.batch,
        comparator_member_sha256: entry.comparator_member_sha256,
        payload_member_sha256: entry.payload_identity_sha256,
        publication_disposition: entry.publication_disposition,
        publication_identity_sha256: entry.publication_identity_sha256,
        publication_member_sha256: digest(codec, &entry.publication_bytes),
        rain_control_state_member_sha256: entry
            .rain_control
            .as_ref()
            .map(|rain| rain.state_identity_sha256),
        rain_control_state_stage_receipt_member_sha256: entry
            .rain_control
            .as_ref()
            .map(|rain| digest(codec, &rain.stage_receipt_bytes)),
        receipt_member_sha256s: entry
            .receipt_bytes
            .iter()
            .map(|bytes| digest(codec, bytes))
            .collect(),
        stage_receipt_member_sha256: digest(codec, &entry.stage_receipt_bytes),
        successor_member_sha256s: entry
            .successor_bytes
            .iter()
            .map(|bytes| digest(codec, bytes))
            .collect(),
    }
}

/// Add the exact fixed hot paths represented by one admitted entry.
fn append_retirements(
    entry: &RevisionColdAckEntryMaterial,
    codec: &impl RevisionArchiveCanonicalCodec,
    output: &mut Vec<RevisionColdRetirementEntry>,
) -> Result<(), RevisionColdAckError> {
    // grouped receipts retire as one atomic all-receipt file
    if let Some(group_bytes) = &entry.group_bytes {
        push_retirement(
            output,
            RevisionColdRetirementKind::RevisionCommitGroup,
            entry.first_projection_identity_sha256,
            digest(codec, group_bytes),
        );
    } else {
        push_retirement(
            output,
            RevisionColdRetirementKind::RevisionCommitReceipt,
            entry.first_frontier_sha256,
            digest(codec, &entry.receipt_bytes[0]),
        );
        push_retirement(
            output,
            RevisionColdRetirementKind::RevisionFrontierSuccessor,
            entry.first_predecessor_frontier_sha256,
            digest(codec, &entry.successor_bytes[0]),
        );
    }

    if entry.payload_is_shadow {
        push_retirement(
            output,
            RevisionColdRetirementKind::ShadowRevisionCapsule,
            entry.first_receipt_sha256,
            entry.payload_identity_sha256,
        );
    } else {
        push_retirement(
            output,
            RevisionColdRetirementKind::RevisionProjection,
            entry.first_projection_identity_sha256,
            entry.payload_identity_sha256,
        );
        push_retirement(
            output,
            RevisionColdRetirementKind::RevisionStageReceipt,
            entry.first_projection_identity_sha256,
            digest(codec, &entry.stage_receipt_bytes),
        );
        // bind public rain state to the same exact page custody as its control projection
        if let Some(rain) = &entry.rain_control {
            push_retirement(
                output,
                RevisionColdRetirementKind::RainControlState,
                rain.state_sha256,
                rain.state_identity_sha256,
            );
            push_retirement(
                output,
                RevisionColdRetirementKind::RainControlStateStageReceipt,
                rain.state_sha256,
                digest(codec, &rain.stage_receipt_bytes),
            );
        }
    }

    let (kind, identity_sha256) = publication_retirement(entry, codec)?;
    push_retirement(
        output,
        kind,
        identity_sha256,
        digest(codec, &entry.publication_bytes),
    );
    Ok(())
}

/// Select the publication path and derive only terminal-gap locator identities.
fn publication_retirement(
    entry: &RevisionColdAckEntryMaterial,
    codec: &impl RevisionArchiveCanonicalCodec,
) -> Result<(RevisionColdRetirementKind, RevisionGapSha256), RevisionColdAckError> {
    let retirement = match &entry.publication_retirement {
        RevisionColdPublicationRetirement::RevisionPublished {
            projection_identity_sha256,
        } => (
            RevisionColdRetirementKind::RevisionPublishReceipt,
            *projection_identity_sha256,
        ),
        RevisionColdPublicationRetirement::RevisionTerminalGap {
            canonical_address_bytes,
        } => (
            RevisionColdRetirementKind::RevisionTerminalGap,
            digest(codec, canonical_address_bytes),
        ),
        RevisionColdPublicationRetirement::ShadowPublished { prediction_sha256 } => (
            RevisionColdRetirementKind::ShadowRevisionPublishReceipt,
            *prediction_sha256,
        ),
        RevisionColdPublicationRetirement::ShadowTerminalGap {
            canonical_address_bytes,
        } => (
            RevisionColdRetirementKind::ShadowRevisionTerminalGap,
            digest(codec, canonical_address_bytes),
        ),
    };
    let disposition_matches = matches!(
        (
            &entry.publication_disposition,
            &entry.publication_retirement
        ),
        (
            RevisionColdPublicationDisposition::Published,
            RevisionColdPublicationRetirement::RevisionPublished { .. }
                | RevisionColdPublicationRetirement::ShadowPublished { .. }
        ) | (
            RevisionColdPublicationDisposition::CommittedUnpublishedGap,
            RevisionColdPublicationRetirement::RevisionTerminalGap { .. }
                | RevisionColdPublicationRetirement::ShadowTerminalGap { .. }
        )
    );
    if !disposition_matches {
        return Err(RevisionColdAckError::Page);
    }
    Ok(retirement)
}

/// Append one already admitted fixed file address.
fn push_retirement(
    output: &mut Vec<RevisionColdRetirementEntry>,
    kind: RevisionColdRetirementKind,
    identity_sha256: RevisionGapSha256,
    file_sha256: RevisionGapSha256,
) {
    output.push(RevisionColdRetirementEntry {
        kind,
        identity_sha256,
        file_sha256,
    });
}

/// Hash exact canonical bytes without adding another cryptographic implementation.
fn digest(codec: &impl RevisionArchiveCanonicalCodec, bytes: &[u8]) -> RevisionGapSha256 {
    RevisionGapSha256::admitted(codec.sha256(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Codec;

    impl RevisionArchiveCanonicalCodec for Codec {
        /// Timestamps are outside the pure material projection tests.
        fn canonical_utc_milliseconds(&self, _: &str) -> Option<i64> {
            None
        }

        /// Produce a stable content-sensitive fixture digest without claiming production crypto.
        fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
            let mut state = [0_u8; 32];
            // fold every source byte into a stable fixed-width fixture identity
            for (index, byte) in bytes.iter().copied().enumerate() {
                state[index % 32] = state[index % 32].wrapping_add(byte);
            }
            state
        }
    }

    #[test]
    fn keeps_one_receipt_group_atomic_in_root_and_retirement_material() {
        let codec = Codec;
        let mut entry = ordinary_entry(true);
        entry.group_bytes = Some(b"group\n".to_vec());
        validate_entry_shape(&entry).expect("valid one-row group");
        let checkpoint = checkpoint_entry(&entry, &codec);
        let value = checkpoint_value(&checkpoint);
        assert!(field(&value, "receiptMemberSha256s").is_some());
        assert!(field(&value, "receiptMemberSha256").is_none());

        let mut retirements = Vec::new();
        append_retirements(&entry, &codec, &mut retirements).expect("group retirements");
        assert_eq!(retirements.len(), 4);
        assert!(retirements.iter().any(
            // retire the atomic group rather than its embedded receipt and successor
            |entry| entry.kind == RevisionColdRetirementKind::RevisionCommitGroup
        ));
        assert!(!retirements.iter().any(
            // embedded group members have no independent hot-spool file
            |entry| matches!(
                entry.kind,
                RevisionColdRetirementKind::RevisionCommitReceipt
                    | RevisionColdRetirementKind::RevisionFrontierSuccessor
            )
        ));
    }

    #[test]
    fn derives_rain_auxiliary_and_terminal_gap_addresses() {
        let codec = Codec;
        let mut entry = ordinary_entry(false);
        entry.rain_control = Some(RevisionColdRainControlMaterial {
            state_identity_sha256: marker(21),
            state_sha256: marker(22),
            stage_receipt_bytes: b"rain-stage\n".to_vec(),
        });
        entry.publication_disposition = RevisionColdPublicationDisposition::CommittedUnpublishedGap;
        entry.publication_retirement = RevisionColdPublicationRetirement::RevisionTerminalGap {
            canonical_address_bytes: b"gap-address\n".to_vec(),
        };
        let checkpoint = checkpoint_entry(&entry, &codec);
        assert_eq!(
            checkpoint.rain_control_state_member_sha256,
            Some(marker(21))
        );

        let mut retirements = Vec::new();
        append_retirements(&entry, &codec, &mut retirements).expect("rain retirements");
        retirements.sort_by_key(|entry| (entry.kind, entry.identity_sha256));
        assert_eq!(retirements.len(), 7);
        assert!(retirements.iter().any(
            // retain the exact state-stage receipt beside the public state
            |entry| entry.kind == RevisionColdRetirementKind::RainControlStateStageReceipt
        ));
        assert!(retirements.iter().any(
            // derive the terminal gap's fixed address from its canonical locator bytes
            |entry| entry.kind == RevisionColdRetirementKind::RevisionTerminalGap
                && entry.identity_sha256 == digest(&codec, b"gap-address\n")
        ));
    }

    #[test]
    fn requires_v2_shadow_comparator_and_parallel_members() {
        let mut shadow = ordinary_entry(false);
        shadow.payload_is_shadow = true;
        shadow.payload_is_shadow_v2 = true;
        shadow.publication_retirement = RevisionColdPublicationRetirement::ShadowPublished {
            prediction_sha256: marker(31),
        };
        assert_eq!(
            validate_entry_shape(&shadow),
            Err(RevisionColdAckError::Page)
        );
        shadow.comparator_member_sha256 = Some(marker(32));
        validate_entry_shape(&shadow).expect("v2 comparator");
        shadow.successor_bytes.clear();
        assert_eq!(
            validate_entry_shape(&shadow),
            Err(RevisionColdAckError::Page)
        );
    }

    #[test]
    // project all twenty compact metadata fields from their source capsule owners
    fn packed_shadow_row_retains_exact_metadata_fields() {
        use weather_contracts::{
            AdjustmentMaintenanceStageInput, AdjustmentMaintenanceStageReceipt,
            AdjustmentMaintenanceStageReceiptContractVersionEnum,
            AdjustmentMaintenanceStageReceiptDurableLiteral, AdjustmentRevisionCommitReceipt,
            AdjustmentRevisionCommitReceiptContractVersionLiteral,
            AdjustmentRevisionCommitReceiptProjectionKindEnum, MaintenanceShadowPredictionMetadata,
        };
        let capsule = PrivateShadowCapsuleDocument {
            version: PrivateShadowArchiveVersion::V2,
            prediction_committed_at: FieldPresence::Value("prediction-time".into()),
            revision_receipt: AdjustmentRevisionCommitReceipt {
                archiveCommitOrdinal: "42".into(), archiveCommittedAt:"archive-time".into(),
                contractVersion:AdjustmentRevisionCommitReceiptContractVersionLiteral,
                frontierSha256:"frontier".into(), predecessorFrontierSha256:"predecessor".into(),
                projectionIdentitySha256:"projection".into(),
                projectionKind:AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction,
                projectionSha256:"body".into(), receiptSha256:"receipt".into(),
                stageReceiptSha256:"stage".into(),
            },
            stage_input:AdjustmentMaintenanceStageInput {
                body:vec![], comparator:FieldPresence::Value(vec![]), sourceProjection:vec![],
                sourceProjectionSha256:"source-projection".into(),
                metadata:MaintenanceShadowPredictionMetadata {
                    bodyByteCount:123, candidateSha256:"candidate".into(), dueKey:"due".into(),
                    inputSha256:"input".into(), issuedAt:"issued".into(), maxValidAt:"max".into(),
                    minValidAt:"min".into(), predictionBodySha256:"prediction-body".into(),
                    predictionSchemaSha256:"prediction-schema".into(), predictionSha256:"prediction".into(),
                    registrationSha256:"registration".into(), rowCount:12, sourceReceiptSha256:"source-receipt".into(),
                    sourceSha256:"source".into(),
                },
            },
            stage_receipt:AdjustmentMaintenanceStageReceipt {
                comparatorSha256:FieldPresence::Value("comp".into()),
                contractVersion:AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV2,
                durable:AdjustmentMaintenanceStageReceiptDurableLiteral,
                durableAt:"durable".into(), dueKey:"due".into(), predictionBodySha256:"prediction-body".into(),
                registrationSha256:"registration".into(), sourceProjectionSha256:"source".into(),
                stageReceiptSha256:"stage".into(),
            }
        };
        let entry = compact_shadow_metadata_entry(&capsule, marker(2)).expect("v2 proof entry");
        assert_eq!(object_fields(&entry).unwrap().len(), 20);
        assert_eq!(text(&entry, "archiveCommitOrdinal").as_deref(), Some("42"));
        assert_eq!(
            text(&entry, "predictionCommittedAt").as_deref(),
            Some("prediction-time")
        );
        assert_eq!(
            text(&entry, "predecessorFrontierSha256").as_deref(),
            Some("predecessor")
        );
        assert_eq!(
            text(&entry, "sourceReceiptSha256").as_deref(),
            Some("source-receipt")
        );
        assert_eq!(
            field(&entry, "bodyByteCount"),
            Some(&JsonValue::Number(123.0))
        );
        assert_eq!(field(&entry, "rowCount"), Some(&JsonValue::Number(12.0)));
        assert_eq!(hash_field(&entry, "capsuleSha256"), Some(marker(2)));
    }

    /// Build one ordinary single or one-receipt grouped material fixture.
    fn ordinary_entry(batch: bool) -> RevisionColdAckEntryMaterial {
        RevisionColdAckEntryMaterial {
            packed_shadow_entry: None,
            batch,
            comparator_member_sha256: None,
            first_frontier_sha256: marker(1),
            first_predecessor_frontier_sha256: marker(2),
            first_projection_identity_sha256: marker(3),
            first_receipt_sha256: marker(4),
            group_bytes: None,
            payload_identity_sha256: marker(5),
            payload_is_shadow: false,
            payload_is_shadow_v2: false,
            publication_bytes: b"publication\n".to_vec(),
            publication_disposition: RevisionColdPublicationDisposition::Published,
            publication_identity_sha256: marker(6),
            publication_retirement: RevisionColdPublicationRetirement::RevisionPublished {
                projection_identity_sha256: marker(3),
            },
            rain_control: None,
            receipt_bytes: vec![b"receipt\n".to_vec()],
            stage_receipt_bytes: b"stage\n".to_vec(),
            successor_bytes: vec![b"successor\n".to_vec()],
        }
    }

    /// Create one recognizable fixed test digest.
    const fn marker(value: u8) -> RevisionGapSha256 {
        RevisionGapSha256::admitted([value; 32])
    }
}
