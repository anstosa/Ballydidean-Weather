//! Native versioned widget projection from the filtered forecast JSON boundary.

use jiff::{Span, Timestamp, civil::Date, tz::TimeZone};
use weather_contracts as c;
use weather_edge::{
    AdmittedWidgetForecastReceivedAt, AdmittedWidgetForecastSnapshotJson, ArchiveWirePrimitives,
    WidgetForecastAdjustmentDeadline, WidgetForecastCanonicalInstant,
    WidgetForecastGenericAdjustment, WidgetForecastIndependentAdjustment,
    WidgetForecastIndependentAdjustmentSource, WidgetForecastIndependentSourceEpochs,
    WidgetForecastProjectionError, WidgetForecastRainAdjustment, WidgetForecastRecordInput,
    WidgetForecastRecordMetrics, WidgetForecastSnapshotProjector, WidgetForecastSourceEpochs,
    WidgetForecastV1Epochs, WidgetForecastV3Boundary, WidgetForecastV3CalendarBoundary,
    WidgetForecastV3InputRecord, WidgetForecastVersion, build_widget_forecast_v3_input,
    project_widget_forecast_v1_input_hour, project_widget_forecast_v2_input_hour,
    summarize_widget_forecast_v2_status,
};

use crate::{
    NativeWirePrimitives, maintenance_canonical_json::field,
    native_open_meteo_values::load_named_timezone,
    native_widget_forecast_admission::admit_native_widget_forecast,
};

const HOUR_MS: i64 = 3_600_000;
const SITE_TIMEZONE: &str = "America/Los_Angeles";
const SITE_LATITUDE: f64 = c::WidgetForecastSnapshotSiteLatitudeLiteral::WIRE_VALUE;
const SITE_LONGITUDE: f64 = c::WidgetForecastSnapshotSiteLongitudeLiteral::WIRE_VALUE;

/// Closed native projection failures hidden behind the public widget 502 boundary.
#[derive(Debug)]
pub enum NativeWidgetProjectionError {
    InvalidForecast,
    Projection(WidgetForecastProjectionError),
    Encoding,
}

impl From<WidgetForecastProjectionError> for NativeWidgetProjectionError {
    /// Retain the typed projector's closed failure without flattening its semantics.
    fn from(value: WidgetForecastProjectionError) -> Self {
        Self::Projection(value)
    }
}

/// Zero-state production projector for all three public widget contracts.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWidgetForecastProjection;

/// One decoded response boundary containing only values the versioned projection consumes.
struct ProjectionInput {
    generated: WidgetForecastCanonicalInstant,
    received: WidgetForecastCanonicalInstant,
    generic_deadline: WidgetForecastAdjustmentDeadline,
    temperature_deadline: WidgetForecastAdjustmentDeadline,
    records: Vec<DecodedRecord>,
}

/// One complete generated API row paired with all prevalidated projection clocks.
struct DecodedRecord {
    record: WidgetForecastRecordInput,
    epochs: WidgetForecastV1Epochs,
}

impl WidgetForecastSnapshotProjector<c::JsonValue> for NativeWidgetForecastProjection {
    type Error = NativeWidgetProjectionError;

    /// Decode the genuine filtered API response and delegate field selection to weather-edge.
    fn project_snapshot(
        &self,
        forecast: &c::JsonValue,
        received_at: &AdmittedWidgetForecastReceivedAt,
        version: WidgetForecastVersion,
    ) -> Result<AdmittedWidgetForecastSnapshotJson, Self::Error> {
        let admitted = admit_native_widget_forecast(forecast)
            .map_err(|_| NativeWidgetProjectionError::InvalidForecast)?;
        let input = decode_projection_input(&admitted.response, received_at.as_str())?;
        let snapshot = match version {
            WidgetForecastVersion::V1 => project_v1(&input)?,
            WidgetForecastVersion::V2 => project_v2(&input)?,
            WidgetForecastVersion::V3 => project_v3(&input)?,
        };
        let mut content = NativeWirePrimitives
            .encode_ordered_json(&snapshot)
            .ok_or(NativeWidgetProjectionError::Encoding)?;
        // remove only the shared archive codec's framing newline
        if content.pop() != Some(b'\n') {
            return Err(NativeWidgetProjectionError::Encoding);
        }
        Ok(AdmittedWidgetForecastSnapshotJson::admitted(
            version, content,
        ))
    }
}

/// Decode one strict generated forecast response and normalize every projection timestamp.
fn decode_projection_input(
    forecast: &c::JsonValue,
    received_at: &str,
) -> Result<ProjectionInput, NativeWidgetProjectionError> {
    validate_site(field(forecast, "site").ok_or(NativeWidgetProjectionError::InvalidForecast)?)?;
    let generated = canonical_instant(text(forecast, "generatedAt")?)?;
    let received = canonical_instant(received_at.to_owned())?;
    // reject an upstream anchor that the edge could not yet have received
    if generated.epoch_ms > received.epoch_ms {
        return Err(NativeWidgetProjectionError::InvalidForecast);
    }
    let generic_deadline = decode_deadline(forecast, "adjustmentRuntime")?;
    let temperature_deadline = decode_deadline(forecast, "temperatureAdjustmentRuntime")?;
    let c::JsonValue::Array(rows) =
        field(forecast, "data").ok_or(NativeWidgetProjectionError::InvalidForecast)?
    else {
        return Err(NativeWidgetProjectionError::InvalidForecast);
    };
    let mut records = Vec::with_capacity(rows.len());
    // admit every visible row exactly once before version-specific calendar indexing
    for row in rows {
        records.push(decode_record(row, generated.epoch_ms)?);
    }
    Ok(ProjectionInput {
        generated,
        received,
        generic_deadline,
        temperature_deadline,
        records,
    })
}

