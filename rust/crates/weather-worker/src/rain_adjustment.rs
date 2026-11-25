use std::cmp::Reverse;

use weather_adjustment::{
    AdmittedRainHurdleWindCalendar, AdmittedRainHurdleWindFeatureInput,
    AdmittedRainHurdleWindStations, RETAINED_RAIN_MODEL_MONTH, RETAINED_RAIN_MODEL_SHA256,
    RainHurdleWindInferenceError, RainHurdleWindV1Model, build_admitted_rain_hurdle_wind_features,
};
use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptProjectionKindEnum,
    AdjustmentRevisionProjectionKind, AdjustmentRevisionStageReceipt, Nullable,
    RainHurdleWindFeatureProjection, RainHurdleWindFeatureRow,
    RainHurdleWindPortableArtifactV1Heads, RainHurdleWindPortableArtifactV1Rules,
    RainWindPerformanceHour, RainWindPerformanceHourOccurrenceProbabilities,
    RainWindPerformanceResult, RainWindPredictionHourReasonCodeEnum, RainWindPredictionInput,
    RainWindRunProfile, RainWindStationHour,
};

use crate::{RainAdjustmentRuntimeSelection, TaskFailure};

const DECISION_DELAY_MILLISECONDS: i64 = 8 * 3_600_000;
const OUTPUT_ROW_COUNT: usize = 23;
const FIRST_MODEL_LEAD_HOURS: usize = 9;
const FORECAST_HOUR_COUNT: usize = 48;
const STATION_COUNT: usize = 12;
const STATION_IDS: [u64; STATION_COUNT] = [
    64_255, 225_947, 38_270, 168_853, 126_537, 201_058, 203_055, 66_270, 34_768, 88_159, 126_197,
    27_140,
];
const STATION_LAGS_HOURS: [i64; 6] = [1, 2, 3, 6, 12, 24];

/// Fixed station arrays admitted for one decision boundary.
struct RainAdjustmentStationAdmission {
    precipitation_by_lag: [[Nullable<f64>; STATION_COUNT]; 6],
    temperature_lag1: [Nullable<f64>; STATION_COUNT],
}

/// One immutable capture address retained by an inference input hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainAdjustmentCaptureIdentity {
    pub body_sha256: String,
    pub claim_id: String,
    pub completed_at: String,
    pub completed_at_ms: i64,
}

/// One already-decoded capture admitted after body hash and codec checks.
#[derive(Clone, Debug, PartialEq)]
pub enum RainAdjustmentCaptureKind {
    Forecast {
        profile: RainWindRunProfile,
        run_initialized_at_ms: i64,
    },
    Station {
        station_id: u64,
        window_start: String,
        window_end_exclusive: String,
    },
}

/// One pending capture with its durable identity and typed decoded content.
#[derive(Clone, Debug, PartialEq)]
pub struct RainAdjustmentCapture {
    pub identity: RainAdjustmentCaptureIdentity,
    pub kind: RainAdjustmentCaptureKind,
}

/// Exact ready inputs selected by the repository's existing causal query.
#[derive(Clone, Debug, PartialEq)]
pub struct RainAdjustmentPendingInput {
    pub captures: Vec<RainAdjustmentCapture>,
    pub station_hours: Vec<RainAdjustmentStationHour>,
}

/// One station hour with admitted parsed clocks retained beside generated data.
#[derive(Clone, Debug, PartialEq)]
pub struct RainAdjustmentStationHour {
    pub hour_at_ms: i64,
    pub received_at_ms: i64,
    pub row: RainWindStationHour,
}

/// One immutable model identity and its inference tables.
#[derive(Clone, Copy)]
pub struct RainAdjustmentModel<'a> {
    tables: RainAdjustmentModelTables<'a>,
    model_month: &'a str,
    model_sha256: &'a str,
}

/// Distinguish build-validated tables from provided typed artifacts.
#[derive(Clone, Copy)]
enum RainAdjustmentModelTables<'a> {
    Retained,
    Provided {
        category_scales: &'a [f64],
        heads: &'a RainHurdleWindPortableArtifactV1Heads,
        rules: &'a [RainHurdleWindPortableArtifactV1Rules],
    },
}

impl RainAdjustmentModel<'static> {
    /// Use the original model and its build-bound identity.
    pub const fn retained() -> Self {
        Self {
            tables: RainAdjustmentModelTables::Retained,
            model_month: RETAINED_RAIN_MODEL_MONTH,
            model_sha256: RETAINED_RAIN_MODEL_SHA256,
        }
    }
}

impl<'a> RainAdjustmentModel<'a> {
    /// Keep provided tables on the existing checked scoring path.
    pub const fn from_typed(
        heads: &'a RainHurdleWindPortableArtifactV1Heads,
        rules: &'a [RainHurdleWindPortableArtifactV1Rules],
        category_scales: &'a [f64],
        model_month: &'a str,
        model_sha256: &'a str,
    ) -> Self {
        Self {
            tables: RainAdjustmentModelTables::Provided {
                category_scales,
                heads,
                rules,
            },
            model_month,
            model_sha256,
        }
    }

    /// Validate provided tables but borrow build-validated tables directly.
    fn scorer(self) -> Result<RainHurdleWindV1Model<'a>, RainHurdleWindInferenceError> {
        match self.tables {
            RainAdjustmentModelTables::Retained => Ok(RainHurdleWindV1Model::retained()),
            RainAdjustmentModelTables::Provided {
                category_scales,
                heads,
                rules,
            } => RainHurdleWindV1Model::new(heads, rules, category_scales),
        }
    }
}

/// One serving reason persisted with a rain-adjustment hour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RainAdjustmentHourReason {
    PhaseUnsupported,
    PredictionInvalid,
    PolicyRaw,
}

impl RainAdjustmentHourReason {
    /// Return the exact TypeScript persistence value.
    pub const fn code(self) -> &'static str {
        // map typed outcomes to the existing database representation
        match self {
            Self::PhaseUnsupported => "phase_unsupported",
            Self::PredictionInvalid => "prediction_invalid",
            Self::PolicyRaw => "policy_raw",
        }
    }
}

/// One public-safe serving hour produced by the frozen rain model.
#[derive(Clone, Debug, PartialEq)]
pub struct RainAdjustmentHour {
    pub applied: bool,
    pub corrected_precipitation_mm: f64,
    pub model_lead_hours: f64,
    pub raw_precipitation_mm: f64,
    pub reason: Option<RainAdjustmentHourReason>,
    pub valid_at: String,
}

/// One immutable serving run passed to the database transaction.
#[derive(Clone, Debug, PartialEq)]
pub struct RainAdjustmentRun {
    pub decision_at: String,
    pub first_received_at: String,
    pub forecast_claim_id: String,
    pub generated_at: String,
    pub hours: Vec<RainAdjustmentHour>,
    pub input_sha256: String,
    pub model_sha256: String,
    pub run_initialized_at: String,
}

/// One fully evaluated source set retained for revision encoding.
#[derive(Clone, Debug, PartialEq)]
pub struct RainAdjustmentEvaluation {
    pub causal_input: RainWindPredictionInput,
    pub captures: Vec<RainAdjustmentCaptureIdentity>,
    pub current_claim_id: String,
    pub feature_projection: RainHurdleWindFeatureProjection,
    pub incumbent_performance: RainWindPerformanceResult,
    pub run: RainAdjustmentRun,
}

/// One persisted serving revision returned before transaction commit.
#[derive(Clone, Debug, PartialEq)]
pub struct RainAdjustmentPersistedRevision {
    pub generated_at: String,
    pub hours: Vec<RainAdjustmentHour>,
    pub input_sha256: String,
    pub model_sha256: String,
    pub run_initialized_at: String,
    pub stored_content_sha256: String,
}

/// One checked immutable revision projection produced by the admitted codec.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainAdjustmentProjection {
    pub bytes: Vec<u8>,
    pub logical_key_sha256: String,
    pub projection_identity_sha256: String,
}

/// Keep the complete original durable archive receipt for the private relay.
pub type RainAdjustmentStageReceipt = AdjustmentRevisionStageReceipt;

/// Keep the original database receipt, including its frontier and ordinal, for publication.
pub type RainAdjustmentCommitReceipt = AdjustmentRevisionCommitReceipt;

/// One permanent revision archival category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RainAdjustmentGapReason {
    ArchiveStageFailed,
    DatabaseBindFailed,
    ArchivePublishFailed,
}

impl RainAdjustmentGapReason {
    /// Return the exact Node gap category.
    pub const fn code(self) -> &'static str {
        // preserve the existing durable reason vocabulary
        match self {
            Self::ArchiveStageFailed => "archive_stage_failed",
            Self::DatabaseBindFailed => "database_bind_failed",
            Self::ArchivePublishFailed => "archive_publish_failed",
        }
    }
}

/// One revision gap bound to its logical and optional encoded identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainAdjustmentGap {
    pub logical_key_sha256: String,
    pub projection_identity_sha256: Option<String>,
    pub projection_sha256: Option<String>,
    pub reason: RainAdjustmentGapReason,
}

/// One encoded revision plan ready for archive staging.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainAdjustmentRevisionPlan {
    pub projection: RainAdjustmentProjection,
}

/// Opaque open serving transaction owned by the database adapter.
#[derive(Debug)]
pub struct RainAdjustmentAppendTransaction<Transaction> {
    pub persisted: RainAdjustmentPersistedRevision,
    pub transaction: Transaction,
}

/// Typed date formatting admitted at the runtime boundary.
pub trait RainAdjustmentTimeAdapter {
    /// Format one already-validated epoch millisecond as canonical UTC.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure>;
}

