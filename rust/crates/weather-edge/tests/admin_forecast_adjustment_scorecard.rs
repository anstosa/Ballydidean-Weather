use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use weather_edge::{
    AdmittedAdminOperator, AdmittedForecastAdjustmentScorecardJson,
    AdmittedForecastAdjustmentScorecardNow, AdmittedForecastAdjustmentScorecardResponseJson,
    AdmittedForecastAdjustmentScorecardSnapshot, ForecastAdjustmentScorecardAuthority,
    ForecastAdjustmentScorecardBody, ForecastAdjustmentScorecardCandidates,
    ForecastAdjustmentScorecardClock, ForecastAdjustmentScorecardContractVersion,
    ForecastAdjustmentScorecardEncoder, ForecastAdjustmentScorecardMethod,
    ForecastAdjustmentScorecardPublicationState, ForecastAdjustmentScorecardRequest,
    ForecastAdjustmentScorecardResponse, ForecastAdjustmentScorecardStore,
    handle_forecast_adjustment_scorecard,
};

const GENERATED_AT_EPOCH_MS: i64 = 1_791_334_800_000;
const NOW_EPOCH_MS: i64 = 1_791_336_600_000;
const V2_VALID_THROUGH_EPOCH_MS: i64 = 1_791_939_600_000;
const V1_VALID_THROUGH_EPOCH_MS: i64 = 4_095_104_400_000;
const NOW: &str = "2026-10-07T01:30:00.000Z";

#[derive(Debug, Clone)]
struct FixtureClock {
    fail: Arc<Mutex<bool>>,
    now: Arc<Mutex<AdmittedForecastAdjustmentScorecardNow>>,
    trace: Arc<Mutex<Vec<&'static str>>>,
}

impl FixtureClock {
    /// create one fixed admitted scorecard read time.
    fn fixed(trace: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            fail: Arc::new(Mutex::new(false)),
            now: Arc::new(Mutex::new(AdmittedForecastAdjustmentScorecardNow {
                canonical: NOW.to_owned(),
                epoch_ms: NOW_EPOCH_MS,
            })),
            trace,
        }
    }

    /// replace the current admitted scorecard read time.
    fn set(&self, canonical: &str, epoch_ms: i64) {
        *self.now.lock().expect("clock lock") = AdmittedForecastAdjustmentScorecardNow {
            canonical: canonical.to_owned(),
            epoch_ms,
        };
    }

    /// select whether canonical time resolution fails.
    fn set_fail(&self, fail: bool) {
        *self.fail.lock().expect("clock lock") = fail;
    }
}

impl ForecastAdjustmentScorecardClock for FixtureClock {
    type Error = ();

    /// return one admitted scorecard read time after recording its ordering point.
    fn now(&self) -> Result<AdmittedForecastAdjustmentScorecardNow, Self::Error> {
        self.trace.lock().expect("trace lock").push("clock");

        // preserve the injected time-source result
        if *self.fail.lock().expect("clock lock") {
            Err(())
        } else {
            Ok(self.now.lock().expect("clock lock").clone())
        }
    }
}

#[derive(Debug, Clone)]
struct FixtureStore {
    candidates: Arc<Mutex<ForecastAdjustmentScorecardCandidates>>,
    fail: Arc<Mutex<bool>>,
    trace: Arc<Mutex<Vec<&'static str>>>,
}

impl FixtureStore {
    /// create one coherent admitted candidate store.
    fn new(
        candidates: ForecastAdjustmentScorecardCandidates,
        trace: Arc<Mutex<Vec<&'static str>>>,
    ) -> Self {
        Self {
            candidates: Arc::new(Mutex::new(candidates)),
            fail: Arc::new(Mutex::new(false)),
            trace,
        }
    }

    /// replace the complete coherent candidate set.
    fn set_candidates(&self, candidates: ForecastAdjustmentScorecardCandidates) {
        *self.candidates.lock().expect("store lock") = candidates;
    }

    /// select whether the verified slot read fails.
    fn set_fail(&self, fail: bool) {
        *self.fail.lock().expect("store lock") = fail;
    }
}

impl ForecastAdjustmentScorecardStore for FixtureStore {
    type Error = ();

    /// return the current coherent admitted candidate set.
    fn read_candidates(
        &self,
        now: &AdmittedForecastAdjustmentScorecardNow,
    ) -> Result<ForecastAdjustmentScorecardCandidates, Self::Error> {
        self.trace.lock().expect("trace lock").push("store");
        assert!(!now.canonical.is_empty());

        // preserve the injected private-store result
        if *self.fail.lock().expect("store lock") {
            Err(())
        } else {
            Ok(self.candidates.lock().expect("store lock").clone())
        }
    }
}

