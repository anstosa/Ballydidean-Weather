//! Exact native registration policy, cohort, rolling identity, and no-action artifacts.

use std::fmt::{self, Display, Formatter};

use weather_api::CurrentTimeResolver;
use weather_contracts::{AdjustmentShadowFamily, JsonValue};

use crate::MaintenanceCanonicalJson;
use crate::capture_epoch_witness::resolve_operator_capture_clock;
use crate::maintenance_canonical_json::valid_hash;
use crate::native_maintenance_rolling_schedule::{
    ADJUSTMENT_ROLLING_SCHEDULE_SHA256, NativeAdjustmentRollingWindow,
};

// exact predecessor policy module identity proved by the Node differential fixture
const POLICY_MODULE_SHA256: &str =
    "c0166ae80ceec09ae957778333ea58a60c6827fe1b0ba5ac62c539f7a1b8550b";

/// Closed construction and canonical identity failures for registration artifacts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceRegistrationPolicyError {
    Input,
    Schedule,
    Clock,
    Codec,
}

impl Display for NativeMaintenanceRegistrationPolicyError {
    /// Render one bounded construction failure without reflecting authority material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "maintenance registration input is invalid",
            Self::Schedule => "maintenance registration schedule differs",
            Self::Clock => "maintenance registration clock is invalid",
            Self::Codec => "maintenance registration serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceRegistrationPolicyError {}

/// Canonical installed policy descriptor and both source identities.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeAdjustmentRegistrationPolicyDescriptor {
    pub descriptor: JsonValue,
    pub bytes: Vec<u8>,
    pub policy_module_sha256: String,
    pub policy_sha256: String,
}

/// Exact archive-derived registration cohort descriptor and identity.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeAdjustmentRegistrationCohortDescriptor {
    pub descriptor: JsonValue,
    pub bytes: Vec<u8>,
    pub cohort_sha256: String,
}

/// Typed source inputs for one archive-derived registration cohort.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeAdjustmentRegistrationCohortInput<'a> {
    pub cutoff_at: &'a str,
    pub due_month: &'a str,
    pub epoch_witness_sha256: &'a str,
    pub family: AdjustmentShadowFamily,
    pub historical_member_root_sha256: &'a str,
    pub input_manifest_sha256: &'a str,
}

/// Typed non-calendar inputs bound to one independently admitted rolling window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeAdjustmentRollingShadowRegistrationInput<'a> {
    pub artifact_sha256: &'a str,
    pub candidate_sha256: &'a str,
    pub cohort_sha256: &'a str,
    pub family: AdjustmentShadowFamily,
    pub policy_sha256: &'a str,
    pub predecessor_registration_sha256: Option<&'a str>,
    pub reserved_key_sha256: &'a str,
    pub source_sha256: &'a str,
}

/// Exact source plan plus its public rolling registration identity.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeAdjustmentRollingShadowRegistration {
    pub plan: JsonValue,
    pub registration: JsonValue,
    pub registration_sha256: String,
}

/// Closed nonmutating terminal outcomes admitted by the source controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeAdjustmentTerminalNoActionDisposition {
    Rejected,
    ResourceRefused,
    SupportFailed,
}

impl NativeAdjustmentTerminalNoActionDisposition {
    /// Return the exact source wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rejected => "rejected",
            Self::ResourceRefused => "resource_refused",
            Self::SupportFailed => "support_failed",
        }
    }
}

/// Typed clocks and immutable identities for one completed no-action result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeAdjustmentTerminalNoActionReceiptInput<'a> {
    pub completed_at: &'a str,
    pub disposition: NativeAdjustmentTerminalNoActionDisposition,
    pub finalized_at: &'a str,
    pub full_member_root_sha256: &'a str,
    pub policy_report_sha256: &'a str,
    pub registration_sha256: &'a str,
}

/// Exact noncircular terminal identity and completed receipt bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeAdjustmentTerminalNoActionReceipt {
    pub identity: JsonValue,
    pub identity_bytes: Vec<u8>,
    pub action_sha256: String,
    pub receipt: JsonValue,
    pub receipt_bytes: Vec<u8>,
}

