//! Native target, HTTP, and safe diagnostics for Xweather raster tiles.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use hyper::Method;
use hyper::header::{ACCEPT, CONTENT_TYPE, HeaderValue, RETRY_AFTER, USER_AGENT};
use url::Url;
use weather_edge::{
    AdmittedXweatherTileTarget, EdgeXweatherCredentials, MAXIMUM_XWEATHER_TILE_BYTES,
    XWEATHER_TILE_TIMEOUT_MS, XweatherTileBodyReadOutcome, XweatherTileDiagnosticContentType,
    XweatherTileDiagnostics, XweatherTileHttpMethod, XweatherTileHttpPort, XweatherTileHttpRequest,
    XweatherTileHttpResponse, XweatherTileHttpResponseHead, XweatherTileRedirectPolicy,
    XweatherTileTargetInput, XweatherTileTargetPort, XweatherTileTransportFailureKind,
};

use crate::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
    NativeOutboundHttpResponse,
};

const RESPONSE_CHUNK_BYTES: usize = 16 * 1024;

/// Credential-owning target encoder bound to one admitted provider origin.
pub struct NativeXweatherTileTargets {
    credentials: EdgeXweatherCredentials,
    origin: Url,
}

impl NativeXweatherTileTargets {
    /// Bind one complete credential pair to an independently revalidated map origin.
    pub fn new(
        origin: Url,
        credentials: EdgeXweatherCredentials,
    ) -> Result<Self, NativeOutboundHttpError> {
        validate_origin(&origin)?;
        Ok(Self {
            credentials,
            origin,
        })
    }
}

impl XweatherTileTargetPort for NativeXweatherTileTargets {
    type Error = NativeOutboundHttpError;

    /// Encode the original credential product geometry and valid-time path.
    fn build_target(
        &self,
        input: XweatherTileTargetInput<'_>,
    ) -> Result<AdmittedXweatherTileTarget, Self::Error> {
        validate_frame(input.frame)?;
        let credential = format!(
            "{}_{}",
            encode_uri_component(self.credentials.client_id()),
            encode_uri_component(self.credentials.client_secret()),
        );
        let path = format!(
            "{credential}/{}/{}x{}/{:.6},{:.6},{}/{}.png",
            input.product.wire_value(),
            input.frame.width,
            input.frame.height,
            input.frame.latitude,
            input.frame.longitude,
            input.frame.zoom,
            input.frame.valid_time,
        );
        let target = self
            .origin
            .join(&path)
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        // prevent encoded credential or path material from changing the admitted provider origin
        if target.origin() != self.origin.origin()
            || target.query().is_some()
            || target.fragment().is_some()
        {
            return Err(NativeOutboundHttpError::InvalidRequest);
        }
        Ok(AdmittedXweatherTileTarget::admitted(target.to_string()))
    }
}

/// Shared process HTTP adapter for the edge-owned bounded tile response policy.
#[derive(Clone)]
pub struct NativeXweatherTileHttp {
    client: Arc<NativeOutboundHttp>,
}

impl NativeXweatherTileHttp {
    /// Retain the process outbound client without creating another runtime or pool.
    pub const fn new(client: Arc<NativeOutboundHttp>) -> Self {
        Self { client }
    }
}

impl XweatherTileHttpPort for NativeXweatherTileHttp {
    type Error = NativeOutboundHttpError;
    type Response = NativeXweatherTileResponse;

    /// Execute the exact GET headers timeout and redirect policy from the edge owner.
    fn execute(&self, request: &XweatherTileHttpRequest) -> Result<Self::Response, Self::Error> {
        // reject alternate port use before parsing a credential-bearing target
        if request.method != XweatherTileHttpMethod::Get
            || request.redirect != XweatherTileRedirectPolicy::Follow
            || request.timeout_ms != XWEATHER_TILE_TIMEOUT_MS
            || request.accept != "image/png"
            || request.user_agent != "Ballydidean-Weather/1.0"
        {
            return Err(NativeOutboundHttpError::InvalidRequest);
        }
        let target = Url::parse(request.target.as_str())
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        let accept = HeaderValue::from_str(request.accept)
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        let user_agent = HeaderValue::from_str(request.user_agent)
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        let outbound = NativeOutboundHttpRequest::new(Method::GET, target)?
            .header(ACCEPT, accept)
            .header(USER_AGENT, user_agent);
        let response = self.client.execute(
            outbound,
            Duration::from_millis(request.timeout_ms),
            u64::try_from(MAXIMUM_XWEATHER_TILE_BYTES)
                .ok()
                .and_then(|limit| limit.checked_add(1))
                .ok_or(NativeOutboundHttpError::InvalidRequest)?,
        )?;
        Ok(NativeXweatherTileResponse { response })
    }

