//! Native source-ordered model publication through the release journal and exact push Check.

use std::path::Path;

use weather_contracts::JsonValue;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::{
    NativeMaintenanceArchiveSegmentClock, NativeMaintenanceArchiveWallClock,
    NativeMaintenanceCaptureFamily, NativeMaintenanceCompensatingModelReleasePrepareInput,
    NativeMaintenanceDevelopmentPublication, NativeMaintenanceDevelopmentPublicationFile,
    NativeMaintenanceJournalStore, NativeMaintenanceModelCheckError,
    NativeMaintenanceModelReleaseGitError, NativeMaintenanceModelReleaseJournalError,
    NativeMaintenanceModelReleasePairInput, NativeMaintenanceModelReleasePrepareInput,
    NativeMaintenanceModelReleaseTagCollisionInput, NativeOutboundHttp, NativeWirePrimitives,
    await_native_maintenance_model_check,
    maintenance_canonical_json::{text, valid_hash},
    native_maintenance_model_release_git::{CommitInput, prepare_commit, remote_ref},
    prepare_native_maintenance_compensating_model_release,
    prepare_native_maintenance_model_release, publish_native_maintenance_model_release_ref,
    record_native_maintenance_model_release_pair,
    record_native_maintenance_model_release_tag_collision,
    select_native_maintenance_model_release_tag,
    verify_native_maintenance_model_release_environment,
};

const REPOSITORY: &str = "/home/ubuntu/weather";

/// Exact published release material, before any family current-settings mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenancePublishedModelRelease {
    pub action_sha256: String,
    pub action_tag: String,
    pub branch: String,
    pub commit_sha: String,
    pub package_root_sha256: String,
    pub release_tag: String,
}

/// Source preparation/publish phase refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceModelPublishError {
    Input,
    Clock,
    Git(NativeMaintenanceModelReleaseGitError),
    Check(NativeMaintenanceModelCheckError),
    Journal(NativeMaintenanceModelReleaseJournalError),
}

use NativeMaintenanceModelPublishError as Error;

impl From<NativeMaintenanceModelReleaseGitError> for Error {
    fn from(value: NativeMaintenanceModelReleaseGitError) -> Self {
        Self::Git(value)
    }
}

impl From<NativeMaintenanceModelReleaseJournalError> for Error {
    fn from(value: NativeMaintenanceModelReleaseJournalError) -> Self {
        Self::Journal(value)
    }
}

/// Borrow the actual release journal and pooled native Check transport.
pub struct NativeMaintenanceModelPublisher<'a> {
    pub journal: &'a mut NativeMaintenanceJournalStore,
    pub http: &'a NativeOutboundHttp,
}

/// Borrow the complete previously verified canonical package operations.
pub(crate) struct NativeMaintenanceModelPublishPackage<'a> {
    pub action: &'a JsonValue,
    pub action_sha256: &'a str,
    pub target_identity: &'a str,
    pub package_root_sha256: &'a str,
    pub package_files: &'a [NativeMaintenanceDevelopmentPublicationFile],
    pub removed_paths: &'a [String],
    pub release_date: &'a str,
    pub release_due_key: &'a str,
    pub release_run_id: &'a str,
}

