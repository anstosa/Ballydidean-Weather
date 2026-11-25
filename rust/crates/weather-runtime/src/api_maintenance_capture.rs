//! Bind the private maintenance capture callback to retained forecast reads.

use weather_api::{
    AdjustmentMaintenanceCaptureCallback, AdjustmentMaintenanceOperationError,
    AdmittedAdjustmentMaintenanceCapture, CanonicalInstant, CurrentTimeResolver,
};
use weather_contracts::{
    AdjustmentMaintenanceGapInput, AdjustmentMaintenanceGapInputFamilyEnum,
    EcmwfTemperatureCanarySidecar, FieldPresence, ForecastQuery, JsDateOrString,
    MaintenanceShadowGapReason, WeatherRecordRow,
};
use weather_database::{
    AdmittedEcmwfTemperatureCanarySidecarQuery, AdmittedForecastQuery, ForecastReadPlan,
    forecast_read_plan,
};

use crate::{PooledWeatherApiReads, WeatherApiReadPool, read_api_forecast};

const SITE: &str = "ballydidean";
const HOUR_MILLISECONDS: i64 = 3_600_000;
const TEMPERATURE_HOURS: u64 = 12;
const WIND_HOURS: u64 = 168;
type OperationError = AdjustmentMaintenanceOperationError;

/// Format already computed milliseconds through the admitted canonical UTC owner.
pub trait ApiMaintenanceCaptureTime {
    /// Return one canonical ISO date for the requested epoch, or refuse an unsupported date.
    fn utc_at_epoch_milliseconds(&self, milliseconds: i64) -> Option<String>;
}

impl<Time: CurrentTimeResolver> ApiMaintenanceCaptureTime for Time {
    /// Reuse the same UTC formatter as the API process and request clocks.
    fn utc_at_epoch_milliseconds(&self, milliseconds: i64) -> Option<String> {
        self.resolve(&JsDateOrString::DateEpochMilliseconds(milliseconds))
            .ok()
            .filter(|instant| instant.epoch_milliseconds == milliseconds)
            .map(|instant| instant.utc)
    }
}

/// Independently acquired forecast and ECMWF sides of the temperature capture.
pub struct ApiMaintenanceTemperatureReads {
    pub best_match_rows: Vec<WeatherRecordRow>,
    pub sidecar: Option<EcmwfTemperatureCanarySidecar>,
}

/// Execute exact forecast plans against the API process's retained data source.
pub trait ApiMaintenanceCaptureReads {
    /// Start the two temperature sequences together and keep their own result identities.
    fn read_temperature(
        &mut self,
        forecast: &ForecastReadPlan,
        sidecar: &AdmittedEcmwfTemperatureCanarySidecarQuery,
    ) -> Result<ApiMaintenanceTemperatureReads, OperationError>;

