//! Prove worker startup retains every reviewed original capture epoch variant.

mod support {
    pub mod capture_epoch;
}

use support::capture_epoch::node_capture_epoch_cases;
use weather_contracts::AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum;
use weather_runtime::{
    CaptureEpochWitnessError, CaptureEpochWitnessFile, RetainedCaptureEpochWitnessLoader,
    WorkerLoadedSidecarAdmission,
};

struct FixtureFile(Vec<u8>);

impl CaptureEpochWitnessFile for FixtureFile {
    /// Return the exact installed fixture bytes through the runtime's read boundary.
    fn read_installed_witness(&mut self) -> Result<Vec<u8>, CaptureEpochWitnessError> {
        Ok(std::mem::take(&mut self.0))
    }
}

/// Preserve all original v14 and v17 combinations through actual worker startup loading.
#[test]
fn retains_original_capture_epoch_tuples_from_node() {
    use AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::{Value14, Value17};

    let cases = node_capture_epoch_cases();
    assert_eq!(cases.len(), 4);
    let versions = [Value14, Value14, Value17, Value17];
    // mirror the frozen Node cases using the shared Rust verifier and loader
    for (case, version) in cases.into_iter().zip(versions) {
        let mut loader = RetainedCaptureEpochWitnessLoader::new(
            FixtureFile(case.bytes.clone()),
            case.clone(),
            case.clone(),
        );
        let mut loaded = WorkerLoadedSidecarAdmission::default();
        loaded.load_capture_epoch(&mut loader);
        let witness = loaded
            .capture_epoch_witness()
            .expect("the original witness is admitted");
        assert_eq!(witness.capture_epoch, case.clock);
        assert_eq!(witness.witness.witnessSha256, case.digest);
        assert_eq!(witness.witness.controlPlaneVersion, version);
    }
}
