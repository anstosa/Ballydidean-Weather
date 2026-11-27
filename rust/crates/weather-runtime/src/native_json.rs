//! Native JSON parsing and serialization over the generated recursive value contract.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt::{self, Formatter};
use std::io::{self, Write};

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Error as _, Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::ser::{CompactFormatter, Formatter as JsonFormatter};
use weather_contracts::JsonValue;

use crate::native_json_utf16::{
    array_index, decode_raw_utf16_json, prepare_utf16, write_prepared_fragment,
};

const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const LINEAR_DUPLICATE_LIMIT: usize = 16;

/// Stateless serde-backed ownership of the generated JSON value boundary.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeJson;

impl NativeJson {
    /// Decode one complete JSON document and retain JavaScript object-key order.
    pub fn decode(bytes: &[u8]) -> Option<JsonValue> {
        serde_json::from_slice::<DecodedJson>(bytes)
            .ok()
            .map(|decoded| decoded.0)
            .or_else(|| decode_raw_utf16_json(bytes))
    }

    /// Encode one producer-ordered document with exactly one trailing line feed.
    pub fn encode_ordered(value: &JsonValue) -> Option<Vec<u8>> {
        encode(value, ObjectOrder::Producer)
    }

    /// Encode one recursively UTF-16-sorted document with exactly one trailing line feed.
    pub fn encode_sorted(value: &JsonValue) -> Option<Vec<u8>> {
        encode(value, ObjectOrder::Canonical)
    }
}

/// Decode one complete serde_json document into generated recursive storage.
pub(crate) fn decode_json(bytes: &[u8]) -> Option<JsonValue> {
    NativeJson::decode(bytes)
}

/// Encode one producer-ordered serde_json document with exactly one trailing line feed.
pub(crate) fn encode_ordered_json(value: &JsonValue) -> Option<Vec<u8>> {
    NativeJson::encode_ordered(value)
}

/// Encode one recursively sorted serde_json document with exactly one trailing line feed.
pub(crate) fn encode_sorted_json(value: &JsonValue) -> Option<Vec<u8>> {
    NativeJson::encode_sorted(value)
}

#[derive(Clone, Copy)]
enum ObjectOrder {
    Producer,
    Canonical,
}

struct EncodedJson<'a> {
    value: &'a JsonValue,
    object_order: ObjectOrder,
}

impl Serialize for EncodedJson<'_> {
    /// Stream one generated value directly into serde_json without an intermediate tree.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // preserve the closed generated value alternatives
        match self.value {
            JsonValue::Boolean(value) => serializer.serialize_bool(*value),
            JsonValue::Number(value) => serialize_number(*value, serializer),
            JsonValue::String(units) => {
                let value = prepare_utf16(units);
                serializer.serialize_str(&value)
            }
            JsonValue::Null => serializer.serialize_none(),
            JsonValue::Array(values) => {
                let mut sequence = serializer.serialize_seq(Some(values.len()))?;
                // serialize each child through the same recursive ordering mode
                for value in values {
                    sequence.serialize_element(&Self {
                        value,
                        object_order: self.object_order,
                    })?;
                }
                sequence.end()
            }
            JsonValue::Object(fields) => {
                let fields = object_fields(fields, self.object_order).ok_or_else(|| {
                    S::Error::custom("JSON object has duplicate or invalid UTF-16 keys")
                })?;
                let mut object = serializer.serialize_map(Some(fields.len()))?;
                // emit the selected JavaScript property order without a map allocation
                for field in fields {
                    object.serialize_entry(
                        &field.key,
                        &Self {
                            value: field.value,
                            object_order: self.object_order,
                        },
                    )?;
                }
                object.end()
            }
        }
    }
}

/// Serialize JavaScript numbers with JSON.stringify's non-finite null behavior.
fn serialize_number<S>(value: f64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    // preserve JSON.stringify's null projection for non-finite runtime numbers
    if !value.is_finite() {
        return serializer.serialize_none();
    }
    // normalize both JavaScript zero representations to JSON.stringify's `0`
    if value == 0.0 {
        return serializer.serialize_i64(0);
    }
    // use integer serialization where the f64 has an exact JavaScript-safe identity
    if value.fract() == 0.0 && value.abs() <= MAXIMUM_SAFE_INTEGER {
        return serializer.serialize_i64(value as i64);
    }
    serializer.serialize_f64(value)
}

struct ObjectField<'a> {
    key: String,
    units: &'a [u16],
    value: &'a JsonValue,
    array_index: Option<u32>,
}

