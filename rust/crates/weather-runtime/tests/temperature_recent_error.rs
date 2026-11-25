use std::cell::RefCell;
use std::rc::Rc;

use weather_contracts::{
    ForecastObservationHourlyMetrics, ForecastObservationHourlyStationRow,
    ForecastObservationProviderFamily, ForecastObservationStationKey, Nullable,
};
use weather_database::{
    CAUSAL_ECMWF_TEMPERATURE_PRIOR_HOURS_SQL, CAUSAL_FORECAST_OBSERVATION_HOURLY_STATIONS_SQL,
    CAUSAL_FORECAST_OBSERVATION_SOURCE_KEYS, CausalEcmwfPriorHourDbRow,
    CausalEcmwfPriorHoursReadPlan, CausalForecastObservationReadPlan, CausalTemperatureReadBind,
};
use weather_runtime::{
    AdmittedCausalEcmwfPriorHourReadRow, AdmittedCausalTemperatureStationReadRow,
    CausalTemperatureCalendarPort, CausalTemperatureDatabasePort,
    DatabaseTemperatureRecentErrorBuilder, build_causal_temperature_recent_error_state,
};
use weather_worker::{
    OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION, OPEN_METEO_ECMWF_SINGLE_RUN_MODEL, TaskFailure,
    TemperatureCanaryBatch, TemperatureCanaryExecution, TemperatureCanaryFetchControls,
    TemperatureCanaryFetchLimits, TemperatureCanaryHour, TemperatureCanaryPersistedRun,
    TemperatureCanaryProvider, TemperatureCanaryRepository, TemperatureCanaryRequest,
    TemperatureCanarySource, TemperatureCanaryTimeAdapter, collect_ecmwf_temperature_canary_runs,
};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const NOW_MS: i64 = 1_788_827_700_000;
const TARGET_MS: i64 = 1_788_804_000_000;
const WINDOW_END_MS: i64 = TARGET_MS - 7 * HOUR_MILLISECONDS;
const WINDOW_START_MS: i64 = WINDOW_END_MS - 71 * HOUR_MILLISECONDS;

#[derive(Default)]
struct FakePostgres {
    fail_prior_read: bool,
    prior_rows: Vec<AdmittedCausalEcmwfPriorHourReadRow>,
    station_rows: Vec<AdmittedCausalTemperatureStationReadRow>,
    trace: Vec<&'static str>,
}

impl CausalTemperatureDatabasePort for FakePostgres {
    /// Execute and verify the source-owned prior-hour statement and bind order.
    fn read_causal_ecmwf_prior_hours(
        &mut self,
        plan: &CausalEcmwfPriorHoursReadPlan,
    ) -> Result<Vec<AdmittedCausalEcmwfPriorHourReadRow>, TaskFailure> {
        assert_eq!(plan.sql, CAUSAL_ECMWF_TEMPERATURE_PRIOR_HOURS_SQL);
        assert_eq!(
            plan.binds,
            [
                CausalTemperatureReadBind::Text("ballydidean".to_owned()),
                CausalTemperatureReadBind::UtcInstant("2026-09-04T12:00:00.000Z".to_owned()),
                CausalTemperatureReadBind::UtcInstant("2026-09-07T11:00:00.000Z".to_owned()),
                CausalTemperatureReadBind::UtcInstant("2026-09-07T18:00:00.000Z".to_owned()),
            ]
        );
        self.trace.push("prior");
        // preserve an unavailable read as a real collector failure
        if self.fail_prior_read {
            return Err(failure("prior rows unavailable"));
        }
        Ok(std::mem::take(&mut self.prior_rows))
    }

    /// Execute and verify the source-owned station statement and bind order.
    fn read_causal_temperature_station_hours(
        &mut self,
        plan: &CausalForecastObservationReadPlan,
    ) -> Result<Vec<AdmittedCausalTemperatureStationReadRow>, TaskFailure> {
        assert_eq!(plan.sql, CAUSAL_FORECAST_OBSERVATION_HOURLY_STATIONS_SQL);
        assert_eq!(
            plan.binds,
            [
                CausalTemperatureReadBind::Text("ballydidean".to_owned()),
                CausalTemperatureReadBind::UtcInstant("2026-09-04T12:00:00.000Z".to_owned()),
                CausalTemperatureReadBind::UtcInstant("2026-09-07T12:00:00.000Z".to_owned()),
                CausalTemperatureReadBind::UtcInstant("2026-09-07T18:00:00.000Z".to_owned()),
                CausalTemperatureReadBind::TextArray(
                    CAUSAL_FORECAST_OBSERVATION_SOURCE_KEYS
                        .iter()
                        .map(|key| (*key).to_owned())
                        .collect(),
                ),
            ]
        );
        self.trace.push("stations");
        Ok(std::mem::take(&mut self.station_rows))
    }
}

