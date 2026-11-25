//! Replay an already admitted active model through existing Rust numerical owners.

use weather_contracts::{
    ForecastAdjustmentCandidateV2, ForecastAdjustmentDecision, ForecastAdjustmentWindCanaryMetric,
};
use weather_worker::wind_performance_numerics::{
    AdmittedWindBundleRow, AdmittedWindV2MetricOrder, evaluate_admitted_wind_bundle_row,
    evaluate_admitted_wind_v2_row,
};

pub use weather_worker::wind_performance_numerics::AdmittedWindRowFeatures;

use crate::WindPerformanceReplayDecision;

/// Frozen model material after startup custody/authorization and row effect admission.
pub enum AdmittedWindPerformanceModel<'a> {
    Legacy(AdmittedWindBundleRow<'a>),
    Maintenance(&'a ForecastAdjustmentCandidateV2),
}

/// Replay both original wind model grammars without an alternate numerical path.
pub fn replay_admitted_wind_performance_model(
    model: AdmittedWindPerformanceModel<'_>,
    row: AdmittedWindRowFeatures<'_>,
) -> Option<WindPerformanceReplayDecision> {
    // keep only successful original model decisions for retrospective band selection
    match model {
        AdmittedWindPerformanceModel::Legacy(bundle) => {
            let ForecastAdjustmentDecision::WindCanaryActive(decision) =
                evaluate_admitted_wind_bundle_row(bundle, row)
            else {
                return None;
            };
            let adjusted = |metric| {
                decision
                    .adjustedMetrics
                    .0
                    .iter()
                    .find_map(|(key, value)| (*key == metric).then_some(*value))
            };
            Some(WindPerformanceReplayDecision {
                lead_band: decision.leadBand,
                adjusted_wind_speed_mps: adjusted(ForecastAdjustmentWindCanaryMetric::WindSpeedMps),
                adjusted_wind_gust_mps: adjusted(ForecastAdjustmentWindCanaryMetric::WindGustMps),
            })
        }
        AdmittedWindPerformanceModel::Maintenance(candidate) => {
            let outcome = evaluate_admitted_wind_v2_row(
                candidate,
                row,
                AdmittedWindV2MetricOrder::Maintenance,
            );
            // all-failed and unsupported rows have no active lead band to score
            if outcome.reason.is_some() {
                return None;
            }
            Some(WindPerformanceReplayDecision {
                lead_band: weather_domain::forecast_lead_band_for(
                    row.raw_forecast_provenance.targetLeadHours as f64,
                )
                .ok()?,
                adjusted_wind_speed_mps: outcome.speed_adjusted,
                adjusted_wind_gust_mps: outcome.gust_adjusted,
            })
        }
    }
}
