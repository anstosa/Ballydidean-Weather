//! Canonical immutable forecast material and first-observation receipt construction.

use std::collections::VecDeque;
use std::sync::Arc;

use weather_contracts::JsonValue;

use crate::forecast_evidence_rows::project_forecast_evidence_rows;
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant, lower_hex};
use crate::{
    ForecastCaptureChannel, ForecastCaptureSource, ForecastCaptureVersion, ForecastEvidenceWindow,
};

const OBJECT_MAXIMUM_BYTES: usize = 512 * 1024;
const COMPRESSED_OBJECT_MAXIMUM_BYTES: usize = 16 * 1024;
const LEGACY_RECEIPT_MAXIMUM_BYTES: usize = 16 * 1024;
const EXTENDED_RECEIPT_MAXIMUM_BYTES: usize = 2_048;
const MAXIMUM_COMPRESSION_RATIO: usize = 64;
const MAXIMUM_LEGACY_ROWS: usize = 240;
const MAXIMUM_EXTENDED_ROWS: usize = 241;
const COMPRESSION_CACHE_MAXIMUM_ENTRIES: usize = 8;
const EXTENDED_AVAILABILITY_MAXIMUM_BYTES: usize = 32 * 1024;

/// existing canonical wire and compression operations owned by the approved runtime codecs.
pub trait ForecastEvidenceMaterialCodec: RevisionArchiveCanonicalCodec {
    /// decode one bounded forecast response after public adjustment projection.
    fn decode_projected_forecast(&self, bytes: &[u8]) -> Option<JsonValue>;

    /// encode the predecessor's canonical JSON bytes without a trailing newline.
    fn canonical_json(&self, value: &JsonValue) -> Option<Vec<u8>>;

    /// use the predecessor's deterministic gzip level nine and zero mtime.
    fn gzip_level_nine_zero_mtime(&self, bytes: &[u8]) -> Option<Vec<u8>>;

    /// format the original standard base64 availability member.
    fn standard_base64(&self, bytes: &[u8]) -> Option<String>;
}

/// existing JSON, base64, and bounded gzip readers for retained predecessor receipts.
pub trait ForecastEvidenceReceiptCodec: ForecastEvidenceMaterialCodec {
    /// decode a complete historical JSON document without normalizing missing fields.
    fn decode_retained_json(&self, bytes: &[u8]) -> Option<JsonValue>;

    /// decode one bounded standard base64 member.
    fn decode_standard_base64(&self, value: &str) -> Option<Vec<u8>>;

    /// read one gzip stream without allocating beyond the provided output ceiling.
    fn gunzip_bounded(&self, bytes: &[u8], maximum_bytes: usize) -> Option<Vec<u8>>;
}

/// first receipt fields consumed by the immutable storage and later channel binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedForecastEvidenceReceipt {
    pub availability_rows: usize,
    pub first_edge_committed_at: String,
    pub object_sha256: [u8; 32],
    pub version: ForecastEvidenceObjectVersion,
    pub window: ForecastEvidenceWindow,
}

/// canonical content validated from one retained object, independently of receipt presence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedForecastEvidenceObject {
    pub row_count: usize,
    pub stable_receipt_identity_sha256: Option<[u8; 32]>,
    pub version: ForecastEvidenceObjectVersion,
    pub window: ForecastEvidenceWindow,
}

/// original 240-row evidence format or the additive fall-back-hour format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastEvidenceObjectVersion {
    Legacy,
    Extended,
}

impl ForecastEvidenceObjectVersion {
    /// select the exact object contract used by the immutable ledger.
    pub const fn object_contract(self) -> &'static str {
        match self {
            Self::Legacy => "forecast-adjustment-evidence-object/v1",
            Self::Extended => "forecast-adjustment-evidence-object/v2",
        }
    }

    /// select the exact matching availability receipt contract.
    pub const fn receipt_contract(self) -> &'static str {
        match self {
            Self::Legacy => "forecast-adjustment-edge-receipt/v1",
            Self::Extended => "forecast-adjustment-edge-receipt/v2",
        }
    }

    /// return the independently bounded receipt size for this object format.
    pub const fn maximum_receipt_bytes(self) -> usize {
        match self {
            Self::Legacy => LEGACY_RECEIPT_MAXIMUM_BYTES,
            Self::Extended => EXTENDED_RECEIPT_MAXIMUM_BYTES,
        }
    }
}

