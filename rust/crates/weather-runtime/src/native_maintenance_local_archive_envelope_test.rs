use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::native_maintenance_backing_capacity::{
    NativeMaintenanceBackingCapacity, NativeMaintenanceBackingRootKind,
    NativeMaintenanceCapacityEvidence,
};
use crate::native_maintenance_local_archive_envelope::{
    NativeMaintenanceLocalArchiveEnvelope, inspect_native_maintenance_local_archive_envelope,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

const NODE_FIXTURE: &str = r#"
import { join } from "node:path";
import { adjustmentArchiveJobTestOnly } from "./scripts/research/adjustment_archive_job.mjs";

const gib = 1024n ** 3n;
const capacity = {
  backing: {
    allocatedBytes: 1n * gib,
    freeBytes: 100n * gib,
    freeInodes: 1_000_000n,
    rootKind: "backing_c",
  },
  ext4FreeBytes: 100n * gib,
  ext4FreeInodes: 1_000_000n,
  ext4BlockSize: 4096n,
  windowsTotalBytes: 200n * gib,
};
const homeRoot = process.env.WEATHER_ARCHIVE_FIXTURE_HOME;
const archiveRoot = join(
  homeRoot,
  ".weather",
  "adjustment-maintenance",
  "v2",
  "archive-primary",
);
const envelope = await adjustmentArchiveJobTestOnly.inspectArchiveEnvelope({
  archiveRoot,
  capacity,
  homeRoot,
});
process.stdout.write([
  envelope.allocatedBytes,
  envelope.fileCount,
  envelope.initializationAllocatedBytes,
  envelope.initializationAtomicAllocatedBytes,
  envelope.initializationAtomicInodes,
  envelope.initializationDirectories,
  envelope.initializationInodes,
  envelope.initializationLockFiles,
  envelope.incomingCount,
  envelope.objectCount,
  envelope.taskInodes,
].map(String).join("\n"));
"#;

/// Remove one bounded disposable archive home.
struct FixtureHome(PathBuf);

impl FixtureHome {
    /// Create one unique literal private home anchor.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let home = std::env::temp_dir().join(format!(
            "weather-native-archive-envelope-{}-{nonce}",
            std::process::id()
        ));
        create_private_directory(&home);
        Self(home)
    }

    /// Populate one complete valid archive, pointer history, and state genesis.
    fn populate(&self) {
        let weather = self.0.join(".weather");
        let maintenance = weather.join("adjustment-maintenance");
        let task = maintenance.join("v2");
        let archive = task.join("archive-primary");
        let objects = archive.join("objects");
        let state = task.join("state");
        // create the exact fixed private directory chain
        for path in [&weather, &maintenance, &task, &archive, &objects, &state] {
            create_private_directory(path);
        }
        write_private_file(&objects.join(format!("sha256-{HASH_A}.obj")), b"current");
        write_private_file(&objects.join(format!("sha256-{HASH_B}.obj")), b"previous");
        write_private_file(
            &archive.join("head.current"),
            format!("{HASH_A}\n").as_bytes(),
        );
        write_private_file(
            &archive.join("head.previous"),
            format!("{HASH_B}\n").as_bytes(),
        );
        write_private_file(&state.join("archive-job.lock"), b"");
        write_private_file(&state.join("head.current"), b"state-head\n");
    }
}

impl Drop for FixtureHome {
    /// Remove only the process-owned disposable archive home.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Create one exact owner-private fixture directory.
fn create_private_directory(path: &Path) {
    fs::create_dir(path).expect("create fixture directory");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .expect("protect fixture directory");
}

/// Create one exact owner-private single-link fixture file.
fn write_private_file(path: &Path, bytes: &[u8]) {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = options.open(path).expect("create fixture file");
    file.write_all(bytes).expect("write fixture file");
    file.sync_all().expect("sync fixture file");
    drop(file);
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("protect fixture file");
}

/// Return the generous dual-layer capacity used by the source fixture.
fn capacity() -> NativeMaintenanceCapacityEvidence {
    NativeMaintenanceCapacityEvidence {
        backing: NativeMaintenanceBackingCapacity {
            allocated_bytes: 1_024_u64.pow(3),
            free_bytes: 100 * 1_024_u64.pow(3),
            free_inodes: 1_000_000,
            root_kind: NativeMaintenanceBackingRootKind::BackingC,
        },
        ext4_free_bytes: 100 * 1_024_u64.pow(3),
        ext4_free_inodes: 1_000_000,
        ext4_block_size: 4096,
        windows_total_bytes: 200 * 1_024_u64.pow(3),
    }
}

/// Inspect one fixture through the actual predecessor implementation.
fn inspect_with_node(home: &Path) -> NativeMaintenanceLocalArchiveEnvelope {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_FIXTURE])
        .current_dir(repository)
        .env("WEATHER_ARCHIVE_FIXTURE_HOME", home)
        .output()
        .expect("run predecessor archive inspection");
    // surface only the predecessor's bounded fixture diagnostic
    if !output.status.success() {
        panic!(
            "predecessor archive fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let values = String::from_utf8(output.stdout)
        .expect("source envelope output")
        .lines()
        .map(|value| value.parse::<u64>().expect("source envelope number"))
        .collect::<Vec<_>>();
    let [
        allocated_bytes,
        file_count,
        initialization_allocated_bytes,
        initialization_atomic_allocated_bytes,
        initialization_atomic_inodes,
        initialization_directories,
        initialization_inodes,
        initialization_lock_files,
        incoming_count,
        object_count,
        task_inodes,
    ] = values.as_slice()
    else {
        panic!("source envelope field count differs");
    };
    NativeMaintenanceLocalArchiveEnvelope {
        allocated_bytes: *allocated_bytes,
        file_count: *file_count,
        initialization_allocated_bytes: *initialization_allocated_bytes,
        initialization_atomic_allocated_bytes: *initialization_atomic_allocated_bytes,
        initialization_atomic_inodes: *initialization_atomic_inodes,
        initialization_directories: *initialization_directories,
        initialization_inodes: *initialization_inodes,
        initialization_lock_files: *initialization_lock_files,
        incoming_count: *incoming_count,
        object_count: *object_count,
        task_inodes: *task_inodes,
    }
}

/// Match complete history and prospective empty genesis against the source owner.
#[test]
fn native_archive_envelope_matches_source_for_existing_and_genesis_layouts() {
    let existing = FixtureHome::create();
    existing.populate();
    let expected = inspect_with_node(&existing.0);
    let actual = inspect_native_maintenance_local_archive_envelope(&existing.0, &capacity())
        .expect("inspect existing archive");
    assert_eq!(actual, expected);
    assert_eq!(actual.object_count, 2);
    assert_eq!(actual.file_count, 4);
    assert_eq!(actual.initialization_lock_files, 1);

    let genesis = FixtureHome::create();
    let expected = inspect_with_node(&genesis.0);
    let actual = inspect_native_maintenance_local_archive_envelope(&genesis.0, &capacity())
        .expect("inspect prospective genesis");
    assert_eq!(actual, expected);
    assert_eq!(actual.initialization_directories, 6);
    assert_eq!(actual.initialization_lock_files, 2);
    assert_eq!(actual.initialization_inodes, 9);
}
