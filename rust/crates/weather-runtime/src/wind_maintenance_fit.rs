//! Run the native pre-holdout fit over the verified journal and calendar projection.

use weather_contracts::{
    ForecastAdjustmentCandidateV2RuntimeFingerprint, ForecastAdjustmentForecastIdentity,
    ForecastAdjustmentTrainingProvenance, ForecastObservationStation,
};
use weather_worker::wind_maintenance_development::{
    AdmittedWindDevelopmentDate, AdmittedWindTrainingRow, WindDevelopmentRunError,
    WindMaintenanceTrainingPreparation, WindTrainingEventError, prepare_wind_maintenance_training,
    qualify_wind_maintenance_development,
};

use crate::maintenance_canonical_json::valid_hash;
use crate::wind_maintenance_candidate::prepare_wind_insufficiency;
use crate::{
    AdmittedWindMaintenanceDate, AdmittedWindMaintenanceEpoch, AdmittedWindMaintenanceInstant,
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    PreparedWindMaintenanceCandidate, ReadyWindMaintenancePreholdout,
    WindMaintenanceCandidateError, WindMaintenanceCandidateIdentity, WindMaintenanceCutoffError,
    WindMaintenanceCutoffInput, WindMaintenanceCutoffOutcome, admit_wind_maintenance_cutoff,
    prepare_wind_maintenance_candidate, wind_maintenance_auxiliary_model_sha256,
};

const FINAL_TRAINING_DAYS: usize = 365;
const QUALIFICATION_DAYS: usize = 402;

/// The existing site calendar resolves midnight with pinned Los Angeles rules.
pub trait WindMaintenanceLocalCalendar {
    /// Resolve midnight for the admitted day or the following local day, never elapsed hours.
    fn wind_local_midnight_milliseconds(&self, date: &str, next_day: bool) -> Option<i64>;
}

/// Original served lineage and calendar identity admitted from the verified input.
#[derive(Clone, Copy, Debug)]
pub struct WindMaintenanceFitProvenance<'a> {
    pub forecast_identity: &'a ForecastAdjustmentForecastIdentity,
    pub runtime_fingerprint: &'a ForecastAdjustmentCandidateV2RuntimeFingerprint,
    pub training_provenance: &'a ForecastAdjustmentTrainingProvenance,
}

/// Ordinary failures distinguish malformed admission input from numerical and serialization failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindMaintenanceFitError {
    Identity,
    Calendar,
    Cutoff(WindMaintenanceCutoffError),
    Training(WindTrainingEventError),
    Development(WindDevelopmentRunError<WindMaintenanceCandidateError>),
    Artifact(WindMaintenanceCandidateError),
}

/// Reuse the approved exact UTC clock for receipt and cutoff comparison.
pub fn admit_wind_maintenance_instant<'a>(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    utc: &'a str,
) -> Option<AdmittedWindMaintenanceInstant<'a>> {
    let millis = values.canonical_utc_epoch_milliseconds(utc)?;
    Some(AdmittedWindMaintenanceInstant::from_canonical(utc, millis))
}

