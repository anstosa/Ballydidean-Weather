//! Typed Open-Meteo Previous Runs forecast-anchor normalization and backfill execution.

use std::collections::BTreeSet;

use weather_contracts::{
    BackfillChunkIdentity, BackfillChunkReport, BackfillChunkReportStatusEnum, BackfillReport,
    BackfillReportExitCodeEnum, CanonicalWeatherMetrics, FixedForecastLeadHours, JsonValue,
    NormalizedForecastAnchorRecord, NormalizedForecastAnchorRecordContractVersionLiteral,
    NormalizedForecastAnchorRecordDatasetLiteral, NormalizedForecastAnchorRecordSourceKindLiteral,
    NormalizedForecastAnchorRecordUpstreamModelLiteral, Nullable, WeatherRecordMetadata,
};

use super::open_meteo_archive_backfill::{
    OpenMeteoArchiveCleanupFailure, append_release_diagnostic,
};
use super::public_station_backfill::{PublicStationBackfillDate, PublicStationBackfillDeadlines};
use super::tide_backfill::{
    TideBackfillOperationError, TideBackfillProviderFailure, as_backfill_provider_failure,
};
use super::{ProviderBatch, TaskFailure};

pub const FORECAST_ANCHOR_ADAPTER_VERSION: &str = "open-meteo-previous-runs/v1";
pub const FORECAST_ANCHOR_CHUNK_PLAN_VERSION: &str = "open-meteo-previous-runs/v1";
pub const FORECAST_ANCHOR_CONTRACT_VERSION: &str = "previous-runs-hourly/v1";
pub const FORECAST_ANCHOR_CONTRACT_EPOCH: &str = "open-meteo-previous-runs-best-match/2026-09";
pub const FORECAST_ANCHOR_ENDPOINT: &str = "previous-runs/forecast";
pub const FORECAST_ANCHOR_MODEL: &str = "best_match";
pub const FORECAST_ANCHOR_DATASET: &str = "previous_runs";
pub const FORECAST_ANCHOR_MAXIMUM_CHUNK_DAYS: u16 = 14;

const MILLISECONDS_PER_HOUR: i64 = 3_600_000;
const MILLISECONDS_PER_DAY: i64 = 86_400_000;
const FORECAST_ANCHOR_LEAD_HOURS: [u16; 7] = [24, 48, 72, 96, 120, 144, 168];
const FORECAST_ANCHOR_VARIABLES: [&str; 9] = [
    "temperature",
    "apparent_temperature",
    "relative_humidity",
    "precipitation",
    "cloud_cover",
    "wind_speed",
    "wind_gust",
    "wind_direction",
    "surface_pressure",
];

pub type ForecastAnchorBackfillDate = PublicStationBackfillDate;
pub type ForecastAnchorBackfillDeadlines = PublicStationBackfillDeadlines;
pub type ForecastAnchorBackfillOperationError = TideBackfillOperationError;
pub type ForecastAnchorBackfillProviderFailure = TideBackfillProviderFailure;

/// Time operations admitted by the caller's UTC calendar and deadline implementation.
pub trait ForecastAnchorBackfillTimeAdapter {
    /// Return the current UTC date used to exclude the partial day.
    fn today_utc(&mut self) -> Result<ForecastAnchorBackfillDate, TaskFailure>;

    /// Add exact UTC calendar days without implementing date parsing locally.
    fn add_utc_days(
        &mut self,
        date: &ForecastAnchorBackfillDate,
        days: i64,
    ) -> Result<ForecastAnchorBackfillDate, TaskFailure>;

    /// Return the admitted provider and durable-run deadlines for one chunk.
    fn deadlines(
        &mut self,
    ) -> Result<ForecastAnchorBackfillDeadlines, ForecastAnchorBackfillOperationError>;
}

/// One typed CLI-equivalent Previous Runs backfill request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastAnchorBackfillArguments {
    pub chunk_days: u16,
    pub dry_run: bool,
    pub from: ForecastAnchorBackfillDate,
    pub resume: bool,
    pub site: String,
    pub source: String,
    pub to: ForecastAnchorBackfillDate,
}

/// Frozen adapter material admitted from checked configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastAnchorAdapterConfiguration {
    pub contract_epoch: String,
    pub contract_version: String,
    pub lead_hours: Vec<u16>,
    pub maximum_chunk_days: u16,
    pub model: String,
    pub variables: Vec<String>,
}

impl ForecastAnchorAdapterConfiguration {
    /// Match the fixed original Previous Runs controls in their declared order.
    pub fn matches_retained_contract(&self) -> bool {
        self.contract_epoch == FORECAST_ANCHOR_CONTRACT_EPOCH
            && self.contract_version == FORECAST_ANCHOR_CONTRACT_VERSION
            && self.lead_hours == FORECAST_ANCHOR_LEAD_HOURS
            && self.maximum_chunk_days == FORECAST_ANCHOR_MAXIMUM_CHUNK_DAYS
            && self.model == FORECAST_ANCHOR_MODEL
            && self.variables == FORECAST_ANCHOR_VARIABLES
    }
}

/// Checked configured source fields used by historical admission.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorRuntimeConfiguration {
    pub active: bool,
    pub adapter: ForecastAnchorAdapterConfiguration,
    pub cadence_seconds: Option<u64>,
    pub capabilities: BTreeSet<ForecastAnchorCapability>,
    pub fingerprint: String,
    pub latitude: f64,
    pub longitude: f64,
    pub material_config_identity: String,
    pub provider_key: String,
    pub site_slug: String,
    pub source_key: String,
    pub source_kind: ForecastAnchorSourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// Only the configured historical capability is valid for Previous Runs.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ForecastAnchorCapability {
    Historical,
    Current,
    Forecast,
    Stream,
}

/// Exact configured and retained source kind for the fixed-anchor route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastAnchorSourceKind {
    Forecast,
    Reanalysis,
}

/// Active storage identity resolved before resume, lock, or provider operations.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorBackfillSource {
    pub active: bool,
    pub id: String,
    pub key: String,
    pub latitude: f64,
    pub longitude: f64,
    pub material_config_identity: String,
    pub provider_key: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
    pub source_kind: ForecastAnchorSourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// Exact durable identity for one inclusive-date chunk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastAnchorBackfillChunkIdentity {
    pub adapter_version: &'static str,
    pub chunk_plan_version: &'static str,
    pub interval_end_exclusive: ForecastAnchorBackfillDate,
    pub interval_start: ForecastAnchorBackfillDate,
    pub requested_from_date: String,
    pub requested_to_date: String,
    pub source_config_fingerprint: String,
    pub source_id: String,
}

/// One planned inclusive UTC request and its half-open durable identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedForecastAnchorBackfillChunk {
    pub end_date: ForecastAnchorBackfillDate,
    pub identity: ForecastAnchorBackfillChunkIdentity,
    pub start_date: ForecastAnchorBackfillDate,
}

/// Metadata committed before the provider operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastAnchorBackfillRunStart {
    pub adapter_version: &'static str,
    pub chunk_plan_version: &'static str,
    pub deadline_at_ms: i64,
    pub end_date: String,
    pub endpoint: &'static str,
    pub mode: ForecastAnchorIngestionMode,
    pub requested_end_exclusive_ms: i64,
    pub requested_start_ms: i64,
    pub source_config_fingerprint: String,
    pub start_date: String,
}

/// The Previous Runs path always commits one backfill-mode run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastAnchorIngestionMode {
    Backfill,
}

/// One positional location in a provider request.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub source_config_fingerprint: String,
    pub source_id: String,
}

/// One exact Previous Runs provider request.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorProviderRequest {
    pub contract_epoch: &'static str,
    pub deadline_at_ms: i64,
    pub end_date: String,
    pub end_exclusive_ms: i64,
    pub locations: Vec<ForecastAnchorLocation>,
    pub start_date: String,
    pub start_ms: i64,
}

/// Parsed provider envelope admitted by the transport boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorEnvelope {
    pub elevation_m: Option<f64>,
    pub grid_cell: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: ForecastAnchorTimezone,
    pub utc_offset_seconds: i32,
}

/// Accepted zero-offset labels emitted by Open-Meteo.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastAnchorTimezone {
    Utc,
    Gmt,
}

/// Nine typed metric cells for one fixed lead and valid hour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForecastAnchorMetricCells {
    pub apparent_temperature_c: Option<f64>,
    pub cloud_cover_percent: Option<f64>,
    pub precipitation_mm: Option<f64>,
    pub pressure_hpa: Option<f64>,
    pub relative_humidity_percent: Option<f64>,
    pub temperature_c: Option<f64>,
    pub wind_direction_degrees: Option<f64>,
    pub wind_gust_mps: Option<f64>,
    pub wind_speed_mps: Option<f64>,
}

