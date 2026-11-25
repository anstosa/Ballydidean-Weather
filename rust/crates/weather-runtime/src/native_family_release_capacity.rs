//! native family-release capacity measurement and exact receipt evaluation.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt::{self, Display};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use flate2::read::GzDecoder;
use hyper::Method;
use hyper::header::{ACCEPT, AUTHORIZATION, HeaderValue, LOCATION, WWW_AUTHENTICATE};
use sha2::{Digest as _, Sha256};
use url::Url;
use weather_contracts::JsonValue;
use weather_edge::{
    ArchiveWirePrimitives, ForecastAllocationStatfsPort, RevisionArchiveCanonicalCodec,
};

use crate::maintenance_canonical_json::{exact_keys, field, text};
use crate::{
    NativeForecastAllocationStatfs, NativeOutboundHttp, NativeOutboundHttpError,
    NativeOutboundHttpRequest, NativeOutboundHttpResponse, NativeWirePrimitives,
    system_worker_health_milliseconds,
};

const BLOCK_BYTES: u64 = 4_096;
const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const PROTECTED_FREE_BYTES: u64 = 2_030_043_136;
const NEXT_CAPTURE_BYTES: u64 = 4_112_384;
const RUNTIME_PACKAGE_MAXIMUM_BYTES: u64 = 8 * 1_024 * 1_024;
const MAXIMUM_OWNED_BYTES: u64 = 4 * 1_024 * 1_024 * 1_024;
const FUTURE_STATE_BYTES: u64 = 1_024 * 1_024;
const ENGINE_METADATA_BYTES: u64 = 64 * 1_024 * 1_024;
const PULL_SCRATCH_BYTES: u64 = 64 * 1_024 * 1_024;
const COMPATIBILITY_FIXTURE_BYTES: u64 = 64 * 1_024 * 1_024;
const COMPATIBILITY_FIXTURE_INODES: u64 = 4_096;
const ENGINE_METADATA_INODES: u64 = 4_096;
const CONTROL_GROWTH_INODES: u64 = 128;
const NEXT_STATE_INODES: u64 = 1_004;
const INODE_FLOOR: u64 = 32_768;
const MAXIMUM_INVENTORY: usize = 256;
const MAXIMUM_LAYERS: usize = 128;
const MAXIMUM_COMPRESSED_IMAGE_BYTES: u64 = 2 * 1_024 * 1_024 * 1_024;
const MAXIMUM_UNPACKED_LAYER_BYTES: u64 = 4 * 1_024 * 1_024 * 1_024;
const MAXIMUM_LAYER_INODES: u64 = 1_000_000;
const COMPENSATION_SCOPE: &str = "family-only-new-release-compensation";
const CONTRACT_VERSION: &str = "adjustment-family-release-capacity/v1";
const DASHBOARD_COMPENSATION_SCOPE: &str = "fixed-core-web-only-source-restore";
const DASHBOARD_CONTRACT_VERSION: &str = "dashboard-web-release-capacity/v1";
const REGISTRY_TIMEOUT: Duration = Duration::from_secs(300);
const TOKEN_TIMEOUT: Duration = Duration::from_secs(30);
const DOCKER_OUTPUT_MAXIMUM: usize = 8 * 1_024 * 1_024;
const CURL: &str = "/usr/bin/curl";
const CONTROL_ROOT: &str = "/opt/weather/current";
const CAPACITY_ROOT: &str = "/var/lib/weather";
const DOCKER_LAYER_ROOT: &str = "/var/lib/docker/overlay2/";

/// one remote OCI layer measured without retaining its blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFamilyReleaseRemoteLayer {
    pub allocated_bytes: u64,
    pub blob_digest: String,
    pub entry_inodes: u64,
}

/// raw immutable OCI evidence returned by the admitted remote measurement port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFamilyReleaseRemoteImage {
    pub config_bytes: Vec<u8>,
    pub manifest_bytes: Vec<u8>,
    pub unpacked_layers: Vec<NativeFamilyReleaseRemoteLayer>,
}

/// one exact locally allocated Docker parent chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFamilyReleaseLocalLayer {
    pub allocated_bytes: u64,
    pub chain_id: String,
}

/// one filesystem capacity observation for `/var/lib/weather`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeFamilyReleaseFilesystemCapacity {
    pub free_bytes: u64,
    pub free_inodes: u64,
}

/// closed host and registry effects needed by the nine-operand command.
pub trait NativeFamilyReleaseCapacityMeasurementPort {
    type Error: Display;

    /// fetch and measure one pinned remote OCI image.
    fn remote_image(
        &mut self,
        reference: &str,
    ) -> Result<NativeFamilyReleaseRemoteImage, Self::Error>;

    /// inspect one optional local image against its expected config and diff ids.
    fn local_image(
        &mut self,
        reference: &str,
        config_digest: &str,
        diff_ids: &[String],
    ) -> Result<Vec<NativeFamilyReleaseLocalLayer>, Self::Error>;

    /// measure the fixed release filesystem without prospective deletion credit.
    fn filesystem_capacity(&mut self)
    -> Result<NativeFamilyReleaseFilesystemCapacity, Self::Error>;

    /// measure the fixed release/control tree without following links.
    fn retained_control_bytes(&mut self) -> Result<u64, Self::Error>;

    /// sample one canonical millisecond UTC observation after measurements.
    fn measured_at(&mut self) -> Result<String, Self::Error>;
}

/// one normalized layer retained by the pure capacity evaluator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFamilyReleaseCapacityLayer {
    pub blob_digest: String,
    pub chain_id: String,
    pub compressed_bytes: u64,
    pub diff_id: String,
    pub entry_inodes: u64,
    pub unpacked_bytes: u64,
}

/// one normalized role/runtime image retained by the pure evaluator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFamilyReleaseCapacityImage {
    pub digest: String,
    pub layers: Vec<NativeFamilyReleaseCapacityLayer>,
    pub metadata_bytes: u64,
    pub role: &'static str,
    pub runtime: &'static str,
}

/// complete exact input to the source capacity formula.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFamilyReleaseCapacityMeasurement {
    pub action_sha256: String,
    pub family: String,
    pub free_bytes: u64,
    pub free_inodes: u64,
    pub images: Vec<NativeFamilyReleaseCapacityImage>,
    pub inventory: Vec<NativeFamilyReleaseLocalLayer>,
    pub measured_at: String,
    pub retained_control_bytes: u64,
    pub runtime_package_bytes: u64,
    pub source_release: String,
}

/// one exact digest coordinate in the emitted receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFamilyReleaseCapacityDigest {
    pub digest: String,
    pub role: &'static str,
    pub runtime: &'static str,
}

/// source-exact ready or denied capacity receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFamilyReleaseCapacityReceipt {
    pub action_sha256: String,
    pub available_free_bytes: u64,
    pub compressed_peak_bytes: u64,
    pub family: String,
    pub future_state_bytes: u64,
    pub image_digests: Vec<NativeFamilyReleaseCapacityDigest>,
    pub measured_at: String,
    pub new_layer_inodes: u64,
    pub persistent_growth_bytes: u64,
    pub persistent_owned_bytes: u64,
    pub pull_peak_growth_bytes: u64,
    pub reasons: Vec<&'static str>,
    pub release_transaction_peak_growth_bytes: u64,
    pub required_free_bytes: u64,
    pub required_free_inodes: u64,
    pub retained_control_bytes: u64,
    pub source_release: String,
}

impl NativeFamilyReleaseCapacityReceipt {
    /// return the predecessor command status while retaining denied stdout.
    pub fn exit_code(&self) -> i32 {
        // retain the closed branch
        if self.reasons.is_empty() { 0 } else { 3 }
    }

