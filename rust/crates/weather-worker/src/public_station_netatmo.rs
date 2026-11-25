//! Typed Netatmo public-station series normalization without transport or JSON ownership.

use std::collections::BTreeMap;

use crate::supplemental_public_station::{
    PublicStationMetric, PublicStationNormalizationError, PublicStationNormalizedSample,
    PublicStationNumber, empty_public_station_metrics, public_station_direction,
    public_station_epoch_ms, public_station_metric, public_station_number, public_station_range,
};
use crate::{CanonicalWeatherMetrics, ScheduledWindow, TaskFailure};

const MAXIMUM_NETATMO_RANGE_MILLISECONDS: i64 = 31 * 24 * 60 * 60 * 1_000;
const FIVE_MINUTES_MILLISECONDS: i64 = 5 * 60 * 1_000;

const OUTDOOR_FIELDS: [NetatmoField; 2] = [
    NetatmoField::TemperatureC,
    NetatmoField::RelativeHumidityPercent,
];
const PRESSURE_FIELDS: [NetatmoField; 1] = [NetatmoField::PressureHpa];
const RAIN_FIELDS: [NetatmoField; 1] = [NetatmoField::PrecipitationMm];
const WIND_FIELDS: [NetatmoField; 4] = [
    NetatmoField::WindSpeedKph,
    NetatmoField::WindDirectionDegrees,
    NetatmoField::WindGustKph,
    NetatmoField::UnusedGustDirection,
];

/// Typed success state decoded from one Netatmo measurement response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetatmoMeasurementStatus {
    Ok,
    InvalidObject,
    Invalid,
}

/// Typed body variants that preserve Netatmo's accepted empty representations.
#[derive(Clone, Debug, PartialEq)]
pub enum NetatmoMeasurementBody {
    Null,
    EmptyArray,
    Rows(Vec<NetatmoMeasurementRow>),
    Invalid,
}

/// One provider timestamp and its still-lazy measurement vector.
#[derive(Clone, Debug, PartialEq)]
pub struct NetatmoMeasurementRow {
    pub epoch_seconds: i64,
    pub values: NetatmoMeasurementValues,
}

/// One typed measurement vector without collapsing malformed rows into gaps.
#[derive(Clone, Debug, PartialEq)]
pub enum NetatmoMeasurementValues {
    Numbers(Vec<PublicStationNumber>),
    Invalid { epoch_text: String },
}

/// One decoded module response in the decoder's JavaScript Object.entries order.
#[derive(Clone, Debug, PartialEq)]
pub struct NetatmoMeasurementSeries {
    pub status: NetatmoMeasurementStatus,
    pub body: NetatmoMeasurementBody,
}

#[derive(Clone, Copy)]
enum NetatmoField {
    TemperatureC,
    RelativeHumidityPercent,
    PressureHpa,
    PrecipitationMm,
    WindSpeedKph,
    WindDirectionDegrees,
    WindGustKph,
    UnusedGustDirection,
}

struct NetatmoBucket {
    metrics: CanonicalWeatherMetrics,
    valid_at_ms: i64,
}

/// Normalize four decoded Netatmo module series into ordered five-minute samples.
pub fn normalize_netatmo_series(
    series: &[NetatmoMeasurementSeries; 4],
    window: ScheduledWindow,
) -> Result<Vec<PublicStationNormalizedSample>, TaskFailure> {
    normalize_netatmo_series_with_diagnostics(series, window)
        .map_err(PublicStationNormalizationError::into_task_failure)
}

/// Normalize decoded Netatmo series while retaining the original provider diagnosis.
pub(super) fn normalize_netatmo_series_with_diagnostics(
    series: &[NetatmoMeasurementSeries; 4],
    window: ScheduledWindow,
) -> Result<Vec<PublicStationNormalizedSample>, PublicStationNormalizationError> {
    public_station_range(window, MAXIMUM_NETATMO_RANGE_MILLISECONDS)?;
    let mut buckets = BTreeMap::new();
    merge_netatmo_series(&mut buckets, &series[0], window, &OUTDOOR_FIELDS)?;
    merge_netatmo_series(&mut buckets, &series[1], window, &PRESSURE_FIELDS)?;
    merge_netatmo_series(&mut buckets, &series[2], window, &RAIN_FIELDS)?;
    merge_netatmo_series(&mut buckets, &series[3], window, &WIND_FIELDS)?;

    Ok(buckets
        .into_values()
        .map(|bucket| PublicStationNormalizedSample {
            valid_at_ms: bucket.valid_at_ms,
            metrics: bucket.metrics,
            quality_status: None,
            quality_flags: Vec::new(),
        })
        .collect())
}