impl ForecastAnchorMetricCells {
    /// Report whether the provider returned null for all nine requested cells.
    fn is_empty(self) -> bool {
        self.apparent_temperature_c.is_none()
            && self.cloud_cover_percent.is_none()
            && self.precipitation_mm.is_none()
            && self.pressure_hpa.is_none()
            && self.relative_humidity_percent.is_none()
            && self.temperature_c.is_none()
            && self.wind_direction_degrees.is_none()
            && self.wind_gust_mps.is_none()
            && self.wind_speed_mps.is_none()
    }
}

/// One admitted provider hour with all seven fixed lead cells in lead order.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorParsedHour {
    pub anchors: [ForecastAnchorMetricCells; 7],
    pub valid_at: String,
    pub valid_at_ms: i64,
}

/// One typed positional Previous Runs response root.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorParsedLocation {
    pub envelope: ForecastAnchorEnvelope,
    pub generation_ms: Option<f64>,
    pub hours: Vec<ForecastAnchorParsedHour>,
}

/// Counts retained for each fixed lead in response evidence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ForecastAnchorLeadCounts {
    pub null: usize,
    pub populated: usize,
    pub requested: usize,
}

/// Typed bounded response evidence committed with actual records.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorResponseMetadata {
    pub anchor_counts_by_lead: [(u16, ForecastAnchorLeadCounts); 7],
    pub generation_ms: f64,
    pub http_status: u16,
    pub location_response_count: usize,
    pub null_anchor_count: usize,
    pub populated_anchor_count: usize,
    pub requested_anchor_count: usize,
    pub requested_location_count: usize,
    pub weighted_api_call_cost: f64,
}

impl ForecastAnchorResponseMetadata {
    /// Project the exact retained provider metadata before generic serialization.
    pub fn to_json_value(&self) -> JsonValue {
        let counts = JsonValue::Object(
            self.anchor_counts_by_lead
                .iter()
                .map(
                    // keep the original numeric keys and the named counters for each lead
                    |(lead, counts)| {
                        (
                            utf16(&lead.to_string()),
                            metadata_object([
                                ("null", JsonValue::Number(counts.null as f64)),
                                ("populated", JsonValue::Number(counts.populated as f64)),
                                ("requested", JsonValue::Number(counts.requested as f64)),
                            ]),
                        )
                    },
                )
                .collect(),
        );
        metadata_object([
            ("anchor_counts_by_lead", counts),
            (
                "data_modification",
                JsonValue::String(utf16("normalized_to_canonical_fixed_lead_anchors")),
            ),
            ("generation_ms", JsonValue::Number(self.generation_ms)),
            (
                "http_status",
                JsonValue::Number(f64::from(self.http_status)),
            ),
            (
                "location_response_count",
                JsonValue::Number(self.location_response_count as f64),
            ),
            (
                "null_anchor_count",
                JsonValue::Number(self.null_anchor_count as f64),
            ),
            (
                "populated_anchor_count",
                JsonValue::Number(self.populated_anchor_count as f64),
            ),
            (
                "requested_anchor_count",
                JsonValue::Number(self.requested_anchor_count as f64),
            ),
            (
                "requested_location_count",
                JsonValue::Number(self.requested_location_count as f64),
            ),
            (
                "selected_field_count",
                JsonValue::Number(
                    (FORECAST_ANCHOR_VARIABLES.len() * FORECAST_ANCHOR_LEAD_HOURS.len()) as f64,
                ),
            ),
            ("upstream_http_response_count", JsonValue::Number(1.0)),
            (
                "weighted_api_call_cost",
                JsonValue::Number(self.weighted_api_call_cost),
            ),
            (
                "wind_gust_interval",
                JsonValue::String(utf16("preceding_hour_maximum")),
            ),
        ])
    }
}

// keep response metadata in its original field order without owning a codec
fn metadata_object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (utf16(key), value))
            .collect(),
    )
}

/// One already-parsed response and admitted persistence evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorAttemptPayload {
    pub attempts: u32,
    pub checksum: String,
    pub http_status: u16,
    pub locations: Vec<ForecastAnchorParsedLocation>,
    pub received_at: String,
}

/// One successful atomic forecast-anchor completion.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastAnchorBackfillCompletion {
    pub attempts: u32,
    pub identity: ForecastAnchorBackfillChunkIdentity,
    pub records: Vec<NormalizedForecastAnchorRecord>,
    pub response_metadata: ForecastAnchorResponseMetadata,
    pub run_id: String,
    pub upstream_response_checksum: String,
}

/// Persistence required by the retained forecast-anchor loop.
pub trait ForecastAnchorBackfillRepository {
    type Session;

    /// Report whether this exact durable chunk already succeeded.
    fn has_successful_chunk(
        &mut self,
        identity: &ForecastAnchorBackfillChunkIdentity,
    ) -> Result<bool, TaskFailure>;

    /// Acquire one source-scoped transaction session.
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure>;

    /// Abandon expired runs before committing the new running row.
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        started_at_ms: i64,
    ) -> Result<(), ForecastAnchorBackfillOperationError>;

    /// Commit one running backfill row before provider I/O.
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: ForecastAnchorBackfillRunStart,
    ) -> Result<String, ForecastAnchorBackfillOperationError>;

    /// Commit records, response evidence, and successful identity atomically.
    fn complete_forecast_anchor_backfill_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: ForecastAnchorBackfillCompletion,
    ) -> Result<(), ForecastAnchorBackfillOperationError>;

    /// Finalize a committed failed run with the retained primary error.
    fn fail_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        run_id: &str,
        identity: &ForecastAnchorBackfillChunkIdentity,
        failure: &ForecastAnchorBackfillProviderFailure,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure>;

    /// Release one source-scoped session after primary work finishes.
    fn release_source_session(
        &mut self,
        session: Self::Session,
    ) -> Result<(), OpenMeteoArchiveCleanupFailure>;
}

/// Provider operation returning parsed typed input rather than implementing transport here.
pub trait ForecastAnchorBackfillProvider {
    /// Fetch and admit one Previous Runs response.
    fn fetch(
        &mut self,
        request: ForecastAnchorProviderRequest,
    ) -> Result<ForecastAnchorAttemptPayload, ForecastAnchorBackfillOperationError>;
}

/// Per-chunk disposition retained in the backfill report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastAnchorBackfillChunkStatus {
    Abandoned,
    Completed,
    Failed,
    Planned,
    Remaining,
    Skipped,
}

/// One exact chunk report including bounded cleanup diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastAnchorBackfillChunkReport {
    pub error_code: Option<String>,
    pub identity: ForecastAnchorBackfillChunkIdentity,
    pub secondary_error: Option<String>,
    pub status: ForecastAnchorBackfillChunkStatus,
}

/// Complete one-shot Previous Runs report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastAnchorBackfillReport {
    pub chunks: Vec<ForecastAnchorBackfillChunkReport>,
    pub dry_run: bool,
    pub exit_code: u8,
    pub from: String,
    pub resume: bool,
    pub site: String,
    pub source: String,
    pub to: String,
}

impl ForecastAnchorBackfillReport {
    /// Convert the retained Previous Runs result to the original generated machine report.
    pub fn into_backfill_report(self) -> BackfillReport {
        // map the internal binary process result to the generated numeric literal
        let exit_code = match self.exit_code {
            0 => BackfillReportExitCodeEnum::Value0,
            _ => BackfillReportExitCodeEnum::Value1,
        };
        BackfillReport {
            chunks: self
                .chunks
                .into_iter()
                .map(ForecastAnchorBackfillChunkReport::into_backfill_chunk_report)
                .collect(),
            dryRun: self.dry_run,
            exitCode: exit_code,
            from: self.from,
            resume: self.resume,
            site: self.site,
            source: self.source,
            to: self.to,
        }
    }
}

impl ForecastAnchorBackfillChunkReport {
    /// Convert one admitted UTC-date identity and its original optional diagnostic.
    fn into_backfill_chunk_report(self) -> BackfillChunkReport {
        BackfillChunkReport {
            errorCode: nullable(self.error_code),
            identity: BackfillChunkIdentity {
                adapterVersion: self.identity.adapter_version.to_owned(),
                chunkPlanVersion: self.identity.chunk_plan_version.to_owned(),
                intervalEndExclusive: utc_midnight(self.identity.interval_end_exclusive.label()),
                intervalStart: utc_midnight(self.identity.interval_start.label()),
                requestedFromDate: self.identity.requested_from_date,
                requestedToDate: self.identity.requested_to_date,
                sourceConfigFingerprint: self.identity.source_config_fingerprint,
                sourceId: self.identity.source_id,
            },
            secondaryError: nullable(self.secondary_error),
            // preserve every original per-chunk disposition without collapsing states
            status: match self.status {
                ForecastAnchorBackfillChunkStatus::Abandoned => {
                    BackfillChunkReportStatusEnum::Abandoned
                }
                ForecastAnchorBackfillChunkStatus::Completed => {
                    BackfillChunkReportStatusEnum::Completed
                }
                ForecastAnchorBackfillChunkStatus::Failed => BackfillChunkReportStatusEnum::Failed,
                ForecastAnchorBackfillChunkStatus::Planned => {
                    BackfillChunkReportStatusEnum::Planned
                }
                ForecastAnchorBackfillChunkStatus::Remaining => {
                    BackfillChunkReportStatusEnum::Remaining
                }
                ForecastAnchorBackfillChunkStatus::Skipped => {
                    BackfillChunkReportStatusEnum::Skipped
                }
            },
        }
    }
}

