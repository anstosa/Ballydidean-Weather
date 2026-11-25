use super::*;

/// Prove the genuine unsupported fitter terminal never manufactures archive material.
#[test]
fn unsupported_control_fit_remains_a_typed_no_candidate() {
    let report = object(vec![
        ("calibrationEndAt", string("2026-11-24T00:00:00.000Z")),
        ("calibrationStartAt", string("2026-08-26T00:00:00.000Z")),
        ("contractVersion", string(CONTROL_REPORT_VERSION)),
        ("generatedAt", string("2026-11-25T00:01:00.000Z")),
        (
            "legacyCalibrationStartAt",
            string("2026-10-10T00:00:00.000Z"),
        ),
        ("modelMonth", string("2026-12")),
        ("ordinalArtifact", JsonValue::Null),
        ("parityRows", JsonValue::Array(Vec::new())),
        ("reason", string("insufficient_control_support")),
        ("scales", JsonValue::Null),
        ("state", string("unsupported")),
        ("support", JsonValue::Null),
        ("trainingMaximumValidAt", JsonValue::Null),
    ]);
    let candidate_json = NativeWirePrimitives
        .encode_sorted_json(&report)
        .expect("canonical unsupported report");
    let readiness = "a".repeat(64);
    let fit = NativeMaintenanceFitResult {
        candidate_sha256: digest(&candidate_json),
        candidate_json,
        code_snapshot_sha256: "b".repeat(64),
        contract_version: "adjustment-fit-sandbox/v1".to_owned(),
        family: NativeMaintenanceCaptureFamily::Rain,
        input_snapshot_sha256: "c".repeat(64),
        runtime_readiness_sha256: readiness.clone(),
        stderr: empty_log("stderr"),
        stdout: empty_log("stdout"),
    };
    let completed = complete_fit(
        &fit,
        &readiness,
        "2026-12",
        "2026-11-25T00:00:00.000Z",
        NativeWirePrimitives
            .canonical_utc_milliseconds("2026-12-01T00:00:00.000Z")
            .expect("month start"),
        &"d".repeat(64),
        &"e".repeat(64),
        &"f".repeat(64),
    )
    .expect("unsupported fit receipt");
    assert!(matches!(
        completed,
        CompletedFit::NoCandidate(
            NativeMaintenanceControlReferenceNoCandidateReason::InsufficientControlSupport
        )
    ));
}

/// Prove the due token carries the exact owned bytes without a lossy JSON buffer alias.
#[test]
fn semantic_token_round_trips_both_exact_byte_members() {
    let state = b"state\n";
    let artifact = b"artifact\n";
    let graph = "1".repeat(64);
    let members = "2".repeat(64);
    let receipts = "3".repeat(64);
    let token = semantic_token(state, &graph, artifact, &members, &receipts)
        .expect("closed semantic token");
    assert_eq!(
        text(&token.value, "controlStateBytesBase64")
            .and_then(|value| NativeWirePrimitives.decode_standard_base64(&value)),
        Some(state.to_vec())
    );
    assert_eq!(
        text(&token.value, "ordinalArtifactBytesBase64")
            .and_then(|value| NativeWirePrimitives.decode_standard_base64(&value)),
        Some(artifact.to_vec())
    );
    assert_eq!(
        text(&token.value, "graphManifestSha256").as_deref(),
        Some(graph.as_str())
    );
}

/// Construct one exact empty captured-log receipt.
fn empty_log(label: &'static str) -> crate::NativeMaintenanceFitLogReport {
    crate::NativeMaintenanceFitLogReport {
        bytes: 0,
        label,
        overflow: false,
        sha256: digest(&[]),
    }
}
