//! File-only application boundary for the native rain performance report.

use std::collections::BTreeMap;
use std::fmt::{self, Display};

use weather_worker::{
    RainAdjustmentFeatureAdmission, RainAdjustmentTimeAdapter, RainPerformanceEvidenceError,
};

use crate::{
    AdjustmentEvaluationPackageVerificationValues, AdmittedRainPerformancePackagePair,
    ForecastTrainingPackageVerificationValues, RainArchivePerformanceError,
    RainArchivePerformanceOwners, RainPerformanceNativePackageValues,
    RainPerformancePackageCompositionError, RainPerformancePackageCompositionValues,
    RainPerformancePackageLoaderValues, RainPerformancePackageMaterializationError,
    RainPerformanceResultWriter, compose_admitted_rain_performance_package_pairs,
    create_archived_rain_performance_report,
    load_verified_rain_performance_package_pair_with_native_verifiers,
};

const RAIN_EVALUATE_COMMAND: &str = "rain-evaluate";

/// Approved clock, Git-revision, and report-composition owners.
pub trait RainPerformanceCliEnvironment:
    RainPerformancePackageCompositionValues + RainArchivePerformanceOwners
{
    type Error: Display;

    /// Capture the command clock once as one canonical UTC instant.
    fn capture_generated_at(&mut self) -> String;

    /// Resolve the bounded repository HEAD when no immutable revision was supplied.
    fn repository_head(&mut self) -> Result<String, Self::Error>;
}

/// Retained opaque package loader used only by the predecessor-compatible command entry point.
pub trait RainPerformanceLegacyCliEnvironment: RainPerformanceCliEnvironment {
    /// Independently verify and admit one package pair in argument order.
    fn load_verified_package_pair(
        &mut self,
        forecast_package: &str,
        adjustment_package: &str,
    ) -> Result<AdmittedRainPerformancePackagePair, Self::Error>;
}

/// Closed command failures preserving the original argument error classes.
#[derive(Debug, PartialEq)]
pub enum RainPerformanceCliError {
    CommandRequired,
    OptionsInvalid,
    UnsupportedCommand(String),
    MatchedPackagesRequired,
    PackageLoad(String),
    Composition(RainPerformancePackageCompositionError),
    Materialization(RainPerformancePackageMaterializationError),
    SourceRevision(String),
    SourceRevisionInvalid,
    Report(RainArchivePerformanceError),
    OptionRequiredExactlyOnce(&'static str),
    ResultPath(String),
    Serialization,
    ResultWrite(String),
}

impl Display for RainPerformanceCliError {
    /// Render the same ordinary command diagnostics as the TypeScript entrypoint.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // retain stable text for each closed command failure
        match self {
            Self::CommandRequired => {
                formatter.write_str("forecast adjustment performance command is required")
            }
            Self::OptionsInvalid => {
                formatter.write_str("forecast adjustment performance options are invalid")
            }
            Self::UnsupportedCommand(command) => write!(
                formatter,
                "unsupported forecast adjustment performance command: {command}"
            ),
            Self::MatchedPackagesRequired => formatter
                .write_str("rain evaluation requires matched forecast and adjustment packages"),
            Self::PackageLoad(message) | Self::SourceRevision(message) => {
                formatter.write_str(message)
            }
            Self::Composition(error) => formatter.write_str(composition_error_message(*error)),
            Self::Materialization(error) => Display::fmt(error, formatter),
            Self::SourceRevisionInvalid => formatter.write_str("source revision is invalid"),
            Self::Report(error) => formatter.write_str(report_error_message(error)),
            Self::OptionRequiredExactlyOnce(option) => {
                write!(formatter, "{option} is required exactly once")
            }
            Self::ResultPath(message) | Self::ResultWrite(message) => formatter.write_str(message),
            Self::Serialization => {
                formatter.write_str("rain performance report serialization failed")
            }
        }
    }
}

impl std::error::Error for RainPerformanceCliError {}

