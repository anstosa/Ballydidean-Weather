use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    EcmwfTemperatureCanaryHour, EcmwfTemperatureCanaryRun,
    EcmwfTemperatureCanaryRunStateStatusEnum, EcmwfTemperatureCanarySidecar,
    EcmwfTemperatureCanaryStatus, EcmwfTemperatureCanaryStatusStateStatusEnum, Nullable,
};

use crate::{Sha256Digest, StoredEcmwfCanaryRun};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const MAX_FORECAST_HOURS: i64 = 264;
const MAX_RUN_INVENTORY_INTERVAL_MILLISECONDS: i64 = 7 * 86_400_000;

/// Exact bounded run-inventory query used by warmup planning.
pub const ECMWF_TEMPERATURE_CANARY_RUN_INITIALIZATIONS_SQL: &str =
    include_str!("../fixtures/ecmwf-canary-run-initializations.sql");
/// Exact latest eligible run query used by the serving sidecar.
pub const ECMWF_TEMPERATURE_CANARY_SIDECAR_RUN_SQL: &str =
    include_str!("../fixtures/ecmwf-canary-sidecar-run.sql");
/// Exact ordered hour query used after one eligible run is selected.
pub const ECMWF_TEMPERATURE_CANARY_SIDECAR_HOURS_SQL: &str =
    include_str!("../fixtures/ecmwf-canary-sidecar-hours.sql");
/// Exact latest collection-status query, without the sidecar's six-hour cutoff.
pub const ECMWF_TEMPERATURE_CANARY_STATUS_SQL: &str =
    include_str!("../fixtures/ecmwf-canary-status.sql");

/// One sidecar request after the shared UTC parser admitted canonical instants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedEcmwfTemperatureCanarySidecarQuery {
    pub as_of: String,
    pub from: String,
    pub from_epoch_milliseconds: i64,
    pub site_slug: String,
    pub to: String,
    pub to_epoch_milliseconds: i64,
}

impl AdmittedEcmwfTemperatureCanarySidecarQuery {
    /// Preserve canonical strings and their already-resolved epoch values.
    pub fn admitted(
        as_of: String,
        from: String,
        from_epoch_milliseconds: i64,
        site_slug: String,
        to: String,
        to_epoch_milliseconds: i64,
    ) -> Self {
        Self {
            as_of,
            from,
            from_epoch_milliseconds,
            site_slug,
            to,
            to_epoch_milliseconds,
        }
    }
}

/// One status request with its independently admitted logical as-of instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedEcmwfTemperatureCanaryStatusQuery {
    pub as_of: String,
    pub site_slug: String,
}

impl AdmittedEcmwfTemperatureCanaryStatusQuery {
    /// Preserve the independently parsed status cutoff.
    pub fn admitted(as_of: String, site_slug: String) -> Self {
        Self { as_of, site_slug }
    }
}

/// Closed scalar bind domain for the temperature canary read statements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EcmwfTemperatureCanaryReadBind {
    Text(String),
    UtcInstant(String),
}

/// Exact warmup run-inventory statement and placeholder order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcmwfTemperatureCanaryRunInitializationsReadPlan {
    pub binds: [EcmwfTemperatureCanaryReadBind; 3],
    pub sql: &'static str,
}

/// Exact latest-run sidecar statement and placeholder order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcmwfTemperatureCanarySidecarRunReadPlan {
    pub binds: [EcmwfTemperatureCanaryReadBind; 2],
    pub sql: &'static str,
}

/// Exact selected-run hour statement and placeholder order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcmwfTemperatureCanarySidecarHoursReadPlan {
    pub binds: [EcmwfTemperatureCanaryReadBind; 3],
    pub sql: &'static str,
}

/// Exact latest-status statement and independently supplied cutoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcmwfTemperatureCanaryStatusReadPlan {
    pub binds: [EcmwfTemperatureCanaryReadBind; 2],
    pub sql: &'static str,
}

