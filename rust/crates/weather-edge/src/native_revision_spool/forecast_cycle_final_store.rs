//! Retain immutable final archive manifests in the original bounded custody directory.

use std::path::{Path, PathBuf};

use crate::forecast_cycle_pages::{ForecastCycleSha256, valid_due_key};
use crate::native_revision_spool_file::{
    ensure_child_directory, list_names, publish_immutable, read_optional_file, require_directory,
};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, is_sha256, lower_hex};

use super::forecast_allocation::ForecastAllocationStatfsPort;
use super::forecast_cycle_disk_slots::{ForecastCycleDiskSlotError, NativeForecastCycleDiskSlots};
use super::uuid_from_linux_entropy;

const DIRECTORY: &str = "final-manifests";
const MAXIMUM_MANIFEST_BYTES: usize = 32 * 1_024;
const MAXIMUM_MANIFESTS: usize = 8_192;

/// Admitted canonical owner of the existing v1 or sealed v2 final manifest.
pub trait ForecastCycleFinalManifestCodec: RevisionArchiveCanonicalCodec {
    type Manifest;

    /// Decode the exact canonical bytes and validate the full retained final schema.
    fn decode_final_manifest(&self, bytes: &[u8]) -> Option<Self::Manifest>;

    /// Encode the exact validated final record with canonical newline framing.
    fn encode_final_manifest(&self, manifest: &Self::Manifest) -> Option<Vec<u8>>;

    /// Return only the authenticated correlation of an already validated manifest.
    fn final_manifest_identity<'a>(&self, manifest: &'a Self::Manifest) -> (&'a str, u64);
}

/// One complete canonical manifest bound to its content-addressed filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedForecastCycleManifest<Manifest> {
    pub manifest: Manifest,
    pub manifest_sha256: ForecastCycleSha256,
}

/// Small validated lookup record kept without manifest payloads during queue ordering.
pub(super) struct FinalManifestIdentity {
    pub due_key: String,
    pub manifest_sha256: ForecastCycleSha256,
}

/// Bounded initialization, write, identity and immutable custody failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCycleFinalManifestError {
    Capacity(ForecastCycleDiskSlotError),
    Collision,
    InvalidManifest,
    RetentionLimit,
    Storage,
}

/// Final manifest access reuses the same admitted evidence root and storage budget.
pub struct NativeForecastCycleFinalManifests<Codec, Statfs> {
    disk: NativeForecastCycleDiskSlots<Codec, Statfs>,
    directory: PathBuf,
}

impl<Codec, Statfs> NativeForecastCycleFinalManifests<Codec, Statfs> {
    /// Bind one previously admitted native custody root and its fixed final directory.
    pub fn new(disk: NativeForecastCycleDiskSlots<Codec, Statfs>) -> std::io::Result<Self> {
        let root = disk.root().join("online-pages");
        require_directory(&root)?;
        let directory = root.join(DIRECTORY);
        ensure_child_directory(&directory)?;
        Ok(Self { disk, directory })
    }

    /// Borrow the single evidence root retained by all local archive data classes.
    pub fn root(&self) -> &Path {
        self.disk.root()
    }
}

