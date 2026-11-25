//! Worker rain-gate revision binding and permanent-gap database plans.

use weather_contracts::{
    AdjustmentRainGateRevisionAdmissionKey, AdjustmentRainGateRevisionPointer,
    AdjustmentRevisionGapInput, AdjustmentRevisionRowReceipt,
};

use crate::{
    AdjustmentRevisionAdmissionKey, AdjustmentRevisionGapMarker, MaintenancePublicationPlan,
    adjustment_revision_gap_plan,
};

/// Exact single-row rain-gate revision binder.
pub const BIND_WORKER_RAIN_ADJUSTMENT_REVISION_SQL: &str =
    "SELECT weather_bind_rain_gate_revision_v1($1::jsonb) AS value";

/// Typed JSON bind retained for the owning database adapter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorkerRainAdjustmentRevisionBind<'a> {
    PointerJson(&'a AdjustmentRainGateRevisionPointer),
}

/// Exact one-bind rain-gate revision statement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkerRainAdjustmentRevisionPlan<'a> {
    pub sql: &'static str,
    pub binds: [WorkerRainAdjustmentRevisionBind<'a>; 1],
}

/// Named savepoint commands used by the original worker transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerRainAdjustmentRevisionSavepointCommand {
    BindSavepoint,
    BindRollbackTo,
    BindRelease,
    ArchiveSavepoint,
    ArchiveRollbackTo,
    ArchiveRelease,
}

impl WorkerRainAdjustmentRevisionSavepointCommand {
    /// Return the exact PostgreSQL command for this recovery transition.
    pub const fn sql(self) -> &'static str {
        // preserve both original savepoint names and action spellings
        match self {
            Self::BindSavepoint => "SAVEPOINT rain_adjustment_revision_bind",
            Self::BindRollbackTo => "ROLLBACK TO SAVEPOINT rain_adjustment_revision_bind",
            Self::BindRelease => "RELEASE SAVEPOINT rain_adjustment_revision_bind",
            Self::ArchiveSavepoint => "SAVEPOINT rain_adjustment_revision_archive",
            Self::ArchiveRollbackTo => "ROLLBACK TO SAVEPOINT rain_adjustment_revision_archive",
            Self::ArchiveRelease => "RELEASE SAVEPOINT rain_adjustment_revision_archive",
        }
    }
}

/// Retained serving transaction capable of binding one staged rain revision.
pub trait WorkerRainAdjustmentRevisionPort {
    type Error;

    /// Execute one exact named savepoint command on this retained transaction.
    fn execute_rain_revision_savepoint(
        &mut self,
        command: WorkerRainAdjustmentRevisionSavepointCommand,
    ) -> Result<(), Self::Error>;

    /// Execute the typed rain-gate binder and decode every returned row.
    fn bind_rain_gate_revision(
        &mut self,
        plan: WorkerRainAdjustmentRevisionPlan<'_>,
    ) -> Result<Vec<AdjustmentRevisionRowReceipt>, Self::Error>;
}

/// Transaction or pool boundary for the existing permanent-gap statement.
pub trait WorkerRainAdjustmentRevisionGapPort {
    type Error;

    /// Execute the shared revision-gap plan and decode every returned marker.
    fn write_rain_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error>;
}

/// Bind, row-contract, marker, or savepoint recovery failure.
#[derive(Debug, PartialEq)]
pub enum WorkerRainAdjustmentRevisionError<PortError> {
    Port(PortError),
    ReceiptCardinality,
    GapMarkerMismatch,
    Recovery(PortError),
}

