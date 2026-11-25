//! Transfer one verified revision page into durable cold custody.

use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::pin::Pin;

use weather_api::CurrentTimeResolver;
use weather_contracts::{JsDateOrString, JsonValue};

use crate::NativeWirePrimitives;
use crate::maintenance_canonical_json::valid_hash;

/// Exact combined start, page, and maximum acknowledgement wire ceiling.
pub const ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES: usize = 4_832 * 1_024;
const CUSTODY_ACK_MAXIMUM_BYTES: usize = 16 * 1_024;

/// Durable local revision frontier retained between bounded cold cycles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRevisionColdCursor {
    pub archive_commit_ordinal: String,
    pub frontier_sha256: String,
}

/// Verified cutoff-bound transfer start returned by the fixed remote owner.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedNativeRevisionColdStart {
    pub cutoff_at: String,
    pub canonical_bytes: Vec<u8>,
    pub value: JsonValue,
    pub start_sha256: String,
    pub watermark_archive_commit_ordinal: String,
    pub watermark_frontier_sha256: String,
}

/// Exact six-coordinate request for one successor page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRevisionColdPageRequest {
    pub after_archive_commit_ordinal: String,
    pub after_frontier_sha256: String,
    pub previous_page_sha256: String,
    pub start_sha256: String,
    pub watermark_archive_commit_ordinal: String,
    pub watermark_frontier_sha256: String,
}

/// Semantic publication state already verified as part of one cold entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRevisionColdPublicationDisposition {
    CommittedUnpublishedGap,
    Published,
}

impl NativeRevisionColdPublicationDisposition {
    /// Return the exact verified producer spelling.
    pub const fn as_str(self) -> &'static str {
        // retain both closed publication outcomes
        match self {
            Self::CommittedUnpublishedGap => "committed_unpublished_gap",
            Self::Published => "published",
        }
    }
}

/// One complete verified cold entry retained for downstream evidence projection.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedNativeRevisionColdEntry {
    pub publication_disposition: NativeRevisionColdPublicationDisposition,
    pub value: JsonValue,
}

/// Verified bounded successor page returned by the fixed remote owner.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedNativeRevisionColdPage {
    pub request: NativeRevisionColdPageRequest,
    pub canonical_bytes: Vec<u8>,
    pub entries: Vec<VerifiedNativeRevisionColdEntry>,
    pub eof: bool,
    pub next_archive_commit_ordinal: String,
    pub next_frontier_sha256: String,
    pub page_sha256: String,
    pub value: JsonValue,
}

/// Durable archive publication needed before any local cursor mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRevisionColdArchivePublication {
    pub custody_checkpoint_sha256: String,
    pub member_root_sha256: String,
    pub start_member_sha256: String,
}

/// Exact archived page request accepted by the custody-only remote command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRevisionColdCustodyRequest {
    pub page: NativeRevisionColdPageRequest,
    pub custody_checkpoint_sha256: String,
    pub member_root_sha256: String,
    pub page_sha256: String,
    pub start_member_sha256: String,
}

/// Durable local intent that makes acknowledgement recovery idempotent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRevisionColdCustodyIntent {
    pub next_archive_commit_ordinal: String,
    pub next_frontier_sha256: String,
    pub request: NativeRevisionColdCustodyRequest,
}

/// Compare-and-swap input for one already archived page frontier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRevisionColdCursorAdvance {
    pub after_archive_commit_ordinal: String,
    pub after_frontier_sha256: String,
    pub custody_checkpoint_sha256: String,
    pub next_archive_commit_ordinal: String,
    pub next_frontier_sha256: String,
    pub now: String,
    pub page_sha256: String,
    pub start_sha256: String,
}

/// Verified v2 custody acknowledgement bound to the request and successor.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedNativeRevisionColdCustodyAcknowledgementV2 {
    pub canonical_bytes: Vec<u8>,
    pub next_archive_commit_ordinal: String,
    pub next_frontier_sha256: String,
    pub request: NativeRevisionColdCustodyRequest,
    pub value: JsonValue,
}

/// One archived page exposed by a successful drain result.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeRevisionColdPublishedPage {
    pub custody_checkpoint_sha256: String,
    pub page: VerifiedNativeRevisionColdPage,
}

