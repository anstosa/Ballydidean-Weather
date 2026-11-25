//! Typed NOAA tide backfill planning and execution without transport, database, or date codecs.

use std::collections::BTreeSet;

use super::{
    NOAA_TIDE_CHUNK_PLAN_VERSION, NOAA_TIDE_DATA_ENDPOINT, NoaaTideProviderCursor,
    NoaaTideRangeRequest, NoaaTideResponseMetadata, NoaaTideRuntimeConfiguration,
    NoaaTideScheduledSource, NoaaTideSourceKind, NormalizedNoaaTideRecord, ProviderBatch,
    TaskFailure, noaa_tide_adapter_version, noaa_tide_source_matches_configuration,
};

const MILLISECONDS_PER_DAY: i64 = 86_400_000;

/// One UTC date admitted by the caller's date library.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TideBackfillDate {
    label: String,
    start_ms: i64,
}

impl TideBackfillDate {
    /// Construct one already-admitted UTC calendar date.
    pub fn admitted(label: impl Into<String>, start_ms: i64) -> Self {
        Self {
            label: label.into(),
            start_ms,
        }
    }

    /// Return the admitted canonical date label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Return the admitted UTC midnight instant.
    pub const fn start_ms(&self) -> i64 {
        self.start_ms
    }
}

/// Exact deadlines produced by the admitted worker time policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TideBackfillDeadlines {
    pub provider_deadline_at_ms: i64,
    pub run_deadline_at_ms: i64,
    pub started_at_ms: i64,
}

/// One durable ingestion-error classification shared with the Node contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TideBackfillErrorClassification {
    Retryable,
    RateLimited,
    Permanent,
    InvalidPayload,
}

/// Exact durable ingestion error retained for a failed backfill run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TideBackfillIngestionError {
    pub classification: TideBackfillErrorClassification,
    pub code: String,
    pub message: String,
}

/// One classified provider failure with its exact provider attempt count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TideBackfillProviderFailure {
    pub attempts: u32,
    pub error: TideBackfillIngestionError,
}

/// One fallible operation inside the Node-compatible backfill catch boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TideBackfillOperationError {
    Provider(TideBackfillProviderFailure),
    Unexpected { message: String },
}

impl TideBackfillOperationError {
    /// Construct one already-classified provider failure.
    pub fn provider(
        attempts: u32,
        classification: TideBackfillErrorClassification,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::Provider(TideBackfillProviderFailure {
            attempts,
            error: TideBackfillIngestionError {
                classification,
                code: code.into(),
                message: message.into(),
            },
        })
    }

    /// Construct one unclassified operation error for catch-boundary coercion.
    pub fn unexpected(message: impl Into<String>) -> Self {
        Self::Unexpected {
            message: message.into(),
        }
    }
}

/// Time operations required by planning and committed chunk execution.
pub trait TideBackfillTimeAdapter {
    /// Return the current admitted UTC date used to exclude the partial day.
    fn today_utc(&mut self) -> Result<TideBackfillDate, TaskFailure>;

    /// Add exact UTC calendar days through an admitted date implementation.
    fn add_utc_days(
        &mut self,
        date: &TideBackfillDate,
        days: i64,
    ) -> Result<TideBackfillDate, TaskFailure>;

    /// Return one bounded provider and durable-run deadline plan.
    fn deadlines(&mut self) -> Result<TideBackfillDeadlines, TideBackfillOperationError>;
}

/// One typed one-shot tide backfill request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TideBackfillArguments {
    pub dry_run: bool,
    pub from: TideBackfillDate,
    pub resume: bool,
    pub site: String,
    pub source_keys: Vec<String>,
    pub to: TideBackfillDate,
}

/// One configured source joined to an admitted active storage identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TideBackfillSource {
    pub configuration: NoaaTideRuntimeConfiguration,
    pub history_start: TideBackfillDate,
    pub source: NoaaTideScheduledSource,
    pub storage_active: bool,
}

/// One exact durable backfill-chunk identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TideBackfillChunkIdentity {
    pub adapter_version: &'static str,
    pub chunk_plan_version: &'static str,
    pub interval_end_exclusive: TideBackfillDate,
    pub interval_start: TideBackfillDate,
    pub requested_from_date: String,
    pub requested_to_date: String,
    pub source_config_fingerprint: String,
    pub source_id: String,
}

/// One inclusive-date chunk and its durable half-open identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedTideBackfillChunk {
    pub end_date: TideBackfillDate,
    pub identity: TideBackfillChunkIdentity,
    pub start_date: TideBackfillDate,
}

/// Metadata committed before one provider request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TideBackfillRunStart {
    pub adapter_version: &'static str,
    pub chunk_plan_version: &'static str,
    pub deadline_at_ms: i64,
    pub endpoint: &'static str,
    pub requested_end_exclusive_ms: i64,
    pub requested_start_ms: i64,
    pub source_config_fingerprint: String,
    pub station_id: String,
}

/// One successful atomic backfill completion.
#[derive(Clone, Debug, PartialEq)]
pub struct TideBackfillCompletion {
    pub attempts: u32,
    pub identity: TideBackfillChunkIdentity,
    pub records: Vec<NormalizedNoaaTideRecord>,
    pub response_metadata: NoaaTideResponseMetadata,
    pub run_id: String,
    pub upstream_response_checksum: String,
}

