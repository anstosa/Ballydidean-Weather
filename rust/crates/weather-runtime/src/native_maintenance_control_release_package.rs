//! Source-exact pre-month rain control-reference target and compensation packages.

use std::fmt::{self, Display, Formatter};

use weather_api::admit_rain_portable_artifact;
use weather_contracts::{JsonValue, RainHurdleWindPortableArtifact};
use weather_domain::admit_rain_maintenance_control_state;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    NativeMaintenanceDevelopmentPublicationFile, NativeModelEvidenceValues, NativeWirePrimitives,
    api_maintenance_admission::ApiRainControlPrimitives,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_model_release_publish::NativeMaintenanceModelPublishPackage,
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
};

const ACTION_VERSION: &str = "forecast-adjustment-rain-control-reference-action/v1";
const SELECTOR_VERSION: &str = "forecast-adjustment-rain-control-reference-registry/v1";
const SELECTOR_PATH: &str = "config/forecast-adjustments/ballydidean-rain-control-reference.json";
const PACKAGE_MAXIMUM_BYTES: usize = 8 * 1_024 * 1_024;
const DAY_MILLISECONDS: i64 = 86_400_000;
const ACTION_KEYS: &[&str] = &[
    "actionKind",
    "contractVersion",
    "controlStateSha256",
    "createdAt",
    "dueMonth",
    "expectedCatalogReceiptSha256",
    "expectedSettingsSha256",
    "expectedSourceCommit",
    "expectedSourceRelease",
    "family",
    "fencingToken",
    "graphManifestSha256",
    "ordinalArtifactSha256",
    "predecessorActionSha256",
    "reason",
    "sourceMemberRootSha256",
    "sourceReceiptRootSha256",
    "validThrough",
];
const SELECTOR_KEYS: &[&str] = &[
    "actionSha256",
    "contractVersion",
    "controlStatePath",
    "controlStateSha256",
    "dueMonth",
    "ordinalArtifactPath",
    "ordinalArtifactSha256",
    "siteKey",
];

/// Verified graph and rolling-control material used to build one target reference.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceControlReferenceTargetInput<'a> {
    pub control_state_bytes: &'a [u8],
    pub due_key: &'a str,
    pub expected_catalog_receipt_sha256: Option<&'a str>,
    pub expected_settings_sha256: &'a str,
    pub expected_source_commit: &'a str,
    pub expected_source_release: &'a str,
    pub fencing_token: &'a str,
    pub graph_manifest_sha256: &'a str,
    pub now: &'a str,
    pub ordinal_artifact_bytes: &'a [u8],
    pub source_member_root_sha256: &'a str,
    pub source_receipt_root_sha256: &'a str,
}

/// Published target coordinates and the selector baseline prepared before target mutation.
#[derive(Clone, Copy, Debug)]
pub struct NativeMaintenanceControlReferenceCompensationInput<'a> {
    pub baseline_selector_bytes: Option<&'a [u8]>,
    pub compensation_created_at: &'a str,
    pub target: &'a NativeMaintenanceControlReleasePackage,
    pub target_commit_sha: &'a str,
    pub target_release_tag: &'a str,
}

/// Immutable target identity retained for release-pair journaling and generic publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceControlReleaseTargetIdentity {
    pub control_state_sha256: String,
    pub due_month: String,
    pub original_expected_source_commit: String,
    pub original_expected_source_release: String,
    pub predecessor_action_sha256: Option<String>,
    pub target_identity_sha256: String,
}

/// Complete target or compensation package before Git, journal, or transport effects.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceControlReleasePackage {
    pub action: JsonValue,
    pub action_bytes: Vec<u8>,
    pub action_path: String,
    pub action_sha256: String,
    pub package_files: Vec<NativeMaintenanceDevelopmentPublicationFile>,
    pub package_root_sha256: String,
    pub package_total_bytes: usize,
    pub removed_paths: Vec<String>,
    pub source_commit: String,
    pub target: NativeMaintenanceControlReleaseTargetIdentity,
}

