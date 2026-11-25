//! Native immutable cold-graph acknowledgements for finalized forecast cycles.

use weather_contracts::ForecastArchiveFinalAcknowledgementV1ContractVersionLiteral;

use std::io;
use std::path::{Path, PathBuf};

use crate::forecast_cycle_pages::ForecastCycleSha256;
use crate::native_revision_spool_file::{
    ensure_child_directory, publish_immutable, read_optional_file, require_directory,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant, lower_hex};

use super::forecast_allocation::{
    FORECAST_ALLOCATION_FREE_INODE_FLOOR, FORECAST_ALLOCATION_REQUIRED_FREE_BYTES,
    ForecastAllocationStatfsPort,
};

/// Exact first cold-graph acknowledgement contract retained by the Node owner.
pub const FORECAST_CYCLE_FINAL_ACK_CONTRACT_VERSION: &str =
    ForecastArchiveFinalAcknowledgementV1ContractVersionLiteral::WIRE_VALUE;

const ONLINE_DIRECTORY: &str = "online-pages";
const FINAL_ACK_DIRECTORY: &str = "final-acknowledgements";
const FINAL_ACK_MAXIMUM_BYTES: usize = 2_048;
const SAFE_JS_INTEGER_MAXIMUM: u64 = 9_007_199_254_740_991;

/// One first-writer binding from a final manifest to its verified cold graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastCycleFinalAcknowledgement {
    pub acknowledged_at: String,
    pub contract_version: &'static str,
    pub graph_sha256: ForecastCycleSha256,
    pub manifest_sha256: ForecastCycleSha256,
}

/// Existing canonical JSON owner for exact final acknowledgement documents.
pub trait ForecastCycleFinalAckCodec: RevisionArchiveCanonicalCodec {
    /// Encode one complete canonical newline-framed final acknowledgement.
    fn encode_final_acknowledgement(
        &self,
        acknowledgement: &ForecastCycleFinalAcknowledgement,
    ) -> Option<Vec<u8>>;

    /// Decode only original canonical JSON-plus-newline bytes with the closed four-field shape.
    fn decode_final_acknowledgement(
        &self,
        canonical_bytes: &[u8],
    ) -> Option<ForecastCycleFinalAcknowledgement>;
}

/// Exact fsync proof returned after the first cold-graph binding is durable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForecastCycleFinalAckDurabilityProof {
    pub fsynced: bool,
    pub manifest_sha256: ForecastCycleSha256,
}

/// Ordinary final acknowledgement validation, capacity, collision, and storage failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastCycleFinalAckError {
    CapacityRefused,
    CapacityUnavailable,
    Codec,
    Collision,
    InvalidAcknowledgement,
    Storage,
    TooLarge,
}

/// Native bounded final-acknowledgement reader and immutable writer.
pub struct NativeForecastCycleFinalAcknowledgements<Codec, Statfs> {
    root: PathBuf,
    codec: Codec,
    statfs: Statfs,
}

