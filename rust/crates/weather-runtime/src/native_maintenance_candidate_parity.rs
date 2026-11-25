//! Native wind candidate packaging and retained/synthetic runtime parity.

use std::fmt::{self, Display, Formatter};

use weather_api::{
    AdmittedWindRowFeatures, AdmittedWindRowSourceIdentity, AdmittedWindV2MetricOrder,
    WindV2RowOutcome, evaluate_admitted_wind_v2_row,
};
use weather_contracts::{
    AdjustmentShadowFamily, CanonicalWeatherMetrics, ForecastAdjustmentRawForecastProvenance,
    ForecastAdjustmentRawForecastProvenanceCohortLiteral,
    ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral, ForecastAdjustmentReasonCode,
    ForecastAdjustmentWindCanaryMetric, ForecastAdjustmentWindMaintenanceRuntimePackage,
    ForecastLeadBandKey, JsonValue, Nullable,
};
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::api_maintenance_candidate_package::admit_qualified_api_canary_package;
use crate::native_fit_ascii_collation::{native_fit_collation_admits, native_fit_collation_cmp};
use crate::native_maintenance_daily_candidate_replay::{
    admit_temperature_package, build_native_rain_maintenance_artifact,
    build_native_temperature_maintenance_package, replay_native_rain_feature_candidate_pair,
    temperature_decision,
};
use crate::native_maintenance_fit_candidate::admit_native_maintenance_fit_candidate;
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    NativeMaintenanceCandidateParity, NativeMaintenanceCandidateParityRequest,
    NativeMaintenanceCaptureFamily, NativeMaintenanceRetainedParityInput,
    NativeMaintenanceSyntheticParityInput, NativePredecessorModelCalendar, NativeWirePrimitives,
};

const FIT_VERSION: &str = "wind-maintenance-fit/v2";
const PACKAGE_VERSION: &str = "forecast-adjustment-maintenance-runtime-package/v1";
const PARITY_VERSION: &str = "forecast-adjustment-model-parity/v1";
const DEVELOPMENT_REPORT_VERSION: &str = "forecast-adjustment-development-report/v1";
const DECISION_VERSION: &str = "forecast-adjustment-decision/v1";
const MAINTENANCE_DECISION_VERSION: &str = "forecast-adjustment-maintenance-decision/v1";
const WIND_FIT_INPUT_VERSION: &str = "wind-maintenance-fit-input/v2";
const TEMPERATURE_FIT_INPUT_VERSION: &str = "temperature-maintenance-fit-input/v2";
const RAIN_FIT_INPUT_VERSION: &str = "rain-maintenance-fit-input/v3";
const RAIN_PARITY_INPUT_VERSION: &str = "rain-maintenance-feature-parity-input/v2";
const RAIN_SOURCE_IDENTITY_SHA256: &str =
    "ac9a4eb5737bedbb1ae260568c365ab7b145e0bd4a76db92bbbed9fb75ce147b";

const FIT_KEYS: &[&str] = &[
    "candidate",
    "confirmationOpened",
    "contractVersion",
    "developmentReport",
    "dueMonth",
    "state",
];
const DEVELOPMENT_REPORT_KEYS: &[&str] = &[
    "contractVersion",
    "developmentReportSha256",
    "enabledMetricBands",
    "folds",
    "generatedBeforeHoldout",
];
const PARITY_INPUT_KEYS: &[&str] = &["evaluatedAt", "metrics", "rawForecastProvenance"];
const METRIC_KEYS: &[&str] = &[
    "apparentTemperatureC",
    "blackGlobeTemperatureC",
    "cloudCoverPercent",
    "pm25MicrogramsPerCubicMeter",
    "precipitationMm",
    "precipitationRateMmPerHour",
    "pressureHpa",
    "relativeHumidityPercent",
    "soilElectricalConductivityMicrosiemensPerCm",
    "soilMoisturePercent",
    "solarRadiationWm2",
    "temperatureC",
    "uvIndex",
    "waterLevelM",
    "wetBulbGlobeTemperatureC",
    "windDirectionDegrees",
    "windGustMps",
    "windSpeedMps",
];
const PROVENANCE_KEYS: &[&str] = &[
    "adapterVersion",
    "cohort",
    "contractEpoch",
    "dataset",
    "referenceAt",
    "referenceKind",
    "sourceConfigFingerprint",
    "sourceKey",
    "targetLeadHours",
    "upstreamModel",
    "validAt",
];

/// Exact authority-free package bytes and identities retained for registration and persistence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenancePortableCandidate {
    pub artifact_bytes: Vec<u8>,
    pub artifact_sha256: String,
    pub candidate_sha256: String,
    pub source_identity_sha256: String,
}

/// Six exact byte streams used by the source parity receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCandidateParityEvidence {
    pub retained_input: Vec<u8>,
    pub retained_native_output: Vec<u8>,
    pub retained_packaged_output: Vec<u8>,
    pub synthetic_input: Vec<u8>,
    pub synthetic_native_output: Vec<u8>,
    pub synthetic_packaged_output: Vec<u8>,
}

/// Source parity receipt and its immutable content address.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceCandidateParityReceipt {
    pub bytes: Vec<u8>,
    pub fixture: JsonValue,
    pub parity_sha256: String,
}

/// Complete candidate material retained across due registration and candidate persistence.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceCandidateParityMaterial {
    pub evidence: NativeMaintenanceCandidateParityEvidence,
    pub portable: NativeMaintenancePortableCandidate,
    pub receipt: NativeMaintenanceCandidateParityReceipt,
}

/// Fit-derived retained and deterministic synthetic parity inputs for semantic inspection.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceFitParityInputs {
    pub family: NativeMaintenanceCaptureFamily,
    pub retained_input: NativeMaintenanceRetainedParityInput,
    pub retained_input_bytes: Vec<u8>,
    pub retained_input_sha256: String,
    pub synthetic_input: NativeMaintenanceSyntheticParityInput,
    pub synthetic_input_bytes: Vec<u8>,
    pub synthetic_input_sha256: String,
}

impl NativeMaintenanceCandidateParityMaterial {
    /// Project the source receipt into the existing opaque due-work parity slot.
    pub fn due_parity(&self) -> NativeMaintenanceCandidateParity {
        NativeMaintenanceCandidateParity {
            value: self.receipt.fixture.clone(),
        }
    }
}

/// Closed packaging, input, evaluation, or canonical-value failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceCandidateParityError {
    Candidate,
    Package,
    Input,
    Evaluation,
    Canonical,
}

