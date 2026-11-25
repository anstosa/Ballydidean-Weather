//! Concrete native family host controller over retained readers and the native release switch.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::maintenance_canonical_json::{exact_keys, text, valid_hash};
use crate::native_family_release_transaction_port::install_native_family_release_catalog;
use crate::{
    FamilyCatalogImageVerification, FamilyReleaseRebindInput, MaintenanceAnchorStatusJson,
    NativeFamilyCatalogImageVerifier, NativeFamilyReleaseCompensationObservation,
    NativeFamilyReleaseCurrentFiles, NativeFamilyReleaseCurrentHostFiles,
    NativeFamilyReleaseCurrentObservation, NativeFamilyReleaseDurableProof,
    NativeFamilyReleaseHostController, NativeFamilyReleaseInspectedCurrent,
    NativeFamilyReleaseLiveObservation, NativeFamilyReleaseLiveRuntime,
    NativeFamilyReleaseLiveRuntimeProcess, NativeFamilyReleaseMutationRecord,
    NativeFamilyReleaseTransactionError, NativeFamilyTargetImage, NativeWirePrimitives,
    read_native_family_release_current, verify_family_release_rebind,
};

const OUTPUT_LIMIT: usize = 64 * 1024;
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

/// Fixed catalog and native-generation release-switch paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseHostControllerPaths {
    pub catalog: PathBuf,
    pub remote_ops: PathBuf,
}

impl NativeFamilyReleaseHostControllerPaths {
    /// Select the installed production catalog and privileged native-generation operator.
    pub fn native() -> Self {
        Self {
            catalog: PathBuf::from(
                "/opt/weather/current/deploy/state/adjustment-candidate-catalog.json",
            ),
            remote_ops: PathBuf::from("/usr/local/sbin/weather-remote-ops"),
        }
    }
}

/// Concrete host controller parameterized only for disposable file/runtime fixtures.
pub struct NativeFamilyReleaseHostProcess<Files, Runtime, Verifier, Codec, Time> {
    pub files: Files,
    pub runtime: Runtime,
    pub verifier: Verifier,
    pub codec: Codec,
    pub time: Time,
    paths: NativeFamilyReleaseHostControllerPaths,
    catalog_uid: u32,
    catalog_gid: u32,
}

/// Fully production-bound family release controller.
pub type NativeProductionFamilyReleaseHost = NativeFamilyReleaseHostProcess<
    NativeFamilyReleaseCurrentHostFiles,
    NativeFamilyReleaseLiveRuntimeProcess,
    NativeFamilyCatalogImageVerifier,
    NativeWirePrimitives,
    NativeWirePrimitives,
>;

impl NativeProductionFamilyReleaseHost {
    /// Bind the fixed root-owned state, Docker observer, catalog verifier, and native switch.
    pub fn native() -> Self {
        Self::for_parts(
            NativeFamilyReleaseCurrentHostFiles::native(0, 0),
            NativeFamilyReleaseLiveRuntimeProcess::native(),
            NativeFamilyCatalogImageVerifier::new(),
            NativeWirePrimitives,
            NativeWirePrimitives,
            NativeFamilyReleaseHostControllerPaths::native(),
            0,
            0,
        )
    }
}

impl<Files, Runtime, Verifier, Codec, Time>
    NativeFamilyReleaseHostProcess<Files, Runtime, Verifier, Codec, Time>
{
    /// Compose explicitly selected owners without observing host state.
    #[allow(clippy::too_many_arguments)]
    pub fn for_parts(
        files: Files,
        runtime: Runtime,
        verifier: Verifier,
        codec: Codec,
        time: Time,
        paths: NativeFamilyReleaseHostControllerPaths,
        catalog_uid: u32,
        catalog_gid: u32,
    ) -> Self {
        Self {
            files,
            runtime,
            verifier,
            codec,
            time,
            paths,
            catalog_uid,
            catalog_gid,
        }
    }
}