impl NativeMaintenanceControlReleasePackage {
    /// Borrow this verified material in the generic native model publisher's exact shape.
    pub(crate) fn as_publish_package<'a>(
        &'a self,
        release_date: &'a str,
        release_due_key: &'a str,
        release_run_id: &'a str,
    ) -> NativeMaintenanceModelPublishPackage<'a> {
        NativeMaintenanceModelPublishPackage {
            action: &self.action,
            action_sha256: &self.action_sha256,
            target_identity: &self.target.target_identity_sha256,
            package_root_sha256: &self.package_root_sha256,
            package_files: &self.package_files,
            removed_paths: &self.removed_paths,
            release_date,
            release_due_key,
            release_run_id,
        }
    }
}

/// Closed material, action, selector, or package refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceControlReleasePackageError {
    Material,
    Action,
    Selector,
    Package,
    Codec,
}

impl Display for NativeMaintenanceControlReleasePackageError {
    /// Render one stable refusal without exposing package content.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // keep material and package failures distinguishable to the caller
        formatter.write_str(match self {
            Self::Material => "rain control reference material is invalid",
            Self::Action => "rain control reference action is invalid",
            Self::Selector => "rain control reference baseline selector is invalid",
            Self::Package => "rain control reference package is invalid",
            Self::Codec => "rain control reference serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceControlReleasePackageError {}

/// Build the exact ordinary control-reference action, members, and active selector.
pub fn build_native_maintenance_control_reference_target(
    input: NativeMaintenanceControlReferenceTargetInput<'_>,
) -> Result<NativeMaintenanceControlReleasePackage, NativeMaintenanceControlReleasePackageError> {
    use NativeMaintenanceControlReleasePackageError as Error;

    let wire = NativeWirePrimitives;
    let values = NativeModelEvidenceValues::new();
    let state = admit_rain_maintenance_control_state(
        &ApiRainControlPrimitives(&values),
        input.control_state_bytes,
    )
    .ok_or(Error::Material)?;
    let artifact_month = admit_ordinal_artifact(&values, input.ordinal_artifact_bytes)?;
    let control_state_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &wire,
        input.control_state_bytes,
    ));
    let ordinal_artifact_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &wire,
        input.ordinal_artifact_bytes,
    ));
    let valid_through = format!("{}-01T00:00:00.000Z", state.model_month);
    let month_start = wire
        .canonical_utc_milliseconds(&valid_through)
        .ok_or(Error::Material)?;
    let now = wire
        .canonical_utc_milliseconds(input.now)
        .ok_or(Error::Material)?;
    // bind the due projection and graph material to the exact admitted rolling state
    if input.due_key != format!("control-reference/rain/{}", state.model_month)
        || artifact_month != state.model_month
        || state.ordinal_artifact_sha256 != ordinal_artifact_sha256
        || state.source_member_root_sha256 != input.source_member_root_sha256
        || state.source_receipt_root_sha256 != input.source_receipt_root_sha256
        || state.generated_epoch_milliseconds < month_start - 7 * DAY_MILLISECONDS
        || state.generated_epoch_milliseconds >= month_start
        || state.generated_epoch_milliseconds > now
        || input.control_state_bytes.len() > PACKAGE_MAXIMUM_BYTES
        || input.ordinal_artifact_bytes.is_empty()
        || input.ordinal_artifact_bytes.len() > PACKAGE_MAXIMUM_BYTES
    {
        return Err(Error::Material);
    }
    let action = object(vec![
        ("actionKind", string("control_reference")),
        ("contractVersion", string(ACTION_VERSION)),
        ("controlStateSha256", string(&control_state_sha256)),
        ("createdAt", string(&state.generated_at)),
        ("dueMonth", string(&state.model_month)),
        (
            "expectedCatalogReceiptSha256",
            input
                .expected_catalog_receipt_sha256
                .map_or(JsonValue::Null, string),
        ),
        (
            "expectedSettingsSha256",
            string(input.expected_settings_sha256),
        ),
        ("expectedSourceCommit", string(input.expected_source_commit)),
        (
            "expectedSourceRelease",
            string(input.expected_source_release),
        ),
        ("family", string("rain")),
        ("fencingToken", string(input.fencing_token)),
        ("graphManifestSha256", string(input.graph_manifest_sha256)),
        ("ordinalArtifactSha256", string(&ordinal_artifact_sha256)),
        ("predecessorActionSha256", JsonValue::Null),
        ("reason", string("premonth_reference")),
        (
            "sourceMemberRootSha256",
            string(input.source_member_root_sha256),
        ),
        (
            "sourceReceiptRootSha256",
            string(input.source_receipt_root_sha256),
        ),
        ("validThrough", string(&valid_through)),
    ]);
    let built = build_action(&wire, action, false)?;
    let selector = control_selector(
        &wire,
        &built.action_sha256,
        &control_state_sha256,
        &state.model_month,
        &ordinal_artifact_sha256,
    )?;
    let files = vec![
        NativeMaintenanceDevelopmentPublicationFile {
            path: built.action_path.clone(),
            bytes: built.action_bytes.clone(),
        },
        NativeMaintenanceDevelopmentPublicationFile {
            path: format!(
                "config/forecast-adjustments/ballydidean/rain-maintenance-control-states/sha256-{control_state_sha256}.json"
            ),
            bytes: input.control_state_bytes.to_vec(),
        },
        NativeMaintenanceDevelopmentPublicationFile {
            path: format!(
                "config/forecast-adjustments/ballydidean/rain-runtime-artifacts/sha256-{ordinal_artifact_sha256}.json"
            ),
            bytes: input.ordinal_artifact_bytes.to_vec(),
        },
        selector,
    ];
    finish_package(
        &wire,
        built,
        files,
        Vec::new(),
        input.expected_source_commit,
        NativeMaintenanceControlReleaseTargetIdentity {
            control_state_sha256: control_state_sha256.clone(),
            due_month: state.model_month,
            original_expected_source_commit: input.expected_source_commit.to_owned(),
            original_expected_source_release: input.expected_source_release.to_owned(),
            predecessor_action_sha256: None,
            target_identity_sha256: control_state_sha256,
        },
    )
}

