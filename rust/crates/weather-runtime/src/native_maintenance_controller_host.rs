//! Installed maintenance controller composition over the native journal, archive, custody and due owners.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use crate::{
    NativeMaintenanceArchiveSegmentClock, NativeMaintenanceArchiveWallClock,
    NativeMaintenanceCaptureEpochLedgerHost, NativeMaintenanceCaptureGenesisDocuments,
    NativeMaintenanceCaptureGenesisHost, NativeMaintenanceCaptureNativeInstallation,
    NativeMaintenanceCaptureReleaseGeneration, NativeMaintenanceColdArchiveHost,
    NativeMaintenanceControllerArchiveValidators, NativeMaintenanceControllerProcessLock,
    NativeMaintenanceControllerRunPorts, NativeMaintenanceDatabaseLedgerReader,
    NativeMaintenanceDue, NativeMaintenanceDueEvidence, NativeMaintenanceDueExecutionPorts,
    NativeMaintenanceDueExecutionResult, NativeMaintenanceDueReadPort, NativeMaintenanceDueState,
    NativeMaintenanceEvidenceCycleHost, NativeMaintenanceFixedOpenPackStore,
    NativeMaintenanceJournalDueReader, NativeMaintenanceJournalStore, NativeMaintenanceOpenPack,
    NativeMaintenancePlaintextArchive, NativeMaintenanceRegistrationLifecycleHost,
    NativeMaintenanceRegistrationLifecycleRemote, NativeRegistrationScheduleStatus,
    NativeWirePrimitives, execute_native_maintenance_due,
    initialize_native_maintenance_capture_lifecycle, registration_schedule_from_lifecycle,
    synchronize_native_maintenance_evidence,
};

const RELEASES_ROOT: &str = "/home/ubuntu/.weather/adjustment-maintenance/runner/releases";
const EXECUTABLE: &str = "bin/weather-maintenance-capture";

type NativeArchive =
    NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>;
type NativePack = NativeMaintenanceOpenPack<NativeMaintenanceFixedOpenPackStore>;
type Registration = NativeMaintenanceRegistrationLifecycleHost<
    NativeMaintenanceDatabaseLedgerReader,
    NativeWirePrimitives,
    NativeWirePrimitives,
>;
type HostFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, NativeMaintenanceControllerHostError>> + 'a>>;

/// Closed installation and controller effect failures with no private host paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceControllerHostError {
    Installation,
    Lock,
    Journal,
    Archive,
    Lifecycle,
    Schedule,
    Evidence,
    Clock,
    Due,
}

struct Journals {
    controller: NativeMaintenanceJournalStore,
    archive: NativeMaintenanceJournalStore,
    cold: NativeMaintenanceJournalStore,
}

/// Actual installed due owner binds the exact source lifecycle snapshot before any semantic work.
pub trait NativeMaintenanceControllerInstalledPorts: NativeMaintenanceDueExecutionPorts {
    /// Retain the authenticated epoch bytes and initial registration transaction for one locked run.
    fn bind_initial_scope(
        &mut self,
        schedule: NativeRegistrationScheduleStatus,
        lifecycle: crate::NativeRegistrationLifecycleStatus,
        witness: crate::VerifiedNativeMaintenanceCaptureEpochWitness,
        witness_bytes: Vec<u8>,
    ) -> Result<(), Self::Error>;
}

/// Actual installed controller owners; only family semantic/release effects remain parameterized.
pub struct NativeMaintenanceControllerHost<Ports> {
    witness: Option<(crate::VerifiedNativeMaintenanceCaptureEpochWitness, Vec<u8>)>,
    epoch_remote: NativeMaintenanceDatabaseLedgerReader,
    evidence_remote: NativeMaintenanceDatabaseLedgerReader,
    registration: Registration,
    schedule: NativeMaintenanceRegistrationLifecycleRemote<
        NativeMaintenanceDatabaseLedgerReader,
        NativeWirePrimitives,
        NativeWirePrimitives,
    >,
    journals: Option<Journals>,
    archive: Option<NativeArchive>,
    pack: Option<NativePack>,
    due_ports: Ports,
}

