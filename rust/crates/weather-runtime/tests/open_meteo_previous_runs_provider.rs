use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_contracts::{FixedForecastLeadHours, JsonValue, Nullable};
use weather_database::Sha256Digest;
use weather_providers::{
    ContentBudget, ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse,
    ProviderControls, ProviderIo, ProviderSleep, ProviderSleepError,
};
use weather_runtime::{
    OpenMeteoPreviousRunsEndpoint, OpenMeteoPreviousRunsOperation, OpenMeteoPreviousRunsPayload,
    OpenMeteoPreviousRunsQuery, OpenMeteoPreviousRunsTransport, OpenMeteoPreviousRunsValues,
};
use weather_worker::{
    FORECAST_ANCHOR_CONTRACT_EPOCH, ForecastAnchorBackfillOperationError,
    ForecastAnchorBackfillProvider, ForecastAnchorLocation, ForecastAnchorProviderRequest,
    normalize_forecast_anchor_payload,
};

const START_MS: i64 = 1_772_928_000_000;
const END_MS: i64 = 1_773_014_400_000;
const BASES: [(&str, &str, f64); 9] = [
    ("temperature_2m", "°C", 10.0),
    ("apparent_temperature", "°C", 9.0),
    ("relative_humidity_2m", "%", 75.0),
    ("precipitation", "mm", 0.0),
    ("cloud_cover", "%", 50.0),
    ("wind_speed_10m", "m/s", 2.0),
    ("wind_gusts_10m", "m/s", 4.0),
    ("wind_direction_10m", "°", 360.0),
    ("surface_pressure", "hPa", 1_012.0),
];

#[derive(Clone)]
struct FixtureExchange {
    payload: OpenMeteoPreviousRunsPayload,
}

impl ProviderIo<OpenMeteoPreviousRunsPayload> for FixtureExchange {
    /// Keep the one-attempt fixture inside its absolute deadline.
    fn now_ms(&mut self) -> i64 {
        10_000
    }

    /// Return deterministic jitter even though the successful fixture never retries.
    fn random_unit(&mut self) -> f64 {
        0.0
    }

    /// Return one admitted decoded response and its physical evidence.
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<OpenMeteoPreviousRunsPayload>, ProviderAttemptError> {
        assert_eq!(attempt.attempt, 1);
        Ok(ProviderAttemptResponse {
            body: self.payload.clone(),
            content: ContentBudget {
                declared_body_bytes: Some(4_096),
                streamed_body_bytes: 4_096,
            },
            provider_reason: None,
            retry_after_ms: None,
            status: 201,
        })
    }

    /// Reject sleep because the response succeeds on its first attempt.
    fn sleep(&mut self, _sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
        unreachable!("successful Previous Runs fixture does not sleep")
    }
}

struct FixtureTransport {
    payload: OpenMeteoPreviousRunsPayload,
    queries: Rc<RefCell<Vec<OpenMeteoPreviousRunsQuery>>>,
}

impl OpenMeteoPreviousRunsTransport for FixtureTransport {
    type Exchange<'a> = FixtureExchange;

    /// Capture the exact ordered query before returning a decoded exchange.
    fn bind_previous_runs(
        &mut self,
        query: &OpenMeteoPreviousRunsQuery,
    ) -> Result<Self::Exchange<'_>, ForecastAnchorBackfillOperationError> {
        self.queries.borrow_mut().push(query.clone());
        Ok(FixtureExchange {
            payload: self.payload.clone(),
        })
    }
}

struct FixtureValues;

impl OpenMeteoPreviousRunsValues for FixtureValues {
    /// Match JavaScript spelling for the two finite fixture coordinates.
    fn number_text(&mut self, value: f64) -> String {
        value.to_string()
    }

    /// Admit the exact 24 ordered fixture hours without adding a date parser.
    fn utc_hour(
        &mut self,
        value: &str,
    ) -> Result<(String, i64), ForecastAnchorBackfillOperationError> {
        let hour = value
            .strip_prefix("2026-03-08T")
            .and_then(|value| value.strip_suffix(":00"))
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|hour| (0..24).contains(hour))
            .expect("fixture UTC hour");
        Ok((
            format!("2026-03-08T{hour:02}:00:00.000Z"),
            START_MS + hour * 3_600_000,
        ))
    }

    /// Return the exact receipt instant used by the compiled Node fixture.
    fn received_at(&mut self) -> Result<String, ForecastAnchorBackfillOperationError> {
        Ok("2026-08-22T05:20:00.000Z".to_owned())
    }
}