/// Build the paired action that restores the exact predecessor selector or its absence.
pub fn build_native_maintenance_control_reference_compensation(
    input: NativeMaintenanceControlReferenceCompensationInput<'_>,
) -> Result<NativeMaintenanceControlReleasePackage, NativeMaintenanceControlReleasePackageError> {
    use NativeMaintenanceControlReleasePackageError as Error;

    let wire = NativeWirePrimitives;
    let target = admit_target(&wire, input.target)?;
    // require immutable published coordinates before replacing the target source identity
    if !valid_commit(input.target_commit_sha) || !valid_release(input.target_release_tag) {
        return Err(Error::Action);
    }
    let action = object(vec![
        ("actionKind", string("compensate_control_reference")),
        ("contractVersion", string(ACTION_VERSION)),
        ("controlStateSha256", string(&target.control_state_sha256)),
        ("createdAt", string(input.compensation_created_at)),
        ("dueMonth", string(&target.due_month)),
        (
            "expectedCatalogReceiptSha256",
            target
                .expected_catalog_receipt_sha256
                .as_deref()
                .map_or(JsonValue::Null, string),
        ),
        (
            "expectedSettingsSha256",
            string(&target.expected_settings_sha256),
        ),
        ("expectedSourceCommit", string(input.target_commit_sha)),
        ("expectedSourceRelease", string(input.target_release_tag)),
        ("family", string("rain")),
        ("fencingToken", string(&target.fencing_token)),
        ("graphManifestSha256", string(&target.graph_manifest_sha256)),
        (
            "ordinalArtifactSha256",
            string(&target.ordinal_artifact_sha256),
        ),
        (
            "predecessorActionSha256",
            string(&input.target.action_sha256),
        ),
        ("reason", string("failed_control_reference")),
        (
            "sourceMemberRootSha256",
            string(&target.source_member_root_sha256),
        ),
        (
            "sourceReceiptRootSha256",
            string(&target.source_receipt_root_sha256),
        ),
        ("validThrough", string(&target.valid_through)),
    ]);
    let built = build_action(&wire, action, true)?;
    let mut files = vec![NativeMaintenanceDevelopmentPublicationFile {
        path: built.action_path.clone(),
        bytes: built.action_bytes.clone(),
    }];
    let mut removed_paths = Vec::new();
    // preserve the predecessor selector bytes exactly, including prepared absence
    if let Some(selector_bytes) = input.baseline_selector_bytes {
        validate_selector(selector_bytes)?;
        files.push(NativeMaintenanceDevelopmentPublicationFile {
            path: SELECTOR_PATH.to_owned(),
            bytes: selector_bytes.to_vec(),
        });
    } else {
        removed_paths.push(SELECTOR_PATH.to_owned());
    }
    finish_package(
        &wire,
        built,
        files,
        removed_paths,
        input.target_commit_sha,
        NativeMaintenanceControlReleaseTargetIdentity {
            control_state_sha256: target.control_state_sha256.clone(),
            due_month: target.due_month,
            original_expected_source_commit: target.expected_source_commit,
            original_expected_source_release: target.expected_source_release,
            predecessor_action_sha256: Some(input.target.action_sha256.clone()),
            target_identity_sha256: target.control_state_sha256,
        },
    )
}

