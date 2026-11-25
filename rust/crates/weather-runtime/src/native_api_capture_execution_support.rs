//! Move one admitted private capture into an owned background job without cloning model artifacts.

use super::*;

use weather_api::{LegacyTemperatureForecastStartup, MaintenanceTemperatureForecastStartup};

use crate::{
    NativeApiCaptureCodec, NativeApiCapturePredictionOwner, NativeApiModelAdmission,
    NativePostgresPool, PreparedPrivateMaintenanceApi,
};

/// Owned request data whose immutable registration is reborrowed from the same startup snapshot.
pub struct NativeApiCaptureDetachedWork {
    request: AdmittedAdjustmentMaintenanceCapture,
    registration_sha256: String,
    family: NativeApiCaptureDetachedFamily,
}

enum NativeApiCaptureDetachedFamily {
    Temperature {
        best_match: Vec<WeatherRecordRow>,
        sidecar: Box<EcmwfTemperatureCanarySidecar>,
    },
    Wind {
        rows: Vec<WeatherRecordRow>,
    },
}

impl ApiMaintenanceCaptureJob<'_> {
    /// Move the already selected row set and registration identity into independently owned work.
    pub fn into_detached_native_work(self) -> NativeApiCaptureDetachedWork {
        let (registration_sha256, family) = match self.family {
            CaptureFamily::Temperature {
                installed,
                best_match,
                sidecar,
            } => (
                installed.registration.registration_sha256.to_owned(),
                NativeApiCaptureDetachedFamily::Temperature {
                    best_match,
                    sidecar,
                },
            ),
            CaptureFamily::Wind { installed, rows } => (
                installed.registration.registration_sha256.to_owned(),
                NativeApiCaptureDetachedFamily::Wind { rows },
            ),
        };
        NativeApiCaptureDetachedWork {
            request: self.request,
            registration_sha256,
            family,
        }
    }
}

/// Run the detached job over the exact immutable candidates, witness, and model snapshot.
pub fn run_detached_native_work(
    work: NativeApiCaptureDetachedWork,
    application: &PreparedPrivateMaintenanceApi<NativePostgresPool>,
    model: &NativeApiModelAdmission,
    prediction_owner: &NativeApiCapturePredictionOwner,
) -> Result<CaptureApiMaintenanceShadowResult, OperationError> {
    let (preparation, snapshots, candidates) = application.capture_material();
    let NativeApiCaptureDetachedWork {
        request,
        registration_sha256,
        family,
    } = work;
    let mut codec = NativeApiCaptureCodec;
    match family {
        NativeApiCaptureDetachedFamily::Wind { rows } => {
            let installed = candidates
                .wind_capture(snapshots, model)
                .ok_or(OperationError::Rejected)?;
            // rebind only the registration selected before the independent task was started
            if installed.registration.registration_sha256 != registration_sha256 {
                return Err(OperationError::Rejected);
            }
            prediction_owner.run(
                ApiMaintenanceCaptureJob {
                    request,
                    witness: &preparation.capture_witness,
                    time: model,
                    family: CaptureFamily::Wind { installed, rows },
                },
                &mut codec,
            )
        }
        NativeApiCaptureDetachedFamily::Temperature {
            best_match,
            sidecar,
        } => {
            let snapshot = &snapshots.temperature_adjustment;
            let legacy = LegacyTemperatureForecastStartup::from_snapshot(snapshot, model)
                .map_err(|_| OperationError::Rejected)?;
            // choose the same installed grammar as the public startup without a second clock sample
            let maintenance = if legacy.is_none() {
                MaintenanceTemperatureForecastStartup::from_snapshot(snapshot, model)
                    .map_err(|_| OperationError::Rejected)?
            } else {
                None
            };
            let (active_runtime, active_owner) = if let Some(startup) = &legacy {
                (
                    Some(startup.runtime()),
                    Some(
                        startup
                            .model_owner(snapshot, model, model)
                            .ok_or(OperationError::Rejected)?,
                    ),
                )
            } else if let Some(startup) = &maintenance {
                (
                    Some(startup.runtime()),
                    Some(
                        startup
                            .model_owner(snapshot, model, model)
                            .ok_or(OperationError::Rejected)?,
                    ),
                )
            } else {
                (None, None)
            };
            let active = active_runtime.zip(active_owner.as_ref());
            let installed = candidates
                .temperature_capture(snapshots, active, model)
                .ok_or(OperationError::Rejected)?;
            // never let an independently running capture silently switch candidate registrations
            if installed.registration.registration_sha256 != registration_sha256 {
                return Err(OperationError::Rejected);
            }
            prediction_owner.run(
                ApiMaintenanceCaptureJob {
                    request,
                    witness: &preparation.capture_witness,
                    time: model,
                    family: CaptureFamily::Temperature {
                        installed,
                        best_match,
                        sidecar,
                    },
                },
                &mut codec,
            )
        }
    }
}
