//! Read-only native projection of the predecessor's current family release authority.

use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_edge::ForecastAdjustmentSettingsJsonAdapter;

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::family_release_identity::compute_family_release_image_identity;
use crate::family_release_images::FamilyCatalogImageVerification;
use crate::family_target_catalog::{canonical_member, catalog_entries};
use crate::family_target_catalog_validation::{V1, validate_family_target_catalog};
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_edge_admin_files::NativeEdgeAdminJson;
use crate::native_unsupported_terminal_proof_install::{
    NativeUnsupportedTerminalProofFamilyAuthority, NativeUnsupportedTerminalProofFamilyCurrent,
    NativeUnsupportedTerminalProofInstallError,
};
use crate::{MaintenanceAnchorStatusJson, MaintenanceCanonicalJson};

const CURRENT_VERSION: &str = "adjustment-family-release-current/v1";
const BOOTSTRAP_VERSION: &str = "adjustment-family-release-bootstrap-current/v1";
const STATUS_VERSION: &str = "adjustment-family-release-current-status/v1";
const BASELINE_RELEASE: &str = "2026.10.09-1";
const BASELINE_COMMIT: &str = "56b327d9c750946f6f6963b6fe1fa5c9bba791ca";
const BASELINE_SERVER_IMAGE: &str = "ghcr.io/anstosa/weather-server@sha256:fb140b46d6eaea463ba2d10dc74303eac515135a37c21ddb746cdce744fd23ab";
const BASELINE_WEB_IMAGE: &str = "ghcr.io/anstosa/weather-web@sha256:fdcb2d10da4c9ed5ec8651bafa96c9d2b240b66b85db85619b909d6e144e2d7b";
const MAXIMUM_CATALOG_BYTES: usize = 256 * 1024;
const MAXIMUM_CURRENT_BYTES: usize = 8 * 1024;
const MAXIMUM_ENVIRONMENT_BYTES: usize = 16 * 1024;
const MAXIMUM_RELEASE_BYTES: usize = 128;
const MAXIMUM_SETTINGS_BYTES: usize = 4 * 1024;
const ENVIRONMENT_KEYS: &[&str] = &[
    "CLOUDFLARED_IMAGE",
    "POSTGRES_IMAGE",
    "WEATHER_CONTROL_PLANE_SHA256",
    "WEATHER_CONTROL_PLANE_VERSION",
    "WEATHER_DATABASE_NAME",
    "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH",
    "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH",
    "WEATHER_POSTGRES_DIR",
    "WEATHER_RELEASE",
    "WEATHER_SERVER_IMAGE",
    "WEATHER_WEB_IMAGE",
];
const BOOTSTRAP_KEYS: &[&str] = &[
    "bootstrappedAt",
    "catalogSha256",
    "commit",
    "contractVersion",
    "imageDigestsSha256",
    "release",
    "serverImage",
    "settingsSha256",
    "state",
    "webImage",
];
const CURRENT_KEYS: &[&str] = &[
    "acknowledgedAt",
    "actionSha256",
    "commit",
    "contractVersion",
    "family",
    "imageDigestsSha256",
    "release",
    "state",
];

/// Bounded failures from the current family release projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeFamilyReleaseCurrentError {
    Family,
    File,
    Release,
    Environment,
    Runtime,
    Settings,
    Catalog,
    Current,
    Identity,
    Codec,
}

impl Display for NativeFamilyReleaseCurrentError {
    /// Return one stable failure without embedding retained host values.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Family => "family current authority request is invalid",
            Self::File => "family release host file is unsafe",
            Self::Release => "current release is invalid",
            Self::Environment => "family release environment differs",
            Self::Runtime => "family release live image or health differs",
            Self::Settings => "adjustment settings are unavailable",
            Self::Catalog => "installed adjustment catalog is invalid",
            Self::Current => "family current acknowledgement is invalid",
            Self::Identity => "family current authority differs",
            Self::Codec => "family current authority serialization failed",
        })
    }
}

impl std::error::Error for NativeFamilyReleaseCurrentError {}

