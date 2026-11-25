//! Private HTTP/JSON relay from the API process to the admitted web archive origin.

use weather_api::{
    AdjustmentMaintenanceGapCallback, AdjustmentMaintenanceOperationError,
    AdjustmentMaintenancePublicationRelay, AdjustmentRevisionPublishResult,
    AdmittedAdjustmentMaintenanceGap, AdmittedAdjustmentMaintenancePublish,
    AdmittedAdjustmentMaintenanceStage, AdmittedAdjustmentRevisionGap,
    AdmittedAdjustmentRevisionPublish, AdmittedAdjustmentRevisionStage,
    AdmittedMaintenanceArchiveOrigin, AdmittedRainFixedGaugeTargetGap,
    AdmittedRainFixedGaugeTargetGapStatus, AdmittedRainMaintenanceControlStateGap,
    AdmittedRainMaintenanceControlStateStage,
};
use weather_contracts::{
    AdjustmentMaintenanceGapInput, AdjustmentMaintenancePublishReceipt,
    AdjustmentMaintenanceStageReceipt, AdjustmentRevisionBatchPublishReceipt,
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionGapInput, AdjustmentRevisionPublishReceipt,
    AdjustmentRevisionStageReceipt, FieldPresence, MaintenanceShadowPredictionMetadata,
    RainFixedGaugeTargetGapInput, RainFixedGaugeTargetGapReceipt, RainFixedGaugeTargetGapStatus,
    RainFixedGaugeTargetGapStatusInput, RainMaintenanceControlStateGapInput,
    RainMaintenanceControlStateStageReceipt,
};

const SHADOW_STAGE_PATH: &str = "/internal/adjustment-maintenance/archive/stage";
const SHADOW_PUBLISH_PATH: &str = "/internal/adjustment-maintenance/archive/publish";
const SHADOW_GAP_PATH: &str = "/internal/adjustment-maintenance/archive/gap";
const REVISION_STAGE_PATH: &str = "/internal/adjustment-maintenance/revision/stage";
const REVISION_PUBLISH_PATH: &str = "/internal/adjustment-maintenance/revision/publish";
const REVISION_GAP_PATH: &str = "/internal/adjustment-maintenance/revision/gap";
const RAIN_CONTROL_STATE_STAGE_PATH: &str =
    "/internal/adjustment-maintenance/rain-control-state/stage";
const RAIN_CONTROL_STATE_GAP_PATH: &str = "/internal/adjustment-maintenance/rain-control-state/gap";
const RAIN_FIXED_GAUGE_TARGET_GAP_PATH: &str =
    "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap";
const RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_PATH: &str =
    "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status";
const MAXIMUM_POST_BYTES: usize = 4_832 * 1_024;

type OperationError = AdjustmentMaintenanceOperationError;

/// Exact borrowed ordinary stage wire members in canonical insertion order.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApiMaintenanceShadowStageBody<'a> {
    pub bodyBase64: &'a str,
    pub comparatorBase64: Option<&'a str>,
    pub metadata: &'a MaintenanceShadowPredictionMetadata,
    pub sourceProjectionBase64: &'a str,
    pub sourceProjectionSha256: &'a str,
}

/// Exact borrowed ordinary publish wire members in canonical insertion order.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApiMaintenanceShadowPublishBody<'a> {
    pub bodyBase64: &'a str,
    pub comparatorBase64: Option<&'a str>,
    pub metadata: &'a MaintenanceShadowPredictionMetadata,
    pub predictionCommittedAt: Option<&'a str>,
    pub revisionReceipt: &'a AdjustmentRevisionCommitReceipt,
    pub sourceProjectionBase64: &'a str,
    pub sourceProjectionSha256: &'a str,
    pub stageReceipt: &'a AdjustmentMaintenanceStageReceipt,
}

/// Exact borrowed revision stage document.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApiMaintenanceRevisionStageBody<'a> {
    pub projectionBase64: &'a str,
}

