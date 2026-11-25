use weather_contracts::AdjustmentShadowFamily;

use super::{
    MAXIMUM_IMAGE_MEMBER_BYTES, NativeFamilyCatalogImageFiles, verify_native_family_catalog_image,
};
use crate::{
    ApiMaintenanceCandidateFiles, ApiQualifiedCanaryFiles, FamilyTargetImageRead,
    MaintenanceAnchorStatusError, RainMaintenanceCandidateFiles, WorkerRainControlReferenceFiles,
    WorkerRainServingFiles,
};

const HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const EMPTY_CATALOG: &[u8] =
    b"{\"contractVersion\":\"adjustment-installed-candidate-catalog/v3\",\"entries\":[]}\n";

#[derive(Default)]
struct FixtureImage {
    reads: Vec<(String, usize)>,
}

impl FamilyTargetImageRead for FixtureImage {
    /// Record exact member selection and return distinct ordinary JSON bytes.
    fn read_member(
        &mut self,
        path: &str,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
        self.reads.push((path.to_owned(), maximum_bytes));
        Ok(format!("{{\"path\":{path:?}}}\n").into_bytes())
    }
}

/// Verify the real validators accept a genuine empty catalog without reading image members.
#[test]
fn verifies_empty_catalog_with_native_candidate_validators() {
    let mut image = FixtureImage::default();

    verify_native_family_catalog_image(EMPTY_CATALOG, &mut image).unwrap();

    assert!(image.reads.is_empty());
}

/// Preserve every original fixed and content-addressed installed-image member path.
#[test]
fn selects_exact_installed_candidate_member_paths() {
    let mut image = FixtureImage::default();
    {
        let mut files = NativeFamilyCatalogImageFiles::new(EMPTY_CATALOG, &mut image);
        files
            .read_members(AdjustmentShadowFamily::Temperature, HASH, HASH, HASH)
            .unwrap();
        files
            .read_qualified_canary_members(AdjustmentShadowFamily::Wind, HASH, HASH)
            .unwrap();
        files.read_rain_members(HASH, HASH, HASH).unwrap();
        files.read_rain_control_members(HASH, HASH).unwrap();
        files.read_worker_rain_control_selector(HASH).unwrap();
        files.read_worker_rain_registry().unwrap();
        files
            .read_worker_rain_serving_members(HASH, HASH, HASH)
            .unwrap();
    }

    // retain image-read order while checking the common owner bound
    let paths = image
        .reads
        .iter()
        .map(|(path, maximum)| {
            assert_eq!(*maximum, MAXIMUM_IMAGE_MEMBER_BYTES);
            path.as_str()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            format!("config/forecast-adjustments/ballydidean/actions/sha256-{HASH}.json"),
            format!(
                "config/forecast-adjustments/ballydidean/shadow-catalog/temperature/sha256-{HASH}.json"
            ),
            format!(
                "config/forecast-adjustments/ballydidean/model-parity/temperature/sha256-{HASH}.json"
            ),
            format!(
                "config/forecast-adjustments/ballydidean/temperature-canary-bundles/sha256-{HASH}.json"
            ),
            "config/forecast-adjustments/ballydidean-wind-canary.json".to_owned(),
            format!("config/forecast-adjustments/ballydidean/actions/sha256-{HASH}.json"),
            format!(
                "config/forecast-adjustments/ballydidean/wind-canary-bundles/sha256-{HASH}.json"
            ),
            format!("config/forecast-adjustments/ballydidean/actions/sha256-{HASH}.json"),
            format!(
                "config/forecast-adjustments/ballydidean/shadow-catalog/rain/sha256-{HASH}.json"
            ),
            format!("config/forecast-adjustments/ballydidean/model-parity/rain/sha256-{HASH}.json"),
            format!(
                "config/forecast-adjustments/ballydidean/rain-runtime-artifacts/sha256-{HASH}.json"
            ),
            format!(
                "config/forecast-adjustments/ballydidean/rain-model-packages/sha256-{HASH}.json"
            ),
            format!(
                "config/forecast-adjustments/ballydidean/rain-maintenance-control-states/sha256-{HASH}.json"
            ),
            format!(
                "config/forecast-adjustments/ballydidean/rain-runtime-artifacts/sha256-{HASH}.json"
            ),
            "config/forecast-adjustments/ballydidean-rain-control-reference.json".to_owned(),
            format!("config/forecast-adjustments/ballydidean/actions/sha256-{HASH}.json"),
            "config/forecast-adjustments/ballydidean-rain-runtime.json".to_owned(),
            "config/forecast-adjustments/ballydidean-rain-runtime.json".to_owned(),
            format!("config/forecast-adjustments/ballydidean/actions/sha256-{HASH}.json"),
            format!(
                "config/forecast-adjustments/ballydidean/rain-runtime-artifacts/sha256-{HASH}.json"
            ),
            format!(
                "config/forecast-adjustments/ballydidean/rain-model-packages/sha256-{HASH}.json"
            ),
        ]
    );
}

/// Reject a non-content-addressed member before invoking the immutable image owner.
#[test]
fn rejects_invalid_member_digest_before_image_read() {
    let mut image = FixtureImage::default();
    {
        let mut files = NativeFamilyCatalogImageFiles::new(EMPTY_CATALOG, &mut image);
        assert!(
            files
                .read_members(AdjustmentShadowFamily::Wind, "not-a-hash", HASH, HASH)
                .is_err()
        );
    }
    assert!(image.reads.is_empty());
}
