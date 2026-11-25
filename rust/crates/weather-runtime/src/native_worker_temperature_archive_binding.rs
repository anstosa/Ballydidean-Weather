//! Native enabled temperature-revision archive composition.

use std::{sync::Arc, time::Duration};

use weather_api::AdmittedAdjustmentRevisionCaptureWitness;
use weather_contracts::{
    AdjustmentEcmwfTemperatureRevisionPointer, AdjustmentRevisionProjectionLogicalKeyValue,
    AdjustmentRevisionProjectionRowsValue, AdjustmentRevisionRowReceipt,
    AdjustmentRevisionRowReceiptContractVersionLiteral,
    AdjustmentTemperatureNativeSourceProjection, AdjustmentWeatherRevisionPointer, JsonValue,
    MaintenanceShadowTemperatureRecentErrorState,
    MaintenanceShadowTemperatureRecentErrorStateCohortLiteral, Nullable,
};
use weather_database::{
    AdjustmentMaintenanceDatabasePort, AdjustmentRevisionGapMarker, EcmwfCanaryPersistencePort,
    EcmwfCanaryRevisionArchivePort, EcmwfTemperatureCanaryRunInitializationsReadPlan,
    MaintenancePublicationPlan, SerializedJson, Sha256Digest, WorkerRevisionArchiveWriteBind,
    WorkerRevisionArchiveWritePlan, WorkerRevisionArchiveWritePort,
    WorkerRevisionArchiveWriteValues, WorkerWeatherRevisionBatchReceipt,
};
use weather_edge::{ArchiveWireCodec, ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_worker::TaskFailure;

use crate::{
    ApiMaintenanceReceiptCanonicalValues, EcmwfRevisionArchiveDatabase,
    EcmwfRevisionArchiveDatabasePort, EcmwfRevisionArchiveError,
    EcmwfRevisionArchiveTransactionPort, EcmwfRevisionArchiveValues,
    EcmwfRevisionBindSavepointCommand, EcmwfRevisionGapPort, EcmwfRevisionMaterialDigest,
    HttpWorkerRainMaintenanceClient, MaintenanceReceiptScalar, NativeEdgeOrigins,
    NativeOutboundHttp, NativePostgresError, NativePostgresParameter, NativePostgresPool,
    NativePostgresTemperatureSession, NativePrivatePostTransport,
    NativeTemperatureCanaryDatabaseFactory, NativeWirePrimitives,
    NativeWorkerRainMaintenanceHttpJson, TemperatureCanaryPersistenceDatabasePort,
    TemperatureCanaryRevisionArchiveMode, native_postgres_weather_rows as field,
};

/// Concrete native HTTP/JSON revision archive client.
pub type NativeTemperatureRevisionArchiveClient = HttpWorkerRainMaintenanceClient<
    NativeWorkerRainMaintenanceHttpJson<NativeWirePrimitives, NativePrivatePostTransport>,
>;

/// Concrete enabled database composed over one serving transaction and one shared pool.
pub type NativeTemperatureRevisionArchiveDatabase = EcmwfRevisionArchiveDatabase<
    NativePostgresTemperatureSession,
    NativeTemperatureRevisionArchiveValues,
    NativeWirePrimitives,
    NativeTemperatureRevisionArchiveClient,
    NativePostgresPool,
    fn(EcmwfRevisionArchiveError) -> NativePostgresError,
>;

/// Native ordered JSON, time, decimal, receipt, and digest owner for ECMWF archive work.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeTemperatureRevisionArchiveValues;

/// Closed archive-value failure without projection or credential material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeTemperatureRevisionArchiveValueError {
    Encoding,
    InvalidClock,
    UnsupportedWeatherBatch,
}

impl ApiMaintenanceReceiptCanonicalValues for NativeTemperatureRevisionArchiveValues {
    /// Delegate exact canonical UTC admission to the approved wire owner.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        NativeWirePrimitives.canonical_utc_milliseconds(value)
    }

    /// Delegate SHA-256 to the approved native receipt owner.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        ApiMaintenanceReceiptCanonicalValues::sha256(&NativeWirePrimitives, bytes)
    }

    /// Delegate source-ordered receipt hashing to the approved native owner.
    fn unsigned_receipt_sha256(
        &self,
        fields: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        NativeWirePrimitives.unsigned_receipt_sha256(fields)
    }
}

