use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use super::*;

/// Match the source compensate-shadow action and one-file package byte for byte.
#[test]
fn development_shadow_compensation_matches_source_fixture() {
    let fixture = source_fixture();
    let target_action = fixture_value(fixture, "targetAction");
    let target_bytes = fixture_bytes(fixture, "targetActionBytes");
    let target_action_sha256 = fixture_text(fixture, "targetActionSha256");
    let target_commit_sha = fixture_text(fixture, "targetCommitSha");
    let target_release_tag = fixture_text(fixture, "targetReleaseTag");
    let compensation_created_at = fixture_text(fixture, "compensationCreatedAt");

    let material = build_native_maintenance_development_compensation(
        NativeMaintenanceDevelopmentCompensationInput {
            compensation_created_at: &compensation_created_at,
            target: NativeMaintenanceDevelopmentCompensationTarget {
                action: target_action,
                action_bytes: &target_bytes,
                action_sha256: &target_action_sha256,
                commit_sha: &target_commit_sha,
                release_tag: &target_release_tag,
            },
        },
    )
    .unwrap();

    assert_eq!(
        material.action,
        *fixture_value(fixture, "compensationAction")
    );
    assert_eq!(
        material.action_bytes,
        fixture_bytes(fixture, "compensationActionBytes")
    );
    assert_eq!(
        material.action_sha256,
        fixture_text(fixture, "compensationActionSha256")
    );
    assert_eq!(
        material.package_root_sha256,
        fixture_text(fixture, "packageRootSha256")
    );
    assert_eq!(
        material.package_total_bytes,
        fixture_number(fixture, "packageTotalBytes") as usize
    );
    assert!(material.removed_paths.is_empty());
    assert_eq!(material.package_files.len(), 1);
    assert_eq!(
        material.package_files[0].path,
        fixture_text(fixture, "compensationActionPath")
    );
    assert_eq!(
        material.target.target_identity_sha256,
        fixture_text(fixture, "targetIdentitySha256")
    );
    assert_eq!(
        material.target.original_expected_source_commit,
        fixture_text(fixture, "originalExpectedSourceCommit")
    );
    assert_eq!(material.source_commit, target_commit_sha);
}

/// Reject a compensation target whose public action identity was substituted.
#[test]
fn development_shadow_compensation_binds_target_action_bytes() {
    let fixture = source_fixture();
    let target_action = fixture_value(fixture, "targetAction");
    let target_bytes = fixture_bytes(fixture, "targetActionBytes");
    let target_commit_sha = fixture_text(fixture, "targetCommitSha");
    let target_release_tag = fixture_text(fixture, "targetReleaseTag");
    let compensation_created_at = fixture_text(fixture, "compensationCreatedAt");
    let result = build_native_maintenance_development_compensation(
        NativeMaintenanceDevelopmentCompensationInput {
            compensation_created_at: &compensation_created_at,
            target: NativeMaintenanceDevelopmentCompensationTarget {
                action: target_action,
                action_bytes: &target_bytes,
                action_sha256: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                commit_sha: &target_commit_sha,
                release_tag: &target_release_tag,
            },
        },
    );
    assert_eq!(
        result,
        Err(NativeMaintenanceDevelopmentCompensationError::Target)
    );
}

/// Produce one source-generated paired development compensation fixture.
fn source_fixture() -> &'static JsonValue {
    static FIXTURE: OnceLock<JsonValue> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let script = r#"
import {
  buildAdjustmentModelAction,
  packageAdjustmentModelFamily,
} from "./scripts/research/adjustment_model_release.mjs";
const h = (value) => value.repeat(64);
const targetAction = {
  actionKind: "shadow",
  candidateGraphSha256: h("a"),
  candidateSha256: h("b"),
  contractVersion: "forecast-adjustment-model-action/v1",
  createdAt: "2027-01-10T08:00:00.000Z",
  expectedInstalledReceiptSha256: h("f"),
  expectedSettingsSha256: h("c"),
  expectedSourceCommit: "d".repeat(40),
  expectedSourceRelease: "2027.01.01-1",
  family: "wind",
  fencingToken: "7",
  fullMemberRootSha256: null,
  lifecycleHeadSha256: h("d"),
  policyDecision: "pending",
  policyReportSha256: h("e"),
  predecessorActionSha256: null,
  reason: "development_candidate",
  reportCreatedAt: "2027-01-03T08:00:00.000Z",
  siteKey: "ballydidean",
  validThrough: "2027-01-10T08:00:00.000Z",
};
const target = buildAdjustmentModelAction(targetAction);
const targetCommitSha = "e".repeat(40);
const targetReleaseTag = "2027.01.10-2";
const compensationCreatedAt = "2027-01-10T08:01:00.000Z";
const compensationAction = {
  ...targetAction,
  actionKind: "compensate_shadow",
  createdAt: compensationCreatedAt,
  expectedSourceCommit: targetCommitSha,
  expectedSourceRelease: targetReleaseTag,
  predecessorActionSha256: target.actionSha256,
  reason: "development_shadow_failure",
};
const compensation = buildAdjustmentModelAction(compensationAction);
const packaged = packageAdjustmentModelFamily({
  action: compensationAction,
  familyFiles: [],
  parity: null,
  shadowRegistration: null,
});
const encoded = (bytes) => Buffer.from(bytes).toString("base64");
process.stdout.write(JSON.stringify({
  compensationAction,
  compensationActionBytes: encoded(compensation.bytes),
  compensationActionPath: compensation.path,
  compensationActionSha256: compensation.actionSha256,
  compensationCreatedAt,
  originalExpectedSourceCommit: targetAction.expectedSourceCommit,
  packageRootSha256: packaged.packageRootSha256,
  packageTotalBytes: packaged.totalBytes,
  targetAction,
  targetActionBytes: encoded(target.bytes),
  targetActionSha256: target.actionSha256,
  targetCommitSha,
  targetIdentitySha256: targetAction.candidateSha256,
  targetReleaseTag,
}) + "\n");
"#;
        let output = Command::new("node")
            .current_dir(root)
            .args(["--input-type=module", "--eval", script])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        NativeWirePrimitives.decode_json(&output.stdout).unwrap()
    })
}

/// Borrow one exact fixture member.
fn fixture_value<'a>(fixture: &'a JsonValue, key: &str) -> &'a JsonValue {
    field(fixture, key).unwrap()
}

/// Decode one fixture UTF-16 string.
fn fixture_text(fixture: &JsonValue, key: &str) -> String {
    text(fixture, key).unwrap()
}

/// Decode one base64 fixture byte member.
fn fixture_bytes(fixture: &JsonValue, key: &str) -> Vec<u8> {
    STANDARD.decode(fixture_text(fixture, key)).unwrap()
}

/// Decode one nonnegative fixture number.
fn fixture_number(fixture: &JsonValue, key: &str) -> u64 {
    let Some(JsonValue::Number(value)) = field(fixture, key) else {
        panic!("fixture number is unavailable")
    };
    *value as u64
}
