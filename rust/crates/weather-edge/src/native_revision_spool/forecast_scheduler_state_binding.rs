//! Bind scheduler restart state to the existing JSON and UTC runtime codecs.

use weather_contracts::JsonValue;

use crate::revision_archive_canonical::instant;
use crate::{ForecastEvidenceMaterial, ForecastEvidenceReceiptCodec};

use super::forecast_scheduler_state::{
    FORECAST_SCHEDULER_HISTORY_LIMIT, FORECAST_SCHEDULER_STATE_CONTRACT_VERSION,
    ForecastSchedulerAttempt, ForecastSchedulerError, ForecastSchedulerGap,
    ForecastSchedulerGapReason, ForecastSchedulerState, ForecastSchedulerStateCodec,
};

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

impl<Codec: ForecastEvidenceReceiptCodec> ForecastSchedulerStateCodec
    for ForecastEvidenceMaterial<Codec>
{
    /// decode any valid JSON framing while retaining the exact closed state schema.
    fn decode_state(&self, bytes: &[u8]) -> Option<ForecastSchedulerState> {
        let value = self.codec().decode_retained_json(bytes)?;
        decode_state_value(self.codec(), &value)
    }

    /// validate and emit the source canonical JSON with its single newline frame.
    fn encode_state(&self, state: &ForecastSchedulerState) -> Option<Vec<u8>> {
        let value = encode_state_value(self.codec(), state)?;
        let mut bytes = self.codec().canonical_json(&value)?;
        bytes.push(b'\n');
        Some(bytes)
    }
}

/// decode one validated scheduler state from the dependency-free JSON tree.
fn decode_state_value(
    codec: &impl ForecastEvidenceReceiptCodec,
    value: &JsonValue,
) -> Option<ForecastSchedulerState> {
    let fields = closed_object(value, &TOP_LEVEL_FIELDS)?;
    let contract_version = text(fields, "contractVersion")?;
    (contract_version == FORECAST_SCHEDULER_STATE_CONTRACT_VERSION).then_some(())?;
    let activation = string_units(fields, "activation")?;
    // preserve JavaScript string length for the unenumerated activation reason
    if activation.len() > ACTIVATION_MAXIMUM_UTF16_UNITS {
        return None;
    }
    let activation = String::from_utf16(activation).ok()?;
    let attempts = decode_attempts(codec, field(fields, "attempts")?)?;
    let errors = decode_errors(codec, field(fields, "errors")?)?;
    let gaps = decode_gaps(codec, field(fields, "gaps")?)?;
    Some(ForecastSchedulerState {
        activation,
        attempts,
        contract_version: FORECAST_SCHEDULER_STATE_CONTRACT_VERSION,
        errors,
        gaps,
        last_checked_at: optional_instant(codec, field(fields, "lastCheckedAt")?)?,
        updated_at: optional_instant(codec, field(fields, "updatedAt")?)?,
    })
}

/// decode the bounded attempt history in original array order.
fn decode_attempts(
    codec: &impl ForecastEvidenceReceiptCodec,
    value: &JsonValue,
) -> Option<Vec<ForecastSchedulerAttempt>> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    // enforce the source history ceiling before allocating the typed result
    if values.len() > FORECAST_SCHEDULER_HISTORY_LIMIT {
        return None;
    }
    let mut attempts = Vec::with_capacity(values.len());
    // retain each exact attempt without normalizing its status
    for value in values {
        let fields = closed_object(value, &ATTEMPT_FIELDS)?;
        attempts.push(ForecastSchedulerAttempt {
            committed_at: required_instant(codec, field(fields, "committedAt")?)?,
            due_key: due_key(field(fields, "dueKey")?)?,
            status: bounded_text(field(fields, "status")?)?,
        });
    }
    Some(attempts)
}

/// decode the bounded error history in original array order.
fn decode_errors(
    codec: &impl ForecastEvidenceReceiptCodec,
    value: &JsonValue,
) -> Option<Vec<ForecastSchedulerError>> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    // enforce the source history ceiling before allocating the typed result
    if values.len() > FORECAST_SCHEDULER_HISTORY_LIMIT {
        return None;
    }
    let mut errors = Vec::with_capacity(values.len());
    // retain each exact error without interpreting its opaque code
    for value in values {
        let fields = closed_object(value, &ERROR_FIELDS)?;
        errors.push(ForecastSchedulerError {
            at: required_instant(codec, field(fields, "at")?)?,
            code: bounded_text(field(fields, "code")?)?,
            due_key: due_key(field(fields, "dueKey")?)?,
        });
    }
    Some(errors)
}

