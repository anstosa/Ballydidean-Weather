//! Installed binding for the three native online forecast archive operator verbs.

use std::{
    ffi::OsString,
    fmt,
    io::{self, Write},
    path::PathBuf,
};

use weather_edge::{
    ForecastAllocationStatfsPort, ForecastArchiveOperatorArgumentError,
    ForecastArchiveOperatorCommand, ForecastArchiveOperatorError, ForecastCycleHashPair,
    ForecastEvidenceReceiptCodec, NativeForecastArchiveOperator,
    NativeForecastArchiveOperatorError, parse_forecast_archive_operator_command,
    run_forecast_archive_operator_command,
};

use crate::{NativeForecastAllocationStatfs, NativeProcessClock, NativeWirePrimitives};

const DEFAULT_EVIDENCE_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";

/// Exact native application/output error returned after command and root admission.
pub type NativeInstalledForecastArchiveCommandError =
    ForecastArchiveOperatorError<NativeForecastArchiveOperatorError>;

/// Installed argument, root, storage, application, or stdout failure category.
#[derive(Debug)]
pub enum NativeInstalledForecastArchiveOperatorError {
    MissingCommand,
    NonUnicodeArgument,
    Arguments(ForecastArchiveOperatorArgumentError),
    UnsupportedCommand,
    Initialize(io::Error),
    Command(NativeInstalledForecastArchiveCommandError),
}

impl fmt::Display for NativeInstalledForecastArchiveOperatorError {
    /// Render one bounded command stage without configured paths or archive contents.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // preserve stable command ownership while keeping custody details out of diagnostics
        match self {
            Self::MissingCommand => formatter.write_str("forecast archive command is missing"),
            Self::NonUnicodeArgument => {
                formatter.write_str("forecast archive command argument is not Unicode")
            }
            Self::Arguments(_) => {
                formatter.write_str("forecast archive command arguments are invalid")
            }
            Self::UnsupportedCommand => {
                formatter.write_str("forecast archive command is unsupported")
            }
            Self::Initialize(_) => {
                formatter.write_str("forecast archive storage initialization failed")
            }
            Self::Command(_) => formatter.write_str("forecast archive command failed"),
        }
    }
}

impl std::error::Error for NativeInstalledForecastArchiveOperatorError {
    /// Preserve only ordinary nested errors with already bounded diagnostics.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // argument and custody errors expose their stable category through this owner
        match self {
            Self::Initialize(error) => Some(error),
            Self::MissingCommand
            | Self::NonUnicodeArgument
            | Self::Arguments(_)
            | Self::UnsupportedCommand
            | Self::Command(_) => None,
        }
    }
}

/// Run one installed command with the actual Linux filesystem-capacity sampler.
pub fn run_native_installed_forecast_archive_operator<Codec>(
    arguments: impl IntoIterator<Item = OsString>,
    codec: Codec,
    output: &mut impl Write,
) -> Result<(), NativeInstalledForecastArchiveOperatorError>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
{
    run_native_installed_forecast_archive_operator_with_statfs(
        arguments,
        codec,
        NativeForecastAllocationStatfs::new(),
        output,
    )
}

/// Run one installed command with explicit codec and filesystem-capacity owners.
pub fn run_native_installed_forecast_archive_operator_with_statfs<Codec, Statfs>(
    arguments: impl IntoIterator<Item = OsString>,
    codec: Codec,
    statfs: Statfs,
    output: &mut impl Write,
) -> Result<(), NativeInstalledForecastArchiveOperatorError>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: Clone + ForecastAllocationStatfsPort,
{
    // parse every verb and operand before reading environment or opening archive storage
    let command = parse_process_command(arguments)?;
    // the predecessor direct archive verbs use a fixed root, not the revision-root environment
    let root = PathBuf::from(DEFAULT_EVIDENCE_ROOT);
    let calendar = NativeWirePrimitives;
    let mut application = NativeForecastArchiveOperator::initialize(
        root,
        &codec,
        &calendar,
        NativeProcessClock,
        statfs,
    )
    .map_err(NativeInstalledForecastArchiveOperatorError::Initialize)?;
    run_forecast_archive_operator_command(command, &mut application, output)
        .map_err(NativeInstalledForecastArchiveOperatorError::Command)
}

/// Decode the complete process argument vector without observing environment or storage.
fn parse_process_command(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<ForecastArchiveOperatorCommand, NativeInstalledForecastArchiveOperatorError> {
    let arguments = arguments
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| NativeInstalledForecastArchiveOperatorError::NonUnicodeArgument)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (action, operands) = arguments
        .split_first()
        .ok_or(NativeInstalledForecastArchiveOperatorError::MissingCommand)?;
    let operands = operands.iter().map(String::as_str).collect::<Vec<_>>();
    parse_forecast_archive_operator_command(action, &operands)
        .map_err(NativeInstalledForecastArchiveOperatorError::Arguments)?
        .ok_or(NativeInstalledForecastArchiveOperatorError::UnsupportedCommand)
}
