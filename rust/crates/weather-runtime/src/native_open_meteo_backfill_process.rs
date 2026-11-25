//! Installed Open-Meteo historical backfill process over native configuration and transports.

use std::fmt::{self, Display, Formatter};
use std::io::{self, Write};
use std::sync::Arc;

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use weather_contracts::{
    BackfillChunkIdentity, BackfillChunkReport, BackfillChunkReportStatusEnum, BackfillReport,
    BackfillReportExitCodeEnum, FieldPresence, IngestionError, IngestionErrorClassification,
    JsonValue, NormalizedForecastAnchorRecord,
    NormalizedForecastAnchorRecordContractVersionLiteral,
    NormalizedForecastAnchorRecordDatasetLiteral, NormalizedForecastAnchorRecordSourceKindLiteral,
    NormalizedForecastAnchorRecordUpstreamModelLiteral, Nullable,
};
use weather_database::{
    AdmittedIngestionError, ForecastAnchorRecordStorageEvidence, SerializedJson, Sha256Digest,
    to_pool_configuration,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_providers::ProviderControls;
use weather_worker::{
    ForecastAnchorBackfillDate, ForecastAnchorBackfillDeadlines,
    ForecastAnchorBackfillOperationError, ForecastAnchorBackfillProviderFailure,
    ForecastAnchorBackfillTimeAdapter, OpenMeteoArchiveCleanupFailure,
    PublicStationBackfillTimeAdapter, TaskFailure, TideBackfillErrorClassification,
    TideBackfillOperationError, TideBackfillProviderFailure, WorkerProcessEnvironmentPort,
};

use crate::native_scheduled_values::{metadata_value, metrics_value};
use crate::{
    DatabaseForecastAnchorBackfillRepository, DatabaseOpenMeteoArchiveBackfillRepository,
    ForecastAnchorBackfillRuntimeValues, ForecastAnchorPersistenceOperation,
    NativeOpenMeteoTransport, NativeOpenMeteoValues, NativeOutboundHttp, NativePublicBackfillTime,
    NativeRuntimeMaterial, NativeScheduledValues, NativeWirePrimitives,
    OpenMeteoArchiveBackfillRuntimeValues, OpenMeteoArchiveOperation,
    OpenMeteoArchivePersistenceOperation, OpenMeteoBackfillApplicationError,
    OpenMeteoBackfillApplicationPort, OpenMeteoPreviousRunsOperation, OpenMeteoRuntimeValues,
    RuntimeTimeValue, WorkerConfiguration, native_worker_configuration_loader,
    run_open_meteo_backfill_application,
};

/// Actual Archive provider retained by the native one-shot process.
pub type NativeOpenMeteoArchiveProcessProvider =
    OpenMeteoArchiveOperation<NativeOpenMeteoTransport, NativeOpenMeteoValues>;

/// Actual Previous Runs provider retained by the native one-shot process.
pub type NativeOpenMeteoPreviousRunsProcessProvider =
    OpenMeteoPreviousRunsOperation<NativeOpenMeteoTransport, NativeOpenMeteoValues>;

/// Bounded native startup or application failure for the standalone command.
#[derive(Debug)]
pub enum NativeOpenMeteoBackfillProcessError {
    Transport,
    Application(OpenMeteoBackfillApplicationError),
}

impl Display for NativeOpenMeteoBackfillProcessError {
    /// Render only stable process failure categories without provider or database material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport => formatter.write_str("native Open-Meteo transport failed to start"),
            Self::Application(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for NativeOpenMeteoBackfillProcessError {}

/// Process owner sharing one HTTP runtime, one configuration load, and one output sink.
pub struct NativeOpenMeteoBackfillProcess<W> {
    client: Arc<NativeOutboundHttp>,
    output: W,
}

impl<W: Write> NativeOpenMeteoBackfillProcess<W> {
    /// Start the retained outbound runtime without opening configuration or PostgreSQL.
    pub fn new(output: W) -> Result<Self, NativeOpenMeteoBackfillProcessError> {
        let client = NativeOutboundHttp::new()
            .map(Arc::new)
            .map_err(|_| NativeOpenMeteoBackfillProcessError::Transport)?;
        Ok(Self { client, output })
    }

    /// Recover the output sink after direct fixture execution.
    pub fn into_output(self) -> W {
        self.output
    }
}

/// Execute one actual native Open-Meteo historical backfill and write its machine report.
pub fn run_native_open_meteo_backfill(
    arguments: &[String],
) -> Result<u8, NativeOpenMeteoBackfillProcessError> {
    run_native_open_meteo_backfill_with_output(arguments, io::stdout().lock())
}

/// Execute one actual backfill with an injected output sink for ordinary process verification.
pub fn run_native_open_meteo_backfill_with_output(
    arguments: &[String],
    output: impl Write,
) -> Result<u8, NativeOpenMeteoBackfillProcessError> {
    let mut process = NativeOpenMeteoBackfillProcess::new(output)?;
    let mut command_time = NativePublicBackfillTime::default();
    let mut material = NativeRuntimeMaterial;
    run_open_meteo_backfill_application(arguments, &mut process, &mut command_time, &mut material)
        .map_err(NativeOpenMeteoBackfillProcessError::Application)
}

/// Previous Runs calendar adapter reusing the already admitted public-backfill calendar.
#[derive(Debug, Default)]
pub struct NativeOpenMeteoPreviousRunsTime(NativePublicBackfillTime);

impl ForecastAnchorBackfillTimeAdapter for NativeOpenMeteoPreviousRunsTime {
    /// Return the same live UTC date used by every native historical process.
    fn today_utc(&mut self) -> Result<ForecastAnchorBackfillDate, TaskFailure> {
        PublicStationBackfillTimeAdapter::today_utc(&mut self.0)
    }

    /// Add exact UTC civil days through the shared native calendar.
    fn add_utc_days(
        &mut self,
        date: &ForecastAnchorBackfillDate,
        days: i64,
    ) -> Result<ForecastAnchorBackfillDate, TaskFailure> {
        PublicStationBackfillTimeAdapter::add_utc_days(&mut self.0, date, days)
    }

    /// Reuse the same provider and committed-run deadline plan.
    fn deadlines(
        &mut self,
    ) -> Result<ForecastAnchorBackfillDeadlines, ForecastAnchorBackfillOperationError> {
        PublicStationBackfillTimeAdapter::deadlines(&mut self.0)
    }
}

impl<W: Write> OpenMeteoBackfillApplicationPort for NativeOpenMeteoBackfillProcess<W> {
    type Pool = crate::NativePostgresPool;
    type ArchiveProvider = NativeOpenMeteoArchiveProcessProvider;
    type PreviousRunsProvider = NativeOpenMeteoPreviousRunsProcessProvider;
    type ArchiveTime = NativePublicBackfillTime;
    type PreviousRunsTime = NativeOpenMeteoPreviousRunsTime;
    type ArchiveRepository<'a>
        = DatabaseOpenMeteoArchiveBackfillRepository<
        &'a mut crate::NativePostgresPool,
        NativeScheduledValues,
    >
    where
        Self: 'a;
    type PreviousRunsRepository<'a>
        = DatabaseForecastAnchorBackfillRepository<
        &'a mut crate::NativePostgresPool,
        NativeScheduledValues,
    >
    where
        Self: 'a;

    /// Load every original catalog and compatibility origin through the native process owner.
    fn load_configuration(&mut self) -> Result<WorkerConfiguration, TaskFailure> {
        native_worker_configuration_loader()
            .load_configuration()
            .map_err(|_| failure("worker_configuration_failed"))
    }

    /// Open one lazy native PostgreSQL pool from the checked worker database configuration.
    fn open_pool(
        &mut self,
        configuration: &WorkerConfiguration,
    ) -> Result<Self::Pool, TaskFailure> {
        crate::NativePostgresPool::new(to_pool_configuration(&configuration.database))
            .map_err(|_| failure("open_meteo_backfill_pool_failed"))
    }

    /// Bind Archive to the process-shared HTTP client and admitted compatibility origin.
    fn create_archive_provider(
        &mut self,
        configuration: &WorkerConfiguration,
    ) -> Self::ArchiveProvider {
        OpenMeteoArchiveOperation::new(
            NativeOpenMeteoTransport::with_shared_client(Arc::clone(&self.client)),
            NativeOpenMeteoValues::new(),
            configuration.open_meteo_compatibility_origin.clone(),
            ProviderControls::default(),
        )
    }

    /// Bind Previous Runs to the same client, retry controls, and compatibility origin.
    fn create_previous_runs_provider(
        &mut self,
        configuration: &WorkerConfiguration,
    ) -> Self::PreviousRunsProvider {
        OpenMeteoPreviousRunsOperation::new(
            NativeOpenMeteoTransport::with_shared_client(Arc::clone(&self.client)),
            NativeOpenMeteoValues::new(),
            configuration.open_meteo_compatibility_origin.clone(),
            ProviderControls::default(),
        )
    }

    /// Construct the site-local Archive calendar and shared deadline owner.
    fn create_archive_time(&mut self) -> Self::ArchiveTime {
        NativePublicBackfillTime::default()
    }

    /// Construct the UTC Previous Runs calendar and shared deadline owner.
    fn create_previous_runs_time(&mut self) -> Self::PreviousRunsTime {
        NativeOpenMeteoPreviousRunsTime::default()
    }

    /// Verify the same PostgreSQL feature floor as the other native historical commands.
    fn assert_supported_database(&mut self, pool: &mut Self::Pool) -> Result<(), TaskFailure> {
        crate::native_tempest_backfill_process::assert_supported_database(pool)
    }

    /// Collapse a source read into the predecessor's bounded process diagnostic.
    fn source_read_failure(
        &mut self,
        _error: <Self::Pool as weather_database::OpenMeteoBackfillSourceReadPort>::Error,
    ) -> TaskFailure {
        failure("open_meteo_backfill_source_read_failed")
    }

    /// Bind weather-record persistence to the application-owned pool.
    fn archive_repository<'a>(
        &'a mut self,
        pool: &'a mut Self::Pool,
    ) -> Self::ArchiveRepository<'a> {
        DatabaseOpenMeteoArchiveBackfillRepository::new(pool, NativeScheduledValues::new())
    }

    /// Bind fixed-anchor persistence to the same application-owned pool.
    fn previous_runs_repository<'a>(
        &'a mut self,
        pool: &'a mut Self::Pool,
    ) -> Self::PreviousRunsRepository<'a> {
        DatabaseForecastAnchorBackfillRepository::new(pool, NativeScheduledValues::new())
    }

    /// Preserve JSON.stringify pretty order and the terminating line feed.
    fn serialize_report(&mut self, report: &BackfillReport) -> Result<Vec<u8>, TaskFailure> {
        serialize_backfill_report(report)
    }

    /// Write the exact serialized report bytes to the injected process sink.
    fn write_stdout(&mut self, contents: &[u8]) -> Result<(), TaskFailure> {
        self.output
            .write_all(contents)
            .map_err(|_| failure("open_meteo_backfill_stdout_failed"))
    }

    /// Close the process pool after output, preserving finally-error precedence.
    fn close_pool(&mut self, pool: Self::Pool) -> Result<(), TaskFailure> {
        pool.close()
            .map_err(|_| failure("open_meteo_backfill_pool_close_failed"))
    }
}

