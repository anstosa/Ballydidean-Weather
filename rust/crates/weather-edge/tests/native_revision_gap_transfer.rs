#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use weather_contracts::{
    AdjustmentRevisionGapInput, AdjustmentRevisionGapReason, AdjustmentRevisionProjectionKind,
    Nullable, RainMaintenanceControlStateStageReceipt,
};
use weather_edge::{
    AdjustmentMaintenanceGapReceipt, AdjustmentRevisionGapReceipt, AdmittedRevisionGapAckIdentity,
    AdmittedRevisionGapPageIdentity, NativeRevisionGapAckDecoder, NativeRevisionGapCommandCodec,
    NativeRevisionGapCommandPorts, NativeRevisionGapMember, NativeRevisionGapPayloadKind,
    NativeRevisionGapProjection, NativeRevisionGapShadowStage, NativeRevisionGapTransferCodec,
    NativeRevisionSpool, NativeRevisionSpoolArtifactCodec, PermanentArchiveGapCodec,
    PermanentArchiveGapInput, REVISION_GAP_ACK_CONTRACT_VERSION,
    REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION, REVISION_GAP_TRANSFER_START_CONTRACT_VERSION,
    RainControlStateWireCodec, RainMaintenanceControlStateGapReceipt, RetainedRevisionGapEntry,
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, RevisionGapAckCodec,
    RevisionGapFrontier, RevisionGapIdentityAdapter, RevisionGapPageRequest,
    RevisionGapPayloadAcknowledgement, RevisionGapPayloadPage, RevisionGapRetainedBytes,
    RevisionGapRetirementGroup, RevisionGapSha256, RevisionGapTransferError,
    RevisionGapTransferStart, RevisionGapUnsignedAcknowledgement, RevisionGapUnsignedPage,
    RevisionGapUnsignedStart, parse_revision_gap_operator_command,
};

#[path = "support/native_spool.rs"]
mod native_spool;
use native_spool::{NodeLeaseCodec, TempSpool};

#[derive(Clone)]
struct NodeWitness {
    fields: BTreeMap<String, String>,
    idle: bool,
    projection_reads: Cell<usize>,
}

impl NodeWitness {
    /// Seed exact source files but leave the pending cursor for Rust to publish.
    fn seed(root: &Path, idle: bool) -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/revision-gap-transfer-node-oracle.mjs");
        let output = Command::new("node")
            .arg(script)
            .arg("--native-fixture")
            .arg(root)
            .arg(if idle { "idle" } else { "classified" })
            .output()
            .expect("start Node oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let fields = String::from_utf8(output.stdout)
            .expect("oracle UTF-8")
            .lines()
            .map(
                // preserve every exact reference line
                |line| {
                    let (key, value) = line.split_once('=').expect("named oracle value");
                    (key.to_owned(), value.to_owned())
                },
            )
            .collect();
        Self {
            fields,
            idle,
            projection_reads: Cell::new(0),
        }
    }

    /// Restore the canonical trailing LF removed by the witness line framing.
    fn bytes(&self, key: &str) -> Vec<u8> {
        format!("{}\n", self.fields[key]).into_bytes()
    }

