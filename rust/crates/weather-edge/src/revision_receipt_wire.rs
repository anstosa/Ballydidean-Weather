//! Concrete canonical revision receipt and cold-chain wire ownership.

use weather_contracts::{
    AdjustmentRevisionBatchPublishReceipt, AdjustmentRevisionBatchPublishReceiptCommittedLiteral,
    AdjustmentRevisionBatchPublishReceiptContractVersionLiteral,
    AdjustmentRevisionBatchPublishReceiptProjectionKindEnum, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionProjectionKind,
    AdjustmentRevisionPublishReceipt, AdjustmentRevisionPublishReceiptCommittedLiteral,
    AdjustmentRevisionPublishReceiptContractVersionLiteral, AdjustmentRevisionStageReceipt,
    AdjustmentRevisionStageReceiptContractVersionLiteral,
    AdjustmentRevisionStageReceiptDurableLiteral, JsonValue,
};

use crate::archive_wire::{
    ArchiveWireCodec, ArchiveWirePrimitives, array, bool_value, exact_keys, hash, member, string,
    text, uint64,
};
use crate::native_revision_spool::{
    NativeRevisionColdBatchGroup, NativeRevisionColdGroupCodec, NativeRevisionColdReceipt,
    NativeRevisionColdSuccessor,
};
use crate::private_revision_archive::{
    PRIVATE_REVISION_BATCH_MAXIMUM_RECEIPTS, PrivateRevisionBatchPublishUnsigned,
    PrivateRevisionPublishUnsigned, PrivateRevisionReceiptCodec, PrivateRevisionStageUnsigned,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant, lower_hex};
use crate::revision_gap_transfer::RevisionGapSha256;

const BATCH_COMMIT_GROUP_VERSION: &str = "adjustment-revision-batch-commit-group/v2";
const SUCCESSOR_VERSION: &str = "adjustment-revision-successor/v1";
const COMMIT_KEYS: &[&str] = &[
    "archiveCommitOrdinal",
    "archiveCommittedAt",
    "contractVersion",
    "frontierSha256",
    "predecessorFrontierSha256",
    "projectionIdentitySha256",
    "projectionKind",
    "projectionSha256",
    "receiptSha256",
    "stageReceiptSha256",
];
const STAGE_KEYS: &[&str] = &[
    "contractVersion",
    "durable",
    "durableAt",
    "projectionIdentitySha256",
    "projectionKind",
    "projectionSha256",
    "stageReceiptSha256",
];
const PUBLISH_KEYS: &[&str] = &[
    "committed",
    "committedAt",
    "contractVersion",
    "projectionIdentitySha256",
    "projectionKind",
    "projectionSha256",
    "publishReceiptSha256",
    "revisionReceiptSha256",
];
const BATCH_PUBLISH_KEYS: &[&str] = &[
    "committed",
    "committedAt",
    "contractVersion",
    "projectionIdentitySha256",
    "projectionKind",
    "projectionSha256",
    "publishReceiptSha256",
    "revisionReceiptSha256s",
];
const SUCCESSOR_KEYS: &[&str] = &[
    "archiveCommitOrdinal",
    "contractVersion",
    "frontierSha256",
    "predecessorFrontierSha256",
    "receiptSha256",
];
const BATCH_GROUP_KEYS: &[&str] = &[
    "contractVersion",
    "projectionIdentitySha256",
    "receipts",
    "successors",
];

impl<P: ArchiveWirePrimitives> ArchiveWireCodec<P> {
    /// decode a parsed database commit receipt
    pub fn decode_revision_commit_value(
        &self,
        value: &JsonValue,
    ) -> Option<AdjustmentRevisionCommitReceipt> {
        decode_commit_receipt(self, value)
    }

    /// decode a parsed revision stage receipt
    pub fn decode_revision_stage_value(
        &self,
        value: &JsonValue,
    ) -> Option<AdjustmentRevisionStageReceipt> {
        decode_stage_receipt(self, value)
    }

    /// decode a parsed revision publication
    pub fn decode_revision_publish_value(
        &self,
        value: &JsonValue,
    ) -> Option<AdjustmentRevisionPublishReceipt> {
        decode_publish_receipt(self, value)
    }

    /// decode a parsed batch publication
    pub fn decode_revision_batch_publish_value(
        &self,
        value: &JsonValue,
    ) -> Option<AdjustmentRevisionBatchPublishReceipt> {
        decode_batch_publish_receipt(self, value)
    }
}

impl<P: ArchiveWirePrimitives> PrivateRevisionReceiptCodec for ArchiveWireCodec<P> {
    /// Decode one canonical stage receipt and authenticate its unsigned body.
    fn decode_stage_receipt(&self, bytes: &[u8]) -> Option<AdjustmentRevisionStageReceipt> {
        let value = self.decode_canonical(bytes)?;
        decode_stage_receipt(self, &value)
    }

