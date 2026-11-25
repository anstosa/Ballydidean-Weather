//! Exact database facades for grouped weather and ECMWF revision binding.

use weather_contracts::{
    AdjustmentEcmwfTemperatureRevisionPointer, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionRowReceipt,
    AdjustmentWeatherRevisionPointer, AdjustmentWeatherRevisionPointerSourceKindEnum,
    CanonicalWeatherMetrics, DeviceMetadata, FieldPresence, JsonValue, NormalizedWeatherRecord,
    Nullable, SourceKind, WeatherRecordMetadata,
};

use crate::{
    AdmittedRevisionUtcInstant, PersistedWeatherAdjustmentRevision, SerializedJson, Sha256Digest,
};

const MAXIMUM_POINTERS: usize = 4_096;
const MAXIMUM_JSON_BYTES: usize = 4 * 1_024 * 1_024;
const WEATHER_REVISION_BATCH_CONTRACT: &str = "adjustment-revision-batch-receipt/v1";

/// Exact grouped weather revision binder owned by the TypeScript repository.
pub const BIND_WEATHER_ADJUSTMENT_REVISIONS_SQL: &str =
    "SELECT weather_bind_weather_record_revisions_v1($1::jsonb) AS value";

/// Exact ECMWF temperature revision binder owned by the TypeScript repository.
pub const BIND_ECMWF_TEMPERATURE_REVISION_SQL: &str =
    "SELECT weather_bind_ecmwf_temperature_revision_v1($1::jsonb) AS value";

/// Exact bounded physical-target backlog lookup owned by the TypeScript repository.
pub const PENDING_PHYSICAL_WEATHER_ADJUSTMENT_REVISIONS_SQL: &str = r#"
    SELECT run.adapter_version AS "adapterVersion",
      wr.apparent_temperature_c AS "apparentTemperatureC",
      wr.black_globe_temperature_c AS "blackGlobeTemperatureC",
      wr.cloud_cover_percent AS "cloudCoverPercent",
      wr.content_hash AS "contentHash",
      wr.device_model AS "deviceModel", wr.device_serial AS "deviceSerial",
      wr.device_vendor AS "deviceVendor", wr.first_received_at AS "logicalReceivedAt",
      wr.pm25_micrograms_per_cubic_meter AS "pm25MicrogramsPerCubicMeter",
      wr.precipitation_mm AS "precipitationMm",
      wr.precipitation_rate_mm_per_hour AS "precipitationRateMmPerHour",
      wr.pressure_hpa AS "pressureHpa", provider.provider_key AS "providerKey",
      wr.provider_metadata AS "providerMetadata", wr.quality_metadata AS "qualityMetadata",
      wr.last_received_at AS "receivedAt",
      wr.relative_humidity_percent AS "relativeHumidityPercent",
      run.source_config_fingerprint::text AS "sourceConfigFingerprint",
      wr.source_id::text AS "sourceId", source.source_key AS "sourceKey",
      wr.soil_electrical_conductivity_us_cm AS "soilElectricalConductivityMicrosiemensPerCm",
      wr.soil_moisture_percent AS "soilMoisturePercent",
      wr.solar_radiation_wm2 AS "solarRadiationWm2", wr.temperature_c AS "temperatureC",
      wr.upstream_model AS "upstreamModel", wr.upstream_timezone AS "upstreamTimezone",
      wr.uv_index AS "uvIndex", wr.valid_at AS "validAt", wr.water_level_m AS "waterLevelM",
      wr.wet_bulb_globe_temperature_c AS "wetBulbGlobeTemperatureC",
      wr.wind_direction_degrees AS "windDirectionDegrees", wr.wind_gust_mps AS "windGustMps",
      wr.wind_speed_mps AS "windSpeedMps"
    FROM weather_records wr
    JOIN sources source ON source.id = wr.source_id
    JOIN providers provider ON provider.id = source.provider_id
    JOIN ingestion_runs run ON run.id = wr.last_ingestion_run_id
    WHERE wr.source_id = $1 AND wr.source_kind = 'physical_sensor'
      AND wr.product_run_at IS NULL AND wr.valid_at >= $2 AND wr.first_received_at >= $2
      AND wr.adjustment_revision_receipt IS NULL
    ORDER BY wr.valid_at, wr.id
    LIMIT 168
"#;

/// One approved serialized JSON document in PostgreSQL placeholder order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerRevisionArchiveWriteBind {
    Json(SerializedJson),
}

/// One closed database function call with its single JSON bind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRevisionArchiveWritePlan {
    pub sql: &'static str,
    pub binds: [WorkerRevisionArchiveWriteBind; 1],
}

/// Ordered bind domain for the pending physical-target lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingPhysicalWeatherRevisionReadBind {
    Text(String),
    UtcInstant(String),
}

/// Exact two-bind pending physical-target read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPhysicalWeatherRevisionReadPlan {
    pub sql: &'static str,
    pub binds: [PendingPhysicalWeatherRevisionReadBind; 2],
}