impl<Files, Runtime, Verifier, Codec, Time> NativeFamilyReleaseHostController
    for NativeFamilyReleaseHostProcess<Files, Runtime, Verifier, Codec, Time>
where
    Files: NativeFamilyReleaseCurrentFiles,
    Runtime: NativeFamilyReleaseLiveRuntime,
    Verifier: FamilyCatalogImageVerification,
    Codec: MaintenanceAnchorStatusJson,
    Time: CurrentTimeResolver,
{
    /// Inspect current authority through the existing exact files/runtime/catalog owner.
    fn inspect_current(
        &mut self,
        family: &str,
    ) -> Result<NativeFamilyReleaseInspectedCurrent, NativeFamilyReleaseTransactionError> {
        let authority = read_native_family_release_current(
            family,
            &mut self.files,
            &mut self.runtime,
            &mut self.verifier,
            &mut self.codec,
            &self.time,
        )
        .map_err(|_| NativeFamilyReleaseTransactionError::SourceCompareAndSwap)?;
        let settings_bytes = self
            .files
            .read_settings()
            .map_err(|_| NativeFamilyReleaseTransactionError::SourceCompareAndSwap)?;
        Ok(NativeFamilyReleaseInspectedCurrent {
            observation: NativeFamilyReleaseCurrentObservation {
                commit: authority.commit.clone(),
                release: authority.release.clone(),
                settings_bytes,
                maximum_fence: String::new(),
                active_installed_receipt_sha256: authority.active_installed_receipt_sha256.clone(),
                shadow_installed_receipt_sha256: authority.shadow_installed_receipt_sha256.clone(),
                control_installed_receipt_sha256: authority
                    .control_installed_receipt_sha256
                    .clone(),
            },
            authority,
        })
    }

    /// Rebind, install, read back, and switch only the verified native target release.
    fn apply_target(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseDurableProof,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        self.rebind(
            &record.input.target_release,
            &record.input.family,
            &proof.target_catalog_bytes,
            &proof.target_family_identity_sha256,
            &proof.target_image_digests_sha256,
        )?;
        self.install_catalog(&proof.target_catalog_bytes)?;
        self.switch_release(
            &record.input.target_release,
            &record.input.expected_current_release,
            &record.input.expected_current_release,
            &proof.target_commit,
            &proof.target_catalog_bytes,
            &proof.target_family_identity_sha256,
            &proof.target_image_digests_sha256,
            &record.input.action_sha256,
        )
    }

    /// Verify the target marker, runtime, catalog bytes, physical family, images, and settings.
    fn verify_live(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseDurableProof,
    ) -> Result<NativeFamilyReleaseLiveObservation, NativeFamilyReleaseTransactionError> {
        self.observe_live(
            &record.input.target_release,
            &proof.target_commit,
            &record.input.family,
            &proof.target_catalog_bytes,
            &proof.target_family_identity_sha256,
            &proof.target_image_digests_sha256,
        )
    }

    /// Rebind, install, switch, and independently observe only the prebuilt compensation.
    fn compensate(
        &mut self,
        record: &NativeFamilyReleaseMutationRecord,
        proof: &NativeFamilyReleaseDurableProof,
    ) -> Result<NativeFamilyReleaseCompensationObservation, NativeFamilyReleaseTransactionError>
    {
        self.rebind(
            &record.input.compensating_release,
            &record.input.family,
            &proof.baseline_catalog_bytes,
            &proof.compensation_family_identity_sha256,
            &proof.compensation_image_digests_sha256,
        )?;
        self.install_catalog(&proof.baseline_catalog_bytes)?;
        self.switch_release(
            &record.input.compensating_release,
            &record.input.expected_current_release,
            &record.input.target_release,
            &proof.compensation_commit,
            &proof.baseline_catalog_bytes,
            &proof.compensation_family_identity_sha256,
            &proof.compensation_image_digests_sha256,
            &record.input.action_sha256,
        )?;
        let live = self.observe_live(
            &record.input.compensating_release,
            &proof.compensation_commit,
            &record.input.family,
            &proof.baseline_catalog_bytes,
            &proof.compensation_family_identity_sha256,
            &proof.compensation_image_digests_sha256,
        )?;
        Ok(NativeFamilyReleaseCompensationObservation {
            action_sha256: proof.compensation_action_sha256.clone(),
            live,
        })
    }
}

