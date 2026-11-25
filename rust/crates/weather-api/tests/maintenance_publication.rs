use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_api::{
    AdjustmentMaintenanceAdmissionBoundary, AdjustmentMaintenanceBodyError,
    AdjustmentMaintenanceBodyRead, AdjustmentMaintenanceGapCallback,
    AdjustmentMaintenanceListenerOwner, AdjustmentMaintenanceOperationError,
    AdjustmentMaintenancePublicationRelay, AdjustmentMaintenanceRelayPorts,
    AdjustmentMaintenanceResponseBody, AdjustmentRevisionPublicationPointer,
    AdjustmentRevisionPublishResult, AdmittedAdjustmentMaintenanceAuthority,
    AdmittedAdjustmentMaintenanceCapture, AdmittedAdjustmentMaintenanceGap,
    AdmittedAdjustmentMaintenancePublish, AdmittedAdjustmentMaintenanceStage,
    AdmittedAdjustmentRevisionGap, AdmittedAdjustmentRevisionPublish,
    AdmittedAdjustmentRevisionStage, AdmittedRainFixedGaugeTargetGap,
    AdmittedRainFixedGaugeTargetGapStatus, AdmittedRainMaintenanceControlStateGap,
    AdmittedRainMaintenanceControlStateStage, CanonicalInstant,
    DatabaseAdjustmentMaintenanceRepository, DecodedAdjustmentMaintenancePublish,
    DecodedAdjustmentMaintenanceRequest, DecodedAdjustmentMaintenanceStage,
    DecodedAdjustmentRevisionPublish, DecodedAdjustmentRevisionStage,
    DecodedRainMaintenanceControlStateStage, MAINTENANCE_GAP_PATH, MAINTENANCE_PUBLISH_PATH,
    MAINTENANCE_RAIN_CONTROL_STATE_GAP_PATH, MAINTENANCE_RAIN_CONTROL_STATE_STAGE_PATH,
    MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_PATH,
    MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_PATH, MAINTENANCE_REVISION_GAP_PATH,
    MAINTENANCE_REVISION_PUBLISH_PATH, MAINTENANCE_REVISION_STAGE_PATH,
    MAINTENANCE_SHADOW_STAGE_PATH, PreparedAdjustmentMaintenancePublish,
    PreparedAdjustmentRevisionPublish, UnifiedAdjustmentMaintenanceServicePorts,
    serve_adjustment_maintenance_request_with_repository,
};
use weather_contracts::{
    AdjustmentEcmwfTemperatureRevisionAdmissionKey, AdjustmentForecastAnchorRevisionAdmissionKey,
    AdjustmentMaintenanceCaptureRequest, AdjustmentMaintenanceGapInput,
    AdjustmentMaintenanceGapInputFamilyEnum, AdjustmentMaintenancePublishInput,
    AdjustmentMaintenancePublishReceipt, AdjustmentMaintenancePublishReceiptCommittedLiteral,
    AdjustmentMaintenancePublishReceiptContractVersionEnum, AdjustmentMaintenanceStageInput,
    AdjustmentMaintenanceStageReceipt, AdjustmentMaintenanceStageReceiptContractVersionEnum,
    AdjustmentMaintenanceStageReceiptDurableLiteral, AdjustmentRainGateRevisionAdmissionKey,
    AdjustmentRevisionBatchPublishReceipt, AdjustmentRevisionBatchPublishReceiptCommittedLiteral,
    AdjustmentRevisionBatchPublishReceiptContractVersionLiteral,
    AdjustmentRevisionBatchPublishReceiptProjectionKindEnum, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionGapInput,
    AdjustmentRevisionGapReason, AdjustmentRevisionProjectionKind,
    AdjustmentRevisionPublishReceipt, AdjustmentRevisionPublishReceiptCommittedLiteral,
    AdjustmentRevisionPublishReceiptContractVersionLiteral, AdjustmentRevisionStageReceipt,
    AdjustmentRevisionStageReceiptContractVersionLiteral,
    AdjustmentRevisionStageReceiptDurableLiteral, AdjustmentShadowRegistrationSlot,
    AdjustmentWeatherRevisionAdmissionKey, AdjustmentWeatherRevisionPointerSourceKindEnum,
    FieldPresence, MaintenanceShadowGapReason, MaintenanceShadowPredictionMetadata, Nullable,
    RainFixedGaugeTargetGapInput, RainFixedGaugeTargetGapInputReasonLiteral,
    RainFixedGaugeTargetGapReceipt, RainFixedGaugeTargetGapReceiptContractVersionLiteral,
    RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral, RainFixedGaugeTargetGapStatus,
    RainFixedGaugeTargetGapStatusInput, RainFixedGaugeTargetGapStatusPresent,
    RainFixedGaugeTargetGapStatusPresentContractVersionLiteral,
    RainFixedGaugeTargetGapStatusPresentGap,
    RainFixedGaugeTargetGapStatusPresentGapContractVersionLiteral,
    RainFixedGaugeTargetGapStatusPresentGapQualificationDispositionLiteral,
    RainFixedGaugeTargetGapStatusPresentGapReasonLiteral,
    RainFixedGaugeTargetGapStatusPresentStateLiteral, RainMaintenanceControlStateGapInput,
    RainMaintenanceControlStateGapInputReasonLiteral, RainMaintenanceControlStateStageReceipt,
    RainMaintenanceControlStateStageReceiptContractVersionLiteral,
    RainMaintenanceControlStateStageReceiptDurableLiteral,
};
use weather_database::{
    AdjustmentMaintenanceDatabasePort, AdjustmentRevisionAdmissionKey, AdjustmentRevisionGapMarker,
    AdjustmentShadowRegistrationSlotQueryPlan, MaintenancePublicationBind,
    MaintenancePublicationPlan, adjustment_revision_gap_plan, adjustment_revision_publication_plan,
    adjustment_shadow_publication_plans,
};

const REVISION_BODY: &str = "c0cbfdded030a87a801fef3b2065442156f1c9ad32f780e5ad8a95517b9baf20";
const REVISION_LOGICAL_KEY: &str =
    "b5fbc5805842d91c3eae48a4ebd0421265abd7498cb8b637f1a687cf50515443";

#[derive(Debug, Clone, Copy)]
enum PublicationBodyKind {
    RegularGap,
    ShadowStage,
    ShadowPublish,
    RevisionGap,
    RevisionStage,
    RevisionSingle,
    RevisionBatch,
    RainControlStateStage,
    RainControlStateGap,
    RainFixedGaugeTargetGap,
    RainFixedGaugeTargetGapStatus,
}

struct PublicationBody {
    kind: PublicationBodyKind,
}

impl AdjustmentMaintenanceBodyRead for PublicationBody {
    /// Report the exact bounded fixture body size.
    fn encoded_len(&self) -> usize {
        256
    }

    /// Reject unrelated capture reads in the publication fixture.
    fn capture(
        &mut self,
    ) -> Result<AdjustmentMaintenanceCaptureRequest, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Decode the ordinary categorical gap selected by this fixture.
    fn gap(&mut self) -> Result<AdjustmentMaintenanceGapInput, AdjustmentMaintenanceBodyError> {
        assert!(matches!(self.kind, PublicationBodyKind::RegularGap));
        Ok(AdjustmentMaintenanceGapInput {
            dueKey: "capture/2028-08-26T06:35:00.000Z".to_owned(),
            family: AdjustmentMaintenanceGapInputFamilyEnum::Wind,
            reason: MaintenanceShadowGapReason::CandidateUnavailable,
            registrationSha256: "a".repeat(64),
        })
    }