/// Validate keys once and select JavaScript's integer-index-aware property order.
fn object_fields<'a>(
    fields: &'a [(Vec<u16>, JsonValue)],
    order: ObjectOrder,
) -> Option<Vec<ObjectField<'a>>> {
    let mut selected = Vec::with_capacity(fields.len());
    let mut seen = (fields.len() > LINEAR_DUPLICATE_LIMIT)
        .then(|| HashSet::<&[u16]>::with_capacity(fields.len()));
    // retain the generated UTF-16 identity while rejecting duplicate object members
    for (position, (units, value)) in fields.iter().enumerate() {
        let duplicate = if let Some(seen) = &mut seen {
            !seen.insert(units.as_slice())
        } else {
            fields[..position]
                .iter()
                .any(|(candidate, _)| candidate == units)
        };
        if duplicate {
            return None;
        }
        let key = prepare_utf16(units);
        selected.push(ObjectField {
            array_index: array_index(units),
            key,
            units,
            value,
        });
    }
    // stable sorting retains producer order for ordinary keys
    selected.sort_by(|left, right| object_field_order(left, right, order));
    Some(selected)
}

/// Compare two properties using Object.keys order plus the selected ordinary-key policy.
fn object_field_order(
    left: &ObjectField<'_>,
    right: &ObjectField<'_>,
    order: ObjectOrder,
) -> Ordering {
    // integer indices always precede ordinary string keys in JavaScript objects
    match (left.array_index, right.array_index) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => {
            // canonical Object.keys(...).sort() compares raw UTF-16 code units
            match order {
                ObjectOrder::Producer => Ordering::Equal,
                ObjectOrder::Canonical => left.units.cmp(right.units),
            }
        }
    }
}

/// Serialize one complete value and add the archive framing line feed.
fn encode(value: &JsonValue, object_order: ObjectOrder) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, JsNumberFormatter);
    EncodedJson {
        value,
        object_order,
    }
    .serialize(&mut serializer)
    .ok()?;
    bytes.push(b'\n');
    Some(bytes)
}

struct JsNumberFormatter;

impl JsonFormatter for JsNumberFormatter {
    /// Apply JSON.stringify's decimal/exponent thresholds to serde_json's shortest digits.
    fn write_f64<W>(&mut self, writer: &mut W, value: f64) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        let mut shortest = Vec::with_capacity(24);
        JsonFormatter::write_f64(&mut CompactFormatter, &mut shortest, value)?;
        let shortest = std::str::from_utf8(&shortest)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let javascript = javascript_number(shortest)
            .ok_or_else(|| io::Error::other("serde_json emitted an invalid finite number"))?;
        writer.write_all(javascript.as_bytes())
    }

    /// Restore reversible lone-surrogate markers only inside JSON string fragments.
    fn write_string_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        write_prepared_fragment(writer, fragment)
    }
}

/// Reframe serde_json's shortest round-trip digits using ECMAScript notation thresholds.
fn javascript_number(shortest: &str) -> Option<String> {
    let (negative, unsigned) = shortest
        .strip_prefix('-')
        .map_or((false, shortest), |value| (true, value));
    // scientific notation directly identifies the first digit's decimal position
    let (coefficient, decimal_position) =
        if let Some((coefficient, exponent)) = unsigned.split_once(['e', 'E']) {
            let exponent = exponent.parse::<i32>().ok()?;
            (coefficient, exponent.checked_add(1)?)
        } else {
            let decimal_position = unsigned.find('.').unwrap_or(unsigned.len()) as i32;
            (unsigned, decimal_position)
        };
    let mut digits = coefficient
        .bytes()
        .filter(|byte| *byte != b'.')
        .collect::<Vec<_>>();
    let leading_zeroes = digits.iter().take_while(|byte| **byte == b'0').count();
    // fixed notation's leading zeros move the significant decimal position left
    let decimal_position = if unsigned.contains(['e', 'E']) {
        decimal_position
    } else {
        decimal_position.checked_sub(i32::try_from(leading_zeroes).ok()?)?
    };
    digits.drain(..leading_zeroes);
    // insignificant trailing zeros are reconstructed from the decimal position
    while digits.last() == Some(&b'0') {
        digits.pop();
    }
    if digits.is_empty() {
        return Some("0".to_owned());
    }
    let digits = std::str::from_utf8(&digits).ok()?;
    format_javascript_number(negative, digits, decimal_position)
}

