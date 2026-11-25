//! Typed operator command binding for retained historical backfill families.

use std::collections::{BTreeMap, BTreeSet};

use super::TaskFailure;
use super::public_station_backfill::{
    PublicStationBackfillArguments, PublicStationBackfillDate, PublicStationBackfillReport,
};
use super::tempest_backfill::{TempestBackfillArguments, TempestBulkBackfillReport};
use super::tide_backfill::{TideBackfillArguments, TideBackfillDate, TideBackfillReport};

const MILLISECONDS_PER_DAY: i64 = 86_400_000;
const JAVASCRIPT_MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const DEPLOYED_SITE: &str = "ballydidean";
const TIDE_DEFAULT_FROM: &str = "2019-01-01";

/// One retained operator command family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoricalBackfillCommandKind {
    Tempest,
    Tide,
    PublicStations,
}

/// Checked local configuration family required before opening provider or database ports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoricalBackfillConfigurationFamily {
    Tempest,
    Tide,
    PublicStations,
}

/// Explicit command-to-configuration relationship without nullable configuration authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoricalBackfillSourceConfiguration {
    Required(HistoricalBackfillConfigurationFamily),
    NotApplicable,
}

/// One UTC calendar date admitted by the command's owning time implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalBackfillCommandDate {
    label: String,
    start_ms: i64,
}

impl HistoricalBackfillCommandDate {
    /// Construct one date already admitted by an approved UTC calendar owner.
    pub fn admitted(label: impl Into<String>, start_ms: i64) -> Self {
        Self {
            label: label.into(),
            start_ms,
        }
    }

    /// Return the canonical date-only label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Return the exact UTC midnight epoch.
    pub const fn start_ms(&self) -> i64 {
        self.start_ms
    }
}

/// Approved UTC date operations used instead of implementing a calendar codec here.
pub trait HistoricalBackfillCommandTimeAdapter {
    /// Return the current UTC calendar date.
    fn today_utc(&mut self) -> Result<HistoricalBackfillCommandDate, TaskFailure>;

    /// Admit one caller-supplied date-only value.
    fn admit_utc_date(&mut self, label: &str)
    -> Result<HistoricalBackfillCommandDate, TaskFailure>;

    /// Apply one exact UTC calendar-day offset.
    fn add_utc_days(
        &mut self,
        date: &HistoricalBackfillCommandDate,
        days: i64,
    ) -> Result<HistoricalBackfillCommandDate, TaskFailure>;
}

/// Exact typed executor arguments selected by the operator command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoricalBackfillArguments {
    Tempest(TempestBackfillArguments),
    Tide(TideBackfillArguments),
    PublicStations(PublicStationBackfillArguments),
}

/// Fully parsed command inputs before configuration, provider, or database work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalBackfillCommand {
    pub arguments: HistoricalBackfillArguments,
    pub report_path: Option<String>,
    pub source_configuration: HistoricalBackfillSourceConfiguration,
}

/// Existing report families routed through one operator-command result boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum HistoricalBackfillReport {
    Tempest(TempestBulkBackfillReport),
    Tide(TideBackfillReport),
    PublicStations(PublicStationBackfillReport),
}

impl HistoricalBackfillReport {
    /// Return the existing exact zero-or-one process result without reinterpreting reports.
    pub const fn exit_code(&self) -> u8 {
        // preserve each executor's owning result field
        match self {
            Self::Tempest(report) => report.exit_code,
            Self::Tide(report) => report.exit_code,
            Self::PublicStations(report) => report.exit_code,
        }
    }
}

/// Closed command-admission failures before side effects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoricalBackfillCommandError {
    InvalidChunkDays,
    InvalidDate(String),
    InvalidRange,
    InvalidSourceKey,
    InvalidStationId,
    MissingRequired(String),
    MissingValue(String),
    Time(TaskFailure),
    UnsupportedArgument(String),
}

