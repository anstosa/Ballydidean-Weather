use weather_contracts::{
    CanonicalWeatherMetrics, CoreAdjustmentApplicationInput,
    CoreAdjustmentApplicationInputEnvelope,
    CoreAdjustmentApplicationResultReasonEnum as AdjustmentReason, ForecastAdjustmentCandidateV2,
    ForecastAdjustmentCoefficient, ForecastAdjustmentCoefficientDaypartEnum,
    ForecastAdjustmentCoefficientLevelEnum, ForecastAdjustmentCoefficientSeasonEnum,
    ForecastAdjustmentDecision, ForecastAdjustmentFailRawDecision,
    ForecastAdjustmentFailRawDecisionAdjustedMetricsRecord,
    ForecastAdjustmentFailRawDecisionAppliedMetricsEmptyArray,
    ForecastAdjustmentFailRawDecisionContractVersionLiteral,
    ForecastAdjustmentFailRawDecisionStateEnum, ForecastAdjustmentMetric,
    ForecastAdjustmentReasonCode, ForecastAdjustmentTrainingEnvelope,
    ForecastAdjustmentTrainingEnvelopeMetricEnum, ForecastAdjustmentWindCanaryActiveDecision,
    ForecastAdjustmentWindCanaryActiveDecisionActivationKindLiteral,
    ForecastAdjustmentWindCanaryActiveDecisionAdjustedMetricsRecord,
    ForecastAdjustmentWindCanaryActiveDecisionAlgorithmContractVersionLiteral,
    ForecastAdjustmentWindCanaryActiveDecisionContractVersionLiteral,
    ForecastAdjustmentWindCanaryActiveDecisionReasonCodeNull,
    ForecastAdjustmentWindCanaryActiveDecisionStateLiteral,
    ForecastAdjustmentWindCanaryCandidateV1, ForecastAdjustmentWindCanaryMetric,
    ForecastLeadBandKey, Nullable,
};

use crate::apply_core_adjustment;

const WIND_METRICS: [(ForecastAdjustmentMetric, ForecastAdjustmentWindCanaryMetric); 3] = [
    (
        ForecastAdjustmentMetric::WindDirectionDegrees,
        ForecastAdjustmentWindCanaryMetric::WindDirectionDegrees,
    ),
    (
        ForecastAdjustmentMetric::WindGustMps,
        ForecastAdjustmentWindCanaryMetric::WindGustMps,
    ),
    (
        ForecastAdjustmentMetric::WindSpeedMps,
        ForecastAdjustmentWindCanaryMetric::WindSpeedMps,
    ),
];

const MAINTENANCE_V2_METRICS: [ForecastAdjustmentMetric; 3] = [
    ForecastAdjustmentMetric::WindDirectionDegrees,
    ForecastAdjustmentMetric::WindGustMps,
    ForecastAdjustmentMetric::WindSpeedMps,
];

const STANDARD_V2_METRICS: [ForecastAdjustmentMetric; 5] = [
    ForecastAdjustmentMetric::RelativeHumidityPercent,
    ForecastAdjustmentMetric::TemperatureC,
    ForecastAdjustmentMetric::WindDirectionDegrees,
    ForecastAdjustmentMetric::WindGustMps,
    ForecastAdjustmentMetric::WindSpeedMps,
];

/// First source-ordered cells selected during one coefficient-table pass.
#[derive(Debug, Clone)]
pub(crate) struct SelectedHierarchy {
    pub(crate) coefficient: Nullable<f64>,
    root_available: bool,
}

/// Verified active bundle identities and canonical candidate tables.
#[derive(Debug, Clone, Copy)]
pub struct AdmittedWindBundleRow<'a> {
    pub authorization_sha256: &'a str,
    pub candidate: &'a ForecastAdjustmentWindCanaryCandidateV1,
    pub transfer_report_sha256: &'a str,
}

/// Already derived local calendar fields used by hierarchy selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedWindRowCalendar {
    pub daypart: ForecastAdjustmentCoefficientDaypartEnum,
    pub month: u64,
    pub season: ForecastAdjustmentCoefficientSeasonEnum,
}

