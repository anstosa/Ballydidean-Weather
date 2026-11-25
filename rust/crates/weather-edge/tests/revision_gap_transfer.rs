use weather_edge::{
    AdmittedRevisionGapPageIdentity, REVISION_GAP_PAGE_MAXIMUM_BYTES,
    REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION, RetainedRevisionGapEntry, RevisionGapFrontier,
    RevisionGapIdentityAdapter, RevisionGapPageRequest, RevisionGapPageSelection,
    RevisionGapPayloadPage, RevisionGapRetainedBytes, RevisionGapRetainedCensus,
    RevisionGapRetainedSnapshot, RevisionGapSha256, RevisionGapTransferError,
    RevisionGapUnsignedPage, RevisionGapUnsignedStart, read_revision_gap_transfer_start,
    select_revision_gap_payload_page,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureEntry {
    name: &'static str,
    wire_bytes: usize,
}

#[derive(Debug, Default)]
struct FixtureIdentity;

impl RevisionGapIdentityAdapter<FixtureEntry> for FixtureIdentity {
    /// Derive a deterministic fixture identity from the actual unsigned start fields.
    fn start_sha256(
        &mut self,
        start: RevisionGapUnsignedStart,
    ) -> Result<RevisionGapSha256, RevisionGapTransferError> {
        assert_eq!(
            start.contract_version,
            "adjustment-revision-gap-transfer-start/v1"
        );
        let mut bytes = *start.frontier_sha256.as_bytes();
        bytes[31] ^= start.page_count as u8;
        Ok(RevisionGapSha256::admitted(bytes))
    }

    /// Model only the canonical byte count and opaque digests owned by the real codec.
    fn page_identity(
        &mut self,
        page: RevisionGapUnsignedPage<'_, FixtureEntry>,
    ) -> Result<AdmittedRevisionGapPageIdentity, RevisionGapTransferError> {
        assert_eq!(
            page.contract_version,
            REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION
        );
        assert_eq!(page.idle, page.entries.is_empty());
        assert_eq!(page.predecessor_frontier_sha256, digest(7));
        assert_ne!(page.start_sha256, page.predecessor_frontier_sha256);
        let canonical_byte_count = 256
            + page
                .entries
                .iter()
                .map(|entry| entry.value.wire_bytes)
                .sum::<usize>();
        let count = page.entries.len() as u8;
        Ok(AdmittedRevisionGapPageIdentity {
            canonical_byte_count,
            member_root_sha256: digest(0x80 | count),
            page_sha256: digest(0x40 | count),
        })
    }
}

/// Construct one already-admitted lowercase-digest fixture.
fn digest(byte: u8) -> RevisionGapSha256 {
    RevisionGapSha256::admitted([byte; 32])
}

/// Construct one ordinary projection member within every source read cap.
fn entry(
    order: u8,
    name: &'static str,
    wire_bytes: usize,
) -> RetainedRevisionGapEntry<FixtureEntry> {
    RetainedRevisionGapEntry {
        gap_sha256: digest(order),
        retained_bytes: RevisionGapRetainedBytes::Projection {
            gap_bytes: 512,
            payload_bytes: 1_024,
            stage_receipt_bytes: 512,
            rain_control_state: None,
        },
        value: FixtureEntry { name, wire_bytes },
    }
}

/// Return one empty and source-bounded spool census.
fn census() -> RevisionGapRetainedCensus {
    RevisionGapRetainedCensus::default()
}

/// Derive the exact start and request used by page fixtures.
fn request(identity: &mut FixtureIdentity) -> (RevisionGapFrontier, RevisionGapPageRequest) {
    let frontier = RevisionGapFrontier {
        frontier_sha256: digest(7),
        page_count: 3,
    };
    let start = read_revision_gap_transfer_start(frontier, identity).expect("start");
    (
        frontier,
        RevisionGapPageRequest {
            frontier_sha256: frontier.frontier_sha256,
            start_sha256: start.start_sha256,
        },
    )
}

#[test]
fn start_projects_the_compact_frontier() {
    let mut identity = FixtureIdentity;
    let frontier = RevisionGapFrontier {
        frontier_sha256: digest(9),
        page_count: 5,
    };

    let start = read_revision_gap_transfer_start(frontier, &mut identity).expect("start");

    assert_eq!(
        start.contract_version,
        "adjustment-revision-gap-transfer-start/v1"
    );
    assert_eq!(start.frontier_sha256, digest(9));
    assert_eq!(start.page_count, 5);
    let mut expected = [9; 32];
    expected[31] = 12;
    assert_eq!(start.start_sha256, RevisionGapSha256::admitted(expected));
}

#[test]
fn new_page_uses_gap_digest_order_and_two_slot_limit() {
    let mut identity = FixtureIdentity;
    let (frontier, request) = request(&mut identity);
    let snapshot = RevisionGapRetainedSnapshot {
        available_entries: vec![
            entry(3, "third", 30),
            entry(1, "first", 10),
            entry(2, "second", 20),
        ],
        census: census(),
        frontier,
        pending_page: None,
    };

    let selected =
        select_revision_gap_payload_page(request, snapshot, &mut identity).expect("selected page");
    // require a newly projected page for the empty pending slot
    let RevisionGapPageSelection::New(page) = selected else {
        panic!("expected a new page");
    };

    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries[0].value.name, "first");
    assert_eq!(page.entries[1].value.name, "second");
    assert!(!page.idle);
    assert_eq!(page.canonical_byte_count, 286);
}

