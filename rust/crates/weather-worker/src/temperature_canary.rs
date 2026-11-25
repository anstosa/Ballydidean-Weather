use std::collections::BTreeSet;

use weather_contracts::{EcmwfTemperatureCanaryRecentErrorState, Nullable};

use crate::{
    TaskFailure, TemperatureCanaryCollectionResult, latest_available_ecmwf_initialization,
};

pub const OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION: &str = "open-meteo-ecmwf-single-run/v1";
pub const OPEN_METEO_ECMWF_SINGLE_RUN_MODEL: &str = "ecmwf_ifs";

const SIX_HOURS_MS: i64 = 6 * 3_600_000;
const SEVEN_HOURS_MS: i64 = 7 * 3_600_000;
const WARMUP_HOURS: i64 = 72;
const MAXIMUM_REQUESTED_RUNS: usize = 2;
const MAXIMUM_TIMEOUT_MS: u64 = 5_000;
const MAXIMUM_BODY_BYTES: usize = 512_000;
const MODEL_50R1_CUTOFF_MS: i64 = 1_778_565_600_000;

/// Root-verified temperature-canary authorization geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemperatureCanaryAuthorization {
    Inactive,
    /// The root-qualified permanent maintenance selection has no activation-clock gate.
    MaintenanceQualified,
    TimeLimited {
        activated_at_ms: i64,
        expires_at_ms: i64,
    },
    Permanent {
        activated_at_ms: i64,
    },
}

/// Immutable site and provider identity used by the private single-run source.
#[derive(Clone, Debug, PartialEq)]
pub struct TemperatureCanarySource {
    pub active: bool,
    pub adapter_version: String,
    pub capability: String,
    pub dataset: String,
    pub latitude: f64,
    pub longitude: f64,
    pub provider_key: String,
    pub site_slug: String,
    pub source_kind: String,
    pub upstream_model: String,
}

/// Optional operator fetch limits before the collector's stricter caps.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TemperatureCanaryFetchLimits {
    pub max_body_bytes: Option<usize>,
    pub timeout_ms: Option<u64>,
}

/// Exact single-attempt controls passed to the provider port.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemperatureCanaryFetchControls {
    pub max_attempts: u8,
    pub max_body_bytes: usize,
    pub timeout_ms: u64,
}

/// One exact private single-run provider request.
#[derive(Clone, Debug, PartialEq)]
pub struct TemperatureCanaryRequest {
    pub latitude: f64,
    pub longitude: f64,
    pub run_initialized_at: String,
}

/// One normalized private single-run hour.
#[derive(Clone, Debug, PartialEq)]
pub struct TemperatureCanaryHour {
    pub model_lead_hours: u8,
    pub raw_relative_humidity_percent: Option<f64>,
    pub raw_temperature_c: f64,
    pub raw_wind_speed_mps: Option<f64>,
    pub valid_at: String,
    pub valid_at_ms: i64,
}

/// One complete normalized private single-run response.
#[derive(Clone, Debug, PartialEq)]
pub struct TemperatureCanaryBatch {
    pub adapter_version: String,
    pub attempts: u8,
    pub hours: Vec<TemperatureCanaryHour>,
    pub model_cycle: String,
    pub provider_response_sha256: String,
    pub received_at: String,
    pub run_initialized_at: String,
    pub upstream_model: String,
}

/// Causal recent-error state selected before persistence.
#[derive(Clone, Debug, PartialEq)]
pub struct TemperatureCanaryRecentErrorState {
    pub b24_c: Option<f64>,
    pub b72_c: Option<f64>,
    pub local_dates: u64,
    pub mad72_c: Option<f64>,
    pub maximum_source_run_initialized_at: Option<String>,
    pub maximum_source_valid_at: Option<String>,
    pub n24: u64,
    pub n72: u64,
    pub source_keys: Vec<String>,
    pub supported: bool,
    pub target_run_initialized_at: String,
    pub window_end_valid_at: String,
}

