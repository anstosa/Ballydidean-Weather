//! Compose the family-release transaction from fixed installed production owners.

use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use weather_contracts::JsonValue;
use weather_edge::{
    ArchiveWirePrimitives, RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec,
};

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::maintenance_canonical_json::{exact_keys, text, valid_hash};
use crate::{
    MaintenanceCanonicalJson, NativeCaptureEpochPublicGitJson, NativeFamilyActionAuthorityFiles,
    NativeFamilyCatalogImageVerifier, NativeFamilyReleaseCurrent, NativeFamilyReleaseCurrentFiles,
    NativeFamilyReleaseInput, NativeFamilyReleaseProductionPort,
    NativeFamilyReleaseTargetAuthorityError, NativeFamilyReleaseTargetAuthorityInput,
    NativeFamilyReleaseTargetAuthorityPorts, NativeFamilyReleaseTargetAuthorityRead,
    NativeFamilyReleaseTransactionError, NativeFamilyReleaseTransactionResult,
    NativeFamilyReleaseTransactionStore, NativeFamilyTargetImage, NativeFamilyTargetWitness,
    NativeOutboundHttp, NativeProcessClock, NativeProductionFamilyReleaseHost,
    NativeWirePrimitives, PreparedNativeFamilyReleaseProofOwner,
    run_native_family_release_production_command,
};

const CAPACITY_MAXIMUM_BYTES: usize = 64 * 1_024;
const EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
const SERVICE_UID: u32 = 10_002;
const SERVICE_GID: u32 = 10_002;
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

/// Fixed host observations required by the read-only target-authority proof.
pub struct NativeInstalledFamilyReleaseTargetRead<Files> {
    files: Files,
    capacity_root: PathBuf,
    capacity_uid: u32,
    capacity_gid: u32,
    refresh_capacity: bool,
}

impl NativeInstalledFamilyReleaseTargetRead<crate::NativeFamilyReleaseCurrentHostFiles> {
    /// Bind the installed root-owned release, settings, catalog, and capacity namespaces.
    pub fn native() -> Self {
        let mut owner = Self::for_parts(
            crate::NativeFamilyReleaseCurrentHostFiles::native(0, 0),
            PathBuf::from("/opt/weather/current/deploy/state/adjustment-release-state"),
            0,
            0,
        );
        owner.refresh_capacity = true;
        owner
    }
}

impl<Files> NativeInstalledFamilyReleaseTargetRead<Files> {
    /// Bind explicit read owners and capacity custody for production composition or fixtures.
    pub fn for_parts(
        files: Files,
        capacity_root: PathBuf,
        capacity_uid: u32,
        capacity_gid: u32,
    ) -> Self {
        Self {
            files,
            capacity_root,
            capacity_uid,
            capacity_gid,
            refresh_capacity: false,
        }
    }
}

