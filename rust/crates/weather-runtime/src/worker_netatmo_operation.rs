//! Execute the typed Netatmo token and four-module measurement operation.

use weather_database::Sha256Digest;
use weather_providers::{
    DEFAULT_PROVIDER_MAX_BODY_BYTES, MAX_PROVIDER_BODY_BYTES, ProviderControls, ProviderIo,
    ProviderRequestPolicy, ProviderResponse, run_provider_request,
};
use weather_worker::{
    NetatmoMeasurementSeries, SupplementalPublicStationBatch, SupplementalPublicStationIdentity,
    SupplementalPublicStationProduct, SupplementalPublicStationScheduledRequest, TaskFailure,
    normalize_supplemental_public_station_batch_with_diagnostics,
};

use crate::{
    SupplementalPublicStationOperationError, WorkerPublicStationEvidence,
    validate_worker_public_station_range, worker_public_station_digest,
};

const MAXIMUM_NETATMO_RANGE_MILLISECONDS: i64 = 31 * 24 * 60 * 60 * 1_000;

/// One configured Netatmo device and its three public measurement modules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetatmoWorkerIdentity<'a> {
    pub device_id: &'a str,
    pub outdoor_module_id: &'a str,
    pub rain_module_id: &'a str,
    pub wind_module_id: &'a str,
}

/// One decoded visitor-token response with its admitted response digest.
pub struct NetatmoTokenProduct {
    pub checksum: Sha256Digest,
    pub token: String,
}

/// One decoded measurement response with its admitted response digest.
#[derive(Clone, Debug, PartialEq)]
pub struct NetatmoMeasurementProduct {
    pub checksum: Sha256Digest,
    pub series: NetatmoMeasurementSeries,
}

/// One exact Netatmo getmeasure request body without its bearer token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetatmoModulePlan {
    pub date_begin: String,
    pub date_end: String,
    pub device_id: String,
    pub module_id: String,
    pub optimize: bool,
    pub scale: &'static str,
    pub measurement_types: &'static str,
}

/// Four successful module responses retained in Node request order.
#[derive(Clone, Debug, PartialEq)]
pub struct NetatmoModuleResponses {
    pub outdoor: ProviderResponse<NetatmoMeasurementProduct>,
    pub pressure: ProviderResponse<NetatmoMeasurementProduct>,
    pub rain: ProviderResponse<NetatmoMeasurementProduct>,
    pub wind: ProviderResponse<NetatmoMeasurementProduct>,
}

/// Request-bound token and concurrent module transport ownership.
pub trait WorkerNetatmoTransport {
    type TokenExchange<'a>: ProviderIo<NetatmoTokenProduct>
    where
        Self: 'a;
    type ModuleExchange: ProviderIo<NetatmoMeasurementProduct> + Send;

    /// Bind the fixed visitor-token request before any measurement request starts.
    fn bind_token(&mut self) -> Result<Self::TokenExchange<'_>, TaskFailure>;

    /// Bind and start all four module exchanges independently, return the first failure unchanged,
    /// retain and drain siblings, and expose the successful responses only after all settle.
    fn run_modules<Error, Outdoor, Pressure, Rain, Wind>(
        &mut self,
        token: String,
        plans: [NetatmoModulePlan; 4],
        outdoor: Outdoor,
        pressure: Pressure,
        rain: Rain,
        wind: Wind,
    ) -> Result<NetatmoModuleResponses, Error>
    where
        Error: From<TaskFailure> + Send + 'static,
        Outdoor: FnOnce(
                &mut Self::ModuleExchange,
            ) -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
            + Send
            + 'static,
        Pressure: FnOnce(
                &mut Self::ModuleExchange,
            ) -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
            + Send
            + 'static,
        Rain: FnOnce(
                &mut Self::ModuleExchange,
            ) -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
            + Send
            + 'static,
        Wind: FnOnce(
                &mut Self::ModuleExchange,
            ) -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
            + Send
            + 'static;

    /// Sample the receipt clock once after token and every module response have settled.
    fn netatmo_received_at_ms(&mut self) -> i64;
}

