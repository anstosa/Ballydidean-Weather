//! Exact qualified serving target and paired incumbent/raw compensation packages.

use std::fmt::{self, Display, Formatter};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use weather_api::admit_rain_portable_artifact;
use weather_contracts::{JsonValue, RainHurdleWindPortableArtifact};
use weather_domain::admit_rain_maintenance_control_state;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_candidate_parity::build_wind_portable_candidate;
use crate::native_maintenance_model_release_publish::NativeMaintenanceModelPublishPackage;
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    ApiMaintenanceReceiptCanonicalValues, ApiMaintenanceShadowEvidenceValues,
    NativeFamilyReleaseCurrent, NativeMaintenanceCaptureFamily,
    NativeMaintenanceDevelopmentPublicationFile, NativeMaintenanceInstalledTerminalMaterial,
    NativeModelEvidenceValues, NativeWirePrimitives, build_native_rain_maintenance_artifact,
    build_native_temperature_maintenance_package,
};

const ACTION_VERSION: &str = "forecast-adjustment-model-action/v1";
const PARITY_VERSION: &str = "forecast-adjustment-model-parity/v1";
const PACKAGE_MAXIMUM_BYTES: usize = 8 * 1024 * 1024;
const PARITY_MAXIMUM_BYTES: usize = 64 * 1024;
const DAY_MILLISECONDS: i64 = 86_400_000;
const ACTION_KEYS: &[&str] = &[
    "actionKind",
    "candidateGraphSha256",
    "candidateSha256",
    "contractVersion",
    "createdAt",
    "expectedInstalledReceiptSha256",
    "expectedSettingsSha256",
    "expectedSourceCommit",
    "expectedSourceRelease",
    "family",
    "fencingToken",
    "fullMemberRootSha256",
    "lifecycleHeadSha256",
    "policyDecision",
    "policyReportSha256",
    "predecessorActionSha256",
    "reason",
    "reportCreatedAt",
    "siteKey",
    "validThrough",
];
const PARITY_KEYS: &[&str] = &[
    "candidateSha256",
    "contractVersion",
    "family",
    "retainedInputSha256",
    "retainedNativeOutputSha256",
    "retainedPackagedOutputSha256",
    "syntheticInputSha256",
    "syntheticNativeOutputSha256",
    "syntheticPackagedOutputSha256",
];

/// Read-only authenticated source member access for an installed commit baseline.
pub trait NativeMaintenanceQualifiedBaselineFiles {
    /// Read one exact ordinary source path at the admitted commit within its byte ceiling.
    fn read_source_file(
        &mut self,
        commit: &str,
        path: &str,
        maximum: usize,
    ) -> Result<Vec<u8>, NativeMaintenanceQualifiedReleasePackageError>;
}

/// Shell-free fixed-repository Git reader for the actual installed compensation baseline.
pub struct NativeMaintenanceQualifiedBaselineGit {
    repository: PathBuf,
}

impl NativeMaintenanceQualifiedBaselineGit {
    /// Bind the reviewed production checkout without reading it yet.
    pub fn native() -> Self {
        Self {
            repository: PathBuf::from("/home/ubuntu/weather"),
        }
    }
}

impl NativeMaintenanceQualifiedBaselineFiles for NativeMaintenanceQualifiedBaselineGit {
    /// Require exact tree membership before reading one bounded blob.
    fn read_source_file(
        &mut self,
        commit: &str,
        path: &str,
        maximum: usize,
    ) -> Result<Vec<u8>, NativeMaintenanceQualifiedReleasePackageError> {
        use NativeMaintenanceQualifiedReleasePackageError as Error;

        // close commit, path, and allocation coordinates before invoking fixed Git
        if !valid_commit(commit) || !valid_source_path(path) || maximum > PACKAGE_MAXIMUM_BYTES {
            return Err(Error::Baseline);
        }
        let tree = run_git_bounded(
            &self.repository,
            &["ls-tree", "-z", "--name-only", commit, "--", path],
            path.len() + 2,
        )?;
        let mut expected = path.as_bytes().to_vec();
        expected.push(0);
        // prohibit absent, aliased, or multiply returned source paths
        if tree != expected {
            return Err(Error::Baseline);
        }
        let object = format!("{commit}:{path}");
        let bytes = run_git_bounded(&self.repository, &["show", &object], maximum)?;
        // retain the source model-package nonempty member rule
        if bytes.len() < 2 {
            return Err(Error::Baseline);
        }
        Ok(bytes)
    }
}

/// Exact already-installed compensation state captured before target mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceQualifiedCompensationBaseline {
    Raw,
    Incumbent {
        artifact_sha256: String,
        candidate_graph_sha256: String,
        candidate_sha256: String,
        family_files: Vec<NativeMaintenanceDevelopmentPublicationFile>,
        parity_receipt_bytes: Vec<u8>,
    },
}

/// One complete target or compensation package ready for the generic native publisher.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceQualifiedReleasePackage {
    pub action: JsonValue,
    pub action_bytes: Vec<u8>,
    pub action_path: String,
    pub action_sha256: String,
    pub family: NativeMaintenanceCaptureFamily,
    pub package_files: Vec<NativeMaintenanceDevelopmentPublicationFile>,
    pub package_root_sha256: String,
    pub package_total_bytes: usize,
    pub removed_paths: Vec<String>,
    pub source_commit: String,
    pub target_identity_sha256: String,
}

impl NativeMaintenanceQualifiedReleasePackage {
    /// Borrow the verified target or compensation in the generic publisher's exact shape.
    pub(crate) fn as_publish_package<'a>(
        &'a self,
        release_date: &'a str,
        release_due_key: &'a str,
        release_run_id: &'a str,
    ) -> NativeMaintenanceModelPublishPackage<'a> {
        NativeMaintenanceModelPublishPackage {
            action: &self.action,
            action_sha256: &self.action_sha256,
            target_identity: &self.target_identity_sha256,
            package_root_sha256: &self.package_root_sha256,
            package_files: &self.package_files,
            removed_paths: &self.removed_paths,
            release_date,
            release_due_key,
            release_run_id,
        }
    }
}

/// Closed candidate, baseline, action, package, or fixed Git refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceQualifiedReleasePackageError {
    Input,
    Action,
    Candidate,
    Baseline,
    Package,
    Codec,
    Git,
}

impl Display for NativeMaintenanceQualifiedReleasePackageError {
    /// Render one stable path-free qualified packaging failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "qualified release package input is invalid",
            Self::Action => "qualified release action is invalid",
            Self::Candidate => "qualified release candidate is invalid",
            Self::Baseline => "qualified release compensation baseline is invalid",
            Self::Package => "qualified release package is invalid",
            Self::Codec => "qualified release serialization failed",
            Self::Git => "qualified release source member is unavailable",
        })
    }
}

