//! Shared field admission for the native aggregate scorecard contracts.

use std::fmt::{self, Display};

use weather_contracts::JsonValue;

use crate::maintenance_canonical_json::{exact_keys, field, valid_hash};

const NULL: JsonValue = JsonValue::Null;

pub(crate) type JsonEntries = [(Vec<u16>, JsonValue)];

/// Closed scorecard validation failure with the predecessor diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScorecardContractError(String);

impl ScorecardContractError {
    /// preserve an ordinary scorecard diagnostic
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for ScorecardContractError {
    /// return the retained field or domain diagnostic
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScorecardContractError {}

/// Approved UTC parser and formatter used by the existing publication interval.
pub trait ScorecardClock {
    /// return the epoch millisecond value admitted by the original date parser
    fn utc_epoch_milliseconds(&self, value: &str) -> Option<i64>;

    /// format one epoch millisecond in the original canonical utc spelling
    fn canonical_utc(&self, milliseconds: i64) -> Option<String>;
}

/// Borrow one member from an admitted key set.
pub(crate) fn member<'a>(value: &'a JsonValue, key: &str) -> &'a JsonValue {
    field(value, key).unwrap_or(&NULL)
}

/// Admit one plain json object.
pub(crate) fn require_object<'a>(
    value: &'a JsonValue,
    path: &str,
) -> Result<&'a JsonEntries, ScorecardContractError> {
    let JsonValue::Object(fields) = value else {
        return Err(ScorecardContractError::new(format!(
            "{path} must be an object"
        )));
    };
    Ok(fields)
}

/// Require the original sorted-independent key set.
pub(crate) fn require_keys(
    value: &JsonValue,
    expected: &[&str],
    path: &str,
) -> Result<(), ScorecardContractError> {
    // reject absent or extra fields without changing their insertion order
    if !exact_keys(value, expected) {
        return Err(ScorecardContractError::new(format!(
            "{path} has invalid keys"
        )));
    }
    Ok(())
}

/// Admit one bounded array with an optional exact cardinality.
pub(crate) fn require_array<'a>(
    value: &'a JsonValue,
    maximum_length: usize,
    path: &str,
    exact_length: Option<usize>,
) -> Result<&'a [JsonValue], ScorecardContractError> {
    let JsonValue::Array(entries) = value else {
        return Err(invalid(path));
    };
    // bind both the upper constraint and the optional exact size
    if entries.len() > maximum_length || exact_length.is_some_and(|length| length != entries.len())
    {
        return Err(invalid(path));
    }
    Ok(entries)
}

/// Require an exact scalar invariant.
pub(crate) fn require_equal(
    value: &JsonValue,
    expected: &JsonValue,
    path: &str,
) -> Result<(), ScorecardContractError> {
    // preserve the predecessor's scalar strict equality
    if value != expected || matches!(value, JsonValue::Array(_) | JsonValue::Object(_)) {
        return Err(invalid(path));
    }
    Ok(())
}

/// Require a member of the original finite string enum.
pub(crate) fn require_enum(
    value: &JsonValue,
    allowed: &[&str],
    path: &str,
) -> Result<(), ScorecardContractError> {
    require_match(value, path, |value| allowed.contains(&value))
}

/// Require a string with the supplied narrow wire syntax.
pub(crate) fn require_match(
    value: &JsonValue,
    path: &str,
    predicate: impl FnOnce(&str) -> bool,
) -> Result<(), ScorecardContractError> {
    let JsonValue::String(units) = value else {
        return Err(invalid(path));
    };
    let value = String::from_utf16(units).map_err(|_| invalid(path))?;
    // reject nonmatching strings without substituting a normalized value
    if !predicate(&value) {
        return Err(invalid(path));
    }
    Ok(())
}

/// Require the original immutable hash spelling.
pub(crate) fn require_sha256(value: &JsonValue, path: &str) -> Result<(), ScorecardContractError> {
    require_match(value, path, valid_hash)
}

/// Require a fixed digest or an explicit unavailable value.
pub(crate) fn require_nullable_sha256(
    value: &JsonValue,
    path: &str,
) -> Result<(), ScorecardContractError> {
    // keep unavailable identities as null
    if *value != NULL {
        require_sha256(value, path)?;
    }
    Ok(())
}

/// Require one canonical utc instant and retain its parsed value.
pub(crate) fn require_instant(
    value: &JsonValue,
    path: &str,
    clock: &impl ScorecardClock,
) -> Result<i64, ScorecardContractError> {
    let JsonValue::String(units) = value else {
        return Err(invalid(path));
    };
    let value = String::from_utf16(units).map_err(|_| invalid(path))?;
    // reject non-utc spellings before invoking the date owner
    if !value.ends_with('Z') {
        return Err(invalid(path));
    }
    let instant = clock
        .utc_epoch_milliseconds(&value)
        .ok_or_else(|| invalid(path))?;
    // verify that parsing did not silently normalize the original instant
    if clock.canonical_utc(instant).as_deref() != Some(value.as_str()) {
        return Err(invalid(path));
    }
    Ok(instant)
}

/// Require a canonical utc instant or null.
pub(crate) fn require_nullable_instant(
    value: &JsonValue,
    path: &str,
    clock: &impl ScorecardClock,
) -> Result<(), ScorecardContractError> {
    // preserve explicit null without invoking the date owner
    if *value != NULL {
        require_instant(value, path, clock)?;
    }
    Ok(())
}

/// Require one ordinary finite value or null.
pub(crate) fn require_nullable_finite(
    value: &JsonValue,
    path: &str,
) -> Result<Option<f64>, ScorecardContractError> {
    // keep unavailable metrics distinct from nonfinite ones
    match value {
        JsonValue::Null => Ok(None),
        JsonValue::Number(number) if number.is_finite() => Ok(Some(*number)),
        _ => Err(ScorecardContractError::new(format!(
            "{path} must be finite or null"
        ))),
    }
}

/// Require one ordinary finite aggregate.
pub(crate) fn require_finite(value: &JsonValue, path: &str) -> Result<f64, ScorecardContractError> {
    // distinguish ordinary numeric values from all other wire types
    match value {
        JsonValue::Number(number) if number.is_finite() => Ok(*number),
        _ => Err(ScorecardContractError::new(format!(
            "{path} must be finite"
        ))),
    }
}

/// Require a finite nonnegative value.
pub(crate) fn require_nonnegative_finite(
    value: &JsonValue,
    path: &str,
) -> Result<f64, ScorecardContractError> {
    let value = require_finite(value, path)?;
    // preserve the original distinct negative diagnostic
    if value < 0.0 {
        return Err(ScorecardContractError::new(format!(
            "{path} must be nonnegative"
        )));
    }
    Ok(value)
}

/// Require one aggregate count at the original integer ceiling.
pub(crate) fn require_count(value: &JsonValue, path: &str) -> Result<u64, ScorecardContractError> {
    // admit only exact nonnegative integers within the retained range
    match value {
        JsonValue::Number(number)
            if number.fract() == 0.0 && (0.0..=1_000_000_000.0).contains(number) =>
        {
            Ok(*number as u64)
        }
        _ => Err(ScorecardContractError::new(format!(
            "{path} must be a bounded count"
        ))),
    }
}

/// Build one exact field-invalid diagnostic.
fn invalid(path: &str) -> ScorecardContractError {
    ScorecardContractError::new(format!("{path} is invalid"))
}
