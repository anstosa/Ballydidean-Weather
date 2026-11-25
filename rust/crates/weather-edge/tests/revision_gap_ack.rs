use std::path::PathBuf;
use std::process::Command;
use weather_edge::{
    AdmittedRevisionGapAckIdentity, AdmittedRevisionGapPageIdentity,
    REVISION_GAP_ACK_CONTRACT_VERSION, REVISION_GAP_PAGE_MAXIMUM_BYTES,
    REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION, RetainedRevisionGapEntry,
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, RevisionGapAckCodec,
    RevisionGapAckError, RevisionGapAckRequest, RevisionGapAckStorage, RevisionGapIdentityAdapter,
    RevisionGapPayloadAcknowledgement, RevisionGapPayloadPage, RevisionGapRetainedBytes,
    RevisionGapRetirementDirectory, RevisionGapRetirementGroup, RevisionGapRetirementMember,
    RevisionGapSha256, RevisionGapTransferError, RevisionGapUnsignedAcknowledgement,
    RevisionGapUnsignedPage, RevisionGapUnsignedStart, RevisionOnlinePayloadAddress,
    RevisionOnlinePayloadKind, RevisionOnlineSpoolError, RevisionOnlineSpoolStorage,
    StoredRevisionGapCheckpoint, StoredRevisionGapPendingPage, acknowledge_revision_gap_payload,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureEntry {
    retirement_groups: Vec<RevisionGapRetirementGroup>,
    wire_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Ensure,
    LeaseStart,
    LeaseEnd,
    ReadCheckpoint(usize),
    ReadPending(usize),
    WriteCheckpoint(usize),
    Verify(RevisionGapRetirementMember),
    Remove(RevisionGapRetirementMember),
    Sync(RevisionGapRetirementDirectory),
    RemovePending(bool),
    SyncTransfer,
}

#[derive(Debug)]
struct FixtureStorage {
    checkpoint: StoredRevisionGapCheckpoint,
    events: Vec<Event>,
    pending: Option<StoredRevisionGapPendingPage<FixtureEntry>>,
    written_checkpoint: Option<Vec<u8>>,
}

impl RevisionOnlineSpoolStorage for FixtureStorage {
    /// hold every fixture operation inside one observable lease interval.
    fn with_revision_spool_lease<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, RevisionOnlineSpoolError>,
    ) -> Result<T, RevisionOnlineSpoolError> {
        self.events.push(Event::LeaseStart);
        let result = operation(self);
        self.events.push(Event::LeaseEnd);
        result
    }

    /// reject unrelated payload reads from the acknowledgement-only fixture.
    fn read_existing_payload(
        &mut self,
        _address: &RevisionOnlinePayloadAddress,
        _maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError> {
        Err(RevisionOnlineSpoolError::Storage)
    }

    /// reject unrelated spool enumeration from the acknowledgement-only fixture.
    fn list_payload_names(
        &mut self,
        _kind: RevisionOnlinePayloadKind,
    ) -> Result<Vec<String>, RevisionOnlineSpoolError> {
        Err(RevisionOnlineSpoolError::Storage)
    }

    /// reject unrelated payload sizing from the acknowledgement-only fixture.
    fn payload_file_length(
        &mut self,
        _address: &RevisionOnlinePayloadAddress,
        _maximum_bytes: usize,
    ) -> Result<usize, RevisionOnlineSpoolError> {
        Err(RevisionOnlineSpoolError::Storage)
    }
}

impl RevisionGapAckStorage<FixtureEntry> for FixtureStorage {
    /// record the pre-lease fixed archive check.
    fn ensure_revision_gap_archive(&mut self) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push(Event::Ensure);
        Ok(())
    }

    /// return the fixture checkpoint with its requested read ceiling.
    fn read_revision_gap_checkpoint(
        &mut self,
        maximum_bytes: usize,
    ) -> Result<StoredRevisionGapCheckpoint, RevisionOnlineSpoolError> {
        self.events.push(Event::ReadCheckpoint(maximum_bytes));
        Ok(self.checkpoint.clone())
    }

    /// return the fixture pending page with its requested read ceiling.
    fn read_revision_gap_pending_page(
        &mut self,
        maximum_bytes: usize,
    ) -> Result<Option<StoredRevisionGapPendingPage<FixtureEntry>>, RevisionOnlineSpoolError> {
        self.events.push(Event::ReadPending(maximum_bytes));
        Ok(self.pending.clone())
    }

    /// retain the exact checkpoint bytes supplied before retirement.
    fn write_revision_gap_checkpoint_atomic(
        &mut self,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Result<(), RevisionOnlineSpoolError> {
        assert!(bytes.len() <= maximum_bytes);
        self.events.push(Event::WriteCheckpoint(bytes.len()));
        self.written_checkpoint = Some(bytes.to_vec());
        Ok(())
    }

    /// record one bounded digest verification.
    fn verify_revision_gap_retirement_member(
        &mut self,
        member: RevisionGapRetirementMember,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push(Event::Verify(member));
        Ok(())
    }

    /// record one idempotent exact-member removal.
    fn remove_revision_gap_retirement_member_if_present(
        &mut self,
        member: RevisionGapRetirementMember,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push(Event::Remove(member));
        Ok(())
    }

    /// record one source-ordered directory durability barrier.
    fn sync_revision_gap_retirement_directory(
        &mut self,
        directory: RevisionGapRetirementDirectory,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push(Event::Sync(directory));
        Ok(())
    }

    /// record fresh versus retry pending-page removal semantics.
    fn remove_revision_gap_pending_page(
        &mut self,
        allow_absent: bool,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push(Event::RemovePending(allow_absent));
        self.pending = None;
        Ok(())
    }

    /// record the final gap-transfer directory durability barrier.
    fn sync_revision_gap_transfer_directory(&mut self) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push(Event::SyncTransfer);
        Ok(())
    }
}