impl WorkerRevisionArchiveWriteValues for NativeTemperatureRevisionArchiveValues {
    type Error = NativeTemperatureRevisionArchiveValueError;

    /// Refuse the unrelated grouped weather binder on this temperature-only owner.
    fn serialize_weather_revision_pointers(
        &mut self,
        _pointers: &[AdjustmentWeatherRevisionPointer],
    ) -> Result<SerializedJson, Self::Error> {
        Err(NativeTemperatureRevisionArchiveValueError::UnsupportedWeatherBatch)
    }

    /// Encode the exact source-ordered ECMWF pointer JSON document.
    fn serialize_ecmwf_temperature_revision_pointer(
        &mut self,
        pointer: &AdjustmentEcmwfTemperatureRevisionPointer,
    ) -> Result<SerializedJson, Self::Error> {
        compact_json(JsonValue::Object(vec![
            member(
                "projectionIdentitySha256",
                string(&pointer.projectionIdentitySha256),
            ),
            member("projectionSha256", string(&pointer.projectionSha256)),
            member(
                "providerResponseSha256",
                string(&pointer.providerResponseSha256),
            ),
            member("runInitializedAt", string(&pointer.runInitializedAt)),
            member("siteId", string(&pointer.siteId)),
            member("stageReceiptSha256", string(&pointer.stageReceiptSha256)),
            member("storedContentSha256", string(&pointer.storedContentSha256)),
        ]))
        .map(SerializedJson::admitted)
    }

    /// Parse one exact canonical database receipt clock.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error> {
        NativeWirePrimitives
            .canonical_utc_milliseconds(value)
            .ok_or(NativeTemperatureRevisionArchiveValueError::InvalidClock)
    }
}

impl EcmwfRevisionMaterialDigest for NativeTemperatureRevisionArchiveValues {
    /// Match JavaScript `toFixed(6)` including exact midpoint and negative-zero behavior.
    fn fixed_six(&mut self, value: f64) -> Option<String> {
        javascript_fixed_six(value)
    }

    /// Hash the exact source-fingerprint preimage with native SHA-256.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        ApiMaintenanceReceiptCanonicalValues::sha256(self, bytes)
    }
}

impl EcmwfRevisionArchiveValues for NativeTemperatureRevisionArchiveValues {
    /// Encode the exact named-field temperature projection with its required trailing LF.
    fn encode_ecmwf_revision_projection(
        &self,
        projection: &AdjustmentTemperatureNativeSourceProjection,
    ) -> Option<Vec<u8>> {
        NativeWirePrimitives.encode_ordered_json(&temperature_projection_value(projection)?)
    }
}

impl WorkerRevisionArchiveWritePort for NativePostgresTemperatureSession {
    type Error = NativePostgresError;

    /// Refuse the unrelated weather batch binder on the temperature transaction owner.
    fn bind_weather_adjustment_revisions(
        &mut self,
        _plan: WorkerRevisionArchiveWritePlan,
    ) -> Result<Vec<WorkerWeatherRevisionBatchReceipt>, Self::Error> {
        Err(NativePostgresError::InvalidParameter(
            "weather revision binder on temperature session",
        ))
    }

    /// Execute and decode one exact ECMWF row-receipt facade.
    fn bind_ecmwf_temperature_revision(
        &mut self,
        plan: WorkerRevisionArchiveWritePlan,
    ) -> Result<Vec<AdjustmentRevisionRowReceipt>, Self::Error> {
        let [WorkerRevisionArchiveWriteBind::Json(json)] = plan.binds;
        self.session_mut()?
            .query(
                plan.sql,
                &[NativePostgresParameter::Json(json.as_str().to_owned())],
            )?
            .iter()
            .map(|row| {
                let value = field::json(row, "value")?.ok_or_else(|| {
                    NativePostgresError::decode("value", "database JSON result is null")
                })?;
                decode_revision_row_receipt(&value)
            })
            .collect()
    }
}

