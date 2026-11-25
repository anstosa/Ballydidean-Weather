//! Atomic database-clock authorization for one prospective rain capture.

use std::borrow::Cow;

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;
const CLAIM_LOCK: i64 = 7_203_492_731_157_622;

const CLAIM_LOCK_SQL: &str = "SELECT pg_advisory_xact_lock($1::bigint)";
const SERVER_CLOCK_SQL: &str = "SELECT clock_timestamp() AS now";
const CLAIM_GATE_SQL: &str = r#"
        SELECT
          EXISTS (SELECT 1 FROM rain_capture_claims WHERE slot_key = $1) AS existing,
          EXISTS (
            SELECT 1 FROM rain_capture_claims c
            LEFT JOIN rain_capture_receipts r ON r.claim_id = c.id
            WHERE r.claim_id IS NULL AND c.claimed_at > $3::timestamptz - interval '120 seconds'
          ) AS pending,
          GREATEST(
            (SELECT max(claimed_at) FROM rain_capture_claims),
            (SELECT max(completed_at) FROM rain_capture_receipts)
          ) AS latest,
          (SELECT count(*) FROM rain_capture_claims
            WHERE kind = $2 AND claimed_at >= $4::timestamptz)::text AS today,
          (SELECT count(*) FROM rain_capture_claims
            WHERE kind = $2 AND claimed_at > $3::timestamptz - interval '24 hours')::text AS rolling,
          (SELECT coalesce(sum(CASE WHEN r.claim_id IS NULL THEN 2100000
              ELSE coalesce(r.compressed_bytes, 0) END), 0)::text
            FROM rain_capture_claims c LEFT JOIN rain_capture_receipts r ON r.claim_id = c.id
            WHERE c.claimed_at >= $4::timestamptz) AS storage_day,
          (SELECT coalesce(sum(CASE WHEN r.claim_id IS NULL THEN 2100000
              ELSE coalesce(r.compressed_bytes, 0) END), 0)::text
            FROM rain_capture_claims c LEFT JOIN rain_capture_receipts r ON r.claim_id = c.id) AS storage_total,
          EXISTS (
            SELECT 1 FROM rain_capture_receipts r
            JOIN rain_capture_claims c ON c.id = r.claim_id
            WHERE (r.outcome = 'rate_limited' AND (
              r.metadata->>'retryAfterRequiresManualResume' = 'true' OR
              r.completed_at + greatest(interval '24 hours',
                coalesce((r.metadata->>'retryAfterSeconds')::integer, 0) * interval '1 second'
              ) > $3::timestamptz))
              OR (r.outcome = 'unauthorized' AND c.kind = $2
                AND r.completed_at > $3::timestamptz - interval '24 hours')
          ) AS paused,
          EXISTS (
            SELECT 1 FROM rain_capture_claims c
            JOIN rain_capture_receipts r ON r.claim_id = c.id
            WHERE $2 = 'forecast' AND c.kind = 'forecast'
              AND c.run_initialized_at = $5::timestamptz AND r.outcome = 'valid'
          ) AS valid_prior
"#;
const INSERT_CLAIM_SQL: &str = r#"
        INSERT INTO rain_capture_claims (
          kind, slot_key, station_id, run_initialized_at, attempt,
          window_start, window_end_exclusive, release, policy, policy_sha256, claimed_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::jsonb, $10, clock_timestamp())
        ON CONFLICT (slot_key) DO NOTHING RETURNING id
"#;

const STATION_IDS: [u64; 12] = [
    64_255, 225_947, 38_270, 168_853, 126_537, 201_058, 203_055, 66_270, 34_768, 88_159, 126_197,
    27_140,
];

struct FrozenPolicy {
    expires_at: &'static str,
    maximum_compressed_body_bytes: u64,
    maximum_forecast_requests_per_day: u64,
    maximum_station_requests_per_day: u64,
    maximum_stored_bytes: u64,
    maximum_stored_bytes_per_day: u64,
    minimum_request_spacing_ms: i64,
    starts_at: &'static str,
    station_access_authorized: bool,
}

