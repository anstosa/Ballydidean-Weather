use std::cmp::Ordering;

use weather_contracts::{
    CanonicalWeatherMetrics, ForecastAdjustmentMetric, ForecastAdjustmentRawForecastProvenance,
    ForecastLeadBandKey, JsonValue, Nullable,
};
use weather_database::Sha256Digest;

use crate::forecast_training_package_row::ForecastTrainingPackageRowTime;
use crate::maintenance_canonical_json::{MaintenanceCanonicalJson, field};
use crate::temperature_performance_target::TemperaturePerformanceTargetValues;
use crate::wind_performance_common::{WindPerformanceBand, WindPerformanceError};
use crate::wind_performance_counterfactual::{
    WindPerformanceCounterfactualValues, WindPerformanceReplayDecision,
    collect_counterfactual_wind_pairs,
};

const VALID_AT: &str = "2026-10-08T13:00:00.000Z";
const REFERENCE_AT: &str = "2026-10-08T06:00:00.000Z";
const ROW_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct Values {
    encoded: Vec<JsonValue>,
    replays: Vec<(
        String,
        CanonicalWeatherMetrics,
        ForecastAdjustmentRawForecastProvenance,
    )>,
}

impl MaintenanceCanonicalJson for Values {
    /// keep parsing outside this already-decoded fixture
    fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
        None
    }

    /// capture each exact counterfactual identity preimage
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        self.encoded.push(value.clone());
        Some(b"row\n".to_vec())
    }

    /// return one deterministic identity for the delegated fixture bytes
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        (bytes == b"row\n").then(|| Sha256Digest::admitted(ROW_HASH.to_owned()))
    }
}

impl ForecastTrainingPackageRowTime for Values {
    /// parse only the fixture's verified valid instant
    fn utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        (value == VALID_AT).then_some(1_760_000_000_000)
    }

    /// leave unused UTC rendering outside this adapter fixture
    fn canonical_utc(&self, _: i64) -> Option<String> {
        None
    }

    /// return the fixture's expected Los Angeles calendar label
    fn los_angeles_local_date(&self, _: i64) -> Option<String> {
        Some("2026-10-08".to_owned())
    }
}

impl TemperaturePerformanceTargetValues for Values {
    /// preserve ordinary ASCII locale ordering in target code
    fn compare_js_locale(&self, left: &str, right: &str) -> Ordering {
        left.cmp(right)
    }
}

impl WindPerformanceCounterfactualValues for Values {
    /// capture the admitted native request and return one active fixture decision
    fn replay_admitted_wind_forecast(
        &mut self,
        generated_at: &str,
        metrics: &CanonicalWeatherMetrics,
        provenance: &ForecastAdjustmentRawForecastProvenance,
    ) -> Result<Option<WindPerformanceReplayDecision>, WindPerformanceError> {
        self.replays
            .push((generated_at.to_owned(), metrics.clone(), provenance.clone()));
        Ok(Some(WindPerformanceReplayDecision {
            lead_band: ForecastLeadBandKey::Value001024,
            adjusted_wind_speed_mps: Some(11.0),
            adjusted_wind_gust_mps: None,
        }))
    }
}

