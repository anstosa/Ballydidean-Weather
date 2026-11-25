//! Reconstruct the legacy v2 maintenance retirement expectation without deleting hot metadata.

use std::path::Path;

use weather_contracts::{ForecastArchiveFinalManifestV2ContractVersionLiteral, JsonValue};
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS,
    FORECAST_CYCLE_MAXIMUM_PAGES, FORECAST_CYCLE_PAGE_CONTRACT_VERSION,
    FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES, ForecastCycleFinalAcknowledgement,
    ForecastCycleFinalManifest, ForecastCycleSha256, ForecastCycleTransferCheckpoint,
    ForecastCycleTransferCheckpointCodec, RetainedForecastCycleManifest,
    RevisionArchiveCanonicalCodec, RevisionGapSha256,
};

use crate::MaintenanceCanonicalJson;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_retirement_files_v2::{
    NativeMaintenanceRetirementEntryV2, NativeMaintenanceRetirementFilesV2Error,
    NativeMaintenanceRetirementKindV2, format_digest, read_expected_maintenance_member_v2,
    read_maintenance_cycle_acknowledgement_names_v2,
};

const FINAL_V2: &str = ForecastArchiveFinalManifestV2ContractVersionLiteral::WIRE_VALUE;
const PAGE_ACK_VERSION: &str = "adjustment-cycle-page-ack/v1";
const SAFE_JS_INTEGER_MAXIMUM: u64 = 9_007_199_254_740_991;

/// One final manifest already admitted by the existing native final-manifest store.
pub struct AdmittedNativeMaintenanceRetirementFinalV2<'a> {
    document: &'a JsonValue,
    manifest_sha256: String,
}

impl<'a> AdmittedNativeMaintenanceRetirementFinalV2<'a> {
    /// Retain only the document and immutable address admitted by the native cycle owner.
    pub fn from_retained(
        final_manifest: &'a RetainedForecastCycleManifest<ForecastCycleFinalManifest>,
    ) -> Self {
        Self {
            document: final_manifest.manifest.document(),
            manifest_sha256: format_digest(*final_manifest.manifest_sha256.as_bytes()),
        }
    }
}

/// Exact v2 finalization inputs reconstructed from retained native authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceRetirementExpectedV2 {
    pub graph_sha256: String,
    pub input_seal_sha256: String,
    pub required_input_root_sha256: String,
    pub revision_input_count: u64,
    pub complete_producers: bool,
    pub entries: Vec<NativeMaintenanceRetirementEntryV2>,
    pub canonical_entries: Vec<u8>,
    pub retirement_set_root_sha256: String,
}

/// Bounded expected-graph failures, including the deliberately unclaimed revision graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceRetirementExpectedV2Error {
    FinalManifest,
    FinalAcknowledgement,
    ProducerPopulation,
    PageAcknowledgement,
    TransferAcknowledgement,
    Store,
    Codec,
    RevisionOwner,
}

/// Source authority consumed by the generic v2 finalizer after a manifest address is selected.
pub trait NativeMaintenanceRetirementExpectedV2Authority {
    /// Read one exact expected graph or reject every unavailable authority class.
    fn read_expected(
        &mut self,
        manifest_sha256: &str,
    ) -> Result<NativeMaintenanceRetirementExpectedV2, NativeMaintenanceRetirementExpectedV2Error>;
}

