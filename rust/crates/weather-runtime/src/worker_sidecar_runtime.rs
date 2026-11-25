//! Recompute optional worker sidecar admission from loaded runtime state for each iteration.

use std::sync::Arc;

use weather_api::{
    AdjustmentRevisionCaptureWitnessLoader, AdmittedAdjustmentRevisionCaptureWitness,
};
use weather_worker::{
    RainAdjustmentRuntimeSelection, RainCollectionConfiguration, TemperatureCanaryAuthorization,
    VerifiedTemperatureCanaryCollection, WorkerProcessFailure, WorkerProcessRuntimeLoader,
    WorkerSidecarDiagnosticSink, WorkerSidecarExecution, WorkerSidecarPhase, WorkerSidecarPlan,
    WorkerSidecarTasks, is_rain_collection_enabled, temperature_canary_collection_is_active,
};

use crate::{
    AdmittedDatabaseWorkerIteration, NativeCaptureEpochWitnessFile,
    RetainedCaptureEpochWitnessLoader, TemperatureCanaryKillSwitch, WorkerConfiguration,
    WorkerIterationDatabaseRepository,
};

const MAXIMUM_JS_DATE_MILLISECONDS: i64 = 8_640_000_000_000_000;

/// root-loader results retained independently from ordinary environment configuration
#[derive(Clone, Debug, PartialEq)]
pub struct WorkerLoadedSidecarAdmission {
    pub adjustment_capture_epoch: Option<Arc<AdmittedAdjustmentRevisionCaptureWitness>>,
    pub adjustment_maintenance_operation_loaded: bool,
    pub rain_adjustment_revision_client_loaded: bool,
    pub rain_adjustment_runtime: RainAdjustmentRuntimeSelection,
    pub temperature_canary_authorization: TemperatureCanaryAuthorization,
}

impl Default for WorkerLoadedSidecarAdmission {
    /// keep every optional loaded authority closed until independently admitted
    fn default() -> Self {
        Self {
            adjustment_capture_epoch: None,
            adjustment_maintenance_operation_loaded: false,
            rain_adjustment_revision_client_loaded: false,
            rain_adjustment_runtime: RainAdjustmentRuntimeSelection::Disabled,
            temperature_canary_authorization: TemperatureCanaryAuthorization::Inactive,
        }
    }
}

impl WorkerLoadedSidecarAdmission {
    /// Load the optional capture witness once through the existing admitted startup owner.
    pub fn load_capture_epoch(&mut self, loader: &mut impl AdjustmentRevisionCaptureWitnessLoader) {
        self.adjustment_capture_epoch = loader.load_verified_capture_witness().ok().map(Arc::new);
    }

    /// Borrow the admitted witness without reducing it to configuration presence.
    pub fn capture_epoch_witness(&self) -> Option<&AdmittedAdjustmentRevisionCaptureWitness> {
        self.adjustment_capture_epoch.as_deref()
    }
}

/// owned configuration projection used to admit one iteration's optional sidecars
#[derive(Clone, Debug, PartialEq)]
pub struct DatabaseWorkerSidecarAdmission {
    adjustment_maintenance_origin_configured: bool,
    loaded: WorkerLoadedSidecarAdmission,
    rain_capture_configured: bool,
    temperature_canary_environment_admitted: bool,
}

impl DatabaseWorkerSidecarAdmission {
    /// project the loaded worker configuration without retaining database or credential material
    pub fn from_configuration(
        configuration: &WorkerConfiguration,
        loaded: WorkerLoadedSidecarAdmission,
    ) -> Self {
        let rain_capture_configured = is_rain_collection_enabled(RainCollectionConfiguration {
            open_meteo_compatibility_origin: configuration
                .open_meteo_compatibility_origin
                .as_deref(),
            rain_collection_enabled: configuration.rain_collection_enabled,
            site_key: &configuration.site.site.key,
            version: &configuration.version,
        });
        Self {
            adjustment_maintenance_origin_configured: configuration
                .adjustment_maintenance_api_origin
                .is_some(),
            loaded,
            rain_capture_configured,
            temperature_canary_environment_admitted: configuration.temperature_canary_kill_switch
                == Some(TemperatureCanaryKillSwitch::Zero),
        }
    }

