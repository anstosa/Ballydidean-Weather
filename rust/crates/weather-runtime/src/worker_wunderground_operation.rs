//! Execute typed Weather Underground local-day requests through the shared provider controller.

use weather_database::Sha256Digest;
use weather_providers::{
    ProviderIo, ProviderRequestPolicy, ProviderResponse, run_provider_request,
};
use weather_worker::{
    SupplementalPublicStationBatch, SupplementalPublicStationIdentity,
    SupplementalPublicStationProduct, SupplementalPublicStationScheduledRequest, TaskFailure,
    WeatherUndergroundObservation, normalize_supplemental_public_station_batch_with_diagnostics,
};

use crate::{
    SupplementalPublicStationOperationError, WorkerPublicStationEvidence,
    validate_worker_public_station_range, worker_public_station_digest,
};

const MAXIMUM_WUNDERGROUND_RANGE_MS: i64 = 24 * 60 * 60 * 1_000;
const WUNDERGROUND_FORMAT: &str = "json";
const WUNDERGROUND_NUMERIC_PRECISION: &str = "decimal";
const WUNDERGROUND_UNITS: &str = "m";

/// one approved source-local date in the provider's compact request form
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WundergroundLocalDate(String);

impl WundergroundLocalDate {
    /// admit one calendar-owner-produced compact local date
    pub fn admitted_yyyymmdd(value: impl Into<String>) -> Result<Self, TaskFailure> {
        let value = value.into();
        // retain only the provider's exact compact numeric representation
        if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid_request());
        }
        Ok(Self(value))
    }

    /// borrow the compact provider request value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// one credential-bound local-day request passed to an admitted transport
pub struct WundergroundDayRequest<'a> {
    pub station_id: &'a str,
    pub public_api_key: &'a str,
    pub date: &'a WundergroundLocalDate,
    pub format: &'static str,
    pub units: &'static str,
    pub numeric_precision: &'static str,
}

/// one decoded local-day response with its approved raw-body digest
#[derive(Clone, Debug, PartialEq)]
pub struct WundergroundDayResponse {
    pub checksum: Sha256Digest,
    pub observations: Vec<WeatherUndergroundObservation>,
}

/// admitted local calendar, receipt clock, and request-bound transport owner
pub trait WorkerWundergroundTransport {
    type Day<'a>: ProviderIo<WundergroundDayResponse>
    where
        Self: 'a;

    /// resolve one UTC instant through the configured source-local calendar
    fn local_date(
        &mut self,
        epoch_ms: i64,
        timezone: &str,
    ) -> Result<WundergroundLocalDate, TaskFailure>;

    /// bind one exact provider day without exposing credentials to the operation
    fn bind_day<'a>(
        &'a mut self,
        request: &WundergroundDayRequest<'_>,
    ) -> Result<Self::Day<'a>, TaskFailure>;

    /// sample the provider receipt clock after every local-day response succeeds
    fn received_at_ms(&mut self) -> i64;
}

/// fetch, evidence, and normalize one scheduled Weather Underground UTC range
pub fn fetch_worker_wunderground(
    transport: &mut impl WorkerWundergroundTransport,
    evidence: &mut impl WorkerPublicStationEvidence,
    request: &SupplementalPublicStationScheduledRequest,
    station_id: &str,
    public_api_key: &str,
    policy: ProviderRequestPolicy,
) -> Result<SupplementalPublicStationBatch, TaskFailure> {
    fetch_worker_wunderground_operation(
        transport,
        evidence,
        request,
        station_id,
        public_api_key,
        policy,
    )
    .map_err(SupplementalPublicStationOperationError::into_task_failure)
}

