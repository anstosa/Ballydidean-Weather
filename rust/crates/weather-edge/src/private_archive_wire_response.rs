//! Materialize private archive receipts and closed errors through the shared wire response.

use weather_contracts::{
    JsonValue, RainFixedGaugeTargetGapInputReasonLiteral,
    RainFixedGaugeTargetGapReceiptContractVersionLiteral,
    RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral, RainFixedGaugeTargetGapStatus,
    RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
    RainFixedGaugeTargetGapStatusAbsentStateLiteral,
    RainFixedGaugeTargetGapStatusPresentContractVersionLiteral,
    RainFixedGaugeTargetGapStatusPresentStateLiteral,
};

use crate::archive_auxiliary_wire::{
    rain_gap_value, rain_stage_receipt_value, revision_gap_value, shadow_gap_value,
};
use crate::archive_wire::{ArchiveWireCodec, ArchiveWirePrimitives, member, string};
use crate::edge_wire_response::{EdgeWireBody, EdgeWireResponse};
use crate::private_archive_service::{
    PrivateArchiveBodyReader, PrivateArchiveErrorBody, PrivateArchiveReceipt,
    PrivateArchiveResponse, PrivateArchiveResponseBody, PrivateArchiveStatus,
    PrivateArchiveStorage, PrivateArchiveTransportRequest, PrivateRevisionPublishReceipt,
    serve_private_archive_request,
};
use crate::revision_receipt_wire::{
    batch_publish_receipt_value, publish_receipt_value, stage_receipt_value,
};
use crate::shadow_receipt_wire::{shadow_publish_receipt_value, shadow_stage_receipt_value};

const NOT_FOUND_JSON: &[u8] = b"{\"error\":\"not_found\"}\n";
const REVISION_ARCHIVE_REJECTED_JSON: &[u8] = b"{\"error\":\"revision_archive_rejected\"}\n";
const REVISION_SPOOL_FULL_JSON: &[u8] = b"{\"error\":\"adjustment_revision_spool_full\"}\n";
const UNSUPPORTED_MEDIA_TYPE_JSON: &[u8] = b"{\"error\":\"unsupported_media_type\"}\n";

/// Approved serializer for every typed private archive success receipt.
pub trait PrivateArchiveWireReceiptEncoder {
    type Error;

    /// Encode one complete `sendJson` representation including its trailing line feed.
    fn encode_receipt(&mut self, receipt: &PrivateArchiveReceipt) -> Result<Vec<u8>, Self::Error>;
}

impl<P: ArchiveWirePrimitives> PrivateArchiveWireReceiptEncoder for ArchiveWireCodec<P> {
    type Error = ();

    /// serialize the closed listener receipt variants
    fn encode_receipt(&mut self, receipt: &PrivateArchiveReceipt) -> Result<Vec<u8>, Self::Error> {
        // use canonical order consistently since node's new and retained receipts can differ in order
        self.encode(&archive_receipt_value(receipt)).ok_or(())
    }
}

/// map one typed private archive receipt through the shared contract mappers
pub fn archive_receipt_value(receipt: &PrivateArchiveReceipt) -> JsonValue {
    // select the owning mapper for the closed receipt variant
    match receipt {
        PrivateArchiveReceipt::RainControlStateStaged(receipt) => rain_stage_receipt_value(receipt),
        PrivateArchiveReceipt::RainControlStateGapRecorded(receipt) => rain_gap_value(
            &receipt.gap_at,
            &receipt.stage_receipt_sha256,
            &receipt.state_sha256,
            Some(&receipt.gap_sha256),
        ),
        PrivateArchiveReceipt::RainFixedGaugeTargetGapRecorded(receipt) => fixed_gauge_gap_value(
            &receipt.gapAt,
            &receipt.gapSha256,
            &receipt.logicalHourAt,
            &receipt.logicalKeySha256,
        ),
        PrivateArchiveReceipt::RainFixedGaugeTargetGapStatus(status) => {
            fixed_gauge_status_value(status)
        }
        PrivateArchiveReceipt::ShadowRevisionStaged(receipt) => shadow_stage_receipt_value(receipt),
        PrivateArchiveReceipt::ShadowRevisionPublished(receipt) => {
            shadow_publish_receipt_value(receipt)
        }
        PrivateArchiveReceipt::ShadowGapRecorded(receipt) => shadow_gap_value(
            &receipt.input,
            &receipt.recorded_at,
            Some(&receipt.gap_sha256),
        ),
        PrivateArchiveReceipt::RevisionStaged(receipt) => stage_receipt_value(receipt),
        PrivateArchiveReceipt::RevisionPublished(PrivateRevisionPublishReceipt::Single(
            receipt,
        )) => publish_receipt_value(receipt),
        PrivateArchiveReceipt::RevisionPublished(PrivateRevisionPublishReceipt::Batch(receipt)) => {
            batch_publish_receipt_value(receipt)
        }
        PrivateArchiveReceipt::RevisionGapRecorded(receipt) => revision_gap_value(
            &receipt.input,
            &receipt.recorded_at,
            Some(&receipt.gap_sha256),
        ),
    }
}

