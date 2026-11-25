use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

use super::*;

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ACTION_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const COMMIT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COMMIT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const COMMIT_C: &str = "cccccccccccccccccccccccccccccccccccccccc";
const COMMIT_D: &str = "dddddddddddddddddddddddddddddddddddddddd";
const DUE: &str = "release/wind/2026-11";
const RUN: &str = "release-wind-2026-11";
const NOW: &str = "2026-11-25T00:00:00.000Z";

struct Fixture(PathBuf);

impl Drop for Fixture {
    /// Remove only this process-owned bounded journal fixture.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Persist the exact fenced target, collision-adjusted compensation, and pair sequence.
#[test]
fn records_fenced_model_release_pair_idempotently() {
    let (mut store, fixture) = journal();
    let acquired = acquire_native_maintenance_release_lease(
        &mut store,
        NativeMaintenanceReleaseLeaseInput {
            due_key: DUE,
            input_head_sha256: HASH_A,
            now: NOW,
            run_id: RUN,
        },
    )
    .unwrap();
    let NativeMaintenanceReleaseLeaseAcquisition::Held(lease) = acquired else {
        panic!("release lease was not held")
    };
    assert_eq!(lease.fencing_token, "1");
    assert_eq!(lease.expires_at, "2026-11-25T08:00:00.000Z");
    assert_eq!(
        acquire_native_maintenance_release_lease(
            &mut store,
            NativeMaintenanceReleaseLeaseInput {
                due_key: DUE,
                input_head_sha256: HASH_A,
                now: NOW,
                run_id: RUN,
            },
        )
        .unwrap(),
        NativeMaintenanceReleaseLeaseAcquisition::Held(lease.clone())
    );

    let target = target_input("2026.11.25-1", "branch/target", "action/target");
    prepare_native_maintenance_model_release(&mut store, target).unwrap();
    prepare_native_maintenance_model_release(&mut store, target).unwrap();
    record_native_maintenance_model_release_tag_collision(
        &mut store,
        NativeMaintenanceModelReleaseTagCollisionInput {
            action_sha256: HASH_A,
            attempted_release_tag: "2026.11.25-1",
            collision_commit_sha: COMMIT_C,
            family: NativeMaintenanceCaptureFamily::Wind,
            fencing_token: "1",
            now: "2026-11-25T00:01:00.000Z",
            successor_release_tag: "2026.11.25-2",
        },
    )
    .unwrap();

    let compensation = compensation_input();
    prepare_native_maintenance_compensating_model_release(&mut store, compensation).unwrap();
    prepare_native_maintenance_compensating_model_release(&mut store, compensation).unwrap();
    record_native_maintenance_model_release_tag_collision(
        &mut store,
        NativeMaintenanceModelReleaseTagCollisionInput {
            action_sha256: ACTION_C,
            attempted_release_tag: "2026.11.25-3",
            collision_commit_sha: COMMIT_C,
            family: NativeMaintenanceCaptureFamily::Wind,
            fencing_token: "1",
            now: "2026-11-25T00:02:00.000Z",
            successor_release_tag: "2026.11.25-4",
        },
    )
    .unwrap();

    let pair = pair_input();
    let receipt = record_native_maintenance_model_release_pair(&mut store, pair).unwrap();
    record_native_maintenance_model_release_pair(&mut store, pair).unwrap();
    assert_eq!(receipt.kind, "model_release_pair_prepared");
    assert_eq!(
        text(&receipt.payload, "targetReleaseTag").as_deref(),
        Some("2026.11.25-2")
    );
    let records = store
        .read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)
        .unwrap();
    assert_eq!(records.len(), 7);
    assert_eq!(
        records
            .iter()
            .filter(|record| kind(record, "model_release_pair_prepared"))
            .count(),
        1
    );
    drop(fixture);
}

