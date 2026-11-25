//! Open-Meteo Archive planning and durable one-shot execution without transport or date codecs.

use weather_contracts::{
    BackfillArguments, BackfillChunkIdentity, BackfillChunkReport, BackfillChunkReportStatusEnum,
    BackfillReport, BackfillReportExitCodeEnum, BackfillSource, IngestionMode, JsonValue,
    NormalizedWeatherRecord, Nullable, SourceKind,
};

use super::tide_backfill::{
    TideBackfillDeadlines, TideBackfillOperationError, TideBackfillProviderFailure,
    as_backfill_provider_failure,
};
use super::{ProviderBatch, TaskFailure};

pub const OPEN_METEO_ARCHIVE_ADAPTER_VERSION: &str = "open-meteo-archive-hourly/v1";
pub const OPEN_METEO_ARCHIVE_CHUNK_PLAN_VERSION: &str = "open-meteo-archive-hourly/v1";
pub const OPEN_METEO_ARCHIVE_CONTRACT_VERSION: &str = "archive-hourly/v1";
pub const OPEN_METEO_ARCHIVE_ENDPOINT: &str = "archive/hourly";
pub const OPEN_METEO_ARCHIVE_SOURCE_KEY: &str = "open-meteo-reanalysis-v1";
pub const OPEN_METEO_ARCHIVE_MAXIMUM_CHUNK_DAYS: u64 = 14;

/// One canonical instant resolved by the caller's site-aware calendar owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenMeteoArchiveInstant {
    pub epoch_milliseconds: i64,
    pub utc: String,
}

/// An exact configured Archive source admitted before planning or repository work.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoArchiveSource {
    source: BackfillSource,
}

impl OpenMeteoArchiveSource {
    /// Admit the fixed Archive contract and its already-compared full configuration identity.
    pub fn admit(
        source: BackfillSource,
        contract_version: &str,
        maximum_chunk_days: u64,
        identity_matches_configuration: bool,
    ) -> Result<Self, TaskFailure> {
        // require the original unscheduled reanalysis identity before constructing the witness
        if source.key != OPEN_METEO_ARCHIVE_SOURCE_KEY
            || source.sourceKey != OPEN_METEO_ARCHIVE_SOURCE_KEY
            || source.sourceKind != SourceKind::Reanalysis
            || contract_version != OPEN_METEO_ARCHIVE_CONTRACT_VERSION
            || maximum_chunk_days != OPEN_METEO_ARCHIVE_MAXIMUM_CHUNK_DAYS
            || !identity_matches_configuration
            || !source.latitude.is_finite()
            || !(-90.0..=90.0).contains(&source.latitude)
            || !source.longitude.is_finite()
            || !(-180.0..=180.0).contains(&source.longitude)
            || source.id.is_empty()
            || source.siteSlug.is_empty()
            || source.timezone.is_empty()
            || !fingerprint_is_valid(&source.sourceConfigFingerprint)
        {
            return Err(worker_failure("open_meteo_archive_source_invalid"));
        }
        Ok(Self { source })
    }

    /// Borrow the admitted database source for runtime persistence binding.
    pub const fn source(&self) -> &BackfillSource {
        &self.source
    }
}

/// Site-calendar and deadline operations deliberately owned outside this crate.
pub trait OpenMeteoArchiveTimeAdapter {
    /// Add exact local calendar days to one canonical date label.
    fn add_calendar_days(&mut self, date: &str, days: u64) -> Result<String, TaskFailure>;

    /// Resolve one site-local midnight to its canonical UTC instant.
    fn local_midnight_utc(
        &mut self,
        date: &str,
        timezone: &str,
    ) -> Result<OpenMeteoArchiveInstant, TaskFailure>;

    /// Produce the ordinary provider and durable-run deadlines for one chunk.
    fn deadlines(&mut self) -> Result<TideBackfillDeadlines, TideBackfillOperationError>;
}

/// One deterministic local-calendar chunk and its durable identity.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedOpenMeteoArchiveChunk {
    pub end_date: String,
    pub end_exclusive_date: String,
    pub identity: BackfillChunkIdentity,
    pub interval_end_exclusive_ms: i64,
    pub interval_start_ms: i64,
    pub start_date: String,
}

