//! Exact native projection of the future-only rolling maintenance schedule.

use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    sync::OnceLock,
};

use jiff::{Span, civil::Date};
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{NativePredecessorModelCalendar, NativeWirePrimitives};

const DAY_MILLISECONDS: i64 = 86_400_000;
const BOOTSTRAP_FINITE_DAYS: i64 = 180 + 7 + 90 + 7 + 366 + 334 + 7 + 62;

static CONTRACT_VALID: OnceLock<bool> = OnceLock::new();

/// Immutable source policy values whose canonical bytes own the schedule identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeAdjustmentRollingScheduleContract {
    pub calibration_gap_days: u16,
    pub contract_version: &'static str,
    pub horizon_extension: &'static str,
    pub late_closure_days: u16,
    pub maximum_month_alignment_days: u16,
    pub rain_calibration_days: u16,
    pub rain_confirmation_days: u16,
    pub rain_minimum_calibration_dates: u16,
    pub rain_minimum_training_dates: u16,
    pub source_history: &'static str,
    pub temperature_confirmation_days: u16,
    pub temperature_minimum_training_dates: u16,
    pub time_zone: &'static str,
    pub training_gap_days: u16,
    pub wind_confirmation_days: u16,
    pub wind_minimum_training_dates: u16,
}

/// Exact policy contract whose canonical LF-framed bytes own the schedule identity.
pub const ADJUSTMENT_ROLLING_SCHEDULE_CONTRACT: NativeAdjustmentRollingScheduleContract =
    NativeAdjustmentRollingScheduleContract {
        calibration_gap_days: 7,
        contract_version: "forecast-adjustment-rolling-registration-schedule/v3",
        horizon_extension: "successful_registered_member_terminal_only",
        late_closure_days: 7,
        maximum_month_alignment_days: 62,
        rain_calibration_days: 90,
        rain_confirmation_days: 334,
        rain_minimum_calibration_dates: 60,
        rain_minimum_training_dates: 180,
        source_history: "post_epoch_and_post_predecessor_terminal_only",
        temperature_confirmation_days: 366,
        temperature_minimum_training_dates: 60,
        time_zone: "America/Los_Angeles",
        training_gap_days: 7,
        wind_confirmation_days: 366,
        wind_minimum_training_dates: 60,
    };

/// Published canonical SHA-256 identity of the exact rolling schedule contract.
pub const ADJUSTMENT_ROLLING_SCHEDULE_SHA256: &str =
    "7c17f5d1a8e8249cd0aa4820638169e51f6edb3433017f50ab4c959e44c62f1f";

/// Exact authenticated epoch input for one finite schedule bootstrap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeAdjustmentRollingScheduleBootstrapInput<'a> {
    pub epoch_at: &'a str,
    pub epoch_witness_sha256: &'a str,
}

/// Complete finite initial horizon and its canonical unsigned identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeAdjustmentRollingScheduleBootstrap {
    pub bootstrap_sha256: String,
    pub contract_version: &'static str,
    pub epoch_at: String,
    pub epoch_witness_sha256: String,
    pub first_complete_local_date: String,
    pub horizon_end_at: String,
    pub schedule_contract_sha256: &'static str,
}

/// Exact value-blind rolling window request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeAdjustmentRollingWindowInput<'a> {
    pub epoch_at: &'a str,
    pub epoch_witness_sha256: &'a str,
    pub family: &'a str,
    pub fit_month: &'a str,
    pub predecessor_terminal_at: Option<&'a str>,
    pub requested_at: &'a str,
}

/// Complete future-only window plan and its canonical unsigned identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeAdjustmentRollingWindow {
    pub calibration_end_at: Option<String>,
    pub calibration_start_at: Option<String>,
    pub contract_version: &'static str,
    pub epoch_witness_sha256: String,
    pub family: String,
    pub fit_month: String,
    pub interval_end_at: String,
    pub interval_start_at: String,
    pub plan_sha256: String,
    pub planned_at: String,
    pub predecessor_terminal_at: Option<String>,
    pub schedule_contract_sha256: &'static str,
    pub source_floor_at: String,
    pub target_cutoff_at: String,
    pub terminal_at: String,
    pub training_end_at: String,
}

/// Exact authenticated clocks for one monotonic capture-horizon extension.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeAdjustmentRollingCaptureHorizonInput<'a> {
    pub current_end_at: &'a str,
    pub member_terminal_at: &'a str,
}