/// Fit the earlier training phase with the existing canonical-value owner.
///
/// Manifest shape, digest, calendar projection and underlying row identities are admitted
/// by the caller. This operation owns neither file access nor a confirmation/publish capability.
pub fn fit_admitted_wind_maintenance(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    local_calendar: &impl WindMaintenanceLocalCalendar,
    input: WindMaintenanceCutoffInput<'_, AdmittedWindTrainingRow<'_>>,
    provenance: WindMaintenanceFitProvenance<'_>,
    stations: &[ForecastObservationStation],
) -> Result<PreparedWindMaintenanceCandidate, WindMaintenanceFitError> {
    // preserve original month and lowercase digest admission before using the epoch
    if !valid_wind_due_month(input.due_month) || !valid_hash(input.manifest.identity.as_str()) {
        return Err(WindMaintenanceFitError::Identity);
    }
    // bind all supplied clocks to the exact local-midnight geometry before receipt comparison
    if !exact_training_geometry(local_calendar, input.due_month, input.cutoff, input.epoch) {
        return Err(WindMaintenanceFitError::Calendar);
    }
    let calendar = input.epoch.local_dates;
    let outcome = admit_wind_maintenance_cutoff(input).map_err(WindMaintenanceFitError::Cutoff)?;
    let phase = match outcome {
        WindMaintenanceCutoffOutcome::Ready(phase) => phase,
        WindMaintenanceCutoffOutcome::Refused(reason) => {
            return prepare_wind_insufficiency(
                values,
                reason.snapshot_manifest_sha256,
                vec![reason.failed_gate().to_owned()],
            )
            .map_err(WindMaintenanceFitError::Artifact);
        }
    };
    finish_wind_maintenance_phase(
        values,
        calendar,
        phase,
        Some(provenance.forecast_identity),
        provenance.runtime_fingerprint,
        provenance.training_provenance,
        stations,
    )
}