    /// Encode the exact unsigned stage receipt shape.
    fn stage_unsigned_bytes(&self, value: PrivateRevisionStageUnsigned<'_>) -> Option<Vec<u8>> {
        self.encode(&stage_unsigned_value(value))
    }

    /// Encode only a fully authenticated stage receipt.
    fn stage_receipt_bytes(&self, value: &AdjustmentRevisionStageReceipt) -> Option<Vec<u8>> {
        let json = stage_receipt_value(value);
        let decoded = decode_stage_receipt(self, &json)?;
        // reject typed values that differ from the generated literal projection
        if decoded != *value {
            return None;
        }
        self.encode(&json)
    }

    /// Validate the closed database receipt fields before storage access.
    fn validate_revision_commit_receipt(&self, value: &AdjustmentRevisionCommitReceipt) -> bool {
        valid_commit_receipt(self, value)
    }

    /// Recompute the database receipt and frontier identities.
    fn validate_revision_commit_receipt_identity(
        &self,
        value: &AdjustmentRevisionCommitReceipt,
    ) -> bool {
        valid_commit_receipt_identity(self, value)
    }

    /// Encode only an authenticated database receipt.
    fn revision_commit_receipt_bytes(
        &self,
        value: &AdjustmentRevisionCommitReceipt,
    ) -> Option<Vec<u8>> {
        // reject structurally valid but unauthenticated database metadata
        if !valid_commit_receipt_identity(self, value) {
            return None;
        }
        self.encode(&commit_receipt_value(value))
    }

    /// Encode the direct successor derived from an authenticated receipt.
    fn revision_successor_bytes(&self, value: &AdjustmentRevisionCommitReceipt) -> Option<Vec<u8>> {
        // prevent an invalid receipt from minting a durable successor coordinate
        if !valid_commit_receipt_identity(self, value) {
            return None;
        }
        self.encode(&successor_value(value))
    }

    /// Encode one complete authenticated grouped receipt chain.
    fn revision_batch_commit_group_bytes(
        &self,
        values: &[AdjustmentRevisionCommitReceipt],
    ) -> Option<Vec<u8>> {
        validate_batch_receipts(self, values)?;
        let first = values.first()?;
        let value = JsonValue::Object(vec![
            member("contractVersion", string(BATCH_COMMIT_GROUP_VERSION)),
            member(
                "projectionIdentitySha256",
                string(&first.projectionIdentitySha256),
            ),
            member(
                "receipts",
                JsonValue::Array(values.iter().map(commit_receipt_value).collect()),
            ),
            member(
                "successors",
                JsonValue::Array(values.iter().map(successor_value).collect()),
            ),
        ]);
        self.encode(&value)
    }

    /// Decode one canonical single publication and authenticate its unsigned body.
    fn decode_publish_receipt(&self, bytes: &[u8]) -> Option<AdjustmentRevisionPublishReceipt> {
        let value = self.decode_canonical(bytes)?;
        decode_publish_receipt(self, &value)
    }

    /// Encode the exact unsigned single-publication receipt shape.
    fn publish_unsigned_bytes(&self, value: PrivateRevisionPublishUnsigned<'_>) -> Option<Vec<u8>> {
        self.encode(&publish_unsigned_value(value))
    }

    /// Encode only a fully authenticated single publication receipt.
    fn publish_receipt_bytes(&self, value: &AdjustmentRevisionPublishReceipt) -> Option<Vec<u8>> {
        let json = publish_receipt_value(value);
        let decoded = decode_publish_receipt(self, &json)?;
        // reject typed values that differ from the generated literal projection
        if decoded != *value {
            return None;
        }
        self.encode(&json)
    }

    /// Decode one canonical grouped publication and authenticate its unsigned body.
    fn decode_batch_publish_receipt(
        &self,
        bytes: &[u8],
    ) -> Option<AdjustmentRevisionBatchPublishReceipt> {
        let value = self.decode_canonical(bytes)?;
        decode_batch_publish_receipt(self, &value)
    }

    /// Encode the exact unsigned grouped-publication receipt shape.
    fn batch_publish_unsigned_bytes(
        &self,
        value: PrivateRevisionBatchPublishUnsigned<'_>,
    ) -> Option<Vec<u8>> {
        self.encode(&batch_publish_unsigned_value(value))
    }

    /// Encode only a fully authenticated grouped publication receipt.
    fn batch_publish_receipt_bytes(
        &self,
        value: &AdjustmentRevisionBatchPublishReceipt,
    ) -> Option<Vec<u8>> {
        let json = batch_publish_receipt_value(value);
        let decoded = decode_batch_publish_receipt(self, &json)?;
        // reject typed values that differ from the generated literal projection
        if decoded != *value {
            return None;
        }
        self.encode(&json)
    }
}

