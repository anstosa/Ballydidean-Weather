//! Installed pre-month rain control-reference release, custody, and family application.

use std::{
    fmt::{self, Display, Formatter},
    io,
    path::Path,
    process::Command,
};

use weather_api::admit_rain_portable_artifact;
use weather_contracts::{JsonValue, RainHurdleWindPortableArtifact};
use weather_domain::admit_rain_maintenance_control_state;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec, RevisionGapSha256};

use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    NativeMaintenanceArchiveSegmentClock, NativeMaintenanceArchiveWallClock,
    NativeMaintenanceCaptureFamily, NativeMaintenanceCaptureRemoteAuthority,
    NativeMaintenanceControlReferenceInput, NativeMaintenanceControlReferenceMaterial,
    NativeMaintenanceControllerArchiveValidators, NativeMaintenanceDatabaseLedgerReader,
    NativeMaintenanceDue, NativeMaintenanceDueMode, NativeMaintenanceDueRelease,
    NativeMaintenanceDueReleaseContext, NativeMaintenanceFamilyReleaseApplyInput,
    NativeMaintenanceFamilyReleaseApplyState, NativeMaintenanceJournalStore,
    NativeMaintenanceModelPublishError, NativeMaintenanceModelPublisher,
    NativeMaintenancePlaintextArchive, NativeMaintenanceReleaseLease,
    NativeMaintenanceReleaseLeaseAcquisition, NativeMaintenanceReleaseLeaseInput,
    NativeMaintenanceReleaseLeaseReleaseInput, NativeModelEvidenceValues, NativeOutboundHttp,
    NativeWirePrimitives, VerifiedNativeMaintenanceCaptureEpochWitness,
    VerifiedNativeRevisionColdCustodyAcknowledgementV2, acquire_native_maintenance_release_lease,
    adjustment_rolling_local_date_at, apply_native_maintenance_family_release,
    build_native_maintenance_family_release_apply_request,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_control_release_package::{
        NativeMaintenanceControlReferenceCompensationInput,
        NativeMaintenanceControlReferenceTargetInput, NativeMaintenanceControlReleasePackage,
        NativeMaintenanceControlReleasePackageError,
        build_native_maintenance_control_reference_compensation,
        build_native_maintenance_control_reference_target,
    },
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
    read_latest_native_maintenance_revision_custody_acknowledgement,
    release_native_maintenance_release_lease, verify_native_maintenance_due,
};

const REPOSITORY: &str = "/home/ubuntu/weather";
const SELECTOR_PATH: &str = "config/forecast-adjustments/ballydidean-rain-control-reference.json";
const SELECTOR_MAXIMUM_BYTES: usize = 16 * 1_024;
const PACKAGE_MAXIMUM_BYTES: usize = 8 * 1_024 * 1_024;
const DAY_MILLISECONDS: i64 = 86_400_000;
const CURRENT_COMMAND: &str = "adjustment-rain-control-custody-anchor-current-v1";
const INSTALL_COMMAND: &str = "adjustment-rain-control-custody-anchor-install-v1";
const INSTALLATION_VERSION: &str = "adjustment-rain-control-custody-anchor-installation/v1";
const CURRENT_KEYS: &[&str] = &["anchor", "anchorSha256"];
const INSTALLATION_KEYS: &[&str] = &["anchorSha256", "contractVersion", "state"];

/// Borrowed exact byte material returned by the archived control-reference semantic owner.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceInstalledControlMaterialInput<'a> {
    pub control_state_bytes: &'a [u8],
    pub graph_manifest_sha256: &'a str,
    pub ordinal_artifact_bytes: &'a [u8],
    pub source_member_root_sha256: &'a str,
    pub source_receipt_root_sha256: &'a str,
}

#[derive(Clone, Debug)]
pub(crate) struct AdmittedControlMaterial {
    control_state_bytes: Vec<u8>,
    graph_manifest_sha256: String,
    ordinal_artifact_bytes: Vec<u8>,
    source_member_root_sha256: String,
    source_receipt_root_sha256: String,
    retained_value: JsonValue,
}