/// Admitted calendar and fixed station geometry required by the pure feature owner.
pub trait RainAdjustmentFeatureAdmission {
    /// Resolve exact UTC harmonics for one already-parsed valid-time epoch.
    fn calendar(&mut self, valid_at_ms: i64)
    -> Result<AdmittedRainHurdleWindCalendar, TaskFailure>;

    /// Return the fixed positive inverse-distance station weights in catalog order.
    fn station_weights(&mut self) -> Result<[f64; STATION_COUNT], TaskFailure>;
}

/// Hash checks delegated to the admitted runtime crypto boundary.
pub trait RainAdjustmentIdentityAdapter {
    /// Derive the exact ordered capture-member input identity.
    fn input_sha256(
        &mut self,
        captures: &[RainAdjustmentCaptureIdentity],
    ) -> Result<String, TaskFailure>;

    /// Verify one durable stage receipt without implementing crypto here.
    fn stage_receipt_is_authentic(&mut self, receipt: &RainAdjustmentStageReceipt) -> bool;

    /// Verify one database commit receipt without implementing crypto here.
    fn commit_receipt_is_authentic(&mut self, receipt: &RainAdjustmentCommitReceipt) -> bool;
}

/// Database effects required by the serving append transaction.
pub trait RainAdjustmentRepository {
    type Transaction;

    /// Select one mature not-yet-evaluated source graph for this model.
    fn read_pending(
        &mut self,
        model_sha256: &str,
    ) -> Result<RainAdjustmentPendingInput, TaskFailure>;

    /// Insert one serving row without committing and return no transaction on conflict.
    fn begin_append(
        &mut self,
        run: &RainAdjustmentRun,
    ) -> Result<Option<RainAdjustmentAppendTransaction<Self::Transaction>>, TaskFailure>;

    /// Bind one staged revision in an internal savepoint of the serving transaction.
    fn bind_revision(
        &mut self,
        transaction: &mut Self::Transaction,
        persisted: &RainAdjustmentPersistedRevision,
        plan: &RainAdjustmentRevisionPlan,
        stage_receipt: &RainAdjustmentStageReceipt,
    ) -> Result<RainAdjustmentCommitReceipt, TaskFailure>;

    /// Persist one permanent gap inside the serving transaction.
    fn mark_revision_gap_in_transaction(
        &mut self,
        transaction: &mut Self::Transaction,
        persisted: &RainAdjustmentPersistedRevision,
        gap: &RainAdjustmentGap,
    ) -> Result<(), TaskFailure>;

    /// Commit one serving transaction after optional revision custody.
    fn commit_append(&mut self, transaction: &mut Self::Transaction) -> Result<(), TaskFailure>;

    /// Roll back one failed serving transaction without replacing the primary error.
    fn rollback_append(&mut self, transaction: &mut Self::Transaction);

    /// Persist one post-commit archive-publication gap.
    fn mark_revision_gap_after_commit(
        &mut self,
        persisted: &RainAdjustmentPersistedRevision,
        gap: &RainAdjustmentGap,
    ) -> Result<(), TaskFailure>;
}

/// Checked capture-epoch and canonical revision encoding boundary.
pub trait RainAdjustmentProjectionCodec {
    /// Derive the exact rain-gate logical key before projection encoding.
    fn logical_key_sha256(
        &mut self,
        persisted: &RainAdjustmentPersistedRevision,
    ) -> Result<String, TaskFailure>;

    /// Build exact revision bytes and identities from the immutable persisted row.
    fn build_revision(
        &mut self,
        pending: &RainAdjustmentPendingInput,
        evaluation: &RainAdjustmentEvaluation,
        persisted: &RainAdjustmentPersistedRevision,
        logical_key_sha256: &str,
        time: &mut impl RainAdjustmentTimeAdapter,
    ) -> Result<RainAdjustmentRevisionPlan, TaskFailure>;

    /// Settle any auxiliary custody once projection staging succeeds or fails.
    fn finish_projection_stage(&mut self, _durable: bool) {}
}

/// Archive staging, publication, and gap effects.
pub trait RainAdjustmentClient {
    /// Stage one checked canonical projection before database binding.
    fn stage_revision(
        &mut self,
        projection: &RainAdjustmentProjection,
    ) -> Result<RainAdjustmentStageReceipt, TaskFailure>;

    /// Publish only after the serving transaction commits.
    fn publish_revision(
        &mut self,
        projection: &RainAdjustmentProjection,
        stage_receipt: &RainAdjustmentStageReceipt,
        commit_receipt: &RainAdjustmentCommitReceipt,
    ) -> Result<(), TaskFailure>;

    /// Best-effort archive one gap already persisted by the database.
    fn record_revision_gap(&mut self, gap: &RainAdjustmentGap) -> Result<(), TaskFailure>;
}

/// Optional checked revision effects for one serving append.
pub struct RainAdjustmentRevisionExecution<'a, Codec, Client> {
    pub client: &'a mut Client,
    pub codec: &'a mut Codec,
}

/// One cohesive inference-publication execution.
pub struct RainAdjustmentExecution<'a, Repository, Admission, Time, Identity, Codec, Client> {
    pub admission: &'a mut Admission,
    pub identity: &'a mut Identity,
    pub model: RainAdjustmentModel<'a>,
    pub repository: &'a mut Repository,
    pub revision: Option<RainAdjustmentRevisionExecution<'a, Codec, Client>>,
    pub runtime: RainAdjustmentRuntimeSelection,
    pub time: &'a mut Time,
}

// build one stable rain-adjustment failure
fn adjustment_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

// require one lowercase hexadecimal sha-256 identity
fn sha256_is_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

// validate one forecast capture against its admitted decoded profile
fn validate_forecast_capture(capture: &RainAdjustmentCapture) -> Result<(), TaskFailure> {
    // require every ordered hash member to have an immutable durable address
    if capture.identity.claim_id.is_empty()
        || capture.identity.completed_at.is_empty()
        || !sha256_is_valid(&capture.identity.body_sha256)
    {
        return Err(adjustment_failure("rain capture identity differs"));
    }
    let RainAdjustmentCaptureKind::Forecast {
        profile,
        run_initialized_at_ms: _,
    } = &capture.kind
    else {
        return Ok(());
    };

    // bind the decoded source profile to its immutable receipt identity
    if capture.identity.completed_at != profile.completedAt || profile.runInitializedAt.is_empty() {
        return Err(adjustment_failure("rain forecast capture differs"));
    }
    Ok(())
}

// select current and exact ordered prior profiles from admitted captures
fn prediction_input<T: RainAdjustmentTimeAdapter>(
    pending: &RainAdjustmentPendingInput,
    now_ms: i64,
    time: &mut T,
) -> Result<Option<(RainWindPredictionInput, RainAdjustmentCaptureIdentity, i64)>, TaskFailure> {
    let mut forecasts = pending
        .captures
        .iter()
        .filter_map(|capture| match &capture.kind {
            RainAdjustmentCaptureKind::Forecast {
                profile,
                run_initialized_at_ms,
            } => Some((*run_initialized_at_ms, capture, profile.clone())),
            RainAdjustmentCaptureKind::Station { .. } => None,
        })
        .collect::<Vec<_>>();
    // select the newest initialized forecast before exact prior cycles
    forecasts.sort_by_key(|capture| Reverse(capture.0));

    // bind every admitted parsed epoch to the exact profile timestamp
    for (initialized_at_ms, _, profile) in &forecasts {
        if time.utc_string(*initialized_at_ms)? != profile.runInitializedAt {
            return Err(adjustment_failure("rain forecast initialization differs"));
        }
    }

    // preserve missing mature forecast work as an idle sidecar
    let Some((initialized_at_ms, current_capture, current_profile)) = forecasts.first().cloned()
    else {
        return Ok(None);
    };
    let decision_at_ms = initialized_at_ms
        .checked_add(DECISION_DELAY_MILLISECONDS)
        .ok_or_else(|| adjustment_failure("rain decision clock overflowed"))?;

    // require repository maturity before model or serving effects
    if decision_at_ms > now_ms {
        return Err(adjustment_failure("rain runtime decision has not matured"));
    }

    // require every retained forecast receipt to be causal and one exact prior cycle
    for (forecast_initialized_at_ms, capture, _) in &forecasts {
        let lag_ms = initialized_at_ms
            .checked_sub(*forecast_initialized_at_ms)
            .ok_or_else(|| adjustment_failure("rain runtime prior cycle invalid"))?;
        if capture.identity.completed_at_ms > decision_at_ms
            || *forecast_initialized_at_ms > capture.identity.completed_at_ms
            || ![0, 6 * 3_600_000, 12 * 3_600_000].contains(&lag_ms)
        {
            return Err(adjustment_failure("rain runtime prior cycle invalid"));
        }
    }
    if forecasts.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(adjustment_failure("rain runtime prior cycle invalid"));
    }
    let now_utc = time.utc_string(now_ms)?;
    let prior_runs = forecasts
        .into_iter()
        .skip(1)
        .map(|(_, _, profile)| profile)
        .collect();
    Ok(Some((
        RainWindPredictionInput {
            currentRun: current_profile,
            priorRuns: prior_runs,
            stationHours: pending
                .station_hours
                .iter()
                .map(|hour| hour.row.clone())
                .collect(),
            nowUtc: now_utc,
        },
        current_capture.identity.clone(),
        initialized_at_ms,
    )))
}

// convert one exact generated profile into the feature owner's fixed geometry
fn forecast_array(
    profile: &RainWindRunProfile,
) -> Result<[weather_contracts::RainWindForecastHour; FORECAST_HOUR_COUNT], TaskFailure> {
    profile
        .hours
        .clone()
        .try_into()
        .map_err(|_| adjustment_failure("rain runtime forecast receipt unavailable at decision"))
}