/// Fetch, evidence, and normalize one admitted Netatmo public-station range.
pub fn fetch_worker_netatmo(
    transport: &mut impl WorkerNetatmoTransport,
    evidence: &mut impl WorkerPublicStationEvidence,
    request: &SupplementalPublicStationScheduledRequest,
    identity: NetatmoWorkerIdentity<'_>,
    policy: ProviderRequestPolicy,
) -> Result<SupplementalPublicStationBatch, TaskFailure> {
    fetch_worker_netatmo_operation(transport, evidence, request, identity, policy)
        .map_err(SupplementalPublicStationOperationError::into_task_failure)
}

/// Retain classified provider controller failures for a one-shot archive owner.
pub(crate) fn fetch_worker_netatmo_operation(
    transport: &mut impl WorkerNetatmoTransport,
    evidence: &mut impl WorkerPublicStationEvidence,
    request: &SupplementalPublicStationScheduledRequest,
    identity: NetatmoWorkerIdentity<'_>,
    policy: ProviderRequestPolicy,
) -> Result<SupplementalPublicStationBatch, SupplementalPublicStationOperationError> {
    validate_worker_public_station_range(request.window, MAXIMUM_NETATMO_RANGE_MILLISECONDS)?;
    validate_identity(request, identity)?;
    let plans = module_plans(request, identity)?;
    let token_policy = policy_with_body_limit(policy, DEFAULT_PROVIDER_MAX_BODY_BYTES);
    let token_response = {
        let mut token = transport.bind_token()?;
        run_provider_request(token_policy, &mut token)?
    };
    let NetatmoTokenProduct {
        checksum: token_checksum,
        token,
    } = token_response.body;
    validate_token(&token)?;
    let module_policy = policy_with_body_limit(policy, MAX_PROVIDER_BODY_BYTES);
    let module_responses = transport
        .run_modules::<SupplementalPublicStationOperationError, _, _, _, _>(
            token,
            plans,
            move |exchange| run_provider_request(module_policy, exchange).map_err(Into::into),
            move |exchange| run_provider_request(module_policy, exchange).map_err(Into::into),
            move |exchange| run_provider_request(module_policy, exchange).map_err(Into::into),
            move |exchange| run_provider_request(module_policy, exchange).map_err(Into::into),
        )?;
    let received_at_ms = transport.netatmo_received_at_ms();
    let NetatmoModuleResponses {
        outdoor,
        pressure,
        rain,
        wind,
    } = module_responses;
    let attempts = u32::from(token_response.attempts)
        + u32::from(outdoor.attempts)
        + u32::from(pressure.attempts)
        + u32::from(rain.attempts)
        + u32::from(wind.attempts);
    let NetatmoMeasurementProduct {
        checksum: outdoor_checksum,
        series: outdoor_series,
    } = outdoor.body;
    let NetatmoMeasurementProduct {
        checksum: pressure_checksum,
        series: pressure_series,
    } = pressure.body;
    let NetatmoMeasurementProduct {
        checksum: rain_checksum,
        series: rain_series,
    } = rain.body;
    let NetatmoMeasurementProduct {
        checksum: wind_checksum,
        series: wind_series,
    } = wind.body;
    let modules = [outdoor_series, pressure_series, rain_series, wind_series];
    let mut batch = normalize_supplemental_public_station_batch_with_diagnostics(
        request.normalization_inputs(),
        SupplementalPublicStationProduct::Netatmo {
            device_id: identity.device_id,
            modules: &modules,
        },
        received_at_ms,
        attempts,
        String::new(),
    )?;
    batch.upstream_response_checksum = worker_public_station_digest(
        evidence,
        &[
            &token_checksum,
            &outdoor_checksum,
            &pressure_checksum,
            &rain_checksum,
            &wind_checksum,
        ],
    )?;
    Ok(batch)
}

