//! Concrete prospective rain receipt values and worker task over the selected archive compressor.

use weather_contracts::{JsonValue, RainCaptureReceiptMetadataValue};
use weather_database::{
    PreparedRainCaptureMetadataJson, RainCaptureClaimCalendar, RainCaptureReceiptCanonicalValues,
    SerializedJson, Sha256Digest,
};
use weather_edge::{ArchiveWirePrimitives, ForecastEvidenceMaterialCodec};
use weather_worker::{RainCaptureResult, TaskFailure};

use crate::{
    ApiMaintenanceReceiptCanonicalValues, NativeModelEvidenceValues, NativePostgresWorkerPool,
    NativeWirePrimitives, NativeWorkerRainCaptureTask, RetainedWorkerSidecarTask,
};

/// Share the exact approved predecessor compression owner with prospective rain receipt storage.
pub struct NativeRainReceiptValues<Codec> {
    model: NativeModelEvidenceValues,
    compression: Codec,
}

impl<Codec> NativeRainReceiptValues<Codec> {
    /// Bind the retained compression owner without invoking it or constructing another dependency.
    pub const fn new(compression: Codec) -> Self {
        Self {
            model: NativeModelEvidenceValues::new(),
            compression,
        }
    }
}

impl<Codec> RainCaptureClaimCalendar for NativeRainReceiptValues<Codec> {
    type Error = TaskFailure;

    /// Preserve the actual canonical UTC clock from a claim's retained database transaction.
    fn parse_utc(&mut self, value: &str) -> Result<i64, Self::Error> {
        self.model.parse_utc(value).map_err(|_| receipt_failure())
    }

    /// Format the admitted database epoch at the source millisecond precision.
    fn format_utc(&mut self, milliseconds: i64) -> Result<String, Self::Error> {
        self.model
            .format_utc(milliseconds)
            .map_err(|_| receipt_failure())
    }

    /// Reuse the actual immutable deployed tag grammar and calendar check.
    fn validate_release(&mut self, release: &str) -> Result<bool, Self::Error> {
        self.model
            .validate_release(release)
            .map_err(|_| receipt_failure())
    }
}

impl<Codec: ForecastEvidenceMaterialCodec> RainCaptureReceiptCanonicalValues
    for NativeRainReceiptValues<Codec>
{
    type Error = TaskFailure;

    /// Retain exact canonical UTC parsing for the original provider receipt clocks.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error> {
        self.parse_utc(value)
    }

    /// Compute the original SHA-256 over unmodified provider bytes.
    fn sha256(&mut self, bytes: &[u8]) -> Result<Sha256Digest, Self::Error> {
        ApiMaintenanceReceiptCanonicalValues::sha256(&self.model, bytes).ok_or_else(receipt_failure)
    }

    /// Use only the independently proved predecessor compression owner; never substitute another codec.
    fn gzip_level_9(&mut self, bytes: &[u8]) -> Result<Vec<u8>, Self::Error> {
        self.compression
            .gzip_level_nine_zero_mtime(bytes)
            .ok_or_else(receipt_failure)
    }

    /// Encode complete source metadata with exact ordered JSON and original JS code-unit length.
    fn metadata_json(
        &mut self,
        metadata: &[(Vec<u16>, RainCaptureReceiptMetadataValue)],
    ) -> Result<PreparedRainCaptureMetadataJson, Self::Error> {
        encode_metadata(metadata).ok_or_else(receipt_failure)
    }
}

/// Encode the original metadata object without an archive framing newline.
fn encode_metadata(
    metadata: &[(Vec<u16>, RainCaptureReceiptMetadataValue)],
) -> Option<PreparedRainCaptureMetadataJson> {
    let fields = metadata
        .iter()
        .map(|(key, value)| {
            let value = match value {
                RainCaptureReceiptMetadataValue::Null => JsonValue::Null,
                RainCaptureReceiptMetadataValue::Boolean(value) => JsonValue::Boolean(*value),
                RainCaptureReceiptMetadataValue::Number(value) if value.is_finite() => {
                    JsonValue::Number(*value)
                }
                RainCaptureReceiptMetadataValue::Number(_) => return None,
                RainCaptureReceiptMetadataValue::String(value) => JsonValue::String(value.clone()),
            };
            Some((key.clone(), value))
        })
        .collect::<Option<Vec<_>>>()?;
    let mut bytes = NativeWirePrimitives.encode_ordered_json(&JsonValue::Object(fields))?;
    // strip exactly the shared JSON encoder's defined framing byte
    if bytes.pop() != Some(b'\n') {
        return None;
    }
    let json = String::from_utf8(bytes).ok()?;
    let js_string_units = json.encode_utf16().count();
    Some(PreparedRainCaptureMetadataJson {
        json: SerializedJson::admitted(json),
        js_string_units,
    })
}

/// Bind the actual prospective collector to separately retained receipt effects and the real worker pool.
pub struct NativeWorkerRainCaptureTaskOwner<Codec> {
    task: NativeWorkerRainCaptureTask,
    values: NativeRainReceiptValues<Codec>,
}

impl<Codec> NativeWorkerRainCaptureTaskOwner<Codec> {
    /// Retain the actual collector and selected archive compressor.
    pub const fn new(task: NativeWorkerRainCaptureTask, compression: Codec) -> Self {
        Self {
            task,
            values: NativeRainReceiptValues::new(compression),
        }
    }
}

impl<Values, Codec: ForecastEvidenceMaterialCodec>
    RetainedWorkerSidecarTask<NativePostgresWorkerPool, Values, ()>
    for NativeWorkerRainCaptureTaskOwner<Codec>
{
    type Output = RainCaptureResult;

    /// Borrow the same retained worker pool and exact receipt effects for the original collector.
    fn run(
        &mut self,
        (): (),
        database: &mut NativePostgresWorkerPool,
        _values: &mut Values,
    ) -> Result<Self::Output, TaskFailure> {
        self.task.run((), database, &mut self.values)
    }
}

/// Preserve a closed receipt preparation category without exposing provider content.
fn receipt_failure() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "rain_capture_receipt_append_failed".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Match the original JSON.stringify text and UTF-16 length for real receipt metadata.
    #[test]
    fn native_rain_capture_metadata_keeps_js_order_and_length() {
        use RainCaptureReceiptMetadataValue::*;
        let fields = [
            (
                "source".encode_utf16().collect(),
                String("station".encode_utf16().collect()),
            ),
            ("count".encode_utf16().collect(), Number(12.0)),
            ("valid".encode_utf16().collect(), Boolean(true)),
            ("reason".encode_utf16().collect(), Null),
        ];
        let prepared = encode_metadata(&fields).unwrap();
        assert_eq!(
            prepared.json.as_str(),
            r#"{"source":"station","count":12,"valid":true,"reason":null}"#
        );
        assert_eq!(
            prepared.js_string_units,
            prepared.json.as_str().encode_utf16().count()
        );
    }
}
