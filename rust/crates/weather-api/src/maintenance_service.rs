use weather_contracts::{
    AdjustmentMaintenanceCaptureRequest, AdjustmentMaintenanceGapInput,
    AdjustmentMaintenancePublishInput, AdjustmentMaintenancePublishReceipt,
    AdjustmentMaintenanceStageInput, AdjustmentMaintenanceStageReceipt,
    AdjustmentRevisionBatchPublishReceipt, AdjustmentRevisionBatchPublishReceiptProjectionKindEnum,
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionGapInput, AdjustmentRevisionGapReason,
    AdjustmentRevisionProjectionKind, AdjustmentRevisionPublishReceipt,
    AdjustmentRevisionStageReceipt, AdjustmentShadowRegistrationSlot, FieldPresence, Nullable,
    RainFixedGaugeTargetGapInput, RainFixedGaugeTargetGapReceipt, RainFixedGaugeTargetGapStatus,
    RainFixedGaugeTargetGapStatusInput, RainMaintenanceControlStateGapInput,
    RainMaintenanceControlStateStageReceipt,
};
use weather_database::{AdjustmentRevisionAdmissionKey, AdjustmentShadowRegistrationSlotQueryPlan};

use crate::{
    CanonicalInstant, maintenance_schedule_slot_reads, project_maintenance_schedule_slots,
};

pub const MAINTENANCE_CAPTURE_PATH: &str = "/internal/adjustment-maintenance/shadow/capture";
pub const MAINTENANCE_GAP_PATH: &str = "/internal/adjustment-maintenance/shadow/gap";
pub const MAINTENANCE_PUBLISH_PATH: &str = "/internal/adjustment-maintenance/shadow/publish";
pub const MAINTENANCE_SCHEDULE_SLOTS_PATH: &str = "/internal/adjustment-maintenance/schedule-slots";
pub const MAINTENANCE_SHADOW_STAGE_PATH: &str = "/internal/adjustment-maintenance/shadow/stage";
pub const MAINTENANCE_REVISION_PUBLISH_PATH: &str =
    "/internal/adjustment-maintenance/revision/publish";
pub const MAINTENANCE_REVISION_STAGE_PATH: &str = "/internal/adjustment-maintenance/revision/stage";
pub const MAINTENANCE_REVISION_GAP_PATH: &str = "/internal/adjustment-maintenance/revision/gap";
pub const MAINTENANCE_RAIN_CONTROL_STATE_STAGE_PATH: &str =
    "/internal/adjustment-maintenance/rain-control-state/stage";
pub const MAINTENANCE_RAIN_CONTROL_STATE_GAP_PATH: &str =
    "/internal/adjustment-maintenance/rain-control-state/gap";
pub const MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_PATH: &str =
    "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap";
pub const MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_PATH: &str =
    "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status";
pub const MAX_ADJUSTMENT_MAINTENANCE_REQUEST_BYTES: usize = 4_832 * 1_024;

/// The separately owned listener allowed to invoke maintenance dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentMaintenanceListenerOwner {
    AdjustmentMaintenanceApplication,
}

/// Admitted ownership supplied only by the separate maintenance listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedAdjustmentMaintenanceAuthority {
    owner: AdjustmentMaintenanceListenerOwner,
}

impl AdmittedAdjustmentMaintenanceAuthority {
    /// Bind one already separated listener owner to maintenance dispatch.
    pub const fn admitted(owner: AdjustmentMaintenanceListenerOwner) -> Self {
        Self { owner }
    }
}

/// One decoded private request before JSON and HTTP response adaptation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedAdjustmentMaintenanceRequest<'a> {
    pub method: &'a str,
    /// Exact origin-form request target; do not drop a query or normalize its path.
    pub pathname: &'a str,
}

/// Transport work admitted before any private request body is accessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentMaintenanceRequestClass {
    Schedule,
    Post,
    NotFound,
}

/// Classify one exact private request through the service's single route allowlist.
pub fn classify_adjustment_maintenance_request(
    request: DecodedAdjustmentMaintenanceRequest<'_>,
) -> AdjustmentMaintenanceRequestClass {
    // preserve the one body-free schedule operation before the POST allowlist
    if request.method == "GET" && request.pathname == MAINTENANCE_SCHEDULE_SLOTS_PATH {
        return AdjustmentMaintenanceRequestClass::Schedule;
    }
    // admit no headers, query aliases, or near matches around the fixed POST routes
    if request.method == "POST"
        && matches!(
            request.pathname,
            MAINTENANCE_CAPTURE_PATH
                | MAINTENANCE_GAP_PATH
                | MAINTENANCE_SHADOW_STAGE_PATH
                | MAINTENANCE_PUBLISH_PATH
                | MAINTENANCE_REVISION_GAP_PATH
                | MAINTENANCE_REVISION_STAGE_PATH
                | MAINTENANCE_RAIN_CONTROL_STATE_STAGE_PATH
                | MAINTENANCE_RAIN_CONTROL_STATE_GAP_PATH
                | MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_PATH
                | MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_PATH
                | MAINTENANCE_REVISION_PUBLISH_PATH
        )
    {
        return AdjustmentMaintenanceRequestClass::Post;
    }
    AdjustmentMaintenanceRequestClass::NotFound
}

/// One fixed scheduler cycle admitted by the shared time boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentMaintenanceCaptureCycle {
    Hour00,
    Hour06,
    Hour12,
    Hour18,
}

/// One decoded capture request after due-key and UTC admission.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedAdjustmentMaintenanceCapture {
    pub request: AdjustmentMaintenanceCaptureRequest,
    pub issued_at: CanonicalInstant,
    pub cycle: AdjustmentMaintenanceCaptureCycle,
}

/// One decoded value-free gap after key and digest admission.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedAdjustmentMaintenanceGap {
    pub request: AdjustmentMaintenanceGapInput,
}

/// Decoded ordinary stage bytes before hash and source admission.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAdjustmentMaintenanceStage {
    pub body: Vec<u8>,
    pub comparator: FieldPresence<Vec<u8>>,
    pub source_projection: Vec<u8>,
}

/// Decoded ordinary publish body before persisted-pointer admission.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAdjustmentMaintenancePublish {
    pub body: Vec<u8>,
    pub comparator: FieldPresence<Vec<u8>>,
    pub prediction_committed_at: FieldPresence<String>,
    pub revision_receipt: AdjustmentRevisionCommitReceipt,
    pub source_projection: Vec<u8>,
    pub stage_receipt: AdjustmentMaintenanceStageReceipt,
}

/// Decoded revision bytes before transport-document and epoch admission.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAdjustmentRevisionStage {
    pub projection: Vec<u8>,
}

