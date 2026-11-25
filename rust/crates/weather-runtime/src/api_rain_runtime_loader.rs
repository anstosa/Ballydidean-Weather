//! Load the root-installed rain startup runtime once without a JavaScript adapter.

use weather_api::{WeatherApiStartupLoadError, WeatherApiStartupRuntimeLoader};
use weather_contracts::{
    FieldPresence, LoadedForecastAdjustmentRainRuntimeRegistryV1,
    LoadedForecastAdjustmentRainRuntimeRegistryV1Active,
    LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveArtifactSha256Literal,
    LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveReasonCodeNull,
    LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveStateLiteral,
    LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledArtifactSha256Null,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledStateLiteral,
    LoadedForecastAdjustmentRuntimeV1, LoadedForecastAdjustmentTemperatureCanaryRuntime,
    LoadedForecastAdjustmentWindCanaryRuntime,
};
use weather_worker::RainAdjustmentRuntimeSelection;

use crate::worker_rain_serving_registry::admit_rain_serving_bytes;
use crate::{
    ApiMaintenanceCandidateCodec, NativeForecastRuntimeFiles, WorkerRainServingFiles,
    WorkerRainServingValues,
};

/// Load the rain runtime selected by its fixed root-installed registry.
pub fn load_installed_api_rain_runtime(
    files: &NativeForecastRuntimeFiles,
    qualified_files: &mut impl WorkerRainServingFiles,
    codec: &mut impl ApiMaintenanceCandidateCodec,
    values: &impl WorkerRainServingValues,
) -> LoadedForecastAdjustmentRainRuntimeRegistryV1 {
    let registry_bytes = match files.read_rain_registry() {
        Ok(bytes) => bytes,
        Err(_) => return disabled_registry_invalid(),
    };
    let Some(admitted) = admit_rain_serving_bytes(&registry_bytes, qualified_files, codec, values)
    else {
        return disabled_registry_invalid();
    };
    match admitted.selection {
        RainAdjustmentRuntimeSelection::Active => {
            LoadedForecastAdjustmentRainRuntimeRegistryV1::Active(
                LoadedForecastAdjustmentRainRuntimeRegistryV1Active {
                    artifactSha256:
                        LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveArtifactSha256Literal,
                    comparatorAuthority: admitted.authority,
                    reasonCode: LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveReasonCodeNull,
                    state: LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveStateLiteral,
                },
            )
        }
        RainAdjustmentRuntimeSelection::DisabledPolicyRawWithComparatorAuthority => {
            LoadedForecastAdjustmentRainRuntimeRegistryV1::Disabled(
                LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled {
                    artifactSha256:
                        LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledArtifactSha256Null,
                    comparatorAuthority: FieldPresence::Value(admitted.authority),
                    reasonCode:
                        LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::PolicyRaw,
                    state: LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledStateLiteral,
                },
            )
        }
        RainAdjustmentRuntimeSelection::Disabled | RainAdjustmentRuntimeSelection::Unconfigured => {
            disabled_registry_invalid()
        }
    }
}

/// Own rain selection while delegating the wind, standard, and temperature startup ports.
pub struct NativeRainApiStartupRuntimeLoader<QualifiedFiles, Codec, Values, Other> {
    files: NativeForecastRuntimeFiles,
    qualified_files: QualifiedFiles,
    codec: Codec,
    values: Values,
    other: Other,
}

impl<QualifiedFiles, Codec, Values, Other>
    NativeRainApiStartupRuntimeLoader<QualifiedFiles, Codec, Values, Other>
{
    /// Retain the fixed root, qualified readers, value effects, and independent loaders.
    pub const fn new(
        files: NativeForecastRuntimeFiles,
        qualified_files: QualifiedFiles,
        codec: Codec,
        values: Values,
        other: Other,
    ) -> Self {
        Self {
            files,
            qualified_files,
            codec,
            values,
            other,
        }
    }

    /// Return the delegated loader after startup or failure inspection.
    pub fn into_other(self) -> Other {
        self.other
    }
}

