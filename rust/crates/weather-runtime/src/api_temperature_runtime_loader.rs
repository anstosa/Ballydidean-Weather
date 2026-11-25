//! Load the root-installed temperature startup runtime without a JavaScript adapter.

use std::io::ErrorKind;

use weather_api::{WeatherApiStartupLoadError, WeatherApiStartupRuntimeLoader};
use weather_contracts::{
    AdjustmentShadowFamily, FieldPresence, ForecastAdjustmentMaintenanceRuntimePackage,
    ForecastAdjustmentTemperatureCanaryRegistryV1ContractVersionLiteral,
    ForecastAdjustmentTemperatureCanaryRegistryV2ContractVersionLiteral,
    ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ContractVersionLiteral,
    ForecastAdjustmentTemperatureCanaryRuntimeBundleV2ContractVersionLiteral, JsonValue,
    LoadedForecastAdjustmentRainRuntimeRegistryV1, LoadedForecastAdjustmentRuntimeV1,
    LoadedForecastAdjustmentTemperatureCanaryRuntime,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceDisabled,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceDisabledBundleNull,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceDisabledReasonCodeLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceDisabledStateLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Active,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1ActiveReasonCodeNull,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1ActiveStateLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledBundleNull,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledStateLiteral,
    LoadedForecastAdjustmentWindCanaryRuntime, MaintenanceShadowComparatorAuthorityKind,
    MaintenanceShadowServingAuthority, Nullable,
};

use crate::NativeForecastRuntimeFiles;
use crate::api_maintenance_candidate_package::admit_projected_qualified_canary_package;
use crate::api_shadow_legacy_temperature::verify_legacy_temperature_bundle;
use crate::api_shadow_wire_validation::{instant, ordered_keys};
use crate::api_standard_runtime_loader::{ApiStandardRuntimeValues, canonical_document};
use crate::api_temperature_runtime_projection::project_verified_temperature_bundle;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};

const LEGACY_REGISTRY_KEYS: &[&str] = &["activeBundle", "contractVersion"];
const LEGACY_ACTIVE_KEYS: &[&str] = &[
    "authorizationSha256",
    "bundleSha256",
    "modelSourceSha256",
    "path",
    "strengthSourceSha256",
];
const MAINTENANCE_REGISTRY_KEYS: &[&str] =
    &["activePackage", "contractVersion", "rawReason", "siteKey"];
const MAINTENANCE_ACTIVE_KEYS: &[&str] =
    &["actionSha256", "artifactSha256", "candidateSha256", "path"];
const RAW_REGISTRY_KEYS: &[&str] = &["activeBundle", "contractVersion", "rawReason", "siteKey"];
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
const TEMPERATURE_MAINTENANCE_REGISTRY: &str =
    "forecast-adjustment-temperature-maintenance-registry/v1";
const RAW_REGISTRY_SHA256: &str =
    "cee570ec382a99b25427523438bde556b6daed780e49ad14d1399d182ac14165";
const RAW_REGISTRY_BYTES: usize = 142;

/// Load the temperature runtime selected by its fixed root-installed registry.
pub fn load_installed_temperature_runtime(
    files: &NativeForecastRuntimeFiles,
    values: &impl ApiStandardRuntimeValues,
    kill_switch: Option<&str>,
    now: &mut dyn FnMut() -> Option<String>,
    qualified: &mut dyn FnMut() -> Result<
        LoadedForecastAdjustmentTemperatureCanaryRuntime,
        WeatherApiStartupLoadError,
    >,
) -> LoadedForecastAdjustmentTemperatureCanaryRuntime {
    // keep temperature default-closed before every filesystem or delegated effect
    if kill_switch != Some("0") {
        return legacy_disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::CanaryKilled,
        );
    }
    let registry_bytes = match files.read_temperature_registry() {
        Ok(bytes) => bytes,
        Err(_) => {
            return legacy_disabled(
                LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
            );
        }
    };
    let Some(registry) = canonical_document(values, &registry_bytes) else {
        return legacy_disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
        );
    };
    let Some(selection) = select_registry(values, &registry, &registry_bytes) else {
        return legacy_disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
        );
    };

    // preserve the four original mutually exclusive registry generations
    match selection {
        TemperatureRegistrySelection::LegacyInactive => legacy_disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
        ),
        TemperatureRegistrySelection::PolicyRaw => {
            policy_raw(values, &registry_bytes).unwrap_or_else(|| {
                legacy_disabled(
                    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
                )
            })
        }
        TemperatureRegistrySelection::Qualified(selection) => qualified()
            .ok()
            .and_then(|loaded| bind_qualified(values, &selection, loaded))
            .unwrap_or_else(|| {
                legacy_disabled(
                    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::RegistryInvalid,
                )
            }),
        TemperatureRegistrySelection::LegacyActive(selection) => {
            load_legacy(files, values, selection, now)
        }
    }
}

