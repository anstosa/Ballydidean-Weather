use weather_contracts::AdjustmentShadowFamily;
use weather_database::{
    ADJUSTMENT_SHADOW_REGISTRATION_SLOT_SQL, AdjustmentShadowRegistrationSlotBind,
    adjustment_shadow_registration_slot_query,
};

#[test]
// preserve the exact source sql and typed family bind
fn builds_exact_registration_slot_query() {
    let plan = adjustment_shadow_registration_slot_query(AdjustmentShadowFamily::Wind);

    assert_eq!(
        plan.sql,
        "SELECT adjustment_shadow_registration_slot_v3($1::text) AS value",
    );
    assert_eq!(plan.sql, ADJUSTMENT_SHADOW_REGISTRATION_SLOT_SQL);
    assert_eq!(
        plan.binds,
        [AdjustmentShadowRegistrationSlotBind::Family(
            AdjustmentShadowFamily::Wind,
        )],
    );
}

#[test]
// retain every closed schedule family without string aliases
fn binds_every_registration_slot_family() {
    let families = [
        AdjustmentShadowFamily::Temperature,
        AdjustmentShadowFamily::Wind,
        AdjustmentShadowFamily::Rain,
    ];

    // verify every generated family variant
    for family in families {
        let plan = adjustment_shadow_registration_slot_query(family);
        assert_eq!(
            plan.binds,
            [AdjustmentShadowRegistrationSlotBind::Family(family)],
        );
    }
}