impl<P: ArchiveWirePrimitives> NativeRevisionColdGroupCodec for ArchiveWireCodec<P> {
    /// Decode one canonical authenticated database receipt.
    fn admitted_commit_receipt(&self, bytes: &[u8]) -> Option<NativeRevisionColdReceipt> {
        let value = self.decode_canonical(bytes)?;
        let receipt = decode_commit_receipt(self, &value)?;
        cold_receipt(&receipt, bytes.to_vec())
    }

    /// Decode one canonical direct successor.
    fn admitted_successor(&self, bytes: &[u8]) -> Option<NativeRevisionColdSuccessor> {
        let value = self.decode_canonical(bytes)?;
        decode_successor(&value, bytes.to_vec())
    }

    /// Decode and bind every member of one canonical grouped commit chain.
    fn admitted_batch_commit_group(&self, bytes: &[u8]) -> Option<NativeRevisionColdBatchGroup> {
        let value = self.decode_canonical(bytes)?;
        // retain only the closed grouped representation
        if !exact_keys(&value, BATCH_GROUP_KEYS)
            || text(&value, "contractVersion")? != BATCH_COMMIT_GROUP_VERSION
        {
            return None;
        }
        let projection_identity = hash(&value, "projectionIdentitySha256")?;
        let receipt_values = array(&value, "receipts")?;
        let successor_values = array(&value, "successors")?;
        // preserve the bounded parallel arrays from the Node parser
        if receipt_values.is_empty()
            || receipt_values.len() > PRIVATE_REVISION_BATCH_MAXIMUM_RECEIPTS
            || receipt_values.len() != successor_values.len()
        {
            return None;
        }
        let mut typed_receipts = Vec::with_capacity(receipt_values.len());
        let mut receipts = Vec::with_capacity(receipt_values.len());
        let mut successors = Vec::with_capacity(successor_values.len());
        // decode every member through the same individual grammars
        for (receipt_value, successor_value) in receipt_values.iter().zip(successor_values) {
            let receipt = decode_commit_receipt(self, receipt_value)?;
            let receipt_bytes = self.encode(receipt_value)?;
            let successor_bytes = self.encode(successor_value)?;
            let successor = decode_successor(successor_value, successor_bytes)?;
            receipts.push(cold_receipt(&receipt, receipt_bytes)?);
            typed_receipts.push(receipt);
            successors.push(successor);
        }
        validate_batch_receipts(self, &typed_receipts)?;
        // bind every supplied successor to its corresponding receipt
        if !successors
            .iter()
            .zip(&receipts)
            .all(|(successor, receipt)| successor_matches(successor, receipt))
            || typed_receipts
                .first()
                .is_none_or(|first| first.projectionIdentitySha256 != projection_identity)
        {
            return None;
        }
        Some(NativeRevisionColdBatchGroup {
            projection_identity_sha256: RevisionGapSha256::from_lower_hex(&projection_identity)?,
            receipts,
            successors,
        })
    }
}

/// Decode one exact authenticated database receipt from a parsed JSON value.
pub(crate) fn decode_commit_receipt(
    codec: &impl RevisionArchiveCanonicalCodec,
    value: &JsonValue,
) -> Option<AdjustmentRevisionCommitReceipt> {
    let receipt = map_commit_receipt(codec, value)?;
    valid_commit_receipt_identity(codec, &receipt).then_some(receipt)
}

/// project a typed database receipt
pub fn commit_receipt_value(value: &AdjustmentRevisionCommitReceipt) -> JsonValue {
    JsonValue::Object(vec![
        member("archiveCommitOrdinal", string(&value.archiveCommitOrdinal)),
        member("archiveCommittedAt", string(&value.archiveCommittedAt)),
        member(
            "contractVersion",
            string(AdjustmentRevisionCommitReceiptContractVersionLiteral::WIRE_VALUE),
        ),
        member("frontierSha256", string(&value.frontierSha256)),
        member(
            "predecessorFrontierSha256",
            string(&value.predecessorFrontierSha256),
        ),
        member(
            "projectionIdentitySha256",
            string(&value.projectionIdentitySha256),
        ),
        member(
            "projectionKind",
            string(commit_kind_wire(value.projectionKind)),
        ),
        member("projectionSha256", string(&value.projectionSha256)),
        member("receiptSha256", string(&value.receiptSha256)),
        member("stageReceiptSha256", string(&value.stageReceiptSha256)),
    ])
}

