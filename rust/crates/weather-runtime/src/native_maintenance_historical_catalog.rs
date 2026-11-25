//! Verified native historical cold-catalog restoration and semantic current-pointer assembly.

use std::collections::{HashMap, HashSet};
use std::fmt::{self, Display, Formatter};
use std::io::Read;

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptProjectionKindEnum, JsonValue,
};
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, PrivateRevisionReceiptCodec,
    PrivateShadowReceiptCodec, RevisionArchiveCanonicalCodec,
};

use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_maintenance_cas_pack::NativeMaintenanceCasMemberValidator;
use crate::native_maintenance_cold_remote_documents::verify_native_revision_cold_start_document;
use crate::native_maintenance_controller_archive_validators::NativeMaintenanceControllerArchiveValidators;
use crate::native_maintenance_graph_restore::{
    NativeMaintenanceGraphRestoreSink, NativeMaintenanceGraphRestoreTarget,
    restore_native_maintenance_full_graph,
};
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceFullGraphObjects, NativeRevisionColdCursor,
    NativeWirePrimitives, VerifiedNativeRevisionColdStart, decode_native_api_revision_document,
};

const INDEX_VERSION: &str = "adjustment-revision-historical-archive-index/v1";
const CATALOG_VERSION: &str = "adjustment-revision-cold-catalog/v1";
const CHECKPOINT_KIND: &str = "adjustment-revision-cold-page-checkpoint/v1";
const START_KIND: &str = "adjustment-revision-cold-transfer-start/v1";
const RECEIPT_KIND: &str = "adjustment-revision-commit-receipt/v1";
const MAXIMUM_SELECTED_BYTES: usize = 512 * 1_024 * 1_024;
const MAXIMUM_PAGE_RESTORE_BYTES: usize = 8 * 1_024 * 1_024;
const MAXIMUM_TRANSFER_BYTES: u64 = 4_832 * 1_024;
const MAXIMUM_PAGES: usize = 16_744;
const MAXIMUM_SELECTED_RECEIPTS: usize = 4_096;
const RANGE_CLOCK_KEYS: &[&str] = &[
    "completedAt",
    "issuedAt",
    "logicalReceivedAt",
    "observedAt",
    "productRunAt",
    "receivedAt",
    "referenceAt",
    "runInitializedAt",
    "validAt",
];

/// One immutable journal mapping from a cold page to its verified graph head.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceHistoricalCatalogMapping {
    pub graph_manifest_sha256: String,
    pub page_sha256: String,
    pub start_sha256: String,
}

/// The three source fitter families admitted by historical selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceHistoricalFamily {
    Rain,
    Temperature,
    Wind,
}

impl NativeMaintenanceHistoricalFamily {
    /// Return the exact predecessor family spelling.
    pub const fn as_str(self) -> &'static str {
        // preserve the closed source family vocabulary
        match self {
            Self::Rain => "rain",
            Self::Temperature => "temperature",
            Self::Wind => "wind",
        }
    }
}

/// Bounded history selection plus exact current-pointer receipt identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceHistoricalSelection {
    pub family: Option<NativeMaintenanceHistoricalFamily>,
    pub from_at: Option<String>,
    pub receipt_sha256s: Vec<String>,
    pub to_at: Option<String>,
}

/// One already restored member from a fully verified immutable graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceHistoricalRetainedMember {
    pub identity_sha256: String,
    pub kind: String,
    pub payload: Vec<u8>,
}

/// One verified retained graph input used by the pure catalog assembler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceHistoricalRetainedPage {
    pub mapping: NativeMaintenanceHistoricalCatalogMapping,
    pub members: Vec<NativeMaintenanceHistoricalRetainedMember>,
}

/// Full pure assembly input after durable cursor and graph restoration.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceHistoricalCatalogInput {
    pub cursor: NativeRevisionColdCursor,
    pub pages: Vec<NativeMaintenanceHistoricalRetainedPage>,
    pub selection: NativeMaintenanceHistoricalSelection,
    pub start: VerifiedNativeRevisionColdStart,
}

/// Exact current semantic member selected from the complete archived prefix.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceHistoricalCurrentMember {
    pub logical_received_at: String,
    pub payload_bytes: Vec<u8>,
    pub payload_kind: String,
    pub receipt: AdjustmentRevisionCommitReceipt,
    pub relation: String,
}

/// One selected immutable payload occurrence and its ordered receipt population.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceHistoricalOccurrence {
    pub graph_manifest_sha256: String,
    pub page_sha256: String,
    pub payload_bytes: Vec<u8>,
    pub payload_identity_sha256: String,
    pub payload_kind: String,
    pub publication_disposition: String,
    pub receipts: Vec<AdjustmentRevisionCommitReceipt>,
}

/// One compact restored-page summary in global frontier order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceHistoricalPageSummary {
    pub after_archive_commit_ordinal: String,
    pub graph_manifest_sha256: String,
    pub next_archive_commit_ordinal: String,
    pub page_sha256: String,
    pub receipt_count: usize,
    pub start_sha256: String,
}

/// Exact four non-shadow serving-producer counts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceColdCatalogKindCounts {
    pub actual_best_match: usize,
    pub native_source: usize,
    pub rain_gate_input: usize,
    pub target_revision: usize,
}

/// Compact complete-frontier cold catalog projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceColdCatalog {
    pub archive_commit_ordinal: String,
    pub catalog_root_sha256: String,
    pub frontier_count: usize,
    pub frontier_root_sha256: String,
    pub kind_counts: NativeMaintenanceColdCatalogKindCounts,
    pub receipt_root_sha256: String,
    pub serving_snapshot_sha256: String,
}

/// Selected history plus its complete-frontier catalog identity.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceHistoricalArchiveIndex {
    pub catalog: NativeMaintenanceColdCatalog,
    pub contract_version: &'static str,
    pub history_root_sha256: String,
    pub occurrences: Vec<NativeMaintenanceHistoricalOccurrence>,
    pub pages: Vec<NativeMaintenanceHistoricalPageSummary>,
    pub receipt_count: usize,
}

/// Fully restored semantic catalog used by maintenance fitters.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceHistoricalCatalogReady {
    pub catalog: NativeMaintenanceColdCatalog,
    pub history: NativeMaintenanceHistoricalArchiveIndex,
    pub input_manifest_sha256: String,
    pub members: Vec<NativeMaintenanceHistoricalCurrentMember>,
    pub serving_snapshot: JsonValue,
}