/// ordinary errors from producing a bounded immutable member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastEvidenceMaterialError {
    InvalidWindow,
    InvalidForecast,
    InvalidCommitClock,
    CanonicalEncoding,
    Compression,
    ObjectTooLarge,
    IdentityCollision,
}

/// complete immutable content independent from its future response finish time.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedForecastEvidence {
    availability: JsonValue,
    compressed_object: Arc<[u8]>,
    edge_receipt_identity_sha256: [u8; 32],
    issuance_channel: Option<ForecastCaptureChannel>,
    object: JsonValue,
    object_sha256: [u8; 32],
    row_count: usize,
    version: ForecastEvidenceObjectVersion,
    window: ForecastEvidenceWindow,
}

impl PreparedForecastEvidence {
    /// borrow exact deterministic gzip bytes for immutable storage.
    pub fn compressed_object(&self) -> &[u8] {
        &self.compressed_object
    }

    /// borrow the complete source projected object for the retained archive.
    pub const fn object(&self) -> &JsonValue {
        &self.object
    }

    /// select the content hash of the uncompressed canonical bytes.
    pub const fn object_sha256(&self) -> [u8; 32] {
        self.object_sha256
    }

    /// select the stable receipt identity which excludes quantities and mutable clocks.
    pub const fn edge_receipt_identity_sha256(&self) -> [u8; 32] {
        self.edge_receipt_identity_sha256
    }

    /// retain the reviewed channel only when the channel-bound runtime was active.
    pub const fn issuance_channel(&self) -> Option<ForecastCaptureChannel> {
        self.issuance_channel
    }

    /// count source rows without rereading immutable object content.
    pub const fn row_count(&self) -> usize {
        self.row_count
    }

    /// return the fixed matching object version.
    pub const fn version(&self) -> ForecastEvidenceObjectVersion {
        self.version
    }

    /// return the exact normalized request window.
    pub const fn window(&self) -> ForecastEvidenceWindow {
        self.window
    }
}

struct CompressionCacheEntry {
    object_sha256: [u8; 32],
    canonical: Vec<u8>,
    gzip: Arc<[u8]>,
}

/// mutable capture preparation with an eight-entry deterministic compression cache.
pub struct ForecastEvidenceMaterial<Codec> {
    codec: Codec,
    cache: VecDeque<CompressionCacheEntry>,
}

impl<Codec: ForecastEvidenceMaterialCodec> RevisionArchiveCanonicalCodec
    for ForecastEvidenceMaterial<Codec>
{
    /// delegate UTC interpretation without a second scheduler-specific codec.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        self.codec.canonical_utc_milliseconds(text)
    }

    /// delegate the scheduler's due-key hash to the same existing owner.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        self.codec.sha256(bytes)
    }
}

impl<Codec: ForecastEvidenceMaterialCodec> ForecastEvidenceMaterial<Codec> {
    /// share one bounded compression cache across successful responses.
    pub fn new(codec: Codec) -> Self {
        Self {
            codec,
            cache: VecDeque::with_capacity(COMPRESSION_CACHE_MAXIMUM_ENTRIES),
        }
    }

    /// borrow the same approved codec for the cold and channel owners.
    pub(crate) const fn codec(&self) -> &Codec {
        &self.codec
    }

