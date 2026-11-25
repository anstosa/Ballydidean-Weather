//! Transport-neutral provider retry, deadline, and response-budget policy.

pub const DEFAULT_PROVIDER_TIMEOUT_MS: u64 = 10_000;
pub const DEFAULT_PROVIDER_MAX_ATTEMPTS: u8 = 3;
pub const DEFAULT_PROVIDER_MAX_BODY_BYTES: u64 = 2_000_000;
pub const MAX_PROVIDER_RETRY_DELAY_MS: u64 = 30_000;
pub const MAX_PROVIDER_BODY_BYTES: u64 = 10_000_000;
pub const MAX_PROVIDER_TIMEOUT_MS: u64 = 60_000;

/// Bounded controls for one provider retry loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderControls {
    pub max_attempts: u8,
    pub max_body_bytes: u64,
    pub timeout_ms: u64,
}

impl Default for ProviderControls {
    /// Return the production Node provider defaults.
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_PROVIDER_MAX_ATTEMPTS,
            max_body_bytes: DEFAULT_PROVIDER_MAX_BODY_BYTES,
            timeout_ms: DEFAULT_PROVIDER_TIMEOUT_MS,
        }
    }
}

/// One validated transport-neutral request policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderRequestPolicy {
    pub controls: ProviderControls,
    pub deadline_at_ms: Option<i64>,
}

/// Validation failures for typed provider controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderControlError {
    AttemptsOutOfRange,
    BodyLimitOutOfRange,
    TimeoutOutOfRange,
}

/// Durable provider failure classifications shared with the Node contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderFailureClassification {
    InvalidPayload,
    Permanent,
    RateLimited,
    Retryable,
}

/// The response-budget boundary which rejected an attempted response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentBudgetRole {
    DeclaredLength,
    StreamedBytes,
}

/// One typed durable provider diagnosis.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderError {
    pub classification: ProviderFailureClassification,
    pub code: String,
    pub message: String,
    pub retry_after_ms: Option<f64>,
}

/// One typed provider failure retained across the retry controller.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderFailure {
    pub attempts: u8,
    pub content_budget_role: Option<ContentBudgetRole>,
    pub error: ProviderError,
    pub status: Option<u16>,
}

impl ProviderFailure {
    /// Construct one already-classified provider failure.
    pub fn classified(
        attempts: u8,
        classification: ProviderFailureClassification,
        code: impl Into<String>,
        message: impl Into<String>,
        status: Option<u16>,
    ) -> Self {
        Self {
            attempts,
            content_budget_role: None,
            error: ProviderError {
                classification,
                code: code.into(),
                message: message.into(),
                retry_after_ms: None,
            },
            status,
        }
    }
}

/// Byte evidence supplied by an admitted response consumer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContentBudget {
    pub declared_body_bytes: Option<u64>,
    pub streamed_body_bytes: u64,
}

/// One parsed response returned by an injected transport.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderAttemptResponse<T> {
    pub body: T,
    pub content: ContentBudget,
    pub provider_reason: Option<String>,
    pub retry_after_ms: Option<f64>,
    pub status: u16,
}

/// One request attempt admitted to an injected transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderAttempt {
    pub attempt: u8,
    pub max_body_bytes: u64,
    pub timeout_ms: u64,
}

/// Transport outcomes which occur before an HTTP response is available.
#[derive(Clone, Debug, PartialEq)]
pub enum ProviderAttemptError {
    Classified(ProviderFailure),
    Transport { message: String },
}

/// One injected sleep plan, including its deadline watchdog budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderSleep {
    pub attempts: u8,
    pub deadline_remaining_ms: Option<u64>,
    pub delay_ms: u64,
}

/// Typed failures from an injected retry sleeper.
#[derive(Clone, Debug, PartialEq)]
pub enum ProviderSleepError {
    DeadlineExceeded,
    Failure(ProviderFailure),
}

/// The successful response retained after bounded retries.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderResponse<T> {
    pub attempts: u8,
    pub body: T,
    pub status: u16,
}

/// Top-level provider controller failure.
#[derive(Clone, Debug, PartialEq)]
pub enum ProviderRunError {
    InvalidControls(ProviderControlError),
    Provider(ProviderFailure),
}

/// Injected transport, clock, jitter, and sleep effects for provider policy.
pub trait ProviderIo<T> {
    /// Return the admitted wall clock in Unix milliseconds.
    fn now_ms(&mut self) -> i64;

