//! Concrete wall-clock, digest, diagnostic, and timer wiring for the worker process.

use std::io::{self, Write};
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_worker::WorkerProcessFailure;

use crate::{
    AnchoredWorkerProcessScheduler, NativeWirePrimitives, NativeWorkerProcessOneShotTimer,
    NativeWorkerProcessTimerEvents, system_worker_health_milliseconds,
};

/// The process's retained native scheduler with its fixed worker diagnostic writer.
pub type NativeWorkerScheduler = AnchoredWorkerProcessScheduler<
    fn() -> i64,
    NativeWorkerProcessOneShotTimer,
    fn(&str) -> Result<[u8; 4], WorkerProcessFailure>,
    Box<dyn FnMut(&WorkerProcessFailure)>,
>;

/// Bind the real timer lane and its process-specific diagnostics without starting a second runtime.
pub fn native_worker_scheduler(
    release: String,
) -> (NativeWorkerScheduler, NativeWorkerProcessTimerEvents) {
    let (timer, events) = NativeWorkerProcessOneShotTimer::new();
    let diagnostics: Box<dyn FnMut(&WorkerProcessFailure)> = Box::new(move |_| {
        // emit the predecessor's bounded structured diagnostic for a background iteration refusal
        if let Some(bytes) = iteration_failure_diagnostic(&release) {
            let _ = io::stderr().lock().write_all(&bytes);
        }
    });
    (
        AnchoredWorkerProcessScheduler::new(
            native_wall_clock as fn() -> i64,
            timer,
            worker_digest_prefix as fn(&str) -> Result<[u8; 4], WorkerProcessFailure>,
            diagnostics,
        ),
        events,
    )
}

/// Return an invalid Date sentinel when the process time cannot be represented.
fn native_wall_clock() -> i64 {
    system_worker_health_milliseconds().unwrap_or(i64::MAX)
}

/// Derive the exact stable scheduling jitter input from the UTF-8 worker key.
fn worker_digest_prefix(key: &str) -> Result<[u8; 4], WorkerProcessFailure> {
    let digest = RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, key.as_bytes());
    Ok([digest[0], digest[1], digest[2], digest[3]])
}

/// Retain the source identifier grammar and case-insensitive Unicode ASCII equivalents.
fn bounded_identifier(value: &str) -> &str {
    // recognize the fixed case-insensitive ASCII identifier classes
    fn admitted_alphanumeric(value: char) -> bool {
        value.is_ascii_alphanumeric() || matches!(value, '\u{017f}' | '\u{212a}')
    }
    let mut units = value.chars();
    // require the initial alphanumeric and count source Unicode code points
    if !units.next().is_some_and(admitted_alphanumeric)
        || value.chars().count() > 128
        || !units.all(|unit| admitted_alphanumeric(unit) || matches!(unit, '.' | '_' | ':' | '-'))
    {
        return "[redacted]";
    }
    value
}

/// Build the exact predecessor failure frame without embedding provider or database error strings.
fn iteration_failure_diagnostic(release: &str) -> Option<Vec<u8>> {
    // encode source member names and values using the common ordered wire primitive
    fn text(value: &str) -> JsonValue {
        JsonValue::String(value.encode_utf16().collect())
    }
    let items = [
        ("count", JsonValue::Number(0.0)),
        ("duration_ms", JsonValue::Number(0.0)),
        ("error_code", text("worker_iteration_failed")),
        ("event", text("worker_iteration")),
        ("release", text(bounded_identifier(release))),
        ("run_id", JsonValue::Null),
        ("source_id", JsonValue::Null),
    ];
    NativeWirePrimitives.encode_ordered_json(&JsonValue::Object(
        items
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Check the native digest and source diagnostic field order against ordinary fixed input.
    #[test]
    fn native_worker_scheduler_retains_stable_native_jitter_and_diagnostic() {
        assert_eq!(
            worker_digest_prefix("abc").unwrap(),
            [0xba, 0x78, 0x16, 0xbf]
        );
        assert_eq!(
            iteration_failure_diagnostic("2026.11.24-1").unwrap(),
            b"{\"count\":0,\"duration_ms\":0,\"error_code\":\"worker_iteration_failed\",\"event\":\"worker_iteration\",\"release\":\"2026.11.24-1\",\"run_id\":null,\"source_id\":null}\n",
        );
    }
}