/// Require the fixed product site that owns the widget contracts.
fn validate_site(value: &c::JsonValue) -> Result<(), NativeWidgetProjectionError> {
    let valid = text(value, "slug")? == c::WidgetForecastSnapshotSiteSlugLiteral::WIRE_VALUE
        && text(value, "timezone")? == c::WidgetForecastSnapshotSiteTimezoneLiteral::WIRE_VALUE
        && text(value, "name")? == c::WidgetForecastSnapshotSiteNameLiteral::WIRE_VALUE
        && nullable_number_option(value, "latitude")? == Some(SITE_LATITUDE)
        && nullable_number_option(value, "longitude")? == Some(SITE_LONGITUDE);
    valid
        .then_some(())
        .ok_or(NativeWidgetProjectionError::InvalidForecast)
}

/// Decode absent, permanent, and finite runtime expiry states without conflating them.
fn decode_deadline(
    response: &c::JsonValue,
    runtime_name: &str,
) -> Result<WidgetForecastAdjustmentDeadline, NativeWidgetProjectionError> {
    let Some(runtime) = field(response, runtime_name) else {
        return Ok(WidgetForecastAdjustmentDeadline::Unavailable);
    };
    let Some(expiry) = object_field(runtime, "expiresAt") else {
        return Ok(WidgetForecastAdjustmentDeadline::Unavailable);
    };
    match expiry {
        c::JsonValue::Null => Ok(WidgetForecastAdjustmentDeadline::Permanent),
        c::JsonValue::String(_) => Ok(WidgetForecastAdjustmentDeadline::AtEpochMilliseconds(
            canonical_instant(string_value(expiry)?)?.epoch_ms,
        )),
        _ => Err(NativeWidgetProjectionError::InvalidForecast),
    }
}

/// Decode only genuine fields consumed by the existing widget projection algorithm.
fn decode_record(
    value: &c::JsonValue,
    generated_at_epoch_ms: i64,
) -> Result<DecodedRecord, NativeWidgetProjectionError> {
    let valid = canonical_instant(text(value, "validAt")?)?;
    let received = canonical_instant(text(value, "receivedAt")?)?;
    let product_run = nullable_canonical_option(value, "productRunAt")?;
    let metrics = required(value, "metrics")?;
    let provenance = required(value, "provenance")?;
    let rain_adjustment = decode_rain_adjustment(object_field(value, "rainAdjustment"))?;
    let temperature_adjustment =
        decode_temperature_adjustment(object_field(value, "temperatureAdjustment"))?;
    let generic_adjustment = decode_generic_adjustment(object_field(value, "adjustment"))?;
    let rain_source = rain_adjustment
        .source
        .as_ref()
        .map(|source| {
            Ok::<_, NativeWidgetProjectionError>(WidgetForecastIndependentSourceEpochs {
                received_at_epoch_ms: canonical_instant(source.first_received_at.clone())?.epoch_ms,
                product_run_at_epoch_ms: Some(
                    canonical_instant(source.run_initialized_at.clone())?.epoch_ms,
                ),
            })
        })
        .transpose()?;
    let rain_decision_at_epoch_ms = rain_adjustment
        .decision_at
        .as_ref()
        .map(|value| canonical_instant(value.clone()).map(|value| value.epoch_ms))
        .transpose()?;
    let temperature_source = temperature_adjustment
        .source
        .as_ref()
        .map(|source| {
            Ok::<_, NativeWidgetProjectionError>(WidgetForecastIndependentSourceEpochs {
                received_at_epoch_ms: canonical_instant(source.first_received_at.clone())?.epoch_ms,
                product_run_at_epoch_ms: Some(
                    canonical_instant(source.run_initialized_at.clone())?.epoch_ms,
                ),
            })
        })
        .transpose()?;
    let product_run_at_epoch_ms = product_run
        .as_ref()
        .map(|value| canonical_instant(value.clone()).map(|value| value.epoch_ms))
        .transpose()?;
    let record = WidgetForecastRecordInput {
        metrics: WidgetForecastRecordMetrics {
            cloud_cover_percent: nullable_number_option(metrics, "cloudCoverPercent")?,
            precipitation_mm: nullable_number_option(metrics, "precipitationMm")?,
            precipitation_rate_mm_per_hour: nullable_number_option(
                metrics,
                "precipitationRateMmPerHour",
            )?,
            temperature_c: nullable_number_option(metrics, "temperatureC")?,
            wind_speed_mps: nullable_number_option(metrics, "windSpeedMps")?,
        },
        product_run_at: product_run,
        received_at: received.value,
        valid_at: valid.value,
        is_forecast: text(provenance, "sourceKind")? == "forecast",
        rain_adjustment,
        generic_adjustment,
        temperature_adjustment,
    };
    Ok(DecodedRecord {
        epochs: WidgetForecastV1Epochs {
            start_epoch_ms: valid.epoch_ms,
            raw_source: WidgetForecastSourceEpochs {
                generated_at_epoch_ms,
                received_at_epoch_ms: received.epoch_ms,
                product_run_at_epoch_ms,
            },
            rain_source,
            temperature_source,
            rain_decision_at_epoch_ms,
        },
        record,
    })
}

/// Decode a filtered rain decision, including the independent source only while active.
fn decode_rain_adjustment(
    value: Option<&c::JsonValue>,
) -> Result<WidgetForecastRainAdjustment, NativeWidgetProjectionError> {
    let Some(value) = value else {
        return Ok(WidgetForecastRainAdjustment {
            active: false,
            corrected_value: None,
            decision_at: None,
            source: None,
        });
    };
    let active = text(value, "state")? == "active";
    // disabled and raw-fallback decisions cannot contribute selected values
    if !active {
        return Ok(WidgetForecastRainAdjustment {
            active: false,
            corrected_value: None,
            decision_at: None,
            source: None,
        });
    }
    let source = required(value, "sourceForecast")?;
    let source = decode_independent_source(source)?;
    Ok(WidgetForecastRainAdjustment {
        active,
        corrected_value: nullable_number_option(value, "correctedPrecipitationMm")?,
        decision_at: Some(
            canonical_instant(text(required(value, "sourceForecast")?, "decisionAt")?)?.value,
        ),
        source: Some(source),
    })
}

