//! Verify the retained self-addressed wind or temperature model package before interpreting it.

use weather_contracts::{
    AdjustmentShadowFamily, ForecastAdjustmentMaintenanceRuntimePackage, ForecastAdjustmentMetric,
    ForecastAdjustmentWindMaintenanceRuntimePackage, ForecastLeadBandKey, JsonValue, Nullable,
    TemperatureMosPermanentRuntimeModel, TemperatureMosRuntimeModelMaterialScopeEnum,
};
use weather_database::Sha256Digest;

use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_maintenance_candidate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceCandidateError, exact_keys, family_name, field,
    text, valid_hash,
};
use crate::api_maintenance_candidate_catalog::ApiShadowInstalledReceipt;
use crate::api_qualified_bundle_projection::project_qualified_bundle;

type CandidateError = ApiMaintenanceCandidateError;

const PACKAGE_VERSION: &str = "forecast-adjustment-maintenance-runtime-package/v1";
const TEMPERATURE_KEYS: &[&str] = &[
    "bundleSha256",
    "candidateSha256",
    "contractVersion",
    "dueMonth",
    "family",
    "model",
    "source",
];
const WIND_KEYS: &[&str] = &[
    "bundleSha256",
    "candidateSha256",
    "contractVersion",
    "dueMonth",
    "family",
    "candidate",
    "source",
];
const WIND_CANDIDATE_KEYS: &[&str] = &[
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
// canonical LF-terminated digests of the two retained candidate.ts lineage constants
const WIND_FORECAST_IDENTITY: &str =
    "8348b5c48d7fe614ced9442b04e8a68cfe704e3d541eab3b8d0ab33a41cd4079";
const WIND_TRAINING_PROVENANCE: &str =
    "cfa905468fb15fb10b3abf02a39db5823d8d3bb17c6dddc625749eb9fe3bb8c3";
const TEMPERATURE_SOURCE_IDENTITY: &str =
    "67e5f000e0eb2c56a68b7846922773fadefd776400975814838e3ff97e0723af";

/// Minimal canonical-value owner shared by installed and comparator package verification.
trait PackageValues {
    /// Decode one closed generated package document.
    fn decode_package(
        &mut self,
        value: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage>;

    /// Encode recursively sorted canonical JSON with its original final LF.
    fn encode_json(&mut self, value: &JsonValue) -> Option<Vec<u8>>;

    /// Hash one exact canonical preimage.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest>;

    /// Admit one exact canonical UTC millisecond instant.
    fn is_canonical_utc_instant(&mut self, value: &str) -> bool;
}

struct CandidatePackageValues<'a, Codec>(&'a mut Codec);

impl<Codec: ApiMaintenanceCandidateCodec> PackageValues for CandidatePackageValues<'_, Codec> {
    /// Reuse the installed-package generated decoder.
    fn decode_package(
        &mut self,
        value: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        self.0.decode_maintenance_package(value)
    }

    /// Reuse installed-candidate canonical JSON.
    fn encode_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        self.0.encode_canonical_json(value)
    }

    /// Reuse installed-candidate SHA-256.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.0.sha256(bytes)
    }

    /// Reuse installed-candidate UTC admission.
    fn is_canonical_utc_instant(&mut self, value: &str) -> bool {
        self.0.is_canonical_utc_instant(value)
    }
}

struct ComparatorPackageValues<'a, Values>(&'a Values);

impl<Values: ApiMaintenanceShadowEvidenceValues> PackageValues
    for ComparatorPackageValues<'_, Values>
{
    /// Decode the already parsed comparator package without requiring canonical raw text.
    fn decode_package(
        &mut self,
        value: &JsonValue,
    ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
        self.0.decode_shadow_maintenance_package(value)
    }

    /// Canonicalize only the projected hash preimages, matching Node package verification.
    fn encode_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        self.0.encode_shadow_sorted_json(value)
    }

    /// Reuse the retained shadow SHA-256 owner.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.0.sha256(bytes)
    }

    /// Admit the same exact four-digit UTC spelling as the source wire validator.
    fn is_canonical_utc_instant(&mut self, value: &str) -> bool {
        value.len() == 24
            && value.as_bytes()[..4].iter().all(u8::is_ascii_digit)
            && self.0.canonical_utc_epoch_milliseconds(value).is_some()
    }
}

