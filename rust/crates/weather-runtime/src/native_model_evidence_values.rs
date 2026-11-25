//! Native canonical evidence values for retained forecast-model artifacts.

use std::sync::OnceLock;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use regex::Regex;
use weather_contracts::JsonValue;
use weather_database::{RainCaptureClaimCalendar, Sha256Digest};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    MaintenanceReceiptScalar, NativeShadowProducerPrimitives, NativeWirePrimitives,
    WorkerRainServingValues,
};

const FORBIDDEN_VALUE_PATTERNS: &[&str] = &[
    r"(?i)-----BEGIN [A-Z ]*PRIVATE KEY-----",
    r"(?:^|[^A-Za-z0-9_])(?:AKIA|ASIA)[A-Z0-9]{16}(?:$|[^A-Za-z0-9_])",
    r"(?:^|[^A-Za-z0-9_])(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|sk-[A-Za-z0-9_-]{16,})(?:$|[^A-Za-z0-9_])",
    r"(?i)(?:^|[^A-Za-z0-9_\x{017f}\x{212a}])Bearer[\x{0009}-\x{000d}\x{0020}\x{00a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}]+[A-Za-z0-9._~+/-]+=*(?:$|[^A-Za-z0-9_\x{017f}\x{212a}])",
    r"(?i)[a-z][a-z0-9+.-]*://[^/\x{0009}-\x{000d}\x{0020}\x{00a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}:@]+:[^/\x{0009}-\x{000d}\x{0020}\x{00a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}@]+@",
    r"(?i)postgres(?:ql)?://",
    r"(?i)(?:^|[/\\])\.weather-(?:data|models)(?:[/\\]|$)",
    r"(?i)(?:^|[/\\])\.weather[/\\]model-evidence(?:[/\\]|$)",
    r"(?i)(?:^|[/\\])(?:evidence|members?|raw(?:[-_ ]?(?:data|rows?))?|encrypted(?:[-_ ]?(?:data|members?))?)(?:[/\\]|$)",
    r"(?i)\.(?:age|enc|gpg|jsonl(?:\.gz)?|p12|pem|pfx)(?:$|[?#])",
    r"(?i)(?:^|[^a-z0-9])(?:api[-_ ]?key|access[-_ ]?key|auth[-_ ]?token|credential|password|passwd|private[-_ ]?key|pwd|secret)(?:[^a-z0-9]|$)",
    r"(?i)(?:^|[^a-z0-9])(?:device[-_ ]?(?:id|mac|serial)|lan[-_ ]?(?:address|host|ip)|mac[-_ ]?address|private[-_ ]?(?:address|host|ip))(?:[^a-z0-9]|$)",
    r"(?i)(?:^|[^a-f0-9])(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}(?:[^a-f0-9]|$)",
    r"(?:^|[^0-9])(?:10(?:\.[0-9]{1,3}){3}|127(?:\.[0-9]{1,3}){3}|169\.254(?:\.[0-9]{1,3}){2}|192\.168(?:\.[0-9]{1,3}){2}|172\.(?:1[6-9]|2[0-9]|3[01])(?:\.[0-9]{1,3}){2})(?:[^0-9]|$)",
    r"(?i)(?:^|[^a-z0-9])localhost(?:[^a-z0-9]|$)",
    r"(?i)(?:^|[^a-f0-9])(?:::1|f[cd][a-f0-9]{2}:|fe[89ab][a-f0-9]:)",
];

static COMPILED_FORBIDDEN_VALUE_PATTERNS: OnceLock<Option<Vec<Regex>>> = OnceLock::new();

/// Zero-state owner for canonical model evidence and its retained runtime filter.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeModelEvidenceValues {
    wire: NativeWirePrimitives,
}

impl NativeModelEvidenceValues {
    /// Construct the native evidence owner without allocating or probing process state.
    pub const fn new() -> Self {
        Self {
            wire: NativeWirePrimitives::new(),
        }
    }
}

impl ApiMaintenanceReceiptCanonicalValues for NativeModelEvidenceValues {
    /// Delegate exact canonical UTC admission to the approved native date owner.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        ApiMaintenanceReceiptCanonicalValues::canonical_utc_epoch_milliseconds(&self.wire, value)
    }

    /// Delegate raw-byte SHA-256 to the approved native digest owner.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        ApiMaintenanceReceiptCanonicalValues::sha256(&self.wire, bytes)
    }

    /// Delegate ordered receipt encoding and hashing to the existing concrete implementation.
    fn unsigned_receipt_sha256(
        &self,
        fields: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        self.wire.unsigned_receipt_sha256(fields)
    }
}

impl ApiMaintenanceShadowEvidenceValues for NativeModelEvidenceValues {
    /// Decode exact source JSON through the approved dependency-free parser.
    fn decode_shadow_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.wire.decode_json(bytes)
    }

    /// Encode ordered nested JSON without the archive codec's framing newline.
    fn encode_shadow_ordered_json(&self, document: &JsonValue) -> Option<Vec<u8>> {
        let mut encoded = self.wire.encode_ordered_json(document)?;
        // remove only the framing newline required by the shared archive primitive
        (encoded.pop() == Some(b'\n')).then_some(encoded)
    }

    /// Decode only canonical padded standard base64 from exact UTF-16 code units.
    fn decode_shadow_comparator_base64(&self, encoded: &[u16]) -> Option<Vec<u8>> {
        let encoded = String::from_utf16(encoded).ok()?;
        let decoded = self.wire.decode_standard_base64(&encoded)?;
        // require the same alphabet and padding to round-trip without normalization
        (self.wire.encode_standard_base64(&decoded)?.as_str() == encoded).then_some(decoded)
    }

    /// Encode recursively sorted canonical JSON with its required final newline.
    fn encode_shadow_sorted_json(&self, document: &JsonValue) -> Option<Vec<u8>> {
        self.wire.encode_sorted_json(document)
    }

    /// Apply the exact retained runtime string-value filter.
    fn check_legacy_wind_runtime_value(&self, code_units: &[u16]) -> Option<()> {
        check_runtime_value(code_units)
    }
}

