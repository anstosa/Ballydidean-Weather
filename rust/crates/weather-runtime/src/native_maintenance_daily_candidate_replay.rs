//! Native candidate-versus-package replay for admitted daily model rows.

use weather_contracts::{
    AdjustmentShadowFamily, ForecastAdjustmentMaintenanceRuntimePackage, JsonValue,
    RainHurdleWindFeaturePerformance, RainHurdleWindPortableArtifact,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_worker::RainPortableReplayCalendar;

use crate::api_maintenance_candidate_catalog::ApiShadowInstalledReceipt;
use crate::api_maintenance_candidate_package::admit_api_shadow_package;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_candidate_parity::{
    NativeMaintenancePortableCandidate, admit_package as admit_wind_package,
    evaluate_wind as evaluate_wind_package, package_from_candidate as wind_package_from_candidate,
};
use crate::native_maintenance_daily_evaluation_input::NativeMaintenanceDailyAdmittedRecord;
use crate::native_maintenance_policy_comparison::{number, object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    MaintenanceCanonicalJson, NativeMaintenanceCaptureFamily, NativeModelEvidenceValues,
    NativeRainPortableCalendar, NativeWirePrimitives, evaluate_native_temperature_fit_batch,
};

const TEMPERATURE_FIT_FIELDS: &[&str] = &[
    "arms",
    "confirmationOpened",
    "contractVersion",
    "developmentIncumbentMae",
    "developmentRawMae",
    "dueMonth",
    "model",
    "selectedArm",
    "servingChanged",
    "state",
];
const HOUR: i64 = 3_600_000;

/// Value-free reasons an admitted daily row cannot be replayed through both model forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeMaintenanceDailyCandidateReplayError {
    Family,
    Candidate,
    Package,
    Input,
    Evaluation,
    Canonical,
}

/// Exact canonical decisions produced independently from fitted and packaged model material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeMaintenanceDailyCandidateReplayPair {
    pub candidate_decision_bytes: Vec<u8>,
    pub packaged_decision_bytes: Vec<u8>,
    pub portable_candidate_bytes: Vec<u8>,
}

/// Replay one already admitted daily record through raw fit and retained package material.
pub fn replay_native_maintenance_daily_candidate_pair(
    record: &NativeMaintenanceDailyAdmittedRecord,
    candidate_bytes: &[u8],
    artifact_bytes: &[u8],
) -> Result<NativeMaintenanceDailyCandidateReplayPair, NativeMaintenanceDailyCandidateReplayError> {
    // dispatch only families with complete raw and packaged native replay owners
    match record.family {
        NativeMaintenanceCaptureFamily::Temperature => {
            replay_native_temperature_daily_candidate_pair(
                &record.artifact_evaluator_input,
                candidate_bytes,
                artifact_bytes,
            )
        }
        NativeMaintenanceCaptureFamily::Wind => replay_native_wind_daily_candidate_pair(
            &record.artifact_evaluator_input,
            candidate_bytes,
            artifact_bytes,
        ),
        NativeMaintenanceCaptureFamily::Rain => crate::replay_native_rain_causal_candidate_pair(
            &record.artifact_evaluator_input,
            candidate_bytes,
            artifact_bytes,
        ),
    }
}

/// Build identities and typed numerical material for one selected raw rain fit.
pub struct NativeMaintenanceRainPortableCandidate {
    pub artifact: RainHurdleWindPortableArtifact,
    pub artifact_bytes: Vec<u8>,
    pub artifact_sha256: String,
    pub candidate_sha256: String,
}

/// Replay one exact fitted feature row through raw-fit and supplied rain artifact material.
pub fn replay_native_rain_feature_candidate_pair(
    input: &JsonValue,
    candidate_bytes: &[u8],
    artifact_bytes: &[u8],
) -> Result<NativeMaintenanceDailyCandidateReplayPair, NativeMaintenanceDailyCandidateReplayError> {
    let candidate = build_native_rain_maintenance_artifact(candidate_bytes)?;
    let artifact = admit_rain_artifact_bytes(artifact_bytes)?;
    let candidate_decision = evaluate_rain_feature(&candidate.artifact, input)?;
    let packaged_decision = evaluate_rain_feature(&artifact, input)?;
    let wire = NativeWirePrimitives;
    Ok(NativeMaintenanceDailyCandidateReplayPair {
        candidate_decision_bytes: wire
            .encode_sorted_json(&candidate_decision)
            .ok_or(NativeMaintenanceDailyCandidateReplayError::Canonical)?,
        packaged_decision_bytes: wire
            .encode_sorted_json(&packaged_decision)
            .ok_or(NativeMaintenanceDailyCandidateReplayError::Canonical)?,
        portable_candidate_bytes: candidate.artifact_bytes,
    })
}