// bind station values to the six frozen decision-time lags and catalog order
fn station_admission<T: RainAdjustmentTimeAdapter>(
    pending: &RainAdjustmentPendingInput,
    decision_at_ms: i64,
    time: &mut T,
) -> Result<RainAdjustmentStationAdmission, TaskFailure> {
    let latest_hour_at_ms = decision_at_ms
        .checked_sub(3_600_000)
        .ok_or_else(|| adjustment_failure("rain runtime station clock overflowed"))?;
    let mut precipitation = std::array::from_fn(|_| std::array::from_fn(|_| Nullable::Null));
    let mut temperature_lag1 = std::array::from_fn(|_| Nullable::Null);
    let mut seen = std::collections::BTreeSet::new();

    // validate all admitted rows before selecting the six consumed lag clocks
    for admitted in &pending.station_hours {
        let station_id = admitted.row.stationId;
        let station_index = STATION_IDS
            .iter()
            .position(|expected| *expected as f64 == station_id)
            .ok_or_else(|| {
                adjustment_failure("rain runtime station value unavailable at decision")
            })?;
        if admitted.row.hourAt != time.utc_string(admitted.hour_at_ms)?
            || admitted.row.receivedAt != time.utc_string(admitted.received_at_ms)?
            || admitted.received_at_ms > decision_at_ms
            || admitted.hour_at_ms > latest_hour_at_ms
            || admitted.hour_at_ms > admitted.received_at_ms
            || !seen.insert((admitted.hour_at_ms, STATION_IDS[station_index]))
        {
            return Err(adjustment_failure(
                "rain runtime station value unavailable at decision",
            ));
        }
        let precipitation_value = match admitted.row.precipitationMm {
            Nullable::Null => Nullable::Null,
            Nullable::Value(value) if value.is_finite() && (0.0..=500.0).contains(&value) => {
                Nullable::Value(value)
            }
            Nullable::Value(_) => {
                return Err(adjustment_failure("rain runtime physical field invalid"));
            }
        };
        let temperature_value = match admitted.row.temperatureC {
            Nullable::Null => Nullable::Null,
            Nullable::Value(value) if value.is_finite() && (-80.0..=60.0).contains(&value) => {
                Nullable::Value(value)
            }
            Nullable::Value(_) => {
                return Err(adjustment_failure("rain runtime physical field invalid"));
            }
        };

        // retain only exact lag targets while still validating all causal rows
        for (lag_index, lag_hours) in STATION_LAGS_HOURS.iter().enumerate() {
            let lag_at_ms = decision_at_ms
                .checked_sub(lag_hours * 3_600_000)
                .ok_or_else(|| adjustment_failure("rain runtime station clock overflowed"))?;
            // project values only into their exact frozen lag bucket
            if admitted.hour_at_ms == lag_at_ms {
                precipitation[lag_index][station_index] = precipitation_value.clone();
                // expose temperature only for the model's lag-one support gate
                if lag_index == 0 {
                    temperature_lag1[station_index] = temperature_value.clone();
                }
            }
        }
    }
    Ok(RainAdjustmentStationAdmission {
        precipitation_by_lag: precipitation,
        temperature_lag1,
    })
}

// require one present finite forecast source value in its physical range
fn forecast_value(value: &Nullable<f64>, minimum: f64, maximum: f64) -> Result<f64, TaskFailure> {
    match value {
        Nullable::Value(value) if value.is_finite() && (minimum..=maximum).contains(value) => {
            Ok(*value)
        }
        Nullable::Null | Nullable::Value(_) => {
            Err(adjustment_failure("rain runtime physical field invalid"))
        }
    }
}

// build and score the complete actual feature projection from selected profiles and stations
fn build_and_score_projection<Admission, Time>(
    pending: &RainAdjustmentPendingInput,
    input: &RainWindPredictionInput,
    initialized_at_ms: i64,
    decision_at: String,
    model: RainAdjustmentModel<'_>,
    admission: &mut Admission,
    time: &mut Time,
) -> Result<
    (
        RainHurdleWindFeatureProjection,
        Vec<RainAdjustmentHour>,
        RainWindPerformanceResult,
    ),
    TaskFailure,
>
where
    Admission: RainAdjustmentFeatureAdmission,
    Time: RainAdjustmentTimeAdapter,
{
    // reject unbound runtime model identities before constructing the scorer
    if !sha256_is_valid(model.model_sha256) || model.model_month.is_empty() {
        return Err(adjustment_failure("rain model identity differs"));
    }
    let scorer = model
        .scorer()
        .map_err(|_| adjustment_failure("rain model tables differ"))?;
    let current = forecast_array(&input.currentRun)?;
    let prior_6h_ms = initialized_at_ms
        .checked_sub(6 * 3_600_000)
        .ok_or_else(|| adjustment_failure("rain runtime prior cycle invalid"))?;
    let prior_12h_ms = initialized_at_ms
        .checked_sub(12 * 3_600_000)
        .ok_or_else(|| adjustment_failure("rain runtime prior cycle invalid"))?;
    let prior_6h_at = time.utc_string(prior_6h_ms)?;
    let prior_12h_at = time.utc_string(prior_12h_ms)?;
    let prior_6h = input
        .priorRuns
        .iter()
        .find(|profile| profile.runInitializedAt == prior_6h_at)
        .map(forecast_array)
        .transpose()?;
    let prior_12h = input
        .priorRuns
        .iter()
        .find(|profile| profile.runInitializedAt == prior_12h_at)
        .map(forecast_array)
        .transpose()?;
    let decision_at_ms = initialized_at_ms
        .checked_add(DECISION_DELAY_MILLISECONDS)
        .ok_or_else(|| adjustment_failure("rain decision clock overflowed"))?;
    let station_values = station_admission(pending, decision_at_ms, time)?;
    let station_weights = admission.station_weights()?;
    let lag_refs = std::array::from_fn(|index| &station_values.precipitation_by_lag[index]);
    let stations = AdmittedRainHurdleWindStations {
        precipitation_by_lag: lag_refs,
        station_weights: &station_weights,
        temperature_lag1: &station_values.temperature_lag1,
    };
    let mut rows = Vec::with_capacity(OUTPUT_ROW_COUNT);
    let mut hours = Vec::with_capacity(OUTPUT_ROW_COUNT);
    let mut performance_hours = Vec::with_capacity(OUTPUT_ROW_COUNT);

    // preserve all 23 fitted output leads and use the adjustment feature owner directly
    for output_lead in 1..=OUTPUT_ROW_COUNT {
        let source_lead = FIRST_MODEL_LEAD_HOURS + output_lead - 1;
        let source_hour = &current[source_lead - 1];
        let raw = forecast_value(&source_hour.precipitationMm, 0.0, 2_000.0)?;
        let temperature = forecast_value(&source_hour.temperatureC, -100.0, 70.0)?;
        let valid_at_ms = initialized_at_ms
            .checked_add(source_lead as i64 * 3_600_000)
            .ok_or_else(|| adjustment_failure("rain output clock overflowed"))?;
        let valid_at = time.utc_string(valid_at_ms)?;
        let admitted = AdmittedRainHurdleWindFeatureInput {
            calendar: admission.calendar(valid_at_ms)?,
            current_hours: &current,
            model_lead_hours: output_lead as u8,
            prior_12h_hours: prior_12h.as_ref(),
            prior_6h_hours: prior_6h.as_ref(),
            stations,
        };
        // retain phase rows without inference and build warm rows once with their prediction
        let (features, prediction) = if temperature <= 2.0 {
            (
                build_admitted_rain_hurdle_wind_features(admitted)
                    .map_err(|_| adjustment_failure("rain feature projection differs"))?,
                None,
            )
        } else {
            match scorer.build_and_predict_admitted(admitted) {
                Ok((features, prediction)) => (features, Some(prediction)),
                Err(_) => (
                    // distinguish a soft scorer failure from a hard projection failure
                    build_admitted_rain_hurdle_wind_features(admitted)
                        .map_err(|_| adjustment_failure("rain feature projection differs"))?,
                    None,
                ),
            }
        };
        let row = RainHurdleWindFeatureRow {
            features: features.to_vec(),
            modelLeadHours: source_lead as f64,
            rawPrecipitationMm: raw,
            rawTargetHourTemperatureC: temperature,
            validAt: valid_at.clone(),
        };
        let (hour, performance_hour) = if temperature <= 2.0 {
            (
                RainAdjustmentHour {
                    applied: false,
                    corrected_precipitation_mm: raw,
                    model_lead_hours: source_lead as f64,
                    raw_precipitation_mm: raw,
                    reason: Some(RainAdjustmentHourReason::PhaseUnsupported),
                    valid_at: valid_at.clone(),
                },
                RainWindPerformanceHour {
                    validAt: valid_at,
                    modelLeadHours: source_lead as f64,
                    rawPrecipitationMm: raw,
                    correctedPrecipitationMm: raw,
                    applied: false,
                    reasonCode: Nullable::Value(
                        RainWindPredictionHourReasonCodeEnum::PhaseUnsupported,
                    ),
                    occurrenceProbabilities: Nullable::Null,
                    positiveAmountMm: Nullable::Null,
                },
            )
        } else {
            // contain one invalid row without abandoning other fitted hours
            match prediction {
                Some(prediction) => (
                    RainAdjustmentHour {
                        applied: true,
                        corrected_precipitation_mm: prediction.correctedPrecipitationMm,
                        model_lead_hours: source_lead as f64,
                        raw_precipitation_mm: raw,
                        reason: None,
                        valid_at: valid_at.clone(),
                    },
                    RainWindPerformanceHour {
                        validAt: valid_at,
                        modelLeadHours: source_lead as f64,
                        rawPrecipitationMm: raw,
                        correctedPrecipitationMm: prediction.correctedPrecipitationMm,
                        applied: true,
                        reasonCode: Nullable::Null,
                        occurrenceProbabilities: Nullable::Value(
                            RainWindPerformanceHourOccurrenceProbabilities {
                                atLeast0_1: prediction.occurrenceProbabilityAtLeast0_1,
                                atLeast1_0: prediction.occurrenceProbabilityAtLeast1_0,
                                atLeast2_5: prediction.occurrenceProbabilityAtLeast2_5,
                            },
                        ),
                        positiveAmountMm: Nullable::Value(prediction.positiveAmountMm),
                    },
                ),
                None => (
                    RainAdjustmentHour {
                        applied: false,
                        corrected_precipitation_mm: raw,
                        model_lead_hours: source_lead as f64,
                        raw_precipitation_mm: raw,
                        reason: Some(RainAdjustmentHourReason::PredictionInvalid),
                        valid_at: valid_at.clone(),
                    },
                    RainWindPerformanceHour {
                        validAt: valid_at,
                        modelLeadHours: source_lead as f64,
                        rawPrecipitationMm: raw,
                        correctedPrecipitationMm: raw,
                        applied: false,
                        reasonCode: Nullable::Value(
                            RainWindPredictionHourReasonCodeEnum::PredictionInvalid,
                        ),
                        occurrenceProbabilities: Nullable::Null,
                        positiveAmountMm: Nullable::Null,
                    },
                ),
            }
        };
        rows.push(row);
        hours.push(hour);
        performance_hours.push(performance_hour);
    }
    Ok((
        RainHurdleWindFeatureProjection {
            decisionAt: decision_at.clone(),
            rows,
            runInitializedAt: input.currentRun.runInitializedAt.clone(),
        },
        hours,
        RainWindPerformanceResult {
            modelSha256: model.model_sha256.to_owned(),
            modelMonth: model.model_month.to_owned(),
            decisionAt: decision_at,
            hours: performance_hours,
        },
    ))
}

