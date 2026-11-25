use super::*;
use std::{
    os::unix::fs::symlink,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);

impl Fixture {
    /// Create a disposable two-bucket native allocation fixture.
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "weather-census-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
}

impl Drop for Fixture {
    /// Remove only the isolated temporary fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Compare the actual source census over hardlinked retained data, directories and symlinks.
#[test]
fn native_backing_census_matches_predecessor_allocation_and_entry_count() {
    let fixture = Fixture::new();
    let a = fixture.0.join("a");
    let b = fixture.0.join("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    fs::write(a.join("member"), [1u8; 8192]).unwrap();
    fs::hard_link(a.join("member"), b.join("same-inode")).unwrap();
    symlink(&a, b.join("pointer")).unwrap();
    let absent = fixture.0.join("absent");
    let roots = [a, b, absent];
    let device = fs::metadata(&fixture.0).unwrap().dev();
    let actual = measure_native_maintenance_backing_census(&roots, device).unwrap();
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../scripts/research/adjustment_archive_job.mjs")
        .canonicalize()
        .unwrap();
    let output = Command::new("node").args(["--input-type=module", "-e",
        "import {pathToFileURL} from 'node:url'; const {measureImmutableLegacyCensus}=await import(pathToFileURL(process.argv[2])); const result=await measureImmutableLegacyCensus(process.argv.slice(4), BigInt(process.argv[3])); process.stdout.write(result.allocatedBytes.toString()+'|'+result.entryCount);"
    ]).arg("weather-census-oracle")
        .arg(oracle).arg(device.to_string()).args(&roots).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}|{}", actual.allocated_bytes, actual.entry_count)
    );
    assert_eq!(actual.entry_count, 5);
}
