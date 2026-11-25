use std::cell::RefCell;
use std::fmt::{self, Display};
use std::rc::Rc;

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use super::*;
use crate::{MaintenanceCanonicalJson, RainPerformanceEdgeRowsValues};

const FORECAST_MANIFEST_SHA256: &str =
    "a40d796cc4ec1f8df48771fe4a42227059c1f8ba0f1a6c87009d1a2c9c4f0dad";
const FORECAST_MEMBER_SHA256: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Debug, Clone, Copy)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Expose deterministic effect failures to the loader boundary.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

#[derive(Clone)]
struct Values {
    events: Rc<RefCell<Vec<String>>>,
    adjustment_manifest: JsonValue,
    forecast_manifest: JsonValue,
    verifier_digest: String,
}

impl Values {
    /// Build one matching package fixture with shared ordered event capture.
    fn matching() -> Self {
        Self {
            events: Rc::new(RefCell::new(Vec::new())),
            adjustment_manifest: adjustment_manifest("2026-10-01"),
            forecast_manifest: forecast_manifest("2026-10-01"),
            verifier_digest: FORECAST_MANIFEST_SHA256.to_owned(),
        }
    }

    /// Record one externally owned operation in exact call order.
    fn record(&self, event: impl Into<String>) {
        self.events.borrow_mut().push(event.into());
    }
}

impl MaintenanceCanonicalJson for Values {
    /// Keep canonical object parsing outside this generic JSON loader fixture.
    fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
        None
    }

    /// Keep edge row serialization unreachable for the empty edge fixture.
    fn encode_canonical_json(&mut self, _: &JsonValue) -> Option<Vec<u8>> {
        None
    }

    /// Return the exact fixture identity for each immutable byte population.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        let digest = match bytes {
            b"forecast-manifest" => FORECAST_MANIFEST_SHA256,
            b"gzip" => FORECAST_MEMBER_SHA256,
            _ => return None,
        };
        self.record(format!("hash:{digest}"));
        Some(Sha256Digest::admitted(digest))
    }
}

impl RainPerformanceTimestampValues for Values {
    /// Parse only the two sub-millisecond fixture instants.
    fn parse_js_timestamp_ms(&mut self, value: &[u16]) -> Option<i64> {
        let value = String::from_utf16(value).ok()?;
        self.record(format!("parse:{value}"));
        match value.as_str() {
            "2026-10-07T12:00:00.123456Z" => Some(1_000),
            "2026-10-01T20:00:00.123456Z" => Some(2_000),
            _ => None,
        }
    }

    /// Format the conservatively rounded availability instants.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Option<Vec<u16>> {
        self.record(format!("format:{epoch_ms}"));
        let value = match epoch_ms {
            1_001 => "2026-10-07T12:00:00.124Z",
            2_001 => "2026-10-01T20:00:00.124Z",
            _ => return None,
        };
        Some(value.encode_utf16().collect())
    }
}

impl RainPerformanceEdgeRowsValues for Values {
    /// Decode the manifest snapshot and one streamed forecast row.
    fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.record(format!("decode:{}", String::from_utf8_lossy(bytes)));
        match bytes {
            b"forecast-manifest" => Some(self.forecast_manifest.clone()),
            br#"{"received_at":"2026-10-01T20:00:00.123456Z"}"# => Some(object(vec![(
                "received_at",
                string("2026-10-01T20:00:00.123456Z"),
            )])),
            _ => None,
        }
    }

    /// Keep edge object inflation unreachable for the empty edge fixture.
    fn gunzip_bounded(&mut self, _: &[u8], _: usize) -> Option<Vec<u8>> {
        None
    }
}

impl RainPerformancePackageLoaderValues for Values {
    type Error = FixtureError;

    /// Resolve the two fixture roots while exposing their order.
    fn resolve_package_root(&mut self, raw: &str) -> Result<String, Self::Error> {
        self.record(format!("resolve:{raw}"));
        Ok(format!("/resolved/{raw}"))
    }

    /// Return the pre-verification manifest snapshot or declared member bytes.
    fn read_package_file(&mut self, root: &str, member_path: &str) -> Result<Vec<u8>, Self::Error> {
        self.record(format!("read:{root}:{member_path}"));
        match member_path {
            "manifest.json" => Ok(b"forecast-manifest".to_vec()),
            "members/fixture.jsonl.gz" => Ok(b"gzip".to_vec()),
            _ => Err(FixtureError("fixture member is unavailable")),
        }
    }

    /// Return one JSONL member body without implementing gzip in the loader.
    fn gunzip_forecast_member(&mut self, compressed: &[u8]) -> Result<Vec<u8>, Self::Error> {
        self.record(format!("gunzip:{}", String::from_utf8_lossy(compressed)));
        Ok(br#"{"received_at":"2026-10-01T20:00:00.123456Z"}
"#
        .to_vec())
    }
}

impl RainPerformanceLegacyPackageLoaderValues for Values {
    /// Return the configured verifier result after the manifest snapshot read.
    fn verify_forecast_package(&mut self, root: &str) -> Result<String, Self::Error> {
        self.record(format!("verify:{root}"));
        Ok(self.verifier_digest.clone())
    }

    /// return one opaque already-verified adjustment bundle
    fn load_verified_adjustment_package(
        &mut self,
        root: &str,
    ) -> Result<VerifiedRainPerformanceAdjustmentPackage, Self::Error> {
        self.record(format!("load-adjustment:{root}"));
        Ok(VerifiedRainPerformanceAdjustmentPackage {
            bodies: vec![AdmittedRainArchiveBody {
                member_path: "bodies/fixture.json.gz".to_owned(),
                compressed: vec![7, 8, 9],
            }],
            edge_members: Vec::new(),
            manifest: self.adjustment_manifest.clone(),
            manifest_sha256: "adjustment-manifest".to_owned(),
            rows: vec![adjustment_claim_row()],
        })
    }
}