    /// construct the admitted object and stable receipt address without touching storage.
    pub fn prepare(
        &mut self,
        source: ForecastCaptureSource<'_>,
        window: Option<ForecastEvidenceWindow>,
        capture: ForecastCaptureVersion,
    ) -> Result<PreparedForecastEvidence, ForecastEvidenceMaterialError> {
        use ForecastEvidenceMaterialError as Error;
        let window = window.ok_or(Error::InvalidWindow)?;
        let decoded;
        let forecast = match source {
            ForecastCaptureSource::Projected(value) => value,
            ForecastCaptureSource::Encoded(bytes) => {
                decoded = self
                    .codec
                    .decode_projected_forecast(bytes)
                    .ok_or(Error::InvalidForecast)?;
                &decoded
            }
        };
        let issuance_channel = match capture {
            ForecastCaptureVersion::Legacy => None,
            ForecastCaptureVersion::ChannelBound(channel) => Some(channel),
        };
        // the legacy runtime rejects the additive fall-back-hour object
        let maximum = if issuance_channel.is_some() {
            MAXIMUM_EXTENDED_ROWS
        } else {
            MAXIMUM_LEGACY_ROWS
        };
        let values = project_forecast_evidence_rows(forecast, maximum, &self.codec)
            .map_err(|_| Error::InvalidForecast)?;
        // channel-bound ordinary windows retain the exact legacy object version
        let row_count = values.rows.len();
        let version = if row_count == MAXIMUM_EXTENDED_ROWS {
            ForecastEvidenceObjectVersion::Extended
        } else {
            ForecastEvidenceObjectVersion::Legacy
        };
        let settings_sha256 = digest_string(
            self.codec
                .sha256(&canonical(&self.codec, &values.settings)?),
        );
        let object = object([
            ("bundleIdentities", values.bundle_identities.clone()),
            ("contractVersion", string(version.object_contract())),
            ("rows", JsonValue::Array(values.rows)),
            ("settingsSha256", settings_sha256.clone()),
            ("siteKey", string("ballydidean")),
            ("window", string(window.wire_value())),
        ]);
        let object_bytes = canonical(&self.codec, &object)?;
        // refuse oversized canonical values before invoking compression
        if object_bytes.len() > OBJECT_MAXIMUM_BYTES {
            return Err(Error::ObjectTooLarge);
        }
        let object_sha256 = self.codec.sha256(&object_bytes);
        let compressed_object = self.compressed(object_sha256, object_bytes)?;
        let stable = object_without_scoring(
            values.bundle_identities,
            values.stable_rows,
            settings_sha256,
            window,
        );
        let edge_receipt_identity_sha256 = self.codec.sha256(&canonical(&self.codec, &stable)?);
        Ok(PreparedForecastEvidence {
            availability: values.availability,
            compressed_object,
            edge_receipt_identity_sha256,
            issuance_channel,
            object,
            object_sha256,
            row_count,
            version,
            window,
        })
    }

    /// materialize a fresh bounded receipt only after the first successful response finish.
    pub fn create_receipt(
        &self,
        prepared: &PreparedForecastEvidence,
        first_edge_committed_at: &str,
    ) -> Result<Vec<u8>, ForecastEvidenceMaterialError> {
        let bytes = self.encode_receipt(prepared, first_edge_committed_at)?;
        // bound the complete framed receipt before any native storage operation
        if bytes.len() > prepared.version.maximum_receipt_bytes() {
            return Err(ForecastEvidenceMaterialError::ObjectTooLarge);
        }
        Ok(bytes)
    }