/// Project and admit the predecessor's portable runtime from one selected raw rain fit.
pub fn build_native_rain_maintenance_artifact(
    candidate_bytes: &[u8],
) -> Result<NativeMaintenanceRainPortableCandidate, NativeMaintenanceDailyCandidateReplayError> {
    let mut wire = NativeWirePrimitives;
    let candidate = wire
        .parse_canonical_object(candidate_bytes)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?;
    // close the selected fit envelope before reducing its development-only artifact
    if !exact_keys(
        &candidate,
        &[
            "artifact",
            "confirmationOpened",
            "contractVersion",
            "reason",
            "state",
        ],
    ) || boolean(&candidate, "confirmationOpened") != Some(false)
        || text(&candidate, "state").as_deref() != Some("development_candidate")
        || text(&candidate, "reason").as_deref() != Some("development_gate_passer")
    {
        return Err(NativeMaintenanceDailyCandidateReplayError::Candidate);
    }
    let contract = text(&candidate, "contractVersion")
        .filter(|value| {
            matches!(
                value.as_str(),
                "rain-maintenance-fit/v2" | "rain-maintenance-fit/v3"
            )
        })
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?;
    let artifact = field(&candidate, "artifact")
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?;
    let calibration = field(artifact, "calibration")
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?;
    // bind the fitted artifact and its calibration contract before selecting runtime members
    if text(artifact, "contractVersion").as_deref() != Some("rain-maintenance-artifact/v2")
        || text(artifact, "projectionId").is_none_or(|value| value.is_empty())
        || text(calibration, "contractVersion").as_deref() != Some("rain-hurdle-calibration/v1")
    {
        return Err(NativeMaintenanceDailyCandidateReplayError::Candidate);
    }
    let rules = rain_runtime_rules(
        field(calibration, "rules").ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?,
    )?;
    let category_scales = rain_category_scales(
        field(calibration, "categories")
            .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?,
    )?;
    let mut fields = vec![
        ("categoryScales", JsonValue::Array(category_scales)),
        (
            "contractVersion",
            string(if contract == "rain-maintenance-fit/v3" {
                "rain-hurdle-wind-runtime/v2"
            } else {
                "rain-hurdle-wind-runtime/v1"
            }),
        ),
        (
            "featureNames",
            field(artifact, "featureNames")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?,
        ),
        (
            "heads",
            field(artifact, "heads")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?,
        ),
        (
            "modelMonth",
            field(artifact, "modelMonth")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?,
        ),
        ("rules", JsonValue::Array(rules)),
    ];
    // retain the reviewed v3 projection identity only in the additive v2 runtime
    if contract == "rain-maintenance-fit/v3" {
        fields.push((
            "projectionId",
            field(artifact, "projectionId")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?,
        ));
    }
    let document = object(fields);
    let artifact_bytes = wire
        .encode_sorted_json(&document)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Canonical)?;
    let typed = admit_rain_artifact(&document)?;
    Ok(NativeMaintenanceRainPortableCandidate {
        artifact: typed,
        artifact_sha256: candidate_sha256(&artifact_bytes),
        artifact_bytes,
        candidate_sha256: candidate_sha256(candidate_bytes),
    })
}

// admit exact canonical public artifact bytes through the existing generated model boundary
fn admit_rain_artifact_bytes(
    bytes: &[u8],
) -> Result<RainHurdleWindPortableArtifact, NativeMaintenanceDailyCandidateReplayError> {
    let mut wire = NativeWirePrimitives;
    let document = wire
        .parse_canonical_object(bytes)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Package)?;
    admit_rain_artifact(&document)
}

