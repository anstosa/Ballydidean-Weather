//! Native rain inference and fixed-gauge task ownership over the shared worker pool.

use std::{sync::Arc, time::Duration};

use jiff::{Timestamp, civil::Date, tz::TimeZone};
use weather_api::AdmittedAdjustmentRevisionCaptureWitness;
use weather_contracts::{
    AdjustmentRainFixedGaugeTargetProjection, CanonicalWeatherMetrics, DeviceMetadata,
    FieldPresence, JsonValue, NormalizedWeatherRecord, Nullable, RainAdjustmentHour, SourceKind,
    WeatherRecordMetadata,
};
use weather_database::{
    ApiMaintenancePredictionValues, SerializedJson, Sha256Digest, WorkerRainAdjustmentAppendValues,
    WorkerRainAdjustmentReadValues, WorkerRainFixedGaugeReadValues,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_worker::{
    AdmittedRainHurdleWindCalendar, RainAdjustmentFeatureAdmission, RainAdjustmentTimeAdapter,
    RainPortableReplayCalendar, TaskFailure,
};

use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    DatabaseRainInferenceBinding, HttpWorkerRainMaintenanceClient, LoadedWorkerRainServing,
    MaintenanceReceiptScalar, NativeApiCaptureCodec, NativeApiCaptureValueError,
    NativeApiMaintenanceCandidateFiles, NativeEdgeOrigins, NativeEvaluationStreamingCodec,
    NativeModelEvidenceValues, NativeOutboundHttp, NativePostgresPool, NativePostgresWorkerPool,
    NativePredecessorModelCalendar, NativePrivatePostTransport, NativeRainCaptureValues,
    NativeScheduledValues, NativeWirePrimitives, NativeWorkerRainMaintenanceHttpJson,
    RetainedWorkerSidecarTask, StartedWorkerRainFixedGauge, WorkerConfiguration,
    WorkerLoadedSidecarAdmission, WorkerRainAdjustmentCaptureValues, WorkerRainControlRevision,
    WorkerRainFixedGaugeIdentityValues, WorkerRainFixedGaugeProjectionValues,
    WorkerRainGaugeReplayValues, WorkerRainInferenceServing, WorkerRainInferenceShadow,
    WorkerRainInferenceTask, WorkerRainRevisionCodec, WorkerRainRevisionIdentity,
    WorkerRainShadowStartup, load_worker_rain_control_reference, native_worker_shared_pool,
    prepare_worker_rain_fixed_gauge_target,
};

/// The actual private rain wire owner sharing the process HTTP pool.
pub type NativeWorkerRainHttp =
    NativeWorkerRainMaintenanceHttpJson<NativeModelEvidenceValues, NativePrivatePostTransport>;

/// Fixed-gauge projection primitives kept separate from iteration persistence values.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWorkerRainProjectionValues {
    capture: NativeRainCaptureValues,
    evidence: NativeModelEvidenceValues,
}

impl NativeWorkerRainProjectionValues {
    /// Construct the genuine provider replay and canonical evidence owners.
    pub const fn new() -> Self {
        Self {
            capture: NativeRainCaptureValues,
            evidence: NativeModelEvidenceValues::new(),
        }
    }
}

/// One retained task using an independent lease from the worker's actual PostgreSQL pool.
pub struct NativeWorkerRainSharedTask<Task> {
    pool: NativePostgresPool,
    task: Task,
}

impl<Task> NativeWorkerRainSharedTask<Task> {
    /// Clone only the real shared-pool handle before retaining the concrete task.
    pub fn from_worker_pool(
        pool: &mut NativePostgresWorkerPool,
        task: Task,
    ) -> Result<Self, TaskFailure> {
        Ok(Self {
            pool: native_worker_shared_pool(pool)?,
            task,
        })
    }
}

impl<Values, Input, Task> RetainedWorkerSidecarTask<NativePostgresWorkerPool, Values, Input>
    for NativeWorkerRainSharedTask<Task>
where
    Task: RetainedWorkerSidecarTask<NativePostgresPool, Values, Input>,
{
    type Output = Task::Output;

    /// Run against an actual lease source while the outer worker retains pool lifetime ownership.
    fn run(
        &mut self,
        input: Input,
        _database: &mut NativePostgresWorkerPool,
        values: &mut Values,
    ) -> Result<Self::Output, TaskFailure> {
        self.task.run(input, &mut self.pool, values)
    }
}

/// Optional private task that fails closed if admission and execution ever disagree.
pub struct NativeOptionalWorkerRainTask<Task> {
    task: Option<Task>,
    unavailable_code: &'static str,
}

impl<Task> NativeOptionalWorkerRainTask<Task> {
    /// Retain an admitted optional task without installing an inactive success substitute.
    pub const fn new(task: Option<Task>, unavailable_code: &'static str) -> Self {
        Self {
            task,
            unavailable_code,
        }
    }

