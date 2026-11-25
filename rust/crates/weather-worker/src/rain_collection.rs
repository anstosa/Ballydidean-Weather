use std::collections::BTreeMap;

use crate::TaskFailure;

const HOUR_MILLISECONDS: i64 = 3_600_000;
const FORECAST_INITIALIZATION_DELAY_MILLISECONDS: i64 = 6 * HOUR_MILLISECONDS;
const FORECAST_CYCLE_MILLISECONDS: i64 = 6 * HOUR_MILLISECONDS;
const FORECAST_SECOND_ATTEMPT_MILLISECONDS: i64 = 7 * HOUR_MILLISECONDS;
const STATION_COMPLETION_OFFSET_MILLISECONDS: i64 = 1_000;
const STATION_ENDPOINT_DELAY_MILLISECONDS: i64 = 120_000;

/// The exact prospective rain-capture controls admitted by the Node worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RainCollectionPolicy {
    pub contract_version: &'static str,
    pub expires_at_ms: i64,
    pub maximum_requests_per_iteration: usize,
    pub minimum_request_spacing_ms: u64,
    pub site_slug: &'static str,
    pub starts_at_ms: i64,
    pub station_access_authorized: bool,
    pub station_window_hours: i64,
}

pub use weather_domain::{RAIN_COLLECTION_STATIONS, RainCollectionStation};

/// Exact collection policy copied from the TypeScript contract.
pub const RAIN_COLLECTION_POLICY: RainCollectionPolicy = RainCollectionPolicy {
    contract_version: "rain-prospective-capture/v1",
    expires_at_ms: 1_822_953_600_000,
    maximum_requests_per_iteration: 2,
    minimum_request_spacing_ms: 1_100,
    site_slug: "ballydidean",
    starts_at_ms: 1_789_257_600_000,
    station_access_authorized: true,
    station_window_hours: 2,
};

/// Runtime inputs that independently admit prospective collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RainCollectionConfiguration<'a> {
    pub open_meteo_compatibility_origin: Option<&'a str>,
    pub rain_collection_enabled: bool,
    pub site_key: &'a str,
    pub version: &'a str,
}

/// One typed collection clock value, including an invalid injected clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RainCollectionTimeValue {
    Invalid,
    EpochMilliseconds(i64),
}

/// One durable credential-free capture request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RainCaptureRequest {
    Forecast {
        attempt: u8,
        run_initialized_at: String,
        slot_key: String,
    },
    Station {
        end_exclusive: String,
        slot_key: String,
        start: String,
        station_id: u64,
    },
}

impl RainCaptureRequest {
    /// Return the immutable durable slot identity.
    pub fn slot_key(&self) -> &str {
        // project the identity from either request family
        match self {
            Self::Forecast { slot_key, .. } | Self::Station { slot_key, .. } => slot_key,
        }
    }
}

/// One admitted raw capture outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RainCaptureOutcome {
    Valid,
    Invalid,
    TransportError,
    RateLimited,
    Unauthorized,
}

/// One typed provider metadata value retained with raw evidence.
#[derive(Clone, Debug, PartialEq)]
pub enum RainCaptureMetadataValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
}

/// One complete raw capture receipt admitted from the provider boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct RainCaptureReceipt {
    pub available_by_decision: Option<bool>,
    pub body: Option<Vec<u8>>,
    pub body_sha256: Option<String>,
    pub completed_at: String,
    pub error_code: Option<String>,
    pub http_status: Option<u16>,
    pub metadata: BTreeMap<String, RainCaptureMetadataValue>,
    pub outcome: RainCaptureOutcome,
    pub parser_version: &'static str,
    pub row_count: usize,
    pub started_at: String,
}

/// Aggregate evidence collection result exposed to the worker diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RainCollectionResult {
    pub attempted: usize,
    pub failed: usize,
    pub valid: usize,
}