impl From<TaskFailure> for HistoricalBackfillCommandError {
    /// Preserve the approved time adapter's bounded failure.
    fn from(value: TaskFailure) -> Self {
        Self::Time(value)
    }
}

/// Return the exact deployed shell-wrapper arguments before CLI defaults are applied.
pub const fn deployed_historical_backfill_arguments(
    kind: HistoricalBackfillCommandKind,
) -> &'static [&'static str] {
    // preserve each installed wrapper's first operator invocation
    match kind {
        HistoricalBackfillCommandKind::Tempest => &["--site", DEPLOYED_SITE, "--resume"],
        HistoricalBackfillCommandKind::Tide => &[
            "--site",
            DEPLOYED_SITE,
            "--from",
            TIDE_DEFAULT_FROM,
            "--resume",
        ],
        HistoricalBackfillCommandKind::PublicStations => &["--site", DEPLOYED_SITE, "--resume"],
    }
}

/// Parse one retained CLI family into its existing typed executor arguments.
pub fn parse_historical_backfill_command<Time: HistoricalBackfillCommandTimeAdapter>(
    kind: HistoricalBackfillCommandKind,
    arguments: &[String],
    time: &mut Time,
) -> Result<HistoricalBackfillCommand, HistoricalBackfillCommandError> {
    let raw = parse_raw_arguments(kind, arguments)?;
    let today = admitted_today(time)?;
    let to = match raw.values.get("--to") {
        Some(label) => admitted_explicit_date(time, label, "--to")?,
        None => admitted_previous_day(time, &today)?,
    };

    // reserve the partial current UTC date for scheduled ingestion
    if to.start_ms >= today.start_ms {
        return Err(HistoricalBackfillCommandError::InvalidRange);
    }
    let site = require_site(kind, &raw.values)?;
    let report_path = raw.values.get("--report").cloned();
    let dry_run = raw.flags.contains("--dry-run");
    let resume = raw.flags.contains("--resume");
    let arguments = match kind {
        HistoricalBackfillCommandKind::Tempest => {
            let from = optional_date(time, raw.values.get("--from"), "--from")?;
            reject_reversed(from.as_ref(), &to)?;
            let chunk_days = tempest_chunk_days(raw.values.get("--chunk-days"))?;
            HistoricalBackfillArguments::Tempest(TempestBackfillArguments {
                chunk_days,
                dry_run,
                from: from.map(public_station_date),
                resume,
                site,
                station_ids: deduplicate(raw.station_ids),
                to: public_station_date(to),
            })
        }
        HistoricalBackfillCommandKind::Tide => {
            let from = match raw.values.get("--from") {
                Some(label) => admitted_explicit_date(time, label, "--from")?,
                None => admitted_explicit_date(time, TIDE_DEFAULT_FROM, "--from")?,
            };
            reject_reversed(Some(&from), &to)?;
            HistoricalBackfillArguments::Tide(TideBackfillArguments {
                dry_run,
                from: tide_date(from),
                resume,
                site,
                source_keys: deduplicate(raw.source_keys),
                to: tide_date(to),
            })
        }
        HistoricalBackfillCommandKind::PublicStations => {
            let from = optional_date(time, raw.values.get("--from"), "--from")?;
            reject_reversed(from.as_ref(), &to)?;
            HistoricalBackfillArguments::PublicStations(PublicStationBackfillArguments {
                dry_run,
                from: from.map(public_station_date),
                resume,
                site,
                source_keys: deduplicate(raw.source_keys),
                to: public_station_date(to),
            })
        }
    };
    Ok(HistoricalBackfillCommand {
        arguments,
        report_path,
        source_configuration: HistoricalBackfillSourceConfiguration::Required(
            configuration_family(kind),
        ),
    })
}