    /// Report whether startup actually constructed the private task.
    pub const fn is_loaded(&self) -> bool {
        self.task.is_some()
    }
}

impl<Database, Values, Input, Task> RetainedWorkerSidecarTask<Database, Values, Input>
    for NativeOptionalWorkerRainTask<Task>
where
    Task: RetainedWorkerSidecarTask<Database, Values, Input>,
{
    type Output = Task::Output;

    /// Refuse an impossible coordinator call instead of fabricating an idle result.
    fn run(
        &mut self,
        input: Input,
        database: &mut Database,
        values: &mut Values,
    ) -> Result<Self::Output, TaskFailure> {
        // only genuinely constructed work may cross the optional admission boundary
        let task = self
            .task
            .as_mut()
            .ok_or_else(|| failure(self.unavailable_code))?;
        task.run(input, database, values)
    }
}

/// Concrete fixed-gauge task over the native provider, archive, and PostgreSQL owners.
pub type NativeWorkerRainFixedGaugeTask = NativeOptionalWorkerRainTask<
    NativeWorkerRainSharedTask<
        StartedWorkerRainFixedGauge<
            NativeWorkerRainProjectionValues,
            NativeWorkerRainProjectionValues,
            NativeWorkerRainHttp,
        >,
    >,
>;

/// Concrete serving and optional shadow task over the native retained model.
pub type NativeWorkerRainInferenceTask = NativeWorkerRainSharedTask<
    WorkerRainInferenceTask<
        DatabaseRainInferenceBinding,
        WorkerRainInferenceServing<
            LoadedWorkerRainServing,
            NativeRainAdjustmentFeatureAdmission,
            NativeRainAdjustmentTime,
            WorkerRainRevisionIdentity<NativeModelEvidenceValues>,
            WorkerRainRevisionCodec<
                NativeModelEvidenceValues,
                HttpWorkerRainMaintenanceClient<NativeWorkerRainHttp>,
            >,
            HttpWorkerRainMaintenanceClient<NativeWorkerRainHttp>,
        >,
        WorkerRainInferenceShadow<
            NativeModelEvidenceValues,
            NativeRainPortableCalendar,
            HttpWorkerRainMaintenanceClient<NativeWorkerRainHttp>,
        >,
    >,
>;

/// Construct the fixed-gauge task only when both private origin and witness are admitted.
pub fn native_worker_rain_fixed_gauge_task(
    pool: &mut NativePostgresWorkerPool,
    client: Arc<NativeOutboundHttp>,
    configuration: &WorkerConfiguration,
    capture_epoch: Option<Arc<AdmittedAdjustmentRevisionCaptureWitness>>,
    request_timeout: Duration,
) -> Result<NativeWorkerRainFixedGaugeTask, TaskFailure> {
    let Some((origin, capture_epoch)) = configuration
        .adjustment_maintenance_api_origin
        .as_deref()
        .zip(capture_epoch)
    else {
        return Ok(NativeOptionalWorkerRainTask::new(
            None,
            "rain_fixed_gauge_private_task_unavailable",
        ));
    };
    let http = native_rain_http(client, request_timeout);
    let mut origins = NativeEdgeOrigins;
    let task = prepare_worker_rain_fixed_gauge_target(
        origin,
        &mut origins,
        capture_epoch,
        NativeWorkerRainProjectionValues::new(),
        Arc::new(NativeWorkerRainProjectionValues::new()),
        http,
    )
    .map_err(|_| failure("rain_fixed_gauge_origin_invalid"))?;
    let task = NativeWorkerRainSharedTask::from_worker_pool(pool, task)?;
    Ok(NativeOptionalWorkerRainTask::new(
        Some(task),
        "rain_fixed_gauge_private_task_unavailable",
    ))
}

/// Preserve the one admitted rain serving model and its local candidate material owner.
pub struct NativeWorkerRainInferenceMaterial<'a> {
    pub files: &'a mut NativeApiMaintenanceCandidateFiles,
    pub values: NativeModelEvidenceValues,
    pub model: LoadedWorkerRainServing,
}