/// Decode a filtered temperature decision without representing its private reason code.
fn decode_temperature_adjustment(
    value: Option<&c::JsonValue>,
) -> Result<WidgetForecastIndependentAdjustment, NativeWidgetProjectionError> {
    let Some(value) = value else {
        return Ok(WidgetForecastIndependentAdjustment {
            active: false,
            corrected_value: None,
            source: None,
        });
    };
    let active = text(value, "state")? == "active";
    // normal admin-disabled decisions remain truthful inactive widget inputs
    if !active {
        return Ok(WidgetForecastIndependentAdjustment {
            active: false,
            corrected_value: None,
            source: None,
        });
    }
    Ok(WidgetForecastIndependentAdjustment {
        active,
        corrected_value: nullable_number_option(value, "correctedTemperatureC")?,
        source: Some(decode_independent_source(required(
            value,
            "sourceForecast",
        )?)?),
    })
}

/// Decode the shared source clocks used by independent rain and temperature corrections.
fn decode_independent_source(
    value: &c::JsonValue,
) -> Result<WidgetForecastIndependentAdjustmentSource, NativeWidgetProjectionError> {
    Ok(WidgetForecastIndependentAdjustmentSource {
        run_initialized_at: canonical_instant(text(value, "runInitializedAt")?)?.value,
        first_received_at: canonical_instant(text(value, "firstReceivedAt")?)?.value,
    })
}

/// Decode only generic application markers and selected values used by temperature or wind.
fn decode_generic_adjustment(
    value: Option<&c::JsonValue>,
) -> Result<WidgetForecastGenericAdjustment, NativeWidgetProjectionError> {
    let Some(value) = value else {
        return Ok(inactive_generic_adjustment());
    };
    // every filtered disabled decision is inactive regardless of its private reason spelling
    if text(value, "state")? != "active" {
        return Ok(inactive_generic_adjustment());
    }
    let c::JsonValue::Array(applied) = required(value, "appliedMetrics")? else {
        return Err(NativeWidgetProjectionError::InvalidForecast);
    };
    let temperature_applied = contains_text(applied, "temperatureC")?;
    let wind_speed_applied = contains_text(applied, "windSpeedMps")?;
    let adjusted = required(value, "adjustedMetrics")?;
    Ok(WidgetForecastGenericAdjustment {
        temperature_applied,
        adjusted_temperature_c: optional_number(adjusted, "temperatureC")?,
        wind_speed_applied,
        adjusted_wind_speed_mps: optional_number(adjusted, "windSpeedMps")?,
    })
}

/// Construct the truthful generic inactive state without a synthetic reason.
fn inactive_generic_adjustment() -> WidgetForecastGenericAdjustment {
    WidgetForecastGenericAdjustment {
        temperature_applied: false,
        adjusted_temperature_c: None,
        wind_speed_applied: false,
        adjusted_wind_speed_mps: None,
    }
}

/// Report whether one string array contains an exact metric name.
fn contains_text(
    values: &[c::JsonValue],
    expected: &str,
) -> Result<bool, NativeWidgetProjectionError> {
    let mut found = false;
    // reject malformed metric arrays rather than ignoring a partial active decision
    for value in values {
        found |= string_value(value)? == expected;
    }
    Ok(found)
}

/// Project the local civil-day v1 snapshot with the shared typed hour owner.
fn project_v1(input: &ProjectionInput) -> Result<c::JsonValue, NativeWidgetProjectionError> {
    let calendar = calendar(input.generated.epoch_ms, false)?;
    let rows = indexed_rows(
        input,
        calendar.day_start.epoch_ms,
        calendar.day_end.epoch_ms,
        23,
        25,
    )?;
    let mut hours = Vec::with_capacity(rows.len());
    // project every real UTC interval through weather-edge's v1 semantics
    for (index, row) in rows.into_iter().enumerate() {
        let start_epoch_ms = hour_at(calendar.day_start.epoch_ms, index)?;
        let (record, epochs) = row_parts(input, row, start_epoch_ms);
        hours.push(project_widget_forecast_v1_input_hour(
            record,
            utc(start_epoch_ms)?,
            epochs,
            input.generic_deadline,
            input.temperature_deadline,
            &NativeWirePrimitives,
        )?);
    }
    let status = summarize_v1_status(&hours);
    Ok(snapshot_value(
        WidgetForecastVersion::V1,
        &calendar,
        &input.generated.value,
        &input.received.value,
        status,
        hours.iter().map(v1_hour_value).collect(),
    ))
}

/// Project the local civil-day v2 snapshot using the same v1 rows and edge extension.
fn project_v2(input: &ProjectionInput) -> Result<c::JsonValue, NativeWidgetProjectionError> {
    let calendar = calendar(input.generated.epoch_ms, false)?;
    let rows = indexed_rows(
        input,
        calendar.day_start.epoch_ms,
        calendar.day_end.epoch_ms,
        23,
        25,
    )?;
    let mut hours = Vec::with_capacity(rows.len());
    // extend every shared base hour with the exact v2 condition fields
    for (index, row) in rows.into_iter().enumerate() {
        let start_epoch_ms = hour_at(calendar.day_start.epoch_ms, index)?;
        let (record, epochs) = row_parts(input, row, start_epoch_ms);
        let base = project_widget_forecast_v1_input_hour(
            record,
            utc(start_epoch_ms)?,
            epochs,
            input.generic_deadline,
            input.temperature_deadline,
            &NativeWirePrimitives,
        )?;
        hours.push(project_widget_forecast_v2_input_hour(
            base,
            record,
            epochs.raw_source,
            input.generic_deadline,
            &NativeWirePrimitives,
        )?);
    }
    let status = summarize_widget_forecast_v2_status(&hours);
    Ok(snapshot_value(
        WidgetForecastVersion::V2,
        &calendar,
        &input.generated.value,
        &input.received.value,
        status,
        hours.iter().map(v2_hour_value).collect(),
    ))
}

