//! Read one stable owner-private family lineage observation for source-closure admission.

use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalClock;

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::family_release_identity::compute_family_release_image_identity;
use crate::family_release_images::FamilyCatalogImageVerification;
use crate::maintenance_canonical_json::{exact_keys, text, valid_hash};
use crate::native_family_release_current::{
    NativeFamilyReleaseCurrentError, NativeFamilyReleaseCurrentFiles,
    NativeFamilyReleaseLiveRuntime, read_native_family_release_current,
};
use crate::native_family_release_lineage::{
    NativeFamilyReleaseLineageError, NativeFamilyReleaseLineageInput,
    NativeFamilyReleaseLineageProjection, NativeFamilyReleaseLineageTransaction,
    project_native_family_release_lineage, verify_native_family_release_ancestor_projection,
};
use crate::native_future_input_seal_current::{
    NativeFutureInputSealCurrentError, NativeFutureInputSealSourceClosure,
};
use crate::{
    CaptureEpochOperatorFile, CaptureEpochOperatorFiles, MaintenanceAnchorStatusJson,
    MaintenanceCanonicalJson, NativeCaptureEpochOperatorFiles, NativeProcessClock,
    NativeWirePrimitives,
};

const STATE_ROOT: &str = "/opt/weather/current/deploy/state/adjustment-release-state";
const MAXIMUM_CURRENT_BYTES: usize = 8 * 1024;
const MAXIMUM_TRANSACTION_BYTES: usize = 512 * 1024;
const MAXIMUM_TRANSACTIONS: usize = 384;
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

/// One bounded host, runtime, witness, clock, or projection failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeFamilyLineageHostError {
    Root,
    Current,
    Inventory,
    Transaction,
    Witness,
    Live,
    Clock,
    Changed,
    Projection,
    Codec,
}

impl Display for NativeFamilyLineageHostError {
    /// Render a stable failure without exposing retained authority material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Root => "family lineage root is unsafe",
            Self::Current => "family lineage current authority is invalid",
            Self::Inventory => "family lineage transaction inventory is invalid",
            Self::Transaction => "family lineage transaction is invalid",
            Self::Witness => "family lineage capture witness is unavailable",
            Self::Live => "family lineage live observation differs",
            Self::Clock => "family lineage verification clock is unavailable",
            Self::Changed => "family lineage authority changed during read",
            Self::Projection => "family lineage source closure differs",
            Self::Codec => "family lineage serialization failed",
        })
    }
}

impl std::error::Error for NativeFamilyLineageHostError {}

/// One filename-bound canonical transaction before JSON admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyLineageRawTransaction {
    pub action_sha256: String,
    pub canonical_bytes: Vec<u8>,
}

/// Read only the fixed current authority and complete bounded transaction inventory.
pub trait NativeFamilyLineageFiles {
    /// Read the required owner-private current authority through a stable descriptor.
    fn read_current(&mut self) -> Result<Vec<u8>, NativeFamilyLineageHostError>;

    /// Read every exact transaction name and stable canonical byte payload in sorted order.
    fn read_transactions(
        &mut self,
    ) -> Result<Vec<NativeFamilyLineageRawTransaction>, NativeFamilyLineageHostError>;
}

/// Fixed owner-private family lineage filesystem reader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyLineageHostFiles {
    root: PathBuf,
    uid: u32,
    gid: u32,
}

impl NativeFamilyLineageHostFiles {
    /// Bind the production root to its root-owned deployment identity.
    pub fn native() -> Self {
        Self::for_root(PathBuf::from(STATE_ROOT), 0, 0)
    }

    /// Bind one disposable or explicitly selected owner-private root.
    pub fn for_root(root: PathBuf, uid: u32, gid: u32) -> Self {
        Self { root, uid, gid }
    }

    /// Require one canonical private directory with the configured owner identity.
    fn require_directory(&self, path: &Path) -> Result<(), NativeFamilyLineageHostError> {
        let metadata =
            fs::symlink_metadata(path).map_err(|_| NativeFamilyLineageHostError::Root)?;
        let canonical = path
            .canonicalize()
            .map_err(|_| NativeFamilyLineageHostError::Root)?;
        // reject links, aliases, foreign ownership and permissive journal roots
        if canonical != path
            || !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != self.uid
            || metadata.gid() != self.gid
            || metadata.mode() & 0o777 != 0o700
        {
            return Err(NativeFamilyLineageHostError::Root);
        }
        Ok(())
    }