/// Verified current rain-control custody anchor and its exact content address.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceRainControlAnchorCurrent {
    pub anchor: JsonValue,
    pub anchor_sha256: String,
    pub sequence: u64,
}

/// Canonical rain-control custody anchor submitted to the installed remote.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceRainControlAnchor {
    pub anchor: JsonValue,
    pub canonical_bytes: Vec<u8>,
    pub anchor_sha256: String,
}

/// Verified installation response bound to the submitted rain-control anchor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceRainControlAnchorInstallation {
    pub anchor_sha256: String,
    pub canonical_bytes: Vec<u8>,
}

/// Closed installed owner failure by the first refusing boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceInstalledControlReleaseError {
    Input,
    Clock,
    Material,
    Journal,
    Archive,
    Current,
    Baseline,
    Package(NativeMaintenanceControlReleasePackageError),
    Publish(NativeMaintenanceModelPublishError),
    Acknowledgement,
    Anchor,
    Remote,
    Apply,
}

use NativeMaintenanceInstalledControlReleaseError as Error;

impl Display for NativeMaintenanceInstalledControlReleaseError {
    /// Render one stable value-free installed control-release failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // retain the first owning boundary without reflecting private material
        formatter.write_str(match self {
            Self::Input => "installed rain control release input is invalid",
            Self::Clock => "installed rain control release clock is invalid",
            Self::Material => "installed rain control material is invalid",
            Self::Journal => "installed rain control release journal failed",
            Self::Archive => "installed rain control graph is unavailable",
            Self::Current => "installed rain control current release is invalid",
            Self::Baseline => "installed rain control baseline selector is unavailable",
            Self::Package(_) => "installed rain control package is invalid",
            Self::Publish(_) => "installed rain control publication failed",
            Self::Acknowledgement => {
                "installed rain control custody acknowledgement is unavailable"
            }
            Self::Anchor => "installed rain control custody anchor is invalid",
            Self::Remote => "installed rain control remote operation failed",
            Self::Apply => "installed rain control family application was not acknowledged",
        })
    }
}

impl std::error::Error for NativeMaintenanceInstalledControlReleaseError {
    /// Retain only the closed package and publisher causes.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // transport and archive failures deliberately remain value-free
        match self {
            Self::Package(error) => Some(error),
            _ => None,
        }
    }
}

/// Exact installed control owner sharing the process journal, archive, transport, and epoch.
pub struct NativeMaintenanceInstalledControlReleasePorts<'a> {
    pub journal: &'a mut NativeMaintenanceJournalStore,
    pub archive:
        &'a mut NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>,
    pub remote: &'a mut NativeMaintenanceDatabaseLedgerReader,
    pub http: &'a NativeOutboundHttp,
    pub epoch_witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    material: Option<AdmittedControlMaterial>,
}

impl<'a> NativeMaintenanceInstalledControlReleasePorts<'a> {
    /// Bind real installed owners without reading or mutating release state.
    pub fn new(
        journal: &'a mut NativeMaintenanceJournalStore,
        archive: &'a mut NativeMaintenancePlaintextArchive<
            NativeMaintenanceControllerArchiveValidators,
        >,
        remote: &'a mut NativeMaintenanceDatabaseLedgerReader,
        http: &'a NativeOutboundHttp,
        epoch_witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    ) -> Self {
        Self {
            journal,
            archive,
            remote,
            http,
            epoch_witness,
            material: None,
        }
    }

