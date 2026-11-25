//! Native JavaScript String coercion for generated JSON values.

use weather_contracts::JsonValue;

use crate::{MaintenanceAnchorStatusJson, NativeWirePrimitives, native_json::encode_ordered_json};

impl MaintenanceAnchorStatusJson for NativeWirePrimitives {
    /// reproduce the predecessor's JavaScript String conversion without a Node process.
    fn js_string(&mut self, value: &JsonValue) -> Option<Vec<u16>> {
        js_string_units(value)
    }
}

/// coerce one generated value with JavaScript's ordinary String rules.
fn js_string_units(value: &JsonValue) -> Option<Vec<u16>> {
    // preserve raw strings and scalar spellings before recursive composites
    match value {
        JsonValue::Boolean(value) => Some(utf16(if *value { "true" } else { "false" })),
        JsonValue::Number(value) => number_units(*value),
        JsonValue::String(units) => Some(units.clone()),
        JsonValue::Null => Some(utf16("null")),
        JsonValue::Array(values) => array_units(values),
        JsonValue::Object(fields) => {
            // json cannot provide a callable own conversion method
            if fields.iter().any(|(key, _)| is_to_string(key)) {
                return None;
            }
            Some(utf16("[object Object]"))
        }
    }
}

/// render one number with JavaScript's String spelling.
fn number_units(value: f64) -> Option<Vec<u16>> {
    // retain the javascript non-finite names excluded from json numbers
    if value.is_nan() {
        return Some(utf16("NaN"));
    }
    // retain positive infinity
    if value == f64::INFINITY {
        return Some(utf16("Infinity"));
    }
    // retain negative infinity
    if value == f64::NEG_INFINITY {
        return Some(utf16("-Infinity"));
    }
    let encoded = encode_ordered_json(&JsonValue::Number(value))?;
    let digits = encoded.strip_suffix(b"\n")?;
    // finite javascript number spellings are ascii
    Some(digits.iter().map(|byte| u16::from(*byte)).collect())
}

/// reproduce Array.prototype.join with its empty null fields.
fn array_units(values: &[JsonValue]) -> Option<Vec<u16>> {
    let mut result = Vec::new();
    // separate every array position, including null positions
    for (index, value) in values.iter().enumerate() {
        // insert the join separator after the first position
        if index != 0 {
            result.push(u16::from(b','));
        }
        // join renders null elements as empty fields
        if matches!(value, JsonValue::Null) {
            continue;
        }
        result.extend(js_string_units(value)?);
    }
    Some(result)
}

/// identify the own property that shadows Object.prototype.toString.
fn is_to_string(key: &[u16]) -> bool {
    key.iter().copied().eq("toString".encode_utf16())
}

/// encode one fixed scalar spelling as UTF-16 units.
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

impl crate::CaptureEpochInitJson for NativeWirePrimitives {
    /// Decode the original bounded database envelope without imposing canonical input framing.
    fn parse_database_envelope(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        weather_edge::ArchiveWirePrimitives::decode_json(self, bytes)
    }
}

impl crate::ShadowMetadataCustodyOwnerJson for NativeWirePrimitives {
    /// Preserve ordinary owner JSON parsing before its typed contract admission.
    fn parse_owner_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        weather_edge::ArchiveWirePrimitives::decode_json(self, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// preserve raw UTF-16 strings and every JavaScript numeric spelling.
    #[test]
    fn scalars_match_javascript_string_conversion() {
        let mut owner = NativeWirePrimitives::new();
        let raw = vec![0xd800, u16::from(b'x'), 0xdc00];

        assert_eq!(owner.js_string(&JsonValue::String(raw.clone())), Some(raw));
        assert_eq!(
            owner.js_string(&JsonValue::Boolean(true)),
            Some(utf16("true"))
        );
        assert_eq!(owner.js_string(&JsonValue::Null), Some(utf16("null")));
        assert_eq!(owner.js_string(&JsonValue::Number(-0.0)), Some(utf16("0")));
        assert_eq!(
            owner.js_string(&JsonValue::Number(1e21)),
            Some(utf16("1e+21"))
        );
        assert_eq!(
            owner.js_string(&JsonValue::Number(f64::NAN)),
            Some(utf16("NaN"))
        );
        assert_eq!(
            owner.js_string(&JsonValue::Number(f64::NEG_INFINITY)),
            Some(utf16("-Infinity"))
        );
    }

    /// join nested arrays while leaving null elements empty.
    #[test]
    fn arrays_match_javascript_join_conversion() {
        let mut owner = NativeWirePrimitives::new();
        let value = JsonValue::Array(vec![
            JsonValue::String(utf16("alpha")),
            JsonValue::Null,
            JsonValue::Boolean(false),
            JsonValue::Array(vec![JsonValue::Number(2.0), JsonValue::Null]),
            JsonValue::Object(Vec::new()),
        ]);

        assert_eq!(
            owner.js_string(&value),
            Some(utf16("alpha,,false,2,,[object Object]"))
        );
    }

    /// retain default objects and reject a non-callable own toString property.
    #[test]
    fn objects_match_javascript_primitive_conversion() {
        let mut owner = NativeWirePrimitives::new();
        let ordinary = JsonValue::Object(vec![(utf16("value"), JsonValue::Number(1.0))]);
        let shadowed = JsonValue::Object(vec![(utf16("toString"), JsonValue::Null)]);

        assert_eq!(owner.js_string(&ordinary), Some(utf16("[object Object]")));
        assert_eq!(owner.js_string(&shadowed), None);
        assert_eq!(owner.js_string(&JsonValue::Array(vec![shadowed])), None);
    }
}
