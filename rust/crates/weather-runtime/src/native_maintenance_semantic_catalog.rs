//! Actual source semantic catalog selection and reconstruction from authenticated cold evidence.

use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceCaptureFamily, NativeMaintenanceDue,
    NativeMaintenanceDueEvidence, NativeMaintenanceDueMode, NativeMaintenanceDueWorkReason,
    NativeMaintenanceEvidenceSynchronization, NativeMaintenanceHistoricalCatalogJournal,
    NativeMaintenanceHistoricalCatalogOutcome, NativeMaintenanceHistoricalCatalogReady,
    NativeMaintenanceHistoricalFamily, NativeMaintenanceHistoricalSelection,
    NativeMaintenanceJournalStore, NativeMaintenancePlaintextObjectStore,
    NativeRegistrationLifecycleEntry, NativeRegistrationLifecycleStatus,
    NativeRevisionColdDrainState, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness,
    maintenance_canonical_json::{field, text, valid_hash},
    select_native_maintenance_daily_lifecycle_entry, verify_native_maintenance_due,
};

/// Source semantic selection and admitted active candidate after a complete historical restore.
pub struct NativeMaintenanceSemanticCatalog {
    pub catalog: Box<NativeMaintenanceHistoricalCatalogReady>,
    pub active_entry: Option<NativeRegistrationLifecycleEntry>,
}

/// Source-blocked receipt or exact complete semantic input.
pub enum NativeMaintenanceSemanticCatalogOutcome {
    Blocked(NativeMaintenanceDueWorkReason),
    Ready(Box<NativeMaintenanceSemanticCatalog>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceSemanticCatalogError {
    Due,
    Lifecycle,
    Source,
    Archive,
}

/// Select the source family and time range, and restore the exact original archive frontier.
pub fn read_native_maintenance_semantic_catalog(
    due: &NativeMaintenanceDue,
    cycle: Option<&NativeMaintenanceDueEvidence>,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    lifecycle: &NativeRegistrationLifecycleStatus,
    journal: &mut NativeMaintenanceJournalStore,
    archive: &mut NativeMaintenancePlaintextObjectStore,
) -> Result<NativeMaintenanceSemanticCatalogOutcome, NativeMaintenanceSemanticCatalogError> {
    use NativeMaintenanceSemanticCatalogError as Error;
    use NativeMaintenanceSemanticCatalogOutcome as Outcome;
    // the source never performs due semantic work without the process's authenticated cold cycle
    let Some(NativeMaintenanceEvidenceSynchronization::Cold { cycle }) = cycle else {
        return Ok(Outcome::Blocked(
            NativeMaintenanceDueWorkReason::SemanticInputBlocked,
        ));
    };
    let Some(start) = cycle.start.as_ref() else {
        return Ok(Outcome::Blocked(
            NativeMaintenanceDueWorkReason::SemanticInputBlocked,
        ));
    };
    if cycle.state == NativeRevisionColdDrainState::ArchivedCursorAhead {
        return Ok(Outcome::Blocked(
            NativeMaintenanceDueWorkReason::SemanticInputBlocked,
        ));
    }
    if !verify_native_maintenance_due(due) || !valid_hash(&witness.witness_sha256) {
        return Err(Error::Due);
    }
    // require the same independently verified future-only epoch for every selected lifecycle entry
    if lifecycle.epoch_witness_sha256 != witness.witness_sha256
        || lifecycle.epoch_at != witness.epoch_at
    {
        return Err(Error::Lifecycle);
    }
    let wire = NativeWirePrimitives;
    let epoch = wire
        .canonical_utc_milliseconds(&witness.epoch_at)
        .ok_or(Error::Source)?;
    let active_entry = match due.mode {
        NativeMaintenanceDueMode::Daily => {
            select_native_maintenance_daily_lifecycle_entry(lifecycle, &lifecycle.snapshot_at)
                .ok_or(Error::Lifecycle)?
                .cloned()
        }
        NativeMaintenanceDueMode::Monthly => None,
    };
    let (family, from_at, to_at) = match due.mode {
        NativeMaintenanceDueMode::Monthly => (
            Some(historical_family(due.family.ok_or(Error::Due)?)),
            Some(witness.epoch_at.clone()),
            Some(due.original_cutoff_at.clone()),
        ),
        NativeMaintenanceDueMode::Daily => (
            active_entry
                .as_ref()
                .map(|entry| historical_family(entry.slot.family)),
            active_entry
                .as_ref()
                .and_then(|entry| entry.active_registration.as_ref())
                .map(|registration| registration.interval_start_at.clone()),
            active_entry
                .as_ref()
                .and_then(|entry| entry.active_registration.as_ref())
                .map(|registration| registration.target_cutoff_at.clone()),
        ),
    };
    let snapshot = field(&start.value, "servingSnapshot").ok_or(Error::Source)?;
    let Some(JsonValue::Array(entries)) = field(snapshot, "entries") else {
        return Err(Error::Source);
    };
    let receipt_sha256s = entries
        .iter()
        .map(|entry| {
            let receipt = field(entry, "receipt")?;
            let identity = text(receipt, "receiptSha256")?;
            valid_hash(&identity).then_some(identity)
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(Error::Source)?;
    let selected = NativeMaintenanceHistoricalSelection {
        family,
        from_at,
        receipt_sha256s,
        to_at,
    };
    let restored = NativeMaintenanceHistoricalCatalogJournal::new(journal)
        .restore_catalog(
            archive,
            start.clone(),
            selected,
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
        )
        .map_err(|_| Error::Archive)?;
    let NativeMaintenanceHistoricalCatalogOutcome::Ready(catalog) = restored else {
        return Ok(Outcome::Blocked(
            NativeMaintenanceDueWorkReason::HistoryUnavailable,
        ));
    };
    // independently bind every current pointer and causal time to the future-only epoch
    for member in &catalog.members {
        let ordinal = member
            .receipt
            .archiveCommitOrdinal
            .parse::<u64>()
            .map_err(|_| Error::Source)?;
        let committed = wire
            .canonical_utc_milliseconds(&member.receipt.archiveCommittedAt)
            .ok_or(Error::Source)?;
        let received = wire
            .canonical_utc_milliseconds(&member.logical_received_at)
            .ok_or(Error::Source)?;
        let document = NativeWirePrimitives
            .parse_canonical_object(&member.payload_bytes)
            .ok_or(Error::Source)?;
        if ordinal == 0
            || member.receipt.archiveCommitOrdinal != ordinal.to_string()
            || committed < epoch
            || received < epoch
            || text(&document, "contractVersion").as_deref() != Some(&member.payload_kind)
        {
            return Err(Error::Source);
        }
    }
    Ok(Outcome::Ready(Box::new(NativeMaintenanceSemanticCatalog {
        catalog,
        active_entry,
    })))
}

/// Translate the closed captured family with no unrelated value fallback.
fn historical_family(family: NativeMaintenanceCaptureFamily) -> NativeMaintenanceHistoricalFamily {
    match family {
        NativeMaintenanceCaptureFamily::Temperature => {
            NativeMaintenanceHistoricalFamily::Temperature
        }
        NativeMaintenanceCaptureFamily::Wind => NativeMaintenanceHistoricalFamily::Wind,
        NativeMaintenanceCaptureFamily::Rain => NativeMaintenanceHistoricalFamily::Rain,
    }
}