impl<Codec: ForecastCycleFinalManifestCodec, Statfs>
    NativeForecastCycleFinalManifests<Codec, Statfs>
{
    /// Read and validate one exact content-addressed immutable final manifest.
    pub fn read_final_manifest(
        &self,
        digest: ForecastCycleSha256,
    ) -> Result<
        Option<RetainedForecastCycleManifest<Codec::Manifest>>,
        ForecastCycleFinalManifestError,
    > {
        use ForecastCycleFinalManifestError as Error;

        let path = self.directory.join(final_name(digest));
        let Some(bytes) =
            read_optional_file(&path, MAXIMUM_MANIFEST_BYTES).map_err(|_| Error::Storage)?
        else {
            return Ok(None);
        };
        let codec = self.disk.codec();
        // bind the complete retained bytes to their requested stable identity
        if codec.sha256(&bytes) != *digest.as_bytes() {
            return Err(Error::Collision);
        }
        let manifest = codec
            .decode_final_manifest(&bytes)
            .ok_or(Error::InvalidManifest)?;
        Ok(Some(RetainedForecastCycleManifest {
            manifest,
            manifest_sha256: digest,
        }))
    }

    /// Validate the full retained census while retaining only sort and lookup keys.
    pub(super) fn validated_final_identities(
        &self,
    ) -> Result<Vec<FinalManifestIdentity>, ForecastCycleFinalManifestError> {
        use ForecastCycleFinalManifestError as Error;

        let mut names =
            list_names(&self.directory, Some(MAXIMUM_MANIFESTS + 1)).map_err(|_| Error::Storage)?;
        // refuse an unbounded manifest directory before sorting or reading contents
        if names.len() > MAXIMUM_MANIFESTS {
            return Err(Error::RetentionLimit);
        }
        names.sort_unstable();
        let mut identities = Vec::with_capacity(names.len());
        // retain only the authenticated scope and digest of each exact complete manifest
        for name in names {
            let digest = final_digest(&name).ok_or(Error::InvalidManifest)?;
            let manifest = self.read_final_manifest(digest)?.ok_or(Error::Storage)?;
            identities.push(FinalManifestIdentity {
                due_key: self
                    .disk
                    .codec()
                    .final_manifest_identity(&manifest.manifest)
                    .0
                    .to_owned(),
                manifest_sha256: digest,
            });
        }
        Ok(identities)
    }

    /// Return the first exact final closure only after validating the retained census.
    pub fn find_final_manifest(
        &self,
        due_key: &str,
        generation: u64,
    ) -> Result<
        Option<RetainedForecastCycleManifest<Codec::Manifest>>,
        ForecastCycleFinalManifestError,
    > {
        use ForecastCycleFinalManifestError as Error;
        // keep the caller's query in the original due-cycle namespace
        if !valid_due_key(due_key) {
            return Err(Error::InvalidManifest);
        }
        let mut names =
            list_names(&self.directory, Some(MAXIMUM_MANIFESTS + 1)).map_err(|_| Error::Storage)?;
        // do not retain or scan beyond the predecessor's finite manifest ceiling
        if names.len() > MAXIMUM_MANIFESTS {
            return Err(Error::RetentionLimit);
        }
        names.sort_unstable();
        let mut found = None;
        // validate every retained record even if the first matching cycle appeared earlier
        for name in names {
            let address = final_digest(&name).ok_or(Error::InvalidManifest)?;
            let Some(entry) = self.read_final_manifest(address)? else {
                continue;
            };
            // retain only the first exact due/generation closure in filename order
            if found.is_none()
                && self.disk.codec().final_manifest_identity(&entry.manifest)
                    == (due_key, generation)
            {
                found = Some(entry);
            }
        }
        Ok(found)
    }
}

impl<Codec: ForecastCycleFinalManifestCodec, Statfs: ForecastAllocationStatfsPort>
    NativeForecastCycleFinalManifests<Codec, Statfs>
{
    /// Publish the exact digest-bound manifest once, or verify identical durable bytes.
    pub fn persist_final_manifest(
        &mut self,
        manifest: &Codec::Manifest,
        expected: ForecastCycleSha256,
    ) -> Result<ForecastCycleSha256, ForecastCycleFinalManifestError> {
        use ForecastCycleFinalManifestError as Error;

        let codec = self.disk.codec();
        let bytes = codec
            .encode_final_manifest(manifest)
            .ok_or(Error::InvalidManifest)?;
        // refuse an oversized, unstable or invalid document before bidding for storage
        if bytes.len() > MAXIMUM_MANIFEST_BYTES
            || codec.sha256(&bytes) != *expected.as_bytes()
            || codec.decode_final_manifest(&bytes).is_none()
        {
            return Err(Error::InvalidManifest);
        }
        self.disk.admit(bytes.len()).map_err(Error::Capacity)?;
        let path = self.directory.join(final_name(expected));
        let read_existing =
            || read_optional_file(&path, MAXIMUM_MANIFEST_BYTES).map_err(|_| Error::Storage);
        // preserve an exact immutable retry without changing the original closure
        if let Some(existing) = read_existing()? {
            return if existing == bytes {
                Ok(expected)
            } else {
                Err(Error::Collision)
            };
        }
        let nonce = uuid_from_linux_entropy().map_err(|_| Error::Storage)?;
        let temporary = self.directory.join(format!(".capture-{nonce}.tmp"));
        match publish_immutable(&path, &temporary, &bytes) {
            Ok(()) => Ok(expected),
            Err(failure)
                if failure.source.kind() == std::io::ErrorKind::AlreadyExists
                    && !failure.temporary_cleanup_failed =>
            {
                // only a byte-identical concurrent immutable result satisfies this invocation
                if read_existing()?.as_deref() == Some(bytes.as_slice()) {
                    Ok(expected)
                } else {
                    Err(Error::Collision)
                }
            }
            Err(_) => Err(Error::Storage),
        }
    }
}

/// Derive the exact content-addressed filename from an admitted binary digest.
fn final_name(digest: ForecastCycleSha256) -> String {
    format!("sha256-{}.json", lower_hex(*digest.as_bytes()))
}

/// Validate a retained manifest address before using any returned path.
fn final_digest(name: &str) -> Option<ForecastCycleSha256> {
    let text = name.strip_prefix("sha256-")?.strip_suffix(".json")?;
    // require the exact lowercase ASCII address before positional decoding
    if !is_sha256(text) {
        return None;
    }
    let mut bytes = [0; 32];
    // decode each validated wire byte without introducing a codec package
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(ForecastCycleSha256::admitted(bytes))
}