/// Parse, dispatch, evaluate, and publish one file-only rain performance command.
pub fn run_forecast_adjustment_performance_cli<Environment, Admission, ReplayTime, Writer>(
    argv: &[String],
    environment: &mut Environment,
    admission: &mut Admission,
    replay_time: &mut ReplayTime,
    writer: &mut Writer,
) -> Result<u8, RainPerformanceCliError>
where
    Environment: RainPerformanceLegacyCliEnvironment,
    Admission: RainAdjustmentFeatureAdmission,
    ReplayTime: RainAdjustmentTimeAdapter,
    Writer: RainPerformanceResultWriter,
{
    run_forecast_adjustment_performance_cli_with_package_loader(
        argv,
        environment,
        admission,
        replay_time,
        writer,
        |environment, forecast_package, adjustment_package| {
            environment.load_verified_package_pair(forecast_package, adjustment_package)
        },
    )
}

/// run the file-only command with both native package verifiers and typed rain loader
pub fn run_forecast_adjustment_performance_cli_with_native_package_loader<
    Environment,
    PackageValues,
    ForecastValues,
    AdjustmentValues,
    Admission,
    ReplayTime,
    Writer,
>(
    argv: &[String],
    environment: &mut Environment,
    package_values: &mut RainPerformanceNativePackageValues<
        '_,
        PackageValues,
        ForecastValues,
        AdjustmentValues,
    >,
    admission: &mut Admission,
    replay_time: &mut ReplayTime,
    writer: &mut Writer,
) -> Result<u8, RainPerformanceCliError>
where
    Environment: RainPerformanceCliEnvironment,
    PackageValues: RainPerformancePackageLoaderValues,
    ForecastValues: ForecastTrainingPackageVerificationValues,
    AdjustmentValues: AdjustmentEvaluationPackageVerificationValues,
    Admission: RainAdjustmentFeatureAdmission,
    ReplayTime: RainAdjustmentTimeAdapter,
    Writer: RainPerformanceResultWriter,
{
    run_forecast_adjustment_performance_cli_with_package_loader(
        argv,
        environment,
        admission,
        replay_time,
        writer,
        |_, forecast_package, adjustment_package| {
            load_verified_rain_performance_package_pair_with_native_verifiers(
                forecast_package,
                adjustment_package,
                package_values,
            )
        },
    )
}

/// Parse and dispatch one command through the selected package-loader boundary.
fn run_forecast_adjustment_performance_cli_with_package_loader<
    Environment,
    Admission,
    ReplayTime,
    Writer,
    LoadError,
>(
    argv: &[String],
    environment: &mut Environment,
    admission: &mut Admission,
    replay_time: &mut ReplayTime,
    writer: &mut Writer,
    mut load_package_pair: impl FnMut(
        &mut Environment,
        &str,
        &str,
    ) -> Result<AdmittedRainPerformancePackagePair, LoadError>,
) -> Result<u8, RainPerformanceCliError>
where
    Environment: RainPerformanceCliEnvironment,
    Admission: RainAdjustmentFeatureAdmission,
    ReplayTime: RainAdjustmentTimeAdapter,
    Writer: RainPerformanceResultWriter,
    LoadError: Display,
{
    let parsed = parse_options(argv)?;
    // dispatch only the migrated file-only command
    if parsed.command != RAIN_EVALUATE_COMMAND {
        return Err(RainPerformanceCliError::UnsupportedCommand(
            parsed.command.to_owned(),
        ));
    }
    run_rain_evaluate_command(
        &parsed.values,
        environment,
        admission,
        replay_time,
        writer,
        &mut load_package_pair,
    )?;
    Ok(0)
}

/// Map every completed native command result to the original process exit convention.
pub fn rain_performance_process_exit_code(result: &Result<u8, RainPerformanceCliError>) -> u8 {
    // preserve success zero and failure one without exposing report bytes on stdout
    if result.is_ok() { 0 } else { 1 }
}

/// Format one entrypoint diagnostic for stderr without performing the write effect.
pub fn rain_performance_failure_diagnostic(error: &RainPerformanceCliError) -> String {
    format!("forecast adjustment performance failed: {error}\n")
}