/// Closed schedule validation and native calendar failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceRollingScheduleError {
    Contract,
    Instant,
    Hash,
    Month,
    FamilyOrRequest,
    Predecessor,
    FutureMonth,
    CalendarSupport,
    Calendar,
    Overflow,
    Encoding,
}

impl Display for NativeMaintenanceRollingScheduleError {
    /// Preserve the source's bounded failure class without exposing input values.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Contract => "rolling schedule contract differs",
            Self::Instant => "rolling schedule instant is invalid",
            Self::Hash => "rolling schedule hash is invalid",
            Self::Month => "rolling schedule month is invalid",
            Self::FamilyOrRequest => "rolling schedule family or request clock is invalid",
            Self::Predecessor => "rolling schedule predecessor clock is invalid",
            Self::FutureMonth => "rolling schedule fit month is in the future",
            Self::CalendarSupport => "rolling schedule lacks post-epoch calendar support",
            Self::Calendar => "rolling schedule local midnight is invalid",
            Self::Overflow => "rolling schedule calendar arithmetic overflowed",
            Self::Encoding => "rolling schedule canonical encoding failed",
        })
    }
}

impl Error for NativeMaintenanceRollingScheduleError {}

/// Derive the exact finite initial horizon without claiming sufficient observations.
pub fn build_adjustment_rolling_schedule_bootstrap(
    input: NativeAdjustmentRollingScheduleBootstrapInput<'_>,
) -> Result<NativeAdjustmentRollingScheduleBootstrap, NativeMaintenanceRollingScheduleError> {
    let wire = NativeWirePrimitives::new();
    let calendar = admitted_calendar()?;
    admit_contract(&wire)?;
    instant(&wire, input.epoch_at)?;
    hash(input.epoch_witness_sha256)?;
    let epoch_local_date = local_date_at(&wire, &calendar, input.epoch_at)?;
    let first_complete_local_date = add_local_dates(&epoch_local_date, 1)?;
    let horizon_local_date = add_local_dates(&first_complete_local_date, BOOTSTRAP_FINITE_DAYS)?;
    let horizon_end_at = local_midnight_at(&wire, &calendar, &horizon_local_date)?;
    let unsigned = bootstrap_value(
        input.epoch_at,
        input.epoch_witness_sha256,
        &first_complete_local_date,
        &horizon_end_at,
    );
    let bootstrap_sha256 = canonical_sha256(&wire, &unsigned)?;
    Ok(NativeAdjustmentRollingScheduleBootstrap {
        bootstrap_sha256,
        contract_version: "adjustment-registration-schedule-bootstrap/v3",
        epoch_at: input.epoch_at.to_owned(),
        epoch_witness_sha256: input.epoch_witness_sha256.to_owned(),
        first_complete_local_date,
        horizon_end_at,
        schedule_contract_sha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256,
    })
}