impl<Codec, Statfs> NativeForecastCycleFinalAcknowledgements<Codec, Statfs> {
    /// Admit one private root and create only its fixed final-acknowledgement directory chain.
    pub fn initialize(root: PathBuf, codec: Codec, statfs: Statfs) -> io::Result<Self> {
        require_directory(&root)?;
        let online = root.join(ONLINE_DIRECTORY);
        ensure_child_directory(&online)?;
        ensure_child_directory(&online.join(FINAL_ACK_DIRECTORY))?;
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

    /// Recover every injected owner after final-acknowledgement processing.
    pub fn into_parts(self) -> (PathBuf, Codec, Statfs) {
        (self.root, self.codec, self.statfs)
    }
}

impl<Codec: ForecastCycleFinalAckCodec, Statfs>
    NativeForecastCycleFinalAcknowledgements<Codec, Statfs>
{
    /// Read the first canonical cold-graph binding for one exact final manifest.
    pub fn read_final_acknowledgement(
        &self,
        manifest_sha256: ForecastCycleSha256,
    ) -> Result<Option<ForecastCycleFinalAcknowledgement>, ForecastCycleFinalAckError> {
        use ForecastCycleFinalAckError as Error;

        let Some(bytes) = read_optional_file(
            &self.final_ack_path(manifest_sha256),
            FINAL_ACK_MAXIMUM_BYTES,
        )
        .map_err(|_| Error::Storage)?
        else {
            return Ok(None);
        };
        let acknowledgement = self
            .codec
            .decode_final_acknowledgement(&bytes)
            .ok_or(Error::Codec)?;

        // bind the canonical receipt's manifest identity to the name used for lookup
        if acknowledgement.manifest_sha256 != manifest_sha256
            || !valid_acknowledgement(&acknowledgement, &self.codec)
        {
            return Err(Error::InvalidAcknowledgement);
        }
        Ok(Some(acknowledgement))
    }

    // derive one exact manifest-addressed acknowledgement path
    fn final_ack_path(&self, manifest_sha256: ForecastCycleSha256) -> PathBuf {
        self.final_ack_directory().join(format!(
            "sha256-{}.json",
            lower_hex(*manifest_sha256.as_bytes())
        ))
    }

    // derive the fixed final cold-graph acknowledgement directory
    fn final_ack_directory(&self) -> PathBuf {
        self.root.join(ONLINE_DIRECTORY).join(FINAL_ACK_DIRECTORY)
    }
}

impl<Codec, Statfs> NativeForecastCycleFinalAcknowledgements<Codec, Statfs>
where
    Codec: ForecastCycleFinalAckCodec,
    Statfs: ForecastAllocationStatfsPort,
{
    /// Publish or verify one immutable canonical cold-graph acknowledgement.
    pub fn persist_final_acknowledgement(
        &mut self,
        acknowledgement: &ForecastCycleFinalAcknowledgement,
    ) -> Result<ForecastCycleFinalAckDurabilityProof, ForecastCycleFinalAckError> {
        use ForecastCycleFinalAckError as Error;

        // reject invalid typed metadata before canonical encoding or capacity admission
        if !valid_acknowledgement(acknowledgement, &self.codec) {
            return Err(Error::InvalidAcknowledgement);
        }
        let bytes = self
            .codec
            .encode_final_acknowledgement(acknowledgement)
            .ok_or(Error::Codec)?;
        // retain the independent first-writer final acknowledgement byte ceiling
        if bytes.len() > FINAL_ACK_MAXIMUM_BYTES {
            return Err(Error::TooLarge);
        }
        admit(&mut self.statfs, &self.root, bytes.len())?;
        publish_or_verify(
            &self.final_ack_path(acknowledgement.manifest_sha256),
            &bytes,
        )?;
        Ok(ForecastCycleFinalAckDurabilityProof {
            fsynced: true,
            manifest_sha256: acknowledgement.manifest_sha256,
        })
    }
}

// validate the exact contract and canonical acknowledgement instant
fn valid_acknowledgement(
    acknowledgement: &ForecastCycleFinalAcknowledgement,
    codec: &impl RevisionArchiveCanonicalCodec,
) -> bool {
    acknowledgement.contract_version == FORECAST_CYCLE_FINAL_ACK_CONTRACT_VERSION
        && instant(codec, &acknowledgement.acknowledged_at).is_some()
}

// publish one acknowledgement or converge only a byte-identical first-writer retry
fn publish_or_verify(path: &Path, bytes: &[u8]) -> Result<(), ForecastCycleFinalAckError> {
    use ForecastCycleFinalAckError as Error;

    // preserve one already durable exact cold-graph binding without rewriting it
    if let Some(existing) =
        read_optional_file(path, FINAL_ACK_MAXIMUM_BYTES).map_err(|_| Error::Storage)?
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
            let retained = read_optional_file(path, FINAL_ACK_MAXIMUM_BYTES)
                .map_err(|_| Error::Storage)?
                .ok_or(Error::Storage)?;

            // accept only the same canonical first-writer graph after a concurrent retry
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
) -> Result<(), ForecastCycleFinalAckError> {
    use ForecastCycleFinalAckError as Error;

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

    // preserve both post-write Node floors before creating a final acknowledgement
    if free_bytes > SAFE_JS_INTEGER_MAXIMUM
        || free_bytes < required_free_bytes
        || sample.free_inodes < required_free_inodes
    {
        return Err(Error::CapacityRefused);
    }
    Ok(())
}