/// Project the overnight v3 window through weather-edge's complete typed builder.
fn project_v3(input: &ProjectionInput) -> Result<c::JsonValue, NativeWidgetProjectionError> {
    let calendar = calendar(input.generated.epoch_ms, true)?;
    let records: Vec<_> = input
        .records
        .iter()
        .map(|row| WidgetForecastV3InputRecord {
            record: &row.record,
            epochs: row.epochs,
            generic_deadline: input.generic_deadline,
            temperature_deadline: input.temperature_deadline,
        })
        .collect();
    let snapshot = build_widget_forecast_v3_input(
        WidgetForecastV3Boundary {
            generated_at: input.generated.clone(),
            received_at: input.received.clone(),
            calendar: WidgetForecastV3CalendarBoundary {
                date: calendar.date.clone(),
                day_start: calendar.day_start.clone(),
                day_end: calendar.day_end.clone(),
                cutoff: calendar.cutoff.clone(),
                anchor_start_epoch_ms: calendar.anchor_start_epoch_ms,
                overnight_end: calendar.overnight_end.clone(),
                sunset: calendar.sunset.clone(),
            },
        },
        &records,
        &NativeWirePrimitives,
    )?;
    Ok(snapshot_value(
        WidgetForecastVersion::V3,
        &calendar,
        &snapshot.generatedAt,
        &snapshot.receivedAt,
        snapshot.status,
        snapshot.hours.iter().map(v2_hour_value).collect(),
    ))
}

/// Internal calendar shared by the version-specific response encoders.
struct ProjectionCalendar {
    date: String,
    day_start: WidgetForecastCanonicalInstant,
    day_end: WidgetForecastCanonicalInstant,
    cutoff: WidgetForecastCanonicalInstant,
    anchor_start_epoch_ms: i64,
    overnight_end: WidgetForecastCanonicalInstant,
    sunset: Option<WidgetForecastCanonicalInstant>,
}

/// Resolve the fixed site civil calendar and exact NOAA sunset.
fn calendar(
    generated_at_epoch_ms: i64,
    overnight: bool,
) -> Result<ProjectionCalendar, NativeWidgetProjectionError> {
    let zone =
        load_named_timezone(SITE_TIMEZONE).ok_or(NativeWidgetProjectionError::InvalidForecast)?;
    let generated = Timestamp::from_millisecond(generated_at_epoch_ms)
        .map_err(|_| NativeWidgetProjectionError::InvalidForecast)?;
    let local = zone.to_datetime(generated);
    let mut date = local.date();
    // v3 keeps the previous local date active until its actual 07:00 instant
    if overnight && local.hour() < 7 {
        date = add_days(date, -1)?;
    }
    let next_date = add_days(date, 1)?;
    let day_start = wall_clock(date, 0, &zone)?;
    let day_end = wall_clock(next_date, 0, &zone)?;
    let cutoff = wall_clock(date, 20, &zone)?;
    let anchor = wall_clock(date, 7, &zone)?;
    let overnight_end = wall_clock(next_date, 7, &zone)?;
    let sunset = solar_sunset(date)?;
    Ok(ProjectionCalendar {
        date: date.to_string(),
        day_start,
        day_end,
        cutoff,
        anchor_start_epoch_ms: anchor.epoch_ms,
        overnight_end,
        sunset,
    })
}

/// Add whole Gregorian days without assuming a fixed UTC duration.
fn add_days(date: Date, days: i64) -> Result<Date, NativeWidgetProjectionError> {
    let span = Span::new()
        .try_days(days)
        .map_err(|_| NativeWidgetProjectionError::InvalidForecast)?;
    date.checked_add(span)
        .map_err(|_| NativeWidgetProjectionError::InvalidForecast)
}

/// Resolve one unique site wall clock and reject folds or gaps.
fn wall_clock(
    date: Date,
    hour: i8,
    zone: &TimeZone,
) -> Result<WidgetForecastCanonicalInstant, NativeWidgetProjectionError> {
    let requested = date.at(hour, 0, 0, 0);
    let earlier = zone
        .to_ambiguous_timestamp(requested)
        .earlier()
        .map_err(|_| NativeWidgetProjectionError::InvalidForecast)?;
    let later = zone
        .to_ambiguous_timestamp(requested)
        .later()
        .map_err(|_| NativeWidgetProjectionError::InvalidForecast)?;
    // reject a non-unique or normalized boundary instead of inventing a site instant
    if earlier != later || zone.to_datetime(earlier) != requested {
        return Err(NativeWidgetProjectionError::InvalidForecast);
    }
    canonical_epoch(earlier.as_millisecond())
}

/// Calculate sunset with the exact three-pass NOAA equation used by the TS owner.
fn solar_sunset(
    date: Date,
) -> Result<Option<WidgetForecastCanonicalInstant>, NativeWidgetProjectionError> {
    let utc_midnight = TimeZone::UTC
        .to_timestamp(date.at(0, 0, 0, 0))
        .map_err(|_| NativeWidgetProjectionError::InvalidForecast)?
        .as_millisecond();
    let solar_noon = utc_midnight as f64 + (720.0 - 4.0 * SITE_LONGITUDE) * 60_000.0;
    let zone =
        load_named_timezone(SITE_TIMEZONE).ok_or(NativeWidgetProjectionError::InvalidForecast)?;
    let local_noon = wall_clock(date, 12, &zone)?.epoch_ms as f64;
    let day_offset = js_round((local_noon - solar_noon) / 86_400_000.0);
    let solar_day = utc_midnight as f64 + day_offset * 86_400_000.0;
    let Some(epoch) = solar_event(solar_day, SITE_LATITUDE, SITE_LONGITUDE, -0.833) else {
        return Ok(None);
    };
    Ok(Some(canonical_epoch(epoch as i64)?))
}