/// Replay one saved source claim through the build-retained rain model without serving effects.
pub fn replay_retained_rain_performance(
    pending: &RainAdjustmentPendingInput,
    expected_claim_id: &str,
    decision_at_ms: i64,
    admission: &mut impl RainAdjustmentFeatureAdmission,
    time: &mut impl RainAdjustmentTimeAdapter,
) -> Result<RainWindPerformanceResult, TaskFailure> {
    let Some((input, current, initialized_at_ms)) =
        prediction_input(pending, decision_at_ms, time)?
    else {
        return Err(adjustment_failure(
            "rain archive source forecast unavailable",
        ));
    };
    // bind replay to the immutable claim saved by the serving run
    if current.claim_id != expected_claim_id {
        return Err(adjustment_failure("rain archive source claim differs"));
    }
    let expected_decision_at_ms = initialized_at_ms
        .checked_add(DECISION_DELAY_MILLISECONDS)
        .ok_or_else(|| adjustment_failure("rain decision clock overflowed"))?;
    // refuse a later maturity clock from changing the saved model decision
    if expected_decision_at_ms != decision_at_ms {
        return Err(adjustment_failure("rain archive decision clock differs"));
    }
    let decision_at = time.utc_string(decision_at_ms)?;
    let (_, _, performance) = build_and_score_projection(
        pending,
        &input,
        initialized_at_ms,
        decision_at,
        RainAdjustmentModel::retained(),
        admission,
        time,
    )?;
    Ok(performance)
}

// replace model decisions with the explicit root-selected raw policy
fn select_policy_raw(hours: &mut [RainAdjustmentHour]) {
    // preserve native values and source clocks while disabling every correction
    for hour in hours {
        hour.applied = false;
        hour.corrected_precipitation_mm = hour.raw_precipitation_mm;
        hour.reason = Some(RainAdjustmentHourReason::PolicyRaw);
    }
}

// create one complete evaluation from ready typed captures
fn create_evaluation<Admission, Time, Identity>(
    pending: &RainAdjustmentPendingInput,
    now_ms: i64,
    runtime: RainAdjustmentRuntimeSelection,
    model: RainAdjustmentModel<'_>,
    admission: &mut Admission,
    time: &mut Time,
    identity: &mut Identity,
) -> Result<Option<RainAdjustmentEvaluation>, TaskFailure>
where
    Identity: RainAdjustmentIdentityAdapter,
    Admission: RainAdjustmentFeatureAdmission,
    Time: RainAdjustmentTimeAdapter,
{
    // validate every decoded capture clock and forecast before selecting source order
    for capture in &pending.captures {
        validate_forecast_capture(capture)?;
        if time.utc_string(capture.identity.completed_at_ms)? != capture.identity.completed_at {
            return Err(adjustment_failure("rain capture completion differs"));
        }
    }
    let Some((input, current, initialized_at_ms)) = prediction_input(pending, now_ms, time)? else {
        return Ok(None);
    };
    let decision_at_ms = initialized_at_ms
        .checked_add(DECISION_DELAY_MILLISECONDS)
        .ok_or_else(|| adjustment_failure("rain decision clock overflowed"))?;
    // exclude station receipts completed after the causal decision boundary
    if pending
        .captures
        .iter()
        .any(|capture| capture.identity.completed_at_ms > decision_at_ms)
    {
        return Err(adjustment_failure("rain capture completed after decision"));
    }
    let decision_at = time.utc_string(decision_at_ms)?;
    let (projection, mut hours, incumbent_performance) = build_and_score_projection(
        pending,
        &input,
        initialized_at_ms,
        decision_at.clone(),
        model,
        admission,
        time,
    )?;

    // apply raw only after executing the same frozen evaluation as Node
    if runtime == RainAdjustmentRuntimeSelection::DisabledPolicyRawWithComparatorAuthority {
        select_policy_raw(&mut hours);
    }
    let captures = pending
        .captures
        .iter()
        .map(|capture| capture.identity.clone())
        .collect::<Vec<_>>();
    let input_sha256 = identity.input_sha256(&captures)?;
    let generated_at = time.utc_string(now_ms)?;
    let run_initialized_at = input.currentRun.runInitializedAt.clone();
    Ok(Some(RainAdjustmentEvaluation {
        causal_input: input,
        captures,
        current_claim_id: current.claim_id.clone(),
        feature_projection: projection,
        incumbent_performance,
        run: RainAdjustmentRun {
            decision_at,
            first_received_at: current.completed_at,
            forecast_claim_id: current.claim_id,
            generated_at,
            hours,
            input_sha256,
            model_sha256: model.model_sha256.to_owned(),
            run_initialized_at,
        },
    }))
}

// validate one stage receipt against exact projection custody
fn stage_receipt_is_valid<I: RainAdjustmentIdentityAdapter>(
    receipt: &RainAdjustmentStageReceipt,
    projection: &RainAdjustmentProjection,
    identity: &mut I,
) -> bool {
    receipt.projectionKind == AdjustmentRevisionProjectionKind::RainGateInput
        && !receipt.durableAt.is_empty()
        && receipt.projectionIdentitySha256 == projection.projection_identity_sha256
        && receipt.projectionSha256 == projection.projection_identity_sha256
        && sha256_is_valid(&receipt.stageReceiptSha256)
        && identity.stage_receipt_is_authentic(receipt)
}

// validate one database receipt against the staged immutable projection
fn commit_receipt_is_valid<I: RainAdjustmentIdentityAdapter>(
    receipt: &RainAdjustmentCommitReceipt,
    projection: &RainAdjustmentProjection,
    stage_receipt: &RainAdjustmentStageReceipt,
    identity: &mut I,
) -> bool {
    receipt.projectionKind == AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput
        && receipt.projectionIdentitySha256 == projection.projection_identity_sha256
        && receipt.projectionSha256 == projection.projection_identity_sha256
        && receipt.stageReceiptSha256 == stage_receipt.stageReceiptSha256
        && sha256_is_valid(&receipt.receiptSha256)
        && identity.commit_receipt_is_authentic(receipt)
}

// build one exact typed gap from optional projection custody
fn revision_gap(
    logical_key_sha256: &str,
    projection: Option<&RainAdjustmentProjection>,
    reason: RainAdjustmentGapReason,
) -> RainAdjustmentGap {
    RainAdjustmentGap {
        logical_key_sha256: logical_key_sha256.to_owned(),
        projection_identity_sha256: projection
            .map(|value| value.projection_identity_sha256.clone()),
        projection_sha256: projection.map(|value| value.projection_identity_sha256.clone()),
        reason,
    }
}

// persist a transactional gap without allowing archive bookkeeping to lose serving
fn mark_transaction_gap_best_effort<R: RainAdjustmentRepository>(
    repository: &mut R,
    transaction: &mut R::Transaction,
    persisted: &RainAdjustmentPersistedRevision,
    gap: &RainAdjustmentGap,
) -> bool {
    // retain whether the matching marker can be archived only after commit
    repository
        .mark_revision_gap_in_transaction(transaction, persisted, gap)
        .is_ok()
}

// commit one open serving transaction or restore its failed state
fn commit_or_rollback<R: RainAdjustmentRepository>(
    repository: &mut R,
    transaction: &mut R::Transaction,
) -> Result<(), TaskFailure> {
    // retain the primary commit error while best-effort rolling back the adapter
    if let Err(error) = repository.commit_append(transaction) {
        repository.rollback_append(transaction);
        return Err(error);
    }
    Ok(())
}