#[derive(Debug, Default)]
struct FixtureCodec;

impl RevisionArchiveCanonicalCodec for FixtureCodec {
    /// admit only the fixed canonical fixture instant.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        (text == "2026-11-23T12:00:00.000Z").then_some(1_795_435_200_000)
    }

    /// return a deterministic fixture digest for unused generic codec calls.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        [u8::try_from(bytes.len()).unwrap_or(u8::MAX); 32]
    }
}

impl RevisionGapIdentityAdapter<FixtureEntry> for FixtureCodec {
    /// derive a deterministic fixture start identity.
    fn start_sha256(
        &mut self,
        start: RevisionGapUnsignedStart,
    ) -> Result<RevisionGapSha256, RevisionGapTransferError> {
        Ok(digest(u8::try_from(start.page_count).unwrap_or(u8::MAX)))
    }

    /// bind the page fixture to its entry count and encoded byte count.
    fn page_identity(
        &mut self,
        page: RevisionGapUnsignedPage<'_, FixtureEntry>,
    ) -> Result<AdmittedRevisionGapPageIdentity, RevisionGapTransferError> {
        let count = u8::try_from(page.entries.len()).unwrap_or(u8::MAX);
        Ok(AdmittedRevisionGapPageIdentity {
            canonical_byte_count: 256
                + page
                    .entries
                    .iter()
                    .map(|entry| entry.value.wire_bytes)
                    .sum::<usize>(),
            member_root_sha256: digest(0x80 | count),
            page_sha256: digest(0x40 | count),
        })
    }
}

impl RevisionGapAckCodec<FixtureEntry> for FixtureCodec {
    /// return the fixed fixture genesis frontier.
    fn genesis_frontier_sha256(&self) -> RevisionGapSha256 {
        digest(7)
    }

    /// encode the page as one exact length-bound fixture representation.
    fn payload_page_bytes(&self, page: &RevisionGapPayloadPage<FixtureEntry>) -> Option<Vec<u8>> {
        let mut bytes = vec![0; page.canonical_byte_count];
        // retain a visible page identity inside the opaque fixture encoding
        if let Some(first) = bytes.first_mut() {
            *first = page.page_sha256.as_bytes()[0];
        }
        Some(bytes)
    }