/// Clock required by one real prospective collection attempt.
pub trait RainCollectionClock {
    /// Capture the planning instant once per collection invocation.
    fn now(&mut self) -> RainCollectionTimeValue;
}

/// Admitted UTC formatting boundary used instead of a Rust date codec.
pub trait RainCollectionTimeAdapter {
    /// Convert one planned epoch to the canonical Node UTC ISO identity.
    fn canonical_utc_iso(&mut self, epoch_ms: i64) -> Result<String, TaskFailure>;
}

/// Durable claim and append operations required by raw collection.
pub trait RainCollectionRepository {
    /// Claim one request slot before provider traffic.
    fn claim_rain_capture_slot(
        &mut self,
        request: &RainCaptureRequest,
        release: &str,
    ) -> Result<Option<String>, TaskFailure>;

    /// Consume one provider receipt without duplicating its raw response bytes.
    fn append_rain_capture_receipt(
        &mut self,
        claim_id: &str,
        receipt: RainCaptureReceipt,
    ) -> Result<(), TaskFailure>;
}

/// Typed raw-capture provider boundary.
pub trait RainCollectionProvider {
    /// Fetch one complete admitted receipt without exposing credentials.
    fn fetch_rain_capture(
        &mut self,
        request: &RainCaptureRequest,
    ) -> Result<RainCaptureReceipt, TaskFailure>;
}

/// Pacing boundary used between committed provider receipts.
pub trait RainCollectionPacer {
    /// Wait the exact policy spacing before another provider start.
    fn sleep(&mut self, milliseconds: u64) -> Result<(), TaskFailure>;
}

/// One cohesive raw collection execution and its typed ports.
pub struct RainCollectionExecution<'a, Clock, Time, Repository, Provider, Pacer> {
    pub catalog: &'a [RainCollectionStation],
    pub clock: &'a mut Clock,
    pub pacer: &'a mut Pacer,
    pub policy: &'a RainCollectionPolicy,
    pub provider: &'a mut Provider,
    pub release: &'a str,
    pub repository: &'a mut Repository,
    pub stations_authorized: bool,
    pub time_adapter: &'a mut Time,
}

// build one bounded collection failure
fn collection_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

// match the Node release allowlist without introducing a regex dependency
fn release_is_admitted(version: &str) -> bool {
    let bytes = version.as_bytes();

    // require the exact one- or two-digit release suffix shape
    if bytes.len() != 12 && bytes.len() != 13 {
        return false;
    }
    bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'.'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'.'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'-'
        && matches!(bytes[11], b'1'..=b'9')
        && (bytes.len() == 12 || bytes[12].is_ascii_digit())
}

/// Require the production-only collection boundary independently of provider credentials.
pub fn is_rain_collection_enabled(configuration: RainCollectionConfiguration<'_>) -> bool {
    configuration.rain_collection_enabled
        && configuration.open_meteo_compatibility_origin.is_none()
        && configuration.site_key == RAIN_COLLECTION_POLICY.site_slug
        && release_is_admitted(configuration.version)
}