/// Merge one module's ordered rows into shared five-minute buckets.
fn merge_netatmo_series(
    buckets: &mut BTreeMap<i64, NetatmoBucket>,
    series: &NetatmoMeasurementSeries,
    window: ScheduledWindow,
    fields: &[NetatmoField],
) -> Result<(), PublicStationNormalizationError> {
    // require the provider's exact successful response status
    match series.status {
        NetatmoMeasurementStatus::Ok => {}
        NetatmoMeasurementStatus::InvalidObject => {
            return Err(PublicStationNormalizationError::invalid_payload(
                "Netatmo measure response must be an object",
            ));
        }
        NetatmoMeasurementStatus::Invalid => {
            return Err(PublicStationNormalizationError::invalid_payload(
                "Netatmo measure response is not successful",
            ));
        }
    }
    let rows = match &series.body {
        NetatmoMeasurementBody::Null | NetatmoMeasurementBody::EmptyArray => return Ok(()),
        NetatmoMeasurementBody::Rows(rows) => rows,
        NetatmoMeasurementBody::Invalid => {
            return Err(PublicStationNormalizationError::invalid_payload(
                "Netatmo measure body must be an object",
            ));
        }
    };

    // retain the decoder's JavaScript Object.entries order for row overwrite behavior
    for row in rows {
        let epoch_ms = public_station_epoch_ms(row.epoch_seconds).map_err(|_| {
            PublicStationNormalizationError::invalid_payload("Netatmo measure timestamp is invalid")
        })?;

        // filter response padding before inspecting the provider row representation or width
        if epoch_ms < window.start_ms || epoch_ms >= window.end_exclusive_ms {
            continue;
        }
        let values = match &row.values {
            NetatmoMeasurementValues::Numbers(values) => values,
            NetatmoMeasurementValues::Invalid { epoch_text } => {
                return Err(PublicStationNormalizationError::invalid_payload(format!(
                    "Netatmo measure {epoch_text} must be an array"
                )));
            }
        };

        // require the full type vector while permitting provider-added trailing cells
        if values.len() < fields.len() {
            return Err(PublicStationNormalizationError::invalid_payload(
                "Netatmo measure row is shorter than requested types",
            ));
        }
        let bucket_at_ms = epoch_ms / FIVE_MINUTES_MILLISECONDS * FIVE_MINUTES_MILLISECONDS;
        let bucket = buckets.entry(bucket_at_ms).or_insert(NetatmoBucket {
            metrics: empty_public_station_metrics(),
            valid_at_ms: epoch_ms,
        });
        bucket.valid_at_ms = bucket.valid_at_ms.max(epoch_ms);

        // overwrite every requested metric in the original row order, including missing cells
        for (index, field) in fields.iter().copied().enumerate() {
            merge_netatmo_field(&mut bucket.metrics, field, values[index])?;
        }
    }
    Ok(())
}

/// Normalize and overwrite one requested Netatmo metric.
fn merge_netatmo_field(
    metrics: &mut CanonicalWeatherMetrics,
    field: NetatmoField,
    value: PublicStationNumber,
) -> Result<(), PublicStationNormalizationError> {
    // ignore gust angle before interpreting its unused provider cell
    if matches!(field, NetatmoField::UnusedGustDirection) {
        return Ok(());
    }
    let value = public_station_number(value, field.diagnostic_name())?;
    match field {
        NetatmoField::TemperatureC => {
            metrics.temperature_c = public_station_metric(PublicStationMetric::Celsius, value)?;
        }
        NetatmoField::RelativeHumidityPercent => {
            metrics.relative_humidity_percent =
                public_station_metric(PublicStationMetric::HumidityPercent, value)?;
        }
        NetatmoField::PressureHpa => {
            metrics.pressure_hpa = public_station_metric(PublicStationMetric::PressureHpa, value)?;
        }
        NetatmoField::PrecipitationMm => {
            metrics.precipitation_mm =
                public_station_metric(PublicStationMetric::PrecipitationMm, value)?;
        }
        NetatmoField::WindSpeedKph => {
            metrics.wind_speed_mps =
                public_station_metric(PublicStationMetric::WindSpeedKph, value)?;
        }
        NetatmoField::WindDirectionDegrees => {
            metrics.wind_direction_degrees = public_station_direction(value)?;
        }
        NetatmoField::WindGustKph => {
            metrics.wind_gust_mps = public_station_metric(PublicStationMetric::WindGustKph, value)?;
        }
        NetatmoField::UnusedGustDirection => unreachable!("unused gust direction returned early"),
    }
    Ok(())
}

