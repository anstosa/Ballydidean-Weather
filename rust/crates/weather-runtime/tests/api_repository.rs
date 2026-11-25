use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_api::{
    AdjustmentMaintenanceOperationError, AdjustmentMaintenancePublishRepository,
    AdjustmentMaintenanceScheduleRepository, AdmittedHistoryCursor, CanonicalInstant,
    CurrentProjectionError, CurrentTimeResolver, DailyPrecipitationServiceBoundary, DatabaseStatus,
    DecodedServiceRequest, ForecastObservationGeometryBoundary, ForecastRainAdjustmentRead,
    ForecastSelection, ForecastServiceBoundary, ForecastTemperatureSidecarRead,
    ForecastTemperatureStatusRead, HistoryBoundaryAdapter, HistoryBoundaryError, HistoryQueryTime,
    InactiveForecastServiceRuntime, MigrationStatus, QueryParameter,
    RainCollectionTimestampBoundary, ResolvedDailyPrecipitationWindow,
    ResolvedForecastObservationGeometry, ResolvedServiceForecastWindow, ServiceClockError,
    TideBoundaryAdapter, TideBoundaryError, TideQueryTime, TrendTimeBoundary, WeatherServiceBody,
    WeatherServiceBoundaries, WeatherServiceClock, WeatherServiceRepository,
    maintenance_schedule_slot_reads, serve_decoded_request,
};
use weather_contracts::{
    AdjustmentForecastAnchorRevisionAdmissionKey, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionGapInput, AdjustmentRevisionGapReason, AdjustmentRevisionProjectionKind,
    AdjustmentShadowFamily, AdjustmentShadowRegistrationSlot,
    AdjustmentShadowRegistrationSlotContractVersionLiteral,
    AdjustmentShadowRegistrationSlotStateEnum, CurrentWeatherRecordRow, DailyPrecipitationRow,
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
    EcmwfTemperatureCanaryRunStateStatusEnum, EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    EcmwfTemperatureCanaryStatusStateStatusEnum, HistoryQueryCursor, JsDate, JsDateOrString,
    Nullable, RainAdjustmentHour, TideRecordRow, TrendPointRow, WeatherRecordRow,
};
use weather_database::{
    ADJUSTMENT_SHADOW_REGISTRATION_SLOT_SQL, ActiveSiteDbRow, AdjustmentMaintenanceDatabasePort,
    AdjustmentRevisionAdmissionKey, AdjustmentRevisionGapMarker,
    AdjustmentShadowRegistrationSlotBind, AdjustmentShadowRegistrationSlotQueryPlan,
    AdmittedEcmwfTemperatureCanarySidecarQuery, AdmittedHealthMigrationIdentity,
    AdmittedRevisionUtcInstant, AppliedMigration, CurrentWeatherQueryPlan,
    DailyPrecipitationReadPlan, DatabaseConfiguration, DatabasePoolConfiguration,
    ECMWF_TEMPERATURE_CANARY_SIDECAR_HOURS_SQL, ECMWF_TEMPERATURE_CANARY_SIDECAR_RUN_SQL,
    ECMWF_TEMPERATURE_CANARY_STATUS_SQL, EcmwfTemperatureCanaryHourReadRow,
    EcmwfTemperatureCanaryReadBind, EcmwfTemperatureCanaryReadPort,
    EcmwfTemperatureCanarySidecarHoursReadPlan, EcmwfTemperatureCanarySidecarRunReadPlan,
    EcmwfTemperatureCanaryStatusReadPlan, EcmwfTemperatureCanaryStatusReadRow,
    ForecastObservationInputDbRow, ForecastObservationInputReadPlan, ForecastPressureContextPlan,
    ForecastProvenancePlan, ForecastReadPlan, ForecastRuntimeProvenanceRow,
    HealthLatestWorkerHeartbeatReadPlan, HealthLatestWorkerHeartbeatRow,
    HealthMigrationLedgerReadPlan, HealthPingReadPlan, HistoryReadPlan, LIST_ACTIVE_SITES_SQL,
    ListActiveSitesQuery, MaintenancePublicationBind, MaintenancePublicationPlan, MigrationDigest,
    MigrationScript, RainAdjustmentRunReadPlan, RainAdjustmentRunReadRow,
    RainCollectionStatusReadPlan, RainCollectionStatusReadRow, RainServingReadBind,
    RainServingReadPort, Sha256Digest, StoredEcmwfCanaryRun, TideReadPlan, TrendReadPlan,
};
use weather_runtime::{
    AdmittedHealthMigrationArtifacts, ApiReadPoolLease, ApiReadSessionDisposition,
    ApiReadSessionFailure, PooledWeatherApiReads, PostgresWeatherServiceRepository,
    RainRepositoryTimeBoundary, TemperatureCanaryRecentErrorDigestPort, WeatherApiMaintenancePool,
    WeatherApiReadPool, WeatherApiReadPoolConnector, WeatherServiceExternalReadPort,
    WeatherServicePostgresReadPort, prepare_pooled_weather_service_repository,
};
use weather_worker::TaskFailure;

#[path = "api_repository/startup_fixture.rs"]
mod api_repository_startup_fixture;
#[path = "api_repository/native_api_process_http.rs"]
mod native_api_process_http;
#[path = "api_repository/native_maintenance_http.rs"]
mod native_maintenance_http;
#[path = "api_repository/native_public_http.rs"]
mod native_public_http;

const RUN_AT: &str = "2026-09-07T12:00:00.000Z";
const SIDECAR_AS_OF: &str = "2026-09-07T18:03:00.000Z";
const STATUS_AS_OF: &str = "2026-09-08T00:03:00.000Z";
const RAIN_AS_OF: &str = "2026-09-08T06:00:00.000Z";
const RAIN_RUN_AT: &str = "2026-09-07T18:00:00.000Z";
const FROM: &str = "2026-09-07T12:00:00.000Z";
const TO: &str = "2026-09-09T00:00:00.000Z";
const RECENT_ERROR_STATE_SHA256: &str =
    "03a38e418a1a7fd5334fb5a9faa1f81ec2633817582888c12db76eff0d24511d";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureError;

impl Display for FixtureError {
    /// Render one injected repository failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("fixture repository failure")
    }
}

impl Error for FixtureError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixturePoolError {
    Checkout,
    StatementOrClient,
    PostQueryDecode,
}

impl Display for FixturePoolError {
    /// Render one typed pool-boundary failure without exposing driver diagnostics.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("fixture pool failure")
    }
}

impl Error for FixturePoolError {}

