//! Exact raw UTF-16 support for serde_json strings containing lone surrogates.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::{self, Formatter, Write as _};
use std::io::{self, Write};

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;
use weather_contracts::JsonValue;

const SENTINEL: char = '\u{e000}';
const MAXIMUM_ARRAY_INDEX: u32 = u32::MAX - 1;
const MAXIMUM_FALLBACK_DEPTH: usize = 128;

/// Convert raw UTF-16 into a reversible valid string for serde serialization.
pub(crate) fn prepare_utf16(units: &[u16]) -> String {
    // keep ordinary valid strings allocation-compatible with String::from_utf16
    if let Ok(valid) = String::from_utf16(units)
        && !valid.contains(SENTINEL)
    {
        return valid;
    }
    let mut prepared = String::with_capacity(units.len());
    // retain scalar values while marking lone surrogates and marker collisions
    for scalar in char::decode_utf16(units.iter().copied()) {
        // keep the sentinel grammar disjoint from every original string
        match scalar {
            Ok(SENTINEL) => {
                prepared.push(SENTINEL);
                prepared.push(SENTINEL);
            }
            Ok(scalar) => prepared.push(scalar),
            Err(surrogate) => {
                prepared.push(SENTINEL);
                write!(&mut prepared, "{:04x}", surrogate.unpaired_surrogate())
                    .expect("writing to String cannot fail");
                prepared.push(SENTINEL);
            }
        }
    }
    prepared
}

/// Write one prepared string fragment, restoring exact JSON surrogate escapes.
pub(crate) fn write_prepared_fragment<W>(writer: &mut W, fragment: &str) -> io::Result<()>
where
    W: ?Sized + Write,
{
    // write the overwhelmingly common marker-free fragment directly
    if !fragment.contains(SENTINEL) {
        return writer.write_all(fragment.as_bytes());
    }
    let mut characters = fragment.char_indices().peekable();
    let mut ordinary_start = 0;
    // intercept only reversible sentinels inside serde_json-owned string fragments
    while let Some((offset, character)) = characters.next() {
        if character != SENTINEL {
            continue;
        }
        writer.write_all(&fragment.as_bytes()[ordinary_start..offset])?;
        let payload_start = offset + SENTINEL.len_utf8();
        // doubled sentinels represent one original private-use scalar
        if characters.peek().is_some_and(|(_, next)| *next == SENTINEL) {
            characters.next();
            writer.write_all(SENTINEL.to_string().as_bytes())?;
            ordinary_start = payload_start + SENTINEL.len_utf8();
            continue;
        }
        let payload_end = payload_start + 4;
        let Some(payload) = fragment.get(payload_start..payload_end) else {
            return Err(io::Error::other("incomplete UTF-16 surrogate sentinel"));
        };
        let Some((closing_offset, closing)) = characters.nth(4) else {
            return Err(io::Error::other("incomplete UTF-16 surrogate sentinel"));
        };
        // admit only the helper's lowercase surrogate marker spelling
        if closing_offset != payload_end
            || closing != SENTINEL
            || !payload
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(io::Error::other("invalid UTF-16 surrogate sentinel"));
        }
        let code_unit = u16::from_str_radix(payload, 16)
            .map_err(|_| io::Error::other("invalid UTF-16 surrogate sentinel"))?;
        if !(0xd800..=0xdfff).contains(&code_unit) {
            return Err(io::Error::other("non-surrogate UTF-16 sentinel"));
        }
        writer.write_all(b"\\u")?;
        writer.write_all(payload.as_bytes())?;
        ordinary_start = closing_offset + SENTINEL.len_utf8();
    }
    writer.write_all(&fragment.as_bytes()[ordinary_start..])
}

/// Parse a serde_json-validated RawValue with exact WTF-8 string retention.
pub(crate) fn decode_raw_utf16_json(bytes: &[u8]) -> Option<JsonValue> {
    let raw = serde_json::from_slice::<&RawValue>(bytes).ok()?;
    decode_raw_value(raw, 0)
}