    /// Validate and retain actual byte material while returning its existing semantic token.
    pub fn admit_control_reference(
        &mut self,
        due: &NativeMaintenanceDue,
        now: &str,
        input: NativeMaintenanceControlReferenceInput,
        material: NativeMaintenanceInstalledControlMaterialInput<'_>,
    ) -> Result<NativeMaintenanceControlReferenceMaterial, Error> {
        validate_material(due, now, material)?;
        self.material = Some(AdmittedControlMaterial {
            control_state_bytes: material.control_state_bytes.to_vec(),
            graph_manifest_sha256: material.graph_manifest_sha256.to_owned(),
            ordinal_artifact_bytes: material.ordinal_artifact_bytes.to_vec(),
            source_member_root_sha256: material.source_member_root_sha256.to_owned(),
            source_receipt_root_sha256: material.source_receipt_root_sha256.to_owned(),
            retained_value: input.value.clone(),
        });
        Ok(NativeMaintenanceControlReferenceMaterial { value: input.value })
    }

    /// Move the already admitted real bytes into the sequential installed controller owner.
    pub(crate) fn take_material(&mut self) -> Option<AdmittedControlMaterial> {
        self.material.take()
    }

    /// Reattach only the exact previously admitted material immediately before publication.
    pub(crate) fn restore_material(&mut self, material: AdmittedControlMaterial) {
        self.material = Some(material);
    }