/// Render shortest digits with JSON.stringify's fixed and exponential notation selection.
fn format_javascript_number(negative: bool, digits: &str, decimal_position: i32) -> Option<String> {
    let digit_count = i32::try_from(digits.len()).ok()?;
    let mut rendered = String::with_capacity(digits.len().saturating_add(16));
    // retain the sign only for a nonzero finite value
    if negative {
        rendered.push('-');
    }
    // append zeros when every significant digit precedes the decimal point
    if digit_count <= decimal_position && decimal_position <= 21 {
        rendered.push_str(digits);
        for _ in 0..usize::try_from(decimal_position - digit_count).ok()? {
            rendered.push('0');
        }
        return Some(rendered);
    }
    // insert a decimal point within the significant digits
    if 0 < decimal_position && decimal_position <= 21 {
        let decimal_position = usize::try_from(decimal_position).ok()?;
        rendered.push_str(&digits[..decimal_position]);
        rendered.push('.');
        rendered.push_str(&digits[decimal_position..]);
        return Some(rendered);
    }
    // use leading fractional zeros through the inclusive 10^-6 boundary
    if -6 < decimal_position && decimal_position <= 0 {
        rendered.push_str("0.");
        for _ in 0..usize::try_from(-decimal_position).ok()? {
            rendered.push('0');
        }
        rendered.push_str(digits);
        return Some(rendered);
    }
    let exponent = decimal_position.checked_sub(1)?;
    rendered.push(char::from(digits.as_bytes()[0]));
    // omit the decimal point for a one-digit coefficient
    if digits.len() > 1 {
        rendered.push('.');
        rendered.push_str(&digits[1..]);
    }
    rendered.push('e');
    // JavaScript includes an explicit sign for positive exponents
    if exponent >= 0 {
        rendered.push('+');
    }
    rendered.push_str(&exponent.to_string());
    Some(rendered)
}

struct DecodedJson(JsonValue);

impl<'de> Deserialize<'de> for DecodedJson {
    /// Decode through serde_json's grammar while retaining object entries as ordered pairs.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(DecodedJsonVisitor)
    }
}

struct DecodedJsonVisitor;

impl<'de> Visitor<'de> for DecodedJsonVisitor {
    type Value = DecodedJson;

    /// Describe the generated recursive JSON grammar.
    fn expecting(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("a finite recursive JSON value")
    }

    /// Retain a decoded JSON null.
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(DecodedJson(JsonValue::Null))
    }

    /// Retain a decoded JSON boolean.
    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(DecodedJson(JsonValue::Boolean(value)))
    }

    /// Convert signed JSON integers through JavaScript's binary64 number domain.
    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(DecodedJson(JsonValue::Number(value as f64)))
    }

    /// Convert unsigned JSON integers through JavaScript's binary64 number domain.
    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(DecodedJson(JsonValue::Number(value as f64)))
    }

    /// Retain only finite JSON numbers admitted by the generated contract.
    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        // reject parser overflow instead of retaining a non-JSON runtime number
        if !value.is_finite() {
            return Err(E::custom("JSON number is not finite"));
        }
        Ok(DecodedJson(JsonValue::Number(value)))
    }

    /// Preserve one decoded Unicode string as its exact UTF-16 code units.
    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(DecodedJson(JsonValue::String(
            value.encode_utf16().collect(),
        )))
    }

    /// Preserve one owned Unicode string as its exact UTF-16 code units.
    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(DecodedJson(JsonValue::String(
            value.encode_utf16().collect(),
        )))
    }

    /// Decode one array without an intermediate serde_json value tree.
    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0));
        // consume each recursive child in source order
        while let Some(value) = sequence.next_element::<DecodedJson>()? {
            values.push(value.0);
        }
        Ok(DecodedJson(JsonValue::Array(values)))
    }

    /// Decode one object with JSON.parse's duplicate replacement and JavaScript key order.
    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut positions = HashMap::<Vec<u16>, usize>::new();
        let mut fields = Vec::<DecodedField>::with_capacity(object.size_hint().unwrap_or(0));
        // consume decoded keys before replacing duplicate members in their first position
        while let Some((key, value)) = object.next_entry::<String, DecodedJson>()? {
            let units = key.encode_utf16().collect::<Vec<_>>();
            if let Some(position) = positions.get(&units).copied() {
                fields[position].value = value.0;
            } else {
                let position = fields.len();
                positions.insert(units.clone(), position);
                fields.push(DecodedField {
                    array_index: array_index(&units),
                    units,
                    value: value.0,
                });
            }
        }
        // match Object.keys order for integer-index properties and retain other source order
        fields.sort_by(|left, right| property_index_order(left.array_index, right.array_index));
        Ok(DecodedJson(JsonValue::Object(
            fields
                .into_iter()
                .map(|field| (field.units, field.value))
                .collect(),
        )))
    }
}

struct DecodedField {
    units: Vec<u16>,
    value: JsonValue,
    array_index: Option<u32>,
}