struct FixtureCalendar;

impl CausalTemperatureCalendarPort for FixtureCalendar {
    /// Resolve every admitted window clock without implementing production date parsing.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        fixture_utc(epoch_ms).ok_or_else(|| failure("unexpected causal clock"))
    }

    /// Return the deterministic source-local fixture date already resolved by the time owner.
    fn local_date(&mut self, valid_at: &str, epoch_ms: i64) -> Result<String, TaskFailure> {
        let expected = fixture_utc(epoch_ms).ok_or_else(|| failure("unexpected local clock"))?;
        // bind the local calendar result to the same canonical instant
        if expected != valid_at {
            return Err(failure("local date instant differs"));
        }
        Ok(expected[..10].to_owned())
    }
}

struct CollectorTime;

impl TemperatureCanaryTimeAdapter for CollectorTime {
    /// Retain exact run identities and opaque provider-hour strings in the collector fixture.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        Ok(match epoch_ms {
            TARGET_MS => "2026-09-07T18:00:00.000Z".to_owned(),
            value if value == TARGET_MS - 6 * HOUR_MILLISECONDS => {
                "2026-09-07T12:00:00.000Z".to_owned()
            }
            value if value == TARGET_MS - 72 * HOUR_MILLISECONDS => {
                "2026-09-04T18:00:00.000Z".to_owned()
            }
            _ => format!("provider-{epoch_ms}"),
        })
    }
}

struct FakeProvider;

impl TemperatureCanaryProvider for FakeProvider {
    /// Return one complete normalized eighteen-hour batch for either requested run.
    fn fetch_single_run(
        &mut self,
        request: &TemperatureCanaryRequest,
        controls: TemperatureCanaryFetchControls,
    ) -> Result<TemperatureCanaryBatch, TaskFailure> {
        assert_eq!(controls.max_attempts, 1);
        assert_eq!(controls.max_body_bytes, 512_000);
        assert_eq!(controls.timeout_ms, 5_000);
        let initialized_at_ms = if request.run_initialized_at == "2026-09-07T18:00:00.000Z" {
            TARGET_MS
        } else {
            TARGET_MS - 6 * HOUR_MILLISECONDS
        };
        let mut time = CollectorTime;
        Ok(TemperatureCanaryBatch {
            adapter_version: OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION.to_owned(),
            attempts: 1,
            hours: (1..=18)
                .map(|lead| {
                    let valid_at_ms = initialized_at_ms + i64::from(lead) * HOUR_MILLISECONDS;
                    TemperatureCanaryHour {
                        model_lead_hours: lead,
                        raw_relative_humidity_percent: Some(75.0),
                        raw_temperature_c: 10.0,
                        raw_wind_speed_mps: Some(3.0),
                        valid_at: time
                            .utc_string(valid_at_ms)
                            .expect("provider fixture clock must resolve"),
                        valid_at_ms,
                    }
                })
                .collect(),
            model_cycle: "50r1".to_owned(),
            provider_response_sha256: "a".repeat(64),
            received_at: "2026-09-08T00:35:00.000Z".to_owned(),
            run_initialized_at: request.run_initialized_at.clone(),
            upstream_model: OPEN_METEO_ECMWF_SINGLE_RUN_MODEL.to_owned(),
        })
    }
}

struct FakeRepository {
    persisted: Rc<RefCell<Vec<TemperatureCanaryPersistedRun>>>,
}

impl TemperatureCanaryRepository for FakeRepository {
    /// Return an empty warmup inventory so latest and one older run are selected.
    fn list_run_initializations(
        &mut self,
        _site_slug: &str,
        _from: &str,
        _to: &str,
    ) -> Result<Vec<String>, TaskFailure> {
        Ok(Vec::new())
    }

    /// Retain the exact run and state handed to the existing atomic persistence seam.
    fn persist_run(&mut self, run: &TemperatureCanaryPersistedRun) -> Result<(), TaskFailure> {
        self.persisted.borrow_mut().push(run.clone());
        Ok(())
    }
}

