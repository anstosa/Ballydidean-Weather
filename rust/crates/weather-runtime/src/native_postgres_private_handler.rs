//! Concrete PostgreSQL private-maintenance request ownership over the native HTTP listener.

use std::{
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use hyper::{Request, Response, body::Incoming};
use tokio::sync::Semaphore;
use weather_api::{AdjustmentMaintenanceCaptureCallback, AdmittedMaintenanceArchiveOrigin};
use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceRevisionEvidenceValues,
    ApiMaintenanceRevisionTransportDocument, ApiMaintenanceShadowEvidenceValues,
    EncodedApiMaintenanceRequestOwners, HttpApiMaintenanceArchiveRelay, MaintenanceReceiptScalar,
    NativeAdjustmentMaintenanceWireEncoder, NativeApiMaintenanceArchiveHttpJson,
    NativeApiMaintenanceHttpRequest, NativeApiMaintenanceRequestBodyCodec, NativeApiModelAdmission,
    NativeHttpBody, NativeModelEvidenceValues, NativeOutboundHttp, NativePostgresPool,
    NativePrivatePostTransport, NativePublicApiIngressError, PreparedPrivateMaintenanceApi,
};

/// Exact native archive relay used by every retained private request lane.
pub type NativePostgresPrivateArchiveRelay = HttpApiMaintenanceArchiveRelay<
    NativeApiMaintenanceArchiveHttpJson<NativeModelEvidenceValues, NativePrivatePostTransport>,
>;

/// Decode the complete generated revision document without weakening its canonical grammar.
pub trait NativePrivateRevisionDocumentDecoder {
    /// Return one typed original document only after all generated fields are admitted.
    fn decode_revision_document(
        &self,
        projection: &[u8],
    ) -> Option<ApiMaintenanceRevisionTransportDocument>;
}

impl<Decoder> NativePrivateRevisionDocumentDecoder for Decoder
where
    Decoder: Fn(&[u8]) -> Option<ApiMaintenanceRevisionTransportDocument>,
{
    /// Allow a retained function owner to supply the exact source-backed decoder.
    fn decode_revision_document(
        &self,
        projection: &[u8],
    ) -> Option<ApiMaintenanceRevisionTransportDocument> {
        self(projection)
    }
}

/// Join the approved native evidence primitives to one complete revision-document decoder.
pub struct NativePostgresPrivateEvidence<Decoder> {
    values: NativeModelEvidenceValues,
    revision: Decoder,
}

impl<Decoder> NativePostgresPrivateEvidence<Decoder> {
    /// Retain the genuine zero-state evidence owner and caller-proved revision grammar.
    pub const fn new(revision: Decoder) -> Self {
        Self {
            values: NativeModelEvidenceValues::new(),
            revision,
        }
    }
}

impl<Decoder> ApiMaintenanceReceiptCanonicalValues for NativePostgresPrivateEvidence<Decoder> {
    /// Preserve the native canonical UTC admission used by every maintenance receipt.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        self.values.canonical_utc_epoch_milliseconds(value)
    }

    /// Hash the exact observed bytes through the approved native digest owner.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        ApiMaintenanceReceiptCanonicalValues::sha256(&self.values, bytes)
    }

    /// Preserve source field order in every unsigned maintenance receipt preimage.
    fn unsigned_receipt_sha256(
        &self,
        fields: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        self.values.unsigned_receipt_sha256(fields)
    }
}

impl<Decoder> ApiMaintenanceShadowEvidenceValues for NativePostgresPrivateEvidence<Decoder> {
    /// Decode exact shadow JSON through the production native evidence owner.
    fn decode_shadow_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.values.decode_shadow_json(bytes)
    }

    /// Preserve ordered nested JSON without the archive framing newline.
    fn encode_shadow_ordered_json(&self, document: &JsonValue) -> Option<Vec<u8>> {
        self.values.encode_shadow_ordered_json(document)
    }

    /// Preserve strict padded-base64 admission for comparator documents.
    fn decode_shadow_comparator_base64(&self, encoded: &[u16]) -> Option<Vec<u8>> {
        self.values.decode_shadow_comparator_base64(encoded)
    }

    /// Preserve recursively sorted shadow evidence with its canonical newline.
    fn encode_shadow_sorted_json(&self, document: &JsonValue) -> Option<Vec<u8>> {
        self.values.encode_shadow_sorted_json(document)
    }

    /// Apply the exact retained runtime-bundle string filter.
    fn check_legacy_wind_runtime_value(&self, code_units: &[u16]) -> Option<()> {
        self.values.check_legacy_wind_runtime_value(code_units)
    }
}

impl<Decoder: NativePrivateRevisionDocumentDecoder> ApiMaintenanceRevisionEvidenceValues
    for NativePostgresPrivateEvidence<Decoder>
{
    /// Require the separately owned full generated revision grammar before database admission.
    fn decode_revision_document(
        &self,
        projection: &[u8],
    ) -> Option<ApiMaintenanceRevisionTransportDocument> {
        self.revision.decode_revision_document(projection)
    }
}

