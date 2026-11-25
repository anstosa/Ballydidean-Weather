//! Production ownership shared by native worker startup, iteration, and archive drain.

use std::io::{self, Write};
use std::sync::Arc;

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;
use weather_worker::{
    WorkerProcessFailure, WorkerProcessPhase, WorkerProcessPorts, WorkerProcessRuntimeLoader,
};

use crate::{
    AdmittedDatabaseWorkerIteration, DatabaseWorkerProcessIterationFactory,
    NativeAmbientWeatherTransport, NativeOutboundHttp, NativeOutboundHttpError,
    NativePostgresWorkerFactory, NativePostgresWorkerPool, NativePublicBackfillTime,
    NativeScheduledValues, NativeSupplementalBackfillProvider, NativeWirePrimitives,
    NativeWorkerBackfillOwner, NativeWorkerCompatibilityOrigin, NativeWorkerConfigurationArtifacts,
    NativeWorkerJsonConfiguration, NativeWorkerProcessTimerEvents, NativeWorkerProviderFactory,
    NativeWorkerProviderRunner, NativeWorkerScheduler, ProcessWorkerConfigurationLoader,
    PublicStationStartupBackfill, PublicStationStartupDiagnostic,
    PublicStationStartupDiagnosticEvent, PublicStationStartupDiagnostics, WorkerConfiguration,
    WorkerIterationDrainPort, native_worker_configuration_loader, native_worker_scheduler,
    system_worker_health_milliseconds,
};

const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Actual process configuration owner selected by native worker composition.
pub type NativeWorkerProcessEnvironment = ProcessWorkerConfigurationLoader<
    NativeWorkerJsonConfiguration,
    NativeWorkerCompatibilityOrigin,
    NativeWorkerConfigurationArtifacts,
>;

/// Synchronous database iteration owner with its real process clock and drain boundary.
pub type NativeWorkerProcessIterationFactory = DatabaseWorkerProcessIterationFactory<
    NativeScheduledValues,
    fn() -> i64,
    NativeWorkerIterationDrain,
>;

/// Public-station startup owners sharing the process HTTP runtime and diagnostic sink.
pub type NativeWorkerStartupArchiveOwners = PublicStationStartupBackfill<
    NativeAmbientWeatherTransport,
    NativeSupplementalBackfillProvider,
    NativePublicBackfillTime,
    NativePublicStationStartupDiagnostics,
>;

/// Complete native startup ports except for the explicitly injected sidecar runtime owner.
pub type NativeWorkerRuntimePorts<Sidecars> = WorkerProcessPorts<
    NativeWorkerProcessEnvironment,
    NativePostgresWorkerFactory,
    NativeWorkerRuntimeLoader<Sidecars>,
    NativeWorkerScheduler,
    NativeWorkerStartupArchiveOwners,
    NativeWorkerProcessIterationFactory,
>;

/// Missing production composition boundary for required model and optional sidecar owners.
///
/// Implementations must load the genuine temperature, rain, target, inference, and maintenance
/// owners. This port intentionally has no inactive fallback because configuration may admit work.
pub trait NativeWorkerSidecarRuntimePort {
    type Phase;

    /// Load all required model and sidecar state before database readiness is asserted.
    fn load_required_sidecar_phase(
        &mut self,
        configuration: &WorkerConfiguration,
        pool: &mut NativePostgresWorkerPool,
    ) -> Result<Self::Phase, WorkerProcessFailure>;

    /// Load admitted maintenance clients into the retained phase after required state exists.
    fn load_startup_maintenance(
        &mut self,
        configuration: &WorkerConfiguration,
        pool: &mut NativePostgresWorkerPool,
        phase: &mut Self::Phase,
    );
}

/// Required runtime loader joining actual provider dispatchers to genuine sidecar ownership.
pub struct NativeWorkerRuntimeLoader<Sidecars> {
    providers: NativeWorkerProviderFactory,
    sidecars: Sidecars,
}

impl<Sidecars> NativeWorkerRuntimeLoader<Sidecars> {
    /// Retain the shared provider factory and required sidecar loader until configuration exists.
    pub const fn new(providers: NativeWorkerProviderFactory, sidecars: Sidecars) -> Self {
        Self {
            providers,
            sidecars,
        }
    }
}

impl<Sidecars> WorkerProcessRuntimeLoader<WorkerConfiguration, NativePostgresWorkerPool>
    for NativeWorkerRuntimeLoader<Sidecars>