    /// encode a fresh receipt for the storage owner which classifies its byte ceiling.
    pub(crate) fn encode_receipt(
        &self,
        prepared: &PreparedForecastEvidence,
        first_edge_committed_at: &str,
    ) -> Result<Vec<u8>, ForecastEvidenceMaterialError> {
        use ForecastEvidenceMaterialError as Error;
        // retain the exact canonical observed clock rather than the source issuance clock
        if instant(&self.codec, first_edge_committed_at).is_none() {
            return Err(Error::InvalidCommitClock);
        }
        let (availability_key, availability) = match prepared.version {
            ForecastEvidenceObjectVersion::Legacy => {
                ("availability", prepared.availability.clone())
            }
            ForecastEvidenceObjectVersion::Extended => {
                let canonical = canonical(&self.codec, &prepared.availability)?;
                let compressed = self
                    .codec
                    .gzip_level_nine_zero_mtime(&canonical)
                    .ok_or(Error::Compression)?;
                let encoded = self
                    .codec
                    .standard_base64(&compressed)
                    .ok_or(Error::CanonicalEncoding)?;
                ("availabilityGzipBase64", string(&encoded))
            }
        };
        let value = object([
            (availability_key, availability),
            (
                "contractVersion",
                string(prepared.version.receipt_contract()),
            ),
            (
                "edgeReceiptIdentitySha256",
                digest_string(prepared.edge_receipt_identity_sha256),
            ),
            ("firstEdgeCommittedAt", string(first_edge_committed_at)),
            ("objectSha256", digest_string(prepared.object_sha256)),
            ("siteKey", string("ballydidean")),
            ("window", string(prepared.window.wire_value())),
        ]);
        let mut bytes = canonical(&self.codec, &value)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// reuse only identical canonical bytes and promote their bounded immutable entry.
    fn compressed(
        &mut self,
        object_sha256: [u8; 32],
        canonical: Vec<u8>,
    ) -> Result<Arc<[u8]>, ForecastEvidenceMaterialError> {
        use ForecastEvidenceMaterialError as Error;
        // preserve the retained cache entry even when a supplied digest owner reports a collision
        if let Some(index) = self.cache.iter().position(
            // compare exact immutable content addresses
            |entry| entry.object_sha256 == object_sha256,
        ) {
            if self.cache[index].canonical != canonical {
                return Err(Error::IdentityCollision);
            }
            let entry = self.cache.remove(index).expect("retained cache entry");
            let compressed = Arc::clone(&entry.gzip);
            self.cache.push_back(entry);
            return Ok(compressed);
        }
        let gzip = self
            .codec
            .gzip_level_nine_zero_mtime(&canonical)
            .ok_or(Error::Compression)?;
        // retain only compressed material inside both source limits
        if gzip.len() > COMPRESSED_OBJECT_MAXIMUM_BYTES
            || canonical.len() > gzip.len().saturating_mul(MAXIMUM_COMPRESSION_RATIO)
        {
            return Err(Error::ObjectTooLarge);
        }
        let gzip: Arc<[u8]> = gzip.into();
        // evict the least recently used entry before storing another bounded object
        if self.cache.len() == COMPRESSION_CACHE_MAXIMUM_ENTRIES {
            self.cache.pop_front();
        }
        self.cache.push_back(CompressionCacheEntry {
            object_sha256,
            canonical,
            gzip: Arc::clone(&gzip),
        });
        Ok(gzip)
    }
}

impl<Codec: ForecastEvidenceReceiptCodec> ForecastEvidenceMaterial<Codec> {
    /// read an existing v1/v2 receipt without re-encoding or replacing its first bytes.
    pub fn admit_retained_receipt(
        &self,
        expected_identity: [u8; 32],
        bytes: &[u8],
    ) -> Option<RetainedForecastEvidenceReceipt> {
        // keep both supported stored-payload reads bounded
        if bytes.len() > LEGACY_RECEIPT_MAXIMUM_BYTES {
            return None;
        }
        let JsonValue::Object(fields) = self.codec.decode_retained_json(bytes)? else {
            return None;
        };
        let version_text = text(field(&fields, "contractVersion")?)?;
        let version = match version_text.as_str() {
            "forecast-adjustment-edge-receipt/v1" => ForecastEvidenceObjectVersion::Legacy,
            "forecast-adjustment-edge-receipt/v2" => ForecastEvidenceObjectVersion::Extended,
            _ => return None,
        };
        // require the exact versioned receipt envelope and content identity
        if bytes.len() > version.maximum_receipt_bytes()
            || fields.len() != 7
            || text(field(&fields, "edgeReceiptIdentitySha256")?)? != lower_hex(expected_identity)
            || text(field(&fields, "siteKey")?)? != "ballydidean"
        {
            return None;
        }
        let object = text(field(&fields, "objectSha256")?)?;
        let object_sha256 = *crate::RevisionGapSha256::from_lower_hex(&object)?.as_bytes();
        let first_edge_committed_at = text(field(&fields, "firstEdgeCommittedAt")?)?;
        instant(&self.codec, &first_edge_committed_at)?;
        let window = stored_window(field(&fields, "window")?)?;
        let availability_rows = match version {
            ForecastEvidenceObjectVersion::Legacy => validate_availability(
                &self.codec,
                field(&fields, "availability")?,
                MAXIMUM_LEGACY_ROWS,
            )?,
            ForecastEvidenceObjectVersion::Extended => {
                self.read_compact_availability(field(&fields, "availabilityGzipBase64")?)?
            }
        };
        Some(RetainedForecastEvidenceReceipt {
            availability_rows,
            first_edge_committed_at,
            object_sha256,
            version,
            window,
        })
    }