impl Display for NativeMaintenanceCandidateParityError {
    /// Render only the value-free parity failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Candidate => "maintenance candidate is invalid",
            Self::Package => "maintenance candidate package is invalid",
            Self::Input => "maintenance candidate parity input is invalid",
            Self::Evaluation => "maintenance candidate parity outputs differ",
            Self::Canonical => "maintenance candidate parity encoding failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceCandidateParityError {}

/// Derive one source-exact retained and synthetic parity pair from monthly fit input.
pub fn build_native_maintenance_fit_parity_inputs(
    family: NativeMaintenanceCaptureFamily,
    fit_input: &JsonValue,
) -> Result<Option<NativeMaintenanceFitParityInputs>, NativeMaintenanceCandidateParityError> {
    let (retained, synthetic) = match family {
        NativeMaintenanceCaptureFamily::Temperature => {
            // require the source temperature fit input before selecting one genuine row
            if text(fit_input, "contractVersion").as_deref() != Some(TEMPERATURE_FIT_INPUT_VERSION)
            {
                return Err(NativeMaintenanceCandidateParityError::Input);
            }
            let retained = fit_temperature_parity_input(first_fit_parity_row(fit_input)?)?;
            let synthetic = synthetic_temperature_parity_input(&retained)?;
            (retained, synthetic)
        }
        NativeMaintenanceCaptureFamily::Wind => {
            // require the archive-native wind fit input before selecting one genuine source row
            if text(fit_input, "contractVersion").as_deref() != Some(WIND_FIT_INPUT_VERSION) {
                return Err(NativeMaintenanceCandidateParityError::Input);
            }
            let retained = fit_wind_parity_input(first_wind_fit_parity_row(fit_input)?)?;
            let synthetic = synthetic_wind_parity_input(&retained)?;
            (retained, synthetic)
        }
        NativeMaintenanceCaptureFamily::Rain => {
            // bind the fitted feature order before selecting pre-candidate numerical material
            if text(fit_input, "contractVersion").as_deref() != Some(RAIN_FIT_INPUT_VERSION) {
                return Err(NativeMaintenanceCandidateParityError::Input);
            }
            let names = rain_feature_names(fit_input)?;
            let retained = fit_rain_parity_input(first_fit_parity_row(fit_input)?, names)?;
            let synthetic = synthetic_rain_parity_input(&retained)?;
            (retained, synthetic)
        }
    };
    let retained_input_bytes = encode(&retained)?;
    let synthetic_input_bytes = encode(&synthetic)?;
    // prohibit a synthetic fixture that leaves the actual evaluator bytes unchanged
    if retained_input_bytes == synthetic_input_bytes {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    Ok(Some(NativeMaintenanceFitParityInputs {
        family,
        retained_input_sha256: digest(&retained_input_bytes),
        retained_input_bytes,
        retained_input: NativeMaintenanceRetainedParityInput { value: retained },
        synthetic_input_sha256: digest(&synthetic_input_bytes),
        synthetic_input_bytes,
        synthetic_input: NativeMaintenanceSyntheticParityInput { value: synthetic },
    }))
}

/// Build candidate parity only for families with both raw and packaged native evaluators.
pub fn build_native_maintenance_candidate_parity(
    request: NativeMaintenanceCandidateParityRequest<'_>,
) -> Result<Option<NativeMaintenanceCandidateParityMaterial>, NativeMaintenanceCandidateParityError>
{
    // dispatch only families with complete independent raw and packaged native evaluators
    match request.family {
        NativeMaintenanceCaptureFamily::Temperature => {
            build_temperature_candidate_parity(request).map(Some)
        }
        NativeMaintenanceCaptureFamily::Wind => build_wind_candidate_parity(request).map(Some),
        NativeMaintenanceCaptureFamily::Rain => build_rain_candidate_parity(request).map(Some),
    }
}

/// Build the complete wind candidate parity material after family dispatch.
fn build_wind_candidate_parity(
    request: NativeMaintenanceCandidateParityRequest<'_>,
) -> Result<NativeMaintenanceCandidateParityMaterial, NativeMaintenanceCandidateParityError> {
    let retained = request
        .retained_input
        .ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let synthetic = request
        .synthetic_input
        .ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let portable = build_wind_portable_candidate(request.candidate_bytes)?;
    let native_package = package_from_candidate(request.candidate_bytes)?.1;
    let packaged_package = admit_package(&portable)?;
    let retained_input = encode(&retained.value)?;
    let synthetic_input = encode(&synthetic.value)?;
    // prohibit a nominal synthetic fixture that leaves evaluator bytes unchanged
    if retained_input == synthetic_input {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    let retained_native_output = evaluate_wind(&native_package, &retained.value)?;
    let retained_packaged_output = evaluate_wind(&packaged_package, &retained.value)?;
    let synthetic_native_output = evaluate_wind(&native_package, &synthetic.value)?;
    let synthetic_packaged_output = evaluate_wind(&packaged_package, &synthetic.value)?;
    // require both independently admitted model paths to agree byte for byte
    if retained_native_output != retained_packaged_output
        || synthetic_native_output != synthetic_packaged_output
    {
        return Err(NativeMaintenanceCandidateParityError::Evaluation);
    }
    let evidence = NativeMaintenanceCandidateParityEvidence {
        retained_input,
        retained_native_output,
        retained_packaged_output,
        synthetic_input,
        synthetic_native_output,
        synthetic_packaged_output,
    };
    let receipt = parity_receipt(
        NativeMaintenanceCaptureFamily::Wind,
        &portable.candidate_sha256,
        &evidence,
    )?;
    Ok(NativeMaintenanceCandidateParityMaterial {
        evidence,
        portable,
        receipt,
    })
}

/// Build temperature parity through the shared daily raw-fit and package evaluators.
fn build_temperature_candidate_parity(
    request: NativeMaintenanceCandidateParityRequest<'_>,
) -> Result<NativeMaintenanceCandidateParityMaterial, NativeMaintenanceCandidateParityError> {
    let retained = request
        .retained_input
        .ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let synthetic = request
        .synthetic_input
        .ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let (artifact_bytes, native_model, native_bundle_sha256) =
        build_native_temperature_maintenance_package(request.candidate_bytes)
            .map_err(|_| NativeMaintenanceCandidateParityError::Candidate)?;
    let candidate_sha256 = digest(request.candidate_bytes);
    let artifact = NativeWirePrimitives
        .decode_json(&artifact_bytes)
        .ok_or(NativeMaintenanceCandidateParityError::Package)?;
    let source =
        field(&artifact, "source").ok_or(NativeMaintenanceCandidateParityError::Package)?;
    let portable = NativeMaintenancePortableCandidate {
        artifact_sha256: native_bundle_sha256.clone(),
        candidate_sha256: candidate_sha256.clone(),
        source_identity_sha256: digest(&encode(source)?),
        artifact_bytes,
    };
    let (packaged_model, packaged_bundle_sha256) =
        admit_temperature_package(&portable.artifact_bytes, Some(candidate_sha256.clone()))
            .map_err(|_| NativeMaintenanceCandidateParityError::Package)?;
    // require the package admission to retain the raw-fit-derived bundle address
    if packaged_bundle_sha256 != native_bundle_sha256 {
        return Err(NativeMaintenanceCandidateParityError::Package);
    }
    let retained_input = encode(&retained.value)?;
    let synthetic_input = encode(&synthetic.value)?;
    // prohibit a nominal synthetic fixture that leaves evaluator bytes unchanged
    if retained_input == synthetic_input {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    let retained_native_output =
        temperature_decision(&native_bundle_sha256, &native_model, &retained.value)
            .map_err(|_| NativeMaintenanceCandidateParityError::Evaluation)
            .and_then(|value| encode(&value))?;
    let retained_packaged_output =
        temperature_decision(&packaged_bundle_sha256, &packaged_model, &retained.value)
            .map_err(|_| NativeMaintenanceCandidateParityError::Evaluation)
            .and_then(|value| encode(&value))?;
    let synthetic_native_output =
        temperature_decision(&native_bundle_sha256, &native_model, &synthetic.value)
            .map_err(|_| NativeMaintenanceCandidateParityError::Evaluation)
            .and_then(|value| encode(&value))?;
    let synthetic_packaged_output =
        temperature_decision(&packaged_bundle_sha256, &packaged_model, &synthetic.value)
            .map_err(|_| NativeMaintenanceCandidateParityError::Evaluation)
            .and_then(|value| encode(&value))?;
    // independently require both temperature model forms to produce identical decisions
    if retained_native_output != retained_packaged_output
        || synthetic_native_output != synthetic_packaged_output
    {
        return Err(NativeMaintenanceCandidateParityError::Evaluation);
    }
    let evidence = NativeMaintenanceCandidateParityEvidence {
        retained_input,
        retained_native_output,
        retained_packaged_output,
        synthetic_input,
        synthetic_native_output,
        synthetic_packaged_output,
    };
    let receipt = parity_receipt(
        NativeMaintenanceCaptureFamily::Temperature,
        &candidate_sha256,
        &evidence,
    )?;
    Ok(NativeMaintenanceCandidateParityMaterial {
        evidence,
        portable,
        receipt,
    })
}

/// Build rain feature parity through the shared raw-fit and portable artifact evaluator.
fn build_rain_candidate_parity(
    request: NativeMaintenanceCandidateParityRequest<'_>,
) -> Result<NativeMaintenanceCandidateParityMaterial, NativeMaintenanceCandidateParityError> {
    let retained = request
        .retained_input
        .ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let synthetic = request
        .synthetic_input
        .ok_or(NativeMaintenanceCandidateParityError::Input)?;
    validate_rain_parity_input(&retained.value)?;
    validate_rain_parity_input(&synthetic.value)?;
    let native = build_native_rain_maintenance_artifact(request.candidate_bytes)
        .map_err(|_| NativeMaintenanceCandidateParityError::Candidate)?;
    let retained_pair = replay_native_rain_feature_candidate_pair(
        &retained.value,
        request.candidate_bytes,
        &native.artifact_bytes,
    )
    .map_err(|_| NativeMaintenanceCandidateParityError::Evaluation)?;
    let synthetic_pair = replay_native_rain_feature_candidate_pair(
        &synthetic.value,
        request.candidate_bytes,
        &native.artifact_bytes,
    )
    .map_err(|_| NativeMaintenanceCandidateParityError::Evaluation)?;
    // require each independently decoded raw and portable rain model result to agree
    if retained_pair.candidate_decision_bytes != retained_pair.packaged_decision_bytes
        || synthetic_pair.candidate_decision_bytes != synthetic_pair.packaged_decision_bytes
        || retained_pair.portable_candidate_bytes != native.artifact_bytes
        || synthetic_pair.portable_candidate_bytes != native.artifact_bytes
    {
        return Err(NativeMaintenanceCandidateParityError::Evaluation);
    }
    let evidence = NativeMaintenanceCandidateParityEvidence {
        retained_input: encode(&retained.value)?,
        retained_native_output: retained_pair.candidate_decision_bytes,
        retained_packaged_output: retained_pair.packaged_decision_bytes,
        synthetic_input: encode(&synthetic.value)?,
        synthetic_native_output: synthetic_pair.candidate_decision_bytes,
        synthetic_packaged_output: synthetic_pair.packaged_decision_bytes,
    };
    let portable = NativeMaintenancePortableCandidate {
        artifact_bytes: native.artifact_bytes,
        artifact_sha256: native.artifact_sha256,
        candidate_sha256: native.candidate_sha256,
        source_identity_sha256: RAIN_SOURCE_IDENTITY_SHA256.to_owned(),
    };
    let receipt = parity_receipt(
        NativeMaintenanceCaptureFamily::Rain,
        &portable.candidate_sha256,
        &evidence,
    )?;
    Ok(NativeMaintenanceCandidateParityMaterial {
        evidence,
        portable,
        receipt,
    })
}

/// Convert one canonical selected wind fit into its exact self-addressed public package.
pub(crate) fn build_wind_portable_candidate(
    candidate_bytes: &[u8],
) -> Result<NativeMaintenancePortableCandidate, NativeMaintenanceCandidateParityError> {
    let (artifact, package) = package_from_candidate(candidate_bytes)?;
    let ForecastAdjustmentWindMaintenanceRuntimePackage {
        bundleSha256,
        candidateSha256,
        ..
    } = package;
    let source =
        field(&artifact, "source").ok_or(NativeMaintenanceCandidateParityError::Package)?;
    let source_identity_sha256 = digest(&encode(source)?);
    Ok(NativeMaintenancePortableCandidate {
        artifact_bytes: encode(&artifact)?,
        artifact_sha256: bundleSha256,
        candidate_sha256: candidateSha256,
        source_identity_sha256,
    })
}

/// Project and re-admit one selected fit through the existing package validator.
pub(crate) fn package_from_candidate(
    candidate_bytes: &[u8],
) -> Result<
    (JsonValue, ForecastAdjustmentWindMaintenanceRuntimePackage),
    NativeMaintenanceCandidateParityError,
> {
    let admitted = admit_native_maintenance_fit_candidate(candidate_bytes)
        .ok_or(NativeMaintenanceCandidateParityError::Candidate)?;
    let fit = NativeWirePrimitives
        .decode_json(candidate_bytes)
        .ok_or(NativeMaintenanceCandidateParityError::Candidate)?;
    // bind the closed selected-wind fit envelope before projecting its portable subset
    if !exact_keys(&fit, FIT_KEYS)
        || text(&fit, "contractVersion").as_deref() != Some(FIT_VERSION)
        || text(&fit, "state").as_deref() != Some("development_candidate")
        || boolean(&fit, "confirmationOpened") != Some(false)
    {
        return Err(NativeMaintenanceCandidateParityError::Candidate);
    }
    let due_month = text(&fit, "dueMonth")
        .filter(|value| valid_month(value))
        .ok_or(NativeMaintenanceCandidateParityError::Candidate)?;
    let candidate = field(&fit, "candidate")
        .cloned()
        .ok_or(NativeMaintenanceCandidateParityError::Candidate)?;
    let report =
        field(&fit, "developmentReport").ok_or(NativeMaintenanceCandidateParityError::Candidate)?;
    validate_report_binding(&candidate, report)?;
    let source = field(&candidate, "forecastIdentity")
        .cloned()
        .ok_or(NativeMaintenanceCandidateParityError::Candidate)?;
    let mut fields = vec![
        pair("candidate", candidate),
        pair("candidateSha256", string(&admitted.candidate_sha256)),
        pair("contractVersion", string(PACKAGE_VERSION)),
        pair("dueMonth", string(&due_month)),
        pair("family", string("wind")),
        pair("source", source),
    ];
    let unsigned = JsonValue::Object(fields.clone());
    let bundle_sha256 = digest(&encode(&unsigned)?);
    fields.push(pair("bundleSha256", string(&bundle_sha256)));
    let document = JsonValue::Object(fields);
    let bytes = encode(&document)?;
    let mut codec = NativeWirePrimitives;
    let (_, package) = admit_qualified_api_canary_package(
        AdjustmentShadowFamily::Wind,
        &bundle_sha256,
        &admitted.candidate_sha256,
        &bytes,
        &mut codec,
    )
    .map_err(|_| NativeMaintenanceCandidateParityError::Package)?;
    let weather_contracts::ForecastAdjustmentMaintenanceRuntimePackage::Wind(package) = package
    else {
        return Err(NativeMaintenanceCandidateParityError::Package);
    };
    Ok((document, package))
}

/// Re-admit public package bytes independently from their raw fit source.
pub(crate) fn admit_package(
    portable: &NativeMaintenancePortableCandidate,
) -> Result<ForecastAdjustmentWindMaintenanceRuntimePackage, NativeMaintenanceCandidateParityError>
{
    let mut codec = NativeWirePrimitives;
    let (_, package) = admit_qualified_api_canary_package(
        AdjustmentShadowFamily::Wind,
        &portable.artifact_sha256,
        &portable.candidate_sha256,
        &portable.artifact_bytes,
        &mut codec,
    )
    .map_err(|_| NativeMaintenanceCandidateParityError::Package)?;
    let weather_contracts::ForecastAdjustmentMaintenanceRuntimePackage::Wind(package) = package
    else {
        return Err(NativeMaintenanceCandidateParityError::Package);
    };
    Ok(package)
}

/// Bind the development report address retained by the selected fitted candidate.
fn validate_report_binding(
    candidate: &JsonValue,
    report: &JsonValue,
) -> Result<(), NativeMaintenanceCandidateParityError> {
    // require the immutable report envelope and its generated-before-holdout marker
    if !exact_keys(report, DEVELOPMENT_REPORT_KEYS)
        || text(report, "contractVersion").as_deref() != Some(DEVELOPMENT_REPORT_VERSION)
        || boolean(report, "generatedBeforeHoldout") != Some(true)
    {
        return Err(NativeMaintenanceCandidateParityError::Candidate);
    }
    let report_sha256 = text(report, "developmentReportSha256")
        .filter(|value| valid_digest(value))
        .ok_or(NativeMaintenanceCandidateParityError::Candidate)?;
    let expected = hash_without_field(report, "developmentReportSha256")?;
    // cross-bind the report self-address and the fitted candidate reference
    if report_sha256 != expected
        || text(candidate, "developmentReportSha256").as_deref() != Some(report_sha256.as_str())
    {
        return Err(NativeMaintenanceCandidateParityError::Candidate);
    }
    Ok(())
}

/// Select the first authentic monthly row by immutable valid clock and row key.
fn first_fit_parity_row(
    fit_input: &JsonValue,
) -> Result<&JsonValue, NativeMaintenanceCandidateParityError> {
    let rows = ["trainingRows", "developmentRows"]
        .into_iter()
        .map(|name| {
            let JsonValue::Array(rows) =
                field(fit_input, name).ok_or(NativeMaintenanceCandidateParityError::Input)?
            else {
                return Err(NativeMaintenanceCandidateParityError::Input);
            };
            Ok(rows.as_slice())
        })
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .flatten()
        .filter_map(|row| {
            let valid_at = text(row, "validAt")?;
            let key = text(row, "key")?;
            (native_fit_collation_admits(&valid_at) && native_fit_collation_admits(&key))
                .then_some(((valid_at, key), row))
        })
        .min_by(|((left_at, left_key), _), ((right_at, right_key), _)| {
            native_fit_collation_cmp(left_at, right_at)
                .then_with(|| native_fit_collation_cmp(left_key, right_key))
        })
        .map(|(_, row)| row)
        .ok_or(NativeMaintenanceCandidateParityError::Input)
}

/// Select the first archive-native wind row by immutable clock and content address.
fn first_wind_fit_parity_row(
    fit_input: &JsonValue,
) -> Result<&JsonValue, NativeMaintenanceCandidateParityError> {
    let JsonValue::Array(rows) =
        field(fit_input, "rows").ok_or(NativeMaintenanceCandidateParityError::Input)?
    else {
        return Err(NativeMaintenanceCandidateParityError::Input);
    };
    rows.iter()
        .filter(|row| text(row, "recordKind").as_deref() == Some("legacy_v4_retrieval_snapshot"))
        .filter_map(|row| {
            let valid_at = text(row, "validAt")?;
            let JsonValue::Array(hashes) = field(row, "contentHashes")? else {
                return None;
            };
            let JsonValue::String(first_hash) = hashes.first()? else {
                return None;
            };
            let first_hash = String::from_utf16(first_hash).ok()?;
            (native_fit_collation_admits(&valid_at)
                && native_fit_collation_admits(&first_hash)
                && valid_digest(&first_hash))
            .then_some(((valid_at, first_hash), row))
        })
        .min_by(|((left_at, left_hash), _), ((right_at, right_hash), _)| {
            native_fit_collation_cmp(left_at, right_at)
                .then_with(|| native_fit_collation_cmp(left_hash, right_hash))
        })
        .map(|(_, row)| row)
        .ok_or(NativeMaintenanceCandidateParityError::Input)
}

/// Project one enriched temperature fit row into the production evaluator input.
fn fit_temperature_parity_input(
    row: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceCandidateParityError> {
    let raw_temperature = finite(row, "rawTemperatureC")?;
    let raw_best = nullable_json_number(row, "rawBestMatchTemperatureC")?;
    let raw_humidity = nullable_json_number(row, "rawRelativeHumidityPercent")?;
    let raw_wind = nullable_json_number(row, "rawWindSpeedMps")?;
    let source = JsonValue::Object(vec![
        pair(
            "adapterVersion",
            string(&required_text(row, "adapterVersion")?),
        ),
        pair("dataset", string("single_run")),
        pair(
            "firstReceivedAt",
            string(&required_text(row, "firstReceivedAt")?),
        ),
        pair(
            "modelCycle",
            field(row, "modelCycle")
                .cloned()
                .ok_or(NativeMaintenanceCandidateParityError::Input)?,
        ),
        pair(
            "modelLeadHours",
            JsonValue::Number(unsigned(row, "modelLeadHours")? as f64),
        ),
        pair("providerKey", string("open-meteo")),
        pair(
            "providerResponseSha256",
            string(&required_text(row, "providerResponseSha256")?),
        ),
        pair("rawRelativeHumidityPercent", raw_humidity),
        pair("rawTemperatureC", JsonValue::Number(raw_temperature)),
        pair("rawWindSpeedMps", raw_wind),
        pair(
            "runInitializedAt",
            string(&required_text(row, "runInitializedAt")?),
        ),
        pair("upstreamModel", string("ecmwf_ifs")),
        pair("validAt", string(&required_text(row, "validAt")?)),
    ]);
    let value = JsonValue::Object(vec![
        pair(
            "evaluatedAt",
            string(&required_text(row, "sourceReceiptAt")?),
        ),
        pair("rawBestMatchTemperatureC", raw_best),
        pair(
            "recentErrorState",
            field(row, "state")
                .cloned()
                .ok_or(NativeMaintenanceCandidateParityError::Input)?,
        ),
        pair("sourceForecast", source),
        pair("validAt", string(&required_text(row, "validAt")?)),
    ]);
    canonical_milliseconds(
        &text(&value, "evaluatedAt").ok_or(NativeMaintenanceCandidateParityError::Input)?,
    )?;
    Ok(value)
}

/// Perturb only the two bounded temperature scalars used by source parity.
fn synthetic_temperature_parity_input(
    retained: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceCandidateParityError> {
    let mut synthetic = retained.clone();
    let source = field_mut(&mut synthetic, "sourceForecast")
        .ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let raw = finite(source, "rawTemperatureC")?;
    *field_mut(source, "rawTemperatureC").ok_or(NativeMaintenanceCandidateParityError::Input)? =
        JsonValue::Number(raw + 0.125);
    let best = nullable_finite(&synthetic, "rawBestMatchTemperatureC")?;
    // retain the source's explicit missing best-match state
    if let Nullable::Value(best) = best {
        *field_mut(&mut synthetic, "rawBestMatchTemperatureC")
            .ok_or(NativeMaintenanceCandidateParityError::Input)? = JsonValue::Number(best + 0.125);
    }
    Ok(synthetic)
}

/// Admit the frozen 107-name rain feature order from the monthly fit input.
fn rain_feature_names(
    fit_input: &JsonValue,
) -> Result<&JsonValue, NativeMaintenanceCandidateParityError> {
    let names =
        field(fit_input, "featureNames").ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let JsonValue::Array(values) = names else {
        return Err(NativeMaintenanceCandidateParityError::Input);
    };
    // retain only the complete string-valued fitted feature order
    if values.len() != 107
        || values
            .iter()
            .any(|value| !matches!(value, JsonValue::String(_)))
    {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    Ok(names)
}

/// Project one authentic pre-candidate rain feature row into the v2 parity contract.
fn fit_rain_parity_input(
    row: &JsonValue,
    feature_names: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceCandidateParityError> {
    let features = field(row, "features")
        .cloned()
        .ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let value = JsonValue::Object(vec![
        pair("contractVersion", string(RAIN_PARITY_INPUT_VERSION)),
        pair("featureNames", feature_names.clone()),
        pair("features", features),
        pair("validAt", string(&required_text(row, "validAt")?)),
    ]);
    validate_rain_parity_input(&value)?;
    Ok(value)
}

/// Perturb only the raw precipitation feature while preserving the fitted feature grammar.
fn synthetic_rain_parity_input(
    retained: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceCandidateParityError> {
    let mut synthetic = retained.clone();
    let JsonValue::Array(features) = field_mut(&mut synthetic, "features")
        .ok_or(NativeMaintenanceCandidateParityError::Input)?
    else {
        return Err(NativeMaintenanceCandidateParityError::Input);
    };
    let JsonValue::Number(raw) = features
        .get(5)
        .ok_or(NativeMaintenanceCandidateParityError::Input)?
    else {
        return Err(NativeMaintenanceCandidateParityError::Input);
    };
    let changed = (*raw + 0.125).max(0.0);
    features[5] = JsonValue::Number(changed);
    validate_rain_parity_input(&synthetic)?;
    Ok(synthetic)
}

/// Validate the exact fitted rain feature order and finite-or-missing numerical vector.
fn validate_rain_parity_input(
    value: &JsonValue,
) -> Result<(), NativeMaintenanceCandidateParityError> {
    let JsonValue::Array(names) =
        field(value, "featureNames").ok_or(NativeMaintenanceCandidateParityError::Input)?
    else {
        return Err(NativeMaintenanceCandidateParityError::Input);
    };
    let JsonValue::Array(features) =
        field(value, "features").ok_or(NativeMaintenanceCandidateParityError::Input)?
    else {
        return Err(NativeMaintenanceCandidateParityError::Input);
    };
    // require exact v2 shape, complete vectors, and only finite-or-missing cells
    if !exact_keys(
        value,
        &["contractVersion", "featureNames", "features", "validAt"],
    ) || text(value, "contractVersion").as_deref() != Some(RAIN_PARITY_INPUT_VERSION)
        || names.len() != 107
        || features.len() != 107
        || features.iter().any(|feature| {
            !matches!(feature, JsonValue::Null)
                && !matches!(feature, JsonValue::Number(number) if number.is_finite())
        })
    {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    let JsonValue::Number(raw) = &features[5] else {
        return Err(NativeMaintenanceCandidateParityError::Input);
    };
    // preserve the raw precipitation feature's nonnegative physical domain
    if *raw < 0.0 {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    canonical_milliseconds(
        &text(value, "validAt").ok_or(NativeMaintenanceCandidateParityError::Input)?,
    )?;
    Ok(())
}

/// Project one authenticated archive wind row into the direct model evaluator input.
fn fit_wind_parity_input(
    row: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceCandidateParityError> {
    let adapter_version = singleton_text(row, "adapterContracts")?;
    let source_config_fingerprint = singleton_text(row, "sourceConfigFingerprints")?;
    let source_key = singleton_text(row, "sourceKeys")?;
    let metrics = field(row, "metrics").ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let speed = finite(metrics, "windSpeedMps")?;
    let gust = nullable_finite(metrics, "windGustMps")?;
    let retained_metrics = parity_wind_metrics(speed, gust.clone());
    let provenance = JsonValue::Object(vec![
        pair("adapterVersion", string(&adapter_version)),
        pair("cohort", string("legacy_v4_retrieval_snapshot")),
        pair(
            "contractEpoch",
            string(&required_text(row, "contractEpoch")?),
        ),
        pair("dataset", string(&required_text(row, "dataset")?)),
        pair("referenceAt", string(&required_text(row, "referenceAt")?)),
        pair("referenceKind", string("retrieval_snapshot")),
        pair(
            "sourceConfigFingerprint",
            string(&source_config_fingerprint),
        ),
        pair("sourceKey", string(&source_key)),
        pair(
            "targetLeadHours",
            JsonValue::Number(unsigned(row, "targetLeadHours")? as f64),
        ),
        pair(
            "upstreamModel",
            string(&required_text(row, "upstreamModel")?),
        ),
        pair("validAt", string(&required_text(row, "validAt")?)),
    ]);
    let value = JsonValue::Object(vec![
        pair("evaluatedAt", string(&required_text(row, "receivedAt")?)),
        pair("metrics", retained_metrics),
        pair("rawForecastProvenance", provenance),
    ]);
    // re-use the direct evaluator's complete causal and shape admission
    wind_metrics(field(&value, "metrics").ok_or(NativeMaintenanceCandidateParityError::Input)?)?;
    wind_provenance(
        field(&value, "rawForecastProvenance")
            .ok_or(NativeMaintenanceCandidateParityError::Input)?,
    )?;
    canonical_milliseconds(
        &text(&value, "evaluatedAt").ok_or(NativeMaintenanceCandidateParityError::Input)?,
    )?;
    Ok(value)
}

/// Perturb only bounded wind scalars while retaining the exact evaluator grammar.
fn synthetic_wind_parity_input(
    retained: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceCandidateParityError> {
    let mut synthetic = retained.clone();
    let metrics =
        field_mut(&mut synthetic, "metrics").ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let speed = finite(metrics, "windSpeedMps")?;
    let synthetic_speed = (speed + 0.125).max(0.0);
    *field_mut(metrics, "windSpeedMps").ok_or(NativeMaintenanceCandidateParityError::Input)? =
        JsonValue::Number(synthetic_speed);
    let gust = nullable_finite(metrics, "windGustMps")?;
    // preserve an explicitly missing gust and otherwise keep it at or above wind speed
    if let Nullable::Value(gust) = gust {
        *field_mut(metrics, "windGustMps").ok_or(NativeMaintenanceCandidateParityError::Input)? =
            JsonValue::Number((gust + 0.125).max(synthetic_speed));
    }
    Ok(synthetic)
}

/// Construct the complete canonical metric object without inventing unused values.
fn parity_wind_metrics(speed: f64, gust: Nullable<f64>) -> JsonValue {
    JsonValue::Object(
        METRIC_KEYS
            .iter()
            .map(|name| {
                let value = match *name {
                    "windGustMps" => match &gust {
                        Nullable::Null => JsonValue::Null,
                        Nullable::Value(value) => JsonValue::Number(*value),
                    },
                    "windSpeedMps" => JsonValue::Number(speed),
                    _ => JsonValue::Null,
                };
                pair(name, value)
            })
            .collect(),
    )
}

/// Evaluate one direct wind parity input through the existing numerical candidate owner.
pub(crate) fn evaluate_wind(
    package: &ForecastAdjustmentWindMaintenanceRuntimePackage,
    input: &JsonValue,
) -> Result<Vec<u8>, NativeMaintenanceCandidateParityError> {
    // close the fit-derived direct evaluator grammar before reading scalar fields
    if !exact_keys(input, PARITY_INPUT_KEYS) {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    let evaluated_at =
        text(input, "evaluatedAt").ok_or(NativeMaintenanceCandidateParityError::Input)?;
    canonical_milliseconds(&evaluated_at)?;
    let metrics_value =
        field(input, "metrics").ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let provenance_value = field(input, "rawForecastProvenance")
        .ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let metrics = wind_metrics(metrics_value)?;
    let provenance = wind_provenance(provenance_value)?;
    let calendar = NativePredecessorModelCalendar::new()
        .ok_or(NativeMaintenanceCandidateParityError::Evaluation)?;
    let features = calendar
        .features(&provenance.validAt)
        .map_err(|_| NativeMaintenanceCandidateParityError::Input)?;
    let expected = &package.candidate.forecastIdentity;
    let identity = if expected.adapterVersion == provenance.adapterVersion
        && expected.contractEpoch == provenance.contractEpoch
        && expected.dataset == provenance.dataset
        && expected.sourceConfigFingerprint == provenance.sourceConfigFingerprint
        && expected.sourceKey == provenance.sourceKey
        && expected.upstreamModel == provenance.upstreamModel
    {
        AdmittedWindRowSourceIdentity::Match
    } else {
        AdmittedWindRowSourceIdentity::IdentityMismatch
    };
    let outcome = evaluate_admitted_wind_v2_row(
        &package.candidate,
        AdmittedWindRowFeatures {
            calendar: weather_api::AdmittedWindRowCalendar {
                daypart: features.daypart,
                month: u64::from(features.month),
                season: features.season,
            },
            calendar_fingerprint_matches: calendar.fingerprint_matches(
                &package.candidate.runtimeFingerprint.icuVersion,
                &package.candidate.runtimeFingerprint.tzdataVersion,
            ),
            metrics: &metrics,
            raw_forecast_provenance: &provenance,
            source_identity: identity,
        },
        AdmittedWindV2MetricOrder::Maintenance,
    );
    encode(&wind_decision(package, provenance_value, outcome)?)
}

/// Decode the exact complete metric object used by fit-derived wind parity.
fn wind_metrics(
    value: &JsonValue,
) -> Result<CanonicalWeatherMetrics, NativeMaintenanceCandidateParityError> {
    // retain only the two fitted wind scalars and require every unused metric to remain null
    if !exact_keys(value, METRIC_KEYS)
        || METRIC_KEYS
            .iter()
            .filter(|name| !matches!(**name, "windGustMps" | "windSpeedMps"))
            .any(|name| !matches!(field(value, name), Some(JsonValue::Null)))
    {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    let gust = nullable_finite(value, "windGustMps")?;
    let speed = Nullable::Value(finite(value, "windSpeedMps")?);
    Ok(CanonicalWeatherMetrics {
        apparentTemperatureC: Nullable::Null,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Null,
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Null,
        relativeHumidityPercent: Nullable::Null,
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Null,
        uvIndex: Nullable::Null,
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: gust,
        windSpeedMps: speed,
        wetBulbGlobeTemperatureC: Nullable::Null,
    })
}

/// Decode and causally bind one exact legacy-v4 retrieval provenance object.
fn wind_provenance(
    value: &JsonValue,
) -> Result<ForecastAdjustmentRawForecastProvenance, NativeMaintenanceCandidateParityError> {
    // require the fixed provenance literals supplied by the monthly fit projection
    if !exact_keys(value, PROVENANCE_KEYS)
        || text(value, "cohort").as_deref() != Some("legacy_v4_retrieval_snapshot")
        || text(value, "referenceKind").as_deref() != Some("retrieval_snapshot")
    {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    let reference_at =
        text(value, "referenceAt").ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let valid_at = text(value, "validAt").ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let reference_ms = canonical_milliseconds(&reference_at)?;
    let valid_ms = canonical_milliseconds(&valid_at)?;
    let target_lead_hours = unsigned(value, "targetLeadHours")?;
    let delta = valid_ms
        .checked_sub(reference_ms)
        .filter(|value| *value >= 0)
        .ok_or(NativeMaintenanceCandidateParityError::Input)?;
    let derived_lead = delta
        .checked_add(3_600_000 - 1)
        .and_then(|value| u64::try_from(value / 3_600_000).ok())
        .ok_or(NativeMaintenanceCandidateParityError::Input)?;
    // preserve the source's ceiling-based retrieval lead binding
    if derived_lead != target_lead_hours {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    Ok(ForecastAdjustmentRawForecastProvenance {
        adapterVersion: required_text(value, "adapterVersion")?,
        cohort: ForecastAdjustmentRawForecastProvenanceCohortLiteral,
        contractEpoch: required_text(value, "contractEpoch")?,
        dataset: required_text(value, "dataset")?,
        referenceAt: reference_at,
        referenceKind: ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral,
        sourceConfigFingerprint: required_text(value, "sourceConfigFingerprint")?,
        sourceKey: required_text(value, "sourceKey")?,
        targetLeadHours: target_lead_hours,
        upstreamModel: required_text(value, "upstreamModel")?,
        validAt: valid_at,
    })
}

/// Project the native numerical outcome into the source maintenance decision grammar.
fn wind_decision(
    package: &ForecastAdjustmentWindMaintenanceRuntimePackage,
    provenance: &JsonValue,
    outcome: WindV2RowOutcome,
) -> Result<JsonValue, NativeMaintenanceCandidateParityError> {
    let mut adjusted = Vec::new();
    let mut applied = Vec::new();
    // preserve the source maintenance metric order before canonical object serialization
    for (name, metric, value) in [
        (
            "windDirectionDegrees",
            ForecastAdjustmentWindCanaryMetric::WindDirectionDegrees,
            outcome.direction_adjusted,
        ),
        (
            "windGustMps",
            ForecastAdjustmentWindCanaryMetric::WindGustMps,
            outcome.gust_adjusted,
        ),
        (
            "windSpeedMps",
            ForecastAdjustmentWindCanaryMetric::WindSpeedMps,
            outcome.speed_adjusted,
        ),
    ] {
        // emit only successfully adjusted metrics exactly as the predecessor does
        if let Some(value) = value {
            adjusted.push(pair(name, JsonValue::Number(value)));
            applied.push(string(enum_wire(
                metric,
                ForecastAdjustmentWindCanaryMetric::WIRE_VALUES,
            )?));
        }
    }
    // retain the closed unchanged-raw result whenever no metric applied
    if applied.is_empty() {
        let reason = outcome
            .reason
            .ok_or(NativeMaintenanceCandidateParityError::Evaluation)?;
        return Ok(JsonValue::Object(vec![
            pair("adjustedMetrics", JsonValue::Object(Vec::new())),
            pair("appliedMetrics", JsonValue::Array(Vec::new())),
            pair("contractVersion", string(DECISION_VERSION)),
            pair(
                "reasonCode",
                string(enum_wire(
                    reason,
                    ForecastAdjustmentReasonCode::WIRE_VALUES,
                )?),
            ),
            pair("state", string("not_applicable")),
        ]));
    }
    let lead_band = outcome
        .lead_band
        .ok_or(NativeMaintenanceCandidateParityError::Evaluation)?;
    Ok(JsonValue::Object(vec![
        pair("adjustedMetrics", JsonValue::Object(adjusted)),
        pair("appliedMetrics", JsonValue::Array(applied)),
        pair("bundleSha256", string(&package.bundleSha256)),
        pair(
            "candidateArtifactSha256",
            string(&package.candidate.candidateArtifactSha256),
        ),
        pair("contractVersion", string(MAINTENANCE_DECISION_VERSION)),
        pair(
            "leadBand",
            string(enum_wire(lead_band, ForecastLeadBandKey::WIRE_VALUES)?),
        ),
        pair("rawForecastProvenance", provenance.clone()),
        pair("reasonCode", JsonValue::Null),
        pair("state", string("active")),
    ]))
}

/// Reduce exact parity streams to the immutable public source receipt.
fn parity_receipt(
    family: NativeMaintenanceCaptureFamily,
    candidate_sha256: &str,
    evidence: &NativeMaintenanceCandidateParityEvidence,
) -> Result<NativeMaintenanceCandidateParityReceipt, NativeMaintenanceCandidateParityError> {
    let fixture = JsonValue::Object(vec![
        pair("candidateSha256", string(candidate_sha256)),
        pair("contractVersion", string(PARITY_VERSION)),
        pair("family", string(family.wire())),
        pair(
            "retainedInputSha256",
            string(&digest(&evidence.retained_input)),
        ),
        pair(
            "retainedNativeOutputSha256",
            string(&digest(&evidence.retained_native_output)),
        ),
        pair(
            "retainedPackagedOutputSha256",
            string(&digest(&evidence.retained_packaged_output)),
        ),
        pair(
            "syntheticInputSha256",
            string(&digest(&evidence.synthetic_input)),
        ),
        pair(
            "syntheticNativeOutputSha256",
            string(&digest(&evidence.synthetic_native_output)),
        ),
        pair(
            "syntheticPackagedOutputSha256",
            string(&digest(&evidence.synthetic_packaged_output)),
        ),
    ]);
    let bytes = encode(&fixture)?;
    Ok(NativeMaintenanceCandidateParityReceipt {
        parity_sha256: digest(&bytes),
        bytes,
        fixture,
    })
}

/// Hash one object after removing only its own self-address field.
fn hash_without_field(
    value: &JsonValue,
    omitted: &str,
) -> Result<String, NativeMaintenanceCandidateParityError> {
    let JsonValue::Object(fields) = value else {
        return Err(NativeMaintenanceCandidateParityError::Candidate);
    };
    let material = JsonValue::Object(
        fields
            .iter()
            .filter(|(name, _)| !name.iter().copied().eq(omitted.encode_utf16()))
            .cloned()
            .collect(),
    );
    Ok(digest(&encode(&material)?))
}

/// Encode one recursively sorted canonical JSON value with its final LF.
fn encode(value: &JsonValue) -> Result<Vec<u8>, NativeMaintenanceCandidateParityError> {
    NativeWirePrimitives
        .encode_sorted_json(value)
        .ok_or(NativeMaintenanceCandidateParityError::Canonical)
}

/// Hash exact bytes through the approved native SHA-256 owner.
fn digest(bytes: &[u8]) -> String {
    format_digest(NativeWirePrimitives.sha256(bytes))
}

/// Read one exact object member without accepting duplicate aliases.
fn field<'a>(value: &'a JsonValue, name: &str) -> Option<&'a JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let mut found = fields
        .iter()
        .filter(|(key, _)| key.iter().copied().eq(name.encode_utf16()));
    let value = found.next().map(|(_, value)| value)?;
    found.next().is_none().then_some(value)
}

/// Mutably borrow one unique exact object member.
fn field_mut<'a>(value: &'a mut JsonValue, name: &str) -> Option<&'a mut JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let index = fields
        .iter()
        .enumerate()
        .filter(|(_, (key, _))| key.iter().copied().eq(name.encode_utf16()))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    // prohibit duplicate mutable aliases before exposing the selected member
    if index.len() != 1 {
        return None;
    }
    fields.get_mut(index[0]).map(|(_, value)| value)
}

/// Read one UTF-16 string field.
fn text(value: &JsonValue, name: &str) -> Option<String> {
    let JsonValue::String(value) = field(value, name)? else {
        return None;
    };
    String::from_utf16(value).ok()
}

/// Read one required nonempty string field.
fn required_text(
    value: &JsonValue,
    name: &str,
) -> Result<String, NativeMaintenanceCandidateParityError> {
    text(value, name)
        .filter(|value| !value.is_empty())
        .ok_or(NativeMaintenanceCandidateParityError::Input)
}

/// Read one boolean field.
fn boolean(value: &JsonValue, name: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(value, name)? else {
        return None;
    };
    Some(*value)
}

/// Read one exact safe unsigned integer field.
fn unsigned(value: &JsonValue, name: &str) -> Result<u64, NativeMaintenanceCandidateParityError> {
    let JsonValue::Number(number) =
        field(value, name).ok_or(NativeMaintenanceCandidateParityError::Input)?
    else {
        return Err(NativeMaintenanceCandidateParityError::Input);
    };
    // reject fractional, negative, nonfinite, and unsafe JavaScript integers
    if !number.is_finite()
        || number.fract() != 0.0
        || !(0.0..=9_007_199_254_740_991.0).contains(number)
    {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    Ok(*number as u64)
}

/// Read one nullable finite metric.
fn nullable_finite(
    value: &JsonValue,
    name: &str,
) -> Result<Nullable<f64>, NativeMaintenanceCandidateParityError> {
    match field(value, name) {
        Some(JsonValue::Null) => Ok(Nullable::Null),
        Some(JsonValue::Number(value)) if value.is_finite() => Ok(Nullable::Value(*value)),
        _ => Err(NativeMaintenanceCandidateParityError::Input),
    }
}

/// Clone one nullable finite number without changing its JSON representation.
fn nullable_json_number(
    value: &JsonValue,
    name: &str,
) -> Result<JsonValue, NativeMaintenanceCandidateParityError> {
    match field(value, name) {
        Some(JsonValue::Null) => Ok(JsonValue::Null),
        Some(JsonValue::Number(number)) if number.is_finite() => Ok(JsonValue::Number(*number)),
        _ => Err(NativeMaintenanceCandidateParityError::Input),
    }
}

/// Read one required finite metric.
fn finite(value: &JsonValue, name: &str) -> Result<f64, NativeMaintenanceCandidateParityError> {
    let JsonValue::Number(value) =
        field(value, name).ok_or(NativeMaintenanceCandidateParityError::Input)?
    else {
        return Err(NativeMaintenanceCandidateParityError::Input);
    };
    // refuse nonfinite fitter material before numerical evaluation
    if !value.is_finite() {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    Ok(*value)
}

/// Read one exact singleton string array.
fn singleton_text(
    value: &JsonValue,
    name: &str,
) -> Result<String, NativeMaintenanceCandidateParityError> {
    let JsonValue::Array(values) =
        field(value, name).ok_or(NativeMaintenanceCandidateParityError::Input)?
    else {
        return Err(NativeMaintenanceCandidateParityError::Input);
    };
    // retain the source's exact singleton archive lineage
    if values.len() != 1 {
        return Err(NativeMaintenanceCandidateParityError::Input);
    }
    let JsonValue::String(value) = &values[0] else {
        return Err(NativeMaintenanceCandidateParityError::Input);
    };
    String::from_utf16(value).map_err(|_| NativeMaintenanceCandidateParityError::Input)
}

/// Require one exact object key set with no duplicates or extensions.
fn exact_keys(value: &JsonValue, expected: &[&str]) -> bool {
    let JsonValue::Object(fields) = value else {
        return false;
    };
    let mut actual = fields
        .iter()
        .map(|(name, _)| String::from_utf16(name))
        .collect::<Result<Vec<_>, _>>()
        .ok();
    let Some(actual) = actual.as_mut() else {
        return false;
    };
    actual.sort();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    actual.iter().map(String::as_str).eq(expected)
}

/// Require one exact source monthly label.
fn valid_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && matches!(
            &bytes[5..],
            b"01"
                | b"02"
                | b"03"
                | b"04"
                | b"05"
                | b"06"
                | b"07"
                | b"08"
                | b"09"
                | b"10"
                | b"11"
                | b"12"
        )
}

/// Require one lowercase SHA-256 spelling.
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Admit one canonical UTC millisecond clock and return its epoch value.
fn canonical_milliseconds(value: &str) -> Result<i64, NativeMaintenanceCandidateParityError> {
    NativeWirePrimitives
        .canonical_utc_milliseconds(value)
        .ok_or(NativeMaintenanceCandidateParityError::Input)
}

/// Return one generated enum's exact public spelling.
fn enum_wire<T: Copy + PartialEq>(
    value: T,
    values: &[(&'static str, T)],
) -> Result<&'static str, NativeMaintenanceCandidateParityError> {
    values
        .iter()
        .find_map(|(wire, candidate)| (*candidate == value).then_some(*wire))
        .ok_or(NativeMaintenanceCandidateParityError::Evaluation)
}

/// Build one UTF-16 object member.
fn pair(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Build one UTF-16 string scalar.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
