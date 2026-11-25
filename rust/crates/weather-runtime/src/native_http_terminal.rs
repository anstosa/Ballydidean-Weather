//! Bind forecast evidence to bytes accepted by one native HTTP connection.

use std::cell::RefCell;
use std::convert::Infallible;
use std::fmt::{self, Display, Formatter};
use std::future::poll_fn;
use std::io::{self, IoSlice};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use hyper::body::{Body, Bytes, Frame, SizeHint};
use hyper::rt::{Read, ReadBufCursor, Write};
use weather_edge::ForecastResponseTerminal;

use crate::NativeHttpBody;

type TerminalCallback = Box<dyn FnOnce(ForecastResponseTerminal)>;

/// Closed setup and connection-state failures for terminal-aware responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeHttpTerminalError {
    ConnectionPoisoned,
    ResponseAlreadyPending,
    ResponseBodyEmpty,
}

impl Display for NativeHttpTerminalError {
    /// Describe the ownership invariant that prevented terminal-aware response creation.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionPoisoned => {
                formatter.write_str("native HTTP terminal connection is poisoned")
            }
            Self::ResponseAlreadyPending => {
                formatter.write_str("native HTTP terminal response is already pending")
            }
            Self::ResponseBodyEmpty => {
                formatter.write_str("native HTTP terminal response body is empty")
            }
        }
    }
}

impl std::error::Error for NativeHttpTerminalError {}

/// Aggregate actual terminal results and fail-closed transport inconsistencies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeHttpTerminalReport {
    pub closed_responses: u64,
    pub finished_responses: u64,
    pub invalid_transitions: u64,
}

/// Connection-local owner shared by its Hyper service, response bodies, and socket adapter.
#[derive(Clone)]
pub struct NativeHttpConnectionScope(Rc<RefCell<NativeHttpConnectionState>>);

struct NativeHttpConnectionState {
    idle_waiter: Option<Waker>,
    next_generation: u64,
    pending: Option<PendingResponse>,
    poisoned: bool,
    report: NativeHttpTerminalReport,
}

struct PendingResponse {
    accepted_body_bytes: usize,
    body_submitted: bool,
    body_waiter: Option<Waker>,
    expected_body_bytes: usize,
    generation: u64,
    header_bytes: usize,
    header_flushed: bool,
    terminal: Option<TerminalCallback>,
}

struct TerminalTransition {
    idle_waiter: Option<Waker>,
    terminal: Option<TerminalCallback>,
}

enum BodyAdmission {
    Ready,
    Wait,
    Refused,
}

enum WriteObservation {
    BodyComplete,
    BodyPartial,
    Header,
    Invalid,
}

