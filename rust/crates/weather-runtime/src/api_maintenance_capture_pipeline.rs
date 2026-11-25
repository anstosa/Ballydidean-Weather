//! Connect retained private forecast reads to the Rust replay and publication pipeline.

use weather_api::{
    AdjustmentMaintenanceGapCallback, AdjustmentMaintenanceOperationError,
    AdmittedAdjustmentMaintenanceCapture, AdmittedAdjustmentMaintenanceGap,
    AdmittedAdjustmentRevisionCaptureWitness, AdmittedTemperatureCanaryRuntime,
    AdmittedTemperatureMosModelOwner, BorrowedTemperatureMosServingModel, CurrentTimeResolver,
    ForecastTemperatureMosAdmissionBoundary, MaintenanceComparatorCanonicalCodec,
    MaintenancePredictionCanonicalCodec, MaintenanceSourceCanonicalCodec,
    MaintenanceSourceProjectionHeader, MaintenanceWindModelCalendar,
    build_temperature_maintenance_source_projection, build_wind_maintenance_source_projection,
    prepare_maintenance_source,
};
use weather_contracts::{
    AdjustmentMaintenanceGapInput, AdjustmentShadowFamily, CaptureApiMaintenanceShadowResult,
    EcmwfTemperatureCanarySidecar, ForecastAdjustmentTemperatureMaintenanceRuntimePackage,
    ForecastAdjustmentWindMaintenanceRuntimePackage, LoadedApiForecastAdjustmentRuntime,
    LoadedForecastAdjustmentTemperatureCanaryRuntime, MaintenanceShadowGapReason, WeatherRecordRow,
};
use weather_database::AdmittedApiMaintenanceRegistration;

use crate::api_maintenance_prediction::report_api_maintenance_capture_gap;
use crate::{
    ApiMaintenanceCaptureOperations, ApiMaintenancePredictionInput, ApiMaintenancePredictionPorts,
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceReplay, run_api_maintenance_prediction,
};

type OperationError = AdjustmentMaintenanceOperationError;
type GapReason = MaintenanceShadowGapReason;

/// Installer-admitted temperature package and the same incumbent used by public startup.
#[derive(Clone, Copy)]
pub struct InstalledApiTemperatureCapture<'a> {
    pub registration: AdmittedApiMaintenanceRegistration<'a>,
    pub(crate) has_capture_receipt_v1: bool,
    pub package: &'a ForecastAdjustmentTemperatureMaintenanceRuntimePackage,
    pub incumbent: &'a LoadedForecastAdjustmentTemperatureCanaryRuntime,
    pub active_incumbent: Option<(
        &'a AdmittedTemperatureCanaryRuntime,
        &'a AdmittedTemperatureMosModelOwner<'a, BorrowedTemperatureMosServingModel<'a>>,
    )>,
    pub calendar: &'a dyn ForecastTemperatureMosAdmissionBoundary,
}

/// Installer-admitted wind package and the same incumbent used by public startup.
#[derive(Clone, Copy)]
pub struct InstalledApiWindCapture<'a> {
    pub registration: AdmittedApiMaintenanceRegistration<'a>,
    pub(crate) has_capture_receipt_v1: bool,
    pub package: &'a ForecastAdjustmentWindMaintenanceRuntimePackage,
    pub incumbent: &'a LoadedApiForecastAdjustmentRuntime,
    pub calendar: &'a dyn MaintenanceWindModelCalendar,
}

/// Canonical codec operations shared by one scheduled private capture.
pub trait ApiMaintenanceCaptureCodec:
    MaintenanceSourceCanonicalCodec
    + MaintenancePredictionCanonicalCodec
    + MaintenanceComparatorCanonicalCodec
{
}

impl<T> ApiMaintenanceCaptureCodec for T where
    T: MaintenanceSourceCanonicalCodec
        + MaintenancePredictionCanonicalCodec
        + MaintenanceComparatorCanonicalCodec
{
}

enum CaptureFamily<'a> {
    Temperature {
        installed: InstalledApiTemperatureCapture<'a>,
        best_match: Vec<WeatherRecordRow>,
        sidecar: Box<EcmwfTemperatureCanarySidecar>,
    },
    Wind {
        installed: InstalledApiWindCapture<'a>,
        rows: Vec<WeatherRecordRow>,
    },
}

