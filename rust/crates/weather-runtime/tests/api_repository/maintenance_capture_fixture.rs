//! Run a real worker capture request through the composed Rust maintenance handler.

use weather_api::{
    AdjustmentMaintenanceCaptureCallback, AdjustmentMaintenanceCaptureCycle,
    AdjustmentMaintenanceOperationError, AdjustmentMaintenanceResponseBody,
    AdjustmentMaintenanceWireEncoder, AdmittedAdjustmentMaintenanceCapture,
    DecodedAdjustmentMaintenanceRequest,
};
use weather_contracts::{
    AdjustmentMaintenanceStageReceipt, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionStageReceipt, JsonValue,
};
use weather_database::Sha256Digest;
use weather_runtime::{
    ApiMaintenanceArchiveHttpJson, ApiMaintenanceArchiveHttpOutcome, ApiMaintenanceArchivePost,
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceRequestBodyCodec,
    ApiMaintenanceRevisionEvidenceValues, ApiMaintenanceRevisionTransportDocument,
    ApiMaintenanceShadowEvidenceValues, EncodedApiMaintenanceRequestOwners,
    HttpApiMaintenanceArchiveRelay, MaintenanceReceiptScalar, PreparedPrivateMaintenanceApi,
    RetainedWorkerSidecarTask, WorkerMaintenanceCapturePost, WorkerMaintenanceCaptureResponse,
    WorkerMaintenanceCaptureTask, WorkerMaintenanceCaptureTransport,
};
use weather_worker::{
    TaskFailure, TemperatureCanaryTimeAdapter, adjustment_maintenance_capture_due_at,
};

use super::FixtureApiReadPool;

pub(crate) const ISSUED: &str = "2026-10-08T18:35:00.000Z";
pub(crate) const DUE: &str = "capture/2026-10-08T18:35:00.000Z";
const ISSUED_MS: i64 = 1_791_484_500_000;
pub(crate) const CAPTURE_JSON: &[u8] =
    br#"{"dueKey":"capture/2026-10-08T18:35:00.000Z","issuedAt":"2026-10-08T18:35:00.000Z"}"#;

struct WireCodec(usize);

impl ApiMaintenanceRequestBodyCodec for WireCodec {
    /// Expose the parsed fixture object while leaving the original key and scalar rules to Rust.
    fn parse_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        assert_eq!(bytes, CAPTURE_JSON);
        self.0 += 1;
        Some(JsonValue::Object(
            [("dueKey", DUE), ("issuedAt", ISSUED)]
                .into_iter()
                .map(|(key, value)| {
                    (
                        key.encode_utf16().collect(),
                        JsonValue::String(value.encode_utf16().collect()),
                    )
                })
                .collect(),
        ))
    }

    /// The value-free capture trigger never reads base64 payloads.
    fn decode_strict_base64(&mut self, _: &[u16], _: usize) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// The capture trigger never consumes a database receipt.
    fn decode_revision_commit_receipt(
        &mut self,
        _: &JsonValue,
    ) -> Option<AdjustmentRevisionCommitReceipt> {
        unreachable!()
    }

    /// The capture trigger does not submit shadow material for re-publication.
    fn decode_shadow_stage_receipt(
        &mut self,
        _: &JsonValue,
    ) -> Option<AdjustmentMaintenanceStageReceipt> {
        unreachable!()
    }

    /// The capture trigger does not submit revision material for re-publication.
    fn decode_revision_stage_receipt(
        &mut self,
        _: &JsonValue,
    ) -> Option<AdjustmentRevisionStageReceipt> {
        unreachable!()
    }
}

pub(crate) struct Values;

impl TemperatureCanaryTimeAdapter for Values {
    /// Give the worker its original due instant, not a second request clock.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        assert_eq!(epoch_ms, ISSUED_MS);
        Ok(ISSUED.to_owned())
    }
}

impl ApiMaintenanceReceiptCanonicalValues for Values {
    /// Resolve only the original elapsed capture clock before the callback.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        (value == ISSUED).then_some(ISSUED_MS)
    }

    /// No evidence bytes are hashed on the trigger route.
    fn sha256(&self, _: &[u8]) -> Option<Sha256Digest> {
        unreachable!()
    }

    /// No evidence receipts are read on the trigger route.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!()
    }
}

impl ApiMaintenanceShadowEvidenceValues for Values {
    /// The trigger supplies no shadow document.
    fn decode_shadow_json(&self, _: &[u8]) -> Option<JsonValue> {
        unreachable!()
    }

    /// The trigger does not project source-row identities.
    fn encode_shadow_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// The trigger supplies no comparator member.
    fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// The trigger supplies no comparator receipt.
    fn encode_shadow_sorted_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// This fixture supplies no decoded maintenance package.
    fn decode_shadow_maintenance_package(
        &self,
        _: &JsonValue,
    ) -> Option<weather_contracts::ForecastAdjustmentMaintenanceRuntimePackage> {
        unreachable!()
    }

