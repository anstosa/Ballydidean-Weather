//! Concrete candidate fit, packaging, registration, archive and preregistration owner.

use std::path::Path;

use weather_contracts::JsonValue;

use crate::{
    NativeMaintenanceArchiveWallClock, NativeMaintenanceCandidateArchiveInput,
    NativeMaintenanceCandidateParity, NativeMaintenanceCandidateParityMaterial,
    NativeMaintenanceCandidateParityRequest, NativeMaintenanceCandidatePersistenceRequest,
    NativeMaintenanceCandidateRegistrationMaterial, NativeMaintenanceCandidateRegistrationRequest,
    NativeMaintenanceCaptureNativeInstallation, NativeMaintenanceConfirmationPreregistration,
    NativeMaintenanceControllerArchiveValidators, NativeMaintenanceFitRequest,
    NativeMaintenanceFitResult, NativeMaintenanceJournalStore, NativeMaintenancePersistedCandidate,
    NativeMaintenancePlaintextArchive, NativeMaintenancePostFitRegistration,
    NativeMaintenancePostFitRegistrationInput, NativeMaintenanceShadowRegistrationMaterial,
    NativeWirePrimitives, VerifiedNativeMaintenanceCaptureEpochWitness,
    build_native_maintenance_candidate_parity, build_native_maintenance_post_fit_registration,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_policy_comparison::{object, string},
    persist_native_maintenance_candidate, preregister_native_maintenance_confirmation,
    read_native_maintenance_confirmation_head, run_native_maintenance_installed_fit,
};

type Archive = NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>;

/// Closed candidate owner effect failures preserve the original phase boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceCandidateOwnerError {
    Installation,
    Journal,
    Archive,
    Witness,
    Fit,
    Parity,
    Registration,
    Persistence,
    Confirmation,
}

/// Actual source-derived candidate effects retained for one sequential installed controller run.
pub struct NativeMaintenanceInstalledCandidateOwner {
    source_commit: String,
    witness: Option<(VerifiedNativeMaintenanceCaptureEpochWitness, Vec<u8>)>,
    journal: Option<NativeMaintenanceJournalStore>,
    archive: Option<Archive>,
    parity: Option<NativeMaintenanceCandidateParityMaterial>,
    registration: Option<NativeMaintenancePostFitRegistration>,
    preregistration: Option<NativeMaintenanceConfirmationPreregistration>,
    persisted_graph_sha256: Option<String>,
}

impl NativeMaintenanceInstalledCandidateOwner {
    /// Bind only the exact current installed release; defer all local writes until the controller lock.
    pub fn bind_installed(
        source_commit: &str,
        root: &Path,
    ) -> Result<Self, NativeMaintenanceCandidateOwnerError> {
        use NativeMaintenanceCandidateOwnerError as Error;
        let expected = Path::new("/home/ubuntu/.weather/adjustment-maintenance/runner/releases")
            .join(source_commit);
        if root != expected {
            return Err(Error::Installation);
        }
        let verified = NativeMaintenanceCaptureNativeInstallation::native_release(source_commit)
            .and_then(|owner| owner.verify())
            .map_err(|_| Error::Installation)?;
        if verified.release.source_commit != source_commit {
            return Err(Error::Installation);
        }
        Ok(Self {
            source_commit: source_commit.to_owned(),
            witness: None,
            journal: None,
            archive: None,
            parity: None,
            registration: None,
            preregistration: None,
            persisted_graph_sha256: None,
        })
    }

    /// Open actual candidate custody using the exact already authenticated controller epoch.
    pub(crate) fn initialize_locked(
        &mut self,
        witness: VerifiedNativeMaintenanceCaptureEpochWitness,
        bytes: Vec<u8>,
        expected_witness_sha256: &str,
    ) -> Result<(), NativeMaintenanceCandidateOwnerError> {
        use NativeMaintenanceCandidateOwnerError as Error;
        // retain the verified controller witness without repeating the root transport
        if witness.witness_sha256 != expected_witness_sha256 || !valid_hash(expected_witness_sha256)
        {
            return Err(Error::Witness);
        }
        let (journal, _) = NativeMaintenanceJournalStore::initialize(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
        )
        .map_err(|_| Error::Journal)?;
        let archive = NativeMaintenancePlaintextArchive::initialize(
            NativeMaintenanceControllerArchiveValidators::new(),
        )
        .map_err(|_| Error::Archive)?;
        self.witness = Some((witness, bytes));
        self.journal = Some(journal);
        self.archive = Some(archive);
        self.parity = None;
        self.registration = None;
        self.preregistration = None;
        self.persisted_graph_sha256 = None;
        Ok(())
    }

    /// Retain the current verified source epoch for monthly and daily semantic selection.
    pub fn witness(&self) -> Option<&VerifiedNativeMaintenanceCaptureEpochWitness> {
        self.witness.as_ref().map(|(witness, _)| witness)
    }

    /// Borrow the actual canonical epoch member bytes for publication and daily semantic material.
    pub fn witness_bytes(&self) -> Option<&[u8]> {
        self.witness.as_ref().map(|(_, bytes)| bytes.as_slice())
    }

