//! Compose one scheduled forecast capture through the admitted public handler lifecycle.

use crate::forecast_response_capture::ForecastScheduleSettlement;
use crate::native_revision_spool::{ForecastSchedulerCaptureRequest, ForecastSchedulerTriggerPort};
use crate::public_api_proxy::{
    AdmittedPublicApiOrigin, PublicApiBodyReadOutcome, PublicApiHttpMethod, PublicApiHttpPort,
    PublicApiHttpRequest, PublicApiHttpResponse, PublicApiRedirectPolicy,
};

/// Exact HTTP and settlement timeout used by the source scheduler request.
pub const FORECAST_SCHEDULER_CAPTURE_TIMEOUT_MS: u64 = 30_000;

/// Preserve a stable source failure reason only when an injected port classified it.
pub trait ForecastSchedulerCaptureClassifiedError {
    /// Borrow the existing bounded reason or leave the scheduler to use its generic failure.
    fn stable_failure_reason(&self) -> Option<&str>;
}

/// Process-owned values needed to publish one externally managed capture association.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastSchedulerCaptureAdmissionRequest<'a> {
    pub due_key: &'a str,
    pub path: &'a str,
    pub timeout_ms: u64,
}

/// Terminal process-association result before the scheduler normalizes commit status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastSchedulerCaptureAwaitOutcome {
    Settled(ForecastScheduleSettlement),
    TimedOut,
}

/// Already-admitted loopback target paired with its externally managed attempt owner.
pub struct PublishedForecastSchedulerCapture<Attempt> {
    attempt: Attempt,
    origin: AdmittedPublicApiOrigin,
    pathname: String,
    search: String,
}

impl<Attempt> PublishedForecastSchedulerCapture<Attempt> {
    /// Retain a target parsed, nonce-associated, and approved by the injected admission boundary.
    pub fn admitted(
        attempt: Attempt,
        origin: AdmittedPublicApiOrigin,
        pathname: impl Into<String>,
        search: impl Into<String>,
    ) -> Self {
        Self {
            attempt,
            origin,
            pathname: pathname.into(),
            search: search.into(),
        }
    }

    /// Split the externally managed attempt from its already-admitted request target.
    fn into_parts(self) -> (Attempt, ForecastSchedulerCaptureTarget) {
        (
            self.attempt,
            ForecastSchedulerCaptureTarget {
                origin: self.origin,
                pathname: self.pathname,
                search: self.search,
            },
        )
    }
}

/// Nonce generation, process association, settlement, timeout, and cleanup boundary.
pub trait ForecastSchedulerCaptureAdmissionPort {
    type Attempt;
    type Error: ForecastSchedulerCaptureClassifiedError;

    /// Publish one process-owned association and its already-admitted public-handler target.
    fn publish_capture(
        &mut self,
        request: ForecastSchedulerCaptureAdmissionRequest<'_>,
    ) -> Result<PublishedForecastSchedulerCapture<Self::Attempt>, Self::Error>;

    /// Settle the attempt's request-failed result exactly once.
    fn settle_request_failed(&mut self, attempt: &mut Self::Attempt);

    /// Wait for response finish plus evidence and optional archive settlement.
    fn await_settlement(
        &mut self,
        attempt: &mut Self::Attempt,
    ) -> Result<ForecastSchedulerCaptureAwaitOutcome, Self::Error>;

    /// Remove the process association and its settlement timeout on every published outcome.
    fn cleanup_capture(&mut self, attempt: Self::Attempt);
}

/// Concrete local scheduler trigger over admission ownership and an admitted HTTP adapter.
pub struct ForecastSchedulerCaptureDriver<Admission, Http> {
    admission: Admission,
    http: Http,
}

impl<Admission, Http> ForecastSchedulerCaptureDriver<Admission, Http> {
    /// Bind the process admission owner and already-admitted local HTTP transport.
    pub const fn new(admission: Admission, http: Http) -> Self {
        Self { admission, http }
    }

    /// Recover both injected owners after the scheduler lifecycle.
    pub fn into_parts(self) -> (Admission, Http) {
        (self.admission, self.http)
    }
}

impl<Admission, Http> ForecastSchedulerTriggerPort
    for ForecastSchedulerCaptureDriver<Admission, Http>
