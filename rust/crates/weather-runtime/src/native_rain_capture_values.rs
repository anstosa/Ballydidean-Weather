//! Actual native retained provider-body decoders for worker rain inference and gauge replay.

use weather_contracts::{JsonValue, Nullable};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_worker::{
    TaskFailure, TempestObservation, TempestObservationEnvelope, TempestResponseType,
};

use crate::{
    NativeWirePrimitives, WorkerRainAdjustmentCaptureValues, WorkerRainForecastHourly,
    WorkerRainForecastPayload, WorkerRainGaugeReplayValues,
    maintenance_canonical_json::{field, text},
};

const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const FORECAST_TIME_COUNT: usize = 49;
const TEMPEST_ROW_COLUMNS: usize = 22;

/// Zero-state native provider body and timestamp owner for retained capture replay.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeRainCaptureValues;

/// Decode one source numeric cell without inventing a missing value.
fn nullable_number(value: &JsonValue) -> Option<Option<f64>> {
    // preserve nullable finite numbers and reject every other provider shape
    match value {
        JsonValue::Null => Some(None),
        JsonValue::Number(value) if value.is_finite() => Some(Some(*value)),
        _ => None,
    }
}

/// Admit only an exact JavaScript-safe integer before narrowing it.
fn integer(value: &JsonValue) -> Option<i64> {
    let JsonValue::Number(value) = value else {
        return None;
    };
    (value.is_finite() && value.fract() == 0.0 && value.abs() <= MAXIMUM_SAFE_INTEGER)
        .then_some(*value as i64)
}

/// Preserve a stable ordinary provider payload failure without leaking raw body bytes.
fn invalid_payload() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "invalid_payload".to_owned(),
    }
}

/// Parse an actual Tempest observation body using the approved native JSON grammar.
pub fn decode_native_tempest_payload(
    bytes: &[u8],
) -> Result<TempestObservationEnvelope, TaskFailure> {
    let value = NativeWirePrimitives
        .decode_json(bytes)
        .ok_or_else(invalid_payload)?;
    let status = field(&value, "status")
        .and_then(|status| field(status, "status_code"))
        .and_then(integer)
        .ok_or_else(invalid_payload)?;
    // retain the predecessor's provider-level rejection even without observation fields
    if status != 0 {
        return Err(TaskFailure {
            attempts: 0,
            code: "provider_request_rejected".to_owned(),
        });
    }
    let device_id = field(&value, "device_id")
        .and_then(integer)
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(invalid_payload)?;
    let response_type = if text(&value, "type").as_deref() == Some("obs_st") {
        TempestResponseType::StationObservation
    } else {
        TempestResponseType::Other
    };
    let bucket_step_minutes = field(&value, "bucket_step_minutes").and_then(|value| match value {
        JsonValue::Number(number) if number.is_finite() => Some(*number),
        _ => None,
    });
    // preserve the source's null no-data sentinel and documented row order
    let observations = match field(&value, "obs").ok_or_else(invalid_payload)? {
        JsonValue::Null => None,
        JsonValue::Array(rows) => Some(
            rows.iter()
                .map(decode_tempest_row)
                .collect::<Option<Vec<_>>>()
                .ok_or_else(invalid_payload)?,
        ),
        _ => return Err(invalid_payload()),
    };
    Ok(TempestObservationEnvelope {
        bucket_step_minutes,
        device_id,
        observations,
        response_type,
        status_code: status,
    })
}

/// Convert one documented obs_st row into the existing typed normalization boundary.
fn decode_tempest_row(value: &JsonValue) -> Option<TempestObservation> {
    let JsonValue::Array(row) = value else {
        return None;
    };
    // require the complete source layout before any position is consumed
    if row.len() < TEMPEST_ROW_COLUMNS {
        return None;
    }
    let epoch_seconds = integer(&row[0]).filter(|epoch| *epoch > 0)?;
    let cell = |index: usize| nullable_number(row.get(index)?);
    Some(TempestObservation {
        epoch_seconds,
        wind_lull_mps: cell(1)?,
        wind_speed_mps: cell(2)?,
        wind_gust_mps: cell(3)?,
        wind_direction_degrees: cell(4)?,
        wind_sample_interval_seconds: cell(5)?,
        pressure_hpa: cell(6)?,
        air_temperature_c: cell(7)?,
        relative_humidity_percent: cell(8)?,
        illuminance_lux: cell(9)?,
        uv_index: cell(10)?,
        solar_radiation_wm2: cell(11)?,
        precipitation_mm: cell(12)?,
        precipitation_type: cell(13)?,
        lightning_average_distance_km: cell(14)?,
        lightning_strike_count: cell(15)?,
        battery_volts: cell(16)?,
        report_interval_minutes: cell(17)?,
        local_daily_rain_accumulation_mm: cell(18)?,
        rain_accumulation_nc_mm: cell(19)?,
        local_daily_rain_accumulation_final_mm: cell(20)?,
        precipitation_analysis_type: cell(21)?,
    })
}

