//! Native UTC calendar and restart-state values for the forecast scheduler.

use jiff::{Span, Timestamp, civil::Date, tz::TimeZone};
use weather_contracts::JsonValue;
use weather_edge::{
    ArchiveWirePrimitives, FORECAST_SCHEDULER_HISTORY_LIMIT,
    FORECAST_SCHEDULER_STATE_CONTRACT_VERSION, ForecastSchedulerAttempt,
    ForecastSchedulerCalendarPort, ForecastSchedulerDayOffset, ForecastSchedulerError,
    ForecastSchedulerGap, ForecastSchedulerGapReason, ForecastSchedulerState,
    ForecastSchedulerStateCodec, ForecastSchedulerUtcBase, ForecastSchedulerUtcDay,
    RevisionArchiveCanonicalCodec,
};

use crate::NativeWirePrimitives;

const UTC_DAY_MILLISECONDS: i64 = 24 * 60 * 60 * 1_000;
const TOP_LEVEL_FIELDS: [&str; 7] = [
    "activation",
    "attempts",
    "contractVersion",
    "errors",
    "gaps",
    "lastCheckedAt",
    "updatedAt",
];
const ATTEMPT_FIELDS: [&str; 3] = ["committedAt", "dueKey", "status"];
const ERROR_FIELDS: [&str; 3] = ["at", "code", "dueKey"];
const GAP_FIELDS: [&str; 3] = ["dueKey", "occurredAt", "reason"];
const ACTIVATION_MAXIMUM_UTF16_UNITS: usize = 64;
const BOUNDED_STRING_MAXIMUM_UTF16_UNITS: usize = 200;

type Object = [(Vec<u16>, JsonValue)];

impl ForecastSchedulerCalendarPort for NativeWirePrimitives {
    /// Resolve the source's yesterday or today date through Jiff's fixed UTC calendar.
    fn fixed_utc_day(
        &self,
        observation_epoch_milliseconds: i64,
        offset: ForecastSchedulerDayOffset,
    ) -> Option<ForecastSchedulerUtcDay> {
        let observation = Timestamp::from_millisecond(observation_epoch_milliseconds).ok()?;
        let mut date = TimeZone::UTC.to_datetime(observation).date();
        // Apply the source's only supported civil-day offset before resolving fixed bases.
        if offset == ForecastSchedulerDayOffset::Yesterday {
            let yesterday = Span::new().try_days(-1).ok()?;
            date = date.checked_add(yesterday).ok()?;
        }
        scheduler_day(self, date)
    }

    /// Format one computed instant with the same strict UTC primitive used by due parsing.
    fn canonical_utc(&self, epoch_milliseconds: i64) -> Option<String> {
        ArchiveWirePrimitives::format_utc_milliseconds(self, epoch_milliseconds)
    }

    /// Advance the source observation by its exact 24-hour UTC cursor step.
    fn next_utc_day_observation(&self, epoch_milliseconds: i64) -> Option<i64> {
        let next = epoch_milliseconds.checked_add(UTC_DAY_MILLISECONDS)?;
        Timestamp::from_millisecond(next).ok()?;
        Some(next)
    }
}

impl ForecastSchedulerStateCodec for NativeWirePrimitives {
    /// Decode JSON.parse-style input while enforcing the source's closed state grammar.
    fn decode_state(&self, bytes: &[u8]) -> Option<ForecastSchedulerState> {
        let value = ArchiveWirePrimitives::decode_json(self, bytes)?;
        decode_state_value(self, &value)
    }

    /// Validate and encode recursively sorted JSON with exactly one terminating line feed.
    fn encode_state(&self, state: &ForecastSchedulerState) -> Option<Vec<u8>> {
        let value = encode_state_value(self, state)?;
        ArchiveWirePrimitives::encode_sorted_json(self, &value)
    }
}

