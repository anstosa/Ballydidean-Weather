use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::{Arc, Mutex};

use crate::{XweatherTileCacheGetError, XweatherTileCacheRequest, XweatherTileLoader};

const JAVASCRIPT_MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// one admitted positive provider map-unit count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XweatherMapUnits(u64);

impl XweatherMapUnits {
    /// validate one exact positive source unit count.
    pub fn admitted(units: u64) -> Result<Self, XweatherMapUnitsError> {
        // preserve the source's positive safe-integer boundary
        if !(1..=JAVASCRIPT_MAXIMUM_SAFE_INTEGER).contains(&units) {
            return Err(XweatherMapUnitsError);
        }

        Ok(Self(units))
    }

    /// return the admitted provider unit count.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// one invalid provider map-unit count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XweatherMapUnitsError;

impl Display for XweatherMapUnitsError {
    /// render the source budget's exact unit validation error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Xweather map units must be a positive integer")
    }
}

impl Error for XweatherMapUnitsError {}

/// one UTC calendar identity admitted by the owning time adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedXweatherUsageCalendar {
    day: String,
    month: String,
}

impl AdmittedXweatherUsageCalendar {
    /// admit canonical UTC day and month identities without a time codec here.
    pub fn admitted(day: impl Into<String>, month: impl Into<String>) -> Self {
        Self {
            day: day.into(),
            month: month.into(),
        }
    }

    /// borrow the admitted canonical UTC day.
    pub fn day(&self) -> &str {
        &self.day
    }

    /// borrow the admitted canonical UTC month.
    pub fn month(&self) -> &str {
        &self.month
    }
}

/// current calendar observation retained outside budget policy.
pub trait XweatherUsageCalendarSource {
    /// return the currently admitted UTC budget identities.
    fn current_calendar(&self) -> AdmittedXweatherUsageCalendar;
}

/// one decoded and validated persistent counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedXweatherUsageLedger {
    calendar: AdmittedXweatherUsageCalendar,
    day_units: u64,
    month_units: u64,
}

impl AdmittedXweatherUsageLedger {
    /// admit one canonical persisted counter after format validation.
    pub fn admitted(
        calendar: AdmittedXweatherUsageCalendar,
        day_units: u64,
        month_units: u64,
    ) -> Result<Self, XweatherUsageLedgerError> {
        // retain only source-safe nonnegative integer counters
        if day_units > JAVASCRIPT_MAXIMUM_SAFE_INTEGER
            || month_units > JAVASCRIPT_MAXIMUM_SAFE_INTEGER
        {
            return Err(XweatherUsageLedgerError);
        }

        Ok(Self {
            calendar,
            day_units,
            month_units,
        })
    }

    /// borrow the persisted calendar identity.
    pub fn calendar(&self) -> &AdmittedXweatherUsageCalendar {
        &self.calendar
    }

    /// return the persisted daily unit count.
    pub const fn day_units(&self) -> u64 {
        self.day_units
    }

    /// return the persisted monthly unit count.
    pub const fn month_units(&self) -> u64 {
        self.month_units
    }
}

/// one invalid canonical persistence adapter result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XweatherUsageLedgerError;

impl Display for XweatherUsageLedgerError {
    /// render the source budget's exact persisted-state error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid persisted Xweather usage budget")
    }
}

impl Error for XweatherUsageLedgerError {}

/// decoded ledger persistence retained outside file and JSON mechanics.
pub trait XweatherUsagePersistence {
    type Error;

    /// load one optional already validated persistent counter.
    fn load(&mut self) -> Result<Option<AdmittedXweatherUsageLedger>, Self::Error>;

    /// atomically persist one complete canonical counter.
    fn persist(&mut self, state: &AdmittedXweatherUsageLedger) -> Result<(), Self::Error>;
}

/// one invalid bounded budget configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XweatherUsageBudgetConfigurationError;

impl Display for XweatherUsageBudgetConfigurationError {
    /// render the source budget's exact configuration error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid Xweather usage budget configuration")
    }
}

impl Error for XweatherUsageBudgetConfigurationError {}

/// one construction failure before the budget becomes available.
#[derive(Debug)]
pub enum XweatherUsageBudgetCreateError<PersistenceError> {
    Configuration(XweatherUsageBudgetConfigurationError),
    Persistence(PersistenceError),
}

