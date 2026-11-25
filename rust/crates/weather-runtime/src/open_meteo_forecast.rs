//! Execute paired Open-Meteo forecast products through independent bounded controllers.

use weather_database::{SerializedJson, Sha256Digest};
use weather_providers::{
    ProviderIo, ProviderRequestPolicy, ProviderResponse, run_provider_request,
};
use weather_worker::{
    NormalizedOpenMeteoRecord, OpenMeteoAirQualitySample, OpenMeteoCursor, OpenMeteoEnvelope,
    OpenMeteoSourceKind, OpenMeteoWeatherSample, ProviderBatch, ProviderRequest,
    ScheduledProviderResult, TaskFailure, normalize_open_meteo_forecast,
};

use crate::open_meteo_forecast_evidence::{
    OpenMeteoForecastEvidenceValues, OpenMeteoForecastResponseEvidence,
    build_open_meteo_forecast_evidence,
};
use crate::{OpenMeteoWorkerLocation, provider_task_failure};

/// Parsed weather forecast plus its admitted upstream evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoForecastWeatherProduct {
    pub checksum: Sha256Digest,
    pub generation_ms: Option<f64>,
    pub envelope: OpenMeteoEnvelope,
    pub samples: Vec<OpenMeteoWeatherSample>,
}

/// Parsed air-quality forecast plus its admitted upstream evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoForecastAirQualityProduct {
    pub checksum: Sha256Digest,
    pub generation_ms: Option<f64>,
    pub samples: Vec<OpenMeteoAirQualitySample>,
}

/// Both independently settled forecast responses in their Node product order.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoForecastPair {
    pub weather: ProviderResponse<OpenMeteoForecastWeatherProduct>,
    pub air_quality: ProviderResponse<OpenMeteoForecastAirQualityProduct>,
}

/// Concurrent owner for the two request-bound forecast exchanges.
pub trait WorkerOpenMeteoForecastTransport {
    type WeatherExchange: ProviderIo<OpenMeteoForecastWeatherProduct> + Send;
    type AirQualityExchange: ProviderIo<OpenMeteoForecastAirQualityProduct> + Send;

    /// Start both closures independently like `Promise.all`, return the first failure unchanged,
    /// retain and drain its sibling inside the transport, and return only after both succeed.
    fn run_pair<Weather, AirQuality>(
        &mut self,
        location: OpenMeteoWorkerLocation,
        request: &ProviderRequest,
        weather: Weather,
        air_quality: AirQuality,
    ) -> Result<OpenMeteoForecastPair, TaskFailure>
    where
        Weather: FnOnce(
                &mut Self::WeatherExchange,
            )
                -> Result<ProviderResponse<OpenMeteoForecastWeatherProduct>, TaskFailure>
            + Send
            + 'static,
        AirQuality: FnOnce(
                &mut Self::AirQualityExchange,
            )
                -> Result<ProviderResponse<OpenMeteoForecastAirQualityProduct>, TaskFailure>
            + Send
            + 'static;

    /// Sample the provider receipt clock once, after both forecast products succeed.
    fn forecast_received_at_ms(&mut self) -> i64;
}

/// Fetch, normalize, and evidence the paired Open-Meteo forecast products.
pub fn fetch_worker_open_meteo_forecast(
    transport: &mut impl WorkerOpenMeteoForecastTransport,
    evidence: &mut impl OpenMeteoForecastEvidenceValues,
    location: OpenMeteoWorkerLocation,
    request: ProviderRequest,
    policy: ProviderRequestPolicy,
) -> ScheduledProviderResult<
    NormalizedOpenMeteoRecord,
    Option<SerializedJson>,
    Option<OpenMeteoCursor>,