/// One flattened pending physical-target database row.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingPhysicalWeatherRevisionRow {
    pub adapter_version: String,
    pub apparent_temperature_c: Nullable<f64>,
    pub black_globe_temperature_c: Nullable<f64>,
    pub cloud_cover_percent: Nullable<f64>,
    pub content_hash: String,
    pub device_model: Nullable<String>,
    pub device_serial: Nullable<String>,
    pub device_vendor: Nullable<String>,
    pub logical_received_at: String,
    pub pm25_micrograms_per_cubic_meter: Nullable<f64>,
    pub precipitation_mm: Nullable<f64>,
    pub precipitation_rate_mm_per_hour: Nullable<f64>,
    pub pressure_hpa: Nullable<f64>,
    pub provider_key: String,
    pub provider_metadata: Nullable<Vec<(Vec<u16>, JsonValue)>>,
    pub quality_metadata: Nullable<Vec<(Vec<u16>, JsonValue)>>,
    pub received_at: String,
    pub relative_humidity_percent: Nullable<f64>,
    pub source_config_fingerprint: String,
    pub source_id: String,
    pub source_key: String,
    pub soil_electrical_conductivity_microsiemens_per_cm: Nullable<f64>,
    pub soil_moisture_percent: Nullable<f64>,
    pub solar_radiation_wm2: Nullable<f64>,
    pub temperature_c: Nullable<f64>,
    pub upstream_model: Nullable<String>,
    pub upstream_timezone: String,
    pub uv_index: Nullable<f64>,
    pub valid_at: String,
    pub water_level_m: Nullable<f64>,
    pub wet_bulb_globe_temperature_c: Nullable<f64>,
    pub wind_direction_degrees: Nullable<f64>,
    pub wind_gust_mps: Nullable<f64>,
    pub wind_speed_mps: Nullable<f64>,
}

/// One weather batch entry decoded from the database-owned wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerWeatherRevisionBoundReceipt {
    pub projection_kind: AdjustmentRevisionCommitReceiptProjectionKindEnum,
    pub revision_receipt: AdjustmentRevisionCommitReceipt,
    pub source_id: String,
    pub stored_content_sha256: String,
    pub valid_at: String,
}

/// One decoded row containing the server-owned batch wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerWeatherRevisionBatchReceipt {
    pub contract_version: String,
    pub receipts: Vec<WorkerWeatherRevisionBoundReceipt>,
}

/// Approved JSON and time owners used before and after database execution.
pub trait WorkerRevisionArchiveWriteValues {
    type Error;

    /// Serialize the ordered weather pointer batch with the existing JSON owner.
    fn serialize_weather_revision_pointers(
        &mut self,
        pointers: &[AdjustmentWeatherRevisionPointer],
    ) -> Result<SerializedJson, Self::Error>;

    /// Serialize one ECMWF pointer with the existing JSON owner.
    fn serialize_ecmwf_temperature_revision_pointer(
        &mut self,
        pointer: &AdjustmentEcmwfTemperatureRevisionPointer,
    ) -> Result<SerializedJson, Self::Error>;

    /// Parse one exact canonical UTC millisecond instant.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error>;
}

/// PostgreSQL execution boundary for the two worker revision binders.
pub trait WorkerRevisionArchiveWritePort {
    type Error;

    /// Execute the weather batch binder and decode every wrapper row.
    fn bind_weather_adjustment_revisions(
        &mut self,
        plan: WorkerRevisionArchiveWritePlan,
    ) -> Result<Vec<WorkerWeatherRevisionBatchReceipt>, Self::Error>;

    /// Execute the ECMWF row binder and decode every wrapper row.
    fn bind_ecmwf_temperature_revision(
        &mut self,
        plan: WorkerRevisionArchiveWritePlan,
    ) -> Result<Vec<AdjustmentRevisionRowReceipt>, Self::Error>;
}

/// PostgreSQL boundary for the bounded pending physical-target read.
pub trait PendingPhysicalWeatherRevisionReadPort {
    type Error;

    /// Execute the exact ordered lookup and decode its flattened rows.
    fn read_pending_physical_weather_revisions(
        &mut self,
        plan: PendingPhysicalWeatherRevisionReadPlan,
    ) -> Result<Vec<PendingPhysicalWeatherRevisionRow>, Self::Error>;
}

/// Closed input and database-response failures at the worker facade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerRevisionArchiveWriteAdmissionError {
    InvalidBatchCardinality,
    JsonTooLarge,
    InvalidSourceIdentity,
    InvalidHash,
    ProjectionIdentityMismatch,
    InvalidSourceKind,
    ReceiptCardinality,
    ReceiptMismatch,
    InvalidPhysicalRevisionRow,
}

/// Failure preserving either the approved values owner or database driver error.
#[derive(Debug, PartialEq, Eq)]
pub enum WorkerRevisionArchiveWriteError<PortError, ValuesError> {
    Admission(WorkerRevisionArchiveWriteAdmissionError),
    Port(PortError),
    Values(ValuesError),
}

/// Bind one ordered staged weather batch and return receipts in pointer order.
pub fn bind_weather_adjustment_revisions<Port, Values>(
    port: &mut Port,
    values: &mut Values,
    pointers: &[AdjustmentWeatherRevisionPointer],
) -> Result<
    Vec<AdjustmentRevisionCommitReceipt>,
    WorkerRevisionArchiveWriteError<Port::Error, Values::Error>,
