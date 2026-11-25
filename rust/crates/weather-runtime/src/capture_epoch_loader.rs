//! Load the original mounted capture epoch for API and worker startup.

use weather_api::{
    AdjustmentRevisionCaptureWitnessLoader, AdmittedAdjustmentRevisionCaptureWitness,
    CurrentTimeResolver, WeatherApiStartupLoadError,
};

use crate::installed_artifact_files::read_root_read_only_bounded;
use crate::{CaptureEpochWitnessError, MaintenanceCanonicalJson, verify_capture_epoch_witness};

/// Literal installed mount shared by the API and worker.
pub const CAPTURE_EPOCH_WITNESS_PATH: &str = "/run/weather/adjustment-capture-epoch.json";
/// Size bound shared by the file reader and decoded witness verifier.
pub const CAPTURE_EPOCH_WITNESS_MAX_BYTES: usize = 16 * 1_024;

/// Read the installed witness only after admitting its native file authority.
pub trait CaptureEpochWitnessFile {
    /// Read one bounded witness from the currently installed application mount.
    fn read_installed_witness(&mut self) -> Result<Vec<u8>, CaptureEpochWitnessError>;
}

/// Retain the same fixed root-owned file used by the predecessor.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeCaptureEpochWitnessFile;

impl CaptureEpochWitnessFile for NativeCaptureEpochWitnessFile {
    /// Read at most the original witness size plus one overflow marker byte.
    fn read_installed_witness(&mut self) -> Result<Vec<u8>, CaptureEpochWitnessError> {
        read_root_read_only_bounded(CAPTURE_EPOCH_WITNESS_PATH, CAPTURE_EPOCH_WITNESS_MAX_BYTES)
            .map_err(|_| CaptureEpochWitnessError::File)
    }
}

/// Startup adapter sharing the same installed-witness decoding across API and worker.
pub struct RetainedCaptureEpochWitnessLoader<File, Codec, Time> {
    file: File,
    codec: Codec,
    time: Time,
}

impl<File, Codec, Time> RetainedCaptureEpochWitnessLoader<File, Codec, Time> {
    /// Bind explicitly admitted process owners without reading the witness early.
    pub const fn new(file: File, codec: Codec, time: Time) -> Self {
        Self { file, codec, time }
    }
}

impl<Codec, Time> RetainedCaptureEpochWitnessLoader<NativeCaptureEpochWitnessFile, Codec, Time> {
    /// Select the fixed native mount; the startup caller retains timing of the read.
    pub const fn native(codec: Codec, time: Time) -> Self {
        Self::new(NativeCaptureEpochWitnessFile, codec, time)
    }
}

impl<File, Codec, Time> RetainedCaptureEpochWitnessLoader<File, Codec, Time>
where
    File: CaptureEpochWitnessFile,
    Codec: MaintenanceCanonicalJson,
    Time: CurrentTimeResolver,
{
    /// Verify the bounded installed artifact without allowing caller-supplied epoch text.
    pub fn load(
        &mut self,
    ) -> Result<AdmittedAdjustmentRevisionCaptureWitness, CaptureEpochWitnessError> {
        let bytes = self.file.read_installed_witness()?;
        verify_capture_epoch_witness(&bytes, &mut self.codec, &self.time)
    }
}

impl<File, Codec, Time> AdjustmentRevisionCaptureWitnessLoader
    for RetainedCaptureEpochWitnessLoader<File, Codec, Time>
where
    File: CaptureEpochWitnessFile,
    Codec: MaintenanceCanonicalJson,
    Time: CurrentTimeResolver,
{
    /// Keep witness failures contained to optional capture at the original startup boundary.
    fn load_verified_capture_witness(
        &mut self,
    ) -> Result<AdmittedAdjustmentRevisionCaptureWitness, WeatherApiStartupLoadError> {
        self.load().map_err(|_| WeatherApiStartupLoadError)
    }
}