/// Bind one staged rain pointer inside the exact recoverable savepoint.
pub fn bind_worker_rain_adjustment_revision<Port>(
    port: &mut Port,
    pointer: &AdjustmentRainGateRevisionPointer,
) -> Result<AdjustmentRevisionRowReceipt, WorkerRainAdjustmentRevisionError<Port::Error>>
where
    Port: WorkerRainAdjustmentRevisionPort,
{
    port.execute_rain_revision_savepoint(
        WorkerRainAdjustmentRevisionSavepointCommand::BindSavepoint,
    )
    .map_err(WorkerRainAdjustmentRevisionError::Port)?;
    let attempted = bind_one_revision(port, pointer).and_then(|receipt| {
        port.execute_rain_revision_savepoint(
            WorkerRainAdjustmentRevisionSavepointCommand::BindRelease,
        )
        .map_err(WorkerRainAdjustmentRevisionError::Port)?;
        Ok(receipt)
    });
    // recover an aborted bind or release before the serving transaction continues
    if attempted.is_err() {
        recover_savepoint(
            port,
            WorkerRainAdjustmentRevisionSavepointCommand::BindRollbackTo,
            WorkerRainAdjustmentRevisionSavepointCommand::BindRelease,
        )?;
    }
    attempted
}

/// Mark one permanent rain revision gap through the shared pool-safe SQL plan.
pub fn mark_worker_rain_adjustment_revision_gap<Port>(
    port: &mut Port,
    key: &AdjustmentRainGateRevisionAdmissionKey,
    gap: &AdjustmentRevisionGapInput,
) -> Result<AdjustmentRevisionGapMarker, WorkerRainAdjustmentRevisionError<Port::Error>>
where
    Port: WorkerRainAdjustmentRevisionGapPort,
{
    let stored_content_sha256 = key.storedContentSha256.as_str();
    let admission_key = AdjustmentRevisionAdmissionKey::RainGate(key.clone());
    let markers = port
        .write_rain_revision_gap(adjustment_revision_gap_plan(&admission_key, gap))
        .map_err(WorkerRainAdjustmentRevisionError::Port)?;
    let [marker]: [AdjustmentRevisionGapMarker; 1] = markers
        .try_into()
        .map_err(|_| WorkerRainAdjustmentRevisionError::ReceiptCardinality)?;
    // require the database marker to retain every supplied disposition field and row identity
    if marker.logical_key_sha256 != gap.logicalKeySha256
        || marker.projection_identity_sha256 != gap.projectionIdentitySha256
        || marker.projection_kind != gap.projectionKind
        || marker.projection_sha256 != gap.projectionSha256
        || marker.reason != gap.reason
        || marker.stored_content_sha256 != stored_content_sha256
    {
        return Err(WorkerRainAdjustmentRevisionError::GapMarkerMismatch);
    }
    Ok(marker)
}

/// Mark one in-transaction gap inside the archive recovery savepoint.
pub fn mark_worker_rain_adjustment_revision_gap_in_transaction<Port>(
    port: &mut Port,
    key: &AdjustmentRainGateRevisionAdmissionKey,
    gap: &AdjustmentRevisionGapInput,
) -> Result<
    AdjustmentRevisionGapMarker,
    WorkerRainAdjustmentRevisionError<<Port as WorkerRainAdjustmentRevisionPort>::Error>,
>
where
    Port: WorkerRainAdjustmentRevisionPort
        + WorkerRainAdjustmentRevisionGapPort<
            Error = <Port as WorkerRainAdjustmentRevisionPort>::Error,
        >,
{
    port.execute_rain_revision_savepoint(
        WorkerRainAdjustmentRevisionSavepointCommand::ArchiveSavepoint,
    )
    .map_err(WorkerRainAdjustmentRevisionError::Port)?;
    let attempted = mark_worker_rain_adjustment_revision_gap(port, key, gap).and_then(|marker| {
        port.execute_rain_revision_savepoint(
            WorkerRainAdjustmentRevisionSavepointCommand::ArchiveRelease,
        )
        .map_err(WorkerRainAdjustmentRevisionError::Port)?;
        Ok(marker)
    });
    // recover marker or release failure so the retained serving row can still commit
    if attempted.is_err() {
        recover_savepoint(
            port,
            WorkerRainAdjustmentRevisionSavepointCommand::ArchiveRollbackTo,
            WorkerRainAdjustmentRevisionSavepointCommand::ArchiveRelease,
        )?;
    }
    attempted
}

