//! One-shot process binding for retained historical backfill commands.

use weather_contracts::{PublicStationConfiguration, TempestConfiguration, TideConfiguration};
use weather_worker::{
    AmbientWeatherBackfillProvider, HistoricalBackfillArguments, HistoricalBackfillCommand,
    HistoricalBackfillConfigurationFamily, HistoricalBackfillReport,
    HistoricalBackfillSourceConfiguration, PublicStationBackfillTimeAdapter,
    SupplementalPublicStationBackfillRepository, TaskFailure, TempestBackfillProvider,
    TempestBackfillRepository, TempestBackfillSource, TempestBackfillTimeAdapter,
    TideBackfillProvider, TideBackfillRepository, TideBackfillSource, TideBackfillTimeAdapter,
    execute_public_station_archive_backfill, execute_tempest_bulk_backfill, execute_tide_backfill,
    plan_public_station_backfill_dry_run,
};

use crate::public_station_archive_binding::bind_public_station_archive;
use crate::{PublicStationBackfillPlanningRepository, SupplementalPublicStationBackfillProvider};

const CONFIGURATION_MISMATCH: &str = "historical_backfill_configuration_mismatch";
const TEMPEST_CONFIGURATION_REQUIRED: &str = "tempest_configuration_required";
const TIDE_CONFIGURATION_REQUIRED: &str = "tide_configuration_required";
const PUBLIC_STATION_CONFIGURATION_REQUIRED: &str = "public_station_configuration_required";

/// Report-file creation policy retained from each Node command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoricalBackfillReportFileMode {
    OwnerReadWrite,
    ProcessDefault,
}

/// Checked Tempest catalog and credential loaded before opening a pool.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoricalTempestProcessConfiguration<Credential> {
    pub catalog: Option<TempestConfiguration>,
    pub credential: Option<Credential>,
}

/// Injected process output and canonical report serialization boundary.
pub trait HistoricalBackfillOutputPort {
    /// Serialize one typed report with its final newline.
    fn serialize_report(
        &mut self,
        report: &HistoricalBackfillReport,
    ) -> Result<Vec<u8>, TaskFailure>;

    /// Persist one requested report before stdout.
    fn write_report_file(
        &mut self,
        path: &str,
        contents: &[u8],
        mode: HistoricalBackfillReportFileMode,
    ) -> Result<(), TaskFailure>;

    /// Write the same serialized report to process stdout.
    fn write_stdout(&mut self, contents: &[u8]) -> Result<(), TaskFailure>;
}

/// Injected checked configuration, provider, source, pool, and time owners.
pub trait HistoricalBackfillProcessPort {
    type TempestCredential;
    type TempestPool: TempestBackfillRepository;
    type TempestProvider: TempestBackfillProvider;
    type TempestTime: TempestBackfillTimeAdapter;
    type TidePool: TideBackfillRepository;
    type TideProvider: TideBackfillProvider;
    type TideTime: TideBackfillTimeAdapter;
    type AmbientPool: PublicStationBackfillPlanningRepository
        + SupplementalPublicStationBackfillRepository;
    type AmbientProvider: AmbientWeatherBackfillProvider;
    type SupplementalPublicStationProvider: SupplementalPublicStationBackfillProvider;
    type AmbientTime: PublicStationBackfillTimeAdapter;

    /// Load the checked Tempest catalog and credential before opening a pool.
    fn load_tempest_configuration(
        &mut self,
    ) -> Result<HistoricalTempestProcessConfiguration<Self::TempestCredential>, TaskFailure>;

    /// Open the Tempest process-owned pool after configuration admission.
    fn open_tempest_pool(
        &mut self,
        configuration: &TempestConfiguration,
    ) -> Result<Self::TempestPool, TaskFailure>;

    /// Verify the supported PostgreSQL contract inside the close-protected boundary.
    fn assert_supported_tempest_database(
        &mut self,
        pool: &mut Self::TempestPool,
    ) -> Result<(), TaskFailure>;

    /// Resolve checked Tempest sources in catalog order.
    fn resolve_tempest_sources(
        &mut self,
        pool: &mut Self::TempestPool,
        configuration: &TempestConfiguration,
        station_ids: &[u64],
    ) -> Result<Vec<TempestBackfillSource>, TaskFailure>;

    /// Verify one live Tempest station identity before a non-dry-run mutation.
    fn verify_tempest_source(
        &mut self,
        credential: &Self::TempestCredential,
        source: &TempestBackfillSource,
    ) -> Result<(), TaskFailure>;

    /// Create the typed Tempest provider after source identity verification.
    fn create_tempest_provider(
        &mut self,
        credential: &Self::TempestCredential,
    ) -> Self::TempestProvider;

    /// Create the admitted Tempest calendar and deadline owner.
    fn create_tempest_time(&mut self) -> Self::TempestTime;

    /// Close the Tempest pool through the application finally boundary.
    fn close_tempest_pool(&mut self, pool: Self::TempestPool) -> Result<(), TaskFailure>;

    /// Load the checked tide catalog before opening a pool.
    fn load_tide_configuration(&mut self) -> Result<Option<TideConfiguration>, TaskFailure>;

    /// Open the tide process-owned pool after configuration admission.
    fn open_tide_pool(
        &mut self,
        configuration: &TideConfiguration,
    ) -> Result<Self::TidePool, TaskFailure>;

    /// Resolve checked tide sources in catalog order.
    fn resolve_tide_sources(
        &mut self,
        pool: &mut Self::TidePool,
        configuration: &TideConfiguration,
        source_keys: &[String],
    ) -> Result<Vec<TideBackfillSource>, TaskFailure>;

    /// Create the typed NOAA provider immediately before execution.
    fn create_tide_provider(&mut self, configuration: &TideConfiguration) -> Self::TideProvider;

    /// Create the admitted tide calendar and deadline owner.
    fn create_tide_time(&mut self) -> Self::TideTime;

    /// Close the tide pool through the application finally boundary.
    fn close_tide_pool(&mut self, pool: Self::TidePool) -> Result<(), TaskFailure>;

    /// Load the checked public-station catalog before opening a pool.
    fn load_public_station_configuration(
        &mut self,
    ) -> Result<Option<PublicStationConfiguration>, TaskFailure>;

    /// Open the Ambient process-owned pool after configuration admission.
    fn open_ambient_pool(
        &mut self,
        configuration: &PublicStationConfiguration,
    ) -> Result<Self::AmbientPool, TaskFailure>;

    /// Create the typed Ambient provider immediately before execution.
    fn create_ambient_provider(
        &mut self,
        configuration: &PublicStationConfiguration,
    ) -> Self::AmbientProvider;

    /// Create the other archive providers with no requests before chunk start.
    fn create_supplemental_public_station_provider(
        &mut self,
        configuration: &PublicStationConfiguration,
    ) -> Self::SupplementalPublicStationProvider;

    /// Pace successive provider chunks from the process boundary.
    fn pace_public_station_backfill(&mut self, delay_ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }

    /// Create the admitted Ambient calendar and deadline owner.
    fn create_ambient_time(&mut self) -> Self::AmbientTime;

    /// Close the Ambient pool through the application finally boundary.
    fn close_ambient_pool(&mut self, pool: Self::AmbientPool) -> Result<(), TaskFailure>;
}

/// Run one already-parsed retained historical backfill command.
pub fn run_historical_backfill_process<Port, Output>(
    command: HistoricalBackfillCommand,
    port: &mut Port,
    output: &mut Output,
) -> Result<u8, TaskFailure>
where
    Port: HistoricalBackfillProcessPort,
    Output: HistoricalBackfillOutputPort,
{
    // reject a command whose admitted configuration authority changed families
    match command.arguments {
        HistoricalBackfillArguments::Tempest(arguments) => {
            require_family(
                command.source_configuration,
                HistoricalBackfillConfigurationFamily::Tempest,
            )?;
            run_tempest_process(arguments, command.report_path, port, output)
        }
        HistoricalBackfillArguments::Tide(arguments) => {
            require_family(
                command.source_configuration,
                HistoricalBackfillConfigurationFamily::Tide,
            )?;
            run_tide_process(arguments, command.report_path, port, output)
        }
        HistoricalBackfillArguments::PublicStations(arguments) => {
            require_family(
                command.source_configuration,
                HistoricalBackfillConfigurationFamily::PublicStations,
            )?;
            run_public_station_process(arguments, command.report_path, port, output)
        }
    }
}

