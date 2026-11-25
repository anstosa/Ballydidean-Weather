//! Select and construct grouped weather revisions for the worker's private archive client.

use weather_contracts::{
    AdjustmentRevisionBatchProjection, AdjustmentRevisionBatchProjectionContractVersionLiteral,
    AdjustmentRevisionBatchProjectionFamilyEnum, AdjustmentRevisionBatchProjectionLogicalKeyValue,
    AdjustmentRevisionBatchProjectionProjectionKindEnum,
    AdjustmentRevisionBatchProjectionRowsValue, JsonValue, Nullable, SourceKind,
};
use weather_database::{PersistedWeatherAdjustmentRevision, Sha256Digest};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const BEST_MATCH_HOURS: usize = 168;
const MAXIMUM_PHYSICAL_COHORT_ROWS: usize = 168;

// exact source and fingerprint identities from the original TS forecast-observation catalog
const RETAINED_SOURCE_LINEAGES: &[(&str, &str)] = &[
    (
        "ambient-maxweather-observations-v1",
        "7a7528a6278924ca5280a1a6045b6647b7e660b112d7fa3008c542a17ff99df4",
    ),
    (
        "ambient-merlin-observations-v1",
        "c3829701bfc25a050022dc3965569d3a87376e8a43b5fdcb7621533f1ae3c65d",
    ),
    (
        "ecowitt-88f15505d89f-local-live-v1",
        "0a44488714d0fa807b924f8aea14965b437722e8cf9f8eae4bc8c81da8a0149d",
    ),
    (
        "netatmo-nearby-observations-v1",
        "5495917dd2465a32d9878e73c68781a229b432901cdc4867875726351efbdbbc",
    ),
    (
        "tempest-126537-observations-v2",
        "34dafbd6584c93d55ed4d3d43dc7e74a0876165d4ddfc921413f7b826dff7ab7",
    ),
    (
        "tempest-168853-observations-v2",
        "1c7a402337a44a5441775246cbc02da7994599dad6ab83dc04b248303facfea5",
    ),
    (
        "tempest-201058-observations-v2",
        "a61cce798cddf682da9608dc245659fc7734a6d5304068939d3115ae7d81a50e",
    ),
    (
        "tempest-203055-observations-v2",
        "9ead4c5359a6a9640f334be91397180aa62b90b0f0ce813b9ff26fe84537acc4",
    ),
    (
        "tempest-225947-observations-v2",
        "b4dd6105d9a56a7c5d0dc4063f830e1cf28d693222a8de15536dd83d3a6178c4",
    ),
    (
        "tempest-38270-observations-v2",
        "ce162067aced4ab3522fb83145a21e608ff24dec189097726188e96fd6cca52f",
    ),
    (
        "tempest-64255-observations-v2",
        "8eb488a358375fc3526347d9ef6c9f23080095a22ea874a42ec400b0317d868a",
    ),
    (
        "wunderground-maxweather-history-v1",
        "52dda6c5444d0a234fbe23d6218027d417ac966ecf291a7d5dfff42fd0dc207c",
    ),
];

/// Existing time, revision codec, and SHA owners used by the worker process.
pub trait WorkerWeatherRevisionMaterialValues {
    /// Parse one canonical UTC instant without implementing another calendar.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Option<i64>;

    /// Encode the generated projection through the original named-order codec.
    fn encode_revision_batch_projection(
        &mut self,
        projection: &AdjustmentRevisionBatchProjection,
    ) -> Option<Vec<u8>>;

    /// Hash exact preimage bytes through the process SHA owner.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest>;
}

/// One source-coherent revision group selected by the original worker rules.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedWeatherRevisionGroup {
    pub capture_epoch: String,
    pub projection_kind: AdjustmentRevisionBatchProjectionProjectionKindEnum,
    pub revisions: Vec<PersistedWeatherAdjustmentRevision>,
}

/// Canonical grouped document, bytes, and shared body identity.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedWeatherRevisionMaterial {
    pub bytes: Vec<u8>,
    pub identity: Sha256Digest,
    pub projection: AdjustmentRevisionBatchProjection,
}

/// Closed selection or material construction failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerWeatherRevisionMaterialError;

