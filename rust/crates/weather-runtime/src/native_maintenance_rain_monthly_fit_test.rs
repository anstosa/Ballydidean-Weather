use std::path::Path;
use std::process::Command;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum,
};

use super::*;
use crate::{
    NativeMaintenanceColdCatalog, NativeMaintenanceColdCatalogKindCounts,
    NativeMaintenanceHistoricalArchiveIndex, NativeMaintenanceHistoricalFamily,
    NativeMaintenanceHistoricalOccurrence, VerifiedNativeMaintenanceCaptureEpochWitness,
    build_native_adjustment_historical_fit_projection,
};

/// Match the predecessor's authentic rain-history join and annual V3 proof identities.
#[test]
fn rain_monthly_v3_matches_predecessor_incomplete_history() {
    let fixture = predecessor_fixture();
    let witness = witness(&fixture["witness"]);
    let historical = build_native_adjustment_historical_fit_projection(
        &witness,
        NativeMaintenanceHistoricalFamily::Rain,
        "2026-12",
        &history(&fixture),
    )
    .unwrap();
    let outcome =
        build_native_adjustment_rain_monthly_fit_assembly("2026-12", &witness, &historical)
            .unwrap();
    let NativeMaintenanceRainMonthlyFitOutcome::NoCandidate { common, reason } = outcome else {
        panic!("fixture has an intentionally incomplete annual population");
    };
    let expected = &fixture["expected"];
    assert_eq!(
        reason,
        NativeMaintenanceRainMonthlyNoCandidateReason::IncompleteDevelopmentPopulation
    );
    assert_eq!(common.cutoff_at, string(expected, "cutoffAt"));
    assert_eq!(
        common.development_start_at,
        string(expected, "developmentStartAt")
    );
    assert_eq!(
        common.development_end_at,
        string(expected, "developmentEndAt")
    );
    assert_eq!(
        common.source_member_root_sha256,
        string(expected, "sourceMemberRootSha256")
    );
    assert_eq!(
        common.source_receipt_root_sha256,
        string(expected, "sourceReceiptRootSha256")
    );
    assert_eq!(
        common.development_population.expected_row_count,
        unsigned(expected, "expectedRowCount")
    );
    assert_eq!(
        common.development_population.observed_row_count,
        unsigned(expected, "observedRowCount")
    );
    assert_eq!(
        common.development_population.missing_source_row_count,
        unsigned(expected, "missingSourceRowCount")
    );
    assert_eq!(
        common.development_population.missing_target_row_count,
        unsigned(expected, "missingTargetRowCount")
    );
    assert_eq!(
        common.development_population.population_sha256,
        string(expected, "populationSha256")
    );
    assert_eq!(
        common.development_population.population_member_root_sha256,
        string(expected, "populationMemberRootSha256")
    );
    assert_eq!(
        common.development_population.population_receipt_root_sha256,
        string(expected, "populationReceiptRootSha256")
    );
}

/// Ask the installed predecessor for genuine archived bytes and exact V3 results.
fn predecessor_fixture() -> Value {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(3).expect("workspace root");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_ORACLE])
        .current_dir(root)
        .output()
        .expect("run rain monthly predecessor fixture");
    assert!(
        output.status.success(),
        "rain monthly predecessor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("decode rain monthly fixture")
}

/// Build the verified historical archive index from predecessor occurrence bytes.
fn history(fixture: &Value) -> NativeMaintenanceHistoricalArchiveIndex {
    let occurrences = fixture["occurrences"]
        .as_array()
        .expect("fixture occurrences")
        .iter()
        .map(|occurrence| NativeMaintenanceHistoricalOccurrence {
            graph_manifest_sha256: string(occurrence, "graphManifestSha256").to_owned(),
            page_sha256: string(occurrence, "pageSha256").to_owned(),
            payload_bytes: STANDARD
                .decode(string(occurrence, "payloadBase64"))
                .expect("decode rain occurrence"),
            payload_identity_sha256: string(occurrence, "payloadIdentitySha256").to_owned(),
            payload_kind: string(occurrence, "payloadKind").to_owned(),
            publication_disposition: "published".to_owned(),
            receipts: occurrence["receipts"]
                .as_array()
                .expect("fixture receipts")
                .iter()
                .map(receipt)
                .collect(),
        })
        .collect::<Vec<_>>();
    let receipt_count = occurrences
        .iter()
        .map(|occurrence| occurrence.receipts.len())
        .sum();
    NativeMaintenanceHistoricalArchiveIndex {
        catalog: NativeMaintenanceColdCatalog {
            archive_commit_ordinal: "13".to_owned(),
            catalog_root_sha256: "1".repeat(64),
            frontier_count: 13,
            frontier_root_sha256: "2".repeat(64),
            kind_counts: NativeMaintenanceColdCatalogKindCounts {
                actual_best_match: 0,
                native_source: 0,
                rain_gate_input: 1,
                target_revision: 12,
            },
            receipt_root_sha256: "3".repeat(64),
            serving_snapshot_sha256: "4".repeat(64),
        },
        contract_version: "adjustment-revision-historical-archive-index/v1",
        history_root_sha256: string(fixture, "historyRootSha256").to_owned(),
        occurrences,
        pages: Vec::new(),
        receipt_count,
    }
}

