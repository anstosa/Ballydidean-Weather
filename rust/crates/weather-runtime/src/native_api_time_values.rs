//! Actual API calendar, cursor, and timestamp boundaries over the approved native primitives.

use base64::{
    Engine as _, alphabet,
    engine::{
        DecodePaddingMode,
        general_purpose::{GeneralPurpose, GeneralPurposeConfig, URL_SAFE_NO_PAD},
    },
};
use jiff::{Span, Timestamp, civil::Date, tz::TimeZone};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use weather_api::{
    AdmittedHistoryCursor, CanonicalInstant, CurrentProjectionError,
    DailyPrecipitationServiceBoundary, ForecastObservationGeometryBoundary, ForecastSelection,
    ForecastServiceBoundary, HistoryBoundaryAdapter, HistoryBoundaryError, HistoryQueryTime,
    RainCollectionTimestampBoundary, ResolvedDailyPrecipitationWindow,
    ResolvedForecastObservationGeometry, ResolvedForecastPressureContextWindow,
    ResolvedForecastReadWindow, ResolvedServiceForecastWindow, TideBoundaryAdapter,
    TideBoundaryError, TideQueryTime, TrendTimeBoundary,
};
use weather_contracts::{HistoryQueryCursor, JsDate, JsonValue};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{NativeWirePrimitives, native_open_meteo_values::load_named_timezone};

const HOUR: i64 = 3_600_000;
const ZONE_CACHE_LIMIT: usize = 64;

/// Shared process calendar cache and native API value owner.
#[derive(Clone, Default)]
pub struct NativeApiTimeValues {
    zones: Arc<RwLock<HashMap<String, TimeZone>>>,
}

impl NativeApiTimeValues {
    /// Cache the admitted site timezones without recreating host TZif data on every request.
    fn zone(&self, name: &str) -> Result<TimeZone, CurrentProjectionError> {
        // retain the predecessor's bounded timezone identity length
        if name.is_empty() || name.len() > 64 {
            return Err(time_error());
        }
        // the ordinary serving path reuses the cached timezone
        if let Some(zone) = self.zones.read().map_err(|_| time_error())?.get(name) {
            return Ok(zone.clone());
        }
        let zone = load_named_timezone(name).ok_or_else(time_error)?;
        let mut zones = self.zones.write().map_err(|_| time_error())?;
        // bound retained timezone names independently of requests
        if zones.len() >= ZONE_CACHE_LIMIT {
            zones.clear();
        }
        zones.entry(name.to_owned()).or_insert_with(|| zone.clone());
        Ok(zone)
    }
}

/// Admit the caller's explicitly zoned instant using the shared native Date boundary.
fn query_instant(value: &str) -> Result<CanonicalInstant, CurrentProjectionError> {
    let milliseconds = NativeWirePrimitives
        .parse_explicit_utc_milliseconds(value)
        .ok_or_else(time_error)?;
    instant(milliseconds)
}

/// Bind one checked epoch value to the original canonical UTC spelling.
fn instant(milliseconds: i64) -> Result<CanonicalInstant, CurrentProjectionError> {
    let utc = NativeWirePrimitives
        .format_utc_milliseconds(milliseconds)
        .ok_or_else(time_error)?;
    Ok(CanonicalInstant {
        epoch_milliseconds: milliseconds,
        utc,
    })
}

/// Shift one sampled nominal instant without another wall-clock read.
fn offset(value: i64, milliseconds: i64) -> Result<CanonicalInstant, CurrentProjectionError> {
    instant(value.checked_add(milliseconds).ok_or_else(time_error)?)
}

/// Return a bounded canonical time failure to the existing API application.
fn time_error() -> CurrentProjectionError {
    CurrentProjectionError {
        code: "api.invalid_time",
        detail: "invalid API timestamp or site calendar".to_owned(),
    }
}

impl HistoryBoundaryAdapter for NativeApiTimeValues {
    /// Normalize an explicit-zone history query timestamp as the predecessor does.
    fn resolve_query_time(
        &self,
        value: &str,
        _field: HistoryQueryTime,
    ) -> Result<CanonicalInstant, HistoryBoundaryError> {
        query_instant(value).map_err(|_| HistoryBoundaryError)
    }