/// Plan recent forecast slots and one fair hourly twelve-gauge sweep.
pub fn plan_rain_capture_requests<T: RainCollectionTimeAdapter>(
    now: RainCollectionTimeValue,
    stations_authorized: bool,
    policy: &RainCollectionPolicy,
    catalog: &[RainCollectionStation],
    time_adapter: &mut T,
) -> Result<Vec<RainCaptureRequest>, TaskFailure> {
    // preserve the Node invalid-clock empty plan
    let RainCollectionTimeValue::EpochMilliseconds(time_ms) = now else {
        return Ok(Vec::new());
    };

    // never collect outside the separately authorized lifetime
    if time_ms < policy.starts_at_ms || time_ms >= policy.expires_at_ms {
        return Ok(Vec::new());
    }

    let delayed_ms = time_ms
        .checked_sub(FORECAST_INITIALIZATION_DELAY_MILLISECONDS)
        .ok_or_else(|| collection_failure("rain capture time overflow"))?;
    let initialized_ms =
        delayed_ms.div_euclid(FORECAST_CYCLE_MILLISECONDS) * FORECAST_CYCLE_MILLISECONDS;
    let run_initialized_at = time_adapter.canonical_utc_iso(initialized_ms)?;
    let elapsed_ms = time_ms
        .checked_sub(initialized_ms)
        .ok_or_else(|| collection_failure("rain capture time overflow"))?;
    let attempt = if elapsed_ms < FORECAST_SECOND_ATTEMPT_MILLISECONDS {
        1
    } else {
        2
    };
    let mut requests = vec![RainCaptureRequest::Forecast {
        attempt,
        slot_key: format!("forecast:{run_initialized_at}:{attempt}"),
        run_initialized_at,
    }];

    // require a distinct operator-confirmed public-station entitlement
    if !stations_authorized {
        return Ok(requests);
    }

    let station_base_ms = time_ms
        .checked_sub(STATION_ENDPOINT_DELAY_MILLISECONDS)
        .ok_or_else(|| collection_failure("rain capture time overflow"))?;
    let station_hour_ms = station_base_ms.div_euclid(HOUR_MILLISECONDS) * HOUR_MILLISECONDS;
    let end_exclusive_ms = station_hour_ms
        .checked_add(STATION_COMPLETION_OFFSET_MILLISECONDS)
        .ok_or_else(|| collection_failure("rain capture time overflow"))?;
    let window_ms = policy
        .station_window_hours
        .checked_mul(HOUR_MILLISECONDS)
        .ok_or_else(|| collection_failure("rain capture time overflow"))?;
    let start_ms = end_exclusive_ms
        .checked_sub(window_ms)
        .ok_or_else(|| collection_failure("rain capture time overflow"))?;
    let end_exclusive = time_adapter.canonical_utc_iso(end_exclusive_ms)?;
    let start = time_adapter.canonical_utc_iso(start_ms)?;

    // preserve the fixed empty-catalog boundary without modulo arithmetic
    if catalog.is_empty() {
        return Ok(requests);
    }
    let catalog_length = i64::try_from(catalog.len())
        .map_err(|_| collection_failure("rain station catalog is too large"))?;
    let rotation = station_hour_ms
        .div_euclid(HOUR_MILLISECONDS)
        .rem_euclid(catalog_length) as usize;

    // rotate priority so prolonged outages cannot always starve the same gauges
    for offset in 0..catalog.len() {
        let index = (rotation + offset) % catalog.len();
        let station = catalog
            .get(index)
            .ok_or_else(|| collection_failure("rain station catalog is incomplete"))?;
        requests.push(RainCaptureRequest::Station {
            end_exclusive: end_exclusive.clone(),
            slot_key: format!("station:{}:{end_exclusive}", station.location_id),
            start: start.clone(),
            station_id: station.location_id,
        });
    }

    Ok(requests)
}

