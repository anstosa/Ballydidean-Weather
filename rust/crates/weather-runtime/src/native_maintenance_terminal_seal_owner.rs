//! Exact qualified terminal input closure, C/T/F documents, and fixed remote transport.

use std::collections::BTreeSet;
use std::fmt::{self, Display, Formatter};
use std::io;

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::maintenance_anchor_status::MaintenanceAnchorState;
use crate::maintenance_anchor_status_v3::validate_v3_anchor;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_future_input_seal_current::parse_future_input_seal;
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    NativeAdjustmentTerminalNoActionDisposition, NativeAdjustmentTerminalNoActionReceipt,
    NativeAdjustmentTerminalNoActionReceiptInput, NativeFutureInputSealCurrent,
    NativeMaintenanceCaptureFamily, NativeMaintenanceDailyTerminalValues,
    NativeMaintenanceDatabaseLedgerReader, NativeMaintenanceInstalledTerminalMaterial,
    NativeMaintenanceVerifiedFullGraph, NativeUnsupportedTerminalProofCurrent,
    NativeWirePrimitives, VerifiedNativeRevisionColdCustodyAcknowledgementV2,
    build_native_adjustment_terminal_no_action_receipt,
    build_native_maintenance_terminal_graph_parts,
    verify_native_revision_cold_custody_acknowledgement_v2_document,
};

pub const NATIVE_TERMINAL_SEAL_CURRENT_COMMAND: &str = "adjustment-future-input-seal-current-v2";
pub const NATIVE_TERMINAL_SEAL_INSTALL_COMMAND: &str = "adjustment-future-input-seal-install-v2";
pub const NATIVE_TERMINAL_ANCHOR_CURRENT_COMMAND: &str = "adjustment-maintenance-anchor-current-v3";
pub const NATIVE_TERMINAL_ANCHOR_INSTALL_COMMAND: &str = "adjustment-maintenance-anchor-install-v3";
pub const NATIVE_TERMINAL_ANCHOR_FINALIZE_COMMAND: &str =
    "adjustment-maintenance-anchor-finalize-v3";
pub const NATIVE_UNSUPPORTED_TERMINAL_PROOF_CURRENT_COMMAND: &str =
    "adjustment-unsupported-terminal-proof-current-v1";
pub const NATIVE_UNSUPPORTED_TERMINAL_PROOF_INSTALL_COMMAND: &str =
    "adjustment-unsupported-terminal-proof-install-v1";

const INPUT_CLASS_NAMES: &[&str] = &[
    "actual_best_match",
    "artifact",
    "candidate",
    "comparator",
    "native_source",
    "rain_gate_input",
    "shadow_body",
    "shadow_source",
    "target",
    "target_revision",
];
const BINDING_KEYS: &[&str] = &[
    "actualBestMatch",
    "capsule",
    "key",
    "nativeSource",
    "target",
];
const PROJECTION_KEYS: &[&str] = &["memberSha256", "payloadIdentitySha256", "receiptSha256"];
const CAPSULE_KEYS: &[&str] = &[
    "comparatorMemberSha256",
    "payloadIdentitySha256",
    "predictionBodySha256",
    "receiptSha256",
    "sourceProjectionSha256",
];
const TARGET_KEYS: &[&str] = &["sourceMemberSha256s", "targetMemberSha256"];
const CURRENT_KEYS: &[&str] = &["anchor", "anchorSha256"];
const SEAL_CURRENT_KEYS: &[&str] = &["seal", "sealSha256"];
const UNSUPPORTED_CURRENT_KEYS: &[&str] = &["proof", "proofSha256"];
const UNSUPPORTED_PROOF_KEYS: &[&str] = &[
    "actionSha256",
    "archiveCommitOrdinal",
    "burnSha256",
    "candidateReportSha256",
    "captureEpochWitnessSha256",
    "confirmationAccessSha256",
    "confirmationChunkCount",
    "contractVersion",
    "controlSha256",
    "controlVersion",
    "custodyCheckpointSha256",
    "dueKey",
    "eligiblePredictionSetSha256",
    "expectedKeySetSha256",
    "family",
    "finalizedAt",
    "frontierSha256",
    "fullGraphVerifiedAt",
    "fullMemberRootSha256",
    "graphManifestSha256",
    "inputClasses",
    "lifecycleLedgerRootSha256",
    "missingClassNames",
    "missingKeyCount",
    "missingKeySetSha256",
    "pageSha256",
    "policyReportSha256",
    "predecessorProofSha256",
    "registrationSha256",
    "requiredInputRootSha256",
    "sequence",
    "sourceCommit",
    "unsupportedReason",
    "workstationJournalHeadSha256",
];

/// One canonical self-addressed terminal authority document.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceTerminalAuthorityDocument {
    pub value: JsonValue,
    pub canonical_bytes: Vec<u8>,
    pub sha256: String,
}

/// Verified current v3 anchor used only as the next transferred predecessor.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceTerminalAnchorCurrent {
    pub anchor: JsonValue,
    pub anchor_sha256: String,
}

/// Complete exact future-only C/T/F construction for a supported installed terminal member.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceTerminalSealAuthority {
    pub input_class_members: JsonValue,
    pub input_classes: JsonValue,
    pub seal: NativeMaintenanceTerminalAuthorityDocument,
    pub transferred_anchor: NativeMaintenanceTerminalAuthorityDocument,
    pub finalization_proof: NativeMaintenanceTerminalAuthorityDocument,
}

/// Exact unsupported policy, no-action receipt, and custody-only proof construction.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceUnsupportedTerminalAuthority {
    pub action: NativeAdjustmentTerminalNoActionReceipt,
    pub input_class_members: JsonValue,
    pub input_classes: JsonValue,
    pub missing_class_names: Vec<String>,
    pub policy: NativeMaintenanceTerminalAuthorityDocument,
    pub proof: NativeMaintenanceTerminalAuthorityDocument,
}

/// Exact pre-publication unsupported policy identity and its genuine partial input classes.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceUnsupportedPolicyAuthority {
    pub action_identity_sha256: String,
    pub input_class_members: JsonValue,
    pub input_classes: JsonValue,
    pub missing_class_names: Vec<String>,
    pub policy: NativeMaintenanceTerminalAuthorityDocument,
}

/// Borrowed actual inputs captured at the original qualified or no-action owner boundary.
pub struct NativeMaintenanceTerminalSealInput<'a> {
    pub acknowledgement: &'a VerifiedNativeRevisionColdCustodyAcknowledgementV2,
    pub action_sha256: &'a str,
    pub candidate_report_sha256: &'a str,
    pub finalized_at: &'a str,
    pub full_graph_verified_at: &'a str,
    pub lifecycle_ledger_root_sha256: &'a str,
    pub material: &'a NativeMaintenanceInstalledTerminalMaterial,
    pub policy_report_sha256: &'a str,
    pub predecessor_anchor: Option<&'a NativeMaintenanceTerminalAnchorCurrent>,
    pub predecessor_seal: Option<&'a NativeFutureInputSealCurrent>,
    pub published_at: &'a str,
    pub source_commit: &'a str,
    pub terminal_graph: &'a NativeMaintenanceVerifiedFullGraph,
    pub workstation_journal_head_sha256: &'a str,
}

/// Borrowed actual inputs captured at the original unsupported terminal owner boundary.
pub struct NativeMaintenanceUnsupportedTerminalInput<'a> {
    pub acknowledgement: &'a VerifiedNativeRevisionColdCustodyAcknowledgementV2,
    pub finalized_at: &'a str,
    pub full_graph_verified_at: &'a str,
    pub material: &'a NativeMaintenanceInstalledTerminalMaterial,
    pub predecessor: Option<&'a NativeUnsupportedTerminalProofCurrent>,
    pub source_commit: &'a str,
    pub terminal_graph: &'a NativeMaintenanceVerifiedFullGraph,
    pub unsupported_reason: &'a str,
    pub workstation_journal_head_sha256: &'a str,
}

/// Exact successful fixed-command acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceTerminalSealInstallation {
    pub seal_sha256: String,
}

/// Exact successful transferred-anchor acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceTerminalAnchorInstallation {
    pub anchor_sha256: String,
}

/// Exact successful finalized-anchor acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceTerminalAnchorFinalization {
    pub anchor_sha256: String,
    pub finalization_proof_sha256: String,
}

/// All three acknowledgements after the source-ordered install/install/finalize transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceTerminalAuthorityInstallation {
    pub seal: NativeMaintenanceTerminalSealInstallation,
    pub anchor: NativeMaintenanceTerminalAnchorInstallation,
    pub finalization: NativeMaintenanceTerminalAnchorFinalization,
}

/// Exact successful unsupported proof installation acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceUnsupportedTerminalInstallation {
    pub proof_sha256: String,
}

/// Closed terminal projection, document, transport, and response refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceTerminalSealOwnerError {
    Material,
    TerminalGraph,
    InputClass,
    Custody,
    Predecessor,
    Clock,
    Seal,
    Anchor,
    Finalization,
    Transport,
    Response,
}

impl Display for NativeMaintenanceTerminalSealOwnerError {
    /// Render one stable refusal without reflecting retained evidence values.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Material => "maintenance terminal installed material differs",
            Self::TerminalGraph => "maintenance terminal graph differs",
            Self::InputClass => "maintenance terminal input class is unavailable",
            Self::Custody => "maintenance terminal custody acknowledgement differs",
            Self::Predecessor => "maintenance terminal predecessor differs",
            Self::Clock => "maintenance terminal authority clock is invalid",
            Self::Seal => "maintenance terminal input seal is invalid",
            Self::Anchor => "maintenance terminal transferred anchor is invalid",
            Self::Finalization => "maintenance terminal finalization proof is invalid",
            Self::Transport => "maintenance terminal authority transport failed",
            Self::Response => "maintenance terminal authority response differs",
        })
    }
}

