//! Actual installed controller action composition over the native release, archive and root owners.

use std::{future::Future, path::Path, pin::Pin};

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeFamilyReleaseCurrent, NativeMaintenanceArchiveSegmentClock,
    NativeMaintenanceArchiveWallClock, NativeMaintenanceCaptureFamily,
    NativeMaintenanceCaptureRemoteAuthority, NativeMaintenanceControlReferenceInput,
    NativeMaintenanceControlReferenceMaterial, NativeMaintenanceControllerArchiveValidators,
    NativeMaintenanceDailyDisposition, NativeMaintenanceDailyEvaluation,
    NativeMaintenanceDailyQualifiedCompletion, NativeMaintenanceDailyTerminalValues,
    NativeMaintenanceDatabaseLedgerReader, NativeMaintenanceDue,
    NativeMaintenanceDueAttemptJournal, NativeMaintenanceDueRelease,
    NativeMaintenanceDueReleaseContext, NativeMaintenanceInstalledActionOwner,
    NativeMaintenanceInstalledCandidateOwner, NativeMaintenanceInstalledControlMaterialInput,
    NativeMaintenanceInstalledControlReleasePorts,
    NativeMaintenanceInstalledDevelopmentReleasePorts,
    NativeMaintenanceInstalledOwnerTerminalInput, NativeMaintenanceInstalledTerminalMaterial,
    NativeMaintenanceJournalStore, NativeMaintenanceOwnerTerminalClass,
    NativeMaintenancePlaintextArchive, NativeMaintenancePlaintextObjectStore,
    NativeMaintenanceTerminalAuthorityInput, NativeMaintenanceTerminalRecordInput,
    NativeMaintenanceTerminalResultDisposition, NativeMaintenanceTerminalResultInput,
    NativeMaintenanceVerifiedFullGraph, NativeOutboundHttp, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness,
    build_native_maintenance_terminal_archive_evidence,
    build_native_maintenance_terminal_no_action_identity,
    build_native_maintenance_terminal_no_action_receipt,
    build_native_maintenance_terminal_record_request,
    complete_native_maintenance_installed_owner_terminal,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_installed_control_release::AdmittedControlMaterial,
    native_maintenance_retirement_files_v2::format_digest,
    publish_native_maintenance_terminal_archive_evidence,
    read_native_maintenance_confirmation_head, read_native_maintenance_owner_terminal_tombstone,
    read_native_maintenance_terminal_authority,
    reconcile_native_maintenance_due_terminal_retirement,
    record_native_maintenance_confirmation_result, record_native_maintenance_terminal_remote,
    stage_native_maintenance_no_action_terminal_authority,
};

type Archive = NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>;
type ActionFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, NativeMaintenanceInstalledActionError>> + 'a>>;

/// Closed original owner failure phases, without publishing private values or credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceInstalledActionError {
    Installation,
    State,
    Clock,
    Witness,
    Journal,
    Archive,
    Remote,
    Material,
    Terminal,
    Control,
    Development,
    Qualified,
}

use NativeMaintenanceInstalledActionError as Error;

/// Owned resources initialized only after the installed process has taken its original lock.
struct ActionResources {
    journal: NativeMaintenanceJournalStore,
    objects: NativeMaintenancePlaintextObjectStore,
    archive: Archive,
    remote: NativeMaintenanceDatabaseLedgerReader,
    http: NativeOutboundHttp,
    witness: VerifiedNativeMaintenanceCaptureEpochWitness,
}

/// Sequential installed action owner; no caller-selected command, release path or fake effect.
pub struct NativeMaintenanceInstalledActions {
    remote: Option<NativeMaintenanceDatabaseLedgerReader>,
    resources: Option<ActionResources>,
    control: Option<AdmittedControlMaterial>,
}

impl NativeMaintenanceInstalledActions {
    /// Bind the immutable source installation before any archive or remote effects.
    pub fn bind_installed(root: &Path, commit: &str) -> Result<Self, Error> {
        // the enclosing installed host already verifies the actual executing inode and full manifest
        if root
            != Path::new("/home/ubuntu/.weather/adjustment-maintenance/runner/releases")
                .join(commit)
            || commit.len() != 40
        {
            return Err(Error::Installation);
        }
        Ok(Self {
            remote: Some(NativeMaintenanceDatabaseLedgerReader::new(root)),
            resources: None,
            control: None,
        })
    }

    /// Borrow the real initialized resources only under the active controller run.
    fn resources(&mut self) -> Result<&mut ActionResources, Error> {
        self.resources.as_mut().ok_or(Error::State)
    }
}

impl NativeMaintenanceInstalledActionOwner for NativeMaintenanceInstalledActions {
    type Error = Error;

    /// Initialize the same fixed native owners under the actual original process lock.
    fn initialize_locked(
        &mut self,
        witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    ) -> Result<(), Error> {
        let (journal, _) = NativeMaintenanceJournalStore::initialize(
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
        )
        .map_err(|_| Error::Journal)?;
        let objects =
            NativeMaintenancePlaintextObjectStore::initialize().map_err(|_| Error::Archive)?;
        let archive = Archive::initialize(NativeMaintenanceControllerArchiveValidators::new())
            .map_err(|_| Error::Archive)?;
        let http = NativeOutboundHttp::new().map_err(|_| Error::Remote)?;
        let remote = self.remote.take().ok_or(Error::State)?;
        self.resources = Some(ActionResources {
            journal,
            objects,
            archive,
            remote,
            http,
            witness: witness.clone(),
        });
        Ok(())
    }