    /// derive deterministic frontier and acknowledgement identities from every binding.
    fn acknowledgement_identity(
        &self,
        value: RevisionGapUnsignedAcknowledgement<'_>,
    ) -> Option<AdmittedRevisionGapAckIdentity> {
        let mut frontier = *value.predecessor_frontier_sha256.as_bytes();
        frontier[0] ^= value.page_sha256.as_bytes()[0];
        frontier[1] ^= value.graph_manifest_sha256.as_bytes()[0];
        frontier[2] ^= value.member_root_sha256.as_bytes()[0];
        frontier[3] ^= u8::try_from(value.page_count).ok()?;
        let mut acknowledgement = frontier;
        acknowledgement[4] ^= u8::try_from(value.acknowledged_at.len()).ok()?;
        Some(AdmittedRevisionGapAckIdentity {
            acknowledgement_sha256: RevisionGapSha256::admitted(acknowledgement),
            frontier_sha256: RevisionGapSha256::admitted(frontier),
        })
    }

    /// encode all complete checkpoint bindings without implementing JSON in the engine.
    fn acknowledgement_bytes(&self, value: &RevisionGapPayloadAcknowledgement) -> Option<Vec<u8>> {
        let mut bytes = value.acknowledged_at.as_bytes().to_vec();
        bytes.extend_from_slice(value.acknowledgement_sha256.as_bytes());
        bytes.extend_from_slice(value.frontier_sha256.as_bytes());
        bytes.extend_from_slice(value.graph_manifest_sha256.as_bytes());
        bytes.extend_from_slice(value.member_root_sha256.as_bytes());
        bytes.extend_from_slice(&value.page_count.to_be_bytes());
        bytes.extend_from_slice(value.page_sha256.as_bytes());
        bytes.extend_from_slice(value.predecessor_frontier_sha256.as_bytes());
        Some(bytes)
    }

    /// return the already-admitted fixed retirement groups for one fixture entry.
    fn retirement_groups(
        &self,
        entry: &RetainedRevisionGapEntry<FixtureEntry>,
    ) -> Option<Vec<RevisionGapRetirementGroup>> {
        Some(entry.value.retirement_groups.clone())
    }
}

#[derive(Debug)]
struct FixtureClock {
    calls: usize,
}

impl RevisionArchiveCanonicalClock for FixtureClock {
    /// return one stable fixture instant and count clock access.
    fn canonical_now(&mut self) -> Option<String> {
        self.calls += 1;
        Some("2026-11-23T12:00:00.000Z".to_owned())
    }
}

/// construct one already-admitted digest fixture.
fn digest(byte: u8) -> RevisionGapSha256 {
    RevisionGapSha256::admitted([byte; 32])
}

/// construct one fixed retirement member.
fn member(
    directory: RevisionGapRetirementDirectory,
    slot: u8,
    expected: u8,
    maximum_bytes: usize,
) -> RevisionGapRetirementMember {
    RevisionGapRetirementMember {
        directory,
        expected_sha256: digest(expected),
        maximum_bytes,
        slot_sha256: digest(slot),
    }
}

/// construct one exact nonempty frozen page and its retained bytes.
fn pending_page(codec: &FixtureCodec) -> StoredRevisionGapPendingPage<FixtureEntry> {
    let projection = member(
        RevisionGapRetirementDirectory::RevisionProjections,
        11,
        21,
        3_500 * 1_024,
    );
    let stage = member(
        RevisionGapRetirementDirectory::RevisionStageReceipts,
        11,
        22,
        4_096,
    );
    let state = member(
        RevisionGapRetirementDirectory::RainControlStates,
        12,
        23,
        32 * 1_024,
    );
    let state_receipt = member(
        RevisionGapRetirementDirectory::RainControlStateStageReceipts,
        12,
        24,
        4_096,
    );
    let entries = vec![RetainedRevisionGapEntry {
        gap_sha256: digest(1),
        retained_bytes: RevisionGapRetainedBytes::Projection {
            gap_bytes: 512,
            payload_bytes: 1_024,
            stage_receipt_bytes: 512,
            rain_control_state: None,
        },
        value: FixtureEntry {
            retirement_groups: vec![
                RevisionGapRetirementGroup {
                    members: vec![projection, stage],
                },
                RevisionGapRetirementGroup {
                    members: vec![state, state_receipt],
                },
            ],
            wire_bytes: 44,
        },
    }];
    let page = RevisionGapPayloadPage {
        canonical_byte_count: 300,
        contract_version: REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION,
        entries,
        idle: false,
        member_root_sha256: digest(0x81),
        page_sha256: digest(0x41),
        predecessor_frontier_sha256: digest(7),
        start_sha256: digest(8),
    };
    StoredRevisionGapPendingPage {
        canonical_bytes: codec.payload_page_bytes(&page).expect("page bytes"),
        page,
    }
}

