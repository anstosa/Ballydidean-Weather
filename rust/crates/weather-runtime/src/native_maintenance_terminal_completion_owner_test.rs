use weather_contracts::JsonValue;

use super::{
    NativeMaintenanceOwnerTerminalClass, NativeMaintenanceOwnerTerminalInput,
    build_native_maintenance_owner_terminal_material, canonical_identity,
};
use crate::native_maintenance_policy_comparison::{object, string};

fn hash(byte: char) -> String {
    byte.to_string().repeat(64)
}

fn terminal_record(disposition: &str) -> JsonValue {
    object(vec![
        ("accessSha256", string(&hash('b'))),
        ("actionCompletedAt", string("2027-09-19T08:00:00.000Z")),
        ("actionDisposition", string(disposition)),
        ("actionSha256", string(&hash('a'))),
        ("candidateSha256", string(&hash('1'))),
        (
            "contractVersion",
            string("adjustment-shadow-terminal-record/v3"),
        ),
        ("family", string("rain")),
        ("finalizedMetadataRootSha256", string(&hash('2'))),
        ("finalizedPredictionCount", JsonValue::Number(1.0)),
        ("maintenanceAnchorSha256", string(&hash('d'))),
        ("metadataGeneration", JsonValue::Number(1.0)),
        ("recordedAt", string("2027-09-19T08:01:00.000Z")),
        ("reconciliationSha256", string(&hash('3'))),
        ("registrationSha256", string(&hash('4'))),
        ("reservedKeySha256", string(&hash('5'))),
        ("sourceSha256", string(&hash('6'))),
        ("terminalMemberSha256", string(&hash('c'))),
        ("terminalResultSha256", string(&hash('7'))),
    ])
}

fn finalization_proof() -> JsonValue {
    object(vec![
        ("actionSha256", string(&hash('a'))),
        ("archiveCommitOrdinal", string("9")),
        ("burnSha256", string(&hash('8'))),
        ("candidateArtifactRootSha256", string(&hash('9'))),
        ("candidateReportSha256", string(&hash('a'))),
        ("captureEpochWitnessSha256", string(&hash('b'))),
        ("confirmationAccessSha256", string(&hash('b'))),
        ("confirmationChunkCount", JsonValue::Number(24.0)),
        (
            "contractVersion",
            string("adjustment-maintenance-finalization-proof/v3"),
        ),
        ("controlSha256", string(&hash('c'))),
        ("controlVersion", string("3")),
        ("custodyCheckpointSha256", string(&hash('d'))),
        (
            "dueKey",
            string(&format!("confirmation/rain/{}", hash('1'))),
        ),
        ("family", string("rain")),
        ("finalizedAt", string("2027-09-19T08:00:00.000Z")),
        ("frontierSha256", string(&hash('e'))),
        ("fullGraphVerifiedAt", string("2027-09-19T07:58:00.000Z")),
        ("fullMemberRootSha256", string(&hash('c'))),
        ("graphManifestSha256", string(&hash('f'))),
        ("inputSealSha256", string(&hash('1'))),
        ("lifecycleLedgerRootSha256", string(&hash('2'))),
        ("pageSha256", string(&hash('3'))),
        ("policyReportSha256", string(&hash('4'))),
        ("predecessorAnchorSha256", JsonValue::Null),
        ("publishedAt", string("2027-09-19T07:59:00.000Z")),
        ("requiredInputRootSha256", string(&hash('5'))),
        ("sequence", string("0")),
        ("sourceCommit", string(&"e".repeat(40))),
        ("transferredAnchorSha256", string(&hash('d'))),
        ("workstationJournalHeadSha256", string(&hash('7'))),
    ])
}