/// Execute actual read plans and reducer only for latest, then persist older as cold.
#[test]
fn collector_reduces_latest_causal_rows_and_keeps_older_run_cold() {
    let persisted = Rc::new(RefCell::new(Vec::new()));
    let mut repository = FakeRepository {
        persisted: Rc::clone(&persisted),
    };
    let mut provider = FakeProvider;
    let mut recent_error =
        DatabaseTemperatureRecentErrorBuilder::new(postgres_fixture(false), FixtureCalendar);
    let mut time = CollectorTime;
    let collection = collect_ecmwf_temperature_canary_runs(
        NOW_MS,
        &mut TemperatureCanaryExecution {
            fetch_limits: TemperatureCanaryFetchLimits::default(),
            provider: &mut provider,
            recent_error: &mut recent_error,
            repository: &mut repository,
            source: &source(),
            time: &mut time,
        },
    )
    .expect("collector must isolate and persist both admitted runs");
    assert_eq!(collection.persisted_runs, 2);
    assert_eq!(collection.failed_runs, 0);
    assert_eq!(recent_error.database().trace, ["prior", "stations"]);
    let runs = persisted.borrow();
    assert_eq!(runs.len(), 2);
    assert!(runs[0].recent_error_state.supported);
    assert_eq!(runs[0].recent_error_state.b24_c, Some(2.0));
    assert_eq!(runs[0].recent_error_state.b72_c, Some(2.0));
    assert_eq!(runs[0].recent_error_state.n24, 24);
    assert_eq!(runs[0].recent_error_state.n72, 30);
    assert_eq!(runs[0].recent_error_state.local_dates, 2);
    assert_eq!(runs[0].recent_error_state.source_keys.len(), 30);
    assert!(!runs[1].recent_error_state.supported);
    assert_eq!(runs[1].recent_error_state.n72, 0);
    assert_eq!(runs[1].state_status.code(), "cold");
}

/// Propagate an absent causal read only for latest while still persisting older cold state.
#[test]
fn collector_isolates_latest_read_failure_and_continues_older_cold_run() {
    let persisted = Rc::new(RefCell::new(Vec::new()));
    let mut repository = FakeRepository {
        persisted: Rc::clone(&persisted),
    };
    let mut provider = FakeProvider;
    let mut recent_error =
        DatabaseTemperatureRecentErrorBuilder::new(postgres_fixture(true), FixtureCalendar);
    let mut time = CollectorTime;
    let collection = collect_ecmwf_temperature_canary_runs(
        NOW_MS,
        &mut TemperatureCanaryExecution {
            fetch_limits: TemperatureCanaryFetchLimits::default(),
            provider: &mut provider,
            recent_error: &mut recent_error,
            repository: &mut repository,
            source: &source(),
            time: &mut time,
        },
    )
    .expect("collector must continue after one causal read failure");
    assert_eq!(collection.failed_runs, 1);
    assert_eq!(collection.persisted_runs, 1);
    assert_eq!(recent_error.database().trace, ["prior"]);
    assert_eq!(persisted.borrow()[0].state_status.code(), "cold");
}

/// Preserve a truthful cold state when both causal queries return no matched evidence.
#[test]
fn empty_database_evidence_reduces_to_cold_without_invented_values() {
    let mut database = FakePostgres::default();
    let state = build_direct(&mut database).expect("empty causal evidence must remain valid");
    assert_eq!(database.trace, ["prior", "stations"]);
    assert!(!state.supported);
    assert_eq!(state.n24, 0);
    assert_eq!(state.n72, 0);
    assert!(state.b24_c.is_none());
    assert!(state.b72_c.is_none());
    assert!(state.source_keys.is_empty());
}

/// Reject non-hourly targets, future source runs, and out-of-order selected forecasts.
#[test]
fn rejects_invalid_target_causality_and_database_order() {
    let mut calendar = FixtureCalendar;
    let mut target_database = postgres_fixture(false);
    assert!(
        build_causal_temperature_recent_error_state(
            &mut target_database,
            &mut calendar,
            "ballydidean",
            "2026-09-07T18:00:00.000Z",
            TARGET_MS + 1,
        )
        .is_err()
    );
    assert!(target_database.trace.is_empty());

    let mut future_database = postgres_fixture(false);
    future_database.prior_rows[0].run_initialized_at_epoch_ms = TARGET_MS;
    future_database.prior_rows[0].row.run_initialized_at = "2026-09-07T18:00:00.000Z".to_owned();
    assert!(build_direct(&mut future_database).is_err());

    let mut unordered_database = postgres_fixture(false);
    unordered_database.prior_rows.swap(0, 1);
    assert!(build_direct(&mut unordered_database).is_err());
}

