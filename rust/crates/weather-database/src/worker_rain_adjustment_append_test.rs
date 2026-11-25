//! Transaction and bind coverage for the original immutable rain serving append.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::SerializedJson;
use weather_contracts::{
    Nullable, PersistedRainAdjustmentRevision, RainAdjustmentHour, RainAdjustmentRun,
};

const HOURS_JSON: &str = "[{\"applied\":true,\"correctedPrecipitationMm\":1.25,\"modelLeadHours\":1,\"rawPrecipitationMm\":1.5,\"reasonCode\":null,\"validAt\":\"2026-11-24T01:00:00.000Z\"}]";

#[derive(Clone, Copy)]
enum InsertOutcome {
    Inserted,
    Conflict,
    Failure,
}

#[derive(Default)]
struct Observed {
    events: Vec<&'static str>,
    sql: Option<&'static str>,
    binds: Vec<String>,
}

struct Pool {
    observed: Rc<RefCell<Observed>>,
    outcome: InsertOutcome,
}

struct Session {
    observed: Rc<RefCell<Observed>>,
    outcome: InsertOutcome,
}

impl WorkerRainAdjustmentAppendPool for Pool {
    type Error = &'static str;
    type Session = Session;

    /// Retain one opaque session before transaction work.
    fn checkout(&mut self) -> Result<Self::Session, Self::Error> {
        self.observed.borrow_mut().events.push("checkout");
        Ok(Session {
            observed: Rc::clone(&self.observed),
            outcome: self.outcome,
        })
    }
}

impl WorkerRainAdjustmentAppendSession for Session {
    type Error = &'static str;

    /// Record the exact start of the serving transaction.
    fn begin(&mut self) -> Result<(), Self::Error> {
        self.observed.borrow_mut().events.push("begin");
        Ok(())
    }

    /// Capture SQL and binds before returning the configured database outcome.
    fn insert(
        &mut self,
        plan: WorkerRainAdjustmentInsertPlan<'_>,
    ) -> Result<Option<PersistedRainAdjustmentRevision>, Self::Error> {
        let mut observed = self.observed.borrow_mut();
        observed.events.push("insert");
        observed.sql = Some(plan.sql);
        observed.binds = plan.binds.into_iter().map(str::to_owned).collect();
        drop(observed);
        match self.outcome {
            InsertOutcome::Inserted => Ok(Some(persisted_revision())),
            InsertOutcome::Conflict => Ok(None),
            InsertOutcome::Failure => Err("insert failed"),
        }
    }

    /// Record successful completion of the retained transaction.
    fn commit(&mut self) -> Result<(), Self::Error> {
        self.observed.borrow_mut().events.push("commit");
        Ok(())
    }

    /// Record recovery of an incomplete retained transaction.
    fn rollback(&mut self) -> Result<(), Self::Error> {
        self.observed.borrow_mut().events.push("rollback");
        Ok(())
    }

    /// Record unconditional release of the retained session.
    fn release(&mut self) {
        self.observed.borrow_mut().events.push("release");
    }
}

struct Values;

impl WorkerRainAdjustmentAppendValues for Values {
    type Error = &'static str;

    /// Return the original compact hour-array JSON without normalization.
    fn encode_hours(
        &mut self,
        hours: &[RainAdjustmentHour],
    ) -> Result<SerializedJson, Self::Error> {
        assert_eq!(hours, fixture_hours());
        Ok(SerializedJson::admitted(HOURS_JSON))
    }
}

/// Prove the six immutable identifiers and exact hour JSON stay bound in source order.
#[test]
fn opens_inserted_run_with_exact_sql_and_binds_until_explicit_commit() {
    let observed = Rc::new(RefCell::new(Observed::default()));
    let mut pool = Pool {
        observed: Rc::clone(&observed),
        outcome: InsertOutcome::Inserted,
    };
    let run = fixture_run();
    let mut open = begin_worker_rain_adjustment_append(&mut pool, &mut Values, &run)
        .expect("insert succeeds")
        .expect("new immutable run opens a transaction");

    assert_eq!(open.persisted, persisted_revision());
    assert!(open.transaction.session_mut().is_some());
    {
        let observed = observed.borrow();
        assert_eq!(observed.events, ["checkout", "begin", "insert"]);
        assert_eq!(observed.sql, Some(expected_insert_sql()));
        assert_eq!(
            observed.binds,
            [
                run.runInitializedAt.as_str(),
                run.modelSha256.as_str(),
                run.inputSha256.as_str(),
                run.forecastClaimId.as_str(),
                run.firstReceivedAt.as_str(),
                run.decisionAt.as_str(),
                HOURS_JSON,
            ]
        );
    }

    open.transaction
        .commit()
        .expect("serving transaction commits");
    assert_eq!(
        observed.borrow().events,
        ["checkout", "begin", "insert", "commit", "release"]
    );
    assert!(open.transaction.session_mut().is_none());
}