    /// Decode and retain the actual fixed control material after independent semantic archive publication.
    fn validate_control_reference(
        &mut self,
        due: &NativeMaintenanceDue,
        now: &str,
        input: NativeMaintenanceControlReferenceInput,
    ) -> Result<NativeMaintenanceControlReferenceMaterial, Error> {
        let wire = NativeWirePrimitives;
        // this closed internal projection transports actual bytes, not Node Buffer-shaped JSON
        if !exact_keys(
            &input.value,
            &[
                "controlStateBytesBase64",
                "ordinalArtifactBytesBase64",
                "graphManifestSha256",
                "sourceMemberRootSha256",
                "sourceReceiptRootSha256",
            ],
        ) {
            return Err(Error::Control);
        }
        let state = wire
            .decode_standard_base64(&required(&input.value, "controlStateBytesBase64")?)
            .ok_or(Error::Control)?;
        let artifact = wire
            .decode_standard_base64(&required(&input.value, "ordinalArtifactBytesBase64")?)
            .ok_or(Error::Control)?;
        let graph = required(&input.value, "graphManifestSha256")?;
        let members = required(&input.value, "sourceMemberRootSha256")?;
        let receipts = required(&input.value, "sourceReceiptRootSha256")?;
        let resources = self.resources()?;
        let mut ports = NativeMaintenanceInstalledControlReleasePorts::new(
            &mut resources.journal,
            &mut resources.archive,
            &mut resources.remote,
            &resources.http,
            &resources.witness,
        );
        let result = ports
            .admit_control_reference(
                due,
                now,
                input,
                NativeMaintenanceInstalledControlMaterialInput {
                    control_state_bytes: &state,
                    ordinal_artifact_bytes: &artifact,
                    graph_manifest_sha256: &graph,
                    source_member_root_sha256: &members,
                    source_receipt_root_sha256: &receipts,
                },
            )
            .map_err(|_| Error::Control)?;
        self.control = ports.take_material();
        Ok(result)
    }

    /// Complete the genuine unsupported terminal through its absolute proof and owner retirement.
    fn complete_unsupported<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        semantic_input: &'a str,
        material: &'a NativeMaintenanceInstalledTerminalMaterial,
    ) -> ActionFuture<'a, ()> {
        Box::pin(async move {
            self.resources()?
                .complete_unsupported(due, semantic_input, material)
        })
    }

    /// Execute an actual qualified publication and retain its reconciled serving disposition.
    fn complete_qualified<'a>(
        &'a mut self,
        due: &'a NativeMaintenanceDue,
        evaluation: &'a NativeMaintenanceDailyEvaluation,
        material: &'a NativeMaintenanceInstalledTerminalMaterial,
    ) -> ActionFuture<'a, NativeMaintenanceDailyQualifiedCompletion> {
        Box::pin(async move {
            self.resources()?
                .complete_qualified(due, evaluation, material)
        })
    }

    /// Finish a real evaluated rejection or support-failed terminal without action publication.
    fn complete_no_action<'a>(
        &'a mut self,
        disposition: NativeMaintenanceDailyDisposition,
        due: &'a NativeMaintenanceDue,
        evaluation: &'a NativeMaintenanceDailyEvaluation,
        material: &'a NativeMaintenanceInstalledTerminalMaterial,
    ) -> ActionFuture<'a, ()> {
        Box::pin(async move {
            self.resources()?
                .complete_no_action(disposition, due, evaluation, material)
        })
    }

    /// Replay only the exact already retained retirement command before attempt publication.
    fn reconcile_terminal<'a>(&'a mut self, due_key: &'a str) -> ActionFuture<'a, ()> {
        Box::pin(async move {
            let owners = self.resources()?;
            reconcile_native_maintenance_due_terminal_retirement(
                &mut NativeMaintenanceDueAttemptJournal::new(&mut owners.journal),
                &mut owners.remote,
                due_key,
                &now()?,
            )
            .map_err(|_| Error::Terminal)?;
            Ok(())
        })
    }

    /// Publish the exact pre-month control bytes retained before the report existed.
    fn publish_control_reference<'a>(
        &'a mut self,
        context: NativeMaintenanceDueReleaseContext<'a>,
    ) -> ActionFuture<'a, NativeMaintenanceDueRelease> {
        Box::pin(async move {
            let control = self.control.take().ok_or(Error::Control)?;
            let owners = self.resources()?;
            let mut ports = NativeMaintenanceInstalledControlReleasePorts::new(
                &mut owners.journal,
                &mut owners.archive,
                &mut owners.remote,
                &owners.http,
                &owners.witness,
            );
            ports.restore_material(control);
            ports
                .publish_control_reference(&context)
                .map_err(|_| Error::Control)
        })
    }

    /// Publish the durable inactive candidate through its actual paired compensation and custody owner.
    fn publish_development_candidate<'a>(
        &'a mut self,
        context: NativeMaintenanceDueReleaseContext<'a>,
        candidate: &'a NativeMaintenanceInstalledCandidateOwner,
    ) -> ActionFuture<'a, NativeMaintenanceDueRelease> {
        Box::pin(async move {
            let owners = self.resources()?;
            NativeMaintenanceInstalledDevelopmentReleasePorts {
                journal: &mut owners.journal,
                objects: &mut owners.objects,
                archive: &mut owners.archive,
                remote: &mut owners.remote,
                http: &owners.http,
            }
            .publish_development_candidate(&context, candidate)
            .map_err(|_| Error::Development)
        })
    }
}

