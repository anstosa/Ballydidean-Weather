use std::io;

use weather_contracts::JsonValue;

use super::*;
use crate::MaintenanceCanonicalJson;
use crate::native_maintenance_policy_comparison::{object, string};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Clone, Debug, Eq, PartialEq)]
enum FixtureCall {
    Anchor {
        anchor_sha256: String,
        canonical_bytes: Vec<u8>,
    },
    Family {
        arguments: [String; 9],
    },
}

struct FixtureRemote {
    response: Vec<u8>,
    calls: Vec<FixtureCall>,
}

impl FixtureRemote {
    /// Construct one isolated fixed-response transport fixture.
    fn new(response: Vec<u8>) -> Self {
        Self {
            response,
            calls: Vec::new(),
        }
    }
}

impl NativeMaintenanceDevelopmentRemotePort for FixtureRemote {
    /// Capture the exact anchor operand and body without any live transport.
    fn install_development_custody_anchor(
        &mut self,
        anchor_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.calls.push(FixtureCall::Anchor {
            anchor_sha256: anchor_sha256.to_owned(),
            canonical_bytes: canonical_bytes.to_vec(),
        });
        Ok(self.response.clone())
    }

    /// Capture the exact family operand order without any live transport.
    fn apply_family_release(
        &mut self,
        request: &NativeMaintenanceFamilyReleaseApplyRequest,
    ) -> io::Result<Vec<u8>> {
        self.calls.push(FixtureCall::Family {
            arguments: request.arguments().map(str::to_owned),
        });
        Ok(self.response.clone())
    }
}

/// Preserve the exact source anchor body, content operand, and bound installation response.
#[test]
fn installs_canonical_development_custody_anchor() {
    let anchor = development_anchor();
    let request = build_native_maintenance_development_anchor_install_request(&anchor).unwrap();
    let response = canonical(object(vec![
        ("anchorSha256", string(&request.anchor_sha256)),
        ("contractVersion", string(ANCHOR_INSTALLATION_VERSION)),
        ("state", string("installed")),
    ]));
    let mut remote = FixtureRemote::new(response.clone());

    let installation =
        install_native_maintenance_development_custody_anchor(&mut remote, &request).unwrap();

    assert_eq!(installation.anchor_sha256, request.anchor_sha256);
    assert_eq!(installation.canonical_bytes, response);
    assert_eq!(
        remote.calls,
        vec![FixtureCall::Anchor {
            anchor_sha256: request.anchor_sha256.clone(),
            canonical_bytes: request.canonical_bytes.clone(),
        }]
    );
}

/// Preserve all nine source family operands and admit only the exact terminal response tuple.
#[test]
fn applies_family_release_with_exact_operands() {
    let request = family_request();
    let response = canonical(family_status("acknowledged", "absent", string("active")));
    let mut remote = FixtureRemote::new(response.clone());

    let status = apply_native_maintenance_family_release(&mut remote, &request).unwrap();

    assert_eq!(
        status.state,
        NativeMaintenanceFamilyReleaseApplyState::Acknowledged
    );
    assert_eq!(
        status.outcome,
        Some(NativeMaintenanceFamilyReleaseOutcome::Active)
    );
    assert_eq!(status.canonical_bytes, response);
    assert_eq!(
        remote.calls,
        vec![FixtureCall::Family {
            arguments: [
                "2026.12.01-2".to_owned(),
                "2026.12.01-3".to_owned(),
                "2026.12.01-1".to_owned(),
                "2026.12.01-1".to_owned(),
                HASH_B.to_owned(),
                "wind".to_owned(),
                HASH_A.to_owned(),
                HASH_C.to_owned(),
                "7".to_owned(),
            ],
        }]
    );
}

