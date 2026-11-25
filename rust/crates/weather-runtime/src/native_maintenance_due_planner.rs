//! Exact oldest-first daily and monthly maintenance due planning.

use std::collections::BTreeSet;
use std::fmt::{self, Display, Formatter};
use std::future::{Future, ready};
use std::pin::Pin;

use jiff::{Span, civil::Date};
use weather_api::CurrentTimeResolver;
use weather_contracts::{JsDateOrString, JsonValue};
use weather_edge::ArchiveWirePrimitives;

use crate::maintenance_canonical_json::{text, valid_hash};
use crate::{
    ADJUSTMENT_ROLLING_SCHEDULE_SHA256, NativeMaintenanceCaptureFamily,
    NativeMaintenanceJournalRecord, NativeMaintenanceJournalStore,
    NativeMaintenanceJournalStoreError, NativePredecessorModelCalendar,
    NativeRegistrationScheduleStatus, NativeRegistrationSlotState, NativeWirePrimitives,
};

const DAILY_LIMIT: usize = 7;
const MAXIMUM_DUE_KEYS: usize = 4_096;
const DAY_MILLISECONDS: i64 = 86_400_000;
const SCHEDULE_VERSION: &str = "adjustment-registration-schedule-status/v3";
const SLOT_VERSION: &str = "adjustment-shadow-registration-slot/v3";

/// Closed controller invocation mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueMode {
    Daily,
    Monthly,
}

impl NativeMaintenanceDueMode {
    /// Return the exact controller mode spelling.
    pub const fn as_str(self) -> &'static str {
        // retain only the two installed modes
        match self {
            Self::Daily => "daily",
            Self::Monthly => "monthly",
        }
    }
}

/// One immutable due projection with its original retry cutoff and lease scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDue {
    pub due_key: String,
    pub family: Option<NativeMaintenanceCaptureFamily>,
    pub mode: NativeMaintenanceDueMode,
    pub original_cutoff_at: String,
    pub scope: String,
}

/// Journal-owned state of one caller-derived due key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueStatus {
    Absent,
    Registered,
    Complete,
}

/// Value-free immutable due registration and completion projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDueState {
    pub due_key: String,
    pub input_head_sha256: Option<String>,
    pub output_sha256: Option<String>,
    pub status: NativeMaintenanceDueStatus,
}

/// Future for one bounded due-state snapshot.
pub type NativeMaintenanceDueReadFuture<'a, E> =
    Pin<Box<dyn Future<Output = Result<Vec<NativeMaintenanceDueState>, E>> + 'a>>;

/// Closed bounded journal inspection used by the pure planner.
pub trait NativeMaintenanceDueReadPort {
    type Error;

    /// Inspect the exact unique input keys while preserving their order.
    fn inspect_due_keys<'a>(
        &'a mut self,
        due_keys: &'a [String],
    ) -> NativeMaintenanceDueReadFuture<'a, Self::Error>;
}

/// Actual verified-journal implementation of bounded due inspection.
pub struct NativeMaintenanceJournalDueReader<'a> {
    store: &'a mut NativeMaintenanceJournalStore,
}

impl<'a> NativeMaintenanceJournalDueReader<'a> {
    /// Borrow the initialized controller journal without reading it.
    pub const fn new(store: &'a mut NativeMaintenanceJournalStore) -> Self {
        Self { store }
    }

    /// Project source due state from one verified journal snapshot.
    fn inspect(
        &mut self,
        due_keys: &[String],
    ) -> Result<Vec<NativeMaintenanceDueState>, NativeMaintenanceDueJournalError> {
        use NativeMaintenanceDueJournalError::Invalid;

        // require one bounded unique admitted query before touching the store
        if due_keys.len() > MAXIMUM_DUE_KEYS
            || due_keys.iter().any(|key| !valid_due_key(key))
            || due_keys.iter().collect::<BTreeSet<_>>().len() != due_keys.len()
        {
            return Err(Invalid);
        }
        let records = self.store.read_verified(
            &mut NativeWirePrimitives::new(),
            &NativeWirePrimitives::new(),
        )?;
        due_keys
            .iter()
            .map(|key| due_state(&records, key))
            .collect()
    }
}