// run the exact Tempest configuration, pool, source, execution, output, and close order
fn run_tempest_process<Port, Output>(
    arguments: weather_worker::TempestBackfillArguments,
    report_path: Option<String>,
    port: &mut Port,
    output: &mut Output,
) -> Result<u8, TaskFailure>
where
    Port: HistoricalBackfillProcessPort,
    Output: HistoricalBackfillOutputPort,
{
    let loaded = port.load_tempest_configuration()?;
    let configuration = loaded
        .catalog
        .ok_or_else(|| task_failure(TEMPEST_CONFIGURATION_REQUIRED))?;
    let credential = loaded
        .credential
        .ok_or_else(|| task_failure(TEMPEST_CONFIGURATION_REQUIRED))?;
    let mut pool = port.open_tempest_pool(&configuration)?;
    let execution = (|| {
        port.assert_supported_tempest_database(&mut pool)?;
        let sources =
            port.resolve_tempest_sources(&mut pool, &configuration, &arguments.station_ids)?;

        // match Node's live identity check only when durable writes are enabled
        if !arguments.dry_run {
            // verify every selected station before executing any station archive
            for source in &sources {
                port.verify_tempest_source(&credential, source)?;
            }
        }
        let mut provider = port.create_tempest_provider(&credential);
        let mut time = port.create_tempest_time();
        let report = execute_tempest_bulk_backfill(
            &arguments,
            &configuration.siteKey,
            &sources,
            &mut pool,
            &mut provider,
            &mut time,
        )?;
        finish_report(
            HistoricalBackfillReport::Tempest(report),
            report_path.as_deref(),
            HistoricalBackfillReportFileMode::OwnerReadWrite,
            output,
        )
    })();
    // let a finally-close failure override the retained operation result
    port.close_tempest_pool(pool)?;
    execution
}

// run the exact tide configuration, pool, source, execution, output, and close order
fn run_tide_process<Port, Output>(
    arguments: weather_worker::TideBackfillArguments,
    report_path: Option<String>,
    port: &mut Port,
    output: &mut Output,
) -> Result<u8, TaskFailure>
where
    Port: HistoricalBackfillProcessPort,
    Output: HistoricalBackfillOutputPort,
{
    let configuration = port
        .load_tide_configuration()?
        .ok_or_else(|| task_failure(TIDE_CONFIGURATION_REQUIRED))?;
    let mut pool = port.open_tide_pool(&configuration)?;
    let execution = (|| {
        let sources =
            port.resolve_tide_sources(&mut pool, &configuration, &arguments.source_keys)?;
        let mut provider = port.create_tide_provider(&configuration);
        let mut time = port.create_tide_time();
        let report = execute_tide_backfill(
            &arguments,
            &configuration.siteKey,
            &sources,
            &mut pool,
            &mut provider,
            &mut time,
        )?;
        finish_report(
            HistoricalBackfillReport::Tide(report),
            report_path.as_deref(),
            HistoricalBackfillReportFileMode::ProcessDefault,
            output,
        )
    })();
    // let a finally-close failure override the retained operation result
    port.close_tide_pool(pool)?;
    execution
}

// run the public station command with the same owned pool and report lifecycle
fn run_public_station_process<Port, Output>(
    arguments: weather_worker::PublicStationBackfillArguments,
    report_path: Option<String>,
    port: &mut Port,
    output: &mut Output,
) -> Result<u8, TaskFailure>
where
    Port: HistoricalBackfillProcessPort,
    Output: HistoricalBackfillOutputPort,
{
    let configuration = port
        .load_public_station_configuration()?
        .ok_or_else(|| task_failure(PUBLIC_STATION_CONFIGURATION_REQUIRED))?;
    let mut pool = port.open_ambient_pool(&configuration)?;
    // protect every operation with the process-owned pool close
    let execution = (|| {
        let planning_sources =
            pool.resolve_public_station_planning_sources(&configuration, &arguments.source_keys)?;
        let mut time = port.create_ambient_time();

        // dry run must never create a provider or enter a source session
        let report = if arguments.dry_run {
            plan_public_station_backfill_dry_run(
                &arguments,
                &configuration.siteKey,
                &planning_sources,
                &mut pool,
                &mut time,
            )?
        } else {
            let (sources, mut provider) = bind_public_station_archive(
                &configuration,
                planning_sources,
                port.create_ambient_provider(&configuration),
                port.create_supplemental_public_station_provider(&configuration),
                // retain the process-owned wait between completed PurpleAir chunks
                |delay_ms| port.pace_public_station_backfill(delay_ms),
            )?;
            execute_public_station_archive_backfill(
                &arguments,
                &configuration.siteKey,
                &sources,
                &mut pool,
                &mut provider,
                &mut time,
            )?
        };
        finish_report(
            HistoricalBackfillReport::PublicStations(report),
            report_path.as_deref(),
            HistoricalBackfillReportFileMode::OwnerReadWrite,
            output,
        )
    })();
    // let a finally-close failure override the retained operation result
    port.close_ambient_pool(pool)?;
    execution
}

// serialize once then retain optional file before stdout ordering
pub(crate) fn finish_report<Output: HistoricalBackfillOutputPort>(
    report: HistoricalBackfillReport,
    report_path: Option<&str>,
    mode: HistoricalBackfillReportFileMode,
    output: &mut Output,
) -> Result<u8, TaskFailure> {
    let exit_code = report.exit_code();
    let serialized = output.serialize_report(&report)?;

    // persist the optional machine report before ordinary process output
    if let Some(path) = report_path {
        output.write_report_file(path, &serialized, mode)?;
    }
    output.write_stdout(&serialized)?;
    Ok(exit_code)
}

// require the exact configuration family admitted by the command parser
fn require_family(
    source: HistoricalBackfillSourceConfiguration,
    expected: HistoricalBackfillConfigurationFamily,
) -> Result<(), TaskFailure> {
    // reject not-applicable or a changed family before configuration loading
    if source != HistoricalBackfillSourceConfiguration::Required(expected) {
        return Err(task_failure(CONFIGURATION_MISMATCH));
    }
    Ok(())
}

