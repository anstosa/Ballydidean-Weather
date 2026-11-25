//! Capture-epoch admission for decoded maintenance shadow source documents.

use weather_api::{AdjustmentMaintenanceBodyError, CanonicalInstant};
use weather_contracts::JsonValue;

use crate::ApiMaintenanceReceiptCanonicalValues;
use crate::maintenance_canonical_json::{field, text as json_text};

const RECENT_ERROR_WINDOW_BACKTRACK_MS: i64 = 71 * 3_600_000;

/// Require every source clock consumed by one shadow evaluation at or after the capture epoch.
pub(crate) fn require_shadow_source_epoch(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    epoch: &CanonicalInstant,
    source: &JsonValue,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    let rows = array_field(source, "rows")?;
    // preserve source row order and reference-before-receipt clock order
    for row in rows {
        admit_clock(values, epoch, &text(row, "referenceAt")?)?;
        admit_clock(values, epoch, &text(row, "receivedAt")?)?;
    }

    let family = text(source, "family")?;
    // temperature additionally retains every selected best-match initialization
    if family == "temperature" {
        for row in rows {
            admit_clock(values, epoch, &text(row, "bestMatchProductRunAt")?)?;
        }
        // include the complete rolling window only when the optional state is present
        if let Some(state) = field(source, "recentErrorState") {
            admit_temperature_recent_error(values, epoch, state)?;
        }
    }

    // rain additionally binds every capture, model run, and station observation clock
    if family == "rain"
        && let Some(causal_inputs) = field(source, "causalInputs")
    {
        admit_rain_causal_inputs(values, epoch, causal_inputs)?;
    }
    // refuse decoded shapes outside the original three source families
    if !matches!(family.as_str(), "temperature" | "wind" | "rain") {
        return Err(AdjustmentMaintenanceBodyError);
    }
    Ok(())
}

/// Admit the complete rolling temperature state, including its oldest derived window clock.
fn admit_temperature_recent_error(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    epoch: &CanonicalInstant,
    state: &JsonValue,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    admit_clock(values, epoch, &text(state, "targetRunInitializedAt")?)?;
    let window_end = text(state, "windowEndValidAt")?;
    values
        .canonical_utc_epoch_milliseconds(&window_end)
        .and_then(|value| value.checked_sub(RECENT_ERROR_WINDOW_BACKTRACK_MS))
        .filter(|value| *value >= epoch.epoch_milliseconds)
        .map(|_| ())
        .ok_or(AdjustmentMaintenanceBodyError)?;
    admit_optional_clock(values, epoch, state, "maximumSourceRunInitializedAt")?;
    admit_optional_clock(values, epoch, state, "maximumSourceValidAt")
}

/// Admit every causal rain clock in the original capture, run, and station order.
fn admit_rain_causal_inputs(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    epoch: &CanonicalInstant,
    causal_inputs: &JsonValue,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    let captures = array_field(causal_inputs, "captureSet")?;
    // preserve capture-set order and its mutually exclusive clock branch
    for capture in captures {
        admit_clock(values, epoch, &text(capture, "completedAt")?)?;
        match text(capture, "kind")?.as_str() {
            "forecast" => {
                admit_clock(values, epoch, &text(capture, "runInitializedAt")?)?;
            }
            "station" => {
                admit_clock(values, epoch, &text(capture, "windowStart")?)?;
                admit_clock(values, epoch, &text(capture, "windowEndExclusive")?)?;
            }
            _ => return Err(AdjustmentMaintenanceBodyError),
        }
    }

    admit_rain_run(
        values,
        epoch,
        field(causal_inputs, "currentRun").ok_or(AdjustmentMaintenanceBodyError)?,
    )?;
    let prior_runs = array_field(causal_inputs, "priorRuns")?;
    // retain the current-run-first ordering before every prior model run
    for run in prior_runs {
        admit_rain_run(values, epoch, run)?;
    }
    let station_hours = array_field(causal_inputs, "stationHours")?;
    // retain every observation hour before its database receipt clock
    for station in station_hours {
        admit_clock(values, epoch, &text(station, "hourAt")?)?;
        admit_clock(values, epoch, &text(station, "receivedAt")?)?;
    }
    Ok(())
}

/// Admit one rain model run's initialization and completion clocks.
fn admit_rain_run(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    epoch: &CanonicalInstant,
    run: &JsonValue,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    admit_clock(values, epoch, &text(run, "runInitializedAt")?)?;
    admit_clock(values, epoch, &text(run, "completedAt")?)
}

/// Admit one optional nullable clock without replacing an honest null.
fn admit_optional_clock(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    epoch: &CanonicalInstant,
    object: &JsonValue,
    key: &str,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    match field(object, key).ok_or(AdjustmentMaintenanceBodyError)? {
        JsonValue::Null => Ok(()),
        JsonValue::String(value) => {
            let value = String::from_utf16(value).map_err(|_| AdjustmentMaintenanceBodyError)?;
            admit_clock(values, epoch, &value)
        }
        _ => Err(AdjustmentMaintenanceBodyError),
    }
}

/// Borrow one required decoded array member.
fn array_field<'a>(
    object: &'a JsonValue,
    key: &str,
) -> Result<&'a [JsonValue], AdjustmentMaintenanceBodyError> {
    let JsonValue::Array(values) = field(object, key).ok_or(AdjustmentMaintenanceBodyError)? else {
        return Err(AdjustmentMaintenanceBodyError);
    };
    Ok(values)
}

