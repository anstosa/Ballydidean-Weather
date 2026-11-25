//! Concrete installed owner for one lock-serialized custody capture cycle.

use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use crate::native_maintenance_capture_cycle_core::{
    NativeMaintenanceCaptureLifecycleInitialization, NativeMaintenanceEvidenceSynchronization,
    initialize_native_maintenance_capture_lifecycle, synchronize_native_maintenance_evidence,
};
use crate::{
    NativeMaintenanceArchiveSegmentClock, NativeMaintenanceArchiveWallClock,
    NativeMaintenanceCaptureCycleLockPort, NativeMaintenanceCaptureGenesisDocuments,
    NativeMaintenanceCaptureGenesisHost, NativeMaintenanceCaptureNativeInstallation,
    NativeMaintenanceCaptureReleaseGeneration, NativeMaintenanceControllerArchiveValidators,
    NativeMaintenanceControllerProcessLock, NativeMaintenanceDatabaseLedgerReader,
    NativeMaintenanceEvidenceCycleHost, NativeMaintenanceFixedOpenPackStore,
    NativeMaintenanceJournalStore, NativeMaintenanceOpenPack, NativeMaintenancePlaintextArchive,
    NativeMaintenanceRegistrationLifecycleHost, NativeMaintenanceRegistrationLifecycleRemote,
    NativeRevisionColdDrainResult, NativeWirePrimitives,
    inspect_native_maintenance_installed_host_readiness,
};

const RELEASES_ROOT: &str = "/home/ubuntu/.weather/adjustment-maintenance/runner/releases";
const EXECUTABLE_RELATIVE_PATH: &str = "bin/weather-maintenance-capture";

/// Exact successful lifecycle, wall-clock cutoff, and mutually exclusive custody result.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceCaptureCycleOutcome {
    pub lifecycle: NativeMaintenanceCaptureLifecycleInitialization,
    pub cutoff_at: String,
    pub evidence: NativeMaintenanceEvidenceSynchronization<NativeRevisionColdDrainResult>,
}

/// Closed installed-host phase that failed before a complete custody outcome existed.
#[derive(Debug)]
pub enum NativeMaintenanceCaptureCycleHostError {
    Installation,
    Lock(io::Error),
    Journal,
    Archive,
    OpenPack,
    Lifecycle,
    Cutoff,
    Evidence,
}

impl Display for NativeMaintenanceCaptureCycleHostError {
    /// Report only the owning fixed phase without reflecting private host material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve the first source-ordered failure boundary
        formatter.write_str(match self {
            Self::Installation => "maintenance capture installation is invalid",
            Self::Lock(_) => "maintenance capture controller lock is unavailable",
            Self::Journal => "maintenance capture journal initialization failed",
            Self::Archive => "maintenance capture archive initialization failed",
            Self::OpenPack => "maintenance capture open pack initialization failed",
            Self::Lifecycle => "maintenance capture lifecycle initialization failed",
            Self::Cutoff => "maintenance capture cutoff is unavailable",
            Self::Evidence => "maintenance capture evidence synchronization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceCaptureCycleHostError {
    /// Retain only the ordinary process-lock I/O cause.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // other owners intentionally expose only their bounded phase category
        match self {
            Self::Lock(error) => Some(error),
            _ => None,
        }
    }
}

type RegistrationHost = NativeMaintenanceRegistrationLifecycleHost<
    NativeMaintenanceDatabaseLedgerReader,
    NativeWirePrimitives,
    NativeWirePrimitives,
>;

/// Retained real remote owners for the installed native custody-only capture command.
pub struct NativeMaintenanceCaptureCycleHost {
    epoch_remote: NativeMaintenanceDatabaseLedgerReader,
    registration: RegistrationHost,
    evidence_remote: NativeMaintenanceDatabaseLedgerReader,
    last_outcome: Option<NativeMaintenanceCaptureCycleOutcome>,
}

impl NativeMaintenanceCaptureCycleHost {
    /// Verify the literal running installed binary and bind only its fixed release owners.
    pub fn for_current_installed_process() -> Result<Self, NativeMaintenanceCaptureCycleHostError> {
        use NativeMaintenanceCaptureCycleHostError as Error;

        let executable = std::env::current_exe().map_err(|_| Error::Installation)?;
        let release_root = release_root_from_executable(&executable).ok_or(Error::Installation)?;
        let source_commit = release_root
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(Error::Installation)?
            .to_owned();
        let installation =
            NativeMaintenanceCaptureNativeInstallation::native_release(&source_commit)
                .map_err(|_| Error::Installation)?;
        let verified = installation.verify().map_err(|_| Error::Installation)?;
        // require the fixed native generation and same path-derived immutable source commit
        if verified.release.generation != NativeMaintenanceCaptureReleaseGeneration::Native
            || verified.release.source_commit != source_commit
            || verified.release.member_count == 0
        {
            return Err(Error::Installation);
        }
        let epoch_remote = NativeMaintenanceDatabaseLedgerReader::new(&release_root);
        let registration_remote = NativeMaintenanceDatabaseLedgerReader::new(&release_root);
        let evidence_remote = NativeMaintenanceDatabaseLedgerReader::new(&release_root);
        let registration = NativeMaintenanceRegistrationLifecycleHost::new(
            NativeMaintenanceRegistrationLifecycleRemote::new(
                registration_remote,
                NativeWirePrimitives::new(),
                NativeWirePrimitives::new(),
            ),
        );
        Ok(Self {
            epoch_remote,
            registration,
            evidence_remote,
            last_outcome: None,
        })
    }

