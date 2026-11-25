//! Typed Tempest backfill planning and execution without transport, JSON, or date codecs.

use std::collections::BTreeSet;

use super::public_station_backfill::{PublicStationBackfillDate, PublicStationBackfillDeadlines};
use super::tide_backfill::{
    TideBackfillErrorClassification, TideBackfillIngestionError, TideBackfillOperationError,
    TideBackfillProviderFailure, as_backfill_provider_failure,
};
use super::{
    NormalizedTempestRecord, ProviderBatch, TEMPEST_OBSERVATION_ADAPTER_VERSION,
    TEMPEST_OBSERVATION_CHUNK_PLAN_VERSION, TEMPEST_OBSERVATION_ENDPOINT,
    TEMPEST_SOURCE_CONTRACT_VERSION, TaskFailure, TempestObservationRequest,
    TempestResponseMetadata, TempestRuntimeConfiguration, TempestScheduledSource,
    tempest_source_matches_configuration,
};

const MILLISECONDS_PER_DAY: i64 = 86_400_000;
const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

pub type TempestBackfillDate = PublicStationBackfillDate;
pub type TempestBackfillDeadlines = PublicStationBackfillDeadlines;
pub type TempestBackfillErrorClassification = TideBackfillErrorClassification;
pub type TempestBackfillIngestionError = TideBackfillIngestionError;
pub type TempestBackfillOperationError = TideBackfillOperationError;
pub type TempestBackfillProviderFailure = TideBackfillProviderFailure;

/// Time operations required by planning and committed Tempest chunk execution.
pub trait TempestBackfillTimeAdapter {
    /// Return the current admitted UTC date used to exclude the partial day.
    fn today_utc(&mut self) -> Result<TempestBackfillDate, TaskFailure>;

    /// Add exact UTC calendar days through an admitted date implementation.
    fn add_utc_days(
        &mut self,
        date: &TempestBackfillDate,
        days: i64,
    ) -> Result<TempestBackfillDate, TaskFailure>;

    /// Return one bounded provider and durable-run deadline plan.
    fn deadlines(&mut self) -> Result<TempestBackfillDeadlines, TempestBackfillOperationError>;
}

/// One typed one-shot Tempest bulk backfill request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TempestBackfillArguments {
    pub chunk_days: u16,
    pub dry_run: bool,
    pub from: Option<TempestBackfillDate>,
    pub resume: bool,
    pub site: String,
    pub station_ids: Vec<u64>,
    pub to: TempestBackfillDate,
}

/// One checked Tempest station joined to its active storage identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TempestBackfillSource {
    pub configuration: TempestRuntimeConfiguration,
    pub history_start: TempestBackfillDate,
    pub provider_active: bool,
    pub source: TempestScheduledSource,
    pub station_active: bool,
    pub storage_active: bool,
}

/// One exact durable Tempest chunk identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TempestBackfillChunkIdentity {
    pub adapter_version: &'static str,
    pub chunk_plan_version: &'static str,
    pub interval_end_exclusive: TempestBackfillDate,
    pub interval_start: TempestBackfillDate,
    pub requested_from_date: String,
    pub requested_to_date: String,
    pub source_config_fingerprint: String,
    pub source_id: String,
}

/// One exact one-day Tempest chunk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedTempestBackfillChunk {
    pub end_date: TempestBackfillDate,
    pub identity: TempestBackfillChunkIdentity,
    pub start_date: TempestBackfillDate,
}

/// Metadata committed before one Tempest provider request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TempestBackfillRunStart {
    pub adapter_version: &'static str,
    pub chunk_plan_version: &'static str,
    pub deadline_at_ms: i64,
    pub device_id: u64,
    pub endpoint: &'static str,
    pub location_id: u64,
    pub requested_end_exclusive_ms: i64,
    pub requested_start_ms: i64,
    pub source_config_fingerprint: String,
}

/// One successful atomic Tempest backfill completion.
#[derive(Clone, Debug, PartialEq)]
pub struct TempestBackfillCompletion {
    pub attempts: u32,
    pub identity: TempestBackfillChunkIdentity,
    pub records: Vec<NormalizedTempestRecord>,
    pub response_metadata: TempestResponseMetadata,
    pub run_id: String,
    pub upstream_response_checksum: String,
}

/// Persistence required by one typed Tempest bulk backfill.
pub trait TempestBackfillRepository {
    type Session;

    /// Report whether this exact durable chunk already succeeded.
    fn has_successful_chunk(
        &mut self,
        identity: &TempestBackfillChunkIdentity,
    ) -> Result<bool, TaskFailure>;

    /// Acquire one source-scoped session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure>;