impl From<EcmwfTemperatureCanaryRecentErrorState> for TemperatureCanaryRecentErrorState {
    /// Retain the generated reducer result in the collector persistence shape.
    fn from(state: EcmwfTemperatureCanaryRecentErrorState) -> Self {
        Self {
            b24_c: nullable_f64(state.b24C),
            b72_c: nullable_f64(state.b72C),
            local_dates: state.localDates,
            mad72_c: nullable_f64(state.mad72C),
            maximum_source_run_initialized_at: nullable_string(state.maximumSourceRunInitializedAt),
            maximum_source_valid_at: nullable_string(state.maximumSourceValidAt),
            n24: state.n24,
            n72: state.n72,
            source_keys: state.sourceKeys,
            supported: state.supported,
            target_run_initialized_at: state.targetRunInitializedAt,
            window_end_valid_at: state.windowEndValidAt,
        }
    }
}

/// One exact recent-state persistence disposition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemperatureCanaryStateStatus {
    Supported,
    Cold,
    Insufficient,
}

impl TemperatureCanaryStateStatus {
    /// Return the existing database status value.
    pub const fn code(self) -> &'static str {
        // preserve the Node persistence vocabulary
        match self {
            Self::Supported => "supported",
            Self::Cold => "cold",
            Self::Insufficient => "insufficient",
        }
    }

    /// Return the existing database reason value.
    pub const fn reason(self) -> &'static str {
        // bind every status to its exact explanatory reason
        match self {
            Self::Supported => "causal_recent_error_state_supported",
            Self::Cold => "no_causal_forecast_observation_pairs",
            Self::Insufficient => "causal_recent_error_state_insufficient",
        }
    }
}

/// One complete run ready for atomic serving persistence.
#[derive(Clone, Debug, PartialEq)]
pub struct TemperatureCanaryPersistedRun {
    pub batch: TemperatureCanaryBatch,
    pub recent_error_state: TemperatureCanaryRecentErrorState,
    pub site_slug: String,
    pub state_status: TemperatureCanaryStateStatus,
}

/// Full collector outcome including the exact latest-first request plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemperatureCanaryCollection {
    pub failed_runs: usize,
    pub persisted_runs: usize,
    pub requested_runs: Vec<String>,
}

impl From<&TemperatureCanaryCollection> for TemperatureCanaryCollectionResult {
    /// Project the detailed collector result into the existing sidecar diagnostic result.
    fn from(collection: &TemperatureCanaryCollection) -> Self {
        Self {
            failed_runs: collection.failed_runs,
            persisted_runs: collection.persisted_runs,
        }
    }
}

/// Time formatting delegated to the admitted runtime date boundary.
pub trait TemperatureCanaryTimeAdapter {
    /// Format one parsed epoch millisecond as canonical UTC.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure>;
}

/// Private single-run provider operation with already-parsed typed output.
pub trait TemperatureCanaryProvider {
    /// Execute one bounded request without implementing transport here.
    fn fetch_single_run(
        &mut self,
        request: &TemperatureCanaryRequest,
        controls: TemperatureCanaryFetchControls,
    ) -> Result<TemperatureCanaryBatch, TaskFailure>;
}

/// Causal state construction kept separate from inventory and persistence effects.
pub trait TemperatureCanaryRecentErrorBuilder {
    /// Build the newest run's state from admitted causal database evidence.
    fn build_recent_error_state(
        &mut self,
        site_slug: &str,
        target_run_initialized_at: &str,
        target_run_initialized_at_ms: i64,
    ) -> Result<TemperatureCanaryRecentErrorState, TaskFailure>;
}

/// Durable inventory and persistence effects.
pub trait TemperatureCanaryRepository {
    /// List already-persisted run initializations in the exact warmup window.
    fn list_run_initializations(
        &mut self,
        site_slug: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<String>, TaskFailure>;

    /// Atomically persist one normalized run and its selected recent state.
    fn persist_run(&mut self, run: &TemperatureCanaryPersistedRun) -> Result<(), TaskFailure>;
}

/// One cohesive temperature-canary collection execution.
pub struct TemperatureCanaryExecution<'a, Repository, Provider, RecentError, Time> {
    pub fetch_limits: TemperatureCanaryFetchLimits,
    pub provider: &'a mut Provider,
    pub recent_error: &'a mut RecentError,
    pub repository: &'a mut Repository,
    pub source: &'a TemperatureCanarySource,
    pub time: &'a mut Time,
}

