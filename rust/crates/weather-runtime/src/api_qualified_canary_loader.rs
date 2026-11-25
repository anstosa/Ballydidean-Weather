//! Load wind and temperature promotions from the independently installed catalog.

use weather_api::WeatherApiStartupLoadError;
use weather_contracts::{
    self as c, AdjustmentShadowFamily, ForecastAdjustmentMaintenanceRuntimePackage as Package,
    MaintenanceShadowComparatorAuthorityKind, MaintenanceShadowServingAuthority, Nullable,
};

use crate::{
    ApiMaintenanceCandidateCodec, ApiQualifiedCanaryFiles, ApiStandardRuntimeValues,
    InstalledQualifiedCanaryLoaders,
    api_maintenance_candidate_package::admit_qualified_api_canary_package,
    api_standard_runtime_projection::candidate_matches,
    api_temperature_runtime_projection::project_permanent_model,
    maintenance_canonical_json::{field, valid_hash},
    worker_rain_serving_catalog::{
        select_qualified_serving_catalog, validate_qualified_serving_members,
    },
};

/// Root-installed canary qualification owner for the native API startup port.
pub struct NativeInstalledQualifiedCanaryLoaders<Files, Codec, Values> {
    files: Files,
    codec: Codec,
    values: Values,
}

impl<Files, Codec, Values> NativeInstalledQualifiedCanaryLoaders<Files, Codec, Values> {
    /// bind the admitted file, canonical package, and comparator byte owners
    pub const fn new(files: Files, codec: Codec, values: Values) -> Self {
        Self {
            files,
            codec,
            values,
        }
    }
}

/// Exact qualification evidence and the verified generated package selected by it.
struct Selection {
    package: Package,
    authority: MaintenanceShadowServingAuthority,
    action_sha256: String,
    full_member_root_sha256: String,
    policy_report_sha256: String,
}

impl<Files, Codec, Values> NativeInstalledQualifiedCanaryLoaders<Files, Codec, Values>
where
    Files: ApiQualifiedCanaryFiles,
    Codec: ApiMaintenanceCandidateCodec,
    Values: ApiStandardRuntimeValues,
{
    /// read only the selected serving slot and bind registry, action, and package
    fn load(&mut self, family: AdjustmentShadowFamily) -> Option<Selection> {
        let catalog_bytes = self.files.read_catalog().ok()?;
        let catalog = self.codec.parse_canonical_object(&catalog_bytes)?;
        let selected =
            select_qualified_serving_catalog(&catalog, family, &mut self.codec).ok()??;
        let members = self
            .files
            .read_qualified_canary_members(family, &selected.action_sha256, &selected.bundle_sha256)
            .ok()?;
        validate_qualified_serving_members(
            &mut self.codec,
            &selected,
            family,
            &members.registry,
            &members.action,
        )
        .ok()?;
        let (document, package) = admit_qualified_api_canary_package(
            family,
            &selected.bundle_sha256,
            &selected.candidate_sha256,
            &members.bundle,
            &mut self.codec,
        )
        .ok()?;
        // bind the fitted values in the generated projection to the original package bytes
        match &package {
            Package::Wind(package) => {
                candidate_matches(&package.candidate, field(&document, "candidate")?)?;
                let fingerprint = &package.candidate.runtimeFingerprint;
                // require the fitted process calendar before exposing a wind runtime
                self.values
                    .standard_calendar_matches(&fingerprint.icuVersion, &fingerprint.tzdataVersion)
                    .then_some(())?;
            }
            Package::Temperature(package) => {
                (project_permanent_model(field(&document, "model")?)? == package.model)
                    .then_some(())?;
            }
        }
        let receipt = self.codec.encode_canonical_json(selected.raw_receipt)?;
        let authority = qualified_authority(
            &self.values,
            &selected.bundle_sha256,
            &members.bundle,
            &receipt,
        )?;
        Some(Selection {
            package,
            authority,
            action_sha256: selected.action_sha256,
            full_member_root_sha256: selected.full_member_root_sha256,
            policy_report_sha256: selected.policy_report_sha256,
        })
    }
}

impl<Files, Codec, Values> InstalledQualifiedCanaryLoaders
    for NativeInstalledQualifiedCanaryLoaders<Files, Codec, Values>
