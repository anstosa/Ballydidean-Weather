use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_contracts::{
    RainFixedGaugeTargetGapInput, RainFixedGaugeTargetGapInputReasonLiteral,
    RainFixedGaugeTargetGapReceipt, RainFixedGaugeTargetGapReceiptContractVersionLiteral,
    RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral, RainFixedGaugeTargetGapStatus,
    RainFixedGaugeTargetGapStatusInput,
};
use weather_edge::{
    RainFixedGaugeGapArchive, RainFixedGaugeGapArchiveError, RainFixedGaugeGapCodec,
    RainFixedGaugeGapFileName, RainFixedGaugeGapStorage, RevisionArchiveCanonicalClock,
    RevisionArchiveCanonicalCodec,
};

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[path = "support/native_spool.rs"]
mod native_spool;

const HOUR: &str = "2026-09-14T01:00:00.000Z";
const GAP_AT: &str = "2026-09-14T01:05:00.123Z";

struct NodeFixture {
    key: Vec<u8>,
    digest: String,
    utc_hour_ms: i64,
    utc_clock_ms: i64,
    filename: String,
    receipt: Vec<u8>,
    unsigned: Vec<u8>,
}

impl NodeFixture {
    /// Read actual Node output from its disposable on-disk private spool.
    fn load() -> Self {
        let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/rain-fixed-gauge-gap-node-oracle.mjs");
        let output = Command::new("node")
            .arg(oracle)
            .output()
            .expect("Node oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = String::from_utf8(output.stdout).expect("oracle UTF-8");
        let fields: BTreeMap<_, _> = output.lines().filter_map(fixture_pair).collect();
        Self {
            key: format!("{}\n", fields["key_preimage"]).into_bytes(),
            digest: fields["digest"].to_owned(),
            utc_hour_ms: fields["key_ms"].parse().expect("oracle hour epoch"),
            utc_clock_ms: fields["clock_ms"].parse().expect("oracle clock epoch"),
            filename: fields["filename"].to_owned(),
            receipt: format!("{}\n", fields["receipt"]).into_bytes(),
            unsigned: format!("{}\n", fields["unsigned"]).into_bytes(),
        }
    }
}

impl RevisionArchiveCanonicalCodec for NodeFixture {
    /// Reuse the admitted Node times without adding a civil time parser to Rust.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        match text {
            HOUR => Some(self.utc_hour_ms),
            GAP_AT => Some(self.utc_clock_ms),
            _ => None,
        }
    }

    /// Assert the exact hash preimages and reuse the real archived fixture digests.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        // select the real key digest without using Rust fixture serialization as evidence
        let digest = if bytes == self.key {
            self.filename
                .strip_prefix("sha256-")
                .and_then(
                    // unwrap only the fixed oracle filename suffix
                    |text| text.strip_suffix(".json"),
                )
                .expect("oracle filename")
        } else {
            assert_eq!(bytes, self.unsigned);
            &self.digest
        };
        std::array::from_fn(
            // decode one exact digest byte from Node hex
            |index| u8::from_str_radix(&digest[index * 2..index * 2 + 2], 16).expect("hex"),
        )
    }
}

impl RainFixedGaugeGapCodec for NodeFixture {
    /// Admit only the exact receipt bytes emitted by the actual Node fixture.
    fn decode_receipt(&self, bytes: &[u8]) -> Option<RainFixedGaugeTargetGapReceipt> {
        (bytes == self.receipt).then(|| {
            // populate the fixed generated receipt shape supplied by the test codec
            RainFixedGaugeTargetGapReceipt {
                logicalHourAt: HOUR.to_owned(),
                logicalKeySha256: "a".repeat(64),
                reason: RainFixedGaugeTargetGapInputReasonLiteral,
                contractVersion: RainFixedGaugeTargetGapReceiptContractVersionLiteral,
                gapAt: GAP_AT.to_owned(),
                gapSha256: self.digest.clone(),
                qualificationDisposition:
                    RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral,
            }
        })
    }
}