impl OpenMeteoArchiveBackfillRuntimeValues for NativeScheduledValues {
    /// Preserve the provider classification, code, message, and exact attempts separately.
    fn open_meteo_archive_ingestion_error(
        &mut self,
        failure: &TideBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        admitted_ingestion_error(failure)
    }

    /// Classify one pool failure outside the committed chunk boundary.
    fn open_meteo_archive_task_failure(
        &mut self,
        operation: OpenMeteoArchivePersistenceOperation,
        _message: &str,
    ) -> TaskFailure {
        failure(archive_persistence_code(operation))
    }

    /// Classify one pool failure inside the original committed chunk catch boundary.
    fn open_meteo_archive_operation_failure(
        &mut self,
        operation: OpenMeteoArchivePersistenceOperation,
        _message: &str,
    ) -> TideBackfillOperationError {
        TideBackfillOperationError::unexpected(archive_persistence_code(operation))
    }

    /// Retain one already-redacted cleanup diagnostic in the machine report.
    fn open_meteo_archive_cleanup_failure(
        &mut self,
        operation: OpenMeteoArchivePersistenceOperation,
        _message: &str,
    ) -> OpenMeteoArchiveCleanupFailure {
        OpenMeteoArchiveCleanupFailure::admitted(archive_persistence_code(operation).to_owned())
    }
}