impl std::error::Error for NativeMaintenanceQualifiedReleasePackageError {}

/// Build the source-exact serving package for one already admitted promoted action.
pub fn build_native_maintenance_qualified_release_target(
    action: &JsonValue,
    installed: &NativeMaintenanceInstalledTerminalMaterial,
) -> Result<NativeMaintenanceQualifiedReleasePackage, NativeMaintenanceQualifiedReleasePackageError>
{
    use NativeMaintenanceQualifiedReleasePackageError as Error;

    let built = admit_action(action, Some("promote"))?;
    let candidate_sha256 = text(action, "candidateSha256").ok_or(Error::Action)?;
    let candidate_graph_sha256 = text(action, "candidateGraphSha256").ok_or(Error::Action)?;
    let family = built.family;
    let assembly_family = match &installed.assembly {
        crate::NativeMaintenanceDailyTerminalValues::Complete { family, .. } => *family,
        crate::NativeMaintenanceDailyTerminalValues::Pending { .. }
        | crate::NativeMaintenanceDailyTerminalValues::Unsupported { .. } => {
            return Err(Error::Input);
        }
    };
    // bind target action to the actual restored archive candidate and complete terminal assembly
    if family != assembly_family
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &NativeWirePrimitives,
            &installed.candidate.candidate_bytes,
        )) != candidate_sha256
        || installed.candidate.candidate_graph_sha256 != candidate_graph_sha256
    {
        return Err(Error::Candidate);
    }
    let artifact_sha256 = artifact_identity(
        family,
        &installed.candidate.candidate_bytes,
        &installed.candidate.artifact_bytes,
    )?;
    // cross-bind the restored candidate to its installed shadow and owner-access evidence
    if text(
        &installed.access.shadow_registration.registration,
        "candidateSha256",
    )
    .as_deref()
        != Some(candidate_sha256.as_str())
        || text(
            &installed.access.shadow_registration.registration,
            "artifactSha256",
        )
        .as_deref()
            != Some(artifact_sha256.as_str())
        || text(action, "fullMemberRootSha256")
            != text(&installed.full_manifest, "fullMemberRootSha256")
        || text(action, "reportCreatedAt") != text(&installed.access.native_access, "accessedAt")
        || text(action, "lifecycleHeadSha256")
            != text(&installed.access.native_access, "journalHeadSha256")
    {
        return Err(Error::Candidate);
    }
    let mut files = serving_family_files(
        family,
        &built.action_sha256,
        &candidate_sha256,
        &artifact_sha256,
        &installed.candidate.candidate_bytes,
        &installed.candidate.artifact_bytes,
    )?;
    validate_parity(
        &installed.candidate.parity_receipt_bytes,
        family,
        &candidate_sha256,
    )?;
    files.push(NativeMaintenanceDevelopmentPublicationFile {
        path: built.action_path.clone(),
        bytes: built.action_bytes.clone(),
    });
    files.push(NativeMaintenanceDevelopmentPublicationFile {
        path: parity_path(family, &candidate_sha256),
        bytes: installed.candidate.parity_receipt_bytes.clone(),
    });
    finish_package(
        built,
        files,
        Vec::new(),
        text(action, "expectedSourceCommit").ok_or(Error::Action)?,
        candidate_sha256,
    )
}

/// Read and validate the exact incumbent family matrix from the authenticated deployed commit.
pub fn read_native_maintenance_qualified_compensation_baseline(
    current: &NativeFamilyReleaseCurrent,
    family: NativeMaintenanceCaptureFamily,
    source: &mut impl NativeMaintenanceQualifiedBaselineFiles,
) -> Result<
    NativeMaintenanceQualifiedCompensationBaseline,
    NativeMaintenanceQualifiedReleasePackageError,
> {
    use NativeMaintenanceQualifiedReleasePackageError as Error;

    // bind the baseline observation to the selected family and authenticated source status
    if current.contract_version != "adjustment-family-release-current-status/v1"
        || current.family != family.wire()
        || !valid_commit(&current.commit)
    {
        return Err(Error::Baseline);
    }
    // legacy or raw incumbents intentionally compensate through the explicit raw registry
    if current.active_installed_receipt_sha256.is_none() {
        return Ok(NativeMaintenanceQualifiedCompensationBaseline::Raw);
    }
    let registry_path = registry_coordinates(family).1;
    let registry_bytes =
        source.read_source_file(&current.commit, registry_path, PACKAGE_MAXIMUM_BYTES)?;
    let registry = canonical_object(&registry_bytes).ok_or(Error::Baseline)?;
    validate_registry_shape(&registry, family, None, None, None)?;
    let active = field(&registry, "activePackage").ok_or(Error::Baseline)?;
    let action_sha256 = text(active, "actionSha256").ok_or(Error::Baseline)?;
    let artifact_sha256 = text(active, "artifactSha256").ok_or(Error::Baseline)?;
    let candidate_sha256 = text(active, "candidateSha256").ok_or(Error::Baseline)?;
    let action_path =
        format!("config/forecast-adjustments/ballydidean/actions/sha256-{action_sha256}.json");
    let action_bytes =
        source.read_source_file(&current.commit, &action_path, PACKAGE_MAXIMUM_BYTES)?;
    let action = canonical_object(&action_bytes).ok_or(Error::Baseline)?;
    let built = admit_action(&action, None)?;
    // admit only prior serving actions and bind selector identities to that exact action
    if !matches!(
        text(&action, "actionKind").as_deref(),
        Some("promote" | "rollback_prior" | "compensate_incumbent")
    ) || built.action_sha256 != action_sha256
        || built.family != family
        || text(&action, "candidateSha256").as_deref() != Some(candidate_sha256.as_str())
    {
        return Err(Error::Baseline);
    }
    validate_registry_shape(
        &registry,
        family,
        Some(&action_sha256),
        Some(&candidate_sha256),
        Some(&artifact_sha256),
    )?;
    let parity_path = parity_path(family, &candidate_sha256);
    let parity_receipt_bytes =
        source.read_source_file(&current.commit, &parity_path, PARITY_MAXIMUM_BYTES)?;
    validate_parity(&parity_receipt_bytes, family, &candidate_sha256)?;
    let family_files = read_incumbent_family_files(
        source,
        &current.commit,
        family,
        &candidate_sha256,
        &artifact_sha256,
    )?;
    Ok(NativeMaintenanceQualifiedCompensationBaseline::Incumbent {
        artifact_sha256,
        candidate_graph_sha256: text(&action, "candidateGraphSha256").ok_or(Error::Baseline)?,
        candidate_sha256,
        family_files,
        parity_receipt_bytes,
    })
}

