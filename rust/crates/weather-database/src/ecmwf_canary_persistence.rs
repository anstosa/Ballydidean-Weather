use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    EcmwfTemperatureCanaryHour, EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRun,
    EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
    EcmwfTemperatureCanaryRunStateStatusEnum, EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    Nullable, PersistedEcmwfTemperatureRevision, WeatherRecordRow,
};

use crate::{AdmittedRevisionUtcInstant, SerializedJson, Sha256Digest};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const ECMWF_50R1_CUTOVER_MILLISECONDS: i64 = 1_778_565_600_000;

/// Exact active-site identity lookup used before the private write.
pub const ECMWF_CANARY_SITE_SQL: &str = include_str!("../fixtures/ecmwf-canary-site.sql");
/// Exact immutable run insert with idempotent conflict behavior.
pub const ECMWF_CANARY_INSERT_RUN_SQL: &str =
    include_str!("../fixtures/ecmwf-canary-insert-run.sql");
/// Exact one-hour insert executed for a newly claimed run.
pub const ECMWF_CANARY_INSERT_HOUR_SQL: &str =
    include_str!("../fixtures/ecmwf-canary-insert-hour.sql");
/// Exact locked existing-run lookup used after an insert conflict.
pub const ECMWF_CANARY_EXISTING_RUN_SQL: &str =
    include_str!("../fixtures/ecmwf-canary-existing-run.sql");
/// Exact ordered existing-hour lookup used for retry identity checks.
pub const ECMWF_CANARY_EXISTING_HOURS_SQL: &str =
    include_str!("../fixtures/ecmwf-canary-existing-hours.sql");
/// Exact monotonic receipt update used after an identical retry.
pub const ECMWF_CANARY_UPDATE_RECEIPT_SQL: &str =
    include_str!("../fixtures/ecmwf-canary-update-receipt.sql");
/// Exact source-decision clock read before archive savepoint work.
pub const ECMWF_CANARY_LOGICAL_RECEIPT_SQL: &str =
    "SELECT clock_timestamp() AS \"logicalReceivedAt\"";
/// Exact complete Best Match product lookup visible at the decision clock.
pub const ECMWF_CANARY_BEST_MATCH_SQL: &str =
    include_str!("../fixtures/ecmwf-canary-best-match.sql");
/// Exact optional archive savepoint command.
pub const SAVEPOINT_ECMWF_CANARY_REVISION_SQL: &str =
    "SAVEPOINT ecmwf_temperature_revision_archive";
/// Exact optional archive savepoint rollback command.
pub const ROLLBACK_ECMWF_CANARY_REVISION_SQL: &str =
    "ROLLBACK TO SAVEPOINT ecmwf_temperature_revision_archive";
/// Exact optional archive savepoint release command.
pub const RELEASE_ECMWF_CANARY_REVISION_SQL: &str =
    "RELEASE SAVEPOINT ecmwf_temperature_revision_archive";

/// One hour after source validation, hashing, and time normalization.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedEcmwfCanaryHour {
    pub content_hash: Sha256Digest,
    pub model_lead_hours: u64,
    pub raw_relative_humidity_percent: Nullable<f64>,
    pub raw_temperature_c: f64,
    pub raw_wind_speed_mps: Nullable<f64>,
    pub valid_at: String,
    pub valid_at_epoch_milliseconds: i64,
}

/// Complete input admitted by the source validation, JSON, time, and hashing ports.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedEcmwfCanaryRunPersistence {
    pub adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
    pub content_hash: Sha256Digest,
    pub hours: Vec<PreparedEcmwfCanaryHour>,
    pub model_cycle: EcmwfTemperatureCanaryRunModelCycleEnum,
    pub provider_response_sha256: Sha256Digest,
    pub received_at: String,
    pub received_at_epoch_milliseconds: i64,
    pub recent_error_state: EcmwfTemperatureCanaryRecentErrorState,
    pub recent_error_state_json: SerializedJson,
    pub recent_error_state_maximum_run_epoch_milliseconds: Option<i64>,
    pub recent_error_state_maximum_valid_epoch_milliseconds: Option<i64>,
    pub recent_error_state_sha256: Sha256Digest,
    pub recent_error_state_window_end_epoch_milliseconds: i64,
    pub recent_error_state_window_end_valid_at: String,
    pub run_initialized_at: String,
    pub run_initialized_at_epoch_milliseconds: i64,
    pub site_slug: String,
    pub state_reason: String,
    pub state_status: EcmwfTemperatureCanaryRunStateStatusEnum,
    pub upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
}

/// PostgreSQL bind values shared by the six persistence statements.
#[derive(Debug, Clone, PartialEq)]
pub enum EcmwfCanaryPersistenceBind {
    Count(u64),
    Digest(Sha256Digest),
    Json(SerializedJson),
    NullableNumber(Option<f64>),
    Number(f64),
    Text(String),
    UtcInstant(String),
}

/// One active-site query and its site-slug bind.
#[derive(Debug, Clone, PartialEq)]
pub struct EcmwfCanarySiteQuery {
    pub binds: [EcmwfCanaryPersistenceBind; 1],
    pub sql: &'static str,
}

/// One immutable run insert and its exact bind order.
#[derive(Debug, Clone, PartialEq)]
pub struct EcmwfCanaryRunInsertQuery {
    pub binds: [EcmwfCanaryPersistenceBind; 12],
    pub sql: &'static str,
}

/// One immutable hour insert and its exact bind order.
#[derive(Debug, Clone, PartialEq)]
pub struct EcmwfCanaryHourInsertQuery {
    pub binds: [EcmwfCanaryPersistenceBind; 7],
    pub sql: &'static str,
}

/// One locked existing-run query after an insert conflict.
#[derive(Debug, Clone, PartialEq)]
pub struct EcmwfCanaryExistingRunQuery {
    pub binds: [EcmwfCanaryPersistenceBind; 2],
    pub sql: &'static str,
}