impl EcmwfRevisionGapPort for NativePostgresTemperatureSession {
    type Error = NativePostgresError;

    /// Persist one permanent gap inside the retained serving transaction.
    fn write_ecmwf_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        AdjustmentMaintenanceDatabasePort::write_revision_gap(self.session_mut()?, plan)
    }
}

impl EcmwfRevisionGapPort for NativePostgresPool {
    type Error = NativePostgresError;

    /// Persist one post-commit publication gap through an independent shared-pool lease.
    fn write_ecmwf_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        let mut session = self.checkout()?;
        let result = AdjustmentMaintenanceDatabasePort::write_revision_gap(&mut session, plan);
        let release = session.release();
        // preserve the statement result before observing lease cleanup
        match result {
            Ok(markers) => {
                release?;
                Ok(markers)
            }
            Err(error) => {
                let _ = release;
                Err(error)
            }
        }
    }
}

impl EcmwfRevisionArchiveTransactionPort for NativePostgresTemperatureSession {
    /// Execute the exact nested revision-bind savepoint on the serving transaction.
    fn execute_ecmwf_revision_bind_savepoint(
        &mut self,
        command: EcmwfRevisionBindSavepointCommand,
    ) -> Result<(), <Self as WorkerRevisionArchiveWritePort>::Error> {
        self.session_mut()?.batch_execute(command.sql())
    }
}

impl EcmwfRevisionArchiveDatabasePort for NativePostgresTemperatureSession {
    /// Delegate the warmup inventory to the actual temperature session implementation.
    fn select_ecmwf_temperature_run_initializations(
        &mut self,
        plan: &EcmwfTemperatureCanaryRunInitializationsReadPlan,
    ) -> Result<Vec<weather_contracts::JsDateOrString>, <Self as EcmwfCanaryPersistencePort>::Error>
    {
        TemperatureCanaryPersistenceDatabasePort::read_temperature_canary_run_initializations(
            self, plan,
        )
    }

    /// Delegate the logical receipt to the retained transaction.
    fn select_ecmwf_revision_logical_receipt(
        &mut self,
        query: &weather_database::EcmwfCanaryLogicalReceiptQuery,
    ) -> Result<
        Option<weather_database::AdmittedRevisionUtcInstant>,
        <Self as EcmwfCanaryPersistencePort>::Error,
    > {
        EcmwfCanaryRevisionArchivePort::select_logical_receipt(self, query)
    }

    /// Delegate the outer archive savepoint to the retained transaction.
    fn execute_ecmwf_revision_archive_savepoint(
        &mut self,
        command: weather_database::EcmwfCanaryRevisionSavepointCommand,
    ) -> Result<(), <Self as EcmwfCanaryPersistencePort>::Error> {
        EcmwfCanaryRevisionArchivePort::execute_revision_savepoint(self, command)
    }

    /// Delegate Best Match reads to the retained transaction.
    fn select_ecmwf_revision_best_match_rows(
        &mut self,
        query: &weather_database::EcmwfCanaryBestMatchQuery,
    ) -> Result<
        Vec<weather_database::AdmittedEcmwfCanaryBestMatchRow>,
        <Self as EcmwfCanaryPersistencePort>::Error,
    > {
        EcmwfCanaryRevisionArchivePort::select_best_match_rows(self, query)
    }
}

/// Factory retaining the real private archive transport and capture-epoch witness.
pub struct NativeTemperatureRevisionArchiveDatabaseFactory {
    capture_witness: AdmittedAdjustmentRevisionCaptureWitness,
    client: Arc<NativeOutboundHttp>,
    origin: String,
    timeout: Duration,
}

impl NativeTemperatureRevisionArchiveDatabaseFactory {
    /// Retain admitted process owners without opening a database or making a request.
    pub fn new(
        capture_witness: AdmittedAdjustmentRevisionCaptureWitness,
        client: Arc<NativeOutboundHttp>,
        origin: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            capture_witness,
            client,
            origin: origin.into(),
            timeout,
        }
    }
}

