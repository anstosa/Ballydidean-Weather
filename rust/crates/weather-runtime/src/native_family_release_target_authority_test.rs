use std::collections::BTreeMap;

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use super::{NativeFamilyReleaseTargetAuthorityError, verify_native_family_release_git_proof};
use crate::{CaptureEpochInitError, CaptureEpochPublicGitJson, NativeWirePrimitives};

const SOURCE_COMMIT: &str = "1111111111111111111111111111111111111111";
const TARGET_COMMIT: &str = "2222222222222222222222222222222222222222";
const SOURCE_RELEASE: &str = "2026.11.01-1";
const TARGET_RELEASE: &str = "2026.11.01-2";
const CANDIDATE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct FixtureGit {
    documents: BTreeMap<String, JsonValue>,
}

impl CaptureEpochPublicGitJson for FixtureGit {
    /// Return only the explicitly captured public metadata fixture.
    fn read_json(&mut self, suffix: &str) -> Result<JsonValue, CaptureEpochInitError> {
        self.documents
            .get(suffix)
            .cloned()
            .ok_or(CaptureEpochInitError::Deployment)
    }
}

/// Prove exact direct-child ancestry, closed paths and Check-before-Publish ordering.
#[test]
fn verifies_target_public_release_authority() {
    let mut codec = NativeWirePrimitives;
    let action = action();
    let action_bytes = codec.encode_sorted_json(&action).unwrap();
    let action_sha = lower_hex(RevisionArchiveCanonicalCodec::sha256(&codec, &action_bytes));
    let mut git = fixture_git(&action_sha, false);

    let proof =
        verify_native_family_release_git_proof(&action, TARGET_RELEASE, &mut git, &mut codec)
            .unwrap();

    assert_eq!(proof.source_commit, SOURCE_COMMIT);
    assert_eq!(proof.target_commit, TARGET_COMMIT);
    assert_eq!(proof.check_run_id, 7);
    assert_eq!(proof.publish_run_id, 9);
}

/// Reject an older successful Check when a newer exact run failed.
#[test]
fn rejects_stale_successful_check() {
    let mut codec = NativeWirePrimitives;
    let action = action();
    let action_bytes = codec.encode_sorted_json(&action).unwrap();
    let action_sha = lower_hex(RevisionArchiveCanonicalCodec::sha256(&codec, &action_bytes));
    let mut git = fixture_git(&action_sha, true);

    assert_eq!(
        verify_native_family_release_git_proof(&action, TARGET_RELEASE, &mut git, &mut codec,),
        Err(NativeFamilyReleaseTargetAuthorityError::Git)
    );
}

/// Build one complete ordinary pending temperature action.
fn action() -> JsonValue {
    json(&format!(
        r#"{{
          "actionKind":"shadow",
          "candidateGraphSha256":"{graph}",
          "candidateSha256":"{CANDIDATE}",
          "contractVersion":"forecast-adjustment-model-action/v1",
          "createdAt":"2026-11-01T00:01:00.000Z",
          "expectedSettingsSha256":"{settings}",
          "expectedSourceCommit":"{SOURCE_COMMIT}",
          "expectedInstalledReceiptSha256":null,
          "expectedSourceRelease":"{SOURCE_RELEASE}",
          "family":"temperature",
          "fencingToken":"1",
          "fullMemberRootSha256":null,
          "lifecycleHeadSha256":"{lifecycle}",
          "policyDecision":"pending",
          "policyReportSha256":"{report}",
          "predecessorActionSha256":null,
          "reason":"development_candidate",
          "reportCreatedAt":"2026-11-01T00:00:00.000Z",
          "siteKey":"ballydidean",
          "validThrough":"2026-11-08T00:00:00.000Z"
        }}"#,
        graph = "c".repeat(64),
        settings = "d".repeat(64),
        lifecycle = "e".repeat(64),
        report = "f".repeat(64),
    ))
}

