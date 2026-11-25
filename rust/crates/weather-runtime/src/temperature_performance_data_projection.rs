//! Project verified temperature package material into the legacy scoring data document.

use std::collections::HashMap;
use std::fmt::{self, Display};

use weather_contracts::{
    ForecastAdjustmentPerformancePackageInputs, JsonValue, Nullable, TemperaturePerformanceDataRow,
    TemperaturePerformanceDataRowCohortLiteral, TemperaturePerformanceDataRowRecordedRuntimeResult,
    TemperaturePerformanceDataWire, TemperaturePerformanceScoredPairMetadata,
    TemperaturePerformanceScoredPairMetadataEvidenceClassEnum,
};
use weather_worker::RainPerformanceReportPackageInputs;

use crate::{
    JoinedRainPerformanceEdgeRow, TemperaturePerformanceTargetError,
    TemperaturePerformanceTargetValues, derive_temperature_performance_target,
    maintenance_canonical_json::field,
};

/// Approved JavaScript coercion, target, canonical JSON, and digest owners.
pub trait TemperaturePerformanceDataProjectionValues: TemperaturePerformanceTargetValues {
    /// Apply the original JavaScript `String(value)` conversion without normalizing UTF-16.
    fn js_string(&mut self, value: &JsonValue) -> Option<Vec<u16>>;
}

/// Closed failures from the verified temperature data projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemperaturePerformanceDataProjectionError {
    CanonicalIdentity,
    InvalidData,
    MissingImmutableRun,
    RunIdentity,
    Target(TemperaturePerformanceTargetError),
}