/// Construct serving inference and attach private revision/shadow work only when admitted.
pub fn native_worker_rain_inference_task(
    pool: &mut NativePostgresWorkerPool,
    client: Arc<NativeOutboundHttp>,
    configuration: &WorkerConfiguration,
    loaded: &mut WorkerLoadedSidecarAdmission,
    material: NativeWorkerRainInferenceMaterial<'_>,
    request_timeout: Duration,
) -> Result<NativeWorkerRainInferenceTask, TaskFailure> {
    let NativeWorkerRainInferenceMaterial {
        files,
        values,
        model,
    } = material;
    let evidence = Arc::new(values);
    let authority = model.comparator_authority();
    let mut revision = None;
    let mut shadow = None;
    loaded.rain_adjustment_revision_client_loaded = false;

    // private revision and shadow effects require the same origin and capture witness
    if let (Some(origin), Some(epoch)) = (
        configuration.adjustment_maintenance_api_origin.as_deref(),
        loaded.adjustment_capture_epoch.clone(),
    ) {
        let mut origins = NativeEdgeOrigins;
        let mut codec = NativeWirePrimitives;
        let shadow_client = HttpWorkerRainMaintenanceClient::admit(
            origin,
            &mut origins,
            native_rain_http(Arc::clone(&client), request_timeout),
        )
        .map_err(|_| failure("rain_shadow_origin_invalid"))?;
        // unavailable local calendar or candidate material disables only optional shadow work
        if let Ok(calendar) = NativeRainPortableCalendar::load() {
            shadow = WorkerRainInferenceShadow::load(
                files,
                &mut codec,
                WorkerRainShadowStartup {
                    evidence: Arc::clone(&evidence),
                    calendar,
                    client: Some(shadow_client),
                    epoch: Some(Arc::clone(&epoch)),
                    authority: authority.clone(),
                },
            );
        }

        let revision_client = HttpWorkerRainMaintenanceClient::admit(
            origin,
            &mut origins,
            native_rain_http(Arc::clone(&client), request_timeout),
        )
        .map_err(|_| failure("rain_revision_origin_invalid"))?;
        let control = load_worker_rain_control_reference(files, &mut codec, evidence.as_ref())
            .ok()
            .flatten()
            .map(|reference| {
                HttpWorkerRainMaintenanceClient::admit(
                    origin,
                    &mut origins,
                    native_rain_http(Arc::clone(&client), request_timeout),
                )
                .map(|archive| WorkerRainControlRevision {
                    reference,
                    authority: authority.clone(),
                    archive,
                })
            })
            .transpose()
            .map_err(|_| failure("rain_control_origin_invalid"))?;
        let features = crate::WorkerRainFeatureRevisionCodec::new(Arc::clone(&evidence), epoch);
        revision = Some((
            WorkerRainRevisionCodec::new(features, control),
            revision_client,
        ));
        loaded.rain_adjustment_revision_client_loaded = true;
    }

    let task = WorkerRainInferenceTask::new(
        DatabaseRainInferenceBinding,
        WorkerRainInferenceServing {
            admission: NativeRainAdjustmentFeatureAdmission,
            time: NativeRainAdjustmentTime,
            identity: WorkerRainRevisionIdentity::new(evidence),
            model,
            revision,
        },
        shadow,
    );
    NativeWorkerRainSharedTask::from_worker_pool(pool, task)
}

/// Exact UTC harmonics and fixed station geometry for native rain inference.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeRainAdjustmentFeatureAdmission;

impl RainAdjustmentFeatureAdmission for NativeRainAdjustmentFeatureAdmission {
    /// Reproduce JavaScript UTC year and hour harmonics from one admitted instant.
    fn calendar(
        &mut self,
        valid_at_ms: i64,
    ) -> Result<AdmittedRainHurdleWindCalendar, TaskFailure> {
        let timestamp = Timestamp::from_millisecond(valid_at_ms)
            .map_err(|_| failure("rain_adjustment_calendar_invalid"))?;
        let datetime = TimeZone::UTC.to_datetime(timestamp);
        let year = datetime.year();
        let year_start = TimeZone::UTC
            .to_timestamp(
                Date::new(year, 1, 1)
                    .map_err(|_| failure("rain_adjustment_calendar_invalid"))?
                    .at(0, 0, 0, 0),
            )
            .map_err(|_| failure("rain_adjustment_calendar_invalid"))?
            .as_millisecond();
        let year_end = TimeZone::UTC
            .to_timestamp(
                Date::new(year + 1, 1, 1)
                    .map_err(|_| failure("rain_adjustment_calendar_invalid"))?
                    .at(0, 0, 0, 0),
            )
            .map_err(|_| failure("rain_adjustment_calendar_invalid"))?
            .as_millisecond();
        let annual = 2.0 * std::f64::consts::PI * (valid_at_ms - year_start) as f64
            / (year_end - year_start) as f64;
        let daily = 2.0 * std::f64::consts::PI * f64::from(datetime.hour()) / 24.0;
        let calendar = AdmittedRainHurdleWindCalendar {
            annual_cos: annual.cos(),
            annual_sin: annual.sin(),
            daily_cos: daily.cos(),
            daily_sin: daily.sin(),
        };
        // reject a platform math failure before the pure model sees it
        if [
            calendar.annual_cos,
            calendar.annual_sin,
            calendar.daily_cos,
            calendar.daily_sin,
        ]
        .iter()
        .any(|value| !value.is_finite())
        {
            return Err(failure("rain_adjustment_calendar_invalid"));
        }
        Ok(calendar)
    }