    /// Read one exact private file without following or accepting mid-read mutation.
    fn read_private(
        &self,
        path: &Path,
        maximum: usize,
        error: NativeFamilyLineageHostError,
    ) -> Result<Vec<u8>, NativeFamilyLineageHostError> {
        let file = open_read_no_follow(path).map_err(|_| error)?;
        let before = file.metadata().map_err(|_| error)?;
        // preserve the source's regular-file, link, owner, mode and size boundary
        if !before.is_file()
            || before.nlink() != 1
            || before.uid() != self.uid
            || before.gid() != self.gid
            || before.mode() & 0o777 != 0o600
            || !(1..=maximum as u64).contains(&before.size())
        {
            return Err(error);
        }
        read_stable(file, before, maximum).map_err(|_| error)
    }
}

impl Default for NativeFamilyLineageHostFiles {
    /// Select the fixed deployed root and owner identity.
    fn default() -> Self {
        Self::native()
    }
}

impl NativeFamilyLineageFiles for NativeFamilyLineageHostFiles {
    /// Read the exact required current authority after admitting its parent root.
    fn read_current(&mut self) -> Result<Vec<u8>, NativeFamilyLineageHostError> {
        self.require_directory(&self.root)?;
        self.read_private(
            &self.root.join("current.json"),
            MAXIMUM_CURRENT_BYTES,
            NativeFamilyLineageHostError::Current,
        )
    }

    /// Read the complete sorted fixed-name transaction inventory.
    fn read_transactions(
        &mut self,
    ) -> Result<Vec<NativeFamilyLineageRawTransaction>, NativeFamilyLineageHostError> {
        self.require_directory(&self.root)?;
        let transactions_root = self.root.join("transactions");
        self.require_directory(&transactions_root)?;
        let mut names = fs::read_dir(&transactions_root)
            .map_err(|_| NativeFamilyLineageHostError::Inventory)?
            .map(|entry| {
                entry
                    .map_err(|_| NativeFamilyLineageHostError::Inventory)?
                    .file_name()
                    .into_string()
                    .map_err(|_| NativeFamilyLineageHostError::Inventory)
            })
            .collect::<Result<Vec<_>, _>>()?;
        names.sort_unstable();
        // refuse unbounded, foreign or multiply named transaction members
        if names.len() > MAXIMUM_TRANSACTIONS || names.iter().any(|name| !valid_name(name)) {
            return Err(NativeFamilyLineageHostError::Inventory);
        }
        let mut transactions = Vec::with_capacity(names.len());
        // retain every stable transaction beside its filename identity
        for name in names {
            let canonical_bytes = self.read_private(
                &transactions_root.join(&name),
                MAXIMUM_TRANSACTION_BYTES,
                NativeFamilyLineageHostError::Transaction,
            )?;
            transactions.push(NativeFamilyLineageRawTransaction {
                action_sha256: action_from_name(&name)
                    .ok_or(NativeFamilyLineageHostError::Inventory)?
                    .to_owned(),
                canonical_bytes,
            });
        }
        Ok(transactions)
    }
}

/// One live release observation admitted by the existing current/runtime owners.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeFamilyLineageLiveObservation {
    pub commit: String,
    pub release: String,
    pub environment: JsonValue,
}

/// Supply one complete runtime-verified release observation.
pub trait NativeFamilyLineageLiveObservationPort {
    /// Re-read and verify the current release, environment, images, catalog and settings.
    fn observe_verified(
        &mut self,
    ) -> Result<NativeFamilyLineageLiveObservation, NativeFamilyLineageHostError>;
}

/// Compose the existing current-family reader into a full lineage live observation.
pub struct PreparedNativeFamilyLineageLiveObservation<'a, Files, Runtime, Verifier, Codec, Time> {
    pub files: &'a mut Files,
    pub runtime: &'a mut Runtime,
    pub verifier: &'a mut Verifier,
    pub codec: &'a mut Codec,
    pub time: &'a Time,
}

impl<Files, Runtime, Verifier, Codec, Time> NativeFamilyLineageLiveObservationPort
    for PreparedNativeFamilyLineageLiveObservation<'_, Files, Runtime, Verifier, Codec, Time>
