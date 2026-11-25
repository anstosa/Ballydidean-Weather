use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use weather_edge::{
    AdmittedForecastObservationsJson, AdmittedForecastRecordedNow,
    AdmittedForecastRecordedProducts, AdmittedSolarCloudEvidenceJson, ForecastRecordedProductBody,
    ForecastRecordedProductMethod, ForecastRecordedProductRequest, ForecastRecordedProductResponse,
    ForecastRecordedProductRoute, ForecastRecordedProductsCache, ForecastRecordedProductsClock,
    ForecastRecordedProductsLoader, handle_forecast_recorded_product,
};

const OBSERVATIONS_JSON: &[u8] =
    b"{\"generatedAt\":\"2026-08-28T12:00:00.000Z\",\"data\":[{\"recorded\":\"observations\"}]}";
const SOLAR_JSON: &[u8] = b"{\"contractVersion\":\"solar-cloud-evidence/v1\",\"generatedAt\":\"2026-08-28T12:00:00.000Z\",\"evidence\":{\"recorded\":\"solar\"}}";

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureLoaderError(&'static str);

impl Display for FixtureLoaderError {
    /// render one injected archive fill failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureLoaderError {}

#[derive(Debug)]
struct DeferredFill {
    released: Mutex<Option<AdmittedForecastRecordedProducts>>,
    ready: Condvar,
    started: Mutex<bool>,
    started_signal: Condvar,
}

impl DeferredFill {
    /// create one blocked archive fill.
    fn pending() -> Self {
        Self {
            released: Mutex::new(None),
            ready: Condvar::new(),
            started: Mutex::new(false),
            started_signal: Condvar::new(),
        }
    }

    /// mark the fill active and wait for its admitted projection.
    fn fill(&self) -> AdmittedForecastRecordedProducts {
        {
            let mut started = self.started.lock().expect("deferred started lock");
            *started = true;
            self.started_signal.notify_all();
        }
        let mut released = self.released.lock().expect("deferred fill lock");

        // retain the archive fill until the fixture releases its projection
        while released.is_none() {
            released = self.ready.wait(released).expect("deferred fill lock");
        }

        released.take().expect("released deferred fill")
    }

    /// wait until the archive loader begins its blocked fill.
    fn wait_until_started(&self) {
        let mut started = self.started.lock().expect("deferred started lock");

        // wait only until the archive fill reports its start
        while !*started {
            started = self
                .started_signal
                .wait(started)
                .expect("deferred started lock");
        }
    }

    /// release one complete admitted projection.
    fn release(&self, products: AdmittedForecastRecordedProducts) {
        let mut released = self.released.lock().expect("deferred fill lock");
        *released = Some(products);
        self.ready.notify_all();
    }
}

#[derive(Debug)]
enum FixtureFillOutcome {
    Products(AdmittedForecastRecordedProducts),
    Error(FixtureLoaderError),
    Deferred(Arc<DeferredFill>),
}

#[derive(Debug, Default)]
struct FixtureLoaderState {
    calls: usize,
    outcomes: VecDeque<FixtureFillOutcome>,
}

#[derive(Debug, Clone, Default)]
struct FixtureLoader {
    state: Arc<Mutex<FixtureLoaderState>>,
}

impl FixtureLoader {
    /// create one deterministic archive fill sequence.
    fn from_outcomes(outcomes: Vec<FixtureFillOutcome>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FixtureLoaderState {
                calls: 0,
                outcomes: outcomes.into(),
            })),
        }
    }

    /// return the observed archive fill count.
    fn calls(&self) -> usize {
        self.state.lock().expect("loader lock").calls
    }
}

impl ForecastRecordedProductsLoader for FixtureLoader {
    type Error = FixtureLoaderError;

    /// return the next admitted fill or injected archive failure.
    fn fill(
        &self,
        _request_started_at_epoch_ms: i64,
    ) -> Result<AdmittedForecastRecordedProducts, Self::Error> {
        let outcome = {
            let mut state = self.state.lock().expect("loader lock");
            state.calls += 1;
            state.outcomes.pop_front().expect("fixture fill outcome")
        };

        // preserve the selected fill outcome category
        match outcome {
            FixtureFillOutcome::Products(products) => Ok(products),
            FixtureFillOutcome::Error(error) => Err(error),
            FixtureFillOutcome::Deferred(deferred) => Ok(deferred.fill()),
        }
    }
}