/// Decode one validated raw token through serde_json's selected type grammar.
fn decode_raw_value(raw: &RawValue, depth: usize) -> Option<JsonValue> {
    // retain a finite recursion bound even on the uncommon second parse
    if depth > MAXIMUM_FALLBACK_DEPTH {
        return None;
    }
    let text = raw.get();
    // RawValue validates the whole token before this bounded type dispatch
    match text.as_bytes().first().copied()? {
        b'"' => serde_json::from_str::<RawUtf16>(text)
            .ok()
            .map(|value| JsonValue::String(value.0)),
        b'[' => {
            let children = serde_json::from_str::<Vec<&RawValue>>(text).ok()?;
            let mut values = Vec::with_capacity(children.len());
            // decode borrowed validated children without copying their raw bytes
            for child in children {
                values.push(decode_raw_value(child, depth.checked_add(1)?)?);
            }
            Some(JsonValue::Array(values))
        }
        b'{' => {
            let object = serde_json::from_str::<RawObject<'_>>(text).ok()?;
            let mut fields = Vec::with_capacity(object.0.len());
            // decode borrowed validated member values at the next bounded depth
            for (key, child) in object.0 {
                fields.push((key, decode_raw_value(child, depth.checked_add(1)?)?));
            }
            Some(JsonValue::Object(fields))
        }
        b't' | b'f' => serde_json::from_str::<bool>(text)
            .ok()
            .map(JsonValue::Boolean),
        b'n' => (text == "null").then_some(JsonValue::Null),
        b'-' | b'0'..=b'9' => serde_json::from_str::<f64>(text)
            .ok()
            .filter(|value| value.is_finite())
            .map(JsonValue::Number),
        _ => None,
    }
}

struct RawUtf16(Vec<u16>);

impl<'de> Deserialize<'de> for RawUtf16 {
    /// Request serde_json's documented WTF-8 representation of one JSON string.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_bytes(RawUtf16Visitor)
    }
}

struct RawUtf16Visitor;

impl Visitor<'_> for RawUtf16Visitor {
    type Value = RawUtf16;

    /// Describe a JSON string represented as valid WTF-8 bytes.
    fn expecting(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON string represented as valid WTF-8 bytes")
    }

    /// Decode borrowed serde_json WTF-8 without replacement characters.
    fn visit_borrowed_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        decode_wtf8(bytes)
            .map(RawUtf16)
            .ok_or_else(|| E::custom("serde_json string was not valid WTF-8"))
    }

    /// Decode copied serde_json WTF-8 without replacement characters.
    fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_borrowed_bytes(bytes)
    }

    /// Decode owned serde_json WTF-8 without replacement characters.
    fn visit_byte_buf<E>(self, bytes: Vec<u8>) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_borrowed_bytes(&bytes)
    }
}

struct RawObject<'a>(Vec<(Vec<u16>, &'a RawValue)>);

impl<'de> Deserialize<'de> for RawObject<'de> {
    /// Decode an object while retaining raw UTF-16 member names.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(RawObjectVisitor)
    }
}

struct RawObjectVisitor;

impl<'de> Visitor<'de> for RawObjectVisitor {
    type Value = RawObject<'de>;

    /// Describe a JSON object with WTF-8 member names.
    fn expecting(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object with WTF-8 member names")
    }

    /// Apply JSON.parse duplicate replacement and Object.keys property ordering.
    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut positions = HashMap::<Vec<u16>, usize>::new();
        let mut members = Vec::<RawMember<'de>>::with_capacity(object.size_hint().unwrap_or(0));
        // decode validated child RawValues only after their complete token is retained
        while let Some((RawUtf16(key), raw)) = object.next_entry::<RawUtf16, &'de RawValue>()? {
            // replace duplicate values without moving the first property position
            if let Some(position) = positions.get(&key).copied() {
                members[position].raw = raw;
            } else {
                let position = members.len();
                positions.insert(key.clone(), position);
                members.push(RawMember {
                    array_index: array_index(&key),
                    key,
                    raw,
                });
            }
        }
        // stable sorting moves integer indices first and retains ordinary insertion order
        members.sort_by(|left, right| property_order(left.array_index, right.array_index));
        Ok(RawObject(
            members
                .into_iter()
                .map(|member| (member.key, member.raw))
                .collect(),
        ))
    }
}

struct RawMember<'a> {
    key: Vec<u16>,
    raw: &'a RawValue,
    array_index: Option<u32>,
}

/// Compare two cached JavaScript property indices.
fn property_order(left: Option<u32>, right: Option<u32>) -> Ordering {
    // ordinary properties remain equal so stable sorting preserves insertion order
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Recognize JavaScript array-index names directly from UTF-16 code units.
pub(crate) fn array_index(key: &[u16]) -> Option<u32> {
    // exclude empty, overlong, and non-canonical leading-zero spellings
    if key.is_empty() || key.len() > 10 || (key.len() > 1 && key[0] == u16::from(b'0')) {
        return None;
    }
    let mut value = 0_u32;
    // parse only ASCII decimal units with checked arithmetic
    for unit in key {
        let digit = u8::try_from(*unit).ok()?.checked_sub(b'0')?;
        if digit > 9 {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u32::from(digit))?;
    }
    (value <= MAXIMUM_ARRAY_INDEX).then_some(value)
}

/// Decode serde_json's documented WTF-8 bytes into exact UTF-16 code units.
fn decode_wtf8(bytes: &[u8]) -> Option<Vec<u16>> {
    // ordinary valid UTF-8 is the common path
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(text.encode_utf16().collect());
    }
    let mut units = Vec::with_capacity(bytes.len());
    let mut offset = 0;
    // decode valid UTF-8 plus WTF-8's surrogate extension
    while offset < bytes.len() {
        let (code_point, width) = decode_code_point(&bytes[offset..])?;
        // preserve BMP code points, including surrogate code points, directly
        if code_point <= 0xffff {
            units.push(u16::try_from(code_point).ok()?);
        } else {
            let supplementary = code_point.checked_sub(0x1_0000)?;
            units.push(u16::try_from(0xd800 + (supplementary >> 10)).ok()?);
            units.push(u16::try_from(0xdc00 + (supplementary & 0x3ff)).ok()?);
        }
        offset = offset.checked_add(width)?;
    }
    Some(units)
}