/// Execute and cardinality-check the generated rain row binder.
fn bind_one_revision<Port>(
    port: &mut Port,
    pointer: &AdjustmentRainGateRevisionPointer,
) -> Result<AdjustmentRevisionRowReceipt, WorkerRainAdjustmentRevisionError<Port::Error>>
where
    Port: WorkerRainAdjustmentRevisionPort,
{
    let receipts = port
        .bind_rain_gate_revision(WorkerRainAdjustmentRevisionPlan {
            sql: BIND_WORKER_RAIN_ADJUSTMENT_REVISION_SQL,
            binds: [WorkerRainAdjustmentRevisionBind::PointerJson(pointer)],
        })
        .map_err(WorkerRainAdjustmentRevisionError::Port)?;
    let [receipt]: [AdjustmentRevisionRowReceipt; 1] = receipts
        .try_into()
        .map_err(|_| WorkerRainAdjustmentRevisionError::ReceiptCardinality)?;
    Ok(receipt)
}

/// Restore and release one failed named savepoint in source order.
fn recover_savepoint<Port>(
    port: &mut Port,
    rollback: WorkerRainAdjustmentRevisionSavepointCommand,
    release: WorkerRainAdjustmentRevisionSavepointCommand,
) -> Result<(), WorkerRainAdjustmentRevisionError<Port::Error>>
where
    Port: WorkerRainAdjustmentRevisionPort,
{
    port.execute_rain_revision_savepoint(rollback)
        .map_err(WorkerRainAdjustmentRevisionError::Recovery)?;
    port.execute_rain_revision_savepoint(release)
        .map_err(WorkerRainAdjustmentRevisionError::Recovery)
}

#[cfg(test)]
mod tests {
    use weather_contracts::{
        AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptContractVersionLiteral,
        AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionGapReason,
        AdjustmentRevisionProjectionKind, AdjustmentRevisionRowReceiptContractVersionLiteral,
        Nullable,
    };

    use super::*;

    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[derive(Default)]
    struct Port {
        events: Vec<&'static str>,
        fail_bind: bool,
    }

    impl WorkerRainAdjustmentRevisionPort for Port {
        type Error = &'static str;

        /// Record the exact named savepoint transition.
        fn execute_rain_revision_savepoint(
            &mut self,
            command: WorkerRainAdjustmentRevisionSavepointCommand,
        ) -> Result<(), Self::Error> {
            self.events.push(command.sql());
            Ok(())
        }

        /// Assert the exact SQL and typed pointer before returning one row.
        fn bind_rain_gate_revision(
            &mut self,
            plan: WorkerRainAdjustmentRevisionPlan<'_>,
        ) -> Result<Vec<AdjustmentRevisionRowReceipt>, Self::Error> {
            self.events.push("bind");
            assert_eq!(plan.sql, BIND_WORKER_RAIN_ADJUSTMENT_REVISION_SQL);
            let [WorkerRainAdjustmentRevisionBind::PointerJson(pointer)] = plan.binds;
            assert_eq!(pointer, &pointer_fixture());
            // emulate one ordinary aborted binder
            if self.fail_bind {
                return Err("bind failed");
            }
            Ok(vec![receipt_fixture()])
        }
    }

    impl WorkerRainAdjustmentRevisionGapPort for Port {
        type Error = &'static str;

        /// Return one exact marker for the shared gap plan.
        fn write_rain_revision_gap(
            &mut self,
            plan: MaintenancePublicationPlan<'_, 3>,
        ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
            self.events.push("gap");
            assert!(plan.sql.contains("weather_mark_adjustment_revision_gap_v1"));
            Ok(vec![marker_fixture()])
        }
    }

    /// Verify successful binding and failed binding recovery use the exact source commands.
    #[test]
    fn binds_and_recovers_with_the_named_savepoint() {
        let mut success = Port::default();
        bind_worker_rain_adjustment_revision(&mut success, &pointer_fixture())
            .expect("revision bind");
        assert_eq!(
            success.events,
            [
                "SAVEPOINT rain_adjustment_revision_bind",
                "bind",
                "RELEASE SAVEPOINT rain_adjustment_revision_bind",
            ]
        );

        let mut failed = Port {
            fail_bind: true,
            ..Port::default()
        };
        assert_eq!(
            bind_worker_rain_adjustment_revision(&mut failed, &pointer_fixture()),
            Err(WorkerRainAdjustmentRevisionError::Port("bind failed"))
        );
        assert_eq!(
            failed.events,
            [
                "SAVEPOINT rain_adjustment_revision_bind",
                "bind",
                "ROLLBACK TO SAVEPOINT rain_adjustment_revision_bind",
                "RELEASE SAVEPOINT rain_adjustment_revision_bind",
            ]
        );
    }

