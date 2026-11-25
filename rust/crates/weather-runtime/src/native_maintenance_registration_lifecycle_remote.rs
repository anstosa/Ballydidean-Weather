//! Canonical remote wire owner for registration lifecycle status and schedule initialization.

use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::pin::Pin;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::MaintenanceCanonicalJson;
use crate::capture_epoch_witness::resolve_operator_capture_clock;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_capture_readiness::NativeMaintenanceCaptureFamily;
use crate::native_maintenance_rolling_schedule::{
    ADJUSTMENT_ROLLING_SCHEDULE_SHA256, NativeAdjustmentRollingScheduleBootstrap,
    NativeAdjustmentRollingScheduleBootstrapInput, build_adjustment_rolling_schedule_bootstrap,
};

const LIFECYCLE_VERSION: &str = "adjustment-registration-lifecycle-status/v4";
const SCHEDULE_VERSION: &str = "adjustment-registration-schedule-status/v3";
const SLOT_VERSION: &str = "adjustment-shadow-registration-slot/v3";
const EMPTY_METADATA_ROOT_SHA256: &str =
    "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945";

const LIFECYCLE_FIELDS: &[&str] = &[
    "contractVersion",
    "entries",
    "epochAt",
    "epochWitnessSha256",
    "horizonEndAt",
    "scheduleContractSha256",
    "snapshotAt",
];
const ENTRY_FIELDS: &[&str] = &[
    "activeRegistration",
    "latestRegistrationSha256",
    "metadata",
    "predecessor",
    "slot",
];
const SLOT_FIELDS: &[&str] = &[
    "contractVersion",
    "epochWitnessSha256",
    "family",
    "horizonEndAt",
    "registrationSha256",
    "scheduleContractSha256",
    "state",
    "terminalAt",
];
const REGISTRATION_FIELDS: &[&str] = &[
    "artifactSha256",
    "candidateSha256",
    "cohortSha256",
    "epochWitnessSha256",
    "family",
    "intervalEndAt",
    "intervalStartAt",
    "policySha256",
    "predecessorRegistrationSha256",
    "registrationSha256",
    "reservedKeySha256",
    "scheduleContractSha256",
    "siteKey",
    "sourceSha256",
    "targetCutoffAt",
    "terminalAt",
];
const PREDECESSOR_FIELDS: &[&str] = &[
    "epochWitnessSha256",
    "reconciliationSha256",
    "registrationSha256",
    "reservedKeySha256",
    "scheduleContractSha256",
    "sourceSha256",
    "terminalAt",
];
const METADATA_FIELDS: &[&str] = &[
    "finalizedPredictionCount",
    "generation",
    "lastAnchorSha256",
    "rootSha256",
    "throughAt",
];
const INITIALIZE_ACK_FIELDS: &[&str] =
    &["bootstrapSha256", "initialized", "scheduleContractSha256"];

/// Closed registration slot occupancy returned by the root projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRegistrationSlotState {
    BusyV2Legacy,
    BusyV3,
    Free,
}

impl NativeRegistrationSlotState {
    /// Return the exact database state spelling.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::BusyV2Legacy => "busy_v2_legacy",
            Self::BusyV3 => "busy_v3",
            Self::Free => "free",
        }
    }
}

/// Exact value-blind database-owned registration slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRegistrationSlot {
    pub contract_version: String,
    pub epoch_witness_sha256: String,
    pub family: NativeMaintenanceCaptureFamily,
    pub horizon_end_at: String,
    pub registration_sha256: Option<String>,
    pub schedule_contract_sha256: String,
    pub state: NativeRegistrationSlotState,
    pub terminal_at: Option<String>,
}

/// Full active v3 registration retained for lifecycle crossbinding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRegistrationActiveRegistration {
    pub artifact_sha256: String,
    pub candidate_sha256: String,
    pub cohort_sha256: String,
    pub epoch_witness_sha256: String,
    pub family: NativeMaintenanceCaptureFamily,
    pub interval_end_at: String,
    pub interval_start_at: String,
    pub policy_sha256: String,
    pub predecessor_registration_sha256: Option<String>,
    pub registration_sha256: String,
    pub reserved_key_sha256: String,
    pub schedule_contract_sha256: String,
    pub site_key: String,
    pub source_sha256: String,
    pub target_cutoff_at: String,
    pub terminal_at: String,
}

/// Reconciled prior terminal identity under the same lifecycle epoch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRegistrationLifecyclePredecessor {
    pub epoch_witness_sha256: String,
    pub reconciliation_sha256: String,
    pub registration_sha256: String,
    pub reserved_key_sha256: String,
    pub schedule_contract_sha256: String,
    pub source_sha256: String,
    pub terminal_at: String,
}

