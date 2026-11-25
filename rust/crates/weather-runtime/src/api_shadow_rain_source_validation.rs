//! Validation of the complete decoded rain causal source graph.

use std::collections::HashSet;

use weather_contracts::JsonValue;

use crate::ApiMaintenanceReceiptCanonicalValues;
use crate::api_shadow_wire_validation::{
    binary64_in, bounded_source_string, hash, instant, ordered_keys, rows, unsigned,
};
use crate::maintenance_canonical_json::{field, text};

const CAUSAL_KEYS: &[&str] = &[
    "contractVersion",
    "captureSet",
    "currentRun",
    "priorRuns",
    "stationHours",
];
const CAPTURE_KEYS: &[&str] = &[
    "claimId",
    "kind",
    "stationId",
    "runInitializedAt",
    "windowStart",
    "windowEndExclusive",
    "completedAt",
    "bodySha256",
];
const RUN_KEYS: &[&str] = &["runInitializedAt", "completedAt", "contentSha256", "hours"];
const RUN_HOUR_KEYS: &[&str] = &[
    "leadHours",
    "precipitationMm64",
    "temperatureC64",
    "relativeHumidityPercent64",
    "cloudCoverPercent64",
    "pressureHpa64",
    "windSpeedMps64",
    "windDirectionDegrees64",
];
const STATION_HOUR_KEYS: &[&str] = &[
    "hourAt",
    "receivedAt",
    "stationId",
    "precipitationMm64",
    "temperatureC64",
];

/// Validate every ordered source value and raw capture identity consumed by rain inference.
pub(crate) fn validate_rain_causal_inputs(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    node: &JsonValue,
    issued_ms: i64,
) -> Option<()> {
    ordered_keys(node, CAUSAL_KEYS)?;
    let capture_set = rows(node, "captureSet")?;
    let prior_runs = rows(node, "priorRuns")?;
    let station_hours = rows(node, "stationHours")?;
    // require the one frozen compiled-input contract and bounded fan-in
    if text(node, "contractVersion")? != "adjustment-shadow-rain-causal-inputs/v1"
        || !(1..=350).contains(&capture_set.len())
        || prior_runs.len() > 2
        || station_hours.len() > 64
    {
        return None;
    }

    let mut capture_hashes = HashSet::with_capacity(capture_set.len());
    let mut capture_claims = HashSet::with_capacity(capture_set.len());
    // validate each exact retained provider capture identity in canonical order
    for capture in capture_set {
        ordered_keys(capture, CAPTURE_KEYS)?;
        let claim = bounded_source_string(field(capture, "claimId")?)?;
        let completed_at = instant(values, capture, "completedAt")?;
        let body_sha256 = hash(capture, "bodySha256")?;
        let kind = text(capture, "kind")?;
        let station_id = nullable_unsigned(capture, "stationId")?;
        let run_initialized_at = nullable_instant(values, capture, "runInitializedAt")?;
        let window_start = nullable_instant(values, capture, "windowStart")?;
        let window_end_exclusive = nullable_instant(values, capture, "windowEndExclusive")?;
        let shape_matches = match kind.as_str() {
            "forecast" => {
                station_id.is_none()
                    && run_initialized_at.is_some()
                    && window_start.is_none()
                    && window_end_exclusive.is_none()
            }
            "station" => {
                station_id.is_some_and(|station_id| station_id >= 1)
                    && run_initialized_at.is_none()
                    && window_start.is_some()
                    && window_end_exclusive.is_some()
            }
            _ => false,
        };
        // retain Node's category relations without adding window ordering constraints
        if !shape_matches || completed_at > issued_ms || !capture_claims.insert(claim) {
            return None;
        }
        capture_hashes.insert(body_sha256);
    }

    validate_rain_run(
        values,
        field(node, "currentRun")?,
        issued_ms,
        &capture_hashes,
    )?;
    // validate each causal prior forecast run in source order
    for run in prior_runs {
        validate_rain_run(values, run, issued_ms, &capture_hashes)?;
    }

    let mut station_keys = HashSet::with_capacity(station_hours.len());
    // validate exact derived station inputs passed to the compiled feature builder
    for hour in station_hours {
        ordered_keys(hour, STATION_HOUR_KEYS)?;
        let hour_at = instant(values, hour, "hourAt")?;
        let received_at = instant(values, hour, "receivedAt")?;
        let station_id = unsigned(hour, "stationId")?;
        // reject only future receipts, duplicate keys, and nonpositive station identities
        if station_id < 1 || received_at > issued_ms || !station_keys.insert((station_id, hour_at))
        {
            return None;
        }
        binary64_in(hour, "precipitationMm64", 0.0, 500.0, true)?;
        binary64_in(hour, "temperatureC64", -100.0, 70.0, true)?;
    }
    Some(())
}