    /// Run the full custody-only cycle while holding the original controller flock.
    pub async fn run_complete_capture_cycle_with_outcome(
        &mut self,
    ) -> Result<NativeMaintenanceCaptureCycleOutcome, NativeMaintenanceCaptureCycleHostError> {
        let _lock = NativeMaintenanceControllerProcessLock::acquire()
            .map_err(NativeMaintenanceCaptureCycleHostError::Lock)?;
        self.run_locked_capture_cycle().await
    }

    /// Return and clear the most recent complete outcome retained for the unit-returning job port.
    pub fn take_last_outcome(&mut self) -> Option<NativeMaintenanceCaptureCycleOutcome> {
        self.last_outcome.take()
    }

    /// Execute the existing read-only installed readiness authority without starting a cycle.
    pub fn inspect_installed_readiness(&self) -> crate::NativeMaintenanceCaptureReadiness {
        inspect_native_maintenance_installed_host_readiness()
    }

    /// Preserve source journal/archive/lifecycle/cutoff/evidence order beneath the held lock.
    async fn run_locked_capture_cycle(
        &mut self,
    ) -> Result<NativeMaintenanceCaptureCycleOutcome, NativeMaintenanceCaptureCycleHostError> {
        use NativeMaintenanceCaptureCycleHostError as Error;

        let (mut lifecycle_journal, mut archive_journal, mut cold_journal) =
            initialize_journal_handles()?;
        let mut archive = NativeMaintenancePlaintextArchive::initialize(
            NativeMaintenanceControllerArchiveValidators::new(),
        )
        .map_err(|_| Error::Archive)?;
        let open_store =
            NativeMaintenanceFixedOpenPackStore::initialize().map_err(|_| Error::OpenPack)?;
        let mut pack = NativeMaintenanceOpenPack { store: open_store };

        let mut clock = NativeMaintenanceArchiveWallClock;
        // release lifecycle borrows at the original custody cutoff boundary
        let lifecycle = {
            let documents = NativeMaintenanceCaptureGenesisDocuments::default();
            let mut authority = crate::NativeMaintenanceCaptureEpochLedgerHost {
                remote: &mut self.epoch_remote,
                documents: &documents,
            };
            let mut genesis = NativeMaintenanceCaptureGenesisHost {
                archive: &mut archive,
                clock: &mut clock,
                documents: &documents,
                journal: &mut lifecycle_journal,
            };
            initialize_native_maintenance_capture_lifecycle(
                &mut authority,
                &mut self.registration,
                &mut genesis,
            )
            .await
            .map_err(|_| Error::Lifecycle)?
        };
        let cutoff_at = clock.now().ok_or(Error::Cutoff)?;
        let mut evidence = NativeMaintenanceEvidenceCycleHost {
            archive: &mut archive,
            pack: &mut pack,
            remote: &mut self.evidence_remote,
            archive_journal: &mut archive_journal,
            cold_journal: &mut cold_journal,
        };
        let synchronized = synchronize_native_maintenance_evidence(&mut evidence, &cutoff_at)
            .await
            .map_err(|_| Error::Evidence)?;
        Ok(NativeMaintenanceCaptureCycleOutcome {
            lifecycle,
            cutoff_at,
            evidence: synchronized,
        })
    }
}

impl NativeMaintenanceCaptureCycleLockPort for NativeMaintenanceCaptureCycleHost {
    type Error = NativeMaintenanceCaptureCycleHostError;

    /// Run one genuine typed cycle under the real process lock and retain its explicit outcome.
    fn run_complete_capture_cycle_with_process_lock(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + '_>> {
        Box::pin(async move {
            // never expose an earlier successful cycle after a later attempt fails
            self.last_outcome = None;
            let outcome = self.run_complete_capture_cycle_with_outcome().await?;
            self.last_outcome = Some(outcome);
            Ok(())
        })
    }
}

/// Derive only the fixed installed release root from the literal native process path.
fn release_root_from_executable(executable: &Path) -> Option<PathBuf> {
    let release_root = executable.parent()?.parent()?.to_path_buf();
    let source_commit = release_root.file_name()?.to_str()?;
    let expected = Path::new(RELEASES_ROOT).join(source_commit);
    // require the exact fixed runner path before any caller can construct remote owners
    if release_root != expected || executable != expected.join(EXECUTABLE_RELATIVE_PATH) {
        return None;
    }
    Some(release_root)
}

/// Open independent handles to the same verified journal for its three sequential owners.
fn initialize_journal_handles() -> Result<
    (
        NativeMaintenanceJournalStore,
        NativeMaintenanceJournalStore,
        NativeMaintenanceJournalStore,
    ),
    NativeMaintenanceCaptureCycleHostError,
> {
    let (lifecycle, lifecycle_status) = NativeMaintenanceJournalStore::initialize(
        &mut NativeWirePrimitives::new(),
        &NativeWirePrimitives::new(),
    )
    .map_err(|_| NativeMaintenanceCaptureCycleHostError::Journal)?;
    let (archive, archive_status) = NativeMaintenanceJournalStore::initialize(
        &mut NativeWirePrimitives::new(),
        &NativeWirePrimitives::new(),
    )
    .map_err(|_| NativeMaintenanceCaptureCycleHostError::Journal)?;
    let (cold, cold_status) = NativeMaintenanceJournalStore::initialize(
        &mut NativeWirePrimitives::new(),
        &NativeWirePrimitives::new(),
    )
    .map_err(|_| NativeMaintenanceCaptureCycleHostError::Journal)?;
    // require all handles to observe the same reconciled journal head and generation
    if lifecycle_status != archive_status || lifecycle_status != cold_status {
        return Err(NativeMaintenanceCaptureCycleHostError::Journal);
    }
    Ok((lifecycle, archive, cold))
}
