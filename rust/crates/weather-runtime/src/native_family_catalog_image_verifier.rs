//! Native compiled-catalog verification against one immutable selected release image.

use weather_contracts::AdjustmentShadowFamily;

use crate::api_maintenance_candidate::valid_hash;
use crate::{
    ApiMaintenanceCandidateError, ApiMaintenanceCandidateFiles, ApiMaintenanceCandidateMembers,
    ApiQualifiedCanaryFiles, ApiQualifiedCanaryMembers, FamilyCatalogImageVerification,
    FamilyTargetImageRead, MaintenanceAnchorStatusError, NativeFamilyTargetImage,
    NativeModelEvidenceValues, NativeWirePrimitives, RainMaintenanceCandidateFiles,
    WorkerRainCandidateMembers, WorkerRainControlMembers, WorkerRainControlReferenceFiles,
    WorkerRainControlSelectorMembers, WorkerRainServingFiles, WorkerRainServingMembers,
    verify_installed_family_catalog,
};

use ApiMaintenanceCandidateError::File;
use MaintenanceAnchorStatusError::Slot;

const MAXIMUM_IMAGE_MEMBER_BYTES: usize = 8 * 1024 * 1024;
const FORECAST_ADJUSTMENT_ROOT: &str = "config/forecast-adjustments/ballydidean";
const RAIN_CONTROL_SELECTOR: &str =
    "config/forecast-adjustments/ballydidean-rain-control-reference.json";
const RAIN_RUNTIME_REGISTRY: &str = "config/forecast-adjustments/ballydidean-rain-runtime.json";

/// Production verifier for the original complete installed family candidate set.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeFamilyCatalogImageVerifier;

impl NativeFamilyCatalogImageVerifier {
    /// Construct the zero-state compiled-catalog verifier.
    pub const fn new() -> Self {
        Self
    }
}

impl FamilyCatalogImageVerification for NativeFamilyCatalogImageVerifier {
    /// Verify supplied catalog bytes with candidate material from the exact immutable image.
    fn verify_catalog(
        &mut self,
        image: &str,
        bytes: &[u8],
    ) -> Result<(), MaintenanceAnchorStatusError> {
        let mut image = NativeFamilyTargetImage::native(image)?;
        verify_native_family_catalog_image(bytes, &mut image)
    }
}

/// Run every original native candidate validator against one already bound image reader.
pub fn verify_native_family_catalog_image(
    catalog: &[u8],
    image: &mut impl FamilyTargetImageRead,
) -> Result<(), MaintenanceAnchorStatusError> {
    let mut files = NativeFamilyCatalogImageFiles::new(catalog, image);
    let mut codec = NativeWirePrimitives::new();
    let values = NativeModelEvidenceValues::new();
    verify_installed_family_catalog(&mut files, &mut codec, &values).map_err(|_| Slot)
}

/// Candidate-file owner that retains supplied catalog bytes and reads members from one image.
struct NativeFamilyCatalogImageFiles<'a, Image> {
    catalog: &'a [u8],
    image: &'a mut Image,
}

impl<'a, Image> NativeFamilyCatalogImageFiles<'a, Image>
where
    Image: FamilyTargetImageRead,
{
    /// Bind all subsequent member reads to one selected immutable image owner.
    const fn new(catalog: &'a [u8], image: &'a mut Image) -> Self {
        Self { catalog, image }
    }

    /// Read one fixed image member through the image owner's retained resource bounds.
    fn read(&mut self, path: &str) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
        self.image
            .read_member(path, MAXIMUM_IMAGE_MEMBER_BYTES)
            .map_err(|_| File)
    }

    /// Read one exact content-addressed member below the installed adjustment root.
    fn read_addressed(
        &mut self,
        directory: &str,
        digest: &str,
    ) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
        // reject names that cannot be an original content address
        if !valid_hash(digest) {
            return Err(File);
        }
        self.read(&format!(
            "{FORECAST_ADJUSTMENT_ROOT}/{directory}/sha256-{digest}.json"
        ))
    }

    /// Read the common action, projection, parity and runtime artifact set in source order.
    fn read_shadow(
        &mut self,
        family: &str,
        bundle_directory: &str,
        action_sha256: &str,
        candidate_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<ApiMaintenanceCandidateMembers, ApiMaintenanceCandidateError> {
        Ok(ApiMaintenanceCandidateMembers {
            action: self.read_addressed("actions", action_sha256)?,
            projection: self
                .read_addressed(&format!("shadow-catalog/{family}"), candidate_sha256)?,
            parity: self.read_addressed(&format!("model-parity/{family}"), candidate_sha256)?,
            bundle: self.read_addressed(bundle_directory, bundle_sha256)?,
        })
    }
}