/// One ordered existing-hour identity query.
#[derive(Debug, Clone, PartialEq)]
pub struct EcmwfCanaryExistingHoursQuery {
    pub binds: [EcmwfCanaryPersistenceBind; 1],
    pub sql: &'static str,
}

/// One monotonic last-received update after an identical retry.
#[derive(Debug, Clone, PartialEq)]
pub struct EcmwfCanaryReceiptUpdateQuery {
    pub binds: [EcmwfCanaryPersistenceBind; 2],
    pub sql: &'static str,
}

/// One zero-bind database logical-clock read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EcmwfCanaryLogicalReceiptQuery {
    pub sql: &'static str,
}

/// One exact Best Match lookup and its placeholder order.
#[derive(Debug, Clone, PartialEq)]
pub struct EcmwfCanaryBestMatchQuery {
    pub binds: [EcmwfCanaryPersistenceBind; 4],
    pub sql: &'static str,
}

/// Canonical 12-hour Best Match window admitted by the shared time boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedEcmwfCanaryRevisionWindow {
    from: String,
    from_epoch_milliseconds: i64,
    to: String,
    to_epoch_milliseconds: i64,
}

impl AdmittedEcmwfCanaryRevisionWindow {
    /// Admit canonical strings with their already resolved epoch evidence.
    pub fn admitted(
        from: impl Into<String>,
        from_epoch_milliseconds: i64,
        to: impl Into<String>,
        to_epoch_milliseconds: i64,
    ) -> Self {
        Self {
            from: from.into(),
            from_epoch_milliseconds,
            to: to.into(),
            to_epoch_milliseconds,
        }
    }
}

/// One decoded Best Match row plus time and hash evidence from owning adapters.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedEcmwfCanaryBestMatchRow {
    pub content_hash: Sha256Digest,
    pub product_run_at_epoch_milliseconds: Option<i64>,
    pub row: WeatherRecordRow,
    pub valid_at_epoch_milliseconds: i64,
}

/// The three exact commands within the optional archive savepoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcmwfCanaryRevisionSavepointCommand {
    Savepoint,
    RollbackTo,
    Release,
}

impl EcmwfCanaryRevisionSavepointCommand {
    /// Return the source-owned command text.
    pub const fn sql(self) -> &'static str {
        // retain exact command spelling and recovery order
        match self {
            Self::Savepoint => SAVEPOINT_ECMWF_CANARY_REVISION_SQL,
            Self::RollbackTo => ROLLBACK_ECMWF_CANARY_REVISION_SQL,
            Self::Release => RELEASE_ECMWF_CANARY_REVISION_SQL,
        }
    }
}

/// One active site row returned by the exact identity lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct EcmwfCanarySiteRow {
    pub id: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// One trusted stored run returned by either run statement.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEcmwfCanaryRun {
    pub adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
    pub content_hash: Sha256Digest,
    pub first_received_at: String,
    pub id: String,
    pub model_cycle: EcmwfTemperatureCanaryRunModelCycleEnum,
    pub provider_response_sha256: Sha256Digest,
    pub recent_error_state: EcmwfTemperatureCanaryRecentErrorState,
    pub recent_error_state_sha256: Sha256Digest,
    pub run_initialized_at: String,
    pub state_reason: String,
    pub state_status: EcmwfTemperatureCanaryRunStateStatusEnum,
    pub upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
}

/// One persisted hour identity read in ascending lead order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEcmwfCanaryHourIdentity {
    pub content_hash: Sha256Digest,
}

/// Input rejected before the transaction starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcmwfCanaryPersistenceInputError {
    InvalidHour,
    InvalidHourCount,
    InvalidModelCycle,
    InvalidRecentErrorState,
    InvalidReceipt,
    InvalidRevisionWindow,
    InvalidStateReason,
    InvalidStateStatus,
}

/// Complete transaction failure with source-compatible retry categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EcmwfCanaryPersistenceError<CommandError> {
    Command(CommandError),
    ExistingRunMissing,
    Input(EcmwfCanaryPersistenceInputError),
    LogicalReceiptMissing,
    RevisionRejected,
    SiteMissing,
    StoredHoursMismatch,
}

impl Display for EcmwfCanaryPersistenceInputError {
    /// Render one staged input admission failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidHour => "ECMWF temperature canary hour is invalid",
            Self::InvalidHourCount => {
                "ECMWF temperature canary run must contain exact leads 1 through 18"
            }
            Self::InvalidModelCycle => "ECMWF temperature canary model cycle is invalid",
            Self::InvalidRecentErrorState => {
                "ECMWF temperature canary recent-error state is invalid"
            }
            Self::InvalidReceipt => "ECMWF temperature canary run timing is invalid",
            Self::InvalidRevisionWindow => "ECMWF temperature canary revision window is invalid",
            Self::InvalidStateReason => "ECMWF temperature canary state reason is invalid",
            Self::InvalidStateStatus => {
                "ECMWF temperature canary state status does not match state"
            }
        })
    }
}

impl Error for EcmwfCanaryPersistenceInputError {}

impl<CommandError: Display> Display for EcmwfCanaryPersistenceError<CommandError> {
    /// Render one persistence transaction failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command(error) => Display::fmt(error, formatter),
            Self::ExistingRunMissing => {
                formatter.write_str("existing ECMWF temperature canary run is missing")
            }
            Self::Input(error) => Display::fmt(error, formatter),
            Self::LogicalReceiptMissing => {
                formatter.write_str("ECMWF temperature revision receipt returned no row")
            }
            Self::RevisionRejected => {
                formatter.write_str("ECMWF temperature canary run revision rejected")
            }
            Self::SiteMissing => formatter.write_str("ECMWF temperature canary site is missing"),
            Self::StoredHoursMismatch => {
                formatter.write_str("ECMWF temperature canary stored hours do not match retry")
            }
        }
    }
}

