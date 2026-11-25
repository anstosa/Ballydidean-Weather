//! Admit decoded private requests before handing them to retained capture, storage, or relay owners.

use weather_api::{
    AdjustmentMaintenanceAdmissionBoundary, AdjustmentMaintenanceBodyError,
    AdjustmentRevisionPublishResult, AdmittedAdjustmentMaintenanceCapture,
    AdmittedAdjustmentMaintenanceGap, AdmittedAdjustmentMaintenancePublish,
    AdmittedAdjustmentMaintenanceStage, AdmittedAdjustmentRevisionCaptureWitness,
    AdmittedAdjustmentRevisionGap, AdmittedAdjustmentRevisionStage,
    AdmittedRainFixedGaugeTargetGap, AdmittedRainFixedGaugeTargetGapStatus,
    AdmittedRainMaintenanceControlStateGap, AdmittedRainMaintenanceControlStateStage,
    CanonicalInstant, DecodedAdjustmentMaintenancePublish, DecodedAdjustmentMaintenanceStage,
    DecodedAdjustmentRevisionPublish, DecodedAdjustmentRevisionStage,
    DecodedRainMaintenanceControlStateStage, PreparedAdjustmentMaintenancePublish,
    PreparedAdjustmentRevisionPublish,
};
use weather_contracts::{
    AdjustmentMaintenanceCaptureRequest, AdjustmentMaintenanceGapInput,
    AdjustmentMaintenancePublishInput, AdjustmentMaintenancePublishReceipt,
    AdjustmentMaintenanceStageInput, AdjustmentMaintenanceStageReceipt,
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptProjectionKindEnum,
    AdjustmentRevisionGapInput, AdjustmentRevisionStageReceipt, FieldPresence,
    ForecastAdjustmentMaintenanceRuntimePackage, JsonValue, MaintenanceShadowPredictionMetadata,
    RainFixedGaugeTargetGapInput, RainFixedGaugeTargetGapReceipt, RainFixedGaugeTargetGapStatus,
    RainFixedGaugeTargetGapStatusInput, RainHurdleWindPortableArtifact,
    RainMaintenanceControlStateGapInput,
};
use weather_domain::{RainMaintenanceControlPrimitives, admit_rain_maintenance_control_state};

use crate::api_maintenance_revision_admission::{
    admit_api_revision_stage, prepare_api_revision_publish, verify_api_revision_publish_receipt,
};
use crate::api_maintenance_scalar_admission::{
    admit_api_capture, admit_api_fixed_gauge_gap, admit_api_fixed_gauge_outcome,
    admit_api_fixed_gauge_receipt, admit_api_fixed_gauge_status, admit_api_rain_control_gap,
    admit_api_revision_gap, admit_api_shadow_gap,
};
use crate::api_shadow_comparator_authority::admit_shadow_comparator_authority;
use crate::api_shadow_comparator_validation::validate_shadow_comparator_document;
use crate::api_shadow_epoch_projection::require_shadow_source_epoch;
use crate::api_shadow_metadata_projection::shadow_prediction_metadata;
use crate::api_shadow_source_validation::validate_shadow_source;
use crate::api_shadow_wire_validation::{
    SHADOW_BODY_MAX_BYTES, SHADOW_SOURCE_MAX_BYTES, validate_shadow_body,
};
use crate::maintenance_canonical_json::{field, valid_hash};
use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceRevisionEvidenceValues,
    admit_api_revision_stage_receipt, admit_api_shadow_publish_receipt,
    admit_api_shadow_stage_receipt,
};

type BodyError = AdjustmentMaintenanceBodyError;
const SHADOW_COMPARATOR_MAX_BYTES: usize = 512 * 1_024;

/// Exact evidence decoder over the original canonical documents, not the outer HTTP JSON.
pub trait ApiMaintenanceShadowEvidenceValues: ApiMaintenanceReceiptCanonicalValues {
    /// Decode original UTF-8 JSON without changing object order or UTF-16 text.
    fn decode_shadow_json(&self, bytes: &[u8]) -> Option<JsonValue>;

    /// Serialize ordered JSON with JavaScript wire spelling and no trailing LF.
    fn encode_shadow_ordered_json(&self, doc: &JsonValue) -> Option<Vec<u8>>;

