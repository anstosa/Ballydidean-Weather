//! Concrete canonical shadow stage, capsule, and receipt wire mappings.

use crate::archive_wire::{
    ArchiveWireCodec, ArchiveWirePrimitives, bool_value, exact_keys, field, hash, member, string,
    text,
};
use crate::private_shadow_archive::{
    PrivateShadowArchiveVersion, PrivateShadowCapsuleDocument, PrivateShadowPublishReceiptIdentity,
    PrivateShadowReceiptCodec, PrivateShadowStageDocument, PrivateShadowStageReceiptIdentity,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant, lower_hex};
use crate::revision_receipt_wire::{commit_receipt_value, decode_commit_receipt, successor_value};
use weather_contracts::{
    AdjustmentMaintenancePublishReceipt, AdjustmentMaintenancePublishReceiptCommittedLiteral,
    AdjustmentMaintenancePublishReceiptContractVersionEnum, AdjustmentMaintenanceStageInput,
    AdjustmentMaintenanceStageReceipt, AdjustmentMaintenanceStageReceiptContractVersionEnum,
    AdjustmentMaintenanceStageReceiptDurableLiteral, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, FieldPresence, JsonValue,
    MaintenanceShadowPredictionMetadata,
};

const CAPSULE_MAXIMUM_BYTES: usize = 2 * 1_024 * 1_024;
const PREDICTION_BODY_MAXIMUM_BYTES: u64 = 3_500 * 1_024;

pub(crate) struct DecodedStageInput {
    comparator_sha256: Option<String>,
    pub(crate) input: AdjustmentMaintenanceStageInput,
    pub(crate) version: PrivateShadowArchiveVersion,
}

impl<P: ArchiveWirePrimitives> ArchiveWireCodec<P> {
    /// decode a standalone shadow stage receipt
    pub fn decode_shadow_stage_value(
        &self,
        value: &JsonValue,
    ) -> Option<AdjustmentMaintenanceStageReceipt> {
        parse_stage_receipt(self, value).map(|(_, receipt)| receipt)
    }

    /// decode a parsed shadow publication
    pub fn decode_shadow_publish_value(
        &self,
        value: &JsonValue,
    ) -> Option<AdjustmentMaintenancePublishReceipt> {
        parse_publish_receipt(self, value)
    }
}

impl<P: ArchiveWirePrimitives> PrivateShadowReceiptCodec for ArchiveWireCodec<P> {
    /// Re-run the closed revision-receipt parser over the typed value.
    fn admit_revision_receipt(&self, receipt: &AdjustmentRevisionCommitReceipt) -> bool {
        decode_commit_receipt(self, &commit_receipt_value(receipt)).as_ref() == Some(receipt)
    }

    /// Admit one canonical database clock inside the producer's inclusive interval.
    fn admit_prediction_commit_time(
        &self,
        committed_at: &str,
        issued_at: &str,
        archive_committed_at: &str,
    ) -> bool {
        let (Some(committed), Some(issued), Some(archived)) = (
            instant(self, committed_at),
            instant(self, issued_at),
            instant(self, archive_committed_at),
        ) else {
            return false;
        };
        issued <= committed && committed <= archived
    }

    /// Preserve the producer's insertion order for the stage identity hash.
    fn stage_receipt_identity_bytes(
        &self,
        identity: &PrivateShadowStageReceiptIdentity,
    ) -> Option<Vec<u8>> {
        let version = stage_contract_version(identity.contract_version);
        // require the comparator member only in the additive grammar
        if matches!(version, PrivateShadowArchiveVersion::V1)
            != identity.comparator_sha256.is_none()
        {
            return None;
        }
        let mut fields = vec![
            member("contractVersion", string(stage_contract_text(version))),
            member("durable", JsonValue::Boolean(true)),
            member("durableAt", string(&identity.durable_at)),
            member("dueKey", string(&identity.due_key)),
            member(
                "predictionBodySha256",
                string(&identity.prediction_body_sha256),
            ),
            member("registrationSha256", string(&identity.registration_sha256)),
            member(
                "sourceProjectionSha256",
                string(&identity.source_projection_sha256),
            ),
        ];
        // append the v2 field after every legacy identity member
        if let Some(comparator) = &identity.comparator_sha256 {
            fields.push(member("comparatorSha256", string(comparator)));
        }
        self.encode_ordered(&JsonValue::Object(fields))
    }