impl Display for TemperaturePerformanceDataProjectionError {
    /// Render stable diagnostics at the native projection boundary.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // preserve one closed diagnostic per failure class
        match self {
            Self::CanonicalIdentity => {
                formatter.write_str("temperature performance identity serialization failed")
            }
            Self::InvalidData => formatter.write_str("temperature performance data is invalid"),
            Self::MissingImmutableRun => {
                formatter.write_str("temperature hour is missing its immutable run")
            }
            Self::RunIdentity => formatter.write_str("temperature run identity coercion failed"),
            Self::Target(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for TemperaturePerformanceDataProjectionError {}

impl From<TemperaturePerformanceTargetError> for TemperaturePerformanceDataProjectionError {
    /// Preserve target derivation failures without collapsing their diagnostics.
    fn from(error: TemperaturePerformanceTargetError) -> Self {
        Self::Target(error)
    }
}

/// Project verified temperature rows, exact captured issuance, and serving fallbacks.
pub fn project_temperature_performance_data(
    forecast_rows: &[JsonValue],
    adjustment_rows: &[JsonValue],
    edge_rows: &[JoinedRainPerformanceEdgeRow],
    inputs: &RainPerformanceReportPackageInputs,
    values: &mut impl TemperaturePerformanceDataProjectionValues,
) -> Result<JsonValue, TemperaturePerformanceDataProjectionError> {
    let mut runs = HashMap::<Vec<u16>, &JsonValue>::new();

    // index the last immutable run for each javascript key
    for row in adjustment_rows {
        // retain only temperature run records
        if string_field_equals(row, "record_kind", "temperature_run") {
            let payload = object_field(row, "payload")?;
            let identity = values
                .js_string(required_field(payload, "id")?)
                .ok_or(TemperaturePerformanceDataProjectionError::RunIdentity)?;
            runs.insert(identity, payload);
        }
    }

    let mut projected_rows = Vec::new();
    // join each immutable hour in package order
    for row in adjustment_rows {
        // skip every non-hour record
        if !string_field_equals(row, "record_kind", "temperature_hour") {
            continue;
        }

        let payload = object_field(row, "payload")?;
        let run_identity = values
            .js_string(required_field(payload, "runId")?)
            .ok_or(TemperaturePerformanceDataProjectionError::RunIdentity)?;
        let run = runs
            .get(&run_identity)
            .copied()
            .ok_or(TemperaturePerformanceDataProjectionError::MissingImmutableRun)?;
        projected_rows.push(project_temperature_hour(
            forecast_rows,
            adjustment_rows,
            edge_rows,
            row,
            payload,
            run,
            values,
        )?);
    }

    let fallbacks = project_serving_fallbacks(edge_rows, values)?;
    Ok(temperature_data_wire_json(TemperaturePerformanceDataWire {
        actualServingFallbacks: fallbacks,
        inputs: package_inputs(inputs),
        rows: projected_rows,
    }))
}

/// Project one temperature hour after its immutable run has been resolved.
#[allow(clippy::too_many_arguments)]
fn project_temperature_hour(
    forecast_rows: &[JsonValue],
    adjustment_rows: &[JsonValue],
    edge_rows: &[JoinedRainPerformanceEdgeRow],
    row: &JsonValue,
    payload: &JsonValue,
    run: &JsonValue,
    values: &mut impl TemperaturePerformanceDataProjectionValues,
) -> Result<TemperaturePerformanceDataRow, TemperaturePerformanceDataProjectionError> {
    let valid_at = required_text(payload, "validAt")?;
    let run_initialized_at = required_text(run, "runInitializedAt")?;
    let model_lead_hours = required_finite(payload, "modelLeadHours")?;
    let target =
        derive_temperature_performance_target(forecast_rows, &valid_at, adjustment_rows, values)?;
    let issuance = find_exact_issuance(
        edge_rows,
        payload,
        run,
        &valid_at,
        &run_initialized_at,
        model_lead_hours,
    )?;
    let temperature_adjustment = issuance
        .map(|edge| object_field(&edge.row, "temperatureAdjustment"))
        .transpose()?;
    let temperature_bundle = issuance
        .map(|edge| object_field(&edge.bundle_identities, "temperature"))
        .transpose()?;
    let row_identity = match issuance {
        Some(edge) => issuance_row_identity(edge, values)?,
        None => required_text(row, "record_revision_identity")?,
    };
    // distinguish captured receipts from retrospective joins
    let evidence_class = if issuance.is_some() {
        TemperaturePerformanceScoredPairMetadataEvidenceClassEnum::ProspectiveReceipt
    } else {
        TemperaturePerformanceScoredPairMetadataEvidenceClassEnum::RetrospectiveCounterfactual
    };
    let source_receipt_at = match temperature_adjustment {
        Some(adjustment) => {
            let source = object_field(adjustment, "sourceForecast")?;
            required_text(source, "firstReceivedAt")?
        }
        None => required_text(row, "first_received_at")?,
    };
    let hour_key = required_text(row, "record_revision_identity")?;
    let local_date = required_text(row, "local_date")?;
    let target_key = target
        .target_identity
        .clone()
        .unwrap_or_else(|| format!("missing:{valid_at}"));
    let provenance_complete =
        issuance.is_none_or(|edge| boolean_field_equals(&edge.row, "provenanceComplete", true));
    let first_edge_committed_at = issuance.map(|edge| edge.first_edge_committed_at.as_str());
    let scored_pair_metadata = TemperaturePerformanceScoredPairMetadata {
        evidenceClass: evidence_class,
        fallback: false,
        firstEdgeCommittedAt: nullable_string(first_edge_committed_at),
        horizonHours: model_lead_hours - 6.0,
        key: hour_key.clone(),
        localDate: local_date,
        provenanceComplete: provenance_complete,
        rowIdentity: row_identity,
        sourceReceiptAt: Nullable::Value(source_receipt_at),
        targetKey: target_key,
        validAt: valid_at.clone(),
        vintageKey: run_initialized_at.clone(),
    };
    let recorded_runtime_result = recorded_runtime_result(
        temperature_adjustment,
        temperature_bundle,
        &scored_pair_metadata,
    )?;

    Ok(TemperaturePerformanceDataRow {
        actualTemperatureC: nullable_value(target.target),
        bestMatchRawTemperatureC: match temperature_adjustment {
            Some(adjustment) => nullable_finite(adjustment, "rawBestMatchTemperatureC")?,
            None => Nullable::Null,
        },
        cohort: TemperaturePerformanceDataRowCohortLiteral,
        key: hour_key,
        modelCycle: required_text(run, "modelCycle")?,
        modelLeadHours: model_lead_hours,
        operationalHorizonHours: model_lead_hours - 6.0,
        rawRelativeHumidityPercent: nullable_finite(payload, "rawRelativeHumidityPercent")?,
        rawTemperatureC: required_finite(payload, "rawTemperatureC")?,
        rawWindSpeedMps: nullable_finite(payload, "rawWindSpeedMps")?,
        recordedRuntimeResult: recorded_runtime_result,
        recentErrorState: required_field(run, "recentErrorState")?.clone(),
        runInitializedAt: run_initialized_at,
        scoredPairMetadata: scored_pair_metadata,
        scoreEligible: (7.0..=18.0).contains(&model_lead_hours),
        targetIdentity: nullable_value(target.target_identity),
        targetMaxReceiptAt: nullable_value(target.max_receipt_at),
        validAt: valid_at,
    })
}

/// Find the first edge whose captured source is byte-semantically identical to the hour.
fn find_exact_issuance<'a>(
    edge_rows: &'a [JoinedRainPerformanceEdgeRow],
    payload: &JsonValue,
    run: &JsonValue,
    valid_at: &str,
    run_initialized_at: &str,
    model_lead_hours: f64,
) -> Result<Option<&'a JoinedRainPerformanceEdgeRow>, TemperaturePerformanceDataProjectionError> {
    let valid_at = json_string(valid_at);
    let run_initialized_at = json_string(run_initialized_at);
    let model_lead_hours = JsonValue::Number(model_lead_hours);

    // retain the predecessor's first exact issuance
    for edge in edge_rows {
        let adjustment = object_field(&edge.row, "temperatureAdjustment")?;
        let source = required_field(adjustment, "sourceForecast")?;
        // ignore the explicit no-source state
        if matches!(source, JsonValue::Null) {
            continue;
        }
        let captured = require_object(source)?;
        let matches = strict_optional_equal(
            field(captured, "runInitializedAt"),
            Some(&run_initialized_at),
        ) && strict_optional_equal(field(captured, "validAt"), Some(&valid_at))
            && strict_optional_equal(
                field(captured, "providerResponseSha256"),
                field(run, "providerResponseSha256"),
            )
            && strict_optional_equal(field(captured, "modelCycle"), field(run, "modelCycle"))
            && strict_optional_equal(field(captured, "modelLeadHours"), Some(&model_lead_hours))
            && strict_optional_equal(
                field(captured, "upstreamModel"),
                field(run, "upstreamModel"),
            )
            && object_is_optional(
                field(captured, "rawTemperatureC"),
                field(payload, "rawTemperatureC"),
            )
            && object_is_optional(
                field(captured, "rawRelativeHumidityPercent"),
                field(payload, "rawRelativeHumidityPercent"),
            )
            && object_is_optional(
                field(captured, "rawWindSpeedMps"),
                field(payload, "rawWindSpeedMps"),
            );
        // stop at the first exact captured edge
        if matches {
            return Ok(Some(edge));
        }
    }
    Ok(None)
}

/// Derive the issued row identity from its exact bundle, record revision, and settings.
fn issuance_row_identity(
    edge: &JoinedRainPerformanceEdgeRow,
    values: &mut impl TemperaturePerformanceDataProjectionValues,
) -> Result<String, TemperaturePerformanceDataProjectionError> {
    let record = object_field(&edge.row, "record")?;
    let identity = json_object(vec![
        (
            "bundle",
            required_field(&edge.bundle_identities, "temperature")?.clone(),
        ),
        ("recordId", json_string(&required_text(record, "id")?)),
        (
            "recordRevision",
            required_field(record, "revisionCount")?.clone(),
        ),
        ("settingsSha256", json_string(&edge.settings_sha256)),
    ]);
    canonical_identity(&identity, values)
}

/// Project one applied captured correction only when it names the active bundle.
fn recorded_runtime_result(
    adjustment: Option<&JsonValue>,
    bundle: Option<&JsonValue>,
    scored_pair_metadata: &TemperaturePerformanceScoredPairMetadata,
) -> Result<
    Nullable<TemperaturePerformanceDataRowRecordedRuntimeResult>,
    TemperaturePerformanceDataProjectionError,
> {
    let Some(adjustment) = adjustment else {
        return Ok(Nullable::Null);
    };
    let captured_bundle = field(adjustment, "bundleSha256");
    let active_bundle = bundle.and_then(|value| field(value, "activeBundle"));
    let captured_state = field(adjustment, "state");
    let corrected = field(adjustment, "correctedTemperatureC");
    let captured_applied =
        value_is_string(captured_state, "active") && !matches!(corrected, Some(JsonValue::Null));

    // reject every nonapplied or stale captured result
    if !captured_applied
        || !matches!(captured_bundle, Some(JsonValue::String(_)))
        || !strict_optional_equal(captured_bundle, active_bundle)
    {
        return Ok(Nullable::Null);
    }

    let serving_bundle = json_text(captured_bundle.expect("string checked"))?;
    let prediction = required_finite(adjustment, "correctedTemperatureC")?;
    let reason_code = match required_field(adjustment, "reasonCode")? {
        JsonValue::Null => Nullable::Null,
        value => Nullable::Value(nonempty_json_text(value)?),
    };
    let mut runtime_metadata = scored_pair_metadata.clone();
    runtime_metadata.evidenceClass =
        TemperaturePerformanceScoredPairMetadataEvidenceClassEnum::AsIssued;
    runtime_metadata.fallback = false;
    Ok(Nullable::Value(
        TemperaturePerformanceDataRowRecordedRuntimeResult {
            applied: true,
            predictionTemperatureC: prediction,
            reasonCode: reason_code,
            scoredPairMetadata: runtime_metadata,
            servingBundleSha256: serving_bundle,
        },
    ))
}

type OrderedFallbackRecord = Vec<(Vec<u16>, JsonValue)>;

/// coalesce nonactive serving decisions by identity without reordering replacements
fn project_serving_fallbacks(
    edge_rows: &[JoinedRainPerformanceEdgeRow],
    values: &mut impl TemperaturePerformanceDataProjectionValues,
) -> Result<Vec<OrderedFallbackRecord>, TemperaturePerformanceDataProjectionError> {
    let mut fallback_indexes = HashMap::<String, usize>::new();
    let mut fallbacks = Vec::<(String, OrderedFallbackRecord)>::new();

    // retain nonactive decisions in first-identity order
    for edge in edge_rows {
        let adjustment = object_field(&edge.row, "temperatureAdjustment")?;
        // keep active corrections in the primary population only
        if value_is_string(field(adjustment, "state"), "active") {
            continue;
        }
        let record = object_field(&edge.row, "record")?;
        let bundle = object_field(&edge.bundle_identities, "temperature")?;
        let identity_value = json_object(vec![
            ("record", record.clone()),
            ("settingsSha256", json_string(&edge.settings_sha256)),
            ("bundle", bundle.clone()),
        ]);
        let identity = canonical_identity(&identity_value, values)?;
        let captured = json_fields(vec![
            (
                "baseline",
                json_string("actual_serving_best_match_fallback"),
            ),
            ("bundleIdentities", edge.bundle_identities.clone()),
            (
                "capturedBundleSha256",
                required_field(adjustment, "bundleSha256")?.clone(),
            ),
            ("evidenceClass", json_string("as_issued")),
            (
                "firstEdgeCommittedAt",
                json_string(&edge.first_edge_committed_at),
            ),
            (
                "provenanceComplete",
                JsonValue::Boolean(boolean_field_equals(&edge.row, "provenanceComplete", true)),
            ),
            (
                "rawBestMatchTemperatureC",
                required_field(adjustment, "rawBestMatchTemperatureC")?.clone(),
            ),
            (
                "reasonCode",
                required_field(adjustment, "reasonCode")?.clone(),
            ),
            ("record", record.clone()),
            ("rowIdentity", json_string(&identity)),
            (
                "servingIdentitySha256",
                required_field(bundle, "activeBundle")?.clone(),
            ),
            ("settingsSha256", json_string(&edge.settings_sha256)),
            ("sourceReceiptAt", json_string(&edge.source_receipt_at)),
            ("state", required_field(adjustment, "state")?.clone()),
            ("validAt", required_field(record, "validAt")?.clone()),
        ]);

        // replace at the original insertion position only for an earlier receipt
        if let Some(index) = fallback_indexes.get(&identity).copied() {
            let (previous_committed_at, previous) = &mut fallbacks[index];
            // preserve the lexically earliest whole committed receipt
            if edge.first_edge_committed_at.as_str() < previous_committed_at.as_str() {
                *previous_committed_at = edge.first_edge_committed_at.clone();
                *previous = captured;
            }
        } else {
            fallback_indexes.insert(identity, fallbacks.len());
            fallbacks.push((edge.first_edge_committed_at.clone(), captured));
        }
    }

    Ok(fallbacks.into_iter().map(|(_, value)| value).collect())
}

/// Serialize and hash one legacy canonical identity object.
fn canonical_identity(
    value: &JsonValue,
    values: &mut impl TemperaturePerformanceDataProjectionValues,
) -> Result<String, TemperaturePerformanceDataProjectionError> {
    let bytes = values
        .encode_canonical_json(value)
        .ok_or(TemperaturePerformanceDataProjectionError::CanonicalIdentity)?;
    let digest = values
        .sha256(&bytes)
        .ok_or(TemperaturePerformanceDataProjectionError::CanonicalIdentity)?;
    Ok(digest.as_str().to_owned())
}

/// Copy verified package inputs into the generated contract type.
fn package_inputs(
    inputs: &RainPerformanceReportPackageInputs,
) -> ForecastAdjustmentPerformancePackageInputs {
    ForecastAdjustmentPerformancePackageInputs {
        adjustmentEvidenceManifestSha256: inputs.adjustment_evidence_manifest_sha256.clone(),
        adjustmentEvidenceWatermarkSha256: inputs.adjustment_evidence_watermark_sha256.clone(),
        forecastTrainingManifestSha256: inputs.forecast_training_manifest_sha256.clone(),
        localDateFrom: inputs.local_date_from.clone(),
        localDateTo: inputs.local_date_to.clone(),
        targetCutoffAt: inputs.target_cutoff_at.clone(),
    }
}

/// Adapt the generated data contract to the existing canonical JSON boundary.
fn temperature_data_wire_json(data: TemperaturePerformanceDataWire) -> JsonValue {
    json_object(vec![
        (
            "actualServingFallbacks",
            JsonValue::Array(
                data.actualServingFallbacks
                    .into_iter()
                    .map(JsonValue::Object)
                    .collect(),
            ),
        ),
        ("inputs", package_inputs_json(data.inputs)),
        (
            "rows",
            JsonValue::Array(
                data.rows
                    .into_iter()
                    .map(temperature_data_row_json)
                    .collect(),
            ),
        ),
    ])
}

/// Adapt generated package inputs to their original JavaScript field shape.
fn package_inputs_json(inputs: ForecastAdjustmentPerformancePackageInputs) -> JsonValue {
    json_object(vec![
        (
            "adjustmentEvidenceManifestSha256",
            json_string(&inputs.adjustmentEvidenceManifestSha256),
        ),
        (
            "adjustmentEvidenceWatermarkSha256",
            json_string(&inputs.adjustmentEvidenceWatermarkSha256),
        ),
        (
            "forecastTrainingManifestSha256",
            json_string(&inputs.forecastTrainingManifestSha256),
        ),
        ("localDateFrom", json_string(&inputs.localDateFrom)),
        ("localDateTo", json_string(&inputs.localDateTo)),
        ("targetCutoffAt", json_string(&inputs.targetCutoffAt)),
    ])
}

/// Adapt one generated temperature row to the legacy JSON object.
fn temperature_data_row_json(row: TemperaturePerformanceDataRow) -> JsonValue {
    json_object(vec![
        (
            "actualTemperatureC",
            nullable_number_json(row.actualTemperatureC),
        ),
        (
            "bestMatchRawTemperatureC",
            nullable_number_json(row.bestMatchRawTemperatureC),
        ),
        (
            "cohort",
            json_string(TemperaturePerformanceDataRowCohortLiteral::WIRE_VALUE),
        ),
        ("key", json_string(&row.key)),
        ("modelCycle", json_string(&row.modelCycle)),
        ("modelLeadHours", JsonValue::Number(row.modelLeadHours)),
        (
            "operationalHorizonHours",
            JsonValue::Number(row.operationalHorizonHours),
        ),
        (
            "rawRelativeHumidityPercent",
            nullable_number_json(row.rawRelativeHumidityPercent),
        ),
        ("rawTemperatureC", JsonValue::Number(row.rawTemperatureC)),
        ("rawWindSpeedMps", nullable_number_json(row.rawWindSpeedMps)),
        (
            "recordedRuntimeResult",
            recorded_runtime_result_json(row.recordedRuntimeResult),
        ),
        ("recentErrorState", row.recentErrorState),
        ("runInitializedAt", json_string(&row.runInitializedAt)),
        (
            "scoredPairMetadata",
            scored_pair_metadata_json(row.scoredPairMetadata),
        ),
        ("scoreEligible", JsonValue::Boolean(row.scoreEligible)),
        ("targetIdentity", nullable_string_json(row.targetIdentity)),
        (
            "targetMaxReceiptAt",
            nullable_string_json(row.targetMaxReceiptAt),
        ),
        ("validAt", json_string(&row.validAt)),
    ])
}

/// Adapt one generated runtime result to JSON null or its legacy object.
fn recorded_runtime_result_json(
    value: Nullable<TemperaturePerformanceDataRowRecordedRuntimeResult>,
) -> JsonValue {
    let Nullable::Value(value) = value else {
        return JsonValue::Null;
    };
    json_object(vec![
        ("applied", JsonValue::Boolean(value.applied)),
        (
            "predictionTemperatureC",
            JsonValue::Number(value.predictionTemperatureC),
        ),
        ("reasonCode", nullable_string_json(value.reasonCode)),
        (
            "scoredPairMetadata",
            scored_pair_metadata_json(value.scoredPairMetadata),
        ),
        (
            "servingBundleSha256",
            json_string(&value.servingBundleSha256),
        ),
    ])
}

/// Adapt generated scored-pair metadata to its legacy JSON object.
fn scored_pair_metadata_json(value: TemperaturePerformanceScoredPairMetadata) -> JsonValue {
    json_object(vec![
        (
            "evidenceClass",
            json_string(evidence_class_wire(value.evidenceClass)),
        ),
        ("fallback", JsonValue::Boolean(value.fallback)),
        (
            "firstEdgeCommittedAt",
            nullable_string_json(value.firstEdgeCommittedAt),
        ),
        ("horizonHours", JsonValue::Number(value.horizonHours)),
        ("key", json_string(&value.key)),
        ("localDate", json_string(&value.localDate)),
        (
            "provenanceComplete",
            JsonValue::Boolean(value.provenanceComplete),
        ),
        ("rowIdentity", json_string(&value.rowIdentity)),
        (
            "sourceReceiptAt",
            nullable_string_json(value.sourceReceiptAt),
        ),
        ("targetKey", json_string(&value.targetKey)),
        ("validAt", json_string(&value.validAt)),
        ("vintageKey", json_string(&value.vintageKey)),
    ])
}

/// Return one generated evidence class's exact wire spelling.
const fn evidence_class_wire(
    value: TemperaturePerformanceScoredPairMetadataEvidenceClassEnum,
) -> &'static str {
    // preserve every generated enum wire value
    match value {
        TemperaturePerformanceScoredPairMetadataEvidenceClassEnum::AsIssued => "as_issued",
        TemperaturePerformanceScoredPairMetadataEvidenceClassEnum::ProspectiveReceipt => {
            "prospective_receipt"
        }
        TemperaturePerformanceScoredPairMetadataEvidenceClassEnum::RetrospectiveCounterfactual => {
            "retrospective_counterfactual"
        }
        TemperaturePerformanceScoredPairMetadataEvidenceClassEnum::Development => "development",
    }
}