    /// Return the exact JavaScript-computed inverse-distance weights in catalog order.
    fn station_weights(&mut self) -> Result<[f64; 12], TaskFailure> {
        Ok([
            f64::from_bits(0x3feac6b3c3e9617f),
            f64::from_bits(0x3fea359b380542db),
            f64::from_bits(0x3fe8e968b3225631),
            f64::from_bits(0x3fe8cdec5703a511),
            f64::from_bits(0x3fe57d6bda81c16d),
            f64::from_bits(0x3fe5757fd13c391e),
            f64::from_bits(0x3fe307ef06f37997),
            f64::from_bits(0x3fde1225a651824a),
            f64::from_bits(0x3fcafe7688724d40),
            f64::from_bits(0x3fca49b09d235947),
            f64::from_bits(0x3fc812ce46bfbc06),
            f64::from_bits(0x3fc2b2faab23c3d3),
        ])
    }
}

/// Exact canonical UTC formatter retained by serving revision work.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeRainAdjustmentTime;

impl RainAdjustmentTimeAdapter for NativeRainAdjustmentTime {
    /// Format one already-admitted epoch without sampling another clock.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        NativeWirePrimitives
            .format_utc_milliseconds(epoch_ms)
            .ok_or_else(|| failure("rain_adjustment_time_invalid"))
    }
}

/// Los Angeles month projection used only by installed portable v2 candidates.
pub struct NativeRainPortableCalendar {
    calendar: NativePredecessorModelCalendar,
}

impl NativeRainPortableCalendar {
    /// Admit the same generated predecessor calendar used by the installed model runtime.
    pub fn load() -> Result<Self, TaskFailure> {
        NativePredecessorModelCalendar::new()
            .map(|calendar| Self { calendar })
            .ok_or_else(|| failure("rain_portable_calendar_unavailable"))
    }
}

impl RainPortableReplayCalendar for NativeRainPortableCalendar {
    /// Derive the original local month using the pinned predecessor transition authority.
    fn local_month(&mut self, valid_at: &str) -> Option<u8> {
        let features = self.calendar.features(valid_at).ok()?;
        (features.valid_at == valid_at).then_some(features.month)
    }
}

impl ApiMaintenanceReceiptCanonicalValues for NativeWorkerRainProjectionValues {
    /// Delegate canonical UTC admission to the genuine evidence owner.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        self.evidence.canonical_utc_epoch_milliseconds(value)
    }

    /// Delegate exact byte hashing to the genuine evidence owner.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        ApiMaintenanceReceiptCanonicalValues::sha256(&self.evidence, bytes)
    }

    /// Delegate original receipt field encoding to the genuine evidence owner.
    fn unsigned_receipt_sha256(
        &self,
        fields: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        self.evidence.unsigned_receipt_sha256(fields)
    }
}

impl WorkerRainFixedGaugeIdentityValues for NativeWorkerRainProjectionValues {
    /// Hash the exact normalized weather content through scheduled production values.
    fn fixed_gauge_record_content_sha256(
        &self,
        record: &NormalizedWeatherRecord,
    ) -> Option<Sha256Digest> {
        fixed_gauge_record_content_sha256(record)
    }
}

impl WorkerRainGaugeReplayValues for NativeWorkerRainProjectionValues {
    /// Decode one actual retained Tempest response through the native provider owner.
    fn decode_tempest(
        &mut self,
        bytes: &[u8],
    ) -> Result<weather_worker::TempestObservationEnvelope, TaskFailure> {
        self.capture.decode_tempest(bytes)
    }

    /// Parse an exact provider replay clock through the native provider owner.
    fn parse_utc_milliseconds(&mut self, value: &str) -> Result<i64, TaskFailure> {
        self.capture.parse_utc_milliseconds(value)
    }

    /// Format an exact provider replay clock through the native provider owner.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        self.capture.format_utc_milliseconds(epoch_ms)
    }
}

impl WorkerRainFixedGaugeProjectionValues for NativeWorkerRainProjectionValues {
    /// Encode standard padded base64 with the approved archive primitive.
    fn encode_fixed_gauge_base64(&self, raw: &[u8]) -> Option<String> {
        self.evidence.encode_standard_base64(raw)
    }

    /// Encode the exact recursively sorted canonical projection with its framing LF.
    fn encode_fixed_gauge_projection(
        &self,
        projection: &AdjustmentRainFixedGaugeTargetProjection,
    ) -> Option<Vec<u8>> {
        self.evidence
            .encode_sorted_json(&fixed_gauge_projection_value(projection)?)
    }
}

impl WorkerRainAdjustmentReadValues for NativeScheduledValues {
    type Error = TaskFailure;

