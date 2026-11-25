use weather_contracts::JsonValue;

use super::{
    NativeFamilyReleaseLineageInput, NativeFamilyReleaseLineageTransaction,
    project_native_family_release_lineage, verify_native_family_release_ancestor_projection,
};
use crate::family_target_catalog::{object, string};
use crate::{MaintenanceCanonicalJson, NativeWirePrimitives};

const SOURCE_COMMIT: &str = "3333333333333333333333333333333333333333";
const TARGET_COMMIT: &str = "6666666666666666666666666666666666666666";
const CONTROL_SHA256: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const HASH_7: &str = "7777777777777777777777777777777777777777777777777777777777777777";
const HASH_8: &str = "8888888888888888888888888888888888888888888888888888888888888888";
const SOURCE_RELEASE: &str = "2026.10.08-1";
const TARGET_RELEASE: &str = "2026.10.09-1";

/// Construct one canonical self-addressed v14 capture witness.
fn witness(codec: &mut NativeWirePrimitives) -> Vec<u8> {
    let mut fields = vec![
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
        ("controlPlaneSha256", string(CONTROL_SHA256)),
        ("controlPlaneVersion", string("14")),
        (
            "databaseMigrationHistorySha256",
            string("6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424"),
        ),
        ("epochAt", string("2026-10-08T16:00:00.000Z")),
        (
            "servingSnapshotSha256",
            string("2222222222222222222222222222222222222222222222222222222222222222"),
        ),
        ("sourceCommit", string(SOURCE_COMMIT)),
        ("sourceRelease", string(SOURCE_RELEASE)),
        (
            "sourceServerImageDigest",
            string("sha256:4444444444444444444444444444444444444444444444444444444444444444"),
        ),
        (
            "sourceWebImageDigest",
            string("sha256:5555555555555555555555555555555555555555555555555555555555555555"),
        ),
    ];
    let unsigned = JsonValue::Object(
        fields
            .iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value.clone()))
            .collect(),
    );
    let bytes = codec
        .encode_canonical_json(&unsigned)
        .expect("encode witness");
    let digest = codec.sha256(&bytes).expect("hash witness");
    fields.push(("witnessSha256", string(digest.as_str())));
    codec
        .encode_canonical_json(&JsonValue::Object(
            fields
                .into_iter()
                .map(|(name, value)| (name.encode_utf16().collect(), value))
                .collect(),
        ))
        .expect("encode addressed witness")
}