    /// Clear only uncommitted per-attempt assembly state between sequential due operations.
    pub fn begin_due(&mut self) {
        self.parity = None;
        self.registration = None;
        self.preregistration = None;
        self.persisted_graph_sha256 = None;
    }

    /// Execute one independently verified native-generation fit in its fixed sandbox.
    pub fn run_fit(
        &mut self,
        request: NativeMaintenanceFitRequest<'_>,
    ) -> Result<NativeMaintenanceFitResult, NativeMaintenanceCandidateOwnerError> {
        run_native_maintenance_installed_fit(&self.source_commit, request)
            .map_err(|_| NativeMaintenanceCandidateOwnerError::Fit)
    }

    /// Produce the independent raw/package parity receipt and retain the typed portable bytes.
    pub fn build_candidate_parity(
        &mut self,
        request: NativeMaintenanceCandidateParityRequest<'_>,
    ) -> Result<Option<NativeMaintenanceCandidateParity>, NativeMaintenanceCandidateOwnerError>
    {
        let material = build_native_maintenance_candidate_parity(request)
            .map_err(|_| NativeMaintenanceCandidateOwnerError::Parity)?;
        let projected = material
            .as_ref()
            .map(NativeMaintenanceCandidateParityMaterial::due_parity);
        self.parity = material;
        Ok(projected)
    }

    /// Report that the actual journal and archive owners are initialized for candidate persistence.
    pub fn candidate_persistence_available(&self) -> bool {
        self.journal.is_some() && self.archive.is_some() && self.witness.is_some()
    }

    /// Bind fitted artifact parity to the source-exact rolling post-fit registration.
    pub fn build_candidate_registration(
        &mut self,
        request: NativeMaintenanceCandidateRegistrationRequest<'_>,
    ) -> Result<
        Option<NativeMaintenanceCandidateRegistrationMaterial>,
        NativeMaintenanceCandidateOwnerError,
    > {
        use NativeMaintenanceCandidateOwnerError as Error;
        let witness = self
            .witness
            .as_ref()
            .map(|(w, _)| w)
            .ok_or(Error::Witness)?;
        let parity = self.parity.as_ref().ok_or(Error::Parity)?;
        if request.parity != Some(&parity.due_parity())
            || request.fit.candidate_sha256 != parity.portable.candidate_sha256
        {
            return Err(Error::Parity);
        }
        let family = request.due.family.ok_or(Error::Registration)?;
        let context = &request
            .inspection
            .registration_context
            .as_ref()
            .ok_or(Error::Registration)?
            .value;
        if !exact_keys(
            context,
            &[
                "cutoffAt",
                "historicalMemberRootSha256",
                "inputManifestSha256",
                "predecessorRegistrationSha256",
                "predecessorTerminalAt",
            ],
        ) {
            return Err(Error::Registration);
        }
        let cutoff = text(context, "cutoffAt").ok_or(Error::Registration)?;
        let historical_root =
            text(context, "historicalMemberRootSha256").ok_or(Error::Registration)?;
        let input_manifest = text(context, "inputManifestSha256").ok_or(Error::Registration)?;
        let predecessor =
            nullable_text(context, "predecessorRegistrationSha256").ok_or(Error::Registration)?;
        let predecessor_terminal =
            nullable_text(context, "predecessorTerminalAt").ok_or(Error::Registration)?;
        if input_manifest != request.inspection.input_manifest_sha256 {
            return Err(Error::Registration);
        }
        let registration = build_native_maintenance_post_fit_registration(
            NativeMaintenancePostFitRegistrationInput {
                artifact_sha256: &parity.portable.artifact_sha256,
                candidate_sha256: &request.fit.candidate_sha256,
                cutoff_at: &cutoff,
                due_month: request
                    .due
                    .due_key
                    .rsplit('/')
                    .next()
                    .ok_or(Error::Registration)?,
                witness,
                family,
                historical_member_root_sha256: &historical_root,
                input_manifest_sha256: &input_manifest,
                predecessor_registration_sha256: predecessor.as_deref(),
                predecessor_terminal_at: predecessor_terminal.as_deref(),
                requested_at: request.requested_at,
                source_identity_sha256: &parity.portable.source_identity_sha256,
            },
        )
        .ok_or(Error::Registration)?;
        let material = registration_material(&registration, &parity.portable.artifact_sha256);
        self.registration = Some(registration);
        Ok(Some(material))
    }