// close decoded rain tables and the fixed feature-name identity through the API owner
fn admit_rain_artifact(
    document: &JsonValue,
) -> Result<RainHurdleWindPortableArtifact, NativeMaintenanceDailyCandidateReplayError> {
    let values = NativeModelEvidenceValues::new();
    weather_api::admit_rain_portable_artifact(
        document,
        |value| values.decode_shadow_rain_artifact(value),
        |names| {
            let bytes = values.encode_shadow_ordered_json(names)?;
            ApiMaintenanceReceiptCanonicalValues::sha256(&values, &bytes)
        },
    )
    .ok_or(NativeMaintenanceDailyCandidateReplayError::Package)
}

// evaluate the closed feature-parity contract through one typed portable model
fn evaluate_rain_feature(
    artifact: &RainHurdleWindPortableArtifact,
    input: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceDailyCandidateReplayError> {
    let version =
        text(input, "contractVersion").ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?;
    let v2 = version == "rain-maintenance-feature-parity-input/v2";
    // require the exact versioned feature input without accepting causal-source substitution
    if !matches!(
        version.as_str(),
        "rain-maintenance-feature-parity-input/v1" | "rain-maintenance-feature-parity-input/v2"
    ) || !exact_keys(
        input,
        if v2 {
            &["contractVersion", "featureNames", "features", "validAt"]
        } else {
            &["contractVersion", "featureNames", "features"]
        },
    ) {
        return Err(NativeMaintenanceDailyCandidateReplayError::Input);
    }
    let feature_names =
        field(input, "featureNames").ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?;
    let expected_names = rain_feature_names(artifact);
    let JsonValue::Array(actual_names) = feature_names else {
        return Err(NativeMaintenanceDailyCandidateReplayError::Input);
    };
    // preserve exact trained feature order and width before float32 conversion
    if actual_names.len() != expected_names.len()
        || actual_names
            .iter()
            .zip(expected_names)
            .any(|(actual, expected)| text_scalar(actual).as_deref() != Some(expected))
    {
        return Err(NativeMaintenanceDailyCandidateReplayError::Input);
    }
    let JsonValue::Array(values) =
        field(input, "features").ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?
    else {
        return Err(NativeMaintenanceDailyCandidateReplayError::Input);
    };
    let features = values
        .iter()
        .map(|value| match value {
            JsonValue::Null => Some(f32::NAN),
            JsonValue::Number(value) if value.is_finite() => {
                let narrowed = *value as f32;
                narrowed.is_finite().then_some(narrowed)
            }
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?;
    // bind the frozen raw precipitation feature before model evaluation
    if features.len() != 107 || !features[5].is_finite() || features[5] < 0.0 {
        return Err(NativeMaintenanceDailyCandidateReplayError::Input);
    }
    let model = weather_api::RainHurdleWindPortableModel::new(artifact)
        .map_err(|_| NativeMaintenanceDailyCandidateReplayError::Evaluation)?;
    let local_month = if v2 {
        let valid_at =
            text(input, "validAt").ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?;
        let mut calendar = NativeRainPortableCalendar::load()
            .map_err(|_| NativeMaintenanceDailyCandidateReplayError::Evaluation)?;
        Some(
            calendar
                .local_month(&valid_at)
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?,
        )
    } else {
        None
    };
    let result = model
        .predict(&features, local_month)
        .map_err(|_| NativeMaintenanceDailyCandidateReplayError::Evaluation)?;
    rain_feature_result(result)
}

// project the native feature performance into the exact public JSON field names
fn rain_feature_result(
    result: RainHurdleWindFeaturePerformance,
) -> Result<JsonValue, NativeMaintenanceDailyCandidateReplayError> {
    let values = [
        result.correctedPrecipitationMm,
        result.occurrenceProbabilityAtLeast0_1,
        result.occurrenceProbabilityAtLeast1_0,
        result.occurrenceProbabilityAtLeast2_5,
        result.positiveAmountMm,
    ];
    // JSON cannot preserve a nonfinite runtime result returned by malformed model material
    if values.iter().any(|value| !value.is_finite()) {
        return Err(NativeMaintenanceDailyCandidateReplayError::Evaluation);
    }
    Ok(object(vec![
        (
            "correctedPrecipitationMm",
            JsonValue::Number(result.correctedPrecipitationMm),
        ),
        (
            "occurrenceProbabilityAtLeast0_1",
            JsonValue::Number(result.occurrenceProbabilityAtLeast0_1),
        ),
        (
            "occurrenceProbabilityAtLeast1_0",
            JsonValue::Number(result.occurrenceProbabilityAtLeast1_0),
        ),
        (
            "occurrenceProbabilityAtLeast2_5",
            JsonValue::Number(result.occurrenceProbabilityAtLeast2_5),
        ),
        (
            "positiveAmountMm",
            JsonValue::Number(result.positiveAmountMm),
        ),
    ]))
}

// project the three physical calibration rules while dropping development-only metadata
fn rain_runtime_rules(
    value: &JsonValue,
) -> Result<Vec<JsonValue>, NativeMaintenanceDailyCandidateReplayError> {
    let JsonValue::Array(rules) = value else {
        return Err(NativeMaintenanceDailyCandidateReplayError::Candidate);
    };
    let thresholds = [0.1, 1.0, 2.5];
    // retain the fixed threshold order and only finite bounded optional cutoffs
    if rules.len() != thresholds.len() {
        return Err(NativeMaintenanceDailyCandidateReplayError::Candidate);
    }
    rules
        .iter()
        .zip(thresholds)
        .map(|(rule, threshold)| {
            if number(rule, "threshold") != Some(threshold) {
                return Err(NativeMaintenanceDailyCandidateReplayError::Candidate);
            }
            let cutoff = match field(rule, "cutoff") {
                Some(JsonValue::Null) => JsonValue::Null,
                Some(JsonValue::Number(value))
                    if value.is_finite() && (0.0..=1.0).contains(value) =>
                {
                    JsonValue::Number(*value)
                }
                _ => return Err(NativeMaintenanceDailyCandidateReplayError::Candidate),
            };
            Ok(object(vec![
                ("cutoff", cutoff),
                ("threshold", JsonValue::Number(threshold)),
            ]))
        })
        .collect()
}

// project the three serving category scales from their fitted category records
fn rain_category_scales(
    value: &JsonValue,
) -> Result<Vec<JsonValue>, NativeMaintenanceDailyCandidateReplayError> {
    ["1", "2", "3"]
        .iter()
        .map(|category| {
            let scale = number(
                field(value, category)
                    .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?,
                "scale",
            )
            .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?;
            (0.1..=3.0)
                .contains(&scale)
                .then_some(JsonValue::Number(scale))
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)
        })
        .collect()
}

// borrow the trained feature-name order from either generated artifact version
fn rain_feature_names(artifact: &RainHurdleWindPortableArtifact) -> &[String] {
    match artifact {
        RainHurdleWindPortableArtifact::V1(artifact) => &artifact.featureNames,
        RainHurdleWindPortableArtifact::V2(artifact) => &artifact.featureNames,
    }
}

// decode one JSON string scalar outside an object member
fn text_scalar(value: &JsonValue) -> Option<String> {
    let JsonValue::String(value) = value else {
        return None;
    };
    String::from_utf16(value).ok()
}

/// Replay one source-shaped wind input through raw-fit and supplied package material.
pub fn replay_native_wind_daily_candidate_pair(
    input: &JsonValue,
    candidate_bytes: &[u8],
    artifact_bytes: &[u8],
) -> Result<NativeMaintenanceDailyCandidateReplayPair, NativeMaintenanceDailyCandidateReplayError> {
    let (candidate_document, candidate_package) = wind_package_from_candidate(candidate_bytes)
        .map_err(|_| NativeMaintenanceDailyCandidateReplayError::Candidate)?;
    let portable_candidate_bytes = NativeWirePrimitives
        .encode_sorted_json(&candidate_document)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Canonical)?;
    let artifact_document = NativeWirePrimitives
        .decode_json(artifact_bytes)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Package)?;
    let artifact_sha256 = text(&artifact_document, "bundleSha256")
        .filter(|value| valid_hash(value))
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Package)?;
    let artifact_candidate_sha256 = text(&artifact_document, "candidateSha256")
        .filter(|value| valid_hash(value))
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Package)?;
    // bind the independently admitted artifact to the raw fit without equating model bytes
    if artifact_candidate_sha256 != candidate_package.candidateSha256 {
        return Err(NativeMaintenanceDailyCandidateReplayError::Package);
    }
    let source = field(&artifact_document, "source")
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Package)?;
    let source_identity_sha256 = canonical_sha256(source)?;
    let portable = NativeMaintenancePortableCandidate {
        artifact_bytes: artifact_bytes.to_vec(),
        artifact_sha256,
        candidate_sha256: artifact_candidate_sha256,
        source_identity_sha256,
    };
    let artifact_package = admit_wind_package(&portable)
        .map_err(|_| NativeMaintenanceDailyCandidateReplayError::Package)?;
    let candidate_decision_bytes = evaluate_wind_package(&candidate_package, input)
        .map_err(|_| NativeMaintenanceDailyCandidateReplayError::Evaluation)?;
    let packaged_decision_bytes = evaluate_wind_package(&artifact_package, input)
        .map_err(|_| NativeMaintenanceDailyCandidateReplayError::Evaluation)?;
    Ok(NativeMaintenanceDailyCandidateReplayPair {
        candidate_decision_bytes,
        packaged_decision_bytes,
        portable_candidate_bytes,
    })
}