#[derive(Debug, Clone)]
struct FixtureEncoder {
    current: Vec<u8>,
    fail: Arc<Mutex<bool>>,
    legacy: Vec<u8>,
    legacy_scorecard: Vec<u8>,
    pending: Vec<u8>,
    trace: Arc<Mutex<Vec<&'static str>>>,
    v2_scorecard: Vec<u8>,
}

impl FixtureEncoder {
    /// create one encoder from actual compiled Node response bytes.
    fn new(node: &str, trace: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            current: oracle_value(node, "body-current=").as_bytes().to_vec(),
            fail: Arc::new(Mutex::new(false)),
            legacy: oracle_value(node, "body-legacy=").as_bytes().to_vec(),
            legacy_scorecard: oracle_value(node, "scorecard-legacy=").as_bytes().to_vec(),
            pending: oracle_value(node, "body-pending=").as_bytes().to_vec(),
            trace,
            v2_scorecard: oracle_value(node, "scorecard-v2=").as_bytes().to_vec(),
        }
    }

    /// select whether response encoding fails.
    fn set_fail(&self, fail: bool) {
        *self.fail.lock().expect("encoder lock") = fail;
    }
}

impl ForecastAdjustmentScorecardEncoder for FixtureEncoder {
    type Error = ();

    /// return the actual Node response envelope for the selected source slot.
    fn encode_response(
        &self,
        snapshot: &AdmittedForecastAdjustmentScorecardSnapshot,
        publication_state: ForecastAdjustmentScorecardPublicationState,
    ) -> Result<AdmittedForecastAdjustmentScorecardResponseJson, Self::Error> {
        self.trace.lock().expect("trace lock").push("encode");

        // preserve the injected canonical encoding failure
        if *self.fail.lock().expect("encoder lock") {
            return Err(());
        }
        let expected_scorecard = match snapshot.contract_version() {
            ForecastAdjustmentScorecardContractVersion::V1 => &self.legacy_scorecard,
            ForecastAdjustmentScorecardContractVersion::V2 => &self.v2_scorecard,
        };
        assert_eq!(snapshot.scorecard().as_bytes(), expected_scorecard);
        let content = match publication_state {
            ForecastAdjustmentScorecardPublicationState::Current => &self.current,
            ForecastAdjustmentScorecardPublicationState::LegacyDisplay => &self.legacy,
            ForecastAdjustmentScorecardPublicationState::PendingUnapplied => &self.pending,
        };
        Ok(AdmittedForecastAdjustmentScorecardResponseJson::admitted(
            content.clone(),
        ))
    }
}

/// extract one exact value from the compiled Node oracle.
fn oracle_value<'a>(oracle: &'a str, prefix: &str) -> &'a str {
    oracle
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("missing oracle line: {prefix}"))
}

/// build one admitted scorecard snapshot for a contract generation.
fn snapshot(
    version: ForecastAdjustmentScorecardContractVersion,
    scorecard: &[u8],
    generated_at_epoch_ms: i64,
    valid_through_epoch_ms: i64,
) -> AdmittedForecastAdjustmentScorecardSnapshot {
    AdmittedForecastAdjustmentScorecardSnapshot::admitted(
        version,
        generated_at_epoch_ms,
        AdmittedForecastAdjustmentScorecardJson::admitted(scorecard.to_vec()),
        valid_through_epoch_ms,
    )
}

/// build a candidate set containing one legacy publication.
fn legacy_candidates(scorecard: &[u8]) -> ForecastAdjustmentScorecardCandidates {
    ForecastAdjustmentScorecardCandidates {
        current_v2: None,
        legacy: Some(snapshot(
            ForecastAdjustmentScorecardContractVersion::V1,
            scorecard,
            GENERATED_AT_EPOCH_MS,
            V1_VALID_THROUGH_EPOCH_MS,
        )),
        pending_v2: None,
        previous_v2_present: false,
    }
}

/// build a candidate set containing one selected v2 publication.
fn v2_candidates(scorecard: &[u8], pending: bool) -> ForecastAdjustmentScorecardCandidates {
    let snapshot = snapshot(
        ForecastAdjustmentScorecardContractVersion::V2,
        scorecard,
        GENERATED_AT_EPOCH_MS,
        V2_VALID_THROUGH_EPOCH_MS,
    );
    ForecastAdjustmentScorecardCandidates {
        current_v2: if pending {
            None
        } else {
            Some(snapshot.clone())
        },
        legacy: None,
        pending_v2: if pending { Some(snapshot) } else { None },
        previous_v2_present: false,
    }
}

