//! prepare and score the original paired forecast-adjustment amount population.

use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};

use weather_contracts::{
    ForecastAdjustmentEvidenceClass, ForecastAdjustmentMetricSummary,
    ForecastAdjustmentPairedMetricSummary, ForecastAdjustmentPerformancePair,
    ForecastAdjustmentPreparedPairs, ForecastAdjustmentPreparedPairsDiagnostics,
    ForecastAdjustmentPreparedPairsExclusions, ForecastAdjustmentWeightedPair, Nullable,
};

/// calendar and canonical-time owner retained outside this arithmetic module.
pub trait ForecastAdjustmentScorecardCalendar {
    /// parse only an exact canonical UTC millisecond instant.
    fn canonical_utc_epoch_milliseconds(&mut self, value: &str) -> Option<i64>;

    /// derive the Los Angeles date for one already parsed UTC instant.
    fn los_angeles_local_date(&mut self, value: &str, epoch_milliseconds: i64) -> Option<String>;
}

/// closed failures from pair preparation or amount scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastAdjustmentScorecardError {
    Calendar,
    DuplicateKey,
    EmptyPopulation,
    EvidenceClassesDiffer,
    FallbackDiffers,
    InvalidPair,
    LostTarget,
    OverlapIdentityCollision,
}

/// coalesced rows and the count of redundant overlap surfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct CoalescedForecastAdjustmentPairs {
    pub duplicate_count: u64,
    pub rows: Vec<ForecastAdjustmentPerformancePair>,
}

/// paired amount metrics together with their exact weighted population.
#[derive(Debug, Clone, PartialEq)]
pub struct BalancedForecastAdjustmentScore {
    pub metrics: ForecastAdjustmentPairedMetricSummary,
    pub weighted_rows: Vec<ForecastAdjustmentWeightedPair>,
}

type PairsByHour<'a> = BTreeMap<String, Vec<&'a ForecastAdjustmentPerformancePair>>;

/// coalesce overlapping response windows at the earliest committed edge response.
pub fn coalesce_forecast_adjustment_performance_pairs(
    pairs: &[ForecastAdjustmentPerformancePair],
    calendar: &mut impl ForecastAdjustmentScorecardCalendar,
) -> Result<CoalescedForecastAdjustmentPairs, ForecastAdjustmentScorecardError> {
    let mut identity_indexes = HashMap::<String, usize>::new();
    let mut rows = Vec::<ForecastAdjustmentPerformancePair>::new();
    let mut duplicate_count = 0_u64;

    // merge exact stable identities
    for pair in pairs {
        // retain the closed canonical identifier collation
        if !sort_fields_are_ascii(pair) {
            return Err(ForecastAdjustmentScorecardError::InvalidPair);
        }
        let identity = format!("{}\0{}", pair.rowIdentity, pair.targetKey);
        let index = match identity_indexes.entry(identity) {
            Entry::Vacant(entry) => {
                entry.insert(rows.len());
                rows.push(pair.clone());
                continue;
            }
            Entry::Occupied(entry) => *entry.get(),
        };
        let existing = &rows[index];

        // reject scoring collisions
        if !overlap_content_equal(existing, pair) {
            return Err(ForecastAdjustmentScorecardError::OverlapIdentityCollision);
        }

        duplicate_count = duplicate_count
            .checked_add(1)
            .ok_or(ForecastAdjustmentScorecardError::InvalidPair)?;
        let replace = match (&existing.firstEdgeCommittedAt, &pair.firstEdgeCommittedAt) {
            (Nullable::Null, Nullable::Value(_)) => true,
            (Nullable::Value(existing), Nullable::Value(candidate)) => {
                canonical_instant(calendar, candidate)? < canonical_instant(calendar, existing)?
            }
            _ => false,
        };

        // retain earliest evidence
        if replace {
            rows[index] = pair.clone();
        }
    }

    rows.sort_by(pair_order);
    Ok(CoalescedForecastAdjustmentPairs {
        duplicate_count,
        rows,
    })
}