/// Require the request's retained Netatmo identity and four lowercase module identifiers.
fn validate_identity(
    request: &SupplementalPublicStationScheduledRequest,
    identity: NetatmoWorkerIdentity<'_>,
) -> Result<(), TaskFailure> {
    let SupplementalPublicStationIdentity::Netatmo {
        device_id,
        outdoor_module_id,
        rain_module_id,
        wind_module_id,
    } = &request.identity
    else {
        return Err(invalid_request());
    };
    // bind the supplied operation identity to the request admitted by the retained catalog
    if device_id != identity.device_id
        || outdoor_module_id != identity.outdoor_module_id
        || rain_module_id != identity.rain_module_id
        || wind_module_id != identity.wind_module_id
        || ![
            identity.device_id,
            identity.outdoor_module_id,
            identity.rain_module_id,
            identity.wind_module_id,
        ]
        .into_iter()
        .all(netatmo_identifier)
    {
        return Err(invalid_request());
    }
    Ok(())
}

/// Match one exact lowercase colon-delimited Netatmo hardware identifier.
fn netatmo_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 17
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            2 | 5 | 8 | 11 | 14 => *byte == b':',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(byte),
        })
}

/// Build the exact ordered getmeasure bodies for the configured module identities.
fn module_plans(
    request: &SupplementalPublicStationScheduledRequest,
    identity: NetatmoWorkerIdentity<'_>,
) -> Result<[NetatmoModulePlan; 4], TaskFailure> {
    let date_begin = request.window.start_ms.div_euclid(1_000).to_string();
    let date_end = request
        .window
        .end_exclusive_ms
        .checked_sub(1)
        .ok_or_else(invalid_request)?
        .div_euclid(1_000)
        .to_string();
    Ok([
        module_plan(
            identity,
            identity.outdoor_module_id,
            "Temperature,Humidity",
            &date_begin,
            &date_end,
        ),
        module_plan(
            identity,
            identity.device_id,
            "Pressure",
            &date_begin,
            &date_end,
        ),
        module_plan(
            identity,
            identity.rain_module_id,
            "Rain",
            &date_begin,
            &date_end,
        ),
        module_plan(
            identity,
            identity.wind_module_id,
            "WindStrength,WindAngle,GustStrength,GustAngle",
            &date_begin,
            &date_end,
        ),
    ])
}

/// Build one getmeasure body while keeping the visitor token outside diagnostic material.
fn module_plan(
    identity: NetatmoWorkerIdentity<'_>,
    module_id: &str,
    measurement_types: &'static str,
    date_begin: &str,
    date_end: &str,
) -> NetatmoModulePlan {
    NetatmoModulePlan {
        date_begin: date_begin.to_owned(),
        date_end: date_end.to_owned(),
        device_id: identity.device_id.to_owned(),
        module_id: module_id.to_owned(),
        optimize: false,
        scale: "max",
        measurement_types,
    }
}

/// Preserve the shared retry/deadline policy while selecting the Node response cap.
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

/// Require the decoded visitor token before constructing any module exchanges.
fn validate_token(token: &str) -> Result<(), SupplementalPublicStationOperationError> {
    // match requireString's nonempty 4096 UTF-16-unit boundary
    if token.is_empty() || token.encode_utf16().count() > 4_096 {
        return Err(SupplementalPublicStationOperationError::invalid_payload(
            1,
            1,
            "token body must be a bounded non-empty string",
        ));
    }
    Ok(())
}