    /// This fixture supplies no decoded portable rain model.
    fn decode_shadow_rain_artifact(
        &self,
        _: &JsonValue,
    ) -> Option<weather_contracts::RainHurdleWindPortableArtifact> {
        unreachable!()
    }

    /// This fixture never examines a general wind runtime string.
    fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
        unreachable!()
    }
}

impl ApiMaintenanceRevisionEvidenceValues for Values {
    /// No historical revision body is supplied with the fixed capture trigger.
    fn decode_revision_document(
        &self,
        _: &[u8],
    ) -> Option<ApiMaintenanceRevisionTransportDocument> {
        unreachable!()
    }
}

pub(crate) struct NoArchive;

impl ApiMaintenanceArchiveHttpJson for NoArchive {
    type Error = ();

    /// Capturing the admitted trigger does not encode an archive member.
    fn base64(&mut self, _: &[u8]) -> Result<String, Self::Error> {
        unreachable!()
    }

    /// No archive post is required to acknowledge the fixture callback.
    fn post_json(
        &mut self,
        _: ApiMaintenanceArchivePost<'_>,
    ) -> Result<ApiMaintenanceArchiveHttpOutcome, Self::Error> {
        unreachable!()
    }
}

struct FixedEncoder;

impl AdjustmentMaintenanceWireEncoder for FixedEncoder {
    type Error = ();

    /// Preserve the service-owned fixed 202 bytes without an extra serialization.
    fn encode<'a>(
        &'a mut self,
        _: &AdjustmentMaintenanceResponseBody,
    ) -> Result<&'a [u8], Self::Error> {
        unreachable!()
    }
}

#[derive(Default)]
pub(crate) struct Capture(pub(crate) Option<AdmittedAdjustmentMaintenanceCapture>);

impl AdjustmentMaintenanceCaptureCallback for Capture {
    /// Observe the actual admitted cycle the service passed into the capture operation.
    fn capture(
        &mut self,
        request: AdmittedAdjustmentMaintenanceCapture,
    ) -> Result<(), AdjustmentMaintenanceOperationError> {
        assert!(self.0.replace(request).is_none());
        Ok(())
    }
}

struct CaptureTransport<'a> {
    app: &'a PreparedPrivateMaintenanceApi<FixtureApiReadPool>,
    callback: Capture,
}

impl WorkerMaintenanceCaptureTransport for CaptureTransport<'_> {
    type Error = ();

    /// Deliver exactly the real worker POST to the prepared Rust app without a network.
    fn post_json(
        &mut self,
        post: WorkerMaintenanceCapturePost<'_>,
    ) -> Result<WorkerMaintenanceCaptureResponse, Self::Error> {
        assert_eq!(post.content_type, "application/json");
        assert_eq!(post.body.dueKey, DUE);
        assert_eq!(post.body.issuedAt, ISSUED);
        let (preparation, _, _) = self.app.capture_material();
        let mut relay = HttpApiMaintenanceArchiveRelay::new(
            preparation.configuration.archive_origin.clone(),
            NoArchive,
        );
        let mut codec = WireCodec(0);
        let mut encoder = FixedEncoder;
        let response = self.app.serve_encoded(
            DecodedAdjustmentMaintenanceRequest {
                method: "POST",
                pathname: post.pathname,
            },
            CAPTURE_JSON,
            EncodedApiMaintenanceRequestOwners {
                body_codec: &mut codec,
                evidence_values: &Values,
                capture: Some(&mut self.callback),
                relay: &mut relay,
            },
            &mut encoder,
        );
        assert_eq!(codec.0, 1);
        assert_eq!(response.status_code, 202);
        assert_eq!(response.headers.content_type, "application/json");
        assert_eq!(response.headers.content_length, 17);
        assert_eq!(response.body.as_bytes(), br#"{"accepted":true}"#);
        Ok(WorkerMaintenanceCaptureResponse {
            status_code: response.status_code,
        })
    }
}

/// Derive a scheduled worker capture and prove the original admitted handoff.
pub(super) fn check_worker_capture(app: &PreparedPrivateMaintenanceApi<FixtureApiReadPool>) {
    let due = adjustment_maintenance_capture_due_at(ISSUED_MS + 60_000)
        .expect("original scheduler cycle");
    let mut task = WorkerMaintenanceCaptureTask::new(
        CaptureTransport {
            app,
            callback: Capture::default(),
        },
        Values,
    );
    task.run(due, &mut (), &mut ())
        .expect("ordinary private capture accepted");
    let (transport, _) = task.into_parts();
    let observed = transport
        .callback
        .0
        .expect("Rust admission reached capture callback");
    assert_eq!(observed.cycle, AdjustmentMaintenanceCaptureCycle::Hour18);
    assert_eq!(observed.issued_at.epoch_milliseconds, ISSUED_MS);
    assert_eq!(observed.request.dueKey, DUE);
}