    /// Persist the genuine candidate, package, parity and registration graph before local preregistration.
    pub fn persist_candidate(
        &mut self,
        request: NativeMaintenanceCandidatePersistenceRequest<'_>,
    ) -> Result<NativeMaintenancePersistedCandidate, NativeMaintenanceCandidateOwnerError> {
        use NativeMaintenanceCandidateOwnerError as Error;
        let (witness, witness_bytes) = self.witness.as_ref().ok_or(Error::Witness)?;
        let parity = self.parity.as_ref().ok_or(Error::Parity)?;
        let registration = self.registration.as_ref().ok_or(Error::Registration)?;
        if request.parity != Some(&parity.due_parity())
            || request.registration_material
                != Some(&registration_material(
                    registration,
                    &parity.portable.artifact_sha256,
                ))
        {
            return Err(Error::Registration);
        }
        let persisted = persist_native_maintenance_candidate(
            NativeMaintenanceCandidateArchiveInput {
                family: request.due.family.ok_or(Error::Registration)?,
                fit: request.fit,
                parity,
                registration,
                witness,
                witness_bytes,
            },
            self.archive.as_mut().ok_or(Error::Archive)?,
            self.journal.as_mut().ok_or(Error::Journal)?,
            &mut NativeMaintenanceArchiveWallClock,
        )
        .ok_or(Error::Persistence)?;
        self.persisted_graph_sha256 = Some(persisted.candidate_graph_sha256.clone());
        Ok(persisted)
    }

    /// Read the same durable local journal head immediately before confirmation registration.
    pub fn read_confirmation_head(
        &mut self,
    ) -> Result<String, NativeMaintenanceCandidateOwnerError> {
        read_native_maintenance_confirmation_head(
            self.journal
                .as_mut()
                .ok_or(NativeMaintenanceCandidateOwnerError::Journal)?,
        )
        .map_err(|_| NativeMaintenanceCandidateOwnerError::Confirmation)
    }

    /// Occupy the original local family slot under its verified journal before release publication.
    pub fn preregister_confirmation(
        &mut self,
        confirmation: &JsonValue,
        input_head_sha256: &str,
        now: &str,
    ) -> Result<String, NativeMaintenanceCandidateOwnerError> {
        let registration = self
            .registration
            .as_ref()
            .ok_or(NativeMaintenanceCandidateOwnerError::Registration)?;
        if confirmation != &registration.confirmation {
            return Err(NativeMaintenanceCandidateOwnerError::Registration);
        }
        let receipt = preregister_native_maintenance_confirmation(
            self.journal
                .as_mut()
                .ok_or(NativeMaintenanceCandidateOwnerError::Journal)?,
            confirmation,
            input_head_sha256,
            now,
        )
        .map_err(|_| NativeMaintenanceCandidateOwnerError::Confirmation)?;
        let identity = receipt.registration_sha256.clone();
        self.preregistration = Some(receipt);
        Ok(identity)
    }

    /// Borrow the exact persisted candidate graph and acknowledged confirmation preregistration.
    pub fn release_custody(&self) -> Option<(&str, &NativeMaintenanceConfirmationPreregistration)> {
        Some((
            self.persisted_graph_sha256.as_deref()?,
            self.preregistration.as_ref()?,
        ))
    }

    /// Borrow complete typed materials for the subsequent fenced inactive release.
    pub fn release_material(
        &self,
    ) -> Option<(
        &NativeMaintenanceCandidateParityMaterial,
        &NativeMaintenancePostFitRegistration,
    )> {
        Some((self.parity.as_ref()?, self.registration.as_ref()?))
    }
}

/// Project the exact source descriptor tree into the retained due-work ownership value.
fn registration_material(
    registration: &NativeMaintenancePostFitRegistration,
    artifact_sha256: &str,
) -> NativeMaintenanceCandidateRegistrationMaterial {
    let value = object(vec![
        (
            "cohort",
            object(vec![
                ("descriptor", registration.cohort.descriptor.clone()),
                ("cohortSha256", string(&registration.cohort.cohort_sha256)),
            ]),
        ),
        ("confirmation", registration.confirmation.clone()),
        (
            "expectedPlan",
            object(vec![
                ("descriptor", registration.expected_plan.descriptor.clone()),
                (
                    "expectedPlanSha256",
                    string(&registration.expected_plan.expected_plan_sha256),
                ),
                (
                    "reservedKeySha256",
                    string(&registration.expected_plan.reserved_key_sha256),
                ),
            ]),
        ),
        (
            "policy",
            object(vec![
                ("descriptor", registration.policy.descriptor.clone()),
                ("policySha256", string(&registration.policy.policy_sha256)),
            ]),
        ),
        ("schedulePlan", registration.shadow.plan.clone()),
        (
            "shadowRegistration",
            registration.shadow.registration.clone(),
        ),
        (
            "source",
            object(vec![
                ("descriptor", registration.source_descriptor.clone()),
                ("sourceSha256", string(&registration.source_sha256)),
            ]),
        ),
    ]);
    NativeMaintenanceCandidateRegistrationMaterial {
        confirmation: registration.confirmation.clone(),
        shadow_registration: Some(NativeMaintenanceShadowRegistrationMaterial {
            artifact_sha256: artifact_sha256.to_owned(),
            registration_sha256: registration.shadow.registration_sha256.clone(),
        }),
        value,
    }
}

/// Decode one source nullable text member without conflating absence with null.
fn nullable_text(value: &JsonValue, key: &str) -> Option<Option<String>> {
    match field(value, key)? {
        JsonValue::Null => Some(None),
        _ => Some(Some(text(value, key)?)),
    }
}