/// Replay one source-shaped temperature input without requiring the surrounding archive record.
pub fn replay_native_temperature_daily_candidate_pair(
    input: &JsonValue,
    candidate_bytes: &[u8],
    artifact_bytes: &[u8],
) -> Result<NativeMaintenanceDailyCandidateReplayPair, NativeMaintenanceDailyCandidateReplayError> {
    let (portable_candidate_bytes, candidate_model, candidate_bundle) =
        build_native_temperature_maintenance_package(candidate_bytes)?;
    let (artifact_model, artifact_bundle) =
        admit_temperature_package(artifact_bytes, Some(candidate_sha256(candidate_bytes)))?;
    let wire = NativeWirePrimitives;
    let candidate_decision = temperature_decision(&candidate_bundle, &candidate_model, input)?;
    let packaged_decision = temperature_decision(&artifact_bundle, &artifact_model, input)?;
    let candidate_decision_bytes = wire
        .encode_sorted_json(&candidate_decision)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Canonical)?;
    let packaged_decision_bytes = wire
        .encode_sorted_json(&packaged_decision)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Canonical)?;
    Ok(NativeMaintenanceDailyCandidateReplayPair {
        candidate_decision_bytes,
        packaged_decision_bytes,
        portable_candidate_bytes,
    })
}