    /// validate exact retained gzip and its bounded canonical object before serving a snapshot.
    pub fn admit_retained_object(
        &self,
        expected: [u8; 32],
        gzip: &[u8],
    ) -> Option<RetainedForecastEvidenceObject> {
        // bound compressed data before asking the existing codec to expand it
        if gzip.len() > COMPRESSED_OBJECT_MAXIMUM_BYTES {
            return None;
        }
        let canonical = self.codec.gunzip_bounded(gzip, OBJECT_MAXIMUM_BYTES)?;
        // preserve exact deterministic material and its content address
        if canonical.len() > OBJECT_MAXIMUM_BYTES
            || canonical.len() > gzip.len().saturating_mul(MAXIMUM_COMPRESSION_RATIO)
            || self.codec.sha256(&canonical) != expected
            || self.codec.gzip_level_nine_zero_mtime(&canonical)? != gzip
        {
            return None;
        }
        let value = self.codec.decode_retained_json(&canonical)?;
        // retain only the original canonical object bytes
        if self.codec.canonical_json(&value)? != canonical {
            return None;
        }
        let JsonValue::Object(fields) = value else {
            return None;
        };
        let contract = text(field(&fields, "contractVersion")?)?;
        let (version, maximum_rows) = match contract.as_str() {
            "forecast-adjustment-evidence-object/v1" => {
                (ForecastEvidenceObjectVersion::Legacy, MAXIMUM_LEGACY_ROWS)
            }
            "forecast-adjustment-evidence-object/v2" => (
                ForecastEvidenceObjectVersion::Extended,
                MAXIMUM_EXTENDED_ROWS,
            ),
            _ => return None,
        };
        let JsonValue::Array(rows) = field(&fields, "rows")? else {
            return None;
        };
        let window = stored_window(field(&fields, "window")?)?;
        let settings_sha256 = field(&fields, "settingsSha256")?;
        let bundle_identities = field(&fields, "bundleIdentities")?;
        // apply the predecessor's exact outer schema and row ceiling
        if fields.len() != 6
            || rows.is_empty()
            || rows.len() > maximum_rows
            || text(field(&fields, "siteKey")?)? != "ballydidean"
            || !crate::revision_archive_canonical::is_sha256(&text(settings_sha256)?)
        {
            return None;
        }
        // orphans need valid objects; only a paired receipt requires stable row addresses
        let stable_receipt_identity_sha256 = stable_identity_for_stored(
            &self.codec,
            bundle_identities,
            rows,
            settings_sha256,
            window,
        );
        Some(RetainedForecastEvidenceObject {
            row_count: rows.len(),
            stable_receipt_identity_sha256,
            version,
            window,
        })
    }

    /// read and verify the exact deterministic compact availability member.
    fn read_compact_availability(&self, value: &JsonValue) -> Option<usize> {
        let encoded = text(value)?;
        // check the small v2 property before decoding it
        if encoded.len() > EXTENDED_RECEIPT_MAXIMUM_BYTES {
            return None;
        }
        let compressed = self.codec.decode_standard_base64(&encoded)?;
        // require the exact stored base64 spelling
        if self.codec.standard_base64(&compressed)? != encoded {
            return None;
        }
        let canonical = self
            .codec
            .gunzip_bounded(&compressed, EXTENDED_AVAILABILITY_MAXIMUM_BYTES)?;
        // inspect only bounded canonical availability bytes
        if canonical.len() > EXTENDED_AVAILABILITY_MAXIMUM_BYTES {
            return None;
        }
        let availability = self.codec.decode_retained_json(&canonical)?;
        // retain source-compatible deterministic gzip and canonical JSON
        if self.codec.canonical_json(&availability)? != canonical
            || self.codec.gzip_level_nine_zero_mtime(&canonical)? != compressed
        {
            return None;
        }
        validate_availability(&self.codec, &availability, MAXIMUM_EXTENDED_ROWS)
    }
}

/// validate the exact grouped timestamps and each bounded row reference.
fn validate_availability(
    codec: &impl ForecastEvidenceReceiptCodec,
    value: &JsonValue,
    maximum_rows: usize,
) -> Option<usize> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let (JsonValue::Array(indexes), JsonValue::Array(timestamps)) = (
        field(fields, "rowTimestampIndexes")?,
        field(fields, "timestamps")?,
    ) else {
        return None;
    };
    // preserve the reviewed exact availability keys and row count
    if fields.len() != 2 || indexes.len() > maximum_rows {
        return None;
    }
    // admit each historical canonical timestamp before dereferencing any retained row
    for timestamp in timestamps {
        instant(codec, &text(timestamp)?)?;
    }
    // require each row to reference one actually retained timestamp
    for index in indexes {
        let JsonValue::Number(value) = index else {
            return None;
        };
        // bound the finite integer before any numeric conversion
        if !value.is_finite()
            || *value < 0.0
            || value.fract() != 0.0
            || *value >= timestamps.len() as f64
        {
            return None;
        }
    }
    Some(indexes.len())
}

