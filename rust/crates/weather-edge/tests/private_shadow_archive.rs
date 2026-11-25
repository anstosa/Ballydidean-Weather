use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_contracts::{
    AdjustmentMaintenancePublishReceipt, AdjustmentMaintenancePublishReceiptCommittedLiteral,
    AdjustmentMaintenancePublishReceiptContractVersionEnum, AdjustmentMaintenanceStageInput,
    AdjustmentMaintenanceStageReceipt, AdjustmentMaintenanceStageReceiptContractVersionEnum,
    AdjustmentMaintenanceStageReceiptDurableLiteral, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, FieldPresence,
    MaintenanceShadowPredictionMetadata, RainMaintenanceControlStateStageReceipt,
};

use weather_edge::{
    AdjustmentMaintenanceGapReceipt, AdjustmentRevisionGapReceipt, NativeRevisionSpool,
    NativeRevisionSpoolArtifactCodec, PermanentArchiveGapCodec, PermanentArchiveGapInput,
    PrivateShadowArchive, PrivateShadowArchiveCodec, PrivateShadowArchiveMetadataKind,
    PrivateShadowArchiveStorage, PrivateShadowArchiveVersion, PrivateShadowCapsuleDocument,
    PrivateShadowExclusiveWrite, PrivateShadowPublishInput, PrivateShadowPublishReceiptIdentity,
    PrivateShadowReceiptCodec, PrivateShadowStageDocument, PrivateShadowStageReceiptIdentity,
    RainControlProjectionBinding, RainControlStateArchiveCodec, RainControlStateWireCodec,
    RainMaintenanceControlStateGapReceipt, RevisionArchiveCanonicalClock,
    RevisionArchiveCanonicalCodec, RevisionOnlinePayloadAddress, RevisionOnlinePayloadKind,
    RevisionOnlineSpoolError, RevisionOnlineSpoolStorage,
};

#[path = "support/native_spool.rs"]
mod native_spool;
use native_spool::{NodeLeaseCodec, TempSpool};

type ArchiveResult<T> = Result<T, RevisionOnlineSpoolError>;
const STAGE_AT: &str = "2026-10-08T00:36:00.000Z";
const PUBLISH_AT: &str = "2026-10-08T00:40:00.000Z";
const PREDICTION_COMMITTED_AT: &str = "2026-10-08T00:38:00.000Z";

impl RevisionArchiveCanonicalCodec for NodeLeaseCodec {
    /// No permanent-gap or rain artifact is decoded in the empty normal shadow fixture.
    fn canonical_utc_milliseconds(&self, _text: &str) -> Option<i64> {
        unreachable!("native storage artifact clock is unused")
    }

    /// No permanent-gap or rain artifact is addressed in the empty normal shadow fixture.
    fn sha256(&self, _bytes: &[u8]) -> [u8; 32] {
        unreachable!("native storage artifact digest is unused")
    }
}

impl RainControlStateWireCodec for NodeLeaseCodec {
    /// This fixture does not submit monthly rain control states.
    fn decode_control_json(&self, _: &[u8]) -> Option<weather_contracts::JsonValue> {
        None
    }

    /// No monthly state document is serialized in this fixture.
    fn encode_control_ordered_json(&self, _: &weather_contracts::JsonValue) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// No monthly state self hash is serialized in this fixture.
    fn encode_control_sorted_json(&self, _: &weather_contracts::JsonValue) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// The shadow fixture never parses a rain-state stage receipt.
    fn decode_stage_receipt(
        &self,
        _bytes: &[u8],
    ) -> Option<RainMaintenanceControlStateStageReceipt> {
        unreachable!("rain stage receipt is unrelated")
    }

    /// The shadow fixture never parses a rain-state gap receipt.
    fn decode_gap_receipt(&self, _bytes: &[u8]) -> Option<RainMaintenanceControlStateGapReceipt> {
        unreachable!("rain gap receipt is unrelated")
    }
}

