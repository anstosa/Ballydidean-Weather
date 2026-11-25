//! Exact database facades for serving and publishing admitted maintenance evidence.

use weather_contracts::{
    AdjustmentEcmwfTemperatureRevisionAdmissionKey, AdjustmentForecastAnchorRevisionAdmissionKey,
    AdjustmentRainGateRevisionAdmissionKey, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionGapInput, AdjustmentRevisionGapReason, AdjustmentRevisionProjectionKind,
    AdjustmentShadowRegistrationSlot, AdjustmentWeatherRevisionAdmissionKey,
    MaintenanceShadowPredictionMetadata, Nullable,
};

use crate::AdjustmentShadowRegistrationSlotQueryPlan;

const SHADOW_BODY_ADMISSION_SQL: &str = "SELECT adjustment_shadow_body_admission_v2(
      $1::text, $2::text, $3::text, $4::integer
    ) AS admitted";
const SHADOW_REVISION_ADMISSION_SQL: &str = "SELECT adjustment_shadow_revision_admission_v1(
      $1::text, $2::text, $3::text, $4::integer
    ) AS value";
const WEATHER_REVISION_ADMISSION_SQL: &str =
    "SELECT adjustment_weather_revision_admission_v1($1::jsonb) AS value";
const FORECAST_ANCHOR_ADMISSION_SQL: &str =
    "SELECT adjustment_forecast_anchor_revision_admission_v1($1::jsonb) AS value";
const RAIN_GATE_ADMISSION_SQL: &str =
    "SELECT adjustment_rain_gate_revision_admission_v1($1::jsonb) AS value";
const ECMWF_TEMPERATURE_ADMISSION_SQL: &str =
    "SELECT adjustment_ecmwf_temperature_revision_admission_v1($1::jsonb) AS value";
const MARK_REVISION_GAP_SQL: &str = "SELECT weather_mark_adjustment_revision_gap_v1(
      $1::text, $2::jsonb, $3::jsonb
    ) AS value";

/// The global wind ceiling reused by both source shadow admission facades.
const MAX_SHADOW_BODY_BYTES: u64 = 65_536;

/// The closed logical key selected by an admitted revision transport document.
#[derive(Debug, Clone, PartialEq)]
pub enum AdjustmentRevisionAdmissionKey {
    WeatherRecord(AdjustmentWeatherRevisionAdmissionKey),
    ForecastAnchor(AdjustmentForecastAnchorRevisionAdmissionKey),
    RainGate(AdjustmentRainGateRevisionAdmissionKey),
    EcmwfTemperature(AdjustmentEcmwfTemperatureRevisionAdmissionKey),
}

impl AdjustmentRevisionAdmissionKey {
    /// Select the exact closed relation name for the gap marker.
    const fn relation_name(&self) -> &'static str {
        match self {
            Self::WeatherRecord(_) => "weather_record",
            Self::ForecastAnchor(_) => "forecast_anchor",
            Self::RainGate(_) => "rain_gate",
            Self::EcmwfTemperature(_) => "ecmwf_temperature",
        }
    }

    /// Select the fixed SQL facade for the admitted key.
    const fn admission_sql(&self) -> &'static str {
        match self {
            Self::WeatherRecord(_) => WEATHER_REVISION_ADMISSION_SQL,
            Self::ForecastAnchor(_) => FORECAST_ANCHOR_ADMISSION_SQL,
            Self::RainGate(_) => RAIN_GATE_ADMISSION_SQL,
            Self::EcmwfTemperature(_) => ECMWF_TEMPERATURE_ADMISSION_SQL,
        }
    }
}