/// Scheduled work keeps immutable startup owners but takes ownership of the original database rows.
pub struct ApiMaintenanceCaptureJob<'a> {
    request: AdmittedAdjustmentMaintenanceCapture,
    witness: &'a AdmittedAdjustmentRevisionCaptureWitness,
    time: &'a dyn CurrentTimeResolver,
    family: CaptureFamily<'a>,
}

impl ApiMaintenanceCaptureJob<'_> {
    /// Build the checked source and run the original candidate and incumbent before publication.
    pub fn run(
        self,
        codec: &mut impl ApiMaintenanceCaptureCodec,
        ports: &mut impl ApiMaintenancePredictionPorts,
        receipt_values: &impl ApiMaintenanceReceiptCanonicalValues,
    ) -> Result<CaptureApiMaintenanceShadowResult, OperationError> {
        let due_key = &self.request.request.dueKey;
        let (registration, projection, replay) = match &self.family {
            CaptureFamily::Temperature {
                installed,
                best_match,
                sidecar,
            } => {
                let registration = &installed.registration;
                require_registration_family(registration, AdjustmentShadowFamily::Temperature)?;
                // classify an unavailable installed bundle before looking at the source
                if !installed.has_capture_receipt_v1
                    || installed.package.candidateSha256 != registration.candidate_sha256
                {
                    return report_api_maintenance_capture_gap(
                        registration,
                        due_key,
                        ports,
                        GapReason::CandidateUnavailable,
                    );
                }
                let projection = build_temperature_maintenance_source_projection(
                    self.source_header(registration),
                    best_match,
                    sidecar,
                    self.time,
                );
                (
                    registration,
                    projection,
                    ApiMaintenanceReplay::Temperature {
                        package: installed.package,
                        incumbent: installed.incumbent,
                        active_incumbent: installed.active_incumbent,
                        calendar: installed.calendar,
                        time: self.time,
                    },
                )
            }
            CaptureFamily::Wind { installed, rows } => {
                let registration = &installed.registration;
                require_registration_family(registration, AdjustmentShadowFamily::Wind)?;
                // classify an unavailable installed bundle before looking at the source
                if !installed.has_capture_receipt_v1
                    || installed.package.candidateSha256 != registration.candidate_sha256
                {
                    return report_api_maintenance_capture_gap(
                        registration,
                        due_key,
                        ports,
                        GapReason::CandidateUnavailable,
                    );
                }
                let projection = build_wind_maintenance_source_projection(
                    self.source_header(registration),
                    rows,
                    self.time,
                );
                (
                    registration,
                    projection,
                    ApiMaintenanceReplay::Wind {
                        package: installed.package,
                        incumbent: installed.incumbent,
                        calendar: installed.calendar,
                        time: self.time,
                    },
                )
            }
        };
        let prepared = projection.and_then(|projection| {
            prepare_maintenance_source(projection, &self.witness.capture_epoch, self.time, codec)
        });
        let source = match prepared {
            Ok(source) => source,
            Err(_) => {
                return report_api_maintenance_capture_gap(
                    registration,
                    due_key,
                    ports,
                    GapReason::SourceIncomplete,
                );
            }
        };
        run_api_maintenance_prediction(
            ApiMaintenancePredictionInput {
                source,
                registration,
                replay,
            },
            codec,
            ports,
            receipt_values,
        )
    }

    /// Use only the exact scheduled instant and selected immutable registration identities.
    fn source_header<'a>(
        &'a self,
        registration: &'a AdmittedApiMaintenanceRegistration<'_>,
    ) -> MaintenanceSourceProjectionHeader<'a> {
        MaintenanceSourceProjectionHeader {
            registration_sha256: registration.registration_sha256,
            candidate_sha256: registration.candidate_sha256,
            source_sha256: registration.source_sha256,
            due_key: &self.request.request.dueKey,
            issued_at: &self.request.issued_at,
        }
    }
}

