//! Genuine installed-model admission and retained task ownership for worker sidecars.

use std::io::{self, Write};
use std::sync::Arc;

use weather_contracts::JsonValue;
use weather_database::SerializedJson;
use weather_edge::ArchiveWirePrimitives;
use weather_worker::{
    WorkerProcessFailure, WorkerProcessPhase, WorkerSidecarDiagnostic, WorkerSidecarDiagnosticSink,
};

use crate::{
    ApiMaintenanceCandidateError, ApiMaintenanceReceiptCanonicalValues,
    DatabaseRainCaptureRepositoryBinding, DatabaseWorkerSidecarAdmission,
    DatabaseWorkerSidecarPhase, LoadedWorkerRainServing, NativeApiMaintenanceCandidateFiles,
    NativeModelEvidenceValues, NativeOutboundHttp, NativeProcessClock, NativeRainCollectionPacer,
    NativeRainCollectionTransport, NativeWirePrimitives, NativeWorkerSidecarRuntimePort,
    PublicStationStartupDiagnosticEvent, WorkerConfiguration, WorkerLoadedSidecarAdmission,
    WorkerRainCaptureTask, load_worker_rain_serving,
};

const INSTALLED_APPLICATION_ROOT: &str = "/opt/weather";
const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const RAIN_COLLECTION_POLICY_JSON: &str = concat!(
    "{\"contractVersion\":\"rain-prospective-capture/v1\",",
    "\"siteSlug\":\"ballydidean\",",
    "\"latitude\":47.950429954185445,",
    "\"longitude\":-122.42797012608193,",
    "\"startsAt\":\"2026-09-13T00:00:00.000Z\",",
    "\"expiresAt\":\"2027-10-08T00:00:00.000Z\",",
    "\"stationAccessAuthorized\":true,",
    "\"forecastHours\":49,",
    "\"decisionDelayHours\":8,",
    "\"forecastFirstAttemptHours\":6,",
    "\"forecastSecondAttemptHours\":7,",
    "\"stationCadenceMinutes\":60,",
    "\"stationWindowHours\":2,",
    "\"maximumForecastRequestsPerDay\":8,",
    "\"maximumStationRequestsPerDay\":288,",
    "\"maximumRequestsPerIteration\":2,",
    "\"minimumRequestSpacingMs\":1100,",
    "\"pendingRequestGuardMs\":120000,",
    "\"rateLimitCooldownHours\":24,",
    "\"maximumBodyBytes\":2000000,",
    "\"maximumCompressedBodyBytes\":2100000,",
    "\"maximumStoredBytesPerDay\":8388608,",
    "\"maximumStoredBytes\":2147483648,",
    "\"timeoutMs\":45000,",
    "\"modelEnabled\":false,",
    "\"qualificationEnabled\":false}"
);

/// Concrete prospective rain task prepared without opening a database session or making traffic.
pub type NativeWorkerRainCaptureTask = WorkerRainCaptureTask<
    DatabaseRainCaptureRepositoryBinding,
    NativeProcessClock,
    NativeWirePrimitives,
    NativeRainCollectionTransport,
    NativeRainCollectionPacer,
>;

/// Complete genuine task set and independently loaded admission returned by one startup owner.
pub struct NativeWorkerRequiredSidecars<Tasks> {
    pub loaded: WorkerLoadedSidecarAdmission,
    pub tasks: Tasks,
}

/// Remaining concrete task-construction boundary over the retained PostgreSQL pool.
///
/// Implementations must construct every task with its actual database, provider, model, and
/// private-client owner. They must also load the capture witness, temperature authorization, and
/// optional maintenance clients into `loaded`; there is deliberately no inactive task fallback.
pub trait NativeWorkerSidecarTaskLoader {
    type Tasks;

    /// Load the complete genuine task set after configuration but before database readiness.
    fn load_required_tasks(
        &mut self,
        configuration: &WorkerConfiguration,
        pool: &mut crate::NativePostgresWorkerPool,
        files: &mut NativeApiMaintenanceCandidateFiles,
        values: NativeModelEvidenceValues,
        rain: LoadedWorkerRainServing,
    ) -> Result<NativeWorkerRequiredSidecars<Self::Tasks>, WorkerProcessFailure>;
}

/// Installed sidecar owner retaining native model values and exact root-owned files.
pub struct NativeWorkerSidecarOwner<Loader> {
    files: NativeApiMaintenanceCandidateFiles,
    loader: Loader,
    values: NativeModelEvidenceValues,
}

