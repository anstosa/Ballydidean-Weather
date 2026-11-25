//! Actual native forecast evidence wire codecs sharing one independently proved compressor.

use sha2::{Digest, Sha256};
use weather_contracts::JsonValue;
use weather_edge::{
    ArchiveWirePrimitives, ForecastCycleHashPair, ForecastEvidenceMaterialCodec,
    ForecastEvidenceReceiptCodec, RevisionArchiveCanonicalCodec,
};

use crate::{NativeEvaluationStreamingCodec, NativeWirePrimitives};

/// The complete evidence codec around a caller-supplied exact predecessor gzip effect.
///
/// Compression remains an explicit dependency of this owner; an unproved implementation
/// cannot be selected through a default or an equivalent-decompression fallback.
#[derive(Clone)]
pub struct NativeForecastArchiveCodec<Compression> {
    compression: Compression,
}

impl<Compression> NativeForecastArchiveCodec<Compression> {
    /// Retain the independently admitted deterministic gzip level-nine implementation.
    pub const fn new(compression: Compression) -> Self {
        Self { compression }
    }
}

impl<Compression> RevisionArchiveCanonicalCodec for NativeForecastArchiveCodec<Compression> {
    /// Reuse the original canonical UTC millisecond owner.
    fn canonical_utc_milliseconds(&self, value: &str) -> Option<i64> {
        NativeWirePrimitives.canonical_utc_milliseconds(value)
    }

    /// Hash the exact supplied immutable material.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, bytes)
    }
}

impl<Compression> ForecastCycleHashPair for NativeForecastArchiveCodec<Compression> {
    /// Hash the header and payload as one stream without allocating their concatenation.
    fn sha256_pair(&self, header: &[u8], payload: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(header);
        hasher.update(payload);
        hasher.finalize().into()
    }
}

impl<Compression: Fn(&[u8]) -> Option<Vec<u8>>> ForecastEvidenceMaterialCodec
    for NativeForecastArchiveCodec<Compression>
{
    /// Decode the real projected public forecast with the shared native JSON primitive.
    fn decode_projected_forecast(&self, bytes: &[u8]) -> Option<JsonValue> {
        NativeWirePrimitives.decode_json(bytes)
    }

    /// Serialize recursively sorted finite JSON with the predecessor's exact no-LF format.
    fn canonical_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        canonical_forecast_json(value)
    }

    /// Call only the explicitly selected deterministic predecessor compression implementation.
    fn gzip_level_nine_zero_mtime(&self, bytes: &[u8]) -> Option<Vec<u8>> {
        (self.compression)(bytes)
    }

    /// Preserve standard padded base64 spelling for availability members.
    fn standard_base64(&self, bytes: &[u8]) -> Option<String> {
        NativeWirePrimitives.encode_standard_base64(bytes)
    }
}

impl<Compression: Fn(&[u8]) -> Option<Vec<u8>>> ForecastEvidenceReceiptCodec
    for NativeForecastArchiveCodec<Compression>
{
    /// Decode retained source JSON without changing original key or scalar representation.
    fn decode_retained_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        NativeWirePrimitives.decode_json(bytes)
    }

    /// Admit standard base64 through the approved native primitive.
    fn decode_standard_base64(&self, value: &str) -> Option<Vec<u8>> {
        NativeWirePrimitives.decode_standard_base64(value)
    }

    /// Inflate existing gzip members under the original output ceiling.
    fn gunzip_bounded(&self, bytes: &[u8], maximum_bytes: usize) -> Option<Vec<u8>> {
        NativeEvaluationStreamingCodec.gunzip_bounded(bytes, maximum_bytes)
    }
}

/// Reject nonfinite values as the original evidence canonicalizer does before serialization.
fn finite_json(value: &JsonValue) -> bool {
    match value {
        JsonValue::Number(number) => number.is_finite(),
        JsonValue::Array(items) => items.iter().all(finite_json),
        JsonValue::Object(fields) => fields.iter().all(|(_, value)| finite_json(value)),
        _ => true,
    }
}

/// Retain exact sorted canonical JSON without the generic archive encoder framing byte.
fn canonical_forecast_json(value: &JsonValue) -> Option<Vec<u8>> {
    // never normalize an unrepresentable number into a different canonical object
    if !finite_json(value) {
        return None;
    }
    let mut bytes = NativeWirePrimitives.encode_sorted_json(value)?;
    (bytes.pop()? == b'\n').then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Match the original recursively sorted forecast evidence JSON with no framing newline.
    #[test]
    fn native_forecast_archive_canonical_json_matches_predecessor() {
        let value = NativeWirePrimitives
            .decode_json(br#"{"z":1,"a":{"z":null,"a":[-0,1]}}"#)
            .unwrap();
        assert_eq!(
            canonical_forecast_json(&value).unwrap(),
            br#"{"a":{"a":[0,1],"z":null},"z":1}"#
        );
    }
}
