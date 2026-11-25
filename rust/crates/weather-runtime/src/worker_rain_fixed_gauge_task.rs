//! Execute grouped fixed-gauge target custody through the retained worker sidecar pool.

use std::sync::Arc;
use weather_api::AdmittedAdjustmentRevisionCaptureWitness;
use weather_contracts::{AdjustmentRevisionGapReason, RainFixedGaugeTargetRevisionResult};
use weather_database::{
    WorkerRainFixedGaugeReadPort, WorkerRainFixedGaugeReadValues, WorkerRainFixedGaugeWritePool,
    commit_worker_rain_fixed_gauge_revision_group, mark_worker_rain_fixed_gauge_revision_group_gap,
    read_pending_rain_fixed_gauge_target_hour, read_rain_fixed_gauge_target_captures_for_hour,
    read_rain_fixed_gauge_target_source_catalog,
};
use weather_worker::{
    FixedGaugeTargetResult, RainFixedGaugeCapture, RainFixedGaugeClient, RainFixedGaugeGapReason,
    RainFixedGaugeIdentityAdapter, RainFixedGaugeProjectionBuilder, RainFixedGaugeRepository,
    RainFixedGaugeRevisionGap, RainFixedGaugeRevisionGroup, RainFixedGaugeRevisionResult,
    RainFixedGaugeSource, RainFixedGaugeSourceCatalog, RainFixedGaugeTargetExecution, TaskFailure,
    publish_rain_fixed_gauge_target,
};

use crate::worker_rain_fixed_gauge_database_values::FixedGaugeDatabaseValues;
use crate::worker_rain_fixed_gauge_wire::{
    received_target_gap, received_target_receipt, target_group,
};
use crate::{
    ApiMaintenanceShadowEvidenceValues, HttpWorkerRainMaintenanceClient,
    NativeWorkerRainFixedGaugeProjection, RetainedWorkerSidecarTask, WorkerRainFixedGaugeIdentity,
    WorkerRainFixedGaugeIdentityValues, WorkerRainMaintenanceOriginOwner,
};

/// One scheduler-owned target task with the original verified epoch and approved owners.
pub struct WorkerRainFixedGaugeTask<Builder, Identity, Client> {
    builder: Builder,
    identity: Identity,
    client: Client,
    capture_epoch: Arc<AdmittedAdjustmentRevisionCaptureWitness>,
}

/// Fixed-gauge task poised to use the original archive relay and retained ingest pool.
pub type StartedWorkerRainFixedGauge<ProjectionValues, IdentityValues, Http> =
    WorkerRainFixedGaugeTask<
        NativeWorkerRainFixedGaugeProjection<ProjectionValues>,
        WorkerRainFixedGaugeIdentity<IdentityValues>,
        HttpWorkerRainMaintenanceClient<Http>,
    >;

/// Prepare the existing sidecar on the already configured private origin and verified epoch.
pub fn prepare_worker_rain_fixed_gauge_target<ProjectionValues, IdentityValues, Http, Origin>(
    origin: &str,
    origin_owner: &mut Origin,
    capture_epoch: Arc<AdmittedAdjustmentRevisionCaptureWitness>,
    projection_values: ProjectionValues,
    identity_values: Arc<IdentityValues>,
    http: Http,
) -> Result<StartedWorkerRainFixedGauge<ProjectionValues, IdentityValues, Http>, Origin::Error>
where
    Origin: WorkerRainMaintenanceOriginOwner,
{
    let client = HttpWorkerRainMaintenanceClient::admit(origin, origin_owner, http)?;
    Ok(WorkerRainFixedGaugeTask::new(
        NativeWorkerRainFixedGaugeProjection::new(projection_values),
        WorkerRainFixedGaugeIdentity::new(identity_values),
        client,
        capture_epoch,
    ))
}

impl<Builder, Identity, Client> WorkerRainFixedGaugeTask<Builder, Identity, Client> {
    /// Retain admitted owners without querying any raw source or private route at startup.
    pub const fn new(
        builder: Builder,
        identity: Identity,
        client: Client,
        capture_epoch: Arc<AdmittedAdjustmentRevisionCaptureWitness>,
    ) -> Self {
        Self {
            builder,
            identity,
            client,
            capture_epoch,
        }
    }
}

impl<Database, Values, Builder, Identity, Client> RetainedWorkerSidecarTask<Database, Values, ()>
    for WorkerRainFixedGaugeTask<Builder, Identity, Client>
where
    Database: WorkerRainFixedGaugeReadPort + WorkerRainFixedGaugeWritePool,
    Values: WorkerRainFixedGaugeReadValues
        + WorkerRainFixedGaugeIdentityValues
        + ApiMaintenanceShadowEvidenceValues,
    Builder: RainFixedGaugeProjectionBuilder,
    Identity: RainFixedGaugeIdentityAdapter,
    Client: RainFixedGaugeClient,
{
    type Output = FixedGaugeTargetResult;

    /// Execute one closed-hour target through the existing retained ingest pool.
    fn run(
        &mut self,
        (): (),
        database: &mut Database,
        values: &mut Values,
    ) -> Result<Self::Output, TaskFailure> {
        let mut repository = DatabaseWorkerRainFixedGaugeRepository { database, values };
        publish_rain_fixed_gauge_target(&mut RainFixedGaugeTargetExecution {
            builder: &mut self.builder,
            capture_epoch_at: &self.capture_epoch.capture_epoch.utc,
            client: &mut self.client,
            identity: &mut self.identity,
            repository: &mut repository,
        })
    }
}

