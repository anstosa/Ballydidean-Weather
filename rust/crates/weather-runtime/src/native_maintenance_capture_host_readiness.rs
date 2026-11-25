//! Concrete workstation source authority composition for native capture readiness.

use crate::{
    NativeMaintenanceCaptureInstalledClosure, NativeMaintenanceCaptureReadiness,
    NativeMaintenanceCaptureReadinessPorts, NativeMaintenanceCaptureRemoteAuthority,
    NativeMaintenanceDatabaseLedgerReader, NativeMaintenanceFitRuntimeProcess,
    NativeMaintenanceFitRuntimeReadiness, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochSnapshot, VerifiedNativeMaintenanceCaptureEpochWitness,
    VerifiedNativeMaintenanceDatabaseLedger, VerifiedNativeMaintenanceLocalArchiveReadiness,
    inspect_native_maintenance_capture_readiness,
    inspect_native_maintenance_local_archive_readiness,
};
use std::{io, path::Path};

/// Bind the real retained predecessor installation and all actual read-only host ports.
///
/// A native successor installation requires a separately verified native binary closure;
/// this predecessor-generation owner cannot authorize it through a caller-selected flag.
pub fn inspect_native_maintenance_predecessor_host_readiness(
    source_commit: &str,
) -> io::Result<NativeMaintenanceCaptureReadiness> {
    let mut installed =
        NativeMaintenanceCaptureInstalledClosure::native_node_predecessor(source_commit)
            .map_err(failed)?;
    let fit = NativeMaintenanceFitRuntimeReadiness::native_node_predecessor(source_commit)
        .map_err(failed)?;
    // bind both fixed remote clients to the same installed release closure before any read
    let root = Path::new("/home/ubuntu/.weather/adjustment-maintenance/runner/releases")
        .join(source_commit);
    let mut ports = HostPrerequisites {
        fit,
        remote: NativeMaintenanceDatabaseLedgerReader::new(&root),
    };
    let mut remote = NativeMaintenanceDatabaseLedgerReader::new(&root);
    let mut source = NativeMaintenanceCaptureRemoteAuthority::new(&mut remote);
    Ok(inspect_native_maintenance_capture_readiness(
        &mut installed,
        &mut ports,
        &mut source,
        &NativeWirePrimitives,
    ))
}

/// Inspect the real installed native generation using only the running executable identity.
pub fn inspect_native_maintenance_installed_host_readiness() -> NativeMaintenanceCaptureReadiness {
    let install = (|| {
        let executable = std::env::current_exe().ok()?;
        let root = executable.parent()?.parent()?;
        let commit = root.file_name()?.to_str()?.to_owned();
        let expected_root =
            Path::new("/home/ubuntu/.weather/adjustment-maintenance/runner/releases").join(&commit);
        // require the actual fixed installed path before constructing any remote or fit owner
        if root != expected_root
            || executable != expected_root.join("bin/weather-maintenance-capture")
        {
            return None;
        }
        let installed =
            crate::NativeMaintenanceCaptureNativeInstallation::native_release(&commit).ok()?;
        let fit = NativeMaintenanceFitRuntimeReadiness::native_installed(&commit).ok()?;
        Some((expected_root, installed, fit))
    })();
    let Some((root, mut installed, fit)) = install else {
        return NativeMaintenanceCaptureReadiness::unavailable();
    };
    let mut ports = HostPrerequisites {
        fit,
        remote: NativeMaintenanceDatabaseLedgerReader::new(&root),
    };
    let mut remote = NativeMaintenanceDatabaseLedgerReader::new(&root);
    let mut source = NativeMaintenanceCaptureRemoteAuthority::new(&mut remote);
    crate::native_maintenance_capture_readiness::inspect_native_maintenance_capture_readiness_for_generation(
        &mut installed, &mut ports, &mut source, &NativeWirePrimitives,
        crate::NativeMaintenanceCaptureReleaseGeneration::Native,
    )
}

struct HostPrerequisites {
    fit: NativeMaintenanceFitRuntimeReadiness<NativeMaintenanceFitRuntimeProcess>,
    remote: NativeMaintenanceDatabaseLedgerReader,
}

impl NativeMaintenanceCaptureReadinessPorts for HostPrerequisites {
    type Error = io::Error;

    /// Verify every actual retained fitter runtime prerequisite without changing host state.
    fn verify_fit_runtime(&mut self) -> io::Result<()> {
        self.fit.verify().map_err(failed)
    }

    /// Measure the actual physical layers and local archive population.
    fn read_local_archive_readiness(
        &mut self,
    ) -> io::Result<VerifiedNativeMaintenanceLocalArchiveReadiness> {
        Ok(inspect_native_maintenance_local_archive_readiness())
    }

    /// Fetch the actual fixed remote database ledger and verify the reviewed migration identity.
    fn read_verified_database_ledger(
        &mut self,
    ) -> io::Result<VerifiedNativeMaintenanceDatabaseLedger> {
        self.remote.fetch()
    }

    /// Read and verify the root-owned original epoch witness through the forced read account.
    fn read_verified_capture_epoch_witness(
        &mut self,
    ) -> io::Result<VerifiedNativeMaintenanceCaptureEpochWitness> {
        NativeMaintenanceCaptureRemoteAuthority::new(&mut self.remote)
            .read_verified_capture_epoch_witness()
            .map_err(failed)
    }

    /// Read and verify the exact root-owned zero snapshot through the same source account.
    fn read_verified_capture_epoch_snapshot(
        &mut self,
    ) -> io::Result<VerifiedNativeMaintenanceCaptureEpochSnapshot> {
        NativeMaintenanceCaptureRemoteAuthority::new(&mut self.remote)
            .read_verified_capture_epoch_snapshot()
            .map_err(failed)
    }
}

/// Retain only one path-free source category at the readiness projection boundary.
fn failed(_error: impl std::fmt::Debug) -> io::Error {
    io::Error::other("maintenance capture readiness source unavailable")
}
