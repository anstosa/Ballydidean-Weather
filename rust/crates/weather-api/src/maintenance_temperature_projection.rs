//! Project the complete temperature maintenance source without encoding it.

use std::collections::BTreeMap;

use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
    EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
    EcmwfTemperatureCanaryRunUpstreamModelLiteral, EcmwfTemperatureCanarySidecar, JsDateOrString,
    Nullable, WeatherRecordRow,
};

use crate::maintenance_source_projection::{
    MaintenanceSourceField, MaintenanceSourceProjectionError, MaintenanceSourceValue,
    admit_source_clock, admit_source_geometry, admit_source_wire_clock, bounded_source_text,
    forecast_dataset, is_source_digest, source_binary64,
};
use crate::{CanonicalInstant, CurrentTimeResolver};

const TEMPERATURE_ROW_COUNT: usize = 12;

/// Project the exact ordered twelve-hour temperature source rows.
pub fn project_temperature_maintenance_source_rows(
    best_match: &[WeatherRecordRow],
    sidecar: &EcmwfTemperatureCanarySidecar,
    issued: &CanonicalInstant,
    source_sha256: &str,
    time: &dyn CurrentTimeResolver,
) -> Result<Vec<[MaintenanceSourceField; 20]>, MaintenanceSourceProjectionError> {
    // require the complete source and comparator halos before admitting any row
    if sidecar.hours.len() != TEMPERATURE_ROW_COUNT || best_match.len() != TEMPERATURE_ROW_COUNT {
        return Err(MaintenanceSourceProjectionError::Incomplete);
    }
    // bind every row to one canonical lowercase source identity
    if !is_source_digest(source_sha256) {
        return Err(MaintenanceSourceProjectionError::Identity);
    }

    let run = &sidecar.run;
    let reference = admit_source_clock(
        time,
        &JsDateOrString::String(run.runInitializedAt.encode_utf16().collect()),
    )?;
    let received = admit_source_clock(
        time,
        &JsDateOrString::String(run.firstReceivedAt.encode_utf16().collect()),
    )?;
    validate_recent_error_state(&run.recentErrorState, issued, time)?;
    // reject unbounded lineage even though generated literal types close known values
    let adapter_version = EcmwfTemperatureCanaryRunAdapterVersionLiteral::WIRE_VALUE;
    let model_cycle = model_cycle_wire_value(run.modelCycle);
    let upstream_model = EcmwfTemperatureCanaryRunUpstreamModelLiteral::WIRE_VALUE;
    for lineage in [
        adapter_version,
        "single_run",
        "open-meteo",
        model_cycle,
        upstream_model,
    ] {
        // retain the same bounded raw lineage rule as the canonical source validator
        if !bounded_source_text(lineage) {
            return Err(MaintenanceSourceProjectionError::Lineage);
        }
    }
    // preserve only canonical content addresses for the run response
    if !is_source_digest(&run.providerResponseSha256) {
        return Err(MaintenanceSourceProjectionError::Identity);
    }

    let comparator_by_valid = index_comparators(best_match, time)?;
    let mut rows = Vec::with_capacity(TEMPERATURE_ROW_COUNT);
    let mut previous_valid_ms = None;
    // preserve sidecar order while proving its consecutive one-hour geometry
    for (index, hour) in sidecar.hours.iter().enumerate() {
        let valid = admit_source_clock(
            time,
            &JsDateOrString::String(hour.validAt.encode_utf16().collect()),
        )?;
        let model_lead_hours = admit_source_geometry(
            issued,
            &valid,
            &reference,
            &received,
            previous_valid_ms,
            Some(hour.modelLeadHours),
        )?;
        let comparator = comparator_by_valid
            .get(&valid.epoch_milliseconds)
            .copied()
            .ok_or(MaintenanceSourceProjectionError::Geometry)?;
        let comparator_product_run = admit_comparator(comparator, &hour.contentHash, issued, time)?;

        let raw_temperature =
            source_binary64(&Nullable::Value(hour.rawTemperatureC), -100.0, 70.0)?;
        let raw_humidity = source_binary64(&hour.rawRelativeHumidityPercent, 0.0, 100.0)?;
        let raw_wind = source_binary64(&hour.rawWindSpeedMps, 0.0, 150.0)?;
        let comparator_temperature = source_binary64(&comparator.temperatureC, -100.0, 70.0)?;
        rows.push([
            ("validAt", MaintenanceSourceValue::Text(valid.utc.clone())),
            (
                "leadHours",
                MaintenanceSourceValue::Number((index + 1) as u64),
            ),
            (
                "modelLeadHours",
                MaintenanceSourceValue::Number(model_lead_hours),
            ),
            (
                "referenceAt",
                MaintenanceSourceValue::Text(reference.utc.clone()),
            ),
            (
                "receivedAt",
                MaintenanceSourceValue::Text(received.utc.clone()),
            ),
            (
                "sourceSha256",
                MaintenanceSourceValue::Text(source_sha256.to_owned()),
            ),
            (
                "contentSha256",
                MaintenanceSourceValue::Text(hour.contentHash.clone()),
            ),
            (
                "adapterVersion",
                MaintenanceSourceValue::Text(adapter_version.to_owned()),
            ),
            (
                "dataset",
                MaintenanceSourceValue::Text("single_run".to_owned()),
            ),
            (
                "providerKey",
                MaintenanceSourceValue::Text("open-meteo".to_owned()),
            ),
            (
                "providerResponseSha256",
                MaintenanceSourceValue::Text(run.providerResponseSha256.clone()),
            ),
            (
                "modelCycle",
                MaintenanceSourceValue::Text(model_cycle.to_owned()),
            ),
            (
                "upstreamModel",
                MaintenanceSourceValue::Text(upstream_model.to_owned()),
            ),
            ("rawTemperatureC64", raw_temperature),
            ("rawRelativeHumidityPercent64", raw_humidity),
            ("rawWindSpeedMps64", raw_wind),
            (
                "bestMatchContentSha256",
                MaintenanceSourceValue::Text(comparator.contentHash.clone()),
            ),
            (
                "bestMatchProductRunAt",
                MaintenanceSourceValue::Text(comparator_product_run.utc),
            ),
            (
                "bestMatchSourceId",
                MaintenanceSourceValue::Text(comparator.sourceId.clone()),
            ),
            ("bestMatchTemperatureC64", comparator_temperature),
        ]);
        previous_valid_ms = Some(valid.epoch_milliseconds);
    }
    Ok(rows)
}

