use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use super::*;
use crate::native_maintenance_development_remote::{
    NativeMaintenanceDevelopmentAnchorInstallRequest, NativeMaintenanceFamilyReleaseApplyInput,
    build_native_maintenance_family_release_apply_request,
    verify_native_maintenance_development_anchor_installation,
    verify_native_maintenance_family_release_apply_status,
};
use crate::{
    NativeMaintenanceArchiveSegment, NativeMaintenanceArchiveSegmentMember,
    NativeMaintenanceDevelopmentPublicationFile, NativeMaintenanceDueMode,
    NativeMaintenanceGraphEntry, NativeMaintenanceGraphManifest, NativeMaintenanceJournalAddition,
    NativeWirePrimitives,
};

const NODE_FIXTURE: &str = r#"
import {
  buildAdjustmentDevelopmentCustodyAnchor,
} from "./scripts/research/adjustment_maintenance_controller.mjs";
import {
  buildAdjustmentModelAction,
} from "./scripts/research/adjustment_model_release.mjs";
import {
  adjustmentSha256,
  canonicalJsonBytes,
} from "./scripts/research/adjustment_plaintext_archive.mjs";

const hash = (character) => character.repeat(64);
const sourceCommit = "a".repeat(40);
const sourceRelease = "2027.02.01-1";
const candidateGraphSha256 = hash("c");
const candidateSha256 = hash("d");
const policyReportSha256 = hash("4");
const target = buildAdjustmentModelAction({
  actionKind: "shadow",
  candidateGraphSha256,
  candidateSha256,
  contractVersion: "forecast-adjustment-model-action/v1",
  createdAt: "2027-02-03T12:00:00.000Z",
  expectedInstalledReceiptSha256: hash("8"),
  expectedSettingsSha256: hash("9"),
  expectedSourceCommit: sourceCommit,
  expectedSourceRelease: sourceRelease,
  family: "wind",
  fencingToken: "7",
  fullMemberRootSha256: null,
  lifecycleHeadSha256: hash("3"),
  policyDecision: "pending",
  policyReportSha256,
  predecessorActionSha256: null,
  reason: "development_candidate",
  reportCreatedAt: "2027-02-03T11:00:00.000Z",
  siteKey: "ballydidean",
  validThrough: "2027-02-10T11:00:00.000Z",
});
const binding = {
  actionSha256: target.actionSha256,
  artifactSha256: hash("b"),
  candidateGraphSha256,
  candidateSha256,
  contractVersion: "adjustment-development-custody-graph/v1",
  policyReportSha256,
  registrationSha256: hash("5"),
  sourceSha256: hash("6"),
};
const bindingBytes = canonicalJsonBytes(binding);
const bindingSha256 = adjustmentSha256(bindingBytes);
const unsignedAcknowledgement = {
  acknowledgedAt: "2027-02-03T12:01:00.000Z",
  afterArchiveCommitOrdinal: "10",
  afterFrontierSha256: hash("1"),
  authority: "cold_custody_only",
  contractVersion: "adjustment-revision-custody-acknowledgement/v2",
  custodyCheckpointSha256: hash("2"),
  memberRootSha256: hash("3"),
  nextArchiveCommitOrdinal: "11",
  nextFrontierSha256: hash("4"),
  pageSha256: hash("5"),
  previousAcknowledgementSha256: hash("6"),
  previousPageSha256: hash("7"),
  retirementEntries: [
    { fileSha256: hash("a"), identitySha256: hash("a"), kind: "revision_commit_receipt" },
    { fileSha256: hash("b"), identitySha256: hash("b"), kind: "revision_frontier_successor" },
    { fileSha256: hash("c"), identitySha256: hash("c"), kind: "revision_projection" },
    { fileSha256: hash("d"), identitySha256: hash("d"), kind: "revision_stage_receipt" },
  ],
  startMemberSha256: hash("8"),
  startSha256: hash("9"),
  watermarkArchiveCommitOrdinal: "20",
  watermarkFrontierSha256: hash("e"),
};
const acknowledgement = {
  ...unsignedAcknowledgement,
  acknowledgementSha256: adjustmentSha256(canonicalJsonBytes(unsignedAcknowledgement)),
};
const common = {
  acknowledgement,
  actionSha256: target.actionSha256,
  artifactSha256: binding.artifactSha256,
  candidateGraphSha256,
  candidateSha256,
  captureEpochWitnessSha256: hash("e"),
  controlSha256: hash("f"),
  controlVersion: "14",
  developmentGraphSha256: hash("1"),
  dueKey: "monthly/wind/2027-02",
  family: "wind",
  fullGraphVerifiedAt: "2027-02-03T12:02:00.000Z",
  inputHeadSha256: hash("2"),
  lifecycleLedgerRootSha256: hash("3"),
  policyReportSha256,
  registrationSha256: binding.registrationSha256,
  sourceCommit,
  sourceSha256: binding.sourceSha256,
};
const prior = buildAdjustmentDevelopmentCustodyAnchor({
  ...common,
  predecessorAnchorSha256: hash("7"),
  sequence: "4",
});
const priorAnchorSha256 = adjustmentSha256(canonicalJsonBytes(prior));
const expected = buildAdjustmentDevelopmentCustodyAnchor({
  ...common,
  predecessorAnchorSha256: priorAnchorSha256,
  sequence: "5",
});
process.stdout.write(JSON.stringify({
  acknowledgementBytes: canonicalJsonBytes(acknowledgement).toString("base64"),
  action: target.action,
  actionBytes: target.bytes.toString("base64"),
  actionSha256: target.actionSha256,
  binding,
  bindingBytes: bindingBytes.toString("base64"),
  bindingSha256,
  expectedAnchorBytes: canonicalJsonBytes(expected).toString("base64"),
  priorCurrentBytes: canonicalJsonBytes({
    anchor: prior,
    anchorSha256: priorAnchorSha256,
  }).toString("base64"),
  sourceCommit,
  sourceRelease,
}));
"#;