/// Exact evaluated terminal bytes retained independently of the eventual root proof.
struct EvaluatedTerminal {
    report_bytes: Vec<u8>,
    policy_bytes: Vec<u8>,
    report_sha256: String,
    policy_sha256: String,
    semantic_input: String,
}

impl ActionResources {
    /// Obtain the original root current source for one actual terminal family.
    fn current(
        &mut self,
        family: NativeMaintenanceCaptureFamily,
    ) -> Result<NativeFamilyReleaseCurrent, Error> {
        NativeMaintenanceCaptureRemoteAuthority::new(&mut self.remote)
            .read_verified_family_current_full(family)
            .map_err(|_| Error::Remote)
    }

    /// Publish the source exact value-bearing terminal data and compact manifest.
    fn terminal_graph(
        &mut self,
        material: &NativeMaintenanceInstalledTerminalMaterial,
        action: &JsonValue,
        policy_bytes: &[u8],
        report: Option<&[u8]>,
    ) -> Result<NativeMaintenanceVerifiedFullGraph, Error> {
        let evidence = build_native_maintenance_terminal_archive_evidence(
            material,
            action,
            policy_bytes,
            report,
        )
        .map_err(|_| Error::Material)?;
        publish_native_maintenance_terminal_archive_evidence(
            &mut self.archive,
            &mut self.journal,
            evidence,
        )
        .map_err(|_| Error::Archive)
    }

    /// Build actual complete input seal, transferred anchor and finalization proof before any root mutation.
    fn build_terminal_authority(
        &mut self,
        material: &NativeMaintenanceInstalledTerminalMaterial,
        source_commit: &str,
        action: &JsonValue,
        evaluated: &EvaluatedTerminal,
        lifecycle_head: &str,
        archive: (&NativeMaintenanceVerifiedFullGraph, &str),
    ) -> Result<crate::NativeMaintenanceTerminalSealAuthority, Error> {
        let (graph, verified_at) = archive;
        let acknowledgement =
            crate::read_latest_native_maintenance_revision_custody_acknowledgement(
                &mut self.journal,
            )
            .map_err(|_| Error::Journal)?
            .ok_or(Error::Journal)?;
        let predecessor_seal =
            crate::read_native_maintenance_terminal_seal_current(&mut self.remote)
                .map_err(|_| Error::Remote)?;
        let head = read_native_maintenance_confirmation_head(&mut self.journal)
            .map_err(|_| Error::Journal)?;
        let predecessor_anchor =
            crate::read_native_maintenance_terminal_anchor_current(&mut self.remote)
                .map_err(|_| Error::Remote)?;
        let published = now()?;
        let finalized = now()?;
        let action_sha = identity(action)?;
        crate::build_native_maintenance_terminal_seal_authority(
            crate::NativeMaintenanceTerminalSealInput {
                acknowledgement: &acknowledgement,
                action_sha256: &action_sha,
                candidate_report_sha256: &evaluated.report_sha256,
                finalized_at: &finalized,
                full_graph_verified_at: verified_at,
                lifecycle_ledger_root_sha256: lifecycle_head,
                material,
                policy_report_sha256: &evaluated.policy_sha256,
                predecessor_anchor: predecessor_anchor.as_ref(),
                predecessor_seal: predecessor_seal.as_ref(),
                published_at: &published,
                terminal_graph: graph,
                workstation_journal_head_sha256: &head,
                source_commit,
            },
        )
        .map_err(|_| Error::Terminal)
    }

    /// Publish the actual owner-record result and retain the exact tombstone and retirement request.
    #[allow(clippy::too_many_arguments)]
    fn finish_owner_terminal(
        &mut self,
        due: &NativeMaintenanceDue,
        material: &NativeMaintenanceInstalledTerminalMaterial,
        semantic_input: &str,
        class: NativeMaintenanceOwnerTerminalClass,
        proof: &JsonValue,
        action: Option<&JsonValue>,
        receipt: &JsonValue,
        local_result: &JsonValue,
    ) -> Result<(), Error> {
        let family = family(material)?;
        let previous = read_native_maintenance_owner_terminal_tombstone(&mut self.journal, family)
            .map_err(|_| Error::Journal)?;
        let source_commit = required(proof, "sourceCommit")?;
        let graph = required(proof, "graphManifestSha256")?;
        let verified_at = now()?;
        let request = build_native_maintenance_terminal_record_request(
            NativeMaintenanceTerminalRecordInput {
                material,
                action,
                action_receipt: receipt,
                authority_proof: proof,
                local_result,
                previous_tombstone: previous.as_ref(),
                source_commit: &source_commit,
                terminal_graph_manifest_sha256: &graph,
                terminal_graph_verified_at: &verified_at,
                unsupported: class == NativeMaintenanceOwnerTerminalClass::Unsupported,
            },
        )
        .map_err(|_| Error::Material)?;
        let terminal = record_native_maintenance_terminal_remote(&self.remote, &request)
            .map_err(|_| Error::Remote)?;
        complete_native_maintenance_installed_owner_terminal(
            NativeMaintenanceInstalledOwnerTerminalInput {
                installed: material,
                authority_proof: proof,
                class,
                due_key: &due.due_key,
                now: &now()?,
                semantic_input_sha256: semantic_input,
                terminal_record: &terminal,
            },
            &mut self.archive,
            &mut self.journal,
            &mut self.remote,
        )
        .map_err(|_| Error::Terminal)?;
        Ok(())
    }

