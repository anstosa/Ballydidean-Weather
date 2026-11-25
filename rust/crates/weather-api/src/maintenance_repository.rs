//! Live API-role query plans behind the typed maintenance publication service.

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptProjectionKindEnum,
    AdjustmentRevisionGapInput, AdjustmentRevisionProjectionKind, AdjustmentRevisionStageReceipt,
    AdjustmentShadowRegistrationSlot,
};
use weather_database::{
    AdjustmentMaintenanceDatabasePort, AdjustmentRevisionAdmissionKey,
    AdjustmentShadowRegistrationSlotQueryPlan, adjustment_revision_gap_plan,
    adjustment_revision_publication_plan, adjustment_shadow_publication_plans,
};

use crate::{
    AdjustmentMaintenanceOperationError, AdjustmentMaintenancePublishRepository,
    AdjustmentMaintenanceScheduleRepository, AdmittedAdjustmentMaintenancePublish,
    PreparedAdjustmentMaintenancePublish, PreparedAdjustmentRevisionPublish,
};

type OperationError = AdjustmentMaintenanceOperationError;

/// PostgreSQL-backed implementation over the admitted API-pool query port.
pub struct DatabaseAdjustmentMaintenanceRepository<P> {
    database: P,
}

impl<P> DatabaseAdjustmentMaintenanceRepository<P> {
    /// Bind a database handle scoped to the existing maintenance application pool.
    pub const fn new(database: P) -> Self {
        Self { database }
    }

    /// Return the database handle to its owning runtime.
    pub fn into_database(self) -> P {
        self.database
    }

    /// Reborrow the same retained repository handle for the API write facades.
    pub fn database_mut(&mut self) -> &mut P {
        &mut self.database
    }
}

/// Distinguish the initial database read from an explicit postcommit admission refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowPublishAdmissionError {
    PreliminaryRead,
    NotAdmitted,
}

impl<P: AdjustmentMaintenanceDatabasePort> DatabaseAdjustmentMaintenanceRepository<P> {
    /// Preserve the capture pipeline's distinct gap outcomes without exposing database details.
    pub fn admit_shadow_publish_classified(
        &mut self,
        prepared: PreparedAdjustmentMaintenancePublish,
    ) -> Result<AdmittedAdjustmentMaintenancePublish, ShadowPublishAdmissionError> {
        let mut relay_input = prepared.relay_input;
        let [body, receipt] = adjustment_shadow_publication_plans(&relay_input.metadata)
            .ok_or(ShadowPublishAdmissionError::PreliminaryRead)?;
        let admitted = sole_row(self.database.read_shadow_body_admission(body))
            .map_err(|_| ShadowPublishAdmissionError::PreliminaryRead)?;
        // do not read a receipt until the server confirms the compact body
        if !admitted {
            return Err(ShadowPublishAdmissionError::NotAdmitted);
        }
        let persisted = sole_row(self.database.read_shadow_revision_receipt(receipt))
            .map_err(|_| ShadowPublishAdmissionError::NotAdmitted)?;
        // require reconstruction against the very same persisted receipt
        if persisted != relay_input.revisionReceipt {
            return Err(ShadowPublishAdmissionError::NotAdmitted);
        }
        relay_input.revisionReceipt = persisted;
        Ok(AdmittedAdjustmentMaintenancePublish { relay_input })
    }
}

impl<P: AdjustmentMaintenanceDatabasePort> AdjustmentMaintenanceScheduleRepository
    for DatabaseAdjustmentMaintenanceRepository<P>
{
    /// Decode the fixed cohort after the database owner has observed all three reads.
    fn schedule_slots(
        &mut self,
        plans: [AdjustmentShadowRegistrationSlotQueryPlan; 3],
    ) -> Result<[AdjustmentShadowRegistrationSlot; 3], OperationError> {
        let rows = self
            .database
            .read_schedule_slots(plans)
            .map_err(|_| OperationError::Rejected)?;
        // validate every returned member before propagating ordered cardinality failures
        let [temperature, wind, rain] = rows.map(sole_decoded_row);
        Ok([temperature?, wind?, rain?])
    }

    /// Decode exactly one typed registration slot for each requested family.
    fn schedule_slot(
        &mut self,
        plan: AdjustmentShadowRegistrationSlotQueryPlan,
    ) -> Result<AdjustmentShadowRegistrationSlot, OperationError> {
        sole_row(self.database.read_schedule_slot(plan))
    }
}

