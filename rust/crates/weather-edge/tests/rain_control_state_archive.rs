use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_contracts::{
    JsonValue, RainMaintenanceControlStateGapInput,
    RainMaintenanceControlStateGapInputReasonLiteral, RainMaintenanceControlStateStageReceipt,
    RainMaintenanceControlStateStageReceiptContractVersionLiteral,
    RainMaintenanceControlStateStageReceiptDurableLiteral,
};
use weather_edge::{
    PrivateArchiveRainControlStore, RainControlArchiveMetadataKind, RainControlExclusiveWrite,
    RainControlProjectionBinding, RainControlStateArchive, RainControlStateArchiveCodec,
    RainControlStateArchiveStorage, RainControlStateWireCodec,
    RainMaintenanceControlStateGapReceipt, RevisionArchiveCanonicalClock,
    RevisionArchiveCanonicalCodec, RevisionOnlinePayloadAddress, RevisionOnlinePayloadKind,
    RevisionOnlineSpoolError, RevisionOnlineSpoolStorage,
};

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[path = "support/native_spool.rs"]
mod native_spool;

const STAGE_AT: &str = "2026-09-14T01:05:00.123Z";
const GAP_AT: &str = "2026-09-14T01:09:00.456Z";
type ArchiveResult<T> = Result<T, RevisionOnlineSpoolError>;

struct NodeFixture {
    fields: BTreeMap<String, String>,
    state: Vec<u8>,
}

impl NodeFixture {
    /// Load the actual Node writer's immutable on-disk state and receipt evidence.
    fn load() -> Self {
        let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/rain-control-state-archive-node-oracle.mjs");
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
        let fields: BTreeMap<_, _> = text
            .lines()
            .filter_map(
                // retain one complete named result per line
                |line| {
                    line.split_once('=').map(
                        // own the bounded disposable receipt fixture
                        |(key, value)| (key.to_owned(), value.to_owned()),
                    )
                },
            )
            .collect();
        let state = decode_hex(&fields["body"]);
        Self { fields, state }
    }

    /// Render a stored result line with the canonical final newline.
    fn framed(&self, key: &str) -> String {
        format!("{}\n", self.fields[key])
    }

    /// Reconstruct the decoded producer document used by the shared state verifier.
    fn control_state(&self, include_state_sha256: bool) -> JsonValue {
        let support = json_object(vec![
            ("calibrationDates", JsonValue::Number(90.0)),
            ("calibrationHours", JsonValue::Number(2_000.0)),
            ("calibrationRows", JsonValue::Number(2_000.0)),
            ("calibrationWetDates", JsonValue::Number(20.0)),
            ("calibrationWetHours", JsonValue::Number(200.0)),
            ("effectiveDates64", json_text("404e000000000000")),
            ("effectiveWetDates64", json_text("4008000000000000")),
            ("legacyCalibrationRows", JsonValue::Number(500.0)),
            ("legacyTrainingRows", JsonValue::Number(2_000.0)),
            ("legacyTrainingWetRows", JsonValue::Number(200.0)),
            ("trainingDates", JsonValue::Number(200.0)),
            ("trainingHours", JsonValue::Number(4_000.0)),
            ("trainingRows", JsonValue::Number(4_000.0)),
            ("trainingWetDates", JsonValue::Number(30.0)),
            ("trainingWetHours", JsonValue::Number(300.0)),
        ]);
        let mut fields = vec![
            ("calibrationEndAt", json_text("2026-08-25T00:00:00.000Z")),
            ("calibrationStartAt", json_text("2026-05-27T00:00:00.000Z")),
            (
                "contractVersion",
                json_text("rain-maintenance-control-state/v1"),
            ),
            ("epochWitnessSha256", json_text(&"3".repeat(64))),
            ("generatedAt", json_text("2026-08-26T00:00:00.000Z")),
            (
                "legacyCalibrationStartAt",
                json_text("2026-07-11T00:00:00.000Z"),
            ),
            ("legacyRawScale64", json_text("3ff199999999999a")),
            ("modelMonth", json_text("2026-09")),
            ("ordinalArtifactSha256", json_text(&"4".repeat(64))),
            ("recentFallbackReason", json_text("recent_calibration")),
            ("recentRawScale64", json_text("3ff3333333333333")),
            ("recentSupported", JsonValue::Boolean(true)),
            (
                "recipeSha256",
                json_text(&self.fields["control_recipe_sha256"]),
            ),
            ("sameWindowRawScale64", json_text("3ff2666666666666")),
            ("scheduleContractSha256", json_text(&"5".repeat(64))),
            ("sourceMemberRootSha256", json_text(&"6".repeat(64))),
            ("sourceReceiptRootSha256", json_text(&"7".repeat(64))),
        ];
        // insert the self address only for the complete decoded state
        if include_state_sha256 {
            fields.push((
                "stateSha256",
                json_text(&self.fields["control_state_sha256"]),
            ));
        }
        fields.extend([
            ("support", support),
            (
                "trainingMaximumValidAt",
                json_text("2026-05-19T23:00:00.000Z"),
            ),
        ]);
        json_object(fields)
    }
}