/// decode the bounded categorical gap history in original array order.
fn decode_gaps(
    codec: &impl ForecastEvidenceReceiptCodec,
    value: &JsonValue,
) -> Option<Vec<ForecastSchedulerGap>> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    // enforce the source history ceiling before allocating the typed result
    if values.len() > FORECAST_SCHEDULER_HISTORY_LIMIT {
        return None;
    }
    let mut gaps = Vec::with_capacity(values.len());
    // admit only the source's two durable scheduler gap categories
    for value in values {
        let fields = closed_object(value, &GAP_FIELDS)?;
        let reason = match text(fields, "reason")?.as_str() {
            "capture_failed" => ForecastSchedulerGapReason::CaptureFailed,
            "missed_capture" => ForecastSchedulerGapReason::MissedCapture,
            _ => return None,
        };
        gaps.push(ForecastSchedulerGap {
            due_key: due_key(field(fields, "dueKey")?)?,
            occurred_at: required_instant(codec, field(fields, "occurredAt")?)?,
            reason,
        });
    }
    Some(gaps)
}

/// validate one typed state and build its exact source-owned JSON shape.
fn encode_state_value(
    codec: &impl ForecastEvidenceReceiptCodec,
    state: &ForecastSchedulerState,
) -> Option<JsonValue> {
    // reject typed values outside the same closed state contract used by reads
    if state.contract_version != FORECAST_SCHEDULER_STATE_CONTRACT_VERSION
        || state.activation.encode_utf16().count() > ACTIVATION_MAXIMUM_UTF16_UNITS
        || state.attempts.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
        || state.errors.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
        || state.gaps.len() > FORECAST_SCHEDULER_HISTORY_LIMIT
    {
        return None;
    }
    let mut attempts = Vec::with_capacity(state.attempts.len());
    // validate and preserve typed attempt order
    for attempt in &state.attempts {
        required_instant_text(codec, &attempt.committed_at)?;
        valid_due_key(&attempt.due_key).then_some(())?;
        valid_bounded_text(&attempt.status).then_some(())?;
        attempts.push(object([
            ("committedAt", string(&attempt.committed_at)),
            ("dueKey", string(&attempt.due_key)),
            ("status", string(&attempt.status)),
        ]));
    }
    let mut errors = Vec::with_capacity(state.errors.len());
    // validate and preserve typed error order
    for error in &state.errors {
        required_instant_text(codec, &error.at)?;
        valid_bounded_text(&error.code).then_some(())?;
        valid_due_key(&error.due_key).then_some(())?;
        errors.push(object([
            ("at", string(&error.at)),
            ("code", string(&error.code)),
            ("dueKey", string(&error.due_key)),
        ]));
    }
    let mut gaps = Vec::with_capacity(state.gaps.len());
    // validate and preserve typed gap order
    for gap in &state.gaps {
        valid_due_key(&gap.due_key).then_some(())?;
        required_instant_text(codec, &gap.occurred_at)?;
        gaps.push(object([
            ("dueKey", string(&gap.due_key)),
            ("occurredAt", string(&gap.occurred_at)),
            ("reason", string(gap.reason.wire())),
        ]));
    }
    // validate both optional instants before constructing their nullable JSON values
    let last_checked_at = match state.last_checked_at.as_deref() {
        Some(value) => Some(required_instant_text(codec, value)?),
        None => None,
    };
    let updated_at = match state.updated_at.as_deref() {
        Some(value) => Some(required_instant_text(codec, value)?),
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

/// require one object with each expected key exactly once and no extras.
fn closed_object<'a>(value: &'a JsonValue, expected: &[&str]) -> Option<&'a Object> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    // duplicate or unknown fields cannot satisfy the complete expected key census
    if fields.len() != expected.len()
        || !expected.iter().all(
            // count exact UTF-16 field spellings without normalizing them
            |key| {
                fields
                    .iter()
                    .filter(|(name, _)| name.iter().copied().eq(key.encode_utf16()))
                    .count()
                    == 1
            },
        )
    {
        return None;
    }
    Some(fields)
}

