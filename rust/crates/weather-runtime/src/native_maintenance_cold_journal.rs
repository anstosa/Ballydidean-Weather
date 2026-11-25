//! Concrete revision cold-custody journal over the verified native store.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::future::{Future, ready};
use std::pin::Pin;

use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_archive_journal::trusted_clock;
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceJournalRecord, NativeMaintenanceJournalStore,
    NativeMaintenanceJournalStoreError, NativeMaintenanceOwnedJournalAddition,
    NativeRevisionColdCursor, NativeRevisionColdCursorAdvance, NativeRevisionColdCustodyIntent,
    NativeRevisionColdCustodyRequest, NativeRevisionColdJournalPort, NativeRevisionColdPageRequest,
    NativeWirePrimitives, VerifiedNativeRevisionColdCustodyAcknowledgementV2,
};

const INTENT_KIND: &str = "revision_custody_intent";
const CURSOR_KIND: &str = "revision_cursor_advanced";
const COMPLETED_V1_KIND: &str = "revision_custody_completed";
const COMPLETED_V2_KIND: &str = "revision_custody_completed_v2";
const GENESIS_KIND: &str = "lifecycle_genesis";
const CUSTODY_ACK_MAXIMUM_BYTES: usize = 16 * 1_024;

/// Closed durable-store and source replay failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceColdJournalError {
    Store(NativeMaintenanceJournalStoreError),
    TrustedClock,
    State,
    Collision,
    Codec,
}

impl Display for NativeMaintenanceColdJournalError {
    /// Render one bounded journal owner diagnostic.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve the actual store cause and closed semantic categories
        match self {
            Self::Store(error) => Display::fmt(error, formatter),
            Self::TrustedClock => formatter.write_str("revision cold journal clock is untrusted"),
            Self::State => formatter.write_str("revision cold journal state is invalid"),
            Self::Collision => formatter.write_str("revision cold journal record collides"),
            Self::Codec => formatter.write_str("revision cold journal serialization failed"),
        }
    }
}

impl std::error::Error for NativeMaintenanceColdJournalError {
    /// Preserve only an actual durable-store source.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // semantic replay failures have no lower-level cause
        match self {
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceColdJournalError {
    /// Retain the concrete store operation failure.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Store(error)
    }
}

/// Borrow the one controller-owned durable journal store.
pub struct NativeMaintenanceColdJournal<'a> {
    store: &'a mut NativeMaintenanceJournalStore,
}

impl<'a> NativeMaintenanceColdJournal<'a> {
    /// Bind the already initialized native store without reading it.
    pub const fn new(store: &'a mut NativeMaintenanceJournalStore) -> Self {
        Self { store }
    }

    /// Read and derive the authenticated genesis or latest successor cursor.
    fn read_cursor(
        &mut self,
    ) -> Result<NativeRevisionColdCursor, NativeMaintenanceColdJournalError> {
        let records = self.store.read_verified(
            &mut NativeWirePrimitives::new(),
            &NativeWirePrimitives::new(),
        )?;
        derive_cursor(&records)
    }

    /// Read the sole unfinished verified custody intent.
    fn read_pending(
        &mut self,
    ) -> Result<Option<NativeRevisionColdCustodyIntent>, NativeMaintenanceColdJournalError> {
        let records = self.store.read_verified(
            &mut NativeWirePrimitives::new(),
            &NativeWirePrimitives::new(),
        )?;
        derive_pending(&records)
    }

