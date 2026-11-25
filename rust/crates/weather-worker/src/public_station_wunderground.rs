//! Typed Weather Underground archive normalization without transport or JSON parsing.

use std::collections::BTreeMap;

use super::supplemental_public_station::{
    PublicStationMetric, PublicStationNormalizationError, PublicStationNormalizedSample,
    PublicStationNumber, empty_public_station_metrics, public_station_average,
    public_station_direction, public_station_epoch_ms, public_station_metric,
    public_station_number, public_station_range,
};
use crate::{CanonicalWeatherMetrics, ScheduledWindow, TaskFailure};

const MAXIMUM_WUNDERGROUND_RANGE_MS: i64 = 24 * 60 * 60 * 1_000;

/// typed metric object decoded from one Weather Underground observation
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherUndergroundMetricObservation {
    pub precip_rate_mm_per_hour: PublicStationNumber,
    pub pressure_max_hpa: PublicStationNumber,
    pub pressure_min_hpa: PublicStationNumber,
    pub temperature_avg_c: PublicStationNumber,
    pub wind_gust_high_kph: PublicStationNumber,
    pub wind_speed_avg_kph: PublicStationNumber,
}

/// exact QC cell state retained by the approved response decoder
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WeatherUndergroundQcStatus {
    Missing,
    Invalid,
    AdmittedNumber(String),
}

/// one typed provider row before range filtering and canonical normalization
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherUndergroundObservation {
    pub epoch_seconds: i64,
    pub humidity_avg_percent: PublicStationNumber,
    /// Absent when the provider's metric field is not an object.
    pub metric: Option<WeatherUndergroundMetricObservation>,
    pub qc_status: WeatherUndergroundQcStatus,
    pub solar_radiation_high_wm2: PublicStationNumber,
    pub station_id: String,
    pub uv_high: PublicStationNumber,
    pub wind_direction_avg_degrees: PublicStationNumber,
}

/// normalize intersecting local-day payloads into sorted distinct canonical samples
pub fn normalize_weather_underground_rows(
    payloads: &[Vec<WeatherUndergroundObservation>],
    station_id: &str,
    window: ScheduledWindow,
) -> Result<Vec<PublicStationNormalizedSample>, TaskFailure> {
    normalize_weather_underground_rows_with_diagnostics(payloads, station_id, window)
        .map_err(PublicStationNormalizationError::into_task_failure)
}

/// normalize rows while retaining the original provider's bounded diagnosis
pub(super) fn normalize_weather_underground_rows_with_diagnostics(
    payloads: &[Vec<WeatherUndergroundObservation>],
    station_id: &str,
    window: ScheduledWindow,
) -> Result<Vec<PublicStationNormalizedSample>, PublicStationNormalizationError> {
    public_station_range(window, MAXIMUM_WUNDERGROUND_RANGE_MS)?;
    let mut distinct = BTreeMap::new();

    // flatten provider-local days while preserving first observations across payloads
    for payload in payloads {
        for (index, row) in payload.iter().enumerate() {
            let valid_at_ms = public_station_epoch_ms(row.epoch_seconds).map_err(|_| {
                PublicStationNormalizationError::invalid_payload(format!(
                    "observations[{index}].epoch must be a positive integer"
                ))
            })?;
            // discard local-day padding before inspecting station identity or metric cells
            if valid_at_ms < window.start_ms || valid_at_ms >= window.end_exclusive_ms {
                continue;
            }
            // reject an in-window row for another configured physical station
            if row.station_id != station_id {
                return Err(PublicStationNormalizationError::invalid_payload(
                    "Weather Underground station identity is invalid",
                ));
            }
            distinct.entry(valid_at_ms).or_insert(row);
        }
    }

    distinct
        .into_iter()
        .map(|(valid_at_ms, row)| normalize_row(valid_at_ms, row))
        .collect()
}