/// validate and filter paired rows under the first-commit causal guard.
pub fn prepare_forecast_adjustment_performance_pairs(
    pairs: &[ForecastAdjustmentPerformancePair],
    calendar: &mut impl ForecastAdjustmentScorecardCalendar,
) -> Result<ForecastAdjustmentPreparedPairs, ForecastAdjustmentScorecardError> {
    let coalesced = coalesce_forecast_adjustment_performance_pairs(pairs, calendar)?;
    let mut exclusions = ForecastAdjustmentPreparedPairsExclusions {
        invalid_target: 0,
        issued_after_valid: 0,
        missing_target: 0,
        receipt_chronology_missing: 0,
        source_late: 0,
    };
    let mut diagnostics = ForecastAdjustmentPreparedPairsDiagnostics {
        provenance_incomplete: 0,
    };
    let mut rows = Vec::new();
    let mut keys = HashSet::new();
    let mut fallback_count = 0_u64;

    // admit each paired occurrence
    for pair in coalesced.rows {
        let valid_at = canonical_instant(calendar, &pair.validAt)?;
        let local_date = calendar
            .los_angeles_local_date(&pair.validAt, valid_at)
            .ok_or(ForecastAdjustmentScorecardError::Calendar)?;
        let horizon_is_integer = pair.horizonHours.is_finite()
            && pair.horizonHours.fract() == 0.0
            && pair.horizonHours >= 1.0;

        // bind stable pair fields
        if pair.key.is_empty()
            || pair.rowIdentity.is_empty()
            || pair.targetKey.is_empty()
            || pair.vintageKey.is_empty()
            || local_date != pair.localDate
            || !pair.rawPrediction.is_finite()
            || !pair.adjustedPrediction.is_finite()
            || !horizon_is_integer
        {
            return Err(ForecastAdjustmentScorecardError::InvalidPair);
        }

        let pair_identity = format!("{}\0{}", pair.key, pair.targetKey);
        // reject true-vintage duplicates
        if !keys.insert(pair_identity) {
            return Err(ForecastAdjustmentScorecardError::DuplicateKey);
        }

        match &pair.target {
            Nullable::Null => {
                exclusions.missing_target += 1;
                continue;
            }
            Nullable::Value(target) if !target.is_finite() => {
                exclusions.invalid_target += 1;
                continue;
            }
            Nullable::Value(_) => {}
        }

        // preserve fallback equality
        if pair.fallback && !js_object_is(pair.rawPrediction, pair.adjustedPrediction) {
            return Err(ForecastAdjustmentScorecardError::FallbackDiffers);
        }

        // retain descriptive diagnostics
        if !pair.provenanceComplete {
            diagnostics.provenance_incomplete += 1;
        }

        let uses_receipt_guard = matches!(
            pair.evidenceClass,
            ForecastAdjustmentEvidenceClass::AsIssued
                | ForecastAdjustmentEvidenceClass::ProspectiveReceipt
        );
        // require receipt chronology
        if uses_receipt_guard
            && (matches!(pair.sourceReceiptAt, Nullable::Null)
                || matches!(pair.firstEdgeCommittedAt, Nullable::Null))
        {
            exclusions.receipt_chronology_missing += 1;
            continue;
        }

        let mut chronology_excluded = false;
        // apply claimed commit chronology
        if let Nullable::Value(committed_at) = &pair.firstEdgeCommittedAt {
            let committed_at = canonical_instant(calendar, committed_at)?;
            // count past issuance
            if committed_at >= valid_at {
                exclusions.issued_after_valid += 1;
                chronology_excluded = true;
            }

            let source_late = match &pair.sourceReceiptAt {
                Nullable::Null => true,
                Nullable::Value(source_receipt_at) => {
                    canonical_instant(calendar, source_receipt_at)? > committed_at
                }
            };
            // count late source evidence
            if source_late {
                exclusions.source_late += 1;
                chronology_excluded = true;
            }
        }

        // exclude once after counters
        if chronology_excluded {
            continue;
        }

        fallback_count += u64::from(pair.fallback);
        rows.push(pair);
    }

    Ok(ForecastAdjustmentPreparedPairs {
        coalescedDuplicateCount: coalesced.duplicate_count,
        diagnostics,
        exclusions,
        fallbackCount: fallback_count,
        rows,
    })
}