impl<P: AdjustmentMaintenanceDatabasePort> AdjustmentMaintenancePublishRepository
    for DatabaseAdjustmentMaintenanceRepository<P>
{
    /// Verify the compact body before reading its authoritative revision receipt.
    fn admit_shadow_publish(
        &mut self,
        prepared: PreparedAdjustmentMaintenancePublish,
    ) -> Result<AdmittedAdjustmentMaintenancePublish, OperationError> {
        self.admit_shadow_publish_classified(prepared)
            .map_err(|_| OperationError::Rejected)
    }

    /// Rebind the single or grouped body to every exact persisted current pointer.
    fn admit_revision_publish(
        &mut self,
        prepared: &PreparedAdjustmentRevisionPublish,
    ) -> Result<(), OperationError> {
        let (pointers, receipts) = prepared.rows();
        // enforce exact cardinality before issuing any current-pointer lookup
        if pointers.is_empty() || pointers.len() != receipts.len() {
            return Err(OperationError::Rejected);
        }
        let mut exact = true;
        let stage = prepared.stage_receipt();
        // read every decoded row in document order before comparing the result set
        for (pointer, expected) in pointers.iter().zip(receipts) {
            let plan = adjustment_revision_publication_plan(&pointer.key);
            let persisted = sole_row(self.database.read_current_revision_receipt(plan))?;
            exact &= persisted == *expected && receipt_matches_stage(&persisted, stage);
        }
        // equal row values are interchangeable with the already-admitted caller receipt
        if !exact || !receipts.windows(2).all(receipts_continue_frontier) {
            return Err(OperationError::Rejected);
        }
        Ok(())
    }

    /// Verify one permanent database marker before cold gap archival.
    fn mark_revision_gap(
        &mut self,
        pointer: &AdjustmentRevisionAdmissionKey,
        gap: &AdjustmentRevisionGapInput,
    ) -> Result<(), OperationError> {
        let marker = sole_row(
            self.database
                .write_revision_gap(adjustment_revision_gap_plan(pointer, gap)),
        )?;
        // match Node's returned logical identity and categorical disposition
        if marker.logical_key_sha256 != gap.logicalKeySha256 || marker.reason != gap.reason {
            return Err(OperationError::Rejected);
        }
        Ok(())
    }
}

/// Bind one stored receipt to the exact staged bytes and document class.
fn receipt_matches_stage(
    receipt: &AdjustmentRevisionCommitReceipt,
    stage: &AdjustmentRevisionStageReceipt,
) -> bool {
    let kind = match stage.projectionKind {
        AdjustmentRevisionProjectionKind::ActualBestMatch => {
            AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch
        }
        AdjustmentRevisionProjectionKind::NativeSource => {
            AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource
        }
        AdjustmentRevisionProjectionKind::RainGateInput => {
            AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput
        }
        AdjustmentRevisionProjectionKind::TargetRevision => {
            AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision
        }
    };
    let ordinal = receipt.archiveCommitOrdinal.as_bytes();
    receipt.projectionKind == kind
        && receipt.projectionIdentitySha256 == stage.projectionIdentitySha256
        && receipt.projectionSha256 == stage.projectionSha256
        && receipt.stageReceiptSha256 == stage.stageReceiptSha256
        && matches!(ordinal.first(), Some(b'1'..=b'9'))
        && ordinal.iter().all(u8::is_ascii_digit)
        && [
            &receipt.frontierSha256,
            &receipt.predecessorFrontierSha256,
            &receipt.receiptSha256,
        ]
        .into_iter()
        .all(|identity| canonical_receipt_digest(identity))
}