struct BuiltAction {
    action: JsonValue,
    action_bytes: Vec<u8>,
    action_path: String,
    action_sha256: String,
}

// encode and re-admit one source action before it enters a package
fn build_action(
    wire: &NativeWirePrimitives,
    action: JsonValue,
    compensation: bool,
) -> Result<BuiltAction, NativeMaintenanceControlReleasePackageError> {
    use NativeMaintenanceControlReleasePackageError as Error;

    validate_action(wire, &action, compensation)?;
    let action_bytes = wire.encode_sorted_json(&action).ok_or(Error::Codec)?;
    let action_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(wire, &action_bytes));
    Ok(BuiltAction {
        action,
        action_path: format!(
            "config/forecast-adjustments/ballydidean/actions/sha256-{action_sha256}.json"
        ),
        action_bytes,
        action_sha256,
    })
}

// validate the closed action matrix and literal pre-month chronology
fn validate_action(
    wire: &NativeWirePrimitives,
    action: &JsonValue,
    compensation: bool,
) -> Result<(), NativeMaintenanceControlReleasePackageError> {
    use NativeMaintenanceControlReleasePackageError::Action;

    let kind = text(action, "actionKind").ok_or(Action)?;
    let reason = text(action, "reason").ok_or(Action)?;
    let due_month = text(action, "dueMonth").ok_or(Action)?;
    let created_at = text(action, "createdAt").ok_or(Action)?;
    let valid_through = text(action, "validThrough").ok_or(Action)?;
    let predecessor = nullable_hash(action, "predecessorActionSha256")?;
    let catalog = nullable_hash(action, "expectedCatalogReceiptSha256")?;
    let month_start = wire
        .canonical_utc_milliseconds(&format!("{due_month}-01T00:00:00.000Z"))
        .ok_or(Action)?;
    let created = wire.canonical_utc_milliseconds(&created_at).ok_or(Action)?;
    // require the source's disjoint target and compensation action domains
    if !exact_keys(action, ACTION_KEYS)
        || text(action, "contractVersion").as_deref() != Some(ACTION_VERSION)
        || text(action, "family").as_deref() != Some("rain")
        || kind
            != if compensation {
                "compensate_control_reference"
            } else {
                "control_reference"
            }
        || reason
            != if compensation {
                "failed_control_reference"
            } else {
                "premonth_reference"
            }
        || !valid_month(&due_month)
        || valid_through != format!("{due_month}-01T00:00:00.000Z")
        || created < month_start - 7 * DAY_MILLISECONDS
        || created >= month_start
        || predecessor.is_some() != compensation
        || catalog.as_ref().is_some_and(|value| !valid_hash(value))
        || !hash_field(action, "expectedSettingsSha256")
        || !hash_field(action, "graphManifestSha256")
        || !hash_field(action, "ordinalArtifactSha256")
        || !hash_field(action, "controlStateSha256")
        || !hash_field(action, "sourceMemberRootSha256")
        || !hash_field(action, "sourceReceiptRootSha256")
        || !text(action, "expectedSourceCommit").is_some_and(|value| valid_commit(&value))
        || !text(action, "expectedSourceRelease").is_some_and(|value| valid_release(&value))
        || !text(action, "fencingToken").is_some_and(|value| valid_fence(&value))
    {
        return Err(Action);
    }
    Ok(())
}