/// Build the post-target compensation package from the frozen pre-target baseline.
pub fn build_native_maintenance_qualified_release_compensation(
    target: &NativeMaintenanceQualifiedReleasePackage,
    target_commit_sha: &str,
    target_release_tag: &str,
    baseline: &NativeMaintenanceQualifiedCompensationBaseline,
) -> Result<NativeMaintenanceQualifiedReleasePackage, NativeMaintenanceQualifiedReleasePackageError>
{
    use NativeMaintenanceQualifiedReleasePackageError as Error;

    let target_action = admit_action(&target.action, Some("promote"))?;
    // require immutable published target coordinates before rebasing recovery authority
    if target_action.action_sha256 != target.action_sha256
        || !valid_commit(target_commit_sha)
        || !valid_release(target_release_tag)
    {
        return Err(Error::Action);
    }
    let (kind, reason, candidate_graph, candidate, mut files, parity) = match baseline {
        NativeMaintenanceQualifiedCompensationBaseline::Raw => (
            "compensate_raw",
            "invalid_incumbent",
            None,
            None,
            Vec::new(),
            None,
        ),
        NativeMaintenanceQualifiedCompensationBaseline::Incumbent {
            candidate_graph_sha256,
            candidate_sha256,
            family_files,
            parity_receipt_bytes,
            ..
        } => (
            "compensate_incumbent",
            "failed_promotion",
            Some(candidate_graph_sha256.as_str()),
            Some(candidate_sha256.as_str()),
            family_files.clone(),
            Some(parity_receipt_bytes.as_slice()),
        ),
    };
    let action = compensation_action(
        &target.action,
        kind,
        reason,
        candidate_graph,
        candidate,
        target_commit_sha,
        target_release_tag,
        &target.action_sha256,
    )?;
    let built = admit_action(&action, Some(kind))?;
    let registry = match baseline {
        NativeMaintenanceQualifiedCompensationBaseline::Raw => raw_registry(target.family)?,
        NativeMaintenanceQualifiedCompensationBaseline::Incumbent {
            artifact_sha256,
            candidate_sha256,
            ..
        } => serving_registry(
            target.family,
            &built.action_sha256,
            candidate_sha256,
            artifact_sha256,
        )?,
    };
    files.push(registry);
    files.push(NativeMaintenanceDevelopmentPublicationFile {
        path: built.action_path.clone(),
        bytes: built.action_bytes.clone(),
    });
    // retain the exact prior parity receipt only for an incumbent restoration
    if let (Some(candidate_sha256), Some(parity)) = (candidate, parity) {
        validate_parity(parity, target.family, candidate_sha256)?;
        files.push(NativeMaintenanceDevelopmentPublicationFile {
            path: parity_path(target.family, candidate_sha256),
            bytes: parity.to_vec(),
        });
    }
    let target_identity = candidate
        .map(str::to_owned)
        .unwrap_or_else(|| built.action_sha256.clone());
    finish_package(
        built,
        files,
        Vec::new(),
        target_commit_sha.to_owned(),
        target_identity,
    )
}

struct BuiltAction {
    action: JsonValue,
    action_bytes: Vec<u8>,
    action_path: String,
    action_sha256: String,
    family: NativeMaintenanceCaptureFamily,
}

/// Admit one exact source action and reproduce its hash-addressed path.
fn admit_action(
    action: &JsonValue,
    expected_kind: Option<&str>,
) -> Result<BuiltAction, NativeMaintenanceQualifiedReleasePackageError> {
    use NativeMaintenanceQualifiedReleasePackageError::Action;

    let family = family(action).ok_or(Action)?;
    let kind = text(action, "actionKind").ok_or(Action)?;
    let candidate = nullable_hash(action, "candidateSha256").ok_or(Action)?;
    let graph = nullable_hash(action, "candidateGraphSha256").ok_or(Action)?;
    let predecessor = nullable_hash(action, "predecessorActionSha256").ok_or(Action)?;
    let full_member = nullable_hash(action, "fullMemberRootSha256").ok_or(Action)?;
    let _expected_receipt =
        nullable_hash(action, "expectedInstalledReceiptSha256").ok_or(Action)?;
    let compensation = matches!(kind.as_str(), "compensate_incumbent" | "compensate_raw");
    let candidate_target = matches!(
        kind.as_str(),
        "promote" | "rollback_prior" | "compensate_incumbent"
    );
    let reason = match kind.as_str() {
        "promote" => "qualified_candidate",
        "rollback_prior" => "qualified_prior",
        "compensate_incumbent" => "failed_promotion",
        "compensate_raw" => "invalid_incumbent",
        _ => return Err(Action),
    };
    let decision = text(action, "policyDecision").ok_or(Action)?;
    let decision_matches = match kind.as_str() {
        "promote" => decision == "qualified",
        "rollback_prior" => decision == "regressed",
        "compensate_incumbent" | "compensate_raw" => {
            matches!(decision.as_str(), "qualified" | "regressed")
        }
        _ => false,
    };
    let wire = NativeWirePrimitives;
    let report_at = instant(action, "reportCreatedAt").ok_or(Action)?;
    let created_at = instant(action, "createdAt").ok_or(Action)?;
    let valid_through = instant(action, "validThrough").ok_or(Action)?;
    // retain the complete source action matrix and seven-day report validity binding
    if !exact_keys(action, ACTION_KEYS)
        || text(action, "contractVersion").as_deref() != Some(ACTION_VERSION)
        || text(action, "siteKey").as_deref() != Some("ballydidean")
        || expected_kind.is_some_and(|expected| kind != expected)
        || !decision_matches
        || text(action, "reason").as_deref() != Some(reason)
        || candidate.is_some() != graph.is_some()
        || candidate_target != candidate.is_some()
        || compensation != predecessor.is_some()
        || full_member.is_none()
        || !text(action, "expectedSettingsSha256").is_some_and(|value| valid_hash(&value))
        || !text(action, "lifecycleHeadSha256").is_some_and(|value| valid_hash(&value))
        || !text(action, "policyReportSha256").is_some_and(|value| valid_hash(&value))
        || !text(action, "expectedSourceCommit").is_some_and(|value| valid_commit(&value))
        || !text(action, "expectedSourceRelease").is_some_and(|value| valid_release(&value))
        || !text(action, "fencingToken").is_some_and(|value| valid_fencing_token(&value))
        || report_at > created_at
        || valid_through.checked_sub(report_at) != Some(7 * DAY_MILLISECONDS)
        || (!compensation && created_at > valid_through)
    {
        return Err(Action);
    }
    let action_bytes = wire.encode_sorted_json(action).ok_or(Action)?;
    let action_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, &action_bytes));
    Ok(BuiltAction {
        action: action.clone(),
        action_path: format!(
            "config/forecast-adjustments/ballydidean/actions/sha256-{action_sha256}.json"
        ),
        action_bytes,
        action_sha256,
        family,
    })
}

