//! Shared canonical JSON, time, and label rules for retained canary artifacts.

use weather_contracts::JsonValue;

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::maintenance_canonical_json::{field, text, valid_hash};

/// Hash one recursively sorted object after omitting only its own hash field.
pub(crate) fn canonical_object_hash_matches(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    node: &JsonValue,
    omitted: &str,
    expected: &str,
) -> Option<()> {
    let JsonValue::Object(fields) = node else {
        return None;
    };
    let material = JsonValue::Object(
        fields
            .iter()
            .filter(|(key, _)| !key.iter().copied().eq(omitted.encode_utf16()))
            .cloned()
            .collect(),
    );
    canonical_value_hash_matches(values, &material, expected)
}

/// Admit one canonical UTC clock through the approved Date-compatible owner.
pub(crate) fn canonical_clock(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    node: &JsonValue,
    key: &str,
) -> Option<i64> {
    values.canonical_utc_epoch_milliseconds(&text(node, key)?)
}

/// Read one strict lowercase SHA-256 field.
pub(crate) fn hash_field(node: &JsonValue, key: &str) -> Option<String> {
    let value = text(node, key)?;
    valid_hash(&value).then_some(value)
}

/// Read one finite JSON number.
pub(crate) fn number_field(node: &JsonValue, key: &str) -> Option<f64> {
    let JsonValue::Number(value) = field(node, key)? else {
        return None;
    };
    value.is_finite().then_some(*value)
}

/// Read one exactly represented JSON integer.
pub(crate) fn integer_field(node: &JsonValue, key: &str) -> Option<i64> {
    let value = number_field(node, key)?;
    // retain only finite safe values whose binary64 representation is integral
    if value.fract() != 0.0 || value < i64::MIN as f64 || value > i64::MAX as f64 {
        return None;
    }
    Some(value as i64)
}

/// Read one required JSON boolean.
pub(crate) fn boolean_field(node: &JsonValue, key: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(node, key)? else {
        return None;
    };
    Some(*value)
}

/// Compare one required text field with an exact ASCII identity.
pub(crate) fn text_equals(node: &JsonValue, key: &str, expected: &str) -> bool {
    text(node, key).is_some_and(|value| value == expected)
}

/// Validate one bounded public or operator label with the retained protection rules.
pub(crate) fn validate_text_field(node: &JsonValue, key: &str) -> Option<()> {
    let JsonValue::String(units) = field(node, key)? else {
        return None;
    };
    // match JavaScript trim and UTF-16 length while excluding credential-like material
    if !(1..=256).contains(&units.len())
        || units.first().is_some_and(|unit| js_trim_unit(*unit))
        || units.last().is_some_and(|unit| js_trim_unit(*unit))
        || units.iter().any(|unit| matches!(*unit, 0x000a | 0x000d))
        || contains_ascii_units(units, b"://")
        || contains_credential_label(units)
    {
        return None;
    }
    Some(())
}

/// Match the exact ECMAScript trim code points representable as one UTF-16 unit.
pub(crate) const fn js_trim_unit(unit: u16) -> bool {
    matches!(
        unit,
        0x0009..=0x000d
            | 0x0020
            | 0x00a0
            | 0x1680
            | 0x2000..=0x200a
            | 0x2028
            | 0x2029
            | 0x202f
            | 0x205f
            | 0x3000
            | 0xfeff
    )
}

/// Find one exact ASCII sequence in raw UTF-16 code units.
fn contains_ascii_units(units: &[u16], needle: &[u8]) -> bool {
    units.windows(needle.len()).any(|window| {
        window
            .iter()
            .copied()
            .zip(needle.iter().copied())
            .all(|(unit, byte)| unit == u16::from(byte))
    })
}

/// Detect the exact case-insensitive credential-like words retained by the Node guard.
fn contains_credential_label(units: &[u16]) -> bool {
    const TERMS: &[&[u8]] = &[
        b"credential",
        b"password",
        b"privatekey",
        b"private-key",
        b"private_key",
        b"private key",
        b"secret",
        b"token",
    ];
    TERMS
        .iter()
        .any(|term| (0..units.len()).any(|start| credential_term_matches(units, start, term)))
}

/// Match one protected term with JavaScript Unicode word boundaries.
fn credential_term_matches(units: &[u16], start: usize, term: &[u8]) -> bool {
    let end = start.saturating_add(term.len());
    // require both regex word boundaries and every literal term unit
    if end > units.len()
        || start
            .checked_sub(1)
            .is_some_and(|index| regex_word_unit(units[index]))
        || units.get(end).is_some_and(|unit| regex_word_unit(*unit))
    {
        return false;
    }
    units[start..end]
        .iter()
        .copied()
        .zip(term.iter().copied())
        .all(|(unit, expected)| case_insensitive_ascii_unit(unit, expected))
}

/// Compare one UTF-16 unit using ECMAScript Unicode folding relevant to ASCII terms.
fn case_insensitive_ascii_unit(unit: u16, expected: u8) -> bool {
    // punctuation in the private-key variants remains exact
    if !expected.is_ascii_alphabetic() {
        return unit == u16::from(expected);
    }
    ascii_folded_unit(unit) == Some(expected)
}

/// Fold the ASCII letters plus the two Unicode characters ECMAScript folds into ASCII.
const fn ascii_folded_unit(unit: u16) -> Option<u8> {
    match unit {
        0x0041..=0x005a => Some((unit as u8) + 32),
        0x0061..=0x007a => Some(unit as u8),
        0x017f => Some(b's'),
        0x212a => Some(b'k'),
        _ => None,
    }
}

/// Recognize JavaScript Unicode-regex word units at a candidate boundary.
const fn regex_word_unit(unit: u16) -> bool {
    matches!(unit, 0x0030..=0x0039 | 0x0041..=0x005a | 0x005f | 0x0061..=0x007a | 0x017f | 0x212a)
}

/// Compare two exact canonical values without depending on parser object order.
pub(crate) fn canonical_fields_equal(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    left: &JsonValue,
    right: &JsonValue,
) -> Option<()> {
    (values.encode_shadow_sorted_json(left)? == values.encode_shadow_sorted_json(right)?)
        .then_some(())
}

/// Check a decoded member against its source-defined canonical address.
pub(crate) fn canonical_value_hash_matches(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    value: &JsonValue,
    expected: &str,
) -> Option<()> {
    let encoded = values.encode_shadow_sorted_json(value)?;
    (values.sha256(&encoded)?.as_str() == expected).then_some(())
}