/// Compact retained metadata root without prediction payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRegistrationLifecycleMetadata {
    pub finalized_prediction_count: u32,
    pub generation: u32,
    pub last_anchor_sha256: Option<String>,
    pub root_sha256: String,
    pub through_at: Option<String>,
}

/// One exact active, predecessor, metadata, and schedule-slot projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRegistrationLifecycleEntry {
    pub active_registration: Option<NativeRegistrationActiveRegistration>,
    pub latest_registration_sha256: Option<String>,
    pub metadata: Option<NativeRegistrationLifecycleMetadata>,
    pub predecessor: Option<NativeRegistrationLifecyclePredecessor>,
    pub slot: NativeRegistrationSlot,
}

/// Full verified root-owned registration lifecycle status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRegistrationLifecycleStatus {
    pub contract_version: String,
    pub entries: [NativeRegistrationLifecycleEntry; 3],
    pub epoch_at: String,
    pub epoch_witness_sha256: String,
    pub horizon_end_at: String,
    pub schedule_contract_sha256: String,
    pub snapshot_at: String,
}

/// Exact planner schedule projected from the lifecycle transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRegistrationScheduleStatus {
    pub contract_version: &'static str,
    pub epoch_at: String,
    pub epoch_witness_sha256: String,
    pub horizon_end_at: String,
    pub schedule_contract_sha256: String,
    pub slots: [NativeRegistrationSlot; 3],
    pub snapshot_at: String,
}

/// Canonical idempotent schedule-initialization acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRegistrationScheduleInitializeAck {
    pub bootstrap_sha256: String,
    pub initialized: bool,
    pub schedule_contract_sha256: String,
}

/// Closed forced-command operation set for later SSH transport application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeRegistrationLifecycleRemoteOperation {
    LifecycleStatusV4,
    ScheduleInitializeV3 {
        bootstrap_sha256: String,
        canonical_bootstrap: Vec<u8>,
    },
}

impl NativeRegistrationLifecycleRemoteOperation {
    /// Return the single fixed remote command for this operation.
    pub const fn command(&self) -> &'static str {
        match self {
            Self::LifecycleStatusV4 => "adjustment-registration-lifecycle-status-v4",
            Self::ScheduleInitializeV3 { .. } => "adjustment-registration-schedule-initialize-v3",
        }
    }

    /// Return the exact ordered command arguments after the fixed operation.
    pub fn arguments(&self) -> Vec<&str> {
        match self {
            Self::LifecycleStatusV4 => Vec::new(),
            Self::ScheduleInitializeV3 {
                bootstrap_sha256, ..
            } => vec![bootstrap_sha256],
        }
    }

    /// Borrow the only canonical stdin body admitted by this closed operation set.
    pub fn body(&self) -> Option<&[u8]> {
        match self {
            Self::LifecycleStatusV4 => None,
            Self::ScheduleInitializeV3 {
                canonical_bootstrap,
                ..
            } => Some(canonical_bootstrap),
        }
    }
}

/// Bounded fixed lifecycle operation response.
pub type NativeRegistrationLifecycleResponse<'a, Error> =
    Pin<Box<dyn Future<Output = Result<Vec<u8>, Error>> + 'a>>;

/// Effect-only application boundary for the two closed registration operations.
pub trait NativeRegistrationLifecycleRemoteOperationPort {
    type Error;

    /// Execute one fixed operation and return its complete bounded response bytes.
    fn execute(
        &mut self,
        operation: NativeRegistrationLifecycleRemoteOperation,
    ) -> NativeRegistrationLifecycleResponse<'_, Self::Error>;
}

/// Canonical request or response refusal without reflected material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRegistrationLifecycleWireError {
    Canonical,
    Shape,
    Clock,
    Identity,
    Binding,
}

impl Display for NativeRegistrationLifecycleWireError {
    /// Render one bounded wire category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Canonical => "registration lifecycle document is not canonical",
            Self::Shape => "registration lifecycle document shape is invalid",
            Self::Clock => "registration lifecycle clock is invalid",
            Self::Identity => "registration lifecycle identity is invalid",
            Self::Binding => "registration lifecycle binding differs",
        })
    }
}

impl std::error::Error for NativeRegistrationLifecycleWireError {}

/// Remote effect or canonical wire failure.
#[derive(Debug)]
pub enum NativeRegistrationLifecycleRemoteError<Port> {
    Port(Port),
    Request(NativeRegistrationLifecycleWireError),
    Response(NativeRegistrationLifecycleWireError),
}

