use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    Nullable, RainAdjustmentHour, RainAdjustmentRun, RainCollectionStatus,
    RainCollectionStatusContractVersionLiteral, RainCollectionStatusModelEnabledLiteral,
    RainCollectionStatusQualificationEnabledLiteral,
};

use crate::{AdmittedRevisionUtcInstant, Sha256Digest};

/// Exact recent immutable rain-run query with its source-owned 12-hour freshness bound.
pub const RAIN_ADJUSTMENT_RUN_READ_SQL: &str =
    include_str!("../fixtures/rain-adjustment-run-read.sql");
/// Exact aggregate-only collection status query.
pub const RAIN_COLLECTION_STATUS_READ_SQL: &str =
    include_str!("../fixtures/rain-collection-status.sql");

/// One rain-run request carrying the caller's parsed data-at evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RainAdjustmentRunReadRequest {
    pub as_of: String,
    pub as_of_epoch_milliseconds: Option<i64>,
    pub site_slug: String,
}

/// Closed scalar bind domain for the rain serving statements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RainServingReadBind {
    UtcInstant(String),
}

/// Exact one-bind recent rain-run statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RainAdjustmentRunReadPlan {
    pub binds: [RainServingReadBind; 1],
    pub sql: &'static str,
}

/// Exact zero-bind aggregate collection-status statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RainCollectionStatusReadPlan {
    pub binds: [RainServingReadBind; 0],
    pub sql: &'static str,
}

/// One database row after UTC and hash fields are admitted by owning adapters.
#[derive(Debug, Clone, PartialEq)]
pub struct RainAdjustmentRunReadRow {
    pub decision_at: AdmittedRevisionUtcInstant,
    pub first_received_at: AdmittedRevisionUtcInstant,
    pub forecast_claim_id: String,
    pub generated_at: AdmittedRevisionUtcInstant,
    pub hours: Vec<RainAdjustmentHour>,
    pub input_sha256: Sha256Digest,
    pub model_sha256: Sha256Digest,
    pub run_initialized_at: AdmittedRevisionUtcInstant,
}

/// One aggregate view row with all nullable timestamps already normalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RainCollectionStatusReadRow {
    pub claims: u64,
    pub compressed_bytes: u64,
    pub contract_version: String,
    pub failed_requests: u64,
    pub last_claim_at: Option<AdmittedRevisionUtcInstant>,
    pub last_forecast_receipt_at: Option<AdmittedRevisionUtcInstant>,
    pub last_receipt_at: Option<AdmittedRevisionUtcInstant>,
    pub last_station_receipt_at: Option<AdmittedRevisionUtcInstant>,
    pub model_enabled: bool,
    pub paused_until: Option<AdmittedRevisionUtcInstant>,
    pub pending_requests: u64,
    pub qualification_enabled: bool,
    pub receipts: u64,
    pub stations_seen: u64,
    pub timely_forecasts: u64,
    pub unknown_requests: u64,
    pub valid_forecasts: u64,
    pub valid_station_windows: u64,
}

/// Complete query and contract failure for ordinary rain serving reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RainServingReadError<QueryError> {
    Query(QueryError),
    StatusContractMismatch,
}

impl<QueryError: Display> Display for RainServingReadError<QueryError> {
    /// Preserve the source contract message and ordinary query errors.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Query(error) => Display::fmt(error, formatter),
            Self::StatusContractMismatch => {
                formatter.write_str("rain collection status contract mismatch")
            }
        }
    }
}

impl<QueryError> Error for RainServingReadError<QueryError> where QueryError: Error + 'static {}

/// Injected transport for the immutable run and aggregate-only status reads.
pub trait RainServingReadPort {
    type Error;

    /// Select the newest run within the statement's source-owned freshness window.
    fn select_rain_adjustment_run(
        &mut self,
        plan: &RainAdjustmentRunReadPlan,
    ) -> Result<Option<RainAdjustmentRunReadRow>, Self::Error>;

    /// Select the sanitized aggregate collection view without raw bodies.
    fn select_rain_collection_status(
        &mut self,
        plan: &RainCollectionStatusReadPlan,
    ) -> Result<Option<RainCollectionStatusReadRow>, Self::Error>;
}

