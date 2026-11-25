//! Execute the typed PurpleAir public-map token and history operation.

use weather_database::Sha256Digest;
use weather_providers::{
    DEFAULT_PROVIDER_MAX_BODY_BYTES, MAX_PROVIDER_BODY_BYTES, ProviderControls, ProviderIo,
    ProviderRequestPolicy, run_provider_request,
};
use weather_worker::{
    PurpleAirDecodedHistory, ScheduledWindow, SupplementalPublicStationBatch,
    SupplementalPublicStationIdentity, SupplementalPublicStationProduct,
    SupplementalPublicStationScheduledRequest, TaskFailure,
    normalize_supplemental_public_station_batch_with_diagnostics,
};

use crate::{
    SupplementalPublicStationOperationError, WorkerPublicStationEvidence,
    validate_worker_public_station_range, worker_public_station_digest,
};

const MAX_JS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAXIMUM_PURPLEAIR_RANGE_MILLISECONDS: i64 = 2 * 24 * 60 * 60 * 1_000;
const PURPLEAIR_TOKEN_CACHE_MILLISECONDS: i64 = 10 * 60 * 1_000;

/// Exact ordered fields requested from the PurpleAir map-history endpoint.
pub const WORKER_PURPLEAIR_HISTORY_FIELDS: [&str; 8] = [
    "humidity_a",
    "humidity_b",
    "pm2.5_atm_a",
    "pm2.5_atm_b",
    "pressure_a",
    "pressure_b",
    "temperature_a",
    "temperature_b",
];

/// One typed public-map token request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerPurpleAirTokenPlan {
    pub map_version: String,
}

/// One successful public-map token response before trimming and admission.
#[derive(Clone, Eq, PartialEq)]
pub struct WorkerPurpleAirTokenProduct {
    pub body: String,
}

/// One typed history request without its public-map token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerPurpleAirHistoryPlan {
    pub average: u8,
    pub fields: [&'static str; 8],
    pub sensor_index: u64,
    pub window: ScheduledWindow,
}

/// One decoded history response with its admitted raw-response digest.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkerPurpleAirHistoryProduct {
    pub checksum: Sha256Digest,
    pub history: PurpleAirDecodedHistory,
}

/// Request-bound token and history transport ownership with independent clocks.
pub trait WorkerPurpleAirTransport {
    type TokenExchange<'a>: ProviderIo<WorkerPurpleAirTokenProduct>
    where
        Self: 'a;
    type HistoryExchange<'a>: ProviderIo<WorkerPurpleAirHistoryProduct>
    where
        Self: 'a;

    /// Bind one versioned public-map token request with the fixed origin and referrer headers.
    fn bind_token(
        &mut self,
        plan: WorkerPurpleAirTokenPlan,
    ) -> Result<Self::TokenExchange<'_>, TaskFailure>;

    /// Bind one history request while keeping the public token outside diagnostic material.
    fn bind_history(
        &mut self,
        token: String,
        plan: WorkerPurpleAirHistoryPlan,
    ) -> Result<Self::HistoryExchange<'_>, TaskFailure>;

    /// Report whether this is the production transport eligible for shared token reuse.
    fn purpleair_production_cache_enabled(&self) -> bool;

    /// Sample the cache epoch independently from request receipt time.
    fn purpleair_cache_epoch_ms(&mut self) -> i64;

    /// Sample receipt time once after the history response succeeds.
    fn purpleair_received_at_ms(&mut self) -> i64;
}

struct PurpleAirTokenCache {
    expires_at_ms: i64,
    map_version: String,
    token: String,
}

/// Stateful PurpleAir operation retaining only an exact-version production token cache.
pub struct WorkerPurpleAirOperation<Transport> {
    token_cache: Option<PurpleAirTokenCache>,
    transport: Transport,
}

impl<Transport> WorkerPurpleAirOperation<Transport> {
    /// Create one operation with an empty production token cache.
    pub fn new(transport: Transport) -> Self {
        Self {
            token_cache: None,
            transport,
        }
    }
}

impl<Transport: WorkerPurpleAirTransport> WorkerPurpleAirOperation<Transport> {
    /// Fetch, evidence, and normalize one admitted PurpleAir public-station range.
    pub fn fetch(
        &mut self,
        evidence: &mut impl WorkerPublicStationEvidence,
        request: &SupplementalPublicStationScheduledRequest,
        map_version: &str,
        sensor_index: u64,
        policy: ProviderRequestPolicy,
    ) -> Result<SupplementalPublicStationBatch, TaskFailure> {
        self.fetch_operation(evidence, request, map_version, sensor_index, policy)
            .map_err(SupplementalPublicStationOperationError::into_task_failure)
    }