impl ForecastAnchorBackfillRuntimeValues for NativeScheduledValues {
    /// Format one exact persistence instant through the existing native UTC owner.
    fn utc_instant(
        &mut self,
        epoch_milliseconds: i64,
        value: crate::ForecastAnchorTimeValue,
    ) -> Result<String, TaskFailure> {
        OpenMeteoRuntimeValues::utc_instant(self, epoch_milliseconds, runtime_time(value))
    }

    /// Admit one exact lowercase response or configuration digest.
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure> {
        OpenMeteoRuntimeValues::sha256_digest(self, value)
    }

    /// Serialize the ordered Previous Runs request evidence without archive framing.
    fn request_metadata_json(
        &mut self,
        metadata: &JsonValue,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        serialize_object_value(self, metadata).map(Some)
    }

    /// Build exact canonical forecast-anchor content and nullable metadata evidence.
    fn record_storage_evidence(
        &mut self,
        record: &NormalizedForecastAnchorRecord,
    ) -> Result<ForecastAnchorRecordStorageEvidence, TaskFailure> {
        Ok(ForecastAnchorRecordStorageEvidence {
            content_hash: forecast_anchor_content_hash(record)?,
            provider_metadata_json: serialize_nullable_object(self, &record.metadata.provider)?,
            quality_metadata_json: serialize_nullable_object(self, &record.metadata.quality)?,
        })
    }

