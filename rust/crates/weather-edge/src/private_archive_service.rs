use crate::private_shadow_archive::PrivateShadowPublishInput;
use weather_contracts::{
    AdjustmentMaintenanceGapInput, AdjustmentMaintenancePublishReceipt,
    AdjustmentMaintenanceStageInput, AdjustmentMaintenanceStageReceipt,
    AdjustmentRevisionBatchPublishReceipt, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionGapInput, AdjustmentRevisionPublishReceipt, AdjustmentRevisionStageReceipt,
    RainFixedGaugeTargetGapInput, RainFixedGaugeTargetGapReceipt, RainFixedGaugeTargetGapStatus,
    RainFixedGaugeTargetGapStatusInput, RainMaintenanceControlStateGapInput,
    RainMaintenanceControlStateStageReceipt,
};

pub const PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES: usize = 4_832 * 1024;

pub const PRIVATE_ARCHIVE_STAGE_TARGET: &str = "/internal/adjustment-maintenance/archive/stage";
pub const PRIVATE_ARCHIVE_PUBLISH_TARGET: &str = "/internal/adjustment-maintenance/archive/publish";
pub const PRIVATE_ARCHIVE_GAP_TARGET: &str = "/internal/adjustment-maintenance/archive/gap";
pub const PRIVATE_REVISION_STAGE_TARGET: &str = "/internal/adjustment-maintenance/revision/stage";
pub const PRIVATE_REVISION_PUBLISH_TARGET: &str =
    "/internal/adjustment-maintenance/revision/publish";
pub const PRIVATE_REVISION_GAP_TARGET: &str = "/internal/adjustment-maintenance/revision/gap";
pub const PRIVATE_RAIN_CONTROL_STATE_STAGE_TARGET: &str =
    "/internal/adjustment-maintenance/rain-control-state/stage";
pub const PRIVATE_RAIN_CONTROL_STATE_GAP_TARGET: &str =
    "/internal/adjustment-maintenance/rain-control-state/gap";
pub const PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_TARGET: &str =
    "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap";
pub const PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET: &str =
    "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status";

/// exact listener allowlist order from the Node web edge.
pub const SUPPORTED_PRIVATE_ARCHIVE_TARGETS: [&str; 10] = [
    PRIVATE_ARCHIVE_STAGE_TARGET,
    PRIVATE_ARCHIVE_PUBLISH_TARGET,
    PRIVATE_ARCHIVE_GAP_TARGET,
    PRIVATE_REVISION_STAGE_TARGET,
    PRIVATE_REVISION_PUBLISH_TARGET,
    PRIVATE_REVISION_GAP_TARGET,
    PRIVATE_RAIN_CONTROL_STATE_STAGE_TARGET,
    PRIVATE_RAIN_CONTROL_STATE_GAP_TARGET,
    PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_TARGET,
    PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
];

/// exact storage-dispatch order from the Node archive adapter.
pub const PRIVATE_ARCHIVE_HANDLER_TARGETS: [&str; 10] = [
    PRIVATE_RAIN_CONTROL_STATE_STAGE_TARGET,
    PRIVATE_RAIN_CONTROL_STATE_GAP_TARGET,
    PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_TARGET,
    PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
    PRIVATE_ARCHIVE_STAGE_TARGET,
    PRIVATE_ARCHIVE_PUBLISH_TARGET,
    PRIVATE_ARCHIVE_GAP_TARGET,
    PRIVATE_REVISION_STAGE_TARGET,
    PRIVATE_REVISION_PUBLISH_TARGET,
    PRIVATE_REVISION_GAP_TARGET,
];

/// one decoded operation selected by the private listener target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateArchiveOperation {
    StageRainControlState,
    RecordRainControlStateGap,
    RecordRainFixedGaugeTargetGap,
    ReadRainFixedGaugeTargetGap,
    StageShadowRevision,
    PublishShadowRevision,
    RecordShadowGap,
    StageRevision,
    PublishRevision,
    RecordRevisionGap,
}