    /// Retain classified provider controller failures for a one-shot archive owner.
    pub(crate) fn fetch_operation(
        &mut self,
        evidence: &mut impl WorkerPublicStationEvidence,
        request: &SupplementalPublicStationScheduledRequest,
        map_version: &str,
        sensor_index: u64,
        policy: ProviderRequestPolicy,
    ) -> Result<SupplementalPublicStationBatch, SupplementalPublicStationOperationError> {
        validate_map_version(map_version)?;
        validate_identity(request, map_version, sensor_index)?;
        let (token, token_attempts) = self.fetch_token(map_version, policy)?;
        validate_worker_public_station_range(request.window, MAXIMUM_PURPLEAIR_RANGE_MILLISECONDS)?;

        // require the configured positive safe integer before binding history traffic
        if sensor_index == 0 || sensor_index > MAX_JS_SAFE_INTEGER {
            return Err(invalid_payload(0).into());
        }

        let history_plan = WorkerPurpleAirHistoryPlan {
            average: 0,
            fields: WORKER_PURPLEAIR_HISTORY_FIELDS,
            sensor_index,
            window: request.window,
        };
        let history_policy = policy_with_body_limit(policy, MAX_PROVIDER_BODY_BYTES);
        let history_response = {
            let mut history = self.transport.bind_history(token, history_plan)?;
            run_provider_request(history_policy, &mut history)?
        };
        let received_at_ms = self.transport.purpleair_received_at_ms();
        let attempts = token_attempts + u32::from(history_response.attempts);
        let mut batch = normalize_supplemental_public_station_batch_with_diagnostics(
            request.normalization_inputs(),
            SupplementalPublicStationProduct::PurpleAir {
                sensor_index,
                history: &history_response.body.history,
            },
            received_at_ms,
            attempts,
            String::new(),
        )?;
        batch.upstream_response_checksum =
            worker_public_station_digest(evidence, &[&history_response.body.checksum])?;
        Ok(batch)
    }

    /// Reuse or fetch one admitted exact-version public-map token.
    fn fetch_token(
        &mut self,
        map_version: &str,
        policy: ProviderRequestPolicy,
    ) -> Result<(String, u32), SupplementalPublicStationOperationError> {
        let cache_enabled = self.transport.purpleair_production_cache_enabled();
        let cached = self
            .token_cache
            .as_ref()
            .filter(|cached| cached.map_version == map_version)
            .map(|cached| (cached.expires_at_ms, cached.token.clone()));

        // reuse only an unexpired exact-version production token
        if cache_enabled
            && let Some((expires_at_ms, token)) = cached
            && expires_at_ms > self.transport.purpleair_cache_epoch_ms()
        {
            return Ok((token, 0));
        }

        let token_policy = policy_with_body_limit(policy, DEFAULT_PROVIDER_MAX_BODY_BYTES);
        let response = {
            let mut token = self.transport.bind_token(WorkerPurpleAirTokenPlan {
                map_version: map_version.to_owned(),
            })?;
            run_provider_request(token_policy, &mut token)?
        };
        let attempts = u32::from(response.attempts);
        let token = admit_token(&response.body.body)?;

        // cache only production transport results using its independent epoch clock
        if cache_enabled {
            let expires_at_ms = self
                .transport
                .purpleair_cache_epoch_ms()
                .checked_add(PURPLEAIR_TOKEN_CACHE_MILLISECONDS)
                .ok_or_else(|| provider_unavailable(attempts))?;
            self.token_cache = Some(PurpleAirTokenCache {
                expires_at_ms,
                map_version: map_version.to_owned(),
                token: token.clone(),
            });
        }

        Ok((token, attempts))
    }
}

/// Bind the operation arguments to the exact catalog identity retained on the request.
fn validate_identity(
    request: &SupplementalPublicStationScheduledRequest,
    map_version: &str,
    sensor_index: u64,
) -> Result<(), TaskFailure> {
    let SupplementalPublicStationIdentity::PurpleAir {
        map_version: expected_map_version,
        sensor_index: expected_sensor_index,
    } = &request.identity
    else {
        return Err(invalid_payload(0));
    };

    // reject arguments that do not match the locked configured source
    if map_version != expected_map_version || sensor_index != *expected_sensor_index {
        return Err(invalid_payload(0));
    }

    Ok(())
}