where
    Sidecars: NativeWorkerSidecarRuntimePort,
{
    type Runtime = AdmittedDatabaseWorkerIteration<NativeWorkerProviderRunner, Sidecars::Phase>;

    /// Load real provider dispatchers and required sidecars before readiness in source order.
    fn load_required_runtime(
        &mut self,
        configuration: &WorkerConfiguration,
        pool: &mut NativePostgresWorkerPool,
    ) -> Result<Self::Runtime, WorkerProcessFailure> {
        let dispatcher =
            self.providers
                .build(configuration)
                .map_err(|failure| WorkerProcessFailure {
                    code: failure.code,
                    phase: WorkerProcessPhase::Runtime,
                })?;
        let sidecar_phase = self
            .sidecars
            .load_required_sidecar_phase(configuration, pool)?;
        Ok(AdmittedDatabaseWorkerIteration {
            dispatcher,
            sidecar_phase,
        })
    }

    /// Delegate optional maintenance loading without replacing the admitted sidecar phase.
    fn load_startup_maintenance(
        &mut self,
        configuration: &WorkerConfiguration,
        pool: &mut NativePostgresWorkerPool,
        runtime: &mut Self::Runtime,
    ) {
        self.sidecars
            .load_startup_maintenance(configuration, pool, &mut runtime.sidecar_phase);
    }
}

/// Actual structured stderr sink shared with the public-station startup thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativePublicStationStartupDiagnostics;

impl PublicStationStartupDiagnostics for NativePublicStationStartupDiagnostics {
    /// Emit one bounded newline-delimited worker diagnostic without provider error material.
    fn record(&self, diagnostic: PublicStationStartupDiagnostic<'_>) {
        // keep sink failure contained like the predecessor's process stderr write
        if let Some(bytes) = startup_diagnostic_json(diagnostic) {
            let _ = io::stderr().lock().write_all(&bytes);
        }
    }
}

/// Drain owner for iterations that execute synchronously on the retained process lane.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWorkerIterationDrain;

impl WorkerIterationDrainPort<NativePostgresWorkerPool> for NativeWorkerIterationDrain {
    /// Complete immediately because the synchronous trigger returns only after database work ends.
    fn drain_iteration(&mut self, _database: &mut NativePostgresWorkerPool) {}
}

/// Compose every completed native worker owner around one HTTP runtime and PostgreSQL factory.
pub fn native_worker_runtime_ports<Sidecars>(
    release: String,
    sidecars: Sidecars,
) -> Result<
    (
        NativeWorkerRuntimePorts<Sidecars>,
        NativeWorkerProcessTimerEvents,
    ),
    NativeOutboundHttpError,
> {
    native_worker_runtime_ports_with_client(release, Arc::new(NativeOutboundHttp::new()?), sidecars)
}

/// Reuse the already selected HTTP owner for scheduled providers, startup and sidecar work.
pub fn native_worker_runtime_ports_with_client<Sidecars>(
    release: String,
    client: Arc<NativeOutboundHttp>,
    sidecars: Sidecars,
) -> Result<
    (
        NativeWorkerRuntimePorts<Sidecars>,
        NativeWorkerProcessTimerEvents,
    ),
    NativeOutboundHttpError,
> {
    let providers = NativeWorkerProviderFactory::with_client(Arc::clone(&client));
    let archive_owners = NativeWorkerBackfillOwner::with_client(client)
        .public_station_startup(Arc::new(NativePublicStationStartupDiagnostics))?;
    let startup = PublicStationStartupBackfill::new(
        archive_owners.ambient,
        archive_owners.supplemental,
        archive_owners.time,
        archive_owners.diagnostics,
    );
    let (scheduler, timer_events) = native_worker_scheduler(release);
    let ports = WorkerProcessPorts {
        backfill: startup,
        environment: native_worker_configuration_loader(),
        iteration_factory: DatabaseWorkerProcessIterationFactory::new(
            NativeScheduledValues::new(),
            native_worker_iteration_clock as fn() -> i64,
            NativeWorkerIterationDrain,
        ),
        loaders: NativeWorkerRuntimeLoader::new(providers, sidecars),
        pool_factory: NativePostgresWorkerFactory::new(),
        scheduler,
    };
    Ok((ports, timer_events))
}

/// Read the process clock used for due-source discovery and successful completion timestamps.
fn native_worker_iteration_clock() -> i64 {
    system_worker_health_milliseconds().unwrap_or(i64::MAX)
}