/// Resolve one exact UTC day and its four fixed scheduler bases.
fn scheduler_day(wire: &NativeWirePrimitives, date: Date) -> Option<ForecastSchedulerUtcDay> {
    let midnight = utc_timestamp(date, 0, 0)?.as_millisecond();
    Some(ForecastSchedulerUtcDay {
        bases: [
            scheduler_base(wire, date, 0)?,
            scheduler_base(wire, date, 6)?,
            scheduler_base(wire, date, 12)?,
            scheduler_base(wire, date, 18)?,
        ],
        midnight_epoch_milliseconds: midnight,
    })
}

/// Resolve one fixed source base and bind its canonical spelling to the same instant.
fn scheduler_base(
    wire: &NativeWirePrimitives,
    date: Date,
    hour: i8,
) -> Option<ForecastSchedulerUtcBase> {
    let epoch_milliseconds = utc_timestamp(date, hour, 35)?.as_millisecond();
    let canonical_at = ArchiveWirePrimitives::format_utc_milliseconds(wire, epoch_milliseconds)?;
    Some(ForecastSchedulerUtcBase {
        canonical_at,
        epoch_milliseconds,
    })
}

/// Resolve one unique fixed-zone UTC civil time without timezone fallback.
fn utc_timestamp(date: Date, hour: i8, minute: i8) -> Option<Timestamp> {
    TimeZone::UTC.to_timestamp(date.at(hour, minute, 0, 0)).ok()
}

/// Decode one validated scheduler state from the generated JSON tree.
fn decode_state_value(
    wire: &NativeWirePrimitives,
    value: &JsonValue,
) -> Option<ForecastSchedulerState> {
    let fields = closed_object(value, &TOP_LEVEL_FIELDS)?;
    let contract_version = text(fields, "contractVersion")?;
    // Reject every state version except the exact scheduler contract.
    if contract_version != FORECAST_SCHEDULER_STATE_CONTRACT_VERSION {
        return None;
    }
    let activation = string_units(fields, "activation")?;
    // Preserve JavaScript string-length admission for the opaque activation reason.
    if activation.len() > ACTIVATION_MAXIMUM_UTF16_UNITS {
        return None;
    }
    let activation = String::from_utf16(activation).ok()?;
    let attempts = decode_attempts(wire, field(fields, "attempts")?)?;
    let errors = decode_errors(wire, field(fields, "errors")?)?;
    let gaps = decode_gaps(wire, field(fields, "gaps")?)?;
    Some(ForecastSchedulerState {
        activation,
        attempts,
        contract_version: FORECAST_SCHEDULER_STATE_CONTRACT_VERSION,
        errors,
        gaps,
        last_checked_at: optional_instant(wire, field(fields, "lastCheckedAt")?)?,
        updated_at: optional_instant(wire, field(fields, "updatedAt")?)?,
    })
}

/// Decode the bounded attempt history in original array order.
fn decode_attempts(
    wire: &NativeWirePrimitives,
    value: &JsonValue,
) -> Option<Vec<ForecastSchedulerAttempt>> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    // Enforce the source history ceiling before allocating the typed result.
    if values.len() > FORECAST_SCHEDULER_HISTORY_LIMIT {
        return None;
    }
    let mut attempts = Vec::with_capacity(values.len());
    // Retain each exact attempt without normalizing its opaque status.
    for value in values {
        let fields = closed_object(value, &ATTEMPT_FIELDS)?;
        attempts.push(ForecastSchedulerAttempt {
            committed_at: required_instant(wire, field(fields, "committedAt")?)?,
            due_key: due_key(field(fields, "dueKey")?)?,
            status: bounded_text(field(fields, "status")?)?,
        });
    }
    Some(attempts)
}

/// Decode the bounded error history in original array order.
fn decode_errors(
    wire: &NativeWirePrimitives,
    value: &JsonValue,
) -> Option<Vec<ForecastSchedulerError>> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    // Enforce the source history ceiling before allocating the typed result.
    if values.len() > FORECAST_SCHEDULER_HISTORY_LIMIT {
        return None;
    }
    let mut errors = Vec::with_capacity(values.len());
    // Retain each exact error without interpreting its opaque code.
    for value in values {
        let fields = closed_object(value, &ERROR_FIELDS)?;
        errors.push(ForecastSchedulerError {
            at: required_instant(wire, field(fields, "at")?)?,
            code: bounded_text(field(fields, "code")?)?,
            due_key: due_key(field(fields, "dueKey")?)?,
        });
    }
    Some(errors)
}