/// Own temperature selection while delegating wind, standard, and rain startup ports.
pub struct NativeTemperatureApiStartupRuntimeLoader<Values, Other, Clock> {
    files: NativeForecastRuntimeFiles,
    values: Values,
    other: Other,
    kill_switch: Option<String>,
    clock: Clock,
}

impl<Values, Other, Clock> NativeTemperatureApiStartupRuntimeLoader<Values, Other, Clock> {
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
    for NativeTemperatureApiStartupRuntimeLoader<Values, Other, Clock>
where
    Values: ApiStandardRuntimeValues,
    Other: WeatherApiStartupRuntimeLoader,
    Clock: FnMut() -> Option<String>,
{
    /// Delegate the independently selected wind canary.
    fn load_wind_canary(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError> {
        self.other.load_wind_canary()
    }

    /// Delegate the independently selected standard fallback.
    fn load_qualified_wind(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRuntimeV1, WeatherApiStartupLoadError> {
        self.other.load_qualified_wind()
    }

    /// Load temperature from the fixed root and delegate only qualified maintenance.
    fn load_temperature(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentTemperatureCanaryRuntime, WeatherApiStartupLoadError> {
        let Self {
            files,
            values,
            other,
            kill_switch,
            clock,
        } = self;
        let mut qualified = || other.load_temperature();
        Ok(load_installed_temperature_runtime(
            files,
            values,
            kill_switch.as_deref(),
            clock,
            &mut qualified,
        ))
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
    model_source_sha256: String,
    strength_source_sha256: String,
}

/// One exact active maintenance registry selection.
struct QualifiedSelection {
    action_sha256: String,
    artifact_sha256: String,
    candidate_sha256: String,
}

/// Closed temperature registry branches before any selected bundle read.
enum TemperatureRegistrySelection {
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
) -> Option<TemperatureRegistrySelection> {
    let version = text(registry, "contractVersion")?;
    // isolate the temperature maintenance selector before legacy generations
    if version == TEMPERATURE_MAINTENANCE_REGISTRY {
        return select_maintenance_registry(registry);
    }
    // reject maintenance registries owned by another adjustment family
    if field(registry, "activePackage").is_some() {
        return None;
    }
    // preserve only the exact frozen nullable raw-v2 registry bytes
    if version == ForecastAdjustmentTemperatureCanaryRegistryV2ContractVersionLiteral::WIRE_VALUE {
        validate_raw_registry(values, registry, bytes)?;
        return Some(TemperatureRegistrySelection::PolicyRaw);
    }
    (version == ForecastAdjustmentTemperatureCanaryRegistryV1ContractVersionLiteral::WIRE_VALUE)
        .then_some(())?;
    exact_keys(registry, LEGACY_REGISTRY_KEYS).then_some(())?;
    let active = field(registry, "activeBundle")?;
    // preserve the reviewed inactive registry without bundle or clock effects
    if matches!(active, JsonValue::Null) {
        return Some(TemperatureRegistrySelection::LegacyInactive);
    }
    exact_keys(active, LEGACY_ACTIVE_KEYS).then_some(())?;
    let selection = LegacySelection {
        authorization_sha256: text(active, "authorizationSha256")?,
        bundle_sha256: text(active, "bundleSha256")?,
        model_source_sha256: text(active, "modelSourceSha256")?,
        strength_source_sha256: text(active, "strengthSourceSha256")?,
    };
    // require all four selectors before deriving the only admitted path
    for digest in [
        &selection.authorization_sha256,
        &selection.bundle_sha256,
        &selection.model_source_sha256,
        &selection.strength_source_sha256,
    ] {
        valid_hash(digest).then_some(())?;
    }
    (text(active, "path")?
        == format!(
            "temperature-canary-bundles/sha256-{}.json",
            selection.bundle_sha256
        ))
    .then_some(())?;
    Some(TemperatureRegistrySelection::LegacyActive(selection))
}

/// Select either raw policy or one closed qualified maintenance identity.
fn select_maintenance_registry(registry: &JsonValue) -> Option<TemperatureRegistrySelection> {
    let active = field(registry, "activePackage")?;
    // keep the actionless maintenance receipt order-sensitive
    if matches!(active, JsonValue::Null) {
        ordered_keys(registry, MAINTENANCE_REGISTRY_KEYS)?;
        (text(registry, "rawReason")? == "policy_raw"
            && text(registry, "siteKey")? == "ballydidean")
            .then_some(())?;
        return Some(TemperatureRegistrySelection::PolicyRaw);
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
    // reject malformed selectors before independent qualification is invoked
    for digest in [
        &selection.action_sha256,
        &selection.artifact_sha256,
        &selection.candidate_sha256,
    ] {
        valid_hash(digest).then_some(())?;
    }
    (text(active, "path")?
        == format!(
            "temperature-canary-bundles/sha256-{}.json",
            selection.artifact_sha256
        ))
    .then_some(())?;
    Some(TemperatureRegistrySelection::Qualified(selection))
}

/// Admit the immutable raw-v2 temperature registry.
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
        .then_some(())
}

/// Bind a delegated qualified runtime to this exact maintenance selector.
fn bind_qualified(
    values: &impl ApiStandardRuntimeValues,
    selection: &QualifiedSelection,
    loaded: LoadedForecastAdjustmentTemperatureCanaryRuntime,
) -> Option<LoadedForecastAdjustmentTemperatureCanaryRuntime> {
    let LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceActive(active) = loaded else {
        return None;
    };
    let authority = &active.comparatorAuthority;
    // bind the public projection to the selected action and artifact
    if authority.authorityKind != MaintenanceShadowComparatorAuthorityKind::MaintenanceQualified
        || active.bundle.bundleSha256 != selection.artifact_sha256
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
    // recover the candidate identity absent from the loaded public projection
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
    let ForecastAdjustmentMaintenanceRuntimePackage::Temperature(package) =
        admit_projected_qualified_canary_package(
            values,
            &artifact,
            &selection.artifact_sha256,
            AdjustmentShadowFamily::Temperature,
        )?
    else {
        return None;
    };
    // retain the independent verified owner without adding legacy calendar activation
    if package.bundleSha256 != selection.artifact_sha256
        || package.candidateSha256 != selection.candidate_sha256
        || package.model != active.bundle.model
    {
        return None;
    }
    Some(LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceActive(active))
}

/// Read and admit one addressed legacy temperature bundle.
fn load_legacy(
    files: &NativeForecastRuntimeFiles,
    values: &impl ApiStandardRuntimeValues,
    selection: LegacySelection,
    now: &mut dyn FnMut() -> Option<String>,
) -> LoadedForecastAdjustmentTemperatureCanaryRuntime {
    let bundle_bytes = match files.read_temperature_bundle(&selection.bundle_sha256) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return legacy_disabled(
                LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::BundleMissing,
            );
        }
        Err(_) => {
            return legacy_disabled(
                LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid,
            );
        }
    };
    // distinguish an elapsed valid authorization from malformed selected material
    match load_legacy_bundle(values, &selection, &bundle_bytes, now) {
        Some(LegacyBundleLoad {
            loaded: Some(loaded),
        }) => loaded,
        Some(LegacyBundleLoad { loaded: None }) => legacy_disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::CanaryExpired,
        ),
        None => legacy_disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid,
        ),
    }
}

/// One verified bundle load, including a valid elapsed authorization.
struct LegacyBundleLoad {
    loaded: Option<LoadedForecastAdjustmentTemperatureCanaryRuntime>,
}

/// Verify the complete legacy graph before projecting its generated serving contract.
fn load_legacy_bundle(
    values: &impl ApiStandardRuntimeValues,
    selection: &LegacySelection,
    bundle_bytes: &[u8],
    now: &mut dyn FnMut() -> Option<String>,
) -> Option<LegacyBundleLoad> {
    let document = canonical_document(values, bundle_bytes)?;
    verify_legacy_temperature_bundle(values, &document, &selection.bundle_sha256)?;
    let authorization = field(&document, "authorization")?;
    let evidence = field(&document, "evidence")?;
    // cross-bind all four registry selectors before reading the clock
    if text(&document, "bundleSha256")? != selection.bundle_sha256
        || text(authorization, "authorizationSha256")? != selection.authorization_sha256
        || text(evidence, "modelSourceSha256")? != selection.model_source_sha256
        || text(evidence, "strengthSourceSha256")? != selection.strength_source_sha256
    {
        return None;
    }
    let fingerprint = field(&document, "runtimeFingerprint")?;
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
    // keep finite authorization half-open and permanent authorization unbounded
    let active = if version
        == ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ContractVersionLiteral::WIRE_VALUE
    {
        now_epoch >= activated_at && now_epoch < instant(values, authorization, "expiresAt")?
    } else if version
        == ForecastAdjustmentTemperatureCanaryRuntimeBundleV2ContractVersionLiteral::WIRE_VALUE
    {
        now_epoch >= activated_at
    } else {
        return None;
    };
    // report a valid elapsed authority as expired rather than invalid
    if !active {
        return Some(LegacyBundleLoad { loaded: None });
    }

    let bundle = project_verified_temperature_bundle(&document)?;
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
        loaded: Some(LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
            value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Active(
                LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Active {
                    bundle,
                    reasonCode:
                        LoadedForecastAdjustmentTemperatureCanaryRuntimeV1ActiveReasonCodeNull,
                    state: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1ActiveStateLiteral,
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
) -> Option<LoadedForecastAdjustmentTemperatureCanaryRuntime> {
    let receipt_member = values.sha256(registry_bytes)?;
    valid_hash(receipt_member.as_str()).then_some(())?;
    Some(
        LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceDisabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceDisabled {
                bundle:
                    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceDisabledBundleNull,
                comparatorAuthority: MaintenanceShadowServingAuthority {
                    artifactBase64: Nullable::Null,
                    artifactIdentitySha256: Nullable::Null,
                    artifactMemberSha256: Nullable::Null,
                    authorityKind: MaintenanceShadowComparatorAuthorityKind::PolicyRaw,
                    receiptBase64: values.encode_standard_base64(registry_bytes)?,
                    receiptMemberSha256: receipt_member.as_str().to_owned(),
                },
                reasonCode:
                    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceDisabledReasonCodeLiteral,
                state:
                    LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceDisabledStateLiteral,
            },
        ),
    )
}

/// Decode one comparator member through the retained canonical base64 effect.
fn decode_base64(values: &impl ApiStandardRuntimeValues, encoded: &str) -> Option<Vec<u8>> {
    let units: Vec<u16> = encoded.encode_utf16().collect();
    values.decode_shadow_comparator_base64(&units)
}

/// Require one embedded member to retain its declared raw-byte digest.
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
    reason_code: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum,
) -> LoadedForecastAdjustmentTemperatureCanaryRuntime {
    LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
        value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Disabled(
            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled {
                bundle: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledBundleNull,
                reasonCode: reason_code,
                state: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledStateLiteral,
            },
        ),
        comparatorAuthority: FieldPresence::Missing,
    }
}