    /// Complete one nonpromoting evaluated member or resume its exact preinstalled authority.
    fn complete_no_action(
        &mut self,
        disposition: NativeMaintenanceDailyDisposition,
        due: &NativeMaintenanceDue,
        evaluation: &NativeMaintenanceDailyEvaluation,
        material: &NativeMaintenanceInstalledTerminalMaterial,
    ) -> Result<(), Error> {
        let family = family(material)?;
        let evaluated = evaluated_terminal(evaluation, material, Some(disposition))?;
        let terminal_disposition = match disposition {
            NativeMaintenanceDailyDisposition::Rejected => {
                NativeMaintenanceTerminalResultDisposition::Rejected
            }
            NativeMaintenanceDailyDisposition::SupportFailed => {
                NativeMaintenanceTerminalResultDisposition::SupportFailed
            }
        };
        let registration = local_registration(material)?;
        let shadow = &material.access.shadow_registration.registration_sha256;
        let action = build_native_maintenance_terminal_no_action_identity(
            terminal_disposition,
            &evaluated.policy_sha256,
            shadow,
        )
        .map_err(|_| Error::Material)?;
        // preserve an interrupted original C/T/F sequence rather than deriving new live-source documents
        let authority = if let Some(retained) =
            read_native_maintenance_terminal_authority(&mut self.journal, &registration, false)
                .map_err(|_| Error::Journal)?
        {
            restore_authority(&retained, material, &action, &evaluated, "actionIdentity")?
        } else {
            let current = self.current(family)?;
            let lifecycle = read_native_maintenance_confirmation_head(&mut self.journal)
                .map_err(|_| Error::Journal)?;
            let graph = self.terminal_graph(
                material,
                &action,
                &evaluated.policy_bytes,
                Some(&evaluated.report_bytes),
            )?;
            let verified = now()?;
            let authority = self.build_terminal_authority(
                material,
                &current.commit,
                &action,
                &evaluated,
                &lifecycle,
                (&graph, &verified),
            )?;
            stage_native_maintenance_no_action_terminal_authority(
                &mut self.journal,
                NativeMaintenanceTerminalAuthorityInput {
                    action: &action,
                    anchor: &authority.transferred_anchor.value,
                    current: &current,
                    family,
                    finalization_proof: &authority.finalization_proof.value,
                    now: &now()?,
                    registration_sha256: &registration,
                    seal: &authority.seal.value,
                },
                terminal_disposition,
            )
            .map_err(|_| Error::Journal)?;
            authority
        };
        crate::install_native_maintenance_terminal_seal_authority(&mut self.remote, &authority)
            .map_err(|_| Error::Remote)?;
        let proof = &authority.finalization_proof.value;
        let completed = required(proof, "finalizedAt")?;
        let next = embargo(&completed)?;
        let local = record_native_maintenance_confirmation_result(
            &mut self.journal,
            NativeMaintenanceTerminalResultInput {
                family,
                registration_sha256: &registration,
                candidate_report_sha256: &evaluated.policy_sha256,
                action_identity_sha256: None,
                disposition: terminal_disposition,
                next_confirmation_eligible_at: &next,
                now: &completed,
            },
        )
        .map_err(|_| Error::Journal)?;
        let full = required(&material.full_manifest, "fullMemberRootSha256")?;
        let receipt = build_native_maintenance_terminal_no_action_receipt(
            terminal_disposition,
            &evaluated.policy_sha256,
            shadow,
            &full,
            &completed,
            &completed,
        )
        .map_err(|_| Error::Material)?;
        let class = match disposition {
            NativeMaintenanceDailyDisposition::Rejected => {
                NativeMaintenanceOwnerTerminalClass::Rejected
            }
            NativeMaintenanceDailyDisposition::SupportFailed => {
                NativeMaintenanceOwnerTerminalClass::SupportFailed
            }
        };
        self.finish_owner_terminal(
            due,
            material,
            &evaluated.semantic_input,
            class,
            proof,
            None,
            &receipt.receipt,
            &local,
        )
    }
}

