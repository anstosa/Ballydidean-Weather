//! Provider-balanced qualification of the original pre-holdout wind folds.

use std::collections::HashMap;

use weather_contracts::{
    DevelopmentLosoFoldResultV1, DevelopmentLosoFoldResultV1ContractVersionLiteral,
    DevelopmentLosoFoldResultV1FoldEnum, DevelopmentLosoStationScoreV1,
    ForecastAdjustmentCoefficient, ForecastAdjustmentMetric, ForecastAdjustmentMetricBand,
    ForecastAdjustmentTrainingEnvelope, ForecastAdjustmentTrainingEnvelopeMetricEnum,
    ForecastLeadBandKey, ForecastObservationProviderFamily, ForecastObservationStation,
    ForecastObservationStationKey,
};

use crate::wind_moving_block_bootstrap::{
    WindMovingBlockBootstrapError, WindMovingBlockStartPlan, wind_paired_skill,
};
use crate::wind_training_events::station_key;
use crate::{
    AdmittedWindDevelopmentFold, ReadyWindMaintenanceTraining, WindBootstrapDateSlot,
    WindBootstrapPairedEvent, WindBootstrapScoredWindow, WindDevelopmentStationError,
    WindRobustHierarchyError, WindTrainingEvent, create_wind_moving_block_start_plan,
    score_wind_development_stations, score_wind_moving_block_bootstrap_plan,
};

type Fold = DevelopmentLosoFoldResultV1FoldEnum;

const FOLDS: [Fold; 5] = [
    Fold::Value1,
    Fold::Value2,
    Fold::Value3,
    Fold::Value4,
    Fold::Value5,
];
const MINIMUM_TRAINING_MATCHES: u64 = 500;
const MINIMUM_SCORE_MATCHES: u64 = 100;
const MINIMUM_STATIONS: usize = 5;
const MINIMUM_FAMILIES: usize = 3;
const MINIMUM_SKILL: f64 = 0.02;
const MINIMUM_NONNEGATIVE_FRACTION: f64 = 0.8;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const FINAL_TRAINING_DAYS: usize = 365;
const SCORE_DAYS: usize = 30;
const EMBARGO_DAYS: usize = 7;
const FIRST_TRAINING_DAYS: usize = 180;
const FAMILY_COUNT: usize = 4;

/// Ordinary failures while deriving one development fold or its promotion verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindDevelopmentError {
    InvalidStationScore,
    IncompleteAuxiliaryCoverage,
    NoScoreableStations,
    InvalidFoldSequence,
    InconsistentFoldEvidence,
}

/// One calendar-owner-admitted Los Angeles label and its local day ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedWindDevelopmentDate<'a> {
    pub label: &'a str,
    pub ordinal: i64,
}

/// Exact typed material for the existing canonical serialization and digest owner.
#[derive(Debug, Clone, Copy)]
pub struct WindAuxiliaryModel<'a> {
    pub coefficients: &'a [ForecastAdjustmentCoefficient],
    pub fold: Fold,
    pub metric_band: &'a ForecastAdjustmentMetricBand,
    pub physical_station_key: ForecastObservationStationKey,
}

/// Qualified pair with final-training-only numerical material, not a published artifact.
#[derive(Debug, Clone, PartialEq)]
pub struct WindDevelopmentQualifiedPair {
    pub pair: ForecastAdjustmentMetricBand,
    pub folds: [DevelopmentLosoFoldResultV1; 5],
    pub coefficients: Vec<ForecastAdjustmentCoefficient>,
    pub envelope: ForecastAdjustmentTrainingEnvelope,
}

/// Development result before immutable report and candidate construction.
#[derive(Debug, Clone, PartialEq)]
pub enum WindDevelopmentQualification {
    NoQualifiedPair,
    Qualified(Vec<WindDevelopmentQualifiedPair>),
}

/// Preserve the ordinary owner or numerical failure at its original boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindDevelopmentRunError<E> {
    Dates,
    Station(WindDevelopmentStationError),
    Bootstrap(WindMovingBlockBootstrapError),
    Fold(WindDevelopmentError),
    Digest(E),
    FinalFit(WindRobustHierarchyError),
    TrainingEnvelope,
}

