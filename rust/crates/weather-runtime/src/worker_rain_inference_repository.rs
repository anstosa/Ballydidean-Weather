//! Execute the scheduled rain inference repository through the retained ingest pool.

use weather_contracts::{
    AdjustmentRainGateRevisionAdmissionKey, AdjustmentRainGateRevisionPointer,
    AdjustmentRevisionGapInput, AdjustmentRevisionGapReason, AdjustmentRevisionProjectionKind,
    Nullable, PersistedRainAdjustmentRevision, RainAdjustmentHour as ContractHour,
    RainAdjustmentRun as ContractRun,
};
use weather_database::{
    OpenWorkerRainAdjustment, WorkerRainAdjustmentAppendPool, WorkerRainAdjustmentAppendValues,
    WorkerRainAdjustmentReadPort, WorkerRainAdjustmentReadValues,
    WorkerRainAdjustmentRevisionGapPort, WorkerRainAdjustmentRevisionPort,
    WorkerRainAdjustmentTransaction, begin_worker_rain_adjustment_append,
    bind_worker_rain_adjustment_revision, mark_worker_rain_adjustment_revision_gap,
    mark_worker_rain_adjustment_revision_gap_in_transaction, read_pending_rain_adjustment_captures,
};
use weather_worker::{
    RainAdjustmentAppendTransaction, RainAdjustmentCommitReceipt, RainAdjustmentGap,
    RainAdjustmentGapReason, RainAdjustmentHour, RainAdjustmentHourReason,
    RainAdjustmentPendingInput, RainAdjustmentPersistedRevision, RainAdjustmentRepository,
    RainAdjustmentRevisionPlan, RainAdjustmentRun, RainAdjustmentStageReceipt, TaskFailure,
};

use crate::{
    WorkerRainAdjustmentCaptureValues, WorkerRainInferenceRepositoryBinding,
    project_worker_rain_adjustment_inputs,
};

/// Reborrow the same worker ingest pool for each eligible inference.
pub struct DatabaseRainInferenceBinding;

/// The exact rain inference read, serving row, and optional revision transaction.
pub struct DatabaseRainInferenceRepository<'a, Database, Values> {
    database: &'a mut Database,
    values: &'a mut Values,
}

impl<Database, Values> WorkerRainInferenceRepositoryBinding<Database, Values>
    for DatabaseRainInferenceBinding
where
    Database: WorkerRainAdjustmentReadPort
        + WorkerRainAdjustmentAppendPool
        + WorkerRainAdjustmentRevisionGapPort,
    Database::Session: WorkerRainAdjustmentRevisionPort<
            Error = <Database as WorkerRainAdjustmentAppendPool>::Error,
        > + WorkerRainAdjustmentRevisionGapPort<
            Error = <Database as WorkerRainAdjustmentAppendPool>::Error,
        >,
    Values: WorkerRainAdjustmentReadValues
        + WorkerRainAdjustmentAppendValues
        + WorkerRainAdjustmentCaptureValues,
{
    type Repository<'a>
        = DatabaseRainInferenceRepository<'a, Database, Values>
    where
        Database: 'a,
        Values: 'a;

    /// Borrow both approved owners without creating a second database or value owner.
    fn bind<'a>(
        &'a mut self,
        database: &'a mut Database,
        values: &'a mut Values,
    ) -> Self::Repository<'a> {
        DatabaseRainInferenceRepository { database, values }
    }
}

impl<Database, Values> RainAdjustmentRepository
    for DatabaseRainInferenceRepository<'_, Database, Values>
