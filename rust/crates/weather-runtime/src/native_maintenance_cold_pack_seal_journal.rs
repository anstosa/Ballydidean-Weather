//! Journal the immutable open-pack checkpoint to sealed-graph mapping.

use std::collections::BTreeSet;
use std::fmt::{self, Display, Formatter};

use weather_contracts::JsonValue;

use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::native_maintenance_archive_journal::trusted_clock;
use crate::{
    NativeMaintenanceJournalRecord, NativeMaintenanceJournalStore,
    NativeMaintenanceJournalStoreError, NativeMaintenanceOpenPackPageRecord,
    NativeMaintenanceOwnedJournalAddition, NativeWirePrimitives,
};

const PACK_SEALED_KIND: &str = "revision_custody_pack_sealed";
const INTENT_KIND: &str = "revision_custody_intent";
const COMPLETED_V1_KIND: &str = "revision_custody_completed";
const COMPLETED_V2_KIND: &str = "revision_custody_completed_v2";
const DUE_COMPLETED_KIND: &str = "due_completed";
const MAXIMUM_PAGES: usize = 19;

/// Closed source refusal and durable-store failures for one pack mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceColdPackSealJournalError {
    Store(NativeMaintenanceJournalStoreError),
    TrustedClock,
    Invalid,
    Collision,
    GraphUnavailable,
    PageReused,
    PageUnavailable,
    CheckpointDiffers,
}

impl Display for NativeMaintenanceColdPackSealJournalError {
    /// Render one path-free source failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve the actual store diagnostic and closed state refusals
        match self {
            Self::Store(error) => Display::fmt(error, formatter),
            Self::TrustedClock => formatter.write_str("cold pack seal journal clock is untrusted"),
            Self::Invalid => formatter.write_str("cold pack seal journal input is invalid"),
            Self::Collision => formatter.write_str("cold pack seal checkpoint collides"),
            Self::GraphUnavailable => formatter.write_str("cold pack sealed graph is unavailable"),
            Self::PageReused => formatter.write_str("cold pack page is already sealed"),
            Self::PageUnavailable => formatter.write_str("cold pack page custody is unavailable"),
            Self::CheckpointDiffers => formatter.write_str("cold pack final checkpoint differs"),
        }
    }
}

impl std::error::Error for NativeMaintenanceColdPackSealJournalError {
    /// Preserve only an actual durable-store source.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // semantic source refusals have no lower-level cause
        match self {
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceColdPackSealJournalError {
    /// Retain the actual native store failure.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Store(error)
    }
}

/// Record one verified sealed graph for the exact ordered open-pack pages.
pub fn record_native_maintenance_cold_pack_seal(
    store: &mut NativeMaintenanceJournalStore,
    checkpoint_sha256: &str,
    graph_manifest_sha256: &str,
    now: &str,
    records: &[NativeMaintenanceOpenPackPageRecord],
) -> Result<(), NativeMaintenanceColdPackSealJournalError> {
    use NativeMaintenanceColdPackSealJournalError as Error;

    // validate every caller-owned coordinate before taking the durable journal lock
    if !valid_hash(checkpoint_sha256)
        || !valid_hash(graph_manifest_sha256)
        || !(1..=MAXIMUM_PAGES).contains(&records.len())
        || records.iter().any(|record| !valid_page(record))
    {
        return Err(Error::Invalid);
    }
    let pages = records.iter().map(page_value).collect::<Vec<_>>();
    let expected_payload = object(vec![
        ("checkpointSha256", string(checkpoint_sha256)),
        ("graphManifestSha256", string(graph_manifest_sha256)),
        ("pages", JsonValue::Array(pages)),
    ]);
    let due_key = format!("archive/revision-custody-pack/{checkpoint_sha256}");
    store.mutate_checked(
        &mut NativeWirePrimitives::new(),
        &NativeWirePrimitives::new(),
        |journal| {
            trusted_clock(journal, now, false).map_err(|_| Error::TrustedClock)?;
            let existing = journal.iter().find(|record| {
                is_kind(record, PACK_SEALED_KIND)
                    && text(&record.payload, "checkpointSha256").as_deref()
                        == Some(checkpoint_sha256)
            });
            // converge only one exact immutable checkpoint-to-graph mapping
            if let Some(existing) = existing {
                // reject checkpoint identity reuse with another graph or page set
                if existing.payload != expected_payload {
                    return Err(Error::Collision);
                }
                return Ok(Vec::new());
            }
            let publication = journal.iter().find(|record| {
                is_kind(record, DUE_COMPLETED_KIND)
                    && record.due_key.as_deref() == Some(due_key.as_str())
            });
            // require the one verified archive output before mapping any page
            if publication
                .and_then(|record| text(&record.payload, "outputSha256"))
                .as_deref()
                != Some(graph_manifest_sha256)
            {
                return Err(Error::GraphUnavailable);
            }
            let mut seen = BTreeSet::new();
            // bind each open-pack page to completed custody in append order
            for page in records {
                if !seen.insert(page.page_sha256.as_str())
                    || page_already_sealed(journal, &page.page_sha256)?
                {
                    return Err(Error::PageReused);
                }
                let intent =
                    find_intent(journal, &page.page_sha256).ok_or(Error::PageUnavailable)?;
                let completed = journal.iter().any(|record| {
                    completion_page_sha256(record).as_deref() == Some(page.page_sha256.as_str())
                });
                let request = field(&intent.payload, "request").ok_or(Error::PageUnavailable)?;
                // preserve every member and start root from the private checkpoint
                if !completed
                    || field(request, "custodyCheckpointSha256").is_none()
                    || text(request, "memberRootSha256").as_deref()
                        != Some(page.member_root_sha256.as_str())
                    || nullable_text(request, "startMemberSha256")?.as_deref()
                        != page.start_member_sha256.as_deref()
                    || text(request, "startSha256").as_deref() != Some(page.start_sha256.as_str())
                {
                    return Err(Error::PageUnavailable);
                }
            }
            let last = records.last().ok_or(Error::Invalid)?;
            let last_intent =
                find_intent(journal, &last.page_sha256).ok_or(Error::PageUnavailable)?;
            let last_request =
                field(&last_intent.payload, "request").ok_or(Error::PageUnavailable)?;
            // bind the sealed checkpoint to the final acknowledged page prefix
            if text(last_request, "custodyCheckpointSha256").as_deref() != Some(checkpoint_sha256) {
                return Err(Error::CheckpointDiffers);
            }
            Ok(vec![NativeMaintenanceOwnedJournalAddition {
                due_key: None,
                kind: PACK_SEALED_KIND.to_owned(),
                payload: expected_payload,
                record_key: format!("revision/custody/pack/{checkpoint_sha256}"),
                recorded_at: now.to_owned(),
            }])
        },
    )
}

/// Validate one source page projection independently from storage offsets.
fn valid_page(record: &NativeMaintenanceOpenPackPageRecord) -> bool {
    valid_hash(&record.member_root_sha256)
        && valid_hash(&record.page_sha256)
        && valid_hash(&record.start_sha256)
        && record.start_member_sha256.as_deref().is_none_or(valid_hash)
}

/// Find the first immutable custody intent for one page.
fn find_intent<'a>(
    journal: &'a [NativeMaintenanceJournalRecord],
    page_sha256: &str,
) -> Option<&'a NativeMaintenanceJournalRecord> {
    journal.iter().find(|record| {
        is_kind(record, INTENT_KIND)
            && field(&record.payload, "request")
                .and_then(|request| text(request, "pageSha256"))
                .as_deref()
                == Some(page_sha256)
    })
}

