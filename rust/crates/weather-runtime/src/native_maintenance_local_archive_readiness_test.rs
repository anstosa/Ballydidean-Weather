use super::*;
use crate::{NativeMaintenanceBackingCapacity, NativeMaintenanceBackingRootKind};

/// Build one explicitly measured fixture with both physical layers above the frozen floors.
fn capacity() -> NativeMaintenanceCapacityEvidence {
    NativeMaintenanceCapacityEvidence {
        backing: NativeMaintenanceBackingCapacity {
            allocated_bytes: 0,
            free_bytes: FREE_FLOOR + 100_000,
            free_inodes: FREE_INODE_FLOOR + 100,
            root_kind: NativeMaintenanceBackingRootKind::BackingC,
        },
        ext4_free_bytes: FREE_FLOOR + 100_000,
        ext4_free_inodes: FREE_INODE_FLOOR + 100,
        ext4_block_size: 4096,
        windows_total_bytes: 128 * 1024 * 1024 * 1024,
    }
}

/// Construct one archive status using the admitted exact prospective envelope fields.
fn envelope() -> NativeMaintenanceLocalArchiveEnvelope {
    NativeMaintenanceLocalArchiveEnvelope {
        allocated_bytes: 16_384,
        file_count: 4,
        initialization_allocated_bytes: 4096,
        initialization_atomic_allocated_bytes: 4096,
        initialization_atomic_inodes: 1,
        initialization_directories: 0,
        initialization_inodes: 1,
        initialization_lock_files: 0,
        incoming_count: 0,
        object_count: 2,
        task_inodes: 12,
    }
}

/// Preserve measured ready status and the source's first refusal order.
#[test]
fn local_archive_readiness_preserves_dual_floor_and_exact_status() {
    let mut measured = capacity();
    let status = project_local_archive_readiness(Some(&measured), Some(Ok(envelope())));
    assert!(status.ready);
    assert_eq!(status.status.unwrap().allocated_bytes, "16384");
    measured.backing.free_bytes = FREE_FLOOR - 1;
    let status = project_local_archive_readiness(Some(&measured), Some(Ok(envelope())));
    assert_eq!(status.reason, "physical_capacity_refused");
    assert!(status.status.is_some());
    let unavailable = project_local_archive_readiness(None, None);
    assert_eq!(unavailable.reason, "physical_capacity_unavailable");
    assert_eq!(unavailable.status, None);
}