impl<PersistenceError: Display> Display for XweatherUsageBudgetCreateError<PersistenceError> {
    /// render the unchanged construction failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve configuration and persistence categories
        match self {
            Self::Configuration(error) => Display::fmt(error, formatter),
            Self::Persistence(error) => Display::fmt(error, formatter),
        }
    }
}

impl<PersistenceError> Error for XweatherUsageBudgetCreateError<PersistenceError>
where
    PersistenceError: Error + 'static,
{
    /// retain the exact underlying construction source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // expose only the owning configuration or persistence failure
        match self {
            Self::Configuration(error) => Some(error),
            Self::Persistence(error) => Some(error),
        }
    }
}

/// one reservation or calendar-roll failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XweatherUsageBudgetError<PersistenceError> {
    Exhausted,
    ReservationPersistence(PersistenceError),
    WindowPersistence(PersistenceError),
}

impl<PersistenceError> XweatherUsageBudgetError<PersistenceError> {
    /// return the source error code when the original path assigns one.
    pub const fn code(&self) -> Option<&'static str> {
        // preserve exhaustion, reservation persistence, and untagged roll failures
        match self {
            Self::Exhausted => Some("xweather_budget_exhausted"),
            Self::ReservationPersistence(_) => Some("xweather_budget_unavailable"),
            Self::WindowPersistence(_) => None,
        }
    }

    /// report the only failure mapped to the public exhaustion response.
    pub const fn is_exhausted(&self) -> bool {
        matches!(self, Self::Exhausted)
    }
}

impl<PersistenceError: Display> Display for XweatherUsageBudgetError<PersistenceError> {
    /// render the source budget's exact failure text.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve exhaustion and persistence failure text
        match self {
            Self::Exhausted => formatter.write_str("Xweather map-unit budget exhausted"),
            Self::ReservationPersistence(_) | Self::WindowPersistence(_) => {
                formatter.write_str("Xweather usage budget could not be persisted")
            }
        }
    }
}

impl<PersistenceError> Error for XweatherUsageBudgetError<PersistenceError>
where
    PersistenceError: Error + 'static,
{
    /// retain persistence causes without inventing one for exhaustion.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // expose only injected persistence failures
        match self {
            Self::Exhausted => None,
            Self::ReservationPersistence(error) | Self::WindowPersistence(error) => Some(error),
        }
    }
}

/// one redacted source-compatible usage snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XweatherUsageSnapshot {
    pub day: String,
    pub daily_limit: u64,
    pub day_units: u64,
    pub month: String,
    pub monthly_limit: u64,
    pub month_units: u64,
}

#[derive(Debug)]
struct XweatherUsageBudgetInner<Persistence> {
    persistence: Persistence,
    state: AdmittedXweatherUsageLedger,
}

/// one persistent calendar budget over admitted time and persistence adapters.
#[derive(Debug)]
pub struct XweatherUsageBudget<Calendar, Persistence> {
    calendar: Calendar,
    daily_limit: u64,
    inner: Mutex<XweatherUsageBudgetInner<Persistence>>,
    monthly_limit: u64,
}

