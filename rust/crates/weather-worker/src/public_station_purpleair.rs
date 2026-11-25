//! Typed PurpleAir public-map history normalization after CSV decoding.

use std::collections::BTreeMap;

use super::{ScheduledWindow, TaskFailure};
use crate::supplemental_public_station::{
    PublicStationMetric, PublicStationNormalizationError, PublicStationNormalizedSample,
    PublicStationNumber, empty_public_station_metrics, public_station_average,
    public_station_metric, public_station_number, public_station_range,
};

const MAX_JS_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const MAX_JS_DATE_MS: i64 = 8_640_000_000_000_000;
const MAX_PURPLEAIR_RANGE_MS: i64 = 2 * 24 * 60 * 60 * 1_000;
const MILLISECONDS_PER_SECOND: i64 = 1_000;

/// One PurpleAir history response decoded by the approved CSV and time boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct PurpleAirDecodedHistory {
    pub header: PurpleAirDecodedHeader,
    pub rows: Vec<PurpleAirDecodedRow>,
}

/// Decoded schema state retaining the distinction between an empty response and a bad header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PurpleAirDecodedHeader {
    Missing,
    Invalid,
    Valid,
}

/// One decoded PurpleAir history row without collapsing invalid cells to gaps.
#[derive(Clone, Debug, PartialEq)]
pub struct PurpleAirDecodedRow {
    pub row_width_valid: bool,
    pub sensor_index: Option<i64>,
    pub timestamp_ms: Option<i64>,
    pub humidity_a: PublicStationNumber,
    pub humidity_b: PublicStationNumber,
    pub pm25_atm_a: PublicStationNumber,
    pub pm25_atm_b: PublicStationNumber,
    pub pressure_a: PublicStationNumber,
    pub pressure_b: PublicStationNumber,
    pub temperature_a: PublicStationNumber,
    pub temperature_b: PublicStationNumber,
}

/// Normalize decoded PurpleAir rows while preserving the scheduled worker's bounded error.
pub fn normalize_purpleair_rows(
    payload: &PurpleAirDecodedHistory,
    sensor_index: u64,
    window: ScheduledWindow,
) -> Result<Vec<PublicStationNormalizedSample>, TaskFailure> {
    normalize_purpleair_rows_with_diagnostics(payload, sensor_index, window)
        .map_err(PublicStationNormalizationError::into_task_failure)
}