impl<Ports: NativeMaintenanceControllerInstalledPorts> NativeMaintenanceControllerHost<Ports> {
    /// Admit only the literal fixed native binary and its immutable release before constructing any owner.
    pub fn for_current_installed_process(
        build_due_ports: impl FnOnce(&Path, &str) -> Result<Ports, NativeMaintenanceControllerHostError>,
    ) -> Result<Self, NativeMaintenanceControllerHostError> {
        use NativeMaintenanceControllerHostError as Error;
        let current = std::env::current_exe().map_err(|_| Error::Installation)?;
        let (root, commit) = installed_root(&current).ok_or(Error::Installation)?;
        let proof = NativeMaintenanceCaptureNativeInstallation::native_release(&commit)
            .and_then(|owner| owner.verify())
            .map_err(|_| Error::Installation)?;
        // require the same current installed native identity and source commit
        if proof.release.generation != NativeMaintenanceCaptureReleaseGeneration::Native
            || proof.release.source_commit != commit
            || proof.executable.path != current
        {
            return Err(Error::Installation);
        }
        let due_ports = build_due_ports(&root, &commit)?;
        let registration = NativeMaintenanceRegistrationLifecycleHost::new(
            NativeMaintenanceRegistrationLifecycleRemote::new(
                NativeMaintenanceDatabaseLedgerReader::new(&root),
                NativeWirePrimitives,
                NativeWirePrimitives,
            ),
        );
        let schedule = NativeMaintenanceRegistrationLifecycleRemote::new(
            NativeMaintenanceDatabaseLedgerReader::new(&root),
            NativeWirePrimitives,
            NativeWirePrimitives,
        );
        Ok(Self {
            witness: None,
            epoch_remote: NativeMaintenanceDatabaseLedgerReader::new(&root),
            evidence_remote: NativeMaintenanceDatabaseLedgerReader::new(&root),
            registration,
            schedule,
            journals: None,
            archive: None,
            pack: None,
            due_ports,
        })
    }
}

impl<Ports: NativeMaintenanceControllerInstalledPorts> NativeMaintenanceDueReadPort
    for NativeMaintenanceControllerHost<Ports>
{
    type Error = NativeMaintenanceControllerHostError;

    /// Read only exact already-initialized journal due-state identities.
    fn inspect_due_keys<'a>(
        &'a mut self,
        keys: &'a [String],
    ) -> HostFuture<'a, Vec<NativeMaintenanceDueState>> {
        Box::pin(async move {
            let journal = &mut self
                .journals
                .as_mut()
                .ok_or(Self::Error::Journal)?
                .controller;
            NativeMaintenanceJournalDueReader::new(journal)
                .inspect_due_keys(keys)
                .await
                .map_err(|_| Self::Error::Journal)
        })
    }
}

