use weather_contracts::{
    JsonValue, RainAdjustmentHour, RainWindPerformanceHourOccurrenceProbabilities,
};

use super::*;

const MODEL: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct Values;

impl RainPerformanceEvidenceValues for Values {
    /// Return one fixed local date for admitted fixture clocks.
    fn local_date(&mut self, valid_at: &str) -> Option<String> {
        valid_at
            .starts_with("2026-")
            .then(|| "2026-09-14".to_owned())
    }

    /// Return a target only for the first fixture hour.
    fn rain_target(&mut self, valid_at: &str) -> Option<RainPerformanceTarget> {
        Some(RainPerformanceTarget {
            // distinguish one complete and one absent target
            target: if valid_at.ends_with("09:00:00.000Z") {
                Nullable::Value(1.0)
            } else {
                Nullable::Null
            },
            target_key: format!("target:{valid_at}"),
        })
    }

    /// Admit only canonical-looking fixture clocks.
    fn receipt_clock(&mut self, value: &str) -> Option<String> {
        value.ends_with('Z').then(|| value.to_owned())
    }

    /// Return the fixture identity through the injected owner.
    fn edge_row_identity(
        &mut self,
        edge: &AdmittedRainEdgeEvidence,
        model_sha256: &str,
    ) -> Option<String> {
        (edge.identity_context == JsonValue::Boolean(true) && model_sha256 == MODEL)
            .then(|| "e".repeat(64))
    }

    /// Return the fixture Best Match identity without a model suffix.
    fn best_match_edge_row_identity(&mut self, edge: &AdmittedRainEdgeEvidence) -> Option<String> {
        (edge.identity_context == JsonValue::Boolean(true)).then(|| "f".repeat(64))
    }
}

impl ForecastAdjustmentScorecardCalendar for Values {
    /// parse exact fixture clocks
    fn canonical_utc_epoch_milliseconds(&mut self, value: &str) -> Option<i64> {
        match value {
            "2026-09-14T00:10:00.000Z" => Some(10 * 60_000),
            "2026-09-14T08:30:00.000Z" => Some((8 * 60 + 30) * 60_000),
            "2026-09-14T08:31:00.000Z" => Some((8 * 60 + 31) * 60_000),
            "2026-09-14T09:00:00.000Z" => Some(9 * 60 * 60_000),
            _ => None,
        }
    }

    /// preserve the fixture date
    fn los_angeles_local_date(&mut self, value: &str, _: i64) -> Option<String> {
        (value == "2026-09-14T09:00:00.000Z").then(|| "2026-09-14".to_owned())
    }
}

/// Preserve the original issued-first evidence partition.
#[test]
fn separates_issued_prospective_and_development_pairs() {
    let stored = stored_run(MODEL, 0.5);
    let replay = replay(0.5, 0.4);
    let issued = edge(true, MODEL, 0.5, 0.4);
    let prospective = edge(false, "b", 0.5, 9.0);
    let evidence = build_rain_performance_evidence(
        &[AdmittedRainPerformanceRun {
            record_revision_identity: "run-revision",
            replay: &replay,
            stored: &stored,
        }],
        &[issued, prospective],
        &mut Values,
    )
    .expect("evidence");

    assert_eq!(
        evidence.evidence_class,
        ForecastAdjustmentEvidenceClass::AsIssued
    );
    assert_eq!(evidence.development_amount_pairs.len(), 1);
    assert_eq!(evidence.issued_amount_pairs.len(), 1);
    assert_eq!(evidence.prospective_amount_pairs.len(), 1);
    assert_eq!(
        evidence.selected_amount_pairs(),
        evidence.issued_amount_pairs.as_slice()
    );
    assert_eq!(evidence.issued_probability_pairs.len(), 1);
    assert!(evidence.issued_probability_pairs[0].targetTilingComplete);
    assert_eq!(evidence.prospective_amount_pairs[0].adjustedPrediction, 0.4);
}

/// Preserve the original identical paired Best Match subcohort.
#[test]
fn scores_source_and_best_match_over_one_issued_population() {
    let mut complete = edge(true, MODEL, 0.5, 0.4);
    complete.raw_best_match_precipitation_mm = Nullable::Value(0.75);
    let mut incomplete = edge(true, MODEL, 0.2, 0.1);
    incomplete.raw_best_match_precipitation_mm = Nullable::Null;

    let diagnostic =
        build_rain_performance_best_match_diagnostic(&[complete, incomplete], &mut Values)
            .expect("Best Match population")
            .expect("one complete paired row");

    assert_eq!(diagnostic.row_count, 1);
    assert_eq!(diagnostic.date_count, 1);
    assert_eq!(diagnostic.source_adjusted_mae, 0.6);
    assert_eq!(diagnostic.source_raw_mae, 0.5);
    assert_eq!(diagnostic.best_match_raw_mae, 0.25);
}

/// Preserve Best Match chronology from the edge-level source receipt.
#[test]
fn excludes_best_match_when_edge_source_receipt_follows_commit() {
    let mut edge = edge(true, MODEL, 0.5, 0.4);
    edge.raw_best_match_precipitation_mm = Nullable::Value(0.75);
    edge.source_receipt_at = "2026-09-14T08:31:00.000Z".to_owned();

    let diagnostic = build_rain_performance_best_match_diagnostic(&[edge], &mut Values)
        .expect("chronology should exclude without fabricating");

    assert_eq!(diagnostic, None);
}