/// Source-exact blocked or ready semantic catalog state.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeMaintenanceHistoricalCatalogOutcome {
    Blocked,
    Ready(Box<NativeMaintenanceHistoricalCatalogReady>),
}

impl NativeMaintenanceHistoricalCatalogOutcome {
    /// Return the exact source state spelling.
    pub const fn state(&self) -> &'static str {
        // retain the predecessor result vocabulary
        match self {
            Self::Blocked => "blocked",
            Self::Ready(_) => "ready",
        }
    }

    /// Return the source refusal reason only for the blocked state.
    pub const fn reason(&self) -> Option<&'static str> {
        // expose no invented reason for a ready result
        match self {
            Self::Blocked => Some("semantic_input_blocked"),
            Self::Ready(_) => None,
        }
    }
}

/// Closed historical restoration or semantic binding failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceHistoricalCatalogError {
    Mapping,
    Restore,
    Checkpoint,
    Start,
    Frontier,
    Selection,
    Receipt,
    Payload,
    CurrentPointer,
    Population,
    Size,
    Codec,
}

impl Display for NativeMaintenanceHistoricalCatalogError {
    /// Render one stable non-path-bearing failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Mapping => "historical archive mapping is invalid",
            Self::Restore => "historical archive member restore failed",
            Self::Checkpoint => "historical archive checkpoint is invalid",
            Self::Start => "historical archive transfer start is invalid",
            Self::Frontier => "historical archive frontier differs",
            Self::Selection => "historical archive selection is invalid",
            Self::Receipt => "historical archive receipt mapping differs",
            Self::Payload => "historical archive payload is invalid",
            Self::CurrentPointer => "historical archive current pointer differs",
            Self::Population => "historical archive producer population is incomplete",
            Self::Size => "historical archive byte ceiling exceeded",
            Self::Codec => "historical archive encoding failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceHistoricalCatalogError {}

/// Restore the exact requested graph members through the verified native object store.
pub fn restore_native_maintenance_historical_catalog(
    store: &mut impl NativeMaintenanceFullGraphObjects,
    cursor: NativeRevisionColdCursor,
    start: VerifiedNativeRevisionColdStart,
    mappings: Vec<NativeMaintenanceHistoricalCatalogMapping>,
    selection: NativeMaintenanceHistoricalSelection,
) -> Result<NativeMaintenanceHistoricalCatalogOutcome, NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError as Error;

    validate_selection(&selection)?;
    // preserve the source early block before any archive read
    if cursor.archive_commit_ordinal != start.watermark_archive_commit_ordinal
        || cursor.frontier_sha256 != start.watermark_frontier_sha256
        || mappings.is_empty()
    {
        return Ok(NativeMaintenanceHistoricalCatalogOutcome::Blocked);
    }
    // retain the fixed archive-page census before graph verification work
    if mappings.len() > MAXIMUM_PAGES {
        return Err(Error::Mapping);
    }
    let requested: HashSet<&str> = selection
        .receipt_sha256s
        .iter()
        .map(String::as_str)
        .collect();
    let mut pages = Vec::with_capacity(mappings.len());
    // restore compact metadata first, then only payloads required by the selection
    for mapping in mappings {
        validate_mapping(&mapping)?;
        let checkpoint = restore_one_member(
            store,
            &mapping.graph_manifest_sha256,
            &mapping.page_sha256,
            "checkpoint.json",
            MAXIMUM_TRANSFER_BYTES,
        )?;
        let parsed = parse_checkpoint(&checkpoint)?;
        // bind the mapping to both immutable page and transfer identities
        if parsed.page_sha256 != mapping.page_sha256 || parsed.start_sha256 != mapping.start_sha256
        {
            return Err(Error::Checkpoint);
        }
        let mut members = vec![NativeMaintenanceHistoricalRetainedMember {
            identity_sha256: mapping.page_sha256.clone(),
            kind: CHECKPOINT_KIND.to_owned(),
            payload: checkpoint,
        }];
        // authenticate a first-page transfer start from the same verified graph
        if parsed.start_member_sha256.is_some() {
            members.push(NativeMaintenanceHistoricalRetainedMember {
                identity_sha256: mapping.start_sha256.clone(),
                kind: START_KIND.to_owned(),
                payload: restore_one_member(
                    store,
                    &mapping.graph_manifest_sha256,
                    &mapping.start_sha256,
                    "start.json",
                    MAXIMUM_TRANSFER_BYTES,
                )?,
            });
        }
        let mut requests = Vec::new();
        // plan every receipt plus only a requested or family-selected payload
        for (entry_index, entry) in parsed.entries.iter().enumerate() {
            // preserve each grouped receipt in its source ordinal order
            for (receipt_index, binding) in entry.receipts.iter().enumerate() {
                requests.push(RestoreRequest {
                    file_name: format!("receipt-{entry_index}-{receipt_index}.json"),
                    identity_sha256: binding.receipt_sha256.clone(),
                    kind: RECEIPT_KIND.to_owned(),
                });
            }
            let retain_payload = selection.family.is_some()
                || entry
                    .receipts
                    .iter()
                    .any(|binding| requested.contains(binding.receipt_sha256.as_str()));
            // avoid restoring unselected historical values
            if retain_payload {
                requests.push(RestoreRequest {
                    file_name: format!("payload-{entry_index}.json"),
                    identity_sha256: entry.payload_identity_sha256.clone(),
                    kind: entry.payload_kind.clone(),
                });
            }
        }
        let mut restored = restore_members(
            store,
            &mapping.graph_manifest_sha256,
            &requests,
            MAXIMUM_PAGE_RESTORE_BYTES as u64,
        )?;
        // retain each verified result under its semantic identity and kind
        for request in requests {
            members.push(NativeMaintenanceHistoricalRetainedMember {
                identity_sha256: request.identity_sha256,
                kind: request.kind,
                payload: restored.remove(&request.file_name).ok_or(Error::Restore)?,
            });
        }
        pages.push(NativeMaintenanceHistoricalRetainedPage { mapping, members });
    }
    assemble_native_maintenance_historical_catalog(NativeMaintenanceHistoricalCatalogInput {
        cursor,
        pages,
        selection,
        start,
    })
}