impl<Files> NativeFamilyReleaseTargetAuthorityRead for NativeInstalledFamilyReleaseTargetRead<Files>
where
    Files: NativeFamilyReleaseCurrentFiles,
{
    /// Read the installed catalog through the same bounded host-file owner as current authority.
    fn read_current_catalog(&mut self) -> Result<Vec<u8>, NativeFamilyReleaseTargetAuthorityError> {
        self.files
            .read_catalog()
            .map_err(|_| NativeFamilyReleaseTargetAuthorityError::Current)
    }

    /// Read the installed settings through the same bounded host-file owner as current authority.
    fn read_current_settings(
        &mut self,
    ) -> Result<Vec<u8>, NativeFamilyReleaseTargetAuthorityError> {
        self.files
            .read_settings()
            .map_err(|_| NativeFamilyReleaseTargetAuthorityError::Current)
    }

    /// Decode one independently selected root-owned release environment without shell evaluation.
    fn read_release_environment(
        &mut self,
        release: &str,
    ) -> Result<JsonValue, NativeFamilyReleaseTargetAuthorityError> {
        let bytes = self
            .files
            .read_environment(release)
            .map_err(|_| NativeFamilyReleaseTargetAuthorityError::Current)?;
        parse_release_environment(&bytes, release)
    }

    /// Renew only an expired exact pre-pull receipt after source CAS and before the proof clock.
    fn prepare_capacity(
        &mut self,
        input: &NativeFamilyReleaseInput,
        current: &NativeFamilyReleaseCurrent,
    ) -> Result<(), NativeFamilyReleaseTargetAuthorityError> {
        use NativeFamilyReleaseTargetAuthorityError as Error;
        // fixtures retain their explicit read boundary and never initiate host measurements
        if !self.refresh_capacity {
            return Ok(());
        }
        let previous = self.read_capacity(&input.action_sha256)?;
        let measured = text(&previous, "measuredAt").ok_or(Error::Capacity)?;
        let source = self.read_release_environment(&current.release)?;
        let target = self.read_release_environment(&input.target_release)?;
        let compensation = self.read_release_environment(&input.compensating_release)?;
        // independently bind the retained pre-pull sample to all six immutable release roles
        crate::native_family_release_target_authority::validate_capacity(
            &previous,
            &NativeFamilyReleaseTargetAuthorityInput {
                action_sha256: &input.action_sha256,
                compensating_release: &input.compensating_release,
                family: &input.family,
                fencing_token: &input.fencing_token,
                now: &measured,
                receipt_created_at: &measured,
                report_sha256: &input.report_sha256,
                target_release: &input.target_release,
            },
            current,
            &source,
            &target,
            &compensation,
        )?;
        let mut clock = NativeProcessClock;
        let now = clock.canonical_now().ok_or(Error::Capacity)?;
        let age = NativeWirePrimitives
            .canonical_utc_milliseconds(&now)
            .and_then(|now| {
                NativeWirePrimitives
                    .canonical_utc_milliseconds(&measured)
                    .and_then(|measured| now.checked_sub(measured))
            })
            .ok_or(Error::Capacity)?;
        // never repair a sample measured in the future
        if age < 0 {
            return Err(Error::Capacity);
        }
        // preserve a still-fresh pre-pull sample without re-reading registry layers
        if age <= 900_000 {
            return Ok(());
        }
        let mut refs = Vec::with_capacity(6);
        // keep the predecessor's exact source, target, compensation role order
        for environment in [&source, &target, &compensation] {
            refs.push(text(environment, "WEATHER_SERVER_IMAGE").ok_or(Error::Capacity)?);
            refs.push(text(environment, "WEATHER_WEB_IMAGE").ok_or(Error::Capacity)?);
        }
        let receipt = crate::collect_native_family_release_capacity_from_installed([
            &input.action_sha256,
            &input.family,
            &current.release,
            &refs[0],
            &refs[1],
            &refs[2],
            &refs[3],
            &refs[4],
            &refs[5],
        ])
        .map_err(|_| Error::Capacity)?;
        let bytes = receipt.canonical_bytes().ok_or(Error::Capacity)?;
        let previous_bytes = NativeWirePrimitives
            .encode_sorted_json(&previous)
            .ok_or(Error::Capacity)?;
        let ready = receipt.exit_code() == 0;
        NativeFamilyReleaseTransactionStore::native()
            .and_then(|store| {
                store.refresh_capacity_receipt(&input.action_sha256, &previous_bytes, &bytes, ready)
            })
            .map_err(|_| Error::Capacity)?;
        // refuse the measured blocked state while retaining its actual receipt
        if !ready {
            return Err(Error::Capacity);
        }
        Ok(())
    }

    /// Read one exact content-addressed root-owned canonical capacity receipt.
    fn read_capacity(
        &mut self,
        action_sha256: &str,
    ) -> Result<JsonValue, NativeFamilyReleaseTargetAuthorityError> {
        // close the filename before observing the fixed capacity namespace
        if !valid_hash(action_sha256) {
            return Err(NativeFamilyReleaseTargetAuthorityError::Capacity);
        }
        let path = self
            .capacity_root
            .join(format!("capacity-sha256-{action_sha256}.json"));
        let bytes = read_private_capacity_file(
            &path,
            self.capacity_uid,
            self.capacity_gid,
            CAPACITY_MAXIMUM_BYTES,
        )?;
        let mut codec = NativeWirePrimitives;
        let value = codec
            .parse_canonical_object(&bytes)
            .ok_or(NativeFamilyReleaseTargetAuthorityError::Capacity)?;
        // prohibit alternate JSON spellings before semantic target-authority validation
        if codec.encode_canonical_json(&value).as_deref() != Some(bytes.as_slice())
            || text(&value, "actionSha256").as_deref() != Some(action_sha256)
        {
            return Err(NativeFamilyReleaseTargetAuthorityError::Capacity);
        }
        Ok(value)
    }
}