    /// Return one jitter input, normally in the inclusive unit interval.
    fn random_unit(&mut self) -> f64;

    /// Execute one already-bounded provider attempt.
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<T>, ProviderAttemptError>;

    /// Sleep under the controller's optional deadline watchdog.
    fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError>;
}

/// Validate typed provider controls before any transport work.
pub fn validate_provider_controls(controls: ProviderControls) -> Result<(), ProviderControlError> {
    // reject unbounded timeouts
    if controls.timeout_ms < 1 || controls.timeout_ms > MAX_PROVIDER_TIMEOUT_MS {
        return Err(ProviderControlError::TimeoutOutOfRange);
    }

    // reject unbounded retries
    if controls.max_attempts < 1 || controls.max_attempts > DEFAULT_PROVIDER_MAX_ATTEMPTS {
        return Err(ProviderControlError::AttemptsOutOfRange);
    }

    // reject unbounded response bodies
    if controls.max_body_bytes < 1 || controls.max_body_bytes > MAX_PROVIDER_BODY_BYTES {
        return Err(ProviderControlError::BodyLimitOutOfRange);
    }

    Ok(())
}

/// Calculate the maximum complete retry duration from bounded controls.
pub fn provider_request_budget_milliseconds(
    controls: ProviderControls,
) -> Result<u64, ProviderControlError> {
    validate_provider_controls(controls)?;
    let attempts = u64::from(controls.max_attempts);
    let retry_count = attempts.saturating_sub(1);

    Ok(attempts * controls.timeout_ms + retry_count * MAX_PROVIDER_RETRY_DELAY_MS)
}

/// Execute one provider request through the exact bounded retry policy.
pub fn run_provider_request<T, I>(
    policy: ProviderRequestPolicy,
    io: &mut I,
) -> Result<ProviderResponse<T>, ProviderRunError>
where
    I: ProviderIo<T>,
{
    validate_provider_controls(policy.controls).map_err(ProviderRunError::InvalidControls)?;

    // attempt only the bounded request budget
    for attempt in 1..=policy.controls.max_attempts {
        let timeout_ms = bounded_attempt_timeout(
            policy.controls.timeout_ms,
            policy.deadline_at_ms,
            io,
            attempt,
        )
        .map_err(ProviderRunError::Provider)?;
        let attempted = io.execute(ProviderAttempt {
            attempt,
            max_body_bytes: policy.controls.max_body_bytes,
            timeout_ms,
        });

        // classify either a parsed response or a transport-level error
        let outcome = match attempted {
            Ok(response) => classify_response(response, policy.controls.max_body_bytes, attempt),
            Err(error) => Err(classify_attempt_error(error, attempt)),
        };

        // return the first complete successful response
        let failure = match outcome {
            Ok(response) => {
                return Ok(ProviderResponse {
                    attempts: attempt,
                    body: response.body,
                    status: response.status,
                });
            }
            Err(failure) => failure,
        };
        let failure = if deadline_expired(policy.deadline_at_ms, io) {
            deadline_failure(attempt)
        } else {
            failure
        };

        // stop on permanent failures exhausted attempts or the run deadline
        if !is_retryable(&failure)
            || attempt == policy.controls.max_attempts
            || failure.error.code == "provider_deadline_exceeded"
        {
            return Err(ProviderRunError::Provider(failure));
        }
        let delay_ms = retry_delay_milliseconds(&failure, attempt, io.random_unit());
        let sleep = bounded_sleep(delay_ms, policy.deadline_at_ms, io, attempt)
            .map_err(ProviderRunError::Provider)?;

        // execute the admitted retry delay under its watchdog
        if let Err(error) = io.sleep(sleep) {
            // preserve an explicit sleep failure or construct its deadline failure
            let failure = match error {
                ProviderSleepError::DeadlineExceeded => deadline_failure(attempt),
                ProviderSleepError::Failure(failure) => failure,
            };
            return Err(ProviderRunError::Provider(failure));
        }
    }

    unreachable!("validated provider attempt loop must terminate")
}