/// Assemble the complete historical frontier from already verified retained graph members.
pub fn assemble_native_maintenance_historical_catalog(
    input: NativeMaintenanceHistoricalCatalogInput,
) -> Result<NativeMaintenanceHistoricalCatalogOutcome, NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError as Error;

    validate_selection(&input.selection)?;
    validate_start(&input.start)?;
    // preserve the source early block without inspecting retained history
    if input.cursor.archive_commit_ordinal != input.start.watermark_archive_commit_ordinal
        || input.cursor.frontier_sha256 != input.start.watermark_frontier_sha256
        || input.pages.is_empty()
    {
        return Ok(NativeMaintenanceHistoricalCatalogOutcome::Blocked);
    }
    // admit one bounded unique journal mapping population
    if input.pages.len() > MAXIMUM_PAGES {
        return Err(Error::Mapping);
    }
    let mut page_identities = HashSet::with_capacity(input.pages.len());
    let mut starts: HashMap<String, VerifiedNativeRevisionColdStart> = HashMap::new();
    let mut pages = Vec::with_capacity(input.pages.len());
    // parse each retained checkpoint and its optional first-page start
    for retained in input.pages {
        validate_mapping(&retained.mapping)?;
        // reject repeated immutable page identities before member lookup
        if !page_identities.insert(retained.mapping.page_sha256.clone()) {
            return Err(Error::Mapping);
        }
        let checkpoint_bytes = unique_member(
            &retained.members,
            &retained.mapping.page_sha256,
            CHECKPOINT_KIND,
        )?;
        let checkpoint = parse_checkpoint(checkpoint_bytes)?;
        // bind the retained mapping to the checkpoint's exact source coordinates
        if checkpoint.page_sha256 != retained.mapping.page_sha256
            || checkpoint.start_sha256 != retained.mapping.start_sha256
        {
            return Err(Error::Checkpoint);
        }
        let transfer_start = if let Some(expected_member) = &checkpoint.start_member_sha256 {
            let bytes = unique_member(&retained.members, &checkpoint.start_sha256, START_KIND)?;
            let verified = verify_start(bytes)?;
            let actual_member = digest(bytes);
            // bind both the graph member bytes and semantic start identity
            if actual_member != *expected_member || verified.start_sha256 != checkpoint.start_sha256
            {
                return Err(Error::Start);
            }
            // admit one archived start member only once across the history
            if starts
                .insert(verified.start_sha256.clone(), verified.clone())
                .is_some()
            {
                return Err(Error::Start);
            }
            Some(verified)
        } else {
            None
        };
        pages.push(ParsedPage {
            checkpoint,
            mapping: retained.mapping,
            members: retained.members,
            transfer_start,
        });
    }
    pages.sort_by_key(|page| page.checkpoint.after_archive_commit_ordinal);
    // resolve continuation pages only after every first-page start is known
    for page in &mut pages {
        // fill only a continuation's missing transfer start
        if page.transfer_start.is_none() {
            page.transfer_start = starts.get(&page.checkpoint.start_sha256).cloned();
        }
        // refuse a page whose archived transfer epoch was never authenticated
        if page.transfer_start.is_none() {
            return Err(Error::Start);
        }
    }
    validate_page_index(&pages, &input.start)?;
    stream_historical_prefix(input.start, input.selection, pages)
}

#[derive(Clone)]
struct ParsedReceiptBinding {
    receipt_member_sha256: String,
    receipt_sha256: String,
}

#[derive(Clone)]
struct ParsedCheckpointEntry {
    payload_identity_sha256: String,
    payload_kind: String,
    publication_disposition: String,
    receipts: Vec<ParsedReceiptBinding>,
}

#[derive(Clone)]
struct ParsedCheckpoint {
    after_archive_commit_ordinal: u64,
    after_archive_commit_ordinal_text: String,
    after_frontier_sha256: String,
    entries: Vec<ParsedCheckpointEntry>,
    next_archive_commit_ordinal: u64,
    next_archive_commit_ordinal_text: String,
    next_frontier_sha256: String,
    page_sha256: String,
    previous_page_sha256: String,
    start_member_sha256: Option<String>,
    start_sha256: String,
    watermark_archive_commit_ordinal_text: String,
    watermark_frontier_sha256: String,
}

struct ParsedPage {
    checkpoint: ParsedCheckpoint,
    mapping: NativeMaintenanceHistoricalCatalogMapping,
    members: Vec<NativeMaintenanceHistoricalRetainedMember>,
    transfer_start: Option<VerifiedNativeRevisionColdStart>,
}

#[derive(Clone)]
struct SnapshotPointer {
    logical_received_at: String,
    receipt: AdjustmentRevisionCommitReceipt,
    receipt_value: JsonValue,
    relation: String,
}

/// Parse one already cold-validator-admitted checkpoint into the typed assembly subset.
fn parse_checkpoint(
    bytes: &[u8],
) -> Result<ParsedCheckpoint, NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError as Error;

    let mut validators = NativeMaintenanceControllerArchiveValidators::new();
    // retain the complete closed checkpoint validator as the admission boundary
    if !validators.validate(CHECKPOINT_KIND, bytes) {
        return Err(Error::Checkpoint);
    }
    let value = NativeWirePrimitives
        .parse_canonical_object(bytes)
        .ok_or(Error::Checkpoint)?;
    let JsonValue::Array(entries) = field(&value, "entries").ok_or(Error::Checkpoint)? else {
        return Err(Error::Checkpoint);
    };
    let mut parsed_entries = Vec::with_capacity(entries.len());
    // project the compact member bindings without weakening the complete validator
    for entry in entries {
        let payload_identity_sha256 = required_hash(entry, "payloadIdentitySha256")?;
        let payload_kind = required_text(entry, "payloadKind")?;
        let publication_disposition = required_text(entry, "publicationDisposition")?;
        let receipts = if let Some(JsonValue::Array(identities)) = field(entry, "receiptSha256s") {
            let JsonValue::Array(members) =
                field(entry, "receiptMemberSha256s").ok_or(Error::Checkpoint)?
            else {
                return Err(Error::Checkpoint);
            };
            let mut bindings = Vec::with_capacity(identities.len());
            // retain the source's parallel grouped receipt order
            for (identity, member) in identities.iter().zip(members) {
                bindings.push(ParsedReceiptBinding {
                    receipt_member_sha256: json_hash(member)?,
                    receipt_sha256: json_hash(identity)?,
                });
            }
            bindings
        } else {
            vec![ParsedReceiptBinding {
                receipt_member_sha256: required_hash(entry, "receiptMemberSha256")?,
                receipt_sha256: required_hash(entry, "receiptSha256")?,
            }]
        };
        parsed_entries.push(ParsedCheckpointEntry {
            payload_identity_sha256,
            payload_kind,
            publication_disposition,
            receipts,
        });
    }
    Ok(ParsedCheckpoint {
        after_archive_commit_ordinal: required_ordinal(&value, "afterArchiveCommitOrdinal")?,
        after_archive_commit_ordinal_text: required_text(&value, "afterArchiveCommitOrdinal")?,
        after_frontier_sha256: required_hash(&value, "afterFrontierSha256")?,
        entries: parsed_entries,
        next_archive_commit_ordinal: required_ordinal(&value, "nextArchiveCommitOrdinal")?,
        next_archive_commit_ordinal_text: required_text(&value, "nextArchiveCommitOrdinal")?,
        next_frontier_sha256: required_hash(&value, "nextFrontierSha256")?,
        page_sha256: required_hash(&value, "pageSha256")?,
        previous_page_sha256: required_hash(&value, "previousPageSha256")?,
        start_member_sha256: optional_hash(&value, "startMemberSha256")?,
        start_sha256: required_hash(&value, "startSha256")?,
        watermark_archive_commit_ordinal_text: required_text(
            &value,
            "watermarkArchiveCommitOrdinal",
        )?,
        watermark_frontier_sha256: required_hash(&value, "watermarkFrontierSha256")?,
    })
}