/// Validate exact configured and active storage identity before lifecycle I/O.
pub fn validate_forecast_anchor_backfill_source(
    arguments: &ForecastAnchorBackfillArguments,
    configuration: &ForecastAnchorRuntimeConfiguration,
    source: &ForecastAnchorBackfillSource,
) -> Result<(), TaskFailure> {
    let exact_capabilities =
        configuration.capabilities == BTreeSet::from([ForecastAnchorCapability::Historical]);
    let exact_contract = configuration.adapter.matches_retained_contract();
    let exact_identity = arguments.site == configuration.site_slug
        && arguments.source == configuration.source_key
        && source.active
        && configuration.active
        && configuration.cadence_seconds.is_none()
        && source.key == configuration.source_key
        && source.source_key == configuration.source_key
        && source.source_kind == ForecastAnchorSourceKind::Forecast
        && configuration.source_kind == ForecastAnchorSourceKind::Forecast
        && source.source_config_fingerprint == configuration.fingerprint
        && fingerprint_is_valid(&source.source_config_fingerprint)
        && source.provider_key == configuration.provider_key
        && source.site_slug == configuration.site_slug
        && source.station_slug == configuration.station_slug
        && source.timezone == configuration.timezone
        && source.material_config_identity == configuration.material_config_identity
        && source.latitude == configuration.latitude
        && source.longitude == configuration.longitude
        && source.latitude.is_finite()
        && (-90.0..=90.0).contains(&source.latitude)
        && source.longitude.is_finite()
        && (-180.0..=180.0).contains(&source.longitude);

    // reject contract or identity drift before resume reads, locks, or provider I/O
    if !exact_capabilities || !exact_contract || !exact_identity {
        return Err(worker_failure("forecast_anchor_source_identity_mismatch"));
    }
    Ok(())
}

/// Plan deterministic inclusive UTC chunks with half-open durable identities.
pub fn plan_forecast_anchor_backfill_chunks<T: ForecastAnchorBackfillTimeAdapter>(
    arguments: &ForecastAnchorBackfillArguments,
    source: &ForecastAnchorBackfillSource,
    time: &mut T,
) -> Result<Vec<PlannedForecastAnchorBackfillChunk>, TaskFailure> {
    validate_admitted_date(&arguments.from)?;
    validate_admitted_date(&arguments.to)?;

    // enforce the exact provider chunk cap
    if !(1..=FORECAST_ANCHOR_MAXIMUM_CHUNK_DAYS).contains(&arguments.chunk_days)
        || arguments.from.start_ms() > arguments.to.start_ms()
    {
        return Err(worker_failure("forecast_anchor_backfill_range_invalid"));
    }
    let mut chunks = Vec::new();
    let mut start_date = arguments.from.clone();

    // cover the requested inclusive UTC dates in ascending order
    while start_date.start_ms() <= arguments.to.start_ms() {
        let maximum_end = time.add_utc_days(&start_date, i64::from(arguments.chunk_days) - 1)?;

        // cap the last request at the inclusive requested end
        let end_date = if maximum_end.start_ms() < arguments.to.start_ms() {
            maximum_end
        } else {
            arguments.to.clone()
        };
        let interval_end_exclusive = time.add_utc_days(&end_date, 1)?;
        let identity = ForecastAnchorBackfillChunkIdentity {
            adapter_version: FORECAST_ANCHOR_ADAPTER_VERSION,
            chunk_plan_version: FORECAST_ANCHOR_CHUNK_PLAN_VERSION,
            interval_end_exclusive: interval_end_exclusive.clone(),
            interval_start: start_date.clone(),
            requested_from_date: arguments.from.label().to_owned(),
            requested_to_date: arguments.to.label().to_owned(),
            source_config_fingerprint: source.source_config_fingerprint.clone(),
            source_id: source.id.clone(),
        };
        validate_chunk_identity(&identity)?;
        chunks.push(PlannedForecastAnchorBackfillChunk {
            end_date,
            identity,
            start_date,
        });
        start_date = interval_end_exclusive;
    }
    Ok(chunks)
}

/// Normalize an admitted positional response into actual fixed-lead records and evidence.
pub fn normalize_forecast_anchor_payload(
    request: &ForecastAnchorProviderRequest,
    payload: ForecastAnchorAttemptPayload,
) -> Result<
    ProviderBatch<NormalizedForecastAnchorRecord, ForecastAnchorResponseMetadata, ()>,
    ForecastAnchorBackfillOperationError,
> {
    validate_provider_request(request)?;

    // require one positional response root for every requested location
    if payload.locations.len() != request.locations.len() || payload.attempts == 0 {
        return Err(invalid_payload(
            "forecast anchor positional response mismatch",
        ));
    }
    let mut counts = [ForecastAnchorLeadCounts::default(); 7];
    let mut records = Vec::new();
    let inclusive_days = (date_span_ms(request)? / MILLISECONDS_PER_DAY) as usize;
    let expected_hours = inclusive_days * 24;
    let mut generation_ms = 0.0;

    // preserve request-position binding across every response root
    for (location, parsed) in request.locations.iter().zip(payload.locations.iter()) {
        validate_envelope(&parsed.envelope)?;
        // ignore missing or invalid generation observations as the provider does
        if let Some(value) = parsed
            .generation_ms
            .filter(|value| value.is_finite() && *value >= 0.0)
        {
            generation_ms += value;
        }

        // require the complete requested hourly axis even when all cells are null
        if parsed.hours.len() != expected_hours {
            return Err(invalid_payload(
                "forecast anchor hourly axis length mismatch",
            ));
        }

        // expand each valid hour into the seven fixed leads
        for (hour_index, hour) in parsed.hours.iter().enumerate() {
            let expected_ms = request_start_ms(request)?
                + i64::try_from(hour_index).map_err(|_| invalid_payload("hour index overflow"))?
                    * MILLISECONDS_PER_HOUR;

            // reject gaps, duplicates, shifted instants, or leakage outside the chunk
            if hour.valid_at_ms != expected_ms
                || hour.valid_at_ms < request_start_ms(request)?
                || hour.valid_at_ms >= request_end_exclusive_ms(request)?
            {
                return Err(invalid_payload("forecast anchor hourly axis mismatch"));
            }

            // retain lead-major normalization inside each valid hour
            for (lead_index, cells) in hour.anchors.iter().copied().enumerate() {
                counts[lead_index].requested += 1;

                // omit only entirely-null anchors
                if cells.is_empty() {
                    counts[lead_index].null += 1;
                    continue;
                }
                let metrics = normalize_metrics(cells)?;
                let lead_hours = fixed_lead(lead_index)?;
                records.push(NormalizedForecastAnchorRecord {
                    adapterVersion: FORECAST_ANCHOR_ADAPTER_VERSION.to_owned(),
                    contractEpoch: FORECAST_ANCHOR_CONTRACT_EPOCH.to_owned(),
                    contractVersion: NormalizedForecastAnchorRecordContractVersionLiteral,
                    dataset: NormalizedForecastAnchorRecordDatasetLiteral,
                    leadHours: lead_hours,
                    metadata: create_record_metadata(&parsed.envelope),
                    metrics,
                    receivedAt: payload.received_at.clone(),
                    sourceConfigFingerprint: location.source_config_fingerprint.clone(),
                    sourceId: location.source_id.clone(),
                    sourceKind: NormalizedForecastAnchorRecordSourceKindLiteral,
                    upstreamModel: NormalizedForecastAnchorRecordUpstreamModelLiteral,
                    validAt: hour.valid_at.clone(),
                });
                counts[lead_index].populated += 1;
            }
        }
    }

    // reject a chunk with no usable anchor records
    if records.is_empty() {
        return Err(invalid_payload(
            "forecast anchor chunk had no populated anchors",
        ));
    }
    let anchor_counts_by_lead =
        std::array::from_fn(|index| (FORECAST_ANCHOR_LEAD_HOURS[index], counts[index]));
    let response_metadata = ForecastAnchorResponseMetadata {
        anchor_counts_by_lead,
        generation_ms,
        http_status: payload.http_status,
        location_response_count: payload.locations.len(),
        null_anchor_count: counts.iter().map(|count| count.null).sum(),
        populated_anchor_count: counts.iter().map(|count| count.populated).sum(),
        requested_anchor_count: counts.iter().map(|count| count.requested).sum(),
        requested_location_count: request.locations.len(),
        // 63 fields across a 14-day maximum cost exactly 45 hundredths per location-day
        weighted_api_call_cost: (request.locations.len() * inclusive_days * 45) as f64 / 100.0,
    };

    Ok(ProviderBatch {
        attempts: payload.attempts,
        provider_cursor: (),
        records,
        response_metadata,
        upstream_response_checksum: payload.checksum,
    })
}