/// Construct and lend one genuine request capture owner while its application material is borrowed.
pub trait NativePostgresPrivateCaptureOwner {
    /// Run one request with a callback bound to the prepared candidates, witness, and native model.
    fn with_capture<Result>(
        &mut self,
        application: &PreparedPrivateMaintenanceApi<NativePostgresPool>,
        model: &NativeApiModelAdmission,
        operation: impl FnOnce(&mut dyn AdjustmentMaintenanceCaptureCallback) -> Result,
    ) -> Result;
}

/// Reusable request-local codecs and retained private effects for one listener handler.
struct NativePostgresPrivateState<Capture, Decoder> {
    body_codec: NativeApiMaintenanceRequestBodyCodec<NativeModelEvidenceValues>,
    capture: Capture,
    encoder: NativeAdjustmentMaintenanceWireEncoder<NativeModelEvidenceValues>,
    evidence: NativePostgresPrivateEvidence<Decoder>,
    model: NativeApiModelAdmission,
    relay: NativePostgresPrivateArchiveRelay,
}

/// One cloneable local-listener handler retaining genuine maintenance owners.
pub struct NativePostgresPrivateHandler<Capture, Decoder> {
    body_deadline: Duration,
    lane: Arc<Semaphore>,
    state: Arc<Mutex<NativePostgresPrivateState<Capture, Decoder>>>,
}

impl<Capture, Decoder> Clone for NativePostgresPrivateHandler<Capture, Decoder> {
    /// Clone only the local retained owner; request state remains single and source ordered.
    fn clone(&self) -> Self {
        Self {
            body_deadline: self.body_deadline,
            lane: Arc::clone(&self.lane),
            state: Arc::clone(&self.state),
        }
    }
}

impl<Capture, Decoder> NativePostgresPrivateHandler<Capture, Decoder> {
    /// Bind admitted archive authority, bounded transport, and complete request owners.
    pub fn new(
        archive_origin: AdmittedMaintenanceArchiveOrigin,
        client: Arc<NativeOutboundHttp>,
        archive_timeout: Duration,
        body_deadline: Duration,
        model: NativeApiModelAdmission,
        capture: Capture,
        revision: Decoder,
    ) -> Self {
        let values = NativeModelEvidenceValues::new();
        let transport = NativePrivatePostTransport::new(client, archive_timeout);
        let archive = NativeApiMaintenanceArchiveHttpJson::new(values, transport);
        Self {
            body_deadline,
            lane: Arc::new(Semaphore::new(1)),
            state: Arc::new(Mutex::new(NativePostgresPrivateState {
                body_codec: NativeApiMaintenanceRequestBodyCodec::new(values),
                capture,
                encoder: NativeAdjustmentMaintenanceWireEncoder::new(values),
                evidence: NativePostgresPrivateEvidence::new(revision),
                model,
                relay: HttpApiMaintenanceArchiveRelay::new(archive_origin, archive),
            })),
        }
    }
}

impl<Capture, Decoder> NativePostgresPrivateHandler<Capture, Decoder>
where
    Capture: NativePostgresPrivateCaptureOwner + Send + 'static,
    Decoder: NativePrivateRevisionDocumentDecoder + Send + 'static,
{
    /// Read one bounded request and dispatch it through the startup-admitted private application.
    pub async fn serve(
        &self,
        request: Request<Incoming>,
        application: Rc<PreparedPrivateMaintenanceApi<NativePostgresPool>>,
    ) -> Result<Response<NativeHttpBody>, hyper::http::Error> {
        let head = request.method() == hyper::Method::HEAD;
        let request = NativeApiMaintenanceHttpRequest::read(request, self.body_deadline).await;
        let permit = match Arc::clone(&self.lane).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => return NativePublicApiIngressError.response(head),
        };
        let application = application.shared_worker();
        let state = Arc::clone(&self.state);
        // retain one reusable blocking request lane while the listener continues polling sockets
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut state = match state.lock() {
                Ok(state) => state,
                Err(_) => return NativePublicApiIngressError.response(head),
            };
            let NativePostgresPrivateState {
                body_codec,
                capture,
                encoder,
                evidence,
                model,
                relay,
            } = &mut *state;
            capture.with_capture(&application, model, |capture| {
                request.serve(
                    &application,
                    EncodedApiMaintenanceRequestOwners {
                        body_codec,
                        evidence_values: evidence,
                        capture: Some(capture),
                        relay,
                    },
                    encoder,
                )
            })
        })
        .await
        .unwrap_or_else(|_| NativePublicApiIngressError.response(head))
    }

    /// Await the last admitted blocking private request after the listeners stop accepting.
    pub async fn drain(&self) {
        // the owned permit is held until actual repository and relay work settles
        let _ = self.lane.acquire().await;
    }
}
