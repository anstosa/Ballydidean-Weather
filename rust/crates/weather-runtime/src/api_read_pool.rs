//! Execute typed API statements through the retained PostgreSQL pool.

use weather_contracts::{
    CurrentWeatherRecordRow, DailyPrecipitationRow, TideRecordRow, TrendPointRow, WeatherRecordRow,
};
use weather_database::{
    ActiveSiteDbRow, AppliedMigration, CurrentWeatherQueryPlan, DailyPrecipitationReadPlan,
    DatabaseConfiguration, DatabasePoolConfiguration, EcmwfTemperatureCanaryHourReadRow,
    EcmwfTemperatureCanaryReadPort, EcmwfTemperatureCanarySidecarHoursReadPlan,
    EcmwfTemperatureCanarySidecarRunReadPlan, EcmwfTemperatureCanaryStatusReadPlan,
    EcmwfTemperatureCanaryStatusReadRow, ForecastObservationInputDbRow,
    ForecastObservationInputReadPlan, ForecastPressureContextPlan, ForecastProvenancePlan,
    ForecastReadPlan, ForecastRuntimeProvenanceRow, HealthLatestWorkerHeartbeatReadPlan,
    HealthLatestWorkerHeartbeatRow, HealthMigrationLedgerReadPlan, HealthPingReadPlan,
    HistoryReadPlan, ListActiveSitesQuery, RainAdjustmentRunReadPlan, RainAdjustmentRunReadRow,
    RainCollectionStatusReadPlan, RainCollectionStatusReadRow, RainServingReadPort,
    StoredEcmwfCanaryRun, TideReadPlan, TrendReadPlan, to_pool_configuration,
};

use crate::{PostgresWeatherServiceRepository, WeatherServicePostgresReadPort};

/// Distinguish normal recycling from disposal after a failed query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiReadSessionDisposition {
    Recycle,
    Discard,
}

/// Classify whether a failed API operation left its checked-out client reusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiReadSessionFailure {
    /// The statement or driver may have left the client unsuitable for reuse.
    StatementOrClient,
    /// SQL completed successfully before application row decoding failed.
    PostQueryDecode,
}

impl ApiReadSessionFailure {
    /// Convert the typed failure boundary into the pool's release action.
    const fn disposition(self) -> ApiReadSessionDisposition {
        // retain only clients whose database operation already completed successfully
        match self {
            Self::StatementOrClient => ApiReadSessionDisposition::Discard,
            Self::PostQueryDecode => ApiReadSessionDisposition::Recycle,
        }
    }
}

/// One driver-owned PostgreSQL client checked out for a single API statement.
pub trait ApiReadPoolLease {
    /// Return or discard the acquired client without masking the query result.
    fn release(self, disposition: ApiReadSessionDisposition);
}

/// Retained API pool using the admitted driver's acquisition and query implementation.
pub trait WeatherApiReadPool {
    type Error;
    type Session<'pool>: WeatherServicePostgresReadPort<Error = Self::Error>
        + RainServingReadPort<Error = Self::Error>
        + ApiReadPoolLease
    where
        Self: 'pool;

    /// Acquire through the process pool's normal bounded admission path.
    fn checkout(&mut self) -> Result<Self::Session<'_>, Self::Error>;

    /// Distinguish SQL/client failure from a later row decode failure without parsing diagnostics.
    fn classify_session_failure(error: &Self::Error) -> ApiReadSessionFailure;
}

/// An independent driver handle to the same retained process pool.
pub trait WeatherApiSharedPool: WeatherApiReadPool + Sized {
    /// Share the existing capacity and connection lifecycle without creating another pool.
    fn share(&self) -> Self;
}

/// The admitted PostgreSQL driver prepares the API's retained pool without querying the server.
pub trait WeatherApiReadPoolConnector {
    type Error;
    type Pool: WeatherApiReadPool;

    /// Construct the pool from the same settings used by the predecessor.
    fn prepare_pool(
        &mut self,
        configuration: DatabasePoolConfiguration,
    ) -> Result<Self::Pool, Self::Error>;
}

/// Repository composed from the API's retained pool and admitted runtime values.
pub type PooledWeatherServiceRepository<Pool, External> =
    PostgresWeatherServiceRepository<PooledWeatherApiReads<Pool>, External>;

/// Prepare the retained repository during API startup without an eager checkout or query.
pub fn prepare_pooled_weather_service_repository<Connector, External>(
    connector: &mut Connector,
    configuration: &DatabaseConfiguration,
    external: External,
) -> Result<PooledWeatherServiceRepository<Connector::Pool, External>, Connector::Error>
where
    Connector: WeatherApiReadPoolConnector,
{
    let pool = connector.prepare_pool(to_pool_configuration(configuration))?;
    Ok(PostgresWeatherServiceRepository::new(
        PooledWeatherApiReads::new(pool),
        external,
    ))
}