/// Plan one exact value-blind rolling member from authenticated epoch and fit clocks.
pub fn build_adjustment_rolling_window(
    input: NativeAdjustmentRollingWindowInput<'_>,
) -> Result<NativeAdjustmentRollingWindow, NativeMaintenanceRollingScheduleError> {
    let wire = NativeWirePrimitives::new();
    let calendar = admitted_calendar()?;
    admit_contract(&wire)?;
    let epoch = instant(&wire, input.epoch_at)?;
    let requested = instant(&wire, input.requested_at)?;
    hash(input.epoch_witness_sha256)?;
    month(input.fit_month)?;
    // retain only the three source families and post-epoch request clocks
    if !matches!(input.family, "temperature" | "wind" | "rain") || requested < epoch {
        return Err(NativeMaintenanceRollingScheduleError::FamilyOrRequest);
    }
    let epoch_local_date = local_date_at(&wire, &calendar, input.epoch_at)?;
    let epoch_floor_date = add_local_dates(&epoch_local_date, 1)?;
    let epoch_floor = local_midnight_at(&wire, &calendar, &epoch_floor_date)?;
    let epoch_floor_milliseconds = instant(&wire, &epoch_floor)?;
    let mut source_floor_at = epoch_floor;
    let mut source_floor_milliseconds = epoch_floor_milliseconds;
    // successors cannot recycle a previously reserved member as new training history
    if let Some(predecessor) = input.predecessor_terminal_at {
        let predecessor_milliseconds = instant(&wire, predecessor)?;
        // keep predecessor clocks inside the authenticated epoch-to-request interval
        if predecessor_milliseconds < epoch || predecessor_milliseconds > requested {
            return Err(NativeMaintenanceRollingScheduleError::Predecessor);
        }
        // preserve the later source spelling exactly after comparing admitted instants
        if predecessor_milliseconds > epoch_floor_milliseconds {
            source_floor_at = predecessor.to_owned();
            source_floor_milliseconds = predecessor_milliseconds;
        }
    }
    let fit_cutoff_at = format!("{}-01T00:00:00.000Z", input.fit_month);
    let fit_cutoff = instant(&wire, &fit_cutoff_at)?;
    let request_local_date = local_date_at(&wire, &calendar, input.requested_at)?;
    let request_month = request_local_date
        .get(..7)
        .ok_or(NativeMaintenanceRollingScheduleError::Calendar)?;
    // prohibit an original-cutoff fit month that has not arrived at the request clock
    if input.fit_month > request_month || fit_cutoff > requested {
        return Err(NativeMaintenanceRollingScheduleError::FutureMonth);
    }
    let interval_start_date = format!("{}-01", next_month(request_month)?);
    let interval_days = if input.family == "rain" { 334 } else { 366 };
    let interval_start_at = local_midnight_at(&wire, &calendar, &interval_start_date)?;
    let interval_end_date = add_local_dates(&interval_start_date, interval_days)?;
    let interval_end_at = local_midnight_at(&wire, &calendar, &interval_end_date)?;
    let interval_start = canonical_instant(&wire, &interval_start_at)?;
    let interval_end = canonical_instant(&wire, &interval_end_at)?;
    let terminal = add_milliseconds(interval_end, 7 * DAY_MILLISECONDS)?;
    let terminal_at = format_instant(&wire, terminal)?;
    let calibration_end = (input.family == "rain")
        .then(|| add_milliseconds(fit_cutoff, -7 * DAY_MILLISECONDS))
        .transpose()?;
    let calibration_start = calibration_end
        .map(|end| add_milliseconds(end, -90 * DAY_MILLISECONDS))
        .transpose()?;
    let training_base = calibration_start.unwrap_or(fit_cutoff);
    let training_end = add_milliseconds(training_base, -7 * DAY_MILLISECONDS)?;
    let minimum_training_days = if input.family == "rain" { 180 } else { 60 };
    let training_span = training_end
        .checked_sub(source_floor_milliseconds)
        .ok_or(NativeMaintenanceRollingScheduleError::Overflow)?;
    let minimum_training_span = minimum_training_days * DAY_MILLISECONDS;
    let calibration_before_source =
        calibration_start.is_some_and(|start| start < source_floor_milliseconds);
    let calibration_gap_short = calibration_end.is_some_and(|end| {
        interval_start
            .checked_sub(end)
            .is_none_or(|span| span < 7 * DAY_MILLISECONDS)
    });
    // potential calendar support never replaces actual fitter observation gates
    if training_span < minimum_training_span
        || calibration_before_source
        || interval_start <= requested
        || calibration_gap_short
    {
        return Err(NativeMaintenanceRollingScheduleError::CalendarSupport);
    }
    let calibration_end_at = calibration_end
        .map(|value| format_instant(&wire, value))
        .transpose()?;
    let calibration_start_at = calibration_start
        .map(|value| format_instant(&wire, value))
        .transpose()?;
    let training_end_at = format_instant(&wire, training_end)?;
    let unsigned = window_value(
        &calibration_end_at,
        &calibration_start_at,
        input,
        &interval_end_at,
        &interval_start_at,
        &source_floor_at,
        &terminal_at,
        &training_end_at,
    );
    let plan_sha256 = canonical_sha256(&wire, &unsigned)?;
    Ok(NativeAdjustmentRollingWindow {
        calibration_end_at,
        calibration_start_at,
        contract_version: "adjustment-shadow-registration-window-plan/v3",
        epoch_witness_sha256: input.epoch_witness_sha256.to_owned(),
        family: input.family.to_owned(),
        fit_month: input.fit_month.to_owned(),
        interval_end_at,
        interval_start_at,
        plan_sha256,
        planned_at: input.requested_at.to_owned(),
        predecessor_terminal_at: input.predecessor_terminal_at.map(str::to_owned),
        schedule_contract_sha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256,
        source_floor_at,
        target_cutoff_at: terminal_at.clone(),
        terminal_at,
        training_end_at,
    })
}

