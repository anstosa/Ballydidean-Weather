//! Production native value and pool bindings for public-station startup backfill.

use jiff::civil::Date;
use weather_contracts::{FieldPresence, IngestionError, IngestionErrorClassification, JsonValue};
use weather_database::{AdmittedIngestionError, SerializedJson};
use weather_worker::{
    PublicStationBackfillDate, PublicStationBackfillErrorClassification,
    PublicStationBackfillOperationError, PublicStationBackfillProviderFailure,
    ScheduledSourceMaterialComparison, TaskFailure,
};

use crate::{
    AmbientBackfillPersistenceOperation, AmbientBackfillRuntimeValues,
    DatabaseAmbientBackfillRepository, NativePostgresPool, NativePostgresWorkerPool,
    NativeRuntimeMaterial, NativeRuntimeMaterialError, NativeScheduledValues,
    OpenMeteoRuntimeValues, PublicStationStartupPool, native_worker_shared_pool,
};

const DATE_INVALID: &str = "native_backfill_date_invalid";

impl AmbientBackfillRuntimeValues for NativeScheduledValues {
    /// Admit one exact Gregorian date and its UTC-midnight witness.
    fn ambient_backfill_date(
        &mut self,
        value: &str,
    ) -> Result<PublicStationBackfillDate, TaskFailure> {
        let date = value
            .parse::<Date>()
            .map_err(|_| task_failure(DATE_INVALID))?;
        // reject Jiff-normalized or expanded spellings
        if date.to_string() != value {
            return Err(task_failure(DATE_INVALID));
        }
        let timestamp = jiff::tz::TimeZone::UTC
            .to_timestamp(date.at(0, 0, 0, 0))
            .map_err(|_| task_failure(DATE_INVALID))?;
        Ok(PublicStationBackfillDate::admitted(
            value,
            timestamp.as_millisecond(),
        ))
    }

    /// Serialize the exact producer-ordered Ambient request evidence.
    fn ambient_backfill_request_metadata_json(
        &mut self,
        endpoint: &str,
        station_key: &str,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        OpenMeteoRuntimeValues::serialize_json_object(
            self,
            &[member("endpoint", endpoint), member("station", station_key)],
        )
        .map(Some)
    }

    /// Preserve the provider's admitted classification and bounded evidence.
    fn ambient_backfill_ingestion_error(
        &mut self,
        failure: &PublicStationBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        let classification = match failure.error.classification {
            PublicStationBackfillErrorClassification::Retryable => {
                IngestionErrorClassification::Retryable
            }
            PublicStationBackfillErrorClassification::RateLimited => {
                IngestionErrorClassification::RateLimited
            }
            PublicStationBackfillErrorClassification::Permanent => {
                IngestionErrorClassification::Permanent
            }
            PublicStationBackfillErrorClassification::InvalidPayload => {
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

    /// Map each persistence boundary to one stable source-owned task code.
    fn ambient_backfill_task_failure(
        &mut self,
        operation: AmbientBackfillPersistenceOperation,
        _message: &str,
    ) -> TaskFailure {
        task_failure(operation_code(operation))
    }

    /// Preserve operation identity and the database message inside the committed boundary.
    fn ambient_backfill_operation_failure(
        &mut self,
        operation: AmbientBackfillPersistenceOperation,
        message: &str,
    ) -> PublicStationBackfillOperationError {
        PublicStationBackfillOperationError::unexpected(format!(
            "{}: {message}",
            operation_code(operation)
        ))
    }
}

impl ScheduledSourceMaterialComparison for NativeScheduledValues {
    type Error = NativeRuntimeMaterialError;

    /// Compare canonical source values through the shared native JSON owner.
    fn values_equal(&mut self, left: &JsonValue, right: &JsonValue) -> Result<bool, Self::Error> {
        NativeRuntimeMaterial.values_equal(left, right)
    }

    /// Compare one source value with its producer-ordered object projection.
    fn value_and_object_equal(
        &mut self,
        value: &JsonValue,
        object: &[(Vec<u16>, JsonValue)],
    ) -> Result<bool, Self::Error> {
        NativeRuntimeMaterial.value_and_object_equal(value, object)
    }
}

impl PublicStationStartupPool for NativePostgresWorkerPool {
    type Repository = DatabaseAmbientBackfillRepository<NativePostgresPool, NativeScheduledValues>;

    /// Share the retained native pool and bind a zero-state production value owner.
    fn share_startup_repository(&mut self) -> Result<Self::Repository, TaskFailure> {
        Ok(DatabaseAmbientBackfillRepository::new(
            native_worker_shared_pool(self)?,
            NativeScheduledValues::new(),
        ))
    }
}

/// Construct one producer-ordered string member.
fn member(key: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

/// Map one Ambient persistence boundary to its stable failure code.
const fn operation_code(operation: AmbientBackfillPersistenceOperation) -> &'static str {
    // keep database failures aligned with the shared scheduled persistence vocabulary
    match operation {
        AmbientBackfillPersistenceOperation::SourceAssociation => {
            "public_station_source_association"
        }
        AmbientBackfillPersistenceOperation::ResumeRead => "persistence_checkpoint_read",
        AmbientBackfillPersistenceOperation::Acquire => "source_session_acquire",
        AmbientBackfillPersistenceOperation::Abandon => "abandon_expired_runs",
        AmbientBackfillPersistenceOperation::Start => "persistence_start",
        AmbientBackfillPersistenceOperation::Complete => "persistence_complete",
        AmbientBackfillPersistenceOperation::FailureFinalize => "persistence_failure_finalize",
        AmbientBackfillPersistenceOperation::Release => "source_session_release",
    }
}

/// Construct one zero-attempt native worker failure.
fn task_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