// bound one attempt by both request and absolute run deadlines
fn bounded_attempt_timeout<T, I>(
    timeout_ms: u64,
    deadline_at_ms: Option<i64>,
    io: &mut I,
    attempts: u8,
) -> Result<u64, ProviderFailure>
where
    I: ProviderIo<T>,
{
    // retain the request timeout without a run deadline
    let Some(deadline_at_ms) = deadline_at_ms else {
        return Ok(timeout_ms);
    };
    let remaining = deadline_at_ms.saturating_sub(io.now_ms());

    // fail before starting work past the run deadline
    if remaining <= 0 {
        return Err(deadline_failure(attempts));
    }
    let remaining = u64::try_from(remaining).unwrap_or(u64::MAX);

    Ok(timeout_ms.min(remaining))
}

// detect an exhausted absolute deadline
fn deadline_expired<T, I>(deadline_at_ms: Option<i64>, io: &mut I) -> bool
where
    I: ProviderIo<T>,
{
    // omit clock reads for standalone provider requests
    let Some(deadline_at_ms) = deadline_at_ms else {
        return false;
    };

    io.now_ms() >= deadline_at_ms
}

// admit one retry sleep inside the absolute deadline
fn bounded_sleep<T, I>(
    delay_ms: u64,
    deadline_at_ms: Option<i64>,
    io: &mut I,
    attempts: u8,
) -> Result<ProviderSleep, ProviderFailure>
where
    I: ProviderIo<T>,
{
    // retain bounded default retry behavior without a run deadline
    let Some(deadline_at_ms) = deadline_at_ms else {
        return Ok(ProviderSleep {
            attempts,
            deadline_remaining_ms: None,
            delay_ms,
        });
    };
    let remaining = deadline_at_ms.saturating_sub(io.now_ms());

    // reject a delay which cannot complete in budget
    if remaining <= 0 || delay_ms > u64::try_from(remaining).unwrap_or(u64::MAX) {
        return Err(deadline_failure(attempts));
    }

    Ok(ProviderSleep {
        attempts,
        deadline_remaining_ms: Some(u64::try_from(remaining).unwrap_or(u64::MAX)),
        delay_ms,
    })
}

// classify one parsed response after its admitted body consumption
fn classify_response<T>(
    response: ProviderAttemptResponse<T>,
    max_body_bytes: u64,
    attempts: u8,
) -> Result<ProviderAttemptResponse<T>, ProviderFailure> {
    // reject an honestly declared oversized response before streamed evidence
    if response
        .content
        .declared_body_bytes
        .is_some_and(|bytes| bytes > max_body_bytes)
    {
        return Err(response_too_large_failure(
            attempts,
            response.status,
            ContentBudgetRole::DeclaredLength,
        ));
    }

    // reject dishonest or absent lengths at the actual streamed boundary
    if response.content.streamed_body_bytes > max_body_bytes {
        return Err(response_too_large_failure(
            attempts,
            response.status,
            ContentBudgetRole::StreamedBytes,
        ));
    }

    // return only successful HTTP status responses
    if (200..300).contains(&response.status) {
        return Ok(response);
    }

    Err(classify_http_failure(&response, attempts))
}

// classify an oversized provider response
fn response_too_large_failure(
    attempts: u8,
    status: u16,
    role: ContentBudgetRole,
) -> ProviderFailure {
    ProviderFailure {
        attempts,
        content_budget_role: Some(role),
        error: ProviderError {
            classification: ProviderFailureClassification::InvalidPayload,
            code: "provider_response_too_large".to_owned(),
            message: "provider response exceeded the byte limit".to_owned(),
            retry_after_ms: None,
        },
        status: Some(status),
    }
}

// classify one parsed non-success HTTP response
fn classify_http_failure<T>(
    response: &ProviderAttemptResponse<T>,
    attempts: u8,
) -> ProviderFailure {
    let retry_after_ms = response.retry_after_ms.filter(|value| value.is_finite());

    // classify rate limits separately
    if response.status == 429 {
        return ProviderFailure {
            attempts,
            content_budget_role: None,
            error: ProviderError {
                classification: ProviderFailureClassification::RateLimited,
                code: "provider_rate_limited".to_owned(),
                message: provider_reason(
                    response.provider_reason.as_deref(),
                    "provider rate limit exceeded",
                ),
                retry_after_ms,
            },
            status: Some(response.status),
        };
    }

    // allow bounded retries for server errors
    if response.status >= 500 {
        return ProviderFailure {
            attempts,
            content_budget_role: None,
            error: ProviderError {
                classification: ProviderFailureClassification::Retryable,
                code: "provider_server_error".to_owned(),
                message: provider_reason(
                    response.provider_reason.as_deref(),
                    &format!("provider returned HTTP {}", response.status),
                ),
                retry_after_ms: None,
            },
            status: Some(response.status),
        };
    }

    ProviderFailure {
        attempts,
        content_budget_role: None,
        error: ProviderError {
            classification: ProviderFailureClassification::Permanent,
            code: "provider_request_rejected".to_owned(),
            message: provider_reason(
                response.provider_reason.as_deref(),
                &format!("provider returned HTTP {}", response.status),
            ),
            retry_after_ms: None,
        },
        status: Some(response.status),
    }
}