/// Decode the bounded categorical gap history in original array order.
fn decode_gaps(
    wire: &NativeWirePrimitives,
    value: &JsonValue,
) -> Option<Vec<ForecastSchedulerGap>> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    // Enforce the source history ceiling before allocating the typed result.
    if values.len() > FORECAST_SCHEDULER_HISTORY_LIMIT {
        return None;
    }
    let mut gaps = Vec::with_capacity(values.len());
    // Admit only the source's two durable scheduler gap categories.
    for value in values {
        let fields = closed_object(value, &GAP_FIELDS)?;
        let reason = match text(fields, "reason")?.as_str() {
            "capture_failed" => ForecastSchedulerGapReason::CaptureFailed,
            "missed_capture" => ForecastSchedulerGapReason::MissedCapture,
            _ => return None,
        };
        gaps.push(ForecastSchedulerGap {
            due_key: due_key(field(fields, "dueKey")?)?,
            occurred_at: required_instant(wire, field(fields, "occurredAt")?)?,
            reason,
        });
    }
    Some(gaps)
}

/// Validate one typed state and build its exact source-owned JSON shape.
fn encode_state_value(
    wire: &NativeWirePrimitives,
    state: &ForecastSchedulerState,
) -> Option<JsonValue> {
    // Reject typed values outside the same closed state contract used by reads.
    if state.contract_version != FORECAST_SCHEDULER_STATE_CONTRACT_VERSION
        || state.activation.encode_utf16().count() > ACTIVATION_MAXIMUM_UTF16_UNITS
        || state.attempts.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
        || state.errors.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
        || state.gaps.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
    {
        return None;
    }
    let mut attempts = Vec::with_capacity(state.attempts.len());
    // Validate and preserve typed attempt order.
    for attempt in &state.attempts {
        required_instant_text(wire, &attempt.committed_at)?;
        valid_due_key(&attempt.due_key).then_some(())?;
        valid_bounded_text(&attempt.status).then_some(())?;
        attempts.push(object([
            ("committedAt", string(&attempt.committed_at)),
            ("dueKey", string(&attempt.due_key)),
            ("status", string(&attempt.status)),
        ]));
    }
    let mut errors = Vec::with_capacity(state.errors.len());
    // Validate and preserve typed error order.
    for error in &state.errors {
        required_instant_text(wire, &error.at)?;
        valid_bounded_text(&error.code).then_some(())?;
        valid_due_key(&error.due_key).then_some(())?;
        errors.push(object([
            ("at", string(&error.at)),
            ("code", string(&error.code)),
            ("dueKey", string(&error.due_key)),
        ]));
    }
    let mut gaps = Vec::with_capacity(state.gaps.len());
    // Validate and preserve typed gap order.
    for gap in &state.gaps {
        valid_due_key(&gap.due_key).then_some(())?;
        required_instant_text(wire, &gap.occurred_at)?;
        gaps.push(object([
            ("dueKey", string(&gap.due_key)),
            ("occurredAt", string(&gap.occurred_at)),
            ("reason", string(gap.reason.wire())),
        ]));
    }
    // Validate both optional instants before constructing their nullable JSON values.
    let last_checked_at = match state.last_checked_at.as_deref() {
        Some(value) => Some(required_instant_text(wire, value)?),
        None => None,
    };
    let updated_at = match state.updated_at.as_deref() {
        Some(value) => Some(required_instant_text(wire, value)?),
        None => None,
    };
    Some(object([
        ("activation", string(&state.activation)),
        ("attempts", JsonValue::Array(attempts)),
        (
            "contractVersion",
            string(FORECAST_SCHEDULER_STATE_CONTRACT_VERSION),
        ),
        ("errors", JsonValue::Array(errors)),
        ("gaps", JsonValue::Array(gaps)),
        ("lastCheckedAt", nullable_string(last_checked_at)),
        ("updatedAt", nullable_string(updated_at)),
    ]))
}

