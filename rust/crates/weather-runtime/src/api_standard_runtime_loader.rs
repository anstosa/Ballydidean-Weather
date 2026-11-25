//! Load the root-installed standard forecast-adjustment runtime once at API startup.

use std::io::ErrorKind;

use weather_api::{WeatherApiStartupLoadError, WeatherApiStartupRuntimeLoader};
use weather_contracts::{
    FieldPresence, ForecastAdjustmentRegistryV1ContractVersionLiteral,
    ForecastAdjustmentRuntimeBundleV2SiteKeyLiteral,
    ForecastAdjustmentRuntimeBundleV2TimezoneLiteral, JsonValue,
    LoadedForecastAdjustmentRainRuntimeRegistryV1, LoadedForecastAdjustmentRuntimeV1,
    LoadedForecastAdjustmentRuntimeV1Active, LoadedForecastAdjustmentRuntimeV1ActiveReasonCodeNull,
    LoadedForecastAdjustmentRuntimeV1ActiveStateLiteral, LoadedForecastAdjustmentRuntimeV1Disabled,
    LoadedForecastAdjustmentRuntimeV1DisabledBundleNull,
    LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentRuntimeV1DisabledStateLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntime, LoadedForecastAdjustmentWindCanaryRuntime,
    MaintenanceShadowComparatorAuthorityKind, MaintenanceShadowServingAuthority, Nullable,
};

use crate::api_shadow_legacy_runtime::verify_legacy_wind_runtime_bundle;
use crate::api_standard_bundle_projection::project_verified_standard_bundle;
use crate::api_standard_runtime_projection::standard_runtime_projection_matches;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{ApiMaintenanceShadowEvidenceValues, NativeForecastRuntimeFiles};

const REGISTRY_KEYS: &[&str] = &["activeBundle", "contractVersion"];
const ACTIVE_KEYS: &[&str] = &[
    "bundleSha256",
    "candidateArtifactSha256",
    "evaluationReportSha256",
    "path",
    "qualificationReceiptSha256",
];

/// Existing JSON, hashing, calendar, and base64 effects used by the installed runtime loader.
pub trait ApiStandardRuntimeValues: ApiMaintenanceShadowEvidenceValues {
    /// Compare the installed model fingerprint to this process's ICU and tzdata owners.
    fn standard_calendar_matches(&self, icu_version: &str, tzdata_version: &str) -> bool;

    /// Encode exact admitted member bytes as canonical padded base64.
    fn encode_standard_base64(&self, bytes: &[u8]) -> Option<String>;
}

/// Load the standard qualified runtime selected by its root-installed registry.
pub fn load_installed_standard_runtime(
    files: &NativeForecastRuntimeFiles,
    values: &impl ApiStandardRuntimeValues,
) -> LoadedForecastAdjustmentRuntimeV1 {
    let registry_bytes = match files.read_standard_registry() {
        Ok(bytes) => bytes,
        Err(_) => {
            return disabled(
                LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
            );
        }
    };
    let Some(registry) = canonical_document(values, &registry_bytes) else {
        return disabled(LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::RegistryInvalid);
    };
    let selection = match select_registry(&registry) {
        Some(Some(selection)) => selection,
        Some(None) => {
            return disabled(
                LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
            );
        }
        None => {
            return disabled(
                LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
            );
        }
    };

    let bundle_bytes = match files.read_standard_bundle(&selection.bundle_sha256) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return disabled(
                LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::BundleMissing,
            );
        }
        Err(_) => {
            return disabled(
                LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::BundleInvalid,
            );
        }
    };
    load_selected_bundle(values, selection, &bundle_bytes).unwrap_or_else(|| {
        disabled(LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::BundleInvalid)
    })
}

/// Delegate independent runtime families while owning the native standard fallback.
pub struct NativeStandardApiStartupRuntimeLoader<Values, Other> {
    files: NativeForecastRuntimeFiles,
    values: Values,
    other: Other,
}