/// The ordered scalar and canonical JSON bind values for the closed facade set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaintenancePublicationBind<'a> {
    Text(&'a str),
    Integer(i32),
    /// Encode the inner key DTO without a Rust variant tag.
    RevisionKeyJson(&'a AdjustmentRevisionAdmissionKey),
    GapJson(&'a AdjustmentRevisionGapInput),
}

/// One fixed query together with its exact number and order of bind values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaintenancePublicationPlan<'a, const N: usize> {
    pub sql: &'static str,
    pub binds: [MaintenancePublicationBind<'a>; N],
}

/// The returned value-free marker after exact wire contract decoding.
#[derive(Debug, Clone, PartialEq)]
pub struct AdjustmentRevisionGapMarker {
    pub logical_key_sha256: String,
    pub projection_identity_sha256: Nullable<String>,
    pub projection_kind: AdjustmentRevisionProjectionKind,
    pub projection_sha256: Nullable<String>,
    pub reason: AdjustmentRevisionGapReason,
    pub stored_content_sha256: String,
}

/// Borrow the four exact shadow binds from already admitted body metadata.
pub fn adjustment_shadow_publication_plans(
    metadata: &MaintenanceShadowPredictionMetadata,
) -> Option<[MaintenancePublicationPlan<'_, 4>; 2]> {
    // preserve the source facade input bound before issuing either read
    if !(1..=MAX_SHADOW_BODY_BYTES).contains(&metadata.bodyByteCount) {
        return None;
    }
    let binds = [
        MaintenancePublicationBind::Text(&metadata.registrationSha256),
        MaintenancePublicationBind::Text(&metadata.dueKey),
        MaintenancePublicationBind::Text(&metadata.predictionBodySha256),
        MaintenancePublicationBind::Integer(i32::try_from(metadata.bodyByteCount).ok()?),
    ];
    Some([
        MaintenancePublicationPlan {
            sql: SHADOW_BODY_ADMISSION_SQL,
            binds,
        },
        MaintenancePublicationPlan {
            sql: SHADOW_REVISION_ADMISSION_SQL,
            binds,
        },
    ])
}

/// Borrow one exact current-pointer lookup plan.
pub const fn adjustment_revision_publication_plan(
    key: &AdjustmentRevisionAdmissionKey,
) -> MaintenancePublicationPlan<'_, 1> {
    MaintenancePublicationPlan {
        sql: key.admission_sql(),
        binds: [MaintenancePublicationBind::RevisionKeyJson(key)],
    }
}

/// Borrow the ordered relation, key, and gap binds for a permanent disposition.
pub const fn adjustment_revision_gap_plan<'a>(
    key: &'a AdjustmentRevisionAdmissionKey,
    gap: &'a AdjustmentRevisionGapInput,
) -> MaintenancePublicationPlan<'a, 3> {
    MaintenancePublicationPlan {
        sql: MARK_REVISION_GAP_SQL,
        binds: [
            MaintenancePublicationBind::Text(key.relation_name()),
            MaintenancePublicationBind::RevisionKeyJson(key),
            MaintenancePublicationBind::GapJson(gap),
        ],
    }
}

/// Execution and strict row decoding using the current API-pool transport.
pub trait AdjustmentMaintenanceDatabasePort {
    type Error;

    /// Decode the fixed temperature, wind, and rain registration cohort in logical order.
    fn read_schedule_slots(
        &mut self,
        plans: [AdjustmentShadowRegistrationSlotQueryPlan; 3],
    ) -> Result<[Vec<AdjustmentShadowRegistrationSlot>; 3], Self::Error> {
        // start every legacy single-plan read before propagating the first ordered failure
        let [temperature, wind, rain] = plans.map(|plan| self.read_schedule_slot(plan));
        Ok([temperature?, wind?, rain?])
    }

    /// Decode every row of the one-family registration lookup.
    fn read_schedule_slot(
        &mut self,
        plan: AdjustmentShadowRegistrationSlotQueryPlan,
    ) -> Result<Vec<AdjustmentShadowRegistrationSlot>, Self::Error>;

    /// Read the exact single scalar body-admission result.
    fn read_shadow_body_admission(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<bool>, Self::Error>;

    /// Decode the exact named shadow revision receipt contract.
    fn read_shadow_revision_receipt(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 4>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error>;

    /// Decode the exact named current-pointer receipt contract.
    fn read_current_revision_receipt(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 1>,
    ) -> Result<Vec<AdjustmentRevisionCommitReceipt>, Self::Error>;

    /// Record a disposition and decode the exact named database marker contract.
    fn write_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error>;
}

#[cfg(test)]
mod tests {
    use weather_contracts::AdjustmentShadowFamily;

    use super::*;
    use crate::{AdjustmentShadowRegistrationSlotBind, adjustment_shadow_registration_slot_query};

    #[derive(Default)]
    struct LegacyPort {
        reads: Vec<AdjustmentShadowFamily>,
    }

    impl AdjustmentMaintenanceDatabasePort for LegacyPort {
        type Error = &'static str;

        /// Retain every legacy single-plan read and fail the first logical family.
        fn read_schedule_slot(
            &mut self,
            plan: AdjustmentShadowRegistrationSlotQueryPlan,
        ) -> Result<Vec<AdjustmentShadowRegistrationSlot>, Self::Error> {
            let [AdjustmentShadowRegistrationSlotBind::Family(family)] = plan.binds;
            self.reads.push(family);
            // preserve one ordinary first-query failure while later reads still execute
            if family == AdjustmentShadowFamily::Temperature {
                return Err("temperature unavailable");
            }
            Ok(Vec::new())
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

    /// The compatibility fanout observes all three logical reads before returning one error.
    #[test]
    fn default_schedule_fanout_starts_every_logical_read() {
        let plans = [
            adjustment_shadow_registration_slot_query(AdjustmentShadowFamily::Temperature),
            adjustment_shadow_registration_slot_query(AdjustmentShadowFamily::Wind),
            adjustment_shadow_registration_slot_query(AdjustmentShadowFamily::Rain),
        ];
        let mut port = LegacyPort::default();

        let result = port.read_schedule_slots(plans);

        assert_eq!(result, Err("temperature unavailable"));
        assert_eq!(
            port.reads,
            [
                AdjustmentShadowFamily::Temperature,
                AdjustmentShadowFamily::Wind,
                AdjustmentShadowFamily::Rain,
            ]
        );
    }
}