    /// Encode only a stage document accepted by the same closed parser.
    fn encode_stage_document(&self, document: &PrivateShadowStageDocument) -> Option<Vec<u8>> {
        let bytes = self.encode(&stage_document_value(self, document)?)?;
        (self.decode_stage_document(&bytes)?.eq(document)).then_some(bytes)
    }

    /// Decode one recursively sorted, newline-framed stage document.
    fn decode_stage_document(&self, bytes: &[u8]) -> Option<PrivateShadowStageDocument> {
        parse_stage_document(self, &self.decode_canonical(bytes)?)
    }

    /// Encode only a capsule accepted by the same binding parser.
    fn encode_capsule_document(&self, document: &PrivateShadowCapsuleDocument) -> Option<Vec<u8>> {
        let bytes = self.encode(&capsule_document_value(self, document)?)?;
        (self.decode_capsule_document(&bytes)?.eq(document)).then_some(bytes)
    }

    /// Decode one bounded canonical capsule and rebind every retained identity.
    fn decode_capsule_document(&self, bytes: &[u8]) -> Option<PrivateShadowCapsuleDocument> {
        if !(2..=CAPSULE_MAXIMUM_BYTES).contains(&bytes.len()) {
            return None;
        }
        parse_capsule_document(self, &self.decode_canonical(bytes)?)
    }

    /// Preserve the producer's insertion order for the publication identity hash.
    fn publish_receipt_identity_bytes(
        &self,
        identity: &PrivateShadowPublishReceiptIdentity,
    ) -> Option<Vec<u8>> {
        let version = publish_contract_version(identity.contract_version);
        // require the comparator member only in the additive grammar
        if matches!(version, PrivateShadowArchiveVersion::V1)
            != identity.comparator_sha256.is_none()
        {
            return None;
        }
        let mut fields = vec![
            member("committed", JsonValue::Boolean(true)),
            member("committedAt", string(&identity.committed_at)),
            member("contractVersion", string(publish_contract_text(version))),
            member("predictionSha256", string(&identity.prediction_sha256)),
        ];
        // append the v2 field after every legacy identity member
        if let Some(comparator) = &identity.comparator_sha256 {
            fields.push(member("comparatorSha256", string(comparator)));
        }
        self.encode_ordered(&JsonValue::Object(fields))
    }

    /// Encode only a publication receipt accepted by the same closed parser.
    fn encode_publish_receipt(
        &self,
        receipt: &AdjustmentMaintenancePublishReceipt,
    ) -> Option<Vec<u8>> {
        let bytes = self.encode(&shadow_publish_receipt_value(receipt))?;
        (self.decode_publish_receipt(&bytes)?.eq(receipt)).then_some(bytes)
    }

    /// Decode one canonical publication receipt and recompute its ordered identity.
    fn decode_publish_receipt(&self, bytes: &[u8]) -> Option<AdjustmentMaintenancePublishReceipt> {
        parse_publish_receipt(self, &self.decode_canonical(bytes)?)
    }

    /// Encode one receipt only after its server identity is recomputed.
    fn encode_revision_receipt(
        &self,
        receipt: &AdjustmentRevisionCommitReceipt,
    ) -> Option<Vec<u8>> {
        self.admit_revision_receipt(receipt)
            .then(|| self.encode(&commit_receipt_value(receipt)))?
    }

    /// Encode the shared frontier-successor projection from its admitted receipt.
    fn encode_revision_successor(
        &self,
        receipt: &AdjustmentRevisionCommitReceipt,
    ) -> Option<Vec<u8>> {
        self.admit_revision_receipt(receipt)
            .then(|| self.encode(&successor_value(receipt)))?
    }
}