impl<Values, Other> NativeStandardApiStartupRuntimeLoader<Values, Other> {
    /// Retain the fixed installed root, admitted value effects, and independent loaders.
    pub const fn new(files: NativeForecastRuntimeFiles, values: Values, other: Other) -> Self {
        Self {
            files,
            values,
            other,
        }
    }

    /// Return the delegated loader after process startup or failure inspection.
    pub fn into_other(self) -> Other {
        self.other
    }
}

impl<Values, Other> WeatherApiStartupRuntimeLoader
    for NativeStandardApiStartupRuntimeLoader<Values, Other>
where
    Values: ApiStandardRuntimeValues,
    Other: WeatherApiStartupRuntimeLoader,
{
    /// Delegate the independently installed wind-canary selection.
    fn load_wind_canary(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError> {
        self.other.load_wind_canary()
    }

    /// Load the standard fallback through the native fixed-root owner.
    fn load_qualified_wind(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRuntimeV1, WeatherApiStartupLoadError> {
        Ok(load_installed_standard_runtime(&self.files, &self.values))
    }

    /// Delegate the independently installed temperature selection.
    fn load_temperature(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentTemperatureCanaryRuntime, WeatherApiStartupLoadError> {
        self.other.load_temperature()
    }

    /// Delegate the independently installed rain selection.
    fn load_rain(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRainRuntimeRegistryV1, WeatherApiStartupLoadError> {
        self.other.load_rain()
    }
}

/// One fully checked active registry selection borrowed from its decoded document.
struct StandardSelection {
    bundle_sha256: String,
    candidate_artifact_sha256: String,
    evaluation_report_sha256: String,
    qualification_receipt_sha256: String,
}

/// Parse the exact canonical registry and retain its one selection.
fn select_registry(registry: &JsonValue) -> Option<Option<StandardSelection>> {
    exact_keys(registry, REGISTRY_KEYS).then_some(())?;
    (text(registry, "contractVersion")?
        == ForecastAdjustmentRegistryV1ContractVersionLiteral::WIRE_VALUE)
        .then_some(())?;
    let active = field(registry, "activeBundle")?;
    // preserve the reviewed inactive registry without attempting bundle I/O
    if matches!(active, JsonValue::Null) {
        return Some(None);
    }
    exact_keys(active, ACTIVE_KEYS).then_some(())?;
    let bundle_sha256 = text(active, "bundleSha256")?;
    let candidate_artifact_sha256 = text(active, "candidateArtifactSha256")?;
    let evaluation_report_sha256 = text(active, "evaluationReportSha256")?;
    let qualification_receipt_sha256 = text(active, "qualificationReceiptSha256")?;
    // Require every selector identity before deriving its only permitted relative path.
    for digest in [
        &bundle_sha256,
        &candidate_artifact_sha256,
        &evaluation_report_sha256,
        &qualification_receipt_sha256,
    ] {
        valid_hash(digest).then_some(())?;
    }
    (text(active, "path")? == format!("bundles/sha256-{bundle_sha256}.json")).then_some(())?;
    Some(Some(StandardSelection {
        bundle_sha256,
        candidate_artifact_sha256,
        evaluation_report_sha256,
        qualification_receipt_sha256,
    }))
}

/// Verify the selected graph before projecting its generated serving contract.
fn load_selected_bundle(
    values: &impl ApiStandardRuntimeValues,
    selection: StandardSelection,
    bundle_bytes: &[u8],
) -> Option<LoadedForecastAdjustmentRuntimeV1> {
    let document = canonical_document(values, bundle_bytes)?;
    verify_legacy_wind_runtime_bundle(values, &document, &selection.bundle_sha256)?;
    let candidate = field(&document, "candidate")?;
    let report = field(&document, "evaluationReport")?;
    let receipt = field(&document, "qualificationReceipt")?;
    // Bind the registry to each public evidence identity before typed projection.
    if text(&document, "bundleSha256")? != selection.bundle_sha256
        || text(candidate, "candidateArtifactSha256")? != selection.candidate_artifact_sha256
        || text(report, "evaluationReportSha256")? != selection.evaluation_report_sha256
        || text(receipt, "qualificationReceiptSha256")? != selection.qualification_receipt_sha256
    {
        return None;
    }
    let bundle = project_verified_standard_bundle(&document)?;
    // refuse a generated projection not tied to the exact verified JSON graph
    if bundle.bundleSha256 != selection.bundle_sha256
        || bundle.candidate.candidateArtifactSha256 != selection.candidate_artifact_sha256
        || bundle.evaluationReport.evaluationReportSha256 != selection.evaluation_report_sha256
        || bundle.qualificationReceipt.qualificationReceiptSha256
            != selection.qualification_receipt_sha256
    {
        return None;
    }
    standard_runtime_projection_matches(&bundle, &document).then_some(())?;
    let fingerprint = field(candidate, "runtimeFingerprint")?;
    let icu_version = text(fingerprint, "icuVersion")?;
    let tzdata_version = text(fingerprint, "tzdataVersion")?;
    values
        .standard_calendar_matches(&icu_version, &tzdata_version)
        .then_some(())?;
    // Generated zero-sized literals carry these identities; the verified source remains explicit.
    (text(&document, "siteKey")? == ForecastAdjustmentRuntimeBundleV2SiteKeyLiteral::WIRE_VALUE)
        .then_some(())?;
    (text(&document, "timezone")? == ForecastAdjustmentRuntimeBundleV2TimezoneLiteral::WIRE_VALUE)
        .then_some(())?;

    let receipt_bytes = values.encode_shadow_sorted_json(receipt)?;
    let artifact_member = values.sha256(bundle_bytes)?;
    let receipt_member = values.sha256(&receipt_bytes)?;
    valid_hash(artifact_member.as_str()).then_some(())?;
    valid_hash(receipt_member.as_str()).then_some(())?;
    let authority = MaintenanceShadowServingAuthority {
        artifactBase64: Nullable::Value(values.encode_standard_base64(bundle_bytes)?),
        artifactIdentitySha256: Nullable::Value(selection.bundle_sha256),
        artifactMemberSha256: Nullable::Value(artifact_member.as_str().to_owned()),
        authorityKind: MaintenanceShadowComparatorAuthorityKind::LegacyActive,
        receiptBase64: values.encode_standard_base64(&receipt_bytes)?,
        receiptMemberSha256: receipt_member.as_str().to_owned(),
    };
    Some(LoadedForecastAdjustmentRuntimeV1::Active(
        LoadedForecastAdjustmentRuntimeV1Active {
            bundle,
            comparatorAuthority: FieldPresence::Value(authority),
            reasonCode: LoadedForecastAdjustmentRuntimeV1ActiveReasonCodeNull,
            state: LoadedForecastAdjustmentRuntimeV1ActiveStateLiteral,
        },
    ))
}

/// Decode only byte-identical canonical JSON objects.
pub(crate) fn canonical_document(
    values: &impl ApiStandardRuntimeValues,
    bytes: &[u8],
) -> Option<JsonValue> {
    let document = values.decode_shadow_json(bytes)?;
    matches!(&document, JsonValue::Object(_)).then_some(())?;
    (values.encode_shadow_sorted_json(&document)? == bytes).then_some(document)
}

/// Construct one closed disabled selection without preserving artifact detail.
fn disabled(
    reason_code: LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum,
) -> LoadedForecastAdjustmentRuntimeV1 {
    LoadedForecastAdjustmentRuntimeV1::Disabled(LoadedForecastAdjustmentRuntimeV1Disabled {
        bundle: LoadedForecastAdjustmentRuntimeV1DisabledBundleNull,
        reasonCode: reason_code,
        state: LoadedForecastAdjustmentRuntimeV1DisabledStateLiteral,
    })
}