/// Refuse an in-flight status and an installation acknowledgement for different bytes.
#[test]
fn refuses_unfinished_or_unbound_responses() {
    let request = family_request();
    let unfinished = canonical(family_status("applying", "absent", JsonValue::Null));
    assert_eq!(
        verify_native_maintenance_family_release_apply_status(&request, &unfinished),
        Err(NativeMaintenanceDevelopmentRemoteError::FamilyResponse)
    );

    let anchor =
        build_native_maintenance_development_anchor_install_request(&development_anchor()).unwrap();
    let unbound = canonical(object(vec![
        ("anchorSha256", string(HASH_B)),
        ("contractVersion", string(ANCHOR_INSTALLATION_VERSION)),
        ("state", string("installed")),
    ]));
    assert_eq!(
        verify_native_maintenance_development_anchor_installation(&anchor, &unbound),
        Err(NativeMaintenanceDevelopmentRemoteError::AnchorResponse)
    );
}

/// Build one source-valid first development custody anchor.
fn development_anchor() -> JsonValue {
    object(vec![
        ("actionKind", string("shadow")),
        ("actionSha256", string(HASH_A)),
        ("archiveCommitOrdinal", string("3")),
        ("artifactSha256", string(HASH_A)),
        ("candidateGraphSha256", string(HASH_A)),
        ("candidateSha256", string(HASH_A)),
        ("captureEpochWitnessSha256", string(HASH_A)),
        (
            "contractVersion",
            string("adjustment-development-custody-anchor/v1"),
        ),
        ("controlSha256", string(HASH_A)),
        ("controlVersion", string("1")),
        ("custodyAcknowledgedAt", string("2026-11-25T12:00:00.000Z")),
        ("custodyAcknowledgementSha256", string(HASH_A)),
        ("custodyCheckpointSha256", string(HASH_A)),
        ("developmentGraphSha256", string(HASH_A)),
        ("dueKey", string("monthly/wind/2026-11")),
        ("family", string("wind")),
        ("frontierSha256", string(HASH_A)),
        ("fullGraphVerifiedAt", string("2026-11-25T12:01:00.000Z")),
        ("inputHeadSha256", string(HASH_A)),
        ("lifecycleLedgerRootSha256", string(HASH_A)),
        ("memberRootSha256", string(HASH_A)),
        ("pageSha256", string(HASH_A)),
        ("policyReportSha256", string(HASH_A)),
        ("predecessorAnchorSha256", JsonValue::Null),
        ("registrationSha256", string(HASH_A)),
        ("sequence", string("0")),
        (
            "sourceCommit",
            string("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        ("sourceSha256", string(HASH_A)),
        ("startMemberSha256", JsonValue::Null),
        ("startSha256", string(HASH_A)),
    ])
}

/// Build one source-valid family operation request.
fn family_request() -> NativeMaintenanceFamilyReleaseApplyRequest {
    build_native_maintenance_family_release_apply_request(
        NativeMaintenanceFamilyReleaseApplyInput {
            action_sha256: HASH_A,
            compensating_release: "2026.12.01-3",
            expected_current_release: "2026.12.01-1",
            expected_settings_sha256: HASH_B,
            expected_source_release: "2026.12.01-1",
            family: "wind",
            fencing_token: "7",
            report_sha256: HASH_C,
            target_release: "2026.12.01-2",
        },
    )
    .unwrap()
}

/// Build one exact family status object with a selected terminal or in-flight tuple.
fn family_status(state: &str, compensation_state: &str, outcome: JsonValue) -> JsonValue {
    object(vec![
        ("actionSha256", string(HASH_A)),
        ("compensatingRelease", string("2026.12.01-3")),
        ("compensationState", string(compensation_state)),
        ("contractVersion", string(FAMILY_STATUS_VERSION)),
        ("family", string("wind")),
        ("fencingToken", string("7")),
        ("outcome", outcome),
        ("state", string(state)),
        ("targetRelease", string("2026.12.01-2")),
    ])
}

/// Encode one fixture with the production recursively sorted canonical codec.
fn canonical(value: JsonValue) -> Vec<u8> {
    MaintenanceCanonicalJson::encode_canonical_json(&mut NativeWirePrimitives, &value).unwrap()
}