impl NativeMaintenanceDueReadPort for NativeMaintenanceJournalDueReader<'_> {
    type Error = NativeMaintenanceDueJournalError;

    /// Read the complete verified chain and preserve query order.
    fn inspect_due_keys<'a>(
        &'a mut self,
        due_keys: &'a [String],
    ) -> NativeMaintenanceDueReadFuture<'a, Self::Error> {
        Box::pin(ready(self.inspect(due_keys)))
    }
}

/// Actual due-reader durable-store and semantic failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDueJournalError {
    Store(NativeMaintenanceJournalStoreError),
    Invalid,
}

impl Display for NativeMaintenanceDueJournalError {
    /// Render one bounded journal inspection diagnostic.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve the concrete store cause independently from state rejection
        match self {
            Self::Store(error) => Display::fmt(error, formatter),
            Self::Invalid => formatter.write_str("maintenance due journal state is invalid"),
        }
    }
}

impl std::error::Error for NativeMaintenanceDueJournalError {
    /// Preserve only an actual durable-store cause.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // semantic state rejection has no lower-level source
        match self {
            Self::Store(error) => Some(error),
            Self::Invalid => None,
        }
    }
}

impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceDueJournalError {
    /// Retain verified-store failure ownership.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Store(error)
    }
}

/// Planner refusal from schedule/calendar binding or the read port.
#[derive(Debug)]
pub enum NativeMaintenanceDuePlannerError<Journal> {
    Journal(Journal),
    Invalid,
    Calendar,
}

impl<Journal: Display> Display for NativeMaintenanceDuePlannerError<Journal> {
    /// Render the first exact owner or binding failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // preserve journal diagnostics without exposing invalid schedule content
        match self {
            Self::Journal(error) => Display::fmt(error, formatter),
            Self::Invalid => formatter.write_str("maintenance due planner input is invalid"),
            Self::Calendar => formatter.write_str("maintenance due planner calendar failed"),
        }
    }
}