impl RainControlStateArchiveCodec for NodeLeaseCodec {
    /// The shadow fixture never parses a rain-bound revision projection.
    fn decode_projection(&self, _bytes: &[u8]) -> Option<RainControlProjectionBinding> {
        unreachable!("rain projection is unrelated")
    }
}

impl PermanentArchiveGapCodec for NodeLeaseCodec {
    /// The normal shadow fixture creates no permanent-gap locator document.
    fn encode_gap_key(&self, _input: PermanentArchiveGapInput<'_>) -> Option<Vec<u8>> {
        unreachable!("permanent gap key is unrelated")
    }

    /// The normal shadow fixture creates no permanent-gap receipt document.
    fn encode_gap_receipt(
        &self,
        _input: PermanentArchiveGapInput<'_>,
        _recorded_at: &str,
        _digest: Option<&str>,
    ) -> Option<Vec<u8>> {
        unreachable!("permanent gap receipt is unrelated")
    }

    /// The normal shadow fixture starts with no permanent revision gaps.
    fn accepted_revision_gap(&self, _bytes: &[u8]) -> Option<AdjustmentRevisionGapReceipt> {
        unreachable!("revision gap directory is empty")
    }

    /// The normal shadow fixture starts with no permanent shadow gaps.
    fn accepted_shadow_gap(&self, _bytes: &[u8]) -> Option<AdjustmentMaintenanceGapReceipt> {
        unreachable!("shadow gap directory is empty")
    }
}

impl NativeRevisionSpoolArtifactCodec for NodeLeaseCodec {
    /// The normal shadow fixture never scans a rain-control ordinal artifact.
    fn admitted_rain_control_ordinal_sha256(&self, _bytes: &[u8]) -> Option<String> {
        unreachable!("rain ordinal artifact is unrelated")
    }
}

#[derive(Clone)]
struct ExpectedFixture {
    body: Vec<u8>,
    capsule_bytes: Vec<u8>,
    comparator: Option<Vec<u8>>,
    publish_bytes: Vec<u8>,
    publish_receipt: AdjustmentMaintenancePublishReceipt,
    revision_bytes: Vec<u8>,
    revision_receipt: AdjustmentRevisionCommitReceipt,
    source: Vec<u8>,
    stage_bytes: Vec<u8>,
    stage_input: AdjustmentMaintenanceStageInput,
    stage_receipt: AdjustmentMaintenanceStageReceipt,
    successor_bytes: Vec<u8>,
    version: PrivateShadowArchiveVersion,
}

struct NodeOracle {
    fields: BTreeMap<String, String>,
}