    /// Abandon expired runs before committing a new run.
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        now_ms: i64,
    ) -> Result<(), TempestBackfillOperationError>;

    /// Commit one running Tempest backfill row.
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: TempestBackfillRunStart,
    ) -> Result<String, TempestBackfillOperationError>;

    /// Commit actual records and successful chunk identity atomically.
    fn complete_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: TempestBackfillCompletion,
    ) -> Result<(), TempestBackfillOperationError>;

    /// Finalize one committed failed Tempest chunk.
    fn fail_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        run_id: &str,
        identity: &TempestBackfillChunkIdentity,
        failure: &TempestBackfillProviderFailure,
    ) -> Result<(), TaskFailure>;

    /// Release one source-scoped session.
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure>;
}

/// One typed Tempest provider operation required by the backfill loop.
pub trait TempestBackfillProvider {
    /// Fetch one normalized Tempest batch.
    fn fetch(
        &mut self,
        request: TempestObservationRequest,
        deadline_at_ms: i64,
    ) -> Result<
        ProviderBatch<NormalizedTempestRecord, TempestResponseMetadata, Option<i64>>,
        TempestBackfillOperationError,
    >;
}

/// One Tempest chunk execution status retained in the bulk report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TempestBackfillChunkStatus {
    Abandoned,
    Completed,
    Failed,
    Planned,
    Skipped,
}

/// One exact Tempest chunk report.
#[derive(Clone, Debug, PartialEq)]
pub struct TempestBackfillChunkReport {
    pub error_code: Option<String>,
    pub identity: TempestBackfillChunkIdentity,
    pub record_count: usize,
    pub secondary_error_codes: Vec<String>,
    pub status: TempestBackfillChunkStatus,
}

/// One station-level Tempest backfill report.
#[derive(Clone, Debug, PartialEq)]
pub struct TempestStationBackfillReport {
    pub chunks: Vec<TempestBackfillChunkReport>,
    pub exit_code: u8,
    pub from: String,
    pub location_id: u64,
    pub source: String,
    pub to: String,
}

/// One complete one-shot Tempest bulk backfill report.
#[derive(Clone, Debug, PartialEq)]
pub struct TempestBulkBackfillReport {
    pub dry_run: bool,
    pub exit_code: u8,
    pub resume: bool,
    pub site: String,
    pub stations: Vec<TempestStationBackfillReport>,
}

/// Resolve selected active Tempest sources in checked station order.
pub fn select_tempest_backfill_sources<'a>(
    arguments: &TempestBackfillArguments,
    configuration_site: &str,
    candidates: &'a [TempestBackfillSource],
) -> Result<Vec<&'a TempestBackfillSource>, TaskFailure> {
    // reject cross-site execution
    if arguments.site != configuration_site {
        return Err(backfill_failure("tempest_backfill_site_mismatch", 0));
    }
    let selectors = arguments
        .station_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();

    // require the parsed unique positive station selector contract
    if selectors.len() != arguments.station_ids.len()
        || selectors.contains(&0)
        || selectors.iter().any(|value| *value > MAXIMUM_SAFE_INTEGER)
    {
        return Err(backfill_failure("tempest_backfill_selector_invalid", 0));
    }
    let mut selected = Vec::new();

    // preserve checked Tempest station order
    for candidate in candidates {
        let selector_matches =
            selectors.is_empty() || selectors.contains(&candidate.configuration.location_id);

        // exclude inactive or unselected station configuration
        if !candidate.station_active || !selector_matches {
            continue;
        }

        // require the active provider and storage join used by the Node resolver
        if !candidate.provider_active || !candidate.storage_active || !candidate.source.active {
            return Err(backfill_failure("tempest_backfill_source_missing", 0));
        }

        // reject contract or material identity drift before provider work
        if candidate.configuration.adapter_contract_version != TEMPEST_SOURCE_CONTRACT_VERSION
            || candidate.configuration.device_id == 0
            || candidate.configuration.device_id > MAXIMUM_SAFE_INTEGER
            || candidate.configuration.location_id == 0
            || candidate.configuration.location_id > MAXIMUM_SAFE_INTEGER
            || candidate.configuration.serial.trim().is_empty()
            || candidate.configuration.serial.len() > 128
            || !fingerprint_is_valid(&candidate.source.source_config_fingerprint)
            || !tempest_source_matches_configuration(&candidate.source, &candidate.configuration)
        {
            return Err(backfill_failure("tempest_backfill_identity_mismatch", 0));
        }
        selected.push(candidate);
    }

    // reject unknown or inactive station selectors
    if !selectors.is_empty() && selected.len() != selectors.len() {
        return Err(backfill_failure("tempest_backfill_source_unavailable", 0));
    }

    Ok(selected)
}