/// Encode the predecessor's exact ordered startup diagnostic through native JSON primitives.
fn startup_diagnostic_json(diagnostic: PublicStationStartupDiagnostic<'_>) -> Option<Vec<u8>> {
    // project the closed event enum into the predecessor's exact wire spelling
    let event = match diagnostic.event {
        PublicStationStartupDiagnosticEvent::SourceRun => "source_run",
        PublicStationStartupDiagnosticEvent::WorkerIteration => "worker_iteration",
    };
    let error_code = diagnostic
        .error_code
        .map(bounded_error_code)
        .map_or(JsonValue::Null, text);
    let source_id = diagnostic
        .source_id
        .map(bounded_identifier)
        .map_or(JsonValue::Null, text);
    let fields = [
        ("count", JsonValue::Number(bounded_usize(diagnostic.count))),
        (
            "duration_ms",
            JsonValue::Number(bounded_milliseconds(diagnostic.duration_ms)),
        ),
        ("error_code", error_code),
        ("event", text(event)),
        ("release", text(bounded_identifier(diagnostic.release))),
        ("run_id", JsonValue::Null),
        ("source_id", source_id),
    ];
    NativeWirePrimitives.encode_ordered_json(&JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    ))
}

/// Convert one count to the predecessor's non-negative safe-integer diagnostic range.
fn bounded_usize(value: usize) -> f64 {
    u64::try_from(value)
        .unwrap_or(u64::MAX)
        .min(MAXIMUM_SAFE_INTEGER) as f64
}

/// Convert one elapsed value to the predecessor's non-negative safe-integer range.
fn bounded_milliseconds(value: i64) -> f64 {
    u64::try_from(value)
        .unwrap_or_default()
        .min(MAXIMUM_SAFE_INTEGER) as f64
}

/// Preserve an admitted worker error code or use the original categorical fallback.
fn bounded_error_code(value: &str) -> &str {
    let mut bytes = value.bytes();
    let admitted = bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        && value.len() <= 64
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    // replace text outside the closed diagnostic grammar
    if admitted { value } else { "worker_failure" }
}

/// Preserve the original case-insensitive Unicode identifier grammar and bound.
fn bounded_identifier(value: &str) -> &str {
    // recognize the fixed case-insensitive ASCII identifier classes
    fn admitted_alphanumeric(value: char) -> bool {
        value.is_ascii_alphanumeric() || matches!(value, '\u{017f}' | '\u{212a}')
    }
    let mut units = value.chars();
    // redact missing, oversized, or invalid identifiers before emitting process diagnostics
    if !units.next().is_some_and(admitted_alphanumeric)
        || value.chars().count() > 128
        || !units.all(|unit| admitted_alphanumeric(unit) || matches!(unit, '.' | '_' | ':' | '-'))
    {
        return "[redacted]";
    }
    value
}

/// Construct one UTF-16 contract string for the native ordered JSON encoder.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Startup diagnostics retain field order, nulls, bounds, and identifier redaction.
    #[test]
    fn startup_diagnostic_uses_exact_native_worker_frame() {
        let bytes = startup_diagnostic_json(PublicStationStartupDiagnostic {
            count: 3,
            duration_ms: 9,
            error_code: Some("public_station_backfill_failed"),
            event: PublicStationStartupDiagnosticEvent::SourceRun,
            release: "2026.11.24-1",
            source_id: Some("ambient:backfill"),
        })
        .unwrap();
        assert_eq!(
            bytes,
            b"{\"count\":3,\"duration_ms\":9,\"error_code\":\"public_station_backfill_failed\",\"event\":\"source_run\",\"release\":\"2026.11.24-1\",\"run_id\":null,\"source_id\":\"ambient:backfill\"}\n"
        );

        let redacted = startup_diagnostic_json(PublicStationStartupDiagnostic {
            count: usize::MAX,
            duration_ms: -1,
            error_code: Some("invalid-code"),
            event: PublicStationStartupDiagnosticEvent::WorkerIteration,
            release: "invalid release",
            source_id: None,
        })
        .unwrap();
        assert_eq!(
            redacted,
            b"{\"count\":9007199254740991,\"duration_ms\":0,\"error_code\":\"worker_failure\",\"event\":\"worker_iteration\",\"release\":\"[redacted]\",\"run_id\":null,\"source_id\":null}\n"
        );
    }
}
