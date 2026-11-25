//! Installed semantic owner retaining the authenticated controller epoch and schedule.

use std::path::Path;

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::{
    NativeMaintenanceBurnedTerminalAccess, NativeMaintenanceCandidateOwnerError,
    NativeMaintenanceCaptureFamily, NativeMaintenanceControllerArchiveValidators,
    NativeMaintenanceDailyEvaluation, NativeMaintenanceDailyEvaluationRequest,
    NativeMaintenanceDailyTerminalValues, NativeMaintenanceDatabaseLedgerReader,
    NativeMaintenanceDue, NativeMaintenanceDueEvidence, NativeMaintenanceDueMode,
    NativeMaintenanceInstalledCandidateOwner, NativeMaintenanceInstalledTerminalMaterial,
    NativeMaintenanceInstalledTerminalMaterialError, NativeMaintenanceJournalStore,
    NativeMaintenancePlaintextArchive, NativeMaintenancePlaintextObjectStore,
    NativeMaintenanceRegistrationLifecycleRemote, NativeMaintenanceSemanticCatalog,
    NativeMaintenanceSemanticCatalogOutcome, NativeMaintenanceSemanticInspection,
    NativeMaintenanceTerminalAccessError, NativeMaintenanceTerminalAccessInput,
    NativeMaintenanceTerminalAccessOutcome, NativeRegistrationLifecycleStatus,
    NativeRegistrationScheduleStatus, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness, evaluate_native_pending_maintenance_daily,
    finalize_native_maintenance_installed_terminal, inspect_native_maintenance_monthly_semantics,
    maintenance_canonical_json::{field, text},
    native_maintenance_policy_comparison::{object, string},
    prepare_native_maintenance_terminal_access, read_native_maintenance_semantic_catalog,
    verify_native_maintenance_due,
};

/// Closed installed semantic effect errors retain the source owning phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceInstalledSemanticError {
    Candidate(NativeMaintenanceCandidateOwnerError),
    Journal,
    Archive,
    Lifecycle,
    Catalog,
    Clock,
    Input,
    Monthly,
    TerminalAccess(NativeMaintenanceTerminalAccessError),
    TerminalMaterial(NativeMaintenanceInstalledTerminalMaterialError),
}

/// Monthly/idle/pending inspection, or genuine daily terminal catalog requiring its burn owner.
pub enum NativeMaintenanceInstalledSemanticInspection {
    Ready(Box<NativeMaintenanceSemanticInspection>),
    DailyTerminal {
        catalog: Box<NativeMaintenanceSemanticCatalog>,
        access: Box<NativeMaintenanceBurnedTerminalAccess>,
    },
}

/// Concrete semantic selection and candidate owner for one installed controller invocation.
pub struct NativeMaintenanceInstalledSemanticOwner {
    source_commit: String,
    pub candidate: NativeMaintenanceInstalledCandidateOwner,
    lifecycle_remote: NativeMaintenanceRegistrationLifecycleRemote<
        NativeMaintenanceDatabaseLedgerReader,
        NativeWirePrimitives,
        NativeWirePrimitives,
    >,
    schedule: Option<NativeRegistrationScheduleStatus>,
    lifecycle: Option<NativeRegistrationLifecycleStatus>,
    catalog_journal: Option<NativeMaintenanceJournalStore>,
    catalog_archive: Option<NativeMaintenancePlaintextObjectStore>,
    terminal_archive:
        Option<NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>>,
    owner_remote: NativeMaintenanceDatabaseLedgerReader,
}