/// Read and exactly bind the native evaluator's complete policy and candidate-report bytes.
fn evaluated_terminal(
    evaluation: &NativeMaintenanceDailyEvaluation,
    material: &NativeMaintenanceInstalledTerminalMaterial,
    disposition: Option<NativeMaintenanceDailyDisposition>,
) -> Result<EvaluatedTerminal, Error> {
    let (semantic, value, expected_state, expected_action) = match (evaluation, disposition) {
        (
            NativeMaintenanceDailyEvaluation::Promote {
                semantic_input_sha256,
                value,
            },
            None,
        ) => (semantic_input_sha256, value, "pass", "promote"),
        (
            NativeMaintenanceDailyEvaluation::Reject {
                semantic_input_sha256,
                value,
            },
            Some(NativeMaintenanceDailyDisposition::Rejected),
        ) => (semantic_input_sha256, value, "fail", "retain"),
        (
            NativeMaintenanceDailyEvaluation::SupportFailed {
                semantic_input_sha256,
                value,
            },
            Some(NativeMaintenanceDailyDisposition::SupportFailed),
        ) => (semantic_input_sha256, value, "pending", "pending"),
        _ => return Err(Error::Material),
    };
    let policy = field(value, "policy").ok_or(Error::Material)?;
    let family = family(material)?;
    if text(value, "state").as_deref() != Some("evaluated")
        || text(value, "family").as_deref() != Some(family.wire())
        || field(value, "fullMemberRootSha256")
            != field(&material.full_manifest, "fullMemberRootSha256")
        || text(policy, "state").as_deref() != Some(expected_state)
        || text(policy, "action").as_deref() != Some(expected_action)
        || !valid_hash(semantic)
    {
        return Err(Error::Material);
    }
    let report_sha = required(value, "candidateReportSha256")?;
    let policy_sha = required(value, "policyReportSha256")?;
    let wire = NativeWirePrimitives;
    let report_bytes = wire
        .decode_standard_base64(&required(value, "candidateReportBase64")?)
        .ok_or(Error::Material)?;
    let policy_bytes = wire
        .decode_standard_base64(&required(value, "policyBytesBase64")?)
        .ok_or(Error::Material)?;
    if format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, &report_bytes)) != report_sha
        || format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, &policy_bytes)) != policy_sha
    {
        return Err(Error::Material);
    }
    Ok(EvaluatedTerminal {
        report_bytes,
        policy_bytes,
        report_sha256: report_sha,
        policy_sha256: policy_sha,
        semantic_input: semantic.clone(),
    })
}

/// Restore the exact staged authority documents and rebind them to the current retained member.
fn restore_authority(
    retained: &JsonValue,
    material: &NativeMaintenanceInstalledTerminalMaterial,
    action: &JsonValue,
    evaluated: &EvaluatedTerminal,
    action_field: &str,
) -> Result<crate::NativeMaintenanceTerminalSealAuthority, Error> {
    let seal = doc(field(retained, "seal").ok_or(Error::Material)?)?;
    let anchor = doc(field(retained, "anchor").ok_or(Error::Material)?)?;
    let proof = doc(field(retained, "finalizationProof").ok_or(Error::Material)?)?;
    let action_sha = identity(action)?;
    let current = field(retained, "current").ok_or(Error::Material)?;
    // compare true canonical source values rather than insertion ordering
    if field(retained, action_field)
        .map(identity)
        .transpose()?
        .as_deref()
        != Some(&action_sha)
        || text(retained, "registrationSha256").as_deref() != Some(&local_registration(material)?)
        || text(&anchor.value, "actionSha256").as_deref() != Some(&action_sha)
        || text(&anchor.value, "inputSealSha256").as_deref() != Some(&seal.sha256)
        || text(&proof.value, "transferredAnchorSha256").as_deref() != Some(&anchor.sha256)
        || text(&proof.value, "actionSha256").as_deref() != Some(&action_sha)
        || field(&proof.value, "graphManifestSha256") != field(&seal.value, "graphManifestSha256")
        || field(&proof.value, "fullMemberRootSha256")
            != field(&material.full_manifest, "fullMemberRootSha256")
        || text(&proof.value, "candidateReportSha256").as_deref() != Some(&evaluated.report_sha256)
        || text(&proof.value, "policyReportSha256").as_deref() != Some(&evaluated.policy_sha256)
        || field(&proof.value, "sourceCommit") != field(current, "commit")
    {
        return Err(Error::Material);
    }
    let input_class_members =
        crate::project_native_maintenance_terminal_input_class_members(material)
            .map_err(|_| Error::Material)?;
    let input_classes = field(&seal.value, "inputClasses")
        .ok_or(Error::Material)?
        .clone();
    Ok(crate::NativeMaintenanceTerminalSealAuthority {
        input_class_members,
        input_classes,
        seal,
        transferred_anchor: anchor,
        finalization_proof: proof,
    })
}

/// Construct a document from exact retained canonical bytes.
fn doc(value: &JsonValue) -> Result<crate::NativeMaintenanceTerminalAuthorityDocument, Error> {
    let canonical_bytes = NativeWirePrimitives
        .encode_sorted_json(value)
        .ok_or(Error::Material)?;
    let sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &canonical_bytes,
    ));
    Ok(crate::NativeMaintenanceTerminalAuthorityDocument {
        value: value.clone(),
        canonical_bytes,
        sha256,
    })
}

/// Identify the complete or unsupported original family.
fn family(
    material: &NativeMaintenanceInstalledTerminalMaterial,
) -> Result<NativeMaintenanceCaptureFamily, Error> {
    match &material.assembly {
        NativeMaintenanceDailyTerminalValues::Complete { family, .. }
        | NativeMaintenanceDailyTerminalValues::Unsupported { family, .. } => Ok(*family),
        _ => Err(Error::Material),
    }
}

