//! Native PostgreSQL execution of the existing two-statement rain receipt repository.

use std::time::SystemTime;

use tokio_postgres::Row;
use weather_contracts::RainCaptureOutcome;
use weather_database::{
    PreparedRainCaptureReceipt, RainCaptureClaimRow, RainCaptureReceiptClaimKind,
    RainCaptureReceiptDatabase,
};

use crate::{
    NativePostgresError, NativePostgresParameter as Parameter, NativePostgresPool,
    NativePostgresType, native_postgres_weather_rows as field,
};

impl RainCaptureReceiptDatabase for NativePostgresPool {
    type Error = NativePostgresError;

    /// Read the exact prior claim by its typed UUID.
    fn select_claim(
        &mut self,
        statement: &'static str,
        claim_id: &str,
    ) -> Result<Option<RainCaptureClaimRow>, Self::Error> {
        let mut session = self.checkout()?;
        let rows = session.query(statement, &[Parameter::Uuid(claim_id.to_owned())]);
        let release = session.release();
        let rows = rows?;
        release?;
        // do not silently select among multiple prior claim rows
        match rows.as_slice() {
            [] => Ok(None),
            [row] => decode_claim(row).map(Some),
            _ => Err(NativePostgresError::decode(
                "rain capture claim",
                "multiple rows",
            )),
        }
    }

    /// Persist the prepared immutable receipt through the original insert statement.
    fn insert_receipt(
        &mut self,
        statement: &'static str,
        receipt: &PreparedRainCaptureReceipt,
    ) -> Result<(), Self::Error> {
        let parameters = receipt_parameters(receipt)?;
        let mut session = self.checkout()?;
        let inserted = session.execute(statement, &parameters);
        let release = session.release();
        let affected = inserted?;
        release?;
        // the original insert must append exactly one receipt
        if affected != 1 {
            return Err(NativePostgresError::decode(
                "rain capture receipt",
                "unexpected affected row count",
            ));
        }
        Ok(())
    }
}

/// Decode the claim's actual database clock and discriminator.
fn decode_claim(row: &Row) -> Result<RainCaptureClaimRow, NativePostgresError> {
    let kind = match field::column::<String>(row, "kind")?.as_str() {
        "forecast" => RainCaptureReceiptClaimKind::Forecast,
        "station" => RainCaptureReceiptClaimKind::Station,
        _ => {
            return Err(NativePostgresError::decode(
                "kind",
                "unknown rain capture kind",
            ));
        }
    };
    let claimed_at = field::column::<SystemTime>(row, "claimed_at")?;
    let run_initialized_at = field::column::<Option<SystemTime>>(row, "run_initialized_at")?;
    Ok(RainCaptureClaimRow {
        id: field::id(row, "id")?,
        kind,
        claimed_at_ms: receipt_milliseconds(claimed_at, "claimed_at")?,
        run_initialized_at_ms: run_initialized_at
            .map(|time| receipt_milliseconds(time, "run_initialized_at"))
            .transpose()?,
    })
}

/// Convert the driver's native timestamp without substituting process time.
fn receipt_milliseconds(time: SystemTime, column: &str) -> Result<i64, NativePostgresError> {
    field::epoch_milliseconds(time)
        .ok_or_else(|| NativePostgresError::decode(column, "timestamp is out of range"))
}

/// Bind the original fourteen receipt fields to their exact PostgreSQL domains.
fn receipt_parameters(
    receipt: &PreparedRainCaptureReceipt,
) -> Result<[Parameter; 14], NativePostgresError> {
    let outcome = match receipt.outcome {
        RainCaptureOutcome::Valid => "valid",
        RainCaptureOutcome::Invalid => "invalid",
        RainCaptureOutcome::TransportError => "transport_error",
        RainCaptureOutcome::RateLimited => "rate_limited",
        RainCaptureOutcome::Unauthorized => "unauthorized",
    };
    let http_status = receipt
        .http_status
        .map(i16::try_from)
        .transpose()
        .map_err(|_| NativePostgresError::InvalidParameter("http_status"))?;
    Ok([
        Parameter::Uuid(receipt.claim_id.clone()),
        Parameter::Timestamp(receipt.started_at.clone()),
        Parameter::Timestamp(receipt.completed_at.clone()),
        http_status.map_or(Parameter::Null(NativePostgresType::I16), Parameter::I16),
        Parameter::Text(outcome.to_owned()),
        optional_text(receipt.error_code.as_deref()),
        receipt
            .compressed_body
            .clone()
            .map_or(Parameter::Null(NativePostgresType::Bytes), Parameter::Bytes),
        optional_text(receipt.body_sha256.as_ref().map(|value| value.as_str())),
        optional_count(receipt.body_bytes)?,
        optional_count(receipt.compressed_bytes)?,
        Parameter::Text(receipt.parser_version.to_owned()),
        Parameter::I32(
            i32::try_from(receipt.row_count)
                .map_err(|_| NativePostgresError::ParameterOutOfRange)?,
        ),
        receipt
            .available_by_decision
            .map_or(Parameter::Null(NativePostgresType::Bool), Parameter::Bool),
        Parameter::Json(receipt.metadata_json.as_str().to_owned()),
    ])
}

/// Preserve explicit text nulls instead of substituting empty strings.
fn optional_text(value: Option<&str>) -> Parameter {
    value.map_or(Parameter::Null(NativePostgresType::Text), |value| {
        Parameter::Text(value.to_owned())
    })
}

/// Preserve nullable integer counts using the migration's exact signed width.
fn optional_count(value: Option<u64>) -> Result<Parameter, NativePostgresError> {
    value.map_or(Ok(Parameter::Null(NativePostgresType::I32)), |value| {
        i32::try_from(value)
            .map(Parameter::I32)
            .map_err(|_| NativePostgresError::ParameterOutOfRange)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_database::{SerializedJson, Sha256Digest};

    /// Retain the exact placeholder order and original nullable storage values.
    #[test]
    fn native_rain_receipt_parameters_preserve_database_types() {
        let receipt = PreparedRainCaptureReceipt {
            available_by_decision: Some(true),
            body_bytes: Some(3),
            body_sha256: Some(Sha256Digest::admitted("a".repeat(64))),
            claim_id: "01234567-89ab-4cde-8123-0123456789ab".to_owned(),
            completed_at: "2026-11-24T12:00:01.000Z".to_owned(),
            compressed_body: Some(vec![1, 2, 3, 4]),
            compressed_bytes: Some(4),
            error_code: None,
            http_status: Some(200),
            metadata_json: SerializedJson::admitted("{}"),
            outcome: RainCaptureOutcome::Valid,
            parser_version: "rain-prospective-capture/v1",
            row_count: 1,
            started_at: "2026-11-24T12:00:00.000Z".to_owned(),
        };
        assert_eq!(
            receipt_parameters(&receipt).unwrap(),
            [
                Parameter::Uuid(receipt.claim_id),
                Parameter::Timestamp(receipt.started_at),
                Parameter::Timestamp(receipt.completed_at),
                Parameter::I16(200),
                Parameter::Text("valid".to_owned()),
                Parameter::Null(NativePostgresType::Text),
                Parameter::Bytes(vec![1, 2, 3, 4]),
                Parameter::Text("a".repeat(64)),
                Parameter::I32(3),
                Parameter::I32(4),
                Parameter::Text("rain-prospective-capture/v1".to_owned()),
                Parameter::I32(1),
                Parameter::Bool(true),
                Parameter::Json("{}".to_owned()),
            ]
        );
    }
}
