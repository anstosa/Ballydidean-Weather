//! Native widget forecast upstream acquisition and canonical receive clock.

use std::{sync::Arc, time::Duration};

use hyper::{
    Method,
    header::{ACCEPT, HeaderValue},
};
use url::Url;
use weather_contracts::JsonValue;
use weather_edge::{
    AdmittedPublicApiOrigin, AdmittedWidgetForecastReceivedAt, ArchiveWirePrimitives,
    PublicAdjustmentProjectionError, PublicAdjustmentSnapshot, PublicApiProjection,
    WidgetForecastClient, WidgetForecastClientRequest, WidgetForecastReceivedAtSource,
    WidgetForecastUpstream, apply_public_adjustment_projection,
};

use crate::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest, NativeWirePrimitives,
    system_worker_health_milliseconds,
};

/// Keep source failures distinct while the widget application owns the 502 response.
#[derive(Debug)]
pub enum NativeWidgetSourceError {
    Transport(NativeOutboundHttpError),
    Status(u16),
    Projection(PublicAdjustmentProjectionError),
    Clock,
}

/// Process-retained widget client over the same HTTP pool and settings snapshot as public forecast.
pub struct NativeWidgetForecastSource<Settings> {
    origin: Url,
    client: Arc<NativeOutboundHttp>,
    settings: Settings,
}

impl<Settings> NativeWidgetForecastSource<Settings> {
    /// Bind the pre-admitted core API origin without opening a second HTTP pool.
    pub fn new(
        origin: AdmittedPublicApiOrigin,
        client: Arc<NativeOutboundHttp>,
        settings: Settings,
    ) -> Result<Self, NativeOutboundHttpError> {
        Ok(Self {
            origin: Url::parse(origin.as_str())
                .map_err(|_| NativeOutboundHttpError::InvalidRequest)?,
            client,
            settings,
        })
    }
}

impl<Settings: PublicAdjustmentSnapshot> WidgetForecastClient
    for NativeWidgetForecastSource<Settings>
{
    type Error = NativeWidgetSourceError;
    type Forecast = JsonValue;

    /// Snapshot settings before fetching and return the filtered forecast without a JSON re-encode.
    fn fetch_forecast(
        &self,
        request: WidgetForecastClientRequest,
    ) -> Result<JsonValue, Self::Error> {
        let settings = self.settings.read_settings();
        // the widget route chooses only these two fixed source targets
        let path = match request.upstream {
            WidgetForecastUpstream::Daily => "/api/v1/sites/ballydidean/forecast?days=1",
            WidgetForecastUpstream::Overnight => {
                "/api/v1/sites/ballydidean/forecast?window=overnight"
            }
        };
        let url = self.origin.join(path).map_err(|_| {
            NativeWidgetSourceError::Transport(NativeOutboundHttpError::InvalidRequest)
        })?;
        let outbound = NativeOutboundHttpRequest::new(Method::GET, url)
            .map_err(NativeWidgetSourceError::Transport)?
            .header(ACCEPT, HeaderValue::from_static("application/json"))
            .manual_redirect()
            .stream_body_limit_only();
        let response = self
            .client
            .execute(
                outbound,
                Duration::from_millis(request.timeout_ms),
                request.maximum_response_bytes as u64,
            )
            .map_err(NativeWidgetSourceError::Transport)?;
        // expose only a successful upstream forecast
        if !(200..300).contains(&response.status) {
            return Err(NativeWidgetSourceError::Status(response.status));
        }
        let mut forecast = NativeWirePrimitives
            .decode_json(String::from_utf8_lossy(&response.body).as_bytes())
            .ok_or(NativeWidgetSourceError::Projection(
                PublicAdjustmentProjectionError::Decode,
            ))?;
        apply_public_adjustment_projection(PublicApiProjection::Forecast, settings, &mut forecast)
            .map_err(NativeWidgetSourceError::Projection)?;
        Ok(forecast)
    }
}

/// Actual native receive-time source for the widget application's post-fetch clock boundary.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWidgetReceivedAt;

impl WidgetForecastReceivedAtSource for NativeWidgetReceivedAt {
    type Error = NativeWidgetSourceError;

    /// Resolve the post-fetch instant with the existing canonical UTC formatter.
    fn approved_received_at(&self) -> Result<AdmittedWidgetForecastReceivedAt, Self::Error> {
        system_worker_health_milliseconds()
            .and_then(|instant| NativeWirePrimitives.format_utc_milliseconds(instant))
            .map(AdmittedWidgetForecastReceivedAt::admitted)
            .ok_or(NativeWidgetSourceError::Clock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    use weather_edge::{
        DEFAULT_FORECAST_ADJUSTMENT_SETTINGS, EdgeStartupOrigins, ForecastAdjustmentSettings,
        RevisionArchiveCanonicalCodec, WidgetForecastVersion,
    };

    struct Settings;

    impl PublicAdjustmentSnapshot for Settings {
        /// Return the fixture's admitted adjustment snapshot before any HTTP request.
        fn read_settings(&self) -> ForecastAdjustmentSettings {
            DEFAULT_FORECAST_ADJUSTMENT_SETTINGS
        }
    }

    /// Exercise the retained widget-specific upstream query and actual forecast filter.
    #[test]
    fn native_edge_widget_source_uses_overnight_target_and_shared_projection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = crate::NativeEdgeOrigins
            .api_origin(&format!("http://{}", listener.local_addr().unwrap()))
            .unwrap();
        let server = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            // await the one fixture request with a bounded wall-clock deadline
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            // retain the exact request target independently of packet fragmentation
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 8192);
            }
            assert!(request.starts_with(
                b"GET /api/v1/sites/ballydidean/forecast?window=overnight HTTP/1.1\r\n"
            ));
            let body = b"{\"data\":[]}";
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            socket.write_all(body).unwrap();
        });
        let source = NativeWidgetForecastSource::new(
            origin,
            Arc::new(NativeOutboundHttp::new().unwrap()),
            Settings,
        )
        .unwrap();
        let forecast = source
            .fetch_forecast(WidgetForecastClientRequest {
                maximum_response_bytes: 1024 * 1024,
                timeout_ms: 5000,
                upstream: WidgetForecastUpstream::Overnight,
                version: WidgetForecastVersion::V3,
            })
            .unwrap();
        let wire = NativeWirePrimitives.encode_ordered_json(&forecast).unwrap();
        assert_eq!(wire, b"{\"data\":[],\"adjustmentSettings\":{\"version\":1,\"temperature\":true,\"wind\":true,\"rain\":true}}\n");
        server.join().unwrap();
        let clock = NativeWidgetReceivedAt.approved_received_at().unwrap();
        assert!(
            NativeWirePrimitives
                .canonical_utc_milliseconds(clock.as_str())
                .is_some()
        );
    }
}
