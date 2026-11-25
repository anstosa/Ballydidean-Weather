//! Original bounded fit candidate JSON admission preserving numerical-runtime bytes.

use crate::{NativeWirePrimitives, native_maintenance_retirement_files_v2::format_digest};
use regex::Regex;
use std::sync::OnceLock;
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

const MAXIMUM_CANDIDATE_BYTES: usize = 8 * 1024 * 1024;
static PRIVATE_FIELD: OnceLock<Option<Regex>> = OnceLock::new();
static PRIVATE_VALUE: OnceLock<Option<Regex>> = OnceLock::new();

/// Exact admitted candidate bytes and their immutable raw source identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFitCandidate {
    pub canonical_bytes: Vec<u8>,
    pub candidate_sha256: String,
}

/// Validate one original compact sorted candidate document without normalizing Python numbers.
pub fn admit_native_maintenance_fit_candidate(
    bytes: &[u8],
) -> Option<NativeMaintenanceFitCandidate> {
    if !(2..=MAXIMUM_CANDIDATE_BYTES).contains(&bytes.len()) || bytes.last() != Some(&b'\n') {
        return None;
    }
    std::str::from_utf8(bytes).ok()?;
    let body = &bytes[..bytes.len() - 1];
    let mut cursor = CandidateCursor { body, pos: 0 };
    // retain the original object-only, single LF framing and exact lexical candidate bytes
    if body.first() != Some(&b'{') || !cursor.value(0) || cursor.pos != body.len() {
        return None;
    }
    let value = NativeWirePrimitives.decode_json(bytes)?;
    if !matches!(value, JsonValue::Object(_)) || !public_candidate_value(&value) {
        return None;
    }
    Some(NativeMaintenanceFitCandidate {
        canonical_bytes: bytes.to_vec(),
        candidate_sha256: format_digest(NativeWirePrimitives.sha256(bytes)),
    })
}

struct CandidateCursor<'a> {
    body: &'a [u8],
    pos: usize,
}

impl CandidateCursor<'_> {
    /// Consume exactly one canonical value with bounded parser depth.
    fn value(&mut self, depth: usize) -> bool {
        if depth > 128 {
            return false;
        }
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().is_some(),
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => false,
        }
    }

    /// Admit keys in the original strictly increasing UTF-16 order.
    fn object(&mut self, depth: usize) -> bool {
        if !self.take(b'{') {
            return false;
        }
        if self.take(b'}') {
            return true;
        }
        let mut previous: Option<Vec<u16>> = None;
        loop {
            let Some(key) = self.string() else {
                return false;
            };
            if previous.as_ref().is_some_and(|prior| prior >= &key)
                || !self.take(b':')
                || !self.value(depth)
            {
                return false;
            }
            previous = Some(key);
            if self.take(b'}') {
                return true;
            }
            if !self.take(b',') {
                return false;
            }
        }
    }

    /// Preserve the original ordered array members and compact delimiters.
    fn array(&mut self, depth: usize) -> bool {
        if !self.take(b'[') {
            return false;
        }
        if self.take(b']') {
            return true;
        }
        loop {
            if !self.value(depth) {
                return false;
            }
            if self.take(b']') {
                return true;
            }
            if !self.take(b',') {
                return false;
            }
        }
    }

    /// Admit exactly the source JSON.stringify string token rather than alternate escapes.
    fn string(&mut self) -> Option<Vec<u16>> {
        let start = self.pos;
        if !self.take(b'"') {
            return None;
        }
        loop {
            let byte = self.peek()?;
            self.pos += 1;
            match byte {
                b'"' => {
                    let token = &self.body[start..self.pos];
                    let value = NativeWirePrimitives.decode_json(token)?;
                    let canonical = NativeWirePrimitives.encode_sorted_json(&value)?;
                    if canonical.strip_suffix(b"\n")? != token {
                        return None;
                    }
                    let JsonValue::String(units) = value else {
                        return None;
                    };
                    return Some(units);
                }
                b'\\' => {
                    let escape = self.peek()?;
                    self.pos += 1;
                    if escape == b'u' {
                        let digits = self.body.get(self.pos..self.pos.checked_add(4)?)?;
                        if !digits
                            .iter()
                            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
                        {
                            return None;
                        }
                        self.pos += 4;
                    } else if !matches!(escape, b'"' | b'\\' | b'b' | b'f' | b'n' | b'r' | b't') {
                        return None;
                    }
                }
                0..=31 => return None,
                _ => {}
            }
        }
    }

    /// Admit source shortest decimal tokens including retained Python integral fractions.
    fn number(&mut self) -> bool {
        let start = self.pos;
        let negative = self.take(b'-');
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.pos += 1;
                self.digits();
            }
            _ => return false,
        }
        let mut fraction_zero = false;
        // preserve the source's integral .0 but not other redundant fraction tails
        if self.take(b'.') {
            let fraction_start = self.pos;
            self.digits();
            if self.pos == fraction_start {
                return false;
            }
            let fraction = &self.body[fraction_start..self.pos];
            fraction_zero = fraction == b"0";
            if !fraction_zero && fraction.last() == Some(&b'0') {
                return false;
            }
        }
        // retain source Javascript exponents and Python signed two-digit exponents
        if self.take(b'e') {
            let signed = self.take(b'+') || self.take(b'-');
            let exponent_start = self.pos;
            self.digits();
            let exponent = &self.body[exponent_start..self.pos];
            if exponent.is_empty()
                || fraction_zero
                || (exponent.len() > 1 && exponent[0] == b'0' && (exponent.len() != 2 || !signed))
            {
                return false;
            }
        }
        let token = &self.body[start..self.pos];
        if negative && token == b"-0" {
            return false;
        }
        std::str::from_utf8(token)
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .is_some_and(f64::is_finite)
    }

    /// Advance through one exact decimal digit field.
    fn digits(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.pos += 1;
        }
    }

    /// Match one source literal without additional normalization.
    fn literal(&mut self, token: &[u8]) -> bool {
        if self
            .body
            .get(self.pos..)
            .is_some_and(|tail| tail.starts_with(token))
        {
            self.pos += token.len();
            true
        } else {
            false
        }
    }

    /// Read the next structural byte.
    fn peek(&self) -> Option<u8> {
        self.body.get(self.pos).copied()
    }

    /// Consume one expected byte without implicit whitespace skipping.
    fn take(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
}