impl<Ports: NativeMaintenanceControllerInstalledPorts> NativeMaintenanceControllerRunPorts
    for NativeMaintenanceControllerHost<Ports>
{
    type ProcessLock = NativeMaintenanceControllerProcessLock;

    /// Retain the original controller lock for the entire due run.
    fn acquire_process_lock(&mut self) -> Result<Self::ProcessLock, Self::Error> {
        NativeMaintenanceControllerProcessLock::acquire().map_err(|_| Self::Error::Lock)
    }

    /// Initialize independent verified handles for the same one journal under the process lock.
    fn initialize_journal(&mut self) -> HostFuture<'_, ()> {
        Box::pin(async move {
            let (controller, a) = NativeMaintenanceJournalStore::initialize(
                &mut NativeWirePrimitives,
                &NativeWirePrimitives,
            )
            .map_err(|_| Self::Error::Journal)?;
            let (archive, b) = NativeMaintenanceJournalStore::initialize(
                &mut NativeWirePrimitives,
                &NativeWirePrimitives,
            )
            .map_err(|_| Self::Error::Journal)?;
            let (cold, c) = NativeMaintenanceJournalStore::initialize(
                &mut NativeWirePrimitives,
                &NativeWirePrimitives,
            )
            .map_err(|_| Self::Error::Journal)?;
            // admit only the coherent reconciled journal for all sequential effects
            if a != b || a != c {
                return Err(Self::Error::Journal);
            }
            self.journals = Some(Journals {
                controller,
                archive,
                cold,
            });
            Ok(())
        })
    }

    /// Open the original fixed archive and private rolling custody pack.
    fn initialize_archive(&mut self) -> HostFuture<'_, ()> {
        Box::pin(async move {
            self.archive = Some(
                NativeMaintenancePlaintextArchive::initialize(
                    NativeMaintenanceControllerArchiveValidators::new(),
                )
                .map_err(|_| Self::Error::Archive)?,
            );
            self.pack = Some(NativeMaintenanceOpenPack {
                store: NativeMaintenanceFixedOpenPackStore::initialize()
                    .map_err(|_| Self::Error::Archive)?,
            });
            Ok(())
        })
    }

    /// Initialize genuine verified epoch and schedule custody before using a database clock.
    fn initialize_lifecycle(&mut self) -> HostFuture<'_, ()> {
        Box::pin(async move {
            let documents = NativeMaintenanceCaptureGenesisDocuments::default();
            let mut authority = NativeMaintenanceCaptureEpochLedgerHost {
                remote: &mut self.epoch_remote,
                documents: &documents,
            };
            let mut genesis = NativeMaintenanceCaptureGenesisHost {
                archive: self.archive.as_mut().ok_or(Self::Error::Archive)?,
                clock: &mut NativeMaintenanceArchiveWallClock,
                documents: &documents,
                journal: &mut self
                    .journals
                    .as_mut()
                    .ok_or(Self::Error::Journal)?
                    .controller,
            };
            initialize_native_maintenance_capture_lifecycle(
                &mut authority,
                &mut self.registration,
                &mut genesis,
            )
            .await
            .map_err(|_| Self::Error::Lifecycle)?;
            self.witness = Some(documents.retained_witness().ok_or(Self::Error::Lifecycle)?);
            Ok(())
        })
    }

    /// Read actual authenticated schedule and its transaction-derived cutoff.
    fn read_registration_schedule(&mut self) -> HostFuture<'_, NativeRegistrationScheduleStatus> {
        Box::pin(async move {
            let lifecycle = self
                .schedule
                .read_lifecycle()
                .await
                .map_err(|_| Self::Error::Schedule)?;
            let schedule = registration_schedule_from_lifecycle(
                &lifecycle,
                &mut NativeWirePrimitives,
                &NativeWirePrimitives,
            )
            .map_err(|_| Self::Error::Schedule)?;
            let (witness, bytes) = self.witness.take().ok_or(Self::Error::Lifecycle)?;
            // bind the semantic owner to the source's one authenticated controller snapshot
            self.due_ports
                .bind_initial_scope(schedule.clone(), lifecycle, witness, bytes)
                .map_err(|_| Self::Error::Lifecycle)?;
            Ok(schedule)
        })
    }

    /// Synchronize one real bounded custody cycle at the authenticated transaction timestamp.
    fn synchronize_evidence<'a>(
        &'a mut self,
        cutoff: &'a str,
    ) -> HostFuture<'a, NativeMaintenanceDueEvidence> {
        Box::pin(async move {
            let journals = self.journals.as_mut().ok_or(Self::Error::Journal)?;
            let mut host = NativeMaintenanceEvidenceCycleHost {
                archive: self.archive.as_mut().ok_or(Self::Error::Archive)?,
                pack: self.pack.as_mut().ok_or(Self::Error::Archive)?,
                remote: &mut self.evidence_remote,
                archive_journal: &mut journals.archive,
                cold_journal: &mut journals.cold,
            };
            synchronize_native_maintenance_evidence(&mut host, cutoff)
                .await
                .map_err(|_| Self::Error::Evidence)
        })
    }

    /// Seal a partial custody pack before exposing historical semantic inputs.
    fn flush_evidence(&mut self) -> HostFuture<'_, ()> {
        Box::pin(async move {
            let mut host = NativeMaintenanceColdArchiveHost {
                archive: self.archive.as_mut().ok_or(Self::Error::Archive)?,
                pack: self.pack.as_mut().ok_or(Self::Error::Archive)?,
                journal: &mut self.journals.as_mut().ok_or(Self::Error::Journal)?.archive,
                clock: &mut NativeMaintenanceArchiveWallClock,
            };
            host.seal(true).map_err(|_| Self::Error::Evidence)
        })
    }

    /// Obtain the actual native wall clock for one ordinary due commit.
    fn canonical_now(&mut self) -> Result<String, Self::Error> {
        NativeMaintenanceArchiveWallClock
            .now()
            .ok_or(Self::Error::Clock)
    }

    /// Execute the sequential, leased due over real controller custody and semantic ports.
    fn execute_due<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        now: &'a str,
        evidence: &'a NativeMaintenanceDueEvidence,
    ) -> HostFuture<'a, NativeMaintenanceDueExecutionResult> {
        Box::pin(async move {
            execute_native_maintenance_due(
                due,
                now,
                Some(evidence),
                &mut self
                    .journals
                    .as_mut()
                    .ok_or(Self::Error::Journal)?
                    .controller,
                self.archive.as_mut().ok_or(Self::Error::Archive)?,
                &mut NativeMaintenanceArchiveWallClock,
                &mut self.due_ports,
            )
            .await
            .map_err(|_| Self::Error::Due)
        })
    }
}

/// Resolve only the fixed release child and expected installed capture/controller executable.
fn installed_root(current: &Path) -> Option<(PathBuf, String)> {
    let root = current.parent()?.parent()?.to_path_buf();
    let commit = root.file_name()?.to_str()?.to_owned();
    if root != Path::new(RELEASES_ROOT).join(&commit) || current != root.join(EXECUTABLE) {
        return None;
    }
    Some((root, commit))
}
