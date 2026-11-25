use weather_api::{
    AdmittedRainAdjustmentRun, AdmittedTemperatureCanarySidecar, ForecastRainAdjustmentRead,
    ForecastTemperatureSidecarRead, HealthSnapshot, MigrationStatus, ServiceRepositoryError,
    WeatherServiceRepository,
};
use weather_contracts::{
    CurrentWeatherRecordRow, DailyPrecipitationRow, EcmwfTemperatureCanaryStatus,
    ForecastObservationInput, JsDateOrString, RainCollectionStatus, TideRecordRow, TrendPointRow,
    WeatherRecordRow,
};
use weather_database::{
    ActiveSiteDbRow, ActiveSiteRow, AdmittedEcmwfTemperatureCanarySidecarQuery,
    AdmittedEcmwfTemperatureCanaryStatusQuery, AdmittedHealthMigrationAuthorization,
    AdmittedHealthMigrationIdentity, AppliedMigration, CurrentWeatherQueryPlan,
    DailyPrecipitationReadPlan, EcmwfTemperatureCanaryHourReadRow, EcmwfTemperatureCanaryReadPort,
    EcmwfTemperatureCanarySidecarHoursReadPlan, EcmwfTemperatureCanarySidecarRunReadPlan,
    EcmwfTemperatureCanaryStatusReadPlan, EcmwfTemperatureCanaryStatusReadRow,
    ForecastObservationInputDbRow, ForecastObservationInputReadPlan, ForecastPressureContextPlan,
    ForecastProvenancePlan, ForecastReadPlan, ForecastRuntimeProvenanceRow,
    HealthLatestWorkerHeartbeatReadPlan, HealthLatestWorkerHeartbeatRow,
    HealthMigrationLedgerReadPlan, HealthPingReadPlan, HistoryReadPlan, ListActiveSitesQuery,
    MigrationScript, RainServingReadPort, StoredEcmwfCanaryRun, TideReadPlan, TrendReadPlan,
    get_ecmwf_temperature_canary_sidecar, get_ecmwf_temperature_canary_status,
    health_latest_worker_heartbeat_read_plan, health_migration_ledger_read_plan,
    health_ping_read_plan, project_active_site_rows, project_current_weather_rows,
    project_daily_precipitation_row, project_forecast_observation_input_rows,
    project_forecast_pressure_context_rows, project_tide_record_rows, project_trend_point_rows,
    project_weather_history_rows, verify_health_migration_readiness,
};

use crate::api_forecast_repository::read_api_forecast;
use crate::{
    RainRepositoryTimeBoundary, TemperatureCanaryRecentErrorDigestPort,
    read_admitted_rain_adjustment_run, read_admitted_rain_collection_status,
};

/// Injected typed PostgreSQL executor for the exact database-owned read plans.
pub trait WeatherServicePostgresReadPort: EcmwfTemperatureCanaryReadPort {
    /// Execute active-site discovery and decode raw scalar rows.
    fn query_active_sites(
        &mut self,
        plan: &ListActiveSitesQuery,
    ) -> Result<Vec<ActiveSiteDbRow>, <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the current-weather statement and decode generated rows.
    fn query_current(
        &mut self,
        plan: &CurrentWeatherQueryPlan,
    ) -> Result<Vec<CurrentWeatherRecordRow>, <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the dynamic history statement in its database order.
    fn query_history(
        &mut self,
        plan: &HistoryReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the bounded tide statement in ascending validity order.
    fn query_tides(
        &mut self,
        plan: &TideReadPlan,
    ) -> Result<Vec<TideRecordRow>, <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the raw forecast statement without applying serving policy.
    fn query_forecast(
        &mut self,
        plan: &ForecastReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the exact-record provenance statement whose result is optional for serving.
    fn query_forecast_provenance(
        &mut self,
        plan: &ForecastProvenancePlan,
    ) -> Result<Vec<ForecastRuntimeProvenanceRow>, <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the optional complete-vintage pressure statement.
    fn query_forecast_pressure_context(
        &mut self,
        plan: &ForecastPressureContextPlan,
    ) -> Result<Vec<WeatherRecordRow>, <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the ordered daily-trend statement.
    fn query_trends(
        &mut self,
        plan: &TrendReadPlan,
    ) -> Result<Vec<TrendPointRow>, <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the nearest-gauge precipitation statement.
    fn query_daily_precipitation(
        &mut self,
        plan: &DailyPrecipitationReadPlan,
    ) -> Result<Vec<DailyPrecipitationRow>, <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the fixed private observation-input statement.
    fn query_forecast_observation_input(
        &mut self,
        plan: &ForecastObservationInputReadPlan,
    ) -> Result<Vec<ForecastObservationInputDbRow>, <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the connectivity probe that gates all remaining health reads.
    fn query_health_ping(
        &mut self,
        plan: &HealthPingReadPlan,
    ) -> Result<(), <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the exact ordered migration-ledger read.
    fn query_health_migration_ledger(
        &mut self,
        plan: &HealthMigrationLedgerReadPlan,
    ) -> Result<Vec<AppliedMigration>, <Self as EcmwfTemperatureCanaryReadPort>::Error>;

    /// Execute the exact latest-worker-heartbeat read.
    fn query_health_latest_worker_heartbeat(
        &mut self,
        plan: &HealthLatestWorkerHeartbeatReadPlan,
    ) -> Result<
        Option<HealthLatestWorkerHeartbeatRow>,
        <Self as EcmwfTemperatureCanaryReadPort>::Error,
    >;
}

/// Local migration artifacts admitted before any ordered ledger query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittedHealthMigrationArtifacts {
    pub artifacts: Vec<MigrationScript>,
    pub authorization: Option<AdmittedHealthMigrationAuthorization>,
    pub release: String,
}

/// Explicit non-PostgreSQL admission boundary for shared time and artifact owners.
pub trait WeatherServiceExternalReadPort {
    type Error;