    /// Format PostgreSQL timestamps through the native canonical UTC owner.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Result<String, Self::Error> {
        NativeWirePrimitives
            .format_utc_milliseconds(epoch_ms)
            .ok_or_else(|| failure("rain_adjustment_time_invalid"))
    }

    /// Inflate concatenated gzip members without exceeding the retained body ceiling.
    fn gunzip_bounded(
        &mut self,
        compressed: &[u8],
        maximum_output_bytes: usize,
    ) -> Result<Vec<u8>, Self::Error> {
        NativeEvaluationStreamingCodec
            .gunzip_bounded(compressed, maximum_output_bytes)
            .ok_or_else(|| failure("rain_adjustment_gzip_invalid"))
    }

    /// Digest the exact decompressed provider bytes.
    fn sha256(&mut self, bytes: &[u8]) -> Result<Sha256Digest, Self::Error> {
        ApiMaintenanceReceiptCanonicalValues::sha256(&NativeModelEvidenceValues::new(), bytes)
            .ok_or_else(|| failure("rain_adjustment_digest_failed"))
    }
}

impl WorkerRainFixedGaugeReadValues for NativeScheduledValues {
    /// Parse only the exact canonical UTC millisecond spelling.
    fn parse_canonical_utc_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error> {
        NativeWirePrimitives
            .canonical_utc_milliseconds(value)
            .ok_or_else(|| failure("rain_fixed_gauge_time_invalid"))
    }
}

impl WorkerRainAdjustmentAppendValues for NativeScheduledValues {
    type Error = TaskFailure;

    /// Encode the original ordered public-safe serving hours for the JSONB bind.
    fn encode_hours(
        &mut self,
        hours: &[RainAdjustmentHour],
    ) -> Result<SerializedJson, Self::Error> {
        let value = JsonValue::Array(
            hours
                .iter()
                .map(rain_hour_value)
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| failure("rain_adjustment_hours_invalid"))?,
        );
        let mut bytes = NativeWirePrimitives
            .encode_ordered_json(&value)
            .ok_or_else(|| failure("rain_adjustment_hours_invalid"))?;
        // database JSON.stringify bytes omit the archive encoder's trailing LF
        if bytes.pop() != Some(b'\n') {
            return Err(failure("rain_adjustment_hours_invalid"));
        }
        String::from_utf8(bytes)
            .map(SerializedJson::admitted)
            .map_err(|_| failure("rain_adjustment_hours_invalid"))
    }
}

impl WorkerRainGaugeReplayValues for NativeScheduledValues {
    /// Decode retained Tempest bytes through the genuine provider body owner.
    fn decode_tempest(
        &mut self,
        bytes: &[u8],
    ) -> Result<weather_worker::TempestObservationEnvelope, TaskFailure> {
        NativeRainCaptureValues.decode_tempest(bytes)
    }

    /// Parse replay clocks through the genuine provider body owner.
    fn parse_utc_milliseconds(&mut self, value: &str) -> Result<i64, TaskFailure> {
        NativeRainCaptureValues.parse_utc_milliseconds(value)
    }

    /// Format replay clocks through the genuine provider body owner.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        NativeRainCaptureValues.format_utc_milliseconds(epoch_ms)
    }
}

impl WorkerRainAdjustmentCaptureValues for NativeScheduledValues {
    /// Decode the exact native forecast capture payload.
    fn decode_forecast(
        &mut self,
        bytes: &[u8],
    ) -> Result<crate::WorkerRainForecastPayload, TaskFailure> {
        NativeRainCaptureValues.decode_forecast(bytes)
    }

    /// Format the original Open-Meteo hourly source axis.
    fn format_open_meteo_hour(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        NativeRainCaptureValues.format_open_meteo_hour(epoch_ms)
    }
}

impl ApiMaintenanceReceiptCanonicalValues for NativeScheduledValues {
    /// Delegate canonical UTC admission to the approved evidence owner.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        NativeModelEvidenceValues::new().canonical_utc_epoch_milliseconds(value)
    }

    /// Delegate exact byte hashing to the approved evidence owner.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        ApiMaintenanceReceiptCanonicalValues::sha256(&NativeModelEvidenceValues::new(), bytes)
    }

    /// Delegate original receipt hashing to the approved evidence owner.
    fn unsigned_receipt_sha256(
        &self,
        fields: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        NativeModelEvidenceValues::new().unsigned_receipt_sha256(fields)
    }
}

