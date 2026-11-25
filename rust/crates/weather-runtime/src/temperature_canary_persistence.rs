use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    EcmwfTemperatureCanaryRun, EcmwfTemperatureCanaryRunAdapterVersionLiteral,
    EcmwfTemperatureCanaryRunModelCycleEnum, EcmwfTemperatureCanaryRunStateStatusEnum,
    EcmwfTemperatureCanaryRunUpstreamModelLiteral, JsDateOrString, Nullable,
};
use weather_database::{
    AdmittedEcmwfCanaryRevisionWindow, AdmittedEcmwfCanaryRunPersistence,
    EcmwfCanaryRevisionArchivePort, EcmwfTemperatureCanaryRunInitializationsReadPlan,
    PreparedEcmwfCanaryHour, SerializedJson, Sha256Digest,
    ecmwf_temperature_canary_run_initializations_read_plan, persist_ecmwf_temperature_canary_run,
    persist_ecmwf_temperature_canary_run_with_revision_archive,
};
use weather_worker::{
    TaskFailure, TemperatureCanaryHour, TemperatureCanaryPersistedRun, TemperatureCanaryRepository,
    TemperatureCanaryStateStatus,
};

const HOUR_MILLISECONDS: i64 = 3_600_000;

/// Whether a configured runtime exposes the optional cold revision archiver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemperatureCanaryRevisionArchiveMode {
    Disabled,
    Enabled,
}

/// Exact canonical fields hashed for one immutable run identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemperatureCanaryRunDigestMaterial {
    pub adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
    pub hour_content_hashes: Vec<Sha256Digest>,
    pub model_cycle: EcmwfTemperatureCanaryRunModelCycleEnum,
    pub provider_response_sha256: Sha256Digest,
    pub recent_error_state_sha256: Sha256Digest,
    pub run_initialized_at: String,
    pub state_reason: String,
    pub state_status: EcmwfTemperatureCanaryRunStateStatusEnum,
    pub upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
}

/// Existing canonical digest owner shared by persistence and fail-raw serving admission.
pub trait TemperatureCanaryRecentErrorDigestPort {
    /// Hash one generated recent state through the existing canonical digest owner.
    fn digest_recent_error_state(
        &mut self,
        state: &EcmwfTemperatureCanaryRecentErrorState,
    ) -> Result<Sha256Digest, TaskFailure>;
}

/// Existing canonical time, JSON, and digest owners used before database execution.
pub trait TemperatureCanaryPersistencePreparationPort:
    TemperatureCanaryRecentErrorDigestPort
{
    /// Parse one explicit-zone instant with the original clock normalization.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, TaskFailure>;

    /// Format one already resolved epoch through the existing time owner.
    fn utc_string(&mut self, epoch_milliseconds: i64) -> Result<String, TaskFailure>;

    /// Serialize one generated recent state through the existing JSON owner.
    fn serialize_recent_error_state(
        &mut self,
        state: &EcmwfTemperatureCanaryRecentErrorState,
    ) -> Result<SerializedJson, TaskFailure>;

    /// Hash one normalized hour through the existing canonical digest owner.
    fn digest_hour(&mut self, hour: &TemperatureCanaryHour) -> Result<Sha256Digest, TaskFailure>;

    /// Hash the exact run material through the existing canonical digest owner.
    fn digest_run(
        &mut self,
        material: &TemperatureCanaryRunDigestMaterial,
    ) -> Result<Sha256Digest, TaskFailure>;
}

/// Database port adding the collector's inventory read to exact persistence commands.
pub trait TemperatureCanaryPersistenceDatabasePort: EcmwfCanaryRevisionArchivePort {
    /// Execute the exact warmup query and return its ordered timestamp column.
    fn read_temperature_canary_run_initializations(
        &mut self,
        plan: &EcmwfTemperatureCanaryRunInitializationsReadPlan,
    ) -> Result<Vec<JsDateOrString>, Self::Error>;
}

/// Worker repository backed by typed database persistence and canonical preparation ports.
pub struct DatabaseTemperatureCanaryRepository<Database, Preparation> {
    archive_mode: TemperatureCanaryRevisionArchiveMode,
    database: Database,
    preparation: Preparation,
}

impl<Database, Preparation> DatabaseTemperatureCanaryRepository<Database, Preparation> {
    /// Bind runtime owners without performing inventory or persistence work.
    pub const fn new(
        database: Database,
        preparation: Preparation,
        archive_mode: TemperatureCanaryRevisionArchiveMode,
    ) -> Self {
        Self {
            archive_mode,
            database,
            preparation,
        }
    }

    /// Inspect the database adapter after one collector execution.
    pub const fn database(&self) -> &Database {
        &self.database
    }

