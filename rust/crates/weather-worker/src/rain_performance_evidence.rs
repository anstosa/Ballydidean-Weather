//! Join admitted rain replay rows to stored runs and edge evidence.

use weather_adjustment::{
    ForecastAdjustmentScorecardCalendar, ForecastAdjustmentScorecardError,
    prepare_forecast_adjustment_performance_pairs, score_balanced_forecast_adjustment_pairs,
};
use weather_contracts::{
    ForecastAdjustmentEvidenceClass, ForecastAdjustmentPerformancePair,
    ForecastAdjustmentRainPerformancePair, ForecastAdjustmentRainProbability, JsonValue, Nullable,
    RainAdjustmentRun, RainWindPerformanceHour, RainWindPerformanceResult,
    RainWindPredictionHourReasonCodeEnum,
};

use crate::RainPerformanceBestMatchDiagnostic;

/// One target returned by the existing fixed-gauge and canonical-identity owners.
#[derive(Debug, Clone, PartialEq)]
pub struct RainPerformanceTarget {
    pub target: Nullable<f64>,
    pub target_key: String,
}

/// One already admitted source forecast retained by an edge receipt.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainEdgeSource {
    pub first_received_at: String,
    pub model_lead_hours: f64,
    pub raw_precipitation_mm: f64,
    pub run_initialized_at: String,
    pub valid_at: String,
}

/// One already admitted edge row needed by the rain performance join.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainEdgeEvidence {
    pub active_bundle: Nullable<String>,
    pub adjustment_bundle_sha256: Nullable<String>,
    pub captured_applied: bool,
    pub corrected_precipitation_mm: Nullable<f64>,
    pub first_edge_committed_at: String,
    pub identity_context: JsonValue,
    pub provenance_complete: bool,
    pub raw_best_match_precipitation_mm: Nullable<f64>,
    pub reason_code: Nullable<String>,
    pub source_receipt_at: String,
    pub source_forecast: Option<AdmittedRainEdgeSource>,
}

/// One verified stored run paired with its deterministic native replay.
#[derive(Debug, Clone, Copy)]
pub struct AdmittedRainPerformanceRun<'a> {
    pub record_revision_identity: &'a str,
    pub replay: &'a RainWindPerformanceResult,
    pub stored: &'a RainAdjustmentRun,
}

/// Existing target, calendar, receipt-clock, and canonical-hash owners.
pub trait RainPerformanceEvidenceValues {
    /// Resolve the original local date for one canonical valid time.
    fn local_date(&mut self, valid_at: &str) -> Option<String>;

    /// Resolve the fixed-gauge target and its canonical identity.
    fn rain_target(&mut self, valid_at: &str) -> Option<RainPerformanceTarget>;

    /// Admit one receipt clock through the existing UTC owner.
    fn receipt_clock(&mut self, value: &str) -> Option<String>;

    /// Hash the exact original edge identity preimage through the canonical owner.
    fn edge_row_identity(
        &mut self,
        edge: &AdmittedRainEdgeEvidence,
        model_sha256: &str,
    ) -> Option<String>;

    /// Hash the original Best Match edge preimage without model identity.
    fn best_match_edge_row_identity(&mut self, edge: &AdmittedRainEdgeEvidence) -> Option<String>;
}

/// All evidence classes produced by the original rain report join.
#[derive(Debug, Clone, PartialEq)]
pub struct RainPerformanceEvidence {
    pub development_amount_pairs: Vec<ForecastAdjustmentPerformancePair>,
    pub development_probability_pairs: Vec<ForecastAdjustmentRainPerformancePair>,
    pub evidence_class: ForecastAdjustmentEvidenceClass,
    pub issued_amount_pairs: Vec<ForecastAdjustmentPerformancePair>,
    pub issued_probability_pairs: Vec<ForecastAdjustmentRainPerformancePair>,
    pub prospective_amount_pairs: Vec<ForecastAdjustmentPerformancePair>,
    pub prospective_probability_pairs: Vec<ForecastAdjustmentRainPerformancePair>,
    pub serving_identity_sha256: Nullable<String>,
}

impl RainPerformanceEvidence {
    /// Borrow the amount rows selected by the original evidence precedence.
    pub fn selected_amount_pairs(&self) -> &[ForecastAdjustmentPerformancePair] {
        // preserve issued then prospective then development precedence
        match self.evidence_class {
            ForecastAdjustmentEvidenceClass::AsIssued => &self.issued_amount_pairs,
            ForecastAdjustmentEvidenceClass::ProspectiveReceipt => &self.prospective_amount_pairs,
            ForecastAdjustmentEvidenceClass::Development => &self.development_amount_pairs,
            ForecastAdjustmentEvidenceClass::RetrospectiveCounterfactual => &[],
        }
    }