>
where
    Port: WorkerRevisionArchiveWritePort,
    Values: WorkerRevisionArchiveWriteValues,
{
    validate_weather_pointers::<Port::Error, _>(values, pointers)?;
    let json = values
        .serialize_weather_revision_pointers(pointers)
        .map_err(WorkerRevisionArchiveWriteError::Values)?;
    // preserve the original four-megabyte metadata envelope
    if json.as_str().len() > MAXIMUM_JSON_BYTES {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::JsonTooLarge,
        ));
    }
    let rows = port
        .bind_weather_adjustment_revisions(WorkerRevisionArchiveWritePlan {
            sql: BIND_WEATHER_ADJUSTMENT_REVISIONS_SQL,
            binds: [WorkerRevisionArchiveWriteBind::Json(json)],
        })
        .map_err(WorkerRevisionArchiveWriteError::Port)?;
    validate_weather_receipts(values, pointers, rows)
}

/// Bind one staged ECMWF temperature run and return its server receipt.
pub fn bind_ecmwf_temperature_revision<Port, Values>(
    port: &mut Port,
    values: &mut Values,
    pointer: &AdjustmentEcmwfTemperatureRevisionPointer,
) -> Result<
    AdjustmentRevisionCommitReceipt,
    WorkerRevisionArchiveWriteError<Port::Error, Values::Error>,
>
where
    Port: WorkerRevisionArchiveWritePort,
    Values: WorkerRevisionArchiveWriteValues,
{
    validate_ecmwf_pointer::<Port::Error, _>(values, pointer)?;
    let json = values
        .serialize_ecmwf_temperature_revision_pointer(pointer)
        .map_err(WorkerRevisionArchiveWriteError::Values)?;
    // preserve the same bounded metadata envelope as the source facade
    if json.as_str().len() > MAXIMUM_JSON_BYTES {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::JsonTooLarge,
        ));
    }
    let rows = port
        .bind_ecmwf_temperature_revision(WorkerRevisionArchiveWritePlan {
            sql: BIND_ECMWF_TEMPERATURE_REVISION_SQL,
            binds: [WorkerRevisionArchiveWriteBind::Json(json)],
        })
        .map_err(WorkerRevisionArchiveWriteError::Port)?;
    validate_ecmwf_receipt(values, pointer, rows)
}

/// Read and reconstruct one bounded post-epoch physical target cohort.
pub fn list_pending_physical_weather_adjustment_revisions<Port, Values>(
    port: &mut Port,
    values: &mut Values,
    source_id: &str,
    epoch_at: &str,
) -> Result<
    Vec<PersistedWeatherAdjustmentRevision>,
    WorkerRevisionArchiveWriteError<Port::Error, Values::Error>,
>
where
    Port: PendingPhysicalWeatherRevisionReadPort,
    Values: WorkerRevisionArchiveWriteValues,
{
    // reject generic source selectors before issuing the bounded lookup
    if !valid_positive_decimal(source_id) {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::InvalidSourceIdentity,
        ));
    }
    values
        .utc_epoch_milliseconds(epoch_at)
        .map_err(WorkerRevisionArchiveWriteError::Values)?;
    let rows = port
        .read_pending_physical_weather_revisions(PendingPhysicalWeatherRevisionReadPlan {
            sql: PENDING_PHYSICAL_WEATHER_ADJUSTMENT_REVISIONS_SQL,
            binds: [
                PendingPhysicalWeatherRevisionReadBind::Text(source_id.to_owned()),
                PendingPhysicalWeatherRevisionReadBind::UtcInstant(epoch_at.to_owned()),
            ],
        })
        .map_err(WorkerRevisionArchiveWriteError::Port)?;
    project_pending_physical_weather_revisions(values, source_id, rows)
}

/// Admit one source-ordered cohort and reconstruct only retained canonical values.
fn project_pending_physical_weather_revisions<PortError, Values>(
    values: &mut Values,
    source_id: &str,
    rows: Vec<PendingPhysicalWeatherRevisionRow>,
) -> Result<
    Vec<PersistedWeatherAdjustmentRevision>,
    WorkerRevisionArchiveWriteError<PortError, Values::Error>,
>
where
    Values: WorkerRevisionArchiveWriteValues,
{
    // a conforming database can never exceed the query's fixed bound
    if rows.len() > 168 {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::InvalidBatchCardinality,
        ));
    }
    let first = rows.first();
    let stable = rows.iter().all(|row| {
        first.is_none_or(|first| {
            row.adapter_version == first.adapter_version
                && row.source_config_fingerprint == first.source_config_fingerprint
        })
    });
    // defer a short stable lineage until one full bounded body is available
    if rows.len() < 168 && stable {
        return Ok(Vec::new());
    }
    let Some(first) = first else {
        return Ok(Vec::new());
    };
    let boundary = rows
        .iter()
        .position(|row| {
            row.adapter_version != first.adapter_version
                || row.source_config_fingerprint != first.source_config_fingerprint
        })
        .unwrap_or(rows.len());
    let mut projected = Vec::with_capacity(boundary);
    // preserve only the exact prefix before the first lineage boundary
    for row in rows.into_iter().take(boundary) {
        projected.push(project_pending_physical_row::<PortError, _>(
            values, source_id, row,
        )?);
    }
    Ok(projected)
}

/// Reconstruct one canonical physical record from its stored database values.
fn project_pending_physical_row<PortError, Values>(
    values: &mut Values,
    source_id: &str,
    row: PendingPhysicalWeatherRevisionRow,
) -> Result<
    PersistedWeatherAdjustmentRevision,
    WorkerRevisionArchiveWriteError<PortError, Values::Error>,
