//! Typed public-station planning and archive execution without transport or date codecs.

use std::collections::BTreeSet;

use super::tide_backfill::{
    TideBackfillErrorClassification, TideBackfillIngestionError, TideBackfillOperationError,
    TideBackfillProviderFailure, as_backfill_provider_failure,
};
use super::{
    AMBIENT_WEATHER_ADAPTER_VERSION, AmbientWeatherProviderCursor, AmbientWeatherRangeRequest,
    AmbientWeatherResponseMetadata, AmbientWeatherRuntimeConfiguration,
    AmbientWeatherScheduledSource, NormalizedAmbientWeatherRecord, ProviderBatch, ScheduledWindow,
    SupplementalPublicStationBatch, SupplementalPublicStationRecord,
    SupplementalPublicStationResponseMetadata, TaskFailure, public_stations,
    supplemental_public_station_contract,
};
use weather_contracts::PublicStationAdapter;

pub const PUBLIC_STATION_CHUNK_PLAN_VERSION: &str = "public-station-calendar-range/v1";
pub const AMBIENT_WEATHER_BACKFILL_ENDPOINT: &str = "ambient-weather";

const MILLISECONDS_PER_DAY: i64 = 86_400_000;

pub type PublicStationBackfillErrorClassification = TideBackfillErrorClassification;
pub type PublicStationBackfillIngestionError = TideBackfillIngestionError;
pub type PublicStationBackfillOperationError = TideBackfillOperationError;
pub type PublicStationBackfillProviderFailure = TideBackfillProviderFailure;

/// One UTC date admitted by the caller's date implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicStationBackfillDate {
    label: String,
    start_ms: i64,
}

impl PublicStationBackfillDate {
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
pub struct PublicStationBackfillDeadlines {
    pub provider_deadline_at_ms: i64,
    pub run_deadline_at_ms: i64,
    pub started_at_ms: i64,
}

/// Time operations required by planning and committed chunk execution.
pub trait PublicStationBackfillTimeAdapter {
    /// Return the current admitted UTC date used to exclude the partial day.
    fn today_utc(&mut self) -> Result<PublicStationBackfillDate, TaskFailure>;

    /// Add exact UTC calendar days through an admitted date implementation.
    fn add_utc_days(
        &mut self,
        date: &PublicStationBackfillDate,
        days: i64,
    ) -> Result<PublicStationBackfillDate, TaskFailure>;

    /// Return one bounded provider and durable-run deadline plan.
    fn deadlines(
        &mut self,
    ) -> Result<PublicStationBackfillDeadlines, PublicStationBackfillOperationError>;
}

/// One typed one-shot public-station backfill request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicStationBackfillArguments {
    pub dry_run: bool,
    pub from: Option<PublicStationBackfillDate>,
    pub resume: bool,
    pub site: String,
    pub source_keys: Vec<String>,
    pub to: PublicStationBackfillDate,
}

/// One Ambient configuration joined to its active storage identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmbientWeatherBackfillSource {
    pub configuration: AmbientWeatherRuntimeConfiguration,
    pub history_end: Option<PublicStationBackfillDate>,
    pub history_start: PublicStationBackfillDate,
    pub provider_active: bool,
    pub source: AmbientWeatherScheduledSource,
    pub station_active: bool,
    pub storage_active: bool,
}

/// One provider-neutral historical source admitted for dry-run planning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicStationBackfillPlanningSource {
    pub adapter: PublicStationAdapter,
    pub source_id: String,
    pub source_key: String,
    pub source_config_fingerprint: String,
    pub history_start: PublicStationBackfillDate,
    pub history_end: Option<PublicStationBackfillDate>,
    pub maximum_chunk_days: u64,
}

/// One provider-neutral historical source with its checked station identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicStationArchiveBackfillSource {
    pub planning: PublicStationBackfillPlanningSource,
    pub station_key: String,
}

/// One exact durable public-station chunk identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicStationBackfillChunkIdentity {
    pub adapter_version: &'static str,
    pub chunk_plan_version: &'static str,
    pub interval_end_exclusive: PublicStationBackfillDate,
    pub interval_start: PublicStationBackfillDate,
    pub requested_from_date: String,
    pub requested_to_date: String,
    pub source_config_fingerprint: String,
    pub source_id: String,
}

/// One inclusive-date chunk and its durable half-open identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedPublicStationBackfillChunk {
    pub end_date: PublicStationBackfillDate,
    pub identity: PublicStationBackfillChunkIdentity,
    pub start_date: PublicStationBackfillDate,
}

/// Metadata committed before one Ambient provider request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicStationBackfillRunStart {
    pub adapter_version: &'static str,
    pub chunk_plan_version: &'static str,
    pub deadline_at_ms: i64,
    pub endpoint: &'static str,
    pub requested_end_exclusive_ms: i64,
    pub requested_start_ms: i64,
    pub source_config_fingerprint: String,
    pub station_key: String,
}

/// One successful atomic public-station backfill completion.
#[derive(Clone, Debug, PartialEq)]
pub struct PublicStationBackfillCompletion<
    Record = NormalizedAmbientWeatherRecord,
    Metadata = AmbientWeatherResponseMetadata,
> {
    pub attempts: u32,
    pub identity: PublicStationBackfillChunkIdentity,
    pub records: Vec<Record>,
    pub response_metadata: Metadata,
    pub run_id: String,
    pub upstream_response_checksum: String,
}

/// Persistence for normalized non-Ambient public-station archive records.
pub trait SupplementalPublicStationBackfillRepository: PublicStationBackfillRepository {
    /// Commit actual supplemental records and successful chunk identity atomically.
    fn complete_supplemental_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: PublicStationBackfillCompletion<
            SupplementalPublicStationRecord,
            SupplementalPublicStationResponseMetadata,
        >,
    ) -> Result<(), PublicStationBackfillOperationError>;
}

/// Persistence required by one typed Ambient backfill.
pub trait PublicStationBackfillRepository {
    type Session;

    /// Report whether this exact durable chunk already succeeded.
    fn has_successful_chunk(
        &mut self,
        identity: &PublicStationBackfillChunkIdentity,
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
    ) -> Result<(), PublicStationBackfillOperationError>;

    /// Commit one running backfill row.
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: PublicStationBackfillRunStart,
    ) -> Result<String, PublicStationBackfillOperationError>;

    /// Commit actual records and successful chunk identity atomically.
    fn complete_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: PublicStationBackfillCompletion,
    ) -> Result<(), PublicStationBackfillOperationError>;

    /// Finalize one committed failed chunk.
    fn fail_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        run_id: &str,
        identity: &PublicStationBackfillChunkIdentity,
        failure: &PublicStationBackfillProviderFailure,
    ) -> Result<(), TaskFailure>;

    /// Release one source-scoped session.
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure>;
}

/// One typed Ambient provider operation required by the backfill loop.
pub trait AmbientWeatherBackfillProvider {
    /// Fetch one normalized Ambient batch.
    fn fetch(
        &mut self,
        request: AmbientWeatherRangeRequest,
        deadline_at_ms: i64,
    ) -> Result<
        ProviderBatch<
            NormalizedAmbientWeatherRecord,
            AmbientWeatherResponseMetadata,
            Option<AmbientWeatherProviderCursor>,
        >,
        PublicStationBackfillOperationError,
    >;
}

/// One provider-neutral result admitted by the archive execution boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum PublicStationArchiveBackfillBatch {
    Ambient(
        ProviderBatch<
            NormalizedAmbientWeatherRecord,
            AmbientWeatherResponseMetadata,
            Option<AmbientWeatherProviderCursor>,
        >,
    ),
    Supplemental(SupplementalPublicStationBatch),
}

