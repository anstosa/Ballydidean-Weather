//! Compare the retained event builder to compiled Node's original selection and network target.

use std::path::PathBuf;
use std::process::Command;

use weather_adjustment::{
    AdmittedWindForecastTrainingRow, AdmittedWindRowCalendar, AdmittedWindStationTrainingRow,
    AdmittedWindTrainingRow, WindMaintenanceTrainingPreparation,
    build_retained_wind_training_events, prepare_wind_maintenance_training,
};
use weather_contracts::{
    ForecastAdjustmentCoefficientDaypartEnum as Daypart,
    ForecastAdjustmentCoefficientSeasonEnum as Season, ForecastAdjustmentMetric,
    ForecastLeadBandKey, ForecastObservationProviderFamily, ForecastObservationStation,
    ForecastObservationStationKey as StationKey, Nullable, SanitizedTrainingMetrics,
};

const MIDNIGHT: &str = "2026-03-08T00:00:00.000Z";
const HOUR: &str = "2026-03-08T01:00:00.000Z";
const LATER: &str = "2026-03-08T02:00:00.000Z";
const MIDNIGHT_MS: i64 = 1_772_928_000_000;
const HOUR_MS: i64 = MIDNIGHT_MS + 3_600_000;
const LATER_MS: i64 = HOUR_MS + 3_600_000;

/// Construct the generated original scalar metric shape.
fn metrics(speed: f64, gust: f64) -> SanitizedTrainingMetrics {
    SanitizedTrainingMetrics {
        relativeHumidityPercent: Nullable::Null,
        temperatureC: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: Nullable::Value(gust),
        windSpeedMps: Nullable::Value(speed),
    }
}

/// Supply one fixed original geometry without repairing timestamps in the fitter.
fn retrieval<'a>(
    content_sha256: &'a str,
    metrics: &'a SanitizedTrainingMetrics,
    valid: (&'a str, i64),
    reference: (&'a str, i64),
    lead: u16,
) -> AdmittedWindTrainingRow<'a> {
    AdmittedWindTrainingRow::Retrieval(AdmittedWindForecastTrainingRow {
        content_sha256,
        metrics,
        calendar: AdmittedWindRowCalendar {
            daypart: Daypart::Evening,
            month: 3,
            season: Season::Spring,
        },
        local_date: "2026-03-07",
        reference_at: reference.0,
        reference_at_ms: reference.1,
        target_lead_hours: lead,
        valid_at: valid.0,
        valid_at_ms: valid.1,
    })
}

/// Supply one configured physical station with its original-shaped eligibility.
fn station(key: StationKey, nearest_rank: u64, weight: f64) -> ForecastObservationStation {
    ForecastObservationStation {
        acceptedSourceKeys: Vec::new(),
        distanceMeters: 0.0,
        eligibleMetrics: vec![
            ForecastAdjustmentMetric::WindSpeedMps,
            ForecastAdjustmentMetric::WindGustMps,
        ],
        key,
        latitude: 48.0,
        longitude: -122.0,
        nearestRank: nearest_rank,
        providerFamily: ForecastObservationProviderFamily::Tempest,
        unnormalizedSpatialWeight: weight,
    }
}

/// Index an already admitted physical station hour.
fn observation(
    key: StationKey,
    valid_at_ms: i64,
    metrics: &SanitizedTrainingMetrics,
) -> AdmittedWindTrainingRow<'_> {
    AdmittedWindTrainingRow::Station(AdmittedWindStationTrainingRow {
        physical_station_key: key,
        valid_at_ms,
        metrics,
    })
}

/// Exact ties, missing coverage, disabled gust pair, and native generated metric values match.
#[test]
fn selected_wind_events_match_compiled_node() {
    let hashes = ["a", "b", "c", "d"].map(|digit| digit.repeat(64));
    let forecasts = [
        metrics(5.0, 10.0),
        metrics(6.0, 11.0),
        metrics(7.0, 12.0),
        metrics(8.0, 13.0),
        metrics(9.0, 14.0),
    ];
    let measured = [metrics(10.0, 18.0), metrics(4.0, 7.0), metrics(8.0, 15.0)];
    let stations = [
        station(StationKey::Tempest38270, 4, 1.0),
        station(StationKey::Tempest126537, 1, 2.0),
        station(StationKey::Tempest168853, 3, 1.0),
    ];
    let rows = [
        AdmittedWindTrainingRow::Other,
        retrieval(
            &hashes[0],
            &forecasts[0],
            (HOUR, HOUR_MS),
            ("2026-03-08T00:01:00.000Z", MIDNIGHT_MS + 60_000),
            1,
        ),
        retrieval(
            &hashes[1],
            &forecasts[1],
            (HOUR, HOUR_MS),
            (MIDNIGHT, MIDNIGHT_MS),
            1,
        ),
        retrieval(
            &hashes[0],
            &forecasts[2],
            (HOUR, HOUR_MS),
            (MIDNIGHT, MIDNIGHT_MS),
            1,
        ),
        retrieval(
            &hashes[2],
            &forecasts[3],
            (HOUR, HOUR_MS),
            ("2026-03-05T23:59:00.000Z", 1_772_755_140_000),
            50,
        ),
        retrieval(
            &hashes[3],
            &forecasts[4],
            (LATER, LATER_MS),
            (HOUR, HOUR_MS),
            1,
        ),
        observation(StationKey::Tempest38270, HOUR_MS, &measured[0]),
        observation(StationKey::Tempest126537, HOUR_MS, &measured[1]),
        observation(StationKey::Tempest168853, HOUR_MS, &measured[2]),
        observation(StationKey::Tempest38270, LATER_MS, &measured[0]),
        observation(StationKey::Tempest126537, LATER_MS, &measured[1]),
    ];

    let result =
        build_retained_wind_training_events(&rows, &stations).expect("wind event fit inputs");
    assert_eq!(result.events.len(), 3);
    let actual = result
        .events
        .iter()
        .map(|event| {
            let metric = ForecastAdjustmentMetric::WIRE_VALUES
                .iter()
                .find_map(|(name, field)| (*field == event.metric).then_some(*name))
                .expect("metric");
            let band = ForecastLeadBandKey::WIRE_VALUES
                .iter()
                .find_map(|(name, field)| (*field == event.lead_band).then_some(*name))
                .expect("band");
            format!(
                "{}\t{metric}\t{band}\t{}\t{}\t{}\t{}\n",
                event.valid_at,
                event.stable_id,
                event.actual,
                event.raw_forecast,
                event.reference_at
            )
        })
        .collect::<String>();
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/wind_training_events_node_oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("compiled Node wind event oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        actual,
        String::from_utf8(output.stdout).expect("oracle stdout")
    );

    // refuse an undercovered training phase before admitting numerical fitting
    let WindMaintenanceTrainingPreparation::Insufficient { failed_gates } =
        prepare_wind_maintenance_training(&rows, &stations, &[]).expect("support")
    else {
        panic!("fixture should not qualify as a training phase");
    };
    assert_eq!(
        failed_gates,
        [
            "five_stations_1000_events_wind_gust_mps_001_024",
            "five_stations_1000_events_wind_speed_mps_001_024",
            "five_stations_1000_events_wind_speed_mps_049_072",
            "metric_6000_events_wind_gust_mps",
            "metric_6000_events_wind_speed_mps",
            "metric_band_500_events_wind_gust_mps_001_024",
            "metric_band_500_events_wind_speed_mps_001_024",
            "metric_band_500_events_wind_speed_mps_049_072",
            "network_330_local_dates",
        ],
    );
}
