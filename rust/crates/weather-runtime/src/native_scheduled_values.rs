//! Production native value ownership for scheduled provider persistence.

use weather_contracts::{
    CanonicalWeatherMetrics, DeviceMetadata, FieldPresence, IngestionError,
    IngestionErrorClassification, JsonValue, NormalizedWeatherRecord, Nullable,
    ScheduledCheckpointState, SourceKind, WeatherRecordMetadata,
};
use weather_database::{AdmittedIngestionError, SerializedJson, Sha256Digest};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_worker::{
    AmbientWeatherProviderCursor, EcowittProviderCursor, NoaaTideProviderCursor, OpenMeteoCursor,
    TaskFailure, TempestProviderCursor,
};

use crate::maintenance_canonical_json::valid_hash;
use crate::{
    AmbientWeatherRuntimeValues, EcowittRuntimeValues, NativeWirePrimitives, NoaaTideRuntimeValues,
    OpenMeteoRuntimeValues, RuntimePersistenceOperation, RuntimeTimeValue, TempestRuntimeValues,
};

/// Zero-state production owner for scheduled time, JSON, cursor, digest, and error values.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeScheduledValues;

impl NativeScheduledValues {
    /// Construct the zero-state production value owner.
    pub const fn new() -> Self {
        Self
    }
}

impl OpenMeteoRuntimeValues for NativeScheduledValues {
    /// Format one supported millisecond instant in Node's fixed UTC spelling.
    fn utc_instant(
        &mut self,
        milliseconds: i64,
        _value: RuntimeTimeValue,
    ) -> Result<String, TaskFailure> {
        ArchiveWirePrimitives::format_utc_milliseconds(&NativeWirePrimitives, milliseconds)
            .ok_or_else(invalid_runtime_time)
    }

    /// Decode only an exact fixed-precision UTC instant.
    fn epoch_milliseconds(
        &mut self,
        instant: &str,
        _value: RuntimeTimeValue,
    ) -> Result<i64, TaskFailure> {
        RevisionArchiveCanonicalCodec::canonical_utc_milliseconds(&NativeWirePrimitives, instant)
            .ok_or_else(invalid_runtime_time)
    }

