//! Pure support gates for the retained wind training set.

use std::collections::HashSet;

use weather_contracts::{
    ForecastAdjustmentCoefficientSeasonEnum, ForecastAdjustmentMetric, ForecastLeadBandKey,
    ForecastObservationStation, ForecastObservationStationKey, Nullable,
};

use crate::{AdmittedWindStationTrainingRow, WindTrainingSet};

const METRIC_COUNT: usize = 5;
const LEAD_BAND_COUNT: usize = 7;
const PAIR_COUNT: usize = METRIC_COUNT * LEAD_BAND_COUNT;
const STATION_KEY_COUNT: usize = 11;

#[derive(Clone, Copy, Debug, Default)]
struct PairSupport {
    event_count: usize,
    observed: bool,
    station_counts: [usize; STATION_KEY_COUNT],
}

/// Return the exact retained evidence insufficiency labels in lexical order.
pub fn retained_wind_sufficiency_failures(
    training: &WindTrainingSet<'_>,
    stations: &[ForecastObservationStation],
    expected_dates: &[(&str, ForecastAdjustmentCoefficientSeasonEnum)],
) -> Vec<String> {
    let mut metric_counts = [0_usize; METRIC_COUNT];
    let mut pair_support = [PairSupport::default(); PAIR_COUNT];
    let mut network_dates = HashSet::new();
    let mut season_dates: [HashSet<&str>; 4] = std::array::from_fn(|_| HashSet::new());
    let mut admitted_station_keys = [false; STATION_KEY_COUNT];
    // retain only keys in the frozen station catalog
    for station in stations {
        admitted_station_keys[station_key_index(station.key)] = true;
    }

    // aggregate every literal support gate without rescanning the event set
    for event in &training.events {
        let metric_index = metric_index(event.metric);
        let pair_index = metric_index * LEAD_BAND_COUNT + lead_band_index(event.lead_band);
        metric_counts[metric_index] += 1;
        network_dates.insert(event.local_date);
        season_dates[season_index(event.calendar.season)].insert(event.local_date);
        let pair = &mut pair_support[pair_index];
        pair.observed = true;
        pair.event_count += 1;
        let mut contributed = [false; STATION_KEY_COUNT];
        // match the source's per-event some check for nonnull station metrics
        for row in training.station_rows_at(event.valid_at_ms) {
            let station_index = station_key_index(row.physical_station_key);
            // exclude rows outside the frozen station catalog supplied by the caller
            if admitted_station_keys[station_index] && station_metric_present(row, event.metric) {
                contributed[station_index] = true;
            }
        }
        // count one contribution per station and event rather than per physical row
        for (count, present) in pair.station_counts.iter_mut().zip(contributed) {
            // preserve the source's boolean event count
            if present {
                *count += 1;
            }
        }
    }

    let mut failed = Vec::new();
    // require broad network-date coverage
    if network_dates.len() < 330 {
        failed.push("network_330_local_dates".to_owned());
    }
    // require literal metric-wide support for every observed metric
    for (index, count) in metric_counts.into_iter().enumerate() {
        // omit entirely absent metrics exactly like the retained source
        if count > 0 && count < 6_000 {
            failed.push(format!(
                "metric_6000_events_{}",
                metric_reason(metric_for_index(index))
            ));
        }
    }
    // require event and physical-station support for every observed pair
    for (index, support) in pair_support.iter().enumerate() {
        // derive pairs only from those present in the event input
        if !support.observed {
            continue;
        }
        let metric = metric_for_index(index / LEAD_BAND_COUNT);
        let lead_band = lead_band_for_index(index % LEAD_BAND_COUNT);
        let reason = pair_reason(metric, lead_band);
        // require literal metric-band support
        if support.event_count < 500 {
            failed.push(format!("metric_band_500_events_{reason}"));
        }
        // require five physically distinct long-running stations
        if support
            .station_counts
            .iter()
            .filter(|count| **count >= 1_000)
            .count()
            < 5
        {
            failed.push(format!("five_stations_1000_events_{reason}"));
        }
    }

    let mut expected_season_dates: [HashSet<&str>; 4] = std::array::from_fn(|_| HashSet::new());
    // preserve Set semantics when an admitted date is repeated
    for (date, season) in expected_dates {
        expected_season_dates[season_index(*season)].insert(*date);
    }
    // preserve the source's winter, spring, summer, autumn traversal
    for season in [
        ForecastAdjustmentCoefficientSeasonEnum::Winter,
        ForecastAdjustmentCoefficientSeasonEnum::Spring,
        ForecastAdjustmentCoefficientSeasonEnum::Summer,
        ForecastAdjustmentCoefficientSeasonEnum::Autumn,
    ] {
        let index = season_index(season);
        // require seasonal support only when the epoch contains sixty dates
        if expected_season_dates[index].len() >= 60 && season_dates[index].len() < 60 {
            failed.push(format!("season_60_dates_{}", season_reason(season)));
        }
    }
    failed.sort_unstable();
    failed.dedup();
    failed
}

