//! Read-only selection for the retained permanent revision-gap transfer.

use crate::revision_archive_canonical::is_sha256;

/// Wire contract emitted by `revision-gap-start-v1`.
pub const REVISION_GAP_TRANSFER_START_CONTRACT_VERSION: &str =
    "adjustment-revision-gap-transfer-start/v1";
/// Wire contract emitted by `revision-gap-page-v1`.
pub const REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION: &str =
    "adjustment-revision-gap-payload-page/v1";
/// Maximum canonical page size retained by the Node archive owner.
pub const REVISION_GAP_PAGE_MAXIMUM_BYTES: usize = 4_832 * 1_024;
/// Maximum number of staged values carried by one page.
pub const REVISION_GAP_PAGE_MAXIMUM_ENTRIES: usize = 2;
/// Maximum number of online files in each bounded spool family.
pub const REVISION_GAP_ONLINE_MAXIMUM_FILES: usize = 2;
/// Maximum number of permanent metadata records in each gap family.
pub const REVISION_GAP_METADATA_MAXIMUM_FILES: usize = 4_096;
/// Maximum bytes read for one permanent gap or stage receipt.
pub const REVISION_GAP_RECEIPT_MAXIMUM_BYTES: usize = 4_096;
/// Maximum bytes read for one grouped batch commit receipt file.
pub const REVISION_GAP_BATCH_COMMIT_RECEIPT_MAXIMUM_BYTES: usize = 256 * 1_024;
/// Maximum bytes read for one ordinary staged projection.
pub const REVISION_GAP_PROJECTION_MAXIMUM_BYTES: usize = 3_500 * 1_024;
/// Maximum bytes read for one retained shadow stage.
pub const REVISION_GAP_SHADOW_STAGE_MAXIMUM_BYTES: usize = 2 * 1_024 * 1_024;
/// Maximum bytes read for one abandoned rain control state.
pub const REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES: usize = 32 * 1_024;

/// One digest admitted by the existing SHA-256 adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RevisionGapSha256([u8; 32]);

impl RevisionGapSha256 {
    /// Retain one already-decoded SHA-256 value without adding a codec here.
    pub const fn admitted(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrow the fixed digest bytes for a later wire adapter.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Read a closed lowercase-hex archive argument without hashing any bytes.
    pub fn from_lower_hex(text: &str) -> Option<Self> {
        // check the reviewed length and alphabet before indexing pairs
        if !is_sha256(text) {
            return None;
        }
        let mut digest = [0_u8; 32];
        // decode only the verified ASCII pairs
        for (output, pair) in digest.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
            *output = hex_digit(pair[0]) * 16 + hex_digit(pair[1]);
        }
        Some(Self(digest))
    }
}

/// Convert one already admitted lowercase ASCII digit.
fn hex_digit(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        _ => byte - b'a' + 10,
    }
}

/// Current durable gap-transfer frontier projected without acknowledgement fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionGapFrontier {
    pub frontier_sha256: RevisionGapSha256,
    pub page_count: u64,
}

/// Compact response for `revision-gap-start-v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionGapTransferStart {
    pub contract_version: &'static str,
    pub frontier_sha256: RevisionGapSha256,
    pub page_count: u64,
    pub start_sha256: RevisionGapSha256,
}

/// Exact cursor supplied back to `revision-gap-page-v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionGapPageRequest {
    pub frontier_sha256: RevisionGapSha256,
    pub start_sha256: RevisionGapSha256,
}

/// File census collected before scanning staged permanent gaps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RevisionGapRetainedCensus {
    pub batch_commit_receipt_files: usize,
    pub batch_commit_receipt_maximum_bytes: usize,
    pub rain_control_state_gap_files: usize,
    pub revision_gap_files: usize,
    pub revision_gap_maximum_bytes: usize,
    pub revision_projection_files: usize,
    pub shadow_gap_files: usize,
    pub shadow_gap_maximum_bytes: usize,
    pub shadow_stage_files: usize,
    pub single_commit_receipt_files: usize,
    pub single_commit_receipt_maximum_bytes: usize,
}

/// Exact retained byte classes already parsed by the archive boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionGapRetainedBytes {
    /// One ordinary staged projection and its separate durable stage receipt.
    Projection {
        gap_bytes: usize,
        payload_bytes: usize,
        stage_receipt_bytes: usize,
        rain_control_state: Option<RevisionGapRainControlStateBytes>,
    },
    /// One abandoned monthly control state and its separate stage receipt.
    RainControlState {
        gap_bytes: usize,
        payload_bytes: usize,
        stage_receipt_bytes: usize,
    },
    /// One self-contained shadow stage whose receipt is embedded in the stage.
    ShadowStage {
        gap_bytes: usize,
        stage_bytes: usize,
    },
}