/// Normalize decoded PurpleAir rows while retaining exact provider diagnoses for archive runs.
pub(super) fn normalize_purpleair_rows_with_diagnostics(
    payload: &PurpleAirDecodedHistory,
    sensor_index: u64,
    window: ScheduledWindow,
) -> Result<Vec<PublicStationNormalizedSample>, PublicStationNormalizationError> {
    public_station_range(window, MAX_PURPLEAIR_RANGE_MS)?;

    // require the configured positive safe integer identity
    if sensor_index == 0 || sensor_index > MAX_JS_SAFE_INTEGER as u64 {
        return Err(PublicStationNormalizationError::invalid_payload(
            "sensorIndex must be a positive integer",
        ));
    }
    let sensor_index = sensor_index as i64;

    // retain the original distinction between an empty response and a malformed schema line
    match payload.header {
        PurpleAirDecodedHeader::Missing => {
            return Err(PublicStationNormalizationError::invalid_payload(
                "PurpleAir history CSV is empty",
            ));
        }
        PurpleAirDecodedHeader::Invalid => {
            return Err(PublicStationNormalizationError::invalid_payload(
                "PurpleAir history CSV schema is invalid",
            ));
        }
        PurpleAirDecodedHeader::Valid => {}
    }

    let mut distinct = BTreeMap::new();

    // validate every row before retaining exact in-window timestamps
    for (index, row) in payload.rows.iter().enumerate() {
        let row_number = index + 1;
        // reject malformed decoded row widths
        if !row.row_width_valid {
            return Err(PublicStationNormalizationError::invalid_payload(format!(
                "PurpleAir history row {row_number} has invalid width"
            )));
        }

        let Some(row_sensor_index) = row.sensor_index else {
            return Err(PublicStationNormalizationError::invalid_payload(format!(
                "PurpleAir history row {row_number} sensor_index is invalid"
            )));
        };

        // reject integers outside Number.isSafeInteger before comparing identities
        if !(-MAX_JS_SAFE_INTEGER..=MAX_JS_SAFE_INTEGER).contains(&row_sensor_index) {
            return Err(PublicStationNormalizationError::invalid_payload(format!(
                "PurpleAir history row {row_number} sensor_index is invalid"
            )));
        }

        // zero and negative safe integers are valid CSV numbers but still the wrong sensor
        if row_sensor_index != sensor_index {
            return Err(PublicStationNormalizationError::invalid_payload(
                "PurpleAir response sensor identity is invalid",
            ));
        }

        let Some(timestamp_ms) = row.timestamp_ms else {
            return Err(PublicStationNormalizationError::invalid_payload(format!(
                "PurpleAir history row {row_number} timestamp is invalid"
            )));
        };

        // retain only decoded JavaScript dates at exact seconds
        if !(-MAX_JS_DATE_MS..=MAX_JS_DATE_MS).contains(&timestamp_ms)
            || timestamp_ms % MILLISECONDS_PER_SECOND != 0
        {
            return Err(PublicStationNormalizationError::invalid_payload(format!(
                "PurpleAir history row {row_number} timestamp is invalid"
            )));
        }

        // exclude provider boundary padding
        if timestamp_ms < window.start_ms || timestamp_ms >= window.end_exclusive_ms {
            continue;
        }

        distinct.entry(timestamp_ms).or_insert(row);
    }

    let mut samples = Vec::with_capacity(distinct.len());

    // normalize only the first retained row for each sorted timestamp
    for (valid_at_ms, row) in distinct {
        samples.push(normalize_purpleair_row_with_diagnostics(valid_at_ms, row)?);
    }

    Ok(samples)
}

/// Normalize one retained PurpleAir dual-channel observation.
fn normalize_purpleair_row_with_diagnostics(
    valid_at_ms: i64,
    row: &PurpleAirDecodedRow,
) -> Result<PublicStationNormalizedSample, PublicStationNormalizationError> {
    let pm25 = purpleair_average(row.pm25_atm_a, row.pm25_atm_b, "pm2.5_atm_a", "pm2.5_atm_b")?;
    let mut quality_flags = vec![
        "dual_channel_average",
        "uncorrected_sensor_enclosure_temperature",
    ];

    // flag and discard impossible particulate spikes
    let pm25 = if pm25.is_some_and(|value| !(0.0..=999.0).contains(&value)) {
        quality_flags.push("pm25_out_of_range");
        None
    } else {
        public_station_metric(PublicStationMetric::Pm25, pm25)?
    };

    let mut metrics = empty_public_station_metrics();
    metrics.pm25_micrograms_per_cubic_meter = pm25;
    metrics.pressure_hpa = purpleair_metric_average(
        PublicStationMetric::PressureHpa,
        "pressureHpa",
        (row.pressure_a, "pressure_a"),
        (row.pressure_b, "pressure_b"),
    )?;
    metrics.relative_humidity_percent = purpleair_metric_average(
        PublicStationMetric::HumidityPercent,
        "relativeHumidityPercent",
        (row.humidity_a, "humidity_a"),
        (row.humidity_b, "humidity_b"),
    )?;
    metrics.temperature_c = purpleair_metric_average(
        PublicStationMetric::Fahrenheit,
        "temperatureC",
        (row.temperature_a, "temperature_a"),
        (row.temperature_b, "temperature_b"),
    )?;

    Ok(PublicStationNormalizedSample {
        valid_at_ms,
        metrics,
        quality_status: None,
        quality_flags,
    })
}