/// Build the predecessor's exact authority-free temperature package from canonical fit bytes.
pub fn build_native_temperature_maintenance_package(
    candidate_bytes: &[u8],
) -> Result<(Vec<u8>, JsonValue, String), NativeMaintenanceDailyCandidateReplayError> {
    let mut wire = NativeWirePrimitives;
    let fit = wire
        .parse_canonical_object(candidate_bytes)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?;
    // require the selected raw-fit grammar before projecting portable model material
    if !exact_keys(&fit, TEMPERATURE_FIT_FIELDS)
        || text(&fit, "contractVersion").as_deref() != Some("temperature-maintenance-fit/v2")
        || text(&fit, "state").as_deref() != Some("development_candidate")
        || boolean(&fit, "confirmationOpened") != Some(false)
        || boolean(&fit, "servingChanged") != Some(false)
        || text(&fit, "selectedArm").is_none_or(|value| value.is_empty())
    {
        return Err(NativeMaintenanceDailyCandidateReplayError::Candidate);
    }
    let due_month = text(&fit, "dueMonth")
        .filter(|value| valid_month(value))
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?;
    let model = field(&fit, "model")
        .cloned()
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Candidate)?;
    let candidate_sha256 = candidate_sha256(candidate_bytes);
    let source = temperature_source();
    let unsigned = object(vec![
        ("candidateSha256", string(&candidate_sha256)),
        (
            "contractVersion",
            string("forecast-adjustment-maintenance-runtime-package/v1"),
        ),
        ("dueMonth", string(&due_month)),
        ("family", string("temperature")),
        ("model", model.clone()),
        ("source", source.clone()),
    ]);
    let bundle_sha256 = canonical_sha256(&unsigned)?;
    let package = object(vec![
        ("bundleSha256", string(&bundle_sha256)),
        ("candidateSha256", string(&candidate_sha256)),
        (
            "contractVersion",
            string("forecast-adjustment-maintenance-runtime-package/v1"),
        ),
        ("dueMonth", string(&due_month)),
        ("family", string("temperature")),
        ("model", model),
        ("source", source),
    ]);
    let bytes = wire
        .encode_canonical_json(&package)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Canonical)?;
    let (model, admitted_bundle) = admit_temperature_package(&bytes, Some(candidate_sha256))?;
    Ok((bytes, model, admitted_bundle))
}

