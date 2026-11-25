//! Focused source-row identity coverage for replay-backed daily evaluation input.

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum,
};
use weather_edge::ArchiveWirePrimitives;

use super::*;

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// Build one exact typed receipt carrying the genuine prospective chronology used by the row.
fn receipt() -> AdjustmentRevisionCommitReceipt {
    AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: "1".to_owned(),
        archiveCommittedAt: "2026-10-09T13:37:00.000Z".to_owned(),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: HASH_A.to_owned(),
        predecessorFrontierSha256: HASH_A.to_owned(),
        projectionIdentitySha256: HASH_A.to_owned(),
        projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction,
        projectionSha256: HASH_A.to_owned(),
        receiptSha256: HASH_A.to_owned(),
        stageReceiptSha256: HASH_A.to_owned(),
    }
}

/// Construct the actual source-shaped temperature record selected by the archived capsule.
fn record() -> NativeMaintenanceDailyAdmittedRecord {
    let body_row = object(vec![
        ("validAt", string("2026-10-09T14:00:00.000Z")),
        ("leadHours", JsonValue::Number(1.0)),
        ("sourceRowSha256", string(HASH_A)),
        ("candidateTemperatureC64", string("4031333333333333")),
        ("wouldApply", JsonValue::Boolean(false)),
        ("fallbackCode", string("ineligible")),
    ]);
    let source_row = object(vec![
        ("validAt", string("2026-10-09T14:00:00.000Z")),
        ("rawTemperatureC64", string("4030666666666666")),
        ("bestMatchTemperatureC64", string("4031333333333333")),
    ]);
    NativeMaintenanceDailyAdmittedRecord {
        artifact_evaluator_input: JsonValue::Null,
        body: object(vec![
            ("family", string("temperature")),
            ("dueKey", string("capture/2026-10-09T12:35:00.000Z")),
            ("rows", JsonValue::Array(vec![body_row])),
        ]),
        capsule_member_sha256: HASH_A.to_owned(),
        chunk_metadata: JsonValue::Null,
        comparison: object(vec![
            ("farmTarget", JsonValue::Null),
            ("firstEdgeCommittedAt", string("2026-10-09T13:38:00.000Z")),
            ("incumbentPrediction", JsonValue::Number(17.1)),
            (
                "membership",
                object(vec![("targetMemberSha256", string(HASH_B))]),
            ),
            ("nearestThree", JsonValue::Null),
            ("providerFamily", JsonValue::Null),
            ("stationKey", JsonValue::Null),
            ("target", JsonValue::Number(16.9)),
        ]),
        family: NativeMaintenanceCaptureFamily::Temperature,
        key: "capture/2026-10-09T12:35:00.000Z/1/temperature".to_owned(),
        metric: None,
        record_index: 0,
        revision_receipt: receipt(),
        row_index: 0,
        source: object(vec![("rows", JsonValue::Array(vec![source_row]))]),
    }
}

#[test]
fn projects_the_exact_compiled_node_temperature_row_identity() {
    let wire = NativeWirePrimitives;
    let decision = object(vec![
        ("correctedTemperatureC", JsonValue::Null),
        ("state", string("disabled")),
    ]);
    let decision_bytes = wire
        .encode_sorted_json(&decision)
        .expect("encode the canonical model decision");
    let row =
        project_native_maintenance_daily_policy_row(&record(), &decision_bytes, &decision_bytes)
            .expect("project the source-backed row");

    assert_eq!(
        row.row_sha256,
        "50270df308258cad926acaff6d25dd4fcf7c5c2bfe986dd435adb5f4d41c4b44"
    );
    assert_eq!(row.candidate_prediction, 17.2);
    assert_eq!(row.native_source_prediction, 16.4);
    assert_eq!(row.actual_best_match_prediction, 17.2);
}

#[test]
fn refuses_divergent_candidate_and_packaged_replays() {
    let wire = NativeWirePrimitives;
    let candidate = wire
        .encode_sorted_json(&object(vec![
            ("correctedTemperatureC", JsonValue::Null),
            ("state", string("disabled")),
        ]))
        .expect("encode the candidate replay");
    let artifact = wire
        .encode_sorted_json(&object(vec![
            ("correctedTemperatureC", JsonValue::Number(18.0)),
            ("state", string("active")),
        ]))
        .expect("encode the artifact replay");

    assert_eq!(
        project_native_maintenance_daily_policy_row(&record(), &candidate, &artifact),
        None
    );
}
