//! Canonical Node-compatible wire envelopes for native forecast cycle transfers.

use weather_contracts::{
    ArchiveTransferFinalEnvelopeV1KindLiteral, ArchiveTransferIdleEnvelopeV1,
    ArchiveTransferIdleEnvelopeV1ContractVersionLiteral, ArchiveTransferIdleEnvelopeV1KindLiteral,
    ArchiveTransferPageEnvelopeV1, ArchiveTransferPageEnvelopeV1ContractVersionLiteral,
    ArchiveTransferPageEnvelopeV1KindLiteral, JsonValue,
};

use crate::ForecastEvidenceReceiptCodec;
use crate::revision_archive_canonical::RevisionArchiveCanonicalCodec;

use super::forecast_cycle_final_ack::{
    ForecastCycleFinalAckCodec, ForecastCycleFinalAcknowledgement,
};
use super::forecast_cycle_final_binding::ForecastCycleFinalManifest;
use super::forecast_cycle_final_store::ForecastCycleFinalManifestCodec;
use super::forecast_cycle_online_next::{
    FORECAST_ARCHIVE_TRANSFER_CONTRACT_VERSION, ForecastCycleOnlineNext,
};
use super::forecast_cycle_page_binding::{
    BoundForecastCyclePageCodec, digest, object, page_header_document, projection_document, text,
    wire_page_header, wire_projection,
};
use super::forecast_cycle_transfer_checkpoint::{
    ForecastCycleTransferCheckpoint, ForecastCycleTransferCheckpointCodec,
};

const ARCHIVE_TRANSFER_MAXIMUM_BYTES: usize = 1_024 * 1_024;
const ARCHIVE_ACKNOWLEDGEMENT_MAXIMUM_BYTES: usize = 2_048;

/// Canonical contract, base64, manifest-binding, and response-bound failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastCycleOnlineWireError {
    AcknowledgementTooLarge,
    Base64,
    Canonical,
    InvalidContract,
    InvalidManifestIdentity,
    TransferTooLarge,
}

/// Encode one exact final-before-page-before-idle native selection for the Node command wire.
pub fn encode_forecast_cycle_online_next<Codec: ForecastEvidenceReceiptCodec>(
    next: &ForecastCycleOnlineNext<ForecastCycleFinalManifest>,
    codec: &BoundForecastCyclePageCodec<'_, Codec>,
) -> Result<Vec<u8>, ForecastCycleOnlineWireError> {
    use ForecastCycleOnlineWireError as Error;

    let value = match next {
        ForecastCycleOnlineNext::Final {
            contract_version,
            manifest,
            manifest_sha256,
        } => {
            // refuse a caller-constructed final that differs from its immutable manifest custody
            require_transfer_contract(contract_version)?;
            let manifest_bytes = codec
                .encode_final_manifest(manifest)
                .ok_or(Error::Canonical)?;
            if codec.sha256(&manifest_bytes) != *manifest_sha256.as_bytes()
                || codec.frame(manifest.document()).as_deref() != Some(manifest_bytes.as_slice())
            {
                return Err(Error::InvalidManifestIdentity);
            }
            object([
                ("contractVersion", text(contract_version)),
                (
                    "kind",
                    text(ArchiveTransferFinalEnvelopeV1KindLiteral::WIRE_VALUE),
                ),
                ("manifest", manifest.document().clone()),
                ("manifestSha256", digest(*manifest_sha256)),
            ])
        }
        ForecastCycleOnlineNext::Page {
            contract_version,
            header,
            page_sha256,
            projections,
            payload,
        } => {
            require_transfer_contract(contract_version)?;
            let wire = ArchiveTransferPageEnvelopeV1 {
                contractVersion: ArchiveTransferPageEnvelopeV1ContractVersionLiteral,
                kind: ArchiveTransferPageEnvelopeV1KindLiteral,
                header: wire_page_header(header).ok_or(Error::Canonical)?,
                pageSha256: crate::revision_archive_canonical::lower_hex(*page_sha256.as_bytes()),
                payloadBase64: codec
                    .byte_codec()
                    .standard_base64(payload)
                    .ok_or(Error::Base64)?,
                projections: projections.iter().map(wire_projection).collect(),
            };
            object([
                (
                    "contractVersion",
                    text(ArchiveTransferPageEnvelopeV1ContractVersionLiteral::WIRE_VALUE),
                ),
                (
                    "kind",
                    text(ArchiveTransferPageEnvelopeV1KindLiteral::WIRE_VALUE),
                ),
                (
                    "header",
                    page_header_document(&wire.header).ok_or(Error::Canonical)?,
                ),
                ("pageSha256", text(&wire.pageSha256)),
                (
                    "projections",
                    JsonValue::Array(wire.projections.iter().map(projection_document).collect()),
                ),
                ("payloadBase64", text(&wire.payloadBase64)),
            ])
        }
        ForecastCycleOnlineNext::Idle { contract_version } => {
            require_transfer_contract(contract_version)?;
            idle_document(ArchiveTransferIdleEnvelopeV1 {
                contractVersion: ArchiveTransferIdleEnvelopeV1ContractVersionLiteral,
                kind: ArchiveTransferIdleEnvelopeV1KindLiteral,
            })
        }
    };
    let bytes = codec.frame(&value).ok_or(Error::Canonical)?;
    // retain the forced-command response ceiling at the final canonical byte boundary
    if bytes.len() > ARCHIVE_TRANSFER_MAXIMUM_BYTES {
        return Err(Error::TransferTooLarge);
    }
    Ok(bytes)
}