/// Require one object with each expected key exactly once and no extras.
fn closed_object<'a>(value: &'a JsonValue, expected: &[&str]) -> Option<&'a Object> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    // Duplicate or unknown fields cannot satisfy the complete expected-key census.
    if fields.len() != expected.len()
        || !expected.iter().all(|key| {
            // Count exact UTF-16 field spellings without normalizing them.
            fields
                .iter()
                .filter(|(name, _)| name.iter().copied().eq(key.encode_utf16()))
                .count()
                == 1
        })
    {
        return None;
    }
    Some(fields)
}

/// Locate one field already proven unique by the closed-object census.
fn field<'a>(fields: &'a Object, key: &str) -> Option<&'a JsonValue> {
    fields.iter().find_map(|(name, value)| {
        // Compare exact original UTF-16 field units.
        name.iter().copied().eq(key.encode_utf16()).then_some(value)
    })
}

/// Borrow one exact JSON string in its JavaScript UTF-16 representation.
fn string_units<'a>(fields: &'a Object, key: &str) -> Option<&'a [u16]> {
    let JsonValue::String(value) = field(fields, key)? else {
        return None;
    };
    Some(value)
}

/// Decode one exact JSON string into the typed scheduler representation.
fn text(fields: &Object, key: &str) -> Option<String> {
    String::from_utf16(string_units(fields, key)?).ok()
}

/// Decode and validate one required canonical UTC instant.
fn required_instant(wire: &NativeWirePrimitives, value: &JsonValue) -> Option<String> {
    let JsonValue::String(value) = value else {
        return None;
    };
    let value = String::from_utf16(value).ok()?;
    required_instant_text(wire, &value)?;
    Some(value)
}

/// Validate one typed canonical UTC instant with the native runtime owner.
fn required_instant_text<'a>(wire: &NativeWirePrimitives, value: &'a str) -> Option<&'a str> {
    RevisionArchiveCanonicalCodec::canonical_utc_milliseconds(wire, value)?;
    Some(value)
}

/// Decode one nullable canonical UTC instant.
fn optional_instant(wire: &NativeWirePrimitives, value: &JsonValue) -> Option<Option<String>> {
    match value {
        JsonValue::Null => Some(None),
        _ => Some(Some(required_instant(wire, value)?)),
    }
}

/// Decode one nonempty bounded opaque string with JavaScript length semantics.
fn bounded_text(value: &JsonValue) -> Option<String> {
    let JsonValue::String(units) = value else {
        return None;
    };
    // Preserve the source UTF-16 length and control-unit bounds.
    if units.is_empty()
        || units.len() > BOUNDED_STRING_MAXIMUM_UTF16_UNITS
        || units
            .iter()
            .any(|unit| matches!(*unit, 0x0000..=0x001f | 0x007f))
    {
        return None;
    }
    String::from_utf16(units).ok()
}

/// Validate one typed bounded opaque string with JavaScript length semantics.
fn valid_bounded_text(value: &str) -> bool {
    let mut length = 0;
    // Count UTF-16 units and reject the same control range as the source.
    for unit in value.encode_utf16() {
        length += 1;
        // Refuse every control code rejected by the predecessor regular expression.
        if matches!(unit, 0x0000..=0x001f | 0x007f) {
            return false;
        }
    }
    (1..=BOUNDED_STRING_MAXIMUM_UTF16_UNITS).contains(&length)
}

/// Decode and validate one scheduler capture due key.
fn due_key(value: &JsonValue) -> Option<String> {
    let JsonValue::String(units) = value else {
        return None;
    };
    let value = String::from_utf16(units).ok()?;
    valid_due_key(&value).then_some(value)
}

/// Match the source due-key regular expression without calendar normalization.
fn valid_due_key(value: &str) -> bool {
    let Some(value) = value.strip_prefix("capture/") else {
        return false;
    };
    let bytes = value.as_bytes();
    bytes.len() == 24
        && bytes.iter().enumerate().all(|(index, byte)| {
            // Admit digits only at the source regular expression's numeric positions.
            match index {
                4 | 7 => *byte == b'-',
                10 => *byte == b'T',
                13 | 16 => *byte == b':',
                19 => *byte == b'.',
                23 => *byte == b'Z',
                _ => byte.is_ascii_digit(),
            }
        })
        && matches!(&bytes[11..13], b"00" | b"06" | b"12" | b"18")
        && &bytes[14..19] == b"35:00"
        && &bytes[20..23] == b"000"
}