/// Exact worker-to-API single-revision publication document.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApiMaintenanceRevisionPublishBody<'a> {
    pub projectionBase64: &'a str,
    pub revisionReceipt: &'a AdjustmentRevisionCommitReceipt,
    pub stageReceipt: &'a AdjustmentRevisionStageReceipt,
}

/// Exact worker-to-API grouped-revision publication document.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApiMaintenanceRevisionBatchPublishBody<'a> {
    pub projectionBase64: &'a str,
    pub revisionReceipts: &'a [AdjustmentRevisionCommitReceipt],
    pub stageReceipt: &'a AdjustmentRevisionStageReceipt,
}

/// Exact compact API-to-web single-revision publication document.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApiMaintenanceArchiveRevisionPublishBody<'a> {
    pub revisionReceipt: &'a AdjustmentRevisionCommitReceipt,
    pub stageReceipt: &'a AdjustmentRevisionStageReceipt,
}

/// Exact compact API-to-web grouped-revision publication document.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApiMaintenanceArchiveRevisionBatchPublishBody<'a> {
    pub revisionReceipts: &'a [AdjustmentRevisionCommitReceipt],
    pub stageReceipt: &'a AdjustmentRevisionStageReceipt,
}

/// Exact borrowed rain control-state stage document.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ApiMaintenanceRainControlStateStageBody<'a> {
    pub stateBase64: &'a str,
}

