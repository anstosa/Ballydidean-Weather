//! Ordinary differential coverage for native daily terminal value-blind planning.

use std::process::Command;

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    NativeMaintenanceCaptureFamily, NativeMaintenanceColdCatalog,
    NativeMaintenanceColdCatalogKindCounts, NativeMaintenanceDailyTerminalPreparation,
    NativeMaintenanceDailyTerminalPreparationInput, NativeMaintenanceDue, NativeMaintenanceDueMode,
    NativeMaintenanceHistoricalArchiveIndex, NativeMaintenanceHistoricalOccurrence,
    NativeMaintenancePostFitRegistration, NativeMaintenancePostFitRegistrationInput,
    NativeWirePrimitives, VerifiedNativeMaintenanceCaptureEpochWitness,
    build_native_maintenance_post_fit_registration,
    prepare_native_maintenance_daily_terminal_material,
};

/// Preserve the unopened pending evaluation input before the active terminal clock.
#[test]
fn native_daily_terminal_preparation_retains_pending_evaluation_input() {
    let fixture = fixture();
    let due = daily_due(&fixture.registration.schedule.terminal_at);
    let terminal = NativeWirePrimitives
        .canonical_utc_milliseconds(&fixture.registration.schedule.terminal_at)
        .expect("terminal clock");
    let clock_at = NativeWirePrimitives
        .format_utc_milliseconds(terminal - 1)
        .expect("preterminal clock");
    let prepared = prepare_native_maintenance_daily_terminal_material(
        NativeMaintenanceDailyTerminalPreparationInput {
            catalog_input_manifest_sha256: &repeated('9'),
            clock_at: &clock_at,
            confirmation_registration: Some(&fixture.confirmation_registration),
            due: &due,
            epoch_witness: &fixture.epoch_witness,
            history: &fixture.history,
            shadow_registration: &fixture.registration.shadow,
            source_commit: &fixture.witness.source_commit,
            verified_epoch_witness: &fixture.witness,
        },
    )
    .expect("prepare pending terminal material");

    let NativeMaintenanceDailyTerminalPreparation::Pending {
        daily_evaluation_input,
    } = prepared
    else {
        panic!("expected pending daily preparation");
    };
    let encoded = NativeWirePrimitives
        .encode_sorted_json(&daily_evaluation_input)
        .expect("encode pending evaluation input");
    let decoded = NativeWirePrimitives
        .decode_json(&encoded)
        .expect("decode pending evaluation input");
    assert_eq!(
        NativeWirePrimitives.encode_sorted_json(&decoded).unwrap(),
        encoded
    );
}