// complete checked revision custody around one already inserted serving row
fn complete_revision<R, Codec, Client, Identity>(
    repository: &mut R,
    open: &mut RainAdjustmentAppendTransaction<R::Transaction>,
    pending: &RainAdjustmentPendingInput,
    evaluation: &RainAdjustmentEvaluation,
    revision: &mut RainAdjustmentRevisionExecution<'_, Codec, Client>,
    identity: &mut Identity,
    time: &mut impl RainAdjustmentTimeAdapter,
) -> Result<(), TaskFailure>
where
    Client: RainAdjustmentClient,
    Codec: RainAdjustmentProjectionCodec,
    Identity: RainAdjustmentIdentityAdapter,
    R: RainAdjustmentRepository,
{
    let logical_key_sha256 = match revision.codec.logical_key_sha256(&open.persisted) {
        Ok(value) if sha256_is_valid(&value) => value,
        _ => return commit_or_rollback(repository, &mut open.transaction),
    };
    let plan = revision.codec.build_revision(
        pending,
        evaluation,
        &open.persisted,
        &logical_key_sha256,
        time,
    );
    let plan = match plan {
        Ok(plan) if plan.projection.logical_key_sha256 == logical_key_sha256 => plan,
        Err(_) => {
            revision.codec.finish_projection_stage(false);
            let gap = revision_gap(
                &logical_key_sha256,
                None,
                RainAdjustmentGapReason::ArchiveStageFailed,
            );
            let marked = mark_transaction_gap_best_effort(
                repository,
                &mut open.transaction,
                &open.persisted,
                &gap,
            );
            commit_or_rollback(repository, &mut open.transaction)?;

            // archive the database marker only after the serving commit
            if marked {
                let _ = revision.client.record_revision_gap(&gap);
            }
            return Ok(());
        }
        Ok(_) => {
            revision.codec.finish_projection_stage(false);
            let gap = revision_gap(
                &logical_key_sha256,
                None,
                RainAdjustmentGapReason::ArchiveStageFailed,
            );
            let marked = mark_transaction_gap_best_effort(
                repository,
                &mut open.transaction,
                &open.persisted,
                &gap,
            );
            commit_or_rollback(repository, &mut open.transaction)?;

            // archive the identity-mismatch gap only after the serving commit
            if marked {
                let _ = revision.client.record_revision_gap(&gap);
            }
            return Ok(());
        }
    };
    let staged = revision.client.stage_revision(&plan.projection);
    let stage_receipt = match staged {
        Ok(receipt) if stage_receipt_is_valid(&receipt, &plan.projection, identity) => {
            revision.codec.finish_projection_stage(true);
            receipt
        }
        _ => {
            revision.codec.finish_projection_stage(false);
            let gap = revision_gap(
                &logical_key_sha256,
                Some(&plan.projection),
                RainAdjustmentGapReason::ArchiveStageFailed,
            );
            let marked = mark_transaction_gap_best_effort(
                repository,
                &mut open.transaction,
                &open.persisted,
                &gap,
            );
            commit_or_rollback(repository, &mut open.transaction)?;

            // archive the stage gap only after the serving commit
            if marked {
                let _ = revision.client.record_revision_gap(&gap);
            }
            return Ok(());
        }
    };
    let bound = repository.bind_revision(
        &mut open.transaction,
        &open.persisted,
        &plan,
        &stage_receipt,
    );
    let commit_receipt = match bound {
        Ok(receipt)
            if commit_receipt_is_valid(&receipt, &plan.projection, &stage_receipt, identity) =>
        {
            receipt
        }
        _ => {
            let gap = revision_gap(
                &logical_key_sha256,
                Some(&plan.projection),
                RainAdjustmentGapReason::DatabaseBindFailed,
            );
            let marked = mark_transaction_gap_best_effort(
                repository,
                &mut open.transaction,
                &open.persisted,
                &gap,
            );
            commit_or_rollback(repository, &mut open.transaction)?;

            // archive the bind gap only after the recovered transaction commits
            if marked {
                let _ = revision.client.record_revision_gap(&gap);
            }
            return Ok(());
        }
    };
    commit_or_rollback(repository, &mut open.transaction)?;

    // publish only after the serving row and revision pointer commit
    if revision
        .client
        .publish_revision(&plan.projection, &stage_receipt, &commit_receipt)
        .is_err()
    {
        let gap = revision_gap(
            &logical_key_sha256,
            Some(&plan.projection),
            RainAdjustmentGapReason::ArchivePublishFailed,
        );

        // archive only the exact post-commit category persisted by this caller
        if repository
            .mark_revision_gap_after_commit(&open.persisted, &gap)
            .is_ok()
        {
            let _ = revision.client.record_revision_gap(&gap);
        }
    }
    Ok(())
}

/// Publish one mature rain-adjustment run with optional checked revision custody.
pub fn publish_rain_adjustment<Repository, Admission, Time, Identity, Codec, Client>(
    now_ms: i64,
    execution: &mut RainAdjustmentExecution<
        '_,
        Repository,
        Admission,
        Time,
        Identity,
        Codec,
        Client,
    >,
) -> Result<bool, TaskFailure>
where
    Client: RainAdjustmentClient,
    Codec: RainAdjustmentProjectionCodec,
    Identity: RainAdjustmentIdentityAdapter,
    Admission: RainAdjustmentFeatureAdmission,
    Repository: RainAdjustmentRepository,
    Time: RainAdjustmentTimeAdapter,
{
    publish_rain_adjustment_with_shadow(now_ms, execution, |_, _| {})
}