/// build one scorecard request after external session admission.
fn request<'a>(
    authority: ForecastAdjustmentScorecardAuthority<'a>,
    method: ForecastAdjustmentScorecardMethod,
    has_query_delimiter: bool,
) -> ForecastAdjustmentScorecardRequest<'a> {
    ForecastAdjustmentScorecardRequest {
        authority,
        has_query_delimiter,
        method,
    }
}

/// render one response like the actual Node route recorder.
fn response_signature(
    response: &ForecastAdjustmentScorecardResponse,
    observed_now: &str,
) -> String {
    let body = match &response.body {
        ForecastAdjustmentScorecardBody::Json(None) => "none",
        ForecastAdjustmentScorecardBody::Json(Some(_))
        | ForecastAdjustmentScorecardBody::Text(_) => "body",
    };
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        response.status_code,
        response.headers.cache_control,
        response.headers.content_type,
        response.headers.content_length,
        response.headers.allow.unwrap_or("-"),
        response.headers.retry_after.unwrap_or("-"),
        body,
        observed_now
    )
}

/// execute one request after clearing its operation trace.
fn execute(
    request: ForecastAdjustmentScorecardRequest<'_>,
    clock: &FixtureClock,
    store: &FixtureStore,
    encoder: &FixtureEncoder,
    trace: &Arc<Mutex<Vec<&'static str>>>,
) -> (ForecastAdjustmentScorecardResponse, Vec<&'static str>) {
    trace.lock().expect("trace lock").clear();
    let response = handle_forecast_adjustment_scorecard(request, clock, store, encoder);
    let observed = trace.lock().expect("trace lock").clone();
    (response, observed)
}