/// Index every comparator by its canonical valid time with JavaScript Map semantics.
fn index_comparators<'a>(
    rows: &'a [WeatherRecordRow],
    time: &dyn CurrentTimeResolver,
) -> Result<BTreeMap<i64, &'a WeatherRecordRow>, MaintenanceSourceProjectionError> {
    let mut by_valid = BTreeMap::new();
    // later duplicate keys replace earlier rows exactly like the source Map constructor
    for row in rows {
        let valid = admit_source_clock(time, &row.validAt)?;
        by_valid.insert(valid.epoch_milliseconds, row);
    }
    Ok(by_valid)
}

/// Admit one Best Match comparator and return its canonical product-run clock.
fn admit_comparator(
    comparator: &WeatherRecordRow,
    hour_content_sha256: &str,
    issued: &CanonicalInstant,
    time: &dyn CurrentTimeResolver,
) -> Result<CanonicalInstant, MaintenanceSourceProjectionError> {
    let Nullable::Value(product_run_value) = &comparator.productRunAt else {
        return Err(MaintenanceSourceProjectionError::Identity);
    };
    let Some(dataset) = forecast_dataset(comparator) else {
        return Err(MaintenanceSourceProjectionError::Lineage);
    };
    // accept only the exact UTF-16 Best Match dataset identity
    if !dataset.iter().copied().eq("best_match".encode_utf16()) {
        return Err(MaintenanceSourceProjectionError::Lineage);
    }
    // every retained hour and comparator must carry its original content address
    if !is_source_digest(hour_content_sha256) || !is_source_digest(&comparator.contentHash) {
        return Err(MaintenanceSourceProjectionError::Identity);
    }
    // retain the predecessor's positive one-to-twenty-digit source identity
    if !valid_best_match_source_id(&comparator.sourceId) {
        return Err(MaintenanceSourceProjectionError::Identity);
    }
    let product_run = admit_source_clock(time, product_run_value)?;
    // refuse prospective comparator runs at the capture instant
    if product_run.epoch_milliseconds > issued.epoch_milliseconds {
        return Err(MaintenanceSourceProjectionError::Clock);
    }
    Ok(product_run)
}