/// Persistence required by one typed tide backfill.
pub trait TideBackfillRepository {
    type Session;

    /// Report whether this exact durable chunk already succeeded.
    fn has_successful_chunk(
        &mut self,
        identity: &TideBackfillChunkIdentity,
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
    ) -> Result<(), TideBackfillOperationError>;

    /// Commit one running backfill row.
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: TideBackfillRunStart,
    ) -> Result<String, TideBackfillOperationError>;

    /// Commit records and the successful chunk identity atomically.
    fn complete_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: TideBackfillCompletion,
    ) -> Result<(), TideBackfillOperationError>;

    /// Finalize one committed failed chunk.
    fn fail_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        run_id: &str,
        identity: &TideBackfillChunkIdentity,
        failure: &TideBackfillProviderFailure,
    ) -> Result<(), TaskFailure>;

    /// Release one source-scoped session.
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure>;
}

/// One typed NOAA request plus its provider deadline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TideBackfillProviderRequest {
    pub deadline_at_ms: i64,
    pub range: NoaaTideRangeRequest,
}

/// One provider operation required by the backfill control loop.
pub trait TideBackfillProvider {
    /// Fetch one normalized NOAA batch.
    fn fetch(
        &mut self,
        request: TideBackfillProviderRequest,
    ) -> Result<
        ProviderBatch<
            NormalizedNoaaTideRecord,
            NoaaTideResponseMetadata,
            Option<NoaaTideProviderCursor>,
        >,
        TideBackfillOperationError,
    >;
}

/// One failed source chunk retained in the final report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TideBackfillFailedChunk {
    pub error_code: String,
    pub start_date: String,
}

/// One source-level backfill report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TideBackfillSourceReport {
    pub completed_chunks: usize,
    pub exit_code: u8,
    pub failed_chunk: Option<TideBackfillFailedChunk>,
    pub from: String,
    pub planned_chunks: usize,
    pub records: usize,
    pub skipped_chunks: usize,
    pub source: String,
    pub to: String,
}

/// One complete one-shot tide backfill report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TideBackfillReport {
    pub dry_run: bool,
    pub exit_code: u8,
    pub resume: bool,
    pub site: String,
    pub sources: Vec<TideBackfillSourceReport>,
}

/// Resolve active selected sources against their exact checked identities.
pub fn select_tide_backfill_sources<'a>(
    arguments: &TideBackfillArguments,
    configuration_site: &str,
    candidates: &'a [TideBackfillSource],
) -> Result<Vec<&'a TideBackfillSource>, TaskFailure> {
    // reject cross-site execution
    if arguments.site != configuration_site {
        return Err(backfill_failure("tide_backfill_site_mismatch", 0));
    }
    let selectors = arguments.source_keys.iter().collect::<BTreeSet<_>>();

    // require the parsed selector contract
    if selectors.len() != arguments.source_keys.len()
        || arguments
            .source_keys
            .iter()
            .any(|value| !stable_key_is_valid(value))
    {
        return Err(backfill_failure("tide_backfill_selector_invalid", 0));
    }
    let mut selected = Vec::new();

    // preserve configured station order
    for candidate in candidates {
        let configured_active =
            candidate.configuration.station_active && candidate.configuration.source_active;
        let selector_matches =
            selectors.is_empty() || selectors.contains(&candidate.configuration.source_key);

        // exclude inactive or unselected configuration
        if !configured_active || !selector_matches {
            continue;
        }

        // require the active storage join used by the Node resolver
        if !candidate.storage_active || !candidate.source.active {
            return Err(backfill_failure("tide_backfill_source_missing", 0));
        }

        // reject material identity drift before provider work
        if !noaa_tide_source_matches_configuration(&candidate.source, &candidate.configuration) {
            return Err(backfill_failure("tide_backfill_identity_mismatch", 0));
        }
        selected.push(candidate);
    }

    // reject unknown or inactive selectors
    if !selectors.is_empty() && selected.len() != selectors.len() {
        return Err(backfill_failure("tide_backfill_source_unavailable", 0));
    }

    Ok(selected)
}