#[derive(Debug, Clone)]
struct FixtureClock {
    now: Arc<Mutex<AdmittedForecastRecordedNow>>,
}

impl FixtureClock {
    /// create one fixed admitted cache observation.
    fn fixed(epoch_ms: i64, local_day: &str) -> Self {
        Self {
            now: Arc::new(Mutex::new(AdmittedForecastRecordedNow {
                epoch_ms,
                local_day: local_day.to_owned(),
            })),
        }
    }

    /// move to one new admitted cache observation.
    fn set(&self, epoch_ms: i64, local_day: &str) {
        *self.now.lock().expect("clock lock") = AdmittedForecastRecordedNow {
            epoch_ms,
            local_day: local_day.to_owned(),
        };
    }
}

impl ForecastRecordedProductsClock for FixtureClock {
    /// return the current admitted cache observation.
    fn now(&self) -> AdmittedForecastRecordedNow {
        self.now.lock().expect("clock lock").clone()
    }
}

/// build one complete source-shaped admitted projection.
fn products(generated_at: &str, day: &str) -> AdmittedForecastRecordedProducts {
    let forecast = if generated_at == "2026-08-28T12:00:00.000Z" {
        OBSERVATIONS_JSON.to_vec()
    } else {
        format!("{{\"generatedAt\":\"{generated_at}\",\"data\":[]}}").into_bytes()
    };
    let solar = if generated_at == "2026-08-28T12:00:00.000Z" {
        SOLAR_JSON.to_vec()
    } else {
        format!("{{\"contractVersion\":\"solar-cloud-evidence/v1\",\"generatedAt\":\"{generated_at}\",\"evidence\":null}}").into_bytes()
    };
    AdmittedForecastRecordedProducts {
        day: day.to_owned(),
        forecast_observations: AdmittedForecastObservationsJson::admitted(forecast),
        generated_at: generated_at.to_owned(),
        solar_cloud_evidence: AdmittedSolarCloudEvidenceJson::admitted(solar),
    }
}

/// map source-derived fixture generation times to their point counts.
fn point_count(products: &AdmittedForecastRecordedProducts) -> usize {
    // retain only the focused source fixture counts
    match products.generated_at.as_str() {
        "2026-08-28T12:00:00.000Z" => 5,
        "2026-08-28T12:05:00.000Z" | "2026-08-28T12:00:30.000Z" => 6,
        other => panic!("unexpected point-count fixture: {other}"),
    }
}

/// render one route response like the compiled Node adapter.
fn response_signature(response: &ForecastRecordedProductResponse) -> String {
    let body = match &response.body {
        ForecastRecordedProductBody::Text(body) => {
            String::from_utf8_lossy(body).trim_end().to_owned()
        }
        ForecastRecordedProductBody::Json(None) => "-".to_owned(),
        ForecastRecordedProductBody::Json(Some(body)) => String::from_utf8_lossy(body).to_string(),
    };
    format!(
        "{}|{}|{}|{}|{}|{}|{}",
        response.status_code,
        response.headers.cache_control,
        response.headers.content_type,
        response.headers.content_length,
        response.headers.allow.unwrap_or("-"),
        response.headers.retry_after.unwrap_or("-"),
        body
    )
}

/// build one queryless recorded route request.
fn route_request(
    method: ForecastRecordedProductMethod,
    route: ForecastRecordedProductRoute,
    has_query: bool,
) -> ForecastRecordedProductRequest {
    ForecastRecordedProductRequest {
        has_query,
        method,
        route,
    }
}