    /// Decode canonical padded base64 from original JSON code units.
    fn decode_shadow_comparator_base64(&self, encoded: &[u16]) -> Option<Vec<u8>>;

    /// Serialize recursively sorted JavaScript JSON with exactly one final LF.
    fn encode_shadow_sorted_json(&self, doc: &JsonValue) -> Option<Vec<u8>>;

    /// Decode the original closed generated wind or temperature maintenance package.
    fn decode_shadow_maintenance_package(
        &self,
        doc: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        crate::decode_maintenance_runtime_package(doc)
    }

    /// Decode the original closed generated portable rain model.
    fn decode_shadow_rain_artifact(
        &self,
        doc: &JsonValue,
    ) -> Option<RainHurdleWindPortableArtifact> {
        crate::maintenance_artifact_rain_decode::decode_rain_portable_artifact(doc)
    }

    /// Apply the original runtime-bundle string filter; reject if unavailable.
    fn check_legacy_wind_runtime_value(&self, code_units: &[u16]) -> Option<()>;
}

/// Reuse the API's ordinary wire primitives for the shared monthly state validator.
pub(crate) struct ApiRainControlPrimitives<'a, Values>(pub(crate) &'a Values);

impl<Values: ApiMaintenanceShadowEvidenceValues> RainMaintenanceControlPrimitives
    for ApiRainControlPrimitives<'_, Values>
{
    /// Decode the original JSON document without substituting derived evidence.
    fn decode_control_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.0.decode_shadow_json(bytes)
    }

    /// Serialize the reviewed object order for exact wire equality.
    fn encode_control_ordered_json(&self, doc: &JsonValue) -> Option<Vec<u8>> {
        let mut encoded = self.0.encode_shadow_ordered_json(doc)?;
        encoded.push(b'\n');
        Some(encoded)
    }

    /// Serialize the unsigned state with its original sorted digest preimage.
    fn encode_control_sorted_json(&self, doc: &JsonValue) -> Option<Vec<u8>> {
        self.0.encode_shadow_sorted_json(doc)
    }

    /// Bind the exact canonical bytes through the deployed SHA-256 primitive.
    fn control_sha256_matches(&self, bytes: &[u8], expected: &str) -> bool {
        self.0
            .sha256(bytes)
            .is_some_and(|digest| digest.as_str() == expected)
    }

    /// Read one exact UTC instant through the API's calendar primitive.
    fn control_utc_milliseconds(&self, utc: &str) -> Option<i64> {
        self.0.canonical_utc_epoch_milliseconds(utc)
    }
}

/// Reuse the verified startup witness and admitted codecs on the private dispatch path.
pub struct RetainedApiMaintenanceAdmission<'a, Values> {
    witness: &'a AdmittedAdjustmentRevisionCaptureWitness,
    values: &'a Values,
}

impl<'a, Values> RetainedApiMaintenanceAdmission<'a, Values> {
    /// Bind the request to the listener's actual startup witness.
    pub const fn new(
        witness: &'a AdmittedAdjustmentRevisionCaptureWitness,
        values: &'a Values,
    ) -> Self {
        Self { witness, values }
    }

    /// Refuse composition with a different already admitted epoch.
    fn require_epoch(&self, epoch: &CanonicalInstant) -> Result<(), BodyError> {
        (epoch == &self.witness.capture_epoch)
            .then_some(())
            .ok_or(AdjustmentMaintenanceBodyError)
    }
}

/// Decode one size-bounded shadow JSON member without applying family rules.
fn decode_shadow_json(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    bytes: &[u8],
    cap: usize,
) -> Result<JsonValue, BodyError> {
    // bound parser work by the original absolute cap and require the canonical newline
    if bytes.len() > cap || !bytes.ends_with(b"\n") {
        return Err(AdjustmentMaintenanceBodyError);
    }
    values
        .decode_shadow_json(bytes)
        .ok_or(AdjustmentMaintenanceBodyError)
}