/// Plan every exact one-day Tempest chunk.
pub fn plan_tempest_backfill_chunks<T: TempestBackfillTimeAdapter>(
    source: &TempestBackfillSource,
    chunk_days: u16,
    from: &TempestBackfillDate,
    to: &TempestBackfillDate,
    time: &mut T,
) -> Result<Vec<PlannedTempestBackfillChunk>, TaskFailure> {
    validate_admitted_date(from)?;
    validate_admitted_date(to)?;

    // preserve provider-native minute density through exact one-day chunks
    if chunk_days != 1 {
        return Err(backfill_failure("tempest_backfill_chunk_range_invalid", 0));
    }
    let mut chunks = Vec::new();
    let mut start_date = from.clone();

    // cover the inclusive requested UTC date range
    while start_date.start_ms() <= to.start_ms() {
        let interval_end_exclusive = add_utc_days(time, &start_date, 1)?;
        let identity = TempestBackfillChunkIdentity {
            adapter_version: TEMPEST_OBSERVATION_ADAPTER_VERSION,
            chunk_plan_version: TEMPEST_OBSERVATION_CHUNK_PLAN_VERSION,
            interval_end_exclusive: interval_end_exclusive.clone(),
            interval_start: start_date.clone(),
            requested_from_date: from.label().to_owned(),
            requested_to_date: to.label().to_owned(),
            source_config_fingerprint: source.source.source_config_fingerprint.clone(),
            source_id: source.source.id.clone(),
        };
        validate_chunk_identity(&identity)?;
        chunks.push(PlannedTempestBackfillChunk {
            end_date: start_date.clone(),
            identity,
            start_date,
        });
        start_date = interval_end_exclusive;
    }

    Ok(chunks)
}