impl NodeOracle {
    /// Execute the production Node archive and retain its exact disposable artifacts.
    fn load() -> Self {
        let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/private-shadow-archive-node-oracle.mjs");
        let output = Command::new("node")
            .arg(oracle)
            .output()
            .expect("Node oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).expect("oracle UTF-8");
        let fields = text
            .lines()
            .filter_map(
                // retain one complete bounded fixture field per output line
                |line| {
                    line.split_once('=').map(
                        // own both the field name and its exact source value
                        |(key, value)| (key.to_owned(), value.to_owned()),
                    )
                },
            )
            .collect();
        Self { fields }
    }

    /// Build one generated typed fixture from the selected Node grammar.
    fn fixture(&self, version: PrivateShadowArchiveVersion) -> ExpectedFixture {
        let prefix = match version {
            PrivateShadowArchiveVersion::V1 => "v1",
            PrivateShadowArchiveVersion::V2 => "v2",
        };
        let field = |name: &str| self.fields[&format!("{prefix}_{name}")].clone();
        let body = decode_hex(&field("body_hex"));
        let source = decode_hex(&field("source_hex"));
        let comparator = match field("comparator_hex").as_str() {
            "none" => None,
            value => Some(decode_hex(value)),
        };
        let comparator_sha256 = comparator.as_ref().map(|_| field("comparator_sha256"));
        let stage_contract = match version {
            PrivateShadowArchiveVersion::V1 => {
                AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV1
            }
            PrivateShadowArchiveVersion::V2 => {
                AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV2
            }
        };
        let publish_contract = match version {
            PrivateShadowArchiveVersion::V1 => {
                AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV1
            }
            PrivateShadowArchiveVersion::V2 => {
                AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV2
            }
        };
        let metadata = MaintenanceShadowPredictionMetadata {
            bodyByteCount: body.len() as u64,
            candidateSha256: "1".repeat(64),
            dueKey: "capture/2026-10-08T00:35:00.000Z".to_owned(),
            inputSha256: "2".repeat(64),
            issuedAt: "2026-10-08T00:35:01.000Z".to_owned(),
            maxValidAt: "2026-10-08T12:00:00.000Z".to_owned(),
            minValidAt: "2026-10-08T01:00:00.000Z".to_owned(),
            predictionBodySha256: field("prediction_body_sha256"),
            predictionSchemaSha256: "3".repeat(64),
            predictionSha256: "4".repeat(64),
            registrationSha256: "5".repeat(64),
            rowCount: 12,
            sourceReceiptSha256: "6".repeat(64),
            sourceSha256: "7".repeat(64),
        };
        let stage_input = AdjustmentMaintenanceStageInput {
            body: body.clone(),
            comparator: option_presence(comparator.clone()),
            metadata: metadata.clone(),
            sourceProjection: source.clone(),
            sourceProjectionSha256: field("source_projection_sha256"),
        };
        let stage_receipt = AdjustmentMaintenanceStageReceipt {
            comparatorSha256: option_presence(comparator_sha256.clone()),
            contractVersion: stage_contract,
            durable: AdjustmentMaintenanceStageReceiptDurableLiteral,
            durableAt: STAGE_AT.to_owned(),
            dueKey: metadata.dueKey.clone(),
            predictionBodySha256: metadata.predictionBodySha256.clone(),
            registrationSha256: metadata.registrationSha256.clone(),
            sourceProjectionSha256: stage_input.sourceProjectionSha256.clone(),
            stageReceiptSha256: field("stage_receipt_sha256"),
        };
        let revision_receipt = AdjustmentRevisionCommitReceipt {
            archiveCommitOrdinal: "1".to_owned(),
            archiveCommittedAt: "2026-10-08T00:39:00.000Z".to_owned(),
            contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
            frontierSha256: field("frontier_sha256"),
            predecessorFrontierSha256: field("predecessor_frontier_sha256"),
            projectionIdentitySha256: metadata.sourceReceiptSha256.clone(),
            projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction,
            projectionSha256: metadata.inputSha256.clone(),
            receiptSha256: field("receipt_sha256"),
            stageReceiptSha256: stage_receipt.stageReceiptSha256.clone(),
        };
        let publish_receipt = AdjustmentMaintenancePublishReceipt {
            committed: AdjustmentMaintenancePublishReceiptCommittedLiteral,
            committedAt: PUBLISH_AT.to_owned(),
            comparatorSha256: option_presence(comparator_sha256),
            contractVersion: publish_contract,
            predictionSha256: metadata.predictionSha256.clone(),
            publishReceiptSha256: field("publish_receipt_sha256"),
        };
        ExpectedFixture {
            body,
            capsule_bytes: decode_hex(&field("capsule_hex")),
            comparator,
            publish_bytes: decode_hex(&field("publish_hex")),
            publish_receipt,
            revision_bytes: decode_hex(&field("revision_hex")),
            revision_receipt,
            source,
            stage_bytes: decode_hex(&field("stage_hex")),
            stage_input,
            stage_receipt,
            successor_bytes: decode_hex(&field("successor_hex")),
            version,
        }
    }
}

/// Decode fixture hex into exact production bytes.
fn decode_hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(
            // decode one fixed byte from each source pair
            |index| u8::from_str_radix(&value[index..index + 2], 16).expect("oracle hex"),
        )
        .collect()
}

/// Convert one optional test value into the generated presence shape.
fn option_presence<T>(value: Option<T>) -> FieldPresence<T> {
    match value {
        Some(value) => FieldPresence::Value(value),
        None => FieldPresence::Missing,
    }
}

