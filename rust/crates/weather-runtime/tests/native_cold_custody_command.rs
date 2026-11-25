#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

#[path = "support/node_primitives.rs"]
mod node_primitives;

#[path = "support/api_archive_relay_wire.rs"]
mod api_archive_relay_wire;
#[path = "support/private_archive_listener.rs"]
mod private_archive_listener;
#[path = "support/private_rain_control_archive.rs"]
mod private_rain_control_archive;
#[path = "support/private_revision_publication.rs"]
mod private_revision_publication;
#[path = "support/private_shadow_publication.rs"]
mod private_shadow_publication;
#[path = "support/public_service_wire.rs"]
mod public_service_wire;
#[path = "support/worker_maintenance_wire.rs"]
mod worker_maintenance_wire;

use node_primitives::{NodePrimitives, script};

use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use weather_contracts::{
    AdjustmentMaintenanceStageInput, AdjustmentMaintenanceStageReceiptContractVersionEnum,
    FieldPresence, JsonValue, MaintenanceShadowPredictionMetadata,
};
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, NativeRevisionSpool, PrivateArchiveTransportRequest,
    PrivateArchiveWireBody, PrivateRevisionArchive, PrivateRevisionProjectionDependency,
    PrivateRevisionProjectionShape, PrivateRevisionReceiptCodec, PrivateShadowArchive,
    RevisionArchiveCanonicalClock, admit_archive_revision_projection,
    parse_revision_cold_ack_command, serve_private_archive_wire_request,
};
use weather_runtime::{
    NativeColdCustodyPorts, NativeColdCustodyRoot, NativeShadowProducerCodec, NativeWirePrimitives,
    bind_native_private_archive_stores, run_native_cold_custody_command,
};

const ACK_TIME: &str = "2026-10-08T03:00:00.000Z";

#[derive(Clone, Copy)]
struct Clock;
impl RevisionArchiveCanonicalClock for Clock {
    /// Freeze only the original ACK clock so both actual stores emit identical bytes.
    fn canonical_now(&mut self) -> Option<String> {
        Some(ACK_TIME.to_owned())
    }
}

struct FixtureRoot(PathBuf);
impl FixtureRoot {
    /// Allocate only the bounded disposable test root inside the admitted scratch.
    fn new(mode: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "weather-native-cold-command-{}-{mode}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("fixture root");
        Self(fs::canonicalize(root).expect("canonical fixture root"))
    }
}

