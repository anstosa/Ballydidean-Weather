//! Actual read-only local archive capacity and finite-count readiness projection.

use std::path::Path;

use crate::{
    NativeMaintenanceCapacityEvidence, NativeMaintenanceCaptureArchiveStatus,
    NativeMaintenanceLocalArchiveEnvelope, NativeMaintenanceLocalArchiveEnvelopeError,
    VerifiedNativeMaintenanceLocalArchiveReadiness,
    inspect_native_maintenance_local_archive_envelope,
    measure_native_maintenance_capacity_evidence,
};

const VERSION: &str = "adjustment-archive-local-readiness/v1";
const FREE_FLOOR: u64 = 16 * 1024 * 1024 * 1024;
const TOTAL_STORAGE_CEILING: u64 = 64 * 1024 * 1024 * 1024;
const ARCHIVE_CEILING: u64 = 18 * 1024 * 1024 * 1024;
const FREE_INODE_FLOOR: u64 = 32_768;
const COMMITTED_OBJECT_CEILING: u64 = 16_744;
const ARCHIVE_FILE_CEILING: u64 = 16_747;
const TASK_INODE_CEILING: u64 = 17_899;

/// Sample the actual fixed physical layers and local archive without initializing any state.
pub fn inspect_native_maintenance_local_archive_readiness()
-> VerifiedNativeMaintenanceLocalArchiveReadiness {
    let capacity = measure_native_maintenance_capacity_evidence().ok();
    let envelope = capacity.as_ref().map(|evidence| {
        inspect_native_maintenance_local_archive_envelope(Path::new("/home/ubuntu"), evidence)
    });
    project_local_archive_readiness(capacity.as_ref(), envelope)
}

/// Retain the predecessor's first physical or finite-count refusal and optional real status.
fn project_local_archive_readiness(
    capacity: Option<&NativeMaintenanceCapacityEvidence>,
    envelope: Option<
        Result<NativeMaintenanceLocalArchiveEnvelope, NativeMaintenanceLocalArchiveEnvelopeError>,
    >,
) -> VerifiedNativeMaintenanceLocalArchiveReadiness {
    // preserve physical capacity precedence independently of archive layout
    let physical_reason = match capacity {
        None => Some("physical_capacity_unavailable"),
        Some(evidence)
            if evidence.ext4_free_bytes < FREE_FLOOR
                || evidence.backing.free_bytes < FREE_FLOOR
                || evidence.ext4_free_inodes < FREE_INODE_FLOOR
                || evidence.backing.allocated_bytes > TOTAL_STORAGE_CEILING =>
        {
            Some("physical_capacity_refused")
        }
        Some(_) => None,
    };
    let (archive_reason, status) = match envelope {
        Some(Ok(archive)) => {
            // apply every source archive and task ceiling to the actual retained population
            let reason = (archive.allocated_bytes > ARCHIVE_CEILING
                || archive.object_count > COMMITTED_OBJECT_CEILING
                || archive.file_count > ARCHIVE_FILE_CEILING
                || archive.task_inodes > TASK_INODE_CEILING
                || archive.incoming_count > 1)
                .then_some("archive_envelope_refused");
            (
                reason,
                Some(NativeMaintenanceCaptureArchiveStatus {
                    allocated_bytes: archive.allocated_bytes.to_string(),
                    file_count: archive.file_count,
                    incoming_count: archive.incoming_count,
                    object_count: archive.object_count,
                    task_inodes: archive.task_inodes.to_string(),
                }),
            )
        }
        Some(Err(NativeMaintenanceLocalArchiveEnvelopeError::Capacity)) => {
            (Some("physical_capacity_refused"), None)
        }
        Some(Err(_)) | None => (Some("archive_envelope_unavailable"), None),
    };
    let reason = physical_reason.or(archive_reason).unwrap_or("ready");
    VerifiedNativeMaintenanceLocalArchiveReadiness {
        contract_version: VERSION.to_owned(),
        ready: reason == "ready",
        reason: reason.to_owned(),
        status,
    }
}

#[cfg(test)]
#[path = "native_maintenance_local_archive_readiness_test.rs"]
mod tests;
