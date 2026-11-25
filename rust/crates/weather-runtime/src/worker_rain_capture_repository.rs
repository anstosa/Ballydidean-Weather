//! Move admitted worker capture evidence into the generated database contract.

use weather_contracts::{
    Nullable, RainCaptureOutcome as CaptureOutcomeContract,
    RainCaptureReceipt as CaptureReceiptContract,
    RainCaptureReceiptMetadataValue as CaptureMetadataContract,
    RainCaptureReceiptParserVersionLiteral,
};
use weather_database::{
    AdmittedRainCaptureClaimPolicy, RainCaptureClaimCalendar, RainCaptureClaimPool,
    RainCaptureClaimRequest, RainCaptureReceiptCanonicalValues, RainCaptureReceiptDatabase,
    SerializedJson, Sha256Digest, append_rain_capture_receipt, claim_rain_capture_slot,
};
use weather_worker::{
    RainCaptureMetadataValue, RainCaptureOutcome, RainCaptureReceipt, RainCaptureRequest,
    RainCollectionRepository, TaskFailure,
};

use crate::WorkerRainCaptureRepositoryBinding;

/// Bind an admitted immutable policy to the worker's claim and receipt statements.
pub struct DatabaseRainCaptureRepositoryBinding {
    policy_json: SerializedJson,
    policy_sha256: Sha256Digest,
}

impl DatabaseRainCaptureRepositoryBinding {
    /// Retain the exact serialized policy and its admitted digest without a new codec.
    pub const fn new(policy_json: SerializedJson, policy_sha256: Sha256Digest) -> Self {
        Self {
            policy_json,
            policy_sha256,
        }
    }
}

/// The claim and append repository borrowing canonical values and the existing worker pool.
pub struct DatabaseRainCaptureRepository<'a, Database, Values> {
    database: &'a mut Database,
    policy: AdmittedRainCaptureClaimPolicy<'a>,
    values: &'a mut Values,
}

impl<Database, Values> WorkerRainCaptureRepositoryBinding<Database, Values>
    for DatabaseRainCaptureRepositoryBinding
where
    Database: RainCaptureClaimPool + RainCaptureReceiptDatabase,
    Values: RainCaptureClaimCalendar + RainCaptureReceiptCanonicalValues,
{
    type Repository<'a>
        = DatabaseRainCaptureRepository<'a, Database, Values>
    where
        Database: 'a,
        Values: 'a;

    /// Reborrow the pool for the collector instead of creating a separate connection owner.
    fn bind<'a>(
        &'a mut self,
        database: &'a mut Database,
        values: &'a mut Values,
    ) -> Result<Self::Repository<'a>, TaskFailure> {
        Ok(DatabaseRainCaptureRepository {
            database,
            policy: AdmittedRainCaptureClaimPolicy {
                json: self.policy_json.as_str(),
                sha256: self.policy_sha256.as_str(),
            },
            values,
        })
    }
}

impl<Database, Values> RainCollectionRepository
    for DatabaseRainCaptureRepository<'_, Database, Values>
where
    Database: RainCaptureClaimPool + RainCaptureReceiptDatabase,
    Values: RainCaptureClaimCalendar + RainCaptureReceiptCanonicalValues,
{
    /// Commit the database-clock claim before the collector starts provider traffic.
    fn claim_rain_capture_slot(
        &mut self,
        request: &RainCaptureRequest,
        release: &str,
    ) -> Result<Option<String>, TaskFailure> {
        let request = match request {
            RainCaptureRequest::Forecast {
                attempt,
                run_initialized_at,
                slot_key,
            } => RainCaptureClaimRequest::Forecast {
                attempt: *attempt,
                run_initialized_at: run_initialized_at.as_str().into(),
                slot_key: slot_key.as_str().into(),
            },
            RainCaptureRequest::Station {
                end_exclusive,
                slot_key,
                start,
                station_id,
            } => RainCaptureClaimRequest::Station {
                end_exclusive: end_exclusive.as_str().into(),
                slot_key: slot_key.as_str().into(),
                start: start.as_str().into(),
                station_id: *station_id,
            },
        };
        claim_rain_capture_slot(self.database, self.values, request, release, self.policy)
            .map_err(|_| failure("rain_capture_claim_failed"))
    }

    /// Move the provider receipt into the generated contract before the checked append.
    fn append_rain_capture_receipt(
        &mut self,
        claim_id: &str,
        receipt: RainCaptureReceipt,
    ) -> Result<(), TaskFailure> {
        let receipt = project_capture_receipt(receipt)?;
        append_rain_capture_receipt(self.database, self.values, claim_id, &receipt)
            .map_err(|_| failure("rain_capture_receipt_append_failed"))
    }
}

