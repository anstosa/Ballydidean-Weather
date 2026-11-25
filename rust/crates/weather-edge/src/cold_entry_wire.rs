//! Concrete cold-page payload and entry mappings over the selected wire primitives.

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionProjectionKind, JsonValue,
};

use crate::archive_auxiliary_wire::{
    rain_stage_receipt_value, revision_gap_value, shadow_gap_value,
};
use crate::archive_wire::{
    ArchiveWireCodec, ArchiveWirePrimitives, array, hash, member, string, text,
};
use crate::native_revision_spool::{
    NativeRevisionColdEntryCodec, NativeRevisionColdEntryMaterial, NativeRevisionColdPayloadKind,
    NativeRevisionColdPublication, NativeRevisionColdStageReceipt,
};
use crate::private_revision_archive::{
    AdmittedPrivateRevisionProjection, PrivateRevisionProjectionDependency,
    PrivateRevisionProjectionShape,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, lower_hex};
use crate::revision_cold_ack_material::RevisionColdAckMaterialCodec;
use crate::revision_receipt_wire::{
    batch_publish_receipt_value, commit_receipt_value, decode_commit_receipt,
    publish_receipt_value, stage_receipt_value,
};
use crate::shadow_receipt_wire::{shadow_publish_receipt_value, shadow_stage_receipt_value};

impl<P: ArchiveWirePrimitives> NativeRevisionColdEntryCodec for ArchiveWireCodec<P> {
    /// Decode and authenticate one exact retained database receipt.
    fn admitted_cold_commit_receipt(
        &self,
        bytes: &[u8],
    ) -> Option<AdjustmentRevisionCommitReceipt> {
        decode_commit_receipt(self, &self.decode_canonical(bytes)?)
    }

    /// Read only metadata needed to export the already retained original cold payload.
    fn admitted_cold_projection(
        &self,
        bytes: &[u8],
    ) -> Option<(
        NativeRevisionColdPayloadKind,
        AdmittedPrivateRevisionProjection,
    )> {
        use NativeRevisionColdPayloadKind as Kind;
        let value = self.decode(bytes)?;
        let contract = text(&value, "contractVersion")?;
        let kind = match contract.as_str() {
            "adjustment-revision-projection/v1" => Kind::Revision,
            "adjustment-revision-batch-projection/v2" => Kind::BatchRevision,
            "adjustment-rain-gate-feature-projection/v2" => Kind::RainGateFeature,
            "adjustment-rain-gate-control-projection/v3" => Kind::RainGateControl,
            "adjustment-rain-fixed-gauge-target-projection/v1" => Kind::RainFixedGaugeTarget,
            "adjustment-temperature-native-source-projection/v2" => Kind::TemperatureNativeSource,
            _ => return None,
        };
        let kind_text = text(&value, "projectionKind")?;
        let projection_kind = AdjustmentRevisionProjectionKind::WIRE_VALUES
            .iter()
            .find_map(
                // map only the original generated producer kind
                |(wire, value)| (*wire == kind_text).then_some(*value),
            )?;
        // retain the complete grouped row count only for the two additive cold contracts
        let shape = if matches!(kind, Kind::BatchRevision | Kind::RainFixedGaugeTarget) {
            PrivateRevisionProjectionShape::Grouped {
                row_count: array(&value, "rows")?.len(),
            }
        } else {
            PrivateRevisionProjectionShape::Single
        };
        // preserve the independent state member binding for the control projection
        let dependency = if kind == Kind::RainGateControl {
            PrivateRevisionProjectionDependency::RainControlState {
                ordinal_artifact_sha256: hash(&value, "ordinalArtifactSha256")?,
                state_sha256: hash(&value, "stateSha256")?,
                state_stage_receipt_sha256: hash(&value, "stateStageReceiptSha256")?,
            }
        } else {
            PrivateRevisionProjectionDependency::None
        };
        Some((
            kind,
            AdmittedPrivateRevisionProjection {
                projection_kind,
                shape,
                dependency,
            },
        ))
    }

