//! Native forecast-evidence allocation census and admission.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::native_revision_spool_file::{invalid, require_directory};

use super::forecast_channels::{
    ForecastChannelAllocationDecision, ForecastChannelAllocationPort,
    ForecastChannelAllocationRequest, ForecastChannelPublication, ForecastChannelPublicationKind,
};
use super::{
    ForecastObjectAllocationDecision, ForecastObjectAllocationPort,
    ForecastObjectAllocationRequest, ForecastObjectPublication, ForecastObjectPublicationKind,
    ForecastObjectSha256,
};

pub const FORECAST_ALLOCATION_MAXIMUM_BYTES: u64 = 64 * 1_024 * 1_024;
pub const FORECAST_ALLOCATION_MAXIMUM_IDENTITIES: usize = 8_192;
pub const FORECAST_ALLOCATION_MAXIMUM_CHANNEL_ASSERTIONS: usize = 256;
pub const FORECAST_ALLOCATION_REQUIRED_FREE_BYTES: u64 = 2_034_155_520;
pub const FORECAST_ALLOCATION_FREE_INODE_FLOOR: u64 = 32_768;

const OBJECT_DIRECTORY: &str = "objects";
const RECEIPT_DIRECTORY: &str = "receipts";
const CHANNEL_DIRECTORY: &str = "channels";
const CHANNEL_BINDING_DIRECTORY: &str = "channel-bindings";
const OBJECT_MAXIMUM_BYTES: u64 = 16 * 1_024;
const METADATA_MAXIMUM_BYTES: u64 = 16 * 1_024;
const ALLOCATED_BLOCK_BYTES: u64 = 512;
const TEMPORARY_RETRY_COUNT: usize = 200;
const TEMPORARY_RETRY: Duration = Duration::from_millis(10);

/// One approved filesystem-capacity sample supplied outside native storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForecastAllocationStatfsSample {
    pub available_blocks: u64,
    pub block_size: u64,
    pub free_inodes: u64,
}

/// Approved statfs boundary without a syscall implementation in this crate.
pub trait ForecastAllocationStatfsPort {
    /// Return one current sample for the already-admitted forecast evidence root.
    fn approved_sample(&mut self, root: &Path) -> io::Result<ForecastAllocationStatfsSample>;
}

/// Exact bounded retained allocation projected from the four Node directories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForecastAllocationCensus {
    pub allocated_bytes: u64,
    pub channel_assertion_count: usize,
    pub channel_binding_count: usize,
    pub object_count: usize,
    pub receipt_count: usize,
}

/// Exact Node status-counter class for one allocation refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastAllocationRefusal {
    Capacity,
    FreeSpace,
    Inodes,
}

#[derive(Debug)]
struct ForecastAllocationState {
    census: ForecastAllocationCensus,
    channel_assertion_identities: BTreeSet<ForecastObjectSha256>,
    channel_binding_identities: BTreeSet<ForecastObjectSha256>,
    object_identities: BTreeSet<ForecastObjectSha256>,
    receipt_identities: BTreeSet<ForecastObjectSha256>,
}

#[derive(Clone, Copy)]
enum ForecastAllocationPublicationKind {
    Object,
    Receipt,
    ChannelAssertion,
    ChannelBinding,
}

/// Native allocation owner using checked metadata and an injected statfs source.
pub struct NativeForecastObjectAllocation<Statfs> {
    root: PathBuf,
    statfs: Statfs,
    state: ForecastAllocationState,
    admitted_block_size: Option<u64>,
    last_refusal: Option<ForecastAllocationRefusal>,
}

impl<Statfs> NativeForecastObjectAllocation<Statfs> {
    /// Admit the exact four retained directories and initialize their bounded census.
    pub fn new(root: PathBuf, statfs: Statfs) -> io::Result<Self> {
        let state = scan_census(&root)?;
        Ok(Self {
            root,
            statfs,
            state,
            admitted_block_size: None,
            last_refusal: None,
        })
    }

    /// Return the current in-memory projection after scan and publication observations.
    pub const fn census(&self) -> ForecastAllocationCensus {
        self.state.census
    }

    /// Return the immediately preceding object or channel admission refusal class.
    pub const fn last_refusal(&self) -> Option<ForecastAllocationRefusal> {
        self.last_refusal
    }

    /// Rescan exact retained allocation after external or interrupted publication.
    pub fn refresh_from_disk(&mut self) -> io::Result<()> {
        self.admitted_block_size = None;
        self.last_refusal = None;
        self.state = scan_census(&self.root)?;
        Ok(())
    }