/// Validate one exact 48-hour forecast input profile.
fn validate_rain_run(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    node: &JsonValue,
    issued_ms: i64,
    capture_hashes: &HashSet<&[u16]>,
) -> Option<()> {
    ordered_keys(node, RUN_KEYS)?;
    instant(values, node, "runInitializedAt")?;
    let completed_at = instant(values, node, "completedAt")?;
    let content_sha256 = hash(node, "contentSha256")?;
    let hours = rows(node, "hours")?;
    // require the raw capture hash, before-issue receipt, and full geometry
    if !capture_hashes.contains(content_sha256) || completed_at > issued_ms || hours.len() != 48 {
        return None;
    }
    // validate all ordered forecast covariates consumed by the compiled artifact
    for (index, hour) in hours.iter().enumerate() {
        ordered_keys(hour, RUN_HOUR_KEYS)?;
        // preserve exact one-based lead order
        if unsigned(hour, "leadHours")? != u64::try_from(index).ok()?.checked_add(1)? {
            return None;
        }
        binary64_in(hour, "precipitationMm64", 0.0, 2_000.0, true)?;
        binary64_in(hour, "temperatureC64", -100.0, 70.0, true)?;
        binary64_in(hour, "relativeHumidityPercent64", 0.0, 100.0, true)?;
        binary64_in(hour, "cloudCoverPercent64", 0.0, 100.0, true)?;
        binary64_in(hour, "pressureHpa64", 100.0, 1_200.0, true)?;
        binary64_in(hour, "windSpeedMps64", 0.0, 150.0, true)?;
        binary64_in(hour, "windDirectionDegrees64", 0.0, 360.0, true)?;
    }
    Some(())
}

/// Decode one exact nullable positive-integer candidate without coercion.
fn nullable_unsigned(node: &JsonValue, key: &str) -> Option<Option<u64>> {
    match field(node, key)? {
        JsonValue::Null => Some(None),
        JsonValue::Number(_) => unsigned(node, key).map(Some),
        _ => None,
    }
}