/// Metadata committed before calling the Archive provider.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoArchiveRunStart {
    pub adapter_version: &'static str,
    pub chunk_plan_version: &'static str,
    pub deadline_at_ms: i64,
    pub end_date: String,
    pub endpoint: &'static str,
    pub mode: IngestionMode,
    pub requested_end_exclusive: String,
    pub requested_start: String,
    pub source_config_fingerprint: String,
    pub start_date: String,
}

/// Exact typed request passed to the runtime Archive provider adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoArchiveRequest {
    pub deadline_at_ms: i64,
    pub end_date: String,
    pub end_exclusive_date: String,
    pub interval_end_exclusive_ms: i64,
    pub interval_start_ms: i64,
    pub latitude: f64,
    pub longitude: f64,
    pub source_id: String,
    pub start_date: String,
    pub timezone: String,
}

/// One atomic successful Archive completion.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoArchiveCompletion {
    pub attempts: u32,
    pub identity: BackfillChunkIdentity,
    pub records: Vec<NormalizedWeatherRecord>,
    pub response_metadata: JsonValue,
    pub run_id: String,
    pub upstream_response_checksum: String,
}

/// Already redacted text from the approved worker diagnostic owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenMeteoArchiveCleanupFailure {
    message: String,
}

impl OpenMeteoArchiveCleanupFailure {
    /// Preserve ordinary diagnostic text instead of the driver's categorical code.
    pub const fn admitted(message: String) -> Self {
        Self { message }
    }

    /// Apply an original worker-report UTF-16 limit to admitted diagnostics.
    pub(crate) fn bounded_message(&self, maximum: usize) -> String {
        bounded_cleanup(&self.message, maximum)
    }
}

/// Join the original named cleanup diagnostics without masking the primary result.
pub(crate) fn append_release_diagnostic(
    finalization: Option<String>,
    release: &OpenMeteoArchiveCleanupFailure,
) -> String {
    let release = release.bounded_message(220);
    match finalization {
        None => format!("release={release}"),
        Some(previous) => format!(
            "finalization={}; release={release}",
            bounded_cleanup(&previous, 220)
        ),
    }
}

/// Durable operations required by the sequential Archive loop.
pub trait OpenMeteoArchiveRepository {
    type Session;

    /// Check exact successful identity before dry-run and lock acquisition.
    fn has_successful_chunk(
        &mut self,
        identity: &BackfillChunkIdentity,
    ) -> Result<bool, TaskFailure>;

    /// Acquire the source-scoped retained session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure>;

    /// Abandon expired running rows before committing a new run.
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        started_at_ms: i64,
    ) -> Result<(), TideBackfillOperationError>;

    /// Commit the running row before provider traffic.
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: OpenMeteoArchiveRunStart,
    ) -> Result<String, TideBackfillOperationError>;

    /// Commit normalized records, evidence, and exact chunk outcome atomically.
    fn complete_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: OpenMeteoArchiveCompletion,
    ) -> Result<(), TideBackfillOperationError>;

    /// Finalize a committed running row without replacing its primary failure.
    fn fail_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        run_id: &str,
        identity: &BackfillChunkIdentity,
        failure: &TideBackfillProviderFailure,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure>;

    /// Release the retained source session after all primary work.
    fn release_source_session(
        &mut self,
        session: Self::Session,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure>;
}

/// Typed provider boundary; transport and payload decoding remain runtime-owned.
pub trait OpenMeteoArchiveProvider {
    /// Fetch one normalized Archive batch within the absolute deadline.
    fn fetch(
        &mut self,
        request: OpenMeteoArchiveRequest,
    ) -> Result<ProviderBatch<NormalizedWeatherRecord, JsonValue, ()>, TideBackfillOperationError>;
}