    /// Execute the target, prepared compensation, custody install, and exact rain family apply.
    pub fn publish_control_reference(
        &mut self,
        context: &NativeMaintenanceDueReleaseContext<'_>,
    ) -> Result<NativeMaintenanceDueRelease, Error> {
        let material = self.material.take().ok_or(Error::Material)?;
        let retained = context
            .outcome
            .control_reference_material
            .as_ref()
            .ok_or(Error::Material)?;
        // require the exact token admitted before the attempt report was built
        if retained.value != material.retained_value {
            return Err(Error::Material);
        }
        let control_state_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
            &NativeWirePrimitives,
            &material.control_state_bytes,
        ));
        let run_id = format!("release-control-{}", &control_state_sha256[..32]);
        let acquired_at = NativeMaintenanceArchiveWallClock
            .now()
            .ok_or(Error::Clock)?;
        let acquisition = acquire_native_maintenance_release_lease(
            self.journal,
            NativeMaintenanceReleaseLeaseInput {
                due_key: &context.due.due_key,
                input_head_sha256: context.input_head_sha256,
                now: &acquired_at,
                run_id: &run_id,
            },
        )
        .map_err(|_| Error::Journal)?;
        let NativeMaintenanceReleaseLeaseAcquisition::Held(lease) = acquisition else {
            return Err(Error::Journal);
        };
        let result = self.complete_control_release(context, &material, &lease);
        // discharge only the exact retained release fence after either terminal result
        release_native_maintenance_release_lease(
            self.journal,
            NativeMaintenanceReleaseLeaseReleaseInput {
                due_key: &context.due.due_key,
                now: &NativeMaintenanceArchiveWallClock
                    .now()
                    .ok_or(Error::Clock)?,
                run_id: &run_id,
            },
        )
        .map_err(|_| Error::Journal)?;
        result
    }

    // complete one already fenced source publication without widening its material
    fn complete_control_release(
        &mut self,
        context: &NativeMaintenanceDueReleaseContext<'_>,
        material: &AdmittedControlMaterial,
        lease: &NativeMaintenanceReleaseLease,
    ) -> Result<NativeMaintenanceDueRelease, Error> {
        let wire = NativeWirePrimitives;
        // rebind the held fence to this due and immutable attempt input
        if lease.due_key != context.due.due_key
            || lease.input_head_sha256 != context.input_head_sha256
            || lease.run_id
                != format!(
                    "release-control-{}",
                    &format_digest(RevisionArchiveCanonicalCodec::sha256(
                        &wire,
                        &material.control_state_bytes
                    ))[..32]
                )
        {
            return Err(Error::Input);
        }
        let graph_digest = digest(&material.graph_manifest_sha256).ok_or(Error::Archive)?;
        let graph = self
            .archive
            .verify_full_graph(graph_digest, false)
            .map_err(|_| Error::Archive)?;
        let state_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
            &wire,
            &material.control_state_bytes,
        ));
        let artifact_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
            &wire,
            &material.ordinal_artifact_bytes,
        ));
        // require both exact public members inside the verified retained graph
        for identity in [&state_sha256, &artifact_sha256] {
            if !graph
                .manifest
                .entries
                .iter()
                .any(|entry| entry.identity_sha256.as_str() == identity.as_str())
            {
                return Err(Error::Archive);
            }
        }
        let current = NativeMaintenanceCaptureRemoteAuthority::new(self.remote)
            .read_verified_family_current_full(NativeMaintenanceCaptureFamily::Rain)
            .map_err(|_| Error::Current)?;
        let baseline = read_rain_control_reference_baseline(&current.commit)?;
        let published_at = NativeMaintenanceArchiveWallClock
            .now()
            .ok_or(Error::Clock)?;
        let release_date = adjustment_rolling_local_date_at(&published_at)
            .map_err(|_| Error::Clock)?
            .replace('-', ".");
        let target = build_native_maintenance_control_reference_target(
            NativeMaintenanceControlReferenceTargetInput {
                control_state_bytes: &material.control_state_bytes,
                due_key: &context.due.due_key,
                expected_catalog_receipt_sha256: current
                    .control_installed_receipt_sha256
                    .as_deref(),
                expected_settings_sha256: &current.settings_sha256,
                expected_source_commit: &current.commit,
                expected_source_release: &current.release,
                fencing_token: &lease.fencing_token,
                graph_manifest_sha256: &material.graph_manifest_sha256,
                now: context.report_created_at,
                ordinal_artifact_bytes: &material.ordinal_artifact_bytes,
                source_member_root_sha256: &material.source_member_root_sha256,
                source_receipt_root_sha256: &material.source_receipt_root_sha256,
            },
        )
        .map_err(Error::Package)?;
        let mut publisher = NativeMaintenanceModelPublisher {
            journal: self.journal,
            http: self.http,
        };
        let published_target = publisher
            .publish_packaged(target.as_publish_package(
                &release_date,
                &context.due.due_key,
                &lease.run_id,
            ))
            .map_err(Error::Publish)?;
        let compensation_created_at = NativeMaintenanceArchiveWallClock
            .now()
            .ok_or(Error::Clock)?;
        let compensation = build_native_maintenance_control_reference_compensation(
            NativeMaintenanceControlReferenceCompensationInput {
                baseline_selector_bytes: baseline.as_deref(),
                compensation_created_at: &compensation_created_at,
                target: &target,
                target_commit_sha: &published_target.commit_sha,
                target_release_tag: &published_target.release_tag,
            },
        )
        .map_err(Error::Package)?;
        let published_compensation = publisher
            .publish_packaged(compensation.as_publish_package(
                &release_date,
                &context.due.due_key,
                &lease.run_id,
            ))
            .map_err(Error::Publish)?;
        publisher
            .record_pair(&target.action, &published_target, &published_compensation)
            .map_err(Error::Publish)?;
        let acknowledgement =
            read_latest_native_maintenance_revision_custody_acknowledgement(self.journal)
                .map_err(|_| Error::Acknowledgement)?
                .ok_or(Error::Acknowledgement)?;
        let previous = read_rain_control_anchor_current(self.remote)?;
        let verified_at = NativeMaintenanceArchiveWallClock
            .now()
            .ok_or(Error::Clock)?;
        let anchor = build_rain_control_anchor(
            context,
            material,
            &target,
            &published_target.action_sha256,
            &current.commit,
            &lease.fencing_token,
            self.epoch_witness,
            &acknowledgement,
            previous.as_ref(),
            &verified_at,
        )?;
        let installed = install_rain_control_anchor(self.remote, &anchor)?;
        let request = build_native_maintenance_family_release_apply_request(
            NativeMaintenanceFamilyReleaseApplyInput {
                action_sha256: &published_target.action_sha256,
                compensating_release: &published_compensation.release_tag,
                expected_current_release: &current.release,
                expected_settings_sha256: &current.settings_sha256,
                expected_source_release: &current.release,
                family: "rain",
                fencing_token: &lease.fencing_token,
                report_sha256: &material.graph_manifest_sha256,
                target_release: &published_target.release_tag,
            },
        )
        .map_err(|_| Error::Apply)?;
        let applied = apply_native_maintenance_family_release(self.remote, &request)
            .map_err(|_| Error::Apply)?;
        // require the exact anchor install and ordinary acknowledged family terminal
        if installed.anchor_sha256 != anchor.anchor_sha256
            || applied.state != NativeMaintenanceFamilyReleaseApplyState::Acknowledged
            || applied.action_sha256 != published_target.action_sha256
            || applied.family.as_deref() != Some("rain")
            || applied.target_release != published_target.release_tag
            || applied.compensating_release != published_compensation.release_tag
            || applied.fencing_token != lease.fencing_token
        {
            return Err(Error::Apply);
        }
        Ok(NativeMaintenanceDueRelease {
            action_sha256: published_target.action_sha256,
            acknowledged: true,
        })
    }
}