/// Prove an idempotent conflict still commits and releases its empty transaction.
#[test]
fn commits_and_releases_insert_conflict_without_open_transaction() {
    let observed = Rc::new(RefCell::new(Observed::default()));
    let mut pool = Pool {
        observed: Rc::clone(&observed),
        outcome: InsertOutcome::Conflict,
    };
    let open = begin_worker_rain_adjustment_append(&mut pool, &mut Values, &fixture_run())
        .expect("conflict is an ordinary successful outcome");

    assert!(open.is_none());
    assert_eq!(
        observed.borrow().events,
        ["checkout", "begin", "insert", "commit", "release"]
    );
}

/// Prove a failed insert rolls back and releases without an overwrite handle.
#[test]
fn rolls_back_and_releases_failed_insert() {
    let observed = Rc::new(RefCell::new(Observed::default()));
    let mut pool = Pool {
        observed: Rc::clone(&observed),
        outcome: InsertOutcome::Failure,
    };
    let result = begin_worker_rain_adjustment_append(&mut pool, &mut Values, &fixture_run());

    assert!(result.is_err());
    assert_eq!(
        observed.borrow().events,
        ["checkout", "begin", "insert", "rollback", "release"]
    );
}

/// Build the exact immutable run accepted from the worker model owner.
fn fixture_run() -> RainAdjustmentRun {
    RainAdjustmentRun {
        runInitializedAt: "2026-11-24T00:00:00.000Z".to_owned(),
        modelSha256: "1".repeat(64),
        inputSha256: "2".repeat(64),
        forecastClaimId: "forecast-claim".to_owned(),
        firstReceivedAt: "2026-11-24T00:05:00.000Z".to_owned(),
        decisionAt: "2026-11-24T08:00:00.000Z".to_owned(),
        generatedAt: "2026-11-24T08:01:00.000Z".to_owned(),
        hours: fixture_hours(),
    }
}

/// Build the single public-safe hour serialized by the approved value owner.
fn fixture_hours() -> Vec<RainAdjustmentHour> {
    vec![RainAdjustmentHour {
        validAt: "2026-11-24T01:00:00.000Z".to_owned(),
        modelLeadHours: 1,
        rawPrecipitationMm: 1.5,
        correctedPrecipitationMm: 1.25,
        applied: true,
        reasonCode: Nullable::Null,
    }]
}

/// Return the exact database-owned tuple returned by INSERT RETURNING.
fn persisted_revision() -> PersistedRainAdjustmentRevision {
    let run = fixture_run();
    PersistedRainAdjustmentRevision {
        generatedAt: "2026-11-24T08:01:01.000Z".to_owned(),
        hours: run.hours,
        inputSha256: run.inputSha256,
        modelSha256: run.modelSha256,
        runInitializedAt: run.runInitializedAt,
        storedContentSha256: "3".repeat(64),
    }
}

/// Preserve the original INSERT, conflict target, and returned tuple exactly.
fn expected_insert_sql() -> &'static str {
    r#"
      INSERT INTO rain_adjustment_runs (run_initialized_at, model_sha256, input_sha256,
        forecast_claim_id, first_received_at, decision_at, hours)
      VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)
      ON CONFLICT (run_initialized_at, model_sha256) DO NOTHING
      RETURNING run_initialized_at AS "runInitializedAt", model_sha256 AS "modelSha256",
        input_sha256 AS "inputSha256", generated_at AS "generatedAt", hours,
        encode(sha256(convert_to(hours::text, 'UTF8')), 'hex') AS "storedContentSha256"
"#
}