// admit one generated package and retain its model JSON for the native numerical bridge
pub(crate) fn admit_temperature_package(
    bytes: &[u8],
    expected_candidate_sha256: Option<String>,
) -> Result<(JsonValue, String), NativeMaintenanceDailyCandidateReplayError> {
    let mut wire = NativeWirePrimitives;
    let document = wire
        .parse_canonical_object(bytes)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Package)?;
    let bundle_sha256 = text(&document, "bundleSha256")
        .filter(|value| valid_hash(value))
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Package)?;
    let candidate_sha256 = text(&document, "candidateSha256")
        .filter(|value| valid_hash(value))
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Package)?;
    // bind packaged evaluation to the independently supplied raw fit identity
    if expected_candidate_sha256
        .as_deref()
        .is_some_and(|expected| expected != candidate_sha256)
    {
        return Err(NativeMaintenanceDailyCandidateReplayError::Package);
    }
    let receipt = package_receipt(candidate_sha256, bundle_sha256.clone());
    let package = admit_api_shadow_package(
        AdjustmentShadowFamily::Temperature,
        &receipt,
        bytes,
        &mut wire,
    )
    .map_err(|_| NativeMaintenanceDailyCandidateReplayError::Package)?;
    let ForecastAdjustmentMaintenanceRuntimePackage::Temperature(_) = package else {
        return Err(NativeMaintenanceDailyCandidateReplayError::Package);
    };
    let model = field(&document, "model")
        .cloned()
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Package)?;
    Ok((model, bundle_sha256))
}

// evaluate one model after reproducing the source identity and chronology admission
pub(crate) fn temperature_decision(
    bundle_sha256: &str,
    model: &JsonValue,
    input: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceDailyCandidateReplayError> {
    let raw_best = nullable_number(input, "rawBestMatchTemperatureC")
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?;
    let Some(source) = field(input, "sourceForecast") else {
        return Err(NativeMaintenanceDailyCandidateReplayError::Input);
    };
    let Some(state) = field(input, "recentErrorState") else {
        return Err(NativeMaintenanceDailyCandidateReplayError::Input);
    };
    // retain the source missing-data fallback before interpreting model material
    if *source == JsonValue::Null || *state == JsonValue::Null {
        return Ok(temperature_fallback(
            raw_best,
            "missing_source_forecast",
            Some(bundle_sha256),
        ));
    }
    let source_admission = admit_temperature_source(input, source);
    let lead = match source_admission {
        Ok(lead) => lead,
        Err(reason) => {
            return Ok(temperature_fallback(raw_best, reason, Some(bundle_sha256)));
        }
    };
    let forecast = temperature_forecast(source)?;
    let bridge_input = object(vec![
        ("forecast", forecast),
        ("model", model.clone()),
        ("recentErrorState", state.clone()),
    ]);
    let batch = object(vec![
        ("inputs", JsonValue::Array(vec![bridge_input])),
        ("operation", string("infer")),
    ]);
    let JsonValue::Array(mut results) = evaluate_native_temperature_fit_batch(&batch)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Evaluation)?
    else {
        return Err(NativeMaintenanceDailyCandidateReplayError::Evaluation);
    };
    let result = results
        .pop()
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Evaluation)?;
    let source_forecast = source_with_operational_horizon(source, lead)?;
    let recent_error_sha256 = canonical_sha256(state)?;
    let applied = boolean(&result, "applied")
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Evaluation)?;
    // preserve applied and raw numerical result shapes as distinct decision states
    let (branch, corrected, reason, state_name) = if applied {
        (
            field(&result, "branch")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Evaluation)?,
            field(&result, "predictionTemperatureC")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Evaluation)?,
            JsonValue::Null,
            "active",
        )
    } else {
        (
            JsonValue::Null,
            JsonValue::Null,
            field(&result, "reason")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Evaluation)?,
            "raw_fallback",
        )
    };
    Ok(object(vec![
        ("branch", branch),
        ("bundleSha256", string(bundle_sha256)),
        (
            "contractVersion",
            string("forecast-temperature-canary-decision/v1"),
        ),
        ("correctedTemperatureC", corrected),
        (
            "rawBestMatchTemperatureC",
            raw_best.map_or(JsonValue::Null, JsonValue::Number),
        ),
        ("reasonCode", reason),
        ("recentErrorStateSha256", string(&recent_error_sha256)),
        ("sourceForecast", source_forecast),
        ("state", string(state_name)),
    ]))
}