/// Share both immutable responses across simultaneous readers and a cache refresh.
#[test]
fn recorded_cache_and_get_share_the_retained_byte_allocations() {
    let initial = products("2026-08-28T12:00:00.000Z", "2026-08-28");
    let refreshed = products("2026-08-28T12:05:00.000Z", "2026-08-28");
    let loader = FixtureLoader::from_outcomes(vec![
        FixtureFillOutcome::Products(initial.clone()),
        FixtureFillOutcome::Products(refreshed.clone()),
    ]);
    let clock = FixtureClock::fixed(1_000, "2026-08-28");
    let cache = ForecastRecordedProductsCache::new(clock.clone(), loader.clone(), 30_000);
    let warmed = cache.get().expect("warm cache");
    assert!(std::ptr::eq(
        warmed.forecast_observations.as_bytes(),
        initial.forecast_observations.as_bytes()
    ));
    assert!(std::ptr::eq(
        warmed.solar_cloud_evidence.as_bytes(),
        initial.solar_cloud_evidence.as_bytes()
    ));
    let observations = handle_forecast_recorded_product(
        route_request(
            ForecastRecordedProductMethod::Get,
            ForecastRecordedProductRoute::ForecastObservations,
            false,
        ),
        &cache,
    );
    let solar = handle_forecast_recorded_product(
        route_request(
            ForecastRecordedProductMethod::Get,
            ForecastRecordedProductRoute::SolarCloudEvidence,
            false,
        ),
        &cache,
    );

    // require each response to retain the selected archive fill without copying it
    for (response, admitted) in [
        (&observations, initial.forecast_observations.as_bytes()),
        (&solar, initial.solar_cloud_evidence.as_bytes()),
    ] {
        let ForecastRecordedProductBody::Json(Some(bytes)) = &response.body else {
            panic!("expected admitted history JSON");
        };
        assert_eq!(response.status_code, 200);
        assert_eq!(response.headers.content_length, admitted.len());
        assert!(std::ptr::eq(&bytes[..], admitted));
    }
    assert_eq!(loader.calls(), 1);
    drop(initial);
    drop(warmed);

    // keep both in-flight responses valid after the cache replaces its own fill
    clock.set(301_000, "2026-08-28");
    let next = cache.get().expect("refreshed cache");
    assert!(std::ptr::eq(
        next.solar_cloud_evidence.as_bytes(),
        refreshed.solar_cloud_evidence.as_bytes()
    ));
    for (response, expected) in [(observations, OBSERVATIONS_JSON), (solar, SOLAR_JSON)] {
        let ForecastRecordedProductBody::Json(Some(bytes)) = response.body else {
            panic!("expected in-flight history JSON");
        };
        assert_eq!(&bytes[..], expected);
    }
    assert_eq!(loader.calls(), 2);
}

