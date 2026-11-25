//! Durable future-only lifecycle genesis over the verified native journal store.

use std::fmt::{self, Display, Formatter};

use weather_api::CurrentTimeResolver;
use weather_contracts::{JsDateOrString, JsonValue};

use crate::capture_epoch_witness::verify_operator_capture_epoch_witness;
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceJournalRecord, NativeMaintenanceJournalStore,
    NativeMaintenanceJournalStoreError, NativeMaintenanceOwnedJournalAddition,
};

const LIFECYCLE_LEDGER_VERSION: &str = "forecast-adjustment-lifecycle-ledger/v2";
const GENESIS_KIND: &str = "lifecycle_genesis";
const GENESIS_RECORD_KEY: &str = "lifecycle/genesis";

/// Closed future-only lifecycle mutation failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFutureOnlyJournalError {
    Store(NativeMaintenanceJournalStoreError),
    Witness,
    Clock,
    Collision,
    Codec,
}

impl Display for NativeMaintenanceFutureOnlyJournalError {
    /// Render one bounded owner diagnostic without witness or path material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve the exact failing boundary
        formatter.write_str(match self {
            Self::Store(error) => return Display::fmt(error, formatter),
            Self::Witness => "future-only lifecycle witness is invalid",
            Self::Clock => "future-only lifecycle genesis clock is invalid",
            Self::Collision => "future-only lifecycle genesis collides",
            Self::Codec => "future-only lifecycle genesis serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceFutureOnlyJournalError {
    /// Preserve only an actual journal-store cause.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // semantic admission failures have no lower-level source
        match self {
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceFutureOnlyJournalError {
    /// Retain the concrete durable-store failure.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Store(error)
    }
}

/// Verify and append the exact future-only lifecycle genesis idempotently.
pub fn initialize_native_maintenance_future_only_journal(
    store: &mut NativeMaintenanceJournalStore,
    now: &str,
    witness_bytes: &[u8],
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeMaintenanceFutureOnlyJournalError> {
    use NativeMaintenanceFutureOnlyJournalError as Error;

    let admitted = verify_operator_capture_epoch_witness(witness_bytes, codec, time)
        .map_err(|_| Error::Witness)?;
    let witness = codec
        .parse_canonical_object(witness_bytes)
        .ok_or(Error::Witness)?;
    // preserve the exact operator bytes instead of rebuilding a projected witness
    if codec.encode_canonical_json(&witness).as_deref() != Some(witness_bytes) {
        return Err(Error::Witness);
    }
    let now_milliseconds = canonical_journal_instant(now, time).ok_or(Error::Clock)?;
    // never append the local lifecycle event before its authenticated server epoch
    if now_milliseconds < admitted.capture_epoch.epoch_milliseconds {
        return Err(Error::Clock);
    }
    let payload = object(vec![
        ("contractVersion", string(LIFECYCLE_LEDGER_VERSION)),
        ("legacyHistoryStatus", string("unavailable")),
        ("sourceKind", string("future_only_verified_capture_epoch")),
        ("witness", witness),
    ]);
    let payload_bytes = codec.encode_canonical_json(&payload).ok_or(Error::Codec)?;
    let payload_sha256 = codec
        .sha256(&payload_bytes)
        .ok_or(Error::Codec)?
        .as_str()
        .to_owned();
    store.mutate_checked(codec, time, |records| {
        // enforce record-key uniqueness under the same verified journal lock
        if let Some(existing) = records
            .iter()
            .find(|record| record.record_key == GENESIS_RECORD_KEY)
        {
            // reuse only the exact immutable source event
            if !is_kind(existing, GENESIS_KIND)
                || existing.due_key.is_some()
                || existing.payload_sha256 != payload_sha256
            {
                return Err(Error::Collision);
            }
            return Ok(Vec::new());
        }
        Ok(vec![NativeMaintenanceOwnedJournalAddition {
            due_key: None,
            kind: GENESIS_KIND.to_owned(),
            payload,
            record_key: GENESIS_RECORD_KEY.to_owned(),
            recorded_at: now.to_owned(),
        }])
    })
}

/// Admit the source's exact four-digit UTC millisecond journal clock.
fn canonical_journal_instant(value: &str, time: &dyn CurrentTimeResolver) -> Option<i64> {
    let bytes = value.as_bytes();
    // retain the predecessor's fixed requireInstant grammar before calendar resolution
    if bytes.len() != 24
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || bytes.get(19) != Some(&b'.')
        || bytes.get(23) != Some(&b'Z')
        || bytes.iter().enumerate().any(|(index, byte)| {
            matches!(index, 0..=3 | 5..=6 | 8..=9 | 11..=12 | 14..=15 | 17..=18 | 20..=22)
                && !byte.is_ascii_digit()
        })
    {
        return None;
    }
    let resolved = time
        .resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .ok()?;
    (resolved.utc == value).then_some(resolved.epoch_milliseconds)
}

/// Compare one verified journal event discriminator.
fn is_kind(record: &NativeMaintenanceJournalRecord, expected: &str) -> bool {
    matches!(&record.kind, JsonValue::String(units) if units.iter().copied().eq(expected.encode_utf16()))
}

/// Build one canonical object in the explicit source field order.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one JSON string without changing its UTF-16 representation.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_future_only_journal_test.rs"]
mod tests;