where
    Admission: ForecastSchedulerCaptureAdmissionPort,
    Http: PublicApiHttpPort,
    Http::Error: ForecastSchedulerCaptureClassifiedError,
{
    /// Publish, drain, verify, settle, and clean up one scheduled public forecast request.
    fn trigger_forecast_capture(
        &mut self,
        request: ForecastSchedulerCaptureRequest,
    ) -> Result<ForecastScheduleSettlement, Option<String>> {
        let published = self
            .admission
            .publish_capture(ForecastSchedulerCaptureAdmissionRequest {
                due_key: request.slot.due_key(),
                path: request.path,
                timeout_ms: FORECAST_SCHEDULER_CAPTURE_TIMEOUT_MS,
            })
            .map_err(|error| classified_reason(&error))?;
        let (mut attempt, target) = published.into_parts();
        let result = self.execute_capture(&mut attempt, target);
        self.admission.cleanup_capture(attempt);
        result
    }
}

impl<Admission, Http> ForecastSchedulerCaptureDriver<Admission, Http>
where
    Admission: ForecastSchedulerCaptureAdmissionPort,
    Http: PublicApiHttpPort,
    Http::Error: ForecastSchedulerCaptureClassifiedError,
{
    /// Execute the complete request before observing status or awaiting evidence settlement.
    fn execute_capture(
        &mut self,
        attempt: &mut Admission::Attempt,
        target: ForecastSchedulerCaptureTarget,
    ) -> Result<ForecastScheduleSettlement, Option<String>> {
        let mut response = match self.http.execute(&PublicApiHttpRequest {
            accept: "application/json".to_owned(),
            method: PublicApiHttpMethod::Get,
            origin: target.origin,
            pathname: target.pathname,
            redirect: PublicApiRedirectPolicy::Manual,
            search: target.search,
            timeout_ms: FORECAST_SCHEDULER_CAPTURE_TIMEOUT_MS,
        }) {
            Ok(response) => response,
            Err(error) => {
                let reason = classified_reason(&error);
                return self.fail_capture(attempt, reason);
            }
        };
        let head = response.head();
        let drained = match response.read_body(&mut |_| true) {
            Ok(outcome) => outcome,
            Err(error) => {
                let reason = classified_reason(&error);
                return self.fail_capture(attempt, reason);
            }
        };

        // require the adapter to finish draining before status or settlement evaluation
        if drained != PublicApiBodyReadOutcome::Complete {
            return self.fail_capture(attempt, None);
        }
        // retain the source's stable category for a completed unsuccessful public response
        if !(200..300).contains(&head.status_code) {
            return self.fail_capture(attempt, Some("forecast_response_failed".to_owned()));
        }

        // wait only after the successful response body has been completely consumed
        match self.admission.await_settlement(attempt) {
            Ok(ForecastSchedulerCaptureAwaitOutcome::Settled(settlement)) => Ok(settlement),
            Ok(ForecastSchedulerCaptureAwaitOutcome::TimedOut) => {
                Err(Some("capture_not_committed".to_owned()))
            }
            Err(error) => {
                let reason = classified_reason(&error);
                self.fail_capture(attempt, reason)
            }
        }
    }

    /// Publish request failure once and preserve only an existing bounded reason.
    fn fail_capture(
        &mut self,
        attempt: &mut Admission::Attempt,
        reason: Option<String>,
    ) -> Result<ForecastScheduleSettlement, Option<String>> {
        self.admission.settle_request_failed(attempt);
        Err(reason)
    }
}

/// Already-admitted request fields produced with the process-owned nonce association.
struct ForecastSchedulerCaptureTarget {
    origin: AdmittedPublicApiOrigin,
    pathname: String,
    search: String,
}