/// Project the direct successor representation derived from one receipt.
pub(crate) fn successor_value(value: &AdjustmentRevisionCommitReceipt) -> JsonValue {
    JsonValue::Object(vec![
        member("archiveCommitOrdinal", string(&value.archiveCommitOrdinal)),
        member("contractVersion", string(SUCCESSOR_VERSION)),
        member("frontierSha256", string(&value.frontierSha256)),
        member(
            "predecessorFrontierSha256",
            string(&value.predecessorFrontierSha256),
        ),
        member("receiptSha256", string(&value.receiptSha256)),
    ])
}

/// Project one typed stage receipt for archive and shadow cold-entry reuse.
pub(crate) fn stage_receipt_value(value: &AdjustmentRevisionStageReceipt) -> JsonValue {
    JsonValue::Object(vec![
        member(
            "contractVersion",
            string(AdjustmentRevisionStageReceiptContractVersionLiteral::WIRE_VALUE),
        ),
        member(
            "durable",
            JsonValue::Boolean(AdjustmentRevisionStageReceiptDurableLiteral::WIRE_VALUE),
        ),
        member("durableAt", string(&value.durableAt)),
        member(
            "projectionIdentitySha256",
            string(&value.projectionIdentitySha256),
        ),
        member(
            "projectionKind",
            string(projection_kind_wire(value.projectionKind)),
        ),
        member("projectionSha256", string(&value.projectionSha256)),
        member("stageReceiptSha256", string(&value.stageReceiptSha256)),
    ])
}

/// project a typed revision stage receipt
pub fn revision_stage_receipt_value(value: &AdjustmentRevisionStageReceipt) -> JsonValue {
    stage_receipt_value(value)
}

/// Project one typed single publication receipt for cold-entry reuse.
pub(crate) fn publish_receipt_value(value: &AdjustmentRevisionPublishReceipt) -> JsonValue {
    JsonValue::Object(vec![
        member(
            "committed",
            JsonValue::Boolean(AdjustmentRevisionPublishReceiptCommittedLiteral::WIRE_VALUE),
        ),
        member("committedAt", string(&value.committedAt)),
        member(
            "contractVersion",
            string(AdjustmentRevisionPublishReceiptContractVersionLiteral::WIRE_VALUE),
        ),
        member(
            "projectionIdentitySha256",
            string(&value.projectionIdentitySha256),
        ),
        member(
            "projectionKind",
            string(projection_kind_wire(value.projectionKind)),
        ),
        member("projectionSha256", string(&value.projectionSha256)),
        member("publishReceiptSha256", string(&value.publishReceiptSha256)),
        member(
            "revisionReceiptSha256",
            string(&value.revisionReceiptSha256),
        ),
    ])
}

/// Project one typed grouped publication receipt for cold-entry reuse.
pub(crate) fn batch_publish_receipt_value(
    value: &AdjustmentRevisionBatchPublishReceipt,
) -> JsonValue {
    JsonValue::Object(vec![
        member(
            "committed",
            JsonValue::Boolean(AdjustmentRevisionBatchPublishReceiptCommittedLiteral::WIRE_VALUE),
        ),
        member("committedAt", string(&value.committedAt)),
        member(
            "contractVersion",
            string(AdjustmentRevisionBatchPublishReceiptContractVersionLiteral::WIRE_VALUE),
        ),
        member(
            "projectionIdentitySha256",
            string(&value.projectionIdentitySha256),
        ),
        member(
            "projectionKind",
            string(batch_kind_wire(value.projectionKind)),
        ),
        member("projectionSha256", string(&value.projectionSha256)),
        member("publishReceiptSha256", string(&value.publishReceiptSha256)),
        member(
            "revisionReceiptSha256s",
            JsonValue::Array(
                value
                    .revisionReceiptSha256s
                    .iter()
                    .map(|value| string(value))
                    .collect(),
            ),
        ),
    ])
}

/// Decode the closed commit receipt fields before identity recomputation.
fn map_commit_receipt(
    codec: &impl RevisionArchiveCanonicalCodec,
    value: &JsonValue,
) -> Option<AdjustmentRevisionCommitReceipt> {
    // require the original source's complete key set
    if !exact_keys(value, COMMIT_KEYS)
        || text(value, "contractVersion")?
            != AdjustmentRevisionCommitReceiptContractVersionLiteral::WIRE_VALUE
    {
        return None;
    }
    let ordinal = uint64(value, "archiveCommitOrdinal")?;
    let archive_committed_at = text(value, "archiveCommittedAt")?;
    // reject the reserved genesis ordinal and noncanonical database clock
    if ordinal == 0 || instant(codec, &archive_committed_at).is_none() {
        return None;
    }
    let projection_kind = decode_commit_kind(&text(value, "projectionKind")?)?;
    let projection_identity_sha256 = hash(value, "projectionIdentitySha256")?;
    let projection_sha256 = hash(value, "projectionSha256")?;
    // retain the shadow receipt's distinct body grammar only
    if projection_kind != AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction
        && projection_sha256 != projection_identity_sha256
    {
        return None;
    }
    Some(AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: ordinal.to_string(),
        archiveCommittedAt: archive_committed_at,
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: hash(value, "frontierSha256")?,
        predecessorFrontierSha256: hash(value, "predecessorFrontierSha256")?,
        projectionIdentitySha256: projection_identity_sha256,
        projectionKind: projection_kind,
        projectionSha256: projection_sha256,
        receiptSha256: hash(value, "receiptSha256")?,
        stageReceiptSha256: hash(value, "stageReceiptSha256")?,
    })
}

