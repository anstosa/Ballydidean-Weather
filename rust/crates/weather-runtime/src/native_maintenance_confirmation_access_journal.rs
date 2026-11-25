//! Original blinded confirmation snapshot, immutable burn and custody acknowledgement journal effects.

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeMaintenanceCaptureFamily, NativeMaintenanceJournalRecord, NativeMaintenanceJournalStore,
    NativeMaintenanceJournalStoreError, NativeMaintenanceOwnedJournalAddition,
    NativeWirePrimitives,
    maintenance_canonical_json::{field, text, valid_hash},
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
};

const REGISTERED: &str = "confirmation_registered";
const SNAPSHOT: &str = "confirmation_revision_snapshot";
const BURNED: &str = "confirmation_burned";
const VERSION: &str = "forecast-adjustment-lifecycle-ledger/v2";
const MAX_ENTRY_COUNT: u64 = 9_500_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceConfirmationAccessJournalError {
    Invalid,
    UnknownRegistration,
    SnapshotUnavailable,
    Premature,
    Collision,
    Store(NativeMaintenanceJournalStoreError),
}
impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceConfirmationAccessJournalError {
    /// Retain the owning append-only journal failure.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Store(error)
    }
}

/// Compact exact blinded snapshot request derived from a source confirmation plan.
pub struct NativeMaintenanceRevisionSnapshotRequest<'a> {
    pub entry_count: u64,
    pub expected_key_set_sha256: &'a str,
    pub family: NativeMaintenanceCaptureFamily,
    pub now: &'a str,
    pub registration_sha256: &'a str,
    pub revision_catalog_watermark_sha256: &'a str,
    pub snapshot_root_sha256: &'a str,
    pub target_cutoff_at: &'a str,
}

/// Append one immutable value-blind confirmation snapshot with source idempotence.
pub fn record_native_maintenance_revision_snapshot(
    journal: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceRevisionSnapshotRequest<'_>,
) -> Result<(), NativeMaintenanceConfirmationAccessJournalError> {
    use NativeMaintenanceConfirmationAccessJournalError as Error;
    let wire = NativeWirePrimitives;
    if input.entry_count > MAX_ENTRY_COUNT
        || ![
            input.expected_key_set_sha256,
            input.registration_sha256,
            input.revision_catalog_watermark_sha256,
            input.snapshot_root_sha256,
        ]
        .into_iter()
        .all(valid_hash)
        || wire.canonical_utc_milliseconds(input.now).is_none()
        || wire
            .canonical_utc_milliseconds(input.target_cutoff_at)
            .is_none()
    {
        return Err(Error::Invalid);
    }
    journal.mutate_checked(&mut NativeWirePrimitives, &wire, |records| {
        let registration = registration_record(records, input.family, input.registration_sha256)
            .ok_or(Error::UnknownRegistration)?;
        let context =
            lifecycle_context(&registration.payload, "snapshot_frozen").ok_or(Error::Invalid)?;
        let payload = extend(
            context,
            vec![
                ("entryCount", JsonValue::Number(input.entry_count as f64)),
                (
                    "expectedKeySetSha256",
                    string(input.expected_key_set_sha256),
                ),
                (
                    "revisionCatalogWatermarkSha256",
                    string(input.revision_catalog_watermark_sha256),
                ),
                ("snapshotRootSha256", string(input.snapshot_root_sha256)),
                ("targetCutoffAt", string(input.target_cutoff_at)),
            ],
        )
        .ok_or(Error::Invalid)?;
        let record_key = format!(
            "confirmation/{}/{}/confirmation_revision_snapshot",
            input.family.wire(),
            input.registration_sha256
        );
        // reproduce the source exact-key immutable mutation instead of duplicating a replay
        unique_addition(
            records,
            NativeMaintenanceOwnedJournalAddition {
                due_key: registration.due_key.clone(),
                kind: SNAPSHOT.to_owned(),
                payload,
                record_key,
                recorded_at: input.now.to_owned(),
            },
        )
    })
}

