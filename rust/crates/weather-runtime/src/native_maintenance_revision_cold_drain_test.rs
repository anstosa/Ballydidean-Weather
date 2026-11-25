use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use weather_contracts::JsonValue;

use super::*;
use crate::NativeBackendRuntime;

#[derive(Clone, Debug)]
struct Shared {
    state: Rc<RefCell<State>>,
}

#[derive(Debug)]
struct State {
    cursor: NativeRevisionColdCursor,
    events: Vec<&'static str>,
    pending: Option<NativeRevisionColdCustodyIntent>,
}

impl Shared {
    /// Create one isolated journal and effect trace.
    fn new(cursor: NativeRevisionColdCursor) -> Self {
        Self {
            state: Rc::new(RefCell::new(State {
                cursor,
                events: Vec::new(),
                pending: None,
            })),
        }
    }

    /// Record one observable source boundary in call order.
    fn event(&self, event: &'static str) {
        self.state.borrow_mut().events.push(event);
    }
}

struct Remote {
    shared: Shared,
    start: VerifiedNativeRevisionColdStart,
    page: VerifiedNativeRevisionColdPage,
    acknowledgement: VerifiedNativeRevisionColdCustodyAcknowledgementV2,
}

impl NativeRevisionColdRemotePort for Remote {
    type Error = &'static str;

    /// Return the one verified cutoff-bound fixture start.
    fn fetch_verified_start<'a>(
        &'a mut self,
        cutoff_at: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<VerifiedNativeRevisionColdStart, Self::Error>> + 'a>>
    {
        Box::pin(async move {
            self.shared.event("start");
            // accept only the requested fixture cutoff
            if cutoff_at != self.start.cutoff_at {
                return Err("cutoff");
            }
            Ok(self.start.clone())
        })
    }

    /// Bind the page fixture to the request passed by the state machine.
    fn fetch_verified_page<'a>(
        &'a mut self,
        request: &'a NativeRevisionColdPageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<VerifiedNativeRevisionColdPage, Self::Error>> + 'a>>
    {
        Box::pin(async move {
            self.shared.event("page");
            // reject a page request outside the fixture coordinates
            if request != &self.page.request {
                return Err("request");
            }
            Ok(self.page.clone())
        })
    }

    /// Return the exact request-bound v2 acknowledgement fixture.
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
    > {
        Box::pin(async move {
            self.shared.event("ack");
            // bind the acknowledgement to the archived fixture request
            if request != &self.acknowledgement.request {
                return Err("ack request");
            }
            Ok(self.acknowledgement.clone())
        })
    }
}

struct Archive {
    shared: Shared,
    publication: NativeRevisionColdArchivePublication,
}

impl NativeRevisionColdArchivePort for Archive {
    type Error = &'static str;

    /// Publish only the fixture page and its matching start.
    fn archive_verified_page<'a>(
        &'a mut self,
        page: &'a VerifiedNativeRevisionColdPage,
        start: &'a VerifiedNativeRevisionColdStart,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdArchivePublication, Self::Error>> + 'a>>
    {
        Box::pin(async move {
            self.shared.event("archive");
            // require the page and start to share their transfer identity
            if page.request.start_sha256 != start.start_sha256 {
                return Err("archive input");
            }
            Ok(self.publication.clone())
        })
    }

    /// Retain the source non-forced pack policy.
    fn seal_revision_custody_pack(
        &mut self,
        force: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>> {
        Box::pin(async move {
            self.shared.event("seal");
            // preserve the normal non-forced pack policy
            if force {
                return Err("forced");
            }
            Ok(())
        })
    }
}

struct Journal {
    shared: Shared,
}

impl NativeRevisionColdJournalPort for Journal {
    type Error = &'static str;

    /// Read the fixture cursor without changing it.
    fn read_revision_cursor(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdCursor, Self::Error>> + '_>> {
        Box::pin(async move {
            self.shared.event("cursor_read");
            Ok(self.shared.state.borrow().cursor.clone())
        })
    }

    /// Read the fixture's sole pending intent.
    fn read_pending_revision_custody(
        &mut self,
    ) -> Pin<
        Box<dyn Future<Output = Result<Option<NativeRevisionColdCustodyIntent>, Self::Error>> + '_>,
    > {
        Box::pin(async move {
            self.shared.event("pending_read");
            Ok(self.shared.state.borrow().pending.clone())
        })
    }

    /// Persist the exact intended successor for recovery.
    fn record_revision_custody_intent<'a>(
        &'a mut self,
        intent: &'a NativeRevisionColdCustodyIntent,
        now: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdCustodyIntent, Self::Error>> + 'a>>
    {
        Box::pin(async move {
            self.shared.event("intent");
            // admit only the canonical fixture clock
            if now != NOW {
                return Err("intent clock");
            }
            self.shared.state.borrow_mut().pending = Some(intent.clone());
            Ok(intent.clone())
        })
    }