/// Result of the exact source identity admission performed outside this pure adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmittedWindRowSourceIdentity {
    Match,
    IdentityMismatch,
    WrongCohort,
}

/// Typed inputs that remain after time, source, and artifact parsing.
#[derive(Debug, Clone, Copy)]
pub struct AdmittedWindRowFeatures<'a> {
    pub calendar: AdmittedWindRowCalendar,
    pub calendar_fingerprint_matches: bool,
    pub metrics: &'a CanonicalWeatherMetrics,
    pub raw_forecast_provenance: &'a weather_contracts::ForecastAdjustmentRawForecastProvenance,
    pub source_identity: AdmittedWindRowSourceIdentity,
}

/// Select the exact TypeScript metric iteration order for one V2 candidate row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmittedWindV2MetricOrder {
    Maintenance,
    Standard,
}

/// authority-free numerical wind projection from one admitted V2 candidate row
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindV2RowOutcome {
    pub humidity_adjusted: Option<f64>,
    pub temperature_adjusted: Option<f64>,
    pub direction_adjusted: Option<f64>,
    pub speed_adjusted: Option<f64>,
    pub gust_adjusted: Option<f64>,
    pub lead_band: Option<ForecastLeadBandKey>,
    pub reason: Option<ForecastAdjustmentReasonCode>,
}

/// Evaluate one verified active wind bundle row using the production hierarchy and guard order.
pub fn evaluate_admitted_wind_bundle_row(
    bundle: AdmittedWindBundleRow<'_>,
    row: AdmittedWindRowFeatures<'_>,
) -> ForecastAdjustmentDecision {
    let Some(lead_band) = lead_band_for(row.raw_forecast_provenance.targetLeadHours) else {
        return fail_raw(ForecastAdjustmentReasonCode::UnsupportedLead);
    };

    // preserve the whole-row source failure before any metric evaluation
    if row.source_identity != AdmittedWindRowSourceIdentity::Match {
        let reason = match row.source_identity {
            AdmittedWindRowSourceIdentity::IdentityMismatch => {
                ForecastAdjustmentReasonCode::IdentityMismatch
            }
            AdmittedWindRowSourceIdentity::WrongCohort => ForecastAdjustmentReasonCode::WrongCohort,
            AdmittedWindRowSourceIdentity::Match => unreachable!(),
        };
        return fail_raw(reason);
    }

    let mut adjusted_metrics = Vec::with_capacity(WIND_METRICS.len());
    let mut applied_metrics = Vec::with_capacity(WIND_METRICS.len());
    let mut first_failure = None;

    // evaluate the exact source allowlist in its stable public order
    for (metric, wind_metric) in WIND_METRICS {
        let Some(raw_value) = raw_metric(row.metrics, metric) else {
            continue;
        };
        let SelectedHierarchy {
            coefficient,
            root_available,
        } = select_hierarchy_coefficient(
            &bundle.candidate.coefficients,
            metric,
            lead_band,
            row.calendar,
        );
        let result = apply_core_adjustment(&CoreAdjustmentApplicationInput {
            calendarFingerprintMatches: row.calendar_fingerprint_matches,
            coefficient,
            enabled: bundle
                .candidate
                .enabledMetricBands
                .iter()
                .any(|pair| pair.metric == metric && pair.leadBand == lead_band),
            envelope: training_envelope(&bundle.candidate.trainingEnvelopes, metric, lead_band),
            identityMatches: true,
            metric,
            rawForecastValue: raw_value,
            rawWindSpeedMps: row.metrics.windSpeedMps.clone(),
            rootAvailable: root_available,
        });

        // retain successful results while preserving the first guard failure
        if result.applied {
            adjusted_metrics.push((wind_metric, result.adjustedValue));
            applied_metrics.push(wind_metric);
        } else if first_failure.is_none() {
            first_failure = Some(result.reason);
        }
    }

    // return the exact all-null or first-failure public result when nothing applied
    if applied_metrics.is_empty() {
        return fail_raw(match first_failure {
            Some(reason) => map_core_failure(reason),
            None => ForecastAdjustmentReasonCode::MetricNotEnabled,
        });
    }

    ForecastAdjustmentDecision::WindCanaryActive(ForecastAdjustmentWindCanaryActiveDecision {
        activationKind: ForecastAdjustmentWindCanaryActiveDecisionActivationKindLiteral,
        adjustedMetrics: ForecastAdjustmentWindCanaryActiveDecisionAdjustedMetricsRecord(
            adjusted_metrics,
        ),
        algorithmContractVersion:
            ForecastAdjustmentWindCanaryActiveDecisionAlgorithmContractVersionLiteral,
        appliedMetrics: applied_metrics,
        authorizationSha256: bundle.authorization_sha256.to_owned(),
        candidateArtifactSha256: bundle.candidate.candidateArtifactSha256.clone(),
        contractVersion: ForecastAdjustmentWindCanaryActiveDecisionContractVersionLiteral,
        leadBand: lead_band,
        rawForecastProvenance: row.raw_forecast_provenance.clone(),
        reasonCode: ForecastAdjustmentWindCanaryActiveDecisionReasonCodeNull,
        state: ForecastAdjustmentWindCanaryActiveDecisionStateLiteral,
        transferReportSha256: bundle.transfer_report_sha256.to_owned(),
    })
}