/// Preserve JavaScript Math.round's tie direction for the solar-day alignment.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// Refine one descending solar crossing to the source's sub-minute precision.
fn solar_event(midnight: f64, latitude: f64, longitude: f64, altitude: f64) -> Option<f64> {
    let radians = std::f64::consts::PI / 180.0;
    let latitude_radians = latitude * radians;
    let mut instant = midnight + 12.0 * HOUR_MS as f64;
    // retain the original fixed three refinement passes
    for _ in 0..3 {
        let century = (instant / 86_400_000.0 + 2_440_587.5 - 2_451_545.0) / 36_525.0;
        let mean_longitude =
            (280.46646 + century * (36_000.769_83 + 0.0003032 * century)) * radians;
        let mean_anomaly = (357.52911 + century * (35_999.050_29 - 0.0001537 * century)) * radians;
        let eccentricity = 0.016708634 - century * (0.000042037 + 0.0000001267 * century);
        let center = mean_anomaly.sin() * (1.914602 - century * (0.004817 + 0.000014 * century))
            + (2.0 * mean_anomaly).sin() * (0.019993 - 0.000101 * century)
            + (3.0 * mean_anomaly).sin() * 0.000289;
        let omega = (125.04 - 1934.136 * century) * radians;
        let apparent_longitude =
            mean_longitude + (center - 0.00569 - 0.00478 * omega.sin()) * radians;
        let obliquity = (23.0
            + (26.0
                + (21.448 - century * (46.815 + century * (0.00059 - 0.001813 * century))) / 60.0)
                / 60.0
            + 0.00256 * omega.cos())
            * radians;
        let declination = (obliquity.sin() * apparent_longitude.sin()).asin();
        let tangent_squared = (obliquity / 2.0).tan().powi(2);
        let equation_minutes = 4.0 / radians
            * (tangent_squared * (2.0 * mean_longitude).sin()
                - 2.0 * eccentricity * mean_anomaly.sin()
                + 4.0
                    * eccentricity
                    * tangent_squared
                    * mean_anomaly.sin()
                    * (2.0 * mean_longitude).cos()
                - 0.5 * tangent_squared.powi(2) * (4.0 * mean_longitude).sin()
                - 1.25 * eccentricity.powi(2) * (2.0 * mean_anomaly).sin());
        let cosine_hour_angle = ((altitude * radians).sin()
            - latitude_radians.sin() * declination.sin())
            / (latitude_radians.cos() * declination.cos());
        // preserve absent polar crossings without inventing a clock time
        if !cosine_hour_angle.is_finite() || cosine_hour_angle.abs() > 1.0 {
            return None;
        }
        let hour_angle = cosine_hour_angle.acos() / radians;
        instant =
            midnight + (720.0 - 4.0 * longitude + 4.0 * hour_angle - equation_minutes) * 60_000.0;
    }
    Some(instant)
}

/// Index supplied rows against one exact real-hour grid.
fn indexed_rows(
    input: &ProjectionInput,
    start: i64,
    end: i64,
    minimum: usize,
    maximum: usize,
) -> Result<Vec<Option<usize>>, NativeWidgetProjectionError> {
    let duration = end
        .checked_sub(start)
        .ok_or(NativeWidgetProjectionError::InvalidForecast)?;
    // require a whole-hour reviewed civil window
    if duration % HOUR_MS != 0 {
        return Err(NativeWidgetProjectionError::InvalidForecast);
    }
    let count = usize::try_from(duration / HOUR_MS)
        .map_err(|_| NativeWidgetProjectionError::InvalidForecast)?;
    // retain only the source's reviewed DST capacities
    if !(minimum..=maximum).contains(&count) {
        return Err(NativeWidgetProjectionError::InvalidForecast);
    }
    let mut rows = vec![None; count];
    // reject nonforecast, duplicate, off-grid, and foreign rows before projection
    for (index, row) in input.records.iter().enumerate() {
        if !row.record.is_forecast {
            return Err(NativeWidgetProjectionError::InvalidForecast);
        }
        let offset = row
            .epochs
            .start_epoch_ms
            .checked_sub(start)
            .ok_or(NativeWidgetProjectionError::InvalidForecast)?;
        if offset < 0 || offset % HOUR_MS != 0 {
            return Err(NativeWidgetProjectionError::InvalidForecast);
        }
        let slot = usize::try_from(offset / HOUR_MS)
            .ok()
            .and_then(|offset| rows.get_mut(offset))
            .ok_or(NativeWidgetProjectionError::InvalidForecast)?;
        if slot.replace(index).is_some() {
            return Err(NativeWidgetProjectionError::InvalidForecast);
        }
    }
    Ok(rows)
}

/// Pair one optional indexed record with the missing-row epochs expected by edge.
fn row_parts(
    input: &ProjectionInput,
    row: Option<usize>,
    start_epoch_ms: i64,
) -> (Option<&WidgetForecastRecordInput>, WidgetForecastV1Epochs) {
    row.map_or_else(
        || {
            (
                None,
                WidgetForecastV1Epochs {
                    start_epoch_ms,
                    raw_source: WidgetForecastSourceEpochs {
                        generated_at_epoch_ms: input.generated.epoch_ms,
                        received_at_epoch_ms: input.generated.epoch_ms,
                        product_run_at_epoch_ms: None,
                    },
                    rain_source: None,
                    temperature_source: None,
                    rain_decision_at_epoch_ms: None,
                },
            )
        },
        |index| {
            let row = &input.records[index];
            (Some(&row.record), row.epochs)
        },
    )
}

/// Resolve one checked hourly offset.
fn hour_at(start: i64, index: usize) -> Result<i64, NativeWidgetProjectionError> {
    let offset = i64::try_from(index)
        .ok()
        .and_then(|index| index.checked_mul(HOUR_MS))
        .ok_or(NativeWidgetProjectionError::InvalidForecast)?;
    start
        .checked_add(offset)
        .ok_or(NativeWidgetProjectionError::InvalidForecast)
}