impl Drop for FixtureRoot {
    /// Clean only the fixture-owned disposable root.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

/// Exercise the concrete archive mappings and actual native packed custody command together.
#[test]
fn nonempty_native_page_and_packed_ack_match_original_node() {
    assert_mode("mixed");
}

/// Run the actual compact shadow proof creation and exact retry through the native command.
#[test]
fn shadow_v2_page_ack_and_proof_match_original_node() {
    assert_mode("shadow");
}

/// Run one independently seeded retained page and its real native custody transaction.
fn assert_mode(mode: &str) {
    let fixture = FixtureRoot::new(mode);
    let result = Command::new("node")
        .arg(script())
        .arg("fixtures")
        .arg(&fixture.0)
        .arg(mode)
        .output()
        .expect("actual predecessor fixture");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let operands = fs::read_to_string(fixture.0.join("operands.txt")).unwrap();
    let operands = operands.lines().collect::<Vec<_>>();
    let request = parse_revision_cold_ack_command("revision-cold-custody-ack-v2", &operands)
        .unwrap()
        .expect("original packed arguments");
    let root = fixture.0.join("rust");
    let metadata = fs::metadata(&root).unwrap();
    let codec = ArchiveWireCodec::new(NativeWirePrimitives);
    let spool = NativeRevisionSpool::new(root.clone(), &codec).unwrap();
    let page = spool
        .read_revision_cold_page(request.page, &codec)
        .expect("concrete retained page");
    assert_eq!(page, fs::read(fixture.0.join("page.json")).unwrap());
    let expected = fs::read(fixture.0.join("ack.json")).unwrap();
    let expected_proof = if mode == "shadow" {
        Some(fs::read(fixture.0.join("proof.json")).unwrap())
    } else {
        None
    };

    // run both the first archive retirement and the ordinary exact retry through the real owner
    for _ in 0..2 {
        let mut bytes = Vec::new();
        let handled = run_native_cold_custody_command(
            "revision-cold-custody-ack-v2",
            &operands,
            NativeColdCustodyRoot {
                root: root.clone(),
                uid: metadata.uid(),
                gid: metadata.gid(),
            },
            &codec,
            NativeColdCustodyPorts {
                archive_codec: &codec,
                proof_codec: &mut NativeWirePrimitives,
                time: &NativeWirePrimitives,
                clock: &mut Clock,
            },
            &mut bytes,
        )
        .expect("native packed custody command");
        assert!(handled);
        assert_eq!(bytes, expected);
        assert_eq!(
            fs::read(root.join("revision-custody-acknowledgements/current.json")).unwrap(),
            expected
        );
        // verify exact successful-spool retirement and empty no-shadow proof state
        for directory in [
            "revision-commit-receipts",
            "revision-frontier-successors",
            "revision-projections",
            "revision-publish-receipts",
            "revision-stage-receipts",
            "shadow-revision-capsules",
            "shadow-revision-publish-receipts",
        ] {
            assert_eq!(
                fs::read_dir(root.join(directory)).unwrap().count(),
                0,
                "{directory}"
            );
        }
        let proof_root = root.join("shadow-metadata-custody-proofs");
        // compare the actual durable compact proof for v2, or genuine no-shadow absence
        if let Some(expected_proof) = &expected_proof {
            assert_eq!(
                fs::read(proof_root.join("current.json")).unwrap(),
                *expected_proof
            );
            assert_eq!(fs::read_dir(&proof_root).unwrap().count(), 1);
        } else {
            assert_eq!(fs::read_dir(&proof_root).unwrap().count(), 0);
        }
    }
}

/// Compare the full producer parser and actual native writer to the installed Node owner.
#[test]
fn producer_projection_admission_and_native_stage_match_original_node() {
    let fixture = FixtureRoot::new("producer");
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/producer-projection-node-oracle.mjs");
    let result = Command::new("node")
        .arg(oracle)
        .arg(&fixture.0)
        .output()
        .expect("producer fixture");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let codec = ArchiveWireCodec::new(NodePrimitives);
    let cases = fs::read_to_string(fixture.0.join("cases.tsv")).unwrap();
    // compare each original parser decision and successful archive mapping
    for line in cases.lines() {
        let fields = line.split('\t').collect::<Vec<_>>();
        let [file, accepted, kind, shape, state] = fields.as_slice() else {
            panic!("fixture framing");
        };
        let input = fs::read(fixture.0.join(file)).unwrap();
        let admitted = admit_archive_revision_projection(&codec, &input);
        if *accepted == "no" {
            assert!(
                admitted.is_none(),
                "{file} should refuse ordinary invalid producer geometry"
            );
            continue;
        }
        let admitted = admitted.unwrap_or_else(|| panic!("Node-accepted producer {file} refused"));
        let wire = weather_contracts::AdjustmentRevisionProjectionKind::WIRE_VALUES
            .iter()
            .find_map(|(wire, value)| (*value == admitted.projection_kind).then_some(*wire))
            .unwrap();
        assert_eq!(wire, *kind, "{file}");
        let expected_shape = if *shape == "single" {
            PrivateRevisionProjectionShape::Single
        } else {
            PrivateRevisionProjectionShape::Grouped {
                row_count: shape.parse().unwrap(),
            }
        };
        assert_eq!(admitted.shape, expected_shape, "{file}");
        if *state == "none" {
            assert_eq!(
                admitted.dependency,
                PrivateRevisionProjectionDependency::None,
                "{file}"
            );
        } else {
            assert!(
                matches!(admitted.dependency, PrivateRevisionProjectionDependency::RainControlState {
                ref state_sha256,ref ordinal_artifact_sha256,ref state_stage_receipt_sha256}
                if state_sha256==*state && ordinal_artifact_sha256.as_str()=="c".repeat(64).as_str() && state_stage_receipt_sha256.as_str()=="e".repeat(64).as_str()),
                "{file}"
            );
        }
    }

    // run actual owned native stage operations under exactly the original receipt clock
    for (name, members) in [
        ("ordinary", vec!["actual", "grouped"]),
        ("gauge", vec!["fixed"]),
    ] {
        let root = fixture.0.join(format!("rust-stage-{name}"));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .expect("private stage fixture");
        let storage =
            NativeRevisionSpool::new(root.clone(), ArchiveWireCodec::new(NodePrimitives)).unwrap();
        let mut writer =
            PrivateRevisionArchive::new(storage, ArchiveWireCodec::new(NodePrimitives), Clock);
        // compare the real stage receipt and immutable bytes for each selected original body
        for member in members {
            let raw = fs::read(fixture.0.join(format!("{member}.json"))).unwrap();
            let expected = fs::read(fixture.0.join(format!("stage-{member}.json"))).unwrap();
            let receipt = writer
                .stage(&raw)
                .unwrap_or_else(|error| panic!("native stage {member}: {error:?}"));
            assert_eq!(
                codec.stage_receipt_bytes(&receipt).unwrap(),
                expected,
                "{member}"
            );
            let path = root
                .join("revision-projections")
                .join(format!("sha256-{}.json", receipt.projectionIdentitySha256));
            assert_eq!(fs::read(path).unwrap(), raw, "{member}");
            // retain the first stable receipt on ordinary native retries
            assert_eq!(writer.stage(&raw).unwrap(), receipt);
        }
    }
}

/// Compare all three shadow producer families and ordinary refusals to the deployed Node parser.
#[test]
fn shadow_producer_admission_and_native_stage_match_original_node() {
    let fixture = FixtureRoot::new("shadow-producer");
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/shadow-producer-node-oracle.mjs");
    let result = Command::new("node")
        .arg(oracle)
        .arg(&fixture.0)
        .output()
        .expect("shadow producer fixture");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let manifest = fs::read_to_string(fixture.0.join("shadow-cases.tsv")).unwrap();
    // run each Node verdict through the real Rust producer and native spool
    for line in manifest.lines() {
        let fields = line.split('\t').collect::<Vec<_>>();
        let [name, accepted, version] = fields.as_slice() else {
            panic!("shadow fixture framing");
        };
        let input = shadow_stage_input(&fixture.0, name, *version == "v2");
        let root = fixture.0.join(format!("rust-{name}"));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .expect("shadow stage root");
        let storage =
            NativeRevisionSpool::new(root.clone(), ArchiveWireCodec::new(NodePrimitives)).unwrap();
        let mut writer = PrivateShadowArchive::new(
            storage,
            NativeShadowProducerCodec::new(NodePrimitives),
            Clock,
        );
        let result = writer.stage_shadow_revision(input.clone());
        let wire_request = fs::read(fixture.0.join(format!("{name}-wire-request.json"))).unwrap();
        let decoder = ArchiveWireCodec::new(NodePrimitives);
        let mut stores =
            bind_native_private_archive_stores(root.clone(), NodePrimitives, Clock).unwrap();
        // exercise the full raw listener over the same native stage for every node verdict
        let wire_response = serve_private_archive_wire_request(
            PrivateArchiveTransportRequest {
                method: "POST",
                target: weather_edge::PRIVATE_ARCHIVE_STAGE_TARGET,
                content_type: Some("application/json"),
            },
            &mut PrivateArchiveWireBody::new(&wire_request, &decoder),
            &mut stores,
            &mut ArchiveWireCodec::new(NodePrimitives),
        );
        if *accepted == "no" {
            assert!(result.is_err(), "Node-refused shadow {name} was admitted");
            assert_eq!(wire_response.status_code, 409, "{name} listener refusal");
            continue;
        }
        let receipt = result.unwrap_or_else(|error| panic!("shadow stage {name}: {error:?}"));
        let expected_receipt = fs::read(fixture.0.join(format!("{name}-receipt.json"))).unwrap();
        assert_eq!(wire_response.status_code, 200, "{name} listener admission");
        assert_eq!(
            wire_response.body.as_bytes(),
            expected_receipt,
            "{name} listener receipt"
        );
        assert_shadow_receipt(&receipt, &expected_receipt, version);
        let retained = root
            .join("shadow-revision-stages")
            .join(format!("sha256-{}.json", input.metadata.predictionSha256));
        assert_eq!(
            fs::read(retained).unwrap(),
            fs::read(fixture.0.join(format!("{name}-stage.json"))).unwrap(),
            "{name} retained stage"
        );
        // preserve the first Node-identical receipt across an ordinary exact retry
        assert_eq!(
            writer.stage_shadow_revision(input).unwrap(),
            receipt,
            "{name}"
        );
    }
}

/// Rebuild one generated typed stage input from the oracle's small metadata member.
fn shadow_stage_input(
    root: &Path,
    name: &str,
    comparator: bool,
) -> AdjustmentMaintenanceStageInput {
    let input_bytes = fs::read(root.join(format!("{name}-input.json"))).unwrap();
    let document = NodePrimitives
        .decode_json(&input_bytes)
        .expect("shadow input metadata");
    let metadata = fixture_field(&document, "metadata").expect("metadata object");
    AdjustmentMaintenanceStageInput {
        body: fs::read(root.join(format!("{name}-body.json"))).unwrap(),
        comparator: if comparator {
            FieldPresence::Value(fs::read(root.join(format!("{name}-comparator.json"))).unwrap())
        } else {
            FieldPresence::Missing
        },
        metadata: MaintenanceShadowPredictionMetadata {
            bodyByteCount: fixture_unsigned(metadata, "bodyByteCount"),
            candidateSha256: fixture_text(metadata, "candidateSha256"),
            dueKey: fixture_text(metadata, "dueKey"),
            inputSha256: fixture_text(metadata, "inputSha256"),
            issuedAt: fixture_text(metadata, "issuedAt"),
            maxValidAt: fixture_text(metadata, "maxValidAt"),
            minValidAt: fixture_text(metadata, "minValidAt"),
            predictionBodySha256: fixture_text(metadata, "predictionBodySha256"),
            predictionSchemaSha256: fixture_text(metadata, "predictionSchemaSha256"),
            predictionSha256: fixture_text(metadata, "predictionSha256"),
            registrationSha256: fixture_text(metadata, "registrationSha256"),
            rowCount: fixture_unsigned(metadata, "rowCount"),
            sourceReceiptSha256: fixture_text(metadata, "sourceReceiptSha256"),
            sourceSha256: fixture_text(metadata, "sourceSha256"),
        },
        sourceProjection: fs::read(root.join(format!("{name}-source.json"))).unwrap(),
        sourceProjectionSha256: fixture_text(&document, "sourceProjectionSha256"),
    }
}

/// Compare every stage receipt field without adding a second JSON implementation.
fn assert_shadow_receipt(
    actual: &weather_contracts::AdjustmentMaintenanceStageReceipt,
    expected_bytes: &[u8],
    version: &str,
) {
    let expected = NodePrimitives
        .decode_json(expected_bytes)
        .expect("shadow receipt JSON");
    let expected_version = if version == "v1" {
        AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV1
    } else {
        AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV2
    };
    assert_eq!(actual.contractVersion, expected_version);
    assert_eq!(actual.durableAt, fixture_text(&expected, "durableAt"));
    assert_eq!(actual.dueKey, fixture_text(&expected, "dueKey"));
    assert_eq!(
        actual.predictionBodySha256,
        fixture_text(&expected, "predictionBodySha256")
    );
    assert_eq!(
        actual.registrationSha256,
        fixture_text(&expected, "registrationSha256")
    );
    assert_eq!(
        actual.sourceProjectionSha256,
        fixture_text(&expected, "sourceProjectionSha256")
    );
    assert_eq!(
        actual.stageReceiptSha256,
        fixture_text(&expected, "stageReceiptSha256")
    );
    let comparator = fixture_field(&expected, "comparatorSha256")
        .map(|_| FieldPresence::Value(fixture_text(&expected, "comparatorSha256")))
        .unwrap_or(FieldPresence::Missing);
    assert_eq!(actual.comparatorSha256, comparator);
}

/// Borrow one exact fixture field by its UTF-16 JSON key.
fn fixture_field<'a>(document: &'a JsonValue, name: &str) -> Option<&'a JsonValue> {
    let JsonValue::Object(fields) = document else {
        return None;
    };
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
}