/// Admit the exact dotted numeric map version required by the public-map service.
fn validate_map_version(map_version: &str) -> Result<(), TaskFailure> {
    let mut parts = map_version.split('.');

    // require exactly three nonempty ASCII-numeric version components
    for _ in 0..3 {
        let Some(part) = parts.next() else {
            return Err(provider_unavailable(0));
        };
        // reject empty or nonnumeric version components
        if part.is_empty() || !part.bytes().all(|value| value.is_ascii_digit()) {
            return Err(provider_unavailable(0));
        }
    }

    // reject trailing version components
    if parts.next().is_some() {
        return Err(provider_unavailable(0));
    }

    Ok(())
}

/// Trim and admit one bounded ASCII public-map token without exposing it to evidence.
fn admit_token(body: &str) -> Result<String, SupplementalPublicStationOperationError> {
    let token = body.trim();

    // reject HTML and malformed public token responses
    if !(64..=256).contains(&token.len())
        || !token
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'+' | b'/' | b'='))
    {
        return Err(SupplementalPublicStationOperationError::invalid_payload(
            1,
            1,
            "PurpleAir map token response is invalid",
        ));
    }

    Ok(token.to_owned())
}

/// Preserve the shared absolute deadline while selecting the Node response cap.
const fn policy_with_body_limit(
    policy: ProviderRequestPolicy,
    max_body_bytes: u64,
) -> ProviderRequestPolicy {
    ProviderRequestPolicy {
        controls: ProviderControls {
            max_body_bytes,
            ..policy.controls
        },
        deadline_at_ms: policy.deadline_at_ms,
    }
}

/// Return one typed invalid-payload failure with physical attempts retained.
fn invalid_payload(attempts: u32) -> TaskFailure {
    TaskFailure {
        attempts,
        code: "invalid_payload".to_owned(),
    }
}