impl<Files, Runtime, Verifier, Codec, Time>
    NativeFamilyReleaseHostProcess<Files, Runtime, Verifier, Codec, Time>
where
    Files: NativeFamilyReleaseCurrentFiles,
    Runtime: NativeFamilyReleaseLiveRuntime,
    Verifier: FamilyCatalogImageVerification,
    Codec: MaintenanceAnchorStatusJson,
    Time: CurrentTimeResolver,
{
    /// Rebind immutable environment, registry, catalog, family, and image identities before mutation.
    fn rebind(
        &mut self,
        release: &str,
        family: &str,
        catalog_bytes: &[u8],
        family_identity_sha256: &str,
        image_digests_sha256: &str,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        let environment = self.read_environment(release)?;
        let server = text(&environment, "WEATHER_SERVER_IMAGE")
            .ok_or(NativeFamilyReleaseTransactionError::Apply)?;
        let mut image = NativeFamilyTargetImage::native(&server)
            .map_err(|_| NativeFamilyReleaseTransactionError::Apply)?;
        verify_family_release_rebind(
            &FamilyReleaseRebindInput {
                family,
                catalog_bytes,
                environment: &environment,
                expected_family_sha256: family_identity_sha256,
                expected_images_sha256: image_digests_sha256,
            },
            &mut image,
            &mut self.verifier,
            &mut self.codec,
            &self.time,
        )
        .map_err(|_| NativeFamilyReleaseTransactionError::Apply)
    }

    /// Install exact catalog bytes atomically and reject substitution before the native switch.
    fn install_catalog(&mut self, bytes: &[u8]) -> Result<(), NativeFamilyReleaseTransactionError> {
        install_native_family_release_catalog(
            &self.paths.catalog,
            bytes,
            self.catalog_uid,
            self.catalog_gid,
        )
        .map_err(|_| NativeFamilyReleaseTransactionError::Apply)?;
        let installed = self
            .files
            .read_catalog()
            .map_err(|_| NativeFamilyReleaseTransactionError::Apply)?;
        // bind the exact installed inode bytes before the first runtime mutation
        if installed != bytes {
            return Err(NativeFamilyReleaseTransactionError::Apply);
        }
        Ok(())
    }

    /// Invoke only the root-owned native-generation switch with every verified proof identity.
    #[allow(clippy::too_many_arguments)]
    fn switch_release(
        &mut self,
        release: &str,
        expected_current: &str,
        alternate_current: &str,
        commit: &str,
        catalog_bytes: &[u8],
        family_identity_sha256: &str,
        image_digests_sha256: &str,
        action_sha256: &str,
    ) -> Result<(), NativeFamilyReleaseTransactionError> {
        let catalog_sha256 = self
            .codec
            .sha256(catalog_bytes)
            .filter(|value| valid_hash(value.as_str()))
            .ok_or(NativeFamilyReleaseTransactionError::Apply)?;
        run_native_switch(
            &self.paths.remote_ops,
            &[
                release,
                expected_current,
                alternate_current,
                commit,
                catalog_sha256.as_str(),
                family_identity_sha256,
                image_digests_sha256,
                action_sha256,
            ],
        )
    }

    /// Observe one applied release without depending on the not-yet-written current acknowledgement.
    fn observe_live(
        &mut self,
        release: &str,
        commit: &str,
        family: &str,
        catalog_bytes: &[u8],
        family_identity_sha256: &str,
        image_digests_sha256: &str,
    ) -> Result<NativeFamilyReleaseLiveObservation, NativeFamilyReleaseTransactionError> {
        let marker = self
            .files
            .read_current_release()
            .map_err(|_| NativeFamilyReleaseTransactionError::LiveVerification)?;
        // require the exact LF-terminated committed marker before runtime observation
        if marker != format!("{release}\n").as_bytes() {
            return Err(NativeFamilyReleaseTransactionError::LiveVerification);
        }
        let environment = self.read_environment(release)?;
        self.runtime
            .verify_running_images(release, &environment)
            .map_err(|_| NativeFamilyReleaseTransactionError::LiveVerification)?;
        let installed = self
            .files
            .read_catalog()
            .map_err(|_| NativeFamilyReleaseTransactionError::LiveVerification)?;
        // require exact catalog bytes rather than semantic equivalence
        if installed != catalog_bytes {
            return Err(NativeFamilyReleaseTransactionError::LiveVerification);
        }
        self.rebind(
            release,
            family,
            catalog_bytes,
            family_identity_sha256,
            image_digests_sha256,
        )
        .map_err(|_| NativeFamilyReleaseTransactionError::LiveVerification)?;
        let settings_bytes = self
            .files
            .read_settings()
            .map_err(|_| NativeFamilyReleaseTransactionError::LiveVerification)?;
        Ok(NativeFamilyReleaseLiveObservation {
            commit: commit.to_owned(),
            family: family.to_owned(),
            family_identity_sha256: family_identity_sha256.to_owned(),
            image_digests_sha256: image_digests_sha256.to_owned(),
            release: release.to_owned(),
            settings_bytes,
        })
    }

    /// Read and admit one exact fixed release environment through the retained file owner.
    fn read_environment(
        &mut self,
        release: &str,
    ) -> Result<JsonValue, NativeFamilyReleaseTransactionError> {
        let bytes = self
            .files
            .read_environment(release)
            .map_err(|_| NativeFamilyReleaseTransactionError::Apply)?;
        parse_environment(&bytes, release)
    }
}