/// Validate the typed structural database receipt grammar.
fn valid_commit_receipt(
    codec: &impl RevisionArchiveCanonicalCodec,
    value: &AdjustmentRevisionCommitReceipt,
) -> bool {
    let ordinal = value.archiveCommitOrdinal.parse::<u64>();
    let canonical_ordinal = ordinal
        .as_ref()
        .is_ok_and(|ordinal| *ordinal > 0 && ordinal.to_string() == value.archiveCommitOrdinal);
    let hashes = [
        &value.frontierSha256,
        &value.predecessorFrontierSha256,
        &value.projectionIdentitySha256,
        &value.projectionSha256,
        &value.receiptSha256,
        &value.stageReceiptSha256,
    ];
    value.contractVersion == AdjustmentRevisionCommitReceiptContractVersionLiteral
        && canonical_ordinal
        && instant(codec, &value.archiveCommittedAt).is_some()
        && hashes
            .into_iter()
            .all(|value| RevisionGapSha256::from_lower_hex(value).is_some())
        && (value.projectionKind
            == AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction
            || value.projectionSha256 == value.projectionIdentitySha256)
}

/// Recompute both linked identities from the exact Node preimages.
fn valid_commit_receipt_identity(
    codec: &impl RevisionArchiveCanonicalCodec,
    value: &AdjustmentRevisionCommitReceipt,
) -> bool {
    // reject malformed fields before constructing either identity preimage
    if !valid_commit_receipt(codec, value) {
        return false;
    }
    let receipt_preimage = [
        AdjustmentRevisionCommitReceiptContractVersionLiteral::WIRE_VALUE,
        &value.archiveCommitOrdinal,
        &value.archiveCommittedAt,
        commit_kind_wire(value.projectionKind),
        &value.projectionIdentitySha256,
        &value.projectionSha256,
        &value.stageReceiptSha256,
        &value.predecessorFrontierSha256,
    ]
    .join("\n");
    let receipt_sha256 = lower_hex(codec.sha256(receipt_preimage.as_bytes()));
    let frontier_preimage = [
        "adjustment-revision-frontier/v1",
        &value.predecessorFrontierSha256,
        &value.archiveCommitOrdinal,
        &receipt_sha256,
    ]
    .join("\n");
    value.receiptSha256 == receipt_sha256
        && value.frontierSha256 == lower_hex(codec.sha256(frontier_preimage.as_bytes()))
}

/// Decode and authenticate one stage receipt value.
pub(crate) fn decode_stage_receipt<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<AdjustmentRevisionStageReceipt> {
    // retain the exact closed stage grammar
    if !exact_keys(value, STAGE_KEYS)
        || text(value, "contractVersion")?
            != AdjustmentRevisionStageReceiptContractVersionLiteral::WIRE_VALUE
        || bool_value(value, "durable")? != AdjustmentRevisionStageReceiptDurableLiteral::WIRE_VALUE
    {
        return None;
    }
    let receipt = AdjustmentRevisionStageReceipt {
        contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
        durable: AdjustmentRevisionStageReceiptDurableLiteral,
        durableAt: text(value, "durableAt")?,
        projectionIdentitySha256: hash(value, "projectionIdentitySha256")?,
        projectionKind: decode_projection_kind(&text(value, "projectionKind")?)?,
        projectionSha256: hash(value, "projectionSha256")?,
        stageReceiptSha256: hash(value, "stageReceiptSha256")?,
    };
    let unsigned = PrivateRevisionStageUnsigned {
        durable_at: &receipt.durableAt,
        projection_identity_sha256: &receipt.projectionIdentitySha256,
        projection_kind: receipt.projectionKind,
        projection_sha256: &receipt.projectionSha256,
    };
    let JsonValue::Object(_) = value else {
        return None;
    };
    // authenticate both the canonical clock and exact unsigned receipt body
    if instant(codec, &receipt.durableAt).is_none()
        || receipt.stageReceiptSha256
            != canonical_value_sha256(codec, &stage_unsigned_value(unsigned))?
    {
        return None;
    }
    Some(receipt)
}