/// Preserve the original bounded rain-capture projection failure category.
fn capture_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "rain capture projection differs".to_owned(),
    }
}

impl WorkerRainGaugeReplayValues for NativeRainCaptureValues {
    /// Decode a retained raw Tempest capture into its typed provider envelope.
    fn decode_tempest(&mut self, bytes: &[u8]) -> Result<TempestObservationEnvelope, TaskFailure> {
        std::str::from_utf8(bytes).map_err(|_| invalid_payload())?;
        decode_native_tempest_payload(bytes)
    }

    /// Admit one exact canonical UTC replay instant.
    fn parse_utc_milliseconds(&mut self, value: &str) -> Result<i64, TaskFailure> {
        NativeWirePrimitives
            .canonical_utc_milliseconds(value)
            .ok_or_else(capture_failure)
    }

    /// Format the original millisecond UTC instant without resampling a clock.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        NativeWirePrimitives
            .format_utc_milliseconds(epoch_ms)
            .ok_or_else(capture_failure)
    }
}

impl WorkerRainAdjustmentCaptureValues for NativeRainCaptureValues {
    /// Decode only the source hour axis and the seven nullable captured forecast columns.
    fn decode_forecast(&mut self, bytes: &[u8]) -> Result<WorkerRainForecastPayload, TaskFailure> {
        decode_native_rain_forecast(bytes).ok_or_else(capture_failure)
    }

    /// Render the exact original minute-precision provider hour spelling.
    fn format_open_meteo_hour(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        let utc = NativeWirePrimitives
            .format_utc_milliseconds(epoch_ms)
            .ok_or_else(capture_failure)?;
        // the original source format uses a four-digit year and UTC minute axis
        utc.get(..16)
            .filter(|_| utc.len() == 24)
            .map(str::to_owned)
            .ok_or_else(capture_failure)
    }
}

/// Decode the frozen rain profile body with the original fatal UTF-8 policy.
fn decode_native_rain_forecast(bytes: &[u8]) -> Option<WorkerRainForecastPayload> {
    std::str::from_utf8(bytes).ok()?;
    let value = NativeWirePrimitives.decode_json(bytes)?;
    let utc_offset_seconds = integer(field(&value, "utc_offset_seconds")?)?;
    let hourly = field(&value, "hourly")?;
    let JsonValue::Array(raw_time) = field(hourly, "time")? else {
        return None;
    };
    // preserve the 49-cell source axis and omit verification of the unused initialization cell
    if raw_time.len() != FORECAST_TIME_COUNT {
        return None;
    }
    let mut time = Vec::with_capacity(FORECAST_TIME_COUNT);
    time.push(String::new());
    // recover each lead timestamp exactly without introducing local-time normalization
    for cell in raw_time.iter().skip(1) {
        let JsonValue::String(units) = cell else {
            return None;
        };
        time.push(String::from_utf16(units).ok()?);
    }
    let column = |name: &str| -> Option<Vec<Nullable<f64>>> {
        let JsonValue::Array(values) = field(hourly, name)? else {
            return None;
        };
        // the predecessor ignores initialization and extra cells outside the one-based lead grid
        if values.len() < FORECAST_TIME_COUNT {
            return None;
        }
        let mut output = Vec::with_capacity(FORECAST_TIME_COUNT);
        output.push(Nullable::Null);
        // retain every used finite/null cell without fabricating zeros
        for cell in &values[1..FORECAST_TIME_COUNT] {
            output.push(nullable_number(cell)?.map_or(Nullable::Null, Nullable::Value));
        }
        Some(output)
    };
    Some(WorkerRainForecastPayload {
        utc_offset_seconds,
        hourly: WorkerRainForecastHourly {
            time,
            precipitation: column("precipitation")?,
            temperature_2m: column("temperature_2m")?,
            relative_humidity_2m: column("relative_humidity_2m")?,
            cloud_cover: column("cloud_cover")?,
            surface_pressure: column("surface_pressure")?,
            wind_speed_10m: column("wind_speed_10m")?,
            wind_direction_10m: column("wind_direction_10m")?,
        },
    })
}
