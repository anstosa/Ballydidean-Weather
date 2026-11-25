//! Project the complete Best Match wind halo into the closed maintenance source row schema.

use weather_contracts::{Nullable, WeatherRecordRow};

use crate::maintenance_source_projection::{
    MaintenanceSourceField, MaintenanceSourceProjectionError, MaintenanceSourceValue,
    admit_source_clock, admit_source_geometry, bounded_source_text, bounded_source_utf16,
    forecast_dataset, is_source_digest, source_binary64,
};
use crate::{CanonicalInstant, CurrentTimeResolver};

const WIND_SOURCE_ROWS: usize = 168;
const MAX_JS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Project one complete coherent wind vintage in exact source-row key order.
pub fn project_wind_maintenance_source_rows(
    rows: &[WeatherRecordRow],
    issued: &CanonicalInstant,
    source_sha256: &str,
    time: &dyn CurrentTimeResolver,
) -> Result<Vec<[MaintenanceSourceField; 17]>, MaintenanceSourceProjectionError> {
    // reject a shortened or extended causal halo before inspecting a partial vintage
    if rows.len() != WIND_SOURCE_ROWS {
        return Err(MaintenanceSourceProjectionError::Incomplete);
    }
    // bind every emitted row to one admitted lowercase content address
    if !is_source_digest(source_sha256) {
        return Err(MaintenanceSourceProjectionError::Identity);
    }

    let first = &rows[0];
    let first_reference = reference_clock(first, time)?;
    let first_dataset =
        forecast_dataset(first).ok_or(MaintenanceSourceProjectionError::Incomplete)?;
    let first_adapter = required_text(&first.adapterVersion)?;
    let first_contract_epoch = required_text(&first.contractEpoch)?;
    let first_fingerprint = required_text(&first.sourceConfigFingerprint)?;
    let first_upstream_model = required_text(&first.upstreamModel)?;
    validate_lineage(
        first_adapter,
        first_contract_epoch,
        first_dataset,
        &first.providerKey,
        &first.sourceKey,
        first_fingerprint,
        first_upstream_model,
    )?;

    let mut previous_valid_ms = None;
    let mut projected = Vec::with_capacity(WIND_SOURCE_ROWS);
    // preserve database order while enforcing one complete coherent hourly vintage
    for (index, row) in rows.iter().enumerate() {
        // reuse the common reference when the database representation is also identical
        let alternate_reference = if row.productRunAt == first.productRunAt {
            None
        } else {
            Some(reference_clock(row, time)?)
        };
        let reference = alternate_reference.as_ref().unwrap_or(&first_reference);
        let dataset = forecast_dataset(row).ok_or(MaintenanceSourceProjectionError::Incomplete)?;

        // freeze every nonpublic provenance discriminator across the full halo
        if reference != &first_reference
            || row.providerKey != first.providerKey
            || row.sourceKey != first.sourceKey
            || row.sourceId != first.sourceId
            || row.siteSlug != first.siteSlug
            || row.stationSlug != first.stationSlug
            || row.adapterVersion != first.adapterVersion
            || row.contractEpoch != first.contractEpoch
            || row.sourceConfigFingerprint != first.sourceConfigFingerprint
            || row.upstreamModel != first.upstreamModel
            || dataset != first_dataset
        {
            return Err(MaintenanceSourceProjectionError::Lineage);
        }

        // retain only exact storage content identities in the projection
        if !is_source_digest(&row.contentHash) {
            return Err(MaintenanceSourceProjectionError::Identity);
        }
        // the original numeric revision field must round-trip as a JavaScript safe integer
        if row.revisionCount > MAX_JS_SAFE_INTEGER {
            return Err(MaintenanceSourceProjectionError::Lineage);
        }

        let valid = admit_source_clock(time, &row.validAt)?;
        let received = admit_source_clock(time, &row.firstReceivedAt)?;
        let model_lead_hours = admit_source_geometry(
            issued,
            &valid,
            reference,
            &received,
            previous_valid_ms,
            None,
        )?;
        let lead_hours =
            u64::try_from(index + 1).map_err(|_| MaintenanceSourceProjectionError::Canonical)?;
        let wind_speed = source_binary64(&row.windSpeedMps, 0.0, 150.0)?;
        let wind_gust = source_binary64(&row.windGustMps, 0.0, 150.0)?;

        projected.push([
            ("validAt", MaintenanceSourceValue::Text(valid.utc)),
            ("leadHours", MaintenanceSourceValue::Number(lead_hours)),
            (
                "modelLeadHours",
                MaintenanceSourceValue::Number(model_lead_hours),
            ),
            (
                "referenceAt",
                MaintenanceSourceValue::Text(reference.utc.clone()),
            ),
            ("receivedAt", MaintenanceSourceValue::Text(received.utc)),
            (
                "sourceSha256",
                MaintenanceSourceValue::Text(source_sha256.to_owned()),
            ),
            (
                "contentSha256",
                MaintenanceSourceValue::Text(row.contentHash.clone()),
            ),
            (
                "adapterVersion",
                MaintenanceSourceValue::Text(first_adapter.to_owned()),
            ),
            (
                "contractEpoch",
                MaintenanceSourceValue::Text(first_contract_epoch.to_owned()),
            ),
            ("dataset", MaintenanceSourceValue::Utf16(dataset.to_vec())),
            (
                "providerKey",
                MaintenanceSourceValue::Text(row.providerKey.clone()),
            ),
            (
                "sourceKey",
                MaintenanceSourceValue::Text(row.sourceKey.clone()),
            ),
            (
                "sourceConfigFingerprint",
                MaintenanceSourceValue::Text(first_fingerprint.to_owned()),
            ),
            (
                "upstreamModel",
                MaintenanceSourceValue::Text(first_upstream_model.to_owned()),
            ),
            (
                "revisionCount",
                MaintenanceSourceValue::Number(row.revisionCount),
            ),
            ("windSpeedMps64", wind_speed),
            ("windGustMps64", wind_gust),
        ]);
        previous_valid_ms = Some(valid.epoch_milliseconds);
    }
    Ok(projected)
}

/// Resolve the required product-run clock through the shared canonical adapter.
fn reference_clock(
    row: &WeatherRecordRow,
    time: &dyn CurrentTimeResolver,
) -> Result<CanonicalInstant, MaintenanceSourceProjectionError> {
    match &row.productRunAt {
        Nullable::Value(value) => admit_source_clock(time, value),
        Nullable::Null => Err(MaintenanceSourceProjectionError::Incomplete),
    }
}

/// Borrow one required generated nullable lineage string.
fn required_text(value: &Nullable<String>) -> Result<&str, MaintenanceSourceProjectionError> {
    match value {
        Nullable::Value(value) => Ok(value),
        Nullable::Null => Err(MaintenanceSourceProjectionError::Incomplete),
    }
}

/// Validate every emitted raw lineage value without normalizing its source spelling.
fn validate_lineage(
    adapter: &str,
    contract_epoch: &str,
    dataset: &[u16],
    provider_key: &str,
    source_key: &str,
    fingerprint: &str,
    upstream_model: &str,
) -> Result<(), MaintenanceSourceProjectionError> {
    // apply the exact bounded string contract to all seven wind lineage fields
    if !bounded_source_text(adapter)
        || !bounded_source_text(contract_epoch)
        || !bounded_source_utf16(dataset)
        || !bounded_source_text(provider_key)
        || !bounded_source_text(source_key)
        || !bounded_source_text(fingerprint)
        || !bounded_source_text(upstream_model)
    {
        return Err(MaintenanceSourceProjectionError::Lineage);
    }
    Ok(())
}