    /// project the exact recursively sortable receipt document.
    pub fn to_json(&self) -> JsonValue {
        object(vec![
            ("actionSha256", string(&self.action_sha256)),
            ("availableFreeBytes", number(self.available_free_bytes)),
            ("captureReservedBytes", number(NEXT_CAPTURE_BYTES)),
            (
                "compatibilityFixtureBytes",
                number(COMPATIBILITY_FIXTURE_BYTES),
            ),
            (
                "compatibilityFixtureInodes",
                number(COMPATIBILITY_FIXTURE_INODES),
            ),
            ("compensationScope", string(COMPENSATION_SCOPE)),
            ("compressedPeakBytes", number(self.compressed_peak_bytes)),
            ("contractVersion", string(CONTRACT_VERSION)),
            ("family", string(&self.family)),
            ("futureStateBytes", number(self.future_state_bytes)),
            (
                "imageDigests",
                JsonValue::Array(
                    self.image_digests
                        .iter()
                        .map(|image| {
                            object(vec![
                                ("digest", string(&image.digest)),
                                ("role", string(image.role)),
                                ("runtime", string(image.runtime)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("measuredAt", string(&self.measured_at)),
            ("newLayerInodes", number(self.new_layer_inodes)),
            (
                "persistentGrowthBytes",
                number(self.persistent_growth_bytes),
            ),
            ("persistentOwnedBytes", number(self.persistent_owned_bytes)),
            ("protectedFreeBytes", number(PROTECTED_FREE_BYTES)),
            ("pullPeakGrowthBytes", number(self.pull_peak_growth_bytes)),
            (
                "releaseTransactionPeakGrowthBytes",
                number(self.release_transaction_peak_growth_bytes),
            ),
            (
                "reasons",
                JsonValue::Array(self.reasons.iter().map(|reason| string(reason)).collect()),
            ),
            ("requiredFreeBytes", number(self.required_free_bytes)),
            ("requiredFreeInodes", number(self.required_free_inodes)),
            ("retainedControlBytes", number(self.retained_control_bytes)),
            ("retirementCreditBytes", number(0)),
            ("sourceRelease", string(&self.source_release)),
            (
                "state",
                string(if self.reasons.is_empty() {
                    "capacity_ready"
                } else {
                    "capacity_blocked"
                }),
            ),
        ])
    }

    /// encode the exact canonical stdout line for weather-evidence.
    pub fn canonical_bytes(&self) -> Option<Vec<u8>> {
        NativeWirePrimitives.encode_sorted_json(&self.to_json())
    }
}

/// dashboard authority wrapped around the shared exact physical calculation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDashboardReleaseCapacityReceipt {
    pub core_release: String,
    pub publication_sha256: String,
    capacity: NativeFamilyReleaseCapacityReceipt,
}

impl NativeDashboardReleaseCapacityReceipt {
    /// return the predecessor status while retaining a complete blocked receipt.
    pub fn exit_code(&self) -> i32 {
        self.capacity.exit_code()
    }

    /// project the dashboard policy and shared physical measurement.
    pub fn to_json(&self) -> JsonValue {
        let capacity = &self.capacity;
        // preserve the source capacity-state spelling
        let state = if capacity.reasons.is_empty() {
            "capacity_ready"
        } else {
            "capacity_blocked"
        };
        object(vec![
            ("availableFreeBytes", number(capacity.available_free_bytes)),
            ("captureReservedBytes", number(NEXT_CAPTURE_BYTES)),
            (
                "compatibilityFixtureBytes",
                number(COMPATIBILITY_FIXTURE_BYTES),
            ),
            (
                "compatibilityFixtureInodes",
                number(COMPATIBILITY_FIXTURE_INODES),
            ),
            ("compensationScope", string(DASHBOARD_COMPENSATION_SCOPE)),
            (
                "compressedPeakBytes",
                number(capacity.compressed_peak_bytes),
            ),
            ("contractVersion", string(DASHBOARD_CONTRACT_VERSION)),
            ("futureStateBytes", number(capacity.future_state_bytes)),
            (
                "imageDigests",
                JsonValue::Array(
                    capacity
                        .image_digests
                        .iter()
                        .map(|image| {
                            object(vec![
                                ("digest", string(&image.digest)),
                                ("role", string(image.role)),
                                ("runtime", string(image.runtime)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("measuredAt", string(&capacity.measured_at)),
            ("newLayerInodes", number(capacity.new_layer_inodes)),
            (
                "persistentGrowthBytes",
                number(capacity.persistent_growth_bytes),
            ),
            (
                "persistentOwnedBytes",
                number(capacity.persistent_owned_bytes),
            ),
            ("protectedFreeBytes", number(PROTECTED_FREE_BYTES)),
            ("publicationSha256", string(&self.publication_sha256)),
            (
                "pullPeakGrowthBytes",
                number(capacity.pull_peak_growth_bytes),
            ),
            (
                "releaseTransactionPeakGrowthBytes",
                number(capacity.release_transaction_peak_growth_bytes),
            ),
            (
                "reasons",
                JsonValue::Array(
                    capacity
                        .reasons
                        .iter()
                        .map(|reason| string(reason))
                        .collect(),
                ),
            ),
            ("requiredFreeBytes", number(capacity.required_free_bytes)),
            ("requiredFreeInodes", number(capacity.required_free_inodes)),
            (
                "retainedControlBytes",
                number(capacity.retained_control_bytes),
            ),
            ("retirementCreditBytes", number(0)),
            ("sourceRelease", string(&self.core_release)),
            ("state", string(state)),
        ])
    }

    /// encode the recursively sorted dashboard receipt for the installed command.
    pub fn canonical_bytes(&self) -> Option<Vec<u8>> {
        NativeWirePrimitives.encode_sorted_json(&self.to_json())
    }
}

/// closed native family-capacity failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeFamilyReleaseCapacityError {
    Arguments,
    Measurement(String),
    Image,
    Inventory,
    Accounting,
    Stale,
}

impl Display for NativeFamilyReleaseCapacityError {
    /// retain bounded command diagnostics.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arguments => formatter.write_str("family release capacity arguments are invalid"),
            Self::Measurement(error) => write!(
                formatter,
                "family release capacity measurement failed: {error}"
            ),
            Self::Image => formatter.write_str("family release image measurement is invalid"),
            Self::Inventory => formatter.write_str("family release inventory is invalid"),
            Self::Accounting => formatter.write_str("family release capacity accounting overflow"),
            Self::Stale => formatter.write_str("family capacity proof is missing or stale"),
        }
    }
}

impl std::error::Error for NativeFamilyReleaseCapacityError {}

/// collect and evaluate the exact nine command operands without hidden host effects.
pub fn collect_native_family_release_capacity(
    port: &mut impl NativeFamilyReleaseCapacityMeasurementPort,
    operands: [&str; 9],
) -> Result<NativeFamilyReleaseCapacityReceipt, NativeFamilyReleaseCapacityError> {
    let [
        action,
        family,
        source_release,
        source_server,
        source_web,
        target_server,
        target_web,
        compensating_server,
        compensating_web,
    ] = operands;
    // retain the closed branch
    if !hash(action)
        || !matches!(family, "rain" | "temperature" | "wind")
        || !release(source_release)
    {
        return Err(NativeFamilyReleaseCapacityError::Arguments);
    }
    let references = [
        source_server,
        source_web,
        target_server,
        target_web,
        compensating_server,
        compensating_web,
    ];
    // bind alternating immutable server and web repositories
    for (index, reference) in references.iter().enumerate() {
        let expected = if index % 2 == 0 { "server" } else { "web" };
        // retain the closed branch
        if reference_digest(reference).is_none_or(|(runtime, _)| runtime != expected) {
            return Err(NativeFamilyReleaseCapacityError::Arguments);
        }
    }
    // retain the closed branch
    if source_server == compensating_server {
        return Err(NativeFamilyReleaseCapacityError::Arguments);
    }

    let measurement = collect_native_release_capacity_measurement(
        port,
        action,
        family,
        source_release,
        references,
    )?;
    evaluate_native_family_release_capacity(measurement)
}

/// collect the shared six-role physical evidence after policy-specific reference admission.
fn collect_native_release_capacity_measurement(
    port: &mut impl NativeFamilyReleaseCapacityMeasurementPort,
    action: &str,
    family: &str,
    source_release: &str,
    references: [&str; 6],
) -> Result<NativeFamilyReleaseCapacityMeasurement, NativeFamilyReleaseCapacityError> {
    let mut remote = BTreeMap::<String, ParsedRemoteImage>::new();
    // fetch every unique content-addressed image once
    for reference in references {
        // retain the closed branch
        if !remote.contains_key(reference) {
            let measured = port.remote_image(reference).map_err(|error| {
                NativeFamilyReleaseCapacityError::Measurement(error.to_string())
            })?;
            remote.insert(
                reference.to_owned(),
                parse_remote_image(reference, measured)?,
            );
        }
    }
    let mut inventory = BTreeMap::<String, u64>::new();
    // merge each local source and optional successor observation
    for (index, reference) in references.iter().enumerate() {
        let image = remote
            .get(*reference)
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let local = port
            .local_image(reference, &image.config_digest, &image.diff_ids)
            .map_err(|error| NativeFamilyReleaseCapacityError::Measurement(error.to_string()))?;
        // retain the closed branch
        if index < 2 && local.is_empty() {
            return Err(NativeFamilyReleaseCapacityError::Inventory);
        }
        // retain the closed branch
        if !local.is_empty() && local.len() != image.layers.len() {
            return Err(NativeFamilyReleaseCapacityError::Inventory);
        }
        // bind every returned chain to its expected parent sequence
        for (layer, expected) in local.into_iter().zip(&image.layers) {
            // retain the closed branch
            if layer.chain_id != expected.chain_id
                || allocated(layer.allocated_bytes) != Some(layer.allocated_bytes)
            {
                return Err(NativeFamilyReleaseCapacityError::Inventory);
            }
            // retain the closed branch
            if inventory
                .insert(layer.chain_id, layer.allocated_bytes)
                .is_some_and(|prior| prior != layer.allocated_bytes)
            {
                return Err(NativeFamilyReleaseCapacityError::Inventory);
            }
        }
    }
    // retain the closed branch
    if inventory.len() > MAXIMUM_INVENTORY {
        return Err(NativeFamilyReleaseCapacityError::Inventory);
    }
    let roles = [
        "source",
        "source",
        "target",
        "target",
        "compensating",
        "compensating",
    ];
    let runtimes = ["server", "web", "server", "web", "server", "web"];
    let images = references
        .iter()
        .enumerate()
        .map(|(index, reference)| {
            let image = remote.get(*reference).expect("admitted remote image");
            NativeFamilyReleaseCapacityImage {
                digest: image.digest.clone(),
                layers: image.layers.clone(),
                metadata_bytes: image.metadata_bytes,
                role: roles[index],
                runtime: runtimes[index],
            }
        })
        .collect();
    let filesystem = port
        .filesystem_capacity()
        .map_err(|error| NativeFamilyReleaseCapacityError::Measurement(error.to_string()))?;
    let retained_control_bytes = port
        .retained_control_bytes()
        .map_err(|error| NativeFamilyReleaseCapacityError::Measurement(error.to_string()))?;
    let measured_at = port
        .measured_at()
        .map_err(|error| NativeFamilyReleaseCapacityError::Measurement(error.to_string()))?;
    Ok(NativeFamilyReleaseCapacityMeasurement {
        action_sha256: action.to_owned(),
        family: family.to_owned(),
        free_bytes: filesystem.free_bytes,
        free_inodes: filesystem.free_inodes,
        images,
        inventory: inventory
            .into_iter()
            .map(
                |(chain_id, allocated_bytes)| NativeFamilyReleaseLocalLayer {
                    allocated_bytes,
                    chain_id,
                },
            )
            .collect(),
        measured_at,
        retained_control_bytes,
        runtime_package_bytes: 0,
        source_release: source_release.to_owned(),
    })
}

/// collect one dashboard web-only receipt through the shared physical measurement owner.
pub fn collect_native_dashboard_release_capacity(
    port: &mut impl NativeFamilyReleaseCapacityMeasurementPort,
    operands: [&str; 5],
) -> Result<NativeDashboardReleaseCapacityReceipt, NativeFamilyReleaseCapacityError> {
    let [
        publication_sha256,
        core_release,
        core_server,
        source_web,
        target_web,
    ] = operands;
    // reject caller-shaped scope before observing registry or host state
    if !hash(publication_sha256)
        || !release(core_release)
        || reference_digest(core_server).is_none_or(|(runtime, _)| runtime != "server")
        || reference_digest(source_web).is_none_or(|(runtime, _)| runtime != "web")
        || reference_digest(target_web).is_none_or(|(runtime, _)| runtime != "web")
        || source_web == target_web
    {
        return Err(NativeFamilyReleaseCapacityError::Arguments);
    }
    let references = [
        core_server,
        source_web,
        core_server,
        target_web,
        core_server,
        source_web,
    ];
    let measurement = collect_native_release_capacity_measurement(
        port,
        publication_sha256,
        "",
        core_release,
        references,
    )?;
    evaluate_native_dashboard_release_capacity(publication_sha256, core_release, measurement)
}

/// admit dashboard role policy around the exact shared physical capacity formula.
pub fn evaluate_native_dashboard_release_capacity(
    publication_sha256: &str,
    core_release: &str,
    input: NativeFamilyReleaseCapacityMeasurement,
) -> Result<NativeDashboardReleaseCapacityReceipt, NativeFamilyReleaseCapacityError> {
    let source_server = capacity_image_digest(&input, "source", "server");
    let source_web = capacity_image_digest(&input, "source", "web");
    let target_server = capacity_image_digest(&input, "target", "server");
    let target_web = capacity_image_digest(&input, "target", "web");
    let compensating_server = capacity_image_digest(&input, "compensating", "server");
    let compensating_web = capacity_image_digest(&input, "compensating", "web");
    let identity_valid = hash(publication_sha256)
        && release(core_release)
        && input.action_sha256 == publication_sha256
        && input.source_release == core_release
        && input.family.is_empty()
        && source_server.is_some()
        && source_server == target_server
        && source_server == compensating_server
        && source_web.is_some()
        && source_web == compensating_web
        && target_web.is_some()
        && source_web != target_web;
    let capacity = evaluate_native_release_capacity(input, identity_valid)?;
    Ok(NativeDashboardReleaseCapacityReceipt {
        core_release: core_release.to_owned(),
        publication_sha256: publication_sha256.to_owned(),
        capacity,
    })
}

/// select one unique role/runtime image digest from a normalized measurement.
fn capacity_image_digest<'a>(
    input: &'a NativeFamilyReleaseCapacityMeasurement,
    role: &str,
    runtime: &str,
) -> Option<&'a str> {
    let mut matching = input
        .images
        .iter()
        .filter(|image| image.role == role && image.runtime == runtime);
    let digest = matching.next()?.digest.as_str();
    // reject duplicate coordinates before formula evaluation
    matching.next().is_none().then_some(digest)
}

/// evaluate one normalized measurement through the predecessor's exact formula.
pub fn evaluate_native_family_release_capacity(
    input: NativeFamilyReleaseCapacityMeasurement,
) -> Result<NativeFamilyReleaseCapacityReceipt, NativeFamilyReleaseCapacityError> {
    let identity_valid = hash(&input.action_sha256)
        && matches!(input.family.as_str(), "rain" | "temperature" | "wind")
        && release(&input.source_release);
    evaluate_native_release_capacity(input, identity_valid)
}

/// evaluate the shared six-role physical calculation after policy-specific identity admission.
fn evaluate_native_release_capacity(
    input: NativeFamilyReleaseCapacityMeasurement,
    identity_valid: bool,
) -> Result<NativeFamilyReleaseCapacityReceipt, NativeFamilyReleaseCapacityError> {
    // retain the closed branch
    if !identity_valid
        || input.images.len() != 6
        || NativeWirePrimitives
            .canonical_utc_milliseconds(&input.measured_at)
            .is_none()
        || allocated(input.free_bytes).is_none()
        || input.free_inodes > MAXIMUM_SAFE_INTEGER
        || allocated(input.retained_control_bytes).is_none()
        || allocated(input.runtime_package_bytes).is_none()
    {
        return Err(NativeFamilyReleaseCapacityError::Arguments);
    }
    let available = input.free_bytes / BLOCK_BYTES * BLOCK_BYTES;
    let mut inventory = BTreeMap::new();
    // retain each unique measured local chain
    for layer in &input.inventory {
        let amount =
            allocated(layer.allocated_bytes).ok_or(NativeFamilyReleaseCapacityError::Inventory)?;
        // retain the closed branch
        if !digest(&layer.chain_id) || inventory.insert(layer.chain_id.clone(), amount).is_some() {
            return Err(NativeFamilyReleaseCapacityError::Inventory);
        }
    }
    let mut owned = BTreeMap::<String, u64>::new();
    let mut layer_inodes = BTreeMap::<String, u64>::new();
    let mut new_compressed = BTreeMap::<String, u64>::new();
    let mut metadata = BTreeMap::<String, u64>::new();
    let mut manifests = BTreeMap::<String, Vec<NativeFamilyReleaseCapacityLayer>>::new();
    let mut roles = BTreeSet::new();
    // require all six literal images to coexist
    for image in &input.images {
        // retain the closed branch
        if !digest(&image.digest)
            || !matches!(image.role, "source" | "target" | "compensating")
            || !matches!(image.runtime, "server" | "web")
            || !roles.insert((image.role, image.runtime))
            || image.layers.is_empty()
        {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        let metadata_bytes =
            allocated(image.metadata_bytes).ok_or(NativeFamilyReleaseCapacityError::Accounting)?;
        // retain the closed branch
        if metadata
            .insert(image.digest.clone(), metadata_bytes)
            .is_some_and(|prior| prior != metadata_bytes)
        {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        // retain the closed branch
        if manifests
            .insert(image.digest.clone(), image.layers.clone())
            .is_some_and(|prior| prior != image.layers)
        {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        let mut diff_ids = Vec::new();
        // deduplicate only complete Docker parent chains
        for layer in &image.layers {
            diff_ids.push(layer.diff_id.clone());
            // retain the closed branch
            if !digest(&layer.blob_digest)
                || !digest(&layer.diff_id)
                || docker_chain_identity(&diff_ids).as_deref() != Some(&layer.chain_id)
                || !(1..=MAXIMUM_LAYER_INODES).contains(&layer.entry_inodes)
            {
                return Err(NativeFamilyReleaseCapacityError::Image);
            }
            let unpacked = allocated(layer.unpacked_bytes)
                .filter(|bytes| *bytes <= MAXIMUM_UNPACKED_LAYER_BYTES)
                .ok_or(NativeFamilyReleaseCapacityError::Accounting)?;
            let compressed = allocated(layer.compressed_bytes)
                .ok_or(NativeFamilyReleaseCapacityError::Accounting)?;
            // retain the closed branch
            if owned
                .insert(layer.chain_id.clone(), unpacked)
                .is_some_and(|prior| prior != unpacked)
                || layer_inodes
                    .insert(layer.chain_id.clone(), layer.entry_inodes)
                    .is_some_and(|prior| prior != layer.entry_inodes)
            {
                return Err(NativeFamilyReleaseCapacityError::Image);
            }
            // charge each absent compressed blob once
            if !inventory.contains_key(&layer.chain_id)
                && new_compressed
                    .insert(layer.blob_digest.clone(), compressed)
                    .is_some_and(|prior| prior != compressed)
            {
                return Err(NativeFamilyReleaseCapacityError::Image);
            }
        }
    }
    // retain the closed branch
    if roles.len() != 6 {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let mut retained_layer_bytes = 0;
    let mut new_layer_bytes = 0;
    let mut new_layer_inodes = 0;
    // account actual retained blocks and conservative absent-chain bounds
    for (chain, bound) in &owned {
        retained_layer_bytes = sum([retained_layer_bytes, *inventory.get(chain).unwrap_or(bound)])?;
        // retain the closed branch
        if !inventory.contains_key(chain) {
            new_layer_bytes = sum([new_layer_bytes, *bound])?;
            new_layer_inodes = sum([
                new_layer_inodes,
                *layer_inodes
                    .get(chain)
                    .ok_or(NativeFamilyReleaseCapacityError::Image)?,
            ])?;
        }
    }
    let metadata_bytes = sum(metadata.into_values())?;
    let package_bytes = allocated(input.runtime_package_bytes)
        .ok_or(NativeFamilyReleaseCapacityError::Accounting)?;
    let retained_control_bytes = allocated(input.retained_control_bytes)
        .ok_or(NativeFamilyReleaseCapacityError::Accounting)?;
    let persistent_owned_bytes = sum([
        retained_layer_bytes,
        metadata_bytes,
        ENGINE_METADATA_BYTES,
        package_bytes,
        retained_control_bytes,
        FUTURE_STATE_BYTES,
    ])?;
    let persistent_growth_bytes = sum([
        new_layer_bytes,
        metadata_bytes,
        ENGINE_METADATA_BYTES,
        package_bytes,
        FUTURE_STATE_BYTES,
    ])?;
    let compressed_peak_bytes = sum(new_compressed.into_values())?;
    let pull_peak_growth_bytes = sum([
        persistent_growth_bytes,
        compressed_peak_bytes,
        PULL_SCRATCH_BYTES,
    ])?;
    let compatibility_peak = sum([persistent_growth_bytes, COMPATIBILITY_FIXTURE_BYTES])?;
    let release_transaction_peak_growth_bytes = pull_peak_growth_bytes.max(compatibility_peak);
    let required_free_bytes = sum([
        PROTECTED_FREE_BYTES,
        NEXT_CAPTURE_BYTES,
        release_transaction_peak_growth_bytes,
    ])?;
    let required_free_inodes = sum([
        INODE_FLOOR,
        NEXT_STATE_INODES,
        new_layer_inodes,
        ENGINE_METADATA_INODES,
        CONTROL_GROWTH_INODES,
        COMPATIBILITY_FIXTURE_INODES,
    ])?;
    let mut reasons = Vec::new();
    // preserve the source refusal ordering
    if available < required_free_bytes {
        reasons.push("protected_floor_or_transaction_peak");
    }
    // retain the closed branch
    if input.free_inodes < required_free_inodes {
        reasons.push("inode_floor");
    }
    // retain the closed branch
    if persistent_owned_bytes > MAXIMUM_OWNED_BYTES {
        reasons.push("persistent_image_ownership");
    }
    // retain the closed branch
    if input.runtime_package_bytes > RUNTIME_PACKAGE_MAXIMUM_BYTES {
        reasons.push("runtime_package_ceiling");
    }
    Ok(NativeFamilyReleaseCapacityReceipt {
        action_sha256: input.action_sha256,
        available_free_bytes: available,
        compressed_peak_bytes,
        family: input.family,
        future_state_bytes: FUTURE_STATE_BYTES,
        image_digests: input
            .images
            .into_iter()
            .map(|image| NativeFamilyReleaseCapacityDigest {
                digest: image.digest,
                role: image.role,
                runtime: image.runtime,
            })
            .collect(),
        measured_at: input.measured_at,
        new_layer_inodes,
        persistent_growth_bytes,
        persistent_owned_bytes,
        pull_peak_growth_bytes,
        reasons,
        release_transaction_peak_growth_bytes,
        required_free_bytes,
        required_free_inodes,
        retained_control_bytes,
        source_release: input.source_release,
    })
}

/// admit one ready receipt through the source's fifteen-minute freshness boundary.
pub fn admit_ready_native_family_release_capacity(
    receipt: &NativeFamilyReleaseCapacityReceipt,
    action_sha256: &str,
    family: &str,
    source_release: &str,
    now: &str,
) -> Result<(), NativeFamilyReleaseCapacityError> {
    let measured = NativeWirePrimitives
        .canonical_utc_milliseconds(&receipt.measured_at)
        .ok_or(NativeFamilyReleaseCapacityError::Stale)?;
    let now = NativeWirePrimitives
        .canonical_utc_milliseconds(now)
        .ok_or(NativeFamilyReleaseCapacityError::Stale)?;
    let age = now
        .checked_sub(measured)
        .ok_or(NativeFamilyReleaseCapacityError::Stale)?;
    let roles = receipt
        .image_digests
        .iter()
        .map(|image| (image.role, image.runtime))
        .collect::<BTreeSet<_>>();
    // retain the closed branch
    if !receipt.reasons.is_empty()
        || receipt.action_sha256 != action_sha256
        || receipt.family != family
        || receipt.source_release != source_release
        || !(0..=900_000).contains(&age)
        || receipt.image_digests.len() != 6
        || roles.len() != 6
        || receipt.image_digests.iter().any(|image| {
            !digest(&image.digest)
                || !matches!(image.role, "source" | "target" | "compensating")
                || !matches!(image.runtime, "server" | "web")
        })
    {
        return Err(NativeFamilyReleaseCapacityError::Stale);
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct ParsedRemoteImage {
    config_digest: String,
    digest: String,
    diff_ids: Vec<String>,
    layers: Vec<NativeFamilyReleaseCapacityLayer>,
    metadata_bytes: u64,
}

/// validate raw OCI/config bytes and bind each unpacked measurement.
fn parse_remote_image(
    reference: &str,
    measured: NativeFamilyReleaseRemoteImage,
) -> Result<ParsedRemoteImage, NativeFamilyReleaseCapacityError> {
    let (_, reference_hash) =
        reference_digest(reference).ok_or(NativeFamilyReleaseCapacityError::Image)?;
    // retain the closed branch
    if measured.manifest_bytes.len() > 1_048_576
        || measured.config_bytes.len() > 4_194_304
        || sha256(&measured.manifest_bytes) != reference_hash
    {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let manifest = NativeWirePrimitives
        .decode_json(&measured.manifest_bytes)
        .ok_or(NativeFamilyReleaseCapacityError::Image)?;
    let config = NativeWirePrimitives
        .decode_json(&measured.config_bytes)
        .ok_or(NativeFamilyReleaseCapacityError::Image)?;
    // retain the closed branch
    if !exact_keys(
        &manifest,
        &["config", "layers", "mediaType", "schemaVersion"],
    ) || integer(&manifest, "schemaVersion") != Some(2)
        || text(&manifest, "mediaType").as_deref()
            != Some("application/vnd.oci.image.manifest.v1+json")
        || text(&config, "architecture").as_deref() != Some("arm64")
        || text(&config, "os").as_deref() != Some("linux")
    {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let config_descriptor =
        field(&manifest, "config").ok_or(NativeFamilyReleaseCapacityError::Image)?;
    let config_digest =
        text(config_descriptor, "digest").ok_or(NativeFamilyReleaseCapacityError::Image)?;
    // retain the closed branch
    if !digest(&config_digest)
        || text(config_descriptor, "mediaType").as_deref()
            != Some("application/vnd.oci.image.config.v1+json")
        || integer(config_descriptor, "size") != u64::try_from(measured.config_bytes.len()).ok()
        || sha256_digest(&measured.config_bytes) != config_digest
    {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let layers = array(&manifest, "layers").ok_or(NativeFamilyReleaseCapacityError::Image)?;
    let rootfs = field(&config, "rootfs").ok_or(NativeFamilyReleaseCapacityError::Image)?;
    let diff_ids =
        string_array(rootfs, "diff_ids").ok_or(NativeFamilyReleaseCapacityError::Image)?;
    // retain the closed branch
    if text(rootfs, "type").as_deref() != Some("layers")
        || layers.is_empty()
        || layers.len() > MAXIMUM_LAYERS
        || layers.len() != diff_ids.len()
        || layers.len() != measured.unpacked_layers.len()
    {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let mut normalized = Vec::with_capacity(layers.len());
    let mut parents = Vec::new();
    let mut compressed_total = 0;
    // bind every manifest descriptor to its measured unpacked layer
    for ((descriptor, diff_id), unpacked) in
        layers.iter().zip(&diff_ids).zip(&measured.unpacked_layers)
    {
        let blob = text(descriptor, "digest").ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let compressed =
            integer(descriptor, "size").ok_or(NativeFamilyReleaseCapacityError::Image)?;
        // retain the closed branch
        if !exact_keys(descriptor, &["digest", "mediaType", "size"])
            || text(descriptor, "mediaType").as_deref()
                != Some("application/vnd.oci.image.layer.v1.tar+gzip")
            || !digest(&blob)
            || !digest(diff_id)
            || blob != unpacked.blob_digest
            || allocated(unpacked.allocated_bytes) != Some(unpacked.allocated_bytes)
            || unpacked.allocated_bytes > MAXIMUM_UNPACKED_LAYER_BYTES
            || !(1..=MAXIMUM_LAYER_INODES).contains(&unpacked.entry_inodes)
        {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        compressed_total = sum([compressed_total, compressed])?;
        parents.push(diff_id.clone());
        normalized.push(NativeFamilyReleaseCapacityLayer {
            blob_digest: blob,
            chain_id: docker_chain_identity(&parents)
                .ok_or(NativeFamilyReleaseCapacityError::Image)?,
            compressed_bytes: compressed,
            diff_id: diff_id.clone(),
            entry_inodes: unpacked.entry_inodes,
            unpacked_bytes: unpacked.allocated_bytes,
        });
    }
    // retain the closed branch
    if compressed_total > MAXIMUM_COMPRESSED_IMAGE_BYTES {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    Ok(ParsedRemoteImage {
        config_digest,
        digest: format!("sha256:{reference_hash}"),
        diff_ids,
        layers: normalized,
        metadata_bytes: sum([
            allocated(measured.manifest_bytes.len() as u64)
                .ok_or(NativeFamilyReleaseCapacityError::Accounting)?,
            allocated(measured.config_bytes.len() as u64)
                .ok_or(NativeFamilyReleaseCapacityError::Accounting)?,
        ])?,
    })
}

/// freeze Docker's recursive parent-plus-diff identity.
pub fn native_family_release_docker_chain_identity(diff_ids: &[String]) -> Option<String> {
    docker_chain_identity(diff_ids)
}

/// derive one recursive Docker parent-chain identity.
fn docker_chain_identity(diff_ids: &[String]) -> Option<String> {
    let mut values = diff_ids.iter();
    let mut identity = values.next()?.clone();
    // retain the closed branch
    if !digest(&identity) {
        return None;
    }
    // retain the recursive Docker chain preimage
    for diff_id in values {
        // retain the closed branch
        if !digest(diff_id) {
            return None;
        }
        identity = sha256_digest(format!("{identity} {diff_id}").as_bytes());
    }
    Some(identity)
}

/// round one byte count to the exact host block size.
fn allocated(bytes: u64) -> Option<u64> {
    (bytes <= MAXIMUM_SAFE_INTEGER - BLOCK_BYTES).then(|| bytes.div_ceil(BLOCK_BYTES) * BLOCK_BYTES)
}

/// sum exact capacity buckets without precision loss.
fn sum(values: impl IntoIterator<Item = u64>) -> Result<u64, NativeFamilyReleaseCapacityError> {
    let mut total = 0_u64;
    // reject every intermediate precision overflow
    for value in values {
        total = total
            .checked_add(value)
            .filter(|total| *total <= MAXIMUM_SAFE_INTEGER)
            .ok_or(NativeFamilyReleaseCapacityError::Accounting)?;
    }
    Ok(total)
}

/// parse one fixed immutable application image reference.
fn reference_digest(reference: &str) -> Option<(&str, &str)> {
    let suffix = reference.strip_prefix("ghcr.io/anstosa/weather-")?;
    let (runtime, digest) = suffix.split_once("@sha256:")?;
    matches!(runtime, "server" | "web")
        .then_some(())
        .filter(|_| hash(digest))?;
    Some((runtime, digest))
}

/// recognize the source release label grammar.
fn release(value: &str) -> bool {
    let Some((date, revision)) = value.rsplit_once('-') else {
        return false;
    };
    date.len() == 10
        && date.as_bytes().get(4) == Some(&b'.')
        && date.as_bytes().get(7) == Some(&b'.')
        && date
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
        && revision
            .as_bytes()
            .first()
            .is_some_and(|byte| matches!(byte, b'1'..=b'9'))
        && revision.bytes().all(|byte| byte.is_ascii_digit())
}

/// recognize one lowercase SHA-256 value.
fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// recognize one OCI digest spelling.
fn digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(hash)
}

/// hash exact bytes into lowercase hexadecimal.
fn sha256(bytes: &[u8]) -> String {
    RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// hash exact bytes into one OCI digest.
fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256(bytes))
}

/// borrow one required JSON array field.
fn array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(value, key)? else {
        return None;
    };
    Some(values)
}

/// decode one exact JSON string array.
fn string_array(value: &JsonValue, key: &str) -> Option<Vec<String>> {
    let values = array(value, key)?;
    values
        .iter()
        .map(|value| match value {
            JsonValue::String(units) => String::from_utf16(units).ok(),
            _ => None,
        })
        .collect()
}

/// decode one nonnegative JavaScript-safe integer.
fn integer(value: &JsonValue, key: &str) -> Option<u64> {
    match field(value, key)? {
        JsonValue::Number(number)
            // retain the closed branch
            if number.is_finite()
                && *number >= 0.0
                && number.fract() == 0.0
                && *number <= MAXIMUM_SAFE_INTEGER as f64 =>
        {
            Some(*number as u64)
        }
        _ => None,
    }
}

/// build one generated JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// build one generated JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// build one exact JSON integer.
fn number(value: u64) -> JsonValue {
    JsonValue::Number(value as f64)
}

/// installed registry, Docker, filesystem, and clock measurement owner.
pub struct NativeInstalledFamilyReleaseCapacity {
    http: NativeOutboundHttp,
    tokens: BTreeMap<String, String>,
    layers: BTreeMap<(String, String), NativeFamilyReleaseRemoteLayer>,
}

impl NativeInstalledFamilyReleaseCapacity {
    /// construct the installed owner without observing remote or host state.
    pub fn new() -> Result<Self, NativeFamilyReleaseCapacityError> {
        Ok(Self {
            http: NativeOutboundHttp::new().map_err(measurement_error)?,
            tokens: BTreeMap::new(),
            layers: BTreeMap::new(),
        })
    }

    /// read one authenticated registry object or its validated signed blob location.
    fn registry_object(
        &mut self,
        repository: &str,
        kind: &str,
        digest: &str,
        accept: &'static str,
        maximum_bytes: u64,
    ) -> Result<RegistryObject, NativeFamilyReleaseCapacityError> {
        let token = self.registry_token(repository, kind, digest, accept)?;
        let url = registry_url(repository, kind, digest)?;
        let authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| NativeFamilyReleaseCapacityError::Image)?;
        let request = NativeOutboundHttpRequest::new(Method::GET, url)
            .map_err(measurement_error)?
            .header(ACCEPT, HeaderValue::from_static(accept))
            .header(AUTHORIZATION, authorization)
            .manual_redirect()
            .stream_body_limit_only();
        let response =
            execute_registry_request(&self.http, request, REGISTRY_TIMEOUT, maximum_bytes)?;
        // accept complete objects directly
        if response.status == 200 {
            // bind direct bytes to the requested digest
            if sha256_digest(&response.body) != digest {
                return Err(NativeFamilyReleaseCapacityError::Image);
            }
            return Ok(RegistryObject::Bytes(response.body));
        }
        // admit only the fixed signed blob handoff
        if response.status == 307 && kind == "blobs" {
            let location = response
                .headers
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(NativeFamilyReleaseCapacityError::Image)?;
            return validate_registry_redirect(location, digest).map(RegistryObject::Redirect);
        }
        Err(NativeFamilyReleaseCapacityError::Image)
    }

    /// obtain and cache one anonymous fixed-repository pull token.
    fn registry_token(
        &mut self,
        repository: &str,
        kind: &str,
        digest: &str,
        accept: &'static str,
    ) -> Result<String, NativeFamilyReleaseCapacityError> {
        // reuse only the exact repository-scoped token
        if let Some(token) = self.tokens.get(repository) {
            return Ok(token.clone());
        }
        let url = registry_url(repository, kind, digest)?;
        let request = NativeOutboundHttpRequest::new(Method::GET, url)
            .map_err(measurement_error)?
            .header(ACCEPT, HeaderValue::from_static(accept))
            .manual_redirect()
            .stream_body_limit_only();
        let response = execute_registry_request(&self.http, request, TOKEN_TIMEOUT, 16_384)?;
        // require the anonymous challenge before token issuance
        if response.status != 401 {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        let challenge = response
            .headers
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let scope = format!("repository:anstosa/{repository}:pull");
        let expected =
            format!("Bearer realm=\"https://ghcr.io/token\",service=\"ghcr.io\",scope=\"{scope}\"");
        // bind the challenge to the fixed repository scope
        if challenge != expected {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        let mut token_url = Url::parse("https://ghcr.io/token")
            .map_err(|_| NativeFamilyReleaseCapacityError::Image)?;
        token_url
            .query_pairs_mut()
            .append_pair("service", "ghcr.io")
            .append_pair("scope", &scope);
        let request = NativeOutboundHttpRequest::new(Method::GET, token_url)
            .map_err(measurement_error)?
            .manual_redirect()
            .stream_body_limit_only();
        let response = execute_registry_request(&self.http, request, TOKEN_TIMEOUT, 16_384)?;
        // require one successful token document
        if response.status != 200 {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        let value = NativeWirePrimitives
            .decode_json(&response.body)
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let token = text(&value, "token")
            .filter(|token| (1..=8_192).contains(&token.len()))
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        self.tokens.insert(repository.to_owned(), token.clone());
        Ok(token)
    }
}

impl NativeFamilyReleaseCapacityMeasurementPort for NativeInstalledFamilyReleaseCapacity {
    type Error = NativeFamilyReleaseCapacityError;

    /// fetch one immutable manifest, config, and each verified streamed layer.
    fn remote_image(
        &mut self,
        reference: &str,
    ) -> Result<NativeFamilyReleaseRemoteImage, Self::Error> {
        let (runtime, hash) =
            reference_digest(reference).ok_or(NativeFamilyReleaseCapacityError::Arguments)?;
        let repository = format!("weather-{runtime}");
        let manifest_digest = format!("sha256:{hash}");
        let manifest_bytes = match self.registry_object(
            &repository,
            "manifests",
            &manifest_digest,
            "application/vnd.oci.image.manifest.v1+json",
            1_048_576,
        )? {
            RegistryObject::Bytes(bytes) => bytes,
            RegistryObject::Redirect(_) => return Err(NativeFamilyReleaseCapacityError::Image),
        };
        let manifest = NativeWirePrimitives
            .decode_json(&manifest_bytes)
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let config_descriptor =
            field(&manifest, "config").ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let config_digest = text(config_descriptor, "digest")
            .filter(|digest| super_digest(digest))
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let config_bytes = match self.registry_object(
            &repository,
            "blobs",
            &config_digest,
            "application/vnd.oci.image.config.v1+json",
            4_194_304,
        )? {
            RegistryObject::Bytes(bytes) => bytes,
            RegistryObject::Redirect(url) => {
                read_signed_registry_object(&self.http, &url, 4_194_304, &config_digest)?
            }
        };
        let config = NativeWirePrimitives
            .decode_json(&config_bytes)
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let descriptors =
            array(&manifest, "layers").ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let rootfs = field(&config, "rootfs").ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let diff_ids =
            string_array(rootfs, "diff_ids").ok_or(NativeFamilyReleaseCapacityError::Image)?;
        // bind every descriptor to one config diff id
        if descriptors.is_empty()
            || descriptors.len() != diff_ids.len()
            || descriptors.len() > MAXIMUM_LAYERS
        {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        let mut compressed_total = 0_u64;
        // reject the descriptor set before fetching layer bodies
        for descriptor in descriptors {
            let _blob = text(descriptor, "digest")
                .filter(|digest| super_digest(digest))
                .ok_or(NativeFamilyReleaseCapacityError::Image)?;
            let compressed = integer(descriptor, "size")
                .filter(|size| *size > 0)
                .ok_or(NativeFamilyReleaseCapacityError::Image)?;
            compressed_total = sum([compressed_total, compressed])?;
        }
        // preserve the complete image bound
        if compressed_total > MAXIMUM_COMPRESSED_IMAGE_BYTES {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        let mut unpacked_layers = Vec::with_capacity(descriptors.len());
        // stream and cache each complete content-addressed measurement
        for (descriptor, diff_id) in descriptors.iter().zip(diff_ids) {
            let blob = text(descriptor, "digest")
                .filter(|digest| super_digest(digest))
                .ok_or(NativeFamilyReleaseCapacityError::Image)?;
            let compressed = integer(descriptor, "size")
                .filter(|size| *size > 0)
                .ok_or(NativeFamilyReleaseCapacityError::Image)?;
            let key = (blob.clone(), diff_id.clone());
            // reuse only a complete content-addressed measurement
            if let Some(measurement) = self.layers.get(&key) {
                unpacked_layers.push(measurement.clone());
                continue;
            }
            let token = self.registry_token(
                &repository,
                "blobs",
                &blob,
                "application/vnd.oci.image.layer.v1.tar+gzip",
            )?;
            let measurement =
                measure_registry_layer(&repository, &blob, compressed, &diff_id, &token)?;
            self.layers.insert(key, measurement.clone());
            unpacked_layers.push(measurement);
        }
        Ok(NativeFamilyReleaseRemoteImage {
            config_bytes,
            manifest_bytes,
            unpacked_layers,
        })
    }

    /// inspect one local Docker image and measure each literal overlay chain.
    fn local_image(
        &mut self,
        reference: &str,
        config_digest: &str,
        diff_ids: &[String],
    ) -> Result<Vec<NativeFamilyReleaseLocalLayer>, Self::Error> {
        inspect_installed_local_image(reference, config_digest, diff_ids)
    }

    /// sample the exact available weather filesystem blocks and free inodes.
    fn filesystem_capacity(
        &mut self,
    ) -> Result<NativeFamilyReleaseFilesystemCapacity, Self::Error> {
        let sample = NativeForecastAllocationStatfs::new()
            .approved_sample(Path::new(CAPACITY_ROOT))
            .map_err(measurement_error)?;
        let free_bytes = sample
            .available_blocks
            .checked_mul(sample.block_size)
            .ok_or(NativeFamilyReleaseCapacityError::Accounting)?;
        Ok(NativeFamilyReleaseFilesystemCapacity {
            free_bytes,
            free_inodes: sample.free_inodes,
        })
    }

    /// measure the fixed installed release tree without following links.
    fn retained_control_bytes(&mut self) -> Result<u64, Self::Error> {
        measure_installed_path(Path::new(CONTROL_ROOT)).map(|measurement| measurement.bytes)
    }

    /// sample and format one canonical millisecond observation.
    fn measured_at(&mut self) -> Result<String, Self::Error> {
        system_worker_health_milliseconds()
            .and_then(|value| NativeWirePrimitives.format_utc_milliseconds(value))
            .ok_or(NativeFamilyReleaseCapacityError::Measurement(
                "installed clock unavailable".to_owned(),
            ))
    }
}

/// collect one production receipt through the complete installed measurement owner.
pub fn collect_native_family_release_capacity_from_installed(
    operands: [&str; 9],
) -> Result<NativeFamilyReleaseCapacityReceipt, NativeFamilyReleaseCapacityError> {
    let mut installed = NativeInstalledFamilyReleaseCapacity::new()?;
    collect_native_family_release_capacity(&mut installed, operands)
}

/// collect one production dashboard receipt through the installed measurement owner.
pub fn collect_native_dashboard_release_capacity_from_installed(
    operands: [&str; 5],
) -> Result<NativeDashboardReleaseCapacityReceipt, NativeFamilyReleaseCapacityError> {
    let mut installed = NativeInstalledFamilyReleaseCapacity::new()?;
    collect_native_dashboard_release_capacity(&mut installed, operands)
}

enum RegistryObject {
    Bytes(Vec<u8>),
    Redirect(Url),
}

/// build one fixed GHCR v2 object target.
fn registry_url(
    repository: &str,
    kind: &str,
    digest: &str,
) -> Result<Url, NativeFamilyReleaseCapacityError> {
    // keep every registry coordinate fixed and immutable
    if !matches!(repository, "weather-server" | "weather-web")
        || !matches!(kind, "manifests" | "blobs")
        || !super_digest(digest)
    {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    Url::parse(&format!(
        "https://ghcr.io/v2/anstosa/{repository}/{kind}/{digest}"
    ))
    .map_err(|_| NativeFamilyReleaseCapacityError::Image)
}

/// distinguish the helper from the local variable names used by registry readers.
fn super_digest(value: &str) -> bool {
    digest(value)
}

/// validate one source-exact GitHub signed blob handoff.
fn validate_registry_redirect(
    location: &str,
    digest: &str,
) -> Result<Url, NativeFamilyReleaseCapacityError> {
    // preserve the source redirect bound
    if location.len() > 4_096 {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let url = Url::parse(location).map_err(|_| NativeFamilyReleaseCapacityError::Image)?;
    let host = url.host_str().unwrap_or_default();
    let prefix = url
        .path()
        .strip_prefix("/ghcr")
        .and_then(|path| path.split_once("/blobs/"));
    let host_index = prefix.map(|value| value.0).unwrap_or_default();
    let host_ok = host_index.strip_prefix("blobs").is_some_and(|digits| {
        digits.len() == 2 && digits.bytes().all(|byte| byte.is_ascii_digit())
    }) || (!host_index.is_empty()
        && host_index.len() <= 3
        && host_index.bytes().all(|byte| byte.is_ascii_digit()));
    let mut pairs = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    let keys = pairs.iter().map(|pair| pair.0.as_str()).collect::<Vec<_>>();
    let expected = [
        "hmac", "se", "sig", "ske", "skoid", "sks", "skt", "sktid", "skv", "sp", "spr", "sr", "sv",
    ];
    // admit only the fixed signed blob handoff
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || host != "pkg-containers.githubusercontent.com"
        || !host_ok
        || prefix.map(|value| value.1) != Some(digest)
        || url.fragment().is_some()
        || keys != expected
        || query(&pairs, "sp") != Some("r")
        || query(&pairs, "spr") != Some("https")
        || query(&pairs, "sr") != Some("b")
        || query(&pairs, "sks") != Some("b")
        || !query(&pairs, "skoid").is_some_and(uuid)
        || !query(&pairs, "sktid").is_some_and(uuid)
        || !query(&pairs, "hmac").is_some_and(hash)
        || !query(&pairs, "sig").is_some_and(signature)
        || !["se", "ske", "skt"].iter().all(|key| {
            query(&pairs, key).is_some_and(|value| {
                NativeWirePrimitives
                    .parse_explicit_utc_milliseconds(value)
                    .is_some()
            })
        })
        || !["skv", "sv"]
            .iter()
            .all(|key| query(&pairs, key).is_some_and(date_label))
    {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    Ok(url)
}

/// borrow one unique decoded query value.
fn query<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    let mut matches = pairs.iter().filter(|pair| pair.0 == key);
    let value = matches.next()?.1.as_str();
    matches.next().is_none().then_some(value)
}

/// recognize one lowercase UUID identity.
fn uuid(value: &str) -> bool {
    let groups = [8, 4, 4, 4, 12];
    let parts = value.split('-').collect::<Vec<_>>();
    parts.len() == groups.len()
        && parts.iter().zip(groups).all(|(part, length)| {
            part.len() == length
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
}

/// recognize the signed query's bounded base64 alphabet.
fn signature(value: &str) -> bool {
    (16..=512).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

/// recognize one fixed-width signed-query date.
fn date_label(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

/// read one small signed registry object without forwarding bearer authority.
fn read_signed_registry_object(
    http: &NativeOutboundHttp,
    url: &Url,
    maximum: u64,
    expected_digest: &str,
) -> Result<Vec<u8>, NativeFamilyReleaseCapacityError> {
    let request = NativeOutboundHttpRequest::new(Method::GET, url.clone())
        .map_err(measurement_error)?
        .manual_redirect()
        .stream_body_limit_only();
    let response = execute_registry_request(http, request, REGISTRY_TIMEOUT, maximum)?;
    // bind the complete small response to its descriptor
    if response.status != 200 || sha256_digest(&response.body) != expected_digest {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    Ok(response.body)
}

/// retry one complete bounded HTTP exchange only after its transport deadline.
fn execute_registry_request(
    http: &NativeOutboundHttp,
    request: NativeOutboundHttpRequest,
    timeout: Duration,
    maximum: u64,
) -> Result<NativeOutboundHttpResponse, NativeFamilyReleaseCapacityError> {
    // permit one fresh attempt after a deadline only
    for attempt in 0..2 {
        match http.execute(request.clone(), timeout, maximum) {
            Ok(response) => return Ok(response),
            Err(NativeOutboundHttpError::Deadline) if attempt == 0 => {}
            Err(error) => return Err(measurement_error(error)),
        }
    }
    Err(measurement_message("registry retry did not terminate"))
}

/// stream one authenticated registry layer through either admitted response path.
fn measure_registry_layer(
    repository: &str,
    blob_digest: &str,
    compressed_size: u64,
    diff_id: &str,
    token: &str,
) -> Result<NativeFamilyReleaseRemoteLayer, NativeFamilyReleaseCapacityError> {
    // permit one fresh complete attempt after curl's fixed deadline
    for attempt in 0..2 {
        match measure_registry_layer_once(repository, blob_digest, compressed_size, diff_id, token)
        {
            Ok(measurement) => return Ok(measurement),
            Err(SignedLayerAttemptError::Timeout) if attempt == 0 => {}
            Err(SignedLayerAttemptError::Timeout) => {
                return Err(measurement_message("registry layer timed out"));
            }
            Err(SignedLayerAttemptError::Failure(error)) => return Err(error),
        }
    }
    Err(measurement_message(
        "registry layer retry did not terminate",
    ))
}

enum SignedLayerAttemptError {
    Timeout,
    Failure(NativeFamilyReleaseCapacityError),
}

struct CurlResponseHeaders {
    location: Option<String>,
    status: u16,
}

/// execute one complete authenticated layer attempt without partial cache state.
fn measure_registry_layer_once(
    repository: &str,
    blob_digest: &str,
    compressed_size: u64,
    diff_id: &str,
    token: &str,
) -> Result<NativeFamilyReleaseRemoteLayer, SignedLayerAttemptError> {
    let failure = SignedLayerAttemptError::Failure;
    let url = registry_url(repository, "blobs", blob_digest).map_err(failure)?;
    let mut child = spawn_authenticated_curl(&url, token).map_err(failure)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| failure(measurement_message("curl stdout unavailable")))?;
    let mut reader = BufReader::new(stdout);
    let headers = match read_curl_response_headers(&mut reader) {
        Ok(headers) => headers,
        Err(error) => {
            finish_curl(&mut child, true)?;
            return Err(failure(error));
        }
    };
    // stream direct success bytes without a second copy or buffer
    if headers.status == 200 {
        return measure_curl_layer_stream(
            &mut child,
            reader,
            blob_digest,
            compressed_size,
            diff_id,
        );
    }
    // admit one body-free signed handoff only
    if headers.status != 307 {
        finish_curl(&mut child, true)?;
        return Err(failure(NativeFamilyReleaseCapacityError::Image));
    }
    let Some(location) = headers.location else {
        finish_curl(&mut child, true)?;
        return Err(failure(NativeFamilyReleaseCapacityError::Image));
    };
    let signed = match validate_registry_redirect(&location, blob_digest) {
        Ok(signed) => signed,
        Err(error) => {
            finish_curl(&mut child, true)?;
            return Err(failure(error));
        }
    };
    drop(reader);
    let _ = finish_curl(&mut child, true)?;
    let mut child = spawn_signed_curl(&signed).map_err(failure)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| failure(measurement_message("curl stdout unavailable")))?;
    measure_curl_layer_stream(&mut child, stdout, blob_digest, compressed_size, diff_id)
}

/// consume one curl body through exact gzip, tar, and digest validation.
fn measure_curl_layer_stream(
    child: &mut Child,
    input: impl Read,
    blob_digest: &str,
    compressed_size: u64,
    diff_id: &str,
) -> Result<NativeFamilyReleaseRemoteLayer, SignedLayerAttemptError> {
    let failure = SignedLayerAttemptError::Failure;
    let compressed = DigestReader::new(input, compressed_size);
    let mut decoder = GzDecoder::new(compressed);
    let measurement = measure_tar_stream(&mut decoder);
    let compressed = decoder.into_inner();
    let compressed_observation = compressed.finish();
    let status = finish_curl(child, measurement.is_err())?;
    let measurement = measurement.map_err(failure)?;
    // bind every complete stream observation
    if !status.success()
        || compressed_observation.bytes != compressed_size
        || compressed_observation.digest != blob_digest
        || measurement.digest != diff_id
    {
        return Err(failure(NativeFamilyReleaseCapacityError::Image));
    }
    Ok(NativeFamilyReleaseRemoteLayer {
        allocated_bytes: measurement.allocated_bytes,
        blob_digest: blob_digest.to_owned(),
        entry_inodes: measurement.entry_inodes,
    })
}

/// settle one curl process and classify only its documented timeout status.
fn finish_curl(
    child: &mut Child,
    failed: bool,
) -> Result<std::process::ExitStatus, SignedLayerAttemptError> {
    let failure = SignedLayerAttemptError::Failure;
    let observed = child
        .try_wait()
        .map_err(|error| failure(measurement_error(error)))?;
    // terminate active failed work before the configured deadline
    let status = if let Some(status) = observed {
        status
    } else if failed {
        let _ = child.kill();
        child
            .wait()
            .map_err(|error| failure(measurement_error(error)))?
    } else {
        child
            .wait()
            .map_err(|error| failure(measurement_error(error)))?
    };
    // expose only curl's fixed deadline for one retry
    if status.code() == Some(28) {
        return Err(SignedLayerAttemptError::Timeout);
    }
    Ok(status)
}

/// parse one bounded HTTP/1.1 response header emitted by fixed curl.
fn read_curl_response_headers(
    input: &mut impl BufRead,
) -> Result<CurlResponseHeaders, NativeFamilyReleaseCapacityError> {
    let mut line = read_bounded_http_line(input, 16_384)?;
    let status = parse_curl_status_line(&line)?;
    let mut total = line.len();
    let mut location = None;
    // retain only bounded single-line headers
    loop {
        line = read_bounded_http_line(input, 16_384 - total)?;
        total += line.len();
        // require canonical line framing
        if !line.ends_with(b"\r\n") {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        // finish only at the canonical empty line
        if line == b"\r\n" {
            break;
        }
        let bytes = &line[..line.len() - 2];
        let colon = bytes
            .iter()
            .position(|byte| *byte == b':')
            .filter(|index| *index > 0)
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let name = &bytes[..colon];
        let value = trim_http_whitespace(&bytes[colon + 1..]);
        // reject folded or control-bearing header material
        if !name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
            || value.iter().any(|byte| *byte < 0x20 || *byte == 0x7f)
        {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        // retain the unique redirect coordinate only
        if name.eq_ignore_ascii_case(b"location") {
            // refuse duplicate redirect coordinates
            if location.is_some() {
                return Err(NativeFamilyReleaseCapacityError::Image);
            }
            location = Some(
                std::str::from_utf8(value)
                    .map_err(|_| NativeFamilyReleaseCapacityError::Image)?
                    .to_owned(),
            );
        }
    }
    Ok(CurlResponseHeaders { location, status })
}

/// read one response-header line without allocating beyond the total cap.
fn read_bounded_http_line(
    input: &mut impl BufRead,
    remaining: usize,
) -> Result<Vec<u8>, NativeFamilyReleaseCapacityError> {
    // refuse a line after the aggregate bound is exhausted
    if remaining == 0 {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let mut line = Vec::new();
    let mut bounded = input.take(remaining as u64 + 1);
    bounded
        .read_until(b'\n', &mut line)
        .map_err(measurement_error)?;
    // retain only one complete bounded line
    if line.is_empty() || line.len() > remaining {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    Ok(line)
}

/// parse the fixed curl HTTP/1.1 status line.
fn parse_curl_status_line(line: &[u8]) -> Result<u16, NativeFamilyReleaseCapacityError> {
    // require curl's forced protocol and canonical framing
    if !line.ends_with(b"\r\n") || line.len() < 14 || !line.starts_with(b"HTTP/1.1 ") {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let code = &line[9..12];
    // retain one three-digit status code
    if !code.iter().all(u8::is_ascii_digit) || line[12] != b' ' {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    std::str::from_utf8(code)
        .ok()
        .and_then(|code| code.parse().ok())
        .filter(|code| (100..=599).contains(code))
        .ok_or(NativeFamilyReleaseCapacityError::Image)
}

/// trim only HTTP optional whitespace.
fn trim_http_whitespace(mut value: &[u8]) -> &[u8] {
    // remove leading optional whitespace
    while value
        .first()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        value = &value[1..];
    }
    // remove trailing optional whitespace
    while value
        .last()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        value = &value[..value.len() - 1];
    }
    value
}

/// start fixed curl with bearer authority supplied only over protected stdin.
fn spawn_authenticated_curl(
    url: &Url,
    token: &str,
) -> Result<Child, NativeFamilyReleaseCapacityError> {
    // keep the stdin curl configuration unambiguous
    if token.is_empty()
        || token.len() > 8_192
        || token
            .bytes()
            .any(|byte| !(0x21..=0x7e).contains(&byte) || matches!(byte, b'"' | b'\\'))
    {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    spawn_configured_curl(
        url,
        Some(("application/vnd.oci.image.layer.v1.tar+gzip", token)),
    )
}

/// start fixed curl with all signed material supplied only over protected stdin.
fn spawn_signed_curl(url: &Url) -> Result<Child, NativeFamilyReleaseCapacityError> {
    spawn_configured_curl(url, None)
}

/// start fixed curl from one closed stdin-only configuration.
fn spawn_configured_curl(
    url: &Url,
    authorization: Option<(&str, &str)>,
) -> Result<Child, NativeFamilyReleaseCapacityError> {
    let text = url.as_str();
    // keep curl configuration unambiguous
    if text
        .bytes()
        .any(|byte| matches!(byte, b'"' | b'\\' | b'\r' | b'\n'))
    {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let mut child = Command::new(CURL)
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(measurement_error)?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| measurement_message("curl stdin unavailable"))?;
    write!(input, "silent\nshow-error\nfail\nhttp1.1\nmax-redirs = 0\nconnect-timeout = 30\nmax-time = 300\nurl = \"{text}\"\n")
        .map_err(measurement_error)?;
    // add bounded response headers and bearer authority only for the registry hop
    if let Some((accept, token)) = authorization {
        write!(input, "suppress-connect-headers\ndump-header = \"-\"\nheader = \"Accept: {accept}\"\nheader = \"Authorization: Bearer {token}\"\n")
            .map_err(measurement_error)?;
    }
    drop(input);
    Ok(child)
}

struct DigestReader<R> {
    inner: R,
    digest: Sha256,
    bytes: u64,
    maximum: u64,
}

impl<R> DigestReader<R> {
    /// initialize one streaming byte and digest observation.
    fn new(inner: R, maximum: u64) -> Self {
        Self {
            inner,
            digest: Sha256::new(),
            bytes: 0,
            maximum,
        }
    }

    /// finish one exact stream observation.
    fn finish(self) -> StreamObservation {
        let digest = self.digest.finalize();
        StreamObservation {
            bytes: self.bytes,
            digest: format_digest(digest.as_ref()),
        }
    }
}

impl<R: Read> Read for DigestReader<R> {
    /// hash only bytes actually returned by the underlying stream.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.bytes = self
            .bytes
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("registry stream size overflow"))?;
        // stop immediately beyond the immutable descriptor
        if self.bytes > self.maximum {
            return Err(io::Error::other("registry stream exceeds its descriptor"));
        }
        self.digest.update(&buffer[..count]);
        Ok(count)
    }
}

struct StreamObservation {
    bytes: u64,
    digest: String,
}

struct TarMeasurement {
    allocated_bytes: u64,
    digest: String,
    entry_inodes: u64,
}

/// parse one complete gzip-decoded tar stream without materializing layer contents.
fn measure_tar_stream(
    input: &mut impl Read,
) -> Result<TarMeasurement, NativeFamilyReleaseCapacityError> {
    let mut digest = Sha256::new();
    let mut allocated_bytes = 0_u64;
    let mut entry_inodes = 0_u64;
    let mut terminators = 0_u64;
    let mut pending_path = None;
    let mut pending_long_link = false;
    let mut stream_bytes = 0_u64;
    // consume one complete bounded tar stream
    while let Some(header) = read_tar_block(input, &mut digest, &mut stream_bytes)? {
        // retain each terminator in the diff-id
        if header.iter().all(|byte| *byte == 0) {
            terminators += 1;
            continue;
        }
        // reject data after a terminator
        if terminators != 0 {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        require_tar_checksum(&header)?;
        let size = parse_tar_octal(&header[124..136])?;
        let kind = if header[156] == 0 { b'0' } else { header[156] };
        let magic = &header[257..263];
        let version = &header[263..265];
        // admit only portable POSIX or GNU ustar headers
        if !((magic == b"ustar\0" && version == b"00") || (magic == b"ustar " && version == b" \0"))
        {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        let name = tar_text(&header[..100])?;
        let prefix = if header[345..500].iter().all(|byte| *byte == 0) {
            None
        } else {
            Some(tar_text(&header[345..500])?)
        };
        let header_path = prefix.map_or(name.clone(), |prefix| format!("{prefix}/{name}"));
        let stored = size.div_ceil(512) * 512;
        // capture only the source-approved extensions
        if matches!(kind, b'x' | b'L' | b'K') {
            // keep extension sequencing closed
            if pending_path.is_some() || pending_long_link || !(1..=16_384).contains(&size) {
                return Err(NativeFamilyReleaseCapacityError::Image);
            }
            account_tar_path(&header_path, size, &mut allocated_bytes, &mut entry_inodes)?;
            let body = read_tar_payload(input, &mut digest, &mut stream_bytes, size, stored)?;
            // retain a PAX path or GNU long value
            if kind == b'x' {
                pending_path = parse_pax_path(&body)?;
            } else {
                let value = body
                    .strip_suffix(&[0])
                    .filter(|value| !value.contains(&0))
                    .ok_or(NativeFamilyReleaseCapacityError::Image)?;
                let value = std::str::from_utf8(value)
                    .map_err(|_| NativeFamilyReleaseCapacityError::Image)?;
                validate_tar_path(value)?;
                // distinguish path and link extensions
                if kind == b'L' {
                    pending_path = Some(value.to_owned());
                } else {
                    pending_long_link = true;
                }
            }
            continue;
        }
        // retain only materializable entry kinds
        if !matches!(kind, b'0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6')
            || (pending_long_link && !matches!(kind, b'1' | b'2'))
        {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        let path = pending_path.take().unwrap_or(header_path);
        account_tar_path(&path, size, &mut allocated_bytes, &mut entry_inodes)?;
        pending_long_link = false;
        discard_tar_payload(input, &mut digest, &mut stream_bytes, stored)?;
    }
    // require complete framing and settled extensions
    if terminators < 2 || pending_path.is_some() || pending_long_link || entry_inodes == 0 {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let digest = digest.finalize();
    Ok(TarMeasurement {
        allocated_bytes: allocated(allocated_bytes)
            .ok_or(NativeFamilyReleaseCapacityError::Accounting)?,
        digest: format_digest(digest.as_ref()),
        entry_inodes,
    })
}

/// read one complete tar block while retaining the diff-id digest.
fn read_tar_block(
    input: &mut impl Read,
    digest: &mut Sha256,
    stream_bytes: &mut u64,
) -> Result<Option<[u8; 512]>, NativeFamilyReleaseCapacityError> {
    let mut block = [0_u8; 512];
    let mut offset = 0;
    // fill exactly one header block
    while offset < block.len() {
        let count = input
            .read(&mut block[offset..])
            .map_err(measurement_error)?;
        // distinguish clean EOF from a partial header
        if count == 0 {
            // admit EOF only between blocks
            if offset == 0 {
                return Ok(None);
            }
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        offset += count;
    }
    observe_tar_bytes(stream_bytes, block.len() as u64)?;
    digest.update(block);
    Ok(Some(block))
}

/// discard one padded body while retaining the complete diff-id digest.
fn discard_tar_payload(
    input: &mut impl Read,
    digest: &mut Sha256,
    stream_bytes: &mut u64,
    mut bytes: u64,
) -> Result<(), NativeFamilyReleaseCapacityError> {
    let mut buffer = [0_u8; 32 * 1024];
    // consume each bounded payload chunk
    while bytes != 0 {
        let wanted = usize::try_from(bytes.min(buffer.len() as u64))
            .map_err(|_| NativeFamilyReleaseCapacityError::Image)?;
        input
            .read_exact(&mut buffer[..wanted])
            .map_err(measurement_error)?;
        observe_tar_bytes(stream_bytes, wanted as u64)?;
        digest.update(&buffer[..wanted]);
        bytes -= wanted as u64;
    }
    Ok(())
}

/// retain one bounded extension payload and discard its tar padding.
fn read_tar_payload(
    input: &mut impl Read,
    digest: &mut Sha256,
    stream_bytes: &mut u64,
    size: u64,
    stored: u64,
) -> Result<Vec<u8>, NativeFamilyReleaseCapacityError> {
    let mut body = vec![0_u8; size as usize];
    input.read_exact(&mut body).map_err(measurement_error)?;
    observe_tar_bytes(stream_bytes, size)?;
    digest.update(&body);
    discard_tar_payload(input, digest, stream_bytes, stored - size)?;
    Ok(body)
}

/// enforce the predecessor's complete uncompressed stream bound.
fn observe_tar_bytes(
    stream_bytes: &mut u64,
    added: u64,
) -> Result<(), NativeFamilyReleaseCapacityError> {
    *stream_bytes = stream_bytes
        .checked_add(added)
        .filter(|bytes| *bytes <= MAXIMUM_UNPACKED_LAYER_BYTES)
        .ok_or(NativeFamilyReleaseCapacityError::Image)?;
    Ok(())
}

/// validate one POSIX tar checksum.
fn require_tar_checksum(header: &[u8; 512]) -> Result<(), NativeFamilyReleaseCapacityError> {
    let field = &header[148..156];
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    let text = std::str::from_utf8(&field[..end])
        .map_err(|_| NativeFamilyReleaseCapacityError::Image)?
        .trim();
    // preserve the checksum field's narrower source grammar
    if text.is_empty() || text.len() > 7 || !text.bytes().all(|byte| matches!(byte, b'0'..=b'7')) {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let expected =
        u64::from_str_radix(text, 8).map_err(|_| NativeFamilyReleaseCapacityError::Image)?;
    let actual = header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            // treat checksum bytes as spaces
            if (148..156).contains(&index) {
                0x20
            } else {
                *byte as u64
            }
        })
        .sum::<u64>();
    (actual == expected)
        .then_some(())
        .ok_or(NativeFamilyReleaseCapacityError::Image)
}

/// parse one canonical null-padded tar octal field.
fn parse_tar_octal(bytes: &[u8]) -> Result<u64, NativeFamilyReleaseCapacityError> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let text = std::str::from_utf8(&bytes[..end])
        .map_err(|_| NativeFamilyReleaseCapacityError::Image)?
        .trim();
    // reject base-256 and noncanonical octal values
    if text.is_empty() || text.len() > 11 || !text.bytes().all(|byte| matches!(byte, b'0'..=b'7')) {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    u64::from_str_radix(text, 8).map_err(|_| NativeFamilyReleaseCapacityError::Image)
}

/// decode one null-padded canonical tar path field.
fn tar_text(bytes: &[u8]) -> Result<String, NativeFamilyReleaseCapacityError> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    // require exact null padding
    if bytes[end..].iter().any(|byte| *byte != 0) {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let value =
        std::str::from_utf8(&bytes[..end]).map_err(|_| NativeFamilyReleaseCapacityError::Image)?;
    validate_tar_path(value)?;
    Ok(value.to_owned())
}

/// validate and normalize one archive path without widening extraction semantics.
fn validate_tar_path(path: &str) -> Result<Vec<&str>, NativeFamilyReleaseCapacityError> {
    // reject empty or oversized decoded names
    if path.is_empty() || path.len() > 4_095 {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    let mut normalized = path;
    // discard only explicit current-directory prefixes
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped;
    }
    normalized = normalized.strip_suffix('/').unwrap_or(normalized);
    // preserve root-like current-directory entries
    if normalized.is_empty() || normalized == "." {
        return Ok(Vec::new());
    }
    let parts = normalized.split('/').collect::<Vec<_>>();
    // retain only extraction-safe relative parts
    if normalized.starts_with('/')
        || normalized
            .bytes()
            .any(|byte| byte < 0x20 || byte == 0x7f || byte == b'\\')
        || parts
            .iter()
            .any(|part| part.is_empty() || matches!(*part, "." | ".."))
    {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    Ok(parts)
}

/// charge one materialized entry and its conservative implicit parents.
fn account_tar_path(
    path: &str,
    size: u64,
    allocated_bytes: &mut u64,
    entry_inodes: &mut u64,
) -> Result<(), NativeFamilyReleaseCapacityError> {
    let parts = validate_tar_path(path)?;
    let parents = parts.len().saturating_sub(1) as u64;
    *entry_inodes = sum([*entry_inodes, 1, parents])?;
    // preserve the conservative inode bound
    if *entry_inodes > MAXIMUM_LAYER_INODES {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    *allocated_bytes = sum([
        *allocated_bytes,
        allocated(size).ok_or(NativeFamilyReleaseCapacityError::Accounting)?,
        allocated(path.len() as u64 + 1).ok_or(NativeFamilyReleaseCapacityError::Accounting)?,
        parents * BLOCK_BYTES,
    ])?;
    // preserve the conservative allocation bound
    if *allocated_bytes > MAXIMUM_UNPACKED_LAYER_BYTES {
        return Err(NativeFamilyReleaseCapacityError::Image);
    }
    Ok(())
}

/// decode one bounded PAX body and retain only its optional path override.
fn parse_pax_path(bytes: &[u8]) -> Result<Option<String>, NativeFamilyReleaseCapacityError> {
    let allowed = [
        "atime", "ctime", "gid", "gname", "linkpath", "mtime", "path", "uid", "uname",
    ];
    let mut offset = 0;
    let mut path = None;
    // validate every framed local PAX record
    while offset < bytes.len() {
        let space = bytes[offset..]
            .iter()
            .position(|byte| *byte == b' ')
            .map(|index| offset + index)
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        // preserve the source length-field position
        if space < offset + 2 || space > offset + 8 {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        let length_text = std::str::from_utf8(&bytes[offset..space])
            .map_err(|_| NativeFamilyReleaseCapacityError::Image)?;
        // preserve the source decimal grammar
        if length_text.len() < 2
            || length_text.len() > 7
            || length_text.as_bytes()[0] == b'0'
            || !length_text.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        let length = length_text
            .parse::<usize>()
            .ok()
            .filter(|length| *length >= 2)
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= bytes.len() && bytes[*end - 1] == b'\n')
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let record = &bytes[space + 1..end - 1];
        let equals = record
            .iter()
            .position(|byte| *byte == b'=')
            .filter(|index| *index > 0)
            .ok_or(NativeFamilyReleaseCapacityError::Image)?;
        let key = std::str::from_utf8(&record[..equals])
            .map_err(|_| NativeFamilyReleaseCapacityError::Image)?;
        // refuse unsupported or repeated path ownership
        if !allowed.contains(&key) || (key == "path" && path.is_some()) {
            return Err(NativeFamilyReleaseCapacityError::Image);
        }
        // retain only the extraction path override
        if key == "path" {
            let value = std::str::from_utf8(&record[equals + 1..])
                .map_err(|_| NativeFamilyReleaseCapacityError::Image)?;
            validate_tar_path(value)?;
            path = Some(value.to_owned());
        }
        offset = end;
    }
    Ok(path)
}

#[derive(Clone, Copy)]
struct InstalledPathMeasurement {
    bytes: u64,
    inodes: u64,
}

/// measure actual allocated blocks and entries without following links.
fn measure_installed_path(
    path: &Path,
) -> Result<InstalledPathMeasurement, NativeFamilyReleaseCapacityError> {
    let metadata = fs::symlink_metadata(path).map_err(measurement_error)?;
    let kind = metadata.file_type();
    let whiteout = kind.is_char_device() && metadata.rdev() == 0;
    // admit only ordinary overlay entries
    if !kind.is_dir() && !kind.is_file() && !kind.is_symlink() && !whiteout {
        return Err(NativeFamilyReleaseCapacityError::Inventory);
    }
    let mut measurement = InstalledPathMeasurement {
        bytes: allocated(
            metadata
                .blocks()
                .checked_mul(512)
                .ok_or(NativeFamilyReleaseCapacityError::Accounting)?,
        )
        .ok_or(NativeFamilyReleaseCapacityError::Accounting)?,
        inodes: 1,
    };
    // descend through real directories only
    if kind.is_dir() {
        // measure each literal child once
        for entry in fs::read_dir(path).map_err(measurement_error)? {
            let child = measure_installed_path(&entry.map_err(measurement_error)?.path())?;
            measurement.bytes = sum([measurement.bytes, child.bytes])?;
            measurement.inodes = sum([measurement.inodes, child.inodes])?;
        }
    }
    Ok(measurement)
}

/// inspect and bind one local immutable image to its exact overlay paths.
fn inspect_installed_local_image(
    reference: &str,
    config_digest: &str,
    expected_diff_ids: &[String],
) -> Result<Vec<NativeFamilyReleaseLocalLayer>, NativeFamilyReleaseCapacityError> {
    let output = run_bounded_process(
        "docker",
        ["image", "inspect", "--format", "{{json .}}", reference],
        DOCKER_OUTPUT_MAXIMUM,
    )?;
    let Some(output) = output else {
        return Ok(Vec::new());
    };
    let value = NativeWirePrimitives
        .decode_json(&output)
        .ok_or(NativeFamilyReleaseCapacityError::Inventory)?;
    let graph = field(&value, "GraphDriver").ok_or(NativeFamilyReleaseCapacityError::Inventory)?;
    let data = field(graph, "Data").ok_or(NativeFamilyReleaseCapacityError::Inventory)?;
    let rootfs = field(&value, "RootFS").ok_or(NativeFamilyReleaseCapacityError::Inventory)?;
    let actual_diff_ids =
        string_array(rootfs, "Layers").ok_or(NativeFamilyReleaseCapacityError::Inventory)?;
    // bind the local platform and exact registry diff ids
    if text(&value, "Id").as_deref() != Some(config_digest)
        || text(&value, "Architecture").as_deref() != Some("arm64")
        || text(&value, "Os").as_deref() != Some("linux")
        || text(graph, "Name").as_deref() != Some("overlay2")
        || actual_diff_ids != expected_diff_ids
    {
        return Err(NativeFamilyReleaseCapacityError::Inventory);
    }
    let mut paths = text(data, "LowerDir")
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .split(':')
                .rev()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // retain the writable top layer when present
    if let Some(upper) = text(data, "UpperDir").filter(|value| !value.is_empty()) {
        paths.push(PathBuf::from(upper));
    }
    // keep every layer inside the fixed overlay root
    if paths.len() != expected_diff_ids.len()
        || paths.iter().any(|path| {
            !path.starts_with(DOCKER_LAYER_ROOT) || path.file_name() != Some(OsStr::new("diff"))
        })
    {
        return Err(NativeFamilyReleaseCapacityError::Inventory);
    }
    let mut parents = Vec::new();
    let mut inventory = Vec::with_capacity(paths.len());
    // measure each recursive parent chain once
    for (path, diff_id) in paths.iter().zip(expected_diff_ids) {
        parents.push(diff_id.clone());
        let measured = measure_installed_path(path)?;
        inventory.push(NativeFamilyReleaseLocalLayer {
            allocated_bytes: measured.bytes,
            chain_id: docker_chain_identity(&parents)
                .ok_or(NativeFamilyReleaseCapacityError::Inventory)?,
        });
    }
    Ok(inventory)
}

/// run one fixed process while refusing oversized output.
fn run_bounded_process<const N: usize>(
    program: &str,
    arguments: [&str; N],
    maximum: usize,
) -> Result<Option<Vec<u8>>, NativeFamilyReleaseCapacityError> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(measurement_error)?;
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(|| measurement_message("process stdout unavailable"))?
        .take(maximum as u64 + 1)
        .read_to_end(&mut output)
        .map_err(measurement_error)?;
    // terminate before retaining oversized output
    if output.len() > maximum {
        let _ = child.kill();
        let _ = child.wait();
        return Err(NativeFamilyReleaseCapacityError::Inventory);
    }
    let status = child.wait().map_err(measurement_error)?;
    Ok(status.success().then_some(output))
}

/// project one digest state into the OCI spelling.
fn format_digest(bytes: &[u8]) -> String {
    let mut output = String::from("sha256:");
    // encode each digest byte once
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("string formatting");
    }
    output
}

/// erase one installed effect error into the closed measurement category.
fn measurement_error(error: impl Display) -> NativeFamilyReleaseCapacityError {
    NativeFamilyReleaseCapacityError::Measurement(error.to_string())
}

/// construct one static installed-effect diagnostic.
fn measurement_message(message: &'static str) -> NativeFamilyReleaseCapacityError {
    NativeFamilyReleaseCapacityError::Measurement(message.to_owned())
}

#[cfg(test)]
#[path = "native_family_release_capacity_test.rs"]
mod tests;