/// normalize one retained in-window row after duplicate selection
fn normalize_row(
    valid_at_ms: i64,
    row: &WeatherUndergroundObservation,
) -> Result<PublicStationNormalizedSample, PublicStationNormalizationError> {
    let metric = row.metric.as_ref().ok_or_else(|| {
        PublicStationNormalizationError::invalid_payload("observation.metric must be an object")
    })?;
    let pressure_max = public_station_number(metric.pressure_max_hpa, "pressureMax")?;
    let pressure_min = public_station_number(metric.pressure_min_hpa, "pressureMin")?;
    let pressure = public_station_average(pressure_max, pressure_min);
    // node validates QC after pressure cells but before validating metric bounds
    let quality_status = match &row.qc_status {
        WeatherUndergroundQcStatus::Missing => None,
        WeatherUndergroundQcStatus::Invalid => {
            return Err(PublicStationNormalizationError::invalid_payload(
                "qcStatus must be a finite number or null",
            ));
        }
        WeatherUndergroundQcStatus::AdmittedNumber(value) => Some(format!("provider_qc_{value}")),
    };
    let mut metrics: CanonicalWeatherMetrics = empty_public_station_metrics();
    metrics.precipitation_rate_mm_per_hour = public_station_metric(
        PublicStationMetric::PrecipitationRateMmPerHour,
        public_station_number(metric.precip_rate_mm_per_hour, "precipitationRateMmPerHour")?,
    )?;
    metrics.pressure_hpa = public_station_metric(PublicStationMetric::PressureHpa, pressure)?;
    metrics.relative_humidity_percent = public_station_metric(
        PublicStationMetric::HumidityPercent,
        public_station_number(row.humidity_avg_percent, "relativeHumidityPercent")?,
    )?;
    metrics.solar_radiation_wm2 = public_station_metric(
        PublicStationMetric::SolarRadiationWm2,
        public_station_number(row.solar_radiation_high_wm2, "solarRadiationWm2")?,
    )?;
    metrics.temperature_c = public_station_metric(
        PublicStationMetric::Celsius,
        public_station_number(metric.temperature_avg_c, "temperatureC")?,
    )?;
    let uv_index = public_station_number(row.uv_high, "uv index")?;
    // discard impossible UV readings rather than rejecting the row
    metrics.uv_index = if uv_index.is_some_and(|value| !(0.0..=20.0).contains(&value)) {
        None
    } else {
        public_station_metric(PublicStationMetric::UvIndex, uv_index)?
    };
    metrics.wind_direction_degrees = public_station_direction(public_station_number(
        row.wind_direction_avg_degrees,
        "wind direction",
    )?)?;
    metrics.wind_gust_mps = public_station_metric(
        PublicStationMetric::WindGustKph,
        public_station_number(metric.wind_gust_high_kph, "windGustMps")?,
    )?;
    metrics.wind_speed_mps = public_station_metric(
        PublicStationMetric::WindSpeedKph,
        public_station_number(metric.wind_speed_avg_kph, "windSpeedMps")?,
    )?;

    Ok(PublicStationNormalizedSample {
        valid_at_ms,
        metrics,
        quality_status,
        quality_flags: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const START_MS: i64 = 1_800_000_000_000;
    const STATION_ID: &str = "KCASANFR123";

    /// preserve range filtering, first-duplicate selection, sorting, units, and QC text
    #[test]
    fn normalizes_sorted_distinct_rows_with_metric_units() {
        let first_epoch_seconds = START_MS / 1_000 + 600;
        let second_epoch_seconds = START_MS / 1_000 + 1_200;
        let payloads = vec![
            vec![
                WeatherUndergroundObservation {
                    uv_high: PublicStationNumber::Number(21.0),
                    ..observation(second_epoch_seconds, PublicStationNumber::Number(14.0))
                },
                observation(first_epoch_seconds, PublicStationNumber::Number(12.0)),
            ],
            vec![
                // the duplicate's invalid temperature is lazy because the first row wins
                observation(first_epoch_seconds, PublicStationNumber::Invalid),
                WeatherUndergroundObservation {
                    epoch_seconds: START_MS / 1_000 - 1,
                    station_id: "OTHER".to_owned(),
                    metric: None,
                    ..observation(first_epoch_seconds, PublicStationNumber::Missing)
                },
            ],
        ];

        let normalized = normalize_weather_underground_rows(
            &payloads,
            STATION_ID,
            ScheduledWindow {
                start_ms: START_MS,
                end_exclusive_ms: START_MS + 3_600_000,
            },
        )
        .expect("typed Weather Underground rows should normalize");

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].valid_at_ms, first_epoch_seconds * 1_000);
        assert_eq!(normalized[0].metrics.temperature_c, Some(12.0));
        assert_eq!(normalized[0].metrics.pressure_hpa, Some(1_000.0));
        assert_eq!(
            normalized[0].metrics.precipitation_rate_mm_per_hour,
            Some(1.5)
        );
        assert_eq!(normalized[0].metrics.relative_humidity_percent, Some(50.0));
        assert_eq!(normalized[0].metrics.solar_radiation_wm2, Some(120.0));
        assert_eq!(normalized[0].metrics.uv_index, Some(3.0));
        assert_eq!(normalized[0].metrics.wind_direction_degrees, Some(0.0));
        assert_eq!(normalized[0].metrics.wind_gust_mps, Some(10.0));
        assert_eq!(normalized[0].metrics.wind_speed_mps, Some(5.0));
        assert_eq!(
            normalized[0].quality_status.as_deref(),
            Some("provider_qc_1")
        );
        assert!(normalized[0].quality_flags.is_empty());
        assert_eq!(normalized[1].valid_at_ms, second_epoch_seconds * 1_000);
        assert_eq!(normalized[1].metrics.uv_index, None);
    }

    /// preserve the first error when QC and a later metric bound are invalid
    #[test]
    fn validates_qc_before_metric_bounds() {
        let row = WeatherUndergroundObservation {
            qc_status: WeatherUndergroundQcStatus::Invalid,
            ..observation(START_MS / 1_000, PublicStationNumber::Number(500.0))
        };
        let window = ScheduledWindow {
            start_ms: START_MS,
            end_exclusive_ms: START_MS + 60_000,
        };
        let diagnosis = normalize_weather_underground_rows_with_diagnostics(
            &[vec![row.clone()]],
            STATION_ID,
            window,
        )
        .expect_err("invalid QC must keep the first Node diagnosis");
        assert_eq!(
            diagnosis.message(),
            "qcStatus must be a finite number or null"
        );
        let legacy = normalize_weather_underground_rows(&[vec![row]], STATION_ID, window)
            .expect_err("scheduled normalization must still fail");
        assert_eq!(
            (legacy.attempts, legacy.code.as_str()),
            (0, "invalid_payload")
        );
    }

    /// reject station substitution after range filtering but before lazy metric decoding
    #[test]
    fn rejects_in_window_station_mismatch() {
        let row = WeatherUndergroundObservation {
            station_id: "OTHER".to_owned(),
            ..observation(START_MS / 1_000, PublicStationNumber::Invalid)
        };
        let failure = normalize_weather_underground_rows_with_diagnostics(
            &[vec![row]],
            STATION_ID,
            ScheduledWindow {
                start_ms: START_MS,
                end_exclusive_ms: START_MS + 60_000,
            },
        )
        .expect_err("station substitution must fail");
        assert_eq!(
            failure.message(),
            "Weather Underground station identity is invalid"
        );
    }

    /// reject invalid epochs and ranges before accepting any row payload
    #[test]
    fn rejects_invalid_epoch_and_oversized_window() {
        let invalid_epoch = normalize_weather_underground_rows_with_diagnostics(
            &[vec![observation(0, PublicStationNumber::Missing)]],
            STATION_ID,
            ScheduledWindow {
                start_ms: START_MS,
                end_exclusive_ms: START_MS + 60_000,
            },
        )
        .expect_err("zero epoch must fail");
        assert_eq!(
            invalid_epoch.message(),
            "observations[0].epoch must be a positive integer"
        );
        let oversized = normalize_weather_underground_rows(
            &[],
            STATION_ID,
            ScheduledWindow {
                start_ms: START_MS,
                end_exclusive_ms: START_MS + MAXIMUM_WUNDERGROUND_RANGE_MS + 1,
            },
        )
        .expect_err("an oversized provider window must fail");
        assert_eq!(oversized.code, "provider_unavailable");
    }

    /// retain the exact original object and numeric-cell diagnoses
    #[test]
    fn retains_metric_object_and_numeric_field_diagnoses() {
        let window = ScheduledWindow {
            start_ms: START_MS,
            end_exclusive_ms: START_MS + 60_000,
        };
        let missing_metric = WeatherUndergroundObservation {
            metric: None,
            ..observation(START_MS / 1_000, PublicStationNumber::Number(12.0))
        };
        let failure = normalize_weather_underground_rows_with_diagnostics(
            &[vec![missing_metric]],
            STATION_ID,
            window,
        )
        .expect_err("missing metric object must fail");
        assert_eq!(failure.message(), "observation.metric must be an object");

        let invalid_pressure = WeatherUndergroundObservation {
            metric: Some(WeatherUndergroundMetricObservation {
                pressure_max_hpa: PublicStationNumber::Invalid,
                ..metric_observation()
            }),
            ..observation(START_MS / 1_000, PublicStationNumber::Number(12.0))
        };
        let failure = normalize_weather_underground_rows_with_diagnostics(
            &[vec![invalid_pressure]],
            STATION_ID,
            window,
        )
        .expect_err("invalid pressure must fail");
        assert_eq!(
            failure.message(),
            "pressureMax must be a finite number or null"
        );

        let invalid_uv = WeatherUndergroundObservation {
            uv_high: PublicStationNumber::Invalid,
            ..observation(START_MS / 1_000, PublicStationNumber::Number(12.0))
        };
        let failure = normalize_weather_underground_rows_with_diagnostics(
            &[vec![invalid_uv]],
            STATION_ID,
            window,
        )
        .expect_err("invalid UV must fail");
        assert_eq!(
            failure.message(),
            "uv index must be a finite number or null"
        );

        let invalid_direction = WeatherUndergroundObservation {
            wind_direction_avg_degrees: PublicStationNumber::Invalid,
            ..observation(START_MS / 1_000, PublicStationNumber::Number(12.0))
        };
        let failure = normalize_weather_underground_rows_with_diagnostics(
            &[vec![invalid_direction]],
            STATION_ID,
            window,
        )
        .expect_err("invalid wind direction must fail");
        assert_eq!(
            failure.message(),
            "wind direction must be a finite number or null"
        );
    }

    /// build one otherwise-valid typed provider observation
    fn observation(
        epoch_seconds: i64,
        temperature_avg_c: PublicStationNumber,
    ) -> WeatherUndergroundObservation {
        WeatherUndergroundObservation {
            epoch_seconds,
            humidity_avg_percent: PublicStationNumber::Number(50.0),
            metric: Some(WeatherUndergroundMetricObservation {
                temperature_avg_c,
                ..metric_observation()
            }),
            qc_status: WeatherUndergroundQcStatus::AdmittedNumber("1".to_owned()),
            solar_radiation_high_wm2: PublicStationNumber::Number(120.0),
            station_id: STATION_ID.to_owned(),
            uv_high: PublicStationNumber::Number(3.0),
            wind_direction_avg_degrees: PublicStationNumber::Number(360.0),
        }
    }

    /// build one valid metric sub-object with representative metric units
    fn metric_observation() -> WeatherUndergroundMetricObservation {
        WeatherUndergroundMetricObservation {
            precip_rate_mm_per_hour: PublicStationNumber::Number(1.5),
            pressure_max_hpa: PublicStationNumber::Number(1_002.0),
            pressure_min_hpa: PublicStationNumber::Number(998.0),
            temperature_avg_c: PublicStationNumber::Number(12.0),
            wind_gust_high_kph: PublicStationNumber::Number(36.0),
            wind_speed_avg_kph: PublicStationNumber::Number(18.0),
        }
    }
}