    /// recompute time-sensitive canary admission for one loop
    pub fn plan_at(&self, loop_at_ms: i64) -> WorkerSidecarPlan {
        let capture_epoch = self.loaded.capture_epoch_witness().is_some();
        let revision_client = self.adjustment_maintenance_origin_configured
            && capture_epoch
            && self.loaded.rain_adjustment_revision_client_loaded;
        let maintenance_capture = self.adjustment_maintenance_origin_configured
            && capture_epoch
            && self.loaded.adjustment_maintenance_operation_loaded;
        let valid_loop_clock =
            (-MAXIMUM_JS_DATE_MILLISECONDS..=MAXIMUM_JS_DATE_MILLISECONDS).contains(&loop_at_ms);
        let temperature_active = self.temperature_canary_environment_admitted
            && valid_loop_clock
            && temperature_canary_collection_is_active(
                self.loaded.temperature_canary_authorization,
                loop_at_ms,
            );

        WorkerSidecarPlan {
            adjustment_capture_epoch_configured: capture_epoch,
            maintenance_capture_configured: maintenance_capture,
            rain_adjustment_revision_client_configured: revision_client,
            rain_capture_configured: self.rain_capture_configured,
            rain_inference_legacy_enabled: self.rain_capture_configured,
            rain_inference_runtime: self.loaded.rain_adjustment_runtime,
            // project the dynamic authorization into the coordinator's closed state
            temperature_canary: if temperature_active {
                VerifiedTemperatureCanaryCollection::Active
            } else {
                VerifiedTemperatureCanaryCollection::Inactive
            },
        }
    }
}

/// process-owned dynamic sidecar phase over the same repository used by source ingestion
pub struct DatabaseWorkerSidecarPhase<Tasks, Diagnostics> {
    admission: DatabaseWorkerSidecarAdmission,
    diagnostics: Diagnostics,
    tasks: Tasks,
}

impl<Tasks, Diagnostics> DatabaseWorkerSidecarPhase<Tasks, Diagnostics> {
    /// bind admitted runtime state to concrete task and diagnostic owners
    pub const fn new(
        admission: DatabaseWorkerSidecarAdmission,
        tasks: Tasks,
        diagnostics: Diagnostics,
    ) -> Self {
        Self {
            admission,
            diagnostics,
            tasks,
        }
    }

    /// Retain the same witness for optional startup clients and iteration-side capture.
    pub fn capture_epoch_witness(&self) -> Option<&AdmittedAdjustmentRevisionCaptureWitness> {
        self.admission.loaded.capture_epoch_witness()
    }
}

/// Load the native capture witness during the existing worker runtime preparation phase.
pub struct DatabaseWorkerRuntimeWithCaptureEpoch<Runtime, Witness> {
    runtime: Runtime,
    witness: Witness,
}

impl<Runtime, Witness> DatabaseWorkerRuntimeWithCaptureEpoch<Runtime, Witness> {
    /// Bind the required worker runtimes and the common installed witness loader.
    pub const fn new(runtime: Runtime, witness: Witness) -> Self {
        Self { runtime, witness }
    }
}

impl<Runtime, Codec, Time>
    DatabaseWorkerRuntimeWithCaptureEpoch<
        Runtime,
        RetainedCaptureEpochWitnessLoader<NativeCaptureEpochWitnessFile, Codec, Time>,
    >
{
    /// Select the original installed mount for the worker's required startup phase.
    pub const fn native(runtime: Runtime, codec: Codec, time: Time) -> Self {
        Self::new(
            runtime,
            RetainedCaptureEpochWitnessLoader::native(codec, time),
        )
    }
}

impl<Pool, Runtime, Witness, Dispatch, Tasks, Diagnostics>
    WorkerProcessRuntimeLoader<WorkerConfiguration, Pool>
    for DatabaseWorkerRuntimeWithCaptureEpoch<Runtime, Witness>