impl<Port: Display> Display for NativeRegistrationLifecycleRemoteError<Port> {
    /// Preserve the port diagnostic or bounded wire phase.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Port(error) => Display::fmt(error, formatter),
            Self::Request(error) => write!(formatter, "registration lifecycle request: {error}"),
            Self::Response(error) => {
                write!(formatter, "registration lifecycle response: {error}")
            }
        }
    }
}

/// Canonical remote owner independent from the later concrete SSH application.
pub struct NativeMaintenanceRegistrationLifecycleRemote<Port, Codec, Time> {
    port: Port,
    codec: Codec,
    time: Time,
}

impl<Port, Codec, Time> NativeMaintenanceRegistrationLifecycleRemote<Port, Codec, Time>
where
    Port: NativeRegistrationLifecycleRemoteOperationPort,
    Codec: MaintenanceCanonicalJson,
    Time: CurrentTimeResolver,
{
    /// Bind one explicit operation port to canonical JSON and time owners.
    pub const fn new(port: Port, codec: Codec, time: Time) -> Self {
        Self { port, codec, time }
    }

    /// Initialize the exact rolling schedule and admit its idempotent acknowledgement.
    pub async fn initialize_schedule(
        &mut self,
        bootstrap: &NativeAdjustmentRollingScheduleBootstrap,
    ) -> Result<
        NativeRegistrationScheduleInitializeAck,
        NativeRegistrationLifecycleRemoteError<Port::Error>,
    > {
        let expected = build_adjustment_rolling_schedule_bootstrap(
            NativeAdjustmentRollingScheduleBootstrapInput {
                epoch_at: &bootstrap.epoch_at,
                epoch_witness_sha256: &bootstrap.epoch_witness_sha256,
            },
        )
        .map_err(|_| {
            NativeRegistrationLifecycleRemoteError::Request(
                NativeRegistrationLifecycleWireError::Binding,
            )
        })?;
        // reject caller-selected horizon, date, hash, or contract substitutions before transport
        if &expected != bootstrap {
            return Err(NativeRegistrationLifecycleRemoteError::Request(
                NativeRegistrationLifecycleWireError::Binding,
            ));
        }
        let document = bootstrap_value(bootstrap);
        let canonical_bootstrap = self.codec.encode_canonical_json(&document).ok_or(
            NativeRegistrationLifecycleRemoteError::Request(
                NativeRegistrationLifecycleWireError::Canonical,
            ),
        )?;
        let response = self
            .port
            .execute(
                NativeRegistrationLifecycleRemoteOperation::ScheduleInitializeV3 {
                    bootstrap_sha256: bootstrap.bootstrap_sha256.clone(),
                    canonical_bootstrap,
                },
            )
            .await
            .map_err(NativeRegistrationLifecycleRemoteError::Port)?;
        decode_registration_schedule_initialize_ack(
            &mut self.codec,
            &response,
            &bootstrap.bootstrap_sha256,
        )
        .map_err(NativeRegistrationLifecycleRemoteError::Response)
    }

    /// Fetch and fully verify the exact v4 lifecycle transaction.
    pub async fn read_lifecycle(
        &mut self,
    ) -> Result<
        NativeRegistrationLifecycleStatus,
        NativeRegistrationLifecycleRemoteError<Port::Error>,
    > {
        let response = self
            .port
            .execute(NativeRegistrationLifecycleRemoteOperation::LifecycleStatusV4)
            .await
            .map_err(NativeRegistrationLifecycleRemoteError::Port)?;
        decode_registration_lifecycle_status(&mut self.codec, &response, &self.time)
            .map_err(NativeRegistrationLifecycleRemoteError::Response)
    }

    /// Release the operation port after fixture or process composition.
    pub fn into_port(self) -> Port {
        self.port
    }
}

/// Decode one canonical v4 lifecycle status and all crossbindings.
pub fn decode_registration_lifecycle_status(
    codec: &mut impl MaintenanceCanonicalJson,
    bytes: &[u8],
    time: &dyn CurrentTimeResolver,
) -> Result<NativeRegistrationLifecycleStatus, NativeRegistrationLifecycleWireError> {
    let value = codec
        .parse_canonical_object(bytes)
        .ok_or(NativeRegistrationLifecycleWireError::Canonical)?;
    let status = decode_lifecycle_value(&value)?;
    validate_lifecycle_status(&status, codec, time)?;
    Ok(status)
}