where
    Database: WorkerRainAdjustmentReadPort
        + WorkerRainAdjustmentAppendPool
        + WorkerRainAdjustmentRevisionGapPort,
    Database::Session: WorkerRainAdjustmentRevisionPort<
            Error = <Database as WorkerRainAdjustmentAppendPool>::Error,
        > + WorkerRainAdjustmentRevisionGapPort<
            Error = <Database as WorkerRainAdjustmentAppendPool>::Error,
        >,
    Values: WorkerRainAdjustmentReadValues
        + WorkerRainAdjustmentAppendValues
        + WorkerRainAdjustmentCaptureValues,
{
    type Transaction = WorkerRainAdjustmentTransaction<Database::Session>;

    /// Query and decode exactly the newest eligible immutable causal source.
    fn read_pending(&mut self, model: &str) -> Result<RainAdjustmentPendingInput, TaskFailure> {
        let captures = read_pending_rain_adjustment_captures(self.database, self.values, model)
            .map_err(|_| failure("rain_adjustment_read_failed"))?;
        project_worker_rain_adjustment_inputs(self.values, captures)
    }

    /// Insert the original public-safe run and retain PostgreSQL's exact stored revision.
    fn begin_append(
        &mut self,
        run: &RainAdjustmentRun,
    ) -> Result<Option<RainAdjustmentAppendTransaction<Self::Transaction>>, TaskFailure> {
        let run = contract_run(run).ok_or_else(|| failure("rain_adjustment_run_invalid"))?;
        let open = begin_worker_rain_adjustment_append(self.database, self.values, &run)
            .map_err(|_| failure("rain_adjustment_append_failed"))?;
        // a conflict has already committed and released the empty transaction
        let Some(OpenWorkerRainAdjustment {
            persisted,
            transaction,
        }) = open
        else {
            return Ok(None);
        };
        let persisted = persisted_revision(persisted)
            .ok_or_else(|| failure("rain_adjustment_append_receipt_invalid"))?;
        Ok(Some(RainAdjustmentAppendTransaction {
            persisted,
            transaction,
        }))
    }

    /// Bind the exact staged revision through the same open serving session.
    fn bind_revision(
        &mut self,
        transaction: &mut Self::Transaction,
        persisted: &RainAdjustmentPersistedRevision,
        plan: &RainAdjustmentRevisionPlan,
        staged: &RainAdjustmentStageReceipt,
    ) -> Result<RainAdjustmentCommitReceipt, TaskFailure> {
        let session = transaction
            .session_mut()
            .ok_or_else(|| failure("rain_adjustment_revision_session_closed"))?;
        let pointer = AdjustmentRainGateRevisionPointer {
            inputSha256: persisted.input_sha256.clone(),
            modelSha256: persisted.model_sha256.clone(),
            projectionIdentitySha256: plan.projection.projection_identity_sha256.clone(),
            projectionSha256: plan.projection.projection_identity_sha256.clone(),
            runInitializedAt: persisted.run_initialized_at.clone(),
            stageReceiptSha256: staged.stageReceiptSha256.clone(),
            storedContentSha256: persisted.stored_content_sha256.clone(),
        };
        bind_worker_rain_adjustment_revision(session, &pointer)
            .map(|value| value.revisionReceipt)
            .map_err(|_| failure("rain_adjustment_revision_bind_failed"))
    }

    /// Keep an optional revision gap failure out of the serving transaction.
    fn mark_revision_gap_in_transaction(
        &mut self,
        transaction: &mut Self::Transaction,
        persisted: &RainAdjustmentPersistedRevision,
        gap: &RainAdjustmentGap,
    ) -> Result<(), TaskFailure> {
        let session = transaction
            .session_mut()
            .ok_or_else(|| failure("rain_adjustment_revision_session_closed"))?;
        mark_worker_rain_adjustment_revision_gap_in_transaction(
            session,
            &revision_key(persisted),
            &revision_gap(gap),
        )
        .map(|_| ())
        .map_err(|_| failure("rain_adjustment_revision_gap_failed"))
    }

    /// Commit then release the retained client before any external archive publication.
    fn commit_append(&mut self, transaction: &mut Self::Transaction) -> Result<(), TaskFailure> {
        transaction
            .commit()
            .map_err(|_| failure("rain_adjustment_commit_failed"))
    }

    /// Roll back the open serving transaction and return its session.
    fn rollback_append(&mut self, transaction: &mut Self::Transaction) {
        transaction.rollback();
    }

    /// Record only the matching permanent post-commit gap through the ingest pool.
    fn mark_revision_gap_after_commit(
        &mut self,
        persisted: &RainAdjustmentPersistedRevision,
        gap: &RainAdjustmentGap,
    ) -> Result<(), TaskFailure> {
        mark_worker_rain_adjustment_revision_gap(
            self.database,
            &revision_key(persisted),
            &revision_gap(gap),
        )
        .map(|_| ())
        .map_err(|_| failure("rain_adjustment_revision_gap_failed"))
    }
}