/// Build one exact generated commit receipt from the source fixture.
fn receipt(value: &Value) -> AdjustmentRevisionCommitReceipt {
    AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: string(value, "archiveCommitOrdinal").to_owned(),
        archiveCommittedAt: string(value, "archiveCommittedAt").to_owned(),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: string(value, "frontierSha256").to_owned(),
        predecessorFrontierSha256: string(value, "predecessorFrontierSha256").to_owned(),
        projectionIdentitySha256: string(value, "projectionIdentitySha256").to_owned(),
        projectionKind: match string(value, "projectionKind") {
            "rain_gate_input" => AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput,
            "target_revision" => AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision,
            _ => panic!("rain fixture projection kind"),
        },
        projectionSha256: string(value, "projectionSha256").to_owned(),
        receiptSha256: string(value, "receiptSha256").to_owned(),
        stageReceiptSha256: string(value, "stageReceiptSha256").to_owned(),
    }
}

/// Retain the already-verified epoch fields consumed by both projectors.
fn witness(value: &Value) -> VerifiedNativeMaintenanceCaptureEpochWitness {
    VerifiedNativeMaintenanceCaptureEpochWitness {
        catalog_frontier_sha256: string(value, "catalogFrontierSha256").to_owned(),
        control_plane_sha256: string(value, "controlPlaneSha256").to_owned(),
        control_plane_version: string(value, "controlPlaneVersion").to_owned(),
        database_migration_history_sha256: string(value, "databaseMigrationHistorySha256")
            .to_owned(),
        epoch_at: string(value, "epochAt").to_owned(),
        serving_snapshot_sha256: string(value, "servingSnapshotSha256").to_owned(),
        source_commit: string(value, "sourceCommit").to_owned(),
        source_release: string(value, "sourceRelease").to_owned(),
        source_server_image_digest: string(value, "sourceServerImageDigest").to_owned(),
        witness_sha256: string(value, "witnessSha256").to_owned(),
    }
}

/// Borrow one required fixture string.
fn string<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name].as_str().expect("fixture string")
}

/// Read one safe fixture count as usize.
fn unsigned(value: &Value, name: &str) -> usize {
    usize::try_from(value[name].as_u64().expect("fixture unsigned")).expect("fixture usize")
}

const NODE_ORACLE: &str = r#"
import { buildAdjustmentHistoricalFitProjection } from
  "./scripts/research/adjustment_historical_fit_assembler.mjs";
import { buildAdjustmentRainMonthlyFitAssembly } from
  "./scripts/research/adjustment_rain_monthly_projection.mjs";
import { rainHistory, rainHistoryEpochWitness } from
  "./scripts/research/fixtures/adjustment-rain-history.mjs";

const witness=rainHistoryEpochWitness();
const history=rainHistory();
const historicalProjection=buildAdjustmentHistoricalFitProjection({epochWitness:witness,
  family:"rain",fitMonth:"2026-12",history});
const result=buildAdjustmentRainMonthlyFitAssembly({dueMonth:"2026-12",epochWitness:witness,
  historicalProjection});
const population=result.developmentPopulation;
console.log(JSON.stringify({witness,historyRootSha256:history.historyRootSha256,
  occurrences:history.occurrences.map(({payloadBytes,...occurrence})=>({...occurrence,
    payloadBase64:payloadBytes.toString("base64")})),expected:{state:result.state,
    reason:result.reason,cutoffAt:result.cutoffAt,developmentStartAt:result.developmentStartAt,
    developmentEndAt:result.developmentEndAt,sourceMemberRootSha256:result.sourceMemberRootSha256,
    sourceReceiptRootSha256:result.sourceReceiptRootSha256,
    expectedRowCount:population.expectedRowCount,observedRowCount:population.observedRowCount,
    missingSourceRowCount:population.missingSourceRowCount,
    missingTargetRowCount:population.missingTargetRowCount,
    populationSha256:population.populationSha256,
    populationMemberRootSha256:population.populationMemberRootSha256,
    populationReceiptRootSha256:population.populationReceiptRootSha256}}));
"#;