/// Project one verified lifecycle into the existing schedule planner contract.
pub fn registration_schedule_from_lifecycle(
    lifecycle: &NativeRegistrationLifecycleStatus,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeRegistrationScheduleStatus, NativeRegistrationLifecycleWireError> {
    validate_lifecycle_status(lifecycle, codec, time)?;
    let [temperature, wind, rain] = lifecycle.entries.clone();
    Ok(NativeRegistrationScheduleStatus {
        contract_version: SCHEDULE_VERSION,
        epoch_at: lifecycle.epoch_at.clone(),
        epoch_witness_sha256: lifecycle.epoch_witness_sha256.clone(),
        horizon_end_at: lifecycle.horizon_end_at.clone(),
        schedule_contract_sha256: lifecycle.schedule_contract_sha256.clone(),
        slots: [temperature.slot, wind.slot, rain.slot],
        snapshot_at: lifecycle.snapshot_at.clone(),
    })
}

/// Decode one exact canonical initialization acknowledgement.
pub fn decode_registration_schedule_initialize_ack(
    codec: &mut impl MaintenanceCanonicalJson,
    bytes: &[u8],
    expected_bootstrap_sha256: &str,
) -> Result<NativeRegistrationScheduleInitializeAck, NativeRegistrationLifecycleWireError> {
    let value = codec
        .parse_canonical_object(bytes)
        .ok_or(NativeRegistrationLifecycleWireError::Canonical)?;
    // reject response extensions before reading any acknowledgement field
    if !exact_keys(&value, INITIALIZE_ACK_FIELDS) {
        return Err(NativeRegistrationLifecycleWireError::Shape);
    }
    let bootstrap_sha256 =
        text(&value, "bootstrapSha256").ok_or(NativeRegistrationLifecycleWireError::Shape)?;
    let schedule_contract_sha256 = text(&value, "scheduleContractSha256")
        .ok_or(NativeRegistrationLifecycleWireError::Shape)?;
    let Some(JsonValue::Boolean(initialized)) = field(&value, "initialized") else {
        return Err(NativeRegistrationLifecycleWireError::Shape);
    };
    // bind retries and first initialization to the exact submitted bootstrap
    // bind the acknowledgement to the submitted bootstrap and rolling contract
    if !valid_hash(expected_bootstrap_sha256)
        || bootstrap_sha256 != expected_bootstrap_sha256
        || schedule_contract_sha256 != ADJUSTMENT_ROLLING_SCHEDULE_SHA256
    {
        return Err(NativeRegistrationLifecycleWireError::Binding);
    }
    Ok(NativeRegistrationScheduleInitializeAck {
        bootstrap_sha256,
        initialized: *initialized,
        schedule_contract_sha256,
    })
}

/// Decode the full typed lifecycle document before semantic crossbinding.
fn decode_lifecycle_value(
    value: &JsonValue,
) -> Result<NativeRegistrationLifecycleStatus, NativeRegistrationLifecycleWireError> {
    // reject lifecycle extensions before constructing the typed status
    if !exact_keys(value, LIFECYCLE_FIELDS) {
        return Err(NativeRegistrationLifecycleWireError::Shape);
    }
    let entries = array(value, "entries")?
        .iter()
        .map(decode_entry)
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| NativeRegistrationLifecycleWireError::Shape)?;
    Ok(NativeRegistrationLifecycleStatus {
        contract_version: required_text(value, "contractVersion")?,
        entries,
        epoch_at: required_text(value, "epochAt")?,
        epoch_witness_sha256: required_text(value, "epochWitnessSha256")?,
        horizon_end_at: required_text(value, "horizonEndAt")?,
        schedule_contract_sha256: required_text(value, "scheduleContractSha256")?,
        snapshot_at: required_text(value, "snapshotAt")?,
    })
}

/// Decode one lifecycle entry without accepting extension members.
fn decode_entry(
    value: &JsonValue,
) -> Result<NativeRegistrationLifecycleEntry, NativeRegistrationLifecycleWireError> {
    // reject entry extensions before decoding nullable authorities
    if !exact_keys(value, ENTRY_FIELDS) {
        return Err(NativeRegistrationLifecycleWireError::Shape);
    }
    Ok(NativeRegistrationLifecycleEntry {
        active_registration: optional_object(value, "activeRegistration", decode_registration)?,
        latest_registration_sha256: nullable_text(value, "latestRegistrationSha256")?,
        metadata: optional_object(value, "metadata", decode_metadata)?,
        predecessor: optional_object(value, "predecessor", decode_predecessor)?,
        slot: decode_slot(
            field(value, "slot").ok_or(NativeRegistrationLifecycleWireError::Shape)?,
        )?,
    })
}

/// Decode one closed database slot.
fn decode_slot(
    value: &JsonValue,
) -> Result<NativeRegistrationSlot, NativeRegistrationLifecycleWireError> {
    // reject slot extensions before decoding occupancy
    if !exact_keys(value, SLOT_FIELDS) {
        return Err(NativeRegistrationLifecycleWireError::Shape);
    }
    Ok(NativeRegistrationSlot {
        contract_version: required_text(value, "contractVersion")?,
        epoch_witness_sha256: required_text(value, "epochWitnessSha256")?,
        family: decode_family(&required_text(value, "family")?)?,
        horizon_end_at: required_text(value, "horizonEndAt")?,
        registration_sha256: nullable_text(value, "registrationSha256")?,
        schedule_contract_sha256: required_text(value, "scheduleContractSha256")?,
        state: decode_slot_state(&required_text(value, "state")?)?,
        terminal_at: nullable_text(value, "terminalAt")?,
    })
}

