//! Loopback functional coverage for concurrent scheduled Open-Meteo forecast transport.

use std::convert::Infallible;
use std::net::TcpListener as StdTcpListener;
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use weather_providers::{ProviderControls, ProviderRequestPolicy, run_provider_request};
use weather_worker::{OpenMeteoSourceKind, ProviderRequest, ScheduledWindow};

use super::{NativeOpenMeteoScheduledTransport, grid_cell_text};
use crate::{OpenMeteoWorkerLocation, WorkerOpenMeteoForecastTransport, provider_task_failure};

const WEATHER_BODY: &str = r#"{
  "latitude":54.123,
  "longitude":-6.5,
  "generationtime_ms":1.25,
  "utc_offset_seconds":0,
  "timezone":"UTC",
  "elevation":42,
  "hourly_units":{
    "time":"iso8601",
    "temperature_2m":"°C",
    "apparent_temperature":"°C",
    "relative_humidity_2m":"%",
    "precipitation":"mm",
    "cloud_cover":"%",
    "wind_speed_10m":"m/s",
    "wind_gusts_10m":"m/s",
    "wind_direction_10m":"°",
    "surface_pressure":"hPa",
    "uv_index":""
  },
  "hourly":{
    "time":["2026-11-24T20:00"],
    "temperature_2m":[10],
    "apparent_temperature":[9],
    "relative_humidity_2m":[70],
    "precipitation":[0.5],
    "cloud_cover":[80],
    "wind_speed_10m":[3],
    "wind_gusts_10m":[5],
    "wind_direction_10m":[360],
    "surface_pressure":[1012],
    "uv_index":[1]
  }
}"#;

const AIR_QUALITY_BODY: &str = r#"{
  "latitude":54.123,
  "longitude":-6.5,
  "generationtime_ms":2.5,
  "utc_offset_seconds":0,
  "timezone":"UTC",
  "hourly_units":{"time":"iso8601","pm2_5":"μg/m³"},
  "hourly":{"time":["2026-11-24T20:00"],"pm2_5":[4.5]}
}"#;

/// Serve both product responses only after both HTTP requests overlap.
fn serve_forecast_pair() -> (String, mpsc::Receiver<String>, JoinHandle<()>) {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind loopback");
    listener.set_nonblocking(true).expect("set nonblocking");
    let address = listener.local_addr().expect("read loopback address");
    let (request_sender, request_receiver) = mpsc::sync_channel(2);
    let server = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build loopback runtime");
        runtime.block_on(async move {
            let listener = TcpListener::from_std(listener).expect("admit loopback listener");
            let barrier = Arc::new(tokio::sync::Barrier::new(2));
            let mut connections = Vec::with_capacity(2);
            // admit both connections before either response may complete
            for _ in 0..2 {
                let (socket, _) = listener.accept().await.expect("accept loopback request");
                let barrier = Arc::clone(&barrier);
                let request_sender = request_sender.clone();
                connections.push(tokio::spawn(async move {
                    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                        let barrier = Arc::clone(&barrier);
                        let request_sender = request_sender.clone();
                        async move {
                            let target = request.uri().to_string();
                            request_sender
                                .send(target.clone())
                                .expect("capture request target");
                            barrier.wait().await;
                            let body = if target.starts_with("/v1/air-quality?") {
                                AIR_QUALITY_BODY.as_bytes()
                            } else {
                                WEATHER_BODY.as_bytes()
                            };
                            Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(body))))
                        }
                    });
                    http1::Builder::new()
                        .keep_alive(false)
                        .serve_connection(TokioIo::new(socket), service)
                        .await
                        .expect("serve forecast response");
                }));
            }
            // drain both finite loopback connections before stopping the server
            for connection in connections {
                connection.await.expect("join forecast connection");
            }
        });
    });
    (format!("http://{address}"), request_receiver, server)
}

/// preserve javascript coordinate spelling in scheduled metadata
#[test]
fn grid_cell_preserves_javascript_number_spelling() {
    let tie = f64::from_bits(0x3ff0_0008_0000_0000);

    assert_eq!(grid_cell_text(-0.0, 1e-7), "0,1e-7");
    assert_eq!(grid_cell_text(tie, -0.0), "1.0000076293945312,0");
}

/// Preserve exact paired query wire and independently overlap both bounded controllers.
#[test]
fn forecast_pair_is_concurrent_and_decodes_native_products() {
    let (origin, request_receiver, server) = serve_forecast_pair();
    let mut transport =
        NativeOpenMeteoScheduledTransport::new(Some(origin)).expect("start native transport");
    let request = ProviderRequest {
        endpoint: "https://api.open-meteo.com/v1/forecast",
        previous_cursor: None,
        source_id: "open-meteo".to_owned(),
        source_kind: OpenMeteoSourceKind::Forecast,
        timezone: "UTC".to_owned(),
        window: ScheduledWindow {
            start_ms: 0,
            end_exclusive_ms: 1,
        },
    };
    let policy = ProviderRequestPolicy {
        controls: ProviderControls {
            max_attempts: 1,
            max_body_bytes: 16_384,
            timeout_ms: 2_000,
        },
        deadline_at_ms: None,
    };
    let pair = transport
        .run_pair(
            OpenMeteoWorkerLocation {
                latitude: 54.123,
                longitude: -6.5,
            },
            &request,
            move |exchange| run_provider_request(policy, exchange).map_err(provider_task_failure),
            move |exchange| run_provider_request(policy, exchange).map_err(provider_task_failure),
        )
        .expect("complete concurrent forecast pair");

    assert_eq!(pair.weather.body.samples.len(), 1);
    assert_eq!(pair.weather.body.samples[0].valid_at_ms, 1_795_550_400_000);
    assert_eq!(
        pair.weather.body.samples[0].wind_direction_degrees,
        Some(0.0)
    );
    assert_eq!(pair.air_quality.body.samples.len(), 1);
    assert_eq!(
        pair.air_quality.body.samples[0].pm25_micrograms_per_cubic_meter,
        Some(4.5)
    );
    let mut targets = vec![
        request_receiver.recv().expect("receive first target"),
        request_receiver.recv().expect("receive second target"),
    ];
    targets.sort();
    assert_eq!(
        targets,
        vec![
            "/v1/air-quality?latitude=54.123&longitude=-6.5&hourly=pm2_5&forecast_days=7&timezone=UTC".to_owned(),
            "/v1/forecast?latitude=54.123&longitude=-6.5&hourly=temperature_2m%2Capparent_temperature%2Crelative_humidity_2m%2Cprecipitation%2Ccloud_cover%2Cwind_speed_10m%2Cwind_gusts_10m%2Cwind_direction_10m%2Csurface_pressure%2Cuv_index&temperature_unit=celsius&wind_speed_unit=ms&precipitation_unit=mm&timezone=UTC&forecast_days=10".to_owned(),
        ]
    );
    drop(transport);
    server.join().expect("join loopback server");
}