/// preserve raw lineage, mask filtering, fallback, identity, and unsupported-lead gaps
#[test]
fn builds_counterfactual_pairs_from_active_live_v4_rows() {
    let forecast_rows = vec![forecast_row(6.0), forecast_row(1.5)];
    let mask = vec![
        band(ForecastAdjustmentMetric::WindSpeedMps),
        band(ForecastAdjustmentMetric::WindGustMps),
        band(ForecastAdjustmentMetric::WindDirectionDegrees),
    ];
    let mut values = Values::default();
    let result = collect_counterfactual_wind_pairs(
        &forecast_rows,
        &[],
        &mask,
        "2026-10-09T00:00:00.000Z",
        &mut values,
    )
    .expect("ordinary live-v4 rows should produce native counterfactual pairs");

    assert_eq!(values.replays.len(), 1);
    let (generated_at, metrics, provenance) = &values.replays[0];
    assert_eq!(generated_at, "2026-10-09T00:00:00.000Z");
    assert_eq!(metrics.relativeHumidityPercent, Nullable::Value(75.0));
    assert_eq!(metrics.temperatureC, Nullable::Value(9.0));
    assert_eq!(metrics.windDirectionDegrees, Nullable::Null);
    assert_eq!(metrics.windSpeedMps, Nullable::Value(10.0));
    assert_eq!(metrics.windGustMps, Nullable::Value(20.0));
    assert_eq!(provenance.adapterVersion, "adapter/v4");
    assert_eq!(provenance.contractEpoch, "weather/v4");
    assert_eq!(provenance.dataset, "best-match");
    assert_eq!(provenance.referenceAt, REFERENCE_AT);
    assert_eq!(provenance.sourceConfigFingerprint, "source-fingerprint");
    assert_eq!(provenance.sourceKey, "source-key");
    assert_eq!(provenance.targetLeadHours, 6);
    assert_eq!(provenance.upstreamModel, "best-match");
    assert_eq!(provenance.validAt, VALID_AT);

    let speed = &result["windSpeedMps:001-024"];
    assert_eq!(speed.len(), 1);
    assert_eq!(speed[0].adjustedPrediction, 11.0);
    assert!(!speed[0].fallback);
    assert_eq!(speed[0].rawPrediction, 10.0);
    assert_eq!(speed[0].localDate, "2026-10-08");
    assert_eq!(speed[0].rowIdentity, ROW_HASH);
    assert_eq!(speed[0].target, Nullable::Null);
    assert_eq!(
        speed[0].targetKey,
        format!("missing:{VALID_AT}:windSpeedMps")
    );
    assert_eq!(speed[0].vintageKey, REFERENCE_AT);

    let gust = &result["windGustMps:001-024"];
    assert_eq!(gust.len(), 1);
    assert_eq!(gust[0].adjustedPrediction, 20.0);
    assert!(gust[0].fallback);
    assert_eq!(gust[0].rawPrediction, 20.0);
    assert!(result["windDirectionDegrees:001-024"].is_empty());
    assert_eq!(values.encoded.len(), 2);
    assert_eq!(
        field(&values.encoded[0], "referenceAt"),
        Some(&string(REFERENCE_AT))
    );
}

/// build one exact active mask member
fn band(metric: ForecastAdjustmentMetric) -> WindPerformanceBand {
    WindPerformanceBand {
        metric,
        lead_band: ForecastLeadBandKey::Value001024,
    }
}

/// build one verified live-v4 retrieval row with the requested finite lead
fn forecast_row(target_lead_hours: f64) -> JsonValue {
    object(vec![
        (
            "adapter_contracts",
            JsonValue::Array(vec![string("adapter/v4")]),
        ),
        (
            "content_hashes",
            object(vec![("forecast", string("forecast-content"))]),
        ),
        ("contract_epoch", string("weather/v4")),
        ("dataset", string("best-match")),
        ("record_kind", string("legacy_v4_retrieval_snapshot")),
        ("reference_at", string(REFERENCE_AT)),
        ("relative_humidity_percent", JsonValue::Number(75.0)),
        (
            "source_config_fingerprints",
            JsonValue::Array(vec![string("source-fingerprint")]),
        ),
        ("source_keys", JsonValue::Array(vec![string("source-key")])),
        ("target_lead_hours", JsonValue::Number(target_lead_hours)),
        ("temperature_c", JsonValue::Number(9.0)),
        ("upstream_model", string("best-match")),
        ("valid_at", string(VALID_AT)),
        ("wind_direction_degrees", JsonValue::Null),
        ("wind_gust_mps", JsonValue::Number(20.0)),
        ("wind_speed_mps", JsonValue::Number(10.0)),
    ])
}

/// build one ordered fixture object
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// build one exact UTF-16 fixture string
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