impl ApiMaintenanceShadowEvidenceValues for NativeScheduledValues {
    /// Decode source JSON through the genuine model evidence owner.
    fn decode_shadow_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        NativeModelEvidenceValues::new().decode_shadow_json(bytes)
    }

    /// Encode source-ordered JSON through the genuine model evidence owner.
    fn encode_shadow_ordered_json(&self, document: &JsonValue) -> Option<Vec<u8>> {
        NativeModelEvidenceValues::new().encode_shadow_ordered_json(document)
    }

    /// Decode canonical comparator base64 through the genuine model evidence owner.
    fn decode_shadow_comparator_base64(&self, encoded: &[u16]) -> Option<Vec<u8>> {
        NativeModelEvidenceValues::new().decode_shadow_comparator_base64(encoded)
    }

    /// Encode recursively sorted JSON through the genuine model evidence owner.
    fn encode_shadow_sorted_json(&self, document: &JsonValue) -> Option<Vec<u8>> {
        NativeModelEvidenceValues::new().encode_shadow_sorted_json(document)
    }

    /// Apply the installed model value filter through its actual owner.
    fn check_legacy_wind_runtime_value(&self, code_units: &[u16]) -> Option<()> {
        ApiMaintenanceShadowEvidenceValues::check_legacy_wind_runtime_value(
            &NativeModelEvidenceValues::new(),
            code_units,
        )
    }
}

impl WorkerRainFixedGaugeIdentityValues for NativeScheduledValues {
    /// Hash the exact normalized weather content through scheduled production values.
    fn fixed_gauge_record_content_sha256(
        &self,
        record: &NormalizedWeatherRecord,
    ) -> Option<Sha256Digest> {
        fixed_gauge_record_content_sha256(record)
    }
}

impl ApiMaintenancePredictionValues for NativeScheduledValues {
    type Error = NativeApiCaptureValueError;

    /// Parse prediction clocks through the actual native prediction codec.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error> {
        ApiMaintenancePredictionValues::utc_epoch_milliseconds(&mut NativeApiCaptureCodec, value)
    }

    /// Encode the exact compact prediction bind through the actual native prediction codec.
    fn canonical_prediction_json(
        &mut self,
        prediction: &weather_contracts::AdjustmentShadowPrediction,
    ) -> Result<SerializedJson, Self::Error> {
        ApiMaintenancePredictionValues::canonical_prediction_json(
            &mut NativeApiCaptureCodec,
            prediction,
        )
    }
}

impl RainAdjustmentTimeAdapter for NativeScheduledValues {
    /// Format the shadow due clock through the canonical native UTC owner.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        NativeWirePrimitives
            .format_utc_milliseconds(epoch_ms)
            .ok_or_else(|| failure("rain_adjustment_time_invalid"))
    }
}

/// Construct one independently owned private HTTP/JSON adapter over the shared client.
fn native_rain_http(
    client: Arc<NativeOutboundHttp>,
    request_timeout: Duration,
) -> NativeWorkerRainHttp {
    NativeWorkerRainMaintenanceHttpJson::new(
        NativeModelEvidenceValues::new(),
        NativePrivatePostTransport::new(client, request_timeout),
    )
}

/// Hash normalized weather content through the production scheduled value contract.
fn fixed_gauge_record_content_sha256(record: &NormalizedWeatherRecord) -> Option<Sha256Digest> {
    crate::OpenMeteoRuntimeValues::weather_record_content_hash(
        &mut NativeScheduledValues::new(),
        record,
    )
    .ok()
}

/// Encode one public-safe serving hour in the predecessor's insertion order.
fn rain_hour_value(hour: &RainAdjustmentHour) -> Option<JsonValue> {
    // reject values that JSON.stringify or the generated contract cannot preserve
    if hour.modelLeadHours > 9_007_199_254_740_991
        || !hour.rawPrecipitationMm.is_finite()
        || !hour.correctedPrecipitationMm.is_finite()
    {
        return None;
    }
    Some(object(vec![
        ("applied", JsonValue::Boolean(hour.applied)),
        (
            "correctedPrecipitationMm",
            JsonValue::Number(hour.correctedPrecipitationMm),
        ),
        (
            "modelLeadHours",
            JsonValue::Number(hour.modelLeadHours as f64),
        ),
        (
            "rawPrecipitationMm",
            JsonValue::Number(hour.rawPrecipitationMm),
        ),
        ("reasonCode", nullable_text(&hour.reasonCode)),
        ("validAt", text(&hour.validAt)),
    ]))
}