const POLICY: FrozenPolicy = FrozenPolicy {
    expires_at: "2027-10-08T00:00:00.000Z",
    maximum_compressed_body_bytes: 2_100_000,
    maximum_forecast_requests_per_day: 8,
    maximum_station_requests_per_day: 288,
    maximum_stored_bytes: 2_147_483_648,
    maximum_stored_bytes_per_day: 8_388_608,
    minimum_request_spacing_ms: 1_100,
    starts_at: "2026-09-13T00:00:00.000Z",
    station_access_authorized: true,
};

/// One credential-free claim identity using either borrowed or owned strings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RainCaptureClaimRequest<'a> {
    Forecast {
        attempt: u8,
        run_initialized_at: Cow<'a, str>,
        slot_key: Cow<'a, str>,
    },
    Station {
        end_exclusive: Cow<'a, str>,
        slot_key: Cow<'a, str>,
        start: Cow<'a, str>,
        station_id: u64,
    },
}

/// Canonical frozen policy bytes and their externally approved SHA-256.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdmittedRainCaptureClaimPolicy<'a> {
    pub json: &'a str,
    pub sha256: &'a str,
}

/// Database discriminator used by both gate and insert statements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RainCaptureClaimKind {
    Forecast,
    Station,
}

/// Exact advisory transaction-lock statement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RainCaptureClaimLockStatement {
    pub sql: &'static str,
    pub lock_id: i64,
}

/// Exact database-clock statement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RainCaptureClaimClockStatement {
    pub sql: &'static str,
}

/// Typed claim gate binds in PostgreSQL placeholder order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainCaptureClaimGateBinds {
    pub slot_key: String,
    pub kind: RainCaptureClaimKind,
    pub now: String,
    pub day_start: String,
    pub run_initialized_at: Option<String>,
}

/// Exact gate statement and its typed binds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainCaptureClaimGateStatement {
    pub sql: &'static str,
    pub binds: RainCaptureClaimGateBinds,
}

/// One typed row returned by the complete quota and cooldown gate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RainCaptureClaimGateRow {
    pub existing: bool,
    pub pending: bool,
    pub latest_ms: Option<i64>,
    pub today: u64,
    pub rolling: u64,
    pub storage_day: u64,
    pub storage_total: u64,
    pub paused: bool,
    pub valid_prior: bool,
}

/// Typed insert binds in PostgreSQL placeholder order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainCaptureClaimInsertBinds {
    pub kind: RainCaptureClaimKind,
    pub slot_key: String,
    pub station_id: Option<u64>,
    pub run_initialized_at: Option<String>,
    pub attempt: Option<u8>,
    pub window_start: Option<String>,
    pub window_end_exclusive: Option<String>,
    pub release: String,
    pub policy_json: String,
    pub policy_sha256: String,
}

/// Exact claim insert statement and its typed binds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainCaptureClaimInsertStatement {
    pub sql: &'static str,
    pub binds: RainCaptureClaimInsertBinds,
}

/// Retained session for the advisory-locked claim transaction.
pub trait RainCaptureClaimSession {
    type Error;

    /// Start the transaction before acquiring the cross-worker lock.
    fn begin(&mut self) -> Result<(), Self::Error>;

    /// Acquire the exact transaction-scoped advisory lock.
    fn advisory_lock(
        &mut self,
        statement: &RainCaptureClaimLockStatement,
    ) -> Result<(), Self::Error>;

    /// Read the PostgreSQL server clock as Unix milliseconds.
    fn server_clock(
        &mut self,
        statement: &RainCaptureClaimClockStatement,
    ) -> Result<Option<i64>, Self::Error>;

    /// Read the complete duplicate, quota, storage, pause, and cooldown gate.
    fn claim_gate(
        &mut self,
        statement: &RainCaptureClaimGateStatement,
    ) -> Result<Option<RainCaptureClaimGateRow>, Self::Error>;

    /// Insert one immutable claim or return no id after a conflict race.
    fn insert_claim(
        &mut self,
        statement: &RainCaptureClaimInsertStatement,
    ) -> Result<Option<String>, Self::Error>;

    /// Commit either an inserted claim or an ordinary denial.
    fn commit(&mut self) -> Result<(), Self::Error>;

    /// Roll back any body or commit failure.
    fn rollback(&mut self) -> Result<(), Self::Error>;