impl NetatmoField {
    /// Return the exact field name used by the TypeScript nullable-number diagnosis.
    const fn diagnostic_name(self) -> &'static str {
        match self {
            Self::TemperatureC => "temperatureC",
            Self::RelativeHumidityPercent => "relativeHumidityPercent",
            Self::PressureHpa => "pressureHpa",
            Self::PrecipitationMm => "precipitationMm",
            Self::WindSpeedKph => "windSpeedMps",
            Self::WindDirectionDegrees => "wind direction",
            Self::WindGustKph => "windGustMps",
            Self::UnusedGustDirection => "unusedGustDirection",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NetatmoMeasurementBody, NetatmoMeasurementRow, NetatmoMeasurementSeries,
        NetatmoMeasurementStatus, NetatmoMeasurementValues, normalize_netatmo_series,
    };
    use crate::ScheduledWindow;
    use crate::supplemental_public_station::PublicStationNumber;

    const START_MS: i64 = 1_787_576_400_000;
    const END_MS: i64 = START_MS + 3_600_000;

    /// Build one successful typed module body.
    fn series(body: NetatmoMeasurementBody) -> NetatmoMeasurementSeries {
        NetatmoMeasurementSeries {
            status: NetatmoMeasurementStatus::Ok,
            body,
        }
    }

    /// Build one typed Netatmo row.
    fn row(epoch_seconds: i64, values: Vec<PublicStationNumber>) -> NetatmoMeasurementRow {
        NetatmoMeasurementRow {
            epoch_seconds,
            values: NetatmoMeasurementValues::Numbers(values),
        }
    }

    /// Return the bounded fixture interval.
    const fn window() -> ScheduledWindow {
        ScheduledWindow {
            start_ms: START_MS,
            end_exclusive_ms: END_MS,
        }
    }

    /// Merge all four module widths into one aligned provider bucket.
    #[test]
    fn aligns_module_series_and_keeps_the_latest_valid_time() {
        let samples = normalize_netatmo_series(
            &[
                series(NetatmoMeasurementBody::Rows(vec![row(
                    1_787_576_617,
                    vec![
                        PublicStationNumber::Number(12.7),
                        PublicStationNumber::Number(96.0),
                    ],
                )])),
                series(NetatmoMeasurementBody::Rows(vec![row(
                    1_787_576_661,
                    vec![PublicStationNumber::Number(1_018.5)],
                )])),
                series(NetatmoMeasurementBody::Rows(vec![row(
                    1_787_576_655,
                    vec![PublicStationNumber::Number(0.2)],
                )])),
                series(NetatmoMeasurementBody::Rows(vec![row(
                    1_787_576_655,
                    vec![
                        PublicStationNumber::Number(3.0),
                        PublicStationNumber::Number(-1.0),
                        PublicStationNumber::Number(5.0),
                        PublicStationNumber::Invalid,
                    ],
                )])),
            ],
            window(),
        )
        .expect("Netatmo module fixture should normalize");

        assert_eq!(samples.len(), 1);
        let sample = &samples[0];
        assert_eq!(sample.valid_at_ms, 1_787_576_661_000);
        assert_eq!(sample.metrics.temperature_c, Some(12.7));
        assert_eq!(sample.metrics.relative_humidity_percent, Some(96.0));
        assert_eq!(sample.metrics.pressure_hpa, Some(1_018.5));
        assert_eq!(sample.metrics.precipitation_mm, Some(0.2));
        assert_eq!(sample.metrics.wind_speed_mps, Some(3.0 / 3.6));
        assert_eq!(sample.metrics.wind_direction_degrees, None);
        assert_eq!(sample.metrics.wind_gust_mps, Some(5.0 / 3.6));
        assert_eq!(sample.quality_status, None);
        assert!(sample.quality_flags.is_empty());
    }