struct DatabaseWorkerRainFixedGaugeRepository<'a, Database, Values> {
    database: &'a mut Database,
    values: &'a mut Values,
}

impl<Database, Values> RainFixedGaugeRepository
    for DatabaseWorkerRainFixedGaugeRepository<'_, Database, Values>
where
    Database: WorkerRainFixedGaugeReadPort + WorkerRainFixedGaugeWritePool,
    Values: WorkerRainFixedGaugeReadValues
        + WorkerRainFixedGaugeIdentityValues
        + ApiMaintenanceShadowEvidenceValues,
{
    /// Admit the immutable dedicated source catalog before any target work.
    fn read_source_catalog(&mut self) -> Result<RainFixedGaugeSourceCatalog, TaskFailure> {
        let catalog = read_rain_fixed_gauge_target_source_catalog(self.database)
            .map_err(|_| failure("rain_fixed_gauge_catalog_failed"))?;
        Ok(RainFixedGaugeSourceCatalog {
            catalog_sha256: catalog.catalogSha256,
            contract_version: weather_contracts::RainFixedGaugeTargetSourceCatalogContractVersionLiteral::WIRE_VALUE.to_owned(),
            run_contract_sha256: catalog.runContractSha256,
            sources: catalog.sources.into_iter().map(|source| RainFixedGaugeSource {
                location_id: source.locationId,
                source_config_fingerprint: source.sourceConfigFingerprint,
                source_id: source.sourceId,
                source_key: source.sourceKey,
            }).collect(),
        })
    }

    /// Select one pending hour without reading provider bodies.
    fn read_pending_hour(&mut self, epoch: &str) -> Result<Option<String>, TaskFailure> {
        read_pending_rain_fixed_gauge_target_hour(self.database, self.values, epoch)
            .map_err(|_| failure("rain_fixed_gauge_pending_failed"))
    }

    /// Recheck the hour and move its ordered verified source bodies into replay.
    fn read_captures_for_hour(
        &mut self,
        epoch: &str,
        valid_at: &str,
    ) -> Result<Vec<RainFixedGaugeCapture>, TaskFailure> {
        let captures = read_rain_fixed_gauge_target_captures_for_hour(
            self.database,
            self.values,
            epoch,
            valid_at,
        )
        .map_err(|_| failure("rain_fixed_gauge_capture_failed"))?;
        Ok(captures
            .into_iter()
            .map(|capture| RainFixedGaugeCapture {
                body: capture.body,
                body_sha256: capture.bodySha256,
                claim_id: capture.claimId,
                completed_at: capture.completedAt,
                station_id: capture.stationId,
                window_end_exclusive: capture.windowEndExclusive,
                window_start: capture.windowStart,
            })
            .collect())
    }

    /// Commit exactly one group and preserve PostgreSQL's ordered receipt chain.
    fn commit_revision_group(
        &mut self,
        group: &RainFixedGaugeRevisionGroup,
    ) -> Result<RainFixedGaugeRevisionResult, TaskFailure> {
        let group = target_group(group).ok_or_else(|| failure("rain_fixed_gauge_group_invalid"))?;
        let stored = commit_worker_rain_fixed_gauge_revision_group(
            self.database,
            &mut FixedGaugeDatabaseValues(self.values),
            &group,
        )
        .map_err(|_| failure("rain_fixed_gauge_commit_failed"))?;
        let projected = match stored {
            RainFixedGaugeTargetRevisionResult::Committed(result) => result
                .revisionReceipts
                .into_iter()
                .map(received_target_receipt)
                .collect::<Option<Vec<_>>>()
                .map(RainFixedGaugeRevisionResult::Committed),
            RainFixedGaugeTargetRevisionResult::Gap(result) => {
                received_target_gap(result.gap).map(RainFixedGaugeRevisionResult::Gap)
            }
        };
        projected.ok_or_else(|| failure("rain_fixed_gauge_commit_receipt_invalid"))
    }

    /// Mark the exact already-committed whole group without overwriting an earlier category.
    fn mark_revision_group_gap(
        &mut self,
        group: &RainFixedGaugeRevisionGroup,
        reason: RainFixedGaugeGapReason,
    ) -> Result<RainFixedGaugeRevisionGap, TaskFailure> {
        let kind = match reason {
            RainFixedGaugeGapReason::ArchivePublishFailed => {
                AdjustmentRevisionGapReason::ArchivePublishFailed
            }
            RainFixedGaugeGapReason::DatabaseAdmissionFailed => {
                AdjustmentRevisionGapReason::DatabaseAdmissionFailed
            }
            _ => return Err(failure("rain_fixed_gauge_gap_invalid")),
        };
        let group = target_group(group).ok_or_else(|| failure("rain_fixed_gauge_group_invalid"))?;
        let gap = mark_worker_rain_fixed_gauge_revision_group_gap(
            self.database,
            &mut FixedGaugeDatabaseValues(self.values),
            &group,
            kind,
        )
        .map_err(|_| failure("rain_fixed_gauge_gap_failed"))?;
        received_target_gap(gap).ok_or_else(|| failure("rain_fixed_gauge_gap_invalid"))
    }
}

/// Keep diagnostics categorical and independent of raw capture data.
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