    /// Decode a fixture's exact Node digest as fixed test identity bytes.
    fn digest(&self, key: &str) -> RevisionGapSha256 {
        let value = &self.fields[key];
        RevisionGapSha256::admitted(std::array::from_fn(
            // convert the next known fixture byte
            |index| u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).expect("hex byte"),
        ))
    }

    /// Return the independently executed Node acknowledgement with all canonical identities.
    fn expected_acknowledgement(&self) -> RevisionGapPayloadAcknowledgement {
        RevisionGapPayloadAcknowledgement {
            acknowledged_at: self.fields["ack_at"].clone(),
            acknowledgement_sha256: self.digest("ack_digest"),
            contract_version: REVISION_GAP_ACK_CONTRACT_VERSION,
            frontier_sha256: self.digest("ack_frontier"),
            graph_manifest_sha256: self.digest("ack_graph"),
            member_root_sha256: self.digest("member_root"),
            page_count: 1,
            page_sha256: self.digest("page"),
            predecessor_frontier_sha256: self.digest("frontier"),
        }
    }

    /// Bind the exact single staged entry, or the source's in-flight-only idle case.
    fn expected_page(&self) -> RevisionGapPayloadPage<NativeRevisionGapMember> {
        let mut entries = Vec::new();
        // retain one actual producer-originated payload only after classification
        if !self.idle {
            let gap_bytes = self.bytes("gap_bytes");
            let payload_bytes = self.bytes("payload_bytes");
            let stage_receipt_bytes = self.bytes("receipt_bytes");
            entries.push(RetainedRevisionGapEntry {
                gap_sha256: self.digest("gap_digest"),
                retained_bytes: RevisionGapRetainedBytes::Projection {
                    gap_bytes: gap_bytes.len(),
                    payload_bytes: payload_bytes.len(),
                    stage_receipt_bytes: stage_receipt_bytes.len(),
                    rain_control_state: None,
                },
                value: NativeRevisionGapMember {
                    kind: NativeRevisionGapPayloadKind::Projection,
                    payload_bytes,
                    payload_sha256: self.digest("payload_hash"),
                    slot_sha256: self.digest("payload_hash"),
                    gap_bytes,
                    gap_member_sha256: self.digest("gap_hash"),
                    stage_receipt_bytes,
                    stage_receipt_member_sha256: self.digest("receipt_hash"),
                    rain_control: None,
                },
            });
        }
        RevisionGapPayloadPage {
            contract_version: REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION,
            entries,
            idle: self.idle,
            canonical_byte_count: self.bytes("page_bytes").len(),
            member_root_sha256: self.digest("member_root"),
            page_sha256: self.digest("page"),
            predecessor_frontier_sha256: self.digest("frontier"),
            start_sha256: self.digest("start"),
        }
    }

    /// Return the complete typed input described by this single producer case.
    fn gap_input(&self) -> AdjustmentRevisionGapInput {
        AdjustmentRevisionGapInput {
            logicalKeySha256: "a".repeat(64),
            projectionIdentitySha256: Nullable::Value(self.fields["payload_hash"].clone()),
            projectionKind: AdjustmentRevisionProjectionKind::ActualBestMatch,
            projectionSha256: Nullable::Value(self.fields["payload_hash"].clone()),
            reason: AdjustmentRevisionGapReason::DatabaseBindFailed,
        }
    }
}

impl RevisionArchiveCanonicalCodec for NodeWitness {
    /// Admit the timestamp supplied by the independently executed Node acknowledgement.
    fn canonical_utc_milliseconds(&self, value: &str) -> Option<i64> {
        // recognize only this particular independently witnessed coordinator clock
        if value != self.fields.get("ack_at")? {
            return None;
        }
        self.fields["ack_clock"].parse().ok()
    }

    /// Return independent Node hashes only for the original complete preimages.
    fn sha256(&self, value: &[u8]) -> [u8; 32] {
        // use the source's exact fixed genesis digest
        if value == b"adjustment-revision-gap-frontier/v1\n0\n" {
            return *self.digest("frontier").as_bytes();
        }
        // match each original canonical preimage against its single Node digest
        for label in ["gap", "payload", "receipt", "gap_key"] {
            let key = if label == "gap_key" {
                label.to_owned()
            } else {
                format!("{label}_bytes")
            };
            // bind opaque witnesses rather than introducing a Rust test codec
            if value == self.bytes(&key) {
                return *self.digest(&format!("{label}_hash")).as_bytes();
            }
        }
        panic!("unexpected canonical digest preimage")
    }
}

impl PermanentArchiveGapCodec for NodeWitness {
    /// Return the exact Node locator only for its corresponding source input.
    fn encode_gap_key(&self, input: PermanentArchiveGapInput<'_>) -> Option<Vec<u8>> {
        match input {
            PermanentArchiveGapInput::Revision(input) if *input == self.gap_input() => {
                Some(self.bytes("gap_key"))
            }
            _ => None,
        }
    }

    /// This fixture uses the Node producer rather than a second gap writer.
    fn encode_gap_receipt(
        &self,
        _: PermanentArchiveGapInput<'_>,
        _: &str,
        _: Option<&str>,
    ) -> Option<Vec<u8>> {
        None
    }

    /// Admit the exact original producer bytes, binding all source fields.
    fn accepted_revision_gap(&self, bytes: &[u8]) -> Option<AdjustmentRevisionGapReceipt> {
        (bytes == self.bytes("gap_bytes")).then(
            // project the already verified closed fixture value
            || AdjustmentRevisionGapReceipt {
                contract_version: "adjustment-revision-gap/v1".to_owned(),
                input: self.gap_input(),
                qualification_disposition: "forever_unqualified".to_owned(),
                recorded_at: self.fields["recorded_at"].clone(),
                gap_sha256: self.fields["gap_digest"].clone(),
            },
        )
    }

    /// The projection-only fixture contains no scheduled shadow gaps.
    fn accepted_shadow_gap(&self, _: &[u8]) -> Option<AdjustmentMaintenanceGapReceipt> {
        None
    }
}

