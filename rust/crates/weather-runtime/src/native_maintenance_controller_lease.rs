//! Exact ordinary daily/monthly controller lease transactions over the verified journal.

use crate::{
    NativeMaintenanceArchiveDue, NativeMaintenanceArchiveJournal,
    NativeMaintenanceArchiveJournalError, NativeMaintenanceArchiveSegmentJournal,
    NativeMaintenanceJournalStore, NativeWirePrimitives,
    maintenance_canonical_json::text,
    native_maintenance_archive_journal::{
        active_leases, addition, due, object, string, trusted_clock,
    },
    native_maintenance_attempt_report::native_maintenance_controller_run_id,
    native_maintenance_retirement_files_v2::format_digest,
};
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec, RevisionGapSha256};

/// Verified ordinary controller head and immutable due registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceControllerLeaseState {
    pub due: NativeMaintenanceArchiveDue,
    pub head_sha256: Option<[u8; 32]>,
}

/// Concrete journal owner for the controller's non-fenced execution leases.
pub struct NativeMaintenanceControllerLeaseJournal<'a> {
    store: &'a mut NativeMaintenanceJournalStore,
}

impl<'a> NativeMaintenanceControllerLeaseJournal<'a> {
    /// Bind the existing admitted process-owned journal.
    pub fn new(store: &'a mut NativeMaintenanceJournalStore) -> Self {
        Self { store }
    }

    /// Read a source-consistent due state and actual verified journal head.
    pub fn state(
        &mut self,
        due_key: &str,
    ) -> Result<NativeMaintenanceControllerLeaseState, NativeMaintenanceArchiveJournalError> {
        let records = self
            .store
            .read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
        Ok(NativeMaintenanceControllerLeaseState {
            due: due(&records, due_key)?,
            head_sha256: records
                .last()
                .map(|record| {
                    RevisionGapSha256::from_lower_hex(&record.record_sha256)
                        .map(|digest| *digest.as_bytes())
                        .ok_or(NativeMaintenanceArchiveJournalError::State)
                })
                .transpose()?,
        })
    }