/// Validate sorted page continuity and every no-rewind transfer watermark.
fn validate_page_index(
    pages: &[ParsedPage],
    current_start: &VerifiedNativeRevisionColdStart,
) -> Result<(), NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError as Error;

    let genesis = revision_frontier_genesis();
    let mut prior_ordinal = 0u64;
    let mut prior_frontier = genesis.clone();
    let mut prior_page: Option<&ParsedPage> = None;
    let mut prior_start_watermark = 0u64;
    let mut prior_start_frontier = genesis;
    // connect each independently authenticated page through one global frontier
    for page in pages {
        let start = page.transfer_start.as_ref().ok_or(Error::Start)?;
        let start_ordinal =
            canonical_ordinal(&start.watermark_archive_commit_ordinal).ok_or(Error::Start)?;
        // require the exact global predecessor and transfer watermark on every page
        if page.checkpoint.after_archive_commit_ordinal != prior_ordinal
            || page.checkpoint.after_frontier_sha256 != prior_frontier
            || page.checkpoint.entries.is_empty()
            || page.checkpoint.watermark_archive_commit_ordinal_text
                != start.watermark_archive_commit_ordinal
            || page.checkpoint.watermark_frontier_sha256 != start.watermark_frontier_sha256
        {
            return Err(Error::Frontier);
        }
        // reject any transfer watermark rewind or same-ordinal frontier substitution
        if start_ordinal < prior_start_watermark
            || (start_ordinal == prior_start_watermark
                && start.watermark_frontier_sha256 != prior_start_frontier)
        {
            return Err(Error::Frontier);
        }
        let first_page = page.checkpoint.start_member_sha256.is_some();
        // bind continuations only to the immediately preceding page of the same transfer
        if first_page {
            // require a first page to name its transfer start as predecessor
            if page.checkpoint.previous_page_sha256 != page.checkpoint.start_sha256 {
                return Err(Error::Frontier);
            }
            prior_start_watermark = start_ordinal;
            prior_start_frontier = start.watermark_frontier_sha256.clone();
        // reject a continuation detached from the immediately prior page
        } else if prior_page.is_none_or(|prior| {
            page.checkpoint.start_sha256 != prior.checkpoint.start_sha256
                || page.checkpoint.previous_page_sha256 != prior.checkpoint.page_sha256
        }) {
            return Err(Error::Frontier);
        }
        prior_ordinal = page.checkpoint.next_archive_commit_ordinal;
        prior_frontier = page.checkpoint.next_frontier_sha256.clone();
        prior_page = Some(page);
    }
    // require the restored suffix to end at the supplied current snapshot
    if prior_ordinal.to_string() != current_start.watermark_archive_commit_ordinal
        || prior_frontier != current_start.watermark_frontier_sha256
    {
        return Err(Error::Frontier);
    }
    Ok(())
}