/// Provider operations required by the provider-neutral archive loop.
pub trait PublicStationArchiveBackfillProvider {
    /// Fetch one normalized batch for the checked source and half-open UTC window.
    fn fetch(
        &mut self,
        source: &PublicStationArchiveBackfillSource,
        window: ScheduledWindow,
        deadline_at_ms: i64,
    ) -> Result<PublicStationArchiveBackfillBatch, PublicStationBackfillOperationError>;

    /// Apply the provider-owned inter-chunk pacing delay.
    fn pace_between_chunks(&mut self, delay_ms: u64);
}

/// One failed source chunk retained in the final report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicStationBackfillFailedChunk {
    pub error_code: String,
    pub start_date: String,
}

/// One source-level public-station backfill report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicStationBackfillSourceReport {
    pub completed_chunks: usize,
    pub exit_code: u8,
    pub failed_chunk: Option<PublicStationBackfillFailedChunk>,
    pub from: String,
    pub planned_chunks: usize,
    pub records: usize,
    pub skipped_chunks: usize,
    pub source: String,
    pub to: String,
}

/// One complete one-shot public-station backfill report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicStationBackfillReport {
    pub dry_run: bool,
    pub exit_code: u8,
    pub resume: bool,
    pub site: String,
    pub sources: Vec<PublicStationBackfillSourceReport>,
}

/// Resolve selected historical Ambient sources in checked catalog order.
pub fn select_ambient_weather_backfill_sources<'a>(
    arguments: &PublicStationBackfillArguments,
    configuration_site: &str,
    candidates: &'a [AmbientWeatherBackfillSource],
) -> Result<Vec<&'a AmbientWeatherBackfillSource>, TaskFailure> {
    // reject cross-site execution
    if arguments.site != configuration_site {
        return Err(backfill_failure("public_station_backfill_site_mismatch", 0));
    }
    let selectors = arguments.source_keys.iter().collect::<BTreeSet<_>>();

    // require the parsed selector contract
    if selectors.len() != arguments.source_keys.len()
        || arguments
            .source_keys
            .iter()
            .any(|value| !stable_key_is_valid(value))
    {
        return Err(backfill_failure(
            "public_station_backfill_selector_invalid",
            0,
        ));
    }
    let mut selected = Vec::new();

    // preserve checked catalog station and source order
    for candidate in candidates {
        let configured_historical = candidate.station_active
            && candidate.provider_active
            && candidate.configuration.source_active
            && candidate
                .configuration
                .capabilities
                .contains(&super::AmbientWeatherCapability::Historical);
        let selector_matches =
            selectors.is_empty() || selectors.contains(&candidate.configuration.source_key);

        // exclude inactive nonhistorical or unselected configuration
        if !configured_historical || !selector_matches {
            continue;
        }

        // require the active storage join used by the Node resolver
        if !candidate.storage_active || !candidate.source.active {
            return Err(backfill_failure(
                "public_station_backfill_source_missing",
                0,
            ));
        }

        // reject material identity drift before provider work
        if candidate.configuration.adapter_contract_version != AMBIENT_WEATHER_ADAPTER_VERSION
            || !fingerprint_is_valid(&candidate.source.source_config_fingerprint)
            || !public_stations::ambient_weather_backfill_source_matches_configuration(
                &candidate.source,
                &candidate.configuration,
            )
        {
            return Err(backfill_failure(
                "public_station_backfill_identity_mismatch",
                0,
            ));
        }
        selected.push(candidate);
    }

    // reject unknown inactive or nonhistorical selectors
    if !selectors.is_empty() && selected.len() != selectors.len() {
        return Err(backfill_failure(
            "public_station_backfill_source_unavailable",
            0,
        ));
    }

    Ok(selected)
}

/// Plan every exact inclusive-date Ambient chunk.
pub fn plan_ambient_weather_backfill_chunks<T: PublicStationBackfillTimeAdapter>(
    source: &AmbientWeatherBackfillSource,
    from: &PublicStationBackfillDate,
    to: &PublicStationBackfillDate,
    time: &mut T,
) -> Result<Vec<PlannedPublicStationBackfillChunk>, TaskFailure> {
    plan_backfill_chunks(
        AMBIENT_WEATHER_ADAPTER_VERSION,
        &source.source.id,
        &source.source.source_config_fingerprint,
        u64::from(source.configuration.maximum_chunk_days),
        from,
        to,
        time,
    )
}

/// Plan every selected provider-neutral source without provider or state writes.
pub fn plan_public_station_backfill_dry_run(
    arguments: &PublicStationBackfillArguments,
    configuration_site: &str,
    sources: &[PublicStationBackfillPlanningSource],
    repository: &mut impl PublicStationBackfillRepository,
    time: &mut impl PublicStationBackfillTimeAdapter,
) -> Result<PublicStationBackfillReport, TaskFailure> {
    // prevent the planning-only path from silently accepting committed execution
    if !arguments.dry_run {
        return Err(backfill_failure(
            "public_station_backfill_dry_run_required",
            0,
        ));
    }
    validate_arguments(arguments, time)?;
    let selected = select_planning_sources(arguments, configuration_site, sources)?;
    let mut reports = Vec::with_capacity(selected.len());

    // preserve the checked catalog order in the final report
    for source in selected {
        reports.push(plan_source_dry_run(arguments, source, repository, time)?);
    }

    Ok(PublicStationBackfillReport {
        dry_run: true,
        exit_code: 0,
        resume: arguments.resume,
        site: arguments.site.clone(),
        sources: reports,
    })
}

// plan exact chunks through the policy shared by every public-station adapter
fn plan_backfill_chunks<T: PublicStationBackfillTimeAdapter>(
    adapter_version: &'static str,
    source_id: &str,
    source_config_fingerprint: &str,
    maximum_chunk_days: u64,
    from: &PublicStationBackfillDate,
    to: &PublicStationBackfillDate,
    time: &mut T,
) -> Result<Vec<PlannedPublicStationBackfillChunk>, TaskFailure> {
    validate_admitted_date(from)?;
    validate_admitted_date(to)?;

    // require the exact public-station provider range ceiling
    if !(1..=31).contains(&maximum_chunk_days) {
        return Err(backfill_failure(
            "public_station_backfill_chunk_range_invalid",
            0,
        ));
    }
    let maximum_chunk_days = maximum_chunk_days as i64;
    let mut chunks = Vec::new();
    let mut start_date = from.clone();

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
        let identity = PublicStationBackfillChunkIdentity {
            adapter_version,
            chunk_plan_version: PUBLIC_STATION_CHUNK_PLAN_VERSION,
            interval_end_exclusive: interval_end_exclusive.clone(),
            interval_start: start_date.clone(),
            requested_from_date: from.label.clone(),
            requested_to_date: to.label.clone(),
            source_config_fingerprint: source_config_fingerprint.to_owned(),
            source_id: source_id.to_owned(),
        };
        validate_chunk_identity(&identity)?;
        chunks.push(PlannedPublicStationBackfillChunk {
            end_date,
            identity,
            start_date,
        });
        start_date = interval_end_exclusive;
    }

    Ok(chunks)
}