impl<CommandError> Error for EcmwfCanaryPersistenceError<CommandError> where
    CommandError: Error + 'static
{
}

/// Injected transaction boundary for one retained database client.
pub trait EcmwfCanaryPersistencePort {
    type Error;

    /// Begin one source transaction.
    fn begin_transaction(&mut self) -> Result<(), Self::Error>;

    /// Read the active site identity.
    fn select_site(
        &mut self,
        query: &EcmwfCanarySiteQuery,
    ) -> Result<Option<EcmwfCanarySiteRow>, Self::Error>;

    /// Attempt the immutable run insert.
    fn insert_run(
        &mut self,
        query: &EcmwfCanaryRunInsertQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error>;

    /// Insert one exact lead for a newly claimed run.
    fn insert_hour(&mut self, query: &EcmwfCanaryHourInsertQuery) -> Result<(), Self::Error>;

    /// Lock the existing run after an idempotent insert conflict.
    fn select_existing_run(
        &mut self,
        query: &EcmwfCanaryExistingRunQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error>;

    /// Read all existing hour identities in lead order.
    fn select_existing_hours(
        &mut self,
        query: &EcmwfCanaryExistingHoursQuery,
    ) -> Result<Vec<StoredEcmwfCanaryHourIdentity>, Self::Error>;

    /// Advance the last receipt monotonically for an identical retry.
    fn update_last_received(
        &mut self,
        query: &EcmwfCanaryReceiptUpdateQuery,
    ) -> Result<(), Self::Error>;

    /// Commit the complete run/hour transaction.
    fn commit_transaction(&mut self) -> Result<(), Self::Error>;

    /// Roll back one body or commit failure.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error>;

    /// Release the retained client after any attempted transaction.
    fn release_client(&mut self);
}

/// Optional cold-revision work executed on the same retained transaction port.
pub trait EcmwfCanaryRevisionArchivePort: EcmwfCanaryPersistencePort {
    type Publication;

    /// Read one database-owned logical receipt before the savepoint.
    fn select_logical_receipt(
        &mut self,
        query: &EcmwfCanaryLogicalReceiptQuery,
    ) -> Result<Option<AdmittedRevisionUtcInstant>, Self::Error>;

    /// Execute one exact archive savepoint command.
    fn execute_revision_savepoint(
        &mut self,
        command: EcmwfCanaryRevisionSavepointCommand,
    ) -> Result<(), Self::Error>;

    /// Read the complete Best Match product visible at the logical receipt.
    fn select_best_match_rows(
        &mut self,
        query: &EcmwfCanaryBestMatchQuery,
    ) -> Result<Vec<AdmittedEcmwfCanaryBestMatchRow>, Self::Error>;

    /// Stage cold bytes and bindings inside the serving transaction.
    fn stage_ecmwf_temperature_revision(
        &mut self,
        revision: &PersistedEcmwfTemperatureRevision,
    ) -> Result<Option<Self::Publication>, Self::Error>;

    /// Publish one staged revision only after the serving commit succeeds.
    fn publish_ecmwf_temperature_revision(
        &mut self,
        publication: Self::Publication,
    ) -> Result<(), Self::Error>;
}

/// Persist one admitted immutable run and its exact 18 hours atomically.
pub fn persist_ecmwf_temperature_canary_run<Port: EcmwfCanaryPersistencePort>(
    input: &AdmittedEcmwfCanaryRunPersistence,
    port: &mut Port,
) -> Result<EcmwfTemperatureCanaryRun, EcmwfCanaryPersistenceError<Port::Error>> {
    validate_admitted_input(input).map_err(EcmwfCanaryPersistenceError::Input)?;
    persist_with_archive_strategy(input, port, &mut DisabledEcmwfCanaryRevisionArchive)
}

/// Persist one admitted run and optionally stage its exact cold revision.
pub fn persist_ecmwf_temperature_canary_run_with_revision_archive<
    Port: EcmwfCanaryRevisionArchivePort,
>(
    input: &AdmittedEcmwfCanaryRunPersistence,
    revision_window: &AdmittedEcmwfCanaryRevisionWindow,
    port: &mut Port,
) -> Result<EcmwfTemperatureCanaryRun, EcmwfCanaryPersistenceError<Port::Error>> {
    validate_admitted_input(input).map_err(EcmwfCanaryPersistenceError::Input)?;
    validate_revision_window(input, revision_window).map_err(EcmwfCanaryPersistenceError::Input)?;
    let mut archive = EnabledEcmwfCanaryRevisionArchive { revision_window };
    persist_with_archive_strategy(input, port, &mut archive)
}

/// Execute and release one retained client with the selected archive strategy.
fn persist_with_archive_strategy<Port, Archive>(
    input: &AdmittedEcmwfCanaryRunPersistence,
    port: &mut Port,
    archive: &mut Archive,
) -> Result<EcmwfTemperatureCanaryRun, EcmwfCanaryPersistenceError<Port::Error>>
where
    Port: EcmwfCanaryPersistencePort,
    Archive: EcmwfCanaryRevisionArchiveStrategy<Port>,
{
    let result = persist_on_retained_client(input, port, archive);
    port.release_client();
    result
}

/// Execute BEGIN through commit or rollback on one retained client.
fn persist_on_retained_client<Port, Archive>(
    input: &AdmittedEcmwfCanaryRunPersistence,
    port: &mut Port,
    archive: &mut Archive,
) -> Result<EcmwfTemperatureCanaryRun, EcmwfCanaryPersistenceError<Port::Error>>
where
    Port: EcmwfCanaryPersistencePort,
    Archive: EcmwfCanaryRevisionArchiveStrategy<Port>,
{
    port.begin_transaction()
        .map_err(EcmwfCanaryPersistenceError::Command)?;
    let operation = persist_transaction_body(input, port).and_then(|persisted| {
        archive
            .stage(&persisted, input, port)
            .map(|publication| (persisted.stored, publication))
    });

    // mirror withTransaction rollback for every body or COMMIT failure
    match operation {
        Ok((stored, publication)) => {
            // keep COMMIT inside the same failure boundary as the source helper
            if let Err(error) = port.commit_transaction() {
                return rollback_after_failure(port, EcmwfCanaryPersistenceError::Command(error));
            }
            archive.publish_after_commit(publication, port);
            Ok(project_stored_run(stored))
        }
        Err(error) => rollback_after_failure(port, error),
    }
}

/// One completed run transaction before optional archive staging.
#[derive(Debug, Clone, PartialEq)]
struct PersistedEcmwfCanaryTransaction {
    newly_inserted: bool,
    site: EcmwfCanarySiteRow,
    stored: StoredEcmwfCanaryRun,
}

/// Execute the shared run and hour transaction body.
fn persist_transaction_body<Port: EcmwfCanaryPersistencePort>(
    input: &AdmittedEcmwfCanaryRunPersistence,
    port: &mut Port,
) -> Result<PersistedEcmwfCanaryTransaction, EcmwfCanaryPersistenceError<Port::Error>> {
    let site = port
        .select_site(&site_query(input))
        .map_err(EcmwfCanaryPersistenceError::Command)?
        .ok_or(EcmwfCanaryPersistenceError::SiteMissing)?;
    let inserted = port
        .insert_run(&run_insert_query(input, &site.id))
        .map_err(EcmwfCanaryPersistenceError::Command)?;

    // insert hours only for a newly claimed initialization
    if let Some(stored) = inserted {
        for hour in &input.hours {
            port.insert_hour(&hour_insert_query(&stored.id, hour))
                .map_err(EcmwfCanaryPersistenceError::Command)?;
        }
        return Ok(PersistedEcmwfCanaryTransaction {
            newly_inserted: true,
            site,
            stored,
        });
    }

    let stored = port
        .select_existing_run(&existing_run_query(&site.id, input))
        .map_err(EcmwfCanaryPersistenceError::Command)?
        .ok_or(EcmwfCanaryPersistenceError::ExistingRunMissing)?;
    // reject provider revisions and causal-state drift by the frozen digest
    if stored.content_hash != input.content_hash {
        return Err(EcmwfCanaryPersistenceError::RevisionRejected);
    }
    let existing_hours = port
        .select_existing_hours(&existing_hours_query(&stored.id))
        .map_err(EcmwfCanaryPersistenceError::Command)?;
    // reject incomplete or altered persisted hours in lead order
    if existing_hours.len() != input.hours.len()
        || existing_hours
            .iter()
            .zip(&input.hours)
            .any(|(existing, expected)| existing.content_hash != expected.content_hash)
    {
        return Err(EcmwfCanaryPersistenceError::StoredHoursMismatch);
    }
    port.update_last_received(&receipt_update_query(&stored.id, input))
        .map_err(EcmwfCanaryPersistenceError::Command)?;
    Ok(PersistedEcmwfCanaryTransaction {
        newly_inserted: false,
        site,
        stored,
    })
}

/// Internal archive selection preserving one shared transaction engine.
trait EcmwfCanaryRevisionArchiveStrategy<Port: EcmwfCanaryPersistencePort> {
    type Publication;

    /// Stage optional archive work before commit.
    fn stage(
        &mut self,
        persisted: &PersistedEcmwfCanaryTransaction,
        input: &AdmittedEcmwfCanaryRunPersistence,
        port: &mut Port,
    ) -> Result<Option<Self::Publication>, EcmwfCanaryPersistenceError<Port::Error>>;

    /// Publish only after the shared engine commits.
    fn publish_after_commit(&mut self, publication: Option<Self::Publication>, port: &mut Port);
}

/// Zero-work strategy for the ordinary no-archiver fast path.
struct DisabledEcmwfCanaryRevisionArchive;

impl<Port: EcmwfCanaryPersistencePort> EcmwfCanaryRevisionArchiveStrategy<Port>
    for DisabledEcmwfCanaryRevisionArchive
{
    type Publication = ();

    /// Preserve the source branch with no clock, query, or savepoint work.
    fn stage(
        &mut self,
        _persisted: &PersistedEcmwfCanaryTransaction,
        _input: &AdmittedEcmwfCanaryRunPersistence,
        _port: &mut Port,
    ) -> Result<Option<Self::Publication>, EcmwfCanaryPersistenceError<Port::Error>> {
        Ok(None)
    }

    /// Perform no publication work when no archiver was supplied.
    fn publish_after_commit(&mut self, _publication: Option<Self::Publication>, _port: &mut Port) {}
}

/// Enabled archive strategy carrying the already admitted source window.
struct EnabledEcmwfCanaryRevisionArchive<'a> {
    revision_window: &'a AdmittedEcmwfCanaryRevisionWindow,
}

impl<Port: EcmwfCanaryRevisionArchivePort> EcmwfCanaryRevisionArchiveStrategy<Port>
    for EnabledEcmwfCanaryRevisionArchive<'_>
{
    type Publication = Port::Publication;

    /// Stage only a newly inserted source revision inside one savepoint.
    fn stage(
        &mut self,
        persisted: &PersistedEcmwfCanaryTransaction,
        input: &AdmittedEcmwfCanaryRunPersistence,
        port: &mut Port,
    ) -> Result<Option<Self::Publication>, EcmwfCanaryPersistenceError<Port::Error>> {
        // never backfill an archive pointer on an identical retry
        if !persisted.newly_inserted {
            return Ok(None);
        }
        stage_ecmwf_canary_revision(persisted, input, self.revision_window, port)
    }

    /// Attempt permanent publication only after the database commit.
    fn publish_after_commit(&mut self, publication: Option<Self::Publication>, port: &mut Port) {
        // preserve serving success after a permanent cold publication gap
        if let Some(publication) = publication {
            let _publication_result = port.publish_ecmwf_temperature_revision(publication);
        }
    }
}

/// Stage one exact newly inserted revision and recover ordinary archive failure.
fn stage_ecmwf_canary_revision<Port: EcmwfCanaryRevisionArchivePort>(
    persisted: &PersistedEcmwfCanaryTransaction,
    input: &AdmittedEcmwfCanaryRunPersistence,
    revision_window: &AdmittedEcmwfCanaryRevisionWindow,
    port: &mut Port,
) -> Result<Option<Port::Publication>, EcmwfCanaryPersistenceError<Port::Error>> {
    let logical_received_at = port
        .select_logical_receipt(&logical_receipt_query())
        .map_err(EcmwfCanaryPersistenceError::Command)?
        .ok_or(EcmwfCanaryPersistenceError::LogicalReceiptMissing)?;
    port.execute_revision_savepoint(EcmwfCanaryRevisionSavepointCommand::Savepoint)
        .map_err(EcmwfCanaryPersistenceError::Command)?;

    let staged = stage_ecmwf_canary_revision_after_savepoint(
        persisted,
        input,
        revision_window,
        &logical_received_at,
        port,
    );

    // remove partial archive SQL and cold writes on any ordinary stage failure
    match staged {
        Ok(publication) => Ok(publication),
        Err(_) => {
            recover_ecmwf_canary_revision_savepoint(port)?;
            Ok(None)
        }
    }
}

/// Execute the source try-block after the savepoint is established.
fn stage_ecmwf_canary_revision_after_savepoint<Port: EcmwfCanaryRevisionArchivePort>(
    persisted: &PersistedEcmwfCanaryTransaction,
    input: &AdmittedEcmwfCanaryRunPersistence,
    revision_window: &AdmittedEcmwfCanaryRevisionWindow,
    logical_received_at: &AdmittedRevisionUtcInstant,
    port: &mut Port,
) -> Result<Option<Port::Publication>, Port::Error> {
    let candidates = port.select_best_match_rows(&best_match_query(
        &input.site_slug,
        revision_window,
        logical_received_at,
    ))?;
    let best_match_rows = qualify_best_match_rows(candidates, revision_window);
    let revision = persisted_revision(persisted, input, logical_received_at, best_match_rows);
    let publication = port.stage_ecmwf_temperature_revision(&revision)?;
    port.execute_revision_savepoint(EcmwfCanaryRevisionSavepointCommand::Release)?;
    Ok(publication)
}

/// Roll back and release the archive savepoint on the same retained port.
fn recover_ecmwf_canary_revision_savepoint<Port: EcmwfCanaryRevisionArchivePort>(
    port: &mut Port,
) -> Result<(), EcmwfCanaryPersistenceError<Port::Error>> {
    port.execute_revision_savepoint(EcmwfCanaryRevisionSavepointCommand::RollbackTo)
        .map_err(EcmwfCanaryPersistenceError::Command)?;
    port.execute_revision_savepoint(EcmwfCanaryRevisionSavepointCommand::Release)
        .map_err(EcmwfCanaryPersistenceError::Command)
}

/// Validate transaction-critical geometry before issuing BEGIN.
fn validate_admitted_input(
    input: &AdmittedEcmwfCanaryRunPersistence,
) -> Result<(), EcmwfCanaryPersistenceInputError> {
    // retain the source receipt ordering before any database command
    if input.received_at_epoch_milliseconds < input.run_initialized_at_epoch_milliseconds
        || input
            .run_initialized_at_epoch_milliseconds
            .rem_euclid(6 * HOUR_MILLISECONDS)
            != 0
    {
        return Err(EcmwfCanaryPersistenceInputError::InvalidReceipt);
    }
    let expected_cycle =
        if input.run_initialized_at_epoch_milliseconds < ECMWF_50R1_CUTOVER_MILLISECONDS {
            EcmwfTemperatureCanaryRunModelCycleEnum::Value49r1
        } else {
            EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1
        };
    // reject model-era drift before the transaction
    if input.model_cycle != expected_cycle {
        return Err(EcmwfCanaryPersistenceInputError::InvalidModelCycle);
    }
    // require one bounded source explanation before writing the run
    if input.state_reason.trim().is_empty() || input.state_reason.len() > 128 {
        return Err(EcmwfCanaryPersistenceInputError::InvalidStateReason);
    }
    validate_recent_error_state(input)?;
    let status_supported =
        input.state_status == EcmwfTemperatureCanaryRunStateStatusEnum::Supported;
    // bind stored status to the already admitted causal state
    if status_supported != input.recent_error_state.supported
        || (input.state_status == EcmwfTemperatureCanaryRunStateStatusEnum::Cold
            && input.recent_error_state.n72 != 0)
        || (input.state_status == EcmwfTemperatureCanaryRunStateStatusEnum::Insufficient
            && input.recent_error_state.n72 == 0)
    {
        return Err(EcmwfCanaryPersistenceInputError::InvalidStateStatus);
    }
    // require every exact lead once before validating its metric row
    if input.hours.len() != 18
        || input
            .hours
            .iter()
            .enumerate()
            .any(|(index, hour)| hour.model_lead_hours != (index + 1) as u64)
    {
        return Err(EcmwfCanaryPersistenceInputError::InvalidHourCount);
    }
    // bind each finite metric row to its exact initialized forecast hour
    if input.hours.iter().enumerate().any(|(index, hour)| {
        let lead = u64::try_from(index + 1).expect("18 leads fit u64");
        let expected_valid_at = i64::try_from(lead)
            .ok()
            .and_then(|lead| lead.checked_mul(HOUR_MILLISECONDS))
            .and_then(|offset| {
                input
                    .run_initialized_at_epoch_milliseconds
                    .checked_add(offset)
            });
        expected_valid_at != Some(hour.valid_at_epoch_milliseconds)
            || !hour.raw_temperature_c.is_finite()
            || !(-100.0..=70.0).contains(&hour.raw_temperature_c)
            || !nullable_in_range(&hour.raw_relative_humidity_percent, 0.0, 100.0)
            || !nullable_in_range(&hour.raw_wind_speed_mps, 0.0, 150.0)
    }) {
        return Err(EcmwfCanaryPersistenceInputError::InvalidHour);
    }
    Ok(())
}

/// Validate the source-derived 7-through-18 lead archive window.
fn validate_revision_window(
    input: &AdmittedEcmwfCanaryRunPersistence,
    revision_window: &AdmittedEcmwfCanaryRevisionWindow,
) -> Result<(), EcmwfCanaryPersistenceInputError> {
    let expected_from = input
        .run_initialized_at_epoch_milliseconds
        .checked_add(7 * HOUR_MILLISECONDS);
    let expected_to = input
        .run_initialized_at_epoch_milliseconds
        .checked_add(19 * HOUR_MILLISECONDS);
    // reject mismatched admitted strings indirectly through their bound hour rows
    if expected_from != Some(revision_window.from_epoch_milliseconds)
        || expected_to != Some(revision_window.to_epoch_milliseconds)
        || input.hours.get(6).map(|hour| hour.valid_at.as_str())
            != Some(revision_window.from.as_str())
        || revision_window.from.is_empty()
        || revision_window.to.is_empty()
    {
        return Err(EcmwfCanaryPersistenceInputError::InvalidRevisionWindow);
    }
    Ok(())
}

/// Validate the pure causal-state structure using admitted epoch evidence.
fn validate_recent_error_state(
    input: &AdmittedEcmwfCanaryRunPersistence,
) -> Result<(), EcmwfCanaryPersistenceInputError> {
    let state = &input.recent_error_state;
    let expected_window_end = input
        .run_initialized_at_epoch_milliseconds
        .checked_sub(7 * HOUR_MILLISECONDS);
    let unique_keys = state.sourceKeys.iter().collect::<BTreeSet<_>>();
    let short_supported = state.n24 >= 6;
    let long_supported = state.n72 >= 24;
    let expected_supported = short_supported && long_supported && state.localDates >= 2;
    // require the exact count, support, source-key, and statistic geometry
    if state.targetRunInitializedAt != input.run_initialized_at
        || state.windowEndValidAt != input.recent_error_state_window_end_valid_at
        || expected_window_end != Some(input.recent_error_state_window_end_epoch_milliseconds)
        || state.n24 > 24
        || state.n72 < state.n24
        || state.n72 > 72
        || state.localDates > state.n72
        || usize::try_from(state.n72).ok() != Some(state.sourceKeys.len())
        || state.sourceKeys.iter().any(String::is_empty)
        || unique_keys.len() != state.sourceKeys.len()
        || state.supported != expected_supported
        || !bounded_nullable_statistic(&state.b24C, short_supported, -6.0, 6.0)
        || !bounded_nullable_statistic(&state.b72C, long_supported, -6.0, 6.0)
        || !bounded_nullable_statistic(&state.mad72C, long_supported, 0.0, 6.0)
    {
        return Err(EcmwfCanaryPersistenceInputError::InvalidRecentErrorState);
    }
    let stored_valid_maximum = matches!(state.maximumSourceValidAt, Nullable::Value(_));
    let stored_run_maximum = matches!(state.maximumSourceRunInitializedAt, Nullable::Value(_));
    let stored_maxima_present = stored_valid_maximum && stored_run_maximum;
    let epoch_maxima = input
        .recent_error_state_maximum_valid_epoch_milliseconds
        .zip(input.recent_error_state_maximum_run_epoch_milliseconds);
    // require both stored and admitted maxima exactly when evidence exists
    if stored_valid_maximum != stored_run_maximum
        || stored_maxima_present != (state.n72 > 0)
        || epoch_maxima.is_some() != stored_maxima_present
    {
        return Err(EcmwfCanaryPersistenceInputError::InvalidRecentErrorState);
    }
    // preserve all causal maximum-source bounds without implementing a time codec
    if let Some((maximum_valid, maximum_run)) = epoch_maxima {
        let window_end = input.recent_error_state_window_end_epoch_milliseconds;
        let earliest = window_end.checked_sub(71 * HOUR_MILLISECONDS);
        let earliest_short = window_end.checked_sub(23 * HOUR_MILLISECONDS);
        let window_age = window_end.checked_sub(maximum_valid);
        let source_age = maximum_valid.checked_sub(maximum_run);
        if maximum_valid > window_end
            || earliest.is_none_or(|minimum| maximum_valid < minimum)
            || (state.n24 > 0 && earliest_short.is_none_or(|minimum| maximum_valid < minimum))
            || window_age.is_none_or(|age| age.rem_euclid(HOUR_MILLISECONDS) != 0)
            || maximum_run >= input.run_initialized_at_epoch_milliseconds
            || maximum_run.rem_euclid(6 * HOUR_MILLISECONDS) != 0
            || source_age
                .is_none_or(|age| !(7 * HOUR_MILLISECONDS..=18 * HOUR_MILLISECONDS).contains(&age))
        {
            return Err(EcmwfCanaryPersistenceInputError::InvalidRecentErrorState);
        }
    }
    Ok(())
}

/// Validate one nullable statistic and its support-dependent presence.
fn bounded_nullable_statistic(
    value: &Nullable<f64>,
    required: bool,
    minimum: f64,
    maximum: f64,
) -> bool {
    match value {
        Nullable::Value(value) => {
            required && value.is_finite() && (minimum..=maximum).contains(value)
        }
        Nullable::Null => !required,
    }
}

/// Check one optional admitted metric against its source range.
fn nullable_in_range(value: &Nullable<f64>, minimum: f64, maximum: f64) -> bool {
    match value {
        Nullable::Value(value) => value.is_finite() && (minimum..=maximum).contains(value),
        Nullable::Null => true,
    }
}

/// Build the active-site identity query.
fn site_query(input: &AdmittedEcmwfCanaryRunPersistence) -> EcmwfCanarySiteQuery {
    EcmwfCanarySiteQuery {
        binds: [EcmwfCanaryPersistenceBind::Text(input.site_slug.clone())],
        sql: ECMWF_CANARY_SITE_SQL,
    }
}

/// Build the immutable run insert query.
fn run_insert_query(
    input: &AdmittedEcmwfCanaryRunPersistence,
    site_id: &str,
) -> EcmwfCanaryRunInsertQuery {
    EcmwfCanaryRunInsertQuery {
        binds: [
            EcmwfCanaryPersistenceBind::Text(site_id.to_owned()),
            EcmwfCanaryPersistenceBind::UtcInstant(input.run_initialized_at.clone()),
            EcmwfCanaryPersistenceBind::UtcInstant(input.received_at.clone()),
            EcmwfCanaryPersistenceBind::Text(
                EcmwfTemperatureCanaryRunUpstreamModelLiteral::WIRE_VALUE.to_owned(),
            ),
            EcmwfCanaryPersistenceBind::Text(
                EcmwfTemperatureCanaryRunAdapterVersionLiteral::WIRE_VALUE.to_owned(),
            ),
            EcmwfCanaryPersistenceBind::Digest(input.provider_response_sha256.clone()),
            EcmwfCanaryPersistenceBind::Text(model_cycle_wire(input.model_cycle).to_owned()),
            EcmwfCanaryPersistenceBind::Json(input.recent_error_state_json.clone()),
            EcmwfCanaryPersistenceBind::Digest(input.recent_error_state_sha256.clone()),
            EcmwfCanaryPersistenceBind::Text(state_status_wire(input.state_status).to_owned()),
            EcmwfCanaryPersistenceBind::Text(input.state_reason.clone()),
            EcmwfCanaryPersistenceBind::Digest(input.content_hash.clone()),
        ],
        sql: ECMWF_CANARY_INSERT_RUN_SQL,
    }
}

/// Build one immutable lead insert query.
fn hour_insert_query(run_id: &str, hour: &PreparedEcmwfCanaryHour) -> EcmwfCanaryHourInsertQuery {
    EcmwfCanaryHourInsertQuery {
        binds: [
            EcmwfCanaryPersistenceBind::Text(run_id.to_owned()),
            EcmwfCanaryPersistenceBind::UtcInstant(hour.valid_at.clone()),
            EcmwfCanaryPersistenceBind::Count(hour.model_lead_hours),
            EcmwfCanaryPersistenceBind::Number(hour.raw_temperature_c),
            EcmwfCanaryPersistenceBind::NullableNumber(nullable_number(
                &hour.raw_relative_humidity_percent,
            )),
            EcmwfCanaryPersistenceBind::NullableNumber(nullable_number(&hour.raw_wind_speed_mps)),
            EcmwfCanaryPersistenceBind::Digest(hour.content_hash.clone()),
        ],
        sql: ECMWF_CANARY_INSERT_HOUR_SQL,
    }
}

/// Build the locked existing-run query.
fn existing_run_query(
    site_id: &str,
    input: &AdmittedEcmwfCanaryRunPersistence,
) -> EcmwfCanaryExistingRunQuery {
    EcmwfCanaryExistingRunQuery {
        binds: [
            EcmwfCanaryPersistenceBind::Text(site_id.to_owned()),
            EcmwfCanaryPersistenceBind::UtcInstant(input.run_initialized_at.clone()),
        ],
        sql: ECMWF_CANARY_EXISTING_RUN_SQL,
    }
}

/// Build the existing-hour identity query.
fn existing_hours_query(run_id: &str) -> EcmwfCanaryExistingHoursQuery {
    EcmwfCanaryExistingHoursQuery {
        binds: [EcmwfCanaryPersistenceBind::Text(run_id.to_owned())],
        sql: ECMWF_CANARY_EXISTING_HOURS_SQL,
    }
}

/// Build the monotonic receipt update query.
fn receipt_update_query(
    run_id: &str,
    input: &AdmittedEcmwfCanaryRunPersistence,
) -> EcmwfCanaryReceiptUpdateQuery {
    EcmwfCanaryReceiptUpdateQuery {
        binds: [
            EcmwfCanaryPersistenceBind::Text(run_id.to_owned()),
            EcmwfCanaryPersistenceBind::UtcInstant(input.received_at.clone()),
        ],
        sql: ECMWF_CANARY_UPDATE_RECEIPT_SQL,
    }
}

/// Build the zero-bind database clock read.
fn logical_receipt_query() -> EcmwfCanaryLogicalReceiptQuery {
    EcmwfCanaryLogicalReceiptQuery {
        sql: ECMWF_CANARY_LOGICAL_RECEIPT_SQL,
    }
}

/// Build the exact 12-hour Best Match query at one logical receipt.
fn best_match_query(
    site_slug: &str,
    revision_window: &AdmittedEcmwfCanaryRevisionWindow,
    logical_received_at: &AdmittedRevisionUtcInstant,
) -> EcmwfCanaryBestMatchQuery {
    EcmwfCanaryBestMatchQuery {
        binds: [
            EcmwfCanaryPersistenceBind::Text(site_slug.to_owned()),
            EcmwfCanaryPersistenceBind::UtcInstant(revision_window.from.clone()),
            EcmwfCanaryPersistenceBind::UtcInstant(revision_window.to.clone()),
            EcmwfCanaryPersistenceBind::UtcInstant(logical_received_at.as_str().to_owned()),
        ],
        sql: ECMWF_CANARY_BEST_MATCH_SQL,
    }
}

/// Return only one complete same-source, same-product hourly matrix.
fn qualify_best_match_rows(
    candidates: Vec<AdmittedEcmwfCanaryBestMatchRow>,
    revision_window: &AdmittedEcmwfCanaryRevisionWindow,
) -> Vec<WeatherRecordRow> {
    let Some(first) = candidates.first() else {
        return Vec::new();
    };
    let Some(first_product_run) = first.product_run_at_epoch_milliseconds else {
        return Vec::new();
    };
    // mirror the source's fail-closed 12-hour geometry check
    if candidates.len() != 12
        || candidates.iter().enumerate().any(|(index, candidate)| {
            let expected_valid_at = i64::try_from(index)
                .ok()
                .and_then(|index| index.checked_mul(HOUR_MILLISECONDS))
                .and_then(|offset| revision_window.from_epoch_milliseconds.checked_add(offset));
            candidate.row.sourceId != first.row.sourceId
                || candidate.product_run_at_epoch_milliseconds != Some(first_product_run)
                || expected_valid_at != Some(candidate.valid_at_epoch_milliseconds)
                || candidate.row.contentHash != candidate.content_hash.as_str()
        })
    {
        return Vec::new();
    }
    candidates
        .into_iter()
        .map(|candidate| candidate.row)
        .collect()
}

/// Build the generated canonical revision passed to the archive adapter.
fn persisted_revision(
    persisted: &PersistedEcmwfCanaryTransaction,
    input: &AdmittedEcmwfCanaryRunPersistence,
    logical_received_at: &AdmittedRevisionUtcInstant,
    best_match_rows: Vec<WeatherRecordRow>,
) -> PersistedEcmwfTemperatureRevision {
    PersistedEcmwfTemperatureRevision {
        adapterVersion: input.adapter_version,
        bestMatchRows: best_match_rows,
        contentHash: persisted.stored.content_hash.as_str().to_owned(),
        hours: input.hours.iter().map(project_prepared_hour).collect(),
        logicalReceivedAt: logical_received_at.as_str().to_owned(),
        modelCycle: input.model_cycle,
        providerResponseSha256: persisted
            .stored
            .provider_response_sha256
            .as_str()
            .to_owned(),
        recentErrorState: input.recent_error_state.clone(),
        recentErrorStateSha256: input.recent_error_state_sha256.as_str().to_owned(),
        runInitializedAt: persisted.stored.run_initialized_at.clone(),
        siteId: persisted.site.id.clone(),
        siteLatitude: persisted.site.latitude,
        siteLongitude: persisted.site.longitude,
        siteSlug: input.site_slug.clone(),
        upstreamModel: input.upstream_model,
    }
}

/// Project one admitted prepared hour into its generated canonical shape.
fn project_prepared_hour(hour: &PreparedEcmwfCanaryHour) -> EcmwfTemperatureCanaryHour {
    EcmwfTemperatureCanaryHour {
        contentHash: hour.content_hash.as_str().to_owned(),
        modelLeadHours: hour.model_lead_hours,
        rawRelativeHumidityPercent: hour.raw_relative_humidity_percent.clone(),
        rawTemperatureC: hour.raw_temperature_c,
        rawWindSpeedMps: hour.raw_wind_speed_mps.clone(),
        validAt: hour.valid_at.clone(),
    }
}

/// Project one trusted stored row to the generated public repository result.
fn project_stored_run(stored: StoredEcmwfCanaryRun) -> EcmwfTemperatureCanaryRun {
    EcmwfTemperatureCanaryRun {
        adapterVersion: stored.adapter_version,
        firstReceivedAt: stored.first_received_at,
        id: stored.id,
        modelCycle: stored.model_cycle,
        providerResponseSha256: stored.provider_response_sha256.as_str().to_owned(),
        recentErrorState: stored.recent_error_state,
        runInitializedAt: stored.run_initialized_at,
        stateReason: stored.state_reason,
        stateStatus: stored.state_status,
        upstreamModel: stored.upstream_model,
    }
}

/// Convert one generated nullable metric to its PostgreSQL scalar bind.
fn nullable_number(value: &Nullable<f64>) -> Option<f64> {
    match value {
        Nullable::Value(value) => Some(*value),
        Nullable::Null => None,
    }
}

/// Return the generated model-cycle wire value.
fn model_cycle_wire(value: EcmwfTemperatureCanaryRunModelCycleEnum) -> &'static str {
    match value {
        EcmwfTemperatureCanaryRunModelCycleEnum::Value49r1 => "49r1",
        EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1 => "50r1",
    }
}

/// Return the generated state-status wire value.
fn state_status_wire(value: EcmwfTemperatureCanaryRunStateStatusEnum) -> &'static str {
    match value {
        EcmwfTemperatureCanaryRunStateStatusEnum::Supported => "supported",
        EcmwfTemperatureCanaryRunStateStatusEnum::Cold => "cold",
        EcmwfTemperatureCanaryRunStateStatusEnum::Insufficient => "insufficient",
        EcmwfTemperatureCanaryRunStateStatusEnum::Invalid => "invalid",
    }
}

/// Roll back while retaining the source helper's rollback-error precedence.
fn rollback_after_failure<Port: EcmwfCanaryPersistencePort>(
    port: &mut Port,
    failure: EcmwfCanaryPersistenceError<Port::Error>,
) -> Result<EcmwfTemperatureCanaryRun, EcmwfCanaryPersistenceError<Port::Error>> {
    match port.rollback_transaction() {
        Ok(()) => Err(failure),
        Err(rollback_error) => Err(EcmwfCanaryPersistenceError::Command(rollback_error)),
    }
}