/// Construct one raw UTF-16 JSON string used by the injected decoder.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Construct one insertion-ordered JSON object without a parser dependency.
fn json_object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

impl RevisionArchiveCanonicalCodec for NodeFixture {
    /// Reuse the exact admitted UTC clock values from the Node counterpart.
    fn canonical_utc_milliseconds(&self, value: &str) -> Option<i64> {
        match value {
            STAGE_AT => self.fields["stage_ms"].parse().ok(),
            GAP_AT => self.fields["gap_ms"].parse().ok(),
            "2026-08-25T00:00:00.000Z" => self.fields["control_calibrationEndAt_ms"].parse().ok(),
            "2026-05-27T00:00:00.000Z" => self.fields["control_calibrationStartAt_ms"].parse().ok(),
            "2026-08-26T00:00:00.000Z" => self.fields["control_generatedAt_ms"].parse().ok(),
            "2026-07-11T00:00:00.000Z" => self.fields["control_legacyCalibrationStartAt_ms"]
                .parse()
                .ok(),
            "2026-05-19T23:00:00.000Z" => self.fields["control_trainingMaximumValidAt_ms"]
                .parse()
                .ok(),
            "2026-09-01T00:00:00.000Z" => self.fields["control_monthStart_ms"].parse().ok(),
            _ => None,
        }
    }

    /// Check every actual canonical hash preimage against Node's real staged artifacts.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        // distinguish value bytes from each fixed receipt preimage
        let key = if bytes == self.state {
            "state_digest"
        } else if bytes == self.framed("control_unsigned").as_bytes() {
            "control_state_sha256"
        } else if bytes == self.framed("stage_unsigned").as_bytes() {
            "stage_digest"
        } else {
            assert_eq!(bytes, self.framed("gap_unsigned").as_bytes());
            "gap_digest"
        };
        decode_hex(&self.fields[key])
            .try_into()
            .expect("digest size")
    }
}

impl RainControlStateWireCodec for NodeFixture {
    /// Decode only the actual producer state into its preserved JSON field order.
    fn decode_control_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        (bytes == self.state).then(|| self.control_state(true))
    }

    /// Re-encode only the complete source-order state with its final line feed.
    fn encode_control_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        (value == &self.control_state(true)).then(|| self.state.clone())
    }

    /// Encode the exact unsigned state preimage used by the Node producer.
    fn encode_control_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        (value == &self.control_state(false)).then(|| self.framed("control_unsigned").into_bytes())
    }

    /// Decode only the Node-produced stage receipt from the admitted fixture.
    fn decode_stage_receipt(
        &self,
        bytes: &[u8],
    ) -> Option<RainMaintenanceControlStateStageReceipt> {
        (bytes == self.framed("stage_bytes").as_bytes()).then(
            // populate the exact generated shape supplied by the stable fixture
            || RainMaintenanceControlStateStageReceipt {
                contractVersion: RainMaintenanceControlStateStageReceiptContractVersionLiteral,
                durable: RainMaintenanceControlStateStageReceiptDurableLiteral,
                durableAt: STAGE_AT.to_owned(),
                stageReceiptSha256: self.fields["stage_digest"].clone(),
                stateSha256: self.fields["state_digest"].clone(),
            },
        )
    }

    /// Decode only the Node-produced gap receipt from the admitted fixture.
    fn decode_gap_receipt(&self, bytes: &[u8]) -> Option<RainMaintenanceControlStateGapReceipt> {
        (bytes == self.framed("gap_bytes").as_bytes()).then(
            // retain the exact immutable abandonment fields
            || RainMaintenanceControlStateGapReceipt {
                contract_version: "adjustment-rain-control-state-gap/v1".to_owned(),
                gap_at: GAP_AT.to_owned(),
                gap_sha256: self.fields["gap_digest"].clone(),
                reason: RainMaintenanceControlStateGapInputReasonLiteral,
                stage_receipt_sha256: self.fields["stage_digest"].clone(),
                state_sha256: self.fields["state_digest"].clone(),
            },
        )
    }
}