static FIXTURE: OnceLock<JsonValue> = OnceLock::new();

/// Remove one bounded process-owned journal fixture.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one private temporary root without touching installed state.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-native-development-anchor-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("protect fixture root");
        Self(root)
    }
}

impl Drop for FixtureRoot {
    /// Remove only the process-owned disposable fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Match the source anchor bytes, predecessor transition, ACK replay, and receipt binding.
#[test]
fn development_anchor_and_acknowledgement_match_the_source() {
    let fixture = source_fixture();
    let acknowledgement_bytes = fixture_bytes(fixture, "acknowledgementBytes");
    let acknowledgement_value = NativeWirePrimitives
        .decode_json(&acknowledgement_bytes)
        .expect("acknowledgement json");
    let request = custody_request_from_acknowledgement(&acknowledgement_value)
        .expect("acknowledgement request");
    let acknowledgement = verify_native_revision_cold_custody_acknowledgement_v2_document(
        &acknowledgement_bytes,
        &request,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .expect("verified acknowledgement");
    let previous = verify_native_maintenance_development_anchor_current_document(&fixture_bytes(
        fixture,
        "priorCurrentBytes",
    ))
    .expect("verified prior")
    .expect("present prior");
    let action = fixture_value(fixture, "action").clone();
    let action_bytes = fixture_bytes(fixture, "actionBytes");
    let binding = fixture_value(fixture, "binding").clone();
    let binding_bytes = fixture_bytes(fixture, "bindingBytes");
    let binding_sha256 = fixture_text(fixture, "bindingSha256");
    let publication = publication(
        action,
        action_bytes,
        fixture_text(fixture, "actionSha256"),
        binding,
        binding_bytes,
        binding_sha256.clone(),
    );
    let graph = verified_graph(&binding_sha256);
    let current = current_release(fixture);
    let witness = witness();
    let due = NativeMaintenanceDue {
        due_key: "monthly/wind/2027-02".to_owned(),
        family: Some(NativeMaintenanceCaptureFamily::Wind),
        mode: NativeMaintenanceDueMode::Monthly,
        original_cutoff_at: "2027-02-01T08:00:00.000Z".to_owned(),
        scope: "monthly/wind".to_owned(),
    };
    let anchor =
        build_native_maintenance_development_anchor(NativeMaintenanceDevelopmentAnchorInput {
            acknowledgement: &acknowledgement,
            current_release: &current,
            development_graph: &graph,
            due: &due,
            epoch_witness: &witness,
            full_graph_verified_at: "2027-02-03T12:02:00.000Z",
            input_head_sha256: &"2".repeat(64),
            lifecycle_head_sha256: &"3".repeat(64),
            previous: Some(&previous),
            publication: &publication,
        })
        .expect("build anchor");
    assert_eq!(
        anchor.canonical_bytes,
        fixture_bytes(fixture, "expectedAnchorBytes")
    );
    assert_eq!(anchor.source_commit, fixture_text(fixture, "sourceCommit"));
    assert_eq!(
        anchor.source_release,
        fixture_text(fixture, "sourceRelease")
    );

    let fixture_root = FixtureRoot::create();
    let (mut journal, _) = NativeMaintenanceJournalStore::initialize_fixture(
        fixture_root.0.clone(),
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .expect("initialize journal");
    let registration_payload = object(vec![
        ("candidateSha256", string(&anchor.candidate_sha256)),
        ("family", string(anchor.family.wire())),
        (
            "registrationSha256",
            string(&anchor.confirmation_registration_sha256),
        ),
    ]);
    let acknowledgement_payload = object(vec![("acknowledgement", acknowledgement.value.clone())]);
    let confirmation_due_key = format!(
        "confirmation/{}/{}",
        anchor.family.wire(),
        anchor.candidate_sha256
    );
    let confirmation_record_key = format!(
        "confirmation/{}/{}/registered",
        anchor.family.wire(),
        anchor.confirmation_registration_sha256
    );
    let acknowledgement_record_key = format!(
        "revision/custody/completed-v2/{}",
        acknowledgement.request.page_sha256
    );
    journal
        .append(
            &[
                NativeMaintenanceJournalAddition {
                    due_key: Some(&confirmation_due_key),
                    kind: "confirmation_registered",
                    payload: &registration_payload,
                    record_key: &confirmation_record_key,
                    recorded_at: "2027-02-03T12:00:00.000Z",
                },
                NativeMaintenanceJournalAddition {
                    due_key: None,
                    kind: CUSTODY_COMPLETED_V2_KIND,
                    payload: &acknowledgement_payload,
                    record_key: &acknowledgement_record_key,
                    recorded_at: "2027-02-03T12:01:00.000Z",
                },
            ],
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
        )
        .expect("seed journal");
    let replayed = read_latest_native_maintenance_revision_custody_acknowledgement(&mut journal)
        .expect("read acknowledgement")
        .expect("present acknowledgement");
    assert_eq!(replayed.canonical_bytes, acknowledgement.canonical_bytes);

    let apply_request = build_native_maintenance_family_release_apply_request(
        NativeMaintenanceFamilyReleaseApplyInput {
            action_sha256: &anchor.action_sha256,
            compensating_release: "2027.02.01-3",
            expected_current_release: &anchor.source_release,
            expected_settings_sha256: &"9".repeat(64),
            expected_source_release: &anchor.source_release,
            family: anchor.family.wire(),
            fencing_token: "7",
            report_sha256: &anchor.policy_report_sha256,
            target_release: "2027.02.01-2",
        },
    )
    .expect("apply request");
    let apply_bytes = NativeWirePrimitives
        .encode_sorted_json(&object(vec![
            ("actionSha256", string(&anchor.action_sha256)),
            ("compensatingRelease", string("2027.02.01-3")),
            ("compensationState", string("absent")),
            (
                "contractVersion",
                string("adjustment-family-release-status/v1"),
            ),
            ("family", string(anchor.family.wire())),
            ("fencingToken", string("7")),
            ("outcome", string("active")),
            ("state", string("acknowledged")),
            ("targetRelease", string("2027.02.01-2")),
        ]))
        .expect("apply bytes");
    let apply_status =
        verify_native_maintenance_family_release_apply_status(&apply_request, &apply_bytes)
            .expect("apply status");
    let install_request = NativeMaintenanceDevelopmentAnchorInstallRequest {
        anchor: anchor.anchor.clone(),
        canonical_bytes: anchor.canonical_bytes.clone(),
        anchor_sha256: anchor.anchor_sha256.clone(),
    };
    let install_bytes = NativeWirePrimitives
        .encode_sorted_json(&object(vec![
            ("anchorSha256", string(&anchor.anchor_sha256)),
            (
                "contractVersion",
                string("adjustment-development-custody-anchor-installation/v1"),
            ),
            ("state", string("installed")),
        ]))
        .expect("install bytes");
    let installation =
        verify_native_maintenance_development_anchor_installation(&install_request, &install_bytes)
            .expect("installation");
    let installed = record_native_maintenance_development_acknowledgement(
        &mut journal,
        NativeMaintenanceDevelopmentAcknowledgementInput {
            anchor: &anchor,
            apply_request: &apply_request,
            apply_status: &apply_status,
            installation: &installation,
            now: "2027-02-03T12:03:00.000Z",
        },
    )
    .expect("record acknowledgement");
    assert_eq!(installed.action_sha256, anchor.action_sha256);
    assert_eq!(
        installed.registration_sha256,
        anchor.confirmation_registration_sha256
    );
    assert_eq!(
        installed.shadow_registration_sha256,
        anchor.shadow_registration_sha256
    );
}

/// Construct the minimum complete publication shape consumed by the anchor owner.
fn publication(
    action: JsonValue,
    action_bytes: Vec<u8>,
    action_sha256: String,
    custody_binding: JsonValue,
    custody_binding_bytes: Vec<u8>,
    custody_binding_sha256: String,
) -> NativeMaintenanceDevelopmentPublication {
    NativeMaintenanceDevelopmentPublication {
        action,
        action_bytes,
        action_path: "config/forecast-adjustments/fixture.json".to_owned(),
        action_sha256,
        confirmation_registration_sha256: "b".repeat(64),
        custody_binding,
        custody_binding_bytes: custody_binding_bytes.clone(),
        custody_binding_sha256: custody_binding_sha256.clone(),
        custody_segment: NativeMaintenanceArchiveSegment {
            members: vec![NativeMaintenanceArchiveSegmentMember {
                kind: CUSTODY_BINDING_VERSION.to_owned(),
                identity_sha256: decode_digest(&custody_binding_sha256),
                payload: custody_binding_bytes,
            }],
            cross_links: Vec::new(),
        },
        family_files: Vec::<NativeMaintenanceDevelopmentPublicationFile>::new(),
        package_files: Vec::<NativeMaintenanceDevelopmentPublicationFile>::new(),
        package_root_sha256: "a".repeat(64),
        package_total_bytes: 0,
        release_date: "2027.02.03".to_owned(),
        release_run_id: "release-fixture".to_owned(),
        shadow_projection: JsonValue::Object(Vec::new()),
        shadow_projection_bytes: Vec::new(),
        shadow_projection_sha256: "a".repeat(64),
    }
}

/// Construct one typed verified graph containing the exact custody member.
fn verified_graph(binding_sha256: &str) -> NativeMaintenanceVerifiedFullGraph {
    NativeMaintenanceVerifiedFullGraph {
        manifest: NativeMaintenanceGraphManifest {
            predecessor_graph_sha256: None,
            entries: vec![NativeMaintenanceGraphEntry {
                kind: CUSTODY_BINDING_VERSION.to_owned(),
                identity_sha256: binding_sha256.to_owned(),
                object_sha256: "a".repeat(64),
                member_offset: 0,
                member_length: 1,
                member_sha256: binding_sha256.to_owned(),
            }],
            cross_links: Vec::new(),
            graph_root_sha256: "b".repeat(64),
        },
        manifest_object_sha256: decode_digest(&"1".repeat(64)),
        object_count: 1,
        member_count: 1,
        predecessor_count: 0,
    }
}

/// Construct the source family release current projection used by the fixture.
fn current_release(fixture: &JsonValue) -> NativeFamilyReleaseCurrent {
    NativeFamilyReleaseCurrent {
        active_installed_receipt_sha256: None,
        catalog_sha256: "8".repeat(64),
        commit: fixture_text(fixture, "sourceCommit"),
        control_installed_receipt_sha256: None,
        contract_version: "adjustment-family-release-current-status/v1".to_owned(),
        family: "wind".to_owned(),
        release: fixture_text(fixture, "sourceRelease"),
        settings_sha256: "9".repeat(64),
        shadow_installed_receipt_sha256: Some("8".repeat(64)),
        source_server_image_digest: "7".repeat(64),
    }
}

/// Construct the exact verified witness fields referenced by the source anchor.
fn witness() -> VerifiedNativeMaintenanceCaptureEpochWitness {
    VerifiedNativeMaintenanceCaptureEpochWitness {
        catalog_frontier_sha256: "1".repeat(64),
        control_plane_sha256: "f".repeat(64),
        control_plane_version: "14".to_owned(),
        database_migration_history_sha256: "2".repeat(64),
        epoch_at: "2027-01-01T00:00:00.000Z".to_owned(),
        serving_snapshot_sha256: "3".repeat(64),
        source_commit: "a".repeat(40),
        source_release: "2027.02.01-1".to_owned(),
        source_server_image_digest: "4".repeat(64),
        witness_sha256: "e".repeat(64),
    }
}

/// Run the source builder once and retain its exact emitted values.
fn source_fixture() -> &'static JsonValue {
    FIXTURE.get_or_init(|| {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let output = Command::new("node")
            .args(["--input-type=module", "--eval", NODE_FIXTURE])
            .current_dir(repository)
            .output()
            .expect("run source fixture");
        // surface only the bounded source fixture diagnostic
        if !output.status.success() {
            panic!(
                "source fixture failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        NativeWirePrimitives
            .decode_json(&output.stdout)
            .expect("decode source fixture")
    })
}

/// Borrow one exact fixture object member.
fn fixture_value<'a>(fixture: &'a JsonValue, key: &str) -> &'a JsonValue {
    field(fixture, key).expect("fixture field")
}

/// Decode one exact fixture string.
fn fixture_text(fixture: &JsonValue, key: &str) -> String {
    text(fixture, key).expect("fixture text")
}

/// Decode one exact fixture base64 payload.
fn fixture_bytes(fixture: &JsonValue, key: &str) -> Vec<u8> {
    STANDARD
        .decode(fixture_text(fixture, key))
        .expect("fixture base64")
}

/// Decode one admitted lowercase digest into its fixed bytes.
fn decode_digest(value: &str) -> [u8; 32] {
    let mut output = [0u8; 32];
    // decode every already fixed lowercase hexadecimal pair
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = nibble(pair[0]) * 16 + nibble(pair[1]);
    }
    output
}

/// Decode one lowercase hexadecimal digit used by a fixed fixture.
fn nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("fixture digest"),
    }
}
