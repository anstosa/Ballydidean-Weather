//! Worker-only read of bounded raw rain captures awaiting one immutable model.

use weather_contracts::{Nullable, RainAdjustmentCapture, RainAdjustmentCaptureKindEnum};

use crate::Sha256Digest;

const DECISION_DELAY_MILLISECONDS: i64 = 8 * 3_600_000;
const MAXIMUM_CAPTURE_BODY_BYTES: usize = 2_000_000;
const MAXIMUM_AGGREGATE_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Exact newest eligible forecast-run selection for one immutable model.
pub const PENDING_RAIN_ADJUSTMENT_RUN_SQL: &str = r#"
    SELECT c.run_initialized_at FROM rain_capture_claims c
    JOIN rain_capture_receipts r ON r.claim_id = c.id
    WHERE c.kind = 'forecast' AND r.outcome = 'valid' AND r.available_by_decision IS TRUE
      AND c.run_initialized_at + interval '8 hours' <= clock_timestamp()
      AND c.run_initialized_at >= clock_timestamp() - interval '20 hours'
      AND NOT EXISTS (SELECT 1 FROM rain_adjustment_runs a
        WHERE a.run_initialized_at = c.run_initialized_at AND a.model_sha256 = $1)
    ORDER BY c.run_initialized_at DESC LIMIT 1
"#;

/// Exact ordered forecast-vintage and station-window capture selection.
pub const PENDING_RAIN_ADJUSTMENT_CAPTURES_SQL: &str = r#"
    SELECT c.id, c.kind, c.station_id, c.run_initialized_at, c.window_start,
      c.window_end_exclusive, r.completed_at, r.body_sha256, r.compressed_body
    FROM rain_capture_claims c JOIN rain_capture_receipts r ON r.claim_id = c.id
    WHERE r.outcome = 'valid' AND r.completed_at <= $2::timestamptz AND (
      (c.kind = 'forecast' AND c.run_initialized_at IN
        ($1::timestamptz, $1::timestamptz - interval '6 hours', $1::timestamptz - interval '12 hours'))
      OR (c.kind = 'station' AND c.window_end_exclusive > $2::timestamptz - interval '26 hours'
        AND c.window_start < $2::timestamptz))
    ORDER BY r.completed_at, c.id LIMIT 350
"#;

/// Closed scalar bind domain for the two pending-capture statements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerRainAdjustmentReadBind {
    Sha256(String),
    UtcInstant(String),
}

/// Exact one-bind pending-run query plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRainAdjustmentRunReadPlan {
    pub sql: &'static str,
    pub binds: [WorkerRainAdjustmentReadBind; 1],
}

/// Exact initialized-at then decision-at capture query plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRainAdjustmentCapturesReadPlan {
    pub sql: &'static str,
    pub binds: [WorkerRainAdjustmentReadBind; 2],
}

/// Selected run timestamp decoded by the database adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingRainAdjustmentRunReadRow {
    pub run_initialized_at_ms: i64,
}

/// Raw compressed capture row decoded without parsing its provider body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRainAdjustmentCaptureReadRow {
    pub claim_id: String,
    pub kind: RainAdjustmentCaptureKindEnum,
    pub station_id: Option<u64>,
    pub run_initialized_at_ms: Option<i64>,
    pub window_start_ms: Option<i64>,
    pub window_end_exclusive_ms: Option<i64>,
    pub completed_at_ms: i64,
    pub body_sha256: String,
    pub compressed_body: Vec<u8>,
}

/// Database boundary for the source's ordered two-query pending read.
pub trait WorkerRainAdjustmentReadPort {
    type Error;

    /// Select the newest eligible run, if one remains for this model.
    fn select_pending_rain_run(
        &mut self,
        plan: &PendingRainAdjustmentRunReadPlan,
    ) -> Result<Option<PendingRainAdjustmentRunReadRow>, Self::Error>;

