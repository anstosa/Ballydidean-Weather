#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_contracts::{
    AdjustmentMaintenanceGapInput, AdjustmentMaintenanceGapInputFamilyEnum,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionGapInput,
    AdjustmentRevisionGapReason, AdjustmentRevisionProjectionKind, MaintenanceShadowGapReason,
    Nullable, RainMaintenanceControlStateStageReceipt,
};
use weather_edge::{
    AdjustmentMaintenanceGapReceipt, AdjustmentRevisionGapReceipt, NativeRevisionSpool,
    NativeRevisionSpoolArtifactCodec, PermanentArchiveGapCodec, PermanentArchiveGapInput,
    PermanentArchiveGapStore, PrivateArchivePermanentGapStore, PrivateRevisionArchiveStorage,
    PrivateShadowArchiveStorage, RainControlStateWireCodec, RainMaintenanceControlStateGapReceipt,
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, RevisionSpoolLeaseCodec,
};

#[path = "support/native_spool.rs"]
mod native_spool;
use native_spool::{NodeLeaseCodec, TempSpool};

const TIME: &str = "2026-09-14T01:05:00.123Z";
const LABELS: [&str; 3] = ["shadow", "stage", "publish"];

#[derive(Clone)]
struct Fixture(BTreeMap<String, String>);

impl Fixture {
    /// Read the exact Node-produced retained files and canonical preimages.
    fn load() -> Self {
        let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/permanent-archive-gap-node-oracle.mjs");
        let output = Command::new("node")
            .arg(oracle)
            .output()
            .expect("Node gap oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("oracle UTF-8");
        Self(
            stdout
                .lines()
                .filter_map(
                    // retain one complete fixture result per emitted line
                    |line| {
                        line.split_once('=').map(
                            // convert to owned test data
                            |(key, value)| (key.to_owned(), value.to_owned()),
                        )
                    },
                )
                .collect(),
        )
    }

    /// Return one exact test-only Node witness.
    fn field(&self, label: &str, kind: &str) -> &str {
        &self.0[&format!("{label}_{kind}")]
    }

    /// Return a canonical newline-framed fixture document.
    fn bytes(&self, label: &str, kind: &str) -> Vec<u8> {
        format!("{}\n", self.field(label, kind)).into_bytes()
    }
}

impl RevisionArchiveCanonicalCodec for Fixture {
    /// Reuse the duration-independent archived clock from the producer.
    fn canonical_utc_milliseconds(&self, value: &str) -> Option<i64> {
        // match only the existing canonical clock for this normal fixture
        if value != TIME {
            return None;
        }
        self.0["clock_ms"].parse().ok()
    }

    /// Check all requested digest preimages against the actual Node output.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        // select one exact canonical key or unsigned receipt
        for label in LABELS {
            // the key is content-addressed by the Node-produced basename
            if bytes == self.bytes(label, "key") {
                let name = self.field(label, "filename");
                return digest(&name[7..71]);
            }
            // the receipt digest is derived independently in the Node store
            if bytes == self.bytes(label, "unsigned") {
                return digest(self.field(label, "digest"));
            }
        }
        panic!("unexpected immutable gap digest preimage")
    }
}

impl PermanentArchiveGapCodec for Fixture {
    /// Encode the exact Node-derived locator for this admitted fixture input.
    fn encode_gap_key(&self, input: PermanentArchiveGapInput<'_>) -> Option<Vec<u8>> {
        fixture_label(input).map(
            // retain Node fixture key bytes without a Rust JSON implementation
            |label| self.bytes(label, "key"),
        )
    }

    /// Supply complete canonical output or its unsigned original producer bytes.
    fn encode_gap_receipt(
        &self,
        input: PermanentArchiveGapInput<'_>,
        recorded_at: &str,
        digest: Option<&str>,
    ) -> Option<Vec<u8>> {
        assert_eq!(recorded_at, TIME);
        let label = fixture_label(input)?;
        // require the exact previously computed digest for the signed bytes
        if let Some(digest) = digest {
            assert_eq!(digest, self.field(label, "digest"));
            return Some(self.bytes(label, "bytes"));
        }
        Some(self.bytes(label, "unsigned"))
    }

    /// Admit the original typed projection disposition from its exact Node bytes.
    fn accepted_revision_gap(&self, bytes: &[u8]) -> Option<AdjustmentRevisionGapReceipt> {
        // distinguish the unbound pre-stage gap from the later body-bound state
        for (label, body_bound) in [("stage", false), ("publish", true)] {
            // match only the original canonical fixture
            if bytes == self.bytes(label, "bytes") {
                return Some(AdjustmentRevisionGapReceipt {
                    contract_version: "adjustment-revision-gap/v1".to_owned(),
                    input: revision(body_bound),
                    qualification_disposition: "forever_unqualified".to_owned(),
                    recorded_at: TIME.to_owned(),
                    gap_sha256: self.field(label, "digest").to_owned(),
                });
            }
        }
        None
    }

    /// Admit the original scheduled shadow disposition from its exact Node bytes.
    fn accepted_shadow_gap(&self, bytes: &[u8]) -> Option<AdjustmentMaintenanceGapReceipt> {
        (bytes == self.bytes("shadow", "bytes")).then(
            // decode the normal scheduled workflow's typed receipt fixture
            || AdjustmentMaintenanceGapReceipt {
                contract_version: "adjustment-shadow-gap/v1".to_owned(),
                input: shadow(),
                qualification_disposition: "forever_unqualified".to_owned(),
                recorded_at: TIME.to_owned(),
                gap_sha256: self.field("shadow", "digest").to_owned(),
            },
        )
    }
}

