//! Bind validated immutable Node final manifests to the existing generated JSON codec.

use weather_contracts::{
    ForecastArchiveFinalManifestV1ContractVersionLiteral,
    ForecastArchiveFinalManifestV2ContractVersionLiteral, JsonValue,
};

use crate::ForecastEvidenceReceiptCodec;
use crate::archive_wire::{ArchiveWireCodec, ArchiveWirePrimitives};
use crate::forecast_cycle_pages::{
    FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS, FORECAST_CYCLE_MAXIMUM_PAGES,
    FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES, ForecastCycleSha256, valid_due_key,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant};

use super::forecast_cycle_final_store::ForecastCycleFinalManifestCodec;
use super::forecast_cycle_metadata_codec::ForecastCycleMetadataCodec;
use super::forecast_cycle_page_binding::{
    BoundForecastCyclePageCodec, read_digest, read_field, read_generation, read_number,
    read_object, read_text,
};

const FINAL_V1: &str = ForecastArchiveFinalManifestV1ContractVersionLiteral::WIRE_VALUE;
const FINAL_V2: &str = ForecastArchiveFinalManifestV2ContractVersionLiteral::WIRE_VALUE;
const CYCLE_MAXIMUM_PAYLOAD_BYTES: u64 = 4_832 * 1_024;
const INPUT_SEAL_MAXIMUM_BYTES: usize = 2_048;
const REQUIRED_INPUT_MAXIMUM_COUNT: u64 = 8_192;

/// A complete validated archive manifest, with content-addressed custody separate from its schema.
#[derive(Debug, Clone)]
pub struct ForecastCycleFinalManifest {
    due_key: String,
    generation: u64,
    value: JsonValue,
    canonical: Vec<u8>,
}

impl PartialEq for ForecastCycleFinalManifest {
    /// Compare immutable wire identity rather than generated JSON insertion order.
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl Eq for ForecastCycleFinalManifest {}

impl ForecastCycleFinalManifest {
    /// Inspect the exact retained document without installing new mutable fields.
    pub const fn document(&self) -> &JsonValue {
        &self.value
    }
}

impl<P: ArchiveWirePrimitives> ForecastCycleFinalManifestCodec for ArchiveWireCodec<P> {
    type Manifest = ForecastCycleFinalManifest;

    /// Decode only exact canonical bytes for the supported final schemas.
    fn decode_final_manifest(&self, bytes: &[u8]) -> Option<Self::Manifest> {
        decode_final_manifest(self, bytes)
    }

    /// Re-emit the original admitted document without changing its identity.
    fn encode_final_manifest(&self, manifest: &Self::Manifest) -> Option<Vec<u8>> {
        encode_final_manifest(manifest)
    }

    /// Select the exact predecessor due/generation correlation.
    fn final_manifest_identity<'a>(&self, manifest: &'a Self::Manifest) -> (&'a str, u64) {
        final_manifest_identity(manifest)
    }
}

impl<Codec: ForecastEvidenceReceiptCodec> ForecastCycleFinalManifestCodec
    for BoundForecastCyclePageCodec<'_, Codec>
{
    type Manifest = ForecastCycleFinalManifest;

    /// Preserve exact full-codec decoding through the narrow shared metadata contract.
    fn decode_final_manifest(&self, bytes: &[u8]) -> Option<Self::Manifest> {
        decode_final_manifest(self, bytes)
    }

    /// Preserve the original admitted canonical bytes without reserialization.
    fn encode_final_manifest(&self, manifest: &Self::Manifest) -> Option<Vec<u8>> {
        encode_final_manifest(manifest)
    }

    /// Select the exact predecessor due/generation correlation.
    fn final_manifest_identity<'a>(&self, manifest: &'a Self::Manifest) -> (&'a str, u64) {
        final_manifest_identity(manifest)
    }
}

/// Decode one exact final manifest through either admitted metadata owner.
fn decode_final_manifest(
    codec: &impl ForecastCycleMetadataCodec,
    bytes: &[u8],
) -> Option<ForecastCycleFinalManifest> {
    let value = codec.decode_cycle_metadata(bytes)?;
    // refuse variants that reserialize differently before trusting any closure
    if codec.encode_cycle_metadata(&value)? != bytes {
        return None;
    }
    let (due_key, generation) = validate_manifest(codec, &value)?;
    Some(ForecastCycleFinalManifest {
        due_key,
        generation,
        value,
        canonical: bytes.to_vec(),
    })
}