    /// Serialize one producer-ordered object without archive line framing.
    fn serialize_json_object(
        &mut self,
        value: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure> {
        serialize_object(value)
    }

    /// Serialize exact endpoint request evidence.
    fn request_metadata_json(&mut self, endpoint: &str) -> Result<SerializedJson, TaskFailure> {
        serialize_object(&[member("endpoint", string_value(endpoint))])
    }

    /// Serialize an Open-Meteo cursor using its product-specific timestamp key.
    fn provider_cursor_json(
        &mut self,
        cursor: Option<&OpenMeteoCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        // preserve the absent provider cursor
        let Some(cursor) = cursor else {
            return Ok(None);
        };
        let (key, milliseconds, value) = match cursor {
            // preserve current-observation validity cursors
            OpenMeteoCursor::ValidAt(milliseconds) => {
                ("valid_at", *milliseconds, RuntimeTimeValue::ValidAt)
            }
            // preserve forecast product-run cursors
            OpenMeteoCursor::ProductRunAt(milliseconds) => (
                "product_run_at",
                *milliseconds,
                RuntimeTimeValue::ProductRunAt,
            ),
        };
        let instant = self.utc_instant(milliseconds, value)?;
        serialize_object(&[member(key, string_value(&instant))]).map(Some)
    }

    /// Decode an exact Open-Meteo cursor object.
    fn checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<OpenMeteoCursor>, TaskFailure> {
        // preserve a null checkpoint cursor
        let Nullable::Value(fields) = &checkpoint.providerCursor else {
            return Ok(None);
        };
        // decode the closed one-field cursor alternatives
        if exact_keys(fields, &["valid_at"]) {
            let instant = cursor_text(fields, "valid_at")?;
            return self
                .epoch_milliseconds(&instant, RuntimeTimeValue::ValidAt)
                .map(OpenMeteoCursor::ValidAt)
                .map(Some);
        }
        // retain the independent forecast cursor contract
        if exact_keys(fields, &["product_run_at"]) {
            let instant = cursor_text(fields, "product_run_at")?;
            return self
                .epoch_milliseconds(&instant, RuntimeTimeValue::ProductRunAt)
                .map(OpenMeteoCursor::ProductRunAt)
                .map(Some);
        }
        Err(invalid_provider_cursor())
    }

    /// Hash the exact canonical weatherRecordContent preimage.
    fn weather_record_content_hash(
        &mut self,
        record: &NormalizedWeatherRecord,
    ) -> Result<Sha256Digest, TaskFailure> {
        weather_record_content_hash(record)
    }

    /// Admit only a lowercase SHA-256 spelling.
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure> {
        // reject malformed source fingerprints and response checksums
        if !valid_hash(value) {
            return Err(failure("invalid_sha256"));
        }
        Ok(Sha256Digest::admitted(value))
    }

    /// Classify one bounded worker failure with the shared ingestion categories.
    fn ingestion_error(
        &mut self,
        failure: &TaskFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        // enforce the original stable bounded error-code grammar
        if !valid_ingestion_code(&failure.code) {
            return Err(failure_code("invalid_ingestion_error"));
        }
        Ok(AdmittedIngestionError::admitted(IngestionError {
            classification: ingestion_classification(&failure.code),
            code: failure.code.clone(),
            message: failure.code.clone(),
            metadata: FieldPresence::Missing,
        }))
    }

    /// Map one typed persistence operation to a stable worker failure code.
    fn persistence_failure(
        &mut self,
        operation: RuntimePersistenceOperation,
        _message: &str,
    ) -> TaskFailure {
        failure(persistence_code(operation))
    }
}

impl TempestRuntimeValues for NativeScheduledValues {
    /// Serialize the nullable Tempest valid-at cursor.
    fn tempest_provider_cursor_json(
        &mut self,
        cursor: Option<&TempestProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        valid_at_cursor_json(self, cursor.map(|cursor| cursor.valid_at_ms))
    }

    /// Decode the nullable Tempest valid-at cursor.
    fn tempest_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<TempestProviderCursor>, TaskFailure> {
        checkpoint_valid_at(self, checkpoint)
            .map(|cursor| cursor.map(|valid_at_ms| TempestProviderCursor { valid_at_ms }))
    }
}

impl NoaaTideRuntimeValues for NativeScheduledValues {
    /// Serialize the nullable NOAA valid-at cursor.
    fn noaa_tide_provider_cursor_json(
        &mut self,
        cursor: Option<&NoaaTideProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        valid_at_cursor_json(self, cursor.map(|cursor| cursor.valid_at_ms))
    }

    /// Decode the nullable NOAA valid-at cursor.
    fn noaa_tide_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<NoaaTideProviderCursor>, TaskFailure> {
        checkpoint_valid_at(self, checkpoint)
            .map(|cursor| cursor.map(|valid_at_ms| NoaaTideProviderCursor { valid_at_ms }))
    }
}

impl AmbientWeatherRuntimeValues for NativeScheduledValues {
    /// Serialize the nullable Ambient valid-at cursor.
    fn ambient_weather_provider_cursor_json(
        &mut self,
        cursor: Option<&AmbientWeatherProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        valid_at_cursor_json(self, cursor.map(|cursor| cursor.valid_at_ms))
    }

    /// Decode the nullable Ambient valid-at cursor.
    fn ambient_weather_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<AmbientWeatherProviderCursor>, TaskFailure> {
        checkpoint_valid_at(self, checkpoint)
            .map(|cursor| cursor.map(|valid_at_ms| AmbientWeatherProviderCursor { valid_at_ms }))
    }
}

impl EcowittRuntimeValues for NativeScheduledValues {
    /// Serialize the Ecowitt rain-counter cursor with an omitted absent total.
    fn ecowitt_provider_cursor_json(
        &mut self,
        cursor: &EcowittProviderCursor,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        let mut fields = Vec::with_capacity(3);
        // include the daily total only when the gateway reported one
        if let Some(total) = cursor.rain_daily_total_mm {
            // reject non-finite values instead of JSON.stringify null coercion
            if !total.is_finite() {
                return Err(invalid_provider_cursor());
            }
            fields.push(member("rain_daily_total_mm", JsonValue::Number(total)));
        }
        fields.push(member("rain_day", string_value(&cursor.rain_day)));
        let valid_at = self.utc_instant(cursor.valid_at_ms, RuntimeTimeValue::ValidAt)?;
        fields.push(member("valid_at", string_value(&valid_at)));
        serialize_object(&fields).map(Some)
    }