    /// Decode the ordinary stage bytes selected by this fixture.
    fn shadow_stage(
        &mut self,
    ) -> Result<DecodedAdjustmentMaintenanceStage, AdjustmentMaintenanceBodyError> {
        assert!(matches!(self.kind, PublicationBodyKind::ShadowStage));
        Ok(DecodedAdjustmentMaintenanceStage {
            body: vec![1, 2],
            comparator: FieldPresence::Missing,
            source_projection: vec![3, 4],
        })
    }

    /// Decode the ordinary publish bytes and caller receipts.
    fn shadow_publish(
        &mut self,
    ) -> Result<DecodedAdjustmentMaintenancePublish, AdjustmentMaintenanceBodyError> {
        assert!(matches!(self.kind, PublicationBodyKind::ShadowPublish));
        Ok(DecodedAdjustmentMaintenancePublish {
            body: vec![1, 2],
            comparator: FieldPresence::Missing,
            prediction_committed_at: FieldPresence::Missing,
            revision_receipt: revision_receipt("9"),
            source_projection: vec![3, 4],
            stage_receipt: shadow_stage_receipt(),
        })
    }

    /// Decode the canonical revision stage bytes.
    fn revision_stage(
        &mut self,
    ) -> Result<DecodedAdjustmentRevisionStage, AdjustmentMaintenanceBodyError> {
        assert!(matches!(self.kind, PublicationBodyKind::RevisionStage));
        Ok(DecodedAdjustmentRevisionStage {
            projection: vec![5, 6],
        })
    }

    /// Decode one single or grouped revision publication.
    fn revision_publish(
        &mut self,
    ) -> Result<DecodedAdjustmentRevisionPublish, AdjustmentMaintenanceBodyError> {
        // preserve the selected revision envelope shape without parsing bytes
        match self.kind {
            PublicationBodyKind::RevisionSingle => Ok(DecodedAdjustmentRevisionPublish::Single {
                projection: vec![5, 6],
                revision_receipt: revision_receipt("9"),
                stage_receipt: revision_stage_receipt(),
            }),
            PublicationBodyKind::RevisionBatch => Ok(DecodedAdjustmentRevisionPublish::Batch {
                projection: vec![7, 8],
                revision_receipts: vec![revision_receipt("9"), revision_receipt("10")],
                stage_receipt: revision_stage_receipt(),
            }),
            PublicationBodyKind::RegularGap
            | PublicationBodyKind::ShadowStage
            | PublicationBodyKind::ShadowPublish
            | PublicationBodyKind::RevisionGap
            | PublicationBodyKind::RevisionStage
            | PublicationBodyKind::RainControlStateStage
            | PublicationBodyKind::RainControlStateGap
            | PublicationBodyKind::RainFixedGaugeTargetGap
            | PublicationBodyKind::RainFixedGaugeTargetGapStatus => {
                Err(AdjustmentMaintenanceBodyError)
            }
        }
    }

    /// Decode one value-free revision gap selected by this fixture.
    fn revision_gap(
        &mut self,
    ) -> Result<AdjustmentRevisionGapInput, AdjustmentMaintenanceBodyError> {
        assert!(matches!(self.kind, PublicationBodyKind::RevisionGap));
        Ok(revision_gap())
    }

    /// Decode canonical control-state bytes selected by this fixture.
    fn rain_control_state_stage(
        &mut self,
    ) -> Result<DecodedRainMaintenanceControlStateStage, AdjustmentMaintenanceBodyError> {
        assert!(matches!(
            self.kind,
            PublicationBodyKind::RainControlStateStage
        ));
        Ok(DecodedRainMaintenanceControlStateStage {
            state: vec![11, 12],
        })
    }

    /// Decode one value-free control-state gap selected by this fixture.
    fn rain_control_state_gap(
        &mut self,
    ) -> Result<RainMaintenanceControlStateGapInput, AdjustmentMaintenanceBodyError> {
        assert!(matches!(
            self.kind,
            PublicationBodyKind::RainControlStateGap
        ));
        Ok(rain_control_state_gap())
    }

    /// Decode one fixed-gauge target gap selected by this fixture.
    fn rain_fixed_gauge_target_gap(
        &mut self,
    ) -> Result<RainFixedGaugeTargetGapInput, AdjustmentMaintenanceBodyError> {
        assert!(matches!(
            self.kind,
            PublicationBodyKind::RainFixedGaugeTargetGap
        ));
        Ok(rain_fixed_gauge_target_gap())
    }

    /// Decode one fixed-gauge status key selected by this fixture.
    fn rain_fixed_gauge_target_gap_status(
        &mut self,
    ) -> Result<RainFixedGaugeTargetGapStatusInput, AdjustmentMaintenanceBodyError> {
        assert!(matches!(
            self.kind,
            PublicationBodyKind::RainFixedGaugeTargetGapStatus
        ));
        Ok(rain_fixed_gauge_target_gap_status())
    }
}

struct PublicationAdmission {
    faults: PublicationFaults,
    trace: Rc<RefCell<Vec<&'static str>>>,
}

impl AdjustmentMaintenanceAdmissionBoundary for PublicationAdmission {
    /// Reject unrelated capture admission in the publication fixture.
    fn capture(
        &self,
        _request: AdjustmentMaintenanceCaptureRequest,
    ) -> Result<AdmittedAdjustmentMaintenanceCapture, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Admit one ordinary categorical gap for the unified relay fixture.
    fn gap(
        &self,
        request: AdjustmentMaintenanceGapInput,
    ) -> Result<AdmittedAdjustmentMaintenanceGap, AdjustmentMaintenanceBodyError> {
        Ok(AdmittedAdjustmentMaintenanceGap { request })
    }

    /// Bind decoded ordinary bytes to their prevalidated metadata.
    fn shadow_stage(
        &self,
        capture_epoch: &CanonicalInstant,
        request: DecodedAdjustmentMaintenanceStage,
    ) -> Result<AdmittedAdjustmentMaintenanceStage, AdjustmentMaintenanceBodyError> {
        assert_eq!(capture_epoch.utc, "2000-01-01T00:00:00.000Z");
        Ok(AdmittedAdjustmentMaintenanceStage {
            relay_input: AdjustmentMaintenanceStageInput {
                body: request.body,
                comparator: request.comparator,
                metadata: metadata(),
                sourceProjection: request.source_projection,
                sourceProjectionSha256: "b".repeat(64),
            },
        })
    }

    /// Bind a durable stage acknowledgement to the exact admitted metadata identities.
    fn shadow_stage_receipt(
        &self,
        input: &AdmittedAdjustmentMaintenanceStage,
        receipt: AdjustmentMaintenanceStageReceipt,
    ) -> Result<AdjustmentMaintenanceStageReceipt, AdjustmentMaintenanceBodyError> {
        let metadata = &input.relay_input.metadata;
        // preserve an ordinary archive acknowledgement refusal after its successful write
        if self.faults.acknowledgement_refused
            || receipt.dueKey != metadata.dueKey
            || receipt.predictionBodySha256 != metadata.predictionBodySha256
            || receipt.registrationSha256 != metadata.registrationSha256
            || receipt.sourceProjectionSha256 != input.relay_input.sourceProjectionSha256
        {
            return Err(AdjustmentMaintenanceBodyError);
        }
        Ok(receipt)
    }