/// Burn one confirmed member only after its genuine source snapshot and terminal access time.
pub fn burn_native_maintenance_confirmation(
    journal: &mut NativeMaintenanceJournalStore,
    family: NativeMaintenanceCaptureFamily,
    registration_sha256: &str,
    now: &str,
) -> Result<JsonValue, NativeMaintenanceConfirmationAccessJournalError> {
    use NativeMaintenanceConfirmationAccessJournalError as Error;
    let wire = NativeWirePrimitives;
    let now_ms = wire.canonical_utc_milliseconds(now).ok_or(Error::Invalid)?;
    if !valid_hash(registration_sha256) {
        return Err(Error::Invalid);
    }
    let mut result = None;
    journal.mutate_checked(&mut NativeWirePrimitives, &wire, |records| {
        let registration = registration_record(records, family, registration_sha256)
            .ok_or(Error::UnknownRegistration)?;
        let snapshot = confirmation_record(records, registration_sha256, SNAPSHOT)
            .ok_or(Error::SnapshotUnavailable)?;
        let terminal_at = text(&registration.payload, "terminalAccessAt").ok_or(Error::Invalid)?;
        // keep source terminal access permanently unavailable before its registered clock
        if now_ms
            < wire
                .canonical_utc_milliseconds(&terminal_at)
                .ok_or(Error::Invalid)?
        {
            return Err(Error::Premature);
        }
        // preserve the exact retained access hash and clock on every restart
        if let Some(existing) = confirmation_record(records, registration_sha256, BURNED) {
            result = Some(existing.payload.clone());
            return Ok(Vec::new());
        }
        let context = lifecycle_context(&registration.payload, "burned").ok_or(Error::Invalid)?;
        let payload = extend(
            context,
            vec![
                (
                    "targetComparatorSnapshotRootSha256",
                    field(&snapshot.payload, "snapshotRootSha256")
                        .ok_or(Error::Invalid)?
                        .clone(),
                ),
                (
                    "revisionCatalogWatermarkSha256",
                    field(&snapshot.payload, "revisionCatalogWatermarkSha256")
                        .ok_or(Error::Invalid)?
                        .clone(),
                ),
                (
                    "expectedKeySetSha256",
                    field(&snapshot.payload, "expectedKeySetSha256")
                        .ok_or(Error::Invalid)?
                        .clone(),
                ),
                (
                    "targetCutoffAt",
                    field(&snapshot.payload, "targetCutoffAt")
                        .ok_or(Error::Invalid)?
                        .clone(),
                ),
                ("accessedAt", string(now)),
            ],
        )
        .ok_or(Error::Invalid)?;
        let access_sha256 = digest_value(&payload).ok_or(Error::Invalid)?;
        let payload = extend(payload, vec![("accessSha256", string(&access_sha256))])
            .ok_or(Error::Invalid)?;
        result = Some(payload.clone());
        Ok(vec![NativeMaintenanceOwnedJournalAddition {
            due_key: registration.due_key.clone(),
            kind: BURNED.to_owned(),
            payload,
            record_key: format!(
                "confirmation/{}/{registration_sha256}/burned",
                family.wire()
            ),
            recorded_at: now.to_owned(),
        }])
    })?;
    result.ok_or(Error::Invalid)
}

/// Latest exact root-owned custody acknowledgement and coherent local journal head.
pub struct NativeMaintenanceConfirmationCustody {
    pub acknowledgement: JsonValue,
    pub journal_head_sha256: String,
}

/// Expose only the latest source v2 custody acknowledgement after the local immutable burn.
pub fn read_native_maintenance_confirmation_custody(
    journal: &mut NativeMaintenanceJournalStore,
) -> Result<
    Option<NativeMaintenanceConfirmationCustody>,
    NativeMaintenanceConfirmationAccessJournalError,