/// Derive and verify the candidate-addressed artifact retained by each family package.
fn artifact_identity(
    family: NativeMaintenanceCaptureFamily,
    candidate_bytes: &[u8],
    artifact_bytes: &[u8],
) -> Result<String, NativeMaintenanceQualifiedReleasePackageError> {
    use NativeMaintenanceQualifiedReleasePackageError::Candidate;

    match family {
        NativeMaintenanceCaptureFamily::Temperature => {
            let (rebuilt, _, identity) =
                build_native_temperature_maintenance_package(candidate_bytes)
                    .map_err(|_| Candidate)?;
            (rebuilt == artifact_bytes)
                .then_some(identity)
                .ok_or(Candidate)
        }
        NativeMaintenanceCaptureFamily::Wind => {
            let rebuilt = build_wind_portable_candidate(candidate_bytes).map_err(|_| Candidate)?;
            (rebuilt.artifact_bytes == artifact_bytes)
                .then_some(rebuilt.artifact_sha256)
                .ok_or(Candidate)
        }
        NativeMaintenanceCaptureFamily::Rain => {
            let rebuilt =
                build_native_rain_maintenance_artifact(candidate_bytes).map_err(|_| Candidate)?;
            (rebuilt.artifact_bytes == artifact_bytes)
                .then_some(rebuilt.artifact_sha256)
                .ok_or(Candidate)
        }
    }
}

/// Build the active family byte matrix, including rain's exact compiled execution source.
fn serving_family_files(
    family: NativeMaintenanceCaptureFamily,
    action_sha256: &str,
    candidate_sha256: &str,
    artifact_sha256: &str,
    candidate_bytes: &[u8],
    artifact_bytes: &[u8],
) -> Result<
    Vec<NativeMaintenanceDevelopmentPublicationFile>,
    NativeMaintenanceQualifiedReleasePackageError,
> {
    let registry = serving_registry(family, action_sha256, candidate_sha256, artifact_sha256)?;
    // rain retains candidate receipt and generated TypeScript beside its portable artifact
    if family == NativeMaintenanceCaptureFamily::Rain {
        let mut files = rain_technical_files(
            candidate_bytes,
            artifact_bytes,
            candidate_sha256,
            artifact_sha256,
        )?;
        files.push(registry);
        return Ok(files);
    }
    let directory = match family {
        NativeMaintenanceCaptureFamily::Temperature => "temperature-canary-bundles",
        NativeMaintenanceCaptureFamily::Wind => "wind-canary-bundles",
        NativeMaintenanceCaptureFamily::Rain => unreachable!(),
    };
    Ok(vec![
        NativeMaintenanceDevelopmentPublicationFile {
            path: format!(
                "config/forecast-adjustments/ballydidean/{directory}/sha256-{artifact_sha256}.json"
            ),
            bytes: artifact_bytes.to_vec(),
        },
        registry,
    ])
}

/// Build rain artifact, candidate receipt, and generated source with source-exact bytes.
fn rain_technical_files(
    candidate_bytes: &[u8],
    artifact_bytes: &[u8],
    candidate_sha256: &str,
    artifact_sha256: &str,
) -> Result<
    Vec<NativeMaintenanceDevelopmentPublicationFile>,
    NativeMaintenanceQualifiedReleasePackageError,
> {
    use NativeMaintenanceQualifiedReleasePackageError as Error;

    let artifact = canonical_object(artifact_bytes).ok_or(Error::Candidate)?;
    let model_month = text(&artifact, "modelMonth").ok_or(Error::Candidate)?;
    let compiled = compiled_rain_source(artifact_bytes, artifact_sha256)?;
    let technical_registry = object(vec![
        (
            "activeArtifact",
            object(vec![("artifactSha256", string(artifact_sha256))]),
        ),
        (
            "contractVersion",
            string("forecast-adjustment-rain-runtime-registry/v1"),
        ),
        ("rawReason", JsonValue::Null),
        ("siteKey", string("ballydidean")),
    ]);
    let wire = NativeWirePrimitives;
    let technical_registry_bytes = wire
        .encode_sorted_json(&technical_registry)
        .ok_or(Error::Codec)?;
    // rebind the raw fit and supplied artifact before producing serving source bytes
    if format_digest(RevisionArchiveCanonicalCodec::sha256(
        &wire,
        candidate_bytes,
    )) != candidate_sha256
        || format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, artifact_bytes))
            != artifact_sha256
    {
        return Err(Error::Candidate);
    }
    let receipt = object(vec![
        (
            "artifactPath",
            string(&format!(
                "rain-runtime-artifacts/sha256-{artifact_sha256}.json"
            )),
        ),
        ("artifactSha256", string(artifact_sha256)),
        ("candidateSha256", string(candidate_sha256)),
        (
            "compiledSourcePath",
            string("packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts"),
        ),
        (
            "compiledSourceSha256",
            string(&format_digest(RevisionArchiveCanonicalCodec::sha256(
                &wire, &compiled,
            ))),
        ),
        (
            "contractVersion",
            string("forecast-adjustment-rain-model-package/v1"),
        ),
        ("modelMonth", string(&model_month)),
        (
            "registrySha256",
            string(&format_digest(RevisionArchiveCanonicalCodec::sha256(
                &wire,
                &technical_registry_bytes,
            ))),
        ),
    ]);
    let receipt_bytes = wire.encode_sorted_json(&receipt).ok_or(Error::Codec)?;
    Ok(vec![
        NativeMaintenanceDevelopmentPublicationFile {
            path: format!(
                "config/forecast-adjustments/ballydidean/rain-runtime-artifacts/sha256-{artifact_sha256}.json"
            ),
            bytes: artifact_bytes.to_vec(),
        },
        NativeMaintenanceDevelopmentPublicationFile {
            path: format!(
                "config/forecast-adjustments/ballydidean/rain-model-packages/sha256-{candidate_sha256}.json"
            ),
            bytes: receipt_bytes,
        },
        NativeMaintenanceDevelopmentPublicationFile {
            path: "packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts".to_owned(),
            bytes: compiled,
        },
    ])
}

/// Build one action-bound active serving selector.
fn serving_registry(
    family: NativeMaintenanceCaptureFamily,
    action_sha256: &str,
    candidate_sha256: &str,
    artifact_sha256: &str,
) -> Result<
    NativeMaintenanceDevelopmentPublicationFile,
    NativeMaintenanceQualifiedReleasePackageError,