    /// Recover both injected owners after the allocation lifecycle.
    pub fn into_parts(self) -> (PathBuf, Statfs) {
        (self.root, self.statfs)
    }

    // account one created final through the shared block and identity projection
    fn observe_created(
        &mut self,
        bytes: usize,
        created: bool,
        identity_sha256: ForecastObjectSha256,
        kind: ForecastAllocationPublicationKind,
    ) {
        // ignore already-present publications and observations after refusal or failure
        if !created {
            return;
        }
        let Some(block_size) = self.admitted_block_size else {
            return;
        };
        let inserted = match kind {
            ForecastAllocationPublicationKind::Object => {
                self.state.object_identities.insert(identity_sha256)
            }
            ForecastAllocationPublicationKind::Receipt => {
                self.state.receipt_identities.insert(identity_sha256)
            }
            ForecastAllocationPublicationKind::ChannelAssertion => self
                .state
                .channel_assertion_identities
                .insert(identity_sha256),
            ForecastAllocationPublicationKind::ChannelBinding => self
                .state
                .channel_binding_identities
                .insert(identity_sha256),
        };

        // count only one created final for each immutable address
        if !inserted {
            return;
        }
        let Ok(allocation) = rounded_allocation(bytes, block_size) else {
            self.admitted_block_size = None;
            return;
        };
        self.state.census.allocated_bytes =
            self.state.census.allocated_bytes.saturating_add(allocation);
        match kind {
            ForecastAllocationPublicationKind::Object => self.state.census.object_count += 1,
            ForecastAllocationPublicationKind::Receipt => self.state.census.receipt_count += 1,
            ForecastAllocationPublicationKind::ChannelAssertion => {
                self.state.census.channel_assertion_count += 1;
            }
            ForecastAllocationPublicationKind::ChannelBinding => {
                self.state.census.channel_binding_count += 1;
            }
        }
    }
}

impl<Statfs: ForecastAllocationStatfsPort> ForecastObjectAllocationPort
    for NativeForecastObjectAllocation<Statfs>
{
    type Error = io::Error;

    /// Refresh allocation and apply the exact Node commit admission floors.
    fn admit(
        &mut self,
        request: ForecastObjectAllocationRequest,
    ) -> Result<ForecastObjectAllocationDecision, Self::Error> {
        self.refresh_from_disk()?;
        let sample = self.statfs.approved_sample(&self.root)?;

        let (decision, refusal) = allocation_admission(self.state.census, request, sample)?;
        self.last_refusal = refusal;

        // retain the admitted filesystem unit only for subsequent publications
        if decision == ForecastObjectAllocationDecision::Admit {
            self.admitted_block_size = Some(sample.block_size);
        } else {
            self.admitted_block_size = None;
        }
        Ok(decision)
    }

    /// Account each newly retained immutable publication in source order.
    fn observe(&mut self, publication: ForecastObjectPublication) {
        let kind = match publication.kind {
            ForecastObjectPublicationKind::Object => ForecastAllocationPublicationKind::Object,
            ForecastObjectPublicationKind::Receipt => ForecastAllocationPublicationKind::Receipt,
        };
        self.observe_created(
            publication.bytes,
            publication.created,
            publication.identity_sha256,
            kind,
        );
    }
}

impl<Statfs: ForecastAllocationStatfsPort> ForecastChannelAllocationPort
    for NativeForecastObjectAllocation<Statfs>
{
    type Error = io::Error;

    /// Refresh the shared census and apply channel plus global allocation authority.
    fn admit(
        &mut self,
        request: ForecastChannelAllocationRequest,
    ) -> Result<ForecastChannelAllocationDecision, Self::Error> {
        self.refresh_from_disk()?;
        let sample = self.statfs.approved_sample(&self.root)?;
        let assertion_exists = self
            .state
            .channel_assertion_identities
            .contains(&request.assertion_sha256);
        let binding_exists = self
            .state
            .channel_binding_identities
            .contains(&request.edge_receipt_identity_sha256);
        let (decision, refusal) = channel_allocation_admission(
            self.state.census,
            assertion_exists,
            binding_exists,
            request,
            sample,
        )?;
        self.last_refusal = refusal;

        // retain the admitted filesystem unit only for subsequent publications
        if decision == ForecastChannelAllocationDecision::Admit {
            self.admitted_block_size = Some(sample.block_size);
        } else {
            self.admitted_block_size = None;
        }
        Ok(decision)
    }

    /// Account each newly retained channel assertion or stable binding.
    fn observe(&mut self, publication: ForecastChannelPublication) {
        let kind = match publication.kind {
            ForecastChannelPublicationKind::Assertion => {
                ForecastAllocationPublicationKind::ChannelAssertion
            }
            ForecastChannelPublicationKind::Binding => {
                ForecastAllocationPublicationKind::ChannelBinding
            }
        };
        self.observe_created(
            publication.bytes,
            publication.created,
            publication.identity_sha256,
            kind,
        );
    }
}