/// Select one complete Best Match run or one admitted physical query cohort.
pub fn select_weather_revision_group(
    changed: &[PersistedWeatherAdjustmentRevision],
    pending_physical: &[PersistedWeatherAdjustmentRevision],
    capture_epoch: &str,
    values: &mut impl WorkerWeatherRevisionMaterialValues,
) -> Option<SelectedWeatherRevisionGroup> {
    let first = changed.first()?;
    // reject mixed source batches before considering either projection family
    if changed.iter().any(|revision| {
        revision.record.sourceId != first.record.sourceId
            || revision.record.sourceKind != first.record.sourceKind
    }) {
        return None;
    }
    if let Some(group) = select_complete_best_match_run(changed, first, capture_epoch, values) {
        return Some(group);
    }
    select_physical_cohort(first, pending_physical, capture_epoch, values)
}

/// Return the one source whose admitted trigger requires the pending physical cohort read.
pub fn pending_physical_weather_revision_source(
    changed: &[PersistedWeatherAdjustmentRevision],
) -> Option<&str> {
    let first = changed.first()?;
    // never issue the physical backlog query for forecast, mixed, or unregistered triggers
    if first.record.sourceKind != SourceKind::PhysicalSensor
        || first.record.productRunAt != Nullable::Null
        || !retained_source_lineage(&first.source_key, &first.source_config_fingerprint)
        || changed.iter().any(|revision| {
            revision.record.sourceId != first.record.sourceId
                || revision.record.sourceKind != first.record.sourceKind
        })
    {
        return None;
    }
    Some(first.record.sourceId.as_str())
}

/// Construct the original generated DTO, canonical bytes, and body hash.
pub fn prepare_weather_revision_material(
    selected: &SelectedWeatherRevisionGroup,
    values: &mut impl WorkerWeatherRevisionMaterialValues,
) -> Result<PreparedWeatherRevisionMaterial, WorkerWeatherRevisionMaterialError> {
    let first = selected
        .revisions
        .first()
        .ok_or(WorkerWeatherRevisionMaterialError)?;
    validate_selected_group(selected, values)?;
    let actual = selected.projection_kind
        == AdjustmentRevisionBatchProjectionProjectionKindEnum::ActualBestMatch;
    admit_selected_capture_epoch(selected, first, actual, values)?;
    let source_kind = source_kind(first.record.sourceKind)?;
    let dataset = if actual {
        provider_string(first, "dataset").ok_or(WorkerWeatherRevisionMaterialError)?
    } else {
        first.source_key.clone()
    };
    let upstream_model = if actual {
        first.record.metadata.model.clone()
    } else {
        Nullable::Null
    };
    let contract_epoch = contract_epoch(first, values)?;
    let rows = selected
        .revisions
        .iter()
        .map(revision_row)
        .collect::<Result<Vec<_>, _>>()?;
    let projection = AdjustmentRevisionBatchProjection {
        contractVersion: AdjustmentRevisionBatchProjectionContractVersionLiteral,
        family: if actual {
            AdjustmentRevisionBatchProjectionFamilyEnum::Wind
        } else {
            AdjustmentRevisionBatchProjectionFamilyEnum::Shared
        },
        logicalKey: vec![
            (
                utf16("productRunAt"),
                logical_nullable(&first.record.productRunAt),
            ),
            (utf16("sourceId"), logical_string(&first.record.sourceId)),
            (utf16("sourceKind"), logical_string(source_kind)),
            (utf16("validAt"), logical_string(&first.record.validAt)),
        ],
        logicalReceivedAt: first.logical_received_at.as_str().to_owned(),
        projectionKind: selected.projection_kind,
        rows,
        source: vec![
            (
                utf16("adapterVersion"),
                nullable_text(&first.adapter_version),
            ),
            (utf16("contractEpoch"), nullable_text(&contract_epoch)),
            (utf16("dataset"), nullable_text(&dataset)),
            (utf16("providerKey"), nullable_text(&first.provider_key)),
            (
                utf16("sourceConfigFingerprint"),
                nullable_text(&first.source_config_fingerprint),
            ),
            (utf16("sourceId"), nullable_text(&first.record.sourceId)),
            (utf16("sourceKey"), nullable_text(&first.source_key)),
            (utf16("sourceKind"), nullable_text(source_kind)),
            (utf16("upstreamModel"), nullable_utf16(&upstream_model)),
        ],
        storedContentSha256: first.content_hash.as_str().to_owned(),
    };
    let bytes = values
        .encode_revision_batch_projection(&projection)
        .ok_or(WorkerWeatherRevisionMaterialError)?;
    let identity = values
        .sha256(&bytes)
        .filter(|digest| valid_hash(digest.as_str()))
        .ok_or(WorkerWeatherRevisionMaterialError)?;
    Ok(PreparedWeatherRevisionMaterial {
        bytes,
        identity,
        projection,
    })
}