/// Read a required string without changing the private admission error.
fn text(value: &JsonValue, key: &str) -> Result<String, AdjustmentMaintenanceBodyError> {
    json_text(value, key).ok_or(AdjustmentMaintenanceBodyError)
}

/// Admit one canonical source clock at the inclusive capture boundary.
fn admit_clock(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    epoch: &CanonicalInstant,
    clock: &str,
) -> Result<(), AdjustmentMaintenanceBodyError> {
    values
        .canonical_utc_epoch_milliseconds(clock)
        .filter(|value| *value >= epoch.epoch_milliseconds)
        .map(|_| ())
        .ok_or(AdjustmentMaintenanceBodyError)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Values;

    impl ApiMaintenanceReceiptCanonicalValues for Values {
        /// Resolve only the three symbolic clocks used by these traversal fixtures.
        fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
            match value {
                "boundary" => Some(1_000),
                "window-inclusive" => Some(1_000 + RECENT_ERROR_WINDOW_BACKTRACK_MS),
                "window-one-ms-old" => Some(999 + RECENT_ERROR_WINDOW_BACKTRACK_MS),
                "pre-epoch" => Some(999),
                _ => None,
            }
        }

        /// Keep hashing outside these source-clock fixtures.
        fn sha256(&self, _bytes: &[u8]) -> Option<weather_database::Sha256Digest> {
            None
        }

        /// Keep canonical receipt hashing outside these source-clock fixtures.
        fn unsigned_receipt_sha256(
            &self,
            _fields: &[(&str, crate::MaintenanceReceiptScalar<'_>)],
        ) -> Option<weather_database::Sha256Digest> {
            None
        }
    }

    /// Admit the oldest rolling-window instant exactly at the epoch and reject one millisecond less.
    #[test]
    fn checks_complete_temperature_window_at_inclusive_epoch() {
        let epoch = epoch();
        assert!(
            require_shadow_source_epoch(&Values, &epoch, &temperature_source("window-inclusive"))
                .is_ok()
        );
        assert!(
            require_shadow_source_epoch(&Values, &epoch, &temperature_source("window-one-ms-old"))
                .is_err()
        );
    }

    /// Traverse forecast and station captures before finding one stale station receipt.
    #[test]
    fn rejects_one_pre_epoch_clock_across_rain_causal_inputs() {
        let epoch = epoch();
        assert!(require_shadow_source_epoch(&Values, &epoch, &rain_source("boundary")).is_ok());
        assert!(require_shadow_source_epoch(&Values, &epoch, &rain_source("pre-epoch")).is_err());
    }

    /// Build the fixed inclusive epoch used by the symbolic clock owner.
    fn epoch() -> CanonicalInstant {
        CanonicalInstant {
            epoch_milliseconds: 1_000,
            utc: "boundary".to_owned(),
        }
    }

    /// Build one temperature source with honest null maximum-history clocks.
    fn temperature_source(window_end: &str) -> JsonValue {
        object(vec![
            ("family", string("temperature")),
            (
                "rows",
                JsonValue::Array(vec![object(vec![
                    ("referenceAt", string("boundary")),
                    ("receivedAt", string("boundary")),
                    ("bestMatchProductRunAt", string("boundary")),
                ])]),
            ),
            (
                "recentErrorState",
                object(vec![
                    ("targetRunInitializedAt", string("boundary")),
                    ("windowEndValidAt", string(window_end)),
                    ("maximumSourceRunInitializedAt", JsonValue::Null),
                    ("maximumSourceValidAt", JsonValue::Null),
                ]),
            ),
        ])
    }

    /// Build one rain source covering both capture variants and every causal clock collection.
    fn rain_source(station_received_at: &str) -> JsonValue {
        object(vec![
            ("family", string("rain")),
            (
                "rows",
                JsonValue::Array(vec![object(vec![
                    ("referenceAt", string("boundary")),
                    ("receivedAt", string("boundary")),
                ])]),
            ),
            (
                "causalInputs",
                object(vec![
                    (
                        "captureSet",
                        JsonValue::Array(vec![
                            object(vec![
                                ("kind", string("forecast")),
                                ("completedAt", string("boundary")),
                                ("runInitializedAt", string("boundary")),
                            ]),
                            object(vec![
                                ("kind", string("station")),
                                ("completedAt", string("boundary")),
                                ("windowStart", string("boundary")),
                                ("windowEndExclusive", string("boundary")),
                            ]),
                        ]),
                    ),
                    ("currentRun", rain_run()),
                    ("priorRuns", JsonValue::Array(vec![rain_run()])),
                    (
                        "stationHours",
                        JsonValue::Array(vec![object(vec![
                            ("hourAt", string("boundary")),
                            ("receivedAt", string(station_received_at)),
                        ])]),
                    ),
                ]),
            ),
        ])
    }

    /// Build one admitted rain run clock pair.
    fn rain_run() -> JsonValue {
        object(vec![
            ("runInitializedAt", string("boundary")),
            ("completedAt", string("boundary")),
        ])
    }

    /// Build one decoded JSON object with exact UTF-16 member names.
    fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
        JsonValue::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.encode_utf16().collect(), value))
                .collect(),
        )
    }

    /// Build one decoded UTF-16 JSON string.
    fn string(value: &str) -> JsonValue {
        JsonValue::String(value.encode_utf16().collect())
    }
}