> {
    use NativeMaintenanceQualifiedReleasePackageError::Package;

    // require all public selector coordinates before deriving its fixed path
    if !valid_hash(action_sha256) || !valid_hash(candidate_sha256) || !valid_hash(artifact_sha256) {
        return Err(Package);
    }
    let (directory, path, contract) = registry_coordinates(family);
    let registry = object(vec![
        (
            "activePackage",
            object(vec![
                ("actionSha256", string(action_sha256)),
                ("artifactSha256", string(artifact_sha256)),
                ("candidateSha256", string(candidate_sha256)),
                (
                    "path",
                    string(&format!("{directory}/sha256-{artifact_sha256}.json")),
                ),
            ]),
        ),
        ("contractVersion", string(contract)),
        ("rawReason", JsonValue::Null),
        ("siteKey", string("ballydidean")),
    ]);
    Ok(NativeMaintenanceDevelopmentPublicationFile {
        path: path.to_owned(),
        bytes: NativeWirePrimitives
            .encode_sorted_json(&registry)
            .ok_or(NativeMaintenanceQualifiedReleasePackageError::Codec)?,
    })
}

/// Build one explicit action-bound raw selector.
fn raw_registry(
    family: NativeMaintenanceCaptureFamily,
) -> Result<
    NativeMaintenanceDevelopmentPublicationFile,
    NativeMaintenanceQualifiedReleasePackageError,
> {
    let (_, path, contract) = registry_coordinates(family);
    let registry = object(vec![
        ("activePackage", JsonValue::Null),
        ("contractVersion", string(contract)),
        ("rawReason", string("policy_raw")),
        ("siteKey", string("ballydidean")),
    ]);
    Ok(NativeMaintenanceDevelopmentPublicationFile {
        path: path.to_owned(),
        bytes: NativeWirePrimitives
            .encode_sorted_json(&registry)
            .ok_or(NativeMaintenanceQualifiedReleasePackageError::Codec)?,
    })
}

/// Return the family bundle directory, fixed registry path, and maintenance contract.
fn registry_coordinates(
    family: NativeMaintenanceCaptureFamily,
) -> (&'static str, &'static str, &'static str) {
    match family {
        NativeMaintenanceCaptureFamily::Temperature => (
            "temperature-canary-bundles",
            "config/forecast-adjustments/ballydidean-temperature-canary.json",
            "forecast-adjustment-temperature-maintenance-registry/v1",
        ),
        NativeMaintenanceCaptureFamily::Wind => (
            "wind-canary-bundles",
            "config/forecast-adjustments/ballydidean-wind-canary.json",
            "forecast-adjustment-wind-maintenance-registry/v1",
        ),
        NativeMaintenanceCaptureFamily::Rain => (
            "rain-runtime-artifacts",
            "config/forecast-adjustments/ballydidean-rain-runtime.json",
            "forecast-adjustment-rain-maintenance-registry/v1",
        ),
    }
}

/// Read the exact immutable technical matrix selected by one incumbent registry.
fn read_incumbent_family_files(
    source: &mut impl NativeMaintenanceQualifiedBaselineFiles,
    commit: &str,
    family: NativeMaintenanceCaptureFamily,
    candidate_sha256: &str,
    artifact_sha256: &str,
) -> Result<
    Vec<NativeMaintenanceDevelopmentPublicationFile>,
    NativeMaintenanceQualifiedReleasePackageError,
> {
    use NativeMaintenanceQualifiedReleasePackageError as Error;

    if family != NativeMaintenanceCaptureFamily::Rain {
        let (directory, _, _) = registry_coordinates(family);
        let path = format!(
            "config/forecast-adjustments/ballydidean/{directory}/sha256-{artifact_sha256}.json"
        );
        let bytes = source.read_source_file(commit, &path, PACKAGE_MAXIMUM_BYTES)?;
        validate_incumbent_bundle(&bytes, family, candidate_sha256, artifact_sha256)?;
        return Ok(vec![NativeMaintenanceDevelopmentPublicationFile {
            path,
            bytes,
        }]);
    }
    let receipt_path = format!(
        "config/forecast-adjustments/ballydidean/rain-model-packages/sha256-{candidate_sha256}.json"
    );
    let receipt_bytes = source.read_source_file(commit, &receipt_path, PACKAGE_MAXIMUM_BYTES)?;
    let receipt = canonical_object(&receipt_bytes).ok_or(Error::Baseline)?;
    let contract = text(&receipt, "contractVersion").ok_or(Error::Baseline)?;
    let v2 = contract == "forecast-adjustment-rain-model-package/v2";
    let expected_keys: &[&str] = if v2 {
        &[
            "artifactPath",
            "artifactSha256",
            "candidateSha256",
            "compiledSourcePath",
            "compiledSourceSha256",
            "contractVersion",
            "controlStatePath",
            "controlStateSha256",
            "modelMonth",
            "ordinalArtifactPath",
            "ordinalArtifactSha256",
            "registrySha256",
        ]
    } else {
        &[
            "artifactPath",
            "artifactSha256",
            "candidateSha256",
            "compiledSourcePath",
            "compiledSourceSha256",
            "contractVersion",
            "modelMonth",
            "registrySha256",
        ]
    };
    let expected_artifact_path = format!("rain-runtime-artifacts/sha256-{artifact_sha256}.json");
    // close either the legacy or fixed-control rain receipt shape
    if !exact_keys(&receipt, expected_keys)
        || !(contract == "forecast-adjustment-rain-model-package/v1" || v2)
        || text(&receipt, "candidateSha256").as_deref() != Some(candidate_sha256)
        || text(&receipt, "artifactSha256").as_deref() != Some(artifact_sha256)
        || text(&receipt, "artifactPath").as_deref() != Some(expected_artifact_path.as_str())
        || text(&receipt, "compiledSourcePath").as_deref()
            != Some("packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts")
        || !text(&receipt, "compiledSourceSha256").is_some_and(|value| valid_hash(&value))
        || !text(&receipt, "registrySha256").is_some_and(|value| valid_hash(&value))
    {
        return Err(Error::Baseline);
    }
    let artifact_path = format!(
        "config/forecast-adjustments/ballydidean/{}",
        text(&receipt, "artifactPath").ok_or(Error::Baseline)?
    );
    let compiled_path = text(&receipt, "compiledSourcePath").ok_or(Error::Baseline)?;
    let artifact = source.read_source_file(commit, &artifact_path, PACKAGE_MAXIMUM_BYTES)?;
    let compiled = source.read_source_file(commit, &compiled_path, PACKAGE_MAXIMUM_BYTES)?;
    let artifact_month = admit_rain_artifact_month(&artifact)?;
    let technical_registry = object(vec![
        (
            "activeArtifact",
            object(vec![("artifactSha256", string(artifact_sha256))]),
        ),
        (
            "contractVersion",
            string("forecast-adjustment-rain-runtime-registry/v1"),
        ),
        ("rawReason", JsonValue::Null),
        ("siteKey", string("ballydidean")),
    ]);
    let technical_registry_bytes = NativeWirePrimitives
        .encode_sorted_json(&technical_registry)
        .ok_or(Error::Codec)?;
    let compiled_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &compiled,
    ));
    let technical_registry_sha256 = format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &technical_registry_bytes,
    ));
    // require exact retained hashes and reproducible compiled source
    if format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        &artifact,
    )) != artifact_sha256
        || text(&receipt, "compiledSourceSha256").as_deref() != Some(compiled_sha256.as_str())
        || compiled_rain_source(&artifact, artifact_sha256)? != compiled
        || text(&receipt, "modelMonth").as_deref() != Some(artifact_month.as_str())
        || text(&receipt, "registrySha256").as_deref() != Some(technical_registry_sha256.as_str())
    {
        return Err(Error::Baseline);
    }
    let mut files = vec![
        NativeMaintenanceDevelopmentPublicationFile {
            path: artifact_path,
            bytes: artifact,
        },
        NativeMaintenanceDevelopmentPublicationFile {
            path: receipt_path,
            bytes: receipt_bytes,
        },
        NativeMaintenanceDevelopmentPublicationFile {
            path: compiled_path,
            bytes: compiled,
        },
    ];
    // retain both independently addressed fixed-control files for a v2 incumbent
    if v2 {
        for (path_key, hash_key) in [
            ("controlStatePath", "controlStateSha256"),
            ("ordinalArtifactPath", "ordinalArtifactSha256"),
        ] {
            let relative = text(&receipt, path_key).ok_or(Error::Baseline)?;
            let expected = text(&receipt, hash_key).ok_or(Error::Baseline)?;
            let path = format!("config/forecast-adjustments/ballydidean/{relative}");
            // collapse the selected artifact alias only when its exact address agrees
            if files.iter().any(|file| file.path == path) {
                if expected != artifact_sha256 {
                    return Err(Error::Baseline);
                }
                continue;
            }
            let bytes = source.read_source_file(commit, &path, PACKAGE_MAXIMUM_BYTES)?;
            if format_digest(RevisionArchiveCanonicalCodec::sha256(
                &NativeWirePrimitives,
                &bytes,
            )) != expected
            {
                return Err(Error::Baseline);
            }
            files.push(NativeMaintenanceDevelopmentPublicationFile { path, bytes });
        }
        validate_rain_control_files(&receipt, &files, &artifact_month)?;
    }
    Ok(files)
}