/// Apply one deployed shell wrapper and then the owning CLI's first defaults.
pub fn deployed_historical_backfill_command<Time: HistoricalBackfillCommandTimeAdapter>(
    kind: HistoricalBackfillCommandKind,
    time: &mut Time,
) -> Result<HistoricalBackfillCommand, HistoricalBackfillCommandError> {
    let arguments = deployed_historical_backfill_arguments(kind)
        .iter()
        .map(
            // retain each literal wrapper argument without shell reinterpretation
            |value| (*value).to_owned(),
        )
        .collect::<Vec<_>>();
    parse_historical_backfill_command(kind, &arguments, time)
}

#[derive(Debug, Default)]
struct RawHistoricalBackfillArguments {
    flags: BTreeSet<String>,
    source_keys: Vec<String>,
    station_ids: Vec<u64>,
    values: BTreeMap<String, String>,
}

// consume the documented option grammar while preserving last scalar and first selector order
fn parse_raw_arguments(
    kind: HistoricalBackfillCommandKind,
    arguments: &[String],
) -> Result<RawHistoricalBackfillArguments, HistoricalBackfillCommandError> {
    let mut raw = RawHistoricalBackfillArguments::default();
    let mut index = 0;

    // consume every documented argument exactly once
    while index < arguments.len() {
        let argument = &arguments[index];

        // retain duplicate boolean controls as one enabled flag
        if matches!(argument.as_str(), "--dry-run" | "--resume") {
            raw.flags.insert(argument.clone());
            index += 1;
            continue;
        }

        // reject controls outside this exact command family
        if !value_option_is_supported(kind, argument) {
            return Err(HistoricalBackfillCommandError::UnsupportedArgument(
                argument.clone(),
            ));
        }
        let value = arguments
            .get(index + 1)
            .ok_or_else(|| HistoricalBackfillCommandError::MissingValue(argument.clone()))?;

        // match Node's missing-value guard before any option-specific parsing
        if value.starts_with("--") {
            return Err(HistoricalBackfillCommandError::MissingValue(
                argument.clone(),
            ));
        }

        // retain repeated selector order while scalar options keep their last value
        match argument.as_str() {
            "--source" => raw.source_keys.push(stable_source_key(value)?),
            "--station" => raw.station_ids.push(tempest_station_id(value)?),
            _ => {
                raw.values.insert(argument.clone(), value.clone());
            }
        }
        index += 2;
    }
    Ok(raw)
}

// report whether one value option belongs to the selected source command
fn value_option_is_supported(kind: HistoricalBackfillCommandKind, argument: &str) -> bool {
    match kind {
        HistoricalBackfillCommandKind::Tempest => matches!(
            argument,
            "--site" | "--station" | "--from" | "--to" | "--chunk-days" | "--report"
        ),
        HistoricalBackfillCommandKind::Tide | HistoricalBackfillCommandKind::PublicStations => {
            matches!(
                argument,
                "--site" | "--source" | "--from" | "--to" | "--report"
            )
        }
    }
}

// bind one command to the exact checked local configuration family
const fn configuration_family(
    kind: HistoricalBackfillCommandKind,
) -> HistoricalBackfillConfigurationFamily {
    match kind {
        HistoricalBackfillCommandKind::Tempest => HistoricalBackfillConfigurationFamily::Tempest,
        HistoricalBackfillCommandKind::Tide => HistoricalBackfillConfigurationFamily::Tide,
        HistoricalBackfillCommandKind::PublicStations => {
            HistoricalBackfillConfigurationFamily::PublicStations
        }
    }
}

// require one scalar option while preserving its original nonempty value
fn require_site(
    kind: HistoricalBackfillCommandKind,
    values: &BTreeMap<String, String>,
) -> Result<String, HistoricalBackfillCommandError> {
    let value = values
        .get("--site")
        .filter(
            // Tide admits whitespace while Tempest and public stations require visible text
            |value| {
                if kind == HistoricalBackfillCommandKind::Tide {
                    !value.is_empty()
                } else {
                    !value.trim().is_empty()
                }
            },
        )
        .ok_or_else(|| HistoricalBackfillCommandError::MissingRequired("--site".to_owned()))?;
    Ok(value.clone())
}

