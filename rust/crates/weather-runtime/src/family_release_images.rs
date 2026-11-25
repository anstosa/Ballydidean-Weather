//! Compose the original target and compensation image identities before any family mutation.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use crate::family_target_catalog::canonical_member;
use crate::family_target_catalog_validation::validate_family_target_catalog;
use crate::family_target_preflight::prepare_family_target_material;
use crate::maintenance_canonical_json::text;
use crate::{
    FamilyActionAuthorityFiles, FamilyTargetImageRead, FamilyTargetPreflightInput,
    FamilyTargetWitnessRead, MaintenanceAnchorStatusError, MaintenanceAnchorStatusJson,
    compute_family_release_identity, compute_family_release_image_identity,
};

use MaintenanceAnchorStatusError::Slot;

const MAXIMUM_CATALOG_BYTES: usize = 256 * 1024;
const MAXIMUM_REGISTRY_BYTES: usize = 8 * 1024 * 1024;

/// The original independently selected target and compensation images and catalog bytes.
pub struct FamilyReleaseImagesInput<'a> {
    pub family: &'a str,
    pub baseline_catalog_bytes: &'a [u8],
    pub target_catalog_bytes: &'a [u8],
    pub source_environment: &'a JsonValue,
    pub target_environment: &'a JsonValue,
    pub compensation_environment: &'a JsonValue,
}

/// Exact immutable identities retained in the original durable family proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyReleaseImageIdentities {
    pub target_family: Sha256Digest,
    pub compensation_family: Sha256Digest,
    pub target_images: Sha256Digest,
    pub compensation_images: Sha256Digest,
}

/// A registry member reader irrevocably bound to the same immutable image as the environment.
pub trait FamilyReleaseBoundImageRead: FamilyTargetImageRead {
    /// Expose the exact fixed selected image reference without reading any image material.
    fn image_reference(&self) -> &str;
}

/// The selected generation's compiled catalog verifier, separate from member reads.
pub trait FamilyCatalogImageVerification {
    /// Require the original complete candidate/control loader verification for these bytes.
    fn verify_catalog(
        &mut self,
        image: &str,
        bytes: &[u8],
    ) -> Result<(), MaintenanceAnchorStatusError>;
}

/// Verify both selected catalogs, then bind all three original registry reads to the durable proof.
pub fn verify_family_release_images(
    input: &FamilyReleaseImagesInput<'_>,
    source: &mut impl FamilyReleaseBoundImageRead,
    target: &mut impl FamilyReleaseBoundImageRead,
    compensation: &mut impl FamilyReleaseBoundImageRead,
    verifier: &mut impl FamilyCatalogImageVerification,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<FamilyReleaseImageIdentities, MaintenanceAnchorStatusError> {
    require_bound_image(source, input.source_environment)?;
    require_bound_image(target, input.target_environment)?;
    require_bound_image(compensation, input.compensation_environment)?;
    let target_catalog = decode_catalog(input.target_catalog_bytes, codec, time)?;
    let baseline = decode_catalog(input.baseline_catalog_bytes, codec, time)?;
    // the original target and compensation compiled loader checks precede any registry read
    verifier.verify_catalog(
        &server_image(input.target_environment)?,
        input.target_catalog_bytes,
    )?;
    verifier.verify_catalog(
        &server_image(input.compensation_environment)?,
        input.baseline_catalog_bytes,
    )?;

    let registry = registry_path(input.family)?;
    let source_registry = read_registry(source, registry)?;
    let target_registry = read_registry(target, registry)?;
    let compensation_registry = read_registry(compensation, registry)?;
    // do not authorize compensation unless it restores the exact pre-action serving bytes
    if compensation_registry != source_registry {
        return Err(Slot);
    }
    let target_family =
        compute_family_release_identity(input.family, &target_registry, &target_catalog, codec)?;
    let compensation_family =
        compute_family_release_identity(input.family, &compensation_registry, &baseline, codec)?;
    let target_images = compute_family_release_image_identity(input.target_environment, codec)?;
    let compensation_images =
        compute_family_release_image_identity(input.compensation_environment, codec)?;

    Ok(FamilyReleaseImageIdentities {
        target_family,
        compensation_family,
        target_images,
        compensation_images,
    })
}

/// The separately selected source, target, compensation and loader verification owners.
pub struct FamilyReleaseImagePorts<'a, Source, Target, Compensation, Verifier> {
    pub source: &'a mut Source,
    pub target: &'a mut Target,
    pub compensation: &'a mut Compensation,
    pub verifier: &'a mut Verifier,
}

/// The original current and separately selected compensation release environments.
pub struct FamilyReleaseEnvironments<'a> {
    pub source: &'a JsonValue,
    pub compensation: &'a JsonValue,
}

/// Ready-for-durability original family authority, target catalog and immutable image identities.
#[derive(Debug, PartialEq)]
pub struct PreparedFamilyReleaseMaterial {
    pub authority: JsonValue,
    pub target_catalog_bytes: Vec<u8>,
    pub identities: FamilyReleaseImageIdentities,
}