    /// Return the retained client to its pool after every outcome.
    fn release(&mut self);
}

/// Pool owner for one retained claim session.
pub trait RainCaptureClaimPool {
    type Error;
    type Session: RainCaptureClaimSession<Error = Self::Error>;

    /// Check out one client after release validation succeeds.
    fn checkout(&mut self) -> Result<Self::Session, Self::Error>;
}

/// Approved calendar boundary for canonical UTC and immutable release validation.
pub trait RainCaptureClaimCalendar {
    type Error;

    /// Parse one exact canonical UTC millisecond instant.
    fn parse_utc(&mut self, value: &str) -> Result<i64, Self::Error>;

    /// Format one admitted Unix millisecond instant as canonical UTC.
    fn format_utc(&mut self, milliseconds: i64) -> Result<String, Self::Error>;

    /// Validate the immutable deployed release tag and real calendar date.
    fn validate_release(&mut self, release: &str) -> Result<bool, Self::Error>;
}

/// Claim failures retain their owning driver or calendar category.
#[derive(Debug, PartialEq, Eq)]
pub enum RainCaptureClaimError<DriverError, CalendarError> {
    Driver(DriverError),
    Calendar(CalendarError),
    DatabaseClockUnavailable,
    InvalidRelease,
    PolicyInactive,
    ForecastSlotOutsidePolicy,
    StationAccessNotAuthorized,
    StationSlotOutsidePolicy,
}

struct CheckedClaim {
    attempt: Option<u8>,
    kind: RainCaptureClaimKind,
    run_initialized_at: Option<String>,
    slot_key: String,
    station_id: Option<u64>,
    window_end_exclusive: Option<String>,
    window_start: Option<String>,
}

/// Claim one bounded rain capture through the exact server-clock transaction.
pub fn claim_rain_capture_slot<Pool, Calendar>(
    pool: &mut Pool,
    calendar: &mut Calendar,
    request: RainCaptureClaimRequest<'_>,
    release: &str,
    policy: AdmittedRainCaptureClaimPolicy<'_>,
) -> Result<Option<String>, RainCaptureClaimError<Pool::Error, Calendar::Error>>
where
    Pool: RainCaptureClaimPool,
    Calendar: RainCaptureClaimCalendar,
{
    let release_valid = calendar
        .validate_release(release)
        .map_err(RainCaptureClaimError::Calendar)?;

    // reject an invalid deployed tag before acquiring a database client
    if !release_valid {
        return Err(RainCaptureClaimError::InvalidRelease);
    }
    let mut session = pool.checkout().map_err(RainCaptureClaimError::Driver)?;
    let result = (|| {
        session.begin().map_err(RainCaptureClaimError::Driver)?;
        let operation = claim_transaction_body(&mut session, calendar, request, release, policy)
            .and_then(|claim| {
                session.commit().map_err(RainCaptureClaimError::Driver)?;
                Ok(claim)
            });

        // preserve Node withTransaction rollback-error precedence
        if operation.is_err() {
            session.rollback().map_err(RainCaptureClaimError::Driver)?;
        }
        operation
    })();
    session.release();
    result
}