struct AdmittedTarget {
    control_state_sha256: String,
    due_month: String,
    expected_catalog_receipt_sha256: Option<String>,
    expected_settings_sha256: String,
    expected_source_commit: String,
    expected_source_release: String,
    fencing_token: String,
    graph_manifest_sha256: String,
    ordinal_artifact_sha256: String,
    source_member_root_sha256: String,
    source_receipt_root_sha256: String,
    valid_through: String,
}

// re-admit the exact ordinary target and its canonical action address
fn admit_target(
    wire: &NativeWirePrimitives,
    package: &NativeMaintenanceControlReleasePackage,
) -> Result<AdmittedTarget, NativeMaintenanceControlReleasePackageError> {
    use NativeMaintenanceControlReleasePackageError::Action;

    validate_action(wire, &package.action, false)?;
    // reject substituted package or action coordinates before deriving compensation
    if wire.encode_sorted_json(&package.action).as_deref() != Some(&package.action_bytes)
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            wire,
            &package.action_bytes,
        )) != package.action_sha256
        || package.action_path
            != format!(
                "config/forecast-adjustments/ballydidean/actions/sha256-{}.json",
                package.action_sha256
            )
        || package.target.predecessor_action_sha256.is_some()
    {
        return Err(Action);
    }
    Ok(AdmittedTarget {
        control_state_sha256: text(&package.action, "controlStateSha256").ok_or(Action)?,
        due_month: text(&package.action, "dueMonth").ok_or(Action)?,
        expected_catalog_receipt_sha256: nullable_hash(
            &package.action,
            "expectedCatalogReceiptSha256",
        )?,
        expected_settings_sha256: text(&package.action, "expectedSettingsSha256").ok_or(Action)?,
        expected_source_commit: text(&package.action, "expectedSourceCommit").ok_or(Action)?,
        expected_source_release: text(&package.action, "expectedSourceRelease").ok_or(Action)?,
        fencing_token: text(&package.action, "fencingToken").ok_or(Action)?,
        graph_manifest_sha256: text(&package.action, "graphManifestSha256").ok_or(Action)?,
        ordinal_artifact_sha256: text(&package.action, "ordinalArtifactSha256").ok_or(Action)?,
        source_member_root_sha256: text(&package.action, "sourceMemberRootSha256").ok_or(Action)?,
        source_receipt_root_sha256: text(&package.action, "sourceReceiptRootSha256")
            .ok_or(Action)?,
        valid_through: text(&package.action, "validThrough").ok_or(Action)?,
    })
}

// admit the generated portable rain artifact and retain its bound model month
fn admit_ordinal_artifact(
    values: &NativeModelEvidenceValues,
    bytes: &[u8],
) -> Result<String, NativeMaintenanceControlReleasePackageError> {
    use NativeMaintenanceControlReleasePackageError::Material;

    let mut wire = NativeWirePrimitives;
    let document =
        crate::maintenance_canonical_json::MaintenanceCanonicalJson::parse_canonical_object(
            &mut wire, bytes,
        )
        .ok_or(Material)?;
    let artifact = admit_rain_portable_artifact(
        &document,
        |value| values.decode_shadow_rain_artifact(value),
        |feature_names| {
            let bytes = values.encode_shadow_ordered_json(feature_names)?;
            ApiMaintenanceReceiptCanonicalValues::sha256(values, &bytes)
        },
    )
    .ok_or(Material)?;
    Ok(match artifact {
        RainHurdleWindPortableArtifact::V1(value) => value.modelMonth,
        RainHurdleWindPortableArtifact::V2(value) => value.modelMonth,
    })
}