/// Collect every exact legacy metadata member supported by the native archive owners.
pub fn collect_native_maintenance_retirement_entries_v2<P: ArchiveWirePrimitives>(
    root: &Path,
    final_manifest: &AdmittedNativeMaintenanceRetirementFinalV2<'_>,
    final_acknowledgement: &ForecastCycleFinalAcknowledgement,
    codec: &mut impl MaintenanceCanonicalJson,
    wire: &ArchiveWireCodec<P>,
    transfer_codec: &impl ForecastCycleTransferCheckpointCodec,
) -> Result<NativeMaintenanceRetirementExpectedV2, NativeMaintenanceRetirementExpectedV2Error> {
    let acknowledged_manifest_sha256 =
        format_digest(*final_acknowledgement.manifest_sha256.as_bytes());
    let graph_sha256 = format_digest(*final_acknowledgement.graph_sha256.as_bytes());
    if acknowledged_manifest_sha256 != final_manifest.manifest_sha256
        || !valid_hash(&graph_sha256)
        || !valid_hash(&final_manifest.manifest_sha256)
    {
        return Err(NativeMaintenanceRetirementExpectedV2Error::FinalAcknowledgement);
    }
    let document = final_manifest.document;
    let input_seal = field(document, "inputSeal")
        .filter(|_| text(document, "contractVersion").as_deref() == Some(FINAL_V2))
        .ok_or(NativeMaintenanceRetirementExpectedV2Error::FinalManifest)?;
    let required_inputs = field(input_seal, "requiredInputs")
        .ok_or(NativeMaintenanceRetirementExpectedV2Error::FinalManifest)?;
    let pages = match field(document, "pages") {
        Some(JsonValue::Array(pages)) if !pages.is_empty() => pages,
        _ => return Err(NativeMaintenanceRetirementExpectedV2Error::FinalManifest),
    };
    let due_key = text(document, "dueKey")
        .ok_or(NativeMaintenanceRetirementExpectedV2Error::FinalManifest)?;
    let generation = text(document, "generation")
        .ok_or(NativeMaintenanceRetirementExpectedV2Error::FinalManifest)?;
    let input_seal_bytes = codec
        .encode_canonical_json(input_seal)
        .ok_or(NativeMaintenanceRetirementExpectedV2Error::Codec)?;
    let required_input_bytes = codec
        .encode_canonical_json(required_inputs)
        .ok_or(NativeMaintenanceRetirementExpectedV2Error::Codec)?;
    let input_seal_sha256 = digest(codec, &input_seal_bytes)?;
    let required_input_root_sha256 = digest(codec, &required_input_bytes)?;
    let (complete_producers, revision_input_count) = producer_population(required_inputs)?;
    if !complete_producers {
        return Err(NativeMaintenanceRetirementExpectedV2Error::ProducerPopulation);
    }

    let mut entries = Vec::with_capacity(pages.len() * 2 + 3);
    // bind both server and workstation acknowledgements for every admitted final page
    for page in pages {
        let page_sha256 = text(page, "pageSha256")
            .filter(|value| valid_hash(value))
            .ok_or(NativeMaintenanceRetirementExpectedV2Error::FinalManifest)?;
        let (acknowledgement_sha256, acknowledgement_bytes) =
            read_page_acknowledgement(root, &page_sha256, codec, wire)?;
        add_entry(
            &mut entries,
            NativeMaintenanceRetirementKindV2::CycleAcknowledgement,
            acknowledgement_sha256,
            &acknowledgement_bytes,
            codec,
        )?;
        let transfer_bytes = required_member(
            root,
            NativeMaintenanceRetirementKindV2::ArchiveAcknowledgement,
            &page_sha256,
        )?;
        let page_digest = RevisionGapSha256::from_lower_hex(&page_sha256)
            .map(|digest| ForecastCycleSha256::admitted(*digest.as_bytes()))
            .ok_or(NativeMaintenanceRetirementExpectedV2Error::TransferAcknowledgement)?;
        let _transfer = transfer_codec
            .decode_transfer_checkpoint(&transfer_bytes)
            .filter(|transfer| transfer.page_sha256 == page_digest)
            .filter(|transfer| valid_transfer_checkpoint(transfer, wire))
            .ok_or(NativeMaintenanceRetirementExpectedV2Error::TransferAcknowledgement)?;
        add_entry(
            &mut entries,
            NativeMaintenanceRetirementKindV2::ArchiveAcknowledgement,
            page_sha256,
            &transfer_bytes,
            codec,
        )?;
    }

    let seal_identity_value = JsonValue::Object(vec![
        ("dueKey".encode_utf16().collect(), json_string(&due_key)),
        (
            "generation".encode_utf16().collect(),
            json_string(&generation),
        ),
    ]);
    let seal_identity_bytes = codec
        .encode_canonical_json(&seal_identity_value)
        .ok_or(NativeMaintenanceRetirementExpectedV2Error::Codec)?;
    let seal_identity = digest(codec, &seal_identity_bytes)?;
    add_required_member(
        root,
        &mut entries,
        NativeMaintenanceRetirementKindV2::InputSeal,
        seal_identity,
        codec,
    )?;
    add_required_member(
        root,
        &mut entries,
        NativeMaintenanceRetirementKindV2::FinalManifest,
        final_manifest.manifest_sha256.clone(),
        codec,
    )?;
    add_required_member(
        root,
        &mut entries,
        NativeMaintenanceRetirementKindV2::FinalAcknowledgement,
        final_manifest.manifest_sha256.clone(),
        codec,
    )?;

    // stop rather than inventing receipt, projection, capsule or terminal-gap graph authority
    // join the authentic nonempty revision/capsule/publication/gap source owner
    let revision_entries =
        crate::collect_native_maintenance_revision_retirement_v2(root, wire, wire)
            .map_err(|_| NativeMaintenanceRetirementExpectedV2Error::RevisionOwner)?;
    entries.extend(revision_entries);
    entries.sort_by(|left, right| {
        left.kind
            .as_str()
            .cmp(right.kind.as_str())
            .then_with(|| left.identity_sha256.cmp(&right.identity_sha256))
    });
    let values = JsonValue::Array(entries.iter().map(|entry| entry.to_json()).collect());
    let canonical_entries = codec
        .encode_canonical_json(&values)
        .ok_or(NativeMaintenanceRetirementExpectedV2Error::Codec)?;
    let retirement_set_root_sha256 = digest(codec, &canonical_entries)?;
    Ok(NativeMaintenanceRetirementExpectedV2 {
        graph_sha256,
        input_seal_sha256,
        required_input_root_sha256,
        revision_input_count,
        complete_producers,
        entries,
        canonical_entries,
        retirement_set_root_sha256,
    })
}