/// Admit one matched pair in exact custody order without retaining forecast rows.
#[test]
fn loads_one_verified_pair_and_streams_forecast_row_normalization() {
    let mut values = Values::matching();
    let events = values.events.clone();
    let pair = load_verified_rain_performance_package_pair("forecast", "adjustment", &mut values)
        .expect("matching verifier-bound packages should load");

    assert_eq!(
        pair.inputs,
        RainPerformanceReportPackageInputs {
            adjustment_evidence_manifest_sha256: "adjustment-manifest".to_owned(),
            adjustment_evidence_watermark_sha256: "watermark".to_owned(),
            forecast_training_manifest_sha256: FORECAST_MANIFEST_SHA256.to_owned(),
            local_date_from: "2026-10-01".to_owned(),
            local_date_to: "2026-10-01".to_owned(),
            target_cutoff_at: "2026-10-07T12:00:00.124Z".to_owned(),
        }
    );
    assert_eq!(pair.bodies[0].compressed, vec![7, 8, 9]);
    assert!(pair.claims.is_empty());
    assert!(pair.edge_rows.is_empty());
    assert!(pair.receipts.is_empty());
    assert!(pair.runs.is_empty());
    assert_eq!(
        pair.normalized_adjustment_rows,
        vec![adjustment_claim_row()]
    );
    let expected_events = vec![
        "resolve:forecast".to_owned(),
        "resolve:adjustment".to_owned(),
        "read:/resolved/forecast:manifest.json".to_owned(),
        "verify:/resolved/forecast".to_owned(),
        format!("hash:{FORECAST_MANIFEST_SHA256}"),
        "decode:forecast-manifest".to_owned(),
        "load-adjustment:/resolved/adjustment".to_owned(),
        "read:/resolved/forecast:members/fixture.jsonl.gz".to_owned(),
        format!("hash:{FORECAST_MEMBER_SHA256}"),
        "gunzip:gzip".to_owned(),
        "decode:{\"received_at\":\"2026-10-01T20:00:00.123456Z\"}".to_owned(),
        "parse:2026-10-01T20:00:00.123456Z".to_owned(),
        "format:2001".to_owned(),
        "parse:2026-10-07T12:00:00.123456Z".to_owned(),
        "format:1001".to_owned(),
    ];
    assert_eq!(events.borrow().as_slice(), expected_events.as_slice());
}

/// Reject a cross-date pair before timestamps, adjustment rows, edges, or forecast members open.
#[test]
fn rejects_mismatched_cohort_before_opening_scored_rows() {
    let mut values = Values::matching();
    values.adjustment_manifest = adjustment_manifest("2026-10-02");
    let events = values.events.clone();

    let error = load_verified_rain_performance_package_pair("forecast", "adjustment", &mut values)
        .expect_err("mismatched intervals must fail closed");

    assert_eq!(error, RainPerformancePackageLoaderError::CohortMismatch);
    assert_eq!(
        events.borrow().last().map(String::as_str),
        Some("load-adjustment:/resolved/adjustment")
    );
}

/// Reject a manifest digest mismatch before parsing or loading the adjustment package.
#[test]
fn binds_verifier_digest_to_the_preverification_manifest_snapshot() {
    let mut values = Values::matching();
    values.verifier_digest =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned();
    let events = values.events.clone();

    let error = load_verified_rain_performance_package_pair("forecast", "adjustment", &mut values)
        .expect_err("a verifier digest for other bytes must fail closed");

    assert_eq!(
        error,
        RainPerformancePackageLoaderError::ForecastManifestChanged
    );
    assert_eq!(
        events.borrow().last().map(String::as_str),
        Some("hash:a40d796cc4ec1f8df48771fe4a42227059c1f8ba0f1a6c87009d1a2c9c4f0dad")
    );
}

/// Build one minimal forecast manifest for the original matched-pair fields.
fn forecast_manifest(local_date: &str) -> JsonValue {
    object(vec![
        ("createdAtUtc", string("2026-10-07T12:00:00.123456Z")),
        ("fromLocalDate", string(local_date)),
        (
            "members",
            JsonValue::Array(vec![object(vec![
                ("path", string("members/fixture.jsonl.gz")),
                ("sha256", string(FORECAST_MEMBER_SHA256)),
                ("sizeBytes", JsonValue::Number(4.0)),
            ])]),
        ),
        ("siteKey", string("ballydidean")),
        ("toLocalDate", string(local_date)),
    ])
}

/// Build one minimal opaque adjustment manifest for pair admission.
fn adjustment_manifest(local_date: &str) -> JsonValue {
    object(vec![
        (
            "edgeEvidence",
            object(vec![("watermarkSha256", string("watermark"))]),
        ),
        ("fromLocalDate", string(local_date)),
        ("siteKey", string("ballydidean")),
        ("toLocalDate", string(local_date)),
    ])
}

/// Build one verifier-returned adjustment row that projects into the typed archive pair.
fn adjustment_claim_row() -> JsonValue {
    object(vec![
        ("record_kind", string("rain_claim")),
        (
            "payload",
            object(vec![
                ("id", string("forecast-claim")),
                ("kind", string("forecast")),
                ("runInitializedAt", JsonValue::Null),
                ("stationId", JsonValue::Null),
                ("windowEndExclusive", JsonValue::Null),
                ("windowStart", JsonValue::Null),
            ]),
        ),
    ])
}

/// Build one dependency-free JSON object with exact UTF-16 keys.
fn object(entries: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one dependency-free JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