where
    Files: NativeFamilyReleaseCurrentFiles,
    Runtime: NativeFamilyReleaseLiveRuntime,
    Verifier: FamilyCatalogImageVerification,
    Codec: MaintenanceAnchorStatusJson,
    Time: CurrentTimeResolver,
{
    /// Reuse the complete production current read, then retain its exact verified environment.
    fn observe_verified(
        &mut self,
    ) -> Result<NativeFamilyLineageLiveObservation, NativeFamilyLineageHostError> {
        let current = read_native_family_release_current(
            "temperature",
            self.files,
            self.runtime,
            self.verifier,
            self.codec,
            self.time,
        )
        .map_err(|_| NativeFamilyLineageHostError::Live)?;
        let release_bytes = self
            .files
            .read_current_release()
            .map_err(|_| NativeFamilyLineageHostError::Live)?;
        let release = parse_release_marker(&release_bytes)?;
        if release != current.release {
            return Err(NativeFamilyLineageHostError::Changed);
        }
        let environment_bytes = self
            .files
            .read_environment(&release)
            .map_err(|_| NativeFamilyLineageHostError::Live)?;
        let environment = parse_environment(&environment_bytes, &release)?;
        // verify the exact re-read environment instead of trusting the earlier observation
        self.runtime
            .verify_running_images(&release, &environment)
            .map_err(|_| NativeFamilyLineageHostError::Live)?;
        Ok(NativeFamilyLineageLiveObservation {
            commit: current.commit,
            release,
            environment,
        })
    }
}

/// Supply the canonical clock sampled after the bounded stable observation.
pub trait NativeFamilyLineageClock {
    /// Return one actual or injected canonical verification instant.
    fn canonical_now(&mut self) -> Option<String>;
}

impl NativeFamilyLineageClock for NativeProcessClock {
    /// Reuse the shared actual process clock and canonical formatter.
    fn canonical_now(&mut self) -> Option<String> {
        RevisionArchiveCanonicalClock::canonical_now(self)
    }
}

/// Actual source-closure owner over fixed files, verified live state and the shared projector.
pub struct NativeFamilyLineageSourceClosure<Files, Witness, Live, Codec, Time, Clock> {
    files: Files,
    witness: Witness,
    live: Live,
    codec: Codec,
    time: Time,
    clock: Clock,
}

impl<Files, Witness, Live, Codec, Time, Clock>
    NativeFamilyLineageSourceClosure<Files, Witness, Live, Codec, Time, Clock>
{
    /// Bind explicit read-only owners without observing authority before the closure request.
    pub const fn new(
        files: Files,
        witness: Witness,
        live: Live,
        codec: Codec,
        time: Time,
        clock: Clock,
    ) -> Self {
        Self {
            files,
            witness,
            live,
            codec,
            time,
            clock,
        }
    }
}

impl<Live>
    NativeFamilyLineageSourceClosure<
        NativeFamilyLineageHostFiles,
        NativeCaptureEpochOperatorFiles,
        Live,
        NativeWirePrimitives,
        NativeWirePrimitives,
        NativeProcessClock,
    >
{
    /// Select the production filesystem, witness, wire, time and clock owners.
    pub fn native(live: Live) -> Self {
        Self::new(
            NativeFamilyLineageHostFiles::native(),
            NativeCaptureEpochOperatorFiles,
            live,
            NativeWirePrimitives,
            NativeWirePrimitives,
            NativeProcessClock,
        )
    }
}

impl<Files, Witness, Live, Codec, Time, Clock> NativeFutureInputSealSourceClosure
    for NativeFamilyLineageSourceClosure<Files, Witness, Live, Codec, Time, Clock>
where
    Files: NativeFamilyLineageFiles,
    Witness: CaptureEpochOperatorFiles,
    Live: NativeFamilyLineageLiveObservationPort,
    Codec: MaintenanceCanonicalJson,
    Time: CurrentTimeResolver,
    Clock: NativeFamilyLineageClock,
{
    /// Verify one source commit against a stable complete terminal lineage observation.
    fn source_is_closed(
        &mut self,
        source_commit: &str,
    ) -> Result<bool, NativeFutureInputSealCurrentError> {
        self.verify_source(source_commit)
            .map_err(|_| NativeFutureInputSealCurrentError::SourceClosure)
    }
}

impl<Files, Witness, Live, Codec, Time, Clock>
    NativeFamilyLineageSourceClosure<Files, Witness, Live, Codec, Time, Clock>