/// construct one fresh fixture storage owner.
fn storage(pending: Option<StoredRevisionGapPendingPage<FixtureEntry>>) -> FixtureStorage {
    FixtureStorage {
        checkpoint: StoredRevisionGapCheckpoint::Genesis,
        events: Vec::new(),
        pending,
        written_checkpoint: None,
    }
}

// verify checkpoint-first retirement ordering
#[test]
fn fresh_acknowledgement_writes_checkpoint_before_ordered_retirement() {
    let mut codec = FixtureCodec;
    let pending = pending_page(&codec);
    let request = RevisionGapAckRequest {
        graph_manifest_sha256: digest(9),
        page_sha256: pending.page.page_sha256,
    };
    let mut storage = storage(Some(pending));
    let mut clock = FixtureClock { calls: 0 };

    let acknowledgement =
        acknowledge_revision_gap_payload(&mut storage, &mut codec, &mut clock, request)
            .expect("acknowledgement");

    assert_eq!(
        acknowledgement.contract_version,
        REVISION_GAP_ACK_CONTRACT_VERSION
    );
    assert_eq!(acknowledgement.page_count, 1);
    assert_eq!(acknowledgement.page_sha256, digest(0x41));
    assert_eq!(clock.calls, 1);
    let projection = member(
        RevisionGapRetirementDirectory::RevisionProjections,
        11,
        21,
        3_500 * 1_024,
    );
    let stage = member(
        RevisionGapRetirementDirectory::RevisionStageReceipts,
        11,
        22,
        4_096,
    );
    let state = member(
        RevisionGapRetirementDirectory::RainControlStates,
        12,
        23,
        32 * 1_024,
    );
    let state_receipt = member(
        RevisionGapRetirementDirectory::RainControlStateStageReceipts,
        12,
        24,
        4_096,
    );
    assert_eq!(
        storage.events,
        vec![
            Event::Ensure,
            Event::LeaseStart,
            Event::ReadCheckpoint(4_096),
            Event::ReadPending(REVISION_GAP_PAGE_MAXIMUM_BYTES),
            Event::WriteCheckpoint(
                storage
                    .written_checkpoint
                    .as_ref()
                    .expect("checkpoint bytes")
                    .len(),
            ),
            Event::Verify(projection),
            Event::Verify(stage),
            Event::Remove(projection),
            Event::Remove(stage),
            Event::Sync(RevisionGapRetirementDirectory::RevisionProjections),
            Event::Sync(RevisionGapRetirementDirectory::RevisionStageReceipts),
            Event::Verify(state),
            Event::Verify(state_receipt),
            Event::Remove(state),
            Event::Remove(state_receipt),
            Event::Sync(RevisionGapRetirementDirectory::RainControlStates),
            Event::Sync(RevisionGapRetirementDirectory::RainControlStateStageReceipts),
            Event::RemovePending(false),
            Event::SyncTransfer,
            Event::LeaseEnd,
        ]
    );
}