/// One decoded hour row whose timestamp, digest, and nullability are already admitted.
#[derive(Debug, Clone, PartialEq)]
pub struct EcmwfTemperatureCanaryHourReadRow {
    pub content_hash: Sha256Digest,
    pub model_lead_hours: u64,
    pub raw_relative_humidity_percent: Nullable<f64>,
    pub raw_temperature_c: f64,
    pub raw_wind_speed_mps: Nullable<f64>,
    pub valid_at: String,
}

/// One decoded status row after the database adapter admits its scalar fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcmwfTemperatureCanaryStatusReadRow {
    pub first_received_at: String,
    pub hour_count: u64,
    pub run_initialized_at: String,
    pub state_reason: String,
    pub state_status: EcmwfTemperatureCanaryRunStateStatusEnum,
}

/// Input rejected before issuing a temperature canary read query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcmwfTemperatureCanaryReadPlanError {
    InvalidHorizon,
    InvalidRunInventoryInterval,
}

impl Display for EcmwfTemperatureCanaryReadPlanError {
    /// Render the exact source-facing range failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHorizon => {
                formatter.write_str("ECMWF temperature canary horizon is invalid")
            }
            Self::InvalidRunInventoryInterval => {
                formatter.write_str("ECMWF temperature canary run inventory interval is invalid")
            }
        }
    }
}

impl Error for EcmwfTemperatureCanaryReadPlanError {}

/// Complete read failure without collapsing input and query errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EcmwfTemperatureCanaryReadError<QueryError> {
    Input(EcmwfTemperatureCanaryReadPlanError),
    Query(QueryError),
}

impl<QueryError: Display> Display for EcmwfTemperatureCanaryReadError<QueryError> {
    /// Preserve the owning error message.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input(error) => Display::fmt(error, formatter),
            Self::Query(error) => Display::fmt(error, formatter),
        }
    }
}

impl<QueryError> Error for EcmwfTemperatureCanaryReadError<QueryError> where
    QueryError: Error + 'static
{
}

/// Injected database transport for the ordinary serving reads.
pub trait EcmwfTemperatureCanaryReadPort {
    type Error;

