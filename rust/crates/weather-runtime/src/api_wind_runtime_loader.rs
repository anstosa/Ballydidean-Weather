//! Load the root-installed wind startup runtime once without a JavaScript adapter.

use std::io::ErrorKind;

use weather_api::{WeatherApiStartupLoadError, WeatherApiStartupRuntimeLoader};
use weather_contracts::{
    AdjustmentShadowFamily, FieldPresence, ForecastAdjustmentMaintenanceRuntimePackage,
    ForecastAdjustmentWindCanaryRegistryV1ContractVersionLiteral,
    ForecastAdjustmentWindCanaryRegistryV2ContractVersionLiteral,
    ForecastAdjustmentWindCanaryRegistryV2EnabledMetricBandsTuple,
    ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral,
    ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral, JsonValue,
    LoadedForecastAdjustmentRainRuntimeRegistryV1, LoadedForecastAdjustmentRuntimeV1,
    LoadedForecastAdjustmentTemperatureCanaryRuntime, LoadedForecastAdjustmentWindCanaryRuntime,
    LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceDisabled,
    LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceDisabledBundleNull,
    LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceDisabledReasonCodeLiteral,
    LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceDisabledStateLiteral,
    LoadedForecastAdjustmentWindCanaryRuntimeV1, LoadedForecastAdjustmentWindCanaryRuntimeV1Active,
    LoadedForecastAdjustmentWindCanaryRuntimeV1ActiveReasonCodeNull,
    LoadedForecastAdjustmentWindCanaryRuntimeV1ActiveStateLiteral,
    LoadedForecastAdjustmentWindCanaryRuntimeV1Disabled,
    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledBundleNull,
    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledStateLiteral,
    MaintenanceShadowComparatorAuthorityKind, MaintenanceShadowServingAuthority, Nullable,
};

use crate::api_maintenance_candidate_package::admit_projected_qualified_canary_package;
use crate::api_shadow_legacy_wind_canary::verify_legacy_wind_canary_bundle;
use crate::api_shadow_wire_validation::{instant, ordered_keys};
use crate::api_standard_runtime_loader::canonical_document;
use crate::api_wind_runtime_projection::{
    project_verified_wind_bundle, wind_runtime_projection_matches,
};
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{ApiStandardRuntimeValues, NativeForecastRuntimeFiles};

const LEGACY_REGISTRY_KEYS: &[&str] = &["activeBundle", "contractVersion"];
const LEGACY_ACTIVE_KEYS: &[&str] = &[
    "authorizationSha256",
    "bundleSha256",
    "candidateArtifactSha256",
    "path",
    "transferReportSha256",
];
const MAINTENANCE_REGISTRY_KEYS: &[&str] =
    &["activePackage", "contractVersion", "rawReason", "siteKey"];
const MAINTENANCE_ACTIVE_KEYS: &[&str] =
    &["actionSha256", "artifactSha256", "candidateSha256", "path"];
const RAW_REGISTRY_KEYS: &[&str] = &[
    "activeBundle",
    "contractVersion",
    "enabledMetricBands",
    "rawReason",
    "siteKey",
];
const QUALIFIED_RECEIPT_KEYS: &[&str] = &[
    "actionKind",
    "actionSha256",
    "bundleSha256",
    "candidateGraphSha256",
    "candidateSha256",
    "contractVersion",
    "controlPlaneSha256",
    "deployedCommit",
    "deployedImageDigest",
    "deployedRelease",
    "deployedSettingsSha256",
    "fencingToken",
    "fullMemberRootSha256",
    "installedAt",
    "lifecycleHeadSha256",
    "policyDecision",
    "policyReportSha256",
    "registrationSha256",
    "sourceSha256",
];
const WIND_MAINTENANCE_REGISTRY: &str = "forecast-adjustment-wind-maintenance-registry/v1";
const RAW_REGISTRY_SHA256: &str =
    "fa9856f136b731427ddedf2be40410142912198b8eabc3222735a0563c6964b9";
const RAW_REGISTRY_BYTES: usize = 763;