/// Read one canonical page acknowledgement selected by its admitted page identity.
fn read_page_acknowledgement(
    root: &Path,
    page_sha256: &str,
    codec: &mut impl MaintenanceCanonicalJson,
    wire: &impl RevisionArchiveCanonicalCodec,
) -> Result<(String, Vec<u8>), NativeMaintenanceRetirementExpectedV2Error> {
    let names = read_maintenance_cycle_acknowledgement_names_v2(root).map_err(store_error)?;
    // preserve the predecessor's sorted first matching acknowledgement scan
    for name in names {
        let identity = name
            .strip_prefix("sha256-")
            .and_then(|value| value.strip_suffix(".json"))
            .ok_or(NativeMaintenanceRetirementExpectedV2Error::PageAcknowledgement)?;
        let bytes = required_member(
            root,
            NativeMaintenanceRetirementKindV2::CycleAcknowledgement,
            identity,
        )?;
        let value = codec
            .parse_canonical_object(&bytes)
            .ok_or(NativeMaintenanceRetirementExpectedV2Error::PageAcknowledgement)?;
        if codec.encode_canonical_json(&value).as_deref() != Some(bytes.as_slice())
            || digest(codec, &bytes)? != identity
            || !valid_page_acknowledgement(&value, wire)
        {
            return Err(NativeMaintenanceRetirementExpectedV2Error::PageAcknowledgement);
        }
        if text(&value, "pageSha256").as_deref() == Some(page_sha256) {
            return Ok((identity.to_owned(), bytes));
        }
    }
    Err(NativeMaintenanceRetirementExpectedV2Error::PageAcknowledgement)
}