/// Decode and authenticate one single publication receipt value.
fn decode_publish_receipt<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<AdjustmentRevisionPublishReceipt> {
    // retain the exact closed publication grammar
    if !exact_keys(value, PUBLISH_KEYS)
        || text(value, "contractVersion")?
            != AdjustmentRevisionPublishReceiptContractVersionLiteral::WIRE_VALUE
        || bool_value(value, "committed")?
            != AdjustmentRevisionPublishReceiptCommittedLiteral::WIRE_VALUE
    {
        return None;
    }
    let receipt = AdjustmentRevisionPublishReceipt {
        committed: AdjustmentRevisionPublishReceiptCommittedLiteral,
        committedAt: text(value, "committedAt")?,
        contractVersion: AdjustmentRevisionPublishReceiptContractVersionLiteral,
        projectionIdentitySha256: hash(value, "projectionIdentitySha256")?,
        projectionKind: decode_projection_kind(&text(value, "projectionKind")?)?,
        projectionSha256: hash(value, "projectionSha256")?,
        publishReceiptSha256: hash(value, "publishReceiptSha256")?,
        revisionReceiptSha256: hash(value, "revisionReceiptSha256")?,
    };
    let unsigned = PrivateRevisionPublishUnsigned {
        committed_at: &receipt.committedAt,
        projection_identity_sha256: &receipt.projectionIdentitySha256,
        projection_kind: receipt.projectionKind,
        projection_sha256: &receipt.projectionSha256,
        revision_receipt_sha256: &receipt.revisionReceiptSha256,
    };
    // authenticate both the canonical clock and exact unsigned receipt body
    if instant(codec, &receipt.committedAt).is_none()
        || receipt.publishReceiptSha256
            != canonical_value_sha256(codec, &publish_unsigned_value(unsigned))?
    {
        return None;
    }
    Some(receipt)
}

/// Decode and authenticate one grouped publication receipt value.
fn decode_batch_publish_receipt<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<AdjustmentRevisionBatchPublishReceipt> {
    // retain the exact closed grouped publication grammar
    if !exact_keys(value, BATCH_PUBLISH_KEYS)
        || text(value, "contractVersion")?
            != AdjustmentRevisionBatchPublishReceiptContractVersionLiteral::WIRE_VALUE
        || bool_value(value, "committed")?
            != AdjustmentRevisionBatchPublishReceiptCommittedLiteral::WIRE_VALUE
    {
        return None;
    }
    let receipt_hashes = array(value, "revisionReceiptSha256s")?;
    // reject the parser's impossible empty input and the producer's batch bound
    if receipt_hashes.is_empty() || receipt_hashes.len() > PRIVATE_REVISION_BATCH_MAXIMUM_RECEIPTS {
        return None;
    }
    let mut revision_receipt_sha256s = Vec::with_capacity(receipt_hashes.len());
    // retain every receipt identity in global frontier order
    for receipt_hash in receipt_hashes {
        let JsonValue::String(units) = receipt_hash else {
            return None;
        };
        let receipt_hash = String::from_utf16(units).ok()?;
        RevisionGapSha256::from_lower_hex(&receipt_hash)?;
        revision_receipt_sha256s.push(receipt_hash);
    }
    let receipt = AdjustmentRevisionBatchPublishReceipt {
        committed: AdjustmentRevisionBatchPublishReceiptCommittedLiteral,
        committedAt: text(value, "committedAt")?,
        contractVersion: AdjustmentRevisionBatchPublishReceiptContractVersionLiteral,
        projectionIdentitySha256: hash(value, "projectionIdentitySha256")?,
        projectionKind: decode_batch_kind(&text(value, "projectionKind")?)?,
        projectionSha256: hash(value, "projectionSha256")?,
        publishReceiptSha256: hash(value, "publishReceiptSha256")?,
        revisionReceiptSha256s: revision_receipt_sha256s,
    };
    let unsigned = PrivateRevisionBatchPublishUnsigned {
        committed_at: &receipt.committedAt,
        projection_identity_sha256: &receipt.projectionIdentitySha256,
        projection_kind: receipt.projectionKind,
        projection_sha256: &receipt.projectionSha256,
        revision_receipt_sha256s: &receipt.revisionReceiptSha256s,
    };
    // authenticate both the canonical clock and exact unsigned receipt body
    if instant(codec, &receipt.committedAt).is_none()
        || receipt.publishReceiptSha256
            != canonical_value_sha256(codec, &batch_publish_unsigned_value(unsigned))?
    {
        return None;
    }
    Some(receipt)
}

