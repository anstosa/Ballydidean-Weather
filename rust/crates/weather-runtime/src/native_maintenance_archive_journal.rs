//! Exact archive-scoped durable lease and due projections over the real journal lock.

use crate::{
    NativeMaintenanceArchiveDue, NativeMaintenanceArchiveJournalStatus,
    NativeMaintenanceArchiveLease, NativeMaintenanceArchiveLeaseRequest,
    NativeMaintenanceArchiveSegmentJournal, NativeMaintenanceJournalRecord,
    NativeMaintenanceJournalStore, NativeMaintenanceJournalStoreError,
    NativeMaintenanceOwnedJournalAddition, NativeWirePrimitives,
    maintenance_canonical_json::{field, text},
    native_maintenance_retirement_files_v2::format_digest,
};
use std::collections::BTreeMap;
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec, RevisionGapSha256};

const ARCHIVE_LEASE_MS: i64 = 3_600_000;
const CLOCK_TOLERANCE_MS: i64 = 300_000;

/// Value-free retained journal state and real durable storage failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceArchiveJournalError {
    Store(NativeMaintenanceJournalStoreError),
    State,
    ClockUntrusted,
    ReconciliationRequired,
    LeaseOccupied,
    DueCollision,
    UnknownLease,
    ExpiredLeaseNotFound,
    DueUnregistered,
    CompletionCollision,
}
impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceArchiveJournalError {
    /// Preserve the actual storage owner's failure.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Store(error)
    }
}