/// Bind the selected bytes and every model invariant before exposing the generated package.
pub(crate) fn admit_api_shadow_package(
    family: AdjustmentShadowFamily,
    receipt: &ApiShadowInstalledReceipt,
    bytes: &[u8],
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<ForecastAdjustmentMaintenanceRuntimePackage, CandidateError> {
    let package = codec
        .parse_canonical_object(bytes)
        .ok_or(CandidateError::Canonical)?;
    let mut values = CandidatePackageValues(codec);
    verify_package_document(
        family,
        &receipt.bundle_sha256,
        Some(&receipt.candidate_sha256),
        &package,
        &mut values,
    )
}

/// admit an exact canonical serving package selected by an installed promotion
pub(crate) fn admit_qualified_api_canary_package(
    family: AdjustmentShadowFamily,
    bundle_sha256: &str,
    candidate_sha256: &str,
    bytes: &[u8],
    codec: &mut impl ApiMaintenanceCandidateCodec,
) -> Result<(JsonValue, ForecastAdjustmentMaintenanceRuntimePackage), CandidateError> {
    let document = codec
        .parse_canonical_object(bytes)
        .ok_or(CandidateError::Canonical)?;
    // preserve the original qualified artifact member bytes, not a reserialized substitute
    if codec.encode_canonical_json(&document).as_deref() != Some(bytes) {
        return Err(CandidateError::Canonical);
    }
    let package = project_qualified_bundle(&document, family).ok_or(CandidateError::Package)?;
    let mut values = CandidatePackageValues(codec);
    let package = verify_projected_package_document(
        family,
        bundle_sha256,
        Some(candidate_sha256),
        &document,
        package,
        &mut values,
    )?;
    Ok((document, package))
}

/// project and verify one already decoded qualified package without an external typed decoder
pub(crate) fn admit_projected_qualified_canary_package(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
    identity: &str,
    family: AdjustmentShadowFamily,
) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
    let package = project_qualified_bundle(doc, family)?;
    let mut values = ComparatorPackageValues(values);
    verify_projected_package_document(family, identity, None, doc, package, &mut values).ok()
}

/// Verify a decoded serving comparator package without requiring canonical raw package text.
pub(crate) fn verify_comparator_maintenance_package(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
    identity: &str,
    family: AdjustmentShadowFamily,
) -> bool {
    let mut values = ComparatorPackageValues(values);
    verify_package_document(family, identity, None, doc, &mut values).is_ok()
}

/// Project one decoded package through only family, self-identity and optional install binding.
fn verify_package_document(
    family: AdjustmentShadowFamily,
    identity: &str,
    installed_candidate_sha256: Option<&str>,
    package: &JsonValue,
    values: &mut impl PackageValues,
) -> Result<ForecastAdjustmentMaintenanceRuntimePackage, CandidateError> {
    let model = values
        .decode_package(package)
        .ok_or(CandidateError::Package)?;
    verify_projected_package_document(
        family,
        identity,
        installed_candidate_sha256,
        package,
        model,
        values,
    )
}