    /// Preserve missing-cell overwrite semantics and accepted empty module forms.
    #[test]
    fn later_rows_overwrite_metrics_with_missing_cells() {
        let samples = normalize_netatmo_series(
            &[
                series(NetatmoMeasurementBody::Rows(vec![
                    row(
                        1_787_576_617,
                        vec![
                            PublicStationNumber::Number(12.7),
                            PublicStationNumber::Number(96.0),
                        ],
                    ),
                    row(
                        1_787_576_650,
                        vec![
                            PublicStationNumber::Missing,
                            PublicStationNumber::Number(97.0),
                        ],
                    ),
                ])),
                series(NetatmoMeasurementBody::Rows(Vec::new())),
                series(NetatmoMeasurementBody::EmptyArray),
                series(NetatmoMeasurementBody::Null),
            ],
            window(),
        )
        .expect("empty Netatmo module forms should preserve outdoor readings");

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].valid_at_ms, 1_787_576_650_000);
        assert_eq!(samples[0].metrics.temperature_c, None);
        assert_eq!(samples[0].metrics.relative_humidity_percent, Some(97.0));
        assert_eq!(samples[0].metrics.pressure_hpa, None);
    }

    /// Filter padded malformed rows before width checks but reject them inside the window.
    #[test]
    fn filters_padding_before_rejecting_a_malformed_row() {
        let outside = NetatmoMeasurementRow {
            epoch_seconds: 1_787_576_399,
            values: NetatmoMeasurementValues::Invalid {
                epoch_text: "1787576399".to_owned(),
            },
        };
        let samples = normalize_netatmo_series(
            &[
                series(NetatmoMeasurementBody::Rows(vec![outside])),
                series(NetatmoMeasurementBody::Null),
                series(NetatmoMeasurementBody::Null),
                series(NetatmoMeasurementBody::Null),
            ],
            window(),
        )
        .expect("out-of-window row representation should remain lazy");
        assert!(samples.is_empty());

        let failure = normalize_netatmo_series(
            &[
                series(NetatmoMeasurementBody::Rows(vec![row(
                    1_787_576_617,
                    vec![PublicStationNumber::Number(12.7)],
                )])),
                series(NetatmoMeasurementBody::Null),
                series(NetatmoMeasurementBody::Null),
                series(NetatmoMeasurementBody::Null),
            ],
            window(),
        )
        .expect_err("in-window short row should be invalid");
        assert_eq!(failure.code, "invalid_payload");
        assert_eq!(failure.attempts, 0);
    }

    /// Retain exact decoded response diagnoses while preserving the legacy wrapper.
    #[test]
    fn retains_exact_invalid_response_diagnoses() {
        let invalid_root = [
            NetatmoMeasurementSeries {
                status: NetatmoMeasurementStatus::InvalidObject,
                body: NetatmoMeasurementBody::Null,
            },
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
        ];
        let root_failure =
            super::normalize_netatmo_series_with_diagnostics(&invalid_root, window())
                .expect_err("non-object response must be rejected");
        assert_eq!(
            root_failure.message(),
            "Netatmo measure response must be an object"
        );

        let invalid_status = [
            NetatmoMeasurementSeries {
                status: NetatmoMeasurementStatus::Invalid,
                body: NetatmoMeasurementBody::Null,
            },
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
        ];
        let status_failure =
            super::normalize_netatmo_series_with_diagnostics(&invalid_status, window())
                .expect_err("unsuccessful response must be rejected");
        assert_eq!(
            status_failure.message(),
            "Netatmo measure response is not successful"
        );

        let invalid_body = [
            series(NetatmoMeasurementBody::Invalid),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
        ];
        let body_failure =
            super::normalize_netatmo_series_with_diagnostics(&invalid_body, window())
                .expect_err("non-object body must be rejected");
        assert_eq!(
            body_failure.message(),
            "Netatmo measure body must be an object"
        );

        let invalid_timestamp = [
            series(NetatmoMeasurementBody::Rows(vec![row(
                0,
                vec![PublicStationNumber::Missing, PublicStationNumber::Missing],
            )])),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
        ];
        let timestamp_failure =
            super::normalize_netatmo_series_with_diagnostics(&invalid_timestamp, window())
                .expect_err("invalid timestamp must be rejected before padding checks");
        assert_eq!(
            timestamp_failure.message(),
            "Netatmo measure timestamp is invalid"
        );

        let malformed_row = [
            series(NetatmoMeasurementBody::Rows(vec![NetatmoMeasurementRow {
                epoch_seconds: START_MS / 1_000,
                values: NetatmoMeasurementValues::Invalid {
                    epoch_text: (START_MS / 1_000).to_string(),
                },
            }])),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
        ];
        let row_failure =
            super::normalize_netatmo_series_with_diagnostics(&malformed_row, window())
                .expect_err("non-array row must be rejected");
        assert_eq!(
            row_failure.message(),
            format!("Netatmo measure {} must be an array", START_MS / 1_000)
        );

        let short_row = [
            series(NetatmoMeasurementBody::Rows(vec![row(
                START_MS / 1_000,
                vec![PublicStationNumber::Number(12.0)],
            )])),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
        ];
        let short_failure = super::normalize_netatmo_series_with_diagnostics(&short_row, window())
            .expect_err("short row must be rejected");
        assert_eq!(
            short_failure.message(),
            "Netatmo measure row is shorter than requested types"
        );

        let malformed_number = [
            series(NetatmoMeasurementBody::Rows(vec![row(
                START_MS / 1_000,
                vec![PublicStationNumber::Invalid, PublicStationNumber::Missing],
            )])),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
            series(NetatmoMeasurementBody::Null),
        ];
        let number_failure =
            super::normalize_netatmo_series_with_diagnostics(&malformed_number, window())
                .expect_err("non-finite temperature must be rejected");
        assert_eq!(
            number_failure.message(),
            "temperatureC must be a finite number or null"
        );

        let legacy = normalize_netatmo_series(&malformed_row, window())
            .expect_err("legacy wrapper must retain invalid-payload classification");
        assert_eq!(
            (legacy.attempts, legacy.code.as_str()),
            (0, "invalid_payload")
        );
    }
}