    /// Reconcile only the selected expired ordinary controller scope.
    pub fn reconcile_expired(
        &mut self,
        scope: &str,
        now: &str,
    ) -> Result<(), NativeMaintenanceArchiveJournalError> {
        use NativeMaintenanceArchiveJournalError as Error;
        let instant = milliseconds(now)?;
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                // absent and still-live source scopes require no reconciliation
                let active = active_leases(records)?;
                let Some(lease) = active.get(scope) else {
                    return Ok(Vec::new());
                };
                if milliseconds(&required(&lease.payload, "expiresAt")?)? > instant {
                    return Ok(Vec::new());
                }
                duration(scope).ok_or(Error::State)?;
                trusted_clock(records, now, true)?;
                let due_key = required(&lease.payload, "dueKey")?;
                let run_id = required(&lease.payload, "runId")?;
                let current = due(records, &due_key)?;
                // resolve only the actual already-completed output or a resumable unfinished due
                let (resolution, output) = match current.output_sha256 {
                    Some(output) => ("complete", string(&format_digest(output))),
                    None => ("resume", JsonValue::Null),
                };
                Ok(vec![addition(
                    &due_key,
                    "lease_reconciled",
                    format!("lease/{scope}/{run_id}/reconciled"),
                    now,
                    object(vec![
                        ("scope", string(scope)),
                        ("runId", string(&run_id)),
                        ("dueKey", string(&due_key)),
                        ("reconciledAt", string(now)),
                        ("resolution", string(resolution)),
                        ("immutableOutputSha256", output),
                        ("remoteStateSha256", JsonValue::Null),
                    ]),
                )])
            },
        )
    }

    /// Atomically register and acquire one exact ordinary daily or monthly lease.
    pub fn acquire(
        &mut self,
        scope: &str,
        due_key: &str,
        run_id: &str,
        now: &str,
        input_head: [u8; 32],
    ) -> Result<(), NativeMaintenanceArchiveJournalError> {
        use NativeMaintenanceArchiveJournalError as Error;
        let duration = duration(scope).ok_or(Error::State)?;
        let instant = milliseconds(now)?;
        let input = format_digest(input_head);
        // only the stable source controller run identifier is admitted here
        if !run_id.strip_prefix("maintenance-").is_some_and(|id| {
            id.len() == 32
                && id
                    .bytes()
                    .all(|n| n.is_ascii_hexdigit() && !n.is_ascii_uppercase())
        }) {
            return Err(Error::State);
        }
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                let active = trusted_clock(records, now, false)?;
                let current = due(records, due_key)?;
                if current
                    .input_head_sha256
                    .is_some_and(|head| head != input_head)
                {
                    return Err(Error::DueCollision);
                }
                // accept only the same still-live immutable source lease
                if let Some(lease) = active.get(scope) {
                    if required(&lease.payload, "runId")? == run_id
                        && required(&lease.payload, "dueKey")? == due_key
                        && required(&lease.payload, "inputHeadSha256")? == input
                    {
                        return Ok(Vec::new());
                    }
                    return Err(Error::LeaseOccupied);
                }
                if current.output_sha256.is_some() {
                    return Ok(Vec::new());
                }
                let expires = NativeWirePrimitives
                    .format_utc_milliseconds(instant.checked_add(duration).ok_or(Error::State)?)
                    .ok_or(Error::State)?;
                let mut additions = Vec::with_capacity(2);
                // record an immutable due registration before its first lease acquisition
                if current.input_head_sha256.is_none() {
                    additions.push(addition(
                        due_key,
                        "due_registered",
                        format!("due/{due_key}"),
                        now,
                        object(vec![("inputHeadSha256", string(&input))]),
                    ));
                }
                additions.push(addition(
                    due_key,
                    "lease_acquired",
                    format!("lease/{scope}/{run_id}/acquired"),
                    now,
                    object(vec![
                        ("scope", string(scope)),
                        ("runId", string(run_id)),
                        ("dueKey", string(due_key)),
                        ("acquiredAt", string(now)),
                        ("expiresAt", string(&expires)),
                        ("inputHeadSha256", string(&input)),
                        ("fencingToken", JsonValue::Null),
                    ]),
                ));
                Ok(additions)
            },
        )
    }

    /// Release only one exact live ordinary scope.
    pub fn release(
        &mut self,
        scope: &str,
        due_key: &str,
        run_id: &str,
        now: &str,
    ) -> Result<(), NativeMaintenanceArchiveJournalError> {
        use NativeMaintenanceArchiveJournalError as Error;
        duration(scope).ok_or(Error::State)?;
        self.store.mutate_checked(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
            |records| {
                let active = trusted_clock(records, now, false)?;
                let lease = active.get(scope).ok_or(Error::UnknownLease)?;
                if required(&lease.payload, "dueKey")? != due_key
                    || required(&lease.payload, "runId")? != run_id
                {
                    return Err(Error::UnknownLease);
                }
                Ok(vec![addition(
                    due_key,
                    "lease_released",
                    format!("lease/{scope}/{run_id}/released"),
                    now,
                    object(vec![
                        ("scope", string(scope)),
                        ("runId", string(run_id)),
                        ("dueKey", string(due_key)),
                        ("releasedAt", string(now)),
                    ]),
                )])
            },
        )
    }

    /// Complete the already-registered due only with a genuine report graph address.
    pub fn complete(
        &mut self,
        key: &str,
        now: &str,
        output: [u8; 32],
    ) -> Result<(), NativeMaintenanceArchiveJournalError> {
        NativeMaintenanceArchiveJournal::new(self.store).complete(key, now, output)
    }

    /// Derive the original controller run from the actual current verified head.
    pub fn run_id(due_key: &str, head: [u8; 32]) -> Option<String> {
        native_maintenance_controller_run_id(due_key, &format_digest(head))
    }
}

// select only the source ordinary controller lease classes and their exact durations
fn duration(scope: &str) -> Option<i64> {
    match scope {
        "daily" | "confirmation/temperature" | "confirmation/wind" | "confirmation/rain" => {
            Some(3 * 60 * 60 * 1000)
        }
        "monthly/temperature" | "monthly/wind" | "monthly/rain" => Some(6 * 60 * 60 * 1000),
        _ => None,
    }
}

// require the original canonical local commit clock
fn milliseconds(value: &str) -> Result<i64, NativeMaintenanceArchiveJournalError> {
    NativeWirePrimitives
        .canonical_utc_milliseconds(value)
        .ok_or(NativeMaintenanceArchiveJournalError::State)
}

// read an exact existing source field
fn required(value: &JsonValue, key: &str) -> Result<String, NativeMaintenanceArchiveJournalError> {
    text(value, key).ok_or(NativeMaintenanceArchiveJournalError::State)
}

#[cfg(test)]
#[path = "native_maintenance_controller_lease_test.rs"]
mod tests;