> {
    let records = journal.read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
    let Some(record) = records
        .iter()
        .rev()
        .find(|record| kind(record, "revision_custody_completed_v2"))
    else {
        return Ok(None);
    };
    let acknowledgement = field(&record.payload, "acknowledgement")
        .ok_or(NativeMaintenanceConfirmationAccessJournalError::Invalid)?
        .clone();
    let journal_head_sha256 = records
        .last()
        .map(|record| record.record_sha256.clone())
        .ok_or(NativeMaintenanceConfirmationAccessJournalError::Invalid)?;
    Ok(Some(NativeMaintenanceConfirmationCustody {
        acknowledgement,
        journal_head_sha256,
    }))
}

/// Build the closed predecessor lifecycle-v2 context from an exact retained registration.
pub(crate) fn lifecycle_context(registration: &JsonValue, access_state: &str) -> Option<JsonValue> {
    const FIELDS: &[&str] = &[
        "family",
        "candidateKind",
        "candidateSha256",
        "cohortLineageSha256",
        "sourceLineageSha256",
        "reservedKeySha256",
        "intervalStartLocalDate",
        "intervalEndExclusiveLocalDate",
        "firstTargetAt",
        "terminalAccessAt",
        "gateManifestSha256",
        "inputHeadSha256",
        "registrationSha256",
    ];
    let mut fields = vec![("contractVersion", string(VERSION))];
    for key in FIELDS {
        fields.push((key, field(registration, key)?.clone()));
    }
    fields.extend([
        ("candidateReportSha256", JsonValue::Null),
        ("actionIdentitySha256", JsonValue::Null),
        ("accessState", string(access_state)),
        ("actionState", string("none")),
    ]);
    Some(object(fields))
}

/// Find the exact registered family and confirmation address.
pub(crate) fn registration_record<'a>(
    records: &'a [NativeMaintenanceJournalRecord],
    family: NativeMaintenanceCaptureFamily,
    registration: &str,
) -> Option<&'a NativeMaintenanceJournalRecord> {
    records.iter().find(|record| {
        kind(record, REGISTERED)
            && text(&record.payload, "family").as_deref() == Some(family.wire())
            && text(&record.payload, "registrationSha256").as_deref() == Some(registration)
    })
}

/// Find one exact retained confirmation event with its original registration identity.
pub(crate) fn confirmation_record<'a>(
    records: &'a [NativeMaintenanceJournalRecord],
    registration: &str,
    event: &str,
) -> Option<&'a NativeMaintenanceJournalRecord> {
    records.iter().find(|record| {
        kind(record, event)
            && text(&record.payload, "registrationSha256").as_deref() == Some(registration)
    })
}

/// Preserve the source logical record key, event identity and canonical payload on an idempotent replay.
fn unique_addition(
    records: &[NativeMaintenanceJournalRecord],
    addition: NativeMaintenanceOwnedJournalAddition,
) -> Result<
    Vec<NativeMaintenanceOwnedJournalAddition>,
    NativeMaintenanceConfirmationAccessJournalError,
> {
    use NativeMaintenanceConfirmationAccessJournalError as Error;
    if let Some(existing) = records
        .iter()
        .find(|record| record.record_key == addition.record_key)
    {
        if !kind(existing, &addition.kind)
            || existing.due_key != addition.due_key
            || digest_value(&addition.payload).as_deref() != Some(existing.payload_sha256.as_str())
        {
            return Err(Error::Collision);
        }
        return Ok(Vec::new());
    }
    Ok(vec![addition])
}

/// Extend a closed source context by a disjoint event-specific field set.
pub(crate) fn extend(value: JsonValue, fields: Vec<(&str, JsonValue)>) -> Option<JsonValue> {
    let JsonValue::Object(mut value) = value else {
        return None;
    };
    value.extend(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value)),
    );
    Some(JsonValue::Object(value))
}

/// Read one retained exact journal kind.
pub(crate) fn kind(record: &NativeMaintenanceJournalRecord, expected: &str) -> bool {
    matches!(&record.kind, JsonValue::String(units) if units.iter().copied().eq(expected.encode_utf16()))
}

/// Hash a source canonical value with its original final LF.
fn digest_value(value: &JsonValue) -> Option<String> {
    let wire = NativeWirePrimitives;
    Some(format_digest(wire.sha256(&wire.encode_sorted_json(value)?)))
}
