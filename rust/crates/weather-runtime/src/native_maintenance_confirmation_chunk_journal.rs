//! Durable v3 confirmation physical-part append and deterministic full-member finalization.

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeMaintenanceCaptureFamily, NativeMaintenanceConfirmationAccessJournalError as Error,
    NativeMaintenanceJournalStore, NativeMaintenanceOwnedJournalAddition, NativeWirePrimitives,
    maintenance_canonical_json::{field, text, valid_hash},
    native_maintenance_confirmation_access_journal::{
        confirmation_record, extend, kind, lifecycle_context, registration_record,
    },
    native_maintenance_daily_evaluation_input::{
        assemble_native_confirmation_part_manifest, native_confirmation_part_follows,
        validate_native_confirmation_part,
    },
};

/// Append an exact first-absent physical part after the retained immutable local burn.
pub fn append_native_maintenance_confirmation_chunk(
    journal: &mut NativeMaintenanceJournalStore,
    family: NativeMaintenanceCaptureFamily,
    registration_sha256: &str,
    now: &str,
    chunk: &JsonValue,
) -> Result<(), Error> {
    let wire = NativeWirePrimitives;
    if !valid_hash(registration_sha256)
        || wire.canonical_utc_milliseconds(now).is_none()
        || !validate_native_confirmation_part(chunk, family)
    {
        return Err(Error::Invalid);
    }
    let identity = coordinate(chunk).ok_or(Error::Invalid)?;
    journal.mutate_checked(&mut NativeWirePrimitives, &wire, |records| {
        let registration = registration_record(records, family, registration_sha256)
            .ok_or(Error::UnknownRegistration)?;
        confirmation_record(records, registration_sha256, "confirmation_burned")
            .ok_or(Error::SnapshotUnavailable)?;
        let chunks = records
            .iter()
            .filter(|record| {
                kind(record, "confirmation_chunk")
                    && text(&record.payload, "registrationSha256").as_deref()
                        == Some(registration_sha256)
            })
            .map(|record| field(&record.payload, "chunk").ok_or(Error::Invalid))
            .collect::<Result<Vec<_>, _>>()?;
        // reuse exact immutable metadata and retain only the first absent ordered physical part
        for previous in &chunks {
            if coordinate(previous).as_deref() == Some(&identity) {
                if wire.encode_sorted_json(previous) != wire.encode_sorted_json(chunk) {
                    return Err(Error::Collision);
                }
                return Ok(Vec::new());
            }
        }
        if !native_confirmation_part_follows(chunks.last().copied(), chunk, family) {
            return Err(Error::Invalid);
        }
        let payload = extend(
            lifecycle_context(&registration.payload, "burned").ok_or(Error::Invalid)?,
            vec![("chunk", chunk.clone())],
        )
        .ok_or(Error::Invalid)?;
        Ok(vec![NativeMaintenanceOwnedJournalAddition {
            due_key: registration.due_key.clone(),
            kind: "confirmation_chunk".to_owned(),
            payload,
            record_key: format!(
                "confirmation/{}/{registration_sha256}/chunk/{identity}",
                family.wire()
            ),
            recorded_at: now.to_owned(),
        }])
    })
}

/// Finalize the actual journal-owned manifest, returning its bare scoring projection separately.
pub fn finalize_native_maintenance_confirmation_member(
    journal: &mut NativeMaintenanceJournalStore,
    family: NativeMaintenanceCaptureFamily,
    registration_sha256: &str,
    now: &str,
) -> Result<JsonValue, Error> {
    let wire = NativeWirePrimitives;
    if !valid_hash(registration_sha256) || wire.canonical_utc_milliseconds(now).is_none() {
        return Err(Error::Invalid);
    }
    let mut result = None;
    journal.mutate_checked(&mut NativeWirePrimitives, &wire, |records| {
        let registration = registration_record(records, family, registration_sha256)
            .ok_or(Error::UnknownRegistration)?;
        let burn = confirmation_record(records, registration_sha256, "confirmation_burned")
            .ok_or(Error::SnapshotUnavailable)?;
        let chunks = records
            .iter()
            .filter(|record| {
                kind(record, "confirmation_chunk")
                    && text(&record.payload, "registrationSha256").as_deref()
                        == Some(registration_sha256)
            })
            .map(|record| {
                field(&record.payload, "chunk")
                    .cloned()
                    .ok_or(Error::Invalid)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let manifest = assemble_native_confirmation_part_manifest(
            &registration.payload,
            &burn.payload,
            &chunks,
        )
        .ok_or(Error::Invalid)?;
        // preserve the actual bare scoring manifest and retain the source's complete lifecycle context
        result = Some(manifest.clone());
        if let Some(existing) =
            confirmation_record(records, registration_sha256, "confirmation_member_complete")
        {
            if text(&existing.payload, "fullMemberRootSha256")
                != text(&manifest, "fullMemberRootSha256")
            {
                return Err(Error::Collision);
            }
            return Ok(Vec::new());
        }
        let context =
            lifecycle_context(&registration.payload, "member_complete").ok_or(Error::Invalid)?;
        let (JsonValue::Object(mut fields), JsonValue::Object(manifest_fields)) =
            (context, manifest)
        else {
            return Err(Error::Invalid);
        };
        // reproduce object spread where manifest identity fields override lifecycle context
        for (key, value) in manifest_fields {
            if let Some((_, existing)) = fields.iter_mut().find(|(existing, _)| *existing == key) {
                *existing = value;
            } else {
                fields.push((key, value));
            }
        }
        Ok(vec![NativeMaintenanceOwnedJournalAddition {
            due_key: registration.due_key.clone(),
            kind: "confirmation_member_complete".to_owned(),
            payload: JsonValue::Object(fields),
            record_key: format!(
                "confirmation/{}/{registration_sha256}/complete",
                family.wire()
            ),
            recorded_at: now.to_owned(),
        }])
    })?;
    result.ok_or(Error::Invalid)
}

/// Preserve the exact logical and physical source record-key coordinate.
fn coordinate(value: &JsonValue) -> Option<String> {
    let number = |key| match field(value, key)? {
        JsonValue::Number(n) if *n >= 0.0 && *n <= usize::MAX as f64 && n.fract() == 0.0 => {
            Some(*n as usize)
        }
        _ => None,
    };
    Some(format!(
        "{}/{}",
        number("logicalChunkIndex")?,
        number("partIndex")?
    ))
}