// admit today's exact UTC date through the external calendar owner
fn admitted_today<Time: HistoricalBackfillCommandTimeAdapter>(
    time: &mut Time,
) -> Result<HistoricalBackfillCommandDate, HistoricalBackfillCommandError> {
    let today = time.today_utc()?;
    validate_admitted_date(&today, today.label())?;
    Ok(today)
}

// admit one explicit date and bind the adapter to the caller's original label
fn admitted_explicit_date<Time: HistoricalBackfillCommandTimeAdapter>(
    time: &mut Time,
    label: &str,
    field: &str,
) -> Result<HistoricalBackfillCommandDate, HistoricalBackfillCommandError> {
    // reject malformed labels before invoking the date owner
    if !date_label_is_valid(label) {
        return Err(HistoricalBackfillCommandError::InvalidDate(
            field.to_owned(),
        ));
    }
    let date = time.admit_utc_date(label)?;
    validate_admitted_date(&date, label)
        .map_err(|_| HistoricalBackfillCommandError::InvalidDate(field.to_owned()))?;
    Ok(date)
}

// apply the Node default of yesterday through the approved UTC date owner
fn admitted_previous_day<Time: HistoricalBackfillCommandTimeAdapter>(
    time: &mut Time,
    today: &HistoricalBackfillCommandDate,
) -> Result<HistoricalBackfillCommandDate, HistoricalBackfillCommandError> {
    let previous = time.add_utc_days(today, -1)?;
    validate_admitted_date(&previous, previous.label())?;
    let expected = today
        .start_ms
        .checked_sub(MILLISECONDS_PER_DAY)
        .ok_or_else(|| HistoricalBackfillCommandError::InvalidDate("--to".to_owned()))?;

    // bind the adapter result to one exact prior UTC-day transition
    if previous.start_ms != expected {
        return Err(HistoricalBackfillCommandError::InvalidDate(
            "--to".to_owned(),
        ));
    }
    Ok(previous)
}

// admit one optional caller-supplied lower bound
fn optional_date<Time: HistoricalBackfillCommandTimeAdapter>(
    time: &mut Time,
    label: Option<&String>,
    field: &str,
) -> Result<Option<HistoricalBackfillCommandDate>, HistoricalBackfillCommandError> {
    label
        .map(
            // retain absence so each executor applies its configured source history bound
            |value| admitted_explicit_date(time, value, field),
        )
        .transpose()
}

// require a forward or single-day explicit range
fn reject_reversed(
    from: Option<&HistoricalBackfillCommandDate>,
    to: &HistoricalBackfillCommandDate,
) -> Result<(), HistoricalBackfillCommandError> {
    // compare admitted UTC midnights rather than raw strings
    if from.is_some_and(|from| from.start_ms > to.start_ms) {
        return Err(HistoricalBackfillCommandError::InvalidRange);
    }
    Ok(())
}

// validate one admitted date without implementing calendar rollover here
fn validate_admitted_date(
    date: &HistoricalBackfillCommandDate,
    expected_label: &str,
) -> Result<(), HistoricalBackfillCommandError> {
    // require exact label preservation and one UTC midnight, including pre-epoch valid dates
    if date.label != expected_label
        || !date_label_is_valid(&date.label)
        || date.start_ms % MILLISECONDS_PER_DAY != 0
    {
        return Err(HistoricalBackfillCommandError::InvalidDate(
            expected_label.to_owned(),
        ));
    }
    Ok(())
}

// validate only the fixed date-label grammar before calendar admission
pub(crate) fn date_label_is_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