/// Read the original local confirmation registration identity, distinct from the shadow registration.
fn local_registration(
    material: &NativeMaintenanceInstalledTerminalMaterial,
) -> Result<String, Error> {
    required(
        &material.access.confirmation_registration,
        "registrationSha256",
    )
}

/// Read a required exact source string.
fn required(value: &JsonValue, key: &str) -> Result<String, Error> {
    text(value, key).ok_or(Error::Material)
}

/// Identify one canonical source object.
fn identity(value: &JsonValue) -> Result<String, Error> {
    let bytes = NativeWirePrimitives
        .encode_sorted_json(value)
        .ok_or(Error::Material)?;
    Ok(format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &bytes,
    )))
}

/// Use the actual canonical wall clock at every independently bound operation.
fn now() -> Result<String, Error> {
    NativeMaintenanceArchiveWallClock.now().ok_or(Error::Clock)
}

/// Preserve the source's exact seven-day terminal embargo.
fn embargo(value: &str) -> Result<String, Error> {
    let wire = NativeWirePrimitives;
    let instant = wire
        .canonical_utc_milliseconds(value)
        .and_then(|value| value.checked_add(7 * 86_400_000))
        .ok_or(Error::Clock)?;
    wire.format_utc_milliseconds(instant).ok_or(Error::Clock)
}

impl ActionResources {
    /// Complete one permanent partial confirmation using only its actual unsupported proof and no-action receipt.
    fn complete_unsupported(
        &mut self,
        due: &NativeMaintenanceDue,
        semantic_input: &str,
        material: &NativeMaintenanceInstalledTerminalMaterial,
    ) -> Result<(), Error> {
        let NativeMaintenanceDailyTerminalValues::Unsupported { family, .. } = &material.assembly
        else {
            return Err(Error::Material);
        };
        let classes = crate::project_native_maintenance_terminal_input_class_members(material)
            .map_err(|_| Error::Material)?;
        // reproduce the source classification from missing actual semantic classes
        let empty = |name| matches!(field(&classes, name), Some(JsonValue::Array(values)) if values.is_empty());
        let reason = if ["shadow_body", "shadow_source", "comparator"]
            .into_iter()
            .any(empty)
        {
            "permanent_capture_gap"
        } else if ["target", "target_revision"].into_iter().any(empty) {
            "permanent_target_gap"
        } else {
            "permanent_source_gap"
        };
        let policy = crate::build_native_maintenance_unsupported_policy(material, reason)
            .map_err(|_| Error::Material)?;
        let shadow = &material.access.shadow_registration.registration_sha256;
        let action = build_native_maintenance_terminal_no_action_identity(
            NativeMaintenanceTerminalResultDisposition::SupportFailed,
            &policy.policy.sha256,
            shadow,
        )
        .map_err(|_| Error::Material)?;
        if identity(&action)? != policy.action_identity_sha256 {
            return Err(Error::Material);
        }
        let graph = self.terminal_graph(material, &action, &policy.policy.canonical_bytes, None)?;
        let verified = now()?;
        let acknowledgement =
            crate::read_latest_native_maintenance_revision_custody_acknowledgement(
                &mut self.journal,
            )
            .map_err(|_| Error::Journal)?
            .ok_or(Error::Journal)?;
        let predecessor =
            crate::read_native_maintenance_unsupported_terminal_proof_current(&mut self.remote)
                .map_err(|_| Error::Remote)?;
        let head = read_native_maintenance_confirmation_head(&mut self.journal)
            .map_err(|_| Error::Journal)?;
        let authority = crate::build_native_maintenance_unsupported_terminal_authority(
            crate::NativeMaintenanceUnsupportedTerminalInput {
                acknowledgement: &acknowledgement,
                finalized_at: &verified,
                full_graph_verified_at: &verified,
                material,
                predecessor: predecessor.as_ref(),
                source_commit: &material.access.source_commit,
                terminal_graph: &graph,
                unsupported_reason: reason,
                workstation_journal_head_sha256: &head,
            },
        )
        .map_err(|_| Error::Terminal)?;
        // the actual archived policy and unsupported authority must name exactly the same immutable action
        if authority.policy.sha256 != policy.policy.sha256
            || authority.action.action_sha256 != policy.action_identity_sha256
        {
            return Err(Error::Material);
        }
        crate::install_native_maintenance_unsupported_terminal_proof(&mut self.remote, &authority)
            .map_err(|_| Error::Remote)?;
        let completed = now()?;
        let next = embargo(&completed)?;
        let registration = local_registration(material)?;
        let local = record_native_maintenance_confirmation_result(
            &mut self.journal,
            NativeMaintenanceTerminalResultInput {
                family: *family,
                registration_sha256: &registration,
                candidate_report_sha256: &policy.policy.sha256,
                action_identity_sha256: None,
                disposition: NativeMaintenanceTerminalResultDisposition::SupportFailed,
                next_confirmation_eligible_at: &next,
                now: &completed,
            },
        )
        .map_err(|_| Error::Journal)?;
        self.finish_owner_terminal(
            due,
            material,
            semantic_input,
            NativeMaintenanceOwnerTerminalClass::Unsupported,
            &authority.proof.value,
            None,
            &authority.action.receipt,
            &local,
        )
    }
}