#[test]
fn pending_page_is_returned_before_current_census_is_scanned() {
    let mut identity = FixtureIdentity;
    let (frontier, request) = request(&mut identity);
    let pending = RevisionGapPayloadPage {
        canonical_byte_count: 300,
        contract_version: REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION,
        entries: vec![entry(1, "pending", 44)],
        idle: false,
        member_root_sha256: digest(0x81),
        page_sha256: digest(0x41),
        predecessor_frontier_sha256: request.frontier_sha256,
        start_sha256: request.start_sha256,
    };
    let snapshot = RevisionGapRetainedSnapshot {
        available_entries: vec![entry(2, "new", 22)],
        census: RevisionGapRetainedCensus {
            revision_projection_files: 3,
            ..census()
        },
        frontier,
        pending_page: Some(pending.clone()),
    };

    let selected =
        select_revision_gap_payload_page(request, snapshot, &mut identity).expect("retained page");

    assert_eq!(selected, RevisionGapPageSelection::Retained(pending));
}

#[test]
fn later_oversized_candidate_leaves_one_bounded_entry() {
    let mut identity = FixtureIdentity;
    let (frontier, request) = request(&mut identity);
    let snapshot = RevisionGapRetainedSnapshot {
        available_entries: vec![
            entry(1, "fits", 1_024),
            entry(2, "too-large-together", REVISION_GAP_PAGE_MAXIMUM_BYTES),
        ],
        census: census(),
        frontier,
        pending_page: None,
    };

    let selected =
        select_revision_gap_payload_page(request, snapshot, &mut identity).expect("one-entry page");
    // require a newly projected page for the empty pending slot
    let RevisionGapPageSelection::New(page) = selected else {
        panic!("expected a new page");
    };

    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].value.name, "fits");
    assert_eq!(page.canonical_byte_count, 1_280);
}

#[test]
fn first_oversized_candidate_is_rejected() {
    let mut identity = FixtureIdentity;
    let (frontier, request) = request(&mut identity);
    let snapshot = RevisionGapRetainedSnapshot {
        available_entries: vec![entry(1, "too-large", REVISION_GAP_PAGE_MAXIMUM_BYTES)],
        census: census(),
        frontier,
        pending_page: None,
    };

    let error = select_revision_gap_payload_page(request, snapshot, &mut identity)
        .expect_err("oversized first member");

    assert_eq!(error, RevisionGapTransferError::PayloadExceedsPageLimit);
}