/// Close the server's receipt digests before accepting a grouped database admission.
fn canonical_receipt_digest(identity: &str) -> bool {
    identity.len() == 64
        && identity
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Verify the exact positive PostgreSQL bigint ordinal progression for one group.
fn receipts_continue_frontier(receipts: &[AdjustmentRevisionCommitReceipt]) -> bool {
    let [previous, next] = receipts else {
        return false;
    };
    let previous_ordinal = previous.archiveCommitOrdinal.parse::<i64>();
    let next_ordinal = next.archiveCommitOrdinal.parse::<i64>();
    // advance the SQL bigint without overflow and bind the adjacent frontier
    match (previous_ordinal, next_ordinal) {
        (Ok(previous_ordinal), Ok(next_ordinal)) => {
            previous_ordinal > 0
                && previous_ordinal.checked_add(1) == Some(next_ordinal)
                && next.predecessorFrontierSha256 == previous.frontierSha256
        }
        _ => false,
    }
}

/// Require one decoded row and contain database diagnostics at the facade.
fn sole_row<T, E>(result: Result<Vec<T>, E>) -> Result<T, OperationError> {
    sole_decoded_row(result.map_err(|_| OperationError::Rejected)?)
}

/// Require exactly one already decoded row without discarding cohort members early.
fn sole_decoded_row<T>(rows: Vec<T>) -> Result<T, OperationError> {
    let [row]: [T; 1] = rows.try_into().map_err(|_| OperationError::Rejected)?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use weather_contracts::{
        AdjustmentShadowFamily, AdjustmentShadowRegistrationSlotContractVersionLiteral,
        AdjustmentShadowRegistrationSlotStateEnum, Nullable,
    };
    use weather_database::{
        AdjustmentRevisionGapMarker, AdjustmentShadowRegistrationSlotBind,
        MaintenancePublicationPlan,
    };

    use super::*;
    use crate::maintenance_schedule_slot_reads;

    struct ScheduleDatabase {
        cohort_reads: usize,
        duplicate_wind: bool,
        single_reads: usize,
    }

    impl AdjustmentMaintenanceDatabasePort for ScheduleDatabase {
        type Error = ();

        /// Return the fixed cohort in the supplied logical plan order.
        fn read_schedule_slots(
            &mut self,
            plans: [AdjustmentShadowRegistrationSlotQueryPlan; 3],
        ) -> Result<[Vec<AdjustmentShadowRegistrationSlot>; 3], Self::Error> {
            self.cohort_reads += 1;
            // retain the requested logical order while constructing decoded rows
            Ok(plans.map(|plan| {
                let [AdjustmentShadowRegistrationSlotBind::Family(family)] = plan.binds;
                let mut rows = vec![slot(family)];
                // expose one ordinary cardinality mismatch in the wind position
                if self.duplicate_wind && family == AdjustmentShadowFamily::Wind {
                    rows.push(slot(family));
                }
                rows
            }))
        }

        /// Count any accidental fallback to sequential single-plan reads.
        fn read_schedule_slot(
            &mut self,
            _plan: AdjustmentShadowRegistrationSlotQueryPlan,
        ) -> Result<Vec<AdjustmentShadowRegistrationSlot>, Self::Error> {
            self.single_reads += 1;
            Err(())
        }

        /// Reject unrelated publication reads in this schedule-only fixture.
        fn read_shadow_body_admission(
            &mut self,
            _plan: MaintenancePublicationPlan<'_, 4>,
        ) -> Result<Vec<bool>, Self::Error> {
            unreachable!("schedule fixture must not read shadow admission")
        }

        /// Reject unrelated publication reads in this schedule-only fixture.
        fn read_shadow_revision_receipt(
            &mut self,
            _plan: MaintenancePublicationPlan<'_, 4>,
        ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
            unreachable!("schedule fixture must not read shadow receipts")
        }

        /// Reject unrelated publication reads in this schedule-only fixture.
        fn read_current_revision_receipt(
            &mut self,
            _plan: MaintenancePublicationPlan<'_, 1>,
        ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error> {
            unreachable!("schedule fixture must not read revision receipts")
        }

        /// Reject unrelated publication writes in this schedule-only fixture.
        fn write_revision_gap(
            &mut self,
            _plan: MaintenancePublicationPlan<'_, 3>,
        ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error> {
            unreachable!("schedule fixture must not write revision gaps")
        }
    }

    /// The repository uses one cohort read and preserves temperature, wind, and rain order.
    #[test]
    fn decodes_one_ordered_schedule_cohort() {
        let mut repository = DatabaseAdjustmentMaintenanceRepository::new(ScheduleDatabase {
            cohort_reads: 0,
            duplicate_wind: false,
            single_reads: 0,
        });

        let rows = repository
            .schedule_slots(maintenance_schedule_slot_reads())
            .unwrap();
        let database = repository.into_database();

        assert_eq!(
            rows.map(|row| row.family),
            [
                AdjustmentShadowFamily::Temperature,
                AdjustmentShadowFamily::Wind,
                AdjustmentShadowFamily::Rain,
            ]
        );
        assert_eq!(database.cohort_reads, 1);
        assert_eq!(database.single_reads, 0);
    }

    /// Every cohort member must decode to exactly one typed database row.
    #[test]
    fn rejects_schedule_cohort_cardinality_mismatch() {
        let mut repository = DatabaseAdjustmentMaintenanceRepository::new(ScheduleDatabase {
            cohort_reads: 0,
            duplicate_wind: true,
            single_reads: 0,
        });

        let result = repository.schedule_slots(maintenance_schedule_slot_reads());

        assert!(matches!(result, Err(OperationError::Rejected)));
        let database = repository.into_database();
        assert_eq!(database.cohort_reads, 1);
        assert_eq!(database.single_reads, 0);
    }

    /// Build one family-bound free registration row.
    fn slot(family: AdjustmentShadowFamily) -> AdjustmentShadowRegistrationSlot {
        AdjustmentShadowRegistrationSlot {
            contractVersion: AdjustmentShadowRegistrationSlotContractVersionLiteral,
            epochWitnessSha256: "d".repeat(64),
            family,
            horizonEndAt: "2028-08-26T07:00:00.000Z".to_owned(),
            registrationSha256: Nullable::Null,
            scheduleContractSha256: "e".repeat(64),
            state: AdjustmentShadowRegistrationSlotStateEnum::Free,
            terminalAt: Nullable::Null,
        }
    }
}