/// Stream the typed prefix in page order and retain only selected payload occurrences.
fn stream_historical_prefix(
    current_start: VerifiedNativeRevisionColdStart,
    selection: NativeMaintenanceHistoricalSelection,
    pages: Vec<ParsedPage>,
) -> Result<NativeMaintenanceHistoricalCatalogOutcome, NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError as Error;

    let snapshot = field(&current_start.value, "servingSnapshot")
        .ok_or(Error::Start)?
        .clone();
    let pointers = snapshot_pointers(&snapshot)?;
    let mut current_receipts = HashMap::with_capacity(pointers.len());
    // retain exact current receipt JSON for byte-equality checks
    for pointer in &pointers {
        // refuse ambiguous current pointers under one receipt identity
        if current_receipts
            .insert(
                pointer.receipt.receiptSha256.clone(),
                pointer.receipt_value.clone(),
            )
            .is_some()
        {
            return Err(Error::CurrentPointer);
        }
    }
    let mut missing_current: HashSet<String> = current_receipts.keys().cloned().collect();
    let requested: HashSet<String> = selection.receipt_sha256s.iter().cloned().collect();
    let codec = ArchiveWireCodec::new(NativeWirePrimitives);
    let mut receipt_identities = Vec::new();
    let mut occurrences = Vec::new();
    let mut summaries = Vec::new();
    let mut predecessor_frontier = revision_frontier_genesis();
    let mut prior_ordinal = 0u64;
    let mut selected_bytes = 0usize;
    // validate every receipt and payload binding in exact checkpoint order
    for page in pages {
        let mut page_receipt_count = 0usize;
        // preserve compact checkpoint entry order within each page
        for entry in &page.checkpoint.entries {
            let mut entry_receipts = Vec::with_capacity(entry.receipts.len());
            // advance the global receipt chain through every grouped member
            for binding in &entry.receipts {
                let bytes = unique_member(&page.members, &binding.receipt_sha256, RECEIPT_KIND)?;
                let receipt = parse_receipt(bytes, &codec)?;
                let ordinal =
                    canonical_ordinal(&receipt.archiveCommitOrdinal).ok_or(Error::Receipt)?;
                // reject rollback, substitution, or a changed member body
                if ordinal <= prior_ordinal
                    || receipt.predecessorFrontierSha256 != predecessor_frontier
                    || receipt.receiptSha256 != binding.receipt_sha256
                    || digest(bytes) != binding.receipt_member_sha256
                {
                    return Err(Error::Receipt);
                }
                // bind current serving pointers to byte-identical archived receipts
                if let Some(expected) = current_receipts.get(&receipt.receiptSha256) {
                    let expected_bytes = NativeWirePrimitives
                        .encode_sorted_json(expected)
                        .ok_or(Error::Codec)?;
                    // preserve exact current receipt bytes rather than typed equality alone
                    if expected_bytes != bytes {
                        return Err(Error::CurrentPointer);
                    }
                    missing_current.remove(&receipt.receiptSha256);
                }
                predecessor_frontier = receipt.frontierSha256.clone();
                prior_ordinal = ordinal;
                page_receipt_count = page_receipt_count.checked_add(1).ok_or(Error::Size)?;
                receipt_identities.push(receipt.receiptSha256.clone());
                entry_receipts.push(receipt);
            }
            let requested_entry = entry_receipts
                .iter()
                .any(|receipt| requested.contains(&receipt.receiptSha256));
            let payload = page.members.iter().find(|member| {
                member.identity_sha256 == entry.payload_identity_sha256
                    && member.kind == entry.payload_kind
            });
            // require a payload exactly when the source restore plan selected it
            if selection.family.is_some() || requested_entry {
                let payload = payload.ok_or(Error::Payload)?;
                // retain only requested or range-selected semantic material
                if should_retain_payload(
                    entry,
                    &payload.payload,
                    &entry_receipts,
                    requested_entry,
                    &selection,
                    &codec,
                )? {
                    selected_bytes = selected_bytes
                        .checked_add(payload.payload.len())
                        .ok_or(Error::Size)?;
                    // stop before retaining more than the source selection ceiling
                    if selected_bytes > MAXIMUM_SELECTED_BYTES {
                        return Err(Error::Size);
                    }
                    occurrences.push(NativeMaintenanceHistoricalOccurrence {
                        graph_manifest_sha256: page.mapping.graph_manifest_sha256.clone(),
                        page_sha256: page.checkpoint.page_sha256.clone(),
                        payload_bytes: payload.payload.clone(),
                        payload_identity_sha256: entry.payload_identity_sha256.clone(),
                        payload_kind: entry.payload_kind.clone(),
                        publication_disposition: entry.publication_disposition.clone(),
                        receipts: entry_receipts,
                    });
                }
            }
        }
        // bind each checkpoint cursor to its restored receipt range
        if page.checkpoint.next_archive_commit_ordinal != prior_ordinal
            || page.checkpoint.next_frontier_sha256 != predecessor_frontier
        {
            return Err(Error::Frontier);
        }
        summaries.push(NativeMaintenanceHistoricalPageSummary {
            after_archive_commit_ordinal: page.checkpoint.after_archive_commit_ordinal_text.clone(),
            graph_manifest_sha256: page.mapping.graph_manifest_sha256,
            next_archive_commit_ordinal: page.checkpoint.next_archive_commit_ordinal_text,
            page_sha256: page.checkpoint.page_sha256,
            receipt_count: page_receipt_count,
            start_sha256: page.checkpoint.start_sha256,
        });
    }
    // require the complete archived prefix and every final serving pointer
    if receipt_identities.is_empty()
        || !missing_current.is_empty()
        || prior_ordinal.to_string() != current_start.watermark_archive_commit_ordinal
        || predecessor_frontier != current_start.watermark_frontier_sha256
    {
        return Err(Error::CurrentPointer);
    }
    let catalog = build_cold_catalog(
        &current_start,
        &pointers,
        receipt_identities.len(),
        predecessor_frontier,
        receipt_root(&receipt_identities)?,
    )?;
    let history_root_sha256 = history_root(&catalog, &occurrences)?;
    let history = NativeMaintenanceHistoricalArchiveIndex {
        catalog: catalog.clone(),
        contract_version: INDEX_VERSION,
        history_root_sha256: history_root_sha256.clone(),
        occurrences: occurrences.clone(),
        pages: summaries,
        receipt_count: receipt_identities.len(),
    };
    let mut by_receipt = HashMap::new();
    // index each retained occurrence back to every value-bearing receipt
    for occurrence in &occurrences {
        // preserve every grouped receipt pointer to its shared payload
        for receipt in &occurrence.receipts {
            // reject one receipt mapped to more than one retained occurrence
            if by_receipt
                .insert(receipt.receiptSha256.clone(), occurrence)
                .is_some()
            {
                return Err(Error::Receipt);
            }
        }
    }
    let mut members = Vec::with_capacity(pointers.len());
    // select only exact published payloads named by the current serving snapshot
    for pointer in pointers {
        let occurrence = by_receipt
            .get(&pointer.receipt.receiptSha256)
            .ok_or(Error::CurrentPointer)?;
        // terminal unpublished gaps never become semantic current members
        if occurrence.publication_disposition != "published" {
            return Err(Error::CurrentPointer);
        }
        members.push(NativeMaintenanceHistoricalCurrentMember {
            logical_received_at: pointer.logical_received_at,
            payload_bytes: occurrence.payload_bytes.clone(),
            payload_kind: occurrence.payload_kind.clone(),
            receipt: pointer.receipt,
            relation: pointer.relation,
        });
    }
    Ok(NativeMaintenanceHistoricalCatalogOutcome::Ready(Box::new(
        NativeMaintenanceHistoricalCatalogReady {
            catalog,
            history,
            input_manifest_sha256: history_root_sha256,
            members,
            serving_snapshot: snapshot,
        },
    )))
}

