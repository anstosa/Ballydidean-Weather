//! Write one generated worker health projection as the existing stdout JSON line.

use std::io::Write;

use weather_contracts::{Nullable, WorkerHealth as WorkerHealthRepresentation};
use weather_edge::ArchiveWirePrimitives;
use weather_worker::WorkerHealth;

use crate::api_service_json_values::{boolean, nullable, object, text};

use crate::worker_database_boot::WorkerHealthOutput;

/// Approved canonical UTC representation boundary for heartbeat epoch values.
pub trait WorkerHealthUtcRepresentation {
    type Error;

    /// Represent one epoch millisecond value as canonical UTC without a local time codec.
    fn utc_milliseconds(&mut self, epoch_milliseconds: i64) -> Result<String, Self::Error>;
}

/// Approved borrowed serializer for one complete worker-health JSON line.
pub trait WorkerHealthJsonLineEncoder {
    type Error;

    /// Encode one generated DTO as complete JSON bytes ending in one line feed.
    fn encode<'a>(
        &'a mut self,
        health: &WorkerHealthRepresentation,
    ) -> Result<&'a [u8], Self::Error>;
}

/// Resolve the original worker heartbeat UTC text through the shared runtime primitive.
pub struct NativeWorkerHealthUtc<P>(P);

impl<P> NativeWorkerHealthUtc<P> {
    /// Bind the selected time primitive without creating another time implementation.
    pub const fn new(primitives: P) -> Self {
        Self(primitives)
    }
}

impl<P: ArchiveWirePrimitives> WorkerHealthUtcRepresentation for NativeWorkerHealthUtc<P> {
    type Error = ();

    /// Render the generated health DTO's canonical UTC timestamp.
    fn utc_milliseconds(&mut self, epoch_milliseconds: i64) -> Result<String, Self::Error> {
        self.0.format_utc_milliseconds(epoch_milliseconds).ok_or(())
    }
}

/// Serialize the generated TypeScript worker health contract in producer member order.
pub struct NativeWorkerHealthJsonLineEncoder<P> {
    primitives: P,
    bytes: Vec<u8>,
}

impl<P> NativeWorkerHealthJsonLineEncoder<P> {
    /// Retain one selected serializer and one output buffer.
    pub const fn new(primitives: P) -> Self {
        Self {
            primitives,
            bytes: Vec::new(),
        }
    }
}

impl<P: ArchiveWirePrimitives> WorkerHealthJsonLineEncoder
    for NativeWorkerHealthJsonLineEncoder<P>
{
    type Error = ();

    /// Preserve the predecessor's explicit null values and single trailing line feed.
    fn encode<'a>(
        &'a mut self,
        health: &WorkerHealthRepresentation,
    ) -> Result<&'a [u8], Self::Error> {
        let value = object([
            (
                "lastLoopAt",
                nullable(&health.lastLoopAt, |value| text(value)),
            ),
            (
                "lastSuccessAt",
                nullable(&health.lastSuccessAt, |value| text(value)),
            ),
            ("live", boolean(health.live)),
            ("ready", boolean(health.ready)),
            ("stale", boolean(health.stale)),
        ]);
        self.bytes = self.primitives.encode_ordered_json(&value).ok_or(())?;
        Ok(&self.bytes)
    }
}

/// Bind the actual worker-health writer to the shared time and JSON owner once.
pub fn native_worker_health_output<Writer, P: ArchiveWirePrimitives + Clone>(
    writer: Writer,
    primitives: P,
) -> JsonLineWorkerHealthOutput<
    Writer,
    NativeWorkerHealthUtc<P>,
    NativeWorkerHealthJsonLineEncoder<P>,
> {
    JsonLineWorkerHealthOutput::new(
        writer,
        NativeWorkerHealthUtc::new(primitives.clone()),
        NativeWorkerHealthJsonLineEncoder::new(primitives),
    )
}

/// Closed output failure before the process selects its one-shot exit code.
#[derive(Debug)]
pub enum WorkerHealthJsonLineError<UtcError, EncoderError> {
    Utc(UtcError),
    Encode(EncoderError),
    MissingLineFeed,
    Write(std::io::Error),
}