// scan through only the bounded concurrent atomic-temporary window
fn scan_census(root: &Path) -> io::Result<ForecastAllocationState> {
    scan_census_with_retry(root, TEMPORARY_RETRY_COUNT, || {
        thread::sleep(TEMPORARY_RETRY)
    })
}

// project exact filenames and metadata with an injected retry action for tests
fn scan_census_with_retry(
    root: &Path,
    retries: usize,
    mut wait: impl FnMut(),
) -> io::Result<ForecastAllocationState> {
    require_directory(root)?;
    let directories = [
        (OBJECT_DIRECTORY, FORECAST_ALLOCATION_MAXIMUM_IDENTITIES),
        (RECEIPT_DIRECTORY, FORECAST_ALLOCATION_MAXIMUM_IDENTITIES),
        (
            CHANNEL_DIRECTORY,
            FORECAST_ALLOCATION_MAXIMUM_CHANNEL_ASSERTIONS,
        ),
        (
            CHANNEL_BINDING_DIRECTORY,
            FORECAST_ALLOCATION_MAXIMUM_CHANNEL_ASSERTIONS,
        ),
    ];

    // retry only when every unexpected name belongs to an atomic writer
    for attempt in 0..=retries {
        let mut names = Vec::with_capacity(directories.len());
        let mut unexpected = Vec::new();

        // read each fixed child before examining any entry metadata
        for (directory, maximum) in directories {
            let path = root.join(directory);
            require_directory(&path)?;
            let entries = read_names(&path)?;
            for name in &entries {
                // retain only the directory's exact immutable namespace
                if !valid_final_name(directory, name) {
                    unexpected.push(name.clone());
                }
            }
            names.push((directory, path, entries, maximum));
        }

        // permit only a concurrently published source-shaped temporary to settle
        if !unexpected.is_empty()
            && attempt < retries
            && unexpected.iter().all(|name| valid_temporary_name(name))
        {
            wait();
            continue;
        }

        // reject every stale temporary and every other unexpected name
        if !unexpected.is_empty() {
            return Err(invalid(
                "forecast allocation directory contains an invalid name",
            ));
        }

        // enforce source count ceilings after concurrent temporaries settle
        if names
            .iter()
            .any(|(_, _, entries, maximum)| entries.len() > *maximum)
        {
            return Err(invalid(
                "forecast allocation directory exceeds its count bound",
            ));
        }
        return project_census(names);
    }
    Err(invalid("forecast allocation retry state is invalid"))
}

// read UTF-8 basenames without accepting nested path input
fn read_names(directory: &Path) -> io::Result<Vec<String>> {
    fs::read_dir(directory)?
        .map(|entry| {
            let name = entry?.file_name();
            name.into_string()
                .map_err(|_| invalid("forecast allocation filename is not UTF-8"))
        })
        .collect()
}

