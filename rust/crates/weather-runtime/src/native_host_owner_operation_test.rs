use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use super::{
    NativeHostOwnerOperationKind, access_burn_sql, project_native_host_owner_operation_result,
};
use crate::NativeWirePrimitives;
use crate::native_maintenance_retirement_files_v2::format_digest;

const AT: &str = "2026-11-25T12:00:00.000Z";

/// Project all three genuine result grammars into canonical response bytes.
#[test]
fn owner_result_projection_closes_access_terminal_and_retirement() {
    let native = native_access();
    let native_sha256 = required_text(&native, "accessSha256");
    let access = object(vec![
        ("state", string("burned")),
        ("nativeAccessSha256", string(&native_sha256)),
        ("nativeAccess", native),
        ("localAccessSha256", string(&hash('9'))),
        (
            "contractVersion",
            string("adjustment-confirmation-access-burn-result/v3"),
        ),
    ]);
    let access_input = NativeWirePrimitives
        .encode_ordered_json(&access)
        .expect("encode access fixture");
    let access_result = project_native_host_owner_operation_result(
        NativeHostOwnerOperationKind::Access,
        &access_input,
    )
    .expect("project access result");
    assert_eq!(
        access_result.canonical_bytes,
        NativeWirePrimitives
            .encode_sorted_json(&access)
            .expect("canonical access fixture"),
    );

    let terminal_record = terminal_record();
    let terminal = object(vec![
        ("terminalRecord", terminal_record),
        ("state", string("recorded")),
        ("registrationSha256", string(&hash('b'))),
        ("reconciliationSha256", string(&hash('a'))),
        (
            "contractVersion",
            string("adjustment-shadow-terminal-record-result/v3"),
        ),
    ]);
    let terminal_result = project_native_host_owner_operation_result(
        NativeHostOwnerOperationKind::Terminal,
        &NativeWirePrimitives
            .encode_ordered_json(&terminal)
            .expect("encode terminal fixture"),
    )
    .expect("project terminal result");
    assert_eq!(terminal_result.value, terminal);

    let retirement = object(vec![
        ("state", string("retired")),
        ("registrationSha256", string(&hash('b'))),
        ("reconciliationSha256", string(&hash('a'))),
        (
            "contractVersion",
            string("adjustment-shadow-terminal-retirement-result/v3"),
        ),
    ]);
    assert!(
        project_native_host_owner_operation_result(
            NativeHostOwnerOperationKind::Retire,
            &NativeWirePrimitives
                .encode_ordered_json(&retirement)
                .expect("encode retirement fixture"),
        )
        .is_ok(),
    );
}

/// Refuse a native access result whose server-derived identity was caller-selected.
#[test]
fn owner_result_projection_recomputes_native_access_identity() {
    let mut native = native_access();
    replace(&mut native, "accessSha256", string(&hash('0')));
    let result = object(vec![
        (
            "contractVersion",
            string("adjustment-confirmation-access-burn-result/v3"),
        ),
        ("localAccessSha256", string(&hash('9'))),
        ("nativeAccess", native),
        ("nativeAccessSha256", string(&hash('0'))),
        ("state", string("burned")),
    ]);
    assert!(
        project_native_host_owner_operation_result(
            NativeHostOwnerOperationKind::Access,
            &NativeWirePrimitives
                .encode_sorted_json(&result)
                .expect("encode invalid access fixture"),
        )
        .is_err(),
    );
}

/// Keep the fixed access SQL free of caller-selected identifiers or operation kinds.
#[test]
fn access_sql_projects_only_fixed_transaction_members() {
    let request_sha256 = hash('8');
    let request = object(vec![
        (
            "archive",
            object(vec![("eligiblePredictionSetSha256", string(&hash('e')))]),
        ),
        ("journalHeadSha256", string(&hash('b'))),
        (
            "localBurn",
            object(vec![
                ("accessSha256", string(&hash('9'))),
                ("expectedKeySetSha256", string(&hash('f'))),
                ("gateManifestSha256", string(&hash('d'))),
                ("revisionCatalogWatermarkSha256", string(&hash('3'))),
                ("targetComparatorSnapshotRootSha256", string(&hash('2'))),
                ("targetCutoffAt", string(AT)),
            ]),
        ),
        (
            "metadata",
            object(vec![
                ("finalizedPredictionCount", JsonValue::Number(7.0)),
                ("generation", JsonValue::Number(2.0)),
                ("lastAnchorSha256", string(&hash('6'))),
                ("rootSha256", string(&hash('1'))),
                ("throughAt", string(AT)),
            ]),
        ),
        (
            "shadowRegistration",
            object(vec![
                ("artifactSha256", string(&hash('1'))),
                ("candidateSha256", string(&hash('2'))),
                ("cohortSha256", string(&hash('3'))),
                ("epochWitnessSha256", string(&hash('4'))),
                ("family", string("temperature")),
                ("intervalEndAt", string(AT)),
                ("intervalStartAt", string(AT)),
                ("policySha256", string(&hash('5'))),
                ("predecessorRegistrationSha256", JsonValue::Null),
                ("registrationSha256", string(&hash('a'))),
                ("reservedKeySha256", string(&hash('6'))),
                ("scheduleContractSha256", string(&hash('7'))),
                ("siteKey", string("ballydidean")),
                ("sourceSha256", string(&hash('8'))),
                ("targetCutoffAt", string(AT)),
                ("terminalAt", string(AT)),
            ]),
        ),
    ]);
    let sql = access_burn_sql(&request, &request_sha256).expect("project access SQL");
    assert!(sql.starts_with("BEGIN;\nSET LOCAL statement_timeout = '30s';\n"));
    assert!(sql.contains(&format!(
        "SET LOCAL weather.adjustment_maintenance_anchor_sha256 = '{request_sha256}';"
    )));
    assert!(sql.contains("PERFORM weather_record_adjustment_confirmation_access_v2(argument);"));
    assert!(sql.ends_with("COMMIT;\n"));
    assert!(!sql.contains("unsupported"));
}