    /// Serialize the ordered bounded response evidence without changing its fields.
    fn response_metadata_json(
        &mut self,
        metadata: &JsonValue,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        serialize_object_value(self, metadata).map(Some)
    }

    /// Preserve the shared provider error classification and message.
    fn ingestion_error(
        &mut self,
        failure: &ForecastAnchorBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        admitted_ingestion_error(failure)
    }

    /// Classify one fixed-anchor persistence failure outside the committed chunk boundary.
    fn task_failure(
        &mut self,
        operation: ForecastAnchorPersistenceOperation,
        _message: &str,
    ) -> TaskFailure {
        failure(forecast_persistence_code(operation))
    }

    /// Classify one fixed-anchor persistence failure inside the committed chunk boundary.
    fn operation_failure(
        &mut self,
        operation: ForecastAnchorPersistenceOperation,
        _message: &str,
    ) -> ForecastAnchorBackfillOperationError {
        ForecastAnchorBackfillOperationError::unexpected(forecast_persistence_code(operation))
    }

    /// Retain one already-redacted cleanup diagnostic in the machine report.
    fn cleanup_failure(
        &mut self,
        operation: ForecastAnchorPersistenceOperation,
        _message: &str,
    ) -> OpenMeteoArchiveCleanupFailure {
        OpenMeteoArchiveCleanupFailure::admitted(forecast_persistence_code(operation).to_owned())
    }
}