/// Extend the capture horizon only to an already authenticated member terminal clock.
pub fn extend_adjustment_rolling_capture_horizon(
    input: NativeAdjustmentRollingCaptureHorizonInput<'_>,
) -> Result<String, NativeMaintenanceRollingScheduleError> {
    let wire = NativeWirePrimitives::new();
    let current = instant(&wire, input.current_end_at)?;
    let member = instant(&wire, input.member_terminal_at)?;
    Ok(if member > current {
        input.member_terminal_at.to_owned()
    } else {
        input.current_end_at.to_owned()
    })
}

/// Expose the exact predecessor Los Angeles local date for controller scheduling.
pub fn adjustment_rolling_local_date_at(
    value: &str,
) -> Result<String, NativeMaintenanceRollingScheduleError> {
    let wire = NativeWirePrimitives::new();
    let calendar = admitted_calendar()?;
    instant(&wire, value)?;
    local_date_at(&wire, &calendar, value)
}

/// Admit the generated predecessor calendar before using its fixed transition table.
fn admitted_calendar()
-> Result<NativePredecessorModelCalendar, NativeMaintenanceRollingScheduleError> {
    NativePredecessorModelCalendar::new().ok_or(NativeMaintenanceRollingScheduleError::Calendar)
}

/// Verify the published constant against the exact canonical LF-framed policy bytes once.
fn admit_contract(
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceRollingScheduleError> {
    let valid = CONTRACT_VALID.get_or_init(|| {
        canonical_sha256(wire, &contract_value()).as_deref()
            == Ok(ADJUSTMENT_ROLLING_SCHEDULE_SHA256)
    });
    valid
        .then_some(())
        .ok_or(NativeMaintenanceRollingScheduleError::Contract)
}

/// Read one strict source instant through the approved native canonical UTC primitive.
fn instant(
    wire: &NativeWirePrimitives,
    value: &str,
) -> Result<i64, NativeMaintenanceRollingScheduleError> {
    // retain the source's input-only 20xx restriction before canonical date admission
    if value.as_bytes().get(..2) != Some(b"20") {
        return Err(NativeMaintenanceRollingScheduleError::Instant);
    }
    canonical_instant(wire, value)
}

/// Admit one derived four-digit canonical UTC instant beyond the input-only 20xx span.
fn canonical_instant(
    wire: &NativeWirePrimitives,
    value: &str,
) -> Result<i64, NativeMaintenanceRollingScheduleError> {
    let bytes = value.as_bytes();
    // retain the source's exact four-digit millisecond UTC framing
    if bytes.len() != 24
        || ![4, 7, 10, 13, 16, 19, 23]
            .into_iter()
            .zip(*b"--T::.Z")
            .all(|(index, expected)| bytes.get(index) == Some(&expected))
        || bytes.iter().enumerate().any(|(index, byte)| {
            ![4, 7, 10, 13, 16, 19, 23].contains(&index) && !byte.is_ascii_digit()
        })
    {
        return Err(NativeMaintenanceRollingScheduleError::Instant);
    }
    wire.canonical_utc_milliseconds(value)
        .ok_or(NativeMaintenanceRollingScheduleError::Instant)
}

/// Require one exact lower-hex SHA-256 identity.
fn hash(value: &str) -> Result<(), NativeMaintenanceRollingScheduleError> {
    // retain only the source's lowercase ASCII digest alphabet
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(NativeMaintenanceRollingScheduleError::Hash);
    }
    Ok(())
}

/// Require one exact 20xx month label.
fn month(value: &str) -> Result<(), NativeMaintenanceRollingScheduleError> {
    let bytes = value.as_bytes();
    let framed = bytes.len() == 7
        && bytes.get(..2) == Some(b"20")
        && bytes.get(4) == Some(&b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || byte.is_ascii_digit());
    let month = framed.then(|| (bytes[5] - b'0') * 10 + (bytes[6] - b'0'));
    let admitted = month.is_some_and(|month| (1..=12).contains(&month));
    admitted
        .then_some(())
        .ok_or(NativeMaintenanceRollingScheduleError::Month)
}