/// verify one projected serving package against its source document and admitted graph
fn verify_projected_package_document(
    family: AdjustmentShadowFamily,
    identity: &str,
    installed_candidate_sha256: Option<&str>,
    package: &JsonValue,
    model: ForecastAdjustmentMaintenanceRuntimePackage,
    values: &mut impl PackageValues,
) -> Result<ForecastAdjustmentMaintenanceRuntimePackage, CandidateError> {
    let keys = match family {
        AdjustmentShadowFamily::Temperature => TEMPERATURE_KEYS,
        AdjustmentShadowFamily::Wind => WIND_KEYS,
        AdjustmentShadowFamily::Rain => return Err(CandidateError::Package),
    };
    let due_month = text(package, "dueMonth")
        .filter(|month| valid_month(month))
        .ok_or(CandidateError::Package)?;
    let candidate_sha256 = text(package, "candidateSha256")
        .filter(|value| valid_hash(value))
        .ok_or(CandidateError::Package)?;
    // retain the selected family and exact self-addressed package without adding install authority
    if !exact_keys(package, keys)
        || text(package, "contractVersion").as_deref() != Some(PACKAGE_VERSION)
        || text(package, "family").as_deref() != Some(family_name(family))
        || text(package, "bundleSha256").as_deref() != Some(identity)
        || installed_candidate_sha256.is_some_and(|expected| candidate_sha256 != expected)
        || !hash_without_member(package, "bundleSha256", identity, values)?
    {
        return Err(CandidateError::Package);
    }
    // the decoded generation must match the independently checked wire document
    match &model {
        ForecastAdjustmentMaintenanceRuntimePackage::Temperature(decoded)
            if family == AdjustmentShadowFamily::Temperature =>
        {
            if decoded.bundleSha256 != identity
                || decoded.candidateSha256 != candidate_sha256
                || decoded.dueMonth != due_month
                || !canonical_hash_matches(
                    field(package, "source").ok_or(CandidateError::Package)?,
                    TEMPERATURE_SOURCE_IDENTITY,
                    values,
                )?
                || !temperature_model_matches(&decoded.model, &due_month, values)
            {
                return Err(CandidateError::Package);
            }
        }
        ForecastAdjustmentMaintenanceRuntimePackage::Wind(decoded)
            if family == AdjustmentShadowFamily::Wind =>
        {
            if decoded.bundleSha256 != identity
                || decoded.candidateSha256 != candidate_sha256
                || decoded.dueMonth != due_month
                || !wind_model_matches(package, decoded, values)?
            {
                return Err(CandidateError::Package);
            }
        }
        _ => return Err(CandidateError::Package),
    }
    Ok(model)
}

/// Require the admitted permanent coefficient vectors and both causal strength bands.
fn temperature_model_matches(
    model: &TemperatureMosPermanentRuntimeModel,
    due_month: &str,
    values: &mut impl PackageValues,
) -> bool {
    // the generated shapes retain the fixed source and model literals
    if model.scope != TemperatureMosRuntimeModelMaterialScopeEnum::AssumedDelay6Next12
        || !model.supported
        || !finite_vector(&model.adaptiveCoefficients, 49)
        || !finite_vector(&model.directCoefficients, 35)
        || model.effectiveFrom.get(..7) != Some(due_month)
        || ![
            &model.effectiveFrom,
            &model.latestTrainingValidAt,
            &model.trainingCutoffUtc,
        ]
        .into_iter()
        .all(|value| values.is_canonical_utc_instant(value))
    {
        return false;
    }
    [
        &model.strengthBands.hours1To6,
        &model.strengthBands.hours7To12,
    ]
    .into_iter()
    .all(|band| {
        band.supported
            && [0.35, 0.5, 0.65, 0.8, 1.0].contains(&band.alpha)
            && values.is_canonical_utc_instant(&band.trainingCutoffUtc)
            && band.trainingCutoffUtc < model.effectiveFrom
    })
}

/// Recognize an exact finite selected coefficient vector.
fn finite_vector(value: &Nullable<Vec<f64>>, width: usize) -> bool {
    matches!(value, Nullable::Value(coefficients) if coefficients.len() == width && coefficients.iter().all(|value| value.is_finite()))
}