/// Exact fixed paths used by the production current-family observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseCurrentPaths {
    pub current_release: PathBuf,
    pub releases: PathBuf,
    pub state_root: PathBuf,
    pub settings: PathBuf,
    pub catalog: PathBuf,
}

impl NativeFamilyReleaseCurrentPaths {
    /// Select only the predecessor's deployed production paths.
    pub fn native() -> Self {
        Self {
            current_release: PathBuf::from("/opt/weather/current/deploy/state/current-release"),
            releases: PathBuf::from("/opt/weather/current/deploy/releases"),
            state_root: PathBuf::from("/opt/weather/current/deploy/state/adjustment-release-state"),
            settings: PathBuf::from("/var/lib/weather/xweather/forecast-adjustment-settings.json"),
            catalog: PathBuf::from(
                "/opt/weather/current/deploy/state/adjustment-candidate-catalog.json",
            ),
        }
    }
}

/// Stable host files required before live runtime or image verification.
pub trait NativeFamilyReleaseCurrentFiles {
    /// Read the root-owned current release marker.
    fn read_current_release(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError>;

    /// Read one exact root-owned release environment.
    fn read_environment(
        &mut self,
        release: &str,
    ) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError>;

    /// Read the operator settings snapshot.
    fn read_settings(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError>;

    /// Read the root-installed candidate catalog.
    fn read_catalog(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError>;

    /// Read the optional owner-private current release acknowledgement.
    fn read_current_authority(
        &mut self,
    ) -> Result<Option<Vec<u8>>, NativeFamilyReleaseCurrentError>;
}

/// Read-only live runtime observation kept separate from filesystem authority.
pub trait NativeFamilyReleaseLiveRuntime {
    /// Verify API, worker, and web are healthy on the exact environment image references.
    fn verify_running_images(
        &mut self,
        release: &str,
        environment: &JsonValue,
    ) -> Result<(), NativeFamilyReleaseCurrentError>;
}

/// Concrete no-follow reader for all fixed current-family host paths.
pub struct NativeFamilyReleaseCurrentHostFiles {
    paths: NativeFamilyReleaseCurrentPaths,
    root_uid: u32,
    settings_uids: [u32; 2],
    state_uid: u32,
    state_gid: u32,
}

impl NativeFamilyReleaseCurrentHostFiles {
    /// Bind production paths and their fixed root/service identities.
    pub fn native(state_uid: u32, state_gid: u32) -> Self {
        Self::for_paths(
            NativeFamilyReleaseCurrentPaths::native(),
            0,
            [0, 10_002],
            state_uid,
            state_gid,
        )
    }

    /// Bind disposable paths and explicitly selected fixture identities.
    pub fn for_paths(
        paths: NativeFamilyReleaseCurrentPaths,
        root_uid: u32,
        settings_uids: [u32; 2],
        state_uid: u32,
        state_gid: u32,
    ) -> Self {
        Self {
            paths,
            root_uid,
            settings_uids,
            state_uid,
            state_gid,
        }
    }
}

impl NativeFamilyReleaseCurrentFiles for NativeFamilyReleaseCurrentHostFiles {
    /// Read the current release through the exact root-owned 0600 boundary.
    fn read_current_release(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        read_host_file(
            &self.paths.current_release,
            MAXIMUM_RELEASE_BYTES,
            0o600,
            &[self.root_uid],
        )
    }

    /// Read only the release-derived fixed environment filename.
    fn read_environment(
        &mut self,
        release: &str,
    ) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        valid_release(release)
            .then_some(())
            .ok_or(NativeFamilyReleaseCurrentError::Release)?;
        read_host_file(
            &self.paths.releases.join(format!("{release}.env")),
            MAXIMUM_ENVIRONMENT_BYTES,
            0o600,
            &[self.root_uid],
        )
    }

    /// Read settings from either source-authorized owner identity.
    fn read_settings(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        read_host_file(
            &self.paths.settings,
            MAXIMUM_SETTINGS_BYTES,
            0o600,
            &self.settings_uids,
        )
    }

    /// Read the root-owned installed catalog without granting model authority.
    fn read_catalog(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
        read_host_file(
            &self.paths.catalog,
            MAXIMUM_CATALOG_BYTES,
            0o644,
            &[self.root_uid],
        )
    }

    /// Preserve genuine absence only for the owner-private current acknowledgement.
    fn read_current_authority(
        &mut self,
    ) -> Result<Option<Vec<u8>>, NativeFamilyReleaseCurrentError> {
        read_private_file(
            &self.paths.state_root.join("current.json"),
            MAXIMUM_CURRENT_BYTES,
            self.state_uid,
            self.state_gid,
            true,
        )
    }
}

/// Exact current status returned after all host, runtime, settings, and catalog checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseCurrent {
    pub active_installed_receipt_sha256: Option<String>,
    pub catalog_sha256: String,
    pub commit: String,
    pub control_installed_receipt_sha256: Option<String>,
    pub contract_version: String,
    pub family: String,
    pub release: String,
    pub settings_sha256: String,
    pub shadow_installed_receipt_sha256: Option<String>,
    pub source_server_image_digest: String,
}

/// Read the exact production current-family projection through supplied read-only owners.
pub fn read_native_family_release_current(
    family: &str,
    files: &mut impl NativeFamilyReleaseCurrentFiles,
    runtime: &mut impl NativeFamilyReleaseLiveRuntime,
    verifier: &mut impl FamilyCatalogImageVerification,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeFamilyReleaseCurrent, NativeFamilyReleaseCurrentError> {
    use NativeFamilyReleaseCurrentError as Error;

    // close the source family selection before observing any host file
    if !matches!(family, "temperature" | "wind" | "rain") {
        return Err(Error::Family);
    }
    let release_bytes = files.read_current_release()?;
    let release = parse_release_marker(&release_bytes)?;
    let environment_bytes = files.read_environment(&release)?;
    let environment = parse_environment(&environment_bytes, &release)?;
    runtime.verify_running_images(&release, &environment)?;

    let settings_bytes = files.read_settings()?;
    validate_settings(&settings_bytes)?;
    let catalog_bytes = files.read_catalog()?;
    let catalog = canonical_member(&catalog_bytes, MAXIMUM_CATALOG_BYTES, codec)
        .map_err(|_| Error::Catalog)?;
    validate_family_target_catalog(&catalog, codec, time).map_err(|_| Error::Catalog)?;
    verifier
        .verify_catalog(
            &text(&environment, "WEATHER_SERVER_IMAGE").ok_or(Error::Environment)?,
            &catalog_bytes,
        )
        .map_err(|_| Error::Catalog)?;
    let current_bytes = files.read_current_authority()?;
    let current = current_bytes
        .as_deref()
        .map(|bytes| parse_current(bytes, codec))
        .transpose()?;
    let catalog_sha256 = digest(codec, &catalog_bytes)?;
    let settings_sha256 = digest(codec, &settings_bytes)?;
    let image_identity = compute_family_release_image_identity(&environment, codec)
        .map_err(|_| Error::Codec)?
        .as_str()
        .to_owned();

    let commit = if release == BASELINE_RELEASE {
        let server = text(&environment, "WEATHER_SERVER_IMAGE").ok_or(Error::Environment)?;
        let web = text(&environment, "WEATHER_WEB_IMAGE").ok_or(Error::Environment)?;
        // seed only the exact frozen predecessor image pair
        if server != BASELINE_SERVER_IMAGE || web != BASELINE_WEB_IMAGE {
            return Err(Error::Identity);
        }
        BASELINE_COMMIT.to_owned()
    } else {
        let current = current.as_ref().ok_or(Error::Current)?;
        // bind the acknowledged authority to this release and exact live image pair
        if text(current, "release").as_deref() != Some(release.as_str())
            || text(current, "imageDigestsSha256").as_deref() != Some(image_identity.as_str())
        {
            return Err(Error::Identity);
        }
        // authenticate the actionless bootstrap against live immutable material
        if text(current, "contractVersion").as_deref() == Some(BOOTSTRAP_VERSION)
            && (text(current, "serverImage") != text(&environment, "WEATHER_SERVER_IMAGE")
                || text(current, "webImage") != text(&environment, "WEATHER_WEB_IMAGE")
                || text(current, "catalogSha256").as_deref() != Some(catalog_sha256.as_str()))
        {
            return Err(Error::Identity);
        }
        text(current, "commit").ok_or(Error::Current)?
    };
    let server_image = text(&environment, "WEATHER_SERVER_IMAGE").ok_or(Error::Environment)?;
    let source_server_image_digest = server_image.split('@').nth(1).ok_or(Error::Environment)?;
    source_server_image_digest
        .strip_prefix("sha256:")
        .filter(|digest| valid_hash(digest))
        .ok_or(Error::Environment)?;

    Ok(NativeFamilyReleaseCurrent {
        active_installed_receipt_sha256: installed_receipt(&catalog, family, "active", codec)?,
        catalog_sha256,
        commit,
        control_installed_receipt_sha256: if family == "rain" {
            installed_receipt(&catalog, family, "control", codec)?
        } else {
            None
        },
        contract_version: STATUS_VERSION.to_owned(),
        family: family.to_owned(),
        release,
        settings_sha256,
        shadow_installed_receipt_sha256: installed_receipt(&catalog, family, "shadow", codec)?,
        source_server_image_digest: source_server_image_digest.to_owned(),
    })
}

/// Retained composition owner that directly satisfies unsupported-proof source admission.
pub struct PreparedNativeFamilyReleaseCurrentAuthority<'a, Files, Runtime, Verifier, Codec, Time> {
    pub files: &'a mut Files,
    pub runtime: &'a mut Runtime,
    pub verifier: &'a mut Verifier,
    pub codec: &'a mut Codec,
    pub time: &'a Time,
}

impl<Files, Runtime, Verifier, Codec, Time> NativeUnsupportedTerminalProofFamilyAuthority
    for PreparedNativeFamilyReleaseCurrentAuthority<'_, Files, Runtime, Verifier, Codec, Time>
where
    Files: NativeFamilyReleaseCurrentFiles,
    Runtime: NativeFamilyReleaseLiveRuntime,
    Verifier: FamilyCatalogImageVerification,
    Codec: MaintenanceAnchorStatusJson,
    Time: CurrentTimeResolver,
{
    /// Re-run the complete read-only live observation for the requested proof family.
    fn read_current_family(
        &mut self,
        family: &str,
    ) -> Result<
        NativeUnsupportedTerminalProofFamilyCurrent,
        NativeUnsupportedTerminalProofInstallError,
    > {
        let current = read_native_family_release_current(
            family,
            self.files,
            self.runtime,
            self.verifier,
            self.codec,
            self.time,
        )
        .map_err(|_| NativeUnsupportedTerminalProofInstallError::Source)?;
        Ok(NativeUnsupportedTerminalProofFamilyCurrent {
            contract_version: current.contract_version,
            family: current.family,
            commit: current.commit,
        })
    }
}

/// Parse the exact ASCII release marker after the source's trim operation.
fn parse_release_marker(bytes: &[u8]) -> Result<String, NativeFamilyReleaseCurrentError> {
    let value = std::str::from_utf8(bytes)
        .ok()
        .filter(|value| value.is_ascii())
        .map(str::trim)
        .filter(|value| valid_release(value))
        .ok_or(NativeFamilyReleaseCurrentError::Release)?;
    Ok(value.to_owned())
}

/// Parse the closed eleven-line release environment without evaluating shell syntax.
fn parse_environment(
    bytes: &[u8],
    release: &str,
) -> Result<JsonValue, NativeFamilyReleaseCurrentError> {
    use NativeFamilyReleaseCurrentError::Environment;

    let source = std::str::from_utf8(bytes).map_err(|_| Environment)?;
    let Some(body) = source.strip_suffix('\n') else {
        return Err(Environment);
    };
    let lines = body.split('\n').collect::<Vec<_>>();
    // retain exactly eleven nonempty ASCII assignment lines
    if lines.len() != 11 || lines.iter().any(|line| !valid_environment_line(line)) {
        return Err(Environment);
    }
    let mut entries = Vec::with_capacity(lines.len());
    // split only the first assignment separator and preserve the remaining value
    for line in lines {
        let (key, value) = line.split_once('=').ok_or(Environment)?;
        entries.push((key.encode_utf16().collect(), json_text(value)));
    }
    let environment = JsonValue::Object(entries);
    exact_keys(&environment, ENVIRONMENT_KEYS)
        .then_some(())
        .ok_or(Environment)?;
    let server = text(&environment, "WEATHER_SERVER_IMAGE").ok_or(Environment)?;
    let web = text(&environment, "WEATHER_WEB_IMAGE").ok_or(Environment)?;
    let postgres = text(&environment, "POSTGRES_IMAGE").ok_or(Environment)?;
    let cloudflared = text(&environment, "CLOUDFLARED_IMAGE").ok_or(Environment)?;
    // bind release, application, dependency, and control-plane identities
    if text(&environment, "WEATHER_RELEASE").as_deref() != Some(release)
        || !valid_weather_image(&server)
        || !valid_weather_image(&web)
        || !valid_generic_image(&postgres)
        || !valid_generic_image(&cloudflared)
        || !text(&environment, "WEATHER_CONTROL_PLANE_SHA256")
            .is_some_and(|value| valid_hash(&value))
        || !text(&environment, "WEATHER_CONTROL_PLANE_VERSION")
            .is_some_and(|value| valid_control_version(&value))
    {
        return Err(Environment);
    }
    Ok(environment)
}

/// Validate the same complete persisted four-field settings document as the source.
fn validate_settings(bytes: &[u8]) -> Result<(), NativeFamilyReleaseCurrentError> {
    ForecastAdjustmentSettingsJsonAdapter::decode(&mut NativeEdgeAdminJson, bytes)
        .map(|_| ())
        .map_err(|_| NativeFamilyReleaseCurrentError::Settings)
}

/// Parse and validate either exact current authority generation.
fn parse_current(
    bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<JsonValue, NativeFamilyReleaseCurrentError> {
    use NativeFamilyReleaseCurrentError::Current;

    if !(2..=MAXIMUM_CURRENT_BYTES).contains(&bytes.len()) {
        return Err(Current);
    }
    let value = codec.parse_canonical_object(bytes).ok_or(Current)?;
    // prohibit alternate serialization before selecting the versioned schema
    if codec.encode_canonical_json(&value).as_deref() != Some(bytes) {
        return Err(Current);
    }
    let version = text(&value, "contractVersion").ok_or(Current)?;
    if version == BOOTSTRAP_VERSION {
        validate_bootstrap_current(&value, codec)?;
    } else {
        validate_acknowledged_current(&value)?;
    }
    Ok(value)
}

/// Validate the actionless inert-v14 bridge current authority.
fn validate_bootstrap_current(
    value: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<(), NativeFamilyReleaseCurrentError> {
    use NativeFamilyReleaseCurrentError::Current;

    exact_keys(value, BOOTSTRAP_KEYS)
        .then_some(())
        .ok_or(Current)?;
    let server = text(value, "serverImage").ok_or(Current)?;
    let web = text(value, "webImage").ok_or(Current)?;
    // preserve the closed bootstrap state and immutable identity grammars
    if text(value, "state").as_deref() != Some("inert_v14_bootstrap")
        || !text(value, "commit").is_some_and(|value| valid_commit(&value))
        || !text(value, "bootstrappedAt").is_some_and(|value| utc_shape(&value))
        || !valid_weather_image(&server)
        || !valid_weather_image(&web)
        || server.contains("weather-web")
        || web.contains("weather-server")
        || !text(value, "catalogSha256").is_some_and(|value| valid_hash(&value))
        || !text(value, "imageDigestsSha256").is_some_and(|value| valid_hash(&value))
        || !text(value, "settingsSha256").is_some_and(|value| valid_hash(&value))
        || !text(value, "release").is_some_and(|value| valid_release(&value))
    {
        return Err(Current);
    }
    let environment = JsonValue::Object(vec![
        (
            "WEATHER_SERVER_IMAGE".encode_utf16().collect(),
            json_text(&server),
        ),
        (
            "WEATHER_WEB_IMAGE".encode_utf16().collect(),
            json_text(&web),
        ),
    ]);
    let expected =
        compute_family_release_image_identity(&environment, codec).map_err(|_| Current)?;
    (text(value, "imageDigestsSha256").as_deref() == Some(expected.as_str()))
        .then_some(())
        .ok_or(Current)
}

/// Validate one acknowledged or compensated family release authority.
fn validate_acknowledged_current(value: &JsonValue) -> Result<(), NativeFamilyReleaseCurrentError> {
    use NativeFamilyReleaseCurrentError::Current;

    exact_keys(value, CURRENT_KEYS)
        .then_some(())
        .ok_or(Current)?;
    let state = text(value, "state").ok_or(Current)?;
    let family = text(value, "family").ok_or(Current)?;
    // admit only the source's two completed states and exact identity grammars
    if text(value, "contractVersion").as_deref() != Some(CURRENT_VERSION)
        || !matches!(state.as_str(), "acknowledged" | "compensated")
        || !matches!(family.as_str(), "temperature" | "wind" | "rain")
        || !text(value, "commit").is_some_and(|value| valid_commit(&value))
        || !text(value, "acknowledgedAt").is_some_and(|value| utc_shape(&value))
        || !text(value, "actionSha256").is_some_and(|value| valid_hash(&value))
        || !text(value, "imageDigestsSha256").is_some_and(|value| valid_hash(&value))
        || !text(value, "release").is_some_and(|value| valid_release(&value))
    {
        return Err(Current);
    }
    Ok(())
}

/// Hash one exact installed family receipt or retain explicit absence.
fn installed_receipt(
    catalog: &JsonValue,
    family: &str,
    slot: &str,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<Option<String>, NativeFamilyReleaseCurrentError> {
    use NativeFamilyReleaseCurrentError::{Catalog, Codec};

    let legacy = text(catalog, "contractVersion").as_deref() == Some(V1);
    let entry = catalog_entries(catalog)
        .map_err(|_| Catalog)?
        .iter()
        .find(|entry| {
            text(entry, "family").as_deref() == Some(family)
                && if legacy {
                    slot == "shadow"
                } else {
                    text(entry, "slot").as_deref() == Some(slot)
                }
        });
    let Some(entry) = entry else {
        return Ok(None);
    };
    let expected_keys = if legacy {
        &["family", "receipt", "registration"][..]
    } else {
        &["family", "receipt", "registration", "slot"][..]
    };
    exact_keys(entry, expected_keys)
        .then_some(())
        .ok_or(Catalog)?;
    let bytes = codec
        .encode_canonical_json(field(entry, "receipt").ok_or(Catalog)?)
        .ok_or(Codec)?;
    Ok(Some(digest(codec, &bytes)?))
}

/// Hash exact observed bytes through the approved shared adapter.
fn digest(
    codec: &mut impl MaintenanceCanonicalJson,
    bytes: &[u8],
) -> Result<String, NativeFamilyReleaseCurrentError> {
    let digest = codec
        .sha256(bytes)
        .ok_or(NativeFamilyReleaseCurrentError::Codec)?;
    valid_hash(digest.as_str())
        .then(|| digest.as_str().to_owned())
        .ok_or(NativeFamilyReleaseCurrentError::Codec)
}

/// Read one fixed host file through a stable no-follow descriptor.
fn read_host_file(
    path: &Path,
    maximum: usize,
    mode: u32,
    owners: &[u32],
) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
    let file = open_read_no_follow(path).map_err(|_| NativeFamilyReleaseCurrentError::File)?;
    let canonical = path
        .canonicalize()
        .map_err(|_| NativeFamilyReleaseCurrentError::File)?;
    let before = file
        .metadata()
        .map_err(|_| NativeFamilyReleaseCurrentError::File)?;
    // retain exact regular-file, owner, group, mode, size, link, and literal-path custody
    if canonical != path
        || !before.is_file()
        || before.nlink() != 1
        || !owners.contains(&before.uid())
        || before.gid() != before.uid()
        || before.mode() & 0o777 != mode
        || !(1..=maximum as u64).contains(&before.size())
    {
        return Err(NativeFamilyReleaseCurrentError::File);
    }
    read_stable_file(file, before, maximum)
}

/// Read one optional owner-private current authority file.
fn read_private_file(
    path: &Path,
    maximum: usize,
    uid: u32,
    gid: u32,
    optional: bool,
) -> Result<Option<Vec<u8>>, NativeFamilyReleaseCurrentError> {
    let file = match open_read_no_follow(path) {
        Ok(file) => file,
        Err(error) if optional && error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(NativeFamilyReleaseCurrentError::File),
    };
    let before = file
        .metadata()
        .map_err(|_| NativeFamilyReleaseCurrentError::File)?;
    // retain the source private-file owner and size boundary
    if !before.is_file()
        || before.nlink() != 1
        || before.uid() != uid
        || before.gid() != gid
        || before.mode() & 0o777 != 0o600
        || !(1..=maximum as u64).contains(&before.size())
    {
        return Err(NativeFamilyReleaseCurrentError::File);
    }
    read_stable_file(file, before, maximum).map(Some)
}

/// Finish one bounded descriptor read and reject mutation while reading.
fn read_stable_file(
    mut file: File,
    before: std::fs::Metadata,
    maximum: usize,
) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
    let expected =
        usize::try_from(before.size()).map_err(|_| NativeFamilyReleaseCurrentError::File)?;
    let mut bytes = Vec::with_capacity(expected);
    (&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| NativeFamilyReleaseCurrentError::File)?;
    let after = file
        .metadata()
        .map_err(|_| NativeFamilyReleaseCurrentError::File)?;
    // compare descriptor identity, exact length, and nanosecond modification clock
    if bytes.len() != expected
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.size() != after.size()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
    {
        return Err(NativeFamilyReleaseCurrentError::File);
    }
    Ok(bytes)
}

/// Recognize one source release with a one-to-three digit positive revision.
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    (12..=14).contains(&bytes.len())
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'.'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'.'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'-'
        && matches!(bytes[11], b'1'..=b'9')
        && bytes[12..].iter().all(u8::is_ascii_digit)
}

/// Recognize one exact lowercase forty-character Git commit.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Recognize one exact Weather server or web immutable reference.
fn valid_weather_image(value: &str) -> bool {
    value
        .strip_prefix("ghcr.io/anstosa/weather-server@sha256:")
        .or_else(|| value.strip_prefix("ghcr.io/anstosa/weather-web@sha256:"))
        .is_some_and(valid_hash)
}

/// Recognize one generic immutable dependency image without whitespace or a second separator.
fn valid_generic_image(value: &str) -> bool {
    let Some((name, digest)) = value.split_once("@sha256:") else {
        return false;
    };
    !name.is_empty()
        && !name
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte == b'@')
        && valid_hash(digest)
}

/// Recognize one positive one-to-six digit control-plane generation.
fn valid_control_version(value: &str) -> bool {
    (1..=6).contains(&value.len())
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Recognize one exact environment assignment line.
fn valid_environment_line(value: &str) -> bool {
    let Some((key, content)) = value.split_once('=') else {
        return false;
    };
    !content.is_empty()
        && !content.bytes().any(|byte| matches!(byte, b'\r' | b'\n'))
        && matches!(key.as_bytes().first(), Some(b'A'..=b'Z'))
        && key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Recognize the source's fixed four-digit millisecond UTC shape without calendar coercion.
fn utc_shape(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 24
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes.get(10) == Some(&b'T')
        && bytes.get(13) == Some(&b':')
        && bytes.get(16) == Some(&b':')
        && bytes.get(19) == Some(&b'.')
        && bytes.get(23) == Some(&b'Z')
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 23) || byte.is_ascii_digit()
        })
}

/// Construct one generated UTF-16 string value.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_family_release_current_test.rs"]
mod tests;
