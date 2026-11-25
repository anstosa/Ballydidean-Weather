//! Approved native implementations of the predecessor's low-level wire primitives.

use std::sync::OnceLock;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use jiff::Timestamp;
use sha2::{Digest, Sha256};
use url::Url;
use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    DecodedNativeApiTarget, DecodedNativeEdgeTarget, MaintenanceCanonicalJson,
    NativeApiRequestTarget, NativeApiRequestTargetOwner, NativeEdgeRequestTargetOwner,
    native_json::{decode_json, encode_ordered_json, encode_sorted_json},
};

const NODE_MINIMUM_MILLISECONDS: i64 = -8_640_000_000_000_000;
const NODE_MAXIMUM_MILLISECONDS: i64 = 8_640_000_000_000_000;

/// Zero-state owner for the approved native JSON, date, digest, base64, and URL primitives.
///
/// Date support is deliberately limited to Jiff's range and ISO timestamps with an explicit UTC
/// offset. Inputs accepted by Node's broader legacy date parser but outside that boundary return
/// the owning trait's failure instead of being guessed, saturated, or silently normalized.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWirePrimitives;

impl NativeWirePrimitives {
    /// Construct the zero-state primitive owner.
    pub const fn new() -> Self {
        Self
    }
}

impl RevisionArchiveCanonicalCodec for NativeWirePrimitives {
    /// Admit only the exact millisecond UTC spelling that formats back identically.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        let timestamp = parse_millisecond_timestamp(text)?;
        let milliseconds = admit_node_milliseconds(timestamp.as_millisecond())?;
        (format_node_utc(timestamp) == text).then_some(milliseconds)
    }

    /// Hash the exact supplied bytes with the approved SHA-256 implementation.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        Sha256::digest(bytes).into()
    }
}

impl ArchiveWirePrimitives for NativeWirePrimitives {
    /// Render one supported millisecond instant with Node's fixed three-digit UTC precision.
    fn format_utc_milliseconds(&self, milliseconds: i64) -> Option<String> {
        Timestamp::from_millisecond(admit_node_milliseconds(milliseconds)?)
            .ok()
            .map(format_node_utc)
    }

    /// Parse one supported ISO instant only when it contains an explicit UTC offset.
    fn parse_explicit_utc_milliseconds(&self, text: &str) -> Option<i64> {
        // reject timezone-free inputs whose Node interpretation can depend on legacy grammar
        if !has_explicit_offset(text) {
            return None;
        }
        let timestamp: Timestamp = text.parse().ok()?;
        let milliseconds = timestamp.as_nanosecond().div_euclid(1_000_000);
        admit_node_milliseconds(i64::try_from(milliseconds).ok()?)
    }

    /// Decode one complete generated JSON value through the approved sibling primitive.
    fn decode_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        decode_json(bytes)
    }

    /// Encode recursively sorted JSON with one terminating LF.
    fn encode_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        encode_sorted_json(value)
    }

    /// Encode producer-ordered JSON with one terminating LF.
    fn encode_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        encode_ordered_json(value)
    }

    /// Decode one standard base64 value without substituting another alphabet.
    fn decode_standard_base64(&self, text: &str) -> Option<Vec<u8>> {
        STANDARD.decode(text).ok()
    }

    /// Encode one standard padded base64 value.
    fn encode_standard_base64(&self, bytes: &[u8]) -> Option<String> {
        Some(STANDARD.encode(bytes))
    }
}

impl NativeApiRequestTargetOwner for NativeWirePrimitives {
    /// Reproduce the Node HTTP-to-Fetch target construction and ordered query decoding.
    fn decode_target(&self, target: NativeApiRequestTarget<'_>) -> Option<DecodedNativeApiTarget> {
        let method = normalize_fetch_method(target.method)?;
        let url = Url::parse(&format!("http://{}{}", target.host, target.raw_target)).ok()?;

        // Fetch refuses request URLs containing embedded credentials
        if !url.username().is_empty() || url.password().is_some() {
            return None;
        }
        Some(DecodedNativeApiTarget {
            method,
            pathname: url.path().to_owned(),
            query: decoded_query(&url),
        })
    }
}

impl NativeEdgeRequestTargetOwner for NativeWirePrimitives {
    /// Decode against the original fixed base while preserving raw search and query order.
    fn decode_edge_target(&self, raw_target: &str) -> Option<DecodedNativeEdgeTarget> {
        let url = Url::options()
            .base_url(Some(edge_base()))
            .parse(raw_target)
            .ok()?;
        Some(DecodedNativeEdgeTarget {
            pathname: url.path().to_owned(),
            search: node_search(&url),
            query: decoded_query(&url),
        })
    }
}