/// Construct the unsigned stage receipt value in the producer field grammar.
fn stage_unsigned_value(value: PrivateRevisionStageUnsigned<'_>) -> JsonValue {
    JsonValue::Object(vec![
        member(
            "contractVersion",
            string(AdjustmentRevisionStageReceiptContractVersionLiteral::WIRE_VALUE),
        ),
        member(
            "durable",
            JsonValue::Boolean(AdjustmentRevisionStageReceiptDurableLiteral::WIRE_VALUE),
        ),
        member("durableAt", string(value.durable_at)),
        member(
            "projectionIdentitySha256",
            string(value.projection_identity_sha256),
        ),
        member(
            "projectionKind",
            string(projection_kind_wire(value.projection_kind)),
        ),
        member("projectionSha256", string(value.projection_sha256)),
    ])
}

/// Construct the unsigned single publication value.
fn publish_unsigned_value(value: PrivateRevisionPublishUnsigned<'_>) -> JsonValue {
    JsonValue::Object(vec![
        member(
            "committed",
            JsonValue::Boolean(AdjustmentRevisionPublishReceiptCommittedLiteral::WIRE_VALUE),
        ),
        member("committedAt", string(value.committed_at)),
        member(
            "contractVersion",
            string(AdjustmentRevisionPublishReceiptContractVersionLiteral::WIRE_VALUE),
        ),
        member(
            "projectionIdentitySha256",
            string(value.projection_identity_sha256),
        ),
        member(
            "projectionKind",
            string(projection_kind_wire(value.projection_kind)),
        ),
        member("projectionSha256", string(value.projection_sha256)),
        member(
            "revisionReceiptSha256",
            string(value.revision_receipt_sha256),
        ),
    ])
}

/// Construct the unsigned grouped publication value.
fn batch_publish_unsigned_value(value: PrivateRevisionBatchPublishUnsigned<'_>) -> JsonValue {
    JsonValue::Object(vec![
        member(
            "committed",
            JsonValue::Boolean(AdjustmentRevisionBatchPublishReceiptCommittedLiteral::WIRE_VALUE),
        ),
        member("committedAt", string(value.committed_at)),
        member(
            "contractVersion",
            string(AdjustmentRevisionBatchPublishReceiptContractVersionLiteral::WIRE_VALUE),
        ),
        member(
            "projectionIdentitySha256",
            string(value.projection_identity_sha256),
        ),
        member(
            "projectionKind",
            string(batch_kind_wire(value.projection_kind)),
        ),
        member("projectionSha256", string(value.projection_sha256)),
        member(
            "revisionReceiptSha256s",
            JsonValue::Array(
                value
                    .revision_receipt_sha256s
                    .iter()
                    .map(|value| string(value))
                    .collect(),
            ),
        ),
    ])
}

/// Hash one canonical JSON value through the shared runtime primitives.
fn canonical_value_sha256<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<String> {
    codec
        .encode(value)
        .map(|bytes| lower_hex(codec.sha256(&bytes)))
}

/// Validate one typed grouped commit chain before encoding or returning it.
fn validate_batch_receipts(
    codec: &impl RevisionArchiveCanonicalCodec,
    values: &[AdjustmentRevisionCommitReceipt],
) -> Option<()> {
    // retain the producer's bounded nonempty grouped representation
    if values.is_empty() || values.len() > PRIVATE_REVISION_BATCH_MAXIMUM_RECEIPTS {
        return None;
    }
    let first = values.first()?;
    // restrict grouped storage to the two canonical weather projection kinds
    if !matches!(
        first.projectionKind,
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch
            | AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision
    ) || first.projectionIdentitySha256 != first.projectionSha256
    {
        return None;
    }
    let mut prior: Option<&AdjustmentRevisionCommitReceipt> = None;
    // bind every receipt to one body, stage, and direct global chain
    for receipt in values {
        if !valid_commit_receipt_identity(codec, receipt)
            || receipt.projectionIdentitySha256 != first.projectionIdentitySha256
            || receipt.projectionSha256 != first.projectionSha256
            || receipt.projectionKind != first.projectionKind
            || receipt.stageReceiptSha256 != first.stageReceiptSha256
        {
            return None;
        }
        // enforce direct frontier and ordinal succession after the first receipt
        if let Some(prior) = prior {
            let ordinal = prior
                .archiveCommitOrdinal
                .parse::<u64>()
                .ok()?
                .checked_add(1)?;
            if receipt.predecessorFrontierSha256 != prior.frontierSha256
                || receipt.archiveCommitOrdinal != ordinal.to_string()
            {
                return None;
            }
        }
        prior = Some(receipt);
    }
    Some(())
}