/// Fixed installed rain-control custody operations over the authenticated remote reader.
pub trait NativeMaintenanceRainControlRemotePort {
    /// Read the zero-operand current control anchor document.
    fn read_rain_control_anchor_current(&mut self) -> io::Result<Vec<u8>>;

    /// Install exact canonical anchor bytes under their SHA-256 operand.
    fn install_rain_control_anchor(
        &mut self,
        anchor_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>>;
}

impl NativeMaintenanceRainControlRemotePort for NativeMaintenanceDatabaseLedgerReader {
    /// Reuse the fixed forced command with no caller-selected operands.
    fn read_rain_control_anchor_current(&mut self) -> io::Result<Vec<u8>> {
        self.request_document(CURRENT_COMMAND, &[], None)
    }

    /// Reuse the fixed forced install command with exact stdin and hash operand.
    fn install_rain_control_anchor(
        &mut self,
        anchor_sha256: &str,
        canonical_bytes: &[u8],
    ) -> io::Result<Vec<u8>> {
        self.request_document(INSTALL_COMMAND, &[anchor_sha256], Some(canonical_bytes))
    }
}

/// Read and verify the sole installed predecessor rain-control anchor.
pub fn read_rain_control_anchor_current(
    remote: &mut impl NativeMaintenanceRainControlRemotePort,
) -> Result<Option<NativeMaintenanceRainControlAnchorCurrent>, Error> {
    let bytes = remote
        .read_rain_control_anchor_current()
        .map_err(|_| Error::Remote)?;
    verify_rain_control_anchor_current(&bytes)
}

/// Verify one canonical nullable current rain-control anchor response.
pub fn verify_rain_control_anchor_current(
    bytes: &[u8],
) -> Result<Option<NativeMaintenanceRainControlAnchorCurrent>, Error> {
    let value =
        crate::MaintenanceCanonicalJson::parse_canonical_object(&mut NativeWirePrimitives, bytes)
            .ok_or(Error::Anchor)?;
    // preserve genuine create-once absence without inventing a predecessor
    if value == JsonValue::Null {
        return Ok(None);
    }
    if !exact_keys(&value, CURRENT_KEYS) {
        return Err(Error::Anchor);
    }
    let anchor = field(&value, "anchor").ok_or(Error::Anchor)?.clone();
    validate_rain_control_anchor(&anchor)?;
    let canonical = NativeWirePrimitives
        .encode_sorted_json(&anchor)
        .ok_or(Error::Anchor)?;
    let anchor_sha256 = text(&value, "anchorSha256").ok_or(Error::Anchor)?;
    let sequence = text(&anchor, "sequence")
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(Error::Anchor)?;
    // bind the wrapper address to the exact canonical anchor bytes
    if !valid_hash(&anchor_sha256)
        || anchor_sha256
            != format_digest(RevisionArchiveCanonicalCodec::sha256(
                &NativeWirePrimitives,
                &canonical,
            ))
    {
        return Err(Error::Anchor);
    }
    Ok(Some(NativeMaintenanceRainControlAnchorCurrent {
        anchor,
        anchor_sha256,
        sequence,
    }))
}

/// Install one exact rain-control anchor and verify its bound installed response.
pub fn install_rain_control_anchor(
    remote: &mut impl NativeMaintenanceRainControlRemotePort,
    anchor: &NativeMaintenanceRainControlAnchor,
) -> Result<NativeMaintenanceRainControlAnchorInstallation, Error> {
    validate_rain_control_anchor(&anchor.anchor)?;
    let canonical = NativeWirePrimitives
        .encode_sorted_json(&anchor.anchor)
        .ok_or(Error::Anchor)?;
    // prevent mutation of any public anchor member after construction
    if canonical != anchor.canonical_bytes
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &NativeWirePrimitives,
            &canonical,
        )) != anchor.anchor_sha256
    {
        return Err(Error::Anchor);
    }
    let bytes = remote
        .install_rain_control_anchor(&anchor.anchor_sha256, &anchor.canonical_bytes)
        .map_err(|_| Error::Remote)?;
    let response =
        crate::MaintenanceCanonicalJson::parse_canonical_object(&mut NativeWirePrimitives, &bytes)
            .ok_or(Error::Remote)?;
    let anchor_sha256 = text(&response, "anchorSha256").ok_or(Error::Remote)?;
    // bind the fixed response schema, state, and submitted content identity
    if !exact_keys(&response, INSTALLATION_KEYS)
        || anchor_sha256 != anchor.anchor_sha256
        || text(&response, "contractVersion").as_deref() != Some(INSTALLATION_VERSION)
        || text(&response, "state").as_deref() != Some("installed")
    {
        return Err(Error::Remote);
    }
    Ok(NativeMaintenanceRainControlAnchorInstallation {
        anchor_sha256,
        canonical_bytes: bytes,
    })
}