#[derive(Debug, Default)]
struct FixtureDatabase {
    active_plans: Vec<ListActiveSitesQuery>,
    api_pool_events: Option<Rc<RefCell<Vec<ApiPoolEvent>>>>,
    active_failure: Option<FixturePoolError>,
    health_applied: Vec<AppliedMigration>,
    health_failure: Option<&'static str>,
    health_order: Option<Rc<RefCell<Vec<&'static str>>>>,
    health_trace: Vec<&'static str>,
    heartbeat: Option<HealthLatestWorkerHeartbeatRow>,
    rain_run: Option<RainAdjustmentRunReadRow>,
    rain_run_plan: Option<RainAdjustmentRunReadPlan>,
    rain_status: Option<RainCollectionStatusReadRow>,
    rain_status_plan: Option<RainCollectionStatusReadPlan>,
    sidecar_hours_plan: Option<EcmwfTemperatureCanarySidecarHoursReadPlan>,
    sidecar_run_plan: Option<EcmwfTemperatureCanarySidecarRunReadPlan>,
    status_plan: Option<EcmwfTemperatureCanaryStatusReadPlan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiPoolEvent {
    Checkout,
    Share(usize),
    ScheduleCohort([AdjustmentShadowFamily; 3]),
    Query(&'static str, usize),
    Release(ApiReadSessionDisposition),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixtureCheckout {
    Ready,
    StatementFailure,
    PostQueryDecodeFailure,
    Unavailable,
}

struct FixtureApiReadPool {
    events: Rc<RefCell<Vec<ApiPoolEvent>>>,
    attempts: Rc<RefCell<VecDeque<FixtureCheckout>>>,
}

struct FixtureApiPoolConnector {
    events: Rc<RefCell<Vec<ApiPoolEvent>>>,
    settings: Option<DatabasePoolConfiguration>,
}

impl WeatherApiReadPoolConnector for FixtureApiPoolConnector {
    type Error = FixturePoolError;
    type Pool = FixtureApiReadPool;

    /// Capture the admitted pool settings and prepare a bounded fixture sequence.
    fn prepare_pool(
        &mut self,
        configuration: DatabasePoolConfiguration,
    ) -> Result<Self::Pool, Self::Error> {
        self.settings = Some(configuration);
        Ok(FixtureApiReadPool {
            events: Rc::clone(&self.events),
            attempts: Rc::new(RefCell::new(VecDeque::from([
                FixtureCheckout::Ready,
                FixtureCheckout::StatementFailure,
                FixtureCheckout::PostQueryDecodeFailure,
                FixtureCheckout::Unavailable,
                FixtureCheckout::Ready,
            ]))),
        })
    }
}

impl weather_runtime::WeatherApiSharedPool for FixtureApiReadPool {
    /// Share the same fixture capacity and event evidence without preparing another pool.
    fn share(&self) -> Self {
        self.events
            .borrow_mut()
            .push(ApiPoolEvent::Share(Rc::as_ptr(&self.attempts) as usize));
        Self {
            events: Rc::clone(&self.events),
            attempts: Rc::clone(&self.attempts),
        }
    }
}

impl WeatherApiReadPool for FixtureApiReadPool {
    type Error = FixturePoolError;
    type Session<'pool> = FixtureDatabase;

    /// Acquire one client or report a bounded fixture checkout failure.
    fn checkout(&mut self) -> Result<Self::Session<'_>, Self::Error> {
        self.events.borrow_mut().push(ApiPoolEvent::Checkout);
        let attempt = self
            .attempts
            .borrow_mut()
            .pop_front()
            .expect("expected checkout");
        // an unavailable pool has no checked-out client to release
        if attempt == FixtureCheckout::Unavailable {
            return Err(FixturePoolError::Checkout);
        }
        // bind each acquired lease to its injected operation phase
        let active_failure = match attempt {
            FixtureCheckout::StatementFailure => Some(FixturePoolError::StatementOrClient),
            FixtureCheckout::PostQueryDecodeFailure => Some(FixturePoolError::PostQueryDecode),
            FixtureCheckout::Ready | FixtureCheckout::Unavailable => None,
        };
        Ok(FixtureDatabase {
            api_pool_events: Some(Rc::clone(&self.events)),
            active_failure,
            ..FixtureDatabase::default()
        })
    }

    /// Map typed driver phases to release actions without inspecting error text.
    fn classify_session_failure(error: &Self::Error) -> ApiReadSessionFailure {
        // keep decode failures reusable only after the statement completed successfully
        match error {
            FixturePoolError::PostQueryDecode => ApiReadSessionFailure::PostQueryDecode,
            FixturePoolError::Checkout | FixturePoolError::StatementOrClient => {
                ApiReadSessionFailure::StatementOrClient
            }
        }
    }
}

impl WeatherApiMaintenancePool for FixtureApiReadPool {
    type MaintenanceSession<'pool> = FixtureDatabase;

    /// Borrow the same underlying pool and fixture attempt queue for private operations.
    fn checkout_maintenance(&mut self) -> Result<Self::MaintenanceSession<'_>, Self::Error> {
        self.checkout()
    }

    /// Record one ordered cohort dispatch instead of three serial wrapper calls.
    fn read_schedule_cohort(
        &mut self,
        plans: [AdjustmentShadowRegistrationSlotQueryPlan; 3],
    ) -> Result<[Vec<AdjustmentShadowRegistrationSlot>; 3], Self::Error> {
        let families = plans.map(schedule_family);
        self.events
            .borrow_mut()
            .push(ApiPoolEvent::ScheduleCohort(families));
        Ok(families.map(|family| vec![free_slot(family)]))
    }
}

impl AdjustmentMaintenanceDatabasePort for FixtureDatabase {
    type Error = FixturePoolError;

    /// Detect an accidental downgrade of the ordered cohort to single schedule calls.
    fn read_schedule_slot(
        &mut self,
        _plan: AdjustmentShadowRegistrationSlotQueryPlan,
    ) -> Result<Vec<AdjustmentShadowRegistrationSlot>, Self::Error> {
        panic!("expected one schedule cohort")
    }

    /// Detect an unexpected compact body read during the gap fixture.
    fn read_shadow_body_admission(
        &mut self,
        _plan: MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<bool>, Self::Error> {
        panic!("unexpected shadow body admission")
    }

    /// Detect an unexpected shadow receipt read during the gap fixture.
    fn read_shadow_revision_receipt(
        &mut self,
        _plan: MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
        panic!("unexpected shadow receipt")
    }

    /// Detect an unexpected revision receipt read during the gap fixture.
    fn read_current_revision_receipt(
        &mut self,
        _plan: MaintenancePublicationPlan<'_, 1>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
        panic!("unexpected current revision receipt")
    }

    /// Return the exact permanent disposition marker from the borrowed API pool.
    fn write_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        self.api_pool_events
            .as_ref()
            .expect("pooled fixture")
            .borrow_mut()
            .push(ApiPoolEvent::Query(plan.sql, plan.binds.len()));
        let [
            MaintenancePublicationBind::Text("forecast_anchor"),
            MaintenancePublicationBind::RevisionKeyJson(
                AdjustmentRevisionAdmissionKey::ForecastAnchor(key),
            ),
            MaintenancePublicationBind::GapJson(gap),
        ] = plan.binds
        else {
            panic!("expected exact forecast anchor gap binds");
        };
        // report the injected typed phase failure on the same client lease
        if let Some(error) = self.active_failure {
            return Err(error);
        }
        Ok(vec![AdjustmentRevisionGapMarker {
            logical_key_sha256: gap.logicalKeySha256.clone(),
            projection_identity_sha256: gap.projectionIdentitySha256.clone(),
            projection_kind: gap.projectionKind,
            projection_sha256: gap.projectionSha256.clone(),
            reason: gap.reason,
            stored_content_sha256: key.storedContentSha256.clone(),
        }])
    }
}

/// Return the plan's exact bound family after proving its original SQL facade.
fn schedule_family(plan: AdjustmentShadowRegistrationSlotQueryPlan) -> AdjustmentShadowFamily {
    assert_eq!(plan.sql, ADJUSTMENT_SHADOW_REGISTRATION_SLOT_SQL);
    let [AdjustmentShadowRegistrationSlotBind::Family(family)] = plan.binds;
    family
}

/// Build one complete generated value-free free slot.
fn free_slot(family: AdjustmentShadowFamily) -> AdjustmentShadowRegistrationSlot {
    AdjustmentShadowRegistrationSlot {
        contractVersion: AdjustmentShadowRegistrationSlotContractVersionLiteral,
        epochWitnessSha256: "a".repeat(64),
        family,
        horizonEndAt: TO.to_owned(),
        registrationSha256: Nullable::Null,
        scheduleContractSha256: "b".repeat(64),
        state: AdjustmentShadowRegistrationSlotStateEnum::Free,
        terminalAt: Nullable::Null,
    }
}

impl ApiReadPoolLease for FixtureDatabase {
    /// Record one explicit release without exposing a reusable fixture client.
    fn release(self, disposition: ApiReadSessionDisposition) {
        self.api_pool_events
            .expect("pooled fixture")
            .borrow_mut()
            .push(ApiPoolEvent::Release(disposition));
    }
}

impl EcmwfTemperatureCanaryReadPort for FixtureDatabase {
    type Error = FixturePoolError;