    /// Persist one exact archive-backed custody intent under the journal lock.
    fn record_intent(
        &mut self,
        intent: &NativeRevisionColdCustodyIntent,
        now: &str,
    ) -> Result<NativeRevisionColdCustodyIntent, NativeMaintenanceColdJournalError> {
        use NativeMaintenanceColdJournalError as Error;

        validate_intent(intent)?;
        let payload = intent_value(intent);
        let page_sha256 = intent.request.page_sha256.clone();
        self.store.mutate_checked(
            &mut NativeWirePrimitives::new(),
            &NativeWirePrimitives::new(),
            |records| {
                trusted_clock(records, now, false).map_err(|_| Error::TrustedClock)?;
                // find the source collision key inside the nested request
                let existing = records.iter().find(|record| {
                    is_kind(record, INTENT_KIND)
                        && field(&record.payload, "request")
                            .and_then(|request| text(request, "pageSha256"))
                            .as_deref()
                            == Some(page_sha256.as_str())
                });
                // converge only a byte-equivalent immutable page intent
                if let Some(existing) = existing {
                    // reject changed content under one immutable page key
                    if existing.payload != payload {
                        return Err(Error::Collision);
                    }
                    return Ok(Vec::new());
                }
                let pending = derive_pending(records)?;
                let cursor = derive_cursor(records)?;
                // serialize custody and bind it to the retained predecessor cursor
                if pending.is_some()
                    || cursor.archive_commit_ordinal
                        != intent.request.page.after_archive_commit_ordinal
                    || cursor.frontier_sha256 != intent.request.page.after_frontier_sha256
                {
                    return Err(Error::State);
                }
                Ok(vec![addition(
                    INTENT_KIND,
                    format!("revision/custody/intent/{page_sha256}"),
                    now,
                    payload,
                )])
            },
        )?;
        Ok(intent.clone())
    }

    /// Advance one exact archived cursor through compare-and-swap replay.
    fn advance_cursor(
        &mut self,
        advance: &NativeRevisionColdCursorAdvance,
    ) -> Result<NativeRevisionColdCursor, NativeMaintenanceColdJournalError> {
        use NativeMaintenanceColdJournalError as Error;

        validate_advance(advance)?;
        let payload = advance_value(advance);
        self.store.mutate_checked(
            &mut NativeWirePrimitives::new(),
            &NativeWirePrimitives::new(),
            |records| {
                trusted_clock(records, &advance.now, false).map_err(|_| Error::TrustedClock)?;
                let existing = records.iter().find(|record| {
                    is_kind(record, CURSOR_KIND)
                        && text(&record.payload, "pageSha256").as_deref()
                            == Some(advance.page_sha256.as_str())
                });
                // reject page identity reuse with another frontier transition
                if let Some(existing) = existing {
                    // require every transition coordinate to remain immutable
                    if existing.payload != payload {
                        return Err(Error::Collision);
                    }
                    return Ok(Vec::new());
                }
                let pending = derive_pending(records)?.ok_or(Error::State)?;
                let cursor = derive_cursor(records)?;
                // require the exact intent proof, successor, and predecessor cursor
                if pending.request.page_sha256 != advance.page_sha256
                    || pending.request.custody_checkpoint_sha256
                        != advance.custody_checkpoint_sha256
                    || pending.request.page.start_sha256 != advance.start_sha256
                    || pending.next_archive_commit_ordinal != advance.next_archive_commit_ordinal
                    || pending.next_frontier_sha256 != advance.next_frontier_sha256
                    || cursor.archive_commit_ordinal != advance.after_archive_commit_ordinal
                    || cursor.frontier_sha256 != advance.after_frontier_sha256
                    || ordinal(&advance.next_archive_commit_ordinal)?
                        <= ordinal(&advance.after_archive_commit_ordinal)?
                {
                    return Err(Error::State);
                }
                Ok(vec![addition(
                    CURSOR_KIND,
                    format!(
                        "revision/cursor/{}/{}",
                        advance.next_archive_commit_ordinal, advance.next_frontier_sha256
                    ),
                    &advance.now,
                    payload,
                )])
            },
        )?;
        Ok(NativeRevisionColdCursor {
            archive_commit_ordinal: advance.next_archive_commit_ordinal.clone(),
            frontier_sha256: advance.next_frontier_sha256.clone(),
        })
    }

