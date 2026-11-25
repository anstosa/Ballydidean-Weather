//! Bind the cycle page's preexisting canonical documents to approved runtime codecs.

use weather_contracts::{
    ForecastArchivePageHeaderV1, ForecastArchivePageHeaderV1ContractVersionLiteral,
    ForecastArchiveProjection, ForecastArchiveProjectionChannelEnum, JsonValue, Nullable,
};

use crate::ForecastEvidenceReceiptCodec;
use crate::forecast_cycle_pages::{
    FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS, FORECAST_CYCLE_MAXIMUM_PAGES,
    FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION, FORECAST_CYCLE_PAGE_CONTRACT_VERSION,
    ForecastCyclePageAcknowledgement, ForecastCyclePageCodec, ForecastCyclePageHeader,
    ForecastCycleProjection, ForecastCycleProjectionChannel, ForecastCycleSha256, valid_due_key,
};
use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalCodec, instant, is_sha256, lower_hex,
};

use super::forecast_cycle_disk_ack::ForecastCycleDiskAckReadCodec;
use super::forecast_cycle_disk_slots::{ForecastCycleDiskSlotCodec, ForecastCycleDiskSlotMetadata};

const SAFE_WIRE_INTEGER: u64 = 9_007_199_254_740_991;

type Object = [(Vec<u16>, JsonValue)];

/// A streaming digest extension over the same approved codec, avoiding a payload copy.
pub trait ForecastCycleHashPair {
    /// Hash exactly the header followed by the payload.
    fn sha256_pair(&self, header: &[u8], payload: &[u8]) -> [u8; 32];
}

/// Use the same runtime codecs as retained evidence and keep page logic wire-neutral.
pub struct BoundForecastCyclePageCodec<'a, Codec>(&'a Codec);

impl<'a, Codec> BoundForecastCyclePageCodec<'a, Codec> {
    /// Bind the same approval-scoped codec used for captured evidence.
    pub const fn new(codec: &'a Codec) -> Self {
        Self(codec)
    }
}

impl<Codec: ForecastEvidenceReceiptCodec> RevisionArchiveCanonicalCodec
    for BoundForecastCyclePageCodec<'_, Codec>
{
    /// Preserve the existing canonical UTC observation rules.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        self.0.canonical_utc_milliseconds(text)
    }

    /// Hash an original one-segment source document through the same owner.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        self.0.sha256(bytes)
    }
}

impl<Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair> ForecastCyclePageCodec
    for BoundForecastCyclePageCodec<'_, Codec>
{
    /// Encode the exact header used in the predecessor-bound page digest.
    fn encode_page_header(&self, header: &ForecastCyclePageHeader) -> Option<Vec<u8>> {
        self.frame(&page_header(header)?)
    }

    /// Encode the acknowledgement before adding the persisted proof digest.
    fn encode_page_acknowledgement(
        &self,
        acknowledgement: &ForecastCyclePageAcknowledgement,
    ) -> Option<Vec<u8>> {
        self.frame(&object([
            ("contractVersion", text(acknowledgement.contract_version)),
            ("dueKey", text(&acknowledgement.due_key)),
            ("generation", text(&acknowledgement.generation.to_string())),
            (
                "pageIndex",
                exact_number(acknowledgement.page_index.try_into().ok()?)?,
            ),
            ("pageSha256", digest(acknowledgement.page_sha256)),
            ("acknowledgedAt", text(&acknowledgement.acknowledged_at)),
        ]))
    }

    /// Ask the owning streaming codec for the exact digest over both byte ranges.
    fn sha256_pair(&self, first: &[u8], second: &[u8]) -> [u8; 32] {
        self.0.sha256_pair(first, second)
    }
}

impl<Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair> ForecastCycleDiskSlotCodec
    for BoundForecastCyclePageCodec<'_, Codec>
{
    /// Encode the exact disk slot metadata around a byte-independent page identity.
    fn encode_disk_slot_metadata(
        &self,
        metadata: &ForecastCycleDiskSlotMetadata,
    ) -> Option<Vec<u8>> {
        self.frame(&object([
            ("header", page_header(&metadata.header)?),
            ("pageSha256", digest(metadata.page_sha256)),
            (
                "projections",
                JsonValue::Array(metadata.projections.iter().map(projection).collect()),
            ),
        ]))
    }

    /// Decode the canonical stored metadata without loading its value-bearing payload.
    fn decode_disk_slot_metadata(&self, bytes: &[u8]) -> Option<ForecastCycleDiskSlotMetadata> {
        let value = self.0.decode_retained_json(bytes)?;
        // preserve the source document's exact byte equality before projecting known fields
        if self.frame(&value)? != bytes {
            return None;
        }
        let fields = read_object(&value)?;
        let header = read_page_header(read_field(fields, "header")?)?;
        let page_sha256 = read_digest(read_field(fields, "pageSha256")?)?;
        let JsonValue::Array(values) = read_field(fields, "projections")? else {
            return None;
        };
        // bound the projection list before restoring its original retained order
        if values.len() > FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS {
            return None;
        }
        let projections = values
            .iter()
            .map(read_projection)
            .collect::<Option<Vec<_>>>()?;
        Some(ForecastCycleDiskSlotMetadata {
            header,
            page_sha256,
            projections,
        })
    }
}