> {
    // reject a cross-product request before opening either forecast exchange
    if request.source_kind != OpenMeteoSourceKind::Forecast {
        return Err(TaskFailure {
            attempts: 0,
            code: "invalid_payload".to_owned(),
        });
    }
    let weather_policy = policy;
    let air_quality_policy = policy;
    let pair = transport.run_pair(
        location,
        &request,
        move |exchange| {
            run_provider_request(weather_policy, exchange).map_err(provider_task_failure)
        },
        move |exchange| {
            run_provider_request(air_quality_policy, exchange).map_err(provider_task_failure)
        },
    )?;
    let received_at_ms = transport.forecast_received_at_ms();
    let ProviderResponse {
        attempts: weather_attempts,
        body: weather,
        status: weather_status,
    } = pair.weather;
    let ProviderResponse {
        attempts: air_quality_attempts,
        body: air_quality,
        status: air_quality_status,
    } = pair.air_quality;
    let attempts = u32::from(weather_attempts) + u32::from(air_quality_attempts);
    let normalized = normalize_open_meteo_forecast(
        weather.envelope,
        &weather.samples,
        &air_quality.samples,
        &request.source_id,
        received_at_ms,
        attempts,
    )?;
    let (checksum, metadata) = build_open_meteo_forecast_evidence(
        &OpenMeteoForecastResponseEvidence {
            checksum: weather.checksum,
            generation_ms: weather.generation_ms,
            http_status: weather_status,
        },
        &OpenMeteoForecastResponseEvidence {
            checksum: air_quality.checksum,
            generation_ms: air_quality.generation_ms,
            http_status: air_quality_status,
        },
        evidence,
    )?;

    Ok(ProviderBatch {
        attempts: normalized.attempts,
        provider_cursor: Some(normalized.provider_cursor),
        records: normalized.records,
        response_metadata: Some(metadata),
        upstream_response_checksum: checksum.as_str().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    use weather_contracts::JsonValue;
    use weather_providers::{
        ContentBudget, ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse,
        ProviderControls, ProviderSleep, ProviderSleepError,
    };
    use weather_worker::ScheduledWindow;

    use super::*;

    const WEATHER_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const AIR_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const COMBINED_SHA: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    struct Exchange<Product> {
        attempts: Arc<Mutex<Vec<ProviderAttempt>>>,
        first_attempts: Arc<(Mutex<u8>, Condvar)>,
        now_ms: i64,
        product: Product,
        statuses: VecDeque<u16>,
    }

    impl<Product: Clone> ProviderIo<Product> for Exchange<Product> {
        /// return one stable clock below the shared absolute deadline.
        fn now_ms(&mut self) -> i64 {
            self.now_ms
        }

        /// provide unused deterministic retry jitter.
        fn random_unit(&mut self) -> f64 {
            0.0
        }

        /// retain the bounded attempt and return the one admitted product.
        fn execute(
            &mut self,
            attempt: ProviderAttempt,
        ) -> Result<ProviderAttemptResponse<Product>, ProviderAttemptError> {
            self.attempts.lock().unwrap().push(attempt);
            // rendezvous both first attempts so the fixture proves overlapping execution
            if attempt.attempt == 1 {
                let (arrivals, ready) = &*self.first_attempts;
                let mut arrivals = arrivals.lock().unwrap();
                *arrivals += 1;
                ready.notify_all();
                let (arrivals, _) = ready
                    .wait_timeout_while(arrivals, Duration::from_secs(5), |count| *count < 2)
                    .unwrap();
                assert_eq!(*arrivals, 2, "both initial exchanges must overlap");
            }
            Ok(ProviderAttemptResponse {
                body: self.product.clone(),
                content: ContentBudget {
                    declared_body_bytes: Some(10),
                    streamed_body_bytes: 10,
                },
                provider_reason: None,
                retry_after_ms: None,
                status: self
                    .statuses
                    .pop_front()
                    .expect("fixture status queue exhausted"),
            })
        }

        /// admit the transient weather response's bounded retry delay.
        fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
            assert_eq!(sleep.deadline_remaining_ms, Some(500));
            assert_eq!(sleep.delay_ms, 250);
            self.now_ms += 250;
            Ok(())
        }
    }

    struct Transport {
        weather: Exchange<OpenMeteoForecastWeatherProduct>,
        air_quality: Exchange<OpenMeteoForecastAirQualityProduct>,
        clock_calls: usize,
    }

    impl WorkerOpenMeteoForecastTransport for Transport {
        type WeatherExchange = Exchange<OpenMeteoForecastWeatherProduct>;
        type AirQualityExchange = Exchange<OpenMeteoForecastAirQualityProduct>;

        /// settle both supplied controllers through the fixture's paired owner.
        fn run_pair<Weather, AirQuality>(
            &mut self,
            _location: OpenMeteoWorkerLocation,
            _request: &ProviderRequest,
            weather: Weather,
            air_quality: AirQuality,
        ) -> Result<OpenMeteoForecastPair, TaskFailure>
        where
            Weather: FnOnce(
                    &mut Self::WeatherExchange,
                )
                    -> Result<ProviderResponse<OpenMeteoForecastWeatherProduct>, TaskFailure>
                + Send
                + 'static,
            AirQuality: FnOnce(
                    &mut Self::AirQualityExchange,
                )
                    -> Result<ProviderResponse<OpenMeteoForecastAirQualityProduct>, TaskFailure>
                + Send
                + 'static,
        {
            let weather_exchange = &mut self.weather;
            let air_quality_exchange = &mut self.air_quality;
            let (weather, air_quality) = std::thread::scope(|scope| {
                let weather = scope.spawn(move || weather(weather_exchange));
                let air_quality = scope.spawn(move || air_quality(air_quality_exchange));
                (
                    weather.join().expect("weather fixture worker panicked"),
                    air_quality
                        .join()
                        .expect("air-quality fixture worker panicked"),
                )
            });
            Ok(OpenMeteoForecastPair {
                weather: weather?,
                air_quality: air_quality?,
            })
        }

        /// retain the single post-pair receipt-clock sample.
        fn forecast_received_at_ms(&mut self) -> i64 {
            self.clock_calls += 1;
            9_000
        }
    }

    #[derive(Default)]
    struct Evidence {
        checksum_inputs: Vec<Vec<u8>>,
        metadata_inputs: Vec<Vec<(Vec<u16>, JsonValue)>>,
    }

    impl OpenMeteoForecastEvidenceValues for Evidence {
        /// retain exact checksum framing and return its approved fixture digest.
        fn forecast_sha256(&mut self, bytes: &[u8]) -> Result<Sha256Digest, TaskFailure> {
            self.checksum_inputs.push(bytes.to_vec());
            Ok(Sha256Digest::admitted(COMBINED_SHA))
        }

        /// retain ordered metadata and return its approved fixture encoding.
        fn forecast_metadata_json(
            &mut self,
            entries: &[(Vec<u16>, JsonValue)],
        ) -> Result<SerializedJson, TaskFailure> {
            self.metadata_inputs.push(entries.to_vec());
            Ok(SerializedJson::admitted("{\"paired\":true}"))
        }
    }

    /// run both controllers under one policy and sum their successful attempt counts.
    #[test]
    fn fetches_paired_forecast_with_shared_policy_and_attempt_accounting() {
        let weather_attempts = Arc::new(Mutex::new(Vec::new()));
        let air_attempts = Arc::new(Mutex::new(Vec::new()));
        let first_attempts = Arc::new((Mutex::new(0), Condvar::new()));
        let weather_sample = OpenMeteoWeatherSample {
            apparent_temperature_c: Some(9.0),
            cloud_cover_percent: Some(20.0),
            precipitation_mm: Some(0.0),
            pressure_hpa: Some(1_010.0),
            relative_humidity_percent: Some(70.0),
            temperature_c: Some(10.0),
            uv_index: Some(1.0),
            valid_at_ms: 10_000,
            wind_direction_degrees: Some(180.0),
            wind_gust_mps: Some(4.0),
            wind_speed_mps: Some(2.0),
        };
        let mut transport = Transport {
            weather: Exchange {
                attempts: Arc::clone(&weather_attempts),
                first_attempts: Arc::clone(&first_attempts),
                now_ms: 1_000,
                product: OpenMeteoForecastWeatherProduct {
                    checksum: Sha256Digest::admitted(WEATHER_SHA),
                    generation_ms: Some(1.5),
                    envelope: OpenMeteoEnvelope {
                        elevation_m: Some(100.0),
                        latitude: 47.95,
                        longitude: -122.43,
                        upstream_timezone: "America/Los_Angeles".to_owned(),
                    },
                    samples: vec![weather_sample],
                },
                statuses: VecDeque::from([503, 200]),
            },
            air_quality: Exchange {
                attempts: Arc::clone(&air_attempts),
                first_attempts,
                now_ms: 1_000,
                product: OpenMeteoForecastAirQualityProduct {
                    checksum: Sha256Digest::admitted(AIR_SHA),
                    generation_ms: Some(2.5),
                    samples: vec![OpenMeteoAirQualitySample {
                        pm25_micrograms_per_cubic_meter: Some(4.0),
                        valid_at_ms: 10_000,
                    }],
                },
                statuses: VecDeque::from([206]),
            },
            clock_calls: 0,
        };
        let mut evidence = Evidence::default();
        let policy = ProviderRequestPolicy {
            controls: ProviderControls {
                max_attempts: 3,
                max_body_bytes: 123,
                timeout_ms: 777,
            },
            deadline_at_ms: Some(1_500),
        };
        let batch = fetch_worker_open_meteo_forecast(
            &mut transport,
            &mut evidence,
            OpenMeteoWorkerLocation {
                latitude: 47.95,
                longitude: -122.43,
            },
            ProviderRequest {
                endpoint: "forecast/hourly",
                previous_cursor: None,
                source_id: "forecast-source".to_owned(),
                source_kind: OpenMeteoSourceKind::Forecast,
                timezone: "America/Los_Angeles".to_owned(),
                window: ScheduledWindow {
                    end_exclusive_ms: 20_000,
                    start_ms: 10_000,
                },
            },
            policy,
        )
        .unwrap();

        let expected_attempt = ProviderAttempt {
            attempt: 1,
            max_body_bytes: 123,
            timeout_ms: 500,
        };
        assert_eq!(
            *weather_attempts.lock().unwrap(),
            [
                expected_attempt,
                ProviderAttempt {
                    attempt: 2,
                    max_body_bytes: 123,
                    timeout_ms: 250,
                },
            ]
        );
        assert_eq!(*air_attempts.lock().unwrap(), [expected_attempt]);
        assert_eq!(transport.clock_calls, 1);
        assert_eq!(batch.attempts, 3);
        assert_eq!(
            batch.provider_cursor,
            Some(OpenMeteoCursor::ProductRunAt(9_000))
        );
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.upstream_response_checksum, COMBINED_SHA);
        assert_eq!(
            batch.response_metadata,
            Some(SerializedJson::admitted("{\"paired\":true}"))
        );
        assert_eq!(
            evidence.checksum_inputs,
            [format!("{WEATHER_SHA}\n{AIR_SHA}\n").into_bytes()]
        );
    }
}