impl<Calendar, Persistence> XweatherUsageBudget<Calendar, Persistence>
where
    Calendar: XweatherUsageCalendarSource,
    Persistence: XweatherUsagePersistence,
{
    /// configure and load one bounded calendar budget.
    pub fn new(
        daily_limit: u64,
        monthly_limit: u64,
        calendar: Calendar,
        mut persistence: Persistence,
    ) -> Result<Self, XweatherUsageBudgetCreateError<Persistence::Error>> {
        // reject the source budget's unsafe numeric configuration
        if !(1..=JAVASCRIPT_MAXIMUM_SAFE_INTEGER).contains(&daily_limit)
            || !(daily_limit..=JAVASCRIPT_MAXIMUM_SAFE_INTEGER).contains(&monthly_limit)
        {
            return Err(XweatherUsageBudgetCreateError::Configuration(
                XweatherUsageBudgetConfigurationError,
            ));
        }
        let current = calendar.current_calendar();
        let state = persistence
            .load()
            .map_err(XweatherUsageBudgetCreateError::Persistence)?
            .unwrap_or(AdmittedXweatherUsageLedger {
                calendar: current,
                day_units: 0,
                month_units: 0,
            });
        Ok(Self {
            calendar,
            daily_limit,
            inner: Mutex::new(XweatherUsageBudgetInner { persistence, state }),
            monthly_limit,
        })
    }

    /// reserve units before one billable provider request.
    pub fn reserve(
        &self,
        units: XweatherMapUnits,
    ) -> Result<(), XweatherUsageBudgetError<Persistence::Error>> {
        let current = self.calendar.current_calendar();
        let mut inner = self
            .inner
            .lock()
            .expect("Xweather usage budget lock poisoned");
        Self::roll_calendar_windows(&mut inner, current)?;
        let units = units.get();
        let next_day_units = inner.state.day_units + units;
        let next_month_units = inner.state.month_units + units;

        // stop before crossing either provider budget
        if next_day_units > self.daily_limit || next_month_units > self.monthly_limit {
            return Err(XweatherUsageBudgetError::Exhausted);
        }
        let next = AdmittedXweatherUsageLedger {
            calendar: inner.state.calendar.clone(),
            day_units: next_day_units,
            month_units: next_month_units,
        };
        inner
            .persistence
            .persist(&next)
            .map_err(XweatherUsageBudgetError::ReservationPersistence)?;
        inner.state = next;
        Ok(())
    }

    /// expose one redacted usage snapshot after calendar rollover.
    pub fn snapshot(
        &self,
    ) -> Result<XweatherUsageSnapshot, XweatherUsageBudgetError<Persistence::Error>> {
        let current = self.calendar.current_calendar();
        let mut inner = self
            .inner
            .lock()
            .expect("Xweather usage budget lock poisoned");
        Self::roll_calendar_windows(&mut inner, current)?;
        Ok(XweatherUsageSnapshot {
            day: inner.state.calendar.day.clone(),
            daily_limit: self.daily_limit,
            day_units: inner.state.day_units,
            month: inner.state.calendar.month.clone(),
            monthly_limit: self.monthly_limit,
            month_units: inner.state.month_units,
        })
    }

    /// reset and persist elapsed UTC calendar windows.
    fn roll_calendar_windows(
        inner: &mut XweatherUsageBudgetInner<Persistence>,
        current: AdmittedXweatherUsageCalendar,
    ) -> Result<(), XweatherUsageBudgetError<Persistence::Error>> {
        let next = if current.month != inner.state.calendar.month {
            Some(AdmittedXweatherUsageLedger {
                calendar: current,
                day_units: 0,
                month_units: 0,
            })
        } else if current.day != inner.state.calendar.day {
            Some(AdmittedXweatherUsageLedger {
                calendar: AdmittedXweatherUsageCalendar {
                    day: current.day,
                    month: inner.state.calendar.month.clone(),
                },
                day_units: 0,
                month_units: inner.state.month_units,
            })
        } else {
            None
        };

        // persist only one changed calendar boundary
        if let Some(next) = next {
            inner
                .persistence
                .persist(&next)
                .map_err(XweatherUsageBudgetError::WindowPersistence)?;
            inner.state = next;
        }
        Ok(())
    }
}

/// reserve one budget before delegating an actual cache miss to its provider.
#[derive(Debug)]
pub struct XweatherBudgetedTileLoader<Calendar, Persistence, Loader> {
    budget: Arc<XweatherUsageBudget<Calendar, Persistence>>,
    loader: Loader,
}

impl<Calendar, Persistence, Loader> XweatherBudgetedTileLoader<Calendar, Persistence, Loader> {
    /// pair one shared budget with the existing provider loader seam.
    pub fn new(budget: Arc<XweatherUsageBudget<Calendar, Persistence>>, loader: Loader) -> Self {
        Self { budget, loader }
    }
}

/// one budget admission or provider failure for a real cache miss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XweatherBudgetedTileLoadError<PersistenceError, LoaderError> {
    Budget(XweatherUsageBudgetError<PersistenceError>),
    Loader(LoaderError),
}