// classify transport and timeout failures
fn classify_attempt_error(error: ProviderAttemptError, attempts: u8) -> ProviderFailure {
    // retain explicit provider failures
    match error {
        ProviderAttemptError::Classified(failure) => failure,
        ProviderAttemptError::Transport { message } => ProviderFailure {
            attempts,
            content_budget_role: None,
            error: ProviderError {
                classification: ProviderFailureClassification::Retryable,
                code: "provider_unavailable".to_owned(),
                message: bounded_message(&message, "provider request failed"),
                retry_after_ms: None,
            },
            status: None,
        },
    }
}

// determine retry eligibility
fn is_retryable(failure: &ProviderFailure) -> bool {
    matches!(
        failure.error.classification,
        ProviderFailureClassification::Retryable | ProviderFailureClassification::RateLimited
    )
}

// calculate bounded exponential backoff
fn retry_delay_milliseconds(failure: &ProviderFailure, attempt: u8, random: f64) -> u64 {
    // honor one finite bounded retry-after response
    if let Some(retry_after_ms) = failure.error.retry_after_ms
        && retry_after_ms.is_finite()
    {
        let rounded = retry_after_ms.round().max(0.0);
        return rounded.min(MAX_PROVIDER_RETRY_DELAY_MS as f64) as u64;
    }

    // match JavaScript timer behavior for a non-finite injected random value
    let random = if random.is_nan() {
        0.0
    } else {
        random.clamp(0.0, 1.0)
    };
    let jitter = (random * 100.0).floor() as u64;
    let base = 250_u64 * 2_u64.pow(u32::from(attempt.saturating_sub(1)));

    (base + jitter).min(MAX_PROVIDER_RETRY_DELAY_MS)
}

// construct one exhausted run-deadline failure
fn deadline_failure(attempts: u8) -> ProviderFailure {
    ProviderFailure {
        attempts,
        content_budget_role: None,
        error: ProviderError {
            classification: ProviderFailureClassification::Retryable,
            code: "provider_deadline_exceeded".to_owned(),
            message: "provider run deadline exceeded".to_owned(),
            retry_after_ms: None,
        },
        status: None,
    }
}

// retain one admitted provider reason or its status fallback
fn provider_reason(reason: Option<&str>, fallback: &str) -> String {
    // omit absent or empty admitted provider reasons
    let Some(reason) = reason.map(str::trim).filter(|value| !value.is_empty()) else {
        return fallback.to_owned();
    };

    bounded_message(reason, fallback)
}