/// Optional public rain-state members carried beside one control projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionGapRainControlStateBytes {
    pub state_bytes: usize,
    pub stage_receipt_bytes: usize,
}

/// One fully admitted retained entry plus its permanent ordering identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedRevisionGapEntry<T> {
    pub gap_sha256: RevisionGapSha256,
    pub retained_bytes: RevisionGapRetainedBytes,
    pub value: T,
}

/// One frozen page, either already retained or newly selected for retention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionGapPayloadPage<T> {
    pub canonical_byte_count: usize,
    pub contract_version: &'static str,
    pub entries: Vec<RetainedRevisionGapEntry<T>>,
    pub idle: bool,
    pub member_root_sha256: RevisionGapSha256,
    pub page_sha256: RevisionGapSha256,
    pub predecessor_frontier_sha256: RevisionGapSha256,
    pub start_sha256: RevisionGapSha256,
}

/// Immutable inputs observed under the existing revision-spool read lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionGapRetainedSnapshot<T> {
    pub available_entries: Vec<RetainedRevisionGapEntry<T>>,
    pub census: RevisionGapRetainedCensus,
    pub frontier: RevisionGapFrontier,
    pub pending_page: Option<RevisionGapPayloadPage<T>>,
}

/// Result distinguishes an existing frozen page from a newly projected response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevisionGapPageSelection<T> {
    Retained(RevisionGapPayloadPage<T>),
    New(RevisionGapPayloadPage<T>),
}

/// Unsigned start fields passed to the admitted canonical identity owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionGapUnsignedStart {
    pub contract_version: &'static str,
    pub frontier_sha256: RevisionGapSha256,
    pub page_count: u64,
}

/// Unsigned page fields passed to the admitted canonical identity owner.
#[derive(Debug, Clone, Copy)]
pub struct RevisionGapUnsignedPage<'a, T> {
    pub contract_version: &'static str,
    pub entries: &'a [RetainedRevisionGapEntry<T>],
    pub idle: bool,
    pub predecessor_frontier_sha256: RevisionGapSha256,
    pub start_sha256: RevisionGapSha256,
}

/// Canonical identities and exact encoded size returned by the existing codec owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedRevisionGapPageIdentity {
    pub canonical_byte_count: usize,
    pub member_root_sha256: RevisionGapSha256,
    pub page_sha256: RevisionGapSha256,
}

/// Existing canonical JSON and SHA-256 boundary used without reimplementing either codec.
pub trait RevisionGapIdentityAdapter<T> {
    /// Bind the compact start projection to its canonical digest.
    fn start_sha256(
        &mut self,
        start: RevisionGapUnsignedStart,
    ) -> Result<RevisionGapSha256, RevisionGapTransferError>;

    /// Bind one candidate page to its member root, page digest, and exact wire size.
    fn page_identity(
        &mut self,
        page: RevisionGapUnsignedPage<'_, T>,
    ) -> Result<AdmittedRevisionGapPageIdentity, RevisionGapTransferError>;
}

/// Bounded ordinary failures from the read-only transfer selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionGapTransferError {
    CensusDiffers,
    IdentityUnavailable,
    PayloadExceedsPageLimit,
    RetainedBytesDiffer,
    RetainedPageDiffers,
    TransferStartDiffers,
}

/// Project the current compact frontier into the exact start contract.
pub fn read_revision_gap_transfer_start<T>(
    frontier: RevisionGapFrontier,
    identity: &mut impl RevisionGapIdentityAdapter<T>,
) -> Result<RevisionGapTransferStart, RevisionGapTransferError> {
    let unsigned = RevisionGapUnsignedStart {
        contract_version: REVISION_GAP_TRANSFER_START_CONTRACT_VERSION,
        frontier_sha256: frontier.frontier_sha256,
        page_count: frontier.page_count,
    };
    let start_sha256 = identity.start_sha256(unsigned)?;
    Ok(RevisionGapTransferStart {
        contract_version: unsigned.contract_version,
        frontier_sha256: unsigned.frontier_sha256,
        page_count: unsigned.page_count,
        start_sha256,
    })
}