impl PrivateArchiveOperation {
    /// select one operation in the same order as the Node storage adapter.
    pub fn from_target(target: &str) -> Option<Self> {
        // preserve route-to-operation ownership
        match target {
            PRIVATE_RAIN_CONTROL_STATE_STAGE_TARGET => Some(Self::StageRainControlState),
            PRIVATE_RAIN_CONTROL_STATE_GAP_TARGET => Some(Self::RecordRainControlStateGap),
            PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_TARGET => Some(Self::RecordRainFixedGaugeTargetGap),
            PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET => {
                Some(Self::ReadRainFixedGaugeTargetGap)
            }
            PRIVATE_ARCHIVE_STAGE_TARGET => Some(Self::StageShadowRevision),
            PRIVATE_ARCHIVE_PUBLISH_TARGET => Some(Self::PublishShadowRevision),
            PRIVATE_ARCHIVE_GAP_TARGET => Some(Self::RecordShadowGap),
            PRIVATE_REVISION_STAGE_TARGET => Some(Self::StageRevision),
            PRIVATE_REVISION_PUBLISH_TARGET => Some(Self::PublishRevision),
            PRIVATE_REVISION_GAP_TARGET => Some(Self::RecordRevisionGap),
            _ => None,
        }
    }
}

/// one admitted single or grouped revision publication.
#[derive(Debug, Clone, PartialEq)]
pub enum PrivateRevisionPublishInput {
    Single {
        revision_receipt: AdjustmentRevisionCommitReceipt,
        stage_receipt: AdjustmentRevisionStageReceipt,
    },
    Batch {
        revision_receipts: Vec<AdjustmentRevisionCommitReceipt>,
        stage_receipt: AdjustmentRevisionStageReceipt,
    },
}

/// one admitted decoded body supplied after bounded JSON parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum PrivateArchiveInput {
    StageRainControlState(Vec<u8>),
    RecordRainControlStateGap(RainMaintenanceControlStateGapInput),
    RecordRainFixedGaugeTargetGap(RainFixedGaugeTargetGapInput),
    ReadRainFixedGaugeTargetGap(RainFixedGaugeTargetGapStatusInput),
    StageShadowRevision(Box<AdjustmentMaintenanceStageInput>),
    PublishShadowRevision(Box<PrivateShadowPublishInput>),
    RecordShadowGap(AdjustmentMaintenanceGapInput),
    StageRevision(Vec<u8>),
    PublishRevision(Box<PrivateRevisionPublishInput>),
    RecordRevisionGap(AdjustmentRevisionGapInput),
}

/// one already-decoded request with the original encoded byte count.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedPrivateArchiveRequest<'a> {
    pub method: &'a str,
    pub target: &'a str,
    pub content_type: Option<&'a str>,
    pub encoded_body_bytes: usize,
    pub input: PrivateArchiveInput,
}

/// one transport request whose body remains unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateArchiveTransportRequest<'a> {
    pub method: &'a str,
    pub target: &'a str,
    pub content_type: Option<&'a str>,
}

/// lazy bounded body decoding below method and header admission.
pub trait PrivateArchiveBodyReader {
    type Error;

    /// read and decode only the selected operation under the supplied ceiling.
    fn read_decoded(
        &mut self,
        operation: PrivateArchiveOperation,
        maximum_bytes: usize,
    ) -> Result<PrivateArchiveInput, Self::Error>;
}

/// the durable rain-control abandonment returned by the archive store.
#[derive(Debug, Clone, PartialEq)]
pub struct RainMaintenanceControlStateGapReceipt {
    pub contract_version: String,
    pub gap_at: String,
    pub gap_sha256: String,
    pub reason: weather_contracts::RainMaintenanceControlStateGapInputReasonLiteral,
    pub stage_receipt_sha256: String,
    pub state_sha256: String,
}