// project checked final metadata into exact allocation and identity counters
fn project_census(
    directories: Vec<(&'static str, PathBuf, Vec<String>, usize)>,
) -> io::Result<ForecastAllocationState> {
    let mut allocated_bytes = 0_u64;
    let mut channel_assertion_identities = BTreeSet::new();
    let mut channel_binding_identities = BTreeSet::new();
    let mut object_identities = BTreeSet::new();
    let mut receipt_identities = BTreeSet::new();
    let mut object_count = 0;
    let mut receipt_count = 0;
    let mut channel_assertion_count = 0;
    let mut channel_binding_count = 0;

    // count Linux allocated blocks without following final-component links
    for (directory, path, names, _) in directories {
        for name in names {
            let metadata = fs::symlink_metadata(path.join(&name))?;
            let maximum_bytes = if directory == OBJECT_DIRECTORY {
                OBJECT_MAXIMUM_BYTES
            } else {
                METADATA_MAXIMUM_BYTES
            };

            // require the Node private regular-file projection
            if !metadata.file_type().is_file()
                || metadata.file_type().is_symlink()
                || metadata.mode() & 0o777 != 0o600
                || metadata.len() > maximum_bytes
            {
                return Err(invalid("forecast allocation entry metadata is invalid"));
            }
            let allocation = metadata
                .blocks()
                .checked_mul(ALLOCATED_BLOCK_BYTES)
                .ok_or_else(|| invalid("forecast allocated blocks overflowed"))?;
            allocated_bytes = allocated_bytes
                .checked_add(allocation)
                .ok_or_else(|| invalid("forecast allocation census overflowed"))?;
            let identity = identity_from_name(directory, &name)?;
            match directory {
                OBJECT_DIRECTORY => {
                    object_count += 1;
                    object_identities.insert(identity);
                }
                RECEIPT_DIRECTORY => {
                    receipt_count += 1;
                    receipt_identities.insert(identity);
                }
                CHANNEL_DIRECTORY => {
                    channel_assertion_count += 1;
                    channel_assertion_identities.insert(identity);
                }
                CHANNEL_BINDING_DIRECTORY => {
                    channel_binding_count += 1;
                    channel_binding_identities.insert(identity);
                }
                _ => return Err(invalid("forecast allocation directory is invalid")),
            }
        }
    }
    Ok(ForecastAllocationState {
        census: ForecastAllocationCensus {
            allocated_bytes,
            channel_assertion_count,
            channel_binding_count,
            object_count,
            receipt_count,
        },
        channel_assertion_identities,
        channel_binding_identities,
        object_identities,
        receipt_identities,
    })
}

// validate one exact final filename for its fixed directory
fn valid_final_name(directory: &str, name: &str) -> bool {
    let suffix = if directory == OBJECT_DIRECTORY {
        ".json.gz"
    } else {
        ".json"
    };
    digest_name(name, suffix).is_some()
}

// decode one exact final filename into its fixed digest bytes
fn identity_from_name(directory: &str, name: &str) -> io::Result<ForecastObjectSha256> {
    let suffix = if directory == OBJECT_DIRECTORY {
        ".json.gz"
    } else {
        ".json"
    };
    let digest = digest_name(name, suffix)
        .ok_or_else(|| invalid("forecast allocation identity filename is invalid"))?;
    let mut bytes = [0_u8; 32];

    // decode each exact lowercase hexadecimal pair without a codec dependency
    for (index, pair) in digest.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Ok(ForecastObjectSha256::admitted(bytes))
}

// return the exact lowercase digest slice from one final basename
fn digest_name<'a>(name: &'a str, suffix: &str) -> Option<&'a str> {
    let digest = name.strip_prefix("sha256-")?.strip_suffix(suffix)?;
    (digest.len() == 64
        && digest
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(*byte, b'a'..=b'f')))
    .then_some(digest)
}

// recognize only the source atomic temporary grammar
fn valid_temporary_name(name: &str) -> bool {
    let Some(unique) = name
        .strip_prefix(".capture-")
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };
    !unique.is_empty()
        && unique
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(*byte, b'a'..=b'f' | b'-'))
}

// decode one lowercase hexadecimal digit
fn hex_nibble(value: u8) -> io::Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(invalid("forecast allocation digest is invalid")),
    }
}

// calculate only a not-yet-existing file's rounded filesystem allocation
fn prospective_allocation(bytes: usize, exists: bool, block_size: u64) -> io::Result<u64> {
    // preserve zero prospective allocation for a retained immutable final
    if exists {
        return Ok(0);
    }
    rounded_allocation(bytes, block_size)
}

// apply the object identity and shared allocation envelope with one refusal class
fn allocation_admission(
    census: ForecastAllocationCensus,
    request: ForecastObjectAllocationRequest,
    sample: ForecastAllocationStatfsSample,
) -> io::Result<(
    ForecastObjectAllocationDecision,
    Option<ForecastAllocationRefusal>,
)> {
    // reject a sample that cannot define filesystem allocation units
    if sample.block_size == 0 {
        return Err(invalid("forecast allocation block size is invalid"));
    }
    let object_allocation = prospective_allocation(
        request.object_bytes,
        request.object_exists,
        sample.block_size,
    )?;
    let receipt_allocation = prospective_allocation(
        request.receipt_bytes,
        request.receipt_exists,
        sample.block_size,
    )?;
    let prospective_bytes = object_allocation
        .checked_add(receipt_allocation)
        .ok_or_else(|| invalid("forecast prospective allocation overflowed"))?;
    let identity_capacity = (request.object_exists
        || census.object_count < FORECAST_ALLOCATION_MAXIMUM_IDENTITIES)
        && (request.receipt_exists
            || census.receipt_count < FORECAST_ALLOCATION_MAXIMUM_IDENTITIES);
    let refusal = global_allocation_refusal(census, prospective_bytes, identity_capacity, sample)?;
    Ok(if refusal.is_none() {
        (ForecastObjectAllocationDecision::Admit, None)
    } else {
        (ForecastObjectAllocationDecision::Refuse, refusal)
    })
}