// construct one bounded process-level task failure
fn task_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::process::Command;
    use std::rc::Rc;

    use weather_contracts::{
        JsonValue, Nullable, PublicStationAdapter, PublicStationConfigurationStation,
        PublicStationProviderConfiguration, PublicStationSourceConfiguration, SourceCapability,
        TempestConfigurationProvider, TideProviderConfiguration,
    };
    use weather_worker::{
        AmbientWeatherProviderCursor, AmbientWeatherRangeRequest, AmbientWeatherResponseMetadata,
        HistoricalBackfillCommandDate, HistoricalBackfillCommandKind,
        HistoricalBackfillCommandTimeAdapter, NoaaTideProviderCursor, NoaaTideResponseMetadata,
        NormalizedAmbientWeatherRecord, NormalizedNoaaTideRecord, NormalizedTempestRecord,
        ProviderBatch, PublicStationBackfillArguments, PublicStationBackfillChunkIdentity,
        PublicStationBackfillCompletion, PublicStationBackfillDate, PublicStationBackfillDeadlines,
        PublicStationBackfillErrorClassification, PublicStationBackfillOperationError,
        PublicStationBackfillPlanningSource, PublicStationBackfillProviderFailure,
        PublicStationBackfillReport, PublicStationBackfillRepository,
        PublicStationBackfillRunStart, ScheduledWindow, SupplementalPublicStationBatch,
        SupplementalPublicStationIdentity, SupplementalPublicStationRecord,
        SupplementalPublicStationResponseMetadata, SupplementalPublicStationScheduledRequest,
        TempestBackfillArguments, TempestBackfillChunkIdentity, TempestBackfillCompletion,
        TempestBackfillDeadlines, TempestBackfillOperationError, TempestBackfillProviderFailure,
        TempestBackfillRunStart, TempestObservationRequest, TempestResponseMetadata,
        TideBackfillArguments, TideBackfillChunkIdentity, TideBackfillCompletion, TideBackfillDate,
        TideBackfillDeadlines, TideBackfillOperationError, TideBackfillProviderFailure,
        TideBackfillProviderRequest, TideBackfillRunStart,
    };

    use crate::public_station_archive_binding::archive_catalog;
    use crate::worker_public_station_dispatch::{material_identity, required_text};

    use super::*;

    type Trace = Rc<RefCell<Vec<String>>>;
    const TODAY_MS: i64 = 1_795_392_000_000;
    const YESTERDAY_MS: i64 = 1_795_305_600_000;
    const AUGUST_19_MS: i64 = 1_787_097_600_000;
    const DAY_MS: i64 = 86_400_000;

    // expose fixture trace labels from the generated enum
    fn adapter_wire_value(adapter: PublicStationAdapter) -> &'static str {
        PublicStationAdapter::WIRE_VALUES
            .iter()
            .find_map(|&(wire, value)| (value == adapter).then_some(wire))
            .expect("known adapter wire value")
    }

    struct FakePool {
        archive_writes: bool,
        trace: Trace,
        planning_sources: Vec<PublicStationBackfillPlanningSource>,
    }

    impl TempestBackfillRepository for FakePool {
        type Session = ();

        // reject persistence from the disposable dry-run fixture
        fn has_successful_chunk(
            &mut self,
            _identity: &TempestBackfillChunkIdentity,
        ) -> Result<bool, TaskFailure> {
            unreachable!("dry-run fixture does not read Tempest persistence")
        }

        // reject session acquisition from the disposable dry-run fixture
        fn acquire_source_session(
            &mut self,
            _source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            unreachable!("dry-run fixture does not acquire Tempest sessions")
        }

        // reject recovery from the disposable dry-run fixture
        fn abandon_expired_runs(
            &mut self,
            _session: &mut Self::Session,
            _now_ms: i64,
        ) -> Result<(), TempestBackfillOperationError> {
            unreachable!("dry-run fixture does not abandon Tempest runs")
        }

        // reject run creation from the disposable dry-run fixture
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _start: TempestBackfillRunStart,
        ) -> Result<String, TempestBackfillOperationError> {
            unreachable!("dry-run fixture does not start Tempest runs")
        }

        // reject completion from the disposable dry-run fixture
        fn complete_backfill_ingestion(
            &mut self,
            _session: &mut Self::Session,
            _completion: TempestBackfillCompletion,
        ) -> Result<(), TempestBackfillOperationError> {
            unreachable!("dry-run fixture does not complete Tempest runs")
        }

        // reject failure finalization from the disposable dry-run fixture
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _run_id: &str,
            _identity: &TempestBackfillChunkIdentity,
            _failure: &TempestBackfillProviderFailure,
        ) -> Result<(), TaskFailure> {
            unreachable!("dry-run fixture does not fail Tempest runs")
        }

        // reject release from the disposable dry-run fixture
        fn release_source_session(&mut self, _session: Self::Session) -> Result<(), TaskFailure> {
            unreachable!("dry-run fixture does not release Tempest sessions")
        }
    }

    impl TideBackfillRepository for FakePool {
        type Session = ();

        // reject persistence from the disposable dry-run fixture
        fn has_successful_chunk(
            &mut self,
            _identity: &TideBackfillChunkIdentity,
        ) -> Result<bool, TaskFailure> {
            unreachable!("dry-run fixture does not read tide persistence")
        }

        // reject session acquisition from the disposable dry-run fixture
        fn acquire_source_session(
            &mut self,
            _source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            unreachable!("dry-run fixture does not acquire tide sessions")
        }

        // reject recovery from the disposable dry-run fixture
        fn abandon_expired_runs(
            &mut self,
            _session: &mut Self::Session,
            _now_ms: i64,
        ) -> Result<(), TideBackfillOperationError> {
            unreachable!("dry-run fixture does not abandon tide runs")
        }

        // reject run creation from the disposable dry-run fixture
        fn start_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _start: TideBackfillRunStart,
        ) -> Result<String, TideBackfillOperationError> {
            unreachable!("dry-run fixture does not start tide runs")
        }

        // reject completion from the disposable dry-run fixture
        fn complete_backfill_ingestion(
            &mut self,
            _session: &mut Self::Session,
            _completion: TideBackfillCompletion,
        ) -> Result<(), TideBackfillOperationError> {
            unreachable!("dry-run fixture does not complete tide runs")
        }

        // reject failure finalization from the disposable dry-run fixture
        fn fail_ingestion_run(
            &mut self,
            _session: &mut Self::Session,
            _run_id: &str,
            _identity: &TideBackfillChunkIdentity,
            _failure: &TideBackfillProviderFailure,
        ) -> Result<(), TaskFailure> {
            unreachable!("dry-run fixture does not fail tide runs")
        }

        // reject release from the disposable dry-run fixture
        fn release_source_session(&mut self, _session: Self::Session) -> Result<(), TaskFailure> {
            unreachable!("dry-run fixture does not release tide sessions")
        }
    }

    impl PublicStationBackfillRepository for FakePool {
        type Session = &'static str;

        // record exact read-only resume checks without writing a run
        fn has_successful_chunk(
            &mut self,
            identity: &PublicStationBackfillChunkIdentity,
        ) -> Result<bool, TaskFailure> {
            // retain the mixed oracle winners before acquiring source sessions
            if self.archive_writes {
                let adapter = self.archive_adapter(&identity.source_id);
                let start = identity.interval_start.start_ms();
                self.trace
                    .borrow_mut()
                    .push(format!("archive:lookup|{adapter}|{start}"));
                return Ok((adapter == "weather-underground" && start == AUGUST_19_MS)
                    || (adapter == "purpleair" && start == AUGUST_19_MS + 2 * DAY_MS));
            }
            self.trace.borrow_mut().push(format!(
                "lookup:{}|{}|{}|{}|{}T00:00:00.000Z|{}T00:00:00.000Z|{}|{}",
                identity.source_id,
                identity.adapter_version,
                identity.source_config_fingerprint,
                identity.chunk_plan_version,
                identity.interval_start.label(),
                identity.interval_end_exclusive.label(),
                identity.requested_from_date,
                identity.requested_to_date
            ));
            Ok(false)
        }

        // lease the same source identity until completion or failure is durable
        fn acquire_source_session(
            &mut self,
            source_id: &str,
        ) -> Result<Option<Self::Session>, TaskFailure> {
            assert!(self.archive_writes, "dry-run fixture acquired a session");
            let adapter = self.archive_adapter(source_id);
            self.trace
                .borrow_mut()
                .push(format!("archive:acquire|{adapter}"));
            Ok(Some(adapter))
        }

        // recover stale rows with the single run clock sample
        fn abandon_expired_runs(
            &mut self,
            session: &mut Self::Session,
            now_ms: i64,
        ) -> Result<(), PublicStationBackfillOperationError> {
            self.trace
                .borrow_mut()
                .push(format!("archive:abandon|{session}|{now_ms}"));
            Ok(())
        }

        // expose every metadata field shared with the original command
        fn start_ingestion_run(
            &mut self,
            session: &mut Self::Session,
            start: PublicStationBackfillRunStart,
        ) -> Result<String, PublicStationBackfillOperationError> {
            self.trace.borrow_mut().push(format!(
                "archive:start|{session}|{}|{}|{}|{}|{}",
                start.endpoint,
                start.station_key,
                start.requested_start_ms,
                start.requested_end_exclusive_ms,
                start.deadline_at_ms,
            ));
            assert_eq!(
                start.chunk_plan_version,
                weather_worker::PUBLIC_STATION_CHUNK_PLAN_VERSION
            );
            assert_eq!(start.source_config_fingerprint.len(), 64);
            Ok("archive-run".to_owned())
        }

        // retain the Ambient observation report through its existing typed writer
        fn complete_backfill_ingestion(
            &mut self,
            session: &mut Self::Session,
            completion: PublicStationBackfillCompletion,
        ) -> Result<(), PublicStationBackfillOperationError> {
            self.record_archive_completion(
                session,
                &completion,
                completion.response_metadata.provider,
            );
            Ok(())
        }

        // finalize the exact classified failed chunk on the acquired session
        fn fail_ingestion_run(
            &mut self,
            session: &mut Self::Session,
            run_id: &str,
            identity: &PublicStationBackfillChunkIdentity,
            failure: &PublicStationBackfillProviderFailure,
        ) -> Result<(), TaskFailure> {
            assert_eq!(
                failure.error.classification,
                PublicStationBackfillErrorClassification::RateLimited
            );
            self.trace.borrow_mut().push(format!(
                "archive:fail|{session}|{run_id}|{}|rate_limited|{}|{}|{}",
                failure.attempts,
                failure.error.code,
                failure.error.message,
                identity.interval_start.start_ms(),
            ));
            Ok(())
        }

        // retain release after all writes on the acquired session
        fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("archive:release|{session}"));
            Ok(())
        }
    }

    impl SupplementalPublicStationBackfillRepository for FakePool {
        // retain supplemental completion without coercing it to Ambient metadata
        fn complete_supplemental_backfill_ingestion(
            &mut self,
            session: &mut Self::Session,
            completion: PublicStationBackfillCompletion<
                SupplementalPublicStationRecord,
                SupplementalPublicStationResponseMetadata,
            >,
        ) -> Result<(), PublicStationBackfillOperationError> {
            self.record_archive_completion(
                session,
                &completion,
                completion.response_metadata.provider,
            );
            Ok(())
        }
    }

    impl FakePool {
        // retrieve the checked provider of the durable fixture source
        fn archive_adapter(&self, id: &str) -> &'static str {
            let source = self
                .planning_sources
                .iter()
                .find(|source| source.source_id == id)
                .expect("known archive source");
            adapter_wire_value(source.adapter)
        }

        // put both typed completion paths on the original command trace
        fn record_archive_completion<Record, Metadata>(
            &self,
            session: &str,
            completion: &PublicStationBackfillCompletion<Record, Metadata>,
            provider: &str,
        ) {
            assert_eq!(
                session,
                self.archive_adapter(&completion.identity.source_id)
            );
            self.trace.borrow_mut().push(format!(
                "archive:complete|{session}|{}|{}|{}|{provider}|{}",
                completion.run_id,
                completion.attempts,
                completion.records.len(),
                completion.upstream_response_checksum,
            ));
        }
    }

    impl PublicStationBackfillPlanningRepository for FakePool {
        /// Resolve only the fixture's selected read-only plan inputs.
        fn resolve_public_station_planning_sources(
            &mut self,
            _configuration: &PublicStationConfiguration,
            selectors: &[String],
        ) -> Result<Vec<PublicStationBackfillPlanningSource>, TaskFailure> {
            self.trace
                .borrow_mut()
                .push("resolve:public-stations".to_owned());
            Ok(self
                .planning_sources
                .iter()
                // retain original catalog order even for an explicit selector
                .filter(|source| selectors.is_empty() || selectors.contains(&source.source_key))
                .cloned()
                .collect())
        }
    }

    struct FakeProvider;

    impl TempestBackfillProvider for FakeProvider {
        // reject provider work from the disposable dry-run fixture
        fn fetch(
            &mut self,
            _request: TempestObservationRequest,
            _deadline_at_ms: i64,
        ) -> Result<
            ProviderBatch<NormalizedTempestRecord, TempestResponseMetadata, Option<i64>>,
            TempestBackfillOperationError,
        > {
            unreachable!("dry-run fixture does not call Tempest")
        }
    }

    impl TideBackfillProvider for FakeProvider {
        // reject provider work from the disposable dry-run fixture
        fn fetch(
            &mut self,
            _request: TideBackfillProviderRequest,
        ) -> Result<
            ProviderBatch<
                NormalizedNoaaTideRecord,
                NoaaTideResponseMetadata,
                Option<NoaaTideProviderCursor>,
            >,
            TideBackfillOperationError,
        > {
            unreachable!("dry-run fixture does not call NOAA")
        }
    }

    struct FakeArchiveProvider {
        catalog: PublicStationConfiguration,
        trace: Trace,
    }

    impl FakeArchiveProvider {
        // verify the exact source key and retrieve its admitted material
        fn fixture_material(&self, source_id: &str) -> &[(Vec<u16>, JsonValue)] {
            let key = source_id
                .strip_prefix("source-")
                .expect("fixture source id");
            &archive_catalog(&self.catalog)[key].1.adapterConfig
        }

        // retain canonical request and deadline without logging request credentials
        fn record_fetch(&self, fields: [&str; 5], window: ScheduledWindow, deadline_at_ms: i64) {
            self.trace.borrow_mut().push(format!(
                "archive:fetch|{}|{}|{}|{}",
                fields.join("|"),
                window.start_ms,
                window.end_exclusive_ms,
                deadline_at_ms,
            ));
        }
    }

    impl AmbientWeatherBackfillProvider for FakeArchiveProvider {
        // exercise the already-normalized Ambient range boundary
        fn fetch(
            &mut self,
            request: AmbientWeatherRangeRequest,
            deadline_at_ms: i64,
        ) -> Result<
            ProviderBatch<
                NormalizedAmbientWeatherRecord,
                AmbientWeatherResponseMetadata,
                Option<AmbientWeatherProviderCursor>,
            >,
            PublicStationBackfillOperationError,
        > {
            let material = self.fixture_material(&request.source_id);
            assert!(request.device_id == required_text(material, "deviceId").expect("device id"));
            assert!(request.mac_address == required_text(material, "macAddress").expect("mac"));
            self.record_fetch(
                [
                    "ambient-weather",
                    &request.source_id,
                    &request.timezone,
                    request.model.as_deref().unwrap_or("-"),
                    request.serial.as_deref().unwrap_or("-"),
                ],
                ScheduledWindow {
                    start_ms: request.start_ms,
                    end_exclusive_ms: request.end_exclusive_ms,
                },
                deadline_at_ms,
            );
            Ok(ProviderBatch {
                attempts: 2,
                provider_cursor: None,
                records: Vec::new(),
                response_metadata: AmbientWeatherResponseMetadata {
                    normalized_record_count: 0,
                    provider: "ambient-weather-network",
                    raw_observation_count: 0,
                },
                upstream_response_checksum: "b".repeat(64),
            })
        }
    }

    impl SupplementalPublicStationBackfillProvider for FakeArchiveProvider {
        // exercise typed archive dispatch and inject a real classified fixture failure
        fn fetch(
            &mut self,
            request: SupplementalPublicStationScheduledRequest,
            deadline_at_ms: i64,
        ) -> Result<SupplementalPublicStationBatch, PublicStationBackfillOperationError> {
            let adapter = request.identity.adapter();
            let material = self.fixture_material(&request.source_id);
            assert!(
                Some(&request.identity)
                    == material_identity(adapter, material)
                        .expect("typed fixture material")
                        .as_ref()
            );
            self.record_fetch(
                [
                    adapter_wire_value(adapter),
                    &request.source_id,
                    &request.timezone,
                    request.model.as_deref().unwrap_or("-"),
                    request.serial.as_deref().unwrap_or("-"),
                ],
                request.window,
                deadline_at_ms,
            );
            // match the original command's classified Weather Underground failure fixture
            if matches!(
                request.identity,
                SupplementalPublicStationIdentity::WeatherUnderground { .. }
            ) {
                return Err(PublicStationBackfillOperationError::provider(
                    2,
                    PublicStationBackfillErrorClassification::RateLimited,
                    "fixture_rate_limited",
                    "fixture rate limit",
                ));
            }
            let is_netatmo = adapter == PublicStationAdapter::Netatmo;
            Ok(ProviderBatch {
                attempts: 2,
                provider_cursor: None,
                records: Vec::new(),
                response_metadata: SupplementalPublicStationResponseMetadata {
                    measurement_request_count: is_netatmo.then_some(4),
                    normalized_record_count: 0,
                    provider: if is_netatmo {
                        "netatmo-weathermap"
                    } else {
                        "purpleair"
                    },
                    raw_observation_count: (!is_netatmo).then_some(0),
                    resolution_seconds: (!is_netatmo).then_some(120),
                },
                upstream_response_checksum: "b".repeat(64),
            })
        }
    }

    struct FakeTime;

    impl TempestBackfillTimeAdapter for FakeTime {
        // return the fixed admitted UTC cutoff
        fn today_utc(&mut self) -> Result<PublicStationBackfillDate, TaskFailure> {
            Ok(PublicStationBackfillDate::admitted("2026-11-23", TODAY_MS))
        }

        // reject chunk planning for the intentionally empty source catalog
        fn add_utc_days(
            &mut self,
            _date: &PublicStationBackfillDate,
            _days: i64,
        ) -> Result<PublicStationBackfillDate, TaskFailure> {
            unreachable!("empty Tempest catalog does not plan chunks")
        }

        // reject deadlines for the disposable dry-run fixture
        fn deadlines(&mut self) -> Result<TempestBackfillDeadlines, TempestBackfillOperationError> {
            unreachable!("dry-run fixture does not create Tempest deadlines")
        }
    }

    impl TideBackfillTimeAdapter for FakeTime {
        // return the fixed admitted UTC cutoff
        fn today_utc(&mut self) -> Result<TideBackfillDate, TaskFailure> {
            Ok(TideBackfillDate::admitted("2026-11-23", TODAY_MS))
        }

        // reject chunk planning for the intentionally empty source catalog
        fn add_utc_days(
            &mut self,
            _date: &TideBackfillDate,
            _days: i64,
        ) -> Result<TideBackfillDate, TaskFailure> {
            unreachable!("empty tide catalog does not plan chunks")
        }

        // reject deadlines for the disposable dry-run fixture
        fn deadlines(&mut self) -> Result<TideBackfillDeadlines, TideBackfillOperationError> {
            unreachable!("dry-run fixture does not create tide deadlines")
        }
    }

    impl PublicStationBackfillTimeAdapter for FakeTime {
        // return the fixed admitted UTC cutoff
        fn today_utc(&mut self) -> Result<PublicStationBackfillDate, TaskFailure> {
            Ok(PublicStationBackfillDate::admitted("2026-11-23", TODAY_MS))
        }

        // admit only the known August window exercised by the original command oracle
        fn add_utc_days(
            &mut self,
            date: &PublicStationBackfillDate,
            days: i64,
        ) -> Result<PublicStationBackfillDate, TaskFailure> {
            let days_since_august_19 = (date.start_ms() - AUGUST_19_MS) / DAY_MS + days;
            assert!((0..=12).contains(&days_since_august_19));
            Ok(PublicStationBackfillDate::admitted(
                format!("2026-08-{}", 19 + days_since_august_19),
                AUGUST_19_MS + days_since_august_19 * DAY_MS,
            ))
        }

        // use the same admitted deadline planner as scheduled provider operations
        fn deadlines(
            &mut self,
        ) -> Result<PublicStationBackfillDeadlines, PublicStationBackfillOperationError> {
            let plan = crate::plan_worker_ingestion_deadlines(
                YESTERDAY_MS,
                weather_providers::ProviderControls::default(),
            )
            .map_err(|failure| PublicStationBackfillOperationError::unexpected(failure.code))?;
            Ok(PublicStationBackfillDeadlines {
                started_at_ms: YESTERDAY_MS,
                provider_deadline_at_ms: plan.provider_deadline_at_ms,
                run_deadline_at_ms: plan.run_deadline_at_ms,
            })
        }
    }

    impl HistoricalBackfillCommandTimeAdapter for FakeTime {
        // return the fixed compiled-Node command date
        fn today_utc(&mut self) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
            Ok(HistoricalBackfillCommandDate::admitted(
                "2026-11-23",
                TODAY_MS,
            ))
        }

        // admit only dates used by original and deployed Tide argv fixtures
        fn admit_utc_date(
            &mut self,
            label: &str,
        ) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
            let start_ms = match label {
                "2019-01-01" => 1_546_300_800_000,
                "2026-11-22" => YESTERDAY_MS,
                "2026-08-19" => AUGUST_19_MS,
                "2026-08-25" => AUGUST_19_MS + 6 * DAY_MS,
                _ => {
                    return Err(task_failure("fixture_command_date_unavailable"));
                }
            };
            Ok(HistoricalBackfillCommandDate::admitted(label, start_ms))
        }

        // apply the original CLI's previous-day default without a local calendar codec
        fn add_utc_days(
            &mut self,
            date: &HistoricalBackfillCommandDate,
            days: i64,
        ) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
            assert_eq!((date.label(), days), ("2026-11-23", -1));
            Ok(HistoricalBackfillCommandDate::admitted(
                "2026-11-22",
                YESTERDAY_MS,
            ))
        }
    }

    struct FakeApplication {
        ambient_enabled: bool,
        archive_writes: bool,
        public_station_catalog_override: Option<PublicStationConfiguration>,
        planning_sources: Vec<PublicStationBackfillPlanningSource>,
        trace: Trace,
    }

    impl HistoricalBackfillProcessPort for FakeApplication {
        type TempestCredential = String;
        type TempestPool = FakePool;
        type TempestProvider = FakeProvider;
        type TempestTime = FakeTime;
        type TidePool = FakePool;
        type TideProvider = FakeProvider;
        type TideTime = FakeTime;
        type AmbientPool = FakePool;
        type AmbientProvider = FakeArchiveProvider;
        type SupplementalPublicStationProvider = FakeArchiveProvider;
        type AmbientTime = FakeTime;

        // load both required Tempest inputs before pool creation
        fn load_tempest_configuration(
            &mut self,
        ) -> Result<HistoricalTempestProcessConfiguration<Self::TempestCredential>, TaskFailure>
        {
            self.trace.borrow_mut().push("load:tempest".to_owned());
            Ok(HistoricalTempestProcessConfiguration {
                catalog: Some(tempest_configuration()),
                credential: Some("fixture-key".to_owned()),
            })
        }

        // open one disposable Tempest pool
        fn open_tempest_pool(
            &mut self,
            _configuration: &TempestConfiguration,
        ) -> Result<Self::TempestPool, TaskFailure> {
            self.trace.borrow_mut().push("pool:tempest".to_owned());
            Ok(FakePool {
                archive_writes: false,
                trace: Rc::clone(&self.trace),
                planning_sources: Vec::new(),
            })
        }

        // retain the Tempest-only readiness ordering
        fn assert_supported_tempest_database(
            &mut self,
            _pool: &mut Self::TempestPool,
        ) -> Result<(), TaskFailure> {
            self.trace.borrow_mut().push("readiness:tempest".to_owned());
            Ok(())
        }

        // resolve the intentionally empty checked Tempest catalog
        fn resolve_tempest_sources(
            &mut self,
            _pool: &mut Self::TempestPool,
            _configuration: &TempestConfiguration,
            _station_ids: &[u64],
        ) -> Result<Vec<TempestBackfillSource>, TaskFailure> {
            self.trace.borrow_mut().push("resolve:tempest".to_owned());
            Ok(Vec::new())
        }

        // reject live identity calls from a dry-run command
        fn verify_tempest_source(
            &mut self,
            _credential: &Self::TempestCredential,
            _source: &TempestBackfillSource,
        ) -> Result<(), TaskFailure> {
            unreachable!("dry-run fixture does not verify live Tempest stations")
        }

        // create the injected Tempest provider after resolution
        fn create_tempest_provider(
            &mut self,
            _credential: &Self::TempestCredential,
        ) -> Self::TempestProvider {
            self.trace.borrow_mut().push("provider:tempest".to_owned());
            FakeProvider
        }

        // create the admitted Tempest clock immediately before execution
        fn create_tempest_time(&mut self) -> Self::TempestTime {
            self.trace.borrow_mut().push("time:tempest".to_owned());
            FakeTime
        }

        // close the Tempest pool after all output
        fn close_tempest_pool(&mut self, _pool: Self::TempestPool) -> Result<(), TaskFailure> {
            self.trace.borrow_mut().push("close:tempest".to_owned());
            Ok(())
        }

        // load the required tide catalog before pool creation
        fn load_tide_configuration(&mut self) -> Result<Option<TideConfiguration>, TaskFailure> {
            self.trace.borrow_mut().push("load:tide".to_owned());
            Ok(Some(tide_configuration()))
        }

        // open one disposable tide pool
        fn open_tide_pool(
            &mut self,
            _configuration: &TideConfiguration,
        ) -> Result<Self::TidePool, TaskFailure> {
            self.trace.borrow_mut().push("pool:tide".to_owned());
            Ok(FakePool {
                archive_writes: false,
                trace: Rc::clone(&self.trace),
                planning_sources: Vec::new(),
            })
        }

        // resolve the intentionally empty checked tide catalog
        fn resolve_tide_sources(
            &mut self,
            _pool: &mut Self::TidePool,
            _configuration: &TideConfiguration,
            _source_keys: &[String],
        ) -> Result<Vec<TideBackfillSource>, TaskFailure> {
            self.trace.borrow_mut().push("resolve:tide".to_owned());
            Ok(Vec::new())
        }

        // create the injected NOAA provider after resolution
        fn create_tide_provider(
            &mut self,
            _configuration: &TideConfiguration,
        ) -> Self::TideProvider {
            self.trace.borrow_mut().push("provider:tide".to_owned());
            FakeProvider
        }

        // create the admitted tide clock immediately before execution
        fn create_tide_time(&mut self) -> Self::TideTime {
            self.trace.borrow_mut().push("time:tide".to_owned());
            FakeTime
        }

        // close the tide pool after all output
        fn close_tide_pool(&mut self, _pool: Self::TidePool) -> Result<(), TaskFailure> {
            self.trace.borrow_mut().push("close:tide".to_owned());
            Ok(())
        }

        // load or deliberately omit the checked public-station catalog
        fn load_public_station_configuration(
            &mut self,
        ) -> Result<Option<PublicStationConfiguration>, TaskFailure> {
            self.trace.borrow_mut().push("load:ambient".to_owned());
            Ok(self.ambient_enabled.then(|| {
                self.public_station_catalog_override
                    .clone()
                    .unwrap_or_else(public_station_configuration)
            }))
        }

        // open one disposable Ambient pool
        fn open_ambient_pool(
            &mut self,
            _configuration: &PublicStationConfiguration,
        ) -> Result<Self::AmbientPool, TaskFailure> {
            self.trace.borrow_mut().push("pool:ambient".to_owned());
            Ok(FakePool {
                archive_writes: self.archive_writes,
                trace: Rc::clone(&self.trace),
                planning_sources: self.planning_sources.clone(),
            })
        }

        // create the injected Ambient provider after resolution
        fn create_ambient_provider(
            &mut self,
            configuration: &PublicStationConfiguration,
        ) -> Self::AmbientProvider {
            self.trace.borrow_mut().push("provider:ambient".to_owned());
            FakeArchiveProvider {
                catalog: configuration.clone(),
                trace: Rc::clone(&self.trace),
            }
        }

        // construct supplemental request ports without doing provider I/O
        fn create_supplemental_public_station_provider(
            &mut self,
            configuration: &PublicStationConfiguration,
        ) -> Self::SupplementalPublicStationProvider {
            self.trace
                .borrow_mut()
                .push("provider:supplemental".to_owned());
            FakeArchiveProvider {
                catalog: configuration.clone(),
                trace: Rc::clone(&self.trace),
            }
        }

        // capture provider pacing without blocking the test thread
        fn pace_public_station_backfill(&mut self, delay_ms: u64) {
            self.trace
                .borrow_mut()
                .push(format!("archive:pace|{delay_ms}"));
        }

        // create the admitted Ambient clock immediately before execution
        fn create_ambient_time(&mut self) -> Self::AmbientTime {
            self.trace.borrow_mut().push("time:ambient".to_owned());
            FakeTime
        }

        // close the Ambient pool after all output
        fn close_ambient_pool(&mut self, _pool: Self::AmbientPool) -> Result<(), TaskFailure> {
            self.trace.borrow_mut().push("close:ambient".to_owned());
            Ok(())
        }
    }

    struct FakeOutput {
        fail_stdout: bool,
        public_station_report: Option<PublicStationBackfillReport>,
        trace: Trace,
    }

    impl HistoricalBackfillOutputPort for FakeOutput {
        // serialize the typed family exactly once
        fn serialize_report(
            &mut self,
            report: &HistoricalBackfillReport,
        ) -> Result<Vec<u8>, TaskFailure> {
            let family = report_family(report);
            self.trace.borrow_mut().push(format!("serialize:{family}"));
            // retain the exact typed report before invoking the fixture serializer
            if let HistoricalBackfillReport::PublicStations(report) = report {
                self.public_station_report = Some(report.clone());
            }
            Ok(format!("{family}:{}\n", report.exit_code()).into_bytes())
        }

        // retain optional report path and family-specific mode
        fn write_report_file(
            &mut self,
            path: &str,
            contents: &[u8],
            mode: HistoricalBackfillReportFileMode,
        ) -> Result<(), TaskFailure> {
            let family = output_family(contents);
            self.trace
                .borrow_mut()
                .push(format!("file:{path}:{mode:?}:{family}"));
            Ok(())
        }

        // retain stdout after optional report persistence
        fn write_stdout(&mut self, contents: &[u8]) -> Result<(), TaskFailure> {
            let family = output_family(contents);
            self.trace.borrow_mut().push(format!("stdout:{family}"));

            // expose a close-protected output failure
            if self.fail_stdout {
                return Err(task_failure("stdout_failed"));
            }
            Ok(())
        }
    }

    // construct one checked empty Tempest catalog
    fn tempest_configuration() -> TempestConfiguration {
        TempestConfiguration {
            provider: TempestConfigurationProvider {
                active: true,
                attributionLabel: "Tempest".to_owned(),
                attributionUrl: "https://tempest.earth".to_owned(),
                displayName: "Tempest".to_owned(),
                key: "tempest".to_owned(),
            },
            siteKey: "ballydidean".to_owned(),
            stations: Vec::new(),
            version: 1,
        }
    }

    // construct one checked empty tide catalog
    fn tide_configuration() -> TideConfiguration {
        TideConfiguration {
            provider: TideProviderConfiguration {
                active: true,
                attributionLabel: "NOAA".to_owned(),
                attributionUrl: "https://tidesandcurrents.noaa.gov".to_owned(),
                displayName: "NOAA tides".to_owned(),
                key: "noaa".to_owned(),
            },
            siteKey: "ballydidean".to_owned(),
            stations: Vec::new(),
            version: 1,
        }
    }

    // construct one checked empty public-station catalog
    fn public_station_configuration() -> PublicStationConfiguration {
        PublicStationConfiguration {
            siteKey: "ballydidean".to_owned(),
            providers: vec![PublicStationProviderConfiguration {
                active: true,
                attributionLabel: "Ambient".to_owned(),
                attributionUrl: "https://ambientweather.net".to_owned(),
                displayName: "Ambient".to_owned(),
                key: "ambient-weather".to_owned(),
            }],
            stations: Vec::new(),
            version: 1,
        }
    }

    // construct one dry-run command for each retained family
    fn command(family: HistoricalBackfillConfigurationFamily) -> HistoricalBackfillCommand {
        let report_path = Some(format!("{}.json", family_name(family)));
        let arguments = match family {
            HistoricalBackfillConfigurationFamily::Tempest => {
                HistoricalBackfillArguments::Tempest(TempestBackfillArguments {
                    chunk_days: 1,
                    dry_run: true,
                    from: None,
                    resume: false,
                    site: "ballydidean".to_owned(),
                    station_ids: Vec::new(),
                    to: PublicStationBackfillDate::admitted("2026-11-22", YESTERDAY_MS),
                })
            }
            HistoricalBackfillConfigurationFamily::Tide => {
                HistoricalBackfillArguments::Tide(TideBackfillArguments {
                    dry_run: true,
                    from: TideBackfillDate::admitted("2026-11-22", YESTERDAY_MS),
                    resume: false,
                    site: "ballydidean".to_owned(),
                    source_keys: Vec::new(),
                    to: TideBackfillDate::admitted("2026-11-22", YESTERDAY_MS),
                })
            }
            HistoricalBackfillConfigurationFamily::PublicStations => {
                HistoricalBackfillArguments::PublicStations(PublicStationBackfillArguments {
                    dry_run: true,
                    from: None,
                    resume: false,
                    site: "ballydidean".to_owned(),
                    source_keys: Vec::new(),
                    to: PublicStationBackfillDate::admitted("2026-11-22", YESTERDAY_MS),
                })
            }
        };
        HistoricalBackfillCommand {
            arguments,
            report_path,
            source_configuration: HistoricalBackfillSourceConfiguration::Required(family),
        }
    }

    // return one stable family label for fixture output
    const fn family_name(family: HistoricalBackfillConfigurationFamily) -> &'static str {
        match family {
            HistoricalBackfillConfigurationFamily::Tempest => "tempest",
            HistoricalBackfillConfigurationFamily::Tide => "tide",
            HistoricalBackfillConfigurationFamily::PublicStations => "ambient",
        }
    }

    // return one stable family label from a typed report
    const fn report_family(report: &HistoricalBackfillReport) -> &'static str {
        match report {
            HistoricalBackfillReport::Tempest(_) => "tempest",
            HistoricalBackfillReport::Tide(_) => "tide",
            HistoricalBackfillReport::PublicStations(_) => "ambient",
        }
    }

    // recover the family prefix from the serializer fixture
    fn output_family(contents: &[u8]) -> &str {
        std::str::from_utf8(contents)
            .expect("fixture UTF-8")
            .split_once(':')
            .expect("fixture family separator")
            .0
    }

    // bind one disposable application and output to a shared trace
    fn fixture(trace: &Trace) -> (FakeApplication, FakeOutput) {
        (
            FakeApplication {
                ambient_enabled: true,
                archive_writes: false,
                public_station_catalog_override: None,
                planning_sources: Vec::new(),
                trace: Rc::clone(trace),
            },
            FakeOutput {
                fail_stdout: false,
                public_station_report: None,
                trace: Rc::clone(trace),
            },
        )
    }

    #[test]
    fn all_families_preserve_node_configuration_output_and_close_order() {
        let cases = [
            (
                HistoricalBackfillConfigurationFamily::Tempest,
                vec![
                    "load:tempest",
                    "pool:tempest",
                    "readiness:tempest",
                    "resolve:tempest",
                    "provider:tempest",
                    "time:tempest",
                    "serialize:tempest",
                    "file:tempest.json:OwnerReadWrite:tempest",
                    "stdout:tempest",
                    "close:tempest",
                ],
            ),
            (
                HistoricalBackfillConfigurationFamily::Tide,
                vec![
                    "load:tide",
                    "pool:tide",
                    "resolve:tide",
                    "provider:tide",
                    "time:tide",
                    "serialize:tide",
                    "file:tide.json:ProcessDefault:tide",
                    "stdout:tide",
                    "close:tide",
                ],
            ),
            (
                HistoricalBackfillConfigurationFamily::PublicStations,
                vec![
                    "load:ambient",
                    "pool:ambient",
                    "resolve:public-stations",
                    "time:ambient",
                    "serialize:ambient",
                    "file:ambient.json:OwnerReadWrite:ambient",
                    "stdout:ambient",
                    "close:ambient",
                ],
            ),
        ];

        // exercise each typed worker executor through the same process entrypoint
        for (family, expected) in cases {
            let trace = Rc::new(RefCell::new(Vec::new()));
            let (mut application, mut output) = fixture(&trace);
            let exit_code =
                run_historical_backfill_process(command(family), &mut application, &mut output)
                    .expect("historical process");
            assert_eq!(exit_code, 0);
            assert_eq!(trace.borrow().as_slice(), expected);
        }
    }

    // run the compiled Node owner with the repo catalog and no network ports
    fn node_public_station_oracle(arguments: &[&str]) -> String {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/public-station-dry-run-node-oracle.mjs");
        let child = Command::new("node")
            .arg(script)
            .args(arguments)
            .output()
            .expect("Node oracle");
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        String::from_utf8(child.stdout).expect("oracle TSV")
    }

    // read one already-decoded checked source returned by the original resolver
    fn oracle_planning_source(line: &str) -> Option<PublicStationBackfillPlanningSource> {
        let columns = line.split('\t').collect::<Vec<_>>();
        let [
            "source",
            adapter,
            id,
            key,
            fingerprint,
            from,
            from_ms,
            to,
            to_ms,
            chunk_days,
        ] = columns.as_slice()
        else {
            return None;
        };
        let adapter = PublicStationAdapter::WIRE_VALUES
            .iter()
            // match the generated canonical provider key
            .find(|(key, _)| key == adapter)
            .expect("catalog adapter")
            .1;
        // preserve a bounded archive end or its explicit absence
        let history_end = if *to == "-" {
            None
        } else {
            Some(PublicStationBackfillDate::admitted(
                *to,
                to_ms.parse().expect("history end"),
            ))
        };
        Some(PublicStationBackfillPlanningSource {
            adapter,
            source_id: (*id).to_owned(),
            source_key: (*key).to_owned(),
            source_config_fingerprint: (*fingerprint).to_owned(),
            history_start: PublicStationBackfillDate::admitted(
                *from,
                from_ms.parse().expect("history start"),
            ),
            history_end,
            maximum_chunk_days: chunk_days.parse().expect("provider chunk days"),
        })
    }

    // compare original rollup fields in the original source order
    fn report_signature(report: &PublicStationBackfillReport) -> String {
        report
            .sources
            .iter()
            .map(|source| {
                format!(
                    "{}|{}|{}|{}|{}|{}|{}|{}",
                    source.source,
                    source.from,
                    source.to,
                    source.planned_chunks,
                    source.skipped_chunks,
                    source.completed_chunks,
                    source.records,
                    source.exit_code,
                )
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    /// Compare all configured adapters and one selected request through the real CLI binding.
    #[test]
    fn public_station_dry_run_matches_the_node_catalog_and_resume_plan() {
        let output = node_public_station_oracle(&[]);
        let mut planning_sources = Vec::new();
        let mut reports = Vec::new();
        let mut lookups = Vec::new();
        // retain original source order and read-only resume observations
        for line in output.lines() {
            match line.split('\t').collect::<Vec<_>>().as_slice() {
                ["source", ..] => {
                    planning_sources.push(oracle_planning_source(line).expect("checked source"))
                }
                ["report", mode, value] => reports.push((*mode, *value)),
                ["lookups", mode, value] => lookups.push((*mode, *value)),
                _ => panic!("unexpected oracle line {line}"),
            }
        }
        assert_eq!(planning_sources.len(), 4);
        assert_eq!(reports.len(), 2);
        assert_eq!(lookups.len(), 2);
        // run both supported dry-run shapes with the same exact worker entrypoint
        for (mode, expected_report) in reports {
            let trace = Rc::new(RefCell::new(Vec::new()));
            let (mut application, mut output) = fixture(&trace);
            application.planning_sources = planning_sources.clone();
            let mut argv = vec![
                "--site",
                "ballydidean",
                "--dry-run",
                "--resume",
                "--from",
                "2026-08-19",
                "--to",
                "2026-08-25",
                "--report",
                "fixture.json",
            ];
            // select only the historical Weather Underground source when requested
            if mode == "selected" {
                argv.extend(["--source", "wunderground-maxweather-history-v1"]);
            } else {
                assert_eq!(mode, "all");
            }
            let argv: Vec<String> = argv.into_iter().map(str::to_owned).collect();
            let code = crate::run_historical_backfill_application(
                HistoricalBackfillCommandKind::PublicStations,
                &argv,
                &mut FakeTime,
                &mut application,
                &mut output,
            )
            .expect("public-station dry-run");
            let report = output
                .public_station_report
                .expect("typed public-station report");
            assert!(report.dry_run && report.resume);
            assert_eq!(report.site, "ballydidean");
            assert_eq!((code, report.exit_code), (0, 0));
            assert!(
                report
                    .sources
                    .iter()
                    .all(|source| source.failed_chunk.is_none())
            );
            assert_eq!(report_signature(&report), expected_report);
            let trace = trace.borrow();
            let resume = trace
                .iter()
                // compare every source-version/interval resume lookup before output
                .filter_map(|line| line.strip_prefix("lookup:"))
                .collect::<Vec<_>>()
                .join(";");
            let expected = lookups
                .iter()
                .find(|(name, _)| *name == mode)
                .expect("oracle lookups")
                .1;
            assert_eq!(resume, expected);
            assert!(!trace.iter().any(|line| line.starts_with("provider:")));
            assert_eq!(
                &trace[trace.len() - 4..],
                [
                    "serialize:ambient",
                    "file:fixture.json:OwnerReadWrite:ambient",
                    "stdout:ambient",
                    "close:ambient",
                ]
            );
        }
    }

    // bind the already decoded Node catalog fields without parsing production JSON here
    fn install_archive_catalog(
        configuration: &mut PublicStationConfiguration,
        planning: &[PublicStationBackfillPlanningSource],
        oracle: &str,
    ) {
        // retain source catalog order while constructing the injected process input
        for line in oracle.lines() {
            let columns = line.split('\t').collect::<Vec<_>>();
            // ignore report and lifecycle output
            let [
                "catalog",
                key,
                station,
                timezone,
                model,
                serial,
                fields @ ..,
            ] = columns.as_slice()
            else {
                continue;
            };
            let source = planning
                .iter()
                .find(|source| source.source_key == *key)
                .expect("matching resolved source");
            let adapter_config = fields
                .chunks_exact(2)
                .map(|pair| {
                    // keep numeric sensor ids typed rather than decoding them as strings
                    let value = if pair[0] == "sensorIndex" {
                        JsonValue::Number(pair[1].parse().expect("catalog sensor id"))
                    } else {
                        JsonValue::String(pair[1].encode_utf16().collect())
                    };
                    (pair[0].encode_utf16().collect(), value)
                })
                .collect();
            configuration
                .stations
                .push(PublicStationConfigurationStation {
                    active: true,
                    displayName: (*station).to_owned(),
                    key: (*station).to_owned(),
                    latitude: 47.9,
                    longitude: -122.4,
                    model: fixture_nullable_text(model),
                    serial: fixture_nullable_text(serial),
                    sources: vec![PublicStationSourceConfiguration {
                        active: true,
                        adapter: source.adapter,
                        adapterConfig: adapter_config,
                        cadenceSeconds: Nullable::Null,
                        capabilities: vec![SourceCapability::Historical],
                        fingerprint: source.source_config_fingerprint.clone(),
                        historyEndDate: source
                            .history_end
                            .as_ref()
                            .map_or(Nullable::Null, |date| {
                                Nullable::Value(date.label().to_owned())
                            }),
                        historyStartDate: Nullable::Value(source.history_start.label().to_owned()),
                        key: source.source_key.clone(),
                        maximumChunkDays: source.maximum_chunk_days,
                        providerKey: adapter_wire_value(source.adapter).to_owned(),
                    }],
                    timezone: (*timezone).to_owned(),
                    vendor: Nullable::Null,
                });
        }
    }

    // preserve missing device metadata at the injected catalog boundary
    fn fixture_nullable_text(value: &str) -> Nullable<String> {
        // keep the oracle's unambiguous missing marker out of requests
        if value == "-" {
            Nullable::Null
        } else {
            Nullable::Value(value.to_owned())
        }
    }

    /// Compare mixed committed archive runs against the original Node command.
    #[test]
    fn public_station_archive_matches_node_committed_lifecycle() {
        let oracle = node_public_station_oracle(&["--archive"]);
        let trace = Rc::new(RefCell::new(Vec::new()));
        let (mut application, mut output) = fixture(&trace);
        application.archive_writes = true;
        application.planning_sources = oracle.lines().filter_map(oracle_planning_source).collect();
        let mut configuration = public_station_configuration();
        install_archive_catalog(&mut configuration, &application.planning_sources, &oracle);
        assert_eq!(
            (
                configuration.stations.len(),
                application.planning_sources.len()
            ),
            (4, 4)
        );
        application.public_station_catalog_override = Some(configuration);
        let argv = [
            "--site",
            "ballydidean",
            "--resume",
            "--from",
            "2026-08-19",
            "--to",
            "2026-08-25",
            "--report",
            "archive.json",
        ]
        .map(str::to_owned);
        let code = crate::run_historical_backfill_application(
            HistoricalBackfillCommandKind::PublicStations,
            &argv,
            &mut FakeTime,
            &mut application,
            &mut output,
        )
        .expect("mixed archive execution");
        let report = output.public_station_report.expect("archive report");
        assert!(!report.dry_run && report.resume);
        assert_eq!(report.site, "ballydidean");
        assert_eq!((code, report.exit_code), (1, 1));
        let failed = report.sources[1]
            .failed_chunk
            .as_ref()
            .expect("failed WU chunk");
        assert_eq!(
            (failed.error_code.as_str(), failed.start_date.as_str()),
            ("fixture_rate_limited", "2026-08-20")
        );
        let expected_report = oracle
            .lines()
            .find_map(|line| line.strip_prefix("archive-report\t"))
            .expect("Node archive report");
        assert_eq!(report_signature(&report), expected_report);
        let expected: Vec<_> = oracle
            .lines()
            .filter(|line| line.starts_with("archive:"))
            .collect();
        let trace = trace.borrow();
        let actual: Vec<_> = trace
            .iter()
            .filter(|line| line.starts_with("archive:"))
            .map(String::as_str)
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(
            &trace[trace.len() - 4..],
            [
                "serialize:ambient",
                "file:archive.json:OwnerReadWrite:ambient",
                "stdout:ambient",
                "close:ambient",
            ]
        );
    }

    // prove original argv reaches the retained report and close lifecycle
    #[test]
    fn original_tide_argv_runs_the_public_application_in_node_order() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let (mut application, mut output) = fixture(&trace);
        let mut time = FakeTime;
        let arguments = [
            "--site",
            "ballydidean",
            "--dry-run",
            "--from",
            "2026-11-22",
            "--to",
            "2026-11-22",
            "--report",
            "tide.json",
        ]
        .map(str::to_owned);

        let exit_code = crate::run_historical_backfill_application(
            HistoricalBackfillCommandKind::Tide,
            &arguments,
            &mut time,
            &mut application,
            &mut output,
        )
        .expect("historical Tide application");

        assert_eq!(exit_code, 0);
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "load:tide",
                "pool:tide",
                "resolve:tide",
                "provider:tide",
                "time:tide",
                "serialize:tide",
                "file:tide.json:ProcessDefault:tide",
                "stdout:tide",
                "close:tide",
            ]
        );
    }

    // prove the deployed shell wrapper keeps report persistence on redirected stdout
    #[test]
    fn deployed_tide_application_preserves_wrapper_stdout_default() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let (mut application, mut output) = fixture(&trace);
        let mut time = FakeTime;

        let exit_code = crate::run_deployed_historical_backfill_application(
            HistoricalBackfillCommandKind::Tide,
            &mut time,
            &mut application,
            &mut output,
        )
        .expect("deployed Tide application");

        assert_eq!(exit_code, 0);
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "load:tide",
                "pool:tide",
                "resolve:tide",
                "provider:tide",
                "time:tide",
                "serialize:tide",
                "stdout:tide",
                "close:tide",
            ]
        );
    }

    // prove argument admission remains ahead of every owned process effect
    #[test]
    fn application_rejects_argv_before_configuration_or_output() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let (mut application, mut output) = fixture(&trace);
        let mut time = FakeTime;

        let failure = crate::run_historical_backfill_application(
            HistoricalBackfillCommandKind::Tide,
            &["--chunk-days".to_owned(), "1".to_owned()],
            &mut time,
            &mut application,
            &mut output,
        )
        .expect_err("unsupported Tide argv");

        assert_eq!(
            failure,
            crate::HistoricalBackfillApplicationError::Command(
                weather_worker::HistoricalBackfillCommandError::UnsupportedArgument(
                    "--chunk-days".to_owned(),
                )
            )
        );
        assert!(trace.borrow().is_empty());
    }

    #[test]
    fn output_failure_still_closes_the_process_pool() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let (mut application, mut output) = fixture(&trace);
        output.fail_stdout = true;
        let failure = run_historical_backfill_process(
            command(HistoricalBackfillConfigurationFamily::Tide),
            &mut application,
            &mut output,
        )
        .expect_err("stdout failure");
        assert_eq!(failure.code, "stdout_failed");
        assert_eq!(
            trace.borrow().last().map(String::as_str),
            Some("close:tide")
        );
    }

    #[test]
    fn missing_required_configuration_never_opens_a_pool() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let (mut application, mut output) = fixture(&trace);
        application.ambient_enabled = false;
        let failure = run_historical_backfill_process(
            command(HistoricalBackfillConfigurationFamily::PublicStations),
            &mut application,
            &mut output,
        )
        .expect_err("missing public-station configuration");
        assert_eq!(failure.code, PUBLIC_STATION_CONFIGURATION_REQUIRED);
        assert_eq!(trace.borrow().as_slice(), ["load:ambient"]);
    }
}