// reproduce the fixed delayed-source identity and causal serving window
fn admit_temperature_source(input: &JsonValue, source: &JsonValue) -> Result<i64, &'static str> {
    let wire = NativeWirePrimitives;
    let evaluated = wire
        .canonical_utc_milliseconds(&text(input, "evaluatedAt").ok_or("source_identity_mismatch")?)
        .ok_or("source_identity_mismatch")?;
    let input_valid = text(input, "validAt").ok_or("source_identity_mismatch")?;
    let run = wire
        .canonical_utc_milliseconds(
            &text(source, "runInitializedAt").ok_or("source_identity_mismatch")?,
        )
        .ok_or("source_identity_mismatch")?;
    let received = wire
        .canonical_utc_milliseconds(
            &text(source, "firstReceivedAt").ok_or("source_identity_mismatch")?,
        )
        .ok_or("source_identity_mismatch")?;
    let valid_at = text(source, "validAt").ok_or("source_identity_mismatch")?;
    let valid = wire
        .canonical_utc_milliseconds(&valid_at)
        .ok_or("source_identity_mismatch")?;
    let lead = integer(source, "modelLeadHours").ok_or("outside_operational_window")?;
    // bind the exact public source family before its clocks
    if text(source, "adapterVersion").as_deref() != Some("open-meteo-ecmwf-single-run/v1")
        || text(source, "providerKey").as_deref() != Some("open-meteo")
        || text(source, "dataset").as_deref() != Some("single_run")
        || text(source, "upstreamModel").as_deref() != Some("ecmwf_ifs")
        || text(source, "providerResponseSha256").is_none_or(|value| !valid_hash(&value))
    {
        return Err("source_identity_mismatch");
    }
    // bind the source hour to this public request and exact model lead
    if valid_at != input_valid || valid.checked_sub(run) != lead.checked_mul(HOUR) {
        return Err("source_time_mismatch");
    }
    let assumed = run.checked_add(6 * HOUR).ok_or("source_not_available")?;
    let available = received.max(assumed);
    // reject backdated, unavailable, and stale receipts in source order
    if received < run || evaluated < available {
        return Err("source_not_available");
    }
    if evaluated
        .checked_sub(available)
        .is_none_or(|age| age > 12 * HOUR)
    {
        return Err("source_stale");
    }
    if valid <= evaluated || !(7..=18).contains(&lead) {
        return Err("outside_operational_window");
    }
    Ok(lead)
}