    /// Reduce native failures to the predecessor's two safe diagnostic names.
    fn failure_kind(&self, error: &Self::Error) -> XweatherTileTransportFailureKind {
        // Fetch exposes target protocol and network failures as TypeError without their message
        if matches!(
            error,
            NativeOutboundHttpError::InvalidRedirect
                | NativeOutboundHttpError::InvalidRequest
                | NativeOutboundHttpError::TlsConfiguration(_)
                | NativeOutboundHttpError::TooManyRedirects
                | NativeOutboundHttpError::Transport(_)
        ) {
            XweatherTileTransportFailureKind::TypeError
        } else {
            XweatherTileTransportFailureKind::Error
        }
    }
}

/// One completely consumed native response exposed through the edge streaming contract.
pub struct NativeXweatherTileResponse {
    response: NativeOutboundHttpResponse,
}

impl XweatherTileHttpResponse for NativeXweatherTileResponse {
    type Error = NativeOutboundHttpError;

    /// Decode only the status content type and numeric Retry-After metadata.
    fn head(&self) -> XweatherTileHttpResponseHead {
        XweatherTileHttpResponseHead {
            content_type: header_text(&self.response, CONTENT_TYPE),
            retry_after_seconds: header_text(&self.response, RETRY_AFTER)
                .and_then(|value| value.trim().parse::<f64>().ok()),
            status_code: self.response.status,
        }
    }

    /// Replay bounded native body chunks until EOF or receiver refusal.
    fn read_body(
        &mut self,
        receiver: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<XweatherTileBodyReadOutcome, Self::Error> {
        // keep edge-owned cumulative accounting active instead of transferring the full vector
        for chunk in self.response.body.chunks(RESPONSE_CHUNK_BYTES) {
            // stop immediately when the edge rejects the first over-limit chunk
            if !receiver(chunk) {
                return Ok(XweatherTileBodyReadOutcome::ReceiverStopped);
            }
        }
        Ok(XweatherTileBodyReadOutcome::Complete)
    }
}

/// Safe stderr diagnostics matching the predecessor without target or credential material.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeXweatherTileDiagnostics;

impl XweatherTileDiagnostics for NativeXweatherTileDiagnostics {
    /// Emit only the reviewed error class for a transport failure.
    fn transport_failed(&self, kind: XweatherTileTransportFailureKind) {
        eprintln!("Xweather tile fetch failed: error={}", kind.wire_value());
    }

