//! Installed root authority composition for the native owner access burn.

use std::fmt::{self, Display, Formatter};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use weather_api::CurrentTimeResolver;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::capture_epoch_operator::{
    CaptureEpochOperatorFile, CaptureEpochOperatorFiles, NativeCaptureEpochOperatorFiles,
};
use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    FamilyCatalogImageVerification, MaintenanceCanonicalJson, NativeFamilyCatalogImageVerifier,
    NativeFamilyReleaseCurrentError, NativeFamilyReleaseCurrentFiles,
    NativeFamilyReleaseCurrentHostFiles, NativeFamilyReleaseLiveRuntime,
    NativeFamilyReleaseLiveRuntimeProcess, NativeHostOwnerAccessAuthority,
    NativeHostOwnerAccessSql, NativeHostOwnerOperationError, NativeWirePrimitives,
    build_native_host_owner_access_sql, read_native_family_release_current,
    system_worker_health_milliseconds,
};

const MAXIMUM_ACCESS_REQUEST_BYTES: usize = 128 * 1_024;
const MAXIMUM_ACKNOWLEDGEMENT_BYTES: usize = 64 * 1024;
const ACCESS_REQUEST_VERSION: &str = "adjustment-confirmation-access-burn-request/v3";
const INSTALLED_ACKNOWLEDGEMENT_PATH: &str =
    "/var/lib/weather/xweather/adjustment-evidence/revision-custody-acknowledgements/current.json";
const SERVICE_UID: u32 = 10_002;

/// Bounded installed-owner failures that do not disclose retained authority material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeHostOwnerAccessInstalledError {
    Request,
    CaptureEpoch,
    Family,
    Catalog,
    Acknowledgement,
    Clock,
    Owner,
}

impl Display for NativeHostOwnerAccessInstalledError {
    /// Render one stable failure category for the fixed root command.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Request => "native owner access request is invalid",
            Self::CaptureEpoch => "native owner capture epoch authority is unavailable",
            Self::Family => "native owner family authority is unavailable",
            Self::Catalog => "native owner installed catalog is unavailable",
            Self::Acknowledgement => "native owner custody acknowledgement is unavailable",
            Self::Clock => "native owner wall clock is unavailable",
            Self::Owner => "native owner access authority differs",
        })
    }
}

impl std::error::Error for NativeHostOwnerAccessInstalledError {}

/// Stable no-follow reader for the service-owned current custody acknowledgement.
pub(crate) struct NativeHostOwnerAccessAcknowledgementFile {
    path: PathBuf,
    owner: u32,
}

impl NativeHostOwnerAccessAcknowledgementFile {
    /// Bind the exact predecessor path and service identity.
    pub(crate) fn native() -> Self {
        Self {
            path: PathBuf::from(INSTALLED_ACKNOWLEDGEMENT_PATH),
            owner: SERVICE_UID,
        }
    }

    /// Bind one disposable literal path without weakening production selection.
    #[cfg(test)]
    pub(crate) fn for_fixture(path: PathBuf, owner: u32) -> Self {
        Self { path, owner }
    }

    /// Read the exact stable regular file under the source owner and mode contract.
    fn read(&mut self) -> Result<Vec<u8>, NativeHostOwnerAccessInstalledError> {
        read_stable_host_file(&self.path, MAXIMUM_ACKNOWLEDGEMENT_BYTES, 0o600, self.owner)
    }
}

/// Independently supplied installed owners used by the testable composition boundary.
pub(crate) struct NativeHostOwnerAccessInstalledOwners<'a, Capture, Family, Runtime, Verifier> {
    pub capture: &'a mut Capture,
    pub family: &'a mut Family,
    pub runtime: &'a mut Runtime,
    pub verifier: &'a mut Verifier,
    pub acknowledgement: &'a mut NativeHostOwnerAccessAcknowledgementFile,
    pub time: &'a dyn CurrentTimeResolver,
    pub now: fn() -> Option<i64>,
}