// bound one admitted diagnosis to the durable 512 UTF-16-unit contract
fn bounded_message(message: &str, fallback: &str) -> String {
    let trimmed = message.trim();

    // retain a stable non-empty fallback
    if trimmed.is_empty() {
        return fallback.to_owned();
    }
    let mut utf16_units = 0;
    let mut bounded = String::new();

    // retain whole Unicode scalars without crossing the durable boundary
    for character in trimmed.chars() {
        let character_units = character.len_utf16();

        // stop before crossing 512 JavaScript string units
        if utf16_units + character_units > 512 {
            break;
        }
        bounded.push(character);
        utf16_units += character_units;
    }

    bounded
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{
        ContentBudget, ContentBudgetRole, DEFAULT_PROVIDER_MAX_BODY_BYTES,
        DEFAULT_PROVIDER_TIMEOUT_MS, MAX_PROVIDER_RETRY_DELAY_MS, ProviderAttempt,
        ProviderAttemptError, ProviderAttemptResponse, ProviderControlError, ProviderControls,
        ProviderFailure, ProviderFailureClassification, ProviderIo, ProviderRequestPolicy,
        ProviderRunError, ProviderSleep, ProviderSleepError, provider_request_budget_milliseconds,
        run_provider_request, validate_provider_controls,
    };

    #[derive(Debug, Default)]
    struct FakeIo {
        attempts: Vec<ProviderAttempt>,
        now_ms: VecDeque<i64>,
        outcomes: VecDeque<Result<ProviderAttemptResponse<String>, ProviderAttemptError>>,
        random: VecDeque<f64>,
        sleep_errors: VecDeque<ProviderSleepError>,
        sleeps: Vec<ProviderSleep>,
    }

    impl ProviderIo<String> for FakeIo {
        // return the next admitted clock value
        fn now_ms(&mut self) -> i64 {
            self.now_ms.pop_front().unwrap_or(0)
        }

        // return the next deterministic jitter value
        fn random_unit(&mut self) -> f64 {
            self.random.pop_front().unwrap_or(0.0)
        }

        // return the next typed transport outcome
        fn execute(
            &mut self,
            attempt: ProviderAttempt,
        ) -> Result<ProviderAttemptResponse<String>, ProviderAttemptError> {
            self.attempts.push(attempt);
            self.outcomes
                .pop_front()
                .unwrap_or_else(|| panic!("fake provider outcome missing"))
        }

        // retain one requested retry sleep
        fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
            self.sleeps.push(sleep);

            // return one injected sleep failure when configured
            if let Some(error) = self.sleep_errors.pop_front() {
                return Err(error);
            }
            Ok(())
        }
    }

    // construct one parsed response with exact byte evidence
    fn response(
        status: u16,
        body: &str,
        reason: Option<&str>,
        retry_after_ms: Option<f64>,
        declared_body_bytes: Option<u64>,
        streamed_body_bytes: u64,
    ) -> ProviderAttemptResponse<String> {
        ProviderAttemptResponse {
            body: body.to_owned(),
            content: ContentBudget {
                declared_body_bytes,
                streamed_body_bytes,
            },
            provider_reason: reason.map(str::to_owned),
            retry_after_ms,
            status,
        }
    }

    // return the default standalone request policy
    fn default_policy() -> ProviderRequestPolicy {
        ProviderRequestPolicy {
            controls: ProviderControls::default(),
            deadline_at_ms: None,
        }
    }

    // retain exact control bounds and complete request budget arithmetic
    #[test]
    fn provider_controls_match_node_bounds_and_budget() {
        assert_eq!(
            provider_request_budget_milliseconds(ProviderControls::default()),
            Ok(90_000),
        );
        assert_eq!(
            validate_provider_controls(ProviderControls {
                max_attempts: 0,
                ..ProviderControls::default()
            }),
            Err(ProviderControlError::AttemptsOutOfRange),
        );
        assert_eq!(
            validate_provider_controls(ProviderControls {
                max_body_bytes: 10_000_001,
                ..ProviderControls::default()
            }),
            Err(ProviderControlError::BodyLimitOutOfRange),
        );
        assert_eq!(
            validate_provider_controls(ProviderControls {
                timeout_ms: 60_001,
                ..ProviderControls::default()
            }),
            Err(ProviderControlError::TimeoutOutOfRange),
        );
    }

    // retry transient server failures with exact exponential jitter delays
    #[test]
    fn provider_retries_server_failures_then_returns_success() {
        let mut io = FakeIo {
            outcomes: VecDeque::from([
                Ok(response(503, "one", Some("temporary"), None, None, 3)),
                Ok(response(503, "two", None, None, None, 3)),
                Ok(response(200, "ok", None, None, Some(2), 2)),
            ]),
            random: VecDeque::from([0.5, 0.5]),
            ..FakeIo::default()
        };

        let result = run_provider_request(default_policy(), &mut io)
            .expect("third bounded provider attempt should succeed");

        assert_eq!(result.attempts, 3);
        assert_eq!(result.status, 200);
        assert_eq!(result.body, "ok");
        assert_eq!(
            io.attempts,
            [
                ProviderAttempt {
                    attempt: 1,
                    max_body_bytes: DEFAULT_PROVIDER_MAX_BODY_BYTES,
                    timeout_ms: DEFAULT_PROVIDER_TIMEOUT_MS,
                },
                ProviderAttempt {
                    attempt: 2,
                    max_body_bytes: DEFAULT_PROVIDER_MAX_BODY_BYTES,
                    timeout_ms: DEFAULT_PROVIDER_TIMEOUT_MS,
                },
                ProviderAttempt {
                    attempt: 3,
                    max_body_bytes: DEFAULT_PROVIDER_MAX_BODY_BYTES,
                    timeout_ms: DEFAULT_PROVIDER_TIMEOUT_MS,
                },
            ],
        );
        assert_eq!(
            io.sleeps,
            [
                ProviderSleep {
                    attempts: 1,
                    deadline_remaining_ms: None,
                    delay_ms: 300,
                },
                ProviderSleep {
                    attempts: 2,
                    deadline_remaining_ms: None,
                    delay_ms: 550,
                },
            ],
        );
    }

    // return the exact final server failure after exhausting all attempts
    #[test]
    fn provider_returns_final_server_failure_after_bounded_retries() {
        let mut io = FakeIo {
            outcomes: VecDeque::from([
                Ok(response(503, "one", None, None, None, 3)),
                Ok(response(503, "two", None, None, None, 3)),
                Ok(response(503, "three", Some("still down"), None, None, 5)),
            ]),
            ..FakeIo::default()
        };

        let failure = run_provider_request(default_policy(), &mut io)
            .expect_err("third server failure should exhaust the attempt budget");
        let ProviderRunError::Provider(failure) = failure else {
            panic!("expected one provider failure");
        };

        assert_eq!(failure.attempts, 3);
        assert_eq!(failure.status, Some(503));
        assert_eq!(failure.error.code, "provider_server_error");
        assert_eq!(failure.error.message, "still down");
        assert_eq!(
            failure.error.classification,
            ProviderFailureClassification::Retryable
        );
        assert_eq!(
            io.sleeps
                .iter()
                .map(|sleep| sleep.delay_ms)
                .collect::<Vec<_>>(),
            [250, 500],
        );
    }

    // honor parsed Retry-After milliseconds before exponential jitter
    #[test]
    fn provider_honors_bounded_parsed_retry_after() {
        let mut io = FakeIo {
            outcomes: VecDeque::from([
                Ok(response(
                    429,
                    "slow",
                    Some("slow down"),
                    Some(1_000.0),
                    None,
                    4,
                )),
                Ok(response(200, "ok", None, None, None, 2)),
            ]),
            random: VecDeque::from([1.0]),
            ..FakeIo::default()
        };

        let result = run_provider_request(default_policy(), &mut io)
            .expect("rate-limited request should retry once");

        assert_eq!(result.attempts, 2);
        assert_eq!(io.sleeps[0].delay_ms, 1_000);
    }

    // reject a Retry-After delay which crosses the persisted run deadline
    #[test]
    fn provider_stops_before_retry_sleep_crosses_deadline() {
        let mut io = FakeIo {
            now_ms: VecDeque::from([0, 0, 0]),
            outcomes: VecDeque::from([Ok(response(
                429,
                "slow",
                Some("slow down"),
                Some(30_000.0),
                None,
                4,
            ))]),
            ..FakeIo::default()
        };
        let policy = ProviderRequestPolicy {
            controls: ProviderControls::default(),
            deadline_at_ms: Some(1_000),
        };

        let failure = run_provider_request(policy, &mut io)
            .expect_err("retry delay must not cross the run deadline");
        let ProviderRunError::Provider(failure) = failure else {
            panic!("expected one deadline provider failure");
        };

        assert_eq!(failure.attempts, 1);
        assert_eq!(failure.error.code, "provider_deadline_exceeded");
        assert!(io.sleeps.is_empty());
        assert_eq!(io.attempts[0].timeout_ms, 1_000);
    }

    // bound the physical attempt timeout by the remaining run deadline
    #[test]
    fn provider_attempt_timeout_uses_remaining_deadline_budget() {
        let mut io = FakeIo {
            now_ms: VecDeque::from([100]),
            outcomes: VecDeque::from([Ok(response(200, "ok", None, None, None, 2))]),
            ..FakeIo::default()
        };
        let policy = ProviderRequestPolicy {
            controls: ProviderControls::default(),
            deadline_at_ms: Some(600),
        };

        run_provider_request(policy, &mut io).expect("bounded attempt should succeed");

        assert_eq!(io.attempts[0].timeout_ms, 500);
    }

    // distinguish declared and streamed response budget enforcement
    #[test]
    fn provider_preserves_content_budget_roles() {
        let controls = ProviderControls {
            max_attempts: 1,
            max_body_bytes: 8,
            timeout_ms: DEFAULT_PROVIDER_TIMEOUT_MS,
        };

        // reject an honest declared length before streamed bytes
        let mut declared = FakeIo {
            outcomes: VecDeque::from([Ok(response(200, "body", None, None, Some(9), 4))]),
            ..FakeIo::default()
        };
        let declared_failure = run_provider_request(
            ProviderRequestPolicy {
                controls,
                deadline_at_ms: None,
            },
            &mut declared,
        )
        .expect_err("declared oversize response should fail");
        let ProviderRunError::Provider(declared_failure) = declared_failure else {
            panic!("expected declared provider failure");
        };
        assert_eq!(
            declared_failure.content_budget_role,
            Some(ContentBudgetRole::DeclaredLength)
        );

        // reject an understated response at the actual streamed boundary
        let mut streamed = FakeIo {
            outcomes: VecDeque::from([Ok(response(400, "body", None, None, Some(1), 9))]),
            ..FakeIo::default()
        };
        let streamed_failure = run_provider_request(
            ProviderRequestPolicy {
                controls,
                deadline_at_ms: None,
            },
            &mut streamed,
        )
        .expect_err("streamed oversize response should fail");
        let ProviderRunError::Provider(streamed_failure) = streamed_failure else {
            panic!("expected streamed provider failure");
        };
        assert_eq!(
            streamed_failure.content_budget_role,
            Some(ContentBudgetRole::StreamedBytes)
        );
        assert_eq!(
            streamed_failure.error.classification,
            ProviderFailureClassification::InvalidPayload
        );
    }

    // stop immediately on one permanent client rejection
    #[test]
    fn provider_does_not_retry_permanent_http_failure() {
        let mut io = FakeIo {
            outcomes: VecDeque::from([Ok(response(400, "bad", Some("bad input"), None, None, 3))]),
            ..FakeIo::default()
        };

        let failure = run_provider_request(default_policy(), &mut io)
            .expect_err("permanent HTTP response should not retry");
        let ProviderRunError::Provider(failure) = failure else {
            panic!("expected permanent provider failure");
        };

        assert_eq!(failure.attempts, 1);
        assert_eq!(failure.error.code, "provider_request_rejected");
        assert_eq!(
            failure.error.classification,
            ProviderFailureClassification::Permanent
        );
        assert!(io.sleeps.is_empty());
        assert_eq!(io.attempts.len(), 1);
    }

    // surface an injected sleep watchdog deadline with the current attempt
    #[test]
    fn provider_preserves_sleep_watchdog_deadline_failure() {
        let mut io = FakeIo {
            outcomes: VecDeque::from([Ok(response(503, "bad", None, None, None, 3))]),
            sleep_errors: VecDeque::from([ProviderSleepError::DeadlineExceeded]),
            ..FakeIo::default()
        };

        let failure = run_provider_request(default_policy(), &mut io)
            .expect_err("sleep watchdog should terminate retries");
        let ProviderRunError::Provider(failure) = failure else {
            panic!("expected sleep provider failure");
        };

        assert_eq!(failure.attempts, 1);
        assert_eq!(failure.error.code, "provider_deadline_exceeded");
    }

    // preserve one explicitly classified transport failure exactly
    #[test]
    fn provider_preserves_explicit_classified_failure() {
        let explicit = ProviderFailure::classified(
            2,
            ProviderFailureClassification::InvalidPayload,
            "invalid_payload",
            "typed failure",
            Some(200),
        );
        let mut io = FakeIo {
            outcomes: VecDeque::from([Err(ProviderAttemptError::Classified(explicit.clone()))]),
            ..FakeIo::default()
        };

        let failure = run_provider_request(default_policy(), &mut io)
            .expect_err("explicit permanent failure should stop");

        assert_eq!(failure, ProviderRunError::Provider(explicit));
    }

    // cap excessively large parsed Retry-After hints
    #[test]
    fn provider_caps_parsed_retry_after() {
        let mut io = FakeIo {
            outcomes: VecDeque::from([
                Ok(response(429, "slow", None, Some(90_000.0), None, 4)),
                Ok(response(200, "ok", None, None, None, 2)),
            ]),
            ..FakeIo::default()
        };

        run_provider_request(default_policy(), &mut io)
            .expect("bounded Retry-After should permit the next attempt");

        assert_eq!(io.sleeps[0].delay_ms, MAX_PROVIDER_RETRY_DELAY_MS);
    }
}