/// Admit one canonical rain portable artifact and retain its source model month.
fn admit_rain_artifact_month(
    bytes: &[u8],
) -> Result<String, NativeMaintenanceQualifiedReleasePackageError> {
    use NativeMaintenanceQualifiedReleasePackageError::Baseline;

    let values = NativeModelEvidenceValues::new();
    let mut wire = NativeWirePrimitives;
    let document = crate::MaintenanceCanonicalJson::parse_canonical_object(&mut wire, bytes)
        .ok_or(Baseline)?;
    let artifact = admit_rain_portable_artifact(
        &document,
        |value| values.decode_shadow_rain_artifact(value),
        |feature_names| {
            let bytes = values.encode_shadow_ordered_json(feature_names)?;
            ApiMaintenanceReceiptCanonicalValues::sha256(&values, &bytes)
        },
    )
    .ok_or(Baseline)?;
    Ok(match artifact {
        RainHurdleWindPortableArtifact::V1(value) => value.modelMonth,
        RainHurdleWindPortableArtifact::V2(value) => value.modelMonth,
    })
}

/// Validate one v2 rain control state and its separately retained ordinal artifact.
fn validate_rain_control_files(
    receipt: &JsonValue,
    files: &[NativeMaintenanceDevelopmentPublicationFile],
    model_month: &str,
) -> Result<(), NativeMaintenanceQualifiedReleasePackageError> {
    use NativeMaintenanceQualifiedReleasePackageError::Baseline;

    let control_path = format!(
        "config/forecast-adjustments/ballydidean/{}",
        text(receipt, "controlStatePath").ok_or(Baseline)?
    );
    let ordinal_path = format!(
        "config/forecast-adjustments/ballydidean/{}",
        text(receipt, "ordinalArtifactPath").ok_or(Baseline)?
    );
    let control = files
        .iter()
        .find(|file| file.path == control_path)
        .ok_or(Baseline)?;
    let ordinal = files
        .iter()
        .find(|file| file.path == ordinal_path)
        .ok_or(Baseline)?;
    let values = NativeModelEvidenceValues::new();
    let state = admit_rain_maintenance_control_state(
        &crate::api_maintenance_admission::ApiRainControlPrimitives(&values),
        &control.bytes,
    )
    .ok_or(Baseline)?;
    let ordinal_month = admit_rain_artifact_month(&ordinal.bytes)?;
    // bind both control documents to the receipt month and retained ordinal address
    if state.model_month != model_month
        || ordinal_month != model_month
        || state.ordinal_artifact_sha256
            != text(receipt, "ordinalArtifactSha256").ok_or(Baseline)?
    {
        return Err(Baseline);
    }
    Ok(())
}

/// Validate one active registry shape and its optional exact action/candidate/artifact binding.
fn validate_registry_shape(
    registry: &JsonValue,
    family: NativeMaintenanceCaptureFamily,
    action: Option<&str>,
    candidate: Option<&str>,
    artifact: Option<&str>,
) -> Result<(), NativeMaintenanceQualifiedReleasePackageError> {
    use NativeMaintenanceQualifiedReleasePackageError::Baseline;

    const REGISTRY_KEYS: &[&str] = &["activePackage", "contractVersion", "rawReason", "siteKey"];
    const ACTIVE_KEYS: &[&str] = &["actionSha256", "artifactSha256", "candidateSha256", "path"];
    let (directory, _, contract) = registry_coordinates(family);
    let active = field(registry, "activePackage").ok_or(Baseline)?;
    // retain only an active maintenance selector with the source's closed fields
    if !exact_keys(registry, REGISTRY_KEYS)
        || !exact_keys(active, ACTIVE_KEYS)
        || text(registry, "contractVersion").as_deref() != Some(contract)
        || text(registry, "siteKey").as_deref() != Some("ballydidean")
        || !matches!(field(registry, "rawReason"), Some(JsonValue::Null))
    {
        return Err(Baseline);
    }
    for key in ["actionSha256", "artifactSha256", "candidateSha256"] {
        if !text(active, key).is_some_and(|value| valid_hash(&value)) {
            return Err(Baseline);
        }
    }
    let selected_artifact = text(active, "artifactSha256").ok_or(Baseline)?;
    let expected_path = format!("{directory}/sha256-{selected_artifact}.json");
    // apply exact cross-bindings only after the caller has admitted the prior action
    if action.is_some_and(|value| text(active, "actionSha256").as_deref() != Some(value))
        || candidate.is_some_and(|value| text(active, "candidateSha256").as_deref() != Some(value))
        || artifact.is_some_and(|value| text(active, "artifactSha256").as_deref() != Some(value))
        || text(active, "path").as_deref() != Some(expected_path.as_str())
    {
        return Err(Baseline);
    }
    Ok(())
}