// execute one direct latest-state build against a supplied fake PostgreSQL port
fn build_direct(
    database: &mut FakePostgres,
) -> Result<weather_worker::TemperatureCanaryRecentErrorState, TaskFailure> {
    build_causal_temperature_recent_error_state(
        database,
        &mut FixtureCalendar,
        "ballydidean",
        "2026-09-07T18:00:00.000Z",
        TARGET_MS,
    )
}

// build actual decoded database rows rather than a precomputed recent-error state
fn postgres_fixture(fail_prior_read: bool) -> FakePostgres {
    let mut prior_rows = Vec::new();
    let mut station_rows = Vec::new();

    // return the latest thirty selected hours in the SQL statement's ascending order
    for age_hours in (0..30).rev() {
        let valid_at_epoch_ms = WINDOW_END_MS - i64::from(age_hours) * HOUR_MILLISECONDS;
        let run_initialized_at_epoch_ms = valid_at_epoch_ms - 7 * HOUR_MILLISECONDS;
        let valid_at = fixture_utc(valid_at_epoch_ms).expect("fixture valid-at must resolve");
        let run_initialized_at =
            fixture_utc(run_initialized_at_epoch_ms).expect("fixture run must resolve");
        prior_rows.push(AdmittedCausalEcmwfPriorHourReadRow {
            row: CausalEcmwfPriorHourDbRow {
                content_hash: format!("{age_hours:064x}"),
                model_lead_hours: 7,
                raw_relative_humidity_percent: Nullable::Value(75.0),
                raw_temperature_c: 10.0,
                raw_wind_speed_mps: Nullable::Value(3.0),
                run_initialized_at,
                valid_at: valid_at.clone(),
            },
            run_initialized_at_epoch_ms,
            valid_at_epoch_ms,
        });
        // retain three real generated station identities for the network support gate
        for (key, provider, temperature_c) in [
            (
                ForecastObservationStationKey::AmbientMerlin,
                ForecastObservationProviderFamily::Ambient,
                11.0,
            ),
            (
                ForecastObservationStationKey::BallydideanEcowitt,
                ForecastObservationProviderFamily::Ecowitt,
                12.0,
            ),
            (
                ForecastObservationStationKey::Tempest64255,
                ForecastObservationProviderFamily::Tempest,
                13.0,
            ),
        ] {
            station_rows.push(AdmittedCausalTemperatureStationReadRow {
                row: ForecastObservationHourlyStationRow {
                    metrics: ForecastObservationHourlyMetrics {
                        relativeHumidityPercent: Nullable::Null,
                        temperatureC: Nullable::Value(temperature_c),
                        windDirectionDegrees: Nullable::Null,
                        windGustMps: Nullable::Null,
                        windSpeedMps: Nullable::Null,
                    },
                    physicalStationKey: key,
                    providerFamily: provider,
                    sourceKeys: vec!["fixture-source".to_owned()],
                    validAt: valid_at.clone(),
                },
                valid_at_epoch_ms,
            });
        }
    }
    FakePostgres {
        fail_prior_read,
        prior_rows,
        station_rows,
        trace: Vec::new(),
    }
}

// return the exact admitted private source identity used by the collector
fn source() -> TemperatureCanarySource {
    TemperatureCanarySource {
        active: true,
        adapter_version: OPEN_METEO_ECMWF_SINGLE_RUN_ADAPTER_VERSION.to_owned(),
        capability: "forecast".to_owned(),
        dataset: "single_run".to_owned(),
        latitude: 56.0,
        longitude: -3.0,
        provider_key: "open-meteo".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_kind: "forecast".to_owned(),
        upstream_model: OPEN_METEO_ECMWF_SINGLE_RUN_MODEL.to_owned(),
    }
}

// format the bounded September fixture clocks through an admitted deterministic table
fn fixture_utc(epoch_ms: i64) -> Option<String> {
    // retain the target beyond the state window for the causal as-of bind
    if epoch_ms == TARGET_MS {
        return Some("2026-09-07T18:00:00.000Z".to_owned());
    }
    let offset_hours = epoch_ms
        .checked_sub(WINDOW_START_MS)?
        .checked_div(HOUR_MILLISECONDS)?;
    // reject non-hourly and out-of-table fixture clocks
    if WINDOW_START_MS + offset_hours * HOUR_MILLISECONDS != epoch_ms
        || !(-13..=72).contains(&offset_hours)
    {
        return None;
    }
    let total_hours = 12 + offset_hours;
    let day = 4 + total_hours.div_euclid(24);
    let hour = total_hours.rem_euclid(24);
    Some(format!("2026-09-{day:02}T{hour:02}:00:00.000Z"))
}

// build one stable fake-port failure
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