    /// Admit canonical sidecar strings with their shared-parser epoch evidence.
    fn admit_temperature_sidecar_read(
        &mut self,
        read: ForecastTemperatureSidecarRead,
    ) -> Result<AdmittedEcmwfTemperatureCanarySidecarQuery, Self::Error>;

    /// Load and admit local artifacts before the database ledger is queried.
    fn admit_health_migration_artifacts(
        &mut self,
    ) -> Result<AdmittedHealthMigrationArtifacts, Self::Error>;

    /// Bind admitted artifacts to the canonical applied-ledger digest.
    fn admit_health_migration_identity(
        &mut self,
        artifacts: AdmittedHealthMigrationArtifacts,
        applied: &[AppliedMigration],
    ) -> Result<AdmittedHealthMigrationIdentity, Self::Error>;

    /// Canonicalize one decoded worker timestamp through the shared time owner.
    fn admit_health_worker_last_loop_epoch(
        &mut self,
        value: &JsDateOrString,
    ) -> Result<i64, Self::Error>;
}

/// Concrete API repository backed by typed database and explicit external ports.
pub struct PostgresWeatherServiceRepository<Database, External> {
    database: Database,
    external: External,
}

impl<Database, External> PostgresWeatherServiceRepository<Database, External> {
    /// Bind the process-owned ports without adding transport or codec behavior.
    pub const fn new(database: Database, external: External) -> Self {
        Self { database, external }
    }

    /// Inspect database evidence after a request in tests or process wiring.
    pub const fn database(&self) -> &Database {
        &self.database
    }

    /// Lend the repository database to the process-owned maintenance application.
    pub(crate) fn database_mut(&mut self) -> &mut Database {
        &mut self.database
    }

    /// Inspect external-boundary evidence after a request.
    pub const fn external(&self) -> &External {
        &self.external
    }