/// Execute the exact lock, server clock, gate, and optional insert body.
fn claim_transaction_body<Session, Calendar>(
    session: &mut Session,
    calendar: &mut Calendar,
    request: RainCaptureClaimRequest<'_>,
    release: &str,
    policy: AdmittedRainCaptureClaimPolicy<'_>,
) -> Result<Option<String>, RainCaptureClaimError<Session::Error, Calendar::Error>>
where
    Session: RainCaptureClaimSession,
    Calendar: RainCaptureClaimCalendar,
{
    session
        .advisory_lock(&RainCaptureClaimLockStatement {
            sql: CLAIM_LOCK_SQL,
            lock_id: CLAIM_LOCK,
        })
        .map_err(RainCaptureClaimError::Driver)?;
    let now_ms = session
        .server_clock(&RainCaptureClaimClockStatement {
            sql: SERVER_CLOCK_SQL,
        })
        .map_err(RainCaptureClaimError::Driver)?
        .ok_or(RainCaptureClaimError::DatabaseClockUnavailable)?;
    let checked = validate_request(request, now_ms, calendar)?;
    let now = calendar
        .format_utc(now_ms)
        .map_err(RainCaptureClaimError::Calendar)?;
    let day_start_ms = now_ms.div_euclid(DAY_MS) * DAY_MS;
    let day_start = calendar
        .format_utc(day_start_ms)
        .map_err(RainCaptureClaimError::Calendar)?;
    let gate = session
        .claim_gate(&RainCaptureClaimGateStatement {
            sql: CLAIM_GATE_SQL,
            binds: RainCaptureClaimGateBinds {
                slot_key: checked.slot_key.clone(),
                kind: checked.kind,
                now,
                day_start,
                run_initialized_at: checked.run_initialized_at.clone(),
            },
        })
        .map_err(RainCaptureClaimError::Driver)?;

    // deny a missing or closed gate without attempting an insert
    let Some(gate) = gate else {
        return Ok(None);
    };
    let limit = match checked.kind {
        RainCaptureClaimKind::Forecast => POLICY.maximum_forecast_requests_per_day,
        RainCaptureClaimKind::Station => POLICY.maximum_station_requests_per_day,
    };

    // preserve every duplicate, overlap, quota, storage, pause, and cooldown denial
    if gate.existing
        || gate.pending
        || gate.paused
        || gate.valid_prior
        || gate.today >= limit
        || gate.rolling >= limit
        || gate
            .storage_day
            .checked_add(POLICY.maximum_compressed_body_bytes)
            .is_none_or(|bytes| bytes > POLICY.maximum_stored_bytes_per_day)
        || gate
            .storage_total
            .checked_add(POLICY.maximum_compressed_body_bytes)
            .is_none_or(|bytes| bytes > POLICY.maximum_stored_bytes)
        || gate.latest_ms.is_some_and(|latest| {
            now_ms
                .checked_sub(latest)
                .is_none_or(|elapsed| elapsed < POLICY.minimum_request_spacing_ms)
        })
    {
        return Ok(None);
    }
    session
        .insert_claim(&RainCaptureClaimInsertStatement {
            sql: INSERT_CLAIM_SQL,
            binds: RainCaptureClaimInsertBinds {
                kind: checked.kind,
                slot_key: checked.slot_key,
                station_id: checked.station_id,
                run_initialized_at: checked.run_initialized_at,
                attempt: checked.attempt,
                window_start: checked.window_start,
                window_end_exclusive: checked.window_end_exclusive,
                release: release.to_owned(),
                policy_json: policy.json.to_owned(),
                policy_sha256: policy.sha256.to_owned(),
            },
        })
        .map_err(RainCaptureClaimError::Driver)
}

/// Validate one fixed forecast or station identity against the server clock.
fn validate_request<DriverError, Calendar>(
    request: RainCaptureClaimRequest<'_>,
    now_ms: i64,
    calendar: &mut Calendar,
) -> Result<CheckedClaim, RainCaptureClaimError<DriverError, Calendar::Error>>
where
    Calendar: RainCaptureClaimCalendar,
{
    let starts_at = calendar
        .parse_utc(POLICY.starts_at)
        .map_err(RainCaptureClaimError::Calendar)?;
    let expires_at = calendar
        .parse_utc(POLICY.expires_at)
        .map_err(RainCaptureClaimError::Calendar)?;

    // authorize collection only inside the immutable policy lifetime
    if now_ms < starts_at || now_ms >= expires_at {
        return Err(RainCaptureClaimError::PolicyInactive);
    }
    match request {
        RainCaptureClaimRequest::Forecast {
            attempt,
            run_initialized_at,
            slot_key,
        } => validate_forecast_claim(attempt, run_initialized_at, slot_key, now_ms, calendar),
        RainCaptureClaimRequest::Station {
            end_exclusive,
            slot_key,
            start,
            station_id,
        } => validate_station_claim(end_exclusive, slot_key, start, station_id, now_ms, calendar),
    }
}