impl<Loader> NativeWorkerSidecarOwner<Loader> {
    /// Bind the fixed installed application root without reading model or credential material.
    pub fn installed(loader: Loader) -> Result<Self, ApiMaintenanceCandidateError> {
        Ok(Self {
            files: NativeApiMaintenanceCandidateFiles::new(INSTALLED_APPLICATION_ROOT)?,
            loader,
            values: NativeModelEvidenceValues::new(),
        })
    }
}

impl<Loader> NativeWorkerSidecarRuntimePort for NativeWorkerSidecarOwner<Loader>
where
    Loader: NativeWorkerSidecarTaskLoader,
{
    type Phase = DatabaseWorkerSidecarPhase<Loader::Tasks, NativeWorkerSidecarDiagnostics>;

    /// Load the installed rain selection and require a complete genuine task composition.
    fn load_required_sidecar_phase(
        &mut self,
        configuration: &WorkerConfiguration,
        pool: &mut crate::NativePostgresWorkerPool,
    ) -> Result<Self::Phase, WorkerProcessFailure> {
        // preserve the predecessor's temperature-before-rain installed-runtime selection
        let temperature_authorization = crate::load_native_worker_temperature_authorization(
            configuration,
            &self.files,
            self.values,
        );
        let mut codec = NativeWirePrimitives;
        let rain = load_worker_rain_serving(&mut self.files, &mut codec, &self.values);
        let rain_selection = rain.selection();
        let NativeWorkerRequiredSidecars { mut loaded, tasks } = self.loader.load_required_tasks(
            configuration,
            pool,
            &mut self.files,
            self.values,
            rain,
        )?;
        loaded.rain_adjustment_runtime = rain_selection;
        loaded.temperature_canary_authorization = temperature_authorization;
        validate_private_sidecar_admission(configuration, &loaded)?;
        let admission = DatabaseWorkerSidecarAdmission::from_configuration(configuration, loaded);
        Ok(DatabaseWorkerSidecarPhase::new(
            admission,
            tasks,
            NativeWorkerSidecarDiagnostics::new(configuration.version.clone()),
        ))
    }

    /// Retain the task loader's already-complete private-client admission without substituting it.
    fn load_startup_maintenance(
        &mut self,
        _configuration: &WorkerConfiguration,
        _pool: &mut crate::NativePostgresWorkerPool,
        _phase: &mut Self::Phase,
    ) {
        // required task loading already binds admitted clients so the cohesive phase stays immutable
    }
}

/// Structured stderr sink for the optional task phase's bounded aggregate diagnostics.
pub struct NativeWorkerSidecarDiagnostics {
    release: String,
}

impl NativeWorkerSidecarDiagnostics {
    /// Bind diagnostics to the same admitted release used by worker configuration.
    pub const fn new(release: String) -> Self {
        Self { release }
    }
}

impl WorkerSidecarDiagnosticSink for NativeWorkerSidecarDiagnostics {
    /// Emit one exact worker-iteration frame without task or transport error text.
    fn record_sidecar_diagnostic(&mut self, diagnostic: WorkerSidecarDiagnostic) {
        // contain an unavailable process stderr without changing iteration success
        if let Some(bytes) = sidecar_diagnostic_json(&self.release, diagnostic) {
            let _ = io::stderr().lock().write_all(&bytes);
        }
    }
}

/// Compose the actual prospective collector around the shared worker HTTP runtime.
pub fn native_worker_rain_capture_task(
    client: Arc<NativeOutboundHttp>,
    configuration: &WorkerConfiguration,
    values: &mut NativeModelEvidenceValues,
) -> Result<NativeWorkerRainCaptureTask, WorkerProcessFailure> {
    let policy_sha256 = ApiMaintenanceReceiptCanonicalValues::sha256(
        values,
        RAIN_COLLECTION_POLICY_JSON.as_bytes(),
    )
    .ok_or_else(|| sidecar_failure("rain_capture_policy_digest_failed"))?;
    let provider =
        NativeRainCollectionTransport::with_client(client, configuration.tempest_api_key.clone())
            .map_err(|_| sidecar_failure("rain_capture_transport_initialization_failed"))?;
    Ok(WorkerRainCaptureTask::new(
        DatabaseRainCaptureRepositoryBinding::new(
            SerializedJson::admitted(RAIN_COLLECTION_POLICY_JSON),
            policy_sha256,
        ),
        NativeProcessClock,
        NativeWirePrimitives,
        provider,
        NativeRainCollectionPacer,
        configuration.version.clone(),
    ))
}