    /// Decode the exact Ecowitt counter cursor while preserving an omitted total.
    fn ecowitt_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<EcowittProviderCursor, TaskFailure> {
        let Nullable::Value(fields) = &checkpoint.providerCursor else {
            return Err(invalid_provider_cursor());
        };
        let expected = cursor_field(fields, "rain_daily_total_mm")
            .map_or(&["rain_day", "valid_at"][..], |_| {
                &["rain_daily_total_mm", "rain_day", "valid_at"][..]
            });

        // reject extensions, missing fields, and duplicate cursor keys
        if !exact_keys(fields, expected) {
            return Err(invalid_provider_cursor());
        }
        let rain_daily_total_mm = cursor_field(fields, "rain_daily_total_mm")
            .map(cursor_finite_number)
            .transpose()?;
        let rain_day = cursor_text(fields, "rain_day")?;
        let valid_at = cursor_text(fields, "valid_at")?;
        let valid_at_ms = self.epoch_milliseconds(&valid_at, RuntimeTimeValue::ValidAt)?;
        Ok(EcowittProviderCursor {
            rain_daily_total_mm,
            rain_day,
            valid_at_ms,
        })
    }
}

/// Serialize one nullable valid-at cursor shared by physical providers.
fn valid_at_cursor_json(
    values: &mut NativeScheduledValues,
    valid_at_ms: Option<i64>,
) -> Result<Option<SerializedJson>, TaskFailure> {
    // preserve an empty provider response as a null cursor
    let Some(valid_at_ms) = valid_at_ms else {
        return Ok(None);
    };
    let valid_at = values.utc_instant(valid_at_ms, RuntimeTimeValue::ValidAt)?;
    serialize_object(&[member("valid_at", string_value(&valid_at))]).map(Some)
}

/// Decode one nullable valid-at cursor shared by physical providers.
fn checkpoint_valid_at(
    values: &mut NativeScheduledValues,
    checkpoint: &ScheduledCheckpointState,
) -> Result<Option<i64>, TaskFailure> {
    // preserve an uninitialized nullable cursor
    let Nullable::Value(fields) = &checkpoint.providerCursor else {
        return Ok(None);
    };
    // require the exact durable cursor object
    if !exact_keys(fields, &["valid_at"]) {
        return Err(invalid_provider_cursor());
    }
    let instant = cursor_text(fields, "valid_at")?;
    values
        .epoch_milliseconds(&instant, RuntimeTimeValue::ValidAt)
        .map(Some)
}

/// Serialize one generated object and remove the native archive line framing.
fn serialize_object(value: &[(Vec<u16>, JsonValue)]) -> Result<SerializedJson, TaskFailure> {
    let mut bytes = ArchiveWirePrimitives::encode_ordered_json(
        &NativeWirePrimitives,
        &JsonValue::Object(value.to_vec()),
    )
    .ok_or_else(invalid_runtime_json)?;

    // strip the framing byte absent from JSON.stringify output
    if bytes.pop() != Some(b'\n') {
        return Err(invalid_runtime_json());
    }
    String::from_utf8(bytes)
        .map(SerializedJson::admitted)
        .map_err(|_| invalid_runtime_json())
}

/// Calculate the exact canonical normalized weather-content digest.
fn weather_record_content_hash(
    record: &NormalizedWeatherRecord,
) -> Result<Sha256Digest, TaskFailure> {
    let value = JsonValue::Object(vec![
        member("metadata", metadata_value(&record.metadata)),
        member("metrics", metrics_value(&record.metrics)),
        member("productRunAt", nullable_string_value(&record.productRunAt)),
        member("sourceId", string_value(&record.sourceId)),
        member(
            "sourceKind",
            string_value(source_kind_wire(record.sourceKind)),
        ),
        member("validAt", string_value(&record.validAt)),
    ]);

    // canonicalizeJson rejects non-finite numbers instead of converting them to null
    if !finite_json(&value) {
        return Err(invalid_runtime_json());
    }
    let mut canonical = ArchiveWirePrimitives::encode_sorted_json(&NativeWirePrimitives, &value)
        .ok_or_else(invalid_runtime_json)?;

    // remove the native archive framing absent from weatherRecordContent
    if canonical.pop() != Some(b'\n') {
        return Err(invalid_runtime_json());
    }
    let digest = RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, &canonical);
    Ok(Sha256Digest::admitted(lower_hex(digest)))
}