/// Bind the API repository read plans to single-statement pooled execution.
pub struct PooledWeatherApiReads<Pool> {
    pool: Pool,
}

impl<Pool> PooledWeatherApiReads<Pool> {
    /// Retain one pool across requests without checking out a client.
    pub const fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Return the retained pool to the process for its normal shutdown.
    pub fn into_pool(self) -> Pool {
        self.pool
    }

    /// Borrow the retained driver handle when binding another request worker.
    pub(crate) const fn pool(&self) -> &Pool {
        &self.pool
    }

    /// Lend the original pool to another application owned by the same API process.
    pub(crate) fn pool_mut(&mut self) -> &mut Pool {
        &mut self.pool
    }
}

impl<Pool: WeatherApiReadPool> PooledWeatherApiReads<Pool> {
    /// Use one checkout per statement like the source pool query interface.
    fn query<T>(
        &mut self,
        execute: impl FnOnce(&mut Pool::Session<'_>) -> Result<T, Pool::Error>,
    ) -> Result<T, Pool::Error> {
        let session = self.pool.checkout()?;
        run_api_pool_query(session, execute, Pool::classify_session_failure)
    }
}

/// Return the API client before reporting a typed statement result.
pub(crate) fn run_api_pool_query<Session: ApiReadPoolLease, T, Error>(
    mut session: Session,
    execute: impl FnOnce(&mut Session) -> Result<T, Error>,
    classify_failure: impl FnOnce(&Error) -> ApiReadSessionFailure,
) -> Result<T, Error> {
    let result = execute(&mut session);
    // preserve the operation result while deriving its exact release action
    let disposition = match &result {
        Ok(_) => ApiReadSessionDisposition::Recycle,
        Err(error) => classify_failure(error).disposition(),
    };
    session.release(disposition);
    result
}

// keep statement forwarding uniform without altering plan or decoded row ownership
macro_rules! pooled_queries {
    ($($method:ident($plan:ty) -> $result:ty;)+) => {
        $(
            /// Execute the typed plan with exactly one pooled client.
            fn $method(&mut self, plan: &$plan) -> Result<$result, Pool::Error> {
                self.query(|session| session.$method(plan))
            }
        )+
    };
}

impl<Pool: WeatherApiReadPool> EcmwfTemperatureCanaryReadPort for PooledWeatherApiReads<Pool> {
    type Error = Pool::Error;

    pooled_queries! {
        select_sidecar_run(EcmwfTemperatureCanarySidecarRunReadPlan) -> Option<StoredEcmwfCanaryRun>;
        select_sidecar_hours(EcmwfTemperatureCanarySidecarHoursReadPlan) -> Vec<EcmwfTemperatureCanaryHourReadRow>;
        select_status(EcmwfTemperatureCanaryStatusReadPlan) -> Option<EcmwfTemperatureCanaryStatusReadRow>;
    }
}

impl<Pool: WeatherApiReadPool> WeatherServicePostgresReadPort for PooledWeatherApiReads<Pool> {
    pooled_queries! {
        query_active_sites(ListActiveSitesQuery) -> Vec<ActiveSiteDbRow>;
        query_current(CurrentWeatherQueryPlan) -> Vec<CurrentWeatherRecordRow>;
        query_history(HistoryReadPlan) -> Vec<WeatherRecordRow>;
        query_tides(TideReadPlan) -> Vec<TideRecordRow>;
        query_forecast(ForecastReadPlan) -> Vec<WeatherRecordRow>;
        query_forecast_provenance(ForecastProvenancePlan) -> Vec<ForecastRuntimeProvenanceRow>;
        query_forecast_pressure_context(ForecastPressureContextPlan) -> Vec<WeatherRecordRow>;
        query_trends(TrendReadPlan) -> Vec<TrendPointRow>;
        query_daily_precipitation(DailyPrecipitationReadPlan) -> Vec<DailyPrecipitationRow>;
        query_forecast_observation_input(ForecastObservationInputReadPlan) -> Vec<ForecastObservationInputDbRow>;
        query_health_ping(HealthPingReadPlan) -> ();
        query_health_migration_ledger(HealthMigrationLedgerReadPlan) -> Vec<AppliedMigration>;
        query_health_latest_worker_heartbeat(HealthLatestWorkerHeartbeatReadPlan) -> Option<HealthLatestWorkerHeartbeatRow>;
    }
}

impl<Pool: WeatherApiReadPool> RainServingReadPort for PooledWeatherApiReads<Pool> {
    type Error = Pool::Error;

    pooled_queries! {
        select_rain_adjustment_run(RainAdjustmentRunReadPlan) -> Option<RainAdjustmentRunReadRow>;
        select_rain_collection_status(RainCollectionStatusReadPlan) -> Option<RainCollectionStatusReadRow>;
    }
}