/// map a typed shadow stage receipt
pub fn shadow_stage_receipt_value(receipt: &AdjustmentMaintenanceStageReceipt) -> JsonValue {
    let mut fields = vec![
        member(
            "contractVersion",
            string(stage_contract_text(stage_contract_version(
                receipt.contractVersion,
            ))),
        ),
        member("durable", JsonValue::Boolean(true)),
        member("durableAt", string(&receipt.durableAt)),
        member("dueKey", string(&receipt.dueKey)),
        member(
            "predictionBodySha256",
            string(&receipt.predictionBodySha256),
        ),
        member("registrationSha256", string(&receipt.registrationSha256)),
        member(
            "sourceProjectionSha256",
            string(&receipt.sourceProjectionSha256),
        ),
    ];
    // retain comparator presence as the v2 wire discriminator
    if let FieldPresence::Value(comparator) = &receipt.comparatorSha256 {
        fields.push(member("comparatorSha256", string(comparator)));
    }
    fields.push(member(
        "stageReceiptSha256",
        string(&receipt.stageReceiptSha256),
    ));
    JsonValue::Object(fields)
}

/// Map one typed publication receipt into its exact JSON object.
pub(crate) fn shadow_publish_receipt_value(
    receipt: &AdjustmentMaintenancePublishReceipt,
) -> JsonValue {
    let mut fields = vec![
        member("committed", JsonValue::Boolean(true)),
        member("committedAt", string(&receipt.committedAt)),
        member(
            "contractVersion",
            string(publish_contract_text(publish_contract_version(
                receipt.contractVersion,
            ))),
        ),
        member("predictionSha256", string(&receipt.predictionSha256)),
    ];
    // retain comparator presence as the v2 wire discriminator
    if let FieldPresence::Value(comparator) = &receipt.comparatorSha256 {
        fields.push(member("comparatorSha256", string(comparator)));
    }
    fields.push(member(
        "publishReceiptSha256",
        string(&receipt.publishReceiptSha256),
    ));
    JsonValue::Object(fields)
}

/// Decode and bind one complete canonical stage object.
fn parse_stage_document<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<PrivateShadowStageDocument> {
    if !exact_keys(value, &["contractVersion", "input", "stageReceipt"]) {
        return None;
    }
    let version = stage_document_version(&text(value, "contractVersion")?)?;
    let prepared = decode_stage_input(codec, field(value, "input")?)?;
    if prepared.version != version {
        return None;
    }
    let stage_receipt =
        decode_stage_receipt(codec, field(value, "stageReceipt")?, version, &prepared)?;
    Some(PrivateShadowStageDocument {
        input: prepared.input,
        stage_receipt,
        version,
    })
}

/// Decode and bind one complete canonical capsule object.
fn parse_capsule_document<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<PrivateShadowCapsuleDocument> {
    let version = capsule_document_version(&text(value, "contractVersion")?)?;
    let keys = match version {
        PrivateShadowArchiveVersion::V1 => &[
            "bodyBase64",
            "contractVersion",
            "metadata",
            "revisionReceipt",
            "sourceProjectionBase64",
            "sourceProjectionSha256",
            "stageReceipt",
        ][..],
        PrivateShadowArchiveVersion::V2 => &[
            "bodyBase64",
            "comparatorBase64",
            "comparatorSha256",
            "contractVersion",
            "metadata",
            "predictionCommittedAt",
            "revisionReceipt",
            "sourceProjectionBase64",
            "sourceProjectionSha256",
            "stageReceipt",
        ][..],
    };
    if !exact_keys(value, keys) {
        return None;
    }
    // borrow the retained stage fields after the closed capsule grammar has been verified
    let prepared = decode_stage_members(codec, value, version)?;
    let stage_receipt =
        decode_stage_receipt(codec, field(value, "stageReceipt")?, version, &prepared)?;
    let revision_receipt = decode_commit_receipt(codec, field(value, "revisionReceipt")?)?;
    if revision_receipt.projectionKind
        != AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction
        || revision_receipt.projectionIdentitySha256 != prepared.input.metadata.sourceReceiptSha256
        || revision_receipt.projectionSha256 != prepared.input.metadata.inputSha256
        || revision_receipt.stageReceiptSha256 != stage_receipt.stageReceiptSha256
    {
        return None;
    }
    let prediction_committed_at = match version {
        PrivateShadowArchiveVersion::V1 => FieldPresence::Missing,
        PrivateShadowArchiveVersion::V2 => {
            let committed = text(value, "predictionCommittedAt")?;
            if !codec.admit_prediction_commit_time(
                &committed,
                &prepared.input.metadata.issuedAt,
                &revision_receipt.archiveCommittedAt,
            ) || hash(value, "comparatorSha256")? != prepared.comparator_sha256.as_deref()?
            {
                return None;
            }
            FieldPresence::Value(committed)
        }
    };
    Some(PrivateShadowCapsuleDocument {
        prediction_committed_at,
        revision_receipt,
        stage_input: prepared.input,
        stage_receipt,
        version,
    })
}