/// Project generated weather metadata into its original JSON value.
pub(crate) fn metadata_value(metadata: &WeatherRecordMetadata) -> JsonValue {
    JsonValue::Object(vec![
        member("device", nullable_device_value(&metadata.device)),
        member("model", nullable_string_value(&metadata.model)),
        member("provider", nullable_object_value(&metadata.provider)),
        member("quality", nullable_object_value(&metadata.quality)),
        member("upstreamTimezone", string_value(&metadata.upstreamTimezone)),
    ])
}

/// Project one nullable device object while omitting missing fields.
fn nullable_device_value(device: &Nullable<DeviceMetadata>) -> JsonValue {
    // preserve null versus one concrete device object
    let Nullable::Value(device) = device else {
        return JsonValue::Null;
    };
    let mut fields = Vec::with_capacity(3);
    push_present_string(&mut fields, "model", &device.model);
    push_present_string(&mut fields, "serial", &device.serial);
    push_present_string(&mut fields, "vendor", &device.vendor);
    JsonValue::Object(fields)
}

/// Append one present device string without synthesizing a missing field.
fn push_present_string(
    fields: &mut Vec<(Vec<u16>, JsonValue)>,
    key: &str,
    value: &FieldPresence<String>,
) {
    // match object-property omission for generated missing values
    if let FieldPresence::Value(value) = value {
        fields.push(member(key, string_value(value)));
    }
}

/// Project one nullable ordered object.
fn nullable_object_value(value: &Nullable<Vec<(Vec<u16>, JsonValue)>>) -> JsonValue {
    // preserve null versus ordered object storage
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(fields) => JsonValue::Object(fields.clone()),
    }
}

/// Project one nullable generated string.
fn nullable_string_value(value: &Nullable<String>) -> JsonValue {
    // preserve null versus string semantics
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => string_value(value),
    }
}

/// Project the complete canonical metric set.
pub(crate) fn metrics_value(metrics: &CanonicalWeatherMetrics) -> JsonValue {
    JsonValue::Object(vec![
        member(
            "apparentTemperatureC",
            nullable_number_value(&metrics.apparentTemperatureC),
        ),
        member(
            "blackGlobeTemperatureC",
            nullable_number_value(&metrics.blackGlobeTemperatureC),
        ),
        member(
            "cloudCoverPercent",
            nullable_number_value(&metrics.cloudCoverPercent),
        ),
        member(
            "pm25MicrogramsPerCubicMeter",
            nullable_number_value(&metrics.pm25MicrogramsPerCubicMeter),
        ),
        member(
            "precipitationMm",
            nullable_number_value(&metrics.precipitationMm),
        ),
        member(
            "precipitationRateMmPerHour",
            nullable_number_value(&metrics.precipitationRateMmPerHour),
        ),
        member("pressureHpa", nullable_number_value(&metrics.pressureHpa)),
        member(
            "relativeHumidityPercent",
            nullable_number_value(&metrics.relativeHumidityPercent),
        ),
        member(
            "soilElectricalConductivityMicrosiemensPerCm",
            nullable_number_value(&metrics.soilElectricalConductivityMicrosiemensPerCm),
        ),
        member(
            "soilMoisturePercent",
            nullable_number_value(&metrics.soilMoisturePercent),
        ),
        member(
            "solarRadiationWm2",
            nullable_number_value(&metrics.solarRadiationWm2),
        ),
        member("temperatureC", nullable_number_value(&metrics.temperatureC)),
        member("uvIndex", nullable_number_value(&metrics.uvIndex)),
        member("waterLevelM", nullable_number_value(&metrics.waterLevelM)),
        member(
            "windDirectionDegrees",
            nullable_number_value(&metrics.windDirectionDegrees),
        ),
        member("windGustMps", nullable_number_value(&metrics.windGustMps)),
        member("windSpeedMps", nullable_number_value(&metrics.windSpeedMps)),
        member(
            "wetBulbGlobeTemperatureC",
            nullable_number_value(&metrics.wetBulbGlobeTemperatureC),
        ),
    ])
}

