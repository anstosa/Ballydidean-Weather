//! Shared archive wire primitives and concrete typed-contract ownership.

use weather_contracts::JsonValue;

use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, is_sha256};

/// The selected runtime's approved JSON, UTC, SHA-256, and base64 primitives.
///
/// Archive contract validation and mapping live in Rust, not behind these effects.
pub trait ArchiveWirePrimitives: RevisionArchiveCanonicalCodec {
    /// Render the selected UTC millisecond instant with the original wire format.
    fn format_utc_milliseconds(&self, milliseconds: i64) -> Option<String>;

    /// Resolve one original explicitly zoned domain instant through the selected date parser.
    fn parse_explicit_utc_milliseconds(&self, text: &str) -> Option<i64>;

    /// Decode one JSON value through the selected primitive.
    fn decode_json(&self, bytes: &[u8]) -> Option<JsonValue>;

    /// Encode recursively sorted JSON with the original terminating LF.
    fn encode_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>>;

    /// Encode producer-ordered JSON with the original terminating LF.
    fn encode_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>>;

    /// Decode one standard base64 value through the selected primitive.
    fn decode_standard_base64(&self, text: &str) -> Option<Vec<u8>>;

    /// Encode one standard padded base64 value through the selected primitive.
    fn encode_standard_base64(&self, bytes: &[u8]) -> Option<String>;
}

/// Bind one admitted primitive implementation to all concrete archive contracts.
pub struct ArchiveWireCodec<P> {
    primitives: P,
}

impl<P: ArchiveWirePrimitives> ArchiveWireCodec<P> {
    /// Bind the approved primitive owner without performing I/O.
    pub const fn new(primitives: P) -> Self {
        Self { primitives }
    }

    /// Borrow the single admitted primitive owner for an adjacent producer validator.
    pub const fn primitives(&self) -> &P {
        &self.primitives
    }

    /// Decode one original JSON value without imposing sorted serialization.
    pub(crate) fn decode(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.primitives.decode_json(bytes)
    }

    /// Match the exact canonical sorted newline-framed file.
    pub(crate) fn decode_canonical(&self, bytes: &[u8]) -> Option<JsonValue> {
        let value = self.decode(bytes)?;
        (self.encode(&value)?.as_slice() == bytes).then_some(value)
    }

    /// Delegate only sorted JSON serialization to the admitted primitive.
    pub(crate) fn encode(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.primitives.encode_sorted_json(value)
    }

    /// Preserve the producer's ordered wire bytes.
    pub(crate) fn encode_ordered(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.primitives.encode_ordered_json(value)
    }

    /// Normalize the domain record's original explicitly zoned timestamp.
    pub(crate) fn normalize_explicit_utc_instant(&self, text: &str) -> Option<String> {
        let bytes = text.as_bytes();
        // preserve the source's explicit Z or signed HH:MM timezone suffix
        let has_explicit_zone = bytes.last()==Some(&b'Z') ||
            bytes.get(bytes.len().saturating_sub(6)..).is_some_and(
                |suffix| matches!(suffix,[b'+'|b'-',a,b,b':',c,d] if [a,b,c,d].iter().all(|digit| digit.is_ascii_digit()))
            );
        if !has_explicit_zone {
            return None;
        }
        // reuse already canonical UTC without formatting or reparsing the same instant
        if self.canonical_utc_milliseconds(text).is_some() {
            return Some(text.to_owned());
        }
        let milliseconds = self.primitives.parse_explicit_utc_milliseconds(text)?;
        let canonical = self.primitives.format_utc_milliseconds(milliseconds)?;
        (self.canonical_utc_milliseconds(&canonical) == Some(milliseconds)).then_some(canonical)
    }

    /// Admit only the canonical standard padded base64 spelling.
    pub(crate) fn base64_decode(&self, text: &str) -> Option<Vec<u8>> {
        let decoded = self.primitives.decode_standard_base64(text)?;
        (self.primitives.encode_standard_base64(&decoded)?.as_str() == text).then_some(decoded)
    }

    /// Encode one exact payload without implementing a second base64 codec.
    pub(crate) fn base64_encode(&self, bytes: &[u8]) -> Option<String> {
        self.primitives.encode_standard_base64(bytes)
    }
}

impl<P: ArchiveWirePrimitives> RevisionArchiveCanonicalCodec for ArchiveWireCodec<P> {
    /// Forward calendar admission to the selected runtime primitive.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        self.primitives.canonical_utc_milliseconds(text)
    }

    /// Hash exact bytes through the selected runtime primitive.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        self.primitives.sha256(bytes)
    }
}