/// Plan deterministic site-local chunks with DST-aware midnight boundaries.
pub fn plan_open_meteo_archive_chunks<T>(
    arguments: &BackfillArguments,
    source: &OpenMeteoArchiveSource,
    time: &mut T,
) -> Result<Vec<PlannedOpenMeteoArchiveChunk>, TaskFailure>
where
    T: OpenMeteoArchiveTimeAdapter,
{
    validate_arguments(arguments, source)?;
    let mut chunks = Vec::new();
    let mut start_date = arguments.from.clone();

    // cover the inclusive request range through the injected local calendar
    while start_date <= arguments.to {
        let maximum_end = time.add_calendar_days(&start_date, arguments.chunkDays - 1)?;
        // clamp the last chunk to the inclusive requested end date
        let end_date = if maximum_end < arguments.to {
            maximum_end
        } else {
            arguments.to.clone()
        };
        let end_exclusive_date = time.add_calendar_days(&end_date, 1)?;
        let interval_start = time.local_midnight_utc(&start_date, &source.source.timezone)?;
        let interval_end = time.local_midnight_utc(&end_exclusive_date, &source.source.timezone)?;

        // require a positive admitted local-day interval, including DST-short or DST-long days
        if end_date < start_date
            || end_exclusive_date <= end_date
            || interval_start.utc.is_empty()
            || interval_end.utc.is_empty()
            || interval_start.epoch_milliseconds >= interval_end.epoch_milliseconds
        {
            return Err(worker_failure("open_meteo_archive_calendar_invalid"));
        }
        chunks.push(PlannedOpenMeteoArchiveChunk {
            end_date,
            end_exclusive_date: end_exclusive_date.clone(),
            identity: BackfillChunkIdentity {
                adapterVersion: OPEN_METEO_ARCHIVE_ADAPTER_VERSION.to_owned(),
                chunkPlanVersion: OPEN_METEO_ARCHIVE_CHUNK_PLAN_VERSION.to_owned(),
                intervalEndExclusive: interval_end.utc,
                intervalStart: interval_start.utc,
                requestedFromDate: arguments.from.clone(),
                requestedToDate: arguments.to.clone(),
                sourceConfigFingerprint: source.source.sourceConfigFingerprint.clone(),
                sourceId: source.source.id.clone(),
            },
            interval_end_exclusive_ms: interval_end.epoch_milliseconds,
            interval_start_ms: interval_start.epoch_milliseconds,
            start_date: start_date.clone(),
        });
        start_date = end_exclusive_date;
    }
    Ok(chunks)
}