/// Decoded single or grouped revision publication envelope.
#[derive(Debug, Clone, PartialEq)]
pub enum DecodedAdjustmentRevisionPublish {
    Single {
        projection: Vec<u8>,
        revision_receipt: AdjustmentRevisionCommitReceipt,
        stage_receipt: AdjustmentRevisionStageReceipt,
    },
    Batch {
        projection: Vec<u8>,
        revision_receipts: Vec<AdjustmentRevisionCommitReceipt>,
        stage_receipt: AdjustmentRevisionStageReceipt,
    },
}

/// Hash-, identity-, source-, and epoch-admitted ordinary stage input.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedAdjustmentMaintenanceStage {
    pub relay_input: AdjustmentMaintenanceStageInput,
}

/// Prepared ordinary publication before live database pointer admission.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedAdjustmentMaintenancePublish {
    pub relay_input: AdjustmentMaintenancePublishInput,
}

/// Ordinary publication rebound to the live persisted database receipt.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedAdjustmentMaintenancePublish {
    pub relay_input: AdjustmentMaintenancePublishInput,
}

/// Transport-document- and epoch-admitted revision stage bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedAdjustmentRevisionStage {
    pub projection: Vec<u8>,
    pub projection_kind: AdjustmentRevisionProjectionKind,
}

/// One decoded permanent revision gap after identity admission.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedAdjustmentRevisionGap {
    pub request: AdjustmentRevisionGapInput,
}

/// Decoded rain control-state bytes before document and epoch admission.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedRainMaintenanceControlStateStage {
    pub state: Vec<u8>,
}

/// Canonical future-only rain control-state bytes admitted for staging.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainMaintenanceControlStateStage {
    pub state: Vec<u8>,
}

/// One decoded control-state gap after closed-field admission.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainMaintenanceControlStateGap {
    pub request: RainMaintenanceControlStateGapInput,
}

/// One decoded fixed-gauge target gap after key and hour admission.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainFixedGaugeTargetGap {
    pub request: RainFixedGaugeTargetGapInput,
}

/// One decoded fixed-gauge target lookup after key and hour admission.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainFixedGaugeTargetGapStatus {
    pub request: RainFixedGaugeTargetGapStatusInput,
}

/// Prepared revision publication before live database pointer admission.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedAdjustmentRevisionPublish {
    pub publication: DecodedAdjustmentRevisionPublish,
    pub pointers: Vec<AdjustmentRevisionPublicationPointer>,
}

/// One ordered row pointer and the digest of its admitted value-free gap key.
#[derive(Debug, Clone, PartialEq)]
pub struct AdjustmentRevisionPublicationPointer {
    pub key: AdjustmentRevisionAdmissionKey,
    pub logical_key_sha256: String,
}

impl PreparedAdjustmentRevisionPublish {
    /// Pair current-pointer keys with exactly one caller receipt per ordered row.
    pub fn rows(
        &self,
    ) -> (
        &[AdjustmentRevisionPublicationPointer],
        &[AdjustmentRevisionCommitReceipt],
    ) {
        let receipts = match &self.publication {
            DecodedAdjustmentRevisionPublish::Single {
                revision_receipt, ..
            } => std::slice::from_ref(revision_receipt),
            DecodedAdjustmentRevisionPublish::Batch {
                revision_receipts, ..
            } => revision_receipts.as_slice(),
        };
        (&self.pointers, receipts)
    }

    /// Borrow the canonical projection identity from the validated stage receipt.
    pub(crate) fn stage_receipt(&self) -> &AdjustmentRevisionStageReceipt {
        match &self.publication {
            DecodedAdjustmentRevisionPublish::Single { stage_receipt, .. }
            | DecodedAdjustmentRevisionPublish::Batch { stage_receipt, .. } => stage_receipt,
        }
    }

    /// Move the exact receipt and body identity into the archive publication.
    fn into_admitted(
        self,
    ) -> (
        AdmittedAdjustmentRevisionPublish,
        Vec<AdjustmentRevisionPublicationPointer>,
    ) {
        let admitted = match self.publication {
            DecodedAdjustmentRevisionPublish::Single {
                projection,
                revision_receipt,
                stage_receipt,
            } => AdmittedAdjustmentRevisionPublish::Single {
                projection,
                revision_receipt,
                stage_receipt,
            },
            DecodedAdjustmentRevisionPublish::Batch {
                projection,
                revision_receipts,
                stage_receipt,
            } => AdmittedAdjustmentRevisionPublish::Batch {
                projection,
                revision_receipts,
                stage_receipt,
            },
        };
        (admitted, self.pointers)
    }
}

/// Revision publication rebound to authoritative persisted receipt rows.
#[derive(Debug, Clone, PartialEq)]
pub enum AdmittedAdjustmentRevisionPublish {
    Single {
        projection: Vec<u8>,
        revision_receipt: AdjustmentRevisionCommitReceipt,
        stage_receipt: AdjustmentRevisionStageReceipt,
    },
    Batch {
        projection: Vec<u8>,
        revision_receipts: Vec<AdjustmentRevisionCommitReceipt>,
        stage_receipt: AdjustmentRevisionStageReceipt,
    },
}

impl AdmittedAdjustmentRevisionPublish {
    /// Borrow the stage identity after moving the payload to the archive call.
    fn stage_receipt(&self) -> &AdjustmentRevisionStageReceipt {
        match self {
            Self::Single { stage_receipt, .. } | Self::Batch { stage_receipt, .. } => stage_receipt,
        }
    }

    /// Bind the archive's returned checkpoint to the exact persisted body and receipt rows.
    fn matches_publish_result(&self, result: &AdjustmentRevisionPublishResult) -> bool {
        match (self, result) {
            (
                Self::Single {
                    revision_receipt,
                    stage_receipt,
                    ..
                },
                AdjustmentRevisionPublishResult::Single(result),
            ) => {
                result.projectionKind == stage_receipt.projectionKind
                    && result.projectionIdentitySha256 == revision_receipt.projectionIdentitySha256
                    && result.projectionSha256 == revision_receipt.projectionSha256
                    && result.revisionReceiptSha256 == revision_receipt.receiptSha256
            }
            (
                Self::Batch {
                    revision_receipts,
                    stage_receipt,
                    ..
                },
                AdjustmentRevisionPublishResult::Batch(result),
            ) => {
                let kind_matches =
                    matches!(
                    (result.projectionKind, stage_receipt.projectionKind),
                    (AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::ActualBestMatch,
                        AdjustmentRevisionProjectionKind::ActualBestMatch)
                    | (AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::TargetRevision,
                        AdjustmentRevisionProjectionKind::TargetRevision)
                );
                kind_matches
                    && result.projectionIdentitySha256 == stage_receipt.projectionIdentitySha256
                    && result.projectionSha256 == stage_receipt.projectionSha256
                    && result.revisionReceiptSha256s.len() == revision_receipts.len()
                    && result
                        .revisionReceiptSha256s
                        .iter()
                        .zip(revision_receipts)
                        .all(
                            // preserve each ordered server-assigned receipt identity
                            |(expected, row)| *expected == row.receiptSha256,
                        )
            }
            _ => false,
        }
    }
}

