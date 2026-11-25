//! Exact native owner for the retained registration lifecycle SQL and response projection.

use std::io::{self, Cursor, Read};

use jiff::{Span, civil::Date};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::{
    CaptureEpochOperatorFile, CaptureEpochOperatorFiles, NativePredecessorModelCalendar,
    NativeRegistrationLifecycleStatus, NativeRegistrationSlotState, NativeWirePrimitives,
    decode_registration_lifecycle_status,
    maintenance_canonical_json::{exact_keys, field, text},
    project_native_registration_schedule_status_v3,
};

const INPUT_MAXIMUM_BYTES: usize = 64 * 1024;
const DAY_MILLISECONDS: i64 = 86_400_000;
const LIFECYCLE_VERSION: &str = "adjustment-registration-lifecycle-status/v4";
const ENVELOPE_FIELDS: &[&str] = &["databaseManifest", "payload", "transaction"];
const ENTRY_FIELDS: &[&str] = &[
    "activeRegistration",
    "latestRegistrationSha256",
    "metadata",
    "predecessor",
    "slot",
];
const LIFECYCLE_STATUS_SQL_V4: &[u8] = br###"BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout = '5min';
SET LOCAL lock_timeout = '5s';
SET LOCAL idle_in_transaction_session_timeout = '30s';
WITH families(family, ordinal) AS (VALUES ('temperature', 1), ('wind', 2), ('rain', 3)),
entries AS (
  SELECT families.ordinal, jsonb_build_object(
    'slot', adjustment_shadow_registration_slot_v3(families.family),
    'latestRegistrationSha256', latest.registration_sha256,
    'activeRegistration', CASE WHEN window_row.registration_sha256 IS NULL THEN NULL
      ELSE jsonb_build_object('artifactSha256', window_row.artifact_sha256,
'candidateSha256', window_row.candidate_sha256,
'cohortSha256', window_row.cohort_sha256,
'epochWitnessSha256', window_row.epoch_witness_sha256,
'family', window_row.family,
'intervalEndAt', to_char(window_row.interval_end_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
'intervalStartAt', to_char(window_row.interval_start_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
'policySha256', window_row.policy_sha256,
'predecessorRegistrationSha256', window_row.predecessor_registration_sha256,
'registrationSha256', window_row.registration_sha256,
'reservedKeySha256', window_row.reserved_key_sha256,
'scheduleContractSha256', window_row.schedule_contract_sha256,
'siteKey', window_row.site_key,
'sourceSha256', window_row.source_sha256,
'targetCutoffAt', to_char(window_row.target_cutoff_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
'terminalAt', to_char(window_row.terminal_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')) END,
    'metadata', CASE WHEN window_row.registration_sha256 IS NULL THEN NULL ELSE jsonb_build_object(
      'generation', hot.metadata_generation,
      'rootSha256', hot.finalized_metadata_root_sha256,
      'finalizedPredictionCount', hot.finalized_prediction_count,
      'throughAt', to_char(hot.finalized_through_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
      'lastAnchorSha256', hot.last_finalization_anchor_sha256) END,
    'predecessor', CASE WHEN predecessor.registration_sha256 IS NULL THEN NULL ELSE jsonb_build_object(
      'registrationSha256', predecessor.registration_sha256,
      'terminalAt', to_char(predecessor.terminal_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
      'reconciliationSha256', terminal.reconciliation_sha256,
      'sourceSha256', predecessor.source_sha256,
      'reservedKeySha256', predecessor.reserved_key_sha256,
      'epochWitnessSha256', predecessor.epoch_witness_sha256,
      'scheduleContractSha256', predecessor.schedule_contract_sha256) END
  ) AS value FROM families
  LEFT JOIN adjustment_shadow_registrations_v2 hot ON hot.family = families.family
  LEFT JOIN adjustment_shadow_registration_windows_v3 window_row
    ON window_row.registration_sha256 = hot.registration_sha256
  LEFT JOIN LATERAL (SELECT registration_sha256
    FROM adjustment_shadow_registration_windows_v3 history
    WHERE history.family = families.family ORDER BY interval_start_at DESC LIMIT 1) latest ON true
  LEFT JOIN adjustment_shadow_terminal_results_v2 terminal ON terminal.family = families.family
  LEFT JOIN adjustment_shadow_registration_windows_v3 predecessor
    ON predecessor.registration_sha256 = terminal.registration_sha256
    AND predecessor.source_sha256 = terminal.source_sha256
    AND predecessor.reserved_key_sha256 = terminal.reserved_key_sha256
)
SELECT json_build_object(
  'databaseManifest', (SELECT row_to_json(manifest) FROM adjustment_evaluation_export_manifest_v1 manifest),
  'payload', (SELECT json_agg(value ORDER BY ordinal) FROM entries),
  'transaction', json_build_object(
    'created_at_utc', to_char(transaction_timestamp() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
    'idle_in_transaction_session_timeout', current_setting('idle_in_transaction_session_timeout'),
    'isolation_level', current_setting('transaction_isolation'),
    'lock_timeout', current_setting('lock_timeout'),
    'read_only', current_setting('transaction_read_only'),
    'statement_timeout', current_setting('statement_timeout')));
COMMIT;
"###;

/// Return the source-exact, operand-free read-only lifecycle query bytes.
pub const fn build_native_registration_lifecycle_status_sql_v4() -> &'static [u8] {
    LIFECYCLE_STATUS_SQL_V4
}

/// Project one lifecycle envelope using already read canonical operator-witness bytes.
pub fn project_native_registration_lifecycle_status_v4(
    input: impl Read,
    epoch_witness_bytes: &[u8],
) -> io::Result<Vec<u8>> {
    let bytes = bounded_input(input)?;
    project_lifecycle_bytes(&bytes, epoch_witness_bytes)
}

/// Preserve the predecessor's stdin-before-authority-read order for the installed operator.
pub fn project_native_registration_lifecycle_status_v4_from_operator(
    input: impl Read,
    files: &mut impl CaptureEpochOperatorFiles,
) -> io::Result<Vec<u8>> {
    let bytes = bounded_input(input)?;
    let witness = files
        .read(CaptureEpochOperatorFile::Witness)
        .map_err(|_| invalid_lifecycle())?;
    project_lifecycle_bytes(&bytes, &witness)
}

/// Share projection only after the bounded SQL result and retained witness reads complete.
fn project_lifecycle_bytes(bytes: &[u8], witness_bytes: &[u8]) -> io::Result<Vec<u8>> {
    let wire = NativeWirePrimitives;
    let envelope = wire.decode_json(bytes).ok_or_else(invalid_lifecycle)?;
    // reject root extensions before extracting the fixed family slot projection
    if !exact_keys(&envelope, ENVELOPE_FIELDS) {
        return Err(invalid_lifecycle());
    }
    let manifest = field(&envelope, "databaseManifest").ok_or_else(invalid_lifecycle)?;
    let transaction = field(&envelope, "transaction").ok_or_else(invalid_lifecycle)?;
    let entries = match field(&envelope, "payload") {
        Some(JsonValue::Array(entries)) if entries.len() == 3 => entries,
        _ => return Err(invalid_lifecycle()),
    };
    let mut slots = Vec::with_capacity(3);
    // retain fixed database order while requiring each complete lifecycle entry shape
    for entry in entries {
        if !exact_keys(entry, ENTRY_FIELDS) {
            return Err(invalid_lifecycle());
        }
        slots.push(field(entry, "slot").ok_or_else(invalid_lifecycle)?.clone());
    }
    let schedule_envelope = JsonValue::Object(vec![
        member("databaseManifest", manifest.clone()),
        member("payload", JsonValue::Array(slots)),
        member("transaction", transaction.clone()),
    ]);
    let schedule_input = wire
        .encode_sorted_json(&schedule_envelope)
        .ok_or_else(invalid_lifecycle)?;
    let schedule_bytes =
        project_native_registration_schedule_status_v3(Cursor::new(schedule_input), witness_bytes)?;
    let schedule = wire
        .decode_json(&schedule_bytes)
        .ok_or_else(invalid_lifecycle)?;
    let output = JsonValue::Object(vec![
        member("contractVersion", string(LIFECYCLE_VERSION)),
        member("entries", JsonValue::Array(entries.to_vec())),
        member("epochAt", required_text(&schedule, "epochAt")?),
        member(
            "epochWitnessSha256",
            required_text(&schedule, "epochWitnessSha256")?,
        ),
        member("horizonEndAt", required_text(&schedule, "horizonEndAt")?),
        member(
            "scheduleContractSha256",
            required_text(&schedule, "scheduleContractSha256")?,
        ),
        member("snapshotAt", required_text(&schedule, "snapshotAt")?),
    ]);
    let canonical = wire
        .encode_sorted_json(&output)
        .ok_or_else(invalid_lifecycle)?;
    let mut codec = NativeWirePrimitives;
    let status = decode_registration_lifecycle_status(&mut codec, &canonical, &wire)
        .map_err(|_| invalid_lifecycle())?;
    validate_source_geometry(&status)?;
    Ok(canonical)
}

/// Reapply the predecessor-only rolling geometry checks not represented by the shared typed decoder.
fn validate_source_geometry(status: &NativeRegistrationLifecycleStatus) -> io::Result<()> {
    let epoch = source_milliseconds(&status.epoch_at)?;
    source_milliseconds(&status.horizon_end_at)?;
    source_milliseconds(&status.snapshot_at)?;
    let calendar = NativePredecessorModelCalendar::new().ok_or_else(invalid_lifecycle)?;
    // validate every optional source clock and active rolling window in fixed family order
    for entry in &status.entries {
        if let Some(terminal) = entry.slot.terminal_at.as_deref() {
            source_milliseconds(terminal)?;
        }
        if let Some(predecessor) = entry.predecessor.as_ref() {
            source_milliseconds(&predecessor.terminal_at)?;
        }
        if let Some(metadata) = entry.metadata.as_ref()
            && let Some(through_at) = metadata.through_at.as_deref()
        {
            source_milliseconds(through_at)?;
        }
        // only v3 occupancy carries the complete rolling registration geometry
        if entry.slot.state != NativeRegistrationSlotState::BusyV3 {
            continue;
        }
        let registration = entry
            .active_registration
            .as_ref()
            .ok_or_else(invalid_lifecycle)?;
        let start = source_milliseconds(&registration.interval_start_at)?;
        let end = source_milliseconds(&registration.interval_end_at)?;
        let target = source_milliseconds(&registration.target_cutoff_at)?;
        let terminal = source_milliseconds(&registration.terminal_at)?;
        let start_features = calendar
            .features(&registration.interval_start_at)
            .map_err(|_| invalid_lifecycle())?;
        let end_features = calendar
            .features(&registration.interval_end_at)
            .map_err(|_| invalid_lifecycle())?;
        let start_date = start_features
            .local_date
            .parse::<Date>()
            .map_err(|_| invalid_lifecycle())?;
        let end_date = end_features
            .local_date
            .parse::<Date>()
            .map_err(|_| invalid_lifecycle())?;
        let days = match entry.slot.family.wire() {
            "rain" => 334,
            "temperature" | "wind" => 366,
            _ => return Err(invalid_lifecycle()),
        };
        let expected_end = start_date
            .checked_add(
                Span::new()
                    .try_days(days)
                    .map_err(|_| invalid_lifecycle())?,
            )
            .map_err(|_| invalid_lifecycle())?;
        // require post-epoch local midnights and the exact family span plus seven-day closure
        if start <= epoch
            || start_features.hour != 0
            || end_features.hour != 0
            || !zero_minute_second(&registration.interval_start_at)
            || !zero_minute_second(&registration.interval_end_at)
            || end_date != expected_end
            || registration.target_cutoff_at != registration.terminal_at
            || target != terminal
            || terminal
                != end
                    .checked_add(7 * DAY_MILLISECONDS)
                    .ok_or_else(invalid_lifecycle)?
        {
            return Err(invalid_lifecycle());
        }
    }
    Ok(())
}

/// Parse only the predecessor's four-digit canonical UTC millisecond clock.
fn source_milliseconds(value: &str) -> io::Result<i64> {
    let bytes = value.as_bytes();
    let framed = bytes.len() == 24
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'.',
            23 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        });
    // reject normalized aliases before using the shared instant parser
    if !framed {
        return Err(invalid_lifecycle());
    }
    NativeWirePrimitives
        .parse_explicit_utc_milliseconds(value)
        .ok_or_else(invalid_lifecycle)
}

/// Match the source formatter's local minute and second checks while retaining milliseconds.
fn zero_minute_second(value: &str) -> bool {
    value.as_bytes().get(14..19) == Some(b"00:00")
}

/// Read the SQL response without allocating beyond its source transport bound.
fn bounded_input(input: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input
        .take((INPUT_MAXIMUM_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    // reject empty, truncated, or oversized lifecycle documents before parsing
    if bytes.is_empty() || bytes.len() > INPUT_MAXIMUM_BYTES {
        return Err(invalid_lifecycle());
    }
    Ok(bytes)
}

/// Clone one required generated string value from an internally projected schedule.
fn required_text(value: &JsonValue, key: &str) -> io::Result<JsonValue> {
    text(value, key)
        .map(|value| string(&value))
        .ok_or_else(invalid_lifecycle)
}

/// Construct one generated object member without changing source field spelling.
fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// Construct one generated JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Collapse malformed database, witness, or lifecycle material to one fixed refusal.
fn invalid_lifecycle() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "registration lifecycle status is invalid",
    )
}