/// the durable value-free shadow disposition returned by the archive store.
#[derive(Debug, Clone, PartialEq)]
pub struct AdjustmentMaintenanceGapReceipt {
    pub contract_version: String,
    pub input: AdjustmentMaintenanceGapInput,
    pub qualification_disposition: String,
    pub recorded_at: String,
    pub gap_sha256: String,
}

/// the durable value-free revision disposition returned by the archive store.
#[derive(Debug, Clone, PartialEq)]
pub struct AdjustmentRevisionGapReceipt {
    pub contract_version: String,
    pub input: AdjustmentRevisionGapInput,
    pub qualification_disposition: String,
    pub recorded_at: String,
    pub gap_sha256: String,
}

/// one single or grouped revision publication receipt.
#[derive(Debug, Clone, PartialEq)]
pub enum PrivateRevisionPublishReceipt {
    Single(AdjustmentRevisionPublishReceipt),
    Batch(AdjustmentRevisionBatchPublishReceipt),
}

/// one successful private archive response body.
#[derive(Debug, Clone, PartialEq)]
pub enum PrivateArchiveReceipt {
    RainControlStateStaged(RainMaintenanceControlStateStageReceipt),
    RainControlStateGapRecorded(RainMaintenanceControlStateGapReceipt),
    RainFixedGaugeTargetGapRecorded(RainFixedGaugeTargetGapReceipt),
    RainFixedGaugeTargetGapStatus(RainFixedGaugeTargetGapStatus),
    ShadowRevisionStaged(AdjustmentMaintenanceStageReceipt),
    ShadowRevisionPublished(AdjustmentMaintenancePublishReceipt),
    ShadowGapRecorded(AdjustmentMaintenanceGapReceipt),
    RevisionStaged(AdjustmentRevisionStageReceipt),
    RevisionPublished(PrivateRevisionPublishReceipt),
    RevisionGapRecorded(AdjustmentRevisionGapReceipt),
}

/// closed operational failures produced by archive storage ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateArchiveStorageError {
    Rejected,
    RevisionSpoolRefused,
}

impl PrivateArchiveStorageError {
    /// return the only legacy storage error code exposed by the listener.
    pub const fn code(self) -> Option<&'static str> {
        // expose only bounded revision capacity
        match self {
            Self::Rejected => None,
            Self::RevisionSpoolRefused => Some("adjustment_revision_spool_refused"),
        }
    }
}

/// separately bindable rain control state storage operations.
pub trait PrivateArchiveRainControlStore {
    /// stage one admitted canonical rain control state.
    fn stage_rain_control_state(
        &mut self,
        bytes: Vec<u8>,
    ) -> Result<RainMaintenanceControlStateStageReceipt, PrivateArchiveStorageError>;

    /// record one admitted permanent rain control state gap.
    fn record_rain_control_state_gap(
        &mut self,
        input: RainMaintenanceControlStateGapInput,
    ) -> Result<RainMaintenanceControlStateGapReceipt, PrivateArchiveStorageError>;
}

/// separately bindable permanent fixed-gauge storage operations.
pub trait PrivateArchiveFixedGaugeStore {
    /// record one admitted permanent fixed-gauge target gap.
    fn record_rain_fixed_gauge_target_gap(
        &mut self,
        input: RainFixedGaugeTargetGapInput,
    ) -> Result<RainFixedGaugeTargetGapReceipt, PrivateArchiveStorageError>;

    /// read one admitted fixed-gauge target gap status.
    fn read_rain_fixed_gauge_target_gap(
        &mut self,
        input: RainFixedGaugeTargetGapStatusInput,
    ) -> Result<RainFixedGaugeTargetGapStatus, PrivateArchiveStorageError>;
}

/// Separately bound shadow stage and capsule publication owner.
pub trait PrivateArchiveShadowStore {
    /// stage one admitted decoded shadow revision.
    fn stage_shadow_revision(
        &mut self,
        input: AdjustmentMaintenanceStageInput,
    ) -> Result<AdjustmentMaintenanceStageReceipt, PrivateArchiveStorageError>;