    /// Load the complete wind forecast sequence with optional provenance or raw fallback.
    fn read_wind(
        &mut self,
        forecast: &ForecastReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, OperationError>;
}

/// Parallel temperature reads supplied by the same driver as the public API pool.
pub trait WeatherApiMaintenanceCapturePool: WeatherApiReadPool {
    /// Acquire both sequences independently, including their normal subsequent queries.
    ///
    /// The forecast side uses `read_api_forecast`; the sidecar side uses the database's
    /// selected-run/hour read. Neither sequence holds a cross-sequence transaction.
    fn read_temperature_capture_cohort(
        &mut self,
        forecast: &ForecastReadPlan,
        sidecar: &AdmittedEcmwfTemperatureCanarySidecarQuery,
    ) -> Result<ApiMaintenanceTemperatureReads, Self::Error>;
}

/// Read adapter using a handle to the already retained API pool.
pub struct PooledApiMaintenanceCaptureReads<Pool> {
    database: PooledWeatherApiReads<Pool>,
}

impl<Pool> PooledApiMaintenanceCaptureReads<Pool> {
    /// Accept an existing pool handle without constructing a new pool.
    pub const fn new(handle: Pool) -> Self {
        Self {
            database: PooledWeatherApiReads::new(handle),
        }
    }
}

impl<Pool: WeatherApiMaintenanceCapturePool> ApiMaintenanceCaptureReads
    for PooledApiMaintenanceCaptureReads<Pool>
{
    /// Give the pair of whole read sequences to the pool's admitted concurrent owner.
    fn read_temperature(
        &mut self,
        forecast: &ForecastReadPlan,
        sidecar: &AdmittedEcmwfTemperatureCanarySidecarQuery,
    ) -> Result<ApiMaintenanceTemperatureReads, OperationError> {
        self.database
            .pool_mut()
            .read_temperature_capture_cohort(forecast, sidecar)
            .map_err(|_| OperationError::Rejected)
    }

    /// Use the same real query planning and provenance assembly as public API reads.
    fn read_wind(
        &mut self,
        forecast: &ForecastReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, OperationError> {
        read_api_forecast(&mut self.database, forecast).map_err(|_| OperationError::Rejected)
    }
}

/// Existing installed candidate, evaluator, and archive owners for one capture callback.
pub trait ApiMaintenanceCaptureOperations {
    type Pending;

    /// Report the admitted temperature registration only when its candidate is installed.
    fn temperature_registration_sha256(&self) -> Option<&str>;

    /// Report whether the admitted wind candidate is installed.
    fn wind_installed(&self) -> bool;

    /// Start temperature evaluation and archive work before the wind read.
    ///
    /// The returned handle must not cancel already-started work when dropped, as the
    /// original capture can keep running if the later wind read fails.
    fn start_temperature_capture(
        &mut self,
        request: &AdmittedAdjustmentMaintenanceCapture,
        best_match_rows: Vec<WeatherRecordRow>,
        sidecar: EcmwfTemperatureCanarySidecar,
    ) -> Result<Self::Pending, OperationError>;

    /// Start wind evaluation and archive work with the complete admitted read.
    fn start_wind_capture(
        &mut self,
        request: &AdmittedAdjustmentMaintenanceCapture,
        rows: Vec<WeatherRecordRow>,
    ) -> Result<Self::Pending, OperationError>;

    /// Await the value-free source gap before doing any wind work.
    fn record_temperature_source_gap(
        &mut self,
        gap: AdjustmentMaintenanceGapInput,
    ) -> Result<(), OperationError>;

    /// Observe both started operations together and propagate their reported failure.
    fn finish_captures(
        &mut self,
        temperature: Option<Self::Pending>,
        wind: Option<Self::Pending>,
    ) -> Result<(), OperationError>;
}

/// Ready to install in the private listener once its configured owners are admitted.
pub struct RetainedApiMaintenanceCapture<Reads, Time, Operations> {
    reads: Reads,
    time: Time,
    operations: Operations,
}

impl<Reads, Time, Operations> RetainedApiMaintenanceCapture<Reads, Time, Operations> {
    /// Retain the real repository, time owner and candidate work without starting capture.
    pub const fn new(reads: Reads, time: Time, operations: Operations) -> Self {
        Self {
            reads,
            time,
            operations,
        }
    }

    /// Return the bound owners without rerunning their admission.
    pub fn into_parts(self) -> (Reads, Time, Operations) {
        (self.reads, self.time, self.operations)
    }
}

impl<Reads, Time, Operations> AdjustmentMaintenanceCaptureCallback
    for RetainedApiMaintenanceCapture<Reads, Time, Operations>
where
    Reads: ApiMaintenanceCaptureReads,
    Time: ApiMaintenanceCaptureTime,
    Operations: ApiMaintenanceCaptureOperations,
{
    /// Run the original temperature read/gap, wind read, and joint completion order.
    fn capture(
        &mut self,
        request: AdmittedAdjustmentMaintenanceCapture,
    ) -> Result<(), OperationError> {
        let temperature_sidecar = temperature_sidecar_query(&self.time, &request.issued_at)?;
        // omit an absent temperature candidate without fabricating a registration
        let temperature =
            if let Some(registration_sha256) = self.operations.temperature_registration_sha256() {
                let forecast = forecast_plan(&self.time, &request.issued_at, TEMPERATURE_HOURS)?;
                let reads = self
                    .reads
                    .read_temperature(&forecast, &temperature_sidecar)?;
                // source-gap publication is awaited before starting the wind side
                match reads.sidecar {
                    Some(sidecar) => Some(self.operations.start_temperature_capture(
                        &request,
                        reads.best_match_rows,
                        sidecar,
                    )?),
                    None => {
                        let registration_sha256 = registration_sha256.to_owned();
                        self.operations.record_temperature_source_gap(
                            AdjustmentMaintenanceGapInput {
                                dueKey: request.request.dueKey.clone(),
                                family: AdjustmentMaintenanceGapInputFamilyEnum::Temperature,
                                reason: MaintenanceShadowGapReason::SourceIncomplete,
                                registrationSha256: registration_sha256,
                            },
                        )?;
                        None
                    }
                }
            } else {
                None
            };

        // start wind evaluation only after fetching the original 168-hour forecast
        let wind = if self.operations.wind_installed() {
            let forecast = forecast_plan(&self.time, &request.issued_at, WIND_HOURS)?;
            let rows = self.reads.read_wind(&forecast)?;
            Some(self.operations.start_wind_capture(&request, rows)?)
        } else {
            None
        };
        // an empty source installation issues no capture or archive operation
        if temperature.is_none() && wind.is_none() {
            return Ok(());
        }
        self.operations.finish_captures(temperature, wind)
    }
}

/// Construct the existing default-product query with the original issue-time anchor.
fn forecast_plan(
    time: &impl ApiMaintenanceCaptureTime,
    issued: &CanonicalInstant,
    hours: u64,
) -> Result<ForecastReadPlan, OperationError> {
    let end = offset_hours(issued.epoch_milliseconds, hours)?;
    let end_utc = time
        .utc_at_epoch_milliseconds(end)
        .ok_or(OperationError::Rejected)?;
    forecast_read_plan(&AdmittedForecastQuery::admitted(
        ForecastQuery {
            asOf: issued.utc.clone(),
            hours,
            productSelection: FieldPresence::Missing,
            siteSlug: SITE.to_owned(),
        },
        issued.epoch_milliseconds,
        end_utc,
        end,
    ))
    .map_err(|_| OperationError::Rejected)
}

/// Derive the separate ECMWF window from the ceiling of the issue hour.
fn temperature_sidecar_query(
    time: &impl ApiMaintenanceCaptureTime,
    issued: &CanonicalInstant,
) -> Result<AdmittedEcmwfTemperatureCanarySidecarQuery, OperationError> {
    let remainder = issued.epoch_milliseconds.rem_euclid(HOUR_MILLISECONDS);
    let from = issued
        .epoch_milliseconds
        .checked_add((-remainder).rem_euclid(HOUR_MILLISECONDS))
        .ok_or(OperationError::Rejected)?;
    let to = offset_hours(from, TEMPERATURE_HOURS)?;
    Ok(AdmittedEcmwfTemperatureCanarySidecarQuery::admitted(
        issued.utc.clone(),
        time.utc_at_epoch_milliseconds(from)
            .ok_or(OperationError::Rejected)?,
        from,
        SITE.to_owned(),
        time.utc_at_epoch_milliseconds(to)
            .ok_or(OperationError::Rejected)?,
        to,
    ))
}

/// Add the fixed hour span without overflowing a captured fixture or process clock.
fn offset_hours(epoch_milliseconds: i64, hours: u64) -> Result<i64, OperationError> {
    let duration = i64::try_from(hours)
        .ok()
        .and_then(|hours| hours.checked_mul(HOUR_MILLISECONDS))
        .ok_or(OperationError::Rejected)?;
    epoch_milliseconds
        .checked_add(duration)
        .ok_or(OperationError::Rejected)
}

#[cfg(test)]
mod tests {
    mod pipeline {
        include!("api_maintenance_capture_pipeline_test.rs");
    }

    use std::cell::RefCell;

    use weather_api::AdjustmentMaintenanceCaptureCycle;
    use weather_contracts::{
        AdjustmentMaintenanceCaptureRequest, EcmwfTemperatureCanaryRecentErrorState,
        EcmwfTemperatureCanaryRecentErrorStateCohortLiteral, EcmwfTemperatureCanaryRun,
        EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
        EcmwfTemperatureCanaryRunStateStatusEnum, EcmwfTemperatureCanaryRunUpstreamModelLiteral,
        Nullable,
    };
    use weather_database::ForecastReadBind;
    use weather_worker::{
        MaintenanceCaptureDue, TaskFailure, TemperatureCanaryTimeAdapter,
        adjustment_maintenance_capture_due_at,
    };

    use super::*;
    use crate::{
        RetainedWorkerSidecarTask, WorkerMaintenanceCapturePost, WorkerMaintenanceCaptureResponse,
        WorkerMaintenanceCaptureTask, WorkerMaintenanceCaptureTransport,
    };

    const ISSUE_MS: i64 = 1_788_784_500_000;
    const ISSUE_UTC: &str = "2026-09-07T12:35:00.000Z";
    const TEMPERATURE_END: &str = "2026-09-08T00:35:00.000Z";
    const SIDECAR_FROM: &str = "2026-09-07T13:00:00.000Z";
    const SIDECAR_TO: &str = "2026-09-08T01:00:00.000Z";
    const WIND_END: &str = "2026-09-14T12:35:00.000Z";
    const REGISTRATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    struct FixtureTime;

    impl ApiMaintenanceCaptureTime for FixtureTime {
        /// Project only the original distinct windows from the due-clock fixture.
        fn utc_at_epoch_milliseconds(&self, milliseconds: i64) -> Option<String> {
            let relative = milliseconds - ISSUE_MS;
            let utc = match relative {
                1_500_000 => SIDECAR_FROM,
                43_200_000 => TEMPERATURE_END,
                44_700_000 => SIDECAR_TO,
                604_800_000 => WIND_END,
                _ => panic!("unexpected capture clock offset {relative}"),
            };
            Some(utc.to_owned())
        }
    }

    impl TemperatureCanaryTimeAdapter for FixtureTime {
        /// Format the worker's derived issue time before it dispatches the typed POST.
        fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
            assert_eq!(epoch_ms, ISSUE_MS);
            Ok(ISSUE_UTC.to_owned())
        }
    }

    struct FixtureReads<'a> {
        events: &'a RefCell<Vec<&'static str>>,
        sidecar_present: bool,
    }