// verify crash retry convergence
#[test]
fn exact_retry_finishes_pending_retirement_without_clock_or_checkpoint_write() {
    let mut codec = FixtureCodec;
    let pending = pending_page(&codec);
    let request = RevisionGapAckRequest {
        graph_manifest_sha256: digest(9),
        page_sha256: pending.page.page_sha256,
    };
    let mut first_storage = storage(Some(pending.clone()));
    let mut first_clock = FixtureClock { calls: 0 };
    let acknowledgement =
        acknowledge_revision_gap_payload(&mut first_storage, &mut codec, &mut first_clock, request)
            .expect("first acknowledgement");
    let checkpoint_bytes = codec
        .acknowledgement_bytes(&acknowledgement)
        .expect("checkpoint bytes");
    let mut retry_storage = FixtureStorage {
        checkpoint: StoredRevisionGapCheckpoint::Acknowledged {
            acknowledgement: Box::new(acknowledgement.clone()),
            canonical_bytes: checkpoint_bytes.clone(),
        },
        events: Vec::new(),
        pending: Some(pending),
        written_checkpoint: None,
    };
    let mut retry_clock = FixtureClock { calls: 0 };

    let retry =
        acknowledge_revision_gap_payload(&mut retry_storage, &mut codec, &mut retry_clock, request)
            .expect("retry acknowledgement");

    assert_eq!(retry, acknowledgement);
    assert_eq!(retry_clock.calls, 0);
    assert!(retry_storage.written_checkpoint.is_none());
    assert!(retry_storage.events.contains(&Event::RemovePending(true)));
    let mut complete_storage = FixtureStorage {
        checkpoint: StoredRevisionGapCheckpoint::Acknowledged {
            acknowledgement: Box::new(acknowledgement.clone()),
            canonical_bytes: checkpoint_bytes,
        },
        events: Vec::new(),
        pending: None,
        written_checkpoint: None,
    };
    let complete = acknowledge_revision_gap_payload(
        &mut complete_storage,
        &mut codec,
        &mut retry_clock,
        request,
    )
    .expect("completed retry");
    assert_eq!(complete, acknowledgement);
    assert_eq!(
        complete_storage.events,
        vec![
            Event::Ensure,
            Event::LeaseStart,
            Event::ReadCheckpoint(4_096),
            Event::ReadPending(REVISION_GAP_PAGE_MAXIMUM_BYTES),
            Event::LeaseEnd,
        ]
    );
}

// verify ordinary mismatch outcomes
#[test]
fn collision_missing_page_and_changed_pending_bytes_stop_before_mutation() {
    let mut codec = FixtureCodec;
    let pending = pending_page(&codec);
    let request = RevisionGapAckRequest {
        graph_manifest_sha256: digest(9),
        page_sha256: pending.page.page_sha256,
    };
    let mut first_storage = storage(Some(pending.clone()));
    let mut clock = FixtureClock { calls: 0 };
    let acknowledgement =
        acknowledge_revision_gap_payload(&mut first_storage, &mut codec, &mut clock, request)
            .expect("first acknowledgement");
    let checkpoint_bytes = codec
        .acknowledgement_bytes(&acknowledgement)
        .expect("checkpoint bytes");
    let mut collision_storage = FixtureStorage {
        checkpoint: StoredRevisionGapCheckpoint::Acknowledged {
            acknowledgement: Box::new(acknowledgement),
            canonical_bytes: checkpoint_bytes,
        },
        events: Vec::new(),
        pending: Some(pending.clone()),
        written_checkpoint: None,
    };
    assert_eq!(
        acknowledge_revision_gap_payload(
            &mut collision_storage,
            &mut codec,
            &mut clock,
            RevisionGapAckRequest {
                graph_manifest_sha256: digest(10),
                ..request
            },
        ),
        Err(RevisionGapAckError::AcknowledgementCollision)
    );
    assert!(
        !collision_storage
            .events
            .iter()
            .any(|event| matches!(event, Event::WriteCheckpoint(_) | Event::Remove(_)))
    );

    let mut missing_storage = storage(None);
    assert_eq!(
        acknowledge_revision_gap_payload(&mut missing_storage, &mut codec, &mut clock, request,),
        Err(RevisionGapAckError::PendingPageUnavailable)
    );
    let mut changed = pending;
    changed.canonical_bytes[0] ^= 1;
    let mut changed_storage = storage(Some(changed));
    assert_eq!(
        acknowledge_revision_gap_payload(&mut changed_storage, &mut codec, &mut clock, request,),
        Err(RevisionGapAckError::PendingPageDiffers)
    );
    assert!(
        !changed_storage
            .events
            .iter()
            .any(|event| matches!(event, Event::WriteCheckpoint(_) | Event::Remove(_)))
    );
}

// verify parity with the source owner
#[test]
fn node_oracle_confirms_checkpoint_identity_retry_and_hot_retirement() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/revision-gap-ack-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run revision gap acknowledgement oracle");

    assert!(
        output.status.success(),
        "oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("oracle stdout");
    assert_eq!(
        stdout,
        concat!(
            "contract=adjustment-revision-gap-payload-ack/v1\n",
            "page_count=1\n",
            "checkpoint_first=true\n",
            "retry_stable=true\n",
            "hot_retired=true\n",
            "permanent_gap_retained=true\n",
        )
    );
}