    /// publish one admitted decoded shadow revision.
    fn publish_shadow_revision(
        &mut self,
        input: PrivateShadowPublishInput,
    ) -> Result<AdjustmentMaintenancePublishReceipt, PrivateArchiveStorageError>;
}

/// Separately bound single and grouped projection publication owner.
pub trait PrivateArchiveRevisionStore {
    /// stage one admitted canonical revision projection.
    fn stage_revision(
        &mut self,
        bytes: Vec<u8>,
    ) -> Result<AdjustmentRevisionStageReceipt, PrivateArchiveStorageError>;

    /// publish one admitted single or grouped revision.
    fn publish_revision(
        &mut self,
        input: PrivateRevisionPublishInput,
    ) -> Result<PrivateRevisionPublishReceipt, PrivateArchiveStorageError>;
}

/// Storage operations owned below the route-neutral private listener.
pub trait PrivateArchiveStorage {
    /// Return the separately bound rain control archive engine.
    fn rain_control_store(&mut self) -> &mut dyn PrivateArchiveRainControlStore;

    /// Return the separately bound fixed-gauge archive engine.
    fn fixed_gauge_gap_store(&mut self) -> &mut dyn PrivateArchiveFixedGaugeStore;

    /// Return the admitted shadow stage and publication archive engine.
    fn shadow_store(&mut self) -> &mut dyn PrivateArchiveShadowStore;

    /// Return the admitted projection stage and publication archive engine.
    fn revision_store(&mut self) -> &mut dyn PrivateArchiveRevisionStore;

    /// Return the separately bound permanent disposition store.
    fn permanent_gap_store(&mut self) -> &mut dyn PrivateArchivePermanentGapStore;
}

/// Separately bound shadow and revision permanent metadata owner.
pub trait PrivateArchivePermanentGapStore {
    /// Record one admitted permanent shadow gap.
    fn record_shadow_gap(
        &mut self,
        input: AdjustmentMaintenanceGapInput,
    ) -> Result<AdjustmentMaintenanceGapReceipt, PrivateArchiveStorageError>;

    /// record one admitted permanent revision gap.
    fn record_revision_gap(
        &mut self,
        input: AdjustmentRevisionGapInput,
    ) -> Result<AdjustmentRevisionGapReceipt, PrivateArchiveStorageError>;
}

/// exact HTTP status values emitted by the private listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateArchiveStatus {
    Ok,
    Conflict,
    NotFound,
    UnsupportedMediaType,
    TooManyRequests,
}

impl PrivateArchiveStatus {
    /// return the wire status code for the listener response.
    pub const fn code(self) -> u16 {
        // preserve Node listener status values
        match self {
            Self::Ok => 200,
            Self::Conflict => 409,
            Self::NotFound => 404,
            Self::UnsupportedMediaType => 415,
            Self::TooManyRequests => 429,
        }
    }
}

/// exact error bodies emitted by the private listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateArchiveErrorBody {
    NotFound,
    RevisionArchiveRejected,
    RevisionSpoolFull,
    UnsupportedMediaType,
}

impl PrivateArchiveErrorBody {
    /// return the legacy JSON error string.
    pub const fn code(self) -> &'static str {
        // preserve Node listener error strings
        match self {
            Self::NotFound => "not_found",
            Self::RevisionArchiveRejected => "revision_archive_rejected",
            Self::RevisionSpoolFull => "adjustment_revision_spool_full",
            Self::UnsupportedMediaType => "unsupported_media_type",
        }
    }
}

/// one serializable success or error body returned by the listener core.
#[derive(Debug, Clone, PartialEq)]
pub enum PrivateArchiveResponseBody {
    Receipt(PrivateArchiveReceipt),
    Error(PrivateArchiveErrorBody),
}

/// one complete status and body result for an admitted transport adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct PrivateArchiveResponse {
    pub status: PrivateArchiveStatus,
    pub body: PrivateArchiveResponseBody,
}

