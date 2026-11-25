use std::collections::BTreeSet;
use std::fs;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

use weather_contracts::JsonValue;

use super::*;
use crate::{NativeBackendRuntime, NativeMaintenanceOwnedJournalAddition, NativeRegistrationSlot};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// One disposable owner-private journal fixture.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique private directory.
    fn create() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("fixture clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "weather-due-planner-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create fixture root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("protect fixture root");
        Self(root)
    }
}

impl Drop for FixtureRoot {
    /// Remove only the bounded fixture root.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct MemoryDueReader {
    completed: BTreeSet<String>,
    registered: BTreeSet<String>,
}

impl NativeMaintenanceDueReadPort for MemoryDueReader {
    type Error = ();

    /// Preserve requested order while projecting fixture completion state.
    fn inspect_due_keys<'a>(
        &'a mut self,
        due_keys: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<NativeMaintenanceDueState>, Self::Error>> + 'a>>
    {
        Box::pin(async move {
            Ok(due_keys
                .iter()
                .map(|key| {
                    // project completion before registration and absence
                    let status = if self.completed.contains(key) {
                        NativeMaintenanceDueStatus::Complete
                    } else if self.registered.contains(key) {
                        NativeMaintenanceDueStatus::Registered
                    } else {
                        NativeMaintenanceDueStatus::Absent
                    };
                    NativeMaintenanceDueState {
                        due_key: key.clone(),
                        input_head_sha256: (status != NativeMaintenanceDueStatus::Absent)
                            .then(|| HASH_A.to_owned()),
                        output_sha256: (status == NativeMaintenanceDueStatus::Complete)
                            .then(|| HASH_B.to_owned()),
                        status,
                    }
                })
                .collect())
        })
    }
}

#[test]
fn native_daily_planner_selects_seven_dates_and_one_old_month_per_family() {
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let mut reader = MemoryDueReader {
            completed: [
                "daily/2026-10-08",
                "daily/2026-10-09",
                "monthly/temperature/2026-10",
                "monthly/wind/2026-10",
                "monthly/rain/2026-10",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            registered: ["daily/2026-10-10".to_owned()].into_iter().collect(),
        };
        let dues = plan_native_adjustment_maintenance_dues(
            &mut reader,
            NativeMaintenanceDueMode::Daily,
            "2026-12-10T12:00:00.000Z",
            &schedule("2026-12-10T12:00:00.000Z"),
        )
        .await
        .expect("plan daily dues");
        let keys = dues
            .iter()
            .map(|due| due.due_key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "daily/2026-10-10",
                "daily/2026-10-11",
                "daily/2026-10-12",
                "daily/2026-10-13",
                "daily/2026-10-14",
                "daily/2026-10-15",
                "daily/2026-10-16",
                "monthly/temperature/2026-11",
                "monthly/wind/2026-11",
                "monthly/rain/2026-11",
            ]
        );
        assert_eq!(dues[0].original_cutoff_at, "2026-10-11T07:00:00.000Z");
        assert_eq!(dues[7].scope, "monthly/temperature");
    });
}

#[test]
fn native_daily_planner_registers_rain_reference_only_in_pre_month_window() {
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let mut reader = MemoryDueReader::default();
        let dues = plan_native_adjustment_maintenance_dues(
            &mut reader,
            NativeMaintenanceDueMode::Daily,
            "2026-10-29T12:00:00.000Z",
            &schedule("2026-10-29T12:00:00.000Z"),
        )
        .await
        .expect("plan rain reference");
        let control = dues
            .iter()
            .find(|due| due.scope == "control-reference/rain")
            .expect("rain reference due");
        assert_eq!(control.due_key, "control-reference/rain/2026-11");
        assert_eq!(control.original_cutoff_at, "2026-10-25T00:00:00.000Z");

        let mut completed = MemoryDueReader {
            completed: [control.due_key.clone()].into_iter().collect(),
            registered: BTreeSet::new(),
        };
        let replay = plan_native_adjustment_maintenance_dues(
            &mut completed,
            NativeMaintenanceDueMode::Daily,
            "2026-10-29T12:00:00.000Z",
            &schedule("2026-10-29T12:00:00.000Z"),
        )
        .await
        .expect("plan completed rain reference");
        assert!(
            replay
                .iter()
                .all(|due| due.scope != "control-reference/rain")
        );
    });
}