where
    Files: NativeFamilyLineageFiles,
    Witness: CaptureEpochOperatorFiles,
    Live: NativeFamilyLineageLiveObservationPort,
    Codec: MaintenanceCanonicalJson,
    Time: CurrentTimeResolver,
    Clock: NativeFamilyLineageClock,
{
    /// Read and verify one complete stable source-closure observation.
    pub fn verify_source(
        &mut self,
        source_commit: &str,
    ) -> Result<bool, NativeFamilyLineageHostError> {
        self.read_observed(|input, codec, time| {
            verify_native_family_release_ancestor_projection(input, source_commit, codec, time)
        })
    }

    /// Emit the original terminal lineage projection from the same stable retained observation.
    pub fn read_lineage(
        &mut self,
    ) -> Result<NativeFamilyReleaseLineageProjection, NativeFamilyLineageHostError> {
        self.read_observed(|input, codec, time| {
            project_native_family_release_lineage(input, codec, time)
        })
    }

    /// Retain one complete stable host observation and project it without duplicate reads.
    fn read_observed<ResultValue>(
        &mut self,
        project: impl FnOnce(
            &NativeFamilyReleaseLineageInput<'_>,
            &mut Codec,
            &Time,
        ) -> Result<ResultValue, NativeFamilyReleaseLineageError>,
    ) -> Result<ResultValue, NativeFamilyLineageHostError> {
        let before_bytes = self.files.read_current()?;
        let current = parse_canonical(&before_bytes, &mut self.codec)
            .ok_or(NativeFamilyLineageHostError::Current)?;
        let witness_bytes = self
            .witness
            .read(CaptureEpochOperatorFile::Witness)
            .map_err(|_| NativeFamilyLineageHostError::Witness)?;
        let before_live = self.live.observe_verified()?;
        let raw_transactions = self.files.read_transactions()?;
        let mut transactions = Vec::with_capacity(raw_transactions.len());
        // admit each transaction only from its exact stable canonical bytes
        for transaction in raw_transactions {
            let document = parse_canonical(&transaction.canonical_bytes, &mut self.codec)
                .ok_or(NativeFamilyLineageHostError::Transaction)?;
            transactions.push(NativeFamilyReleaseLineageTransaction {
                action_sha256: transaction.action_sha256,
                document,
            });
        }
        let after_live = self.live.observe_verified()?;
        let after_bytes = self.files.read_current()?;
        let current_commit =
            text(&current, "commit").ok_or(NativeFamilyLineageHostError::Current)?;
        let current_release =
            text(&current, "release").ok_or(NativeFamilyLineageHostError::Current)?;
        let current_images =
            text(&current, "imageDigestsSha256").ok_or(NativeFamilyLineageHostError::Current)?;
        let before_images =
            compute_family_release_image_identity(&before_live.environment, &mut self.codec)
                .map_err(|_| NativeFamilyLineageHostError::Codec)?;
        let after_images =
            compute_family_release_image_identity(&after_live.environment, &mut self.codec)
                .map_err(|_| NativeFamilyLineageHostError::Codec)?;
        let before_control = live_control(&before_live.environment)?;
        let after_control = live_control(&after_live.environment)?;
        // recheck current bytes and live release identity around the complete journal walk
        if before_bytes != after_bytes
            || before_live.commit != current_commit
            || after_live.commit != current_commit
            || before_live.release != current_release
            || after_live.release != current_release
            || before_images.as_str() != current_images
            || after_images.as_str() != current_images
            || before_control != after_control
        {
            return Err(NativeFamilyLineageHostError::Changed);
        }
        let verified_at = self
            .clock
            .canonical_now()
            .ok_or(NativeFamilyLineageHostError::Clock)?;
        let input = NativeFamilyReleaseLineageInput {
            control_sha256: &after_control.1,
            control_version: &after_control.0,
            current: &current,
            epoch_witness_bytes: &witness_bytes,
            transactions: &transactions,
            verified_at: &verified_at,
        };
        project(&input, &mut self.codec, &self.time)
            .map_err(|_| NativeFamilyLineageHostError::Projection)
    }
}

/// Parse one complete canonical object without accepting alternate bytes.
fn parse_canonical(bytes: &[u8], codec: &mut impl MaintenanceCanonicalJson) -> Option<JsonValue> {
    let value = codec.parse_canonical_object(bytes)?;
    (codec.encode_canonical_json(&value)?.as_slice() == bytes).then_some(value)
}

/// Extract the two fixed live control identities from an admitted environment.
fn live_control(environment: &JsonValue) -> Result<(String, String), NativeFamilyLineageHostError> {
    let version = text(environment, "WEATHER_CONTROL_PLANE_VERSION")
        .ok_or(NativeFamilyLineageHostError::Live)?;
    let sha256 = text(environment, "WEATHER_CONTROL_PLANE_SHA256")
        .ok_or(NativeFamilyLineageHostError::Live)?;
    Ok((version, sha256))
}