/// Validate the exact closed page acknowledgement schema and scalar bounds.
fn valid_page_acknowledgement(
    value: &JsonValue,
    wire: &impl RevisionArchiveCanonicalCodec,
) -> bool {
    if !exact_keys(
        value,
        &[
            "acknowledgedAt",
            "contractVersion",
            "dueKey",
            "generation",
            "pageIndex",
            "pageSha256",
        ],
    ) || text(value, "contractVersion").as_deref() != Some(PAGE_ACK_VERSION)
        || !text(value, "dueKey").as_deref().is_some_and(valid_due_key)
        || !text(value, "pageSha256").as_deref().is_some_and(valid_hash)
        || !text(value, "acknowledgedAt")
            .as_deref()
            .is_some_and(|value| {
                utc_millisecond_shape(value) && wire.canonical_utc_milliseconds(value).is_some()
            })
        || !text(value, "generation").as_deref().is_some_and(valid_u64)
    {
        return false;
    }
    matches!(
        field(value, "pageIndex"),
        Some(JsonValue::Number(index))
            if index.is_finite()
                && index.fract() == 0.0
                && *index >= 0.0
                && *index < FORECAST_CYCLE_MAXIMUM_PAGES as f64
    )
}

/// Retain the existing typed transfer checkpoint invariants after canonical decode.
fn valid_transfer_checkpoint(
    transfer: &ForecastCycleTransferCheckpoint,
    wire: &impl RevisionArchiveCanonicalCodec,
) -> bool {
    let header = &transfer.header;
    if transfer.contract_version != "adjustment-archive-transfer-ack/v1"
        || header.contract_version != FORECAST_CYCLE_PAGE_CONTRACT_VERSION
        || !valid_due_key(&header.due_key)
        || header.page_index >= FORECAST_CYCLE_MAXIMUM_PAGES
        || header.payload_offset > SAFE_JS_INTEGER_MAXIMUM
        || !(1..=FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES).contains(&transfer.payload_length)
        || !(1..=FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS).contains(&transfer.projections.len())
        || wire
            .canonical_utc_milliseconds(&transfer.acknowledged_at)
            .is_none()
        || !utc_millisecond_shape(&transfer.acknowledged_at)
        || (header.page_index == 0
            && (header.predecessor_page_sha256.is_some() || header.payload_offset != 0))
        || (header.page_index > 0 && header.predecessor_page_sha256.is_none())
    {
        return false;
    }
    let mut identities = transfer
        .projections
        .iter()
        .map(|projection| projection.identity_sha256)
        .collect::<Vec<_>>();
    identities.sort_unstable();
    !identities.windows(2).any(|pair| pair[0] == pair[1])
}

/// Validate required producer closure and count server-ordinal revision classes.
fn producer_population(
    required_inputs: &JsonValue,
) -> Result<(bool, u64), NativeMaintenanceRetirementExpectedV2Error> {
    let required = [
        "actual_best_match",
        "native_source",
        "rain_gate_input",
        "shadow_body",
        "shadow_metadata",
        "target_revision",
    ];
    let mut counts = Vec::with_capacity(required.len());
    // extract only counts from the already admitted native input-seal schema
    for name in required {
        let population = field(required_inputs, name)
            .ok_or(NativeMaintenanceRetirementExpectedV2Error::FinalManifest)?;
        let count = match field(population, "count") {
            Some(JsonValue::Number(count))
                if count.is_finite()
                    && count.fract() == 0.0
                    && (0.0..=9_500_000.0).contains(count) =>
            {
                *count as u64
            }
            _ => return Err(NativeMaintenanceRetirementExpectedV2Error::FinalManifest),
        };
        counts.push(count);
    }
    let complete = counts.iter().all(|count| *count > 0);
    let revision_count = counts[0] + counts[1] + counts[2] + counts[5];
    Ok((complete, revision_count))
}