/// Split one machine-readable Node result line.
fn fixture_pair(line: &str) -> Option<(&str, &str)> {
    line.split_once('=')
}

#[derive(Default)]
struct FixtureStorage {
    locked: bool,
    files: BTreeMap<String, Vec<u8>>,
    names_override: Option<Vec<String>>,
    trace: Vec<&'static str>,
}

impl RainFixedGaugeGapStorage for FixtureStorage {
    /// Observe readiness of the explicitly supplied private spool.
    fn ensure_revision_root(&mut self) -> Result<(), RainFixedGaugeGapArchiveError> {
        assert!(!self.locked);
        self.trace.push("root");
        Ok(())
    }

    /// Preserve read, census, and write in one explicit storage critical section.
    fn with_revision_spool_mutation<T>(
        &mut self,
        mutation: impl FnOnce(&mut Self) -> Result<T, RainFixedGaugeGapArchiveError>,
    ) -> Result<T, RainFixedGaugeGapArchiveError> {
        assert!(!self.locked);
        self.trace.push("lock");
        self.locked = true;
        let result = mutation(self);
        self.locked = false;
        self.trace.push("unlock");
        result
    }

    /// Return the optional stored canonical bytes and confirm the fixed bound.
    fn read_gap_file(
        &mut self,
        name: &RainFixedGaugeGapFileName,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RainFixedGaugeGapArchiveError> {
        assert_eq!(maximum_bytes, 4096);
        self.trace.push("read");
        Ok(self.files.get(name.as_str()).cloned())
    }

    /// Supply a bounded ordinary index census under the shared mutation lease.
    fn list_gap_names(
        &mut self,
        maximum_names: usize,
    ) -> Result<Vec<String>, RainFixedGaugeGapArchiveError> {
        assert!(self.locked);
        assert_eq!(maximum_names, 4096);
        self.trace.push("list");
        Ok(self.names_override.clone().unwrap_or_else(
            // return only the bounded fixture index
            || self.files.keys().cloned().collect(),
        ))
    }

    /// Save the actual bytes supplied by the Rust archive.
    fn write_gap_exclusive(
        &mut self,
        name: &RainFixedGaugeGapFileName,
        bytes: &[u8],
    ) -> Result<(), RainFixedGaugeGapArchiveError> {
        assert!(self.locked);
        self.trace.push("write");
        assert!(
            self.files
                .insert(name.as_str().to_owned(), bytes.to_vec())
                .is_none()
        );
        Ok(())
    }
}

struct FixtureClock(Rc<Cell<usize>>);

impl RevisionArchiveCanonicalClock for FixtureClock {
    /// Capture one admitted exact first-write instant.
    fn canonical_now(&mut self) -> Option<String> {
        self.0.set(self.0.get() + 1);
        Some(GAP_AT.to_owned())
    }
}

/// Build the supported normal oversized target disposition.
fn input() -> RainFixedGaugeTargetGapInput {
    RainFixedGaugeTargetGapInput {
        logicalHourAt: HOUR.to_owned(),
        logicalKeySha256: "a".repeat(64),
        reason: RainFixedGaugeTargetGapInputReasonLiteral,
    }
}

#[test]
// compare ordinary absent, first durable receipt, same-key retry, and present lookup to Node
fn matches_node_permanent_gap_bytes_and_retry_clock() {
    let fixture = NodeFixture::load();
    let name = fixture.filename.clone();
    let bytes = fixture.receipt.clone();
    let clock = Rc::new(Cell::new(0));
    let mut archive = RainFixedGaugeGapArchive::new(
        FixtureStorage::default(),
        fixture,
        FixtureClock(Rc::clone(&clock)),
    );
    let input = input();
    let key = RainFixedGaugeTargetGapStatusInput {
        logicalHourAt: input.logicalHourAt.clone(),
        logicalKeySha256: input.logicalKeySha256.clone(),
    };
    assert!(matches!(
        archive.status(&key),
        Ok(RainFixedGaugeTargetGapStatus::Absent(_))
    ));
    let first = archive.record(&input).expect("record");
    assert_eq!(archive.record(&input), Ok(first.clone()));
    let Ok(RainFixedGaugeTargetGapStatus::Present(status)) = archive.status(&key) else {
        panic!("expected retained gap");
    };
    assert_eq!(status.gap.gapSha256, first.gapSha256);
    assert_eq!(status.gap.gapAt, GAP_AT);
    assert_eq!(clock.get(), 1);
    let storage = archive.into_storage();
    assert_eq!(storage.files, BTreeMap::from([(name, bytes)]));
    assert_eq!(
        storage.trace,
        [
            "root", "read", "root", "lock", "read", "list", "write", "unlock", "root", "lock",
            "read", "unlock", "root", "read",
        ]
    );
}

#[test]
// preserve permanent rows rather than evicting one when a new key hits the capacity bound
fn rejects_a_new_gap_at_capacity_before_reading_the_write_clock() {
    let fixture = NodeFixture::load();
    let clock = Rc::new(Cell::new(0));
    let names = (0..4096)
        .map(
            // generate ordinary named permanent metadata entries
            |index| format!("sha256-{index:064x}.json"),
        )
        .collect();
    let mut archive = RainFixedGaugeGapArchive::new(
        FixtureStorage {
            names_override: Some(names),
            ..FixtureStorage::default()
        },
        fixture,
        FixtureClock(Rc::clone(&clock)),
    );
    assert_eq!(
        archive.record(&input()),
        Err(RainFixedGaugeGapArchiveError::Census)
    );
    assert_eq!(clock.get(), 0);
    assert_eq!(
        archive.into_storage().trace,
        ["root", "lock", "read", "list", "unlock"]
    );
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
// prove the actual retained Node reader and retry share native files and lease bytes
fn native_spool_is_byte_compatible_with_node_lease_and_fixed_gauge_retry() {
    use native_spool::{NodeLeaseCodec, TempSpool};
    use weather_edge::NativeRevisionSpool;

    let temp = TempSpool::new();
    let mut spool = NativeRevisionSpool::new(temp.path().to_path_buf(), NodeLeaseCodec)
        .expect("actual native storage");
    spool
        .ensure_revision_directories()
        .expect("native custody tree");
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/native-revision-spool-node-oracle.mjs");
    spool
        .with_revision_spool_mutation(
            // observe the held native lease via the retained process's JSON/UTC behavior
            |_| {
                let output = Command::new("node")
                    .arg(&oracle)
                    .arg(temp.path())
                    .arg("lease")
                    .output()
                    .expect("Node lease witness");
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(output.stdout, b"adjustment-revision-spool-lock/v1\n");
                Ok(())
            },
        )
        .expect("native revision lease");
    assert!(!temp.path().join("revision-spool-locks/spool.lock").exists());

    let fixture = NodeFixture::load();
    let path = temp
        .path()
        .join("rain-fixed-gauge-target-gaps")
        .join(&fixture.filename);
    let expected = fixture.receipt.clone();
    let clock = Rc::new(Cell::new(0));
    let mut archive =
        RainFixedGaugeGapArchive::new(spool, fixture, FixtureClock(Rc::clone(&clock)));
    let first = archive
        .record(&input())
        .expect("native fixed gauge disposition");
    assert_eq!(
        std::fs::read(path).expect("native durable receipt"),
        expected
    );
    let output = Command::new("node")
        .arg(oracle)
        .arg(temp.path())
        .arg("retry")
        .output()
        .expect("retained Node archive");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("oracle UTF-8"),
        format!("{}|{}\n", first.gapAt, first.gapSha256),
    );
    assert_eq!(archive.record(&input()), Ok(first));
    assert_eq!(clock.get(), 1);
    assert!(!temp.path().join("revision-spool-locks/spool.lock").exists());
}
