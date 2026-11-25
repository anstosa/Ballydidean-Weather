use super::*;
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    /// Allocate one bounded disposable state directory.
    fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "weather-archive-journal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    /// Remove only this test's isolated journal directory.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Compare the real fsynced journal transition bytes to the original source lease owner.
#[test]
fn native_archive_journal_matches_predecessor_lease_and_completion() {
    const NOW: &str = "2026-10-10T12:00:00.000Z";
    let root = Fixture::new();
    let (mut store, _) = NativeMaintenanceJournalStore::initialize_fixture(
        root.0.clone(),
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .unwrap();
    let key = format!("archive/revision-gap-page/{}", format_digest([3; 32]));
    let run_id = format!("archive-segment-{}", format_digest([3; 32]));
    let mut journal = NativeMaintenanceArchiveJournal::new(&mut store);
    let request = || NativeMaintenanceArchiveLeaseRequest {
        due_key: &key,
        run_id: &run_id,
        now: NOW,
        input_head_sha256: [1; 32],
    };
    journal.acquire(request()).unwrap();
    journal.acquire(request()).unwrap();
    journal.complete(&key, NOW, [2; 32]).unwrap();
    journal.complete(&key, NOW, [2; 32]).unwrap();
    journal.release(&key, &run_id, NOW).unwrap();
    assert_eq!(journal.due(&key).unwrap().output_sha256, Some([2; 32]));
    assert_eq!(journal.status().unwrap().active_lease, None);
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let expected=Command::new("node").args(["--input-type=module","-e",r#"
import {ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND,MaintenanceJournal} from "./scripts/research/adjustment_maintenance_state.mjs";
let bytes=Buffer.alloc(0);
const store={initialize:async()=>({rootKind:ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND}),withLock:async f=>f({readJournal:async()=>Buffer.from(bytes),append:async more=>{bytes=Buffer.concat([bytes,more]);},writeHeads:async()=>{}})};
const journal=new MaintenanceJournal(store); const dueKey=process.argv[1],runId=process.argv[2],now=process.argv[3];
await journal.initialize();
await journal.acquireLease({dueKey,runId,now,inputHeadSha256:"01".repeat(32),scope:"archive"});
await journal.acquireLease({dueKey,runId,now,inputHeadSha256:"01".repeat(32),scope:"archive"});
await journal.completeDue({dueKey,now,outputSha256:"02".repeat(32)});
await journal.completeDue({dueKey,now,outputSha256:"02".repeat(32)});
await journal.releaseLease({dueKey,runId,now,scope:"archive"});
process.stdout.write(bytes);
"#]).args([&key,&run_id,NOW]).current_dir(repo).output().unwrap();
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    assert_eq!(
        fs::read(root.0.join("journal.jsonl")).unwrap(),
        expected.stdout
    );
}
