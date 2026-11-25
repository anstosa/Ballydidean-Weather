use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use super::{
    finalized_anchor_matches, qualified_retirement_sql, qualified_terminal_sql, terminal_class,
};
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{NativeMaintenanceOwnerTerminalClass, NativeWirePrimitives};

/// Bind qualified retirement to the exact finalized anchor without a shadow registration.
#[test]
fn qualified_retirement_binds_finalized_anchor_without_shadow_registration() {
    let proof = object(vec![
        ("actionSha256", string(&hash('1'))),
        ("fullMemberRootSha256", string(&hash('2'))),
        ("lifecycleLedgerRootSha256", string(&hash('3'))),
        ("policyReportSha256", string(&hash('4'))),
        ("sourceCommit", string(&"5".repeat(40))),
    ]);
    let proof_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &NativeWirePrimitives
            .encode_sorted_json(&proof)
            .expect("encode finalization proof fixture"),
    ));
    let anchor = object(vec![
        (
            "contractVersion",
            string("adjustment-maintenance-anchor/v3"),
        ),
        ("ctfState", string("finalized")),
        ("finalizationProofSha256", string(&proof_sha256)),
        ("actionSha256", string(&hash('1'))),
        ("fullMemberRootSha256", string(&hash('2'))),
        ("lifecycleLedgerRootSha256", string(&hash('3'))),
        ("policyReportSha256", string(&hash('4'))),
        ("sourceCommit", string(&"5".repeat(40))),
    ]);
    assert!(finalized_anchor_matches(&proof, &anchor));

    let request = object(vec![
        ("registrationSha256", string(&hash('6'))),
        (
            "terminalRecord",
            object(vec![
                ("reconciliationSha256", string(&hash('7'))),
                ("registrationSha256", string(&hash('6'))),
            ]),
        ),
    ]);
    assert!(qualified_retirement_sql(&request, &hash('8')).is_some());

    let changed_anchor = object(vec![
        (
            "contractVersion",
            string("adjustment-maintenance-anchor/v3"),
        ),
        ("ctfState", string("finalized")),
        ("finalizationProofSha256", string(&hash('9'))),
        ("actionSha256", string(&hash('1'))),
        ("fullMemberRootSha256", string(&hash('2'))),
        ("lifecycleLedgerRootSha256", string(&hash('3'))),
        ("policyReportSha256", string(&hash('4'))),
        ("sourceCommit", string(&"5".repeat(40))),
    ]);
    assert!(!finalized_anchor_matches(&proof, &changed_anchor));
}

/// Project all previously missing terminal classes into their exact source row spellings.
#[test]
fn qualified_terminal_projects_resource_and_operator_off_classes() {
    let resource = object(vec![(
        "actionDisposition",
        string("resource_refused_no_action"),
    )]);
    assert_eq!(
        terminal_class(&resource),
        Some(NativeMaintenanceOwnerTerminalClass::ResourceRefused),
    );
    let operator_off = object(vec![(
        "actionDisposition",
        string("operator_off_compensated"),
    )]);
    assert_eq!(
        terminal_class(&operator_off),
        Some(NativeMaintenanceOwnerTerminalClass::OperatorOffCompensated),
    );

    let request = record_request("resource_refused", "verified_no_action");
    let sql = qualified_terminal_sql(&request, &hash('8'), "2026-11-25T12:00:00.000Z")
        .expect("project qualified no-action terminal SQL");
    assert!(sql.contains("resource_refused_no_action"));
    assert!(sql.contains("weather_record_adjustment_shadow_terminal_result_v2"));
}

/// Build only fields consumed by the already validated terminal SQL projection.
fn record_request(disposition: &str, receipt_state: &str) -> JsonValue {
    object(vec![
        (
            "finalizationProof",
            object(vec![
                ("actionSha256", string(&hash('1'))),
                ("fullMemberRootSha256", string(&hash('2'))),
                ("transferredAnchorSha256", string(&hash('3'))),
            ]),
        ),
        (
            "shadowRegistration",
            object(vec![
                ("artifactSha256", string(&hash('4'))),
                ("candidateSha256", string(&hash('5'))),
                ("family", string("rain")),
                ("registrationSha256", string(&hash('6'))),
                ("reservedKeySha256", string(&hash('7'))),
                ("sourceSha256", string(&hash('8'))),
            ]),
        ),
        (
            "nativeAccess",
            object(vec![
                ("accessSha256", string(&hash('9'))),
                ("accessedAt", string("2026-11-25T10:00:00.000Z")),
                ("eligiblePredictionSetSha256", string(&hash('a'))),
                ("expectedKeySetSha256", string(&hash('7'))),
                ("gateManifestSha256", string(&hash('b'))),
                ("journalHeadSha256", string(&hash('c'))),
                ("maintenanceAnchorSha256", string(&hash('d'))),
                ("metadataRootSha256", string(&hash('e'))),
                ("revisionCatalogWatermarkSha256", string(&hash('f'))),
                ("targetComparatorSnapshotRootSha256", string(&hash('0'))),
                ("targetCutoffAt", string("2026-11-25T11:00:00.000Z")),
            ]),
        ),
        (
            "localResult",
            object(vec![("disposition", string(disposition))]),
        ),
        (
            "actionReceipt",
            object(vec![("state", string(receipt_state))]),
        ),
        ("previousTombstone", JsonValue::Null),
    ])
}

/// Return one lowercase SHA-256-shaped fixture identity.
fn hash(character: char) -> String {
    std::iter::repeat_n(character, 64).collect()
}
