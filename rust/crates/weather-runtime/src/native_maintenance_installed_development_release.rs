//! Installed development release preparation using actual archived candidate and fenced publisher.

use std::io::{Read, Seek, SeekFrom};

use weather_edge::{RevisionArchiveCanonicalCodec, RevisionGapSha256};

use crate::{
    NativeFamilyReleaseCurrent, NativeMaintenanceArchiveJournal,
    NativeMaintenanceArchiveSegmentClock, NativeMaintenanceArchiveWallClock,
    NativeMaintenanceCaptureRemoteAuthority, NativeMaintenanceControllerArchiveValidators,
    NativeMaintenanceDatabaseLedgerReader, NativeMaintenanceDevelopmentCompensationError,
    NativeMaintenanceDevelopmentCompensationInput, NativeMaintenanceDevelopmentCompensationTarget,
    NativeMaintenanceDevelopmentPublication, NativeMaintenanceDevelopmentPublicationError,
    NativeMaintenanceDevelopmentPublicationInput, NativeMaintenanceDueReleaseContext,
    NativeMaintenanceInstalledCandidateOwner, NativeMaintenanceJournalStore,
    NativeMaintenanceModelPublishError, NativeMaintenanceModelPublisher,
    NativeMaintenancePlaintextArchive, NativeMaintenancePlaintextObjectStore,
    NativeMaintenancePublishedModelRelease, NativeMaintenanceReleaseLease,
    NativeMaintenanceVerifiedFullGraph, NativeOutboundHttp, NativeWirePrimitives,
    build_native_maintenance_development_compensation,
    build_native_maintenance_development_publication,
    native_maintenance_model_release_publish::NativeMaintenanceModelPublishPackage,
    native_maintenance_retirement_files_v2::format_digest,
    publish_native_maintenance_archive_segment,
};

use crate::{
    NativeMaintenanceDevelopmentAcknowledgementInput, NativeMaintenanceDevelopmentAnchorError,
    NativeMaintenanceDevelopmentAnchorInput, NativeMaintenanceDevelopmentRemoteError,
    NativeMaintenanceDueRelease, NativeMaintenanceFamilyReleaseApplyInput,
    NativeMaintenanceReleaseLeaseAcquisition, NativeMaintenanceReleaseLeaseInput,
    NativeMaintenanceReleaseLeaseReleaseInput, acquire_native_maintenance_release_lease,
    apply_native_maintenance_family_release, build_native_maintenance_development_anchor,
    build_native_maintenance_development_anchor_install_request,
    build_native_maintenance_family_release_apply_request,
    install_native_maintenance_development_custody_anchor, maintenance_canonical_json::valid_hash,
    read_latest_native_maintenance_revision_custody_acknowledgement,
    read_native_maintenance_development_anchor_current, release_native_maintenance_release_lease,
    verify_native_maintenance_development_acknowledgement,
};

const CANDIDATE_MAXIMUM_BYTES: u64 = 8 * 1024 * 1024;

/// Actual source material retained across target/compensation publication and custody install.
pub struct NativeMaintenanceDevelopmentReleasedGraph {
    pub publication: NativeMaintenanceDevelopmentPublication,
    pub current: NativeFamilyReleaseCurrent,
    pub target: NativeMaintenancePublishedModelRelease,
    pub compensation: NativeMaintenancePublishedModelRelease,
    pub development_graph: NativeMaintenanceVerifiedFullGraph,
}

/// Exact stage failure without exposing private artifact bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceInstalledDevelopmentReleaseError {
    Input,
    Clock,
    Journal,
    Archive,
    Current,
    Candidate,
    Material(NativeMaintenanceDevelopmentPublicationError),
    Compensation(NativeMaintenanceDevelopmentCompensationError),
    Publish(NativeMaintenanceModelPublishError),
    Anchor(NativeMaintenanceDevelopmentAnchorError),
    Remote(NativeMaintenanceDevelopmentRemoteError),
}