/// Decode one complete active v3 registration.
fn decode_registration(
    value: &JsonValue,
) -> Result<NativeRegistrationActiveRegistration, NativeRegistrationLifecycleWireError> {
    // reject registration extensions before deriving its identity
    if !exact_keys(value, REGISTRATION_FIELDS) {
        return Err(NativeRegistrationLifecycleWireError::Shape);
    }
    Ok(NativeRegistrationActiveRegistration {
        artifact_sha256: required_text(value, "artifactSha256")?,
        candidate_sha256: required_text(value, "candidateSha256")?,
        cohort_sha256: required_text(value, "cohortSha256")?,
        epoch_witness_sha256: required_text(value, "epochWitnessSha256")?,
        family: decode_family(&required_text(value, "family")?)?,
        interval_end_at: required_text(value, "intervalEndAt")?,
        interval_start_at: required_text(value, "intervalStartAt")?,
        policy_sha256: required_text(value, "policySha256")?,
        predecessor_registration_sha256: nullable_text(value, "predecessorRegistrationSha256")?,
        registration_sha256: required_text(value, "registrationSha256")?,
        reserved_key_sha256: required_text(value, "reservedKeySha256")?,
        schedule_contract_sha256: required_text(value, "scheduleContractSha256")?,
        site_key: required_text(value, "siteKey")?,
        source_sha256: required_text(value, "sourceSha256")?,
        target_cutoff_at: required_text(value, "targetCutoffAt")?,
        terminal_at: required_text(value, "terminalAt")?,
    })
}

/// Decode one reconciled predecessor identity.
fn decode_predecessor(
    value: &JsonValue,
) -> Result<NativeRegistrationLifecyclePredecessor, NativeRegistrationLifecycleWireError> {
    // reject predecessor extensions before binding its terminal clock
    if !exact_keys(value, PREDECESSOR_FIELDS) {
        return Err(NativeRegistrationLifecycleWireError::Shape);
    }
    Ok(NativeRegistrationLifecyclePredecessor {
        epoch_witness_sha256: required_text(value, "epochWitnessSha256")?,
        reconciliation_sha256: required_text(value, "reconciliationSha256")?,
        registration_sha256: required_text(value, "registrationSha256")?,
        reserved_key_sha256: required_text(value, "reservedKeySha256")?,
        schedule_contract_sha256: required_text(value, "scheduleContractSha256")?,
        source_sha256: required_text(value, "sourceSha256")?,
        terminal_at: required_text(value, "terminalAt")?,
    })
}

/// Decode one compact metadata head with exact integer representation.
fn decode_metadata(
    value: &JsonValue,
) -> Result<NativeRegistrationLifecycleMetadata, NativeRegistrationLifecycleWireError> {
    // reject metadata extensions before validating its generation root
    if !exact_keys(value, METADATA_FIELDS) {
        return Err(NativeRegistrationLifecycleWireError::Shape);
    }
    Ok(NativeRegistrationLifecycleMetadata {
        finalized_prediction_count: bounded_u32(value, "finalizedPredictionCount")?,
        generation: bounded_u32(value, "generation")?,
        last_anchor_sha256: nullable_text(value, "lastAnchorSha256")?,
        root_sha256: required_text(value, "rootSha256")?,
        through_at: nullable_text(value, "throughAt")?,
    })
}