/// Summarize the two v1 fields with the source's all-equal rule.
fn summarize_v1_status(hours: &[c::WidgetForecastHour]) -> c::WidgetForecastStatus {
    let mut modes = hours
        .iter()
        .flat_map(|hour| [hour.temperatureC.mode, hour.rainMmPerHour.mode]);
    let Some(first) = modes.next() else {
        return c::WidgetForecastStatus::Mixed;
    };
    // preserve one mode only when every field agrees
    if modes.all(|mode| mode == first) {
        return status_for_mode(first);
    }
    c::WidgetForecastStatus::Mixed
}

/// Map a field mode to its same-name snapshot status.
fn status_for_mode(mode: c::WidgetForecastMode) -> c::WidgetForecastStatus {
    match mode {
        c::WidgetForecastMode::Adjusted => c::WidgetForecastStatus::Adjusted,
        c::WidgetForecastMode::Raw => c::WidgetForecastStatus::Raw,
        c::WidgetForecastMode::Unavailable => c::WidgetForecastStatus::Unavailable,
    }
}

/// Encode one complete snapshot in the original TypeScript insertion order.
fn snapshot_value(
    version: WidgetForecastVersion,
    calendar: &ProjectionCalendar,
    generated_at: &str,
    received_at: &str,
    status: c::WidgetForecastStatus,
    hours: Vec<c::JsonValue>,
) -> c::JsonValue {
    let mut calendar_members = vec![
        member("cutoff", string(&calendar.cutoff.value)),
        member("date", string(&calendar.date)),
        member("dayEnd", string(&calendar.day_end.value)),
        member("dayStart", string(&calendar.day_start.value)),
    ];
    // v3 inserts overnightEnd before sunset in its literal source object
    if version == WidgetForecastVersion::V3 {
        calendar_members.push(member(
            "overnightEnd",
            string(&calendar.overnight_end.value),
        ));
    }
    calendar_members.push(member(
        "sunset",
        calendar
            .sunset
            .as_ref()
            .map_or(c::JsonValue::Null, |value| string(&value.value)),
    ));
    c::JsonValue::Object(vec![
        member(
            "attribution",
            c::JsonValue::Object(vec![
                member(
                    "label",
                    string(c::WidgetForecastSnapshotAttributionLabelLiteral::WIRE_VALUE),
                ),
                member(
                    "licenseUrl",
                    string(c::WidgetForecastSnapshotAttributionLicenseUrlLiteral::WIRE_VALUE),
                ),
                member(
                    "providerUrl",
                    string(c::WidgetForecastSnapshotAttributionProviderUrlLiteral::WIRE_VALUE),
                ),
            ]),
        ),
        member("calendar", c::JsonValue::Object(calendar_members)),
        member("generatedAt", string(generated_at)),
        member("hours", c::JsonValue::Array(hours)),
        member("receivedAt", string(received_at)),
        member(
            "schemaVersion",
            string(match version {
                WidgetForecastVersion::V1 => {
                    c::WidgetForecastSnapshotSchemaVersionLiteral::WIRE_VALUE
                }
                WidgetForecastVersion::V2 => {
                    c::WidgetForecastV2SnapshotSchemaVersionLiteral::WIRE_VALUE
                }
                WidgetForecastVersion::V3 => {
                    c::WidgetForecastV3SnapshotSchemaVersionLiteral::WIRE_VALUE
                }
            }),
        ),
        member(
            "site",
            c::JsonValue::Object(vec![
                member("latitude", c::JsonValue::Number(SITE_LATITUDE)),
                member("longitude", c::JsonValue::Number(SITE_LONGITUDE)),
                member(
                    "name",
                    string(c::WidgetForecastSnapshotSiteNameLiteral::WIRE_VALUE),
                ),
                member(
                    "slug",
                    string(c::WidgetForecastSnapshotSiteSlugLiteral::WIRE_VALUE),
                ),
                member(
                    "timezone",
                    string(c::WidgetForecastSnapshotSiteTimezoneLiteral::WIRE_VALUE),
                ),
            ]),
        ),
        member("status", string(status_text(status))),
    ])
}

/// Encode one v1 hour without reordering generated contract members.
fn v1_hour_value(hour: &c::WidgetForecastHour) -> c::JsonValue {
    c::JsonValue::Object(vec![
        member("end", string(&hour.end)),
        member("rainMmPerHour", field_value(&hour.rainMmPerHour)),
        member("start", string(&hour.start)),
        member("temperatureC", field_value(&hour.temperatureC)),
    ])
}

/// Encode one v2/v3 hour with the extension fields appended after v1 members.
fn v2_hour_value(hour: &c::WidgetForecastV2Hour) -> c::JsonValue {
    c::JsonValue::Object(vec![
        member("end", string(&hour.end)),
        member("rainMmPerHour", field_value(&hour.rainMmPerHour)),
        member("start", string(&hour.start)),
        member("temperatureC", field_value(&hour.temperatureC)),
        member("cloudCoverPercent", field_value(&hour.cloudCoverPercent)),
        member("windSpeedMps", field_value(&hour.windSpeedMps)),
    ])
}

/// Encode one widget field with exact nullable source and number semantics.
fn field_value(value: &c::WidgetForecastField) -> c::JsonValue {
    c::JsonValue::Object(vec![
        member("mode", string(mode_text(value.mode))),
        member("raw", nullable_number_json(&value.raw)),
        member("rawSource", nullable_source_json(&value.rawSource)),
        member("reason", string(reason_text(value.reason))),
        member("selected", nullable_number_json(&value.selected)),
        member(
            "selectedSource",
            nullable_source_json(&value.selectedSource),
        ),
        member("selectedUntil", nullable_text_json(&value.selectedUntil)),
    ])
}

/// Encode one nullable widget source with its fixed member order.
fn nullable_source_json(value: &c::Nullable<c::WidgetForecastSource>) -> c::JsonValue {
    match value {
        c::Nullable::Null => c::JsonValue::Null,
        c::Nullable::Value(value) => c::JsonValue::Object(vec![
            member("runAt", nullable_text_json(&value.runAt)),
            member("receivedAt", string(&value.receivedAt)),
        ]),
    }
}

