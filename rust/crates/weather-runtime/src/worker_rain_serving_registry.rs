//! Admit the original installed rain registry and retain the frozen incumbent once.

use std::sync::Arc;

use weather_contracts::retained_rain_hurdle_wind::RETAINED_RAIN_MODEL_SHA256;
use weather_contracts::{
    JsonValue, MaintenanceShadowComparatorAuthorityKind as AuthorityKind,
    MaintenanceShadowServingAuthority, Nullable,
};
use weather_worker::{RainAdjustmentModel, RainAdjustmentRuntimeSelection};

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::worker_rain_candidate_package::check_worker_rain_package;
use crate::worker_rain_serving_catalog::{
    select_worker_rain_serving_catalog, validate_worker_rain_serving_members,
};
use crate::{
    ApiMaintenanceCandidateCodec, ApiMaintenanceShadowEvidenceValues,
    PreparedWorkerRainMaintenanceCandidate, WorkerRainServingFiles,
};

pub(crate) const RETAINED_RAIN_ARTIFACT: &[u8] =
    include_bytes!("../../weather-contracts/retained-rain-hurdle-wind.json");
const RAW_REGISTRY_SHA256: &str =
    "0bd90b7fdd9a0531bc9517c38093bdb59f53b84b58273a4cc8b4756b6683e888";
const RAW_REGISTRY_BYTES: usize = 138;
const LEGACY_REGISTRY: &str = "forecast-adjustment-rain-runtime-registry/v1";
const MAINTENANCE_REGISTRY: &str = "forecast-adjustment-rain-maintenance-registry/v1";

/// Approved outbound base64 in addition to the original immutable model primitives.
pub trait WorkerRainServingValues: ApiMaintenanceShadowEvidenceValues {
    /// Encode exact retained bytes for the generated comparator authority.
    fn encode_worker_rain_base64(&self, bytes: &[u8]) -> Option<String>;
}

/// Runtime selection, original model, and corroborating comparator authority from one startup.
pub struct LoadedWorkerRainServing {
    selection: RainAdjustmentRuntimeSelection,
    authority: Option<Arc<MaintenanceShadowServingAuthority>>,
}

impl LoadedWorkerRainServing {
    /// Read the worker scheduler's exact active/raw/disabled selection.
    pub const fn selection(&self) -> RainAdjustmentRuntimeSelection {
        self.selection
    }

    /// Borrow the original incumbent without recopying or reparsing its tables.
    pub const fn model(&self) -> Option<RainAdjustmentModel<'static>> {
        // preserve incumbent features in policy-raw mode without enabling inference on invalid registry
        match self.selection {
            RainAdjustmentRuntimeSelection::Active
            | RainAdjustmentRuntimeSelection::DisabledPolicyRawWithComparatorAuthority => {
                Some(RainAdjustmentModel::retained())
            }
            RainAdjustmentRuntimeSelection::Disabled
            | RainAdjustmentRuntimeSelection::Unconfigured => None,
        }
    }

    /// Share the same immutable comparator witness with optional archive work.
    pub fn comparator_authority(&self) -> Option<Arc<MaintenanceShadowServingAuthority>> {
        self.authority.clone()
    }

    /// Retain no inference or comparator authority when installed material is invalid.
    const fn disabled() -> Self {
        Self {
            selection: RainAdjustmentRuntimeSelection::Disabled,
            authority: None,
        }
    }
}

/// Read and admit the worker's complete startup selection once, without stopping raw serving.
pub fn load_worker_rain_serving(
    files: &mut impl WorkerRainServingFiles,
    codec: &mut impl ApiMaintenanceCandidateCodec,
    values: &impl WorkerRainServingValues,
) -> LoadedWorkerRainServing {
    read_serving(files, codec, values).unwrap_or_else(LoadedWorkerRainServing::disabled)
}

/// Exact admitted selection and authority before the worker shares the immutable witness.
pub(crate) struct AdmittedRainServing {
    pub(crate) selection: RainAdjustmentRuntimeSelection,
    pub(crate) authority: MaintenanceShadowServingAuthority,
}

/// Four branches of the original closed startup registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerRainRegistryChoice {
    LegacyActive,
    LegacyRaw,
    MaintenanceRaw,
    MaintenanceActive,
}

/// Classify the parsed canonical registry without granting authority for a selected package.
fn classify_worker_rain_registry(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    doc: &JsonValue,
    bytes: &[u8],
) -> Option<WorkerRainRegistryChoice> {
    if let Some(active) = field(doc, "activePackage") {
        // defer package selectors to the original independent installed serving catalog
        if !matches!(active, JsonValue::Null) {
            return Some(WorkerRainRegistryChoice::MaintenanceActive);
        }
        return (exact_keys(
            doc,
            &["activePackage", "contractVersion", "rawReason", "siteKey"],
        ) && text(doc, "contractVersion").as_deref() == Some(MAINTENANCE_REGISTRY)
            && text(doc, "rawReason").as_deref() == Some("policy_raw")
            && text(doc, "siteKey").as_deref() == Some("ballydidean"))
        .then_some(WorkerRainRegistryChoice::MaintenanceRaw);
    }
    // recognize only the retained legacy registry version and frozen site
    if !exact_keys(
        doc,
        &["activeArtifact", "contractVersion", "rawReason", "siteKey"],
    ) || text(doc, "siteKey").as_deref() != Some("ballydidean")
        || text(doc, "contractVersion").as_deref() != Some(LEGACY_REGISTRY)
    {
        return None;
    }
    let artifact = field(doc, "activeArtifact")?;
    match artifact {
        JsonValue::Null => (text(doc, "rawReason").as_deref() == Some("policy_raw")
            && bytes.len() == RAW_REGISTRY_BYTES
            && values.sha256(bytes)?.as_str() == RAW_REGISTRY_SHA256)
            .then_some(WorkerRainRegistryChoice::LegacyRaw),
        _ => (exact_keys(artifact, &["artifactSha256"])
            && text(artifact, "artifactSha256").as_deref() == Some(RETAINED_RAIN_MODEL_SHA256)
            && matches!(field(doc, "rawReason"), Some(JsonValue::Null)))
        .then_some(WorkerRainRegistryChoice::LegacyActive),
    }
}