/// Decode one closed stage input and bind its exact private bytes.
pub(crate) fn decode_stage_input<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<DecodedStageInput> {
    let version = if field(value, "comparatorBase64").is_some() {
        PrivateShadowArchiveVersion::V2
    } else {
        PrivateShadowArchiveVersion::V1
    };
    let keys = match version {
        PrivateShadowArchiveVersion::V1 => &[
            "bodyBase64",
            "metadata",
            "sourceProjectionBase64",
            "sourceProjectionSha256",
        ][..],
        PrivateShadowArchiveVersion::V2 => &[
            "bodyBase64",
            "comparatorBase64",
            "metadata",
            "sourceProjectionBase64",
            "sourceProjectionSha256",
        ][..],
    };
    if !exact_keys(value, keys) {
        return None;
    }
    decode_stage_members(codec, value, version)
}

/// Decode staged members directly inside the exact v2 publication envelope.
pub(crate) fn decode_shadow_publish_stage_input<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<DecodedStageInput> {
    // require the complete publication envelope before borrowing its staged values
    exact_keys(
        value,
        &[
            "bodyBase64",
            "comparatorBase64",
            "metadata",
            "revisionReceipt",
            "predictionCommittedAt",
            "sourceProjectionBase64",
            "sourceProjectionSha256",
            "stageReceipt",
        ],
    )
    .then_some(())?;
    decode_stage_members(codec, value, PrivateShadowArchiveVersion::V2)
}

/// Decode the common stage fields without copying the outer UTF-16 payload values.
fn decode_stage_members<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
    version: PrivateShadowArchiveVersion,
) -> Option<DecodedStageInput> {
    let body = codec.base64_decode(&text(value, "bodyBase64")?)?;
    let comparator = match version {
        PrivateShadowArchiveVersion::V1 => FieldPresence::Missing,
        PrivateShadowArchiveVersion::V2 => {
            FieldPresence::Value(codec.base64_decode(&text(value, "comparatorBase64")?)?)
        }
    };
    let source = codec.base64_decode(&text(value, "sourceProjectionBase64")?)?;
    let metadata = decode_shadow_metadata(codec, field(value, "metadata")?)?;
    let source_sha256 = hash(value, "sourceProjectionSha256")?;
    if source_sha256 != digest(codec, &source)
        || metadata.predictionBodySha256 != digest(codec, &body)
        || metadata.bodyByteCount != body.len() as u64
    {
        return None;
    }
    let comparator_sha256 = match &comparator {
        FieldPresence::Missing => None,
        FieldPresence::Value(bytes) => Some(digest(codec, bytes)),
    };
    Some(DecodedStageInput {
        comparator_sha256,
        input: AdjustmentMaintenanceStageInput {
            body,
            comparator,
            metadata,
            sourceProjection: source,
            sourceProjectionSha256: source_sha256,
        },
        version,
    })
}