/// Encode one nullable number without a sentinel.
fn nullable_number_json(value: &c::Nullable<f64>) -> c::JsonValue {
    match value {
        c::Nullable::Null => c::JsonValue::Null,
        c::Nullable::Value(value) => c::JsonValue::Number(*value),
    }
}

/// Encode one nullable string without changing UTF-16 content.
fn nullable_text_json(value: &c::Nullable<String>) -> c::JsonValue {
    match value {
        c::Nullable::Null => c::JsonValue::Null,
        c::Nullable::Value(value) => string(value),
    }
}

/// Return one field mode wire spelling.
fn mode_text(value: c::WidgetForecastMode) -> &'static str {
    wire_name(value, c::WidgetForecastMode::WIRE_VALUES)
}

/// Return one field reason wire spelling.
fn reason_text(value: c::WidgetForecastReason) -> &'static str {
    wire_name(value, c::WidgetForecastReason::WIRE_VALUES)
}

/// Return one snapshot status wire spelling.
fn status_text(value: c::WidgetForecastStatus) -> &'static str {
    wire_name(value, c::WidgetForecastStatus::WIRE_VALUES)
}

/// Look up the generated wire name for one copyable enum value.
fn wire_name<T: Copy + PartialEq>(value: T, values: &'static [(&'static str, T)]) -> &'static str {
    values
        .iter()
        .find_map(|(name, candidate)| (*candidate == value).then_some(*name))
        .expect("generated wire enum is exhaustive")
}

/// Parse and normalize one explicit-zone instant.
fn canonical_instant(
    value: String,
) -> Result<WidgetForecastCanonicalInstant, NativeWidgetProjectionError> {
    let epoch_ms = NativeWirePrimitives
        .parse_explicit_utc_milliseconds(&value)
        .ok_or(NativeWidgetProjectionError::InvalidForecast)?;
    canonical_epoch(epoch_ms)
}

/// Pair one admitted epoch with its canonical UTC millisecond spelling.
fn canonical_epoch(
    epoch_ms: i64,
) -> Result<WidgetForecastCanonicalInstant, NativeWidgetProjectionError> {
    Ok(WidgetForecastCanonicalInstant {
        value: utc(epoch_ms)?,
        epoch_ms,
    })
}

/// Format one supported epoch with the approved Node-compatible primitive.
fn utc(epoch_ms: i64) -> Result<String, NativeWidgetProjectionError> {
    NativeWirePrimitives
        .format_utc_milliseconds(epoch_ms)
        .ok_or(NativeWidgetProjectionError::InvalidForecast)
}

/// Borrow one exact object field while preserving missing/null distinctions.
fn object_field<'a>(value: &'a c::JsonValue, name: &str) -> Option<&'a c::JsonValue> {
    let c::JsonValue::Object(fields) = value else {
        return None;
    };
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
}

/// Require one object member without coercion.
fn required<'a>(
    value: &'a c::JsonValue,
    name: &str,
) -> Result<&'a c::JsonValue, NativeWidgetProjectionError> {
    object_field(value, name).ok_or(NativeWidgetProjectionError::InvalidForecast)
}

/// Decode one required UTF-16 string field.
fn text(value: &c::JsonValue, name: &str) -> Result<String, NativeWidgetProjectionError> {
    string_value(required(value, name)?)
}

/// Decode one direct UTF-16 string value.
fn string_value(value: &c::JsonValue) -> Result<String, NativeWidgetProjectionError> {
    let c::JsonValue::String(units) = value else {
        return Err(NativeWidgetProjectionError::InvalidForecast);
    };
    String::from_utf16(units).map_err(|_| NativeWidgetProjectionError::InvalidForecast)
}

/// Decode and normalize one required nullable explicit-zone timestamp.
fn nullable_canonical_option(
    value: &c::JsonValue,
    name: &str,
) -> Result<Option<String>, NativeWidgetProjectionError> {
    match required(value, name)? {
        c::JsonValue::Null => Ok(None),
        value => canonical_instant(string_value(value)?).map(|value| Some(value.value)),
    }
}

/// Decode one required nullable finite number.
fn nullable_number_option(
    value: &c::JsonValue,
    name: &str,
) -> Result<Option<f64>, NativeWidgetProjectionError> {
    match required(value, name)? {
        c::JsonValue::Null => Ok(None),
        c::JsonValue::Number(value) if value.is_finite() => Ok(Some(*value)),
        _ => Err(NativeWidgetProjectionError::InvalidForecast),
    }
}

/// Decode one optional finite numeric object member.
fn optional_number(
    value: &c::JsonValue,
    name: &str,
) -> Result<Option<f64>, NativeWidgetProjectionError> {
    // metric maps must remain JSON objects even when the selected property is absent
    if !matches!(value, c::JsonValue::Object(_)) {
        return Err(NativeWidgetProjectionError::InvalidForecast);
    }
    match object_field(value, name) {
        None => Ok(None),
        Some(c::JsonValue::Number(value)) if value.is_finite() => Ok(Some(*value)),
        _ => Err(NativeWidgetProjectionError::InvalidForecast),
    }
}

