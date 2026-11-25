use std::cell::RefCell;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_edge::{
    AdmittedForecastObservationsJson, AdmittedForecastRecordedInstant,
    AdmittedSolarCloudEvidenceJson, DecodedForecastRecordedInput, ForecastRecordedBodyReadOutcome,
    ForecastRecordedInputHttpPort, ForecastRecordedInputMethod, ForecastRecordedInputMetrics,
    ForecastRecordedInputProvenance, ForecastRecordedInputRecord, ForecastRecordedInputRequest,
    ForecastRecordedInputResponse, ForecastRecordedInputResponseHead, ForecastRecordedInputSite,
    ForecastRecordedInputSourceKind, ForecastRecordedLoaderBoundary, ForecastRecordedLoaderClock,
    ForecastRecordedLoaderError, ForecastRecordedProductsLoader, ForecastRecordedRedirectPolicy,
    ForecastRecordedSolarSite, HttpForecastRecordedProductsLoader,
};

const GENERATED_MS: i64 = 1_787_918_400_000;
const PROJECTION_MS: i64 = 1_787_900_400_000;
const QUERY_MS: i64 = 1_787_830_800_000;

#[derive(Clone, Debug, Eq, PartialEq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Render one injected adapter failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

#[derive(Clone)]
struct FixtureClock {
    monotonic: Rc<RefCell<VecDeque<u64>>>,
    wall_epoch_ms: i64,
}

impl FixtureClock {
    /// Build one deterministic wall and monotonic clock sequence.
    fn new(monotonic: impl IntoIterator<Item = u64>) -> Self {
        Self {
            monotonic: Rc::new(RefCell::new(monotonic.into_iter().collect())),
            wall_epoch_ms: GENERATED_MS,
        }
    }
}

impl ForecastRecordedLoaderClock for FixtureClock {
    /// Consume the next monotonic fixture observation.
    fn monotonic_ms(&self) -> u64 {
        self.monotonic
            .borrow_mut()
            .pop_front()
            .expect("fixture monotonic observation")
    }

    /// Return the admitted response receipt wall clock.
    fn wall_epoch_ms(&self) -> i64 {
        self.wall_epoch_ms
    }
}

#[derive(Clone)]
struct FixtureHttp {
    chunks: Vec<Vec<u8>>,
    head: ForecastRecordedInputResponseHead,
    request: Rc<RefCell<Option<ForecastRecordedInputRequest>>>,
}

impl FixtureHttp {
    /// Build one ordinary successful streaming response.
    fn success(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            chunks,
            head: ForecastRecordedInputResponseHead {
                body_present: true,
                status_code: 200,
            },
            request: Rc::new(RefCell::new(None)),
        }
    }
}

struct FixtureResponse {
    chunks: Vec<Vec<u8>>,
    head: ForecastRecordedInputResponseHead,
}

impl ForecastRecordedInputResponse for FixtureResponse {
    type Error = FixtureError;

    /// Return the fixed response head before streaming.
    fn head(&self) -> ForecastRecordedInputResponseHead {
        self.head
    }

    /// Offer each chunk until the receiver refuses one.
    fn read_body(
        &mut self,
        receiver: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<ForecastRecordedBodyReadOutcome, Self::Error> {
        // preserve physical response chunk order
        for chunk in &self.chunks {
            // stop without offering later chunks after bounded refusal
            if !receiver(chunk) {
                return Ok(ForecastRecordedBodyReadOutcome::ReceiverStopped);
            }
        }
        Ok(ForecastRecordedBodyReadOutcome::Complete)
    }
}

impl ForecastRecordedInputHttpPort for FixtureHttp {
    type Error = FixtureError;
    type Response = FixtureResponse;

