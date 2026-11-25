//! Map the existing three online archive command verbs without opening host or network state.

use std::io::{self, Write};

use crate::forecast_cycle_pages::ForecastCycleSha256;
use crate::revision_gap_transfer::RevisionGapSha256;

/// Source output bound for a single canonical forced-command archive response.
pub const FORECAST_ARCHIVE_OPERATOR_MAXIMUM_BYTES: usize = 1_024 * 1_024;

/// Exact command operands admitted before the evidence root is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastArchiveOperatorCommand {
    Next,
    AcknowledgePage {
        page_sha256: ForecastCycleSha256,
        checkpoint_sha256: ForecastCycleSha256,
    },
    AcknowledgeFinal {
        manifest_sha256: ForecastCycleSha256,
        graph_sha256: ForecastCycleSha256,
    },
}

/// Keep each argument rejection in its existing command and operand category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastArchiveOperatorArgumentError {
    NextArity,
    PageAckArity,
    FinalAckArity,
    PageDigest,
    CheckpointDigest,
    ManifestDigest,
    GraphDigest,
}

/// Parse only the exact online archive family, leaving every other deployed verb to its owner.
pub fn parse_forecast_archive_operator_command(
    action: &str,
    operands: &[&str],
) -> Result<Option<ForecastArchiveOperatorCommand>, ForecastArchiveOperatorArgumentError> {
    use ForecastArchiveOperatorArgumentError as Error;
    use ForecastArchiveOperatorCommand as Command;

    Ok(Some(match (action, operands) {
        ("archive-next", []) => Command::Next,
        ("archive-next", _) => return Err(Error::NextArity),
        ("archive-ack", [page, checkpoint]) => Command::AcknowledgePage {
            page_sha256: digest(page).ok_or(Error::PageDigest)?,
            checkpoint_sha256: digest(checkpoint).ok_or(Error::CheckpointDigest)?,
        },
        ("archive-ack", _) => return Err(Error::PageAckArity),
        ("archive-ack-final", [manifest, graph]) => Command::AcknowledgeFinal {
            manifest_sha256: digest(manifest).ok_or(Error::ManifestDigest)?,
            graph_sha256: digest(graph).ok_or(Error::GraphDigest)?,
        },
        ("archive-ack-final", _) => return Err(Error::FinalAckArity),
        _ => return Ok(None),
    }))
}

/// Command-facing native custody and canonical envelope adapter.
pub trait ForecastArchiveOperatorApplication {
    type Error;

    /// Return a single exact canonical newline-framed final, page or idle envelope.
    fn next_archive_bytes(&mut self) -> Result<Vec<u8>, Self::Error>;

    /// Make this exact workstation transfer checkpoint durable and release its physical page.
    fn acknowledge_archive_page(
        &mut self,
        page_sha256: ForecastCycleSha256,
        checkpoint_sha256: ForecastCycleSha256,
    ) -> Result<(), Self::Error>;

    /// Preserve the first verified cold-graph binding for the exact pending final.
    fn acknowledge_archive_final(
        &mut self,
        manifest_sha256: ForecastCycleSha256,
        graph_sha256: ForecastCycleSha256,
    ) -> Result<(), Self::Error>;
}

/// Closed output and application errors for the existing operator process adapter.
#[derive(Debug)]
pub enum ForecastArchiveOperatorError<AppError> {
    Application(AppError),
    OutputTooLarge,
    Output(io::Error),
}

/// Execute an already admitted operand set against its attached local archive owner.
pub fn run_forecast_archive_operator_command<App: ForecastArchiveOperatorApplication>(
    command: ForecastArchiveOperatorCommand,
    application: &mut App,
    output: &mut impl Write,
) -> Result<(), ForecastArchiveOperatorError<App::Error>> {
    use ForecastArchiveOperatorCommand as Command;
    use ForecastArchiveOperatorError as Error;

    match command {
        Command::Next => {
            let bytes = application
                .next_archive_bytes()
                .map_err(Error::Application)?;
            // keep the existing final byte ceiling before writing any envelope bytes
            if bytes.len() > FORECAST_ARCHIVE_OPERATOR_MAXIMUM_BYTES {
                return Err(Error::OutputTooLarge);
            }
            output.write_all(&bytes).map_err(Error::Output)
        }
        Command::AcknowledgePage {
            page_sha256,
            checkpoint_sha256,
        } => application
            .acknowledge_archive_page(page_sha256, checkpoint_sha256)
            .map_err(Error::Application),
        Command::AcknowledgeFinal {
            manifest_sha256,
            graph_sha256,
        } => application
            .acknowledge_archive_final(manifest_sha256, graph_sha256)
            .map_err(Error::Application),
    }
}

/// Reuse the repository's admitted lowercase SHA-256 wire decoder.
fn digest(value: &str) -> Option<ForecastCycleSha256> {
    RevisionGapSha256::from_lower_hex(value)
        .map(|value| ForecastCycleSha256::admitted(*value.as_bytes()))
}