/// Project the generated fixed-gauge document into the canonical JSON value grammar.
fn fixed_gauge_projection_value(
    projection: &AdjustmentRainFixedGaugeTargetProjection,
) -> Option<JsonValue> {
    Some(object(vec![
        (
            "captureBodies",
            JsonValue::Array(
                projection
                    .captureBodies
                    .iter()
                    .map(|body| {
                        Some(object(vec![
                            ("bodyBase64", text(&body.bodyBase64)),
                            ("bodySha256", text(&body.bodySha256)),
                            (
                                "claims",
                                JsonValue::Array(
                                    body.claims
                                        .iter()
                                        .map(|claim| {
                                            Some(object(vec![
                                                ("claimId", text(&claim.claimId)),
                                                ("completedAt", text(&claim.completedAt)),
                                                ("stationId", safe_integer(claim.stationId)?),
                                                (
                                                    "windowEndExclusive",
                                                    text(&claim.windowEndExclusive),
                                                ),
                                                ("windowStart", text(&claim.windowStart)),
                                            ]))
                                        })
                                        .collect::<Option<Vec<_>>>()?,
                                ),
                            ),
                        ]))
                    })
                    .collect::<Option<Vec<_>>>()?,
            ),
        ),
        (
            "contractVersion",
            text("adjustment-rain-fixed-gauge-target-projection/v1"),
        ),
        ("family", text("rain")),
        ("logicalReceivedAt", text(&projection.logicalReceivedAt)),
        ("projectionKind", text("target_revision")),
        (
            "rows",
            JsonValue::Array(
                projection
                    .rows
                    .iter()
                    .map(|row| {
                        Some(object(vec![
                            (
                                "captureMembers",
                                JsonValue::Array(
                                    row.captureMembers
                                        .iter()
                                        .map(|member| {
                                            object(vec![
                                                ("bodySha256", text(&member.bodySha256)),
                                                ("claimId", text(&member.claimId)),
                                                ("completedAt", text(&member.completedAt)),
                                            ])
                                        })
                                        .collect(),
                                ),
                            ),
                            (
                                "intervals",
                                JsonValue::Array(
                                    row.intervals
                                        .iter()
                                        .map(|interval| {
                                            Some(object(vec![
                                                ("bodySha256", text(&interval.bodySha256)),
                                                ("claimId", text(&interval.claimId)),
                                                ("completedAt", text(&interval.completedAt)),
                                                (
                                                    "durationMinutes",
                                                    safe_integer(interval.durationMinutes)?,
                                                ),
                                                (
                                                    "precipitationMm64",
                                                    text(&interval.precipitationMm64),
                                                ),
                                                (
                                                    "temperatureC64",
                                                    nullable_text(&interval.temperatureC64),
                                                ),
                                                ("validAt", text(&interval.validAt)),
                                            ]))
                                        })
                                        .collect::<Option<Vec<_>>>()?,
                                ),
                            ),
                            (
                                "normalizedRecord",
                                normalized_record_value(&row.normalizedRecord)?,
                            ),
                            ("stationId", safe_integer(row.stationId)?),
                            ("storedContentSha256", text(&row.storedContentSha256)),
                        ]))
                    })
                    .collect::<Option<Vec<_>>>()?,
            ),
        ),
        ("validAt", text(&projection.validAt)),
    ]))
}

/// Project one normalized weather record without dropping receipt-time evidence.
fn normalized_record_value(record: &NormalizedWeatherRecord) -> Option<JsonValue> {
    Some(object(vec![
        ("metadata", metadata_value(&record.metadata)?),
        ("metrics", metrics_value(&record.metrics)?),
        ("productRunAt", nullable_text(&record.productRunAt)),
        ("receivedAt", text(&record.receivedAt)),
        ("sourceId", text(&record.sourceId)),
        ("sourceKind", text(source_kind(record.sourceKind))),
        ("validAt", text(&record.validAt)),
    ]))
}

/// Project exact device, provider, quality, and timezone metadata.
fn metadata_value(metadata: &WeatherRecordMetadata) -> Option<JsonValue> {
    Some(object(vec![
        ("device", device_value(&metadata.device)),
        ("model", nullable_text(&metadata.model)),
        ("provider", nullable_object(&metadata.provider)?),
        ("quality", nullable_object(&metadata.quality)?),
        ("upstreamTimezone", text(&metadata.upstreamTimezone)),
    ]))
}

/// Project one nullable device while preserving omitted optional properties.
fn device_value(device: &Nullable<DeviceMetadata>) -> JsonValue {
    let Nullable::Value(device) = device else {
        return JsonValue::Null;
    };
    let mut fields = Vec::with_capacity(3);
    push_present_text(&mut fields, "model", &device.model);
    push_present_text(&mut fields, "serial", &device.serial);
    push_present_text(&mut fields, "vendor", &device.vendor);
    JsonValue::Object(fields)
}

/// Append one present generated string while retaining missing-field omission.
fn push_present_text(
    fields: &mut Vec<(Vec<u16>, JsonValue)>,
    key: &str,
    value: &FieldPresence<String>,
) {
    // preserve missing fields instead of converting them to null
    if let FieldPresence::Value(value) = value {
        fields.push(member(key, text(value)));
    }
}