/// Bounded body-read or body-admission failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdjustmentMaintenanceBodyError;

/// Bounded repository, archive, or callback failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentMaintenanceOperationError {
    Rejected,
    RevisionSpoolFull,
}

/// Decoded typed request bodies supplied by the transport adapter.
pub trait AdjustmentMaintenanceBodyRead {
    /// Report the exact encoded body bytes observed before decoding.
    fn encoded_len(&self) -> usize;

    /// Read the generated capture DTO without interpreting its time strings.
    fn capture(
        &mut self,
    ) -> Result<AdjustmentMaintenanceCaptureRequest, AdjustmentMaintenanceBodyError>;

    /// Read the generated closed gap DTO without interpreting its key strings.
    fn gap(&mut self) -> Result<AdjustmentMaintenanceGapInput, AdjustmentMaintenanceBodyError>;

    /// Decode ordinary shadow stage bytes without parsing their contracts.
    fn shadow_stage(
        &mut self,
    ) -> Result<DecodedAdjustmentMaintenanceStage, AdjustmentMaintenanceBodyError>;

    /// Decode ordinary shadow publish bytes and typed receipts.
    fn shadow_publish(
        &mut self,
    ) -> Result<DecodedAdjustmentMaintenancePublish, AdjustmentMaintenanceBodyError>;

    /// Decode revision stage bytes without parsing their document.
    fn revision_stage(
        &mut self,
    ) -> Result<DecodedAdjustmentRevisionStage, AdjustmentMaintenanceBodyError>;

    /// Decode one exact single or grouped revision publish envelope.
    fn revision_publish(
        &mut self,
    ) -> Result<DecodedAdjustmentRevisionPublish, AdjustmentMaintenanceBodyError>;

    /// Decode one value-free permanent revision gap.
    fn revision_gap(
        &mut self,
    ) -> Result<AdjustmentRevisionGapInput, AdjustmentMaintenanceBodyError>;

    /// Decode canonical rain control-state bytes without parsing their document.
    fn rain_control_state_stage(
        &mut self,
    ) -> Result<DecodedRainMaintenanceControlStateStage, AdjustmentMaintenanceBodyError>;

    /// Decode one closed rain control-state staging gap.
    fn rain_control_state_gap(
        &mut self,
    ) -> Result<RainMaintenanceControlStateGapInput, AdjustmentMaintenanceBodyError>;

    /// Decode one fixed-gauge target permanent gap.
    fn rain_fixed_gauge_target_gap(
        &mut self,
    ) -> Result<RainFixedGaugeTargetGapInput, AdjustmentMaintenanceBodyError>;

    /// Decode one fixed-gauge target gap status key.
    fn rain_fixed_gauge_target_gap_status(
        &mut self,
    ) -> Result<RainFixedGaugeTargetGapStatusInput, AdjustmentMaintenanceBodyError>;
}

/// Shared admitted time, cycle, due-key, and digest boundary.
pub trait AdjustmentMaintenanceAdmissionBoundary {
    /// Admit one typed capture body after the optional callback is present.
    fn capture(
        &self,
        request: AdjustmentMaintenanceCaptureRequest,
    ) -> Result<AdmittedAdjustmentMaintenanceCapture, AdjustmentMaintenanceBodyError>;

    /// Admit one typed value-free gap body.
    fn gap(
        &self,
        request: AdjustmentMaintenanceGapInput,
    ) -> Result<AdmittedAdjustmentMaintenanceGap, AdjustmentMaintenanceBodyError>;

    /// Admit canonical ordinary stage material without performing archive I/O.
    fn shadow_stage(
        &self,
        capture_epoch: &CanonicalInstant,
        request: DecodedAdjustmentMaintenanceStage,
    ) -> Result<AdmittedAdjustmentMaintenanceStage, AdjustmentMaintenanceBodyError>;

    /// Revalidate one returned ordinary stage receipt against its admitted input.
    fn shadow_stage_receipt(
        &self,
        input: &AdmittedAdjustmentMaintenanceStage,
        receipt: AdjustmentMaintenanceStageReceipt,
    ) -> Result<AdjustmentMaintenanceStageReceipt, AdjustmentMaintenanceBodyError>;

    /// Prepare canonical ordinary publish material for persisted admission.
    fn shadow_publish(
        &self,
        capture_epoch: &CanonicalInstant,
        request: DecodedAdjustmentMaintenancePublish,
    ) -> Result<PreparedAdjustmentMaintenancePublish, AdjustmentMaintenanceBodyError>;

    /// Revalidate one returned ordinary publish receipt against its admitted input.
    fn shadow_publish_receipt(
        &self,
        input: &AdmittedAdjustmentMaintenancePublish,
        receipt: AdjustmentMaintenancePublishReceipt,
    ) -> Result<AdjustmentMaintenancePublishReceipt, AdjustmentMaintenanceBodyError>;

    /// Admit one canonical revision transport document for staging.
    fn revision_stage(
        &self,
        capture_epoch: &CanonicalInstant,
        request: DecodedAdjustmentRevisionStage,
    ) -> Result<AdmittedAdjustmentRevisionStage, AdjustmentMaintenanceBodyError>;

    /// Revalidate one returned revision stage receipt against its admitted bytes and kind.
    fn revision_stage_receipt(
        &self,
        input: &AdmittedAdjustmentRevisionStage,
        receipt: AdjustmentRevisionStageReceipt,
    ) -> Result<AdjustmentRevisionStageReceipt, AdjustmentMaintenanceBodyError>;

    /// Prepare one canonical single or grouped revision publication.
    fn revision_publish(
        &self,
        capture_epoch: &CanonicalInstant,
        request: DecodedAdjustmentRevisionPublish,
    ) -> Result<PreparedAdjustmentRevisionPublish, AdjustmentMaintenanceBodyError>;

    /// Verify the archive's canonical commit instant and self-hash outside relay failure handling.
    fn verify_revision_publish_receipt(
        &self,
        receipt: &AdjustmentRevisionPublishResult,
    ) -> Result<(), AdjustmentMaintenanceBodyError>;

    /// Admit one closed permanent revision gap document.
    fn revision_gap(
        &self,
        request: AdjustmentRevisionGapInput,
    ) -> Result<AdmittedAdjustmentRevisionGap, AdjustmentMaintenanceBodyError>;