impl NativeMaintenanceInstalledSemanticOwner {
    /// Construct the verified current installed owner without creating local state before the lock.
    pub fn bind_installed(
        root: &Path,
        source_commit: &str,
    ) -> Result<Self, NativeMaintenanceInstalledSemanticError> {
        let candidate =
            NativeMaintenanceInstalledCandidateOwner::bind_installed(source_commit, root)
                .map_err(NativeMaintenanceInstalledSemanticError::Candidate)?;
        Ok(Self {
            source_commit: source_commit.to_owned(),
            candidate,
            lifecycle_remote: NativeMaintenanceRegistrationLifecycleRemote::new(
                NativeMaintenanceDatabaseLedgerReader::new(root),
                NativeWirePrimitives,
                NativeWirePrimitives,
            ),
            schedule: None,
            lifecycle: None,
            catalog_journal: None,
            catalog_archive: None,
            terminal_archive: None,
            owner_remote: NativeMaintenanceDatabaseLedgerReader::new(root),
        })
    }

    /// Retain the controller's single authenticated initial registration snapshot under its process lock.
    pub fn initialize_locked(
        &mut self,
        schedule: NativeRegistrationScheduleStatus,
        lifecycle: NativeRegistrationLifecycleStatus,
        witness: VerifiedNativeMaintenanceCaptureEpochWitness,
        witness_bytes: Vec<u8>,
    ) -> Result<(), NativeMaintenanceInstalledSemanticError> {
        use NativeMaintenanceInstalledSemanticError as Error;
        // retain exactly the same transaction snapshot for monthly inputs and controller cutoff
        if schedule.epoch_witness_sha256 != lifecycle.epoch_witness_sha256
            || schedule.snapshot_at != lifecycle.snapshot_at
        {
            return Err(Error::Lifecycle);
        }
        self.candidate
            .initialize_locked(witness, witness_bytes, &schedule.epoch_witness_sha256)
            .map_err(Error::Candidate)?;
        let (journal, _) = NativeMaintenanceJournalStore::initialize(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
        )
        .map_err(|_| Error::Journal)?;
        let archive =
            NativeMaintenancePlaintextObjectStore::initialize().map_err(|_| Error::Archive)?;
        self.schedule = Some(schedule);
        self.lifecycle = Some(lifecycle);
        self.catalog_journal = Some(journal);
        self.catalog_archive = Some(archive);
        self.terminal_archive = Some(
            NativeMaintenancePlaintextArchive::initialize(
                NativeMaintenanceControllerArchiveValidators::new(),
            )
            .map_err(|_| Error::Archive)?,
        );
        Ok(())
    }

    /// Return the authenticated original controller clock, not a later lifecycle or wall-clock read.
    pub fn controller_clock(&self) -> Option<&str> {
        self.schedule
            .as_ref()
            .map(|schedule| schedule.snapshot_at.as_str())
    }

    /// Borrow the authenticated newest lifecycle after the original daily refresh when applicable.
    pub fn lifecycle(&self) -> Option<&NativeRegistrationLifecycleStatus> {
        self.lifecycle.as_ref()
    }