/// Copy one port-classified stable reason into the scheduler's owned failure result.
fn classified_reason(error: &impl ForecastSchedulerCaptureClassifiedError) -> Option<String> {
    error.stable_failure_reason().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::convert::Infallible;
    use std::rc::Rc;

    use crate::forecast_response_capture::{
        AdmittedForecastScheduleCapture, ForecastCaptureChannel, ForecastEvidenceOutcome,
    };

    use super::*;

    const DUE_KEY: &str = "capture/2026-10-08T00:35:00.000Z";
    const PATH: &str = "/api/v1/sites/ballydidean/forecast?days=10";

    impl ForecastSchedulerCaptureClassifiedError for Infallible {
        /// Leave the impossible fixture error without an invented category.
        fn stable_failure_reason(&self) -> Option<&str> {
            match *self {}
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum HttpError {
        Timeout,
    }

    impl ForecastSchedulerCaptureClassifiedError for HttpError {
        /// Preserve one already-classified transport category from the injected adapter.
        fn stable_failure_reason(&self) -> Option<&str> {
            Some("request_timeout")
        }
    }

    struct Attempt;

    struct Admission {
        events: Rc<RefCell<Vec<&'static str>>>,
        settlement: Result<ForecastSchedulerCaptureAwaitOutcome, Infallible>,
    }

    impl ForecastSchedulerCaptureAdmissionPort for Admission {
        type Attempt = Attempt;
        type Error = Infallible;

        /// Publish the fixture's process-owned association and exact loopback target.
        fn publish_capture(
            &mut self,
            request: ForecastSchedulerCaptureAdmissionRequest<'_>,
        ) -> Result<PublishedForecastSchedulerCapture<Self::Attempt>, Self::Error> {
            assert_eq!(request.due_key, DUE_KEY);
            assert_eq!(request.path, PATH);
            assert_eq!(request.timeout_ms, 30_000);
            self.events.borrow_mut().push("publish");
            Ok(PublishedForecastSchedulerCapture::admitted(
                Attempt,
                AdmittedPublicApiOrigin::admitted("http://127.0.0.1:3000"),
                "/api/v1/sites/ballydidean/forecast",
                "?days=10&__weather_scheduler_nonce=owned",
            ))
        }

        /// Retain one idempotent request-failed settlement observation.
        fn settle_request_failed(&mut self, _attempt: &mut Self::Attempt) {
            self.events.borrow_mut().push("fail");
        }

        /// Return the configured finish/evidence/archive settlement.
        fn await_settlement(
            &mut self,
            _attempt: &mut Self::Attempt,
        ) -> Result<ForecastSchedulerCaptureAwaitOutcome, Self::Error> {
            self.events.borrow_mut().push("await");
            self.settlement
        }

        /// Remove the fixture association after every published terminal result.
        fn cleanup_capture(&mut self, _attempt: Self::Attempt) {
            self.events.borrow_mut().push("cleanup");
        }
    }

    #[derive(Clone, Copy)]
    enum Body {
        Complete,
        Error,
    }

    struct Http {
        body: Body,
        events: Rc<RefCell<Vec<&'static str>>>,
        request: Rc<RefCell<Option<PublicApiHttpRequest>>>,
        status_code: u16,
    }

    impl PublicApiHttpPort for Http {
        type Error = HttpError;
        type Response = Response;

        /// Observe the exact admitted local request without opening a socket.
        fn execute(&self, request: &PublicApiHttpRequest) -> Result<Self::Response, Self::Error> {
            self.events.borrow_mut().push("http");
            self.request.replace(Some(request.clone()));
            Ok(Response {
                body: self.body,
                events: Rc::clone(&self.events),
                status_code: self.status_code,
            })
        }
    }

    struct Response {
        body: Body,
        events: Rc<RefCell<Vec<&'static str>>>,
        status_code: u16,
    }

    struct Fixture {
        driver: ForecastSchedulerCaptureDriver<Admission, Http>,
        events: Rc<RefCell<Vec<&'static str>>>,
        request: Rc<RefCell<Option<PublicApiHttpRequest>>>,
    }

    impl PublicApiHttpResponse for Response {
        type Error = HttpError;

        /// Return the configured ordinary public-handler status.
        fn head(&self) -> crate::public_api_proxy::PublicApiHttpResponseHead {
            crate::public_api_proxy::PublicApiHttpResponseHead {
                content_length: None,
                content_type: Some("application/json".to_owned()),
                status_code: self.status_code,
            }
        }

        /// Drain without retaining bytes or return the configured transport timeout.
        fn read_body(
            &mut self,
            receiver: &mut dyn FnMut(&[u8]) -> bool,
        ) -> Result<PublicApiBodyReadOutcome, Self::Error> {
            self.events.borrow_mut().push("drain");
            match self.body {
                Body::Complete => {
                    assert!(receiver(b"forecast"));
                    Ok(PublicApiBodyReadOutcome::Complete)
                }
                Body::Error => Err(HttpError::Timeout),
            }
        }
    }

    /// Drain a successful response before awaiting and returning its durable settlement.
    #[test]
    fn success_drains_then_awaits_settlement_and_cleans_up() {
        let Fixture {
            mut driver,
            events,
            request,
        } = fixture(
            200,
            Body::Complete,
            Ok(ForecastSchedulerCaptureAwaitOutcome::Settled(
                ForecastScheduleSettlement::Evidence(ForecastEvidenceOutcome::Created),
            )),
        );
        assert_eq!(
            driver.trigger_forecast_capture(capture_request()),
            Ok(ForecastScheduleSettlement::Evidence(
                ForecastEvidenceOutcome::Created
            ))
        );
        assert_eq!(
            events.borrow().as_slice(),
            ["publish", "http", "drain", "await", "cleanup"]
        );
        assert_eq!(
            request.borrow().as_ref(),
            Some(&PublicApiHttpRequest {
                accept: "application/json".to_owned(),
                method: PublicApiHttpMethod::Get,
                origin: AdmittedPublicApiOrigin::admitted("http://127.0.0.1:3000"),
                pathname: "/api/v1/sites/ballydidean/forecast".to_owned(),
                redirect: PublicApiRedirectPolicy::Manual,
                search: "?days=10&__weather_scheduler_nonce=owned".to_owned(),
                timeout_ms: 30_000,
            })
        );
    }

    /// Drain an unsuccessful response before publishing the source failure category.
    #[test]
    fn unsuccessful_status_drains_then_fails_and_cleans_up() {
        let Fixture {
            mut driver, events, ..
        } = fixture(
            503,
            Body::Complete,
            Ok(ForecastSchedulerCaptureAwaitOutcome::Settled(
                ForecastScheduleSettlement::Evidence(ForecastEvidenceOutcome::Created),
            )),
        );
        assert_eq!(
            driver.trigger_forecast_capture(capture_request()),
            Err(Some("forecast_response_failed".to_owned()))
        );
        assert_eq!(
            events.borrow().as_slice(),
            ["publish", "http", "drain", "fail", "cleanup"]
        );
    }

    /// Preserve a classified body-read timeout and clean up without awaiting evidence.
    #[test]
    fn body_read_failure_settles_request_failure_and_cleans_up() {
        let Fixture {
            mut driver, events, ..
        } = fixture(
            200,
            Body::Error,
            Ok(ForecastSchedulerCaptureAwaitOutcome::Settled(
                ForecastScheduleSettlement::Evidence(ForecastEvidenceOutcome::Created),
            )),
        );
        assert_eq!(
            driver.trigger_forecast_capture(capture_request()),
            Err(Some("request_timeout".to_owned()))
        );
        assert_eq!(
            events.borrow().as_slice(),
            ["publish", "http", "drain", "fail", "cleanup"]
        );
    }

    /// Preserve capture-timeout classification after successful response drain and cleanup.
    #[test]
    fn settlement_timeout_fails_once_then_cleans_up() {
        let Fixture {
            mut driver, events, ..
        } = fixture(
            200,
            Body::Complete,
            Ok(ForecastSchedulerCaptureAwaitOutcome::TimedOut),
        );
        assert_eq!(
            driver.trigger_forecast_capture(capture_request()),
            Err(Some("capture_not_committed".to_owned()))
        );
        assert_eq!(
            events.borrow().as_slice(),
            ["publish", "http", "drain", "await", "cleanup"]
        );
    }

    /// Build one ordinary injected driver without sockets, timers, or external state.
    fn fixture(
        status_code: u16,
        body: Body,
        settlement: Result<ForecastSchedulerCaptureAwaitOutcome, Infallible>,
    ) -> Fixture {
        let events = Rc::new(RefCell::new(Vec::new()));
        let request = Rc::new(RefCell::new(None));
        let admission = Admission {
            events: Rc::clone(&events),
            settlement,
        };
        let http = Http {
            body,
            events: Rc::clone(&events),
            request: Rc::clone(&request),
            status_code,
        };
        Fixture {
            driver: ForecastSchedulerCaptureDriver::new(admission, http),
            events,
            request,
        }
    }

    /// Build the exact request shape emitted by the native scheduler.
    fn capture_request() -> ForecastSchedulerCaptureRequest {
        ForecastSchedulerCaptureRequest {
            slot: AdmittedForecastScheduleCapture::admitted(DUE_KEY),
            commit_at: "2026-10-08T00:40:00.000Z".to_owned(),
            channel: ForecastCaptureChannel::SchedulerRequest,
            path: PATH,
        }
    }
}