/// Select the exact future 168-hour body from a six-hour Best Match cycle.
fn select_complete_best_match_run(
    changed: &[PersistedWeatherAdjustmentRevision],
    first: &PersistedWeatherAdjustmentRevision,
    capture_epoch: &str,
    values: &mut impl WorkerWeatherRevisionMaterialValues,
) -> Option<SelectedWeatherRevisionGroup> {
    let Nullable::Value(product_run_at) = &first.record.productRunAt else {
        return None;
    };
    if first.record.sourceKind != SourceKind::Forecast
        || first.source_key != "open-meteo-forecast-v4"
        || provider_string(first, "dataset").as_deref() != Some("best_match")
        || nullable_string(&first.record.metadata.model).as_deref() != Some("best_match")
    {
        return None;
    }
    let run_at = values.utc_epoch_milliseconds(product_run_at)?;
    // retain only the same four daily causal cycles as the original worker
    if run_at.div_euclid(HOUR_MILLISECONDS).rem_euclid(6) != 0 {
        return None;
    }
    let maximum = run_at.checked_add(BEST_MATCH_HOURS as i64 * HOUR_MILLISECONDS)?;
    let mut revisions = changed
        .iter()
        .filter_map(|revision| {
            let valid_at = values.utc_epoch_milliseconds(&revision.record.validAt)?;
            (revision.record.productRunAt == first.record.productRunAt
                && valid_at > run_at
                && valid_at <= maximum)
                .then_some((valid_at, revision.clone()))
        })
        .collect::<Vec<_>>();
    revisions.sort_by_key(|(valid_at, _)| *valid_at);
    (revisions.len() == BEST_MATCH_HOURS).then(|| SelectedWeatherRevisionGroup {
        capture_epoch: capture_epoch.to_owned(),
        projection_kind: AdjustmentRevisionBatchProjectionProjectionKindEnum::ActualBestMatch,
        revisions: revisions
            .into_iter()
            .map(|(_, revision)| revision)
            .collect(),
    })
}

/// Admit the bounded, epoch-filtered rows returned by the retained physical query.
fn select_physical_cohort(
    first: &PersistedWeatherAdjustmentRevision,
    pending: &[PersistedWeatherAdjustmentRevision],
    capture_epoch: &str,
    values: &mut impl WorkerWeatherRevisionMaterialValues,
) -> Option<SelectedWeatherRevisionGroup> {
    if first.record.sourceKind != SourceKind::PhysicalSensor
        || first.record.productRunAt != Nullable::Null
        || !retained_source_lineage(&first.source_key, &first.source_config_fingerprint)
        || pending.is_empty()
        || pending.len() > MAXIMUM_PHYSICAL_COHORT_ROWS
    {
        return None;
    }
    let epoch = values.utc_epoch_milliseconds(capture_epoch)?;
    let pending_first = pending.first()?;
    let mut previous_valid_at = None;
    // require the exact query's source, lineage, causal clocks, and order
    for revision in pending {
        let valid_at = values.utc_epoch_milliseconds(&revision.record.validAt)?;
        let received_at = values.utc_epoch_milliseconds(revision.logical_received_at.as_str())?;
        if revision.record.sourceId != first.record.sourceId
            || revision.record.sourceKind != SourceKind::PhysicalSensor
            || revision.record.productRunAt != Nullable::Null
            || revision.source_key != pending_first.source_key
            || revision.source_config_fingerprint != pending_first.source_config_fingerprint
            || revision.adapter_version != pending_first.adapter_version
            || valid_at < epoch
            || received_at < epoch
            || previous_valid_at.is_some_and(|previous| valid_at < previous)
        {
            return None;
        }
        previous_valid_at = Some(valid_at);
    }
    Some(SelectedWeatherRevisionGroup {
        capture_epoch: capture_epoch.to_owned(),
        projection_kind: AdjustmentRevisionBatchProjectionProjectionKindEnum::TargetRevision,
        revisions: pending.to_vec(),
    })
}