/// Project one nullable metric number.
fn nullable_number_value(value: &Nullable<f64>) -> JsonValue {
    // preserve null versus numeric metric semantics
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => JsonValue::Number(*value),
    }
}

/// Require every recursive JSON number to be finite.
fn finite_json(value: &JsonValue) -> bool {
    // recurse through the generated closed JSON alternatives
    match value {
        JsonValue::Number(value) => value.is_finite(),
        JsonValue::Array(values) => values.iter().all(finite_json),
        JsonValue::Object(fields) => fields.iter().all(|(_, value)| finite_json(value)),
        JsonValue::Boolean(_) | JsonValue::String(_) | JsonValue::Null => true,
    }
}

/// Render one generated source kind as its wire spelling.
const fn source_kind_wire(value: SourceKind) -> &'static str {
    // retain every closed generated alternative
    match value {
        SourceKind::PhysicalSensor => "physical_sensor",
        SourceKind::ModelCurrent => "model_current",
        SourceKind::Reanalysis => "reanalysis",
        SourceKind::Forecast => "forecast",
        SourceKind::TideObservation => "tide_observation",
        SourceKind::TidePrediction => "tide_prediction",
    }
}

/// Select the shared domain classification retained by one worker code.
fn ingestion_classification(code: &str) -> IngestionErrorClassification {
    // retain the two transport-specific non-default classifications
    match code {
        "provider_rate_limited" => IngestionErrorClassification::RateLimited,
        "provider_request_rejected" => IngestionErrorClassification::Permanent,
        "ecowitt_invalid_payload"
        | "empty_payload"
        | "invalid_json"
        | "invalid_noaa_runtime_record"
        | "invalid_payload"
        | "noaa_invalid_payload"
        | "provider_response_too_large" => IngestionErrorClassification::InvalidPayload,
        _ if code.starts_with("invalid_") => IngestionErrorClassification::InvalidPayload,
        _ => IngestionErrorClassification::Retryable,
    }
}

/// Validate the domain's stable bounded ingestion code grammar.
fn valid_ingestion_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b':' | b'-')
        })
}

/// Map one persistence operation to its stable failure code.
const fn persistence_code(operation: RuntimePersistenceOperation) -> &'static str {
    // avoid Debug spelling in durable worker results
    match operation {
        RuntimePersistenceOperation::CheckpointRead => "persistence_checkpoint_read",
        RuntimePersistenceOperation::Complete => "persistence_complete",
        RuntimePersistenceOperation::DueSourceDiscovery => "persistence_due_source_discovery",
        RuntimePersistenceOperation::FailureFinalize => "persistence_failure_finalize",
        RuntimePersistenceOperation::Heartbeat => "persistence_heartbeat",
        RuntimePersistenceOperation::Start => "persistence_start",
    }
}

/// Require an exact set of ordered cursor keys without extensions or duplicates.
fn exact_keys(fields: &[(Vec<u16>, JsonValue)], keys: &[&str]) -> bool {
    fields.len() == keys.len() && keys.iter().all(|key| cursor_field(fields, key).is_some())
}

/// Borrow one cursor field by exact UTF-16 key.
fn cursor_field<'a>(fields: &'a [(Vec<u16>, JsonValue)], key: &str) -> Option<&'a JsonValue> {
    fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
}