/// Convert one authenticated receipt into the cold selector representation.
fn cold_receipt(
    receipt: &AdjustmentRevisionCommitReceipt,
    canonical_bytes: Vec<u8>,
) -> Option<NativeRevisionColdReceipt> {
    Some(NativeRevisionColdReceipt {
        archive_commit_ordinal: receipt.archiveCommitOrdinal.parse().ok()?,
        frontier_sha256: RevisionGapSha256::from_lower_hex(&receipt.frontierSha256)?,
        predecessor_frontier_sha256: RevisionGapSha256::from_lower_hex(
            &receipt.predecessorFrontierSha256,
        )?,
        projection_identity_sha256: RevisionGapSha256::from_lower_hex(
            &receipt.projectionIdentitySha256,
        )?,
        receipt_sha256: RevisionGapSha256::from_lower_hex(&receipt.receiptSha256)?,
        canonical_bytes,
    })
}

/// Decode one exact direct successor value.
fn decode_successor(
    value: &JsonValue,
    canonical_bytes: Vec<u8>,
) -> Option<NativeRevisionColdSuccessor> {
    // retain the source's complete successor field grammar
    if !exact_keys(value, SUCCESSOR_KEYS) || text(value, "contractVersion")? != SUCCESSOR_VERSION {
        return None;
    }
    Some(NativeRevisionColdSuccessor {
        archive_commit_ordinal: uint64(value, "archiveCommitOrdinal")?,
        frontier_sha256: RevisionGapSha256::from_lower_hex(&hash(value, "frontierSha256")?)?,
        predecessor_frontier_sha256: RevisionGapSha256::from_lower_hex(&hash(
            value,
            "predecessorFrontierSha256",
        )?)?,
        receipt_sha256: RevisionGapSha256::from_lower_hex(&hash(value, "receiptSha256")?)?,
        canonical_bytes,
    })
}

/// Bind one supplied successor to its corresponding receipt.
fn successor_matches(
    successor: &NativeRevisionColdSuccessor,
    receipt: &NativeRevisionColdReceipt,
) -> bool {
    successor.archive_commit_ordinal == receipt.archive_commit_ordinal
        && successor.frontier_sha256 == receipt.frontier_sha256
        && successor.predecessor_frontier_sha256 == receipt.predecessor_frontier_sha256
        && successor.receipt_sha256 == receipt.receipt_sha256
}

/// Decode one commit projection kind without widening the generated enum.
fn decode_commit_kind(value: &str) -> Option<AdjustmentRevisionCommitReceiptProjectionKindEnum> {
    match value {
        "actual_best_match" => {
            Some(AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch)
        }
        "native_source" => Some(AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource),
        "rain_gate_input" => Some(AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput),
        "shadow_prediction" => {
            Some(AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction)
        }
        "target_revision" => {
            Some(AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision)
        }
        _ => None,
    }
}

/// Encode one commit projection kind with its generated wire spelling.
fn commit_kind_wire(value: AdjustmentRevisionCommitReceiptProjectionKindEnum) -> &'static str {
    match value {
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch => "actual_best_match",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource => "native_source",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput => "rain_gate_input",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction => "shadow_prediction",
        AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision => "target_revision",
    }
}

/// Decode one ordinary projection kind without accepting shadow receipts.
fn decode_projection_kind(value: &str) -> Option<AdjustmentRevisionProjectionKind> {
    match value {
        "actual_best_match" => Some(AdjustmentRevisionProjectionKind::ActualBestMatch),
        "native_source" => Some(AdjustmentRevisionProjectionKind::NativeSource),
        "rain_gate_input" => Some(AdjustmentRevisionProjectionKind::RainGateInput),
        "target_revision" => Some(AdjustmentRevisionProjectionKind::TargetRevision),
        _ => None,
    }
}

/// Encode one ordinary projection kind with its generated wire spelling.
fn projection_kind_wire(value: AdjustmentRevisionProjectionKind) -> &'static str {
    match value {
        AdjustmentRevisionProjectionKind::ActualBestMatch => "actual_best_match",
        AdjustmentRevisionProjectionKind::NativeSource => "native_source",
        AdjustmentRevisionProjectionKind::RainGateInput => "rain_gate_input",
        AdjustmentRevisionProjectionKind::TargetRevision => "target_revision",
    }
}

/// Decode one grouped projection kind.
fn decode_batch_kind(
    value: &str,
) -> Option<AdjustmentRevisionBatchPublishReceiptProjectionKindEnum> {
    match value {
        "actual_best_match" => {
            Some(AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::ActualBestMatch)
        }
        "target_revision" => {
            Some(AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::TargetRevision)
        }
        _ => None,
    }
}

/// Encode one grouped projection kind with its generated wire spelling.
fn batch_kind_wire(value: AdjustmentRevisionBatchPublishReceiptProjectionKindEnum) -> &'static str {
    match value {
        AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::ActualBestMatch => {
            "actual_best_match"
        }
        AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::TargetRevision => {
            "target_revision"
        }
    }
}