/// Borrow one required object member.
fn object_field<'a>(
    value: &'a JsonValue,
    key: &str,
) -> Result<&'a JsonValue, TemperaturePerformanceDataProjectionError> {
    require_object(required_field(value, key)?)
}

/// Require one JSON object value.
fn require_object(
    value: &JsonValue,
) -> Result<&JsonValue, TemperaturePerformanceDataProjectionError> {
    // reject nonobject values at the original object boundary
    if !matches!(value, JsonValue::Object(_)) {
        return Err(TemperaturePerformanceDataProjectionError::InvalidData);
    }
    Ok(value)
}

/// Borrow one required member without accepting JavaScript `undefined`.
fn required_field<'a>(
    value: &'a JsonValue,
    key: &str,
) -> Result<&'a JsonValue, TemperaturePerformanceDataProjectionError> {
    field(value, key).ok_or(TemperaturePerformanceDataProjectionError::InvalidData)
}

/// Decode one required valid UTF-16 string member.
fn required_text(
    value: &JsonValue,
    key: &str,
) -> Result<String, TemperaturePerformanceDataProjectionError> {
    nonempty_json_text(required_field(value, key)?)
}

/// Decode one required nonempty JSON string.
fn nonempty_json_text(
    value: &JsonValue,
) -> Result<String, TemperaturePerformanceDataProjectionError> {
    let value = json_text(value)?;
    // reject empty identifiers at the original text boundary
    if value.is_empty() {
        return Err(TemperaturePerformanceDataProjectionError::InvalidData);
    }
    Ok(value)
}

