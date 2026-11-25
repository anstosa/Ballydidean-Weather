//! Bounded Xweather tile transport behind admitted target and HTTP ports.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::{
    XweatherTileCacheRequest, XweatherTileFrame, XweatherTileLayer, XweatherTileLoad,
    XweatherTileLoader, XweatherTilePhase,
};

/// exact deployed response byte limit for one provider tile.
pub const MAXIMUM_XWEATHER_TILE_BYTES: usize = 4 * 1024 * 1024;

/// exact deployed provider request timeout.
pub const XWEATHER_TILE_TIMEOUT_MS: u64 = 8_000;

/// one already-validated provider target produced by the URL and credential owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedXweatherTileTarget(String);

impl AdmittedXweatherTileTarget {
    /// retain one complete target admitted by the owning encoder.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// borrow the opaque admitted target.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// exact deployed provider product selected from phase and public layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XweatherTileProduct {
    ForecastClouds,
    ForecastPrecipitation,
    ForecastRadar,
    ForecastWind,
    HistoryClouds,
    HistoryPrecipitation,
    HistoryRadar,
    HistoryWind,
}

impl XweatherTileProduct {
    /// return the provider's exact deployed layer segment.
    pub const fn wire_value(self) -> &'static str {
        // preserve the source phase and layer table
        match self {
            Self::ForecastClouds => "fsatellite",
            Self::ForecastPrecipitation => "fqpf-1h",
            Self::ForecastRadar => "fradar",
            Self::ForecastWind => "fwind-speeds",
            Self::HistoryClouds => "satellite-geocolor",
            Self::HistoryPrecipitation => "precip-1h",
            Self::HistoryRadar => "radar",
            Self::HistoryWind => "wind-speeds",
        }
    }
}

/// typed frame components handed to the approved target encoder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XweatherTileTargetInput<'a> {
    pub frame: &'a XweatherTileFrame,
    pub phase: XweatherTilePhase,
    pub product: XweatherTileProduct,
}

/// URL construction and credential encoding boundary.
pub trait XweatherTileTargetPort {
    type Error;

    /// produce one complete credentialed target from typed admitted components.
    fn build_target(
        &self,
        input: XweatherTileTargetInput<'_>,
    ) -> Result<AdmittedXweatherTileTarget, Self::Error>;
}

/// the only transport method used by the deployed tile fetch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XweatherTileHttpMethod {
    Get,
}

/// deployed fetch redirect behavior for provider tiles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XweatherTileRedirectPolicy {
    Follow,
}

/// one exact provider request after URL and credential admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XweatherTileHttpRequest {
    pub accept: &'static str,
    pub method: XweatherTileHttpMethod,
    pub redirect: XweatherTileRedirectPolicy,
    pub target: AdmittedXweatherTileTarget,
    pub timeout_ms: u64,
    pub user_agent: &'static str,
}

/// typed response metadata decoded by the HTTP adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct XweatherTileHttpResponseHead {
    pub content_type: Option<String>,
    pub retry_after_seconds: Option<f64>,
    pub status_code: u16,
}

/// result of one adapter-owned streaming body read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XweatherTileBodyReadOutcome {
    Complete,
    ReceiverStopped,
}

/// one provider response whose body remains streaming and adapter-owned.
pub trait XweatherTileHttpResponse {
    type Error;

    /// return decoded status and header metadata before any body read.
    fn head(&self) -> XweatherTileHttpResponseHead;

    /// stream response chunks until EOF or bounded receiver refusal.
    fn read_body(
        &mut self,
        receiver: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<XweatherTileBodyReadOutcome, Self::Error>;
}

/// safe deployed transport failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XweatherTileTransportFailureKind {
    Error,
    TypeError,
}

impl XweatherTileTransportFailureKind {
    /// return the exact safe diagnostic value.
    pub const fn wire_value(self) -> &'static str {
        // preserve the source's two diagnostic categories
        match self {
            Self::Error => "Error",
            Self::TypeError => "TypeError",
        }
    }
}

/// HTTP framing, TLS, timeout, and network implementation boundary.
pub trait XweatherTileHttpPort {
    type Error;
    type Response: XweatherTileHttpResponse<Error = Self::Error>;

    /// execute one already-admitted provider request.
    fn execute(&self, request: &XweatherTileHttpRequest) -> Result<Self::Response, Self::Error>;

    /// classify one failure without exposing its message or target.
    fn failure_kind(&self, error: &Self::Error) -> XweatherTileTransportFailureKind;
}

/// reviewed content-type categories retained in provider diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XweatherTileDiagnosticContentType {
    ApplicationJson,
    ImagePng,
    Missing,
    Other,
    TextHtml,
    TextPlain,
}