/// Authenticate and project an access burn from only the fixed installed host authorities.
pub fn build_native_host_owner_access_sql_from_installed(
    request_bytes: &[u8],
    expected_sha256: &str,
) -> Result<NativeHostOwnerAccessSql, NativeHostOwnerAccessInstalledError> {
    let mut capture = NativeCaptureEpochOperatorFiles;
    let mut family = NativeFamilyReleaseCurrentHostFiles::native(0, 0);
    let mut runtime = NativeFamilyReleaseLiveRuntimeProcess::native();
    let mut verifier = NativeFamilyCatalogImageVerifier::new();
    let mut acknowledgement = NativeHostOwnerAccessAcknowledgementFile::native();
    build_native_host_owner_access_sql_from_owners(
        request_bytes,
        expected_sha256,
        NativeHostOwnerAccessInstalledOwners {
            capture: &mut capture,
            family: &mut family,
            runtime: &mut runtime,
            verifier: &mut verifier,
            acknowledgement: &mut acknowledgement,
            time: &NativeWirePrimitives,
            now: system_worker_health_milliseconds,
        },
    )
}

/// Compose independently injected owners while preserving the production observation order.
pub(crate) fn build_native_host_owner_access_sql_from_owners<Capture, Family, Runtime, Verifier>(
    request_bytes: &[u8],
    expected_sha256: &str,
    owners: NativeHostOwnerAccessInstalledOwners<'_, Capture, Family, Runtime, Verifier>,
) -> Result<NativeHostOwnerAccessSql, NativeHostOwnerAccessInstalledError>
where
    Capture: CaptureEpochOperatorFiles,
    Family: NativeFamilyReleaseCurrentFiles,
    Runtime: NativeFamilyReleaseLiveRuntime,
    Verifier: FamilyCatalogImageVerification,
{
    let family = admitted_request_family(request_bytes, expected_sha256)?;
    let witness_bytes = owners
        .capture
        .read(CaptureEpochOperatorFile::Witness)
        .map_err(|_| NativeHostOwnerAccessInstalledError::CaptureEpoch)?;
    let mut family_files = CatalogCapturingFiles::new(owners.family);
    let mut codec = NativeWirePrimitives;
    let current = read_native_family_release_current(
        &family,
        &mut family_files,
        owners.runtime,
        owners.verifier,
        &mut codec,
        owners.time,
    )
    .map_err(map_family_error)?;
    let catalog_bytes = family_files
        .catalog
        .as_deref()
        .ok_or(NativeHostOwnerAccessInstalledError::Catalog)?;
    let installed_catalog = codec
        .parse_canonical_object(catalog_bytes)
        .ok_or(NativeHostOwnerAccessInstalledError::Catalog)?;
    let acknowledgement_bytes = owners.acknowledgement.read()?;
    let actual_acknowledgement = codec
        .parse_canonical_object(&acknowledgement_bytes)
        .ok_or(NativeHostOwnerAccessInstalledError::Acknowledgement)?;
    let now_milliseconds = (owners.now)().ok_or(NativeHostOwnerAccessInstalledError::Clock)?;
    build_native_host_owner_access_sql(
        request_bytes,
        expected_sha256,
        NativeHostOwnerAccessAuthority {
            capture_epoch_witness_bytes: &witness_bytes,
            current: &current,
            installed_catalog: &installed_catalog,
            actual_acknowledgement: &actual_acknowledgement,
            now_milliseconds,
            time: owners.time,
        },
    )
    .map_err(map_owner_error)
}

/// Admit only enough canonical request identity to select the independent family authority.
fn admitted_request_family(
    request_bytes: &[u8],
    expected_sha256: &str,
) -> Result<String, NativeHostOwnerAccessInstalledError> {
    let mut wire = NativeWirePrimitives;
    // reject a malformed caller document before opening any retained authority
    if !(2..=MAXIMUM_ACCESS_REQUEST_BYTES).contains(&request_bytes.len())
        || !valid_hash(expected_sha256)
    {
        return Err(NativeHostOwnerAccessInstalledError::Request);
    }
    let request = wire
        .parse_canonical_object(request_bytes)
        .ok_or(NativeHostOwnerAccessInstalledError::Request)?;
    let actual_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, request_bytes));
    let shadow = field(&request, "shadowRegistration")
        .ok_or(NativeHostOwnerAccessInstalledError::Request)?;
    let family = text(shadow, "family").ok_or(NativeHostOwnerAccessInstalledError::Request)?;
    // select only a source family after the canonical operand and request version agree
    if actual_sha256 != expected_sha256
        || text(&request, "contractVersion").as_deref() != Some(ACCESS_REQUEST_VERSION)
        || !matches!(family.as_str(), "temperature" | "wind" | "rain")
    {
        return Err(NativeHostOwnerAccessInstalledError::Request);
    }
    Ok(family)
}

