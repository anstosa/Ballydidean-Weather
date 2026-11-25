//! Native immutable producer-input seals for acknowledged forecast cycles.

use weather_contracts::ForecastArchiveInputSealV1ContractVersionLiteral;

use std::io;
use std::path::{Path, PathBuf};

use crate::forecast_cycle_pages::{
    FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS, ForecastCycleSha256, valid_due_key,
};
use crate::native_revision_spool_file::{
    ensure_child_directory, publish_immutable, read_optional_file, require_directory,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant, lower_hex};

use super::forecast_allocation::{
    FORECAST_ALLOCATION_FREE_INODE_FLOOR, FORECAST_ALLOCATION_REQUIRED_FREE_BYTES,
    ForecastAllocationStatfsPort,
};

/// Exact immutable input-seal contract retained by the Node owner.
pub const FORECAST_CYCLE_INPUT_SEAL_CONTRACT_VERSION: &str =
    ForecastArchiveInputSealV1ContractVersionLiteral::WIRE_VALUE;

const ONLINE_DIRECTORY: &str = "online-pages";
const INPUT_SEAL_DIRECTORY: &str = "input-seals";
const INPUT_SEAL_MAXIMUM_BYTES: usize = 2_048;
const REQUIRED_INPUT_MAXIMUM_COUNT: usize = 8_192;
const SAFE_JS_INTEGER_MAXIMUM: u64 = 9_007_199_254_740_991;

/// One declared producer population root in the closed input set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForecastCycleRequiredInputRoot {
    pub count: usize,
    pub root_sha256: ForecastCycleSha256,
}

/// Every mandatory producer class retained by the source contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForecastCycleRequiredInputs {
    pub actual_best_match: ForecastCycleRequiredInputRoot,
    pub native_source: ForecastCycleRequiredInputRoot,
    pub rain_gate_input: ForecastCycleRequiredInputRoot,
    pub serving_evidence: ForecastCycleRequiredInputRoot,
    pub shadow_body: ForecastCycleRequiredInputRoot,
    pub shadow_metadata: ForecastCycleRequiredInputRoot,
    pub target_revision: ForecastCycleRequiredInputRoot,
}

impl ForecastCycleRequiredInputs {
    // retain the exact seven source populations without allocating or changing order
    fn entries(&self) -> [&ForecastCycleRequiredInputRoot; 7] {
        [
            &self.actual_best_match,
            &self.native_source,
            &self.rain_gate_input,
            &self.serving_evidence,
            &self.shadow_body,
            &self.shadow_metadata,
            &self.target_revision,
        ]
    }
}

/// One complete immutable closure over acknowledged pages, gaps, and producer roots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastCycleInputSeal {
    pub acknowledged_projection_count: usize,
    pub acknowledged_projection_root_sha256: ForecastCycleSha256,
    pub acknowledgement_root_sha256: ForecastCycleSha256,
    pub contract_version: &'static str,
    pub due_key: String,
    pub evidence_gap_root_sha256: ForecastCycleSha256,
    pub generation: u64,
    pub page_root_sha256: ForecastCycleSha256,
    pub required_inputs: ForecastCycleRequiredInputs,
    pub sealed_at: String,
}

/// Existing canonical JSON owner for exact input-seal and identity documents.
pub trait ForecastCycleInputSealCodec: RevisionArchiveCanonicalCodec {
    /// Encode one complete canonical newline-framed input seal.
    fn encode_input_seal(&self, seal: &ForecastCycleInputSeal) -> Option<Vec<u8>>;

    /// Decode only original canonical JSON-plus-newline bytes with closed source shapes.
    fn decode_input_seal(&self, canonical_bytes: &[u8]) -> Option<ForecastCycleInputSeal>;

    /// Encode canonical `{ dueKey, generation }` identity bytes with decimal-string generation and newline.
    fn encode_input_seal_identity(&self, due_key: &str, generation: u64) -> Option<Vec<u8>>;

    /// Encode the canonical empty population used by every zero-count producer root.
    fn encode_empty_input_population(&self) -> Option<Vec<u8>>;
}

/// Exact fsync proof returned after the immutable producer closure is durable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForecastCycleInputSealDurabilityProof {
    pub fsynced: bool,
}

/// Ordinary seal validation, codec, capacity, collision, and storage failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastCycleInputSealError {
    CapacityRefused,
    CapacityUnavailable,
    Codec,
    Collision,
    InvalidIdentity,
    InvalidSeal,
    Storage,
    TooLarge,
}

/// Native bounded input-seal reader and immutable writer.
pub struct NativeForecastCycleInputSeals<Codec, Statfs> {
    root: PathBuf,
    codec: Codec,
    statfs: Statfs,
}

impl<Codec, Statfs> NativeForecastCycleInputSeals<Codec, Statfs> {
    /// Admit one private root and create only its fixed input-seal directory chain.
    pub fn initialize(root: PathBuf, codec: Codec, statfs: Statfs) -> io::Result<Self> {
        require_directory(&root)?;
        let online = root.join(ONLINE_DIRECTORY);
        ensure_child_directory(&online)?;
        ensure_child_directory(&online.join(INPUT_SEAL_DIRECTORY))?;
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

    /// Recover every injected owner after input-seal processing.
    pub fn into_parts(self) -> (PathBuf, Codec, Statfs) {
        (self.root, self.codec, self.statfs)
    }
}

impl<Codec: ForecastCycleInputSealCodec, Statfs> NativeForecastCycleInputSeals<Codec, Statfs> {
    /// Read only the canonical seal for one exact due-key and generation identity.
    pub fn read_input_seal(
        &self,
        due_key: &str,
        generation: u64,
    ) -> Result<Option<ForecastCycleInputSeal>, ForecastCycleInputSealError> {
        use ForecastCycleInputSealError as Error;

        let identity =
            seal_identity(&self.codec, due_key, generation).ok_or(Error::InvalidIdentity)?;
        let Some(bytes) =
            read_optional_file(&self.input_seal_path(identity), INPUT_SEAL_MAXIMUM_BYTES)
                .map_err(|_| Error::Storage)?
        else {
            return Ok(None);
        };
        let seal = self.codec.decode_input_seal(&bytes).ok_or(Error::Codec)?;
        let valid = valid_input_seal(&seal, &self.codec).ok_or(Error::Codec)?;

        // bind the canonical document to the due-key and generation that selected its path
        if !valid || seal.due_key != due_key || seal.generation != generation {
            return Err(Error::InvalidSeal);
        }
        Ok(Some(seal))
    }

