//! Compare the real archive provider operation with the compiled Node contract.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{JsonValue, Nullable, SourceKind};
use weather_database::Sha256Digest;
use weather_providers::{
    ContentBudget, ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse,
    ProviderControls, ProviderIo, ProviderSleep, ProviderSleepError,
};
use weather_runtime::{
    OpenMeteoArchiveEndpoint, OpenMeteoArchiveOperation, OpenMeteoArchivePayload,
    OpenMeteoArchiveQuery, OpenMeteoArchiveQueryValue, OpenMeteoArchiveTransport,
    OpenMeteoArchiveValues,
};
use weather_worker::{
    OpenMeteoArchiveInstant, OpenMeteoArchiveProvider, OpenMeteoArchiveRequest,
    TideBackfillOperationError,
};

const START_MS: i64 = 1_772_956_800_000;
const END_MS: i64 = 1_773_039_600_000;
const RECEIVED_AT: &str = "2026-08-22T05:20:00.000Z";
const CHECKSUM: &str = "a3d4f0a3a3d4f0a3a3d4f0a3a3d4f0a3a3d4f0a3a3d4f0a3a3d4f0a3a3d4f0a3";

/// Build exact decoded member order without parsing a fixture document.
fn object<const N: usize>(values: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        values
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one decoded scalar string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Expand one known metric column with explicit nulls.
fn column(values: [Option<f64>; 4]) -> JsonValue {
    JsonValue::Array(
        values
            .into_iter()
            .map(|value| value.map_or(JsonValue::Null, JsonValue::Number))
            .collect(),
    )
}

/// Supply UTC padding around the shortened local LA spring-forward day.
fn payload() -> JsonValue {
    object([
        ("latitude", JsonValue::Number(48.1)),
        ("longitude", JsonValue::Number(-122.7)),
        ("elevation", JsonValue::Number(12.5)),
        ("timezone", string("UTC")),
        ("utc_offset_seconds", JsonValue::Number(0.0)),
        ("generationtime_ms", JsonValue::Number(-1.25)),
        (
            "hourly",
            object([
                (
                    "time",
                    JsonValue::Array(
                        [
                            "2026-03-08T07:00",
                            "2026-03-08T08:00",
                            "2026-03-09T06:00",
                            "2026-03-09T07:00",
                        ]
                        .into_iter()
                        .map(string)
                        .collect(),
                    ),
                ),
                (
                    "temperature_2m",
                    column([Some(5.0), Some(0.0), None, Some(8.0)]),
                ),
                ("apparent_temperature", column([None; 4])),
                ("relative_humidity_2m", column([None; 4])),
                ("precipitation", column([None, None, Some(1.25), None])),
                ("cloud_cover", column([None; 4])),
                ("wind_speed_10m", column([None; 4])),
                ("wind_gusts_10m", column([None; 4])),
                (
                    "wind_direction_10m",
                    column([None, Some(360.0), None, None]),
                ),
                ("surface_pressure", column([None; 4])),
            ]),
        ),
        (
            "hourly_units",
            object([
                ("temperature_2m", string("°C")),
                ("apparent_temperature", string("°C")),
                ("relative_humidity_2m", string("%")),
                ("precipitation", string("mm")),
                ("cloud_cover", string("%")),
                ("wind_speed_10m", string("m/s")),
                ("wind_gusts_10m", string("m/s")),
                ("wind_direction_10m", string("°")),
                ("surface_pressure", string("hPa")),
            ]),
        ),
    ])
}

struct Values;

impl OpenMeteoArchiveValues for Values {
    /// Admit precisely the two original fixture calendar zones.
    fn validate_timezone(&mut self, timezone: &str) -> Result<(), TideBackfillOperationError> {
        assert!(matches!(timezone, "UTC" | "America/Los_Angeles"));
        Ok(())
    }

    /// Resolve only the four fixture instants and prove the previous instant is retained even across padding.
    fn resolve_provider_time(
        &mut self,
        wall_time: &str,
        timezone: &str,
        offset: i32,
        previous: Option<&OpenMeteoArchiveInstant>,
    ) -> Result<OpenMeteoArchiveInstant, TideBackfillOperationError> {
        assert_eq!((timezone, offset), ("UTC", 0));
        let (epoch_milliseconds, previous_ms) = match wall_time {
            "2026-03-08T07:00" => (START_MS - 3_600_000, None),
            "2026-03-08T08:00" => (START_MS, Some(START_MS - 3_600_000)),
            "2026-03-09T06:00" => (END_MS - 3_600_000, Some(START_MS)),
            "2026-03-09T07:00" => (END_MS, Some(END_MS - 3_600_000)),
            _ => panic!("unexpected fixture instant"),
        };
        assert_eq!(previous.map(|value| value.epoch_milliseconds), previous_ms);
        Ok(OpenMeteoArchiveInstant {
            epoch_milliseconds,
            utc: format!("{wall_time}:00.000Z"),
        })
    }

    /// Return the same receipt clock as the compiled provider.
    fn received_at(&mut self) -> Result<String, TideBackfillOperationError> {
        Ok(RECEIVED_AT.to_owned())
    }

    /// Render the fixture coordinates with their shared finite decimal spelling.
    fn number_text(&mut self, value: f64) -> String {
        value.to_string()
    }
}

#[derive(Default)]
struct Transport {
    query: Vec<String>,
}

impl OpenMeteoArchiveTransport for &mut Transport {
    type Exchange<'a>
        = Exchange
    where
        Self: 'a;

    /// Retain the exact admitted provider plan before creating the independent exchange.
    fn bind_archive(
        &mut self,
        query: &OpenMeteoArchiveQuery<'_>,
    ) -> Result<Exchange, TideBackfillOperationError> {
        let OpenMeteoArchiveEndpoint::Official(endpoint) = query.endpoint else {
            panic!("unexpected fixture compatibility origin");
        };
        self.query.push(endpoint.to_owned());
        // retain control order and leave URL encoding outside the fixture
        for (key, value) in query.parameters {
            let text = match value {
                OpenMeteoArchiveQueryValue::Text(value) => value.to_owned(),
                OpenMeteoArchiveQueryValue::Number(value) => value.to_string(),
            };
            self.query.push(format!("{key}\t{text}"));
        }
        Ok(Exchange)
    }
}

struct Exchange;

impl ProviderIo<OpenMeteoArchivePayload> for Exchange {
    /// Leave only two seconds on the request's absolute deadline.
    fn now_ms(&mut self) -> i64 {
        END_MS - 2_000
    }

    /// Return the unused deterministic jitter.
    fn random_unit(&mut self) -> f64 {
        0.0
    }

    /// Return original decoded evidence over the bounded retry path.
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<OpenMeteoArchivePayload>, ProviderAttemptError> {
        assert_eq!((attempt.attempt, attempt.timeout_ms), (1, 2_000));
        Ok(ProviderAttemptResponse {
            body: OpenMeteoArchivePayload {
                body: payload(),
                checksum: Sha256Digest::admitted(CHECKSUM),
            },
            content: ContentBudget {
                declared_body_bytes: Some(256),
                streamed_body_bytes: 256,
            },
            provider_reason: None,
            retry_after_ms: None,
            status: 201,
        })
    }

    /// Reject unexpected retry I/O on the successful fixture.
    fn sleep(&mut self, _: ProviderSleep) -> Result<(), ProviderSleepError> {
        unreachable!("successful archive exchange should not retry")
    }
}

/// Render selected existing nullable scalars for the oracle comparison.
fn scalar<T: ToString>(value: &Nullable<T>) -> String {
    match value {
        Nullable::Null => "null".to_owned(),
        Nullable::Value(value) => value.to_string(),
    }
}

/// View one original object member from the admitted generated DTO.
fn field(fields: &[(Vec<u16>, JsonValue)], key: &str) -> String {
    let value = fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
        .expect("field");
    match value {
        JsonValue::Number(value) => value.to_string(),
        JsonValue::String(value) => String::from_utf16(value).expect("field UTF-16"),
        _ => panic!("unexpected oracle field"),
    }
}

/// Compare original query, 23-hour window filtering, provenance, status and archive metrics.
#[test]
fn archive_provider_matches_compiled_node_for_spring_transition() {
    let mut transport = Transport::default();
    let mut operation =
        OpenMeteoArchiveOperation::new(&mut transport, Values, None, ProviderControls::default());
    let batch = operation
        .fetch(OpenMeteoArchiveRequest {
            deadline_at_ms: END_MS,
            end_date: "2026-03-08".to_owned(),
            end_exclusive_date: "2026-03-09".to_owned(),
            interval_end_exclusive_ms: END_MS,
            interval_start_ms: START_MS,
            latitude: 48.1,
            longitude: -122.7,
            source_id: "source-reanalysis".to_owned(),
            start_date: "2026-03-08".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        })
        .expect("archive provider");
    assert_eq!(batch.records.len(), 2);
    assert_eq!(batch.upstream_response_checksum, CHECKSUM);
    let JsonValue::Object(response_metadata) = batch.response_metadata else {
        panic!("metadata object");
    };
    let mut actual = transport.query.join("\n");
    write!(
        actual,
        "\n{}\tnull\t{}\t{}",
        batch.attempts,
        field(&response_metadata, "http_status"),
        field(&response_metadata, "generation_ms")
    )
    .expect("write");
    // compare all retained fixture rows in provider order
    for record in batch.records {
        let Nullable::Value(provider) = record.metadata.provider else {
            panic!("archive provenance");
        };
        assert_eq!(record.sourceKind, SourceKind::Reanalysis);
        let row = [
            record.validAt,
            record.receivedAt,
            "reanalysis".to_owned(),
            record.sourceId,
            scalar(&record.productRunAt),
            scalar(&record.metadata.model),
            field(&provider, "dataset"),
            field(&provider, "elevation_m"),
            field(&provider, "grid_cell"),
            record.metadata.upstreamTimezone,
            scalar(&record.metrics.temperatureC),
            scalar(&record.metrics.windDirectionDegrees),
            scalar(&record.metrics.precipitationMm),
            scalar(&record.metrics.precipitationRateMmPerHour),
        ];
        write!(actual, "\n{}", row.join("\t")).expect("write");
    }
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/open_meteo_archive_provider_node_oracle.mjs");
    let node = Command::new("node")
        .arg(oracle)
        .output()
        .expect("Node archive provider oracle");
    assert!(
        node.status.success(),
        "{}",
        String::from_utf8_lossy(&node.stderr)
    );
    assert_eq!(
        actual,
        String::from_utf8(node.stdout).expect("oracle output")
    );
}