/// Run the exact native switch command for an ordinary disposable command fixture.
#[cfg(test)]
pub(crate) fn run_native_family_release_switch_fixture(
    program: &Path,
    arguments: [&str; 8],
) -> Result<(), NativeFamilyReleaseTransactionError> {
    run_native_switch(program, &arguments)
}

// run the root-owned native-generation switch with bounded output and inherited lock environment
fn run_native_switch(
    program: &Path,
    arguments: &[&str; 8],
) -> Result<(), NativeFamilyReleaseTransactionError> {
    let mut child = Command::new(program)
        .arg("--family-apply-native-unlocked")
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| NativeFamilyReleaseTransactionError::Apply)?;
    capture_bounded(&mut child, OUTPUT_LIMIT)
}

enum Captured {
    Output(io::Result<()>),
    Diagnostic(io::Result<()>),
}

// drain both child streams concurrently and reject aggregate output overflow
fn capture_bounded(
    child: &mut Child,
    maximum: usize,
) -> Result<(), NativeFamilyReleaseTransactionError> {
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(NativeFamilyReleaseTransactionError::Apply);
    };
    let (sender, receiver) = mpsc::channel();
    let output_sender = sender.clone();
    let total = AtomicUsize::new(0);
    let combined = &total;
    let captured = thread::scope(|scope| {
        // drain stdout without retaining privileged diagnostics
        scope.spawn(move || {
            let result = read_pipe(stdout, maximum, combined);
            let _ = output_sender.send(Captured::Output(result));
        });
        // drain stderr independently so the child cannot deadlock
        scope.spawn(move || {
            let result = read_pipe(stderr, maximum, combined);
            let _ = sender.send(Captured::Diagnostic(result));
        });
        let mut failed = false;
        // consume exactly one terminal result from each child pipe
        for _ in 0..2 {
            match receiver.recv() {
                Ok(Captured::Output(Ok(())) | Captured::Diagnostic(Ok(()))) => {}
                _ => {
                    failed = true;
                    let _ = child.kill();
                }
            }
        }
        !failed
    });
    let status = child
        .wait()
        .map_err(|_| NativeFamilyReleaseTransactionError::Apply)?;
    // publish success only after bounded EOF and ordinary zero exit
    if !captured || !status.success() {
        return Err(NativeFamilyReleaseTransactionError::Apply);
    }
    Ok(())
}