/// Concrete health output retaining the writer, UTC owner, and borrowed encoder scratch.
pub struct JsonLineWorkerHealthOutput<Writer, Utc, Encoder> {
    writer: Writer,
    utc: Utc,
    encoder: Encoder,
}

impl<Writer, Utc, Encoder> JsonLineWorkerHealthOutput<Writer, Utc, Encoder> {
    /// Bind one process-owned writer to approved time and JSON representation boundaries.
    pub const fn new(writer: Writer, utc: Utc, encoder: Encoder) -> Self {
        Self {
            writer,
            utc,
            encoder,
        }
    }

    /// Recover all injected owners after a one-shot health output attempt.
    pub fn into_parts(self) -> (Writer, Utc, Encoder) {
        (self.writer, self.utc, self.encoder)
    }
}

impl<Writer, Utc, Encoder> WorkerHealthOutput for JsonLineWorkerHealthOutput<Writer, Utc, Encoder>
where
    Writer: Write,
    Utc: WorkerHealthUtcRepresentation,
    Encoder: WorkerHealthJsonLineEncoder,
{
    type Error = WorkerHealthJsonLineError<Utc::Error, Encoder::Error>;

    /// Project generated nullable timestamps and write one complete represented line.
    fn write_health(&mut self, health: WorkerHealth) -> Result<(), Self::Error> {
        let represented = WorkerHealthRepresentation {
            lastLoopAt: optional_utc(health.last_loop_at_ms, &mut self.utc)
                .map_err(WorkerHealthJsonLineError::Utc)?,
            lastSuccessAt: optional_utc(health.last_success_at_ms, &mut self.utc)
                .map_err(WorkerHealthJsonLineError::Utc)?,
            live: health.live,
            ready: health.ready,
            stale: health.stale,
        };
        let line = self
            .encoder
            .encode(&represented)
            .map_err(WorkerHealthJsonLineError::Encode)?;
        // refuse an incomplete representation instead of adding a second JSON/time protocol
        if !line.ends_with(b"\n") {
            return Err(WorkerHealthJsonLineError::MissingLineFeed);
        }
        self.writer
            .write_all(line)
            .map_err(WorkerHealthJsonLineError::Write)
    }
}