/// Select the exact pending page or freeze the next bounded ordered page.
pub fn select_revision_gap_payload_page<T>(
    request: RevisionGapPageRequest,
    mut snapshot: RevisionGapRetainedSnapshot<T>,
    identity: &mut impl RevisionGapIdentityAdapter<T>,
) -> Result<RevisionGapPageSelection<T>, RevisionGapTransferError> {
    let start = read_revision_gap_transfer_start(snapshot.frontier, identity)?;
    // bind the read to the exact frontier the caller observed
    if start.start_sha256 != request.start_sha256
        || start.frontier_sha256 != request.frontier_sha256
    {
        return Err(RevisionGapTransferError::TransferStartDiffers);
    }

    // retain an already-frozen nonempty page ahead of all current spool changes
    if let Some(pending) = snapshot.pending_page.take() {
        validate_retained_page(&pending, request)?;
        return Ok(RevisionGapPageSelection::Retained(pending));
    }

    select_new_revision_gap_payload_page(
        request,
        snapshot.census,
        snapshot.available_entries,
        identity,
    )
    .map(RevisionGapPageSelection::New)
}

/// Select new members after the native spool has bound the cursor and checked pending storage.
pub(crate) fn select_new_revision_gap_payload_page<T>(
    request: RevisionGapPageRequest,
    census: RevisionGapRetainedCensus,
    mut available_entries: Vec<RetainedRevisionGapEntry<T>>,
    identity: &mut impl RevisionGapIdentityAdapter<T>,
) -> Result<RevisionGapPayloadPage<T>, RevisionGapTransferError> {
    validate_census(census)?;
    // parse and bound the complete collected spool before taking its first two members
    for entry in &available_entries {
        validate_retained_entry(entry)?;
    }
    // match the source's lowercase digest order before applying page limits
    available_entries.sort_by_key(|entry| entry.gap_sha256);

    let mut selected = Vec::with_capacity(REVISION_GAP_PAGE_MAXIMUM_ENTRIES);
    // admit candidates one at a time so an oversized second member leaves a one-entry page
    for entry in available_entries {
        selected.push(entry);
        let candidate = identity.page_identity(unsigned_page(&selected, request))?;
        // reject an unpageable first payload but stop before a later oversized payload
        if candidate.canonical_byte_count > REVISION_GAP_PAGE_MAXIMUM_BYTES {
            if selected.len() == 1 {
                return Err(RevisionGapTransferError::PayloadExceedsPageLimit);
            }
            selected.pop();
            break;
        }
        // stop at the reviewed two online payload slots
        if selected.len() == REVISION_GAP_PAGE_MAXIMUM_ENTRIES {
            break;
        }
    }

    let admitted = identity.page_identity(unsigned_page(&selected, request))?;
    // preserve the hard wire ceiling even if the canonical adapter changes between calls
    if admitted.canonical_byte_count > REVISION_GAP_PAGE_MAXIMUM_BYTES {
        return Err(RevisionGapTransferError::PayloadExceedsPageLimit);
    }
    Ok(RevisionGapPayloadPage {
        canonical_byte_count: admitted.canonical_byte_count,
        contract_version: REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION,
        idle: selected.is_empty(),
        entries: selected,
        member_root_sha256: admitted.member_root_sha256,
        page_sha256: admitted.page_sha256,
        predecessor_frontier_sha256: request.frontier_sha256,
        start_sha256: request.start_sha256,
    })
}

/// Build the exact unsigned identity view for one candidate page.
fn unsigned_page<T>(
    entries: &[RetainedRevisionGapEntry<T>],
    request: RevisionGapPageRequest,
) -> RevisionGapUnsignedPage<'_, T> {
    RevisionGapUnsignedPage {
        contract_version: REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION,
        entries,
        idle: entries.is_empty(),
        predecessor_frontier_sha256: request.frontier_sha256,
        start_sha256: request.start_sha256,
    }
}