    /// Apply the fixture compare-and-swap cursor transition.
    fn advance_revision_cursor<'a>(
        &'a mut self,
        advance: &'a NativeRevisionColdCursorAdvance,
    ) -> Pin<Box<dyn Future<Output = Result<NativeRevisionColdCursor, Self::Error>> + 'a>> {
        Box::pin(async move {
            self.shared.event("cursor");
            let mut state = self.shared.state.borrow_mut();
            // model the journal's exact compare-and-swap boundary
            if state.cursor.archive_commit_ordinal != advance.after_archive_commit_ordinal
                || state.cursor.frontier_sha256 != advance.after_frontier_sha256
                || advance.now != NOW
            {
                return Err("cursor compare and swap");
            }
            state.cursor = NativeRevisionColdCursor {
                archive_commit_ordinal: advance.next_archive_commit_ordinal.clone(),
                frontier_sha256: advance.next_frontier_sha256.clone(),
            };
            Ok(state.cursor.clone())
        })
    }

    /// Retire the fixture pending intent only after acknowledgement.
    fn complete_revision_custody_v2<'a>(
        &'a mut self,
        acknowledgement: &'a VerifiedNativeRevisionColdCustodyAcknowledgementV2,
        now: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + 'a>> {
        Box::pin(async move {
            self.shared.event("complete");
            // bind completion to the second canonical clock sample
            if now != NOW {
                return Err("complete clock");
            }
            let mut state = self.shared.state.borrow_mut();
            let pending = state.pending.as_ref().ok_or("pending")?;
            // refuse an acknowledgement for a different pending page
            if pending.request != acknowledgement.request {
                return Err("complete request");
            }
            state.pending = None;
            Ok(())
        })
    }
}

struct Clock;

impl NativeRevisionColdClock for Clock {
    type Error = &'static str;

    /// Return the fixed canonical fixture commit time.
    fn canonical_now(&mut self) -> Result<String, Self::Error> {
        Ok(NOW.to_owned())
    }
}

const CUTOFF: &str = "2026-10-09T01:10:00.000Z";
const NOW: &str = "2026-10-09T01:11:00.000Z";

#[test]
fn native_cold_drain_archives_before_cursor_and_acknowledgement() {
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let (mut remote, mut archive, mut journal, shared) = fixture("0", "2", false);
        let result = drain_native_maintenance_revision_cold_page(
            CUTOFF,
            &mut remote,
            &mut archive,
            &mut journal,
            &mut Clock,
        )
        .await
        .expect("drain page");

        assert_eq!(
            result.state,
            NativeRevisionColdDrainState::CapturedThroughWatermark
        );
        assert_eq!(result.cursor.archive_commit_ordinal, "2");
        assert_eq!(result.pages.len(), 1);
        assert_eq!(result.published_entries.len(), 1);
        assert_eq!(result.transfer_bytes, 600);
        assert_eq!(
            shared.state.borrow().events,
            [
                "cursor_read",
                "pending_read",
                "start",
                "page",
                "archive",
                "intent",
                "cursor_read",
                "cursor",
                "cursor_read",
                "ack",
                "complete",
                "seal",
            ]
        );
    });
}

#[test]
fn native_cold_drain_resumes_pending_custody_before_new_remote_reads() {
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let (mut remote, mut archive, mut journal, shared) = fixture("2", "2", false);
        shared.state.borrow_mut().pending = Some(intent(&remote));
        let result = drain_native_maintenance_revision_cold_page(
            CUTOFF,
            &mut remote,
            &mut archive,
            &mut journal,
            &mut Clock,
        )
        .await
        .expect("resume custody");

        assert_eq!(
            result.state,
            NativeRevisionColdDrainState::CustodyReconciled
        );
        assert!(result.start.is_none());
        assert_eq!(result.transfer_bytes, 300);
        assert_eq!(
            shared.state.borrow().events,
            [
                "cursor_read",
                "pending_read",
                "cursor_read",
                "cursor_read",
                "ack",
                "complete",
                "seal",
            ]
        );
    });
}

#[test]
fn native_cold_drain_empty_watermark_skips_archive_and_custody() {
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let (mut remote, mut archive, mut journal, shared) = fixture("0", "0", true);
        let result = drain_native_maintenance_revision_cold_page(
            CUTOFF,
            &mut remote,
            &mut archive,
            &mut journal,
            &mut Clock,
        )
        .await
        .expect("empty watermark");

        assert_eq!(
            result.state,
            NativeRevisionColdDrainState::CapturedThroughWatermark
        );
        assert!(result.pages.is_empty());
        assert_eq!(result.transfer_bytes, 300);
        assert_eq!(
            shared.state.borrow().events,
            ["cursor_read", "pending_read", "start", "page"]
        );
    });
}

