//! Project a verified qualified maintenance package without an external typed decoder.

use weather_contracts::{self as c, AdjustmentShadowFamily, JsonValue};

use crate::api_forecast_model_projection::{project_candidate_v2, project_forecast_identity};
use crate::api_maintenance_candidate::exact_keys;
use crate::api_shadow_legacy_values::{integer_field, text_equals};
use crate::api_temperature_runtime_projection::project_permanent_model;
use crate::maintenance_canonical_json::{field, text};

const TEMPERATURE_PACKAGE_KEYS: &[&str] = &[
    "bundleSha256",
    "candidateSha256",
    "contractVersion",
    "dueMonth",
    "family",
    "model",
    "source",
];
const WIND_PACKAGE_KEYS: &[&str] = &[
    "bundleSha256",
    "candidateSha256",
    "contractVersion",
    "dueMonth",
    "family",
    "candidate",
    "source",
];
const TEMPERATURE_MODEL_KEYS: &[&str] = &[
    "adaptiveCoefficients",
    "cohort",
    "contractVersion",
    "directCoefficients",
    "effectiveFrom",
    "latestTrainingValidAt",
    "learnedStrengthContractVersion",
    "scope",
    "strengthBands",
    "supported",
    "trainingCutoffUtc",
];
const TEMPERATURE_SOURCE_KEYS: &[&str] = &[
    "adapterVersion",
    "cohort",
    "dataset",
    "maximumReceiptAgeHours",
    "providerKey",
    "scope",
    "sourceDelayHours",
    "upstreamModel",
];
const STRENGTH_BAND_KEYS: &[&str] = &["alpha", "supported", "trainingCutoffUtc"];
const STRENGTH_BANDS_KEYS: &[&str] = &["1-6", "7-12"];
const CANDIDATE_KEYS: &[&str] = &[
    "algorithmContractVersion",
    "candidateArtifactSha256",
    "coefficientPayloadSha256",
    "coefficients",
    "contractVersion",
    "developmentReportSha256",
    "enabledMetricBands",
    "evaluationEpochId",
    "exportManifestSha256",
    "finalTrainingCutoff",
    "forecastIdentity",
    "metricPolicies",
    "runtimeFingerprint",
    "siteKey",
    "timezone",
    "trainingEnvelopes",
    "trainingProvenance",
];
const COEFFICIENT_KEYS: &[&str] = &[
    "coefficient",
    "daypart",
    "effectiveEventCount",
    "leadBand",
    "level",
    "metric",
    "month",
    "season",
];
const FORECAST_IDENTITY_KEYS: &[&str] = &[
    "adapterVersion",
    "cohort",
    "contractEpoch",
    "dataset",
    "referenceKind",
    "sourceConfigFingerprint",
    "sourceKey",
    "upstreamModel",
];
const METRIC_BAND_KEYS: &[&str] = &["leadBand", "metric"];
const METRIC_POLICY_KEYS: &[&str] = &[
    "correctionMaximum",
    "correctionMinimum",
    "finalMaximum",
    "finalMaximumExclusive",
    "finalMinimum",
    "metric",
    "wrapsFinalValue",
];
const RUNTIME_FINGERPRINT_KEYS: &[&str] = &["icuVersion", "tzdataVersion"];
const TRAINING_ENVELOPE_KEYS: &[&str] = &["leadBand", "maximum", "metric", "minimum"];
const TRAINING_PROVENANCE_KEYS: &[&str] = &[
    "aggregationContractSha256",
    "coordinateManifestSha256",
    "metricEligibilitySha256",
    "observationSourceLineageSha256",
    "observationStationManifestSha256",
    "spatialWeightSha256",
];