/// locate one field already proven unique by the closed-object census.
fn field<'a>(fields: &'a Object, key: &str) -> Option<&'a JsonValue> {
    fields.iter().find_map(
        // compare exact original UTF-16 field units
        |(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value),
    )
}

/// borrow one exact JSON string in its JavaScript UTF-16 representation.
fn string_units<'a>(fields: &'a Object, key: &str) -> Option<&'a [u16]> {
    let JsonValue::String(value) = field(fields, key)? else {
        return None;
    };
    Some(value)
}

/// decode one exact JSON string into the typed scheduler representation.
fn text(fields: &Object, key: &str) -> Option<String> {
    String::from_utf16(string_units(fields, key)?).ok()
}

/// decode and validate one required canonical UTC instant.
fn required_instant(
    codec: &impl ForecastEvidenceReceiptCodec,
    value: &JsonValue,
) -> Option<String> {
    let JsonValue::String(value) = value else {
        return None;
    };
    let value = String::from_utf16(value).ok()?;
    required_instant_text(codec, &value)?;
    Some(value)
}

/// validate one typed canonical UTC instant with the existing runtime owner.
fn required_instant_text<'a>(
    codec: &impl ForecastEvidenceReceiptCodec,
    value: &'a str,
) -> Option<&'a str> {
    instant(codec, value)?;
    Some(value)
}

/// decode one nullable canonical UTC instant.
fn optional_instant(
    codec: &impl ForecastEvidenceReceiptCodec,
    value: &JsonValue,
) -> Option<Option<String>> {
    match value {
        JsonValue::Null => Some(None),
        _ => Some(Some(required_instant(codec, value)?)),
    }
}