/// Retain the exact catalog bytes already admitted by the current-family owner.
struct CatalogCapturingFiles<'a, Files> {
    inner: &'a mut Files,
    catalog: Option<Vec<u8>>,
}

impl<'a, Files> CatalogCapturingFiles<'a, Files> {
    /// Wrap one file owner without adding another filesystem observation.
    const fn new(inner: &'a mut Files) -> Self {
        Self {
            inner,
            catalog: None,
        }
    }
}

impl<Files> NativeFamilyReleaseCurrentFiles for CatalogCapturingFiles<'_, Files>
where
    Files: NativeFamilyReleaseCurrentFiles,
{
    /// Forward the release marker read unchanged.
    fn read_current_release(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        self.inner.read_current_release()
    }

    /// Forward the release-derived environment read unchanged.
    fn read_environment(
        &mut self,
        release: &str,
    ) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        self.inner.read_environment(release)
    }

    /// Forward the independent settings read unchanged.
    fn read_settings(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        self.inner.read_settings()
    }

    /// Retain the already read catalog bytes for the later owner cross-binding.
    fn read_catalog(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        let bytes = self.inner.read_catalog()?;
        self.catalog = Some(bytes.clone());
        Ok(bytes)
    }

    /// Forward the optional completed family authority read unchanged.
    fn read_current_authority(
        &mut self,
    ) -> Result<Option<Vec<u8>>, NativeFamilyReleaseCurrentError> {
        self.inner.read_current_authority()
    }
}

/// Map family observation failures without reflecting their source material.
fn map_family_error(error: NativeFamilyReleaseCurrentError) -> NativeHostOwnerAccessInstalledError {
    match error {
        NativeFamilyReleaseCurrentError::Catalog => NativeHostOwnerAccessInstalledError::Catalog,
        _ => NativeHostOwnerAccessInstalledError::Family,
    }
}

/// Map final cross-binding refusal to the installed owner boundary.
fn map_owner_error(error: NativeHostOwnerOperationError) -> NativeHostOwnerAccessInstalledError {
    match error {
        NativeHostOwnerOperationError::Size
        | NativeHostOwnerOperationError::Canonical
        | NativeHostOwnerOperationError::Request => NativeHostOwnerAccessInstalledError::Request,
        NativeHostOwnerOperationError::Authority
        | NativeHostOwnerOperationError::Result
        | NativeHostOwnerOperationError::Sql => NativeHostOwnerAccessInstalledError::Owner,
    }
}

/// Read one exact source host file and reject mutation during the bounded descriptor read.
fn read_stable_host_file(
    path: &Path,
    maximum: usize,
    mode: u32,
    owner: u32,
) -> Result<Vec<u8>, NativeHostOwnerAccessInstalledError> {
    let mut file = open_read_no_follow(path)
        .map_err(|_| NativeHostOwnerAccessInstalledError::Acknowledgement)?;
    let canonical = path
        .canonicalize()
        .map_err(|_| NativeHostOwnerAccessInstalledError::Acknowledgement)?;
    let before = file
        .metadata()
        .map_err(|_| NativeHostOwnerAccessInstalledError::Acknowledgement)?;
    // preserve readHostFile's literal path, ownership, mode, link and size contract
    if canonical != path
        || !before.is_file()
        || before.nlink() != 1
        || before.uid() != owner
        || before.gid() != owner
        || before.mode() & 0o777 != mode
        || !(1..=maximum as u64).contains(&before.size())
    {
        return Err(NativeHostOwnerAccessInstalledError::Acknowledgement);
    }
    let length = usize::try_from(before.size())
        .map_err(|_| NativeHostOwnerAccessInstalledError::Acknowledgement)?;
    let mut bytes = Vec::with_capacity(length);
    file.by_ref()
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| NativeHostOwnerAccessInstalledError::Acknowledgement)?;
    let after = file
        .metadata()
        .map_err(|_| NativeHostOwnerAccessInstalledError::Acknowledgement)?;
    // reject partial, grown, replaced or modified material after the read
    if bytes.len() != length
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.size() != after.size()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
    {
        return Err(NativeHostOwnerAccessInstalledError::Acknowledgement);
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "native_host_owner_access_installed_test.rs"]
mod tests;