/// Return one zero-attempt typed request construction failure.
fn invalid_request() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "provider_unavailable".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use weather_providers::{
        ContentBudget, ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse,
        ProviderSleep, ProviderSleepError,
    };
    use weather_worker::{
        NetatmoMeasurementBody, NetatmoMeasurementRow, NetatmoMeasurementStatus,
        NetatmoMeasurementValues, PublicStationNumber, ScheduledWindow,
        SupplementalPublicStationIdentity,
    };

    use super::*;

    const TOKEN_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OUTDOOR_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const PRESSURE_SHA: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const RAIN_SHA: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const WIND_SHA: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const COMBINED_SHA: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    const START_MS: i64 = 1_787_576_400_000;
    const END_MS: i64 = 1_787_580_000_000;

    struct TokenExchange {
        attempts: Arc<Mutex<Vec<ProviderAttempt>>>,
        trace: Arc<Mutex<Vec<&'static str>>>,
    }

    impl ProviderIo<NetatmoTokenProduct> for TokenExchange {
        /// Return the shared clock below the absolute deadline.
        fn now_ms(&mut self) -> i64 {
            1_000
        }

        /// Return unused deterministic jitter.
        fn random_unit(&mut self) -> f64 {
            0.0
        }

        /// Capture the bounded token attempt and return its decoded token.
        fn execute(
            &mut self,
            attempt: ProviderAttempt,
        ) -> Result<ProviderAttemptResponse<NetatmoTokenProduct>, ProviderAttemptError> {
            self.trace.lock().unwrap().push("token-attempt");
            self.attempts.lock().unwrap().push(attempt);
            Ok(ProviderAttemptResponse {
                body: NetatmoTokenProduct {
                    checksum: Sha256Digest::admitted(TOKEN_SHA),
                    token: "visitor-token".to_owned(),
                },
                content: ContentBudget {
                    declared_body_bytes: Some(100),
                    streamed_body_bytes: 100,
                },
                provider_reason: None,
                retry_after_ms: None,
                status: 200,
            })
        }

        /// Reject an unexpected token retry.
        fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
            panic!("unexpected token retry: {sleep:?}")
        }
    }

    struct ModuleExchange {
        label: &'static str,
        attempts: Arc<Mutex<Vec<(&'static str, ProviderAttempt)>>>,
        now_ms: i64,
        product: NetatmoMeasurementProduct,
        statuses: VecDeque<u16>,
        trace: Arc<Mutex<Vec<&'static str>>>,
    }

    impl ProviderIo<NetatmoMeasurementProduct> for ModuleExchange {
        /// Return the module-local clock under the common deadline.
        fn now_ms(&mut self) -> i64 {
            self.now_ms
        }

        /// Return deterministic zero jitter for the one retry.
        fn random_unit(&mut self) -> f64 {
            0.0
        }

        /// Capture one physical module attempt and return its typed series.
        fn execute(
            &mut self,
            attempt: ProviderAttempt,
        ) -> Result<ProviderAttemptResponse<NetatmoMeasurementProduct>, ProviderAttemptError>
        {
            self.trace.lock().unwrap().push(self.label);
            self.attempts.lock().unwrap().push((self.label, attempt));
            Ok(ProviderAttemptResponse {
                body: self.product.clone(),
                content: ContentBudget {
                    declared_body_bytes: Some(1_000),
                    streamed_body_bytes: 1_000,
                },
                provider_reason: None,
                retry_after_ms: None,
                status: self
                    .statuses
                    .pop_front()
                    .expect("module status queue exhausted"),
            })
        }

        /// Apply the bounded retry delay without changing the absolute deadline.
        fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
            assert_eq!(sleep.deadline_remaining_ms, Some(500));
            assert_eq!(sleep.delay_ms, 250);
            self.now_ms += 250;
            Ok(())
        }
    }

    struct Transport {
        module_attempts: Arc<Mutex<Vec<(&'static str, ProviderAttempt)>>>,
        plans: Option<[NetatmoModulePlan; 4]>,
        receipt_calls: usize,
        token_attempts: Arc<Mutex<Vec<ProviderAttempt>>>,
        trace: Arc<Mutex<Vec<&'static str>>>,
    }

    impl WorkerNetatmoTransport for Transport {
        type TokenExchange<'a>
            = TokenExchange
        where
            Self: 'a;
        type ModuleExchange = ModuleExchange;

        /// Bind the token exchange before module construction.
        fn bind_token(&mut self) -> Result<Self::TokenExchange<'_>, TaskFailure> {
            self.trace.lock().unwrap().push("token-bind");
            Ok(TokenExchange {
                attempts: Arc::clone(&self.token_attempts),
                trace: Arc::clone(&self.trace),
            })
        }

        /// Settle four independently controlled module exchanges in request order.
        fn run_modules<Error, Outdoor, Pressure, Rain, Wind>(
            &mut self,
            token: String,
            plans: [NetatmoModulePlan; 4],
            outdoor: Outdoor,
            pressure: Pressure,
            rain: Rain,
            wind: Wind,
        ) -> Result<NetatmoModuleResponses, Error>
        where
            Error: From<TaskFailure> + Send + 'static,
            Outdoor: FnOnce(
                    &mut Self::ModuleExchange,
                )
                    -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
                + Send
                + 'static,
            Pressure: FnOnce(
                    &mut Self::ModuleExchange,
                )
                    -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
                + Send
                + 'static,
            Rain: FnOnce(
                    &mut Self::ModuleExchange,
                )
                    -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
                + Send
                + 'static,
            Wind: FnOnce(
                    &mut Self::ModuleExchange,
                )
                    -> Result<ProviderResponse<NetatmoMeasurementProduct>, Error>
                + Send
                + 'static,
        {
            assert_eq!(token, "visitor-token");
            self.trace.lock().unwrap().push("module-bind");
            self.plans = Some(plans);
            let mut outdoor_exchange = self.module("outdoor-attempt", OUTDOOR_SHA, true);
            let mut pressure_exchange = self.module("pressure-attempt", PRESSURE_SHA, false);
            let mut rain_exchange = self.module("rain-attempt", RAIN_SHA, false);
            let mut wind_exchange = self.module("wind-attempt", WIND_SHA, false);
            Ok(NetatmoModuleResponses {
                outdoor: outdoor(&mut outdoor_exchange)?,
                pressure: pressure(&mut pressure_exchange)?,
                rain: rain(&mut rain_exchange)?,
                wind: wind(&mut wind_exchange)?,
            })
        }

        /// Sample the receipt clock once after every module settles.
        fn netatmo_received_at_ms(&mut self) -> i64 {
            self.receipt_calls += 1;
            1_787_583_600_000
        }
    }

    impl Transport {
        /// Build one typed module exchange with an optional transient response.
        fn module(&self, label: &'static str, checksum: &str, retry: bool) -> ModuleExchange {
            ModuleExchange {
                label,
                attempts: Arc::clone(&self.module_attempts),
                now_ms: 1_000,
                product: NetatmoMeasurementProduct {
                    checksum: Sha256Digest::admitted(checksum),
                    series: if label == "outdoor-attempt" {
                        outdoor_series()
                    } else {
                        empty_series()
                    },
                },
                statuses: if retry {
                    VecDeque::from([503, 200])
                } else {
                    VecDeque::from([200])
                },
                trace: Arc::clone(&self.trace),
            }
        }
    }

    #[derive(Default)]
    struct Evidence {
        material: Vec<u8>,
    }

    impl WorkerPublicStationEvidence for Evidence {
        /// Capture exact ordered response-digest framing.
        fn sha256(&mut self, material: &[u8]) -> Result<Sha256Digest, TaskFailure> {
            self.material = material.to_vec();
            Ok(Sha256Digest::admitted(COMBINED_SHA))
        }
    }

    /// Preserve token-first execution, module plans, budgets, attempts, and digest order.
    #[test]
    fn executes_token_then_four_bounded_modules_with_ordered_evidence() {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let token_attempts = Arc::new(Mutex::new(Vec::new()));
        let module_attempts = Arc::new(Mutex::new(Vec::new()));
        let mut transport = Transport {
            module_attempts: Arc::clone(&module_attempts),
            plans: None,
            receipt_calls: 0,
            token_attempts: Arc::clone(&token_attempts),
            trace: Arc::clone(&trace),
        };
        let mut evidence = Evidence::default();
        let request = fixture_request();
        let batch = fetch_worker_netatmo(
            &mut transport,
            &mut evidence,
            &request,
            fixture_identity(),
            ProviderRequestPolicy {
                controls: ProviderControls {
                    max_attempts: 3,
                    max_body_bytes: 123,
                    timeout_ms: 777,
                },
                deadline_at_ms: Some(1_500),
            },
        )
        .expect("typed Netatmo operation should complete");

        assert_eq!(batch.attempts, 6);
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.upstream_response_checksum, COMBINED_SHA);
        assert_eq!(transport.receipt_calls, 1);
        assert_eq!(
            token_attempts.lock().unwrap().as_slice(),
            [ProviderAttempt {
                attempt: 1,
                max_body_bytes: DEFAULT_PROVIDER_MAX_BODY_BYTES,
                timeout_ms: 500,
            }],
        );
        let module_attempts = module_attempts.lock().unwrap();
        assert_eq!(module_attempts.len(), 5);
        assert!(module_attempts.iter().all(|(_, attempt)| {
            attempt.max_body_bytes == MAX_PROVIDER_BODY_BYTES
                && matches!(attempt.timeout_ms, 500 | 250)
        }));
        let plans = transport.plans.as_ref().expect("module plans should bind");
        assert_eq!(
            plans
                .iter()
                .map(|plan| (plan.module_id.as_str(), plan.measurement_types))
                .collect::<Vec<_>>(),
            [
                ("02:00:00:69:34:4e", "Temperature,Humidity"),
                ("70:ee:50:71:3f:c4", "Pressure"),
                ("05:00:00:08:97:7e", "Rain"),
                (
                    "06:00:00:06:45:7c",
                    "WindStrength,WindAngle,GustStrength,GustAngle",
                ),
            ],
        );
        assert!(plans.iter().all(|plan| {
            plan.date_begin == "1787576400"
                && plan.date_end == "1787579999"
                && !plan.optimize
                && plan.scale == "max"
        }));
        assert_eq!(
            evidence.material,
            format!("{TOKEN_SHA}\n{OUTDOOR_SHA}\n{PRESSURE_SHA}\n{RAIN_SHA}\n{WIND_SHA}\n")
                .into_bytes(),
        );
        assert_eq!(
            trace.lock().unwrap().as_slice(),
            [
                "token-bind",
                "token-attempt",
                "module-bind",
                "outdoor-attempt",
                "outdoor-attempt",
                "pressure-attempt",
                "rain-attempt",
                "wind-attempt",
            ],
        );
    }

    /// Build the exact configured identity used by the Node fixture.
    const fn fixture_identity() -> NetatmoWorkerIdentity<'static> {
        NetatmoWorkerIdentity {
            device_id: "70:ee:50:71:3f:c4",
            outdoor_module_id: "02:00:00:69:34:4e",
            rain_module_id: "05:00:00:08:97:7e",
            wind_module_id: "06:00:00:06:45:7c",
        }
    }

    /// Build one exact admitted scheduled Netatmo request.
    fn fixture_request() -> SupplementalPublicStationScheduledRequest {
        SupplementalPublicStationScheduledRequest {
            identity: SupplementalPublicStationIdentity::Netatmo {
                device_id: "70:ee:50:71:3f:c4".to_owned(),
                outdoor_module_id: "02:00:00:69:34:4e".to_owned(),
                rain_module_id: "05:00:00:08:97:7e".to_owned(),
                wind_module_id: "06:00:00:06:45:7c".to_owned(),
            },
            model: Some("test-model".to_owned()),
            serial: Some("test-serial".to_owned()),
            source_id: "source-public".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
            window: ScheduledWindow {
                start_ms: START_MS,
                end_exclusive_ms: END_MS,
            },
        }
    }

    /// Build the one outdoor observation retained by common normalization.
    fn outdoor_series() -> NetatmoMeasurementSeries {
        NetatmoMeasurementSeries {
            status: NetatmoMeasurementStatus::Ok,
            body: NetatmoMeasurementBody::Rows(vec![NetatmoMeasurementRow {
                epoch_seconds: 1_787_576_617,
                values: NetatmoMeasurementValues::Numbers(vec![
                    PublicStationNumber::Number(12.7),
                    PublicStationNumber::Number(96.0),
                ]),
            }]),
        }
    }

    /// Build one successful empty optional module response.
    fn empty_series() -> NetatmoMeasurementSeries {
        NetatmoMeasurementSeries {
            status: NetatmoMeasurementStatus::Ok,
            body: NetatmoMeasurementBody::Null,
        }
    }
}