impl RainControlStateWireCodec for NodeWitness {
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

    /// No detached monthly stage receipt is involved in this fixture.
    fn decode_stage_receipt(&self, _: &[u8]) -> Option<RainMaintenanceControlStateStageReceipt> {
        None
    }

    /// No monthly abandonment is involved in this fixture.
    fn decode_gap_receipt(&self, _: &[u8]) -> Option<RainMaintenanceControlStateGapReceipt> {
        None
    }
}

impl NativeRevisionSpoolArtifactCodec for NodeWitness {
    /// No monthly ordinal is involved in this fixture.
    fn admitted_rain_control_ordinal_sha256(&self, _: &[u8]) -> Option<String> {
        None
    }
}

impl RevisionGapIdentityAdapter<NativeRevisionGapMember> for NodeWitness {
    /// Return the Node-generated identity only for the exact genesis request.
    fn start_sha256(
        &mut self,
        start: RevisionGapUnsignedStart,
    ) -> Result<RevisionGapSha256, RevisionGapTransferError> {
        assert_eq!(
            start.contract_version,
            REVISION_GAP_TRANSFER_START_CONTRACT_VERSION
        );
        match start.page_count {
            0 => {
                assert_eq!(start.frontier_sha256, self.digest("frontier"));
                Ok(self.digest("start"))
            }
            1 => {
                assert_eq!(start.frontier_sha256, self.digest("ack_frontier"));
                Ok(self.digest("next_start"))
            }
            number => panic!("unexpected fixture page count {number}"),
        }
    }

    /// Bind every typed member and derive size/identities from the independent Node wire.
    fn page_identity(
        &mut self,
        page: RevisionGapUnsignedPage<'_, NativeRevisionGapMember>,
    ) -> Result<AdmittedRevisionGapPageIdentity, RevisionGapTransferError> {
        let expected = self.expected_page();
        assert_eq!(page.contract_version, expected.contract_version);
        assert_eq!(page.entries, expected.entries);
        assert_eq!(page.idle, expected.idle);
        assert_eq!(page.start_sha256, expected.start_sha256);
        assert_eq!(
            page.predecessor_frontier_sha256,
            expected.predecessor_frontier_sha256,
        );
        Ok(AdmittedRevisionGapPageIdentity {
            canonical_byte_count: expected.canonical_byte_count,
            member_root_sha256: expected.member_root_sha256,
            page_sha256: expected.page_sha256,
        })
    }
}

impl NativeRevisionGapTransferCodec for NodeWitness {
    /// Admit the compact frontier only from the exact Node checkpoint encoding.
    fn admitted_frontier(&self, bytes: &[u8]) -> Option<RevisionGapFrontier> {
        let ack = self.admitted_acknowledgement(bytes)?;
        Some(RevisionGapFrontier {
            frontier_sha256: ack.frontier_sha256,
            page_count: ack.page_count,
        })
    }

    /// Admit only the exact native file matching the Node pending witness.
    fn admitted_pending(
        &self,
        bytes: &[u8],
    ) -> Option<RevisionGapPayloadPage<NativeRevisionGapMember>> {
        (bytes == self.bytes("page_bytes")).then(
            // recover the exact typed Node fixture
            || self.expected_page(),
        )
    }

    /// Emit the original wire only after comparing each selected native field.
    fn encode_page(
        &self,
        page: &RevisionGapPayloadPage<NativeRevisionGapMember>,
    ) -> Option<Vec<u8>> {
        (*page == self.expected_page()).then(
            // return exact canonical bytes rather than a Rust substitute
            || self.bytes("page_bytes"),
        )
    }

    /// This stage has no committed predecessor receipt.
    fn single_commit_projection(
        &self,
        _: &[u8],
        _: RevisionGapSha256,
    ) -> Option<RevisionGapSha256> {
        None
    }

    /// This stage has no committed group.
    fn accepted_commit_group(&self, _: &[u8], _: RevisionGapSha256) -> bool {
        false
    }

    /// Admit the exact single normal projection and observe when collection touches its body.
    fn gap_projection(&self, bytes: &[u8]) -> Option<NativeRevisionGapProjection> {
        self.projection_reads.set(self.projection_reads.get() + 1);
        (!self.idle && bytes == self.bytes("payload_bytes")).then_some(
            NativeRevisionGapProjection {
                kind: NativeRevisionGapPayloadKind::Projection,
                rain_control: None,
            },
        )
    }