/// Decode one JSON string without replacing invalid UTF-16.
fn json_text(value: &JsonValue) -> Result<String, TemperaturePerformanceDataProjectionError> {
    let JsonValue::String(units) = value else {
        return Err(TemperaturePerformanceDataProjectionError::InvalidData);
    };
    String::from_utf16(units).map_err(|_| TemperaturePerformanceDataProjectionError::InvalidData)
}

/// Decode one required finite numeric member.
fn required_finite(
    value: &JsonValue,
    key: &str,
) -> Result<f64, TemperaturePerformanceDataProjectionError> {
    let JsonValue::Number(number) = required_field(value, key)? else {
        return Err(TemperaturePerformanceDataProjectionError::InvalidData);
    };
    // reject nonfinite report numbers
    if !number.is_finite() {
        return Err(TemperaturePerformanceDataProjectionError::InvalidData);
    }
    Ok(*number)
}

/// Preserve one nullable finite numeric member in the generated contract.
fn nullable_finite(
    value: &JsonValue,
    key: &str,
) -> Result<Nullable<f64>, TemperaturePerformanceDataProjectionError> {
    // retain explicit null
    if matches!(required_field(value, key)?, JsonValue::Null) {
        return Ok(Nullable::Null);
    }
    Ok(Nullable::Value(required_finite(value, key)?))
}