/// Report whether a root-verified candidate admits collection at one instant.
pub const fn temperature_canary_collection_is_active(
    authorization: TemperatureCanaryAuthorization,
    now_ms: i64,
) -> bool {
    // preserve closed defaults and half-open expiration semantics
    match authorization {
        TemperatureCanaryAuthorization::Inactive => false,
        TemperatureCanaryAuthorization::MaintenanceQualified => true,
        TemperatureCanaryAuthorization::TimeLimited {
            activated_at_ms,
            expires_at_ms,
        } => now_ms >= activated_at_ms && now_ms < expires_at_ms,
        TemperatureCanaryAuthorization::Permanent { activated_at_ms } => now_ms >= activated_at_ms,
    }
}

// build one stable collector failure
fn canary_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

// project one generated nullable number into the collector's optional shape
fn nullable_f64(value: Nullable<f64>) -> Option<f64> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value),
    }
}

// project one generated nullable string into the collector's optional shape
fn nullable_string(value: Nullable<String>) -> Option<String> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value),
    }
}

// require one lowercase hexadecimal sha-256 identity
fn sha256_is_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

// validate the fixed source identity before inventory or provider work
fn validate_source(source: &TemperatureCanarySource) -> Result<(), TaskFailure> {
    // require the exact private provider family and one finite site location
    if !source.active
        || source.adapter_version != OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION
        || source.capability != "forecast"
        || source.dataset != "single_run"
        || source.provider_key != "open-meteo"
        || source.source_kind != "forecast"
        || source.upstream_model != OPEN_METEO_ECMWF_SINGLE_RUN_MODEL
        || source.site_slug.is_empty()
        || !source.latitude.is_finite()
        || !(-90.0..=90.0).contains(&source.latitude)
        || !source.longitude.is_finite()
        || !(-180.0..=180.0).contains(&source.longitude)
    {
        return Err(canary_failure("temperature canary source identity differs"));
    }
    Ok(())
}

// clamp operator controls to the stricter sidecar budget
fn bounded_fetch_controls(limits: TemperatureCanaryFetchLimits) -> TemperatureCanaryFetchControls {
    TemperatureCanaryFetchControls {
        max_attempts: 1,
        max_body_bytes: limits
            .max_body_bytes
            .unwrap_or(MAXIMUM_BODY_BYTES)
            .min(MAXIMUM_BODY_BYTES),
        timeout_ms: limits
            .timeout_ms
            .unwrap_or(MAXIMUM_TIMEOUT_MS)
            .min(MAXIMUM_TIMEOUT_MS),
    }
}

// plan at most two newest missing six-hour initializations
fn plan_missing_runs<R, T>(
    now_ms: i64,
    source: &TemperatureCanarySource,
    repository: &mut R,
    time: &mut T,
) -> Result<Vec<(i64, String)>, TaskFailure>
where
    R: TemperatureCanaryRepository,
    T: TemperatureCanaryTimeAdapter,
{
    let latest_ms = latest_available_ecmwf_initialization(now_ms)
        .map_err(|_| canary_failure("temperature canary initialization overflowed"))?;
    let oldest_ms = latest_ms
        .checked_sub(WARMUP_HOURS * 3_600_000)
        .ok_or_else(|| canary_failure("temperature canary warmup overflowed"))?;
    let latest = time.utc_string(latest_ms)?;
    let oldest = time.utc_string(oldest_ms)?;
    let collected = repository
        .list_run_initializations(&source.site_slug, &oldest, &latest)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut missing = Vec::with_capacity((WARMUP_HOURS / 6 + 1) as usize);

    // enumerate the complete latest-first warmup window before taking two gaps
    for offset_hours in (0..=WARMUP_HOURS).step_by((SIX_HOURS_MS / 3_600_000) as usize) {
        let initialized_at_ms = latest_ms
            .checked_sub(offset_hours * 3_600_000)
            .ok_or_else(|| canary_failure("temperature canary warmup overflowed"))?;
        let initialized_at = time.utc_string(initialized_at_ms)?;

        // retain only exact initializations not already persisted
        if !collected.contains(&initialized_at) {
            missing.push((initialized_at_ms, initialized_at));
        }
    }
    missing.truncate(MAXIMUM_REQUESTED_RUNS);
    Ok(missing)
}

