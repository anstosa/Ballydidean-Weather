//! Process-owned scheduled forecast association and bounded loopback transport.

use std::{
    collections::HashMap,
    fmt::Write as _,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use hyper::{
    Method,
    header::{ACCEPT, CONTENT_TYPE, HeaderValue},
};
use url::Url;
use weather_edge::{
    AdmittedForecastScheduleCapture, AdmittedPublicApiOrigin, FORECAST_SCHEDULER_CAPTURE_PATH,
    ForecastScheduleSettlement, ForecastSchedulerCaptureAdmissionPort,
    ForecastSchedulerCaptureAdmissionRequest, ForecastSchedulerCaptureAwaitOutcome,
    ForecastSchedulerCaptureClassifiedError, PublicApiBodyReadOutcome, PublicApiHttpMethod,
    PublicApiHttpPort, PublicApiHttpRequest, PublicApiHttpResponse, PublicApiHttpResponseHead,
    PublicApiRedirectPolicy, PublishedForecastSchedulerCapture,
};

use crate::{
    DecodedNativeEdgeTarget, NativeOutboundHttp, NativeOutboundHttpError,
    NativeOutboundHttpRequest, NativeOutboundHttpResponse,
};

const PARAMETER: &str = "__weather_scheduler_nonce";
const MAXIMUM_BODY_BYTES: u64 = 1024 * 1024;

/// One failure below scheduler request classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeSchedulerCaptureError {
    Association,
    RequestFailed,
    Transport,
}

impl ForecastSchedulerCaptureClassifiedError for NativeSchedulerCaptureError {
    /// Keep the original generic capture failure when no source-specific code was returned.
    fn stable_failure_reason(&self) -> Option<&str> {
        None
    }
}

/// A first-wins result owned only by a published scheduler association.
struct Settlement {
    sender: Mutex<
        Option<mpsc::SyncSender<Result<ForecastScheduleSettlement, NativeSchedulerCaptureError>>>,
    >,
}

impl Settlement {
    /// Publish the first terminal result without waiting for the blocking scheduler receiver.
    fn settle(&self, result: Result<ForecastScheduleSettlement, NativeSchedulerCaptureError>) {
        // consume the private sender once even if a late completion follows timeout or failure
        if let Ok(mut held) = self.sender.lock()
            && let Some(sender) = held.take()
        {
            let _ = sender.try_send(result);
        }
    }
}

struct Entry {
    slot: AdmittedForecastScheduleCapture,
    settlement: Arc<Settlement>,
}

/// One process-scoped live registry shared by the scheduler and real edge request handler.
#[derive(Clone, Default)]
pub struct NativeScheduledCaptureRegistry {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
}

/// One admitted request association retained through its actual response terminal.
pub struct NativeScheduledCaptureClaim {
    slot: AdmittedForecastScheduleCapture,
    settlement: Arc<Settlement>,
}

impl NativeScheduledCaptureClaim {
    /// Bind the existing capture application to the admitted due slot.
    pub fn slot(&self) -> AdmittedForecastScheduleCapture {
        self.slot.clone()
    }

    /// Publish the exact evidence/archive settlement after the true response terminal.
    pub fn settle(self, settlement: ForecastScheduleSettlement) {
        self.settlement.settle(Ok(settlement));
    }

    /// Retain an explicit request failure when the actual response could not be completed.
    pub fn fail(self) {
        self.settlement
            .settle(Err(NativeSchedulerCaptureError::RequestFailed));
    }
}

impl Drop for NativeScheduledCaptureClaim {
    /// Explicitly fail a claimed request that never reached a completed response settlement.
    fn drop(&mut self) {
        self.settlement
            .settle(Err(NativeSchedulerCaptureError::RequestFailed));
    }
}

impl NativeScheduledCaptureRegistry {
    /// Consume only an actual published loopback request and remove its process-owned query member.
    pub fn claim(
        &self,
        peer: IpAddr,
        method: &str,
        target: &mut DecodedNativeEdgeTarget,
    ) -> Option<NativeScheduledCaptureClaim> {
        // match the source's exact forecast lane and its read-only method boundary
        if !matches!(method, "GET" | "HEAD")
            || target.pathname != "/api/v1/sites/ballydidean/forecast"
        {
            return None;
        }
        // preserve the source's actual loopback association requirement
        let loopback = match peer {
            IpAddr::V4(address) => address == Ipv4Addr::LOCALHOST,
            IpAddr::V6(address) => {
                address == Ipv6Addr::LOCALHOST
                    || address.to_ipv4_mapped() == Some(Ipv4Addr::LOCALHOST)
            }
        };
        if !loopback {
            return None;
        }
        let mut candidates = target.query.iter().filter(|(name, _)| name == PARAMETER);
        let (_, nonce) = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        let entry = self.entries.lock().ok()?.remove(nonce)?;
        // mirror the source URLSearchParams deletion and its normalized remaining query encoding
        target.query.retain(|(name, _)| name != PARAMETER);
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.extend_pairs(
            target
                .query
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );
        let query = query.finish();
        target.search = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        Some(NativeScheduledCaptureClaim {
            slot: entry.slot,
            settlement: entry.settlement,
        })
    }
}