/// Validate one exact ECMWF cycle attempt window.
fn validate_forecast_claim<DriverError, Calendar>(
    attempt: u8,
    run_initialized_at: Cow<'_, str>,
    slot_key: Cow<'_, str>,
    now_ms: i64,
    calendar: &mut Calendar,
) -> Result<CheckedClaim, RainCaptureClaimError<DriverError, Calendar::Error>>
where
    Calendar: RainCaptureClaimCalendar,
{
    let initialized_ms = calendar
        .parse_utc(&run_initialized_at)
        .map_err(RainCaptureClaimError::Calendar)?;
    let age = now_ms
        .checked_sub(initialized_ms)
        .ok_or(RainCaptureClaimError::ForecastSlotOutsidePolicy)?;
    let expected = format!("forecast:{run_initialized_at}:{attempt}");
    let fixed_cycle =
        initialized_ms % HOUR_MS == 0 && initialized_ms.div_euclid(HOUR_MS).rem_euclid(24) % 6 == 0;
    let fixed_attempt_window = match attempt {
        1 => (6 * HOUR_MS..7 * HOUR_MS).contains(&age),
        2 => (7 * HOUR_MS..12 * HOUR_MS).contains(&age),
        _ => false,
    };

    // bind the slot key to one of the two exact cycle receipt windows
    if !fixed_cycle || slot_key != expected || !fixed_attempt_window {
        return Err(RainCaptureClaimError::ForecastSlotOutsidePolicy);
    }
    Ok(CheckedClaim {
        attempt: Some(attempt),
        kind: RainCaptureClaimKind::Forecast,
        run_initialized_at: Some(run_initialized_at.into_owned()),
        slot_key: slot_key.into_owned(),
        station_id: None,
        window_end_exclusive: None,
        window_start: None,
    })
}