/// Load the wind runtime selected by its fixed root-installed registry.
pub fn load_installed_wind_runtime(
    files: &NativeForecastRuntimeFiles,
    values: &impl ApiStandardRuntimeValues,
    kill_switch: Option<&str>,
    now: &mut dyn FnMut() -> Option<String>,
    qualified: &mut dyn FnMut() -> Result<
        LoadedForecastAdjustmentWindCanaryRuntime,
        WeatherApiStartupLoadError,
    >,
) -> LoadedForecastAdjustmentWindCanaryRuntime {
    // preserve the literal one-way disable before any filesystem effect
    if kill_switch == Some("1") {
        return legacy_disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled,
        );
    }
    let registry_bytes = match files.read_wind_registry() {
        Ok(bytes) => bytes,
        Err(_) => {
            return legacy_disabled(
                LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
            );
        }
    };
    let Some(registry) = canonical_document(values, &registry_bytes) else {
        return legacy_disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
        );
    };
    let Some(selection) = select_registry(values, &registry, &registry_bytes) else {
        return legacy_disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
        );
    };

    match selection {
        WindRegistrySelection::LegacyInactive => legacy_disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
        ),
        WindRegistrySelection::PolicyRaw => policy_raw(values, &registry_bytes).unwrap_or_else(|| {
            legacy_disabled(
                LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
            )
        }),
        WindRegistrySelection::Qualified(selection) => qualified()
            .ok()
            .and_then(|loaded| bind_qualified(values, &selection, loaded))
            .unwrap_or_else(|| {
                legacy_disabled(
                    LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
                )
            }),
        WindRegistrySelection::LegacyActive(selection) => {
            load_legacy(files, values, selection, now)
        }
    }
}

/// Own wind selection while delegating the standard, temperature, and rain startup ports.
pub struct NativeWindApiStartupRuntimeLoader<Values, Other, Clock> {
    files: NativeForecastRuntimeFiles,
    values: Values,
    other: Other,
    kill_switch: Option<String>,
    clock: Clock,
}

impl<Values, Other, Clock> NativeWindApiStartupRuntimeLoader<Values, Other, Clock> {
    /// Retain the installed root, value effects, remaining loaders, switch, and delayed clock.
    pub const fn new(
        files: NativeForecastRuntimeFiles,
        values: Values,
        other: Other,
        kill_switch: Option<String>,
        clock: Clock,
    ) -> Self {
        Self {
            files,
            values,
            other,
            kill_switch,
            clock,
        }
    }

    /// Return the delegated loader after startup or failure inspection.
    pub fn into_other(self) -> Other {
        self.other
    }
}

impl<Values, Other, Clock> WeatherApiStartupRuntimeLoader
    for NativeWindApiStartupRuntimeLoader<Values, Other, Clock>