/// Construct one exact JSON string value.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Construct one exact nullable JSON string value.
fn nullable_string(value: Option<&str>) -> JsonValue {
    value.map_or(JsonValue::Null, string)
}

/// Construct one ordered closed object for the canonical JSON codec.
fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| {
                // Preserve the source field spellings in UTF-16 storage.
                (name.encode_utf16().collect(), value)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE_CANONICAL_FRAME: &[u8] = concat!(
        "{\"activation\":\"active\",\"attempts\":[{\"committedAt\":\"2026-11-23T00:36:00.000Z\",",
        "\"dueKey\":\"capture/2026-11-23T00:35:00.000Z\",\"status\":\"committed\"}],",
        "\"contractVersion\":\"adjustment-evidence-scheduler-state/v1\",\"errors\":[],",
        "\"gaps\":[],\"lastCheckedAt\":\"2026-11-23T00:37:00.000Z\",",
        "\"updatedAt\":\"2026-11-23T00:38:00.000Z\"}\n",
    )
    .as_bytes();

    /// Verify Jiff resolves the exact UTC date and fixed scheduler bases.
    #[test]
    fn resolves_exact_scheduler_utc_day() {
        let wire = NativeWirePrimitives;
        let day = wire
            .fixed_utc_day(1_795_396_500_000, ForecastSchedulerDayOffset::Today)
            .expect("scheduler day");
        assert_eq!(day.midnight_epoch_milliseconds, 1_795_392_000_000);
        assert_eq!(
            day.bases.map(|base| base.canonical_at),
            [
                "2026-11-23T00:35:00.000Z",
                "2026-11-23T06:35:00.000Z",
                "2026-11-23T12:35:00.000Z",
                "2026-11-23T18:35:00.000Z",
            ]
        );
    }

    /// Verify noncanonical JSON framing decodes and re-encodes as exact Node canonical bytes.
    #[test]
    fn round_trips_exact_scheduler_state_grammar() {
        let noncanonical = concat!(
            "{\n \"updatedAt\": \"2026-11-23T00:38:00.000Z\", \"gaps\": [],",
            "\"activation\":\"active\",\"errors\":[],",
            "\"contractVersion\":\"adjustment-evidence-scheduler-state/v1\",",
            "\"attempts\":[{\"status\":\"committed\",",
            "\"dueKey\":\"capture/2026-11-23T00:35:00.000Z\",",
            "\"committedAt\":\"2026-11-23T00:36:00.000Z\"}],",
            "\"lastCheckedAt\":\"2026-11-23T00:37:00.000Z\"}\n",
        );
        let wire = NativeWirePrimitives;
        let state = wire
            .decode_state(noncanonical.as_bytes())
            .expect("decoded scheduler state");
        assert_eq!(state.attempts[0].status, "committed");
        assert_eq!(
            wire.encode_state(&state).as_deref(),
            Some(NODE_CANONICAL_FRAME)
        );
    }

    /// Verify due-key hours and bounded opaque fields retain their closed source domains.
    #[test]
    fn rejects_invalid_scheduler_state_values() {
        let wire = NativeWirePrimitives;
        let mut state = ForecastSchedulerState::empty();
        state.activation = "active".to_owned();
        state.attempts.push(ForecastSchedulerAttempt {
            committed_at: "2026-11-23T00:36:00.000Z".to_owned(),
            due_key: "capture/2026-11-23T01:35:00.000Z".to_owned(),
            status: "committed".to_owned(),
        });
        assert_eq!(wire.encode_state(&state), None);

        state.attempts[0].due_key = "capture/2026-11-23T00:35:00.000Z".to_owned();
        state.attempts[0].status.clear();
        assert_eq!(wire.encode_state(&state), None);
    }
}