/// Project one nullable ordered JSON object after finite-number admission.
fn nullable_object(value: &Nullable<Vec<(Vec<u16>, JsonValue)>>) -> Option<JsonValue> {
    match value {
        Nullable::Null => Some(JsonValue::Null),
        Nullable::Value(fields) => finite_json(&JsonValue::Object(fields.clone()))
            .then(|| JsonValue::Object(fields.clone())),
    }
}

/// Project all normalized weather metrics in their generated contract shape.
fn metrics_value(metrics: &CanonicalWeatherMetrics) -> Option<JsonValue> {
    Some(object(vec![
        (
            "apparentTemperatureC",
            nullable_number(&metrics.apparentTemperatureC)?,
        ),
        (
            "blackGlobeTemperatureC",
            nullable_number(&metrics.blackGlobeTemperatureC)?,
        ),
        (
            "cloudCoverPercent",
            nullable_number(&metrics.cloudCoverPercent)?,
        ),
        (
            "pm25MicrogramsPerCubicMeter",
            nullable_number(&metrics.pm25MicrogramsPerCubicMeter)?,
        ),
        (
            "precipitationMm",
            nullable_number(&metrics.precipitationMm)?,
        ),
        (
            "precipitationRateMmPerHour",
            nullable_number(&metrics.precipitationRateMmPerHour)?,
        ),
        ("pressureHpa", nullable_number(&metrics.pressureHpa)?),
        (
            "relativeHumidityPercent",
            nullable_number(&metrics.relativeHumidityPercent)?,
        ),
        (
            "soilElectricalConductivityMicrosiemensPerCm",
            nullable_number(&metrics.soilElectricalConductivityMicrosiemensPerCm)?,
        ),
        (
            "soilMoisturePercent",
            nullable_number(&metrics.soilMoisturePercent)?,
        ),
        (
            "solarRadiationWm2",
            nullable_number(&metrics.solarRadiationWm2)?,
        ),
        ("temperatureC", nullable_number(&metrics.temperatureC)?),
        ("uvIndex", nullable_number(&metrics.uvIndex)?),
        ("waterLevelM", nullable_number(&metrics.waterLevelM)?),
        (
            "wetBulbGlobeTemperatureC",
            nullable_number(&metrics.wetBulbGlobeTemperatureC)?,
        ),
        (
            "windDirectionDegrees",
            nullable_number(&metrics.windDirectionDegrees)?,
        ),
        ("windGustMps", nullable_number(&metrics.windGustMps)?),
        ("windSpeedMps", nullable_number(&metrics.windSpeedMps)?),
    ]))
}

/// Preserve a nullable finite number without JavaScript non-finite coercion.
fn nullable_number(value: &Nullable<f64>) -> Option<JsonValue> {
    match value {
        Nullable::Null => Some(JsonValue::Null),
        Nullable::Value(value) => value.is_finite().then_some(JsonValue::Number(*value)),
    }
}

/// Preserve one nullable generated string.
fn nullable_text(value: &Nullable<String>) -> JsonValue {
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => text(value),
    }
}

/// Admit a generated safe unsigned integer before binary64 JSON encoding.
fn safe_integer(value: u64) -> Option<JsonValue> {
    (value <= 9_007_199_254_740_991).then_some(JsonValue::Number(value as f64))
}

/// Require finite numbers recursively in retained provider metadata.
fn finite_json(value: &JsonValue) -> bool {
    match value {
        JsonValue::Number(value) => value.is_finite(),
        JsonValue::Array(values) => values.iter().all(finite_json),
        JsonValue::Object(fields) => fields.iter().all(|(_, value)| finite_json(value)),
        JsonValue::Boolean(_) | JsonValue::String(_) | JsonValue::Null => true,
    }
}

/// Render one generated source kind with its exact wire spelling.
const fn source_kind(value: SourceKind) -> &'static str {
    match value {
        SourceKind::PhysicalSensor => "physical_sensor",
        SourceKind::ModelCurrent => "model_current",
        SourceKind::Reanalysis => "reanalysis",
        SourceKind::Forecast => "forecast",
        SourceKind::TideObservation => "tide_observation",
        SourceKind::TidePrediction => "tide_prediction",
    }
}

/// Construct one ordered JSON object from UTF-8 field names.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| member(key, value))
            .collect(),
    )
}

/// Construct one UTF-16 JSON object member.
fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// Construct one exact UTF-16 JSON string.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Keep native rain binding failures categorical and data-free.
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replay portable model months across a UTC/local month boundary.
    #[test]
    fn native_worker_rain_portable_calendar_matches_predecessor_month() {
        let mut calendar = NativeRainPortableCalendar::load().unwrap();
        assert_eq!(calendar.local_month("2026-09-01T06:59:59.999Z"), Some(8));
        assert_eq!(calendar.local_month("2026-09-01T07:00:00.000Z"), Some(9));
    }
}

#[cfg(test)]
#[path = "native_worker_rain_task_binding_test.rs"]
mod differential_tests;