/// Match the actual predecessor's unsupported value-blind plan for absent capture cycles.
#[test]
fn native_daily_terminal_unsupported_plan_matches_source() {
    let fixture = fixture();
    let terminal_at = fixture.registration.schedule.terminal_at.clone();
    let due = daily_due(&terminal_at);
    let prepared = prepare_native_maintenance_daily_terminal_material(
        NativeMaintenanceDailyTerminalPreparationInput {
            catalog_input_manifest_sha256: &repeated('9'),
            clock_at: &terminal_at,
            confirmation_registration: Some(&fixture.confirmation_registration),
            due: &due,
            epoch_witness: &fixture.epoch_witness,
            history: &fixture.history,
            shadow_registration: &fixture.registration.shadow,
            source_commit: &fixture.witness.source_commit,
            verified_epoch_witness: &fixture.witness,
        },
    )
    .expect("prepare unsupported terminal plan");
    let NativeMaintenanceDailyTerminalPreparation::Unsupported { plan } = prepared else {
        panic!("expected unsupported daily preparation");
    };
    let actual = NativeWirePrimitives
        .encode_sorted_json(&plan)
        .expect("encode native unsupported plan");
    let witness = json_argument(&fixture.epoch_witness);
    let confirmation = json_argument(&fixture.confirmation_registration);
    let shadow = json_argument(&fixture.registration.shadow.registration);
    let due = json_argument(&object(vec![
        ("dueKey", string(&due.due_key)),
        ("family", JsonValue::Null),
        ("mode", string("daily")),
        ("originalCutoffAt", string(&due.original_cutoff_at)),
        ("scope", string("daily")),
    ]));
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .args([
            "--input-type=module",
            "--eval",
            NODE_ORACLE,
            &witness,
            &confirmation,
            &shadow,
            &due,
        ])
        .current_dir(repository)
        .output()
        .expect("run source unsupported planner");
    assert!(
        output.status.success(),
        "source unsupported planner failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(actual, output.stdout);
}

struct Fixture {
    confirmation_registration: JsonValue,
    epoch_witness: JsonValue,
    history: NativeMaintenanceHistoricalArchiveIndex,
    registration: NativeMaintenancePostFitRegistration,
    witness: VerifiedNativeMaintenanceCaptureEpochWitness,
}

/// Build one exact future-only witness, rolling registration, and unrelated retained member.
fn fixture() -> Fixture {
    let mut epoch_witness = object(vec![
        ("activationKind", string("inert_v14_pre_activation")),
        ("archiveCommitOrdinal", string("0")),
        (
            "catalogFrontierSha256",
            string("5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af"),
        ),
        (
            "contractVersion",
            string("adjustment-revision-capture-epoch-witness/v1"),
        ),
        ("controlPlaneSha256", string(&repeated('1'))),
        ("controlPlaneVersion", string("14")),
        (
            "databaseMigrationHistorySha256",
            string("6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424"),
        ),
        ("epochAt", string("2025-01-01T00:00:00.000Z")),
        ("servingSnapshotSha256", string(&repeated('2'))),
        ("sourceCommit", string(&"3".repeat(40))),
        ("sourceRelease", string("2025.01.01-1")),
        (
            "sourceServerImageDigest",
            string(&format!("sha256:{}", repeated('4'))),
        ),
        (
            "sourceWebImageDigest",
            string(&format!("sha256:{}", repeated('5'))),
        ),
    ]);
    let witness_bytes = NativeWirePrimitives
        .encode_sorted_json(&epoch_witness)
        .expect("encode unsigned witness");
    let witness_sha256 = format_digest(NativeWirePrimitives.sha256(&witness_bytes));
    let JsonValue::Object(fields) = &mut epoch_witness else {
        unreachable!("fixture witness object");
    };
    fields.push((
        "witnessSha256".encode_utf16().collect(),
        string(&witness_sha256),
    ));
    let witness = VerifiedNativeMaintenanceCaptureEpochWitness {
        catalog_frontier_sha256: "5d932b9623819be9432877a513194158708719497d755155f5ed9a4b501b48af"
            .to_owned(),
        control_plane_sha256: repeated('1'),
        control_plane_version: "14".to_owned(),
        database_migration_history_sha256:
            "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424".to_owned(),
        epoch_at: "2025-01-01T00:00:00.000Z".to_owned(),
        serving_snapshot_sha256: repeated('2'),
        source_commit: "3".repeat(40),
        source_release: "2025.01.01-1".to_owned(),
        source_server_image_digest: format!("sha256:{}", repeated('4')),
        witness_sha256,
    };
    let registration =
        build_native_maintenance_post_fit_registration(NativeMaintenancePostFitRegistrationInput {
            artifact_sha256: &repeated('6'),
            candidate_sha256: &repeated('7'),
            cutoff_at: "2026-09-01T00:00:00.000Z",
            due_month: "2026-09",
            witness: &witness,
            family: NativeMaintenanceCaptureFamily::Temperature,
            historical_member_root_sha256: &repeated('8'),
            input_manifest_sha256: &repeated('9'),
            predecessor_registration_sha256: None,
            predecessor_terminal_at: None,
            requested_at: "2026-10-15T00:00:00.000Z",
            source_identity_sha256: &repeated('a'),
        })
        .expect("build rolling registration fixture");
    let confirmation_registration = confirmation_registration(&registration.confirmation);
    let history = NativeMaintenanceHistoricalArchiveIndex {
        catalog: NativeMaintenanceColdCatalog {
            archive_commit_ordinal: "1".to_owned(),
            catalog_root_sha256: repeated('b'),
            frontier_count: 1,
            frontier_root_sha256: repeated('c'),
            kind_counts: NativeMaintenanceColdCatalogKindCounts {
                actual_best_match: 1,
                native_source: 0,
                rain_gate_input: 0,
                target_revision: 0,
            },
            receipt_root_sha256: repeated('d'),
            serving_snapshot_sha256: repeated('e'),
        },
        contract_version: "adjustment-revision-historical-archive-index/v1",
        history_root_sha256: repeated('f'),
        occurrences: vec![NativeMaintenanceHistoricalOccurrence {
            graph_manifest_sha256: repeated('1'),
            page_sha256: repeated('2'),
            payload_bytes: Vec::new(),
            payload_identity_sha256: repeated('3'),
            payload_kind: "adjustment-revision-projection/v1".to_owned(),
            publication_disposition: "published".to_owned(),
            receipts: Vec::new(),
        }],
        pages: Vec::new(),
        receipt_count: 0,
    };
    Fixture {
        confirmation_registration,
        epoch_witness,
        history,
        registration,
        witness,
    }
}

/// Derive the exact journal-owned preregistration identity and retained lifecycle payload.
fn confirmation_registration(confirmation: &JsonValue) -> JsonValue {
    let JsonValue::Object(mut identity_fields) = confirmation.clone() else {
        unreachable!("fixture confirmation object");
    };
    identity_fields.extend([
        (
            "contractVersion".encode_utf16().collect(),
            string("forecast-adjustment-lifecycle-ledger/v2"),
        ),
        (
            "inputHeadSha256".encode_utf16().collect(),
            string(&repeated('0')),
        ),
    ]);
    let identity = JsonValue::Object(identity_fields.clone());
    let identity_bytes = NativeWirePrimitives
        .encode_sorted_json(&identity)
        .expect("encode confirmation registration identity");
    let registration_sha256 = format_digest(NativeWirePrimitives.sha256(&identity_bytes));
    identity_fields.extend([
        (
            "registrationSha256".encode_utf16().collect(),
            string(&registration_sha256),
        ),
        (
            "candidateReportSha256".encode_utf16().collect(),
            JsonValue::Null,
        ),
        (
            "actionIdentitySha256".encode_utf16().collect(),
            JsonValue::Null,
        ),
        ("accessState".encode_utf16().collect(), string("registered")),
        ("actionState".encode_utf16().collect(), string("none")),
    ]);
    JsonValue::Object(identity_fields)
}

/// Construct the scheduler's exact nullable-family daily due projection.
fn daily_due(terminal_at: &str) -> NativeMaintenanceDue {
    NativeMaintenanceDue {
        due_key: format!("daily/{}", &terminal_at[..10]),
        family: None,
        mode: NativeMaintenanceDueMode::Daily,
        original_cutoff_at: terminal_at.to_owned(),
        scope: "daily".to_owned(),
    }
}

/// Encode one source object as a process argument without the canonical final LF.
fn json_argument(value: &JsonValue) -> String {
    let mut bytes = NativeWirePrimitives
        .encode_sorted_json(value)
        .expect("encode source argument");
    assert_eq!(bytes.pop(), Some(b'\n'));
    String::from_utf8(bytes).expect("utf8 source argument")
}

/// Construct one valid lowercase digest fixture.
fn repeated(value: char) -> String {
    value.to_string().repeat(64)
}

const NODE_ORACLE: &str = r#"
import {
  planAdjustmentMaintenanceUnsupportedDailyConfirmation,
} from "./scripts/research/adjustment_confirmation_values.mjs";
import {
  canonicalJsonBytes,
} from "./scripts/research/adjustment-maintenance-runtime/forecast/candidate.js";

const witness = JSON.parse(process.argv[1]);
const confirmationRegistration = JSON.parse(process.argv[2]);
const shadowRegistration = JSON.parse(process.argv[3]);
const due = JSON.parse(process.argv[4]);
const history = {
  catalog: {
    archiveCommitOrdinal: "1",
    catalogRootSha256: "b".repeat(64),
    frontierRootSha256: "c".repeat(64),
  },
  contractVersion: "adjustment-revision-historical-archive-index/v1",
  historyRootSha256: "f".repeat(64),
  occurrences: [{
    graphManifestSha256: "1".repeat(64),
    pageSha256: "2".repeat(64),
    payloadBytes: new Uint8Array(),
    payloadIdentitySha256: "3".repeat(64),
    payloadKind: "adjustment-revision-projection/v1",
    publicationDisposition: "published",
    receipts: [],
  }],
  pages: [],
  receiptCount: 0,
};
process.stdout.write(canonicalJsonBytes(planAdjustmentMaintenanceUnsupportedDailyConfirmation({
  confirmationRegistration,
  due,
  epochWitness: witness,
  history,
  shadowRegistration,
})));
"#;