impl NativeHttpConnectionScope {
    /// Create one independent terminal owner for one accepted socket.
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(NativeHttpConnectionState {
            idle_waiter: None,
            next_generation: 1,
            pending: None,
            poisoned: false,
            report: NativeHttpTerminalReport::default(),
        })))
    }

    /// Prevent pipelined response headers from overtaking the prior real terminal event.
    pub async fn wait_until_idle(&self) -> Result<(), NativeHttpTerminalError> {
        poll_fn(|context| {
            let mut state = self.0.borrow_mut();
            // stop application dispatch after an impossible byte-accounting transition
            if state.poisoned {
                return Poll::Ready(Err(NativeHttpTerminalError::ConnectionPoisoned));
            }
            // admit the next application reply only after the prior callback was removed
            if state.pending.is_none() {
                return Poll::Ready(Ok(()));
            }
            // retain only the current service task's latest equivalent wake target
            if state
                .idle_waiter
                .as_ref()
                .is_none_or(|waiter| !waiter.will_wake(context.waker()))
            {
                state.idle_waiter = Some(context.waker().clone());
            }
            Poll::Pending
        })
        .await
    }

    /// Bind one non-empty finite body to its first real finish or close event.
    pub fn tracked_body(
        &self,
        body: NativeHttpBody,
        terminal: impl FnOnce(ForecastResponseTerminal) + 'static,
    ) -> Result<NativeTrackedHttpBody, NativeHttpTerminalError> {
        let mut terminal: Option<TerminalCallback> = Some(Box::new(terminal));
        let expected_body_bytes = body.size_hint().exact().unwrap_or_default() as usize;
        // a successful forecast capture must always own non-empty representation bytes
        if expected_body_bytes == 0 {
            self.reject_registration(terminal.take().expect("one terminal callback"));
            return Err(NativeHttpTerminalError::ResponseBodyEmpty);
        }
        let registration = {
            let mut state = self.0.borrow_mut();
            let error = if state.poisoned {
                // never attach another response after connection accounting became ambiguous
                Some(NativeHttpTerminalError::ConnectionPoisoned)
            } else if state.pending.is_some() {
                // preserve one response phase per connection so accepted bytes stay attributable
                Some(NativeHttpTerminalError::ResponseAlreadyPending)
            } else {
                None
            };
            // install the callback only after every connection invariant is satisfied
            if let Some(error) = error {
                Err(error)
            } else {
                let generation = state.next_generation;
                state.next_generation = state.next_generation.wrapping_add(1).max(1);
                state.pending = Some(PendingResponse {
                    accepted_body_bytes: 0,
                    body_submitted: false,
                    body_waiter: None,
                    expected_body_bytes,
                    generation,
                    header_bytes: 0,
                    header_flushed: false,
                    terminal: terminal.take(),
                });
                Ok(generation)
            }
        };
        let generation = match registration {
            Ok(generation) => generation,
            Err(error) => {
                self.reject_registration(terminal.take().expect("one terminal callback"));
                return Err(error);
            }
        };
        Ok(NativeTrackedHttpBody {
            body,
            cancelled: false,
            generation: Some(generation),
            scope: Some(self.clone()),
        })
    }

    /// Use the same concrete Hyper body type for responses without forecast evidence.
    pub fn untracked_body(&self, body: NativeHttpBody) -> NativeTrackedHttpBody {
        NativeTrackedHttpBody {
            body,
            cancelled: false,
            generation: None,
            scope: None,
        }
    }

    /// Attach actual socket writes and connection closure to this response owner.
    pub fn tracked_io<Io>(&self, io: Io) -> NativeTrackedHttpIo<Io> {
        NativeTrackedHttpIo {
            io,
            scope: self.clone(),
        }
    }

    /// Snapshot terminal counts without consuming the connection owner.
    pub fn report(&self) -> NativeHttpTerminalReport {
        self.0.borrow().report
    }

    /// Admit one body frame only after non-empty response headers reached a real flush.
    fn admit_body(&self, generation: u64, waiter: &Waker) -> BodyAdmission {
        let mut state = self.0.borrow_mut();
        // refuse stale bodies after a terminal event or connection accounting failure
        if state.poisoned {
            return BodyAdmission::Refused;
        }
        let Some(pending) = state
            .pending
            .as_mut()
            .filter(|pending| pending.generation == generation)
        else {
            return BodyAdmission::Refused;
        };
        // hold the frame until the header bytes are accepted and flushed independently
        if !pending.header_flushed {
            if pending
                .body_waiter
                .as_ref()
                .is_none_or(|current| !current.will_wake(waiter))
            {
                pending.body_waiter = Some(waiter.clone());
            }
            return BodyAdmission::Wait;
        }
        pending.body_submitted = true;
        BodyAdmission::Ready
    }

    /// Release the body barrier after a successful flush of non-empty header output.
    fn observe_flush(&self) {
        let waiter = {
            let mut state = self.0.borrow_mut();
            // ordinary untracked flushes do not participate in forecast evidence
            let Some(pending) = state.pending.as_mut() else {
                return;
            };
            // only the first flush following accepted header bytes opens the body gate
            if pending.header_flushed || pending.header_bytes == 0 || pending.body_waiter.is_none()
            {
                return;
            }
            pending.header_flushed = true;
            pending.body_waiter.take()
        };
        // wake outside the RefCell borrow so the body can re-enter this scope immediately
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }

    /// Attribute bytes accepted by the OS to the current header or body phase.
    fn observe_write(&self, count: usize) {
        // ignore successful zero-byte writes because they make no terminal progress
        if count == 0 {
            return;
        }
        let mut terminal = ForecastResponseTerminal::Closed;
        let transition = {
            let mut state = self.0.borrow_mut();
            // writes without a tracked response belong to ordinary application traffic
            let Some(pending) = state.pending.as_ref() else {
                return;
            };
            let observation = if !pending.header_flushed {
                WriteObservation::Header
            } else if !pending.body_submitted
                || count
                    > pending
                        .expected_body_bytes
                        .saturating_sub(pending.accepted_body_bytes)
            {
                WriteObservation::Invalid
            } else if count
                == pending
                    .expected_body_bytes
                    .saturating_sub(pending.accepted_body_bytes)
            {
                WriteObservation::BodyComplete
            } else {
                WriteObservation::BodyPartial
            };
            match observation {
                WriteObservation::Header => {
                    let pending = state.pending.as_mut().expect("observed pending response");
                    pending.header_bytes = pending.header_bytes.saturating_add(count);
                    None
                }
                WriteObservation::BodyPartial => {
                    let pending = state.pending.as_mut().expect("observed pending response");
                    pending.accepted_body_bytes += count;
                    None
                }
                WriteObservation::BodyComplete => {
                    terminal = ForecastResponseTerminal::Finished;
                    take_terminal(&mut state, ForecastResponseTerminal::Finished)
                }
                WriteObservation::Invalid => {
                    // fail closed rather than credit bytes from another representation
                    state.poisoned = true;
                    state.report.invalid_transitions += 1;
                    take_terminal(&mut state, ForecastResponseTerminal::Closed)
                }
            }
        };
        run_transition(transition, terminal);
    }

    /// Close one pending response when its transport ends before exact completion.
    fn close_pending(&self) {
        let transition = {
            let mut state = self.0.borrow_mut();
            take_terminal(&mut state, ForecastResponseTerminal::Closed)
        };
        run_transition(transition, ForecastResponseTerminal::Closed);
    }

    /// Close an unsubmitted body that was abandoned before Hyper could own its frame.
    fn close_unsubmitted(&self, generation: u64) {
        let transition =
            {
                let mut state = self.0.borrow_mut();
                // do not disturb a newer response or a body already handed to the transport
                if state.pending.as_ref().is_none_or(|pending| {
                    pending.generation != generation || pending.body_submitted
                }) {
                    return;
                }
                take_terminal(&mut state, ForecastResponseTerminal::Closed)
            };
        run_transition(transition, ForecastResponseTerminal::Closed);
    }

    /// Poison one response whose finite body ended without producing its promised frame.
    fn invalidate_body(&self, generation: u64) {
        let transition = {
            let mut state = self.0.borrow_mut();
            // ignore a stale body after its real terminal event already consumed the callback
            if state
                .pending
                .as_ref()
                .is_none_or(|pending| pending.generation != generation)
            {
                return;
            }
            state.poisoned = true;
            state.report.invalid_transitions += 1;
            take_terminal(&mut state, ForecastResponseTerminal::Closed)
        };
        run_transition(transition, ForecastResponseTerminal::Closed);
    }

    /// Refuse further I/O once byte attribution becomes ambiguous.
    fn is_poisoned(&self) -> bool {
        self.0.borrow().poisoned
    }

    /// Consume a newly relinquished observer and any ambiguous predecessor outside the borrow.
    fn reject_registration(&self, terminal: TerminalCallback) {
        let predecessor = {
            let mut state = self.0.borrow_mut();
            state.poisoned = true;
            state.report.invalid_transitions += 1;
            state.report.closed_responses += 1;
            take_terminal(&mut state, ForecastResponseTerminal::Closed)
        };
        run_transition(predecessor, ForecastResponseTerminal::Closed);
        // the new observer was never installed and therefore receives its own close event
        terminal(ForecastResponseTerminal::Closed);
    }
}