    /// Encode the exact original cold entry around already admitted typed material.
    fn encode_cold_entry(&self, material: &NativeRevisionColdEntryMaterial<'_>) -> Option<Vec<u8>> {
        let payload = JsonValue::Object(vec![
            member(
                "bytesBase64",
                string(&self.base64_encode(material.payload_bytes)?),
            ),
            member("identitySha256", string(&material.payload_identity_sha256)),
            member("kind", string(payload_kind(material.payload_kind))),
        ]);
        let publication = publication_value(&material.publication);
        let stage = match &material.stage_receipt {
            NativeRevisionColdStageReceipt::Revision(receipt) => stage_receipt_value(receipt),
            NativeRevisionColdStageReceipt::Shadow(receipt) => shadow_stage_receipt_value(receipt),
        };
        let mut fields = vec![
            member("payload", payload),
            member("publication", publication),
        ];
        // preserve the grouped wire shape even for the one-receipt case
        if material.batch {
            if material.rain_control.is_some()
                || material.receipts.is_empty()
                || material.receipts.len() != material.successor_bytes.len()
            {
                return None;
            }
            fields.push(member(
                "receipts",
                JsonValue::Array(material.receipts.iter().map(commit_receipt_value).collect()),
            ));
            fields.push(member("stageReceipt", stage));
            let successors = material
                .successor_bytes
                .iter()
                .map(
                    // decode each retained canonical successor without changing its fields
                    |bytes| self.decode_canonical(bytes),
                )
                .collect::<Option<Vec<_>>>()?;
            fields.push(member("successors", JsonValue::Array(successors)));
        } else {
            let [receipt] = material.receipts else {
                return None;
            };
            let [successor] = material.successor_bytes.as_slice() else {
                return None;
            };
            // add the original public monthly state only for the exact control projection
            if let Some(rain) = &material.rain_control {
                fields.push(member(
                    "rainControlState",
                    JsonValue::Object(vec![
                        member(
                            "bytesBase64",
                            string(&self.base64_encode(&rain.state_bytes)?),
                        ),
                        member(
                            "identitySha256",
                            string(&lower_hex(self.sha256(&rain.state_bytes))),
                        ),
                        member("kind", string("rain-maintenance-control-state/v1")),
                        member("stateSha256", string(&rain.state_sha256)),
                    ]),
                ));
                fields.push(member(
                    "rainControlStateStageReceipt",
                    rain_stage_receipt_value(&rain.stage_receipt),
                ));
            }
            fields.push(member("receipt", commit_receipt_value(receipt)));
            fields.push(member("stageReceipt", stage));
            fields.push(member("successor", self.decode_canonical(successor)?));
        }
        self.encode(&JsonValue::Object(fields))
    }
}

impl<P: ArchiveWirePrimitives> RevisionColdAckMaterialCodec for ArchiveWireCodec<P> {
    /// Use the admitted primitive to decode only canonical standard base64.
    fn decode_canonical_base64(&self, value: &str) -> Option<Vec<u8>> {
        self.base64_decode(value)
    }
}

/// Map the retained payload contract without altering its wire label.
fn payload_kind(kind: NativeRevisionColdPayloadKind) -> &'static str {
    use NativeRevisionColdPayloadKind as Kind;
    match kind {
        Kind::Revision => "adjustment-revision-projection/v1",
        Kind::BatchRevision => "adjustment-revision-batch-projection/v2",
        Kind::RainGateFeature => "adjustment-rain-gate-feature-projection/v2",
        Kind::RainGateControl => "adjustment-rain-gate-control-projection/v3",
        Kind::RainFixedGaugeTarget => "adjustment-rain-fixed-gauge-target-projection/v1",
        Kind::TemperatureNativeSource => "adjustment-temperature-native-source-projection/v2",
        Kind::ShadowV1 => "adjustment-shadow-revision-capsule/v1",
        Kind::ShadowV2 => "adjustment-shadow-revision-capsule/v2",
    }
}

/// Map one already validated durable publication without relabelling its identity.
fn publication_value(publication: &NativeRevisionColdPublication) -> JsonValue {
    let (disposition, value) = match publication {
        NativeRevisionColdPublication::Revision(receipt) => {
            ("published", publish_receipt_value(receipt))
        }
        NativeRevisionColdPublication::Batch(receipt) => {
            ("published", batch_publish_receipt_value(receipt))
        }
        NativeRevisionColdPublication::Shadow(receipt) => {
            ("published", shadow_publish_receipt_value(receipt))
        }
        NativeRevisionColdPublication::RevisionGap(receipt) => (
            "committed_unpublished_gap",
            revision_gap_value(
                &receipt.input,
                &receipt.recorded_at,
                Some(&receipt.gap_sha256),
            ),
        ),
        NativeRevisionColdPublication::ShadowGap(receipt) => (
            "committed_unpublished_gap",
            shadow_gap_value(
                &receipt.input,
                &receipt.recorded_at,
                Some(&receipt.gap_sha256),
            ),
        ),
    };
    JsonValue::Object(vec![
        member("disposition", string(disposition)),
        member("value", value),
    ])
}