/// Derive bounded oldest-first due work from immutable journal state.
pub async fn plan_native_adjustment_maintenance_dues<Journal>(
    journal: &mut Journal,
    mode: NativeMaintenanceDueMode,
    now: &str,
    schedule: &NativeRegistrationScheduleStatus,
) -> Result<Vec<NativeMaintenanceDue>, NativeMaintenanceDuePlannerError<Journal::Error>>
where
    Journal: NativeMaintenanceDueReadPort,
{
    use NativeMaintenanceDuePlannerError as Error;

    let wire = NativeWirePrimitives::new();
    let calendar = NativePredecessorModelCalendar::new().ok_or(Error::Calendar)?;
    let now_milliseconds = canonical_instant(&wire, now).ok_or(Error::Invalid)?;
    validate_schedule(schedule, &wire, &calendar)?;
    // bind planning to the authenticated database transaction clock
    if now != schedule.snapshot_at {
        return Err(Error::Invalid);
    }
    let current_local_date = local_date_at(&calendar, now).ok_or(Error::Calendar)?;
    let current_month = current_local_date[..7].to_owned();
    let epoch_local_date = local_date_at(&calendar, &schedule.epoch_at).ok_or(Error::Calendar)?;
    let first_local_date = add_local_dates(&epoch_local_date, 1).ok_or(Error::Calendar)?;
    let horizon_end_exclusive_local_date =
        local_date_at(&calendar, &schedule.horizon_end_at).ok_or(Error::Calendar)?;
    let mut candidates = Vec::new();

    // daily mode includes bounded dates, rain reference, and old family months
    if mode == NativeMaintenanceDueMode::Daily {
        let yesterday = add_local_dates(&current_local_date, -1).ok_or(Error::Calendar)?;
        let horizon_last =
            add_local_dates(&horizon_end_exclusive_local_date, -1).ok_or(Error::Calendar)?;
        let final_date = yesterday.min(horizon_last);
        let daily_keys = local_dates_between(&first_local_date, &final_date)
            .ok_or(Error::Calendar)?
            .into_iter()
            .map(|date| format!("daily/{date}"))
            .collect::<Vec<_>>();
        let daily_state = inspect_exact(journal, &daily_keys).await?;
        // retain only the oldest seven unfinished dates
        for state in daily_state {
            if state.status == NativeMaintenanceDueStatus::Complete {
                continue;
            }
            candidates.push(due_from_key(&state.due_key, &wire, &calendar)?);
            // stop at the fixed daily catch-up ceiling
            if candidates.len() == DAILY_LIMIT {
                break;
            }
        }

        let next_control_month = next_utc_month(now).ok_or(Error::Calendar)?;
        let control_month_start =
            canonical_instant(&wire, &format!("{next_control_month}-01T00:00:00.000Z"))
                .ok_or(Error::Calendar)?;
        let control_window_start = control_month_start
            .checked_sub(7 * DAY_MILLISECONDS)
            .ok_or(Error::Calendar)?;
        let horizon = canonical_instant(&wire, &schedule.horizon_end_at).ok_or(Error::Invalid)?;
        // register the next rain reference only inside its authenticated pre-month window
        if now_milliseconds >= control_window_start
            && now_milliseconds < control_month_start
            && control_month_start < horizon
        {
            let control_key = format!("control-reference/rain/{next_control_month}");
            let state = inspect_exact(journal, std::slice::from_ref(&control_key)).await?;
            let control = state.first().ok_or(Error::Invalid)?;
            // never replay a published immutable reference
            if control.status != NativeMaintenanceDueStatus::Complete {
                candidates.push(due_from_key(&control_key, &wire, &calendar)?);
            }
        }

        let previous_current = previous_month(&current_month).ok_or(Error::Calendar)?;
        let horizon_month = &horizon_end_exclusive_local_date[..7];
        let previous_horizon = previous_month(horizon_month).ok_or(Error::Calendar)?;
        let final_month = previous_current.min(previous_horizon);
        let old_months =
            months_between(&first_local_date[..7], &final_month).ok_or(Error::Calendar)?;
        // advance at most one oldest incomplete month for each family
        for family in families() {
            let keys = old_months
                .iter()
                .map(|month| format!("monthly/{}/{month}", family_name(family)))
                .collect::<Vec<_>>();
            let states = inspect_exact(journal, &keys).await?;
            // append no synthetic work when this family backlog is clear
            if let Some(unfinished) = states
                .into_iter()
                .find(|state| state.status != NativeMaintenanceDueStatus::Complete)
            {
                candidates.push(due_from_key(&unfinished.due_key, &wire, &calendar)?);
            }
        }
        return Ok(candidates);
    }

    let local_day = current_local_date[8..10]
        .parse::<u8>()
        .map_err(|_| Error::Calendar)?;
    // wait for the fixed second-day monthly boundary inside the authenticated horizon
    if local_day < 2
        || current_month.as_str() < &first_local_date[..7]
        || current_local_date >= horizon_end_exclusive_local_date
    {
        return Ok(Vec::new());
    }
    let monthly_keys = families()
        .into_iter()
        .map(|family| format!("monthly/{}/{current_month}", family_name(family)))
        .collect::<Vec<_>>();
    let states = inspect_exact(journal, &monthly_keys).await?;
    states
        .into_iter()
        .filter(|state| state.status != NativeMaintenanceDueStatus::Complete)
        .map(|state| due_from_key(&state.due_key, &wire, &calendar))
        .collect()
}

/// Require the read port to preserve every requested due key and its order.
async fn inspect_exact<Journal>(
    journal: &mut Journal,
    keys: &[String],
) -> Result<Vec<NativeMaintenanceDueState>, NativeMaintenanceDuePlannerError<Journal::Error>>
where
    Journal: NativeMaintenanceDueReadPort,
{
    use NativeMaintenanceDuePlannerError as Error;

    let states = journal
        .inspect_due_keys(keys)
        .await
        .map_err(Error::Journal)?;
    // reject missing, reordered, or substituted journal projections
    if states.len() != keys.len()
        || states
            .iter()
            .zip(keys)
            .any(|(state, key)| state.due_key != *key)
    {
        return Err(Error::Invalid);
    }
    Ok(states)
}