impl Default for NativeHttpConnectionScope {
    /// Create the same empty state as the explicit constructor.
    fn default() -> Self {
        Self::new()
    }
}

/// One finite HTTP body that optionally waits for real header completion.
pub struct NativeTrackedHttpBody {
    body: NativeHttpBody,
    cancelled: bool,
    generation: Option<u64>,
    scope: Option<NativeHttpConnectionScope>,
}

impl Body for NativeTrackedHttpBody {
    type Data = Bytes;
    type Error = Infallible;

    /// Release a tracked body frame only after its independent header write and flush.
    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        // a cancelled or already submitted finite body has no second frame
        if self.cancelled || self.body.is_end_stream() {
            return Poll::Ready(None);
        }
        // ordinary responses directly use the same finite body implementation
        if let (Some(scope), Some(generation)) = (self.scope.clone(), self.generation) {
            match scope.admit_body(generation, context.waker()) {
                BodyAdmission::Wait => return Poll::Pending,
                BodyAdmission::Refused => {
                    self.cancelled = true;
                    return Poll::Ready(None);
                }
                BodyAdmission::Ready => {}
            }
        }
        let result = Pin::new(&mut self.body).poll_frame(context);
        // a tracked non-empty finite body must produce exactly one promised frame
        if matches!(result, Poll::Ready(None))
            && let (Some(scope), Some(generation)) = (self.scope.clone(), self.generation)
        {
            scope.invalidate_body(generation);
            self.cancelled = true;
        }
        result
    }

    /// Preserve exact end-of-stream state only for ordinary or consumed bodies.
    fn is_end_stream(&self) -> bool {
        self.cancelled || self.body.is_end_stream()
    }

    /// Preserve exact content length while a tracked frame waits behind its header barrier.
    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}