/// Decode one cursor string without UTF-16 replacement.
fn cursor_text(fields: &[(Vec<u16>, JsonValue)], key: &str) -> Result<String, TaskFailure> {
    // reject missing and non-string cursor members
    let Some(JsonValue::String(value)) = cursor_field(fields, key) else {
        return Err(invalid_provider_cursor());
    };
    String::from_utf16(value).map_err(|_| invalid_provider_cursor())
}

/// Decode one finite cursor number.
fn cursor_finite_number(value: &JsonValue) -> Result<f64, TaskFailure> {
    // reject null, strings, and non-finite runtime numbers
    match value {
        JsonValue::Number(value) if value.is_finite() => Ok(*value),
        _ => Err(invalid_provider_cursor()),
    }
}

/// Construct one generated object member.
fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// Construct one generated JSON string.
fn string_value(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Render one digest in lowercase hexadecimal.
fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);

    // emit two lowercase nibbles for every digest byte
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

/// Construct one zero-attempt task failure.
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

/// Preserve a distinct name where a failure parameter shadows the helper.
fn failure_code(code: &str) -> TaskFailure {
    failure(code)
}

/// Construct one invalid-time failure.
fn invalid_runtime_time() -> TaskFailure {
    failure("invalid_runtime_time")
}

/// Construct one invalid-JSON failure.
fn invalid_runtime_json() -> TaskFailure {
    failure("invalid_runtime_json")
}

/// Construct one invalid-cursor failure.
fn invalid_provider_cursor() -> TaskFailure {
    failure("invalid_provider_cursor")
}

impl crate::TideBackfillRuntimeValues for NativeScheduledValues {
    /// Retain the original request metadata keys and spelling.
    fn tide_backfill_request_metadata_json(
        &mut self,
        endpoint: &str,
        station_id: &str,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        serialize_object(&[
            member("endpoint", string_value(endpoint)),
            member("station_id", string_value(station_id)),
        ])
        .map(Some)
    }

    /// Preserve the exact provider classification, code and bounded message.
    fn tide_backfill_ingestion_error(
        &mut self,
        failure: &weather_worker::TideBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        use weather_worker::TideBackfillErrorClassification as Classification;
        let classification = match failure.error.classification {
            Classification::Retryable => IngestionErrorClassification::Retryable,
            Classification::RateLimited => IngestionErrorClassification::RateLimited,
            Classification::Permanent => IngestionErrorClassification::Permanent,
            Classification::InvalidPayload => IngestionErrorClassification::InvalidPayload,
        };
        Ok(AdmittedIngestionError::admitted(IngestionError {
            classification,
            code: failure.error.code.clone(),
            message: failure.error.message.clone(),
            metadata: FieldPresence::Missing,
        }))
    }

    /// Retain the shared source persistence operation diagnostics.
    fn tide_backfill_task_failure(
        &mut self,
        operation: crate::TideBackfillPersistenceOperation,
        _message: &str,
    ) -> TaskFailure {
        failure(tide_backfill_operation_code(operation))
    }

    /// Preserve the original failure boundary for committed chunks.
    fn tide_backfill_operation_failure(
        &mut self,
        operation: crate::TideBackfillPersistenceOperation,
        message: &str,
    ) -> weather_worker::TideBackfillOperationError {
        weather_worker::TideBackfillOperationError::unexpected(format!(
            "{}: {message}",
            tide_backfill_operation_code(operation)
        ))
    }
}

/// Map the finite tide database boundary vocabulary to the worker codes.
fn tide_backfill_operation_code(
    operation: crate::TideBackfillPersistenceOperation,
) -> &'static str {
    use crate::TideBackfillPersistenceOperation as Operation;
    match operation {
        Operation::ResumeRead => "persistence_checkpoint_read",
        Operation::Acquire => "source_session_acquire",
        Operation::Abandon => "abandon_expired_runs",
        Operation::Start => "persistence_start",
        Operation::Complete => "persistence_complete",
        Operation::FailureFinalize => "persistence_failure_finalize",
        Operation::Release => "source_session_release",
    }
}