/// Finish the selected phase without handing future rows to any numerical operation.
pub(crate) fn finish_wind_maintenance_phase(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    calendar: &[AdmittedWindMaintenanceDate<'_>],
    phase: ReadyWindMaintenancePreholdout<'_, AdmittedWindTrainingRow<'_>>,
    forecast_identity: Option<&ForecastAdjustmentForecastIdentity>,
    runtime_fingerprint: &ForecastAdjustmentCandidateV2RuntimeFingerprint,
    training_provenance: &ForecastAdjustmentTrainingProvenance,
    stations: &[ForecastObservationStation],
) -> Result<PreparedWindMaintenanceCandidate, WindMaintenanceFitError> {
    let training = prepare_wind_maintenance_training(
        &phase.training_rows,
        stations,
        &phase.expected_training_dates,
    )
    .map_err(WindMaintenanceFitError::Training)?;
    // release the phase row vector before constructing the fold working sets
    drop(phase.training_rows);
    let ready = match training {
        WindMaintenanceTrainingPreparation::Ready(ready) => ready,
        WindMaintenanceTrainingPreparation::Insufficient { failed_gates } => {
            return prepare_wind_insufficiency(
                values,
                phase.snapshot_manifest_sha256,
                failed_gates,
            )
            .map_err(WindMaintenanceFitError::Artifact);
        }
    };
    let calendar =
        std::array::from_fn::<_, FINAL_TRAINING_DAYS, _>(|index| AdmittedWindDevelopmentDate {
            label: calendar[index].label,
            ordinal: calendar[index].ordinal,
        });
    let qualification =
        qualify_wind_maintenance_development(&ready, stations, &calendar, |model| {
            wind_maintenance_auxiliary_model_sha256(values, model)
                .map(|sha| sha.as_str().to_owned())
        })
        .map_err(WindMaintenanceFitError::Development)?;
    // build no candidate if the original development qualification could not promote a pair
    if matches!(
        qualification,
        weather_worker::wind_maintenance_development::WindDevelopmentQualification::NoQualifiedPair
    ) {
        return prepare_wind_insufficiency(
            values,
            phase.snapshot_manifest_sha256,
            vec!["development_loso_qualification".to_owned()],
        )
        .map_err(WindMaintenanceFitError::Artifact);
    }
    prepare_wind_maintenance_candidate(
        values,
        &WindMaintenanceCandidateIdentity {
            due_month: phase.due_month,
            d0: phase.d0,
            d401: phase.d401,
            snapshot_manifest_sha256: phase.snapshot_manifest_sha256,
            final_training_cutoff: phase.final_training_cutoff.utc(),
            forecast_identity: forecast_identity.ok_or(WindMaintenanceFitError::Identity)?,
            runtime_fingerprint,
            training_provenance,
        },
        stations,
        qualification,
    )
    .map_err(WindMaintenanceFitError::Artifact)
}

/// Check the original due-month wire shape without interpreting the calendar.
pub(crate) fn valid_wind_due_month(value: &str) -> bool {
    matches!(
        value.as_bytes(),
        [
            b'0'..=b'9',
            b'0'..=b'9',
            b'0'..=b'9',
            b'0'..=b'9',
            b'-',
            b'0',
            b'1'..=b'9'
        ] | [
            b'0'..=b'9',
            b'0'..=b'9',
            b'0'..=b'9',
            b'0'..=b'9',
            b'-',
            b'1',
            b'0'..=b'2'
        ]
    )
}

/// Recompute the three literal boundary instants with the approved site calendar.
fn exact_training_geometry(
    calendar: &impl WindMaintenanceLocalCalendar,
    due_month: &str,
    cutoff: AdmittedWindMaintenanceInstant<'_>,
    epoch: AdmittedWindMaintenanceEpoch<'_>,
) -> bool {
    // require the complete epoch before resolving D364 and D401
    if epoch.local_dates.len() != QUALIFICATION_DAYS {
        return false;
    }
    let first_due_date = format!("{due_month}-01");
    calendar.wind_local_midnight_milliseconds(&first_due_date, false)
        == Some(cutoff.epoch_milliseconds())
        && calendar
            .wind_local_midnight_milliseconds(epoch.local_dates[QUALIFICATION_DAYS - 1].label, true)
            == Some(epoch.end_exclusive.epoch_milliseconds())
        && calendar
            .wind_local_midnight_milliseconds(
                epoch.local_dates[FINAL_TRAINING_DAYS - 1].label,
                true,
            )
            .and_then(|cutoff| cutoff.checked_sub(1))
            == Some(epoch.final_training_cutoff.epoch_milliseconds())
}

#[cfg(test)]
mod tests {
    use weather_contracts::ForecastAdjustmentCoefficientSeasonEnum;

    use super::*;
    use crate::AdmittedWindMaintenanceDate;

    struct FixtureCalendar;

    impl WindMaintenanceLocalCalendar for FixtureCalendar {
        /// Admit only the three source labels needed to bind the fit clocks.
        fn wind_local_midnight_milliseconds(&self, date: &str, next_day: bool) -> Option<i64> {
            match (date, next_day) {
                ("2026-10-01", false) => Some(3_000),
                ("2026-07-10", true) => Some(2_000),
                ("2026-06-03", true) => Some(1_000),
                _ => None,
            }
        }
    }

    /// Require the exact due midnight and D364 maximum, not only their ordering.
    #[test]
    fn binds_the_three_calendar_owned_fit_clocks() {
        let mut dates = [AdmittedWindMaintenanceDate {
            label: "unused",
            ordinal: 0,
            season: ForecastAdjustmentCoefficientSeasonEnum::Summer,
        }; QUALIFICATION_DAYS];
        dates[364].label = "2026-06-03";
        dates[401].label = "2026-07-10";
        let due = AdmittedWindMaintenanceInstant::from_canonical("due", 3_000);
        let epoch = AdmittedWindMaintenanceEpoch {
            local_dates: &dates,
            end_exclusive: AdmittedWindMaintenanceInstant::from_canonical("end", 2_000),
            final_training_cutoff: AdmittedWindMaintenanceInstant::from_canonical("last", 999),
        };
        assert!(exact_training_geometry(
            &FixtureCalendar,
            "2026-10",
            due,
            epoch
        ));
        assert!(!exact_training_geometry(
            &FixtureCalendar,
            "2026-10",
            AdmittedWindMaintenanceInstant::from_canonical("due", 3_001),
            epoch,
        ));
        assert!(!exact_training_geometry(
            &FixtureCalendar,
            "2026-10",
            due,
            AdmittedWindMaintenanceEpoch {
                final_training_cutoff: AdmittedWindMaintenanceInstant::from_canonical("last", 998),
                ..epoch
            },
        ));
    }
}