// create truthful cold state for non-serving warmup runs
fn cold_recent_error_state<T: TemperatureCanaryTimeAdapter>(
    initialized_at_ms: i64,
    initialized_at: &str,
    time: &mut T,
) -> Result<TemperatureCanaryRecentErrorState, TaskFailure> {
    let window_end_ms = initialized_at_ms
        .checked_sub(SEVEN_HOURS_MS)
        .ok_or_else(|| canary_failure("temperature canary state clock overflowed"))?;
    Ok(TemperatureCanaryRecentErrorState {
        b24_c: None,
        b72_c: None,
        local_dates: 0,
        mad72_c: None,
        maximum_source_run_initialized_at: None,
        maximum_source_valid_at: None,
        n24: 0,
        n72: 0,
        source_keys: Vec::new(),
        supported: false,
        target_run_initialized_at: initialized_at.to_owned(),
        window_end_valid_at: time.utc_string(window_end_ms)?,
    })
}

// validate causal state identity and finite statistics
fn validate_recent_error_state(
    state: &TemperatureCanaryRecentErrorState,
    target_run_initialized_at: &str,
) -> Result<TemperatureCanaryStateStatus, TaskFailure> {
    let statistics_are_finite = [state.b24_c, state.b72_c, state.mad72_c]
        .into_iter()
        .flatten()
        .all(f64::is_finite);

    // reject cross-run state or contradictory supported geometry
    if state.target_run_initialized_at != target_run_initialized_at
        || state.window_end_valid_at.is_empty()
        || !statistics_are_finite
        || (state.supported && (state.n24 == 0 || state.n72 == 0))
    {
        return Err(canary_failure("temperature canary recent state differs"));
    }
    Ok(if state.supported {
        TemperatureCanaryStateStatus::Supported
    } else if state.n72 == 0 {
        TemperatureCanaryStateStatus::Cold
    } else {
        TemperatureCanaryStateStatus::Insufficient
    })
}

// validate one complete normalized provider batch
fn validate_batch<T: TemperatureCanaryTimeAdapter>(
    batch: &TemperatureCanaryBatch,
    request: &TemperatureCanaryRequest,
    initialized_at_ms: i64,
    time: &mut T,
) -> Result<(), TaskFailure> {
    let expected_cycle = if initialized_at_ms < MODEL_50R1_CUTOFF_MS {
        "49r1"
    } else {
        "50r1"
    };

    // bind response identity and exact eighteen-hour source geometry
    if batch.adapter_version != OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION
        || batch.attempts != 1
        || batch.hours.len() != 18
        || batch.model_cycle != expected_cycle
        || !sha256_is_valid(&batch.provider_response_sha256)
        || batch.received_at.is_empty()
        || batch.run_initialized_at != request.run_initialized_at
        || batch.upstream_model != OPEN_METEO_ECMWF_SINGLE_RUN_MODEL
    {
        return Err(canary_failure("temperature canary provider batch differs"));
    }

    // preserve exact lead order and physical ranges from provider normalization
    for (index, hour) in batch.hours.iter().enumerate() {
        let expected_lead = (index + 1) as u8;
        let expected_valid_at_ms = initialized_at_ms
            .checked_add(i64::from(expected_lead) * 3_600_000)
            .ok_or_else(|| canary_failure("temperature canary hour overflowed"))?;
        let expected_valid_at = time.utc_string(expected_valid_at_ms)?;
        let humidity_valid = hour
            .raw_relative_humidity_percent
            .is_none_or(|value| value.is_finite() && (0.0..=100.0).contains(&value));
        let wind_valid = hour
            .raw_wind_speed_mps
            .is_none_or(|value| value.is_finite() && (0.0..=150.0).contains(&value));

        // reject reordered clocks or invalid physical fields before persistence
        if hour.model_lead_hours != expected_lead
            || hour.valid_at != expected_valid_at
            || hour.valid_at_ms != expected_valid_at_ms
            || !hour.raw_temperature_c.is_finite()
            || !(-100.0..=70.0).contains(&hour.raw_temperature_c)
            || !humidity_valid
            || !wind_valid
        {
            return Err(canary_failure("temperature canary provider hour differs"));
        }
    }
    Ok(())
}