/// Reject an old provider or transaction clock before encoding or staging bytes.
fn admit_selected_capture_epoch(
    selected: &SelectedWeatherRevisionGroup,
    first: &PersistedWeatherAdjustmentRevision,
    actual: bool,
    values: &mut impl WorkerWeatherRevisionMaterialValues,
) -> Result<(), WorkerWeatherRevisionMaterialError> {
    let source_clock = if actual {
        nullable_string(&first.record.productRunAt).ok_or(WorkerWeatherRevisionMaterialError)?
    } else {
        first.record.validAt.clone()
    };
    let source_epoch = values
        .utc_epoch_milliseconds(&source_clock)
        .ok_or(WorkerWeatherRevisionMaterialError)?;
    let receipt_epoch = values
        .utc_epoch_milliseconds(first.logical_received_at.as_str())
        .ok_or(WorkerWeatherRevisionMaterialError)?;
    let capture_epoch = values
        .utc_epoch_milliseconds(&selected.capture_epoch)
        .ok_or(WorkerWeatherRevisionMaterialError)?;
    if source_epoch < capture_epoch || receipt_epoch < capture_epoch {
        return Err(WorkerWeatherRevisionMaterialError);
    }
    Ok(())
}

/// Recheck caller-constructed selected values before producing canonical bytes.
fn validate_selected_group(
    selected: &SelectedWeatherRevisionGroup,
    values: &mut impl WorkerWeatherRevisionMaterialValues,
) -> Result<(), WorkerWeatherRevisionMaterialError> {
    let first = selected
        .revisions
        .first()
        .ok_or(WorkerWeatherRevisionMaterialError)?;
    let mut previous = None;
    // keep one coherent source, lineage, and increasing row order
    for revision in &selected.revisions {
        let valid_at = values
            .utc_epoch_milliseconds(&revision.record.validAt)
            .ok_or(WorkerWeatherRevisionMaterialError)?;
        if revision.record.sourceId != first.record.sourceId
            || revision.record.sourceKind != first.record.sourceKind
            || revision.record.productRunAt != first.record.productRunAt
            || revision.adapter_version != first.adapter_version
            || revision.source_config_fingerprint != first.source_config_fingerprint
            || revision.source_key != first.source_key
            || previous.is_some_and(|clock| valid_at < clock)
            || !valid_hash(revision.content_hash.as_str())
        {
            return Err(WorkerWeatherRevisionMaterialError);
        }
        previous = Some(valid_at);
    }
    Ok(())
}

/// Derive the source contract epoch with the shared SHA owner.
fn contract_epoch(
    revision: &PersistedWeatherAdjustmentRevision,
    values: &mut impl WorkerWeatherRevisionMaterialValues,
) -> Result<String, WorkerWeatherRevisionMaterialError> {
    let mut material = revision.adapter_version.as_bytes().to_vec();
    material.push(0);
    material.extend_from_slice(revision.source_config_fingerprint.as_bytes());
    let digest = values
        .sha256(&material)
        .filter(|digest| valid_hash(digest.as_str()))
        .ok_or(WorkerWeatherRevisionMaterialError)?;
    let prefix = if revision.record.sourceKind == SourceKind::Forecast {
        "legacy-v4"
    } else {
        "normalized-weather/v1"
    };
    Ok(format!("{prefix}/{}", digest.as_str()))
}

/// Construct one generated row in the original key order.
fn revision_row(
    revision: &PersistedWeatherAdjustmentRevision,
) -> Result<
    Vec<(Vec<u16>, AdjustmentRevisionBatchProjectionRowsValue)>,
    WorkerWeatherRevisionMaterialError,
