use super::*;
use crate::{NativeMaintenanceGraphEntry, NativeMaintenanceGraphManifest};

/// Produce one exact repeated lowercase digest fixture.
fn hash(character: char) -> String {
    character.to_string().repeat(64)
}

/// The native policy and no-action identity match the predecessor's canonical Node fixture.
#[test]
fn unsupported_policy_and_identity_match_node_fixture() {
    let policy = object(vec![
        (
            "contractVersion",
            string("adjustment-maintenance-unsupported-policy-report/v1"),
        ),
        ("family", string("rain")),
        (
            "missingClassNames",
            JsonValue::Array(vec![string("target")]),
        ),
        ("missingKeyCount", JsonValue::Number(2.0)),
        ("missingKeySetSha256", string(&hash('a'))),
        ("registrationSha256", string(&hash('b'))),
        ("state", string("unsupported")),
        ("targetCutoffAt", string("2026-01-01T00:00:00.000Z")),
        ("unsupportedReason", string("permanent_target_gap")),
    ]);
    let policy = canonical_document(policy, NativeMaintenanceTerminalSealOwnerError::Material)
        .expect("fixture policy must encode");
    assert_eq!(
        policy.sha256,
        "9b2fd475bd86166d0b117935021a0b75c4ad5c929d7e0e100ba91c3716ee11fc"
    );
    let identity = object(vec![
        (
            "contractVersion",
            string("adjustment-terminal-no-action-identity/v3"),
        ),
        ("disposition", string("support_failed")),
        ("policyReportSha256", string(&policy.sha256)),
        ("registrationSha256", string(&hash('b'))),
    ]);
    assert_eq!(
        canonical_identity(&identity).expect("fixture identity must encode"),
        "505e23ecfe7ccbb6e18fd8ca0ae2509880d6891eac7ae64db9079569344df32c"
    );
    assert_eq!(
        classify_unsupported_reason(&["shadow_body".to_owned(), "target".to_owned()]),
        "permanent_capture_gap"
    );
    assert_eq!(
        classify_unsupported_reason(&["target_revision".to_owned()]),
        "permanent_target_gap"
    );
    assert_eq!(classify_unsupported_reason(&[]), "permanent_source_gap");
}

/// Graph admission binds the compact partition manifest, not the original graph identity.
#[test]
fn verified_graph_requires_compact_terminal_manifest_member() {
    let unsigned = object(vec![
        ("bindings", JsonValue::Array(Vec::new())),
        (
            "contractVersion",
            string("adjustment-maintenance-confirmation-terminal-graph/v1"),
        ),
        ("derivedTargetMemberSha256s", JsonValue::Array(Vec::new())),
        ("graphManifestSha256s", JsonValue::Array(Vec::new())),
    ]);
    let original_sha = canonical_identity(&unsigned).expect("fixture graph must encode");
    let graph = object(vec![
        ("bindings", JsonValue::Array(Vec::new())),
        (
            "contractVersion",
            string("adjustment-maintenance-confirmation-terminal-graph/v1"),
        ),
        ("derivedTargetMemberSha256s", JsonValue::Array(Vec::new())),
        ("graphManifestSha256s", JsonValue::Array(Vec::new())),
        ("terminalGraphSha256", string(&original_sha)),
    ]);
    let partition = build_native_maintenance_terminal_graph_parts(
        NativeMaintenanceCaptureFamily::Temperature,
        &graph,
    )
    .expect("fixture partition must build");
    let compact_sha = text(&partition.manifest, "terminalGraphManifestSha256")
        .expect("fixture compact identity must exist");
    let entry = NativeMaintenanceGraphEntry {
        kind: "adjustment-maintenance-confirmation-terminal-graph-manifest/v1".to_owned(),
        identity_sha256: compact_sha.clone(),
        object_sha256: hash('c'),
        member_offset: 0,
        member_length: 1,
        member_sha256: hash('d'),
    };
    let verified = NativeMaintenanceVerifiedFullGraph {
        manifest: NativeMaintenanceGraphManifest {
            predecessor_graph_sha256: None,
            entries: vec![entry],
            cross_links: Vec::new(),
            graph_root_sha256: hash('e'),
        },
        manifest_object_sha256: [0; 32],
        object_count: 1,
        member_count: 1,
        predecessor_count: 0,
    };
    assert_eq!(
        verified_terminal_graph_manifest_identity(
            NativeMaintenanceCaptureFamily::Temperature,
            &graph,
            &verified,
        )
        .expect("compact manifest member must be admitted"),
        compact_sha
    );
    let mut wrong = verified;
    wrong.manifest.entries[0].identity_sha256 = original_sha;
    assert_eq!(
        verified_terminal_graph_manifest_identity(
            NativeMaintenanceCaptureFamily::Temperature,
            &graph,
            &wrong,
        ),
        Err(NativeMaintenanceTerminalSealOwnerError::TerminalGraph)
    );
}