/// Publish one mature run, then invoke optional shadow work after serving settles.
pub fn publish_rain_adjustment_with_shadow<Repository, Admission, Time, Identity, Codec, Client>(
    now_ms: i64,
    execution: &mut RainAdjustmentExecution<
        '_,
        Repository,
        Admission,
        Time,
        Identity,
        Codec,
        Client,
    >,
    shadow: impl FnOnce(RainAdjustmentPendingInput, RainAdjustmentEvaluation),
) -> Result<bool, TaskFailure>
where
    Client: RainAdjustmentClient,
    Codec: RainAdjustmentProjectionCodec,
    Identity: RainAdjustmentIdentityAdapter,
    Admission: RainAdjustmentFeatureAdmission,
    Repository: RainAdjustmentRepository,
    Time: RainAdjustmentTimeAdapter,
{
    // invalid disabled policy stops before capture storage or model inference
    if execution.runtime == RainAdjustmentRuntimeSelection::Disabled {
        return Ok(false);
    }
    let pending = execution
        .repository
        .read_pending(execution.model.model_sha256)?;
    let evaluation = create_evaluation(
        &pending,
        now_ms,
        execution.runtime,
        execution.model,
        execution.admission,
        execution.time,
        execution.identity,
    )?;

    // preserve no mature source as a normal idle result
    let Some(evaluation) = evaluation else {
        return Ok(false);
    };
    let open = execution.repository.begin_append(&evaluation.run)?;
    let inserted = match open {
        Some(mut open) => {
            // keep the exact serving transaction when revision archival is absent
            if let Some(revision) = execution.revision.as_mut() {
                complete_revision(
                    execution.repository,
                    &mut open,
                    &pending,
                    &evaluation,
                    revision,
                    execution.identity,
                    execution.time,
                )?;
            } else {
                commit_or_rollback(execution.repository, &mut open.transaction)?;
            }
            true
        }
        // an existing serving row still owns the same optional shadow opportunity
        None => false,
    };
    shadow(pending, evaluation);
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::process::Command;
    use std::rc::Rc;

    use weather_adjustment::AdmittedRainHurdleWindCalendar;
    use weather_contracts::{
        AdjustmentRevisionCommitReceiptContractVersionLiteral,
        AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionProjectionKind,
        AdjustmentRevisionStageReceiptContractVersionLiteral,
        AdjustmentRevisionStageReceiptDurableLiteral, Nullable,
        RainHurdleWindPortableArtifactV1Heads, RainHurdleWindPortableArtifactV1HeadsNativeHead,
        RainHurdleWindPortableArtifactV1HeadsObjectiveEnum as Objective,
        RainHurdleWindPortableArtifactV1Rules, RainWindForecastHour,
        RainWindPredictionHourReasonCodeEnum, RainWindRunProfile, RainWindStationHour,
    };

    use super::{
        RainAdjustmentAppendTransaction, RainAdjustmentCapture, RainAdjustmentCaptureIdentity,
        RainAdjustmentCaptureKind, RainAdjustmentClient, RainAdjustmentCommitReceipt,
        RainAdjustmentExecution, RainAdjustmentFeatureAdmission, RainAdjustmentGap,
        RainAdjustmentHourReason, RainAdjustmentIdentityAdapter, RainAdjustmentModel,
        RainAdjustmentPendingInput, RainAdjustmentPersistedRevision, RainAdjustmentProjection,
        RainAdjustmentProjectionCodec, RainAdjustmentRepository, RainAdjustmentRevisionExecution,
        RainAdjustmentRevisionPlan, RainAdjustmentRun, RainAdjustmentStageReceipt,
        RainAdjustmentStationHour, RainAdjustmentTimeAdapter, publish_rain_adjustment,
        replay_retained_rain_performance,
    };
    use crate::{RainAdjustmentRuntimeSelection, TaskFailure};

    const NOW_MS: i64 = 1_789_372_800_000;
    const CURRENT_MS: i64 = NOW_MS - 8 * 3_600_000;
    const STATION_HOUR_MS: i64 = NOW_MS - 3_600_000;
    const STATION_RECEIVED_MS: i64 = STATION_HOUR_MS + 30 * 60_000;

    // build one deterministic fixture digest
    fn hash(value: u64) -> String {
        format!("{value:064x}")
    }

    // build one stable fixture error
    fn failure(code: &str) -> TaskFailure {
        TaskFailure {
            attempts: 0,
            code: code.to_owned(),
        }
    }

    // build one complete typed forecast profile
    fn profile(run_initialized_at: &str, completed_at: &str) -> RainWindRunProfile {
        RainWindRunProfile {
            runInitializedAt: run_initialized_at.to_owned(),
            completedAt: completed_at.to_owned(),
            hours: (1..=48)
                .map(|lead| RainWindForecastHour {
                    leadHours: f64::from(lead),
                    precipitationMm: Nullable::Value(0.5),
                    temperatureC: Nullable::Value(12.0),
                    relativeHumidityPercent: Nullable::Value(80.0),
                    cloudCoverPercent: Nullable::Value(90.0),
                    pressureHpa: Nullable::Value(1_000.0),
                    windSpeedMps: Nullable::Value(4.0),
                    windDirectionDegrees: Nullable::Value(180.0),
                })
                .collect(),
        }
    }

    // build one exact mature pending source graph in repository order
    fn pending() -> RainAdjustmentPendingInput {
        RainAdjustmentPendingInput {
            captures: vec![
                RainAdjustmentCapture {
                    identity: RainAdjustmentCaptureIdentity {
                        body_sha256: hash(1),
                        claim_id: "prior".to_owned(),
                        completed_at: "2026-09-14T00:30:00.000Z".to_owned(),
                        completed_at_ms: CURRENT_MS + 30 * 60_000,
                    },
                    kind: RainAdjustmentCaptureKind::Forecast {
                        profile: profile("2026-09-13T18:00:00.000Z", "2026-09-14T00:30:00.000Z"),
                        run_initialized_at_ms: CURRENT_MS - 6 * 3_600_000,
                    },
                },
                RainAdjustmentCapture {
                    identity: RainAdjustmentCaptureIdentity {
                        body_sha256: hash(2),
                        claim_id: "current".to_owned(),
                        completed_at: "2026-09-14T06:01:00.000Z".to_owned(),
                        completed_at_ms: CURRENT_MS + 6 * 3_600_000 + 60_000,
                    },
                    kind: RainAdjustmentCaptureKind::Forecast {
                        profile: profile("2026-09-14T00:00:00.000Z", "2026-09-14T06:01:00.000Z"),
                        run_initialized_at_ms: CURRENT_MS,
                    },
                },
                RainAdjustmentCapture {
                    identity: RainAdjustmentCaptureIdentity {
                        body_sha256: hash(3),
                        claim_id: "station".to_owned(),
                        completed_at: "2026-09-14T07:45:00.000Z".to_owned(),
                        completed_at_ms: CURRENT_MS + 7 * 3_600_000 + 45 * 60_000,
                    },
                    kind: RainAdjustmentCaptureKind::Station {
                        station_id: 64_255,
                        window_start: "2026-09-14T07:00:00.000Z".to_owned(),
                        window_end_exclusive: "2026-09-14T08:00:00.000Z".to_owned(),
                    },
                },
            ],
            station_hours: [
                (64_255.0, 1.0, 7.0),
                (225_947.0, 2.0, 8.0),
                (38_270.0, 3.0, 9.0),
            ]
            .into_iter()
            // retain the first three catalog stations for the weighted-temperature support gate
            .map(
                |(station_id, precipitation_mm, temperature_c)| RainAdjustmentStationHour {
                    hour_at_ms: STATION_HOUR_MS,
                    received_at_ms: STATION_RECEIVED_MS,
                    row: RainWindStationHour {
                        hourAt: "2026-09-14T07:00:00.000Z".to_owned(),
                        precipitationMm: Nullable::Value(precipitation_mm),
                        receivedAt: "2026-09-14T07:30:00.000Z".to_owned(),
                        stationId: station_id,
                        temperatureC: Nullable::Value(temperature_c),
                    },
                },
            )
            .collect(),
        }
    }

    // build one source-shaped compact model table
    fn head(objective: Objective) -> RainHurdleWindPortableArtifactV1HeadsNativeHead {
        RainHurdleWindPortableArtifactV1HeadsNativeHead {
            objective,
            baseScore: 0.5,
            trees: vec![[vec![0.0], vec![0.25], vec![-1.0], vec![-1.0], vec![0.0]]],
        }
    }

    struct FixtureModel {
        heads: RainHurdleWindPortableArtifactV1Heads,
        rules: Vec<RainHurdleWindPortableArtifactV1Rules>,
        scales: Vec<f64>,
    }

    // build one validated scorer fixture
    fn model_fixture() -> FixtureModel {
        FixtureModel {
            heads: RainHurdleWindPortableArtifactV1Heads {
                atLeast0_1: head(Objective::BinaryLogistic),
                atLeast1_0: head(Objective::BinaryLogistic),
                atLeast2_5: head(Objective::BinaryLogistic),
                amount: head(Objective::RegGamma),
            },
            rules: vec![
                RainHurdleWindPortableArtifactV1Rules {
                    threshold: 0.1,
                    cutoff: Nullable::Value(0.1),
                },
                RainHurdleWindPortableArtifactV1Rules {
                    threshold: 1.0,
                    cutoff: Nullable::Value(0.1),
                },
                RainHurdleWindPortableArtifactV1Rules {
                    threshold: 2.5,
                    cutoff: Nullable::Value(0.1),
                },
            ],
            scales: vec![1.0, 1.0, 1.0],
        }
    }

    struct FixtureTime;

    impl RainAdjustmentTimeAdapter for FixtureTime {
        // map only the admitted fixture clocks to canonical UTC values
        fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
            match epoch_ms {
                NOW_MS => Ok("2026-09-14T08:00:00.000Z".to_owned()),
                CURRENT_MS => Ok("2026-09-14T00:00:00.000Z".to_owned()),
                STATION_HOUR_MS => Ok("2026-09-14T07:00:00.000Z".to_owned()),
                STATION_RECEIVED_MS => Ok("2026-09-14T07:30:00.000Z".to_owned()),
                value if value == CURRENT_MS + 30 * 60_000 => {
                    Ok("2026-09-14T00:30:00.000Z".to_owned())
                }
                value if value == CURRENT_MS + 6 * 3_600_000 + 60_000 => {
                    Ok("2026-09-14T06:01:00.000Z".to_owned())
                }
                value if value == CURRENT_MS + 7 * 3_600_000 + 45 * 60_000 => {
                    Ok("2026-09-14T07:45:00.000Z".to_owned())
                }
                value if value == CURRENT_MS - 6 * 3_600_000 => {
                    Ok("2026-09-13T18:00:00.000Z".to_owned())
                }
                value if value == CURRENT_MS - 12 * 3_600_000 => {
                    Ok("2026-09-13T12:00:00.000Z".to_owned())
                }
                value if value == CURRENT_MS + 8 * 3_600_000 => {
                    Ok("2026-09-14T08:00:00.000Z".to_owned())
                }
                value
                    if (CURRENT_MS + 9 * 3_600_000..=CURRENT_MS + 31 * 3_600_000)
                        .contains(&value) =>
                {
                    let lead = (value - CURRENT_MS) / 3_600_000;
                    // format the bounded two-day output window without owning date parsing
                    if lead < 24 {
                        Ok(format!("2026-09-14T{lead:02}:00:00.000Z"))
                    } else {
                        Ok(format!("2026-09-15T{:02}:00:00.000Z", lead - 24))
                    }
                }
                _ => Err(failure("unexpected fixture clock")),
            }
        }
    }

    struct FixtureAdmission;

    impl RainAdjustmentFeatureAdmission for FixtureAdmission {
        // supply deterministic finite UTC harmonics at the admitted runtime boundary
        fn calendar(
            &mut self,
            _valid_at_ms: i64,
        ) -> Result<AdmittedRainHurdleWindCalendar, TaskFailure> {
            Ok(AdmittedRainHurdleWindCalendar {
                annual_cos: 0.2,
                annual_sin: 0.1,
                daily_cos: 0.4,
                daily_sin: 0.3,
            })
        }

        // retain catalog order with explicit positive finite fixture weights
        fn station_weights(&mut self) -> Result<[f64; 12], TaskFailure> {
            Ok([1.0; 12])
        }
    }

    struct ArchiveFixtureAdmission;

    impl RainAdjustmentFeatureAdmission for ArchiveFixtureAdmission {
        // reproduce the original utc annual and daily harmonics for the oracle fixture
        fn calendar(
            &mut self,
            valid_at_ms: i64,
        ) -> Result<AdmittedRainHurdleWindCalendar, TaskFailure> {
            const YEAR_START_MS: i64 = 1_767_225_600_000;
            const YEAR_END_MS: i64 = 1_798_761_600_000;
            let annual = std::f64::consts::TAU * (valid_at_ms - YEAR_START_MS) as f64
                / (YEAR_END_MS - YEAR_START_MS) as f64;
            let utc_hour = (valid_at_ms / 3_600_000).rem_euclid(24) as f64;
            let daily = std::f64::consts::TAU * utc_hour / 24.0;
            Ok(AdmittedRainHurdleWindCalendar {
                annual_cos: annual.cos(),
                annual_sin: annual.sin(),
                daily_cos: daily.cos(),
                daily_sin: daily.sin(),
            })
        }

        // retain catalog order even though this oracle contains no station rows
        fn station_weights(&mut self) -> Result<[f64; 12], TaskFailure> {
            Ok([1.0; 12])
        }
    }

    struct FixtureIdentity;

    impl RainAdjustmentIdentityAdapter for FixtureIdentity {
        // bind the exact repository capture order to one deterministic digest
        fn input_sha256(
            &mut self,
            captures: &[RainAdjustmentCaptureIdentity],
        ) -> Result<String, TaskFailure> {
            assert_eq!(
                captures
                    .iter()
                    .map(|capture| capture.claim_id.as_str())
                    .collect::<Vec<_>>(),
                ["prior", "current", "station"]
            );
            Ok(hash(10))
        }

        // authenticate the exact stage fixture
        fn stage_receipt_is_authentic(&mut self, receipt: &RainAdjustmentStageReceipt) -> bool {
            receipt.stageReceiptSha256 == hash(13)
        }

        // authenticate the exact commit fixture
        fn commit_receipt_is_authentic(&mut self, receipt: &RainAdjustmentCommitReceipt) -> bool {
            receipt.receiptSha256 == hash(14)
        }
    }

    struct FixtureRepository {
        commit_failure: bool,
        duplicate: bool,
        pending: RainAdjustmentPendingInput,
        trace: Rc<RefCell<Vec<String>>>,
    }

    impl RainAdjustmentRepository for FixtureRepository {
        type Transaction = ();

        // select one exact mature input set
        fn read_pending(
            &mut self,
            model_sha256: &str,
        ) -> Result<RainAdjustmentPendingInput, TaskFailure> {
            self.trace.borrow_mut().push(format!("read:{model_sha256}"));
            Ok(self.pending.clone())
        }

        // open one serving transaction and retain the selected current identity
        fn begin_append(
            &mut self,
            run: &RainAdjustmentRun,
        ) -> Result<Option<RainAdjustmentAppendTransaction<Self::Transaction>>, TaskFailure>
        {
            self.trace.borrow_mut().push(format!(
                "begin:{}:{}:{}",
                run.forecast_claim_id,
                run.hours.len(),
                run.hours[0]
                    .reason
                    .map_or("none", RainAdjustmentHourReason::code)
            ));
            // simulate the existing-row path after the append key is checked
            if self.duplicate {
                return Ok(None);
            }
            Ok(Some(RainAdjustmentAppendTransaction {
                persisted: RainAdjustmentPersistedRevision {
                    generated_at: run.generated_at.clone(),
                    hours: run.hours.clone(),
                    input_sha256: run.input_sha256.clone(),
                    model_sha256: run.model_sha256.clone(),
                    run_initialized_at: run.run_initialized_at.clone(),
                    stored_content_sha256: hash(11),
                },
                transaction: (),
            }))
        }

        // bind one staged projection inside the serving transaction
        fn bind_revision(
            &mut self,
            _transaction: &mut Self::Transaction,
            _persisted: &RainAdjustmentPersistedRevision,
            plan: &RainAdjustmentRevisionPlan,
            stage_receipt: &RainAdjustmentStageReceipt,
        ) -> Result<RainAdjustmentCommitReceipt, TaskFailure> {
            self.trace.borrow_mut().push("bind".to_owned());
            Ok(RainAdjustmentCommitReceipt {
                archiveCommitOrdinal: "1".to_owned(),
                archiveCommittedAt: "2026-09-14T08:00:02.000Z".to_owned(),
                contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
                frontierSha256: hash(15),
                predecessorFrontierSha256: hash(16),
                projectionIdentitySha256: plan.projection.projection_identity_sha256.clone(),
                projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput,
                projectionSha256: plan.projection.projection_identity_sha256.clone(),
                receiptSha256: hash(14),
                stageReceiptSha256: stage_receipt.stageReceiptSha256.clone(),
            })
        }

        // retain transactional permanent gaps
        fn mark_revision_gap_in_transaction(
            &mut self,
            _transaction: &mut Self::Transaction,
            _persisted: &RainAdjustmentPersistedRevision,
            gap: &RainAdjustmentGap,
        ) -> Result<(), TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("gap-tx:{}", gap.reason.code()));
            Ok(())
        }

        // commit the serving transaction after revision binding
        fn commit_append(
            &mut self,
            _transaction: &mut Self::Transaction,
        ) -> Result<(), TaskFailure> {
            self.trace.borrow_mut().push("commit".to_owned());
            // surface one serving commit failure after the attempted durable order
            if self.commit_failure {
                return Err(failure("commit failed"));
            }
            Ok(())
        }

        // retain unexpected transaction rollback evidence
        fn rollback_append(&mut self, _transaction: &mut Self::Transaction) {
            self.trace.borrow_mut().push("rollback".to_owned());
        }

        // retain post-commit permanent gaps
        fn mark_revision_gap_after_commit(
            &mut self,
            _persisted: &RainAdjustmentPersistedRevision,
            gap: &RainAdjustmentGap,
        ) -> Result<(), TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("gap-post:{}", gap.reason.code()));
            Ok(())
        }
    }

    struct FixtureCodec {
        trace: Rc<RefCell<Vec<String>>>,
    }

    impl RainAdjustmentProjectionCodec for FixtureCodec {
        // derive the exact logical serving identity before projection encoding
        fn logical_key_sha256(
            &mut self,
            _persisted: &RainAdjustmentPersistedRevision,
        ) -> Result<String, TaskFailure> {
            self.trace.borrow_mut().push("logical-key".to_owned());
            Ok(hash(12))
        }

        // build admitted canonical bytes after the immutable serving insert
        fn build_revision(
            &mut self,
            _pending: &RainAdjustmentPendingInput,
            evaluation: &super::RainAdjustmentEvaluation,
            persisted: &RainAdjustmentPersistedRevision,
            _logical_key_sha256: &str,
            _time: &mut impl RainAdjustmentTimeAdapter,
        ) -> Result<RainAdjustmentRevisionPlan, TaskFailure> {
            let first = &evaluation.feature_projection.rows[0];
            // prove selected forecast, station, calendar, and prior inputs reached the real builder
            assert_eq!(evaluation.feature_projection.rows.len(), 23);
            assert_eq!(first.features.len(), 107);
            assert_eq!(first.features[0], 1.0);
            assert_eq!(first.features[1], 0.1_f32);
            assert_eq!(first.features[2], 0.2_f32);
            assert_eq!(first.features[3], 0.3_f32);
            assert_eq!(first.features[4], 0.4_f32);
            assert_eq!(first.features[5], 0.5);
            assert_eq!(first.features[15], 8.0);
            assert_eq!(first.features[16], 1.5);
            assert_eq!(&first.features[17..21], &[2.0, 1.0, 3.0, 3.0]);
            assert_eq!(first.features[41], 1.0);
            assert_eq!(first.features[83], 0.5);
            assert!(first.features[84].is_nan());
            assert_eq!(first.validAt, "2026-09-14T09:00:00.000Z");
            self.trace.borrow_mut().push(format!(
                "encode:{}:{}",
                evaluation.current_claim_id, persisted.stored_content_sha256
            ));
            Ok(RainAdjustmentRevisionPlan {
                projection: RainAdjustmentProjection {
                    bytes: b"canonical-rain-revision".to_vec(),
                    logical_key_sha256: hash(12),
                    projection_identity_sha256: hash(12),
                },
            })
        }
    }

    struct FixtureClient {
        publish_failure: bool,
        stage_failure: bool,
        trace: Rc<RefCell<Vec<String>>>,
    }

    impl RainAdjustmentClient for FixtureClient {
        // stage one exact canonical revision
        fn stage_revision(
            &mut self,
            projection: &RainAdjustmentProjection,
        ) -> Result<RainAdjustmentStageReceipt, TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("stage:{}", projection.bytes.len()));

            // inject one archive stage refusal after retaining the attempted order
            if self.stage_failure {
                return Err(failure("stage failed"));
            }
            Ok(RainAdjustmentStageReceipt {
                contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
                durable: AdjustmentRevisionStageReceiptDurableLiteral,
                durableAt: "2026-09-14T08:00:01.000Z".to_owned(),
                projectionIdentitySha256: projection.projection_identity_sha256.clone(),
                projectionKind: AdjustmentRevisionProjectionKind::RainGateInput,
                projectionSha256: projection.projection_identity_sha256.clone(),
                stageReceiptSha256: hash(13),
            })
        }

        // publish after database commit
        fn publish_revision(
            &mut self,
            _projection: &RainAdjustmentProjection,
            _stage_receipt: &RainAdjustmentStageReceipt,
            _commit_receipt: &RainAdjustmentCommitReceipt,
        ) -> Result<(), TaskFailure> {
            self.trace.borrow_mut().push("publish".to_owned());

            // inject one post-commit archive publication failure
            if self.publish_failure {
                return Err(failure("publish failed"));
            }
            Ok(())
        }

        // retain exact archived gap categories
        fn record_revision_gap(&mut self, gap: &RainAdjustmentGap) -> Result<(), TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("record-gap:{}", gap.reason.code()));
            Ok(())
        }
    }

    // execute one normal or policy-raw fixture through the real Rust scorer
    fn run_fixture_with_failures(
        runtime: RainAdjustmentRuntimeSelection,
        stage_failure: bool,
        publish_failure: bool,
    ) -> (bool, Vec<String>) {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let model = model_fixture();
        let mut repository = FixtureRepository {
            commit_failure: false,
            duplicate: false,
            pending: pending(),
            trace: Rc::clone(&trace),
        };
        let mut admission = FixtureAdmission;
        let mut time = FixtureTime;
        let mut identity = FixtureIdentity;
        let mut codec = FixtureCodec {
            trace: Rc::clone(&trace),
        };
        let mut client = FixtureClient {
            publish_failure,
            stage_failure,
            trace: Rc::clone(&trace),
        };
        let published = publish_rain_adjustment(
            NOW_MS,
            &mut RainAdjustmentExecution {
                admission: &mut admission,
                identity: &mut identity,
                model: RainAdjustmentModel::from_typed(
                    &model.heads,
                    &model.rules,
                    &model.scales,
                    "2026-08",
                    &hash(9),
                ),
                repository: &mut repository,
                revision: Some(RainAdjustmentRevisionExecution {
                    client: &mut client,
                    codec: &mut codec,
                }),
                runtime,
                time: &mut time,
            },
        )
        .expect("normal rain publication should complete");
        let events = trace.borrow().clone();
        (published, events)
    }

    // execute the successful default fixture
    fn run_fixture(runtime: RainAdjustmentRuntimeSelection) -> (bool, Vec<String>) {
        run_fixture_with_failures(runtime, false, false)
    }

    // execute one callback fixture through the same serving and revision owners
    fn run_shadow_fixture(
        pending: RainAdjustmentPendingInput,
        duplicate: bool,
        commit_failure: bool,
    ) -> (Result<bool, TaskFailure>, Vec<String>) {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let model = model_fixture();
        let mut repository = FixtureRepository {
            commit_failure,
            duplicate,
            pending,
            trace: Rc::clone(&trace),
        };
        let mut admission = FixtureAdmission;
        let mut time = FixtureTime;
        let mut identity = FixtureIdentity;
        let mut codec = FixtureCodec {
            trace: Rc::clone(&trace),
        };
        let mut client = FixtureClient {
            publish_failure: false,
            stage_failure: false,
            trace: Rc::clone(&trace),
        };
        let shadow_trace = Rc::clone(&trace);
        let result = super::publish_rain_adjustment_with_shadow(
            NOW_MS,
            &mut RainAdjustmentExecution {
                admission: &mut admission,
                identity: &mut identity,
                model: RainAdjustmentModel::from_typed(
                    &model.heads,
                    &model.rules,
                    &model.scales,
                    "2026-08",
                    &hash(9),
                ),
                repository: &mut repository,
                revision: Some(RainAdjustmentRevisionExecution {
                    client: &mut client,
                    codec: &mut codec,
                }),
                runtime: RainAdjustmentRuntimeSelection::Active,
                time: &mut time,
            },
            |pending, evaluation| {
                shadow_trace.borrow_mut().push(format!(
                    "shadow:{}:{}",
                    evaluation.current_claim_id,
                    pending.captures.len()
                ));
            },
        );
        let events = trace.borrow().clone();
        (result, events)
    }

    /// Match the compiled Node publisher's normal selected output geometry.
    #[test]
    fn publishes_normal_model_run_with_checked_revision_order() {
        let (published, events) = run_fixture(RainAdjustmentRuntimeSelection::Active);
        assert!(published);
        assert_eq!(
            events,
            [
                format!("read:{}", hash(9)),
                "begin:current:23:none".to_owned(),
                "logical-key".to_owned(),
                format!("encode:current:{}", hash(11)),
                "stage:23".to_owned(),
                "bind".to_owned(),
                "commit".to_owned(),
                "publish".to_owned(),
            ]
        );
        assert_eq!(node_oracle(), "true|current|23|9|31");
    }

    /// Match the compiled Node retained-model archive replay without publication owners.
    #[test]
    fn replays_retained_archive_model_with_exact_saved_identity() {
        let mut archived = pending();
        let mut current = archived.captures.remove(1);
        // require the fixture source to remain the expected forecast shape
        let RainAdjustmentCaptureKind::Forecast { profile, .. } = &mut current.kind else {
            panic!("current fixture should be a forecast");
        };
        profile.hours[8].temperatureC = Nullable::Value(1.0);
        archived.captures = vec![current];
        archived.station_hours.clear();

        let performance = replay_retained_rain_performance(
            &archived,
            "current",
            NOW_MS,
            &mut ArchiveFixtureAdmission,
            &mut FixtureTime,
        )
        .expect("retained archive replay should succeed");
        assert_eq!(
            performance.modelSha256,
            "dfba22520eb66944046f999e96ee51378913ef926a2da27f0562490d9fb7ef64"
        );
        assert_eq!(performance.modelMonth, "2026-08");
        assert_eq!(performance.decisionAt, "2026-09-14T08:00:00.000Z");
        assert_eq!(performance.hours.len(), 23);
        assert_eq!(performance.hours[0].modelLeadHours, 9.0);
        assert_eq!(performance.hours[0].correctedPrecipitationMm, 0.5);
        assert_eq!(
            performance.hours[0].reasonCode,
            Nullable::Value(RainWindPredictionHourReasonCodeEnum::PhaseUnsupported)
        );
        let lead_10 = &performance.hours[1];
        assert!((lead_10.correctedPrecipitationMm - 0.1557320453415243).abs() < 1e-12);
        let Nullable::Value(probabilities) = &lead_10.occurrenceProbabilities else {
            panic!("warm oracle hour should retain occurrence probabilities");
        };
        assert!((probabilities.atLeast0_1 - 0.19661302864551544).abs() < 1e-12);
        assert!((probabilities.atLeast1_0 - 0.04049503430724144).abs() < 1e-12);
        assert!((probabilities.atLeast2_5 - 0.013692766427993774).abs() < 1e-12);
        assert!(
            (performance.hours[22].correctedPrecipitationMm - 0.17513893488146984).abs() < 1e-12
        );

        let claim_error = replay_retained_rain_performance(
            &archived,
            "different",
            NOW_MS,
            &mut ArchiveFixtureAdmission,
            &mut FixtureTime,
        )
        .expect_err("saved claim mismatch should fail");
        assert_eq!(claim_error.code, "rain archive source claim differs");
        let clock_error = replay_retained_rain_performance(
            &archived,
            "current",
            NOW_MS + 3_600_000,
            &mut ArchiveFixtureAdmission,
            &mut FixtureTime,
        )
        .expect_err("saved decision mismatch should fail");
        assert_eq!(clock_error.code, "rain archive decision clock differs");
    }

    /// Invoke shadow work only after the serving revision publication completes.
    #[test]
    fn invokes_shadow_after_committed_serving_order() {
        let (published, events) = run_shadow_fixture(pending(), false, false);
        assert!(published.expect("serving should commit"));
        assert!(events.ends_with(&["publish".to_owned(), "shadow:current:3".to_owned()]));
    }

    /// Preserve a shadow opportunity when the serving row already exists.
    #[test]
    fn duplicate_serving_row_still_invokes_shadow() {
        let (published, events) = run_shadow_fixture(pending(), true, false);
        assert!(!published.expect("duplicate should stay idle"));
        assert!(events.ends_with(&[
            "begin:current:23:none".to_owned(),
            "shadow:current:3".to_owned(),
        ]));
        assert!(!events.iter().any(|event| event == "commit"));
    }

    /// Suppress shadow work when no mature source exists or serving fails.
    #[test]
    fn idle_and_failed_serving_do_not_invoke_shadow() {
        let empty = RainAdjustmentPendingInput {
            captures: Vec::new(),
            station_hours: Vec::new(),
        };
        let (idle, idle_events) = run_shadow_fixture(empty, false, false);
        assert!(!idle.expect("missing source should stay idle"));
        assert!(!idle_events.iter().any(|event| event.starts_with("shadow:")));

        let (failed, failed_events) = run_shadow_fixture(pending(), false, true);
        assert_eq!(
            failed.expect_err("commit failure should surface").code,
            "commit failed"
        );
        assert!(failed_events.ends_with(&["commit".to_owned(), "rollback".to_owned()]));
        assert!(
            !failed_events
                .iter()
                .any(|event| event.starts_with("shadow:"))
        );
    }

    /// Preserve root-selected raw decisions after executing normal inference.
    #[test]
    fn publishes_policy_raw_without_skipping_evaluation() {
        let (published, events) =
            run_fixture(RainAdjustmentRuntimeSelection::DisabledPolicyRawWithComparatorAuthority);
        assert!(published);
        assert!(events.contains(&"begin:current:23:policy_raw".to_owned()));
        assert!(
            events
                .iter()
                .any(|event| event.starts_with("encode:current:"))
        );
    }

    /// Commit serving before archiving a durable stage gap.
    #[test]
    fn stage_failure_preserves_serving_and_records_gap_after_commit() {
        let (published, events) =
            run_fixture_with_failures(RainAdjustmentRuntimeSelection::Active, true, false);
        assert!(published);
        let relevant = events
            .into_iter()
            .filter(|event| {
                event.starts_with("stage:")
                    || event.starts_with("gap-")
                    || event == "commit"
                    || event.starts_with("record-gap:")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            relevant,
            [
                "stage:23",
                "gap-tx:archive_stage_failed",
                "commit",
                "record-gap:archive_stage_failed",
            ]
        );
    }

    /// Preserve serving while marking an archive publication failure after commit.
    #[test]
    fn publish_failure_marks_exact_post_commit_gap() {
        let (published, events) =
            run_fixture_with_failures(RainAdjustmentRuntimeSelection::Active, false, true);
        assert!(published);
        assert!(events.ends_with(&[
            "commit".to_owned(),
            "publish".to_owned(),
            "gap-post:archive_publish_failed".to_owned(),
            "record-gap:archive_publish_failed".to_owned(),
        ]));
    }

    /// Stop disabled policy before pending capture access.
    #[test]
    fn disabled_runtime_has_no_effects() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let model = model_fixture();
        let mut repository = FixtureRepository {
            commit_failure: false,
            duplicate: false,
            pending: pending(),
            trace: Rc::clone(&trace),
        };
        let mut admission = FixtureAdmission;
        let mut time = FixtureTime;
        let mut identity = FixtureIdentity;
        let mut codec = FixtureCodec {
            trace: Rc::clone(&trace),
        };
        let mut client = FixtureClient {
            publish_failure: false,
            stage_failure: false,
            trace: Rc::clone(&trace),
        };
        let published = publish_rain_adjustment(
            NOW_MS,
            &mut RainAdjustmentExecution {
                admission: &mut admission,
                identity: &mut identity,
                model: RainAdjustmentModel::from_typed(
                    &model.heads,
                    &model.rules,
                    &model.scales,
                    "2026-08",
                    &hash(9),
                ),
                repository: &mut repository,
                revision: Some(RainAdjustmentRevisionExecution {
                    client: &mut client,
                    codec: &mut codec,
                }),
                runtime: RainAdjustmentRuntimeSelection::Disabled,
                time: &mut time,
            },
        )
        .expect("disabled runtime should return idle");
        assert!(!published);
        assert!(trace.borrow().is_empty());
    }

    // invoke the compiled normal TypeScript publisher fixture
    fn node_oracle() -> String {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/rain_adjustment_publisher_oracle.mjs");
        let output = Command::new("node")
            .arg(script)
            .output()
            .expect("compiled Node publisher oracle should start");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("Node publisher oracle should emit UTF-8")
            .trim()
            .to_owned()
    }
}