/// Re-run every source lifecycle semantic and cross-owner binding.
fn validate_lifecycle_status(
    lifecycle: &NativeRegistrationLifecycleStatus,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeRegistrationLifecycleWireError> {
    let epoch = instant(&lifecycle.epoch_at, time)?;
    let horizon = instant(&lifecycle.horizon_end_at, time)?;
    let snapshot = instant(&lifecycle.snapshot_at, time)?;
    // bind the top-level lifecycle to one valid rolling schedule head
    if lifecycle.contract_version != LIFECYCLE_VERSION
        || !valid_hash(&lifecycle.epoch_witness_sha256)
        || lifecycle.schedule_contract_sha256 != ADJUSTMENT_ROLLING_SCHEDULE_SHA256
        || snapshot < epoch
        || horizon <= epoch
    {
        return Err(NativeRegistrationLifecycleWireError::Binding);
    }
    let expected = [
        NativeMaintenanceCaptureFamily::Temperature,
        NativeMaintenanceCaptureFamily::Wind,
        NativeMaintenanceCaptureFamily::Rain,
    ];
    // preserve fixed family order and every active/predecessor/metadata crossbinding
    for (entry, family) in lifecycle.entries.iter().zip(expected) {
        validate_slot(entry, family, lifecycle, time)?;
        validate_predecessor(entry.predecessor.as_ref(), lifecycle, time)?;
        // reject malformed optional latest identities before state-specific checks
        if entry
            .latest_registration_sha256
            .as_deref()
            .is_some_and(|value| !valid_hash(value))
        {
            return Err(NativeRegistrationLifecycleWireError::Identity);
        }
        match entry.slot.state {
            NativeRegistrationSlotState::BusyV3 => {
                let registration = entry
                    .active_registration
                    .as_ref()
                    .ok_or(NativeRegistrationLifecycleWireError::Binding)?;
                let metadata = entry
                    .metadata
                    .as_ref()
                    .ok_or(NativeRegistrationLifecycleWireError::Binding)?;
                validate_active_registration(registration, codec, time)?;
                validate_metadata(metadata, &lifecycle.snapshot_at, time)?;
                let predecessor_sha = entry
                    .predecessor
                    .as_ref()
                    .map(|value| value.registration_sha256.as_str());
                let predecessor_matches = registration.predecessor_registration_sha256.as_deref()
                    == predecessor_sha
                    || predecessor_sha == Some(registration.registration_sha256.as_str());
                // crossbind the active row to the slot and reconciled predecessor
                if registration.family != entry.slot.family
                    || Some(registration.registration_sha256.as_str())
                        != entry.slot.registration_sha256.as_deref()
                    || Some(registration.registration_sha256.as_str())
                        != entry.latest_registration_sha256.as_deref()
                    || registration.terminal_at != entry.slot.terminal_at.as_deref().unwrap_or("")
                    || registration.epoch_witness_sha256 != lifecycle.epoch_witness_sha256
                    || !predecessor_matches
                {
                    return Err(NativeRegistrationLifecycleWireError::Binding);
                }
            }
            NativeRegistrationSlotState::BusyV2Legacy => {
                // legacy occupancy cannot confer v3 registration or metadata authority
                if entry.active_registration.is_some() || entry.metadata.is_some() {
                    return Err(NativeRegistrationLifecycleWireError::Binding);
                }
            }
            NativeRegistrationSlotState::Free => {
                let predecessor_sha = entry
                    .predecessor
                    .as_ref()
                    .map(|value| value.registration_sha256.as_str());
                // free slots may retain only the reconciled predecessor identity
                if entry.active_registration.is_some()
                    || entry.metadata.is_some()
                    || entry.latest_registration_sha256.as_deref() != predecessor_sha
                {
                    return Err(NativeRegistrationLifecycleWireError::Binding);
                }
            }
        }
    }
    Ok(())
}

/// Validate one slot against the shared schedule head.
fn validate_slot(
    entry: &NativeRegistrationLifecycleEntry,
    expected_family: NativeMaintenanceCaptureFamily,
    lifecycle: &NativeRegistrationLifecycleStatus,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeRegistrationLifecycleWireError> {
    let slot = &entry.slot;
    let occupied = slot.state != NativeRegistrationSlotState::Free;
    // bind occupancy fields and clocks to the authenticated schedule head
    if slot.contract_version != SLOT_VERSION
        || slot.family != expected_family
        || slot.epoch_witness_sha256 != lifecycle.epoch_witness_sha256
        || slot.schedule_contract_sha256 != lifecycle.schedule_contract_sha256
        || slot.horizon_end_at != lifecycle.horizon_end_at
        || occupied != slot.registration_sha256.is_some()
        || occupied != slot.terminal_at.is_some()
        || slot
            .registration_sha256
            .as_deref()
            .is_some_and(|value| !valid_hash(value))
        || slot
            .terminal_at
            .as_deref()
            .is_some_and(|value| instant(value, time).is_err())
    {
        return Err(NativeRegistrationLifecycleWireError::Binding);
    }
    Ok(())
}

/// Validate one reconciled predecessor under the lifecycle clocks.
fn validate_predecessor(
    predecessor: Option<&NativeRegistrationLifecyclePredecessor>,
    lifecycle: &NativeRegistrationLifecycleStatus,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeRegistrationLifecycleWireError> {
    let Some(predecessor) = predecessor else {
        return Ok(());
    };
    // require all predecessor identities before comparing lifecycle clocks
    if [
        predecessor.epoch_witness_sha256.as_str(),
        predecessor.reconciliation_sha256.as_str(),
        predecessor.registration_sha256.as_str(),
        predecessor.reserved_key_sha256.as_str(),
        predecessor.schedule_contract_sha256.as_str(),
        predecessor.source_sha256.as_str(),
    ]
    .into_iter()
    .any(|value| !valid_hash(value))
    {
        return Err(NativeRegistrationLifecycleWireError::Identity);
    }
    let terminal = instant(&predecessor.terminal_at, time)?;
    let epoch = instant(&lifecycle.epoch_at, time)?;
    let snapshot = instant(&lifecycle.snapshot_at, time)?;
    // crossbind the predecessor to the epoch and completed snapshot interval
    if predecessor.epoch_witness_sha256 != lifecycle.epoch_witness_sha256
        || predecessor.schedule_contract_sha256 != lifecycle.schedule_contract_sha256
        || terminal <= epoch
        || terminal > snapshot
    {
        return Err(NativeRegistrationLifecycleWireError::Binding);
    }
    Ok(())
}

/// Validate one complete active registration and its final-LF identity grammar.
fn validate_active_registration(
    registration: &NativeRegistrationActiveRegistration,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeRegistrationLifecycleWireError> {
    // refuse registrations outside the fixed site, contract, and hash grammar
    if registration.site_key != "ballydidean"
        || registration.schedule_contract_sha256 != ADJUSTMENT_ROLLING_SCHEDULE_SHA256
        || [
            registration.artifact_sha256.as_str(),
            registration.candidate_sha256.as_str(),
            registration.cohort_sha256.as_str(),
            registration.epoch_witness_sha256.as_str(),
            registration.policy_sha256.as_str(),
            registration.registration_sha256.as_str(),
            registration.reserved_key_sha256.as_str(),
            registration.schedule_contract_sha256.as_str(),
            registration.source_sha256.as_str(),
        ]
        .into_iter()
        .any(|value| !valid_hash(value))
        || registration
            .predecessor_registration_sha256
            .as_deref()
            .is_some_and(|value| !valid_hash(value))
    {
        return Err(NativeRegistrationLifecycleWireError::Identity);
    }
    // require every registration boundary to retain canonical UTC milliseconds
    for clock in [
        registration.interval_end_at.as_str(),
        registration.interval_start_at.as_str(),
        registration.target_cutoff_at.as_str(),
        registration.terminal_at.as_str(),
    ] {
        instant(clock, time)?;
    }
    let predecessor = registration
        .predecessor_registration_sha256
        .as_deref()
        .unwrap_or("none");
    let material = format!(
        "adjustment-shadow-registration/v3\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        registration.site_key,
        registration.family.wire(),
        registration.candidate_sha256,
        registration.artifact_sha256,
        registration.policy_sha256,
        registration.cohort_sha256,
        registration.reserved_key_sha256,
        registration.source_sha256,
        registration.epoch_witness_sha256,
        registration.schedule_contract_sha256,
        predecessor,
        registration.interval_start_at,
        registration.interval_end_at,
        registration.target_cutoff_at,
        registration.terminal_at,
    );
    let actual = codec
        .sha256(material.as_bytes())
        .ok_or(NativeRegistrationLifecycleWireError::Identity)?;
    // compare the final-lf preimage identity without accepting aliases
    if actual.as_str() != registration.registration_sha256 {
        return Err(NativeRegistrationLifecycleWireError::Identity);
    }
    Ok(())
}

/// Validate generation-zero or anchored nonempty metadata.
fn validate_metadata(
    metadata: &NativeRegistrationLifecycleMetadata,
    snapshot_at: &str,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeRegistrationLifecycleWireError> {
    // require every retained metadata identity before generation checks
    if !valid_hash(&metadata.root_sha256)
        || metadata
            .last_anchor_sha256
            .as_deref()
            .is_some_and(|value| !valid_hash(value))
    {
        return Err(NativeRegistrationLifecycleWireError::Identity);
    }
    // generation zero is the exact canonical empty metadata root
    // preserve the unique empty metadata root at generation zero
    if metadata.generation == 0 {
        // reject any range or anchor authority on the empty generation
        if metadata.finalized_prediction_count != 0
            || metadata.through_at.is_some()
            || metadata.last_anchor_sha256.is_some()
            || metadata.root_sha256 != EMPTY_METADATA_ROOT_SHA256
        {
            return Err(NativeRegistrationLifecycleWireError::Binding);
        }
        return Ok(());
    }
    let through = metadata
        .through_at
        .as_deref()
        .ok_or(NativeRegistrationLifecycleWireError::Binding)
        .and_then(|value| instant(value, time))?;
    let snapshot = instant(snapshot_at, time)?;
    // require a nonempty anchored range no later than the lifecycle snapshot
    if metadata.finalized_prediction_count < 1
        || metadata.last_anchor_sha256.is_none()
        || through > snapshot
    {
        return Err(NativeRegistrationLifecycleWireError::Binding);
    }
    Ok(())
}

/// Build the source canonical initialization request document.
pub(crate) fn bootstrap_value(bootstrap: &NativeAdjustmentRollingScheduleBootstrap) -> JsonValue {
    JsonValue::Object(vec![
        member("bootstrapSha256", string(&bootstrap.bootstrap_sha256)),
        member("contractVersion", string(bootstrap.contract_version)),
        member("epochAt", string(&bootstrap.epoch_at)),
        member(
            "epochWitnessSha256",
            string(&bootstrap.epoch_witness_sha256),
        ),
        member(
            "firstCompleteLocalDate",
            string(&bootstrap.first_complete_local_date),
        ),
        member("horizonEndAt", string(&bootstrap.horizon_end_at)),
        member(
            "scheduleContractSha256",
            string(bootstrap.schedule_contract_sha256),
        ),
    ])
}

/// Decode one family from the closed source set.
fn decode_family(
    value: &str,
) -> Result<NativeMaintenanceCaptureFamily, NativeRegistrationLifecycleWireError> {
    match value {
        "temperature" => Ok(NativeMaintenanceCaptureFamily::Temperature),
        "wind" => Ok(NativeMaintenanceCaptureFamily::Wind),
        "rain" => Ok(NativeMaintenanceCaptureFamily::Rain),
        _ => Err(NativeRegistrationLifecycleWireError::Shape),
    }
}

/// Decode one slot state from the closed database set.
fn decode_slot_state(
    value: &str,
) -> Result<NativeRegistrationSlotState, NativeRegistrationLifecycleWireError> {
    match value {
        "busy_v2_legacy" => Ok(NativeRegistrationSlotState::BusyV2Legacy),
        "busy_v3" => Ok(NativeRegistrationSlotState::BusyV3),
        "free" => Ok(NativeRegistrationSlotState::Free),
        _ => Err(NativeRegistrationLifecycleWireError::Shape),
    }
}

/// Resolve one exact canonical UTC millisecond instant.
fn instant(
    value: &str,
    time: &dyn CurrentTimeResolver,
) -> Result<i64, NativeRegistrationLifecycleWireError> {
    resolve_operator_capture_clock(value, time)
        .map(|value| value.epoch_milliseconds)
        .ok_or(NativeRegistrationLifecycleWireError::Clock)
}

/// Borrow one required array member.
fn array<'a>(
    value: &'a JsonValue,
    name: &str,
) -> Result<&'a [JsonValue], NativeRegistrationLifecycleWireError> {
    let Some(JsonValue::Array(values)) = field(value, name) else {
        return Err(NativeRegistrationLifecycleWireError::Shape);
    };
    Ok(values)
}

/// Decode one required UTF-16 JSON string.
fn required_text(
    value: &JsonValue,
    name: &str,
) -> Result<String, NativeRegistrationLifecycleWireError> {
    text(value, name).ok_or(NativeRegistrationLifecycleWireError::Shape)
}

/// Decode one explicit null or UTF-16 JSON string.
fn nullable_text(
    value: &JsonValue,
    name: &str,
) -> Result<Option<String>, NativeRegistrationLifecycleWireError> {
    match field(value, name) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .map(Some)
            .map_err(|_| NativeRegistrationLifecycleWireError::Shape),
        _ => Err(NativeRegistrationLifecycleWireError::Shape),
    }
}

/// Decode one explicit null or exact typed object.
fn optional_object<T>(
    value: &JsonValue,
    name: &str,
    decode: impl FnOnce(&JsonValue) -> Result<T, NativeRegistrationLifecycleWireError>,
) -> Result<Option<T>, NativeRegistrationLifecycleWireError> {
    match field(value, name) {
        Some(JsonValue::Null) => Ok(None),
        Some(object @ JsonValue::Object(_)) => decode(object).map(Some),
        _ => Err(NativeRegistrationLifecycleWireError::Shape),
    }
}

/// Decode one source-safe nonnegative 32-bit integer.
fn bounded_u32(value: &JsonValue, name: &str) -> Result<u32, NativeRegistrationLifecycleWireError> {
    let Some(JsonValue::Number(number)) = field(value, name) else {
        return Err(NativeRegistrationLifecycleWireError::Shape);
    };
    // mirror the source's nonnegative signed-32-bit integer admission
    if !number.is_finite() || number.fract() != 0.0 || *number < 0.0 || *number > i32::MAX as f64 {
        return Err(NativeRegistrationLifecycleWireError::Shape);
    }
    Ok(*number as u32)
}

/// Construct one ordered UTF-16 JSON object member.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Construct one exact UTF-16 JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