where
    Values: ApiStandardRuntimeValues,
    Other: WeatherApiStartupRuntimeLoader,
    Clock: FnMut() -> Option<String>,
{
    /// Load wind from the fixed root and use the delegate only for qualified maintenance.
    fn load_wind_canary(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError> {
        let Self {
            files,
            values,
            other,
            kill_switch,
            clock,
        } = self;
        let mut qualified = || other.load_wind_canary();
        Ok(load_installed_wind_runtime(
            files,
            values,
            kill_switch.as_deref(),
            clock,
            &mut qualified,
        ))
    }

    /// Delegate the independently selected standard fallback.
    fn load_qualified_wind(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRuntimeV1, WeatherApiStartupLoadError> {
        self.other.load_qualified_wind()
    }

    /// Delegate the independent temperature selection.
    fn load_temperature(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentTemperatureCanaryRuntime, WeatherApiStartupLoadError> {
        self.other.load_temperature()
    }

    /// Delegate the independent rain selection.
    fn load_rain(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRainRuntimeRegistryV1, WeatherApiStartupLoadError> {
        self.other.load_rain()
    }
}

/// One exact legacy registry selection.
struct LegacySelection {
    authorization_sha256: String,
    bundle_sha256: String,
    candidate_artifact_sha256: String,
    transfer_report_sha256: String,
}

/// One exact active maintenance registry selection.
struct QualifiedSelection {
    action_sha256: String,
    artifact_sha256: String,
    candidate_sha256: String,
}

/// Closed wind registry branches before any selected bundle read.
enum WindRegistrySelection {
    LegacyInactive,
    LegacyActive(LegacySelection),
    PolicyRaw,
    Qualified(QualifiedSelection),
}

/// Classify one canonical registry while preserving every original branch guard.
fn select_registry(
    values: &impl ApiStandardRuntimeValues,
    registry: &JsonValue,
    bytes: &[u8],
) -> Option<WindRegistrySelection> {
    let version = text(registry, "contractVersion")?;
    // isolate maintenance selectors before recognizing either legacy registry generation
    if version == WIND_MAINTENANCE_REGISTRY {
        return select_maintenance_registry(registry);
    }
    // reject a maintenance registry belonging to another family
    if field(registry, "activePackage").is_some() {
        return None;
    }
    // preserve only the exact frozen raw-v2 bytes and mask
    if version == ForecastAdjustmentWindCanaryRegistryV2ContractVersionLiteral::WIRE_VALUE {
        validate_raw_registry(values, registry, bytes)?;
        return Some(WindRegistrySelection::PolicyRaw);
    }
    (version == ForecastAdjustmentWindCanaryRegistryV1ContractVersionLiteral::WIRE_VALUE)
        .then_some(())?;
    exact_keys(registry, LEGACY_REGISTRY_KEYS).then_some(())?;
    let active = field(registry, "activeBundle")?;
    // preserve the reviewed inactive default without bundle I/O or a clock read
    if matches!(active, JsonValue::Null) {
        return Some(WindRegistrySelection::LegacyInactive);
    }
    exact_keys(active, LEGACY_ACTIVE_KEYS).then_some(())?;
    let selection = LegacySelection {
        authorization_sha256: text(active, "authorizationSha256")?,
        bundle_sha256: text(active, "bundleSha256")?,
        candidate_artifact_sha256: text(active, "candidateArtifactSha256")?,
        transfer_report_sha256: text(active, "transferReportSha256")?,
    };
    // require each address before deriving its single permitted relative filename
    for digest in [
        &selection.authorization_sha256,
        &selection.bundle_sha256,
        &selection.candidate_artifact_sha256,
        &selection.transfer_report_sha256,
    ] {
        valid_hash(digest).then_some(())?;
    }
    (text(active, "path")?
        == format!(
            "wind-canary-bundles/sha256-{}.json",
            selection.bundle_sha256
        ))
    .then_some(())?;
    Some(WindRegistrySelection::LegacyActive(selection))
}

/// Select either raw policy or one closed qualified maintenance identity.
fn select_maintenance_registry(registry: &JsonValue) -> Option<WindRegistrySelection> {
    let active = field(registry, "activePackage")?;
    // raw maintenance is order-sensitive because these exact bytes are its receipt
    if matches!(active, JsonValue::Null) {
        ordered_keys(registry, MAINTENANCE_REGISTRY_KEYS)?;
        (text(registry, "rawReason")? == "policy_raw"
            && text(registry, "siteKey")? == "ballydidean")
            .then_some(())?;
        return Some(WindRegistrySelection::PolicyRaw);
    }
    exact_keys(registry, MAINTENANCE_REGISTRY_KEYS).then_some(())?;
    (matches!(field(registry, "rawReason"), Some(JsonValue::Null))
        && text(registry, "siteKey")? == "ballydidean")
        .then_some(())?;
    exact_keys(active, MAINTENANCE_ACTIVE_KEYS).then_some(())?;
    let selection = QualifiedSelection {
        action_sha256: text(active, "actionSha256")?,
        artifact_sha256: text(active, "artifactSha256")?,
        candidate_sha256: text(active, "candidateSha256")?,
    };
    // reject malformed selectors before invoking independent qualification
    for digest in [
        &selection.action_sha256,
        &selection.artifact_sha256,
        &selection.candidate_sha256,
    ] {
        valid_hash(digest).then_some(())?;
    }
    (text(active, "path")?
        == format!(
            "wind-canary-bundles/sha256-{}.json",
            selection.artifact_sha256
        ))
    .then_some(())?;
    Some(WindRegistrySelection::Qualified(selection))
}

/// Admit the immutable raw-v2 registry, including its exact ordered metric mask.
fn validate_raw_registry(
    values: &impl ApiStandardRuntimeValues,
    registry: &JsonValue,
    bytes: &[u8],
) -> Option<()> {
    ordered_keys(registry, RAW_REGISTRY_KEYS)?;
    (matches!(field(registry, "activeBundle"), Some(JsonValue::Null))
        && text(registry, "rawReason")? == "policy_raw"
        && text(registry, "siteKey")? == "ballydidean"
        && bytes.len() == RAW_REGISTRY_BYTES
        && values.sha256(bytes)?.as_str() == RAW_REGISTRY_SHA256)
        .then_some(())?;
    let JsonValue::Array(bands) = field(registry, "enabledMetricBands")? else {
        return None;
    };
    (bands.len()
        == ForecastAdjustmentWindCanaryRegistryV2EnabledMetricBandsTuple::WIRE_VALUES.len())
    .then_some(())?;
    // retain both tuple order and each tuple object's field order
    for (band, (lead_band, metric)) in bands
        .iter()
        .zip(ForecastAdjustmentWindCanaryRegistryV2EnabledMetricBandsTuple::WIRE_VALUES)
    {
        ordered_keys(band, &["leadBand", "metric"])?;
        (text(band, "leadBand")? == *lead_band && text(band, "metric")? == *metric).then_some(())?;
    }
    Some(())
}

/// Bind a delegated qualified runtime back to this exact maintenance registry selector.
fn bind_qualified(
    values: &impl ApiStandardRuntimeValues,
    selection: &QualifiedSelection,
    loaded: LoadedForecastAdjustmentWindCanaryRuntime,
) -> Option<LoadedForecastAdjustmentWindCanaryRuntime> {
    let LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceActive(active) = loaded else {
        return None;
    };
    let authority = &active.comparatorAuthority;
    // require the delegated runtime's typed projection to name this action and artifact
    if authority.authorityKind != MaintenanceShadowComparatorAuthorityKind::MaintenanceQualified
        || active.bundle.bundleSha256 != selection.artifact_sha256
        || active.bundle.maintenanceBundleSha256 != selection.artifact_sha256
        || active.bundle.maintenanceAuthority.actionSha256 != selection.action_sha256
        || !matches!(
            &authority.artifactIdentitySha256,
            Nullable::Value(identity) if identity == &selection.artifact_sha256
        )
    {
        return None;
    }

    let receipt_bytes = decode_base64(values, &authority.receiptBase64)?;
    member_hash_matches(values, &receipt_bytes, &authority.receiptMemberSha256)?;
    let receipt = canonical_document(values, &receipt_bytes)?;
    exact_keys(&receipt, QUALIFIED_RECEIPT_KEYS).then_some(())?;
    // recover and bind the candidate identity that is intentionally absent from the loaded shape
    if text(&receipt, "contractVersion")? != "adjustment-installed-candidate-receipt/v2"
        || text(&receipt, "actionKind")? != "promote"
        || text(&receipt, "policyDecision")? != "qualified"
        || text(&receipt, "actionSha256")? != selection.action_sha256
        || text(&receipt, "bundleSha256")? != selection.artifact_sha256
        || text(&receipt, "candidateSha256")? != selection.candidate_sha256
        || text(&receipt, "fullMemberRootSha256")?
            != active.bundle.maintenanceAuthority.fullMemberRootSha256
        || text(&receipt, "policyReportSha256")?
            != active.bundle.maintenanceAuthority.policyReportSha256
    {
        return None;
    }

    let Nullable::Value(artifact_base64) = &authority.artifactBase64 else {
        return None;
    };
    let Nullable::Value(artifact_member_sha256) = &authority.artifactMemberSha256 else {
        return None;
    };
    let artifact_bytes = decode_base64(values, artifact_base64)?;
    member_hash_matches(values, &artifact_bytes, artifact_member_sha256)?;
    let artifact = canonical_document(values, &artifact_bytes)?;
    let ForecastAdjustmentMaintenanceRuntimePackage::Wind(package) =
        admit_projected_qualified_canary_package(
            values,
            &artifact,
            &selection.artifact_sha256,
            AdjustmentShadowFamily::Wind,
        )?
    else {
        return None;
    };
    // keep the generated result tied to the independently verified package projection
    if package.bundleSha256 != selection.artifact_sha256
        || package.candidateSha256 != selection.candidate_sha256
        || package.candidate != active.bundle.candidate
        || !values.standard_calendar_matches(
            &active.bundle.candidate.runtimeFingerprint.icuVersion,
            &active.bundle.candidate.runtimeFingerprint.tzdataVersion,
        )
    {
        return None;
    }
    Some(LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceActive(active))
}

/// Read and admit one addressed legacy canary bundle.
fn load_legacy(
    files: &NativeForecastRuntimeFiles,
    values: &impl ApiStandardRuntimeValues,
    selection: LegacySelection,
    now: &mut dyn FnMut() -> Option<String>,
) -> LoadedForecastAdjustmentWindCanaryRuntime {
    let bundle_bytes = match files.read_wind_bundle(&selection.bundle_sha256) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return legacy_disabled(
                LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::BundleMissing,
            );
        }
        Err(_) => {
            return legacy_disabled(
                LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid,
            );
        }
    };
    match load_legacy_bundle(values, &selection, &bundle_bytes, now) {
        Some(LegacyBundleLoad {
            loaded: Some(loaded),
        }) => loaded,
        Some(LegacyBundleLoad { loaded: None }) => legacy_disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::CanaryExpired,
        ),
        None => legacy_disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid,
        ),
    }
}