// resolve selected provider-neutral sources in their original catalog order
fn select_planning_sources<'a>(
    arguments: &PublicStationBackfillArguments,
    configuration_site: &str,
    sources: &'a [PublicStationBackfillPlanningSource],
) -> Result<Vec<&'a PublicStationBackfillPlanningSource>, TaskFailure> {
    // reject cross-site execution
    if arguments.site != configuration_site {
        return Err(backfill_failure("public_station_backfill_site_mismatch", 0));
    }
    let selectors = arguments.source_keys.iter().collect::<BTreeSet<_>>();

    // require unique stable selectors from the parsed command contract
    if selectors.len() != arguments.source_keys.len()
        || arguments
            .source_keys
            .iter()
            .any(|value| !stable_key_is_valid(value))
    {
        return Err(backfill_failure(
            "public_station_backfill_selector_invalid",
            0,
        ));
    }
    let mut selected = Vec::new();
    let mut selected_keys = BTreeSet::new();

    // filter without reordering the resolved source catalog
    for source in sources {
        let selector_matches = selectors.is_empty() || selectors.contains(&source.source_key);

        // leave unrequested catalog entries untouched
        if !selector_matches {
            continue;
        }
        validate_planning_source(source)?;

        // reject duplicate resolved identities even without explicit selectors
        if !selected_keys.insert(&source.source_key) {
            return Err(backfill_failure(
                "public_station_backfill_identity_mismatch",
                0,
            ));
        }
        selected.push(source);
    }

    // reject unknown or unavailable source selectors
    if !selectors.is_empty() && selected.len() != selectors.len() {
        return Err(backfill_failure(
            "public_station_backfill_source_unavailable",
            0,
        ));
    }

    Ok(selected)
}

// validate one provider-neutral historical source before range clamping
fn validate_planning_source(
    source: &PublicStationBackfillPlanningSource,
) -> Result<(), TaskFailure> {
    validate_admitted_date(&source.history_start)?;

    // validate and order the optional configured upper bound
    if let Some(history_end) = &source.history_end {
        validate_admitted_date(history_end)?;

        // reject reversed catalog history before planning an empty range
        if history_end.start_ms < source.history_start.start_ms {
            return Err(backfill_failure(
                "public_station_backfill_identity_mismatch",
                0,
            ));
        }
    }

    // require the bounded catalog chunk policy even for an empty requested range
    if !(1..=31).contains(&source.maximum_chunk_days) {
        return Err(backfill_failure(
            "public_station_backfill_chunk_range_invalid",
            0,
        ));
    }

    // retain the durable source identity used by exact resume lookups
    if !stable_key_is_valid(&source.source_key)
        || source.source_id.trim().is_empty()
        || source.source_id.len() > 128
        || !fingerprint_is_valid(&source.source_config_fingerprint)
    {
        return Err(backfill_failure(
            "public_station_backfill_identity_mismatch",
            0,
        ));
    }

    Ok(())
}

// produce one all-skipped source report while retaining exact resume reads
fn plan_source_dry_run(
    arguments: &PublicStationBackfillArguments,
    source: &PublicStationBackfillPlanningSource,
    repository: &mut impl PublicStationBackfillRepository,
    time: &mut impl PublicStationBackfillTimeAdapter,
) -> Result<PublicStationBackfillSourceReport, TaskFailure> {
    let from = later_date(source.history_start.clone(), arguments.from.clone());
    let to = earlier_date(arguments.to.clone(), source.history_end.clone());

    // preserve the Node empty-range report without manufacturing a chunk
    let chunks = if from.start_ms > to.start_ms {
        Vec::new()
    } else {
        plan_backfill_chunks(
            public_station_adapter_version(source.adapter),
            &source.source_id,
            &source.source_config_fingerprint,
            source.maximum_chunk_days,
            &from,
            &to,
            time,
        )?
    };
    let mut skipped_chunks = 0;

    // perform exact-success reads before the dry-run skip boundary
    for chunk in &chunks {
        // preserve resume lookup and error propagation even though every chunk is skipped
        if arguments.resume && repository.has_successful_chunk(&chunk.identity)? {
            skipped_chunks += 1;
            continue;
        }
        skipped_chunks += 1;
    }

    Ok(PublicStationBackfillSourceReport {
        completed_chunks: 0,
        exit_code: 0,
        failed_chunk: None,
        from: from.label,
        planned_chunks: chunks.len(),
        records: 0,
        skipped_chunks,
        source: source.source_key.clone(),
        to: to.label,
    })
}

// select the exact retained ingestion contract for one public-station adapter
fn public_station_adapter_version(adapter: PublicStationAdapter) -> &'static str {
    match supplemental_public_station_contract(adapter) {
        Some((adapter_version, _)) => adapter_version,
        None => AMBIENT_WEATHER_ADAPTER_VERSION,
    }
}

/// Execute every selected provider through the shared exact-resume chunk lifecycle.
pub fn execute_public_station_archive_backfill<R, P, T>(
    arguments: &PublicStationBackfillArguments,
    configuration_site: &str,
    sources: &[PublicStationArchiveBackfillSource],
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<PublicStationBackfillReport, TaskFailure>
where
    P: PublicStationArchiveBackfillProvider,
    R: SupplementalPublicStationBackfillRepository,
    T: PublicStationBackfillTimeAdapter,
{
    validate_arguments(arguments, time)?;
    let selected = select_archive_sources(arguments, configuration_site, sources)?;
    let mut reports = Vec::with_capacity(selected.len());

    // isolate each checked source archive while retaining catalog order
    for source in selected {
        reports.push(execute_archive_source(
            arguments, source, repository, provider, time,
        )?);
    }
    let exit_code = u8::from(reports.iter().any(|report| report.exit_code == 1));

    Ok(PublicStationBackfillReport {
        dry_run: arguments.dry_run,
        exit_code,
        resume: arguments.resume,
        site: arguments.site.clone(),
        sources: reports,
    })
}

// resolve provider-neutral executable sources without reordering the checked catalog
fn select_archive_sources<'a>(
    arguments: &PublicStationBackfillArguments,
    configuration_site: &str,
    sources: &'a [PublicStationArchiveBackfillSource],
) -> Result<Vec<&'a PublicStationArchiveBackfillSource>, TaskFailure> {
    // reject cross-site execution before inspecting source identities
    if arguments.site != configuration_site {
        return Err(backfill_failure("public_station_backfill_site_mismatch", 0));
    }
    let selectors = arguments.source_keys.iter().collect::<BTreeSet<_>>();

    // require the parsed selector contract
    if selectors.len() != arguments.source_keys.len()
        || arguments
            .source_keys
            .iter()
            .any(|value| !stable_key_is_valid(value))
    {
        return Err(backfill_failure(
            "public_station_backfill_selector_invalid",
            0,
        ));
    }
    let mut selected = Vec::new();
    let mut selected_keys = BTreeSet::new();

    // retain checked catalog order for selected sources
    for source in sources {
        let planning = &source.planning;
        let selector_matches = selectors.is_empty() || selectors.contains(&planning.source_key);

        // leave unrequested catalog entries untouched
        if !selector_matches {
            continue;
        }
        validate_planning_source(planning)?;

        // bind run metadata to one stable checked station and unique source key
        if !stable_key_is_valid(&source.station_key) || !selected_keys.insert(&planning.source_key)
        {
            return Err(backfill_failure(
                "public_station_backfill_identity_mismatch",
                0,
            ));
        }
        selected.push(source);
    }

    // reject unknown or unavailable source selectors
    if !selectors.is_empty() && selected.len() != selectors.len() {
        return Err(backfill_failure(
            "public_station_backfill_source_unavailable",
            0,
        ));
    }

    Ok(selected)
}