/// Match the exact original compact bytes after Rust has validated each object's order.
fn require_shadow_bytes(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
    bytes: &[u8],
) -> Result<(), BodyError> {
    let encoded = values
        .encode_shadow_ordered_json(doc)
        .ok_or(AdjustmentMaintenanceBodyError)?;
    (bytes.strip_suffix(b"\n") == Some(encoded.as_slice()))
        .then_some(())
        .ok_or(AdjustmentMaintenanceBodyError)
}

impl<Values: ApiMaintenanceShadowEvidenceValues + ApiMaintenanceRevisionEvidenceValues>
    AdjustmentMaintenanceAdmissionBoundary for RetainedApiMaintenanceAdmission<'_, Values>
{
    /// Admit the exact decoded fixed-cycle trigger; the service enforces the witness epoch.
    fn capture(
        &self,
        request: AdjustmentMaintenanceCaptureRequest,
    ) -> Result<AdmittedAdjustmentMaintenanceCapture, BodyError> {
        admit_api_capture(self.values, request)
    }

    /// Admit only the retained categorical shadow gap.
    fn gap(
        &self,
        request: AdjustmentMaintenanceGapInput,
    ) -> Result<AdmittedAdjustmentMaintenanceGap, BodyError> {
        admit_api_shadow_gap(request)
    }

    /// Bind exact source, prediction and optional comparator material to this epoch.
    fn shadow_stage(
        &self,
        epoch: &CanonicalInstant,
        request: DecodedAdjustmentMaintenanceStage,
    ) -> Result<AdmittedAdjustmentMaintenanceStage, BodyError> {
        self.require_epoch(epoch)?;
        let source = decode_shadow_json(
            self.values,
            &request.source_projection,
            SHADOW_SOURCE_MAX_BYTES,
        )?;
        validate_shadow_source(self.values, &source, request.source_projection.len())
            .ok_or(AdjustmentMaintenanceBodyError)?;
        require_shadow_bytes(self.values, &source, &request.source_projection)?;
        require_shadow_source_epoch(self.values, epoch, &source)?;
        let body = decode_shadow_json(self.values, &request.body, SHADOW_BODY_MAX_BYTES)?;
        validate_shadow_body(self.values, &body, request.body.len())
            .ok_or(AdjustmentMaintenanceBodyError)?;
        require_shadow_bytes(self.values, &body, &request.body)?;
        let metadata = shadow_prediction_metadata(
            self.values,
            &source,
            &body,
            &request.source_projection,
            &request.body,
        )
        .ok_or(AdjustmentMaintenanceBodyError)?;
        // release the decoded source before allocating the optional embedded model
        drop(source);
        // bind additive comparator bytes before the request can reach the relay
        if let FieldPresence::Value(comparator) = &request.comparator {
            let doc = decode_shadow_json(self.values, comparator, SHADOW_COMPARATOR_MAX_BYTES)?;
            let family = validate_shadow_comparator_document(&doc, &body, &metadata)
                .ok_or(AdjustmentMaintenanceBodyError)?;
            // the comparator rows no longer need the decoded prediction
            drop(body);
            require_shadow_bytes(self.values, &doc, comparator)?;
            let authority =
                field(&doc, "servingAuthority").ok_or(AdjustmentMaintenanceBodyError)?;
            admit_shadow_comparator_authority(self.values, authority, family)
                .ok_or(AdjustmentMaintenanceBodyError)?;
        }
        Ok(AdmittedAdjustmentMaintenanceStage {
            relay_input: AdjustmentMaintenanceStageInput {
                body: request.body,
                comparator: request.comparator,
                sourceProjectionSha256: metadata.inputSha256.clone(),
                metadata,
                sourceProjection: request.source_projection,
            },
        })
    }

    /// Revalidate the relay's exact durable stage acknowledgement.
    fn shadow_stage_receipt(
        &self,
        input: &AdmittedAdjustmentMaintenanceStage,
        receipt: AdjustmentMaintenanceStageReceipt,
    ) -> Result<AdjustmentMaintenanceStageReceipt, BodyError> {
        admit_api_shadow_stage_receipt(self.values, input, receipt)
    }

    /// Rebind the same staged bytes and supplied receipts before reading a live database pointer.
    fn shadow_publish(
        &self,
        epoch: &CanonicalInstant,
        request: DecodedAdjustmentMaintenancePublish,
    ) -> Result<PreparedAdjustmentMaintenancePublish, BodyError> {
        let DecodedAdjustmentMaintenancePublish {
            body,
            comparator,
            prediction_committed_at,
            revision_receipt,
            source_projection,
            stage_receipt,
        } = request;
        let stage = self.shadow_stage(
            epoch,
            DecodedAdjustmentMaintenanceStage {
                body,
                comparator,
                source_projection,
            },
        )?;
        let stage_receipt = admit_api_shadow_stage_receipt(self.values, &stage, stage_receipt)?;
        let input = stage.relay_input;
        let archived_at = admit_shadow_revision(
            self.values,
            epoch,
            &input.metadata,
            &stage_receipt,
            &revision_receipt,
        )?;
        let issued_at = self
            .values
            .canonical_utc_epoch_milliseconds(&input.metadata.issuedAt)
            .ok_or(AdjustmentMaintenanceBodyError)?;
        // only additive comparator publications carry the original compact commit clock
        match (&input.comparator, &prediction_committed_at) {
            (FieldPresence::Missing, FieldPresence::Missing) => {}
            (FieldPresence::Value(_), FieldPresence::Value(value)) => {
                let clock = self
                    .values
                    .canonical_utc_epoch_milliseconds(value)
                    .ok_or(AdjustmentMaintenanceBodyError)?;
                // preserve the inclusive issue-to-archive commit window
                if !(issued_at..=archived_at).contains(&clock) {
                    return Err(AdjustmentMaintenanceBodyError);
                }
            }
            _ => return Err(AdjustmentMaintenanceBodyError),
        }
        Ok(PreparedAdjustmentMaintenancePublish {
            relay_input: AdjustmentMaintenancePublishInput {
                body: input.body,
                comparator: input.comparator,
                metadata: input.metadata,
                predictionCommittedAt: prediction_committed_at,
                revisionReceipt: revision_receipt,
                sourceProjection: input.sourceProjection,
                sourceProjectionSha256: input.sourceProjectionSha256,
                stageReceipt: stage_receipt,
            },
        })
    }

    /// Observe the exact archive checkpoint only after database admission.
    fn shadow_publish_receipt(
        &self,
        input: &AdmittedAdjustmentMaintenancePublish,
        receipt: AdjustmentMaintenancePublishReceipt,
    ) -> Result<AdjustmentMaintenancePublishReceipt, BodyError> {
        admit_api_shadow_publish_receipt(self.values, input, receipt)
    }

    /// Admit the original transport revision and its entire relevant epoch frontier.
    fn revision_stage(
        &self,
        epoch: &CanonicalInstant,
        request: DecodedAdjustmentRevisionStage,
    ) -> Result<AdmittedAdjustmentRevisionStage, BodyError> {
        self.require_epoch(epoch)?;
        admit_api_revision_stage(self.values, epoch, request)
    }

    /// Rebind revision stage receipt to the exact submitted bytes.
    fn revision_stage_receipt(
        &self,
        input: &AdmittedAdjustmentRevisionStage,
        receipt: AdjustmentRevisionStageReceipt,
    ) -> Result<AdjustmentRevisionStageReceipt, BodyError> {
        admit_api_revision_stage_receipt(self.values, input, receipt)
    }

    /// Prepare one closed single or grouped publication before the database authority check.
    fn revision_publish(
        &self,
        epoch: &CanonicalInstant,
        request: DecodedAdjustmentRevisionPublish,
    ) -> Result<PreparedAdjustmentRevisionPublish, BodyError> {
        self.require_epoch(epoch)?;
        prepare_api_revision_publish(self.values, epoch, request)
    }

    /// Verify the archive's stable revision checkpoint after persisted binding.
    fn verify_revision_publish_receipt(
        &self,
        receipt: &AdjustmentRevisionPublishResult,
    ) -> Result<(), BodyError> {
        verify_api_revision_publish_receipt(self.values, receipt)
    }

    /// Admit the reviewed permanent revision gap without any source data.
    fn revision_gap(
        &self,
        request: AdjustmentRevisionGapInput,
    ) -> Result<AdmittedAdjustmentRevisionGap, BodyError> {
        admit_api_revision_gap(request)
    }

    /// Bind the decoded original monthly rain control to the admitted witness and future clocks.
    fn rain_control_state_stage(
        &self,
        epoch: &CanonicalInstant,
        request: DecodedRainMaintenanceControlStateStage,
    ) -> Result<AdmittedRainMaintenanceControlStateStage, BodyError> {
        self.require_epoch(epoch)?;
        let evidence = admit_rain_maintenance_control_state(
            &ApiRainControlPrimitives(self.values),
            &request.state,
        )
        .ok_or(AdjustmentMaintenanceBodyError)?;
        // both causal training and material generation must be at or after the same witness
        if evidence.epoch_witness_sha256 != self.witness.witness.witnessSha256
            || evidence.training_maximum_epoch_milliseconds < epoch.epoch_milliseconds
            || evidence.generated_epoch_milliseconds < epoch.epoch_milliseconds
        {
            return Err(AdjustmentMaintenanceBodyError);
        }
        Ok(AdmittedRainMaintenanceControlStateStage {
            state: request.state,
        })
    }

    /// Admit an abandoned control state before sending the value-free categorical gap.
    fn rain_control_state_gap(
        &self,
        request: RainMaintenanceControlStateGapInput,
    ) -> Result<AdmittedRainMaintenanceControlStateGap, BodyError> {
        admit_api_rain_control_gap(request)
    }

    /// Admit a fixed-gauge gap's closed hour and logical identity.
    fn rain_fixed_gauge_target_gap(
        &self,
        request: RainFixedGaugeTargetGapInput,
    ) -> Result<AdmittedRainFixedGaugeTargetGap, BodyError> {
        admit_api_fixed_gauge_gap(self.values, request)
    }

    /// Admit the exact lookup key before reading an archived fixed-gauge disposition.
    fn rain_fixed_gauge_target_gap_status(
        &self,
        request: RainFixedGaugeTargetGapStatusInput,
    ) -> Result<AdmittedRainFixedGaugeTargetGapStatus, BodyError> {
        admit_api_fixed_gauge_status(self.values, request)
    }

    /// Rebind the returned archive receipt to the exact requested hour and key.
    fn rain_fixed_gauge_target_gap_receipt(
        &self,
        request: &AdmittedRainFixedGaugeTargetGap,
        receipt: RainFixedGaugeTargetGapReceipt,
    ) -> Result<RainFixedGaugeTargetGapReceipt, BodyError> {
        admit_api_fixed_gauge_receipt(self.values, request, receipt)
    }

    /// Rebind a present disposition, preserving the original absent response.
    fn rain_fixed_gauge_target_gap_status_outcome(
        &self,
        request: &AdmittedRainFixedGaugeTargetGapStatus,
        status: RainFixedGaugeTargetGapStatus,
    ) -> Result<RainFixedGaugeTargetGapStatus, BodyError> {
        admit_api_fixed_gauge_outcome(self.values, request, status)
    }
}

