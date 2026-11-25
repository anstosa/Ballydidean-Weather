use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::JsonValue;
use weather_edge::{
    AdmittedForecastScheduleCapture, ForecastArchiveAppendOutcome, ForecastCaptureChannel,
    ForecastCaptureSource, ForecastCaptureVersion, ForecastEvidenceOutcome, ForecastEvidenceWindow,
    ForecastResponseEvidenceStore, ForecastResponseTerminal, ForecastScheduleArchive,
    ForecastScheduledEvidenceArchive, PublicServingApplicationBody,
    PublicServingApplicationHeaders, PublicServingApplicationResponse, PublicServingHeaderProfile,
    RevisionArchiveCanonicalClock, observe_forecast_response_terminal,
    persist_forecast_response_capture, prepare_forecast_response_capture,
};

const NOW: &str = "2026-11-23T20:10:05.500Z";
const DUE: &str = "capture/2026-11-23T20:10:00.000Z";
const BODY: &[u8] = b"{\"data\":[]}\n";

#[derive(Clone, Copy)]
enum ArchiveMode {
    Ready,
    Gap,
    Unavailable,
    Error,
    Disabled,
}

struct Case {
    label: &'static str,
    query: &'static [(&'static str, &'static str)],
    scheduled: bool,
    terminal: ForecastResponseTerminal,
    preparation: bool,
    outcome: ForecastEvidenceOutcome,
    archive: ArchiveMode,
}

struct Store<'a> {
    case: &'a Case,
    events: &'a RefCell<Vec<String>>,
    window: Option<ForecastEvidenceWindow>,
    version: Option<ForecastCaptureVersion>,
    close_without_finish: usize,
    write_failures: usize,
}

impl ForecastResponseEvidenceStore for Store<'_> {
    type Prepared = &'static str;
    type Error = ();

    /// verify the bounded pre-send evidence source and capture generation.
    fn prepare(
        &mut self,
        source: ForecastCaptureSource<'_>,
        window: Option<ForecastEvidenceWindow>,
        version: ForecastCaptureVersion,
    ) -> Option<Self::Prepared> {
        match source {
            ForecastCaptureSource::Encoded(bytes) => assert_eq!(bytes, BODY),
            ForecastCaptureSource::Projected(value) => {
                assert_eq!(value, &JsonValue::Object(Vec::new()))
            }
        }
        self.window = window;
        self.version = Some(version);
        self.case.preparation.then_some("prepared")
    }

    /// commit only after the real listener events changed the trace.
    fn commit(
        &mut self,
        prepared: &Self::Prepared,
        first_edge_committed_at: &str,
    ) -> Result<ForecastEvidenceOutcome, Self::Error> {
        assert_eq!(*prepared, "prepared");
        self.events
            .borrow_mut()
            .push(format!("commit:{first_edge_committed_at}"));
        // preserve the ordinary filesystem failure used by the independent oracle
        if self.case.outcome == ForecastEvidenceOutcome::WriteFailed {
            return Err(());
        }
        Ok(self.case.outcome)
    }

    /// count the early close exactly once.
    fn note_close_without_finish(&mut self) {
        self.close_without_finish += 1;
    }

    /// count the one post-finish writer failure.
    fn note_write_failure(&mut self) {
        self.write_failures += 1;
    }
}

struct Clock<'a>(&'a RefCell<Vec<String>>, Cell<usize>);

impl RevisionArchiveCanonicalClock for Clock<'_> {
    /// sample the source clock exactly on successful response finish.
    fn canonical_now(&mut self) -> Option<String> {
        self.1.set(self.1.get() + 1);
        self.0.borrow_mut().push("clock".to_owned());
        Some(NOW.to_owned())
    }
}

struct Archive<'a> {
    mode: ArchiveMode,
    events: &'a RefCell<Vec<String>>,
}

impl ForecastScheduledEvidenceArchive<&str> for Archive<'_> {
    type Error = ();

    /// append only a genuine commit with its original scheduler key.
    fn append_committed(
        &mut self,
        due_key: &str,
        prepared: &&str,
    ) -> Result<ForecastArchiveAppendOutcome, Self::Error> {
        assert_eq!(*prepared, "prepared");
        self.events.borrow_mut().push(format!("append:{due_key}"));
        match self.mode {
            ArchiveMode::Gap => Ok(ForecastArchiveAppendOutcome::GapPersistenceRefused),
            ArchiveMode::Error => Err(()),
            _ => Ok(ForecastArchiveAppendOutcome::Settled),
        }
    }
}

/// build one real public owner result with or without a response body.
fn response(tracked: bool) -> PublicServingApplicationResponse {
    PublicServingApplicationResponse {
        body: PublicServingApplicationBody::Bytes(tracked.then(|| BODY.to_vec())),
        header_profile: PublicServingHeaderProfile::Standard,
        headers: PublicServingApplicationHeaders {
            allow: None,
            cache_control: "no-store".to_owned(),
            content_length: BODY.len(),
            content_type: "application/json; charset=utf-8".to_owned(),
            vary: Some("Accept"),
        },
        status_code: 200,
        track_successful_forecast_response: tracked,
    }
}