impl<Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair> ForecastCycleDiskAckReadCodec
    for BoundForecastCyclePageCodec<'_, Codec>
{
    /// Decode exactly the original source acknowledgement and validate its durable clock.
    fn decode_page_acknowledgement(
        &self,
        bytes: &[u8],
    ) -> Option<ForecastCyclePageAcknowledgement> {
        let value = self.0.decode_retained_json(bytes)?;
        let fields = read_object(&value)?;
        // bind the closed source contract and canonical bytes before projecting fields
        if fields.len() != 6
            || read_text(read_field(fields, "contractVersion")?)?
                != FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION
            || self.frame(&value)? != bytes
        {
            return None;
        }
        let due_key = read_text(read_field(fields, "dueKey")?)?;
        let acknowledged_at = read_text(read_field(fields, "acknowledgedAt")?)?;
        let page_index: usize = read_number(read_field(fields, "pageIndex")?)?
            .try_into()
            .ok()?;
        // admit the exact same scope, clock and page cap as the retained source
        if !valid_due_key(&due_key)
            || instant(self, &acknowledged_at).is_none()
            || page_index >= FORECAST_CYCLE_MAXIMUM_PAGES
        {
            return None;
        }
        Some(ForecastCyclePageAcknowledgement {
            contract_version: FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION,
            acknowledged_at,
            due_key,
            generation: read_generation(read_field(fields, "generation")?)?,
            page_index,
            page_sha256: read_digest(read_field(fields, "pageSha256")?)?,
        })
    }
}

impl<Codec: ForecastEvidenceReceiptCodec> BoundForecastCyclePageCodec<'_, Codec> {
    /// Reuse the exact same admitted parser for sibling immutable record kinds.
    pub(super) const fn byte_codec(&self) -> &Codec {
        self.0
    }

    /// Apply only the source newline envelope around canonical JSON bytes.
    pub(super) fn frame(&self, value: &JsonValue) -> Option<Vec<u8>> {
        let mut bytes = self.0.canonical_json(value)?;
        bytes.push(b'\n');
        Some(bytes)
    }
}

/// Project runtime custody data into the generated TypeScript page-header contract.
pub(super) fn wire_page_header(
    header: &ForecastCyclePageHeader,
) -> Option<ForecastArchivePageHeaderV1> {
    // bind the admitted runtime contract before narrowing the original size fields
    if header.contract_version != ForecastArchivePageHeaderV1ContractVersionLiteral::WIRE_VALUE {
        return None;
    }
    Some(ForecastArchivePageHeaderV1 {
        contractVersion: ForecastArchivePageHeaderV1ContractVersionLiteral,
        dueKey: header.due_key.clone(),
        generation: header.generation.to_string(),
        pageIndex: header.page_index.try_into().ok()?,
        predecessorPageSha256: header
            .predecessor_page_sha256
            .map_or(Nullable::Null, |value| {
                Nullable::Value(lower_hex(*value.as_bytes()))
            }),
        payloadSha256: lower_hex(*header.payload_sha256.as_bytes()),
        payloadOffset: header.payload_offset,
    })
}

/// Express one generated page header through the admitted generic JSON owner.
pub(super) fn page_header_document(value: &ForecastArchivePageHeaderV1) -> Option<JsonValue> {
    Some(object([
        (
            "contractVersion",
            text(ForecastArchivePageHeaderV1ContractVersionLiteral::WIRE_VALUE),
        ),
        ("dueKey", text(&value.dueKey)),
        ("generation", text(&value.generation)),
        ("pageIndex", exact_number(value.pageIndex)?),
        (
            "predecessorPageSha256",
            match &value.predecessorPageSha256 {
                Nullable::Null => JsonValue::Null,
                Nullable::Value(hash) => text(hash),
            },
        ),
        ("payloadSha256", text(&value.payloadSha256)),
        ("payloadOffset", exact_number(value.payloadOffset)?),
    ]))
}

/// Retain the same generated header contract for hashes, slots, and transfer receipts.
pub(super) fn page_header(header: &ForecastCyclePageHeader) -> Option<JsonValue> {
    page_header_document(&wire_page_header(header)?)
}

/// Project a retained channel and stable identity into the generated contract.
pub(super) fn wire_projection(value: &ForecastCycleProjection) -> ForecastArchiveProjection {
    ForecastArchiveProjection {
        channel: match value.channel {
            ForecastCycleProjectionChannel::Organic => {
                ForecastArchiveProjectionChannelEnum::Organic
            }
            ForecastCycleProjectionChannel::SchedulerRequest => {
                ForecastArchiveProjectionChannelEnum::SchedulerRequest
            }
        },
        identitySha256: lower_hex(*value.identity_sha256.as_bytes()),
    }
}