/// Typed request variants serialized only by the approved JSON owner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ApiMaintenanceArchiveRequest<'a> {
    ShadowStage(ApiMaintenanceShadowStageBody<'a>),
    ShadowPublish(ApiMaintenanceShadowPublishBody<'a>),
    ShadowGap(&'a AdjustmentMaintenanceGapInput),
    RevisionStage(ApiMaintenanceRevisionStageBody<'a>),
    RevisionPublish(ApiMaintenanceArchiveRevisionPublishBody<'a>),
    RevisionBatchPublish(ApiMaintenanceArchiveRevisionBatchPublishBody<'a>),
    RevisionGap(&'a AdjustmentRevisionGapInput),
    RainControlStateStage(ApiMaintenanceRainControlStateStageBody<'a>),
    RainControlStateGap(&'a RainMaintenanceControlStateGapInput),
    RainFixedGaugeTargetGap(&'a RainFixedGaugeTargetGapInput),
    RainFixedGaugeTargetGapStatus(&'a RainFixedGaugeTargetGapStatusInput),
}

/// Exact decoded response variants returned only after a successful HTTP status.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiMaintenanceArchiveResponse {
    /// The gap path parsed a successful JSON response but Node does not inspect its value.
    ParsedGapJson,
    ShadowStage(AdjustmentMaintenanceStageReceipt),
    ShadowPublish(AdjustmentMaintenancePublishReceipt),
    RevisionStage(AdjustmentRevisionStageReceipt),
    RevisionPublish(AdjustmentRevisionPublishReceipt),
    RevisionBatchPublish(AdjustmentRevisionBatchPublishReceipt),
    RainControlStateStage(RainMaintenanceControlStateStageReceipt),
    RainFixedGaugeTargetGap(RainFixedGaugeTargetGapReceipt),
    RainFixedGaugeTargetGapStatus(RainFixedGaugeTargetGapStatus),
}

/// Transport result preserving Node's status-before-JSON-response ordering.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiMaintenanceArchiveHttpOutcome {
    Success(ApiMaintenanceArchiveResponse),
    HttpStatus(u16),
}

/// One private POST with the original media type and encoded body limit.
pub struct ApiMaintenanceArchivePost<'a> {
    pub origin: &'a AdmittedMaintenanceArchiveOrigin,
    pub path: &'static str,
    pub content_type: &'static str,
    pub maximum_body_bytes: usize,
    pub body: ApiMaintenanceArchiveRequest<'a>,
}

/// Approved base64, bounded JSON, and private HTTP transport owner.
pub trait ApiMaintenanceArchiveHttpJson {
    type Error;

    /// Encode exact raw bytes using the shared standard base64 owner.
    fn base64(&mut self, bytes: &[u8]) -> Result<String, Self::Error>;

    /// Serialize once, reject encoded bytes over the provided cap, and decode only a successful response.
    ///
    /// For the three value-free gap routes, require a parseable JSON document and discard its value.
    fn post_json(
        &mut self,
        post: ApiMaintenanceArchivePost<'_>,
    ) -> Result<ApiMaintenanceArchiveHttpOutcome, Self::Error>;
}

/// Retained admitted origin and injected HTTP/JSON/base64 owner.
pub struct HttpApiMaintenanceArchiveRelay<Owner> {
    origin: AdmittedMaintenanceArchiveOrigin,
    owner: Owner,
}

impl<Owner> HttpApiMaintenanceArchiveRelay<Owner> {
    /// Compose the relay only after startup has admitted the private origin.
    pub const fn new(origin: AdmittedMaintenanceArchiveOrigin, owner: Owner) -> Self {
        Self { origin, owner }
    }

    /// Return the injected owner after process shutdown or fixture completion.
    pub fn into_owner(self) -> Owner {
        self.owner
    }
}

impl<Owner: ApiMaintenanceArchiveHttpJson> HttpApiMaintenanceArchiveRelay<Owner> {
    /// Encode one optional byte member while preserving source omission.
    fn optional_base64(
        &mut self,
        value: &FieldPresence<Vec<u8>>,
    ) -> Result<Option<String>, OperationError> {
        match value {
            FieldPresence::Missing => Ok(None),
            FieldPresence::Value(bytes) => self
                .owner
                .base64(bytes)
                .map(Some)
                .map_err(|_| OperationError::Rejected),
        }
    }

    /// POST one typed document and apply the shared Node response classification.
    fn post(
        &mut self,
        path: &'static str,
        body: ApiMaintenanceArchiveRequest<'_>,
    ) -> Result<ApiMaintenanceArchiveResponse, OperationError> {
        let outcome = self
            .owner
            .post_json(ApiMaintenanceArchivePost {
                origin: &self.origin,
                path,
                content_type: "application/json",
                maximum_body_bytes: MAXIMUM_POST_BYTES,
                body,
            })
            .map_err(|_| OperationError::Rejected)?;
        // preserve only the archive's bounded capacity signal as retryable
        match outcome {
            ApiMaintenanceArchiveHttpOutcome::Success(response) => Ok(response),
            ApiMaintenanceArchiveHttpOutcome::HttpStatus(429) => {
                Err(OperationError::RevisionSpoolFull)
            }
            ApiMaintenanceArchiveHttpOutcome::HttpStatus(_) => Err(OperationError::Rejected),
        }
    }

    /// Await a successful JSON response without requiring particular acknowledgement fields.
    fn post_gap(
        &mut self,
        path: &'static str,
        body: ApiMaintenanceArchiveRequest<'_>,
    ) -> Result<(), OperationError> {
        // the original gap callers ignore the decoded value
        match self.post(path, body)? {
            ApiMaintenanceArchiveResponse::ParsedGapJson => Ok(()),
            _ => Err(OperationError::Rejected),
        }
    }
}

impl<Owner: ApiMaintenanceArchiveHttpJson> AdjustmentMaintenanceGapCallback
    for HttpApiMaintenanceArchiveRelay<Owner>
{
    /// Relay one admitted categorical gap without raw prediction values.
    fn record_gap(
        &mut self,
        input: AdmittedAdjustmentMaintenanceGap,
    ) -> Result<(), OperationError> {
        self.post_gap(
            SHADOW_GAP_PATH,
            ApiMaintenanceArchiveRequest::ShadowGap(&input.request),
        )
    }
}

impl<Owner: ApiMaintenanceArchiveHttpJson> AdjustmentMaintenancePublicationRelay
    for HttpApiMaintenanceArchiveRelay<Owner>
{
    /// Stage exact body, comparator, metadata, and source bytes before database admission.
    fn stage_shadow(
        &mut self,
        input: &AdmittedAdjustmentMaintenanceStage,
    ) -> Result<AdjustmentMaintenanceStageReceipt, OperationError> {
        let body = self
            .owner
            .base64(&input.relay_input.body)
            .map_err(|_| OperationError::Rejected)?;
        let comparator = self.optional_base64(&input.relay_input.comparator)?;
        let source = self
            .owner
            .base64(&input.relay_input.sourceProjection)
            .map_err(|_| OperationError::Rejected)?;
        let request = ApiMaintenanceShadowStageBody {
            bodyBase64: &body,
            comparatorBase64: comparator.as_deref(),
            metadata: &input.relay_input.metadata,
            sourceProjectionBase64: &source,
            sourceProjectionSha256: &input.relay_input.sourceProjectionSha256,
        };
        // require the exact stage receipt response variant
        match self.post(
            SHADOW_STAGE_PATH,
            ApiMaintenanceArchiveRequest::ShadowStage(request),
        )? {
            ApiMaintenanceArchiveResponse::ShadowStage(receipt) => Ok(receipt),
            _ => Err(OperationError::Rejected),
        }
    }

    /// Publish exact admitted bytes and receipts only after database admission.
    fn publish_shadow(
        &mut self,
        input: &AdmittedAdjustmentMaintenancePublish,
    ) -> Result<AdjustmentMaintenancePublishReceipt, OperationError> {
        let body = self
            .owner
            .base64(&input.relay_input.body)
            .map_err(|_| OperationError::Rejected)?;
        let comparator = self.optional_base64(&input.relay_input.comparator)?;
        let source = self
            .owner
            .base64(&input.relay_input.sourceProjection)
            .map_err(|_| OperationError::Rejected)?;
        let prediction_committed_at = match &input.relay_input.predictionCommittedAt {
            FieldPresence::Missing => None,
            FieldPresence::Value(value) => Some(value.as_str()),
        };
        let request = ApiMaintenanceShadowPublishBody {
            bodyBase64: &body,
            comparatorBase64: comparator.as_deref(),
            metadata: &input.relay_input.metadata,
            predictionCommittedAt: prediction_committed_at,
            revisionReceipt: &input.relay_input.revisionReceipt,
            sourceProjectionBase64: &source,
            sourceProjectionSha256: &input.relay_input.sourceProjectionSha256,
            stageReceipt: &input.relay_input.stageReceipt,
        };
        // require the exact publication receipt response variant
        match self.post(
            SHADOW_PUBLISH_PATH,
            ApiMaintenanceArchiveRequest::ShadowPublish(request),
        )? {
            ApiMaintenanceArchiveResponse::ShadowPublish(receipt) => Ok(receipt),
            _ => Err(OperationError::Rejected),
        }
    }

    /// Stage one exact canonical revision projection before pointer binding.
    fn stage_revision(
        &mut self,
        input: &AdmittedAdjustmentRevisionStage,
    ) -> Result<AdjustmentRevisionStageReceipt, OperationError> {
        let projection = self
            .owner
            .base64(&input.projection)
            .map_err(|_| OperationError::Rejected)?;
        // require the exact revision stage receipt response variant
        match self.post(
            REVISION_STAGE_PATH,
            ApiMaintenanceArchiveRequest::RevisionStage(ApiMaintenanceRevisionStageBody {
                projectionBase64: &projection,
            }),
        )? {
            ApiMaintenanceArchiveResponse::RevisionStage(receipt) => Ok(receipt),
            _ => Err(OperationError::Rejected),
        }
    }

    /// Publish one single or grouped revision after authoritative database admission.
    fn publish_revision(
        &mut self,
        input: &AdmittedAdjustmentRevisionPublish,
    ) -> Result<AdjustmentRevisionPublishResult, OperationError> {
        // retain the source's distinct single and grouped document members
        match input {
            AdmittedAdjustmentRevisionPublish::Single {
                projection: _,
                revision_receipt,
                stage_receipt,
            } => {
                let response = self.post(
                    REVISION_PUBLISH_PATH,
                    ApiMaintenanceArchiveRequest::RevisionPublish(
                        ApiMaintenanceArchiveRevisionPublishBody {
                            revisionReceipt: revision_receipt,
                            stageReceipt: stage_receipt,
                        },
                    ),
                )?;
                // reject a grouped or unrelated success body on the single path
                match response {
                    ApiMaintenanceArchiveResponse::RevisionPublish(receipt) => {
                        Ok(AdjustmentRevisionPublishResult::Single(receipt))
                    }
                    _ => Err(OperationError::Rejected),
                }
            }
            AdmittedAdjustmentRevisionPublish::Batch {
                projection: _,
                revision_receipts,
                stage_receipt,
            } => {
                let response = self.post(
                    REVISION_PUBLISH_PATH,
                    ApiMaintenanceArchiveRequest::RevisionBatchPublish(
                        ApiMaintenanceArchiveRevisionBatchPublishBody {
                            revisionReceipts: revision_receipts,
                            stageReceipt: stage_receipt,
                        },
                    ),
                )?;
                // reject a single or unrelated success body on the grouped path
                match response {
                    ApiMaintenanceArchiveResponse::RevisionBatchPublish(receipt) => {
                        Ok(AdjustmentRevisionPublishResult::Batch(receipt))
                    }
                    _ => Err(OperationError::Rejected),
                }
            }
        }
    }

    /// Relay one admitted permanent database revision gap without raw values.
    fn record_revision_gap(
        &mut self,
        input: AdmittedAdjustmentRevisionGap,
    ) -> Result<(), OperationError> {
        self.post_gap(
            REVISION_GAP_PATH,
            ApiMaintenanceArchiveRequest::RevisionGap(&input.request),
        )
    }

    /// Stage one canonical future-only rain control state.
    fn stage_rain_control_state(
        &mut self,
        input: AdmittedRainMaintenanceControlStateStage,
    ) -> Result<RainMaintenanceControlStateStageReceipt, OperationError> {
        let state = self
            .owner
            .base64(&input.state)
            .map_err(|_| OperationError::Rejected)?;
        // require the exact rain control-state stage response variant
        match self.post(
            RAIN_CONTROL_STATE_STAGE_PATH,
            ApiMaintenanceArchiveRequest::RainControlStateStage(
                ApiMaintenanceRainControlStateStageBody {
                    stateBase64: &state,
                },
            ),
        )? {
            ApiMaintenanceArchiveResponse::RainControlStateStage(receipt) => Ok(receipt),
            _ => Err(OperationError::Rejected),
        }
    }

    /// Relay one admitted abandoned rain control-state gap.
    fn record_rain_control_state_gap(
        &mut self,
        input: AdmittedRainMaintenanceControlStateGap,
    ) -> Result<(), OperationError> {
        self.post_gap(
            RAIN_CONTROL_STATE_GAP_PATH,
            ApiMaintenanceArchiveRequest::RainControlStateGap(&input.request),
        )
    }

    /// Relay one admitted fixed-gauge permanent gap and decode its receipt.
    fn record_rain_fixed_gauge_target_gap(
        &mut self,
        input: &AdmittedRainFixedGaugeTargetGap,
    ) -> Result<RainFixedGaugeTargetGapReceipt, OperationError> {
        // require the exact fixed-gauge receipt response variant
        match self.post(
            RAIN_FIXED_GAUGE_TARGET_GAP_PATH,
            ApiMaintenanceArchiveRequest::RainFixedGaugeTargetGap(&input.request),
        )? {
            ApiMaintenanceArchiveResponse::RainFixedGaugeTargetGap(receipt) => Ok(receipt),
            _ => Err(OperationError::Rejected),
        }
    }

    /// Read one admitted fixed-gauge permanent-gap disposition.
    fn read_rain_fixed_gauge_target_gap(
        &mut self,
        input: &AdmittedRainFixedGaugeTargetGapStatus,
    ) -> Result<RainFixedGaugeTargetGapStatus, OperationError> {
        // require the exact fixed-gauge status response variant
        match self.post(
            RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_PATH,
            ApiMaintenanceArchiveRequest::RainFixedGaugeTargetGapStatus(&input.request),
        )? {
            ApiMaintenanceArchiveResponse::RainFixedGaugeTargetGapStatus(status) => Ok(status),
            _ => Err(OperationError::Rejected),
        }
    }
}

#[cfg(test)]
mod tests {
    use weather_contracts::{AdjustmentMaintenanceGapInputFamilyEnum, MaintenanceShadowGapReason};

    use super::*;

    struct FixtureOwner {
        outcome: Option<ApiMaintenanceArchiveHttpOutcome>,
        origin: Option<String>,
        path: Option<&'static str>,
        due_key: Option<String>,
    }

    impl ApiMaintenanceArchiveHttpJson for FixtureOwner {
        type Error = &'static str;

        /// Reject base64 work in the value-free gap fixture.
        fn base64(&mut self, _bytes: &[u8]) -> Result<String, Self::Error> {
            Err("unexpected base64")
        }

        /// Capture the exact origin, path, and borrowed categorical gap document.
        fn post_json(
            &mut self,
            post: ApiMaintenanceArchivePost<'_>,
        ) -> Result<ApiMaintenanceArchiveHttpOutcome, Self::Error> {
            assert_eq!(post.content_type, "application/json");
            assert_eq!(post.maximum_body_bytes, 4_947_968);
            let ApiMaintenanceArchiveRequest::ShadowGap(gap) = post.body else {
                return Err("unexpected request");
            };
            self.origin = Some(post.origin.canonical().to_owned());
            self.path = Some(post.path);
            self.due_key = Some(gap.dueKey.clone());
            self.outcome.take().ok_or("missing outcome")
        }
    }

    /// Accept decoded gap JSON without requiring a recorded flag.
    #[test]
    fn relays_admitted_gap_to_exact_archive_path() {
        let mut relay = fixture_relay(ApiMaintenanceArchiveHttpOutcome::Success(
            ApiMaintenanceArchiveResponse::ParsedGapJson,
        ));

        relay
            .record_gap(gap())
            .expect("parseable JSON response must succeed");

        let owner = relay.into_owner();
        assert_eq!(owner.origin.as_deref(), Some("http://web:8080/"));
        assert_eq!(owner.path, Some(SHADOW_GAP_PATH));
        assert_eq!(
            owner.due_key.as_deref(),
            Some("capture/2026-01-01T00:35:00.000Z")
        );
    }

    /// Retain the archive's original retryable capacity status.
    #[test]
    fn preserves_archive_capacity_status() {
        let mut relay = fixture_relay(ApiMaintenanceArchiveHttpOutcome::HttpStatus(429));

        assert_eq!(
            relay.record_gap(gap()),
            Err(OperationError::RevisionSpoolFull)
        );
    }

    /// Compose one admitted origin with the configured response fixture.
    fn fixture_relay(
        outcome: ApiMaintenanceArchiveHttpOutcome,
    ) -> HttpApiMaintenanceArchiveRelay<FixtureOwner> {
        HttpApiMaintenanceArchiveRelay::new(
            AdmittedMaintenanceArchiveOrigin::admitted("http://web:8080/".to_owned()),
            FixtureOwner {
                outcome: Some(outcome),
                origin: None,
                path: None,
                due_key: None,
            },
        )
    }

    /// Build one ordinary admitted value-free gap.
    fn gap() -> AdmittedAdjustmentMaintenanceGap {
        AdmittedAdjustmentMaintenanceGap {
            request: AdjustmentMaintenanceGapInput {
                dueKey: "capture/2026-01-01T00:35:00.000Z".to_owned(),
                family: AdjustmentMaintenanceGapInputFamilyEnum::Wind,
                reason: MaintenanceShadowGapReason::SourceIncomplete,
                registrationSha256: "a".repeat(64),
            },
        }
    }
}