/// Plan every exact inclusive-date NOAA chunk.
pub fn plan_tide_backfill_chunks<T: TideBackfillTimeAdapter>(
    source: &TideBackfillSource,
    from: &TideBackfillDate,
    to: &TideBackfillDate,
    time: &mut T,
) -> Result<Vec<PlannedTideBackfillChunk>, TaskFailure> {
    validate_admitted_date(from)?;
    validate_admitted_date(to)?;
    let mut chunks = Vec::new();
    let mut start_date = from.clone();
    let maximum_chunk_days = i64::from(source.configuration.maximum_chunk_days);

    // select the provider's source-kind-specific range ceiling
    let maximum_supported_days = match source.configuration.source_kind {
        NoaaTideSourceKind::Observation => 31,
        NoaaTideSourceKind::Prediction => 366,
    };

    // require the bounded source-kind-specific catalog range
    if maximum_chunk_days < 1 || maximum_chunk_days > maximum_supported_days {
        return Err(backfill_failure("tide_backfill_chunk_range_invalid", 0));
    }

    // cover the inclusive archive range in configured source-sized chunks
    while start_date.start_ms <= to.start_ms {
        let maximum_end = add_utc_days(time, &start_date, maximum_chunk_days - 1)?;

        // cap the final chunk at the requested inclusive date
        let end_date = if maximum_end.start_ms < to.start_ms {
            maximum_end
        } else {
            to.clone()
        };
        let interval_end_exclusive = add_utc_days(time, &end_date, 1)?;
        let identity = TideBackfillChunkIdentity {
            adapter_version: noaa_tide_adapter_version(source.configuration.source_kind),
            chunk_plan_version: NOAA_TIDE_CHUNK_PLAN_VERSION,
            interval_end_exclusive: interval_end_exclusive.clone(),
            interval_start: start_date.clone(),
            requested_from_date: from.label.clone(),
            requested_to_date: to.label.clone(),
            source_config_fingerprint: source.source.source_config_fingerprint.clone(),
            source_id: source.source.id.clone(),
        };
        validate_chunk_identity(&identity)?;
        chunks.push(PlannedTideBackfillChunk {
            end_date,
            identity,
            start_date,
        });
        start_date = interval_end_exclusive;
    }

    Ok(chunks)
}