impl RevisionSpoolLeaseCodec for Fixture {
    /// Reuse the exact test-only Node lease grammar.
    fn encode_canonical_lease(&self, milliseconds: i64, token: &str) -> Option<Vec<u8>> {
        NodeLeaseCodec.encode_canonical_lease(milliseconds, token)
    }

    /// Read an original lease through the normal fixture codec.
    fn validated_lease_expiry(&self, bytes: &[u8]) -> Option<i64> {
        NodeLeaseCodec.validated_lease_expiry(bytes)
    }
}

impl RainControlStateWireCodec for Fixture {
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

    /// The permanent scanner does not admit a state stage receipt.
    fn decode_stage_receipt(&self, _: &[u8]) -> Option<RainMaintenanceControlStateStageReceipt> {
        unreachable!("value-free gap fixture")
    }

    /// The permanent scanner does not admit rain-control gap receipts.
    fn decode_gap_receipt(&self, _: &[u8]) -> Option<RainMaintenanceControlStateGapReceipt> {
        unreachable!("value-free gap fixture")
    }
}

impl NativeRevisionSpoolArtifactCodec for Fixture {
    /// This ordinary scheduled or projection-gap read has no rain control dependency.
    fn admitted_rain_control_ordinal_sha256(&self, _: &[u8]) -> Option<String> {
        unreachable!("value-free gap fixture")
    }
}

/// Bind only the three actual Node fixture input variants.
fn fixture_label(input: PermanentArchiveGapInput<'_>) -> Option<&'static str> {
    match input {
        PermanentArchiveGapInput::Shadow(value) if value == &shadow() => Some("shadow"),
        PermanentArchiveGapInput::Revision(value) if value == &revision(false) => Some("stage"),
        PermanentArchiveGapInput::Revision(value) if value == &revision(true) => Some("publish"),
        _ => None,
    }
}

/// Decode one retained Node SHA digest into bytes.
fn digest(value: &str) -> [u8; 32] {
    std::array::from_fn(
        // decode only each exact lowercase hex pair
        |index| u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).expect("oracle hex"),
    )
}

/// Build the supported value-free scheduled failure.
fn shadow() -> AdjustmentMaintenanceGapInput {
    AdjustmentMaintenanceGapInput {
        dueKey: "capture/2026-09-14T00:35:00.000Z".to_owned(),
        family: AdjustmentMaintenanceGapInputFamilyEnum::Wind,
        reason: MaintenanceShadowGapReason::CandidateUnavailable,
        registrationSha256: "a".repeat(64),
    }
}

/// Build either actual pre-stage or committed-body disposition.
fn revision(body_bound: bool) -> AdjustmentRevisionGapInput {
    // the pre-stage path has no archived content identity yet
    let projection = if body_bound {
        Nullable::Value("c".repeat(64))
    } else {
        Nullable::Null
    };
    AdjustmentRevisionGapInput {
        logicalKeySha256: "b".repeat(64),
        projectionIdentitySha256: projection.clone(),
        projectionKind: AdjustmentRevisionProjectionKind::ActualBestMatch,
        projectionSha256: projection,
        // preserve the actual categorical outcome of each admitted step
        reason: if body_bound {
            AdjustmentRevisionGapReason::ArchivePublishFailed
        } else {
            AdjustmentRevisionGapReason::ArchiveStageFailed
        },
    }
}

struct Clock(Rc<Cell<usize>>);

impl RevisionArchiveCanonicalClock for Clock {
    /// Capture one normal first-persistence timestamp, not a retry time.
    fn canonical_now(&mut self) -> Option<String> {
        self.0.set(self.0.get() + 1);
        Some(TIME.to_owned())
    }
}

#[test]
// compare all three normal immutable records with actual Node files over real native storage
fn native_permanent_gaps_keep_node_bytes_addresses_and_first_clocks() {
    let temp = TempSpool::new();
    let fixture = Fixture::load();
    let expected = LABELS.map(
        // retain the exact Node address and durable bytes for each supported case
        |label| {
            (
                label,
                fixture.field(label, "filename").to_owned(),
                fixture.bytes(label, "bytes"),
            )
        },
    );
    let read_fixture = fixture.clone();
    let native =
        NativeRevisionSpool::new(temp.path().to_path_buf(), NodeLeaseCodec).expect("native spool");
    let clock = Rc::new(Cell::new(0));
    let mut archive = PermanentArchiveGapStore::new(native, fixture, Clock(Rc::clone(&clock)));
    let staged = archive
        .record_shadow_gap(shadow())
        .expect("normal scheduled gap");
    assert_eq!(archive.record_shadow_gap(shadow()), Ok(staged));
    // preserve both exact pre-stage and body-bound records without re-reading time
    for body_bound in [false, true] {
        let first = archive
            .record_revision_gap(revision(body_bound))
            .expect("revision gap");
        assert_eq!(archive.record_revision_gap(revision(body_bound)), Ok(first));
    }
    assert_eq!(clock.get(), 3);
    // verify the actual bytes committed under the exact two native directories
    for (label, name, bytes) in expected {
        let directory = if label == "shadow" {
            "shadow-revision-gaps"
        } else {
            "revision-gaps"
        };
        assert_eq!(
            std::fs::read(temp.path().join(directory).join(name)).expect("native data"),
            bytes
        );
    }
    assert!(!temp.path().join("revision-spool-locks/spool.lock").exists());
    let mut reader = NativeRevisionSpool::new(temp.path().to_path_buf(), read_fixture)
        .expect("second retained owner");
    assert_eq!(
        reader.has_permanent_revision_gap(
            &"c".repeat(64),
            AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
        ),
        Ok(true),
    );
    assert_eq!(
        reader.has_permanent_shadow_gap(&shadow().dueKey, &shadow().registrationSha256),
        Ok(true),
    );
}