impl NativeTemperatureCanaryDatabaseFactory for NativeTemperatureRevisionArchiveDatabaseFactory {
    type Database = NativeTemperatureRevisionArchiveDatabase;

    /// Compose the complete enabled archive database over shared process pools.
    fn bind_database(
        &mut self,
        pool: &NativePostgresPool,
        archive_mode: TemperatureCanaryRevisionArchiveMode,
    ) -> Result<Self::Database, TaskFailure> {
        // never install an archive owner under a disabled-mode configuration mismatch
        if archive_mode != TemperatureCanaryRevisionArchiveMode::Enabled {
            return Err(binding_failure("native_temperature_archive_mode_mismatch"));
        }
        let transport = NativePrivatePostTransport::new(Arc::clone(&self.client), self.timeout);
        let http = NativeWorkerRainMaintenanceHttpJson::new(NativeWirePrimitives, transport);
        let mut origins = NativeEdgeOrigins;
        let client = HttpWorkerRainMaintenanceClient::admit(&self.origin, &mut origins, http)
            .map_err(|_| binding_failure("native_temperature_archive_origin_invalid"))?;
        Ok(EcmwfRevisionArchiveDatabase::new(
            NativePostgresTemperatureSession::from_pool(pool.clone()),
            NativeTemperatureRevisionArchiveValues,
            NativeWirePrimitives,
            client,
            pool.clone(),
            self.capture_witness.clone(),
            map_archive_error as fn(EcmwfRevisionArchiveError) -> NativePostgresError,
        ))
    }
}

/// Map archive orchestration failures into the serving transaction's closed error domain.
fn map_archive_error(error: EcmwfRevisionArchiveError) -> NativePostgresError {
    let parameter = match error {
        EcmwfRevisionArchiveError::Identity => "ECMWF revision archive identity",
        EcmwfRevisionArchiveError::Marker => "ECMWF revision archive marker",
        EcmwfRevisionArchiveError::Port => "ECMWF revision archive port",
        EcmwfRevisionArchiveError::Publish => "ECMWF revision archive publish",
        EcmwfRevisionArchiveError::Recovery => "ECMWF revision archive recovery",
    };
    NativePostgresError::InvalidParameter(parameter)
}

/// Decode one exact database row wrapper and its authenticated commit receipt.
fn decode_revision_row_receipt(
    value: &JsonValue,
) -> Result<AdjustmentRevisionRowReceipt, NativePostgresError> {
    let JsonValue::Object(fields) = value else {
        return Err(NativePostgresError::decode("value", "expected object"));
    };
    // require the exact closed wrapper without duplicate or extension fields
    if fields.len() != 2
        || json_text(value, "contractVersion")?.as_str()
            != AdjustmentRevisionRowReceiptContractVersionLiteral::WIRE_VALUE
    {
        return Err(NativePostgresError::decode(
            "value",
            "unexpected revision row wrapper",
        ));
    }
    let receipt = json_member(value, "revisionReceipt")?;
    let revision_receipt = ArchiveWireCodec::new(NativeWirePrimitives)
        .decode_revision_commit_value(receipt)
        .ok_or_else(|| NativePostgresError::decode("revisionReceipt", "contract mismatch"))?;
    Ok(AdjustmentRevisionRowReceipt {
        contractVersion: AdjustmentRevisionRowReceiptContractVersionLiteral,
        revisionReceipt: revision_receipt,
    })
}