/// Project either retained custody completion version onto its page identity.
fn completion_page_sha256(record: &NativeMaintenanceJournalRecord) -> Option<String> {
    // select only the two source completion payload shapes
    if is_kind(record, COMPLETED_V1_KIND) {
        return text(&record.payload, "pageSha256");
    }
    if is_kind(record, COMPLETED_V2_KIND) {
        return field(&record.payload, "acknowledgement")
            .and_then(|value| text(value, "pageSha256"));
    }
    None
}

/// Reject a page already mapped into any earlier immutable pack.
fn page_already_sealed(
    journal: &[NativeMaintenanceJournalRecord],
    page_sha256: &str,
) -> Result<bool, NativeMaintenanceColdPackSealJournalError> {
    use NativeMaintenanceColdPackSealJournalError::Invalid;

    // scan every retained pack and its exact page projection
    for record in journal
        .iter()
        .filter(|record| is_kind(record, PACK_SEALED_KIND))
    {
        let JsonValue::Array(pages) = field(&record.payload, "pages").ok_or(Invalid)? else {
            return Err(Invalid);
        };
        // stop at the first prior immutable mapping
        if pages
            .iter()
            .any(|page| text(page, "pageSha256").as_deref() == Some(page_sha256))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Decode one required nullable source string.
fn nullable_text(
    value: &JsonValue,
    key: &str,
) -> Result<Option<String>, NativeMaintenanceColdPackSealJournalError> {
    use NativeMaintenanceColdPackSealJournalError::Invalid;

    // preserve strict null independently from a string identity
    match field(value, key).ok_or(Invalid)? {
        JsonValue::Null => Ok(None),
        JsonValue::String(units) => String::from_utf16(units).map(Some).map_err(|_| Invalid),
        _ => Err(Invalid),
    }
}

/// Project only the four source journal fields from one durable open-pack record.
fn page_value(record: &NativeMaintenanceOpenPackPageRecord) -> JsonValue {
    object(vec![
        ("memberRootSha256", string(&record.member_root_sha256)),
        ("pageSha256", string(&record.page_sha256)),
        (
            "startMemberSha256",
            record
                .start_member_sha256
                .as_deref()
                .map_or(JsonValue::Null, string),
        ),
        ("startSha256", string(&record.start_sha256)),
    ])
}

/// Compare one verified journal event discriminator.
fn is_kind(record: &NativeMaintenanceJournalRecord, expected: &str) -> bool {
    matches!(&record.kind, JsonValue::String(units) if units.iter().copied().eq(expected.encode_utf16()))
}

/// Build one canonical ordinary object in explicit source order.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one source JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_cold_pack_seal_journal_test.rs"]
mod tests;