/// Re-emit the exact bytes admitted by final-manifest decoding.
fn encode_final_manifest(manifest: &ForecastCycleFinalManifest) -> Option<Vec<u8>> {
    Some(manifest.canonical.clone())
}

/// Borrow the authenticated due-key and generation from an admitted final manifest.
fn final_manifest_identity(manifest: &ForecastCycleFinalManifest) -> (&str, u64) {
    (&manifest.due_key, manifest.generation)
}

/// Validate the final manifest's exact page spine and closure linkage.
fn validate_manifest(
    codec: &impl ForecastCycleMetadataCodec,
    value: &JsonValue,
) -> Option<(String, u64)> {
    let fields = read_object(value)?;
    let version = read_text(read_field(fields, "contractVersion")?)?;
    let sealed = version == FINAL_V2;
    // retain only the two original closed schemas
    if (version != FINAL_V1 && !sealed) || fields.len() != 13 + usize::from(sealed) {
        return None;
    }
    let due_key = read_text(read_field(fields, "dueKey")?)?;
    let generation = read_generation(read_field(fields, "generation")?)?;
    let finalized_at = read_text(read_field(fields, "finalizedAt")?)?;
    let finalized_epoch = instant(codec, &finalized_at)?;
    let total = read_number(read_field(fields, "totalPayloadBytes")?)?;
    let page_count: usize = read_number(read_field(fields, "pageCount")?)?
        .try_into()
        .ok()?;
    read_number(read_field(fields, "acknowledgedProjectionCount")?)?;
    let page_root = read_digest(read_field(fields, "pageRootSha256")?)?;
    read_number(read_field(fields, "evidenceGapCount")?)?;
    for name in [
        "acknowledgementRootSha256",
        "acknowledgedProjectionRootSha256",
        "evidenceGapRootSha256",
    ] {
        read_digest(read_field(fields, name)?)?;
    }
    let pages_value = read_field(fields, "pages")?;
    let JsonValue::Array(pages) = pages_value else {
        return None;
    };
    // bind bounded page counts and ranges to their exact canonical ordered root
    if !valid_due_key(&due_key)
        || !(1..=FORECAST_CYCLE_MAXIMUM_PAGES).contains(&page_count)
        || page_count != pages.len()
        || !(1..=CYCLE_MAXIMUM_PAYLOAD_BYTES).contains(&total)
        || codec.sha256(&codec.encode_cycle_metadata(pages_value)?) != *page_root.as_bytes()
        || !validate_pages(pages, total)
    {
        return None;
    }

    // a sealed closure must match all original roots and chronology
    if sealed {
        let input_seal = read_field(fields, "inputSeal")?;
        let sealed_at = validate_embedded_input_seal(codec, input_seal)?;
        // a seal cannot have been produced after its final closure
        if instant(codec, &sealed_at)? > finalized_epoch {
            return None;
        }
        let seal = read_object(input_seal)?;
        for name in [
            "dueKey",
            "generation",
            "pageRootSha256",
            "acknowledgementRootSha256",
            "acknowledgedProjectionCount",
            "acknowledgedProjectionRootSha256",
            "evidenceGapRootSha256",
        ] {
            if read_field(seal, name)? != read_field(fields, name)? {
                return None;
            }
        }
    }
    Some((due_key, generation))
}