where
    Runtime: WorkerProcessRuntimeLoader<
            WorkerConfiguration,
            Pool,
            Runtime = AdmittedDatabaseWorkerIteration<
                Dispatch,
                DatabaseWorkerSidecarPhase<Tasks, Diagnostics>,
            >,
        >,
    Witness: AdjustmentRevisionCaptureWitnessLoader,
{
    type Runtime = Runtime::Runtime;

    /// Check the witness after provider/model preparation even with no optional origin set.
    fn load_required_runtime(
        &mut self,
        configuration: &WorkerConfiguration,
        pool: &mut Pool,
    ) -> Result<Self::Runtime, WorkerProcessFailure> {
        let mut runtime = self.runtime.load_required_runtime(configuration, pool)?;
        runtime
            .sidecar_phase
            .admission
            .loaded
            .load_capture_epoch(&mut self.witness);
        Ok(runtime)
    }

    /// Let the existing optional factory construct clients with the already retained witness.
    fn load_startup_maintenance(
        &mut self,
        configuration: &WorkerConfiguration,
        pool: &mut Pool,
        runtime: &mut Self::Runtime,
    ) {
        // let an unavailable witness keep optional clients inactive without failing startup
        if runtime.sidecar_phase.capture_epoch_witness().is_some() {
            self.runtime
                .load_startup_maintenance(configuration, pool, runtime);
        }
    }
}

impl<'repository, Database, Values, Tasks, Diagnostics>
    WorkerSidecarPhase<WorkerIterationDatabaseRepository<'repository, Database, Values>>
    for DatabaseWorkerSidecarPhase<Tasks, Diagnostics>
where
    Diagnostics: WorkerSidecarDiagnosticSink,
    Tasks: WorkerSidecarTasks<WorkerIterationDatabaseRepository<'repository, Database, Values>>,
{
    /// delegate each freshly admitted plan to the worker's single sidecar coordinator
    fn run<Clock: FnMut() -> i64>(
        &mut self,
        repository: &mut WorkerIterationDatabaseRepository<'repository, Database, Values>,
        loop_at_ms: i64,
        clock: &mut Clock,
    ) {
        let mut execution = WorkerSidecarExecution::new(
            self.admission.plan_at(loop_at_ms),
            &mut self.tasks,
            &mut self.diagnostics,
        );
        WorkerSidecarPhase::run(&mut execution, repository, loop_at_ms, clock);
    }
}

#[cfg(test)]
mod tests {
    use weather_api::{
        AdmittedAdjustmentRevisionCaptureWitness, CanonicalInstant, WeatherApiStartupLoadError,
    };
    use weather_contracts::{
        AdjustmentRevisionCaptureEpochWitness,
        AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral,
        AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral,
        AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral,
        AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum, Nullable, SiteConfiguration,
        SiteConfigurationProvider, SiteConfigurationSite, SiteConfigurationStation, StationKind,
    };
    use weather_database::DatabaseConfiguration;
    use weather_worker::{
        FixedGaugeTargetResult, MaintenanceCaptureDue, RainCaptureResult, TaskFailure,
        TemperatureCanaryCollectionResult, WorkerSidecarDiagnostic,
    };

    use super::*;

    const LOOP_AT_MS: i64 = 1_800_000_000_000;

    #[derive(Default)]
    struct Tasks {
        events: Vec<&'static str>,
    }

    impl<'repository> WorkerSidecarTasks<WorkerIterationDatabaseRepository<'repository, (), ()>>
        for Tasks
    {
        /// retain one temperature task invocation
        fn collect_temperature_canary(
            &mut self,
            _repository: &mut WorkerIterationDatabaseRepository<'repository, (), ()>,
        ) -> Result<TemperatureCanaryCollectionResult, TaskFailure> {
            self.events.push("temperature");
            Ok(TemperatureCanaryCollectionResult {
                failed_runs: 0,
                persisted_runs: 1,
            })
        }

        /// retain one rain-capture task invocation
        fn collect_rain_capture(
            &mut self,
            _repository: &mut WorkerIterationDatabaseRepository<'repository, (), ()>,
        ) -> Result<RainCaptureResult, TaskFailure> {
            self.events.push("rain");
            Ok(RainCaptureResult {
                failed: 0,
                valid: 1,
            })
        }

        /// retain one fixed-target task invocation
        fn publish_fixed_gauge_target(
            &mut self,
            _repository: &mut WorkerIterationDatabaseRepository<'repository, (), ()>,
        ) -> Result<FixedGaugeTargetResult, TaskFailure> {
            self.events.push("fixed");
            Ok(FixedGaugeTargetResult::Idle)
        }

        /// retain one inference task invocation
        fn publish_rain_inference(
            &mut self,
            _repository: &mut WorkerIterationDatabaseRepository<'repository, (), ()>,
            _now_ms: i64,
        ) -> Result<bool, TaskFailure> {
            self.events.push("inference");
            Ok(false)
        }

        /// retain one maintenance task invocation
        fn capture_adjustment_maintenance(
            &mut self,
            _repository: &mut WorkerIterationDatabaseRepository<'repository, (), ()>,
            _due: MaintenanceCaptureDue,
        ) -> Result<(), TaskFailure> {
            self.events.push("maintenance");
            Ok(())
        }
    }