/// Decode one exact nullable canonical clock without adding causal comparisons.
fn nullable_instant(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    node: &JsonValue,
    key: &str,
) -> Option<Option<i64>> {
    match field(node, key)? {
        JsonValue::Null => Some(None),
        JsonValue::String(_) => instant(values, node, key).map(Some),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INITIALIZED: &str = "2026-01-01T00:00:00.000Z";
    const COMPLETED: &str = "2026-01-01T00:30:00.000Z";
    const ISSUED: &str = "2026-01-01T01:00:00.000Z";

    struct Values;

    impl ApiMaintenanceReceiptCanonicalValues for Values {
        /// Resolve only the canonical clocks retained by the complete rain fixture.
        fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
            match value {
                INITIALIZED => Some(0),
                COMPLETED => Some(1_800_000),
                ISSUED => Some(3_600_000),
                _ => None,
            }
        }

        /// Keep hashing outside this already decoded causal-graph fixture.
        fn sha256(&self, _bytes: &[u8]) -> Option<weather_database::Sha256Digest> {
            None
        }

        /// Keep canonical receipt hashing outside this causal-graph fixture.
        fn unsigned_receipt_sha256(
            &self,
            _fields: &[(&str, crate::MaintenanceReceiptScalar<'_>)],
        ) -> Option<weather_database::Sha256Digest> {
            None
        }
    }

    /// Accept one complete captured run and station row while retaining raw UTF-16 claim identity.
    #[test]
    fn validates_complete_captured_run_and_optional_station_row() {
        let graph = rain_graph('a', 48);
        assert!(validate_rain_causal_inputs(&Values, &graph, 3_600_000).is_some());
    }

    /// Reject a complete run whose content is absent from the retained capture membership.
    #[test]
    fn rejects_run_with_changed_capture_content() {
        let graph = rain_graph('b', 48);
        assert!(validate_rain_causal_inputs(&Values, &graph, 3_600_000).is_none());
    }

    /// Build one exact ordered causal graph with a parameterized capture identity.
    fn rain_graph(capture_hash: char, hour_count: usize) -> JsonValue {
        let run_hash = "a".repeat(64);
        object(vec![
            (
                "contractVersion",
                string("adjustment-shadow-rain-causal-inputs/v1"),
            ),
            (
                "captureSet",
                JsonValue::Array(vec![object(vec![
                    // preserve a legitimate JavaScript string that cannot become Rust UTF-8
                    ("claimId", JsonValue::String(vec![0xd800, u16::from(b'x')])),
                    ("kind", string("forecast")),
                    ("stationId", JsonValue::Null),
                    ("runInitializedAt", string(INITIALIZED)),
                    ("windowStart", JsonValue::Null),
                    ("windowEndExclusive", JsonValue::Null),
                    ("completedAt", string(COMPLETED)),
                    ("bodySha256", string(&capture_hash.to_string().repeat(64))),
                ])]),
            ),
            ("currentRun", rain_run(&run_hash, hour_count)),
            ("priorRuns", JsonValue::Array(Vec::new())),
            (
                "stationHours",
                JsonValue::Array(vec![object(vec![
                    ("hourAt", string(INITIALIZED)),
                    ("receivedAt", string(COMPLETED)),
                    ("stationId", JsonValue::Number(7.0)),
                    ("precipitationMm64", JsonValue::Null),
                    ("temperatureC64", JsonValue::Null),
                ])]),
            ),
        ])
    }

    /// Build one exact ordered forecast run with complete one-based lead geometry.
    fn rain_run(content_sha256: &str, hour_count: usize) -> JsonValue {
        JsonValue::Object(vec![
            utf16_field("runInitializedAt", string(INITIALIZED)),
            utf16_field("completedAt", string(COMPLETED)),
            utf16_field("contentSha256", string(content_sha256)),
            utf16_field(
                "hours",
                JsonValue::Array((1..=hour_count).map(rain_hour).collect()),
            ),
        ])
    }

    /// Build one exact ordered nullable covariate row.
    fn rain_hour(lead: usize) -> JsonValue {
        object(vec![
            ("leadHours", JsonValue::Number(lead as f64)),
            ("precipitationMm64", JsonValue::Null),
            ("temperatureC64", JsonValue::Null),
            ("relativeHumidityPercent64", JsonValue::Null),
            ("cloudCoverPercent64", JsonValue::Null),
            ("pressureHpa64", JsonValue::Null),
            ("windSpeedMps64", JsonValue::Null),
            ("windDirectionDegrees64", JsonValue::Null),
        ])
    }

    /// Build one ordered decoded JSON object.
    fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
        JsonValue::Object(
            fields
                .into_iter()
                .map(|(key, value)| utf16_field(key, value))
                .collect(),
        )
    }

    /// Build one decoded object field with its exact UTF-16 name.
    fn utf16_field(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
        (key.encode_utf16().collect(), value)
    }

    /// Build one decoded UTF-16 JSON string.
    fn string(value: &str) -> JsonValue {
        JsonValue::String(value.encode_utf16().collect())
    }
}