impl Drop for NativeTrackedHttpBody {
    /// Close a response abandoned before its frame was handed to Hyper.
    fn drop(&mut self) {
        // submitted bodies remain owned by the I/O adapter until actual finish or close
        if let (Some(scope), Some(generation)) = (&self.scope, self.generation) {
            scope.close_unsubmitted(generation);
        }
    }
}

/// Hyper I/O adapter that observes only bytes accepted by the wrapped socket.
pub struct NativeTrackedHttpIo<Io> {
    io: Io,
    scope: NativeHttpConnectionScope,
}

impl<Io> NativeTrackedHttpIo<Io> {
    /// Bind one wrapped Hyper I/O owner to its connection-local terminal scope.
    pub fn new(io: Io, scope: NativeHttpConnectionScope) -> Self {
        Self { io, scope }
    }
}

impl<Io: Read + Write + Unpin> Read for NativeTrackedHttpIo<Io> {
    /// Forward protocol reads without inspecting or parsing HTTP bytes.
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        // end a poisoned connection rather than accepting a response with ambiguous accounting
        if this.scope.is_poisoned() {
            return Poll::Ready(Err(poisoned_io_error()));
        }
        let result = Pin::new(&mut this.io).poll_read(context, buffer);
        // an ordinary read failure closes any body not already completed
        if matches!(result, Poll::Ready(Err(_))) {
            this.scope.close_pending();
        }
        result
    }
}

impl<Io: Read + Write + Unpin> Write for NativeTrackedHttpIo<Io> {
    /// Credit only scalar bytes accepted by the wrapped socket.
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        // terminate on the first call after an invalid transition was observed
        if this.scope.is_poisoned() {
            return Poll::Ready(Err(poisoned_io_error()));
        }
        let result = Pin::new(&mut this.io).poll_write(context, bytes);
        match &result {
            Poll::Ready(Ok(count)) => this.scope.observe_write(*count),
            Poll::Ready(Err(_)) => this.scope.close_pending(),
            Poll::Pending => {}
        }
        result
    }

    /// Credit the actual aggregate count returned for vectored writes.
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        // terminate on the first call after an invalid transition was observed
        if this.scope.is_poisoned() {
            return Poll::Ready(Err(poisoned_io_error()));
        }
        let result = Pin::new(&mut this.io).poll_write_vectored(context, buffers);
        match &result {
            Poll::Ready(Ok(count)) => this.scope.observe_write(*count),
            Poll::Ready(Err(_)) => this.scope.close_pending(),
            Poll::Pending => {}
        }
        result
    }

    /// Retain the wrapped socket's native vectored-write capability.
    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    /// Open the body gate only after the wrapped socket successfully flushes headers.
    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        // terminate on the first call after an invalid transition was observed
        if this.scope.is_poisoned() {
            return Poll::Ready(Err(poisoned_io_error()));
        }
        let result = Pin::new(&mut this.io).poll_flush(context);
        match &result {
            Poll::Ready(Ok(())) => this.scope.observe_flush(),
            Poll::Ready(Err(_)) => this.scope.close_pending(),
            Poll::Pending => {}
        }
        result
    }

    /// Forward shutdown while closing any response that has not actually finished.
    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.io).poll_shutdown(context);
        // a completed socket shutdown cannot carry any pending representation bytes
        if matches!(result, Poll::Ready(_)) {
            this.scope.close_pending();
        }
        result
    }
}