impl<PersistenceError, LoaderError> XweatherBudgetedTileLoadError<PersistenceError, LoaderError> {
    /// return the source budget code without assigning one to provider failures.
    pub const fn code(&self) -> Option<&'static str> {
        // preserve only codes assigned by the underlying budget path
        match self {
            Self::Budget(error) => error.code(),
            Self::Loader(_) => None,
        }
    }

    /// report whether the public route must select its exhaustion response.
    pub const fn is_budget_exhausted(&self) -> bool {
        matches!(self, Self::Budget(error) if error.is_exhausted())
    }
}

impl<PersistenceError: Display, LoaderError: Display> Display
    for XweatherBudgetedTileLoadError<PersistenceError, LoaderError>
{
    /// render the unchanged budget or provider failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the owning failure category
        match self {
            Self::Budget(error) => Display::fmt(error, formatter),
            Self::Loader(error) => Display::fmt(error, formatter),
        }
    }
}

impl<PersistenceError, LoaderError> Error
    for XweatherBudgetedTileLoadError<PersistenceError, LoaderError>
where
    PersistenceError: Error + 'static,
    LoaderError: Error + 'static,
{
    /// retain the exact budget or provider source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // expose only the owning failure
        match self {
            Self::Budget(error) => Some(error),
            Self::Loader(error) => Some(error),
        }
    }
}

impl<Calendar, Persistence, Loader> XweatherTileLoader
    for XweatherBudgetedTileLoader<Calendar, Persistence, Loader>
where
    Calendar: XweatherUsageCalendarSource,
    Persistence: XweatherUsagePersistence,
    Persistence::Error: Clone,
    Loader: XweatherTileLoader,
{
    type Error = XweatherBudgetedTileLoadError<Persistence::Error, Loader::Error>;

    /// reserve exactly once before delegating one cache miss or retry.
    fn load_tile(&self, request: &XweatherTileCacheRequest) -> Result<Vec<u8>, Self::Error> {
        self.budget
            .reserve(request.provider_units)
            .map_err(XweatherBudgetedTileLoadError::Budget)?;
        self.loader
            .load_tile(request)
            .map_err(XweatherBudgetedTileLoadError::Loader)
    }
}

/// one admitted source-compatible public retry delay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedXweatherBudgetRetryAfter(String);

impl AdmittedXweatherBudgetRetryAfter {
    /// admit the owning UTC-day boundary projection without a time codec here.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// borrow the admitted Retry-After value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// the exact public response selected after one budgeted tile load failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XweatherBudgetedTileFailureResponse {
    BudgetExhausted {
        retry_after: AdmittedXweatherBudgetRetryAfter,
    },
    Unavailable,
}

impl XweatherBudgetedTileFailureResponse {
    /// return the source route's exact failure status.
    pub const fn status_code(&self) -> u16 {
        // preserve exhaustion and ordinary upstream failure categories
        match self {
            Self::BudgetExhausted { .. } => 429,
            Self::Unavailable => 502,
        }
    }

    /// return the source route's exact failure body.
    pub const fn body(&self) -> &'static str {
        // preserve exhaustion and ordinary upstream response text
        match self {
            Self::BudgetExhausted { .. } => "weather map budget exhausted\n",
            Self::Unavailable => "weather tile unavailable\n",
        }
    }

    /// return the source route's only failure-specific header.
    pub fn retry_after_header(&self) -> Option<(&'static str, &str)> {
        // attach retry guidance only to budget exhaustion
        match self {
            Self::BudgetExhausted { retry_after } => Some(("Retry-After", retry_after.as_str())),
            Self::Unavailable => None,
        }
    }
}

/// map one budgeted cache failure into the source route decision.
pub fn xweather_budgeted_tile_failure_response<PersistenceError, LoaderError>(
    error: &XweatherTileCacheGetError<XweatherBudgetedTileLoadError<PersistenceError, LoaderError>>,
    retry_after: AdmittedXweatherBudgetRetryAfter,
) -> XweatherBudgetedTileFailureResponse {
    // select 429 only for the source's coded budget exhaustion
    if matches!(error, XweatherTileCacheGetError::Loader(error) if error.is_budget_exhausted()) {
        return XweatherBudgetedTileFailureResponse::BudgetExhausted { retry_after };
    }

    XweatherBudgetedTileFailureResponse::Unavailable
}