impl NativeMaintenanceModelPublisher<'_> {
    /// Publish the already built source development target under its durable release fence.
    pub fn publish_development(
        &mut self,
        publication: &NativeMaintenanceDevelopmentPublication,
        due_key: &str,
    ) -> Result<NativeMaintenancePublishedModelRelease, Error> {
        let target = text(&publication.action, "candidateSha256").ok_or(Error::Input)?;
        self.publish_packaged(NativeMaintenanceModelPublishPackage {
            action: &publication.action,
            action_sha256: &publication.action_sha256,
            target_identity: &target,
            package_root_sha256: &publication.package_root_sha256,
            package_files: &publication.package_files,
            removed_paths: &[],
            release_date: &publication.release_date,
            release_due_key: due_key,
            release_run_id: &publication.release_run_id,
        })
    }

    /// Publish the reviewed native rain control target or selector compensation.
    pub fn publish_control_package(
        &mut self,
        publication: &crate::NativeMaintenanceControlReleasePackage,
        release_date: &str,
        due_key: &str,
        run_id: &str,
    ) -> Result<NativeMaintenancePublishedModelRelease, Error> {
        self.publish_packaged(publication.as_publish_package(release_date, due_key, run_id))
    }

    /// Publish exact package operations in the source's immutable preparation, Check, tag sequence.
    pub(crate) fn publish_packaged(
        &mut self,
        package: NativeMaintenanceModelPublishPackage<'_>,
    ) -> Result<NativeMaintenancePublishedModelRelease, Error> {
        let wire = NativeWirePrimitives;
        let repository = Path::new(REPOSITORY);
        let source = text(package.action, "expectedSourceCommit").ok_or(Error::Input)?;
        let source_release = text(package.action, "expectedSourceRelease").ok_or(Error::Input)?;
        let family = family(package.action)?;
        let kind = text(package.action, "actionKind").ok_or(Error::Input)?;
        let fencing_token = text(package.action, "fencingToken").ok_or(Error::Input)?;
        let created_at = text(package.action, "createdAt").ok_or(Error::Input)?;
        let valid_through = text(package.action, "validThrough").ok_or(Error::Input)?;
        let compensation = matches!(
            kind.as_str(),
            "compensate_incumbent"
                | "compensate_raw"
                | "compensate_shadow"
                | "compensate_control_reference"
        );
        // verify the complete immutable source binding before detached mutation
        if !valid_hash(package.action_sha256)
            || !valid_hash(package.target_identity)
            || !valid_hash(package.package_root_sha256)
        {
            return Err(Error::Input);
        }
        verify_native_maintenance_model_release_environment(repository, &source, &source_release)?;
        let start = instant()?;
        let start_ms = wire
            .canonical_utc_milliseconds(&start)
            .ok_or(Error::Clock)?;
        let created_ms = wire
            .canonical_utc_milliseconds(&created_at)
            .ok_or(Error::Clock)?;
        let valid_ms = wire
            .canonical_utc_milliseconds(&valid_through)
            .ok_or(Error::Clock)?;
        // preserve ordinary action validity and source recovery compensation exception
        if start_ms < created_ms || (!compensation && start_ms > valid_ms) {
            return Err(Error::Clock);
        }
        let prepared = prepare_commit(
            repository,
            CommitInput {
                source: &source,
                family: family.wire(),
                target: package.target_identity,
                action_sha256: package.action_sha256,
                created: &created_at,
                files: package.package_files,
                removed: package.removed_paths,
            },
        )?;
        let release_tag = select_native_maintenance_model_release_tag(
            repository,
            package.release_date,
            &prepared.commit_sha,
            1,
        )?;
        let now = instant()?;
        // commit the exact lease-bound identity before attempting any publication
        if compensation {
            let predecessor =
                text(package.action, "predecessorActionSha256").ok_or(Error::Input)?;
            prepare_native_maintenance_compensating_model_release(
                self.journal,
                NativeMaintenanceCompensatingModelReleasePrepareInput {
                    action_sha256: package.action_sha256,
                    action_tag: &prepared.action_tag,
                    branch: &prepared.branch,
                    commit_sha: &prepared.commit_sha,
                    expected_source_commit: &source,
                    family,
                    fencing_token: &fencing_token,
                    now: &now,
                    package_root_sha256: package.package_root_sha256,
                    predecessor_action_sha256: &predecessor,
                    release_due_key: package.release_due_key,
                    release_run_id: package.release_run_id,
                    release_tag: &release_tag,
                },
            )?;
        } else {
            prepare_native_maintenance_model_release(
                self.journal,
                NativeMaintenanceModelReleasePrepareInput {
                    action_sha256: package.action_sha256,
                    action_tag: &prepared.action_tag,
                    branch: &prepared.branch,
                    commit_sha: &prepared.commit_sha,
                    expected_source_commit: &source,
                    family,
                    fencing_token: &fencing_token,
                    now: &now,
                    package_root_sha256: package.package_root_sha256,
                    release_due_key: package.release_due_key,
                    release_run_id: package.release_run_id,
                    release_tag: &release_tag,
                },
            )?;
        }
        publish_native_maintenance_model_release_ref(
            repository,
            &prepared.commit_sha,
            &format!("refs/heads/{}", prepared.branch),
        )?;
        await_native_maintenance_model_check(self.http, &prepared.commit_sha)
            .map_err(Error::Check)?;
        publish_native_maintenance_model_release_ref(
            repository,
            &prepared.commit_sha,
            &format!("refs/tags/{}", prepared.action_tag),
        )?;
        let mut tag = release_tag;
        // retry only the bounded immutable date-tag collisions proved by the remote
        for _ in 0..999 {
            let reference = format!("refs/tags/{tag}");
            let mut existing = remote_ref(repository, &reference)?;
            if existing
                .as_deref()
                .is_none_or(|value| value == prepared.commit_sha)
            {
                match publish_native_maintenance_model_release_ref(
                    repository,
                    &prepared.commit_sha,
                    &reference,
                ) {
                    Ok(()) => {
                        return Ok(NativeMaintenancePublishedModelRelease {
                            action_sha256: package.action_sha256.to_owned(),
                            action_tag: prepared.action_tag,
                            branch: prepared.branch,
                            commit_sha: prepared.commit_sha,
                            package_root_sha256: package.package_root_sha256.to_owned(),
                            release_tag: tag,
                        });
                    }
                    Err(error) => {
                        existing = remote_ref(repository, &reference)?;
                        // do not classify transport failure or an exact retry as a competing commit
                        if existing
                            .as_deref()
                            .is_none_or(|value| value == prepared.commit_sha)
                        {
                            return Err(Error::Git(error));
                        }
                    }
                }
            }
            let attempted_suffix = tag
                .rsplit_once('-')
                .and_then(|(_, suffix)| suffix.parse::<u16>().ok())
                .ok_or(Error::Input)?;
            let next = attempted_suffix
                .checked_add(1)
                .filter(|value| *value <= 999)
                .ok_or(Error::Git(NativeMaintenanceModelReleaseGitError::Namespace))?;
            let successor = select_native_maintenance_model_release_tag(
                repository,
                package.release_date,
                &prepared.commit_sha,
                next,
            )?;
            record_native_maintenance_model_release_tag_collision(
                self.journal,
                NativeMaintenanceModelReleaseTagCollisionInput {
                    action_sha256: package.action_sha256,
                    attempted_release_tag: &tag,
                    collision_commit_sha: existing.as_deref().ok_or(Error::Input)?,
                    family,
                    fencing_token: &fencing_token,
                    now: &instant()?,
                    successor_release_tag: &successor,
                },
            )?;
            tag = successor;
        }
        Err(Error::Git(NativeMaintenanceModelReleaseGitError::Namespace))
    }

    /// Persist the exact target/compensation pair before any family mutation.
    pub fn record_pair(
        &mut self,
        target_action: &JsonValue,
        target: &NativeMaintenancePublishedModelRelease,
        compensation: &NativeMaintenancePublishedModelRelease,
    ) -> Result<(), Error> {
        let fencing_token = text(target_action, "fencingToken").ok_or(Error::Input)?;
        record_native_maintenance_model_release_pair(
            self.journal,
            NativeMaintenanceModelReleasePairInput {
                compensation_action_sha256: &compensation.action_sha256,
                compensation_commit_sha: &compensation.commit_sha,
                compensation_release_tag: &compensation.release_tag,
                family: family(target_action)?,
                fencing_token: &fencing_token,
                now: &instant()?,
                target_action_sha256: &target.action_sha256,
                target_commit_sha: &target.commit_sha,
                target_release_tag: &target.release_tag,
            },
        )?;
        Ok(())
    }
}

/// Sample the original wall clock independently at every journal boundary.
fn instant() -> Result<String, Error> {
    NativeMaintenanceArchiveWallClock.now().ok_or(Error::Clock)
}

/// Resolve the action family to the closed native transport identity.
fn family(action: &JsonValue) -> Result<NativeMaintenanceCaptureFamily, Error> {
    match text(action, "family").as_deref() {
        Some("temperature") => Ok(NativeMaintenanceCaptureFamily::Temperature),
        Some("wind") => Ok(NativeMaintenanceCaptureFamily::Wind),
        Some("rain") => Ok(NativeMaintenanceCaptureFamily::Rain),
        _ => Err(Error::Input),
    }
}