/// Retain classified provider controller failures for a one-shot archive owner.
pub(crate) fn fetch_worker_wunderground_operation(
    transport: &mut impl WorkerWundergroundTransport,
    evidence: &mut impl WorkerPublicStationEvidence,
    request: &SupplementalPublicStationScheduledRequest,
    station_id: &str,
    public_api_key: &str,
    policy: ProviderRequestPolicy,
) -> Result<SupplementalPublicStationBatch, SupplementalPublicStationOperationError> {
    validate_worker_public_station_range(request.window, MAXIMUM_WUNDERGROUND_RANGE_MS)?;
    validate_identity(request, station_id, public_api_key)?;
    let last_inclusive_ms = request
        .window
        .end_exclusive_ms
        .checked_sub(1)
        .ok_or_else(invalid_request)?;
    let first_date = transport.local_date(request.window.start_ms, &request.timezone)?;
    let last_date = transport.local_date(last_inclusive_ms, &request.timezone)?;
    let mut dates = vec![first_date];
    // avoid a second request when both UTC boundaries occupy one local day
    if dates[0] != last_date {
        dates.push(last_date);
    }

    let mut responses: Vec<ProviderResponse<WundergroundDayResponse>> =
        Vec::with_capacity(dates.len());
    // retain local-day order while sharing one absolute deadline across retry controllers
    for date in &dates {
        let binding = WundergroundDayRequest {
            station_id,
            public_api_key,
            date,
            format: WUNDERGROUND_FORMAT,
            units: WUNDERGROUND_UNITS,
            numeric_precision: WUNDERGROUND_NUMERIC_PRECISION,
        };
        let mut day = transport.bind_day(&binding)?;
        responses.push(run_provider_request(policy, &mut day)?);
    }

    let received_at_ms = transport.received_at_ms();
    let attempts = responses
        .iter()
        .map(|response| u32::from(response.attempts))
        .sum();
    let checksums = responses
        .iter()
        .map(|response| response.body.checksum.clone())
        .collect::<Vec<_>>();
    let days = responses
        .into_iter()
        .map(|response| response.body.observations)
        .collect::<Vec<_>>();

    let mut batch = normalize_supplemental_public_station_batch_with_diagnostics(
        request.normalization_inputs(),
        SupplementalPublicStationProduct::WeatherUnderground {
            station_id,
            days: &days,
        },
        received_at_ms,
        attempts,
        String::new(),
    )?;
    let checksum_inputs = checksums.iter().collect::<Vec<_>>();
    batch.upstream_response_checksum = worker_public_station_digest(evidence, &checksum_inputs)?;
    Ok(batch)
}

/// validate frozen request identity before opening a transport
fn validate_identity(
    request: &SupplementalPublicStationScheduledRequest,
    station_id: &str,
    public_api_key: &str,
) -> Result<(), TaskFailure> {
    // bind the operation arguments to the catalog identity retained on the scheduled request
    let SupplementalPublicStationIdentity::WeatherUnderground {
        station_id: expected_station_id,
        public_api_key: expected_public_api_key,
    } = &request.identity
    else {
        return Err(invalid_request());
    };
    let station_bytes = station_id.as_bytes();
    let public_key_bytes = public_api_key.as_bytes();
    // match the provider's exact ASCII identifier grammars and retained credential values
    if station_id != expected_station_id
        || public_api_key != expected_public_api_key
        || !(3..=32).contains(&station_bytes.len())
        || !station_bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        || public_key_bytes.len() != 32
        || !public_key_bytes
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid_request());
    }
    Ok(())
}