/// Build the policy descriptor from the pinned predecessor policy module identity.
pub fn build_native_adjustment_registration_policy_descriptor(
    family: AdjustmentShadowFamily,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<NativeAdjustmentRegistrationPolicyDescriptor, NativeMaintenanceRegistrationPolicyError>
{
    use NativeMaintenanceRegistrationPolicyError as Error;

    let policy_module_sha256 = POLICY_MODULE_SHA256.to_owned();
    // refuse a broken digest owner before projecting source authority
    if !valid_hash(&policy_module_sha256) || !valid_hash(ADJUSTMENT_ROLLING_SCHEDULE_SHA256) {
        return Err(Error::Codec);
    }
    let descriptor = object(vec![
        string_field("contractVersion", "adjustment-registration-policy/v3"),
        string_field("family", family_name(family)),
        string_field("policyModuleSha256", &policy_module_sha256),
        string_field("policyVersion", policy_version(family)),
        string_field("scheduleContractSha256", ADJUSTMENT_ROLLING_SCHEDULE_SHA256),
    ]);
    let bytes = codec
        .encode_canonical_json(&descriptor)
        .ok_or(Error::Codec)?;
    let policy_sha256 = codec
        .sha256(&bytes)
        .ok_or(Error::Codec)?
        .as_str()
        .to_owned();
    Ok(NativeAdjustmentRegistrationPolicyDescriptor {
        descriptor,
        bytes,
        policy_module_sha256,
        policy_sha256,
    })
}

/// Build one exact archive-derived monthly fit cohort descriptor.
pub fn build_native_adjustment_registration_cohort_descriptor(
    input: NativeAdjustmentRegistrationCohortInput<'_>,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeAdjustmentRegistrationCohortDescriptor, NativeMaintenanceRegistrationPolicyError>
{
    use NativeMaintenanceRegistrationPolicyError as Error;

    // admit the closed source month, clock, and independent archive identities
    if !valid_month(input.due_month)
        || !valid_instant(input.cutoff_at, time)
        || !valid_hash(input.epoch_witness_sha256)
        || !valid_hash(input.historical_member_root_sha256)
        || !valid_hash(input.input_manifest_sha256)
    {
        return Err(Error::Input);
    }
    let descriptor = object(vec![
        string_field("contractVersion", "adjustment-registration-cohort/v3"),
        string_field("cutoffAt", input.cutoff_at),
        string_field("dueMonth", input.due_month),
        string_field("epochWitnessSha256", input.epoch_witness_sha256),
        string_field("family", family_name(input.family)),
        string_field(
            "historicalMemberRootSha256",
            input.historical_member_root_sha256,
        ),
        string_field("inputManifestSha256", input.input_manifest_sha256),
    ]);
    let bytes = codec
        .encode_canonical_json(&descriptor)
        .ok_or(Error::Codec)?;
    let cohort_sha256 = codec
        .sha256(&bytes)
        .ok_or(Error::Codec)?
        .as_str()
        .to_owned();
    Ok(NativeAdjustmentRegistrationCohortDescriptor {
        descriptor,
        bytes,
        cohort_sha256,
    })
}

/// Bind one admitted native rolling window to the exact public registration grammar.
pub fn build_native_adjustment_rolling_shadow_registration(
    input: NativeAdjustmentRollingShadowRegistrationInput<'_>,
    window: &NativeAdjustmentRollingWindow,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeAdjustmentRollingShadowRegistration, NativeMaintenanceRegistrationPolicyError> {
    use NativeMaintenanceRegistrationPolicyError as Error;

    let hashes = [
        input.artifact_sha256,
        input.candidate_sha256,
        input.cohort_sha256,
        input.policy_sha256,
        input.reserved_key_sha256,
        input.source_sha256,
        window.epoch_witness_sha256.as_str(),
    ];
    // close every independently supplied identity before hashing the registration
    if !hashes.into_iter().all(valid_hash)
        || input
            .predecessor_registration_sha256
            .is_some_and(|value| !valid_hash(value))
    {
        return Err(Error::Input);
    }
    // bind only the reviewed schedule owner and the requested source family
    if window.family != family_name(input.family)
        || window.contract_version != "adjustment-shadow-registration-window-plan/v3"
        || window.schedule_contract_sha256 != ADJUSTMENT_ROLLING_SCHEDULE_SHA256
    {
        return Err(Error::Schedule);
    }
    let clocks = [
        window.interval_start_at.as_str(),
        window.interval_end_at.as_str(),
        window.target_cutoff_at.as_str(),
        window.terminal_at.as_str(),
    ];
    // retain exact canonical UTC clocks at every public registration boundary
    if !clocks.into_iter().all(|value| valid_instant(value, time)) {
        return Err(Error::Clock);
    }
    let plan = rolling_plan_value(window, codec)?;
    let predecessor = input.predecessor_registration_sha256.unwrap_or("none");
    let identity = format!(
        "adjustment-shadow-registration/v3\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        "ballydidean",
        family_name(input.family),
        input.candidate_sha256,
        input.artifact_sha256,
        input.policy_sha256,
        input.cohort_sha256,
        input.reserved_key_sha256,
        input.source_sha256,
        window.epoch_witness_sha256,
        window.schedule_contract_sha256,
        predecessor,
        window.interval_start_at,
        window.interval_end_at,
        window.target_cutoff_at,
        window.terminal_at,
    );
    let registration_sha256 = codec
        .sha256(identity.as_bytes())
        .ok_or(Error::Codec)?
        .as_str()
        .to_owned();
    let registration = object(vec![
        string_field("artifactSha256", input.artifact_sha256),
        string_field("candidateSha256", input.candidate_sha256),
        string_field("cohortSha256", input.cohort_sha256),
        string_field("epochWitnessSha256", &window.epoch_witness_sha256),
        string_field("family", family_name(input.family)),
        string_field("intervalEndAt", &window.interval_end_at),
        string_field("intervalStartAt", &window.interval_start_at),
        string_field("policySha256", input.policy_sha256),
        nullable_string_field(
            "predecessorRegistrationSha256",
            input.predecessor_registration_sha256,
        ),
        string_field("registrationSha256", &registration_sha256),
        string_field("reservedKeySha256", input.reserved_key_sha256),
        string_field("scheduleContractSha256", window.schedule_contract_sha256),
        string_field("siteKey", "ballydidean"),
        string_field("sourceSha256", input.source_sha256),
        string_field("targetCutoffAt", &window.target_cutoff_at),
        string_field("terminalAt", &window.terminal_at),
    ]);
    Ok(NativeAdjustmentRollingShadowRegistration {
        plan,
        registration,
        registration_sha256,
    })
}

/// Build one noncircular no-action identity and its completed receipt.
pub fn build_native_adjustment_terminal_no_action_receipt(
    input: NativeAdjustmentTerminalNoActionReceiptInput<'_>,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeAdjustmentTerminalNoActionReceipt, NativeMaintenanceRegistrationPolicyError> {
    use NativeMaintenanceRegistrationPolicyError as Error;

    let completed = resolve_operator_capture_clock(input.completed_at, time).ok_or(Error::Clock)?;
    let finalized = resolve_operator_capture_clock(input.finalized_at, time).ok_or(Error::Clock)?;
    // admit only complete immutable identities after the actual finalization clock
    if completed.utc != input.completed_at
        || finalized.utc != input.finalized_at
        || completed.epoch_milliseconds < finalized.epoch_milliseconds
        || !valid_hash(input.full_member_root_sha256)
        || !valid_hash(input.policy_report_sha256)
        || !valid_hash(input.registration_sha256)
    {
        return Err(Error::Input);
    }
    let disposition = input.disposition.as_str();
    let identity = object(vec![
        string_field(
            "contractVersion",
            "adjustment-terminal-no-action-identity/v3",
        ),
        string_field("disposition", disposition),
        string_field("policyReportSha256", input.policy_report_sha256),
        string_field("registrationSha256", input.registration_sha256),
    ]);
    let identity_bytes = codec.encode_canonical_json(&identity).ok_or(Error::Codec)?;
    let action_sha256 = codec
        .sha256(&identity_bytes)
        .ok_or(Error::Codec)?
        .as_str()
        .to_owned();
    let receipt = object(vec![
        string_field("actionSha256", &action_sha256),
        string_field("completedAt", input.completed_at),
        string_field(
            "contractVersion",
            "adjustment-terminal-no-action-receipt/v3",
        ),
        string_field("disposition", disposition),
        string_field("fullMemberRootSha256", input.full_member_root_sha256),
        string_field("policyReportSha256", input.policy_report_sha256),
        string_field("registrationSha256", input.registration_sha256),
        string_field("state", "verified_no_action"),
    ]);
    let receipt_bytes = codec.encode_canonical_json(&receipt).ok_or(Error::Codec)?;
    Ok(NativeAdjustmentTerminalNoActionReceipt {
        identity,
        identity_bytes,
        action_sha256,
        receipt,
        receipt_bytes,
    })
}

/// Rebuild the complete source plan and verify the schedule owner's identity.
fn rolling_plan_value(
    window: &NativeAdjustmentRollingWindow,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<JsonValue, NativeMaintenanceRegistrationPolicyError> {
    use NativeMaintenanceRegistrationPolicyError as Error;

    let mut fields = vec![
        nullable_owned_string_field("calibrationEndAt", window.calibration_end_at.as_deref()),
        nullable_owned_string_field("calibrationStartAt", window.calibration_start_at.as_deref()),
        string_field("contractVersion", window.contract_version),
        string_field("epochWitnessSha256", &window.epoch_witness_sha256),
        string_field("family", &window.family),
        string_field("fitMonth", &window.fit_month),
        string_field("intervalEndAt", &window.interval_end_at),
        string_field("intervalStartAt", &window.interval_start_at),
        string_field("plannedAt", &window.planned_at),
        nullable_owned_string_field(
            "predecessorTerminalAt",
            window.predecessor_terminal_at.as_deref(),
        ),
        string_field("scheduleContractSha256", window.schedule_contract_sha256),
        string_field("sourceFloorAt", &window.source_floor_at),
        string_field("targetCutoffAt", &window.target_cutoff_at),
        string_field("terminalAt", &window.terminal_at),
        string_field("trainingEndAt", &window.training_end_at),
    ];
    let unsigned = object(fields.clone());
    let bytes = codec.encode_canonical_json(&unsigned).ok_or(Error::Codec)?;
    let plan_sha256 = codec.sha256(&bytes).ok_or(Error::Codec)?;
    // refuse typed schedule fields that no longer reproduce their source plan identity
    if plan_sha256.as_str() != window.plan_sha256 {
        return Err(Error::Schedule);
    }
    fields.push(string_field("planSha256", &window.plan_sha256));
    Ok(object(fields))
}

/// Return one source family spelling.
const fn family_name(family: AdjustmentShadowFamily) -> &'static str {
    match family {
        AdjustmentShadowFamily::Temperature => "temperature",
        AdjustmentShadowFamily::Wind => "wind",
        AdjustmentShadowFamily::Rain => "rain",
    }
}

/// Return the installed source policy contract for one family.
const fn policy_version(family: AdjustmentShadowFamily) -> &'static str {
    match family {
        AdjustmentShadowFamily::Temperature => "temperature-maintenance-policy/v2",
        AdjustmentShadowFamily::Wind => "wind-maintenance-policy/v2",
        AdjustmentShadowFamily::Rain => "rain-maintenance-policy/v2",
    }
}

/// Recognize the source's 20xx monthly fit label.
fn valid_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    let framed = bytes.len() == 7
        && bytes.get(..2) == Some(b"20")
        && bytes.get(4) == Some(&b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || byte.is_ascii_digit());
    let month = framed.then(|| (bytes[5] - b'0') * 10 + (bytes[6] - b'0'));
    month.is_some_and(|month| (1..=12).contains(&month))
}

/// Admit one canonical source instant through the shared native clock owner.
fn valid_instant(value: &str, time: &dyn CurrentTimeResolver) -> bool {
    resolve_operator_capture_clock(value, time).is_some_and(|resolved| resolved.utc == value)
}

/// Construct one generated JSON object without introducing a map dependency.
fn object(fields: Vec<(Vec<u16>, JsonValue)>) -> JsonValue {
    JsonValue::Object(fields)
}

/// Construct one generated JSON string member.
fn string_field(key: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

/// Construct one borrowed nullable generated JSON string member.
fn nullable_string_field(key: &str, value: Option<&str>) -> (Vec<u16>, JsonValue) {
    nullable_owned_string_field(key, value)
}

/// Construct one nullable generated JSON string member.
fn nullable_owned_string_field(key: &str, value: Option<&str>) -> (Vec<u16>, JsonValue) {
    (
        key.encode_utf16().collect(),
        value.map_or(JsonValue::Null, |value| {
            JsonValue::String(value.encode_utf16().collect())
        }),
    )
}