impl RainControlStateArchiveCodec for NodeFixture {
    /// No projection is staged in the ordinary abandonment workflow.
    fn decode_projection(&self, _bytes: &[u8]) -> Option<RainControlProjectionBinding> {
        unreachable!("unreferenced state")
    }
}

/// Decode fixture hex into the exact admitted value bytes.
fn decode_hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(
            // decode only one exact byte for each fixed pair
            |index| u8::from_str_radix(&value[index..index + 2], 16).expect("oracle hex"),
        )
        .collect()
}

#[derive(Default)]
struct FixtureSpool {
    held: bool,
    content: BTreeMap<RevisionOnlinePayloadAddress, Vec<u8>>,
    metadata: BTreeMap<(RainControlArchiveMetadataKind, String), Vec<u8>>,
    writes: Vec<&'static str>,
}

impl RevisionOnlineSpoolStorage for FixtureSpool {
    /// Keep the whole durable stage or abandonment operation under one lease.
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

    /// Read just the requested bounded state bytes.
    fn read_existing_payload(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> ArchiveResult<Option<Vec<u8>>> {
        assert!(self.held);
        let value = self.content.get(address);
        assert!(value.is_none_or(|value| value.len() <= maximum_bytes));
        Ok(value.cloned())
    }

    /// Enumerate only the normal retained members in the selected fixed directory.
    fn list_payload_names(
        &mut self,
        kind: RevisionOnlinePayloadKind,
    ) -> ArchiveResult<Vec<String>> {
        assert!(self.held);
        Ok(self
            .content
            .keys()
            .filter_map(
                // preserve the type-bound basename in the admitted directory
                |address| {
                    let (directory, name) = address.components();
                    (directory == kind).then_some(name.to_owned())
                },
            )
            .collect())
    }

    /// Read the exact validated storage length for capacity.
    fn payload_file_length(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> ArchiveResult<usize> {
        assert!(self.held);
        let size = self.content[address].len();
        assert!(size <= maximum_bytes);
        Ok(size)
    }
}

impl RainControlStateArchiveStorage for FixtureSpool {
    /// Verify one admitted private root ahead of storage work.
    fn ensure_revision_root(&mut self) -> ArchiveResult<()> {
        assert!(!self.held);
        Ok(())
    }

    /// Retain one exact immutable state before creating an availability receipt.
    fn write_state_exclusive(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> ArchiveResult<()> {
        assert!(self.held);
        self.writes.push("state");
        assert!(
            self.content
                .insert(address.clone(), bytes.to_vec())
                .is_none()
        );
        Ok(())
    }

    /// Read the same first retained receipt without invoking the clock.
    fn read_metadata(
        &mut self,
        kind: RainControlArchiveMetadataKind,
        digest: &str,
        maximum_bytes: usize,
    ) -> ArchiveResult<Option<Vec<u8>>> {
        assert!(self.held);
        assert_eq!(maximum_bytes, 4096);
        Ok(self.metadata.get(&(kind, digest.to_owned())).cloned())
    }

    /// Retain one exact canonical immutable receipt for the actual archive engine.
    fn write_metadata_exclusive(
        &mut self,
        kind: RainControlArchiveMetadataKind,
        digest: &str,
        bytes: &[u8],
    ) -> ArchiveResult<RainControlExclusiveWrite> {
        assert!(self.held);
        self.writes.push(kind.directory());
        assert!(
            self.metadata
                .insert((kind, digest.to_owned()), bytes.to_vec())
                .is_none()
        );
        Ok(RainControlExclusiveWrite::Created)
    }

    /// The normal abandonment fixture selected no pending projection.
    fn read_projection(&mut self, _name: &str, _maximum: usize) -> ArchiveResult<Vec<u8>> {
        unreachable!("no active projection")
    }
}

struct FixtureClock(Rc<Cell<usize>>);

impl RevisionArchiveCanonicalClock for FixtureClock {
    /// Witness the first stage and first abandonment clocks in source order.
    fn canonical_now(&mut self) -> Option<String> {
        let index = self.0.get();
        self.0.set(index + 1);
        [STAGE_AT, GAP_AT].get(index).map(
            // preserve one admitted exact UTC instant
            |value| (*value).to_owned(),
        )
    }
}

#[test]
// compare the real private stage and abandonment application bindings with Node durable bytes
fn matches_actual_monthly_state_stage_and_gap_receipts_on_retries() {
    let fixture = NodeFixture::load();
    let state = fixture.state.clone();
    let state_digest = fixture.fields["state_digest"].clone();
    let expected_stage = fixture.framed("stage_bytes").into_bytes();
    let expected_gap = fixture.framed("gap_bytes").into_bytes();
    let clock = Rc::new(Cell::new(0));
    let mut archive = RainControlStateArchive::new(
        FixtureSpool::default(),
        fixture,
        FixtureClock(Rc::clone(&clock)),
    );
    let stage = archive
        .stage_rain_control_state(state.clone())
        .expect("stage");
    assert_eq!(stage.stateSha256, state_digest);
    assert_eq!(stage.durableAt, STAGE_AT);
    assert_eq!(
        archive.stage_rain_control_state(state.clone()),
        Ok(stage.clone())
    );
    let input = RainMaintenanceControlStateGapInput {
        reason: RainMaintenanceControlStateGapInputReasonLiteral,
        stageReceiptSha256: stage.stageReceiptSha256,
        stateSha256: state_digest.clone(),
    };
    let gap = archive
        .record_rain_control_state_gap(input.clone())
        .expect("gap");
    assert_eq!(gap.gap_at, GAP_AT);
    assert_eq!(archive.record_rain_control_state_gap(input), Ok(gap));
    assert_eq!(clock.get(), 2);
    let spool = archive.into_storage();
    assert_eq!(
        spool.writes,
        [
            "state",
            "rain-control-state-stage-receipts",
            "rain-control-state-gaps"
        ]
    );
    let address = RevisionOnlinePayloadAddress::from_sha256(
        RevisionOnlinePayloadKind::RainControlState,
        &state_digest,
    )
    .expect("state address");
    assert_eq!(spool.content[&address], state);
    assert_eq!(
        spool.metadata,
        BTreeMap::from([
            (
                (
                    RainControlArchiveMetadataKind::StageReceipt,
                    state_digest.clone()
                ),
                expected_stage
            ),
            (
                (RainControlArchiveMetadataKind::GapReceipt, state_digest),
                expected_gap
            ),
        ]),
    );
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
// prove the actual filesystem adapter writes the Node-matched immutable receipts in place
fn native_spool_durably_stages_and_abandons_monthly_rain_state() {
    use native_spool::{NodeLeaseCodec, TempSpool};
    use weather_edge::NativeRevisionSpool;

    let temp = TempSpool::new();
    let fixture = NodeFixture::load();
    let state = fixture.state.clone();
    let expected_stage = fixture.framed("stage_bytes").into_bytes();
    let expected_gap = fixture.framed("gap_bytes").into_bytes();
    let clock = Rc::new(Cell::new(0));
    let spool = NativeRevisionSpool::new(temp.path().to_path_buf(), NodeLeaseCodec)
        .expect("actual native storage");
    let mut archive = RainControlStateArchive::new(spool, fixture, FixtureClock(Rc::clone(&clock)));
    let stage = archive
        .stage_rain_control_state(state.clone())
        .expect("stage");
    let input = RainMaintenanceControlStateGapInput {
        reason: RainMaintenanceControlStateGapInputReasonLiteral,
        stageReceiptSha256: stage.stageReceiptSha256,
        stateSha256: stage.stateSha256,
    };
    let recorded = archive
        .record_rain_control_state_gap(input.clone())
        .expect("gap");
    assert_eq!(
        archive.record_rain_control_state_gap(input.clone()),
        Ok(recorded)
    );
    assert_eq!(clock.get(), 2);
    let filename = format!("sha256-{}.json", input.stateSha256);
    // verify the exact bytes and zero retained temporary files in all three directories
    for (subdirectory, expected) in [
        ("rain-control-states", state),
        ("rain-control-state-stage-receipts", expected_stage),
        ("rain-control-state-gaps", expected_gap),
    ] {
        let directory = temp.path().join(subdirectory);
        assert_eq!(
            std::fs::read(directory.join(&filename)).expect("native file"),
            expected
        );
        assert_eq!(
            std::fs::read_dir(directory)
                .expect("native directory")
                .count(),
            1
        );
    }
    assert!(!temp.path().join("revision-spool-locks/spool.lock").exists());
}