/// reconstruct only the stable record and provenance members used by first receipt addresses.
fn stable_identity_for_stored(
    codec: &impl ForecastEvidenceReceiptCodec,
    bundles: &JsonValue,
    rows: &[JsonValue],
    settings_sha256: &JsonValue,
    window: ForecastEvidenceWindow,
) -> Option<[u8; 32]> {
    let mut stable_rows = Vec::with_capacity(rows.len());
    // preserve each original record/source pair in the response order
    for row in rows {
        let JsonValue::Object(fields) = row else {
            return None;
        };
        stable_rows.push(object([
            ("record", field(fields, "record")?.clone()),
            ("source", field(fields, "source")?.clone()),
        ]));
    }
    let stable = object_without_scoring(
        bundles.clone(),
        stable_rows,
        settings_sha256.clone(),
        window,
    );
    Some(codec.sha256(&codec.canonical_json(&stable)?))
}

/// retain the closed source window spelling in a stored object or receipt.
fn stored_window(value: &JsonValue) -> Option<ForecastEvidenceWindow> {
    match text(value)?.as_str() {
        "days=1" => Some(ForecastEvidenceWindow::OneDay),
        "days=5" => Some(ForecastEvidenceWindow::FiveDays),
        "days=10" => Some(ForecastEvidenceWindow::TenDays),
        "overnight" => Some(ForecastEvidenceWindow::Overnight),
        _ => None,
    }
}

/// find one exact existing field without reordering historical JSON.
fn field<'a>(fields: &'a [(Vec<u16>, JsonValue)], key: &str) -> Option<&'a JsonValue> {
    fields.iter().find_map(
        // compare original UTF-16 units to each known protocol field name
        |(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value),
    )
}

/// project an admitted textual identity into its ordinary Rust string.
fn text(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(units) => String::from_utf16(units).ok(),
        _ => None,
    }
}

/// construct the canonical stable identity without volatile clocks or correction quantities.
fn object_without_scoring(
    bundle_identities: JsonValue,
    stable_rows: Vec<JsonValue>,
    settings_sha256: JsonValue,
    window: ForecastEvidenceWindow,
) -> JsonValue {
    object([
        ("bundleIdentities", bundle_identities),
        ("rows", JsonValue::Array(stable_rows)),
        ("settingsSha256", settings_sha256),
        ("siteKey", string("ballydidean")),
        ("window", string(window.wire_value())),
    ])
}

/// forward exact ordered canonical bytes from the owner's JSON formatter.
fn canonical(
    codec: &impl ForecastEvidenceMaterialCodec,
    value: &JsonValue,
) -> Result<Vec<u8>, ForecastEvidenceMaterialError> {
    codec
        .canonical_json(value)
        .ok_or(ForecastEvidenceMaterialError::CanonicalEncoding)
}

/// construct one checked canonical hash field without a new crypto implementation.
fn digest_string(bytes: [u8; 32]) -> JsonValue {
    string(&lower_hex(bytes))
}

/// construct one original wire string with all known code units.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// construct one fixed-shape object without encoding any JSON syntax.
fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(
                // retain each exact source-owned field spelling
                |(name, value)| (name.encode_utf16().collect(), value),
            )
            .collect(),
    )
}
