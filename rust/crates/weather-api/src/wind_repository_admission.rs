//! Admit public forecast repository rows for the installed wind model.

use std::sync::Arc;

use weather_adjustment::AdmittedWindRowSourceIdentity;
use weather_contracts::{
    ForecastAdjustmentCandidateV2, ForecastAdjustmentForecastIdentity,
    ForecastAdjustmentRawForecastProvenance, ForecastAdjustmentRawForecastProvenanceCohortLiteral,
    ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral, ForecastAdjustmentReasonCode,
    ForecastAdjustmentWindCanaryCandidateV1, JsonValue, Nullable, WeatherRecordRow,
};

use crate::{
    AdmittedWindModelRow, CurrentProjectionError, CurrentTimeResolver,
    ForecastMaintenanceWindModelAdmissionBoundary, ForecastWindModelAdmissionBoundary,
    MaintenanceWindModelCalendar, WindForecastRowAdmission, current_projection::object_value,
};

const HOUR_MILLISECONDS: i64 = 3_600_000;

/// production row adapter shared by both wind models
pub struct RepositoryWindModelAdmission {
    time: Arc<dyn CurrentTimeResolver + Send + Sync>,
    calendar: Arc<dyn MaintenanceWindModelCalendar + Send + Sync>,
}

impl RepositoryWindModelAdmission {
    /// retain process time and calendar owners
    pub fn new(
        time: Arc<dyn CurrentTimeResolver + Send + Sync>,
        calendar: Arc<dyn MaintenanceWindModelCalendar + Send + Sync>,
    ) -> Self {
        Self { time, calendar }
    }

    /// derive exact public-row provenance
    fn admit_row(
        &self,
        expected: &ForecastAdjustmentForecastIdentity,
        icu_version: &str,
        tzdata_version: &str,
        row: &WeatherRecordRow,
    ) -> Result<WindForecastRowAdmission, CurrentProjectionError> {
        let reference_at = match &row.productRunAt {
            Nullable::Null => None,
            Nullable::Value(value) => Some(self.time.resolve(value)?),
        };
        let valid_at = self.time.resolve(&row.validAt)?;
        let Some(reference_at) = reference_at else {
            return Ok(WindForecastRowAdmission::Rejected(
                ForecastAdjustmentReasonCode::UnsupportedLead,
            ));
        };
        let Some(target_lead_hours) =
            target_lead_hours(reference_at.epoch_milliseconds, valid_at.epoch_milliseconds)
        else {
            return Ok(WindForecastRowAdmission::Rejected(
                ForecastAdjustmentReasonCode::UnsupportedLead,
            ));
        };
        // refuse identity before calendar work
        if !row_identity_matches(expected, row) {
            return Ok(WindForecastRowAdmission::Rejected(
                ForecastAdjustmentReasonCode::IdentityMismatch,
            ));
        }
        let provenance =
            repository_provenance(expected, reference_at.utc, valid_at.utc, target_lead_hours);
        let calendar = self.calendar.calendar(&provenance.validAt)?;
        let calendar_fingerprint_matches = self
            .calendar
            .fingerprint_matches(icu_version, tzdata_version);
        Ok(WindForecastRowAdmission::Admitted(AdmittedWindModelRow {
            calendar,
            calendar_fingerprint_matches,
            raw_forecast_provenance: provenance,
            source_identity: AdmittedWindRowSourceIdentity::Match,
        }))
    }
}

impl ForecastWindModelAdmissionBoundary for RepositoryWindModelAdmission {
    /// admit one legacy public row
    fn admit(
        &self,
        candidate: &ForecastAdjustmentWindCanaryCandidateV1,
        row: &WeatherRecordRow,
    ) -> Result<WindForecastRowAdmission, CurrentProjectionError> {
        self.admit_row(
            &candidate.servedForecastIdentity,
            &candidate.runtimeFingerprint.icuVersion,
            &candidate.runtimeFingerprint.tzdataVersion,
            row,
        )
    }
}

impl ForecastMaintenanceWindModelAdmissionBoundary for RepositoryWindModelAdmission {
    /// admit one qualified public row
    fn admit(
        &self,
        candidate: &ForecastAdjustmentCandidateV2,
        row: &WeatherRecordRow,
    ) -> Result<WindForecastRowAdmission, CurrentProjectionError> {
        self.admit_row(
            &candidate.forecastIdentity,
            &candidate.runtimeFingerprint.icuVersion,
            &candidate.runtimeFingerprint.tzdataVersion,
            row,
        )
    }
}

/// compute supported JavaScript ceiling lead hours
fn target_lead_hours(reference_epoch_ms: i64, valid_epoch_ms: i64) -> Option<u64> {
    let delta = valid_epoch_ms.checked_sub(reference_epoch_ms)?;
    // reject nonpositive geometry early
    if delta <= 0 {
        return None;
    }
    let complete_hours = delta / HOUR_MILLISECONDS;
    let partial_hour = i64::from(delta % HOUR_MILLISECONDS != 0);
    let lead = complete_hours.checked_add(partial_hour)?;
    // preserve the frozen lead range
    if !(1..=168).contains(&lead) {
        return None;
    }
    u64::try_from(lead).ok()
}

/// build generated canonical provenance
fn repository_provenance(
    expected: &ForecastAdjustmentForecastIdentity,
    reference_at: String,
    valid_at: String,
    target_lead_hours: u64,
) -> ForecastAdjustmentRawForecastProvenance {
    ForecastAdjustmentRawForecastProvenance {
        adapterVersion: expected.adapterVersion.clone(),
        cohort: ForecastAdjustmentRawForecastProvenanceCohortLiteral,
        contractEpoch: expected.contractEpoch.clone(),
        dataset: expected.dataset.clone(),
        referenceAt: reference_at,
        referenceKind: ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral,
        sourceConfigFingerprint: expected.sourceConfigFingerprint.clone(),
        sourceKey: expected.sourceKey.clone(),
        targetLeadHours: target_lead_hours,
        upstreamModel: expected.upstreamModel.clone(),
        validAt: valid_at,
    }
}

/// match source identity without allocation
fn row_identity_matches(
    expected: &ForecastAdjustmentForecastIdentity,
    row: &WeatherRecordRow,
) -> bool {
    expected.adapterVersion == nullable_text(&row.adapterVersion)
        && expected.contractEpoch == nullable_text(&row.contractEpoch)
        && provider_dataset_matches(&row.providerMetadata, &expected.dataset)
        && expected.sourceConfigFingerprint == nullable_text(&row.sourceConfigFingerprint)
        && expected.sourceKey == row.sourceKey
        && expected.upstreamModel == nullable_text(&row.upstreamModel)
}

/// borrow the Node nullable fallback
fn nullable_text(value: &Nullable<String>) -> &str {
    match value {
        Nullable::Value(value) => value,
        Nullable::Null => "",
    }
}

/// compare the exact UTF-16 dataset
fn provider_dataset_matches(
    metadata: &Nullable<Vec<(Vec<u16>, JsonValue)>>,
    expected: &str,
) -> bool {
    let Nullable::Value(metadata) = metadata else {
        return expected.is_empty();
    };
    let Some(JsonValue::String(dataset)) = object_value(metadata, "dataset") else {
        return expected.is_empty();
    };
    dataset.iter().copied().eq(expected.encode_utf16())
}