>
where
    Values: WorkerRevisionArchiveWriteValues,
{
    // retain the query source and immutable stored identity exactly
    if row.source_id != source_id
        || !valid_sha256(&row.content_hash)
        || row.adapter_version.is_empty()
        || row.provider_key.is_empty()
        || row.source_config_fingerprint.is_empty()
        || row.source_key.is_empty()
        || row.upstream_timezone.is_empty()
        || !row_metrics_finite(&row)
    {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::InvalidPhysicalRevisionRow,
        ));
    }
    values
        .utc_epoch_milliseconds(&row.logical_received_at)
        .map_err(WorkerRevisionArchiveWriteError::Values)?;
    values
        .utc_epoch_milliseconds(&row.received_at)
        .map_err(WorkerRevisionArchiveWriteError::Values)?;
    values
        .utc_epoch_milliseconds(&row.valid_at)
        .map_err(WorkerRevisionArchiveWriteError::Values)?;
    let device = device_metadata(row.device_model, row.device_serial, row.device_vendor);
    Ok(PersistedWeatherAdjustmentRevision {
        adapter_version: row.adapter_version,
        content_hash: Sha256Digest::admitted(row.content_hash),
        logical_received_at: AdmittedRevisionUtcInstant::admitted(row.logical_received_at),
        provider_key: row.provider_key,
        record: NormalizedWeatherRecord {
            metadata: WeatherRecordMetadata {
                device,
                model: row.upstream_model,
                provider: row.provider_metadata,
                quality: row.quality_metadata,
                upstreamTimezone: row.upstream_timezone,
            },
            metrics: CanonicalWeatherMetrics {
                apparentTemperatureC: row.apparent_temperature_c,
                blackGlobeTemperatureC: row.black_globe_temperature_c,
                cloudCoverPercent: row.cloud_cover_percent,
                pm25MicrogramsPerCubicMeter: row.pm25_micrograms_per_cubic_meter,
                precipitationMm: row.precipitation_mm,
                precipitationRateMmPerHour: row.precipitation_rate_mm_per_hour,
                pressureHpa: row.pressure_hpa,
                relativeHumidityPercent: row.relative_humidity_percent,
                soilElectricalConductivityMicrosiemensPerCm: row
                    .soil_electrical_conductivity_microsiemens_per_cm,
                soilMoisturePercent: row.soil_moisture_percent,
                solarRadiationWm2: row.solar_radiation_wm2,
                temperatureC: row.temperature_c,
                uvIndex: row.uv_index,
                waterLevelM: row.water_level_m,
                windDirectionDegrees: row.wind_direction_degrees,
                windGustMps: row.wind_gust_mps,
                windSpeedMps: row.wind_speed_mps,
                wetBulbGlobeTemperatureC: row.wet_bulb_globe_temperature_c,
            },
            productRunAt: Nullable::Null,
            receivedAt: row.received_at,
            sourceId: row.source_id,
            sourceKind: SourceKind::PhysicalSensor,
            validAt: row.valid_at,
        },
        source_config_fingerprint: row.source_config_fingerprint,
        source_key: row.source_key,
    })
}

/// Build the original optional device object without introducing null fields.
fn device_metadata(
    model: Nullable<String>,
    serial: Nullable<String>,
    vendor: Nullable<String>,
) -> Nullable<DeviceMetadata> {
    // omit the whole object only when all three stored values are null
    if matches!(&model, Nullable::Null)
        && matches!(&serial, Nullable::Null)
        && matches!(&vendor, Nullable::Null)
    {
        Nullable::Null
    } else {
        Nullable::Value(DeviceMetadata {
            model: field_presence(model),
            serial: field_presence(serial),
            vendor: field_presence(vendor),
        })
    }
}

/// Convert a stored nullable field to the original optional object member.
fn field_presence(value: Nullable<String>) -> FieldPresence<String> {
    // preserve missing rather than serializing a nullable member
    match value {
        Nullable::Null => FieldPresence::Missing,
        Nullable::Value(value) => FieldPresence::Value(value),
    }
}

/// Require every stored weather metric to remain finite when present.
fn row_metrics_finite(row: &PendingPhysicalWeatherRevisionRow) -> bool {
    [
        &row.apparent_temperature_c,
        &row.black_globe_temperature_c,
        &row.cloud_cover_percent,
        &row.pm25_micrograms_per_cubic_meter,
        &row.precipitation_mm,
        &row.precipitation_rate_mm_per_hour,
        &row.pressure_hpa,
        &row.relative_humidity_percent,
        &row.soil_electrical_conductivity_microsiemens_per_cm,
        &row.soil_moisture_percent,
        &row.solar_radiation_wm2,
        &row.temperature_c,
        &row.uv_index,
        &row.water_level_m,
        &row.wet_bulb_globe_temperature_c,
        &row.wind_direction_degrees,
        &row.wind_gust_mps,
        &row.wind_speed_mps,
    ]
    .into_iter()
    .all(|value| match value {
        Nullable::Null => true,
        Nullable::Value(value) => value.is_finite(),
    })
}