/// Concrete archive lease owner operating each decision under the verified journal lock.
pub struct NativeMaintenanceArchiveJournal<'a> {
    store: &'a mut NativeMaintenanceJournalStore,
}
impl<'a> NativeMaintenanceArchiveJournal<'a> {
    /// Borrow the one actual journal initialized for the current controller.
    pub fn new(store: &'a mut NativeMaintenanceJournalStore) -> Self {
        Self { store }
    }
}
impl NativeMaintenanceArchiveSegmentJournal for NativeMaintenanceArchiveJournal<'_> {
    type Error = NativeMaintenanceArchiveJournalError;
    /// Read the exact current immutable due registration and completion.
    fn due(&mut self, key: &str) -> Result<NativeMaintenanceArchiveDue, Self::Error> {
        let records = self
            .store
            .read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
        due(&records, key)
    }
    /// Project the verified head and active archive mutation lease.
    fn status(&mut self) -> Result<NativeMaintenanceArchiveJournalStatus, Self::Error> {
        let records = self
            .store
            .read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
        let active = active_leases(&records)?;
        let lease = active
            .get("archive")
            .map(|record| -> Result<_, Self::Error> {
                Ok(NativeMaintenanceArchiveLease {
                    due_key: required(&record.payload, "dueKey")?,
                    run_id: required(&record.payload, "runId")?,
                    expires_at: required(&record.payload, "expiresAt")?,
                })
            })
            .transpose()?;
        let head = records
            .last()
            .map(|record| parse_hash(&record.record_sha256))
            .transpose()?;
        Ok(NativeMaintenanceArchiveJournalStatus {
            head_sha256: head,
            active_lease: lease,
        })
    }
    /// Atomically register and acquire exactly one archive-scoped source lease.
    fn acquire(
        &mut self,
        request: NativeMaintenanceArchiveLeaseRequest<'_>,
    ) -> Result<(), Self::Error> {
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                let active = trusted_clock(records, request.now, false)?;
                let registration = due(records, request.due_key)?;
                if registration
                    .input_head_sha256
                    .is_some_and(|digest| digest != request.input_head_sha256)
                {
                    return Err(Self::Error::DueCollision);
                }
                let input = format_digest(request.input_head_sha256);
                // allow only an exact idempotent live acquisition for this archive scope
                if let Some(lease) = active.get("archive") {
                    if required(&lease.payload, "runId")? == request.run_id
                        && required(&lease.payload, "dueKey")? == request.due_key
                        && required(&lease.payload, "inputHeadSha256")? == input
                    {
                        return Ok(vec![]);
                    }
                    return Err(Self::Error::LeaseOccupied);
                }
                if registration.output_sha256.is_some() {
                    return Ok(vec![]);
                }
                let expires = milliseconds(request.now)?
                    .checked_add(ARCHIVE_LEASE_MS)
                    .ok_or(Self::Error::State)?;
                let expires = NativeWirePrimitives
                    .format_utc_milliseconds(expires)
                    .ok_or(Self::Error::State)?;
                let mut additions = Vec::new();
                // retain the immutable due registration before its associated lease
                if registration.input_head_sha256.is_none() {
                    additions.push(addition(
                        request.due_key,
                        "due_registered",
                        format!("due/{}", request.due_key),
                        request.now,
                        object(vec![("inputHeadSha256", string(&input))]),
                    ));
                }
                additions.push(addition(
                    request.due_key,
                    "lease_acquired",
                    format!("lease/archive/{}/acquired", request.run_id),
                    request.now,
                    object(vec![
                        ("scope", string("archive")),
                        ("runId", string(request.run_id)),
                        ("dueKey", string(request.due_key)),
                        ("acquiredAt", string(request.now)),
                        ("expiresAt", string(&expires)),
                        ("inputHeadSha256", string(&input)),
                        ("fencingToken", JsonValue::Null),
                    ]),
                ));
                Ok(additions)
            },
        )
    }
    /// Append only one immutable output for an already registered due key.
    fn complete(&mut self, key: &str, now: &str, output: [u8; 32]) -> Result<(), Self::Error> {
        milliseconds(now)?;
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                let current = due(records, key)?;
                if current.input_head_sha256.is_none() {
                    return Err(Self::Error::DueUnregistered);
                }
                if let Some(existing) = current.output_sha256 {
                    return if existing == output {
                        Ok(vec![])
                    } else {
                        Err(Self::Error::CompletionCollision)
                    };
                }
                Ok(vec![addition(
                    key,
                    "due_completed",
                    format!("due/{key}/complete"),
                    now,
                    object(vec![("outputSha256", string(&format_digest(output)))]),
                )])
            },
        )
    }
    /// Release only the current exact still-live archive lease.
    fn release(&mut self, key: &str, run_id: &str, now: &str) -> Result<(), Self::Error> {
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                let active = trusted_clock(records, now, false)?;
                let lease = active.get("archive").ok_or(Self::Error::UnknownLease)?;
                if required(&lease.payload, "runId")? != run_id
                    || required(&lease.payload, "dueKey")? != key
                {
                    return Err(Self::Error::UnknownLease);
                }
                Ok(vec![addition(
                    key,
                    "lease_released",
                    format!("lease/archive/{run_id}/released"),
                    now,
                    object(vec![
                        ("scope", string("archive")),
                        ("runId", string(run_id)),
                        ("dueKey", string(key)),
                        ("releasedAt", string(now)),
                    ]),
                )])
            },
        )
    }
    /// Persist the exact original expired archive lease reconciliation without manufacturing a due completion.
    fn reconcile_complete(
        &mut self,
        key: &str,
        run_id: &str,
        now: &str,
        output: [u8; 32],
        head: Option<[u8; 32]>,
    ) -> Result<(), Self::Error> {
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                let active = trusted_clock(records, now, true)?;
                let lease = active
                    .get("archive")
                    .ok_or(Self::Error::ExpiredLeaseNotFound)?;
                if required(&lease.payload, "runId")? != run_id
                    || required(&lease.payload, "dueKey")? != key
                    || milliseconds(&required(&lease.payload, "expiresAt")?)? > milliseconds(now)?
                {
                    return Err(Self::Error::ExpiredLeaseNotFound);
                }
                Ok(vec![addition(
                    key,
                    "lease_reconciled",
                    format!("lease/archive/{run_id}/reconciled"),
                    now,
                    object(vec![
                        ("scope", string("archive")),
                        ("runId", string(run_id)),
                        ("dueKey", string(key)),
                        ("reconciledAt", string(now)),
                        ("resolution", string("complete")),
                        ("immutableOutputSha256", string(&format_digest(output))),
                        (
                            "remoteStateSha256",
                            head.map_or(JsonValue::Null, |hash| string(&format_digest(hash))),
                        ),
                    ]),
                )])
            },
        )
    }
}

/// Replay source due registration and completion by exact first immutable key.
pub(crate) fn due(
    records: &[NativeMaintenanceJournalRecord],
    key: &str,
) -> Result<NativeMaintenanceArchiveDue, NativeMaintenanceArchiveJournalError> {
    let first = |name: &str, field_name: &str| -> Result<_, NativeMaintenanceArchiveJournalError> {
        records
            .iter()
            .find(|record| is_kind(record, name) && record.due_key.as_deref() == Some(key))
            .map(|record| parse_hash(&required(&record.payload, field_name)?))
            .transpose()
    };
    Ok(NativeMaintenanceArchiveDue {
        input_head_sha256: first("due_registered", "inputHeadSha256")?,
        output_sha256: first("due_completed", "outputSha256")?,
    })
}