/// Validate one incumbent temperature or wind package and its self-addressed unsigned body.
fn validate_incumbent_bundle(
    bytes: &[u8],
    family: NativeMaintenanceCaptureFamily,
    candidate_sha256: &str,
    artifact_sha256: &str,
) -> Result<(), NativeMaintenanceQualifiedReleasePackageError> {
    use NativeMaintenanceQualifiedReleasePackageError::Baseline;

    let bundle = canonical_object(bytes).ok_or(Baseline)?;
    let JsonValue::Object(fields) = &bundle else {
        return Err(Baseline);
    };
    let unsigned = JsonValue::Object(
        fields
            .iter()
            .filter(|(key, _)| key.iter().copied().ne("bundleSha256".encode_utf16()))
            .cloned()
            .collect(),
    );
    let unsigned_bytes = NativeWirePrimitives
        .encode_sorted_json(&unsigned)
        .ok_or(NativeMaintenanceQualifiedReleasePackageError::Codec)?;
    // retain only authority-free monthly runtime packages from prior qualified publications
    if text(&bundle, "contractVersion").as_deref()
        != Some("forecast-adjustment-maintenance-runtime-package/v1")
        || text(&bundle, "family").as_deref() != Some(family.wire())
        || text(&bundle, "candidateSha256").as_deref() != Some(candidate_sha256)
        || text(&bundle, "bundleSha256").as_deref() != Some(artifact_sha256)
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &NativeWirePrimitives,
            &unsigned_bytes,
        )) != artifact_sha256
    {
        return Err(Baseline);
    }
    Ok(())
}

/// Validate one canonical immutable parity receipt against its family candidate.
fn validate_parity(
    bytes: &[u8],
    family: NativeMaintenanceCaptureFamily,
    candidate_sha256: &str,
) -> Result<(), NativeMaintenanceQualifiedReleasePackageError> {
    use NativeMaintenanceQualifiedReleasePackageError::Candidate;

    let value = canonical_object(bytes).ok_or(Candidate)?;
    // close the receipt and bind every immutable hash field
    if !exact_keys(&value, PARITY_KEYS)
        || text(&value, "contractVersion").as_deref() != Some(PARITY_VERSION)
        || text(&value, "family").as_deref() != Some(family.wire())
        || text(&value, "candidateSha256").as_deref() != Some(candidate_sha256)
        || PARITY_KEYS
            .iter()
            .filter(|key| key.ends_with("Sha256"))
            .any(|key| text(&value, key).is_none_or(|hash| !valid_hash(&hash)))
    {
        return Err(Candidate);
    }
    Ok(())
}

/// Derive the exact compensation action by replacing only source-owned target coordinates.
#[allow(clippy::too_many_arguments)]
fn compensation_action(
    target: &JsonValue,
    kind: &str,
    reason: &str,
    candidate_graph: Option<&str>,
    candidate: Option<&str>,
    source_commit: &str,
    source_release: &str,
    predecessor_action: &str,
) -> Result<JsonValue, NativeMaintenanceQualifiedReleasePackageError> {
    let JsonValue::Object(fields) = target else {
        return Err(NativeMaintenanceQualifiedReleasePackageError::Action);
    };
    let mut action = JsonValue::Object(fields.clone());
    for (key, value) in [
        ("actionKind", string(kind)),
        (
            "candidateGraphSha256",
            candidate_graph.map_or(JsonValue::Null, string),
        ),
        ("candidateSha256", candidate.map_or(JsonValue::Null, string)),
        ("expectedSourceCommit", string(source_commit)),
        ("expectedSourceRelease", string(source_release)),
        ("predecessorActionSha256", string(predecessor_action)),
        ("reason", string(reason)),
    ] {
        set_field(&mut action, key, value)?;
    }
    Ok(action)
}

/// Replace one known action field without altering any other retained coordinate.
fn set_field(
    value: &mut JsonValue,
    key: &str,
    replacement: JsonValue,
) -> Result<(), NativeMaintenanceQualifiedReleasePackageError> {
    let JsonValue::Object(fields) = value else {
        return Err(NativeMaintenanceQualifiedReleasePackageError::Action);
    };
    let key = key.encode_utf16().collect::<Vec<_>>();
    let Some((_, current)) = fields.iter_mut().find(|(name, _)| *name == key) else {
        return Err(NativeMaintenanceQualifiedReleasePackageError::Action);
    };
    *current = replacement;
    Ok(())
}

/// Sort, bound, and hash one complete public package operation matrix.
fn finish_package(
    built: BuiltAction,
    mut files: Vec<NativeMaintenanceDevelopmentPublicationFile>,
    removed_paths: Vec<String>,
    source_commit: String,
    target_identity_sha256: String,
) -> Result<NativeMaintenanceQualifiedReleasePackage, NativeMaintenanceQualifiedReleasePackageError>
{
    use NativeMaintenanceQualifiedReleasePackageError as Error;

    files.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    let mut total = 0usize;
    let mut members = Vec::with_capacity(files.len() + removed_paths.len());
    let mut previous_path: Option<&str> = None;
    // bind each exact public write operation under the shared package ceiling
    for file in &files {
        total = total.checked_add(file.bytes.len()).ok_or(Error::Package)?;
        if file.bytes.is_empty()
            || file.bytes.len() > PACKAGE_MAXIMUM_BYTES
            || total > PACKAGE_MAXIMUM_BYTES
            || !valid_public_path(&file.path)
            || previous_path == Some(file.path.as_str())
        {
            return Err(Error::Package);
        }
        previous_path = Some(&file.path);
        members.push(object(vec![
            ("bytes", JsonValue::Number(file.bytes.len() as f64)),
            ("path", string(&file.path)),
            (
                "sha256",
                string(&format_digest(RevisionArchiveCanonicalCodec::sha256(
                    &NativeWirePrimitives,
                    &file.bytes,
                ))),
            ),
        ]));
    }
    // qualified model packages have no deletion operations
    if !removed_paths.is_empty() {
        return Err(Error::Package);
    }
    let root_bytes = NativeWirePrimitives
        .encode_sorted_json(&JsonValue::Array(members))
        .ok_or(Error::Codec)?;
    Ok(NativeMaintenanceQualifiedReleasePackage {
        action: built.action,
        action_bytes: built.action_bytes,
        action_path: built.action_path,
        action_sha256: built.action_sha256,
        family: built.family,
        package_files: files,
        package_root_sha256: format_digest(RevisionArchiveCanonicalCodec::sha256(
            &NativeWirePrimitives,
            &root_bytes,
        )),
        package_total_bytes: total,
        removed_paths,
        source_commit,
        target_identity_sha256,
    })
}