/// Avoid binding a selected package to the other API family's database facade.
fn require_registration_family(
    registration: &AdmittedApiMaintenanceRegistration<'_>,
    family: AdjustmentShadowFamily,
) -> Result<(), OperationError> {
    (registration.family == family)
        .then_some(())
        .ok_or(OperationError::Rejected)
}

/// The retained runtime schedules jobs; each job owns replay, source admission, and publication.
///
/// The owner allocates each job's codec and database/relay handles independently. Starting a
/// job must not defer it until finish; dropping a pending result leaves started work running.
pub trait ApiMaintenanceCaptureExecutor<'a> {
    type Pending;

    /// Start actual Rust capture work with the existing production owners.
    fn start(&mut self, job: ApiMaintenanceCaptureJob<'a>) -> Self::Pending;

    /// Observe completion of both started jobs; a returned categorical gap is not a failure.
    fn finish(
        &mut self,
        temperature: Option<Self::Pending>,
        wind: Option<Self::Pending>,
    ) -> Result<(), OperationError>;
}

/// Installed candidate set bound to the private capture callback's retained execution owner.
pub struct PreparedApiMaintenanceCaptureOperations<'a, Executor, Gap> {
    pub temperature: Option<InstalledApiTemperatureCapture<'a>>,
    pub wind: Option<InstalledApiWindCapture<'a>>,
    pub witness: &'a AdmittedAdjustmentRevisionCaptureWitness,
    pub time: &'a dyn CurrentTimeResolver,
    pub executor: Executor,
    pub gap: Gap,
}

impl<'a, Executor: ApiMaintenanceCaptureExecutor<'a>, Gap: AdjustmentMaintenanceGapCallback>
    ApiMaintenanceCaptureOperations for PreparedApiMaintenanceCaptureOperations<'a, Executor, Gap>
{
    type Pending = Executor::Pending;

    /// Return only the immutable selected temperature registration, if any.
    fn temperature_registration_sha256(&self) -> Option<&str> {
        Some(self.temperature.as_ref()?.registration.registration_sha256)
    }

    /// Keep uninstalled wind outside the database read and evidence path.
    fn wind_installed(&self) -> bool {
        self.wind.is_some()
    }

    /// Schedule temperature source projection before the separate wind database read.
    fn start_temperature_capture(
        &mut self,
        request: &AdmittedAdjustmentMaintenanceCapture,
        best_match: Vec<WeatherRecordRow>,
        sidecar: EcmwfTemperatureCanarySidecar,
    ) -> Result<Self::Pending, OperationError> {
        let installed = self.temperature.ok_or(OperationError::Rejected)?;
        Ok(self.executor.start(ApiMaintenanceCaptureJob {
            request: request.clone(),
            witness: self.witness,
            time: self.time,
            family: CaptureFamily::Temperature {
                installed,
                best_match,
                sidecar: Box::new(sidecar),
            },
        }))
    }

    /// Schedule wind replay from the same forecast rows the retained repository returned.
    fn start_wind_capture(
        &mut self,
        request: &AdmittedAdjustmentMaintenanceCapture,
        rows: Vec<WeatherRecordRow>,
    ) -> Result<Self::Pending, OperationError> {
        let installed = self.wind.ok_or(OperationError::Rejected)?;
        Ok(self.executor.start(ApiMaintenanceCaptureJob {
            request: request.clone(),
            witness: self.witness,
            time: self.time,
            family: CaptureFamily::Wind { installed, rows },
        }))
    }

    /// Await the direct missing-sidecar gap, unlike the best-effort capture-internal gaps.
    fn record_temperature_source_gap(
        &mut self,
        request: AdjustmentMaintenanceGapInput,
    ) -> Result<(), OperationError> {
        self.gap
            .record_gap(AdmittedAdjustmentMaintenanceGap { request })
    }

    /// Observe the original pair of started captures without changing their result classification.
    fn finish_captures(
        &mut self,
        temperature: Option<Self::Pending>,
        wind: Option<Self::Pending>,
    ) -> Result<(), OperationError> {
        self.executor.finish(temperature, wind)
    }
}

#[path = "native_api_capture_execution_support.rs"]
mod native_execution_support;
pub use native_execution_support::*;
