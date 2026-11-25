//! Finalize actual post-burn chunks and restore the installed candidate for daily scoring and actions.

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeMaintenanceBurnedTerminalAccess, NativeMaintenanceDailyChunkInput,
    NativeMaintenanceDailyEvaluation, NativeMaintenanceDailyEvaluationRequest,
    NativeMaintenanceDailyTerminalChunk, NativeMaintenanceDailyTerminalMaterial,
    NativeMaintenanceDailyTerminalValues, NativeMaintenanceDailyUnsupportedMaterial,
    NativeMaintenanceDue, NativeMaintenanceJournalStore, NativeMaintenancePlaintextObjectStore,
    NativeMaintenanceRestoredCandidate, NativeMaintenanceSemanticInspection, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness, append_native_maintenance_confirmation_chunk,
    evaluate_native_maintenance_daily_terminal, finalize_native_maintenance_confirmation_member,
    maintenance_canonical_json::{text, valid_hash},
    native_maintenance_policy_comparison::{object, string},
    restore_native_maintenance_active_candidate,
};

/// Actual completed terminal material shared by the original evaluator and retirement actions.
pub struct NativeMaintenanceInstalledTerminalMaterial {
    pub access: NativeMaintenanceBurnedTerminalAccess,
    pub assembly: NativeMaintenanceDailyTerminalValues,
    pub candidate: NativeMaintenanceRestoredCandidate,
    pub full_manifest: JsonValue,
    pub catalog_input_manifest_sha256: String,
    pub clock_at: String,
    pub epoch_witness: JsonValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceInstalledTerminalMaterialError {
    Assembly,
    Journal,
    Candidate,
    Codec,
    Evaluation,
}

/// Persist every genuine physical part in source order then restore the acknowledged candidate.
pub fn finalize_native_maintenance_installed_terminal(
    access: NativeMaintenanceBurnedTerminalAccess,
    assembly: NativeMaintenanceDailyTerminalValues,
    catalog_input_manifest_sha256: &str,
    clock_at: &str,
    epoch_witness: JsonValue,
    journal: &mut NativeMaintenanceJournalStore,
    archive: &mut NativeMaintenancePlaintextObjectStore,
) -> Result<
    NativeMaintenanceInstalledTerminalMaterial,
    NativeMaintenanceInstalledTerminalMaterialError,
> {
    use NativeMaintenanceInstalledTerminalMaterialError as Error;
    let wire = NativeWirePrimitives;
    let (family, registration, chunks, expected_manifest) = match &assembly {
        NativeMaintenanceDailyTerminalValues::Complete {
            family,
            registration_sha256,
            chunks,
            full_manifest,
            ..
        } => (*family, registration_sha256, chunks, Some(full_manifest)),
        NativeMaintenanceDailyTerminalValues::Unsupported {
            family,
            registration_sha256,
            chunks,
            ..
        } => (*family, registration_sha256, chunks, None),
        NativeMaintenanceDailyTerminalValues::Pending { .. } => return Err(Error::Assembly),
    };
    // bind the actual post-burn member to the immutable registration and controller cutoff
    if !valid_hash(catalog_input_manifest_sha256)
        || wire.canonical_utc_milliseconds(clock_at).is_none()
        || text(&access.confirmation_registration, "registrationSha256").as_deref()
            != Some(registration)
        || text(&access.shadow_registration.registration, "family").as_deref()
            != Some(family.wire())
        || (access.unsupported && expected_manifest.is_some())
    {
        return Err(Error::Assembly);
    }
    // retain the original deterministic journal order with exact idempotence across interrupted runs
    for chunk in chunks {
        append_native_maintenance_confirmation_chunk(
            journal,
            family,
            registration,
            clock_at,
            &chunk.metadata,
        )
        .map_err(|_| Error::Journal)?;
    }
    let full_manifest =
        finalize_native_maintenance_confirmation_member(journal, family, registration, clock_at)
            .map_err(|_| Error::Journal)?;
    // require the independently assembled complete member to agree with actual journal finalization
    if let Some(expected) = expected_manifest
        && wire.encode_sorted_json(expected) != wire.encode_sorted_json(&full_manifest)
    {
        return Err(Error::Assembly);
    }
    let candidate = restore_native_maintenance_active_candidate(
        family,
        &text(&access.shadow_registration.registration, "candidateSha256")
            .ok_or(Error::Candidate)?,
        &text(&access.shadow_registration.registration, "artifactSha256")
            .ok_or(Error::Candidate)?,
        journal,
        archive,
    )
    .ok_or(Error::Candidate)?;
    Ok(NativeMaintenanceInstalledTerminalMaterial {
        access,
        assembly,
        candidate,
        full_manifest,
        catalog_input_manifest_sha256: catalog_input_manifest_sha256.to_owned(),
        clock_at: clock_at.to_owned(),
        epoch_witness,
    })
}

impl NativeMaintenanceInstalledTerminalMaterial {
    /// Project the original semantic inspection while retaining typed byte-bearing terminal material.
    pub fn inspection(
        &self,
        due: &NativeMaintenanceDue,
    ) -> Option<NativeMaintenanceSemanticInspection> {
        let common = self.common_value()?;
        match &self.assembly {
            NativeMaintenanceDailyTerminalValues::Complete { chunks, .. } => {
                let wire = NativeWirePrimitives;
                let lifecycle = object(vec![
                    ("access", self.access.local_burn.clone()),
                    (
                        "artifactBase64",
                        string(&wire.encode_standard_base64(&self.candidate.artifact_bytes)?),
                    ),
                    (
                        "candidateBase64",
                        string(&wire.encode_standard_base64(&self.candidate.candidate_bytes)?),
                    ),
                    (
                        "chunks",
                        JsonValue::Array(chunks.iter().map(chunk_value).collect()),
                    ),
                    (
                        "confirmationRegistration",
                        self.access.confirmation_registration.clone(),
                    ),
                    ("fullManifest", self.full_manifest.clone()),
                    (
                        "shadowRegistration",
                        self.access.shadow_registration.registration.clone(),
                    ),
                    ("state", string("burned_complete")),
                ]);
                let value = object(vec![
                    (
                        "catalogInputManifestSha256",
                        string(&self.catalog_input_manifest_sha256),
                    ),
                    ("clockAt", string(&self.clock_at)),
                    ("due", due_value(due)),
                    ("epochWitness", self.epoch_witness.clone()),
                    ("lifecycle", lifecycle),
                ]);
                Some(NativeMaintenanceSemanticInspection::DailyReady {
                    input: NativeMaintenanceDailyEvaluationRequest { value },
                    input_manifest_sha256: self.catalog_input_manifest_sha256.clone(),
                    terminal_material: Some(NativeMaintenanceDailyTerminalMaterial {
                        value: common,
                    }),
                })
            }
            NativeMaintenanceDailyTerminalValues::Unsupported { .. } => {
                Some(NativeMaintenanceSemanticInspection::DailyUnsupported {
                    input_manifest_sha256: self.catalog_input_manifest_sha256.clone(),
                    material: NativeMaintenanceDailyUnsupportedMaterial { value: common },
                })
            }
            NativeMaintenanceDailyTerminalValues::Pending { .. } => None,
        }
    }