// retain the original source candidate material filter independently of lexical decoding
fn public_candidate_value(value: &JsonValue) -> bool {
    let Some(fields)=PRIVATE_FIELD.get_or_init(||Regex::new(r"(?i)(password|secret|credential|private.?key|github.?token|archive.?root|key.?path|agent.?socket|dbus)").ok())else{return false;};
    let Some(values)=PRIVATE_VALUE.get_or_init(||Regex::new(r"(?i)(-----BEGIN [^-]*PRIVATE KEY-----|/home/ubuntu(?:/|$)|/mnt/|\.ssh(?:/|$)|SSH_AUTH_SOCK|GITHUB_TOKEN|GH_TOKEN|AWS_SECRET_ACCESS_KEY|DBUS_SESSION_BUS_ADDRESS)").ok())else{return false;};
    let mut pending = vec![value];
    // inspect every source leaf without emitting private candidate text
    while let Some(value) = pending.pop() {
        match value {
            JsonValue::Object(entries) => {
                for (key, item) in entries {
                    let Ok(key) = String::from_utf16(key) else {
                        return false;
                    };
                    if fields.is_match(&key) {
                        return false;
                    }
                    pending.push(item);
                }
            }
            JsonValue::Array(items) => pending.extend(items),
            JsonValue::String(units) => {
                let Ok(value) = String::from_utf16(units) else {
                    return false;
                };
                if values.is_match(&value) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retain the genuine numerical runtime's decimal and exponent spelling without reserialization.
    #[test]
    fn native_maintenance_fit_candidate_preserves_numeric_source_bytes() {
        let bytes = b"{\"a\":1.0,\"b\":1e-07,\"c\":1e+20,\"d\":[false,null,-0.0]}\n";
        let admitted = admit_native_maintenance_fit_candidate(bytes).unwrap();
        assert_eq!(admitted.canonical_bytes, bytes);
        assert_eq!(
            admitted.candidate_sha256,
            format_digest(NativeWirePrimitives.sha256(bytes))
        );
    }
}