/// Apply the original nullable-number check to the averaged CSV metric before domain conversion.
fn purpleair_metric_average(
    metric: PublicStationMetric,
    field: &str,
    left: (PublicStationNumber, &str),
    right: (PublicStationNumber, &str),
) -> Result<Option<f64>, PublicStationNormalizationError> {
    let average = purpleair_average(left.0, right.0, left.1, right.1)?;
    let checked = public_station_number(
        average.map_or(PublicStationNumber::Missing, PublicStationNumber::Number),
        field,
    )?;
    public_station_metric(metric, checked)
}

/// Decode and average the available values from one PurpleAir channel pair.
fn purpleair_average(
    left: PublicStationNumber,
    right: PublicStationNumber,
    left_field: &str,
    right_field: &str,
) -> Result<Option<f64>, PublicStationNormalizationError> {
    Ok(public_station_average(
        purpleair_csv_number(left, left_field)?,
        purpleair_csv_number(right, right_field)?,
    ))
}

// retain PurpleAir CSV numeric diagnostics rather than the generic JSON-number wording
fn purpleair_csv_number(
    value: PublicStationNumber,
    field: &str,
) -> Result<Option<f64>, PublicStationNormalizationError> {
    match value {
        PublicStationNumber::Missing => Ok(None),
        PublicStationNumber::Number(value) if value.is_finite() => Ok(Some(value)),
        _ => Err(PublicStationNormalizationError::invalid_payload(format!(
            "PurpleAir {field} must be numeric or null"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENSOR_INDEX: u64 = 42;
    const START_MS: i64 = 1_700_000_000_000;

    /// Build one ordinary two-minute PurpleAir row.
    fn row(timestamp_ms: i64) -> PurpleAirDecodedRow {
        PurpleAirDecodedRow {
            row_width_valid: true,
            sensor_index: Some(SENSOR_INDEX as i64),
            timestamp_ms: Some(timestamp_ms),
            humidity_a: PublicStationNumber::Number(55.0),
            humidity_b: PublicStationNumber::Number(57.0),
            pm25_atm_a: PublicStationNumber::Number(10.0),
            pm25_atm_b: PublicStationNumber::Number(12.0),
            pressure_a: PublicStationNumber::Number(1_010.0),
            pressure_b: PublicStationNumber::Number(1_012.0),
            temperature_a: PublicStationNumber::Number(68.0),
            temperature_b: PublicStationNumber::Number(70.0),
        }
    }

    /// Build the ordinary half-open fixture window.
    const fn window() -> ScheduledWindow {
        ScheduledWindow {
            start_ms: START_MS,
            end_exclusive_ms: START_MS + 10 * 60 * 1_000,
        }
    }

    /// Wrap decoded rows under a validated schema.
    fn history(rows: Vec<PurpleAirDecodedRow>) -> PurpleAirDecodedHistory {
        PurpleAirDecodedHistory {
            header: PurpleAirDecodedHeader::Valid,
            rows,
        }
    }

    /// UTC slicing sorts retained rows and preserves the first duplicate.
    #[test]
    fn slices_sorts_and_deduplicates_exact_timestamps() {
        let mut duplicate = row(START_MS + 2 * 60 * 1_000);
        duplicate.pm25_atm_a = PublicStationNumber::Invalid;
        let payload = history(vec![
            row(START_MS + 12 * 60 * 1_000),
            row(START_MS + 4 * 60 * 1_000),
            row(START_MS + 2 * 60 * 1_000),
            duplicate,
            row(START_MS - 2 * 60 * 1_000),
        ]);

        let samples = normalize_purpleair_rows(&payload, SENSOR_INDEX, window()).unwrap();

        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].valid_at_ms, START_MS + 2 * 60 * 1_000);
        assert_eq!(samples[1].valid_at_ms, START_MS + 4 * 60 * 1_000);
        assert_eq!(
            samples[0].metrics.pm25_micrograms_per_cubic_meter,
            Some(11.0)
        );
    }

    /// Identity and timestamp validation occurs before window filtering.
    #[test]
    fn rejects_invalid_identity_and_timestamp_outside_the_window() {
        let mut wrong_sensor = row(START_MS - 1_000);
        wrong_sensor.sensor_index = Some(SENSOR_INDEX as i64 + 1);
        let identity_error =
            normalize_purpleair_rows(&history(vec![wrong_sensor]), SENSOR_INDEX, window())
                .unwrap_err();
        assert_eq!(identity_error.code, "invalid_payload");

        let mut fractional_second = row(START_MS - 1);
        fractional_second.timestamp_ms = Some(START_MS - 1);
        let timestamp_error =
            normalize_purpleair_rows(&history(vec![fractional_second]), SENSOR_INDEX, window())
                .unwrap_err();
        assert_eq!(timestamp_error.code, "invalid_payload");
    }

    /// Retain the original empty-response and malformed-header diagnoses separately.
    #[test]
    fn retains_exact_header_diagnostics() {
        let empty = normalize_purpleair_rows_with_diagnostics(
            &PurpleAirDecodedHistory {
                header: PurpleAirDecodedHeader::Missing,
                rows: Vec::new(),
            },
            SENSOR_INDEX,
            window(),
        )
        .unwrap_err();
        assert_eq!(empty.message(), "PurpleAir history CSV is empty");

        let invalid = normalize_purpleair_rows_with_diagnostics(
            &PurpleAirDecodedHistory {
                header: PurpleAirDecodedHeader::Invalid,
                rows: Vec::new(),
            },
            SENSOR_INDEX,
            window(),
        )
        .unwrap_err();
        assert_eq!(invalid.message(), "PurpleAir history CSV schema is invalid");
    }

    /// Keep row positions and sensor/timestamp diagnoses through decoded validation.
    #[test]
    fn retains_exact_row_diagnostics() {
        let configured =
            normalize_purpleair_rows_with_diagnostics(&history(Vec::new()), 0, window())
                .unwrap_err();
        assert_eq!(
            configured.message(),
            "sensorIndex must be a positive integer"
        );

        let mut bad_width = row(START_MS);
        bad_width.row_width_valid = false;
        let width = normalize_purpleair_rows_with_diagnostics(
            &history(vec![row(START_MS - 1_000), bad_width]),
            SENSOR_INDEX,
            window(),
        )
        .unwrap_err();
        assert_eq!(width.message(), "PurpleAir history row 2 has invalid width");

        let mut malformed_sensor = row(START_MS);
        malformed_sensor.sensor_index = None;
        let sensor = normalize_purpleair_rows_with_diagnostics(
            &history(vec![malformed_sensor]),
            SENSOR_INDEX,
            window(),
        )
        .unwrap_err();
        assert_eq!(
            sensor.message(),
            "PurpleAir history row 1 sensor_index is invalid"
        );

        let mut unsafe_sensor = row(START_MS);
        unsafe_sensor.sensor_index = Some(MAX_JS_SAFE_INTEGER + 1);
        let unsafe_sensor = normalize_purpleair_rows_with_diagnostics(
            &history(vec![unsafe_sensor]),
            SENSOR_INDEX,
            window(),
        )
        .unwrap_err();
        assert_eq!(
            unsafe_sensor.message(),
            "PurpleAir history row 1 sensor_index is invalid"
        );

        let mut wrong_sensor = row(START_MS);
        wrong_sensor.sensor_index = Some(-1);
        let identity = normalize_purpleair_rows_with_diagnostics(
            &history(vec![wrong_sensor]),
            SENSOR_INDEX,
            window(),
        )
        .unwrap_err();
        assert_eq!(
            identity.message(),
            "PurpleAir response sensor identity is invalid"
        );

        let mut malformed_timestamp = row(START_MS);
        malformed_timestamp.timestamp_ms = None;
        let timestamp = normalize_purpleair_rows_with_diagnostics(
            &history(vec![malformed_timestamp]),
            SENSOR_INDEX,
            window(),
        )
        .unwrap_err();
        assert_eq!(
            timestamp.message(),
            "PurpleAir history row 1 timestamp is invalid"
        );
    }

    /// Identify the exact malformed numeric column while the scheduled API stays bounded.
    #[test]
    fn retains_exact_channel_diagnostic_and_legacy_code() {
        let mut malformed = row(START_MS);
        malformed.pressure_b = PublicStationNumber::Invalid;
        let rich = normalize_purpleair_rows_with_diagnostics(
            &history(vec![malformed.clone()]),
            SENSOR_INDEX,
            window(),
        )
        .unwrap_err();
        assert_eq!(
            rich.message(),
            "PurpleAir pressure_b must be numeric or null"
        );

        let scheduled = normalize_purpleair_rows(&history(vec![malformed]), SENSOR_INDEX, window())
            .unwrap_err();
        assert_eq!(scheduled.code, "invalid_payload");
        assert_eq!(scheduled.attempts, 0);
    }

    /// Channel gaps use the available value while pairs use their average.
    #[test]
    fn normalizes_single_and_dual_channels() {
        let mut decoded = row(START_MS);
        decoded.pm25_atm_b = PublicStationNumber::Missing;
        decoded.pressure_b = PublicStationNumber::Missing;
        let samples =
            normalize_purpleair_rows(&history(vec![decoded]), SENSOR_INDEX, window()).unwrap();
        let sample = &samples[0];

        assert_eq!(sample.metrics.pm25_micrograms_per_cubic_meter, Some(10.0));
        assert_eq!(sample.metrics.pressure_hpa, Some(1_010.0));
        assert_eq!(sample.metrics.relative_humidity_percent, Some(56.0));
        assert!((sample.metrics.temperature_c.unwrap() - 20.555_555_555_555_557).abs() < 1e-12);
        assert_eq!(
            sample.quality_flags,
            vec![
                "dual_channel_average",
                "uncorrected_sensor_enclosure_temperature"
            ]
        );
    }

    /// Particulate spikes become flagged gaps without rejecting the row.
    #[test]
    fn flags_out_of_range_pm25() {
        let mut decoded = row(START_MS);
        decoded.pm25_atm_a = PublicStationNumber::Number(1_000.0);
        decoded.pm25_atm_b = PublicStationNumber::Missing;
        let samples =
            normalize_purpleair_rows(&history(vec![decoded]), SENSOR_INDEX, window()).unwrap();
        let sample = &samples[0];

        assert_eq!(sample.metrics.pm25_micrograms_per_cubic_meter, None);
        assert_eq!(
            sample.quality_flags,
            vec![
                "dual_channel_average",
                "uncorrected_sensor_enclosure_temperature",
                "pm25_out_of_range"
            ]
        );
    }

    /// PurpleAir requests remain limited to two exact-second days.
    #[test]
    fn rejects_ranges_over_two_days() {
        let oversized = ScheduledWindow {
            start_ms: START_MS,
            end_exclusive_ms: START_MS + MAX_PURPLEAIR_RANGE_MS + 1_000,
        };

        let error =
            normalize_purpleair_rows(&history(Vec::new()), SENSOR_INDEX, oversized).unwrap_err();

        assert_eq!(error.code, "provider_unavailable");
    }

    /// Retained nonfinite and malformed channel cells remain invalid payloads.
    #[test]
    fn rejects_invalid_retained_channel_values() {
        let mut nonfinite = row(START_MS);
        nonfinite.humidity_a = PublicStationNumber::Number(f64::NAN);
        let nonfinite_error =
            normalize_purpleair_rows(&history(vec![nonfinite]), SENSOR_INDEX, window())
                .unwrap_err();
        assert_eq!(nonfinite_error.code, "invalid_payload");

        let mut malformed = row(START_MS);
        malformed.temperature_b = PublicStationNumber::Invalid;
        let malformed_error =
            normalize_purpleair_rows(&history(vec![malformed]), SENSOR_INDEX, window())
                .unwrap_err();
        assert_eq!(malformed_error.code, "invalid_payload");
    }
}