impl ActionResources {
    /// Complete a genuine promotion under the original global release fence or recover its staged authority.
    fn complete_qualified(
        &mut self,
        due: &NativeMaintenanceDue,
        evaluation: &NativeMaintenanceDailyEvaluation,
        material: &NativeMaintenanceInstalledTerminalMaterial,
    ) -> Result<NativeMaintenanceDailyQualifiedCompletion, Error> {
        let family = family(material)?;
        let evaluated = evaluated_terminal(evaluation, material, None)?;
        let registration = local_registration(material)?;
        // resume an originally staged promotion without deriving a replacement fence or release from live state
        if let Some(retained) =
            read_native_maintenance_terminal_authority(&mut self.journal, &registration, true)
                .map_err(|_| Error::Journal)?
        {
            let action = field(&retained, "action").ok_or(Error::Material)?.clone();
            let current =
                current_from_value(field(&retained, "current").ok_or(Error::Material)?, family)?;
            let action_sha256 = identity(&action)?;
            let local = crate::read_native_maintenance_confirmation_result(
                &mut self.journal,
                &registration,
            )
            .map_err(|_| Error::Journal)?
            .ok_or(Error::Journal)?;
            if text(&local, "disposition").as_deref() != Some("promoted")
                || text(&local, "actionIdentitySha256").as_deref() != Some(&action_sha256)
                || field(&action, "candidateSha256")
                    != field(
                        &material.access.shadow_registration.registration,
                        "candidateSha256",
                    )
                || text(&action, "candidateGraphSha256").as_deref()
                    != Some(&material.candidate.candidate_graph_sha256)
                || field(&action, "fullMemberRootSha256")
                    != field(&material.full_manifest, "fullMemberRootSha256")
                || text(&action, "policyReportSha256").as_deref() != Some(&evaluated.policy_sha256)
                || text(&action, "expectedSourceCommit").as_deref() != Some(&current.commit)
                || text(&action, "expectedSourceRelease").as_deref() != Some(&current.release)
                || text(&action, "expectedSettingsSha256").as_deref()
                    != Some(&current.settings_sha256)
            {
                return Err(Error::Material);
            }
            let authority = restore_authority(&retained, material, &action, &evaluated, "action")?;
            let action = crate::NativeMaintenanceQualifiedAction {
                action,
                action_sha256,
                candidate_report_bytes: evaluated.report_bytes.clone(),
                policy_bytes: evaluated.policy_bytes.clone(),
                policy_report_sha256: evaluated.policy_sha256.clone(),
                next_confirmation_eligible_at: required(&local, "nextConfirmationEligibleAt")?,
                semantic_input_sha256: evaluated.semantic_input.clone(),
            };
            crate::install_native_maintenance_terminal_seal_authority(&mut self.remote, &authority)
                .map_err(|_| Error::Remote)?;
            let lease = crate::continue_native_maintenance_qualified_release_lease(
                &mut self.journal,
                family,
                &registration,
                &now()?,
            )
            .map_err(|_| Error::Journal)?;
            let result = self.apply_qualified(
                due,
                material,
                &action,
                &current,
                lease.as_ref(),
                &authority.finalization_proof.value,
                &local,
            )?;
            // discharge the surviving exact original fence only after owner retirement
            if let Some(lease) = lease {
                release_qualified_lease(&mut self.journal, &lease)?;
            }
            return Ok(result);
        }
        let current = self.current(family)?;
        let candidate = required(
            &material.access.shadow_registration.registration,
            "candidateSha256",
        )?;
        if !valid_hash(&candidate) {
            return Err(Error::Material);
        }
        let due_key = format!("confirmation/{}/{candidate}", family.wire());
        let run_id = format!("release-promote-{}", &candidate[..32]);
        let head = read_native_maintenance_confirmation_head(&mut self.journal)
            .map_err(|_| Error::Journal)?;
        let acquisition = crate::acquire_native_maintenance_release_lease(
            &mut self.journal,
            crate::NativeMaintenanceReleaseLeaseInput {
                due_key: &due_key,
                input_head_sha256: &head,
                now: &now()?,
                run_id: &run_id,
            },
        )
        .map_err(|_| Error::Journal)?;
        let crate::NativeMaintenanceReleaseLeaseAcquisition::Held(lease) = acquisition else {
            return Err(Error::Journal);
        };
        let result =
            self.qualify_and_apply(due, evaluation, material, &evaluated, &current, &lease);
        // preserve the source finally guard; an unresolved staged action refuses release rather than discarding its fence
        release_qualified_lease(&mut self.journal, &lease)?;
        result
    }

