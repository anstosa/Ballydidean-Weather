//! Native one-shot HTTP health probe for the installed API and web containers.

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::io;
use std::net::Ipv4Addr;
use std::process::ExitCode;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::{Method, Request, client::conn::http1, header};
use hyper_util::rt::TokioIo;
use tokio::{net::TcpStream, runtime::Builder};

const REQUEST_DEADLINE: Duration = Duration::from_secs(4);
const MAXIMUM_HTML_BYTES: usize = 2 * 1024 * 1024;
const MAXIMUM_JSON_BYTES: usize = 512 * 1024;

/// one closed installed service selected by its compose healthcheck flag
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HealthTarget {
    Api,
    Web,
}

impl HealthTarget {
    /// Retain only the fixed loopback route, port, and HTTP authority for this container.
    const fn endpoint(self) -> (u16, &'static str, &'static str) {
        match self {
            Self::Api => (3001, "/api/v1/health", "127.0.0.1:3001"),
            Self::Web => (3000, "/", "127.0.0.1:3000"),
        }
    }

    /// return the bounded service name used by local diagnostics
    const fn label(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Web => "web",
        }
    }
}

/// One ordinary health probe or exact release-aware web readiness command.
#[derive(Clone, Debug, Eq, PartialEq)]
enum ProbeCommand {
    Health(HealthTarget),
    WebRelease(String),
}

impl ProbeCommand {
    /// Parse only the two retained health flags or `web-release RELEASE`.
    fn from_args(mut arguments: impl Iterator<Item = OsString>) -> Option<Self> {
        let _program = arguments.next()?;
        let operation = arguments.next()?;
        let operation = operation.to_str()?;
        // preserve the ordinary single-flag healthcheck grammar
        if matches!(operation, "api" | "web") {
            let target = match operation {
                "api" => HealthTarget::Api,
                "web" => HealthTarget::Web,
                _ => return None,
            };
            return arguments.next().is_none().then_some(Self::Health(target));
        }
        if operation != "web-release" {
            return None;
        }
        let release = arguments.next()?.into_string().ok()?;
        // reject extra operands and alternate release spellings before any loopback request
        if arguments.next().is_some() || !valid_release(&release) {
            return None;
        }
        Some(Self::WebRelease(release))
    }
}

/// Issue one fixed plaintext GET and return only its status, like the original health check.
fn probe(target: HealthTarget) -> Result<u16, Box<dyn Error>> {
    // a one-shot loopback health check needs neither TLS initialization nor a pooled worker thread
    let runtime = Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        tokio::time::timeout(REQUEST_DEADLINE, async {
            let (port, path, authority) = target.endpoint();
            let socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
            let (mut sender, connection) = http1::handshake(TokioIo::new(socket)).await?;
            let request = Request::builder()
                .method(Method::GET)
                .uri(path)
                .header(header::HOST, authority)
                .header(header::CONNECTION, "close")
                .body(Empty::<Bytes>::new())?;
            let driver = tokio::spawn(connection);
            // the source checks response status without buffering any representation bytes
            let result = sender
                .send_request(request)
                .await
                .map(|response| response.status().as_u16());
            driver.abort();
            let _settled = driver.await;
            result.map_err(|error| -> Box<dyn Error> { Box::new(error) })
        })
        .await?
    })
}

/// Verify the exact web release asset and public API shapes through fixed loopback routes.
fn verify_web_release(release: &str) -> Result<(), Box<dyn Error>> {
    let runtime = Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        tokio::time::timeout(REQUEST_DEADLINE, verify_web_release_async(release))
            .await
            .map_err(|error| -> Box<dyn Error> { Box::new(error) })?
    })
}

/// Perform the predecessor's five fixed web-runtime reads under one shared deadline.
async fn verify_web_release_async(release: &str) -> Result<(), Box<dyn Error>> {
    let (status, html) = request_with_body(3000, "/", "127.0.0.1:3000", MAXIMUM_HTML_BYTES).await?;
    require_success(status)?;
    let html = std::str::from_utf8(&html)?;
    // require the selected immutable release asset root in the actual served HTML
    if !html.contains(&format!("/assets/{release}/")) {
        return Err(Box::new(io::Error::other("web release asset root differs")));
    }
    let (status, health) =
        request_with_body(3000, "/api/v1/health", "127.0.0.1:3000", MAXIMUM_JSON_BYTES).await?;
    require_success(status)?;
    let health: serde_json::Value = serde_json::from_slice(&health)?;
    // preserve the predecessor's exact ready-true health admission
    if health
        .pointer("/data/ready")
        .and_then(|value| value.as_bool())
        != Some(true)
    {
        return Err(Box::new(io::Error::other("web API health is not ready")));
    }
    let (status, sites) =
        request_with_body(3000, "/api/v1/sites", "127.0.0.1:3000", MAXIMUM_JSON_BYTES).await?;
    require_success(status)?;
    let sites: serde_json::Value = serde_json::from_slice(&sites)?;
    let site = sites
        .pointer("/data/0/slug")
        .and_then(|value| value.as_str())
        .ok_or_else(|| io::Error::other("web API site projection differs"))?;
    let site = encode_uri_component(site);
    let root = format!("/api/v1/sites/{site}");
    // retain both readable array projections before accepting the web release
    for suffix in ["current", "forecast"] {
        let path = format!("{root}/{suffix}");
        let (status, body) =
            request_with_body(3000, &path, "127.0.0.1:3000", MAXIMUM_JSON_BYTES).await?;
        require_success(status)?;
        let body: serde_json::Value = serde_json::from_slice(&body)?;
        if !body.get("data").is_some_and(|data| data.is_array()) {
            return Err(Box::new(io::Error::other(
                "web API weather projection differs",
            )));
        }
    }
    Ok(())
}

