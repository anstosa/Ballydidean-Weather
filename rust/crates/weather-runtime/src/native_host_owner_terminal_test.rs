use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use super::{retained_unsupported_proof, retirement_sql};
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{NativeUnsupportedTerminalProofCurrent, NativeWirePrimitives};

/// Bind unsupported retirement to retained proof without requiring a shadow registration.
#[test]
fn unsupported_retirement_uses_exact_retained_proof_without_shadow_registration() {
    let proof = object(vec![
        (
            "contractVersion",
            string("adjustment-maintenance-unsupported-terminal-proof/v1"),
        ),
        ("family", string("rain")),
        ("proofMember", string(&hash('1'))),
    ]);
    let proof_bytes = NativeWirePrimitives
        .encode_sorted_json(&proof)
        .expect("encode retained proof fixture");
    let retained = NativeUnsupportedTerminalProofCurrent {
        proof: proof.clone(),
        proof_sha256: format_digest(RevisionArchiveCanonicalCodec::sha256(
            &NativeWirePrimitives,
            &proof_bytes,
        )),
    };
    let request = object(vec![
        (
            "contractVersion",
            string("adjustment-shadow-unsupported-terminal-retirement-request/v1"),
        ),
        ("registrationSha256", string(&hash('2'))),
        ("terminalRecord", terminal_record()),
        ("terminalTombstone", tombstone()),
        ("unsupportedProof", proof.clone()),
    ]);
    assert_eq!(
        retained_unsupported_proof(&request, &retained),
        Some(&proof),
    );
    assert!(retirement_sql(&request, &hash('9')).is_some());

    let changed = object(vec![
        (
            "contractVersion",
            string("adjustment-maintenance-unsupported-terminal-proof/v1"),
        ),
        ("family", string("rain")),
        ("proofMember", string(&hash('3'))),
    ]);
    let changed_request = object(vec![
        ("unsupportedProof", changed),
        ("terminalRecord", terminal_record()),
    ]);
    assert!(retained_unsupported_proof(&changed_request, &retained).is_none());
}

/// Build only fields consumed by the already validated retirement SQL projection.
fn terminal_record() -> JsonValue {
    object(vec![
        ("reconciliationSha256", string(&hash('4'))),
        ("registrationSha256", string(&hash('2'))),
    ])
}

/// Build one ordinary retained tombstone fixture.
fn tombstone() -> JsonValue {
    object(vec![
        (
            "contractVersion",
            string("adjustment-shadow-terminal-tombstone/v3"),
        ),
        ("reconciliationSha256", string(&hash('4'))),
        ("registrationSha256", string(&hash('2'))),
        ("terminalResultSha256", string(&hash('5'))),
    ])
}

/// Return one lowercase SHA-256-shaped fixture identity.
fn hash(character: char) -> String {
    std::iter::repeat_n(character, 64).collect()
}