    /// Select the complete ordered raw capture cohort for the chosen decision.
    fn select_pending_rain_captures(
        &mut self,
        plan: &PendingRainAdjustmentCapturesReadPlan,
    ) -> Result<Vec<PendingRainAdjustmentCaptureReadRow>, Self::Error>;
}

/// Approved UTC, gzip, and SHA-256 operations outside the database layer.
pub trait WorkerRainAdjustmentReadValues {
    type Error;

    /// Format one database timestamp as a canonical UTC millisecond instant.
    fn format_utc_milliseconds(&mut self, epoch_milliseconds: i64) -> Result<String, Self::Error>;

    /// Decode one gzip member while applying the exact per-body output limit.
    fn gunzip_bounded(
        &mut self,
        compressed: &[u8],
        maximum_output_bytes: usize,
    ) -> Result<Vec<u8>, Self::Error>;

    /// Digest the exact decompressed source bytes before provider parsing.
    fn sha256(&mut self, bytes: &[u8]) -> Result<Sha256Digest, Self::Error>;
}

/// Local refusal before returning any raw provider capture bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerRainAdjustmentReadAdmissionError {
    InvalidModelSha256,
    InvalidDecisionClock,
    CaptureBodyTooLarge,
    AggregateBodyTooLarge,
    CaptureBodySha256Mismatch,
}

/// Complete pending-capture read failure with owning boundaries retained.
#[derive(Debug, PartialEq, Eq)]
pub enum WorkerRainAdjustmentReadError<PortError, ValuesError> {
    Admission(WorkerRainAdjustmentReadAdmissionError),
    Port(PortError),
    Values(ValuesError),
}

/// Read and verify one bounded raw capture cohort for runtime provider decoding.
pub fn read_pending_rain_adjustment_captures<Port, Values>(
    port: &mut Port,
    values: &mut Values,
    model_sha256: &str,
) -> Result<Vec<RainAdjustmentCapture>, WorkerRainAdjustmentReadError<Port::Error, Values::Error>>
where
    Port: WorkerRainAdjustmentReadPort,
    Values: WorkerRainAdjustmentReadValues,
{
    // reject an unpinned model before reading private capture evidence
    if !valid_sha256(model_sha256) {
        return Err(admission(
            WorkerRainAdjustmentReadAdmissionError::InvalidModelSha256,
        ));
    }
    let run = port
        .select_pending_rain_run(&PendingRainAdjustmentRunReadPlan {
            sql: PENDING_RAIN_ADJUSTMENT_RUN_SQL,
            binds: [WorkerRainAdjustmentReadBind::Sha256(
                model_sha256.to_owned(),
            )],
        })
        .map_err(WorkerRainAdjustmentReadError::Port)?;
    // stop before formatting or re-reading frozen inputs when no run is pending
    let Some(run) = run else {
        return Ok(Vec::new());
    };
    let decision_at_ms = run
        .run_initialized_at_ms
        .checked_add(DECISION_DELAY_MILLISECONDS)
        .ok_or(admission(
            WorkerRainAdjustmentReadAdmissionError::InvalidDecisionClock,
        ))?;
    let run_initialized_at = values
        .format_utc_milliseconds(run.run_initialized_at_ms)
        .map_err(WorkerRainAdjustmentReadError::Values)?;
    let decision_at = values
        .format_utc_milliseconds(decision_at_ms)
        .map_err(WorkerRainAdjustmentReadError::Values)?;
    let rows = port
        .select_pending_rain_captures(&PendingRainAdjustmentCapturesReadPlan {
            sql: PENDING_RAIN_ADJUSTMENT_CAPTURES_SQL,
            binds: [
                WorkerRainAdjustmentReadBind::UtcInstant(run_initialized_at),
                WorkerRainAdjustmentReadBind::UtcInstant(decision_at),
            ],
        })
        .map_err(WorkerRainAdjustmentReadError::Port)?;

    let mut decoded_bytes = 0_usize;
    let mut captures = Vec::with_capacity(rows.len());
    // retain database order while enforcing per-body and aggregate decoded limits
    for row in rows {
        let body = values
            .gunzip_bounded(&row.compressed_body, MAXIMUM_CAPTURE_BODY_BYTES)
            .map_err(WorkerRainAdjustmentReadError::Values)?;
        // contain a codec owner that returns more than the requested output bound
        if body.len() > MAXIMUM_CAPTURE_BODY_BYTES {
            return Err(admission(
                WorkerRainAdjustmentReadAdmissionError::CaptureBodyTooLarge,
            ));
        }
        decoded_bytes = decoded_bytes.checked_add(body.len()).ok_or(admission(
            WorkerRainAdjustmentReadAdmissionError::AggregateBodyTooLarge,
        ))?;
        // keep one inference within the exact aggregate decoded memory budget
        if decoded_bytes > MAXIMUM_AGGREGATE_BODY_BYTES {
            return Err(admission(
                WorkerRainAdjustmentReadAdmissionError::AggregateBodyTooLarge,
            ));
        }
        let digest = values
            .sha256(&body)
            .map_err(WorkerRainAdjustmentReadError::Values)?;
        // reject stored-body substitution before any runtime provider parser sees bytes
        if digest.as_str() != row.body_sha256 {
            return Err(admission(
                WorkerRainAdjustmentReadAdmissionError::CaptureBodySha256Mismatch,
            ));
        }
        captures.push(project_capture(row, body, values)?);
    }
    Ok(captures)
}

