use super::*;
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    /// Remove only this process-owned bounded journal fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Compare actual native controller lease transactions with the source journal byte for byte.
#[test]
fn native_maintenance_controller_lease_matches_source_daily_and_monthly_reconciliation() {
    let root = std::env::temp_dir().join(format!(
        "weather-native-controller-lease-{}",
        std::process::id()
    ));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let _cleanup = Fixture(root.clone());
    let (mut store, _) = NativeMaintenanceJournalStore::initialize_fixture(
        root.clone(),
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .unwrap();
    let mut journal = NativeMaintenanceControllerLeaseJournal::new(&mut store);
    let daily = "daily/2026-10-08";
    let monthly = "monthly/rain/2026-10";
    let run1 = NativeMaintenanceControllerLeaseJournal::run_id(daily, [1; 32]).unwrap();
    let run2 = NativeMaintenanceControllerLeaseJournal::run_id(daily, [2; 32]).unwrap();
    let run3 = NativeMaintenanceControllerLeaseJournal::run_id(monthly, [3; 32]).unwrap();
    let initial = "2026-10-10T00:00:00.000Z";
    let later = "2026-10-10T04:00:00.000Z";
    journal
        .acquire("daily", daily, &run1, initial, [1; 32])
        .unwrap();
    journal
        .acquire("daily", daily, &run1, initial, [1; 32])
        .unwrap();
    journal.reconcile_expired("daily", later).unwrap();
    journal
        .acquire("daily", daily, &run2, later, [1; 32])
        .unwrap();
    journal.complete(daily, later, [4; 32]).unwrap();
    journal.release("daily", daily, &run2, later).unwrap();
    journal
        .acquire("monthly/rain", monthly, &run3, later, [3; 32])
        .unwrap();
    journal
        .release("monthly/rain", monthly, &run3, later)
        .unwrap();
    let expected=Command::new("node").current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")).args(["--input-type=module","--eval",r#"
import {ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND,MaintenanceJournal} from "./scripts/research/adjustment_maintenance_state.mjs";
import {adjustmentSha256} from "./scripts/research/adjustment_plaintext_archive.mjs";
let bytes=Buffer.alloc(0);
const store={initialize:async()=>({rootKind:ADJUSTMENT_MAINTENANCE_STATE_ROOT_KIND}),withLock:async f=>f({readJournal:async()=>Buffer.from(bytes),append:async more=>{bytes=Buffer.concat([bytes,more]);},writeHeads:async()=>{}})};
const journal=new MaintenanceJournal(store);
await journal.initialize();
const daily="daily/2026-10-08",monthly="monthly/rain/2026-10",initial="2026-10-10T00:00:00.000Z",later="2026-10-10T04:00:00.000Z";
const run=(due,hex)=>"maintenance-"+adjustmentSha256(`${due}\n${hex.repeat(32)}\n`).slice(0,32);
const run1=run(daily,"01"),run2=run(daily,"02"),run3=run(monthly,"03");
const first={scope:"daily",dueKey:daily,runId:run1,now:initial,inputHeadSha256:"01".repeat(32)};
await journal.acquireLease(first);await journal.acquireLease(first);
await journal.reconcileExpiredLease({scope:"daily",dueKey:daily,runId:run1,now:later,resolution:"resume",immutableOutputSha256:null,remoteStateSha256:null});
await journal.acquireLease({...first,runId:run2,now:later});
await journal.completeDue({dueKey:daily,now:later,outputSha256:"04".repeat(32)});
await journal.releaseLease({scope:"daily",dueKey:daily,runId:run2,now:later});
await journal.acquireLease({scope:"monthly/rain",dueKey:monthly,runId:run3,now:later,inputHeadSha256:"03".repeat(32)});
await journal.releaseLease({scope:"monthly/rain",dueKey:monthly,runId:run3,now:later});
process.stdout.write(bytes);
"#]).output().unwrap();
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    assert_eq!(
        fs::read(root.join("journal.jsonl")).unwrap(),
        expected.stdout
    );
    assert_eq!(
        journal.state(daily).unwrap().due.output_sha256,
        Some([4; 32])
    );
    assert_eq!(journal.state(monthly).unwrap().due.output_sha256, None);
}