// retain the object-only decision helper used by focused boundary tests
#[cfg(test)]
fn allocation_decision(
    census: ForecastAllocationCensus,
    request: ForecastObjectAllocationRequest,
    sample: ForecastAllocationStatfsSample,
) -> io::Result<ForecastObjectAllocationDecision> {
    allocation_admission(census, request, sample).map(|(decision, _)| decision)
}

// apply per-channel census authority before the shared global envelope
fn channel_allocation_admission(
    census: ForecastAllocationCensus,
    census_assertion_exists: bool,
    binding_exists: bool,
    request: ForecastChannelAllocationRequest,
    sample: ForecastAllocationStatfsSample,
) -> io::Result<(
    ForecastChannelAllocationDecision,
    Option<ForecastAllocationRefusal>,
)> {
    // reject a sample that cannot define filesystem allocation units
    if sample.block_size == 0 {
        return Err(invalid("forecast allocation block size is invalid"));
    }
    let assertion_exists = request.assertion_exists || census_assertion_exists;
    let assertion_count = request.assertion_count.max(census.channel_assertion_count);
    let binding_count = request.binding_count.max(census.channel_binding_count);
    let assertion_allocation =
        prospective_allocation(request.assertion_bytes, assertion_exists, sample.block_size)?;
    let binding_allocation =
        prospective_allocation(request.binding_bytes, binding_exists, sample.block_size)?;
    let prospective_bytes = assertion_allocation
        .checked_add(binding_allocation)
        .ok_or_else(|| invalid("forecast channel allocation overflowed"))?;
    let channel_capacity = (binding_exists
        || binding_count < FORECAST_ALLOCATION_MAXIMUM_CHANNEL_ASSERTIONS)
        && (assertion_exists || assertion_count < FORECAST_ALLOCATION_MAXIMUM_CHANNEL_ASSERTIONS);
    let refusal = global_allocation_refusal(census, prospective_bytes, channel_capacity, sample)?;
    Ok(if refusal.is_none() {
        (ForecastChannelAllocationDecision::Admit, None)
    } else {
        (ForecastChannelAllocationDecision::Refuse, refusal)
    })
}

// classify one shared global capacity, free-space, or inode refusal in Node order
fn global_allocation_refusal(
    census: ForecastAllocationCensus,
    prospective_bytes: u64,
    source_capacity: bool,
    sample: ForecastAllocationStatfsSample,
) -> io::Result<Option<ForecastAllocationRefusal>> {
    let retained_bytes = census
        .allocated_bytes
        .checked_add(prospective_bytes)
        .ok_or_else(|| invalid("forecast retained allocation overflowed"))?;

    // classify identity and byte ceilings before filesystem reserve floors
    if !source_capacity || retained_bytes > FORECAST_ALLOCATION_MAXIMUM_BYTES {
        return Ok(Some(ForecastAllocationRefusal::Capacity));
    }
    let free_bytes = sample
        .available_blocks
        .checked_mul(sample.block_size)
        .ok_or_else(|| invalid("forecast free allocation overflowed"))?;
    let required_free_bytes = FORECAST_ALLOCATION_REQUIRED_FREE_BYTES
        .checked_add(prospective_bytes)
        .ok_or_else(|| invalid("forecast free reservation overflowed"))?;

    // classify the protected free-space reservation before inode capacity
    if free_bytes < required_free_bytes {
        return Ok(Some(ForecastAllocationRefusal::FreeSpace));
    }

    // retain Node's unadjusted shared inode floor
    if sample.free_inodes < FORECAST_ALLOCATION_FREE_INODE_FLOOR {
        return Ok(Some(ForecastAllocationRefusal::Inodes));
    }
    Ok(None)
}

// round one byte length up to the approved filesystem block size
fn rounded_allocation(bytes: usize, block_size: u64) -> io::Result<u64> {
    let bytes =
        u64::try_from(bytes).map_err(|_| invalid("forecast allocation byte length is invalid"))?;

    // preserve an empty allocation without underflowing the rounding expression
    if bytes == 0 {
        return Ok(0);
    }
    bytes
        .checked_add(block_size - 1)
        .and_then(|rounded| rounded.checked_div(block_size))
        .and_then(|blocks| blocks.checked_mul(block_size))
        .ok_or_else(|| invalid("forecast allocation rounding overflowed"))
}

