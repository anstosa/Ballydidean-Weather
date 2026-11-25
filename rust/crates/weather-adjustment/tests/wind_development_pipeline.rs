//! Drive the original chronological fold geometry through the admitted native wind pipeline.

use std::{convert::Infallible, path::Path, process::Command};

use weather_adjustment::{
    AdmittedWindDevelopmentDate, AdmittedWindForecastTrainingRow, AdmittedWindRowCalendar,
    AdmittedWindStationTrainingRow, AdmittedWindTrainingRow, WindDevelopmentQualification,
    WindMaintenanceTrainingPreparation, prepare_wind_maintenance_training,
    qualify_wind_maintenance_development,
};
use weather_contracts::{
    DevelopmentLosoFoldResultV1FoldEnum as Fold,
    ForecastAdjustmentCoefficientDaypartEnum as Daypart,
    ForecastAdjustmentCoefficientSeasonEnum as Season, ForecastAdjustmentMetric as Metric,
    ForecastObservationProviderFamily as Family, ForecastObservationStation,
    ForecastObservationStationKey as StationKey, Nullable, SanitizedTrainingMetrics,
};

const EVENTS_PER_DATE: usize = 17;
const HOUR_MS: i64 = 3_600_000;
const CONTENT_SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// Generate consecutive D0–D364 labels and exact features using the existing compiled calendar.
fn original_calendar_rows() -> String {
    let output = Command::new("node")
        .args([
            "--input-type=module",
            "-e",
            r#"
              import { createQualificationCalendarEpoch, localCalendarFeaturesFor } from
                "./packages/forecast-adjustment/dist/index.js";
              const epoch = createQualificationCalendarEpoch("2025-12-31");
              for (const date of epoch.finalTraining.localDates) {
                const start = Date.parse(date + "T09:00:00.000Z");
                for (let offset = 0; offset < 17; offset++) {
                  const ms = start + offset * 3_600_000;
                  const valid = new Date(ms).toISOString();
                  const reference = new Date(ms - 3_600_000).toISOString();
                  const {localDate, daypart, month, season} = localCalendarFeaturesFor(valid);
                  if (date !== localDate) throw new Error("test fixture escaped local day");
                  console.log([date, valid, reference, ms, daypart, month, season].join("\t"));
                }
              }
            "#,
        ])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."))
        .output()
        .expect("compiled calendar runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("calendar fixture is UTF-8")
}

/// Supply only the maintained speed metric; the original pipeline skips absent gusts.
fn speed_only(speed: f64) -> SanitizedTrainingMetrics {
    SanitizedTrainingMetrics {
        relativeHumidityPercent: Nullable::Null,
        temperatureC: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: Nullable::Null,
        windSpeedMps: Nullable::Value(speed),
    }
}

/// Construct admitted fixture station geometry with a near station after any one exclusion.
fn fixture_station(
    key: StationKey,
    provider_family: Family,
    rank: u64,
) -> ForecastObservationStation {
    ForecastObservationStation {
        acceptedSourceKeys: Vec::new(),
        distanceMeters: 0.0,
        eligibleMetrics: vec![Metric::WindSpeedMps],
        key,
        latitude: 48.0,
        longitude: -122.0,
        nearestRank: rank,
        providerFamily: provider_family,
        unnormalizedSpatialWeight: 1.0,
    }
}

/// Require real support, exclude each station independently, and refit only after five folds.
#[test]
fn supported_final_training_runs_all_five_wind_folds() {
    let calendar = original_calendar_rows();
    let raw = speed_only(2.0);
    let station_values = [6.0, 7.0, 8.0, 9.0, 10.0].map(speed_only);
    let stations = [
        fixture_station(StationKey::AmbientMaxweather, Family::Ambient, 3),
        fixture_station(StationKey::AmbientMerlin, Family::Ambient, 2),
        fixture_station(StationKey::BallydideanEcowitt, Family::Ecowitt, 1),
        fixture_station(StationKey::NetatmoNearby, Family::Netatmo, 4),
        fixture_station(StationKey::Tempest126537, Family::Tempest, 5),
    ];
    let mut expected_dates = Vec::new();
    let mut admitted_dates = Vec::new();
    let mut rows = Vec::new();
    // retain each date's exact 17 immutable hours and five physical observations
    for (index, line) in calendar.lines().enumerate() {
        let fields = line.split('\t').collect::<Vec<_>>();
        let [date, valid, reference, ms, daypart, month, season] = fields.as_slice() else {
            panic!("unexpected compiled calendar projection");
        };
        let daypart = Daypart::WIRE_VALUES
            .iter()
            .find_map(|(name, value)| (name == daypart).then_some(*value))
            .expect("known daypart");
        let season = Season::WIRE_VALUES
            .iter()
            .find_map(|(name, value)| (name == season).then_some(*value))
            .expect("known season");
        let valid_at_ms = ms.parse::<i64>().expect("exact fixture instant");
        // capture each local date before its first event
        if index % EVENTS_PER_DATE == 0 {
            expected_dates.push((*date, season));
            admitted_dates.push(AdmittedWindDevelopmentDate {
                label: date,
                ordinal: (index / EVENTS_PER_DATE) as i64,
            });
        }
        rows.push(AdmittedWindTrainingRow::Retrieval(
            AdmittedWindForecastTrainingRow {
                content_sha256: CONTENT_SHA256,
                metrics: &raw,
                calendar: AdmittedWindRowCalendar {
                    daypart,
                    month: month.parse().expect("exact fixture month"),
                    season,
                },
                local_date: date,
                reference_at: reference,
                reference_at_ms: valid_at_ms - HOUR_MS,
                target_lead_hours: 1,
                valid_at: valid,
                valid_at_ms,
            },
        ));
        // preserve origin order and exact values for station-excluded weighted medians
        for (station, metrics) in stations.iter().zip(&station_values) {
            rows.push(AdmittedWindTrainingRow::Station(
                AdmittedWindStationTrainingRow {
                    physical_station_key: station.key,
                    valid_at_ms,
                    metrics,
                },
            ));
        }
    }
    let WindMaintenanceTrainingPreparation::Ready(training) =
        prepare_wind_maintenance_training(&rows, &stations, &expected_dates)
            .expect("train admitted")
    else {
        panic!("original support gates must admit this fixture");
    };
    let dates = admitted_dates
        .try_into()
        .expect("exact final-training window");
    let mut observed = 0usize;
    let result = qualify_wind_maintenance_development(&training, &stations, &dates, |model| {
        let fold = match model.fold {
            Fold::Value1 => 0,
            Fold::Value2 => 1,
            Fold::Value3 => 2,
            Fold::Value4 => 3,
            Fold::Value5 => 4,
        };
        let station_index = stations
            .iter()
            .position(|station| station.key == model.physical_station_key)
            .expect("original station roster order");
        assert_eq!(observed, fold * stations.len() + station_index);
        assert_eq!(model.metric_band.metric, Metric::WindSpeedMps);
        // each omitted station changes the exact midpoint of the four retained values
        let expected_network = [8.5, 8.5, 8.0, 7.5, 7.5][station_index];
        let count = ((180 + 37 * fold) * EVENTS_PER_DATE) as f64;
        let coefficient = &model.coefficients[0];
        assert_eq!(coefficient.effectiveEventCount, count);
        assert_eq!(
            coefficient.coefficient,
            (count / (count + 200.0)) * (expected_network - 2.0)
        );
        observed += 1;
        // the injected fixture digest checks exact typed material rather than claiming SHA-256
        Ok::<_, Infallible>(format!("{observed:064x}"))
    })
    .expect("score and qualify all five folds");
    assert_eq!(observed, 25);
    let WindDevelopmentQualification::Qualified(pairs) = result else {
        panic!("the improving fixture must qualify");
    };
    assert_eq!(pairs.len(), 1);
    let pair = &pairs[0];
    assert_eq!(pair.pair.metric, Metric::WindSpeedMps);
    assert_eq!(pair.envelope.minimum, 2.0);
    assert_eq!(pair.envelope.maximum, 2.0);
    assert_eq!(pair.coefficients[0].effectiveEventCount, 6_205.0);
    assert!(pair.folds.iter().all(|fold| {
        fold.passed
            && fold.providerFamilies.len() == 4
            && fold.scoreableStationKeys.len() == 5
            && fold.bootstrapLowerBound > 0.5
    }));
}