/// Capture bounded append-only raw evidence without changing ordinary ingestion.
pub fn collect_rain_evidence<Clock, Time, Repository, Provider, Pacer>(
    execution: &mut RainCollectionExecution<'_, Clock, Time, Repository, Provider, Pacer>,
) -> Result<RainCollectionResult, TaskFailure>
where
    Clock: RainCollectionClock,
    Pacer: RainCollectionPacer,
    Provider: RainCollectionProvider,
    Repository: RainCollectionRepository,
    Time: RainCollectionTimeAdapter,
{
    let requests = plan_rain_capture_requests(
        execution.clock.now(),
        execution.stations_authorized,
        execution.policy,
        execution.catalog,
        execution.time_adapter,
    )?;
    let mut result = RainCollectionResult {
        attempted: 0,
        failed: 0,
        valid: 0,
    };

    // let durable uniqueness and cooldown checks arbitrate every individual slot
    for request in &requests {
        // preserve a fixed per-iteration budget even after failed captures
        if result.attempted >= execution.policy.maximum_requests_per_iteration {
            break;
        }
        let claim_id = execution
            .repository
            .claim_rain_capture_slot(request, execution.release)?;

        // do not repeat successful, unknown, or otherwise unavailable slots
        let Some(claim_id) = claim_id else {
            continue;
        };
        result.attempted += 1;

        let receipt = match execution.provider.fetch_rain_capture(request) {
            Ok(receipt) => receipt,
            Err(_) => {
                result.failed += 1;
                break;
            }
        };

        let outcome = receipt.outcome;
        // leave an explicit unknown claim when receipt storage fails
        if execution
            .repository
            .append_rain_capture_receipt(&claim_id, receipt)
            .is_err()
        {
            result.failed += 1;
            break;
        }

        // classify valid evidence separately from every retained provider failure
        if outcome == RainCaptureOutcome::Valid {
            result.valid += 1;
        } else {
            result.failed += 1;
        }

        // suspend immediately after explicit provider access or rate errors
        if matches!(
            outcome,
            RainCaptureOutcome::RateLimited | RainCaptureOutcome::Unauthorized
        ) {
            break;
        }

        // pace only while the current iteration retains another request budget
        if result.attempted < execution.policy.maximum_requests_per_iteration
            && execution
                .pacer
                .sleep(execution.policy.minimum_request_spacing_ms)
                .is_err()
        {
            result.failed += 1;
            break;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    use super::{
        RAIN_COLLECTION_POLICY, RAIN_COLLECTION_STATIONS, RainCaptureMetadataValue,
        RainCaptureOutcome, RainCaptureReceipt, RainCaptureRequest, RainCollectionClock,
        RainCollectionConfiguration, RainCollectionExecution, RainCollectionPacer,
        RainCollectionProvider, RainCollectionRepository, RainCollectionResult,
        RainCollectionTimeAdapter, RainCollectionTimeValue, collect_rain_evidence,
        is_rain_collection_enabled, plan_rain_capture_requests,
    };
    use crate::TaskFailure;

    const FIXTURE_NOW_MS: i64 = 1_789_387_800_000;

    // build one bounded fixture failure
    fn failure(code: &str) -> TaskFailure {
        TaskFailure {
            attempts: 0,
            code: code.to_owned(),
        }
    }

    #[derive(Default)]
    struct FixtureTimeAdapter;

    impl RainCollectionTimeAdapter for FixtureTimeAdapter {
        // admit only the exact fixture instants used by the Node parity cases
        fn canonical_utc_iso(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
            let value = match epoch_ms {
                1_789_365_600_000 => "2026-09-14T06:00:00.000Z",
                1_789_380_001_000 => "2026-09-14T10:00:01.000Z",
                1_789_387_201_000 => "2026-09-14T12:00:01.000Z",
                _ => return Err(failure("unexpected fixture timestamp")),
            };
            Ok(value.to_owned())
        }
    }

    struct FixtureClock(RainCollectionTimeValue);

    impl RainCollectionClock for FixtureClock {
        // return one injected collection clock
        fn now(&mut self) -> RainCollectionTimeValue {
            self.0
        }
    }

    struct FixtureRepository {
        claimed_slots: BTreeSet<String>,
        events: Vec<String>,
        fail_append: bool,
    }

    impl RainCollectionRepository for FixtureRepository {
        // retain the exact claim order and deduplicate durable slot identities
        fn claim_rain_capture_slot(
            &mut self,
            request: &RainCaptureRequest,
            release: &str,
        ) -> Result<Option<String>, TaskFailure> {
            self.events
                .push(format!("claim:{}:{release}", request.slot_key()));

            // preserve already claimed slots without provider replay
            if !self.claimed_slots.insert(request.slot_key().to_owned()) {
                return Ok(None);
            }
            Ok(Some(request.slot_key().to_owned()))
        }

        // retain raw receipts after provider completion
        fn append_rain_capture_receipt(
            &mut self,
            claim_id: &str,
            receipt: RainCaptureReceipt,
        ) -> Result<(), TaskFailure> {
            self.events
                .push(format!("receipt:{claim_id}:{:?}", receipt.outcome));

            // inject one storage failure after the provider response
            if self.fail_append {
                return Err(failure("storage unavailable"));
            }
            Ok(())
        }
    }

    struct FixtureProvider {
        events: Vec<String>,
        outcomes: VecDeque<RainCaptureOutcome>,
    }

    impl RainCollectionProvider for FixtureProvider {
        // return one complete raw receipt for the selected request
        fn fetch_rain_capture(
            &mut self,
            request: &RainCaptureRequest,
        ) -> Result<RainCaptureReceipt, TaskFailure> {
            self.events.push(format!("fetch:{}", request.slot_key()));
            let outcome = self
                .outcomes
                .pop_front()
                .ok_or_else(|| failure("missing provider fixture"))?;
            Ok(RainCaptureReceipt {
                available_by_decision: Some(true),
                body: Some(b"{}".to_vec()),
                body_sha256: Some("fixture".to_owned()),
                completed_at: "2026-09-14T12:10:00.000Z".to_owned(),
                error_code: None,
                http_status: Some(200),
                metadata: BTreeMap::from([(
                    "request_kind".to_owned(),
                    RainCaptureMetadataValue::String(match request {
                        RainCaptureRequest::Forecast { .. } => "forecast".to_owned(),
                        RainCaptureRequest::Station { .. } => "station".to_owned(),
                    }),
                )]),
                outcome,
                parser_version: RAIN_COLLECTION_POLICY.contract_version,
                row_count: 48,
                started_at: "2026-09-14T12:10:00.000Z".to_owned(),
            })
        }
    }

    #[derive(Default)]
    struct FixturePacer(Vec<u64>);

    impl RainCollectionPacer for FixturePacer {
        // retain every exact pacing delay without sleeping
        fn sleep(&mut self, milliseconds: u64) -> Result<(), TaskFailure> {
            self.0.push(milliseconds);
            Ok(())
        }
    }

    // preserve the exact independent opt-in boundary
    #[test]
    fn rain_collection_enablement_matches_node_release_and_runtime_guards() {
        let enabled = RainCollectionConfiguration {
            open_meteo_compatibility_origin: None,
            rain_collection_enabled: true,
            site_key: "ballydidean",
            version: "2026.09.14-1",
        };
        assert!(is_rain_collection_enabled(enabled));

        // reject every independently guarded configuration drift
        for rejected in [
            RainCollectionConfiguration {
                rain_collection_enabled: false,
                ..enabled
            },
            RainCollectionConfiguration {
                open_meteo_compatibility_origin: Some("http://fixture:8080"),
                ..enabled
            },
            RainCollectionConfiguration {
                site_key: "another-site",
                ..enabled
            },
            RainCollectionConfiguration {
                version: "development",
                ..enabled
            },
            RainCollectionConfiguration {
                version: "2026.09.14-0",
                ..enabled
            },
            RainCollectionConfiguration {
                version: "2026.09.14-100",
                ..enabled
            },
        ] {
            assert!(!is_rain_collection_enabled(rejected));
        }
    }

    // match the compiled Node request identities, windows, and fixed catalog order
    #[test]
    fn rain_capture_planner_matches_node_forecast_and_station_fixture() {
        let mut time_adapter = FixtureTimeAdapter;
        let requests = plan_rain_capture_requests(
            RainCollectionTimeValue::EpochMilliseconds(FIXTURE_NOW_MS),
            true,
            &RAIN_COLLECTION_POLICY,
            RAIN_COLLECTION_STATIONS,
            &mut time_adapter,
        )
        .expect("fixture planning should succeed");

        assert_eq!(requests.len(), 13);
        assert_eq!(
            requests[0],
            RainCaptureRequest::Forecast {
                attempt: 1,
                run_initialized_at: "2026-09-14T06:00:00.000Z".to_owned(),
                slot_key: "forecast:2026-09-14T06:00:00.000Z:1".to_owned(),
            }
        );
        assert_eq!(
            requests[1],
            RainCaptureRequest::Station {
                end_exclusive: "2026-09-14T12:00:01.000Z".to_owned(),
                slot_key: "station:64255:2026-09-14T12:00:01.000Z".to_owned(),
                start: "2026-09-14T10:00:01.000Z".to_owned(),
                station_id: 64_255,
            }
        );
        assert_eq!(
            requests
                .iter()
                .skip(1)
                .map(|request| match request {
                    RainCaptureRequest::Station { station_id, .. } => *station_id,
                    RainCaptureRequest::Forecast { .. } => 0,
                })
                .collect::<Vec<_>>(),
            [
                64_255, 225_947, 38_270, 168_853, 126_537, 201_058, 203_055, 66_270, 34_768,
                88_159, 126_197, 27_140,
            ]
        );
        assert!(
            plan_rain_capture_requests(
                RainCollectionTimeValue::Invalid,
                true,
                &RAIN_COLLECTION_POLICY,
                RAIN_COLLECTION_STATIONS,
                &mut time_adapter,
            )
            .expect("invalid clock should produce an empty plan")
            .is_empty()
        );
    }

    // execute durable claims, raw receipts, and pacing in Node order
    #[test]
    fn rain_collection_executes_two_claimed_requests_in_order() {
        let mut clock = FixtureClock(RainCollectionTimeValue::EpochMilliseconds(FIXTURE_NOW_MS));
        let mut time_adapter = FixtureTimeAdapter;
        let mut repository = FixtureRepository {
            claimed_slots: BTreeSet::new(),
            events: Vec::new(),
            fail_append: false,
        };
        let mut provider = FixtureProvider {
            events: Vec::new(),
            outcomes: VecDeque::from([RainCaptureOutcome::Valid, RainCaptureOutcome::Invalid]),
        };
        let mut pacer = FixturePacer::default();
        let result = collect_rain_evidence(&mut RainCollectionExecution {
            catalog: RAIN_COLLECTION_STATIONS,
            clock: &mut clock,
            pacer: &mut pacer,
            policy: &RAIN_COLLECTION_POLICY,
            provider: &mut provider,
            release: "2026.09.14-1",
            repository: &mut repository,
            stations_authorized: true,
            time_adapter: &mut time_adapter,
        })
        .expect("fixture collection should complete");

        assert_eq!(
            result,
            RainCollectionResult {
                attempted: 2,
                failed: 1,
                valid: 1,
            }
        );
        assert_eq!(provider.events.len(), 2);
        assert!(provider.events[0].contains("forecast:"));
        assert!(provider.events[1].contains("station:64255:"));
        assert_eq!(repository.events.len(), 4);
        assert!(repository.events[0].starts_with("claim:forecast:"));
        assert!(repository.events[1].starts_with("receipt:forecast:"));
        assert!(repository.events[2].starts_with("claim:station:64255:"));
        assert!(repository.events[3].starts_with("receipt:station:64255:"));
        assert_eq!(pacer.0, [1_100]);
    }

    // skip durable duplicates while retaining station catalog order and attempt budget
    #[test]
    fn rain_collection_deduplicates_without_replaying_provider_work() {
        let mut clock = FixtureClock(RainCollectionTimeValue::EpochMilliseconds(FIXTURE_NOW_MS));
        let mut time_adapter = FixtureTimeAdapter;
        let mut repository = FixtureRepository {
            claimed_slots: BTreeSet::from(["forecast:2026-09-14T06:00:00.000Z:1".to_owned()]),
            events: Vec::new(),
            fail_append: false,
        };
        let mut provider = FixtureProvider {
            events: Vec::new(),
            outcomes: VecDeque::from([RainCaptureOutcome::Valid, RainCaptureOutcome::Valid]),
        };
        let mut pacer = FixturePacer::default();
        let result = collect_rain_evidence(&mut RainCollectionExecution {
            catalog: RAIN_COLLECTION_STATIONS,
            clock: &mut clock,
            pacer: &mut pacer,
            policy: &RAIN_COLLECTION_POLICY,
            provider: &mut provider,
            release: "2026.09.14-1",
            repository: &mut repository,
            stations_authorized: true,
            time_adapter: &mut time_adapter,
        })
        .expect("durable duplicate should remain a non-error skip");

        assert_eq!(
            result,
            RainCollectionResult {
                attempted: 2,
                failed: 0,
                valid: 2,
            }
        );
        assert_eq!(provider.events.len(), 2);
        assert!(provider.events[0].contains("station:64255:"));
        assert!(provider.events[1].contains("station:225947:"));
        assert_eq!(pacer.0, [1_100]);
    }

    // stop after retained provider suspension and after unknown receipt storage
    #[test]
    fn rain_collection_preserves_provider_and_storage_failure_boundaries() {
        // verify both explicit provider suspension outcomes
        for outcome in [
            RainCaptureOutcome::RateLimited,
            RainCaptureOutcome::Unauthorized,
        ] {
            let mut clock =
                FixtureClock(RainCollectionTimeValue::EpochMilliseconds(FIXTURE_NOW_MS));
            let mut time_adapter = FixtureTimeAdapter;
            let mut repository = FixtureRepository {
                claimed_slots: BTreeSet::new(),
                events: Vec::new(),
                fail_append: false,
            };
            let mut provider = FixtureProvider {
                events: Vec::new(),
                outcomes: VecDeque::from([outcome]),
            };
            let mut pacer = FixturePacer::default();
            let result = collect_rain_evidence(&mut RainCollectionExecution {
                catalog: RAIN_COLLECTION_STATIONS,
                clock: &mut clock,
                pacer: &mut pacer,
                policy: &RAIN_COLLECTION_POLICY,
                provider: &mut provider,
                release: "2026.09.14-1",
                repository: &mut repository,
                stations_authorized: true,
                time_adapter: &mut time_adapter,
            })
            .expect("provider suspension should remain a retained result");

            assert_eq!(
                result,
                RainCollectionResult {
                    attempted: 1,
                    failed: 1,
                    valid: 0,
                }
            );
            assert_eq!(provider.events.len(), 1);
            assert!(pacer.0.is_empty());
        }

        let mut clock = FixtureClock(RainCollectionTimeValue::EpochMilliseconds(FIXTURE_NOW_MS));
        let mut time_adapter = FixtureTimeAdapter;
        let mut repository = FixtureRepository {
            claimed_slots: BTreeSet::new(),
            events: Vec::new(),
            fail_append: true,
        };
        let mut provider = FixtureProvider {
            events: Vec::new(),
            outcomes: VecDeque::from([RainCaptureOutcome::Valid]),
        };
        let mut pacer = FixturePacer::default();
        let result = collect_rain_evidence(&mut RainCollectionExecution {
            catalog: RAIN_COLLECTION_STATIONS,
            clock: &mut clock,
            pacer: &mut pacer,
            policy: &RAIN_COLLECTION_POLICY,
            provider: &mut provider,
            release: "2026.09.14-1",
            repository: &mut repository,
            stations_authorized: true,
            time_adapter: &mut time_adapter,
        })
        .expect("storage failure should stay isolated in the collection result");

        assert_eq!(
            result,
            RainCollectionResult {
                attempted: 1,
                failed: 1,
                valid: 0,
            }
        );
        assert!(pacer.0.is_empty());
    }
}