// verifies private scorecard route and publication selection against actual Node behavior
#[test]
fn matches_compiled_node_admin_scorecard_route_and_selection() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/admin-forecast-adjustment-scorecard-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run admin scorecard Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let trace = Arc::new(Mutex::new(Vec::new()));
    let clock = FixtureClock::fixed(Arc::clone(&trace));
    let encoder = FixtureEncoder::new(&node, Arc::clone(&trace));
    let store = FixtureStore::new(
        legacy_candidates(&encoder.legacy_scorecard),
        Arc::clone(&trace),
    );
    let operator = AdmittedAdminOperator::admitted("fixture-operator");
    let admitted = ForecastAdjustmentScorecardAuthority::Admitted(&operator);

    let early_cases = [
        (
            "empty-query",
            request(admitted, ForecastAdjustmentScorecardMethod::Get, true),
        ),
        (
            "query",
            request(admitted, ForecastAdjustmentScorecardMethod::Get, true),
        ),
        (
            "unsupported",
            request(
                admitted,
                ForecastAdjustmentScorecardMethod::Unsupported,
                false,
            ),
        ),
    ];

    // compare admitted empty-query, query, and method precedence before store work
    for (name, request) in early_cases {
        let (response, observed) = execute(request, &clock, &store, &encoder, &trace);
        assert!(observed.is_empty(), "{name}");
        assert_eq!(
            response_signature(&response, "-"),
            oracle_value(&node, &format!("{name}=")),
            "{name}"
        );
    }

    let (legacy, observed) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(observed, vec!["clock", "store", "encode"]);
    assert_eq!(
        response_signature(&legacy, NOW),
        oracle_value(&node, "legacy=")
    );
    assert_eq!(
        legacy.body,
        ForecastAdjustmentScorecardBody::Json(Some(
            [oracle_value(&node, "body-legacy=").as_bytes(), b"\n"].concat()
        ))
    );

    let (legacy_head, _) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Head, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(
        response_signature(&legacy_head, NOW),
        oracle_value(&node, "legacy-head=")
    );
    assert_eq!(
        legacy_head.body,
        ForecastAdjustmentScorecardBody::Json(None)
    );

    store.set_candidates(v2_candidates(&encoder.v2_scorecard, true));
    let (pending, _) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(
        response_signature(&pending, NOW),
        oracle_value(&node, "pending=")
    );
    assert_eq!(
        pending.body,
        ForecastAdjustmentScorecardBody::Json(Some(
            [oracle_value(&node, "body-pending=").as_bytes(), b"\n"].concat()
        ))
    );

    store.set_candidates(v2_candidates(&encoder.v2_scorecard, false));
    let (current, _) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(
        response_signature(&current, NOW),
        oracle_value(&node, "current=")
    );
    assert_eq!(
        ForecastAdjustmentScorecardPublicationState::Current.wire_value(),
        "current"
    );

    let mut priority = v2_candidates(&encoder.v2_scorecard, true);
    priority.current_v2 = Some(snapshot(
        ForecastAdjustmentScorecardContractVersion::V2,
        &encoder.v2_scorecard,
        GENERATED_AT_EPOCH_MS,
        V2_VALID_THROUGH_EPOCH_MS,
    ));
    priority.legacy = Some(snapshot(
        ForecastAdjustmentScorecardContractVersion::V1,
        &encoder.legacy_scorecard,
        GENERATED_AT_EPOCH_MS,
        V1_VALID_THROUGH_EPOCH_MS,
    ));
    store.set_candidates(priority);
    let (pending_priority, _) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(pending_priority.status_code, 503);

    let mut pending_over_legacy = v2_candidates(&encoder.v2_scorecard, true);
    pending_over_legacy.legacy = Some(snapshot(
        ForecastAdjustmentScorecardContractVersion::V1,
        &encoder.legacy_scorecard,
        GENERATED_AT_EPOCH_MS,
        V1_VALID_THROUGH_EPOCH_MS,
    ));
    store.set_candidates(pending_over_legacy);
    let (pending_priority, _) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(pending_priority.status_code, 200);
    assert_eq!(pending_priority.body, pending.body);

    let mut forbidden = legacy_candidates(&encoder.legacy_scorecard);
    forbidden.previous_v2_present = true;
    store.set_candidates(forbidden);
    let (forbidden, observed) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(forbidden.status_code, 503);
    assert_eq!(observed, vec!["clock", "store"]);

    let mismatch = ForecastAdjustmentScorecardCandidates {
        current_v2: None,
        legacy: None,
        pending_v2: Some(snapshot(
            ForecastAdjustmentScorecardContractVersion::V1,
            &encoder.legacy_scorecard,
            GENERATED_AT_EPOCH_MS,
            V1_VALID_THROUGH_EPOCH_MS,
        )),
        previous_v2_present: false,
    };
    store.set_candidates(mismatch);
    let (mismatch, _) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(mismatch.status_code, 503);

    let expired_snapshot = snapshot(
        ForecastAdjustmentScorecardContractVersion::V1,
        &encoder.legacy_scorecard,
        GENERATED_AT_EPOCH_MS,
        NOW_EPOCH_MS,
    );
    store.set_candidates(ForecastAdjustmentScorecardCandidates {
        current_v2: None,
        legacy: Some(expired_snapshot),
        pending_v2: None,
        previous_v2_present: false,
    });
    let (expired, _) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(
        response_signature(&expired, NOW),
        oracle_value(&node, "expired=")
    );

    clock.set("2026-10-07T01:29:59.999Z", NOW_EPOCH_MS - 1);
    let (before_expiry, _) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(before_expiry.status_code, 200);
    clock.set(NOW, NOW_EPOCH_MS);

    store.set_candidates(ForecastAdjustmentScorecardCandidates {
        current_v2: Some(snapshot(
            ForecastAdjustmentScorecardContractVersion::V2,
            &encoder.v2_scorecard,
            GENERATED_AT_EPOCH_MS,
            V2_VALID_THROUGH_EPOCH_MS + 1,
        )),
        legacy: None,
        pending_v2: None,
        previous_v2_present: false,
    });
    let (overlong, _) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(overlong.status_code, 503);

    store.set_fail(true);
    let (unavailable, _) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(
        response_signature(&unavailable, NOW),
        oracle_value(&node, "unavailable=")
    );
    store.set_fail(false);

    store.set_candidates(legacy_candidates(&encoder.legacy_scorecard));
    encoder.set_fail(true);
    let (encode_failure, observed) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(encode_failure.status_code, 503);
    assert_eq!(observed, vec!["clock", "store", "encode"]);
    encoder.set_fail(false);

    clock.set_fail(true);
    let (clock_failure, observed) = execute(
        request(admitted, ForecastAdjustmentScorecardMethod::Get, false),
        &clock,
        &store,
        &encoder,
        &trace,
    );
    assert_eq!(clock_failure.status_code, 503);
    assert_eq!(observed, vec!["clock"]);
}