/// Process-owned scheduler admission binding to its actual public listener port.
pub struct NativeSchedulerCaptureAdmission {
    registry: NativeScheduledCaptureRegistry,
    origin: Url,
}

/// Retain the receiver, deadline, and cleanup identity of one real published request.
pub struct NativeSchedulerCaptureAttempt {
    nonce: String,
    settlement: Arc<Settlement>,
    receiver: mpsc::Receiver<Result<ForecastScheduleSettlement, NativeSchedulerCaptureError>>,
    deadline: Instant,
}

impl NativeSchedulerCaptureAdmission {
    /// Retain the actual admitted local listener port and shared request association registry.
    pub fn new(
        port: u16,
        registry: NativeScheduledCaptureRegistry,
    ) -> Result<Self, NativeSchedulerCaptureError> {
        if port == 0 {
            return Err(NativeSchedulerCaptureError::Association);
        }
        let origin = Url::parse(&format!("http://127.0.0.1:{port}"))
            .map_err(|_| NativeSchedulerCaptureError::Association)?;
        Ok(Self { registry, origin })
    }
}

impl ForecastSchedulerCaptureAdmissionPort for NativeSchedulerCaptureAdmission {
    type Attempt = NativeSchedulerCaptureAttempt;
    type Error = NativeSchedulerCaptureError;

    /// Publish one genuine process association and the fixed source capture target.
    fn publish_capture(
        &mut self,
        request: ForecastSchedulerCaptureAdmissionRequest<'_>,
    ) -> Result<PublishedForecastSchedulerCapture<Self::Attempt>, Self::Error> {
        // refuse any target outside the existing scheduler capture route
        if request.path != FORECAST_SCHEDULER_CAPTURE_PATH {
            return Err(NativeSchedulerCaptureError::Association);
        }
        let mut entropy = [0u8; 32];
        getrandom::fill(&mut entropy).map_err(|_| NativeSchedulerCaptureError::Association)?;
        let mut nonce = String::with_capacity(64);
        // render the predecessor's exact lower-hex nonce spelling
        for byte in entropy {
            write!(&mut nonce, "{byte:02x}").expect("string write");
        }
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(request.timeout_ms))
            .ok_or(NativeSchedulerCaptureError::Association)?;
        let mut target = self
            .origin
            .join(request.path)
            .map_err(|_| NativeSchedulerCaptureError::Association)?;
        target.query_pairs_mut().append_pair(PARAMETER, &nonce);
        let (sender, receiver) = mpsc::sync_channel(1);
        let settlement = Arc::new(Settlement {
            sender: Mutex::new(Some(sender)),
        });
        let mut entries = self
            .registry
            .entries
            .lock()
            .map_err(|_| NativeSchedulerCaptureError::Association)?;
        // retain the one real association without replacing a live identity
        if entries.contains_key(&nonce) {
            return Err(NativeSchedulerCaptureError::Association);
        }
        entries.insert(
            nonce.clone(),
            Entry {
                slot: AdmittedForecastScheduleCapture::admitted(request.due_key),
                settlement: Arc::clone(&settlement),
            },
        );
        let attempt = NativeSchedulerCaptureAttempt {
            nonce,
            settlement,
            receiver,
            deadline,
        };
        Ok(PublishedForecastSchedulerCapture::admitted(
            attempt,
            AdmittedPublicApiOrigin::admitted(self.origin.as_str()),
            target.path(),
            target
                .query()
                .map_or_else(String::new, |query| format!("?{query}")),
        ))
    }

    /// Settle the genuine attempt once on a failed loopback request.
    fn settle_request_failed(&mut self, attempt: &mut Self::Attempt) {
        attempt
            .settlement
            .settle(Err(NativeSchedulerCaptureError::RequestFailed));
    }

    /// Wait only for a completed real response terminal and its evidence settlement, with the source deadline.
    fn await_settlement(
        &mut self,
        attempt: &mut Self::Attempt,
    ) -> Result<ForecastSchedulerCaptureAwaitOutcome, Self::Error> {
        match attempt
            .receiver
            .recv_timeout(attempt.deadline.saturating_duration_since(Instant::now()))
        {
            Ok(result) => result.map(ForecastSchedulerCaptureAwaitOutcome::Settled),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Ok(ForecastSchedulerCaptureAwaitOutcome::TimedOut)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(NativeSchedulerCaptureError::Association)
            }
        }
    }

    /// Retire the exact process association on every completed or failed attempt.
    fn cleanup_capture(&mut self, attempt: Self::Attempt) {
        // remove the unconsumed association and prevent a later stale terminal from settling it
        if let Ok(mut entries) = self.registry.entries.lock() {
            entries.remove(&attempt.nonce);
        }
        attempt
            .settlement
            .settle(Err(NativeSchedulerCaptureError::RequestFailed));
    }
}