/// Rebind the supplied compact revision receipt before the independent persisted read.
fn admit_shadow_revision(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    epoch: &CanonicalInstant,
    metadata: &MaintenanceShadowPredictionMetadata,
    stage: &AdjustmentMaintenanceStageReceipt,
    receipt: &AdjustmentRevisionCommitReceipt,
) -> Result<i64, BodyError> {
    let committed = values
        .canonical_utc_epoch_milliseconds(&receipt.archiveCommittedAt)
        .ok_or(AdjustmentMaintenanceBodyError)?;
    let ordinal = receipt.archiveCommitOrdinal.as_bytes();
    // preserve the original unbounded positive decimal ordinal and exact database receipt hashes
    if committed < epoch.epoch_milliseconds
        || receipt.projectionKind
            != AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction
        || !matches!(ordinal.first(), Some(b'1'..=b'9'))
        || !ordinal.iter().all(u8::is_ascii_digit)
        || receipt.projectionIdentitySha256 != metadata.sourceReceiptSha256
        || receipt.projectionSha256 != metadata.inputSha256
        || receipt.stageReceiptSha256 != stage.stageReceiptSha256
        || ![
            &receipt.frontierSha256,
            &receipt.predecessorFrontierSha256,
            &receipt.receiptSha256,
        ]
        .into_iter()
        .all(|digest| valid_hash(digest))
    {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(committed)
}