/// Compare cached JavaScript array indices while leaving ordinary fields stable.
fn property_index_order(left: Option<u32>, right: Option<u32>) -> Ordering {
    // ordinary decoded keys remain stable in their source order
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode one fixture object member.
    fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
        (key.encode_utf16().collect(), value)
    }

    /// Encode one fixture string value.
    fn text(value: &str) -> JsonValue {
        JsonValue::String(value.encode_utf16().collect())
    }

    /// Decode objects into the same integer-index-aware order exposed by Object.keys.
    #[test]
    fn decode_preserves_javascript_property_order() {
        let value =
            NativeJson::decode(br#"{"b":true,"10":10,"2":2,"a":null}"#).expect("valid JSON");
        assert_eq!(
            value,
            JsonValue::Object(vec![
                member("2", JsonValue::Number(2.0)),
                member("10", JsonValue::Number(10.0)),
                member("b", JsonValue::Boolean(true)),
                member("a", JsonValue::Null),
            ])
        );
    }

    /// Keep producer order for ordinary keys while ordering integer indices numerically.
    #[test]
    fn ordered_encoding_matches_javascript_property_order() {
        let value = JsonValue::Object(vec![
            member("b", JsonValue::Boolean(true)),
            member("10", JsonValue::Number(10.0)),
            member("2", JsonValue::Number(2.0)),
            member("a", JsonValue::Null),
        ]);
        assert_eq!(
            NativeJson::encode_ordered(&value).as_deref(),
            Some(
                br#"{"2":2,"10":10,"b":true,"a":null}
"#
                .as_slice()
            )
        );
    }

    /// Sort ordinary keys by raw UTF-16 while retaining JavaScript integer indices first.
    #[test]
    fn sorted_encoding_uses_utf16_code_unit_order_recursively() {
        let value = JsonValue::Object(vec![
            member("\u{e000}", JsonValue::Null),
            member("10", JsonValue::Number(10.0)),
            member("\u{10000}", JsonValue::Null),
            member("2", JsonValue::Number(2.0)),
            member(
                "b",
                JsonValue::Object(vec![member("z", text("last")), member("a", text("first"))]),
            ),
        ]);
        assert_eq!(
            NativeJson::encode_sorted(&value),
            Some("{\"2\":2,\"10\":10,\"b\":{\"a\":\"first\",\"z\":\"last\"},\"𐀀\":null,\"\":null}\n".as_bytes().to_vec())
        );
    }

    /// Preserve lone UTF-16 surrogates while rejecting duplicate stored keys.
    #[test]
    fn encoding_preserves_lone_surrogates_and_rejects_duplicate_keys() {
        assert_eq!(
            NativeJson::encode_ordered(&JsonValue::String(vec![0xd800])).as_deref(),
            Some(b"\"\\ud800\"\n".as_slice())
        );
        assert_eq!(
            NativeJson::encode_ordered(&JsonValue::Object(vec![
                member("same", JsonValue::Null),
                member("same", JsonValue::Boolean(true)),
            ])),
            None
        );
        assert_eq!(
            NativeJson::encode_ordered(&JsonValue::Number(f64::NAN)).as_deref(),
            Some(b"null\n".as_slice())
        );
    }

    /// Preserve lone surrogates and marker collisions in both keys and nested values.
    #[test]
    fn surrogate_keys_and_values_round_trip_without_marker_collisions() {
        let value = JsonValue::Object(vec![(
            vec![0xd800],
            JsonValue::Array(vec![JsonValue::String(vec![0xe000, 0xdc00])]),
        )]);
        let encoded = NativeJson::encode_ordered(&value).expect("surrogate JSON");
        assert_eq!(encoded, "{\"\\ud800\":[\"\u{e000}\\udc00\"]}\n".as_bytes());
        assert_eq!(NativeJson::decode(&encoded), Some(value));
    }

    /// Preserve common JSON.stringify number spellings without `.0` or negative zero.
    #[test]
    fn encoding_matches_common_javascript_number_spelling() {
        let value = JsonValue::Array(vec![
            JsonValue::Number(1.0),
            JsonValue::Number(-0.0),
            JsonValue::Number(2.5),
            JsonValue::Number(1e-6),
            JsonValue::Number(1e-7),
            JsonValue::Number(1e20),
            JsonValue::Number(1e21),
            JsonValue::Number(-1e21),
            JsonValue::Number(f64::from_bits(0x3ff0_0008_0000_0000)),
        ]);
        assert_eq!(
            NativeJson::encode_ordered(&value).as_deref(),
            Some(
                b"[1,0,2.5,0.000001,1e-7,100000000000000000000,1e+21,-1e+21,1.0000076293945312]\n"
                    .as_slice()
            )
        );
    }

    /// Match JSON.parse by replacing duplicate members without moving their first position.
    #[test]
    fn decode_uses_last_duplicate_value_in_first_member_position() {
        assert_eq!(
            NativeJson::decode(br#"{"same":1,"other":true,"same":2}"#),
            Some(JsonValue::Object(vec![
                member("same", JsonValue::Number(2.0)),
                member("other", JsonValue::Boolean(true)),
            ]))
        );
    }
}