/// Decide whether one verified payload belongs to the bounded fitter selection.
fn should_retain_payload(
    entry: &ParsedCheckpointEntry,
    bytes: &[u8],
    receipts: &[AdjustmentRevisionCommitReceipt],
    requested: bool,
    selection: &NativeMaintenanceHistoricalSelection,
    codec: &ArchiveWireCodec<NativeWirePrimitives>,
) -> Result<bool, NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError as Error;

    // bind all selected bytes to the compact checkpoint identity
    if digest(bytes) != entry.payload_identity_sha256 {
        return Err(Error::Payload);
    }
    let mut validators = NativeMaintenanceControllerArchiveValidators::new();
    // require the payload's complete registered source schema
    if !validators.validate(&entry.payload_kind, bytes) {
        return Err(Error::Payload);
    }
    // retain explicitly requested current pointers before family decoding
    if requested {
        return Ok(true);
    }
    let Some(family) = selection.family else {
        return Ok(false);
    };
    // discard unqualified historical gaps before semantic decoding
    if entry.publication_disposition != "published" {
        return Ok(false);
    }
    let document = if matches!(
        entry.payload_kind.as_str(),
        "adjustment-shadow-revision-capsule/v1" | "adjustment-shadow-revision-capsule/v2"
    ) {
        let capsule = PrivateShadowReceiptCodec::decode_capsule_document(codec, bytes)
            .ok_or(Error::Payload)?;
        let document = NativeWirePrimitives
            .decode_json(&capsule.stage_input.sourceProjection)
            .ok_or(Error::Payload)?;
        // retain shadow history only for its exact source family
        if json_text(field(&document, "family").ok_or(Error::Payload)?)? != family.as_str() {
            return Ok(false);
        }
        document
    } else {
        decode_native_api_revision_document(bytes).ok_or(Error::Payload)?;
        let document = NativeWirePrimitives
            .decode_json(bytes)
            .ok_or(Error::Payload)?;
        let projection_kind = receipts
            .first()
            .map(|receipt| receipt.projectionKind)
            .ok_or(Error::Receipt)?;
        // preserve the predecessor's family-specific semantic revision classes
        let relevant = match projection_kind {
            AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision
            | AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch => true,
            AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource => {
                family != NativeMaintenanceHistoricalFamily::Temperature
                    || field(&document, "family")
                        .and_then(|value| json_text(value).ok())
                        .as_deref()
                        == Some("temperature")
            }
            AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput => {
                family == NativeMaintenanceHistoricalFamily::Rain
            }
            AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction => false,
        };
        // discard a valid but fitter-irrelevant semantic producer class
        if !relevant {
            return Ok(false);
        }
        document
    };
    let from = selection
        .from_at
        .as_deref()
        .and_then(canonical_instant)
        .ok_or(Error::Selection)?;
    let to = selection
        .to_at
        .as_deref()
        .and_then(canonical_instant)
        .ok_or(Error::Selection)?;
    Ok(has_clock_in_range(&document, None, from, to))
}

/// Build the exact compact serving catalog and its canonical root.
fn build_cold_catalog(
    start: &VerifiedNativeRevisionColdStart,
    pointers: &[SnapshotPointer],
    frontier_count: usize,
    frontier_root_sha256: String,
    receipt_root_sha256: String,
) -> Result<NativeMaintenanceColdCatalog, NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError as Error;

    let mut counts = NativeMaintenanceColdCatalogKindCounts {
        actual_best_match: 0,
        native_source: 0,
        rain_gate_input: 0,
        target_revision: 0,
    };
    // count only genuine current semantic producer pointers
    for pointer in pointers {
        match pointer.receipt.projectionKind {
            AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch => {
                counts.actual_best_match += 1;
            }
            AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource => {
                counts.native_source += 1;
            }
            AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput => {
                counts.rain_gate_input += 1;
            }
            AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision => {
                counts.target_revision += 1;
            }
            AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction => {}
        }
    }
    // require all four serving producer classes before exposing semantics
    if counts.actual_best_match == 0
        || counts.native_source == 0
        || counts.rain_gate_input == 0
        || counts.target_revision == 0
    {
        return Err(Error::Population);
    }
    let snapshot = field(&start.value, "servingSnapshot").ok_or(Error::Start)?;
    let serving_snapshot_sha256 = required_hash(snapshot, "snapshotSha256")?;
    let value = catalog_value(
        &start.watermark_archive_commit_ordinal,
        frontier_count,
        &frontier_root_sha256,
        counts,
        &receipt_root_sha256,
        &serving_snapshot_sha256,
    );
    let catalog_root_sha256 = digest(
        &NativeWirePrimitives
            .encode_canonical_json(&value)
            .ok_or(Error::Codec)?,
    );
    Ok(NativeMaintenanceColdCatalog {
        archive_commit_ordinal: start.watermark_archive_commit_ordinal.clone(),
        catalog_root_sha256,
        frontier_count,
        frontier_root_sha256,
        kind_counts: counts,
        receipt_root_sha256,
        serving_snapshot_sha256,
    })
}

/// Hash the exact history metadata without duplicating selected private payload bytes.
fn history_root(
    catalog: &NativeMaintenanceColdCatalog,
    occurrences: &[NativeMaintenanceHistoricalOccurrence],
) -> Result<String, NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError::Codec;

    let mut entries = Vec::with_capacity(occurrences.len());
    // retain occurrence and ordered receipt identities only
    for occurrence in occurrences {
        entries.push(object(vec![
            (
                "graphManifestSha256",
                string(&occurrence.graph_manifest_sha256),
            ),
            ("pageSha256", string(&occurrence.page_sha256)),
            (
                "payloadIdentitySha256",
                string(&occurrence.payload_identity_sha256),
            ),
            ("payloadKind", string(&occurrence.payload_kind)),
            (
                "publicationDisposition",
                string(&occurrence.publication_disposition),
            ),
            (
                "receiptSha256s",
                JsonValue::Array(
                    occurrence
                        .receipts
                        .iter()
                        .map(|receipt| string(&receipt.receiptSha256))
                        .collect(),
                ),
            ),
        ]));
    }
    let value = object(vec![
        ("catalogRootSha256", string(&catalog.catalog_root_sha256)),
        ("entries", JsonValue::Array(entries)),
    ]);
    Ok(digest(
        &NativeWirePrimitives
            .encode_canonical_json(&value)
            .ok_or(Codec)?,
    ))
}

/// Hash the exact JSON receipt identity array with its canonical final newline.
fn receipt_root(receipts: &[String]) -> Result<String, NativeMaintenanceHistoricalCatalogError> {
    let value = JsonValue::Array(receipts.iter().map(|value| string(value)).collect());
    Ok(digest(
        &NativeWirePrimitives
            .encode_canonical_json(&value)
            .ok_or(NativeMaintenanceHistoricalCatalogError::Codec)?,
    ))
}

/// Extract and type every current serving snapshot pointer.
fn snapshot_pointers(
    snapshot: &JsonValue,
) -> Result<Vec<SnapshotPointer>, NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError as Error;

    let JsonValue::Array(entries) = field(snapshot, "entries").ok_or(Error::Start)? else {
        return Err(Error::Start);
    };
    let codec = ArchiveWireCodec::new(NativeWirePrimitives);
    let mut pointers = Vec::with_capacity(entries.len());
    // retain source order and exact receipt JSON for archived byte comparison
    for entry in entries {
        let receipt_value = field(entry, "receipt").ok_or(Error::Start)?.clone();
        let receipt = codec
            .decode_revision_commit_value(&receipt_value)
            .ok_or(Error::Start)?;
        // recompute each current receipt before using it as an archive pointer
        if !codec.validate_revision_commit_receipt_identity(&receipt) {
            return Err(Error::Start);
        }
        pointers.push(SnapshotPointer {
            logical_received_at: required_text(entry, "logicalReceivedAt")?,
            receipt,
            receipt_value,
            relation: required_text(entry, "relation")?,
        });
    }
    Ok(pointers)
}

