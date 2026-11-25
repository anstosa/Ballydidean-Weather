//! project a verified temperature artifact into generated contracts

use weather_contracts::{self as contracts, JsonValue, Nullable};

use crate::api_shadow_legacy_values::{boolean_field, integer_field, number_field, text_equals};
use crate::maintenance_canonical_json::{exact_keys, field, text};

const TIME_LIMITED_BUNDLE: &str = "forecast-adjustment-temperature-canary-bundle/v1";
const PERMANENT_BUNDLE: &str = "forecast-adjustment-temperature-canary-bundle/v2";

/// project one verified temperature artifact
pub(crate) fn project_verified_temperature_bundle(
    document: &JsonValue,
) -> Option<contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundle> {
    let common = project_common_bundle(document)?;
    let authorization = field(document, "authorization")?;
    let model = field(document, "model")?;

    // preserve each verified generation's distinct model and authorization shape
    match text(document, "contractVersion")?.as_str() {
        TIME_LIMITED_BUNDLE => Some(
            contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundle::TimeLimited(
                contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1 {
                    artifactKind: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ArtifactKindLiteral,
                    authorization: project_time_limited_authorization(authorization)?,
                    bundleSha256: common.bundle_sha256,
                    contractVersion: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ContractVersionLiteral,
                    evidence: common.evidence,
                    model: project_monthly_model(model)?,
                    runtimeFingerprint: common.runtime_fingerprint,
                    servedForecastIdentity: common.served_forecast_identity,
                    siteKey: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1SiteKeyLiteral,
                    timezone: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TimezoneLiteral,
                    trainingForecastIdentity: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentity {
                        cohort: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentityCohortLiteral,
                        scope: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentityScopeLiteral,
                    },
                },
            ),
        ),
        PERMANENT_BUNDLE => Some(
            contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundle::Permanent(
                contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV2 {
                    artifactKind: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ArtifactKindLiteral,
                    bundleSha256: common.bundle_sha256,
                    evidence: common.evidence,
                    runtimeFingerprint: common.runtime_fingerprint,
                    servedForecastIdentity: common.served_forecast_identity,
                    siteKey: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1SiteKeyLiteral,
                    timezone: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TimezoneLiteral,
                    trainingForecastIdentity: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentity {
                        cohort: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentityCohortLiteral,
                        scope: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentityScopeLiteral,
                    },
                    authorization: project_permanent_authorization(authorization)?,
                    contractVersion: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV2ContractVersionLiteral,
                    model: project_permanent_model(model)?,
                },
            ),
        ),
        _ => None,
    }
}

/// hold shared mutable bundle members
struct CommonBundleProjection {
    bundle_sha256: String,
    evidence: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1Evidence,
    runtime_fingerprint:
        contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1RuntimeFingerprint,
    served_forecast_identity:
        contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentity,
}

/// project common mutable bundle members
fn project_common_bundle(document: &JsonValue) -> Option<CommonBundleProjection> {
    let evidence = field(document, "evidence")?;
    let fingerprint = field(document, "runtimeFingerprint")?;
    let identity = field(document, "servedForecastIdentity")?;
    Some(CommonBundleProjection {
        bundle_sha256: text(document, "bundleSha256")?,
        evidence: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1Evidence {
            modelSourceSha256: text(evidence, "modelSourceSha256")?,
            researchSummarySha256: text(evidence, "researchSummarySha256")?,
            retentionManifestSha256: text(evidence, "retentionManifestSha256")?,
            strengthSourceSha256: text(evidence, "strengthSourceSha256")?,
        },
        runtime_fingerprint:
            contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1RuntimeFingerprint {
                icuVersion: text(fingerprint, "icuVersion")?,
                tzdataVersion: text(fingerprint, "tzdataVersion")?,
            },
        served_forecast_identity:
            contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentity {
                adapterVersion: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityAdapterVersionLiteral,
                dataset: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityDatasetLiteral,
                maximumReceiptAgeHours: integer_field(identity, "maximumReceiptAgeHours")?
                    .try_into()
                    .ok()?,
                providerKey: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityProviderKeyLiteral,
                sourceDelayHours: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentitySourceDelayHoursLiteral,
                upstreamModel: contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityUpstreamModelLiteral,
            },
    })
}

/// project finite authorization fields
fn project_time_limited_authorization(
    authorization: &JsonValue,
) -> Option<contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV1> {
    Some(
        contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV1 {
            activatedAt: text(authorization, "activatedAt")?,
            authorizationReason: text(authorization, "authorizationReason")?,
            authorizationSha256: text(authorization, "authorizationSha256")?,
            authorized:
                contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV1AuthorizedLiteral,
            authorizedAt: text(authorization, "authorizedAt")?,
            authorizedBy: text(authorization, "authorizedBy")?,
            expiresAt: text(authorization, "expiresAt")?,
        },
    )
}