    /// Admit canonical future-only rain control-state bytes.
    fn rain_control_state_stage(
        &self,
        capture_epoch: &CanonicalInstant,
        request: DecodedRainMaintenanceControlStateStage,
    ) -> Result<AdmittedRainMaintenanceControlStateStage, AdjustmentMaintenanceBodyError>;

    /// Admit one exact rain control-state staging gap.
    fn rain_control_state_gap(
        &self,
        request: RainMaintenanceControlStateGapInput,
    ) -> Result<AdmittedRainMaintenanceControlStateGap, AdjustmentMaintenanceBodyError>;

    /// Admit one fixed-gauge target permanent gap.
    fn rain_fixed_gauge_target_gap(
        &self,
        request: RainFixedGaugeTargetGapInput,
    ) -> Result<AdmittedRainFixedGaugeTargetGap, AdjustmentMaintenanceBodyError>;

    /// Admit one fixed-gauge target status lookup key.
    fn rain_fixed_gauge_target_gap_status(
        &self,
        request: RainFixedGaugeTargetGapStatusInput,
    ) -> Result<AdmittedRainFixedGaugeTargetGapStatus, AdjustmentMaintenanceBodyError>;

    /// Revalidate one durable fixed-gauge receipt against its admitted request.
    fn rain_fixed_gauge_target_gap_receipt(
        &self,
        request: &AdmittedRainFixedGaugeTargetGap,
        receipt: RainFixedGaugeTargetGapReceipt,
    ) -> Result<RainFixedGaugeTargetGapReceipt, AdjustmentMaintenanceBodyError>;

    /// Revalidate one fixed-gauge status outcome against its admitted key.
    fn rain_fixed_gauge_target_gap_status_outcome(
        &self,
        request: &AdmittedRainFixedGaugeTargetGapStatus,
        status: RainFixedGaugeTargetGapStatus,
    ) -> Result<RainFixedGaugeTargetGapStatus, AdjustmentMaintenanceBodyError>;
}

/// Exact schedule-slot repository command boundary.
pub trait AdjustmentMaintenanceScheduleRepository {
    /// Execute one family-bound schedule-slot read.
    fn schedule_slot(
        &mut self,
        plan: AdjustmentShadowRegistrationSlotQueryPlan,
    ) -> Result<AdjustmentShadowRegistrationSlot, AdjustmentMaintenanceOperationError>;

    /// Execute the fixed ordered schedule cohort before propagating any individual failure.
    fn schedule_slots(
        &mut self,
        plans: [AdjustmentShadowRegistrationSlotQueryPlan; 3],
    ) -> Result<[AdjustmentShadowRegistrationSlot; 3], AdjustmentMaintenanceOperationError> {
        let [temperature, wind, rain] = plans.map(|plan| self.schedule_slot(plan));
        Ok([temperature?, wind?, rain?])
    }
}

/// Optional scheduler capture callback installed by the application.
pub trait AdjustmentMaintenanceCaptureCallback {
    /// Trigger one already admitted fixed capture cycle.
    fn capture(
        &mut self,
        request: AdmittedAdjustmentMaintenanceCapture,
    ) -> Result<(), AdjustmentMaintenanceOperationError>;
}

/// Value-free maintenance gap relay callback.
pub trait AdjustmentMaintenanceGapCallback {
    /// Record one already admitted categorical gap.
    fn record_gap(
        &mut self,
        request: AdmittedAdjustmentMaintenanceGap,
    ) -> Result<(), AdjustmentMaintenanceOperationError>;
}

/// Live database admission boundary used only after staging has completed.
pub trait AdjustmentMaintenancePublishRepository {
    /// Rebind one ordinary publish request to its authoritative persisted receipt.
    fn admit_shadow_publish(
        &mut self,
        prepared: PreparedAdjustmentMaintenancePublish,
    ) -> Result<AdmittedAdjustmentMaintenancePublish, AdjustmentMaintenanceOperationError>;

    /// Rebind every revision publish receipt to its authoritative current pointer.
    fn admit_revision_publish(
        &mut self,
        prepared: &PreparedAdjustmentRevisionPublish,
    ) -> Result<(), AdjustmentMaintenanceOperationError>;

    /// Permanently mark the exact current pointer before value-free cold archival.
    fn mark_revision_gap(
        &mut self,
        pointer: &AdjustmentRevisionAdmissionKey,
        gap: &AdjustmentRevisionGapInput,
    ) -> Result<(), AdjustmentMaintenanceOperationError>;
}

/// One repository owner capable of both schedule reads and publication admission.
pub trait AdjustmentMaintenanceRepository:
    AdjustmentMaintenanceScheduleRepository + AdjustmentMaintenancePublishRepository
{
}

impl<Repository> AdjustmentMaintenanceRepository for Repository where
    Repository:
        AdjustmentMaintenanceScheduleRepository + AdjustmentMaintenancePublishRepository + ?Sized
{
}

/// Durable archive relay for admitted stage and postcommit publication inputs.
pub trait AdjustmentMaintenancePublicationRelay {
    /// Persist ordinary stage bytes before database admission.
    fn stage_shadow(
        &mut self,
        input: &AdmittedAdjustmentMaintenanceStage,
    ) -> Result<AdjustmentMaintenanceStageReceipt, AdjustmentMaintenanceOperationError>;

    /// Publish ordinary bytes only after database admission.
    fn publish_shadow(
        &mut self,
        input: &AdmittedAdjustmentMaintenancePublish,
    ) -> Result<AdjustmentMaintenancePublishReceipt, AdjustmentMaintenanceOperationError>;

    /// Persist revision bytes before any current-pointer bind.
    fn stage_revision(
        &mut self,
        input: &AdmittedAdjustmentRevisionStage,
    ) -> Result<AdjustmentRevisionStageReceipt, AdjustmentMaintenanceOperationError>;

    /// Publish one single or grouped revision after database admission.
    fn publish_revision(
        &mut self,
        input: &AdmittedAdjustmentRevisionPublish,
    ) -> Result<AdjustmentRevisionPublishResult, AdjustmentMaintenanceOperationError>;

    /// Record one admitted permanent revision gap.
    fn record_revision_gap(
        &mut self,
        input: AdmittedAdjustmentRevisionGap,
    ) -> Result<(), AdjustmentMaintenanceOperationError>;

    /// Persist one canonical future-only rain control state.
    fn stage_rain_control_state(
        &mut self,
        input: AdmittedRainMaintenanceControlStateStage,
    ) -> Result<RainMaintenanceControlStateStageReceipt, AdjustmentMaintenanceOperationError>;

    /// Record one admitted rain control-state staging gap.
    fn record_rain_control_state_gap(
        &mut self,
        input: AdmittedRainMaintenanceControlStateGap,
    ) -> Result<(), AdjustmentMaintenanceOperationError>;