// consume one child pipe within the aggregate output bound
fn read_pipe(mut stream: impl Read, maximum: usize, total: &AtomicUsize) -> io::Result<()> {
    let mut buffer = [0_u8; 8192];
    // stop only after EOF or the first aggregate overflow
    loop {
        let remaining = maximum.saturating_sub(total.load(Ordering::Relaxed));
        let requested = (remaining + 1).min(buffer.len());
        let count = match stream.read(&mut buffer[..requested]) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        // an empty read ends only this owned child pipe
        if count == 0 {
            return Ok(());
        }
        let prior = total.fetch_add(count, Ordering::Relaxed);
        // reject the first byte beyond the shared output allowance
        if prior + count > maximum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native family release output exceeds bound",
            ));
        }
    }
}

// parse one exact fixed release environment without accepting shell expansion or aliases
fn parse_environment(
    bytes: &[u8],
    release: &str,
) -> Result<JsonValue, NativeFamilyReleaseTransactionError> {
    let source =
        std::str::from_utf8(bytes).map_err(|_| NativeFamilyReleaseTransactionError::Apply)?;
    let body = source
        .strip_suffix('\n')
        .ok_or(NativeFamilyReleaseTransactionError::Apply)?;
    let lines = body.split('\n').collect::<Vec<_>>();
    // retain exactly eleven nonempty ASCII assignment lines
    if lines.len() != 11 || lines.iter().any(|line| !valid_environment_line(line)) {
        return Err(NativeFamilyReleaseTransactionError::Apply);
    }
    let mut fields = Vec::with_capacity(lines.len());
    // split only the first assignment separator and preserve the remaining value
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or(NativeFamilyReleaseTransactionError::Apply)?;
        fields.push((key.encode_utf16().collect(), json_text(value)));
    }
    let environment = JsonValue::Object(fields);
    let server = text(&environment, "WEATHER_SERVER_IMAGE")
        .ok_or(NativeFamilyReleaseTransactionError::Apply)?;
    let web = text(&environment, "WEATHER_WEB_IMAGE")
        .ok_or(NativeFamilyReleaseTransactionError::Apply)?;
    let postgres =
        text(&environment, "POSTGRES_IMAGE").ok_or(NativeFamilyReleaseTransactionError::Apply)?;
    let cloudflared = text(&environment, "CLOUDFLARED_IMAGE")
        .ok_or(NativeFamilyReleaseTransactionError::Apply)?;
    // bind the closed environment, release, application images, and control-plane identity
    if !exact_keys(&environment, ENVIRONMENT_KEYS)
        || text(&environment, "WEATHER_RELEASE").as_deref() != Some(release)
        || !valid_weather_image(&server, "server")
        || !valid_weather_image(&web, "web")
        || !valid_generic_image(&postgres)
        || !valid_generic_image(&cloudflared)
        || !text(&environment, "WEATHER_CONTROL_PLANE_SHA256")
            .is_some_and(|value| valid_hash(&value))
        || !text(&environment, "WEATHER_CONTROL_PLANE_VERSION")
            .is_some_and(|value| valid_control_version(&value))
    {
        return Err(NativeFamilyReleaseTransactionError::Apply);
    }
    Ok(environment)
}

// recognize one exact fixed environment assignment without shell grammar
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

// recognize one role-correct immutable Weather image reference
fn valid_weather_image(value: &str, role: &str) -> bool {
    value
        .strip_prefix(&format!("ghcr.io/anstosa/weather-{role}@sha256:"))
        .is_some_and(valid_hash)
}

// recognize one generic immutable dependency image without whitespace or a second separator
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

// recognize one positive one-to-six digit control-plane generation
fn valid_control_version(value: &str) -> bool {
    (1..=6).contains(&value.len())
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

// construct one exact UTF-16 environment string
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
