//! Verified development custody anchoring and acknowledged journal binding.

use std::fmt::{self, Display, Formatter};
use std::io;

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::custody_anchor_current::CustodyAnchorKind;
use crate::custody_anchor_validation::validate_custody_anchor;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_development_remote::{
    NativeMaintenanceDevelopmentAnchorInstallation, NativeMaintenanceFamilyReleaseApplyRequest,
    NativeMaintenanceFamilyReleaseApplyState, NativeMaintenanceFamilyReleaseApplyStatus,
    build_native_maintenance_development_anchor_install_request,
};
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    MaintenanceCanonicalJson, NativeFamilyReleaseCurrent, NativeMaintenanceCaptureFamily,
    NativeMaintenanceDatabaseLedgerReader, NativeMaintenanceDevelopmentCandidateInstalled,
    NativeMaintenanceDevelopmentPublication, NativeMaintenanceDue,
    NativeMaintenanceDueAttemptJournal, NativeMaintenanceDueAttemptJournalError,
    NativeMaintenanceJournalStore, NativeMaintenanceJournalStoreError,
    NativeMaintenanceRecordDevelopmentCandidateInstalled, NativeMaintenanceVerifiedFullGraph,
    NativeRevisionColdCustodyRequest, NativeRevisionColdPageRequest, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness,
    VerifiedNativeRevisionColdCustodyAcknowledgementV2, verify_native_maintenance_due,
    verify_native_revision_cold_custody_acknowledgement_v2_document,
};

pub const NATIVE_MAINTENANCE_DEVELOPMENT_ANCHOR_CURRENT_COMMAND: &str =
    "adjustment-development-custody-anchor-current-v1";

const ANCHOR_CURRENT_KEYS: &[&str] = &["anchor", "anchorSha256"];
const CUSTODY_BINDING_KEYS: &[&str] = &[
    "actionSha256",
    "artifactSha256",
    "candidateGraphSha256",
    "candidateSha256",
    "contractVersion",
    "policyReportSha256",
    "registrationSha256",
    "sourceSha256",
];
const CUSTODY_BINDING_VERSION: &str = "adjustment-development-custody-graph/v1";
const CUSTODY_COMPLETED_V2_KIND: &str = "revision_custody_completed_v2";

/// Verified current development anchor and its content identity.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDevelopmentAnchorCurrent {
    pub anchor: JsonValue,
    pub anchor_sha256: String,
    pub sequence: u64,
}

/// Exact constructed anchor plus the identities needed by the later journal acknowledgement.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDevelopmentAnchor {
    pub anchor: JsonValue,
    pub canonical_bytes: Vec<u8>,
    pub anchor_sha256: String,
    pub action_sha256: String,
    pub artifact_sha256: String,
    pub candidate_graph_sha256: String,
    pub candidate_sha256: String,
    pub confirmation_registration_sha256: String,
    pub family: NativeMaintenanceCaptureFamily,
    pub policy_report_sha256: String,
    pub shadow_registration_sha256: String,
    pub source_commit: String,
    pub source_release: String,
}

/// Borrowed verified inputs for the source development custody anchor transition.
pub struct NativeMaintenanceDevelopmentAnchorInput<'a> {
    pub acknowledgement: &'a VerifiedNativeRevisionColdCustodyAcknowledgementV2,
    pub current_release: &'a NativeFamilyReleaseCurrent,
    pub development_graph: &'a NativeMaintenanceVerifiedFullGraph,
    pub due: &'a NativeMaintenanceDue,
    pub epoch_witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    pub full_graph_verified_at: &'a str,
    pub input_head_sha256: &'a str,
    pub lifecycle_head_sha256: &'a str,
    pub previous: Option<&'a NativeMaintenanceDevelopmentAnchorCurrent>,
    pub publication: &'a NativeMaintenanceDevelopmentPublication,
}