impl std::error::Error for NativeMaintenanceTerminalSealOwnerError {}

/// Fixed privileged operations with no caller-selected command or path.
pub trait NativeMaintenanceTerminalSealRemotePort {
    /// Read the exact nullable current seal response.
    fn read_terminal_seal_current(&mut self) -> io::Result<Vec<u8>>;

    /// Install one canonical seal under its exact content operand.
    fn install_terminal_seal(
        &mut self,
        seal_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>>;

    /// Read the exact nullable current v3 anchor response.
    fn read_terminal_anchor_current(&mut self) -> io::Result<Vec<u8>>;

    /// Install one canonical transferred anchor under its exact content operand.
    fn install_terminal_anchor(
        &mut self,
        anchor_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>>;

    /// Finalize one transferred anchor using the exact proof content operand.
    fn finalize_terminal_anchor(
        &mut self,
        proof_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>>;

    /// Read the exact nullable current unsupported terminal proof response.
    fn read_unsupported_terminal_proof_current(&mut self) -> io::Result<Vec<u8>>;

    /// Install one canonical unsupported proof under its exact content operand.
    fn install_unsupported_terminal_proof(
        &mut self,
        proof_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>>;
}

impl NativeMaintenanceTerminalSealRemotePort for NativeMaintenanceDatabaseLedgerReader {
    /// Use the fixed zero-operand future seal current command.
    fn read_terminal_seal_current(&mut self) -> io::Result<Vec<u8>> {
        self.request_document(NATIVE_TERMINAL_SEAL_CURRENT_COMMAND, &[], None)
    }

    /// Use the fixed future seal install command with canonical stdin.
    fn install_terminal_seal(
        &mut self,
        seal_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.request_document(
            NATIVE_TERMINAL_SEAL_INSTALL_COMMAND,
            &[seal_sha256],
            Some(canonical_bytes),
        )
    }

    /// Use the fixed zero-operand v3 anchor current command.
    fn read_terminal_anchor_current(&mut self) -> io::Result<Vec<u8>> {
        self.request_document(NATIVE_TERMINAL_ANCHOR_CURRENT_COMMAND, &[], None)
    }

    /// Use the fixed transferred anchor install command with canonical stdin.
    fn install_terminal_anchor(
        &mut self,
        anchor_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.request_document(
            NATIVE_TERMINAL_ANCHOR_INSTALL_COMMAND,
            &[anchor_sha256],
            Some(canonical_bytes),
        )
    }

    /// Use the fixed v3 finalization command with canonical proof stdin.
    fn finalize_terminal_anchor(
        &mut self,
        proof_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.request_document(
            NATIVE_TERMINAL_ANCHOR_FINALIZE_COMMAND,
            &[proof_sha256],
            Some(canonical_bytes),
        )
    }

    /// Use the fixed zero-operand unsupported proof current command.
    fn read_unsupported_terminal_proof_current(&mut self) -> io::Result<Vec<u8>> {
        self.request_document(NATIVE_UNSUPPORTED_TERMINAL_PROOF_CURRENT_COMMAND, &[], None)
    }

    /// Use the fixed unsupported proof install command with canonical stdin.
    fn install_unsupported_terminal_proof(
        &mut self,
        proof_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.request_document(
            NATIVE_UNSUPPORTED_TERMINAL_PROOF_INSTALL_COMMAND,
            &[proof_sha256],
            Some(canonical_bytes),
        )
    }
}

/// Project the source's ten exact sorted member classes from installed terminal bindings.
pub fn project_native_maintenance_terminal_input_class_members(
    material: &NativeMaintenanceInstalledTerminalMaterial,
) -> Result<JsonValue, NativeMaintenanceTerminalSealOwnerError> {
    use NativeMaintenanceTerminalSealOwnerError as Error;

    let (family, rain_gate, terminal_graph) = terminal_assembly(material)?;
    let registration = &material.access.shadow_registration.registration;
    let candidate_sha256 = required_hash(registration, "candidateSha256", Error::Material)?;
    let artifact_sha256 = required_hash(registration, "artifactSha256", Error::Material)?;
    let artifact_identity_matches =
        format_digest(NativeWirePrimitives.sha256(&material.candidate.artifact_bytes))
            == artifact_sha256
            || canonical_artifact_identity(&material.candidate.artifact_bytes).as_deref()
                == Some(artifact_sha256.as_str());
    // bind the registered identities to the actual restored candidate and artifact bytes
    if format_digest(NativeWirePrimitives.sha256(&material.candidate.candidate_bytes))
        != candidate_sha256
        || !artifact_identity_matches
        || text(registration, "family").as_deref() != Some(family.wire())
    {
        return Err(Error::Material);
    }
    let rain_gate = admitted_hashes(rain_gate, Error::InputClass)?;
    // prohibit synthetic rain-gate members for the two non-rain families
    if family != NativeMaintenanceCaptureFamily::Rain && !rain_gate.is_empty() {
        return Err(Error::InputClass);
    }
    let mut classes = INPUT_CLASS_NAMES
        .iter()
        .map(|name| (*name, BTreeSet::<String>::new()))
        .collect::<std::collections::BTreeMap<_, _>>();
    classes
        .get_mut("candidate")
        .ok_or(Error::InputClass)?
        .insert(candidate_sha256);
    classes
        .get_mut("artifact")
        .ok_or(Error::InputClass)?
        .insert(artifact_sha256);
    classes
        .get_mut("rain_gate_input")
        .ok_or(Error::InputClass)?
        .extend(rain_gate);
    let bindings = match field(terminal_graph, "bindings") {
        Some(JsonValue::Array(bindings)) => bindings,
        _ => return Err(Error::TerminalGraph),
    };
    // derive only the graph-crosslinked identities retained by the source projection
    for binding in bindings {
        if !exact_keys(binding, BINDING_KEYS) {
            return Err(Error::TerminalGraph);
        }
        let actual = field(binding, "actualBestMatch").ok_or(Error::TerminalGraph)?;
        let capsule = field(binding, "capsule").ok_or(Error::TerminalGraph)?;
        let native = field(binding, "nativeSource").ok_or(Error::TerminalGraph)?;
        let target = field(binding, "target").ok_or(Error::TerminalGraph)?;
        if !exact_keys(actual, PROJECTION_KEYS)
            || !exact_keys(capsule, CAPSULE_KEYS)
            || !exact_keys(native, PROJECTION_KEYS)
            || !exact_keys(target, TARGET_KEYS)
        {
            return Err(Error::TerminalGraph);
        }
        insert_hash(&mut classes, "actual_best_match", actual, "memberSha256")?;
        insert_hash(
            &mut classes,
            "comparator",
            capsule,
            "comparatorMemberSha256",
        )?;
        insert_hash(&mut classes, "shadow_body", capsule, "predictionBodySha256")?;
        insert_hash(
            &mut classes,
            "shadow_source",
            capsule,
            "sourceProjectionSha256",
        )?;
        insert_hash(&mut classes, "native_source", native, "memberSha256")?;
        insert_hash(&mut classes, "target", target, "targetMemberSha256")?;
        let source_members = match field(target, "sourceMemberSha256s") {
            Some(JsonValue::Array(members)) => members,
            _ => return Err(Error::TerminalGraph),
        };
        // retain every target revision after exact hash admission and set deduplication
        for member in source_members {
            let identity = json_hash(member).ok_or(Error::TerminalGraph)?;
            classes
                .get_mut("target_revision")
                .ok_or(Error::InputClass)?
                .insert(identity);
        }
    }
    Ok(object(
        INPUT_CLASS_NAMES
            .iter()
            .map(|name| {
                let members = classes
                    .remove(*name)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|value| string(&value))
                    .collect();
                (*name, JsonValue::Array(members))
            })
            .collect(),
    ))
}

/// Build the exact input classes, future seal, transferred anchor, and finalization proof.
pub fn build_native_maintenance_terminal_seal_authority(
    input: NativeMaintenanceTerminalSealInput<'_>,
) -> Result<NativeMaintenanceTerminalSealAuthority, NativeMaintenanceTerminalSealOwnerError> {
    use NativeMaintenanceTerminalSealOwnerError as Error;

    let (family, _, terminal_graph_value) = complete_assembly(input.material)?;
    let acknowledgement = reverify_acknowledgement(input.acknowledgement)?;
    let input_class_members =
        project_native_maintenance_terminal_input_class_members(input.material)?;
    let input_classes = summarize_input_classes(&input_class_members, family)?;
    let registration = &input.material.access.shadow_registration.registration;
    let candidate_sha256 = required_hash(registration, "candidateSha256", Error::Material)?;
    let full_member_root_sha256 = required_hash(
        &input.material.full_manifest,
        "fullMemberRootSha256",
        Error::Material,
    )?;
    let burn_sha256 = required_hash(
        &input.material.access.local_burn,
        "accessSha256",
        Error::Material,
    )?;
    let confirmation_access_sha256 = required_hash(
        &input.material.access.native_access,
        "accessSha256",
        Error::Material,
    )?;
    let _terminal_graph_manifest_sha256 = verified_terminal_graph_manifest_identity(
        family,
        terminal_graph_value,
        input.terminal_graph,
    )?;
    let graph_manifest_sha256 = format_digest(input.terminal_graph.manifest_object_sha256);
    let capture_epoch_witness_sha256 = canonical_identity(&input.material.epoch_witness)?;
    let control_sha256 = required_hash(
        &input.material.epoch_witness,
        "controlPlaneSha256",
        Error::Material,
    )?;
    let control_version = text(&input.material.epoch_witness, "controlPlaneVersion")
        .filter(|value| valid_control_version(value))
        .ok_or(Error::Material)?;
    // close every caller-provided authority identity before deriving self-addressed documents
    if ![
        input.action_sha256,
        input.candidate_report_sha256,
        input.lifecycle_ledger_root_sha256,
        input.policy_report_sha256,
        input.workstation_journal_head_sha256,
    ]
    .into_iter()
    .all(valid_hash)
        || !valid_commit(input.source_commit)
    {
        return Err(Error::Material);
    }
    let sealed_at = instant(input.full_graph_verified_at).ok_or(Error::Clock)?;
    let published_at = instant(input.published_at).ok_or(Error::Clock)?;
    let finalized_at = instant(input.finalized_at).ok_or(Error::Clock)?;
    // preserve the exact graph-publication then anchor-publication then finalization order
    if sealed_at > published_at || published_at > finalized_at {
        return Err(Error::Clock);
    }
    let (sequence, predecessor_seal_sha256) = seal_predecessor(input.predecessor_seal)?;
    let predecessor_anchor_sha256 = anchor_predecessor(input.predecessor_anchor)?;
    let required_input_root_sha256 = canonical_identity(&input_classes)?;
    let comparator_root_sha256 = class_root(&input_classes, "comparator")?;
    let target_root_sha256 = class_root(&input_classes, "target")?;
    let candidate_root_sha256 = class_root(&input_classes, "candidate")?;
    let artifact_root_sha256 = class_root(&input_classes, "artifact")?;
    let candidate_artifact_root_sha256 = canonical_identity(&JsonValue::Array(vec![
        string(&candidate_root_sha256),
        string(&artifact_root_sha256),
    ]))?;
    let seal = object(vec![
        (
            "archiveCommitOrdinal",
            string(&acknowledgement.next_archive_commit_ordinal),
        ),
        ("burnSha256", string(&burn_sha256)),
        (
            "candidateArtifactRootSha256",
            string(&candidate_artifact_root_sha256),
        ),
        (
            "candidateReportSha256",
            string(input.candidate_report_sha256),
        ),
        (
            "captureEpochWitnessSha256",
            string(&capture_epoch_witness_sha256),
        ),
        ("comparatorRootSha256", string(&comparator_root_sha256)),
        (
            "confirmationAccessSha256",
            string(&confirmation_access_sha256),
        ),
        (
            "confirmationChunkCount",
            JsonValue::Number(if family == NativeMaintenanceCaptureFamily::Rain {
                24.0
            } else {
                27.0
            }),
        ),
        (
            "contractVersion",
            string("adjustment-future-only-input-seal/v2"),
        ),
        (
            "custodyCheckpointSha256",
            string(&acknowledgement.request.custody_checkpoint_sha256),
        ),
        (
            "dueKey",
            string(&format!(
                "confirmation/{}/{candidate_sha256}",
                family.wire()
            )),
        ),
        ("family", string(family.wire())),
        (
            "frontierSha256",
            string(&acknowledgement.next_frontier_sha256),
        ),
        ("fullMemberCount", JsonValue::Number(1.0)),
        ("fullMemberRootSha256", string(&full_member_root_sha256)),
        ("graphManifestSha256", string(&graph_manifest_sha256)),
        ("inputClasses", input_classes.clone()),
        (
            "lifecycleLedgerRootSha256",
            string(input.lifecycle_ledger_root_sha256),
        ),
        ("pageSha256", string(&acknowledgement.request.page_sha256)),
        ("policyReportSha256", string(input.policy_report_sha256)),
        ("predecessorSealSha256", predecessor_seal_sha256),
        (
            "requiredInputRootSha256",
            string(&required_input_root_sha256),
        ),
        ("sealedAt", string(input.full_graph_verified_at)),
        ("sequence", string(&sequence.to_string())),
        ("sourceCommit", string(input.source_commit)),
        ("targetRootSha256", string(&target_root_sha256)),
        (
            "workstationJournalHeadSha256",
            string(input.workstation_journal_head_sha256),
        ),
    ]);
    let seal_document = admitted_seal_document(seal)?;
    let anchor = object(vec![
        ("actionSha256", string(input.action_sha256)),
        (
            "archiveCommitOrdinal",
            copied(&seal_document.value, "archiveCommitOrdinal")?,
        ),
        ("burnSha256", copied(&seal_document.value, "burnSha256")?),
        (
            "candidateArtifactRootSha256",
            copied(&seal_document.value, "candidateArtifactRootSha256")?,
        ),
        (
            "candidateReportSha256",
            copied(&seal_document.value, "candidateReportSha256")?,
        ),
        (
            "captureEpochWitnessSha256",
            copied(&seal_document.value, "captureEpochWitnessSha256")?,
        ),
        (
            "confirmationAccessSha256",
            copied(&seal_document.value, "confirmationAccessSha256")?,
        ),
        (
            "confirmationChunkCount",
            copied(&seal_document.value, "confirmationChunkCount")?,
        ),
        (
            "contractVersion",
            string("adjustment-maintenance-anchor/v3"),
        ),
        ("controlSha256", string(&control_sha256)),
        ("controlVersion", string(&control_version)),
        ("ctfState", string("transferred")),
        (
            "custodyCheckpointSha256",
            copied(&seal_document.value, "custodyCheckpointSha256")?,
        ),
        ("dueKey", copied(&seal_document.value, "dueKey")?),
        ("family", copied(&seal_document.value, "family")?),
        (
            "frontierSha256",
            copied(&seal_document.value, "frontierSha256")?,
        ),
        ("fullGraphVerifiedAt", string(input.full_graph_verified_at)),
        (
            "fullMemberRootSha256",
            copied(&seal_document.value, "fullMemberRootSha256")?,
        ),
        (
            "graphManifestSha256",
            copied(&seal_document.value, "graphManifestSha256")?,
        ),
        ("inputSealSha256", string(&seal_document.sha256)),
        (
            "lifecycleLedgerRootSha256",
            copied(&seal_document.value, "lifecycleLedgerRootSha256")?,
        ),
        ("pageSha256", copied(&seal_document.value, "pageSha256")?),
        (
            "policyReportSha256",
            copied(&seal_document.value, "policyReportSha256")?,
        ),
        ("predecessorAnchorSha256", predecessor_anchor_sha256),
        ("publishedAt", string(input.published_at)),
        (
            "requiredInputRootSha256",
            copied(&seal_document.value, "requiredInputRootSha256")?,
        ),
        ("sequence", copied(&seal_document.value, "sequence")?),
        (
            "sourceCommit",
            copied(&seal_document.value, "sourceCommit")?,
        ),
        (
            "workstationJournalHeadSha256",
            copied(&seal_document.value, "workstationJournalHeadSha256")?,
        ),
    ]);
    let transferred_anchor = admitted_anchor_document(anchor, MaintenanceAnchorState::Transferred)?;
    let proof = finalization_proof(&transferred_anchor, input.finalized_at)?;
    Ok(NativeMaintenanceTerminalSealAuthority {
        input_class_members,
        input_classes,
        seal: seal_document,
        transferred_anchor,
        finalization_proof: proof,
    })
}

/// Build the source-classified unsupported policy before terminal evidence publication.
pub fn build_native_maintenance_unsupported_policy(
    material: &NativeMaintenanceInstalledTerminalMaterial,
    unsupported_reason: &str,
) -> Result<NativeMaintenanceUnsupportedPolicyAuthority, NativeMaintenanceTerminalSealOwnerError> {
    use NativeMaintenanceTerminalSealOwnerError as Error;

    let NativeMaintenanceDailyTerminalValues::Unsupported {
        family,
        missing_key_count,
        missing_key_set_sha256,
        registration_sha256: local_registration_sha256,
        ..
    } = &material.assembly
    else {
        return Err(Error::Material);
    };
    let input_class_members = project_native_maintenance_terminal_input_class_members(material)?;
    let (input_classes, missing_class_names) =
        summarize_unsupported_input_classes(&input_class_members, *family)?;
    let classified_reason = classify_unsupported_reason(&missing_class_names);
    // require genuine source-classified missing evidence rather than caller-selected policy
    if *missing_key_count == 0
        || *missing_key_count > 9_500_000
        || !valid_hash(missing_key_set_sha256)
        || classified_reason != unsupported_reason
    {
        return Err(Error::Material);
    }
    let registration = &material.access.shadow_registration.registration;
    let registered_sha256 = required_hash(registration, "registrationSha256", Error::Material)?;
    let confirmation_registration_sha256 = required_hash(
        &material.access.confirmation_registration,
        "registrationSha256",
        Error::Material,
    )?;
    let target_cutoff_at = text(registration, "targetCutoffAt")
        .filter(|value| instant(value).is_some())
        .ok_or(Error::Material)?;
    // bind the pre-publication report to actual registration, manifest, and native cutoff inputs
    if confirmation_registration_sha256 != *local_registration_sha256
        || text(&material.access.native_access, "registrationSha256").as_deref()
            != Some(registered_sha256.as_str())
        || text(&material.access.local_burn, "registrationSha256").as_deref()
            != Some(local_registration_sha256)
        || text(&material.full_manifest, "registrationSha256").as_deref()
            != Some(local_registration_sha256)
        || text(&material.full_manifest, "missingKeySetSha256").as_deref()
            != Some(missing_key_set_sha256)
        || text(&material.full_manifest, "targetCutoffAt").as_deref()
            != Some(target_cutoff_at.as_str())
        || text(&material.access.native_access, "targetCutoffAt").as_deref()
            != Some(target_cutoff_at.as_str())
    {
        return Err(Error::Material);
    }
    let policy_value = object(vec![
        (
            "contractVersion",
            string("adjustment-maintenance-unsupported-policy-report/v1"),
        ),
        ("family", string(family.wire())),
        (
            "missingClassNames",
            JsonValue::Array(
                missing_class_names
                    .iter()
                    .map(|name| string(name))
                    .collect(),
            ),
        ),
        (
            "missingKeyCount",
            JsonValue::Number(*missing_key_count as f64),
        ),
        ("missingKeySetSha256", string(missing_key_set_sha256)),
        ("registrationSha256", string(&registered_sha256)),
        ("state", string("unsupported")),
        ("targetCutoffAt", string(&target_cutoff_at)),
        ("unsupportedReason", string(unsupported_reason)),
    ]);
    let policy = canonical_document(policy_value, Error::Material)?;
    let action_identity = object(vec![
        (
            "contractVersion",
            string("adjustment-terminal-no-action-identity/v3"),
        ),
        ("disposition", string("support_failed")),
        ("policyReportSha256", string(&policy.sha256)),
        ("registrationSha256", string(&registered_sha256)),
    ]);
    let action_identity_sha256 = canonical_identity(&action_identity)?;
    Ok(NativeMaintenanceUnsupportedPolicyAuthority {
        action_identity_sha256,
        input_class_members,
        input_classes,
        missing_class_names,
        policy,
    })
}

/// Build the exact unsupported policy, no-action receipt, and custody-only proof.
pub fn build_native_maintenance_unsupported_terminal_authority(
    input: NativeMaintenanceUnsupportedTerminalInput<'_>,
) -> Result<NativeMaintenanceUnsupportedTerminalAuthority, NativeMaintenanceTerminalSealOwnerError>
{
    use NativeMaintenanceTerminalSealOwnerError as Error;

    let NativeMaintenanceDailyTerminalValues::Unsupported {
        family,
        missing_key_count,
        missing_key_set_sha256,
        terminal_graph,
        ..
    } = &input.material.assembly
    else {
        return Err(Error::Material);
    };
    let acknowledgement = reverify_acknowledgement(input.acknowledgement)?;
    let policy_authority =
        build_native_maintenance_unsupported_policy(input.material, input.unsupported_reason)?;
    let NativeMaintenanceUnsupportedPolicyAuthority {
        action_identity_sha256,
        input_class_members,
        input_classes,
        missing_class_names,
        policy,
    } = policy_authority;
    let registration = &input.material.access.shadow_registration.registration;
    let candidate_sha256 = required_hash(registration, "candidateSha256", Error::Material)?;
    let shadow_registration_sha256 =
        required_hash(registration, "registrationSha256", Error::Material)?;
    let full_member_root_sha256 = required_hash(
        &input.material.full_manifest,
        "fullMemberRootSha256",
        Error::Material,
    )?;
    let burn_sha256 = required_hash(
        &input.material.access.local_burn,
        "accessSha256",
        Error::Material,
    )?;
    let expected_key_set_sha256 = required_hash(
        &input.material.access.local_burn,
        "expectedKeySetSha256",
        Error::Material,
    )?;
    let confirmation_access_sha256 = required_hash(
        &input.material.access.native_access,
        "accessSha256",
        Error::Material,
    )?;
    let eligible_prediction_set_sha256 = required_hash(
        &input.material.access.native_access,
        "eligiblePredictionSetSha256",
        Error::Material,
    )?;
    let target_cutoff_at = text(&input.material.access.native_access, "targetCutoffAt")
        .filter(|value| instant(value).is_some())
        .ok_or(Error::Material)?;
    let _terminal_graph_manifest_sha256 =
        verified_terminal_graph_manifest_identity(*family, terminal_graph, input.terminal_graph)?;
    // bind the post-publication proof to the fresh source revision and exact journal head
    if !valid_hash(input.workstation_journal_head_sha256) || !valid_commit(input.source_commit) {
        return Err(Error::Material);
    }
    let finalized_at = instant(input.finalized_at).ok_or(Error::Clock)?;
    let full_graph_verified_at = instant(input.full_graph_verified_at).ok_or(Error::Clock)?;
    // require completion after graph verification and an exact canonical cutoff instant
    if finalized_at < full_graph_verified_at || instant(&target_cutoff_at).is_none() {
        return Err(Error::Clock);
    }
    let action = build_native_adjustment_terminal_no_action_receipt(
        NativeAdjustmentTerminalNoActionReceiptInput {
            completed_at: input.finalized_at,
            disposition: NativeAdjustmentTerminalNoActionDisposition::SupportFailed,
            finalized_at: input.full_graph_verified_at,
            full_member_root_sha256: &full_member_root_sha256,
            policy_report_sha256: &policy.sha256,
            registration_sha256: &shadow_registration_sha256,
        },
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .map_err(|_| Error::Material)?;
    // reproduce the pre-publication no-action identity in the completed receipt builder
    if action.action_sha256 != action_identity_sha256 {
        return Err(Error::Material);
    }
    let (sequence, predecessor_proof_sha256) = unsupported_predecessor(input.predecessor)?;
    let capture_epoch_witness_sha256 = canonical_identity(&input.material.epoch_witness)?;
    let control_sha256 = required_hash(
        &input.material.epoch_witness,
        "controlPlaneSha256",
        Error::Material,
    )?;
    let control_version = text(&input.material.epoch_witness, "controlPlaneVersion")
        .filter(|value| valid_control_version(value))
        .ok_or(Error::Material)?;
    let required_input_root_sha256 = canonical_identity(&input_classes)?;
    let graph_manifest_sha256 = format_digest(input.terminal_graph.manifest_object_sha256);
    let proof_value = object(vec![
        ("actionSha256", string(&action.action_sha256)),
        (
            "archiveCommitOrdinal",
            string(&acknowledgement.next_archive_commit_ordinal),
        ),
        ("burnSha256", string(&burn_sha256)),
        ("candidateReportSha256", string(&policy.sha256)),
        (
            "captureEpochWitnessSha256",
            string(&capture_epoch_witness_sha256),
        ),
        (
            "confirmationAccessSha256",
            string(&confirmation_access_sha256),
        ),
        (
            "confirmationChunkCount",
            JsonValue::Number(if *family == NativeMaintenanceCaptureFamily::Rain {
                24.0
            } else {
                27.0
            }),
        ),
        (
            "contractVersion",
            string("adjustment-maintenance-unsupported-terminal-proof/v1"),
        ),
        ("controlSha256", string(&control_sha256)),
        ("controlVersion", string(&control_version)),
        (
            "custodyCheckpointSha256",
            string(&acknowledgement.request.custody_checkpoint_sha256),
        ),
        (
            "dueKey",
            string(&format!(
                "confirmation/{}/{candidate_sha256}",
                family.wire()
            )),
        ),
        (
            "eligiblePredictionSetSha256",
            string(&eligible_prediction_set_sha256),
        ),
        ("expectedKeySetSha256", string(&expected_key_set_sha256)),
        ("family", string(family.wire())),
        ("finalizedAt", string(input.finalized_at)),
        (
            "frontierSha256",
            string(&acknowledgement.next_frontier_sha256),
        ),
        ("fullGraphVerifiedAt", string(input.full_graph_verified_at)),
        ("fullMemberRootSha256", string(&full_member_root_sha256)),
        ("graphManifestSha256", string(&graph_manifest_sha256)),
        ("inputClasses", input_classes.clone()),
        (
            "lifecycleLedgerRootSha256",
            string(input.workstation_journal_head_sha256),
        ),
        (
            "missingClassNames",
            JsonValue::Array(
                missing_class_names
                    .iter()
                    .map(|name| string(name))
                    .collect(),
            ),
        ),
        (
            "missingKeyCount",
            JsonValue::Number(*missing_key_count as f64),
        ),
        ("missingKeySetSha256", string(missing_key_set_sha256)),
        ("pageSha256", string(&acknowledgement.request.page_sha256)),
        ("policyReportSha256", string(&policy.sha256)),
        ("predecessorProofSha256", predecessor_proof_sha256),
        ("registrationSha256", string(&shadow_registration_sha256)),
        (
            "requiredInputRootSha256",
            string(&required_input_root_sha256),
        ),
        ("sequence", string(&sequence.to_string())),
        ("sourceCommit", string(input.source_commit)),
        ("unsupportedReason", string(input.unsupported_reason)),
        (
            "workstationJournalHeadSha256",
            string(input.workstation_journal_head_sha256),
        ),
    ]);
    let proof = admitted_unsupported_proof_document(proof_value)?;
    Ok(NativeMaintenanceUnsupportedTerminalAuthority {
        action,
        input_class_members,
        input_classes,
        missing_class_names,
        policy,
        proof,
    })
}

/// Read and verify the exact nullable predecessor unsupported terminal proof.
pub fn read_native_maintenance_unsupported_terminal_proof_current(
    remote: &mut impl NativeMaintenanceTerminalSealRemotePort,
) -> Result<Option<NativeUnsupportedTerminalProofCurrent>, NativeMaintenanceTerminalSealOwnerError>
{
    let bytes = remote
        .read_unsupported_terminal_proof_current()
        .map_err(|_| NativeMaintenanceTerminalSealOwnerError::Transport)?;
    verify_unsupported_current(&bytes)
}

/// Install one unsupported proof through the fixed custody-only privileged operation.
pub fn install_native_maintenance_unsupported_terminal_proof(
    remote: &mut impl NativeMaintenanceTerminalSealRemotePort,
    authority: &NativeMaintenanceUnsupportedTerminalAuthority,
) -> Result<NativeMaintenanceUnsupportedTerminalInstallation, NativeMaintenanceTerminalSealOwnerError>
{
    let response = remote
        .install_unsupported_terminal_proof(
            &authority.proof.sha256,
            &authority.proof.canonical_bytes,
        )
        .map_err(|_| NativeMaintenanceTerminalSealOwnerError::Transport)?;
    let value =
        canonical_value(&response).ok_or(NativeMaintenanceTerminalSealOwnerError::Response)?;
    // admit only the exact immutable installed response for the submitted unsupported proof
    if !exact_keys(&value, &["contractVersion", "proofSha256", "state"])
        || text(&value, "contractVersion").as_deref()
            != Some("adjustment-maintenance-unsupported-terminal-proof-installation/v1")
        || text(&value, "proofSha256").as_deref() != Some(authority.proof.sha256.as_str())
        || text(&value, "state").as_deref() != Some("installed")
    {
        return Err(NativeMaintenanceTerminalSealOwnerError::Response);
    }
    Ok(NativeMaintenanceUnsupportedTerminalInstallation {
        proof_sha256: authority.proof.sha256.clone(),
    })
}

/// Read and verify the exact nullable predecessor input seal.
pub fn read_native_maintenance_terminal_seal_current(
    remote: &mut impl NativeMaintenanceTerminalSealRemotePort,
) -> Result<Option<NativeFutureInputSealCurrent>, NativeMaintenanceTerminalSealOwnerError> {
    let bytes = remote
        .read_terminal_seal_current()
        .map_err(|_| NativeMaintenanceTerminalSealOwnerError::Transport)?;
    verify_seal_current(&bytes)
}

/// Read and verify the exact nullable predecessor v3 anchor.
pub fn read_native_maintenance_terminal_anchor_current(
    remote: &mut impl NativeMaintenanceTerminalSealRemotePort,
) -> Result<Option<NativeMaintenanceTerminalAnchorCurrent>, NativeMaintenanceTerminalSealOwnerError>
{
    let bytes = remote
        .read_terminal_anchor_current()
        .map_err(|_| NativeMaintenanceTerminalSealOwnerError::Transport)?;
    verify_anchor_current(&bytes)
}

/// Install C then T then finalize F through the source's fixed privileged operations.
pub fn install_native_maintenance_terminal_seal_authority(
    remote: &mut impl NativeMaintenanceTerminalSealRemotePort,
    authority: &NativeMaintenanceTerminalSealAuthority,
) -> Result<NativeMaintenanceTerminalAuthorityInstallation, NativeMaintenanceTerminalSealOwnerError>
{
    let seal = install_seal(remote, &authority.seal)?;
    let anchor = install_anchor(remote, &authority.transferred_anchor)?;
    let finalization = finalize_anchor(
        remote,
        &authority.transferred_anchor,
        &authority.finalization_proof,
    )?;
    Ok(NativeMaintenanceTerminalAuthorityInstallation {
        seal,
        anchor,
        finalization,
    })
}

/// Install one admitted future-only seal and bind the exact response identity.
fn install_seal(
    remote: &mut impl NativeMaintenanceTerminalSealRemotePort,
    seal: &NativeMaintenanceTerminalAuthorityDocument,
) -> Result<NativeMaintenanceTerminalSealInstallation, NativeMaintenanceTerminalSealOwnerError> {
    let response = remote
        .install_terminal_seal(&seal.sha256, &seal.canonical_bytes)
        .map_err(|_| NativeMaintenanceTerminalSealOwnerError::Transport)?;
    let value =
        canonical_value(&response).ok_or(NativeMaintenanceTerminalSealOwnerError::Response)?;
    // admit only the exact immutable installed response for the submitted seal
    if !exact_keys(&value, &["contractVersion", "sealSha256", "state"])
        || text(&value, "contractVersion").as_deref()
            != Some("adjustment-future-only-input-seal-installation/v2")
        || text(&value, "sealSha256").as_deref() != Some(seal.sha256.as_str())
        || text(&value, "state").as_deref() != Some("installed")
    {
        return Err(NativeMaintenanceTerminalSealOwnerError::Response);
    }
    Ok(NativeMaintenanceTerminalSealInstallation {
        seal_sha256: seal.sha256.clone(),
    })
}

/// Install one admitted transferred anchor and bind the exact response identity.
fn install_anchor(
    remote: &mut impl NativeMaintenanceTerminalSealRemotePort,
    anchor: &NativeMaintenanceTerminalAuthorityDocument,
) -> Result<NativeMaintenanceTerminalAnchorInstallation, NativeMaintenanceTerminalSealOwnerError> {
    let response = remote
        .install_terminal_anchor(&anchor.sha256, &anchor.canonical_bytes)
        .map_err(|_| NativeMaintenanceTerminalSealOwnerError::Transport)?;
    let value =
        canonical_value(&response).ok_or(NativeMaintenanceTerminalSealOwnerError::Response)?;
    // admit only the exact transferred response for the submitted anchor
    if !exact_keys(&value, &["anchorSha256", "contractVersion", "state"])
        || text(&value, "anchorSha256").as_deref() != Some(anchor.sha256.as_str())
        || text(&value, "contractVersion").as_deref()
            != Some("adjustment-maintenance-anchor-installation/v3")
        || text(&value, "state").as_deref() != Some("transferred")
    {
        return Err(NativeMaintenanceTerminalSealOwnerError::Response);
    }
    Ok(NativeMaintenanceTerminalAnchorInstallation {
        anchor_sha256: anchor.sha256.clone(),
    })
}

/// Finalize one admitted transferred anchor and bind both response identities.
fn finalize_anchor(
    remote: &mut impl NativeMaintenanceTerminalSealRemotePort,
    anchor: &NativeMaintenanceTerminalAuthorityDocument,
    proof: &NativeMaintenanceTerminalAuthorityDocument,
) -> Result<NativeMaintenanceTerminalAnchorFinalization, NativeMaintenanceTerminalSealOwnerError> {
    let response = remote
        .finalize_terminal_anchor(&proof.sha256, &proof.canonical_bytes)
        .map_err(|_| NativeMaintenanceTerminalSealOwnerError::Transport)?;
    let value =
        canonical_value(&response).ok_or(NativeMaintenanceTerminalSealOwnerError::Response)?;
    // admit only the exact finalized response for the submitted proof and transferred anchor
    if !exact_keys(
        &value,
        &[
            "anchorSha256",
            "contractVersion",
            "finalizationProofSha256",
            "state",
        ],
    ) || text(&value, "anchorSha256").as_deref() != Some(anchor.sha256.as_str())
        || text(&value, "contractVersion").as_deref()
            != Some("adjustment-maintenance-anchor-finalization/v3")
        || text(&value, "finalizationProofSha256").as_deref() != Some(proof.sha256.as_str())
        || text(&value, "state").as_deref() != Some("finalized")
    {
        return Err(NativeMaintenanceTerminalSealOwnerError::Response);
    }
    Ok(NativeMaintenanceTerminalAnchorFinalization {
        anchor_sha256: anchor.sha256.clone(),
        finalization_proof_sha256: proof.sha256.clone(),
    })
}

/// Summarize every already sorted unique class into its exact count and canonical root.
fn summarize_input_classes(
    members: &JsonValue,
    family: NativeMaintenanceCaptureFamily,
) -> Result<JsonValue, NativeMaintenanceTerminalSealOwnerError> {
    use NativeMaintenanceTerminalSealOwnerError::InputClass;

    if !exact_keys(members, INPUT_CLASS_NAMES) {
        return Err(InputClass);
    }
    let mut entries = Vec::with_capacity(INPUT_CLASS_NAMES.len());
    // bind every class count and root to its exact ordered identity population
    for name in INPUT_CLASS_NAMES {
        let identities = match field(members, name) {
            Some(JsonValue::Array(identities)) => identities,
            _ => return Err(InputClass),
        };
        if identities.len() > 9_500_000
            || identities
                .iter()
                .any(|identity| json_hash(identity).is_none())
            || identities
                .windows(2)
                .any(|pair| match (&pair[0], &pair[1]) {
                    (JsonValue::String(left), JsonValue::String(right)) => left >= right,
                    _ => true,
                })
        {
            return Err(InputClass);
        }
        // require every genuine family class and exact non-rain gate absence
        if family == NativeMaintenanceCaptureFamily::Rain || *name != "rain_gate_input" {
            if identities.is_empty() {
                return Err(InputClass);
            }
        } else if !identities.is_empty() {
            return Err(InputClass);
        }
        entries.push((
            *name,
            object(vec![
                ("count", JsonValue::Number(identities.len() as f64)),
                (
                    "rootSha256",
                    string(&canonical_identity(
                        field(members, name).ok_or(InputClass)?,
                    )?),
                ),
            ]),
        ));
    }
    Ok(object(entries))
}

/// Summarize partial unsupported classes and derive the exact required zero-class list.
fn summarize_unsupported_input_classes(
    members: &JsonValue,
    family: NativeMaintenanceCaptureFamily,
) -> Result<(JsonValue, Vec<String>), NativeMaintenanceTerminalSealOwnerError> {
    use NativeMaintenanceTerminalSealOwnerError::InputClass;

    if !exact_keys(members, INPUT_CLASS_NAMES) {
        return Err(InputClass);
    }
    let mut entries = Vec::with_capacity(INPUT_CLASS_NAMES.len());
    let mut missing = Vec::new();
    // retain every genuine partial population without manufacturing absent members
    for name in INPUT_CLASS_NAMES {
        let identities = match field(members, name) {
            Some(JsonValue::Array(identities)) => identities,
            _ => return Err(InputClass),
        };
        if identities.len() > 9_500_000
            || identities
                .iter()
                .any(|identity| json_hash(identity).is_none())
            || identities
                .windows(2)
                .any(|pair| match (&pair[0], &pair[1]) {
                    (JsonValue::String(left), JsonValue::String(right)) => left >= right,
                    _ => true,
                })
        {
            return Err(InputClass);
        }
        // omit only the intentionally empty non-rain gate from missing diagnostics
        if identities.is_empty()
            && (family == NativeMaintenanceCaptureFamily::Rain || *name != "rain_gate_input")
        {
            missing.push((*name).to_owned());
        }
        entries.push((
            *name,
            object(vec![
                ("count", JsonValue::Number(identities.len() as f64)),
                (
                    "rootSha256",
                    string(&canonical_identity(
                        field(members, name).ok_or(InputClass)?,
                    )?),
                ),
            ]),
        ));
    }
    let input_classes = object(entries);
    let candidate_count = class_count(&input_classes, "candidate").ok_or(InputClass)?;
    let artifact_count = class_count(&input_classes, "artifact").ok_or(InputClass)?;
    // preserve one exact restored candidate and at least one genuine artifact identity
    if candidate_count != 1 || artifact_count < 1 {
        return Err(InputClass);
    }
    Ok((input_classes, missing))
}

/// Encode one self-addressed canonical document after its caller completed contract admission.
fn canonical_document(
    value: JsonValue,
    error: NativeMaintenanceTerminalSealOwnerError,
) -> Result<NativeMaintenanceTerminalAuthorityDocument, NativeMaintenanceTerminalSealOwnerError> {
    let canonical_bytes = NativeWirePrimitives
        .encode_sorted_json(&value)
        .ok_or(error)?;
    Ok(NativeMaintenanceTerminalAuthorityDocument {
        sha256: format_digest(NativeWirePrimitives.sha256(&canonical_bytes)),
        value,
        canonical_bytes,
    })
}

/// Admit one constructed unsupported proof through the exact retained proof grammar.
fn admitted_unsupported_proof_document(
    value: JsonValue,
) -> Result<NativeMaintenanceTerminalAuthorityDocument, NativeMaintenanceTerminalSealOwnerError> {
    if !valid_unsupported_proof(&value) {
        return Err(NativeMaintenanceTerminalSealOwnerError::Finalization);
    }
    canonical_document(value, NativeMaintenanceTerminalSealOwnerError::Finalization)
}

/// Admit one constructed seal through the same native contract used by current and install.
fn admitted_seal_document(
    value: JsonValue,
) -> Result<NativeMaintenanceTerminalAuthorityDocument, NativeMaintenanceTerminalSealOwnerError> {
    let bytes = NativeWirePrimitives
        .encode_sorted_json(&value)
        .ok_or(NativeMaintenanceTerminalSealOwnerError::Seal)?;
    parse_future_input_seal(&bytes, &mut NativeWirePrimitives, &NativeWirePrimitives)
        .ok_or(NativeMaintenanceTerminalSealOwnerError::Seal)?;
    Ok(NativeMaintenanceTerminalAuthorityDocument {
        sha256: format_digest(NativeWirePrimitives.sha256(&bytes)),
        value,
        canonical_bytes: bytes,
    })
}

/// Admit one constructed v3 anchor in its required source state.
fn admitted_anchor_document(
    value: JsonValue,
    state: MaintenanceAnchorState,
) -> Result<NativeMaintenanceTerminalAuthorityDocument, NativeMaintenanceTerminalSealOwnerError> {
    let bytes = NativeWirePrimitives
        .encode_sorted_json(&value)
        .ok_or(NativeMaintenanceTerminalSealOwnerError::Anchor)?;
    // keep transferred construction disjoint from finalized stored successors
    if validate_v3_anchor(&value, &mut NativeWirePrimitives, &NativeWirePrimitives) != Some(state) {
        return Err(NativeMaintenanceTerminalSealOwnerError::Anchor);
    }
    Ok(NativeMaintenanceTerminalAuthorityDocument {
        sha256: format_digest(NativeWirePrimitives.sha256(&bytes)),
        value,
        canonical_bytes: bytes,
    })
}

/// Build the external proof by copying every transferred field except version and CTF state.
fn finalization_proof(
    anchor: &NativeMaintenanceTerminalAuthorityDocument,
    finalized_at: &str,
) -> Result<NativeMaintenanceTerminalAuthorityDocument, NativeMaintenanceTerminalSealOwnerError> {
    use NativeMaintenanceTerminalSealOwnerError::Finalization;

    let JsonValue::Object(fields) = &anchor.value else {
        return Err(Finalization);
    };
    let mut proof_fields = fields
        .iter()
        // exclude only the two fields replaced by the external proof contract
        .filter(|(key, _)| {
            !key.iter().copied().eq("contractVersion".encode_utf16())
                && !key.iter().copied().eq("ctfState".encode_utf16())
        })
        .cloned()
        .collect::<Vec<_>>();
    proof_fields.extend([
        (
            "contractVersion".encode_utf16().collect(),
            string("adjustment-maintenance-finalization-proof/v3"),
        ),
        ("finalizedAt".encode_utf16().collect(), string(finalized_at)),
        (
            "transferredAnchorSha256".encode_utf16().collect(),
            string(&anchor.sha256),
        ),
    ]);
    let value = JsonValue::Object(proof_fields);
    let bytes = NativeWirePrimitives
        .encode_sorted_json(&value)
        .ok_or(Finalization)?;
    // rebind every proof primitive and source chronology before exposing the document
    if text(&value, "contractVersion").as_deref()
        != Some("adjustment-maintenance-finalization-proof/v3")
        || text(&value, "transferredAnchorSha256").as_deref() != Some(anchor.sha256.as_str())
        || instant(finalized_at).ok_or(Finalization)?
            < instant(&text(&anchor.value, "publishedAt").ok_or(Finalization)?)
                .ok_or(Finalization)?
    {
        return Err(Finalization);
    }
    Ok(NativeMaintenanceTerminalAuthorityDocument {
        sha256: format_digest(NativeWirePrimitives.sha256(&bytes)),
        value,
        canonical_bytes: bytes,
    })
}

/// Verify one nullable canonical seal current response.
fn verify_seal_current(
    bytes: &[u8],
) -> Result<Option<NativeFutureInputSealCurrent>, NativeMaintenanceTerminalSealOwnerError> {
    use NativeMaintenanceTerminalSealOwnerError::Response;

    let value = canonical_value(bytes).ok_or(Response)?;
    // preserve genuine genesis absence without manufacturing a predecessor sequence
    if value == JsonValue::Null {
        return Ok(None);
    }
    if !exact_keys(&value, SEAL_CURRENT_KEYS) {
        return Err(Response);
    }
    let seal = field(&value, "seal").ok_or(Response)?.clone();
    let seal_bytes = NativeWirePrimitives
        .encode_sorted_json(&seal)
        .ok_or(Response)?;
    parse_future_input_seal(
        &seal_bytes,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .ok_or(Response)?;
    let seal_sha256 = text(&value, "sealSha256").ok_or(Response)?;
    // bind the current wrapper to exact canonical seal bytes
    if seal_sha256 != format_digest(NativeWirePrimitives.sha256(&seal_bytes)) {
        return Err(Response);
    }
    Ok(Some(NativeFutureInputSealCurrent { seal, seal_sha256 }))
}

/// Verify one nullable canonical v3 anchor current response.
fn verify_anchor_current(
    bytes: &[u8],
) -> Result<Option<NativeMaintenanceTerminalAnchorCurrent>, NativeMaintenanceTerminalSealOwnerError>
{
    use NativeMaintenanceTerminalSealOwnerError::Response;

    let value = canonical_value(bytes).ok_or(Response)?;
    // preserve genuine genesis absence without manufacturing an anchor predecessor
    if value == JsonValue::Null {
        return Ok(None);
    }
    if !exact_keys(&value, CURRENT_KEYS) {
        return Err(Response);
    }
    let anchor = field(&value, "anchor").ok_or(Response)?.clone();
    if validate_v3_anchor(&anchor, &mut NativeWirePrimitives, &NativeWirePrimitives).is_none() {
        return Err(Response);
    }
    let anchor_bytes = NativeWirePrimitives
        .encode_sorted_json(&anchor)
        .ok_or(Response)?;
    let anchor_sha256 = text(&value, "anchorSha256").ok_or(Response)?;
    // bind the current wrapper to exact canonical anchor bytes
    if anchor_sha256 != format_digest(NativeWirePrimitives.sha256(&anchor_bytes)) {
        return Err(Response);
    }
    Ok(Some(NativeMaintenanceTerminalAnchorCurrent {
        anchor,
        anchor_sha256,
    }))
}

/// Verify one nullable canonical unsupported proof current response.
fn verify_unsupported_current(
    bytes: &[u8],
) -> Result<Option<NativeUnsupportedTerminalProofCurrent>, NativeMaintenanceTerminalSealOwnerError>
{
    use NativeMaintenanceTerminalSealOwnerError::Response;

    let value = canonical_value(bytes).ok_or(Response)?;
    // preserve genuine genesis absence without manufacturing an unsupported predecessor
    if value == JsonValue::Null {
        return Ok(None);
    }
    if !exact_keys(&value, UNSUPPORTED_CURRENT_KEYS) {
        return Err(Response);
    }
    let proof = field(&value, "proof").ok_or(Response)?.clone();
    if !valid_unsupported_proof(&proof) {
        return Err(Response);
    }
    let proof_bytes = NativeWirePrimitives
        .encode_sorted_json(&proof)
        .ok_or(Response)?;
    let proof_sha256 = text(&value, "proofSha256").ok_or(Response)?;
    // bind the current wrapper to exact canonical unsupported proof bytes
    if proof_sha256 != format_digest(NativeWirePrimitives.sha256(&proof_bytes)) {
        return Err(Response);
    }
    Ok(Some(NativeUnsupportedTerminalProofCurrent {
        proof,
        proof_sha256,
    }))
}

/// Verify one prior unsupported proof and derive its exact successor coordinates.
fn unsupported_predecessor(
    previous: Option<&NativeUnsupportedTerminalProofCurrent>,
) -> Result<(u64, JsonValue), NativeMaintenanceTerminalSealOwnerError> {
    use NativeMaintenanceTerminalSealOwnerError::Predecessor;

    // preserve the source's zero genesis and direct content-addressed predecessor
    match previous {
        None => Ok((0, JsonValue::Null)),
        Some(previous) => {
            if !valid_unsupported_proof(&previous.proof) {
                return Err(Predecessor);
            }
            let bytes = NativeWirePrimitives
                .encode_sorted_json(&previous.proof)
                .ok_or(Predecessor)?;
            if format_digest(NativeWirePrimitives.sha256(&bytes)) != previous.proof_sha256 {
                return Err(Predecessor);
            }
            let sequence = text(&previous.proof, "sequence")
                .and_then(|value| value.parse::<u64>().ok())
                .and_then(|value| value.checked_add(1))
                .ok_or(Predecessor)?;
            Ok((sequence, string(&previous.proof_sha256)))
        }
    }
}

/// Validate the complete unsupported proof grammar retained by both client and root store.
fn valid_unsupported_proof(value: &JsonValue) -> bool {
    if !exact_keys(value, UNSUPPORTED_PROOF_KEYS)
        || text(value, "contractVersion").as_deref()
            != Some("adjustment-maintenance-unsupported-terminal-proof/v1")
    {
        return false;
    }
    let Some(family) = text(value, "family") else {
        return false;
    };
    if !matches!(family.as_str(), "temperature" | "wind" | "rain") {
        return false;
    }
    let Some(due_key) = text(value, "dueKey") else {
        return false;
    };
    let due = due_key.split('/').collect::<Vec<_>>();
    // require the confirmation key to bind the exact family and one candidate identity
    if due.len() != 3 || due[0] != "confirmation" || due[1] != family || !valid_hash(due[2]) {
        return false;
    }
    let chunk_count = class_number(value, "confirmationChunkCount");
    // keep the family cardinality and immutable source identities exact
    if chunk_count != Some(if family == "rain" { 24 } else { 27 })
        || !text(value, "controlVersion").is_some_and(|version| valid_control_version(&version))
        || !text(value, "sourceCommit").is_some_and(|commit| valid_commit(&commit))
        || !text(value, "archiveCommitOrdinal").is_some_and(|ordinal| valid_ordinal(&ordinal))
        || !text(value, "sequence").is_some_and(|ordinal| valid_ordinal(&ordinal))
    {
        return false;
    }
    // require every content-addressed proof field before any cross-binding comparison
    for key in [
        "actionSha256",
        "burnSha256",
        "candidateReportSha256",
        "captureEpochWitnessSha256",
        "confirmationAccessSha256",
        "controlSha256",
        "custodyCheckpointSha256",
        "eligiblePredictionSetSha256",
        "expectedKeySetSha256",
        "frontierSha256",
        "fullMemberRootSha256",
        "graphManifestSha256",
        "lifecycleLedgerRootSha256",
        "missingKeySetSha256",
        "pageSha256",
        "policyReportSha256",
        "registrationSha256",
        "requiredInputRootSha256",
        "workstationJournalHeadSha256",
    ] {
        if !text(value, key).is_some_and(|identity| valid_hash(&identity)) {
            return false;
        }
    }
    let predecessor = field(value, "predecessorProofSha256");
    let predecessor_valid = match predecessor {
        Some(JsonValue::Null) => true,
        Some(candidate) => json_hash(candidate).is_some(),
        None => false,
    };
    let sequence = text(value, "sequence").and_then(|value| value.parse::<u64>().ok());
    // permit only the zero sequence to omit its direct predecessor
    if !predecessor_valid || (sequence == Some(0)) != matches!(predecessor, Some(JsonValue::Null)) {
        return false;
    }
    let finalized_at = text(value, "finalizedAt").and_then(|value| instant(&value));
    let verified_at = text(value, "fullGraphVerifiedAt").and_then(|value| instant(&value));
    // retain exact source clock order across graph verification and terminal completion
    if !matches!((finalized_at, verified_at), (Some(finalized), Some(verified)) if finalized >= verified)
    {
        return false;
    }
    let Some(input_classes @ JsonValue::Object(_)) = field(value, "inputClasses") else {
        return false;
    };
    if !exact_keys(input_classes, INPUT_CLASS_NAMES) {
        return false;
    }
    let empty_root = canonical_identity(&JsonValue::Array(Vec::new())).ok();
    let mut zero_classes = Vec::new();
    // validate every exact partial population and derive actual absent required classes
    for name in INPUT_CLASS_NAMES {
        let Some(entry) = field(input_classes, name) else {
            return false;
        };
        if !exact_keys(entry, &["count", "rootSha256"])
            || !text(entry, "rootSha256").is_some_and(|root| valid_hash(&root))
        {
            return false;
        }
        let Some(count) = class_number(entry, "count") else {
            return false;
        };
        if count > 9_500_000 || (count == 0 && text(entry, "rootSha256") != empty_root) {
            return false;
        }
        // exclude the intentionally absent non-rain gate from missing diagnostics
        if count == 0 && (family == "rain" || *name != "rain_gate_input") {
            zero_classes.push((*name).to_owned());
        }
    }
    let missing_class_names = string_array(field(value, "missingClassNames"));
    let missing_key_count = class_number(value, "missingKeyCount");
    // accept only the three closed permanent source reasons and genuine bounded missing evidence
    if missing_class_names.as_deref() != Some(zero_classes.as_slice())
        || !matches!(missing_key_count, Some(1..=9_500_000))
        || !text(value, "unsupportedReason").is_some_and(|reason| {
            matches!(
                reason.as_str(),
                "permanent_capture_gap" | "permanent_source_gap" | "permanent_target_gap"
            )
        })
    {
        return false;
    }
    let candidate_count = class_count(input_classes, "candidate");
    let artifact_count = class_count(input_classes, "artifact");
    let candidate_root = canonical_identity(&JsonValue::Array(vec![string(due[2])])).ok();
    // require retained candidate/artifact evidence and exact input-root closure
    if candidate_count != Some(1)
        || !artifact_count.is_some_and(|count| count >= 1)
        || class_root(input_classes, "candidate").ok() != candidate_root
        || canonical_identity(input_classes).ok() != text(value, "requiredInputRootSha256")
    {
        return false;
    }
    let Some(policy_report_sha256) = text(value, "policyReportSha256") else {
        return false;
    };
    let Some(registration_sha256) = text(value, "registrationSha256") else {
        return false;
    };
    let identity = object(vec![
        (
            "contractVersion",
            string("adjustment-terminal-no-action-identity/v3"),
        ),
        ("disposition", string("support_failed")),
        ("policyReportSha256", string(&policy_report_sha256)),
        ("registrationSha256", string(&registration_sha256)),
    ]);
    // close the proof to the exact no-action identity and one unsupported policy report
    canonical_identity(&identity).ok() == text(value, "actionSha256")
        && text(value, "candidateReportSha256") == Some(policy_report_sha256)
}

/// Re-run full canonical ACK-v2 admission against its retained request coordinates.
fn reverify_acknowledgement(
    acknowledgement: &VerifiedNativeRevisionColdCustodyAcknowledgementV2,
) -> Result<
    VerifiedNativeRevisionColdCustodyAcknowledgementV2,
    NativeMaintenanceTerminalSealOwnerError,
> {
    let verified = verify_native_revision_cold_custody_acknowledgement_v2_document(
        &acknowledgement.canonical_bytes,
        &acknowledgement.request,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .map_err(|_| NativeMaintenanceTerminalSealOwnerError::Custody)?;
    // reject a publicly constructible wrapper that diverges from its canonical ACK
    if &verified != acknowledgement {
        return Err(NativeMaintenanceTerminalSealOwnerError::Custody);
    }
    Ok(verified)
}

/// Verify one prior seal and derive its exact successor coordinates.
fn seal_predecessor(
    previous: Option<&NativeFutureInputSealCurrent>,
) -> Result<(u64, JsonValue), NativeMaintenanceTerminalSealOwnerError> {
    use NativeMaintenanceTerminalSealOwnerError::Predecessor;

    // preserve the source's zero genesis and direct content-addressed predecessor
    match previous {
        None => Ok((0, JsonValue::Null)),
        Some(previous) => {
            let bytes = NativeWirePrimitives
                .encode_sorted_json(&previous.seal)
                .ok_or(Predecessor)?;
            parse_future_input_seal(&bytes, &mut NativeWirePrimitives, &NativeWirePrimitives)
                .ok_or(Predecessor)?;
            if format_digest(NativeWirePrimitives.sha256(&bytes)) != previous.seal_sha256 {
                return Err(Predecessor);
            }
            let sequence = text(&previous.seal, "sequence")
                .and_then(|value| value.parse::<u64>().ok())
                .and_then(|value| value.checked_add(1))
                .ok_or(Predecessor)?;
            Ok((sequence, string(&previous.seal_sha256)))
        }
    }
}

/// Verify one prior anchor and return its exact content predecessor identity.
fn anchor_predecessor(
    previous: Option<&NativeMaintenanceTerminalAnchorCurrent>,
) -> Result<JsonValue, NativeMaintenanceTerminalSealOwnerError> {
    use NativeMaintenanceTerminalSealOwnerError::Predecessor;

    let Some(previous) = previous else {
        return Ok(JsonValue::Null);
    };
    let bytes = NativeWirePrimitives
        .encode_sorted_json(&previous.anchor)
        .ok_or(Predecessor)?;
    // admit either transferred or finalized current authority without weakening its identity
    if validate_v3_anchor(
        &previous.anchor,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .is_none()
        || format_digest(NativeWirePrimitives.sha256(&bytes)) != previous.anchor_sha256
    {
        return Err(Predecessor);
    }
    Ok(string(&previous.anchor_sha256))
}

/// Return the complete-only family, rain gate, and terminal graph projection.
fn complete_assembly(
    material: &NativeMaintenanceInstalledTerminalMaterial,
) -> Result<
    (NativeMaintenanceCaptureFamily, &[String], &JsonValue),
    NativeMaintenanceTerminalSealOwnerError,
> {
    // keep future-only C/T/F authority disjoint from pending and unsupported evidence
    match &material.assembly {
        NativeMaintenanceDailyTerminalValues::Complete {
            family,
            rain_gate_input_member_sha256s,
            terminal_graph,
            ..
        } => Ok((*family, rain_gate_input_member_sha256s, terminal_graph)),
        NativeMaintenanceDailyTerminalValues::Pending { .. }
        | NativeMaintenanceDailyTerminalValues::Unsupported { .. } => {
            Err(NativeMaintenanceTerminalSealOwnerError::Material)
        }
    }
}

/// Return the family, rain gate, and terminal graph for either terminal evidence state.
fn terminal_assembly(
    material: &NativeMaintenanceInstalledTerminalMaterial,
) -> Result<
    (NativeMaintenanceCaptureFamily, &[String], &JsonValue),
    NativeMaintenanceTerminalSealOwnerError,
> {
    // expose only genuine terminal assemblies and keep retryable pending material excluded
    match &material.assembly {
        NativeMaintenanceDailyTerminalValues::Complete {
            family,
            rain_gate_input_member_sha256s,
            terminal_graph,
            ..
        }
        | NativeMaintenanceDailyTerminalValues::Unsupported {
            family,
            rain_gate_input_member_sha256s,
            terminal_graph,
            ..
        } => Ok((*family, rain_gate_input_member_sha256s, terminal_graph)),
        NativeMaintenanceDailyTerminalValues::Pending { .. } => {
            Err(NativeMaintenanceTerminalSealOwnerError::Material)
        }
    }
}

/// Rebuild the compact terminal partition manifest and bind it to the verified archive head.
fn verified_terminal_graph_manifest_identity(
    family: NativeMaintenanceCaptureFamily,
    terminal_graph: &JsonValue,
    verified: &NativeMaintenanceVerifiedFullGraph,
) -> Result<String, NativeMaintenanceTerminalSealOwnerError> {
    let partition = build_native_maintenance_terminal_graph_parts(family, terminal_graph)
        .map_err(|_| NativeMaintenanceTerminalSealOwnerError::TerminalGraph)?;
    let identity = required_hash(
        &partition.manifest,
        "terminalGraphManifestSha256",
        NativeMaintenanceTerminalSealOwnerError::TerminalGraph,
    )?;
    // require the exact compact manifest kind and self-identity in the actual verified graph
    if !verified.manifest.entries.iter().any(|entry| {
        entry.kind == "adjustment-maintenance-confirmation-terminal-graph-manifest/v1"
            && entry.identity_sha256 == identity
    }) {
        return Err(NativeMaintenanceTerminalSealOwnerError::TerminalGraph);
    }
    Ok(identity)
}

/// Classify the exact permanent unsupported reason from absent semantic classes.
fn classify_unsupported_reason(missing_class_names: &[String]) -> &'static str {
    // capture inputs take precedence over target gaps exactly as the predecessor classifier does
    if missing_class_names.iter().any(|name| {
        matches!(
            name.as_str(),
            "comparator" | "shadow_body" | "shadow_source"
        )
    }) {
        "permanent_capture_gap"
    } else if missing_class_names
        .iter()
        .any(|name| matches!(name.as_str(), "target" | "target_revision"))
    {
        "permanent_target_gap"
    } else {
        "permanent_source_gap"
    }
}

/// Insert one exact binding hash into its closed class set.
fn insert_hash(
    classes: &mut std::collections::BTreeMap<&str, BTreeSet<String>>,
    class: &'static str,
    value: &JsonValue,
    key: &str,
) -> Result<(), NativeMaintenanceTerminalSealOwnerError> {
    let identity = required_hash(
        value,
        key,
        NativeMaintenanceTerminalSealOwnerError::TerminalGraph,
    )?;
    classes
        .get_mut(class)
        .ok_or(NativeMaintenanceTerminalSealOwnerError::InputClass)?
        .insert(identity);
    Ok(())
}

/// Admit one caller list as unique hashes without silently sorting invalid duplicates.
fn admitted_hashes(
    values: &[String],
    error: NativeMaintenanceTerminalSealOwnerError,
) -> Result<BTreeSet<String>, NativeMaintenanceTerminalSealOwnerError> {
    let admitted = values.iter().cloned().collect::<BTreeSet<_>>();
    // reject duplicate or malformed source identities before set projection
    if admitted.len() != values.len() || admitted.iter().any(|value| !valid_hash(value)) {
        return Err(error);
    }
    Ok(admitted)
}

/// Decode one required exact hash from a closed object.
fn required_hash(
    value: &JsonValue,
    key: &str,
    error: NativeMaintenanceTerminalSealOwnerError,
) -> Result<String, NativeMaintenanceTerminalSealOwnerError> {
    let value = text(value, key).ok_or(error)?;
    // reject aliases and alternate hash spellings before graph use
    if !valid_hash(&value) {
        return Err(error);
    }
    Ok(value)
}

/// Decode one exact hash JSON string.
fn json_hash(value: &JsonValue) -> Option<String> {
    let JsonValue::String(units) = value else {
        return None;
    };
    let value = String::from_utf16(units).ok()?;
    valid_hash(&value).then_some(value)
}

/// Decode one bounded nonnegative JSON integer used by source count contracts.
fn class_number(value: &JsonValue, key: &str) -> Option<usize> {
    match field(value, key) {
        Some(JsonValue::Number(number))
            if number.is_finite()
                && *number >= 0.0
                && *number <= usize::MAX as f64
                && number.fract() == 0.0 =>
        {
            Some(*number as usize)
        }
        _ => None,
    }
}

/// Read one summarized class count from the closed input-class map.
fn class_count(input_classes: &JsonValue, name: &str) -> Option<usize> {
    class_number(field(input_classes, name)?, "count")
}

/// Decode one exact JSON string array without coercion or normalization.
fn string_array(value: Option<&JsonValue>) -> Option<Vec<String>> {
    let JsonValue::Array(values) = value? else {
        return None;
    };
    values
        .iter()
        .map(|value| match value {
            JsonValue::String(units) => String::from_utf16(units).ok(),
            _ => None,
        })
        .collect()
}

/// Copy one required source scalar without normalizing its representation.
fn copied(
    value: &JsonValue,
    key: &str,
) -> Result<JsonValue, NativeMaintenanceTerminalSealOwnerError> {
    field(value, key)
        .cloned()
        .ok_or(NativeMaintenanceTerminalSealOwnerError::Anchor)
}

/// Read one summarized input-class root.
fn class_root(
    input_classes: &JsonValue,
    name: &str,
) -> Result<String, NativeMaintenanceTerminalSealOwnerError> {
    field(input_classes, name)
        .and_then(|entry| text(entry, "rootSha256"))
        .filter(|value| valid_hash(value))
        .ok_or(NativeMaintenanceTerminalSealOwnerError::InputClass)
}

/// Hash one canonical JSON value through the approved native primitive.
fn canonical_identity(
    value: &JsonValue,
) -> Result<String, NativeMaintenanceTerminalSealOwnerError> {
    let bytes = NativeWirePrimitives
        .encode_sorted_json(value)
        .ok_or(NativeMaintenanceTerminalSealOwnerError::Material)?;
    Ok(format_digest(NativeWirePrimitives.sha256(&bytes)))
}

/// Parse one exact canonical JSON response of any source shape.
fn canonical_value(bytes: &[u8]) -> Option<JsonValue> {
    let value = NativeWirePrimitives.decode_json(bytes)?;
    (NativeWirePrimitives.encode_sorted_json(&value)?.as_slice() == bytes).then_some(value)
}

/// Recover one canonical portable artifact's self-declared bundle identity.
fn canonical_artifact_identity(bytes: &[u8]) -> Option<String> {
    let value = canonical_value(bytes)?;
    text(&value, "bundleSha256").filter(|identity| valid_hash(identity))
}

/// Parse one canonical UTC instant to its sortable epoch value.
fn instant(value: &str) -> Option<i64> {
    NativeWirePrimitives.canonical_utc_milliseconds(value)
}

/// Recognize one exact lowercase forty-character source commit.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Recognize one exact source decimal archive ordinal without leading aliases.
fn valid_ordinal(value: &str) -> bool {
    value == "0"
        || (!value.starts_with('0')
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && value.parse::<u64>().is_ok())
}

/// Recognize one exact positive bounded control version.
fn valid_control_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 6
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
#[path = "native_maintenance_terminal_seal_owner_test.rs"]
mod tests;