use NativeMaintenanceInstalledDevelopmentReleaseError as Error;

/// Active installed source owners shared by the development and control release paths.
pub struct NativeMaintenanceInstalledDevelopmentReleasePorts<'a> {
    pub journal: &'a mut NativeMaintenanceJournalStore,
    pub objects: &'a mut NativeMaintenancePlaintextObjectStore,
    pub archive:
        &'a mut NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>,
    pub remote: &'a mut NativeMaintenanceDatabaseLedgerReader,
    pub http: &'a NativeOutboundHttp,
}

impl NativeMaintenanceInstalledDevelopmentReleasePorts<'_> {
    /// Execute one complete source development-candidate transaction under its exact release lease.
    pub fn publish_development_candidate(
        &mut self,
        context: &NativeMaintenanceDueReleaseContext<'_>,
        candidate: &NativeMaintenanceInstalledCandidateOwner,
    ) -> Result<NativeMaintenanceDueRelease, Error> {
        let candidate_sha256 = context
            .outcome
            .candidate_sha256
            .as_deref()
            .ok_or(Error::Input)?;
        // retain the source run id, immutable due head and fresh actual acquisition clock
        if !valid_hash(candidate_sha256) {
            return Err(Error::Input);
        }
        let run_id = format!("release-{}", &candidate_sha256[..32]);
        let acquired_at = NativeMaintenanceArchiveWallClock
            .now()
            .ok_or(Error::Clock)?;
        let lease = acquire_native_maintenance_release_lease(
            self.journal,
            NativeMaintenanceReleaseLeaseInput {
                due_key: &context.due.due_key,
                input_head_sha256: context.input_head_sha256,
                now: &acquired_at,
                run_id: &run_id,
            },
        )
        .map_err(|_| Error::Journal)?;
        let NativeMaintenanceReleaseLeaseAcquisition::Held(lease) = lease else {
            return Err(Error::Journal);
        };
        let result = self.complete_development(context, candidate, &lease);
        // always discharge only the exact retained publisher fence, including a failed publication
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

    /// Install the exact verified development custody graph and then apply the acknowledged family release.
    fn complete_development(
        &mut self,
        context: &NativeMaintenanceDueReleaseContext<'_>,
        candidate: &NativeMaintenanceInstalledCandidateOwner,
        lease: &NativeMaintenanceReleaseLease,
    ) -> Result<NativeMaintenanceDueRelease, Error> {
        let released = self.publish_pair_and_graph(context, candidate, lease)?;
        let acknowledgement =
            read_latest_native_maintenance_revision_custody_acknowledgement(self.journal)
                .map_err(Error::Anchor)?
                .ok_or(Error::Journal)?;
        let previous = read_native_maintenance_development_anchor_current(self.remote)
            .map_err(Error::Anchor)?;
        let verified_at = NativeMaintenanceArchiveWallClock
            .now()
            .ok_or(Error::Clock)?;
        let anchor =
            build_native_maintenance_development_anchor(NativeMaintenanceDevelopmentAnchorInput {
                acknowledgement: &acknowledgement,
                current_release: &released.current,
                development_graph: &released.development_graph,
                due: context.due,
                epoch_witness: candidate.witness().ok_or(Error::Candidate)?,
                full_graph_verified_at: &verified_at,
                input_head_sha256: context.input_head_sha256,
                lifecycle_head_sha256: context.lifecycle_head_sha256,
                previous: previous.as_ref(),
                publication: &released.publication,
            })
            .map_err(Error::Anchor)?;
        let install = build_native_maintenance_development_anchor_install_request(&anchor.anchor)
            .map_err(Error::Remote)?;
        let installation =
            install_native_maintenance_development_custody_anchor(self.remote, &install)
                .map_err(Error::Remote)?;
        let family = context.due.family.ok_or(Error::Input)?;
        let apply_request = build_native_maintenance_family_release_apply_request(
            NativeMaintenanceFamilyReleaseApplyInput {
                action_sha256: &released.target.action_sha256,
                compensating_release: &released.compensation.release_tag,
                expected_current_release: &released.current.release,
                expected_settings_sha256: &released.current.settings_sha256,
                expected_source_release: &released.current.release,
                family: family.wire(),
                fencing_token: &lease.fencing_token,
                report_sha256: &context.report.report_sha256,
                target_release: &released.target.release_tag,
            },
        )
        .map_err(Error::Remote)?;
        let apply_status = apply_native_maintenance_family_release(self.remote, &apply_request)
            .map_err(Error::Remote)?;
        // preserve the source's exact acknowledged install before the outer due journal records it
        let installed = verify_native_maintenance_development_acknowledgement(
            &NativeMaintenanceDevelopmentAcknowledgementInput {
                anchor: &anchor,
                apply_request: &apply_request,
                apply_status: &apply_status,
                installation: &installation,
                now: &NativeMaintenanceArchiveWallClock
                    .now()
                    .ok_or(Error::Clock)?,
            },
        )
        .map_err(Error::Anchor)?;
        Ok(NativeMaintenanceDueRelease {
            action_sha256: installed.action_sha256,
            acknowledged: true,
        })
    }

    /// Publish the real paired model release and its exact development custody archive.
    pub fn publish_pair_and_graph(
        &mut self,
        context: &NativeMaintenanceDueReleaseContext<'_>,
        candidate: &NativeMaintenanceInstalledCandidateOwner,
        lease: &NativeMaintenanceReleaseLease,
    ) -> Result<NativeMaintenanceDevelopmentReleasedGraph, Error> {
        let family = context.due.family.ok_or(Error::Input)?;
        let (parity, registration) = candidate.release_material().ok_or(Error::Candidate)?;
        let (graph_sha256, preregistration) =
            candidate.release_custody().ok_or(Error::Candidate)?;
        let graph_digest = digest(graph_sha256).ok_or(Error::Candidate)?;
        // rebind the genuine active release fence to this attempt and candidate
        if lease.due_key != context.due.due_key
            || lease.input_head_sha256 != context.input_head_sha256
            || context.outcome.candidate_graph_sha256.as_deref() != Some(graph_sha256)
            || context.outcome.candidate_sha256.as_deref()
                != Some(&parity.portable.candidate_sha256)
            || parity.portable.candidate_sha256.len() != 64
            || lease.run_id != format!("release-{}", &parity.portable.candidate_sha256[..32])
        {
            return Err(Error::Input);
        }
        let candidate_graph = self
            .archive
            .verify_full_graph(graph_digest, false)
            .map_err(|_| Error::Archive)?;
        // recover the exact candidate bytes without retaining a second fit snapshot
        let candidate_bytes = read_candidate(
            self.objects,
            &candidate_graph,
            &parity.portable.candidate_sha256,
            family.wire(),
        )?;
        let current = NativeMaintenanceCaptureRemoteAuthority::new(self.remote)
            .read_verified_family_current_full(family)
            .map_err(|_| Error::Current)?;
        let created_at = NativeMaintenanceArchiveWallClock
            .now()
            .ok_or(Error::Clock)?;
        let publication = build_native_maintenance_development_publication(
            NativeMaintenanceDevelopmentPublicationInput {
                candidate_bytes: &candidate_bytes,
                candidate_graph: &candidate_graph,
                confirmation_preregistration: preregistration,
                created_at: &created_at,
                current: &current,
                expected_plan_sha256: &registration.expected_plan.expected_plan_sha256,
                family,
                fencing_token: &lease.fencing_token,
                lifecycle_head_sha256: context.lifecycle_head_sha256,
                parity: &parity.receipt,
                portable: &parity.portable,
                registration: &registration.shadow.registration,
                report: context.report,
                report_created_at: context.report_created_at,
            },
        )
        .map_err(Error::Material)?;
        let mut publisher = NativeMaintenanceModelPublisher {
            journal: self.journal,
            http: self.http,
        };
        let target = publisher
            .publish_development(&publication, &context.due.due_key)
            .map_err(Error::Publish)?;
        // derive compensation on the actual immutable target commit and release, never the previous source
        let compensation_created_at = NativeMaintenanceArchiveWallClock
            .now()
            .ok_or(Error::Clock)?;
        let compensation = build_native_maintenance_development_compensation(
            NativeMaintenanceDevelopmentCompensationInput {
                compensation_created_at: &compensation_created_at,
                target: NativeMaintenanceDevelopmentCompensationTarget {
                    action: &publication.action,
                    action_bytes: &publication.action_bytes,
                    action_sha256: &target.action_sha256,
                    commit_sha: &target.commit_sha,
                    release_tag: &target.release_tag,
                },
            },
        )
        .map_err(Error::Compensation)?;
        let compensation = publisher
            .publish_packaged(NativeMaintenanceModelPublishPackage {
                action: &compensation.action,
                action_sha256: &compensation.action_sha256,
                target_identity: &compensation.target.target_identity_sha256,
                package_root_sha256: &compensation.package_root_sha256,
                package_files: &compensation.package_files,
                removed_paths: &compensation.removed_paths,
                release_date: &publication.release_date,
                release_due_key: &context.due.due_key,
                release_run_id: &publication.release_run_id,
            })
            .map_err(Error::Publish)?;
        publisher
            .record_pair(&publication.action, &target, &compensation)
            .map_err(Error::Publish)?;
        // publish the source seven-member development custody graph under its own archive lease
        let graph = publish_native_maintenance_archive_segment(
            self.archive,
            &mut NativeMaintenanceArchiveJournal::new(self.journal),
            &mut NativeMaintenanceArchiveWallClock,
            &format!(
                "archive/development-custody/{}",
                publication.custody_binding_sha256
            ),
            &publication.custody_segment,
        )
        .map_err(|_| Error::Archive)?;
        let development_graph = self
            .archive
            .verify_full_graph(graph.manifest_object_sha256, false)
            .map_err(|_| Error::Archive)?;
        Ok(NativeMaintenanceDevelopmentReleasedGraph {
            publication,
            current,
            target,
            compensation,
            development_graph,
        })
    }
}