    /// Prepare ordinary publication with its caller-supplied receipt.
    fn shadow_publish(
        &self,
        capture_epoch: &CanonicalInstant,
        request: DecodedAdjustmentMaintenancePublish,
    ) -> Result<PreparedAdjustmentMaintenancePublish, AdjustmentMaintenanceBodyError> {
        assert_eq!(capture_epoch.utc, "2000-01-01T00:00:00.000Z");
        Ok(PreparedAdjustmentMaintenancePublish {
            relay_input: AdjustmentMaintenancePublishInput {
                body: request.body,
                comparator: request.comparator,
                metadata: metadata(),
                predictionCommittedAt: request.prediction_committed_at,
                revisionReceipt: request.revision_receipt,
                sourceProjection: request.source_projection,
                sourceProjectionSha256: "b".repeat(64),
                stageReceipt: request.stage_receipt,
            },
        })
    }

    /// Bind a committed acknowledgement to the exact admitted prediction identity.
    fn shadow_publish_receipt(
        &self,
        input: &AdmittedAdjustmentMaintenancePublish,
        receipt: AdjustmentMaintenancePublishReceipt,
    ) -> Result<AdjustmentMaintenancePublishReceipt, AdjustmentMaintenanceBodyError> {
        // preserve an ordinary archive acknowledgement refusal after its successful write
        if self.faults.acknowledgement_refused
            || receipt.predictionSha256 != input.relay_input.metadata.predictionSha256
        {
            return Err(AdjustmentMaintenanceBodyError);
        }
        Ok(receipt)
    }

    /// Admit parsed revision stage bytes after the epoch boundary.
    fn revision_stage(
        &self,
        capture_epoch: &CanonicalInstant,
        request: DecodedAdjustmentRevisionStage,
    ) -> Result<AdmittedAdjustmentRevisionStage, AdjustmentMaintenanceBodyError> {
        assert_eq!(capture_epoch.utc, "2000-01-01T00:00:00.000Z");
        Ok(AdmittedAdjustmentRevisionStage {
            projection: request.projection,
            projection_kind: AdjustmentRevisionProjectionKind::ActualBestMatch,
        })
    }

    /// Bind a durable revision acknowledgement to its admitted kind and body identity.
    fn revision_stage_receipt(
        &self,
        input: &AdmittedAdjustmentRevisionStage,
        receipt: AdjustmentRevisionStageReceipt,
    ) -> Result<AdjustmentRevisionStageReceipt, AdjustmentMaintenanceBodyError> {
        // preserve an ordinary archive acknowledgement refusal after its successful write
        if self.faults.acknowledgement_refused
            || receipt.projectionKind != input.projection_kind
            || receipt.projectionIdentitySha256 != REVISION_BODY
            || receipt.projectionSha256 != REVISION_BODY
        {
            return Err(AdjustmentMaintenanceBodyError);
        }
        Ok(receipt)
    }

    /// Prepare the selected single or grouped revision publication.
    fn revision_publish(
        &self,
        capture_epoch: &CanonicalInstant,
        request: DecodedAdjustmentRevisionPublish,
    ) -> Result<PreparedAdjustmentRevisionPublish, AdjustmentMaintenanceBodyError> {
        assert_eq!(capture_epoch.utc, "2000-01-01T00:00:00.000Z");
        // retain the already admitted closed publication shape
        let pointers = match request {
            DecodedAdjustmentRevisionPublish::Single { .. } => vec![pointer("01")],
            DecodedAdjustmentRevisionPublish::Batch { .. } => vec![pointer("01"), pointer("02")],
        };
        Ok(PreparedAdjustmentRevisionPublish {
            publication: request,
            pointers,
        })
    }

    /// Admit the fixture's already verified canonical receipt clock and self-hash.
    fn verify_revision_publish_receipt(
        &self,
        receipt: &AdjustmentRevisionPublishResult,
    ) -> Result<(), AdjustmentMaintenanceBodyError> {
        let (clock, digest) = match receipt {
            AdjustmentRevisionPublishResult::Single(receipt) => {
                (&receipt.committedAt, &receipt.publishReceiptSha256)
            }
            AdjustmentRevisionPublishResult::Batch(receipt) => {
                (&receipt.committedAt, &receipt.publishReceiptSha256)
            }
        };
        // fixture literals represent the external time and digest verification
        if clock != "2026-09-14T00:40:00.000Z" || digest != &"a".repeat(64) {
            return Err(AdjustmentMaintenanceBodyError);
        }
        Ok(())
    }

    /// Preserve one already validated revision gap.
    fn revision_gap(
        &self,
        request: AdjustmentRevisionGapInput,
    ) -> Result<AdmittedAdjustmentRevisionGap, AdjustmentMaintenanceBodyError> {
        Ok(AdmittedAdjustmentRevisionGap { request })
    }

    /// Bind canonical control-state bytes to the authenticated epoch.
    fn rain_control_state_stage(
        &self,
        capture_epoch: &CanonicalInstant,
        request: DecodedRainMaintenanceControlStateStage,
    ) -> Result<AdmittedRainMaintenanceControlStateStage, AdjustmentMaintenanceBodyError> {
        assert_eq!(capture_epoch.utc, "2000-01-01T00:00:00.000Z");
        Ok(AdmittedRainMaintenanceControlStateStage {
            state: request.state,
        })
    }

    /// Preserve one already validated rain control-state gap.
    fn rain_control_state_gap(
        &self,
        request: RainMaintenanceControlStateGapInput,
    ) -> Result<AdmittedRainMaintenanceControlStateGap, AdjustmentMaintenanceBodyError> {
        Ok(AdmittedRainMaintenanceControlStateGap { request })
    }

    /// Preserve one already validated fixed-gauge target gap.
    fn rain_fixed_gauge_target_gap(
        &self,
        request: RainFixedGaugeTargetGapInput,
    ) -> Result<AdmittedRainFixedGaugeTargetGap, AdjustmentMaintenanceBodyError> {
        Ok(AdmittedRainFixedGaugeTargetGap { request })
    }

    /// Preserve one already validated fixed-gauge status key.
    fn rain_fixed_gauge_target_gap_status(
        &self,
        request: RainFixedGaugeTargetGapStatusInput,
    ) -> Result<AdmittedRainFixedGaugeTargetGapStatus, AdjustmentMaintenanceBodyError> {
        Ok(AdmittedRainFixedGaugeTargetGapStatus { request })
    }

    /// Revalidate the durable fixed-gauge receipt against its admitted request.
    fn rain_fixed_gauge_target_gap_receipt(
        &self,
        request: &AdmittedRainFixedGaugeTargetGap,
        receipt: RainFixedGaugeTargetGapReceipt,
    ) -> Result<RainFixedGaugeTargetGapReceipt, AdjustmentMaintenanceBodyError> {
        assert_eq!(receipt.logicalHourAt, request.request.logicalHourAt);
        assert_eq!(receipt.logicalKeySha256, request.request.logicalKeySha256);
        self.trace
            .borrow_mut()
            .push("rain-fixed-gauge-target-gap-outcome");
        Ok(receipt)
    }