/// Produce one ordered JSON member.
fn member(name: &str, value: c::JsonValue) -> (Vec<u16>, c::JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Produce one UTF-16 JSON string.
fn string(value: &str) -> c::JsonValue {
    c::JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    use super::*;
    use crate::native_widget_forecast_admission::tests::{FixtureMode, fixture};

    const NODE_WIDGET_PROJECTION: &str = r#"
import { readFileSync } from "node:fs";
import { projectWidgetForecast } from "./apps/web/dist/widget-forecast.js";
import { projectWidgetForecastV2 } from "./apps/web/dist/widget-forecast-v2.js";
import { projectWidgetForecastV3 } from "./apps/web/dist/widget-forecast-v3.js";
const input = JSON.parse(readFileSync(0, "utf8"));
process.stdout.write(JSON.stringify({
  v1: projectWidgetForecast(input.forecast, input.receivedAt),
  v2: projectWidgetForecastV2(input.forecast, input.receivedAt),
  v3: projectWidgetForecastV3(input.forecast, input.receivedAt),
}));
"#;

    /// Match every native widget version against the compiled original projection.
    #[test]
    fn native_widget_versions_match_compiled_typescript_from_complete_source() {
        let forecast = fixture(FixtureMode::Active);
        let received_at = "2026-09-12T20:06:00.000Z";
        let actual = c::JsonValue::Object(vec![
            member(
                "v1",
                projected_at(&forecast, received_at, WidgetForecastVersion::V1),
            ),
            member(
                "v2",
                projected_at(&forecast, received_at, WidgetForecastVersion::V2),
            ),
            member(
                "v3",
                projected_at(&forecast, received_at, WidgetForecastVersion::V3),
            ),
        ]);
        let expected = run_node_projection(&forecast, received_at);
        assert_eq!(actual, expected);
    }

    /// Project one active fixture through every version and preserve its real calendar geometry.
    #[test]
    fn native_widget_projection_projects_v1_v2_v3_from_filtered_json() {
        let forecast = fixture(FixtureMode::Active);
        let cases = [
            (WidgetForecastVersion::V1, "weather-widget/v1", 24usize),
            (WidgetForecastVersion::V2, "weather-widget/v2", 24usize),
            (WidgetForecastVersion::V3, "weather-widget/v3", 31usize),
        ];
        // verify each public route from the same genuine filtered response boundary
        for (version, schema, hour_count) in cases {
            let snapshot = projected(&forecast, version);
            assert_eq!(json_text(&snapshot, "schemaVersion"), schema);
            let hours = json_array(&snapshot, "hours");
            assert_eq!(hours.len(), hour_count);
            let hour = &hours[14];
            assert_eq!(json_text(hour, "start"), "2026-09-12T21:00:00.000Z");
            assert_eq!(field_text(hour, "rainMmPerHour", "mode"), "adjusted");
            assert_eq!(field_text(hour, "temperatureC", "mode"), "adjusted");
            // v2 and v3 append the same supplemental condition projection
            if version != WidgetForecastVersion::V1 {
                assert_eq!(field_text(hour, "cloudCoverPercent", "mode"), "raw");
                assert_eq!(field_text(hour, "windSpeedMps", "mode"), "adjusted");
            }
            let calendar = required(&snapshot, "calendar").unwrap();
            assert_eq!(json_text(calendar, "date"), "2026-09-12");
            assert!(matches!(
                required(calendar, "sunset"),
                Ok(c::JsonValue::String(_))
            ));
        }
    }

    /// Keep normal edge-filtered admin-disabled decisions raw without a fabricated reason enum.
    #[test]
    fn native_widget_projection_accepts_admin_disabled_filtered_decisions() {
        let snapshot = projected(&fixture(FixtureMode::Disabled), WidgetForecastVersion::V2);
        let hour = &json_array(&snapshot, "hours")[14];
        // every disabled correction retains the genuine raw forecast selection
        for name in ["rainMmPerHour", "temperatureC", "windSpeedMps"] {
            assert_eq!(field_text(hour, name, "mode"), "raw");
            assert_eq!(field_text(hour, name, "reason"), "raw_forecast");
        }
        assert_eq!(field_number(hour, "rainMmPerHour", "selected"), 1.0);
        assert_eq!(field_number(hour, "temperatureC", "selected"), 10.0);
        assert_eq!(field_number(hour, "windSpeedMps", "selected"), 4.0);
    }

    /// Run the actual public projector and decode its compact snapshot for assertions.
    fn projected(forecast: &c::JsonValue, version: WidgetForecastVersion) -> c::JsonValue {
        projected_at(forecast, "2026-09-12T20:06:00.000Z", version)
    }

    /// Run one native widget version at the supplied admitted edge receipt clock.
    fn projected_at(
        forecast: &c::JsonValue,
        received_at: &str,
        version: WidgetForecastVersion,
    ) -> c::JsonValue {
        let projected = NativeWidgetForecastProjection
            .project_snapshot(
                forecast,
                &AdmittedWidgetForecastReceivedAt::admitted(received_at),
                version,
            )
            .unwrap();
        NativeWirePrimitives
            .decode_json(projected.as_bytes())
            .unwrap()
    }

    /// Run all three compiled TypeScript widget projectors against the same complete envelope.
    fn run_node_projection(forecast: &c::JsonValue, received_at: &str) -> c::JsonValue {
        let input = c::JsonValue::Object(vec![
            member("forecast", forecast.clone()),
            member("receivedAt", string(received_at)),
        ]);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let bytes = NativeWirePrimitives
            .encode_ordered_json(&input)
            .expect("widget source fixture JSON");
        let mut child = Command::new("node")
            .args(["--input-type=module", "--eval", NODE_WIDGET_PROJECTION])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("compiled TypeScript widget oracle");
        child
            .stdin
            .take()
            .expect("Node stdin")
            .write_all(&bytes)
            .expect("write widget source fixture");
        let output = child.wait_with_output().expect("Node widget output");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        NativeWirePrimitives
            .decode_json(&output.stdout)
            .expect("Node widget JSON output")
    }

    /// Read one required string member from a projected object.
    fn json_text(value: &c::JsonValue, name: &str) -> String {
        string_value(required(value, name).unwrap()).unwrap()
    }

    /// Read one required array member from a projected object.
    fn json_array<'a>(value: &'a c::JsonValue, name: &str) -> &'a [c::JsonValue] {
        let c::JsonValue::Array(values) = required(value, name).unwrap() else {
            panic!("projected member is not an array")
        };
        values
    }

    /// Read one nested widget-field string.
    fn field_text(hour: &c::JsonValue, field: &str, name: &str) -> String {
        json_text(required(hour, field).unwrap(), name)
    }

    /// Read one nested widget-field number.
    fn field_number(hour: &c::JsonValue, field: &str, name: &str) -> f64 {
        let c::JsonValue::Number(value) = required(required(hour, field).unwrap(), name).unwrap()
        else {
            panic!("projected member is not a number")
        };
        *value
    }
}