    /// Close one packed intent only after its verified successor is durable.
    fn complete_v2(
        &mut self,
        acknowledgement: &VerifiedNativeRevisionColdCustodyAcknowledgementV2,
        now: &str,
    ) -> Result<(), NativeMaintenanceColdJournalError> {
        use NativeMaintenanceColdJournalError as Error;

        validate_acknowledgement(acknowledgement)?;
        let payload = object(vec![("acknowledgement", acknowledgement.value.clone())]);
        let page_sha256 = acknowledgement.request.page_sha256.clone();
        self.store.mutate_checked(
            &mut NativeWirePrimitives::new(),
            &NativeWirePrimitives::new(),
            |records| {
                trusted_clock(records, now, false).map_err(|_| Error::TrustedClock)?;
                let existing = records.iter().find(|record| {
                    is_kind(record, COMPLETED_V2_KIND)
                        && field(&record.payload, "acknowledgement")
                            .and_then(|value| text(value, "pageSha256"))
                            .as_deref()
                            == Some(page_sha256.as_str())
                });
                // converge only the exact retained server acknowledgement
                if let Some(existing) = existing {
                    // reject a different acknowledgement for the same page
                    if existing.payload != payload {
                        return Err(Error::Collision);
                    }
                    return Ok(Vec::new());
                }
                let pending = derive_pending(records)?.ok_or(Error::State)?;
                let cursor = derive_cursor(records)?;
                // complete only the exact pending request after durable successor visibility
                if pending.request != acknowledgement.request
                    || pending.next_archive_commit_ordinal
                        != acknowledgement.next_archive_commit_ordinal
                    || pending.next_frontier_sha256 != acknowledgement.next_frontier_sha256
                    || cursor.archive_commit_ordinal != pending.next_archive_commit_ordinal
                    || cursor.frontier_sha256 != pending.next_frontier_sha256
                {
                    return Err(Error::State);
                }
                Ok(vec![addition(
                    COMPLETED_V2_KIND,
                    format!("revision/custody/completed-v2/{page_sha256}"),
                    now,
                    payload,
                )])
            },
        )
    }
}

impl NativeRevisionColdJournalPort for NativeMaintenanceColdJournal<'_> {
    type Error = NativeMaintenanceColdJournalError;

    /// Read the current authenticated cold cursor.
    fn read_revision_cursor(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdCursor, Self::Error>> + '_>> {
        Box::pin(ready(self.read_cursor()))
    }

    /// Read the sole unfinished custody intent.
    fn read_pending_revision_custody(
        &mut self,
    ) -> Pin<
        Box<dyn Future<Output = Result<Option<NativeRevisionColdCustodyIntent>, Self::Error>> + '_>,
    > {
        Box::pin(ready(self.read_pending()))
    }

    /// Persist and return the exact page-bound intent.
    fn record_revision_custody_intent<'a>(
        &'a mut self,
        intent: &'a NativeRevisionColdCustodyIntent,
        now: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdCustodyIntent, Self::Error>> + 'a>>
    {
        Box::pin(ready(self.record_intent(intent, now)))
    }

    /// Compare and swap the exact archived successor cursor.
    fn advance_revision_cursor<'a>(
        &'a mut self,
        advance: &'a NativeRevisionColdCursorAdvance,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdCursor, Self::Error>> + 'a>> {
        Box::pin(ready(self.advance_cursor(advance)))
    }

    /// Retain the complete verified v2 acknowledgement.
    fn complete_revision_custody_v2<'a>(
        &'a mut self,
        acknowledgement: &'a VerifiedNativeRevisionColdCustodyAcknowledgementV2,
        now: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>> {
        Box::pin(ready(self.complete_v2(acknowledgement, now)))
    }
}

/// Derive the latest durable successor or authenticated future-only genesis cursor.
fn derive_cursor(
    records: &[NativeMaintenanceJournalRecord],
) -> Result<NativeRevisionColdCursor, NativeMaintenanceColdJournalError> {
    use NativeMaintenanceColdJournalError::State;

    // prefer the most recent successfully archived page cursor
    if let Some(record) = records
        .iter()
        .rev()
        .find(|record| is_kind(record, CURSOR_KIND))
    {
        let cursor = NativeRevisionColdCursor {
            archive_commit_ordinal: text(&record.payload, "nextArchiveCommitOrdinal")
                .ok_or(State)?,
            frontier_sha256: text(&record.payload, "nextFrontierSha256").ok_or(State)?,
        };
        validate_cursor(&cursor)?;
        return Ok(cursor);
    }
    let genesis = records.iter().find(|record| {
        is_kind(record, GENESIS_KIND)
            && text(&record.payload, "sourceKind").as_deref()
                == Some("future_only_verified_capture_epoch")
    });
    let witness = genesis
        .and_then(|record| field(&record.payload, "witness"))
        .ok_or(State)?;
    let cursor = NativeRevisionColdCursor {
        archive_commit_ordinal: text(witness, "archiveCommitOrdinal").ok_or(State)?,
        frontier_sha256: text(witness, "catalogFrontierSha256").ok_or(State)?,
    };
    validate_cursor(&cursor)?;
    Ok(cursor)
}