    /// Select and restore real history, and assemble the genuine monthly or unopened daily semantic request.
    pub async fn inspect(
        &mut self,
        due: &NativeMaintenanceDue,
        evidence: Option<&NativeMaintenanceDueEvidence>,
    ) -> Result<NativeMaintenanceInstalledSemanticInspection, NativeMaintenanceInstalledSemanticError>
    {
        use NativeMaintenanceInstalledSemanticError as Error;
        use NativeMaintenanceInstalledSemanticInspection as Outcome;
        if !verify_native_maintenance_due(due) {
            return Err(Error::Input);
        }
        self.candidate.begin_due();
        let schedule = self.schedule.as_ref().ok_or(Error::Lifecycle)?;
        let witness = self.candidate.witness().ok_or(Error::Lifecycle)?;
        // daily members refresh owner lifecycle after each earlier terminal, while monthly retains initial view
        if due.mode == NativeMaintenanceDueMode::Daily
            && let Some(crate::NativeMaintenanceEvidenceSynchronization::Cold { cycle }) = evidence
            && cycle.start.is_some()
            && cycle.state != crate::NativeRevisionColdDrainState::ArchivedCursorAhead
        {
            let lifecycle = self
                .lifecycle_remote
                .read_lifecycle()
                .await
                .map_err(|_| Error::Lifecycle)?;
            if lifecycle.epoch_witness_sha256 != witness.witness_sha256 {
                return Err(Error::Lifecycle);
            }
            self.lifecycle = Some(lifecycle);
        }
        let lifecycle = self.lifecycle.as_ref().ok_or(Error::Lifecycle)?;
        let restored = read_native_maintenance_semantic_catalog(
            due,
            evidence,
            witness,
            lifecycle,
            self.catalog_journal.as_mut().ok_or(Error::Journal)?,
            self.catalog_archive.as_mut().ok_or(Error::Archive)?,
        )
        .map_err(|_| Error::Catalog)?;
        let catalog = match restored {
            NativeMaintenanceSemanticCatalogOutcome::Blocked(reason) => {
                return Ok(Outcome::Ready(Box::new(
                    NativeMaintenanceSemanticInspection::Blocked { reason },
                )));
            }
            NativeMaintenanceSemanticCatalogOutcome::Ready(catalog) => catalog,
        };
        // pre-month control references consume their own genuine rain historical population
        if due.due_key.starts_with("control-reference/rain/") {
            let witness = witness.clone();
            let mut owner =
                crate::NativeMaintenanceInstalledControlReferenceSemanticOwner::initialize(
                    &self.source_commit,
                    &mut self.candidate,
                    self.catalog_journal.as_mut().ok_or(Error::Journal)?,
                )
                .map_err(|_| Error::Monthly)?;
            let result = owner
                .build(due, &witness, &catalog.catalog, &schedule.snapshot_at)
                .map_err(|_| Error::Monthly)?;
            let inspection = match result {
                crate::NativeMaintenanceControlReferenceSemanticOutcome::Ready {
                    input_manifest_sha256,
                    material,
                } => NativeMaintenanceSemanticInspection::ControlReference {
                    input_manifest_sha256,
                    material: material.input,
                },
                crate::NativeMaintenanceControlReferenceSemanticOutcome::NoCandidate { .. } => {
                    NativeMaintenanceSemanticInspection::Blocked {
                        reason: crate::NativeMaintenanceDueWorkReason::SemanticInputBlocked,
                    }
                }
            };
            return Ok(Outcome::Ready(Box::new(inspection)));
        }
        if due.mode == NativeMaintenanceDueMode::Monthly {
            let inspection = inspect_native_maintenance_monthly_semantics(
                &self.source_commit,
                due,
                witness,
                &catalog.catalog,
                schedule,
                lifecycle,
            )
            .map_err(|_| Error::Monthly)?;
            return Ok(Outcome::Ready(Box::new(inspection)));
        }
        let Some(entry) = catalog.active_entry.as_ref() else {
            return Ok(Outcome::Ready(Box::new(
                NativeMaintenanceSemanticInspection::Idle {
                    input_manifest_sha256: catalog.catalog.input_manifest_sha256.clone(),
                },
            )));
        };
        let current = crate::NativeMaintenanceCaptureRemoteAuthority::new(&mut self.owner_remote)
            .read_verified_family_current_full(entry.slot.family)
            .map_err(|_| Error::Lifecycle)?;
        let witness_bytes = self.candidate.witness_bytes().ok_or(Error::Lifecycle)?;
        // the original current family authority supplies the deployed source commit for the daily burn
        let access = prepare_native_maintenance_terminal_access(
            NativeMaintenanceTerminalAccessInput {
                clock_at: &schedule.snapshot_at,
                due,
                catalog: &catalog.catalog,
                witness,
                witness_bytes,
                entry,
                source_commit: &current.commit,
            },
            self.catalog_journal.as_mut().ok_or(Error::Journal)?,
            self.terminal_archive.as_mut().ok_or(Error::Archive)?,
            &mut self.owner_remote,
        )
        .map_err(Error::TerminalAccess)?;
        match access {
            NativeMaintenanceTerminalAccessOutcome::Pending(value) => Ok(Outcome::Ready(Box::new(
                NativeMaintenanceSemanticInspection::DailyReady {
                    input: NativeMaintenanceDailyEvaluationRequest { value },
                    input_manifest_sha256: catalog.catalog.input_manifest_sha256.clone(),
                    terminal_material: None,
                },
            ))),
            NativeMaintenanceTerminalAccessOutcome::Burned(access) => {
                Ok(Outcome::DailyTerminal { catalog, access })
            }
        }
    }