/// assign equal date, valid-hour, and vintage mass.
pub fn balance_forecast_adjustment_performance_pairs(
    pairs: &[ForecastAdjustmentPerformancePair],
) -> Result<Vec<ForecastAdjustmentWeightedPair>, ForecastAdjustmentScorecardError> {
    // reject empty populations
    if pairs.is_empty() {
        return Err(ForecastAdjustmentScorecardError::EmptyPopulation);
    }

    let evidence_class = pairs[0].evidenceClass;
    // prevent population pooling
    if pairs
        .iter()
        .any(|pair| pair.evidenceClass != evidence_class)
    {
        return Err(ForecastAdjustmentScorecardError::EvidenceClassesDiffer);
    }

    let mut by_date = BTreeMap::<String, PairsByHour<'_>>::new();
    // collect nested vintages
    for pair in pairs {
        // retain the closed canonical identifier collation
        if !sort_fields_are_ascii(pair) {
            return Err(ForecastAdjustmentScorecardError::InvalidPair);
        }
        by_date
            .entry(pair.localDate.clone())
            .or_default()
            .entry(pair.validAt.clone())
            .or_default()
            .push(pair);
    }

    let date_count = by_date.len() as f64;
    let mut weighted = Vec::with_capacity(pairs.len());
    // grant equal date mass
    for by_hour in by_date.values_mut() {
        let hour_count = by_hour.len() as f64;
        // grant equal hour mass
        for vintages in by_hour.values_mut() {
            vintages.sort_by(|left, right| left.vintageKey.cmp(&right.vintageKey));
            let vintage_count = vintages.len() as f64;
            // grant equal vintage mass
            for pair in vintages {
                weighted.push(weighted_pair(
                    pair,
                    1.0 / date_count / hour_count / vintage_count,
                ));
            }
        }
    }
    Ok(weighted)
}

/// score the paired raw and adjusted weighted amount distributions.
pub fn score_balanced_forecast_adjustment_pairs(
    pairs: &[ForecastAdjustmentPerformancePair],
) -> Result<BalancedForecastAdjustmentScore, ForecastAdjustmentScorecardError> {
    let weighted_rows = balance_forecast_adjustment_performance_pairs(pairs)?;
    let raw = metric_summary(&weighted_rows, false)?;
    let adjusted = metric_summary(&weighted_rows, true)?;
    let skill = paired_skill(raw.mae, adjusted.mae)?;
    Ok(BalancedForecastAdjustmentScore {
        metrics: ForecastAdjustmentPairedMetricSummary {
            delta: ForecastAdjustmentMetricSummary {
                bias: adjusted.bias - raw.bias,
                mae: adjusted.mae - raw.mae,
                p95AbsoluteError: adjusted.p95AbsoluteError - raw.p95AbsoluteError,
                rmse: adjusted.rmse - raw.rmse,
            },
            adjusted,
            raw,
            skill,
        },
        weighted_rows,
    })
}

/// parse one canonical instant through the approved owner.
fn canonical_instant(
    calendar: &mut impl ForecastAdjustmentScorecardCalendar,
    value: &str,
) -> Result<i64, ForecastAdjustmentScorecardError> {
    calendar
        .canonical_utc_epoch_milliseconds(value)
        .ok_or(ForecastAdjustmentScorecardError::Calendar)
}

/// compare the JSON-visible overlap content.
fn overlap_content_equal(
    left: &ForecastAdjustmentPerformancePair,
    right: &ForecastAdjustmentPerformancePair,
) -> bool {
    js_json_number_equal(left.adjustedPrediction, right.adjustedPrediction)
        && left.evidenceClass == right.evidenceClass
        && left.fallback == right.fallback
        && js_json_number_equal(left.horizonHours, right.horizonHours)
        && left.key == right.key
        && left.localDate == right.localDate
        && left.provenanceComplete == right.provenanceComplete
        && js_json_number_equal(left.rawPrediction, right.rawPrediction)
        && left.rowIdentity == right.rowIdentity
        && nullable_json_number_equal(&left.target, &right.target)
        && left.targetKey == right.targetKey
        && left.validAt == right.validAt
        && left.vintageKey == right.vintageKey
}

/// preserve JSON.stringify number equivalence.
fn js_json_number_equal(left: f64, right: f64) -> bool {
    (!left.is_finite() && !right.is_finite()) || left == right
}

/// preserve JSON.stringify nullable-number equivalence.
fn nullable_json_number_equal(left: &Nullable<f64>, right: &Nullable<f64>) -> bool {
    match (left, right) {
        (Nullable::Value(left), Nullable::Value(right)) => js_json_number_equal(*left, *right),
        (Nullable::Null, Nullable::Null) => true,
        (Nullable::Null, Nullable::Value(value)) | (Nullable::Value(value), Nullable::Null) => {
            !value.is_finite()
        }
    }
}

/// preserve Object.is for finite fallback values.
fn js_object_is(left: f64, right: f64) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

