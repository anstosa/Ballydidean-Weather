//! Project the remaining public-station records into the existing scheduled repository.

use weather_contracts::{
    DeviceMetadata, FieldPresence, JsonValue, NormalizedWeatherRecord, Nullable,
    ScheduledCheckpointState, SourceKind, WeatherRecordMetadata,
};
use weather_database::{PreparedWeatherRecord, SerializedJson, WeatherRecordStorageEvidence};
use weather_worker::{
    SupplementalPublicStationRecord, SupplementalPublicStationResponseMetadata, TaskFailure,
};

use crate::{
    OpenMeteoRuntimeValues, RuntimeTimeValue, ScheduledDatabaseAdapter,
    ScheduledDatabaseRepository, field_presence, json_string, project_metrics,
};

/// Storage projection for Weather Underground, Netatmo, and PurpleAir observations.
pub struct SupplementalPublicStationDatabaseAdapter;

/// Retain the same pool and transaction path used by existing provider schedules.
pub type SupplementalPublicStationDatabaseRepository<'a, Database, Values> =
    ScheduledDatabaseRepository<'a, Database, Values, SupplementalPublicStationDatabaseAdapter>;

impl<Values: OpenMeteoRuntimeValues> ScheduledDatabaseAdapter<Values>
    for SupplementalPublicStationDatabaseAdapter
{
    type Cursor = Option<i64>;
    type Record = SupplementalPublicStationRecord;
    type ResponseMetadata = SupplementalPublicStationResponseMetadata;

    /// Use the checkpoint window and version; these providers do not consume prior cursor JSON.
    fn checkpoint_cursor(
        _values: &mut Values,
        _checkpoint: &ScheduledCheckpointState,
    ) -> Result<Self::Cursor, TaskFailure> {
        Ok(None)
    }

    /// Build the TS-canonical record and let the admitted codec own its hash and JSON fields.
    fn project_record(
        values: &mut Values,
        record: Self::Record,
    ) -> Result<PreparedWeatherRecord, TaskFailure> {
        let provider = vec![
            entry("dataset", json_string(record.dataset)),
            entry("device_id", json_string(&record.device_id)),
            entry(
                "report_interval_minutes",
                JsonValue::Number(f64::from(record.report_interval_minutes)),
            ),
        ];
        let mut quality = Vec::with_capacity(3);
        // retain optional provider evidence in source insertion order
        if !record.sample.quality_flags.is_empty() {
            quality.push(entry(
                "flags",
                JsonValue::Array(
                    record
                        .sample
                        .quality_flags
                        .iter()
                        .copied()
                        .map(json_string)
                        .collect(),
                ),
            ));
        }
        quality.push(entry("sampling", json_string(record.sampling)));
        // omit unreported status instead of inventing a quality code
        if let Some(status) = record.sample.quality_status.as_deref() {
            quality.push(entry("status", json_string(status)));
        }
        let provider_metadata_json = values.serialize_json_object(&provider)?;
        let quality_metadata_json = values.serialize_json_object(&quality)?;
        let record = NormalizedWeatherRecord {
            metadata: WeatherRecordMetadata {
                device: Nullable::Value(DeviceMetadata {
                    model: field_presence(record.model),
                    serial: field_presence(record.serial),
                    vendor: FieldPresence::Value(record.vendor.to_owned()),
                }),
                model: Nullable::Null,
                provider: Nullable::Value(provider),
                quality: Nullable::Value(quality),
                upstreamTimezone: record.timezone,
            },
            metrics: project_metrics(record.sample.metrics),
            productRunAt: Nullable::Null,
            receivedAt: values.utc_instant(record.received_at_ms, RuntimeTimeValue::ReceivedAt)?,
            sourceId: record.source_id,
            sourceKind: SourceKind::PhysicalSensor,
            validAt: values.utc_instant(record.sample.valid_at_ms, RuntimeTimeValue::ValidAt)?,
        };
        let content_hash = values.weather_record_content_hash(&record)?;
        Ok(PreparedWeatherRecord {
            record,
            storage: WeatherRecordStorageEvidence {
                content_hash,
                provider_metadata_json: Some(provider_metadata_json),
                quality_metadata_json: Some(quality_metadata_json),
            },
        })
    }

    /// Retain the canonical valid-at cursor only for a nonempty provider response.
    fn provider_cursor_json(
        values: &mut Values,
        cursor: &Self::Cursor,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        // preserve empty provider cursor state
        let Some(valid_at_ms) = cursor else {
            return Ok(None);
        };
        let valid_at = values.utc_instant(*valid_at_ms, RuntimeTimeValue::ValidAt)?;
        values
            .serialize_json_object(&[entry("valid_at", json_string(&valid_at))])
            .map(Some)
    }

    /// Map the original provider-specific metadata without exposing physical request details.
    fn response_metadata_json(
        values: &mut Values,
        metadata: &Self::ResponseMetadata,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        let mut entries = Vec::with_capacity(4);
        // Netatmo records the count of measurement modules, not raw rows
        if let Some(count) = metadata.measurement_request_count {
            entries.push(entry(
                "measurement_request_count",
                JsonValue::Number(count as f64),
            ));
        }
        entries.push(entry(
            "normalized_record_count",
            JsonValue::Number(metadata.normalized_record_count as f64),
        ));
        entries.push(entry("provider", json_string(metadata.provider)));
        // Weather Underground and PurpleAir retain raw row counts
        if let Some(count) = metadata.raw_observation_count {
            entries.push(entry(
                "raw_observation_count",
                JsonValue::Number(count as f64),
            ));
        }
        // PurpleAir records the actual authorized public map sampling period
        if let Some(seconds) = metadata.resolution_seconds {
            entries.push(entry(
                "resolution_seconds",
                JsonValue::Number(f64::from(seconds)),
            ));
        }
        values.serialize_json_object(&entries).map(Some)
    }
}

/// Store an object entry in the generated UTF-16 representation.
fn entry(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}