/// Execute every selected source through exact resume and failure boundaries.
pub fn execute_tide_backfill<R, P, T>(
    arguments: &TideBackfillArguments,
    configuration_site: &str,
    candidates: &[TideBackfillSource],
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<TideBackfillReport, TaskFailure>
where
    P: TideBackfillProvider,
    R: TideBackfillRepository,
    T: TideBackfillTimeAdapter,
{
    validate_arguments(arguments, time)?;
    let selected = select_tide_backfill_sources(arguments, configuration_site, candidates)?;
    let mut reports = Vec::with_capacity(selected.len());

    // isolate each configured source archive
    for source in selected {
        reports.push(execute_source_backfill(
            arguments, source, repository, provider, time,
        )?);
    }
    let exit_code = u8::from(reports.iter().any(|report| report.exit_code == 1));

    Ok(TideBackfillReport {
        dry_run: arguments.dry_run,
        exit_code,
        resume: arguments.resume,
        site: arguments.site.clone(),
        sources: reports,
    })
}

// execute all exact chunks for one source
fn execute_source_backfill<R, P, T>(
    arguments: &TideBackfillArguments,
    source: &TideBackfillSource,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<TideBackfillSourceReport, TaskFailure>
where
    P: TideBackfillProvider,
    R: TideBackfillRepository,
    T: TideBackfillTimeAdapter,
{
    // clamp each source to its configured historical availability
    let from = if arguments.from.start_ms > source.history_start.start_ms {
        arguments.from.clone()
    } else {
        source.history_start.clone()
    };
    let chunks = plan_tide_backfill_chunks(source, &from, &arguments.to, time)?;
    let mut completed_chunks = 0;
    let mut records = 0;
    let mut skipped_chunks = 0;
    let mut failed_chunk = None;

    // execute chunks in their planned order
    for chunk in &chunks {
        // satisfy exact-success resume semantics before dry-run handling
        if arguments.resume && repository.has_successful_chunk(&chunk.identity)? {
            skipped_chunks += 1;
            continue;
        }

        // prove dry-run has no session or provider side effects
        if arguments.dry_run {
            skipped_chunks += 1;
            continue;
        }
        let result = execute_chunk(source, chunk, repository, provider, time)?;

        // stop only this source after its first failed chunk
        if !result.ok {
            failed_chunk = Some(TideBackfillFailedChunk {
                error_code: result.error_code,
                start_date: chunk.start_date.label.clone(),
            });
            break;
        }

        completed_chunks += 1;
        records += result.record_count;
    }

    Ok(TideBackfillSourceReport {
        completed_chunks,
        exit_code: u8::from(failed_chunk.is_some()),
        failed_chunk,
        from: from.label,
        planned_chunks: chunks.len(),
        records,
        skipped_chunks,
        source: source.source.source_key.clone(),
        to: arguments.to.label.clone(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TideChunkExecution {
    error_code: String,
    ok: bool,
    record_count: usize,
}

// execute one committed backfill chunk
fn execute_chunk<R, P, T>(
    source: &TideBackfillSource,
    chunk: &PlannedTideBackfillChunk,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<TideChunkExecution, TaskFailure>
where
    P: TideBackfillProvider,
    R: TideBackfillRepository,
    T: TideBackfillTimeAdapter,
{
    let Some(mut session) = repository.acquire_source_session(&source.source.id)? else {
        return Ok(TideChunkExecution {
            error_code: "source_locked".to_owned(),
            ok: false,
            record_count: 0,
        });
    };
    let mut run_id = None;
    let mut attempts = 0;

    // retain provider or persistence failure as the primary outcome
    let execution = (|| {
        let deadlines = time.deadlines()?;
        validate_deadlines(deadlines)?;
        repository.abandon_expired_runs(&mut session, deadlines.started_at_ms)?;
        let started_id = repository.start_ingestion_run(
            &mut session,
            TideBackfillRunStart {
                adapter_version: chunk.identity.adapter_version,
                chunk_plan_version: chunk.identity.chunk_plan_version,
                deadline_at_ms: deadlines.run_deadline_at_ms,
                endpoint: NOAA_TIDE_DATA_ENDPOINT,
                requested_end_exclusive_ms: chunk.identity.interval_end_exclusive.start_ms,
                requested_start_ms: chunk.identity.interval_start.start_ms,
                source_config_fingerprint: chunk.identity.source_config_fingerprint.clone(),
                station_id: source.configuration.station_id.clone(),
            },
        )?;
        run_id = Some(started_id.clone());
        let batch = provider.fetch(TideBackfillProviderRequest {
            deadline_at_ms: deadlines.provider_deadline_at_ms,
            range: create_range_request(source, &chunk.identity),
        })?;
        attempts = batch.attempts;
        assert_records_within_chunk(&batch.records, &chunk.identity)?;
        let record_count = batch.records.len();

        // preserve Node behavior where resume is chunk-identity based rather than cursor based
        let _provider_cursor = batch.provider_cursor;
        repository.complete_backfill_ingestion(
            &mut session,
            TideBackfillCompletion {
                attempts,
                identity: chunk.identity.clone(),
                records: batch.records,
                response_metadata: batch.response_metadata,
                run_id: started_id,
                upstream_response_checksum: batch.upstream_response_checksum,
            },
        )?;

        Ok::<usize, TideBackfillOperationError>(record_count)
    })();
    // map the primary committed execution outcome
    let mut result = match execution {
        Ok(record_count) => TideChunkExecution {
            error_code: String::new(),
            ok: true,
            record_count,
        },
        Err(failure) => {
            let primary = as_backfill_provider_failure(failure, attempts.max(1));

            // finalize only a committed running row without masking the primary failure
            if let Some(started_id) = run_id.as_deref() {
                let _ = repository.fail_ingestion_run(
                    &mut session,
                    started_id,
                    &chunk.identity,
                    &primary,
                );
            }

            TideChunkExecution {
                error_code: primary.error.code,
                ok: false,
                record_count: 0,
            }
        }
    };

    // surface release failure only after otherwise successful work
    if repository.release_source_session(session).is_err() && result.ok {
        result.error_code = "session_release_failed".to_owned();
        result.ok = false;
    }

    Ok(result)
}

// build one provider request from the exact durable chunk
fn create_range_request(
    source: &TideBackfillSource,
    identity: &TideBackfillChunkIdentity,
) -> NoaaTideRangeRequest {
    NoaaTideRangeRequest {
        end_exclusive_ms: identity.interval_end_exclusive.start_ms,
        source_id: source.source.id.clone(),
        source_kind: source.configuration.source_kind,
        start_ms: identity.interval_start.start_ms,
        station_id: source.configuration.station_id.clone(),
        timezone: source.configuration.timezone.clone(),
    }
}

// require normalized records to remain inside their durable chunk
fn assert_records_within_chunk(
    records: &[NormalizedNoaaTideRecord],
    identity: &TideBackfillChunkIdentity,
) -> Result<(), TideBackfillOperationError> {
    // validate every normalized identity and half-open timestamp
    for record in records {
        if record.source_id != identity.source_id
            || record.valid_at_ms < identity.interval_start.start_ms
            || record.valid_at_ms >= identity.interval_end_exclusive.start_ms
        {
            return Err(TideBackfillOperationError::unexpected(
                "NOAA tide record escaped its backfill chunk",
            ));
        }
    }

    Ok(())
}

// validate the admitted one-shot request
fn validate_arguments<T: TideBackfillTimeAdapter>(
    arguments: &TideBackfillArguments,
    time: &mut T,
) -> Result<(), TaskFailure> {
    validate_admitted_date(&arguments.from)?;
    validate_admitted_date(&arguments.to)?;
    let today = time.today_utc()?;
    validate_admitted_date(&today)?;

    // reserve the partial current day and reject reversed ranges
    if arguments.to.start_ms >= today.start_ms || arguments.from.start_ms > arguments.to.start_ms {
        return Err(backfill_failure("tide_backfill_range_invalid", 0));
    }

    Ok(())
}

// validate one typed UTC midnight without parsing its label
fn validate_admitted_date(date: &TideBackfillDate) -> Result<(), TaskFailure> {
    // require one bounded admitted label and exact UTC-day boundary
    if date.label.trim().is_empty()
        || date.label.len() > 10
        || date.start_ms <= 0
        || date.start_ms % MILLISECONDS_PER_DAY != 0
    {
        return Err(backfill_failure("tide_backfill_date_invalid", 0));
    }

    Ok(())
}

// add exact UTC days through the admitted time adapter
fn add_utc_days<T: TideBackfillTimeAdapter>(
    time: &mut T,
    date: &TideBackfillDate,
    days: i64,
) -> Result<TideBackfillDate, TaskFailure> {
    let expected_ms = days
        .checked_mul(MILLISECONDS_PER_DAY)
        .and_then(|duration| date.start_ms.checked_add(duration))
        .ok_or_else(|| backfill_failure("tide_backfill_date_overflow", 0))?;
    let result = time.add_utc_days(date, days)?;
    validate_admitted_date(&result)?;

    // bind the external date adapter to the exact UTC-day transition
    if result.start_ms != expected_ms {
        return Err(backfill_failure("tide_backfill_date_transition_invalid", 0));
    }

    Ok(result)
}

// validate one exact durable chunk identity
fn validate_chunk_identity(identity: &TideBackfillChunkIdentity) -> Result<(), TaskFailure> {
    // require bounded source identity and a forward half-open interval
    if identity.source_id.trim().is_empty()
        || identity.source_id.len() > 128
        || identity.source_config_fingerprint.len() != 64
        || !identity
            .source_config_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || identity.interval_start.start_ms >= identity.interval_end_exclusive.start_ms
    {
        return Err(backfill_failure("tide_backfill_chunk_identity_invalid", 0));
    }

    Ok(())
}

// validate the injected worker deadline plan
fn validate_deadlines(deadlines: TideBackfillDeadlines) -> Result<(), TideBackfillOperationError> {
    // require provider work and durable finalization after start in order
    if deadlines.provider_deadline_at_ms <= deadlines.started_at_ms
        || deadlines.run_deadline_at_ms <= deadlines.provider_deadline_at_ms
    {
        return Err(TideBackfillOperationError::unexpected(
            "tide backfill deadlines are invalid",
        ));
    }

    Ok(())
}

// preserve classified provider failures and coerce all other catch values
pub(crate) fn as_backfill_provider_failure(
    failure: TideBackfillOperationError,
    fallback_attempts: u32,
) -> TideBackfillProviderFailure {
    // match Node asProviderFailure retention and fallback behavior
    match failure {
        TideBackfillOperationError::Provider(provider_failure) => provider_failure,
        TideBackfillOperationError::Unexpected { message } => TideBackfillProviderFailure {
            attempts: fallback_attempts.max(1),
            error: TideBackfillIngestionError {
                classification: TideBackfillErrorClassification::Retryable,
                code: "provider_unavailable".to_owned(),
                message: bounded_unexpected_message(&message),
            },
        },
    }
}

// bound one admitted operation message to the durable Node contract
fn bounded_unexpected_message(message: &str) -> String {
    let trimmed = message.trim();

    // match the Node empty-message fallback
    if trimmed.is_empty() {
        return "provider request failed".to_owned();
    }

    let mut utf16_units = 0;
    let mut bounded = String::new();

    // retain whole Unicode scalars without exceeding Node's 512 UTF-16-unit boundary
    for character in trimmed.chars() {
        let character_units = character.len_utf16();
        // stop before crossing the durable message boundary
        if utf16_units + character_units > 512 {
            break;
        }
        bounded.push(character);
        utf16_units += character_units;
    }

    bounded
}

// validate one already-parsed stable selector key
fn stable_key_is_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

// create one bounded tide backfill failure
fn backfill_failure(code: &str, attempts: u32) -> TaskFailure {
    TaskFailure {
        attempts,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{
        MILLISECONDS_PER_DAY, TideBackfillArguments, TideBackfillChunkIdentity,
        TideBackfillCompletion, TideBackfillDate, TideBackfillDeadlines,
        TideBackfillErrorClassification, TideBackfillOperationError, TideBackfillProvider,
        TideBackfillProviderFailure, TideBackfillProviderRequest, TideBackfillRepository,
        TideBackfillRunStart, TideBackfillSource, TideBackfillTimeAdapter, execute_tide_backfill,
        plan_tide_backfill_chunks, select_tide_backfill_sources,
    };
    use crate::{
        NOAA_TIDE_CHUNK_PLAN_VERSION, NOAA_TIDE_OBSERVATION_ADAPTER_VERSION, NoaaTideCapability,
        NoaaTideObservation, NoaaTidePayload, NoaaTideProviderCursor, NoaaTideResponseMetadata,
        NoaaTideRuntimeConfiguration, NoaaTideScheduledSource, NoaaTideSourceKind,
        NormalizedNoaaTideRecord, ProviderBatch, TaskFailure, normalize_noaa_tide_range,
    };

    // construct one known admitted UTC date
    fn date(label: &str, start_ms: i64) -> TideBackfillDate {
        TideBackfillDate::admitted(label, start_ms)
    }

    // build one exact active observation source
    fn observation_source() -> TideBackfillSource {
        let fingerprint = "a".repeat(64);
        let source = NoaaTideScheduledSource {
            active: true,
            cadence_seconds: 360,
            id: "source-noaa-observation".to_owned(),
            material_config_identity: "noaa-observation-material".to_owned(),
            provider_key: "noaa-co-ops".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_config_fingerprint: fingerprint.clone(),
            source_key: "noaa-port-townsend-water-level-v1".to_owned(),
            source_kind: NoaaTideSourceKind::Observation,
            station_slug: "port-townsend-tide-gauge".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        };
        let configuration = NoaaTideRuntimeConfiguration {
            adapter_contract_version: NOAA_TIDE_OBSERVATION_ADAPTER_VERSION,
            capabilities: BTreeSet::from([
                NoaaTideCapability::Current,
                NoaaTideCapability::Historical,
            ]),
            forecast_days: None,
            material_config_identity: source.material_config_identity.clone(),
            maximum_chunk_days: 31,
            provider_key: source.provider_key.clone(),
            site_slug: source.site_slug.clone(),
            source_active: true,
            source_config_fingerprint: fingerprint,
            source_key: source.source_key.clone(),
            source_kind: source.source_kind,
            station_active: true,
            station_id: "9444900".to_owned(),
            station_slug: source.station_slug.clone(),
            timezone: source.timezone.clone(),
        };

        TideBackfillSource {
            configuration,
            history_start: date("2019-01-01", 1_546_300_800_000),
            source,
            storage_active: true,
        }
    }

    #[derive(Debug)]
    struct FakeTime {
        dates: BTreeMap<i64, String>,
        deadlines: Vec<TideBackfillDeadlines>,
        today: TideBackfillDate,
    }

    impl FakeTime {
        // construct every admitted transition used by the fixture
        fn fixture() -> Self {
            let dates = [
                (1_785_542_400_000, "2026-08-01"),
                (1_785_628_800_000, "2026-08-02"),
                (1_788_134_400_000, "2026-08-31"),
                (1_788_220_800_000, "2026-09-01"),
                (1_790_812_800_000, "2026-10-01"),
                (1_790_899_200_000, "2026-10-02"),
                (1_793_491_200_000, "2026-11-01"),
                (1_793_577_600_000, "2026-11-02"),
                (1_793_836_800_000, "2026-11-05"),
                (1_793_923_200_000, "2026-11-06"),
                (1_794_009_600_000, "2026-11-07"),
                (1_796_169_600_000, "2026-12-02"),
            ]
            .into_iter()
            .map(|(epoch, label)| (epoch, label.to_owned()))
            .collect();

            Self {
                dates,
                deadlines: vec![
                    TideBackfillDeadlines {
                        provider_deadline_at_ms: 1_794_009_690_000,
                        run_deadline_at_ms: 1_794_009_720_000,
                        started_at_ms: 1_794_009_600_000,
                    },
                    TideBackfillDeadlines {
                        provider_deadline_at_ms: 1_794_009_700_000,
                        run_deadline_at_ms: 1_794_009_730_000,
                        started_at_ms: 1_794_009_610_000,
                    },
                ],
                today: date("2026-11-07", 1_794_009_600_000),
            }
        }
    }

    impl TideBackfillTimeAdapter for FakeTime {
        // return the fixed current UTC day
        fn today_utc(&mut self) -> Result<TideBackfillDate, TaskFailure> {
            Ok(self.today.clone())
        }

        // resolve only pre-admitted UTC date transitions
        fn add_utc_days(
            &mut self,
            date: &TideBackfillDate,
            days: i64,
        ) -> Result<TideBackfillDate, TaskFailure> {
            let start_ms = date
                .start_ms()
                .checked_add(days * MILLISECONDS_PER_DAY)
                .ok_or_else(|| failure("date_overflow", 0))?;
            let label = self
                .dates
                .get(&start_ms)
                .ok_or_else(|| failure("date_not_admitted", 0))?;
            Ok(TideBackfillDate::admitted(label.clone(), start_ms))
        }

        // return the next exact pre-admitted deadline plan
        fn deadlines(&mut self) -> Result<TideBackfillDeadlines, TideBackfillOperationError> {
            // retain declared execution order
            if self.deadlines.is_empty() {
                return Err(TideBackfillOperationError::unexpected(
                    "deadline fixture missing",
                ));
            }
            Ok(self.deadlines.remove(0))
        }
    }

    #[derive(Debug, Default)]
    struct FakeRepository {
        completions: Vec<TideBackfillCompletion>,
        events: Vec<String>,
        failures: Vec<(
            String,
            TideBackfillChunkIdentity,
            TideBackfillProviderFailure,
        )>,
        resume_start_ms: Option<i64>,
        starts: Vec<TideBackfillRunStart>,
    }

    impl TideBackfillRepository for FakeRepository {
        type Session = String;

        // match one preexisting exact successful chunk
        fn has_successful_chunk(
            &mut self,
            identity: &TideBackfillChunkIdentity,
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
        ) -> Result<(), TideBackfillOperationError> {
            self.events.push(format!("abandon:{now_ms}"));
            Ok(())
        }

        // commit one running chunk
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            start: TideBackfillRunStart,
        ) -> Result<String, TideBackfillOperationError> {
            let run_id = format!("run-{}", self.starts.len() + 1);
            self.events.push(format!("start:{run_id}"));
            self.starts.push(start);
            Ok(run_id)
        }

        // retain one successful chunk atomically
        fn complete_backfill_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: TideBackfillCompletion,
        ) -> Result<(), TideBackfillOperationError> {
            self.events.push(format!("complete:{}", completion.run_id));
            self.completions.push(completion);
            Ok(())
        }

        // retain one failed committed chunk
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            run_id: &str,
            identity: &TideBackfillChunkIdentity,
            failure: &TideBackfillProviderFailure,
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
        cursors: Vec<Option<NoaaTideProviderCursor>>,
        requests: Vec<TideBackfillProviderRequest>,
    }

    impl TideBackfillProvider for NormalizingProvider {
        // normalize the first dispatched request and fail the next
        fn fetch(
            &mut self,
            request: TideBackfillProviderRequest,
        ) -> Result<
            ProviderBatch<
                NormalizedNoaaTideRecord,
                NoaaTideResponseMetadata,
                Option<NoaaTideProviderCursor>,
            >,
            TideBackfillOperationError,
        > {
            self.requests.push(request.clone());

            // preserve the fixture's first-failure stop boundary
            if self.requests.len() == 2 {
                return Err(TideBackfillOperationError::provider(
                    2,
                    TideBackfillErrorClassification::Retryable,
                    "provider_server_error",
                    "fixture failure",
                ));
            }
            let valid_at_ms = request.range.start_ms + 360_000;
            let normalized = normalize_noaa_tide_range(
                &NoaaTidePayload {
                    error_message: None,
                    observations: Some(vec![NoaaTideObservation {
                        flags: "0,0,0,0".to_owned(),
                        quality_status: "v".to_owned(),
                        valid_at_ms,
                        water_level_m: 1.234,
                    }]),
                    predictions: None,
                },
                &request.range,
                request.deadline_at_ms - 1,
                1,
            )
            .map_err(|failure| {
                TideBackfillOperationError::provider(
                    failure.attempts.max(1),
                    TideBackfillErrorClassification::InvalidPayload,
                    failure.code,
                    "NOAA tide payload is invalid",
                )
            })?;
            let cursor = normalized
                .provider_cursor_valid_at_ms
                .map(|valid_at_ms| NoaaTideProviderCursor { valid_at_ms });
            self.cursors.push(cursor);

            Ok(ProviderBatch {
                attempts: normalized.attempts,
                provider_cursor: cursor,
                records: normalized.records,
                response_metadata: normalized.response_metadata,
                upstream_response_checksum: "b".repeat(64),
            })
        }
    }

    #[derive(Debug, Default)]
    struct UnexpectedProvider {
        requests: Vec<TideBackfillProviderRequest>,
    }

    impl TideBackfillProvider for UnexpectedProvider {
        // expose one ordinary non-provider error at the Node catch boundary
        fn fetch(
            &mut self,
            request: TideBackfillProviderRequest,
        ) -> Result<
            ProviderBatch<
                NormalizedNoaaTideRecord,
                NoaaTideResponseMetadata,
                Option<NoaaTideProviderCursor>,
            >,
            TideBackfillOperationError,
        > {
            self.requests.push(request);
            Err(TideBackfillOperationError::unexpected("socket exploded"))
        }
    }

    // create one bounded test failure
    fn failure(code: &str, attempts: u32) -> TaskFailure {
        TaskFailure {
            attempts,
            code: code.to_owned(),
        }
    }

    // match Node chunk boundaries and stop after the first failed exact chunk
    #[test]
    fn tide_backfill_preserves_resume_order_cursor_and_failure_boundaries() {
        let source = observation_source();
        let arguments = TideBackfillArguments {
            dry_run: false,
            from: date("2026-08-01", 1_785_542_400_000),
            resume: true,
            site: "ballydidean".to_owned(),
            source_keys: vec![source.source.source_key.clone()],
            to: date("2026-11-05", 1_793_836_800_000),
        };
        let mut repository = FakeRepository {
            resume_start_ms: Some(arguments.from.start_ms()),
            ..FakeRepository::default()
        };
        let mut provider = NormalizingProvider::default();
        let mut time = FakeTime::fixture();

        let report = execute_tide_backfill(
            &arguments,
            "ballydidean",
            &[source],
            &mut repository,
            &mut provider,
            &mut time,
        )
        .expect("typed tide backfill should report its provider failure");

        assert_eq!(report.exit_code, 1);
        assert_eq!(report.sources.len(), 1);
        let source_report = &report.sources[0];
        assert_eq!(source_report.planned_chunks, 4);
        assert_eq!(source_report.skipped_chunks, 1);
        assert_eq!(source_report.completed_chunks, 1);
        assert_eq!(source_report.records, 1);
        assert_eq!(source_report.from, "2026-08-01");
        assert_eq!(source_report.to, "2026-11-05");
        assert_eq!(
            source_report.failed_chunk,
            Some(super::TideBackfillFailedChunk {
                error_code: "provider_server_error".to_owned(),
                start_date: "2026-10-02".to_owned(),
            }),
        );
        assert_eq!(provider.requests.len(), 2);
        assert_eq!(provider.requests[0].range.start_ms, 1_788_220_800_000);
        assert_eq!(
            provider.requests[0].range.end_exclusive_ms,
            1_790_899_200_000
        );
        assert_eq!(provider.requests[1].range.start_ms, 1_790_899_200_000);
        assert_eq!(
            provider.requests[1].range.end_exclusive_ms,
            1_793_577_600_000
        );
        assert_eq!(provider.requests[0].range.station_id, "9444900");
        assert_eq!(
            provider.requests[0].range.source_kind,
            NoaaTideSourceKind::Observation
        );
        assert_eq!(
            provider.cursors,
            [Some(NoaaTideProviderCursor {
                valid_at_ms: 1_788_221_160_000,
            })],
            "provider cursors remain provider evidence while resume uses chunk identity",
        );
        assert_eq!(repository.completions.len(), 1);
        assert_eq!(
            repository.completions[0].identity.interval_start.label(),
            "2026-09-01"
        );
        assert_eq!(repository.completions[0].records.len(), 1);
        assert_eq!(
            repository.completions[0].upstream_response_checksum,
            "b".repeat(64)
        );
        assert_eq!(repository.failures.len(), 1);
        assert_eq!(
            repository.failures[0].1.interval_start.label(),
            "2026-10-02"
        );
        assert_eq!(repository.failures[0].2.attempts, 2);
        assert_eq!(
            repository.failures[0].2.error.classification,
            TideBackfillErrorClassification::Retryable,
        );
        assert_eq!(repository.failures[0].2.error.code, "provider_server_error");
        assert_eq!(repository.failures[0].2.error.message, "fixture failure");
        assert!(
            repository
                .events
                .iter()
                .all(|event| !event.contains("2026-11-02")),
            "the fourth chunk must not execute after failure",
        );
    }

    // coerce an ordinary caught error to the exact Node durable failure shape
    #[test]
    fn tide_backfill_coerces_unexpected_failure_before_durable_finalization() {
        let source = observation_source();
        let arguments = TideBackfillArguments {
            dry_run: false,
            from: date("2026-08-01", 1_785_542_400_000),
            resume: false,
            site: "ballydidean".to_owned(),
            source_keys: vec![source.source.source_key.clone()],
            to: date("2026-08-01", 1_785_542_400_000),
        };
        let mut repository = FakeRepository::default();
        let mut provider = UnexpectedProvider::default();
        let mut time = FakeTime::fixture();

        let report = execute_tide_backfill(
            &arguments,
            "ballydidean",
            &[source],
            &mut repository,
            &mut provider,
            &mut time,
        )
        .expect("unexpected provider error should become a reported chunk failure");

        assert_eq!(report.exit_code, 1);
        assert_eq!(provider.requests.len(), 1);
        assert_eq!(repository.failures.len(), 1);
        let failure = &repository.failures[0].2;
        assert_eq!(failure.attempts, 1);
        assert_eq!(
            failure.error.classification,
            TideBackfillErrorClassification::Retryable,
        );
        assert_eq!(failure.error.code, "provider_unavailable");
        assert_eq!(failure.error.message, "socket exploded");
        assert_eq!(
            report.sources[0].failed_chunk,
            Some(super::TideBackfillFailedChunk {
                error_code: "provider_unavailable".to_owned(),
                start_date: "2026-08-01".to_owned(),
            }),
        );
    }

    // plan the exact compiled Node four-chunk date fixture
    #[test]
    fn tide_backfill_plans_exact_inclusive_date_chunks() {
        let source = observation_source();
        let from = date("2026-08-01", 1_785_542_400_000);
        let to = date("2026-11-05", 1_793_836_800_000);
        let mut time = FakeTime::fixture();

        let chunks = plan_tide_backfill_chunks(&source, &from, &to, &mut time)
            .expect("admitted dates should plan");

        let boundaries = chunks
            .iter()
            .map(|chunk| (chunk.start_date.label(), chunk.end_date.label()))
            .collect::<Vec<_>>();
        assert_eq!(
            boundaries,
            [
                ("2026-08-01", "2026-08-31"),
                ("2026-09-01", "2026-10-01"),
                ("2026-10-02", "2026-11-01"),
                ("2026-11-02", "2026-11-05"),
            ],
        );
        assert!(chunks.iter().all(|chunk| {
            chunk.identity.adapter_version == NOAA_TIDE_OBSERVATION_ADAPTER_VERSION
                && chunk.identity.chunk_plan_version == NOAA_TIDE_CHUNK_PLAN_VERSION
                && chunk.identity.requested_from_date == "2026-08-01"
                && chunk.identity.requested_to_date == "2026-11-05"
        }));
    }

    // reject unknown selectors identity drift and cross-site execution before side effects
    #[test]
    fn tide_backfill_selection_fails_closed_and_dry_run_stays_read_only() {
        let source = observation_source();
        let mut arguments = TideBackfillArguments {
            dry_run: true,
            from: date("2026-08-01", 1_785_542_400_000),
            resume: false,
            site: "ballydidean".to_owned(),
            source_keys: vec!["unknown-tide-source".to_owned()],
            to: date("2026-08-01", 1_785_542_400_000),
        };
        let unavailable =
            select_tide_backfill_sources(&arguments, "ballydidean", std::slice::from_ref(&source))
                .expect_err("unknown selector should fail");
        assert_eq!(unavailable.code, "tide_backfill_source_unavailable");

        arguments.source_keys = vec![source.source.source_key.clone()];
        let mut drifted = source.clone();
        drifted.source.material_config_identity = "drifted".to_owned();
        let identity =
            select_tide_backfill_sources(&arguments, "ballydidean", std::slice::from_ref(&drifted))
                .expect_err("identity drift should fail");
        assert_eq!(identity.code, "tide_backfill_identity_mismatch");

        let mut repository = FakeRepository::default();
        let mut provider = NormalizingProvider::default();
        let mut time = FakeTime::fixture();
        let report = execute_tide_backfill(
            &arguments,
            "ballydidean",
            &[source],
            &mut repository,
            &mut provider,
            &mut time,
        )
        .expect("dry run should complete without writes");
        assert_eq!(report.exit_code, 0);
        assert_eq!(report.sources[0].planned_chunks, 1);
        assert_eq!(report.sources[0].skipped_chunks, 1);
        assert!(repository.events.is_empty());
        assert!(provider.requests.is_empty());
    }
}