/// sort the closed string identities in source order.
fn pair_order(
    left: &ForecastAdjustmentPerformancePair,
    right: &ForecastAdjustmentPerformancePair,
) -> Ordering {
    left.localDate
        .cmp(&right.localDate)
        .then_with(|| left.validAt.cmp(&right.validAt))
        .then_with(|| left.vintageKey.cmp(&right.vintageKey))
        .then_with(|| left.key.cmp(&right.key))
}

/// restrict owner-free ordering to the report's canonical identifier alphabet.
fn sort_fields_are_ascii(pair: &ForecastAdjustmentPerformancePair) -> bool {
    pair.localDate.is_ascii()
        && pair.validAt.is_ascii()
        && pair.vintageKey.is_ascii()
        && pair.key.is_ascii()
}

/// copy one generated pair and append its nested weight.
fn weighted_pair(
    pair: &ForecastAdjustmentPerformancePair,
    weight: f64,
) -> ForecastAdjustmentWeightedPair {
    ForecastAdjustmentWeightedPair {
        adjustedPrediction: pair.adjustedPrediction,
        evidenceClass: pair.evidenceClass,
        fallback: pair.fallback,
        firstEdgeCommittedAt: pair.firstEdgeCommittedAt.clone(),
        horizonHours: pair.horizonHours,
        key: pair.key.clone(),
        localDate: pair.localDate.clone(),
        provenanceComplete: pair.provenanceComplete,
        rawPrediction: pair.rawPrediction,
        rowIdentity: pair.rowIdentity.clone(),
        sourceReceiptAt: pair.sourceReceiptAt.clone(),
        target: pair.target.clone(),
        targetKey: pair.targetKey.clone(),
        validAt: pair.validAt.clone(),
        vintageKey: pair.vintageKey.clone(),
        weight,
    }
}

/// compute one weighted error distribution.
fn metric_summary(
    pairs: &[ForecastAdjustmentWeightedPair],
    adjusted: bool,
) -> Result<ForecastAdjustmentMetricSummary, ForecastAdjustmentScorecardError> {
    let mut absolute = 0.0;
    let mut signed = 0.0;
    let mut squared = 0.0;
    let mut errors = Vec::with_capacity(pairs.len());
    // aggregate paired errors
    for pair in pairs {
        let Nullable::Value(target) = &pair.target else {
            return Err(ForecastAdjustmentScorecardError::LostTarget);
        };
        let prediction = if adjusted {
            pair.adjustedPrediction
        } else {
            pair.rawPrediction
        };
        let error = prediction - *target;
        absolute += pair.weight * error.abs();
        signed += pair.weight * error;
        squared += pair.weight * error.powi(2);
        errors.push((error.abs(), pair.weight));
    }
    Ok(ForecastAdjustmentMetricSummary {
        bias: signed,
        mae: absolute,
        p95AbsoluteError: weighted_quantile(&mut errors, 0.95)?,
        rmse: squared.sqrt(),
    })
}

/// select the deterministic left-continuous weighted quantile.
fn weighted_quantile(
    values: &mut [(f64, f64)],
    probability: f64,
) -> Result<f64, ForecastAdjustmentScorecardError> {
    values.sort_by(|left, right| left.0.partial_cmp(&right.0).unwrap_or(Ordering::Equal));
    let mut cumulative = 0.0;
    // return the first reached value
    for (value, weight) in values.iter() {
        cumulative += weight;
        // preserve epsilon boundary
        if cumulative >= probability - f64::EPSILON {
            return Ok(*value);
        }
    }
    values
        .last()
        .map(|entry| entry.0)
        .ok_or(ForecastAdjustmentScorecardError::EmptyPopulation)
}

/// compute the original zero-safe paired skill.
fn paired_skill(
    raw_loss: f64,
    adjusted_loss: f64,
) -> Result<f64, ForecastAdjustmentScorecardError> {
    // reject invalid losses
    if !raw_loss.is_finite() || !adjusted_loss.is_finite() || raw_loss < 0.0 || adjusted_loss < 0.0
    {
        return Err(ForecastAdjustmentScorecardError::InvalidPair);
    }
    // preserve equal zero loss
    if raw_loss == 0.0 && adjusted_loss == 0.0 {
        return Ok(0.0);
    }
    // preserve harmed zero baseline
    if raw_loss == 0.0 {
        return Ok(-1.0);
    }
    Ok((raw_loss - adjusted_loss) / raw_loss)
}

#[cfg(test)]
#[path = "performance_scorecard_amount_test.rs"]
mod tests;