    impl ApiMaintenanceCaptureReads for FixtureReads<'_> {
        /// Observe one pair of source sequences with different original time windows.
        fn read_temperature(
            &mut self,
            forecast: &ForecastReadPlan,
            sidecar: &AdmittedEcmwfTemperatureCanarySidecarQuery,
        ) -> Result<ApiMaintenanceTemperatureReads, OperationError> {
            self.events.borrow_mut().push("temperature-pair");
            assert_forecast(forecast, TEMPERATURE_END);
            assert_eq!(sidecar.site_slug, SITE);
            assert_eq!(sidecar.as_of, ISSUE_UTC);
            assert_eq!(sidecar.from, SIDECAR_FROM);
            assert_eq!(sidecar.to, SIDECAR_TO);
            Ok(ApiMaintenanceTemperatureReads {
                best_match_rows: Vec::new(),
                sidecar: self.sidecar_present.then(sidecar_fixture),
            })
        }

        /// Observe the untouched issue instant and 168-hour default-product span.
        fn read_wind(
            &mut self,
            forecast: &ForecastReadPlan,
        ) -> Result<Vec<WeatherRecordRow>, OperationError> {
            self.events.borrow_mut().push("wind-read");
            assert_forecast(forecast, WIND_END);
            Ok(Vec::new())
        }
    }

    struct FixtureOperations<'a> {
        events: &'a RefCell<Vec<&'static str>>,
        gap_fails: bool,
        temperature_installed: bool,
        wind_installed: bool,
    }

    struct FixtureTransport<'a, Capture>(&'a mut Capture);

    impl<Capture: AdjustmentMaintenanceCaptureCallback> WorkerMaintenanceCaptureTransport
        for FixtureTransport<'_, Capture>
    {
        type Error = ();

        /// Route the exact typed worker POST to the admitted capture without network traffic.
        fn post_json(
            &mut self,
            post: WorkerMaintenanceCapturePost<'_>,
        ) -> Result<WorkerMaintenanceCaptureResponse, Self::Error> {
            assert_eq!(post.pathname, weather_api::MAINTENANCE_CAPTURE_PATH);
            assert_eq!(post.content_type, "application/json");
            let result = self.0.capture(admitted(post.body.clone()));
            Ok(WorkerMaintenanceCaptureResponse {
                status_code: result.map_or(409, |()| 202),
            })
        }
    }

    impl ApiMaintenanceCaptureOperations for FixtureOperations<'_> {
        type Pending = &'static str;

        /// Load only the installed temperature registration.
        fn temperature_registration_sha256(&self) -> Option<&str> {
            self.temperature_installed.then_some(REGISTRATION)
        }

        /// Expose the installed wind candidate choice.
        fn wind_installed(&self) -> bool {
            self.wind_installed
        }

        /// Observe temperature start before the wind read and preserve the selected run.
        fn start_temperature_capture(
            &mut self,
            request: &AdmittedAdjustmentMaintenanceCapture,
            best_match_rows: Vec<WeatherRecordRow>,
            sidecar: EcmwfTemperatureCanarySidecar,
        ) -> Result<Self::Pending, OperationError> {
            self.events.borrow_mut().push("temperature-start");
            assert_eq!(request.request.dueKey, format!("capture/{ISSUE_UTC}"));
            assert!(best_match_rows.is_empty());
            assert_eq!(sidecar.run.id, "selected-run");
            Ok("temperature")
        }

        /// Observe wind start after its own retained forecast read.
        fn start_wind_capture(
            &mut self,
            request: &AdmittedAdjustmentMaintenanceCapture,
            rows: Vec<WeatherRecordRow>,
        ) -> Result<Self::Pending, OperationError> {
            self.events.borrow_mut().push("wind-start");
            assert_eq!(request.request.issuedAt, ISSUE_UTC);
            assert!(rows.is_empty());
            Ok("wind")
        }

        /// Return one ordinary gap rejection if the fixture archive is unavailable.
        fn record_temperature_source_gap(
            &mut self,
            gap: AdjustmentMaintenanceGapInput,
        ) -> Result<(), OperationError> {
            self.events.borrow_mut().push("temperature-gap");
            assert_eq!(
                gap.family,
                AdjustmentMaintenanceGapInputFamilyEnum::Temperature
            );
            assert_eq!(gap.reason, MaintenanceShadowGapReason::SourceIncomplete);
            assert_eq!(gap.registrationSha256, REGISTRATION);
            assert_eq!(gap.dueKey, format!("capture/{ISSUE_UTC}"));
            // stop before wind reads when the predecessor gap relay fails
            if self.gap_fails {
                return Err(OperationError::Rejected);
            }
            Ok(())
        }

        /// Verify that completion observes both actually started capture handles.
        fn finish_captures(
            &mut self,
            temperature: Option<Self::Pending>,
            wind: Option<Self::Pending>,
        ) -> Result<(), OperationError> {
            let recorded = self.events.borrow().contains(&"temperature-start");
            assert_eq!(temperature, recorded.then_some("temperature"));
            assert_eq!(wind, self.wind_installed.then_some("wind"));
            self.events.borrow_mut().push("finish");
            Ok(())
        }
    }

    /// Use two source windows and start temperature before reading wind.
    #[test]
    fn capture_uses_distinct_source_windows_and_waits_for_started_captures() {
        let events = RefCell::new(Vec::new());
        let mut api = fixture_capture(&events, true, false, true, true);
        let mut worker = WorkerMaintenanceCaptureTask::new(FixtureTransport(&mut api), FixtureTime);
        let due = adjustment_maintenance_capture_due_at(ISSUE_MS + 60_000).expect("capture due");
        assert_eq!(
            RetainedWorkerSidecarTask::<(), (), MaintenanceCaptureDue>::run(
                &mut worker,
                due,
                &mut (),
                &mut (),
            ),
            Ok(())
        );
        assert_eq!(
            events.borrow().as_slice(),
            [
                "temperature-pair",
                "temperature-start",
                "wind-read",
                "wind-start",
                "finish"
            ]
        );
    }

    /// Wait for a missing-sidecar gap, stopping before wind only when that write fails.
    #[test]
    fn capture_awaits_a_missing_temperature_gap_and_skips_uninstalled_families() {
        let events = RefCell::new(Vec::new());
        assert_eq!(capture(&events, false, false, true, true), Ok(()));
        assert_eq!(
            events.replace(Vec::new()),
            [
                "temperature-pair",
                "temperature-gap",
                "wind-read",
                "wind-start",
                "finish"
            ]
        );
        assert_eq!(
            capture(&events, false, true, true, true),
            Err(OperationError::Rejected)
        );
        assert_eq!(
            events.replace(Vec::new()),
            ["temperature-pair", "temperature-gap"]
        );
        assert_eq!(capture(&events, false, false, false, false), Ok(()));
        assert!(events.borrow().is_empty());
    }

    /// Run the assembled private callback with one admitted, typed worker request.
    fn capture(
        events: &RefCell<Vec<&'static str>>,
        sidecar_present: bool,
        gap_fails: bool,
        temperature_installed: bool,
        wind_installed: bool,
    ) -> Result<(), OperationError> {
        fixture_capture(
            events,
            sidecar_present,
            gap_fails,
            temperature_installed,
            wind_installed,
        )
        .capture(admitted(AdjustmentMaintenanceCaptureRequest {
            dueKey: format!("capture/{ISSUE_UTC}"),
            issuedAt: ISSUE_UTC.to_owned(),
        }))
    }

    /// Bind the retained callback to deterministic sources and candidate operations.
    fn fixture_capture<'a>(
        events: &'a RefCell<Vec<&'static str>>,
        sidecar_present: bool,
        gap_fails: bool,
        temperature_installed: bool,
        wind_installed: bool,
    ) -> RetainedApiMaintenanceCapture<FixtureReads<'a>, FixtureTime, FixtureOperations<'a>> {
        RetainedApiMaintenanceCapture::new(
            FixtureReads {
                events,
                sidecar_present,
            },
            FixtureTime,
            FixtureOperations {
                events,
                gap_fails,
                temperature_installed,
                wind_installed,
            },
        )
    }

    /// Represent the already admitted internal body's fixed canonical issue instant.
    fn admitted(
        request: AdjustmentMaintenanceCaptureRequest,
    ) -> AdmittedAdjustmentMaintenanceCapture {
        assert_eq!(request.issuedAt, ISSUE_UTC);
        AdmittedAdjustmentMaintenanceCapture {
            request,
            issued_at: CanonicalInstant {
                epoch_milliseconds: ISSUE_MS,
                utc: ISSUE_UTC.to_owned(),
            },
            cycle: AdjustmentMaintenanceCaptureCycle::Hour12,
        }
    }

    /// Confirm the prepared bind order and original implicit latest-product selector.
    fn assert_forecast(plan: &ForecastReadPlan, end: &str) {
        assert_eq!(
            plan.binds,
            [
                ForecastReadBind::Text(SITE.to_owned()),
                ForecastReadBind::UtcInstant(ISSUE_UTC.to_owned()),
                ForecastReadBind::UtcInstant(end.to_owned()),
            ]
        );
        assert!(plan.sql.contains("FROM public.forecast_products"));
    }

    /// Supply an already decoded eligible sidecar without fabricating forecast rows.
    fn sidecar_fixture() -> EcmwfTemperatureCanarySidecar {
        EcmwfTemperatureCanarySidecar {
            hours: Vec::new(),
            run: EcmwfTemperatureCanaryRun {
                adapterVersion: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
                firstReceivedAt: ISSUE_UTC.to_owned(),
                id: "selected-run".to_owned(),
                modelCycle: EcmwfTemperatureCanaryRunModelCycleEnum::Value49r1,
                providerResponseSha256: "d".repeat(64),
                recentErrorState: EcmwfTemperatureCanaryRecentErrorState {
                    b24C: Nullable::Null,
                    b72C: Nullable::Null,
                    cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
                    localDates: 0,
                    mad72C: Nullable::Null,
                    maximumSourceRunInitializedAt: Nullable::Null,
                    maximumSourceValidAt: Nullable::Null,
                    n24: 0,
                    n72: 0,
                    sourceKeys: Vec::new(),
                    supported: false,
                    targetRunInitializedAt: ISSUE_UTC.to_owned(),
                    windowEndValidAt: ISSUE_UTC.to_owned(),
                },
                runInitializedAt: ISSUE_UTC.to_owned(),
                stateReason: "no_causal_forecast_observation_pairs".to_owned(),
                stateStatus: EcmwfTemperatureCanaryRunStateStatusEnum::Cold,
                upstreamModel: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
            },
        }
    }
}