#[allow(clippy::too_many_arguments)]
// build and validate the exact custody-only control anchor after paired publication
fn build_rain_control_anchor(
    context: &NativeMaintenanceDueReleaseContext<'_>,
    material: &AdmittedControlMaterial,
    package: &NativeMaintenanceControlReleasePackage,
    action_sha256: &str,
    source_commit: &str,
    fencing_token: &str,
    witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
    acknowledgement: &VerifiedNativeRevisionColdCustodyAcknowledgementV2,
    previous: Option<&NativeMaintenanceRainControlAnchorCurrent>,
    full_graph_verified_at: &str,
) -> Result<NativeMaintenanceRainControlAnchor, Error> {
    let (predecessor, sequence) = match previous {
        None => (JsonValue::Null, 0),
        Some(previous) => (
            string(&previous.anchor_sha256),
            previous.sequence.checked_add(1).ok_or(Error::Anchor)?,
        ),
    };
    let acknowledged_at =
        text(&acknowledgement.value, "acknowledgedAt").ok_or(Error::Acknowledgement)?;
    let verified_ms = NativeWirePrimitives
        .canonical_utc_milliseconds(full_graph_verified_at)
        .ok_or(Error::Clock)?;
    let acknowledged_ms = NativeWirePrimitives
        .canonical_utc_milliseconds(&acknowledged_at)
        .ok_or(Error::Acknowledgement)?;
    // require the verified graph observation at or after its retained custody acknowledgement
    if verified_ms < acknowledged_ms
        || !valid_hash(&witness.witness_sha256)
        || !valid_hash(&witness.control_plane_sha256)
        || !valid_hash(action_sha256)
        || package.action_sha256 != action_sha256
    {
        return Err(Error::Anchor);
    }
    let anchor = object(vec![
        ("actionKind", string("control_reference")),
        ("actionSha256", string(action_sha256)),
        (
            "archiveCommitOrdinal",
            string(&acknowledgement.next_archive_commit_ordinal),
        ),
        ("captureEpochWitnessSha256", string(&witness.witness_sha256)),
        (
            "contractVersion",
            string("adjustment-rain-control-custody-anchor/v1"),
        ),
        ("controlSha256", string(&witness.control_plane_sha256)),
        (
            "controlStateSha256",
            string(&package.target.control_state_sha256),
        ),
        ("controlVersion", string(&witness.control_plane_version)),
        ("custodyAcknowledgedAt", string(&acknowledged_at)),
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
        ("dueMonth", string(&package.target.due_month)),
        ("fencingToken", string(fencing_token)),
        (
            "frontierSha256",
            string(&acknowledgement.next_frontier_sha256),
        ),
        ("fullGraphVerifiedAt", string(full_graph_verified_at)),
        (
            "graphManifestSha256",
            string(&material.graph_manifest_sha256),
        ),
        (
            "memberRootSha256",
            string(&acknowledgement.request.member_root_sha256),
        ),
        (
            "ordinalArtifactSha256",
            string(&format_digest(RevisionArchiveCanonicalCodec::sha256(
                &NativeWirePrimitives,
                &material.ordinal_artifact_bytes,
            ))),
        ),
        ("pageSha256", string(&acknowledgement.request.page_sha256)),
        ("predecessorAnchorSha256", predecessor),
        ("sequence", string(&sequence.to_string())),
        ("sourceCommit", string(source_commit)),
        (
            "sourceMemberRootSha256",
            string(&material.source_member_root_sha256),
        ),
        (
            "sourceReceiptRootSha256",
            string(&material.source_receipt_root_sha256),
        ),
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
        (
            "workstationJournalHeadSha256",
            string(context.lifecycle_head_sha256),
        ),
    ]);
    validate_rain_control_anchor(&anchor)?;
    let canonical_bytes = NativeWirePrimitives
        .encode_sorted_json(&anchor)
        .ok_or(Error::Anchor)?;
    let anchor_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &canonical_bytes,
    ));
    Ok(NativeMaintenanceRainControlAnchor {
        anchor,
        canonical_bytes,
        anchor_sha256,
    })
}