/// Collect at most two missing ECMWF temperature-canary runs latest first.
pub fn collect_ecmwf_temperature_canary_runs<Repository, Provider, RecentError, Time>(
    now_ms: i64,
    execution: &mut TemperatureCanaryExecution<'_, Repository, Provider, RecentError, Time>,
) -> Result<TemperatureCanaryCollection, TaskFailure>
where
    Provider: TemperatureCanaryProvider,
    RecentError: TemperatureCanaryRecentErrorBuilder,
    Repository: TemperatureCanaryRepository,
    Time: TemperatureCanaryTimeAdapter,
{
    validate_source(execution.source)?;
    let requested = plan_missing_runs(
        now_ms,
        execution.source,
        execution.repository,
        execution.time,
    )?;
    let latest_ms = latest_available_ecmwf_initialization(now_ms)
        .map_err(|_| canary_failure("temperature canary initialization overflowed"))?;
    let controls = bounded_fetch_controls(execution.fetch_limits);
    let mut failed_runs = 0;
    let mut persisted_runs = 0;

    // isolate provider, causal-state, and persistence failures per requested run
    for (initialized_at_ms, initialized_at) in &requested {
        let request = TemperatureCanaryRequest {
            latitude: execution.source.latitude,
            longitude: execution.source.longitude,
            run_initialized_at: initialized_at.clone(),
        };
        let attempted = (|| {
            let batch = execution.provider.fetch_single_run(&request, controls)?;
            validate_batch(&batch, &request, *initialized_at_ms, execution.time)?;
            let state = if *initialized_at_ms == latest_ms {
                execution.recent_error.build_recent_error_state(
                    &execution.source.site_slug,
                    initialized_at,
                    *initialized_at_ms,
                )?
            } else {
                cold_recent_error_state(*initialized_at_ms, initialized_at, execution.time)?
            };
            let state_status = validate_recent_error_state(&state, initialized_at)?;
            execution
                .repository
                .persist_run(&TemperatureCanaryPersistedRun {
                    batch,
                    recent_error_state: state,
                    site_slug: execution.source.site_slug.clone(),
                    state_status,
                })
        })();

        // continue bounded warmup after one unavailable or rejected run
        if attempted.is_ok() {
            persisted_runs += 1;
        } else {
            failed_runs += 1;
        }
    }
    Ok(TemperatureCanaryCollection {
        failed_runs,
        persisted_runs,
        requested_runs: requested
            .into_iter()
            .map(|(_, initialized_at)| initialized_at)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::process::Command;
    use std::rc::Rc;

    use super::{
        OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION, OPEN_METEO_ECMWF_SINGLE_RUN_MODEL,
        SIX_HOURS_MS, TemperatureCanaryAuthorization, TemperatureCanaryBatch,
        TemperatureCanaryExecution, TemperatureCanaryFetchControls, TemperatureCanaryFetchLimits,
        TemperatureCanaryHour, TemperatureCanaryPersistedRun, TemperatureCanaryProvider,
        TemperatureCanaryRecentErrorBuilder, TemperatureCanaryRecentErrorState,
        TemperatureCanaryRepository, TemperatureCanaryRequest, TemperatureCanarySource,
        TemperatureCanaryTimeAdapter, collect_ecmwf_temperature_canary_runs,
        temperature_canary_collection_is_active,
    };
    use crate::TaskFailure;

    const NOW_MS: i64 = 1_788_827_700_000;
    const LATEST_MS: i64 = 1_788_804_000_000;

    // build one stable fixture failure
    fn failure(code: &str) -> TaskFailure {
        TaskFailure {
            attempts: 1,
            code: code.to_owned(),
        }
    }

    // build the exact admitted private source identity
    fn source() -> TemperatureCanarySource {
        TemperatureCanarySource {
            active: true,
            adapter_version: OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION.to_owned(),
            capability: "forecast".to_owned(),
            dataset: "single_run".to_owned(),
            latitude: 56.0,
            longitude: -3.0,
            provider_key: "open-meteo".to_owned(),
            site_slug: "ballydidean".to_owned(),
            source_kind: "forecast".to_owned(),
            upstream_model: OPEN_METEO_ECMWF_SINGLE_RUN_MODEL.to_owned(),
        }
    }

    struct FixtureTime;

    impl TemperatureCanaryTimeAdapter for FixtureTime {
        // format fixture cycles without implementing a date codec
        fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
            let hours = (LATEST_MS - epoch_ms) / 3_600_000;
            Ok(match hours {
                0 => "2026-09-07T18:00:00.000Z".to_owned(),
                6 => "2026-09-07T12:00:00.000Z".to_owned(),
                7 => "2026-09-07T11:00:00.000Z".to_owned(),
                12 => "2026-09-07T06:00:00.000Z".to_owned(),
                72 => "2026-09-04T18:00:00.000Z".to_owned(),
                _ => format!("valid-{epoch_ms}"),
            })
        }
    }

    struct FixtureProvider {
        fail_first: bool,
        trace: Rc<RefCell<Vec<String>>>,
    }

    impl TemperatureCanaryProvider for FixtureProvider {
        // return one complete normalized eighteen-hour batch
        fn fetch_single_run(
            &mut self,
            request: &TemperatureCanaryRequest,
            controls: TemperatureCanaryFetchControls,
        ) -> Result<TemperatureCanaryBatch, TaskFailure> {
            self.trace.borrow_mut().push(format!(
                "fetch:{}:{}:{}:{}",
                request.run_initialized_at,
                controls.max_attempts,
                controls.timeout_ms,
                controls.max_body_bytes
            ));

            // isolate one newest-run provider failure
            if self.fail_first {
                self.fail_first = false;
                return Err(failure("provider unavailable"));
            }
            let initialized_at_ms = if request.run_initialized_at == "2026-09-07T18:00:00.000Z" {
                LATEST_MS
            } else {
                LATEST_MS - SIX_HOURS_MS
            };
            let mut time = FixtureTime;
            Ok(TemperatureCanaryBatch {
                adapter_version: OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION.to_owned(),
                attempts: 1,
                hours: (1..=18)
                    .map(|lead| {
                        let valid_at_ms = initialized_at_ms + i64::from(lead) * 3_600_000;
                        TemperatureCanaryHour {
                            model_lead_hours: lead,
                            raw_relative_humidity_percent: Some(75.0),
                            raw_temperature_c: 10.0,
                            raw_wind_speed_mps: Some(3.0),
                            valid_at: time
                                .utc_string(valid_at_ms)
                                .expect("fixture hour should format"),
                            valid_at_ms,
                        }
                    })
                    .collect(),
                model_cycle: "50r1".to_owned(),
                provider_response_sha256: "a".repeat(64),
                received_at: "2026-09-08T00:35:00.000Z".to_owned(),
                run_initialized_at: request.run_initialized_at.clone(),
                upstream_model: OPEN_METEO_ECMWF_SINGLE_RUN_MODEL.to_owned(),
            })
        }
    }

    struct FixtureRepository {
        collected: Vec<String>,
        persisted: Rc<RefCell<Vec<TemperatureCanaryPersistedRun>>>,
        trace: Rc<RefCell<Vec<String>>>,
    }

    impl TemperatureCanaryRepository for FixtureRepository {
        // return exact persisted inventory in the warmup range
        fn list_run_initializations(
            &mut self,
            site_slug: &str,
            from: &str,
            to: &str,
        ) -> Result<Vec<String>, TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("list:{site_slug}:{from}:{to}"));
            Ok(self.collected.clone())
        }

        // retain one atomic normalized persistence input
        fn persist_run(&mut self, run: &TemperatureCanaryPersistedRun) -> Result<(), TaskFailure> {
            self.trace.borrow_mut().push(format!(
                "persist:{}:{}:{}",
                run.batch.run_initialized_at,
                run.state_status.code(),
                run.state_status.reason()
            ));
            self.persisted.borrow_mut().push(run.clone());
            Ok(())
        }
    }

    struct FixtureRecentError {
        trace: Rc<RefCell<Vec<String>>>,
    }

    impl TemperatureCanaryRecentErrorBuilder for FixtureRecentError {
        // build one supported state only for the newest requested run
        fn build_recent_error_state(
            &mut self,
            site_slug: &str,
            target_run_initialized_at: &str,
            target_run_initialized_at_ms: i64,
        ) -> Result<TemperatureCanaryRecentErrorState, TaskFailure> {
            assert_eq!(target_run_initialized_at_ms, LATEST_MS);
            self.trace
                .borrow_mut()
                .push(format!("state:{site_slug}:{target_run_initialized_at}"));
            Ok(TemperatureCanaryRecentErrorState {
                b24_c: Some(2.0),
                b72_c: Some(2.0),
                local_dates: 2,
                mad72_c: Some(0.0),
                maximum_source_run_initialized_at: Some("2026-09-07T06:00:00.000Z".to_owned()),
                maximum_source_valid_at: Some("2026-09-07T17:00:00.000Z".to_owned()),
                n24: 24,
                n72: 24,
                source_keys: vec!["source".to_owned()],
                supported: true,
                target_run_initialized_at: target_run_initialized_at.to_owned(),
                window_end_valid_at: "2026-09-07T11:00:00.000Z".to_owned(),
            })
        }
    }

    // execute one admitted collector fixture
    fn run_fixture(fail_first: bool) -> (super::TemperatureCanaryCollection, Vec<String>) {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let persisted = Rc::new(RefCell::new(Vec::new()));
        let mut repository = FixtureRepository {
            collected: Vec::new(),
            persisted: Rc::clone(&persisted),
            trace: Rc::clone(&trace),
        };
        let mut provider = FixtureProvider {
            fail_first,
            trace: Rc::clone(&trace),
        };
        let mut recent_error = FixtureRecentError {
            trace: Rc::clone(&trace),
        };
        let mut time = FixtureTime;
        let result = collect_ecmwf_temperature_canary_runs(
            NOW_MS,
            &mut TemperatureCanaryExecution {
                fetch_limits: TemperatureCanaryFetchLimits {
                    max_body_bytes: Some(900_000),
                    timeout_ms: Some(20_000),
                },
                provider: &mut provider,
                recent_error: &mut recent_error,
                repository: &mut repository,
                source: &source(),
                time: &mut time,
            },
        )
        .expect("temperature canary collection should complete");
        let events = trace.borrow().clone();
        (result, events)
    }

    /// Match latest-first bounded collection and cold-start persistence.
    #[test]
    fn collects_two_missing_runs_with_exact_controls_and_state() {
        let (result, events) = run_fixture(false);
        assert_eq!(
            result.requested_runs,
            ["2026-09-07T18:00:00.000Z", "2026-09-07T12:00:00.000Z"]
        );
        assert_eq!(result.persisted_runs, 2);
        assert_eq!(result.failed_runs, 0);
        assert!(events.contains(&"fetch:2026-09-07T18:00:00.000Z:1:5000:512000".to_owned()));
        assert!(
            events.contains(
                &"persist:2026-09-07T18:00:00.000Z:supported:causal_recent_error_state_supported"
                    .to_owned()
            )
        );
        assert!(
            events.contains(
                &"persist:2026-09-07T12:00:00.000Z:cold:no_causal_forecast_observation_pairs"
                    .to_owned()
            )
        );
        assert_eq!(node_oracle(), "18,12|2|0|1,5000,512000|cold,cold");
    }

    /// Continue warmup after one isolated unavailable run.
    #[test]
    fn isolates_provider_failure_and_persists_older_run() {
        let (result, events) = run_fixture(true);
        assert_eq!(result.failed_runs, 1);
        assert_eq!(result.persisted_runs, 1);
        assert!(!events.iter().any(|event| event.starts_with("state:")));
        assert!(events.iter().any(|event| event.contains(":cold:")));
    }

    /// Enforce activation and half-open expiration without side effects.
    #[test]
    fn authorization_uses_exact_activation_boundaries() {
        let limited = TemperatureCanaryAuthorization::TimeLimited {
            activated_at_ms: 10,
            expires_at_ms: 20,
        };
        assert!(!temperature_canary_collection_is_active(limited, 9));
        assert!(temperature_canary_collection_is_active(limited, 10));
        assert!(!temperature_canary_collection_is_active(limited, 20));
        assert!(temperature_canary_collection_is_active(
            TemperatureCanaryAuthorization::Permanent {
                activated_at_ms: 10,
            },
            20,
        ));
        assert!(temperature_canary_collection_is_active(
            TemperatureCanaryAuthorization::MaintenanceQualified,
            -1,
        ));
        assert!(!temperature_canary_collection_is_active(
            TemperatureCanaryAuthorization::Inactive,
            20,
        ));
    }

    // invoke the compiled normal TypeScript collector fixture
    fn node_oracle() -> String {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/temperature_canary_collector_oracle.mjs");
        let output = Command::new("node")
            .arg(script)
            .output()
            .expect("compiled Node collector oracle should start");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("Node collector oracle should emit UTF-8")
            .trim()
            .to_owned()
    }
}