/// Compare one member to an exact JSON string.
fn string_field_equals(value: &JsonValue, key: &str, expected: &str) -> bool {
    value_is_string(field(value, key), expected)
}

/// Compare one optional value to an exact JSON string.
fn value_is_string(value: Option<&JsonValue>, expected: &str) -> bool {
    matches!(value, Some(JsonValue::String(units)) if units.iter().copied().eq(expected.encode_utf16()))
}

/// Compare one member to an exact JSON boolean.
fn boolean_field_equals(value: &JsonValue, key: &str, expected: bool) -> bool {
    matches!(field(value, key), Some(JsonValue::Boolean(actual)) if *actual == expected)
}

/// Apply JavaScript strict equality to optional decoded JSON members.
fn strict_optional_equal(left: Option<&JsonValue>, right: Option<&JsonValue>) -> bool {
    // preserve javascript undefined equality
    match (left, right) {
        (None, None) => true,
        (Some(JsonValue::Null), Some(JsonValue::Null)) => true,
        (Some(JsonValue::Boolean(left)), Some(JsonValue::Boolean(right))) => left == right,
        (Some(JsonValue::Number(left)), Some(JsonValue::Number(right))) => left == right,
        (Some(JsonValue::String(left)), Some(JsonValue::String(right))) => left == right,
        _ => false,
    }
}