/// Reuse one fixed-gauge disposition mapping for both returned contract forms.
fn fixed_gauge_gap_value(gap_at: &str, digest: &str, hour: &str, key: &str) -> JsonValue {
    JsonValue::Object(vec![
        member(
            "contractVersion",
            string(RainFixedGaugeTargetGapReceiptContractVersionLiteral::WIRE_VALUE),
        ),
        member("gapAt", string(gap_at)),
        member("gapSha256", string(digest)),
        member("logicalHourAt", string(hour)),
        member("logicalKeySha256", string(key)),
        member(
            "qualificationDisposition",
            string(RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral::WIRE_VALUE),
        ),
        member(
            "reason",
            string(RainFixedGaugeTargetGapInputReasonLiteral::WIRE_VALUE),
        ),
    ])
}

/// Materialize the generated status discriminant without adding a synthetic gap.
fn fixed_gauge_status_value(status: &RainFixedGaugeTargetGapStatus) -> JsonValue {
    match status {
        RainFixedGaugeTargetGapStatus::Absent(_) => JsonValue::Object(vec![
            member(
                "contractVersion",
                string(RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral::WIRE_VALUE),
            ),
            member(
                "state",
                string(RainFixedGaugeTargetGapStatusAbsentStateLiteral::WIRE_VALUE),
            ),
        ]),
        RainFixedGaugeTargetGapStatus::Present(present) => JsonValue::Object(vec![
            member(
                "contractVersion",
                string(RainFixedGaugeTargetGapStatusPresentContractVersionLiteral::WIRE_VALUE),
            ),
            member(
                "gap",
                fixed_gauge_gap_value(
                    &present.gap.gapAt,
                    &present.gap.gapSha256,
                    &present.gap.logicalHourAt,
                    &present.gap.logicalKeySha256,
                ),
            ),
            member(
                "state",
                string(RainFixedGaugeTargetGapStatusPresentStateLiteral::WIRE_VALUE),
            ),
        ]),
    }
}

/// Run lazy private admission and storage dispatch, then materialize its complete wire response.
pub fn serve_private_archive_wire_request<Reader, Storage, Encoder>(
    request: PrivateArchiveTransportRequest<'_>,
    body: &mut Reader,
    storage: &mut Storage,
    encoder: &mut Encoder,
) -> EdgeWireResponse
where
    Reader: PrivateArchiveBodyReader,
    Storage: PrivateArchiveStorage,
    Encoder: PrivateArchiveWireReceiptEncoder,
{
    let omit_body = request.method == "HEAD";
    let response = serve_private_archive_request(request, body, storage);
    let mut wire = materialize_private_archive_wire_response(response, encoder);
    // preserve the fixed representation length while the global writer suppresses head bodies
    if omit_body {
        wire.body = EdgeWireBody::Empty;
    }
    wire
}

/// Encode one typed private archive result before any transport writer observes headers.
pub fn materialize_private_archive_wire_response<Encoder>(
    response: PrivateArchiveResponse,
    encoder: &mut Encoder,
) -> EdgeWireResponse
where
    Encoder: PrivateArchiveWireReceiptEncoder,
{
    let status_code = response.status.code();
    // materialize success only through the injected canonical receipt serializer
    match response.body {
        PrivateArchiveResponseBody::Receipt(receipt) => match encoder.encode_receipt(&receipt) {
            Ok(bytes) => EdgeWireResponse::json(status_code, bytes, false, "no-store"),
            Err(_) => revision_archive_rejected(),
        },
        PrivateArchiveResponseBody::Error(error) => fixed_error(status_code, error),
    }
}

/// Select one fixed error representation without allocating or re-encoding JSON.
fn fixed_error(status_code: u16, error: PrivateArchiveErrorBody) -> EdgeWireResponse {
    // bind the closed error enum directly to its original JSON representation
    let body = match error {
        PrivateArchiveErrorBody::NotFound => NOT_FOUND_JSON,
        PrivateArchiveErrorBody::RevisionArchiveRejected => REVISION_ARCHIVE_REJECTED_JSON,
        PrivateArchiveErrorBody::RevisionSpoolFull => REVISION_SPOOL_FULL_JSON,
        PrivateArchiveErrorBody::UnsupportedMediaType => UNSUPPORTED_MEDIA_TYPE_JSON,
    };
    EdgeWireResponse::fixed_json(status_code, body, "no-store")
}