    /// Return one trusted run and capture the exact serving-delay statement.
    fn select_sidecar_run(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarRunReadPlan,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        self.sidecar_run_plan = Some(plan.clone());
        Ok(Some(stored_run()))
    }

    /// Return one admitted nullable hour and capture its selected-run statement.
    fn select_sidecar_hours(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarHoursReadPlan,
    ) -> Result<Vec<EcmwfTemperatureCanaryHourReadRow>, Self::Error> {
        self.sidecar_hours_plan = Some(plan.clone());
        Ok(vec![EcmwfTemperatureCanaryHourReadRow {
            content_hash: Sha256Digest::admitted("d".repeat(64)),
            model_lead_hours: 7,
            raw_relative_humidity_percent: Nullable::Null,
            raw_temperature_c: 10.5,
            raw_wind_speed_mps: Nullable::Value(3.5),
            valid_at: "2026-09-07T19:00:00.000Z".to_owned(),
        }])
    }

    /// Return one status row and capture its independent safety cutoff.
    fn select_status(
        &mut self,
        plan: &EcmwfTemperatureCanaryStatusReadPlan,
    ) -> Result<Option<EcmwfTemperatureCanaryStatusReadRow>, Self::Error> {
        self.status_plan = Some(plan.clone());
        Ok(Some(EcmwfTemperatureCanaryStatusReadRow {
            first_received_at: "2026-09-07T17:59:00.000Z".to_owned(),
            hour_count: 18,
            run_initialized_at: RUN_AT.to_owned(),
            state_reason: "no_causal_forecast_observation_pairs".to_owned(),
            state_status: EcmwfTemperatureCanaryRunStateStatusEnum::Cold,
        }))
    }
}

impl RainServingReadPort for FixtureDatabase {
    type Error = FixturePoolError;

    /// Return one retained adjustment run and capture its exact typed plan.
    fn select_rain_adjustment_run(
        &mut self,
        plan: &RainAdjustmentRunReadPlan,
    ) -> Result<Option<RainAdjustmentRunReadRow>, Self::Error> {
        self.rain_run_plan = Some(plan.clone());
        Ok(self.rain_run.clone())
    }

    /// Return one aggregate-only status and capture its zero-bind plan.
    fn select_rain_collection_status(
        &mut self,
        plan: &RainCollectionStatusReadPlan,
    ) -> Result<Option<RainCollectionStatusReadRow>, Self::Error> {
        self.rain_status_plan = Some(plan.clone());
        Ok(self.rain_status.clone())
    }
}

impl WeatherServicePostgresReadPort for FixtureDatabase {
    /// Return one raw active row or inject the service-visible database failure.
    fn query_active_sites(
        &mut self,
        plan: &ListActiveSitesQuery,
    ) -> Result<Vec<ActiveSiteDbRow>, Self::Error> {
        // record the exact typed statement passed through a checked-out client
        if let Some(events) = &self.api_pool_events {
            events
                .borrow_mut()
                .push(ApiPoolEvent::Query(plan.sql, plan.binds.len()));
        }
        self.active_plans.push(*plan);
        // expose the typed operation phase without manufacturing an empty result
        if let Some(error) = self.active_failure {
            return Err(error);
        }
        Ok(vec![active_site_row()])
    }

    /// Return decoded current rows for the exact supplied plan.
    fn query_current(
        &mut self,
        _plan: &CurrentWeatherQueryPlan,
    ) -> Result<Vec<CurrentWeatherRecordRow>, Self::Error> {
        Ok(Vec::new())
    }