/// Reject configured private sidecars unless their genuine witness-bound clients were loaded.
fn validate_private_sidecar_admission(
    configuration: &WorkerConfiguration,
    loaded: &WorkerLoadedSidecarAdmission,
) -> Result<(), WorkerProcessFailure> {
    let private_admitted = configuration.adjustment_maintenance_api_origin.is_some()
        && loaded.capture_epoch_witness().is_some();
    // require exact agreement rather than silently replacing a configured private sidecar
    if loaded.adjustment_maintenance_operation_loaded != private_admitted
        || loaded.rain_adjustment_revision_client_loaded != private_admitted
    {
        return Err(sidecar_failure("native_sidecar_private_clients_incomplete"));
    }
    Ok(())
}

/// Encode one optional-sidecar aggregate through the exact worker diagnostic schema.
fn sidecar_diagnostic_json(release: &str, diagnostic: WorkerSidecarDiagnostic) -> Option<Vec<u8>> {
    let error_code = diagnostic
        .error_code
        .as_deref()
        .map_or(JsonValue::Null, |value| text(bounded_error_code(value)));
    let fields = [
        ("count", JsonValue::Number(bounded_usize(diagnostic.count))),
        (
            "duration_ms",
            JsonValue::Number(bounded_milliseconds(diagnostic.duration_ms)),
        ),
        ("error_code", error_code),
        (
            "event",
            text(event_name(PublicStationStartupDiagnosticEvent::SourceRun)),
        ),
        ("release", text(bounded_identifier(release))),
        ("run_id", JsonValue::Null),
        ("source_id", text(bounded_identifier(diagnostic.source_id))),
    ];
    NativeWirePrimitives.encode_ordered_json(&JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    ))
}

/// Retain the ordinary source-run event spelling used by optional sidecar diagnostics.
const fn event_name(event: PublicStationStartupDiagnosticEvent) -> &'static str {
    // keep the shared diagnostic enum closed even though sidecars always emit source runs
    match event {
        PublicStationStartupDiagnosticEvent::SourceRun => "source_run",
        PublicStationStartupDiagnosticEvent::WorkerIteration => "worker_iteration",
    }
}

/// Convert one count to the predecessor's non-negative safe-integer range.
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

/// Preserve an admitted task code or replace it with the closed worker fallback.
fn bounded_error_code(value: &str) -> &str {
    let mut bytes = value.bytes();
    let admitted = bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        && value.len() <= 64
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    // reject arbitrary task text before writing the process diagnostic
    if admitted { value } else { "worker_failure" }
}

/// Preserve the original case-insensitive Unicode diagnostic identifier grammar.
fn bounded_identifier(value: &str) -> &str {
    // recognize the two Unicode simple-fold equivalents accepted by JavaScript's `/iu`
    fn admitted_alphanumeric(value: char) -> bool {
        value.is_ascii_alphanumeric() || matches!(value, '\u{017f}' | '\u{212a}')
    }
    let mut units = value.chars();
    // redact absent, oversized, or invalid identifiers without retaining their contents
    if !units.next().is_some_and(admitted_alphanumeric)
        || value.chars().count() > 128
        || !units.all(|unit| admitted_alphanumeric(unit) || matches!(unit, '.' | '_' | ':' | '-'))
    {
        return "[redacted]";
    }
    value
}

/// Construct one contract string for the native ordered JSON owner.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Construct one bounded runtime startup failure without transport diagnostics.
fn sidecar_failure(code: &str) -> WorkerProcessFailure {
    WorkerProcessFailure {
        code: code.to_owned(),
        phase: WorkerProcessPhase::Runtime,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sidecar diagnostics preserve source-run schema and bounded aggregate values.
    #[test]
    fn sidecar_diagnostic_uses_exact_worker_frame() {
        let bytes = sidecar_diagnostic_json(
            "2026.11.24-1",
            WorkerSidecarDiagnostic {
                count: 2,
                duration_ms: 17,
                error_code: Some("rain_collection_failed".to_owned()),
                source_id: "rain-prospective-capture",
            },
        )
        .unwrap();
        assert_eq!(
            bytes,
            b"{\"count\":2,\"duration_ms\":17,\"error_code\":\"rain_collection_failed\",\"event\":\"source_run\",\"release\":\"2026.11.24-1\",\"run_id\":null,\"source_id\":\"rain-prospective-capture\"}\n"
        );
    }

    /// The database claim identity is bound to the exact frozen TypeScript policy bytes.
    #[test]
    fn rain_policy_keeps_original_json_identity() {
        let expected = "9f630d0fb4672dd44049e8fdda0b2f1efd35a18a955ce1ada39b421ed028ea51";
        let actual = ApiMaintenanceReceiptCanonicalValues::sha256(
            &NativeModelEvidenceValues::new(),
            RAIN_COLLECTION_POLICY_JSON.as_bytes(),
        )
        .unwrap();
        assert_eq!(actual.as_str(), expected);
    }
}