/// project permanent authorization fields
fn project_permanent_authorization(
    authorization: &JsonValue,
) -> Option<contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV2> {
    Some(
        contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV2 {
            activatedAt: text(authorization, "activatedAt")?,
            artifactKind:
                contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV2ArtifactKindLiteral,
            authorizationReason: text(authorization, "authorizationReason")?,
            authorizationSha256: text(authorization, "authorizationSha256")?,
            authorized:
                contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV2AuthorizedLiteral,
            authorizedAt: text(authorization, "authorizedAt")?,
            authorizedBy: text(authorization, "authorizedBy")?,
            contractVersion:
                contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV2ContractVersionLiteral,
            expiresAt: contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV2ExpiresAtNull,
            permanent:
                contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV2PermanentLiteral,
        },
    )
}

/// project one verified monthly model
fn project_monthly_model(model: &JsonValue) -> Option<contracts::TemperatureMosRuntimeModel> {
    text_equals(
        model,
        "contractVersion",
        contracts::TemperatureMosRuntimeModelContractVersionLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    let material = project_model_material(model)?;
    Some(contracts::TemperatureMosRuntimeModel {
        adaptiveCoefficients: material.adaptive_coefficients,
        cohort: contracts::TemperatureMosRuntimeModelMaterialCohortLiteral,
        directCoefficients: material.direct_coefficients,
        learnedStrengthContractVersion:
            contracts::TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral,
        scope: contracts::TemperatureMosRuntimeModelMaterialScopeEnum::AssumedDelay6Next12,
        strengthBands: material.strength_bands,
        supported: material.supported,
        trainingCutoffUtc: material.training_cutoff_utc,
        contractVersion: contracts::TemperatureMosRuntimeModelContractVersionLiteral,
        month: text(model, "month")?,
    })
}

/// project one verified permanent model
pub(crate) fn project_permanent_model(
    model: &JsonValue,
) -> Option<contracts::TemperatureMosPermanentRuntimeModel> {
    exact_keys(
        model,
        &[
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
        ],
    )
    .then_some(())?;
    text_equals(
        model,
        "contractVersion",
        contracts::TemperatureMosPermanentRuntimeModelContractVersionLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    let material = project_model_material(model)?;
    Some(contracts::TemperatureMosPermanentRuntimeModel {
        adaptiveCoefficients: material.adaptive_coefficients,
        cohort: contracts::TemperatureMosRuntimeModelMaterialCohortLiteral,
        directCoefficients: material.direct_coefficients,
        learnedStrengthContractVersion:
            contracts::TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral,
        scope: contracts::TemperatureMosRuntimeModelMaterialScopeEnum::AssumedDelay6Next12,
        strengthBands: material.strength_bands,
        supported: material.supported,
        trainingCutoffUtc: material.training_cutoff_utc,
        contractVersion: contracts::TemperatureMosPermanentRuntimeModelContractVersionLiteral,
        effectiveFrom: text(model, "effectiveFrom")?,
        latestTrainingValidAt: text(model, "latestTrainingValidAt")?,
    })
}

/// hold shared mutable model material
struct ModelMaterialProjection {
    adaptive_coefficients: Nullable<Vec<f64>>,
    direct_coefficients: Nullable<Vec<f64>>,
    strength_bands: contracts::TemperatureMosRuntimeModelMaterialStrengthBands,
    supported: bool,
    training_cutoff_utc: String,
}

/// project shared verified model material
fn project_model_material(model: &JsonValue) -> Option<ModelMaterialProjection> {
    // check fixed values before constructing their generated representations
    text_equals(
        model,
        "cohort",
        contracts::TemperatureMosRuntimeModelMaterialCohortLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(model, "learnedStrengthContractVersion",
        contracts::TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral::WIRE_VALUE).then_some(())?;
    text_equals(model, "scope", "assumed_delay6_next12").then_some(())?;
    let strength_bands = field(model, "strengthBands")?;
    exact_keys(strength_bands, &["1-6", "7-12"]).then_some(())?;
    Some(ModelMaterialProjection {
        adaptive_coefficients: Nullable::Value(project_coefficient_vector(
            model,
            "adaptiveCoefficients",
        )?),
        direct_coefficients: Nullable::Value(project_coefficient_vector(
            model,
            "directCoefficients",
        )?),
        strength_bands: contracts::TemperatureMosRuntimeModelMaterialStrengthBands {
            hours1To6: project_strength_band(field(strength_bands, "1-6")?)?,
            hours7To12: project_strength_band(field(strength_bands, "7-12")?)?,
        },
        supported: boolean_field(model, "supported")?,
        training_cutoff_utc: text(model, "trainingCutoffUtc")?,
    })
}

/// copy one verified coefficient vector
fn project_coefficient_vector(model: &JsonValue, key: &str) -> Option<Vec<f64>> {
    let JsonValue::Array(values) = field(model, key)? else {
        return None;
    };
    // copy only verified numeric members
    values
        .iter()
        .map(|value| match value {
            JsonValue::Number(number) if number.is_finite() => Some(*number),
            _ => None,
        })
        .collect()
}

/// project one verified strength band
fn project_strength_band(band: &JsonValue) -> Option<contracts::TemperatureMosRuntimeStrengthBand> {
    exact_keys(band, &["alpha", "supported", "trainingCutoffUtc"]).then_some(())?;
    Some(contracts::TemperatureMosRuntimeStrengthBand {
        alpha: number_field(band, "alpha")?,
        supported: boolean_field(band, "supported")?,
        trainingCutoffUtc: text(band, "trainingCutoffUtc")?,
    })
}