/// Execute the original rain-evaluate effect order over injected file-only owners.
fn run_rain_evaluate_command<Environment, Admission, ReplayTime, Writer, LoadError>(
    values: &BTreeMap<String, Vec<String>>,
    environment: &mut Environment,
    admission: &mut Admission,
    replay_time: &mut ReplayTime,
    writer: &mut Writer,
    load_package_pair: &mut impl FnMut(
        &mut Environment,
        &str,
        &str,
    ) -> Result<AdmittedRainPerformancePackagePair, LoadError>,
) -> Result<(), RainPerformanceCliError>
where
    Environment: RainPerformanceCliEnvironment,
    Admission: RainAdjustmentFeatureAdmission,
    ReplayTime: RainAdjustmentTimeAdapter,
    Writer: RainPerformanceResultWriter,
    LoadError: Display,
{
    let generated_at = environment.capture_generated_at();
    let forecast_packages = option_values(values, "--forecast-package");
    let adjustment_packages = values
        .get("--adjustment-package")
        .or_else(|| values.get("--evaluation-packages"))
        .map(Vec::as_slice)
        .unwrap_or_default();

    // reject missing or mismatched pairs after capturing the clock once
    if forecast_packages.is_empty() || forecast_packages.len() != adjustment_packages.len() {
        return Err(RainPerformanceCliError::MatchedPackagesRequired);
    }

    let mut pairs = Vec::with_capacity(forecast_packages.len());
    // verify each pair independently in the exact repeated-option order
    for (forecast_package, adjustment_package) in forecast_packages.iter().zip(adjustment_packages)
    {
        pairs.push(
            load_package_pair(environment, forecast_package, adjustment_package)
                .map_err(|error| RainPerformanceCliError::PackageLoad(error.to_string()))?,
        );
    }
    let composed = compose_admitted_rain_performance_package_pairs(pairs, environment)
        .map_err(RainPerformanceCliError::Composition)?;
    let source_revision = resolve_source_revision(values, environment)?;
    let (archive, inputs) = composed
        .materialize_archive()
        .map_err(RainPerformanceCliError::Materialization)?;
    let report = create_archived_rain_performance_report(
        archive,
        &inputs,
        &generated_at,
        &source_revision,
        environment,
        admission,
        replay_time,
    )
    .map_err(RainPerformanceCliError::Report)?;

    let output = one_option(values, "--output")?;
    let target = writer
        .admit_result_path(output)
        .map_err(|error| RainPerformanceCliError::ResultPath(error.to_string()))?;
    let bytes = environment
        .encode_canonical_json(&report.document)
        .ok_or(RainPerformanceCliError::Serialization)?;
    writer
        .write_new_result(target, &bytes)
        .map_err(|error| RainPerformanceCliError::ResultWrite(error.to_string()))?;
    Ok(())
}

/// Parsed command and repeated named values retained in argument order.
pub(crate) struct ParsedOptions {
    pub(crate) command: String,
    pub(crate) values: BTreeMap<String, Vec<String>>,
}

/// Parse exact option/value pairs while retaining unknown options for command-local handling.
pub(crate) fn parse_options(argv: &[String]) -> Result<ParsedOptions, RainPerformanceCliError> {
    let Some(command) = argv.first() else {
        return Err(RainPerformanceCliError::CommandRequired);
    };
    let mut values = BTreeMap::<String, Vec<String>>::new();
    let mut index = 1usize;
    // consume pairs without interpreting option-looking values
    while index < argv.len() {
        let Some(option) = argv.get(index) else {
            return Err(RainPerformanceCliError::OptionsInvalid);
        };
        let Some(value) = argv.get(index + 1) else {
            return Err(RainPerformanceCliError::OptionsInvalid);
        };
        // reject positional keys while retaining arbitrary named options
        if !option.starts_with("--") {
            return Err(RainPerformanceCliError::OptionsInvalid);
        }
        values
            .entry(option.clone())
            .or_default()
            .push(value.clone());
        index += 2;
    }
    Ok(ParsedOptions {
        command: command.clone(),
        values,
    })
}

/// Borrow repeated values or the original empty fallback.
pub(crate) fn option_values<'a>(
    values: &'a BTreeMap<String, Vec<String>>,
    option: &str,
) -> &'a [String] {
    values.get(option).map(Vec::as_slice).unwrap_or_default()
}

