//! Native immutable workstation transfer checkpoints for forecast cycle pages.

use weather_contracts::ForecastArchiveTransferAcknowledgementV1ContractVersionLiteral;

use std::io;
use std::path::{Path, PathBuf};

use crate::forecast_cycle_pages::{
    FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS, FORECAST_CYCLE_MAXIMUM_PAGES,
    FORECAST_CYCLE_PAGE_CONTRACT_VERSION, FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES,
    ForecastCyclePageHeader, ForecastCycleProjection, ForecastCycleSha256, valid_due_key,
};
use crate::native_revision_spool_file::{
    ensure_child_directory, list_names, publish_immutable, read_optional_file, require_directory,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant, lower_hex};

use super::forecast_allocation::{
    FORECAST_ALLOCATION_FREE_INODE_FLOOR, FORECAST_ALLOCATION_REQUIRED_FREE_BYTES,
    ForecastAllocationStatfsPort,
};

/// Exact immutable transfer-checkpoint contract retained by the Node owner.
pub const FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION: &str =
    ForecastArchiveTransferAcknowledgementV1ContractVersionLiteral::WIRE_VALUE;

const ONLINE_DIRECTORY: &str = "online-pages";
const CHECKPOINT_DIRECTORY: &str = "archive-acknowledgements";
const CHECKPOINT_MAXIMUM_BYTES: usize = 2_048;
const CHECKPOINT_MAXIMUM_COUNT: usize = 8_192;
const SAFE_JS_INTEGER_MAXIMUM: u64 = 9_007_199_254_740_991;

/// One first-writer workstation checkpoint for a transferred online page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastCycleTransferCheckpoint {
    pub acknowledged_at: String,
    pub checkpoint_sha256: ForecastCycleSha256,
    pub contract_version: &'static str,
    pub header: ForecastCyclePageHeader,
    pub page_sha256: ForecastCycleSha256,
    pub payload_length: usize,
    pub projections: Vec<ForecastCycleProjection>,
}

/// Existing canonical JSON owner for exact checkpoint documents.
pub trait ForecastCycleTransferCheckpointCodec: RevisionArchiveCanonicalCodec {
    /// Encode one complete canonical newline-framed checkpoint.
    fn encode_transfer_checkpoint(
        &self,
        checkpoint: &ForecastCycleTransferCheckpoint,
    ) -> Option<Vec<u8>>;

    /// Decode only original canonical JSON-plus-newline bytes with closed source object shapes.
    fn decode_transfer_checkpoint(
        &self,
        canonical_bytes: &[u8],
    ) -> Option<ForecastCycleTransferCheckpoint>;
}

/// Exact fsync receipt returned after an immutable checkpoint is durable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForecastCycleTransferCheckpointDurabilityProof {
    pub fsynced: bool,
    pub page_sha256: ForecastCycleSha256,
}

/// Ordinary checkpoint capacity, census, canonical, collision, and storage failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastCycleTransferCheckpointError {
    CapacityRefused,
    CapacityUnavailable,
    Census,
    Codec,
    Collision,
    InvalidCheckpoint,
    Storage,
    TooLarge,
}

/// Native bounded archive-checkpoint reader and immutable writer.
pub struct NativeForecastCycleTransferCheckpoints<Codec, Statfs> {
    root: PathBuf,
    codec: Codec,
    statfs: Statfs,
}