/// Validate the planner-visible schedule and its three fixed family slots.
fn validate_schedule<Journal>(
    schedule: &NativeRegistrationScheduleStatus,
    wire: &NativeWirePrimitives,
    calendar: &NativePredecessorModelCalendar,
) -> Result<(), NativeMaintenanceDuePlannerError<Journal>> {
    use NativeMaintenanceDuePlannerError::Invalid;

    let epoch = canonical_instant(wire, &schedule.epoch_at).ok_or(Invalid)?;
    let horizon = canonical_instant(wire, &schedule.horizon_end_at).ok_or(Invalid)?;
    let snapshot = canonical_instant(wire, &schedule.snapshot_at).ok_or(Invalid)?;
    local_date_at(calendar, &schedule.epoch_at).ok_or(Invalid)?;
    local_date_at(calendar, &schedule.horizon_end_at).ok_or(Invalid)?;
    // retain the exact rolling contract and fixed future horizon
    if schedule.contract_version != SCHEDULE_VERSION
        || schedule.schedule_contract_sha256 != ADJUSTMENT_ROLLING_SCHEDULE_SHA256
        || !valid_hash(&schedule.epoch_witness_sha256)
        || snapshot < epoch
        || horizon <= epoch
    {
        return Err(Invalid);
    }
    let expected = families();
    // bind every slot to the same schedule head in fixed family order
    for (slot, family) in schedule.slots.iter().zip(expected) {
        let occupied = slot.state != NativeRegistrationSlotState::Free;
        if slot.contract_version != SLOT_VERSION
            || slot.family != family
            || slot.epoch_witness_sha256 != schedule.epoch_witness_sha256
            || slot.horizon_end_at != schedule.horizon_end_at
            || slot.schedule_contract_sha256 != schedule.schedule_contract_sha256
            || occupied != slot.registration_sha256.is_some()
            || occupied != slot.terminal_at.is_some()
            || slot
                .registration_sha256
                .as_deref()
                .is_some_and(|value| !valid_hash(value))
            || slot
                .terminal_at
                .as_deref()
                .is_some_and(|value| canonical_instant(wire, value).is_none())
        {
            return Err(Invalid);
        }
    }
    Ok(())
}

/// Revalidate one exact original due projection before any journal mutation.
pub fn verify_native_maintenance_due(due: &NativeMaintenanceDue) -> bool {
    // preserve the same calendar and cutoff projection used by the source planner
    let Some(calendar) = NativePredecessorModelCalendar::new() else {
        return false;
    };
    due_from_key::<()>(&due.due_key, &NativeWirePrimitives, &calendar)
        .is_ok_and(|expected| expected == *due)
}

/// Project one closed due key into its immutable retry cutoff and scope.
fn due_from_key<Journal>(
    due_key: &str,
    wire: &NativeWirePrimitives,
    calendar: &NativePredecessorModelCalendar,
) -> Result<NativeMaintenanceDue, NativeMaintenanceDuePlannerError<Journal>> {
    use NativeMaintenanceDuePlannerError::{Calendar, Invalid};

    if !valid_due_key(due_key) {
        return Err(Invalid);
    }
    // project one daily cutoff at the next local midnight
    if let Some(local_date) = due_key.strip_prefix("daily/") {
        let next = add_local_dates(local_date, 1).ok_or(Calendar)?;
        return Ok(NativeMaintenanceDue {
            due_key: due_key.to_owned(),
            family: None,
            mode: NativeMaintenanceDueMode::Daily,
            original_cutoff_at: local_midnight(wire, calendar, &next).ok_or(Calendar)?,
            scope: "daily".to_owned(),
        });
    }
    // project one rain reference at the fixed seven-day UTC pre-month boundary
    if let Some(month) = due_key.strip_prefix("control-reference/rain/") {
        let month_start =
            canonical_instant(wire, &format!("{month}-01T00:00:00.000Z")).ok_or(Calendar)?;
        let cutoff = month_start
            .checked_sub(7 * DAY_MILLISECONDS)
            .and_then(|value| wire.format_utc_milliseconds(value))
            .ok_or(Calendar)?;
        return Ok(NativeMaintenanceDue {
            due_key: due_key.to_owned(),
            family: Some(NativeMaintenanceCaptureFamily::Rain),
            mode: NativeMaintenanceDueMode::Monthly,
            original_cutoff_at: cutoff,
            scope: "control-reference/rain".to_owned(),
        });
    }
    let parts = due_key.split('/').collect::<Vec<_>>();
    let family = parse_family(parts.get(1).copied().ok_or(Invalid)?).ok_or(Invalid)?;
    let month = parts.get(2).copied().ok_or(Invalid)?;
    Ok(NativeMaintenanceDue {
        due_key: due_key.to_owned(),
        family: Some(family),
        mode: NativeMaintenanceDueMode::Monthly,
        original_cutoff_at: local_midnight(wire, calendar, &format!("{month}-01"))
            .ok_or(Calendar)?,
        scope: format!("monthly/{}", family_name(family)),
    })
}