/// Validate every weather pointer before any database execution.
fn validate_weather_pointers<PortError, Values>(
    values: &mut Values,
    pointers: &[AdjustmentWeatherRevisionPointer],
) -> Result<(), WorkerRevisionArchiveWriteError<PortError, Values::Error>>
where
    Values: WorkerRevisionArchiveWriteValues,
{
    // reject empty or unbounded batches at the facade edge
    if pointers.is_empty() || pointers.len() > MAXIMUM_POINTERS {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::InvalidBatchCardinality,
        ));
    }
    // validate every exact pointer before serialization or database access
    for pointer in pointers {
        // retain the positive decimal PostgreSQL identity domain
        if !valid_positive_decimal(&pointer.sourceId) {
            return Err(admission(
                WorkerRevisionArchiveWriteAdmissionError::InvalidSourceIdentity,
            ));
        }
        // reject malformed body and stage identities before the binder
        if [
            pointer.storedContentSha256.as_str(),
            pointer.projectionIdentitySha256.as_str(),
            pointer.projectionSha256.as_str(),
            pointer.stageReceiptSha256.as_str(),
        ]
        .into_iter()
        .any(|value| !valid_sha256(value))
        {
            return Err(admission(
                WorkerRevisionArchiveWriteAdmissionError::InvalidHash,
            ));
        }
        // bind both receipt identities to the same canonical projection bytes
        if pointer.projectionSha256 != pointer.projectionIdentitySha256 {
            return Err(admission(
                WorkerRevisionArchiveWriteAdmissionError::ProjectionIdentityMismatch,
            ));
        }
        values
            .utc_epoch_milliseconds(&pointer.validAt)
            .map_err(WorkerRevisionArchiveWriteError::Values)?;
        // require the source-kind-specific product clock
        match (&pointer.sourceKind, &pointer.productRunAt) {
            (
                AdjustmentWeatherRevisionPointerSourceKindEnum::Forecast,
                Nullable::Value(product_run_at),
            ) => values
                .utc_epoch_milliseconds(product_run_at)
                .map_err(WorkerRevisionArchiveWriteError::Values)?,
            (AdjustmentWeatherRevisionPointerSourceKindEnum::PhysicalSensor, Nullable::Null) => 0,
            _ => {
                return Err(admission(
                    WorkerRevisionArchiveWriteAdmissionError::InvalidSourceKind,
                ));
            }
        };
    }
    Ok(())
}

/// Validate one ECMWF pointer before serialization or database execution.
fn validate_ecmwf_pointer<PortError, Values>(
    values: &mut Values,
    pointer: &AdjustmentEcmwfTemperatureRevisionPointer,
) -> Result<(), WorkerRevisionArchiveWriteError<PortError, Values::Error>>
where
    Values: WorkerRevisionArchiveWriteValues,
{
    // retain the positive decimal PostgreSQL identity domain
    if !valid_positive_decimal(&pointer.siteId) {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::InvalidSourceIdentity,
        ));
    }
    // reject malformed source, body, and stage identities before the binder
    if [
        pointer.storedContentSha256.as_str(),
        pointer.projectionIdentitySha256.as_str(),
        pointer.projectionSha256.as_str(),
        pointer.providerResponseSha256.as_str(),
        pointer.stageReceiptSha256.as_str(),
    ]
    .into_iter()
    .any(|value| !valid_sha256(value))
    {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::InvalidHash,
        ));
    }
    // bind both receipt identities to the same canonical projection bytes
    if pointer.projectionSha256 != pointer.projectionIdentitySha256 {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::ProjectionIdentityMismatch,
        ));
    }
    values
        .utc_epoch_milliseconds(&pointer.runInitializedAt)
        .map_err(WorkerRevisionArchiveWriteError::Values)?;
    Ok(())
}

/// Validate one database weather wrapper and preserve pointer order.
fn validate_weather_receipts<PortError, Values>(
    values: &mut Values,
    pointers: &[AdjustmentWeatherRevisionPointer],
    rows: Vec<WorkerWeatherRevisionBatchReceipt>,
) -> Result<
    Vec<AdjustmentRevisionCommitReceipt>,
    WorkerRevisionArchiveWriteError<PortError, Values::Error>,
>
where
    Values: WorkerRevisionArchiveWriteValues,
{
    let [row]: [WorkerWeatherRevisionBatchReceipt; 1] = rows
        .try_into()
        .map_err(|_| admission(WorkerRevisionArchiveWriteAdmissionError::ReceiptCardinality))?;
    // require the exact wrapper and one ordered receipt per pointer
    if row.contract_version != WEATHER_REVISION_BATCH_CONTRACT
        || row.receipts.len() != pointers.len()
    {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::ReceiptCardinality,
        ));
    }
    let mut receipts = Vec::with_capacity(pointers.len());
    // join each returned entry to the pointer at the same stable ordinal
    for (entry, pointer) in row.receipts.into_iter().zip(pointers) {
        let expected_kind = weather_projection_kind(pointer.sourceKind);
        validate_commit_receipt::<PortError, _>(values, &entry.revision_receipt)?;
        // reject reordering or a receipt bound to different staged material
        if entry.projection_kind != expected_kind
            || entry.source_id != pointer.sourceId
            || entry.stored_content_sha256 != pointer.storedContentSha256
            || entry.valid_at != pointer.validAt
            || entry.revision_receipt.projectionKind != expected_kind
            || entry.revision_receipt.projectionIdentitySha256 != pointer.projectionIdentitySha256
            || entry.revision_receipt.projectionSha256 != pointer.projectionSha256
            || entry.revision_receipt.stageReceiptSha256 != pointer.stageReceiptSha256
        {
            return Err(admission(
                WorkerRevisionArchiveWriteAdmissionError::ReceiptMismatch,
            ));
        }
        receipts.push(entry.revision_receipt);
    }
    Ok(receipts)
}