impl PrivateArchiveResponse {
    /// build one successful receipt response.
    fn ok(receipt: PrivateArchiveReceipt) -> Self {
        Self {
            status: PrivateArchiveStatus::Ok,
            body: PrivateArchiveResponseBody::Receipt(receipt),
        }
    }

    /// build one closed error response.
    fn error(status: PrivateArchiveStatus, body: PrivateArchiveErrorBody) -> Self {
        Self {
            status,
            body: PrivateArchiveResponseBody::Error(body),
        }
    }
}

/// one eager decoded body adapted through the lazy transport boundary.
struct EagerPrivateArchiveBody {
    encoded_body_bytes: usize,
    input: Option<PrivateArchiveInput>,
}

impl PrivateArchiveBodyReader for EagerPrivateArchiveBody {
    type Error = ();

    /// enforce the ceiling before releasing the decoded fixture body.
    fn read_decoded(
        &mut self,
        _operation: PrivateArchiveOperation,
        maximum_bytes: usize,
    ) -> Result<PrivateArchiveInput, Self::Error> {
        // reject a body beyond the exact listener ceiling
        if self.encoded_body_bytes > maximum_bytes {
            return Err(());
        }
        self.input.take().ok_or(())
    }
}

/// dispatch one already-decoded request through the lazy listener boundary.
pub fn dispatch_private_archive_request<Storage: PrivateArchiveStorage>(
    request: AdmittedPrivateArchiveRequest<'_>,
    storage: &mut Storage,
) -> PrivateArchiveResponse {
    let AdmittedPrivateArchiveRequest {
        method,
        target,
        content_type,
        encoded_body_bytes,
        input,
    } = request;
    let mut body = EagerPrivateArchiveBody {
        encoded_body_bytes,
        input: Some(input),
    };
    serve_private_archive_request(
        PrivateArchiveTransportRequest {
            method,
            target,
            content_type,
        },
        &mut body,
        storage,
    )
}

/// serve one transport request while preserving pre-body admission order.
pub fn serve_private_archive_request<Reader, Storage>(
    request: PrivateArchiveTransportRequest<'_>,
    body: &mut Reader,
    storage: &mut Storage,
) -> PrivateArchiveResponse
where
    Reader: PrivateArchiveBodyReader,
    Storage: PrivateArchiveStorage,
{
    // admit method and raw target before consulting headers or body
    let operation = match admit_private_archive_transport(request) {
        Ok(operation) => operation,
        Err((status, body)) => return PrivateArchiveResponse::error(status, body),
    };
    // request decoded bytes only after transport admission
    let input = match body.read_decoded(operation, PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES) {
        Ok(input) => input,
        Err(_) => {
            return PrivateArchiveResponse::error(
                PrivateArchiveStatus::Conflict,
                PrivateArchiveErrorBody::RevisionArchiveRejected,
            );
        }
    };
    dispatch_private_archive_operation(operation, input, storage)
}

/// admit exact listener metadata without accessing request bytes.
pub fn admit_private_archive_transport(
    request: PrivateArchiveTransportRequest<'_>,
) -> Result<PrivateArchiveOperation, (PrivateArchiveStatus, PrivateArchiveErrorBody)> {
    // reject methods and raw targets before inspecting content metadata
    let Some(operation) = (request.method == "POST"
        && SUPPORTED_PRIVATE_ARCHIVE_TARGETS.contains(&request.target))
    .then(|| PrivateArchiveOperation::from_target(request.target))
    .flatten() else {
        return Err((
            PrivateArchiveStatus::NotFound,
            PrivateArchiveErrorBody::NotFound,
        ));
    };
    // require the original json media class before body admission
    if !is_json_content_type(request.content_type) {
        return Err((
            PrivateArchiveStatus::UnsupportedMediaType,
            PrivateArchiveErrorBody::UnsupportedMediaType,
        ));
    }
    Ok(operation)
}

