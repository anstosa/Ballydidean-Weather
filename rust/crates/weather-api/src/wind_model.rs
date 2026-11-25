use weather_adjustment::{
    AdmittedWindBundleRow, AdmittedWindRowCalendar, AdmittedWindRowFeatures,
    AdmittedWindRowSourceIdentity, evaluate_admitted_wind_bundle_row,
};
use weather_contracts::{
    CanonicalWeatherMetrics, ForecastAdjustmentRawForecastProvenance, ForecastAdjustmentReasonCode,
    ForecastAdjustmentWindCanaryCandidateV1, WeatherRecordRow,
};

use crate::{
    AdmittedWindCanaryEvidence, AdmittedWindCanaryRuntime, AdmittedWindRowOutput,
    CurrentProjectionError, ForecastWindAdjustmentBoundary,
};

/// One row after calendar, source, lead, and runtime-fingerprint admission.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedWindModelRow {
    pub calendar: AdmittedWindRowCalendar,
    pub calendar_fingerprint_matches: bool,
    pub raw_forecast_provenance: ForecastAdjustmentRawForecastProvenance,
    pub source_identity: AdmittedWindRowSourceIdentity,
}

/// one admitted or fail-raw repository row
#[derive(Debug, Clone, PartialEq)]
#[allow(
    clippy::large_enum_variant,
    reason = "avoid an additional allocation on each admitted forecast row"
)]
pub enum WindForecastRowAdmission {
    Admitted(AdmittedWindModelRow),
    Rejected(ForecastAdjustmentReasonCode),
}

/// Typed boundary for time, identity, and runtime-fingerprint resolution.
pub trait ForecastWindModelAdmissionBoundary {
    /// Resolve the row material that cannot be derived without admitted adapters.
    fn admit(
        &self,
        candidate: &ForecastAdjustmentWindCanaryCandidateV1,
        row: &WeatherRecordRow,
    ) -> Result<WindForecastRowAdmission, CurrentProjectionError>;
}

/// Validated startup wind model and its admitted row-material boundary.
pub struct AdmittedWindModelOwner<'a> {
    pub candidate: &'a ForecastAdjustmentWindCanaryCandidateV1,
    pub evidence: &'a AdmittedWindCanaryEvidence,
    pub admission: &'a dyn ForecastWindModelAdmissionBoundary,
}

impl ForecastWindAdjustmentBoundary for AdmittedWindModelOwner<'_> {
    /// Evaluate one row with the exact admitted candidate and pure model adapter.
    fn apply(
        &self,
        runtime: &AdmittedWindCanaryRuntime,
        row: &WeatherRecordRow,
    ) -> Result<AdmittedWindRowOutput, CurrentProjectionError> {
        // reject any drift between the startup runtime and admitted model owner
        if &runtime.evidence != self.evidence
            || self.candidate.candidateArtifactSha256 != self.evidence.candidate_artifact_sha256
            || runtime.enabled_metric_bands != self.candidate.enabledMetricBands
        {
            return Err(CurrentProjectionError {
                code: "forecast_projection.wind_model",
                detail: "wind model startup evidence does not match the active runtime".to_owned(),
            });
        }

        let admitted = match self.admission.admit(self.candidate, row)? {
            WindForecastRowAdmission::Admitted(admitted) => admitted,
            WindForecastRowAdmission::Rejected(reason) => {
                return Ok(AdmittedWindRowOutput::FailRaw(reason));
            }
        };
        let metrics = canonical_weather_metrics(row);
        let decision = evaluate_admitted_wind_bundle_row(
            AdmittedWindBundleRow {
                authorization_sha256: &self.evidence.authorization_sha256,
                candidate: self.candidate,
                transfer_report_sha256: &self.evidence.transfer_report_sha256,
            },
            AdmittedWindRowFeatures {
                calendar: admitted.calendar,
                calendar_fingerprint_matches: admitted.calendar_fingerprint_matches,
                metrics: &metrics,
                raw_forecast_provenance: &admitted.raw_forecast_provenance,
                source_identity: admitted.source_identity,
            },
        );

        Ok(AdmittedWindRowOutput::Decision(Box::new(decision)))
    }
}

/// Project one repository row into the generated canonical model metric shape.
pub(crate) fn canonical_weather_metrics(row: &WeatherRecordRow) -> CanonicalWeatherMetrics {
    CanonicalWeatherMetrics {
        apparentTemperatureC: row.apparentTemperatureC.clone(),
        blackGlobeTemperatureC: row.blackGlobeTemperatureC.clone(),
        cloudCoverPercent: row.cloudCoverPercent.clone(),
        pm25MicrogramsPerCubicMeter: row.pm25MicrogramsPerCubicMeter.clone(),
        precipitationMm: row.precipitationMm.clone(),
        precipitationRateMmPerHour: row.precipitationRateMmPerHour.clone(),
        pressureHpa: row.pressureHpa.clone(),
        relativeHumidityPercent: row.relativeHumidityPercent.clone(),
        soilElectricalConductivityMicrosiemensPerCm: row
            .soilElectricalConductivityMicrosiemensPerCm
            .clone(),
        soilMoisturePercent: row.soilMoisturePercent.clone(),
        solarRadiationWm2: row.solarRadiationWm2.clone(),
        temperatureC: row.temperatureC.clone(),
        uvIndex: row.uvIndex.clone(),
        waterLevelM: row.waterLevelM.clone(),
        windDirectionDegrees: row.windDirectionDegrees.clone(),
        windGustMps: row.windGustMps.clone(),
        windSpeedMps: row.windSpeedMps.clone(),
        wetBulbGlobeTemperatureC: row.wetBulbGlobeTemperatureC.clone(),
    }
}