impl CurrentTimeResolver for NativeWirePrimitives {
    /// Normalize one supported database date or string without accepting invalid fallbacks.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        // preserve the generated date, string, and invalid-date variants independently
        let epoch_milliseconds = match value {
            JsDateOrString::DateEpochMilliseconds(milliseconds) => {
                Timestamp::from_millisecond(*milliseconds)
                    .map(|_| *milliseconds)
                    .map_err(|_| {
                        unsupported_time("database date is outside the native date range")
                    })?
            }
            JsDateOrString::String(units) => {
                let text = String::from_utf16(units)
                    .map_err(|_| unsupported_time("database timestamp contains invalid UTF-16"))?;
                ArchiveWirePrimitives::parse_explicit_utc_milliseconds(self, &text).ok_or_else(
                    || unsupported_time("database timestamp is not a supported explicit instant"),
                )?
            }
            JsDateOrString::InvalidDate => {
                return Err(unsupported_time("database returned an invalid timestamp"));
            }
        };
        let utc = ArchiveWirePrimitives::format_utc_milliseconds(self, epoch_milliseconds)
            .ok_or_else(|| unsupported_time("database date is outside the native date range"))?;
        Ok(CanonicalInstant {
            epoch_milliseconds,
            utc,
        })
    }
}

impl MaintenanceCanonicalJson for NativeWirePrimitives {
    /// Admit only values whose recursively sorted bytes already match the source document.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        let value = decode_json(bytes)?;
        (encode_sorted_json(&value)?.as_slice() == bytes).then_some(value)
    }

    /// Encode the original recursively sorted canonical JSON with one trailing LF.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        encode_sorted_json(value)
    }

    /// Hash exact canonical bytes into the shared lowercase digest wrapper.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        let digest = RevisionArchiveCanonicalCodec::sha256(self, bytes);
        Some(Sha256Digest::admitted(lower_hex(digest)))
    }
}

/// Parse only timestamps represented exactly at millisecond precision.
fn parse_millisecond_timestamp(text: &str) -> Option<Timestamp> {
    let timestamp: Timestamp = text.parse().ok()?;

    // reject sub-millisecond input instead of silently truncating an instant
    if timestamp.as_nanosecond() % 1_000_000 != 0 {
        return None;
    }
    Some(timestamp)
}

/// Admit only the finite integer range supported by the source Date primitive.
fn admit_node_milliseconds(milliseconds: i64) -> Option<i64> {
    (NODE_MINIMUM_MILLISECONDS..=NODE_MAXIMUM_MILLISECONDS)
        .contains(&milliseconds)
        .then_some(milliseconds)
}

/// Render the source Date wire shape with exactly three fractional digits.
fn format_node_utc(timestamp: Timestamp) -> String {
    format!("{timestamp:.3}")
}

/// Recognize the explicit zone suffixes admitted by the archive boundary.
fn has_explicit_offset(text: &str) -> bool {
    let bytes = text.as_bytes();

    // preserve uppercase Z and signed hours with minute precision
    if bytes.last() == Some(&b'Z') {
        return true;
    }
    matches!(
        bytes.get(bytes.len().saturating_sub(6)..),
        Some([b'+' | b'-', hour_tens, hour_ones, b':', minute_tens, minute_ones])
            if [hour_tens, hour_ones, minute_tens, minute_ones]
                .iter()
                .all(|digit| digit.is_ascii_digit())
    )
}

/// Normalize the Fetch method set while rejecting its forbidden methods.
fn normalize_fetch_method(method: &str) -> Option<String> {
    // reject empty or non-token methods before applying Fetch normalization
    if method.is_empty() || !method.bytes().all(is_http_token_byte) {
        return None;
    }
    let upper = method.to_ascii_uppercase();

    // reject the three methods forbidden by the Fetch Request constructor
    if matches!(upper.as_str(), "CONNECT" | "TRACE" | "TRACK") {
        return None;
    }
    // uppercase only the standard methods normalized by Fetch
    if matches!(
        upper.as_str(),
        "DELETE" | "GET" | "HEAD" | "OPTIONS" | "POST" | "PUT"
    ) {
        Some(upper)
    } else {
        Some(method.to_owned())
    }
}