impl<Io> Drop for NativeTrackedHttpIo<Io> {
    /// Convert connection cancellation or drop into exactly one close terminal.
    fn drop(&mut self) {
        self.scope.close_pending();
    }
}

/// Remove one pending callback and update its report before any callback can re-enter.
fn take_terminal(
    state: &mut NativeHttpConnectionState,
    terminal: ForecastResponseTerminal,
) -> Option<TerminalTransition> {
    let pending = state.pending.take()?;
    // count the same first terminal event that consumes the callback
    match terminal {
        ForecastResponseTerminal::Finished => state.report.finished_responses += 1,
        ForecastResponseTerminal::Closed => state.report.closed_responses += 1,
    }
    Some(TerminalTransition {
        idle_waiter: state.idle_waiter.take(),
        terminal: pending.terminal,
    })
}

/// Wake queued service work and invoke the terminal callback outside the state borrow.
fn run_transition(transition: Option<TerminalTransition>, terminal: ForecastResponseTerminal) {
    let Some(transition) = transition else {
        return;
    };
    // schedule the waiting service before its next poll rechecks the authoritative state
    if let Some(waiter) = transition.idle_waiter {
        waiter.wake();
    }
    // callback re-entry is safe because the state borrow and pending slot are already released
    if let Some(callback) = transition.terminal {
        callback(terminal);
    }
}