/// Prepare the exact authority tuple for the already-compiled serving model.
fn read_serving(
    files: &mut impl WorkerRainServingFiles,
    codec: &mut impl ApiMaintenanceCandidateCodec,
    values: &impl WorkerRainServingValues,
) -> Option<LoadedWorkerRainServing> {
    let bytes = files.read_worker_rain_registry().ok()?;
    let admitted = admit_rain_serving_bytes(&bytes, files, codec, values)?;
    Some(LoadedWorkerRainServing {
        selection: admitted.selection,
        authority: Some(Arc::new(admitted.authority)),
    })
}

/// Admit one already-read installed registry through the shared worker serving policy.
pub(crate) fn admit_rain_serving_bytes(
    bytes: &[u8],
    files: &mut impl WorkerRainServingFiles,
    codec: &mut impl ApiMaintenanceCandidateCodec,
    values: &impl WorkerRainServingValues,
) -> Option<AdmittedRainServing> {
    let doc = codec.parse_canonical_object(bytes)?;
    let (selection, authority) = match classify_worker_rain_registry(values, &doc, bytes)? {
        WorkerRainRegistryChoice::LegacyRaw | WorkerRainRegistryChoice::MaintenanceRaw => (
            RainAdjustmentRuntimeSelection::DisabledPolicyRawWithComparatorAuthority,
            serving_authority(values, AuthorityKind::PolicyRaw, bytes, None)?,
        ),
        WorkerRainRegistryChoice::LegacyActive => (
            RainAdjustmentRuntimeSelection::Active,
            serving_authority(
                values,
                AuthorityKind::LegacyActive,
                bytes,
                Some(RETAINED_RAIN_ARTIFACT),
            )?,
        ),
        WorkerRainRegistryChoice::MaintenanceActive => (
            RainAdjustmentRuntimeSelection::Active,
            qualified_serving_authority(bytes, files, codec, values)?,
        ),
    };
    Some(AdmittedRainServing {
        selection,
        authority,
    })
}

/// Load exact root-installed qualified material without selecting an independent control slot.
fn qualified_serving_authority(
    registry_bytes: &[u8],
    files: &mut impl WorkerRainServingFiles,
    codec: &mut impl ApiMaintenanceCandidateCodec,
    values: &impl WorkerRainServingValues,
) -> Option<MaintenanceShadowServingAuthority> {
    let catalog = codec.parse_canonical_object(&files.read_catalog().ok()?)?;
    let selected = select_worker_rain_serving_catalog(&catalog, codec).ok()??;
    let members = files
        .read_worker_rain_serving_members(
            &selected.action_sha256,
            &selected.candidate_sha256,
            &selected.bundle_sha256,
        )
        .ok()?;
    // bind qualified members to the registry already admitted by the startup owner
    if members.registry != registry_bytes {
        return None;
    }
    validate_worker_rain_serving_members(codec, &selected, &members.registry, &members.action)
        .ok()?;
    let packaged = PreparedWorkerRainMaintenanceCandidate::prepare(
        values,
        &members.bundle,
        &selected.bundle_sha256,
    )?;
    check_worker_rain_package(
        codec,
        &members.package,
        &selected.candidate_sha256,
        &selected.bundle_sha256,
        packaged.model_month(),
    )
    .ok()?;
    // a qualified registry may not replace the artifact compiled into the serving image
    if selected.bundle_sha256 != RETAINED_RAIN_MODEL_SHA256 {
        return None;
    }
    let receipt = codec.encode_canonical_json(selected.raw_receipt)?;
    serving_authority(
        values,
        AuthorityKind::MaintenanceQualified,
        &receipt,
        Some(&members.bundle),
    )
}

/// Bind generated authority members to exact original receipt and artifact bytes.
fn serving_authority(
    values: &impl WorkerRainServingValues,
    kind: AuthorityKind,
    receipt: &[u8],
    artifact: Option<&[u8]>,
) -> Option<MaintenanceShadowServingAuthority> {
    let receipt_member = values
        .sha256(receipt)
        .filter(|digest| valid_hash(digest.as_str()))?;
    let (artifact_base64, artifact_hash) = match artifact {
        Some(bytes) => {
            let hash = values
                .sha256(bytes)
                .filter(|digest| digest.as_str() == RETAINED_RAIN_MODEL_SHA256)?;
            (
                Nullable::Value(values.encode_worker_rain_base64(bytes)?),
                Nullable::Value(hash.as_str().to_owned()),
            )
        }
        None => (Nullable::Null, Nullable::Null),
    };
    Some(MaintenanceShadowServingAuthority {
        artifactBase64: artifact_base64,
        artifactIdentitySha256: artifact_hash.clone(),
        artifactMemberSha256: artifact_hash,
        authorityKind: kind,
        receiptBase64: values.encode_worker_rain_base64(receipt)?,
        receiptMemberSha256: receipt_member.as_str().to_owned(),
    })
}

#[cfg(test)]
#[path = "worker_rain_serving_registry_test.rs"]
mod tests;