    /// Select the latest run admitted by the sidecar's logical cutoff.
    fn select_sidecar_run(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarRunReadPlan,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error>;

    /// Select ordered leads seven through eighteen for one selected run.
    fn select_sidecar_hours(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarHoursReadPlan,
    ) -> Result<Vec<EcmwfTemperatureCanaryHourReadRow>, Self::Error>;

    /// Select the latest collector status using its independent logical cutoff.
    fn select_status(
        &mut self,
        plan: &EcmwfTemperatureCanaryStatusReadPlan,
    ) -> Result<Option<EcmwfTemperatureCanaryStatusReadRow>, Self::Error>;
}

/// Build the inclusive warmup run-inventory query after checking its seven-day envelope.
pub fn ecmwf_temperature_canary_run_initializations_read_plan(
    site_slug: &str,
    from: String,
    from_epoch_milliseconds: i64,
    to: String,
    to_epoch_milliseconds: i64,
) -> Result<EcmwfTemperatureCanaryRunInitializationsReadPlan, EcmwfTemperatureCanaryReadPlanError> {
    let elapsed_milliseconds = to_epoch_milliseconds
        .checked_sub(from_epoch_milliseconds)
        .ok_or(EcmwfTemperatureCanaryReadPlanError::InvalidRunInventoryInterval)?;
    // match Node's canonical string ordering while retaining inclusive equality
    if from.as_str() > to.as_str() || elapsed_milliseconds > MAX_RUN_INVENTORY_INTERVAL_MILLISECONDS
    {
        return Err(EcmwfTemperatureCanaryReadPlanError::InvalidRunInventoryInterval);
    }
    Ok(EcmwfTemperatureCanaryRunInitializationsReadPlan {
        binds: [
            EcmwfTemperatureCanaryReadBind::Text(site_slug.to_owned()),
            EcmwfTemperatureCanaryReadBind::UtcInstant(from),
            EcmwfTemperatureCanaryReadBind::UtcInstant(to),
        ],
        sql: ECMWF_TEMPERATURE_CANARY_RUN_INITIALIZATIONS_SQL,
    })
}

/// Build the latest eligible sidecar run query after checking the bounded horizon.
pub fn ecmwf_temperature_canary_sidecar_run_read_plan(
    query: &AdmittedEcmwfTemperatureCanarySidecarQuery,
) -> Result<EcmwfTemperatureCanarySidecarRunReadPlan, EcmwfTemperatureCanaryReadPlanError> {
    validate_sidecar_horizon(query)?;
    Ok(EcmwfTemperatureCanarySidecarRunReadPlan {
        binds: [
            EcmwfTemperatureCanaryReadBind::Text(query.site_slug.clone()),
            EcmwfTemperatureCanaryReadBind::UtcInstant(query.as_of.clone()),
        ],
        sql: ECMWF_TEMPERATURE_CANARY_SIDECAR_RUN_SQL,
    })
}

/// Build the ordered hour query for the run selected by the first statement.
pub fn ecmwf_temperature_canary_sidecar_hours_read_plan(
    query: &AdmittedEcmwfTemperatureCanarySidecarQuery,
    run_id: String,
) -> Result<EcmwfTemperatureCanarySidecarHoursReadPlan, EcmwfTemperatureCanaryReadPlanError> {
    validate_sidecar_horizon(query)?;
    Ok(EcmwfTemperatureCanarySidecarHoursReadPlan {
        binds: [
            EcmwfTemperatureCanaryReadBind::Text(run_id),
            EcmwfTemperatureCanaryReadBind::UtcInstant(query.from.clone()),
            EcmwfTemperatureCanaryReadBind::UtcInstant(query.to.clone()),
        ],
        sql: ECMWF_TEMPERATURE_CANARY_SIDECAR_HOURS_SQL,
    })
}

/// Build the collector-status query without importing the sidecar's six-hour rule.
pub fn ecmwf_temperature_canary_status_read_plan(
    query: &AdmittedEcmwfTemperatureCanaryStatusQuery,
) -> EcmwfTemperatureCanaryStatusReadPlan {
    EcmwfTemperatureCanaryStatusReadPlan {
        binds: [
            EcmwfTemperatureCanaryReadBind::Text(query.site_slug.clone()),
            EcmwfTemperatureCanaryReadBind::UtcInstant(query.as_of.clone()),
        ],
        sql: ECMWF_TEMPERATURE_CANARY_STATUS_SQL,
    }
}

/// Execute the two-stage sidecar read without mixing hours across runs.
pub fn get_ecmwf_temperature_canary_sidecar<Port: EcmwfTemperatureCanaryReadPort>(
    port: &mut Port,
    query: &AdmittedEcmwfTemperatureCanarySidecarQuery,
) -> Result<Option<EcmwfTemperatureCanarySidecar>, EcmwfTemperatureCanaryReadError<Port::Error>> {
    let run_plan = ecmwf_temperature_canary_sidecar_run_read_plan(query)
        .map_err(EcmwfTemperatureCanaryReadError::Input)?;
    // preserve the raw-serving path when no eligible model run exists
    let Some(stored_run) = port
        .select_sidecar_run(&run_plan)
        .map_err(EcmwfTemperatureCanaryReadError::Query)?
    else {
        return Ok(None);
    };
    let hours_plan = ecmwf_temperature_canary_sidecar_hours_read_plan(query, stored_run.id.clone())
        .map_err(EcmwfTemperatureCanaryReadError::Input)?;
    let hours = port
        .select_sidecar_hours(&hours_plan)
        .map_err(EcmwfTemperatureCanaryReadError::Query)?;

    Ok(Some(EcmwfTemperatureCanarySidecar {
        hours: hours.into_iter().map(project_hour).collect(),
        run: project_run(stored_run),
    }))
}

/// Execute the collector-status read with its own as-of contract.
pub fn get_ecmwf_temperature_canary_status<Port: EcmwfTemperatureCanaryReadPort>(
    port: &mut Port,
    query: &AdmittedEcmwfTemperatureCanaryStatusQuery,
) -> Result<Option<EcmwfTemperatureCanaryStatus>, EcmwfTemperatureCanaryReadError<Port::Error>> {
    let plan = ecmwf_temperature_canary_status_read_plan(query);
    let row = port
        .select_status(&plan)
        .map_err(EcmwfTemperatureCanaryReadError::Query)?;
    Ok(row.map(project_status))
}

/// Require one increasing interval no longer than the Node maximum forecast horizon.
fn validate_sidecar_horizon(
    query: &AdmittedEcmwfTemperatureCanarySidecarQuery,
) -> Result<(), EcmwfTemperatureCanaryReadPlanError> {
    // reject epoch arithmetic overflow before evaluating the bound
    let Some(duration) = query
        .to_epoch_milliseconds
        .checked_sub(query.from_epoch_milliseconds)
    else {
        return Err(EcmwfTemperatureCanaryReadPlanError::InvalidHorizon);
    };
    // reject reversed, empty, or excessive windows before any query
    if duration <= 0 || duration > MAX_FORECAST_HOURS * HOUR_MILLISECONDS {
        return Err(EcmwfTemperatureCanaryReadPlanError::InvalidHorizon);
    }
    Ok(())
}

/// Project one already-admitted stored run into the generated serving contract.
fn project_run(row: StoredEcmwfCanaryRun) -> EcmwfTemperatureCanaryRun {
    EcmwfTemperatureCanaryRun {
        adapterVersion: row.adapter_version,
        firstReceivedAt: row.first_received_at,
        id: row.id,
        modelCycle: row.model_cycle,
        providerResponseSha256: row.provider_response_sha256.as_str().to_owned(),
        recentErrorState: row.recent_error_state,
        runInitializedAt: row.run_initialized_at,
        stateReason: row.state_reason,
        stateStatus: row.state_status,
        upstreamModel: row.upstream_model,
    }
}

/// Project one already-admitted nullable hour row into the generated contract.
fn project_hour(row: EcmwfTemperatureCanaryHourReadRow) -> EcmwfTemperatureCanaryHour {
    EcmwfTemperatureCanaryHour {
        contentHash: row.content_hash.as_str().to_owned(),
        modelLeadHours: row.model_lead_hours,
        rawRelativeHumidityPercent: row.raw_relative_humidity_percent,
        rawTemperatureC: row.raw_temperature_c,
        rawWindSpeedMps: row.raw_wind_speed_mps,
        validAt: row.valid_at,
    }
}

/// Project one collector row while retaining the generated status-specific enum.
fn project_status(row: EcmwfTemperatureCanaryStatusReadRow) -> EcmwfTemperatureCanaryStatus {
    // translate the equivalent generated enum without string parsing
    let state_status = match row.state_status {
        EcmwfTemperatureCanaryRunStateStatusEnum::Supported => {
            EcmwfTemperatureCanaryStatusStateStatusEnum::Supported
        }
        EcmwfTemperatureCanaryRunStateStatusEnum::Cold => {
            EcmwfTemperatureCanaryStatusStateStatusEnum::Cold
        }
        EcmwfTemperatureCanaryRunStateStatusEnum::Insufficient => {
            EcmwfTemperatureCanaryStatusStateStatusEnum::Insufficient
        }
        EcmwfTemperatureCanaryRunStateStatusEnum::Invalid => {
            EcmwfTemperatureCanaryStatusStateStatusEnum::Invalid
        }
    };
    EcmwfTemperatureCanaryStatus {
        firstReceivedAt: row.first_received_at,
        hourCount: row.hour_count,
        latestRunInitializedAt: row.run_initialized_at,
        stateReason: row.state_reason,
        stateStatus: state_status,
    }
}