/// Recognize one HTTP method token byte without accepting non-ASCII text.
fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Decode URLSearchParams pairs in submitted order.
fn decoded_query(url: &Url) -> Vec<(String, String)> {
    url.query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// Preserve Node's empty search spelling for a bare question mark.
fn node_search(url: &Url) -> String {
    // Node hides a present but empty query from the public search field
    match url.query() {
        Some(query) if !query.is_empty() => format!("?{query}"),
        Some(_) | None => String::new(),
    }
}

/// Retain one parsed immutable base for every edge target owner.
fn edge_base() -> &'static Url {
    static BASE: OnceLock<Url> = OnceLock::new();
    BASE.get_or_init(|| {
        Url::parse("http://weather.invalid")
            .expect("the fixed native edge URL base must remain valid")
    })
}

/// Render one digest in the shared lowercase hexadecimal alphabet.
fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);

    // emit exactly two lowercase nibbles for every digest byte
    for byte in digest {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

/// Construct the owning current-projection boundary error.
fn unsupported_time(detail: &str) -> CurrentProjectionError {
    CurrentProjectionError {
        code: "current_projection.time",
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Match ordinary Node date, SHA-256, and standard padded base64 results.
    #[test]
    fn date_digest_and_base64_match_node_primitives() {
        let primitives = NativeWirePrimitives::new();
        assert_eq!(
            primitives.canonical_utc_milliseconds("2026-10-08T01:02:03.004Z"),
            Some(1_791_421_323_004)
        );
        assert_eq!(
            primitives.parse_explicit_utc_milliseconds("2026-10-08T02:02:03.004+01:00"),
            Some(1_791_421_323_004)
        );
        assert_eq!(
            primitives.parse_explicit_utc_milliseconds("2026-10-08T02:02:03.004999+01:00"),
            Some(1_791_421_323_004)
        );
        assert_eq!(
            primitives.parse_explicit_utc_milliseconds("1969-12-31T23:59:59.999500Z"),
            Some(-1)
        );
        assert_eq!(
            primitives.format_utc_milliseconds(1_791_421_323_004),
            Some("2026-10-08T01:02:03.004Z".to_owned())
        );
        assert_eq!(
            lower_hex(RevisionArchiveCanonicalCodec::sha256(&primitives, b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            primitives.encode_standard_base64(b"weather"),
            Some("d2VhdGhlcg==".to_owned())
        );
        assert_eq!(
            primitives.decode_standard_base64("d2VhdGhlcg=="),
            Some(b"weather".to_vec())
        );
    }

    /// Preserve Fetch normalization and ordered URLSearchParams decoding.
    #[test]
    fn api_target_matches_node_request_boundary() {
        let decoded = NativeWirePrimitives::new()
            .decode_target(NativeApiRequestTarget {
                method: "get",
                host: "weather.invalid",
                raw_target: "/api/%73ites?first+name=Ansel&first%20name=Cora&empty=",
            })
            .expect("ordinary Node request target");
        assert_eq!(decoded.method, "GET");
        assert_eq!(decoded.pathname, "/api/%73ites");
        assert_eq!(
            decoded.query,
            [
                ("first name".to_owned(), "Ansel".to_owned()),
                ("first name".to_owned(), "Cora".to_owned()),
                ("empty".to_owned(), String::new()),
            ]
        );
        assert!(
            NativeWirePrimitives::new()
                .decode_target(NativeApiRequestTarget {
                    method: "GET",
                    host: "user:password@weather.invalid",
                    raw_target: "/api/v1/sites",
                })
                .is_none()
        );
    }

    /// Preserve fixed-base path normalization, raw search, and decoded query values.
    #[test]
    fn edge_target_matches_node_url_boundary() {
        let decoded = NativeWirePrimitives::new()
            .decode_edge_target("/a/../b?x=%2F&x=+")
            .expect("ordinary Node edge target");
        assert_eq!(decoded.pathname, "/b");
        assert_eq!(decoded.search, "?x=%2F&x=+");
        assert_eq!(
            decoded.query,
            [
                ("x".to_owned(), "/".to_owned()),
                ("x".to_owned(), " ".to_owned()),
            ]
        );
    }

    /// Refuse unsupported legacy dates rather than substituting a different instant.
    #[test]
    fn date_limitations_are_explicit_failures() {
        let primitives = NativeWirePrimitives::new();
        assert_eq!(
            primitives.parse_explicit_utc_milliseconds("2026-10-08"),
            None
        );
        assert_eq!(
            primitives.format_utc_milliseconds(8_640_000_000_000_001),
            None
        );
    }
}