/// preserve a pre-traffic Node operation failure at the worker boundary
fn invalid_request() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "provider_unavailable".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use weather_providers::{
        ContentBudget, ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse,
        ProviderControls, ProviderFailure, ProviderFailureClassification, ProviderRunError,
        ProviderSleep, ProviderSleepError,
    };
    use weather_worker::{
        PublicStationNumber, ScheduledWindow, WeatherUndergroundMetricObservation,
        WeatherUndergroundQcStatus,
    };

    use super::*;

    const STATION_ID: &str = "KCASANFR123";
    const PUBLIC_API_KEY: &str = "0123456789abcdef0123456789abcdef";
    const FIRST_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SECOND_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const COMBINED_SHA: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const START_MS: i64 = 1_800_000_000_000;

    #[derive(Debug, Eq, PartialEq)]
    struct BoundDay {
        date: String,
        format: &'static str,
        numeric_precision: &'static str,
        station_id: String,
        units: &'static str,
    }

    struct DayIo {
        attempts: Rc<RefCell<Vec<ProviderAttempt>>>,
        failure: Option<ProviderFailure>,
        now_ms: i64,
        response: WundergroundDayResponse,
        statuses: VecDeque<u16>,
    }

    impl ProviderIo<WundergroundDayResponse> for DayIo {
        /// return one stable attempt clock below the operation deadline
        fn now_ms(&mut self) -> i64 {
            self.now_ms
        }

        /// use the deterministic minimum retry jitter
        fn random_unit(&mut self) -> f64 {
            0.0
        }

        /// retain each physical attempt and return the decoded day product
        fn execute(
            &mut self,
            attempt: ProviderAttempt,
        ) -> Result<ProviderAttemptResponse<WundergroundDayResponse>, ProviderAttemptError>
        {
            self.attempts.borrow_mut().push(attempt);
            // return the exact classified transport diagnosis for archive failure coverage
            if let Some(failure) = self.failure.take() {
                return Err(ProviderAttemptError::Classified(failure));
            }
            Ok(ProviderAttemptResponse {
                body: self.response.clone(),
                content: ContentBudget {
                    declared_body_bytes: Some(128),
                    streamed_body_bytes: 128,
                },
                provider_reason: None,
                retry_after_ms: None,
                status: self
                    .statuses
                    .pop_front()
                    .expect("fixture response status must exist"),
            })
        }

        /// admit the first day's one bounded retry delay
        fn sleep(&mut self, _sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
            self.now_ms += 250;
            Ok(())
        }
    }

    struct Transport {
        bindings: Vec<BoundDay>,
        days: VecDeque<DayIo>,
        local_date_calls: Vec<(i64, String)>,
        local_dates: VecDeque<WundergroundLocalDate>,
        receipt_calls: usize,
    }

    impl WorkerWundergroundTransport for Transport {
        type Day<'a> = DayIo;

        /// return the approved fixture calendar projection in request order
        fn local_date(
            &mut self,
            epoch_ms: i64,
            timezone: &str,
        ) -> Result<WundergroundLocalDate, TaskFailure> {
            self.local_date_calls.push((epoch_ms, timezone.to_owned()));
            self.local_dates.pop_front().ok_or_else(invalid_request)
        }

        /// retain only noncredential binding evidence before returning the next exchange
        fn bind_day<'a>(
            &'a mut self,
            request: &WundergroundDayRequest<'_>,
        ) -> Result<Self::Day<'a>, TaskFailure> {
            assert_eq!(request.public_api_key, PUBLIC_API_KEY);
            self.bindings.push(BoundDay {
                date: request.date.as_str().to_owned(),
                format: request.format,
                numeric_precision: request.numeric_precision,
                station_id: request.station_id.to_owned(),
                units: request.units,
            });
            self.days.pop_front().ok_or_else(invalid_request)
        }

        /// retain the one post-response receipt-clock sample
        fn received_at_ms(&mut self) -> i64 {
            self.receipt_calls += 1;
            START_MS + 3_600_000
        }
    }

    struct Evidence {
        materials: Vec<Vec<u8>>,
    }

    impl WorkerPublicStationEvidence for Evidence {
        /// retain the exact ordered response digest material
        fn sha256(&mut self, material: &[u8]) -> Result<Sha256Digest, TaskFailure> {
            self.materials.push(material.to_vec());
            Ok(Sha256Digest::admitted(COMBINED_SHA))
        }
    }

    /// prove local-day order, shared policy attempts, receipt timing, and normalized output
    #[test]
    fn fetches_ordered_days_and_combines_attempts_and_evidence() {
        let attempts = Rc::new(RefCell::new(Vec::new()));
        let first_epoch_seconds = START_MS / 1_000 + 600;
        let second_epoch_seconds = START_MS / 1_000 + 1_200;
        let mut transport = Transport {
            bindings: Vec::new(),
            days: VecDeque::from([
                day_io(
                    &attempts,
                    FIRST_SHA,
                    first_epoch_seconds,
                    VecDeque::from([500, 200]),
                ),
                day_io(
                    &attempts,
                    SECOND_SHA,
                    second_epoch_seconds,
                    VecDeque::from([200]),
                ),
            ]),
            local_date_calls: Vec::new(),
            local_dates: VecDeque::from([
                WundergroundLocalDate::admitted_yyyymmdd("20261122").unwrap(),
                WundergroundLocalDate::admitted_yyyymmdd("20261123").unwrap(),
            ]),
            receipt_calls: 0,
        };
        let request = scheduled_request(ScheduledWindow {
            start_ms: START_MS,
            end_exclusive_ms: START_MS + 3_600_000,
        });
        let mut evidence = Evidence {
            materials: Vec::new(),
        };

        let batch = fetch_worker_wunderground(
            &mut transport,
            &mut evidence,
            &request,
            STATION_ID,
            PUBLIC_API_KEY,
            ProviderRequestPolicy {
                controls: ProviderControls {
                    max_attempts: 2,
                    max_body_bytes: 1_024,
                    timeout_ms: 1_000,
                },
                deadline_at_ms: None,
            },
        )
        .expect("typed local-day operation should succeed");

        assert_eq!(batch.attempts, 3);
        assert_eq!(batch.records.len(), 2);
        assert_eq!(batch.provider_cursor, Some(second_epoch_seconds * 1_000));
        assert_eq!(batch.upstream_response_checksum, COMBINED_SHA);
        assert_eq!(transport.receipt_calls, 1);
        assert_eq!(
            transport.local_date_calls,
            [
                (START_MS, "UTC".to_owned()),
                (START_MS + 3_600_000 - 1, "UTC".to_owned()),
            ]
        );
        assert_eq!(
            transport.bindings,
            [
                BoundDay {
                    date: "20261122".to_owned(),
                    format: "json",
                    numeric_precision: "decimal",
                    station_id: STATION_ID.to_owned(),
                    units: "m",
                },
                BoundDay {
                    date: "20261123".to_owned(),
                    format: "json",
                    numeric_precision: "decimal",
                    station_id: STATION_ID.to_owned(),
                    units: "m",
                },
            ]
        );
        assert_eq!(attempts.borrow().len(), 3);
        assert_eq!(
            evidence.materials,
            [format!("{FIRST_SHA}\n{SECOND_SHA}\n").into_bytes()]
        );
    }

    /// Retain a classified HTTP diagnosis before the archive catch boundary.
    #[test]
    fn archive_operation_retains_classified_controller_failure() {
        let attempts = Rc::new(RefCell::new(Vec::new()));
        let mut failed_day = day_io(&attempts, FIRST_SHA, START_MS / 1_000, VecDeque::new());
        failed_day.failure = Some(ProviderFailure::classified(
            1,
            ProviderFailureClassification::Permanent,
            "provider_forbidden",
            "station history is not available",
            Some(403),
        ));
        let mut transport = Transport {
            bindings: Vec::new(),
            days: VecDeque::from([failed_day]),
            local_date_calls: Vec::new(),
            local_dates: VecDeque::from([
                WundergroundLocalDate::admitted_yyyymmdd("20261122").unwrap(),
                WundergroundLocalDate::admitted_yyyymmdd("20261122").unwrap(),
            ]),
            receipt_calls: 0,
        };
        let request = scheduled_request(ScheduledWindow {
            start_ms: START_MS,
            end_exclusive_ms: START_MS + 3_600_000,
        });
        let mut evidence = Evidence {
            materials: Vec::new(),
        };

        let error = fetch_worker_wunderground_operation(
            &mut transport,
            &mut evidence,
            &request,
            STATION_ID,
            PUBLIC_API_KEY,
            ProviderRequestPolicy {
                controls: ProviderControls::default(),
                deadline_at_ms: None,
            },
        )
        .expect_err("classified controller failure");
        let SupplementalPublicStationOperationError::Provider(ProviderRunError::Provider(failure)) =
            error
        else {
            panic!("expected exact provider failure");
        };
        assert_eq!(failure.attempts, 1);
        assert_eq!(
            failure.error.classification,
            ProviderFailureClassification::Permanent
        );
        assert_eq!(failure.error.code, "provider_forbidden");
        assert_eq!(failure.error.message, "station history is not available");
        assert_eq!(failure.status, Some(403));
    }

    /// Carry a malformed decoded row through the physical operation into archive failure evidence.
    #[test]
    fn archive_operation_retains_decoded_station_failure() {
        let attempts = Rc::new(RefCell::new(Vec::new()));
        let mut day = day_io(
            &attempts,
            FIRST_SHA,
            START_MS / 1_000,
            VecDeque::from([200]),
        );
        day.response.observations[0].station_id = "different-station".to_owned();
        let mut transport = Transport {
            bindings: Vec::new(),
            days: VecDeque::from([day]),
            local_date_calls: Vec::new(),
            local_dates: VecDeque::from([
                WundergroundLocalDate::admitted_yyyymmdd("20261122").unwrap(),
                WundergroundLocalDate::admitted_yyyymmdd("20261122").unwrap(),
            ]),
            receipt_calls: 0,
        };
        let request = scheduled_request(ScheduledWindow {
            start_ms: START_MS,
            end_exclusive_ms: START_MS + 3_600_000,
        });
        let mut evidence = Evidence {
            materials: Vec::new(),
        };
        let error = fetch_worker_wunderground_operation(
            &mut transport,
            &mut evidence,
            &request,
            STATION_ID,
            PUBLIC_API_KEY,
            ProviderRequestPolicy {
                controls: ProviderControls::default(),
                deadline_at_ms: None,
            },
        )
        .expect_err("malformed station row");
        assert_eq!(
            error.into_backfill_failure(),
            weather_worker::PublicStationBackfillOperationError::provider(
                1,
                weather_worker::TideBackfillErrorClassification::InvalidPayload,
                "invalid_payload",
                "Weather Underground station identity is invalid",
            ),
        );
        assert_eq!(attempts.borrow().len(), 1);
        assert!(evidence.materials.is_empty());
    }

    /// build one request carrying the same frozen Weather Underground identity
    fn scheduled_request(
        window: weather_worker::ScheduledWindow,
    ) -> SupplementalPublicStationScheduledRequest {
        SupplementalPublicStationScheduledRequest {
            identity: SupplementalPublicStationIdentity::WeatherUnderground {
                station_id: STATION_ID.to_owned(),
                public_api_key: PUBLIC_API_KEY.to_owned(),
            },
            model: Some("WS-2902".to_owned()),
            serial: Some("station-1".to_owned()),
            source_id: "source-wunderground".to_owned(),
            timezone: "UTC".to_owned(),
            window,
        }
    }

    /// build one deterministic decoded local-day exchange
    fn day_io(
        attempts: &Rc<RefCell<Vec<ProviderAttempt>>>,
        checksum: &str,
        epoch_seconds: i64,
        statuses: VecDeque<u16>,
    ) -> DayIo {
        DayIo {
            attempts: attempts.clone(),
            failure: None,
            now_ms: START_MS,
            response: WundergroundDayResponse {
                checksum: Sha256Digest::admitted(checksum),
                observations: vec![observation(epoch_seconds)],
            },
            statuses,
        }
    }

    /// build one valid decoded Weather Underground observation
    fn observation(epoch_seconds: i64) -> WeatherUndergroundObservation {
        WeatherUndergroundObservation {
            epoch_seconds,
            humidity_avg_percent: PublicStationNumber::Number(55.0),
            metric: Some(WeatherUndergroundMetricObservation {
                precip_rate_mm_per_hour: PublicStationNumber::Number(0.0),
                pressure_max_hpa: PublicStationNumber::Number(1_010.0),
                pressure_min_hpa: PublicStationNumber::Number(1_008.0),
                temperature_avg_c: PublicStationNumber::Number(12.0),
                wind_gust_high_kph: PublicStationNumber::Number(18.0),
                wind_speed_avg_kph: PublicStationNumber::Number(9.0),
            }),
            qc_status: WeatherUndergroundQcStatus::AdmittedNumber("1".to_owned()),
            solar_radiation_high_wm2: PublicStationNumber::Number(0.0),
            station_id: STATION_ID.to_owned(),
            uv_high: PublicStationNumber::Number(0.0),
            wind_direction_avg_degrees: PublicStationNumber::Number(180.0),
        }
    }
}