/// Preserve the exact bounded directory populations read by the Node collector.
fn validate_census(census: RevisionGapRetainedCensus) -> Result<(), RevisionGapTransferError> {
    // sum both supported commit receipt representations without integer wraparound
    let Some(commit_receipt_files) = census
        .single_commit_receipt_files
        .checked_add(census.batch_commit_receipt_files)
    else {
        return Err(RevisionGapTransferError::CensusDiffers);
    };
    // keep online receipts and staged families within their common two-slot bound
    if commit_receipt_files > REVISION_GAP_ONLINE_MAXIMUM_FILES
        || census.rain_control_state_gap_files > REVISION_GAP_ONLINE_MAXIMUM_FILES
        || census.revision_projection_files > REVISION_GAP_ONLINE_MAXIMUM_FILES
        || census.shadow_stage_files > REVISION_GAP_ONLINE_MAXIMUM_FILES
        || census.revision_gap_files > REVISION_GAP_METADATA_MAXIMUM_FILES
        || census.shadow_gap_files > REVISION_GAP_METADATA_MAXIMUM_FILES
        || !bounded_census_bytes(
            census.single_commit_receipt_files,
            census.single_commit_receipt_maximum_bytes,
            REVISION_GAP_RECEIPT_MAXIMUM_BYTES,
        )
        || !bounded_census_bytes(
            census.batch_commit_receipt_files,
            census.batch_commit_receipt_maximum_bytes,
            REVISION_GAP_BATCH_COMMIT_RECEIPT_MAXIMUM_BYTES,
        )
        || !bounded_census_bytes(
            census.revision_gap_files,
            census.revision_gap_maximum_bytes,
            REVISION_GAP_RECEIPT_MAXIMUM_BYTES,
        )
        || !bounded_census_bytes(
            census.shadow_gap_files,
            census.shadow_gap_maximum_bytes,
            REVISION_GAP_RECEIPT_MAXIMUM_BYTES,
        )
    {
        return Err(RevisionGapTransferError::CensusDiffers);
    }
    Ok(())
}

/// Bind an observed maximum byte count to the census that produced it.
fn bounded_census_bytes(files: usize, maximum_bytes: usize, limit: usize) -> bool {
    maximum_bytes <= limit && (files == 0) == (maximum_bytes == 0)
}

/// Enforce the source read ceilings before an admitted retained value reaches a page.
pub(crate) fn validate_retained_entry<T>(
    entry: &RetainedRevisionGapEntry<T>,
) -> Result<(), RevisionGapTransferError> {
    // apply the file ceilings for the exact retained source representation
    let valid = match entry.retained_bytes {
        RevisionGapRetainedBytes::Projection {
            gap_bytes,
            payload_bytes,
            stage_receipt_bytes,
            rain_control_state,
        } => {
            let ordinary = gap_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES
                && payload_bytes <= REVISION_GAP_PROJECTION_MAXIMUM_BYTES
                && stage_receipt_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES;
            // require both optional public control-state members inside their source bounds
            ordinary
                && rain_control_state.is_none_or(|state| {
                    state.state_bytes <= REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES
                        && state.stage_receipt_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES
                })
        }
        RevisionGapRetainedBytes::RainControlState {
            gap_bytes,
            payload_bytes,
            stage_receipt_bytes,
        } => {
            gap_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES
                && payload_bytes <= REVISION_GAP_RAIN_CONTROL_STATE_MAXIMUM_BYTES
                && stage_receipt_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES
        }
        RevisionGapRetainedBytes::ShadowStage {
            gap_bytes,
            stage_bytes,
        } => {
            gap_bytes <= REVISION_GAP_RECEIPT_MAXIMUM_BYTES
                && stage_bytes <= REVISION_GAP_SHADOW_STAGE_MAXIMUM_BYTES
        }
    };
    // reject values the archive storage adapter could not have admitted
    if !valid {
        return Err(RevisionGapTransferError::RetainedBytesDiffer);
    }
    Ok(())
}

/// Recheck the immutable pending-page bindings and exact retained page caps.
pub(crate) fn validate_retained_page<T>(
    page: &RevisionGapPayloadPage<T>,
    request: RevisionGapPageRequest,
) -> Result<(), RevisionGapTransferError> {
    // retain only a canonical page for the exact observed start and predecessor
    if page.contract_version != REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION
        || page.entries.len() > REVISION_GAP_PAGE_MAXIMUM_ENTRIES
        || page.idle != page.entries.is_empty()
        || page.canonical_byte_count > REVISION_GAP_PAGE_MAXIMUM_BYTES
        || page.start_sha256 != request.start_sha256
        || page.predecessor_frontier_sha256 != request.frontier_sha256
    {
        return Err(RevisionGapTransferError::RetainedPageDiffers);
    }
    // reapply every individual storage read ceiling to the frozen members
    for entry in &page.entries {
        validate_retained_entry(entry)?;
    }
    Ok(())
}