/// Execute one admitted family release using only installed native production owners.
pub fn run_native_family_release_from_installed(
    input: NativeFamilyReleaseInput,
) -> Result<NativeFamilyReleaseTransactionResult, NativeFamilyReleaseTransactionError> {
    let mut target_host = NativeInstalledFamilyReleaseTargetRead::native();
    let source_environment = target_host
        .read_release_environment(&input.expected_source_release)
        .map_err(|_| NativeFamilyReleaseTransactionError::Proof)?;
    let target_environment = target_host
        .read_release_environment(&input.target_release)
        .map_err(|_| NativeFamilyReleaseTransactionError::Proof)?;
    let compensation_environment = target_host
        .read_release_environment(&input.compensating_release)
        .map_err(|_| NativeFamilyReleaseTransactionError::Proof)?;
    let mut source_image = target_image_from_environment(&source_environment)?;
    let mut target_image = target_image_from_environment(&target_environment)?;
    let mut compensation_image = target_image_from_environment(&compensation_environment)?;

    let client = Arc::new(
        NativeOutboundHttp::new().map_err(|_| NativeFamilyReleaseTransactionError::Proof)?,
    );
    let mut git = NativeCaptureEpochPublicGitJson::new(client);
    let mut authority = NativeFamilyActionAuthorityFiles::for_evidence_root(
        PathBuf::from(EVIDENCE_ROOT),
        SERVICE_UID,
        SERVICE_GID,
        SERVICE_UID,
    );
    let mut witness = NativeFamilyTargetWitness::native();
    let mut verifier = NativeFamilyCatalogImageVerifier::new();
    let mut codec = NativeWirePrimitives;
    let time = NativeWirePrimitives;
    let proof = PreparedNativeFamilyReleaseProofOwner {
        ports: NativeFamilyReleaseTargetAuthorityPorts {
            host: &mut target_host,
            git: &mut git,
            authority: &mut authority,
            source_image: &mut source_image,
            target_image: &mut target_image,
            compensation_image: &mut compensation_image,
            witness: &mut witness,
            verifier: &mut verifier,
        },
        codec: &mut codec,
        time: &time,
    };
    let store = NativeFamilyReleaseTransactionStore::native()?;
    let family = input.family.clone();
    let mut port = NativeFamilyReleaseProductionPort::new(
        store,
        NativeProductionFamilyReleaseHost::native(),
        proof,
        NativeProcessClock,
        family,
    );
    run_native_family_release_production_command(input, &mut port)
}

/// Compensate the sole crash-left installed mutation from its already durable proof.
pub fn recover_native_family_release_from_installed()
-> Result<bool, NativeFamilyReleaseTransactionError> {
    let store = NativeFamilyReleaseTransactionStore::native()?;
    let Some(action_sha256) = store.read_unfinished_recovery_action()? else {
        return Ok(false);
    };
    let mut port = NativeFamilyReleaseProductionPort::new(
        store,
        NativeProductionFamilyReleaseHost::native(),
        (),
        NativeProcessClock,
        String::new(),
    );
    port.reconcile_compensation(&action_sha256)?;
    Ok(true)
}

// bind one concrete image reader to the independently decoded release environment
fn target_image_from_environment(
    environment: &JsonValue,
) -> Result<NativeFamilyTargetImage, NativeFamilyReleaseTransactionError> {
    let image = text(environment, "WEATHER_SERVER_IMAGE")
        .ok_or(NativeFamilyReleaseTransactionError::Proof)?;
    NativeFamilyTargetImage::native(&image).map_err(|_| NativeFamilyReleaseTransactionError::Proof)
}