    /// Capture the exact fixed request and return the streaming fixture.
    fn execute(
        &self,
        request: &ForecastRecordedInputRequest,
    ) -> Result<Self::Response, Self::Error> {
        *self.request.borrow_mut() = Some(request.clone());
        Ok(FixtureResponse {
            chunks: self.chunks.clone(),
            head: self.head,
        })
    }
}

#[derive(Clone)]
struct FixtureBoundary {
    input: DecodedForecastRecordedInput,
    solar_fails: bool,
    trace: Rc<RefCell<Vec<&'static str>>>,
}

impl FixtureBoundary {
    /// Build one source-valid decoded private envelope.
    fn normal() -> Self {
        Self {
            input: decoded_input(Vec::new()),
            solar_fails: true,
            trace: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl ForecastRecordedLoaderBoundary for FixtureBoundary {
    type Error = FixtureError;
    type SolarCloudEvidence = String;

    /// Decode only the exact assembled transport fixture.
    fn decode_input(&self, bytes: &[u8]) -> Result<DecodedForecastRecordedInput, Self::Error> {
        assert_eq!(bytes, br#"{"fixture":true}"#);
        self.trace.borrow_mut().push("decode");
        Ok(self.input.clone())
    }

    /// Resolve the fixture farm day for generated and projection instants.
    fn local_day(
        &self,
        _instant: &AdmittedForecastRecordedInstant,
        timezone: &str,
    ) -> Result<String, Self::Error> {
        assert_eq!(timezone, "America/Los_Angeles");
        Ok("2026-08-28".to_owned())
    }

    /// Resolve the exact fixture farm-local midnight.
    fn site_midnight(
        &self,
        local_day: &str,
        timezone: &str,
    ) -> Result<AdmittedForecastRecordedInstant, Self::Error> {
        assert_eq!(local_day, "2026-08-28");
        assert_eq!(timezone, "America/Los_Angeles");
        Ok(instant("2026-08-28T07:00:00.000Z", PROJECTION_MS))
    }

    /// Serialize the exact calculated query lower bound.
    fn utc_instant(&self, epoch_ms: i64) -> Result<AdmittedForecastRecordedInstant, Self::Error> {
        assert_eq!(epoch_ms, QUERY_MS);
        Ok(instant("2026-08-27T11:40:00.000Z", QUERY_MS))
    }

    /// Produce the complete recorded-observation product.
    fn forecast_observations(
        &self,
        records: &[ForecastRecordedInputRecord],
        projection_from: &AdmittedForecastRecordedInstant,
        through: &AdmittedForecastRecordedInstant,
    ) -> Result<AdmittedForecastObservationsJson, Self::Error> {
        assert!(records.is_empty());
        assert_eq!(projection_from.epoch_ms, PROJECTION_MS);
        assert_eq!(through.epoch_ms, GENERATED_MS);
        self.trace.borrow_mut().push("forecast");
        Ok(AdmittedForecastObservationsJson::admitted(
            br#"{"generatedAt":"2026-08-28T12:00:00.000Z","data":[]}"#.to_vec(),
        ))
    }

    /// Inject the source-compatible independent optional solar refusal.
    fn build_solar_cloud_evidence(
        &self,
        _records: &[ForecastRecordedInputRecord],
        site: ForecastRecordedSolarSite,
        _through: &AdmittedForecastRecordedInstant,
    ) -> Result<Self::SolarCloudEvidence, Self::Error> {
        assert_eq!(site.latitude, 47.950_429_954_185_445);
        assert_eq!(site.longitude, -122.427_970_126_081_93);
        self.trace.borrow_mut().push("solar");
        // exercise the isolated fallback only when configured
        if self.solar_fails {
            return Err(FixtureError("optional solar refusal"));
        }
        Ok("solar-evidence".to_owned())
    }

    /// Encode projected evidence or the valid null product after optional refusal.
    fn encode_solar_cloud_evidence(
        &self,
        through: &AdmittedForecastRecordedInstant,
        evidence: Option<&String>,
    ) -> Result<AdmittedSolarCloudEvidenceJson, Self::Error> {
        assert_eq!(through.epoch_ms, GENERATED_MS);
        // encode the exact optional evidence selected by the independent builder
        match evidence {
            Some(evidence) => {
                self.trace.borrow_mut().push("solar-encode");
                assert_eq!(evidence, "solar-evidence");
                Ok(AdmittedSolarCloudEvidenceJson::admitted(b"solar".to_vec()))
            }
            None => {
                self.trace.borrow_mut().push("solar-null");
                Ok(AdmittedSolarCloudEvidenceJson::admitted(
                    br#"{"contractVersion":"solar-cloud-evidence/v1","generatedAt":"2026-08-28T12:00:00.000Z","evidence":null}"#.to_vec(),
                ))
            }
        }
    }
}

/// Construct one admitted canonical fixture instant.
fn instant(value: &str, epoch_ms: i64) -> AdmittedForecastRecordedInstant {
    AdmittedForecastRecordedInstant::admitted(value, epoch_ms)
}

/// Construct one exact decoded input with selected records.
fn decoded_input(records: Vec<ForecastRecordedInputRecord>) -> DecodedForecastRecordedInput {
    DecodedForecastRecordedInput {
        contract_version: "forecast-observation-input/v1".to_owned(),
        generated_at: instant("2026-08-28T12:00:00.000Z", GENERATED_MS),
        projection_from: instant("2026-08-28T07:00:00.000Z", PROJECTION_MS),
        query_from: instant("2026-08-27T11:40:00.000Z", QUERY_MS),
        records,
        site: ForecastRecordedInputSite {
            slug: "ballydidean".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        },
        through: instant("2026-08-28T12:00:00.000Z", GENERATED_MS),
    }
}

/// Construct one finite compact record for ordering fixtures.
fn record(id: &str) -> ForecastRecordedInputRecord {
    ForecastRecordedInputRecord {
        id: id.to_owned(),
        metrics: ForecastRecordedInputMetrics {
            apparent_temperature_c: None,
            cloud_cover_percent: None,
            pm25_micrograms_per_cubic_meter: None,
            precipitation_rate_mm_per_hour: None,
            pressure_hpa: Some(1_013.2),
            relative_humidity_percent: None,
            solar_radiation_wm2: None,
            temperature_c: Some(12.0),
            uv_index: None,
            wind_direction_degrees: None,
            wind_gust_mps: None,
            wind_speed_mps: None,
        },
        provenance: ForecastRecordedInputProvenance {
            provider_key: "ambient-weather-network".to_owned(),
            source_id: "source-ambient".to_owned(),
            source_key: "ambient-merlin-observations-v1".to_owned(),
            source_kind: ForecastRecordedInputSourceKind::PhysicalSensor,
            station_slug: "ambient-merlin".to_owned(),
        },
        received_at: instant("2026-08-28T11:55:00.000Z", GENERATED_MS - 5 * 60_000),
        valid_at: instant("2026-08-28T11:00:00.000Z", GENERATED_MS - 60 * 60_000),
    }
}

/// Prove fixed transport policy, valid fill composition, and optional solar isolation.
#[test]
fn fills_valid_recorded_products_through_fixed_private_request() {
    let http = FixtureHttp::success(vec![b"{\"fixture\":".to_vec(), b"true}".to_vec()]);
    let captured_request = Rc::clone(&http.request);
    let boundary = FixtureBoundary::normal();
    let trace = Rc::clone(&boundary.trace);
    let loader = HttpForecastRecordedProductsLoader::new(
        http,
        FixtureClock::new([1_000, 1_001, 1_002, 1_003]),
        boundary,
        usize::MAX,
        30_000,
    );
    let products = loader.fill(GENERATED_MS).expect("valid bounded fill");

    assert_eq!(products.day, "2026-08-28");
    assert_eq!(products.generated_at, "2026-08-28T12:00:00.000Z");
    assert_eq!(
        products.forecast_observations.as_bytes(),
        br#"{"generatedAt":"2026-08-28T12:00:00.000Z","data":[]}"#
    );
    assert!(products.solar_cloud_evidence.as_bytes().ends_with(b"null}"));
    assert_eq!(
        captured_request.borrow().as_ref(),
        Some(&ForecastRecordedInputRequest {
            accept: "application/json",
            deadline_monotonic_ms: 31_000,
            method: ForecastRecordedInputMethod::Get,
            path: "/internal/v1/forecast-observation-input",
            redirect: ForecastRecordedRedirectPolicy::Error,
        })
    );
    assert_eq!(
        trace.borrow().as_slice(),
        ["decode", "forecast", "solar", "solar-null"]
    );
}

/// Prove the absolute four-mebibyte cap applies even with a larger configured bound.
#[test]
fn refuses_the_first_byte_beyond_the_absolute_input_cap() {
    let loader = HttpForecastRecordedProductsLoader::new(
        FixtureHttp::success(vec![vec![b'x'; 4 * 1024 * 1024], vec![b'y'], vec![b'z']]),
        FixtureClock::new([1_000, 1_001, 1_002]),
        FixtureBoundary::normal(),
        usize::MAX,
        30_000,
    );
    let error = loader
        .fill(GENERATED_MS)
        .expect_err("reject byte beyond four MiB");
    assert_eq!(
        error,
        ForecastRecordedLoaderError::InputLimitExceeded::<FixtureError, FixtureError>
    );
}

/// Prove projection computation remains inside the original monotonic deadline.
#[test]
fn rejects_a_fill_that_finishes_projection_at_the_deadline() {
    let loader = HttpForecastRecordedProductsLoader::new(
        FixtureHttp::success(vec![br#"{"fixture":true}"#.to_vec()]),
        FixtureClock::new([100, 101, 110]),
        FixtureBoundary::normal(),
        4 * 1024 * 1024,
        10,
    );
    let error = loader
        .fill(GENERATED_MS)
        .expect_err("calculation deadline is inclusive");
    assert_eq!(
        error,
        ForecastRecordedLoaderError::DeadlineExceeded::<FixtureError, FixtureError>
    );
}

/// Prove equal-time identities remain in descending numeric order.
#[test]
fn rejects_ascending_record_ids_at_the_same_source_time() {
    let mut boundary = FixtureBoundary::normal();
    boundary.input = decoded_input(vec![record("1"), record("2")]);
    let loader = HttpForecastRecordedProductsLoader::new(
        FixtureHttp::success(vec![br#"{"fixture":true}"#.to_vec()]),
        FixtureClock::new([100, 101]),
        boundary,
        4 * 1024 * 1024,
        30_000,
    );
    let error = loader
        .fill(GENERATED_MS)
        .expect_err("ascending ids violate the frozen query order");
    assert_eq!(
        error,
        ForecastRecordedLoaderError::InputOrderInvalid::<FixtureError, FixtureError>
    );
}

/// Compare the fixed request and cap behavior with the deployed Node fill.
#[test]
fn matches_compiled_node_fill_witness() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/forecast-recorded-loader-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run forecast recorded loader Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("Node UTF-8"),
        "node-fill=/internal/v1/forecast-observation-input|GET|application/json|error|5|true\nnode-cap=Forecast observation input limit exceeded\n"
    );
}