/// Recompute the wind fit addresses and retain the original canonical lineage and pair order.
fn wind_model_matches(
    package: &JsonValue,
    decoded: &ForecastAdjustmentWindMaintenanceRuntimePackage,
    values: &mut impl PackageValues,
) -> Result<bool, CandidateError> {
    let candidate = field(package, "candidate").ok_or(CandidateError::Package)?;
    let fitted = &decoded.candidate;
    let coefficients = field(candidate, "coefficients").ok_or(CandidateError::Package)?;
    let forecast_identity = field(candidate, "forecastIdentity").ok_or(CandidateError::Package)?;
    let provenance = field(candidate, "trainingProvenance").ok_or(CandidateError::Package)?;
    // compare canonical bytes for the exact Node source and known provenance objects
    if !exact_keys(candidate, WIND_CANDIDATE_KEYS)
        || !valid_hash(&fitted.candidateArtifactSha256)
        || !valid_hash(&fitted.coefficientPayloadSha256)
        || decoded.source != fitted.forecastIdentity
        || !hash_without_member(
            candidate,
            "candidateArtifactSha256",
            &fitted.candidateArtifactSha256,
            values,
        )?
        || !canonical_hash_matches(coefficients, &fitted.coefficientPayloadSha256, values)?
        || !canonical_hash_matches(forecast_identity, WIND_FORECAST_IDENTITY, values)?
        || !canonical_hash_matches(provenance, WIND_TRAINING_PROVENANCE, values)?
        || contains_post_fit_field(candidate)
        || fitted.enabledMetricBands.len() != 13
    {
        return Ok(false);
    }
    let mut previous = None;
    // wire table ordering matches the original lexicographic metric-band sort
    for pair in &fitted.enabledMetricBands {
        // the original maintenance package excludes direction and the unavailable gust band
        if pair.metric == ForecastAdjustmentMetric::WindDirectionDegrees
            || (pair.metric == ForecastAdjustmentMetric::WindGustMps
                && pair.leadBand == ForecastLeadBandKey::Value049072)
        {
            return Ok(false);
        }
        let metric = ForecastAdjustmentMetric::WIRE_VALUES
            .iter()
            .position(|(_, value)| *value == pair.metric);
        let lead = ForecastLeadBandKey::WIRE_VALUES
            .iter()
            .position(|(_, value)| *value == pair.leadBand);
        let position = metric.zip(lead).ok_or(CandidateError::Package)?;
        // a strict increase preserves the canonical order and rejects repeated pairs
        if previous.is_some_and(|previous| previous >= position) {
            return Ok(false);
        }
        previous = Some(position);
    }
    Ok(true)
}

/// Walk the decoded fit and match the original excluded post-fit evidence keys.
fn contains_post_fit_field(value: &JsonValue) -> bool {
    let mut queue = vec![value];
    // keep traversal bounded by the actual parsed material, without recursive calls
    while let Some(value) = queue.pop() {
        match value {
            JsonValue::Array(items) => queue.extend(items),
            JsonValue::Object(fields) => {
                // visit all nested candidate members before accepting the fit
                for (name, child) in fields {
                    // U+017F folds to s in the original JavaScript unicode-insensitive pattern
                    let name = String::from_utf16_lossy(name).replace('ſ', "s");
                    if [
                        "actual",
                        "adjustedLoss",
                        "evaluationReport",
                        "holdout",
                        "qualification",
                        "rawLoss",
                        "receipt",
                    ]
                    .iter()
                    .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
                    {
                        return true;
                    }
                    queue.push(child);
                }
            }
            _ => {}
        }
    }
    false
}

/// Compute the immutable package or fit address with only its self hash removed.
fn hash_without_member(
    value: &JsonValue,
    key: &str,
    expected: &str,
    values: &mut impl PackageValues,
) -> Result<bool, CandidateError> {
    let JsonValue::Object(fields) = value else {
        return Err(CandidateError::Package);
    };
    let material = JsonValue::Object(
        fields
            .iter()
            .filter(|(name, _)| !name.iter().copied().eq(key.encode_utf16()))
            .cloned()
            .collect(),
    );
    canonical_hash_matches(&material, expected, values)
}

/// Hash canonical bytes through the admitted process codec, not a hand-written serializer.
fn canonical_hash_matches(
    value: &JsonValue,
    expected: &str,
    values: &mut impl PackageValues,
) -> Result<bool, CandidateError> {
    let bytes = values.encode_json(value).ok_or(CandidateError::Canonical)?;
    let digest = values.sha256(&bytes).ok_or(CandidateError::Canonical)?;
    Ok(digest.as_str() == expected)
}

/// Require one exact zero-padded monthly scope from the original package header.
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