/// Evaluate one admitted standard V2 candidate row without activation or serving authority.
pub fn evaluate_admitted_wind_v2_row(
    candidate: &ForecastAdjustmentCandidateV2,
    row: AdmittedWindRowFeatures<'_>,
    order: AdmittedWindV2MetricOrder,
) -> WindV2RowOutcome {
    let Some(lead_band) = lead_band_for(row.raw_forecast_provenance.targetLeadHours) else {
        return wind_v2_failure(ForecastAdjustmentReasonCode::UnsupportedLead);
    };

    // preserve whole-row source admission before any numerical metric result
    if row.source_identity != AdmittedWindRowSourceIdentity::Match {
        let reason = match row.source_identity {
            AdmittedWindRowSourceIdentity::IdentityMismatch => {
                ForecastAdjustmentReasonCode::IdentityMismatch
            }
            AdmittedWindRowSourceIdentity::WrongCohort => ForecastAdjustmentReasonCode::WrongCohort,
            AdmittedWindRowSourceIdentity::Match => unreachable!(),
        };
        return wind_v2_failure(reason);
    }

    let metrics = match order {
        AdmittedWindV2MetricOrder::Maintenance => MAINTENANCE_V2_METRICS.as_slice(),
        AdmittedWindV2MetricOrder::Standard => STANDARD_V2_METRICS.as_slice(),
    };
    let mut speed_adjusted = None;
    let mut gust_adjusted = None;
    let mut direction_adjusted = None;
    let mut humidity_adjusted = None;
    let mut temperature_adjusted = None;
    let mut any_applied = false;
    let mut first_failure = None;

    // follow the selected Node list so the first failure remains source ordered
    for metric in metrics.iter().copied() {
        let Some(raw_value) = raw_metric(row.metrics, metric) else {
            continue;
        };
        let SelectedHierarchy {
            coefficient,
            root_available,
        } = select_hierarchy_coefficient(&candidate.coefficients, metric, lead_band, row.calendar);
        let result = apply_core_adjustment(&CoreAdjustmentApplicationInput {
            calendarFingerprintMatches: row.calendar_fingerprint_matches,
            coefficient,
            enabled: candidate
                .enabledMetricBands
                .iter()
                .any(|pair| pair.metric == metric && pair.leadBand == lead_band),
            envelope: training_envelope(&candidate.trainingEnvelopes, metric, lead_band),
            identityMatches: true,
            metric,
            rawForecastValue: raw_value,
            rawWindSpeedMps: row.metrics.windSpeedMps.clone(),
            rootAvailable: root_available,
        });

        // retain only the requested wind values while tracking any standard success
        if result.applied {
            any_applied = true;
            match metric {
                ForecastAdjustmentMetric::WindGustMps => {
                    gust_adjusted = Some(result.adjustedValue);
                }
                ForecastAdjustmentMetric::WindSpeedMps => {
                    speed_adjusted = Some(result.adjustedValue);
                }
                ForecastAdjustmentMetric::RelativeHumidityPercent => {
                    humidity_adjusted = Some(result.adjustedValue);
                }
                ForecastAdjustmentMetric::TemperatureC => {
                    temperature_adjusted = Some(result.adjustedValue);
                }
                ForecastAdjustmentMetric::WindDirectionDegrees => {
                    direction_adjusted = Some(result.adjustedValue);
                }
            }
        } else if first_failure.is_none() {
            first_failure = Some(result.reason);
        }
    }

    // match the Node all-null and all-failed reason disposition
    let reason = if any_applied {
        None
    } else {
        Some(match first_failure {
            Some(reason) => map_core_failure(reason),
            None => ForecastAdjustmentReasonCode::MetricNotEnabled,
        })
    };
    WindV2RowOutcome {
        humidity_adjusted,
        temperature_adjusted,
        direction_adjusted,
        speed_adjusted,
        gust_adjusted,
        lead_band: Some(lead_band),
        reason,
    }
}