    // derive one exact due-and-generation-addressed seal path
    fn input_seal_path(&self, identity: ForecastCycleSha256) -> PathBuf {
        self.input_seal_directory()
            .join(format!("sha256-{}.json", lower_hex(*identity.as_bytes())))
    }

    // derive the fixed producer-seal directory
    fn input_seal_directory(&self) -> PathBuf {
        self.root.join(ONLINE_DIRECTORY).join(INPUT_SEAL_DIRECTORY)
    }
}

impl<Codec, Statfs> NativeForecastCycleInputSeals<Codec, Statfs>
where
    Codec: ForecastCycleInputSealCodec,
    Statfs: ForecastAllocationStatfsPort,
{
    /// Publish or verify one immutable canonical input seal.
    pub fn persist_input_seal(
        &mut self,
        seal: &ForecastCycleInputSeal,
    ) -> Result<ForecastCycleInputSealDurabilityProof, ForecastCycleInputSealError> {
        use ForecastCycleInputSealError as Error;

        let valid = valid_input_seal(seal, &self.codec).ok_or(Error::Codec)?;
        // reject invalid typed metadata before canonical encoding or capacity admission
        if !valid {
            return Err(Error::InvalidSeal);
        }
        let bytes = self.codec.encode_input_seal(seal).ok_or(Error::Codec)?;
        // retain the source's independent canonical input-seal byte ceiling
        if bytes.len() > INPUT_SEAL_MAXIMUM_BYTES {
            return Err(Error::TooLarge);
        }
        let identity = seal_identity(&self.codec, &seal.due_key, seal.generation)
            .ok_or(Error::InvalidIdentity)?;
        let existing = self.read_input_seal(&seal.due_key, seal.generation)?;
        // charge one filesystem block and inode only when no seal is already durable
        if existing.is_none() {
            admit(&mut self.statfs, &self.root, bytes.len())?;
        }
        publish_or_verify(&self.input_seal_path(identity), &bytes)?;
        Ok(ForecastCycleInputSealDurabilityProof { fsynced: true })
    }
}

// validate every typed input population and cross-class source invariant
pub(super) fn valid_input_seal(
    seal: &ForecastCycleInputSeal,
    codec: &impl ForecastCycleInputSealCodec,
) -> Option<bool> {
    let empty_bytes = codec.encode_empty_input_population()?;
    let empty_root = ForecastCycleSha256::admitted(codec.sha256(&empty_bytes));
    // preserve every source scalar bound before inspecting mandatory populations
    if seal.contract_version != FORECAST_CYCLE_INPUT_SEAL_CONTRACT_VERSION
        || !valid_due_key(&seal.due_key)
        || instant(codec, &seal.sealed_at).is_none()
        || !(1..=FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS)
            .contains(&seal.acknowledged_projection_count)
        || seal.required_inputs.serving_evidence.count == 0
        || seal.required_inputs.shadow_body.count != seal.required_inputs.shadow_metadata.count
    {
        return Some(false);
    }
    // require every mandatory population and the exact canonical empty root for zero counts
    for entry in seal.required_inputs.entries() {
        if entry.count > REQUIRED_INPUT_MAXIMUM_COUNT
            || (entry.count == 0 && entry.root_sha256 != empty_root)
        {
            return Some(false);
        }
    }
    Some(true)
}

// hash the exact canonical due-key and generation identity document
fn seal_identity(
    codec: &impl ForecastCycleInputSealCodec,
    due_key: &str,
    generation: u64,
) -> Option<ForecastCycleSha256> {
    // reject invalid path inputs before asking the canonical owner to encode them
    if !valid_due_key(due_key) {
        return None;
    }
    let bytes = codec.encode_input_seal_identity(due_key, generation)?;
    Some(ForecastCycleSha256::admitted(codec.sha256(&bytes)))
}

// publish one seal or converge only a byte-identical first-writer retry
fn publish_or_verify(path: &Path, bytes: &[u8]) -> Result<(), ForecastCycleInputSealError> {
    use ForecastCycleInputSealError as Error;

    // preserve one already durable exact producer closure without rewriting it
    if let Some(existing) =
        read_optional_file(path, INPUT_SEAL_MAXIMUM_BYTES).map_err(|_| Error::Storage)?
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
            let retained = read_optional_file(path, INPUT_SEAL_MAXIMUM_BYTES)
                .map_err(|_| Error::Storage)?
                .ok_or(Error::Storage)?;

            // accept only the same canonical first-writer seal after a concurrent retry
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
) -> Result<(), ForecastCycleInputSealError> {
    use ForecastCycleInputSealError as Error;

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

    // preserve both post-write Node floors before creating a producer seal
    if free_bytes > SAFE_JS_INTEGER_MAXIMUM
        || free_bytes < required_free_bytes
        || sample.free_inodes < required_free_inodes
    {
        return Err(Error::CapacityRefused);
    }
    Ok(())
}