/// Parse the exact ASCII release marker after the predecessor's trim operation.
fn parse_release_marker(bytes: &[u8]) -> Result<String, NativeFamilyLineageHostError> {
    let value = std::str::from_utf8(bytes)
        .ok()
        .filter(|value| value.is_ascii())
        .map(str::trim)
        .filter(|value| valid_release(value))
        .ok_or(NativeFamilyLineageHostError::Live)?;
    Ok(value.to_owned())
}

/// Parse the exact closed eleven-line release environment.
fn parse_environment(
    bytes: &[u8],
    release: &str,
) -> Result<JsonValue, NativeFamilyLineageHostError> {
    let source = std::str::from_utf8(bytes).map_err(|_| NativeFamilyLineageHostError::Live)?;
    let body = source
        .strip_suffix('\n')
        .ok_or(NativeFamilyLineageHostError::Live)?;
    let lines = body.split('\n').collect::<Vec<_>>();
    // retain exactly eleven nonempty ASCII assignment lines
    if lines.len() != 11 || lines.iter().any(|line| !valid_environment_line(line)) {
        return Err(NativeFamilyLineageHostError::Live);
    }
    let mut entries = Vec::with_capacity(lines.len());
    // split only the first assignment separator and preserve the remaining value
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or(NativeFamilyLineageHostError::Live)?;
        entries.push((key.encode_utf16().collect(), json_text(value)));
    }
    let environment = JsonValue::Object(entries);
    if !exact_keys(&environment, ENVIRONMENT_KEYS) {
        return Err(NativeFamilyLineageHostError::Live);
    }
    let server =
        text(&environment, "WEATHER_SERVER_IMAGE").ok_or(NativeFamilyLineageHostError::Live)?;
    let web = text(&environment, "WEATHER_WEB_IMAGE").ok_or(NativeFamilyLineageHostError::Live)?;
    let postgres =
        text(&environment, "POSTGRES_IMAGE").ok_or(NativeFamilyLineageHostError::Live)?;
    let cloudflared =
        text(&environment, "CLOUDFLARED_IMAGE").ok_or(NativeFamilyLineageHostError::Live)?;
    // bind release, application, dependency and control-plane identities
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
        return Err(NativeFamilyLineageHostError::Live);
    }
    Ok(environment)
}

/// Finish one bounded descriptor read and reject mutation while reading.
fn read_stable(
    mut file: File,
    before: std::fs::Metadata,
    maximum: usize,
) -> Result<Vec<u8>, NativeFamilyLineageHostError> {
    let expected =
        usize::try_from(before.size()).map_err(|_| NativeFamilyLineageHostError::Transaction)?;
    let mut bytes = Vec::with_capacity(expected);
    (&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| NativeFamilyLineageHostError::Transaction)?;
    let after = file
        .metadata()
        .map_err(|_| NativeFamilyLineageHostError::Transaction)?;
    // compare descriptor identity, exact length and nanosecond modification clock
    if bytes.len() != expected
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.size() != after.size()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
    {
        return Err(NativeFamilyLineageHostError::Transaction);
    }
    Ok(bytes)
}

/// Recognize one exact transaction filename and lowercase action digest.
fn valid_name(value: &str) -> bool {
    action_from_name(value).is_some_and(valid_hash)
}

/// Extract the action digest without indexing untrusted UTF-8 filename bytes.
fn action_from_name(value: &str) -> Option<&str> {
    value.strip_prefix("sha256-")?.strip_suffix(".json")
}

/// Recognize one immutable release with a one-to-three digit revision.
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

/// Recognize one exact Weather server or web immutable reference.
fn valid_weather_image(value: &str) -> bool {
    value
        .strip_prefix("ghcr.io/anstosa/weather-server@sha256:")
        .or_else(|| value.strip_prefix("ghcr.io/anstosa/weather-web@sha256:"))
        .is_some_and(valid_hash)
}

/// Recognize one generic immutable dependency image.
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

/// Construct one generated UTF-16 string value.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

impl From<NativeFamilyReleaseCurrentError> for NativeFamilyLineageHostError {
    /// Contain all lower current-reader failures at the live observation boundary.
    fn from(_: NativeFamilyReleaseCurrentError) -> Self {
        Self::Live
    }
}

#[cfg(test)]
#[path = "native_family_lineage_host_test.rs"]
mod tests;