/// Bounded shared-client transport for the fixed local scheduler request.
pub struct NativeSchedulerCaptureHttp {
    client: Arc<NativeOutboundHttp>,
}

impl NativeSchedulerCaptureHttp {
    /// Adopt the actual process pooled client without starting another runtime.
    pub const fn new(client: Arc<NativeOutboundHttp>) -> Self {
        Self { client }
    }
}

/// Retain one complete bounded native scheduler response without another body copy.
pub struct NativeSchedulerCaptureResponse(NativeOutboundHttpResponse);

impl PublicApiHttpResponse for NativeSchedulerCaptureResponse {
    type Error = NativeSchedulerCaptureError;

    /// Expose the actual received response metadata.
    fn head(&self) -> PublicApiHttpResponseHead {
        PublicApiHttpResponseHead {
            content_length: self
                .0
                .declared_body_bytes
                .and_then(|length| usize::try_from(length).ok()),
            content_type: self
                .0
                .headers
                .get(CONTENT_TYPE)
                .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned()),
            status_code: self.0.status,
        }
    }

    /// Drain the already bounded native response through the source receiver exactly once.
    fn read_body(
        &mut self,
        receiver: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<PublicApiBodyReadOutcome, Self::Error> {
        let body = std::mem::take(&mut self.0.body);
        // retain the original receiver's finite refusal semantics
        for chunk in body.chunks(16 * 1024) {
            if !receiver(chunk) {
                return Ok(PublicApiBodyReadOutcome::ReceiverStopped);
            }
        }
        Ok(PublicApiBodyReadOutcome::Complete)
    }
}

impl PublicApiHttpPort for NativeSchedulerCaptureHttp {
    type Error = NativeSchedulerCaptureError;
    type Response = NativeSchedulerCaptureResponse;

    /// Execute only the admitted scheduler's fixed loopback forecast request.
    fn execute(&self, request: &PublicApiHttpRequest) -> Result<Self::Response, Self::Error> {
        // preserve the source's method, accept, redirect, and exact forecast route
        if request.method != PublicApiHttpMethod::Get
            || request.redirect != PublicApiRedirectPolicy::Manual
            || request.accept != "application/json"
            || request.pathname != "/api/v1/sites/ballydidean/forecast"
        {
            return Err(NativeSchedulerCaptureError::Transport);
        }
        let base = Url::parse(request.origin.as_str())
            .map_err(|_| NativeSchedulerCaptureError::Transport)?;
        if base.scheme() != "http" || base.host_str() != Some("127.0.0.1") {
            return Err(NativeSchedulerCaptureError::Transport);
        }
        let target = base
            .join(&format!("{}{}", request.pathname, request.search))
            .map_err(|_| NativeSchedulerCaptureError::Transport)?;
        if target.origin() != base.origin() {
            return Err(NativeSchedulerCaptureError::Transport);
        }
        let outbound = NativeOutboundHttpRequest::new(Method::GET, target)
            .map_err(transport_error)?
            .header(ACCEPT, HeaderValue::from_static("application/json"))
            .manual_redirect()
            .stream_body_limit_only();
        self.client
            .execute(
                outbound,
                Duration::from_millis(request.timeout_ms),
                MAXIMUM_BODY_BYTES,
            )
            .map(NativeSchedulerCaptureResponse)
            .map_err(transport_error)
    }
}

/// Keep provider or transport details out of the scheduler's bounded diagnostics.
fn transport_error(_: NativeOutboundHttpError) -> NativeSchedulerCaptureError {
    NativeSchedulerCaptureError::Transport
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_edge::ForecastEvidenceOutcome;

    /// Exercise the ordinary genuine scheduler publish and response-terminal settlement path.
    #[test]
    fn native_scheduler_capture_publish_and_settle_actual_association() {
        let registry = NativeScheduledCaptureRegistry::default();
        let mut admission = NativeSchedulerCaptureAdmission::new(3000, registry.clone()).unwrap();
        // obtain the actual process-generated nonce from the registry's published attempt
        let _published = admission
            .publish_capture(ForecastSchedulerCaptureAdmissionRequest {
                due_key: "fixture-due",
                path: FORECAST_SCHEDULER_CAPTURE_PATH,
                timeout_ms: 5000,
            })
            .unwrap();
        let nonce = registry
            .entries
            .lock()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone();
        let mut target = DecodedNativeEdgeTarget {
            pathname: "/api/v1/sites/ballydidean/forecast".to_owned(),
            search: format!("?days=10&{PARAMETER}={nonce}"),
            query: vec![
                ("days".to_owned(), "10".to_owned()),
                (PARAMETER.to_owned(), nonce),
            ],
        };
        let claim = registry
            .claim("127.0.0.1".parse().unwrap(), "GET", &mut target)
            .unwrap();
        assert_eq!(claim.slot().due_key(), "fixture-due");
        assert_eq!(target.search, "?days=10");
        claim.settle(ForecastScheduleSettlement::Evidence(
            ForecastEvidenceOutcome::Created,
        ));
        assert!(registry.entries.lock().unwrap().is_empty());
    }
}