/// Borrow one exact source field without normalizing UTF-16 keys.
pub(crate) fn field<'a>(value: &'a JsonValue, name: &str) -> Option<&'a JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
}

/// Admit the exact original object member set.
pub(crate) fn exact_keys(value: &JsonValue, keys: &[&str]) -> bool {
    matches!(value,JsonValue::Object(fields) if fields.len()==keys.len())
        && keys.iter().all(|name| field(value, name).is_some())
}

/// Read one unmodified canonical UTF-16 string field.
pub(crate) fn text(value: &JsonValue, name: &str) -> Option<String> {
    let JsonValue::String(text) = field(value, name)? else {
        return None;
    };
    String::from_utf16(text).ok()
}

/// Represent one exact string field through the generated JSON value contract.
pub(crate) fn string(text: &str) -> JsonValue {
    JsonValue::String(text.encode_utf16().collect())
}

/// Represent one known object member.
pub(crate) fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Borrow the retained array without cloning payload bytes.
pub(crate) fn array<'a>(value: &'a JsonValue, name: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(entries) = field(value, name)? else {
        return None;
    };
    Some(entries)
}

/// Admit one canonical uint64 decimal string.
pub(crate) fn uint64(value: &JsonValue, name: &str) -> Option<u64> {
    let text = text(value, name)?;
    let number: u64 = text.parse().ok()?;
    (number.to_string() == text).then_some(number)
}

/// Admit one exact lowercase SHA-256 string.
pub(crate) fn hash(value: &JsonValue, name: &str) -> Option<String> {
    let text = text(value, name)?;
    is_sha256(&text).then_some(text)
}

/// Read one original boolean field without coercion.
pub(crate) fn bool_value(value: &JsonValue, name: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(value, name)? else {
        return None;
    };
    Some(*value)
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl<P: ArchiveWirePrimitives> crate::native_revision_spool::RevisionSpoolLeaseCodec
    for ArchiveWireCodec<P>
{
    /// Encode the original shared native spool lease without a second codec.
    fn encode_canonical_lease(&self, expires_at_milliseconds: i64, token: &str) -> Option<Vec<u8>> {
        let expires_at = self
            .primitives
            .format_utc_milliseconds(expires_at_milliseconds)?;
        // preserve the exact expiry and generated token envelope
        if self.canonical_utc_milliseconds(&expires_at) != Some(expires_at_milliseconds)
            || !lease_token(token)
        {
            return None;
        }
        self.encode(&JsonValue::Object(vec![
            member(
                "contractVersion",
                string("adjustment-revision-spool-lock/v1"),
            ),
            member("expiresAt", string(&expires_at)),
            member("token", string(token)),
        ]))
    }

    /// Read the original complete canonical lease before trusting its expiry.
    fn validated_lease_expiry(&self, bytes: &[u8]) -> Option<i64> {
        let value = self.decode_canonical(bytes)?;
        // preserve the source's fixed version and token predicate
        if text(&value, "contractVersion")? != "adjustment-revision-spool-lock/v1"
            || !lease_token(&text(&value, "token")?)
        {
            return None;
        }
        self.canonical_utc_milliseconds(&text(&value, "expiresAt")?)
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl<P: ArchiveWirePrimitives> crate::native_revision_spool::RevisionSpoolLeaseCodec
    for &ArchiveWireCodec<P>
{
    /// Let the native lease and contract reader share one admitted primitive owner.
    fn encode_canonical_lease(&self, expires_at_milliseconds: i64, token: &str) -> Option<Vec<u8>> {
        <ArchiveWireCodec<P> as crate::native_revision_spool::RevisionSpoolLeaseCodec>::encode_canonical_lease(*self, expires_at_milliseconds, token)
    }

    /// Forward the exact shared lease admission.
    fn validated_lease_expiry(&self, bytes: &[u8]) -> Option<i64> {
        <ArchiveWireCodec<P> as crate::native_revision_spool::RevisionSpoolLeaseCodec>::validated_lease_expiry(*self, bytes)
    }
}

/// Admit the source's fixed token alphabet without introducing a UUID parser.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn lease_token(token: &str) -> bool {
    token.len() == 36
        && token.bytes().all(
            // match the original lowercase hex and dash predicate
            |byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) || byte == b'-',
        )
}