    /// Revalidate the fixed-gauge status outcome against its admitted key.
    fn rain_fixed_gauge_target_gap_status_outcome(
        &self,
        request: &AdmittedRainFixedGaugeTargetGapStatus,
        status: RainFixedGaugeTargetGapStatus,
    ) -> Result<RainFixedGaugeTargetGapStatus, AdjustmentMaintenanceBodyError> {
        // require the present result to retain the exact admitted target key
        let RainFixedGaugeTargetGapStatus::Present(present) = &status else {
            return Err(AdjustmentMaintenanceBodyError);
        };
        assert_eq!(present.gap.logicalHourAt, request.request.logicalHourAt);
        assert_eq!(
            present.gap.logicalKeySha256,
            request.request.logicalKeySha256
        );
        self.trace
            .borrow_mut()
            .push("rain-fixed-gauge-target-gap-status-outcome");
        Ok(status)
    }
}

impl AdjustmentMaintenanceGapCallback for PublicationRelay {
    /// Retain an ordinary categorical gap through the shared relay owner.
    fn record_gap(
        &mut self,
        request: AdmittedAdjustmentMaintenanceGap,
    ) -> Result<(), AdjustmentMaintenanceOperationError> {
        assert_eq!(
            request.request.family,
            AdjustmentMaintenanceGapInputFamilyEnum::Wind
        );
        assert_eq!(
            request.request.reason,
            MaintenanceShadowGapReason::CandidateUnavailable
        );
        self.trace.borrow_mut().push("shadow-gap");
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
struct PublicationFaults {
    acknowledgement_refused: bool,
    shadow_uncommitted: bool,
    revision_read_failed: bool,
    revision_publish_failed: bool,
    marker_failed: bool,
}

struct PublicationDatabase {
    faults: PublicationFaults,
    trace: Rc<RefCell<Vec<&'static str>>>,
}

impl AdjustmentMaintenanceDatabasePort for PublicationDatabase {
    type Error = ();

    /// Reject schedule reads outside the publication fixture.
    fn read_schedule_slot(
        &mut self,
        _plan: AdjustmentShadowRegistrationSlotQueryPlan,
    ) -> Result<Vec<AdjustmentShadowRegistrationSlot>, Self::Error> {
        Err(())
    }

    /// Observe compact body admission before the receipt read.
    fn read_shadow_body_admission(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<bool>, Self::Error> {
        assert_eq!(
            plan.binds,
            [
                MaintenancePublicationBind::Text(&"f".repeat(64)),
                MaintenancePublicationBind::Text("capture/2026-09-14T00:35:00.000Z"),
                MaintenancePublicationBind::Text(&"c".repeat(64)),
                MaintenancePublicationBind::Integer(2),
            ],
        );
        self.trace.borrow_mut().push("shadow-admit");
        Ok(vec![!self.faults.shadow_uncommitted])
    }

    /// Return the exact committed shadow receipt through the second query.
    fn read_shadow_revision_receipt(
        &mut self,
        _plan: MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
        self.trace.borrow_mut().push("shadow-revision-admit");
        Ok(vec![revision_receipt("9")])
    }

    /// Return one distinct receipt for each admitted row in document order.
    fn read_current_revision_receipt(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 1>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
        self.trace.borrow_mut().push("revision-admit");
        // preserve an ordinary unavailable database read
        if self.faults.revision_read_failed {
            return Err(());
        }
        let [
            MaintenancePublicationBind::RevisionKeyJson(
                AdjustmentRevisionAdmissionKey::WeatherRecord(key),
            ),
        ] = plan.binds
        else {
            panic!("expected exact weather key");
        };
        assert_eq!(key.sourceId, "7");
        let ordinal = match key.validAt.as_str() {
            "2026-09-14T01:00:00.000Z" => "9",
            "2026-09-14T02:00:00.000Z" => "10",
            _ => panic!("unexpected row order or identity"),
        };
        Ok(vec![revision_receipt(ordinal)])
    }

    /// Match the closed typed marker inputs and preserve optional write failure.
    fn write_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
        let [
            MaintenancePublicationBind::Text("weather_record"),
            MaintenancePublicationBind::RevisionKeyJson(
                AdjustmentRevisionAdmissionKey::WeatherRecord(key),
            ),
            MaintenancePublicationBind::GapJson(gap),
        ] = plan.binds
        else {
            panic!("expected exact gap bind order");
        };
        assert_eq!(
            gap.projectionIdentitySha256,
            Nullable::Value(REVISION_BODY.to_owned())
        );
        assert_eq!(gap.projectionIdentitySha256, gap.projectionSha256);
        assert_eq!(
            gap.projectionKind,
            AdjustmentRevisionProjectionKind::ActualBestMatch
        );
        self.trace.borrow_mut().push(match gap.reason {
            AdjustmentRevisionGapReason::DatabaseAdmissionFailed => {
                "revision-mark:database_admission_failed"
            }
            AdjustmentRevisionGapReason::ArchivePublishFailed => {
                "revision-mark:archive_publish_failed"
            }
            _ => panic!("unexpected publication gap reason"),
        });
        // simulate a temporary marker write failure
        if self.faults.marker_failed {
            return Err(());
        }
        Ok(vec![AdjustmentRevisionGapMarker {
            logical_key_sha256: gap.logicalKeySha256.clone(),
            projection_identity_sha256: gap.projectionIdentitySha256.clone(),
            projection_kind: gap.projectionKind,
            projection_sha256: gap.projectionSha256.clone(),
            reason: gap.reason,
            stored_content_sha256: key.storedContentSha256.clone(),
        }])
    }
}

struct PublicationRelay {
    faults: PublicationFaults,
    trace: Rc<RefCell<Vec<&'static str>>>,
}

impl AdjustmentMaintenancePublicationRelay for PublicationRelay {
    /// Persist ordinary bytes without consulting the database port.
    fn stage_shadow(
        &mut self,
        input: &AdmittedAdjustmentMaintenanceStage,
    ) -> Result<AdjustmentMaintenanceStageReceipt, AdjustmentMaintenanceOperationError> {
        assert_eq!(input.relay_input.body, [1, 2]);
        self.trace.borrow_mut().push("shadow-stage");
        Ok(shadow_stage_receipt())
    }

    /// Publish ordinary bytes after the repository has rebound the receipt.
    fn publish_shadow(
        &mut self,
        input: &AdmittedAdjustmentMaintenancePublish,
    ) -> Result<AdjustmentMaintenancePublishReceipt, AdjustmentMaintenanceOperationError> {
        assert_eq!(input.relay_input.revisionReceipt.archiveCommitOrdinal, "9");
        self.trace.borrow_mut().push("shadow-publish");
        Ok(shadow_publish_receipt())
    }

    /// Persist revision bytes without consulting the database port.
    fn stage_revision(
        &mut self,
        input: &AdmittedAdjustmentRevisionStage,
    ) -> Result<AdjustmentRevisionStageReceipt, AdjustmentMaintenanceOperationError> {
        assert_eq!(input.projection, [5, 6]);
        assert_eq!(
            input.projection_kind,
            AdjustmentRevisionProjectionKind::ActualBestMatch
        );
        self.trace.borrow_mut().push("revision-stage");
        Ok(revision_stage_receipt())
    }

    /// Publish one admitted single or grouped revision.
    fn publish_revision(
        &mut self,
        input: &AdmittedAdjustmentRevisionPublish,
    ) -> Result<AdjustmentRevisionPublishResult, AdjustmentMaintenanceOperationError> {
        self.trace.borrow_mut().push("revision-publish");
        // preserve the ordinary archive failure after persisted admission
        if self.faults.revision_publish_failed {
            return Err(AdjustmentMaintenanceOperationError::Rejected);
        }
        // return the matching generated receipt shape
        Ok(match input {
            AdmittedAdjustmentRevisionPublish::Single {
                revision_receipt, ..
            } => {
                AdjustmentRevisionPublishResult::Single(revision_publish_receipt(revision_receipt))
            }
            AdmittedAdjustmentRevisionPublish::Batch {
                revision_receipts, ..
            } => AdjustmentRevisionPublishResult::Batch(revision_batch_publish_receipt(
                revision_receipts,
            )),
        })
    }

    /// Retain one permanent revision gap without projection bytes.
    fn record_revision_gap(
        &mut self,
        input: AdmittedAdjustmentRevisionGap,
    ) -> Result<(), AdjustmentMaintenanceOperationError> {
        let (trace, identity) = match input.request.reason {
            AdjustmentRevisionGapReason::ArchiveStageFailed => ("revision-gap", "1".repeat(64)),
            AdjustmentRevisionGapReason::DatabaseAdmissionFailed => (
                "revision-archive-gap:database_admission_failed",
                REVISION_LOGICAL_KEY.to_owned(),
            ),
            AdjustmentRevisionGapReason::ArchivePublishFailed => (
                "revision-archive-gap:archive_publish_failed",
                REVISION_LOGICAL_KEY.to_owned(),
            ),
            _ => panic!("unexpected cold gap"),
        };
        assert_eq!(input.request.logicalKeySha256, identity);
        self.trace.borrow_mut().push(trace);
        Ok(())
    }

    /// Persist one epoch-admitted rain control state.
    fn stage_rain_control_state(
        &mut self,
        input: AdmittedRainMaintenanceControlStateStage,
    ) -> Result<RainMaintenanceControlStateStageReceipt, AdjustmentMaintenanceOperationError> {
        assert_eq!(input.state, [11, 12]);
        self.trace.borrow_mut().push("rain-control-state-stage");
        Ok(rain_control_state_stage_receipt())
    }

    /// Retain one permanent rain control-state staging gap.
    fn record_rain_control_state_gap(
        &mut self,
        input: AdmittedRainMaintenanceControlStateGap,
    ) -> Result<(), AdjustmentMaintenanceOperationError> {
        assert_eq!(input.request.stateSha256, "2".repeat(64));
        self.trace.borrow_mut().push("rain-control-state-gap");
        Ok(())
    }

    /// Retain one fixed-gauge target gap and return its durable receipt.
    fn record_rain_fixed_gauge_target_gap(
        &mut self,
        input: &AdmittedRainFixedGaugeTargetGap,
    ) -> Result<RainFixedGaugeTargetGapReceipt, AdjustmentMaintenanceOperationError> {
        assert_eq!(input.request.logicalKeySha256, "4".repeat(64));
        self.trace.borrow_mut().push("rain-fixed-gauge-target-gap");
        Ok(rain_fixed_gauge_target_gap_receipt())
    }

    /// Return the permanent fixed-gauge disposition for one exact key.
    fn read_rain_fixed_gauge_target_gap(
        &mut self,
        input: &AdmittedRainFixedGaugeTargetGapStatus,
    ) -> Result<RainFixedGaugeTargetGapStatus, AdjustmentMaintenanceOperationError> {
        assert_eq!(input.request.logicalKeySha256, "4".repeat(64));
        self.trace
            .borrow_mut()
            .push("rain-fixed-gauge-target-gap-status");
        Ok(rain_fixed_gauge_target_gap_status_present())
    }
}

/// Invoke one admitted private publication request with a shared event trace.
fn invoke(
    kind: PublicationBodyKind,
    pathname: &str,
    trace: Rc<RefCell<Vec<&'static str>>>,
) -> weather_api::AdjustmentMaintenanceResponse {
    invoke_with_faults(kind, pathname, trace, PublicationFaults::default())
}

/// Run the production repository through one ordinary dependency failure.
fn invoke_with_faults(
    kind: PublicationBodyKind,
    pathname: &str,
    trace: Rc<RefCell<Vec<&'static str>>>,
    faults: PublicationFaults,
) -> weather_api::AdjustmentMaintenanceResponse {
    let mut repository = DatabaseAdjustmentMaintenanceRepository::new(PublicationDatabase {
        faults,
        trace: Rc::clone(&trace),
    });
    let mut relay = PublicationRelay {
        faults,
        trace: Rc::clone(&trace),
    };
    invoke_with_unified_relay(kind, pathname, trace, faults, &mut repository, &mut relay)
}

/// Invoke one request through a retained relay that owns both private operations.
fn invoke_with_unified_relay(
    kind: PublicationBodyKind,
    pathname: &str,
    trace: Rc<RefCell<Vec<&'static str>>>,
    faults: PublicationFaults,
    repository: &mut DatabaseAdjustmentMaintenanceRepository<PublicationDatabase>,
    relay: &mut PublicationRelay,
) -> weather_api::AdjustmentMaintenanceResponse {
    let mut body = PublicationBody { kind };
    let admission = PublicationAdmission { faults, trace };
    serve_adjustment_maintenance_request_with_repository(
        AdmittedAdjustmentMaintenanceAuthority::admitted(
            AdjustmentMaintenanceListenerOwner::AdjustmentMaintenanceApplication,
        ),
        repository,
        &mut body,
        UnifiedAdjustmentMaintenanceServicePorts {
            capture_epoch: CanonicalInstant {
                epoch_milliseconds: 946_684_800_000,
                utc: "2000-01-01T00:00:00.000Z".to_owned(),
            },
            admission: &admission,
            capture: None,
            relay: AdjustmentMaintenanceRelayPorts::Unified(relay),
        },
        DecodedAdjustmentMaintenanceRequest {
            method: "POST",
            pathname,
        },
    )
}

#[test]
// retain one relay owner across ordinary gap and publication operations
fn unified_relay_owner_records_gap_then_stages_publication() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let faults = PublicationFaults::default();
    let mut repository = DatabaseAdjustmentMaintenanceRepository::new(PublicationDatabase {
        faults,
        trace: Rc::clone(&trace),
    });
    let mut relay = PublicationRelay {
        faults,
        trace: Rc::clone(&trace),
    };

    let gap = invoke_with_unified_relay(
        PublicationBodyKind::RegularGap,
        MAINTENANCE_GAP_PATH,
        Rc::clone(&trace),
        faults,
        &mut repository,
        &mut relay,
    );
    assert_eq!(gap.status, 200);
    assert_eq!(gap.body, AdjustmentMaintenanceResponseBody::Recorded);

    let stage = invoke_with_unified_relay(
        PublicationBodyKind::ShadowStage,
        MAINTENANCE_SHADOW_STAGE_PATH,
        Rc::clone(&trace),
        faults,
        &mut repository,
        &mut relay,
    );
    assert_eq!(stage.status, 200);
    assert!(matches!(
        stage.body,
        AdjustmentMaintenanceResponseBody::ShadowStage(_)
    ));
    assert_eq!(*trace.borrow(), ["shadow-gap", "shadow-stage"]);
}

#[test]
// preserve archive stage and postcommit database-before-publication ordering
fn stages_and_publishes_ordinary_and_revision_material_in_source_order() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let shadow_stage = invoke(
        PublicationBodyKind::ShadowStage,
        MAINTENANCE_SHADOW_STAGE_PATH,
        Rc::clone(&trace),
    );
    assert_eq!(shadow_stage.status, 200);
    assert!(matches!(
        shadow_stage.body,
        AdjustmentMaintenanceResponseBody::ShadowStage(_)
    ));
    assert_eq!(*trace.borrow(), ["shadow-stage"]);