/// Execute exact sequential resume, lock, provider, and atomic completion behavior.
pub fn execute_forecast_anchor_backfill<R, P, T>(
    arguments: &ForecastAnchorBackfillArguments,
    configuration: &ForecastAnchorRuntimeConfiguration,
    source: &ForecastAnchorBackfillSource,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<ForecastAnchorBackfillReport, TaskFailure>
where
    P: ForecastAnchorBackfillProvider,
    R: ForecastAnchorBackfillRepository,
    T: ForecastAnchorBackfillTimeAdapter,
{
    validate_forecast_anchor_backfill_source(arguments, configuration, source)?;
    validate_arguments(arguments, time)?;
    let chunks = plan_forecast_anchor_backfill_chunks(arguments, source, time)?;
    let mut reports = Vec::with_capacity(chunks.len());

    // execute every exact chunk in ascending date order
    for (index, chunk) in chunks.iter().enumerate() {
        // satisfy exact-success resume semantics before any lock or provider operation
        if arguments.resume && repository.has_successful_chunk(&chunk.identity)? {
            reports.push(chunk_report(
                chunk,
                ForecastAnchorBackfillChunkStatus::Skipped,
                None,
            ));
            continue;
        }

        // prove dry-run has no lock, provider, or durable writes
        if arguments.dry_run {
            reports.push(chunk_report(
                chunk,
                ForecastAnchorBackfillChunkStatus::Planned,
                None,
            ));
            continue;
        }

        // stop immediately when the source-scoped transaction lock is held
        let Some(session) = repository.acquire_source_session(&source.id)? else {
            reports.push(chunk_report(
                chunk,
                ForecastAnchorBackfillChunkStatus::Abandoned,
                Some("source_locked".to_owned()),
            ));
            append_remaining(&mut reports, &chunks[index + 1..]);
            return Ok(create_report(arguments, reports, 1));
        };
        let result = execute_chunk(chunk, source, session, repository, provider, time)?;
        let succeeded = result.status == ForecastAnchorBackfillChunkStatus::Completed;
        reports.push(result);

        // stop after the first failed committed chunk
        if !succeeded {
            append_remaining(&mut reports, &chunks[index + 1..]);
            return Ok(create_report(arguments, reports, 1));
        }
    }
    Ok(create_report(arguments, reports, 0))
}

// execute one committed chunk and retain primary outcome across cleanup
fn execute_chunk<R, P, T>(
    chunk: &PlannedForecastAnchorBackfillChunk,
    source: &ForecastAnchorBackfillSource,
    mut session: R::Session,
    repository: &mut R,
    provider: &mut P,
    time: &mut T,
) -> Result<ForecastAnchorBackfillChunkReport, TaskFailure>
where
    P: ForecastAnchorBackfillProvider,
    R: ForecastAnchorBackfillRepository,
    T: ForecastAnchorBackfillTimeAdapter,
{
    let mut run_id = None;
    let mut attempts = 0;
    let execution = (|| {
        let deadlines = time.deadlines()?;
        validate_deadlines(deadlines)?;
        repository.abandon_expired_runs(&mut session, deadlines.started_at_ms)?;
        let started_id = repository.start_ingestion_run(
            &mut session,
            ForecastAnchorBackfillRunStart {
                adapter_version: FORECAST_ANCHOR_ADAPTER_VERSION,
                chunk_plan_version: FORECAST_ANCHOR_CHUNK_PLAN_VERSION,
                deadline_at_ms: deadlines.run_deadline_at_ms,
                end_date: chunk.end_date.label().to_owned(),
                endpoint: FORECAST_ANCHOR_ENDPOINT,
                mode: ForecastAnchorIngestionMode::Backfill,
                requested_end_exclusive_ms: chunk.identity.interval_end_exclusive.start_ms(),
                requested_start_ms: chunk.identity.interval_start.start_ms(),
                source_config_fingerprint: chunk.identity.source_config_fingerprint.clone(),
                start_date: chunk.start_date.label().to_owned(),
            },
        )?;
        run_id = Some(started_id.clone());
        let request = create_provider_request(chunk, source, deadlines.provider_deadline_at_ms);
        let payload = provider.fetch(request.clone())?;
        attempts = payload.attempts;
        let batch = normalize_forecast_anchor_payload(&request, payload)?;
        assert_records_within_chunk(&batch.records, &chunk.identity)?;
        repository.complete_forecast_anchor_backfill_ingestion(
            &mut session,
            ForecastAnchorBackfillCompletion {
                attempts: batch.attempts,
                identity: chunk.identity.clone(),
                records: batch.records,
                response_metadata: batch.response_metadata,
                run_id: started_id,
                upstream_response_checksum: batch.upstream_response_checksum,
            },
        )?;
        Ok::<(), ForecastAnchorBackfillOperationError>(())
    })();
    let mut report = match execution {
        Ok(()) => chunk_report(chunk, ForecastAnchorBackfillChunkStatus::Completed, None),
        Err(error) => {
            let failure = as_backfill_provider_failure(error, attempts.max(1));
            let mut failed_report = chunk_report(
                chunk,
                ForecastAnchorBackfillChunkStatus::Failed,
                Some(failure.error.code.clone()),
            );

            // retain bounded finalization text without masking the primary code
            if let Some(started_id) = run_id.as_deref()
                && let Err(finalization) = repository.fail_ingestion_run(
                    &mut session,
                    started_id,
                    &chunk.identity,
                    &failure,
                )
            {
                failed_report.secondary_error = Some(finalization.bounded_message(512));
            }
            failed_report
        }
    };

    // preserve committed success while retaining cleanup as a secondary diagnostic
    if let Err(release) = repository.release_source_session(session) {
        report.secondary_error = Some(append_release_diagnostic(
            report.secondary_error.take(),
            &release,
        ));
    }
    Ok(report)
}

// create one exact provider request after the durable start
fn create_provider_request(
    chunk: &PlannedForecastAnchorBackfillChunk,
    source: &ForecastAnchorBackfillSource,
    deadline_at_ms: i64,
) -> ForecastAnchorProviderRequest {
    ForecastAnchorProviderRequest {
        contract_epoch: FORECAST_ANCHOR_CONTRACT_EPOCH,
        deadline_at_ms,
        end_date: chunk.end_date.label().to_owned(),
        end_exclusive_ms: chunk.identity.interval_end_exclusive.start_ms(),
        locations: vec![ForecastAnchorLocation {
            latitude: source.latitude,
            longitude: source.longitude,
            source_config_fingerprint: chunk.identity.source_config_fingerprint.clone(),
            source_id: source.id.clone(),
        }],
        start_date: chunk.start_date.label().to_owned(),
        start_ms: chunk.identity.interval_start.start_ms(),
    }
}

// require records to stay inside the durable half-open chunk
fn assert_records_within_chunk(
    records: &[NormalizedForecastAnchorRecord],
    identity: &ForecastAnchorBackfillChunkIdentity,
) -> Result<(), ForecastAnchorBackfillOperationError> {
    // retain only records produced for this exact source and material fingerprint
    if records.iter().any(|record| {
        record.sourceId != identity.source_id
            || record.sourceConfigFingerprint != identity.source_config_fingerprint
    }) {
        return Err(invalid_payload("forecast anchor record escaped chunk"));
    }
    Ok(())
}

// validate exact source request constraints
fn validate_provider_request(
    request: &ForecastAnchorProviderRequest,
) -> Result<(), ForecastAnchorBackfillOperationError> {
    let mut source_ids = BTreeSet::new();
    // require the frozen contract, bounded positional locations, and a positive deadline
    if request.contract_epoch != FORECAST_ANCHOR_CONTRACT_EPOCH
        || !(1..=100).contains(&request.locations.len())
        || request.deadline_at_ms <= 0
        || request.start_ms >= request.end_exclusive_ms
        || request.locations.iter().any(|location| {
            !location.latitude.is_finite()
                || !(-90.0..=90.0).contains(&location.latitude)
                || !location.longitude.is_finite()
                || !(-180.0..=180.0).contains(&location.longitude)
                || !fingerprint_is_valid(&location.source_config_fingerprint)
                || location.source_id.trim().is_empty()
                || location.source_id.encode_utf16().count() > 128
                || !source_ids.insert(location.source_id.as_str())
        })
    {
        return Err(invalid_payload("forecast anchor request invalid"));
    }
    Ok(())
}

// validate one zero-offset response envelope
fn validate_envelope(
    envelope: &ForecastAnchorEnvelope,
) -> Result<(), ForecastAnchorBackfillOperationError> {
    // require documented UTC/GMT semantics and finite coordinate metadata
    if envelope.utc_offset_seconds != 0
        || !envelope.latitude.is_finite()
        || !(-90.0..=90.0).contains(&envelope.latitude)
        || !envelope.longitude.is_finite()
        || !(-180.0..=180.0).contains(&envelope.longitude)
        || envelope.elevation_m.is_some_and(|value| !value.is_finite())
    {
        return Err(invalid_payload("forecast anchor envelope invalid"));
    }
    Ok(())
}

// normalize exactly the nine Previous Runs canonical metrics
fn normalize_metrics(
    cells: ForecastAnchorMetricCells,
) -> Result<CanonicalWeatherMetrics, ForecastAnchorBackfillOperationError> {
    let direction = match cells.wind_direction_degrees {
        Some(360.0) => Some(0.0),
        value => normalize_value(value, 0.0, 360.0, true)?,
    };
    Ok(CanonicalWeatherMetrics {
        apparentTemperatureC: nullable(normalize_value(
            cells.apparent_temperature_c,
            -100.0,
            70.0,
            false,
        )?),
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: nullable(normalize_value(
            cells.cloud_cover_percent,
            0.0,
            100.0,
            false,
        )?),
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: nullable(normalize_value(
            cells.precipitation_mm,
            0.0,
            2_000.0,
            false,
        )?),
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: nullable(normalize_value(cells.pressure_hpa, 100.0, 1_200.0, false)?),
        relativeHumidityPercent: nullable(normalize_value(
            cells.relative_humidity_percent,
            0.0,
            100.0,
            false,
        )?),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: nullable(normalize_value(cells.temperature_c, -100.0, 70.0, false)?),
        uvIndex: Nullable::Null,
        waterLevelM: Nullable::Null,
        windDirectionDegrees: nullable(direction),
        windGustMps: nullable(normalize_value(cells.wind_gust_mps, 0.0, 150.0, false)?),
        windSpeedMps: nullable(normalize_value(cells.wind_speed_mps, 0.0, 150.0, false)?),
        wetBulbGlobeTemperatureC: Nullable::Null,
    })
}

// validate one ordinary canonical metric
fn normalize_value(
    value: Option<f64>,
    minimum: f64,
    maximum: f64,
    maximum_exclusive: bool,
) -> Result<Option<f64>, ForecastAnchorBackfillOperationError> {
    // preserve provider null without inventing a canonical value
    let Some(value) = value else {
        return Ok(None);
    };

    // reject nonfinite or out-of-domain provider cells
    if !value.is_finite()
        || value < minimum
        || value > maximum
        || (maximum_exclusive && value == maximum)
    {
        return Err(invalid_payload("forecast anchor metric invalid"));
    }
    Ok(Some(value))
}

// project an optional value into its generated nullable contract
fn nullable<T>(value: Option<T>) -> Nullable<T> {
    match value {
        Some(value) => Nullable::Value(value),
        None => Nullable::Null,
    }
}

// format one already-admitted UTC date without introducing another calendar owner
fn utc_midnight(date: &str) -> String {
    format!("{date}T00:00:00.000Z")
}

// construct deterministic provider provenance without transport or a JSON parser
fn create_record_metadata(envelope: &ForecastAnchorEnvelope) -> WeatherRecordMetadata {
    let mut provider = vec![
        (
            utf16("dataset"),
            JsonValue::String(utf16(FORECAST_ANCHOR_DATASET)),
        ),
        (
            utf16("grid_cell"),
            JsonValue::String(utf16(&envelope.grid_cell)),
        ),
    ];

    // retain documented optional elevation in ordered provider metadata
    if let Some(elevation) = envelope.elevation_m {
        provider.insert(1, (utf16("elevation_m"), JsonValue::Number(elevation)));
    }
    WeatherRecordMetadata {
        device: Nullable::Null,
        model: Nullable::Value(FORECAST_ANCHOR_MODEL.to_owned()),
        provider: Nullable::Value(provider),
        quality: Nullable::Null,
        upstreamTimezone: "UTC".to_owned(),
    }
}

// convert one admitted Rust string to generated JavaScript string units
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

// map one frozen lead position to the generated contract enum
fn fixed_lead(
    index: usize,
) -> Result<FixedForecastLeadHours, ForecastAnchorBackfillOperationError> {
    match index {
        0 => Ok(FixedForecastLeadHours::Value24),
        1 => Ok(FixedForecastLeadHours::Value48),
        2 => Ok(FixedForecastLeadHours::Value72),
        3 => Ok(FixedForecastLeadHours::Value96),
        4 => Ok(FixedForecastLeadHours::Value120),
        5 => Ok(FixedForecastLeadHours::Value144),
        6 => Ok(FixedForecastLeadHours::Value168),
        _ => Err(invalid_payload("forecast anchor lead position invalid")),
    }
}

// validate top-level CLI-equivalent bounds
fn validate_arguments<T: ForecastAnchorBackfillTimeAdapter>(
    arguments: &ForecastAnchorBackfillArguments,
    time: &mut T,
) -> Result<(), TaskFailure> {
    let today = time.today_utc()?;
    validate_admitted_date(&today)?;

    // require a complete historical range strictly before today
    if arguments.site.is_empty()
        || arguments.source.is_empty()
        || !(1..=FORECAST_ANCHOR_MAXIMUM_CHUNK_DAYS).contains(&arguments.chunk_days)
        || arguments.from.start_ms() > arguments.to.start_ms()
        || arguments.to.start_ms() >= today.start_ms()
    {
        return Err(worker_failure("forecast_anchor_backfill_arguments_invalid"));
    }
    Ok(())
}

// require an exact admitted UTC midnight date
fn validate_admitted_date(date: &ForecastAnchorBackfillDate) -> Result<(), TaskFailure> {
    // fail closed on empty labels or a non-midnight epoch
    if date.label().is_empty() || date.start_ms().rem_euclid(MILLISECONDS_PER_DAY) != 0 {
        return Err(worker_failure("forecast_anchor_date_invalid"));
    }
    Ok(())
}

// validate durable identity invariants independent of display range
fn validate_chunk_identity(
    identity: &ForecastAnchorBackfillChunkIdentity,
) -> Result<(), TaskFailure> {
    // require one positive half-open interval and exact adapter identity
    if identity.adapter_version != FORECAST_ANCHOR_ADAPTER_VERSION
        || identity.chunk_plan_version != FORECAST_ANCHOR_CHUNK_PLAN_VERSION
        || identity.interval_start.start_ms() >= identity.interval_end_exclusive.start_ms()
        || !fingerprint_is_valid(&identity.source_config_fingerprint)
        || identity.source_id.is_empty()
    {
        return Err(worker_failure("forecast_anchor_chunk_identity_invalid"));
    }
    Ok(())
}

// validate deadline ordering shared with Node ingestion policy
fn validate_deadlines(
    deadlines: ForecastAnchorBackfillDeadlines,
) -> Result<(), ForecastAnchorBackfillOperationError> {
    // require provider completion before the durable finalization deadline
    if deadlines.provider_deadline_at_ms <= deadlines.started_at_ms
        || deadlines.run_deadline_at_ms <= deadlines.provider_deadline_at_ms
    {
        return Err(ForecastAnchorBackfillOperationError::unexpected(
            "forecast anchor deadlines invalid",
        ));
    }
    Ok(())
}

// calculate the half-open request span from admitted dates
fn date_span_ms(
    request: &ForecastAnchorProviderRequest,
) -> Result<i64, ForecastAnchorBackfillOperationError> {
    let start = request_start_ms(request)?;
    let end = request_end_exclusive_ms(request)?;

    // require a positive bounded whole-day range
    if end <= start
        || (end - start).rem_euclid(MILLISECONDS_PER_DAY) != 0
        || end - start > i64::from(FORECAST_ANCHOR_MAXIMUM_CHUNK_DAYS) * MILLISECONDS_PER_DAY
    {
        return Err(invalid_payload("forecast anchor date span invalid"));
    }
    Ok(end - start)
}

// recover the admitted start epoch from the first typed provider hour
fn request_start_ms(
    request: &ForecastAnchorProviderRequest,
) -> Result<i64, ForecastAnchorBackfillOperationError> {
    Ok(request.start_ms)
}

// recover the admitted end-exclusive epoch from the typed date labels
fn request_end_exclusive_ms(
    request: &ForecastAnchorProviderRequest,
) -> Result<i64, ForecastAnchorBackfillOperationError> {
    Ok(request.end_exclusive_ms)
}

// append untouched later chunks after the first terminal failure
fn append_remaining(
    reports: &mut Vec<ForecastAnchorBackfillChunkReport>,
    chunks: &[PlannedForecastAnchorBackfillChunk],
) {
    // preserve every planned later identity in report order
    for chunk in chunks {
        reports.push(chunk_report(
            chunk,
            ForecastAnchorBackfillChunkStatus::Remaining,
            None,
        ));
    }
}

// construct one bounded chunk report
fn chunk_report(
    chunk: &PlannedForecastAnchorBackfillChunk,
    status: ForecastAnchorBackfillChunkStatus,
    error_code: Option<String>,
) -> ForecastAnchorBackfillChunkReport {
    ForecastAnchorBackfillChunkReport {
        error_code,
        identity: chunk.identity.clone(),
        secondary_error: None,
        status,
    }
}

// construct the complete one-shot report
fn create_report(
    arguments: &ForecastAnchorBackfillArguments,
    chunks: Vec<ForecastAnchorBackfillChunkReport>,
    exit_code: u8,
) -> ForecastAnchorBackfillReport {
    ForecastAnchorBackfillReport {
        chunks,
        dry_run: arguments.dry_run,
        exit_code,
        from: arguments.from.label().to_owned(),
        resume: arguments.resume,
        site: arguments.site.clone(),
        source: arguments.source.clone(),
        to: arguments.to.label().to_owned(),
    }
}

// require an admitted lowercase hexadecimal fingerprint
fn fingerprint_is_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// construct one durable worker input failure
fn worker_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

// construct one provider-shape failure for catch-boundary retention
fn invalid_payload(message: &str) -> ForecastAnchorBackfillOperationError {
    ForecastAnchorBackfillOperationError::provider(
        1,
        super::TideBackfillErrorClassification::InvalidPayload,
        "invalid_payload",
        message,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;
    use std::process::Command;

    use weather_contracts::{FixedForecastLeadHours, Nullable};

    use super::*;
    use crate::{TideBackfillErrorClassification, TideBackfillIngestionError};

    const MARCH_1: i64 = 1_772_323_200_000;
    const AUGUST_22: i64 = 1_787_356_800_000;

    // construct one admitted UTC date
    fn date(label: &str, start_ms: i64) -> ForecastAnchorBackfillDate {
        ForecastAnchorBackfillDate::admitted(label, start_ms)
    }

    // construct every exact frozen source contract field
    fn configuration() -> ForecastAnchorRuntimeConfiguration {
        ForecastAnchorRuntimeConfiguration {
            active: true,
            adapter: ForecastAnchorAdapterConfiguration {
                contract_epoch: FORECAST_ANCHOR_CONTRACT_EPOCH.to_owned(),
                contract_version: FORECAST_ANCHOR_CONTRACT_VERSION.to_owned(),
                lead_hours: FORECAST_ANCHOR_LEAD_HOURS.to_vec(),
                maximum_chunk_days: FORECAST_ANCHOR_MAXIMUM_CHUNK_DAYS,
                model: FORECAST_ANCHOR_MODEL.to_owned(),
                variables: FORECAST_ANCHOR_VARIABLES
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            },
            cadence_seconds: None,
            capabilities: BTreeSet::from([ForecastAnchorCapability::Historical]),
            fingerprint: "a".repeat(64),
            latitude: 48.1,
            longitude: -122.7,
            material_config_identity: "admitted-previous-runs-material".to_owned(),
            provider_key: "open-meteo".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_key: "open-meteo-previous-runs-v1".to_owned(),
            source_kind: ForecastAnchorSourceKind::Forecast,
            station_slug: "ballydidean-primary".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // preserve javascript coordinate spelling
    #[test]
    fn record_metadata_spells_javascript_coordinates() {
        let metadata = create_record_metadata(&ForecastAnchorEnvelope {
            elevation_m: None,
            grid_cell: "0,1e-7".to_owned(),
            latitude: -0.0,
            longitude: 1e-7,
            timezone: ForecastAnchorTimezone::Utc,
            utc_offset_seconds: 0,
        });
        // require the deterministic provider metadata object
        let Nullable::Value(provider) = metadata.provider else {
            panic!("provider metadata must be present");
        };
        let grid_cell = provider
            .iter()
            .find(|(key, _)| key == &utf16("grid_cell"))
            .map(|(_, value)| value);

        assert_eq!(grid_cell, Some(&JsonValue::String(utf16("0,1e-7"))));
    }

    // construct one exact active database source identity
    fn source() -> ForecastAnchorBackfillSource {
        ForecastAnchorBackfillSource {
            active: true,
            id: "source-previous-runs".to_owned(),
            key: "open-meteo-previous-runs-v1".to_owned(),
            latitude: 48.1,
            longitude: -122.7,
            material_config_identity: "admitted-previous-runs-material".to_owned(),
            provider_key: "open-meteo".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_config_fingerprint: "a".repeat(64),
            source_key: "open-meteo-previous-runs-v1".to_owned(),
            source_kind: ForecastAnchorSourceKind::Forecast,
            station_slug: "ballydidean-primary".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }
    }

    // construct one ordinary one-day request
    fn arguments() -> ForecastAnchorBackfillArguments {
        ForecastAnchorBackfillArguments {
            chunk_days: 14,
            dry_run: false,
            from: date("2026-03-01", MARCH_1),
            resume: false,
            site: "ballydidean".to_owned(),
            source: "open-meteo-previous-runs-v1".to_owned(),
            to: date("2026-03-01", MARCH_1),
        }
    }

    #[derive(Debug)]
    struct FakeTime {
        labels: BTreeMap<i64, String>,
        deadline_index: i64,
    }

    impl FakeTime {
        // construct the dates used by planning and the current-day guard
        fn fixture() -> Self {
            let mut labels = BTreeMap::new();

            // admit every maximum-end date inspected by the 15-day planner fixture
            for offset in 0..=28 {
                let epoch = MARCH_1 + offset * MILLISECONDS_PER_DAY;
                labels.insert(epoch, format!("2026-03-{:02}", offset + 1));
            }
            labels.insert(AUGUST_22, "2026-08-22".to_owned());
            Self {
                labels,
                deadline_index: 0,
            }
        }
    }

    impl ForecastAnchorBackfillTimeAdapter for FakeTime {
        // return the current UTC date after the requested historical range
        fn today_utc(&mut self) -> Result<ForecastAnchorBackfillDate, TaskFailure> {
            Ok(date("2026-08-22", AUGUST_22))
        }

        // add exact admitted UTC days
        fn add_utc_days(
            &mut self,
            value: &ForecastAnchorBackfillDate,
            days: i64,
        ) -> Result<ForecastAnchorBackfillDate, TaskFailure> {
            let epoch = value.start_ms() + days * MILLISECONDS_PER_DAY;
            self.labels
                .get(&epoch)
                .map(|label| date(label, epoch))
                .ok_or_else(|| worker_failure("fixture_date_missing"))
        }

        // return exact Node fixture deadlines and advance between chunks
        fn deadlines(
            &mut self,
        ) -> Result<ForecastAnchorBackfillDeadlines, ForecastAnchorBackfillOperationError> {
            let started_at_ms = 1_787_375_200_000 + self.deadline_index * 10_000;
            self.deadline_index += 1;
            Ok(ForecastAnchorBackfillDeadlines {
                provider_deadline_at_ms: started_at_ms + 90_000,
                run_deadline_at_ms: started_at_ms + 120_000,
                started_at_ms,
            })
        }
    }

    // construct one entirely-null fixed lead
    fn null_cells() -> ForecastAnchorMetricCells {
        ForecastAnchorMetricCells {
            apparent_temperature_c: None,
            cloud_cover_percent: None,
            precipitation_mm: None,
            pressure_hpa: None,
            relative_humidity_percent: None,
            temperature_c: None,
            wind_direction_degrees: None,
            wind_gust_mps: None,
            wind_speed_mps: None,
        }
    }

    // construct one populated canonical fixed lead
    fn populated_cells() -> ForecastAnchorMetricCells {
        ForecastAnchorMetricCells {
            apparent_temperature_c: Some(9.0),
            cloud_cover_percent: Some(50.0),
            precipitation_mm: Some(0.0),
            pressure_hpa: Some(1_012.0),
            relative_humidity_percent: Some(75.0),
            temperature_c: Some(10.0),
            wind_direction_degrees: Some(360.0),
            wind_gust_mps: Some(4.0),
            wind_speed_mps: Some(2.0),
        }
    }

    // create one full typed hourly response with one populated anchor
    fn payload(request: &ForecastAnchorProviderRequest) -> ForecastAnchorAttemptPayload {
        let hours = (0..24)
            .map(|hour| {
                let mut anchors = [null_cells(); 7];

                // populate only the first fixed lead of the first valid hour
                if hour == 0 {
                    anchors[0] = populated_cells();
                }
                ForecastAnchorParsedHour {
                    anchors,
                    valid_at: format!("{}T{:02}:00:00.000Z", request.start_date, hour),
                    valid_at_ms: request.start_ms + i64::from(hour) * MILLISECONDS_PER_HOUR,
                }
            })
            .collect();
        ForecastAnchorAttemptPayload {
            attempts: 2,
            checksum: "d".repeat(64),
            http_status: 201,
            locations: vec![ForecastAnchorParsedLocation {
                envelope: ForecastAnchorEnvelope {
                    elevation_m: Some(12.5),
                    grid_cell: "48.1,-122.7".to_owned(),
                    latitude: 48.1,
                    longitude: -122.7,
                    timezone: ForecastAnchorTimezone::Utc,
                    utc_offset_seconds: 0,
                },
                generation_ms: Some(1.25),
                hours,
            }],
            received_at: "2026-08-22T05:20:00.000Z".to_owned(),
        }
    }

    #[derive(Default)]
    struct FakeProvider {
        requests: Vec<ForecastAnchorProviderRequest>,
    }

    impl ForecastAnchorBackfillProvider for FakeProvider {
        // retain the request and return actual typed parsed input
        fn fetch(
            &mut self,
            request: ForecastAnchorProviderRequest,
        ) -> Result<ForecastAnchorAttemptPayload, ForecastAnchorBackfillOperationError> {
            let response = payload(&request);
            self.requests.push(request);
            Ok(response)
        }
    }

    #[derive(Default)]
    struct FakeRepository {
        completions: Vec<ForecastAnchorBackfillCompletion>,
        events: Vec<String>,
        failures: Vec<ForecastAnchorBackfillProviderFailure>,
        finalization_failure: Option<String>,
        release_failure: Option<String>,
        starts: Vec<ForecastAnchorBackfillRunStart>,
        successful_starts: BTreeSet<i64>,
    }

    impl ForecastAnchorBackfillRepository for FakeRepository {
        type Session = String;

        // record the exact resume read
        fn has_successful_chunk(
            &mut self,
            identity: &ForecastAnchorBackfillChunkIdentity,
        ) -> Result<bool, TaskFailure> {
            self.events
                .push(format!("resume:{}", identity.interval_start.label()));
            Ok(self
                .successful_starts
                .contains(&identity.interval_start.start_ms()))
        }

        // acquire one source lock
        fn acquire_source_session(
            &mut self,
            source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            self.events.push("lock".to_owned());
            Ok(Some(source_id.to_owned()))
        }

        // record expiration before durable start
        fn abandon_expired_runs(
            &mut self,
            _session: &mut Self::Session,
            _started_at_ms: i64,
        ) -> Result<(), ForecastAnchorBackfillOperationError> {
            self.events.push("abandon".to_owned());
            Ok(())
        }

        // commit and retain one running row
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            start: ForecastAnchorBackfillRunStart,
        ) -> Result<String, ForecastAnchorBackfillOperationError> {
            self.events.push("start".to_owned());
            self.starts.push(start);
            Ok("run-1".to_owned())
        }

        // atomically retain actual records and response evidence
        fn complete_forecast_anchor_backfill_ingestion(
            &mut self,
            _session: &mut Self::Session,
            completion: ForecastAnchorBackfillCompletion,
        ) -> Result<(), ForecastAnchorBackfillOperationError> {
            self.events.push("complete".to_owned());
            self.completions.push(completion);
            Ok(())
        }

        // retain the exact durable failure
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _run_id: &str,
            _identity: &ForecastAnchorBackfillChunkIdentity,
            failure: &ForecastAnchorBackfillProviderFailure,
        ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
            self.events.push("fail".to_owned());
            self.failures.push(failure.clone());
            // surface the fixture's already-redacted finalization text when configured
            if let Some(message) = self.finalization_failure.take() {
                return Err(OpenMeteoArchiveCleanupFailure::admitted(message));
            }
            Ok(())
        }

        // release the source-scoped session last
        fn release_source_session(
            &mut self,
            _session: Self::Session,
        ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
            self.events.push("release".to_owned());
            // surface the fixture's already-redacted release text when configured
            if let Some(message) = self.release_failure.take() {
                return Err(OpenMeteoArchiveCleanupFailure::admitted(message));
            }
            Ok(())
        }
    }

    #[test]
    fn planner_matches_compiled_node_utc_chunk_contract() {
        let mut arguments = arguments();
        arguments.to = date("2026-03-15", MARCH_1 + 14 * MILLISECONDS_PER_DAY);
        let chunks =
            plan_forecast_anchor_backfill_chunks(&arguments, &source(), &mut FakeTime::fixture())
                .expect("plan");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].start_date.label(), "2026-03-01");
        assert_eq!(chunks[0].end_date.label(), "2026-03-14");
        assert_eq!(
            chunks[0].identity.interval_end_exclusive.start_ms()
                - chunks[0].identity.interval_start.start_ms(),
            14 * MILLISECONDS_PER_DAY
        );
        assert_eq!(
            chunks[1].identity.interval_start,
            chunks[0].identity.interval_end_exclusive
        );

        let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/forecast-anchor-backfill-node-oracle.mjs");
        let output = Command::new("node")
            .arg(oracle)
            .output()
            .expect("node oracle");
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("utf8 oracle");
        assert_eq!(
            stdout,
            concat!(
                "{\"cli\":{\"chunkDays\":14,\"dryRun\":false,\"from\":\"2026-03-08\",",
                "\"reportPath\":\"report.json\",\"resume\":true,\"site\":\"ballydidean\",",
                "\"source\":\"open-meteo-previous-runs-v1\",\"to\":\"2026-03-08\"},",
                "\"completeAttempts\":2,\"completeRecordCount\":1,",
                "\"events\":[\"lock\",\"abandon\",\"start\",\"fetch\",\"complete\",\"release\"],",
                "\"firstEnd\":\"2026-03-15T00:00:00.000Z\",",
                "\"firstStart\":\"2026-03-01T00:00:00.000Z\",",
                "\"normalized\":{\"count\":1,\"leadHours\":24,\"temperatureC\":10,",
                "\"validAt\":\"2026-03-08T00:00:00.000Z\",\"windDirectionDegrees\":0},",
                "\"planCount\":2,\"secondStart\":\"2026-03-15T00:00:00.000Z\",",
                "\"startDeadline\":\"2026-08-22T05:22:00.000Z\",",
                "\"startEndpoint\":\"previous-runs/forecast\",\"startMode\":\"backfill\",",
                "\"statuses\":[\"completed\"]}"
            )
        );
    }

    #[test]
    fn execution_commits_actual_normalized_anchor_records_atomically() {
        let mut repository = FakeRepository::default();
        let mut provider = FakeProvider::default();
        let report = execute_forecast_anchor_backfill(
            &arguments(),
            &configuration(),
            &source(),
            &mut repository,
            &mut provider,
            &mut FakeTime::fixture(),
        )
        .expect("execute");

        assert_eq!(report.exit_code, 0);
        assert_eq!(
            report.chunks[0].status,
            ForecastAnchorBackfillChunkStatus::Completed
        );
        assert_eq!(
            repository.events,
            ["lock", "abandon", "start", "complete", "release"]
        );
        assert_eq!(provider.requests.len(), 1);
        assert_eq!(provider.requests[0].deadline_at_ms, 1_787_375_290_000);
        assert_eq!(
            provider.requests[0].contract_epoch,
            FORECAST_ANCHOR_CONTRACT_EPOCH
        );
        assert_eq!(
            repository.starts[0].mode,
            ForecastAnchorIngestionMode::Backfill
        );
        assert_eq!(repository.starts[0].endpoint, FORECAST_ANCHOR_ENDPOINT);
        assert_eq!(repository.completions.len(), 1);
        let completion = &repository.completions[0];
        assert_eq!(completion.attempts, 2);
        assert_eq!(completion.records.len(), 1);
        assert_eq!(
            completion.records[0].leadHours,
            FixedForecastLeadHours::Value24
        );
        assert_eq!(completion.records[0].validAt, "2026-03-01T00:00:00.000Z");
        assert_eq!(
            completion.records[0].metrics.windDirectionDegrees,
            Nullable::Value(0.0)
        );
        assert_eq!(completion.response_metadata.requested_anchor_count, 168);
        assert_eq!(completion.response_metadata.populated_anchor_count, 1);
        assert_eq!(completion.response_metadata.null_anchor_count, 167);
        assert_eq!(completion.upstream_response_checksum, "d".repeat(64));
    }

    // compare wire evidence to the original provider result before JSON persistence
    #[test]
    fn response_metadata_matches_the_compiled_node_provider() {
        let chunks =
            plan_forecast_anchor_backfill_chunks(&arguments(), &source(), &mut FakeTime::fixture())
                .expect("plan");
        let request = create_provider_request(&chunks[0], &source(), 1_787_375_290_000);
        let batch =
            normalize_forecast_anchor_payload(&request, payload(&request)).expect("normalize");
        let mut actual = String::new();
        append_metadata_fields(&batch.response_metadata.to_json_value(), "", &mut actual);
        let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/forecast-anchor-backfill-node-oracle.mjs");
        let output = Command::new("node")
            .arg(oracle)
            .arg("--response-metadata")
            .output()
            .expect("metadata oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            actual,
            String::from_utf8(output.stdout).expect("utf8 metadata oracle")
        );
    }

    // compare names, order and scalar values without a test JSON parser
    fn append_metadata_fields(value: &JsonValue, prefix: &str, out: &mut String) {
        use std::fmt::Write as _;

        match value {
            JsonValue::Object(fields) => {
                // extend one original nested member name at a time
                for (key, field) in fields {
                    let key = String::from_utf16(key).expect("metadata field");
                    let path = if prefix.is_empty() {
                        key
                    } else {
                        format!("{prefix}.{key}")
                    };
                    append_metadata_fields(field, &path, out);
                }
            }
            JsonValue::Number(number) => writeln!(out, "{prefix}\t{number}").expect("write"),
            JsonValue::String(text) => writeln!(
                out,
                "{prefix}\t{}",
                String::from_utf16(text).expect("metadata value")
            )
            .expect("write"),
            _ => panic!("unexpected response metadata scalar"),
        }
    }

    struct FailingCompletionRepository(FakeRepository);

    impl ForecastAnchorBackfillRepository for FailingCompletionRepository {
        type Session = String;

        // delegate the resume check
        fn has_successful_chunk(
            &mut self,
            identity: &ForecastAnchorBackfillChunkIdentity,
        ) -> Result<bool, TaskFailure> {
            self.0.has_successful_chunk(identity)
        }

        // delegate source locking
        fn acquire_source_session(
            &mut self,
            source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            self.0.acquire_source_session(source_id)
        }

        // delegate abandonment
        fn abandon_expired_runs(
            &mut self,
            session: &mut Self::Session,
            started_at_ms: i64,
        ) -> Result<(), ForecastAnchorBackfillOperationError> {
            self.0.abandon_expired_runs(session, started_at_ms)
        }

        // delegate committed start
        fn start_ingestion_run(
            &mut self,
            session: &mut Self::Session,
            start: ForecastAnchorBackfillRunStart,
        ) -> Result<String, ForecastAnchorBackfillOperationError> {
            self.0.start_ingestion_run(session, start)
        }

        // fail the atomic completion after provider attempts are retained
        fn complete_forecast_anchor_backfill_ingestion(
            &mut self,
            _session: &mut Self::Session,
            _completion: ForecastAnchorBackfillCompletion,
        ) -> Result<(), ForecastAnchorBackfillOperationError> {
            self.0.events.push("completion-failed".to_owned());
            Err(ForecastAnchorBackfillOperationError::unexpected(
                "transaction rolled back",
            ))
        }

        // delegate exact durable failure finalization
        fn fail_ingestion_run(
            &mut self,
            session: &mut Self::Session,
            run_id: &str,
            identity: &ForecastAnchorBackfillChunkIdentity,
            failure: &ForecastAnchorBackfillProviderFailure,
        ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
            self.0
                .fail_ingestion_run(session, run_id, identity, failure)
        }

        // delegate terminal session release
        fn release_source_session(
            &mut self,
            session: Self::Session,
        ) -> Result<(), OpenMeteoArchiveCleanupFailure> {
            self.0.release_source_session(session)
        }
    }

    #[test]
    fn completion_failure_finalizes_with_retained_provider_attempts() {
        let mut repository = FailingCompletionRepository(FakeRepository::default());
        let report = execute_forecast_anchor_backfill(
            &arguments(),
            &configuration(),
            &source(),
            &mut repository,
            &mut FakeProvider::default(),
            &mut FakeTime::fixture(),
        )
        .expect("execute");

        assert_eq!(report.exit_code, 1);
        assert_eq!(
            report.chunks[0].status,
            ForecastAnchorBackfillChunkStatus::Failed
        );
        assert_eq!(
            report.chunks[0].error_code.as_deref(),
            Some("provider_unavailable")
        );
        assert_eq!(repository.0.failures.len(), 1);
        assert_eq!(repository.0.failures[0].attempts, 2);
        assert_eq!(
            repository.0.failures[0].error,
            TideBackfillIngestionError {
                classification: TideBackfillErrorClassification::Retryable,
                code: "provider_unavailable".to_owned(),
                message: "transaction rolled back".to_owned(),
            }
        );
        assert_eq!(
            repository.0.events,
            [
                "lock",
                "abandon",
                "start",
                "completion-failed",
                "fail",
                "release"
            ]
        );
    }

    struct FailingProvider;

    impl ForecastAnchorBackfillProvider for FailingProvider {
        // return one exact classified provider failure with retained attempts
        fn fetch(
            &mut self,
            _request: ForecastAnchorProviderRequest,
        ) -> Result<ForecastAnchorAttemptPayload, ForecastAnchorBackfillOperationError> {
            Err(ForecastAnchorBackfillOperationError::provider(
                3,
                TideBackfillErrorClassification::Retryable,
                "previous_runs_timeout",
                "fixture timeout",
            ))
        }
    }

    #[test]
    fn resume_failure_stops_provider_work_and_retains_remaining_identities() {
        let mut arguments = arguments();
        arguments.chunk_days = 1;
        arguments.resume = true;
        arguments.to = date("2026-03-03", MARCH_1 + 2 * MILLISECONDS_PER_DAY);
        let mut repository = FakeRepository::default();
        repository.successful_starts.insert(MARCH_1);
        let report = execute_forecast_anchor_backfill(
            &arguments,
            &configuration(),
            &source(),
            &mut repository,
            &mut FailingProvider,
            &mut FakeTime::fixture(),
        )
        .expect("execute");

        assert_eq!(report.exit_code, 1);
        assert_eq!(
            report
                .chunks
                .iter()
                .map(|chunk| chunk.status)
                .collect::<Vec<_>>(),
            [
                ForecastAnchorBackfillChunkStatus::Skipped,
                ForecastAnchorBackfillChunkStatus::Failed,
                ForecastAnchorBackfillChunkStatus::Remaining,
            ]
        );
        assert_eq!(
            report.chunks[1].error_code.as_deref(),
            Some("previous_runs_timeout")
        );
        assert_eq!(repository.failures.len(), 1);
        assert_eq!(repository.failures[0].attempts, 3);
        assert_eq!(
            repository.events,
            [
                "resume:2026-03-01",
                "resume:2026-03-02",
                "lock",
                "abandon",
                "start",
                "fail",
                "release",
            ]
        );
    }

    #[test]
    fn cleanup_text_and_generated_report_match_original_shape() {
        let mut repository = FakeRepository {
            finalization_failure: Some("finalization unavailable".to_owned()),
            release_failure: Some("release unavailable".to_owned()),
            ..FakeRepository::default()
        };
        let report = execute_forecast_anchor_backfill(
            &arguments(),
            &configuration(),
            &source(),
            &mut repository,
            &mut FailingProvider,
            &mut FakeTime::fixture(),
        )
        .expect("execute");
        assert_eq!(repository.failures[0].attempts, 3);

        let generated = report.into_backfill_report();
        assert_eq!(generated.exitCode, BackfillReportExitCodeEnum::Value1);
        assert_eq!(generated.chunks.len(), 1);
        let chunk = &generated.chunks[0];
        assert_eq!(chunk.status, BackfillChunkReportStatusEnum::Failed);
        assert_eq!(
            chunk.errorCode,
            Nullable::Value("previous_runs_timeout".to_owned())
        );
        assert_eq!(
            chunk.secondaryError,
            Nullable::Value(
                "finalization=finalization unavailable; release=release unavailable".to_owned()
            )
        );
        assert_eq!(chunk.identity.intervalStart, "2026-03-01T00:00:00.000Z");
        assert_eq!(
            chunk.identity.intervalEndExclusive,
            "2026-03-02T00:00:00.000Z"
        );
    }

    #[test]
    fn identity_drift_is_rejected_before_all_repository_and_provider_work() {
        let mut configuration = configuration();
        configuration.adapter.contract_version = "previous-runs-hourly/v2".to_owned();
        let mut repository = FakeRepository::default();
        let mut provider = FakeProvider::default();
        let error = execute_forecast_anchor_backfill(
            &arguments(),
            &configuration,
            &source(),
            &mut repository,
            &mut provider,
            &mut FakeTime::fixture(),
        )
        .expect_err("identity drift");
        assert_eq!(error.code, "forecast_anchor_source_identity_mismatch");
        assert!(repository.events.is_empty());
        assert!(provider.requests.is_empty());
    }
}