fn unsupported_proof() -> JsonValue {
    let candidate = hash('1');
    let empty_root = canonical_identity(&JsonValue::Array(Vec::new())).expect("empty root");
    let candidate_root =
        canonical_identity(&JsonValue::Array(vec![string(&candidate)])).expect("candidate root");
    let names = [
        "actual_best_match",
        "artifact",
        "candidate",
        "comparator",
        "native_source",
        "rain_gate_input",
        "shadow_body",
        "shadow_source",
        "target",
        "target_revision",
    ];
    let classes = JsonValue::Object(
        names
            .iter()
            .map(|name| {
                let (count, root) = match *name {
                    "actual_best_match" => (0.0, empty_root.clone()),
                    "candidate" => (1.0, candidate_root.clone()),
                    _ => (1.0, hash('9')),
                };
                (
                    name.encode_utf16().collect(),
                    object(vec![
                        ("count", JsonValue::Number(count)),
                        ("rootSha256", string(&root)),
                    ]),
                )
            })
            .collect(),
    );
    let required = canonical_identity(&classes).expect("required root");
    object(vec![
        ("actionSha256", string(&hash('a'))),
        ("archiveCommitOrdinal", string("9")),
        ("burnSha256", string(&hash('8'))),
        ("candidateReportSha256", string(&hash('a'))),
        ("captureEpochWitnessSha256", string(&hash('b'))),
        ("confirmationAccessSha256", string(&hash('b'))),
        ("confirmationChunkCount", JsonValue::Number(24.0)),
        (
            "contractVersion",
            string("adjustment-maintenance-unsupported-terminal-proof/v1"),
        ),
        ("controlSha256", string(&hash('c'))),
        ("controlVersion", string("3")),
        ("custodyCheckpointSha256", string(&hash('d'))),
        ("dueKey", string(&format!("confirmation/rain/{candidate}"))),
        ("eligiblePredictionSetSha256", string(&hash('e'))),
        ("expectedKeySetSha256", string(&hash('f'))),
        ("family", string("rain")),
        ("finalizedAt", string("2027-09-19T08:00:00.000Z")),
        ("frontierSha256", string(&hash('1'))),
        ("fullGraphVerifiedAt", string("2027-09-19T07:58:00.000Z")),
        ("fullMemberRootSha256", string(&hash('c'))),
        ("graphManifestSha256", string(&hash('2'))),
        ("inputClasses", classes),
        ("lifecycleLedgerRootSha256", string(&hash('3'))),
        (
            "missingClassNames",
            JsonValue::Array(vec![string("actual_best_match")]),
        ),
        ("missingKeyCount", JsonValue::Number(1.0)),
        ("missingKeySetSha256", string(&hash('4'))),
        ("pageSha256", string(&hash('5'))),
        ("policyReportSha256", string(&hash('6'))),
        ("predecessorProofSha256", JsonValue::Null),
        ("registrationSha256", string(&hash('4'))),
        ("requiredInputRootSha256", string(&required)),
        ("sequence", string("0")),
        ("sourceCommit", string(&"e".repeat(40))),
        ("unsupportedReason", string("permanent_source_gap")),
        ("workstationJournalHeadSha256", string(&hash('7'))),
    ])
}

#[test]
fn builds_disjoint_source_retirement_requests() {
    let no_action_record = terminal_record("rejected_no_action");
    let no_action =
        build_native_maintenance_owner_terminal_material(NativeMaintenanceOwnerTerminalInput {
            authority_proof: &finalization_proof(),
            class: NativeMaintenanceOwnerTerminalClass::Rejected,
            due_key: "daily/2027-09-19",
            now: "2027-09-19T08:02:00.000Z",
            semantic_input_sha256: &hash('8'),
            terminal_record: &no_action_record,
        })
        .expect("no-action material");
    assert_eq!(
        crate::maintenance_canonical_json::text(&no_action.retirement_request, "contractVersion")
            .as_deref(),
        Some("adjustment-shadow-terminal-retirement-request/v3")
    );

    let unsupported_record = terminal_record("support_failed_no_action");
    let unsupported =
        build_native_maintenance_owner_terminal_material(NativeMaintenanceOwnerTerminalInput {
            authority_proof: &unsupported_proof(),
            class: NativeMaintenanceOwnerTerminalClass::Unsupported,
            due_key: "daily/2027-09-19",
            now: "2027-09-19T08:02:00.000Z",
            semantic_input_sha256: &hash('8'),
            terminal_record: &unsupported_record,
        })
        .expect("unsupported material");
    assert_eq!(
        crate::maintenance_canonical_json::text(&unsupported.retirement_request, "contractVersion")
            .as_deref(),
        Some("adjustment-shadow-unsupported-terminal-retirement-request/v1")
    );
    assert_ne!(
        no_action.retirement_request_sha256,
        unsupported.retirement_request_sha256
    );
}