#[derive(Clone)]
struct FixtureCodec {
    expected: ExpectedFixture,
}

impl RevisionArchiveCanonicalCodec for FixtureCodec {
    /// Admit only the two exact receipt clocks emitted by the Node fixture.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        match text {
            STAGE_AT => Some(1),
            PUBLISH_AT => Some(2),
            _ => None,
        }
    }

    /// Map exact source and receipt preimages to their production Node digests.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        let digest = if bytes == self.expected.body {
            &self.expected.stage_input.metadata.predictionBodySha256
        } else if bytes == self.expected.source {
            &self.expected.stage_input.sourceProjectionSha256
        } else if self
            .expected
            .comparator
            .as_ref()
            .is_some_and(|value| bytes == value)
        {
            match &self.expected.stage_receipt.comparatorSha256 {
                FieldPresence::Value(value) => value,
                FieldPresence::Missing => unreachable!("comparator digest"),
            }
        } else if bytes == b"stage-receipt-identity" {
            &self.expected.stage_receipt.stageReceiptSha256
        } else {
            assert_eq!(bytes, b"publish-receipt-identity");
            &self.expected.publish_receipt.publishReceiptSha256
        };
        decode_hex(digest).try_into().expect("digest size")
    }
}

impl PrivateShadowReceiptCodec for FixtureCodec {
    /// Admit only the actual database receipt bound into the Node capsule.
    fn admit_revision_receipt(&self, receipt: &AdjustmentRevisionCommitReceipt) -> bool {
        receipt == &self.expected.revision_receipt
    }

    /// Admit only the v2 database append clock from the ordinary source path.
    fn admit_prediction_commit_time(
        &self,
        committed_at: &str,
        issued_at: &str,
        archive_committed_at: &str,
    ) -> bool {
        committed_at == PREDICTION_COMMITTED_AT
            && issued_at == self.expected.stage_input.metadata.issuedAt
            && archive_committed_at == self.expected.revision_receipt.archiveCommittedAt
    }

    /// Bind the exact typed stage identity fields before returning the fixture preimage.
    fn stage_receipt_identity_bytes(
        &self,
        identity: &PrivateShadowStageReceiptIdentity,
    ) -> Option<Vec<u8>> {
        let expected_comparator = match &self.expected.stage_receipt.comparatorSha256 {
            FieldPresence::Missing => None,
            FieldPresence::Value(value) => Some(value.clone()),
        };
        (identity.contract_version == self.expected.stage_receipt.contractVersion
            && identity.durable_at == self.expected.stage_receipt.durableAt
            && identity.due_key == self.expected.stage_receipt.dueKey
            && identity.prediction_body_sha256 == self.expected.stage_receipt.predictionBodySha256
            && identity.registration_sha256 == self.expected.stage_receipt.registrationSha256
            && identity.source_projection_sha256
                == self.expected.stage_receipt.sourceProjectionSha256
            && identity.comparator_sha256 == expected_comparator)
            .then(|| b"stage-receipt-identity".to_vec())
    }

    /// Encode only the exact production stage document.
    fn encode_stage_document(&self, document: &PrivateShadowStageDocument) -> Option<Vec<u8>> {
        (document.input == self.expected.stage_input
            && document.stage_receipt == self.expected.stage_receipt
            && document.version == self.expected.version)
            .then(|| self.expected.stage_bytes.clone())
    }

    /// Decode only the exact production stage bytes.
    fn decode_stage_document(&self, bytes: &[u8]) -> Option<PrivateShadowStageDocument> {
        (bytes == self.expected.stage_bytes).then(|| PrivateShadowStageDocument {
            input: self.expected.stage_input.clone(),
            stage_receipt: self.expected.stage_receipt.clone(),
            version: self.expected.version,
        })
    }

    /// Encode only the exact production capsule document.
    fn encode_capsule_document(&self, document: &PrivateShadowCapsuleDocument) -> Option<Vec<u8>> {
        (document == &self.capsule_document()).then(|| self.expected.capsule_bytes.clone())
    }