/// Issue one fixed loopback GET and buffer no more than the admitted response bound.
async fn request_with_body(
    port: u16,
    path: &str,
    authority: &str,
    maximum_bytes: usize,
) -> Result<(u16, Vec<u8>), Box<dyn Error>> {
    let socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
    let (mut sender, connection) = http1::handshake(TokioIo::new(socket)).await?;
    let request = Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(header::HOST, authority)
        .header(header::CONNECTION, "close")
        .body(Empty::<Bytes>::new())?;
    let driver = tokio::spawn(connection);
    let response = sender.send_request(request).await?;
    let status = response.status().as_u16();
    let mut body = response.into_body();
    let mut bytes = Vec::new();
    // copy only data frames and stop before one oversized representation is retained
    while let Some(frame) = body.frame().await {
        let frame = frame?;
        if let Some(data) = frame.data_ref() {
            if bytes.len().saturating_add(data.len()) > maximum_bytes {
                driver.abort();
                return Err(Box::new(io::Error::other(
                    "web probe response exceeds its bound",
                )));
            }
            bytes.extend_from_slice(data);
        }
    }
    driver.abort();
    let _settled = driver.await;
    Ok((status, bytes))
}

/// Require one ordinary successful HTTP response without following redirects.
fn require_success(status: u16) -> Result<(), Box<dyn Error>> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(Box::new(io::Error::other(format!(
            "web probe returned HTTP {status}",
        ))))
    }
}

/// Match the fixed dashboard release grammar before constructing its asset marker.
fn valid_release(value: &str) -> bool {
    let Some((date, sequence)) = value.rsplit_once('-') else {
        return false;
    };
    let mut dates = date.split('.');
    let components = [dates.next(), dates.next(), dates.next()];
    // require exactly YYYY.MM.DD plus a positive one-or-two digit release sequence
    components[0]
        .is_some_and(|year| year.len() == 4 && year.bytes().all(|byte| byte.is_ascii_digit()))
        && components[1].is_some_and(|month| {
            month.len() == 2 && month.bytes().all(|byte| byte.is_ascii_digit())
        })
        && components[2]
            .is_some_and(|day| day.len() == 2 && day.bytes().all(|byte| byte.is_ascii_digit()))
        && dates.next().is_none()
        && !sequence.starts_with('0')
        && (1..=2).contains(&sequence.len())
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

/// Encode one UTF-8 path component with the predecessor's `encodeURIComponent` pass-through set.
fn encode_uri_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    // preserve only JavaScript's exact unescaped component bytes
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

/// preserve compose's ready-zero and every-other-outcome-one contract
fn main() -> ExitCode {
    let Some(command) = ProbeCommand::from_args(env::args_os()) else {
        eprintln!("usage: weather-http-health <api|web|web-release RELEASE>");
        return ExitCode::FAILURE;
    };

    match command {
        ProbeCommand::WebRelease(release) => match verify_web_release(&release) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("weather-http-health: web release probe failed: {error}");
                ExitCode::FAILURE
            }
        },
        ProbeCommand::Health(target) => match probe(target) {
            Ok(status) if (200..300).contains(&status) => ExitCode::SUCCESS,
            Ok(status) => {
                eprintln!(
                    "weather-http-health: {} returned HTTP {status}",
                    target.label()
                );
                ExitCode::FAILURE
            }
            Err(error) => {
                eprintln!(
                    "weather-http-health: {} probe failed: {error}",
                    target.label()
                );
                ExitCode::FAILURE
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{HealthTarget, ProbeCommand, encode_uri_component, valid_release};

    /// retain the two exact service flags and their fixed loopback routes
    #[test]
    fn admits_only_fixed_health_targets() {
        assert_eq!(
            ProbeCommand::from_args(arguments(&["weather-http-health", "api"])),
            Some(ProbeCommand::Health(HealthTarget::Api)),
        );
        assert_eq!(
            ProbeCommand::from_args(arguments(&["weather-http-health", "web"])),
            Some(ProbeCommand::Health(HealthTarget::Web)),
        );
        assert_eq!(
            HealthTarget::Api.endpoint(),
            (3001, "/api/v1/health", "127.0.0.1:3001"),
        );
        assert_eq!(HealthTarget::Web.endpoint(), (3000, "/", "127.0.0.1:3000"));
        assert_eq!(
            ProbeCommand::from_args(arguments(&["weather-http-health"])),
            None,
        );
        assert_eq!(
            ProbeCommand::from_args(arguments(&["weather-http-health", "API"])),
            None,
        );
        assert_eq!(
            ProbeCommand::from_args(arguments(&[
                "weather-http-health",
                "api",
                "http://example.com/",
            ])),
            None,
        );
    }

    /// Admit only exact release-aware web probes without weakening ordinary flags.
    #[test]
    fn admits_exact_web_release_probe() {
        assert_eq!(
            ProbeCommand::from_args(arguments(&[
                "weather-http-health",
                "web-release",
                "2026.11.25-1",
            ])),
            Some(ProbeCommand::WebRelease("2026.11.25-1".to_owned())),
        );
        assert_eq!(
            ProbeCommand::from_args(arguments(&["weather-http-health", "api"])),
            Some(ProbeCommand::Health(HealthTarget::Api)),
        );
        assert!(!valid_release("2026.11.25-0"));
        assert!(!valid_release("2026-11-25-1"));
        assert_eq!(
            encode_uri_component("north field/β"),
            "north%20field%2F%CE%B2"
        );
    }

    /// construct owned process arguments for the fixed parser
    fn arguments(values: &[&str]) -> impl Iterator<Item = OsString> {
        values
            .iter()
            .map(|value| OsString::from(*value))
            .collect::<Vec<_>>()
            .into_iter()
    }
}