/// Decode one UTF-8 scalar or WTF-8 lone surrogate code point.
fn decode_code_point(bytes: &[u8]) -> Option<(u32, usize)> {
    let first = *bytes.first()?;
    // preserve ASCII directly
    if first <= 0x7f {
        return Some((u32::from(first), 1));
    }
    // decode a non-overlong two-byte sequence
    if (0xc2..=0xdf).contains(&first) {
        return Some((
            (u32::from(first & 0x1f) << 6) | continuation(*bytes.get(1)?)?,
            2,
        ));
    }
    // admit three-byte UTF-8 plus ED A0..BF for WTF-8 surrogates
    if (0xe0..=0xef).contains(&first) {
        let second_byte = *bytes.get(1)?;
        if (first == 0xe0 && second_byte < 0xa0) || !(0x80..=0xbf).contains(&second_byte) {
            return None;
        }
        return Some((
            (u32::from(first & 0x0f) << 12)
                | (continuation(second_byte)? << 6)
                | continuation(*bytes.get(2)?)?,
            3,
        ));
    }
    // admit only Unicode-range four-byte sequences
    if (0xf0..=0xf4).contains(&first) {
        let second_byte = *bytes.get(1)?;
        if (first == 0xf0 && second_byte < 0x90)
            || (first == 0xf4 && second_byte > 0x8f)
            || !(0x80..=0xbf).contains(&second_byte)
        {
            return None;
        }
        return Some((
            (u32::from(first & 0x07) << 18)
                | (continuation(second_byte)? << 12)
                | (continuation(*bytes.get(2)?)? << 6)
                | continuation(*bytes.get(3)?)?,
            4,
        ));
    }
    None
}

/// Decode one UTF-8 continuation byte.
fn continuation(byte: u8) -> Option<u32> {
    (0x80..=0xbf)
        .contains(&byte)
        .then_some(u32::from(byte & 0x3f))
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use serde_json::ser::{Formatter as JsonFormatter, Serializer};

    use super::*;

    struct FixtureFormatter;

    impl JsonFormatter for FixtureFormatter {
        /// Restore prepared fragments through the same production hook.
        fn write_string_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
        where
            W: ?Sized + Write,
        {
            write_prepared_fragment(writer, fragment)
        }
    }

    /// Preserve lone surrogates in nested values, keys, and duplicate replacement.
    #[test]
    fn raw_fallback_preserves_nested_lone_surrogates() {
        let decoded = decode_raw_utf16_json(br#"{"\ud800":["\udc00",{"same":1,"same":"\ud801"}]}"#)
            .expect("raw UTF-16 JSON");
        assert_eq!(
            decoded,
            JsonValue::Object(vec![(
                vec![0xd800],
                JsonValue::Array(vec![
                    JsonValue::String(vec![0xdc00]),
                    JsonValue::Object(vec![(
                        "same".encode_utf16().collect(),
                        JsonValue::String(vec![0xd801]),
                    )]),
                ]),
            )])
        );
    }

    /// Keep marker collisions distinct from exact lowercase surrogate escapes.
    #[test]
    fn prepared_surrogate_encoding_is_reversible() {
        let prepared = prepare_utf16(&[0xe000, 0xd800, 0xe000, 0xdc00]);
        let mut encoded = Vec::new();
        prepared
            .serialize(&mut Serializer::with_formatter(
                &mut encoded,
                FixtureFormatter,
            ))
            .expect("prepared JSON string");
        assert_eq!(encoded, "\"\u{e000}\\ud800\u{e000}\\udc00\"".as_bytes());
    }

    /// Keep valid surrogate pairs on the ordinary scalar path.
    #[test]
    fn valid_pair_stays_scalar() {
        assert_eq!(prepare_utf16(&[0xd800, 0xdc00]), "\u{10000}");
    }
}