// execute every exact chunk for one provider-neutral source
fn execute_archive_source<R, P, T>(
    arguments: &PublicStationBackfillArguments,
    source: &PublicStationArchiveBackfillSource,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<PublicStationBackfillSourceReport, TaskFailure>
where
    P: PublicStationArchiveBackfillProvider,
    R: SupplementalPublicStationBackfillRepository,
    T: PublicStationBackfillTimeAdapter,
{
    let planning = &source.planning;
    let from = later_date(planning.history_start.clone(), arguments.from.clone());
    let to = earlier_date(arguments.to.clone(), planning.history_end.clone());
    let chunks = if from.start_ms > to.start_ms {
        Vec::new()
    } else {
        plan_backfill_chunks(
            public_station_adapter_version(planning.adapter),
            &planning.source_id,
            &planning.source_config_fingerprint,
            planning.maximum_chunk_days,
            &from,
            &to,
            time,
        )?
    };
    let mut completed_chunks = 0;
    let mut records = 0;
    let mut skipped_chunks = 0;
    let mut failed_chunk = None;

    // execute chunks in ascending calendar order
    for (chunk_index, chunk) in chunks.iter().enumerate() {
        // satisfy exact-success resume semantics before locking
        if arguments.resume && repository.has_successful_chunk(&chunk.identity)? {
            skipped_chunks += 1;
            continue;
        }

        // keep the executable path safe for direct dry-run callers
        if arguments.dry_run {
            skipped_chunks += 1;
            continue;
        }
        let result = execute_archive_chunk(source, chunk, repository, provider, time)?;

        // stop only this source after its first failed chunk
        if !result.ok {
            failed_chunk = Some(PublicStationBackfillFailedChunk {
                error_code: result.error_code,
                start_date: chunk.start_date.label.clone(),
            });
            break;
        }
        completed_chunks += 1;
        records += result.record_count;

        // retain PurpleAir's post-success delay before every later planned chunk
        if planning.adapter == PublicStationAdapter::Purpleair && chunk_index + 1 < chunks.len() {
            provider.pace_between_chunks(1_000);
        }
    }

    Ok(PublicStationBackfillSourceReport {
        completed_chunks,
        exit_code: u8::from(failed_chunk.is_some()),
        failed_chunk,
        from: from.label,
        planned_chunks: chunks.len(),
        records,
        skipped_chunks,
        source: planning.source_key.clone(),
        to: to.label,
    })
}

/// Execute every selected Ambient source through exact resume and failure boundaries.
pub fn execute_ambient_weather_backfill<R, P, T>(
    arguments: &PublicStationBackfillArguments,
    configuration_site: &str,
    candidates: &[AmbientWeatherBackfillSource],
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<PublicStationBackfillReport, TaskFailure>
where
    P: AmbientWeatherBackfillProvider,
    R: PublicStationBackfillRepository,
    T: PublicStationBackfillTimeAdapter,
{
    validate_arguments(arguments, time)?;
    let selected =
        select_ambient_weather_backfill_sources(arguments, configuration_site, candidates)?;
    let mut reports = Vec::with_capacity(selected.len());

    // isolate each checked Ambient source archive
    for source in selected {
        reports.push(execute_source_backfill(
            arguments, source, repository, provider, time,
        )?);
    }
    let exit_code = u8::from(reports.iter().any(|report| report.exit_code == 1));

    Ok(PublicStationBackfillReport {
        dry_run: arguments.dry_run,
        exit_code,
        resume: arguments.resume,
        site: arguments.site.clone(),
        sources: reports,
    })
}

// execute every exact chunk for one Ambient source
fn execute_source_backfill<R, P, T>(
    arguments: &PublicStationBackfillArguments,
    source: &AmbientWeatherBackfillSource,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<PublicStationBackfillSourceReport, TaskFailure>
where
    P: AmbientWeatherBackfillProvider,
    R: PublicStationBackfillRepository,
    T: PublicStationBackfillTimeAdapter,
{
    validate_admitted_date(&source.history_start)?;

    // validate the optional configured upper bound
    if let Some(history_end) = &source.history_end {
        validate_admitted_date(history_end)?;
    }
    let from = later_date(source.history_start.clone(), arguments.from.clone());
    let to = earlier_date(arguments.to.clone(), source.history_end.clone());
    let chunks = if from.start_ms > to.start_ms {
        Vec::new()
    } else {
        plan_ambient_weather_backfill_chunks(source, &from, &to, time)?
    };
    let mut completed_chunks = 0;
    let mut records = 0;
    let mut skipped_chunks = 0;
    let mut failed_chunk = None;

    // execute every chunk in ascending calendar order
    for chunk in &chunks {
        // satisfy exact-success resume semantics
        if arguments.resume && repository.has_successful_chunk(&chunk.identity)? {
            skipped_chunks += 1;
            continue;
        }

        // prove dry-run has no provider or persistence writes
        if arguments.dry_run {
            skipped_chunks += 1;
            continue;
        }
        let result = execute_chunk(source, chunk, repository, provider, time)?;

        // stop this source after its first failed chunk
        if !result.ok {
            failed_chunk = Some(PublicStationBackfillFailedChunk {
                error_code: result.error_code,
                start_date: chunk.start_date.label.clone(),
            });
            break;
        }

        completed_chunks += 1;
        records += result.record_count;
    }

    Ok(PublicStationBackfillSourceReport {
        completed_chunks,
        exit_code: u8::from(failed_chunk.is_some()),
        failed_chunk,
        from: from.label,
        planned_chunks: chunks.len(),
        records,
        skipped_chunks,
        source: source.source.source_key.clone(),
        to: to.label,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChunkExecution {
    error_code: String,
    ok: bool,
    record_count: usize,
}

// execute one committed Ambient backfill chunk through the shared lifecycle
fn execute_chunk<R, P, T>(
    source: &AmbientWeatherBackfillSource,
    chunk: &PlannedPublicStationBackfillChunk,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<ChunkExecution, TaskFailure>
where
    P: AmbientWeatherBackfillProvider,
    R: PublicStationBackfillRepository,
    T: PublicStationBackfillTimeAdapter,
{
    execute_committed_chunk(
        &source.source.id,
        &source.configuration.station_slug,
        AMBIENT_WEATHER_BACKFILL_ENDPOINT,
        chunk,
        repository,
        time,
        |repository, session, provider_deadline_at_ms, started_id, attempts| {
            let batch = provider.fetch(
                create_range_request(source, &chunk.identity),
                provider_deadline_at_ms,
            )?;
            *attempts = batch.attempts;
            assert_records_within_chunk(&batch.records, &chunk.identity)?;
            let record_count = batch.records.len();

            // preserve Node behavior where resume is chunk-identity based rather than cursor based
            let _provider_cursor = batch.provider_cursor;
            repository.complete_backfill_ingestion(
                session,
                PublicStationBackfillCompletion {
                    attempts: *attempts,
                    identity: chunk.identity.clone(),
                    records: batch.records,
                    response_metadata: batch.response_metadata,
                    run_id: started_id,
                    upstream_response_checksum: batch.upstream_response_checksum,
                },
            )?;

            Ok(record_count)
        },
    )
}

// execute one provider-neutral batch and select its typed atomic completion
fn execute_archive_chunk<R, P, T>(
    source: &PublicStationArchiveBackfillSource,
    chunk: &PlannedPublicStationBackfillChunk,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<ChunkExecution, TaskFailure>
where
    P: PublicStationArchiveBackfillProvider,
    R: SupplementalPublicStationBackfillRepository,
    T: PublicStationBackfillTimeAdapter,
{
    let endpoint = public_station_adapter_endpoint(source.planning.adapter);
    execute_committed_chunk(
        &source.planning.source_id,
        &source.station_key,
        endpoint,
        chunk,
        repository,
        time,
        |repository, session, provider_deadline_at_ms, started_id, attempts| {
            let window = ScheduledWindow {
                end_exclusive_ms: chunk.identity.interval_end_exclusive.start_ms,
                start_ms: chunk.identity.interval_start.start_ms,
            };
            let batch = provider.fetch(source, window, provider_deadline_at_ms)?;

            // dispatch only the batch family admitted by the checked adapter
            match (source.planning.adapter, batch) {
                (
                    PublicStationAdapter::AmbientWeather,
                    PublicStationArchiveBackfillBatch::Ambient(batch),
                ) => {
                    *attempts = batch.attempts;
                    assert_records_within_chunk(&batch.records, &chunk.identity)?;
                    let record_count = batch.records.len();
                    let _provider_cursor = batch.provider_cursor;
                    repository.complete_backfill_ingestion(
                        session,
                        PublicStationBackfillCompletion {
                            attempts: *attempts,
                            identity: chunk.identity.clone(),
                            records: batch.records,
                            response_metadata: batch.response_metadata,
                            run_id: started_id,
                            upstream_response_checksum: batch.upstream_response_checksum,
                        },
                    )?;
                    Ok(record_count)
                }
                (adapter, PublicStationArchiveBackfillBatch::Supplemental(batch))
                    if supplemental_batch_matches(adapter, &batch) =>
                {
                    *attempts = batch.attempts;
                    assert_supplemental_records_within_chunk(&batch.records, &chunk.identity)?;
                    let record_count = batch.records.len();
                    let _provider_cursor = batch.provider_cursor;
                    repository.complete_supplemental_backfill_ingestion(
                        session,
                        PublicStationBackfillCompletion {
                            attempts: *attempts,
                            identity: chunk.identity.clone(),
                            records: batch.records,
                            response_metadata: batch.response_metadata,
                            run_id: started_id,
                            upstream_response_checksum: batch.upstream_response_checksum,
                        },
                    )?;
                    Ok(record_count)
                }
                _ => Err(PublicStationBackfillOperationError::unexpected(
                    "public-station provider returned a batch for another adapter",
                )),
            }
        },
    )
}

// own the shared source lock, committed-run, failure-finalization, and release lifecycle
fn execute_committed_chunk<R, T, F>(
    source_id: &str,
    station_key: &str,
    endpoint: &'static str,
    chunk: &PlannedPublicStationBackfillChunk,
    repository: &mut R,
    time: &mut T,
    execute: F,
) -> Result<ChunkExecution, TaskFailure>
where
    R: PublicStationBackfillRepository,
    T: PublicStationBackfillTimeAdapter,
    F: FnOnce(
        &mut R,
        &mut R::Session,
        i64,
        String,
        &mut u32,
    ) -> Result<usize, PublicStationBackfillOperationError>,
{
    let Some(mut session) = repository.acquire_source_session(source_id)? else {
        return Ok(ChunkExecution {
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
            PublicStationBackfillRunStart {
                adapter_version: chunk.identity.adapter_version,
                chunk_plan_version: chunk.identity.chunk_plan_version,
                deadline_at_ms: deadlines.run_deadline_at_ms,
                endpoint,
                requested_end_exclusive_ms: chunk.identity.interval_end_exclusive.start_ms,
                requested_start_ms: chunk.identity.interval_start.start_ms,
                source_config_fingerprint: chunk.identity.source_config_fingerprint.clone(),
                station_key: station_key.to_owned(),
            },
        )?;
        run_id = Some(started_id.clone());
        execute(
            repository,
            &mut session,
            deadlines.provider_deadline_at_ms,
            started_id,
            &mut attempts,
        )
    })();
    // map the primary committed execution outcome
    let mut result = match execution {
        Ok(record_count) => ChunkExecution {
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

            ChunkExecution {
                error_code: primary.error.code,
                ok: false,
                record_count: 0,
            }
        }
    };

    // retain the primary result when release also fails
    if repository.release_source_session(session).is_err() {
        // surface release failure only after otherwise successful work
        if result.ok {
            result.error_code = "session_release_failed".to_owned();
        }
        result.ok = false;
    }

    Ok(result)
}

// build one Ambient range request from checked configuration
fn create_range_request(
    source: &AmbientWeatherBackfillSource,
    identity: &PublicStationBackfillChunkIdentity,
) -> AmbientWeatherRangeRequest {
    AmbientWeatherRangeRequest {
        device_id: source.configuration.device_id.clone(),
        end_exclusive_ms: identity.interval_end_exclusive.start_ms,
        mac_address: source.configuration.mac_address.clone(),
        model: source.configuration.model.clone(),
        serial: source.configuration.serial.clone(),
        source_id: source.source.id.clone(),
        start_ms: identity.interval_start.start_ms,
        timezone: source.configuration.timezone.clone(),
    }
}

// require normalized records to remain inside their durable chunk
fn assert_records_within_chunk(
    records: &[NormalizedAmbientWeatherRecord],
    identity: &PublicStationBackfillChunkIdentity,
) -> Result<(), PublicStationBackfillOperationError> {
    // reject provider response leakage
    if records.iter().any(|record| {
        record.valid_at_ms < identity.interval_start.start_ms
            || record.valid_at_ms >= identity.interval_end_exclusive.start_ms
    }) {
        return Err(PublicStationBackfillOperationError::unexpected(
            "public-station payload contained records outside the requested chunk",
        ));
    }

    Ok(())
}

// require supplemental normalized records to remain inside their durable chunk
fn assert_supplemental_records_within_chunk(
    records: &[SupplementalPublicStationRecord],
    identity: &PublicStationBackfillChunkIdentity,
) -> Result<(), PublicStationBackfillOperationError> {
    // reject provider response leakage through the shared archive boundary
    if records.iter().any(|record| {
        record.sample.valid_at_ms < identity.interval_start.start_ms
            || record.sample.valid_at_ms >= identity.interval_end_exclusive.start_ms
    }) {
        return Err(PublicStationBackfillOperationError::unexpected(
            "public-station payload contained records outside the requested chunk",
        ));
    }

    Ok(())
}

// match normalized metadata to the checked adapter before persistence
fn supplemental_batch_matches(
    adapter: PublicStationAdapter,
    batch: &SupplementalPublicStationBatch,
) -> bool {
    match adapter {
        PublicStationAdapter::WeatherUnderground => {
            batch.response_metadata.provider == "weather-underground"
        }
        PublicStationAdapter::Netatmo => batch.response_metadata.provider == "netatmo-weathermap",
        PublicStationAdapter::Purpleair => batch.response_metadata.provider == "purpleair",
        PublicStationAdapter::AmbientWeather => false,
    }
}

// preserve the original manual backfill request metadata discriminator
const fn public_station_adapter_endpoint(adapter: PublicStationAdapter) -> &'static str {
    match adapter {
        PublicStationAdapter::AmbientWeather => "ambient-weather",
        PublicStationAdapter::WeatherUnderground => "weather-underground",
        PublicStationAdapter::Netatmo => "netatmo",
        PublicStationAdapter::Purpleair => "purpleair",
    }
}

// validate the admitted one-shot request
fn validate_arguments<T: PublicStationBackfillTimeAdapter>(
    arguments: &PublicStationBackfillArguments,
    time: &mut T,
) -> Result<(), TaskFailure> {
    validate_admitted_date(&arguments.to)?;

    // validate the optional explicit lower bound
    if let Some(from) = &arguments.from {
        validate_admitted_date(from)?;

        // reject reversed explicit ranges
        if from.start_ms > arguments.to.start_ms {
            return Err(backfill_failure("public_station_backfill_range_invalid", 0));
        }
    }
    let today = time.today_utc()?;
    validate_admitted_date(&today)?;

    // reserve the partial current day
    if arguments.to.start_ms >= today.start_ms {
        return Err(backfill_failure("public_station_backfill_range_invalid", 0));
    }

    Ok(())
}

// validate one typed UTC midnight without parsing its label
fn validate_admitted_date(date: &PublicStationBackfillDate) -> Result<(), TaskFailure> {
    // require one bounded admitted label and exact UTC-day boundary
    if date.label.trim().is_empty()
        || date.label.len() > 10
        || date.start_ms <= 0
        || date.start_ms % MILLISECONDS_PER_DAY != 0
    {
        return Err(backfill_failure("public_station_backfill_date_invalid", 0));
    }

    Ok(())
}

// add exact UTC days through the admitted time adapter
fn add_utc_days<T: PublicStationBackfillTimeAdapter>(
    time: &mut T,
    date: &PublicStationBackfillDate,
    days: i64,
) -> Result<PublicStationBackfillDate, TaskFailure> {
    let expected_ms = days
        .checked_mul(MILLISECONDS_PER_DAY)
        .and_then(|duration| date.start_ms.checked_add(duration))
        .ok_or_else(|| backfill_failure("public_station_backfill_date_overflow", 0))?;
    let result = time.add_utc_days(date, days)?;
    validate_admitted_date(&result)?;

    // bind the external date adapter to the exact UTC-day transition
    if result.start_ms != expected_ms {
        return Err(backfill_failure(
            "public_station_backfill_date_transition_invalid",
            0,
        ));
    }

    Ok(result)
}

// validate one exact durable chunk identity
fn validate_chunk_identity(
    identity: &PublicStationBackfillChunkIdentity,
) -> Result<(), TaskFailure> {
    // require bounded source identity fingerprint and forward half-open interval
    if identity.source_id.trim().is_empty()
        || identity.source_id.len() > 128
        || identity.source_config_fingerprint.len() != 64
        || !identity
            .source_config_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || identity.interval_start.start_ms >= identity.interval_end_exclusive.start_ms
    {
        return Err(backfill_failure(
            "public_station_backfill_chunk_identity_invalid",
            0,
        ));
    }

    Ok(())
}

// validate the injected worker deadline plan
fn validate_deadlines(
    deadlines: PublicStationBackfillDeadlines,
) -> Result<(), PublicStationBackfillOperationError> {
    // require provider work and durable finalization after start in order
    if deadlines.provider_deadline_at_ms <= deadlines.started_at_ms
        || deadlines.run_deadline_at_ms <= deadlines.provider_deadline_at_ms
    {
        return Err(PublicStationBackfillOperationError::unexpected(
            "public-station backfill deadlines are invalid",
        ));
    }

    Ok(())
}

// choose the later configured or optional requested lower bound
fn later_date(
    configured: PublicStationBackfillDate,
    requested: Option<PublicStationBackfillDate>,
) -> PublicStationBackfillDate {
    // retain the configured lower bound without an override
    let Some(requested) = requested else {
        return configured;
    };

    // select the later inclusive date
    if configured.start_ms > requested.start_ms {
        configured
    } else {
        requested
    }
}

// choose the earlier requested or optional configured upper bound
fn earlier_date(
    requested: PublicStationBackfillDate,
    configured: Option<PublicStationBackfillDate>,
) -> PublicStationBackfillDate {
    // retain the requested upper bound without a provider cutoff
    let Some(configured) = configured else {
        return requested;
    };

    // select the earlier inclusive date
    if configured.start_ms < requested.start_ms {
        configured
    } else {
        requested
    }
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

// validate one lowercase SHA-256 source fingerprint
fn fingerprint_is_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

// create one bounded public-station backfill failure
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
        AMBIENT_WEATHER_ADAPTER_VERSION, AMBIENT_WEATHER_BACKFILL_ENDPOINT,
        AmbientWeatherBackfillProvider, AmbientWeatherBackfillSource, MILLISECONDS_PER_DAY,
        PublicStationArchiveBackfillBatch, PublicStationArchiveBackfillProvider,
        PublicStationArchiveBackfillSource, PublicStationBackfillArguments,
        PublicStationBackfillChunkIdentity, PublicStationBackfillCompletion,
        PublicStationBackfillDate, PublicStationBackfillDeadlines,
        PublicStationBackfillErrorClassification, PublicStationBackfillOperationError,
        PublicStationBackfillPlanningSource, PublicStationBackfillProviderFailure,
        PublicStationBackfillRepository, PublicStationBackfillRunStart,
        PublicStationBackfillTimeAdapter, SupplementalPublicStationBackfillRepository,
        execute_ambient_weather_backfill, execute_public_station_archive_backfill,
        select_ambient_weather_backfill_sources,
    };
    use crate::{
        AmbientWeatherCapability, AmbientWeatherDeviceIdentity, AmbientWeatherObservation,
        AmbientWeatherPayload, AmbientWeatherProviderCursor, AmbientWeatherRangeRequest,
        AmbientWeatherResponseMetadata, AmbientWeatherRuntimeConfiguration,
        AmbientWeatherScheduledSource, AmbientWeatherSourceKind, NormalizedAmbientWeatherRecord,
        ProviderBatch, ScheduledWindow, SupplementalPublicStationBatch,
        SupplementalPublicStationRecord, SupplementalPublicStationResponseMetadata, TaskFailure,
        normalize_ambient_weather_range,
    };
    use weather_contracts::PublicStationAdapter;

    // construct one known admitted UTC date
    fn date(label: &str, start_ms: i64) -> PublicStationBackfillDate {
        PublicStationBackfillDate::admitted(label, start_ms)
    }

    // build one exact checked historical Ambient source
    fn ambient_source(
        source_key: &str,
        source_id: &str,
        station_slug: &str,
        device_id: &str,
        mac_address: &str,
        fingerprint_byte: char,
    ) -> AmbientWeatherBackfillSource {
        let fingerprint = fingerprint_byte.to_string().repeat(64);
        let material_identity = format!("material-{source_key}");
        let source = AmbientWeatherScheduledSource {
            active: true,
            cadence_seconds: 3_600,
            id: source_id.to_owned(),
            material_config_identity: material_identity.clone(),
            provider_key: "ambient-weather-network".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_config_fingerprint: fingerprint.clone(),
            source_key: source_key.to_owned(),
            source_kind: AmbientWeatherSourceKind::PhysicalSensor,
            station_slug: station_slug.to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        };
        let configuration = AmbientWeatherRuntimeConfiguration {
            adapter_contract_version: AMBIENT_WEATHER_ADAPTER_VERSION,
            capabilities: BTreeSet::from([
                AmbientWeatherCapability::Current,
                AmbientWeatherCapability::Historical,
            ]),
            device_id: device_id.to_owned(),
            mac_address: mac_address.to_owned(),
            material_config_identity: material_identity,
            maximum_chunk_days: 7,
            model: Some("AMBWeatherV4.3.6".to_owned()),
            provider_key: source.provider_key.clone(),
            serial: Some(device_id.to_owned()),
            site_slug: source.site_slug.clone(),
            source_active: true,
            source_config_fingerprint: fingerprint,
            source_key: source.source_key.clone(),
            source_kind: source.source_kind,
            station_slug: source.station_slug.clone(),
            timezone: source.timezone.clone(),
        };

        AmbientWeatherBackfillSource {
            configuration,
            history_end: Some(date("2026-08-20", 1_787_184_000_000)),
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
        deadlines: Vec<PublicStationBackfillDeadlines>,
        today: PublicStationBackfillDate,
    }

    impl FakeTime {
        // construct every admitted transition used by the fixture
        fn fixture() -> Self {
            let dates = [
                (1_785_542_400_000, "2026-08-01"),
                (1_785_628_800_000, "2026-08-02"),
                (1_786_060_800_000, "2026-08-07"),
                (1_786_147_200_000, "2026-08-08"),
                (1_786_665_600_000, "2026-08-14"),
                (1_786_752_000_000, "2026-08-15"),
                (1_787_184_000_000, "2026-08-20"),
                (1_787_270_400_000, "2026-08-21"),
                (1_787_356_800_000, "2026-08-22"),
                (1_794_009_600_000, "2026-11-07"),
            ]
            .into_iter()
            .map(|(epoch, label)| (epoch, label.to_owned()))
            .collect();

            Self {
                dates,
                deadlines: vec![
                    PublicStationBackfillDeadlines {
                        provider_deadline_at_ms: 1_794_009_690_000,
                        run_deadline_at_ms: 1_794_009_720_000,
                        started_at_ms: 1_794_009_600_000,
                    },
                    PublicStationBackfillDeadlines {
                        provider_deadline_at_ms: 1_794_009_700_000,
                        run_deadline_at_ms: 1_794_009_730_000,
                        started_at_ms: 1_794_009_610_000,
                    },
                ],
                today: date("2026-11-07", 1_794_009_600_000),
            }
        }
    }

    impl PublicStationBackfillTimeAdapter for FakeTime {
        // return the fixed current UTC day
        fn today_utc(&mut self) -> Result<PublicStationBackfillDate, TaskFailure> {
            Ok(self.today.clone())
        }

        // resolve only pre-admitted UTC date transitions
        fn add_utc_days(
            &mut self,
            date: &PublicStationBackfillDate,
            days: i64,
        ) -> Result<PublicStationBackfillDate, TaskFailure> {
            let start_ms = date
                .start_ms()
                .checked_add(days * MILLISECONDS_PER_DAY)
                .ok_or_else(|| failure("date_overflow", 0))?;
            let label = self
                .dates
                .get(&start_ms)
                .ok_or_else(|| failure("date_not_admitted", 0))?;
            Ok(PublicStationBackfillDate::admitted(label.clone(), start_ms))
        }

        // return the next exact pre-admitted deadline plan
        fn deadlines(
            &mut self,
        ) -> Result<PublicStationBackfillDeadlines, PublicStationBackfillOperationError> {
            // retain declared execution order
            if self.deadlines.is_empty() {
                return Err(PublicStationBackfillOperationError::unexpected(
                    "deadline fixture missing",
                ));
            }
            Ok(self.deadlines.remove(0))
        }
    }

    #[derive(Debug, Default)]
    struct FakeRepository {
        completions: Vec<PublicStationBackfillCompletion>,
        events: Vec<String>,
        failures: Vec<(
            String,
            PublicStationBackfillChunkIdentity,
            PublicStationBackfillProviderFailure,
        )>,
        resume_start_ms: Option<i64>,
        starts: Vec<PublicStationBackfillRunStart>,
    }

    impl PublicStationBackfillRepository for FakeRepository {
        type Session = String;

        // match one preexisting exact successful chunk
        fn has_successful_chunk(
            &mut self,
            identity: &PublicStationBackfillChunkIdentity,
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
        ) -> Result<(), PublicStationBackfillOperationError> {
            self.events.push(format!("abandon:{now_ms}"));
            Ok(())
        }

        // commit one running chunk
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            start: PublicStationBackfillRunStart,
        ) -> Result<String, PublicStationBackfillOperationError> {
            let run_id = format!("run-{}", self.starts.len() + 1);
            self.events.push(format!("start:{run_id}"));
            self.starts.push(start);
            Ok(run_id)
        }

        // retain one successful chunk atomically
        fn complete_backfill_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: PublicStationBackfillCompletion,
        ) -> Result<(), PublicStationBackfillOperationError> {
            self.events.push(format!("complete:{}", completion.run_id));
            self.completions.push(completion);
            Ok(())
        }

        // retain one failed committed chunk
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            run_id: &str,
            identity: &PublicStationBackfillChunkIdentity,
            failure: &PublicStationBackfillProviderFailure,
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

    impl SupplementalPublicStationBackfillRepository for FakeRepository {
        // retain one successful supplemental chunk through the same fixture event stream
        fn complete_supplemental_backfill_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: PublicStationBackfillCompletion<
                SupplementalPublicStationRecord,
                SupplementalPublicStationResponseMetadata,
            >,
        ) -> Result<(), PublicStationBackfillOperationError> {
            self.events.push(format!("complete:{}", completion.run_id));
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct SupplementalArchiveProvider {
        paced: Vec<u64>,
        windows: Vec<ScheduledWindow>,
    }

    impl PublicStationArchiveBackfillProvider for SupplementalArchiveProvider {
        // return one empty normalized PurpleAir result for every requested range
        fn fetch(
            &mut self,
            _source: &PublicStationArchiveBackfillSource,
            window: ScheduledWindow,
            _deadline_at_ms: i64,
        ) -> Result<PublicStationArchiveBackfillBatch, PublicStationBackfillOperationError>
        {
            self.windows.push(window);
            Ok(PublicStationArchiveBackfillBatch::Supplemental(
                SupplementalPublicStationBatch {
                    attempts: 1,
                    provider_cursor: None,
                    records: Vec::new(),
                    response_metadata: SupplementalPublicStationResponseMetadata {
                        measurement_request_count: None,
                        normalized_record_count: 0,
                        provider: "purpleair",
                        raw_observation_count: Some(0),
                        resolution_seconds: Some(120),
                    },
                    upstream_response_checksum: "d".repeat(64),
                },
            ))
        }

        // record only the explicit provider pacing effect
        fn pace_between_chunks(&mut self, delay_ms: u64) {
            self.paced.push(delay_ms);
        }
    }

    #[derive(Debug, Default)]
    struct NormalizingProvider {
        requests: Vec<(AmbientWeatherRangeRequest, i64)>,
    }

    impl AmbientWeatherBackfillProvider for NormalizingProvider {
        // normalize the first dispatched request and fail the next
        fn fetch(
            &mut self,
            request: AmbientWeatherRangeRequest,
            deadline_at_ms: i64,
        ) -> Result<
            ProviderBatch<
                NormalizedAmbientWeatherRecord,
                AmbientWeatherResponseMetadata,
                Option<AmbientWeatherProviderCursor>,
            >,
            PublicStationBackfillOperationError,
        > {
            self.requests.push((request.clone(), deadline_at_ms));

            // preserve the fixture's first-failure stop boundary
            if self.requests.len() == 2 {
                return Err(PublicStationBackfillOperationError::provider(
                    2,
                    PublicStationBackfillErrorClassification::Retryable,
                    "provider_server_error",
                    "fixture failure",
                ));
            }
            let normalized = normalize_ambient_weather_range(
                &AmbientWeatherPayload {
                    observations: vec![AmbientWeatherObservation {
                        apparent_temperature_f: Some(54.0),
                        date_utc_ms: request.start_ms + 3_600_000,
                        device_identity: AmbientWeatherDeviceIdentity::Value(
                            request.device_id.clone(),
                        ),
                        hourly_rain_in: Some(0.1),
                        pressure_in_hg: Some(30.0),
                        relative_humidity_percent: Some(90.0),
                        solar_radiation_wm2: Some(100.0),
                        temperature_f: Some(53.6),
                        uv_index: Some(1.0),
                        wind_direction_degrees: Some(360.0),
                        wind_gust_mph: Some(10.0),
                        wind_speed_mph: Some(5.0),
                    }],
                },
                &request,
                deadline_at_ms - 1,
                1,
            )
            .map_err(|failure| {
                PublicStationBackfillOperationError::provider(
                    failure.attempts.max(1),
                    PublicStationBackfillErrorClassification::InvalidPayload,
                    failure.code,
                    "Ambient Weather payload is invalid",
                )
            })?;

            Ok(ProviderBatch {
                attempts: normalized.attempts,
                provider_cursor: normalized.provider_cursor,
                records: normalized.records,
                response_metadata: normalized.response_metadata,
                upstream_response_checksum: "c".repeat(64),
            })
        }
    }

    // create one bounded test failure
    fn failure(code: &str, attempts: u32) -> TaskFailure {
        TaskFailure {
            attempts,
            code: code.to_owned(),
        }
    }

    // preserve exact catalog order even when selectors use another order
    #[test]
    fn ambient_backfill_selection_preserves_checked_catalog_order() {
        let first = ambient_source(
            "ambient-merlin-observations-v1",
            "source-merlin",
            "ambient-merlin",
            "5fe7d16a25083b000171f8e5",
            "F0:08:D1:07:B3:9E",
            'a',
        );
        let second = ambient_source(
            "ambient-lapis-observations-v1",
            "source-lapis",
            "ambient-lapis",
            "6fe7d16a25083b000171f8e6",
            "A0:08:D1:07:B3:9F",
            'b',
        );
        let arguments = PublicStationBackfillArguments {
            dry_run: true,
            from: None,
            resume: false,
            site: "ballydidean".to_owned(),
            source_keys: vec![
                second.source.source_key.clone(),
                first.source.source_key.clone(),
            ],
            to: date("2026-08-20", 1_787_184_000_000),
        };

        let candidates = [first, second];
        let selected =
            select_ambient_weather_backfill_sources(&arguments, "ballydidean", &candidates)
                .expect("checked historical sources should select");

        assert_eq!(selected.len(), 2);
        assert_eq!(
            selected[0].source.source_key,
            "ambient-merlin-observations-v1"
        );
        assert_eq!(
            selected[1].source.source_key,
            "ambient-lapis-observations-v1"
        );
    }

    // match Node range resume normalization completion and first-failure boundaries
    #[test]
    fn ambient_backfill_preserves_range_resume_order_and_atomic_records() {
        let source = ambient_source(
            "ambient-merlin-observations-v1",
            "source-merlin",
            "ambient-merlin",
            "5fe7d16a25083b000171f8e5",
            "F0:08:D1:07:B3:9E",
            'a',
        );
        let arguments = PublicStationBackfillArguments {
            dry_run: false,
            from: None,
            resume: true,
            site: "ballydidean".to_owned(),
            source_keys: vec![source.source.source_key.clone()],
            to: date("2026-08-22", 1_787_356_800_000),
        };
        let mut repository = FakeRepository {
            resume_start_ms: Some(source.history_start.start_ms()),
            ..FakeRepository::default()
        };
        let mut provider = NormalizingProvider::default();
        let mut time = FakeTime::fixture();

        let report = execute_ambient_weather_backfill(
            &arguments,
            "ballydidean",
            &[source],
            &mut repository,
            &mut provider,
            &mut time,
        )
        .expect("typed Ambient backfill should report its provider failure");

        assert_eq!(report.exit_code, 1);
        assert_eq!(report.sources.len(), 1);
        let source_report = &report.sources[0];
        assert_eq!(source_report.from, "2026-08-01");
        assert_eq!(source_report.to, "2026-08-20");
        assert_eq!(source_report.planned_chunks, 3);
        assert_eq!(source_report.skipped_chunks, 1);
        assert_eq!(source_report.completed_chunks, 1);
        assert_eq!(source_report.records, 1);
        assert_eq!(
            source_report.failed_chunk,
            Some(super::PublicStationBackfillFailedChunk {
                error_code: "provider_server_error".to_owned(),
                start_date: "2026-08-15".to_owned(),
            }),
        );
        assert_eq!(provider.requests.len(), 2);
        assert_eq!(provider.requests[0].0.start_ms, 1_786_147_200_000);
        assert_eq!(provider.requests[0].0.end_exclusive_ms, 1_786_752_000_000);
        assert_eq!(provider.requests[1].0.start_ms, 1_786_752_000_000);
        assert_eq!(provider.requests[1].0.end_exclusive_ms, 1_787_270_400_000);
        assert_eq!(provider.requests[0].0.source_id, "source-merlin");
        assert_eq!(provider.requests[0].0.timezone, "America/Los_Angeles");
        assert_eq!(provider.requests[0].1, 1_794_009_690_000);
        assert_eq!(repository.starts.len(), 2);
        assert_eq!(
            repository.starts[0].endpoint,
            AMBIENT_WEATHER_BACKFILL_ENDPOINT
        );
        assert_eq!(
            repository.starts[0].adapter_version,
            AMBIENT_WEATHER_ADAPTER_VERSION
        );
        assert_eq!(repository.completions.len(), 1);
        assert_eq!(repository.completions[0].records.len(), 1);
        assert_eq!(
            repository.completions[0].records[0].valid_at_ms,
            1_786_150_800_000
        );
        assert_eq!(
            repository.completions[0].response_metadata.provider,
            "ambient-weather-network"
        );
        assert_eq!(
            repository.completions[0].upstream_response_checksum,
            "c".repeat(64)
        );
        assert_eq!(repository.failures.len(), 1);
        assert_eq!(repository.failures[0].2.attempts, 2);
        assert_eq!(
            repository.failures[0].2.error.classification,
            PublicStationBackfillErrorClassification::Retryable,
        );
        assert_eq!(repository.failures[0].2.error.code, "provider_server_error");
        assert_eq!(repository.failures[0].2.error.message, "fixture failure");
        assert_eq!(
            repository.events,
            [
                "resume:2026-08-01",
                "resume:2026-08-08",
                "lock:source-merlin",
                "abandon:1794009600000",
                "start:run-1",
                "complete:run-1",
                "release:source-merlin",
                "resume:2026-08-15",
                "lock:source-merlin",
                "abandon:1794009610000",
                "start:run-2",
                "fail:run-2",
                "release:source-merlin",
            ]
        );
    }

    // preserve PurpleAir pacing after success even when the next planned chunk resumes
    #[test]
    fn provider_neutral_backfill_paces_purpleair_by_planned_chunk_position() {
        let source = PublicStationArchiveBackfillSource {
            planning: PublicStationBackfillPlanningSource {
                adapter: PublicStationAdapter::Purpleair,
                source_id: "source-purpleair".to_owned(),
                source_key: "purpleair-history-v1".to_owned(),
                source_config_fingerprint: "e".repeat(64),
                history_start: date("2026-08-01", 1_785_542_400_000),
                history_end: Some(date("2026-08-20", 1_787_184_000_000)),
                maximum_chunk_days: 7,
            },
            station_key: "purpleair-station".to_owned(),
        };
        let arguments = PublicStationBackfillArguments {
            dry_run: false,
            from: None,
            resume: true,
            site: "ballydidean".to_owned(),
            source_keys: vec![source.planning.source_key.clone()],
            to: date("2026-08-20", 1_787_184_000_000),
        };
        let mut repository = FakeRepository {
            resume_start_ms: Some(1_786_147_200_000),
            ..FakeRepository::default()
        };
        let mut provider = SupplementalArchiveProvider::default();
        let mut time = FakeTime::fixture();

        let report = execute_public_station_archive_backfill(
            &arguments,
            "ballydidean",
            &[source],
            &mut repository,
            &mut provider,
            &mut time,
        )
        .expect("typed supplemental archive should complete");

        assert_eq!(report.exit_code, 0);
        assert_eq!(report.sources[0].planned_chunks, 3);
        assert_eq!(report.sources[0].completed_chunks, 2);
        assert_eq!(report.sources[0].skipped_chunks, 1);
        assert_eq!(provider.windows.len(), 2);
        assert_eq!(provider.paced, [1_000]);
        assert_eq!(repository.starts.len(), 2);
        assert!(
            repository
                .starts
                .iter()
                .all(|start| start.endpoint == "purpleair")
        );
    }
}