/// Closed source result states for one bounded drain invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRevisionColdDrainState {
    ArchivedCursorAhead,
    CapturedThroughWatermark,
    CustodyReconciled,
    PageCaptured,
}

impl NativeRevisionColdDrainState {
    /// Return the exact source result spelling.
    pub const fn as_str(self) -> &'static str {
        // retain only the four source state-machine outcomes
        match self {
            Self::ArchivedCursorAhead => "archived_cursor_ahead",
            Self::CapturedThroughWatermark => "captured_through_watermark",
            Self::CustodyReconciled => "custody_reconciled",
            Self::PageCaptured => "page_captured",
        }
    }
}

/// Complete result consumed as the cold branch of capture-cycle synchronization.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeRevisionColdDrainResult {
    pub cursor: NativeRevisionColdCursor,
    pub eof: bool,
    pub pages: Vec<NativeRevisionColdPublishedPage>,
    pub published_entries: Vec<VerifiedNativeRevisionColdEntry>,
    pub start: Option<VerifiedNativeRevisionColdStart>,
    pub state: NativeRevisionColdDrainState,
    pub transfer_bytes: usize,
}

/// Fixed verified remote revision source and custody acknowledgement authority.
pub trait NativeRevisionColdRemotePort {
    type Error;

    /// Read one cutoff-bound start through the fixed authenticated command.
    fn fetch_verified_start<'a>(
        &'a mut self,
        cutoff_at: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<VerifiedNativeRevisionColdStart, Self::Error>> + 'a>>;

    /// Read one page bound to all six request coordinates.
    fn fetch_verified_page<'a>(
        &'a mut self,
        request: &'a NativeRevisionColdPageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<VerifiedNativeRevisionColdPage, Self::Error>> + 'a>>;

    /// Acknowledge only one exact archive-backed custody request.
    fn acknowledge_verified_v2<'a>(
        &'a mut self,
        request: &'a NativeRevisionColdCustodyRequest,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        VerifiedNativeRevisionColdCustodyAcknowledgementV2,
                        Self::Error,
                    >,
                > + 'a,
        >,
    >;
}

/// Durable cold graph archive and pack-seal effects.
pub trait NativeRevisionColdArchivePort {
    type Error;

    /// Archive and verify the complete page graph before returning its custody roots.
    fn archive_verified_page<'a>(
        &'a mut self,
        page: &'a VerifiedNativeRevisionColdPage,
        start: &'a VerifiedNativeRevisionColdStart,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdArchivePublication, Self::Error>> + 'a>>;

    /// Seal only by the archive owner's normal threshold policy.
    fn seal_revision_custody_pack(
        &mut self,
        force: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>>;
}

/// Bounded pending custody journal lookup.
pub type NativeRevisionColdPendingFuture<'a, Error> =
    Pin<Box<dyn Future<Output = Result<Option<NativeRevisionColdCustodyIntent>, Error>> + 'a>>;

/// Append-only journal effects for cursor and custody recovery.
pub trait NativeRevisionColdJournalPort {
    type Error;

    /// Read the current durable cold cursor.
    fn read_revision_cursor(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdCursor, Self::Error>> + '_>>;

    /// Read the sole unfinished archived-page custody intent.
    fn read_pending_revision_custody(&mut self)
    -> NativeRevisionColdPendingFuture<'_, Self::Error>;

    /// Persist one page-bound intent before cursor advancement.
    fn record_revision_custody_intent<'a>(
        &'a mut self,
        intent: &'a NativeRevisionColdCustodyIntent,
        now: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdCustodyIntent, Self::Error>> + 'a>>;

    /// Advance the exact archived cursor through the journal compare-and-swap boundary.
    fn advance_revision_cursor<'a>(
        &'a mut self,
        advance: &'a NativeRevisionColdCursorAdvance,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdCursor, Self::Error>> + 'a>>;

    /// Complete custody only after the verified remote acknowledgement is available.
    fn complete_revision_custody_v2<'a>(
        &'a mut self,
        acknowledgement: &'a VerifiedNativeRevisionColdCustodyAcknowledgementV2,
        now: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>>;
}

/// Actual local commit clock sampled independently for every journal mutation.
pub trait NativeRevisionColdClock {
    type Error;