impl<Image> ApiMaintenanceCandidateFiles for NativeFamilyCatalogImageFiles<'_, Image>
where
    Image: FamilyTargetImageRead,
{
    /// Return the exact externally selected catalog for every independent family loader.
    fn read_catalog(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
        Ok(self.catalog.to_vec())
    }

    /// Read the installed temperature or wind shadow material from the selected image.
    fn read_members(
        &mut self,
        family: AdjustmentShadowFamily,
        action_sha256: &str,
        candidate_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<ApiMaintenanceCandidateMembers, ApiMaintenanceCandidateError> {
        // preserve the original family-to-directory mapping
        let (family, bundle_directory) = match family {
            AdjustmentShadowFamily::Temperature => ("temperature", "temperature-canary-bundles"),
            AdjustmentShadowFamily::Wind => ("wind", "wind-canary-bundles"),
            AdjustmentShadowFamily::Rain => return Err(File),
        };
        self.read_shadow(
            family,
            bundle_directory,
            action_sha256,
            candidate_sha256,
            bundle_sha256,
        )
    }
}

impl<Image> ApiQualifiedCanaryFiles for NativeFamilyCatalogImageFiles<'_, Image>
where
    Image: FamilyTargetImageRead,
{
    /// Read the fixed serving registry and its selected action and bundle in source order.
    fn read_qualified_canary_members(
        &mut self,
        family: AdjustmentShadowFamily,
        action_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<ApiQualifiedCanaryMembers, ApiMaintenanceCandidateError> {
        // preserve the original active registry and bundle pair
        let (registry, bundle_directory) = match family {
            AdjustmentShadowFamily::Temperature => (
                "config/forecast-adjustments/ballydidean-temperature-canary.json",
                "temperature-canary-bundles",
            ),
            AdjustmentShadowFamily::Wind => (
                "config/forecast-adjustments/ballydidean-wind-canary.json",
                "wind-canary-bundles",
            ),
            AdjustmentShadowFamily::Rain => return Err(File),
        };
        Ok(ApiQualifiedCanaryMembers {
            registry: self.read(registry)?,
            action: self.read_addressed("actions", action_sha256)?,
            bundle: self.read_addressed(bundle_directory, bundle_sha256)?,
        })
    }
}

impl<Image> RainMaintenanceCandidateFiles for NativeFamilyCatalogImageFiles<'_, Image>
where
    Image: FamilyTargetImageRead,
{
    /// Read the complete rain shadow candidate and its technical package.
    fn read_rain_members(
        &mut self,
        action_sha256: &str,
        candidate_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<WorkerRainCandidateMembers, ApiMaintenanceCandidateError> {
        Ok(WorkerRainCandidateMembers {
            members: self.read_shadow(
                "rain",
                "rain-runtime-artifacts",
                action_sha256,
                candidate_sha256,
                bundle_sha256,
            )?,
            package: self.read_addressed("rain-model-packages", candidate_sha256)?,
        })
    }

    /// Read the exact fixed-control state and ordinal artifact selected by the catalog.
    fn read_rain_control_members(
        &mut self,
        state_sha256: &str,
        ordinal_sha256: &str,
    ) -> Result<WorkerRainControlMembers, ApiMaintenanceCandidateError> {
        Ok(WorkerRainControlMembers {
            state: self.read_addressed("rain-maintenance-control-states", state_sha256)?,
            ordinal: self.read_addressed("rain-runtime-artifacts", ordinal_sha256)?,
        })
    }
}

impl<Image> WorkerRainControlReferenceFiles for NativeFamilyCatalogImageFiles<'_, Image>
where
    Image: FamilyTargetImageRead,
{
    /// Read the independent rain control selector and its addressed action.
    fn read_worker_rain_control_selector(
        &mut self,
        action_sha256: &str,
    ) -> Result<WorkerRainControlSelectorMembers, ApiMaintenanceCandidateError> {
        Ok(WorkerRainControlSelectorMembers {
            selector: self.read(RAIN_CONTROL_SELECTOR)?,
            action: self.read_addressed("actions", action_sha256)?,
        })
    }
}

impl<Image> WorkerRainServingFiles for NativeFamilyCatalogImageFiles<'_, Image>
where
    Image: FamilyTargetImageRead,
{
    /// Read the installed incumbent rain runtime registry from the selected image.
    fn read_worker_rain_registry(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
        self.read(RAIN_RUNTIME_REGISTRY)
    }

    /// Read the qualified rain registry, action, runtime artifact and package in source order.
    fn read_worker_rain_serving_members(
        &mut self,
        action_sha256: &str,
        candidate_sha256: &str,
        bundle_sha256: &str,
    ) -> Result<WorkerRainServingMembers, ApiMaintenanceCandidateError> {
        Ok(WorkerRainServingMembers {
            registry: self.read(RAIN_RUNTIME_REGISTRY)?,
            action: self.read_addressed("actions", action_sha256)?,
            bundle: self.read_addressed("rain-runtime-artifacts", bundle_sha256)?,
            package: self.read_addressed("rain-model-packages", candidate_sha256)?,
        })
    }
}

#[cfg(test)]
#[path = "native_family_catalog_image_verifier_test.rs"]
mod tests;