    /// Decode only the exact production capsule bytes.
    fn decode_capsule_document(&self, bytes: &[u8]) -> Option<PrivateShadowCapsuleDocument> {
        (bytes == self.expected.capsule_bytes).then(|| self.capsule_document())
    }

    /// Bind the exact typed publish identity before returning the fixture preimage.
    fn publish_receipt_identity_bytes(
        &self,
        identity: &PrivateShadowPublishReceiptIdentity,
    ) -> Option<Vec<u8>> {
        let expected_comparator = match &self.expected.publish_receipt.comparatorSha256 {
            FieldPresence::Missing => None,
            FieldPresence::Value(value) => Some(value.clone()),
        };
        (identity.committed_at == self.expected.publish_receipt.committedAt
            && identity.comparator_sha256 == expected_comparator
            && identity.contract_version == self.expected.publish_receipt.contractVersion
            && identity.prediction_sha256 == self.expected.publish_receipt.predictionSha256)
            .then(|| b"publish-receipt-identity".to_vec())
    }

    /// Encode only the exact production publication receipt.
    fn encode_publish_receipt(
        &self,
        receipt: &AdjustmentMaintenancePublishReceipt,
    ) -> Option<Vec<u8>> {
        (receipt == &self.expected.publish_receipt).then(|| self.expected.publish_bytes.clone())
    }

    /// Decode only the exact production publication receipt.
    fn decode_publish_receipt(&self, bytes: &[u8]) -> Option<AdjustmentMaintenancePublishReceipt> {
        (bytes == self.expected.publish_bytes).then(|| self.expected.publish_receipt.clone())
    }

    /// Encode only the exact production database receipt.
    fn encode_revision_receipt(
        &self,
        receipt: &AdjustmentRevisionCommitReceipt,
    ) -> Option<Vec<u8>> {
        (receipt == &self.expected.revision_receipt).then(|| self.expected.revision_bytes.clone())
    }

    /// Encode only the exact production frontier successor.
    fn encode_revision_successor(
        &self,
        receipt: &AdjustmentRevisionCommitReceipt,
    ) -> Option<Vec<u8>> {
        (receipt == &self.expected.revision_receipt).then(|| self.expected.successor_bytes.clone())
    }
}

impl PrivateShadowArchiveCodec for FixtureCodec {
    /// Admit the actual producer-parsed stage selected by the Node fixture.
    fn admit_stage_input(&self, input: &AdjustmentMaintenanceStageInput) -> bool {
        input == &self.expected.stage_input
    }
}

impl FixtureCodec {
    /// Reconstruct the exact typed capsule carried by the Node bytes.
    fn capsule_document(&self) -> PrivateShadowCapsuleDocument {
        PrivateShadowCapsuleDocument {
            prediction_committed_at: match self.expected.version {
                PrivateShadowArchiveVersion::V1 => FieldPresence::Missing,
                PrivateShadowArchiveVersion::V2 => {
                    FieldPresence::Value(PREDICTION_COMMITTED_AT.to_owned())
                }
            },
            revision_receipt: self.expected.revision_receipt.clone(),
            stage_input: self.expected.stage_input.clone(),
            stage_receipt: self.expected.stage_receipt.clone(),
            version: self.expected.version,
        }
    }
}

#[derive(Default)]
struct FixtureStorage {
    content: BTreeMap<RevisionOnlinePayloadAddress, Vec<u8>>,
    held: bool,
    metadata: BTreeMap<(PrivateShadowArchiveMetadataKind, String), Vec<u8>>,
    trace: Vec<String>,
}