/// Distinguish a valid elapsed authorization from malformed selected material.
struct LegacyBundleLoad {
    loaded: Option<LoadedForecastAdjustmentWindCanaryRuntime>,
}

/// Verify the complete legacy graph before decoding or allocating its serving projection.
fn load_legacy_bundle(
    values: &impl ApiStandardRuntimeValues,
    selection: &LegacySelection,
    bundle_bytes: &[u8],
    now: &mut dyn FnMut() -> Option<String>,
) -> Option<LegacyBundleLoad> {
    let document = canonical_document(values, bundle_bytes)?;
    verify_legacy_wind_canary_bundle(values, &document, &selection.bundle_sha256)?;
    let authorization = field(&document, "authorization")?;
    let candidate = field(&document, "candidate")?;
    let transfer_report = field(&document, "transferReport")?;
    // cross-bind all four registry addresses before consulting the wall clock
    if text(&document, "bundleSha256")? != selection.bundle_sha256
        || text(authorization, "authorizationSha256")? != selection.authorization_sha256
        || text(candidate, "candidateArtifactSha256")? != selection.candidate_artifact_sha256
        || text(transfer_report, "transferReportSha256")? != selection.transfer_report_sha256
    {
        return None;
    }
    let fingerprint = field(candidate, "runtimeFingerprint")?;
    values
        .standard_calendar_matches(
            &text(fingerprint, "icuVersion")?,
            &text(fingerprint, "tzdataVersion")?,
        )
        .then_some(())?;

    let now = now()?;
    let now_epoch = exact_instant(values, &now)?;
    let activated_at = instant(values, authorization, "activatedAt")?;
    let version = text(&document, "contractVersion")?;
    // preserve the finite half-open window while permanent authority has no expiry
    let active = if version
        == ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral::WIRE_VALUE
    {
        now_epoch >= activated_at && now_epoch < instant(values, authorization, "expiresAt")?
    } else if version
        == ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral::WIRE_VALUE
    {
        now_epoch >= activated_at
    } else {
        return None;
    };
    // a well-formed elapsed authority is a distinct disabled result, not an invalid bundle
    if !active {
        return Some(LegacyBundleLoad { loaded: None });
    }

    let bundle = project_verified_wind_bundle(&document)?;
    wind_runtime_projection_matches(&bundle, &document).then_some(())?;
    let receipt_bytes = values.encode_shadow_sorted_json(authorization)?;
    let artifact_member = values.sha256(bundle_bytes)?;
    let receipt_member = values.sha256(&receipt_bytes)?;
    valid_hash(artifact_member.as_str()).then_some(())?;
    valid_hash(receipt_member.as_str()).then_some(())?;
    let authority = MaintenanceShadowServingAuthority {
        artifactBase64: Nullable::Value(values.encode_standard_base64(bundle_bytes)?),
        artifactIdentitySha256: Nullable::Value(selection.bundle_sha256.clone()),
        artifactMemberSha256: Nullable::Value(artifact_member.as_str().to_owned()),
        authorityKind: MaintenanceShadowComparatorAuthorityKind::LegacyActive,
        receiptBase64: values.encode_standard_base64(&receipt_bytes)?,
        receiptMemberSha256: receipt_member.as_str().to_owned(),
    };
    Some(LegacyBundleLoad {
        loaded: Some(LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
            value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Active(
                LoadedForecastAdjustmentWindCanaryRuntimeV1Active {
                    bundle,
                    reasonCode: LoadedForecastAdjustmentWindCanaryRuntimeV1ActiveReasonCodeNull,
                    state: LoadedForecastAdjustmentWindCanaryRuntimeV1ActiveStateLiteral,
                },
            ),
            comparatorAuthority: FieldPresence::Value(authority),
        }),
    })
}