    /// Return one canonical UTC millisecond instant.
    fn canonical_now(&mut self) -> Result<String, Self::Error>;
}

/// Failure from one exact owner or a cross-owner wire binding.
#[derive(Debug)]
pub enum NativeRevisionColdDrainError<Remote, Archive, Journal, Clock> {
    Remote(Remote),
    Archive(Archive),
    Journal(Journal),
    Clock(Clock),
    InvalidBinding,
    CycleByteCeiling,
}

impl<Remote: Display, Archive: Display, Journal: Display, Clock: Display> Display
    for NativeRevisionColdDrainError<Remote, Archive, Journal, Clock>
{
    /// Retain the first owner diagnostic or one bounded protocol refusal.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve source effect ownership in the failure surface
        match self {
            Self::Remote(error) => Display::fmt(error, formatter),
            Self::Archive(error) => Display::fmt(error, formatter),
            Self::Journal(error) => Display::fmt(error, formatter),
            Self::Clock(error) => Display::fmt(error, formatter),
            Self::InvalidBinding => formatter.write_str("revision cold drain binding differs"),
            Self::CycleByteCeiling => {
                formatter.write_str("revision cold drain cycle exceeds byte ceiling")
            }
        }
    }
}

/// Transfer one bounded page and advance custody only after durable graph verification.
pub async fn drain_native_maintenance_revision_cold_page<Remote, Archive, Journal, Clock>(
    cutoff_at: &str,
    remote: &mut Remote,
    archive: &mut Archive,
    journal: &mut Journal,
    clock: &mut Clock,
) -> Result<
    NativeRevisionColdDrainResult,
    NativeRevisionColdDrainError<Remote::Error, Archive::Error, Journal::Error, Clock::Error>,
>
where
    Remote: NativeRevisionColdRemotePort,
    Archive: NativeRevisionColdArchivePort,
    Journal: NativeRevisionColdJournalPort,
    Clock: NativeRevisionColdClock,
{
    type Error<R, A, J, C> = NativeRevisionColdDrainError<R, A, J, C>;

    // reject a substituted or noncanonical due boundary before any owner effect
    if canonical_instant(cutoff_at).is_none() {
        return Err(Error::InvalidBinding);
    }
    let cursor = journal
        .read_revision_cursor()
        .await
        .map_err(Error::Journal)?;
    // admit the retained unsigned database coordinate before recovery or remote reads
    if !valid_cursor(&cursor) {
        return Err(Error::InvalidBinding);
    }
    let pending = journal
        .read_pending_revision_custody()
        .await
        .map_err(Error::Journal)?;
    // reconcile the archived request before fetching a newer database snapshot
    if let Some(intent) = pending {
        let (cursor, transfer_bytes) = finish_custody::<Remote, Journal, Clock, Archive::Error>(
            &intent, remote, journal, clock,
        )
        .await?;
        archive
            .seal_revision_custody_pack(false)
            .await
            .map_err(Error::Archive)?;
        return Ok(NativeRevisionColdDrainResult {
            cursor,
            eof: false,
            pages: Vec::new(),
            published_entries: Vec::new(),
            start: None,
            state: NativeRevisionColdDrainState::CustodyReconciled,
            transfer_bytes,
        });
    }
    let start = remote
        .fetch_verified_start(cutoff_at)
        .await
        .map_err(Error::Remote)?;
    // bind the retained source snapshot to the original due cutoff and wire limits
    if !valid_start(&start) || start.cutoff_at != cutoff_at {
        return Err(Error::InvalidBinding);
    }
    let start_bytes = start.canonical_bytes.len();
    let watermark =
        canonical_ordinal(&start.watermark_archive_commit_ordinal).ok_or(Error::InvalidBinding)?;
    let cursor_ordinal =
        canonical_ordinal(&cursor.archive_commit_ordinal).ok_or(Error::InvalidBinding)?;
    // historical work uses retained graphs and cannot rewind global custody
    if watermark < cursor_ordinal {
        return Ok(NativeRevisionColdDrainResult {
            cursor,
            eof: false,
            pages: Vec::new(),
            published_entries: Vec::new(),
            start: Some(start),
            state: NativeRevisionColdDrainState::ArchivedCursorAhead,
            transfer_bytes: start_bytes,
        });
    }
    let request = NativeRevisionColdPageRequest {
        after_archive_commit_ordinal: cursor.archive_commit_ordinal.clone(),
        after_frontier_sha256: cursor.frontier_sha256.clone(),
        previous_page_sha256: start.start_sha256.clone(),
        start_sha256: start.start_sha256.clone(),
        watermark_archive_commit_ordinal: start.watermark_archive_commit_ordinal.clone(),
        watermark_frontier_sha256: start.watermark_frontier_sha256.clone(),
    };
    let page = remote
        .fetch_verified_page(&request)
        .await
        .map_err(Error::Remote)?;
    // bind every response coordinate before any archive write
    if !valid_page(&page) || page.request != request {
        return Err(Error::InvalidBinding);
    }
    let mut transfer_bytes = start_bytes
        .checked_add(page.canonical_bytes.len())
        .ok_or(Error::CycleByteCeiling)?;
    // reserve no acknowledgement bytes for unchanged frontiers
    let reserved_ack_bytes = if page.entries.is_empty() {
        0
    } else {
        CUSTODY_ACK_MAXIMUM_BYTES
    };
    // reserve the complete acknowledgement ceiling before graph publication
    if transfer_bytes
        .checked_add(reserved_ack_bytes)
        .is_none_or(|bytes| bytes > ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES)
    {
        return Err(Error::CycleByteCeiling);
    }
    // unchanged authenticated frontiers do not create archive objects
    if page.entries.is_empty() {
        return Ok(NativeRevisionColdDrainResult {
            cursor,
            eof: page.eof,
            pages: Vec::new(),
            published_entries: Vec::new(),
            start: Some(start),
            state: NativeRevisionColdDrainState::CapturedThroughWatermark,
            transfer_bytes,
        });
    }
    let publication = archive
        .archive_verified_page(&page, &start)
        .await
        .map_err(Error::Archive)?;
    // only genuine content-addressed graph roots can authorize a cursor transition
    if !valid_publication(&publication) {
        return Err(Error::InvalidBinding);
    }
    let custody_request = NativeRevisionColdCustodyRequest {
        page: request,
        custody_checkpoint_sha256: publication.custody_checkpoint_sha256.clone(),
        member_root_sha256: publication.member_root_sha256.clone(),
        page_sha256: page.page_sha256.clone(),
        start_member_sha256: publication.start_member_sha256.clone(),
    };
    let intended = NativeRevisionColdCustodyIntent {
        next_archive_commit_ordinal: page.next_archive_commit_ordinal.clone(),
        next_frontier_sha256: page.next_frontier_sha256.clone(),
        request: custody_request,
    };
    let now = clock.canonical_now().map_err(Error::Clock)?;
    // persist exactly the archive-backed request before cursor mutation
    if canonical_instant(&now).is_none() {
        return Err(Error::InvalidBinding);
    }
    let intent = journal
        .record_revision_custody_intent(&intended, &now)
        .await
        .map_err(Error::Journal)?;
    // prevent a journal owner from returning a different pending intent
    if intent != intended {
        return Err(Error::InvalidBinding);
    }
    let (next_cursor, acknowledgement_bytes) =
        finish_custody::<Remote, Journal, Clock, Archive::Error>(&intent, remote, journal, clock)
            .await?;
    transfer_bytes = transfer_bytes
        .checked_add(acknowledgement_bytes)
        .ok_or(Error::CycleByteCeiling)?;
    archive
        .seal_revision_custody_pack(false)
        .await
        .map_err(Error::Archive)?;
    // account for the actual acknowledgement after normal threshold sealing
    if transfer_bytes > ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES {
        return Err(Error::CycleByteCeiling);
    }
    let published_entries = page
        .entries
        .iter()
        .filter(|entry| {
            // terminal publication failures remain custody but not semantic evidence
            entry.publication_disposition == NativeRevisionColdPublicationDisposition::Published
        })
        .cloned()
        .collect();
    // distinguish a complete watermark from one bounded intermediate page
    let state = if page.eof {
        NativeRevisionColdDrainState::CapturedThroughWatermark
    } else {
        NativeRevisionColdDrainState::PageCaptured
    };
    Ok(NativeRevisionColdDrainResult {
        cursor: next_cursor,
        eof: page.eof,
        pages: vec![NativeRevisionColdPublishedPage {
            custody_checkpoint_sha256: publication.custody_checkpoint_sha256,
            page,
        }],
        published_entries,
        start: Some(start),
        state,
        transfer_bytes,
    })
}

/// Resume one durable intent and retire server bytes only after cursor visibility.
async fn finish_custody<Remote, Journal, Clock, ArchiveError>(
    intent: &NativeRevisionColdCustodyIntent,
    remote: &mut Remote,
    journal: &mut Journal,
    clock: &mut Clock,
) -> Result<
    (NativeRevisionColdCursor, usize),
    NativeRevisionColdDrainError<Remote::Error, ArchiveError, Journal::Error, Clock::Error>,
>
where
    Remote: NativeRevisionColdRemotePort,
    Journal: NativeRevisionColdJournalPort,
    Clock: NativeRevisionColdClock,
{
    type Error<R, A, J, C> = NativeRevisionColdDrainError<R, A, J, C>;

    // reject malformed retained intents before cursor or remote mutation
    if !valid_intent(intent) {
        return Err(Error::InvalidBinding);
    }
    let mut cursor = journal
        .read_revision_cursor()
        .await
        .map_err(Error::Journal)?;
    if !valid_cursor(&cursor) {
        return Err(Error::InvalidBinding);
    }
    // repair a crash after intent persistence but before cursor commit
    if cursor.archive_commit_ordinal == intent.request.page.after_archive_commit_ordinal
        && cursor.frontier_sha256 == intent.request.page.after_frontier_sha256
    {
        let now = clock.canonical_now().map_err(Error::Clock)?;
        if canonical_instant(&now).is_none() {
            return Err(Error::InvalidBinding);
        }
        cursor = journal
            .advance_revision_cursor(&NativeRevisionColdCursorAdvance {
                after_archive_commit_ordinal: intent
                    .request
                    .page
                    .after_archive_commit_ordinal
                    .clone(),
                after_frontier_sha256: intent.request.page.after_frontier_sha256.clone(),
                custody_checkpoint_sha256: intent.request.custody_checkpoint_sha256.clone(),
                next_archive_commit_ordinal: intent.next_archive_commit_ordinal.clone(),
                next_frontier_sha256: intent.next_frontier_sha256.clone(),
                now,
                page_sha256: intent.request.page_sha256.clone(),
                start_sha256: intent.request.page.start_sha256.clone(),
            })
            .await
            .map_err(Error::Journal)?;
    }
    let durable = journal
        .read_revision_cursor()
        .await
        .map_err(Error::Journal)?;
    // require the exact successor to survive an independent durable reread
    if !valid_cursor(&cursor)
        || cursor.archive_commit_ordinal != intent.next_archive_commit_ordinal
        || cursor.frontier_sha256 != intent.next_frontier_sha256
        || durable != cursor
    {
        return Err(Error::InvalidBinding);
    }
    let acknowledgement = remote
        .acknowledge_verified_v2(&intent.request)
        .await
        .map_err(Error::Remote)?;
    // bind the custody-only reply to the archived request and intended successor
    if !valid_acknowledgement(&acknowledgement)
        || acknowledgement.request != intent.request
        || acknowledgement.next_archive_commit_ordinal != intent.next_archive_commit_ordinal
        || acknowledgement.next_frontier_sha256 != intent.next_frontier_sha256
    {
        return Err(Error::InvalidBinding);
    }
    let transfer_bytes = acknowledgement.canonical_bytes.len();
    let now = clock.canonical_now().map_err(Error::Clock)?;
    if canonical_instant(&now).is_none() {
        return Err(Error::InvalidBinding);
    }
    journal
        .complete_revision_custody_v2(&acknowledgement, &now)
        .await
        .map_err(Error::Journal)?;
    Ok((cursor, transfer_bytes))
}

/// Admit one canonical uint64 source spelling.
fn canonical_ordinal(value: &str) -> Option<u64> {
    let parsed = value.parse::<u64>().ok()?;
    (parsed.to_string() == value).then_some(parsed)
}

/// Admit one canonical UTC millisecond source spelling.
fn canonical_instant(value: &str) -> Option<i64> {
    let resolved = NativeWirePrimitives::new()
        .resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .ok()?;
    (resolved.utc == value).then_some(resolved.epoch_milliseconds)
}

/// Validate one cursor without granting any transition authority.
fn valid_cursor(cursor: &NativeRevisionColdCursor) -> bool {
    canonical_ordinal(&cursor.archive_commit_ordinal).is_some()
        && valid_hash(&cursor.frontier_sha256)
}

/// Validate the fields used by the drain after remote start verification.
fn valid_start(start: &VerifiedNativeRevisionColdStart) -> bool {
    canonical_instant(&start.cutoff_at).is_some()
        && canonical_ordinal(&start.watermark_archive_commit_ordinal).is_some()
        && valid_hash(&start.watermark_frontier_sha256)
        && valid_hash(&start.start_sha256)
        && !start.canonical_bytes.is_empty()
        && start.canonical_bytes.len() <= ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES
}

/// Validate typed page coordinates after complete producer-side admission.
fn valid_page(page: &VerifiedNativeRevisionColdPage) -> bool {
    let Some(after) = canonical_ordinal(&page.request.after_archive_commit_ordinal) else {
        return false;
    };
    let Some(next) = canonical_ordinal(&page.next_archive_commit_ordinal) else {
        return false;
    };
    let Some(watermark) = canonical_ordinal(&page.request.watermark_archive_commit_ordinal) else {
        return false;
    };
    // preserve the remote validator's empty-page and successor invariants locally
    valid_hash(&page.request.after_frontier_sha256)
        && valid_hash(&page.request.previous_page_sha256)
        && valid_hash(&page.request.start_sha256)
        && valid_hash(&page.request.watermark_frontier_sha256)
        && valid_hash(&page.next_frontier_sha256)
        && valid_hash(&page.page_sha256)
        && !page.canonical_bytes.is_empty()
        && page.canonical_bytes.len() <= ADJUSTMENT_REVISION_COLD_CYCLE_MAXIMUM_BYTES
        && ((page.entries.is_empty() && next == after)
            || (!page.entries.is_empty() && next > after))
        && next <= watermark
        && page.entries.len() <= 2
        && (!page.entries.is_empty() || page.eof)
        && (page.eof
            == (page.next_archive_commit_ordinal == page.request.watermark_archive_commit_ordinal
                && page.next_frontier_sha256 == page.request.watermark_frontier_sha256))
}

/// Validate the content identities returned only after durable graph publication.
fn valid_publication(publication: &NativeRevisionColdArchivePublication) -> bool {
    valid_hash(&publication.custody_checkpoint_sha256)
        && valid_hash(&publication.member_root_sha256)
        && valid_hash(&publication.start_member_sha256)
}

/// Validate one retained intent's coordinate and hash grammar.
fn valid_intent(intent: &NativeRevisionColdCustodyIntent) -> bool {
    let Some(after) = canonical_ordinal(&intent.request.page.after_archive_commit_ordinal) else {
        return false;
    };
    let Some(next) = canonical_ordinal(&intent.next_archive_commit_ordinal) else {
        return false;
    };
    let Some(watermark) = canonical_ordinal(&intent.request.page.watermark_archive_commit_ordinal)
    else {
        return false;
    };
    // bind the strict successor and every archived request identity
    next > after
        && next <= watermark
        && valid_hash(&intent.next_frontier_sha256)
        && valid_hash(&intent.request.page.after_frontier_sha256)
        && valid_hash(&intent.request.page.previous_page_sha256)
        && valid_hash(&intent.request.page.start_sha256)
        && valid_hash(&intent.request.page.watermark_frontier_sha256)
        && valid_hash(&intent.request.custody_checkpoint_sha256)
        && valid_hash(&intent.request.member_root_sha256)
        && valid_hash(&intent.request.page_sha256)
        && valid_hash(&intent.request.start_member_sha256)
}

/// Validate the bounded acknowledgement before journal completion.
fn valid_acknowledgement(
    acknowledgement: &VerifiedNativeRevisionColdCustodyAcknowledgementV2,
) -> bool {
    !acknowledgement.canonical_bytes.is_empty()
        && acknowledgement.canonical_bytes.len() <= CUSTODY_ACK_MAXIMUM_BYTES
        && canonical_ordinal(&acknowledgement.next_archive_commit_ordinal).is_some()
        && valid_hash(&acknowledgement.next_frontier_sha256)
}

#[cfg(test)]
#[path = "native_maintenance_revision_cold_drain_test.rs"]
mod tests;