/// Validate the exact embedded producer seal without requiring evidence gzip operations.
fn validate_embedded_input_seal(
    codec: &impl ForecastCycleMetadataCodec,
    value: &JsonValue,
) -> Option<String> {
    let fields = read_object(value)?;
    // preserve the closed ten-field seal schema and its fixed contract version
    if fields.len() != 10
        || read_text(read_field(fields, "contractVersion")?)?
            != FORECAST_CYCLE_INPUT_SEAL_CONTRACT_VERSION
    {
        return None;
    }
    let due_key = read_text(read_field(fields, "dueKey")?)?;
    read_generation(read_field(fields, "generation")?)?;
    read_digest(read_field(fields, "pageRootSha256")?)?;
    read_digest(read_field(fields, "acknowledgementRootSha256")?)?;
    read_digest(read_field(fields, "acknowledgedProjectionRootSha256")?)?;
    read_digest(read_field(fields, "evidenceGapRootSha256")?)?;
    let acknowledged_projection_count =
        read_number(read_field(fields, "acknowledgedProjectionCount")?)?;
    let sealed_at = read_text(read_field(fields, "sealedAt")?)?;
    let required = read_object(read_field(fields, "requiredInputs")?)?;
    // retain every explicit producer class before applying population invariants
    if required.len() != 7
        || !valid_due_key(&due_key)
        || instant(codec, &sealed_at).is_none()
        || !(1..=FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS as u64)
            .contains(&acknowledged_projection_count)
        || codec.encode_cycle_metadata(value)?.len() > INPUT_SEAL_MAXIMUM_BYTES
    {
        return None;
    }

    let empty_bytes = codec.encode_cycle_metadata(&JsonValue::Array(Vec::new()))?;
    let empty_root = ForecastCycleSha256::admitted(codec.sha256(&empty_bytes));
    let mut serving_evidence_count = None;
    let mut shadow_body_count = None;
    let mut shadow_metadata_count = None;
    // validate each mandatory population against the same bounded empty-root rule
    for name in [
        "actual_best_match",
        "native_source",
        "rain_gate_input",
        "serving_evidence",
        "shadow_body",
        "shadow_metadata",
        "target_revision",
    ] {
        let (count, root) = read_required_input_root(read_field(required, name)?)?;
        // reject oversized populations and false empty-root declarations
        if count > REQUIRED_INPUT_MAXIMUM_COUNT || (count == 0 && root != empty_root) {
            return None;
        }
        // retain only the three counts needed by the cross-population invariants
        match name {
            "serving_evidence" => serving_evidence_count = Some(count),
            "shadow_body" => shadow_body_count = Some(count),
            "shadow_metadata" => shadow_metadata_count = Some(count),
            _ => {}
        }
    }
    // preserve the nonempty serving set and one-to-one shadow body/metadata closure
    if serving_evidence_count == Some(0) || shadow_body_count != shadow_metadata_count {
        return None;
    }
    Some(sealed_at)
}

/// Read one exact two-field required-input population record.
fn read_required_input_root(value: &JsonValue) -> Option<(u64, ForecastCycleSha256)> {
    let fields = read_object(value)?;
    // reject aliases or additional members before admitting the count and digest
    if fields.len() != 2 {
        return None;
    }
    Some((
        read_number(read_field(fields, "count")?)?,
        read_digest(read_field(fields, "rootSha256")?)?,
    ))
}

/// Require every final page to be an exact contiguous range after its predecessor.
fn validate_pages(pages: &[JsonValue], total: u64) -> bool {
    let mut predecessor = None;
    let mut expected_offset = 0;
    // inspect ordered immutable page extents without loading any payload
    for (index, page) in pages.iter().enumerate() {
        let Some((previous, digest, offset, length, actual_index)) = read_final_page(page) else {
            return false;
        };
        // refuse a gap, overlap or changed predecessor link
        if previous != predecessor || offset != expected_offset || actual_index != index as u64 {
            return false;
        }
        predecessor = Some(digest);
        expected_offset += length;
    }
    expected_offset == total
}

type FinalPageLink = (
    Option<ForecastCycleSha256>,
    ForecastCycleSha256,
    u64,
    u64,
    u64,
);

/// Recover the exact immutable page metadata retained in the final manifest.
fn read_final_page(page: &JsonValue) -> Option<FinalPageLink> {
    let fields = read_object(page)?;
    // insist on the source page schema before accepting its extent
    if fields.len() != 6 {
        return None;
    }
    let page_sha256 = read_digest(read_field(fields, "pageSha256")?)?;
    let previous = match read_field(fields, "predecessorPageSha256")? {
        JsonValue::Null => None,
        value => Some(read_digest(value)?),
    };
    read_digest(read_field(fields, "payloadSha256")?)?;
    let offset = read_number(read_field(fields, "payloadOffset")?)?;
    let length = read_number(read_field(fields, "payloadLength")?)?;
    let index = read_number(read_field(fields, "pageIndex")?)?;
    (1..=FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES as u64)
        .contains(&length)
        .then_some((previous, page_sha256, offset, length, index))
}