/// Project immutable due registration and completion hashes from verified records.
fn due_state(
    records: &[NativeMaintenanceJournalRecord],
    key: &str,
) -> Result<NativeMaintenanceDueState, NativeMaintenanceDueJournalError> {
    use NativeMaintenanceDueJournalError::Invalid;

    let registration = records
        .iter()
        .find(|record| is_kind(record, "due_registered") && record.due_key.as_deref() == Some(key));
    let completion = records
        .iter()
        .find(|record| is_kind(record, "due_completed") && record.due_key.as_deref() == Some(key));
    let input_head_sha256 = registration
        .map(|record| text(&record.payload, "inputHeadSha256").ok_or(Invalid))
        .transpose()?;
    let output_sha256 = completion
        .map(|record| text(&record.payload, "outputSha256").ok_or(Invalid))
        .transpose()?;
    // reject malformed identities before exposing journal state
    if input_head_sha256
        .as_deref()
        .is_some_and(|hash| !valid_hash(hash))
        || output_sha256
            .as_deref()
            .is_some_and(|hash| !valid_hash(hash))
    {
        return Err(Invalid);
    }
    let status = if completion.is_some() {
        NativeMaintenanceDueStatus::Complete
    } else if registration.is_some() {
        NativeMaintenanceDueStatus::Registered
    } else {
        NativeMaintenanceDueStatus::Absent
    };
    Ok(NativeMaintenanceDueState {
        due_key: key.to_owned(),
        input_head_sha256,
        output_sha256,
        status,
    })
}

/// Enumerate inclusive local calendar dates.
fn local_dates_between(start: &str, end: &str) -> Option<Vec<String>> {
    // return no dates for a reversed interval
    if end < start {
        return Some(Vec::new());
    }
    let mut dates = Vec::new();
    let mut current = start.to_owned();
    // retain every exact calendar label once
    while current.as_str() <= end {
        dates.push(current.clone());
        current = add_local_dates(&current, 1)?;
    }
    Some(dates)
}

/// Enumerate inclusive calendar months.
fn months_between(start: &str, end: &str) -> Option<Vec<String>> {
    // return no months for a reversed interval
    if end < start {
        return Some(Vec::new());
    }
    let mut months = Vec::new();
    let mut current = start.to_owned();
    // retain every exact calendar month once
    while current.as_str() <= end {
        months.push(current.clone());
        current = add_months(&current, 1)?;
    }
    Some(months)
}

/// Add exact Gregorian calendar days to one local date label.
fn add_local_dates(value: &str, days: i64) -> Option<String> {
    let date = value.parse::<Date>().ok()?;
    let span = Span::new().try_days(days).ok()?;
    date.checked_add(span).ok().map(|date| date.to_string())
}

/// Add exact Gregorian calendar months to one month label.
fn add_months(value: &str, months: i64) -> Option<String> {
    let date = format!("{value}-01").parse::<Date>().ok()?;
    let span = Span::new().try_months(months).ok()?;
    let date = date.checked_add(span).ok()?;
    Some(format!("{:04}-{:02}", date.year(), date.month()))
}

/// Return the preceding Gregorian month.
fn previous_month(value: &str) -> Option<String> {
    add_months(value, -1)
}