/// Decode one exact value-free metadata object.
pub(crate) fn decode_shadow_metadata<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<MaintenanceShadowPredictionMetadata> {
    if !exact_keys(
        value,
        &[
            "bodyByteCount",
            "candidateSha256",
            "dueKey",
            "inputSha256",
            "issuedAt",
            "maxValidAt",
            "minValidAt",
            "predictionBodySha256",
            "predictionSchemaSha256",
            "predictionSha256",
            "registrationSha256",
            "rowCount",
            "sourceReceiptSha256",
            "sourceSha256",
        ],
    ) {
        return None;
    }
    let body_byte_count = safe_integer(value, "bodyByteCount")?;
    let row_count = safe_integer(value, "rowCount")?;
    let due_key = text(value, "dueKey")?;
    let issued_at = text(value, "issuedAt")?;
    let min_valid_at = text(value, "minValidAt")?;
    let max_valid_at = text(value, "maxValidAt")?;
    if !(2..=PREDICTION_BODY_MAXIMUM_BYTES).contains(&body_byte_count)
        || !(1..=168).contains(&row_count)
        || !is_due_key(&due_key)
        || instant(codec, &issued_at).is_none()
        || instant(codec, &min_valid_at).is_none()
        || instant(codec, &max_valid_at).is_none()
    {
        return None;
    }
    Some(MaintenanceShadowPredictionMetadata {
        bodyByteCount: body_byte_count,
        candidateSha256: hash(value, "candidateSha256")?,
        dueKey: due_key,
        inputSha256: hash(value, "inputSha256")?,
        issuedAt: issued_at,
        maxValidAt: max_valid_at,
        minValidAt: min_valid_at,
        predictionBodySha256: hash(value, "predictionBodySha256")?,
        predictionSchemaSha256: hash(value, "predictionSchemaSha256")?,
        predictionSha256: hash(value, "predictionSha256")?,
        registrationSha256: hash(value, "registrationSha256")?,
        rowCount: row_count,
        sourceReceiptSha256: hash(value, "sourceReceiptSha256")?,
        sourceSha256: hash(value, "sourceSha256")?,
    })
}

/// Decode and recompute one stage receipt against its exact input.
pub(crate) fn decode_stage_receipt<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
    version: PrivateShadowArchiveVersion,
    prepared: &DecodedStageInput,
) -> Option<AdjustmentMaintenanceStageReceipt> {
    let (decoded_version, receipt) = parse_stage_receipt(codec, value)?;
    // retain the request-bound grammar and every source value binding
    if decoded_version != version
        || receipt.dueKey != prepared.input.metadata.dueKey
        || receipt.predictionBodySha256 != prepared.input.metadata.predictionBodySha256
        || receipt.registrationSha256 != prepared.input.metadata.registrationSha256
        || receipt.sourceProjectionSha256 != prepared.input.sourceProjectionSha256
        || presence_option(&receipt.comparatorSha256) != prepared.comparator_sha256
    {
        return None;
    }
    Some(receipt)
}

/// decode a standalone shadow stage receipt
fn parse_stage_receipt<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<(
    PrivateShadowArchiveVersion,
    AdjustmentMaintenanceStageReceipt,
)> {
    let version = if field(value, "comparatorSha256").is_some() {
        PrivateShadowArchiveVersion::V2
    } else {
        PrivateShadowArchiveVersion::V1
    };
    let keys = match version {
        PrivateShadowArchiveVersion::V1 => &[
            "contractVersion",
            "durable",
            "durableAt",
            "dueKey",
            "predictionBodySha256",
            "registrationSha256",
            "sourceProjectionSha256",
            "stageReceiptSha256",
        ][..],
        PrivateShadowArchiveVersion::V2 => &[
            "contractVersion",
            "durable",
            "durableAt",
            "dueKey",
            "predictionBodySha256",
            "registrationSha256",
            "sourceProjectionSha256",
            "comparatorSha256",
            "stageReceiptSha256",
        ][..],
    };
    if !exact_keys(value, keys)
        || text(value, "contractVersion")?.as_str() != stage_contract_text(version)
        || !bool_value(value, "durable")?
    {
        return None;
    }
    let durable_at = text(value, "durableAt")?;
    let due_key = text(value, "dueKey")?;
    let prediction_body_sha256 = hash(value, "predictionBodySha256")?;
    let registration_sha256 = hash(value, "registrationSha256")?;
    let source_projection_sha256 = hash(value, "sourceProjectionSha256")?;
    let stage_receipt_sha256 = hash(value, "stageReceiptSha256")?;
    let comparator_sha256 = match version {
        PrivateShadowArchiveVersion::V1 => FieldPresence::Missing,
        PrivateShadowArchiveVersion::V2 => FieldPresence::Value(hash(value, "comparatorSha256")?),
    };
    let identity = PrivateShadowStageReceiptIdentity {
        comparator_sha256: presence_option(&comparator_sha256),
        contract_version: stage_contract(version),
        durable_at: durable_at.clone(),
        due_key: due_key.clone(),
        prediction_body_sha256: prediction_body_sha256.clone(),
        registration_sha256: registration_sha256.clone(),
        source_projection_sha256: source_projection_sha256.clone(),
    };
    let identity_bytes = codec.stage_receipt_identity_bytes(&identity)?;
    if instant(codec, &durable_at).is_none()
        || stage_receipt_sha256 != digest(codec, &identity_bytes)
    {
        return None;
    }
    Some((
        version,
        AdjustmentMaintenanceStageReceipt {
            comparatorSha256: comparator_sha256,
            contractVersion: identity.contract_version,
            durable: AdjustmentMaintenanceStageReceiptDurableLiteral,
            durableAt: durable_at,
            dueKey: due_key,
            predictionBodySha256: prediction_body_sha256,
            registrationSha256: registration_sha256,
            sourceProjectionSha256: source_projection_sha256,
            stageReceiptSha256: stage_receipt_sha256,
        },
    ))
}