    /// Archive, stage C/T/F and publish one actual release while holding its original fence.
    #[allow(clippy::too_many_arguments)]
    fn qualify_and_apply(
        &mut self,
        due: &NativeMaintenanceDue,
        evaluation: &NativeMaintenanceDailyEvaluation,
        material: &NativeMaintenanceInstalledTerminalMaterial,
        evaluated: &EvaluatedTerminal,
        current: &NativeFamilyReleaseCurrent,
        lease: &crate::NativeMaintenanceReleaseLease,
    ) -> Result<NativeMaintenanceDailyQualifiedCompletion, Error> {
        let action =
            crate::build_native_maintenance_qualified_action(material, evaluation, current, lease)
                .ok_or(Error::Material)?;
        let graph = self.terminal_graph(
            material,
            &action.action,
            &action.policy_bytes,
            Some(&action.candidate_report_bytes),
        )?;
        let verified_at = now()?;
        let lifecycle = required(&action.action, "lifecycleHeadSha256")?;
        let authority = self.build_terminal_authority(
            material,
            &current.commit,
            &action.action,
            evaluated,
            &lifecycle,
            (&graph, &verified_at),
        )?;
        let registration = local_registration(material)?;
        let family = family(material)?;
        let staged = crate::stage_native_maintenance_qualified_terminal_authority(
            &mut self.journal,
            crate::NativeMaintenanceQualifiedAuthorityInput {
                authority: NativeMaintenanceTerminalAuthorityInput {
                    action: &action.action,
                    anchor: &authority.transferred_anchor.value,
                    current,
                    family,
                    finalization_proof: &authority.finalization_proof.value,
                    now: &now()?,
                    registration_sha256: &registration,
                    seal: &authority.seal.value,
                },
                lease,
                candidate_report_sha256: &action.policy_report_sha256,
                next_confirmation_eligible_at: &action.next_confirmation_eligible_at,
            },
        )
        .map_err(|_| Error::Journal)?;
        // only the retained atomic qualified action authorizes installing the actual C/T/F chain
        if text(&staged.local_result, "actionIdentitySha256").as_deref()
            != Some(&action.action_sha256)
        {
            return Err(Error::Material);
        }
        crate::install_native_maintenance_terminal_seal_authority(&mut self.remote, &authority)
            .map_err(|_| Error::Remote)?;
        self.apply_qualified(
            due,
            material,
            &action,
            current,
            Some(lease),
            &authority.finalization_proof.value,
            &staged.local_result,
        )
    }

    /// Publish or recover an exact paired release, then record the authenticated owner terminal and retirement.
    #[allow(clippy::too_many_arguments)]
    fn apply_qualified(
        &mut self,
        due: &NativeMaintenanceDue,
        material: &NativeMaintenanceInstalledTerminalMaterial,
        action: &crate::NativeMaintenanceQualifiedAction,
        current: &NativeFamilyReleaseCurrent,
        lease: Option<&crate::NativeMaintenanceReleaseLease>,
        proof: &JsonValue,
        local: &JsonValue,
    ) -> Result<NativeMaintenanceDailyQualifiedCompletion, Error> {
        let released = crate::apply_native_maintenance_installed_qualified_release(
            crate::NativeMaintenanceInstalledQualifiedReleaseInput {
                action,
                material,
                current,
                lease,
                finalization_proof: proof,
            },
            &mut self.journal,
            &self.http,
            &mut self.remote,
        )
        .map_err(|_| Error::Qualified)?;
        self.finish_owner_terminal(
            due,
            material,
            &action.semantic_input_sha256,
            released.terminal_class,
            proof,
            Some(&action.action),
            &released.status.value,
            local,
        )?;
        Ok(NativeMaintenanceDailyQualifiedCompletion {
            serving_changed: released.serving_changed,
        })
    }
}

/// Release only the exact publisher run's original fence.
fn release_qualified_lease(
    journal: &mut NativeMaintenanceJournalStore,
    lease: &crate::NativeMaintenanceReleaseLease,
) -> Result<(), Error> {
    crate::release_native_maintenance_release_lease(
        journal,
        crate::NativeMaintenanceReleaseLeaseReleaseInput {
            due_key: &lease.due_key,
            now: &now()?,
            run_id: &lease.run_id,
        },
    )
    .map_err(|_| Error::Journal)
}

/// Restore the exact original current release projection retained before qualified publication.
fn current_from_value(
    value: &JsonValue,
    family: NativeMaintenanceCaptureFamily,
) -> Result<NativeFamilyReleaseCurrent, Error> {
    let nullable = |key| match field(value, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(_)) => text(value, key)
            .filter(|value| valid_hash(value))
            .map(Some)
            .ok_or(Error::Material),
        _ => Err(Error::Material),
    };
    if !exact_keys(
        value,
        &[
            "activeInstalledReceiptSha256",
            "catalogSha256",
            "commit",
            "controlInstalledReceiptSha256",
            "contractVersion",
            "family",
            "release",
            "settingsSha256",
            "shadowInstalledReceiptSha256",
            "sourceServerImageDigest",
        ],
    ) || text(value, "family").as_deref() != Some(family.wire())
        || text(value, "contractVersion").as_deref()
            != Some("adjustment-family-release-current-status/v1")
    {
        return Err(Error::Material);
    }
    Ok(NativeFamilyReleaseCurrent {
        active_installed_receipt_sha256: nullable("activeInstalledReceiptSha256")?,
        catalog_sha256: required(value, "catalogSha256")?,
        commit: required(value, "commit")?,
        control_installed_receipt_sha256: nullable("controlInstalledReceiptSha256")?,
        contract_version: required(value, "contractVersion")?,
        family: required(value, "family")?,
        release: required(value, "release")?,
        settings_sha256: required(value, "settingsSha256")?,
        shadow_installed_receipt_sha256: nullable("shadowInstalledReceiptSha256")?,
        source_server_image_digest: required(value, "sourceServerImageDigest")?,
    })
}
