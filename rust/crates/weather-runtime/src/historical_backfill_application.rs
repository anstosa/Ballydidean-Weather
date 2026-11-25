//! Application entrypoints joining historical backfill argv to the retained process owner.

use weather_worker::{
    HistoricalBackfillCommandError, HistoricalBackfillCommandKind,
    HistoricalBackfillCommandTimeAdapter, TaskFailure, deployed_historical_backfill_command,
    parse_historical_backfill_command,
};

use crate::{
    HistoricalBackfillOutputPort, HistoricalBackfillProcessPort, run_historical_backfill_process,
};

/// One admission or execution failure from the historical backfill application boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoricalBackfillApplicationError {
    Command(HistoricalBackfillCommandError),
    Process(TaskFailure),
}

impl From<HistoricalBackfillCommandError> for HistoricalBackfillApplicationError {
    /// Preserve command admission separately from provider or persistence execution.
    fn from(value: HistoricalBackfillCommandError) -> Self {
        Self::Command(value)
    }
}

/// Parse one original CLI argv and run its retained configuration-to-report lifecycle.
pub fn run_historical_backfill_application<Time, Port, Output>(
    kind: HistoricalBackfillCommandKind,
    arguments: &[String],
    time: &mut Time,
    port: &mut Port,
    output: &mut Output,
) -> Result<u8, HistoricalBackfillApplicationError>
where
    Time: HistoricalBackfillCommandTimeAdapter,
    Port: HistoricalBackfillProcessPort,
    Output: HistoricalBackfillOutputPort,
{
    let command = parse_historical_backfill_command(kind, arguments, time)?;
    run_historical_backfill_process(command, port, output)
        .map_err(HistoricalBackfillApplicationError::Process)
}

/// Run the exact arguments installed by one deployed historical backfill wrapper.
pub fn run_deployed_historical_backfill_application<Time, Port, Output>(
    kind: HistoricalBackfillCommandKind,
    time: &mut Time,
    port: &mut Port,
    output: &mut Output,
) -> Result<u8, HistoricalBackfillApplicationError>
where
    Time: HistoricalBackfillCommandTimeAdapter,
    Port: HistoricalBackfillProcessPort,
    Output: HistoricalBackfillOutputPort,
{
    let command = deployed_historical_backfill_command(kind, time)?;
    run_historical_backfill_process(command, port, output)
        .map_err(HistoricalBackfillApplicationError::Process)
}