/// Map one integer provider lead to the frozen inclusive application band.
fn lead_band_for(target_lead_hours: u64) -> Option<ForecastLeadBandKey> {
    // preserve the seven exact source ranges without a fallthrough band
    match target_lead_hours {
        1..=24 => Some(ForecastLeadBandKey::Value001024),
        25..=48 => Some(ForecastLeadBandKey::Value025048),
        49..=72 => Some(ForecastLeadBandKey::Value049072),
        73..=96 => Some(ForecastLeadBandKey::Value073096),
        97..=120 => Some(ForecastLeadBandKey::Value097120),
        121..=144 => Some(ForecastLeadBandKey::Value121144),
        145..=168 => Some(ForecastLeadBandKey::Value145168),
        _ => None,
    }
}

/// Read one allowlisted metric without allocating or inventing an absent value.
fn raw_metric(metrics: &CanonicalWeatherMetrics, metric: ForecastAdjustmentMetric) -> Option<f64> {
    let value = match metric {
        ForecastAdjustmentMetric::WindDirectionDegrees => &metrics.windDirectionDegrees,
        ForecastAdjustmentMetric::WindGustMps => &metrics.windGustMps,
        ForecastAdjustmentMetric::WindSpeedMps => &metrics.windSpeedMps,
        ForecastAdjustmentMetric::RelativeHumidityPercent => &metrics.relativeHumidityPercent,
        ForecastAdjustmentMetric::TemperatureC => &metrics.temperatureC,
    };
    // translate only the generated nullable storage boundary
    match value {
        Nullable::Value(value) => Some(*value),
        Nullable::Null => None,
    }
}

/// Select month then season then root using first-match artifact ordering.
pub(crate) fn select_hierarchy_coefficient(
    coefficients: &[ForecastAdjustmentCoefficient],
    metric: ForecastAdjustmentMetric,
    lead_band: ForecastLeadBandKey,
    calendar: AdmittedWindRowCalendar,
) -> SelectedHierarchy {
    let mut root = None;
    let mut season = None;
    let mut month = None;
    // collect only each depth's first matching cell in one table pass
    for coefficient in coefficients {
        // skip cells owned by another metric-band pair
        if coefficient.metric != metric || coefficient.leadBand != lead_band {
            continue;
        }
        // preserve source first-match behavior independently at every depth
        match coefficient.level {
            ForecastAdjustmentCoefficientLevelEnum::Value1 if root.is_none() => {
                root = Some(coefficient.coefficient);
            }
            ForecastAdjustmentCoefficientLevelEnum::Value2
                if season.is_none()
                    && coefficient.season == Nullable::Value(calendar.season)
                    && coefficient.daypart == Nullable::Value(calendar.daypart) =>
            {
                season = Some(coefficient.coefficient);
            }
            ForecastAdjustmentCoefficientLevelEnum::Value3
                if month.is_none()
                    && coefficient.month == Nullable::Value(calendar.month)
                    && coefficient.daypart == Nullable::Value(calendar.daypart) =>
            {
                month = Some(coefficient.coefficient);
            }
            _ => {}
        }
    }
    SelectedHierarchy {
        coefficient: match root {
            Some(root) => Nullable::Value(month.or(season).unwrap_or(root)),
            None => Nullable::Null,
        },
        root_available: root.is_some(),
    }
}