/// Express one generated projection without changing its original channel spelling.
pub(super) fn projection_document(value: &ForecastArchiveProjection) -> JsonValue {
    let channel = ForecastArchiveProjectionChannelEnum::WIRE_VALUES
        .iter()
        .find_map(|(label, channel)| (*channel == value.channel).then_some(*label))
        .expect("generated archive channels carry exact wire values");
    object([
        ("channel", text(channel)),
        ("identitySha256", text(&value.identitySha256)),
    ])
}

/// Keep the existing projection wire representation shared by custody and transfer.
pub(super) fn projection(value: &ForecastCycleProjection) -> JsonValue {
    projection_document(&wire_projection(value))
}

/// Represent a fixed typed object inside the generated value surface.
pub(super) fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Preserve an exact source text value in its original typed UTF-16 representation.
pub(super) fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Do not round a retained page offset through a wider JavaScript number.
fn exact_number(value: u64) -> Option<JsonValue> {
    (value <= SAFE_WIRE_INTEGER).then_some(JsonValue::Number(value as f64))
}

/// Emit only the existing lowercase content address.
pub(super) fn digest(value: ForecastCycleSha256) -> JsonValue {
    text(&lower_hex(*value.as_bytes()))
}

/// Read the exact generated JSON object surface returned by the approved decoder.
pub(super) fn read_object(value: &JsonValue) -> Option<&Object> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    Some(fields)
}

/// Match the original UTF-16 member spelling without normalizing it.
pub(super) fn read_field<'a>(fields: &'a Object, key: &str) -> Option<&'a JsonValue> {
    fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
}

/// Read one exact wire string from the admitted syntax.
pub(super) fn read_text(value: &JsonValue) -> Option<String> {
    let JsonValue::String(units) = value else {
        return None;
    };
    String::from_utf16(units).ok()
}

/// Read one safe wire number without silently changing an exact offset.
pub(super) fn read_number(value: &JsonValue) -> Option<u64> {
    let JsonValue::Number(number) = value else {
        return None;
    };
    (number.fract() == 0.0 && (0.0..=SAFE_WIRE_INTEGER as f64).contains(number))
        .then_some(*number as u64)
}

/// Decode the existing decimal string without leading zeros or suffixes.
pub(super) fn read_generation(value: &JsonValue) -> Option<u64> {
    let source = read_text(value)?;
    let number: u64 = source.parse().ok()?;
    (number.to_string() == source).then_some(number)
}

/// Admit one source lowercase content address after structural decoding.
pub(super) fn read_digest(value: &JsonValue) -> Option<ForecastCycleSha256> {
    let text = read_text(value)?;
    // bind ASCII extent before slicing two-digit hex bytes
    if !is_sha256(&text) {
        return None;
    }
    let mut bytes = [0; 32];
    // retain only the admitted digest value, never the surrounding field text
    for (index, value) in bytes.iter_mut().enumerate() {
        *value = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(ForecastCycleSha256::admitted(bytes))
}

/// Read and cross-check the retained page header without touching the payload.
pub(super) fn read_page_header(value: &JsonValue) -> Option<ForecastCyclePageHeader> {
    let fields = read_object(value)?;
    // enforce the same closed header keys and source contract
    if fields.len() != 7
        || read_text(read_field(fields, "contractVersion")?)?
            != FORECAST_CYCLE_PAGE_CONTRACT_VERSION
    {
        return None;
    }
    let due_key = read_text(read_field(fields, "dueKey")?)?;
    let generation = read_generation(read_field(fields, "generation")?)?;
    let page_index: usize = read_number(read_field(fields, "pageIndex")?)?
        .try_into()
        .ok()?;
    let predecessor_page_sha256 = match read_field(fields, "predecessorPageSha256")? {
        JsonValue::Null => None,
        value => Some(read_digest(value)?),
    };
    let payload_sha256 = read_digest(read_field(fields, "payloadSha256")?)?;
    let payload_offset = read_number(read_field(fields, "payloadOffset")?)?;
    // require finite cycle bounds and the exact genesis or successor formation
    if !valid_due_key(&due_key)
        || page_index >= FORECAST_CYCLE_MAXIMUM_PAGES
        || (page_index == 0) != predecessor_page_sha256.is_none()
        || (page_index == 0 && payload_offset != 0)
    {
        return None;
    }
    Some(ForecastCyclePageHeader {
        contract_version: FORECAST_CYCLE_PAGE_CONTRACT_VERSION,
        due_key,
        generation,
        page_index,
        predecessor_page_sha256,
        payload_sha256,
        payload_offset,
    })
}

/// Project the closed source channel and immutable identity of one retained member.
pub(super) fn read_projection(value: &JsonValue) -> Option<ForecastCycleProjection> {
    let fields = read_object(value)?;
    // reject added projection fields rather than ignoring them
    if fields.len() != 2 {
        return None;
    }
    let channel = match read_text(read_field(fields, "channel")?)?.as_str() {
        "organic" => ForecastCycleProjectionChannel::Organic,
        "scheduler_request" => ForecastCycleProjectionChannel::SchedulerRequest,
        _ => return None,
    };
    Some(ForecastCycleProjection {
        channel,
        identity_sha256: read_digest(read_field(fields, "identitySha256")?)?,
    })
}