    /// Preserve source cursor field/type admission and the standard base64url representation.
    fn decode_cursor(&self, value: &str) -> Result<AdmittedHistoryCursor, HistoryBoundaryError> {
        let bytes = decode_node_base64url(value).ok_or(HistoryBoundaryError)?;
        let text = String::from_utf8_lossy(&bytes);
        let JsonValue::Object(fields) = NativeWirePrimitives
            .decode_json(text.as_bytes())
            .ok_or(HistoryBoundaryError)?
        else {
            return Err(HistoryBoundaryError);
        };
        // require exactly the two existing cursor properties
        if fields.len() != 2 {
            return Err(HistoryBoundaryError);
        }
        let id = cursor_text(&fields, "id")
            .filter(|id| positive_decimal(id))
            .ok_or(HistoryBoundaryError)?;
        let valid_at = cursor_text(&fields, "validAt").ok_or(HistoryBoundaryError)?;
        Ok(AdmittedHistoryCursor {
            id,
            valid_at: query_instant(&valid_at).map_err(|_| HistoryBoundaryError)?,
        })
    }

    /// Encode the existing two-field stable cursor with no padding or archival newline.
    fn encode_cursor(&self, cursor: &HistoryQueryCursor) -> Result<String, CurrentProjectionError> {
        let value = JsonValue::Object(vec![
            (
                "id".encode_utf16().collect(),
                JsonValue::String(cursor.id.encode_utf16().collect()),
            ),
            (
                "validAt".encode_utf16().collect(),
                JsonValue::String(cursor.validAt.encode_utf16().collect()),
            ),
        ]);
        let mut bytes = NativeWirePrimitives
            .encode_ordered_json(&value)
            .ok_or_else(time_error)?;
        // remove only shared archive framing before base64url
        if bytes.pop() != Some(b'\n') {
            return Err(time_error());
        }
        Ok(URL_SAFE_NO_PAD.encode(bytes))
    }
}

/// Borrow one decoded string cursor property without changing its Unicode representation.
fn cursor_text(fields: &[(Vec<u16>, JsonValue)], name: &str) -> Option<String> {
    fields.iter().find_map(|(key, value)| {
        // retain only an exact matching string member
        if key.iter().copied().eq(name.encode_utf16())
            && let JsonValue::String(units) = value
        {
            String::from_utf16(units).ok()
        } else {
            None
        }
    })
}

/// Require the original positive decimal source identifier grammar.
fn positive_decimal(value: &str) -> bool {
    let bytes = value.as_bytes();
    matches!(bytes.first(), Some(b'1'..=b'9')) && bytes.iter().all(u8::is_ascii_digit)
}

/// Use the approved base64 engine with Node's permissive base64url input normalization.
fn decode_node_base64url(value: &str) -> Option<Vec<u8>> {
    let engine = GeneralPurpose::new(
        &alphabet::URL_SAFE,
        GeneralPurposeConfig::new()
            .with_encode_padding(false)
            .with_decode_padding_mode(DecodePaddingMode::RequireNone)
            .with_decode_allow_trailing_bits(true),
    );
    let mut encoded: Vec<u8> = value
        .bytes()
        .take_while(|byte| *byte != b'=')
        .filter_map(|byte| match byte {
            b'+' => Some(b'-'),
            b'/' => Some(b'_'),
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' => Some(byte),
            _ => None,
        })
        .collect();
    // Node ignores a trailing isolated base64 symbol rather than synthesizing a byte
    if encoded.len() % 4 == 1 {
        encoded.pop();
    }
    engine.decode(encoded).ok()
}

impl TideBoundaryAdapter for NativeApiTimeValues {
    /// Preserve explicitly zoned tide query values.
    fn resolve_query_time(
        &self,
        value: &str,
        _field: TideQueryTime,
    ) -> Result<CanonicalInstant, TideBoundaryError> {
        query_instant(value).map_err(|_| TideBoundaryError)
    }

    /// Compute tide defaults from the same nominal request instant.
    fn offset_data_time(
        &self,
        data_at: &CanonicalInstant,
        offset_milliseconds: i64,
        _field: TideQueryTime,
    ) -> Result<CanonicalInstant, TideBoundaryError> {
        offset(data_at.epoch_milliseconds, offset_milliseconds).map_err(|_| TideBoundaryError)
    }
}

