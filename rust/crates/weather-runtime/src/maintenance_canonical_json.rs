//! Shared canonical document boundary for maintenance packages and the capture epoch.

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

/// Approved process serialization and digest owner for the original TS canonical documents.
pub trait MaintenanceCanonicalJson {
    /// Parse one exact sorted LF-terminated JSON object, with no duplicate or normalized keys.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue>;

    /// Encode the original recursively sorted canonical JSON with one trailing LF.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>>;

    /// Hash the provided bytes with the selected SHA-256 primitive.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest>;
}

/// Borrow exactly one known object member from a decoded TypeScript JSON value.
pub(crate) fn field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
}

/// Decode one expected canonical UTF-16 string without replacing invalid code units.
pub(crate) fn text(value: &JsonValue, key: &str) -> Option<String> {
    let JsonValue::String(value) = field(value, key)? else {
        return None;
    };
    String::from_utf16(value).ok()
}

/// Require every expected key and no extension on a parsed object.
pub(crate) fn exact_keys(value: &JsonValue, keys: &[&str]) -> bool {
    matches!(value, JsonValue::Object(fields) if fields.len() == keys.len())
        && keys.iter().all(|name| field(value, name).is_some())
}

/// Recognize the exact immutable lowercase SHA-256 spelling.
pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