#[cfg(test)]
mod tests {
    use std::fs::{DirBuilder, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static FIXTURE_ORDINAL: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone, Copy)]
    struct FixtureStatfs {
        sample: ForecastAllocationStatfsSample,
    }

    impl ForecastAllocationStatfsPort for FixtureStatfs {
        // return only the test-admitted capacity sample
        fn approved_sample(&mut self, _root: &Path) -> io::Result<ForecastAllocationStatfsSample> {
            Ok(self.sample)
        }
    }

    struct FixtureRoot {
        path: PathBuf,
    }

    impl FixtureRoot {
        // create one owner-private disposable evidence root
        fn new() -> Self {
            let ordinal = FIXTURE_ORDINAL.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "weather-forecast-allocation-{}-{ordinal}",
                std::process::id()
            ));
            private_directory(&path);

            // create only the four fixed evidence children
            for directory in [
                OBJECT_DIRECTORY,
                RECEIPT_DIRECTORY,
                CHANNEL_DIRECTORY,
                CHANNEL_BINDING_DIRECTORY,
            ] {
                private_directory(&path.join(directory));
            }
            Self { path }
        }

        // write one owner-private final fixture
        fn write(&self, directory: &str, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.path.join(directory).join(name);
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&path)
                .expect("create fixture file");
            file.write_all(bytes).expect("write fixture file");
            path
        }
    }

    impl Drop for FixtureRoot {
        // remove the complete bounded disposable fixture
        fn drop(&mut self) {
            fs::remove_dir_all(&self.path).expect("remove forecast allocation fixture");
        }
    }

    // create one exact-mode private directory
    fn private_directory(path: &Path) {
        DirBuilder::new()
            .mode(0o700)
            .create(path)
            .expect("create private fixture directory");
    }

    // return one exact lowercase digest filename
    fn final_name(directory: &str, value: u64) -> String {
        let suffix = if directory == OBJECT_DIRECTORY {
            ".json.gz"
        } else {
            ".json"
        };
        format!("sha256-{value:064x}{suffix}")
    }

    // return one sample with exact required free reservation plus allocation
    fn admitted_sample(prospective_bytes: u64) -> ForecastAllocationStatfsSample {
        let block_size = 4_096;
        ForecastAllocationStatfsSample {
            available_blocks: (FORECAST_ALLOCATION_REQUIRED_FREE_BYTES + prospective_bytes)
                / block_size,
            block_size,
            free_inodes: FORECAST_ALLOCATION_FREE_INODE_FLOOR,
        }
    }

    // return one two-file allocation request
    fn allocation_request() -> ForecastObjectAllocationRequest {
        ForecastObjectAllocationRequest {
            edge_receipt_identity_sha256: ForecastObjectSha256::admitted([2; 32]),
            object_bytes: 1,
            object_exists: false,
            object_sha256: ForecastObjectSha256::admitted([1; 32]),
            receipt_bytes: 1,
            receipt_exists: false,
        }
    }

    #[test]
    fn census_uses_exact_names_limits_and_linux_allocated_blocks() {
        let fixture = FixtureRoot::new();
        let paths = [
            fixture.write(
                OBJECT_DIRECTORY,
                &final_name(OBJECT_DIRECTORY, 1),
                b"object",
            ),
            fixture.write(
                RECEIPT_DIRECTORY,
                &final_name(RECEIPT_DIRECTORY, 2),
                b"receipt",
            ),
            fixture.write(
                CHANNEL_DIRECTORY,
                &final_name(CHANNEL_DIRECTORY, 3),
                b"channel",
            ),
            fixture.write(
                CHANNEL_BINDING_DIRECTORY,
                &final_name(CHANNEL_BINDING_DIRECTORY, 4),
                b"binding",
            ),
        ];
        let expected_bytes = paths
            .iter()
            .map(|path| {
                fs::symlink_metadata(path)
                    .expect("fixture metadata")
                    .blocks()
                    * ALLOCATED_BLOCK_BYTES
            })
            .sum();
        let allocation = NativeForecastObjectAllocation::new(
            fixture.path.clone(),
            FixtureStatfs {
                sample: admitted_sample(0),
            },
        )
        .expect("admit native allocation");
        assert_eq!(
            allocation.census(),
            ForecastAllocationCensus {
                allocated_bytes: expected_bytes,
                channel_assertion_count: 1,
                channel_binding_count: 1,
                object_count: 1,
                receipt_count: 1,
            }
        );
    }

    #[test]
    fn census_retries_only_a_known_atomic_temporary() {
        let fixture = FixtureRoot::new();
        let temporary = fixture.write(OBJECT_DIRECTORY, ".capture-a1-b2.tmp", b"pending");
        let mut waits = 0;
        let state = scan_census_with_retry(&fixture.path, 1, || {
            waits += 1;
            fs::remove_file(&temporary).expect("settle temporary");
        })
        .expect("retry settled temporary");
        assert_eq!(waits, 1);
        assert_eq!(state.census.allocated_bytes, 0);

        let invalid = fixture.write(OBJECT_DIRECTORY, "unexpected.tmp", b"invalid");
        assert!(scan_census_with_retry(&fixture.path, 1, || {}).is_err());
        fs::remove_file(invalid).expect("remove invalid fixture");
    }

    #[test]
    fn census_rejects_oversized_files_and_channel_count_overflow() {
        let fixture = FixtureRoot::new();
        fixture.write(
            RECEIPT_DIRECTORY,
            &final_name(RECEIPT_DIRECTORY, 1),
            &vec![0; METADATA_MAXIMUM_BYTES as usize + 1],
        );
        assert!(scan_census_with_retry(&fixture.path, 0, || {}).is_err());

        let fixture = FixtureRoot::new();

        // exceed the independent hot channel ceiling by one exact final
        for value in 0..=FORECAST_ALLOCATION_MAXIMUM_CHANNEL_ASSERTIONS {
            fixture.write(
                CHANNEL_DIRECTORY,
                &final_name(CHANNEL_DIRECTORY, value as u64),
                b"",
            );
        }
        assert!(scan_census_with_retry(&fixture.path, 0, || {}).is_err());
    }

    #[test]
    fn allocation_applies_exact_rounding_capacity_and_free_floors() {
        let fixture = FixtureRoot::new();
        let prospective_bytes = 2 * 4_096;
        let mut allocation = NativeForecastObjectAllocation::new(
            fixture.path.clone(),
            FixtureStatfs {
                sample: admitted_sample(prospective_bytes),
            },
        )
        .expect("admit native allocation");
        assert_eq!(
            ForecastObjectAllocationPort::admit(&mut allocation, allocation_request())
                .expect("admission"),
            ForecastObjectAllocationDecision::Admit
        );
        assert_eq!(allocation.last_refusal(), None);
        ForecastObjectAllocationPort::observe(
            &mut allocation,
            ForecastObjectPublication {
                bytes: 1,
                created: true,
                identity_sha256: ForecastObjectSha256::admitted([1; 32]),
                kind: ForecastObjectPublicationKind::Object,
            },
        );
        ForecastObjectAllocationPort::observe(
            &mut allocation,
            ForecastObjectPublication {
                bytes: 1,
                created: true,
                identity_sha256: ForecastObjectSha256::admitted([2; 32]),
                kind: ForecastObjectPublicationKind::Receipt,
            },
        );
        assert_eq!(allocation.census().allocated_bytes, prospective_bytes);
        assert_eq!(allocation.census().object_count, 1);
        assert_eq!(allocation.census().receipt_count, 1);

        let mut low_space = NativeForecastObjectAllocation::new(
            fixture.path.clone(),
            FixtureStatfs {
                sample: ForecastAllocationStatfsSample {
                    available_blocks: admitted_sample(prospective_bytes).available_blocks - 1,
                    ..admitted_sample(prospective_bytes)
                },
            },
        )
        .expect("low-space allocation");
        assert_eq!(
            ForecastObjectAllocationPort::admit(&mut low_space, allocation_request())
                .expect("refusal"),
            ForecastObjectAllocationDecision::Refuse
        );
        assert_eq!(
            low_space.last_refusal(),
            Some(ForecastAllocationRefusal::FreeSpace)
        );

        let mut low_inodes = NativeForecastObjectAllocation::new(
            fixture.path.clone(),
            FixtureStatfs {
                sample: ForecastAllocationStatfsSample {
                    free_inodes: FORECAST_ALLOCATION_FREE_INODE_FLOOR - 1,
                    ..admitted_sample(prospective_bytes)
                },
            },
        )
        .expect("low-inode allocation");
        assert_eq!(
            ForecastObjectAllocationPort::admit(&mut low_inodes, allocation_request())
                .expect("refusal"),
            ForecastObjectAllocationDecision::Refuse
        );
        assert_eq!(
            low_inodes.last_refusal(),
            Some(ForecastAllocationRefusal::Inodes)
        );
    }

    #[test]
    fn allocation_refuses_frozen_identity_and_byte_ceilings() {
        let prospective_bytes = 2 * 4_096;
        let mut census = ForecastAllocationCensus {
            allocated_bytes: 0,
            channel_assertion_count: 0,
            channel_binding_count: 0,
            object_count: FORECAST_ALLOCATION_MAXIMUM_IDENTITIES,
            receipt_count: 0,
        };
        assert_eq!(
            allocation_decision(
                census,
                allocation_request(),
                admitted_sample(prospective_bytes),
            )
            .expect("identity refusal"),
            ForecastObjectAllocationDecision::Refuse
        );

        census.object_count = 0;
        census.allocated_bytes = FORECAST_ALLOCATION_MAXIMUM_BYTES;
        assert_eq!(
            allocation_decision(
                census,
                allocation_request(),
                admitted_sample(prospective_bytes),
            )
            .expect("byte refusal"),
            ForecastObjectAllocationDecision::Refuse
        );
    }

    #[test]
    fn channel_allocation_reuses_global_policy_and_observes_both_finals() {
        let fixture = FixtureRoot::new();
        let prospective_bytes = 2 * 4_096;
        let mut allocation = NativeForecastObjectAllocation::new(
            fixture.path.clone(),
            FixtureStatfs {
                sample: admitted_sample(prospective_bytes),
            },
        )
        .expect("admit native channel allocation");
        let request = ForecastChannelAllocationRequest {
            assertion_bytes: 1,
            assertion_count: 0,
            assertion_exists: false,
            assertion_sha256: ForecastObjectSha256::admitted([3; 32]),
            binding_bytes: 1,
            binding_count: 0,
            edge_receipt_identity_sha256: ForecastObjectSha256::admitted([4; 32]),
        };
        assert_eq!(
            ForecastChannelAllocationPort::admit(&mut allocation, request)
                .expect("channel admission"),
            ForecastChannelAllocationDecision::Admit
        );
        assert_eq!(allocation.last_refusal(), None);
        ForecastChannelAllocationPort::observe(
            &mut allocation,
            ForecastChannelPublication {
                bytes: 1,
                created: true,
                identity_sha256: request.assertion_sha256,
                kind: ForecastChannelPublicationKind::Assertion,
            },
        );
        ForecastChannelAllocationPort::observe(
            &mut allocation,
            ForecastChannelPublication {
                bytes: 1,
                created: true,
                identity_sha256: request.edge_receipt_identity_sha256,
                kind: ForecastChannelPublicationKind::Binding,
            },
        );
        assert_eq!(allocation.census().allocated_bytes, prospective_bytes);
        assert_eq!(allocation.census().channel_assertion_count, 1);
        assert_eq!(allocation.census().channel_binding_count, 1);
    }

    #[test]
    fn channel_capacity_refusal_is_classified_and_refresh_resets_it() {
        let fixture = FixtureRoot::new();
        let prospective_bytes = 2 * 4_096;
        let mut allocation = NativeForecastObjectAllocation::new(
            fixture.path.clone(),
            FixtureStatfs {
                sample: admitted_sample(prospective_bytes),
            },
        )
        .expect("admit native channel allocation");
        let request = ForecastChannelAllocationRequest {
            assertion_bytes: 1,
            assertion_count: FORECAST_ALLOCATION_MAXIMUM_CHANNEL_ASSERTIONS,
            assertion_exists: false,
            assertion_sha256: ForecastObjectSha256::admitted([5; 32]),
            binding_bytes: 1,
            binding_count: FORECAST_ALLOCATION_MAXIMUM_CHANNEL_ASSERTIONS,
            edge_receipt_identity_sha256: ForecastObjectSha256::admitted([6; 32]),
        };
        assert_eq!(
            ForecastChannelAllocationPort::admit(&mut allocation, request)
                .expect("channel refusal"),
            ForecastChannelAllocationDecision::Refuse
        );
        assert_eq!(
            allocation.last_refusal(),
            Some(ForecastAllocationRefusal::Capacity)
        );
        allocation.refresh_from_disk().expect("refresh census");
        assert_eq!(allocation.last_refusal(), None);

        let ordinary_request = ForecastChannelAllocationRequest {
            assertion_count: 0,
            binding_count: 0,
            ..request
        };
        allocation.statfs.sample.available_blocks -= 1;
        assert_eq!(
            ForecastChannelAllocationPort::admit(&mut allocation, ordinary_request)
                .expect("channel free-space refusal"),
            ForecastChannelAllocationDecision::Refuse
        );
        assert_eq!(
            allocation.last_refusal(),
            Some(ForecastAllocationRefusal::FreeSpace)
        );

        allocation.statfs.sample = ForecastAllocationStatfsSample {
            free_inodes: FORECAST_ALLOCATION_FREE_INODE_FLOOR - 1,
            ..admitted_sample(prospective_bytes)
        };
        assert_eq!(
            ForecastChannelAllocationPort::admit(&mut allocation, ordinary_request)
                .expect("channel inode refusal"),
            ForecastChannelAllocationDecision::Refuse
        );
        assert_eq!(
            allocation.last_refusal(),
            Some(ForecastAllocationRefusal::Inodes)
        );
    }
}