/// Match the source's nullable metric lookup and calm-direction exclusion.
fn station_metric_present(
    row: &AdmittedWindStationTrainingRow<'_>,
    metric: ForecastAdjustmentMetric,
) -> bool {
    let present = match metric {
        ForecastAdjustmentMetric::RelativeHumidityPercent => {
            matches!(&row.metrics.relativeHumidityPercent, Nullable::Value(_))
        }
        ForecastAdjustmentMetric::TemperatureC => {
            matches!(&row.metrics.temperatureC, Nullable::Value(_))
        }
        ForecastAdjustmentMetric::WindDirectionDegrees => {
            matches!(&row.metrics.windDirectionDegrees, Nullable::Value(_))
        }
        ForecastAdjustmentMetric::WindGustMps => {
            matches!(&row.metrics.windGustMps, Nullable::Value(_))
        }
        ForecastAdjustmentMetric::WindSpeedMps => {
            matches!(&row.metrics.windSpeedMps, Nullable::Value(_))
        }
    };
    // exclude calm station direction evidence after the direction value exists
    if metric == ForecastAdjustmentMetric::WindDirectionDegrees {
        present && matches!(&row.metrics.windSpeedMps, Nullable::Value(speed) if *speed >= 1.0)
    } else {
        present
    }
}

/// Map one generated metric to its source-order aggregation slot.
const fn metric_index(metric: ForecastAdjustmentMetric) -> usize {
    match metric {
        ForecastAdjustmentMetric::RelativeHumidityPercent => 0,
        ForecastAdjustmentMetric::TemperatureC => 1,
        ForecastAdjustmentMetric::WindDirectionDegrees => 2,
        ForecastAdjustmentMetric::WindGustMps => 3,
        ForecastAdjustmentMetric::WindSpeedMps => 4,
    }
}

/// Restore one metric from its frozen source order.
const fn metric_for_index(index: usize) -> ForecastAdjustmentMetric {
    match index {
        0 => ForecastAdjustmentMetric::RelativeHumidityPercent,
        1 => ForecastAdjustmentMetric::TemperatureC,
        2 => ForecastAdjustmentMetric::WindDirectionDegrees,
        3 => ForecastAdjustmentMetric::WindGustMps,
        4 => ForecastAdjustmentMetric::WindSpeedMps,
        _ => unreachable!(),
    }
}

/// Map one generated lead band to its fixed pair slot.
const fn lead_band_index(lead_band: ForecastLeadBandKey) -> usize {
    match lead_band {
        ForecastLeadBandKey::Value001024 => 0,
        ForecastLeadBandKey::Value025048 => 1,
        ForecastLeadBandKey::Value049072 => 2,
        ForecastLeadBandKey::Value073096 => 3,
        ForecastLeadBandKey::Value097120 => 4,
        ForecastLeadBandKey::Value121144 => 5,
        ForecastLeadBandKey::Value145168 => 6,
    }
}

/// Restore one lead band from its frozen source order.
const fn lead_band_for_index(index: usize) -> ForecastLeadBandKey {
    match index {
        0 => ForecastLeadBandKey::Value001024,
        1 => ForecastLeadBandKey::Value025048,
        2 => ForecastLeadBandKey::Value049072,
        3 => ForecastLeadBandKey::Value073096,
        4 => ForecastLeadBandKey::Value097120,
        5 => ForecastLeadBandKey::Value121144,
        6 => ForecastLeadBandKey::Value145168,
        _ => unreachable!(),
    }
}

/// Map the generated station key to the frozen manifest slot.
const fn station_key_index(key: ForecastObservationStationKey) -> usize {
    match key {
        ForecastObservationStationKey::AmbientMaxweather => 0,
        ForecastObservationStationKey::AmbientMerlin => 1,
        ForecastObservationStationKey::BallydideanEcowitt => 2,
        ForecastObservationStationKey::NetatmoNearby => 3,
        ForecastObservationStationKey::Tempest126537 => 4,
        ForecastObservationStationKey::Tempest168853 => 5,
        ForecastObservationStationKey::Tempest201058 => 6,
        ForecastObservationStationKey::Tempest203055 => 7,
        ForecastObservationStationKey::Tempest225947 => 8,
        ForecastObservationStationKey::Tempest38270 => 9,
        ForecastObservationStationKey::Tempest64255 => 10,
    }
}

