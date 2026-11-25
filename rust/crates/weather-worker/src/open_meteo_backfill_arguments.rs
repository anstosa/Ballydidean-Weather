//! Own the shared original Open-Meteo archive and Previous Runs command arguments.

use std::fmt;

use weather_contracts::{BackfillArguments, Nullable};

use crate::historical_backfill_command::{date_label_is_valid, javascript_number, javascript_trim};
use crate::{
    FORECAST_ANCHOR_MAXIMUM_CHUNK_DAYS, ForecastAnchorBackfillDate,
    HistoricalBackfillCommandTimeAdapter, TaskFailure,
};

const UTC_DAY_MILLISECONDS: i64 = 86_400_000;
const VALUE_OPTIONS: [&str; 6] = [
    "--site",
    "--from",
    "--to",
    "--source",
    "--chunk-days",
    "--report",
];

/// The original generated argument payload and its two admitted UTC dates.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedOpenMeteoBackfillArguments {
    arguments: BackfillArguments,
    from: ForecastAnchorBackfillDate,
    to: ForecastAnchorBackfillDate,
}

impl ParsedOpenMeteoBackfillArguments {
    /// Move generated arguments and validated date boundaries into a selected backend.
    pub fn into_parts(
        self,
    ) -> (
        BackfillArguments,
        ForecastAnchorBackfillDate,
        ForecastAnchorBackfillDate,
    ) {
        (self.arguments, self.from, self.to)
    }
}

/// One original argument refusal before opening configuration or a process pool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenMeteoBackfillArgumentError {
    UnsupportedArgument(String),
    RequiresValue(&'static str),
    RequiredArgument(&'static str),
    InvalidDate(&'static str),
    InvalidTimeValue,
    ReversedRange,
    NonPastRange,
    InvalidChunkDays,
    Clock(TaskFailure),
}

impl fmt::Display for OpenMeteoBackfillArgumentError {
    /// Render the existing command diagnostics before process-level redaction.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedArgument(value) => {
                write!(formatter, "unsupported backfill argument: {value}")
            }
            Self::RequiresValue(name) => write!(formatter, "{name} requires a value"),
            Self::RequiredArgument(name) => write!(formatter, "{name} is required"),
            Self::InvalidDate(name) => write!(formatter, "{name} must use a valid YYYY-MM-DD date"),
            Self::InvalidTimeValue => formatter.write_str("Invalid time value"),
            Self::ReversedRange => formatter.write_str("--from must not follow --to"),
            Self::NonPastRange => formatter.write_str("--to must be before today"),
            Self::InvalidChunkDays => formatter.write_str("--chunk-days must be between 1 and 14"),
            Self::Clock(failure) => formatter.write_str(&failure.code),
        }
    }
}

impl std::error::Error for OpenMeteoBackfillArgumentError {}

/// Parse exact original argv before source selection or any configuration read.
pub fn parse_open_meteo_backfill_arguments<Time: HistoricalBackfillCommandTimeAdapter>(
    arguments: &[String],
    time: &mut Time,
) -> Result<ParsedOpenMeteoBackfillArguments, OpenMeteoBackfillArgumentError> {
    use OpenMeteoBackfillArgumentError as Error;

    let mut values = [None; VALUE_OPTIONS.len()];
    let mut dry_run = false;
    let mut resume = false;
    let mut arguments = arguments.iter().map(String::as_str);

    // scan controls before validating any semantic fields
    while let Some(argument) = arguments.next() {
        match argument {
            "--dry-run" => dry_run = true,
            "--resume" => resume = true,
            _ => {
                let index = VALUE_OPTIONS
                    .iter()
                    .position(|option| *option == argument)
                    .ok_or_else(|| Error::UnsupportedArgument(argument.to_owned()))?;
                let value = arguments
                    .next()
                    .filter(|value| !value.starts_with("--"))
                    .ok_or(Error::RequiresValue(VALUE_OPTIONS[index]))?;
                values[index] = Some(value);
            }
        }
    }

    let [site, from_label, to_label, source, chunk_days, report_path] = values;
    let site = require_value(site, "--site")?;
    let from = admit_date(time, require_value(from_label, "--from")?, "--from")?;
    let to = admit_date(time, require_value(to_label, "--to")?, "--to")?;
    // preserve the original reversed-range diagnostic ahead of the clock comparison
    if from.label() > to.label() {
        return Err(Error::ReversedRange);
    }
    let today = time.today_utc().map_err(Error::Clock)?;
    // require a well-formed UTC date from the clock owner
    if !date_label_is_valid(today.label()) || today.start_ms() % UTC_DAY_MILLISECONDS != 0 {
        return Err(Error::InvalidTimeValue);
    }
    // exclude the partial current UTC day without choosing any site timezone
    if to.label() >= today.label() {
        return Err(Error::NonPastRange);
    }

    let max_chunk_days = FORECAST_ANCHOR_MAXIMUM_CHUNK_DAYS;
    let chunk_days = chunk_days
        .map_or(Some(f64::from(max_chunk_days)), javascript_number)
        .filter(|value| value.fract() == 0.0 && (1.0..=f64::from(max_chunk_days)).contains(value))
        .ok_or(Error::InvalidChunkDays)?;

    Ok(ParsedOpenMeteoBackfillArguments {
        arguments: BackfillArguments {
            chunkDays: chunk_days as u64,
            dryRun: dry_run,
            from: from.label().to_owned(),
            reportPath: report_path.map_or(Nullable::Null, |path| Nullable::Value(path.to_owned())),
            resume,
            site: site.to_owned(),
            source: source.map_or(Nullable::Null, |key| Nullable::Value(key.to_owned())),
            to: to.label().to_owned(),
        },
        from,
        to,
    })
}

/// Reject required blanks without altering any caller-supplied nonblank string.
fn require_value<'a>(
    value: Option<&'a str>,
    name: &'static str,
) -> Result<&'a str, OpenMeteoBackfillArgumentError> {
    value
        .filter(|value| !javascript_trim(value).is_empty())
        .ok_or(OpenMeteoBackfillArgumentError::RequiredArgument(name))
}

/// Keep date validation at the admitted calendar owner and retain the original CLI error groups.
fn admit_date<Time: HistoricalBackfillCommandTimeAdapter>(
    time: &mut Time,
    label: &str,
    name: &'static str,
) -> Result<ForecastAnchorBackfillDate, OpenMeteoBackfillArgumentError> {
    use OpenMeteoBackfillArgumentError as Error;

    // check only the original date-label grammar before passing to the time owner
    if !date_label_is_valid(label) {
        return Err(Error::InvalidDate(name));
    }
    // Date.toISOString throws on impossible month or day components instead of a rollover
    if !("01"..="12").contains(&&label[5..7]) || !("01"..="31").contains(&&label[8..10]) {
        return Err(Error::InvalidTimeValue);
    }
    let date = time
        .admit_utc_date(label)
        .map_err(|_| Error::InvalidDate(name))?;
    // reject a normalized label or a non-midnight interpretation
    if date.label() != label || date.start_ms() % UTC_DAY_MILLISECONDS != 0 {
        return Err(Error::InvalidDate(name));
    }
    Ok(ForecastAnchorBackfillDate::admitted(label, date.start_ms()))
}