/// Parse one canonical commit receipt and recompute its identity and frontier.
fn parse_receipt(
    bytes: &[u8],
    codec: &ArchiveWireCodec<NativeWirePrimitives>,
) -> Result<AdjustmentRevisionCommitReceipt, NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError::Receipt;

    let mut validators = NativeMaintenanceControllerArchiveValidators::new();
    // use the same complete receipt schema as retained graph verification
    if !validators.validate(RECEIPT_KIND, bytes) {
        return Err(Receipt);
    }
    let value = NativeWirePrimitives.decode_json(bytes).ok_or(Receipt)?;
    let receipt = codec.decode_revision_commit_value(&value).ok_or(Receipt)?;
    codec
        .validate_revision_commit_receipt_identity(&receipt)
        .then_some(receipt)
        .ok_or(Receipt)
}

/// Validate one retained start and preserve its typed verified projection.
fn verify_start(
    bytes: &[u8],
) -> Result<VerifiedNativeRevisionColdStart, NativeMaintenanceHistoricalCatalogError> {
    verify_native_revision_cold_start_document(
        bytes,
        &mut NativeWirePrimitives::new(),
        &NativeWirePrimitives::new(),
    )
    .map_err(|_| NativeMaintenanceHistoricalCatalogError::Start)
}

/// Reverify the supplied current typed start against its retained canonical bytes.
fn validate_start(
    start: &VerifiedNativeRevisionColdStart,
) -> Result<(), NativeMaintenanceHistoricalCatalogError> {
    let verified = verify_start(&start.canonical_bytes)?;
    (verified == *start)
        .then_some(())
        .ok_or(NativeMaintenanceHistoricalCatalogError::Start)
}

/// Validate one bounded unique selection and its optional half-open time range.
fn validate_selection(
    selection: &NativeMaintenanceHistoricalSelection,
) -> Result<(), NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError::Selection;

    // bound, validate, and de-duplicate all explicitly requested receipts
    if selection.receipt_sha256s.len() > MAXIMUM_SELECTED_RECEIPTS
        || selection
            .receipt_sha256s
            .iter()
            .any(|identity| !valid_hash(identity))
        || selection
            .receipt_sha256s
            .iter()
            .collect::<HashSet<_>>()
            .len()
            != selection.receipt_sha256s.len()
    {
        return Err(Selection);
    }
    // keep family absence inseparable from absent range clocks
    match selection.family {
        None if selection.from_at.is_none() && selection.to_at.is_none() => Ok(()),
        Some(_) => {
            let from = selection
                .from_at
                .as_deref()
                .and_then(canonical_instant)
                .ok_or(Selection)?;
            let to = selection
                .to_at
                .as_deref()
                .and_then(canonical_instant)
                .ok_or(Selection)?;
            (from < to).then_some(()).ok_or(Selection)
        }
        None => Err(Selection),
    }
}

/// Validate one exact immutable journal mapping.
fn validate_mapping(
    mapping: &NativeMaintenanceHistoricalCatalogMapping,
) -> Result<(), NativeMaintenanceHistoricalCatalogError> {
    (valid_hash(&mapping.graph_manifest_sha256)
        && valid_hash(&mapping.page_sha256)
        && valid_hash(&mapping.start_sha256))
    .then_some(())
    .ok_or(NativeMaintenanceHistoricalCatalogError::Mapping)
}

/// Restore one exact selected member into a bounded in-memory sink.
fn restore_one_member(
    store: &mut impl NativeMaintenanceFullGraphObjects,
    graph_manifest_sha256: &str,
    identity_sha256: &str,
    file_name: &str,
    maximum_bytes: u64,
) -> Result<Vec<u8>, NativeMaintenanceHistoricalCatalogError> {
    let request = RestoreRequest {
        file_name: file_name.to_owned(),
        identity_sha256: identity_sha256.to_owned(),
        kind: String::new(),
    };
    let mut restored = restore_members(
        store,
        graph_manifest_sha256,
        std::slice::from_ref(&request),
        maximum_bytes,
    )?;
    restored
        .remove(file_name)
        .ok_or(NativeMaintenanceHistoricalCatalogError::Restore)
}

struct RestoreRequest {
    file_name: String,
    identity_sha256: String,
    kind: String,
}

/// Restore one bounded target set through a single verified graph traversal.
fn restore_members(
    store: &mut impl NativeMaintenanceFullGraphObjects,
    graph_manifest_sha256: &str,
    requests: &[RestoreRequest],
    maximum_bytes: u64,
) -> Result<HashMap<String, Vec<u8>>, NativeMaintenanceHistoricalCatalogError> {
    use NativeMaintenanceHistoricalCatalogError::Restore;

    let graph = parse_digest(graph_manifest_sha256).ok_or(Restore)?;
    let identities = requests
        .iter()
        .map(|request| {
            Some((
                request.file_name.as_str(),
                parse_digest(&request.identity_sha256)?,
            ))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(Restore)?;
    let targets = identities
        .iter()
        .map(
            |(file_name, identity_sha256)| NativeMaintenanceGraphRestoreTarget {
                file_name,
                identity_sha256: *identity_sha256,
            },
        )
        .collect::<Vec<_>>();
    let mut validators = NativeMaintenanceControllerArchiveValidators::new();
    let mut sink = MemoryRestoreSink::default();
    restore_native_maintenance_full_graph(
        store,
        graph,
        &mut validators,
        &mut sink,
        &targets,
        Some(maximum_bytes),
    )
    .ok_or(Restore)?;
    (sink.files.len() == requests.len())
        .then_some(sink.files)
        .ok_or(Restore)
}

#[derive(Default)]
struct MemoryRestoreSink {
    files: HashMap<String, Vec<u8>>,
}

impl NativeMaintenanceGraphRestoreSink for MemoryRestoreSink {
    /// Retain fully consumed verified members for immediate typed assembly.
    fn write_exclusive(
        &mut self,
        name: &str,
        reader: &mut impl Read,
        expected_length: u64,
    ) -> Option<()> {
        // refuse duplicate writes or unsupported output names
        if self.files.contains_key(name) {
            return None;
        }
        let length = usize::try_from(expected_length).ok()?;
        let mut bytes = Vec::with_capacity(length);
        reader.read_to_end(&mut bytes).ok()?;
        // bind the sink result to the graph manifest's admitted length
        if bytes.len() != length {
            return None;
        }
        self.files.insert(name.to_owned(), bytes);
        Some(())
    }
}

/// Resolve exactly one retained identity and kind without accepting duplicates.
fn unique_member<'a>(
    members: &'a [NativeMaintenanceHistoricalRetainedMember],
    identity: &str,
    kind: &str,
) -> Result<&'a [u8], NativeMaintenanceHistoricalCatalogError> {
    let mut selected = members
        .iter()
        .filter(|member| member.identity_sha256 == identity && member.kind == kind);
    let member = selected
        .next()
        .ok_or(NativeMaintenanceHistoricalCatalogError::Restore)?;
    // reject duplicate aliases within one verified retained graph input
    if selected.next().is_some() {
        return Err(NativeMaintenanceHistoricalCatalogError::Restore);
    }
    Ok(&member.payload)
}