/// Apply JavaScript `Object.is` to optional decoded JSON members.
fn object_is_optional(left: Option<&JsonValue>, right: Option<&JsonValue>) -> bool {
    // preserve javascript undefined identity
    match (left, right) {
        (None, None) => true,
        (Some(JsonValue::Null), Some(JsonValue::Null)) => true,
        (Some(JsonValue::Boolean(left)), Some(JsonValue::Boolean(right))) => left == right,
        (Some(JsonValue::String(left)), Some(JsonValue::String(right))) => left == right,
        (Some(JsonValue::Number(left)), Some(JsonValue::Number(right))) => {
            // preserve nan identity and signed zero distinction
            if left.is_nan() && right.is_nan() {
                true
            } else if *left == 0.0 && *right == 0.0 {
                left.is_sign_negative() == right.is_sign_negative()
            } else {
                left == right
            }
        }
        _ => false,
    }
}

/// Convert one optional value into the generated nullable contract.
fn nullable_value<T>(value: Option<T>) -> Nullable<T> {
    value.map_or(Nullable::Null, Nullable::Value)
}

/// Copy one optional borrowed string into the generated nullable contract.
fn nullable_string(value: Option<&str>) -> Nullable<String> {
    value.map_or(Nullable::Null, |value| Nullable::Value(value.to_owned()))
}

/// Encode one generated nullable number as JSON null or number.
fn nullable_number_json(value: Nullable<f64>) -> JsonValue {
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => JsonValue::Number(value),
    }
}

/// Encode one generated nullable string as JSON null or string.
fn nullable_string_json(value: Nullable<String>) -> JsonValue {
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => json_string(&value),
    }
}

/// Build one generated JSON string without wire serialization.
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one generated JSON object with the declared semantic fields.
fn json_object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(json_fields(fields))
}

/// Build one generated JSON object field vector in declared order.
fn json_fields(fields: Vec<(&str, JsonValue)>) -> Vec<(Vec<u16>, JsonValue)> {
    fields
        .into_iter()
        .map(|(name, value)| (name.encode_utf16().collect(), value))
        .collect()
}

#[cfg(test)]
#[path = "temperature_performance_data_projection_test.rs"]
mod tests;