    /// Record one fixed-gauge target permanent gap and return its receipt.
    fn record_rain_fixed_gauge_target_gap(
        &mut self,
        input: &AdmittedRainFixedGaugeTargetGap,
    ) -> Result<RainFixedGaugeTargetGapReceipt, AdjustmentMaintenanceOperationError>;

    /// Read one fixed-gauge target permanent gap status.
    fn read_rain_fixed_gauge_target_gap(
        &mut self,
        input: &AdmittedRainFixedGaugeTargetGapStatus,
    ) -> Result<RainFixedGaugeTargetGapStatus, AdjustmentMaintenanceOperationError>;
}

/// One private relay owner capable of both categorical gaps and publication operations.
pub trait AdjustmentMaintenanceServiceRelay:
    AdjustmentMaintenanceGapCallback + AdjustmentMaintenancePublicationRelay
{
}

impl<Relay> AdjustmentMaintenanceServiceRelay for Relay where
    Relay: AdjustmentMaintenanceGapCallback + AdjustmentMaintenancePublicationRelay + ?Sized
{
}

/// Either legacy split callbacks or one retained private archive client.
pub enum AdjustmentMaintenanceRelayPorts<'gap, 'relay> {
    Split {
        gap: &'gap mut dyn AdjustmentMaintenanceGapCallback,
        publication_relay: Option<&'relay mut dyn AdjustmentMaintenancePublicationRelay>,
    },
    Unified(&'relay mut dyn AdjustmentMaintenanceServiceRelay),
}

impl<'relay> AdjustmentMaintenanceRelayPorts<'_, 'relay> {
    /// Reborrow only the categorical gap operation for the current dispatch branch.
    fn gap_callback(&mut self) -> &mut dyn AdjustmentMaintenanceGapCallback {
        match self {
            Self::Split { gap, .. } => &mut **gap,
            Self::Unified(relay) => &mut **relay,
        }
    }

    /// Reborrow only one publication operation while retaining legacy absence.
    fn publication_relay(
        &mut self,
    ) -> Option<&mut (dyn AdjustmentMaintenancePublicationRelay + 'relay)> {
        match self {
            Self::Split {
                publication_relay, ..
            } => publication_relay.as_deref_mut(),
            Self::Unified(relay) => Some(&mut **relay),
        }
    }
}

/// Per-listener admitted maintenance dependencies.
pub struct AdjustmentMaintenanceServicePorts<'admission, 'capture, 'gap, 'repository, 'relay> {
    pub capture_epoch: CanonicalInstant,
    pub admission: &'admission dyn AdjustmentMaintenanceAdmissionBoundary,
    pub capture: Option<&'capture mut dyn AdjustmentMaintenanceCaptureCallback>,
    pub gap: &'gap mut dyn AdjustmentMaintenanceGapCallback,
    pub publish_repository: Option<&'repository mut dyn AdjustmentMaintenancePublishRepository>,
    pub publication_relay: Option<&'relay mut dyn AdjustmentMaintenancePublicationRelay>,
}

/// Per-listener dependencies when one repository owns schedule and publication operations.
pub struct UnifiedAdjustmentMaintenanceServicePorts<'admission, 'capture, 'gap, 'relay> {
    pub capture_epoch: CanonicalInstant,
    pub admission: &'admission dyn AdjustmentMaintenanceAdmissionBoundary,
    pub capture: Option<&'capture mut dyn AdjustmentMaintenanceCaptureCallback>,
    pub relay: AdjustmentMaintenanceRelayPorts<'gap, 'relay>,
}

/// Stable transport-facing error literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentMaintenanceErrorCode {
    NotFound,
    RevisionSpoolFull,
    ShadowRejected,
}

/// One single or grouped revision publication receipt.
#[derive(Debug, Clone, PartialEq)]
pub enum AdjustmentRevisionPublishResult {
    Single(AdjustmentRevisionPublishReceipt),
    Batch(AdjustmentRevisionBatchPublishReceipt),
}

/// Typed body variants serialized only by the private transport adapter.
#[derive(Debug, Clone, PartialEq)]
pub enum AdjustmentMaintenanceResponseBody {
    ScheduleSlots(Vec<AdjustmentShadowRegistrationSlot>),
    Accepted,
    Recorded,
    ShadowStage(AdjustmentMaintenanceStageReceipt),
    ShadowPublish(AdjustmentMaintenancePublishReceipt),
    RevisionStage(AdjustmentRevisionStageReceipt),
    RevisionPublish(AdjustmentRevisionPublishResult),
    RainControlStateStage(RainMaintenanceControlStateStageReceipt),
    RainFixedGaugeTargetGap(RainFixedGaugeTargetGapReceipt),
    RainFixedGaugeTargetGapStatus(RainFixedGaugeTargetGapStatus),
    Error(AdjustmentMaintenanceErrorCode),
}

/// One transport-independent private maintenance response.
#[derive(Debug, Clone, PartialEq)]
pub struct AdjustmentMaintenanceResponse {
    pub status: u16,
    pub body: AdjustmentMaintenanceResponseBody,
}

/// Adapt independently owned legacy schedule and publication repositories to one dispatch owner.
struct SplitAdjustmentMaintenanceRepository<'schedule, 'publish> {
    schedule: &'schedule mut dyn AdjustmentMaintenanceScheduleRepository,
    publish: Option<&'publish mut dyn AdjustmentMaintenancePublishRepository>,
}

impl AdjustmentMaintenanceScheduleRepository for SplitAdjustmentMaintenanceRepository<'_, '_> {
    /// Delegate one legacy schedule read to its original repository owner.
    fn schedule_slot(
        &mut self,
        plan: AdjustmentShadowRegistrationSlotQueryPlan,
    ) -> Result<AdjustmentShadowRegistrationSlot, AdjustmentMaintenanceOperationError> {
        self.schedule.schedule_slot(plan)
    }

    /// Preserve a legacy repository's optimized or all-started schedule cohort behavior.
    fn schedule_slots(
        &mut self,
        plans: [AdjustmentShadowRegistrationSlotQueryPlan; 3],
    ) -> Result<[AdjustmentShadowRegistrationSlot; 3], AdjustmentMaintenanceOperationError> {
        self.schedule.schedule_slots(plans)
    }
}

impl AdjustmentMaintenancePublishRepository for SplitAdjustmentMaintenanceRepository<'_, '_> {
    /// Delegate ordinary publication admission when the optional legacy owner exists.
    fn admit_shadow_publish(
        &mut self,
        prepared: PreparedAdjustmentMaintenancePublish,
    ) -> Result<AdmittedAdjustmentMaintenancePublish, AdjustmentMaintenanceOperationError> {
        self.publish
            .as_deref_mut()
            .ok_or(AdjustmentMaintenanceOperationError::Rejected)?
            .admit_shadow_publish(prepared)
    }