    trace.borrow_mut().clear();
    let shadow_publish = invoke(
        PublicationBodyKind::ShadowPublish,
        MAINTENANCE_PUBLISH_PATH,
        Rc::clone(&trace),
    );
    assert_eq!(shadow_publish.status, 200);
    assert!(matches!(
        shadow_publish.body,
        AdjustmentMaintenanceResponseBody::ShadowPublish(_)
    ));
    assert_eq!(
        *trace.borrow(),
        ["shadow-admit", "shadow-revision-admit", "shadow-publish"]
    );

    trace.borrow_mut().clear();
    let revision_stage = invoke(
        PublicationBodyKind::RevisionStage,
        MAINTENANCE_REVISION_STAGE_PATH,
        Rc::clone(&trace),
    );
    assert_eq!(revision_stage.status, 200);
    assert!(matches!(
        revision_stage.body,
        AdjustmentMaintenanceResponseBody::RevisionStage(_)
    ));
    assert_eq!(*trace.borrow(), ["revision-stage"]);

    // prove both single and grouped revision receipts retain the same admission order
    for (kind, reads) in [
        (PublicationBodyKind::RevisionSingle, 1),
        (PublicationBodyKind::RevisionBatch, 2),
    ] {
        trace.borrow_mut().clear();
        let response = invoke(kind, MAINTENANCE_REVISION_PUBLISH_PATH, Rc::clone(&trace));
        assert_eq!(response.status, 200);
        assert!(matches!(
            response.body,
            AdjustmentMaintenanceResponseBody::RevisionPublish(_)
        ));
        let mut expected = vec!["revision-admit"; reads];
        expected.push("revision-publish");
        assert_eq!(*trace.borrow(), expected);
    }
}