/// Validate the complete causal rolling-error state retained beside the rows.
fn validate_recent_error_state(
    state: &EcmwfTemperatureCanaryRecentErrorState,
    issued: &CanonicalInstant,
    time: &dyn CurrentTimeResolver,
) -> Result<(), MaintenanceSourceProjectionError> {
    admit_source_wire_clock(time, &state.targetRunInitializedAt)
        .map_err(|_| MaintenanceSourceProjectionError::CausalState)?;
    admit_source_wire_clock(time, &state.windowEndValidAt)
        .map_err(|_| MaintenanceSourceProjectionError::CausalState)?;
    // keep both optional maxima canonical and no later than capture
    for value in [
        &state.maximumSourceRunInitializedAt,
        &state.maximumSourceValidAt,
    ] {
        // validate only present causal maxima
        if let Nullable::Value(value) = value {
            let instant = admit_source_wire_clock(time, value)
                .map_err(|_| MaintenanceSourceProjectionError::CausalState)?;
            // future maxima cannot become causal evidence
            if instant.epoch_milliseconds > issued.epoch_milliseconds {
                return Err(MaintenanceSourceProjectionError::CausalState);
            }
        }
    }
    // retain the exact fitted cohort and bounded aggregate cardinalities
    if EcmwfTemperatureCanaryRecentErrorStateCohortLiteral::WIRE_VALUE
        != "ecmwf_single_run_hindcast"
        || state.localDates > 366
        || state.n24 > 10_000
        || state.n72 > 10_000
        || state.sourceKeys.len() > 10_000
    {
        return Err(MaintenanceSourceProjectionError::CausalState);
    }
    // preserve the exact bounded source keys instead of an opaque state claim
    for source_key in &state.sourceKeys {
        if !bounded_source_text(source_key) {
            return Err(MaintenanceSourceProjectionError::CausalState);
        }
    }
    // reject nonfinite learned statistics while retaining their unbounded finite values
    for statistic in [&state.b24C, &state.b72C, &state.mad72C] {
        if let Nullable::Value(value) = statistic
            && !value.is_finite()
        {
            return Err(MaintenanceSourceProjectionError::CausalState);
        }
    }
    Ok(())
}

/// Return the generated model-cycle wire label without accepting an alias.
const fn model_cycle_wire_value(cycle: EcmwfTemperatureCanaryRunModelCycleEnum) -> &'static str {
    // keep exhaustive generated variants tied to their source wire labels
    match cycle {
        EcmwfTemperatureCanaryRunModelCycleEnum::Value49r1 => "49r1",
        EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1 => "50r1",
    }
}

/// Accept the original positive ASCII decimal source identity grammar.
fn valid_best_match_source_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    // require one to twenty digits with a nonzero leading digit
    if bytes.is_empty() || bytes.len() > 20 || !(b'1'..=b'9').contains(&bytes[0]) {
        return false;
    }
    bytes[1..].iter().all(u8::is_ascii_digit)
}