impl TrendTimeBoundary for NativeApiTimeValues {
    /// Resolve the fixed source local-calendar start in the actual site timezone.
    fn calendar_start(
        &self,
        _generated_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<CanonicalInstant, CurrentProjectionError> {
        let date: Date = "2019-01-01".parse().map_err(|_| time_error())?;
        unique_wall_clock(date, 0, &self.zone(timezone)?)
    }

    /// Render a retained PostgreSQL Date value without consulting the process clock.
    fn resolve_row_time(&self, value: &JsDate) -> Result<CanonicalInstant, CurrentProjectionError> {
        match value {
            JsDate::EpochMilliseconds(milliseconds) => instant(*milliseconds),
            JsDate::InvalidDate => Err(time_error()),
        }
    }
}

/// Resolve exactly one representable local clock as required by the source API calendar.
fn unique_wall_clock(
    date: Date,
    hour: i8,
    zone: &TimeZone,
) -> Result<CanonicalInstant, CurrentProjectionError> {
    let requested = date.at(hour, 0, 0, 0);
    let earlier = zone
        .to_ambiguous_timestamp(requested)
        .earlier()
        .map_err(|_| time_error())?;
    let later = zone
        .to_ambiguous_timestamp(requested)
        .later()
        .map_err(|_| time_error())?;
    // reject folds and gaps rather than silently replacing the user's local day
    if earlier != later || zone.to_datetime(earlier) != requested {
        return Err(time_error());
    }
    instant(earlier.as_millisecond())
}

/// Add whole civil days without assuming 24 actual UTC hours.
fn add_days(date: Date, days: i64) -> Result<Date, CurrentProjectionError> {
    let span = Span::new().try_days(days).map_err(|_| time_error())?;
    date.checked_add(span).map_err(|_| time_error())
}

/// Determine public forecast geometry before its repository-only context expansion.
fn public_forecast_window(
    selection: ForecastSelection,
    data_at: &CanonicalInstant,
    zone: &TimeZone,
) -> Result<(CanonicalInstant, CanonicalInstant, u64), CurrentProjectionError> {
    let timestamp =
        Timestamp::from_millisecond(data_at.epoch_milliseconds).map_err(|_| time_error())?;
    let local = zone.to_datetime(timestamp);
    let today = local.date();
    let (start_date, end_date, end_hour, minimum, maximum) = match selection {
        ForecastSelection::Daily { days } if matches!(days, 1 | 5 | 10) => (
            today,
            add_days(today, i64::from(days))?,
            0,
            i64::from(days) * 24 - 1,
            i64::from(days) * 24 + 1,
        ),
        ForecastSelection::Overnight => {
            // keep the previous local date active until the actual 07:00 cutoff
            let anchor = if local.hour() < 7 {
                add_days(today, -1)?
            } else {
                today
            };
            (anchor, add_days(anchor, 1)?, 7, 30, 32)
        }
        _ => return Err(time_error()),
    };
    let start = unique_wall_clock(start_date, 0, zone)?;
    let end = unique_wall_clock(end_date, end_hour, zone)?;
    let elapsed = end
        .epoch_milliseconds
        .checked_sub(start.epoch_milliseconds)
        .ok_or_else(time_error)?;
    let hours = elapsed / HOUR;
    // accept only the source's bounded whole-hour DST windows
    if elapsed % HOUR != 0 || !(minimum..=maximum).contains(&hours) {
        return Err(time_error());
    }
    Ok((start, end, hours as u64))
}

impl ForecastServiceBoundary for NativeApiTimeValues {
    /// Compute the public and expanded repository windows with exact civil geometry and UTC offsets.
    fn resolve_window(
        &self,
        selection: ForecastSelection,
        data_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<ResolvedServiceForecastWindow, CurrentProjectionError> {
        let (public_start, public_end, hours) =
            public_forecast_window(selection, data_at, &self.zone(timezone)?)?;
        let (start, end, repository_hours) = match selection {
            ForecastSelection::Daily { .. } => (
                offset(public_start.epoch_milliseconds, -12 * HOUR)?,
                offset(public_end.epoch_milliseconds, HOUR)?,
                hours + 13,
            ),
            ForecastSelection::Overnight => (public_start.clone(), public_end.clone(), hours),
        };
        let pressure = ResolvedForecastPressureContextWindow {
            as_of: public_start.clone(),
            context_start: offset(public_start.epoch_milliseconds, -3 * HOUR)?,
            context_end: offset(public_start.epoch_milliseconds, 3 * HOUR)?,
            oldest_run: offset(public_start.epoch_milliseconds, -48 * HOUR)?,
        };
        Ok(ResolvedServiceForecastWindow {
            public_start,
            public_end,
            repository: ResolvedForecastReadWindow {
                as_of: start,
                end_exclusive: end,
                hours: repository_hours,
            },
            pressure_context: pressure,
        })
    }
}

impl DailyPrecipitationServiceBoundary for NativeApiTimeValues {
    /// Use the same civil-day window as the original forecast helper and preserve nominal end time.
    fn resolve_window(
        &self,
        data_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<ResolvedDailyPrecipitationWindow, CurrentProjectionError> {
        let (from, _, _) = public_forecast_window(
            ForecastSelection::Daily { days: 1 },
            data_at,
            &self.zone(timezone)?,
        )?;
        Ok(ResolvedDailyPrecipitationWindow {
            from,
            to: data_at.clone(),
        })
    }
}

impl RainCollectionTimestampBoundary for NativeApiTimeValues {
    /// Require the exact retained canonical UTC spelling for public rainfall status.
    fn validate_canonical_utc(&self, value: &str) -> Result<(), CurrentProjectionError> {
        NativeWirePrimitives
            .canonical_utc_milliseconds(value)
            .map(|_| ())
            .ok_or_else(time_error)
    }
}

impl ForecastObservationGeometryBoundary for NativeApiTimeValues {
    /// Bind the private fixed observation window to the exact database projection geometry.
    fn resolve_expected_geometry(
        &self,
        data_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<ResolvedForecastObservationGeometry, CurrentProjectionError> {
        let zone = self.zone(timezone)?;
        let timestamp =
            Timestamp::from_millisecond(data_at.epoch_milliseconds).map_err(|_| time_error())?;
        let projection_from = unique_wall_clock(zone.to_datetime(timestamp).date(), 0, &zone)?;
        let midnight_context = projection_from
            .epoch_milliseconds
            .checked_sub(3 * HOUR)
            .ok_or_else(time_error)?;
        let recent_context = data_at
            .epoch_milliseconds
            .checked_sub(24 * HOUR + 20 * 60_000)
            .ok_or_else(time_error)?;
        Ok(ResolvedForecastObservationGeometry {
            projection_from,
            query_from: instant(midnight_context.min(recent_context))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Match real DST calendar spans and opaque source cursors without a database connection.
    #[test]
    fn native_api_time_values_preserve_dst_and_cursor() {
        let values = NativeApiTimeValues::default();
        let data_at = query_instant("2026-03-08T19:00:00.000Z").unwrap();
        let day = ForecastServiceBoundary::resolve_window(
            &values,
            ForecastSelection::Daily { days: 1 },
            &data_at,
            "America/Los_Angeles",
        )
        .unwrap();
        assert_eq!(day.public_start.utc, "2026-03-08T08:00:00.000Z");
        assert_eq!(day.public_end.utc, "2026-03-09T07:00:00.000Z");
        assert_eq!(day.repository.hours, 36);
        let overnight_at = query_instant("2026-03-08T12:00:00Z").unwrap();
        let overnight = ForecastServiceBoundary::resolve_window(
            &values,
            ForecastSelection::Overnight,
            &overnight_at,
            "America/Los_Angeles",
        )
        .unwrap();
        assert_eq!(overnight.repository.hours, 30);
        let cursor = HistoryQueryCursor {
            id: "12345678901234".to_owned(),
            validAt: data_at.utc.clone(),
        };
        let encoded = values.encode_cursor(&cursor).unwrap();
        let decoded = values.decode_cursor(&encoded).unwrap();
        assert_eq!(decoded.id, cursor.id);
        assert_eq!(decoded.valid_at, data_at);
    }
}