/// Collapse a pre-writer receipt encoding refusal to the source legacy conflict response.
fn revision_archive_rejected() -> EdgeWireResponse {
    EdgeWireResponse::fixed_json(
        PrivateArchiveStatus::Conflict.code(),
        REVISION_ARCHIVE_REJECTED_JSON,
        "no-store",
    )
}

#[cfg(test)]
mod tests {
    use weather_contracts::{
        RainFixedGaugeTargetGapStatus, RainFixedGaugeTargetGapStatusAbsent,
        RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
        RainFixedGaugeTargetGapStatusAbsentStateLiteral,
    };

    use crate::edge_wire_response::EdgeWireBody;

    use super::*;

    const RECEIPT_JSON: &[u8] = b"{\"contractVersion\":\"fixture/v1\",\"state\":\"absent\"}\n";

    #[derive(Default)]
    struct Encoder {
        calls: usize,
        refuse: bool,
    }

    impl PrivateArchiveWireReceiptEncoder for Encoder {
        type Error = ();

        /// Observe the exact typed receipt and return approved complete fixture bytes.
        fn encode_receipt(
            &mut self,
            receipt: &PrivateArchiveReceipt,
        ) -> Result<Vec<u8>, Self::Error> {
            self.calls += 1;
            assert!(matches!(
                receipt,
                PrivateArchiveReceipt::RainFixedGaugeTargetGapStatus(
                    RainFixedGaugeTargetGapStatus::Absent(_)
                )
            ));
            // inject only an ordinary serializer refusal before transport output
            if self.refuse {
                return Err(());
            }
            Ok(RECEIPT_JSON.to_vec())
        }
    }

    /// Materialize one successful typed receipt as the serializer's complete JSON bytes.
    #[test]
    fn materializes_successful_receipt_before_writer() {
        let mut encoder = Encoder::default();
        let response = materialize_private_archive_wire_response(success(), &mut encoder);
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.as_bytes(), RECEIPT_JSON);
        assert_eq!(response.headers.cache_control, "no-store");
        assert_eq!(response.headers.content_length, RECEIPT_JSON.len());
        assert_eq!(encoder.calls, 1);
    }

    /// Collapse an ordinary receipt serializer refusal to the legacy archive conflict.
    #[test]
    fn encoder_refusal_returns_revision_archive_rejected() {
        let mut encoder = Encoder {
            refuse: true,
            ..Encoder::default()
        };
        let response = materialize_private_archive_wire_response(success(), &mut encoder);
        assert_eq!(response.status_code, 409);
        assert!(matches!(response.body, EdgeWireBody::Static(_)));
        assert_eq!(response.body.as_bytes(), REVISION_ARCHIVE_REJECTED_JSON);
        assert_eq!(
            response.headers.content_length,
            REVISION_ARCHIVE_REJECTED_JSON.len()
        );
        assert_eq!(encoder.calls, 1);
    }

    /// Preserve both ordinary storage refusal categories as fixed private JSON.
    #[test]
    fn materializes_closed_storage_errors_without_encoder() {
        let mut encoder = Encoder::default();
        // compare each storage-owned rejection without invoking receipt encoding
        for (status, error, expected) in [
            (
                PrivateArchiveStatus::Conflict,
                PrivateArchiveErrorBody::RevisionArchiveRejected,
                REVISION_ARCHIVE_REJECTED_JSON,
            ),
            (
                PrivateArchiveStatus::TooManyRequests,
                PrivateArchiveErrorBody::RevisionSpoolFull,
                REVISION_SPOOL_FULL_JSON,
            ),
        ] {
            let response = materialize_private_archive_wire_response(
                PrivateArchiveResponse {
                    status,
                    body: PrivateArchiveResponseBody::Error(error),
                },
                &mut encoder,
            );
            assert_eq!(response.status_code, status.code());
            assert_eq!(response.body.as_bytes(), expected);
            assert_eq!(response.headers.content_length, expected.len());
        }
        assert_eq!(encoder.calls, 0);
    }

    /// Build one valid ordinary fixed-gauge status receipt without wire encoding it here.
    fn success() -> PrivateArchiveResponse {
        PrivateArchiveResponse {
            status: PrivateArchiveStatus::Ok,
            body: PrivateArchiveResponseBody::Receipt(
                PrivateArchiveReceipt::RainFixedGaugeTargetGapStatus(
                    RainFixedGaugeTargetGapStatus::Absent(RainFixedGaugeTargetGapStatusAbsent {
                        contractVersion: RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
                        state: RainFixedGaugeTargetGapStatusAbsentStateLiteral,
                    }),
                ),
            ),
        }
    }
}