/// Advance one admitted month label through the Gregorian year boundary.
fn next_month(value: &str) -> Result<String, NativeMaintenanceRollingScheduleError> {
    month(value)?;
    let year = value[..4]
        .parse::<u16>()
        .map_err(|_| NativeMaintenanceRollingScheduleError::Month)?;
    let month = value[5..]
        .parse::<u8>()
        .map_err(|_| NativeMaintenanceRollingScheduleError::Month)?;
    // carry December into the following calendar year
    if month == 12 {
        Ok(format!("{:04}-01", year + 1))
    } else {
        Ok(format!("{year:04}-{:02}", month + 1))
    }
}

/// Advance a local date label without assuming that its local day has twenty-four hours.
fn add_local_dates(
    value: &str,
    days: i64,
) -> Result<String, NativeMaintenanceRollingScheduleError> {
    let date = value
        .parse::<Date>()
        .map_err(|_| NativeMaintenanceRollingScheduleError::Calendar)?;
    let span = Span::new()
        .try_days(days)
        .map_err(|_| NativeMaintenanceRollingScheduleError::Overflow)?;
    date.checked_add(span)
        .map(|value| value.to_string())
        .map_err(|_| NativeMaintenanceRollingScheduleError::Overflow)
}

/// Resolve one exact LA local date through the pinned predecessor transition authority.
fn local_date_at(
    wire: &NativeWirePrimitives,
    calendar: &NativePredecessorModelCalendar,
    value: &str,
) -> Result<String, NativeMaintenanceRollingScheduleError> {
    canonical_instant(wire, value)?;
    calendar
        .features(value)
        .map(|features| features.local_date)
        .map_err(|_| NativeMaintenanceRollingScheduleError::Calendar)
}

/// Resolve LA midnight using the same three offset-correction iterations as the source.
fn local_midnight_at(
    wire: &NativeWirePrimitives,
    calendar: &NativePredecessorModelCalendar,
    local_date: &str,
) -> Result<String, NativeMaintenanceRollingScheduleError> {
    let target = canonical_instant(wire, &format!("{local_date}T00:00:00.000Z"))?;
    let mut guess = canonical_instant(wire, &format!("{local_date}T08:00:00.000Z"))?;
    // reproduce the predecessor's bounded wall-clock offset correction
    for _ in 0..3 {
        let guess_at = format_instant(wire, guess)?;
        let offset = calendar
            .features(&guess_at)
            .map_err(|_| NativeMaintenanceRollingScheduleError::Calendar)?
            .offset_seconds;
        let observed = guess
            .checked_add(i64::from(offset) * 1_000)
            .ok_or(NativeMaintenanceRollingScheduleError::Overflow)?;
        guess = guess
            .checked_add(
                target
                    .checked_sub(observed)
                    .ok_or(NativeMaintenanceRollingScheduleError::Overflow)?,
            )
            .ok_or(NativeMaintenanceRollingScheduleError::Overflow)?;
    }
    let result = format_instant(wire, guess)?;
    // reject an impossible transition-table resolution rather than changing the date
    if local_date_at(wire, calendar, &result)? != local_date {
        return Err(NativeMaintenanceRollingScheduleError::Calendar);
    }
    Ok(result)
}

/// Shift one UTC instant by an exact source duration.
fn add_milliseconds(
    value: i64,
    milliseconds: i64,
) -> Result<i64, NativeMaintenanceRollingScheduleError> {
    value
        .checked_add(milliseconds)
        .ok_or(NativeMaintenanceRollingScheduleError::Overflow)
}

/// Format one derived instant through the same canonical UTC primitive used for admission.
fn format_instant(
    wire: &NativeWirePrimitives,
    milliseconds: i64,
) -> Result<String, NativeMaintenanceRollingScheduleError> {
    wire.format_utc_milliseconds(milliseconds)
        .ok_or(NativeMaintenanceRollingScheduleError::Instant)
}

/// Hash one recursively sorted canonical JSON value including its terminating line feed.
fn canonical_sha256(
    wire: &NativeWirePrimitives,
    value: &JsonValue,
) -> Result<String, NativeMaintenanceRollingScheduleError> {
    let bytes = wire
        .encode_sorted_json(value)
        .ok_or(NativeMaintenanceRollingScheduleError::Encoding)?;
    Ok(lower_hex(wire.sha256(&bytes)))
}

/// Encode one SHA-256 result in the source's lowercase hex spelling.
fn lower_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    // emit each digest byte without another formatting dependency
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}

