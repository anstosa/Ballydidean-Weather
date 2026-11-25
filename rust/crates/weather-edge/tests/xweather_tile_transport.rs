use std::cell::RefCell;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::rc::Rc;

use weather_edge::{
    AdmittedXweatherTileTarget, BoundedXweatherTileTransport, MAXIMUM_XWEATHER_TILE_BYTES,
    XWEATHER_TILE_TIMEOUT_MS, XweatherMapUnits, XweatherTileBodyReadOutcome, XweatherTileCacheKey,
    XweatherTileCacheRequest, XweatherTileDiagnosticContentType, XweatherTileDiagnostics,
    XweatherTileFrame, XweatherTileHttpMethod, XweatherTileHttpPort, XweatherTileHttpRequest,
    XweatherTileHttpResponse, XweatherTileHttpResponseHead, XweatherTileLayer, XweatherTileLoad,
    XweatherTileLoader, XweatherTilePhase, XweatherTileProduct, XweatherTileRedirectPolicy,
    XweatherTileTargetInput, XweatherTileTargetPort, XweatherTileTransportError,
    XweatherTileTransportFailureKind,
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// render one injected adapter failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

#[derive(Clone, Debug, PartialEq)]
struct CapturedTarget {
    frame: XweatherTileFrame,
    phase: XweatherTilePhase,
    product: XweatherTileProduct,
}

#[derive(Clone, Default)]
struct FixtureTargets {
    captured: Rc<RefCell<Vec<CapturedTarget>>>,
}

impl XweatherTileTargetPort for FixtureTargets {
    type Error = FixtureError;

    /// capture typed components and return one externally encoded target.
    fn build_target(
        &self,
        input: XweatherTileTargetInput<'_>,
    ) -> Result<AdmittedXweatherTileTarget, Self::Error> {
        self.captured.borrow_mut().push(CapturedTarget {
            frame: input.frame.clone(),
            phase: input.phase,
            product: input.product,
        });
        Ok(AdmittedXweatherTileTarget::admitted(format!(
            "opaque-target/{}",
            input.product.wire_value()
        )))
    }
}

#[derive(Clone)]
enum FixtureHttpPlan {
    Error(FixtureError),
    Response {
        chunks: Vec<Vec<u8>>,
        head: XweatherTileHttpResponseHead,
    },
}

#[derive(Clone, Default)]
struct FixtureHttpState {
    cancelled: Rc<RefCell<usize>>,
    reads: Rc<RefCell<usize>>,
    requests: Rc<RefCell<Vec<XweatherTileHttpRequest>>>,
}

#[derive(Clone)]
struct FixtureHttp {
    failure_kind: XweatherTileTransportFailureKind,
    plan: FixtureHttpPlan,
    state: FixtureHttpState,
}

struct FixtureResponse {
    chunks: VecDeque<Vec<u8>>,
    head: XweatherTileHttpResponseHead,
    state: FixtureHttpState,
}

impl XweatherTileHttpResponse for FixtureResponse {
    type Error = FixtureError;

    /// return the injected typed response metadata.
    fn head(&self) -> XweatherTileHttpResponseHead {
        self.head.clone()
    }

    /// offer physical chunks until EOF or receiver cancellation.
    fn read_body(
        &mut self,
        receiver: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<XweatherTileBodyReadOutcome, Self::Error> {
        // preserve physical provider chunk order
        while let Some(chunk) = self.chunks.pop_front() {
            *self.state.reads.borrow_mut() += 1;

            // stop immediately when the bounded receiver refuses a chunk
            if !receiver(&chunk) {
                *self.state.cancelled.borrow_mut() += 1;
                return Ok(XweatherTileBodyReadOutcome::ReceiverStopped);
            }
        }
        Ok(XweatherTileBodyReadOutcome::Complete)
    }
}

impl XweatherTileHttpPort for FixtureHttp {
    type Error = FixtureError;
    type Response = FixtureResponse;

    /// capture the exact request and return the selected provider outcome.
    fn execute(&self, request: &XweatherTileHttpRequest) -> Result<Self::Response, Self::Error> {
        self.state.requests.borrow_mut().push(request.clone());

        // preserve the injected transport outcome
        match &self.plan {
            FixtureHttpPlan::Error(error) => Err(error.clone()),
            FixtureHttpPlan::Response { chunks, head } => Ok(FixtureResponse {
                chunks: chunks.clone().into(),
                head: head.clone(),
                state: self.state.clone(),
            }),
        }
    }

    /// return the injected safe fetch failure category.
    fn failure_kind(&self, _error: &Self::Error) -> XweatherTileTransportFailureKind {
        self.failure_kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DiagnosticEvent {
    ResponseRejected(u16, XweatherTileDiagnosticContentType),
    TransportFailed(XweatherTileTransportFailureKind),
}

#[derive(Clone, Default)]
struct FixtureDiagnostics {
    events: Rc<RefCell<Vec<DiagnosticEvent>>>,
}

impl XweatherTileDiagnostics for FixtureDiagnostics {
    /// retain one safe transport category.
    fn transport_failed(&self, kind: XweatherTileTransportFailureKind) {
        self.events
            .borrow_mut()
            .push(DiagnosticEvent::TransportFailed(kind));
    }

    /// retain only safe status and content-type response diagnostics.
    fn response_rejected(&self, status_code: u16, content_type: XweatherTileDiagnosticContentType) {
        self.events
            .borrow_mut()
            .push(DiagnosticEvent::ResponseRejected(status_code, content_type));
    }
}

/// build one exact parsed provider frame request.
fn request(phase: XweatherTilePhase, layer: XweatherTileLayer) -> XweatherTileCacheRequest {
    XweatherTileCacheRequest {
        key: XweatherTileCacheKey::admitted(format!("{phase:?}-{layer:?}")),
        phase,
        provider_units: XweatherMapUnits::admitted(1).expect("one provider unit"),
        tile: XweatherTileLoad::Frame(XweatherTileFrame {
            height: 168,
            latitude: 56.123_456,
            layer,
            longitude: -3.654_321,
            valid_time: "20261123160000".to_owned(),
            width: 256,
            zoom: 10,
        }),
    }
}

/// build one successful PNG fixture plan.
fn successful_plan(chunks: Vec<Vec<u8>>) -> FixtureHttpPlan {
    FixtureHttpPlan::Response {
        chunks,
        head: XweatherTileHttpResponseHead {
            content_type: Some("image/png".to_owned()),
            retry_after_seconds: None,
            status_code: 200,
        },
    }
}

/// prove all phase products and the exact deployed request metadata.
#[test]
fn maps_all_products_and_streams_png_bytes() {
    let targets = FixtureTargets::default();
    let http_state = FixtureHttpState::default();
    let diagnostics = FixtureDiagnostics::default();
    let transport = BoundedXweatherTileTransport::new(
        targets.clone(),
        FixtureHttp {
            failure_kind: XweatherTileTransportFailureKind::Error,
            plan: successful_plan(vec![vec![137, 80], vec![78, 71]]),
            state: http_state.clone(),
        },
        diagnostics.clone(),
    );
    let cases = [
        (
            XweatherTilePhase::Forecast,
            XweatherTileLayer::Clouds,
            XweatherTileProduct::ForecastClouds,
        ),
        (
            XweatherTilePhase::Forecast,
            XweatherTileLayer::Precipitation,
            XweatherTileProduct::ForecastPrecipitation,
        ),
        (
            XweatherTilePhase::Forecast,
            XweatherTileLayer::Radar,
            XweatherTileProduct::ForecastRadar,
        ),
        (
            XweatherTilePhase::Forecast,
            XweatherTileLayer::Wind,
            XweatherTileProduct::ForecastWind,
        ),
        (
            XweatherTilePhase::History,
            XweatherTileLayer::Clouds,
            XweatherTileProduct::HistoryClouds,
        ),
        (
            XweatherTilePhase::History,
            XweatherTileLayer::Precipitation,
            XweatherTileProduct::HistoryPrecipitation,
        ),
        (
            XweatherTilePhase::History,
            XweatherTileLayer::Radar,
            XweatherTileProduct::HistoryRadar,
        ),
        (
            XweatherTilePhase::History,
            XweatherTileLayer::Wind,
            XweatherTileProduct::HistoryWind,
        ),
    ];

    // prove every deployed nested table entry
    for (phase, layer, _) in cases {
        assert_eq!(
            transport
                .load_tile(&request(phase, layer))
                .expect("bounded PNG"),
            [137, 80, 78, 71]
        );
    }

    assert_eq!(
        targets
            .captured
            .borrow()
            .iter()
            .map(|target| target.product)
            .collect::<Vec<_>>(),
        cases
            .into_iter()
            .map(|(_, _, product)| product)
            .collect::<Vec<_>>()
    );
    assert!(targets.captured.borrow().iter().all(|target| {
        target.frame.width == 256
            && target.frame.height == 168
            && target.frame.latitude == 56.123_456
            && target.frame.longitude == -3.654_321
            && target.frame.zoom == 10
            && target.frame.valid_time == "20261123160000"
    }));
    let requests = http_state.requests.borrow();
    assert_eq!(requests.len(), cases.len());
    assert!(requests.iter().all(|request| request
        == &XweatherTileHttpRequest {
            accept: "image/png",
            method: XweatherTileHttpMethod::Get,
            redirect: XweatherTileRedirectPolicy::Follow,
            target: request.target.clone(),
            timeout_ms: XWEATHER_TILE_TIMEOUT_MS,
            user_agent: "Ballydidean-Weather/1.0",
        }));
    assert_eq!(
        requests
            .iter()
            .map(|request| request.target.as_str())
            .collect::<Vec<_>>(),
        [
            "opaque-target/fsatellite",
            "opaque-target/fqpf-1h",
            "opaque-target/fradar",
            "opaque-target/fwind-speeds",
            "opaque-target/satellite-geocolor",
            "opaque-target/precip-1h",
            "opaque-target/radar",
            "opaque-target/wind-speeds",
        ]
    );
    assert!(diagnostics.events.borrow().is_empty());
}

/// prove invalid responses remain unread and retain safe retry diagnostics.
#[test]
fn rejects_invalid_response_with_bounded_retry_metadata() {
    let http_state = FixtureHttpState::default();
    let diagnostics = FixtureDiagnostics::default();
    let transport = BoundedXweatherTileTransport::new(
        FixtureTargets::default(),
        FixtureHttp {
            failure_kind: XweatherTileTransportFailureKind::Error,
            plan: FixtureHttpPlan::Response {
                chunks: vec![b"must-not-read".to_vec()],
                head: XweatherTileHttpResponseHead {
                    content_type: Some(" Application/JSON ; charset=utf-8".to_owned()),
                    retry_after_seconds: Some(60.5),
                    status_code: 503,
                },
            },
            state: http_state.clone(),
        },
        diagnostics.clone(),
    );
    let error = transport
        .load_tile(&request(
            XweatherTilePhase::Forecast,
            XweatherTileLayer::Wind,
        ))
        .expect_err("invalid response");

    assert_eq!(error.retry_after_ms(), Some(60_000));
    assert_eq!(
        error,
        XweatherTileTransportError::InvalidResponse {
            retry_after_ms: Some(60_000),
        }
    );
    assert_eq!(*http_state.reads.borrow(), 0);
    assert_eq!(
        diagnostics.events.borrow().as_slice(),
        [DiagnosticEvent::ResponseRejected(
            503,
            XweatherTileDiagnosticContentType::ApplicationJson,
        )]
    );
}

/// prove fetch failures expose only the reviewed diagnostic category.
#[test]
fn classifies_transport_failure_without_target_material() {
    let diagnostics = FixtureDiagnostics::default();
    let transport = BoundedXweatherTileTransport::new(
        FixtureTargets::default(),
        FixtureHttp {
            failure_kind: XweatherTileTransportFailureKind::TypeError,
            plan: FixtureHttpPlan::Error(FixtureError("opaque transport failure")),
            state: FixtureHttpState::default(),
        },
        diagnostics.clone(),
    );
    let error = transport
        .load_tile(&request(
            XweatherTilePhase::Forecast,
            XweatherTileLayer::Radar,
        ))
        .expect_err("transport failure");

    assert_eq!(
        error,
        XweatherTileTransportError::Http(FixtureError("opaque transport failure"))
    );
    assert_eq!(
        diagnostics.events.borrow().as_slice(),
        [DiagnosticEvent::TransportFailed(
            XweatherTileTransportFailureKind::TypeError,
        )]
    );
}

/// prove streaming stops before retaining a byte beyond four MiB.
#[test]
fn cancels_an_oversized_png_stream() {
    let http_state = FixtureHttpState::default();
    let transport = BoundedXweatherTileTransport::new(
        FixtureTargets::default(),
        FixtureHttp {
            failure_kind: XweatherTileTransportFailureKind::Error,
            plan: successful_plan(vec![vec![1; MAXIMUM_XWEATHER_TILE_BYTES], vec![2]]),
            state: http_state.clone(),
        },
        FixtureDiagnostics::default(),
    );
    let error = transport
        .load_tile(&request(
            XweatherTilePhase::History,
            XweatherTileLayer::Clouds,
        ))
        .expect_err("oversized response");

    assert_eq!(error, XweatherTileTransportError::ResponseLimitExceeded);
    assert_eq!(*http_state.reads.borrow(), 2);
    assert_eq!(*http_state.cancelled.borrow(), 1);
}

/// prove impossible opaque loads never reach target or transport owners.
#[test]
fn rejects_an_opaque_load_before_external_ports() {
    let targets = FixtureTargets::default();
    let http_state = FixtureHttpState::default();
    let transport = BoundedXweatherTileTransport::new(
        targets.clone(),
        FixtureHttp {
            failure_kind: XweatherTileTransportFailureKind::Error,
            plan: successful_plan(Vec::new()),
            state: http_state.clone(),
        },
        FixtureDiagnostics::default(),
    );
    let error = transport
        .load_tile(&XweatherTileCacheRequest {
            key: XweatherTileCacheKey::admitted("opaque"),
            phase: XweatherTilePhase::Forecast,
            provider_units: XweatherMapUnits::admitted(1).expect("one provider unit"),
            tile: XweatherTileLoad::Opaque,
        })
        .expect_err("opaque load");

    assert_eq!(error, XweatherTileTransportError::UnsupportedLoad);
    assert!(targets.captured.borrow().is_empty());
    assert!(http_state.requests.borrow().is_empty());
}
