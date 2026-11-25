use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use weather_contracts::JsonValue;

use crate::native_maintenance_registration_lifecycle_remote::{
    NativeMaintenanceRegistrationLifecycleRemote, NativeRegistrationLifecycleRemoteOperation,
    NativeRegistrationLifecycleRemoteOperationPort, NativeRegistrationLifecycleWireError,
    NativeRegistrationSlotState, decode_registration_lifecycle_status,
    registration_schedule_from_lifecycle,
};
use crate::{
    ADJUSTMENT_ROLLING_SCHEDULE_SHA256, MaintenanceCanonicalJson,
    NativeAdjustmentRollingScheduleBootstrapInput, NativeBackendRuntime, NativeWirePrimitives,
    build_adjustment_rolling_schedule_bootstrap,
};

const EPOCH: &str = "2026-10-09T04:10:45.000Z";
const HORIZON: &str = "2029-08-27T07:00:00.000Z";
const SNAPSHOT: &str = "2026-10-10T04:10:45.000Z";
const TERMINAL: &str = "2027-10-10T07:00:00.000Z";

/// Decode a nonempty three-family lifecycle and preserve its schedule slots.
#[test]
fn lifecycle_wire_validates_active_predecessor_and_schedule_projection() {
    let value = lifecycle_value(None);
    let bytes = canonical(&value);
    let lifecycle = decode_registration_lifecycle_status(
        &mut NativeWirePrimitives,
        &bytes,
        &NativeWirePrimitives,
    )
    .expect("decode lifecycle");
    assert_eq!(
        lifecycle.entries[0].slot.state,
        NativeRegistrationSlotState::BusyV3,
    );
    assert_eq!(
        lifecycle.entries[1].slot.state,
        NativeRegistrationSlotState::BusyV2Legacy,
    );
    assert_eq!(
        lifecycle.entries[2].slot.state,
        NativeRegistrationSlotState::Free,
    );
    let schedule = registration_schedule_from_lifecycle(
        &lifecycle,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .expect("project schedule");
    assert_eq!(
        schedule.contract_version,
        "adjustment-registration-schedule-status/v3"
    );
    assert_eq!(schedule.slots[0], lifecycle.entries[0].slot);
    assert_eq!(schedule.slots[2], lifecycle.entries[2].slot);
}

/// Refuse a busy active row whose latest database identity differs.
#[test]
fn lifecycle_wire_refuses_active_latest_registration_drift() {
    let bytes = canonical(&lifecycle_value(Some(hash('9'))));
    assert_eq!(
        decode_registration_lifecycle_status(
            &mut NativeWirePrimitives,
            &bytes,
            &NativeWirePrimitives,
        ),
        Err(NativeRegistrationLifecycleWireError::Binding),
    );
}

/// Submit exact bootstrap bytes and admit an idempotent false acknowledgement.
#[test]
fn remote_owner_preserves_closed_initialize_operation_and_ack() {
    let witness = hash('e');
    let bootstrap = build_adjustment_rolling_schedule_bootstrap(
        NativeAdjustmentRollingScheduleBootstrapInput {
            epoch_at: EPOCH,
            epoch_witness_sha256: &witness,
        },
    )
    .expect("build bootstrap");
    assert_eq!(bootstrap.horizon_end_at, HORIZON);
    let ack = object(vec![
        member("bootstrapSha256", string(&bootstrap.bootstrap_sha256)),
        member("initialized", JsonValue::Boolean(false)),
        member(
            "scheduleContractSha256",
            string(ADJUSTMENT_ROLLING_SCHEDULE_SHA256),
        ),
    ]);
    let port = FixturePort::new(vec![canonical(&ack)]);
    let operations = Arc::clone(&port.operations);
    let mut owner = NativeMaintenanceRegistrationLifecycleRemote::new(
        port,
        NativeWirePrimitives,
        NativeWirePrimitives,
    );
    let runtime = NativeBackendRuntime::new().expect("start fixture runtime");
    let response = runtime
        .block_on(owner.initialize_schedule(&bootstrap))
        .expect("initialize schedule");
    assert!(!response.initialized);
    assert_eq!(response.bootstrap_sha256, bootstrap.bootstrap_sha256);
    let operations = operations.lock().expect("read operations");
    assert_eq!(operations.len(), 1);
    assert_eq!(
        operations[0].command(),
        "adjustment-registration-schedule-initialize-v3",
    );
    assert_eq!(
        operations[0].arguments(),
        [bootstrap.bootstrap_sha256.as_str()]
    );
    let body = operations[0].body().expect("bootstrap body");
    let parsed = NativeWirePrimitives
        .parse_canonical_object(body)
        .expect("canonical bootstrap body");
    assert_eq!(
        parsed,
        bootstrap_value(
            &bootstrap.bootstrap_sha256,
            &bootstrap.epoch_witness_sha256,
            &bootstrap.first_complete_local_date,
            &bootstrap.horizon_end_at,
        ),
    );
}

/// Fetch lifecycle status through the only closed read operation.
#[test]
fn remote_owner_uses_fixed_lifecycle_status_operation() {
    let port = FixturePort::new(vec![canonical(&lifecycle_value(None))]);
    let operations = Arc::clone(&port.operations);
    let mut owner = NativeMaintenanceRegistrationLifecycleRemote::new(
        port,
        NativeWirePrimitives,
        NativeWirePrimitives,
    );
    let runtime = NativeBackendRuntime::new().expect("start fixture runtime");
    let lifecycle = runtime
        .block_on(owner.read_lifecycle())
        .expect("read lifecycle");
    assert_eq!(lifecycle.snapshot_at, SNAPSHOT);
    let operations = operations.lock().expect("read operations");
    assert_eq!(
        operations.as_slice(),
        [NativeRegistrationLifecycleRemoteOperation::LifecycleStatusV4]
    );
    assert!(operations[0].arguments().is_empty());
    assert!(operations[0].body().is_none());
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FixturePortError;

impl Display for FixturePortError {
    /// Render one path-free fixture error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("fixture port failed")
    }
}

struct FixturePort {
    operations: Arc<Mutex<Vec<NativeRegistrationLifecycleRemoteOperation>>>,
    responses: VecDeque<Vec<u8>>,
}

impl FixturePort {
    /// Bind one ordered in-memory response sequence.
    fn new(responses: Vec<Vec<u8>>) -> Self {
        Self {
            operations: Arc::new(Mutex::new(Vec::new())),
            responses: responses.into(),
        }
    }
}

impl NativeRegistrationLifecycleRemoteOperationPort for FixturePort {
    type Error = FixturePortError;

    /// Record the closed operation and return its fixture response.
    fn execute(
        &mut self,
        operation: NativeRegistrationLifecycleRemoteOperation,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, Self::Error>> + '_>> {
        self.operations
            .lock()
            .expect("record operation")
            .push(operation);
        let response = self.responses.pop_front().ok_or(FixturePortError);
        Box::pin(ready(response))
    }
}

/// Build one source-valid mixed lifecycle fixture.
fn lifecycle_value(latest_override: Option<String>) -> JsonValue {
    let witness = hash('e');
    let active = active_registration(&witness);
    let active_sha = object_text(&active, "registrationSha256");
    let legacy_latest_sha = hash('3');
    let legacy_registration_sha = hash('2');
    let predecessor_sha = hash('4');
    let entries = vec![
        entry(
            active,
            latest_override.as_deref().unwrap_or(&active_sha),
            metadata_zero(),
            JsonValue::Null,
            slot(
                "temperature",
                "busy_v3",
                Some(&active_sha),
                Some(TERMINAL),
                &witness,
            ),
        ),
        entry(
            JsonValue::Null,
            &legacy_latest_sha,
            JsonValue::Null,
            JsonValue::Null,
            slot(
                "wind",
                "busy_v2_legacy",
                Some(&legacy_registration_sha),
                Some(TERMINAL),
                &witness,
            ),
        ),
        entry(
            JsonValue::Null,
            &predecessor_sha,
            JsonValue::Null,
            predecessor(&predecessor_sha, &witness),
            slot("rain", "free", None, None, &witness),
        ),
    ];
    object(vec![
        member(
            "contractVersion",
            string("adjustment-registration-lifecycle-status/v4"),
        ),
        member("entries", JsonValue::Array(entries)),
        member("epochAt", string(EPOCH)),
        member("epochWitnessSha256", string(&witness)),
        member("horizonEndAt", string(HORIZON)),
        member(
            "scheduleContractSha256",
            string(ADJUSTMENT_ROLLING_SCHEDULE_SHA256),
        ),
        member("snapshotAt", string(SNAPSHOT)),
    ])
}

/// Build one exact lifecycle entry with explicit nullable members.
fn entry(
    active: JsonValue,
    latest: &str,
    metadata: JsonValue,
    predecessor: JsonValue,
    slot: JsonValue,
) -> JsonValue {
    object(vec![
        member("activeRegistration", active),
        member("latestRegistrationSha256", string(latest)),
        member("metadata", metadata),
        member("predecessor", predecessor),
        member("slot", slot),
    ])
}

/// Build one exact database slot fixture.
fn slot(
    family: &str,
    state: &str,
    registration_sha256: Option<&str>,
    terminal_at: Option<&str>,
    witness: &str,
) -> JsonValue {
    object(vec![
        member(
            "contractVersion",
            string("adjustment-shadow-registration-slot/v3"),
        ),
        member("epochWitnessSha256", string(witness)),
        member("family", string(family)),
        member("horizonEndAt", string(HORIZON)),
        member(
            "registrationSha256",
            registration_sha256.map_or(JsonValue::Null, string),
        ),
        member(
            "scheduleContractSha256",
            string(ADJUSTMENT_ROLLING_SCHEDULE_SHA256),
        ),
        member("state", string(state)),
        member("terminalAt", terminal_at.map_or(JsonValue::Null, string)),
    ])
}

/// Build one source-valid active registration and final-LF identity.
fn active_registration(witness: &str) -> JsonValue {
    let family = "temperature";
    let candidate = hash('b');
    let artifact = hash('a');
    let policy = hash('d');
    let cohort = hash('c');
    let reserved = hash('f');
    let source = hash('1');
    let interval_start = "2026-11-01T07:00:00.000Z";
    let interval_end = "2027-10-03T07:00:00.000Z";
    let material = format!(
        "adjustment-shadow-registration/v3\nballydidean\n{family}\n{candidate}\n{artifact}\n{policy}\n{cohort}\n{reserved}\n{source}\n{witness}\n{ADJUSTMENT_ROLLING_SCHEDULE_SHA256}\nnone\n{interval_start}\n{interval_end}\n{TERMINAL}\n{TERMINAL}\n"
    );
    let registration = digest(material.as_bytes());
    object(vec![
        member("artifactSha256", string(&artifact)),
        member("candidateSha256", string(&candidate)),
        member("cohortSha256", string(&cohort)),
        member("epochWitnessSha256", string(witness)),
        member("family", string(family)),
        member("intervalEndAt", string(interval_end)),
        member("intervalStartAt", string(interval_start)),
        member("policySha256", string(&policy)),
        member("predecessorRegistrationSha256", JsonValue::Null),
        member("registrationSha256", string(&registration)),
        member("reservedKeySha256", string(&reserved)),
        member(
            "scheduleContractSha256",
            string(ADJUSTMENT_ROLLING_SCHEDULE_SHA256),
        ),
        member("siteKey", string("ballydidean")),
        member("sourceSha256", string(&source)),
        member("targetCutoffAt", string(TERMINAL)),
        member("terminalAt", string(TERMINAL)),
    ])
}

/// Build the exact generation-zero metadata root.
fn metadata_zero() -> JsonValue {
    object(vec![
        member("finalizedPredictionCount", JsonValue::Number(0.0)),
        member("generation", JsonValue::Number(0.0)),
        member("lastAnchorSha256", JsonValue::Null),
        member(
            "rootSha256",
            string("4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945"),
        ),
        member("throughAt", JsonValue::Null),
    ])
}

/// Build one source-valid reconciled predecessor.
fn predecessor(registration_sha256: &str, witness: &str) -> JsonValue {
    object(vec![
        member("epochWitnessSha256", string(witness)),
        member("reconciliationSha256", string(&hash('5'))),
        member("registrationSha256", string(registration_sha256)),
        member("reservedKeySha256", string(&hash('6'))),
        member(
            "scheduleContractSha256",
            string(ADJUSTMENT_ROLLING_SCHEDULE_SHA256),
        ),
        member("sourceSha256", string(&hash('7'))),
        member("terminalAt", string("2026-10-09T05:10:45.000Z")),
    ])
}

/// Reconstruct the exact canonical bootstrap request for operation assertions.
fn bootstrap_value(
    bootstrap_sha256: &str,
    witness: &str,
    first_complete_local_date: &str,
    horizon_end_at: &str,
) -> JsonValue {
    object(vec![
        member("bootstrapSha256", string(bootstrap_sha256)),
        member(
            "contractVersion",
            string("adjustment-registration-schedule-bootstrap/v3"),
        ),
        member("epochAt", string(EPOCH)),
        member("epochWitnessSha256", string(witness)),
        member("firstCompleteLocalDate", string(first_complete_local_date)),
        member("horizonEndAt", string(horizon_end_at)),
        member(
            "scheduleContractSha256",
            string(ADJUSTMENT_ROLLING_SCHEDULE_SHA256),
        ),
    ])
}

/// Encode one fixture through the real canonical framing owner.
fn canonical(value: &JsonValue) -> Vec<u8> {
    NativeWirePrimitives
        .encode_canonical_json(value)
        .expect("encode canonical fixture")
}

/// Decode one required fixture string.
fn object_text(value: &JsonValue, name: &str) -> String {
    let JsonValue::Object(fields) = value else {
        panic!("fixture object");
    };
    let (_, JsonValue::String(units)) = fields
        .iter()
        .find(|(key, _)| key.iter().copied().eq(name.encode_utf16()))
        .expect("fixture field")
    else {
        panic!("fixture string");
    };
    String::from_utf16(units).expect("fixture UTF-16")
}

/// Construct one ordered object.
fn object(fields: Vec<(Vec<u16>, JsonValue)>) -> JsonValue {
    JsonValue::Object(fields)
}

/// Construct one object member.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Construct one UTF-16 string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Produce one repeated lowercase digest fixture.
fn hash(value: char) -> String {
    value.to_string().repeat(64)
}

/// Hash one exact source identity preimage.
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