/// Replay the two completion versions and return the sole pending intent.
fn derive_pending(
    records: &[NativeMaintenanceJournalRecord],
) -> Result<Option<NativeRevisionColdCustodyIntent>, NativeMaintenanceColdJournalError> {
    use NativeMaintenanceColdJournalError::State;

    let mut intents = BTreeMap::new();
    // replay only the closed custody intent and completion transitions
    for record in records {
        // retain every immutable page intent until its completion appears
        if is_kind(record, INTENT_KIND) {
            let intent = parse_intent(&record.payload)?;
            intents.insert(intent.request.page_sha256.clone(), intent);
        }
        // close only the exact acknowledged page for either retained version
        if is_kind(record, COMPLETED_V1_KIND) {
            let page_sha256 = text(&record.payload, "pageSha256").ok_or(State)?;
            intents.remove(&page_sha256);
        } else if is_kind(record, COMPLETED_V2_KIND) {
            let page_sha256 = field(&record.payload, "acknowledgement")
                .and_then(|value| text(value, "pageSha256"))
                .ok_or(State)?;
            intents.remove(&page_sha256);
        }
    }
    // reject a history that claims concurrent page custody
    if intents.len() > 1 {
        return Err(State);
    }
    Ok(intents.into_values().next())
}

/// Parse one exact retained custody intent payload.
fn parse_intent(
    value: &JsonValue,
) -> Result<NativeRevisionColdCustodyIntent, NativeMaintenanceColdJournalError> {
    use NativeMaintenanceColdJournalError::State;

    if !exact_keys(
        value,
        &["nextArchiveCommitOrdinal", "nextFrontierSha256", "request"],
    ) {
        return Err(State);
    }
    let intent = NativeRevisionColdCustodyIntent {
        next_archive_commit_ordinal: text(value, "nextArchiveCommitOrdinal").ok_or(State)?,
        next_frontier_sha256: text(value, "nextFrontierSha256").ok_or(State)?,
        request: parse_request(field(value, "request").ok_or(State)?)?,
    };
    validate_intent(&intent)?;
    Ok(intent)
}

/// Parse the source's exact ten-coordinate packed custody request.
fn parse_request(
    value: &JsonValue,
) -> Result<NativeRevisionColdCustodyRequest, NativeMaintenanceColdJournalError> {
    use NativeMaintenanceColdJournalError::State;

    if !exact_keys(
        value,
        &[
            "afterArchiveCommitOrdinal",
            "afterFrontierSha256",
            "custodyCheckpointSha256",
            "memberRootSha256",
            "pageSha256",
            "previousPageSha256",
            "startMemberSha256",
            "startSha256",
            "watermarkArchiveCommitOrdinal",
            "watermarkFrontierSha256",
        ],
    ) {
        return Err(State);
    }
    let request = NativeRevisionColdCustodyRequest {
        page: NativeRevisionColdPageRequest {
            after_archive_commit_ordinal: text(value, "afterArchiveCommitOrdinal").ok_or(State)?,
            after_frontier_sha256: text(value, "afterFrontierSha256").ok_or(State)?,
            previous_page_sha256: text(value, "previousPageSha256").ok_or(State)?,
            start_sha256: text(value, "startSha256").ok_or(State)?,
            watermark_archive_commit_ordinal: text(value, "watermarkArchiveCommitOrdinal")
                .ok_or(State)?,
            watermark_frontier_sha256: text(value, "watermarkFrontierSha256").ok_or(State)?,
        },
        custody_checkpoint_sha256: text(value, "custodyCheckpointSha256").ok_or(State)?,
        member_root_sha256: text(value, "memberRootSha256").ok_or(State)?,
        page_sha256: text(value, "pageSha256").ok_or(State)?,
        start_member_sha256: text(value, "startMemberSha256").ok_or(State)?,
    };
    validate_request(&request)?;
    Ok(request)
}

/// Validate one intent independently of its durable predecessor cursor.
fn validate_intent(
    intent: &NativeRevisionColdCustodyIntent,
) -> Result<(), NativeMaintenanceColdJournalError> {
    use NativeMaintenanceColdJournalError::State;

    validate_request(&intent.request)?;
    let after = ordinal(&intent.request.page.after_archive_commit_ordinal)?;
    let next = ordinal(&intent.next_archive_commit_ordinal)?;
    let watermark = ordinal(&intent.request.page.watermark_archive_commit_ordinal)?;
    // retain only one strict successor within the immutable watermark
    if next <= after || next > watermark || !valid_hash(&intent.next_frontier_sha256) {
        return Err(State);
    }
    Ok(())
}