/// Refuse immutable identity reuse without appending a partial preparation.
#[test]
fn rejects_model_release_identity_reuse() {
    let (mut store, fixture) = journal();
    acquire_native_maintenance_release_lease(
        &mut store,
        NativeMaintenanceReleaseLeaseInput {
            due_key: DUE,
            input_head_sha256: HASH_A,
            now: NOW,
            run_id: RUN,
        },
    )
    .unwrap();
    prepare_native_maintenance_model_release(
        &mut store,
        target_input("2026.11.25-1", "branch/shared", "action/target"),
    )
    .unwrap();
    let reused = NativeMaintenanceModelReleasePrepareInput {
        action_sha256: HASH_B,
        action_tag: "action/other",
        branch: "branch/shared",
        commit_sha: COMMIT_D,
        expected_source_commit: COMMIT_B,
        family: NativeMaintenanceCaptureFamily::Wind,
        fencing_token: "1",
        now: "2026-11-25T00:01:00.000Z",
        package_root_sha256: HASH_B,
        release_due_key: DUE,
        release_run_id: RUN,
        release_tag: "2026.11.25-9",
    };
    assert_eq!(
        prepare_native_maintenance_model_release(&mut store, reused),
        Err(NativeMaintenanceModelReleaseJournalError::ModelReleaseIdentityReused)
    );
    let records = store
        .read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)
        .unwrap();
    assert_eq!(records.len(), 3);
    drop(fixture);
}

/// Release one exact live fence and allocate the next monotonic fence on reacquisition.
#[test]
fn release_lease_retains_monotonic_fence_history() {
    let (mut store, fixture) = journal();
    acquire_native_maintenance_release_lease(
        &mut store,
        NativeMaintenanceReleaseLeaseInput {
            due_key: DUE,
            input_head_sha256: HASH_A,
            now: NOW,
            run_id: RUN,
        },
    )
    .unwrap();
    release_native_maintenance_release_lease(
        &mut store,
        NativeMaintenanceReleaseLeaseReleaseInput {
            due_key: DUE,
            now: "2026-11-25T00:01:00.000Z",
            run_id: RUN,
        },
    )
    .unwrap();
    let acquired = acquire_native_maintenance_release_lease(
        &mut store,
        NativeMaintenanceReleaseLeaseInput {
            due_key: DUE,
            input_head_sha256: HASH_A,
            now: "2026-11-25T00:02:00.000Z",
            run_id: "release-wind-2026-11-retry",
        },
    )
    .unwrap();
    let NativeMaintenanceReleaseLeaseAcquisition::Held(lease) = acquired else {
        panic!("release lease was not reacquired")
    };
    assert_eq!(lease.fencing_token, "2");
    drop(fixture);
}

/// Initialize one owner-private disposable native journal.
fn journal() -> (NativeMaintenanceJournalStore, Fixture) {
    let root = std::env::temp_dir().join(format!(
        "weather-native-model-release-journal-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let fixture = Fixture(root.clone());
    let (store, _) = NativeMaintenanceJournalStore::initialize_fixture(
        root,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .unwrap();
    (store, fixture)
}

/// Build one exact target preparation fixture.
fn target_input<'a>(
    release_tag: &'a str,
    branch: &'a str,
    action_tag: &'a str,
) -> NativeMaintenanceModelReleasePrepareInput<'a> {
    NativeMaintenanceModelReleasePrepareInput {
        action_sha256: HASH_A,
        action_tag,
        branch,
        commit_sha: COMMIT_A,
        expected_source_commit: COMMIT_B,
        family: NativeMaintenanceCaptureFamily::Wind,
        fencing_token: "1",
        now: "2026-11-25T00:01:00.000Z",
        package_root_sha256: HASH_B,
        release_due_key: DUE,
        release_run_id: RUN,
        release_tag,
    }
}

/// Build one exact compensating preparation fixture.
fn compensation_input() -> NativeMaintenanceCompensatingModelReleasePrepareInput<'static> {
    NativeMaintenanceCompensatingModelReleasePrepareInput {
        action_sha256: ACTION_C,
        action_tag: "action/compensation",
        branch: "branch/compensation",
        commit_sha: COMMIT_D,
        expected_source_commit: COMMIT_A,
        family: NativeMaintenanceCaptureFamily::Wind,
        fencing_token: "1",
        now: "2026-11-25T00:02:00.000Z",
        package_root_sha256: HASH_B,
        predecessor_action_sha256: HASH_A,
        release_due_key: DUE,
        release_run_id: RUN,
        release_tag: "2026.11.25-3",
    }
}

/// Build one exact collision-adjusted release pair fixture.
fn pair_input() -> NativeMaintenanceModelReleasePairInput<'static> {
    NativeMaintenanceModelReleasePairInput {
        compensation_action_sha256: ACTION_C,
        compensation_commit_sha: COMMIT_D,
        compensation_release_tag: "2026.11.25-4",
        family: NativeMaintenanceCaptureFamily::Wind,
        fencing_token: "1",
        now: "2026-11-25T00:03:00.000Z",
        target_action_sha256: HASH_A,
        target_commit_sha: COMMIT_A,
        target_release_tag: "2026.11.25-2",
    }
}