/// Validate one fixed gauge and exact closed two-hour station window.
fn validate_station_claim<DriverError, Calendar>(
    end_exclusive: Cow<'_, str>,
    slot_key: Cow<'_, str>,
    start: Cow<'_, str>,
    station_id: u64,
    now_ms: i64,
    calendar: &mut Calendar,
) -> Result<CheckedClaim, RainCaptureClaimError<DriverError, Calendar::Error>>
where
    Calendar: RainCaptureClaimCalendar,
{
    // keep the frozen operator authorization in the claim boundary
    if !POLICY.station_access_authorized {
        return Err(RainCaptureClaimError::StationAccessNotAuthorized);
    }
    let start_ms = calendar
        .parse_utc(&start)
        .map_err(RainCaptureClaimError::Calendar)?;
    let end_ms = calendar
        .parse_utc(&end_exclusive)
        .map_err(RainCaptureClaimError::Calendar)?;
    let hour_ms = end_ms
        .checked_sub(1_000)
        .ok_or(RainCaptureClaimError::StationSlotOutsidePolicy)?;
    let earliest_ms = hour_ms
        .checked_add(2 * 60_000)
        .ok_or(RainCaptureClaimError::StationSlotOutsidePolicy)?;
    let latest_ms = hour_ms
        .checked_add(2 * HOUR_MS)
        .ok_or(RainCaptureClaimError::StationSlotOutsidePolicy)?;
    let expected = format!("station:{station_id}:{end_exclusive}");
    let fixed_window = end_ms
        .checked_sub(start_ms)
        .is_some_and(|duration| duration == 2 * HOUR_MS);

    // require one frozen gauge and its two-hour post-close admission interval
    if !STATION_IDS.contains(&station_id)
        || hour_ms % HOUR_MS != 0
        || !fixed_window
        || slot_key != expected
        || now_ms < earliest_ms
        || now_ms >= latest_ms
    {
        return Err(RainCaptureClaimError::StationSlotOutsidePolicy);
    }
    Ok(CheckedClaim {
        attempt: None,
        kind: RainCaptureClaimKind::Station,
        run_initialized_at: None,
        slot_key: slot_key.into_owned(),
        station_id: Some(station_id),
        window_end_exclusive: Some(end_exclusive.into_owned()),
        window_start: Some(start.into_owned()),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    const INITIALIZED_MS: i64 = 1_789_365_600_000;
    const NOW_MS: i64 = 1_789_389_000_000;
    const DAY_START_MS: i64 = 1_789_344_000_000;
    const INITIALIZED: &str = "2026-09-14T06:00:00.000Z";
    const NOW: &str = "2026-09-14T12:30:00.000Z";
    const DAY_START: &str = "2026-09-14T00:00:00.000Z";
    const RELEASE: &str = "2026.09.14-1";
    const SLOT: &str = "forecast:2026-09-14T06:00:00.000Z:1";
    const POLICY_JSON: &str = r#"{"contractVersion":"rain-prospective-capture/v1"}"#;
    const POLICY_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    type Trace = Rc<RefCell<Vec<&'static str>>>;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct FakeError;

    struct FakePool {
        gate: Option<RainCaptureClaimGateRow>,
        insert_id: Option<String>,
        now_ms: Option<i64>,
        trace: Trace,
    }

    impl RainCaptureClaimPool for FakePool {
        type Error = FakeError;
        type Session = FakeSession;

        /// Check out one fixture session with the configured statement results.
        fn checkout(&mut self) -> Result<Self::Session, Self::Error> {
            self.trace.borrow_mut().push("checkout");
            Ok(FakeSession {
                gate: self.gate,
                insert_id: self.insert_id.clone(),
                now_ms: self.now_ms,
                trace: Rc::clone(&self.trace),
            })
        }
    }

    struct FakeSession {
        gate: Option<RainCaptureClaimGateRow>,
        insert_id: Option<String>,
        now_ms: Option<i64>,
        trace: Trace,
    }

    impl RainCaptureClaimSession for FakeSession {
        type Error = FakeError;

        /// Record transaction start.
        fn begin(&mut self) -> Result<(), Self::Error> {
            self.trace.borrow_mut().push("begin");
            Ok(())
        }

        /// Require the exact advisory transaction lock.
        fn advisory_lock(
            &mut self,
            statement: &RainCaptureClaimLockStatement,
        ) -> Result<(), Self::Error> {
            assert_eq!(statement.sql, CLAIM_LOCK_SQL);
            assert_eq!(statement.lock_id, CLAIM_LOCK);
            self.trace.borrow_mut().push("lock");
            Ok(())
        }

        /// Return one stable database clock.
        fn server_clock(
            &mut self,
            statement: &RainCaptureClaimClockStatement,
        ) -> Result<Option<i64>, Self::Error> {
            assert_eq!(statement.sql, SERVER_CLOCK_SQL);
            self.trace.borrow_mut().push("clock");
            Ok(self.now_ms)
        }

        /// Assert exact gate SQL and bind order.
        fn claim_gate(
            &mut self,
            statement: &RainCaptureClaimGateStatement,
        ) -> Result<Option<RainCaptureClaimGateRow>, Self::Error> {
            assert_eq!(statement.sql, CLAIM_GATE_SQL);
            assert_eq!(
                statement.binds,
                RainCaptureClaimGateBinds {
                    slot_key: SLOT.to_owned(),
                    kind: RainCaptureClaimKind::Forecast,
                    now: NOW.to_owned(),
                    day_start: DAY_START.to_owned(),
                    run_initialized_at: Some(INITIALIZED.to_owned()),
                }
            );
            self.trace.borrow_mut().push("gate");
            Ok(self.gate)
        }

        /// Assert exact immutable claim insert material.
        fn insert_claim(
            &mut self,
            statement: &RainCaptureClaimInsertStatement,
        ) -> Result<Option<String>, Self::Error> {
            assert_eq!(statement.sql, INSERT_CLAIM_SQL);
            assert_eq!(
                statement.binds,
                RainCaptureClaimInsertBinds {
                    kind: RainCaptureClaimKind::Forecast,
                    slot_key: SLOT.to_owned(),
                    station_id: None,
                    run_initialized_at: Some(INITIALIZED.to_owned()),
                    attempt: Some(1),
                    window_start: None,
                    window_end_exclusive: None,
                    release: RELEASE.to_owned(),
                    policy_json: POLICY_JSON.to_owned(),
                    policy_sha256: POLICY_SHA.to_owned(),
                }
            );
            self.trace.borrow_mut().push("insert");
            Ok(self.insert_id.clone())
        }

        /// Record transaction commit.
        fn commit(&mut self) -> Result<(), Self::Error> {
            self.trace.borrow_mut().push("commit");
            Ok(())
        }

        /// Record transaction rollback.
        fn rollback(&mut self) -> Result<(), Self::Error> {
            self.trace.borrow_mut().push("rollback");
            Ok(())
        }

        /// Record unconditional retained-client release.
        fn release(&mut self) {
            self.trace.borrow_mut().push("release");
        }
    }

    struct FakeCalendar;

    impl RainCaptureClaimCalendar for FakeCalendar {
        type Error = FakeError;

        /// Parse only the fixture policy and cycle instants.
        fn parse_utc(&mut self, value: &str) -> Result<i64, Self::Error> {
            Ok(match value {
                "2026-09-13T00:00:00.000Z" => 1_789_257_600_000,
                "2027-10-08T00:00:00.000Z" => 1_822_953_600_000,
                INITIALIZED => INITIALIZED_MS,
                _ => panic!("unexpected fixture UTC parse: {value}"),
            })
        }

        /// Format only the server and UTC day-start instants.
        fn format_utc(&mut self, milliseconds: i64) -> Result<String, Self::Error> {
            Ok(match milliseconds {
                NOW_MS => NOW,
                DAY_START_MS => DAY_START,
                _ => panic!("unexpected fixture UTC format: {milliseconds}"),
            }
            .to_owned())
        }

        /// Admit only the immutable fixture release.
        fn validate_release(&mut self, release: &str) -> Result<bool, Self::Error> {
            Ok(release == RELEASE)
        }
    }

    /// Execute one exact admitted forecast claim through commit and release.
    #[test]
    fn claims_one_forecast_slot_in_server_transaction_order() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut pool = FakePool {
            gate: Some(RainCaptureClaimGateRow::default()),
            insert_id: Some("claim-1".to_owned()),
            now_ms: Some(NOW_MS),
            trace: Rc::clone(&trace),
        };
        let claim = claim_rain_capture_slot(
            &mut pool,
            &mut FakeCalendar,
            forecast_request(SLOT),
            RELEASE,
            admitted_policy(),
        )
        .expect("eligible fixture should claim");

        assert_eq!(claim.as_deref(), Some("claim-1"));
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "checkout", "begin", "lock", "clock", "gate", "insert", "commit", "release",
            ]
        );
    }

    /// Commit and release an ordinary no-row gate denial without inserting.
    #[test]
    fn denies_a_missing_gate_row_without_inserting() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut pool = FakePool {
            gate: None,
            insert_id: Some("must-not-insert".to_owned()),
            now_ms: Some(NOW_MS),
            trace: Rc::clone(&trace),
        };
        let claim = claim_rain_capture_slot(
            &mut pool,
            &mut FakeCalendar,
            forecast_request(SLOT),
            RELEASE,
            admitted_policy(),
        )
        .expect("missing gate row should be an ordinary denial");

        assert_eq!(claim, None);
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "checkout", "begin", "lock", "clock", "gate", "commit", "release"
            ]
        );
    }

    /// Roll back and release when the server-clock request domain guard refuses a slot.
    #[test]
    fn rolls_back_an_invalid_slot_before_gate_query() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut pool = FakePool {
            gate: Some(RainCaptureClaimGateRow::default()),
            insert_id: None,
            now_ms: Some(NOW_MS),
            trace: Rc::clone(&trace),
        };
        let failure = claim_rain_capture_slot(
            &mut pool,
            &mut FakeCalendar,
            forecast_request("forecast:wrong"),
            RELEASE,
            admitted_policy(),
        )
        .expect_err("wrong slot identity should fail");

        assert_eq!(failure, RainCaptureClaimError::ForecastSlotOutsidePolicy);
        assert_eq!(
            trace.borrow().as_slice(),
            ["checkout", "begin", "lock", "clock", "rollback", "release"]
        );
    }

    /// Build one borrowed forecast request.
    fn forecast_request(slot_key: &'static str) -> RainCaptureClaimRequest<'static> {
        RainCaptureClaimRequest::Forecast {
            attempt: 1,
            run_initialized_at: Cow::Borrowed(INITIALIZED),
            slot_key: Cow::Borrowed(slot_key),
        }
    }

    /// Return the approved immutable policy fixture.
    const fn admitted_policy() -> AdmittedRainCaptureClaimPolicy<'static> {
        AdmittedRainCaptureClaimPolicy {
            json: POLICY_JSON,
            sha256: POLICY_SHA,
        }
    }
}