// retain Node Number and safe-integer semantics for a Tempest location selector
fn tempest_station_id(value: &str) -> Result<u64, HistoricalBackfillCommandError> {
    let normalized = value.strip_prefix("tempest-").unwrap_or(value);
    let parsed =
        javascript_number(normalized).ok_or(HistoricalBackfillCommandError::InvalidStationId)?;

    // accept every positive JavaScript-safe integer spelling and reject all others
    if !parsed.is_finite()
        || !(1.0..=JAVASCRIPT_MAXIMUM_SAFE_INTEGER).contains(&parsed)
        || parsed.fract() != 0.0
    {
        return Err(HistoricalBackfillCommandError::InvalidStationId);
    }
    Ok(parsed as u64)
}

// retain the exact lowercase kebab-case source selector contract
fn stable_source_key(value: &str) -> Result<String, HistoricalBackfillCommandError> {
    let bytes = value.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= 80
        && is_lowercase_alphanumeric(bytes[0])
        && is_lowercase_alphanumeric(bytes[bytes.len() - 1])
        && bytes.iter().all(
            // admit only lowercase letters, decimal digits, and separated hyphens
            |byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-',
        )
        && !bytes.windows(2).any(|pair| pair == b"--");

    // reject any source key outside the Node regular expression
    if !valid {
        return Err(HistoricalBackfillCommandError::InvalidSourceKey);
    }
    Ok(value.to_owned())
}

// retain Node Number semantics while requiring the provider-native one-day Tempest chunk
fn tempest_chunk_days(value: Option<&String>) -> Result<u16, HistoricalBackfillCommandError> {
    let parsed = value.map_or(Some(1.0), |value| javascript_number(value));

    // reject every noninteger or changed Tempest chunk size
    if parsed != Some(1.0) {
        return Err(HistoricalBackfillCommandError::InvalidChunkDays);
    }
    Ok(1)
}

// report whether one byte belongs to the source-key alphanumeric alphabet
fn is_lowercase_alphanumeric(value: u8) -> bool {
    value.is_ascii_lowercase() || value.is_ascii_digit()
}

// reproduce the admitted JavaScript Number spellings used by the existing CLI
pub(crate) fn javascript_number(value: &str) -> Option<f64> {
    let trimmed = javascript_trim(value);

    // JavaScript Number maps an empty or whitespace-only string to zero
    if trimmed.is_empty() {
        return Some(0.0);
    }
    let radix = [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ]
    .into_iter()
    .find_map(
        // JavaScript accepts unsigned explicit radix prefixes
        |(prefix, radix)| trimmed.strip_prefix(prefix).map(|digits| (digits, radix)),
    );

    // admit the explicit radix forms before ordinary decimal and exponent syntax
    if let Some((digits, radix)) = radix {
        // reject a prefix without any digits
        if digits.is_empty() || !digits.chars().all(|digit| digit.is_digit(radix)) {
            return None;
        }
        return u64::from_str_radix(digits, radix)
            .ok()
            .map(|value| value as f64);
    }
    trimmed.parse::<f64>().ok()
}

// reproduce the whitespace and line terminators accepted by JavaScript string trim
pub(crate) fn javascript_trim(value: &str) -> &str {
    value.trim_matches(
        // include the BOM and exclude Unicode spaces outside the ECMAScript set
        |character| {
            matches!(
                character,
                '\u{0009}'..='\u{000d}'
                    | '\u{0020}'
                    | '\u{00a0}'
                    | '\u{1680}'
                    | '\u{2000}'..='\u{200a}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202f}'
                    | '\u{205f}'
                    | '\u{3000}'
                    | '\u{feff}'
            )
        },
    )
}

// preserve insertion order while matching JavaScript Set duplicate removal
fn deduplicate<T: Clone + Ord>(values: Vec<T>) -> Vec<T> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::with_capacity(values.len());

    // keep only the first occurrence of every selector
    for value in values {
        if seen.insert(value.clone()) {
            unique.push(value);
        }
    }
    unique
}

// convert one neutral command date to the public-station and Tempest date owner
fn public_station_date(date: HistoricalBackfillCommandDate) -> PublicStationBackfillDate {
    PublicStationBackfillDate::admitted(date.label, date.start_ms)
}