/// Serialize one generated report in the original insertion order.
fn serialize_backfill_report(report: &BackfillReport) -> Result<Vec<u8>, TaskFailure> {
    let mut bytes = serde_json::to_vec_pretty(&Report(report))
        .map_err(|_| failure("open_meteo_backfill_report_encoding_failed"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Borrow one report for explicit ordered serialization.
struct Report<'a>(&'a BackfillReport);

impl Serialize for Report<'_> {
    /// Emit exactly the eight source report fields in predecessor insertion order.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let report = self.0;
        let mut out = serializer.serialize_struct("BackfillReport", 8)?;
        out.serialize_field(
            "chunks",
            &report.chunks.iter().map(Chunk).collect::<Vec<_>>(),
        )?;
        out.serialize_field("dryRun", &report.dryRun)?;
        out.serialize_field("exitCode", &report_exit_code(report.exitCode))?;
        out.serialize_field("from", &report.from)?;
        out.serialize_field("resume", &report.resume)?;
        out.serialize_field("site", &report.site)?;
        out.serialize_field("source", &report.source)?;
        out.serialize_field("to", &report.to)?;
        out.end()
    }
}

/// Borrow one chunk report for explicit ordered serialization.
struct Chunk<'a>(&'a BackfillChunkReport);

impl Serialize for Chunk<'_> {
    /// Emit exact nullable diagnostics, identity, and categorical status order.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let chunk = self.0;
        let mut out = serializer.serialize_struct("BackfillChunkReport", 4)?;
        out.serialize_field("errorCode", &nullable_string(&chunk.errorCode))?;
        out.serialize_field("identity", &Identity(&chunk.identity))?;
        out.serialize_field("secondaryError", &nullable_string(&chunk.secondaryError))?;
        out.serialize_field("status", status_name(chunk.status))?;
        out.end()
    }
}

/// Borrow one generated chunk identity for explicit field order.
struct Identity<'a>(&'a BackfillChunkIdentity);

impl Serialize for Identity<'_> {
    /// Emit the exact eight-field chunk preimage order.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let identity = self.0;
        let mut out = serializer.serialize_struct("BackfillChunkIdentity", 8)?;
        out.serialize_field("adapterVersion", &identity.adapterVersion)?;
        out.serialize_field("chunkPlanVersion", &identity.chunkPlanVersion)?;
        out.serialize_field("intervalEndExclusive", &identity.intervalEndExclusive)?;
        out.serialize_field("intervalStart", &identity.intervalStart)?;
        out.serialize_field("requestedFromDate", &identity.requestedFromDate)?;
        out.serialize_field("requestedToDate", &identity.requestedToDate)?;
        out.serialize_field("sourceConfigFingerprint", &identity.sourceConfigFingerprint)?;
        out.serialize_field("sourceId", &identity.sourceId)?;
        out.end()
    }
}

/// Convert the closed generated exit-code enum to its wire integer.
const fn report_exit_code(value: BackfillReportExitCodeEnum) -> u8 {
    match value {
        BackfillReportExitCodeEnum::Value0 => 0,
        BackfillReportExitCodeEnum::Value1 => 1,
    }
}

/// Convert the closed generated status enum to its wire spelling.
const fn status_name(value: BackfillChunkReportStatusEnum) -> &'static str {
    match value {
        BackfillChunkReportStatusEnum::Abandoned => "abandoned",
        BackfillChunkReportStatusEnum::Completed => "completed",
        BackfillChunkReportStatusEnum::Failed => "failed",
        BackfillChunkReportStatusEnum::Planned => "planned",
        BackfillChunkReportStatusEnum::Remaining => "remaining",
        BackfillChunkReportStatusEnum::Skipped => "skipped",
    }
}