/// Validate one database ECMWF wrapper and its staged material identity.
fn validate_ecmwf_receipt<PortError, Values>(
    values: &mut Values,
    pointer: &AdjustmentEcmwfTemperatureRevisionPointer,
    rows: Vec<AdjustmentRevisionRowReceipt>,
) -> Result<
    AdjustmentRevisionCommitReceipt,
    WorkerRevisionArchiveWriteError<PortError, Values::Error>,
>
where
    Values: WorkerRevisionArchiveWriteValues,
{
    let [row]: [AdjustmentRevisionRowReceipt; 1] = rows
        .try_into()
        .map_err(|_| admission(WorkerRevisionArchiveWriteAdmissionError::ReceiptCardinality))?;
    let receipt = row.revisionReceipt;
    validate_commit_receipt::<PortError, _>(values, &receipt)?;
    // bind the returned native-source receipt to the submitted projection
    if receipt.projectionKind != AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource
        || receipt.projectionIdentitySha256 != pointer.projectionIdentitySha256
        || receipt.projectionSha256 != pointer.projectionSha256
        || receipt.stageReceiptSha256 != pointer.stageReceiptSha256
    {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::ReceiptMismatch,
        ));
    }
    Ok(receipt)
}

/// Validate one server-owned commit receipt shape after typed decoding.
fn validate_commit_receipt<PortError, Values>(
    values: &mut Values,
    receipt: &AdjustmentRevisionCommitReceipt,
) -> Result<(), WorkerRevisionArchiveWriteError<PortError, Values::Error>>
where
    Values: WorkerRevisionArchiveWriteValues,
{
    // retain the database's positive global frontier ordinal
    if !valid_positive_decimal(&receipt.archiveCommitOrdinal) {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::ReceiptMismatch,
        ));
    }
    values
        .utc_epoch_milliseconds(&receipt.archiveCommittedAt)
        .map_err(WorkerRevisionArchiveWriteError::Values)?;
    // validate every database-owned chain identity before returning it
    if [
        receipt.frontierSha256.as_str(),
        receipt.predecessorFrontierSha256.as_str(),
        receipt.projectionIdentitySha256.as_str(),
        receipt.projectionSha256.as_str(),
        receipt.receiptSha256.as_str(),
        receipt.stageReceiptSha256.as_str(),
    ]
    .into_iter()
    .any(|value| !valid_sha256(value))
    {
        return Err(admission(
            WorkerRevisionArchiveWriteAdmissionError::ReceiptMismatch,
        ));
    }
    Ok(())
}

/// Select the original projection kind from one weather source kind.
const fn weather_projection_kind(
    source_kind: AdjustmentWeatherRevisionPointerSourceKindEnum,
) -> AdjustmentRevisionCommitReceiptProjectionKindEnum {
    // preserve the comparator and physical-target grammar split
    match source_kind {
        AdjustmentWeatherRevisionPointerSourceKindEnum::Forecast => {
            AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch
        }
        AdjustmentWeatherRevisionPointerSourceKindEnum::PhysicalSensor => {
            AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision
        }
    }
}