    /// Emit only status and the allowlisted content-type category.
    fn response_rejected(&self, status_code: u16, content_type: XweatherTileDiagnosticContentType) {
        eprintln!(
            "Xweather tile response rejected: status={status_code} content-type={}",
            content_type.wire_value()
        );
    }
}

/// Revalidate the configured root origin so target construction cannot bypass startup admission.
fn validate_origin(origin: &Url) -> Result<(), NativeOutboundHttpError> {
    let cleartext_loopback =
        origin.scheme() == "http" && matches!(origin.host_str(), Some("127.0.0.1" | "localhost"));
    // retain HTTPS everywhere and the predecessor's two disposable loopback exceptions
    if !(origin.scheme() == "https" || cleartext_loopback)
        || origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin
            .password()
            .is_some_and(|password| !password.is_empty())
        || origin.path() != "/"
        || origin.query().is_some_and(|query| !query.is_empty())
        || origin
            .fragment()
            .is_some_and(|fragment| !fragment.is_empty())
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(())
}

/// Require the exact geometry already admitted by the public map route owner.
fn validate_frame(frame: &weather_edge::XweatherTileFrame) -> Result<(), NativeOutboundHttpError> {
    // reject direct trait calls that did not pass through the reviewed static-frame route
    if frame.width != 256
        || frame.height != 168
        || frame.zoom != 10
        || !frame.latitude.is_finite()
        || !(-90.0..=90.0).contains(&frame.latitude)
        || !frame.longitude.is_finite()
        || !(-180.0..=180.0).contains(&frame.longitude)
        || frame.valid_time.len() != 14
        || !frame.valid_time.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(())
}

/// Match encodeURIComponent for one already-normalized UTF-8 credential value.
fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    // encode UTF-8 bytes using the exact ECMAScript unescaped component alphabet
    for byte in value.bytes() {
        // retain only encodeURIComponent's fixed ASCII pass-through population
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(&mut encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// Decode one response header without retaining invalid protocol bytes.
fn header_text(
    response: &NativeOutboundHttpResponse,
    name: hyper::header::HeaderName,
) -> Option<String> {
    response
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::thread;
    use std::time::Duration;
    use weather_edge::{
        BoundedXweatherTileTransport, EdgeXweatherCredentialByteReader,
        EdgeXweatherCredentialPaths, XweatherMapUnits, XweatherTileCacheKey,
        XweatherTileCacheRequest, XweatherTileFrame, XweatherTileLayer, XweatherTileLoad,
        XweatherTileLoader, XweatherTilePhase, load_edge_xweather_credentials,
    };

    struct FixtureCredentials;

    impl EdgeXweatherCredentialByteReader for FixtureCredentials {
        type Error = std::convert::Infallible;

        /// Return only the two bounded fixture credentials selected by their paths.
        fn read_optional(&mut self, path: &Path) -> Result<Option<Vec<u8>>, Self::Error> {
            // distinguish only the two requested fixture files
            Ok(match path.to_str() {
                Some("client-id") => Some(b"client/id".to_vec()),
                Some("client-secret") => Some(b"secret?value".to_vec()),
                _ => None,
            })
        }
    }

    /// Execute the complete target and HTTP adapter against one finite loopback tile.
    #[test]
    fn loads_exact_credentialed_tile_request_over_shared_client() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = b"\x89PNG\r\n\x1a\nfixture";
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            // consume the complete request headers before returning the bounded PNG fixture
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                // reject an incomplete request rather than block after EOF
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png; charset=binary\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
            String::from_utf8(request).unwrap()
        });
        let credentials = load_edge_xweather_credentials(
            EdgeXweatherCredentialPaths {
                client_id_file: Some("client-id"),
                client_secret_file: Some("client-secret"),
            },
            &mut FixtureCredentials,
        )
        .unwrap()
        .unwrap();
        let origin = Url::parse(&format!("http://{address}/")).unwrap();
        let targets = NativeXweatherTileTargets::new(origin, credentials).unwrap();
        let http = NativeXweatherTileHttp::new(Arc::new(NativeOutboundHttp::new().unwrap()));
        let transport =
            BoundedXweatherTileTransport::new(targets, http, NativeXweatherTileDiagnostics);
        let request = XweatherTileCacheRequest {
            key: XweatherTileCacheKey::admitted("forecast/radar/fixture"),
            phase: XweatherTilePhase::Forecast,
            provider_units: XweatherMapUnits::admitted(1).unwrap(),
            tile: XweatherTileLoad::Frame(XweatherTileFrame {
                height: 168,
                latitude: 47.950_43,
                layer: XweatherTileLayer::Radar,
                longitude: -122.427_97,
                valid_time: "20261124150000".to_owned(),
                width: 256,
                zoom: 10,
            }),
        };

        assert_eq!(transport.load_tile(&request).unwrap(), body);
        let observed = server.join().unwrap();
        assert!(observed.starts_with(concat!(
            "GET /client%2Fid_secret%3Fvalue/fradar/256x168/",
            "47.950430,-122.427970,10/20261124150000.png HTTP/1.1\r\n"
        )));
        let lower = observed.to_ascii_lowercase();
        assert!(lower.contains("accept: image/png\r\n"));
        assert!(lower.contains("user-agent: ballydidean-weather/1.0\r\n"));
    }

    /// Reject non-loopback cleartext origins before credentials can enter a target.
    #[test]
    fn rejects_cleartext_non_loopback_origin() {
        let credentials = load_edge_xweather_credentials(
            EdgeXweatherCredentialPaths {
                client_id_file: Some("client-id"),
                client_secret_file: Some("client-secret"),
            },
            &mut FixtureCredentials,
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            NativeXweatherTileTargets::new(
                Url::parse("http://maps.api.xweather.com/").unwrap(),
                credentials,
            ),
            Err(NativeOutboundHttpError::InvalidRequest)
        ));
    }
}