/// Refuse negative-zero and active serving-value drift.
#[test]
fn rejects_object_is_and_captured_serving_drift() {
    let stored = stored_run(MODEL, -0.0);
    let zero_replay = replay(0.0, 0.4);
    assert_eq!(
        build_rain_performance_evidence(
            &[AdmittedRainPerformanceRun {
                record_revision_identity: "run-revision",
                replay: &zero_replay,
                stored: &stored,
            }],
            &[],
            &mut Values,
        ),
        Err(RainPerformanceEvidenceError::StoredAmountParity),
    );

    let stored = stored_run(MODEL, 0.5);
    let serving_replay = replay(0.5, 0.4);
    let drifted = edge(true, MODEL, 0.5, 0.7);
    assert_eq!(
        build_rain_performance_evidence(
            &[AdmittedRainPerformanceRun {
                record_revision_identity: "run-revision",
                replay: &serving_replay,
                stored: &stored,
            }],
            &[drifted],
            &mut Values,
        ),
        Err(RainPerformanceEvidenceError::CapturedServingParity),
    );
}

/// Refuse one report that combines different deterministic models.
#[test]
fn rejects_mixed_model_identities() {
    let first = stored_run(MODEL, 0.5);
    let second = stored_run(&"b".repeat(64), 0.5);
    let replay = replay(0.5, 0.4);
    let runs = [
        AdmittedRainPerformanceRun {
            record_revision_identity: "one",
            replay: &replay,
            stored: &first,
        },
        AdmittedRainPerformanceRun {
            record_revision_identity: "two",
            replay: &replay,
            stored: &second,
        },
    ];
    assert_eq!(
        build_rain_performance_evidence(&runs, &[], &mut Values),
        Err(RainPerformanceEvidenceError::MixedModelIdentity),
    );
}

/// Build one stored run matching the replay fixture.
fn stored_run(model: &str, raw: f64) -> RainAdjustmentRun {
    RainAdjustmentRun {
        runInitializedAt: "2026-09-14T00:00:00.000Z".to_owned(),
        modelSha256: model.to_owned(),
        inputSha256: "i".repeat(64),
        forecastClaimId: "claim".to_owned(),
        firstReceivedAt: "2026-09-14T00:05:00.000Z".to_owned(),
        decisionAt: "2026-09-14T08:05:00.000Z".to_owned(),
        generatedAt: "2026-09-14T08:05:01.000Z".to_owned(),
        hours: vec![RainAdjustmentHour {
            validAt: "2026-09-14T09:00:00.000Z".to_owned(),
            modelLeadHours: 9,
            rawPrecipitationMm: raw,
            correctedPrecipitationMm: 0.4,
            applied: true,
            reasonCode: Nullable::Null,
        }],
    }
}

/// Build one deterministic replay fixture.
fn replay(raw: f64, corrected: f64) -> RainWindPerformanceResult {
    RainWindPerformanceResult {
        modelSha256: MODEL.to_owned(),
        modelMonth: "11-2".to_owned(),
        decisionAt: "2026-09-14T08:05:00.000Z".to_owned(),
        hours: vec![RainWindPerformanceHour {
            validAt: "2026-09-14T09:00:00.000Z".to_owned(),
            modelLeadHours: 9.0,
            rawPrecipitationMm: raw,
            correctedPrecipitationMm: corrected,
            applied: true,
            reasonCode: Nullable::Null,
            occurrenceProbabilities: Nullable::Value(
                RainWindPerformanceHourOccurrenceProbabilities {
                    atLeast0_1: 0.8,
                    atLeast1_0: 0.5,
                    atLeast2_5: 0.2,
                },
            ),
            positiveAmountMm: Nullable::Value(0.5),
        }],
    }
}

/// Build one matching or prospective edge fixture.
fn edge(exact: bool, bundle: &str, raw: f64, corrected: f64) -> AdmittedRainEdgeEvidence {
    AdmittedRainEdgeEvidence {
        active_bundle: Nullable::Value(bundle.to_owned()),
        adjustment_bundle_sha256: Nullable::Value(bundle.to_owned()),
        captured_applied: true,
        corrected_precipitation_mm: Nullable::Value(corrected),
        first_edge_committed_at: "2026-09-14T08:30:00.000Z".to_owned(),
        identity_context: JsonValue::Boolean(true),
        provenance_complete: exact,
        raw_best_match_precipitation_mm: Nullable::Null,
        reason_code: Nullable::Null,
        source_receipt_at: "2026-09-14T00:10:00.000Z".to_owned(),
        source_forecast: Some(AdmittedRainEdgeSource {
            first_received_at: "2026-09-14T00:05:00.000Z".to_owned(),
            model_lead_hours: 9.0,
            raw_precipitation_mm: raw,
            run_initialized_at: "2026-09-14T00:00:00.000Z".to_owned(),
            valid_at: "2026-09-14T09:00:00.000Z".to_owned(),
        }),
    }
}