    #[derive(Default)]
    struct Diagnostics(Vec<WorkerSidecarDiagnostic>);

    impl WorkerSidecarDiagnosticSink for Diagnostics {
        /// retain ordinary coordinator diagnostics
        fn record_sidecar_diagnostic(&mut self, diagnostic: WorkerSidecarDiagnostic) {
            self.0.push(diagnostic);
        }
    }

    /// preserve inclusive activation, exclusive expiry, and policy-raw comparator inference
    #[test]
    fn phase_recomputes_time_limited_admission_for_each_loop() {
        let admission = DatabaseWorkerSidecarAdmission::from_configuration(
            &configuration(None, Some("https://maintenance.example")),
            WorkerLoadedSidecarAdmission {
                adjustment_capture_epoch: Some(Arc::new(capture_witness())),
                adjustment_maintenance_operation_loaded: true,
                rain_adjustment_revision_client_loaded: true,
                rain_adjustment_runtime:
                    RainAdjustmentRuntimeSelection::DisabledPolicyRawWithComparatorAuthority,
                temperature_canary_authorization: TemperatureCanaryAuthorization::TimeLimited {
                    activated_at_ms: LOOP_AT_MS,
                    expires_at_ms: LOOP_AT_MS + 1_000,
                },
            },
        );
        let mut phase =
            DatabaseWorkerSidecarPhase::new(admission, Tasks::default(), Diagnostics::default());
        let mut database = ();
        let mut values = ();
        let mut repository = WorkerIterationDatabaseRepository::new(&mut database, &mut values);
        let mut clock_value = LOOP_AT_MS;
        let mut clock = || {
            clock_value += 1;
            clock_value
        };

        WorkerSidecarPhase::run(&mut phase, &mut repository, LOOP_AT_MS, &mut clock);
        WorkerSidecarPhase::run(&mut phase, &mut repository, LOOP_AT_MS + 1_000, &mut clock);

        assert_eq!(
            phase.tasks.events,
            [
                "temperature",
                "rain",
                "fixed",
                "inference",
                "maintenance",
                "rain",
                "fixed",
                "inference",
                "maintenance",
            ]
        );
    }