// construct the canonical active selector from the target action address
fn control_selector(
    wire: &NativeWirePrimitives,
    action_sha256: &str,
    control_state_sha256: &str,
    due_month: &str,
    ordinal_artifact_sha256: &str,
) -> Result<NativeMaintenanceDevelopmentPublicationFile, NativeMaintenanceControlReleasePackageError>
{
    use NativeMaintenanceControlReleasePackageError::Codec;

    let value = object(vec![
        ("actionSha256", string(action_sha256)),
        ("contractVersion", string(SELECTOR_VERSION)),
        (
            "controlStatePath",
            string(&format!(
                "rain-maintenance-control-states/sha256-{control_state_sha256}.json"
            )),
        ),
        ("controlStateSha256", string(control_state_sha256)),
        ("dueMonth", string(due_month)),
        (
            "ordinalArtifactPath",
            string(&format!(
                "rain-runtime-artifacts/sha256-{ordinal_artifact_sha256}.json"
            )),
        ),
        ("ordinalArtifactSha256", string(ordinal_artifact_sha256)),
        ("siteKey", string("ballydidean")),
    ]);
    Ok(NativeMaintenanceDevelopmentPublicationFile {
        path: SELECTOR_PATH.to_owned(),
        bytes: wire.encode_sorted_json(&value).ok_or(Codec)?,
    })
}

// validate one canonical predecessor selector before restoring its bytes
fn validate_selector(bytes: &[u8]) -> Result<(), NativeMaintenanceControlReleasePackageError> {
    use NativeMaintenanceControlReleasePackageError::Selector;

    let mut wire = NativeWirePrimitives;
    let selector =
        crate::maintenance_canonical_json::MaintenanceCanonicalJson::parse_canonical_object(
            &mut wire, bytes,
        )
        .ok_or(Selector)?;
    let action = text(&selector, "actionSha256").ok_or(Selector)?;
    let state = text(&selector, "controlStateSha256").ok_or(Selector)?;
    let artifact = text(&selector, "ordinalArtifactSha256").ok_or(Selector)?;
    let month = text(&selector, "dueMonth").ok_or(Selector)?;
    // bind all content-addressed paths while accepting any valid predecessor identities
    if !exact_keys(&selector, SELECTOR_KEYS)
        || !valid_hash(&action)
        || !valid_hash(&state)
        || !valid_hash(&artifact)
        || !valid_month(&month)
        || text(&selector, "contractVersion").as_deref() != Some(SELECTOR_VERSION)
        || text(&selector, "siteKey").as_deref() != Some("ballydidean")
        || text(&selector, "controlStatePath").as_deref()
            != Some(format!("rain-maintenance-control-states/sha256-{state}.json").as_str())
        || text(&selector, "ordinalArtifactPath").as_deref()
            != Some(format!("rain-runtime-artifacts/sha256-{artifact}.json").as_str())
    {
        return Err(Selector);
    }
    Ok(())
}

// sort, bound, screen, and address one exact target or compensation operation set
fn finish_package(
    wire: &NativeWirePrimitives,
    built: BuiltAction,
    mut files: Vec<NativeMaintenanceDevelopmentPublicationFile>,
    mut removed_paths: Vec<String>,
    source_commit: &str,
    target: NativeMaintenanceControlReleaseTargetIdentity,
) -> Result<NativeMaintenanceControlReleasePackage, NativeMaintenanceControlReleasePackageError> {
    use NativeMaintenanceControlReleasePackageError as Error;

    files.sort_by(|left, right| left.path.cmp(&right.path));
    removed_paths.sort();
    let mut changed = files
        .iter()
        .map(|file| file.path.as_str())
        .chain(removed_paths.iter().map(String::as_str))
        .collect::<Vec<_>>();
    changed.sort_unstable();
    // refuse duplicate write/remove operations before calculating their shared root
    if changed.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::Package);
    }
    let mut total = 0usize;
    let mut operations = Vec::with_capacity(changed.len());
    // preserve sorted file order and the original public package limit
    for file in &files {
        total = total.checked_add(file.bytes.len()).ok_or(Error::Package)?;
        if file.bytes.is_empty()
            || total > PACKAGE_MAXIMUM_BYTES
            || !valid_public_path(&file.path)
            || contains_private_material(&file.bytes)
        {
            return Err(Error::Package);
        }
        operations.push((
            file.path.clone(),
            object(vec![
                ("operation", string("write")),
                ("path", string(&file.path)),
                (
                    "sha256",
                    string(&format_digest(RevisionArchiveCanonicalCodec::sha256(
                        wire,
                        &file.bytes,
                    ))),
                ),
            ]),
        ));
    }
    // retain the one source-approved selector deletion as an explicit null-addressed operation
    for path in &removed_paths {
        if path != SELECTOR_PATH {
            return Err(Error::Package);
        }
        operations.push((
            path.clone(),
            object(vec![
                ("operation", string("remove")),
                ("path", string(path)),
                ("sha256", JsonValue::Null),
            ]),
        ));
    }
    operations.sort_by(|left, right| left.0.cmp(&right.0));
    let operation_values = operations.into_iter().map(|(_, value)| value).collect();
    let operation_bytes = wire
        .encode_sorted_json(&JsonValue::Array(operation_values))
        .ok_or(Error::Codec)?;
    Ok(NativeMaintenanceControlReleasePackage {
        action: built.action,
        action_bytes: built.action_bytes,
        action_path: built.action_path,
        action_sha256: built.action_sha256,
        package_files: files,
        package_root_sha256: format_digest(RevisionArchiveCanonicalCodec::sha256(
            wire,
            &operation_bytes,
        )),
        package_total_bytes: total,
        removed_paths,
        source_commit: source_commit.to_owned(),
        target,
    })
}