impl XweatherTileDiagnosticContentType {
    /// return the exact safe diagnostic value.
    pub const fn wire_value(self) -> &'static str {
        // preserve the source allowlist and fallback labels
        match self {
            Self::ApplicationJson => "application/json",
            Self::ImagePng => "image/png",
            Self::Missing => "missing",
            Self::Other => "other",
            Self::TextHtml => "text/html",
            Self::TextPlain => "text/plain",
        }
    }
}

/// safe diagnostic sink kept independent from stderr and logging choices.
pub trait XweatherTileDiagnostics {
    /// record one fetch rejection without credential or target material.
    fn transport_failed(&self, kind: XweatherTileTransportFailureKind);

    /// record one invalid response using only reviewed categories.
    fn response_rejected(&self, status_code: u16, content_type: XweatherTileDiagnosticContentType);
}

/// one bounded target, transport, response, or body failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XweatherTileTransportError<TargetError, HttpError> {
    Http(HttpError),
    InvalidResponse { retry_after_ms: Option<u64> },
    ResponseLimitExceeded,
    Target(TargetError),
    UnsupportedLoad,
}

impl<TargetError, HttpError> XweatherTileTransportError<TargetError, HttpError> {
    /// return the provider's one bounded retry delay when present.
    pub const fn retry_after_ms(&self) -> Option<u64> {
        // retain retry metadata only for rejected provider responses
        match self {
            Self::InvalidResponse { retry_after_ms } => *retry_after_ms,
            Self::Http(_)
            | Self::ResponseLimitExceeded
            | Self::Target(_)
            | Self::UnsupportedLoad => None,
        }
    }
}

impl<TargetError: Display, HttpError: Display> Display
    for XweatherTileTransportError<TargetError, HttpError>
{
    /// render the source-compatible bounded failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve injected failures and exact edge-owned messages
        match self {
            Self::Http(error) => Display::fmt(error, formatter),
            Self::InvalidResponse { .. } => {
                formatter.write_str("Xweather tile provider returned an invalid response")
            }
            Self::ResponseLimitExceeded => {
                formatter.write_str("weather map response exceeded the edge limit")
            }
            Self::Target(error) => Display::fmt(error, formatter),
            Self::UnsupportedLoad => {
                formatter.write_str("Xweather tile loader requires a parsed frame")
            }
        }
    }
}

impl<TargetError, HttpError> Error for XweatherTileTransportError<TargetError, HttpError>
where
    TargetError: Error + 'static,
    HttpError: Error + 'static,
{
    /// return only injected adapter failures as underlying sources.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // expose no invented source for edge-owned refusals
        match self {
            Self::Http(error) => Some(error),
            Self::Target(error) => Some(error),
            Self::InvalidResponse { .. } | Self::ResponseLimitExceeded | Self::UnsupportedLoad => {
                None
            }
        }
    }
}

/// concrete frame loader used behind the existing cache and usage budget.
pub struct BoundedXweatherTileTransport<Targets, Http, Diagnostics> {
    diagnostics: Diagnostics,
    http: Http,
    targets: Targets,
}

impl<Targets, Http, Diagnostics> BoundedXweatherTileTransport<Targets, Http, Diagnostics> {
    /// bind approved target, HTTP, and diagnostic owners.
    pub const fn new(targets: Targets, http: Http, diagnostics: Diagnostics) -> Self {
        Self {
            diagnostics,
            http,
            targets,
        }
    }

    /// recover all bound owners after the serving lifecycle.
    pub fn into_parts(self) -> (Targets, Http, Diagnostics) {
        (self.targets, self.http, self.diagnostics)
    }
}

impl<Targets, Http, Diagnostics> XweatherTileLoader
    for BoundedXweatherTileTransport<Targets, Http, Diagnostics>