/// Build the exact ordered temperature projection JSON tree.
fn temperature_projection_value(
    projection: &AdjustmentTemperatureNativeSourceProjection,
) -> Option<JsonValue> {
    Some(JsonValue::Object(vec![
        member(
            "contractVersion",
            string(
                weather_contracts::AdjustmentTemperatureNativeSourceProjectionContractVersionLiteral::WIRE_VALUE,
            ),
        ),
        member(
            "family",
            string(
                weather_contracts::AdjustmentTemperatureNativeSourceProjectionFamilyLiteral::WIRE_VALUE,
            ),
        ),
        member("logicalKey", logical_object(&projection.logicalKey)?),
        member("logicalReceivedAt", string(&projection.logicalReceivedAt)),
        member(
            "projectionKind",
            string(
                weather_contracts::AdjustmentTemperatureNativeSourceProjectionProjectionKindLiteral::WIRE_VALUE,
            ),
        ),
        member(
            "recentErrorState",
            recent_error_value(&projection.recentErrorState),
        ),
        member(
            "recentErrorStateSha256",
            string(&projection.recentErrorStateSha256),
        ),
        member(
            "rows",
            JsonValue::Array(
                projection
                    .rows
                    .iter()
                    .map(|row| row_object(row))
                    .collect::<Option<Vec<_>>>()?,
            ),
        ),
        member("source", source_object(&projection.source)?),
        member("storedContentSha256", string(&projection.storedContentSha256)),
    ]))
}

/// Convert ordered logical-key scalar members without changing their stored order.
fn logical_object(
    fields: &[(Vec<u16>, AdjustmentRevisionProjectionLogicalKeyValue)],
) -> Option<JsonValue> {
    Some(JsonValue::Object(
        fields
            .iter()
            .map(|(key, value)| {
                Some((
                    key.clone(),
                    match value {
                        AdjustmentRevisionProjectionLogicalKeyValue::Null => JsonValue::Null,
                        AdjustmentRevisionProjectionLogicalKeyValue::Number(value)
                            if value.is_finite() =>
                        {
                            JsonValue::Number(*value)
                        }
                        AdjustmentRevisionProjectionLogicalKeyValue::String(value) => {
                            JsonValue::String(value.clone())
                        }
                        _ => return None,
                    },
                ))
            })
            .collect::<Option<Vec<_>>>()?,
    ))
}

/// Convert one ordered temperature row scalar object.
fn row_object(fields: &[(Vec<u16>, AdjustmentRevisionProjectionRowsValue)]) -> Option<JsonValue> {
    Some(JsonValue::Object(
        fields
            .iter()
            .map(|(key, value)| {
                Some((
                    key.clone(),
                    match value {
                        AdjustmentRevisionProjectionRowsValue::Null => JsonValue::Null,
                        AdjustmentRevisionProjectionRowsValue::Boolean(value) => {
                            JsonValue::Boolean(*value)
                        }
                        AdjustmentRevisionProjectionRowsValue::Number(value)
                            if value.is_finite() =>
                        {
                            JsonValue::Number(*value)
                        }
                        AdjustmentRevisionProjectionRowsValue::String(value) => {
                            JsonValue::String(value.clone())
                        }
                        _ => return None,
                    },
                ))
            })
            .collect::<Option<Vec<_>>>()?,
    ))
}

/// Convert the ordered nullable source object.
fn source_object(fields: &[(Vec<u16>, Nullable<Vec<u16>>)]) -> Option<JsonValue> {
    Some(JsonValue::Object(
        fields
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    match value {
                        Nullable::Null => JsonValue::Null,
                        Nullable::Value(value) => JsonValue::String(value.clone()),
                    },
                )
            })
            .collect(),
    ))
}

/// Convert the exact named-order recent-error state.
fn recent_error_value(state: &MaintenanceShadowTemperatureRecentErrorState) -> JsonValue {
    JsonValue::Object(vec![
        member("b24C", nullable_number(&state.b24C)),
        member("b72C", nullable_number(&state.b72C)),
        member(
            "cohort",
            string(MaintenanceShadowTemperatureRecentErrorStateCohortLiteral::WIRE_VALUE),
        ),
        member("localDates", JsonValue::Number(state.localDates as f64)),
        member("mad72C", nullable_number(&state.mad72C)),
        member(
            "maximumSourceRunInitializedAt",
            nullable_string(&state.maximumSourceRunInitializedAt),
        ),
        member(
            "maximumSourceValidAt",
            nullable_string(&state.maximumSourceValidAt),
        ),
        member("n24", JsonValue::Number(state.n24 as f64)),
        member("n72", JsonValue::Number(state.n72 as f64)),
        member(
            "sourceKeys",
            JsonValue::Array(state.sourceKeys.iter().map(|value| string(value)).collect()),
        ),
        member("supported", JsonValue::Boolean(state.supported)),
        member(
            "targetRunInitializedAt",
            string(&state.targetRunInitializedAt),
        ),
        member("windowEndValidAt", string(&state.windowEndValidAt)),
    ])
}