    /// Evaluate the actual complete raw/package replay directly from retained typed material.
    pub fn evaluate(
        &self,
        due: &NativeMaintenanceDue,
        witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    ) -> Result<NativeMaintenanceDailyEvaluation, NativeMaintenanceInstalledTerminalMaterialError>
    {
        use NativeMaintenanceInstalledTerminalMaterialError as Error;
        let NativeMaintenanceDailyTerminalValues::Complete { chunks, .. } = &self.assembly else {
            return Err(Error::Assembly);
        };
        let chunks = chunks.iter().map(chunk_value).collect::<Vec<_>>();
        evaluate_native_maintenance_daily_terminal(
            due,
            &self.catalog_input_manifest_sha256,
            NativeMaintenanceDailyChunkInput {
                clock_at: &self.clock_at,
                candidate_bytes: &self.candidate.candidate_bytes,
                artifact_bytes: &self.candidate.artifact_bytes,
                confirmation_registration: &self.access.confirmation_registration,
                access: &self.access.local_burn,
                shadow_registration: &self.access.shadow_registration.registration,
                chunks: &chunks,
                full_manifest: &self.full_manifest,
                witness,
            },
        )
        .map_err(|_| Error::Evaluation)
    }

    /// Return compact immutable common evidence identities alongside retained typed byte material.
    pub fn common_value(&self) -> Option<JsonValue> {
        let wire = NativeWirePrimitives;
        let assembly = match &self.assembly {
            NativeMaintenanceDailyTerminalValues::Complete {
                chunks,
                contract_version,
                derived_targets,
                family,
                full_manifest,
                graph_manifest_sha256s,
                registration_sha256,
                rain_gate_input_member_sha256s,
                required_member_identity_sha256s,
                terminal_graph,
            } => object(vec![
                (
                    "chunks",
                    JsonValue::Array(chunks.iter().map(chunk_value).collect()),
                ),
                ("contractVersion", string(contract_version)),
                (
                    "derivedTargets",
                    JsonValue::Array(
                        derived_targets
                            .iter()
                            .map(|target| {
                                Some(object(vec![
                                    (
                                        "bytesBase64",
                                        string(&wire.encode_standard_base64(&target.bytes)?),
                                    ),
                                    ("targetMemberSha256", string(&target.target_member_sha256)),
                                ]))
                            })
                            .collect::<Option<Vec<_>>>()?,
                    ),
                ),
                ("family", string(family.wire())),
                ("fullManifest", full_manifest.clone()),
                ("graphManifestSha256s", strings(graph_manifest_sha256s)),
                ("registrationSha256", string(registration_sha256)),
                (
                    "rainGateInputMemberSha256s",
                    strings(rain_gate_input_member_sha256s),
                ),
                (
                    "requiredMemberIdentitySha256s",
                    strings(required_member_identity_sha256s),
                ),
                ("state", string("burned_complete")),
                ("terminalGraph", terminal_graph.clone()),
            ]),
            NativeMaintenanceDailyTerminalValues::Unsupported {
                chunks,
                contract_version,
                derived_targets,
                family,
                graph_manifest_sha256s,
                missing_key_count,
                missing_key_set_sha256,
                registration_sha256,
                rain_gate_input_member_sha256s,
                required_member_identity_sha256s,
                terminal_graph,
            } => object(vec![
                (
                    "chunks",
                    JsonValue::Array(chunks.iter().map(chunk_value).collect()),
                ),
                ("contractVersion", string(contract_version)),
                (
                    "derivedTargets",
                    JsonValue::Array(
                        derived_targets
                            .iter()
                            .map(|target| {
                                Some(object(vec![
                                    (
                                        "bytesBase64",
                                        string(&wire.encode_standard_base64(&target.bytes)?),
                                    ),
                                    ("targetMemberSha256", string(&target.target_member_sha256)),
                                ]))
                            })
                            .collect::<Option<Vec<_>>>()?,
                    ),
                ),
                ("family", string(family.wire())),
                ("graphManifestSha256s", strings(graph_manifest_sha256s)),
                (
                    "missingKeyCount",
                    JsonValue::Number(*missing_key_count as f64),
                ),
                ("missingKeySetSha256", string(missing_key_set_sha256)),
                ("registrationSha256", string(registration_sha256)),
                (
                    "rainGateInputMemberSha256s",
                    strings(rain_gate_input_member_sha256s),
                ),
                (
                    "requiredMemberIdentitySha256s",
                    strings(required_member_identity_sha256s),
                ),
                ("state", string("burned_unsupported")),
                ("terminalGraph", terminal_graph.clone()),
            ]),
            NativeMaintenanceDailyTerminalValues::Pending { .. } => return None,
        };
        Some(object(vec![
            ("assembly", assembly),
            (
                "candidate",
                object(vec![
                    (
                        "artifactBase64",
                        string(&wire.encode_standard_base64(&self.candidate.artifact_bytes)?),
                    ),
                    (
                        "candidateBase64",
                        string(&wire.encode_standard_base64(&self.candidate.candidate_bytes)?),
                    ),
                    (
                        "parityReceiptBase64",
                        string(&wire.encode_standard_base64(&self.candidate.parity_receipt_bytes)?),
                    ),
                    ("paritySha256", string(&self.candidate.parity_sha256)),
                    (
                        "candidateGraphSha256",
                        string(&self.candidate.candidate_graph_sha256),
                    ),
                ]),
            ),
            (
                "confirmationRegistration",
                self.access.confirmation_registration.clone(),
            ),
            ("epochWitness", self.epoch_witness.clone()),
            ("fullManifest", self.full_manifest.clone()),
            ("localBurn", self.access.local_burn.clone()),
            ("nativeAccess", self.access.native_access.clone()),
            ("plan", self.access.plan.clone()),
            (
                "planGraphManifestSha256",
                string(&self.access.plan_graph_manifest_sha256),
            ),
            (
                "shadowRegistration",
                self.access.shadow_registration.registration.clone(),
            ),
            ("sourceCommit", string(&self.access.source_commit)),
        ]))
    }
}

/// Project one exact value-bearing physical chunk.
fn chunk_value(chunk: &NativeMaintenanceDailyTerminalChunk) -> JsonValue {
    object(vec![
        ("metadata", chunk.metadata.clone()),
        ("payloadBase64", string(&chunk.payload_base64)),
    ])
}

/// Retain original ordered content-address arrays.
fn strings(values: &[String]) -> JsonValue {
    JsonValue::Array(values.iter().map(|value| string(value)).collect())
}

/// Preserve exact source daily due projection.
fn due_value(due: &NativeMaintenanceDue) -> JsonValue {
    object(vec![
        ("dueKey", string(&due.due_key)),
        ("family", JsonValue::Null),
        ("mode", string("daily")),
        ("originalCutoffAt", string(&due.original_cutoff_at)),
        ("scope", string(&due.scope)),
    ])
}
