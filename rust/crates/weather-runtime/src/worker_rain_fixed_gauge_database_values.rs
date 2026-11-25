//! Build exact fixed-gauge statements from the retained approved value primitives.

use weather_contracts::{FieldPresence, JsonValue, NormalizedWeatherRecord, Nullable};
use weather_database::{
    SerializedJson, WorkerRainAdjustmentReadValues, WorkerRainFixedGaugeWriteValues,
};
use weather_worker::{RAIN_COLLECTION_STATIONS, TaskFailure};

use crate::api_revision_projection::revision_logical_key_sha256;
use crate::maintenance_canonical_json::valid_hash;
use crate::worker_rain_shadow::{object, string};
use crate::{
    ApiMaintenanceShadowEvidenceValues, MaintenanceReceiptScalar,
    WorkerRainFixedGaugeIdentityValues,
};

pub(crate) struct FixedGaugeDatabaseValues<'a, Values>(pub &'a mut Values);

impl<Values> WorkerRainFixedGaugeWriteValues for FixedGaugeDatabaseValues<'_, Values>
where
    Values: WorkerRainAdjustmentReadValues
        + WorkerRainFixedGaugeIdentityValues
        + ApiMaintenanceShadowEvidenceValues,
{
    type Error = TaskFailure;

    /// Admit the exact closed hour used by the immutable target group.
    fn is_closed_utc_hour(&mut self, value: &str) -> Result<bool, Self::Error> {
        Ok(self
            .0
            .canonical_utc_epoch_milliseconds(value)
            .is_some_and(|time| time.rem_euclid(3_600_000) == 0))
    }

    /// Preserve the closed logical-key grammar shared with the pre-read archive gap.
    fn logical_key_sha256(
        &mut self,
        source_ids: &[&str],
        valid_at: &str,
    ) -> Result<String, Self::Error> {
        let source_ids = source_ids
            .iter()
            .map(|source| (*source).to_owned())
            .collect::<Vec<_>>();
        revision_logical_key_sha256(
            self.0,
            &[
                (
                    "contractVersion",
                    MaintenanceReceiptScalar::Text(
                        "adjustment-rain-fixed-gauge-target-logical-key/v1",
                    ),
                ),
                (
                    "sourceIds",
                    MaintenanceReceiptScalar::TextArray(&source_ids),
                ),
                ("validAt", MaintenanceReceiptScalar::Text(valid_at)),
            ],
        )
        .map_err(|_| failure())
    }

    /// Bind the target row to the original canonical persisted content.
    fn weather_record_content_sha256(
        &mut self,
        record: &NormalizedWeatherRecord,
    ) -> Result<String, Self::Error> {
        self.0
            .fixed_gauge_record_content_sha256(record)
            .filter(|digest| valid_hash(digest.as_str()))
            .map(|digest| digest.as_str().to_owned())
            .ok_or_else(failure)
    }

    /// Preserve the original fixed station provenance check before any insertion.
    fn is_exact_target_record(
        &mut self,
        record: &NormalizedWeatherRecord,
        station_id: u64,
    ) -> Result<bool, Self::Error> {
        let station = RAIN_COLLECTION_STATIONS
            .iter()
            .find(|station| station.location_id == station_id)
            .ok_or_else(failure)?;
        let Nullable::Value(device) = &record.metadata.device else {
            return Ok(false);
        };
        Ok(record.metadata.upstreamTimezone == "America/Los_Angeles"
            && matches!(&device.vendor, FieldPresence::Value(value) if value == "WeatherFlow")
            && matches!(&device.model, FieldPresence::Value(value) if value == "Tempest")
            && matches!(&device.serial, FieldPresence::Value(value) if value == station.serial))
    }

    /// Format the one original prior-hour request start using the approved UTC owner.
    fn previous_utc_hour(&mut self, valid_at: &str) -> Result<String, Self::Error> {
        let start = self
            .0
            .canonical_utc_epoch_milliseconds(valid_at)
            .and_then(|time| time.checked_sub(3_600_000))
            .ok_or_else(failure)?;
        self.0.format_utc_milliseconds(start).map_err(|_| failure())
    }

    /// Build the exact scheduled provenance request metadata in source order.
    fn encode_run_request_metadata(
        &mut self,
        projection: &str,
        run_contract: &str,
    ) -> Result<SerializedJson, Self::Error> {
        encode(
            self.0,
            &object([
                (
                    "contractVersion",
                    string("adjustment-rain-fixed-gauge-target-run/v1"),
                ),
                ("projectionIdentitySha256", string(projection)),
                ("runContractSha256", string(run_contract)),
            ]),
        )
    }

    /// Build the original closed response metadata for one immutable row.
    fn encode_run_response_metadata(
        &mut self,
        logical_received_at: &str,
    ) -> Result<SerializedJson, Self::Error> {
        encode(
            self.0,
            &object([("logicalReceivedAt", string(logical_received_at))]),
        )
    }

    /// Preserve exact record metadata values, including original JSON literal null.
    fn encode_record_metadata(
        &mut self,
        metadata: &Nullable<Vec<(Vec<u16>, JsonValue)>>,
    ) -> Result<SerializedJson, Self::Error> {
        let document = match metadata {
            Nullable::Null => JsonValue::Null,
            Nullable::Value(fields) => JsonValue::Object(fields.clone()),
        };
        encode(self.0, &document)
    }
}

/// Use the original compact encoding for one JSONB bind without handwritten JSON.
fn encode(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    document: &JsonValue,
) -> Result<SerializedJson, TaskFailure> {
    let bytes = values
        .encode_shadow_ordered_json(document)
        .ok_or_else(failure)?;
    let json = String::from_utf8(bytes).map_err(|_| failure())?;
    Ok(SerializedJson::admitted(json))
}

/// Keep diagnostics categorical.
fn failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "rain_fixed_gauge_value_invalid".to_owned(),
    }
}