/// Build one identity-complete native access row.
fn native_access() -> JsonValue {
    let fields = vec![
        ("accessedAt", string(AT)),
        ("eligiblePredictionSetSha256", string(&hash('e'))),
        ("expectedKeySetSha256", string(&hash('f'))),
        ("gateManifestSha256", string(&hash('d'))),
        ("journalHeadSha256", string(&hash('b'))),
        ("maintenanceAnchorSha256", string(&hash('c'))),
        ("metadataRootSha256", string(&hash('1'))),
        ("registrationSha256", string(&hash('a'))),
        ("revisionCatalogWatermarkSha256", string(&hash('3'))),
        ("targetComparatorSnapshotRootSha256", string(&hash('2'))),
        ("targetCutoffAt", string(AT)),
    ];
    let preimage = [
        "adjustment-confirmation-access/v2".to_owned(),
        hash('a'),
        hash('b'),
        hash('c'),
        hash('d'),
        hash('e'),
        hash('f'),
        hash('1'),
        hash('2'),
        hash('3'),
        AT.to_owned(),
    ]
    .join("\n");
    let access_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        preimage.as_bytes(),
    ));
    let mut complete = vec![("accessSha256", string(&access_sha256))];
    complete.extend(fields);
    object(complete)
}

/// Build one complete owner terminal row for result projection.
fn terminal_record() -> JsonValue {
    object(vec![
        ("accessSha256", string(&hash('1'))),
        ("actionCompletedAt", string(AT)),
        ("actionDisposition", string("promoted_verified")),
        ("actionSha256", string(&hash('2'))),
        ("candidateSha256", string(&hash('3'))),
        (
            "contractVersion",
            string("adjustment-shadow-terminal-record/v3"),
        ),
        ("family", string("temperature")),
        ("finalizedMetadataRootSha256", string(&hash('4'))),
        ("finalizedPredictionCount", JsonValue::Number(7.0)),
        ("maintenanceAnchorSha256", string(&hash('5'))),
        ("metadataGeneration", JsonValue::Number(2.0)),
        ("recordedAt", string(AT)),
        ("reconciliationSha256", string(&hash('a'))),
        ("registrationSha256", string(&hash('b'))),
        ("reservedKeySha256", string(&hash('6'))),
        ("sourceSha256", string(&hash('7'))),
        ("terminalMemberSha256", string(&hash('8'))),
        ("terminalResultSha256", string(&hash('9'))),
    ])
}

/// Replace one known fixture member.
fn replace(value: &mut JsonValue, name: &str, replacement: JsonValue) {
    let JsonValue::Object(fields) = value else {
        panic!("fixture object");
    };
    let (_, member) = fields
        .iter_mut()
        .find(|(key, _)| key.iter().copied().eq(name.encode_utf16()))
        .expect("fixture member");
    *member = replacement;
}

/// Read one fixture string member.
fn required_text(value: &JsonValue, name: &str) -> String {
    let JsonValue::Object(fields) = value else {
        panic!("fixture object");
    };
    let JsonValue::String(units) = &fields
        .iter()
        .find(|(key, _)| key.iter().copied().eq(name.encode_utf16()))
        .expect("fixture member")
        .1
    else {
        panic!("fixture string");
    };
    String::from_utf16(units).expect("fixture UTF-16")
}

/// Construct one generated JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct one generated JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Repeat one lowercase hexadecimal digit into a SHA-256 fixture.
fn hash(character: char) -> String {
    character.to_string().repeat(64)
}