    /// Return both injected ports to their process owner.
    pub fn into_parts(self) -> (Database, External) {
        (self.database, self.external)
    }
}

impl<Database, External> WeatherServiceRepository
    for PostgresWeatherServiceRepository<Database, External>
where
    Database: WeatherServicePostgresReadPort + RainServingReadPort,
    External: WeatherServiceExternalReadPort
        + RainRepositoryTimeBoundary
        + TemperatureCanaryRecentErrorDigestPort,
{
    /// Execute and project active-site rows through the database contract owner.
    fn active_sites(
        &mut self,
        plan: ListActiveSitesQuery,
    ) -> Result<Vec<ActiveSiteRow>, ServiceRepositoryError> {
        let rows = self
            .database
            .query_active_sites(&plan)
            .map_err(|_| ServiceRepositoryError)?;
        project_active_site_rows(rows).map_err(|_| ServiceRepositoryError)
    }

    /// Execute and preserve the current-weather row contract.
    fn current(
        &mut self,
        plan: CurrentWeatherQueryPlan,
    ) -> Result<Vec<CurrentWeatherRecordRow>, ServiceRepositoryError> {
        let rows = self
            .database
            .query_current(&plan)
            .map_err(|_| ServiceRepositoryError)?;
        Ok(project_current_weather_rows(rows).rows)
    }

    /// Execute and preserve database history order.
    fn history(
        &mut self,
        plan: HistoryReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, ServiceRepositoryError> {
        let rows = self
            .database
            .query_history(&plan)
            .map_err(|_| ServiceRepositoryError)?;
        Ok(project_weather_history_rows(rows).rows)
    }

    /// Execute and preserve ascending tide order.
    fn tides(&mut self, plan: TideReadPlan) -> Result<Vec<TideRecordRow>, ServiceRepositoryError> {
        let rows = self
            .database
            .query_tides(&plan)
            .map_err(|_| ServiceRepositoryError)?;
        Ok(project_tide_record_rows(rows).rows)
    }

    /// Execute the raw forecast and decorate only admitted optional provenance.
    fn forecast(
        &mut self,
        plan: ForecastReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, ServiceRepositoryError> {
        read_api_forecast(&mut self.database, &plan).map_err(|_| ServiceRepositoryError)
    }

    /// Execute and preserve the optional pressure-context order.
    fn forecast_pressure_context(
        &mut self,
        plan: ForecastPressureContextPlan,
    ) -> Result<Vec<WeatherRecordRow>, ServiceRepositoryError> {
        let rows = self
            .database
            .query_forecast_pressure_context(&plan)
            .map_err(|_| ServiceRepositoryError)?;
        Ok(project_forecast_pressure_context_rows(rows).rows)
    }

    /// Resolve admitted epochs, execute the two-stage read, and retain its material digest.
    fn temperature_canary_sidecar(
        &mut self,
        read: ForecastTemperatureSidecarRead,
    ) -> Result<Option<AdmittedTemperatureCanarySidecar>, ServiceRepositoryError> {
        let query = self
            .external
            .admit_temperature_sidecar_read(read)
            .map_err(|_| ServiceRepositoryError)?;
        let mut port =
            DigestValidatingTemperatureReadPort::new(&mut self.database, &mut self.external);
        let sidecar = get_ecmwf_temperature_canary_sidecar(&mut port, &query)
            .map_err(|_| ServiceRepositoryError)?;

        // bind the serving material to the exact state digest read with its selected run
        match sidecar {
            Some(sidecar) => Ok(Some(AdmittedTemperatureCanarySidecar {
                sidecar,
                recent_error_state_sha256: port
                    .recent_error_state_sha256
                    .ok_or(ServiceRepositoryError)?,
            })),
            None => Ok(None),
        }
    }

    /// Execute status with its independent safety-clock cutoff and no six-hour delay.
    fn temperature_canary_status(
        &mut self,
        read: weather_api::ForecastTemperatureStatusRead,
    ) -> Result<Option<EcmwfTemperatureCanaryStatus>, ServiceRepositoryError> {
        get_ecmwf_temperature_canary_status(
            &mut self.database,
            &AdmittedEcmwfTemperatureCanaryStatusQuery::admitted(read.as_of, read.site_slug),
        )
        .map_err(|_| ServiceRepositoryError)
    }

    /// Execute the typed rain statement and admit every retained canonical timestamp.
    fn rain_adjustment_run(
        &mut self,
        read: ForecastRainAdjustmentRead,
    ) -> Result<Option<AdmittedRainAdjustmentRun>, ServiceRepositoryError> {
        read_admitted_rain_adjustment_run(&mut self.database, &mut self.external, read)
            .map_err(|_| ServiceRepositoryError)
    }

    /// Execute and preserve ordered trend rows.
    fn trends(
        &mut self,
        plan: TrendReadPlan,
    ) -> Result<Vec<TrendPointRow>, ServiceRepositoryError> {
        let rows = self
            .database
            .query_trends(&plan)
            .map_err(|_| ServiceRepositoryError)?;
        Ok(project_trend_point_rows(rows).rows)
    }

    /// Execute and preserve the database's first-row-or-null behavior.
    fn daily_precipitation(
        &mut self,
        plan: DailyPrecipitationReadPlan,
    ) -> Result<Option<DailyPrecipitationRow>, ServiceRepositoryError> {
        let rows = self
            .database
            .query_daily_precipitation(&plan)
            .map_err(|_| ServiceRepositoryError)?;
        Ok(project_daily_precipitation_row(rows).row)
    }

    /// Execute Node-compatible health reads with independent fallback boundaries.
    fn health(&mut self) -> Result<HealthSnapshot, ServiceRepositoryError> {
        // only connectivity failure skips migration and heartbeat reads
        if self
            .database
            .query_health_ping(&health_ping_read_plan())
            .is_err()
        {
            return Ok(HealthSnapshot {
                database: weather_api::DatabaseStatus::Unavailable,
                migration_status: MigrationStatus::Unavailable,
                migration_version: None,
                worker_last_loop_epoch_ms: None,
            });
        }

        let migration = self
            .external
            .admit_health_migration_artifacts()
            .ok()
            .and_then(|artifacts| {
                let applied = self
                    .database
                    .query_health_migration_ledger(&health_migration_ledger_read_plan())
                    .ok()?;
                let identity = self
                    .external
                    .admit_health_migration_identity(artifacts, &applied)
                    .ok()?;
                verify_health_migration_readiness(&identity, &applied).ok()
            });
        let worker_last_loop_epoch_ms = self
            .database
            .query_health_latest_worker_heartbeat(&health_latest_worker_heartbeat_read_plan())
            .ok()
            .flatten()
            .and_then(|row| {
                self.external
                    .admit_health_worker_last_loop_epoch(&row.last_loop_at)
                    .ok()
            });
        // expose current only after complete artifact-ledger verification
        let (migration_status, migration_version) = match migration {
            Some(readiness) => (MigrationStatus::Current, Some(readiness.version)),
            None => (MigrationStatus::Outdated, None),
        };

        Ok(HealthSnapshot {
            database: weather_api::DatabaseStatus::Ready,
            migration_status,
            migration_version,
            worker_last_loop_epoch_ms,
        })
    }

    /// Execute the aggregate-only collection status statement.
    fn rain_collection_status(
        &mut self,
    ) -> Result<Option<RainCollectionStatus>, ServiceRepositoryError> {
        read_admitted_rain_collection_status(&mut self.database)
            .map(Some)
            .map_err(|_| ServiceRepositoryError)
    }

    /// Execute and project the fixed private observation-input statement.
    fn forecast_observation_input(
        &mut self,
        plan: ForecastObservationInputReadPlan,
    ) -> Result<Option<ForecastObservationInput>, ServiceRepositoryError> {
        let rows = self
            .database
            .query_forecast_observation_input(&plan)
            .map_err(|_| ServiceRepositoryError)?;
        project_forecast_observation_input_rows(rows).map_err(|_| ServiceRepositoryError)
    }
}

/// One database or canonical-digest failure contained by the fail-raw sidecar path.
enum TemperatureReadAdmissionError<QueryError> {
    Query(QueryError),
    Digest,
    Mismatch,
}

/// Borrowing adapter that validates the selected run's actual decoded state digest.
struct DigestValidatingTemperatureReadPort<'a, Port, Digest> {
    digest: &'a mut Digest,
    inner: &'a mut Port,
    recent_error_state_sha256: Option<String>,
}

impl<'a, Port, Digest> DigestValidatingTemperatureReadPort<'a, Port, Digest> {
    /// Start one two-query read without retained digest state.
    const fn new(inner: &'a mut Port, digest: &'a mut Digest) -> Self {
        Self {
            digest,
            inner,
            recent_error_state_sha256: None,
        }
    }
}

impl<Port, Digest> EcmwfTemperatureCanaryReadPort
    for DigestValidatingTemperatureReadPort<'_, Port, Digest>
where
    Port: EcmwfTemperatureCanaryReadPort,
    Digest: TemperatureCanaryRecentErrorDigestPort,
{
    type Error = TemperatureReadAdmissionError<Port::Error>;

    /// Bind the stored digest to the actual decoded generated state before reading hours.
    fn select_sidecar_run(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarRunReadPlan,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        let row = self
            .inner
            .select_sidecar_run(plan)
            .map_err(TemperatureReadAdmissionError::Query)?;

        // validate only one selected run and preserve the empty raw path
        if let Some(run) = row.as_ref() {
            let digest = self
                .digest
                .digest_recent_error_state(&run.recent_error_state)
                .map_err(|_| TemperatureReadAdmissionError::Digest)?;

            // fail raw before hour selection or adjustment use on stored-state drift
            if digest != run.recent_error_state_sha256 {
                return Err(TemperatureReadAdmissionError::Mismatch);
            }
            self.recent_error_state_sha256 = Some(digest.as_str().to_owned());
        }
        Ok(row)
    }

    /// Delegate selected-run hour decoding without changing order or nullability.
    fn select_sidecar_hours(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarHoursReadPlan,
    ) -> Result<Vec<EcmwfTemperatureCanaryHourReadRow>, Self::Error> {
        self.inner
            .select_sidecar_hours(plan)
            .map_err(TemperatureReadAdmissionError::Query)
    }

    /// Delegate status reads for trait completeness; sidecar execution never calls this method.
    fn select_status(
        &mut self,
        plan: &EcmwfTemperatureCanaryStatusReadPlan,
    ) -> Result<Option<EcmwfTemperatureCanaryStatusReadRow>, Self::Error> {
        self.inner
            .select_status(plan)
            .map_err(TemperatureReadAdmissionError::Query)
    }
}