/// Render the generated idle contract with no additional envelope fields.
fn idle_document(
    ArchiveTransferIdleEnvelopeV1 {
        contractVersion: _,
        kind: _,
    }: ArchiveTransferIdleEnvelopeV1,
) -> JsonValue {
    object([
        (
            "contractVersion",
            text(ArchiveTransferIdleEnvelopeV1ContractVersionLiteral::WIRE_VALUE),
        ),
        (
            "kind",
            text(ArchiveTransferIdleEnvelopeV1KindLiteral::WIRE_VALUE),
        ),
    ])
}

/// Encode the exact canonical page-checkpoint response already admitted by its owner.
pub fn encode_forecast_cycle_page_acknowledgement<Codec: ForecastCycleTransferCheckpointCodec>(
    acknowledgement: &ForecastCycleTransferCheckpoint,
    codec: &Codec,
) -> Result<Vec<u8>, ForecastCycleOnlineWireError> {
    encode_bounded_acknowledgement(codec.encode_transfer_checkpoint(acknowledgement))
}

/// Encode the exact canonical final cold-graph response already admitted by its owner.
pub fn encode_forecast_cycle_final_acknowledgement<Codec: ForecastCycleFinalAckCodec>(
    acknowledgement: &ForecastCycleFinalAcknowledgement,
    codec: &Codec,
) -> Result<Vec<u8>, ForecastCycleOnlineWireError> {
    encode_bounded_acknowledgement(codec.encode_final_acknowledgement(acknowledgement))
}

// require the one exact contract shared by every closed transfer variant
fn require_transfer_contract(contract_version: &str) -> Result<(), ForecastCycleOnlineWireError> {
    // reject manually constructed variants before any value-bearing bytes are encoded
    if contract_version != FORECAST_ARCHIVE_TRANSFER_CONTRACT_VERSION {
        return Err(ForecastCycleOnlineWireError::InvalidContract);
    }
    Ok(())
}

// preserve the native acknowledgement codec while applying its fixed response ceiling
fn encode_bounded_acknowledgement(
    bytes: Option<Vec<u8>>,
) -> Result<Vec<u8>, ForecastCycleOnlineWireError> {
    let bytes = bytes.ok_or(ForecastCycleOnlineWireError::Canonical)?;
    // refuse an acknowledgement outside the same bounded immutable source schema
    if bytes.len() > ARCHIVE_ACKNOWLEDGEMENT_MAXIMUM_BYTES {
        return Err(ForecastCycleOnlineWireError::AcknowledgementTooLarge);
    }
    Ok(bytes)
}