/// Recover one already verified graph member and rebind its immutable bytes.
fn read_candidate(
    objects: &NativeMaintenancePlaintextObjectStore,
    graph: &NativeMaintenanceVerifiedFullGraph,
    candidate_sha256: &str,
    family: &str,
) -> Result<Vec<u8>, Error> {
    let expected_kind = format!("forecast-adjustment-{family}-development-candidate/v1");
    let entry = graph
        .manifest
        .entries
        .iter()
        .find(|entry| entry.identity_sha256 == candidate_sha256 && entry.kind == expected_kind)
        .ok_or(Error::Candidate)?;
    // retain bounded exact original fit bytes without materializing unrelated members
    if entry.member_length == 0 || entry.member_length > CANDIDATE_MAXIMUM_BYTES {
        return Err(Error::Candidate);
    }
    let mut object = objects
        .open_object(&entry.object_sha256)
        .map_err(|_| Error::Archive)?
        .ok_or(Error::Archive)?;
    object
        .seek(SeekFrom::Start(entry.member_offset))
        .map_err(|_| Error::Archive)?;
    let mut bytes = Vec::with_capacity(entry.member_length as usize);
    object
        .take(entry.member_length)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Archive)?;
    if bytes.len() as u64 != entry.member_length
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &NativeWirePrimitives,
            &bytes,
        )) != entry.member_sha256
        || entry.member_sha256 != candidate_sha256
    {
        return Err(Error::Candidate);
    }
    Ok(bytes)
}

/// Decode an already admitted content address without another serialization pass.
fn digest(value: &str) -> Option<[u8; 32]> {
    Some(*RevisionGapSha256::from_lower_hex(value)?.as_bytes())
}