#[test]
// refuse an ordinary successful archive write whose returned acknowledgement is not admitted
fn rejects_unadmitted_archive_acknowledgement_after_relay_success() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let response = invoke_with_faults(
        PublicationBodyKind::ShadowStage,
        MAINTENANCE_SHADOW_STAGE_PATH,
        Rc::clone(&trace),
        PublicationFaults {
            acknowledgement_refused: true,
            ..PublicationFaults::default()
        },
    );

    assert_eq!(response.status, 409);
    assert_eq!(
        response.body,
        AdjustmentMaintenanceResponseBody::Error(
            weather_api::AdjustmentMaintenanceErrorCode::ShadowRejected
        )
    );
    assert_eq!(*trace.borrow(), ["shadow-stage"]);
}

#[test]
// compare query text and durable error ordering with the real compiled Node service
fn matches_node_publication_facades_and_ordinary_failure_order() {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/maintenance-service-node-oracle.mjs");
    let execution = Command::new("node")
        .arg(script)
        .output()
        .expect("run Node maintenance");
    assert!(
        execution.status.success(),
        "{}",
        String::from_utf8_lossy(&execution.stderr)
    );
    let stdout = String::from_utf8(execution.stdout).expect("Node oracle UTF-8");
    let metadata = metadata();
    let [body, receipt] = adjustment_shadow_publication_plans(&metadata).expect("body plans");
    let pointer = pointer("01");
    let gap = revision_gap();
    // compare each exact source facade including its PostgreSQL casts
    for (label, sql) in [
        ("shadow-body-sql", body.sql),
        ("shadow-receipt-sql", receipt.sql),
        (
            "revision-sql",
            adjustment_revision_publication_plan(&pointer.key).sql,
        ),
        (
            "revision-gap-sql",
            adjustment_revision_gap_plan(&pointer.key, &gap).sql,
        ),
    ] {
        assert_eq!(
            oracle_value(&stdout, label),
            sql.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }
    assert_eq!(oracle_value(&stdout, "revision-body-sha256"), REVISION_BODY);
    assert_eq!(
        oracle_value(&stdout, "revision-logical-sha256"),
        REVISION_LOGICAL_KEY
    );
    // select the exact SQL and single typed JSON bind for the other persisted relations
    for (label, key) in [
        (
            "forecast-anchor-sql",
            AdjustmentRevisionAdmissionKey::ForecastAnchor(
                AdjustmentForecastAnchorRevisionAdmissionKey {
                    leadHours: 24,
                    sourceId: "7".to_owned(),
                    storedContentSha256: "1".repeat(64),
                    validAt: "2026-09-14T01:00:00.000Z".to_owned(),
                },
            ),
        ),
        (
            "rain-gate-sql",
            AdjustmentRevisionAdmissionKey::RainGate(AdjustmentRainGateRevisionAdmissionKey {
                inputSha256: "2".repeat(64),
                modelSha256: "3".repeat(64),
                runInitializedAt: "2026-09-14T00:00:00.000Z".to_owned(),
                storedContentSha256: "1".repeat(64),
            }),
        ),
        (
            "ecmwf-temperature-sql",
            AdjustmentRevisionAdmissionKey::EcmwfTemperature(
                AdjustmentEcmwfTemperatureRevisionAdmissionKey {
                    providerResponseSha256: "4".repeat(64),
                    runInitializedAt: "2026-09-14T00:00:00.000Z".to_owned(),
                    siteId: "7".to_owned(),
                    storedContentSha256: "1".repeat(64),
                },
            ),
        ),
    ] {
        let plan = adjustment_revision_publication_plan(&key);
        assert_eq!(oracle_value(&stdout, label), plan.sql);
        assert_eq!(
            plan.binds,
            [MaintenancePublicationBind::RevisionKeyJson(&key)]
        );
    }

    // compare the ordinary database and archive failure outcomes and exact event order
    for (label, faults) in [
        (
            "revision-read-failure",
            PublicationFaults {
                revision_read_failed: true,
                ..PublicationFaults::default()
            },
        ),
        (
            "revision-publish-failure",
            PublicationFaults {
                revision_publish_failed: true,
                ..PublicationFaults::default()
            },
        ),
    ] {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let failed = invoke_with_faults(
            PublicationBodyKind::RevisionSingle,
            MAINTENANCE_REVISION_PUBLISH_PATH,
            Rc::clone(&trace),
            faults,
        );
        assert_eq!(failed.status, 409);
        assert_eq!(oracle_value(&stdout, label), trace.borrow().join(","));
    }
}

#[test]
// keep a missing compact commit from entering the receipt or archive phases
fn skips_shadow_receipt_read_until_the_compact_body_commits() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let failed = invoke_with_faults(
        PublicationBodyKind::ShadowPublish,
        MAINTENANCE_PUBLISH_PATH,
        Rc::clone(&trace),
        PublicationFaults {
            shadow_uncommitted: true,
            ..PublicationFaults::default()
        },
    );
    assert_eq!(failed.status, 409);
    assert_eq!(*trace.borrow(), ["shadow-admit"]);
}