/// Decode and recompute one stable publication acknowledgement.
fn parse_publish_receipt<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<AdjustmentMaintenancePublishReceipt> {
    let version = if field(value, "comparatorSha256").is_some() {
        PrivateShadowArchiveVersion::V2
    } else {
        PrivateShadowArchiveVersion::V1
    };
    let keys = match version {
        PrivateShadowArchiveVersion::V1 => &[
            "committed",
            "committedAt",
            "contractVersion",
            "predictionSha256",
            "publishReceiptSha256",
        ][..],
        PrivateShadowArchiveVersion::V2 => &[
            "committed",
            "committedAt",
            "contractVersion",
            "predictionSha256",
            "comparatorSha256",
            "publishReceiptSha256",
        ][..],
    };
    if !exact_keys(value, keys)
        || !bool_value(value, "committed")?
        || text(value, "contractVersion")?.as_str() != publish_contract_text(version)
    {
        return None;
    }
    let committed_at = text(value, "committedAt")?;
    let prediction_sha256 = hash(value, "predictionSha256")?;
    let publish_receipt_sha256 = hash(value, "publishReceiptSha256")?;
    let comparator_sha256 = match version {
        PrivateShadowArchiveVersion::V1 => FieldPresence::Missing,
        PrivateShadowArchiveVersion::V2 => FieldPresence::Value(hash(value, "comparatorSha256")?),
    };
    let identity = PrivateShadowPublishReceiptIdentity {
        committed_at: committed_at.clone(),
        comparator_sha256: presence_option(&comparator_sha256),
        contract_version: publish_contract(version),
        prediction_sha256: prediction_sha256.clone(),
    };
    let identity_bytes = codec.publish_receipt_identity_bytes(&identity)?;
    if instant(codec, &committed_at).is_none()
        || publish_receipt_sha256 != digest(codec, &identity_bytes)
    {
        return None;
    }
    Some(AdjustmentMaintenancePublishReceipt {
        committed: AdjustmentMaintenancePublishReceiptCommittedLiteral,
        committedAt: committed_at,
        comparatorSha256: comparator_sha256,
        contractVersion: identity.contract_version,
        predictionSha256: prediction_sha256,
        publishReceiptSha256: publish_receipt_sha256,
    })
}

/// Construct one canonical stage JSON value from typed fields.
fn stage_document_value<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    document: &PrivateShadowStageDocument,
) -> Option<JsonValue> {
    Some(JsonValue::Object(vec![
        member(
            "contractVersion",
            string(stage_document_contract(document.version)),
        ),
        member("input", stage_input_value(codec, &document.input)?),
        member(
            "stageReceipt",
            shadow_stage_receipt_value(&document.stage_receipt),
        ),
    ]))
}