use crate::forecast_cycle_pages::{ForecastCyclePage, ForecastCyclePageState, valid_state};

use super::forecast_cycle_final_store::RetainedForecastCycleManifest;
use super::forecast_cycle_gap::DurableForecastCycleGap;
use super::forecast_cycle_input_seal::{
    FORECAST_CYCLE_INPUT_SEAL_CONTRACT_VERSION, ForecastCycleInputSeal, ForecastCycleRequiredInputs,
};
use super::forecast_cycle_input_seal_binding::seal_value;
use super::forecast_cycle_page_binding::{ForecastCycleHashPair, digest, object, text};

/// Finalization refused because the retained frontier or declared producer roots are incomplete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCycleFinalizationError {
    Incomplete,
    InvalidFrontier,
    Codec,
}

struct FinalBindings {
    pages: JsonValue,
    page_root: ForecastCycleSha256,
    acknowledgement_root: ForecastCycleSha256,
    projection_root: ForecastCycleSha256,
    projection_count: usize,
    gap_root: ForecastCycleSha256,
    gap_count: usize,
    payload_bytes: u64,
}

/// Prepare the exact combined declaration from acknowledged pages and admitted producer roots.
pub fn prepare_forecast_cycle_input_seal<
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
>(
    state: &ForecastCyclePageState<DurableForecastCycleGap>,
    required_inputs: ForecastCycleRequiredInputs,
    sealed_at: &str,
    codec: &BoundForecastCyclePageCodec<'_, Codec>,
) -> Result<ForecastCycleInputSeal, ForecastCycleFinalizationError> {
    use ForecastCycleFinalizationError as Error;
    let bindings = final_bindings(state, codec)?;
    let value = ForecastCycleInputSeal {
        acknowledged_projection_count: bindings.projection_count,
        acknowledged_projection_root_sha256: bindings.projection_root,
        acknowledgement_root_sha256: bindings.acknowledgement_root,
        contract_version: FORECAST_CYCLE_INPUT_SEAL_CONTRACT_VERSION,
        due_key: state.due_key.clone(),
        evidence_gap_root_sha256: bindings.gap_root,
        generation: state.generation,
        page_root_sha256: bindings.page_root,
        required_inputs,
        sealed_at: sealed_at.to_owned(),
    };
    codec
        .admit_input_seal_value(&seal_value(&value).ok_or(Error::Codec)?)
        .ok_or(Error::InvalidFrontier)
}

/// Prepare the exact closure for native persistence; sealed runtime callers supply the durable seal.
pub fn prepare_forecast_cycle_final_manifest<
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
>(
    state: &ForecastCyclePageState<DurableForecastCycleGap>,
    finalized_at: &str,
    seal: Option<&ForecastCycleInputSeal>,
    codec: &BoundForecastCyclePageCodec<'_, Codec>,
) -> Result<RetainedForecastCycleManifest<ForecastCycleFinalManifest>, ForecastCycleFinalizationError>
{
    use ForecastCycleFinalizationError as Error;
    let bindings = final_bindings(state, codec)?;
    let JsonValue::Object(mut fields) = object([
        (
            "contractVersion",
            text(if seal.is_some() { FINAL_V2 } else { FINAL_V1 }),
        ),
        ("dueKey", text(&state.due_key)),
        ("generation", text(&state.generation.to_string())),
        ("finalizedAt", text(finalized_at)),
        (
            "pageCount",
            number(state.pages.len().try_into().map_err(|_| Error::Codec)?)?,
        ),
        ("totalPayloadBytes", number(bindings.payload_bytes)?),
        ("pages", bindings.pages),
        ("pageRootSha256", digest(bindings.page_root)),
        (
            "acknowledgementRootSha256",
            digest(bindings.acknowledgement_root),
        ),
        (
            "acknowledgedProjectionCount",
            number(
                bindings
                    .projection_count
                    .try_into()
                    .map_err(|_| Error::Codec)?,
            )?,
        ),
        (
            "acknowledgedProjectionRootSha256",
            digest(bindings.projection_root),
        ),
        (
            "evidenceGapCount",
            number(bindings.gap_count.try_into().map_err(|_| Error::Codec)?)?,
        ),
        ("evidenceGapRootSha256", digest(bindings.gap_root)),
    ]) else {
        return Err(Error::Codec);
    };
    // bind the durable producer closure to all freshly derived page and acknowledgement roots
    if let Some(seal) = seal {
        fields.push((
            "inputSeal".encode_utf16().collect(),
            seal_value(seal).ok_or(Error::Codec)?,
        ));
    }
    let value = JsonValue::Object(fields);
    let (due_key, generation) = validate_manifest(codec, &value).ok_or(Error::InvalidFrontier)?;
    let bytes = codec.frame(&value).ok_or(Error::Codec)?;
    let manifest_sha256 = ForecastCycleSha256::admitted(codec.sha256(&bytes));
    Ok(RetainedForecastCycleManifest {
        manifest: ForecastCycleFinalManifest {
            due_key,
            generation,
            value,
            canonical: bytes,
        },
        manifest_sha256,
    })
}

