//! Preserve the scheduler's controller, registration-head, expiry, and capacity order.

use std::path::Path;

use weather_contracts::{
    AdjustmentShadowFamily, AdjustmentShadowRegistrationSlot,
    AdjustmentShadowRegistrationSlotStateEnum, Nullable,
};

use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant, is_sha256};

use super::forecast_allocation::{
    FORECAST_ALLOCATION_FREE_INODE_FLOOR, FORECAST_ALLOCATION_REQUIRED_FREE_BYTES,
    ForecastAllocationStatfsPort,
};

const SCHEDULE_CONTRACT_SHA256: &str =
    "7c17f5d1a8e8249cd0aa4820638169e51f6edb3433017f50ab4c959e44c62f1f";
const SAFE_JS_INTEGER_MAXIMUM: u128 = 9_007_199_254_740_991;

/// fixed controller configuration supplied once by the existing runtime environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastSchedulerConfiguration {
    pub enabled: bool,
    pub migration_ready: bool,
}

/// existing private API's decoded slot set, read only when controller and migration are ready.
pub trait ForecastSchedulerSlotsPort {
    type Error;

    /// read the exact three source-owned schedule slot projections in API order.
    fn read_schedule_slots(&mut self)
    -> Result<Vec<AdjustmentShadowRegistrationSlot>, Self::Error>;
}

/// three matching database projections for one finite rolling schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastSchedulerHorizon {
    pub epoch_witness_sha256: String,
    pub horizon_end_at: String,
    pub horizon_end_epoch_ms: i64,
}

/// closed scheduler activation result used by the durable state and poll owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastSchedulerAdmission {
    ControllerInactive,
    MigrationUnavailable,
    RegistrationScheduleUnavailable,
    CaptureEnded(ForecastSchedulerHorizon),
    CapacityBlocked {
        free_bytes: u128,
    },
    CapacityUnavailable,
    Active {
        horizon: ForecastSchedulerHorizon,
        free_bytes: u64,
    },
}

impl ForecastSchedulerAdmission {
    /// preserve the exact scheduling activation strings.
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::ControllerInactive => "controller_inactive",
            Self::MigrationUnavailable => "migration_unavailable",
            Self::RegistrationScheduleUnavailable => "registration_schedule_unavailable",
            Self::CaptureEnded(_) => "capture_ended",
            Self::CapacityBlocked { .. } => "capacity_blocked",
            Self::CapacityUnavailable => "capacity_unavailable",
            Self::Active { .. } => "active",
        }
    }
}

/// refresh existing scheduler admission without probing storage for inert or ended controllers.
pub fn evaluate_forecast_scheduler_admission(
    configuration: ForecastSchedulerConfiguration,
    now_epoch_ms: Option<i64>,
    root: &Path,
    slots: &mut impl ForecastSchedulerSlotsPort,
    filesystem: &mut impl ForecastAllocationStatfsPort,
    time: &impl RevisionArchiveCanonicalCodec,
) -> ForecastSchedulerAdmission {
    use ForecastSchedulerAdmission as Admission;
    // preserve the source controller precedence above every expensive read
    if !configuration.enabled {
        return Admission::ControllerInactive;
    }
    if !configuration.migration_ready {
        return Admission::MigrationUnavailable;
    }
    let horizon = slots.read_schedule_slots().ok().and_then(
        // validate one complete database-owned horizon before sampling capacity
        |slots| validate_forecast_scheduler_horizon(&slots, time),
    );
    let Some(horizon) = horizon else {
        return Admission::RegistrationScheduleUnavailable;
    };
    // stop on the authenticated horizon before consulting statfs
    if now_epoch_ms.is_none_or(|now| now >= horizon.horizon_end_epoch_ms) {
        return Admission::CaptureEnded(horizon);
    }
    let Ok(sample) = filesystem.approved_sample(root) else {
        return Admission::CapacityUnavailable;
    };
    let exact_free_bytes = u128::from(sample.available_blocks) * u128::from(sample.block_size);
    // require the current reservation and inode floor without an unsafe JS numeric admission
    if exact_free_bytes > SAFE_JS_INTEGER_MAXIMUM
        || exact_free_bytes < u128::from(FORECAST_ALLOCATION_REQUIRED_FREE_BYTES)
        || sample.free_inodes < FORECAST_ALLOCATION_FREE_INODE_FLOOR
    {
        return Admission::CapacityBlocked {
            free_bytes: exact_free_bytes,
        };
    }
    Admission::Active {
        horizon,
        free_bytes: exact_free_bytes as u64,
    }
}