/// Create one exact policy-raw result whose receipt is the admitted registry bytes.
fn policy_raw(
    values: &impl ApiStandardRuntimeValues,
    registry_bytes: &[u8],
) -> Option<LoadedForecastAdjustmentWindCanaryRuntime> {
    let receipt_member = values.sha256(registry_bytes)?;
    valid_hash(receipt_member.as_str()).then_some(())?;
    Some(
        LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceDisabled(
            LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceDisabled {
                bundle: LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceDisabledBundleNull,
                comparatorAuthority: MaintenanceShadowServingAuthority {
                    artifactBase64: Nullable::Null,
                    artifactIdentitySha256: Nullable::Null,
                    artifactMemberSha256: Nullable::Null,
                    authorityKind: MaintenanceShadowComparatorAuthorityKind::PolicyRaw,
                    receiptBase64: values.encode_standard_base64(registry_bytes)?,
                    receiptMemberSha256: receipt_member.as_str().to_owned(),
                },
                reasonCode:
                    LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceDisabledReasonCodeLiteral,
                state: LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceDisabledStateLiteral,
            },
        ),
    )
}

/// Decode one comparator member using the retained canonical padded-base64 effect.
fn decode_base64(values: &impl ApiStandardRuntimeValues, encoded: &str) -> Option<Vec<u8>> {
    let units: Vec<u16> = encoded.encode_utf16().collect();
    values.decode_shadow_comparator_base64(&units)
}

/// Require one embedded comparator member to retain its declared raw-byte digest.
fn member_hash_matches(
    values: &impl ApiStandardRuntimeValues,
    bytes: &[u8],
    expected: &str,
) -> Option<()> {
    (valid_hash(expected) && values.sha256(bytes)?.as_str() == expected).then_some(())
}

/// Parse one exact four-digit canonical UTC millisecond instant.
fn exact_instant(values: &impl ApiStandardRuntimeValues, value: &str) -> Option<i64> {
    (value.len() == 24 && value.as_bytes()[..4].iter().all(u8::is_ascii_digit)).then_some(())?;
    values.canonical_utc_epoch_milliseconds(value)
}

/// Construct one closed legacy disabled selection without artifact detail.
fn legacy_disabled(
    reason_code: LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum,
) -> LoadedForecastAdjustmentWindCanaryRuntime {
    LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
        value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Disabled(
            LoadedForecastAdjustmentWindCanaryRuntimeV1Disabled {
                bundle: LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledBundleNull,
                reasonCode: reason_code,
                state: LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledStateLiteral,
            },
        ),
        comparatorAuthority: FieldPresence::Missing,
    }
}