    /// Bind the original stage receipt to its exact source kind and body identity.
    fn accepted_projection_stage(
        &self,
        bytes: &[u8],
        slot: RevisionGapSha256,
        kind: AdjustmentRevisionProjectionKind,
    ) -> bool {
        bytes == self.bytes("receipt_bytes")
            && slot == self.digest("payload_hash")
            && kind == AdjustmentRevisionProjectionKind::ActualBestMatch
    }

    /// This fixture has no scheduled shadow stage.
    fn gap_shadow_stage(&self, _: &[u8]) -> Option<NativeRevisionGapShadowStage> {
        None
    }
}

impl NativeRevisionGapAckDecoder for NodeWitness {
    /// Retain the checked full canonical Node checkpoint.
    fn admitted_acknowledgement(&self, bytes: &[u8]) -> Option<RevisionGapPayloadAcknowledgement> {
        (!self.idle && bytes == self.bytes("ack_bytes")).then(
            // do not normalize or synthesize foreign input
            || self.expected_acknowledgement(),
        )
    }

    /// Reuse the same accepted producer-derived pending page.
    fn admitted_pending_page(
        &self,
        bytes: &[u8],
    ) -> Option<RevisionGapPayloadPage<NativeRevisionGapMember>> {
        self.admitted_pending(bytes)
    }

    /// Use the independent digest preimages for native verification.
    fn storage_sha256(&self, bytes: &[u8]) -> [u8; 32] {
        self.sha256(bytes)
    }
}

impl RevisionGapAckCodec<NativeRevisionGapMember> for NodeWitness {
    /// Return the exact protected transfer genesis, unmodified.
    fn genesis_frontier_sha256(&self) -> RevisionGapSha256 {
        self.digest("frontier")
    }

    /// Encode only the exact confirmed producer-derived page.
    fn payload_page_bytes(
        &self,
        page: &RevisionGapPayloadPage<NativeRevisionGapMember>,
    ) -> Option<Vec<u8>> {
        self.encode_page(page)
    }

    /// Bind all unsigned native fields to the independently read Node output.
    fn acknowledgement_identity(
        &self,
        unsigned: RevisionGapUnsignedAcknowledgement<'_>,
    ) -> Option<AdmittedRevisionGapAckIdentity> {
        let ack = self.expected_acknowledgement();
        assert_eq!(unsigned.acknowledged_at, ack.acknowledged_at);
        assert_eq!(unsigned.contract_version, ack.contract_version);
        assert_eq!(unsigned.graph_manifest_sha256, ack.graph_manifest_sha256);
        assert_eq!(unsigned.member_root_sha256, ack.member_root_sha256);
        assert_eq!(unsigned.page_count, ack.page_count);
        assert_eq!(unsigned.page_sha256, ack.page_sha256);
        assert_eq!(
            unsigned.predecessor_frontier_sha256,
            ack.predecessor_frontier_sha256
        );
        Some(AdmittedRevisionGapAckIdentity {
            acknowledgement_sha256: ack.acknowledgement_sha256,
            frontier_sha256: ack.frontier_sha256,
        })
    }

    /// Return the original Node acknowledgement bytes for the entire admitted envelope.
    fn acknowledgement_bytes(&self, ack: &RevisionGapPayloadAcknowledgement) -> Option<Vec<u8>> {
        (*ack == self.expected_acknowledgement()).then(
            // forward the original command wire
            || self.bytes("ack_bytes"),
        )
    }

    /// Execute the real Rust retirement mapping for the native sealed page.
    fn retirement_groups(
        &self,
        entry: &RetainedRevisionGapEntry<NativeRevisionGapMember>,
    ) -> Option<Vec<RevisionGapRetirementGroup>> {
        entry.value.retirement_groups()
    }
}

impl NativeRevisionGapCommandCodec for NodeWitness {
    /// Emit the actual Node start before or after acknowledgement.
    fn encode_start(&self, start: &RevisionGapTransferStart) -> Option<Vec<u8>> {
        let label = match start.page_count {
            0 => "start",
            1 => "next_start",
            _ => return None,
        };
        assert_eq!(start.start_sha256, self.digest(label));
        Some(self.bytes(&format!("{label}_bytes")))
    }
}

struct FixedOperatorClock(String);

impl RevisionArchiveCanonicalClock for FixedOperatorClock {
    /// Return the time from the independent Node archive execution.
    fn canonical_now(&mut self) -> Option<String> {
        Some(self.0.clone())
    }
}

