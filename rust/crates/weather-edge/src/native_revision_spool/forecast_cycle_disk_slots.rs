//! Native current/next durability for bounded forecast cycle pages.

use std::io;
use std::path::{Path, PathBuf};

use crate::forecast_cycle_pages::{
    FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS, FORECAST_CYCLE_MAXIMUM_PAGES,
    FORECAST_CYCLE_PAGE_CONTRACT_VERSION, FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES,
    ForecastCyclePageCodec, ForecastCyclePageDurabilityProof, ForecastCyclePageHeader,
    ForecastCyclePagePersistence, ForecastCyclePagePersistenceRequest, ForecastCyclePageSlotKind,
    ForecastCycleProjection, ForecastCycleSha256,
};
use crate::native_revision_spool_file::{
    ensure_child_directory, publish_immutable, read_optional_file, require_directory,
};

use super::forecast_allocation::{
    FORECAST_ALLOCATION_FREE_INODE_FLOOR, FORECAST_ALLOCATION_REQUIRED_FREE_BYTES,
    ForecastAllocationStatfsPort,
};

const ONLINE_DIRECTORY: &str = "online-pages";
const SLOT_DIRECTORY: &str = "slots";
const METADATA_MAXIMUM_BYTES: usize = 32 * 1_024;
const SLOT_MAXIMUM_BYTES: usize = 320 * 1_024;
const SAFE_JS_INTEGER_MAXIMUM: u64 = 9_007_199_254_740_991;
const CHILDREN: [&str; 8] = [
    "acknowledgements",
    "archive-acknowledgements",
    "final-acknowledgements",
    "final-manifests",
    "gaps",
    "input-seals",
    "locks",
    SLOT_DIRECTORY,
];

/// Exact typed metadata carried before one persisted page payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastCycleDiskSlotMetadata {
    pub header: ForecastCyclePageHeader,
    pub page_sha256: ForecastCycleSha256,
    pub projections: Vec<ForecastCycleProjection>,
}

/// One fully validated immutable current or next page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastCycleDiskSlotPage {
    pub header: ForecastCyclePageHeader,
    pub page_sha256: ForecastCycleSha256,
    pub payload: Vec<u8>,
    pub projections: Vec<ForecastCycleProjection>,
}

impl ForecastCycleDiskSlotPage {
    /// Project the validated retained slot into value-independent recovery metadata.
    pub fn page_metadata(&self) -> crate::forecast_cycle_pages::ForecastCyclePage {
        crate::forecast_cycle_pages::ForecastCyclePage {
            page_index: self.header.page_index,
            page_sha256: self.page_sha256,
            predecessor_page_sha256: self.header.predecessor_page_sha256,
            payload_sha256: self.header.payload_sha256,
            payload_offset: self.header.payload_offset,
            payload_length: self.payload.len(),
            projection_identities: self
                .projections
                .iter()
                .map(|item| item.identity_sha256)
                .collect(),
            projection_channels: self.projections.iter().map(|item| item.channel).collect(),
        }
    }
}

/// Existing canonical JSON boundary for disk metadata only.
pub trait ForecastCycleDiskSlotCodec: ForecastCyclePageCodec {
    /// Encode exact canonical newline-framed header, page digest, and projections.
    fn encode_disk_slot_metadata(
        &self,
        metadata: &ForecastCycleDiskSlotMetadata,
    ) -> Option<Vec<u8>>;

    /// Decode only an original canonical frame, preserving source acceptance of extra outer keys.
    ///
    /// The decoder must compare the original decoded document to its canonical bytes before
    /// projecting the known header, page digest, and projections. Header and projection objects
    /// remain exact closed shapes; outer metadata extras are ignored only after that byte check.
    fn decode_disk_slot_metadata(
        &self,
        canonical_bytes: &[u8],
    ) -> Option<ForecastCycleDiskSlotMetadata>;
}

/// Ordinary slot initialization, admission, integrity, and durability failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastCycleDiskSlotError {
    CapacityRefused,
    CapacityUnavailable,
    Codec,
    Collision,
    InvalidPage,
    MetadataTooLarge,
    SlotTooLarge,
    Storage,
}

/// Native fixed-slot owner over an approved metadata codec and statfs source.
pub struct NativeForecastCycleDiskSlots<Codec, Statfs> {
    root: PathBuf,
    codec: Codec,
    statfs: Statfs,
}

impl<Codec, Statfs> NativeForecastCycleDiskSlots<Codec, Statfs> {
    /// Create only the exact online-page directory set below an admitted private root.
    pub fn initialize(root: PathBuf, codec: Codec, statfs: Statfs) -> io::Result<Self> {
        require_directory(&root)?;
        let online = root.join(ONLINE_DIRECTORY);
        ensure_child_directory(&online)?;

        // create every fixed Node metadata class without caller-selected paths
        for child in CHILDREN {
            ensure_child_directory(&online.join(child))?;
        }
        Ok(Self {
            root,
            codec,
            statfs,
        })
    }