    /// Delegate revision publication admission when the optional legacy owner exists.
    fn admit_revision_publish(
        &mut self,
        prepared: &PreparedAdjustmentRevisionPublish,
    ) -> Result<(), AdjustmentMaintenanceOperationError> {
        self.publish
            .as_deref_mut()
            .ok_or(AdjustmentMaintenanceOperationError::Rejected)?
            .admit_revision_publish(prepared)
    }

    /// Delegate permanent marker persistence when the optional legacy owner exists.
    fn mark_revision_gap(
        &mut self,
        pointer: &AdjustmentRevisionAdmissionKey,
        gap: &AdjustmentRevisionGapInput,
    ) -> Result<(), AdjustmentMaintenanceOperationError> {
        self.publish
            .as_deref_mut()
            .ok_or(AdjustmentMaintenanceOperationError::Rejected)?
            .mark_revision_gap(pointer, gap)
    }
}

/// Dispatch through the legacy independently borrowed repository inputs.
pub fn serve_adjustment_maintenance_request(
    authority: AdmittedAdjustmentMaintenanceAuthority,
    repository: &mut dyn AdjustmentMaintenanceScheduleRepository,
    body: &mut dyn AdjustmentMaintenanceBodyRead,
    ports: AdjustmentMaintenanceServicePorts<'_, '_, '_, '_, '_>,
    request: DecodedAdjustmentMaintenanceRequest<'_>,
) -> AdjustmentMaintenanceResponse {
    let AdjustmentMaintenanceServicePorts {
        capture_epoch,
        admission,
        capture,
        gap,
        publish_repository,
        publication_relay,
    } = ports;
    let publication_repository_available = publish_repository.is_some();
    let mut repository = SplitAdjustmentMaintenanceRepository {
        schedule: repository,
        publish: publish_repository,
    };
    serve_adjustment_maintenance_request_with_repository_impl(
        authority,
        &mut repository,
        body,
        publication_repository_available,
        UnifiedAdjustmentMaintenanceServicePorts {
            capture_epoch,
            admission,
            capture,
            relay: AdjustmentMaintenanceRelayPorts::Split {
                gap,
                publication_relay,
            },
        },
        request,
    )
}

/// Dispatch schedule and publication operations through one shared repository instance.
pub fn serve_adjustment_maintenance_request_with_repository(
    authority: AdmittedAdjustmentMaintenanceAuthority,
    repository: &mut dyn AdjustmentMaintenanceRepository,
    body: &mut dyn AdjustmentMaintenanceBodyRead,
    ports: UnifiedAdjustmentMaintenanceServicePorts<'_, '_, '_, '_>,
    request: DecodedAdjustmentMaintenanceRequest<'_>,
) -> AdjustmentMaintenanceResponse {
    serve_adjustment_maintenance_request_with_repository_impl(
        authority, repository, body, true, ports, request,
    )
}