/// Project one verified raw row into the generated capture transport contract.
fn project_capture<PortError, Values>(
    row: PendingRainAdjustmentCaptureReadRow,
    body: Vec<u8>,
    values: &mut Values,
) -> Result<RainAdjustmentCapture, WorkerRainAdjustmentReadError<PortError, Values::Error>>
where
    Values: WorkerRainAdjustmentReadValues,
{
    Ok(RainAdjustmentCapture {
        claimId: row.claim_id,
        kind: row.kind,
        stationId: nullable(row.station_id),
        runInitializedAt: format_nullable_utc(row.run_initialized_at_ms, values)?,
        windowStart: format_nullable_utc(row.window_start_ms, values)?,
        windowEndExclusive: format_nullable_utc(row.window_end_exclusive_ms, values)?,
        completedAt: values
            .format_utc_milliseconds(row.completed_at_ms)
            .map_err(WorkerRainAdjustmentReadError::Values)?,
        bodySha256: row.body_sha256,
        body,
    })
}

/// Format one nullable database timestamp without inventing a clock codec.
fn format_nullable_utc<PortError, Values>(
    value: Option<i64>,
    values: &mut Values,
) -> Result<Nullable<String>, WorkerRainAdjustmentReadError<PortError, Values::Error>>
where
    Values: WorkerRainAdjustmentReadValues,
{
    value
        .map(|value| {
            values
                .format_utc_milliseconds(value)
                .map(Nullable::Value)
                .map_err(WorkerRainAdjustmentReadError::Values)
        })
        .unwrap_or(Ok(Nullable::Null))
}

/// Project one optional scalar into the generated nullable representation.
fn nullable<T>(value: Option<T>) -> Nullable<T> {
    value.map_or(Nullable::Null, Nullable::Value)
}