/// Decode one fixture string through the generated UTF-16 representation.
fn fixture_text(document: &JsonValue, name: &str) -> String {
    let JsonValue::String(value) = fixture_field(document, name).expect("fixture field") else {
        panic!("fixture string {name}");
    };
    String::from_utf16(value).expect("fixture UTF-16")
}

/// Decode one safe unsigned fixture integer from the actual Node number.
fn fixture_unsigned(document: &JsonValue, name: &str) -> u64 {
    let JsonValue::Number(value) = fixture_field(document, name).expect("fixture field") else {
        panic!("fixture number {name}");
    };
    assert!(value.is_finite() && *value >= 0.0 && value.fract() == 0.0);
    *value as u64
}

/// Exercise the native process entrypoint against a disposable predecessor-owned archive.
#[test]
fn native_evidence_command_emits_and_reenters_actual_ack() {
    let fixture = FixtureRoot::new("process-mixed");
    let source = Command::new("node")
        .arg(script())
        .arg("fixtures")
        .arg(&fixture.0)
        .arg("mixed")
        .output()
        .expect("predecessor fixture");
    assert!(
        source.status.success(),
        "{}",
        String::from_utf8_lossy(&source.stderr)
    );
    let operands = fs::read_to_string(fixture.0.join("operands.txt")).unwrap();
    let root = fixture.0.join("rust");
    let call = || {
        Command::new(env!("CARGO_BIN_EXE_weather-evidence"))
            .env("WEATHER_ADJUSTMENT_EVIDENCE_ROOT", &root)
            .arg("revision-cold-custody-ack-v2")
            .args(operands.lines())
            .output()
            .unwrap()
    };
    let first = call();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(!first.stdout.is_empty());
    assert!(NativeWirePrimitives.decode_json(&first.stdout).is_some());
    assert_eq!(
        first.stdout,
        fs::read(root.join("revision-custody-acknowledgements/current.json")).unwrap()
    );
    let retry = call();
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_eq!(retry.stdout, first.stdout);
}