// verifies actual route responses and shared archive cache coordination parity
#[test]
fn matches_compiled_node_recorded_product_routes_and_cache() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/forecast-recorded-products-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run forecast recorded-products Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");

    let route_clock = FixtureClock::fixed(1_787_918_400_000, "2026-08-28");
    let route_loader = FixtureLoader::from_outcomes(vec![FixtureFillOutcome::Products(products(
        "2026-08-28T12:00:00.000Z",
        "2026-08-28",
    ))]);
    let route_cache = ForecastRecordedProductsCache::new(route_clock, route_loader, 30_000);
    let unsupported = handle_forecast_recorded_product(
        route_request(
            ForecastRecordedProductMethod::Unsupported,
            ForecastRecordedProductRoute::ForecastObservations,
            false,
        ),
        &route_cache,
    );
    let query = handle_forecast_recorded_product(
        route_request(
            ForecastRecordedProductMethod::Get,
            ForecastRecordedProductRoute::ForecastObservations,
            true,
        ),
        &route_cache,
    );
    let observations = handle_forecast_recorded_product(
        route_request(
            ForecastRecordedProductMethod::Get,
            ForecastRecordedProductRoute::ForecastObservations,
            false,
        ),
        &route_cache,
    );
    let observations_head = handle_forecast_recorded_product(
        route_request(
            ForecastRecordedProductMethod::Head,
            ForecastRecordedProductRoute::ForecastObservations,
            false,
        ),
        &route_cache,
    );
    let solar = handle_forecast_recorded_product(
        route_request(
            ForecastRecordedProductMethod::Get,
            ForecastRecordedProductRoute::SolarCloudEvidence,
            false,
        ),
        &route_cache,
    );
    let failing_cache = ForecastRecordedProductsCache::new(
        FixtureClock::fixed(1_787_918_400_000, "2026-08-28"),
        FixtureLoader::from_outcomes(vec![FixtureFillOutcome::Error(FixtureLoaderError(
            "archive gap",
        ))]),
        30_000,
    );
    let archive_gap = handle_forecast_recorded_product(
        route_request(
            ForecastRecordedProductMethod::Get,
            ForecastRecordedProductRoute::ForecastObservations,
            false,
        ),
        &failing_cache,
    );

    let cache_clock = FixtureClock::fixed(1_787_918_400_000, "2026-08-28");
    let cache_loader = FixtureLoader::from_outcomes(vec![
        FixtureFillOutcome::Products(products("2026-08-28T12:00:00.000Z", "2026-08-28")),
        FixtureFillOutcome::Products(products("2026-08-28T12:05:00.000Z", "2026-08-28")),
    ]);
    let cache =
        ForecastRecordedProductsCache::new(cache_clock.clone(), cache_loader.clone(), 30_000);
    let cached_first = cache.get().expect("first cache fill");
    let cached_second = cache.get().expect("cached second read");
    cache_clock.set(1_787_918_400_000 + 5 * 60_000 - 1, "2026-08-28");
    let cached_before_expiry = cache.get().expect("cached before expiry");
    cache_clock.set(1_787_918_400_000 + 5 * 60_000, "2026-08-28");
    let cached_refreshed = cache.get().expect("cache refresh");

    let pending_clock = FixtureClock::fixed(1_787_918_400_000, "2026-08-28");
    let deferred = Arc::new(DeferredFill::pending());
    let pending_loader =
        FixtureLoader::from_outcomes(vec![FixtureFillOutcome::Deferred(Arc::clone(&deferred))]);
    let pending_cache = Arc::new(ForecastRecordedProductsCache::new(
        pending_clock,
        pending_loader.clone(),
        30_000,
    ));
    let first_cache = Arc::clone(&pending_cache);
    let pending_first_request = thread::spawn(move || first_cache.get());
    deferred.wait_until_started();
    let second_cache = Arc::clone(&pending_cache);
    let pending_second_request = thread::spawn(move || second_cache.get());
    thread::sleep(Duration::from_millis(20));
    deferred.release(products("2026-08-28T12:00:00.000Z", "2026-08-28"));
    let pending_first = pending_first_request
        .join()
        .expect("first pending thread")
        .expect("first pending result");
    let pending_second = pending_second_request
        .join()
        .expect("second pending thread")
        .expect("second pending result");

    let retry_clock = FixtureClock::fixed(1_787_918_400_000, "2026-08-28");
    let retry_loader = FixtureLoader::from_outcomes(vec![
        FixtureFillOutcome::Error(FixtureLoaderError("archive gap")),
        FixtureFillOutcome::Products(products("2026-08-28T12:00:30.000Z", "2026-08-28")),
    ]);
    let retry_cache =
        ForecastRecordedProductsCache::new(retry_clock.clone(), retry_loader.clone(), 30_000);
    let initial_failure = retry_cache
        .get()
        .expect_err("inject initial archive gap")
        .to_string();
    let cooldown_failure = retry_cache
        .get()
        .expect_err("enforce retry cooldown")
        .to_string();
    retry_clock.set(1_787_918_430_000, "2026-08-28");
    let retry_recovered = retry_cache.get().expect("recover at cooldown boundary");

    let rollover_clock = FixtureClock::fixed(1_788_003_599_000, "2026-08-28");
    let rollover_loader = FixtureLoader::from_outcomes(vec![
        FixtureFillOutcome::Products(products("2026-08-29T06:59:59.000Z", "2026-08-28")),
        FixtureFillOutcome::Products(products("2026-08-29T07:00:00.000Z", "2026-08-29")),
    ]);
    let rollover_cache =
        ForecastRecordedProductsCache::new(rollover_clock.clone(), rollover_loader.clone(), 30_000);
    let rollover_before = rollover_cache.get().expect("pre-midnight fill");
    rollover_clock.set(1_788_003_600_000, "2026-08-29");
    let rollover_after = rollover_cache.get().expect("post-midnight fill");

    let rust = [
        format!("unsupported={}", response_signature(&unsupported)),
        format!("query={}", response_signature(&query)),
        format!("observations={}", response_signature(&observations)),
        format!(
            "observations-head={}",
            response_signature(&observations_head)
        ),
        format!("solar={}", response_signature(&solar)),
        format!("archive-gap={}", response_signature(&archive_gap)),
        format!(
            "cache={}/{}/{}/{}:fetches={}",
            point_count(&cached_first),
            cached_first == cached_second,
            cached_first == cached_before_expiry,
            point_count(&cached_refreshed),
            cache_loader.calls()
        ),
        format!(
            "pending={}/{}:fetches={}",
            point_count(&pending_first),
            pending_first == pending_second,
            pending_loader.calls()
        ),
        format!(
            "retry={initial_failure}:{cooldown_failure}:{}:fetches={}",
            point_count(&retry_recovered),
            retry_loader.calls()
        ),
        format!(
            "rollover={}/{}:fetches={}",
            rollover_before.generated_at,
            rollover_after.generated_at,
            rollover_loader.calls()
        ),
    ]
    .join("\n");
    assert_eq!(rust, node.trim_end());
}