impl RevisionOnlineSpoolStorage for FixtureStorage {
    /// Keep one complete online admission and persistence operation under the lease.
    fn with_revision_spool_lease<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> ArchiveResult<T>,
    ) -> ArchiveResult<T> {
        assert!(!self.held);
        self.held = true;
        let result = operation(self);
        self.held = false;
        result
    }

    /// Read the exact retained candidate while the shared lease is held.
    fn read_existing_payload(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> ArchiveResult<Option<Vec<u8>>> {
        assert!(self.held);
        let value = self.content.get(address);
        assert!(value.is_none_or(|bytes| bytes.len() <= maximum_bytes));
        Ok(value.cloned())
    }

    /// Enumerate the fixed online directory selected by the spool owner.
    fn list_payload_names(
        &mut self,
        kind: RevisionOnlinePayloadKind,
    ) -> ArchiveResult<Vec<String>> {
        assert!(self.held);
        Ok(self
            .content
            .keys()
            .filter_map(
                // retain only names from the requested typed directory
                |address| {
                    let (stored_kind, name) = address.components();
                    (stored_kind == kind).then_some(name.to_owned())
                },
            )
            .collect())
    }

    /// Return the exact retained length after the storage-bound check.
    fn payload_file_length(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> ArchiveResult<usize> {
        assert!(self.held);
        let length = self.content[address].len();
        assert!(length <= maximum_bytes);
        Ok(length)
    }
}

impl PrivateShadowArchiveStorage for FixtureStorage {
    /// Verify the disposable fixture root before each source operation.
    fn ensure_revision_root(&mut self) -> ArchiveResult<()> {
        Ok(())
    }

    /// The ordinary fixture has no permanent unqualified shadow gap.
    fn has_permanent_shadow_gap(
        &mut self,
        _due_key: &str,
        _registration_sha256: &str,
    ) -> ArchiveResult<bool> {
        Ok(false)
    }

    /// Retain one new online stage under the held lease.
    fn write_online_exclusive(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> ArchiveResult<()> {
        assert!(self.held);
        self.trace.push("stage".to_owned());
        assert!(
            self.content
                .insert(address.clone(), bytes.to_vec())
                .is_none()
        );
        Ok(())
    }

    /// Read the exact staged value outside the later capsule mutation lease.
    fn read_online(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> ArchiveResult<Option<Vec<u8>>> {
        assert!(!self.held);
        let value = self.content.get(address);
        assert!(value.is_none_or(|bytes| bytes.len() <= maximum_bytes));
        Ok(value.cloned())
    }

    /// Retain or verify the complete capsule under its database receipt address.
    fn write_online_immutable(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> ArchiveResult<()> {
        assert!(self.held);
        self.trace.push("capsule".to_owned());
        // preserve exact retry bytes without replacing the first file
        if let Some(existing) = self.content.get(address) {
            assert_eq!(existing, bytes);
        } else {
            self.content.insert(address.clone(), bytes.to_vec());
        }
        Ok(())
    }

    /// Retire only the redundant stage after capsule durability.
    fn remove_online_if_present(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
    ) -> ArchiveResult<()> {
        assert!(self.held);
        self.trace.push("retire-stage".to_owned());
        self.content.remove(address);
        Ok(())
    }

    /// Record the stage-directory durability fence last inside capsule persistence.
    fn sync_online_directory(&mut self, kind: RevisionOnlinePayloadKind) -> ArchiveResult<()> {
        assert!(self.held);
        assert_eq!(kind, RevisionOnlinePayloadKind::ShadowStage);
        self.trace.push("sync-stage-directory".to_owned());
        Ok(())
    }

    /// Read one exact small metadata artifact by its typed address.
    fn read_metadata(
        &mut self,
        kind: PrivateShadowArchiveMetadataKind,
        digest: &str,
        maximum_bytes: usize,
    ) -> ArchiveResult<Option<Vec<u8>>> {
        let value = self.metadata.get(&(kind, digest.to_owned()));
        assert!(value.is_none_or(|bytes| bytes.len() <= maximum_bytes));
        Ok(value.cloned())
    }

    /// Retain or verify database receipt metadata under the capsule lease.
    fn write_metadata_immutable(
        &mut self,
        kind: PrivateShadowArchiveMetadataKind,
        digest: &str,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> ArchiveResult<()> {
        assert!(self.held);
        assert!(bytes.len() <= maximum_bytes);
        self.trace.push(kind.directory().to_owned());
        let key = (kind, digest.to_owned());
        // preserve byte-identical retry metadata
        if let Some(existing) = self.metadata.get(&key) {
            assert_eq!(existing, bytes);
        } else {
            self.metadata.insert(key, bytes.to_vec());
        }
        Ok(())
    }

    /// Retain the first publication acknowledgement outside the capsule lease.
    fn write_metadata_exclusive(
        &mut self,
        kind: PrivateShadowArchiveMetadataKind,
        digest: &str,
        bytes: &[u8],
    ) -> ArchiveResult<PrivateShadowExclusiveWrite> {
        assert!(!self.held);
        self.trace.push(kind.directory().to_owned());
        let key = (kind, digest.to_owned());
        // report a racing or retained exact address without replacing it
        if self.metadata.contains_key(&key) {
            return Ok(PrivateShadowExclusiveWrite::AlreadyPresent);
        }
        self.metadata.insert(key, bytes.to_vec());
        Ok(PrivateShadowExclusiveWrite::Created)
    }
}

struct FixtureClock {
    calls: Rc<Cell<usize>>,
}

impl RevisionArchiveCanonicalClock for FixtureClock {
    /// Return the first stage and publication clocks in source order.
    fn canonical_now(&mut self) -> Option<String> {
        let index = self.calls.get();
        self.calls.set(index + 1);
        [STAGE_AT, PUBLISH_AT].get(index).map(
            // own one exact admitted canonical instant
            |value| (*value).to_owned(),
        )
    }
}

/// Build the exact compact v1 or complete v2 publication input.
fn publish_input(expected: &ExpectedFixture) -> PrivateShadowPublishInput {
    match expected.version {
        PrivateShadowArchiveVersion::V1 => PrivateShadowPublishInput::V1 {
            metadata: expected.stage_input.metadata.clone(),
            revision_receipt: expected.revision_receipt.clone(),
            source_projection_sha256: expected.stage_input.sourceProjectionSha256.clone(),
            stage_receipt_sha256: expected.stage_receipt.stageReceiptSha256.clone(),
        },
        PrivateShadowArchiveVersion::V2 => PrivateShadowPublishInput::V2 {
            prediction_committed_at: PREDICTION_COMMITTED_AT.to_owned(),
            revision_receipt: expected.revision_receipt.clone(),
            stage_input: Box::new(expected.stage_input.clone()),
            stage_receipt: expected.stage_receipt.clone(),
        },
    }
}

#[test]
fn matches_node_v1_and_v2_stage_capsule_and_receipt_order() {
    let oracle = NodeOracle::load();
    // execute both closed source grammars through independent two-slot spools
    for version in [
        PrivateShadowArchiveVersion::V1,
        PrivateShadowArchiveVersion::V2,
    ] {
        let expected = oracle.fixture(version);
        let clock_calls = Rc::new(Cell::new(0));
        let codec = FixtureCodec {
            expected: expected.clone(),
        };
        let mut archive = PrivateShadowArchive::new(
            FixtureStorage::default(),
            codec,
            FixtureClock {
                calls: Rc::clone(&clock_calls),
            },
        );

        let stage = archive
            .stage_shadow_revision(expected.stage_input.clone())
            .expect("stage");
        assert_eq!(stage, expected.stage_receipt);
        assert_eq!(
            archive.stage_shadow_revision(expected.stage_input.clone()),
            Ok(stage.clone())
        );
        assert_eq!(clock_calls.get(), 1);

        let publication = archive
            .publish_shadow_revision(publish_input(&expected))
            .expect("publish");
        assert_eq!(publication, expected.publish_receipt);
        assert_eq!(clock_calls.get(), 2);
        assert_eq!(
            archive.persist_shadow_revision_capsule(&expected.capsule_bytes),
            Ok(FixtureCodec {
                expected: expected.clone(),
            }
            .capsule_document())
        );
        assert_eq!(clock_calls.get(), 2);

        let spool = archive.into_storage();
        let stage_address = RevisionOnlinePayloadAddress::from_sha256(
            RevisionOnlinePayloadKind::ShadowStage,
            &expected.stage_input.metadata.predictionSha256,
        )
        .expect("stage address");
        let capsule_address = RevisionOnlinePayloadAddress::from_sha256(
            RevisionOnlinePayloadKind::ShadowCapsule,
            &expected.revision_receipt.receiptSha256,
        )
        .expect("capsule address");
        assert!(!spool.content.contains_key(&stage_address));
        assert_eq!(spool.content[&capsule_address], expected.capsule_bytes);
        assert_eq!(
            spool.trace,
            [
                "stage",
                "revision-commit-receipts",
                "revision-frontier-successors",
                "capsule",
                "retire-stage",
                "sync-stage-directory",
                "shadow-revision-publish-receipts",
                "revision-commit-receipts",
                "revision-frontier-successors",
                "capsule",
                "retire-stage",
                "sync-stage-directory",
            ]
        );
        assert_eq!(
            spool.metadata[&(
                PrivateShadowArchiveMetadataKind::RevisionCommitReceipt,
                expected.revision_receipt.frontierSha256.clone(),
            )],
            expected.revision_bytes
        );
        assert_eq!(
            spool.metadata[&(
                PrivateShadowArchiveMetadataKind::RevisionFrontierSuccessor,
                expected.revision_receipt.predecessorFrontierSha256.clone(),
            )],
            expected.successor_bytes
        );
        assert_eq!(
            spool.metadata[&(
                PrivateShadowArchiveMetadataKind::PublishReceipt,
                expected.stage_input.metadata.predictionSha256.clone(),
            )],
            expected.publish_bytes
        );
    }
}

#[test]
fn native_spool_retires_stage_after_durable_node_matched_capsule() {
    let temp = TempSpool::new();
    let expected = NodeOracle::load().fixture(PrivateShadowArchiveVersion::V1);
    let storage =
        NativeRevisionSpool::new(temp.path().to_path_buf(), NodeLeaseCodec).expect("native spool");
    let clock_calls = Rc::new(Cell::new(0));
    let codec = FixtureCodec {
        expected: expected.clone(),
    };
    let mut archive = PrivateShadowArchive::new(
        storage,
        codec,
        FixtureClock {
            calls: Rc::clone(&clock_calls),
        },
    );

    let stage = archive
        .stage_shadow_revision(expected.stage_input.clone())
        .expect("native stage");
    assert_eq!(stage, expected.stage_receipt);
    let publication = archive
        .publish_shadow_revision(publish_input(&expected))
        .expect("native publish");
    assert_eq!(publication, expected.publish_receipt);
    assert_eq!(clock_calls.get(), 2);

    let stage_name = format!(
        "sha256-{}.json",
        expected.stage_input.metadata.predictionSha256
    );
    let capsule_name = format!("sha256-{}.json", expected.revision_receipt.receiptSha256);
    let revision_name = format!("sha256-{}.json", expected.revision_receipt.frontierSha256);
    let successor_name = format!(
        "sha256-{}.json",
        expected.revision_receipt.predecessorFrontierSha256
    );
    let publish_name = format!(
        "sha256-{}.json",
        expected.stage_input.metadata.predictionSha256
    );
    assert!(
        !temp
            .path()
            .join("shadow-revision-stages")
            .join(stage_name)
            .exists()
    );
    assert_eq!(
        std::fs::read(
            temp.path()
                .join("shadow-revision-capsules")
                .join(capsule_name)
        )
        .expect("capsule"),
        expected.capsule_bytes
    );
    assert_eq!(
        std::fs::read(
            temp.path()
                .join("revision-commit-receipts")
                .join(revision_name)
        )
        .expect("revision receipt"),
        expected.revision_bytes
    );
    assert_eq!(
        std::fs::read(
            temp.path()
                .join("revision-frontier-successors")
                .join(successor_name)
        )
        .expect("frontier successor"),
        expected.successor_bytes
    );
    assert_eq!(
        std::fs::read(
            temp.path()
                .join("shadow-revision-publish-receipts")
                .join(publish_name)
        )
        .expect("publish receipt"),
        expected.publish_bytes
    );
    assert!(!temp.path().join("revision-spool-locks/spool.lock").exists());
}
