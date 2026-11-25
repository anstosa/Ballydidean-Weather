//! Worker HTTP/JSON client for the credential-free private maintenance API.

use weather_contracts::{
    AdjustmentMaintenanceGapInput, AdjustmentMaintenanceStageReceipt,
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionGapInput, AdjustmentRevisionGapReason,
    AdjustmentRevisionProjectionKind, AdjustmentRevisionStageReceipt,
    AdjustmentRevisionStageReceiptContractVersionLiteral,
    AdjustmentRevisionStageReceiptDurableLiteral, Nullable, RainFixedGaugeTargetGapInput,
    RainFixedGaugeTargetGapInputReasonLiteral, RainFixedGaugeTargetGapReceipt,
    RainFixedGaugeTargetGapStatus, RainFixedGaugeTargetGapStatusInput,
    RainMaintenanceControlStateGapInput, RainMaintenanceControlStateStageReceipt,
};
use weather_worker::{
    RainAdjustmentClient, RainAdjustmentCommitReceipt, RainAdjustmentGap, RainAdjustmentGapReason,
    RainAdjustmentProjection, RainAdjustmentStageReceipt, RainFixedGaugeClient,
    RainFixedGaugeCommitReceipt, RainFixedGaugeDurableGap, RainFixedGaugeGapReason,
    RainFixedGaugeGapStatus, RainFixedGaugeRevisionGap, RainFixedGaugeStageReceipt, TaskFailure,
};

use crate::worker_rain_fixed_gauge_wire::{outbound_target_gap, outbound_target_receipt};
use crate::{
    ApiMaintenanceRevisionBatchPublishBody, ApiMaintenanceRevisionPublishBody,
    ApiMaintenanceRevisionStageBody, WorkerRainShadowMembers, WorkerRainShadowPublicationClient,
};

const SHADOW_STAGE_PATH: &str = "/internal/adjustment-maintenance/shadow/stage";
const SHADOW_PUBLISH_PATH: &str = "/internal/adjustment-maintenance/shadow/publish";
const SHADOW_GAP_PATH: &str = "/internal/adjustment-maintenance/shadow/gap";
const REVISION_STAGE_PATH: &str = "/internal/adjustment-maintenance/revision/stage";
const REVISION_PUBLISH_PATH: &str = "/internal/adjustment-maintenance/revision/publish";
const REVISION_GAP_PATH: &str = "/internal/adjustment-maintenance/revision/gap";
const RAIN_CONTROL_STATE_STAGE_PATH: &str =
    "/internal/adjustment-maintenance/rain-control-state/stage";
const RAIN_CONTROL_STATE_GAP_PATH: &str = "/internal/adjustment-maintenance/rain-control-state/gap";
const FIXED_GAUGE_GAP_PATH: &str = "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap";
const FIXED_GAUGE_GAP_STATUS_PATH: &str =
    "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status";
const MAXIMUM_REQUEST_BYTES: usize = 2 * 1_024 * 1_024;
const ARCHIVE_STAGE_ATTEMPTS: usize = 7;
const ARCHIVE_STAGE_WAIT_MILLISECONDS: u64 = 5_000;

/// Private API origin admitted and canonicalized by the process URL owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedWorkerRainMaintenanceOrigin(String);

impl AdmittedWorkerRainMaintenanceOrigin {
    /// Borrow the canonical admitted origin for the injected transport.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Approved parser for the worker's distinct credential-free private API origin.
pub trait WorkerRainMaintenanceOriginOwner {
    type Error;

    /// Admit the exact Node origin policy and return its canonical representation.
    fn admit_worker_rain_maintenance_origin(&mut self, raw: &str) -> Result<String, Self::Error>;
}

/// Exact worker shadow-stage body, intentionally omitting API-derived metadata and hashes.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerRainShadowStageBody<'a> {
    pub bodyBase64: &'a str,
    pub comparatorBase64: &'a str,
    pub sourceProjectionBase64: &'a str,
}

/// Exact worker shadow-publication body in source insertion order.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkerRainShadowPublishBody<'a> {
    pub bodyBase64: &'a str,
    pub comparatorBase64: &'a str,
    pub predictionCommittedAt: &'a str,
    pub revisionReceipt: &'a AdjustmentRevisionCommitReceipt,
    pub sourceProjectionBase64: &'a str,
    pub stageReceipt: &'a AdjustmentMaintenanceStageReceipt,
}

/// Exact worker control-state stage body in source insertion order.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerRainControlStateStageBody<'a> {
    pub stateBase64: &'a str,
}