    /// Append the actual post-burn parts and restore the acknowledged installed candidate.
    pub fn finish_daily_terminal(
        &mut self,
        catalog: &NativeMaintenanceSemanticCatalog,
        access: NativeMaintenanceBurnedTerminalAccess,
        assembly: NativeMaintenanceDailyTerminalValues,
    ) -> Result<NativeMaintenanceInstalledTerminalMaterial, NativeMaintenanceInstalledSemanticError>
    {
        use NativeMaintenanceInstalledSemanticError as Error;
        let witness = self.candidate.witness_bytes().ok_or(Error::Lifecycle)?;
        let witness = NativeWirePrimitives
            .decode_json(witness)
            .ok_or(Error::Input)?;
        let clock = &self.schedule.as_ref().ok_or(Error::Lifecycle)?.snapshot_at;
        finalize_native_maintenance_installed_terminal(
            access,
            assembly,
            &catalog.catalog.input_manifest_sha256,
            clock,
            witness,
            self.catalog_journal.as_mut().ok_or(Error::Journal)?,
            self.catalog_archive.as_mut().ok_or(Error::Archive)?,
        )
        .map_err(Error::TerminalMaterial)
    }

    /// Expose the verified fixed owner transport for source-bound report and terminal actions.
    pub fn owner_remote(&mut self) -> &mut NativeMaintenanceDatabaseLedgerReader {
        &mut self.owner_remote
    }

    /// Evaluate the source pending receipt from the exact unopened request and retained authenticated epoch.
    pub fn evaluate_pending(
        &self,
        input: &NativeMaintenanceDailyEvaluationRequest,
        due: &NativeMaintenanceDue,
    ) -> Option<NativeMaintenanceDailyEvaluation> {
        let witness = self.candidate.witness()?;
        let lifecycle = field(&input.value, "lifecycle")?;
        if text(lifecycle, "state")?.as_str() != "pending"
            || field(&input.value, "due")? != &daily_due_value(due)
        {
            return None;
        }
        let family = match text(lifecycle, "family")?.as_str() {
            "temperature" => NativeMaintenanceCaptureFamily::Temperature,
            "wind" => NativeMaintenanceCaptureFamily::Wind,
            "rain" => NativeMaintenanceCaptureFamily::Rain,
            _ => return None,
        };
        let registration = match field(lifecycle, "registrationSha256")? {
            JsonValue::Null => None,
            _ => Some(text(lifecycle, "registrationSha256")?),
        };
        let clock = text(&input.value, "clockAt")?;
        if self.controller_clock()? != clock {
            return None;
        }
        evaluate_native_pending_maintenance_daily(
            &text(&input.value, "catalogInputManifestSha256")?,
            &clock,
            due,
            witness,
            family,
            registration.as_deref(),
        )
    }
}

/// Preserve the exact original daily due wire coordinates without any monthly alias.
fn daily_due_value(due: &NativeMaintenanceDue) -> JsonValue {
    object(vec![
        ("dueKey", string(&due.due_key)),
        ("family", JsonValue::Null),
        ("mode", string("daily")),
        ("originalCutoffAt", string(&due.original_cutoff_at)),
        ("scope", string(&due.scope)),
    ])
}