/// Validate all packed custody request coordinates.
fn validate_request(
    request: &NativeRevisionColdCustodyRequest,
) -> Result<(), NativeMaintenanceColdJournalError> {
    use NativeMaintenanceColdJournalError::State;

    let after = ordinal(&request.page.after_archive_commit_ordinal)?;
    let watermark = ordinal(&request.page.watermark_archive_commit_ordinal)?;
    // bind every content identity and prohibit a cursor beyond its watermark
    if after > watermark
        || [
            request.page.after_frontier_sha256.as_str(),
            request.page.previous_page_sha256.as_str(),
            request.page.start_sha256.as_str(),
            request.page.watermark_frontier_sha256.as_str(),
            request.custody_checkpoint_sha256.as_str(),
            request.member_root_sha256.as_str(),
            request.page_sha256.as_str(),
            request.start_member_sha256.as_str(),
        ]
        .iter()
        .any(|value| !valid_hash(value))
    {
        return Err(State);
    }
    Ok(())
}

/// Validate one cursor transition argument before locking the store.
fn validate_advance(
    advance: &NativeRevisionColdCursorAdvance,
) -> Result<(), NativeMaintenanceColdJournalError> {
    use NativeMaintenanceColdJournalError::State;

    let after = ordinal(&advance.after_archive_commit_ordinal)?;
    let next = ordinal(&advance.next_archive_commit_ordinal)?;
    // require one strict addressed successor and a canonical source clock
    if next <= after
        || [
            advance.after_frontier_sha256.as_str(),
            advance.custody_checkpoint_sha256.as_str(),
            advance.next_frontier_sha256.as_str(),
            advance.page_sha256.as_str(),
            advance.start_sha256.as_str(),
        ]
        .iter()
        .any(|value| !valid_hash(value))
        || NativeWirePrimitives::new()
            .canonical_utc_milliseconds(&advance.now)
            .is_none()
    {
        return Err(State);
    }
    Ok(())
}

/// Bind the verified acknowledgement bytes and typed request projection.
fn validate_acknowledgement(
    acknowledgement: &VerifiedNativeRevisionColdCustodyAcknowledgementV2,
) -> Result<(), NativeMaintenanceColdJournalError> {
    use NativeMaintenanceColdJournalError::{Codec, State};

    validate_request(&acknowledgement.request)?;
    if acknowledgement.canonical_bytes.is_empty()
        || acknowledgement.canonical_bytes.len() > CUSTODY_ACK_MAXIMUM_BYTES
        || NativeWirePrimitives::new()
            .encode_canonical_json(&acknowledgement.value)
            .ok_or(Codec)?
            != acknowledgement.canonical_bytes
        || !acknowledgement_bindings_match(acknowledgement)
    {
        return Err(State);
    }
    Ok(())
}

/// Compare all request and successor fields copied into the acknowledgement.
fn acknowledgement_bindings_match(
    acknowledgement: &VerifiedNativeRevisionColdCustodyAcknowledgementV2,
) -> bool {
    let value = &acknowledgement.value;
    let request = &acknowledgement.request;
    // retain the exact v2 authority and every archived request coordinate
    text(value, "contractVersion").as_deref()
        == Some("adjustment-revision-custody-acknowledgement/v2")
        && text(value, "authority").as_deref() == Some("cold_custody_only")
        && text(value, "afterArchiveCommitOrdinal").as_deref()
            == Some(request.page.after_archive_commit_ordinal.as_str())
        && text(value, "afterFrontierSha256").as_deref()
            == Some(request.page.after_frontier_sha256.as_str())
        && text(value, "custodyCheckpointSha256").as_deref()
            == Some(request.custody_checkpoint_sha256.as_str())
        && text(value, "memberRootSha256").as_deref() == Some(request.member_root_sha256.as_str())
        && text(value, "pageSha256").as_deref() == Some(request.page_sha256.as_str())
        && text(value, "previousPageSha256").as_deref()
            == Some(request.page.previous_page_sha256.as_str())
        && text(value, "startMemberSha256").as_deref() == Some(request.start_member_sha256.as_str())
        && text(value, "startSha256").as_deref() == Some(request.page.start_sha256.as_str())
        && text(value, "watermarkArchiveCommitOrdinal").as_deref()
            == Some(request.page.watermark_archive_commit_ordinal.as_str())
        && text(value, "watermarkFrontierSha256").as_deref()
            == Some(request.page.watermark_frontier_sha256.as_str())
        && text(value, "nextArchiveCommitOrdinal").as_deref()
            == Some(acknowledgement.next_archive_commit_ordinal.as_str())
        && text(value, "nextFrontierSha256").as_deref()
            == Some(acknowledgement.next_frontier_sha256.as_str())
}