// parse the exact closed release environment before selecting any immutable image
fn parse_release_environment(
    bytes: &[u8],
    release: &str,
) -> Result<JsonValue, NativeFamilyReleaseTargetAuthorityError> {
    use NativeFamilyReleaseTargetAuthorityError as Error;

    let source = std::str::from_utf8(bytes).map_err(|_| Error::Current)?;
    let body = source.strip_suffix('\n').ok_or(Error::Current)?;
    let lines = body.split('\n').collect::<Vec<_>>();
    // retain exactly eleven nonempty ASCII assignments
    if lines.len() != 11 || lines.iter().any(|line| !valid_environment_line(line)) {
        return Err(Error::Current);
    }
    let mut entries = Vec::with_capacity(lines.len());
    // split only the first separator because a retained value may contain equals signs
    for line in lines {
        let (key, value) = line.split_once('=').ok_or(Error::Current)?;
        entries.push((key.encode_utf16().collect(), json_text(value)));
    }
    let environment = JsonValue::Object(entries);
    if !exact_keys(&environment, ENVIRONMENT_KEYS)
        || text(&environment, "WEATHER_RELEASE").as_deref() != Some(release)
        || !text(&environment, "WEATHER_SERVER_IMAGE")
            .is_some_and(|value| valid_weather_image(&value, "weather-server"))
        || !text(&environment, "WEATHER_WEB_IMAGE")
            .is_some_and(|value| valid_weather_image(&value, "weather-web"))
        || !text(&environment, "POSTGRES_IMAGE").is_some_and(|value| valid_generic_image(&value))
        || !text(&environment, "CLOUDFLARED_IMAGE").is_some_and(|value| valid_generic_image(&value))
        || !text(&environment, "WEATHER_CONTROL_PLANE_SHA256")
            .is_some_and(|value| valid_hash(&value))
        || !text(&environment, "WEATHER_CONTROL_PLANE_VERSION")
            .is_some_and(|value| valid_control_version(&value))
    {
        return Err(Error::Current);
    }
    Ok(environment)
}

// read one exact owner-private capacity file without following its final component
fn read_private_capacity_file(
    path: &Path,
    uid: u32,
    gid: u32,
    maximum: usize,
) -> Result<Vec<u8>, NativeFamilyReleaseTargetAuthorityError> {
    let file =
        open_read_no_follow(path).map_err(|_| NativeFamilyReleaseTargetAuthorityError::Capacity)?;
    let before = file
        .metadata()
        .map_err(|_| NativeFamilyReleaseTargetAuthorityError::Capacity)?;
    let size = usize::try_from(before.size())
        .map_err(|_| NativeFamilyReleaseTargetAuthorityError::Capacity)?;
    // require the predecessor's private single-link bounded file custody
    if !before.is_file()
        || before.nlink() != 1
        || before.uid() != uid
        || before.gid() != gid
        || before.mode() & 0o777 != 0o600
        || !(1..=maximum).contains(&size)
    {
        return Err(NativeFamilyReleaseTargetAuthorityError::Capacity);
    }
    read_stable_capacity_file(file, before, size, maximum)
}

// retain descriptor identity and nanosecond metadata across the complete bounded read
fn read_stable_capacity_file(
    mut file: File,
    before: std::fs::Metadata,
    size: usize,
    maximum: usize,
) -> Result<Vec<u8>, NativeFamilyReleaseTargetAuthorityError> {
    let mut bytes = Vec::with_capacity(size);
    (&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| NativeFamilyReleaseTargetAuthorityError::Capacity)?;
    let after = file
        .metadata()
        .map_err(|_| NativeFamilyReleaseTargetAuthorityError::Capacity)?;
    // reject truncation, growth, replacement, or mutation observed through the descriptor
    if bytes.len() != size
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.size() != after.size()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
    {
        return Err(NativeFamilyReleaseTargetAuthorityError::Capacity);
    }
    Ok(bytes)
}

// close the predecessor's unquoted ASCII environment assignment grammar
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

// admit only one exact immutable Weather image role
fn valid_weather_image(value: &str, role: &str) -> bool {
    value
        .strip_prefix(&format!("ghcr.io/anstosa/{role}@sha256:"))
        .is_some_and(valid_hash)
}

// admit only one immutable dependency image without a shell-sensitive value
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

// retain the original positive one-to-six digit control-plane generation grammar
fn valid_control_version(value: &str) -> bool {
    (1..=6).contains(&value.len())
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

// construct one generated JSON string without normalization
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