where
    Files: ApiQualifiedCanaryFiles,
    Codec: ApiMaintenanceCandidateCodec,
    Values: ApiStandardRuntimeValues,
{
    /// move the installed wind model into its qualified public startup shape
    fn load_qualified_wind_canary(
        &mut self,
    ) -> Result<c::LoadedForecastAdjustmentWindCanaryRuntime, WeatherApiStartupLoadError> {
        let selected = self
            .load(AdjustmentShadowFamily::Wind)
            .ok_or(WeatherApiStartupLoadError)?;
        let Package::Wind(package) = selected.package else {
            return Err(WeatherApiStartupLoadError);
        };
        Ok(c::LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceActive(
            c::LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActive {
                bundle: c::LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActiveBundle {
                    maintenanceBundleSha256: package.bundleSha256.clone(),
                    bundleSha256: package.bundleSha256,
                    candidate: package.candidate,
                    maintenanceAuthority:
                        c::LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActiveBundleMaintenanceAuthority {
                            actionSha256: selected.action_sha256,
                            fullMemberRootSha256: selected.full_member_root_sha256,
                            policyReportSha256: selected.policy_report_sha256,
                        },
                },
                comparatorAuthority: selected.authority,
                reasonCode: c::LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActiveReasonCodeNull,
                state: c::LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActiveStateLiteral,
            },
        ))
    }

    /// move the installed temperature model into the fixed source projection
    fn load_qualified_temperature_canary(
        &mut self,
    ) -> Result<c::LoadedForecastAdjustmentTemperatureCanaryRuntime, WeatherApiStartupLoadError>
    {
        let selected = self
            .load(AdjustmentShadowFamily::Temperature)
            .ok_or(WeatherApiStartupLoadError)?;
        let Package::Temperature(package) = selected.package else {
            return Err(WeatherApiStartupLoadError);
        };
        Ok(c::LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceActive(
            c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActive {
                bundle: c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundle {
                    bundleSha256: package.bundleSha256,
                    maintenanceAuthority:
                        c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleMaintenanceAuthority {
                            actionSha256: selected.action_sha256,
                            fullMemberRootSha256: selected.full_member_root_sha256,
                            policyReportSha256: selected.policy_report_sha256,
                        },
                    maintenancePackage:
                        c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleMaintenancePackageLiteral,
                    model: package.model,
                    servedForecastIdentity: temperature_served_identity(),
                    trainingForecastIdentity:
                        c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleTrainingForecastIdentity {
                            cohort: c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleTrainingForecastIdentityCohortLiteral,
                            scope: c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleTrainingForecastIdentityScopeLiteral,
                        },
                },
                comparatorAuthority: selected.authority,
                reasonCode: c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveReasonCodeNull,
                state: c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveStateLiteral,
            },
        ))
    }
}

/// project the source literals already bound by the verified temperature package
fn temperature_served_identity()
-> c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentity
{
    c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentity {
        adapterVersion: c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityAdapterVersionLiteral,
        dataset: c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityDatasetLiteral,
        maximumReceiptAgeHours: c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityMaximumReceiptAgeHoursLiteral,
        providerKey: c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityProviderKeyLiteral,
        sourceDelayHours: c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentitySourceDelayHoursLiteral,
        upstreamModel: c::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityUpstreamModelLiteral,
    }
}

/// bind the separate package identity and exact retained member bytes
fn qualified_authority(
    values: &impl ApiStandardRuntimeValues,
    identity: &str,
    artifact: &[u8],
    receipt: &[u8],
) -> Option<MaintenanceShadowServingAuthority> {
    let artifact_sha = values
        .sha256(artifact)
        .filter(|hash| valid_hash(hash.as_str()))?;
    let receipt_sha = values
        .sha256(receipt)
        .filter(|hash| valid_hash(hash.as_str()))?;
    Some(MaintenanceShadowServingAuthority {
        artifactBase64: Nullable::Value(values.encode_standard_base64(artifact)?),
        artifactIdentitySha256: Nullable::Value(identity.to_owned()),
        artifactMemberSha256: Nullable::Value(artifact_sha.as_str().to_owned()),
        authorityKind: MaintenanceShadowComparatorAuthorityKind::MaintenanceQualified,
        receiptBase64: values.encode_standard_base64(receipt)?,
        receiptMemberSha256: receipt_sha.as_str().to_owned(),
    })
}