/// Expose an operation category rather than internal database or provider strings.
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

/// Preserve the owned raw response while converting the exact generated receipt shape.
fn project_capture_receipt(
    receipt: RainCaptureReceipt,
) -> Result<CaptureReceiptContract, TaskFailure> {
    // the generated marker represents only the retained parser version
    if receipt.parser_version != RainCaptureReceiptParserVersionLiteral::WIRE_VALUE {
        return Err(TaskFailure {
            attempts: 0,
            code: "rain_capture_receipt_parser_invalid".to_owned(),
        });
    }
    let row_count = u64::try_from(receipt.row_count).map_err(|_| TaskFailure {
        attempts: 0,
        code: "rain_capture_receipt_count_invalid".to_owned(),
    })?;
    let outcome = match receipt.outcome {
        RainCaptureOutcome::Valid => CaptureOutcomeContract::Valid,
        RainCaptureOutcome::Invalid => CaptureOutcomeContract::Invalid,
        RainCaptureOutcome::TransportError => CaptureOutcomeContract::TransportError,
        RainCaptureOutcome::RateLimited => CaptureOutcomeContract::RateLimited,
        RainCaptureOutcome::Unauthorized => CaptureOutcomeContract::Unauthorized,
    };
    let metadata = receipt
        .metadata
        .into_iter()
        // retain the generated typed scalar envelope for the admitted JSON encoder
        .map(|(key, value)| (key.encode_utf16().collect(), project_scalar(value)))
        .collect();
    Ok(CaptureReceiptContract {
        startedAt: receipt.started_at,
        completedAt: receipt.completed_at,
        httpStatus: nullable(receipt.http_status.map(u64::from)),
        body: nullable(receipt.body),
        bodySha256: nullable(receipt.body_sha256),
        outcome,
        errorCode: nullable(receipt.error_code),
        parserVersion: RainCaptureReceiptParserVersionLiteral,
        rowCount: row_count,
        availableByDecision: nullable(receipt.available_by_decision),
        metadata,
    })
}

/// Map one typed scalar without adding an independent serializer.
fn project_scalar(value: RainCaptureMetadataValue) -> CaptureMetadataContract {
    match value {
        RainCaptureMetadataValue::String(value) => {
            CaptureMetadataContract::String(value.encode_utf16().collect())
        }
        RainCaptureMetadataValue::Number(value) => CaptureMetadataContract::Number(value),
        RainCaptureMetadataValue::Boolean(value) => CaptureMetadataContract::Boolean(value),
        RainCaptureMetadataValue::Null => CaptureMetadataContract::Null,
    }
}

/// Keep Node nullability without synthesizing omitted data.
fn nullable<T>(value: Option<T>) -> Nullable<T> {
    match value {
        Some(value) => Nullable::Value(value),
        None => Nullable::Null,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// Retain the captured response allocation and all outcome metadata through projection.
    #[test]
    fn generated_receipt_keeps_owned_response_bytes() {
        let response = vec![0, 1, 255];
        let original_buffer = response.as_ptr();
        let projected = project_capture_receipt(RainCaptureReceipt {
            available_by_decision: Some(false),
            body: Some(response),
            body_sha256: Some("fixture-digest".to_owned()),
            completed_at: "2026-09-13T06:00:00.000Z".to_owned(),
            error_code: Some("provider_rate_limited".to_owned()),
            http_status: Some(429),
            metadata: BTreeMap::from([(
                "retryAfterSeconds".to_owned(),
                RainCaptureMetadataValue::Number(60.0),
            )]),
            outcome: RainCaptureOutcome::RateLimited,
            parser_version: RainCaptureReceiptParserVersionLiteral::WIRE_VALUE,
            row_count: 0,
            started_at: "2026-09-13T05:59:59.000Z".to_owned(),
        })
        .unwrap();
        // verify that conversion did not allocate another raw body
        let Nullable::Value(body) = projected.body else {
            panic!("expected retained raw provider evidence");
        };
        assert_eq!(body.as_ptr(), original_buffer);
        assert_eq!(body, [0, 1, 255]);
        assert_eq!(projected.outcome, CaptureOutcomeContract::RateLimited);
        assert_eq!(projected.httpStatus, Nullable::Value(429));
        assert_eq!(projected.availableByDecision, Nullable::Value(false));
        assert_eq!(
            projected.metadata,
            [(
                "retryAfterSeconds".encode_utf16().collect(),
                CaptureMetadataContract::Number(60.0)
            )]
        );
    }
}