/// Borrow one generated nullable string as the equivalent serde option.
fn nullable_string(value: &Nullable<String>) -> Option<&str> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value),
    }
}

/// Serialize one ordered generated object without adding native archive line framing.
fn serialize_object_value(
    values: &mut NativeScheduledValues,
    value: &JsonValue,
) -> Result<SerializedJson, TaskFailure> {
    let JsonValue::Object(fields) = value else {
        return Err(failure("open_meteo_backfill_json_invalid"));
    };
    OpenMeteoRuntimeValues::serialize_json_object(values, fields)
}

/// Serialize one nullable generated metadata object.
fn serialize_nullable_object(
    values: &mut NativeScheduledValues,
    value: &Nullable<Vec<(Vec<u16>, JsonValue)>>,
) -> Result<Option<SerializedJson>, TaskFailure> {
    match value {
        Nullable::Null => Ok(None),
        Nullable::Value(fields) => {
            OpenMeteoRuntimeValues::serialize_json_object(values, fields).map(Some)
        }
    }
}

/// Hash the exact canonical forecastAnchorRecordContent preimage.
fn forecast_anchor_content_hash(
    record: &NormalizedForecastAnchorRecord,
) -> Result<Sha256Digest, TaskFailure> {
    let lead_hours = record
        .leadHours
        .wire_value()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| failure("open_meteo_backfill_record_invalid"))?;
    let value = JsonValue::Object(vec![
        member("adapterVersion", string(&record.adapterVersion)),
        member("contractEpoch", string(&record.contractEpoch)),
        member(
            "contractVersion",
            string(NormalizedForecastAnchorRecordContractVersionLiteral::WIRE_VALUE),
        ),
        member(
            "dataset",
            string(NormalizedForecastAnchorRecordDatasetLiteral::WIRE_VALUE),
        ),
        member("leadHours", JsonValue::Number(f64::from(lead_hours))),
        member("metadata", metadata_value(&record.metadata)),
        member("metrics", metrics_value(&record.metrics)),
        member(
            "sourceConfigFingerprint",
            string(&record.sourceConfigFingerprint),
        ),
        member("sourceId", string(&record.sourceId)),
        member(
            "sourceKind",
            string(NormalizedForecastAnchorRecordSourceKindLiteral::WIRE_VALUE),
        ),
        member(
            "upstreamModel",
            string(NormalizedForecastAnchorRecordUpstreamModelLiteral::WIRE_VALUE),
        ),
        member("validAt", string(&record.validAt)),
    ]);
    let mut bytes = NativeWirePrimitives
        .encode_sorted_json(&value)
        .ok_or_else(|| failure("open_meteo_backfill_record_invalid"))?;
    // remove the native LF framing absent from forecastAnchorRecordContent
    if bytes.pop() != Some(b'\n') {
        return Err(failure("open_meteo_backfill_record_invalid"));
    }
    let digest = RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, &bytes);
    Ok(Sha256Digest::admitted(lower_hex(digest)))
}

/// Map one fixed-lead enum back to the generated integer wire value.
trait FixedLeadWireValue {
    /// Return the exact generated integer associated with this closed enum.
    fn wire_value(self) -> Option<u64>;
}

impl FixedLeadWireValue for weather_contracts::FixedForecastLeadHours {
    /// Recover the wire integer from the generated source-of-truth table.
    fn wire_value(self) -> Option<u64> {
        weather_contracts::FixedForecastLeadHours::WIRE_VALUES
            .iter()
            .find_map(|(wire, value)| (*value == self).then_some(*wire))
    }
}

/// Build one UTF-16 JSON object member without changing source field order.
fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// Build one generated JSON string value.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Render one SHA-256 digest with the predecessor's lowercase spelling.
fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    // preserve every digest byte in big-endian display order
    for byte in digest {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}