    /// Borrow the admitted adjustment-evidence root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reuse the admitted codec at sibling durability boundaries for this same root.
    pub(super) const fn codec(&self) -> &Codec {
        &self.codec
    }

    /// Recover the injected metadata and capacity owners after the slot lifecycle.
    pub fn into_parts(self) -> (PathBuf, Codec, Statfs) {
        (self.root, self.codec, self.statfs)
    }
}

impl<Codec: ForecastCycleDiskSlotCodec, Statfs> NativeForecastCycleDiskSlots<Codec, Statfs> {
    /// Read and validate one exact current or next immutable slot.
    pub fn read_slot(
        &self,
        slot: ForecastCyclePageSlotKind,
    ) -> Result<Option<ForecastCycleDiskSlotPage>, ForecastCycleDiskSlotError> {
        use ForecastCycleDiskSlotError as Error;

        let Some(bytes) = read_optional_file(&self.slot_path(slot), SLOT_MAXIMUM_BYTES)
            .map_err(|_| Error::Storage)?
        else {
            return Ok(None);
        };

        // reject every truncated prefix before reading its big-endian length
        if bytes.len() < 5 {
            return Err(Error::InvalidPage);
        }
        let metadata_length = usize::try_from(u32::from_be_bytes(
            bytes[..4].try_into().map_err(|_| Error::InvalidPage)?,
        ))
        .map_err(|_| Error::InvalidPage)?;
        let metadata_end = 4_usize
            .checked_add(metadata_length)
            .ok_or(Error::InvalidPage)?;

        // retain the exact metadata and nonempty payload bounds inside one slot
        if !(2..=METADATA_MAXIMUM_BYTES).contains(&metadata_length) || metadata_end >= bytes.len() {
            return Err(Error::InvalidPage);
        }
        let metadata_bytes = &bytes[4..metadata_end];
        let metadata = self
            .codec
            .decode_disk_slot_metadata(metadata_bytes)
            .ok_or(Error::Codec)?;
        let payload = bytes[metadata_end..].to_vec();
        let projections = validate_page(
            &self.codec,
            &metadata.header,
            metadata.page_sha256,
            &payload,
            &metadata.projections,
        )?;
        Ok(Some(ForecastCycleDiskSlotPage {
            header: metadata.header,
            page_sha256: metadata.page_sha256,
            payload,
            projections,
        }))
    }

    // derive one fixed current or next slot path
    fn slot_path(&self, slot: ForecastCyclePageSlotKind) -> PathBuf {
        self.root
            .join(ONLINE_DIRECTORY)
            .join(SLOT_DIRECTORY)
            .join(format!("{}.page", slot.wire()))
    }
}

impl<Codec, Statfs> ForecastCyclePagePersistence for NativeForecastCycleDiskSlots<Codec, Statfs>
where
    Codec: ForecastCycleDiskSlotCodec,
    Statfs: ForecastAllocationStatfsPort,
{
    type Error = ForecastCycleDiskSlotError;

    /// Admit and durably publish one exact immutable slot frame.
    fn persist_page(
        &mut self,
        request: ForecastCyclePagePersistenceRequest<'_>,
    ) -> Result<ForecastCyclePageDurabilityProof, Self::Error> {
        use ForecastCycleDiskSlotError as Error;

        let projections = validate_page(
            &self.codec,
            request.header,
            request.page_sha256,
            request.payload,
            request.projections,
        )?;
        let metadata = self
            .codec
            .encode_disk_slot_metadata(&ForecastCycleDiskSlotMetadata {
                header: request.header.clone(),
                page_sha256: request.page_sha256,
                projections,
            })
            .ok_or(Error::Codec)?;

        // preserve the source metadata prefix bound before framing it
        if !(2..=METADATA_MAXIMUM_BYTES).contains(&metadata.len()) {
            return Err(Error::MetadataTooLarge);
        }
        let metadata_length = u32::try_from(metadata.len()).map_err(|_| Error::MetadataTooLarge)?;
        let total = 4_usize
            .checked_add(metadata.len())
            .and_then(|length| length.checked_add(request.payload.len()))
            .ok_or(Error::SlotTooLarge)?;

        // retain the complete fixed slot ceiling before allocating bytes
        if total > SLOT_MAXIMUM_BYTES {
            return Err(Error::SlotTooLarge);
        }
        self.admit(total)?;
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(&metadata_length.to_be_bytes());
        bytes.extend_from_slice(&metadata);
        bytes.extend_from_slice(request.payload);
        let path = self.slot_path(request.slot);

        // preserve one already durable byte-identical slot without rewriting it
        if let Some(existing) =
            read_optional_file(&path, SLOT_MAXIMUM_BYTES).map_err(|_| Error::Storage)?
        {
            if existing != bytes {
                return Err(Error::Collision);
            }
            return Ok(ForecastCyclePageDurabilityProof {
                fsynced: true,
                page_sha256: request.page_sha256,
            });
        }
        let unique = super::uuid_from_linux_entropy().map_err(|_| Error::Storage)?;
        let temporary = path.with_file_name(format!(".capture-{unique}.tmp"));
        match publish_immutable(&path, &temporary, &bytes) {
            Ok(()) => {}
            Err(error)
                if error.source.kind() == io::ErrorKind::AlreadyExists
                    && !error.temporary_cleanup_failed =>
            {
                let existing = read_optional_file(&path, SLOT_MAXIMUM_BYTES)
                    .map_err(|_| Error::Storage)?
                    .ok_or(Error::Storage)?;

                // converge only a byte-identical concurrent immutable writer
                if existing != bytes {
                    return Err(Error::Collision);
                }
            }
            Err(_) => return Err(Error::Storage),
        }
        Ok(ForecastCyclePageDurabilityProof {
            fsynced: true,
            page_sha256: request.page_sha256,
        })
    }
}

