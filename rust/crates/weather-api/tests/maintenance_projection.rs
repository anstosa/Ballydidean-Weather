use weather_api::{maintenance_schedule_slot_reads, project_maintenance_schedule_slots};
use weather_contracts::{
    AdjustmentShadowFamily, AdjustmentShadowRegistrationSlot,
    AdjustmentShadowRegistrationSlotContractVersionLiteral,
    AdjustmentShadowRegistrationSlotStateEnum, Nullable,
};
use weather_database::{
    ADJUSTMENT_SHADOW_REGISTRATION_SLOT_SQL, AdjustmentShadowRegistrationSlotBind,
};

/// build one typed schedule slot
fn slot(
    family: AdjustmentShadowFamily,
    state: AdjustmentShadowRegistrationSlotStateEnum,
) -> AdjustmentShadowRegistrationSlot {
    // derive the paired occupancy fields
    let (registration_sha256, terminal_at) = match state {
        AdjustmentShadowRegistrationSlotStateEnum::Free => (Nullable::Null, Nullable::Null),
        _ => (
            Nullable::Value("a".repeat(64)),
            Nullable::Value("2028-08-26T06:00:00.000Z".to_owned()),
        ),
    };
    AdjustmentShadowRegistrationSlot {
        contractVersion: AdjustmentShadowRegistrationSlotContractVersionLiteral,
        epochWitnessSha256: "d".repeat(64),
        family,
        horizonEndAt: "2028-08-26T07:00:00.000Z".to_owned(),
        registrationSha256: registration_sha256,
        scheduleContractSha256: "e".repeat(64),
        state,
        terminalAt: terminal_at,
    }
}

#[test]
// build exact source query reads in response order
fn builds_fixed_schedule_slot_reads() {
    let reads = maintenance_schedule_slot_reads();

    assert_eq!(
        reads[0].binds,
        [AdjustmentShadowRegistrationSlotBind::Family(
            AdjustmentShadowFamily::Temperature,
        )],
    );
    assert_eq!(
        reads[1].binds,
        [AdjustmentShadowRegistrationSlotBind::Family(
            AdjustmentShadowFamily::Wind,
        )],
    );
    assert_eq!(
        reads[2].binds,
        [AdjustmentShadowRegistrationSlotBind::Family(
            AdjustmentShadowFamily::Rain,
        )],
    );
    assert!(
        reads
            .iter()
            .all(|read| read.sql == ADJUSTMENT_SHADOW_REGISTRATION_SLOT_SQL)
    );
    assert_eq!(
        ADJUSTMENT_SHADOW_REGISTRATION_SLOT_SQL,
        "SELECT adjustment_shadow_registration_slot_v3($1::text) AS value",
    );
}

#[test]
// preserve typed slot fields and fixed family order
fn projects_fixed_schedule_slot_response() {
    let slots = project_maintenance_schedule_slots([
        slot(
            AdjustmentShadowFamily::Temperature,
            AdjustmentShadowRegistrationSlotStateEnum::Free,
        ),
        slot(
            AdjustmentShadowFamily::Wind,
            AdjustmentShadowRegistrationSlotStateEnum::BusyV2Legacy,
        ),
        slot(
            AdjustmentShadowFamily::Rain,
            AdjustmentShadowRegistrationSlotStateEnum::BusyV3,
        ),
    ])
    .expect("fixed family slots must project");

    assert_eq!(slots.len(), 3);
    assert_eq!(slots[0].family, AdjustmentShadowFamily::Temperature);
    assert_eq!(slots[0].registrationSha256, Nullable::Null);
    assert_eq!(
        slots[1].state,
        AdjustmentShadowRegistrationSlotStateEnum::BusyV2Legacy
    );
    assert_eq!(slots[1].registrationSha256, Nullable::Value("a".repeat(64)));
    assert_eq!(slots[2].family, AdjustmentShadowFamily::Rain);
    assert_eq!(
        slots[2].terminalAt,
        Nullable::Value("2028-08-26T06:00:00.000Z".to_owned()),
    );
}

#[test]
// reject a repository result assigned to the wrong read
fn rejects_cross_family_schedule_slot() {
    let error = project_maintenance_schedule_slots([
        slot(
            AdjustmentShadowFamily::Temperature,
            AdjustmentShadowRegistrationSlotStateEnum::Free,
        ),
        slot(
            AdjustmentShadowFamily::Rain,
            AdjustmentShadowRegistrationSlotStateEnum::Free,
        ),
        slot(
            AdjustmentShadowFamily::Wind,
            AdjustmentShadowRegistrationSlotStateEnum::Free,
        ),
    ])
    .expect_err("family mismatch must fail");

    assert_eq!(error.code, "maintenance_schedule.family");
    assert_eq!(error.detail, "schedule slot family mismatch: expected Wind",);
}