// project only the generated MOS forecast fields from the admitted source record
fn temperature_forecast(
    source: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceDailyCandidateReplayError> {
    let response = text(source, "providerResponseSha256")
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?;
    let valid_at =
        text(source, "validAt").ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?;
    Ok(object(vec![
        ("cohort", string("ecmwf_single_run_hindcast")),
        ("key", string(&format!("{response}:{valid_at}"))),
        (
            "modelCycle",
            field(source, "modelCycle")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?,
        ),
        (
            "modelLeadHours",
            field(source, "modelLeadHours")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?,
        ),
        (
            "rawRelativeHumidityPercent",
            field(source, "rawRelativeHumidityPercent")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?,
        ),
        (
            "rawTemperatureC",
            field(source, "rawTemperatureC")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?,
        ),
        (
            "rawWindSpeedMps",
            field(source, "rawWindSpeedMps")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?,
        ),
        (
            "runInitializedAt",
            field(source, "runInitializedAt")
                .cloned()
                .ok_or(NativeMaintenanceDailyCandidateReplayError::Input)?,
        ),
        ("validAt", string(&valid_at)),
    ]))
}

// append the one derived field retained by the public decision contract
fn source_with_operational_horizon(
    source: &JsonValue,
    lead: i64,
) -> Result<JsonValue, NativeMaintenanceDailyCandidateReplayError> {
    let JsonValue::Object(fields) = source else {
        return Err(NativeMaintenanceDailyCandidateReplayError::Input);
    };
    let mut output = fields.clone();
    output.push((
        "operationalHorizonHours".encode_utf16().collect(),
        JsonValue::Number((lead - 6) as f64),
    ));
    Ok(JsonValue::Object(output))
}

// produce the source's schema-complete failure without retaining unadmitted material
fn temperature_fallback(
    raw_best: Option<f64>,
    reason: &str,
    bundle_sha256: Option<&str>,
) -> JsonValue {
    object(vec![
        ("branch", JsonValue::Null),
        (
            "bundleSha256",
            bundle_sha256.map_or(JsonValue::Null, string),
        ),
        (
            "contractVersion",
            string("forecast-temperature-canary-decision/v1"),
        ),
        ("correctedTemperatureC", JsonValue::Null),
        (
            "rawBestMatchTemperatureC",
            raw_best.map_or(JsonValue::Null, JsonValue::Number),
        ),
        ("reasonCode", string(reason)),
        ("recentErrorStateSha256", JsonValue::Null),
        ("sourceForecast", JsonValue::Null),
        ("state", string("raw_fallback")),
    ])
}

// construct the exact package source literal owned by the predecessor runtime
fn temperature_source() -> JsonValue {
    object(vec![
        ("adapterVersion", string("open-meteo-ecmwf-single-run/v1")),
        ("cohort", string("ecmwf_single_run_hindcast")),
        ("dataset", string("single_run")),
        ("maximumReceiptAgeHours", JsonValue::Number(12.0)),
        ("providerKey", string("open-meteo")),
        ("scope", string("assumed_delay6_next12")),
        ("sourceDelayHours", JsonValue::Number(6.0)),
        ("upstreamModel", string("ecmwf_ifs")),
    ])
}

// retain only the package identities inspected by its existing generated verifier
fn package_receipt(candidate_sha256: String, bundle_sha256: String) -> ApiShadowInstalledReceipt {
    let zero = "0".repeat(64);
    ApiShadowInstalledReceipt {
        action_sha256: zero.clone(),
        registration_sha256: zero.clone(),
        candidate_graph_sha256: zero.clone(),
        candidate_sha256,
        bundle_sha256,
        source_sha256: zero.clone(),
        deployed_settings_sha256: zero.clone(),
        fencing_token: zero,
        lifecycle: None,
    }
}

// hash exact raw candidate bytes without interpreting their canonical document again
fn candidate_sha256(bytes: &[u8]) -> String {
    format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        bytes,
    ))
}

// hash one canonical JSON value with the source framing newline
fn canonical_sha256(
    value: &JsonValue,
) -> Result<String, NativeMaintenanceDailyCandidateReplayError> {
    let wire = NativeWirePrimitives;
    let bytes = wire
        .encode_sorted_json(value)
        .ok_or(NativeMaintenanceDailyCandidateReplayError::Canonical)?;
    Ok(format_digest(RevisionArchiveCanonicalCodec::sha256(
        &wire, &bytes,
    )))
}

// admit one exact zero-padded source month
fn valid_month(value: &str) -> bool {
    value.len() == 7
        && value.as_bytes()[..4].iter().all(u8::is_ascii_digit)
        && value.as_bytes().get(4) == Some(&b'-')
        && matches!(
            value.as_bytes().get(5..),
            Some([b'0', b'1'..=b'9'] | [b'1', b'0'..=b'2'])
        )
}

// decode a strict finite nullable source number
fn nullable_number(value: &JsonValue, key: &str) -> Option<Option<f64>> {
    // preserve explicit source null separately from a malformed numeric member
    if *field(value, key)? == JsonValue::Null {
        Some(None)
    } else {
        number(value, key).map(Some)
    }
}

// decode one exact JSON boolean
fn boolean(value: &JsonValue, key: &str) -> Option<bool> {
    match field(value, key)? {
        JsonValue::Boolean(value) => Some(*value),
        _ => None,
    }
}

// decode one safe integer without accepting rounded fractions
fn integer(value: &JsonValue, key: &str) -> Option<i64> {
    let value = number(value, key)?;
    (value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0).then_some(value as i64)
}

#[cfg(test)]
#[path = "native_maintenance_daily_candidate_replay_test.rs"]
mod tests;