#[test]
// preserve permanent grouped row marking and the single-row finally path
fn keeps_grouped_and_single_marker_failure_semantics_distinct() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let faults = PublicationFaults {
        revision_publish_failed: true,
        ..PublicationFaults::default()
    };
    let grouped = invoke_with_faults(
        PublicationBodyKind::RevisionBatch,
        MAINTENANCE_REVISION_PUBLISH_PATH,
        Rc::clone(&trace),
        faults,
    );
    assert_eq!(grouped.status, 409);
    assert_eq!(
        *trace.borrow(),
        [
            "revision-admit",
            "revision-admit",
            "revision-publish",
            "revision-mark:archive_publish_failed",
            "revision-mark:archive_publish_failed",
            "revision-archive-gap:archive_publish_failed",
        ]
    );

    // stop grouped markers after the first failed write while the single finally still archives
    for (kind, expected) in [
        (
            PublicationBodyKind::RevisionSingle,
            vec![
                "revision-admit",
                "revision-publish",
                "revision-mark:archive_publish_failed",
                "revision-archive-gap:archive_publish_failed",
            ],
        ),
        (
            PublicationBodyKind::RevisionBatch,
            vec![
                "revision-admit",
                "revision-admit",
                "revision-publish",
                "revision-mark:archive_publish_failed",
            ],
        ),
    ] {
        trace.borrow_mut().clear();
        let failed = invoke_with_faults(
            kind,
            MAINTENANCE_REVISION_PUBLISH_PATH,
            Rc::clone(&trace),
            PublicationFaults {
                marker_failed: true,
                ..faults
            },
        );
        assert_eq!(failed.status, 409);
        assert_eq!(*trace.borrow(), expected);
    }
}

/// Read one exact value-free baseline oracle field.
fn oracle_value<'a>(stdout: &'a str, label: &str) -> &'a str {
    stdout
        .lines()
        .find_map(|line| {
            // split only the first delimiter in the emitted field
            let (key, value) = line.split_once('=')?;
            (key == label).then_some(value)
        })
        .expect("oracle must emit the requested label")
}

#[test]
// preserve every remaining private relay route and its typed response outcome
fn records_revision_and_rain_maintenance_outcomes_without_database_admission() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let revision_gap_response = invoke(
        PublicationBodyKind::RevisionGap,
        MAINTENANCE_REVISION_GAP_PATH,
        Rc::clone(&trace),
    );
    assert_eq!(revision_gap_response.status, 200);
    assert_eq!(
        revision_gap_response.body,
        AdjustmentMaintenanceResponseBody::Recorded
    );
    assert_eq!(*trace.borrow(), ["revision-gap"]);

    trace.borrow_mut().clear();
    let state_stage_response = invoke(
        PublicationBodyKind::RainControlStateStage,
        MAINTENANCE_RAIN_CONTROL_STATE_STAGE_PATH,
        Rc::clone(&trace),
    );
    assert_eq!(state_stage_response.status, 200);
    assert_eq!(
        state_stage_response.body,
        AdjustmentMaintenanceResponseBody::RainControlStateStage(rain_control_state_stage_receipt())
    );
    assert_eq!(*trace.borrow(), ["rain-control-state-stage"]);

    trace.borrow_mut().clear();
    let state_gap_response = invoke(
        PublicationBodyKind::RainControlStateGap,
        MAINTENANCE_RAIN_CONTROL_STATE_GAP_PATH,
        Rc::clone(&trace),
    );
    assert_eq!(state_gap_response.status, 200);
    assert_eq!(
        state_gap_response.body,
        AdjustmentMaintenanceResponseBody::Recorded
    );
    assert_eq!(*trace.borrow(), ["rain-control-state-gap"]);

    trace.borrow_mut().clear();
    let fixed_gap_response = invoke(
        PublicationBodyKind::RainFixedGaugeTargetGap,
        MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_PATH,
        Rc::clone(&trace),
    );
    assert_eq!(fixed_gap_response.status, 200);
    assert_eq!(
        fixed_gap_response.body,
        AdjustmentMaintenanceResponseBody::RainFixedGaugeTargetGap(
            rain_fixed_gauge_target_gap_receipt()
        )
    );
    assert_eq!(
        *trace.borrow(),
        [
            "rain-fixed-gauge-target-gap",
            "rain-fixed-gauge-target-gap-outcome"
        ]
    );

    trace.borrow_mut().clear();
    let fixed_status_response = invoke(
        PublicationBodyKind::RainFixedGaugeTargetGapStatus,
        MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_PATH,
        Rc::clone(&trace),
    );
    assert_eq!(fixed_status_response.status, 200);
    assert_eq!(
        fixed_status_response.body,
        AdjustmentMaintenanceResponseBody::RainFixedGaugeTargetGapStatus(
            rain_fixed_gauge_target_gap_status_present()
        )
    );
    assert_eq!(
        *trace.borrow(),
        [
            "rain-fixed-gauge-target-gap-status",
            "rain-fixed-gauge-target-gap-status-outcome"
        ]
    );
}

/// Build one admitted permanent revision gap without value material.
fn revision_gap() -> AdjustmentRevisionGapInput {
    AdjustmentRevisionGapInput {
        logicalKeySha256: "1".repeat(64),
        projectionIdentitySha256: Nullable::Null,
        projectionKind: AdjustmentRevisionProjectionKind::ActualBestMatch,
        projectionSha256: Nullable::Null,
        reason: AdjustmentRevisionGapReason::ArchiveStageFailed,
    }
}

/// Build one already-admitted fixture pointer with its canonical gap identity.
fn pointer(hour: &str) -> AdjustmentRevisionPublicationPointer {
    // the first row shares the authentic Node fixture; the second is grouped fixture-only
    let logical_key_sha256 = if hour == "01" {
        REVISION_LOGICAL_KEY.to_owned()
    } else {
        "0".repeat(64)
    };
    AdjustmentRevisionPublicationPointer {
        key: AdjustmentRevisionAdmissionKey::WeatherRecord(AdjustmentWeatherRevisionAdmissionKey {
            productRunAt: Nullable::Value("2026-09-14T00:00:00.000Z".to_owned()),
            sourceId: "7".to_owned(),
            sourceKind: AdjustmentWeatherRevisionPointerSourceKindEnum::Forecast,
            storedContentSha256: "c".repeat(64),
            validAt: format!("2026-09-14T{hour}:00:00.000Z"),
        }),
        logical_key_sha256,
    }
}

/// Build one admitted rain control-state staging gap.
fn rain_control_state_gap() -> RainMaintenanceControlStateGapInput {
    RainMaintenanceControlStateGapInput {
        reason: RainMaintenanceControlStateGapInputReasonLiteral,
        stateSha256: "2".repeat(64),
        stageReceiptSha256: "3".repeat(64),
    }
}

/// Build one admitted fixed-gauge target permanent gap.
fn rain_fixed_gauge_target_gap() -> RainFixedGaugeTargetGapInput {
    RainFixedGaugeTargetGapInput {
        logicalHourAt: "2026-09-14T01:00:00.000Z".to_owned(),
        logicalKeySha256: "4".repeat(64),
        reason: RainFixedGaugeTargetGapInputReasonLiteral,
    }
}

/// Build the matching fixed-gauge status lookup key.
fn rain_fixed_gauge_target_gap_status() -> RainFixedGaugeTargetGapStatusInput {
    RainFixedGaugeTargetGapStatusInput {
        logicalHourAt: "2026-09-14T01:00:00.000Z".to_owned(),
        logicalKeySha256: "4".repeat(64),
    }
}

/// Build one durable rain control-state stage receipt.
fn rain_control_state_stage_receipt() -> RainMaintenanceControlStateStageReceipt {
    RainMaintenanceControlStateStageReceipt {
        contractVersion: RainMaintenanceControlStateStageReceiptContractVersionLiteral,
        durable: RainMaintenanceControlStateStageReceiptDurableLiteral,
        durableAt: "2026-09-14T00:36:00.000Z".to_owned(),
        stateSha256: "2".repeat(64),
        stageReceiptSha256: "3".repeat(64),
    }
}