impl<Codec, Statfs> NativeForecastCycleTransferCheckpoints<Codec, Statfs> {
    /// Admit one private root and create only its fixed checkpoint directory chain.
    pub fn initialize(root: PathBuf, codec: Codec, statfs: Statfs) -> io::Result<Self> {
        require_directory(&root)?;
        let online = root.join(ONLINE_DIRECTORY);
        ensure_child_directory(&online)?;
        ensure_child_directory(&online.join(CHECKPOINT_DIRECTORY))?;
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

    /// Recover every injected owner after checkpoint processing.
    pub fn into_parts(self) -> (PathBuf, Codec, Statfs) {
        (self.root, self.codec, self.statfs)
    }
}

impl<Codec: ForecastCycleTransferCheckpointCodec, Statfs>
    NativeForecastCycleTransferCheckpoints<Codec, Statfs>
{
    /// Read one first-writer checkpoint by its exact transferred page identity.
    pub fn read_transfer_acknowledgement(
        &self,
        page_sha256: ForecastCycleSha256,
    ) -> Result<Option<ForecastCycleTransferCheckpoint>, ForecastCycleTransferCheckpointError> {
        use ForecastCycleTransferCheckpointError as Error;

        let Some(bytes) =
            read_optional_file(&self.checkpoint_path(page_sha256), CHECKPOINT_MAXIMUM_BYTES)
                .map_err(|_| Error::Storage)?
        else {
            return Ok(None);
        };
        let checkpoint = self
            .codec
            .decode_transfer_checkpoint(&bytes)
            .ok_or(Error::Codec)?;

        // bind the first-writer file name to the page inside the canonical receipt
        if checkpoint.page_sha256 != page_sha256 || !valid_checkpoint(&checkpoint, &self.codec) {
            return Err(Error::InvalidCheckpoint);
        }
        Ok(Some(checkpoint))
    }

    /// Enumerate the complete finite checkpoint history in source filename order.
    pub fn list_transfer_acknowledgements(
        &self,
    ) -> Result<Vec<ForecastCycleTransferCheckpoint>, ForecastCycleTransferCheckpointError> {
        use ForecastCycleTransferCheckpointError as Error;

        let directory = self.checkpoint_directory();
        let mut names = list_names(&directory, Some(CHECKPOINT_MAXIMUM_COUNT + 1))
            .map_err(|_| Error::Storage)?;

        // refuse an unexpected lifetime checkpoint spool before parsing members
        if names.len() > CHECKPOINT_MAXIMUM_COUNT {
            return Err(Error::Census);
        }
        names.sort();
        let mut checkpoints = Vec::with_capacity(names.len());

        // validate every exact content-addressed filename before restart reconstruction
        for name in names {
            let page_sha256 = digest_from_name(&name).ok_or(Error::Census)?;
            let checkpoint = self
                .read_transfer_acknowledgement(page_sha256)?
                .ok_or(Error::Census)?;
            checkpoints.push(checkpoint);
        }
        Ok(checkpoints)
    }

    // derive one exact page-addressed checkpoint path
    fn checkpoint_path(&self, page_sha256: ForecastCycleSha256) -> PathBuf {
        self.checkpoint_directory().join(format!(
            "sha256-{}.json",
            lower_hex(*page_sha256.as_bytes())
        ))
    }

    // derive the one fixed checkpoint directory
    fn checkpoint_directory(&self) -> PathBuf {
        self.root.join(ONLINE_DIRECTORY).join(CHECKPOINT_DIRECTORY)
    }
}

impl<Codec, Statfs> NativeForecastCycleTransferCheckpoints<Codec, Statfs>
where
    Codec: ForecastCycleTransferCheckpointCodec,
    Statfs: ForecastAllocationStatfsPort,
{
    /// Publish or verify one immutable canonical transfer checkpoint.
    pub fn persist_transfer_acknowledgement(
        &mut self,
        checkpoint: &ForecastCycleTransferCheckpoint,
    ) -> Result<ForecastCycleTransferCheckpointDurabilityProof, ForecastCycleTransferCheckpointError>
    {
        use ForecastCycleTransferCheckpointError as Error;

        // reject invalid typed metadata before canonical encoding or capacity admission
        if !valid_checkpoint(checkpoint, &self.codec) {
            return Err(Error::InvalidCheckpoint);
        }
        let bytes = self
            .codec
            .encode_transfer_checkpoint(checkpoint)
            .ok_or(Error::Codec)?;

        // retain the independent first-writer checkpoint byte ceiling
        if bytes.len() > CHECKPOINT_MAXIMUM_BYTES {
            return Err(Error::TooLarge);
        }
        admit(&mut self.statfs, &self.root, bytes.len())?;
        publish_or_verify(&self.checkpoint_path(checkpoint.page_sha256), &bytes)?;
        Ok(ForecastCycleTransferCheckpointDurabilityProof {
            fsynced: true,
            page_sha256: checkpoint.page_sha256,
        })
    }
}

// validate exact typed page bounds, predecessor shape, projections, and acknowledgement clock
fn valid_checkpoint(
    checkpoint: &ForecastCycleTransferCheckpoint,
    codec: &impl RevisionArchiveCanonicalCodec,
) -> bool {
    let header = &checkpoint.header;

    // preserve every source scalar bound before inspecting projection identities
    if checkpoint.contract_version != FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION
        || header.contract_version != FORECAST_CYCLE_PAGE_CONTRACT_VERSION
        || !valid_due_key(&header.due_key)
        || header.page_index >= FORECAST_CYCLE_MAXIMUM_PAGES
        || header.payload_offset > SAFE_JS_INTEGER_MAXIMUM
        || checkpoint.payload_length == 0
        || checkpoint.payload_length > FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES
        || checkpoint.projections.is_empty()
        || checkpoint.projections.len() > FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS
        || instant(codec, &checkpoint.acknowledged_at).is_none()
        || (header.page_index == 0
            && (header.predecessor_page_sha256.is_some() || header.payload_offset != 0))
        || (header.page_index > 0 && header.predecessor_page_sha256.is_none())
    {
        return false;
    }
    let mut identities = checkpoint
        .projections
        .iter()
        .map(|projection| projection.identity_sha256)
        .collect::<Vec<_>>();
    identities.sort();

    // reject one duplicate stable projection identity without changing source order
    !identities.windows(2).any(|pair| pair[0] == pair[1])
}

// decode one exact lowercase page-addressed checkpoint filename
fn digest_from_name(name: &str) -> Option<ForecastCycleSha256> {
    let digest = name.strip_prefix("sha256-")?.strip_suffix(".json")?;
    let digest = crate::RevisionGapSha256::from_lower_hex(digest)?;
    Some(ForecastCycleSha256::admitted(*digest.as_bytes()))
}

// publish one checkpoint or converge only a byte-identical first-writer retry
fn publish_or_verify(
    path: &Path,
    bytes: &[u8],
) -> Result<(), ForecastCycleTransferCheckpointError> {
    use ForecastCycleTransferCheckpointError as Error;

    // preserve one already durable exact receipt without rewriting it
    if let Some(existing) =
        read_optional_file(path, CHECKPOINT_MAXIMUM_BYTES).map_err(|_| Error::Storage)?
    {
        return if existing == bytes {
            Ok(())
        } else {
            Err(Error::Collision)
        };
    }
    let unique = super::uuid_from_linux_entropy().map_err(|_| Error::Storage)?;
    let temporary = path.with_file_name(format!(".capture-{unique}.tmp"));
    match publish_immutable(path, &temporary, bytes) {
        Ok(()) => Ok(()),
        Err(error)
            if error.source.kind() == io::ErrorKind::AlreadyExists
                && !error.temporary_cleanup_failed =>
        {
            let retained = read_optional_file(path, CHECKPOINT_MAXIMUM_BYTES)
                .map_err(|_| Error::Storage)?
                .ok_or(Error::Storage)?;

            // accept only the same canonical first-writer checkpoint
            if retained == bytes {
                Ok(())
            } else {
                Err(Error::Collision)
            }
        }
        Err(_) => Err(Error::Storage),
    }
}

// apply the exact protected free-space and one-inode reservation before publication
fn admit(
    statfs: &mut impl ForecastAllocationStatfsPort,
    root: &Path,
    byte_length: usize,
) -> Result<(), ForecastCycleTransferCheckpointError> {
    use ForecastCycleTransferCheckpointError as Error;

    let sample = statfs
        .approved_sample(root)
        .map_err(|_| Error::CapacityUnavailable)?;

    // reject quantities outside the source JavaScript safe-integer boundary
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

    // preserve both post-write Node floors before creating a checkpoint
    if free_bytes > SAFE_JS_INTEGER_MAXIMUM
        || free_bytes < required_free_bytes
        || sample.free_inodes < required_free_inodes
    {
        return Err(Error::CapacityRefused);
    }
    Ok(())
}
