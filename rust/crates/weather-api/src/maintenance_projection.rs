use weather_contracts::{AdjustmentShadowFamily, AdjustmentShadowRegistrationSlot};
use weather_database::{
    AdjustmentShadowRegistrationSlotQueryPlan, adjustment_shadow_registration_slot_query,
};

/// bounded schedule slot projection failure
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceScheduleSlotError {
    pub code: &'static str,
    pub detail: String,
}

/// build the three fixed schedule reads
pub fn maintenance_schedule_slot_reads() -> [AdjustmentShadowRegistrationSlotQueryPlan; 3] {
    [
        adjustment_shadow_registration_slot_query(AdjustmentShadowFamily::Temperature),
        adjustment_shadow_registration_slot_query(AdjustmentShadowFamily::Wind),
        adjustment_shadow_registration_slot_query(AdjustmentShadowFamily::Rain),
    ]
}

/// project the fixed private schedule response
pub fn project_maintenance_schedule_slots(
    slots: [AdjustmentShadowRegistrationSlot; 3],
) -> Result<Vec<AdjustmentShadowRegistrationSlot>, MaintenanceScheduleSlotError> {
    let expected = [
        AdjustmentShadowFamily::Temperature,
        AdjustmentShadowFamily::Wind,
        AdjustmentShadowFamily::Rain,
    ];

    // bind every result to its requested family
    for (slot, family) in slots.iter().zip(expected) {
        // reject cross-family repository results
        if slot.family != family {
            return Err(MaintenanceScheduleSlotError {
                code: "maintenance_schedule.family",
                detail: format!("schedule slot family mismatch: expected {family:?}"),
            });
        }
    }

    Ok(Vec::from(slots))
}