/// Replay every scope so an unrelated expired lease also blocks the source trusted clock.
pub(crate) fn active_leases(
    records: &[NativeMaintenanceJournalRecord],
) -> Result<BTreeMap<String, &NativeMaintenanceJournalRecord>, NativeMaintenanceArchiveJournalError>
{
    let mut active: BTreeMap<String, &NativeMaintenanceJournalRecord> = BTreeMap::new();
    for record in records {
        if is_kind(record, "lease_acquired") {
            active.insert(required(&record.payload, "scope")?, record);
        } else if is_kind(record, "lease_continued")
            || is_kind(record, "lease_released")
            || is_kind(record, "lease_reconciled")
        {
            let scope = required(&record.payload, "scope")?;
            if let Some(current) = active.get(&scope) {
                let matches = field(&current.payload, "runId") == field(&record.payload, "runId")
                    && field(&current.payload, "dueKey") == field(&record.payload, "dueKey");
                if matches && is_kind(record, "lease_continued") {
                    if text(&record.payload, "predecessorLeaseRecordSha256").as_deref()
                        == Some(current.record_sha256.as_str())
                        && field(&current.payload, "fencingToken")
                            == field(&record.payload, "fencingToken")
                    {
                        active.insert(scope, record);
                    }
                } else if matches {
                    active.remove(&scope);
                }
            }
        }
    }
    Ok(active)
}

/// Preserve the source clock rollback allowance and reconciliation requirement.
pub(crate) fn trusted_clock<'a>(
    records: &'a [NativeMaintenanceJournalRecord],
    now: &str,
    allow_expired: bool,
) -> Result<
    BTreeMap<String, &'a NativeMaintenanceJournalRecord>,
    NativeMaintenanceArchiveJournalError,
> {
    use NativeMaintenanceArchiveJournalError as Error;
    let instant = milliseconds(now)?;
    if let Some(latest) = records
        .iter()
        .map(|record| record.recorded_at.as_str())
        .max()
        && instant < milliseconds(latest)?.saturating_sub(CLOCK_TOLERANCE_MS)
    {
        return Err(Error::ClockUntrusted);
    }
    let active = active_leases(records)?;
    if !allow_expired {
        for lease in active.values() {
            if milliseconds(&required(&lease.payload, "expiresAt")?)? <= instant {
                return Err(Error::ReconciliationRequired);
            }
        }
    }
    Ok(active)
}

/// Compare one retained source event discriminator.
fn is_kind(record: &NativeMaintenanceJournalRecord, name: &str) -> bool {
    matches!(&record.kind,JsonValue::String(units) if units.iter().copied().eq(name.encode_utf16()))
}
/// Parse a canonical source clock.
fn milliseconds(time: &str) -> Result<i64, NativeMaintenanceArchiveJournalError> {
    NativeWirePrimitives
        .canonical_utc_milliseconds(time)
        .ok_or(NativeMaintenanceArchiveJournalError::State)
}
/// Read one exact payload string.
fn required(value: &JsonValue, key: &str) -> Result<String, NativeMaintenanceArchiveJournalError> {
    text(value, key).ok_or(NativeMaintenanceArchiveJournalError::State)
}
/// Parse a retained strong identity.
fn parse_hash(value: &str) -> Result<[u8; 32], NativeMaintenanceArchiveJournalError> {
    RevisionGapSha256::from_lower_hex(value)
        .map(|hash| *hash.as_bytes())
        .ok_or(NativeMaintenanceArchiveJournalError::State)
}
/// Construct one owned append mutation before assigning generation and chain hashes.
pub(crate) fn addition(
    key: &str,
    kind: &str,
    record_key: String,
    now: &str,
    payload: JsonValue,
) -> NativeMaintenanceOwnedJournalAddition {
    NativeMaintenanceOwnedJournalAddition {
        due_key: Some(key.to_owned()),
        kind: kind.to_owned(),
        payload,
        record_key,
        recorded_at: now.to_owned(),
    }
}
/// Encode an exact retained event object.
pub(crate) fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}
/// Encode one source JSON scalar.
pub(crate) fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_archive_journal_test.rs"]
mod tests;