// read one nullable hash without accepting a non-string present value
fn nullable_hash(
    value: &JsonValue,
    name: &str,
) -> Result<Option<String>, NativeMaintenanceControlReleasePackageError> {
    use NativeMaintenanceControlReleasePackageError::Action;

    match field(value, name).ok_or(Action)? {
        JsonValue::Null => Ok(None),
        JsonValue::String(units) => {
            let value = String::from_utf16(units).map_err(|_| Action)?;
            valid_hash(&value).then_some(Some(value)).ok_or(Action)
        }
        _ => Err(Action),
    }
}

// require one named action member to be a lowercase full SHA-256
fn hash_field(value: &JsonValue, name: &str) -> bool {
    text(value, name).is_some_and(|value| valid_hash(&value))
}

// recognize the source's four-digit year and bounded calendar month
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

// recognize one exact lowercase immutable Git commit identity
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// recognize the source's immutable calendar release namespace
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    let revision = bytes.get(11..).unwrap_or_default();
    bytes.len() >= 12
        && bytes.get(4) == Some(&b'.')
        && bytes.get(7) == Some(&b'.')
        && bytes.get(10) == Some(&b'-')
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && matches!(revision.first(), Some(b'1'..=b'9'))
        && revision.iter().all(u8::is_ascii_digit)
}

// recognize one canonical positive unsigned 64-bit release fence
fn valid_fence(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

// reject paths outside the source's public repository-relative grammar
fn valid_public_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 240
        && !value.starts_with('/')
        && !value.contains("..")
        && !value.contains("//")
        && !value.starts_with(".github/")
        && !value.contains('\\')
}

// reject the source's fixed obvious credential and host-private byte markers
fn contains_private_material(bytes: &[u8]) -> bool {
    let Ok(value) = std::str::from_utf8(bytes) else {
        return false;
    };
    // reject the exact fixed private markers first
    if [
        "-----BEGIN OPENSSH PRIVATE KEY-----",
        "-----BEGIN RSA PRIVATE KEY-----",
        "-----BEGIN EC PRIVATE KEY-----",
        "-----BEGIN DSA PRIVATE KEY-----",
        "/home/",
        "/root/",
        "/mnt/c/Users/",
        "SSH_AUTH_SOCK",
        "ANSEL_HOST_PASSWORD",
    ]
    .iter()
    .any(|needle| value.contains(needle))
    {
        return true;
    }
    // preserve token-prefix matching without adding another regular-expression dependency
    ["ghp", "github_pat", "glpat"].iter().any(|prefix| {
        value.match_indices(prefix).any(|(offset, _)| {
            let suffix = &value.as_bytes()[offset + prefix.len()..];
            let suffix = suffix.strip_prefix(b"-").unwrap_or(suffix);
            suffix
                .iter()
                .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
                .count()
                >= 20
        })
    })
}

#[cfg(test)]
#[path = "native_maintenance_control_release_package_test.rs"]
mod tests;