    /// Build one typed staged rain revision pointer.
    fn pointer_fixture() -> AdjustmentRainGateRevisionPointer {
        AdjustmentRainGateRevisionPointer {
            inputSha256: HASH_A.to_owned(),
            modelSha256: HASH_B.to_owned(),
            projectionIdentitySha256: HASH_A.to_owned(),
            projectionSha256: HASH_A.to_owned(),
            runInitializedAt: "2026-11-24T00:00:00.000Z".to_owned(),
            stageReceiptSha256: HASH_B.to_owned(),
            storedContentSha256: HASH_A.to_owned(),
        }
    }

    /// Build one decoded row receipt fixture.
    fn receipt_fixture() -> AdjustmentRevisionRowReceipt {
        AdjustmentRevisionRowReceipt {
            contractVersion: AdjustmentRevisionRowReceiptContractVersionLiteral,
            revisionReceipt: AdjustmentRevisionCommitReceipt {
                archiveCommitOrdinal: "1".to_owned(),
                archiveCommittedAt: "2026-11-24T00:01:00.000Z".to_owned(),
                contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
                frontierSha256: HASH_A.to_owned(),
                predecessorFrontierSha256: HASH_B.to_owned(),
                projectionIdentitySha256: HASH_A.to_owned(),
                projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput,
                projectionSha256: HASH_A.to_owned(),
                receiptSha256: HASH_B.to_owned(),
                stageReceiptSha256: HASH_B.to_owned(),
            },
        }
    }

    /// Build one closed rain admission key.
    fn key_fixture() -> AdjustmentRainGateRevisionAdmissionKey {
        AdjustmentRainGateRevisionAdmissionKey {
            inputSha256: HASH_A.to_owned(),
            modelSha256: HASH_B.to_owned(),
            runInitializedAt: "2026-11-24T00:00:00.000Z".to_owned(),
            storedContentSha256: HASH_A.to_owned(),
        }
    }

    /// Build one permanent rain bind disposition.
    fn gap_fixture() -> AdjustmentRevisionGapInput {
        AdjustmentRevisionGapInput {
            logicalKeySha256: HASH_B.to_owned(),
            projectionIdentitySha256: Nullable::Value(HASH_A.to_owned()),
            projectionKind: AdjustmentRevisionProjectionKind::RainGateInput,
            projectionSha256: Nullable::Value(HASH_A.to_owned()),
            reason: AdjustmentRevisionGapReason::DatabaseBindFailed,
        }
    }

    /// Build one exact decoded marker for the fixture key and gap.
    fn marker_fixture() -> AdjustmentRevisionGapMarker {
        let gap = gap_fixture();
        AdjustmentRevisionGapMarker {
            logical_key_sha256: gap.logicalKeySha256,
            projection_identity_sha256: gap.projectionIdentitySha256,
            projection_kind: gap.projectionKind,
            projection_sha256: gap.projectionSha256,
            reason: gap.reason,
            stored_content_sha256: HASH_A.to_owned(),
        }
    }

    /// Verify the transaction gap path uses the archive savepoint and shared marker plan.
    #[test]
    fn marks_transaction_gap_inside_archive_savepoint() {
        let mut port = Port::default();
        mark_worker_rain_adjustment_revision_gap_in_transaction(
            &mut port,
            &key_fixture(),
            &gap_fixture(),
        )
        .expect("gap marker");
        assert_eq!(
            port.events,
            [
                "SAVEPOINT rain_adjustment_revision_archive",
                "gap",
                "RELEASE SAVEPOINT rain_adjustment_revision_archive",
            ]
        );
    }
}