/// Build the five public documents used by one family Git proof.
fn fixture_git(action_sha: &str, failed_latest_check: bool) -> FixtureGit {
    let mut documents = BTreeMap::new();
    documents.insert(
        format!("git/ref/tags/{SOURCE_RELEASE}"),
        json(&format!(
            r#"{{"object":{{"sha":"{SOURCE_COMMIT}","type":"commit"}}}}"#
        )),
    );
    documents.insert(
        format!("git/ref/tags/{TARGET_RELEASE}"),
        json(&format!(
            r#"{{"object":{{"sha":"{TARGET_COMMIT}","type":"commit"}}}}"#
        )),
    );
    documents.insert(
        format!("compare/{SOURCE_COMMIT}...{TARGET_COMMIT}?per_page=100"),
        json(&format!(
            r#"{{
              "ahead_by":1,
              "base_commit":{{"sha":"{SOURCE_COMMIT}"}},
              "behind_by":0,
              "commits":[{{"parents":[{{"sha":"{SOURCE_COMMIT}"}}],"sha":"{TARGET_COMMIT}"}}],
              "files":[
                {{"filename":"config/forecast-adjustments/ballydidean/actions/sha256-{action_sha}.json","status":"added"}},
                {{"filename":"config/forecast-adjustments/ballydidean/model-parity/temperature/sha256-{CANDIDATE}.json","status":"added"}},
                {{"filename":"config/forecast-adjustments/ballydidean/shadow-catalog/temperature/sha256-{CANDIDATE}.json","status":"added"}},
                {{"filename":"config/forecast-adjustments/ballydidean/temperature-canary-bundles/sha256-{artifact}.json","status":"added"}}
              ],
              "status":"ahead",
              "total_commits":1
            }}"#,
            artifact = "a".repeat(64),
        )),
    );
    documents.insert(
        format!(
            "actions/workflows/check.yml/runs?head_sha={TARGET_COMMIT}&event=push&per_page=100"
        ),
        workflow_document("check.yml", 7, failed_latest_check),
    );
    documents.insert(
        format!(
            "actions/workflows/publish-images.yml/runs?head_sha={TARGET_COMMIT}&event=push&per_page=100"
        ),
        workflow_document("publish-images.yml", 9, false),
    );
    FixtureGit { documents }
}

/// Build one complete workflow page with an optional newer failed run.
fn workflow_document(filename: &str, id: u64, failed_latest: bool) -> JsonValue {
    let created = if filename == "check.yml" {
        "2026-11-01T00:02:00.000Z"
    } else {
        "2026-11-01T00:04:00.000Z"
    };
    let updated = if filename == "check.yml" {
        "2026-11-01T00:03:00.000Z"
    } else {
        "2026-11-01T00:05:00.000Z"
    };
    let success = workflow_run(filename, id, created, updated, "success");
    let runs = if failed_latest {
        format!(
            "{success},{}",
            workflow_run(
                filename,
                id + 1,
                "2026-11-01T00:03:30.000Z",
                "2026-11-01T00:03:40.000Z",
                "failure",
            )
        )
    } else {
        success
    };
    let total = if failed_latest { 2 } else { 1 };
    json(&format!(
        r#"{{"total_count":{total},"workflow_runs":[{runs}]}}"#
    ))
}

/// Build one same-repository exact-commit completed push run.
fn workflow_run(
    filename: &str,
    id: u64,
    created_at: &str,
    updated_at: &str,
    conclusion: &str,
) -> String {
    format!(
        r#"{{
          "conclusion":"{conclusion}",
          "created_at":"{created_at}",
          "event":"push",
          "head_repository":{{"fork":false,"full_name":"anstosa/Ballydidean-Weather","id":1342404160}},
          "head_sha":"{TARGET_COMMIT}",
          "id":{id},
          "path":".github/workflows/{filename}",
          "repository":{{"fork":false,"full_name":"anstosa/Ballydidean-Weather","id":1342404160}},
          "run_attempt":1,
          "status":"completed",
          "updated_at":"{updated_at}"
        }}"#
    )
}

/// Decode one ordinary fixture document through the production JSON primitive.
fn json(value: &str) -> JsonValue {
    NativeWirePrimitives.decode_json(value.as_bytes()).unwrap()
}

/// Render one SHA-256 digest as exact lowercase hexadecimal.
fn lower_hex(value: [u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    // append exactly two lowercase digits for every digest byte
    for byte in value {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}