/// Preserve the serving contract properties without converting floats to untyped JSON.
fn contract_run(run: &RainAdjustmentRun) -> Option<ContractRun> {
    let hours = run.hours.iter().map(contract_hour).collect::<Option<_>>()?;
    Some(ContractRun {
        runInitializedAt: run.run_initialized_at.clone(),
        modelSha256: run.model_sha256.clone(),
        inputSha256: run.input_sha256.clone(),
        forecastClaimId: run.forecast_claim_id.clone(),
        firstReceivedAt: run.first_received_at.clone(),
        decisionAt: run.decision_at.clone(),
        generatedAt: run.generated_at.clone(),
        hours,
    })
}

/// Keep the saved model lead within the generated safe integer contract.
fn contract_hour(hour: &RainAdjustmentHour) -> Option<ContractHour> {
    // refuse numeric values that could be rounded or saturated by the DTO conversion
    if !(0.0..=9_007_199_254_740_991.0).contains(&hour.model_lead_hours)
        || hour.model_lead_hours.fract() != 0.0
    {
        return None;
    }
    Some(ContractHour {
        validAt: hour.valid_at.clone(),
        modelLeadHours: hour.model_lead_hours as u64,
        rawPrecipitationMm: hour.raw_precipitation_mm,
        correctedPrecipitationMm: hour.corrected_precipitation_mm,
        applied: hour.applied,
        reasonCode: hour.reason.map_or(Nullable::Null, |reason| {
            Nullable::Value(reason.code().to_owned())
        }),
    })
}

/// Move the exact DB-returned revision into the worker archive sequence.
fn persisted_revision(
    value: PersistedRainAdjustmentRevision,
) -> Option<RainAdjustmentPersistedRevision> {
    Some(RainAdjustmentPersistedRevision {
        generated_at: value.generatedAt,
        hours: value
            .hours
            .into_iter()
            .map(persisted_hour)
            .collect::<Option<_>>()?,
        input_sha256: value.inputSha256,
        model_sha256: value.modelSha256,
        run_initialized_at: value.runInitializedAt,
        stored_content_sha256: value.storedContentSha256,
    })
}

/// Decode the closed reason codes emitted by the immutable serving inference.
fn persisted_hour(hour: ContractHour) -> Option<RainAdjustmentHour> {
    let reason = match hour.reasonCode {
        Nullable::Null => None,
        Nullable::Value(code) => match code.as_str() {
            "phase_unsupported" => Some(RainAdjustmentHourReason::PhaseUnsupported),
            "prediction_invalid" => Some(RainAdjustmentHourReason::PredictionInvalid),
            "policy_raw" => Some(RainAdjustmentHourReason::PolicyRaw),
            _ => return None,
        },
    };
    Some(RainAdjustmentHour {
        applied: hour.applied,
        corrected_precipitation_mm: hour.correctedPrecipitationMm,
        model_lead_hours: hour.modelLeadHours as f64,
        raw_precipitation_mm: hour.rawPrecipitationMm,
        reason,
        valid_at: hour.validAt,
    })
}

/// Address only the retained server-issued serving row.
fn revision_key(row: &RainAdjustmentPersistedRevision) -> AdjustmentRainGateRevisionAdmissionKey {
    AdjustmentRainGateRevisionAdmissionKey {
        inputSha256: row.input_sha256.clone(),
        modelSha256: row.model_sha256.clone(),
        runInitializedAt: row.run_initialized_at.clone(),
        storedContentSha256: row.stored_content_sha256.clone(),
    }
}

/// Convert the selected gap into the existing revision marker contract.
fn revision_gap(gap: &RainAdjustmentGap) -> AdjustmentRevisionGapInput {
    AdjustmentRevisionGapInput {
        logicalKeySha256: gap.logical_key_sha256.clone(),
        projectionIdentitySha256: gap
            .projection_identity_sha256
            .clone()
            .map_or(Nullable::Null, Nullable::Value),
        projectionKind: AdjustmentRevisionProjectionKind::RainGateInput,
        projectionSha256: gap
            .projection_sha256
            .clone()
            .map_or(Nullable::Null, Nullable::Value),
        reason: match gap.reason {
            RainAdjustmentGapReason::ArchiveStageFailed => {
                AdjustmentRevisionGapReason::ArchiveStageFailed
            }
            RainAdjustmentGapReason::DatabaseBindFailed => {
                AdjustmentRevisionGapReason::DatabaseBindFailed
            }
            RainAdjustmentGapReason::ArchivePublishFailed => {
                AdjustmentRevisionGapReason::ArchivePublishFailed
            }
        },
    }
}

/// Keep errors categorical without exposing a provider body or database key.
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