/// Admit one provider failure without losing its exact durable classification or message.
fn admitted_ingestion_error(
    failure: &TideBackfillProviderFailure,
) -> Result<AdmittedIngestionError, TaskFailure> {
    // require the same bounded code shape accepted by the scheduled value owner
    OpenMeteoRuntimeValues::ingestion_error(
        &mut NativeScheduledValues::new(),
        &TaskFailure {
            attempts: failure.attempts,
            code: failure.error.code.clone(),
        },
    )?;
    let classification = match failure.error.classification {
        TideBackfillErrorClassification::Retryable => IngestionErrorClassification::Retryable,
        TideBackfillErrorClassification::RateLimited => IngestionErrorClassification::RateLimited,
        TideBackfillErrorClassification::Permanent => IngestionErrorClassification::Permanent,
        TideBackfillErrorClassification::InvalidPayload => {
            IngestionErrorClassification::InvalidPayload
        }
    };
    Ok(AdmittedIngestionError::admitted(IngestionError {
        classification,
        code: failure.error.code.clone(),
        message: failure.error.message.clone(),
        metadata: FieldPresence::Missing,
    }))
}

/// Map Archive persistence roles to the existing scheduled value boundary where possible.
const fn archive_persistence_code(operation: OpenMeteoArchivePersistenceOperation) -> &'static str {
    match operation {
        OpenMeteoArchivePersistenceOperation::ResumeRead => "open_meteo_archive_resume_read_failed",
        OpenMeteoArchivePersistenceOperation::Acquire => "open_meteo_archive_acquire_failed",
        OpenMeteoArchivePersistenceOperation::Abandon => "open_meteo_archive_abandon_failed",
        OpenMeteoArchivePersistenceOperation::Start => "open_meteo_archive_start_failed",
        OpenMeteoArchivePersistenceOperation::Complete => "open_meteo_archive_complete_failed",
        OpenMeteoArchivePersistenceOperation::FailureFinalize => {
            "open_meteo_archive_failure_finalize_failed"
        }
        OpenMeteoArchivePersistenceOperation::Release => "open_meteo_archive_release_failed",
    }
}

/// Map Previous Runs persistence roles to stable bounded process diagnostics.
const fn forecast_persistence_code(operation: ForecastAnchorPersistenceOperation) -> &'static str {
    match operation {
        ForecastAnchorPersistenceOperation::ResumeRead => {
            "open_meteo_previous_runs_resume_read_failed"
        }
        ForecastAnchorPersistenceOperation::Acquire => "open_meteo_previous_runs_acquire_failed",
        ForecastAnchorPersistenceOperation::Abandon => "open_meteo_previous_runs_abandon_failed",
        ForecastAnchorPersistenceOperation::Start => "open_meteo_previous_runs_start_failed",
        ForecastAnchorPersistenceOperation::Complete => "open_meteo_previous_runs_complete_failed",
        ForecastAnchorPersistenceOperation::FailureFinalize => {
            "open_meteo_previous_runs_failure_finalize_failed"
        }
        ForecastAnchorPersistenceOperation::Release => "open_meteo_previous_runs_release_failed",
    }
}

/// Translate fixed-anchor persistence time roles to the shared UTC value owner.
const fn runtime_time(value: crate::ForecastAnchorTimeValue) -> RuntimeTimeValue {
    match value {
        crate::ForecastAnchorTimeValue::AbandonAsOf => RuntimeTimeValue::AbandonAsOf,
        crate::ForecastAnchorTimeValue::Deadline => RuntimeTimeValue::Deadline,
        crate::ForecastAnchorTimeValue::IntervalEndExclusive => {
            RuntimeTimeValue::RequestedEndExclusive
        }
        crate::ForecastAnchorTimeValue::IntervalStart => RuntimeTimeValue::RequestedStart,
    }
}

/// Construct one stable process failure without host or provider material.
fn failure(code: &'static str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
#[path = "native_open_meteo_backfill_process_test.rs"]
mod tests;