#[test]
fn native_monthly_planner_waits_for_second_local_day() {
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let mut reader = MemoryDueReader::default();
        let early = plan_native_adjustment_maintenance_dues(
            &mut reader,
            NativeMaintenanceDueMode::Monthly,
            "2026-12-01T12:00:00.000Z",
            &schedule("2026-12-01T12:00:00.000Z"),
        )
        .await
        .expect("plan first day");
        assert!(early.is_empty());
        let dues = plan_native_adjustment_maintenance_dues(
            &mut reader,
            NativeMaintenanceDueMode::Monthly,
            "2026-12-02T12:00:00.000Z",
            &schedule("2026-12-02T12:00:00.000Z"),
        )
        .await
        .expect("plan second day");
        assert_eq!(
            dues.iter()
                .map(|due| due.due_key.as_str())
                .collect::<Vec<_>>(),
            [
                "monthly/temperature/2026-12",
                "monthly/wind/2026-12",
                "monthly/rain/2026-12",
            ]
        );
    });
}

#[test]
fn native_due_reader_projects_registered_and_completed_records() {
    let fixture = FixtureRoot::create();
    let mut codec = NativeWirePrimitives::new();
    let time = NativeWirePrimitives::new();
    let (mut store, _) =
        NativeMaintenanceJournalStore::initialize_fixture(fixture.0.clone(), &mut codec, &time)
            .expect("initialize due journal fixture");
    store
        .mutate_checked(&mut codec, &time, |_| {
            Ok::<_, NativeMaintenanceDueJournalError>(vec![
                addition(
                    "due_registered",
                    "due/daily/2026-10-10",
                    Some("daily/2026-10-10"),
                    object(vec![("inputHeadSha256", string(HASH_A))]),
                ),
                addition(
                    "due_completed",
                    "due/daily/2026-10-10/complete",
                    Some("daily/2026-10-10"),
                    object(vec![("outputSha256", string(HASH_B))]),
                ),
                addition(
                    "due_registered",
                    "due/monthly/rain/2026-10",
                    Some("monthly/rain/2026-10"),
                    object(vec![("inputHeadSha256", string(HASH_A))]),
                ),
            ])
        })
        .expect("append due states");
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let mut reader = NativeMaintenanceJournalDueReader::new(&mut store);
        let states = reader
            .inspect_due_keys(&[
                "daily/2026-10-10".to_owned(),
                "monthly/rain/2026-10".to_owned(),
                "daily/2026-10-11".to_owned(),
            ])
            .await
            .expect("inspect actual journal");
        assert_eq!(states[0].status, NativeMaintenanceDueStatus::Complete);
        assert_eq!(states[1].status, NativeMaintenanceDueStatus::Registered);
        assert_eq!(states[2].status, NativeMaintenanceDueStatus::Absent);
    });
}

/// Build one verified free registration slot.
fn slot(family: NativeMaintenanceCaptureFamily) -> NativeRegistrationSlot {
    NativeRegistrationSlot {
        contract_version: SLOT_VERSION.to_owned(),
        epoch_witness_sha256: HASH_A.to_owned(),
        family,
        horizon_end_at: "2027-10-08T07:00:00.000Z".to_owned(),
        registration_sha256: None,
        schedule_contract_sha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256.to_owned(),
        state: NativeRegistrationSlotState::Free,
        terminal_at: None,
    }
}

/// Build one source-shaped authenticated schedule fixture.
fn schedule(snapshot_at: &str) -> NativeRegistrationScheduleStatus {
    NativeRegistrationScheduleStatus {
        contract_version: SCHEDULE_VERSION,
        epoch_at: "2026-10-07T08:00:00.000Z".to_owned(),
        epoch_witness_sha256: HASH_A.to_owned(),
        horizon_end_at: "2027-10-08T07:00:00.000Z".to_owned(),
        schedule_contract_sha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256.to_owned(),
        slots: [
            slot(NativeMaintenanceCaptureFamily::Temperature),
            slot(NativeMaintenanceCaptureFamily::Wind),
            slot(NativeMaintenanceCaptureFamily::Rain),
        ],
        snapshot_at: snapshot_at.to_owned(),
    }
}

/// Build one exact owned journal addition.
fn addition(
    kind: &str,
    record_key: &str,
    due_key: Option<&str>,
    payload: JsonValue,
) -> NativeMaintenanceOwnedJournalAddition {
    NativeMaintenanceOwnedJournalAddition {
        due_key: due_key.map(str::to_owned),
        kind: kind.to_owned(),
        payload,
        record_key: record_key.to_owned(),
        recorded_at: "2026-10-10T12:00:00.000Z".to_owned(),
    }
}

/// Build one fixture object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one fixture string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