impl RainCaptureClaimCalendar for NativeModelEvidenceValues {
    type Error = ();

    /// Preserve the exact canonical UTC millisecond spelling of capture slots.
    fn parse_utc(&mut self, value: &str) -> Result<i64, Self::Error> {
        self.wire.canonical_utc_milliseconds(value).ok_or(())
    }

    /// Format the actual database-clock sample at the source precision.
    fn format_utc(&mut self, milliseconds: i64) -> Result<String, Self::Error> {
        self.wire.format_utc_milliseconds(milliseconds).ok_or(())
    }

    /// Require the deployed immutable tag and its genuine Gregorian date.
    fn validate_release(&mut self, release: &str) -> Result<bool, Self::Error> {
        // the shared tag grammar guarantees all date positions are ASCII
        if !crate::api_process_configuration::migration_authorization_release_is_valid(release) {
            return Ok(false);
        }
        let date = format!(
            "{}-{}-{}T00:00:00.000Z",
            &release[..4],
            &release[5..7],
            &release[8..10]
        );
        Ok(self.wire.canonical_utc_milliseconds(&date).is_some())
    }
}

impl WorkerRainServingValues for NativeModelEvidenceValues {
    /// Encode the exact retained artifact bytes for the original comparator authority.
    fn encode_worker_rain_base64(&self, bytes: &[u8]) -> Option<String> {
        Some(STANDARD.encode(bytes))
    }
}

impl NativeShadowProducerPrimitives for NativeModelEvidenceValues {
    /// Apply the same retained runtime string-value filter on producer admission.
    fn check_legacy_wind_runtime_value(&self, code_units: &[u16]) -> Option<()> {
        check_runtime_value(code_units)
    }
}

impl RevisionArchiveCanonicalCodec for NativeModelEvidenceValues {
    /// Delegate canonical UTC parsing to the retained primitive owner.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        self.wire.canonical_utc_milliseconds(text)
    }

    /// Delegate exact-byte hashing to the retained primitive owner.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        RevisionArchiveCanonicalCodec::sha256(&self.wire, bytes)
    }
}

impl ArchiveWirePrimitives for NativeModelEvidenceValues {
    /// Delegate canonical UTC formatting to the retained primitive owner.
    fn format_utc_milliseconds(&self, milliseconds: i64) -> Option<String> {
        self.wire.format_utc_milliseconds(milliseconds)
    }

    /// Delegate explicit-offset instant parsing to the retained primitive owner.
    fn parse_explicit_utc_milliseconds(&self, text: &str) -> Option<i64> {
        self.wire.parse_explicit_utc_milliseconds(text)
    }

    /// Delegate exact JSON decoding to the retained primitive owner.
    fn decode_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.wire.decode_json(bytes)
    }

    /// Delegate recursively sorted canonical encoding to the retained primitive owner.
    fn encode_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.wire.encode_sorted_json(value)
    }

    /// Delegate source-ordered JSON encoding to the retained primitive owner.
    fn encode_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.wire.encode_ordered_json(value)
    }

    /// Delegate standard padded base64 decoding to the retained primitive owner.
    fn decode_standard_base64(&self, text: &str) -> Option<Vec<u8>> {
        self.wire.decode_standard_base64(text)
    }

    /// Delegate standard padded base64 encoding to the retained primitive owner.
    fn encode_standard_base64(&self, bytes: &[u8]) -> Option<String> {
        self.wire.encode_standard_base64(bytes)
    }
}

/// Compile the fixed predecessor regex grammar exactly once for the retained process.
fn compiled_patterns() -> Option<&'static [Regex]> {
    COMPILED_FORBIDDEN_VALUE_PATTERNS
        .get_or_init(|| {
            FORBIDDEN_VALUE_PATTERNS
                .iter()
                .map(|pattern| Regex::new(pattern))
                .collect::<Result<Vec<_>, _>>()
                .ok()
        })
        .as_deref()
}

/// Apply the original JavaScript value grammar without trimming or normalizing source text.
fn check_runtime_value(code_units: &[u16]) -> Option<()> {
    let value = String::from_utf16_lossy(code_units);
    let patterns = compiled_patterns()?;
    // reject the first forbidden value shape without changing the admitted text
    for pattern in patterns {
        if pattern.is_match(&value) {
            return None;
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Admit ordinary generated model strings through both evidence and producer boundaries.
    #[test]
    fn native_model_evidence_accepts_ordinary_artifact_values() {
        let values = NativeModelEvidenceValues::new();
        // cover representative runtime identities, contract labels, and public source text
        for value in [
            "forecast-adjustment-runtime-bundle/v2",
            "America/Los_Angeles",
            "open-meteo:forecast",
            "robust-hierarchical-median/v1",
            "2026-09-12T20:00:00.000Z",
        ] {
            let units = value.encode_utf16().collect::<Vec<_>>();
            assert_eq!(
                ApiMaintenanceShadowEvidenceValues::check_legacy_wind_runtime_value(
                    &values, &units
                ),
                Some(())
            );
            assert_eq!(
                NativeShadowProducerPrimitives::check_legacy_wind_runtime_value(&values, &units),
                Some(())
            );
        }
    }
}