/// Freeze and replay the exact reference page using the real on-disk native spool.
#[test]
fn native_gap_page_publishes_the_node_bytes_and_replays_before_collecting() {
    let root = TempSpool::new();
    let mut witness = NodeWitness::seed(root.path(), false);
    let mut spool = NativeRevisionSpool::new(root.path().to_path_buf(), NodeLeaseCodec).unwrap();
    let transfer = root.path().join("revision-gap-transfer");
    let start = spool.read_gap_transfer_start(&mut witness).unwrap();
    assert_eq!(start.start_sha256, witness.digest("start"));
    assert_eq!(start.frontier_sha256, witness.digest("frontier"));
    assert!(!transfer.join("current.json").exists());
    assert!(!transfer.join("pending.json").exists());
    let request = RevisionGapPageRequest {
        frontier_sha256: start.frontier_sha256,
        start_sha256: start.start_sha256,
    };
    let page = spool.read_gap_payload_page(request, &mut witness).unwrap();
    assert_eq!(page, witness.expected_page());
    assert_eq!(
        fs::read(transfer.join("pending.json")).unwrap(),
        witness.bytes("page_bytes"),
    );
    assert_eq!(witness.projection_reads.get(), 1);
    assert_eq!(
        spool.read_gap_payload_page(request, &mut witness).unwrap(),
        page,
    );
    assert_eq!(witness.projection_reads.get(), 1);
    assert!(!root.path().join("revision-spool-locks/spool.lock").exists());
}

/// Execute start, page, and crash-safe ack through the native command owner against Node bytes.
#[test]
fn native_gap_operator_frames_and_retires_the_exact_node_command_members() {
    let root = TempSpool::new();
    let mut witness = NodeWitness::seed(root.path(), false);
    let decoder = witness.clone();
    let mut clock = FixedOperatorClock(decoder.fields["ack_at"].clone());
    let mut ports = NativeRevisionGapCommandPorts {
        codec: &mut witness,
        ack_decoder: &decoder,
        clock: &mut clock,
    };
    let mut spool = NativeRevisionSpool::new(root.path().to_path_buf(), NodeLeaseCodec).unwrap();
    let mut output = Vec::new();
    let start = parse_revision_gap_operator_command("revision-gap-start-v1", &[])
        .unwrap()
        .unwrap();
    let page = parse_revision_gap_operator_command(
        "revision-gap-page-v1",
        &[&decoder.fields["start"], &decoder.fields["frontier"]],
    )
    .unwrap()
    .unwrap();
    let ack = parse_revision_gap_operator_command(
        "revision-gap-ack-v1",
        &[&decoder.fields["page"], &decoder.fields["ack_graph"]],
    )
    .unwrap()
    .unwrap();
    // compare both first and retried output with the independent source command
    for (command, label) in [
        (start, "start_bytes"),
        (page, "page_bytes"),
        (page, "page_bytes"),
        (ack, "ack_bytes"),
        (ack, "ack_bytes"),
        (start, "next_start_bytes"),
    ] {
        output.clear();
        spool
            .run_gap_operator_command(command, &mut ports, &mut output)
            .unwrap();
        assert_eq!(output, decoder.bytes(label));
    }
    assert_eq!(
        fs::read(root.path().join("revision-gap-transfer/current.json")).unwrap(),
        decoder.bytes("ack_bytes"),
    );
    assert!(
        !root
            .path()
            .join("revision-gap-transfer/pending.json")
            .exists()
    );
    // only the exact acknowledged staged values retire
    for dir in ["revision-projections", "revision-stage-receipts"] {
        assert_eq!(fs::read_dir(root.path().join(dir)).unwrap().count(), 0);
    }
    assert_eq!(
        fs::read_dir(root.path().join("revision-gaps"))
            .unwrap()
            .count(),
        1
    );
}

/// Preserve an ordinary in-flight projection without publishing an idle pending cursor.
#[test]
fn native_gap_page_keeps_in_flight_stage_idle_without_writing_a_cursor() {
    let root = TempSpool::new();
    let mut witness = NodeWitness::seed(root.path(), true);
    let mut spool = NativeRevisionSpool::new(root.path().to_path_buf(), NodeLeaseCodec).unwrap();
    let start = spool.read_gap_transfer_start(&mut witness).unwrap();
    let request = RevisionGapPageRequest {
        frontier_sha256: start.frontier_sha256,
        start_sha256: start.start_sha256,
    };
    assert_eq!(
        spool.read_gap_payload_page(request, &mut witness).unwrap(),
        witness.expected_page(),
    );
    assert_eq!(witness.projection_reads.get(), 0);
    assert!(
        !root
            .path()
            .join("revision-gap-transfer/pending.json")
            .exists()
    );
}