/// Return one typed provider failure for an unusable approved transport clock.
fn provider_unavailable(attempts: u32) -> TaskFailure {
    TaskFailure {
        attempts,
        code: "provider_unavailable".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::path::Path;
    use std::process::Command;
    use std::rc::Rc;

    use weather_providers::{
        ContentBudget, ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse,
        ProviderSleep, ProviderSleepError,
    };

    use super::*;

    const HISTORY_SHA_ONE: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HISTORY_SHA_TWO: &str =
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const HISTORY_SHA_THREE: &str =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const COMBINED_SHA: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const MAP_VERSION: &str = "3.6.1";
    const SENSOR_INDEX: u64 = 42;
    const START_MS: i64 = 1_800_000_000_000;

    struct Exchange<Product> {
        attempts: Rc<RefCell<Vec<ProviderAttempt>>>,
        now_ms: i64,
        product: Product,
        statuses: VecDeque<u16>,
    }

    impl<Product: Clone> ProviderIo<Product> for Exchange<Product> {
        /// Return the deterministic controller clock.
        fn now_ms(&mut self) -> i64 {
            self.now_ms
        }

        /// Select the deterministic minimum retry jitter.
        fn random_unit(&mut self) -> f64 {
            0.0
        }

        /// Retain one physical attempt and return the approved decoded product.
        fn execute(
            &mut self,
            attempt: ProviderAttempt,
        ) -> Result<ProviderAttemptResponse<Product>, ProviderAttemptError> {
            self.attempts.borrow_mut().push(attempt);
            Ok(ProviderAttemptResponse {
                body: self.product.clone(),
                content: ContentBudget {
                    declared_body_bytes: Some(128),
                    streamed_body_bytes: 128,
                },
                provider_reason: None,
                retry_after_ms: None,
                status: self
                    .statuses
                    .pop_front()
                    .expect("fixture response status must exist"),
            })
        }

        /// Advance only the exchange clock for one bounded retry delay.
        fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
            self.now_ms += i64::try_from(sleep.delay_ms).expect("fixture delay must fit i64");
            Ok(())
        }
    }

    struct Transport {
        cache_epochs: VecDeque<i64>,
        history_bindings: Vec<WorkerPurpleAirHistoryPlan>,
        history_exchanges: VecDeque<Exchange<WorkerPurpleAirHistoryProduct>>,
        receipt_calls: usize,
        token_bindings: Vec<WorkerPurpleAirTokenPlan>,
        token_exchanges: VecDeque<Exchange<WorkerPurpleAirTokenProduct>>,
    }

    impl WorkerPurpleAirTransport for Transport {
        type TokenExchange<'a> = Exchange<WorkerPurpleAirTokenProduct>;
        type HistoryExchange<'a> = Exchange<WorkerPurpleAirHistoryProduct>;

        /// Retain the public version request and return the next token exchange.
        fn bind_token(
            &mut self,
            plan: WorkerPurpleAirTokenPlan,
        ) -> Result<Self::TokenExchange<'_>, TaskFailure> {
            self.token_bindings.push(plan);
            self.token_exchanges
                .pop_front()
                .ok_or_else(|| invalid_payload(0))
        }

        /// Verify the unlogged token and retain the noncredential history plan.
        fn bind_history(
            &mut self,
            token: String,
            plan: WorkerPurpleAirHistoryPlan,
        ) -> Result<Self::HistoryExchange<'_>, TaskFailure> {
            assert_eq!(token, "A".repeat(64));
            self.history_bindings.push(plan);
            self.history_exchanges
                .pop_front()
                .ok_or_else(|| invalid_payload(0))
        }

        /// Enable the production-only cache path in this fixture.
        fn purpleair_production_cache_enabled(&self) -> bool {
            true
        }

        /// Return independent cache epochs in exact use order.
        fn purpleair_cache_epoch_ms(&mut self) -> i64 {
            self.cache_epochs
                .pop_front()
                .expect("fixture cache epoch must exist")
        }

        /// Count receipt-clock samples independently from cache epochs.
        fn purpleair_received_at_ms(&mut self) -> i64 {
            self.receipt_calls += 1;
            START_MS + 120_000
        }
    }

    struct Evidence {
        materials: Vec<Vec<u8>>,
    }

    impl WorkerPublicStationEvidence for Evidence {
        /// Retain the exact history-only digest framing.
        fn sha256(&mut self, material: &[u8]) -> Result<Sha256Digest, TaskFailure> {
            self.materials.push(material.to_vec());
            Ok(Sha256Digest::admitted(COMBINED_SHA))
        }
    }

    /// Prove strict cache hits and misses while excluding token bytes from evidence.
    #[test]
    fn caches_only_unexpired_tokens_and_counts_physical_attempts() {
        let token_attempts = Rc::new(RefCell::new(Vec::new()));
        let history_attempts = Rc::new(RefCell::new(Vec::new()));
        let token = WorkerPurpleAirTokenProduct {
            body: format!(" \n{}\t", "A".repeat(64)),
        };
        let mut operation = WorkerPurpleAirOperation::new(Transport {
            cache_epochs: VecDeque::from([1_000, 600_999, 601_000, 601_001]),
            history_bindings: Vec::new(),
            history_exchanges: VecDeque::from([
                history_exchange(&history_attempts, HISTORY_SHA_ONE),
                history_exchange(&history_attempts, HISTORY_SHA_TWO),
                history_exchange(&history_attempts, HISTORY_SHA_THREE),
            ]),
            receipt_calls: 0,
            token_bindings: Vec::new(),
            token_exchanges: VecDeque::from([
                Exchange {
                    attempts: token_attempts.clone(),
                    now_ms: START_MS,
                    product: token.clone(),
                    statuses: VecDeque::from([503, 200]),
                },
                Exchange {
                    attempts: token_attempts.clone(),
                    now_ms: START_MS,
                    product: token,
                    statuses: VecDeque::from([200]),
                },
            ]),
        });
        let mut evidence = Evidence {
            materials: Vec::new(),
        };
        let request = scheduled_request();
        let policy = ProviderRequestPolicy {
            controls: ProviderControls {
                max_attempts: 2,
                max_body_bytes: 128,
                timeout_ms: 1_000,
            },
            deadline_at_ms: None,
        };

        let first = operation
            .fetch(&mut evidence, &request, MAP_VERSION, SENSOR_INDEX, policy)
            .expect("initial token miss should succeed");
        let second = operation
            .fetch(&mut evidence, &request, MAP_VERSION, SENSOR_INDEX, policy)
            .expect("strictly unexpired token should hit cache");
        let third = operation
            .fetch(&mut evidence, &request, MAP_VERSION, SENSOR_INDEX, policy)
            .expect("token at exact expiry should miss cache");

        assert_eq!([first.attempts, second.attempts, third.attempts], [3, 1, 2]);
        assert_eq!(operation.transport.token_bindings.len(), 2);
        assert_eq!(operation.transport.history_bindings.len(), 3);
        assert_eq!(operation.transport.receipt_calls, 3);
        assert_eq!(
            operation.transport.history_bindings,
            vec![
                WorkerPurpleAirHistoryPlan {
                    average: 0,
                    fields: WORKER_PURPLEAIR_HISTORY_FIELDS,
                    sensor_index: SENSOR_INDEX,
                    window: request.window,
                };
                3
            ]
        );
        assert_eq!(
            evidence.materials,
            [
                format!("{HISTORY_SHA_ONE}\n").into_bytes(),
                format!("{HISTORY_SHA_TWO}\n").into_bytes(),
                format!("{HISTORY_SHA_THREE}\n").into_bytes(),
            ]
        );
        let token_attempts = token_attempts.borrow();
        let history_attempts = history_attempts.borrow();
        assert_eq!(token_attempts.len(), 3);
        assert_eq!(history_attempts.len(), 3);
        assert_eq!(
            token_attempts[0].max_body_bytes,
            DEFAULT_PROVIDER_MAX_BODY_BYTES
        );
        assert_eq!(history_attempts[0].max_body_bytes, MAX_PROVIDER_BODY_BYTES);
    }

    /// Match the original compiled provider's exact malformed-token diagnosis.
    #[test]
    fn archive_token_failure_matches_the_node_provider() {
        let token_attempts = Rc::new(RefCell::new(Vec::new()));
        let mut operation = WorkerPurpleAirOperation::new(Transport {
            cache_epochs: VecDeque::new(),
            history_bindings: Vec::new(),
            history_exchanges: VecDeque::new(),
            receipt_calls: 0,
            token_bindings: Vec::new(),
            token_exchanges: VecDeque::from([Exchange {
                attempts: token_attempts.clone(),
                now_ms: START_MS,
                product: WorkerPurpleAirTokenProduct {
                    body: "<html>bad</html>".to_owned(),
                },
                statuses: VecDeque::from([200]),
            }]),
        });
        let mut evidence = Evidence {
            materials: Vec::new(),
        };
        let error = operation
            .fetch_operation(
                &mut evidence,
                &scheduled_request(),
                MAP_VERSION,
                SENSOR_INDEX,
                ProviderRequestPolicy {
                    controls: ProviderControls::default(),
                    deadline_at_ms: None,
                },
            )
            .expect_err("malformed token must retain its diagnosis");
        let SupplementalPublicStationOperationError::InvalidPayload {
            backfill_attempts,
            message,
            scheduled_attempts,
        } = error
        else {
            panic!("expected one exact invalid-payload diagnosis");
        };
        assert_eq!(scheduled_attempts, 1);

        let oracle = Command::new("node")
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/public-station-dry-run-node-oracle.mjs"),
            )
            .arg("--malformed-token")
            .output()
            .expect("compiled Node malformed-token oracle");
        assert!(
            oracle.status.success(),
            "{}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        let oracle_output = String::from_utf8(oracle.stdout).expect("oracle output");
        let expected = oracle_output
            .lines()
            // select the extra malformed-token row after the ordinary planning oracle
            .find_map(|line| line.strip_prefix("malformed-token\t"))
            .expect("malformed-token oracle row");
        assert_eq!(
            format!("{backfill_attempts}|invalid_payload|invalid_payload|{message}"),
            expected
        );
        assert_eq!(token_attempts.borrow().len(), 1);
        assert!(operation.transport.history_bindings.is_empty());
    }

    /// Build one empty decoded history exchange with approved evidence.
    fn history_exchange(
        attempts: &Rc<RefCell<Vec<ProviderAttempt>>>,
        checksum: &str,
    ) -> Exchange<WorkerPurpleAirHistoryProduct> {
        Exchange {
            attempts: attempts.clone(),
            now_ms: START_MS,
            product: WorkerPurpleAirHistoryProduct {
                checksum: Sha256Digest::admitted(checksum),
                history: PurpleAirDecodedHistory {
                    header: weather_worker::PurpleAirDecodedHeader::Valid,
                    rows: Vec::new(),
                },
            },
            statuses: VecDeque::from([200]),
        }
    }

    /// Build one request carrying the same frozen PurpleAir identity.
    fn scheduled_request() -> SupplementalPublicStationScheduledRequest {
        SupplementalPublicStationScheduledRequest {
            identity: SupplementalPublicStationIdentity::PurpleAir {
                map_version: MAP_VERSION.to_owned(),
                sensor_index: SENSOR_INDEX,
            },
            model: Some("PA-II".to_owned()),
            serial: Some("purple-42".to_owned()),
            source_id: "source-purpleair".to_owned(),
            timezone: "UTC".to_owned(),
            window: ScheduledWindow {
                start_ms: START_MS,
                end_exclusive_ms: START_MS + 120_000,
            },
        }
    }
}