/// Score the frozen five-fold schedule and refit only promoted speed/gust pairs.
///
/// The caller admits D0–D364 labels against the original calendar owner and supplies
/// the canonical digest capability. This interface never receives confirmation rows.
pub fn qualify_wind_maintenance_development<E>(
    training: &ReadyWindMaintenanceTraining<'_>,
    stations: &[ForecastObservationStation],
    dates: &[AdmittedWindDevelopmentDate<'_>; FINAL_TRAINING_DAYS],
    mut digest: impl FnMut(WindAuxiliaryModel<'_>) -> Result<String, E>,
) -> Result<WindDevelopmentQualification, WindDevelopmentRunError<E>> {
    let labels = dates.map(|date| date.label);
    // enforce contiguous chronology and reject any event outside final training
    if dates.windows(2).any(|days| {
        days[0].label >= days[1].label || days[0].ordinal.checked_add(1) != Some(days[1].ordinal)
    }) || training
        .events()
        .events
        .iter()
        .any(|event| labels.binary_search(&event.local_date).is_err())
    {
        return Err(WindDevelopmentRunError::Dates);
    }
    let plan = create_wind_moving_block_start_plan(&[SCORE_DAYS])
        .map_err(WindDevelopmentRunError::Bootstrap)?;
    let mut promoted = Vec::new();
    // evaluate pair keys in original lexical metric and lead-band order
    for metric in [
        ForecastAdjustmentMetric::WindGustMps,
        ForecastAdjustmentMetric::WindSpeedMps,
    ] {
        // retain each observed pair independently
        for &(_, lead_band) in ForecastLeadBandKey::WIRE_VALUES {
            let pair = ForecastAdjustmentMetricBand {
                leadBand: lead_band,
                metric,
            };
            // never fabricate folds for a missing pair
            if !training
                .events()
                .events
                .iter()
                .any(|event| matches_pair(event, &pair))
            {
                continue;
            }
            let mut results = Vec::with_capacity(FOLDS.len());
            // use fixed rolling expansion, an exact seven-day embargo, and thirty score dates
            for (index, fold) in FOLDS.into_iter().enumerate() {
                let end_training = FIRST_TRAINING_DAYS + index * (EMBARGO_DAYS + SCORE_DAYS);
                let start_score = end_training + EMBARGO_DAYS;
                let end_score = start_score + SCORE_DAYS;
                let retained = score_wind_development_stations(
                    training.events(),
                    stations,
                    &pair,
                    &AdmittedWindDevelopmentFold {
                        training_local_dates: &labels[..end_training],
                        score_local_dates: &labels[start_score..end_score],
                    },
                )
                .map_err(WindDevelopmentRunError::Station)?;
                results.push(score_retained_wind_development_fold(
                    &pair,
                    fold,
                    stations,
                    retained,
                    &dates[start_score..end_score],
                    &plan,
                    &mut digest,
                )?);
            }
            let folds = results.try_into().map_err(|_| {
                WindDevelopmentRunError::Fold(WindDevelopmentError::InvalidFoldSequence)
            })?;
            // never fit the final candidate's coefficients or envelope for a disabled pair
            if wind_development_pair_passes(&pair, &folds).map_err(WindDevelopmentRunError::Fold)? {
                promoted.push((pair, folds));
            }
        }
    }
    // distinguish a literal development insufficiency from an arithmetic or input error
    if promoted.is_empty() {
        return Ok(WindDevelopmentQualification::NoQualifiedPair);
    }
    let mut qualified = Vec::with_capacity(promoted.len());
    // finish all development folds before fitting final-training coefficients
    for (pair, folds) in promoted {
        let coefficients = training
            .fit_pair(&pair)
            .map_err(WindDevelopmentRunError::FinalFit)?;
        let envelope = training_envelope(&training.events().events, &pair)
            .ok_or(WindDevelopmentRunError::TrainingEnvelope)?;
        qualified.push(WindDevelopmentQualifiedPair {
            pair,
            folds,
            coefficients,
            envelope,
        });
    }
    Ok(WindDevelopmentQualification::Qualified(qualified))
}

#[derive(Debug, Default, Clone, Copy)]
struct WindLossTotal {
    raw_sum: f64,
    adjusted_sum: f64,
    count: usize,
}

impl WindLossTotal {
    /// Accumulate same-event scalar losses in original chronological order.
    fn add(&mut self, event: WindBootstrapPairedEvent) {
        self.raw_sum += (event.actual - event.raw_prediction).abs();
        self.adjusted_sum += (event.actual - event.adjusted_prediction).abs();
        self.count += 1;
    }

    /// Project one station/day mean or one family/day mean without regrouping addition.
    fn mean(self) -> WindBootstrapPairedEvent {
        WindBootstrapPairedEvent {
            actual: 0.0,
            adjusted_prediction: self.adjusted_sum / self.count as f64,
            raw_prediction: self.raw_sum / self.count as f64,
        }
    }
}

/// Preserve both whole-station and per-day provider weighting for one retained fold.
fn score_retained_wind_development_fold<E>(
    pair: &ForecastAdjustmentMetricBand,
    fold: Fold,
    stations: &[ForecastObservationStation],
    retained: Vec<crate::RetainedWindDevelopmentStationScore<'_, '_>>,
    dates: &[AdmittedWindDevelopmentDate<'_>],
    plan: &WindMovingBlockStartPlan,
    digest: &mut impl FnMut(WindAuxiliaryModel<'_>) -> Result<String, E>,
) -> Result<DevelopmentLosoFoldResultV1, WindDevelopmentRunError<E>> {
    // match the original failure instead of promoting an undefined provider mean
    if retained.is_empty() {
        return Err(WindDevelopmentRunError::Fold(
            WindDevelopmentError::NoScoreableStations,
        ));
    }
    let slots_by_label: HashMap<_, _> = dates
        .iter()
        .enumerate()
        .map(|(index, date)| (date.label, index))
        .collect();
    let mut family_days = vec![[WindLossTotal::default(); FAMILY_COUNT]; dates.len()];
    let mut scores = Vec::with_capacity(retained.len());
    let mut hashes = Vec::with_capacity(retained.len());
    let mut by_station_day = Vec::with_capacity(retained.len());
    // preserve original station roster order for within-family daily means
    for station in retained {
        let key = station.station.key;
        let family = station.station.providerFamily;
        let family_index = family_index(family);
        let mut day_events = vec![Vec::new(); dates.len()];
        let mut daily_totals = vec![WindLossTotal::default(); dates.len()];
        let mut point = WindLossTotal::default();
        // keep score order in the point total and within each sampled date
        for paired in &station.paired_events {
            let index = slots_by_label
                .get(paired.local_date)
                .copied()
                .ok_or(WindDevelopmentRunError::Dates)?;
            point.add(paired.event);
            daily_totals[index].add(paired.event);
            day_events[index].push(paired.event);
        }
        let point = point.mean();
        let point_skill = wind_paired_skill(point.raw_prediction, point.adjusted_prediction)
            .map_err(WindDevelopmentRunError::Bootstrap)?;
        scores.push(DevelopmentLosoStationScoreV1 {
            adjustedLoss: point.adjusted_prediction,
            eventCount: station.paired_events.len() as u64,
            physicalStationKey: key,
            pointSkill: point_skill,
            providerFamily: family,
            rawLoss: point.raw_prediction,
            remainingNetworkScoreEvents: station.remaining_network_score_events as u64,
            scoreMatches: station.score_matches as u64,
            trainingMatches: station.training_matches as u64,
        });
        hashes.push(
            digest(WindAuxiliaryModel {
                coefficients: &station.coefficients,
                fold,
                metric_band: pair,
                physical_station_key: key,
            })
            .map_err(WindDevelopmentRunError::Digest)?,
        );
        // give each present station one daily contribution with no weight for missing dates
        for (families, daily) in family_days.iter_mut().zip(daily_totals) {
            // omit stations with no paired events on this specific local day
            if daily.count > 0 {
                families[family_index].add(daily.mean());
            }
        }
        by_station_day.push((key, day_events));
    }
    let balanced = family_days
        .into_iter()
        .map(|families| {
            // retain exactly one ordered occurrence per represented family and day
            families
                .into_iter()
                .filter(|daily| daily.count > 0)
                .map(WindLossTotal::mean)
                .collect()
        })
        .collect::<Vec<Vec<_>>>();
    let bootstrap = bootstrap_wind_days(dates, &balanced, "provider-balanced", plan)
        .map_err(WindDevelopmentRunError::Bootstrap)?;
    let mut harm = Vec::new();
    // evaluate each original station's harm only after provider-balanced scoring
    for (key, day_events) in by_station_day {
        let station_bootstrap = bootstrap_wind_days(dates, &day_events, station_key(key), plan)
            .map_err(WindDevelopmentRunError::Bootstrap)?;
        // retain the literal event count, point skill, and upper-bound thresholds
        if station_bootstrap.event_count >= 100
            && station_bootstrap.skill <= -0.02
            && station_bootstrap.bootstrap_upper_bound < 0.0
        {
            harm.push(key);
        }
    }
    evaluate_wind_development_loso_fold(
        pair,
        fold,
        stations,
        &scores,
        &hashes,
        bootstrap.bootstrap_lower_bound,
        &harm,
    )
    .map_err(WindDevelopmentRunError::Fold)
}

/// Index the same lexical provider order in both aggregation layers.
const fn family_index(family: ForecastObservationProviderFamily) -> usize {
    match family {
        ForecastObservationProviderFamily::Ambient => 0,
        ForecastObservationProviderFamily::Ecowitt => 1,
        ForecastObservationProviderFamily::Netatmo => 2,
        ForecastObservationProviderFamily::Tempest => 3,
    }
}

/// Bootstrap a declared window, retaining zero-event date slots.
fn bootstrap_wind_days(
    dates: &[AdmittedWindDevelopmentDate<'_>],
    events: &[Vec<WindBootstrapPairedEvent>],
    key: &str,
    plan: &WindMovingBlockStartPlan,
) -> Result<
    crate::wind_moving_block_bootstrap::WindMovingBlockBootstrapResult,
    WindMovingBlockBootstrapError,
> {
    let date_slots = dates
        .iter()
        .zip(events)
        .map(|(date, events)| WindBootstrapDateSlot {
            local_day_ordinal: date.ordinal,
            events,
        })
        .collect::<Vec<_>>();
    score_wind_moving_block_bootstrap_plan(
        &[WindBootstrapScoredWindow {
            date_slots: &date_slots,
            key,
        }],
        plan,
        false,
    )
}

/// Keep the original fitted pair identity.
fn matches_pair(event: &WindTrainingEvent<'_>, pair: &ForecastAdjustmentMetricBand) -> bool {
    event.metric == pair.metric && event.lead_band == pair.leadBand
}

/// Derive the final unmodified raw-value envelope for the selected scalar pair.
fn training_envelope(
    events: &[WindTrainingEvent<'_>],
    pair: &ForecastAdjustmentMetricBand,
) -> Option<ForecastAdjustmentTrainingEnvelope> {
    let metric = match pair.metric {
        ForecastAdjustmentMetric::WindGustMps => {
            ForecastAdjustmentTrainingEnvelopeMetricEnum::WindGustMps
        }
        ForecastAdjustmentMetric::WindSpeedMps => {
            ForecastAdjustmentTrainingEnvelopeMetricEnum::WindSpeedMps
        }
        _ => return None,
    };
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    // preserve inclusive original raw bounds and numeric signed-zero ordering
    for event in events.iter().filter(|event| matches_pair(event, pair)) {
        let raw = event.raw_forecast;
        // refuse noncanonical source values rather than fitting an envelope to other rows
        if !(0.0..=150.0).contains(&raw) {
            return None;
        }
        // use the IEEE order to retain Math.min's negative-zero result
        if raw.total_cmp(&minimum).is_lt() {
            minimum = raw;
        }
        // use the IEEE order to retain Math.max's positive-zero result
        if raw.total_cmp(&maximum).is_gt() {
            maximum = raw;
        }
    }
    minimum
        .is_finite()
        .then_some(ForecastAdjustmentTrainingEnvelope {
            leadBand: pair.leadBand,
            maximum,
            metric,
            minimum,
        })
}

/// Derive a generated fold result from admitted canonical station identities and model hashes.
pub fn evaluate_wind_development_loso_fold(
    pair: &ForecastAdjustmentMetricBand,
    fold: Fold,
    stations: &[ForecastObservationStation],
    station_scores: &[DevelopmentLosoStationScoreV1],
    auxiliary_model_sha256s: &[String],
    bootstrap_lower_bound: f64,
    material_harm: &[ForecastObservationStationKey],
) -> Result<DevelopmentLosoFoldResultV1, WindDevelopmentError> {
    let mut hashes = auxiliary_model_sha256s.to_vec();
    hashes.sort_unstable();
    let mut scores = station_scores.iter().collect::<Vec<_>>();
    scores.sort_unstable_by_key(|score| station_key(score.physicalStationKey));
    // validate all submitted rows before applying the frozen coverage requirements
    if !bootstrap_lower_bound.is_finite()
        || hashes.iter().any(|hash| !valid_digest(hash))
        || hashes.windows(2).any(|pair| pair[0] == pair[1])
        || scores
            .windows(2)
            .any(|rows| rows[0].physicalStationKey == rows[1].physicalStationKey)
        || scores
            .iter()
            .any(|score| !valid_station_score(score, stations))
    {
        return Err(WindDevelopmentError::InvalidStationScore);
    }

    // refuse partial auxiliary fits rather than silently dropping submitted stations
    if scores.len() != hashes.len()
        || scores.iter().any(|score| {
            score.trainingMatches < MINIMUM_TRAINING_MATCHES
                || score.scoreMatches < MINIMUM_SCORE_MATCHES
                || score.remainingNetworkScoreEvents < MINIMUM_SCORE_MATCHES
        })
    {
        return Err(WindDevelopmentError::IncompleteAuxiliaryCoverage);
    }
    // the original equal-provider loss is undefined for zero scoreable stations
    if scores.is_empty() {
        return Err(WindDevelopmentError::NoScoreableStations);
    }

    let mut families = Vec::with_capacity(ForecastObservationProviderFamily::WIRE_VALUES.len());
    let mut raw_sum = 0.0;
    let mut adjusted_sum = 0.0;
    // take one equal station mean per family then weight each present family equally
    for (_, family) in ForecastObservationProviderFamily::WIRE_VALUES {
        let mut raw = 0.0;
        let mut adjusted = 0.0;
        let mut count = 0;
        // keep the original sorted station order inside each family sum
        for score in scores
            .iter()
            .filter(|score| score.providerFamily == *family)
        {
            raw += score.rawLoss;
            adjusted += score.adjustedLoss;
            count += 1;
        }
        // omit absent families rather than assigning a zero contribution
        if count > 0 {
            families.push(*family);
            raw_sum += raw / f64::from(count);
            adjusted_sum += adjusted / f64::from(count);
        }
    }
    let raw = raw_sum / families.len() as f64;
    let adjusted = adjusted_sum / families.len() as f64;
    let skill =
        wind_paired_skill(raw, adjusted).map_err(|_| WindDevelopmentError::InvalidStationScore)?;
    let nonnegative = scores
        .iter()
        .filter(|score| score.pointSkill >= 0.0)
        .count() as f64
        / scores.len() as f64;
    let mut harm = material_harm
        .iter()
        .map(|key| station_key(*key).to_owned())
        .collect::<Vec<_>>();
    harm.sort_unstable();
    let passed = scores.len() >= MINIMUM_STATIONS
        && families.len() >= MINIMUM_FAMILIES
        && skill >= MINIMUM_SKILL
        && bootstrap_lower_bound > 0.0
        && nonnegative >= MINIMUM_NONNEGATIVE_FRACTION
        && harm.is_empty();

    Ok(DevelopmentLosoFoldResultV1 {
        auxiliaryModelSha256s: hashes,
        bootstrapLowerBound: bootstrap_lower_bound,
        contractVersion: DevelopmentLosoFoldResultV1ContractVersionLiteral,
        fold,
        materialHarmSliceKeys: harm,
        metricBand: pair.clone(),
        nonnegativeStationFraction: nonnegative,
        passed,
        providerBalancedAdjustedLoss: adjusted,
        providerBalancedRawLoss: raw,
        providerBalancedSkill: skill,
        providerFamilies: families,
        scoreableStationKeys: scores
            .iter()
            .map(|score| score.physicalStationKey)
            .collect(),
    })
}

/// Promote only four passing folds including the fifth for the same original pair.
pub fn wind_development_pair_passes(
    pair: &ForecastAdjustmentMetricBand,
    folds: &[DevelopmentLosoFoldResultV1; 5],
) -> Result<bool, WindDevelopmentError> {
    // require exact identity and chronological position in the admitted schedule
    if folds
        .iter()
        .zip(FOLDS)
        .any(|(result, number)| &result.metricBand != pair || result.fold != number)
    {
        return Err(WindDevelopmentError::InvalidFoldSequence);
    }
    Ok(folds[4].passed && folds.iter().filter(|result| result.passed).count() >= 4)
}

/// Recheck the original aggregate evidence before an immutable report is created.
pub fn validate_wind_development_fold(
    fold: &DevelopmentLosoFoldResultV1,
    stations: &[ForecastObservationStation],
) -> Result<(), WindDevelopmentError> {
    use WindDevelopmentError::InconsistentFoldEvidence;
    let mut families = Vec::new();
    let mut keys = Vec::with_capacity(fold.scoreableStationKeys.len());
    // bind each scoreable station to the admitted original provider family
    for key in &fold.scoreableStationKeys {
        let station = stations
            .iter()
            .find(|station| station.key == *key)
            .ok_or(InconsistentFoldEvidence)?;
        keys.push(station_key(*key));
        families.push(station.providerFamily);
    }
    keys.sort_unstable();
    let expected_families: Vec<_> = ForecastObservationProviderFamily::WIRE_VALUES
        .iter()
        .filter_map(|(_, family)| families.contains(family).then_some(*family))
        .collect();
    let derived_pass = keys.len() >= MINIMUM_STATIONS
        && expected_families.len() >= MINIMUM_FAMILIES
        && fold.providerBalancedSkill >= MINIMUM_SKILL
        && fold.bootstrapLowerBound > 0.0
        && fold.nonnegativeStationFraction >= MINIMUM_NONNEGATIVE_FRACTION
        && fold.materialHarmSliceKeys.is_empty();
    // require the exact derived verdict, roster coverage and finite generated fields
    if fold.passed != derived_pass
        || fold.auxiliaryModelSha256s.len() != keys.len()
        || keys.windows(2).any(|pair| pair[0] == pair[1])
        || fold.providerFamilies != expected_families
        || ![
            fold.bootstrapLowerBound,
            fold.nonnegativeStationFraction,
            fold.providerBalancedAdjustedLoss,
            fold.providerBalancedRawLoss,
            fold.providerBalancedSkill,
        ]
        .into_iter()
        .all(f64::is_finite)
    {
        return Err(InconsistentFoldEvidence);
    }
    Ok(())
}

/// Check the original lowercase textual SHA-256 representation without hashing.
fn valid_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Require a complete finite score bound to the admitted catalog's family identity.
fn valid_station_score(
    score: &DevelopmentLosoStationScoreV1,
    stations: &[ForecastObservationStation],
) -> bool {
    let family = stations
        .iter()
        .find(|station| station.key == score.physicalStationKey)
        .map(|station| station.providerFamily);
    family == Some(score.providerFamily)
        && score.eventCount > 0
        && [
            score.eventCount,
            score.trainingMatches,
            score.scoreMatches,
            score.remainingNetworkScoreEvents,
        ]
        .into_iter()
        .all(|count| count <= MAX_SAFE_INTEGER)
        && score.pointSkill.is_finite()
        && wind_paired_skill(score.rawLoss, score.adjustedLoss)
            .is_ok_and(|skill| (skill - score.pointSkill).abs() <= f64::EPSILON * 8.0)
}