/// Derive every ordered root from the fully acknowledged retained frontier.
fn final_bindings<Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair>(
    state: &ForecastCyclePageState<DurableForecastCycleGap>,
    codec: &BoundForecastCyclePageCodec<'_, Codec>,
) -> Result<FinalBindings, ForecastCycleFinalizationError> {
    use ForecastCycleFinalizationError as Error;

    // refuse incomplete custody before computing any final root
    if state.pages.is_empty()
        || !state.slots.is_empty()
        || state.pages.len() != state.acknowledgements.len()
    {
        return Err(Error::Incomplete);
    }
    // ensure every declared permanent gap still belongs to this immutable cycle
    if !valid_state(state, codec)
        || state.gaps().iter().any(|gap| {
            gap.record().due_key != state.due_key || gap.record().generation != state.generation
        })
    {
        return Err(Error::InvalidFrontier);
    }
    let pages = JsonValue::Array(
        state
            .pages
            .iter()
            .map(final_page_value)
            .collect::<Result<Vec<_>, _>>()?,
    );
    let page_root = hash_value(codec, &pages)?;
    let acknowledgement_root = hash_value(
        codec,
        &JsonValue::Array(
            state
                .acknowledgements
                .iter()
                .map(|ack| digest(ack.acknowledgement_sha256))
                .collect(),
        ),
    )?;
    let mut projections: Vec<_> = state
        .pages
        .iter()
        .flat_map(|page| page.projection_identities.iter().copied())
        .collect();
    projections.sort_unstable();
    let projection_count = projections.len();
    let projection_root = hash_value(
        codec,
        &JsonValue::Array(projections.into_iter().map(digest).collect()),
    )?;
    let mut gaps: Vec<_> = state
        .gaps()
        .iter()
        .map(DurableForecastCycleGap::digest)
        .collect();
    gaps.sort_unstable();
    let gap_count = gaps.len();
    let gap_root = hash_value(
        codec,
        &JsonValue::Array(gaps.into_iter().map(digest).collect()),
    )?;
    let payload_bytes = state
        .pages
        .iter()
        .try_fold(0_u64, |total, page| {
            total.checked_add(page.payload_length.try_into().ok()?)
        })
        .ok_or(Error::Codec)?;
    Ok(FinalBindings {
        pages,
        page_root,
        acknowledgement_root,
        projection_root,
        projection_count,
        gap_root,
        gap_count,
        payload_bytes,
    })
}

/// Project one exact final page range without the released value-bearing bytes.
fn final_page_value(page: &ForecastCyclePage) -> Result<JsonValue, ForecastCycleFinalizationError> {
    use ForecastCycleFinalizationError as Error;
    Ok(object([
        (
            "pageIndex",
            number(page.page_index.try_into().map_err(|_| Error::Codec)?)?,
        ),
        ("pageSha256", digest(page.page_sha256)),
        (
            "predecessorPageSha256",
            page.predecessor_page_sha256.map_or(JsonValue::Null, digest),
        ),
        ("payloadSha256", digest(page.payload_sha256)),
        ("payloadOffset", number(page.payload_offset)?),
        (
            "payloadLength",
            number(page.payload_length.try_into().map_err(|_| Error::Codec)?)?,
        ),
    ]))
}