    /// Inspect canonical preparation evidence after one collector execution.
    pub const fn preparation(&self) -> &Preparation {
        &self.preparation
    }
}

impl<Database, Preparation> TemperatureCanaryRepository
    for DatabaseTemperatureCanaryRepository<Database, Preparation>
where
    Database: TemperatureCanaryPersistenceDatabasePort,
    Preparation: TemperatureCanaryPersistencePreparationPort,
{
    /// Normalize clocks before fetching existing runs in the bounded warmup window.
    fn list_run_initializations(
        &mut self,
        site_slug: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<String>, TaskFailure> {
        let (from, from_ms) = normalize_inventory_clock(&mut self.preparation, from)?;
        let (to, to_ms) = normalize_inventory_clock(&mut self.preparation, to)?;
        let plan = ecmwf_temperature_canary_run_initializations_read_plan(
            site_slug, from, from_ms, to, to_ms,
        )
        .map_err(|_| persistence_failure())?;
        let rows = self
            .database
            .read_temperature_canary_run_initializations(&plan)
            .map_err(|_| persistence_failure())?;
        rows.into_iter()
            // preserve order and the Node Date-or-zoned-string conversion
            .map(|row| match row {
                JsDateOrString::InvalidDate => Err(persistence_failure()),
                JsDateOrString::DateEpochMilliseconds(epoch) => self
                    .preparation
                    .utc_string(epoch)
                    .map_err(|_| persistence_failure()),
                JsDateOrString::String(units) => {
                    let clock = String::from_utf16(&units).map_err(|_| persistence_failure())?;
                    normalize_inventory_clock(&mut self.preparation, &clock)
                        .map(|(canonical, _)| canonical)
                }
            })
            .collect()
    }

    /// Prepare exact identities and execute one shared database transaction.
    fn persist_run(&mut self, run: &TemperatureCanaryPersistedRun) -> Result<(), TaskFailure> {
        let prepared = prepare_persistence(run, self.archive_mode, &mut self.preparation)?;
        let stored = match self.archive_mode {
            TemperatureCanaryRevisionArchiveMode::Disabled => {
                persist_ecmwf_temperature_canary_run(&prepared.input, &mut self.database)
            }
            TemperatureCanaryRevisionArchiveMode::Enabled => {
                let revision_window = prepared
                    .revision_window
                    .as_ref()
                    .ok_or_else(persistence_failure)?;
                persist_ecmwf_temperature_canary_run_with_revision_archive(
                    &prepared.input,
                    revision_window,
                    &mut self.database,
                )
            }
        }
        .map_err(|_| persistence_failure())?;
        validate_stored_run(run, &prepared.input, &stored)
    }
}

/// Normalize one explicit UTC or offset timestamp through the existing time owner.
fn normalize_inventory_clock(
    preparation: &mut impl TemperatureCanaryPersistencePreparationPort,
    value: &str,
) -> Result<(String, i64), TaskFailure> {
    let suffix = value.as_bytes().last_chunk::<6>();
    let has_offset = matches!(
        suffix,
        Some([b'+' | b'-', h1, h2, b':', m1, m2])
            if [h1, h2, m1, m2].into_iter().all(u8::is_ascii_digit)
    );
    // do not let the process time resolver infer a local timezone
    if !value.ends_with('Z') && !has_offset {
        return Err(persistence_failure());
    }
    let epoch = preparation
        .utc_epoch_milliseconds(value)
        .map_err(|_| persistence_failure())?;
    let canonical = preparation
        .utc_string(epoch)
        .map_err(|_| persistence_failure())?;
    Ok((canonical, epoch))
}

struct PreparedTemperatureCanaryPersistence {
    input: AdmittedEcmwfCanaryRunPersistence,
    revision_window: Option<AdmittedEcmwfCanaryRevisionWindow>,
}

/// Convert one collector-owned run through admitted time and digest boundaries.
fn prepare_persistence<Preparation: TemperatureCanaryPersistencePreparationPort>(
    run: &TemperatureCanaryPersistedRun,
    archive_mode: TemperatureCanaryRevisionArchiveMode,
    preparation: &mut Preparation,
) -> Result<PreparedTemperatureCanaryPersistence, TaskFailure> {
    let run_initialized_at_epoch_milliseconds =
        preparation.utc_epoch_milliseconds(&run.batch.run_initialized_at)?;
    let received_at_epoch_milliseconds =
        preparation.utc_epoch_milliseconds(&run.batch.received_at)?;
    let recent_error_state = generated_recent_error_state(run);
    let recent_error_state_json = preparation.serialize_recent_error_state(&recent_error_state)?;
    let recent_error_state_sha256 = preparation.digest_recent_error_state(&recent_error_state)?;
    let model_cycle = model_cycle(&run.batch.model_cycle)?;
    let state_status = state_status(run.state_status);
    let provider_response_sha256 =
        Sha256Digest::admitted(run.batch.provider_response_sha256.clone());
    let mut hours = Vec::with_capacity(run.batch.hours.len());

    // prepare each already validated normalized hour in source order
    for hour in &run.batch.hours {
        hours.push(PreparedEcmwfCanaryHour {
            content_hash: preparation.digest_hour(hour)?,
            model_lead_hours: u64::from(hour.model_lead_hours),
            raw_relative_humidity_percent: nullable_number(hour.raw_relative_humidity_percent),
            raw_temperature_c: hour.raw_temperature_c,
            raw_wind_speed_mps: nullable_number(hour.raw_wind_speed_mps),
            valid_at: hour.valid_at.clone(),
            valid_at_epoch_milliseconds: hour.valid_at_ms,
        });
    }
    let digest_material = TemperatureCanaryRunDigestMaterial {
        adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
        hour_content_hashes: hours.iter().map(|hour| hour.content_hash.clone()).collect(),
        model_cycle,
        provider_response_sha256: provider_response_sha256.clone(),
        recent_error_state_sha256: recent_error_state_sha256.clone(),
        run_initialized_at: run.batch.run_initialized_at.clone(),
        state_reason: run.state_status.reason().to_owned(),
        state_status,
        upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    };
    let content_hash = preparation.digest_run(&digest_material)?;
    let maximum_source_run_epoch_milliseconds = optional_epoch(
        preparation,
        run.recent_error_state
            .maximum_source_run_initialized_at
            .as_deref(),
    )?;
    let maximum_source_valid_epoch_milliseconds = optional_epoch(
        preparation,
        run.recent_error_state.maximum_source_valid_at.as_deref(),
    )?;
    let recent_error_state_window_end_epoch_milliseconds =
        preparation.utc_epoch_milliseconds(&run.recent_error_state.window_end_valid_at)?;
    let revision_window = match archive_mode {
        TemperatureCanaryRevisionArchiveMode::Disabled => None,
        TemperatureCanaryRevisionArchiveMode::Enabled => Some(revision_window(
            run,
            run_initialized_at_epoch_milliseconds,
            preparation,
        )?),
    };
    Ok(PreparedTemperatureCanaryPersistence {
        input: AdmittedEcmwfCanaryRunPersistence {
            adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
            content_hash,
            hours,
            model_cycle,
            provider_response_sha256,
            received_at: run.batch.received_at.clone(),
            received_at_epoch_milliseconds,
            recent_error_state,
            recent_error_state_json,
            recent_error_state_maximum_run_epoch_milliseconds:
                maximum_source_run_epoch_milliseconds,
            recent_error_state_maximum_valid_epoch_milliseconds:
                maximum_source_valid_epoch_milliseconds,
            recent_error_state_sha256,
            recent_error_state_window_end_epoch_milliseconds,
            recent_error_state_window_end_valid_at: run
                .recent_error_state
                .window_end_valid_at
                .clone(),
            run_initialized_at: run.batch.run_initialized_at.clone(),
            run_initialized_at_epoch_milliseconds,
            site_slug: run.site_slug.clone(),
            state_reason: run.state_status.reason().to_owned(),
            state_status,
            upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
        },
        revision_window,
    })
}

/// Build the source-owned archive window only when an archiver is configured.
fn revision_window<Preparation: TemperatureCanaryPersistencePreparationPort>(
    run: &TemperatureCanaryPersistedRun,
    run_initialized_at_epoch_milliseconds: i64,
    preparation: &mut Preparation,
) -> Result<AdmittedEcmwfCanaryRevisionWindow, TaskFailure> {
    let from_epoch_milliseconds = run_initialized_at_epoch_milliseconds
        .checked_add(7 * HOUR_MILLISECONDS)
        .ok_or_else(persistence_failure)?;
    let to_epoch_milliseconds = run_initialized_at_epoch_milliseconds
        .checked_add(19 * HOUR_MILLISECONDS)
        .ok_or_else(persistence_failure)?;
    let from = run
        .batch
        .hours
        .get(6)
        .ok_or_else(persistence_failure)?
        .valid_at
        .clone();
    let to = preparation.utc_string(to_epoch_milliseconds)?;
    Ok(AdmittedEcmwfCanaryRevisionWindow::admitted(
        from,
        from_epoch_milliseconds,
        to,
        to_epoch_milliseconds,
    ))
}

/// Convert the worker state into its generated canonical contract.
fn generated_recent_error_state(
    run: &TemperatureCanaryPersistedRun,
) -> EcmwfTemperatureCanaryRecentErrorState {
    EcmwfTemperatureCanaryRecentErrorState {
        b24C: nullable_number(run.recent_error_state.b24_c),
        b72C: nullable_number(run.recent_error_state.b72_c),
        cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
        localDates: run.recent_error_state.local_dates,
        mad72C: nullable_number(run.recent_error_state.mad72_c),
        maximumSourceRunInitializedAt: nullable_string(
            run.recent_error_state
                .maximum_source_run_initialized_at
                .clone(),
        ),
        maximumSourceValidAt: nullable_string(
            run.recent_error_state.maximum_source_valid_at.clone(),
        ),
        n24: run.recent_error_state.n24,
        n72: run.recent_error_state.n72,
        sourceKeys: run.recent_error_state.source_keys.clone(),
        supported: run.recent_error_state.supported,
        targetRunInitializedAt: run.recent_error_state.target_run_initialized_at.clone(),
        windowEndValidAt: run.recent_error_state.window_end_valid_at.clone(),
    }
}

/// Parse one optional canonical instant through the injected time owner.
fn optional_epoch<Preparation: TemperatureCanaryPersistencePreparationPort>(
    preparation: &mut Preparation,
    value: Option<&str>,
) -> Result<Option<i64>, TaskFailure> {
    value
        .map(|value| preparation.utc_epoch_milliseconds(value))
        .transpose()
}

/// Map one collector model-cycle value into the generated canonical enum.
fn model_cycle(value: &str) -> Result<EcmwfTemperatureCanaryRunModelCycleEnum, TaskFailure> {
    // preserve only the two source-validated ECMWF eras
    match value {
        "49r1" => Ok(EcmwfTemperatureCanaryRunModelCycleEnum::Value49r1),
        "50r1" => Ok(EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1),
        _ => Err(persistence_failure()),
    }
}

/// Map one collector status into the generated database status.
const fn state_status(
    value: TemperatureCanaryStateStatus,
) -> EcmwfTemperatureCanaryRunStateStatusEnum {
    // retain exact worker status vocabulary
    match value {
        TemperatureCanaryStateStatus::Supported => {
            EcmwfTemperatureCanaryRunStateStatusEnum::Supported
        }
        TemperatureCanaryStateStatus::Cold => EcmwfTemperatureCanaryRunStateStatusEnum::Cold,
        TemperatureCanaryStateStatus::Insufficient => {
            EcmwfTemperatureCanaryRunStateStatusEnum::Insufficient
        }
    }
}

/// Map one generated stored status back to the collector vocabulary.
fn returned_state_status(
    value: EcmwfTemperatureCanaryRunStateStatusEnum,
) -> Result<TemperatureCanaryStateStatus, TaskFailure> {
    // reject the stored-only invalid status at this collector boundary
    match value {
        EcmwfTemperatureCanaryRunStateStatusEnum::Supported => {
            Ok(TemperatureCanaryStateStatus::Supported)
        }
        EcmwfTemperatureCanaryRunStateStatusEnum::Cold => Ok(TemperatureCanaryStateStatus::Cold),
        EcmwfTemperatureCanaryRunStateStatusEnum::Insufficient => {
            Ok(TemperatureCanaryStateStatus::Insufficient)
        }
        EcmwfTemperatureCanaryRunStateStatusEnum::Invalid => Err(persistence_failure()),
    }
}

/// Confirm the stored result still represents the collector's admitted run.
fn validate_stored_run(
    expected: &TemperatureCanaryPersistedRun,
    input: &AdmittedEcmwfCanaryRunPersistence,
    stored: &EcmwfTemperatureCanaryRun,
) -> Result<(), TaskFailure> {
    // reject an adapter returning a different immutable run or status
    if stored.adapterVersion != input.adapter_version
        || stored.modelCycle != input.model_cycle
        || stored.providerResponseSha256 != input.provider_response_sha256.as_str()
        || stored.recentErrorState != input.recent_error_state
        || stored.runInitializedAt != input.run_initialized_at
        || returned_state_status(stored.stateStatus)? != expected.state_status
        || stored.upstreamModel != input.upstream_model
    {
        return Err(persistence_failure());
    }
    Ok(())
}

/// Convert one optional number into the generated nullable shape.
fn nullable_number(value: Option<f64>) -> Nullable<f64> {
    value.map_or(Nullable::Null, Nullable::Value)
}

/// Convert one optional string into the generated nullable shape.
fn nullable_string(value: Option<String>) -> Nullable<String> {
    value.map_or(Nullable::Null, Nullable::Value)
}

/// Build one stable worker-facing persistence failure.
fn persistence_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "temperature_canary_persistence_failed".to_owned(),
    }
}