/// bind one decoded body to its selected storage operation.
fn dispatch_private_archive_operation<Storage: PrivateArchiveStorage>(
    operation: PrivateArchiveOperation,
    input: PrivateArchiveInput,
    storage: &mut Storage,
) -> PrivateArchiveResponse {
    // preserve the source route-to-operation binding
    let result = match (operation, input) {
        (
            PrivateArchiveOperation::StageRainControlState,
            PrivateArchiveInput::StageRainControlState(bytes),
        ) => storage
            .rain_control_store()
            .stage_rain_control_state(bytes)
            .map(PrivateArchiveReceipt::RainControlStateStaged),
        (
            PrivateArchiveOperation::RecordRainControlStateGap,
            PrivateArchiveInput::RecordRainControlStateGap(input),
        ) => storage
            .rain_control_store()
            .record_rain_control_state_gap(input)
            .map(PrivateArchiveReceipt::RainControlStateGapRecorded),
        (
            PrivateArchiveOperation::RecordRainFixedGaugeTargetGap,
            PrivateArchiveInput::RecordRainFixedGaugeTargetGap(input),
        ) => storage
            .fixed_gauge_gap_store()
            .record_rain_fixed_gauge_target_gap(input)
            .map(PrivateArchiveReceipt::RainFixedGaugeTargetGapRecorded),
        (
            PrivateArchiveOperation::ReadRainFixedGaugeTargetGap,
            PrivateArchiveInput::ReadRainFixedGaugeTargetGap(input),
        ) => storage
            .fixed_gauge_gap_store()
            .read_rain_fixed_gauge_target_gap(input)
            .map(PrivateArchiveReceipt::RainFixedGaugeTargetGapStatus),
        (
            PrivateArchiveOperation::StageShadowRevision,
            PrivateArchiveInput::StageShadowRevision(input),
        ) => storage
            .shadow_store()
            .stage_shadow_revision(*input)
            .map(PrivateArchiveReceipt::ShadowRevisionStaged),
        (
            PrivateArchiveOperation::PublishShadowRevision,
            PrivateArchiveInput::PublishShadowRevision(input),
        ) => storage
            .shadow_store()
            .publish_shadow_revision(*input)
            .map(PrivateArchiveReceipt::ShadowRevisionPublished),
        (PrivateArchiveOperation::RecordShadowGap, PrivateArchiveInput::RecordShadowGap(input)) => {
            storage
                .permanent_gap_store()
                .record_shadow_gap(input)
                .map(PrivateArchiveReceipt::ShadowGapRecorded)
        }
        (PrivateArchiveOperation::StageRevision, PrivateArchiveInput::StageRevision(bytes)) => {
            storage
                .revision_store()
                .stage_revision(bytes)
                .map(PrivateArchiveReceipt::RevisionStaged)
        }
        (PrivateArchiveOperation::PublishRevision, PrivateArchiveInput::PublishRevision(input)) => {
            storage
                .revision_store()
                .publish_revision(*input)
                .map(PrivateArchiveReceipt::RevisionPublished)
        }
        (
            PrivateArchiveOperation::RecordRevisionGap,
            PrivateArchiveInput::RecordRevisionGap(input),
        ) => storage
            .permanent_gap_store()
            .record_revision_gap(input)
            .map(PrivateArchiveReceipt::RevisionGapRecorded),
        _ => Err(PrivateArchiveStorageError::Rejected),
    };

    // collapse operational details to the original private response contract
    match result {
        Ok(receipt) => PrivateArchiveResponse::ok(receipt),
        Err(PrivateArchiveStorageError::RevisionSpoolRefused) => PrivateArchiveResponse::error(
            PrivateArchiveStatus::TooManyRequests,
            PrivateArchiveErrorBody::RevisionSpoolFull,
        ),
        Err(PrivateArchiveStorageError::Rejected) => PrivateArchiveResponse::error(
            PrivateArchiveStatus::Conflict,
            PrivateArchiveErrorBody::RevisionArchiveRejected,
        ),
    }
}

/// match the Node listener's case-insensitive JSON media-class normalization.
fn is_json_content_type(value: Option<&str>) -> bool {
    value
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .eq_ignore_ascii_case("application/json")
}