/// decode one nonempty bounded opaque string with JavaScript length semantics.
fn bounded_text(value: &JsonValue) -> Option<String> {
    let JsonValue::String(units) = value else {
        return None;
    };
    // preserve the source UTF-16 length and control-unit bounds
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

/// validate one typed bounded opaque string with JavaScript length semantics.
fn valid_bounded_text(value: &str) -> bool {
    let mut length = 0;
    // count UTF-16 units and reject the same control range as the source
    for unit in value.encode_utf16() {
        length += 1;
        if matches!(unit, 0x0000..=0x001f | 0x007f) {
            return false;
        }
    }
    (1..=BOUNDED_STRING_MAXIMUM_UTF16_UNITS).contains(&length)
}

/// decode and validate one scheduler capture due key.
fn due_key(value: &JsonValue) -> Option<String> {
    let JsonValue::String(units) = value else {
        return None;
    };
    let value = String::from_utf16(units).ok()?;
    valid_due_key(&value).then_some(value)
}

/// match the source regular expression without implementing calendar validation.
fn valid_due_key(value: &str) -> bool {
    let Some(value) = value.strip_prefix("capture/") else {
        return false;
    };
    let bytes = value.as_bytes();
    // preserve the exact canonical-looking schedule syntax and four fixed hours
    bytes.len() == 24
        && bytes.iter().enumerate().all(
            // admit digits only at the source regex's numeric positions
            |(index, byte)| match index {
                4 | 7 => *byte == b'-',
                10 => *byte == b'T',
                13 | 16 => *byte == b':',
                19 => *byte == b'.',
                23 => *byte == b'Z',
                _ => byte.is_ascii_digit(),
            },
        )
        && matches!(&bytes[11..13], b"00" | b"06" | b"12" | b"18")
        && &bytes[14..19] == b"35:00"
        && &bytes[20..23] == b"000"
}

/// construct one exact JSON string value.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// construct one exact nullable JSON string value.
fn nullable_string(value: Option<&str>) -> JsonValue {
    value.map_or(JsonValue::Null, string)
}

/// construct one ordered closed object for the approved canonical JSON codec.
fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(
                // preserve the source field spellings in UTF-16 storage
                |(name, value)| (name.encode_utf16().collect(), value),
            )
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::revision_archive_canonical::RevisionArchiveCanonicalCodec;
    use crate::{ForecastEvidenceMaterialCodec, ForecastEvidenceReceiptCodec};

    const DUE_KEY: &str = "capture/2026-11-23T00:35:00.000Z";
    const COMMITTED_AT: &str = "2026-11-23T00:36:00.000Z";
    const OBSERVED_AT: &str = "2026-11-23T00:37:00.000Z";
    const UPDATED_AT: &str = "2026-11-23T00:38:00.000Z";
    const NONCANONICAL_INPUT: &[u8] = concat!(
        "{\n  \"updatedAt\" : \"2026-11-23T00:38:00.000Z\",\n  \"gaps\" : [{\"reason\":",
        "\"missed_capture\",\"occurredAt\":\"2026-11-23T00:37:00.000Z\",\"dueKey\":",
        "\"capture/2026-11-23T00:35:00.000Z\"}],\n  \"activation\" : \"active\",\n  \"errors\" : ",
        "[{\"dueKey\":\"capture/2026-11-23T00:35:00.000Z\",\"code\":\"capture_retry\",",
        "\"at\":\"2026-11-23T00:37:00.000Z\"}],\n  \"contractVersion\" : ",
        "\"adjustment-evidence-scheduler-state/v1\",\n  \"attempts\" : [{\"status\":\"committed\",",
        "\"dueKey\":\"capture/2026-11-23T00:35:00.000Z\",\"committedAt\":",
        "\"2026-11-23T00:36:00.000Z\"}],\n  \"lastCheckedAt\" : \"2026-11-23T00:37:00.000Z\"\n}\n",
    )
    .as_bytes();
    const NODE_CANONICAL_FRAME: &[u8] = concat!(
        "{\"activation\":\"active\",\"attempts\":[{\"committedAt\":\"2026-11-23T00:36:00.000Z\",",
        "\"dueKey\":\"capture/2026-11-23T00:35:00.000Z\",\"status\":\"committed\"}],",
        "\"contractVersion\":\"adjustment-evidence-scheduler-state/v1\",\"errors\":[{\"at\":",
        "\"2026-11-23T00:37:00.000Z\",\"code\":\"capture_retry\",\"dueKey\":",
        "\"capture/2026-11-23T00:35:00.000Z\"}],\"gaps\":[{\"dueKey\":",
        "\"capture/2026-11-23T00:35:00.000Z\",\"occurredAt\":\"2026-11-23T00:37:00.000Z\",",
        "\"reason\":\"missed_capture\"}],\"lastCheckedAt\":\"2026-11-23T00:37:00.000Z\",",
        "\"updatedAt\":\"2026-11-23T00:38:00.000Z\"}\n",
    )
    .as_bytes();

    struct FixtureCodec;

    impl RevisionArchiveCanonicalCodec for FixtureCodec {
        /// admit only the exact independently captured fixture instants.
        fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
            match text {
                COMMITTED_AT => Some(1_795_394_160_000),
                OBSERVED_AT => Some(1_795_394_220_000),
                UPDATED_AT => Some(1_795_394_280_000),
                _ => None,
            }
        }

        /// return an unused deterministic fixture digest.
        fn sha256(&self, _bytes: &[u8]) -> [u8; 32] {
            [0_u8; 32]
        }
    }

    impl ForecastEvidenceMaterialCodec for FixtureCodec {
        /// leave projected forecast decoding outside this fixture.
        fn decode_projected_forecast(&self, _bytes: &[u8]) -> Option<JsonValue> {
            None
        }

        /// return the independently captured Node canonical scheduler bytes.
        fn canonical_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
            let decoded = decode_state_value(self, value)?;
            let expected = decode_state_value(
                self,
                &fixture_value("active", DUE_KEY, "committed", "missed_capture"),
            )?;
            (decoded == expected)
                .then(|| NODE_CANONICAL_FRAME[..NODE_CANONICAL_FRAME.len() - 1].to_vec())
        }

        /// leave compression outside this fixture.
        fn gzip_level_nine_zero_mtime(&self, _bytes: &[u8]) -> Option<Vec<u8>> {
            None
        }

        /// leave base64 encoding outside this fixture.
        fn standard_base64(&self, _bytes: &[u8]) -> Option<String> {
            None
        }
    }

    impl ForecastEvidenceReceiptCodec for FixtureCodec {
        /// map one deliberately noncanonical frame to the parsed Node value.
        fn decode_retained_json(&self, bytes: &[u8]) -> Option<JsonValue> {
            (bytes == NONCANONICAL_INPUT)
                .then(|| fixture_value("active", DUE_KEY, "committed", "missed_capture"))
        }

        /// leave base64 decoding outside this fixture.
        fn decode_standard_base64(&self, _value: &str) -> Option<Vec<u8>> {
            None
        }

        /// leave gzip decoding outside this fixture.
        fn gunzip_bounded(&self, _bytes: &[u8], _maximum_bytes: usize) -> Option<Vec<u8>> {
            None
        }
    }

    /// build the exact parsed scheduler fixture used by the Node byte witness.
    fn fixture_value(activation: &str, due: &str, status: &str, reason: &str) -> JsonValue {
        object([
            ("updatedAt", string(UPDATED_AT)),
            (
                "gaps",
                JsonValue::Array(vec![object([
                    ("reason", string(reason)),
                    ("occurredAt", string(OBSERVED_AT)),
                    ("dueKey", string(due)),
                ])]),
            ),
            ("activation", string(activation)),
            (
                "errors",
                JsonValue::Array(vec![object([
                    ("dueKey", string(due)),
                    ("code", string("capture_retry")),
                    ("at", string(OBSERVED_AT)),
                ])]),
            ),
            (
                "contractVersion",
                string(FORECAST_SCHEDULER_STATE_CONTRACT_VERSION),
            ),
            (
                "attempts",
                JsonValue::Array(vec![object([
                    ("status", string(status)),
                    ("dueKey", string(due)),
                    ("committedAt", string(COMMITTED_AT)),
                ])]),
            ),
            ("lastCheckedAt", string(OBSERVED_AT)),
        ])
    }

    // verify JSON.parse-style reads and exact canonical Node writes
    #[test]
    fn accepts_noncanonical_read_and_emits_exact_node_frame() {
        let material = ForecastEvidenceMaterial::new(FixtureCodec);
        let state = material
            .decode_state(NONCANONICAL_INPUT)
            .expect("decoded state");
        assert_eq!(state.activation, "active");
        assert_eq!(state.attempts[0].due_key, DUE_KEY);
        assert_eq!(state.errors[0].code, "capture_retry");
        assert_eq!(
            state.gaps[0].reason,
            ForecastSchedulerGapReason::MissedCapture
        );
        assert_eq!(
            material.encode_state(&state).expect("encoded state"),
            NODE_CANONICAL_FRAME
        );
    }

    // verify exact fields, due grammar, bounded text, and categorical reason validation
    #[test]
    fn rejects_invalid_scheduler_shapes_and_values() {
        let codec = FixtureCodec;
        let mut extra_field = fixture_value("active", DUE_KEY, "committed", "missed_capture");
        // add one unknown top-level field to the otherwise complete object
        let JsonValue::Object(fields) = &mut extra_field else {
            panic!("fixture object");
        };
        fields.push(("unexpected".encode_utf16().collect(), JsonValue::Null));
        let invalid = [
            extra_field,
            fixture_value(&"x".repeat(65), DUE_KEY, "committed", "missed_capture"),
            fixture_value(
                "active",
                "capture/2026-11-23T01:35:00.000Z",
                "committed",
                "missed_capture",
            ),
            fixture_value("active", DUE_KEY, "", "missed_capture"),
            fixture_value("active", DUE_KEY, "committed", "unknown"),
        ];
        // reject every ordinary schema mismatch without invoking canonical encoding
        for value in invalid {
            assert_eq!(decode_state_value(&codec, &value), None);
        }
    }

    // verify typed writes pass through the same time and bounded-text validation
    #[test]
    fn rejects_invalid_typed_state_before_canonical_encoding() {
        let material = ForecastEvidenceMaterial::new(FixtureCodec);
        let mut state = material
            .decode_state(NONCANONICAL_INPUT)
            .expect("decoded state");
        state.errors[0].code.clear();
        assert_eq!(material.encode_state(&state), None);

        state.errors[0].code = "capture_retry".to_owned();
        state.updated_at = Some("2026-11-23T00:39:00.000Z".to_owned());
        assert_eq!(material.encode_state(&state), None);
    }
}