/// Construct one canonical capsule JSON value from typed fields.
fn capsule_document_value<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    document: &PrivateShadowCapsuleDocument,
) -> Option<JsonValue> {
    let input = stage_input_value(codec, &document.stage_input)?;
    let mut fields = vec![member("bodyBase64", field(&input, "bodyBase64")?.clone())];
    // retain the three additive v2 fields only in the comparator grammar
    if matches!(document.version, PrivateShadowArchiveVersion::V2) {
        fields.push(member(
            "comparatorBase64",
            field(&input, "comparatorBase64")?.clone(),
        ));
        let FieldPresence::Value(comparator_sha256) = &document.stage_receipt.comparatorSha256
        else {
            return None;
        };
        fields.push(member("comparatorSha256", string(comparator_sha256)));
    }
    fields.push(member(
        "contractVersion",
        string(capsule_document_contract(document.version)),
    ));
    fields.push(member("metadata", field(&input, "metadata")?.clone()));
    match (&document.version, &document.prediction_committed_at) {
        (PrivateShadowArchiveVersion::V1, FieldPresence::Missing) => {}
        (PrivateShadowArchiveVersion::V2, FieldPresence::Value(committed)) => {
            fields.push(member("predictionCommittedAt", string(committed)))
        }
        _ => return None,
    }
    fields.push(member(
        "revisionReceipt",
        commit_receipt_value(&document.revision_receipt),
    ));
    fields.push(member(
        "sourceProjectionBase64",
        field(&input, "sourceProjectionBase64")?.clone(),
    ));
    fields.push(member(
        "sourceProjectionSha256",
        field(&input, "sourceProjectionSha256")?.clone(),
    ));
    fields.push(member(
        "stageReceipt",
        shadow_stage_receipt_value(&document.stage_receipt),
    ));
    Some(JsonValue::Object(fields))
}

/// Construct one stage input transport with canonical padded base64.
fn stage_input_value<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    input: &AdjustmentMaintenanceStageInput,
) -> Option<JsonValue> {
    let mut fields = vec![member(
        "bodyBase64",
        string(&codec.base64_encode(&input.body)?),
    )];
    // preserve comparator field absence as the v1 discriminator
    if let FieldPresence::Value(comparator) = &input.comparator {
        fields.push(member(
            "comparatorBase64",
            string(&codec.base64_encode(comparator)?),
        ));
    }
    fields.push(member("metadata", shadow_metadata_value(&input.metadata)));
    fields.push(member(
        "sourceProjectionBase64",
        string(&codec.base64_encode(&input.sourceProjection)?),
    ));
    fields.push(member(
        "sourceProjectionSha256",
        string(&input.sourceProjectionSha256),
    ));
    Some(JsonValue::Object(fields))
}

/// map typed shadow metadata
pub fn shadow_metadata_value(metadata: &MaintenanceShadowPredictionMetadata) -> JsonValue {
    JsonValue::Object(vec![
        member(
            "bodyByteCount",
            JsonValue::Number(metadata.bodyByteCount as f64),
        ),
        member("candidateSha256", string(&metadata.candidateSha256)),
        member("dueKey", string(&metadata.dueKey)),
        member("inputSha256", string(&metadata.inputSha256)),
        member("issuedAt", string(&metadata.issuedAt)),
        member("maxValidAt", string(&metadata.maxValidAt)),
        member("minValidAt", string(&metadata.minValidAt)),
        member(
            "predictionBodySha256",
            string(&metadata.predictionBodySha256),
        ),
        member(
            "predictionSchemaSha256",
            string(&metadata.predictionSchemaSha256),
        ),
        member("predictionSha256", string(&metadata.predictionSha256)),
        member("registrationSha256", string(&metadata.registrationSha256)),
        member("rowCount", JsonValue::Number(metadata.rowCount as f64)),
        member("sourceReceiptSha256", string(&metadata.sourceReceiptSha256)),
        member("sourceSha256", string(&metadata.sourceSha256)),
    ])
}

/// Read one nonnegative JavaScript-safe integer number.
fn safe_integer(value: &JsonValue, name: &str) -> Option<u64> {
    let JsonValue::Number(number) = field(value, name)? else {
        return None;
    };
    if !number.is_finite() || *number < 0.0 || number.fract() != 0.0 {
        return None;
    }
    let integer = *number as u64;
    (integer <= 9_007_199_254_740_991 && integer as f64 == *number).then_some(integer)
}

/// Match the frozen six-hour capture key without adding a regex package.
fn is_due_key(value: &str) -> bool {
    let Some(timestamp) = value.strip_prefix("capture/") else {
        return false;
    };
    let bytes = timestamp.as_bytes();
    if bytes.len() != 24 {
        return false;
    }
    // require digits only in the date positions
    if ![0, 1, 2, 3, 5, 6, 8, 9]
        .into_iter()
        .all(|index| bytes[index].is_ascii_digit())
    {
        return false;
    }
    bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && matches!(&bytes[11..13], b"00" | b"06" | b"12" | b"18")
        && &bytes[13..] == b":35:00.000Z"
}