impl<Codec, Statfs: ForecastAllocationStatfsPort> NativeForecastCycleDiskSlots<Codec, Statfs> {
    // apply the exact Node protected-free-space and one-inode reservation
    pub(super) fn admit(&mut self, byte_length: usize) -> Result<(), ForecastCycleDiskSlotError> {
        use ForecastCycleDiskSlotError as Error;

        let sample = self
            .statfs
            .approved_sample(&self.root)
            .map_err(|_| Error::CapacityUnavailable)?;

        // reject values that the source JavaScript integer boundary cannot represent
        if sample.block_size == 0
            || sample.block_size > SAFE_JS_INTEGER_MAXIMUM
            || sample.free_inodes > SAFE_JS_INTEGER_MAXIMUM
        {
            return Err(Error::CapacityRefused);
        }
        let free_bytes = sample
            .available_blocks
            .checked_mul(sample.block_size)
            .ok_or(Error::CapacityRefused)?;
        let byte_length = u64::try_from(byte_length).map_err(|_| Error::CapacityRefused)?;
        let prospective_bytes = byte_length
            .checked_add(sample.block_size - 1)
            .and_then(|rounded| rounded.checked_div(sample.block_size))
            .and_then(|blocks| blocks.checked_mul(sample.block_size))
            .ok_or(Error::CapacityRefused)?;
        let required_free_bytes = FORECAST_ALLOCATION_REQUIRED_FREE_BYTES
            .checked_add(prospective_bytes)
            .ok_or(Error::CapacityRefused)?;
        let required_free_inodes = FORECAST_ALLOCATION_FREE_INODE_FLOOR
            .checked_add(1)
            .ok_or(Error::CapacityRefused)?;

        // preserve both the exact protected reservation and post-write inode floor
        if free_bytes > SAFE_JS_INTEGER_MAXIMUM
            || free_bytes < required_free_bytes
            || sample.free_inodes < required_free_inodes
        {
            return Err(Error::CapacityRefused);
        }
        Ok(())
    }
}

// validate hashes, predecessor shape, bounds, and deterministic projection order
fn validate_page(
    codec: &impl ForecastCycleDiskSlotCodec,
    header: &ForecastCyclePageHeader,
    page_sha256: ForecastCycleSha256,
    payload: &[u8],
    projections: &[ForecastCycleProjection],
) -> Result<Vec<ForecastCycleProjection>, ForecastCycleDiskSlotError> {
    use ForecastCycleDiskSlotError as Error;

    // retain the source header and payload bounds before hashing any bytes
    if header.contract_version != FORECAST_CYCLE_PAGE_CONTRACT_VERSION
        || header.page_index >= FORECAST_CYCLE_MAXIMUM_PAGES
        || header.payload_offset > SAFE_JS_INTEGER_MAXIMUM
        || payload.is_empty()
        || payload.len() > FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES
        || projections.len() > FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS
        || (header.page_index == 0
            && (header.predecessor_page_sha256.is_some() || header.payload_offset != 0))
        || (header.page_index > 0 && header.predecessor_page_sha256.is_none())
    {
        return Err(Error::InvalidPage);
    }
    let payload_sha256 = ForecastCycleSha256::admitted(codec.sha256(payload));
    let header_bytes = codec.encode_page_header(header).ok_or(Error::Codec)?;
    let calculated_page_sha256 =
        ForecastCycleSha256::admitted(codec.sha256_pair(&header_bytes, payload));

    // bind both exact source digests before exposing or persisting the page
    if payload_sha256 != header.payload_sha256 || calculated_page_sha256 != page_sha256 {
        return Err(Error::InvalidPage);
    }
    let mut normalized = projections.to_vec();
    normalized.sort_by_key(|projection| projection.identity_sha256);

    // reject a duplicate immutable projection identity after source ordering
    if normalized
        .windows(2)
        .any(|pair| pair[0].identity_sha256 == pair[1].identity_sha256)
    {
        return Err(Error::InvalidPage);
    }
    Ok(normalized)
}