/// Build one stable I/O error after fail-closed accounting poisons the connection.
fn poisoned_io_error() -> io::Error {
    io::Error::other("native HTTP response terminal accounting failed")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    enum WriteStep {
        Accept(usize),
        Fail,
        Pending,
    }

    struct FakeIo {
        writes: VecDeque<WriteStep>,
    }

    impl Read for FakeIo {
        /// Leave request input pending because these tests exercise only output ownership.
        fn poll_read(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            _buffer: ReadBufCursor<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }

    impl Write for FakeIo {
        /// Execute one deterministic socket acceptance, failure, or pending result.
        fn poll_write(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            // each poll consumes only an explicitly queued fixture transition
            match self.writes.pop_front().expect("queued write transition") {
                WriteStep::Accept(count) => {
                    assert!(count <= bytes.len());
                    Poll::Ready(Ok(count))
                }
                WriteStep::Fail => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "fixture peer closed",
                ))),
                WriteStep::Pending => Poll::Pending,
            }
        }

        /// Reuse the scalar fixture transition over the aggregate offered vector length.
        fn poll_write_vectored(
            self: Pin<&mut Self>,
            context: &mut Context<'_>,
            buffers: &[IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            let offered = buffers.iter().map(|buffer| buffer.len()).sum::<usize>();
            let bytes = vec![0; offered];
            self.poll_write(context, &bytes)
        }

        /// Complete header flushes deterministically without adding accepted bytes.
        fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        /// Complete fixture shutdown without changing write outcomes.
        fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    /// Poll a body once through the same Hyper body interface used by the listener.
    fn poll_body(
        body: &mut NativeTrackedHttpBody,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        Pin::new(body).poll_frame(context)
    }

    /// Open a tracked body's header barrier through actual wrapped I/O calls.
    fn open_body(
        body: &mut NativeTrackedHttpBody,
        io: &mut NativeTrackedHttpIo<FakeIo>,
        context: &mut Context<'_>,
    ) -> Bytes {
        assert!(poll_body(body, context).is_pending());
        assert!(matches!(
            Pin::new(&mut *io).poll_write(context, b"headers"),
            Poll::Ready(Ok(7))
        ));
        assert!(matches!(
            Pin::new(&mut *io).poll_flush(context),
            Poll::Ready(Ok(()))
        ));
        let Poll::Ready(Some(Ok(frame))) = poll_body(body, context) else {
            panic!("tracked body frame")
        };
        frame.into_data().expect("data frame")
    }

    /// Credit finish only after the wrapped socket accepts every promised body byte.
    #[test]
    fn completes_after_real_full_body_write() {
        let scope = NativeHttpConnectionScope::new();
        let terminals = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&terminals);
        let mut body = scope
            .tracked_body(
                NativeHttpBody::new(Bytes::from_static(b"body")),
                move |event| {
                    observed.borrow_mut().push(event);
                },
            )
            .unwrap();
        let mut io = scope.tracked_io(FakeIo {
            writes: VecDeque::from([WriteStep::Accept(7), WriteStep::Accept(4)]),
        });
        let mut context = Context::from_waker(Waker::noop());
        let bytes = open_body(&mut body, &mut io, &mut context);
        assert!(matches!(
            poll_body(&mut body, &mut context),
            Poll::Ready(None)
        ));
        assert!(terminals.borrow().is_empty());
        assert!(matches!(
            Pin::new(&mut io).poll_write(&mut context, &bytes),
            Poll::Ready(Ok(4))
        ));
        assert_eq!(*terminals.borrow(), [ForecastResponseTerminal::Finished]);
        assert_eq!(scope.report().finished_responses, 1);
        assert_eq!(scope.report().closed_responses, 0);
    }

    /// Convert an accepted prefix followed by an ordinary peer failure into one close.
    #[test]
    fn closes_after_real_prefix_then_broken_pipe() {
        let scope = NativeHttpConnectionScope::new();
        let terminals = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&terminals);
        let mut body = scope
            .tracked_body(
                NativeHttpBody::new(Bytes::from_static(b"body")),
                move |event| {
                    observed.borrow_mut().push(event);
                },
            )
            .unwrap();
        let mut io = scope.tracked_io(FakeIo {
            writes: VecDeque::from([WriteStep::Accept(7), WriteStep::Accept(1), WriteStep::Fail]),
        });
        let mut context = Context::from_waker(Waker::noop());
        let bytes = open_body(&mut body, &mut io, &mut context);
        assert!(matches!(
            Pin::new(&mut io).poll_write(&mut context, &bytes),
            Poll::Ready(Ok(1))
        ));
        assert!(matches!(
            Pin::new(&mut io).poll_write(&mut context, &bytes[1..]),
            Poll::Ready(Err(error)) if error.kind() == io::ErrorKind::BrokenPipe
        ));
        assert_eq!(*terminals.borrow(), [ForecastResponseTerminal::Closed]);
        assert_eq!(scope.report().closed_responses, 1);
        assert_eq!(scope.report().finished_responses, 0);
    }

    /// Retain pending output without fabricating completion, then close it on connection drop.
    #[test]
    fn pending_write_closes_only_when_io_drops() {
        let scope = NativeHttpConnectionScope::new();
        let terminals = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&terminals);
        let mut body = scope
            .tracked_body(
                NativeHttpBody::new(Bytes::from_static(b"body")),
                move |event| {
                    observed.borrow_mut().push(event);
                },
            )
            .unwrap();
        let mut io = scope.tracked_io(FakeIo {
            writes: VecDeque::from([WriteStep::Accept(7), WriteStep::Pending]),
        });
        let mut context = Context::from_waker(Waker::noop());
        let bytes = open_body(&mut body, &mut io, &mut context);
        assert!(
            Pin::new(&mut io)
                .poll_write(&mut context, &bytes)
                .is_pending()
        );
        assert!(terminals.borrow().is_empty());
        drop(io);
        assert_eq!(*terminals.borrow(), [ForecastResponseTerminal::Closed]);
    }

    /// Close an observer when its response body is abandoned before Hyper polls it.
    #[test]
    fn body_drop_before_submission_closes_once() {
        let scope = NativeHttpConnectionScope::new();
        let terminals = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&terminals);
        let body = scope
            .tracked_body(
                NativeHttpBody::new(Bytes::from_static(b"body")),
                move |event| {
                    observed.borrow_mut().push(event);
                },
            )
            .unwrap();
        drop(body);
        assert_eq!(*terminals.borrow(), [ForecastResponseTerminal::Closed]);
        assert_eq!(scope.report().closed_responses, 1);
    }
}
