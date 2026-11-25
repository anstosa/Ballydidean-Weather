//! Exact native projection of the retained registration schedule SQL envelope.

use std::io::{self, Cursor, Read};

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::{
    NativeWirePrimitives,
    capture_epoch_operator::{CaptureEpochOperatorFile, CaptureEpochOperatorFiles},
    capture_epoch_witness::{
        resolve_operator_capture_clock, verify_operator_capture_epoch_witness,
    },
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_database_ledger::{
        project_native_maintenance_database_ledger_v3,
        read_native_maintenance_database_ledger_bytes,
    },
    native_maintenance_rolling_schedule::ADJUSTMENT_ROLLING_SCHEDULE_SHA256,
};

const INPUT_MAXIMUM_BYTES: usize = 64 * 1024;
const SCHEDULE_VERSION: &str = "adjustment-registration-schedule-status/v3";
const SLOT_VERSION: &str = "adjustment-shadow-registration-slot/v3";
const RETAINED_ROLLING_MIGRATION_HISTORY_SHA256: &str =
    "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";
const ENVELOPE_FIELDS: &[&str] = &["databaseManifest", "payload", "transaction"];
const SLOT_FIELDS: &[&str] = &[
    "contractVersion",
    "epochWitnessSha256",
    "family",
    "horizonEndAt",
    "registrationSha256",
    "scheduleContractSha256",
    "state",
    "terminalAt",
];

/// Project one bounded SQL envelope using the independently retained canonical epoch witness.
pub fn project_native_registration_schedule_status_v3(
    input: impl Read,
    epoch_witness_bytes: &[u8],
) -> io::Result<Vec<u8>> {
    let bytes = bounded_input(input)?;
    project_schedule_bytes(&bytes, epoch_witness_bytes)
}

/// Read the fixed operator witness after stdin, then project the authenticated schedule status.
pub fn project_native_registration_schedule_status_v3_from_operator(
    input: impl Read,
    files: &mut impl CaptureEpochOperatorFiles,
) -> io::Result<Vec<u8>> {
    let bytes = bounded_input(input)?;
    let witness = files
        .read(CaptureEpochOperatorFile::Witness)
        .map_err(|_| invalid_schedule())?;
    project_schedule_bytes(&bytes, &witness)
}

/// Share the exact projection after the source-ordered input and witness reads are complete.
fn project_schedule_bytes(bytes: &[u8], epoch_witness_bytes: &[u8]) -> io::Result<Vec<u8>> {
    let wire = NativeWirePrimitives;
    let envelope = wire.decode_json(bytes).ok_or_else(invalid_schedule)?;
    // reject caller extensions before deriving the metadata-only ledger projection
    if !exact_keys(&envelope, ENVELOPE_FIELDS) {
        return Err(invalid_schedule());
    }
    let manifest = field(&envelope, "databaseManifest").ok_or_else(invalid_schedule)?;
    let transaction = field(&envelope, "transaction").ok_or_else(invalid_schedule)?;
    let slots = match field(&envelope, "payload") {
        Some(JsonValue::Array(slots)) if slots.len() == 3 => slots,
        _ => return Err(invalid_schedule()),
    };
    let ledger_input = JsonValue::Object(vec![
        member("databaseManifest", manifest.clone()),
        member("payload", JsonValue::Null),
        member("transaction", transaction.clone()),
    ]);
    let ledger_input_bytes = wire
        .encode_sorted_json(&ledger_input)
        .ok_or_else(invalid_schedule)?;
    let ledger_bytes =
        project_native_maintenance_database_ledger_v3(Cursor::new(ledger_input_bytes))?;
    let ledger = read_native_maintenance_database_ledger_bytes(&ledger_bytes)?;

    let mut codec = NativeWirePrimitives;
    let admitted = verify_operator_capture_epoch_witness(
        epoch_witness_bytes,
        &mut codec,
        &NativeWirePrimitives,
    )
    .map_err(|_| invalid_schedule())?;
    let witness = &admitted.witness;
    // bind this projection to the retained rolling epoch rather than a later compatible witness
    if witness.databaseMigrationHistorySha256 != RETAINED_ROLLING_MIGRATION_HISTORY_SHA256 {
        return Err(invalid_schedule());
    }
    let horizon_end_at = validate_slots(slots, &witness.witnessSha256, &wire)?;
    let snapshot =
        resolve_operator_capture_clock(&ledger.snapshot_at, &wire).ok_or_else(invalid_schedule)?;
    let horizon =
        resolve_operator_capture_clock(&horizon_end_at, &wire).ok_or_else(invalid_schedule)?;
    // require the actual read-only transaction to occur at or after the epoch and cover its future
    if snapshot.epoch_milliseconds < admitted.capture_epoch.epoch_milliseconds
        || horizon.epoch_milliseconds <= admitted.capture_epoch.epoch_milliseconds
    {
        return Err(invalid_schedule());
    }
    let output = JsonValue::Object(vec![
        member("contractVersion", string(SCHEDULE_VERSION)),
        member("epochAt", string(&witness.epochAt)),
        member("epochWitnessSha256", string(&witness.witnessSha256)),
        member("horizonEndAt", string(&horizon_end_at)),
        member(
            "scheduleContractSha256",
            string(ADJUSTMENT_ROLLING_SCHEDULE_SHA256),
        ),
        member("snapshotAt", string(&ledger.snapshot_at)),
        member("slots", JsonValue::Array(slots.to_vec())),
    ]);
    wire.encode_sorted_json(&output)
        .ok_or_else(invalid_schedule)
}

