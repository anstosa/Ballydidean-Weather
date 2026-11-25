//! Concrete private maintenance response encoding through the shared archive receipt projection.

use weather_api::{
    AdjustmentMaintenanceErrorCode, AdjustmentMaintenanceResponseBody,
    AdjustmentMaintenanceWireEncoder, AdjustmentRevisionPublishResult,
};
use weather_contracts::{
    AdjustmentShadowFamily, AdjustmentShadowRegistrationSlot,
    AdjustmentShadowRegistrationSlotContractVersionLiteral,
    AdjustmentShadowRegistrationSlotStateEnum, JsonValue,
};
use weather_edge::{
    ArchiveWirePrimitives, PrivateArchiveReceipt, PrivateRevisionPublishReceipt,
    archive_receipt_value,
};

use crate::api_service_json_values::{array, boolean, enum_text, nullable, object, text};

/// Keep the selected serializer and only the most recent complete maintenance response bytes.
pub struct NativeAdjustmentMaintenanceWireEncoder<P> {
    primitives: P,
    bytes: Vec<u8>,
}

impl<P: ArchiveWirePrimitives> NativeAdjustmentMaintenanceWireEncoder<P> {
    /// Bind an approved JSON primitive without creating another runtime dependency.
    pub const fn new(primitives: P) -> Self {
        Self {
            primitives,
            bytes: Vec::new(),
        }
    }
}

impl<P: ArchiveWirePrimitives> AdjustmentMaintenanceWireEncoder
    for NativeAdjustmentMaintenanceWireEncoder<P>
{
    type Error = ();

    /// Materialize the existing private DTO in producer order with no final line feed.
    fn encode<'a>(
        &'a mut self,
        body: &AdjustmentMaintenanceResponseBody,
    ) -> Result<&'a [u8], Self::Error> {
        let value = adjustment_maintenance_response_value(body);
        let mut bytes = self.primitives.encode_ordered_json(&value).ok_or(())?;
        // reuse the serializer buffer instead of copying it to remove the archive LF
        if bytes.pop() != Some(b'\n') {
            return Err(());
        }
        self.bytes = bytes;
        Ok(&self.bytes)
    }
}

/// Project every typed private maintenance success or error into its exact Node JSON shape.
pub fn adjustment_maintenance_response_value(
    body: &AdjustmentMaintenanceResponseBody,
) -> JsonValue {
    // retain fixed bodies and route all archive-backed receipts through their single shared mapper
    match body {
        AdjustmentMaintenanceResponseBody::ScheduleSlots(slots) => {
            array(slots, schedule_slot_value)
        }
        AdjustmentMaintenanceResponseBody::Accepted => object([("accepted", boolean(true))]),
        AdjustmentMaintenanceResponseBody::Recorded => object([("recorded", boolean(true))]),
        AdjustmentMaintenanceResponseBody::ShadowStage(receipt) => archive_receipt_value(
            &PrivateArchiveReceipt::ShadowRevisionStaged(receipt.clone()),
        ),
        AdjustmentMaintenanceResponseBody::ShadowPublish(receipt) => archive_receipt_value(
            &PrivateArchiveReceipt::ShadowRevisionPublished(receipt.clone()),
        ),
        AdjustmentMaintenanceResponseBody::RevisionStage(receipt) => {
            archive_receipt_value(&PrivateArchiveReceipt::RevisionStaged(receipt.clone()))
        }
        AdjustmentMaintenanceResponseBody::RevisionPublish(result) => {
            revision_publish_value(result)
        }
        AdjustmentMaintenanceResponseBody::RainControlStateStage(receipt) => archive_receipt_value(
            &PrivateArchiveReceipt::RainControlStateStaged(receipt.clone()),
        ),
        AdjustmentMaintenanceResponseBody::RainFixedGaugeTargetGap(receipt) => {
            archive_receipt_value(&PrivateArchiveReceipt::RainFixedGaugeTargetGapRecorded(
                receipt.clone(),
            ))
        }
        AdjustmentMaintenanceResponseBody::RainFixedGaugeTargetGapStatus(status) => {
            archive_receipt_value(&PrivateArchiveReceipt::RainFixedGaugeTargetGapStatus(
                status.clone(),
            ))
        }
        AdjustmentMaintenanceResponseBody::Error(error) => {
            object([("error", text(maintenance_error_code(*error)))])
        }
    }
}

/// Preserve the database projection's member order and explicit nullable occupancy fields.
fn schedule_slot_value(slot: &AdjustmentShadowRegistrationSlot) -> JsonValue {
    object([
        (
            "contractVersion",
            text(AdjustmentShadowRegistrationSlotContractVersionLiteral::WIRE_VALUE),
        ),
        ("epochWitnessSha256", text(&slot.epochWitnessSha256)),
        (
            "family",
            enum_text(slot.family, AdjustmentShadowFamily::WIRE_VALUES),
        ),
        ("horizonEndAt", text(&slot.horizonEndAt)),
        (
            "registrationSha256",
            nullable(&slot.registrationSha256, |value| text(value)),
        ),
        ("scheduleContractSha256", text(&slot.scheduleContractSha256)),
        (
            "state",
            enum_text(
                slot.state,
                AdjustmentShadowRegistrationSlotStateEnum::WIRE_VALUES,
            ),
        ),
        (
            "terminalAt",
            nullable(&slot.terminalAt, |value| text(value)),
        ),
    ])
}

/// Rebind either revision publication form to the shared archive receipt category.
fn revision_publish_value(result: &AdjustmentRevisionPublishResult) -> JsonValue {
    // keep the single and batch receipt variants distinct at the shared archive boundary
    let receipt = match result {
        AdjustmentRevisionPublishResult::Single(receipt) => {
            PrivateRevisionPublishReceipt::Single(receipt.clone())
        }
        AdjustmentRevisionPublishResult::Batch(receipt) => {
            PrivateRevisionPublishReceipt::Batch(receipt.clone())
        }
    };
    archive_receipt_value(&PrivateArchiveReceipt::RevisionPublished(receipt))
}

/// Select the source-fixed JSON error spelling without widening the closed category.
const fn maintenance_error_code(error: AdjustmentMaintenanceErrorCode) -> &'static str {
    // mirror the fixed representations owned by the API maintenance wire boundary
    match error {
        AdjustmentMaintenanceErrorCode::NotFound => "not_found",
        AdjustmentMaintenanceErrorCode::RevisionSpoolFull => "revision_spool_full",
        AdjustmentMaintenanceErrorCode::ShadowRejected => "shadow_rejected",
    }
}