/// Require exactly one value for a command singleton.
pub(crate) fn one_option<'a>(
    values: &'a BTreeMap<String, Vec<String>>,
    option: &'static str,
) -> Result<&'a str, RainPerformanceCliError> {
    let entries = option_values(values, option);
    // reject absent and repeated singleton values with one stable class
    if entries.len() != 1 {
        return Err(RainPerformanceCliError::OptionRequiredExactlyOnce(option));
    }
    Ok(entries[0].as_str())
}

/// Resolve either the supplied immutable revision or the bounded repository owner.
fn resolve_source_revision(
    values: &BTreeMap<String, Vec<String>>,
    environment: &mut impl RainPerformanceCliEnvironment,
) -> Result<String, RainPerformanceCliError> {
    resolve_performance_source_revision(values, || environment.repository_head())
}

/// resolve one shared performance command revision without family-specific effects
pub(crate) fn resolve_performance_source_revision<Error: Display>(
    values: &BTreeMap<String, Vec<String>>,
    repository_head: impl FnOnce() -> Result<String, Error>,
) -> Result<String, RainPerformanceCliError> {
    let revision = if values.contains_key("--source-revision") {
        one_option(values, "--source-revision")?.to_owned()
    } else {
        repository_head()
            .map_err(|error| RainPerformanceCliError::SourceRevision(error.to_string()))?
    };
    // require one full lowercase Git object identity
    if !is_full_source_revision(&revision) {
        return Err(RainPerformanceCliError::SourceRevisionInvalid);
    }
    Ok(revision)
}

/// Recognize only exact lowercase 40- or 64-hexadecimal revisions.
fn is_full_source_revision(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Preserve predecessor messages for identifiable composition refusals.
pub(crate) fn composition_error_message(
    error: RainPerformancePackageCompositionError,
) -> &'static str {
    // map only refusals with exact predecessor counterparts
    match error {
        RainPerformancePackageCompositionError::Empty => {
            "performance package composition requires package pairs"
        }
        RainPerformancePackageCompositionError::OverlappingIntervals => {
            "performance package date intervals overlap"
        }
        RainPerformancePackageCompositionError::BodyIdentityCollision => {
            "performance package body identity collision"
        }
        RainPerformancePackageCompositionError::EdgeReceiptRowIdentityCollision => {
            "performance edge receipt row identity collision"
        }
        RainPerformancePackageCompositionError::Calendar
        | RainPerformancePackageCompositionError::CanonicalIdentity => {
            "performance package composition failed"
        }
    }
}

/// Preserve predecessor messages for identifiable rain replay refusals.
fn report_error_message(error: &RainArchivePerformanceError) -> &'static str {
    // map only refusals with exact predecessor counterparts
    match error {
        RainArchivePerformanceError::MissingBody | RainArchivePerformanceError::MissingClaim => {
            "rain receipt is missing its claim or body"
        }
        RainArchivePerformanceError::BodyHash(_) => {
            "rain provider body hash differs after verification"
        }
        RainArchivePerformanceError::InvalidClaimShape => "rain claim kind is invalid",
        RainArchivePerformanceError::MissingSourceForecast => {
            "rain adjustment run is missing its source forecast"
        }
        RainArchivePerformanceError::Evidence(RainPerformanceEvidenceError::StoredAmountParity) => {
            "rain stored amount parity failed"
        }
        RainArchivePerformanceError::Evidence(RainPerformanceEvidenceError::MixedModelIdentity) => {
            "rain evaluation contains mixed model identities"
        }
        RainArchivePerformanceError::Evidence(
            RainPerformanceEvidenceError::CapturedSourceParity,
        ) => "rain captured source amount parity failed",
        RainArchivePerformanceError::Evidence(
            RainPerformanceEvidenceError::CapturedServingParity,
        ) => "rain captured amount applied reason parity failed",
        _ => "rain performance report failed",
    }
}

#[cfg(test)]
#[path = "rain_performance_cli_test.rs"]
pub(crate) mod tests;