impl crate::TempestBackfillRuntimeValues for NativeScheduledValues {
    /// Preserve original numeric station ids and producer metadata ordering.
    fn tempest_backfill_request_metadata_json(
        &mut self,
        endpoint: &str,
        device_id: u64,
        location_id: u64,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        serialize_object(&[
            member("device_id", JsonValue::Number(device_id as f64)),
            member("endpoint", string_value(endpoint)),
            member("location_id", JsonValue::Number(location_id as f64)),
        ])
        .map(Some)
    }

    /// Share the NOAA/Tempest source's identical normalized failure contract.
    fn tempest_backfill_ingestion_error(
        &mut self,
        failure: &weather_worker::TempestBackfillProviderFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure> {
        crate::TideBackfillRuntimeValues::tide_backfill_ingestion_error(self, failure)
    }

    /// Retain the same persisted chunk operation vocabulary.
    fn tempest_backfill_task_failure(
        &mut self,
        operation: crate::TempestBackfillPersistenceOperation,
        _message: &str,
    ) -> TaskFailure {
        failure(tempest_backfill_operation_code(operation))
    }

    /// Preserve the inner committed-run failure boundary.
    fn tempest_backfill_operation_failure(
        &mut self,
        operation: crate::TempestBackfillPersistenceOperation,
        message: &str,
    ) -> weather_worker::TempestBackfillOperationError {
        weather_worker::TempestBackfillOperationError::unexpected(format!(
            "{}: {message}",
            tempest_backfill_operation_code(operation)
        ))
    }
}

/// Translate the finite Tempest lifecycle to the shared normalized backfill vocabulary.
fn tempest_backfill_operation_code(
    operation: crate::TempestBackfillPersistenceOperation,
) -> &'static str {
    use crate::{
        TempestBackfillPersistenceOperation as Tempest, TideBackfillPersistenceOperation as Tide,
    };
    tide_backfill_operation_code(match operation {
        Tempest::ResumeRead => Tide::ResumeRead,
        Tempest::Acquire => Tide::Acquire,
        Tempest::Abandon => Tide::Abandon,
        Tempest::Start => Tide::Start,
        Tempest::Complete => Tide::Complete,
        Tempest::FailureFinalize => Tide::FailureFinalize,
        Tempest::Release => Tide::Release,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf, process::Command};
    use weather_worker::{TempestObservationRequest, normalize_tempest_observations};

    /// Compare actual native scheduled record storage identities against predecessor output.
    #[test]
    fn scheduled_tempest_content_hash_matches_node() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let oracle = Command::new("node")
            .arg(root.join(
                "rust/crates/weather-runtime/tests/native-rain-capture-values-node-oracle.mjs",
            ))
            .output()
            .unwrap();
        assert!(
            oracle.status.success(),
            "{}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        let oracle = NativeWirePrimitives.decode_json(&oracle.stdout).unwrap();
        let JsonValue::Object(fields) = oracle else {
            panic!("oracle object")
        };
        let expected = fields
            .into_iter()
            .find_map(|(key, value)| {
                key.into_iter()
                    .eq("tempestHashes".encode_utf16())
                    .then_some(value)
            })
            .unwrap();
        let bytes =
            fs::read(root.join("packages/providers/test/fixtures/tempest/observations.json"))
                .unwrap();
        let envelope = crate::decode_native_tempest_payload(&bytes).unwrap();
        let request = TempestObservationRequest {
            device_id: 470937,
            end_exclusive_ms: 1787191200000,
            location_id: 203055,
            serial: "ST-00198967".to_owned(),
            source_id: "source-tempest".to_owned(),
            start_ms: 1787184000000,
            timezone: "America/Los_Angeles".to_owned(),
        };
        let received = NativeWirePrimitives
            .canonical_utc_milliseconds("2026-08-20T12:00:00.000Z")
            .unwrap();
        let result = normalize_tempest_observations(envelope, &request, received, 1).unwrap();
        let mut values = NativeScheduledValues::new();
        let hashes = result
            .records
            .into_iter()
            .map(|record| {
                let prepared = crate::project_tempest_record(record, &mut values).unwrap();
                string_value(prepared.storage.content_hash.as_str())
            })
            .collect();
        assert_eq!(JsonValue::Array(hashes), expected);
    }
}