/// Keep exact bounded source integers without rounding.
fn number(value: u64) -> Result<JsonValue, ForecastCycleFinalizationError> {
    let value: u32 = value
        .try_into()
        .map_err(|_| ForecastCycleFinalizationError::Codec)?;
    Ok(JsonValue::Number(f64::from(value)))
}

/// Use the admitted canonical framing for every independently verifiable digest.
fn hash_value<Codec: ForecastEvidenceReceiptCodec>(
    codec: &BoundForecastCyclePageCodec<'_, Codec>,
    value: &JsonValue,
) -> Result<ForecastCycleSha256, ForecastCycleFinalizationError> {
    let bytes = codec
        .frame(value)
        .ok_or(ForecastCycleFinalizationError::Codec)?;
    Ok(ForecastCycleSha256::admitted(codec.sha256(&bytes)))
}

use super::forecast_allocation::ForecastAllocationStatfsPort;
use super::forecast_cycle_final_store::{
    ForecastCycleFinalManifestError, NativeForecastCycleFinalManifests,
};
use super::forecast_cycle_input_seal::{
    ForecastCycleInputSealError, NativeForecastCycleInputSeals,
};

/// Native cycle frontier after the exact final manifest is safely retained.
pub type FinalizedForecastCycleState = ForecastCyclePageState<
    DurableForecastCycleGap,
    RetainedForecastCycleManifest<ForecastCycleFinalManifest>,
>;

/// Errors beneath the independently scheduled producer closure lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeForecastCycleFinalizationError {
    RootMismatch,
    MissingInputSeal,
    Seal(ForecastCycleInputSealError),
    Prepare(ForecastCycleFinalizationError),
    Manifest(ForecastCycleFinalManifestError),
}

/// A newly closed local frontier or the original durable closure retained across restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeForecastCycleFinalizationOutcome {
    Finalized(Box<FinalizedForecastCycleState>),
    AlreadyFinalized(RetainedForecastCycleManifest<ForecastCycleFinalManifest>),
}

/// Close the exact fully acknowledged native cycle only after its separate producer seal is durable.
pub fn finalize_native_forecast_cycle<Codec, SealStatfs, ManifestStatfs>(
    state: ForecastCyclePageState<DurableForecastCycleGap>,
    seals: &NativeForecastCycleInputSeals<BoundForecastCyclePageCodec<'_, Codec>, SealStatfs>,
    manifests: &mut NativeForecastCycleFinalManifests<
        BoundForecastCyclePageCodec<'_, Codec>,
        ManifestStatfs,
    >,
    codec: &BoundForecastCyclePageCodec<'_, Codec>,
) -> Result<NativeForecastCycleFinalizationOutcome, NativeForecastCycleFinalizationError>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    ManifestStatfs: ForecastAllocationStatfsPort,
{
    use NativeForecastCycleFinalizationError as Error;

    // never close one evidence root using another root's producer seal
    if seals.root() != manifests.root() {
        return Err(Error::RootMismatch);
    }
    let seal = seals
        .read_input_seal(&state.due_key, state.generation)
        .map_err(Error::Seal)?
        .ok_or(Error::MissingInputSeal)?;
    let previous = manifests
        .find_final_manifest(&state.due_key, state.generation)
        .map_err(Error::Manifest)?;
    // preserve the first final before recomputing roots from any retained frontier
    if let Some(previous) = previous {
        return Ok(NativeForecastCycleFinalizationOutcome::AlreadyFinalized(
            previous,
        ));
    }
    // source recovery records the seal clock rather than the current process clock
    let prepared =
        prepare_forecast_cycle_final_manifest(&state, &seal.sealed_at, Some(&seal), codec)
            .map_err(Error::Prepare)?;
    manifests
        .persist_final_manifest(&prepared.manifest, prepared.manifest_sha256)
        .map_err(Error::Manifest)?;
    Ok(NativeForecastCycleFinalizationOutcome::Finalized(Box::new(
        state.with_final_manifest(prepared),
    )))
}