where
    Targets: XweatherTileTargetPort,
    Http: XweatherTileHttpPort,
    Diagnostics: XweatherTileDiagnostics,
{
    type Error = XweatherTileTransportError<Targets::Error, Http::Error>;

    /// fetch one parsed frame with the deployed bounded provider contract.
    fn load_tile(&self, request: &XweatherTileCacheRequest) -> Result<Vec<u8>, Self::Error> {
        let XweatherTileLoad::Frame(frame) = &request.tile else {
            return Err(XweatherTileTransportError::UnsupportedLoad);
        };
        let product = xweather_tile_product(request.phase, frame.layer);
        let target = self
            .targets
            .build_target(XweatherTileTargetInput {
                frame,
                phase: request.phase,
                product,
            })
            .map_err(XweatherTileTransportError::Target)?;
        let mut response = match self.http.execute(&XweatherTileHttpRequest {
            accept: "image/png",
            method: XweatherTileHttpMethod::Get,
            redirect: XweatherTileRedirectPolicy::Follow,
            target,
            timeout_ms: XWEATHER_TILE_TIMEOUT_MS,
            user_agent: "Ballydidean-Weather/1.0",
        }) {
            Ok(response) => response,
            Err(error) => {
                // report only the source's safe transport category
                self.diagnostics
                    .transport_failed(self.http.failure_kind(&error));
                return Err(XweatherTileTransportError::Http(error));
            }
        };
        let head = response.head();
        let successful = (200..300).contains(&head.status_code);
        let png = head
            .content_type
            .as_deref()
            .is_some_and(|value| value.starts_with("image/png"));

        // reject status and representation failures before reading any bytes
        if !successful || !png {
            let content_type = diagnostic_content_type(head.content_type.as_deref());
            self.diagnostics
                .response_rejected(head.status_code, content_type);
            return Err(XweatherTileTransportError::InvalidResponse {
                retry_after_ms: bounded_retry_after_ms(head.retry_after_seconds),
            });
        }

        let mut body = Vec::new();
        let mut receiver_stopped = false;
        let outcome = response
            .read_body(&mut |chunk| {
                // reject a cumulative length that cannot be represented
                let Some(next_length) = body.len().checked_add(chunk.len()) else {
                    receiver_stopped = true;
                    return false;
                };

                // stop before retaining the first byte beyond four MiB
                if next_length > MAXIMUM_XWEATHER_TILE_BYTES {
                    receiver_stopped = true;
                    return false;
                }
                body.extend_from_slice(chunk);
                true
            })
            .map_err(XweatherTileTransportError::Http)?;

        // require the adapter to honor bounded receiver cancellation
        if receiver_stopped || outcome == XweatherTileBodyReadOutcome::ReceiverStopped {
            return Err(XweatherTileTransportError::ResponseLimitExceeded);
        }
        Ok(body)
    }
}

/// select the exact source product from the admitted phase and layer.
const fn xweather_tile_product(
    phase: XweatherTilePhase,
    layer: XweatherTileLayer,
) -> XweatherTileProduct {
    // preserve the deployed nested product table
    match (phase, layer) {
        (XweatherTilePhase::Forecast, XweatherTileLayer::Clouds) => {
            XweatherTileProduct::ForecastClouds
        }
        (XweatherTilePhase::Forecast, XweatherTileLayer::Precipitation) => {
            XweatherTileProduct::ForecastPrecipitation
        }
        (XweatherTilePhase::Forecast, XweatherTileLayer::Radar) => {
            XweatherTileProduct::ForecastRadar
        }
        (XweatherTilePhase::Forecast, XweatherTileLayer::Wind) => XweatherTileProduct::ForecastWind,
        (XweatherTilePhase::History, XweatherTileLayer::Clouds) => {
            XweatherTileProduct::HistoryClouds
        }
        (XweatherTilePhase::History, XweatherTileLayer::Precipitation) => {
            XweatherTileProduct::HistoryPrecipitation
        }
        (XweatherTilePhase::History, XweatherTileLayer::Radar) => XweatherTileProduct::HistoryRadar,
        (XweatherTilePhase::History, XweatherTileLayer::Wind) => XweatherTileProduct::HistoryWind,
    }
}

/// classify one decoded provider content type without retaining parameters.
fn diagnostic_content_type(value: Option<&str>) -> XweatherTileDiagnosticContentType {
    let Some(value) = value else {
        return XweatherTileDiagnosticContentType::Missing;
    };
    let media_type = value
        .split_once(';')
        .map_or(value, |(media_type, _)| media_type)
        .trim()
        .to_ascii_lowercase();

    // retain only the source diagnostic allowlist
    match media_type.as_str() {
        "application/json" => XweatherTileDiagnosticContentType::ApplicationJson,
        "image/png" => XweatherTileDiagnosticContentType::ImagePng,
        "text/html" => XweatherTileDiagnosticContentType::TextHtml,
        "text/plain" => XweatherTileDiagnosticContentType::TextPlain,
        _ => XweatherTileDiagnosticContentType::Other,
    }
}

/// convert one adapter-decoded retry header to the source's bounded delay.
fn bounded_retry_after_ms(retry_after_seconds: Option<f64>) -> Option<u64> {
    let seconds = retry_after_seconds?;

    // accept only positive finite numeric retry values
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    Some((seconds * 1_000.0).ceil().min(60_000.0) as u64)
}