#[test]
fn native_cold_drain_historical_watermark_never_fetches_or_rewinds() {
    let runtime = NativeBackendRuntime::new().expect("runtime");
    runtime.block_on(async {
        let (mut remote, mut archive, mut journal, shared) = fixture("3", "2", false);
        let result = drain_native_maintenance_revision_cold_page(
            CUTOFF,
            &mut remote,
            &mut archive,
            &mut journal,
            &mut Clock,
        )
        .await
        .expect("historical watermark");

        assert_eq!(
            result.state,
            NativeRevisionColdDrainState::ArchivedCursorAhead
        );
        assert_eq!(result.cursor.archive_commit_ordinal, "3");
        assert_eq!(
            shared.state.borrow().events,
            ["cursor_read", "pending_read", "start"]
        );
    });
}

/// Build one internally bound start, page, publication, and acknowledgement fixture.
fn fixture(
    cursor_ordinal: &str,
    watermark_ordinal: &str,
    empty: bool,
) -> (Remote, Archive, Journal, Shared) {
    let genesis = hash('0');
    // retain genesis only for the empty watermark fixture
    let next_frontier = if empty { genesis.clone() } else { hash('2') };
    let shared = Shared::new(NativeRevisionColdCursor {
        archive_commit_ordinal: cursor_ordinal.to_owned(),
        // bind only the zero cursor to the genesis frontier
        frontier_sha256: if cursor_ordinal == "0" {
            genesis.clone()
        } else {
            next_frontier.clone()
        },
    });
    let start = VerifiedNativeRevisionColdStart {
        cutoff_at: CUTOFF.to_owned(),
        canonical_bytes: vec![b's'; 100],
        value: JsonValue::Object(Vec::new()),
        start_sha256: hash('a'),
        watermark_archive_commit_ordinal: watermark_ordinal.to_owned(),
        // keep the empty snapshot on the genesis frontier
        watermark_frontier_sha256: if empty {
            genesis.clone()
        } else {
            next_frontier.clone()
        },
    };
    let request = NativeRevisionColdPageRequest {
        after_archive_commit_ordinal: "0".to_owned(),
        after_frontier_sha256: genesis,
        previous_page_sha256: start.start_sha256.clone(),
        start_sha256: start.start_sha256.clone(),
        watermark_archive_commit_ordinal: watermark_ordinal.to_owned(),
        watermark_frontier_sha256: start.watermark_frontier_sha256.clone(),
    };
    // include one published and one terminal-failure entry on nonempty pages
    let entries = if empty {
        Vec::new()
    } else {
        vec![
            VerifiedNativeRevisionColdEntry {
                publication_disposition: NativeRevisionColdPublicationDisposition::Published,
                value: JsonValue::Object(Vec::new()),
            },
            VerifiedNativeRevisionColdEntry {
                publication_disposition:
                    NativeRevisionColdPublicationDisposition::CommittedUnpublishedGap,
                value: JsonValue::Object(Vec::new()),
            },
        ]
    };
    let page = VerifiedNativeRevisionColdPage {
        request,
        canonical_bytes: vec![b'p'; 200],
        entries,
        eof: true,
        next_archive_commit_ordinal: watermark_ordinal.to_owned(),
        next_frontier_sha256: start.watermark_frontier_sha256.clone(),
        page_sha256: hash('b'),
        value: JsonValue::Object(Vec::new()),
    };
    let publication = NativeRevisionColdArchivePublication {
        custody_checkpoint_sha256: hash('c'),
        member_root_sha256: hash('d'),
        start_member_sha256: hash('a'),
    };
    let request = NativeRevisionColdCustodyRequest {
        page: page.request.clone(),
        custody_checkpoint_sha256: publication.custody_checkpoint_sha256.clone(),
        member_root_sha256: publication.member_root_sha256.clone(),
        page_sha256: page.page_sha256.clone(),
        start_member_sha256: publication.start_member_sha256.clone(),
    };
    let acknowledgement = VerifiedNativeRevisionColdCustodyAcknowledgementV2 {
        canonical_bytes: vec![b'a'; 300],
        next_archive_commit_ordinal: watermark_ordinal.to_owned(),
        next_frontier_sha256: start.watermark_frontier_sha256.clone(),
        request,
        value: JsonValue::Object(Vec::new()),
    };
    (
        Remote {
            shared: shared.clone(),
            start,
            page,
            acknowledgement,
        },
        Archive {
            shared: shared.clone(),
            publication,
        },
        Journal {
            shared: shared.clone(),
        },
        shared,
    )
}

/// Project the fixture acknowledgement into its matching durable intent.
fn intent(remote: &Remote) -> NativeRevisionColdCustodyIntent {
    NativeRevisionColdCustodyIntent {
        next_archive_commit_ordinal: remote.acknowledgement.next_archive_commit_ordinal.clone(),
        next_frontier_sha256: remote.acknowledgement.next_frontier_sha256.clone(),
        request: remote.acknowledgement.request.clone(),
    }
}

/// Build one valid lowercase content identity fixture.
fn hash(character: char) -> String {
    character.to_string().repeat(64)
}