/// Run the single-repository dispatch shared by compatibility and unified entrypoints.
fn serve_adjustment_maintenance_request_with_repository_impl<Repository>(
    authority: AdmittedAdjustmentMaintenanceAuthority,
    repository: &mut Repository,
    body: &mut dyn AdjustmentMaintenanceBodyRead,
    publication_repository_available: bool,
    mut ports: UnifiedAdjustmentMaintenanceServicePorts<'_, '_, '_, '_>,
    request: DecodedAdjustmentMaintenanceRequest<'_>,
) -> AdjustmentMaintenanceResponse
where
    Repository: AdjustmentMaintenanceRepository + ?Sized,
{
    // require the exact listener owner before dispatching private operations
    if authority.owner != AdjustmentMaintenanceListenerOwner::AdjustmentMaintenanceApplication {
        return shadow_rejected();
    }

    let request_class = classify_adjustment_maintenance_request(request);

    // preserve route and method rejection before any body read
    if request_class == AdjustmentMaintenanceRequestClass::NotFound {
        return not_found();
    }

    // serve fixed schedule reads without touching the request body
    if request_class == AdjustmentMaintenanceRequestClass::Schedule {
        return serve_schedule_slots(repository);
    }

    // apply the source body ceiling only after exact route and method matching
    if body.encoded_len() > MAX_ADJUSTMENT_MAINTENANCE_REQUEST_BYTES {
        return shadow_rejected();
    }

    // preserve the source capture branch before the ordinary gap branch
    if request.pathname == MAINTENANCE_CAPTURE_PATH {
        let decoded = match body.capture() {
            Ok(decoded) => decoded,
            Err(_) => return shadow_rejected(),
        };
        let Some(callback) = ports.capture.as_deref_mut() else {
            return shadow_rejected();
        };
        let admitted = match ports.admission.capture(decoded) {
            Ok(admitted) => admitted,
            Err(_) => return shadow_rejected(),
        };

        // enforce the inclusive authenticated capture-epoch boundary
        if admitted.issued_at.epoch_milliseconds < ports.capture_epoch.epoch_milliseconds {
            return shadow_rejected();
        }

        return match callback.capture(admitted) {
            Ok(()) => AdjustmentMaintenanceResponse {
                status: 202,
                body: AdjustmentMaintenanceResponseBody::Accepted,
            },
            Err(_) => shadow_rejected(),
        };
    }

    // preserve the ordinary categorical gap branch before staged publications
    if request.pathname == MAINTENANCE_GAP_PATH {
        let decoded = match body.gap() {
            Ok(decoded) => decoded,
            Err(_) => return shadow_rejected(),
        };
        let admitted = match ports.admission.gap(decoded) {
            Ok(admitted) => admitted,
            Err(_) => return shadow_rejected(),
        };
        return match ports.relay.gap_callback().record_gap(admitted) {
            Ok(()) => AdjustmentMaintenanceResponse {
                status: 200,
                body: AdjustmentMaintenanceResponseBody::Recorded,
            },
            Err(error) => operation_error(error),
        };
    }

    // preserve permanent revision gaps before stage and publication branches
    if request.pathname == MAINTENANCE_REVISION_GAP_PATH {
        let decoded = match body.revision_gap() {
            Ok(decoded) => decoded,
            Err(_) => return shadow_rejected(),
        };
        let admitted = match ports.admission.revision_gap(decoded) {
            Ok(admitted) => admitted,
            Err(_) => return shadow_rejected(),
        };
        let Some(relay) = ports.relay.publication_relay() else {
            return shadow_rejected();
        };
        return match relay.record_revision_gap(admitted) {
            Ok(()) => AdjustmentMaintenanceResponse {
                status: 200,
                body: AdjustmentMaintenanceResponseBody::Recorded,
            },
            Err(error) => operation_error(error),
        };
    }

    // keep archive staging independent from every live database admission
    if request.pathname == MAINTENANCE_REVISION_STAGE_PATH {
        let decoded = match body.revision_stage() {
            Ok(decoded) => decoded,
            Err(_) => return shadow_rejected(),
        };
        let admitted = match ports
            .admission
            .revision_stage(&ports.capture_epoch, decoded)
        {
            Ok(admitted) => admitted,
            Err(_) => return shadow_rejected(),
        };
        let Some(relay) = ports.relay.publication_relay() else {
            return shadow_rejected();
        };
        return match relay.stage_revision(&admitted) {
            Ok(receipt) => match ports.admission.revision_stage_receipt(&admitted, receipt) {
                Ok(receipt) => AdjustmentMaintenanceResponse {
                    status: 200,
                    body: AdjustmentMaintenanceResponseBody::RevisionStage(receipt),
                },
                Err(_) => shadow_rejected(),
            },
            Err(error) => operation_error(error),
        };
    }

    // stage canonical rain control state only after typed epoch admission
    if request.pathname == MAINTENANCE_RAIN_CONTROL_STATE_STAGE_PATH {
        let decoded = match body.rain_control_state_stage() {
            Ok(decoded) => decoded,
            Err(_) => return shadow_rejected(),
        };
        let admitted = match ports
            .admission
            .rain_control_state_stage(&ports.capture_epoch, decoded)
        {
            Ok(admitted) => admitted,
            Err(_) => return shadow_rejected(),
        };
        let Some(relay) = ports.relay.publication_relay() else {
            return shadow_rejected();
        };
        return match relay.stage_rain_control_state(admitted) {
            Ok(receipt) => AdjustmentMaintenanceResponse {
                status: 200,
                body: AdjustmentMaintenanceResponseBody::RainControlStateStage(receipt),
            },
            Err(error) => operation_error(error),
        };
    }

    // record one exact rain control-state staging gap without value bytes
    if request.pathname == MAINTENANCE_RAIN_CONTROL_STATE_GAP_PATH {
        let decoded = match body.rain_control_state_gap() {
            Ok(decoded) => decoded,
            Err(_) => return shadow_rejected(),
        };
        let admitted = match ports.admission.rain_control_state_gap(decoded) {
            Ok(admitted) => admitted,
            Err(_) => return shadow_rejected(),
        };
        let Some(relay) = ports.relay.publication_relay() else {
            return shadow_rejected();
        };
        return match relay.record_rain_control_state_gap(admitted) {
            Ok(()) => AdjustmentMaintenanceResponse {
                status: 200,
                body: AdjustmentMaintenanceResponseBody::Recorded,
            },
            Err(error) => operation_error(error),
        };
    }

    // retain one exact fixed-gauge target gap and return its durable receipt
    if request.pathname == MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_PATH {
        let decoded = match body.rain_fixed_gauge_target_gap() {
            Ok(decoded) => decoded,
            Err(_) => return shadow_rejected(),
        };
        let admitted = match ports.admission.rain_fixed_gauge_target_gap(decoded) {
            Ok(admitted) => admitted,
            Err(_) => return shadow_rejected(),
        };
        let Some(relay) = ports.relay.publication_relay() else {
            return shadow_rejected();
        };
        let receipt = match relay.record_rain_fixed_gauge_target_gap(&admitted) {
            Ok(receipt) => receipt,
            Err(error) => return operation_error(error),
        };
        return match ports
            .admission
            .rain_fixed_gauge_target_gap_receipt(&admitted, receipt)
        {
            Ok(receipt) => AdjustmentMaintenanceResponse {
                status: 200,
                body: AdjustmentMaintenanceResponseBody::RainFixedGaugeTargetGap(receipt),
            },
            Err(_) => shadow_rejected(),
        };
    }

    // read permanent fixed-gauge disposition only for one admitted target key
    if request.pathname == MAINTENANCE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_PATH {
        let decoded = match body.rain_fixed_gauge_target_gap_status() {
            Ok(decoded) => decoded,
            Err(_) => return shadow_rejected(),
        };
        let admitted = match ports.admission.rain_fixed_gauge_target_gap_status(decoded) {
            Ok(admitted) => admitted,
            Err(_) => return shadow_rejected(),
        };
        let Some(relay) = ports.relay.publication_relay() else {
            return shadow_rejected();
        };
        let status = match relay.read_rain_fixed_gauge_target_gap(&admitted) {
            Ok(status) => status,
            Err(error) => return operation_error(error),
        };
        return match ports
            .admission
            .rain_fixed_gauge_target_gap_status_outcome(&admitted, status)
        {
            Ok(status) => AdjustmentMaintenanceResponse {
                status: 200,
                body: AdjustmentMaintenanceResponseBody::RainFixedGaugeTargetGapStatus(status),
            },
            Err(_) => shadow_rejected(),
        };
    }

    // require live persisted pointer admission before any revision publication
    if request.pathname == MAINTENANCE_REVISION_PUBLISH_PATH {
        let decoded = match body.revision_publish() {
            Ok(decoded) => decoded,
            Err(_) => return shadow_rejected(),
        };
        let prepared = match ports
            .admission
            .revision_publish(&ports.capture_epoch, decoded)
        {
            Ok(prepared) => prepared,
            Err(_) => return shadow_rejected(),
        };
        // preserve the compatibility entrypoint's optional publication owner boundary
        if !publication_repository_available {
            return shadow_rejected();
        }
        let Some(relay) = ports.relay.publication_relay() else {
            return shadow_rejected();
        };
        return publish_revision(repository, relay, ports.admission, prepared);
    }

    // retain ordinary shadow stage before the persisted publish branch
    if request.pathname == MAINTENANCE_SHADOW_STAGE_PATH {
        let decoded = match body.shadow_stage() {
            Ok(decoded) => decoded,
            Err(_) => return shadow_rejected(),
        };
        let admitted = match ports.admission.shadow_stage(&ports.capture_epoch, decoded) {
            Ok(admitted) => admitted,
            Err(_) => return shadow_rejected(),
        };
        let Some(relay) = ports.relay.publication_relay() else {
            return shadow_rejected();
        };
        return match relay.stage_shadow(&admitted) {
            Ok(receipt) => match ports.admission.shadow_stage_receipt(&admitted, receipt) {
                Ok(receipt) => AdjustmentMaintenanceResponse {
                    status: 200,
                    body: AdjustmentMaintenanceResponseBody::ShadowStage(receipt),
                },
                Err(_) => shadow_rejected(),
            },
            Err(error) => operation_error(error),
        };
    }

    let decoded = match body.shadow_publish() {
        Ok(decoded) => decoded,
        Err(_) => return shadow_rejected(),
    };
    let prepared = match ports
        .admission
        .shadow_publish(&ports.capture_epoch, decoded)
    {
        Ok(prepared) => prepared,
        Err(_) => return shadow_rejected(),
    };
    // preserve the compatibility entrypoint's optional publication owner boundary
    if !publication_repository_available {
        return shadow_rejected();
    }
    let admitted = match repository.admit_shadow_publish(prepared) {
        Ok(admitted) => admitted,
        Err(error) => return operation_error(error),
    };
    let Some(relay) = ports.relay.publication_relay() else {
        return shadow_rejected();
    };
    match relay.publish_shadow(&admitted) {
        Ok(receipt) => match ports.admission.shadow_publish_receipt(&admitted, receipt) {
            Ok(receipt) => AdjustmentMaintenanceResponse {
                status: 200,
                body: AdjustmentMaintenanceResponseBody::ShadowPublish(receipt),
            },
            Err(_) => shadow_rejected(),
        },
        Err(error) => operation_error(error),
    }
}