/// Validate one replayed cursor projection.
fn validate_cursor(
    cursor: &NativeRevisionColdCursor,
) -> Result<(), NativeMaintenanceColdJournalError> {
    ordinal(&cursor.archive_commit_ordinal)?;
    // refuse any non-addressed frontier
    if !valid_hash(&cursor.frontier_sha256) {
        return Err(NativeMaintenanceColdJournalError::State);
    }
    Ok(())
}

/// Parse one canonical uint64 source spelling.
fn ordinal(value: &str) -> Result<u64, NativeMaintenanceColdJournalError> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| NativeMaintenanceColdJournalError::State)?;
    // prohibit leading zeros and every alternate decimal spelling
    if parsed.to_string() != value {
        return Err(NativeMaintenanceColdJournalError::State);
    }
    Ok(parsed)
}

/// Encode one typed intent payload in source field order.
fn intent_value(intent: &NativeRevisionColdCustodyIntent) -> JsonValue {
    object(vec![
        (
            "nextArchiveCommitOrdinal",
            string(&intent.next_archive_commit_ordinal),
        ),
        ("nextFrontierSha256", string(&intent.next_frontier_sha256)),
        ("request", request_value(&intent.request)),
    ])
}

/// Encode one source packed custody request.
fn request_value(request: &NativeRevisionColdCustodyRequest) -> JsonValue {
    object(vec![
        (
            "afterArchiveCommitOrdinal",
            string(&request.page.after_archive_commit_ordinal),
        ),
        (
            "afterFrontierSha256",
            string(&request.page.after_frontier_sha256),
        ),
        (
            "custodyCheckpointSha256",
            string(&request.custody_checkpoint_sha256),
        ),
        ("memberRootSha256", string(&request.member_root_sha256)),
        ("pageSha256", string(&request.page_sha256)),
        (
            "previousPageSha256",
            string(&request.page.previous_page_sha256),
        ),
        ("startMemberSha256", string(&request.start_member_sha256)),
        ("startSha256", string(&request.page.start_sha256)),
        (
            "watermarkArchiveCommitOrdinal",
            string(&request.page.watermark_archive_commit_ordinal),
        ),
        (
            "watermarkFrontierSha256",
            string(&request.page.watermark_frontier_sha256),
        ),
    ])
}

/// Encode one cursor transition without its journal-only clock.
fn advance_value(advance: &NativeRevisionColdCursorAdvance) -> JsonValue {
    object(vec![
        (
            "afterArchiveCommitOrdinal",
            string(&advance.after_archive_commit_ordinal),
        ),
        (
            "afterFrontierSha256",
            string(&advance.after_frontier_sha256),
        ),
        (
            "custodyCheckpointSha256",
            string(&advance.custody_checkpoint_sha256),
        ),
        (
            "nextArchiveCommitOrdinal",
            string(&advance.next_archive_commit_ordinal),
        ),
        ("nextFrontierSha256", string(&advance.next_frontier_sha256)),
        ("pageSha256", string(&advance.page_sha256)),
        ("startSha256", string(&advance.start_sha256)),
    ])
}

/// Construct one owned source journal addition.
fn addition(
    kind: &str,
    record_key: String,
    now: &str,
    payload: JsonValue,
) -> NativeMaintenanceOwnedJournalAddition {
    NativeMaintenanceOwnedJournalAddition {
        due_key: None,
        kind: kind.to_owned(),
        payload,
        record_key,
        recorded_at: now.to_owned(),
    }
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
#[path = "native_maintenance_cold_journal_test.rs"]
mod tests;