/// Compare the actual typed operation with a compiled original Node witness.
#[test]
fn previous_runs_query_retry_and_projection_match_compiled_node() {
    let node = Command::new("node")
        .arg(node_oracle())
        .arg("--summary")
        .output()
        .expect("run compiled Node Previous Runs oracle");
    assert!(
        node.status.success(),
        "{}",
        String::from_utf8_lossy(&node.stderr)
    );
    assert_eq!(
        String::from_utf8(node.stdout).expect("UTF-8 Node summary"),
        concat!(
            "parameters=10\n",
            "hourlyFields=63\n",
            "attempts=1\n",
            "recordCount=1\n",
            "requested=168\n",
            "populated=1\n",
            "status=201\n",
            "generationMs=1.25\n",
            "first=24|10|0|2026-03-08T00:00:00.000Z\n",
        )
    );

    let queries = Rc::new(RefCell::new(Vec::new()));
    let transport = FixtureTransport {
        payload: OpenMeteoPreviousRunsPayload {
            body: response_body(),
            checksum: Sha256Digest::admitted("d".repeat(64)),
        },
        queries: Rc::clone(&queries),
    };
    let mut operation = OpenMeteoPreviousRunsOperation::new(
        transport,
        FixtureValues,
        None,
        ProviderControls {
            max_attempts: 1,
            max_body_bytes: 2_000_000,
            timeout_ms: 10_000,
        },
    );
    let request = request();
    let payload = operation
        .fetch(request.clone())
        .expect("typed provider payload");
    let batch = normalize_forecast_anchor_payload(&request, payload).expect("normalized anchors");
    let query = &queries.borrow()[0];

    assert_eq!(
        query.endpoint,
        OpenMeteoPreviousRunsEndpoint::Official(
            "https://previous-runs-api.open-meteo.com/v1/forecast"
        )
    );
    assert_eq!(query.parameters.len(), 10);
    assert_eq!(query.parameters[2].name, "hourly");
    assert_eq!(query.parameters[2].value.split(',').count(), 63);
    assert_eq!(batch.attempts, 1);
    assert_eq!(batch.records.len(), 1);
    assert_eq!(batch.records[0].leadHours, FixedForecastLeadHours::Value24);
    assert_eq!(batch.records[0].metrics.temperatureC, Nullable::Value(10.0));
    assert_eq!(
        batch.records[0].metrics.windDirectionDegrees,
        Nullable::Value(0.0)
    );
    assert_eq!(batch.response_metadata.requested_anchor_count, 168);
    assert_eq!(batch.response_metadata.populated_anchor_count, 1);
    assert_eq!(batch.response_metadata.http_status, 201);
    assert_eq!(batch.response_metadata.generation_ms, 1.25);
}

/// Build the exact one-day worker request consumed by both implementations.
fn request() -> ForecastAnchorProviderRequest {
    ForecastAnchorProviderRequest {
        contract_epoch: FORECAST_ANCHOR_CONTRACT_EPOCH,
        deadline_at_ms: 100_000,
        end_date: "2026-03-08".to_owned(),
        end_exclusive_ms: END_MS,
        locations: vec![ForecastAnchorLocation {
            latitude: 48.1,
            longitude: -122.7,
            source_config_fingerprint: "a".repeat(64),
            source_id: "source-previous-runs".to_owned(),
        }],
        start_date: "2026-03-08".to_owned(),
        start_ms: START_MS,
    }
}

/// Build one decoded provider object with 63 aligned arrays and one populated lead.
fn response_body() -> JsonValue {
    let times = (0..24)
        .map(|hour| string(&format!("2026-03-08T{hour:02}:00")))
        .collect();
    let mut hourly = vec![(key("time"), JsonValue::Array(times))];
    let mut units = vec![(key("time"), string("iso8601"))];

    // retain original base-major then lead order in both objects
    for (base, unit, first_value) in BASES {
        // materialize every fixed lead, with only day one at the first hour populated
        for day in 1..=7 {
            let field = format!("{base}_previous_day{day}");
            let mut values = vec![JsonValue::Null; 24];

            // reproduce the one populated first anchor from the Node oracle
            if day == 1 {
                values[0] = JsonValue::Number(first_value);
            }
            hourly.push((key(&field), JsonValue::Array(values)));
            units.push((key(&field), string(unit)));
        }
    }
    JsonValue::Object(vec![
        (key("elevation"), JsonValue::Number(12.5)),
        (key("generationtime_ms"), JsonValue::Number(1.25)),
        (key("hourly"), JsonValue::Object(hourly)),
        (key("hourly_units"), JsonValue::Object(units)),
        (key("latitude"), JsonValue::Number(48.1)),
        (key("longitude"), JsonValue::Number(-122.7)),
        (key("timezone"), string("UTC")),
        (key("utc_offset_seconds"), JsonValue::Number(0.0)),
    ])
}

/// Resolve the repository-relative compiled Node oracle path.
fn node_oracle() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/open-meteo-previous-runs-provider-node-oracle.mjs")
}

/// Encode one generated JavaScript string key.
fn key(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// Encode one generated JavaScript string value.
fn string(value: &str) -> JsonValue {
    JsonValue::String(key(value))
}