/// Compose all original pre-mutation custody, catalog and release image material checks.
pub fn prepare_family_release_material(
    input: &FamilyTargetPreflightInput<'_>,
    environments: &FamilyReleaseEnvironments<'_>,
    files: &mut impl FamilyActionAuthorityFiles,
    images: &mut FamilyReleaseImagePorts<
        '_,
        impl FamilyReleaseBoundImageRead,
        impl FamilyReleaseBoundImageRead,
        impl FamilyReleaseBoundImageRead,
        impl FamilyCatalogImageVerification,
    >,
    witness: &mut impl FamilyTargetWitnessRead,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<PreparedFamilyReleaseMaterial, MaintenanceAnchorStatusError> {
    // refuse a substituted reader before any target material can be observed
    require_bound_image(images.source, environments.source)?;
    require_bound_image(images.target, input.environment)?;
    require_bound_image(images.compensation, environments.compensation)?;
    // preserve the exact same target image owner for both construction and subsequent registry read
    let (authority, target_catalog_bytes) =
        prepare_family_target_material(input, files, images.target, witness, codec, time)?;
    let identities = verify_family_release_images(
        &FamilyReleaseImagesInput {
            family: input.family,
            baseline_catalog_bytes: input.source_bytes,
            target_catalog_bytes: &target_catalog_bytes,
            source_environment: environments.source,
            target_environment: input.environment,
            compensation_environment: environments.compensation,
        },
        images.source,
        images.target,
        images.compensation,
        images.verifier,
        codec,
        time,
    )?;
    Ok(PreparedFamilyReleaseMaterial {
        authority,
        target_catalog_bytes,
        identities,
    })
}

/// One target or compensation proof reloaded immediately before an original catalog mutation.
pub struct FamilyReleaseRebindInput<'a> {
    pub family: &'a str,
    pub catalog_bytes: &'a [u8],
    pub environment: &'a JsonValue,
    pub expected_family_sha256: &'a str,
    pub expected_images_sha256: &'a str,
}

/// Rebind immutable image and family state before verifying the selected catalog again.
pub fn verify_family_release_rebind(
    input: &FamilyReleaseRebindInput<'_>,
    image: &mut impl FamilyReleaseBoundImageRead,
    verifier: &mut impl FamilyCatalogImageVerification,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), MaintenanceAnchorStatusError> {
    require_bound_image(image, input.environment)?;
    let catalog = decode_catalog(input.catalog_bytes, codec, time)?;
    let registry = read_registry(image, registry_path(input.family)?)?;
    // preserve the original short-circuit image check before comparing the family receipt
    if compute_family_release_image_identity(input.environment, codec)?.as_str()
        != input.expected_images_sha256
        || compute_family_release_identity(input.family, &registry, &catalog, codec)?.as_str()
            != input.expected_family_sha256
    {
        return Err(Slot);
    }
    verifier.verify_catalog(&server_image(input.environment)?, input.catalog_bytes)
}

// admit the original bounded canonical operator catalog before interpreting a receipt
fn decode_catalog(
    bytes: &[u8],
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    let value = canonical_member(bytes, MAXIMUM_CATALOG_BYTES, codec)?;
    validate_family_target_catalog(&value, codec, time)?;
    Ok(value)
}

// require the member reader to name the same exact immutable image as the selected environment
fn require_bound_image(
    image: &impl FamilyReleaseBoundImageRead,
    environment: &JsonValue,
) -> Result<(), MaintenanceAnchorStatusError> {
    // do not attest bytes selected from a different image than the loader and digest proof
    if image.image_reference() != server_image(environment)? {
        return Err(Slot);
    }
    Ok(())
}

// obtain the already selected immutable server image from the original environment
fn server_image(environment: &JsonValue) -> Result<String, MaintenanceAnchorStatusError> {
    text(environment, "WEATHER_SERVER_IMAGE").ok_or(Slot)
}

// map only the original three fixed serving registry members
fn registry_path(family: &str) -> Result<&'static str, MaintenanceAnchorStatusError> {
    match family {
        "temperature" => Ok("config/forecast-adjustments/ballydidean-temperature-canary.json"),
        "wind" => Ok("config/forecast-adjustments/ballydidean-wind-canary.json"),
        "rain" => Ok("config/forecast-adjustments/ballydidean-rain-runtime.json"),
        _ => Err(Slot),
    }
}

// preserve the original bounded immutable registry byte read
fn read_registry(
    image: &mut impl FamilyTargetImageRead,
    path: &str,
) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
    let bytes = image.read_member(path, MAXIMUM_REGISTRY_BYTES)?;
    // keep the original image-owner bound even for an injected fixture reader
    if !(2..=MAXIMUM_REGISTRY_BYTES).contains(&bytes.len()) {
        return Err(Slot);
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "family_release_images_test.rs"]
mod tests;