/// bind the exact family order, occupancy, and shared schedule head.
pub fn validate_forecast_scheduler_horizon(
    slots: &[AdjustmentShadowRegistrationSlot],
    time: &impl RevisionArchiveCanonicalCodec,
) -> Option<ForecastSchedulerHorizon> {
    let [first, wind, rain] = slots else {
        return None;
    };
    let horizon_end_epoch_ms = instant(time, &first.horizonEndAt)?;
    let ordered = [
        (AdjustmentShadowFamily::Temperature, first),
        (AdjustmentShadowFamily::Wind, wind),
        (AdjustmentShadowFamily::Rain, rain),
    ];
    // retain one shared authenticated global horizon across all three families
    for (family, slot) in ordered {
        if slot.family != family
            || slot.scheduleContractSha256 != SCHEDULE_CONTRACT_SHA256
            || slot.epochWitnessSha256 != first.epochWitnessSha256
            || slot.horizonEndAt != first.horizonEndAt
            || !is_sha256(&slot.epochWitnessSha256)
        {
            return None;
        }
        let occupied = slot.state != AdjustmentShadowRegistrationSlotStateEnum::Free;
        match (&slot.registrationSha256, &slot.terminalAt) {
            (Nullable::Null, Nullable::Null) if !occupied => {}
            (Nullable::Value(registration), Nullable::Value(terminal)) if occupied => {
                // preserve occupancy evidence without substituting for the global horizon
                if !is_sha256(registration) || instant(time, terminal).is_none() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(ForecastSchedulerHorizon {
        epoch_witness_sha256: first.epochWitnessSha256.clone(),
        horizon_end_at: first.horizonEndAt.clone(),
        horizon_end_epoch_ms,
    })
}

use crate::{
    AdmittedPublicApiOrigin, PublicApiBodyReadOutcome, PublicApiHttpMethod, PublicApiHttpPort,
    PublicApiHttpRequest, PublicApiHttpResponse, PublicApiRedirectPolicy,
};

const MAXIMUM_SCHEDULE_BODY: usize = 8 * 1024;

/// approved decoder of the exact generated schedule-slot array.
pub trait ForecastSchedulerSlotDecoder {
    /// decode one bounded private schedule response with its versioned slot shapes.
    fn decode_slots(&self, body: &[u8]) -> Option<Vec<AdjustmentShadowRegistrationSlot>>;
}

/// ordinary errors retained below the scheduler's inactive status.
#[derive(Debug, PartialEq, Eq)]
pub enum ForecastSchedulerSlotsError<Error> {
    OriginUnavailable,
    Transport(Error),
    UnavailableStatus,
    SizeLimit,
    InvalidSlots,
}

/// bounded private scheduler reader using the existing admitted HTTP substrate.
pub struct BoundedForecastSchedulerSlotReader<Http, Decoder> {
    private_origin: Option<AdmittedPublicApiOrigin>,
    http: Http,
    decoder: Decoder,
}

impl<Http, Decoder> BoundedForecastSchedulerSlotReader<Http, Decoder> {
    /// bind an already-approved compose-private origin, or retain the inactive configuration.
    pub const fn new(
        private_origin: Option<AdmittedPublicApiOrigin>,
        http: Http,
        decoder: Decoder,
    ) -> Self {
        Self {
            private_origin,
            http,
            decoder,
        }
    }
}

impl<Http: PublicApiHttpPort, Decoder: ForecastSchedulerSlotDecoder> ForecastSchedulerSlotsPort
    for BoundedForecastSchedulerSlotReader<Http, Decoder>
{
    type Error = ForecastSchedulerSlotsError<Http::Error>;

    /// read only the deployed private route and retain at most its original eight KiB.
    fn read_schedule_slots(
        &mut self,
    ) -> Result<Vec<AdjustmentShadowRegistrationSlot>, Self::Error> {
        use ForecastSchedulerSlotsError as Error;
        let origin = self
            .private_origin
            .clone()
            .ok_or(Error::OriginUnavailable)?;
        let mut response = self
            .http
            .execute(&PublicApiHttpRequest {
                accept: "application/json".to_owned(),
                method: PublicApiHttpMethod::Get,
                origin,
                pathname: "/internal/adjustment-maintenance/schedule-slots".to_owned(),
                redirect: PublicApiRedirectPolicy::Manual,
                search: String::new(),
                timeout_ms: 5_000,
            })
            .map_err(Error::Transport)?;
        // release a failed response through the owning transport without consuming its body
        if !(200..300).contains(&response.head().status_code) {
            return Err(Error::UnavailableStatus);
        }
        let mut body = Vec::new();
        let mut refused = false;
        let outcome = response
            .read_body(&mut |chunk| {
                // cancel before retaining the first chunk beyond the exact bounded contract
                if chunk.len() > MAXIMUM_SCHEDULE_BODY - body.len() {
                    refused = true;
                    return false;
                }
                body.extend_from_slice(chunk);
                true
            })
            .map_err(Error::Transport)?;
        // keep failed or incomplete reads out of the decoded schedule owner
        if refused || outcome != PublicApiBodyReadOutcome::Complete {
            return Err(Error::SizeLimit);
        }
        self.decoder.decode_slots(&body).ok_or(Error::InvalidSlots)
    }
}