    /// reject compatibility collection and close operations without every loaded prerequisite
    #[test]
    fn plan_enforces_compatibility_and_loaded_operation_prerequisites() {
        let compatibility = DatabaseWorkerSidecarAdmission::from_configuration(
            &configuration(
                Some("https://open-meteo-compatibility.example"),
                Some("https://maintenance.example"),
            ),
            WorkerLoadedSidecarAdmission {
                adjustment_capture_epoch: Some(Arc::new(capture_witness())),
                adjustment_maintenance_operation_loaded: false,
                rain_adjustment_revision_client_loaded: false,
                rain_adjustment_runtime: RainAdjustmentRuntimeSelection::Active,
                temperature_canary_authorization: TemperatureCanaryAuthorization::Permanent {
                    activated_at_ms: LOOP_AT_MS,
                },
            },
        );
        let plan = compatibility.plan_at(LOOP_AT_MS);

        assert!(!plan.rain_capture_configured);
        assert!(!plan.rain_inference_legacy_enabled);
        assert!(plan.adjustment_capture_epoch_configured);
        assert!(!plan.rain_adjustment_revision_client_configured);
        assert!(!plan.maintenance_capture_configured);
        assert_eq!(
            plan.temperature_canary,
            VerifiedTemperatureCanaryCollection::Active
        );
        assert_eq!(
            compatibility
                .plan_at(MAXIMUM_JS_DATE_MILLISECONDS + 1)
                .temperature_canary,
            VerifiedTemperatureCanaryCollection::Inactive
        );

        let mut killed_configuration = configuration(None, Some("https://maintenance.example"));
        killed_configuration.temperature_canary_kill_switch = None;
        let killed = DatabaseWorkerSidecarAdmission::from_configuration(
            &killed_configuration,
            WorkerLoadedSidecarAdmission {
                temperature_canary_authorization: TemperatureCanaryAuthorization::Permanent {
                    activated_at_ms: LOOP_AT_MS,
                },
                ..WorkerLoadedSidecarAdmission::default()
            },
        )
        .plan_at(LOOP_AT_MS);
        assert_eq!(
            killed.temperature_canary,
            VerifiedTemperatureCanaryCollection::Inactive
        );

        let missing_origin = DatabaseWorkerSidecarAdmission::from_configuration(
            &configuration(None, None),
            WorkerLoadedSidecarAdmission {
                adjustment_capture_epoch: Some(Arc::new(capture_witness())),
                adjustment_maintenance_operation_loaded: true,
                rain_adjustment_revision_client_loaded: true,
                ..WorkerLoadedSidecarAdmission::default()
            },
        )
        .plan_at(LOOP_AT_MS);
        assert!(missing_origin.adjustment_capture_epoch_configured);
        assert!(!missing_origin.rain_adjustment_revision_client_configured);
        assert!(!missing_origin.maintenance_capture_configured);

        let closed = DatabaseWorkerSidecarAdmission::from_configuration(
            &configuration(None, Some("https://maintenance.example")),
            WorkerLoadedSidecarAdmission::default(),
        )
        .plan_at(LOOP_AT_MS);
        assert_eq!(
            closed.temperature_canary,
            VerifiedTemperatureCanaryCollection::Inactive
        );
        assert_eq!(
            closed.rain_inference_runtime,
            RainAdjustmentRuntimeSelection::Disabled
        );
        assert!(!closed.adjustment_capture_epoch_configured);
        assert!(!closed.rain_adjustment_revision_client_configured);
        assert!(!closed.maintenance_capture_configured);
    }

    /// Load and retain the actual admitted witness independently of optional origin settings.
    #[test]
    fn loads_and_exposes_capture_epoch_witness_once_at_startup() {
        let configuration = configuration(None, None);
        let mut loader =
            DatabaseWorkerRuntimeWithCaptureEpoch::new(RequiredRuntime, WitnessLoader { calls: 0 });
        let runtime = loader
            .load_required_runtime(&configuration, &mut ())
            .expect("original required startup");
        let phase = runtime.sidecar_phase;
        assert_eq!(loader.witness.calls, 1);
        assert_eq!(
            phase
                .capture_epoch_witness()
                .expect("admitted capture witness")
                .capture_epoch
                .utc,
            "2000-01-01T00:00:00.000Z"
        );
        assert!(
            phase
                .admission
                .plan_at(LOOP_AT_MS)
                .adjustment_capture_epoch_configured
        );
    }

    struct RequiredRuntime;

    impl WorkerProcessRuntimeLoader<WorkerConfiguration, ()> for RequiredRuntime {
        type Runtime = AdmittedDatabaseWorkerIteration<(), DatabaseWorkerSidecarPhase<(), ()>>;

        /// Bind the original worker runtime before a witness is read.
        fn load_required_runtime(
            &mut self,
            configuration: &WorkerConfiguration,
            _pool: &mut (),
        ) -> Result<Self::Runtime, WorkerProcessFailure> {
            Ok(AdmittedDatabaseWorkerIteration {
                dispatcher: (),
                sidecar_phase: DatabaseWorkerSidecarPhase::new(
                    DatabaseWorkerSidecarAdmission::from_configuration(
                        configuration,
                        WorkerLoadedSidecarAdmission::default(),
                    ),
                    (),
                    (),
                ),
            })
        }