/// Project the generated scalar envelope expected by the pure core.
fn training_envelope(
    training_envelopes: &[ForecastAdjustmentTrainingEnvelope],
    metric: ForecastAdjustmentMetric,
    lead_band: ForecastLeadBandKey,
) -> Nullable<CoreAdjustmentApplicationInputEnvelope> {
    let envelope_metric = match metric {
        ForecastAdjustmentMetric::RelativeHumidityPercent => {
            Some(ForecastAdjustmentTrainingEnvelopeMetricEnum::RelativeHumidityPercent)
        }
        ForecastAdjustmentMetric::TemperatureC => {
            Some(ForecastAdjustmentTrainingEnvelopeMetricEnum::TemperatureC)
        }
        ForecastAdjustmentMetric::WindGustMps => {
            Some(ForecastAdjustmentTrainingEnvelopeMetricEnum::WindGustMps)
        }
        ForecastAdjustmentMetric::WindSpeedMps => {
            Some(ForecastAdjustmentTrainingEnvelopeMetricEnum::WindSpeedMps)
        }
        ForecastAdjustmentMetric::WindDirectionDegrees => None,
    };
    let Some(envelope_metric) = envelope_metric else {
        return Nullable::Null;
    };
    let envelope = training_envelopes
        .iter()
        .find(|envelope| envelope.metric == envelope_metric && envelope.leadBand == lead_band);
    // preserve the source's absent-envelope guard for every scalar metric
    match envelope {
        Some(envelope) => Nullable::Value(CoreAdjustmentApplicationInputEnvelope {
            maximum: envelope.maximum,
            minimum: envelope.minimum,
        }),
        None => Nullable::Null,
    }
}

/// Construct one numerical V2 refusal without fabricating adjusted values.
fn wind_v2_failure(reason: ForecastAdjustmentReasonCode) -> WindV2RowOutcome {
    WindV2RowOutcome {
        humidity_adjusted: None,
        temperature_adjusted: None,
        direction_adjusted: None,
        speed_adjusted: None,
        gust_adjusted: None,
        lead_band: None,
        reason: Some(reason),
    }
}

/// Convert one pure-core guard to the bounded public decision reason.
fn map_core_failure(reason: AdjustmentReason) -> ForecastAdjustmentReasonCode {
    // retain the exact source mapping including defensive adjusted handling
    match reason {
        AdjustmentReason::Adjusted => ForecastAdjustmentReasonCode::AdjustmentError,
        AdjustmentReason::CalendarFingerprintMismatch => {
            ForecastAdjustmentReasonCode::RuntimeFingerprintMismatch
        }
        AdjustmentReason::CoefficientMissing | AdjustmentReason::RootMissing => {
            ForecastAdjustmentReasonCode::CoefficientMissing
        }
        AdjustmentReason::DisabledMetricBand => ForecastAdjustmentReasonCode::MetricNotEnabled,
        AdjustmentReason::ForecastIdentityMismatch => {
            ForecastAdjustmentReasonCode::IdentityMismatch
        }
        AdjustmentReason::RawDirectionCalm => ForecastAdjustmentReasonCode::DirectionCalm,
        AdjustmentReason::RawValueInvalid => ForecastAdjustmentReasonCode::MetricOutOfBounds,
        AdjustmentReason::RawValueOod => ForecastAdjustmentReasonCode::TrainingEnvelopeMismatch,
    }
}

/// Construct the generated no-adjustment decision without serialization.
fn fail_raw(reason: ForecastAdjustmentReasonCode) -> ForecastAdjustmentDecision {
    ForecastAdjustmentDecision::FailRaw(ForecastAdjustmentFailRawDecision {
        adjustedMetrics: ForecastAdjustmentFailRawDecisionAdjustedMetricsRecord,
        appliedMetrics: ForecastAdjustmentFailRawDecisionAppliedMetricsEmptyArray,
        contractVersion: ForecastAdjustmentFailRawDecisionContractVersionLiteral,
        reasonCode: reason,
        state: ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable,
    })
}