/// construct the same trace as the independently executed listener callback.
fn run(case: &Case) -> String {
    let events = RefCell::new(Vec::new());
    let mut store = Store {
        case,
        events: &events,
        window: None,
        version: None,
        close_without_finish: 0,
        write_failures: 0,
    };
    let mut clock = Clock(&events, Cell::new(0));
    let mut archive = Archive {
        mode: case.archive,
        events: &events,
    };
    let mut pending = prepare_forecast_response_capture(
        &response(true),
        case.scheduled.then_some(&JsonValue::Object(Vec::new())),
        case.query,
        case.scheduled,
        case.scheduled
            .then(|| AdmittedForecastScheduleCapture::admitted(DUE)),
        &mut store,
    );
    let expected_version = if case.scheduled {
        ForecastCaptureVersion::ChannelBound(ForecastCaptureChannel::SchedulerRequest)
    } else {
        ForecastCaptureVersion::Legacy
    };
    assert_eq!(store.version, Some(expected_version));
    assert!(events.borrow().is_empty());
    events
        .borrow_mut()
        .extend(["headers".to_owned(), "body".to_owned()]);
    let work =
        observe_forecast_response_terminal(&mut pending, case.terminal, &mut clock, &mut store)
            .unwrap();
    let later = match case.terminal {
        ForecastResponseTerminal::Finished => ForecastResponseTerminal::Closed,
        ForecastResponseTerminal::Closed => ForecastResponseTerminal::Finished,
    };
    assert!(
        observe_forecast_response_terminal(&mut pending, later, &mut clock, &mut store).is_none()
    );
    assert!(!events.borrow().iter().any(
        // no store commit can occur synchronously in the native listener event
        |entry| entry.starts_with("commit:"),
    ));
    let archive_state = match case.archive {
        ArchiveMode::Disabled => ForecastScheduleArchive::Disabled,
        ArchiveMode::Unavailable => ForecastScheduleArchive::Unavailable,
        _ => ForecastScheduleArchive::Ready(&mut archive),
    };
    let completed = persist_forecast_response_capture(work, &mut store, archive_state);
    assert_eq!(
        store.close_without_finish,
        usize::from(case.terminal == ForecastResponseTerminal::Closed)
    );
    assert_eq!(
        store.write_failures,
        usize::from(case.outcome == ForecastEvidenceOutcome::WriteFailed)
    );
    let evidence_event = format!("evidence:{}", completed.evidence.wire_value());
    // place the returned evidence at the callback boundary before archive settlement
    let mut captured = events.borrow().clone();
    let offset = captured
        .iter()
        .position(
            // an archive write occurs strictly after the evidence result
            |entry| entry.starts_with("append:"),
        )
        .unwrap_or(captured.len());
    captured.insert(offset, evidence_event);
    let scheduled = completed.scheduled.as_ref().map_or(
        "-",
        // compare the actual scheduler result rather than raw storage exceptions
        |saved| {
            assert_eq!(saved.slot.due_key(), DUE);
            saved.settlement.wire_value()
        },
    );
    format!(
        "{}|{}|{scheduled}",
        store.window.map_or("-", ForecastEvidenceWindow::wire_value),
        captured.join(",")
    )
}

/// prove byte-independent sequencing, exact windows, and scheduler fallback against the real Node callbacks.
#[test]
fn forecast_capture_matches_node_finish_commit_and_archive_settlement() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/forecast-response-capture-node-oracle.mjs");
    let output = Command::new("node").arg(script).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let witness = String::from_utf8(output.stdout).unwrap();
    let cases = [
        Case {
            label: "public",
            query: &[],
            scheduled: false,
            terminal: ForecastResponseTerminal::Finished,
            preparation: true,
            outcome: ForecastEvidenceOutcome::Created,
            archive: ArchiveMode::Ready,
        },
        Case {
            label: "scheduled_gap",
            query: &[("window", "overnight")],
            scheduled: true,
            terminal: ForecastResponseTerminal::Finished,
            preparation: true,
            outcome: ForecastEvidenceOutcome::Duplicate,
            archive: ArchiveMode::Gap,
        },
        Case {
            label: "scheduled_error",
            query: &[("days", "5")],
            scheduled: true,
            terminal: ForecastResponseTerminal::Finished,
            preparation: true,
            outcome: ForecastEvidenceOutcome::Created,
            archive: ArchiveMode::Error,
        },
        Case {
            label: "unavailable",
            query: &[("days", "10")],
            scheduled: true,
            terminal: ForecastResponseTerminal::Finished,
            preparation: true,
            outcome: ForecastEvidenceOutcome::Created,
            archive: ArchiveMode::Unavailable,
        },
        Case {
            label: "refused",
            query: &[("days", "1")],
            scheduled: false,
            terminal: ForecastResponseTerminal::Finished,
            preparation: false,
            outcome: ForecastEvidenceOutcome::Created,
            archive: ArchiveMode::Disabled,
        },
        Case {
            label: "aborted",
            query: &[("days", "1")],
            scheduled: true,
            terminal: ForecastResponseTerminal::Closed,
            preparation: true,
            outcome: ForecastEvidenceOutcome::Created,
            archive: ArchiveMode::Ready,
        },
        Case {
            label: "write_failed",
            query: &[("days", "5")],
            scheduled: true,
            terminal: ForecastResponseTerminal::Finished,
            preparation: true,
            outcome: ForecastEvidenceOutcome::WriteFailed,
            archive: ArchiveMode::Ready,
        },
    ];
    // compare each ordinary lifecycle with its original callback behavior
    for case in cases {
        let prefix = format!("{}=", case.label);
        let expected = witness
            .lines()
            .find_map(
                // compare one independently produced oracle record
                |line| line.strip_prefix(&prefix),
            )
            .unwrap();
        assert_eq!(run(&case), expected, "{}", case.label);
    }
}