/// Render one digest through the shared primitive owner.
fn digest(codec: &impl RevisionArchiveCanonicalCodec, bytes: &[u8]) -> String {
    lower_hex(codec.sha256(bytes))
}

/// Convert one generated optional field into an owned option.
fn presence_option<T: Clone>(value: &FieldPresence<T>) -> Option<T> {
    match value {
        FieldPresence::Missing => None,
        FieldPresence::Value(value) => Some(value.clone()),
    }
}

/// Resolve the generated stage contract for one grammar.
fn stage_contract(
    version: PrivateShadowArchiveVersion,
) -> AdjustmentMaintenanceStageReceiptContractVersionEnum {
    match version {
        PrivateShadowArchiveVersion::V1 => {
            AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV1
        }
        PrivateShadowArchiveVersion::V2 => {
            AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV2
        }
    }
}

/// Resolve the grammar selected by one generated stage contract.
fn stage_contract_version(
    contract: AdjustmentMaintenanceStageReceiptContractVersionEnum,
) -> PrivateShadowArchiveVersion {
    match contract {
        AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV1 => {
            PrivateShadowArchiveVersion::V1
        }
        AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV2 => {
            PrivateShadowArchiveVersion::V2
        }
    }
}

/// Render one generated stage contract.
fn stage_contract_text(version: PrivateShadowArchiveVersion) -> &'static str {
    match version {
        PrivateShadowArchiveVersion::V1 => "adjustment-shadow-stage-receipt/v1",
        PrivateShadowArchiveVersion::V2 => "adjustment-shadow-stage-receipt/v2",
    }
}

/// Resolve the generated publication contract for one grammar.
fn publish_contract(
    version: PrivateShadowArchiveVersion,
) -> AdjustmentMaintenancePublishReceiptContractVersionEnum {
    match version {
        PrivateShadowArchiveVersion::V1 => {
            AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV1
        }
        PrivateShadowArchiveVersion::V2 => {
            AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV2
        }
    }
}

/// Resolve the grammar selected by one generated publication contract.
fn publish_contract_version(
    contract: AdjustmentMaintenancePublishReceiptContractVersionEnum,
) -> PrivateShadowArchiveVersion {
    match contract {
        AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV1 => PrivateShadowArchiveVersion::V1,
        AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV2 => PrivateShadowArchiveVersion::V2,
    }
}

/// Render one generated publication contract.
fn publish_contract_text(version: PrivateShadowArchiveVersion) -> &'static str {
    match version {
        PrivateShadowArchiveVersion::V1 => "adjustment-shadow-publish-receipt/v1",
        PrivateShadowArchiveVersion::V2 => "adjustment-shadow-publish-receipt/v2",
    }
}

/// Decode one exact stage-document contract.
fn stage_document_version(value: &str) -> Option<PrivateShadowArchiveVersion> {
    match value {
        "adjustment-shadow-revision-stage/v1" => Some(PrivateShadowArchiveVersion::V1),
        "adjustment-shadow-revision-stage/v2" => Some(PrivateShadowArchiveVersion::V2),
        _ => None,
    }
}

/// Render one exact stage-document contract.
fn stage_document_contract(version: PrivateShadowArchiveVersion) -> &'static str {
    match version {
        PrivateShadowArchiveVersion::V1 => "adjustment-shadow-revision-stage/v1",
        PrivateShadowArchiveVersion::V2 => "adjustment-shadow-revision-stage/v2",
    }
}

/// Decode one exact capsule contract.
fn capsule_document_version(value: &str) -> Option<PrivateShadowArchiveVersion> {
    match value {
        "adjustment-shadow-revision-capsule/v1" => Some(PrivateShadowArchiveVersion::V1),
        "adjustment-shadow-revision-capsule/v2" => Some(PrivateShadowArchiveVersion::V2),
        _ => None,
    }
}

/// Render one exact capsule contract.
fn capsule_document_contract(version: PrivateShadowArchiveVersion) -> &'static str {
    match version {
        PrivateShadowArchiveVersion::V1 => "adjustment-shadow-revision-capsule/v1",
        PrivateShadowArchiveVersion::V2 => "adjustment-shadow-revision-capsule/v2",
    }
}