    /// Borrow the probability rows selected by the original evidence precedence.
    pub fn selected_probability_pairs(&self) -> &[ForecastAdjustmentRainPerformancePair] {
        // preserve issued then prospective then development precedence
        match self.evidence_class {
            ForecastAdjustmentEvidenceClass::AsIssued => &self.issued_probability_pairs,
            ForecastAdjustmentEvidenceClass::ProspectiveReceipt => {
                &self.prospective_probability_pairs
            }
            ForecastAdjustmentEvidenceClass::Development => &self.development_probability_pairs,
            ForecastAdjustmentEvidenceClass::RetrospectiveCounterfactual => &[],
        }
    }
}

/// Closed refusal classes from the original evidence join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainPerformanceEvidenceError {
    Calendar,
    CapturedServingParity,
    CapturedSourceParity,
    EdgeIdentity,
    Geometry,
    MixedModelIdentity,
    ReceiptClock,
    StoredAmountParity,
    Target,
}

/// Closed failures from the separately paired Best Match diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainPerformanceBestMatchError {
    Calendar,
    EdgeIdentity,
    Geometry,
    ReceiptClock,
    Scorecard(ForecastAdjustmentScorecardError),
    Target,
}

/// Score source raw and Best Match raw values over one identical issued subcohort.
pub fn build_rain_performance_best_match_diagnostic(
    edges: &[AdmittedRainEdgeEvidence],
    values: &mut (impl RainPerformanceEvidenceValues + ForecastAdjustmentScorecardCalendar),
) -> Result<Option<RainPerformanceBestMatchDiagnostic>, RainPerformanceBestMatchError> {
    let mut source_pairs = Vec::new();
    let mut best_match_pairs = Vec::new();
    // retain only complete same-paired issuance rows
    for edge in edges {
        let Some(source) = &edge.source_forecast else {
            continue;
        };
        let (Nullable::Value(corrected), Nullable::Value(best_match)) = (
            &edge.corrected_precipitation_mm,
            &edge.raw_best_match_precipitation_mm,
        ) else {
            continue;
        };
        // preserve finite admitted amounts
        if !corrected.is_finite()
            || !best_match.is_finite()
            || !source.raw_precipitation_mm.is_finite()
        {
            return Err(RainPerformanceBestMatchError::Geometry);
        }
        let horizon_hours = source.model_lead_hours - 8.0;
        // retain the positive operational horizon
        if !horizon_hours.is_finite() || horizon_hours.fract() != 0.0 || horizon_hours < 1.0 {
            continue;
        }
        let target = values
            .rain_target(&source.valid_at)
            .ok_or(RainPerformanceBestMatchError::Target)?;
        let local_date = values
            .local_date(&source.valid_at)
            .ok_or(RainPerformanceBestMatchError::Calendar)?;
        let first_edge_committed_at = values
            .receipt_clock(&edge.first_edge_committed_at)
            .ok_or(RainPerformanceBestMatchError::ReceiptClock)?;
        let source_receipt_at = values
            .receipt_clock(&edge.source_receipt_at)
            .ok_or(RainPerformanceBestMatchError::ReceiptClock)?;
        let row_identity = values
            .best_match_edge_row_identity(edge)
            .ok_or(RainPerformanceBestMatchError::EdgeIdentity)?;
        let base = ForecastAdjustmentPerformancePair {
            adjustedPrediction: *corrected,
            evidenceClass: ForecastAdjustmentEvidenceClass::AsIssued,
            fallback: false,
            firstEdgeCommittedAt: Nullable::Value(first_edge_committed_at),
            horizonHours: horizon_hours,
            key: format!("{}:{}", source.run_initialized_at, source.valid_at),
            localDate: local_date,
            provenanceComplete: edge.provenance_complete,
            rawPrediction: source.raw_precipitation_mm,
            rowIdentity: row_identity,
            sourceReceiptAt: Nullable::Value(source_receipt_at),
            target: target.target,
            targetKey: target.target_key,
            validAt: source.valid_at.clone(),
            vintageKey: source.run_initialized_at.clone(),
        };
        source_pairs.push(base.clone());
        best_match_pairs.push(ForecastAdjustmentPerformancePair {
            rawPrediction: *best_match,
            ..base
        });
    }
    let prepared_source = prepare_forecast_adjustment_performance_pairs(&source_pairs, values)
        .map_err(RainPerformanceBestMatchError::Scorecard)?;
    let prepared_best_match =
        prepare_forecast_adjustment_performance_pairs(&best_match_pairs, values)
            .map_err(RainPerformanceBestMatchError::Scorecard)?;
    // preserve honest absence for empty or unequal populations
    if prepared_source.rows.is_empty()
        || prepared_source.rows.len() != prepared_best_match.rows.len()
    {
        return Ok(None);
    }
    let source = score_balanced_forecast_adjustment_pairs(&prepared_source.rows)
        .map_err(RainPerformanceBestMatchError::Scorecard)?;
    let best_match = score_balanced_forecast_adjustment_pairs(&prepared_best_match.rows)
        .map_err(RainPerformanceBestMatchError::Scorecard)?;
    let date_count = prepared_source
        .rows
        .iter()
        .map(|row| row.localDate.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    Ok(Some(RainPerformanceBestMatchDiagnostic {
        best_match_raw_mae: best_match.metrics.raw.mae,
        date_count: date_count as u64,
        row_count: prepared_source.rows.len() as u64,
        source_adjusted_mae: source.metrics.adjusted.mae,
        source_raw_mae: source.metrics.raw.mae,
    }))
}

/// Join deterministic replays to stored and edge evidence without package I/O.
pub fn build_rain_performance_evidence(
    runs: &[AdmittedRainPerformanceRun<'_>],
    edges: &[AdmittedRainEdgeEvidence],
    values: &mut impl RainPerformanceEvidenceValues,
) -> Result<RainPerformanceEvidence, RainPerformanceEvidenceError> {
    let mut evidence = RainPerformanceEvidence {
        development_amount_pairs: Vec::new(),
        development_probability_pairs: Vec::new(),
        evidence_class: ForecastAdjustmentEvidenceClass::Development,
        issued_amount_pairs: Vec::new(),
        issued_probability_pairs: Vec::new(),
        prospective_amount_pairs: Vec::new(),
        prospective_probability_pairs: Vec::new(),
        serving_identity_sha256: Nullable::Null,
    };

    // preserve stored-run order across the report
    for run in runs {
        require_run_geometry(run)?;
        require_one_model_identity(
            &mut evidence.serving_identity_sha256,
            &run.stored.modelSha256,
        )?;

        // preserve replay-hour order within each run
        for (stored_hour, replay_hour) in run.stored.hours.iter().zip(&run.replay.hours) {
            require_stored_amount_parity(stored_hour, replay_hour)?;
            let target = values
                .rain_target(&replay_hour.validAt)
                .ok_or(RainPerformanceEvidenceError::Target)?;
            let local_date = values
                .local_date(&replay_hour.validAt)
                .ok_or(RainPerformanceEvidenceError::Calendar)?;
            let lead = stored_hour.modelLeadHours;
            let key = format!("{}:{lead}", run.stored.runInitializedAt);
            let development = amount_pair(
                replay_hour.correctedPrecipitationMm,
                ForecastAdjustmentEvidenceClass::Development,
                !replay_hour.applied,
                Nullable::Null,
                lead,
                key,
                local_date.clone(),
                true,
                replay_hour.rawPrecipitationMm,
                format!("{}:{lead}", run.record_revision_identity),
                Nullable::Null,
                &target,
                replay_hour.validAt.clone(),
                run.stored.runInitializedAt.clone(),
            );
            evidence.development_amount_pairs.push(development.clone());
            push_probability_pair(
                &mut evidence.development_probability_pairs,
                &development,
                replay_hour,
                true,
                &run.stored.runInitializedAt,
            );

            // retain original edge order for duplicate evidence rows
            for edge in edges {
                let Some(source) = &edge.source_forecast else {
                    continue;
                };
                // join only the exact run, valid time, and lead
                if source.run_initialized_at != run.stored.runInitializedAt
                    || source.valid_at != replay_hour.validAt
                    || source.model_lead_hours != replay_hour.modelLeadHours
                {
                    continue;
                }
                // bind the captured source bytes before classifying its bundle
                if !same_js_value(source.raw_precipitation_mm, replay_hour.rawPrecipitationMm) {
                    return Err(RainPerformanceEvidenceError::CapturedSourceParity);
                }
                // retain only finite admitted source amounts
                if !source.raw_precipitation_mm.is_finite() {
                    return Err(RainPerformanceEvidenceError::Geometry);
                }
                let exact_serving_bundle = nullable_text(&edge.adjustment_bundle_sha256)
                    == Some(run.stored.modelSha256.as_str())
                    && nullable_text(&edge.active_bundle) == Some(run.stored.modelSha256.as_str());
                // choose the public captured value only while it was active
                let captured_amount = if edge.captured_applied {
                    finite_nullable(&edge.corrected_precipitation_mm)
                        .ok_or(RainPerformanceEvidenceError::Geometry)?
                } else {
                    replay_hour.rawPrecipitationMm
                };
                let captured_parity =
                    same_js_value(captured_amount, replay_hour.correctedPrecipitationMm)
                        && edge.captured_applied == replay_hour.applied
                        && nullable_text(&edge.reason_code) == replay_reason(replay_hour);
                // fail closed only for a claimed active native bundle
                if exact_serving_bundle && !captured_parity {
                    return Err(RainPerformanceEvidenceError::CapturedServingParity);
                }
                // distinguish exact issuance from a replay-only receipt
                let evidence_class = if exact_serving_bundle {
                    ForecastAdjustmentEvidenceClass::AsIssued
                } else {
                    ForecastAdjustmentEvidenceClass::ProspectiveReceipt
                };
                let committed_at = values
                    .receipt_clock(&edge.first_edge_committed_at)
                    .ok_or(RainPerformanceEvidenceError::ReceiptClock)?;
                let source_receipt_at = values
                    .receipt_clock(&source.first_received_at)
                    .ok_or(RainPerformanceEvidenceError::ReceiptClock)?;
                let row_identity = values
                    .edge_row_identity(edge, &run.stored.modelSha256)
                    .ok_or(RainPerformanceEvidenceError::EdgeIdentity)?;
                let fresh = amount_pair(
                    // preserve captured output only for the exact serving bundle
                    if exact_serving_bundle {
                        captured_amount
                    } else {
                        replay_hour.correctedPrecipitationMm
                    },
                    evidence_class,
                    // retain the matching edge fallback state
                    if exact_serving_bundle {
                        !edge.captured_applied
                    } else {
                        !replay_hour.applied
                    },
                    Nullable::Value(committed_at),
                    lead,
                    format!("{}:{lead}", run.stored.runInitializedAt),
                    local_date.clone(),
                    edge.provenance_complete,
                    source.raw_precipitation_mm,
                    row_identity,
                    Nullable::Value(source_receipt_at),
                    &target,
                    replay_hour.validAt.clone(),
                    run.stored.runInitializedAt.clone(),
                );
                // partition edge rows without changing their source order
                let (amounts, probabilities) = if exact_serving_bundle {
                    (
                        &mut evidence.issued_amount_pairs,
                        &mut evidence.issued_probability_pairs,
                    )
                } else {
                    (
                        &mut evidence.prospective_amount_pairs,
                        &mut evidence.prospective_probability_pairs,
                    )
                };
                amounts.push(fresh.clone());
                push_probability_pair(
                    probabilities,
                    &fresh,
                    replay_hour,
                    // require captured parity only for the serving bundle
                    if exact_serving_bundle {
                        captured_parity
                    } else {
                        true
                    },
                    &run.stored.runInitializedAt,
                );
            }
        }
    }
    // select the strongest populated evidence class
    evidence.evidence_class = if !evidence.issued_amount_pairs.is_empty() {
        ForecastAdjustmentEvidenceClass::AsIssued
    } else if !evidence.prospective_amount_pairs.is_empty() {
        ForecastAdjustmentEvidenceClass::ProspectiveReceipt
    } else {
        ForecastAdjustmentEvidenceClass::Development
    };
    Ok(evidence)
}

/// Require stored and replay rows to share one exact geometry.
fn require_run_geometry(
    run: &AdmittedRainPerformanceRun<'_>,
) -> Result<(), RainPerformanceEvidenceError> {
    // stop before partial pairing
    if run.stored.hours.len() != run.replay.hours.len() {
        return Err(RainPerformanceEvidenceError::StoredAmountParity);
    }
    Ok(())
}

/// Retain one deterministic model identity across all stored runs.
fn require_one_model_identity(
    retained: &mut Nullable<String>,
    candidate: &str,
) -> Result<(), RainPerformanceEvidenceError> {
    // reject mixed report cards
    match retained {
        Nullable::Null => *retained = Nullable::Value(candidate.to_owned()),
        Nullable::Value(existing) if existing == candidate => {}
        Nullable::Value(_) => return Err(RainPerformanceEvidenceError::MixedModelIdentity),
    }
    Ok(())
}

/// Require the original Object.is amount and exact categorical parity.
fn require_stored_amount_parity(
    stored: &weather_contracts::RainAdjustmentHour,
    replay: &RainWindPerformanceHour,
) -> Result<(), RainPerformanceEvidenceError> {
    let reason_matches = nullable_text(&stored.reasonCode) == replay_reason(replay);
    // compare every original stored field
    if stored.validAt != replay.validAt
        || stored.modelLeadHours as f64 != replay.modelLeadHours
        || !same_js_value(stored.rawPrecipitationMm, replay.rawPrecipitationMm)
        || !same_js_value(
            stored.correctedPrecipitationMm,
            replay.correctedPrecipitationMm,
        )
        || stored.applied != replay.applied
        || !reason_matches
    {
        return Err(RainPerformanceEvidenceError::StoredAmountParity);
    }
    Ok(())
}

/// Build one generated amount pair without a second DTO definition.
#[allow(clippy::too_many_arguments)]
fn amount_pair(
    adjusted_prediction: f64,
    evidence_class: ForecastAdjustmentEvidenceClass,
    fallback: bool,
    first_edge_committed_at: Nullable<String>,
    lead: u64,
    key: String,
    local_date: String,
    provenance_complete: bool,
    raw_prediction: f64,
    row_identity: String,
    source_receipt_at: Nullable<String>,
    target: &RainPerformanceTarget,
    valid_at: String,
    vintage_key: String,
) -> ForecastAdjustmentPerformancePair {
    ForecastAdjustmentPerformancePair {
        adjustedPrediction: adjusted_prediction,
        evidenceClass: evidence_class,
        fallback,
        firstEdgeCommittedAt: first_edge_committed_at,
        horizonHours: lead as f64 - 8.0,
        key,
        localDate: local_date,
        provenanceComplete: provenance_complete,
        rawPrediction: raw_prediction,
        rowIdentity: row_identity,
        sourceReceiptAt: source_receipt_at,
        target: target.target.clone(),
        targetKey: target.target_key.clone(),
        validAt: valid_at,
        vintageKey: vintage_key,
    }
}

/// Append rain-only probability fields when the native replay retained them.
fn push_probability_pair(
    pairs: &mut Vec<ForecastAdjustmentRainPerformancePair>,
    amount: &ForecastAdjustmentPerformancePair,
    replay: &RainWindPerformanceHour,
    amount_parity: bool,
    run_key: &str,
) {
    let Nullable::Value(probability) = &replay.occurrenceProbabilities else {
        return;
    };
    pairs.push(ForecastAdjustmentRainPerformancePair {
        adjustedPrediction: amount.adjustedPrediction,
        evidenceClass: amount.evidenceClass,
        fallback: amount.fallback,
        firstEdgeCommittedAt: amount.firstEdgeCommittedAt.clone(),
        horizonHours: amount.horizonHours,
        key: amount.key.clone(),
        localDate: amount.localDate.clone(),
        provenanceComplete: amount.provenanceComplete,
        rawPrediction: amount.rawPrediction,
        rowIdentity: amount.rowIdentity.clone(),
        sourceReceiptAt: amount.sourceReceiptAt.clone(),
        target: amount.target.clone(),
        targetKey: amount.targetKey.clone(),
        validAt: amount.validAt.clone(),
        vintageKey: amount.vintageKey.clone(),
        adjustedProbability: ForecastAdjustmentRainProbability {
            atLeast0_1: probability.atLeast0_1,
            atLeast1_0: probability.atLeast1_0,
            atLeast2_5: probability.atLeast2_5,
        },
        amountParity: amount_parity,
        runKey: run_key.to_owned(),
        targetTilingComplete: matches!(&amount.target, Nullable::Value(_)),
    });
}

/// Preserve JavaScript Object.is behavior for runtime numbers.
fn same_js_value(left: f64, right: f64) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

/// Borrow one generated nullable string.
fn nullable_text(value: &Nullable<String>) -> Option<&str> {
    // retain null distinctly
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value.as_str()),
    }
}

/// Borrow the original wire reason for one replay row.
fn replay_reason(hour: &RainWindPerformanceHour) -> Option<&'static str> {
    // preserve the closed reason enum
    match &hour.reasonCode {
        Nullable::Null => None,
        Nullable::Value(RainWindPredictionHourReasonCodeEnum::PhaseUnsupported) => {
            Some("phase_unsupported")
        }
        Nullable::Value(RainWindPredictionHourReasonCodeEnum::PredictionInvalid) => {
            Some("prediction_invalid")
        }
    }
}

/// Borrow one finite nullable number.
fn finite_nullable(value: &Nullable<f64>) -> Option<f64> {
    // reject missing or nonfinite captured amounts
    match value {
        Nullable::Value(value) if value.is_finite() => Some(*value),
        Nullable::Null | Nullable::Value(_) => None,
    }
}

#[cfg(test)]
#[path = "rain_performance_evidence_test.rs"]
mod tests;