/// Map one generated season to its counter slot.
const fn season_index(season: ForecastAdjustmentCoefficientSeasonEnum) -> usize {
    match season {
        ForecastAdjustmentCoefficientSeasonEnum::Winter => 0,
        ForecastAdjustmentCoefficientSeasonEnum::Spring => 1,
        ForecastAdjustmentCoefficientSeasonEnum::Summer => 2,
        ForecastAdjustmentCoefficientSeasonEnum::Autumn => 3,
    }
}

/// Preserve the source's exact snake-case metric label.
const fn metric_reason(metric: ForecastAdjustmentMetric) -> &'static str {
    match metric {
        ForecastAdjustmentMetric::RelativeHumidityPercent => "relative_humidity_percent",
        ForecastAdjustmentMetric::TemperatureC => "temperature_c",
        ForecastAdjustmentMetric::WindDirectionDegrees => "wind_direction_degrees",
        ForecastAdjustmentMetric::WindGustMps => "wind_gust_mps",
        ForecastAdjustmentMetric::WindSpeedMps => "wind_speed_mps",
    }
}

/// Preserve the source's exact snake-case lead-band suffix.
const fn lead_band_reason(lead_band: ForecastLeadBandKey) -> &'static str {
    match lead_band {
        ForecastLeadBandKey::Value001024 => "001_024",
        ForecastLeadBandKey::Value025048 => "025_048",
        ForecastLeadBandKey::Value049072 => "049_072",
        ForecastLeadBandKey::Value073096 => "073_096",
        ForecastLeadBandKey::Value097120 => "097_120",
        ForecastLeadBandKey::Value121144 => "121_144",
        ForecastLeadBandKey::Value145168 => "145_168",
    }
}

/// Join one metric-band reason without changing its source spelling.
fn pair_reason(metric: ForecastAdjustmentMetric, lead_band: ForecastLeadBandKey) -> String {
    format!("{}_{}", metric_reason(metric), lead_band_reason(lead_band))
}

/// Preserve the source's exact season label.
const fn season_reason(season: ForecastAdjustmentCoefficientSeasonEnum) -> &'static str {
    match season {
        ForecastAdjustmentCoefficientSeasonEnum::Autumn => "autumn",
        ForecastAdjustmentCoefficientSeasonEnum::Spring => "spring",
        ForecastAdjustmentCoefficientSeasonEnum::Summer => "summer",
        ForecastAdjustmentCoefficientSeasonEnum::Winter => "winter",
    }
}

#[cfg(test)]
mod tests {
    use weather_contracts::ForecastAdjustmentCoefficientDaypartEnum;

    use super::*;
    use crate::{AdmittedWindRowCalendar, WindTrainingEvent};

    /// Build one winter speed event at the given retained instant.
    fn event(valid_at_ms: i64) -> WindTrainingEvent<'static> {
        WindTrainingEvent {
            actual: 4.0,
            calendar: AdmittedWindRowCalendar {
                daypart: ForecastAdjustmentCoefficientDaypartEnum::Night,
                month: 1,
                season: ForecastAdjustmentCoefficientSeasonEnum::Winter,
            },
            lead_band: ForecastLeadBandKey::Value001024,
            local_date: "2025-01-01",
            metric: ForecastAdjustmentMetric::WindSpeedMps,
            raw_forecast: 3.0,
            reference_at: "2025-01-01T00:00:00.000Z",
            stable_id: "event".to_owned(),
            target_lead_hours: 1,
            valid_at: "2025-01-01T01:00:00.000Z",
            valid_at_ms,
        }
    }

    #[test]
    fn returns_exact_sorted_labels_for_one_observed_pair() {
        let mut training = WindTrainingSet::default();
        training.events.push(event(10));

        let failures = retained_wind_sufficiency_failures(&training, &[], &[]);

        assert_eq!(
            failures,
            [
                "five_stations_1000_events_wind_speed_mps_001_024",
                "metric_6000_events_wind_speed_mps",
                "metric_band_500_events_wind_speed_mps_001_024",
                "network_330_local_dates",
            ]
        );
    }
}