/// Recognize one lowercase hexadecimal SHA-256 identity.
fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Recognize one positive PostgreSQL bigint spelling.
fn valid_positive_decimal(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes()[0] != b'0'
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Wrap one local admission failure without changing its category.
const fn admission<PortError, ValuesError>(
    value: WorkerRevisionArchiveWriteAdmissionError,
) -> WorkerRevisionArchiveWriteError<PortError, ValuesError> {
    WorkerRevisionArchiveWriteError::Admission(value)
}

#[cfg(test)]
mod tests {
    use weather_contracts::{
        AdjustmentRevisionCommitReceiptContractVersionLiteral,
        AdjustmentRevisionRowReceiptContractVersionLiteral,
    };

    use super::*;

    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    #[derive(Default)]
    struct Values {
        parsed: Vec<String>,
    }

    impl WorkerRevisionArchiveWriteValues for Values {
        type Error = &'static str;

        /// Return one bounded fixture document for the weather pointer vector.
        fn serialize_weather_revision_pointers(
            &mut self,
            _: &[AdjustmentWeatherRevisionPointer],
        ) -> Result<SerializedJson, Self::Error> {
            Ok(SerializedJson::admitted("weather-json"))
        }

        /// Return one bounded fixture document for the ECMWF pointer.
        fn serialize_ecmwf_temperature_revision_pointer(
            &mut self,
            _: &AdjustmentEcmwfTemperatureRevisionPointer,
        ) -> Result<SerializedJson, Self::Error> {
            Ok(SerializedJson::admitted("ecmwf-json"))
        }

        /// Record each instant delegated to the approved parser.
        fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error> {
            self.parsed.push(value.to_owned());
            Ok(0)
        }
    }

    #[derive(Default)]
    struct Port {
        plan: Option<WorkerRevisionArchiveWritePlan>,
        physical_plan: Option<PendingPhysicalWeatherRevisionReadPlan>,
        physical_rows: Vec<PendingPhysicalWeatherRevisionRow>,
        weather: Vec<WorkerWeatherRevisionBatchReceipt>,
        ecmwf: Vec<AdjustmentRevisionRowReceipt>,
    }

    impl PendingPhysicalWeatherRevisionReadPort for Port {
        type Error = &'static str;

        /// Capture one physical backlog plan and return its fixture rows.
        fn read_pending_physical_weather_revisions(
            &mut self,
            plan: PendingPhysicalWeatherRevisionReadPlan,
        ) -> Result<Vec<PendingPhysicalWeatherRevisionRow>, Self::Error> {
            self.physical_plan = Some(plan);
            Ok(std::mem::take(&mut self.physical_rows))
        }
    }

    impl WorkerRevisionArchiveWritePort for Port {
        type Error = &'static str;

        /// Capture one weather plan and return its fixture wrapper.
        fn bind_weather_adjustment_revisions(
            &mut self,
            plan: WorkerRevisionArchiveWritePlan,
        ) -> Result<Vec<WorkerWeatherRevisionBatchReceipt>, Self::Error> {
            self.plan = Some(plan);
            Ok(std::mem::take(&mut self.weather))
        }

        /// Capture one ECMWF plan and return its fixture wrapper.
        fn bind_ecmwf_temperature_revision(
            &mut self,
            plan: WorkerRevisionArchiveWritePlan,
        ) -> Result<Vec<AdjustmentRevisionRowReceipt>, Self::Error> {
            self.plan = Some(plan);
            Ok(std::mem::take(&mut self.ecmwf))
        }
    }

    /// Build one server receipt bound to the supplied projection kind.
    fn receipt(
        kind: AdjustmentRevisionCommitReceiptProjectionKindEnum,
    ) -> AdjustmentRevisionCommitReceipt {
        AdjustmentRevisionCommitReceipt {
            archiveCommitOrdinal: "1".to_owned(),
            archiveCommittedAt: "2026-11-24T00:00:00.000Z".to_owned(),
            contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
            frontierSha256: HASH_A.to_owned(),
            predecessorFrontierSha256: HASH_B.to_owned(),
            projectionIdentitySha256: HASH_A.to_owned(),
            projectionKind: kind,
            projectionSha256: HASH_A.to_owned(),
            receiptSha256: HASH_C.to_owned(),
            stageReceiptSha256: HASH_B.to_owned(),
        }
    }

    /// Build one valid forecast weather pointer.
    fn weather_pointer() -> AdjustmentWeatherRevisionPointer {
        AdjustmentWeatherRevisionPointer {
            productRunAt: Nullable::Value("2026-11-23T18:00:00.000Z".to_owned()),
            projectionIdentitySha256: HASH_A.to_owned(),
            projectionSha256: HASH_A.to_owned(),
            sourceId: "42".to_owned(),
            sourceKind: AdjustmentWeatherRevisionPointerSourceKindEnum::Forecast,
            stageReceiptSha256: HASH_B.to_owned(),
            storedContentSha256: HASH_C.to_owned(),
            validAt: "2026-11-24T00:00:00.000Z".to_owned(),
        }
    }

    /// Build one stored physical row for cohort admission fixtures.
    fn physical_row(adapter_version: &str, fingerprint: &str) -> PendingPhysicalWeatherRevisionRow {
        PendingPhysicalWeatherRevisionRow {
            adapter_version: adapter_version.to_owned(),
            apparent_temperature_c: Nullable::Null,
            black_globe_temperature_c: Nullable::Null,
            cloud_cover_percent: Nullable::Null,
            content_hash: HASH_A.to_owned(),
            device_model: Nullable::Null,
            device_serial: Nullable::Null,
            device_vendor: Nullable::Null,
            logical_received_at: "2026-11-24T00:00:00.000Z".to_owned(),
            pm25_micrograms_per_cubic_meter: Nullable::Null,
            precipitation_mm: Nullable::Value(1.5),
            precipitation_rate_mm_per_hour: Nullable::Null,
            pressure_hpa: Nullable::Null,
            provider_key: "ecowitt".to_owned(),
            provider_metadata: Nullable::Null,
            quality_metadata: Nullable::Null,
            received_at: "2026-11-24T00:00:01.000Z".to_owned(),
            relative_humidity_percent: Nullable::Null,
            source_config_fingerprint: fingerprint.to_owned(),
            source_id: "42".to_owned(),
            source_key: "physical-42".to_owned(),
            soil_electrical_conductivity_microsiemens_per_cm: Nullable::Null,
            soil_moisture_percent: Nullable::Null,
            solar_radiation_wm2: Nullable::Null,
            temperature_c: Nullable::Value(12.25),
            upstream_model: Nullable::Null,
            upstream_timezone: "America/Los_Angeles".to_owned(),
            uv_index: Nullable::Null,
            valid_at: "2026-11-24T00:00:00.000Z".to_owned(),
            water_level_m: Nullable::Null,
            wet_bulb_globe_temperature_c: Nullable::Null,
            wind_direction_degrees: Nullable::Null,
            wind_gust_mps: Nullable::Null,
            wind_speed_mps: Nullable::Null,
        }
    }

    /// Bind one weather receipt through exact SQL and ordered identity checks.
    #[test]
    fn binds_ordered_weather_batch() {
        let pointer = weather_pointer();
        let revision = receipt(AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch);
        let mut port = Port {
            weather: vec![WorkerWeatherRevisionBatchReceipt {
                contract_version: WEATHER_REVISION_BATCH_CONTRACT.to_owned(),
                receipts: vec![WorkerWeatherRevisionBoundReceipt {
                    projection_kind:
                        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
                    revision_receipt: revision.clone(),
                    source_id: pointer.sourceId.clone(),
                    stored_content_sha256: pointer.storedContentSha256.clone(),
                    valid_at: pointer.validAt.clone(),
                }],
            }],
            ..Port::default()
        };
        let mut values = Values::default();

        let result = bind_weather_adjustment_revisions(
            &mut port,
            &mut values,
            std::slice::from_ref(&pointer),
        )
        .expect("weather batch");

        assert_eq!(result, vec![revision]);
        assert_eq!(
            port.plan.expect("plan").sql,
            BIND_WEATHER_ADJUSTMENT_REVISIONS_SQL
        );
        assert_eq!(values.parsed.len(), 3);
    }

    /// Reject a reordered database entry before returning any receipt.
    #[test]
    fn rejects_weather_batch_identity_drift() {
        let pointer = weather_pointer();
        let mut port = Port {
            weather: vec![WorkerWeatherRevisionBatchReceipt {
                contract_version: WEATHER_REVISION_BATCH_CONTRACT.to_owned(),
                receipts: vec![WorkerWeatherRevisionBoundReceipt {
                    projection_kind:
                        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
                    revision_receipt: receipt(
                        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
                    ),
                    source_id: "99".to_owned(),
                    stored_content_sha256: pointer.storedContentSha256.clone(),
                    valid_at: pointer.validAt.clone(),
                }],
            }],
            ..Port::default()
        };

        let error =
            bind_weather_adjustment_revisions(&mut port, &mut Values::default(), &[pointer])
                .expect_err("identity drift");

        assert_eq!(
            error,
            WorkerRevisionArchiveWriteError::Admission(
                WorkerRevisionArchiveWriteAdmissionError::ReceiptMismatch
            )
        );
    }

    /// Bind one ECMWF native-source receipt through the exact row facade.
    #[test]
    fn binds_ecmwf_temperature_revision() {
        let pointer = AdjustmentEcmwfTemperatureRevisionPointer {
            projectionIdentitySha256: HASH_A.to_owned(),
            projectionSha256: HASH_A.to_owned(),
            providerResponseSha256: HASH_C.to_owned(),
            runInitializedAt: "2026-11-24T00:00:00.000Z".to_owned(),
            siteId: "7".to_owned(),
            stageReceiptSha256: HASH_B.to_owned(),
            storedContentSha256: HASH_C.to_owned(),
        };
        let revision = receipt(AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource);
        let mut port = Port {
            ecmwf: vec![AdjustmentRevisionRowReceipt {
                contractVersion: AdjustmentRevisionRowReceiptContractVersionLiteral,
                revisionReceipt: revision.clone(),
            }],
            ..Port::default()
        };
        let mut values = Values::default();

        let result =
            bind_ecmwf_temperature_revision(&mut port, &mut values, &pointer).expect("ecmwf");

        assert_eq!(result, revision);
        assert_eq!(
            port.plan.expect("plan").sql,
            BIND_ECMWF_TEMPERATURE_REVISION_SQL
        );
        assert_eq!(values.parsed.len(), 2);
    }

    /// Defer a short stable physical lineage until its bounded body is complete.
    #[test]
    fn defers_short_stable_physical_revision_cohort() {
        let mut port = Port {
            physical_rows: vec![physical_row("adapter-v1", "fingerprint-v1")],
            ..Port::default()
        };
        let mut values = Values::default();

        let revisions = list_pending_physical_weather_adjustment_revisions(
            &mut port,
            &mut values,
            "42",
            "2026-11-23T00:00:00.000Z",
        )
        .expect("stable cohort");

        assert!(revisions.is_empty());
        assert_eq!(
            port.physical_plan.expect("plan").sql,
            PENDING_PHYSICAL_WEATHER_ADJUSTMENT_REVISIONS_SQL
        );
        assert_eq!(values.parsed.len(), 1);
    }

    /// Preserve a terminal prefix when the stored lineage changes inside the bound.
    #[test]
    fn projects_physical_revision_prefix_at_lineage_boundary() {
        let mut port = Port {
            physical_rows: vec![
                physical_row("adapter-v1", "fingerprint-v1"),
                physical_row("adapter-v2", "fingerprint-v2"),
            ],
            ..Port::default()
        };
        let mut values = Values::default();

        let revisions = list_pending_physical_weather_adjustment_revisions(
            &mut port,
            &mut values,
            "42",
            "2026-11-23T00:00:00.000Z",
        )
        .expect("lineage prefix");

        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].adapter_version, "adapter-v1");
        assert_eq!(revisions[0].record.sourceKind, SourceKind::PhysicalSensor);
        assert_eq!(values.parsed.len(), 4);
    }
}