/// Project one optional heartbeat epoch through the approved UTC owner.
fn optional_utc<Utc: WorkerHealthUtcRepresentation>(
    value: Option<i64>,
    utc: &mut Utc,
) -> Result<Nullable<String>, Utc::Error> {
    match value {
        Some(value) => utc.utc_milliseconds(value).map(Nullable::Value),
        None => Ok(Nullable::Null),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const LINE: &[u8] = b"{\"lastLoopAt\":\"1000-utc\",\"lastSuccessAt\":\"900-utc\",\"live\":true,\"ready\":true,\"stale\":false}\n";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FixtureError {
        Utc,
        Encode,
    }

    #[derive(Default)]
    struct Utc {
        fail: bool,
        values: Vec<i64>,
    }

    impl WorkerHealthUtcRepresentation for Utc {
        type Error = FixtureError;

        /// Retain each epoch and return its approved fixture representation.
        fn utc_milliseconds(&mut self, epoch_milliseconds: i64) -> Result<String, Self::Error> {
            // inject one ordinary clock representation failure
            if self.fail {
                return Err(FixtureError::Utc);
            }
            self.values.push(epoch_milliseconds);
            Ok(format!("{epoch_milliseconds}-utc"))
        }
    }

    struct Encoder {
        fail: bool,
        line: Vec<u8>,
        observed: Option<WorkerHealthRepresentation>,
    }

    impl Encoder {
        /// Build one successful complete-line fixture encoder.
        fn complete() -> Self {
            Self {
                fail: false,
                line: LINE.to_vec(),
                observed: None,
            }
        }
    }

    impl WorkerHealthJsonLineEncoder for Encoder {
        type Error = FixtureError;

        /// Retain the generated DTO and return the injected serializer scratch.
        fn encode<'a>(
            &'a mut self,
            health: &WorkerHealthRepresentation,
        ) -> Result<&'a [u8], Self::Error> {
            self.observed = Some(health.clone());
            // inject one ordinary serializer failure before writer access
            if self.fail {
                return Err(FixtureError::Encode);
            }
            Ok(&self.line)
        }
    }

    /// Project both heartbeat instants and write one complete JSON line to a Vec sink.
    #[test]
    fn writes_nonempty_health_projection_once() {
        let mut output =
            JsonLineWorkerHealthOutput::new(Vec::new(), Utc::default(), Encoder::complete());
        output
            .write_health(WorkerHealth {
                last_loop_at_ms: Some(1_000),
                last_success_at_ms: Some(900),
                live: true,
                ready: true,
                stale: false,
            })
            .unwrap();
        let (writer, utc, encoder) = output.into_parts();
        assert_eq!(writer, LINE);
        assert_eq!(utc.values, [1_000, 900]);
        assert_eq!(
            encoder.observed,
            Some(WorkerHealthRepresentation {
                lastLoopAt: Nullable::Value("1000-utc".to_owned()),
                lastSuccessAt: Nullable::Value("900-utc".to_owned()),
                live: true,
                ready: true,
                stale: false,
            })
        );
    }

    /// Project absent heartbeat instants as generated null values without UTC calls.
    #[test]
    fn writes_nullable_health_projection_without_time_calls() {
        let mut encoder = Encoder::complete();
        encoder.line = b"{\"lastLoopAt\":null,\"lastSuccessAt\":null,\"live\":true,\"ready\":false,\"stale\":true}\n".to_vec();
        let mut output = JsonLineWorkerHealthOutput::new(Vec::new(), Utc::default(), encoder);
        output
            .write_health(WorkerHealth {
                last_loop_at_ms: None,
                last_success_at_ms: None,
                live: true,
                ready: false,
                stale: true,
            })
            .unwrap();
        let (_, utc, encoder) = output.into_parts();
        assert!(utc.values.is_empty());
        assert_eq!(
            encoder.observed,
            Some(WorkerHealthRepresentation {
                lastLoopAt: Nullable::Null,
                lastSuccessAt: Nullable::Null,
                live: true,
                ready: false,
                stale: true,
            })
        );
    }

    /// Stop before writer access on UTC, encoding, or incomplete-line failure.
    #[test]
    fn representation_failures_leave_vec_sink_empty() {
        let cases = [
            (true, false, LINE.to_vec()),
            (false, true, LINE.to_vec()),
            (false, false, b"{}".to_vec()),
        ];
        // exercise each ordinary pre-writer failure independently
        for (utc_fail, encode_fail, line) in cases {
            let mut output = JsonLineWorkerHealthOutput::new(
                Vec::new(),
                Utc {
                    fail: utc_fail,
                    ..Utc::default()
                },
                Encoder {
                    fail: encode_fail,
                    line,
                    observed: None,
                },
            );
            let error = output
                .write_health(WorkerHealth {
                    last_loop_at_ms: Some(1_000),
                    last_success_at_ms: None,
                    live: true,
                    ready: false,
                    stale: true,
                })
                .expect_err("fixture must refuse before writing");
            assert!(matches!(
                error,
                WorkerHealthJsonLineError::Utc(_)
                    | WorkerHealthJsonLineError::Encode(_)
                    | WorkerHealthJsonLineError::MissingLineFeed
            ));
            assert!(output.into_parts().0.is_empty());
        }
    }

    struct FailingWriter {
        calls: Cell<usize>,
    }

    impl Write for FailingWriter {
        /// Refuse the complete represented line on its first writer call.
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            self.calls.set(self.calls.get() + 1);
            Err(std::io::Error::other("fixture write failure"))
        }

        /// Retain source behavior without adding a flush operation.
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Surface one sink failure after a single attempted complete-line write.
    #[test]
    fn writer_failure_is_returned_after_one_attempt() {
        let mut output = JsonLineWorkerHealthOutput::new(
            FailingWriter {
                calls: Cell::new(0),
            },
            Utc::default(),
            Encoder::complete(),
        );
        let error = output
            .write_health(WorkerHealth {
                last_loop_at_ms: Some(1_000),
                last_success_at_ms: Some(900),
                live: true,
                ready: true,
                stale: false,
            })
            .expect_err("fixture writer must fail");
        assert!(matches!(error, WorkerHealthJsonLineError::Write(_)));
        assert_eq!(output.into_parts().0.calls.get(), 1);
    }
}