/// Decode a maintenance package into the generated TypeScript DTO using its family discriminator.
pub fn decode_maintenance_runtime_package(
    document: &JsonValue,
) -> Option<c::ForecastAdjustmentMaintenanceRuntimePackage> {
    let family = match text(document, "family")?.as_str() {
        "temperature" => AdjustmentShadowFamily::Temperature,
        "wind" => AdjustmentShadowFamily::Wind,
        _ => return None,
    };
    project_qualified_bundle(document, family)
}

/// project one closed generated qualified package after its canonical graph is admitted
pub(crate) fn project_qualified_bundle(
    document: &JsonValue,
    family: AdjustmentShadowFamily,
) -> Option<c::ForecastAdjustmentMaintenanceRuntimePackage> {
    // select only the requested family instead of trusting the document discriminant
    match family {
        AdjustmentShadowFamily::Temperature => project_temperature_package(document),
        AdjustmentShadowFamily::Wind => project_wind_package(document),
        AdjustmentShadowFamily::Rain => None,
    }
}

/// project the closed temperature package and its exact generated literal objects
fn project_temperature_package(
    document: &JsonValue,
) -> Option<c::ForecastAdjustmentMaintenanceRuntimePackage> {
    exact_keys(document, TEMPERATURE_PACKAGE_KEYS).then_some(())?;
    text_equals(
        document,
        "contractVersion",
        c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageContractVersionLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(
        document,
        "family",
        c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageFamilyLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    let model = field(document, "model")?;
    let source = field(document, "source")?;
    temperature_model_shape_matches(model).then_some(())?;
    temperature_source_matches(source).then_some(())?;
    Some(c::ForecastAdjustmentMaintenanceRuntimePackage::Temperature(
        c::ForecastAdjustmentTemperatureMaintenanceRuntimePackage {
            bundleSha256: text(document, "bundleSha256")?,
            candidateSha256: text(document, "candidateSha256")?,
            contractVersion:
                c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageContractVersionLiteral,
            dueMonth: text(document, "dueMonth")?,
            family: c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageFamilyLiteral,
            model: project_permanent_model(model)?,
            source: c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceObject {
                adapterVersion: c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceAdapterVersionLiteral,
                cohort: c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceCohortLiteral,
                dataset: c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceDatasetLiteral,
                maximumReceiptAgeHours: c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceMaximumReceiptAgeHoursLiteral,
                providerKey: c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceProviderKeyLiteral,
                scope: c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceScopeLiteral,
                sourceDelayHours: c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceSourceDelayHoursLiteral,
                upstreamModel: c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceUpstreamModelLiteral,
            },
        },
    ))
}

/// project the closed wind package and every ordered fitted candidate member
fn project_wind_package(
    document: &JsonValue,
) -> Option<c::ForecastAdjustmentMaintenanceRuntimePackage> {
    exact_keys(document, WIND_PACKAGE_KEYS).then_some(())?;
    text_equals(
        document,
        "contractVersion",
        c::ForecastAdjustmentWindMaintenanceRuntimePackageContractVersionLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(
        document,
        "family",
        c::ForecastAdjustmentWindMaintenanceRuntimePackageFamilyLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    let candidate = field(document, "candidate")?;
    let source = field(document, "source")?;
    candidate_shape_matches(candidate).then_some(())?;
    exact_keys(source, FORECAST_IDENTITY_KEYS).then_some(())?;
    Some(c::ForecastAdjustmentMaintenanceRuntimePackage::Wind(
        c::ForecastAdjustmentWindMaintenanceRuntimePackage {
            bundleSha256: text(document, "bundleSha256")?,
            candidate: project_candidate_v2(candidate)?,
            candidateSha256: text(document, "candidateSha256")?,
            contractVersion:
                c::ForecastAdjustmentWindMaintenanceRuntimePackageContractVersionLiteral,
            dueMonth: text(document, "dueMonth")?,
            family: c::ForecastAdjustmentWindMaintenanceRuntimePackageFamilyLiteral,
            source: project_forecast_identity(source)?,
        },
    ))
}

/// require the generated permanent-model schema before fixed literals are materialized
fn temperature_model_shape_matches(model: &JsonValue) -> bool {
    let Some(strength_bands) = field(model, "strengthBands") else {
        return false;
    };
    // require both named bands and reject extension fields in either band
    if !exact_keys(model, TEMPERATURE_MODEL_KEYS)
        || !exact_keys(strength_bands, STRENGTH_BANDS_KEYS)
        || !["1-6", "7-12"].into_iter().all(|key| {
            field(strength_bands, key).is_some_and(|band| exact_keys(band, STRENGTH_BAND_KEYS))
        })
    {
        return false;
    }
    text_equals(
        model,
        "contractVersion",
        c::TemperatureMosPermanentRuntimeModelContractVersionLiteral::WIRE_VALUE,
    ) && text_equals(
        model,
        "cohort",
        c::TemperatureMosRuntimeModelMaterialCohortLiteral::WIRE_VALUE,
    ) && text_equals(
        model,
        "learnedStrengthContractVersion",
        c::TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral::WIRE_VALUE,
    ) && text_equals(model, "scope", "assumed_delay6_next12")
}

/// require the exact frozen temperature source object represented by generated literals
fn temperature_source_matches(source: &JsonValue) -> bool {
    exact_keys(source, TEMPERATURE_SOURCE_KEYS)
        && text_equals(
            source,
            "adapterVersion",
            c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceAdapterVersionLiteral::WIRE_VALUE,
        )
        && text_equals(
            source,
            "cohort",
            c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceCohortLiteral::WIRE_VALUE,
        )
        && text_equals(
            source,
            "dataset",
            c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceDatasetLiteral::WIRE_VALUE,
        )
        && integer_field(source, "maximumReceiptAgeHours")
            == Some(c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceMaximumReceiptAgeHoursLiteral::WIRE_VALUE as i64)
        && text_equals(
            source,
            "providerKey",
            c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceProviderKeyLiteral::WIRE_VALUE,
        )
        && text_equals(
            source,
            "scope",
            c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceScopeLiteral::WIRE_VALUE,
        )
        && integer_field(source, "sourceDelayHours")
            == Some(c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceSourceDelayHoursLiteral::WIRE_VALUE as i64)
        && text_equals(
            source,
            "upstreamModel",
            c::ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceUpstreamModelLiteral::WIRE_VALUE,
        )
}

/// require the generated candidate object and every nested closed object shape
fn candidate_shape_matches(candidate: &JsonValue) -> bool {
    let Some(forecast_identity) = field(candidate, "forecastIdentity") else {
        return false;
    };
    let Some(fingerprint) = field(candidate, "runtimeFingerprint") else {
        return false;
    };
    let Some(provenance) = field(candidate, "trainingProvenance") else {
        return false;
    };
    exact_keys(candidate, CANDIDATE_KEYS)
        && exact_keys(forecast_identity, FORECAST_IDENTITY_KEYS)
        && exact_keys(fingerprint, RUNTIME_FINGERPRINT_KEYS)
        && exact_keys(provenance, TRAINING_PROVENANCE_KEYS)
        && array_objects_have_exact_keys(candidate, "coefficients", COEFFICIENT_KEYS)
        && array_objects_have_exact_keys(candidate, "enabledMetricBands", METRIC_BAND_KEYS)
        && array_objects_have_exact_keys(candidate, "metricPolicies", METRIC_POLICY_KEYS)
        && array_objects_have_exact_keys(candidate, "trainingEnvelopes", TRAINING_ENVELOPE_KEYS)
}

/// require every member of one generated object array to have its exact key set
fn array_objects_have_exact_keys(document: &JsonValue, key: &str, keys: &[&str]) -> bool {
    let Some(JsonValue::Array(items)) = field(document, key) else {
        return false;
    };
    // preserve array cardinality while rejecting nested extension fields
    items.iter().all(|item| exact_keys(item, keys))
}