/// Execute one selected Tempest bulk backfill through exact chunk boundaries.
pub fn execute_tempest_bulk_backfill<R, P, T>(
    arguments: &TempestBackfillArguments,
    configuration_site: &str,
    candidates: &[TempestBackfillSource],
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<TempestBulkBackfillReport, TaskFailure>
where
    P: TempestBackfillProvider,
    R: TempestBackfillRepository,
    T: TempestBackfillTimeAdapter,
{
    validate_arguments(arguments, time)?;
    let selected = select_tempest_backfill_sources(arguments, configuration_site, candidates)?;
    let mut reports = Vec::with_capacity(selected.len());

    // isolate each checked Tempest station result
    for source in selected {
        reports.push(execute_station_backfill(
            arguments, source, repository, provider, time,
        )?);
    }
    let exit_code = u8::from(reports.iter().any(|report| report.exit_code == 1));

    Ok(TempestBulkBackfillReport {
        dry_run: arguments.dry_run,
        exit_code,
        resume: arguments.resume,
        site: arguments.site.clone(),
        stations: reports,
    })
}

// execute every exact chunk for one Tempest station
fn execute_station_backfill<R, P, T>(
    arguments: &TempestBackfillArguments,
    source: &TempestBackfillSource,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<TempestStationBackfillReport, TaskFailure>
where
    P: TempestBackfillProvider,
    R: TempestBackfillRepository,
    T: TempestBackfillTimeAdapter,
{
    validate_admitted_date(&source.history_start)?;
    let from = arguments
        .from
        .clone()
        .unwrap_or_else(|| source.history_start.clone());
    let chunks =
        plan_tempest_backfill_chunks(source, arguments.chunk_days, &from, &arguments.to, time)?;
    let mut reports = Vec::new();

    // execute every exact chunk independently in calendar order
    for chunk in chunks {
        // satisfy exact-success resume semantics
        if arguments.resume && repository.has_successful_chunk(&chunk.identity)? {
            reports.push(chunk_report(
                chunk.identity,
                TempestBackfillChunkStatus::Skipped,
            ));
            continue;
        }

        // prove dry-run has no provider or persistence writes
        if arguments.dry_run {
            reports.push(chunk_report(
                chunk.identity,
                TempestBackfillChunkStatus::Planned,
            ));
            continue;
        }
        let Some(session) = repository.acquire_source_session(&source.source.id)? else {
            let mut report = chunk_report(chunk.identity, TempestBackfillChunkStatus::Abandoned);
            report.error_code = Some("source_locked".to_owned());
            reports.push(report);
            break;
        };
        let result = execute_chunk(source, &chunk, session, repository, provider, time)?;
        let completed = result.status == TempestBackfillChunkStatus::Completed;
        reports.push(result);

        // stop this station after the first failed or abandoned chunk
        if !completed {
            break;
        }
    }
    let exit_code = u8::from(reports.iter().any(|report| {
        matches!(
            report.status,
            TempestBackfillChunkStatus::Failed | TempestBackfillChunkStatus::Abandoned
        )
    }));

    Ok(TempestStationBackfillReport {
        chunks: reports,
        exit_code,
        from: from.label().to_owned(),
        location_id: source.configuration.location_id,
        source: source.source.source_key.clone(),
        to: arguments.to.label().to_owned(),
    })
}

// execute one committed Tempest backfill chunk
fn execute_chunk<R, P, T>(
    source: &TempestBackfillSource,
    chunk: &PlannedTempestBackfillChunk,
    mut session: R::Session,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<TempestBackfillChunkReport, TaskFailure>
where
    P: TempestBackfillProvider,
    R: TempestBackfillRepository,
    T: TempestBackfillTimeAdapter,
{
    let mut run_id = None;
    let mut attempts = 0;
    let mut record_count = 0;

    // retain provider or persistence failure as the primary outcome
    let execution = (|| {
        let deadlines = time.deadlines()?;
        validate_deadlines(deadlines)?;
        repository.abandon_expired_runs(&mut session, deadlines.started_at_ms)?;
        let started_id = repository.start_ingestion_run(
            &mut session,
            TempestBackfillRunStart {
                adapter_version: chunk.identity.adapter_version,
                chunk_plan_version: chunk.identity.chunk_plan_version,
                deadline_at_ms: deadlines.run_deadline_at_ms,
                device_id: source.configuration.device_id,
                endpoint: TEMPEST_OBSERVATION_ENDPOINT,
                location_id: source.configuration.location_id,
                requested_end_exclusive_ms: chunk.identity.interval_end_exclusive.start_ms(),
                requested_start_ms: chunk.identity.interval_start.start_ms(),
                source_config_fingerprint: chunk.identity.source_config_fingerprint.clone(),
            },
        )?;
        run_id = Some(started_id.clone());
        let batch = provider.fetch(
            create_request(source, &chunk.identity),
            deadlines.provider_deadline_at_ms,
        )?;
        attempts = batch.attempts;
        record_count = batch.records.len();
        assert_records_within_chunk(&batch.records, &chunk.identity)?;

        // match Node completion which does not retain provider cursor for backfills
        let _provider_cursor = batch.provider_cursor;
        repository.complete_backfill_ingestion(
            &mut session,
            TempestBackfillCompletion {
                attempts,
                identity: chunk.identity.clone(),
                records: batch.records,
                response_metadata: batch.response_metadata,
                run_id: started_id,
                upstream_response_checksum: batch.upstream_response_checksum,
            },
        )?;

        Ok::<(), TempestBackfillOperationError>(())
    })();
    // map the primary committed execution outcome
    let mut report = match execution {
        Ok(()) => TempestBackfillChunkReport {
            error_code: None,
            identity: chunk.identity.clone(),
            record_count,
            secondary_error_codes: Vec::new(),
            status: TempestBackfillChunkStatus::Completed,
        },
        Err(failure) => {
            let primary = as_backfill_provider_failure(failure, attempts.max(1));
            let mut secondary_error_codes = Vec::new();

            // finalize only a committed running row without masking the primary failure
            if let Some(started_id) = run_id.as_deref() {
                // retain a bounded finalization diagnostic
                if let Err(finalization) = repository.fail_ingestion_run(
                    &mut session,
                    started_id,
                    &chunk.identity,
                    &primary,
                ) {
                    secondary_error_codes.push(format!("finalization:{}", finalization.code));
                }
            }

            TempestBackfillChunkReport {
                error_code: Some(primary.error.code),
                identity: chunk.identity.clone(),
                record_count,
                secondary_error_codes,
                status: TempestBackfillChunkStatus::Failed,
            }
        }
    };

    // retain the primary diagnostics when release also fails
    if let Err(release) = repository.release_source_session(session) {
        // surface release failure only after otherwise completed work
        if report.status == TempestBackfillChunkStatus::Completed {
            report.error_code = Some("session_release_failed".to_owned());
        }
        report
            .secondary_error_codes
            .push(format!("release:{}", release.code));
        report.status = TempestBackfillChunkStatus::Failed;
    }

    Ok(report)
}

// build one Tempest request from checked configuration and exact chunk identity
fn create_request(
    source: &TempestBackfillSource,
    identity: &TempestBackfillChunkIdentity,
) -> TempestObservationRequest {
    TempestObservationRequest {
        device_id: source.configuration.device_id,
        end_exclusive_ms: identity.interval_end_exclusive.start_ms(),
        location_id: source.configuration.location_id,
        serial: source.configuration.serial.clone(),
        source_id: source.source.id.clone(),
        start_ms: identity.interval_start.start_ms(),
        timezone: source.configuration.timezone.clone(),
    }
}

// require normalized records to remain inside their exact half-open interval
fn assert_records_within_chunk(
    records: &[NormalizedTempestRecord],
    identity: &TempestBackfillChunkIdentity,
) -> Result<(), TempestBackfillOperationError> {
    // reject out-of-window provider records
    if records.iter().any(|record| {
        record.valid_at_ms < identity.interval_start.start_ms()
            || record.valid_at_ms >= identity.interval_end_exclusive.start_ms()
    }) {
        return Err(TempestBackfillOperationError::unexpected(
            "Tempest payload contained records outside the requested chunk",
        ));
    }

    Ok(())
}

// construct one empty status report for a chunk without provider work
fn chunk_report(
    identity: TempestBackfillChunkIdentity,
    status: TempestBackfillChunkStatus,
) -> TempestBackfillChunkReport {
    TempestBackfillChunkReport {
        error_code: None,
        identity,
        record_count: 0,
        secondary_error_codes: Vec::new(),
        status,
    }
}

// validate the admitted one-shot request
fn validate_arguments<T: TempestBackfillTimeAdapter>(
    arguments: &TempestBackfillArguments,
    time: &mut T,
) -> Result<(), TaskFailure> {
    validate_admitted_date(&arguments.to)?;

    // preserve provider-native minute density
    if arguments.chunk_days != 1 {
        return Err(backfill_failure("tempest_backfill_chunk_range_invalid", 0));
    }

    // validate the optional explicit lower bound
    if let Some(from) = &arguments.from {
        validate_admitted_date(from)?;

        // reject reversed explicit ranges
        if from.start_ms() > arguments.to.start_ms() {
            return Err(backfill_failure("tempest_backfill_range_invalid", 0));
        }
    }
    let today = time.today_utc()?;
    validate_admitted_date(&today)?;

    // reserve partial current-day ingestion for scheduled work
    if arguments.to.start_ms() >= today.start_ms() {
        return Err(backfill_failure("tempest_backfill_range_invalid", 0));
    }

    Ok(())
}

// validate one typed UTC midnight without parsing its label
fn validate_admitted_date(date: &TempestBackfillDate) -> Result<(), TaskFailure> {
    // require one bounded admitted label and exact UTC-day boundary
    if date.label().trim().is_empty()
        || date.label().len() > 10
        || date.start_ms() <= 0
        || date.start_ms() % MILLISECONDS_PER_DAY != 0
    {
        return Err(backfill_failure("tempest_backfill_date_invalid", 0));
    }

    Ok(())
}

// add exact UTC days through the admitted time adapter
fn add_utc_days<T: TempestBackfillTimeAdapter>(
    time: &mut T,
    date: &TempestBackfillDate,
    days: i64,
) -> Result<TempestBackfillDate, TaskFailure> {
    let expected_ms = days
        .checked_mul(MILLISECONDS_PER_DAY)
        .and_then(|duration| date.start_ms().checked_add(duration))
        .ok_or_else(|| backfill_failure("tempest_backfill_date_overflow", 0))?;
    let result = time.add_utc_days(date, days)?;
    validate_admitted_date(&result)?;

    // bind the external date adapter to the exact UTC-day transition
    if result.start_ms() != expected_ms {
        return Err(backfill_failure(
            "tempest_backfill_date_transition_invalid",
            0,
        ));
    }

    Ok(result)
}

// validate one exact durable Tempest chunk identity
fn validate_chunk_identity(identity: &TempestBackfillChunkIdentity) -> Result<(), TaskFailure> {
    // require bounded source identity fingerprint and forward half-open interval
    if identity.source_id.trim().is_empty()
        || identity.source_id.len() > 128
        || !fingerprint_is_valid(&identity.source_config_fingerprint)
        || identity.interval_start.start_ms() >= identity.interval_end_exclusive.start_ms()
    {
        return Err(backfill_failure(
            "tempest_backfill_chunk_identity_invalid",
            0,
        ));
    }

    Ok(())
}

// validate the injected worker deadline plan
fn validate_deadlines(
    deadlines: TempestBackfillDeadlines,
) -> Result<(), TempestBackfillOperationError> {
    // require provider work and durable finalization after start in order
    if deadlines.provider_deadline_at_ms <= deadlines.started_at_ms
        || deadlines.run_deadline_at_ms <= deadlines.provider_deadline_at_ms
    {
        return Err(TempestBackfillOperationError::unexpected(
            "Tempest backfill deadlines are invalid",
        ));
    }

    Ok(())
}

// validate one lowercase SHA-256 source fingerprint
fn fingerprint_is_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

// create one bounded Tempest backfill failure
fn backfill_failure(code: &str, attempts: u32) -> TaskFailure {
    TaskFailure {
        attempts,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        MILLISECONDS_PER_DAY, TEMPEST_OBSERVATION_ADAPTER_VERSION,
        TEMPEST_OBSERVATION_CHUNK_PLAN_VERSION, TEMPEST_OBSERVATION_ENDPOINT,
        TEMPEST_SOURCE_CONTRACT_VERSION, TempestBackfillArguments, TempestBackfillChunkIdentity,
        TempestBackfillChunkStatus, TempestBackfillCompletion, TempestBackfillDate,
        TempestBackfillDeadlines, TempestBackfillErrorClassification,
        TempestBackfillOperationError, TempestBackfillProvider, TempestBackfillProviderFailure,
        TempestBackfillRepository, TempestBackfillRunStart, TempestBackfillSource,
        TempestBackfillTimeAdapter, execute_tempest_bulk_backfill, select_tempest_backfill_sources,
    };
    use crate::{
        NormalizedTempestRecord, ProviderBatch, TaskFailure, TempestObservation,
        TempestObservationEnvelope, TempestObservationRequest, TempestResponseMetadata,
        TempestResponseType, TempestRuntimeConfiguration, TempestScheduledSource,
        TempestSourceKind, normalize_tempest_observations,
    };

    // construct one known admitted UTC date
    fn date(label: &str, start_ms: i64) -> TempestBackfillDate {
        TempestBackfillDate::admitted(label, start_ms)
    }

    // build one exact checked Tempest backfill source
    fn tempest_source(
        location_id: u64,
        device_id: u64,
        source_id: &str,
        station_slug: &str,
        serial: &str,
        fingerprint_byte: char,
    ) -> TempestBackfillSource {
        let fingerprint = fingerprint_byte.to_string().repeat(64);
        let source_key = format!("tempest-{location_id}-observations-v2");
        let material_identity = format!("material-{location_id}");
        let source = TempestScheduledSource {
            active: true,
            cadence_seconds: 3_600,
            id: source_id.to_owned(),
            material_config_identity: material_identity.clone(),
            provider_key: "weatherflow-tempest".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_config_fingerprint: fingerprint.clone(),
            source_key: source_key.clone(),
            source_kind: TempestSourceKind::PhysicalSensor,
            station_slug: station_slug.to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        };
        let configuration = TempestRuntimeConfiguration {
            adapter_contract_version: TEMPEST_SOURCE_CONTRACT_VERSION,
            device_id,
            location_id,
            material_config_identity: material_identity,
            provider_key: source.provider_key.clone(),
            serial: serial.to_owned(),
            site_slug: source.site_slug.clone(),
            source_config_fingerprint: fingerprint,
            source_key,
            source_kind: source.source_kind,
            station_slug: source.station_slug.clone(),
            timezone: source.timezone.clone(),
        };

        TempestBackfillSource {
            configuration,
            history_start: date("2026-08-01", 1_785_542_400_000),
            provider_active: true,
            source,
            station_active: true,
            storage_active: true,
        }
    }

    #[derive(Debug)]
    struct FakeTime {
        dates: BTreeMap<i64, String>,
        deadlines: Vec<TempestBackfillDeadlines>,
        today: TempestBackfillDate,
    }

    impl FakeTime {
        // construct every admitted transition used by the fixture
        fn fixture() -> Self {
            let dates = [
                (1_785_542_400_000, "2026-08-01"),
                (1_785_628_800_000, "2026-08-02"),
                (1_785_715_200_000, "2026-08-03"),
                (1_785_801_600_000, "2026-08-04"),
                (1_785_888_000_000, "2026-08-05"),
                (1_794_009_600_000, "2026-11-07"),
            ]
            .into_iter()
            .map(|(epoch, label)| (epoch, label.to_owned()))
            .collect();

            Self {
                dates,
                deadlines: vec![
                    TempestBackfillDeadlines {
                        provider_deadline_at_ms: 1_794_009_690_000,
                        run_deadline_at_ms: 1_794_009_720_000,
                        started_at_ms: 1_794_009_600_000,
                    },
                    TempestBackfillDeadlines {
                        provider_deadline_at_ms: 1_794_009_700_000,
                        run_deadline_at_ms: 1_794_009_730_000,
                        started_at_ms: 1_794_009_610_000,
                    },
                ],
                today: date("2026-11-07", 1_794_009_600_000),
            }
        }
    }

    impl TempestBackfillTimeAdapter for FakeTime {
        // return the fixed current UTC day
        fn today_utc(&mut self) -> Result<TempestBackfillDate, TaskFailure> {
            Ok(self.today.clone())
        }

        // resolve only pre-admitted UTC date transitions
        fn add_utc_days(
            &mut self,
            date: &TempestBackfillDate,
            days: i64,
        ) -> Result<TempestBackfillDate, TaskFailure> {
            let start_ms = date
                .start_ms()
                .checked_add(days * MILLISECONDS_PER_DAY)
                .ok_or_else(|| failure("date_overflow", 0))?;
            let label = self
                .dates
                .get(&start_ms)
                .ok_or_else(|| failure("date_not_admitted", 0))?;
            Ok(TempestBackfillDate::admitted(label.clone(), start_ms))
        }

        // return the next exact pre-admitted deadline plan
        fn deadlines(&mut self) -> Result<TempestBackfillDeadlines, TempestBackfillOperationError> {
            // retain declared execution order
            if self.deadlines.is_empty() {
                return Err(TempestBackfillOperationError::unexpected(
                    "deadline fixture missing",
                ));
            }
            Ok(self.deadlines.remove(0))
        }
    }

    #[derive(Debug, Default)]
    struct FakeRepository {
        completions: Vec<TempestBackfillCompletion>,
        events: Vec<String>,
        failures: Vec<(
            String,
            TempestBackfillChunkIdentity,
            TempestBackfillProviderFailure,
        )>,
        resume_start_ms: Option<i64>,
        starts: Vec<TempestBackfillRunStart>,
    }

    impl TempestBackfillRepository for FakeRepository {
        type Session = String;

        // match one preexisting exact successful chunk
        fn has_successful_chunk(
            &mut self,
            identity: &TempestBackfillChunkIdentity,
        ) -> Result<bool, TaskFailure> {
            self.events
                .push(format!("resume:{}", identity.interval_start.label()));
            Ok(self.resume_start_ms == Some(identity.interval_start.start_ms()))
        }

        // acquire one source session
        fn acquire_source_session(
            &mut self,
            source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            self.events.push(format!("lock:{source_id}"));
            Ok(Some(source_id.to_owned()))
        }

        // record stale-run cleanup
        fn abandon_expired_runs(
            &mut self,
            _session: &mut Self::Session,
            now_ms: i64,
        ) -> Result<(), TempestBackfillOperationError> {
            self.events.push(format!("abandon:{now_ms}"));
            Ok(())
        }

        // commit one running chunk
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            start: TempestBackfillRunStart,
        ) -> Result<String, TempestBackfillOperationError> {
            let run_id = format!("run-{}", self.starts.len() + 1);
            self.events.push(format!("start:{run_id}"));
            self.starts.push(start);
            Ok(run_id)
        }

        // retain one successful chunk atomically
        fn complete_backfill_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: TempestBackfillCompletion,
        ) -> Result<(), TempestBackfillOperationError> {
            self.events.push(format!("complete:{}", completion.run_id));
            self.completions.push(completion);
            Ok(())
        }

        // retain one failed committed chunk
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            run_id: &str,
            identity: &TempestBackfillChunkIdentity,
            failure: &TempestBackfillProviderFailure,
        ) -> Result<(), TaskFailure> {
            self.events.push(format!("fail:{run_id}"));
            self.failures
                .push((run_id.to_owned(), identity.clone(), failure.clone()));
            Ok(())
        }

        // release one source session
        fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
            self.events.push(format!("release:{session}"));
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct NormalizingProvider {
        requests: Vec<(TempestObservationRequest, i64)>,
    }

    impl TempestBackfillProvider for NormalizingProvider {
        // normalize the first dispatched request and fail the next
        fn fetch(
            &mut self,
            request: TempestObservationRequest,
            deadline_at_ms: i64,
        ) -> Result<
            ProviderBatch<NormalizedTempestRecord, TempestResponseMetadata, Option<i64>>,
            TempestBackfillOperationError,
        > {
            self.requests.push((request.clone(), deadline_at_ms));

            // preserve the fixture's first-failure stop boundary
            if self.requests.len() == 2 {
                return Err(TempestBackfillOperationError::provider(
                    2,
                    TempestBackfillErrorClassification::Retryable,
                    "provider_server_error",
                    "fixture failure",
                ));
            }
            let normalized = normalize_tempest_observations(
                TempestObservationEnvelope {
                    bucket_step_minutes: Some(1.0),
                    device_id: request.device_id,
                    observations: Some(vec![fixture_observation(request.start_ms / 1_000 + 60)]),
                    response_type: TempestResponseType::StationObservation,
                    status_code: 0,
                },
                &request,
                deadline_at_ms - 1,
                1,
            )
            .map_err(|failure| {
                TempestBackfillOperationError::provider(
                    failure.attempts.max(1),
                    TempestBackfillErrorClassification::InvalidPayload,
                    failure.code,
                    "Tempest payload is invalid",
                )
            })?;

            Ok(ProviderBatch {
                attempts: normalized.attempts,
                provider_cursor: normalized.provider_cursor_valid_at_ms,
                records: normalized.records,
                response_metadata: normalized.response_metadata,
                upstream_response_checksum: "t".repeat(64),
            })
        }
    }

    // build one typed Tempest minute observation
    fn fixture_observation(epoch_seconds: i64) -> TempestObservation {
        TempestObservation {
            air_temperature_c: Some(12.5),
            battery_volts: Some(2.6),
            epoch_seconds,
            illuminance_lux: Some(100.0),
            lightning_average_distance_km: None,
            lightning_strike_count: Some(0.0),
            local_daily_rain_accumulation_final_mm: None,
            local_daily_rain_accumulation_mm: None,
            precipitation_analysis_type: None,
            precipitation_mm: Some(0.1),
            precipitation_type: Some(0.0),
            pressure_hpa: Some(1_012.0),
            rain_accumulation_nc_mm: Some(0.1),
            relative_humidity_percent: Some(80.0),
            report_interval_minutes: Some(1.0),
            solar_radiation_wm2: Some(50.0),
            uv_index: Some(1.0),
            wind_direction_degrees: Some(360.0),
            wind_gust_mps: Some(3.0),
            wind_lull_mps: Some(1.0),
            wind_sample_interval_seconds: Some(3.0),
            wind_speed_mps: Some(2.0),
        }
    }

    // create one bounded test failure
    fn failure(code: &str, attempts: u32) -> TaskFailure {
        TaskFailure {
            attempts,
            code: code.to_owned(),
        }
    }

    // preserve exact checked station order despite selector order
    #[test]
    fn tempest_backfill_selection_preserves_checked_station_order() {
        let first = tempest_source(
            203_055,
            470_937,
            "source-tempest-203055",
            "tempest-203055",
            "ST-00198967",
            'a',
        );
        let second = tempest_source(
            201_058,
            466_938,
            "source-tempest-201058",
            "tempest-201058",
            "ST-00194085",
            'b',
        );
        let arguments = TempestBackfillArguments {
            chunk_days: 1,
            dry_run: true,
            from: Some(date("2026-08-01", 1_785_542_400_000)),
            resume: false,
            site: "ballydidean".to_owned(),
            station_ids: vec![201_058, 203_055],
            to: date("2026-08-04", 1_785_801_600_000),
        };
        let candidates = [first, second];

        let selected = select_tempest_backfill_sources(&arguments, "ballydidean", &candidates)
            .expect("checked active Tempest stations should select");

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].configuration.location_id, 203_055);
        assert_eq!(selected[1].configuration.location_id, 201_058);
    }

    // match Node bounds chunks resume normalization lifecycle and stop behavior
    #[test]
    fn tempest_backfill_preserves_bounds_resume_and_committed_records() {
        let source = tempest_source(
            203_055,
            470_937,
            "source-tempest-203055",
            "tempest-203055",
            "ST-00198967",
            'a',
        );
        let arguments = TempestBackfillArguments {
            chunk_days: 1,
            dry_run: false,
            from: None,
            resume: true,
            site: "ballydidean".to_owned(),
            station_ids: vec![203_055],
            to: date("2026-08-04", 1_785_801_600_000),
        };
        let mut repository = FakeRepository {
            resume_start_ms: Some(source.history_start.start_ms()),
            ..FakeRepository::default()
        };
        let mut provider = NormalizingProvider::default();
        let mut time = FakeTime::fixture();

        let report = execute_tempest_bulk_backfill(
            &arguments,
            "ballydidean",
            &[source],
            &mut repository,
            &mut provider,
            &mut time,
        )
        .expect("typed Tempest backfill should report its provider failure");

        assert_eq!(report.exit_code, 1);
        assert_eq!(report.stations.len(), 1);
        let station_report = &report.stations[0];
        assert_eq!(station_report.from, "2026-08-01");
        assert_eq!(station_report.to, "2026-08-04");
        assert_eq!(station_report.location_id, 203_055);
        assert_eq!(station_report.chunks.len(), 3);
        assert_eq!(
            station_report
                .chunks
                .iter()
                .map(|chunk| chunk.status)
                .collect::<Vec<_>>(),
            [
                TempestBackfillChunkStatus::Skipped,
                TempestBackfillChunkStatus::Completed,
                TempestBackfillChunkStatus::Failed,
            ],
        );
        assert_eq!(station_report.chunks[1].record_count, 1);
        assert_eq!(station_report.chunks[2].record_count, 0);
        assert_eq!(
            station_report.chunks[2].error_code.as_deref(),
            Some("provider_server_error")
        );
        assert_eq!(provider.requests.len(), 2);
        assert_eq!(provider.requests[0].0.start_ms, 1_785_628_800_000);
        assert_eq!(provider.requests[0].0.end_exclusive_ms, 1_785_715_200_000);
        assert_eq!(provider.requests[1].0.start_ms, 1_785_715_200_000);
        assert_eq!(provider.requests[1].0.end_exclusive_ms, 1_785_801_600_000);
        assert_eq!(provider.requests[0].0.location_id, 203_055);
        assert_eq!(provider.requests[0].0.device_id, 470_937);
        assert_eq!(provider.requests[0].1, 1_794_009_690_000);
        assert_eq!(repository.starts.len(), 2);
        assert_eq!(repository.starts[0].endpoint, TEMPEST_OBSERVATION_ENDPOINT);
        assert_eq!(
            repository.starts[0].adapter_version,
            TEMPEST_OBSERVATION_ADAPTER_VERSION
        );
        assert_eq!(
            repository.starts[0].chunk_plan_version,
            TEMPEST_OBSERVATION_CHUNK_PLAN_VERSION
        );
        assert_eq!(repository.completions.len(), 1);
        assert_eq!(repository.completions[0].records.len(), 1);
        assert_eq!(
            repository.completions[0].records[0].valid_at_ms,
            1_785_628_860_000
        );
        assert_eq!(
            repository.completions[0].response_metadata.device_id,
            470_937
        );
        assert_eq!(
            repository.completions[0].upstream_response_checksum,
            "t".repeat(64)
        );
        assert_eq!(repository.failures.len(), 1);
        assert_eq!(repository.failures[0].2.attempts, 2);
        assert_eq!(
            repository.failures[0].2.error.classification,
            TempestBackfillErrorClassification::Retryable,
        );
        assert_eq!(repository.failures[0].2.error.code, "provider_server_error");
        assert_eq!(repository.failures[0].2.error.message, "fixture failure");
        assert_eq!(
            repository.events,
            [
                "resume:2026-08-01",
                "resume:2026-08-02",
                "lock:source-tempest-203055",
                "abandon:1794009600000",
                "start:run-1",
                "complete:run-1",
                "release:source-tempest-203055",
                "resume:2026-08-03",
                "lock:source-tempest-203055",
                "abandon:1794009610000",
                "start:run-2",
                "fail:run-2",
                "release:source-tempest-203055",
            ]
        );
    }
}