/// Encode compact JSON for one JSONB database parameter.
fn compact_json(value: JsonValue) -> Result<String, NativeTemperatureRevisionArchiveValueError> {
    let mut bytes = NativeWirePrimitives
        .encode_ordered_json(&value)
        .ok_or(NativeTemperatureRevisionArchiveValueError::Encoding)?;
    // remove only the archive encoder's required framing LF for JSON.stringify parity
    if bytes.pop() != Some(b'\n') {
        return Err(NativeTemperatureRevisionArchiveValueError::Encoding);
    }
    String::from_utf8(bytes).map_err(|_| NativeTemperatureRevisionArchiveValueError::Encoding)
}

/// Match JavaScript six-decimal formatting for finite ordinary coordinates.
fn javascript_fixed_six(value: f64) -> Option<String> {
    const SCALE: f64 = 1_000_000.0;
    // reject JSON-inexpressible values before formatting
    if !value.is_finite() {
        return None;
    }
    // normalize JavaScript's negative zero result
    if value == 0.0 {
        return Some("0.000000".to_owned());
    }
    let magnitude = value.abs();
    let scaled = magnitude * SCALE;
    // preserve exact midpoint rounding away from zero
    if scaled.fract() == 0.5 && magnitude.mul_add(SCALE, -scaled) == 0.0 {
        let rounded = scaled.ceil() as u64;
        let sign = if value < 0.0 { "-" } else { "" };
        return Some(format!(
            "{sign}{}.{:06}",
            rounded / 1_000_000,
            rounded % 1_000_000
        ));
    }
    Some(format!("{value:.6}"))
}

/// Convert one nullable finite number.
fn nullable_number(value: &Nullable<f64>) -> JsonValue {
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => JsonValue::Number(*value),
    }
}

/// Convert one nullable string.
fn nullable_string(value: &Nullable<String>) -> JsonValue {
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => string(value),
    }
}

/// Create one ordered JSON object member.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Create one JSON string using exact UTF-16 units.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Read one exact object member while rejecting duplicate keys.
fn json_member<'a>(value: &'a JsonValue, name: &str) -> Result<&'a JsonValue, NativePostgresError> {
    let JsonValue::Object(fields) = value else {
        return Err(NativePostgresError::decode(name, "expected object"));
    };
    let mut matches = fields
        .iter()
        .filter(|(key, _)| String::from_utf16(key).ok().is_some_and(|key| key == name));
    let value = matches
        .next()
        .map(|(_, value)| value)
        .ok_or_else(|| NativePostgresError::decode(name, "missing field"))?;
    // reject duplicate object keys before typed receipt decoding
    if matches.next().is_some() {
        return Err(NativePostgresError::decode(name, "duplicate field"));
    }
    Ok(value)
}

/// Read one required JSON string member.
fn json_text(value: &JsonValue, name: &str) -> Result<String, NativePostgresError> {
    let JsonValue::String(units) = json_member(value, name)? else {
        return Err(NativePostgresError::decode(name, "expected string"));
    };
    String::from_utf16(units).map_err(|error| NativePostgresError::decode(name, error))
}

/// Preserve one stable archive binding failure.
fn binding_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Match the original Node coordinate spelling for immutable temperature revision identities.
    #[test]
    fn native_temperature_archive_coordinate_spelling_matches_node() {
        let values = [
            47.950429954185445,
            -122.42797012608193,
            -0.0,
            -0.0000005,
            0.0000005,
            12.3456785,
        ];
        let expected = [
            "47.950430",
            "-122.427970",
            "0.000000",
            "-0.000000",
            "0.000000",
            "12.345678",
        ];
        assert_eq!(
            values.map(|value| javascript_fixed_six(value).unwrap()),
            expected
        );
    }
}