/// Read one required fixed-address member without treating absence as an empty graph.
fn required_member(
    root: &Path,
    kind: NativeMaintenanceRetirementKindV2,
    identity: &str,
) -> Result<Vec<u8>, NativeMaintenanceRetirementExpectedV2Error> {
    read_expected_maintenance_member_v2(root, kind, identity)
        .map_err(store_error)?
        .ok_or(NativeMaintenanceRetirementExpectedV2Error::Store)
}

/// Read, hash and append one fixed-address member.
fn add_required_member(
    root: &Path,
    entries: &mut Vec<NativeMaintenanceRetirementEntryV2>,
    kind: NativeMaintenanceRetirementKindV2,
    identity: String,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<(), NativeMaintenanceRetirementExpectedV2Error> {
    let bytes = required_member(root, kind, &identity)?;
    add_entry(entries, kind, identity, &bytes, codec)
}

/// Append one exact redundant byte identity to the expected retirement set.
fn add_entry(
    entries: &mut Vec<NativeMaintenanceRetirementEntryV2>,
    kind: NativeMaintenanceRetirementKindV2,
    identity: String,
    bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<(), NativeMaintenanceRetirementExpectedV2Error> {
    let file_sha256 = digest(codec, bytes)?;
    entries.push(
        NativeMaintenanceRetirementEntryV2::new(kind, identity, file_sha256)
            .ok_or(NativeMaintenanceRetirementExpectedV2Error::Codec)?,
    );
    Ok(())
}

/// Hash exact bytes through the admitted codec and retain lowercase wire spelling.
fn digest(
    codec: &mut impl MaintenanceCanonicalJson,
    bytes: &[u8],
) -> Result<String, NativeMaintenanceRetirementExpectedV2Error> {
    codec
        .sha256(bytes)
        .map(|digest| digest.as_str().to_owned())
        .filter(|digest| valid_hash(digest))
        .ok_or(NativeMaintenanceRetirementExpectedV2Error::Codec)
}

/// Map fixed-file failures without exposing path or ownership details.
fn store_error(
    _: NativeMaintenanceRetirementFilesV2Error,
) -> NativeMaintenanceRetirementExpectedV2Error {
    NativeMaintenanceRetirementExpectedV2Error::Store
}

/// Recognize one canonical decimal unsigned 64-bit string.
fn valid_u64(value: &str) -> bool {
    value
        .parse::<u64>()
        .ok()
        .is_some_and(|parsed| parsed.to_string() == value)
}

/// Recognize the exact fixed scheduler due-key grammar used by the cycle owner.
fn valid_due_key(value: &str) -> bool {
    let Some(value) = value.strip_prefix("capture/") else {
        return false;
    };
    let bytes = value.as_bytes();
    bytes.len() == 24
        && bytes.iter().enumerate().all(|(index, byte)| {
            // retain punctuation only at the source's fixed clock positions
            match index {
                4 | 7 => *byte == b'-',
                10 => *byte == b'T',
                13 | 16 => *byte == b':',
                19 => *byte == b'.',
                23 => *byte == b'Z',
                _ => byte.is_ascii_digit(),
            }
        })
        && matches!(&bytes[11..13], b"00" | b"06" | b"12" | b"18")
        && &bytes[14..19] == b"35:00"
        && &bytes[20..23] == b"000"
}

/// Recognize the fixed source UTC millisecond shape before parsing its calendar value.
fn utc_millisecond_shape(value: &str) -> bool {
    let shape = b"dddd-dd-ddTdd:dd:dd.dddZ";
    value.len() == shape.len()
        && value.bytes().zip(shape).all(|(observed, expected)| {
            // require decimal digits at every variable position
            if *expected == b'd' {
                observed.is_ascii_digit()
            } else {
                observed == *expected
            }
        })
}

/// Construct one generated UTF-16 JSON string.
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_retirement_expected_v2_test.rs"]
mod tests;