/// A completed install and family apply transaction before its local replay receipt is appended.
pub struct NativeMaintenanceDevelopmentAcknowledgementInput<'a> {
    pub anchor: &'a NativeMaintenanceDevelopmentAnchor,
    pub apply_request: &'a NativeMaintenanceFamilyReleaseApplyRequest,
    pub apply_status: &'a NativeMaintenanceFamilyReleaseApplyStatus,
    pub installation: &'a NativeMaintenanceDevelopmentAnchorInstallation,
    pub now: &'a str,
}

/// Closed construction, remote admission, or durable journal refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDevelopmentAnchorError {
    Input,
    Publication,
    Graph,
    Acknowledgement,
    Current,
    Clock,
    Anchor,
    Release,
    Transport,
    Journal(NativeMaintenanceJournalStoreError),
    Receipt(NativeMaintenanceDueAttemptJournalError),
}

impl Display for NativeMaintenanceDevelopmentAnchorError {
    /// Render one stable error without reflecting retained release material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve the owning boundary while keeping transport diagnostics closed
        formatter.write_str(match self {
            Self::Input => "development custody anchor input is invalid",
            Self::Publication => "development custody publication differs",
            Self::Graph => "development custody graph is unavailable",
            Self::Acknowledgement => "development custody acknowledgement is unavailable",
            Self::Current => "development custody predecessor is invalid",
            Self::Clock => "development custody anchor clock is invalid",
            Self::Anchor => "development custody anchor is invalid",
            Self::Release => "development release acknowledgement differs",
            Self::Transport => "development custody current transport failed",
            Self::Journal(_) => "development custody journal read failed",
            Self::Receipt(_) => "development custody receipt append failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceDevelopmentAnchorError {
    /// Retain only actual durable-store causes.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // semantic refusals and closed remote errors have no exposed source
        match self {
            Self::Journal(error) => Some(error),
            Self::Receipt(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceDevelopmentAnchorError {
    /// Preserve one actual journal verification failure.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Journal(error)
    }
}

/// Fixed read-only remote operation for the sole retained development anchor predecessor.
pub trait NativeMaintenanceDevelopmentAnchorCurrentPort {
    /// Read the zero-operand current projection without selecting another command.
    fn read_development_anchor_current(&mut self) -> io::Result<Vec<u8>>;
}

impl NativeMaintenanceDevelopmentAnchorCurrentPort for NativeMaintenanceDatabaseLedgerReader {
    /// Reuse the authenticated fixed-command reader with no caller-selected operands.
    fn read_development_anchor_current(&mut self) -> io::Result<Vec<u8>> {
        self.request_document(
            NATIVE_MAINTENANCE_DEVELOPMENT_ANCHOR_CURRENT_COMMAND,
            &[],
            None,
        )
    }
}

/// Read and verify the sole remote development-anchor predecessor.
pub fn read_native_maintenance_development_anchor_current(
    remote: &mut impl NativeMaintenanceDevelopmentAnchorCurrentPort,
) -> Result<
    Option<NativeMaintenanceDevelopmentAnchorCurrent>,
    NativeMaintenanceDevelopmentAnchorError,
> {
    let bytes = remote
        .read_development_anchor_current()
        .map_err(|_| NativeMaintenanceDevelopmentAnchorError::Transport)?;
    verify_native_maintenance_development_anchor_current_document(&bytes)
}

/// Verify one canonical nullable current-anchor response and its content address.
pub fn verify_native_maintenance_development_anchor_current_document(
    bytes: &[u8],
) -> Result<
    Option<NativeMaintenanceDevelopmentAnchorCurrent>,
    NativeMaintenanceDevelopmentAnchorError,
> {
    use NativeMaintenanceDevelopmentAnchorError::Current;

    let value = MaintenanceCanonicalJson::parse_canonical_object(&mut NativeWirePrimitives, bytes)
        .ok_or(Current)?;
    // preserve genuine create-once absence without inventing a sequence
    if value == JsonValue::Null {
        return Ok(None);
    }
    if !exact_keys(&value, ANCHOR_CURRENT_KEYS) {
        return Err(Current);
    }
    let anchor = field(&value, "anchor").ok_or(Current)?.clone();
    validate_custody_anchor(
        CustodyAnchorKind::Development,
        &anchor,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .ok_or(Current)?;
    let canonical_bytes =
        MaintenanceCanonicalJson::encode_canonical_json(&mut NativeWirePrimitives, &anchor)
            .ok_or(Current)?;
    let anchor_sha256 = text(&value, "anchorSha256").ok_or(Current)?;
    let actual = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &canonical_bytes,
    ));
    let sequence = text(&anchor, "sequence")
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(Current)?;
    // bind the wrapper to the exact validated canonical anchor bytes
    if !valid_hash(&anchor_sha256) || anchor_sha256 != actual {
        return Err(Current);
    }
    Ok(Some(NativeMaintenanceDevelopmentAnchorCurrent {
        anchor,
        anchor_sha256,
        sequence,
    }))
}

/// Replay the source's last completed-v2 projection and validate the full ACK document.
pub fn read_latest_native_maintenance_revision_custody_acknowledgement(
    journal: &mut NativeMaintenanceJournalStore,
) -> Result<
    Option<VerifiedNativeRevisionColdCustodyAcknowledgementV2>,
    NativeMaintenanceDevelopmentAnchorError,
> {
    use NativeMaintenanceDevelopmentAnchorError::Acknowledgement;

    let records = journal.read_verified(&mut NativeWirePrimitives, &NativeWirePrimitives)?;
    let Some(record) = records
        .iter()
        .rev()
        .find(|record| journal_kind(record, CUSTODY_COMPLETED_V2_KIND))
    else {
        return Ok(None);
    };
    let value = field(&record.payload, "acknowledgement")
        .ok_or(Acknowledgement)?
        .clone();
    let canonical_bytes =
        MaintenanceCanonicalJson::encode_canonical_json(&mut NativeWirePrimitives, &value)
            .ok_or(Acknowledgement)?;
    let request = custody_request_from_acknowledgement(&value).ok_or(Acknowledgement)?;
    let verified = verify_native_revision_cold_custody_acknowledgement_v2_document(
        &canonical_bytes,
        &request,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .map_err(|_| Acknowledgement)?;
    Ok(Some(verified))
}

/// Construct the exact source anchor from verified graph, custody, release, and predecessor inputs.
pub fn build_native_maintenance_development_anchor(
    input: NativeMaintenanceDevelopmentAnchorInput<'_>,
) -> Result<NativeMaintenanceDevelopmentAnchor, NativeMaintenanceDevelopmentAnchorError> {
    use NativeMaintenanceDevelopmentAnchorError as Error;

    let family = input.due.family.ok_or(Error::Input)?;
    // require the source monthly due and its exact family before projecting any authority
    if !verify_native_maintenance_due(input.due)
        || input.due.mode.as_str() != "monthly"
        || input.current_release.family != family.wire()
        || input.current_release.contract_version != "adjustment-family-release-current-status/v1"
        || !valid_commit(&input.current_release.commit)
        || !valid_release(&input.current_release.release)
    {
        return Err(Error::Input);
    }
    let binding = admit_publication(input.publication, family)?;
    verify_development_graph(input.publication, input.development_graph)?;
    let acknowledgement = reverify_acknowledgement(input.acknowledgement)?;
    let full_graph_verified_at = NativeWirePrimitives
        .canonical_utc_milliseconds(input.full_graph_verified_at)
        .ok_or(Error::Clock)?;
    let acknowledged_at = text(&acknowledgement.value, "acknowledgedAt")
        .and_then(|value| NativeWirePrimitives.canonical_utc_milliseconds(&value))
        .ok_or(Error::Acknowledgement)?;
    // keep the graph verification at or after the retained custody acknowledgement
    if full_graph_verified_at < acknowledged_at
        || !valid_hash(input.input_head_sha256)
        || !valid_hash(input.lifecycle_head_sha256)
        || !valid_hash(&input.epoch_witness.witness_sha256)
        || !valid_hash(&input.epoch_witness.control_plane_sha256)
        || !valid_control_version(&input.epoch_witness.control_plane_version)
    {
        return Err(Error::Clock);
    }
    let (predecessor, sequence) = next_predecessor(input.previous)?;
    let development_graph_sha256 = format_digest(input.development_graph.manifest_object_sha256);
    let anchor = object(vec![
        ("actionKind", string("shadow")),
        ("actionSha256", string(&binding.action_sha256)),
        (
            "archiveCommitOrdinal",
            string(&acknowledgement.next_archive_commit_ordinal),
        ),
        ("artifactSha256", string(&binding.artifact_sha256)),
        (
            "candidateGraphSha256",
            string(&binding.candidate_graph_sha256),
        ),
        ("candidateSha256", string(&binding.candidate_sha256)),
        (
            "captureEpochWitnessSha256",
            string(&input.epoch_witness.witness_sha256),
        ),
        (
            "contractVersion",
            string("adjustment-development-custody-anchor/v1"),
        ),
        (
            "controlSha256",
            string(&input.epoch_witness.control_plane_sha256),
        ),
        (
            "controlVersion",
            string(&input.epoch_witness.control_plane_version),
        ),
        (
            "custodyAcknowledgedAt",
            string(&text(&acknowledgement.value, "acknowledgedAt").ok_or(Error::Acknowledgement)?),
        ),
        (
            "custodyAcknowledgementSha256",
            string(
                &text(&acknowledgement.value, "acknowledgementSha256")
                    .ok_or(Error::Acknowledgement)?,
            ),
        ),
        (
            "custodyCheckpointSha256",
            string(&acknowledgement.request.custody_checkpoint_sha256),
        ),
        ("developmentGraphSha256", string(&development_graph_sha256)),
        ("dueKey", string(&input.due.due_key)),
        ("family", string(family.wire())),
        (
            "frontierSha256",
            string(&acknowledgement.next_frontier_sha256),
        ),
        ("fullGraphVerifiedAt", string(input.full_graph_verified_at)),
        ("inputHeadSha256", string(input.input_head_sha256)),
        (
            "lifecycleLedgerRootSha256",
            string(input.lifecycle_head_sha256),
        ),
        (
            "memberRootSha256",
            string(&acknowledgement.request.member_root_sha256),
        ),
        ("pageSha256", string(&acknowledgement.request.page_sha256)),
        ("policyReportSha256", string(&binding.policy_report_sha256)),
        ("predecessorAnchorSha256", predecessor),
        ("registrationSha256", string(&binding.registration_sha256)),
        ("sequence", string(&sequence.to_string())),
        ("sourceCommit", string(&input.current_release.commit)),
        ("sourceSha256", string(&binding.source_sha256)),
        (
            "startMemberSha256",
            field(&acknowledgement.value, "startMemberSha256")
                .ok_or(Error::Acknowledgement)?
                .clone(),
        ),
        (
            "startSha256",
            string(&acknowledgement.request.page.start_sha256),
        ),
    ]);
    let request = build_native_maintenance_development_anchor_install_request(&anchor)
        .map_err(|_| Error::Anchor)?;
    Ok(NativeMaintenanceDevelopmentAnchor {
        anchor: request.anchor,
        canonical_bytes: request.canonical_bytes,
        anchor_sha256: request.anchor_sha256,
        action_sha256: binding.action_sha256,
        artifact_sha256: binding.artifact_sha256,
        candidate_graph_sha256: binding.candidate_graph_sha256,
        candidate_sha256: binding.candidate_sha256,
        confirmation_registration_sha256: input
            .publication
            .confirmation_registration_sha256
            .clone(),
        family,
        policy_report_sha256: binding.policy_report_sha256,
        shadow_registration_sha256: binding.registration_sha256,
        source_commit: input.current_release.commit.clone(),
        source_release: input.current_release.release.clone(),
    })
}

/// Append the exact source replay receipt only after both remote acknowledgements bind.
pub fn verify_native_maintenance_development_acknowledgement(
    input: &NativeMaintenanceDevelopmentAcknowledgementInput<'_>,
) -> Result<NativeMaintenanceDevelopmentCandidateInstalled, NativeMaintenanceDevelopmentAnchorError>
{
    use NativeMaintenanceDevelopmentAnchorError as Error;

    let rebuilt = build_native_maintenance_development_anchor_install_request(&input.anchor.anchor)
        .map_err(|_| Error::Anchor)?;
    let arguments = input.apply_request.arguments();
    // bind canonical anchor bytes, installation identity, and all release request identities
    if rebuilt.canonical_bytes != input.anchor.canonical_bytes
        || rebuilt.anchor_sha256 != input.anchor.anchor_sha256
        || input.installation.anchor_sha256 != input.anchor.anchor_sha256
        || input.apply_request.action_sha256() != input.anchor.action_sha256
        || arguments[0] != input.apply_status.target_release
        || arguments[1] != input.apply_status.compensating_release
        || arguments[2] != input.anchor.source_release
        || arguments[3] != input.anchor.source_release
        || arguments[5] != input.anchor.family.wire()
        || arguments[6] != input.anchor.action_sha256
        || arguments[7] != input.anchor.policy_report_sha256
    {
        return Err(Error::Release);
    }
    // admit only the source's root-acknowledged inactive family transaction
    if input.apply_status.state != NativeMaintenanceFamilyReleaseApplyState::Acknowledged
        || input.apply_status.action_sha256 != input.anchor.action_sha256
        || input.apply_status.family.as_deref() != Some(input.anchor.family.wire())
        || input.apply_status.target_release == input.anchor.source_release
        || input.apply_status.compensating_release == input.anchor.source_release
    {
        return Err(Error::Release);
    }
    Ok(NativeMaintenanceDevelopmentCandidateInstalled {
        action_sha256: input.anchor.action_sha256.clone(),
        artifact_sha256: input.anchor.artifact_sha256.clone(),
        candidate_graph_sha256: input.anchor.candidate_graph_sha256.clone(),
        candidate_sha256: input.anchor.candidate_sha256.clone(),
        family: input.anchor.family,
        registration_sha256: input.anchor.confirmation_registration_sha256.clone(),
        shadow_registration_sha256: input.anchor.shadow_registration_sha256.clone(),
    })
}

/// Append the already verified source replay receipt through the existing journal owner.
pub fn record_native_maintenance_development_acknowledgement(
    journal: &mut NativeMaintenanceJournalStore,
    input: NativeMaintenanceDevelopmentAcknowledgementInput<'_>,
) -> Result<NativeMaintenanceDevelopmentCandidateInstalled, NativeMaintenanceDevelopmentAnchorError>
{
    use NativeMaintenanceDevelopmentAnchorError::Receipt;

    let installed = verify_native_maintenance_development_acknowledgement(&input)?;
    NativeMaintenanceDueAttemptJournal::new(journal)
        .record_development_candidate_installed(
            NativeMaintenanceRecordDevelopmentCandidateInstalled {
                action_sha256: &installed.action_sha256,
                artifact_sha256: &installed.artifact_sha256,
                candidate_graph_sha256: &installed.candidate_graph_sha256,
                candidate_sha256: &installed.candidate_sha256,
                family: installed.family,
                now: input.now,
                registration_sha256: &installed.registration_sha256,
                shadow_registration_sha256: &installed.shadow_registration_sha256,
            },
        )
        .map_err(Receipt)
}

/// Closed publication identities already proven by the source custody graph builder.
struct AdmittedPublicationBinding {
    action_sha256: String,
    artifact_sha256: String,
    candidate_graph_sha256: String,
    candidate_sha256: String,
    policy_report_sha256: String,
    registration_sha256: String,
    source_sha256: String,
}

/// Rebind the public action and custody document to their exact canonical bytes.
fn admit_publication(
    publication: &NativeMaintenanceDevelopmentPublication,
    family: NativeMaintenanceCaptureFamily,
) -> Result<AdmittedPublicationBinding, NativeMaintenanceDevelopmentAnchorError> {
    use NativeMaintenanceDevelopmentAnchorError::Publication;

    let wire = NativeWirePrimitives;
    let action_bytes = wire
        .encode_sorted_json(&publication.action)
        .ok_or(Publication)?;
    let binding_bytes = wire
        .encode_sorted_json(&publication.custody_binding)
        .ok_or(Publication)?;
    // prevent a caller from changing any typed publication field after construction
    if action_bytes != publication.action_bytes
        || format_digest(wire.sha256(&action_bytes)) != publication.action_sha256
        || binding_bytes != publication.custody_binding_bytes
        || format_digest(wire.sha256(&binding_bytes)) != publication.custody_binding_sha256
        || !valid_hash(&publication.confirmation_registration_sha256)
        || !exact_keys(&publication.custody_binding, CUSTODY_BINDING_KEYS)
        || text(&publication.custody_binding, "contractVersion").as_deref()
            != Some(CUSTODY_BINDING_VERSION)
    {
        return Err(Publication);
    }
    let binding = AdmittedPublicationBinding {
        action_sha256: required_hash(&publication.custody_binding, "actionSha256")?,
        artifact_sha256: required_hash(&publication.custody_binding, "artifactSha256")?,
        candidate_graph_sha256: required_hash(
            &publication.custody_binding,
            "candidateGraphSha256",
        )?,
        candidate_sha256: required_hash(&publication.custody_binding, "candidateSha256")?,
        policy_report_sha256: required_hash(&publication.custody_binding, "policyReportSha256")?,
        registration_sha256: required_hash(&publication.custody_binding, "registrationSha256")?,
        source_sha256: required_hash(&publication.custody_binding, "sourceSha256")?,
    };
    // crossbind the action and immutable custody binding exactly as the source publisher does
    if binding.action_sha256 != publication.action_sha256
        || text(&publication.action, "candidateGraphSha256").as_deref()
            != Some(binding.candidate_graph_sha256.as_str())
        || text(&publication.action, "candidateSha256").as_deref()
            != Some(binding.candidate_sha256.as_str())
        || text(&publication.action, "policyReportSha256").as_deref()
            != Some(binding.policy_report_sha256.as_str())
        || text(&publication.action, "family").as_deref() != Some(family.wire())
    {
        return Err(Publication);
    }
    Ok(binding)
}

/// Require every custody segment member inside the verified published head graph.
fn verify_development_graph(
    publication: &NativeMaintenanceDevelopmentPublication,
    graph: &NativeMaintenanceVerifiedFullGraph,
) -> Result<(), NativeMaintenanceDevelopmentAnchorError> {
    use NativeMaintenanceDevelopmentAnchorError::Graph;

    if publication.custody_segment.members.is_empty() {
        return Err(Graph);
    }
    // bind every exact source segment member, not only its leading custody identity
    for member in &publication.custody_segment.members {
        let identity = format_digest(member.identity_sha256);
        let retained = graph
            .manifest
            .entries
            .iter()
            .any(|entry| entry.identity_sha256 == identity && entry.kind == member.kind);
        // reject a hash-only graph that omits or relabels one publication member
        if !retained {
            return Err(Graph);
        }
    }
    Ok(())
}

/// Re-run full canonical acknowledgement admission against its archived request coordinates.
fn reverify_acknowledgement(
    acknowledgement: &VerifiedNativeRevisionColdCustodyAcknowledgementV2,
) -> Result<
    VerifiedNativeRevisionColdCustodyAcknowledgementV2,
    NativeMaintenanceDevelopmentAnchorError,
> {
    use NativeMaintenanceDevelopmentAnchorError::Acknowledgement;

    let verified = verify_native_revision_cold_custody_acknowledgement_v2_document(
        &acknowledgement.canonical_bytes,
        &acknowledgement.request,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .map_err(|_| Acknowledgement)?;
    // prevent a publicly constructible typed wrapper from diverging from its canonical bytes
    if &verified != acknowledgement {
        return Err(Acknowledgement);
    }
    Ok(verified)
}

/// Project the first or successor anchor sequence with exact overflow refusal.
fn next_predecessor(
    previous: Option<&NativeMaintenanceDevelopmentAnchorCurrent>,
) -> Result<(JsonValue, u64), NativeMaintenanceDevelopmentAnchorError> {
    use NativeMaintenanceDevelopmentAnchorError::Current;

    // preserve the source's create-once zero boundary and exact content predecessor
    match previous {
        None => Ok((JsonValue::Null, 0)),
        Some(previous) => {
            let sequence = previous.sequence.checked_add(1).ok_or(Current)?;
            Ok((string(&previous.anchor_sha256), sequence))
        }
    }
}

/// Recover the exact archived request projection embedded in one completed ACK-v2.
fn custody_request_from_acknowledgement(
    value: &JsonValue,
) -> Option<NativeRevisionColdCustodyRequest> {
    let start_member_sha256 = text(value, "startMemberSha256")?;
    Some(NativeRevisionColdCustodyRequest {
        page: NativeRevisionColdPageRequest {
            after_archive_commit_ordinal: text(value, "afterArchiveCommitOrdinal")?,
            after_frontier_sha256: text(value, "afterFrontierSha256")?,
            previous_page_sha256: text(value, "previousPageSha256")?,
            start_sha256: text(value, "startSha256")?,
            watermark_archive_commit_ordinal: text(value, "watermarkArchiveCommitOrdinal")?,
            watermark_frontier_sha256: text(value, "watermarkFrontierSha256")?,
        },
        custody_checkpoint_sha256: text(value, "custodyCheckpointSha256")?,
        member_root_sha256: text(value, "memberRootSha256")?,
        page_sha256: text(value, "pageSha256")?,
        start_member_sha256,
    })
}

/// Decode one required lowercase hash from a closed publication object.
fn required_hash(
    value: &JsonValue,
    key: &str,
) -> Result<String, NativeMaintenanceDevelopmentAnchorError> {
    let value = text(value, key).ok_or(NativeMaintenanceDevelopmentAnchorError::Publication)?;
    // reject noncanonical digests before using them as retained graph identities
    if !valid_hash(&value) {
        return Err(NativeMaintenanceDevelopmentAnchorError::Publication);
    }
    Ok(value)
}

/// Compare one verified journal event discriminator.
fn journal_kind(record: &crate::NativeMaintenanceJournalRecord, expected: &str) -> bool {
    matches!(&record.kind, JsonValue::String(units) if units.iter().copied().eq(expected.encode_utf16()))
}

/// Recognize one exact lowercase forty-character Git commit.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Recognize the source's immutable calendar release namespace.
fn valid_release(value: &str) -> bool {
    // split the calendar and positive revision once
    let Some((date, revision)) = value.split_once('-') else {
        return false;
    };
    let bytes = date.as_bytes();
    bytes.len() == 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'.'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'.'
        && bytes[8..].iter().all(u8::is_ascii_digit)
        && !revision.is_empty()
        && !revision.starts_with('0')
        && revision.bytes().all(|byte| byte.is_ascii_digit())
}

/// Recognize the source's positive bounded control-plane version.
fn valid_control_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 6
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
#[path = "native_maintenance_development_anchor_test.rs"]
mod tests;