impl<QualifiedFiles, Codec, Values, Other> WeatherApiStartupRuntimeLoader
    for NativeRainApiStartupRuntimeLoader<QualifiedFiles, Codec, Values, Other>
where
    QualifiedFiles: WorkerRainServingFiles,
    Codec: ApiMaintenanceCandidateCodec,
    Values: WorkerRainServingValues,
    Other: WeatherApiStartupRuntimeLoader,
{
    /// Delegate the independently installed wind-canary selection.
    fn load_wind_canary(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError> {
        self.other.load_wind_canary()
    }

    /// Delegate the independently installed standard fallback.
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

    /// Load rain from the fixed root through the shared serving admission policy.
    fn load_rain(
        &mut self,
    ) -> Result<LoadedForecastAdjustmentRainRuntimeRegistryV1, WeatherApiStartupLoadError> {
        Ok(load_installed_api_rain_runtime(
            &self.files,
            &mut self.qualified_files,
            &mut self.codec,
            &self.values,
        ))
    }
}

/// Construct one invalid disabled selection without preserving partial authority.
fn disabled_registry_invalid() -> LoadedForecastAdjustmentRainRuntimeRegistryV1 {
    LoadedForecastAdjustmentRainRuntimeRegistryV1::Disabled(
        LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled {
            artifactSha256: LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledArtifactSha256Null,
            comparatorAuthority: FieldPresence::Missing,
            reasonCode:
                LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::RegistryInvalid,
            state: LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledStateLiteral,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use weather_contracts::{
        AdjustmentShadowFamily, ForecastAdjustmentMaintenanceRuntimePackage, JsonValue,
        MaintenanceShadowComparatorAuthorityKind, RainHurdleWindPortableArtifact,
    };
    use weather_database::Sha256Digest;

    use super::*;
    use crate::worker_rain_serving_registry::RETAINED_RAIN_ARTIFACT;
    use crate::{
        ApiMaintenanceCandidateError, ApiMaintenanceCandidateFiles, ApiMaintenanceCandidateMembers,
        ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
        MaintenanceCanonicalJson, MaintenanceReceiptScalar, WorkerRainServingMembers,
    };

    const ACTIVE_REGISTRY: &[u8] =
        include_bytes!("../../../../config/forecast-adjustments/ballydidean-rain-runtime.json");
    const ACTIVE_REGISTRY_SHA256: &str =
        "17d933d550f44e2664c1b64d6fa1ba9fde3fda85980433778a54e818ddca2db8";
    const RAW_REGISTRY: &[u8] = b"{\"activeArtifact\":null,\"contractVersion\":\"forecast-adjustment-rain-runtime-registry/v1\",\"rawReason\":\"policy_raw\",\"siteKey\":\"ballydidean\"}\n";
    const RAW_REGISTRY_SHA256: &str =
        "0bd90b7fdd9a0531bc9517c38093bdb59f53b84b58273a4cc8b4756b6683e888";
    const RETAINED_RAIN_MODEL_SHA256: &str =
        "dfba22520eb66944046f999e96ee51378913ef926a2da27f0562490d9fb7ef64";

    struct Codec;

    impl MaintenanceCanonicalJson for Codec {
        /// Decode only the two exact canonical functional fixtures.
        fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
            // preserve exact checked-in bytes for the active fixture
            if bytes == ACTIVE_REGISTRY {
                return Some(legacy_registry(
                    object(vec![("artifactSha256", string(RETAINED_RAIN_MODEL_SHA256))]),
                    JsonValue::Null,
                ));
            }
            // preserve exact pinned bytes for the policy-raw fixture
            if bytes == RAW_REGISTRY {
                return Some(legacy_registry(JsonValue::Null, string("policy_raw")));
            }
            None
        }

        /// Qualified receipt encoding is not reached by legacy fixtures.
        fn encode_canonical_json(&mut self, _: &JsonValue) -> Option<Vec<u8>> {
            unreachable!("legacy rain fixtures have no qualified receipt")
        }

        /// Qualified catalog hashing is not reached by legacy fixtures.
        fn sha256(&mut self, _: &[u8]) -> Option<Sha256Digest> {
            unreachable!("legacy rain fixtures have no qualified catalog")
        }
    }

    impl ApiMaintenanceCandidateCodec for Codec {
        /// Legacy rain fixtures have no maintenance timestamp.
        fn is_canonical_utc_instant(&mut self, _: &str) -> bool {
            unreachable!("legacy rain fixtures have no maintenance timestamp")
        }

        /// Legacy rain fixtures have no maintenance package.
        fn decode_maintenance_package(
            &mut self,
            _: &JsonValue,
        ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
            unreachable!("legacy rain fixtures have no maintenance package")
        }
    }

    struct Values;

    impl ApiMaintenanceReceiptCanonicalValues for Values {
        /// Rain registry loading performs no clock work.
        fn canonical_utc_epoch_milliseconds(&self, _: &str) -> Option<i64> {
            unreachable!("rain registry loading has no clock")
        }

        /// Return only the three frozen identities used by the legacy runtime.
        fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
            // distinguish each reviewed byte source without synthesizing another identity
            let digest = if bytes == ACTIVE_REGISTRY {
                ACTIVE_REGISTRY_SHA256
            } else if bytes == RAW_REGISTRY {
                RAW_REGISTRY_SHA256
            } else if bytes == RETAINED_RAIN_ARTIFACT {
                RETAINED_RAIN_MODEL_SHA256
            } else {
                return None;
            };
            Some(Sha256Digest::admitted(digest))
        }

        /// Rain registry loading derives no database receipt identity.
        fn unsigned_receipt_sha256(
            &self,
            _: &[(&str, MaintenanceReceiptScalar<'_>)],
        ) -> Option<Sha256Digest> {
            unreachable!("rain registry loading has no database receipt")
        }
    }

    impl ApiMaintenanceShadowEvidenceValues for Values {
        /// Registry parsing is owned by the candidate codec in this loader.
        fn decode_shadow_json(&self, _: &[u8]) -> Option<JsonValue> {
            unreachable!("rain registry parsing uses the candidate codec")
        }

        /// Legacy serving authority retains the original registry bytes.
        fn encode_shadow_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
            unreachable!("legacy serving authority retains original bytes")
        }

        /// Legacy registry loading has no inbound comparator payload.
        fn decode_shadow_comparator_base64(&self, _: &[u16]) -> Option<Vec<u8>> {
            unreachable!("legacy registry loading has no comparator input")
        }

        /// Legacy serving authority performs no sorted JSON serialization.
        fn encode_shadow_sorted_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
            unreachable!("legacy serving authority retains original bytes")
        }

        /// Legacy registry loading has no maintenance package.
        fn decode_shadow_maintenance_package(
            &self,
            _: &JsonValue,
        ) -> Option<ForecastAdjustmentMaintenanceRuntimePackage> {
            unreachable!("legacy registry loading has no maintenance package")
        }

        /// Legacy registry loading does not decode the compiled portable artifact.
        fn decode_shadow_rain_artifact(
            &self,
            _: &JsonValue,
        ) -> Option<RainHurdleWindPortableArtifact> {
            unreachable!("legacy registry loading trusts the compiled artifact")
        }

        /// General wind compatibility is outside the rain registry.
        fn check_legacy_wind_runtime_value(&self, _: &[u16]) -> Option<()> {
            unreachable!("rain registry loading has no general wind value")
        }
    }

    impl WorkerRainServingValues for Values {
        /// Retain a deterministic text witness for the fixture's exact UTF-8 bytes.
        fn encode_worker_rain_base64(&self, bytes: &[u8]) -> Option<String> {
            String::from_utf8(bytes.to_vec()).ok()
        }
    }

    struct NoQualifiedFiles;

    impl ApiMaintenanceCandidateFiles for NoQualifiedFiles {
        /// Legacy registry loading must not read the qualified catalog.
        fn read_catalog(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
            unreachable!("legacy registry loading has no qualified catalog")
        }

        /// Legacy registry loading must not read qualified members.
        fn read_members(
            &mut self,
            _: AdjustmentShadowFamily,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<ApiMaintenanceCandidateMembers, ApiMaintenanceCandidateError> {
            unreachable!("legacy registry loading has no qualified members")
        }
    }

    impl WorkerRainServingFiles for NoQualifiedFiles {
        /// The API fixed reader owns the first registry read.
        fn read_worker_rain_registry(&mut self) -> Result<Vec<u8>, ApiMaintenanceCandidateError> {
            unreachable!("api rain loading uses the native fixed reader")
        }

        /// Legacy registry loading must not read qualified serving members.
        fn read_worker_rain_serving_members(
            &mut self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<WorkerRainServingMembers, ApiMaintenanceCandidateError> {
            unreachable!("legacy registry loading has no qualified serving members")
        }
    }

    /// Load the checked-in active registry through the production native file reader.
    #[test]
    fn loads_checked_in_active_registry_with_legacy_authority() {
        let files = NativeForecastRuntimeFiles::new(checked_in_runtime_root()).unwrap();
        let loaded =
            load_installed_api_rain_runtime(&files, &mut NoQualifiedFiles, &mut Codec, &Values);
        let LoadedForecastAdjustmentRainRuntimeRegistryV1::Active(active) = loaded else {
            panic!("checked-in rain registry was not active");
        };
        assert_eq!(
            active.comparatorAuthority.authorityKind,
            MaintenanceShadowComparatorAuthorityKind::LegacyActive
        );
        assert_eq!(
            active.comparatorAuthority.receiptMemberSha256,
            ACTIVE_REGISTRY_SHA256
        );
    }

    /// Preserve the exact raw registry receipt while disabling rain inference.
    #[test]
    fn loads_exact_policy_raw_registry_with_comparator_authority() {
        let root = TempRainRoot::new(RAW_REGISTRY);
        let files = NativeForecastRuntimeFiles::new(root.path.clone()).unwrap();
        let loaded =
            load_installed_api_rain_runtime(&files, &mut NoQualifiedFiles, &mut Codec, &Values);
        let LoadedForecastAdjustmentRainRuntimeRegistryV1::Disabled(disabled) = loaded else {
            panic!("policy-raw rain registry was not disabled");
        };
        assert_eq!(
            disabled.reasonCode,
            LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::PolicyRaw
        );
        let FieldPresence::Value(authority) = disabled.comparatorAuthority else {
            panic!("policy-raw rain registry lost comparator authority");
        };
        assert_eq!(
            authority.authorityKind,
            MaintenanceShadowComparatorAuthorityKind::PolicyRaw
        );
        assert_eq!(authority.receiptMemberSha256, RAW_REGISTRY_SHA256);
    }

    /// Resolve the repository's checked-in forecast runtime root.
    fn checked_in_runtime_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../config/forecast-adjustments")
            .canonicalize()
            .unwrap()
    }

    struct TempRainRoot {
        path: PathBuf,
    }

    impl TempRainRoot {
        /// Create one isolated fixed runtime root containing the provided registry bytes.
        fn new(registry: &[u8]) -> Self {
            static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "weather-api-rain-runtime-{}-{}",
                std::process::id(),
                NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            fs::write(path.join("ballydidean-rain-runtime.json"), registry).unwrap();
            Self { path }
        }
    }

    impl Drop for TempRainRoot {
        /// Remove the bounded functional fixture after its native read completes.
        fn drop(&mut self) {
            fs::remove_dir_all(&self.path).unwrap();
        }
    }

    /// Build one exact legacy registry with source-order fields.
    fn legacy_registry(active_artifact: JsonValue, raw_reason: JsonValue) -> JsonValue {
        object(vec![
            ("activeArtifact", active_artifact),
            (
                "contractVersion",
                string("forecast-adjustment-rain-runtime-registry/v1"),
            ),
            ("rawReason", raw_reason),
            ("siteKey", string("ballydidean")),
        ])
    }

    /// Build one dependency-free ordered JSON object.
    fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
        JsonValue::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.encode_utf16().collect(), value))
                .collect(),
        )
    }

    /// Build one dependency-free JSON string.
    fn string(value: &str) -> JsonValue {
        JsonValue::String(value.encode_utf16().collect())
    }
}