/// Execute sequential Archive chunks with exact resume and durable failure semantics.
pub fn execute_open_meteo_archive_backfill<R, P, T>(
    arguments: &BackfillArguments,
    source: &OpenMeteoArchiveSource,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<BackfillReport, TaskFailure>
where
    R: OpenMeteoArchiveRepository,
    P: OpenMeteoArchiveProvider,
    T: OpenMeteoArchiveTimeAdapter,
{
    let chunks = plan_open_meteo_archive_chunks(arguments, source, time)?;
    let mut reports = Vec::with_capacity(chunks.len());

    // execute in ascending local-date order and stop after the first terminal chunk
    for (index, chunk) in chunks.iter().enumerate() {
        // preserve Node ordering where resume lookup precedes dry-run handling
        if arguments.resume && repository.has_successful_chunk(&chunk.identity)? {
            reports.push(chunk_report(
                chunk,
                BackfillChunkReportStatusEnum::Skipped,
                None,
                None,
            ));
            continue;
        }

        // prove dry-run has no lock, provider, or durable write side effects
        if arguments.dryRun {
            reports.push(chunk_report(
                chunk,
                BackfillChunkReportStatusEnum::Planned,
                None,
                None,
            ));
            continue;
        }
        let Some(session) = repository.acquire_source_session(&source.source.id)? else {
            reports.push(chunk_report(
                chunk,
                BackfillChunkReportStatusEnum::Abandoned,
                Some("source_locked".to_owned()),
                None,
            ));
            append_remaining(&mut reports, &chunks[index + 1..]);
            return Ok(create_report(arguments, source, reports, true));
        };
        let result = execute_chunk(chunk, source, session, repository, provider, time)?;
        let completed = result.status == BackfillChunkReportStatusEnum::Completed;
        reports.push(result);

        // append untouched planned identities after the first primary failure
        if !completed {
            append_remaining(&mut reports, &chunks[index + 1..]);
            return Ok(create_report(arguments, source, reports, true));
        }
    }
    Ok(create_report(arguments, source, reports, false))
}

/// Execute one committed chunk and preserve primary outcome across finalization and release.
fn execute_chunk<R, P, T>(
    chunk: &PlannedOpenMeteoArchiveChunk,
    source: &OpenMeteoArchiveSource,
    mut session: R::Session,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<BackfillChunkReport, TaskFailure>
where
    R: OpenMeteoArchiveRepository,
    P: OpenMeteoArchiveProvider,
    T: OpenMeteoArchiveTimeAdapter,
{
    let mut run_id = None;
    let mut attempts = 0;
    let execution = (|| {
        let deadlines = time.deadlines()?;
        validate_deadlines(deadlines)?;
        repository.abandon_expired_runs(&mut session, deadlines.started_at_ms)?;
        let started_id = repository.start_ingestion_run(
            &mut session,
            OpenMeteoArchiveRunStart {
                adapter_version: OPEN_METEO_ARCHIVE_ADAPTER_VERSION,
                chunk_plan_version: OPEN_METEO_ARCHIVE_CHUNK_PLAN_VERSION,
                deadline_at_ms: deadlines.run_deadline_at_ms,
                end_date: chunk.end_date.clone(),
                endpoint: OPEN_METEO_ARCHIVE_ENDPOINT,
                mode: IngestionMode::Backfill,
                requested_end_exclusive: chunk.identity.intervalEndExclusive.clone(),
                requested_start: chunk.identity.intervalStart.clone(),
                source_config_fingerprint: chunk.identity.sourceConfigFingerprint.clone(),
                start_date: chunk.start_date.clone(),
            },
        )?;
        run_id = Some(started_id.clone());
        let batch = provider.fetch(OpenMeteoArchiveRequest {
            deadline_at_ms: deadlines.provider_deadline_at_ms,
            end_date: chunk.end_date.clone(),
            end_exclusive_date: chunk.end_exclusive_date.clone(),
            interval_end_exclusive_ms: chunk.interval_end_exclusive_ms,
            interval_start_ms: chunk.interval_start_ms,
            latitude: source.source.latitude,
            longitude: source.source.longitude,
            source_id: source.source.id.clone(),
            start_date: chunk.start_date.clone(),
            timezone: source.source.timezone.clone(),
        })?;
        attempts = batch.attempts;
        assert_records_within_chunk(&batch.records, chunk)?;
        repository.complete_backfill_ingestion(
            &mut session,
            OpenMeteoArchiveCompletion {
                attempts,
                identity: chunk.identity.clone(),
                records: batch.records,
                response_metadata: batch.response_metadata,
                run_id: started_id,
                upstream_response_checksum: batch.upstream_response_checksum,
            },
        )?;
        Ok::<(), TideBackfillOperationError>(())
    })();
    let mut report = match execution {
        Ok(()) => chunk_report(chunk, BackfillChunkReportStatusEnum::Completed, None, None),
        Err(error) => {
            let failure = as_backfill_provider_failure(error, attempts.max(1));
            let mut secondary = None;

            // finalize only after a committed start and retain its failure separately
            if let Some(started_id) = run_id.as_deref()
                && let Err(finalization) = repository.fail_ingestion_run(
                    &mut session,
                    started_id,
                    &chunk.identity,
                    &failure,
                )
            {
                secondary = Some(finalization.bounded_message(512));
            }
            chunk_report(
                chunk,
                BackfillChunkReportStatusEnum::Failed,
                Some(failure.error.code),
                secondary,
            )
        }
    };

    // retain completion while surfacing cleanup as a bounded secondary diagnostic
    if let Err(release) = repository.release_source_session(session) {
        let finalization = match report.secondaryError {
            Nullable::Null => None,
            Nullable::Value(previous) => Some(previous),
        };
        report.secondaryError = Nullable::Value(append_release_diagnostic(finalization, &release));
    }
    Ok(report)
}

/// Require a nonempty in-window batch before the original DB identity checks.
fn assert_records_within_chunk(
    records: &[NormalizedWeatherRecord],
    chunk: &PlannedOpenMeteoArchiveChunk,
) -> Result<(), TideBackfillOperationError> {
    // preserve the ordinary Node error and actual batch attempts for a bad completion
    if records.is_empty() {
        return Err(TideBackfillOperationError::unexpected(
            "archive payload contained no records",
        ));
    }
    // reject escaped provider results before entering the completion transaction
    if records.iter().any(|record| {
        record.validAt.as_str() < chunk.identity.intervalStart.as_str()
            || record.validAt.as_str() >= chunk.identity.intervalEndExclusive.as_str()
    }) {
        return Err(TideBackfillOperationError::unexpected(
            "archive payload contained records outside the requested chunk",
        ));
    }
    Ok(())
}

/// Bound already-redacted text with the original member limits.
fn bounded_cleanup(message: &str, maximum: usize) -> String {
    let trimmed = message.trim();
    // retain the original ordinary fallback when no diagnostic remains
    if trimmed.is_empty() {
        return "worker operation failed".to_owned();
    }
    let mut remaining = maximum;
    trimmed
        .chars()
        .take_while(|character| {
            // count UTF-16 units without splitting a scalar at the limit
            let Some(next) = remaining.checked_sub(character.len_utf16()) else {
                return false;
            };
            remaining = next;
            true
        })
        .collect()
}

/// Validate the already-parsed command and exact selected source.
fn validate_arguments(
    arguments: &BackfillArguments,
    source: &OpenMeteoArchiveSource,
) -> Result<(), TaskFailure> {
    let selected_source = match &arguments.source {
        Nullable::Null => OPEN_METEO_ARCHIVE_SOURCE_KEY,
        Nullable::Value(value) => value,
    };

    // require exact site/source and the parser's bounded inclusive date contract
    if arguments.site != source.source.siteSlug
        || selected_source != source.source.key
        || !(1..=OPEN_METEO_ARCHIVE_MAXIMUM_CHUNK_DAYS).contains(&arguments.chunkDays)
        || arguments.from.is_empty()
        || arguments.to.is_empty()
        || arguments.from.as_str() > arguments.to.as_str()
    {
        return Err(worker_failure("open_meteo_archive_arguments_invalid"));
    }
    Ok(())
}

/// Validate the provider deadline precedes durable finalization.
fn validate_deadlines(deadlines: TideBackfillDeadlines) -> Result<(), TideBackfillOperationError> {
    // refuse invalid injected clocks inside the primary catch boundary
    if deadlines.provider_deadline_at_ms <= deadlines.started_at_ms
        || deadlines.run_deadline_at_ms <= deadlines.provider_deadline_at_ms
    {
        return Err(TideBackfillOperationError::unexpected(
            "archive deadlines invalid",
        ));
    }
    Ok(())
}

/// Append later identities without attempting them.
fn append_remaining(
    reports: &mut Vec<BackfillChunkReport>,
    chunks: &[PlannedOpenMeteoArchiveChunk],
) {
    // preserve original deterministic chunk order
    for chunk in chunks {
        reports.push(chunk_report(
            chunk,
            BackfillChunkReportStatusEnum::Remaining,
            None,
            None,
        ));
    }
}

/// Construct one generated report row.
fn chunk_report(
    chunk: &PlannedOpenMeteoArchiveChunk,
    status: BackfillChunkReportStatusEnum,
    error_code: Option<String>,
    secondary_error: Option<String>,
) -> BackfillChunkReport {
    BackfillChunkReport {
        errorCode: nullable(error_code),
        identity: chunk.identity.clone(),
        secondaryError: nullable(secondary_error),
        status,
    }
}

/// Construct the generated one-shot report.
fn create_report(
    arguments: &BackfillArguments,
    source: &OpenMeteoArchiveSource,
    chunks: Vec<BackfillChunkReport>,
    failed: bool,
) -> BackfillReport {
    // project the binary process outcome to the generated literal enum
    let exit_code = if failed {
        BackfillReportExitCodeEnum::Value1
    } else {
        BackfillReportExitCodeEnum::Value0
    };
    BackfillReport {
        chunks,
        dryRun: arguments.dryRun,
        exitCode: exit_code,
        from: arguments.from.clone(),
        resume: arguments.resume,
        site: arguments.site.clone(),
        source: source.source.key.clone(),
        to: arguments.to.clone(),
    }
}

/// Convert an optional owned value to the generated nullable representation.
fn nullable<T>(value: Option<T>) -> Nullable<T> {
    match value {
        Some(value) => Nullable::Value(value),
        None => Nullable::Null,
    }
}

/// Require one lowercase SHA-256 identity.
fn fingerprint_is_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Construct one stable worker-boundary failure.
fn worker_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
