use weather_contracts::JsonValue;

use super::*;
use crate::{
    NativeMaintenanceScorecardPublicationInput, build_native_maintenance_scorecard_publication,
    forecast_scorecard_cli::tests::ordinary_family_card,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const HASH_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const HASH_E: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const HASH_F: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

struct FixtureTransport {
    body: Vec<u8>,
    fail: bool,
    operand: String,
}

impl NativeMaintenanceScorecardPublicationTransport for FixtureTransport {
    /// Capture the exact fixed operation operands without invoking a live owner.
    fn install_scorecard_v2(
        &mut self,
        scorecard_sha256: &str,
        canonical_scorecard: &[u8],
    ) -> io::Result<()> {
        self.operand = scorecard_sha256.to_owned();
        self.body = canonical_scorecard.to_vec();
        // model only the source's clean-exit acknowledgement, not diagnostic text
        if self.fail {
            return Err(io::Error::other("fixture transport failed"));
        }
        Ok(())
    }
}

/// Accept one clean forced-command completion and project the exact public root.
#[test]
fn publishes_only_verified_scorecard_acknowledgements() {
    let publication = publication();
    let mut transport = FixtureTransport {
        body: Vec::new(),
        fail: false,
        operand: String::new(),
    };
    let root = publish_native_maintenance_scorecard_v2(
        &publication,
        "2026-10-10T12:00:00.000Z",
        &mut transport,
    )
    .expect("publish acknowledged scorecard");
    assert_eq!(root.scorecard_sha256, publication.scorecard_sha256);
    assert_eq!(transport.operand, publication.scorecard_sha256);
    assert_eq!(transport.body, publication.bytes);
    assert_eq!(
        root.to_json(),
        object(vec![(
            "scorecardSha256",
            string(&publication.scorecard_sha256),
        )])
    );
}

/// Refuse publication when the fixed forced command does not acknowledge clean completion.
#[test]
fn refuses_unverified_scorecard_acknowledgement() {
    let publication = publication();
    let mut transport = FixtureTransport {
        body: Vec::new(),
        fail: true,
        operand: String::new(),
    };
    assert_eq!(
        publish_native_maintenance_scorecard_v2(
            &publication,
            "2026-10-10T12:00:00.000Z",
            &mut transport,
        ),
        Err(NativeMaintenanceScorecardTransportError::Transport)
    );
}

/// Build one genuine complete v2 publication through the separately differential-tested owner.
fn publication() -> NativeMaintenanceScorecardPublication {
    let source_revision = "1".repeat(40);
    let nullable = identities(JsonValue::Null);
    let families = object(vec![
        ("rain", ordinary_family_card("rain")),
        ("temperature", ordinary_family_card("temperature")),
        ("wind", ordinary_family_card("wind")),
    ]);
    let identity_roots = object(vec![
        ("active", nullable.clone()),
        ("prior", nullable.clone()),
        ("raw", identities(string(HASH_B))),
        ("shadow", nullable),
    ]);
    let inputs = object(vec![
        ("adjustmentEvidenceManifestSha256", string(HASH_C)),
        ("adjustmentEvidenceWatermarkSha256", string(HASH_D)),
        ("forecastTrainingManifestSha256", string(HASH_E)),
        ("frontierSha256", string(HASH_F)),
        ("inputManifestSha256", string(HASH_A)),
        ("localDateFrom", string("2026-10-08")),
        ("localDateTo", string("2026-10-09")),
        ("reportSha256", string(HASH_B)),
        ("reportSha256s", identities(string(HASH_B))),
        ("sourceRevision", string(&source_revision)),
        ("targetCutoffAt", string("2026-10-10T07:00:00.000Z")),
    ]);
    let job = object(vec![
        ("attemptState", string("blocked")),
        ("backlogState", string("blocked")),
        ("dueState", string("overdue")),
        ("successState", string("failed")),
    ]);
    let progress = object(vec![
        ("confirmationCompletedEpochs", JsonValue::Number(0.0)),
        ("confirmationRequiredEpochs", JsonValue::Number(9.0)),
        ("rainCaptureExpiresAt", string("2027-10-08T07:00:00.000Z")),
        ("rollbackCompletedEpochs", JsonValue::Number(0.0)),
        ("rollbackRequiredEpochs", JsonValue::Number(0.0)),
    ]);
    let warnings = object(vec![
        ("capacity", JsonValue::Array(Vec::new())),
        (
            "capture",
            JsonValue::Array(vec![string("semantic_catalog_unavailable")]),
        ),
        ("fallback", JsonValue::Array(Vec::new())),
        ("gauge", JsonValue::Array(Vec::new())),
        ("source", JsonValue::Array(Vec::new())),
    ]);
    build_native_maintenance_scorecard_publication(NativeMaintenanceScorecardPublicationInput {
        attempt_sha256: HASH_A,
        families: &families,
        generated_at: "2026-10-10T12:00:00.000Z",
        identities: &identity_roots,
        inputs: &inputs,
        job: &job,
        operator_state: "enabled",
        progress: &progress,
        source_revision: &source_revision,
        warnings: &warnings,
    })
    .expect("build scorecard publication")
}

/// Project one family-keyed identity object.
fn identities(value: JsonValue) -> JsonValue {
    object(vec![
        ("rain", value.clone()),
        ("temperature", value.clone()),
        ("wind", value),
    ])
}

/// Construct one source-shaped JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Construct one generated JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