/// Admit only the frozen site and an as-of instant parsed by the shared time owner.
pub fn rain_adjustment_run_read_plan(
    request: &RainAdjustmentRunReadRequest,
) -> Option<RainAdjustmentRunReadPlan> {
    // mirror the source's null result for another site or invalid data-at instant
    if request.site_slug != "ballydidean" || request.as_of_epoch_milliseconds.is_none() {
        return None;
    }
    Some(RainAdjustmentRunReadPlan {
        binds: [RainServingReadBind::UtcInstant(request.as_of.clone())],
        sql: RAIN_ADJUSTMENT_RUN_READ_SQL,
    })
}

/// Build the aggregate-only zero-bind collection status read.
pub const fn rain_collection_status_read_plan() -> RainCollectionStatusReadPlan {
    RainCollectionStatusReadPlan {
        binds: [],
        sql: RAIN_COLLECTION_STATUS_READ_SQL,
    }
}

/// Read one recent immutable run without exposing capture provider bodies.
pub fn read_rain_adjustment_run<Port: RainServingReadPort>(
    port: &mut Port,
    request: &RainAdjustmentRunReadRequest,
) -> Result<Option<RainAdjustmentRun>, RainServingReadError<Port::Error>> {
    // preserve the source's pre-query null disposition
    let Some(plan) = rain_adjustment_run_read_plan(request) else {
        return Ok(None);
    };
    let row = port
        .select_rain_adjustment_run(&plan)
        .map_err(RainServingReadError::Query)?;
    Ok(row.map(project_rain_adjustment_run))
}

/// Read and enforce the frozen aggregate-only collection status contract.
pub fn read_rain_collection_status<Port: RainServingReadPort>(
    port: &mut Port,
) -> Result<RainCollectionStatus, RainServingReadError<Port::Error>> {
    let row = port
        .select_rain_collection_status(&rain_collection_status_read_plan())
        .map_err(RainServingReadError::Query)?
        .ok_or(RainServingReadError::StatusContractMismatch)?;
    // require the source view's frozen non-serving contract
    if row.contract_version != RainCollectionStatusContractVersionLiteral::WIRE_VALUE
        || row.model_enabled
        || row.qualification_enabled
    {
        return Err(RainServingReadError::StatusContractMismatch);
    }
    Ok(project_rain_collection_status(row))
}

/// Project one typed storage row while retaining hour order and nullable reasons.
fn project_rain_adjustment_run(row: RainAdjustmentRunReadRow) -> RainAdjustmentRun {
    RainAdjustmentRun {
        runInitializedAt: row.run_initialized_at.as_str().to_owned(),
        modelSha256: row.model_sha256.as_str().to_owned(),
        inputSha256: row.input_sha256.as_str().to_owned(),
        forecastClaimId: row.forecast_claim_id,
        firstReceivedAt: row.first_received_at.as_str().to_owned(),
        decisionAt: row.decision_at.as_str().to_owned(),
        generatedAt: row.generated_at.as_str().to_owned(),
        hours: row.hours,
    }
}

/// Project one admitted aggregate view into the generated canonical contract.
fn project_rain_collection_status(row: RainCollectionStatusReadRow) -> RainCollectionStatus {
    RainCollectionStatus {
        contractVersion: RainCollectionStatusContractVersionLiteral,
        modelEnabled: RainCollectionStatusModelEnabledLiteral,
        qualificationEnabled: RainCollectionStatusQualificationEnabledLiteral,
        claims: row.claims,
        receipts: row.receipts,
        validForecasts: row.valid_forecasts,
        timelyForecasts: row.timely_forecasts,
        validStationWindows: row.valid_station_windows,
        stationsSeen: row.stations_seen,
        failedRequests: row.failed_requests,
        pendingRequests: row.pending_requests,
        unknownRequests: row.unknown_requests,
        lastClaimAt: nullable_instant(row.last_claim_at),
        lastReceiptAt: nullable_instant(row.last_receipt_at),
        lastForecastReceiptAt: nullable_instant(row.last_forecast_receipt_at),
        lastStationReceiptAt: nullable_instant(row.last_station_receipt_at),
        pausedUntil: nullable_instant(row.paused_until),
        compressedBytes: row.compressed_bytes,
    }
}

/// Project one nullable admitted instant without adding a time codec.
fn nullable_instant(value: Option<AdmittedRevisionUtcInstant>) -> Nullable<String> {
    value.map_or(Nullable::Null, |value| {
        Nullable::Value(value.as_str().to_owned())
    })
}