/// Rebind callers to the persisted pointers before publishing staged bytes.
fn publish_revision<Repository>(
    repository: &mut Repository,
    relay: &mut dyn AdjustmentMaintenancePublicationRelay,
    admission: &dyn AdjustmentMaintenanceAdmissionBoundary,
    prepared: PreparedAdjustmentRevisionPublish,
) -> AdjustmentMaintenanceResponse
where
    Repository: AdjustmentMaintenancePublishRepository + ?Sized,
{
    let (pointers, receipts) = prepared.rows();
    // reject malformed receipt cardinality before the durable publication sequence
    if pointers.is_empty() || pointers.len() != receipts.len() {
        return shadow_rejected();
    }
    let single = matches!(
        prepared.publication,
        DecodedAdjustmentRevisionPublish::Single { .. }
    );
    // mark every affected pointer after an ordinary database admission failure
    if repository.admit_revision_publish(&prepared).is_err() {
        let _ = record_revision_publication_gaps(
            repository,
            relay,
            pointers,
            prepared.stage_receipt(),
            single,
            AdjustmentRevisionGapReason::DatabaseAdmissionFailed,
        );
        return shadow_rejected();
    }
    let (admitted, pointers) = prepared.into_admitted();
    match relay.publish_revision(&admitted) {
        Ok(receipt)
            if admitted.matches_publish_result(&receipt)
                && admission.verify_revision_publish_receipt(&receipt).is_ok() =>
        {
            AdjustmentMaintenanceResponse {
                status: 200,
                body: AdjustmentMaintenanceResponseBody::RevisionPublish(receipt),
            }
        }
        // a bad returned checkpoint is distinct from an archive call failure
        Ok(_) => shadow_rejected(),
        // retain the permanent disposition before reporting the original archive error
        Err(error) => match record_revision_publication_gaps(
            repository,
            relay,
            &pointers,
            admitted.stage_receipt(),
            single,
            AdjustmentRevisionGapReason::ArchivePublishFailed,
        ) {
            Ok(()) => operation_error(error),
            Err(gap_error) => operation_error(gap_error),
        },
    }
}

/// Record all database row markers, then one shared cold gap for the staged body.
fn record_revision_publication_gaps<Repository>(
    repository: &mut Repository,
    relay: &mut dyn AdjustmentMaintenancePublicationRelay,
    pointers: &[AdjustmentRevisionPublicationPointer],
    stage: &AdjustmentRevisionStageReceipt,
    single: bool,
    reason: AdjustmentRevisionGapReason,
) -> Result<(), AdjustmentMaintenanceOperationError>
where
    Repository: AdjustmentMaintenancePublishRepository + ?Sized,
{
    let mut first_gap = None;
    // retain row-specific logical identities in canonical document order
    for pointer in pointers {
        let gap = AdjustmentRevisionGapInput {
            logicalKeySha256: pointer.logical_key_sha256.clone(),
            projectionIdentitySha256: Nullable::Value(stage.projectionIdentitySha256.clone()),
            projectionKind: stage.projectionKind,
            projectionSha256: Nullable::Value(stage.projectionIdentitySha256.clone()),
            reason,
        };
        let recorded = repository.mark_revision_gap(&pointer.key, &gap);
        // the single-row Node path attempts its cold disposition even after a database error
        if single {
            let _ = relay.record_revision_gap(AdmittedAdjustmentRevisionGap { request: gap });
            return recorded;
        }
        // the grouped Node path stops at the first failed database marker
        recorded?;
        first_gap.get_or_insert(gap);
    }
    // do not let cold archival failure mask already retained database markers
    if let Some(request) = first_gap {
        let _ = relay.record_revision_gap(AdmittedAdjustmentRevisionGap { request });
    }
    Ok(())
}

/// Read and project the fixed temperature, wind, and rain schedule slots.
fn serve_schedule_slots<Repository>(repository: &mut Repository) -> AdjustmentMaintenanceResponse
where
    Repository: AdjustmentMaintenanceScheduleRepository + ?Sized,
{
    let reads = maintenance_schedule_slot_reads();

    // execute all fixed family reads before projecting their ordered results
    let Ok(slots) = repository.schedule_slots(reads) else {
        return shadow_rejected();
    };
    match project_maintenance_schedule_slots(slots) {
        Ok(slots) => AdjustmentMaintenanceResponse {
            status: 200,
            body: AdjustmentMaintenanceResponseBody::ScheduleSlots(slots),
        },
        Err(_) => shadow_rejected(),
    }
}

/// Build the fixed private not-found response.
fn not_found() -> AdjustmentMaintenanceResponse {
    AdjustmentMaintenanceResponse {
        status: 404,
        body: AdjustmentMaintenanceResponseBody::Error(AdjustmentMaintenanceErrorCode::NotFound),
    }
}

/// Build the one redacted private operation failure.
fn shadow_rejected() -> AdjustmentMaintenanceResponse {
    AdjustmentMaintenanceResponse {
        status: 409,
        body: AdjustmentMaintenanceResponseBody::Error(
            AdjustmentMaintenanceErrorCode::ShadowRejected,
        ),
    }
}

/// Map one bounded operation failure to the exact private response category.
fn operation_error(error: AdjustmentMaintenanceOperationError) -> AdjustmentMaintenanceResponse {
    // retain the one explicit revision spool capacity response
    if error == AdjustmentMaintenanceOperationError::RevisionSpoolFull {
        return AdjustmentMaintenanceResponse {
            status: 429,
            body: AdjustmentMaintenanceResponseBody::Error(
                AdjustmentMaintenanceErrorCode::RevisionSpoolFull,
            ),
        };
    }
    shadow_rejected()
}