> {
    let metrics = &revision.record.metrics;
    Ok(vec![
        (
            utf16("apparentTemperatureC64"),
            row_nullable(&metrics.apparentTemperatureC)?,
        ),
        (
            utf16("blackGlobeTemperatureC64"),
            row_nullable(&metrics.blackGlobeTemperatureC)?,
        ),
        (
            utf16("cloudCoverPercent64"),
            row_nullable(&metrics.cloudCoverPercent)?,
        ),
        (
            utf16("contentSha256"),
            row_string(revision.content_hash.as_str()),
        ),
        (
            utf16("pm25MicrogramsPerCubicMeter64"),
            row_nullable(&metrics.pm25MicrogramsPerCubicMeter)?,
        ),
        (
            utf16("precipitationMm64"),
            row_nullable(&metrics.precipitationMm)?,
        ),
        (
            utf16("precipitationRateMmPerHour64"),
            row_nullable(&metrics.precipitationRateMmPerHour)?,
        ),
        (utf16("pressureHpa64"), row_nullable(&metrics.pressureHpa)?),
        (
            utf16("relativeHumidityPercent64"),
            row_nullable(&metrics.relativeHumidityPercent)?,
        ),
        (
            utf16("soilElectricalConductivityMicrosiemensPerCm64"),
            row_nullable(&metrics.soilElectricalConductivityMicrosiemensPerCm)?,
        ),
        (
            utf16("soilMoisturePercent64"),
            row_nullable(&metrics.soilMoisturePercent)?,
        ),
        (
            utf16("solarRadiationWm264"),
            row_nullable(&metrics.solarRadiationWm2)?,
        ),
        (
            utf16("temperatureC64"),
            row_nullable(&metrics.temperatureC)?,
        ),
        (utf16("uvIndex64"), row_nullable(&metrics.uvIndex)?),
        (utf16("validAt"), row_string(&revision.record.validAt)),
        (utf16("waterLevelM64"), row_nullable(&metrics.waterLevelM)?),
        (
            utf16("wetBulbGlobeTemperatureC64"),
            row_nullable(&metrics.wetBulbGlobeTemperatureC)?,
        ),
        (
            utf16("windDirectionDegrees64"),
            row_nullable(&metrics.windDirectionDegrees)?,
        ),
        (utf16("windGustMps64"), row_nullable(&metrics.windGustMps)?),
        (
            utf16("windSpeedMps64"),
            row_nullable(&metrics.windSpeedMps)?,
        ),
    ])
}

fn provider_string(revision: &PersistedWeatherAdjustmentRevision, name: &str) -> Option<String> {
    let Nullable::Value(fields) = &revision.record.metadata.provider else {
        return None;
    };
    let JsonValue::String(value) = fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))?
    else {
        return None;
    };
    String::from_utf16(value).ok()
}

fn retained_source_lineage(source_key: &str, fingerprint: &str) -> bool {
    RETAINED_SOURCE_LINEAGES.contains(&(source_key, fingerprint))
}

fn source_kind(kind: SourceKind) -> Result<&'static str, WorkerWeatherRevisionMaterialError> {
    match kind {
        SourceKind::Forecast => Ok("forecast"),
        SourceKind::PhysicalSensor => Ok("physical_sensor"),
        _ => Err(WorkerWeatherRevisionMaterialError),
    }
}

fn row_nullable(
    value: &Nullable<f64>,
) -> Result<AdjustmentRevisionBatchProjectionRowsValue, WorkerWeatherRevisionMaterialError> {
    match value {
        Nullable::Null => Ok(AdjustmentRevisionBatchProjectionRowsValue::Null),
        Nullable::Value(value) if value.is_finite() => {
            Ok(row_string(&format!("{:016x}", value.to_bits())))
        }
        Nullable::Value(_) => Err(WorkerWeatherRevisionMaterialError),
    }
}

fn row_string(value: &str) -> AdjustmentRevisionBatchProjectionRowsValue {
    AdjustmentRevisionBatchProjectionRowsValue::String(utf16(value))
}

fn logical_nullable(value: &Nullable<String>) -> AdjustmentRevisionBatchProjectionLogicalKeyValue {
    match value {
        Nullable::Null => AdjustmentRevisionBatchProjectionLogicalKeyValue::Null,
        Nullable::Value(value) => logical_string(value),
    }
}

fn logical_string(value: &str) -> AdjustmentRevisionBatchProjectionLogicalKeyValue {
    AdjustmentRevisionBatchProjectionLogicalKeyValue::String(utf16(value))
}

fn nullable_text(value: &str) -> Nullable<Vec<u16>> {
    Nullable::Value(utf16(value))
}

fn nullable_utf16(value: &Nullable<String>) -> Nullable<Vec<u16>> {
    match value {
        Nullable::Null => Nullable::Null,
        Nullable::Value(value) => nullable_text(value),
    }
}

fn nullable_string(value: &Nullable<String>) -> Option<String> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value.clone()),
    }
}

fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "worker_weather_revision_material_test.rs"]
mod tests;