/// Construct one exact acknowledged action and its canonical digest.
fn acknowledged_transaction(
    codec: &mut NativeWirePrimitives,
) -> (String, NativeFamilyReleaseLineageTransaction) {
    let action = object(vec![
        ("actionKind", string("promote")),
        ("candidateGraphSha256", string(HASH_7)),
        ("candidateSha256", string(HASH_8)),
        (
            "contractVersion",
            string("forecast-adjustment-model-action/v1"),
        ),
        ("createdAt", string("2026-10-08T16:00:00.000Z")),
        ("expectedSettingsSha256", string(HASH_8)),
        ("expectedSourceCommit", string(SOURCE_COMMIT)),
        ("expectedInstalledReceiptSha256", JsonValue::Null),
        ("expectedSourceRelease", string(SOURCE_RELEASE)),
        ("family", string("temperature")),
        ("fencingToken", string("1")),
        ("fullMemberRootSha256", string(HASH_7)),
        ("lifecycleHeadSha256", string(HASH_8)),
        ("policyDecision", string("qualified")),
        ("policyReportSha256", string(HASH_7)),
        ("predecessorActionSha256", JsonValue::Null),
        ("reason", string("qualified_candidate")),
        ("reportCreatedAt", string("2026-10-08T16:00:00.000Z")),
        ("siteKey", string("ballydidean")),
        ("validThrough", string("2026-10-15T16:00:00.000Z")),
    ]);
    let action_bytes = codec.encode_canonical_json(&action).expect("encode action");
    let action_sha256 = codec
        .sha256(&action_bytes)
        .expect("hash action")
        .as_str()
        .to_owned();
    let input = object(vec![
        ("actionSha256", string(&action_sha256)),
        ("compensatingRelease", string("2026.10.10-1")),
        ("expectedCurrentRelease", string(SOURCE_RELEASE)),
        ("expectedSettingsSha256", string(HASH_8)),
        ("expectedSourceRelease", string(SOURCE_RELEASE)),
        ("family", string("temperature")),
        ("fencingToken", string("1")),
        ("reportSha256", string(HASH_7)),
        ("targetRelease", string(TARGET_RELEASE)),
    ]);
    let record = object(vec![
        ("action", action),
        ("createdAt", string("2026-10-08T16:00:00.000Z")),
        ("input", input),
        ("sourceCommit", string(SOURCE_COMMIT)),
        ("sourceSettingsSha256", string(HASH_8)),
        ("state", string("acknowledged")),
        ("targetCommit", string(TARGET_COMMIT)),
        ("verifiedAt", string("2026-10-08T16:01:00.000Z")),
    ]);
    let compensation = object(vec![
        ("actionSha256", string(HASH_7)),
        ("release", string("2026.10.10-1")),
        ("state", string("absent")),
        ("verifiedAt", JsonValue::Null),
    ]);
    let proof = object(vec![
        ("baselineCatalogBase64", string("e30=")),
        ("compensationActionSha256", string(HASH_7)),
        (
            "compensationCommit",
            string("9999999999999999999999999999999999999999"),
        ),
        ("compensationFamilyIdentitySha256", string(HASH_7)),
        ("compensationImageDigestsSha256", string(HASH_8)),
        ("sourceSettingsBase64", string("e30=")),
        ("targetCatalogBase64", string("e30=")),
        ("targetCommit", string(TARGET_COMMIT)),
        ("targetFamilyIdentitySha256", string(HASH_7)),
        ("targetImageDigestsSha256", string(HASH_8)),
    ]);
    let document = object(vec![
        ("compensation", compensation),
        (
            "contractVersion",
            string("adjustment-family-release-transaction/v1"),
        ),
        ("proof", proof),
        ("record", record),
    ]);
    (
        action_sha256.clone(),
        NativeFamilyReleaseLineageTransaction {
            action_sha256,
            document,
        },
    )
}

/// Preserve one acknowledged terminal edge and exact ancestor membership.
#[test]
fn projects_acknowledged_terminal_ancestry() {
    let mut codec = NativeWirePrimitives::new();
    let witness = witness(&mut codec);
    let (action_sha256, transaction) = acknowledged_transaction(&mut codec);
    let current = object(vec![
        ("acknowledgedAt", string("2026-10-08T16:01:00.000Z")),
        ("actionSha256", string(&action_sha256)),
        ("commit", string(TARGET_COMMIT)),
        (
            "contractVersion",
            string("adjustment-family-release-current/v1"),
        ),
        ("family", string("temperature")),
        ("imageDigestsSha256", string(HASH_8)),
        ("release", string(TARGET_RELEASE)),
        ("state", string("acknowledged")),
    ]);
    let transactions = vec![transaction];
    let time = NativeWirePrimitives::new();
    let input = NativeFamilyReleaseLineageInput {
        control_sha256: CONTROL_SHA256,
        control_version: "14",
        current: &current,
        epoch_witness_bytes: &witness,
        transactions: &transactions,
        verified_at: "2026-10-08T16:02:00.000Z",
    };
    let projection =
        project_native_family_release_lineage(&input, &mut codec, &time).expect("project lineage");
    assert_eq!(projection.current_commit, TARGET_COMMIT);
    assert_eq!(projection.epoch_ancestor_commit, SOURCE_COMMIT);
    assert_eq!(
        projection.current_action_sha256.as_deref(),
        Some(action_sha256.as_str())
    );
    assert!(
        verify_native_family_release_ancestor_projection(&input, SOURCE_COMMIT, &mut codec, &time,)
            .expect("verify epoch")
    );
    assert!(
        !verify_native_family_release_ancestor_projection(
            &input,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &mut codec,
            &time,
        )
        .expect("verify unrelated")
    );
}