    /// Return decoded history rows for the exact supplied plan.
    fn query_history(
        &mut self,
        _plan: &HistoryReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, Self::Error> {
        Ok(Vec::new())
    }

    /// Return decoded tide rows for the exact supplied plan.
    fn query_tides(&mut self, _plan: &TideReadPlan) -> Result<Vec<TideRecordRow>, Self::Error> {
        Ok(Vec::new())
    }

    /// Return decoded forecast rows for the exact supplied plan.
    fn query_forecast(
        &mut self,
        _plan: &ForecastReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, Self::Error> {
        Ok(Vec::new())
    }

    /// Return no optional provenance rows for the general API repository fixture.
    fn query_forecast_provenance(
        &mut self,
        _plan: &ForecastProvenancePlan,
    ) -> Result<Vec<ForecastRuntimeProvenanceRow>, Self::Error> {
        Ok(Vec::new())
    }

    /// Return decoded pressure rows for the exact supplied plan.
    fn query_forecast_pressure_context(
        &mut self,
        _plan: &ForecastPressureContextPlan,
    ) -> Result<Vec<WeatherRecordRow>, Self::Error> {
        Ok(Vec::new())
    }

    /// Return decoded trend rows for the exact supplied plan.
    fn query_trends(&mut self, _plan: &TrendReadPlan) -> Result<Vec<TrendPointRow>, Self::Error> {
        Ok(Vec::new())
    }

    /// Return decoded precipitation rows for the exact supplied plan.
    fn query_daily_precipitation(
        &mut self,
        _plan: &DailyPrecipitationReadPlan,
    ) -> Result<Vec<DailyPrecipitationRow>, Self::Error> {
        Ok(Vec::new())
    }

    /// Return decoded private projection rows for the exact supplied plan.
    fn query_forecast_observation_input(
        &mut self,
        _plan: &ForecastObservationInputReadPlan,
    ) -> Result<Vec<ForecastObservationInputDbRow>, Self::Error> {
        Ok(Vec::new())
    }

    /// Execute or fail the connectivity gate and record exact ordering.
    fn query_health_ping(&mut self, _plan: &HealthPingReadPlan) -> Result<(), Self::Error> {
        self.health_trace.push("ping");

        // retain cross-boundary readiness order for the fixture
        if let Some(order) = &self.health_order {
            order.borrow_mut().push("ping");
        }
        // stop the health sequence only at the connectivity gate
        if self.health_failure == Some("ping") {
            return Err(FixturePoolError::StatementOrClient);
        }
        Ok(())
    }

    /// Execute or fail migration history independently from heartbeat.
    fn query_health_migration_ledger(
        &mut self,
        _plan: &HealthMigrationLedgerReadPlan,
    ) -> Result<Vec<AppliedMigration>, Self::Error> {
        self.health_trace.push("migration");

        // retain cross-boundary readiness order for the fixture
        if let Some(order) = &self.health_order {
            order.borrow_mut().push("migration");
        }
        // isolate migration unavailability from the later heartbeat read
        if self.health_failure == Some("migration") {
            return Err(FixturePoolError::StatementOrClient);
        }
        Ok(self.health_applied.clone())
    }

    /// Execute or fail heartbeat independently from migration classification.
    fn query_health_latest_worker_heartbeat(
        &mut self,
        _plan: &HealthLatestWorkerHeartbeatReadPlan,
    ) -> Result<Option<HealthLatestWorkerHeartbeatRow>, Self::Error> {
        self.health_trace.push("heartbeat");

        // retain cross-boundary readiness order for the fixture
        if let Some(order) = &self.health_order {
            order.borrow_mut().push("heartbeat");
        }
        // collapse only this optional heartbeat when its query fails
        if self.health_failure == Some("heartbeat") {
            return Err(FixturePoolError::StatementOrClient);
        }
        Ok(self.heartbeat.clone())
    }
}

#[derive(Debug, Default)]
struct FixtureExternal {
    admitted_sidecars: Vec<ForecastTemperatureSidecarRead>,
    digested_temperature_states: Vec<EcmwfTemperatureCanaryRecentErrorState>,
    health_applied_history_sha256: Option<MigrationDigest>,
    health_artifacts: Option<AdmittedHealthMigrationArtifacts>,
    health_failure: Option<&'static str>,
    health_order: Option<Rc<RefCell<Vec<&'static str>>>>,
    health_worker_epoch: Option<i64>,
    health_worker_values: Vec<JsDateOrString>,
    rain_time_values: Vec<String>,
    temperature_digest: Option<Sha256Digest>,
}

impl WeatherServiceExternalReadPort for FixtureExternal {
    type Error = FixtureError;

    /// Supply epochs from the admitted shared time boundary.
    fn admit_temperature_sidecar_read(
        &mut self,
        read: ForecastTemperatureSidecarRead,
    ) -> Result<AdmittedEcmwfTemperatureCanarySidecarQuery, Self::Error> {
        self.admitted_sidecars.push(read.clone());
        Ok(AdmittedEcmwfTemperatureCanarySidecarQuery::admitted(
            read.as_of,
            read.from,
            1_788_782_400_000,
            read.site_slug,
            read.to,
            1_788_912_000_000,
        ))
    }

    /// Load admitted local migration artifacts before database history.
    fn admit_health_migration_artifacts(
        &mut self,
    ) -> Result<AdmittedHealthMigrationArtifacts, Self::Error> {
        // retain cross-boundary readiness order for the fixture
        if let Some(order) = &self.health_order {
            order.borrow_mut().push("artifacts");
        }

        // isolate local artifact failure from heartbeat collection
        if self.health_failure == Some("artifacts") {
            return Err(FixtureError);
        }
        self.health_artifacts.clone().ok_or(FixtureError)
    }

    /// Return one identity prepared by the admitted local file/hash owner.
    fn admit_health_migration_identity(
        &mut self,
        artifacts: AdmittedHealthMigrationArtifacts,
        _applied: &[AppliedMigration],
    ) -> Result<AdmittedHealthMigrationIdentity, Self::Error> {
        // retain cross-boundary readiness order for the fixture
        if let Some(order) = &self.health_order {
            order.borrow_mut().push("identity");
        }
        Ok(AdmittedHealthMigrationIdentity {
            applied_history_sha256: self
                .health_applied_history_sha256
                .clone()
                .ok_or(FixtureError)?,
            artifacts: artifacts.artifacts,
            authorization: artifacts.authorization,
            release: artifacts.release,
        })
    }

    /// Return one epoch prepared by the admitted shared time owner.
    fn admit_health_worker_last_loop_epoch(
        &mut self,
        value: &JsDateOrString,
    ) -> Result<i64, Self::Error> {
        self.health_worker_values.push(value.clone());
        self.health_worker_epoch.ok_or(FixtureError)
    }
}

impl TemperatureCanaryRecentErrorDigestPort for FixtureExternal {
    /// Hash the actual decoded generated state through the injected owner.
    fn digest_recent_error_state(
        &mut self,
        state: &EcmwfTemperatureCanaryRecentErrorState,
    ) -> Result<Sha256Digest, TaskFailure> {
        self.digested_temperature_states.push(state.clone());
        self.temperature_digest.clone().ok_or(TaskFailure {
            attempts: 1,
            code: "temperature_digest_unavailable".to_owned(),
        })
    }
}

impl RainRepositoryTimeBoundary for FixtureExternal {
    type Error = FixtureError;

    /// Admit the fixture's already-normalized canonical rain timestamps.
    fn admit_rain_instant(&mut self, value: &str) -> Result<CanonicalInstant, Self::Error> {
        self.rain_time_values.push(value.to_owned());
        Ok(instant(rain_epoch(value)?, value))
    }
}

struct FixtureClock;

impl WeatherServiceClock for FixtureClock {
    /// Return one canonical service request clock.
    fn request_time(&mut self) -> Result<CanonicalInstant, ServiceClockError> {
        Ok(instant(1_788_804_180_000, SIDECAR_AS_OF))
    }
}

struct UnusedBoundary;

impl CurrentTimeResolver for UnusedBoundary {
    /// Reject repository timestamps because the sites fixture never resolves one.
    fn resolve(&self, _value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        Err(boundary_error())
    }
}

impl HistoryBoundaryAdapter for UnusedBoundary {
    /// Reject submitted history time because the sites fixture never parses one.
    fn resolve_query_time(
        &self,
        _value: &str,
        _field: HistoryQueryTime,
    ) -> Result<CanonicalInstant, HistoryBoundaryError> {
        Err(HistoryBoundaryError)
    }

    /// Reject submitted history cursors because the sites fixture never parses one.
    fn decode_cursor(&self, _value: &str) -> Result<AdmittedHistoryCursor, HistoryBoundaryError> {
        Err(HistoryBoundaryError)
    }

    /// Reject cursor encoding because the sites fixture never projects history.
    fn encode_cursor(
        &self,
        _cursor: &HistoryQueryCursor,
    ) -> Result<String, CurrentProjectionError> {
        Err(boundary_error())
    }
}

impl TideBoundaryAdapter for UnusedBoundary {
    /// Reject submitted tide bounds because the sites fixture never parses them.
    fn resolve_query_time(
        &self,
        _value: &str,
        _field: TideQueryTime,
    ) -> Result<CanonicalInstant, TideBoundaryError> {
        Err(TideBoundaryError)
    }

    /// Reject default tide offsets because the sites fixture never resolves them.
    fn offset_data_time(
        &self,
        _data_at: &CanonicalInstant,
        _offset_milliseconds: i64,
        _field: TideQueryTime,
    ) -> Result<CanonicalInstant, TideBoundaryError> {
        Err(TideBoundaryError)
    }
}

impl ForecastServiceBoundary for UnusedBoundary {
    /// Reject forecast windows because the sites fixture never resolves them.
    fn resolve_window(
        &self,
        _selection: ForecastSelection,
        _data_at: &CanonicalInstant,
        _timezone: &str,
    ) -> Result<ResolvedServiceForecastWindow, CurrentProjectionError> {
        Err(boundary_error())
    }
}

impl DailyPrecipitationServiceBoundary for UnusedBoundary {
    /// Reject local-day windows because the sites fixture never resolves them.
    fn resolve_window(
        &self,
        _data_at: &CanonicalInstant,
        _timezone: &str,
    ) -> Result<ResolvedDailyPrecipitationWindow, CurrentProjectionError> {
        Err(boundary_error())
    }
}

impl TrendTimeBoundary for UnusedBoundary {
    /// Reject calendar starts because the sites fixture never resolves them.
    fn calendar_start(
        &self,
        _generated_at: &CanonicalInstant,
        _timezone: &str,
    ) -> Result<CanonicalInstant, CurrentProjectionError> {
        Err(boundary_error())
    }

    /// Reject trend dates because the sites fixture never resolves them.
    fn resolve_row_time(
        &self,
        _value: &JsDate,
    ) -> Result<CanonicalInstant, CurrentProjectionError> {
        Err(boundary_error())
    }
}

impl RainCollectionTimestampBoundary for UnusedBoundary {
    /// Reject collection timestamps because the sites fixture never resolves them.
    fn validate_canonical_utc(&self, _value: &str) -> Result<(), CurrentProjectionError> {
        Err(boundary_error())
    }
}

impl ForecastObservationGeometryBoundary for UnusedBoundary {
    /// Reject private geometry because the public sites fixture never resolves it.
    fn resolve_expected_geometry(
        &self,
        _data_at: &CanonicalInstant,
        _timezone: &str,
    ) -> Result<ResolvedForecastObservationGeometry, CurrentProjectionError> {
        Err(boundary_error())
    }
}

/// Serve a real API discovery request from raw decoded PostgreSQL rows and exact SQL.
#[test]
fn serves_sites_through_concrete_postgres_repository() {
    let mut repository = PostgresWeatherServiceRepository::new(
        FixtureDatabase::default(),
        FixtureExternal::default(),
    );
    let response = sites_request(&mut repository);

    assert_eq!(response.status, 200);
    let Some(WeatherServiceBody::Sites(body)) = response.body else {
        panic!("sites response body");
    };
    assert_eq!(body.data.len(), 1);
    assert_eq!(body.data[0].slug, "ballydidean");
    assert_eq!(body.data[0].stations[0].sources[0].id, "source-1");
    assert_eq!(repository.database().active_plans.len(), 1);
    assert_eq!(
        repository.database().active_plans[0].sql,
        LIST_ACTIVE_SITES_SQL
    );
    assert!(repository.database().active_plans[0].binds.is_empty());
}

/// Map a typed PostgreSQL failure to the API's actual internal-error response.
#[test]
fn maps_postgres_failure_to_typed_service_error() {
    let mut repository = PostgresWeatherServiceRepository::new(
        FixtureDatabase {
            active_failure: Some(FixturePoolError::StatementOrClient),
            ..FixtureDatabase::default()
        },
        FixtureExternal::default(),
    );
    let response = sites_request(&mut repository);

    assert_eq!(response.status, 500);
    let Some(WeatherServiceBody::Error(body)) = response.body else {
        panic!("error response body");
    };
    assert_eq!(body.error.code, "internal_error");
}

/// Preserve the sites contract while classifying every pool failure phase.
#[test]
fn sites_route_releases_each_typed_failure_phase_and_recovers() {
    use ApiPoolEvent::{Checkout, Query, Release};
    use ApiReadSessionDisposition::{Discard, Recycle};

    let events = Rc::new(RefCell::new(Vec::new()));
    let mut connector = FixtureApiPoolConnector {
        events: Rc::clone(&events),
        settings: None,
    };
    let mut repository = prepare_pooled_weather_service_repository(
        &mut connector,
        &DatabaseConfiguration {
            application_name: "weather-api".to_owned(),
            database: "fixture_database".to_owned(),
            host: "fixture_database_host".to_owned(),
            lock_timeout_ms: 2_000,
            password: "fixture-not-a-secret".to_owned(),
            port: 5432,
            ssl: true,
            statement_timeout_ms: 6_000,
            user: "fixture_api".to_owned(),
        },
        FixtureExternal::default(),
    )
    .expect("prepare the retained API repository");
    let settings = connector
        .settings
        .expect("typed database pool configuration");
    assert_eq!(settings.application_name, "weather-api");
    assert_eq!(settings.user, "fixture_api");
    assert_eq!(settings.max, 10);
    assert_eq!(settings.statement_timeout, 6_000);
    assert!(settings.ssl);
    assert!(events.borrow().is_empty());

    // exercise success, statement failure, decode failure, checkout failure and recovery
    for expected in [200, 500, 500, 500, 200] {
        let response = sites_request(&mut repository);
        assert_eq!(response.status, expected);
        match (expected, response.body) {
            (200, Some(WeatherServiceBody::Sites(body))) => {
                assert_eq!(body.data[0].slug, "ballydidean");
                assert_eq!(body.data[0].stations[0].sources[0].id, "source-1");
            }
            (500, Some(WeatherServiceBody::Error(body))) => {
                assert_eq!(body.error.code, "internal_error");
            }
            _ => panic!("expected the original sites or error contract"),
        }
    }

    assert_eq!(
        events.borrow().as_slice(),
        [
            Checkout,
            Query(LIST_ACTIVE_SITES_SQL, 0),
            Release(Recycle),
            Checkout,
            Query(LIST_ACTIVE_SITES_SQL, 0),
            Release(Discard),
            Checkout,
            Query(LIST_ACTIVE_SITES_SQL, 0),
            Release(Recycle),
            Checkout,
            Checkout,
            Query(LIST_ACTIVE_SITES_SQL, 0),
            Release(Recycle),
        ]
    );
    let (reads, _) = repository.into_parts();
    assert!(reads.into_pool().attempts.borrow().is_empty());
}

/// Use one API pool for the private schedule, private write and subsequent public read.
#[test]
fn public_and_private_repositories_share_one_pool() {
    use AdjustmentShadowFamily::{Rain, Temperature, Wind};
    use ApiPoolEvent::{Checkout, Query, Release, ScheduleCohort};
    use ApiReadSessionDisposition::{Discard, Recycle};

    let events = Rc::new(RefCell::new(Vec::new()));
    let mut repository = PostgresWeatherServiceRepository::new(
        PooledWeatherApiReads::new(FixtureApiReadPool {
            events: Rc::clone(&events),
            attempts: Rc::new(RefCell::new(VecDeque::from([
                FixtureCheckout::StatementFailure,
                FixtureCheckout::PostQueryDecodeFailure,
                FixtureCheckout::Ready,
                FixtureCheckout::Ready,
            ]))),
        }),
        FixtureExternal::default(),
    );
    let key = AdjustmentRevisionAdmissionKey::ForecastAnchor(
        AdjustmentForecastAnchorRevisionAdmissionKey {
            leadHours: 6,
            sourceId: "7".to_owned(),
            storedContentSha256: "c".repeat(64),
            validAt: FROM.to_owned(),
        },
    );
    let gap = AdjustmentRevisionGapInput {
        logicalKeySha256: "d".repeat(64),
        projectionIdentitySha256: Nullable::Null,
        projectionKind: AdjustmentRevisionProjectionKind::ActualBestMatch,
        projectionSha256: Nullable::Null,
        reason: AdjustmentRevisionGapReason::ArchiveStageFailed,
    };

    {
        let mut maintenance = repository.maintenance_repository();
        let slots = maintenance
            .schedule_slots(maintenance_schedule_slot_reads())
            .expect("three admitted slots");
        assert_eq!(slots.map(|slot| slot.family), [Temperature, Wind, Rain]);
        assert_eq!(
            maintenance.mark_revision_gap(&key, &gap),
            Err(AdjustmentMaintenanceOperationError::Rejected)
        );
        assert_eq!(
            maintenance.mark_revision_gap(&key, &gap),
            Err(AdjustmentMaintenanceOperationError::Rejected)
        );
        maintenance
            .mark_revision_gap(&key, &gap)
            .expect("committed gap marker");
    }

    assert_eq!(sites_request(&mut repository).status, 200);
    let gap_sql = "SELECT weather_mark_adjustment_revision_gap_v1(\n      $1::text, $2::jsonb, $3::jsonb\n    ) AS value";
    assert_eq!(
        events.borrow().as_slice(),
        [
            ScheduleCohort([Temperature, Wind, Rain]),
            Checkout,
            Query(gap_sql, 3),
            Release(Discard),
            Checkout,
            Query(gap_sql, 3),
            Release(Recycle),
            Checkout,
            Query(gap_sql, 3),
            Release(Recycle),
            Checkout,
            Query(LIST_ACTIVE_SITES_SQL, 0),
            Release(Recycle),
        ]
    );
    let (reads, _) = repository.into_parts();
    assert!(reads.into_pool().attempts.borrow().is_empty());
}

/// Execute sidecar and status through exact typed plans while retaining the state digest.
#[test]
fn executes_temperature_reads_with_independent_cutoffs() {
    let mut repository = PostgresWeatherServiceRepository::new(
        FixtureDatabase::default(),
        FixtureExternal {
            temperature_digest: Some(Sha256Digest::admitted(temperature_digest_node_oracle())),
            ..FixtureExternal::default()
        },
    );
    let material = repository
        .temperature_canary_sidecar(ForecastTemperatureSidecarRead {
            site_slug: "ballydidean".to_owned(),
            as_of: SIDECAR_AS_OF.to_owned(),
            from: FROM.to_owned(),
            to: TO.to_owned(),
        })
        .expect("sidecar query")
        .expect("sidecar material");
    let projected_status = repository
        .temperature_canary_status(ForecastTemperatureStatusRead {
            site_slug: "ballydidean".to_owned(),
            as_of: STATUS_AS_OF.to_owned(),
        })
        .expect("status query")
        .expect("status row");

    assert_eq!(material.sidecar.run.id, "run-1");
    assert_eq!(
        material.sidecar.hours[0].rawRelativeHumidityPercent,
        Nullable::Null
    );
    assert_eq!(
        material.recent_error_state_sha256,
        RECENT_ERROR_STATE_SHA256
    );
    assert_eq!(
        projected_status.stateStatus,
        EcmwfTemperatureCanaryStatusStateStatusEnum::Cold
    );
    let run = repository
        .database()
        .sidecar_run_plan
        .as_ref()
        .expect("run plan");
    let hours = repository
        .database()
        .sidecar_hours_plan
        .as_ref()
        .expect("hours plan");
    let status = repository
        .database()
        .status_plan
        .as_ref()
        .expect("status plan");
    assert_eq!(run.sql, ECMWF_TEMPERATURE_CANARY_SIDECAR_RUN_SQL);
    assert_eq!(hours.sql, ECMWF_TEMPERATURE_CANARY_SIDECAR_HOURS_SQL);
    assert_eq!(status.sql, ECMWF_TEMPERATURE_CANARY_STATUS_SQL);
    assert_eq!(
        run.binds,
        [
            EcmwfTemperatureCanaryReadBind::Text("ballydidean".to_owned()),
            EcmwfTemperatureCanaryReadBind::UtcInstant(SIDECAR_AS_OF.to_owned()),
        ]
    );
    assert_eq!(
        status.binds,
        [
            EcmwfTemperatureCanaryReadBind::Text("ballydidean".to_owned()),
            EcmwfTemperatureCanaryReadBind::UtcInstant(STATUS_AS_OF.to_owned()),
        ]
    );
    assert_eq!(repository.external().admitted_sidecars.len(), 1);
    assert_eq!(
        repository.external().digested_temperature_states,
        [cold_state()]
    );
}

/// Fail raw before hour selection when stored state and canonical digest drift.
#[test]
fn rejects_temperature_state_digest_mismatch_before_hours() {
    let mut repository = PostgresWeatherServiceRepository::new(
        FixtureDatabase::default(),
        FixtureExternal {
            temperature_digest: Some(Sha256Digest::admitted("e".repeat(64))),
            ..FixtureExternal::default()
        },
    );

    let result = repository.temperature_canary_sidecar(ForecastTemperatureSidecarRead {
        site_slug: "ballydidean".to_owned(),
        as_of: SIDECAR_AS_OF.to_owned(),
        from: FROM.to_owned(),
        to: TO.to_owned(),
    });

    assert_eq!(result, Err(weather_api::ServiceRepositoryError));
    assert!(repository.database().sidecar_hours_plan.is_none());
    assert_eq!(
        repository.external().digested_temperature_states,
        [cold_state()]
    );
}

/// Match compiled Node ping gating and independent migration and heartbeat fallbacks.
#[test]
fn matches_compiled_node_database_health_flow() {
    let oracle = health_node_oracle();
    assert_eq!(
        health_order_node_oracle(),
        "health-order=ping,artifacts,ledger,heartbeat"
    );
    let heartbeat_epoch = oracle["heartbeat-epoch"]
        .parse::<i64>()
        .expect("heartbeat epoch");

    let mut current = health_repository(&oracle, None);
    let current_snapshot = current.health().expect("current health snapshot");
    assert_eq!(current_snapshot.database, DatabaseStatus::Ready);
    assert_eq!(current_snapshot.migration_status, MigrationStatus::Current);
    assert_eq!(
        current_snapshot.migration_version.as_deref(),
        Some("0023_forecast_product_read_index.sql")
    );
    assert_eq!(
        current_snapshot.worker_last_loop_epoch_ms,
        Some(heartbeat_epoch)
    );
    assert_eq!(
        current.database().health_trace.join(","),
        oracle["current-trace"]
    );
    assert_eq!(current.external().health_worker_values.len(), 1);
    assert_eq!(
        current
            .external()
            .health_order
            .as_ref()
            .expect("shared health order")
            .borrow()
            .as_slice(),
        ["ping", "artifacts", "migration", "identity", "heartbeat"]
    );

    let mut artifact_failure = health_repository(&oracle, Some("artifacts"));
    let artifact_snapshot = artifact_failure
        .health()
        .expect("artifact fallback snapshot");
    assert_eq!(artifact_snapshot.database, DatabaseStatus::Ready);
    assert_eq!(
        artifact_snapshot.migration_status,
        MigrationStatus::Outdated
    );
    assert_eq!(artifact_snapshot.migration_version, None);
    assert_eq!(
        artifact_snapshot.worker_last_loop_epoch_ms,
        Some(heartbeat_epoch)
    );
    assert_eq!(
        artifact_failure.database().health_trace,
        ["ping", "heartbeat"]
    );
    assert_eq!(
        artifact_failure
            .external()
            .health_order
            .as_ref()
            .expect("shared health order")
            .borrow()
            .as_slice(),
        ["ping", "artifacts", "heartbeat"]
    );

    let mut migration_failure = health_repository(&oracle, Some("migration"));
    let migration_snapshot = migration_failure
        .health()
        .expect("migration fallback snapshot");
    assert_eq!(migration_snapshot.database, DatabaseStatus::Ready);
    assert_eq!(
        migration_snapshot.migration_status,
        MigrationStatus::Outdated
    );
    assert_eq!(migration_snapshot.migration_version, None);
    assert_eq!(
        migration_snapshot.worker_last_loop_epoch_ms,
        Some(heartbeat_epoch)
    );
    assert_eq!(
        migration_failure.database().health_trace.join(","),
        oracle["migration-failure-trace"]
    );

    let mut heartbeat_failure = health_repository(&oracle, Some("heartbeat"));
    let heartbeat_snapshot = heartbeat_failure
        .health()
        .expect("heartbeat fallback snapshot");
    assert_eq!(heartbeat_snapshot.database, DatabaseStatus::Ready);
    assert_eq!(
        heartbeat_snapshot.migration_status,
        MigrationStatus::Current
    );
    assert_eq!(heartbeat_snapshot.worker_last_loop_epoch_ms, None);
    assert_eq!(
        heartbeat_failure.database().health_trace.join(","),
        oracle["heartbeat-failure-trace"]
    );

    let heartbeat_invalid = health_repository(&oracle, None);
    let (database, mut external) = heartbeat_invalid.into_parts();
    external.health_worker_epoch = None;
    let mut heartbeat_invalid = PostgresWeatherServiceRepository::new(database, external);
    let invalid_snapshot = heartbeat_invalid
        .health()
        .expect("canonical heartbeat fallback snapshot");
    assert_eq!(invalid_snapshot.database, DatabaseStatus::Ready);
    assert_eq!(invalid_snapshot.migration_status, MigrationStatus::Current);
    assert_eq!(invalid_snapshot.worker_last_loop_epoch_ms, None);
    assert_eq!(
        heartbeat_invalid.database().health_trace.join(","),
        oracle["heartbeat-invalid-trace"]
    );

    let mut ping_failure = health_repository(&oracle, Some("ping"));
    let ping_snapshot = ping_failure.health().expect("unavailable snapshot");
    assert_eq!(ping_snapshot.database, DatabaseStatus::Unavailable);
    assert_eq!(ping_snapshot.migration_status, MigrationStatus::Unavailable);
    assert_eq!(ping_snapshot.migration_version, None);
    assert_eq!(ping_snapshot.worker_last_loop_epoch_ms, None);
    assert_eq!(
        ping_failure.database().health_trace.join(","),
        oracle["ping-failure-trace"]
    );

    assert_eq!(
        oracle["current"],
        "ready|current|0023_forecast_product_read_index.sql|2026-09-07T18:02:00.000Z"
    );
    assert_eq!(
        oracle["migration-failure"],
        "ready|outdated|null|2026-09-07T18:02:00.000Z"
    );
    assert_eq!(
        oracle["heartbeat-failure"],
        "ready|current|0023_forecast_product_read_index.sql|null"
    );
    assert_eq!(
        oracle["heartbeat-invalid"],
        "ready|current|0023_forecast_product_read_index.sql|null"
    );
    assert_eq!(oracle["ping-failure"], "unavailable|unavailable|null|null");
}

/// Execute both serving DTO reads through the typed database-backed repository.
#[test]
fn executes_typed_rain_repository_reads() {
    let database = FixtureDatabase {
        rain_run: Some(rain_run_row()),
        rain_status: Some(rain_status_row()),
        ..FixtureDatabase::default()
    };
    let mut repository =
        PostgresWeatherServiceRepository::new(database, FixtureExternal::default());

    let run = repository
        .rain_adjustment_run(ForecastRainAdjustmentRead {
            site_slug: "ballydidean".to_owned(),
            as_of: RAIN_AS_OF.to_owned(),
        })
        .expect("rain run read")
        .expect("rain run");
    let status = repository
        .rain_collection_status()
        .expect("rain status read")
        .expect("rain status");

    assert_eq!(
        run.run_initialized_at,
        instant(1_788_804_000_000, RAIN_RUN_AT)
    );
    assert_eq!(run.model_sha256, "a".repeat(64));
    assert_eq!(run.hours.len(), 2);
    assert_eq!(run.hours[0].reason_code, None);
    assert_eq!(run.hours[1].reason_code.as_deref(), Some("hurdle_active"));
    assert_eq!(status.claims, 14);
    assert_eq!(status.lastStationReceiptAt, Nullable::Null);
    assert_eq!(
        repository
            .database()
            .rain_run_plan
            .as_ref()
            .expect("run plan")
            .binds,
        [RainServingReadBind::UtcInstant(RAIN_AS_OF.to_owned())]
    );
    assert_eq!(
        repository
            .database()
            .rain_status_plan
            .as_ref()
            .expect("status plan")
            .binds,
        []
    );
    assert_eq!(
        repository.external().rain_time_values,
        [
            RAIN_AS_OF,
            RAIN_RUN_AT,
            SIDECAR_AS_OF,
            "2026-09-08T02:00:00.000Z",
            "2026-09-08T02:05:00.000Z",
            "2026-09-08T01:00:00.000Z",
            "2026-09-08T02:00:00.000Z",
        ]
    );
}

/// Execute one public sites request with every unused boundary explicitly inert.
fn sites_request(
    repository: &mut dyn WeatherServiceRepository,
) -> weather_api::WeatherServiceResponse {
    let boundary = UnusedBoundary;
    let inactive = InactiveForecastServiceRuntime::unconfigured(SIDECAR_AS_OF, SIDECAR_AS_OF);
    serve_decoded_request(
        repository,
        &mut FixtureClock,
        &WeatherServiceBoundaries {
            nominal_data_at: None,
            time_resolver: &boundary,
            history: &boundary,
            tides: &boundary,
            forecast: &boundary,
            forecast_runtime: &inactive,
            forecast_wind_runtime: None,
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: &boundary,
            daily_precipitation: &boundary,
            rain_collection: &boundary,
            forecast_observation: &boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/sites",
            query: &[] as &[QueryParameter<'_>],
        },
    )
}

/// Build one raw decoded discovery row before database enum projection.
fn active_site_row() -> ActiveSiteDbRow {
    ActiveSiteDbRow {
        attribution_label: "Weather data by Open-Meteo".to_owned(),
        attribution_url: "https://open-meteo.com/".to_owned(),
        latitude: 47.95,
        longitude: -122.42,
        provider_key: "open-meteo".to_owned(),
        provider_name: "Open-Meteo".to_owned(),
        site_name: "Ballydidean".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_id: "source-1".to_owned(),
        source_key: "open-meteo-current".to_owned(),
        source_kind: "model_current".to_owned(),
        station_kind: "virtual".to_owned(),
        station_latitude: 47.95,
        station_longitude: -122.42,
        station_name: "Open-Meteo Virtual".to_owned(),
        station_slug: "open-meteo-virtual".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// Build one trusted selected run with its serving material digest.
fn stored_run() -> StoredEcmwfCanaryRun {
    StoredEcmwfCanaryRun {
        adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
        content_hash: Sha256Digest::admitted("b".repeat(64)),
        first_received_at: "2026-09-07T17:59:00.000Z".to_owned(),
        id: "run-1".to_owned(),
        model_cycle: EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1,
        provider_response_sha256: Sha256Digest::admitted("a".repeat(64)),
        recent_error_state: cold_state(),
        recent_error_state_sha256: Sha256Digest::admitted(RECENT_ERROR_STATE_SHA256),
        run_initialized_at: RUN_AT.to_owned(),
        state_reason: "no_causal_forecast_observation_pairs".to_owned(),
        state_status: EcmwfTemperatureCanaryRunStateStatusEnum::Cold,
        upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    }
}

/// Build one retained rain run decoded by the typed database port.
fn rain_run_row() -> RainAdjustmentRunReadRow {
    RainAdjustmentRunReadRow {
        decision_at: AdmittedRevisionUtcInstant::admitted("2026-09-08T02:00:00.000Z"),
        first_received_at: AdmittedRevisionUtcInstant::admitted(SIDECAR_AS_OF),
        forecast_claim_id: "claim-1".to_owned(),
        generated_at: AdmittedRevisionUtcInstant::admitted("2026-09-08T02:05:00.000Z"),
        hours: vec![
            RainAdjustmentHour {
                validAt: "2026-09-08T01:00:00.000Z".to_owned(),
                modelLeadHours: 7,
                rawPrecipitationMm: 0.0,
                correctedPrecipitationMm: 0.0,
                applied: false,
                reasonCode: Nullable::Null,
            },
            RainAdjustmentHour {
                validAt: "2026-09-08T02:00:00.000Z".to_owned(),
                modelLeadHours: 8,
                rawPrecipitationMm: 1.5,
                correctedPrecipitationMm: 1.25,
                applied: true,
                reasonCode: Nullable::Value("hurdle_active".to_owned()),
            },
        ],
        input_sha256: Sha256Digest::admitted("b".repeat(64)),
        model_sha256: Sha256Digest::admitted("a".repeat(64)),
        run_initialized_at: AdmittedRevisionUtcInstant::admitted(RAIN_RUN_AT),
    }
}

/// Build one sanitized aggregate-only rain collection status row.
fn rain_status_row() -> RainCollectionStatusReadRow {
    RainCollectionStatusReadRow {
        claims: 14,
        compressed_bytes: 4096,
        contract_version: "rain-prospective-capture/v1".to_owned(),
        failed_requests: 2,
        last_claim_at: Some(AdmittedRevisionUtcInstant::admitted(
            "2026-09-08T05:00:00.000Z",
        )),
        last_forecast_receipt_at: Some(AdmittedRevisionUtcInstant::admitted(
            "2026-09-08T04:00:00.000Z",
        )),
        last_receipt_at: Some(AdmittedRevisionUtcInstant::admitted(
            "2026-09-08T05:05:00.000Z",
        )),
        last_station_receipt_at: None,
        model_enabled: false,
        paused_until: None,
        pending_requests: 1,
        qualification_enabled: false,
        receipts: 12,
        stations_seen: 9,
        timely_forecasts: 3,
        unknown_requests: 1,
        valid_forecasts: 4,
        valid_station_windows: 5,
    }
}

/// Build the cold state selected by the compiled Node serving fixture.
fn cold_state() -> EcmwfTemperatureCanaryRecentErrorState {
    EcmwfTemperatureCanaryRecentErrorState {
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
        targetRunInitializedAt: RUN_AT.to_owned(),
        windowEndValidAt: "2026-09-07T05:00:00.000Z".to_owned(),
    }
}

/// Build one canonical request instant.
fn instant(epoch_milliseconds: i64, utc: &str) -> CanonicalInstant {
    CanonicalInstant {
        epoch_milliseconds,
        utc: utc.to_owned(),
    }
}

/// Return admitted epoch evidence for the fixed rain repository fixture.
fn rain_epoch(value: &str) -> Result<i64, FixtureError> {
    match value {
        RAIN_AS_OF => Ok(1_788_847_200_000),
        RAIN_RUN_AT => Ok(1_788_804_000_000),
        SIDECAR_AS_OF => Ok(1_788_804_180_000),
        "2026-09-08T01:00:00.000Z" => Ok(1_788_829_200_000),
        "2026-09-08T02:00:00.000Z" => Ok(1_788_832_800_000),
        "2026-09-08T02:05:00.000Z" => Ok(1_788_833_100_000),
        _ => Err(FixtureError),
    }
}

/// Build one explicit unused-boundary error.
fn boundary_error() -> CurrentProjectionError {
    CurrentProjectionError {
        code: "fixture.unused",
        detail: "unused fixture boundary".to_owned(),
    }
}

/// Invoke the compiled Node API health store over fake no-socket queries.
fn health_node_oracle() -> BTreeMap<String, String> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../weather-database/tests/health-reads-node-oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .output()
        .expect("Node health oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Node health oracle must emit UTF-8")
        .lines()
        .map(|line| {
            let (key, value) = line.split_once('=').expect("oracle field");
            (key.to_owned(), value.to_owned())
        })
        .collect()
}

/// Verify compiled API and migration readiness ordering without opening a socket.
fn health_order_node_oracle() -> String {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/api-repository-health-order-node-oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .output()
        .expect("Node health ordering oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Node health ordering oracle must emit UTF-8")
        .trim()
        .to_owned()
}

/// Hash the exact generated cold state through the ordinary Node digest owner.
fn temperature_digest_node_oracle() -> String {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/api-repository-temperature-digest-node-oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .output()
        .expect("Node temperature digest oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let digest = String::from_utf8(output.stdout)
        .expect("Node temperature digest oracle must emit UTF-8")
        .trim()
        .to_owned();
    assert_eq!(digest, RECENT_ERROR_STATE_SHA256);
    digest
}

/// Build one health repository from the production local artifact identities.
fn health_repository(
    oracle: &BTreeMap<String, String>,
    failure: Option<&'static str>,
) -> PostgresWeatherServiceRepository<FixtureDatabase, FixtureExternal> {
    let rows = oracle["ledger"]
        .split(',')
        .map(|entry| {
            let (name, checksum) = entry.split_once(':').expect("ledger row");
            (name.to_owned(), MigrationDigest::admitted(checksum))
        })
        .collect::<Vec<_>>();
    let artifacts = rows
        .iter()
        .map(|(name, digest)| MigrationScript {
            name: name.clone(),
            digest: digest.clone(),
            sql: String::new(),
        })
        .collect();
    let applied = rows
        .into_iter()
        .map(|(name, digest)| AppliedMigration { name, digest })
        .collect::<Vec<_>>();
    let artifacts = AdmittedHealthMigrationArtifacts {
        artifacts,
        authorization: None,
        release: "development".to_owned(),
    };
    let health_order = Rc::new(RefCell::new(Vec::new()));
    let database_failure = failure.filter(|value| *value != "artifacts");

    PostgresWeatherServiceRepository::new(
        FixtureDatabase {
            health_applied: applied,
            health_failure: database_failure,
            health_order: Some(Rc::clone(&health_order)),
            heartbeat: Some(HealthLatestWorkerHeartbeatRow {
                last_loop_at: JsDateOrString::String(
                    "2026-09-07T18:02:00.000Z".encode_utf16().collect(),
                ),
            }),
            ..FixtureDatabase::default()
        },
        FixtureExternal {
            health_applied_history_sha256: Some(MigrationDigest::admitted(
                &oracle["history-sha256"],
            )),
            health_artifacts: Some(artifacts),
            health_failure: failure,
            health_order: Some(health_order),
            health_worker_epoch: Some(
                oracle["heartbeat-epoch"]
                    .parse::<i64>()
                    .expect("heartbeat epoch"),
            ),
            ..FixtureExternal::default()
        },
    )
}