/// Read the SQL envelope without allowing a caller-controlled allocation beyond the source bound.
fn bounded_input(input: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input
        .take((INPUT_MAXIMUM_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    // reject empty, truncated, or over-bound documents before JSON decoding
    if bytes.is_empty() || bytes.len() > INPUT_MAXIMUM_BYTES {
        return Err(invalid_schedule());
    }
    Ok(bytes)
}

/// Validate the fixed family order, shared horizon, epoch binding, and slot occupancy grammar.
fn validate_slots(
    slots: &[JsonValue],
    witness_sha256: &str,
    time: &NativeWirePrimitives,
) -> io::Result<String> {
    let families = ["temperature", "wind", "rain"];
    let horizon_end_at = text(&slots[0], "horizonEndAt").ok_or_else(invalid_schedule)?;
    // inspect each fixed database row in the predecessor's projection order
    for (slot, family) in slots.iter().zip(families) {
        // reject extensions and cross-epoch schedule rows before inspecting occupancy
        if !exact_keys(slot, SLOT_FIELDS)
            || text(slot, "contractVersion").as_deref() != Some(SLOT_VERSION)
            || text(slot, "epochWitnessSha256").as_deref() != Some(witness_sha256)
            || text(slot, "family").as_deref() != Some(family)
            || text(slot, "horizonEndAt").as_deref() != Some(horizon_end_at.as_str())
            || text(slot, "scheduleContractSha256").as_deref()
                != Some(ADJUSTMENT_ROLLING_SCHEDULE_SHA256)
        {
            return Err(invalid_schedule());
        }
        let state = text(slot, "state").ok_or_else(invalid_schedule)?;
        let registration = nullable_text(slot, "registrationSha256")?;
        let terminal = nullable_text(slot, "terminalAt")?;
        // free rows must be empty while both retained busy spellings carry complete identities
        match state.as_str() {
            "free" if registration.is_none() && terminal.is_none() => {}
            "busy_v3" | "busy_v2_legacy"
                if registration.as_deref().is_some_and(valid_hash) && terminal.is_some() => {}
            _ => return Err(invalid_schedule()),
        }
        // occupied rows retain their exact canonical terminal instant
        if terminal
            .as_deref()
            .is_some_and(|value| resolve_operator_capture_clock(value, time).is_none())
        {
            return Err(invalid_schedule());
        }
    }
    // validate the shared horizon independently of the later epoch ordering check
    if resolve_operator_capture_clock(&horizon_end_at, time).is_none() {
        return Err(invalid_schedule());
    }
    Ok(horizon_end_at)
}

/// Decode an exact nullable string field without accepting omission or another JSON type.
fn nullable_text(value: &JsonValue, key: &str) -> io::Result<Option<String>> {
    match field(value, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .map(Some)
            .map_err(|_| invalid_schedule()),
        _ => Err(invalid_schedule()),
    }
}

/// Construct one producer-ordered JSON object member.
fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// Construct one generated JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Collapse all malformed or cross-authority input to the fixed projection refusal.
fn invalid_schedule() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "registration schedule status is invalid",
    )
}