// convert one neutral command date to the tide executor date owner
fn tide_date(date: HistoricalBackfillCommandDate) -> TideBackfillDate {
    TideBackfillDate::admitted(date.label, date.start_ms)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;

    const TODAY_MS: i64 = 1_795_392_000_000;
    const YESTERDAY_MS: i64 = 1_795_305_600_000;
    const TIDE_FROM_MS: i64 = 1_546_300_800_000;

    struct FixtureTime;

    impl HistoricalBackfillCommandTimeAdapter for FixtureTime {
        /// Return the fixed Node oracle UTC date.
        fn today_utc(&mut self) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
            Ok(HistoricalBackfillCommandDate::admitted(
                "2026-11-23",
                TODAY_MS,
            ))
        }

        /// Admit only exact explicit fixture dates.
        fn admit_utc_date(
            &mut self,
            label: &str,
        ) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
            // map every explicit parity date without implementing a date codec
            let start_ms = match label {
                "2019-01-01" => TIDE_FROM_MS,
                "2026-11-20" => 1_795_132_800_000,
                "2026-11-21" => 1_795_219_200_000,
                "2026-11-22" => YESTERDAY_MS,
                "2026-11-23" => TODAY_MS,
                _ => {
                    return Err(TaskFailure {
                        attempts: 0,
                        code: "fixture_date_unavailable".to_owned(),
                    });
                }
            };
            Ok(HistoricalBackfillCommandDate::admitted(label, start_ms))
        }

        /// Apply only the deployed yesterday default.
        fn add_utc_days(
            &mut self,
            date: &HistoricalBackfillCommandDate,
            days: i64,
        ) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
            assert_eq!(date.label(), "2026-11-23");
            assert_eq!(days, -1);
            Ok(HistoricalBackfillCommandDate::admitted(
                "2026-11-22",
                YESTERDAY_MS,
            ))
        }
    }

    // preserve deployed wrapper arguments before the first CLI defaults
    #[test]
    fn deployed_wrappers_map_to_exact_typed_defaults() {
        let mut time = FixtureTime;
        let tempest =
            deployed_historical_backfill_command(HistoricalBackfillCommandKind::Tempest, &mut time)
                .expect("Tempest wrapper");
        assert_eq!(tempest.report_path, None);
        assert_eq!(
            tempest.source_configuration,
            HistoricalBackfillSourceConfiguration::Required(
                HistoricalBackfillConfigurationFamily::Tempest,
            )
        );
        let HistoricalBackfillArguments::Tempest(tempest) = tempest.arguments else {
            panic!("Tempest arguments expected");
        };
        assert_eq!(tempest.chunk_days, 1);
        assert!(!tempest.dry_run);
        assert_eq!(tempest.from, None);
        assert!(tempest.resume);
        assert_eq!(tempest.site, DEPLOYED_SITE);
        assert_eq!(tempest.station_ids, Vec::<u64>::new());
        assert_eq!(tempest.to.label(), "2026-11-22");

        let tide =
            deployed_historical_backfill_command(HistoricalBackfillCommandKind::Tide, &mut time)
                .expect("tide wrapper");
        let HistoricalBackfillArguments::Tide(tide) = tide.arguments else {
            panic!("tide arguments expected");
        };
        assert_eq!(tide.from.label(), TIDE_DEFAULT_FROM);
        assert_eq!(tide.to.label(), "2026-11-22");
        assert!(tide.resume);

        let public = deployed_historical_backfill_command(
            HistoricalBackfillCommandKind::PublicStations,
            &mut time,
        )
        .expect("public-stations wrapper");
        let HistoricalBackfillArguments::PublicStations(public) = public.arguments else {
            panic!("public-stations arguments expected");
        };
        assert_eq!(public.from, None);
        assert_eq!(public.to.label(), "2026-11-22");
        assert!(public.resume);
    }

    // match the actual compiled Node parsers without loading configuration or opening ports
    #[test]
    fn dry_run_arguments_match_compiled_node_cli_parsers() {
        let tempest_args = strings(&[
            "--site",
            "ballydidean",
            "--dry-run",
            "--resume",
            "--station",
            "tempest-203055",
            "--station",
            "0x3192f",
            "--chunk-days",
            "0x1",
            "--report",
            "tempest.json",
        ]);
        let tide_args = strings(&[
            "--site",
            "ballydidean",
            "--dry-run",
            "--resume",
            "--source",
            "noaa-port-townsend-water-level-v1",
            "--source",
            "noaa-port-townsend-water-level-v1",
            "--report",
            "tide.json",
        ]);
        let public_args = strings(&[
            "--site",
            "ballydidean",
            "--dry-run",
            "--source",
            "ambient-ksea-observations-v1",
            "--from",
            "2026-11-20",
            "--to",
            "2026-11-21",
            "--report",
            "public.json",
        ]);
        let mut time = FixtureTime;
        let rust = [
            parity_line(
                &parse_historical_backfill_command(
                    HistoricalBackfillCommandKind::Tempest,
                    &tempest_args,
                    &mut time,
                )
                .expect("Tempest dry-run"),
            ),
            parity_line(
                &parse_historical_backfill_command(
                    HistoricalBackfillCommandKind::Tide,
                    &tide_args,
                    &mut time,
                )
                .expect("tide dry-run"),
            ),
            parity_line(
                &parse_historical_backfill_command(
                    HistoricalBackfillCommandKind::PublicStations,
                    &public_args,
                    &mut time,
                )
                .expect("public-stations dry-run"),
            ),
        ]
        .join("\n");
        assert_eq!(rust, compiled_node_parity());
    }

    // fail before side effects for changed ranges selectors and option families
    #[test]
    fn rejects_invalid_command_inputs_before_execution() {
        let mut time = FixtureTime;
        assert_eq!(
            parse_historical_backfill_command(
                HistoricalBackfillCommandKind::Tempest,
                &strings(&["--site", "ballydidean", "--station", "tempest-0"]),
                &mut time,
            ),
            Err(HistoricalBackfillCommandError::InvalidStationId)
        );
        assert_eq!(
            parse_historical_backfill_command(
                HistoricalBackfillCommandKind::Tide,
                &strings(&["--site", "ballydidean", "--source", "NOAA"]),
                &mut time,
            ),
            Err(HistoricalBackfillCommandError::InvalidSourceKey)
        );
        assert_eq!(
            parse_historical_backfill_command(
                HistoricalBackfillCommandKind::PublicStations,
                &strings(&[
                    "--site",
                    "ballydidean",
                    "--from",
                    "2026-11-22",
                    "--to",
                    "2026-11-20",
                ]),
                &mut time,
            ),
            Err(HistoricalBackfillCommandError::InvalidRange)
        );
        assert_eq!(
            parse_historical_backfill_command(
                HistoricalBackfillCommandKind::Tide,
                &strings(&["--site", "ballydidean", "--chunk-days", "1"]),
                &mut time,
            ),
            Err(HistoricalBackfillCommandError::UnsupportedArgument(
                "--chunk-days".to_owned(),
            ))
        );
    }

    /// Convert fixed test literals into process-style owned arguments.
    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    /// Project one typed command into the compiled parser oracle's stable line.
    fn parity_line(command: &HistoricalBackfillCommand) -> String {
        // retain exact family-specific nullable and repeated selector fields
        match &command.arguments {
            HistoricalBackfillArguments::Tempest(value) => format!(
                "tempest|{}|{}|{}|{}|{}|{}|{}|{}",
                value.chunk_days,
                value.dry_run,
                value.from.as_ref().map_or("null", |date| date.label()),
                command.report_path.as_deref().unwrap_or("null"),
                value.resume,
                value.site,
                value
                    .station_ids
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
                value.to.label(),
            ),
            HistoricalBackfillArguments::Tide(value) => format!(
                "tide|{}|{}|{}|{}|{}|{}|{}|{}",
                value.dry_run,
                value.from.label(),
                command.report_path.as_deref().unwrap_or("null"),
                value.resume,
                value.site,
                value.source_keys.join(","),
                value.to.label(),
                value.source_keys.len(),
            ),
            HistoricalBackfillArguments::PublicStations(value) => format!(
                "public|{}|{}|{}|{}|{}|{}|{}|{}",
                value.dry_run,
                value.from.as_ref().map_or("null", |date| date.label()),
                command.report_path.as_deref().unwrap_or("null"),
                value.resume,
                value.site,
                value.source_keys.join(","),
                value.to.label(),
                value.source_keys.len(),
            ),
        }
    }

    /// Execute only the three compiled parser functions with one fixed UTC clock.
    fn compiled_node_parity() -> String {
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let tempest = repository.join("apps/worker/dist/tempest-backfill-cli.js");
        let tide = repository.join("apps/worker/dist/tide-backfill-cli.js");
        let public = repository.join("apps/worker/dist/public-stations-backfill-cli.js");
        let script = format!(
            r#"
import {{ parseTempestBackfillArguments }} from "file://{}";
import {{ parseTideBackfillArguments }} from "file://{}";
import {{ parsePublicStationBackfillArguments }} from "file://{}";
const today = new Date("2026-11-23T12:00:00.000Z");
const tempest = parseTempestBackfillArguments({}, today);
const tide = parseTideBackfillArguments({}, today);
const publicStation = parsePublicStationBackfillArguments({}, today);
process.stdout.write([
  `tempest|${{tempest.chunkDays}}|${{tempest.dryRun}}|${{tempest.from ?? "null"}}|${{tempest.reportPath ?? "null"}}|${{tempest.resume}}|${{tempest.site}}|${{tempest.stationIds.join(",")}}|${{tempest.to}}`,
  `tide|${{tide.dryRun}}|${{tide.from}}|${{tide.reportPath ?? "null"}}|${{tide.resume}}|${{tide.site}}|${{tide.sourceKeys.join(",")}}|${{tide.to}}|${{tide.sourceKeys.length}}`,
  `public|${{publicStation.dryRun}}|${{publicStation.from ?? "null"}}|${{publicStation.reportPath ?? "null"}}|${{publicStation.resume}}|${{publicStation.site}}|${{publicStation.sourceKeys.join(",")}}|${{publicStation.to}}|${{publicStation.sourceKeys.length}}`,
].join("\n"));
"#,
            tempest.display(),
            tide.display(),
            public.display(),
            node_array(&[
                "--site",
                "ballydidean",
                "--dry-run",
                "--resume",
                "--station",
                "tempest-203055",
                "--station",
                "0x3192f",
                "--chunk-days",
                "0x1",
                "--report",
                "tempest.json",
            ]),
            node_array(&[
                "--site",
                "ballydidean",
                "--dry-run",
                "--resume",
                "--source",
                "noaa-port-townsend-water-level-v1",
                "--source",
                "noaa-port-townsend-water-level-v1",
                "--report",
                "tide.json",
            ]),
            node_array(&[
                "--site",
                "ballydidean",
                "--dry-run",
                "--source",
                "ambient-ksea-observations-v1",
                "--from",
                "2026-11-20",
                "--to",
                "2026-11-21",
                "--report",
                "public.json",
            ]),
        );
        let output = Command::new("node")
            .args(["--input-type=module", "--eval", &script])
            .output()
            .expect("execute compiled CLI parsers");
        assert!(
            output.status.success(),
            "Node parser failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("UTF-8 Node parser output")
    }

    /// Encode one fixed literal list without introducing a JSON dependency.
    fn node_array(values: &[&str]) -> String {
        format!(
            "[{}]",
            values
                .iter()
                .map(
                    // fixture values contain no escaping characters
                    |value| format!("\"{value}\""),
                )
                .collect::<Vec<_>>()
                .join(","),
        )
    }
}