/// Reproduce the generated rain TypeScript source without a Node runtime.
fn compiled_rain_source(
    artifact_bytes: &[u8],
    artifact_sha256: &str,
) -> Result<Vec<u8>, NativeMaintenanceQualifiedReleasePackageError> {
    let mut encoded = NativeWirePrimitives
        .encode_sorted_json(&string(
            std::str::from_utf8(artifact_bytes)
                .map_err(|_| NativeMaintenanceQualifiedReleasePackageError::Candidate)?,
        ))
        .ok_or(NativeMaintenanceQualifiedReleasePackageError::Codec)?;
    // remove canonical scalar framing because the string becomes a TypeScript literal
    if encoded.pop() != Some(b'\n') {
        return Err(NativeMaintenanceQualifiedReleasePackageError::Codec);
    }
    let mut output = Vec::new();
    output.extend_from_slice(
        b"// generated by adjustment_rain_model_package.mjs from a sanitized fit candidate\n",
    );
    output.extend_from_slice(
        format!(
            "export const RAIN_HURDLE_WIND_ARTIFACT_SHA256 = \"{artifact_sha256}\" as const;\n"
        )
        .as_bytes(),
    );
    output.extend_from_slice(b"export const RAIN_HURDLE_WIND_ARTIFACT_JSON = ");
    output.extend_from_slice(&encoded);
    output.extend_from_slice(b" as const;\n");
    Ok(output)
}

/// Parse one canonical top-level object without alternate whitespace or key order.
fn canonical_object(bytes: &[u8]) -> Option<JsonValue> {
    let value = NativeWirePrimitives.decode_json(bytes)?;
    matches!(value, JsonValue::Object(_))
        .then_some(())
        .and_then(|()| {
            (NativeWirePrimitives.encode_sorted_json(&value)?.as_slice() == bytes).then_some(value)
        })
}

/// Read one nullable hash while rejecting absent fields and non-string aliases.
fn nullable_hash(value: &JsonValue, key: &str) -> Option<Option<String>> {
    match field(value, key)? {
        JsonValue::Null => Some(None),
        JsonValue::String(_) => text(value, key).filter(|hash| valid_hash(hash)).map(Some),
        _ => None,
    }
}

/// Parse one closed family discriminator.
fn family(value: &JsonValue) -> Option<NativeMaintenanceCaptureFamily> {
    match text(value, "family")?.as_str() {
        "temperature" => Some(NativeMaintenanceCaptureFamily::Temperature),
        "wind" => Some(NativeMaintenanceCaptureFamily::Wind),
        "rain" => Some(NativeMaintenanceCaptureFamily::Rain),
        _ => None,
    }
}

/// Parse one canonical source instant.
fn instant(value: &JsonValue, key: &str) -> Option<i64> {
    NativeWirePrimitives.canonical_utc_milliseconds(&text(value, key)?)
}

/// Return the immutable parity receipt path for one family candidate.
fn parity_path(family: NativeMaintenanceCaptureFamily, candidate: &str) -> String {
    format!(
        "config/forecast-adjustments/ballydidean/model-parity/{}/sha256-{candidate}.json",
        family.wire()
    )
}

/// Recognize one exact lowercase Git object identity.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Recognize the immutable calendar release namespace.
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

/// Recognize one canonical positive unsigned 64-bit release fence.
fn valid_fencing_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

/// Restrict package writes to ordinary repository-relative public model paths.
fn valid_public_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 240
        && !value.starts_with('/')
        && !value.contains("..")
        && !value.contains("//")
        && !value.starts_with(".github/")
        && !value.contains('\\')
}

/// Restrict baseline reads to the predecessor's public model-member paths.
fn valid_source_path(value: &str) -> bool {
    valid_public_path(value)
        && (value.starts_with("config/forecast-adjustments/")
            || value == "packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts")
}

/// Run fixed Git while bounding retained stdout and discarding diagnostics.
fn run_git_bounded(
    repository: &Path,
    arguments: &[&str],
    maximum: usize,
) -> Result<Vec<u8>, NativeMaintenanceQualifiedReleasePackageError> {
    let mut child = Command::new("/usr/bin/git")
        .current_dir(repository)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| NativeMaintenanceQualifiedReleasePackageError::Git)?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or(NativeMaintenanceQualifiedReleasePackageError::Git)?;
    let mut bytes = Vec::with_capacity(maximum.min(4096));
    stdout
        .by_ref()
        .take(maximum.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| NativeMaintenanceQualifiedReleasePackageError::Git)?;
    // terminate and reap a producer that exceeded its admitted blob ceiling
    if bytes.len() > maximum {
        let _ = child.kill();
        let _ = child.wait();
        return Err(NativeMaintenanceQualifiedReleasePackageError::Git);
    }
    let status = child
        .wait()
        .map_err(|_| NativeMaintenanceQualifiedReleasePackageError::Git)?;
    if !status.success() {
        return Err(NativeMaintenanceQualifiedReleasePackageError::Git);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keep raw selectors byte-exact across all three source families.
    #[test]
    fn raw_registry_uses_fixed_family_contract_and_path() {
        for family in [
            NativeMaintenanceCaptureFamily::Temperature,
            NativeMaintenanceCaptureFamily::Wind,
            NativeMaintenanceCaptureFamily::Rain,
        ] {
            let file = raw_registry(family).expect("raw registry");
            let value = canonical_object(&file.bytes).expect("canonical registry");
            assert!(matches!(
                field(&value, "activePackage"),
                Some(JsonValue::Null)
            ));
            assert_eq!(text(&value, "rawReason").as_deref(), Some("policy_raw"));
            assert_eq!(file.path, registry_coordinates(family).1);
        }
    }
}