/// Project the exact canonical contract value that owns the published schedule hash.
fn contract_value() -> JsonValue {
    let contract = ADJUSTMENT_ROLLING_SCHEDULE_CONTRACT;
    object(vec![
        number("calibrationGapDays", contract.calibration_gap_days),
        text("contractVersion", contract.contract_version),
        text("horizonExtension", contract.horizon_extension),
        number("lateClosureDays", contract.late_closure_days),
        number(
            "maximumMonthAlignmentDays",
            contract.maximum_month_alignment_days,
        ),
        number("rainCalibrationDays", contract.rain_calibration_days),
        number("rainConfirmationDays", contract.rain_confirmation_days),
        number(
            "rainMinimumCalibrationDates",
            contract.rain_minimum_calibration_dates,
        ),
        number(
            "rainMinimumTrainingDates",
            contract.rain_minimum_training_dates,
        ),
        text("sourceHistory", contract.source_history),
        number(
            "temperatureConfirmationDays",
            contract.temperature_confirmation_days,
        ),
        number(
            "temperatureMinimumTrainingDates",
            contract.temperature_minimum_training_dates,
        ),
        text("timeZone", contract.time_zone),
        number("trainingGapDays", contract.training_gap_days),
        number("windConfirmationDays", contract.wind_confirmation_days),
        number(
            "windMinimumTrainingDates",
            contract.wind_minimum_training_dates,
        ),
    ])
}

/// Project the exact unsigned bootstrap fields before deriving its identity.
fn bootstrap_value(
    epoch_at: &str,
    epoch_witness_sha256: &str,
    first_complete_local_date: &str,
    horizon_end_at: &str,
) -> JsonValue {
    object(vec![
        text(
            "contractVersion",
            "adjustment-registration-schedule-bootstrap/v3",
        ),
        text("epochAt", epoch_at),
        text("epochWitnessSha256", epoch_witness_sha256),
        text("firstCompleteLocalDate", first_complete_local_date),
        text("horizonEndAt", horizon_end_at),
        text("scheduleContractSha256", ADJUSTMENT_ROLLING_SCHEDULE_SHA256),
    ])
}

/// Project the exact unsigned window fields before deriving its identity.
#[allow(clippy::too_many_arguments)]
fn window_value(
    calibration_end_at: &Option<String>,
    calibration_start_at: &Option<String>,
    input: NativeAdjustmentRollingWindowInput<'_>,
    interval_end_at: &str,
    interval_start_at: &str,
    source_floor_at: &str,
    terminal_at: &str,
    training_end_at: &str,
) -> JsonValue {
    object(vec![
        optional_text("calibrationEndAt", calibration_end_at.as_deref()),
        optional_text("calibrationStartAt", calibration_start_at.as_deref()),
        text(
            "contractVersion",
            "adjustment-shadow-registration-window-plan/v3",
        ),
        text("epochWitnessSha256", input.epoch_witness_sha256),
        text("family", input.family),
        text("fitMonth", input.fit_month),
        text("intervalEndAt", interval_end_at),
        text("intervalStartAt", interval_start_at),
        text("plannedAt", input.requested_at),
        optional_text("predecessorTerminalAt", input.predecessor_terminal_at),
        text("scheduleContractSha256", ADJUSTMENT_ROLLING_SCHEDULE_SHA256),
        text("sourceFloorAt", source_floor_at),
        text("targetCutoffAt", terminal_at),
        text("terminalAt", terminal_at),
        text("trainingEndAt", training_end_at),
    ])
}

/// Construct one JSON object from already admitted members.
fn object(fields: Vec<(Vec<u16>, JsonValue)>) -> JsonValue {
    JsonValue::Object(fields)
}

/// Construct one exact UTF-16 string member.
fn text(name: &str, value: &str) -> (Vec<u16>, JsonValue) {
    (
        name.encode_utf16().collect(),
        JsonValue::String(value.encode_utf16().collect()),
    )
}

/// Construct one nullable exact UTF-16 string member.
fn optional_text(name: &str, value: Option<&str>) -> (Vec<u16>, JsonValue) {
    match value {
        Some(value) => text(name, value),
        None => (name.encode_utf16().collect(), JsonValue::Null),
    }
}

/// Construct one source-safe integer member.
fn number(name: &str, value: u16) -> (Vec<u16>, JsonValue) {
    (
        name.encode_utf16().collect(),
        JsonValue::Number(f64::from(value)),
    )
}

#[cfg(test)]
#[path = "native_maintenance_rolling_schedule_test.rs"]
mod tests;