/// Build one durable fixed-gauge target gap receipt.
fn rain_fixed_gauge_target_gap_receipt() -> RainFixedGaugeTargetGapReceipt {
    RainFixedGaugeTargetGapReceipt {
        logicalHourAt: "2026-09-14T01:00:00.000Z".to_owned(),
        logicalKeySha256: "4".repeat(64),
        reason: RainFixedGaugeTargetGapInputReasonLiteral,
        contractVersion: RainFixedGaugeTargetGapReceiptContractVersionLiteral,
        gapAt: "2026-09-14T01:00:01.000Z".to_owned(),
        gapSha256: "5".repeat(64),
        qualificationDisposition: RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral,
    }
}

/// Build one present fixed-gauge target status response.
fn rain_fixed_gauge_target_gap_status_present() -> RainFixedGaugeTargetGapStatus {
    RainFixedGaugeTargetGapStatus::Present(RainFixedGaugeTargetGapStatusPresent {
        contractVersion: RainFixedGaugeTargetGapStatusPresentContractVersionLiteral,
        gap: RainFixedGaugeTargetGapStatusPresentGap {
            contractVersion: RainFixedGaugeTargetGapStatusPresentGapContractVersionLiteral,
            gapAt: "2026-09-14T01:00:01.000Z".to_owned(),
            gapSha256: "5".repeat(64),
            logicalHourAt: "2026-09-14T01:00:00.000Z".to_owned(),
            logicalKeySha256: "4".repeat(64),
            qualificationDisposition:
                RainFixedGaugeTargetGapStatusPresentGapQualificationDispositionLiteral,
            reason: RainFixedGaugeTargetGapStatusPresentGapReasonLiteral,
        },
        state: RainFixedGaugeTargetGapStatusPresentStateLiteral,
    })
}

/// Build the canonical metadata already admitted by the fixture boundary.
fn metadata() -> MaintenanceShadowPredictionMetadata {
    MaintenanceShadowPredictionMetadata {
        bodyByteCount: 2,
        candidateSha256: "a".repeat(64),
        dueKey: "capture/2026-09-14T00:35:00.000Z".to_owned(),
        inputSha256: "b".repeat(64),
        issuedAt: "2026-09-14T00:35:00.000Z".to_owned(),
        maxValidAt: "2026-09-14T12:00:00.000Z".to_owned(),
        minValidAt: "2026-09-14T01:00:00.000Z".to_owned(),
        predictionBodySha256: "c".repeat(64),
        predictionSchemaSha256: "d".repeat(64),
        predictionSha256: "e".repeat(64),
        registrationSha256: "f".repeat(64),
        rowCount: 12,
        sourceReceiptSha256: "1".repeat(64),
        sourceSha256: "2".repeat(64),
    }
}

/// Build one admitted ordinary stage receipt.
fn shadow_stage_receipt() -> AdjustmentMaintenanceStageReceipt {
    AdjustmentMaintenanceStageReceipt {
        comparatorSha256: FieldPresence::Missing,
        contractVersion:
            AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV1,
        durable: AdjustmentMaintenanceStageReceiptDurableLiteral,
        durableAt: "2026-09-14T00:36:00.000Z".to_owned(),
        dueKey: "capture/2026-09-14T00:35:00.000Z".to_owned(),
        predictionBodySha256: "c".repeat(64),
        registrationSha256: "f".repeat(64),
        sourceProjectionSha256: "b".repeat(64),
        stageReceiptSha256: "3".repeat(64),
    }
}

/// Build one admitted ordinary publish receipt.
fn shadow_publish_receipt() -> AdjustmentMaintenancePublishReceipt {
    AdjustmentMaintenancePublishReceipt {
        committed: AdjustmentMaintenancePublishReceiptCommittedLiteral,
        committedAt: "2026-09-14T00:40:00.000Z".to_owned(),
        comparatorSha256: FieldPresence::Missing,
        contractVersion:
            AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV1,
        predictionSha256: "e".repeat(64),
        publishReceiptSha256: "4".repeat(64),
    }
}

/// Build one database revision receipt with a selected global ordinal.
fn revision_receipt(ordinal: &str) -> AdjustmentRevisionCommitReceipt {
    // the second grouped row inherits the exact previous frontier
    let predecessor = if ordinal == "10" { "5" } else { "6" };
    AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: ordinal.to_owned(),
        archiveCommittedAt: "2026-09-14T00:38:00.000Z".to_owned(),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: "5".repeat(64),
        predecessorFrontierSha256: predecessor.repeat(64),
        projectionIdentitySha256: REVISION_BODY.to_owned(),
        projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
        projectionSha256: REVISION_BODY.to_owned(),
        receiptSha256: format!("{ordinal:0>64}"),
        stageReceiptSha256: "9".repeat(64),
    }
}

/// Build one admitted revision stage receipt.
fn revision_stage_receipt() -> AdjustmentRevisionStageReceipt {
    AdjustmentRevisionStageReceipt {
        contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
        durable: AdjustmentRevisionStageReceiptDurableLiteral,
        durableAt: "2026-09-14T00:36:00.000Z".to_owned(),
        projectionIdentitySha256: REVISION_BODY.to_owned(),
        projectionKind: AdjustmentRevisionProjectionKind::ActualBestMatch,
        projectionSha256: REVISION_BODY.to_owned(),
        stageReceiptSha256: "9".repeat(64),
    }
}

/// Build one admitted single revision publish receipt.
fn revision_publish_receipt(
    receipt: &AdjustmentRevisionCommitReceipt,
) -> AdjustmentRevisionPublishReceipt {
    AdjustmentRevisionPublishReceipt {
        committed: AdjustmentRevisionPublishReceiptCommittedLiteral,
        committedAt: "2026-09-14T00:40:00.000Z".to_owned(),
        contractVersion: AdjustmentRevisionPublishReceiptContractVersionLiteral,
        projectionIdentitySha256: receipt.projectionIdentitySha256.clone(),
        projectionKind: AdjustmentRevisionProjectionKind::ActualBestMatch,
        projectionSha256: receipt.projectionSha256.clone(),
        publishReceiptSha256: "a".repeat(64),
        revisionReceiptSha256: receipt.receiptSha256.clone(),
    }
}

/// Build one admitted grouped revision publish receipt.
fn revision_batch_publish_receipt(
    receipts: &[AdjustmentRevisionCommitReceipt],
) -> AdjustmentRevisionBatchPublishReceipt {
    AdjustmentRevisionBatchPublishReceipt {
        committed: AdjustmentRevisionBatchPublishReceiptCommittedLiteral,
        committedAt: "2026-09-14T00:40:00.000Z".to_owned(),
        contractVersion: AdjustmentRevisionBatchPublishReceiptContractVersionLiteral,
        projectionIdentitySha256: receipts[0].projectionIdentitySha256.clone(),
        projectionKind: AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::ActualBestMatch,
        projectionSha256: receipts[0].projectionSha256.clone(),
        publishReceiptSha256: "a".repeat(64),
        // retain authoritative database receipt order in the grouped response
        revisionReceiptSha256s: receipts
            .iter()
            .map(|receipt| receipt.receiptSha256.clone())
            .collect(),
    }
}