// validate through the shared closed rain-control custody schema
fn validate_rain_control_anchor(anchor: &JsonValue) -> Result<(), Error> {
    crate::custody_anchor_validation::validate_custody_anchor(
        crate::custody_anchor_current::CustodyAnchorKind::RainControl,
        anchor,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .ok_or(Error::Anchor)
}

// validate original byte material before the report can retain publication authority
fn validate_material(
    due: &NativeMaintenanceDue,
    now: &str,
    material: NativeMaintenanceInstalledControlMaterialInput<'_>,
) -> Result<(), Error> {
    if !verify_native_maintenance_due(due)
        || due.mode != NativeMaintenanceDueMode::Monthly
        || due.family != Some(NativeMaintenanceCaptureFamily::Rain)
        || due.scope != "control-reference/rain"
        || !due.due_key.starts_with("control-reference/rain/")
        || material.control_state_bytes.is_empty()
        || material.ordinal_artifact_bytes.is_empty()
        || material.control_state_bytes.len() > PACKAGE_MAXIMUM_BYTES
        || material.ordinal_artifact_bytes.len() > PACKAGE_MAXIMUM_BYTES
        || !valid_hash(material.graph_manifest_sha256)
        || !valid_hash(material.source_member_root_sha256)
        || !valid_hash(material.source_receipt_root_sha256)
    {
        return Err(Error::Material);
    }
    let values = NativeModelEvidenceValues::new();
    let state = admit_rain_maintenance_control_state(
        &crate::api_maintenance_admission::ApiRainControlPrimitives(&values),
        material.control_state_bytes,
    )
    .ok_or(Error::Material)?;
    let mut wire = NativeWirePrimitives;
    let artifact_document = crate::MaintenanceCanonicalJson::parse_canonical_object(
        &mut wire,
        material.ordinal_artifact_bytes,
    )
    .ok_or(Error::Material)?;
    let artifact = admit_rain_portable_artifact(
        &artifact_document,
        |value| values.decode_shadow_rain_artifact(value),
        |feature_names| {
            let bytes = values.encode_shadow_ordered_json(feature_names)?;
            ApiMaintenanceReceiptCanonicalValues::sha256(&values, &bytes)
        },
    )
    .ok_or(Error::Material)?;
    let artifact_month = match artifact {
        RainHurdleWindPortableArtifact::V1(value) => value.modelMonth,
        RainHurdleWindPortableArtifact::V2(value) => value.modelMonth,
    };
    let due_month = due
        .due_key
        .strip_prefix("control-reference/rain/")
        .ok_or(Error::Material)?;
    let month_start = wire
        .canonical_utc_milliseconds(&format!("{due_month}-01T00:00:00.000Z"))
        .ok_or(Error::Material)?;
    let now = wire
        .canonical_utc_milliseconds(now)
        .ok_or(Error::Material)?;
    // bind the state, portable artifact, graph roots, and literal pre-month window
    if state.model_month != due_month
        || artifact_month != due_month
        || state.ordinal_artifact_sha256
            != format_digest(RevisionArchiveCanonicalCodec::sha256(
                &wire,
                material.ordinal_artifact_bytes,
            ))
        || state.source_member_root_sha256 != material.source_member_root_sha256
        || state.source_receipt_root_sha256 != material.source_receipt_root_sha256
        || state.generated_epoch_milliseconds < month_start - 7 * DAY_MILLISECONDS
        || state.generated_epoch_milliseconds >= month_start
        || state.generated_epoch_milliseconds > now
    {
        return Err(Error::Material);
    }
    Ok(())
}

// read exact predecessor selector bytes or genuine absence from the authenticated source commit
fn read_rain_control_reference_baseline(source_commit: &str) -> Result<Option<Vec<u8>>, Error> {
    if !valid_commit(source_commit) {
        return Err(Error::Baseline);
    }
    let repository = Path::new(REPOSITORY);
    let tree = git(
        repository,
        &[
            "ls-tree",
            "-z",
            "--name-only",
            source_commit,
            "--",
            SELECTOR_PATH,
        ],
    )?;
    // distinguish exact absence from alternate or ambiguous tree output
    if tree.is_empty() {
        return Ok(None);
    }
    if tree != format!("{SELECTOR_PATH}\0").as_bytes() {
        return Err(Error::Baseline);
    }
    let object = format!("{source_commit}:{SELECTOR_PATH}");
    let bytes = git(repository, &["show", &object])?;
    // preserve the source selector's independent narrow byte ceiling
    if !(2..=SELECTOR_MAXIMUM_BYTES).contains(&bytes.len()) {
        return Err(Error::Baseline);
    }
    Ok(Some(bytes))
}

// execute the fixed local Git binary with bounded output and no interactive fallback
fn git(repository: &Path, arguments: &[&str]) -> Result<Vec<u8>, Error> {
    let output = Command::new("/usr/bin/git")
        .current_dir(repository)
        .args(arguments)
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|_| Error::Baseline)?;
    // reject failures and output beyond the only two source read domains
    if !output.status.success()
        || output.stdout.len() > SELECTOR_MAXIMUM_BYTES + 1
        || output.stderr.len() > 32 * 1024
    {
        return Err(Error::Baseline);
    }
    Ok(output.stdout)
}

// decode one already admitted lowercase content address
fn digest(value: &str) -> Option<[u8; 32]> {
    Some(*RevisionGapSha256::from_lower_hex(value)?.as_bytes())
}

// recognize one exact lowercase forty-character Git commit
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