/// Closed worker request variants serialized only by the approved JSON owner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorkerRainMaintenanceRequest<'a> {
    ShadowStage(WorkerRainShadowStageBody<'a>),
    ShadowPublish(WorkerRainShadowPublishBody<'a>),
    ShadowGap(&'a AdjustmentMaintenanceGapInput),
    RevisionStage(ApiMaintenanceRevisionStageBody<'a>),
    RevisionPublish(ApiMaintenanceRevisionPublishBody<'a>),
    RevisionBatchPublish(ApiMaintenanceRevisionBatchPublishBody<'a>),
    RevisionGap(&'a AdjustmentRevisionGapInput),
    RainControlStateStage(WorkerRainControlStateStageBody<'a>),
    RainControlStateGap(&'a RainMaintenanceControlStateGapInput),
    FixedGaugeGapStatus(&'a RainFixedGaugeTargetGapStatusInput),
    FixedGaugeGap(&'a RainFixedGaugeTargetGapInput),
}

/// Successful response decoded only after the HTTP owner observes a 2xx status.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerRainMaintenanceResponse {
    ParsedJson,
    ShadowStage(AdjustmentMaintenanceStageReceipt),
    RevisionStage(AdjustmentRevisionStageReceipt),
    RainControlStateStage(RainMaintenanceControlStateStageReceipt),
    FixedGaugeGapStatus(RainFixedGaugeTargetGapStatus),
    FixedGaugeGap(RainFixedGaugeTargetGapReceipt),
}

/// Status-first transport outcome that cannot carry decoded JSON for a failed status.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerRainMaintenanceHttpOutcome {
    Success(WorkerRainMaintenanceResponse),
    HttpStatus(u16),
}

/// One bounded credential-free private POST.
pub struct WorkerRainMaintenancePost<'a> {
    pub origin: &'a AdmittedWorkerRainMaintenanceOrigin,
    pub path: &'static str,
    pub content_type: &'static str,
    pub maximum_body_bytes: usize,
    pub body: WorkerRainMaintenanceRequest<'a>,
}

/// Approved base64, bounded JSON, response decoding, and private HTTP owner.
pub trait WorkerRainMaintenanceHttpJson {
    type Error;

    /// Encode exact raw bytes using the shared standard base64 owner.
    fn base64(&mut self, bytes: &[u8]) -> Result<String, Self::Error>;

    /// Serialize and POST once, returning status before attempting successful JSON decoding.
    fn post_json(
        &mut self,
        post: WorkerRainMaintenancePost<'_>,
    ) -> Result<WorkerRainMaintenanceHttpOutcome, Self::Error>;

    /// Wait once before retrying only an explicit archive-spool capacity refusal.
    fn wait_for_revision_archive_capacity(&mut self, milliseconds: u64) -> Result<(), Self::Error>;
}

/// Worker client for staging and abandoning one root-selected rain control state.
pub trait WorkerRainControlArchiveClient {
    /// Stage exact canonical state bytes through the private API with bounded capacity retries.
    fn stage_rain_control_state(
        &mut self,
        bytes: &[u8],
    ) -> Result<RainMaintenanceControlStateStageReceipt, TaskFailure>;

    /// Record one exact abandoned state without retrying any response.
    fn record_rain_control_state_gap(
        &mut self,
        input: &RainMaintenanceControlStateGapInput,
    ) -> Result<(), TaskFailure>;
}

/// Byte-oriented revision archive client shared by weather and temperature workers.
pub trait WorkerRevisionArchiveClient {
    /// Stage exact canonical projection bytes with bounded spool backpressure.
    fn stage_revision(
        &mut self,
        bytes: &[u8],
    ) -> Result<AdjustmentRevisionStageReceipt, TaskFailure>;

    /// Publish one database-bound revision only after the serving commit.
    fn publish_revision(
        &mut self,
        bytes: &[u8],
        stage: &AdjustmentRevisionStageReceipt,
        commit: &AdjustmentRevisionCommitReceipt,
    ) -> Result<(), TaskFailure>;

    /// Publish one ordered database-bound revision group after the serving commit.
    fn publish_revision_batch(
        &mut self,
        bytes: &[u8],
        stage: &AdjustmentRevisionStageReceipt,
        commits: &[AdjustmentRevisionCommitReceipt],
    ) -> Result<(), TaskFailure>;

    /// Relay one permanent value-free revision disposition without capacity retries.
    fn record_revision_gap(&mut self, gap: &AdjustmentRevisionGapInput) -> Result<(), TaskFailure>;
}

/// Retained admitted private origin and injected HTTP/JSON/base64 owner.
pub struct HttpWorkerRainMaintenanceClient<Owner> {
    origin: AdmittedWorkerRainMaintenanceOrigin,
    owner: Owner,
}

impl<Owner> HttpWorkerRainMaintenanceClient<Owner> {
    /// Admit the raw private origin before retaining the HTTP owner.
    pub fn admit<Origin>(
        raw_origin: &str,
        origin_owner: &mut Origin,
        owner: Owner,
    ) -> Result<Self, Origin::Error>
    where
        Origin: WorkerRainMaintenanceOriginOwner,
    {
        let origin = origin_owner.admit_worker_rain_maintenance_origin(raw_origin)?;
        Ok(Self {
            origin: AdmittedWorkerRainMaintenanceOrigin(origin),
            owner,
        })
    }

    /// Return the HTTP owner after worker shutdown or fixture completion.
    pub fn into_owner(self) -> Owner {
        self.owner
    }
}

impl<Owner: WorkerRainMaintenanceHttpJson> HttpWorkerRainMaintenanceClient<Owner> {
    /// POST one typed request and preserve Node's status classification.
    fn post(
        &mut self,
        path: &'static str,
        body: WorkerRainMaintenanceRequest<'_>,
    ) -> Result<WorkerRainMaintenanceResponse, TaskFailure> {
        let outcome = self
            .owner
            .post_json(WorkerRainMaintenancePost {
                origin: &self.origin,
                path,
                content_type: "application/json",
                maximum_body_bytes: MAXIMUM_REQUEST_BYTES,
                body,
            })
            .map_err(|_| maintenance_failure())?;
        // retain only the original revision spool capacity classification
        match outcome {
            WorkerRainMaintenanceHttpOutcome::Success(response) => Ok(response),
            WorkerRainMaintenanceHttpOutcome::HttpStatus(429) => Err(TaskFailure {
                attempts: 1,
                code: "adjustment_revision_spool_refused".to_owned(),
            }),
            WorkerRainMaintenanceHttpOutcome::HttpStatus(_) => Err(maintenance_failure()),
        }
    }

    /// Require a successful parsed JSON response without inspecting its value.
    fn post_value_free(
        &mut self,
        path: &'static str,
        body: WorkerRainMaintenanceRequest<'_>,
    ) -> Result<(), TaskFailure> {
        // every original value-free call still consumes one successful JSON document
        match self.post(path, body)? {
            WorkerRainMaintenanceResponse::ParsedJson => Ok(()),
            _ => Err(maintenance_failure()),
        }
    }

    /// Share the exact seven-attempt bounded-capacity policy across staged archive payloads.
    fn post_with_archive_backpressure(
        &mut self,
        path: &'static str,
        body: WorkerRainMaintenanceRequest<'_>,
    ) -> Result<WorkerRainMaintenanceResponse, TaskFailure> {
        // bound six waits to thirty seconds while retrying only explicit spool capacity
        for attempt in 0..ARCHIVE_STAGE_ATTEMPTS {
            match self.post(path, body) {
                Ok(response) => return Ok(response),
                Err(error)
                    if error.code == "adjustment_revision_spool_refused"
                        && attempt + 1 < ARCHIVE_STAGE_ATTEMPTS =>
                {
                    self.owner
                        .wait_for_revision_archive_capacity(ARCHIVE_STAGE_WAIT_MILLISECONDS)
                        .map_err(|_| maintenance_failure())?;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("bounded archive stage loop always returns")
    }

    /// Encode the three exact shadow members without deriving API-owned fields.
    fn encode_shadow_members(
        &mut self,
        members: WorkerRainShadowMembers<'_>,
    ) -> Result<(String, String, String), TaskFailure> {
        let body = self
            .owner
            .base64(members.body)
            .map_err(|_| maintenance_failure())?;
        let comparator = self
            .owner
            .base64(members.comparator)
            .map_err(|_| maintenance_failure())?;
        let source = self
            .owner
            .base64(members.source)
            .map_err(|_| maintenance_failure())?;
        Ok((body, comparator, source))
    }
}

impl<Owner: WorkerRainMaintenanceHttpJson> WorkerRainControlArchiveClient
    for HttpWorkerRainMaintenanceClient<Owner>
{
    /// Stage one exact root-selected state before control scoring.
    fn stage_rain_control_state(
        &mut self,
        bytes: &[u8],
    ) -> Result<RainMaintenanceControlStateStageReceipt, TaskFailure> {
        let state_base64 = self
            .owner
            .base64(bytes)
            .map_err(|_| maintenance_failure())?;
        let response = self.post_with_archive_backpressure(
            RAIN_CONTROL_STATE_STAGE_PATH,
            WorkerRainMaintenanceRequest::RainControlStateStage(WorkerRainControlStateStageBody {
                stateBase64: &state_base64,
            }),
        )?;
        // require the complete generated state stage receipt response
        match response {
            WorkerRainMaintenanceResponse::RainControlStateStage(receipt) => Ok(receipt),
            _ => Err(maintenance_failure()),
        }
    }

    /// Relay one value-free abandoned state without capacity retries.
    fn record_rain_control_state_gap(
        &mut self,
        input: &RainMaintenanceControlStateGapInput,
    ) -> Result<(), TaskFailure> {
        self.post_value_free(
            RAIN_CONTROL_STATE_GAP_PATH,
            WorkerRainMaintenanceRequest::RainControlStateGap(input),
        )
    }
}

impl<Owner: WorkerRainMaintenanceHttpJson> WorkerRevisionArchiveClient
    for HttpWorkerRainMaintenanceClient<Owner>
{
    /// Stage one canonical projection through the existing bounded revision route.
    fn stage_revision(
        &mut self,
        bytes: &[u8],
    ) -> Result<AdjustmentRevisionStageReceipt, TaskFailure> {
        let projection = self
            .owner
            .base64(bytes)
            .map_err(|_| maintenance_failure())?;
        let response = self.post_with_archive_backpressure(
            REVISION_STAGE_PATH,
            WorkerRainMaintenanceRequest::RevisionStage(ApiMaintenanceRevisionStageBody {
                projectionBase64: &projection,
            }),
        )?;
        // leave body-identity and projection-kind admission with the typed caller
        match response {
            WorkerRainMaintenanceResponse::RevisionStage(receipt) => Ok(receipt),
            _ => Err(maintenance_failure()),
        }
    }

    /// Publish one exact projection, stage acknowledgement, and database receipt.
    fn publish_revision(
        &mut self,
        bytes: &[u8],
        stage: &AdjustmentRevisionStageReceipt,
        commit: &AdjustmentRevisionCommitReceipt,
    ) -> Result<(), TaskFailure> {
        let projection = self
            .owner
            .base64(bytes)
            .map_err(|_| maintenance_failure())?;
        self.post_value_free(
            REVISION_PUBLISH_PATH,
            WorkerRainMaintenanceRequest::RevisionPublish(ApiMaintenanceRevisionPublishBody {
                projectionBase64: &projection,
                revisionReceipt: commit,
                stageReceipt: stage,
            }),
        )
    }

    /// Publish one exact ordered receipt vector without rebuilding its DTOs.
    fn publish_revision_batch(
        &mut self,
        bytes: &[u8],
        stage: &AdjustmentRevisionStageReceipt,
        commits: &[AdjustmentRevisionCommitReceipt],
    ) -> Result<(), TaskFailure> {
        let projection = self
            .owner
            .base64(bytes)
            .map_err(|_| maintenance_failure())?;
        self.post_value_free(
            REVISION_PUBLISH_PATH,
            WorkerRainMaintenanceRequest::RevisionBatchPublish(
                ApiMaintenanceRevisionBatchPublishBody {
                    projectionBase64: &projection,
                    revisionReceipts: commits,
                    stageReceipt: stage,
                },
            ),
        )
    }

    /// Persist one exact generated gap through the existing value-free route.
    fn record_revision_gap(&mut self, gap: &AdjustmentRevisionGapInput) -> Result<(), TaskFailure> {
        self.post_value_free(
            REVISION_GAP_PATH,
            WorkerRainMaintenanceRequest::RevisionGap(gap),
        )
    }
}

impl<Owner: WorkerRainMaintenanceHttpJson> WorkerRainShadowPublicationClient
    for HttpWorkerRainMaintenanceClient<Owner>
{
    type Error = TaskFailure;

    /// Stage exact source, prediction, and comparator bytes through the private API.
    fn stage_shadow(
        &mut self,
        members: WorkerRainShadowMembers<'_>,
    ) -> Result<AdjustmentMaintenanceStageReceipt, Self::Error> {
        let (body, comparator, source) = self.encode_shadow_members(members)?;
        let request = WorkerRainShadowStageBody {
            bodyBase64: &body,
            comparatorBase64: &comparator,
            sourceProjectionBase64: &source,
        };
        // require the complete generated stage receipt response
        match self.post(
            SHADOW_STAGE_PATH,
            WorkerRainMaintenanceRequest::ShadowStage(request),
        )? {
            WorkerRainMaintenanceResponse::ShadowStage(receipt) => Ok(receipt),
            _ => Err(maintenance_failure()),
        }
    }

    /// Publish the exact staged members and authoritative database receipt.
    fn publish_shadow(
        &mut self,
        members: WorkerRainShadowMembers<'_>,
        committed_at: &str,
        stage: &AdjustmentMaintenanceStageReceipt,
        revision: &AdjustmentRevisionCommitReceipt,
    ) -> Result<(), Self::Error> {
        let (body, comparator, source) = self.encode_shadow_members(members)?;
        self.post_value_free(
            SHADOW_PUBLISH_PATH,
            WorkerRainMaintenanceRequest::ShadowPublish(WorkerRainShadowPublishBody {
                bodyBase64: &body,
                comparatorBase64: &comparator,
                predictionCommittedAt: committed_at,
                revisionReceipt: revision,
                sourceProjectionBase64: &source,
                stageReceipt: stage,
            }),
        )
    }

    /// Relay one categorical shadow gap without causal source values.
    fn record_shadow_gap(
        &mut self,
        gap: &AdjustmentMaintenanceGapInput,
    ) -> Result<(), Self::Error> {
        self.post_value_free(
            SHADOW_GAP_PATH,
            WorkerRainMaintenanceRequest::ShadowGap(gap),
        )
    }
}

impl<Owner: WorkerRainMaintenanceHttpJson> RainAdjustmentClient
    for HttpWorkerRainMaintenanceClient<Owner>
{
    /// Stage one canonical rain revision projection before database binding.
    fn stage_revision(
        &mut self,
        projection: &RainAdjustmentProjection,
    ) -> Result<RainAdjustmentStageReceipt, TaskFailure> {
        WorkerRevisionArchiveClient::stage_revision(self, &projection.bytes)
    }

    /// Publish one bound revision only after the serving transaction commits.
    fn publish_revision(
        &mut self,
        projection: &RainAdjustmentProjection,
        stage_receipt: &RainAdjustmentStageReceipt,
        commit_receipt: &RainAdjustmentCommitReceipt,
    ) -> Result<(), TaskFailure> {
        WorkerRevisionArchiveClient::publish_revision(
            self,
            &projection.bytes,
            stage_receipt,
            commit_receipt,
        )
    }

    /// Relay one permanent value-free rain revision gap.
    fn record_revision_gap(&mut self, gap: &RainAdjustmentGap) -> Result<(), TaskFailure> {
        let request = AdjustmentRevisionGapInput {
            logicalKeySha256: gap.logical_key_sha256.clone(),
            projectionIdentitySha256: nullable(gap.projection_identity_sha256.as_ref()),
            projectionKind: AdjustmentRevisionProjectionKind::RainGateInput,
            projectionSha256: nullable(gap.projection_sha256.as_ref()),
            reason: revision_gap_reason(gap.reason),
        };
        WorkerRevisionArchiveClient::record_revision_gap(self, &request)
    }
}

impl<Owner: WorkerRainMaintenanceHttpJson> RainFixedGaugeClient
    for HttpWorkerRainMaintenanceClient<Owner>
{
    /// Read the permanent oversized disposition before touching raw capture bodies.
    fn read_oversized_gap(
        &mut self,
        logical_hour_at: &str,
        logical_key_sha256: &str,
    ) -> Result<RainFixedGaugeGapStatus, TaskFailure> {
        let request = RainFixedGaugeTargetGapStatusInput {
            logicalHourAt: logical_hour_at.to_owned(),
            logicalKeySha256: logical_key_sha256.to_owned(),
        };
        match self.post(
            FIXED_GAUGE_GAP_STATUS_PATH,
            WorkerRainMaintenanceRequest::FixedGaugeGapStatus(&request),
        )? {
            WorkerRainMaintenanceResponse::FixedGaugeGapStatus(
                RainFixedGaugeTargetGapStatus::Absent(_),
            ) => Ok(RainFixedGaugeGapStatus::Absent),
            WorkerRainMaintenanceResponse::FixedGaugeGapStatus(
                RainFixedGaugeTargetGapStatus::Present(value),
            ) => {
                let gap = value.gap;
                Ok(RainFixedGaugeGapStatus::Present(RainFixedGaugeDurableGap {
                    gap_at: gap.gapAt,
                    gap_sha256: gap.gapSha256,
                    logical_hour_at: gap.logicalHourAt,
                    logical_key_sha256: gap.logicalKeySha256,
                    reason: RainFixedGaugeGapReason::TargetSourceOversized,
                }))
            }
            _ => Err(maintenance_failure()),
        }
    }

    /// Persist one value-free oversized disposition while retaining its durable receipt.
    fn record_oversized_gap(
        &mut self,
        logical_hour_at: &str,
        logical_key_sha256: &str,
    ) -> Result<RainFixedGaugeDurableGap, TaskFailure> {
        let request = RainFixedGaugeTargetGapInput {
            logicalHourAt: logical_hour_at.to_owned(),
            logicalKeySha256: logical_key_sha256.to_owned(),
            reason: RainFixedGaugeTargetGapInputReasonLiteral,
        };
        match self.post(
            FIXED_GAUGE_GAP_PATH,
            WorkerRainMaintenanceRequest::FixedGaugeGap(&request),
        )? {
            WorkerRainMaintenanceResponse::FixedGaugeGap(gap) => Ok(RainFixedGaugeDurableGap {
                gap_at: gap.gapAt,
                gap_sha256: gap.gapSha256,
                logical_hour_at: gap.logicalHourAt,
                logical_key_sha256: gap.logicalKeySha256,
                reason: RainFixedGaugeGapReason::TargetSourceOversized,
            }),
            _ => Err(maintenance_failure()),
        }
    }

    /// Bound retries and preserve the original target projection receipt kind.
    fn stage_revision(&mut self, bytes: &[u8]) -> Result<RainFixedGaugeStageReceipt, TaskFailure> {
        let response = WorkerRevisionArchiveClient::stage_revision(self, bytes)?;
        match response {
            value if value.projectionKind == AdjustmentRevisionProjectionKind::TargetRevision => {
                Ok(RainFixedGaugeStageReceipt {
                    durable: true,
                    durable_at: value.durableAt,
                    projection_identity_sha256: value.projectionIdentitySha256,
                    projection_sha256: value.projectionSha256,
                    stage_receipt_sha256: value.stageReceiptSha256,
                })
            }
            _ => Err(maintenance_failure()),
        }
    }

    /// Relay the database's retained whole-group disposition without a provider body.
    fn record_revision_gap(&mut self, gap: &RainFixedGaugeRevisionGap) -> Result<(), TaskFailure> {
        let wire = outbound_target_gap(gap).ok_or_else(maintenance_failure)?;
        WorkerRevisionArchiveClient::record_revision_gap(self, &wire)
    }

    /// Publish the original ordered twelve-receipt vector in one private request.
    fn publish_revision_batch(
        &mut self,
        bytes: &[u8],
        stage: &RainFixedGaugeStageReceipt,
        receipts: &[RainFixedGaugeCommitReceipt],
    ) -> Result<(), TaskFailure> {
        // a grouped publication cannot fabricate receipt durability
        if !stage.durable {
            return Err(maintenance_failure());
        }
        let stage = AdjustmentRevisionStageReceipt {
            contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
            durable: AdjustmentRevisionStageReceiptDurableLiteral,
            durableAt: stage.durable_at.clone(),
            projectionIdentitySha256: stage.projection_identity_sha256.clone(),
            projectionKind: AdjustmentRevisionProjectionKind::TargetRevision,
            projectionSha256: stage.projection_sha256.clone(),
            stageReceiptSha256: stage.stage_receipt_sha256.clone(),
        };
        let receipts = receipts
            .iter()
            .map(outbound_target_receipt)
            .collect::<Vec<_>>();
        WorkerRevisionArchiveClient::publish_revision_batch(self, bytes, &stage, &receipts)
    }
}

/// Project one optional borrowed string into an owned generated nullable field.
fn nullable(value: Option<&String>) -> Nullable<String> {
    value.map_or(Nullable::Null, |value| Nullable::Value(value.clone()))
}

/// Map the worker's closed gap category to the generated private wire category.
const fn revision_gap_reason(value: RainAdjustmentGapReason) -> AdjustmentRevisionGapReason {
    match value {
        RainAdjustmentGapReason::ArchiveStageFailed => {
            AdjustmentRevisionGapReason::ArchiveStageFailed
        }
        RainAdjustmentGapReason::DatabaseBindFailed => {
            AdjustmentRevisionGapReason::DatabaseBindFailed
        }
        RainAdjustmentGapReason::ArchivePublishFailed => {
            AdjustmentRevisionGapReason::ArchivePublishFailed
        }
    }
}

/// Build the original stable private maintenance failure.
fn maintenance_failure() -> TaskFailure {
    TaskFailure {
        attempts: 1,
        code: "rain maintenance api failed".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use weather_contracts::{
        AdjustmentMaintenanceGapInputFamilyEnum,
        AdjustmentMaintenanceStageReceiptContractVersionEnum,
        AdjustmentMaintenanceStageReceiptDurableLiteral,
        AdjustmentRevisionCommitReceiptContractVersionLiteral,
        AdjustmentRevisionCommitReceiptProjectionKindEnum,
        AdjustmentRevisionStageReceiptContractVersionLiteral,
        AdjustmentRevisionStageReceiptDurableLiteral, FieldPresence, MaintenanceShadowGapReason,
        RainMaintenanceControlStateGapInputReasonLiteral,
        RainMaintenanceControlStateStageReceiptContractVersionLiteral,
        RainMaintenanceControlStateStageReceiptDurableLiteral,
    };

    use super::*;

    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    struct Origin;

    impl WorkerRainMaintenanceOriginOwner for Origin {
        type Error = ();

        /// Admit only the fixture's original private API origin.
        fn admit_worker_rain_maintenance_origin(
            &mut self,
            raw: &str,
        ) -> Result<String, Self::Error> {
            assert_eq!(raw, "http://api");
            Ok("http://api/".to_owned())
        }
    }

    #[derive(Default)]
    struct Owner {
        paths: Vec<&'static str>,
        revision_stage_statuses: Vec<u16>,
        control_stage_statuses: Vec<u16>,
        control_gap_status: Option<u16>,
        shadow_stage_status: Option<u16>,
        waits: Vec<u64>,
    }

    impl WorkerRainMaintenanceHttpJson for Owner {
        type Error = ();

        /// Encode fixture bytes while retaining their member identity visibly.
        fn base64(&mut self, bytes: &[u8]) -> Result<String, Self::Error> {
            Ok(format!("base64:{}", String::from_utf8_lossy(bytes)))
        }

        /// Verify exact paths and bodies before returning their typed successful responses.
        fn post_json(
            &mut self,
            post: WorkerRainMaintenancePost<'_>,
        ) -> Result<WorkerRainMaintenanceHttpOutcome, Self::Error> {
            assert_eq!(post.origin.as_str(), "http://api/");
            assert_eq!(post.content_type, "application/json");
            assert_eq!(post.maximum_body_bytes, MAXIMUM_REQUEST_BYTES);
            self.paths.push(post.path);
            // expose one configured shadow status without permitting client retries
            if matches!(post.body, WorkerRainMaintenanceRequest::ShadowStage(_))
                && let Some(status) = self.shadow_stage_status.take()
            {
                return Ok(WorkerRainMaintenanceHttpOutcome::HttpStatus(status));
            }
            // consume configured revision-stage statuses in request order
            if matches!(post.body, WorkerRainMaintenanceRequest::RevisionStage(_))
                && !self.revision_stage_statuses.is_empty()
            {
                let status = self.revision_stage_statuses.remove(0);
                return Ok(WorkerRainMaintenanceHttpOutcome::HttpStatus(status));
            }
            // consume configured control-state stage statuses in request order
            if matches!(
                post.body,
                WorkerRainMaintenanceRequest::RainControlStateStage(_)
            ) && !self.control_stage_statuses.is_empty()
            {
                let status = self.control_stage_statuses.remove(0);
                return Ok(WorkerRainMaintenanceHttpOutcome::HttpStatus(status));
            }
            // expose one configured control gap status without permitting retries
            if matches!(
                post.body,
                WorkerRainMaintenanceRequest::RainControlStateGap(_)
            ) && let Some(status) = self.control_gap_status.take()
            {
                return Ok(WorkerRainMaintenanceHttpOutcome::HttpStatus(status));
            }
            let response = match post.body {
                WorkerRainMaintenanceRequest::ShadowStage(body) => {
                    assert_eq!(body.bodyBase64, "base64:body");
                    assert_eq!(body.comparatorBase64, "base64:comparator");
                    assert_eq!(body.sourceProjectionBase64, "base64:source");
                    WorkerRainMaintenanceResponse::ShadowStage(shadow_stage_receipt())
                }
                WorkerRainMaintenanceRequest::ShadowPublish(body) => {
                    assert_eq!(body.bodyBase64, "base64:body");
                    assert_eq!(body.comparatorBase64, "base64:comparator");
                    assert_eq!(body.sourceProjectionBase64, "base64:source");
                    assert_eq!(body.predictionCommittedAt, "committed-at");
                    WorkerRainMaintenanceResponse::ParsedJson
                }
                WorkerRainMaintenanceRequest::ShadowGap(gap) => {
                    assert_eq!(gap.family, AdjustmentMaintenanceGapInputFamilyEnum::Rain);
                    WorkerRainMaintenanceResponse::ParsedJson
                }
                WorkerRainMaintenanceRequest::RevisionStage(body) => {
                    assert_eq!(body.projectionBase64, "base64:projection");
                    WorkerRainMaintenanceResponse::RevisionStage(revision_stage_receipt())
                }
                WorkerRainMaintenanceRequest::RevisionPublish(body) => {
                    assert_eq!(body.projectionBase64, "base64:projection");
                    assert_eq!(body.revisionReceipt, &commit_receipt());
                    WorkerRainMaintenanceResponse::ParsedJson
                }
                WorkerRainMaintenanceRequest::RevisionBatchPublish(body) => {
                    assert_eq!(body.projectionBase64, "base64:projection");
                    assert_eq!(body.revisionReceipts.len(), 1);
                    assert_eq!(body.revisionReceipts[0], commit_receipt());
                    assert_eq!(body.stageReceipt, &revision_stage_receipt());
                    WorkerRainMaintenanceResponse::ParsedJson
                }
                WorkerRainMaintenanceRequest::RevisionGap(gap) => {
                    assert_eq!(
                        gap.projectionKind,
                        AdjustmentRevisionProjectionKind::RainGateInput
                    );
                    assert_eq!(gap.reason, AdjustmentRevisionGapReason::DatabaseBindFailed);
                    WorkerRainMaintenanceResponse::ParsedJson
                }
                WorkerRainMaintenanceRequest::RainControlStateStage(body) => {
                    assert_eq!(body.stateBase64, "base64:control-state");
                    WorkerRainMaintenanceResponse::RainControlStateStage(
                        control_state_stage_receipt(),
                    )
                }
                WorkerRainMaintenanceRequest::RainControlStateGap(gap) => {
                    assert_eq!(gap.reason, RainMaintenanceControlStateGapInputReasonLiteral);
                    assert_eq!(gap.stateSha256, HASH_A);
                    assert_eq!(gap.stageReceiptSha256, HASH_B);
                    WorkerRainMaintenanceResponse::ParsedJson
                }
                WorkerRainMaintenanceRequest::FixedGaugeGap(_)
                | WorkerRainMaintenanceRequest::FixedGaugeGapStatus(_) => {
                    unreachable!("fixed gauge requests have their own fixture")
                }
            };
            Ok(WorkerRainMaintenanceHttpOutcome::Success(response))
        }

        /// Record deterministic capacity waits without sleeping the fixture.
        fn wait_for_revision_archive_capacity(
            &mut self,
            milliseconds: u64,
        ) -> Result<(), Self::Error> {
            self.waits.push(milliseconds);
            Ok(())
        }
    }

    /// Build one complete generated shadow stage receipt.
    fn shadow_stage_receipt() -> AdjustmentMaintenanceStageReceipt {
        AdjustmentMaintenanceStageReceipt {
            comparatorSha256: FieldPresence::Value(HASH_B.to_owned()),
            contractVersion:
                AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV2,
            durable: AdjustmentMaintenanceStageReceiptDurableLiteral,
            durableAt: "2026-11-24T00:00:00.000Z".to_owned(),
            dueKey: "capture/2026-11-24T00:35:00.000Z".to_owned(),
            predictionBodySha256: HASH_A.to_owned(),
            registrationSha256: HASH_B.to_owned(),
            sourceProjectionSha256: HASH_A.to_owned(),
            stageReceiptSha256: HASH_B.to_owned(),
        }
    }

    /// Build one complete generated revision stage receipt.
    fn revision_stage_receipt() -> AdjustmentRevisionStageReceipt {
        AdjustmentRevisionStageReceipt {
            contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
            durable: AdjustmentRevisionStageReceiptDurableLiteral,
            durableAt: "2026-11-24T00:00:00.000Z".to_owned(),
            projectionIdentitySha256: HASH_A.to_owned(),
            projectionKind: AdjustmentRevisionProjectionKind::RainGateInput,
            projectionSha256: HASH_A.to_owned(),
            stageReceiptSha256: HASH_B.to_owned(),
        }
    }

    /// Build one complete generated control-state stage receipt.
    fn control_state_stage_receipt() -> RainMaintenanceControlStateStageReceipt {
        RainMaintenanceControlStateStageReceipt {
            contractVersion: RainMaintenanceControlStateStageReceiptContractVersionLiteral,
            durable: RainMaintenanceControlStateStageReceiptDurableLiteral,
            durableAt: "2026-11-24T00:00:00.000Z".to_owned(),
            stateSha256: HASH_A.to_owned(),
            stageReceiptSha256: HASH_B.to_owned(),
        }
    }

    /// Build one complete database revision receipt.
    fn commit_receipt() -> AdjustmentRevisionCommitReceipt {
        AdjustmentRevisionCommitReceipt {
            archiveCommitOrdinal: "1".to_owned(),
            archiveCommittedAt: "2026-11-24T00:01:00.000Z".to_owned(),
            contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
            frontierSha256: HASH_A.to_owned(),
            predecessorFrontierSha256: HASH_B.to_owned(),
            projectionIdentitySha256: HASH_A.to_owned(),
            projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput,
            projectionSha256: HASH_A.to_owned(),
            receiptSha256: HASH_B.to_owned(),
            stageReceiptSha256: HASH_B.to_owned(),
        }
    }

    /// Exercise the original stage, publication, gap, and control-state route shapes.
    #[test]
    fn sends_exact_shadow_and_single_revision_routes() {
        let mut client =
            HttpWorkerRainMaintenanceClient::admit("http://api", &mut Origin, Owner::default())
                .expect("admitted origin");
        let members = WorkerRainShadowMembers {
            source: b"source",
            body: b"body",
            comparator: b"comparator",
        };
        let shadow_stage = client.stage_shadow(members).expect("shadow stage");
        client
            .publish_shadow(members, "committed-at", &shadow_stage, &commit_receipt())
            .expect("shadow publish");
        client
            .record_shadow_gap(&AdjustmentMaintenanceGapInput {
                dueKey: "capture/2026-11-24T00:35:00.000Z".to_owned(),
                family: AdjustmentMaintenanceGapInputFamilyEnum::Rain,
                reason: MaintenanceShadowGapReason::ArchiveStageFailed,
                registrationSha256: HASH_A.to_owned(),
            })
            .expect("shadow gap");

        let projection = RainAdjustmentProjection {
            bytes: b"projection".to_vec(),
            logical_key_sha256: HASH_B.to_owned(),
            projection_identity_sha256: HASH_A.to_owned(),
        };
        let revision_stage =
            RainAdjustmentClient::stage_revision(&mut client, &projection).expect("revision stage");
        RainAdjustmentClient::publish_revision(
            &mut client,
            &projection,
            &revision_stage,
            &commit_receipt(),
        )
        .expect("revision publish");
        RainAdjustmentClient::record_revision_gap(
            &mut client,
            &RainAdjustmentGap {
                logical_key_sha256: HASH_B.to_owned(),
                projection_identity_sha256: Some(HASH_A.to_owned()),
                projection_sha256: Some(HASH_A.to_owned()),
                reason: RainAdjustmentGapReason::DatabaseBindFailed,
            },
        )
        .expect("revision gap");
        let control_stage = client
            .stage_rain_control_state(b"control-state")
            .expect("control state stage");
        client
            .record_rain_control_state_gap(&RainMaintenanceControlStateGapInput {
                reason: RainMaintenanceControlStateGapInputReasonLiteral,
                stateSha256: control_stage.stateSha256,
                stageReceiptSha256: control_stage.stageReceiptSha256,
            })
            .expect("control state gap");

        assert_eq!(
            client.into_owner().paths,
            [
                SHADOW_STAGE_PATH,
                SHADOW_PUBLISH_PATH,
                SHADOW_GAP_PATH,
                REVISION_STAGE_PATH,
                REVISION_PUBLISH_PATH,
                REVISION_GAP_PATH,
                RAIN_CONTROL_STATE_STAGE_PATH,
                RAIN_CONTROL_STATE_GAP_PATH,
            ]
        );
    }

    /// Send the shared single, grouped, and value-free revision request shapes unchanged.
    #[test]
    fn sends_exact_shared_revision_archive_routes() {
        let mut client =
            HttpWorkerRainMaintenanceClient::admit("http://api", &mut Origin, Owner::default())
                .expect("admitted origin");
        let stage = WorkerRevisionArchiveClient::stage_revision(&mut client, b"projection")
            .expect("revision stage");
        WorkerRevisionArchiveClient::publish_revision(
            &mut client,
            b"projection",
            &stage,
            &commit_receipt(),
        )
        .expect("single publication");
        WorkerRevisionArchiveClient::publish_revision_batch(
            &mut client,
            b"projection",
            &stage,
            &[commit_receipt()],
        )
        .expect("batch publication");
        WorkerRevisionArchiveClient::record_revision_gap(
            &mut client,
            &AdjustmentRevisionGapInput {
                logicalKeySha256: HASH_B.to_owned(),
                projectionIdentitySha256: Nullable::Value(HASH_A.to_owned()),
                projectionKind: AdjustmentRevisionProjectionKind::RainGateInput,
                projectionSha256: Nullable::Value(HASH_A.to_owned()),
                reason: AdjustmentRevisionGapReason::DatabaseBindFailed,
            },
        )
        .expect("revision gap");

        assert_eq!(
            client.into_owner().paths,
            [
                REVISION_STAGE_PATH,
                REVISION_PUBLISH_PATH,
                REVISION_PUBLISH_PATH,
                REVISION_GAP_PATH,
            ]
        );
    }

    /// Retry revision staging seven times with exactly six five-second pacing calls.
    #[test]
    fn bounds_revision_stage_backpressure_retries() {
        let mut client = HttpWorkerRainMaintenanceClient::admit(
            "http://api",
            &mut Origin,
            Owner {
                revision_stage_statuses: vec![429; ARCHIVE_STAGE_ATTEMPTS],
                ..Owner::default()
            },
        )
        .expect("admitted origin");
        let result = WorkerRevisionArchiveClient::stage_revision(&mut client, b"projection");

        assert_eq!(
            result,
            Err(TaskFailure {
                attempts: 1,
                code: "adjustment_revision_spool_refused".to_owned(),
            })
        );
        let owner = client.into_owner();
        assert_eq!(
            owner.paths,
            vec![REVISION_STAGE_PATH; ARCHIVE_STAGE_ATTEMPTS]
        );
        assert_eq!(
            owner.waits,
            vec![ARCHIVE_STAGE_WAIT_MILLISECONDS; ARCHIVE_STAGE_ATTEMPTS - 1]
        );
    }

    /// Apply the same seven attempts and six waits to control-state staging only.
    #[test]
    fn bounds_control_state_stage_backpressure_retries() {
        let mut client = HttpWorkerRainMaintenanceClient::admit(
            "http://api",
            &mut Origin,
            Owner {
                control_stage_statuses: vec![429; ARCHIVE_STAGE_ATTEMPTS],
                ..Owner::default()
            },
        )
        .expect("admitted origin");
        let result = client.stage_rain_control_state(b"control-state");

        assert_eq!(
            result,
            Err(TaskFailure {
                attempts: 1,
                code: "adjustment_revision_spool_refused".to_owned(),
            })
        );
        let owner = client.into_owner();
        assert_eq!(
            owner.paths,
            vec![RAIN_CONTROL_STATE_STAGE_PATH; ARCHIVE_STAGE_ATTEMPTS]
        );
        assert_eq!(
            owner.waits,
            vec![ARCHIVE_STAGE_WAIT_MILLISECONDS; ARCHIVE_STAGE_ATTEMPTS - 1]
        );
    }

    /// Never retry another status, a shadow-stage response, or a control gap.
    #[test]
    fn does_not_retry_noncapacity_or_shadow_stage_failures() {
        let mut revision = HttpWorkerRainMaintenanceClient::admit(
            "http://api",
            &mut Origin,
            Owner {
                revision_stage_statuses: vec![500, 200],
                ..Owner::default()
            },
        )
        .expect("admitted origin");
        assert!(
            RainAdjustmentClient::stage_revision(
                &mut revision,
                &RainAdjustmentProjection {
                    bytes: b"projection".to_vec(),
                    logical_key_sha256: HASH_B.to_owned(),
                    projection_identity_sha256: HASH_A.to_owned(),
                }
            )
            .is_err()
        );
        let revision = revision.into_owner();
        assert_eq!(revision.paths, [REVISION_STAGE_PATH]);
        assert!(revision.waits.is_empty());

        let mut shadow = HttpWorkerRainMaintenanceClient::admit(
            "http://api",
            &mut Origin,
            Owner {
                shadow_stage_status: Some(429),
                ..Owner::default()
            },
        )
        .expect("admitted origin");
        assert!(
            shadow
                .stage_shadow(WorkerRainShadowMembers {
                    source: b"source",
                    body: b"body",
                    comparator: b"comparator",
                })
                .is_err()
        );
        let shadow = shadow.into_owner();
        assert_eq!(shadow.paths, [SHADOW_STAGE_PATH]);
        assert!(shadow.waits.is_empty());

        let mut gap = HttpWorkerRainMaintenanceClient::admit(
            "http://api",
            &mut Origin,
            Owner {
                control_gap_status: Some(429),
                ..Owner::default()
            },
        )
        .expect("admitted origin");
        assert!(
            gap.record_rain_control_state_gap(&RainMaintenanceControlStateGapInput {
                reason: RainMaintenanceControlStateGapInputReasonLiteral,
                stateSha256: HASH_A.to_owned(),
                stageReceiptSha256: HASH_B.to_owned(),
            })
            .is_err()
        );
        let gap = gap.into_owner();
        assert_eq!(gap.paths, [RAIN_CONTROL_STATE_GAP_PATH]);
        assert!(gap.waits.is_empty());
    }
}

#[cfg(test)]
#[path = "worker_rain_fixed_gauge_http_test.rs"]
mod fixed_gauge_tests;
