use weather_providers::{ProviderControls, provider_request_budget_milliseconds};
use weather_worker::TaskFailure;

const RUN_FINALIZATION_GRACE_MS: i64 = 30_000;
const MINIMUM_JAVASCRIPT_DATE_MS: i64 = -8_640_000_000_000_000;
const MAXIMUM_JAVASCRIPT_DATE_MS: i64 = 8_640_000_000_000_000;
const INVALID_PROVIDER_CONTROLS: &str = "invalid_provider_controls";
const INVALID_INGESTION_DEADLINE: &str = "invalid_ingestion_deadline";

/// Absolute provider and durable-run deadlines derived from one admitted start time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerIngestionDeadlines {
    pub provider_deadline_at_ms: i64,
    pub run_deadline_at_ms: i64,
}

/// Plan provider retry and finalization deadlines without sampling another clock.
pub fn plan_worker_ingestion_deadlines(
    started_at_ms: i64,
    controls: ProviderControls,
) -> Result<WorkerIngestionDeadlines, TaskFailure> {
    // reject a clock that Node could not represent as the supplied Date
    if !(MINIMUM_JAVASCRIPT_DATE_MS..=MAXIMUM_JAVASCRIPT_DATE_MS).contains(&started_at_ms) {
        return Err(failure(INVALID_INGESTION_DEADLINE));
    }
    let provider_budget_ms = provider_request_budget_milliseconds(controls)
        .map_err(|_| failure(INVALID_PROVIDER_CONTROLS))?;
    let provider_budget_ms =
        i64::try_from(provider_budget_ms).map_err(|_| failure(INVALID_INGESTION_DEADLINE))?;
    let provider_deadline_at_ms = started_at_ms
        .checked_add(provider_budget_ms)
        .ok_or_else(|| failure(INVALID_INGESTION_DEADLINE))?;
    let run_deadline_at_ms = provider_deadline_at_ms
        .checked_add(RUN_FINALIZATION_GRACE_MS)
        .ok_or_else(|| failure(INVALID_INGESTION_DEADLINE))?;

    // preserve Node toISOString validity for both returned deadlines
    if !(MINIMUM_JAVASCRIPT_DATE_MS..=MAXIMUM_JAVASCRIPT_DATE_MS).contains(&provider_deadline_at_ms)
        || !(MINIMUM_JAVASCRIPT_DATE_MS..=MAXIMUM_JAVASCRIPT_DATE_MS).contains(&run_deadline_at_ms)
    {
        return Err(failure(INVALID_INGESTION_DEADLINE));
    }
    Ok(WorkerIngestionDeadlines {
        provider_deadline_at_ms,
        run_deadline_at_ms,
    })
}

/// Construct one pre-provider planning failure without claiming an attempt.
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Match the default three-attempt provider budget plus finalization grace.
    #[test]
    fn plans_default_deadlines() {
        assert_eq!(
            plan_worker_ingestion_deadlines(1_000_000, ProviderControls::default()),
            Ok(WorkerIngestionDeadlines {
                provider_deadline_at_ms: 1_090_000,
                run_deadline_at_ms: 1_120_000,
            })
        );
    }

    /// Preserve custom bounded timeout and retry controls.
    #[test]
    fn plans_custom_deadlines() {
        assert_eq!(
            plan_worker_ingestion_deadlines(
                2_000_000,
                ProviderControls {
                    max_attempts: 2,
                    max_body_bytes: 1,
                    timeout_ms: 5_000,
                }
            ),
            Ok(WorkerIngestionDeadlines {
                provider_deadline_at_ms: 2_040_000,
                run_deadline_at_ms: 2_070_000,
            })
        );
    }

    /// Reject invalid controls and unrepresentable absolute times before provider work.
    #[test]
    fn rejects_invalid_controls_and_time_bounds() {
        assert_eq!(
            plan_worker_ingestion_deadlines(
                0,
                ProviderControls {
                    max_attempts: 0,
                    max_body_bytes: 1,
                    timeout_ms: 1,
                }
            ),
            Err(failure(INVALID_PROVIDER_CONTROLS))
        );
        assert_eq!(
            plan_worker_ingestion_deadlines(
                MAXIMUM_JAVASCRIPT_DATE_MS,
                ProviderControls::default()
            ),
            Err(failure(INVALID_INGESTION_DEADLINE))
        );
    }
}