        /// The no-origin fixture has no optional startup clients to prepare.
        fn load_startup_maintenance(
            &mut self,
            _: &WorkerConfiguration,
            _: &mut (),
            _: &mut Self::Runtime,
        ) {
        }
    }

    struct WitnessLoader {
        calls: usize,
    }

    impl AdjustmentRevisionCaptureWitnessLoader for WitnessLoader {
        /// Return the fixture witness while proving the startup owner is called once.
        fn load_verified_capture_witness(
            &mut self,
        ) -> Result<AdmittedAdjustmentRevisionCaptureWitness, WeatherApiStartupLoadError> {
            self.calls += 1;
            Ok(capture_witness())
        }
    }

    /// Build one actual admitted capture witness and its matching canonical instant.
    fn capture_witness() -> AdmittedAdjustmentRevisionCaptureWitness {
        AdmittedAdjustmentRevisionCaptureWitness::from_verified(
            AdjustmentRevisionCaptureEpochWitness {
                activationKind: AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral,
                archiveCommitOrdinal:
                    AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral,
                catalogFrontierSha256: "1".repeat(64),
                contractVersion: AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral,
                controlPlaneSha256: "2".repeat(64),
                controlPlaneVersion:
                    AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value17,
                databaseMigrationHistorySha256: "3".repeat(64),
                epochAt: "2000-01-01T00:00:00.000Z".to_owned(),
                servingSnapshotSha256: "4".repeat(64),
                sourceCommit: "abc123".to_owned(),
                sourceRelease: "v1".to_owned(),
                sourceServerImageDigest: "sha256:server".to_owned(),
                sourceWebImageDigest: "sha256:web".to_owned(),
                witnessSha256: "5".repeat(64),
            },
            CanonicalInstant {
                epoch_milliseconds: 946_684_800_000,
                utc: "2000-01-01T00:00:00.000Z".to_owned(),
            },
        )
        .expect("matching capture witness fixture")
    }

    /// build one loaded configuration without external environment or file access
    fn configuration(
        compatibility_origin: Option<&str>,
        maintenance_origin: Option<&str>,
    ) -> WorkerConfiguration {
        WorkerConfiguration {
            adjustment_maintenance_api_origin: maintenance_origin.map(str::to_owned),
            database: DatabaseConfiguration {
                application_name: "fixture".to_owned(),
                database: "fixture".to_owned(),
                host: "fixture".to_owned(),
                lock_timeout_ms: 10_000,
                password: "fixture".to_owned(),
                port: 5432,
                ssl: false,
                statement_timeout_ms: 30_000,
                user: "fixture".to_owned(),
            },
            ecowitt: None,
            instance: "fixture".to_owned(),
            migration_authorization: None,
            migration_directory: "migrations".to_owned(),
            open_meteo_compatibility_origin: compatibility_origin.map(str::to_owned),
            public_stations: None,
            rain_collection_enabled: true,
            site: site_configuration(),
            site_configuration_path: "site.json".to_owned(),
            tempest: None,
            tempest_api_key: None,
            temperature_canary_kill_switch: Some(TemperatureCanaryKillSwitch::Zero),
            tides: None,
            version: "2026.11.23-1".to_owned(),
        }
    }

    /// build the exact rain-policy site identity used by the admission fixture
    fn site_configuration() -> SiteConfiguration {
        SiteConfiguration {
            provider: SiteConfigurationProvider {
                active: true,
                attributionLabel: String::new(),
                attributionUrl: String::new(),
                displayName: "Fixture".to_owned(),
                key: "fixture".to_owned(),
            },
            site: SiteConfigurationSite {
                active: true,
                displayName: "Ballydidean".to_owned(),
                key: "ballydidean".to_owned(),
                latitude: 47.95,
                longitude: -122.43,
                timezone: "America/Los_Angeles".to_owned(),
            },
            sources: Vec::new(),
            station: SiteConfigurationStation {
                active: true,
                displayName: "Fixture".to_owned(),
                key: "fixture".to_owned(),
                kind: StationKind::Virtual,
                latitude: 47.95,
                longitude: -122.43,
                model: Nullable::Null,
                serial: Nullable::Null,
                vendor: Nullable::Null,
            },
            version: 1,
        }
    }
}