/// Return the UTC calendar month following one canonical instant.
fn next_utc_month(value: &str) -> Option<String> {
    add_months(value.get(..7)?, 1)
}

/// Resolve one exact instant into the predecessor LA local date.
fn local_date_at(calendar: &NativePredecessorModelCalendar, value: &str) -> Option<String> {
    calendar
        .features(value)
        .ok()
        .map(|features| features.local_date)
}

/// Resolve LA midnight through the source's three offset-correction iterations.
fn local_midnight(
    wire: &NativeWirePrimitives,
    calendar: &NativePredecessorModelCalendar,
    local_date: &str,
) -> Option<String> {
    let target = canonical_instant(wire, &format!("{local_date}T00:00:00.000Z"))?;
    let mut guess = canonical_instant(wire, &format!("{local_date}T08:00:00.000Z"))?;
    // reproduce the predecessor's bounded wall-clock correction
    for _ in 0..3 {
        let guess_at = wire.format_utc_milliseconds(guess)?;
        let offset = calendar.features(&guess_at).ok()?.offset_seconds;
        let observed = guess.checked_add(i64::from(offset) * 1_000)?;
        guess = guess.checked_add(target.checked_sub(observed)?)?;
    }
    wire.format_utc_milliseconds(guess)
}

/// Resolve one canonical source instant without accepting aliases.
fn canonical_instant(wire: &NativeWirePrimitives, value: &str) -> Option<i64> {
    let resolved = wire
        .resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .ok()?;
    (resolved.utc == value).then_some(resolved.epoch_milliseconds)
}

/// Admit one closed source due key and its Gregorian suffix.
fn valid_due_key(value: &str) -> bool {
    // select only the three source due classes
    if let Some(date) = value.strip_prefix("daily/") {
        return digit_shape(date, b"dddd-dd-dd");
    }
    if let Some(month) = value.strip_prefix("control-reference/rain/") {
        return valid_month(month);
    }
    let parts = value.split('/').collect::<Vec<_>>();
    parts.len() == 3
        && parts[0] == "monthly"
        && parse_family(parts[1]).is_some()
        && valid_month(parts[2])
}

/// Admit one exact four-digit Gregorian month label.
fn valid_month(value: &str) -> bool {
    digit_shape(value, b"dddd-dd")
}

/// Match one fixed ASCII digit-and-separator grammar.
fn digit_shape(value: &str, shape: &[u8]) -> bool {
    value.len() == shape.len()
        && value.bytes().zip(shape).all(|(observed, expected)| {
            // require a decimal digit at each variable position
            if *expected == b'd' {
                observed.is_ascii_digit()
            } else {
                observed == *expected
            }
        })
}

/// Return the fixed family order used by planning and lifecycle schedules.
const fn families() -> [NativeMaintenanceCaptureFamily; 3] {
    [
        NativeMaintenanceCaptureFamily::Temperature,
        NativeMaintenanceCaptureFamily::Wind,
        NativeMaintenanceCaptureFamily::Rain,
    ]
}

/// Parse one fixed family spelling.
fn parse_family(value: &str) -> Option<NativeMaintenanceCaptureFamily> {
    // reject every cross-family alias
    match value {
        "temperature" => Some(NativeMaintenanceCaptureFamily::Temperature),
        "wind" => Some(NativeMaintenanceCaptureFamily::Wind),
        "rain" => Some(NativeMaintenanceCaptureFamily::Rain),
        _ => None,
    }
}

/// Return one fixed family spelling.
const fn family_name(family: NativeMaintenanceCaptureFamily) -> &'static str {
    // preserve the database family names
    match family {
        NativeMaintenanceCaptureFamily::Temperature => "temperature",
        NativeMaintenanceCaptureFamily::Wind => "wind",
        NativeMaintenanceCaptureFamily::Rain => "rain",
    }
}

/// Compare one verified journal event discriminator.
fn is_kind(record: &NativeMaintenanceJournalRecord, expected: &str) -> bool {
    matches!(&record.kind, JsonValue::String(units) if units.iter().copied().eq(expected.encode_utf16()))
}

#[cfg(test)]
#[path = "native_maintenance_due_planner_test.rs"]
mod tests;