/// Recognize one lowercase hexadecimal SHA-256 identity.
fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Wrap one local admission refusal without changing either owner error.
const fn admission<PortError, ValuesError>(
    error: WorkerRainAdjustmentReadAdmissionError,
) -> WorkerRainAdjustmentReadError<PortError, ValuesError> {
    WorkerRainAdjustmentReadError::Admission(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL_SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const BODY_SHA256: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[derive(Default)]
    struct Port {
        run_plan: Option<PendingRainAdjustmentRunReadPlan>,
        captures_plan: Option<PendingRainAdjustmentCapturesReadPlan>,
    }

    impl WorkerRainAdjustmentReadPort for Port {
        type Error = &'static str;

        /// Capture the pending-run plan and return one fixed initialized time.
        fn select_pending_rain_run(
            &mut self,
            plan: &PendingRainAdjustmentRunReadPlan,
        ) -> Result<Option<PendingRainAdjustmentRunReadRow>, Self::Error> {
            self.run_plan = Some(plan.clone());
            Ok(Some(PendingRainAdjustmentRunReadRow {
                run_initialized_at_ms: 1_000,
            }))
        }

        /// Capture the cohort plan and return two rows in database order.
        fn select_pending_rain_captures(
            &mut self,
            plan: &PendingRainAdjustmentCapturesReadPlan,
        ) -> Result<Vec<PendingRainAdjustmentCaptureReadRow>, Self::Error> {
            self.captures_plan = Some(plan.clone());
            Ok(vec![capture("first", 2_000), capture("second", 3_000)])
        }
    }

    struct Values;

    impl WorkerRainAdjustmentReadValues for Values {
        type Error = &'static str;

        /// Render fixture epochs in a deterministic admitted representation.
        fn format_utc_milliseconds(
            &mut self,
            epoch_milliseconds: i64,
        ) -> Result<String, Self::Error> {
            Ok(format!("utc:{epoch_milliseconds}"))
        }

        /// Treat fixture compressed bytes as the decoded provider bytes.
        fn gunzip_bounded(
            &mut self,
            compressed: &[u8],
            maximum_output_bytes: usize,
        ) -> Result<Vec<u8>, Self::Error> {
            assert_eq!(maximum_output_bytes, MAXIMUM_CAPTURE_BODY_BYTES);
            Ok(compressed.to_vec())
        }

        /// Bind the fixture raw body to its configured digest.
        fn sha256(&mut self, bytes: &[u8]) -> Result<Sha256Digest, Self::Error> {
            assert_eq!(bytes, b"body");
            Ok(Sha256Digest::admitted(BODY_SHA256))
        }
    }

    /// Build one ordinary forecast capture database row.
    fn capture(claim_id: &str, completed_at_ms: i64) -> PendingRainAdjustmentCaptureReadRow {
        PendingRainAdjustmentCaptureReadRow {
            claim_id: claim_id.to_owned(),
            kind: RainAdjustmentCaptureKindEnum::Forecast,
            station_id: None,
            run_initialized_at_ms: Some(1_000),
            window_start_ms: None,
            window_end_exclusive_ms: None,
            completed_at_ms,
            body_sha256: BODY_SHA256.to_owned(),
            compressed_body: b"body".to_vec(),
        }
    }

    /// Verify exact query ordering, binds, UTC projection, and raw-row order.
    #[test]
    fn reads_exact_pending_cohort_and_preserves_row_order() {
        let mut port = Port::default();
        let captures = read_pending_rain_adjustment_captures(&mut port, &mut Values, MODEL_SHA256)
            .expect("pending captures");

        assert_eq!(
            port.run_plan,
            Some(PendingRainAdjustmentRunReadPlan {
                sql: PENDING_RAIN_ADJUSTMENT_RUN_SQL,
                binds: [WorkerRainAdjustmentReadBind::Sha256(
                    MODEL_SHA256.to_owned(),
                )],
            })
        );
        assert_eq!(
            port.captures_plan,
            Some(PendingRainAdjustmentCapturesReadPlan {
                sql: PENDING_RAIN_ADJUSTMENT_CAPTURES_SQL,
                binds: [
                    WorkerRainAdjustmentReadBind::UtcInstant("utc:1000".to_owned()),
                    WorkerRainAdjustmentReadBind::UtcInstant("utc:28801000".to_owned()),
                ],
            })
        );
        assert_eq!(
            captures
                .iter()
                .map(|capture| capture.claimId.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        assert_eq!(captures[0].completedAt, "utc:2000");
        assert_eq!(captures[0].body, b"body");
    }
}