#[test]
fn transfer_rechecks_start_census_and_retained_file_caps() {
    let mut identity = FixtureIdentity;
    let (frontier, request) = request(&mut identity);
    let wrong_start = RevisionGapPageRequest {
        start_sha256: digest(99),
        ..request
    };
    let empty = RevisionGapRetainedSnapshot {
        available_entries: Vec::new(),
        census: census(),
        frontier,
        pending_page: None,
    };
    assert_eq!(
        select_revision_gap_payload_page(wrong_start, empty, &mut identity),
        Err(RevisionGapTransferError::TransferStartDiffers)
    );

    let overfull = RevisionGapRetainedSnapshot {
        available_entries: Vec::new(),
        census: RevisionGapRetainedCensus {
            single_commit_receipt_files: 3,
            single_commit_receipt_maximum_bytes: 4_096,
            ..census()
        },
        frontier,
        pending_page: None,
    };
    assert_eq!(
        select_revision_gap_payload_page(request, overfull, &mut identity),
        Err(RevisionGapTransferError::CensusDiffers)
    );

    let exact_receipt_caps = RevisionGapRetainedSnapshot {
        available_entries: Vec::new(),
        census: RevisionGapRetainedCensus {
            batch_commit_receipt_files: 1,
            batch_commit_receipt_maximum_bytes: 256 * 1_024,
            single_commit_receipt_files: 1,
            single_commit_receipt_maximum_bytes: 4_096,
            ..census()
        },
        frontier,
        pending_page: None,
    };
    assert!(matches!(
        select_revision_gap_payload_page(request, exact_receipt_caps, &mut identity),
        Ok(RevisionGapPageSelection::New(page)) if page.idle
    ));

    let oversized_batch_receipt = RevisionGapRetainedSnapshot {
        available_entries: Vec::new(),
        census: RevisionGapRetainedCensus {
            batch_commit_receipt_files: 1,
            batch_commit_receipt_maximum_bytes: 256 * 1_024 + 1,
            ..census()
        },
        frontier,
        pending_page: None,
    };
    assert_eq!(
        select_revision_gap_payload_page(request, oversized_batch_receipt, &mut identity),
        Err(RevisionGapTransferError::CensusDiffers)
    );

    let oversized_receipt = RevisionGapRetainedSnapshot {
        available_entries: vec![RetainedRevisionGapEntry {
            gap_sha256: digest(1),
            retained_bytes: RevisionGapRetainedBytes::Projection {
                gap_bytes: 512,
                payload_bytes: 1_024,
                stage_receipt_bytes: 4_097,
                rain_control_state: None,
            },
            value: FixtureEntry {
                name: "bad-receipt",
                wire_bytes: 10,
            },
        }],
        census: census(),
        frontier,
        pending_page: None,
    };
    assert_eq!(
        select_revision_gap_payload_page(request, oversized_receipt, &mut identity),
        Err(RevisionGapTransferError::RetainedBytesDiffer)
    );

    let invalid_unselected = RevisionGapRetainedSnapshot {
        available_entries: vec![
            entry(1, "first", 10),
            entry(2, "second", 10),
            RetainedRevisionGapEntry {
                gap_sha256: digest(3),
                retained_bytes: RevisionGapRetainedBytes::ShadowStage {
                    gap_bytes: 4_096,
                    stage_bytes: 2 * 1_024 * 1_024 + 1,
                },
                value: FixtureEntry {
                    name: "invalid-third",
                    wire_bytes: 10,
                },
            },
        ],
        census: census(),
        frontier,
        pending_page: None,
    };
    assert_eq!(
        select_revision_gap_payload_page(request, invalid_unselected, &mut identity),
        Err(RevisionGapTransferError::RetainedBytesDiffer)
    );
    assert_eq!(
        RevisionGapTransferError::IdentityUnavailable,
        RevisionGapTransferError::IdentityUnavailable
    );
}

#[test]
fn retained_state_and_shadow_members_keep_their_distinct_source_caps() {
    let mut identity = FixtureIdentity;
    let (frontier, request) = request(&mut identity);
    let snapshot = RevisionGapRetainedSnapshot {
        available_entries: vec![
            RetainedRevisionGapEntry {
                gap_sha256: digest(1),
                retained_bytes: RevisionGapRetainedBytes::RainControlState {
                    gap_bytes: 4_096,
                    payload_bytes: 32 * 1_024,
                    stage_receipt_bytes: 4_096,
                },
                value: FixtureEntry {
                    name: "state",
                    wire_bytes: 1_000,
                },
            },
            RetainedRevisionGapEntry {
                gap_sha256: digest(2),
                retained_bytes: RevisionGapRetainedBytes::ShadowStage {
                    gap_bytes: 4_096,
                    stage_bytes: 2 * 1_024 * 1_024,
                },
                value: FixtureEntry {
                    name: "shadow",
                    wire_bytes: 1_000,
                },
            },
        ],
        census: census(),
        frontier,
        pending_page: None,
    };

    let selected = select_revision_gap_payload_page(request, snapshot, &mut identity)
        .expect("bounded source classes");
    // require a newly projected page for the empty pending slot
    let RevisionGapPageSelection::New(page) = selected else {
        panic!("expected a new page");
    };

    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries[0].value.name, "state");
    assert_eq!(page.entries[1].value.name, "shadow");
}