/// Traverse parser-validated evidence for one causal clock in the half-open interval.
fn has_clock_in_range(value: &JsonValue, key: Option<&str>, from: i64, to: i64) -> bool {
    match value {
        JsonValue::String(units) => {
            let Some(key) = key else {
                return false;
            };
            let Ok(value) = String::from_utf16(units) else {
                return false;
            };
            RANGE_CLOCK_KEYS.contains(&key)
                && canonical_instant(&value).is_some_and(|instant| instant >= from && instant < to)
        }
        JsonValue::Array(values) => {
            // stop at the first exact clock intersecting the selected interval
            values
                .iter()
                .any(|value| has_clock_in_range(value, None, from, to))
        }
        JsonValue::Object(fields) => {
            // traverse only exact decoded object members
            fields.iter().any(|(name, value)| {
                String::from_utf16(name)
                    .ok()
                    .is_some_and(|name| has_clock_in_range(value, Some(&name), from, to))
            })
        }
        _ => false,
    }
}

/// Build the catalog value whose canonical bytes define the catalog root.
fn catalog_value(
    archive_commit_ordinal: &str,
    frontier_count: usize,
    frontier_root_sha256: &str,
    counts: NativeMaintenanceColdCatalogKindCounts,
    receipt_root_sha256: &str,
    serving_snapshot_sha256: &str,
) -> JsonValue {
    object(vec![
        ("archiveCommitOrdinal", string(archive_commit_ordinal)),
        ("contractVersion", string(CATALOG_VERSION)),
        ("frontierCount", JsonValue::Number(frontier_count as f64)),
        ("frontierRootSha256", string(frontier_root_sha256)),
        (
            "kindCounts",
            object(vec![
                (
                    "actual_best_match",
                    JsonValue::Number(counts.actual_best_match as f64),
                ),
                (
                    "native_source",
                    JsonValue::Number(counts.native_source as f64),
                ),
                (
                    "rain_gate_input",
                    JsonValue::Number(counts.rain_gate_input as f64),
                ),
                (
                    "target_revision",
                    JsonValue::Number(counts.target_revision as f64),
                ),
            ]),
        ),
        ("receiptRootSha256", string(receipt_root_sha256)),
        ("servingSnapshotSha256", string(serving_snapshot_sha256)),
    ])
}

/// Resolve one canonical exact 20xx millisecond UTC instant.
fn canonical_instant(value: &str) -> Option<i64> {
    // retain the source's deliberately bounded four-digit UTC grammar
    if value.len() != 24 || !value.starts_with("20") || !value.ends_with('Z') {
        return None;
    }
    RevisionArchiveCanonicalCodec::canonical_utc_milliseconds(&NativeWirePrimitives, value)
}

/// Parse one canonical uint64 decimal spelling.
fn canonical_ordinal(value: &str) -> Option<u64> {
    let parsed = value.parse::<u64>().ok()?;
    (parsed.to_string() == value).then_some(parsed)
}

/// Derive the immutable global frontier genesis.
fn revision_frontier_genesis() -> String {
    digest(b"adjustment-revision-frontier/v1\n0\n")
}

/// Hash exact bytes through the approved native SHA-256 owner.
fn digest(bytes: &[u8]) -> String {
    crate::native_maintenance_retirement_files_v2::format_digest(
        RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, bytes),
    )
}

/// Decode one lowercase SHA-256 content address.
fn parse_digest(value: &str) -> Option<[u8; 32]> {
    Some(*weather_edge::RevisionGapSha256::from_lower_hex(value)?.as_bytes())
}

/// Read one required canonical uint64 field.
fn required_ordinal(
    value: &JsonValue,
    name: &str,
) -> Result<u64, NativeMaintenanceHistoricalCatalogError> {
    required_text(value, name)
        .ok()
        .and_then(|value| canonical_ordinal(&value))
        .ok_or(NativeMaintenanceHistoricalCatalogError::Checkpoint)
}

/// Read one required lowercase digest field.
fn required_hash(
    value: &JsonValue,
    name: &str,
) -> Result<String, NativeMaintenanceHistoricalCatalogError> {
    let value = required_text(value, name)?;
    valid_hash(&value)
        .then_some(value)
        .ok_or(NativeMaintenanceHistoricalCatalogError::Checkpoint)
}

/// Read one nullable lowercase digest field.
fn optional_hash(
    value: &JsonValue,
    name: &str,
) -> Result<Option<String>, NativeMaintenanceHistoricalCatalogError> {
    match field(value, name) {
        Some(JsonValue::Null) => Ok(None),
        Some(value) => Ok(Some(json_hash(value)?)),
        None => Err(NativeMaintenanceHistoricalCatalogError::Checkpoint),
    }
}

/// Read one required JSON string field.
fn required_text(
    value: &JsonValue,
    name: &str,
) -> Result<String, NativeMaintenanceHistoricalCatalogError> {
    text(value, name).ok_or(NativeMaintenanceHistoricalCatalogError::Checkpoint)
}

/// Decode one digest-shaped JSON string.
fn json_hash(value: &JsonValue) -> Result<String, NativeMaintenanceHistoricalCatalogError> {
    let value = json_text(value)?;
    valid_hash(&value)
        .then_some(value)
        .ok_or(NativeMaintenanceHistoricalCatalogError::Checkpoint)
}

/// Decode one exact UTF-16 JSON string.
fn json_text(value: &JsonValue) -> Result<String, NativeMaintenanceHistoricalCatalogError> {
    let JsonValue::String(units) = value else {
        return Err(NativeMaintenanceHistoricalCatalogError::Payload);
    };
    String::from_utf16(units).map_err(|_| NativeMaintenanceHistoricalCatalogError::Payload)
}

/// Construct one UTF-16 JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Construct one source JSON object in the provided field order.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

#[cfg(test)]
#[path = "native_maintenance_historical_catalog_test.rs"]
mod tests;
