//! Read-only production authority for one family target and its prebuilt compensation.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{
    CaptureEpochPublicGitJson, FamilyActionAuthorityFiles, FamilyCatalogImageVerification,
    FamilyReleaseBoundImageRead, FamilyReleaseEnvironments, FamilyReleaseImagePorts,
    FamilyTargetPreflightInput, FamilyTargetWitnessRead, MaintenanceAnchorStatusJson,
    NativeFamilyReleaseCurrent, NativeFamilyReleaseCurrentObservation,
    NativeFamilyReleaseDurableProof, NativeFamilyReleaseInput, NativeFamilyReleaseMutationRecord,
    NativeFamilyReleaseTransactionError, NativeFamilyReleaseVerifiedProof, NativeWirePrimitives,
    prepare_family_release_material,
};

const ACTION_ROOT: &str = "config/forecast-adjustments/ballydidean";
const CAPACITY_VERSION: &str = "adjustment-family-release-capacity/v1";
const COMPENSATION_SCOPE: &str = "family-only-new-release-compensation";
const MODEL_ACTION_VERSION: &str = "forecast-adjustment-model-action/v1";
const RAIN_ACTION_VERSION: &str = "forecast-adjustment-rain-control-reference-action/v1";
const REPOSITORY_ID: u64 = 1_342_404_160;
const REPOSITORY_NAME: &str = "anstosa/Ballydidean-Weather";
const MAXIMUM_ACTION_BYTES: usize = 32 * 1024;
const ENVIRONMENT_KEYS: &[&str] = &[
    "CLOUDFLARED_IMAGE",
    "POSTGRES_IMAGE",
    "WEATHER_CONTROL_PLANE_SHA256",
    "WEATHER_CONTROL_PLANE_VERSION",
    "WEATHER_DATABASE_NAME",
    "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH",
    "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH",
    "WEATHER_POSTGRES_DIR",
    "WEATHER_RELEASE",
    "WEATHER_SERVER_IMAGE",
    "WEATHER_WEB_IMAGE",
];

/// One contained target-authority refusal without exposing private document contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeFamilyReleaseTargetAuthorityError {
    Input,
    Current,
    Git,
    Action,
    Image,
    Capacity,
    Authority,
    Unsupported,
}

use NativeFamilyReleaseTargetAuthorityError as Error;

/// Read-only host observations kept separate from public Git and immutable image readers.
pub trait NativeFamilyReleaseTargetAuthorityRead {
    /// Read the current catalog bytes through the same installed host authority.
    fn read_current_catalog(&mut self) -> Result<Vec<u8>, Error>;

    /// Read the current settings bytes through the same installed host authority.
    fn read_current_settings(&mut self) -> Result<Vec<u8>, Error>;

    /// Read one fixed release environment through a root-owned release directory.
    fn read_release_environment(&mut self, release: &str) -> Result<JsonValue, Error>;

    /// Refresh a stale previously admitted pair capacity before any public proof clock is captured.
    fn prepare_capacity(
        &mut self,
        _input: &NativeFamilyReleaseInput,
        _current: &NativeFamilyReleaseCurrent,
    ) -> Result<(), Error> {
        Ok(())
    }

    /// Read the already measured capacity receipt addressed by the target action.
    fn read_capacity(&mut self, action_sha256: &str) -> Result<JsonValue, Error>;
}

/// Exact immutable public release proof retained with either target action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseGitProof {
    pub check_run_id: u64,
    pub publish_run_id: u64,
    pub source_commit: String,
    pub target_commit: String,
    pub target_release: String,
}

/// Durable proof members consumed by the independent family transaction core.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFamilyReleaseTargetDurableProof {
    pub baseline_catalog_bytes: Vec<u8>,
    pub compensation_action_sha256: String,
    pub compensation_commit: String,
    pub compensation_family_identity_sha256: String,
    pub compensation_image_digests_sha256: String,
    pub source_settings_bytes: Vec<u8>,
    pub target_catalog_bytes: Vec<u8>,
    pub target_commit: String,
    pub target_family_identity_sha256: String,
    pub target_image_digests_sha256: String,
}

/// Source-verified material produced before the family transaction persists any state.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedNativeFamilyReleaseTargetAuthority {
    pub action: JsonValue,
    pub action_bytes: Vec<u8>,
    pub authority: JsonValue,
    pub capacity: JsonValue,
    pub compensation_action: JsonValue,
    pub compensation_action_bytes: Vec<u8>,
    pub compensation_git: NativeFamilyReleaseGitProof,
    pub durable: NativeFamilyReleaseTargetDurableProof,
    pub git: NativeFamilyReleaseGitProof,
}

impl VerifiedNativeFamilyReleaseTargetAuthority {
    /// Transfer only source-verified actions, authority and durable bytes into the transaction core.
    pub fn into_transaction_proof(self) -> NativeFamilyReleaseVerifiedProof {
        NativeFamilyReleaseVerifiedProof {
            action: self.action,
            authority: self.authority,
            compensation_action: self.compensation_action,
            durable: NativeFamilyReleaseDurableProof {
                baseline_catalog_bytes: self.durable.baseline_catalog_bytes,
                compensation_action_sha256: self.durable.compensation_action_sha256,
                compensation_commit: self.durable.compensation_commit,
                compensation_family_identity_sha256: self
                    .durable
                    .compensation_family_identity_sha256,
                compensation_image_digests_sha256: self.durable.compensation_image_digests_sha256,
                source_settings_bytes: self.durable.source_settings_bytes,
                target_catalog_bytes: self.durable.target_catalog_bytes,
                target_commit: self.durable.target_commit,
                target_family_identity_sha256: self.durable.target_family_identity_sha256,
                target_image_digests_sha256: self.durable.target_image_digests_sha256,
            },
        }
    }
}

/// Exact command and verification coordinates already admitted by the transaction parser.
pub struct NativeFamilyReleaseTargetAuthorityInput<'a> {
    pub action_sha256: &'a str,
    pub compensating_release: &'a str,
    pub family: &'a str,
    pub fencing_token: &'a str,
    pub now: &'a str,
    pub receipt_created_at: &'a str,
    pub report_sha256: &'a str,
    pub target_release: &'a str,
}

/// Independently owned public, host, custody, witness and immutable-image read ports.
pub struct NativeFamilyReleaseTargetAuthorityPorts<
    'a,
    Host,
    Git,
    Authority,
    Source,
    Target,
    Compensation,
    Witness,
    Verifier,
> {
    pub host: &'a mut Host,
    pub git: &'a mut Git,
    pub authority: &'a mut Authority,
    pub source_image: &'a mut Source,
    pub target_image: &'a mut Target,
    pub compensation_image: &'a mut Compensation,
    pub witness: &'a mut Witness,
    pub verifier: &'a mut Verifier,
}

/// Adapt the source authority directly into the independent transaction core's proof contract.
#[allow(clippy::too_many_arguments)]
pub fn verify_native_family_release_transaction_proof(
    input: &NativeFamilyReleaseInput,
    current: &NativeFamilyReleaseCurrentObservation,
    current_authority: &NativeFamilyReleaseCurrent,
    prior: Option<&NativeFamilyReleaseMutationRecord>,
    now: &str,
    ports: &mut NativeFamilyReleaseTargetAuthorityPorts<
        '_,
        impl NativeFamilyReleaseTargetAuthorityRead,
        impl CaptureEpochPublicGitJson,
        impl FamilyActionAuthorityFiles,
        impl FamilyReleaseBoundImageRead,
        impl FamilyReleaseBoundImageRead,
        impl FamilyReleaseBoundImageRead,
        impl FamilyTargetWitnessRead,
        impl FamilyCatalogImageVerification,
    >,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeFamilyReleaseVerifiedProof, NativeFamilyReleaseTransactionError> {
    let observed_settings_sha256 = codec
        .sha256(&current.settings_bytes)
        .ok_or(NativeFamilyReleaseTransactionError::Proof)?;
    // the separately verified live current must be the same observation held under the lock
    if current.commit != current_authority.commit
        || current.release != current_authority.release
        || input.family != current_authority.family
        || observed_settings_sha256.as_str() != current_authority.settings_sha256
        || input.expected_current_release != current.release
        || input.expected_source_release != current.release
    {
        return Err(NativeFamilyReleaseTransactionError::Proof);
    }
    let receipt_created_at = prior.map_or(now, |record| record.created_at.as_str());
    verify_native_family_release_target_authority(
        &NativeFamilyReleaseTargetAuthorityInput {
            action_sha256: &input.action_sha256,
            compensating_release: &input.compensating_release,
            family: &input.family,
            fencing_token: &input.fencing_token,
            now,
            receipt_created_at,
            report_sha256: &input.report_sha256,
            target_release: &input.target_release,
        },
        current_authority,
        ports,
        codec,
        time,
    )
    .map(VerifiedNativeFamilyReleaseTargetAuthority::into_transaction_proof)
    .map_err(|_| NativeFamilyReleaseTransactionError::Proof)
}

/// Verify current bytes, public releases, actions, capacity, custody and immutable images.
#[allow(clippy::too_many_arguments)]
pub fn verify_native_family_release_target_authority(
    input: &NativeFamilyReleaseTargetAuthorityInput<'_>,
    current: &NativeFamilyReleaseCurrent,
    ports: &mut NativeFamilyReleaseTargetAuthorityPorts<
        '_,
        impl NativeFamilyReleaseTargetAuthorityRead,
        impl CaptureEpochPublicGitJson,
        impl FamilyActionAuthorityFiles,
        impl FamilyReleaseBoundImageRead,
        impl FamilyReleaseBoundImageRead,
        impl FamilyReleaseBoundImageRead,
        impl FamilyTargetWitnessRead,
        impl FamilyCatalogImageVerification,
    >,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<VerifiedNativeFamilyReleaseTargetAuthority, Error> {
    validate_request(input, current)?;
    let baseline_catalog_bytes = ports.host.read_current_catalog()?;
    let source_settings_bytes = ports.host.read_current_settings()?;
    let source_environment = ports.host.read_release_environment(&current.release)?;
    let target_environment = ports.host.read_release_environment(input.target_release)?;
    let compensation_environment = ports
        .host
        .read_release_environment(input.compensating_release)?;
    verify_release_environment(&source_environment, &current.release)?;
    verify_release_environment(&target_environment, input.target_release)?;
    verify_release_environment(&compensation_environment, input.compensating_release)?;
    verify_current_material(
        current,
        &baseline_catalog_bytes,
        &source_settings_bytes,
        &source_environment,
        codec,
    )?;

    let target_path = action_path(input.action_sha256);
    let target_git_bytes = read_repository_file(
        &current.commit,
        input.target_release,
        &target_path,
        ports.git,
        codec,
    )?;
    let action = validate_action(
        &target_git_bytes,
        input.action_sha256,
        input.family,
        input.report_sha256,
        input.now,
        &current.commit,
        &current.release,
        codec,
    )?;
    let git =
        verify_native_family_release_git_proof(&action, input.target_release, ports.git, codec)?;
    let target_image_bytes = ports
        .target_image
        .read_member(&target_path, MAXIMUM_ACTION_BYTES)
        .map_err(|_| Error::Image)?;
    // require identical public and image bytes before granting target authority
    if target_image_bytes != target_git_bytes {
        return Err(Error::Image);
    }

    let compensation_commit = resolve_public_tag(input.compensating_release, ports.git)?;
    let compensation_compare = ports
        .git
        .read_json(&format!(
            "compare/{}...{}?per_page=100",
            git.target_commit, compensation_commit
        ))
        .map_err(|_| Error::Git)?;
    let compensation_path = discover_compensation_action_path(&compensation_compare)?;
    let compensation_action_sha256 = action_hash_from_path(&compensation_path).ok_or(Error::Git)?;
    let compensation_git_bytes = read_repository_file(
        &git.target_commit,
        input.compensating_release,
        &compensation_path,
        ports.git,
        codec,
    )?;
    let compensation_action = validate_action(
        &compensation_git_bytes,
        &compensation_action_sha256,
        input.family,
        input.report_sha256,
        input.now,
        &git.target_commit,
        input.target_release,
        codec,
    )?;
    let compensation_git = verify_native_family_release_git_proof(
        &compensation_action,
        input.compensating_release,
        ports.git,
        codec,
    )?;
    verify_compensation_lineage(input, &action, &compensation_action)?;
    let compensation_image_bytes = ports
        .compensation_image
        .read_member(&compensation_path, MAXIMUM_ACTION_BYTES)
        .map_err(|_| Error::Image)?;
    // require the prebuilt compensation image to retain the public action bytes
    if compensation_image_bytes != compensation_git_bytes {
        return Err(Error::Image);
    }

    let capacity = ports.host.read_capacity(input.action_sha256)?;
    validate_capacity(
        &capacity,
        input,
        current,
        &source_environment,
        &target_environment,
        &compensation_environment,
    )?;
    let clock = || Ok(input.receipt_created_at.to_owned());
    let mut images = FamilyReleaseImagePorts {
        source: ports.source_image,
        target: ports.target_image,
        compensation: ports.compensation_image,
        verifier: ports.verifier,
    };
    let prepared = prepare_family_release_material(
        &FamilyTargetPreflightInput {
            action: &action,
            action_sha256: input.action_sha256,
            family: input.family,
            source_bytes: &baseline_catalog_bytes,
            environment: &target_environment,
            image_commit: &git.target_commit,
            settings_bytes: &source_settings_bytes,
            clock: &clock,
        },
        &FamilyReleaseEnvironments {
            source: &source_environment,
            compensation: &compensation_environment,
        },
        ports.authority,
        &mut images,
        ports.witness,
        codec,
        time,
    )
    .map_err(|_| Error::Authority)?;

    Ok(VerifiedNativeFamilyReleaseTargetAuthority {
        action,
        action_bytes: target_image_bytes,
        authority: prepared.authority,
        capacity,
        compensation_action,
        compensation_action_bytes: compensation_image_bytes,
        compensation_git: compensation_git.clone(),
        durable: NativeFamilyReleaseTargetDurableProof {
            baseline_catalog_bytes,
            compensation_action_sha256,
            compensation_commit: compensation_git.target_commit,
            compensation_family_identity_sha256: prepared
                .identities
                .compensation_family
                .as_str()
                .to_owned(),
            compensation_image_digests_sha256: prepared
                .identities
                .compensation_images
                .as_str()
                .to_owned(),
            source_settings_bytes,
            target_catalog_bytes: prepared.target_catalog_bytes,
            target_commit: git.target_commit.clone(),
            target_family_identity_sha256: prepared.identities.target_family.as_str().to_owned(),
            target_image_digests_sha256: prepared.identities.target_images.as_str().to_owned(),
        },
        git,
    })
}

/// Prove one exact direct-child public tag, closed diff and Check-before-Publish ordering.
pub fn verify_native_family_release_git_proof(
    action: &JsonValue,
    target_release: &str,
    git: &mut impl CaptureEpochPublicGitJson,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<NativeFamilyReleaseGitProof, Error> {
    let source_release = text(action, "expectedSourceRelease").ok_or(Error::Git)?;
    let expected_source_commit = text(action, "expectedSourceCommit").ok_or(Error::Git)?;
    // reject a retry against the same tag or a caller-selected malformed tag
    if !valid_release(target_release) || target_release == source_release {
        return Err(Error::Git);
    }
    let source_commit = resolve_public_tag(&source_release, git)?;
    let target_commit = resolve_public_tag(target_release, git)?;
    // bind the public source tag to the action's independently verified source
    if source_commit != expected_source_commit || source_commit == target_commit {
        return Err(Error::Git);
    }
    let compare = git
        .read_json(&format!(
            "compare/{source_commit}...{target_commit}?per_page=100"
        ))
        .map_err(|_| Error::Git)?;
    verify_direct_child(&compare, &source_commit, &target_commit)?;
    verify_changed_paths(action, &compare, codec)?;
    let check = latest_successful_workflow(&target_commit, "check.yml", git)?;
    let publish = latest_successful_workflow(&target_commit, "publish-images.yml", git)?;
    // immutable image publication must follow the completed exact-commit Check
    if check.updated_at > publish.created_at {
        return Err(Error::Git);
    }
    Ok(NativeFamilyReleaseGitProof {
        check_run_id: check.id,
        publish_run_id: publish.id,
        source_commit,
        target_commit,
        target_release: target_release.to_owned(),
    })
}

/// Read one content-addressed public action and rebind its canonical bytes and image copy.
pub fn read_native_family_release_action(
    source_commit: &str,
    release: &str,
    action_sha256: &str,
    git: &mut impl CaptureEpochPublicGitJson,
    image: &mut impl FamilyReleaseBoundImageRead,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<(JsonValue, Vec<u8>), Error> {
    let path = action_path(action_sha256);
    let bytes = read_repository_file(source_commit, release, &path, git, codec)?;
    let image_bytes = image
        .read_member(&path, MAXIMUM_ACTION_BYTES)
        .map_err(|_| Error::Image)?;
    // public and image reads must name the exact same immutable action
    if image_bytes != bytes {
        return Err(Error::Image);
    }
    let action = codec
        .parse_canonical_object(&bytes)
        .filter(|value| matches!(value, JsonValue::Object(_)))
        .ok_or(Error::Action)?;
    Ok((action, bytes))
}

// close command and current coordinates before observing any external authority
fn validate_request(
    input: &NativeFamilyReleaseTargetAuthorityInput<'_>,
    current: &NativeFamilyReleaseCurrent,
) -> Result<(), Error> {
    // retain only the three source families and distinct immutable releases
    if !matches!(input.family, "temperature" | "wind" | "rain")
        || current.family != input.family
        || !valid_hash(input.action_sha256)
        || !valid_hash(input.report_sha256)
        || !valid_release(input.target_release)
        || !valid_release(input.compensating_release)
        || input.target_release == current.release
        || input.compensating_release == current.release
        || input.compensating_release == input.target_release
        || !valid_u64(input.fencing_token, false)
        || parse_instant(input.now).is_none()
        || parse_instant(input.receipt_created_at).is_none()
    {
        return Err(Error::Input);
    }
    Ok(())
}

// rebind the raw host bytes and selected source environment to the verified current status
fn verify_current_material(
    current: &NativeFamilyReleaseCurrent,
    catalog: &[u8],
    settings: &[u8],
    environment: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<(), Error> {
    let catalog_sha = codec.sha256(catalog).ok_or(Error::Current)?;
    let settings_sha = codec.sha256(settings).ok_or(Error::Current)?;
    let server = text(environment, "WEATHER_SERVER_IMAGE").ok_or(Error::Current)?;
    // raw material is useful only when it is the exact previously verified observation
    if catalog_sha.as_str() != current.catalog_sha256
        || settings_sha.as_str() != current.settings_sha256
        || text(environment, "WEATHER_RELEASE").as_deref() != Some(current.release.as_str())
        || image_digest(&server).as_deref() != Some(current.source_server_image_digest.as_str())
    {
        return Err(Error::Current);
    }
    Ok(())
}

// revalidate one closed release environment without trusting the host adapter's projection
fn verify_release_environment(environment: &JsonValue, release: &str) -> Result<(), Error> {
    let server = text(environment, "WEATHER_SERVER_IMAGE").ok_or(Error::Current)?;
    let web = text(environment, "WEATHER_WEB_IMAGE").ok_or(Error::Current)?;
    let postgres = text(environment, "POSTGRES_IMAGE").ok_or(Error::Current)?;
    let cloudflared = text(environment, "CLOUDFLARED_IMAGE").ok_or(Error::Current)?;
    let control = text(environment, "WEATHER_CONTROL_PLANE_VERSION").ok_or(Error::Current)?;
    // preserve all eleven exact assignments and immutable release identities
    if !exact_keys(environment, ENVIRONMENT_KEYS)
        || text(environment, "WEATHER_RELEASE").as_deref() != Some(release)
        || !valid_weather_image(&server)
        || !valid_weather_image(&web)
        || !valid_generic_image(&postgres)
        || !valid_generic_image(&cloudflared)
        || !text(environment, "WEATHER_CONTROL_PLANE_SHA256")
            .is_some_and(|value| valid_hash(&value))
        || control.is_empty()
        || control.len() > 6
        || control.starts_with('0')
        || !control.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(Error::Current);
    }
    Ok(())
}

// resolve a direct or annotated public tag through the predecessor's bounded depth
fn resolve_public_tag(
    release: &str,
    git: &mut impl CaptureEpochPublicGitJson,
) -> Result<String, Error> {
    // refuse malformed tags before constructing a public repository suffix
    if !valid_release(release) {
        return Err(Error::Git);
    }
    let mut document = git
        .read_json(&format!("git/ref/tags/{release}"))
        .map_err(|_| Error::Git)?;
    // follow at most three canonical tag objects
    for _ in 0..3 {
        let object = field(&document, "object").ok_or(Error::Git)?;
        let sha = text(object, "sha")
            .filter(|value| valid_commit(value))
            .ok_or(Error::Git)?;
        match text(object, "type").as_deref() {
            Some("commit") => return Ok(sha),
            Some("tag") => {
                document = git
                    .read_json(&format!("git/tags/{sha}"))
                    .map_err(|_| Error::Git)?;
            }
            _ => return Err(Error::Git),
        }
    }
    Err(Error::Git)
}

// require the exact one-commit ancestry relation before inspecting changed paths
fn verify_direct_child(
    compare: &JsonValue,
    source_commit: &str,
    target_commit: &str,
) -> Result<(), Error> {
    let commits = array(compare, "commits").ok_or(Error::Git)?;
    let commit = commits.first().ok_or(Error::Git)?;
    let parents = array(commit, "parents").ok_or(Error::Git)?;
    // reject hidden commits, truncation and merge commits
    if text(compare, "status").as_deref() != Some("ahead")
        || integer(compare, "ahead_by") != Some(1)
        || integer(compare, "behind_by") != Some(0)
        || integer(compare, "total_commits") != Some(1)
        || field(compare, "base_commit")
            .and_then(|value| text(value, "sha"))
            .as_deref()
            != Some(source_commit)
        || commits.len() != 1
        || text(commit, "sha").as_deref() != Some(target_commit)
        || parents.len() != 1
        || text(&parents[0], "sha").as_deref() != Some(source_commit)
    {
        return Err(Error::Git);
    }
    Ok(())
}

struct WorkflowEvidence {
    id: u64,
    created_at: i64,
    updated_at: i64,
}

// select only the newest complete same-repository successful push workflow
fn latest_successful_workflow(
    commit: &str,
    filename: &str,
    git: &mut impl CaptureEpochPublicGitJson,
) -> Result<WorkflowEvidence, Error> {
    let document = git
        .read_json(&format!(
            "actions/workflows/{filename}/runs?head_sha={commit}&event=push&per_page=100"
        ))
        .map_err(|_| Error::Git)?;
    let total = integer(&document, "total_count").ok_or(Error::Git)?;
    let runs = array(&document, "workflow_runs").ok_or(Error::Git)?;
    // complete pages prevent a hidden newer failed attempt
    if !(1..=100).contains(&total) || runs.len() != total as usize {
        return Err(Error::Git);
    }
    let mut selected: Option<WorkflowEvidence> = None;
    // inspect every exact repository run before choosing the newest one
    for run in runs {
        // unrelated runs cannot contribute authority
        if text(run, "head_sha").as_deref() != Some(commit)
            || text(run, "event").as_deref() != Some("push")
            || !repository(field(run, "repository"))
            || !repository(field(run, "head_repository"))
        {
            continue;
        }
        let id = positive_integer(run, "id").ok_or(Error::Git)?;
        positive_integer(run, "run_attempt").ok_or(Error::Git)?;
        // malformed scoped evidence blocks stale-success fallback
        if text(run, "path").as_deref() != Some(format!(".github/workflows/{filename}").as_str()) {
            return Err(Error::Git);
        }
        let created_at = instant(run, "created_at").ok_or(Error::Git)?;
        let updated_at = instant(run, "updated_at").ok_or(Error::Git)?;
        let candidate = WorkflowEvidence {
            id,
            created_at,
            updated_at,
        };
        // latest creation time and then run id match the source ordering
        if selected.as_ref().is_none_or(|current| {
            (candidate.created_at, candidate.id) > (current.created_at, current.id)
        }) {
            // queued, failed and cancelled latest runs block publication
            if text(run, "status").as_deref() != Some("completed")
                || text(run, "conclusion").as_deref() != Some("success")
            {
                selected = Some(WorkflowEvidence {
                    id,
                    created_at,
                    updated_at: i64::MIN,
                });
            } else {
                selected = Some(candidate);
            }
        }
    }
    let selected = selected.ok_or(Error::Git)?;
    // the sentinel records that the newest exact run was not successful
    if selected.updated_at == i64::MIN {
        return Err(Error::Git);
    }
    Ok(selected)
}

// read and bind one public repository file to its content-addressed filename
fn read_repository_file(
    source_commit: &str,
    release: &str,
    path: &str,
    git: &mut impl CaptureEpochPublicGitJson,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<Vec<u8>, Error> {
    // require independent source and path coordinates before a public request
    if !valid_commit(source_commit)
        || !valid_release(release)
        || !path.starts_with(&format!("{ACTION_ROOT}/actions/sha256-"))
        || action_hash_from_path(path).is_none()
    {
        return Err(Error::Git);
    }
    let document = git
        .read_json(&format!("contents/{path}?ref={release}"))
        .map_err(|_| Error::Git)?;
    let size = integer(&document, "size").ok_or(Error::Git)?;
    let content = text(&document, "content").ok_or(Error::Git)?;
    // only one bounded ordinary base64 file can carry action authority
    if text(&document, "type").as_deref() != Some("file")
        || text(&document, "path").as_deref() != Some(path)
        || text(&document, "encoding").as_deref() != Some("base64")
        || !(2..=MAXIMUM_ACTION_BYTES as u64).contains(&size)
    {
        return Err(Error::Git);
    }
    let compact = content.replace('\n', "");
    let bytes = NativeWirePrimitives
        .decode_standard_base64(&compact)
        .filter(|bytes| bytes.len() as u64 == size)
        .ok_or(Error::Git)?;
    let expected = action_hash_from_path(path).ok_or(Error::Git)?;
    let actual = codec.sha256(&bytes).ok_or(Error::Git)?;
    // canonical bytes and their public address must agree before parsing fields
    if actual.as_str() != expected || codec.parse_canonical_object(&bytes).is_none() {
        return Err(Error::Git);
    }
    Ok(bytes)
}

// validate the exact action grammar and freshness used by the family executor
#[allow(clippy::too_many_arguments)]
fn validate_action(
    bytes: &[u8],
    expected_sha256: &str,
    expected_family: &str,
    expected_report_sha256: &str,
    now: &str,
    expected_source_commit: &str,
    expected_source_release: &str,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<JsonValue, Error> {
    let action = codec.parse_canonical_object(bytes).ok_or(Error::Action)?;
    let version = text(&action, "contractVersion").ok_or(Error::Action)?;
    // dispatch only the two source action contracts
    if version == MODEL_ACTION_VERSION {
        validate_model_action(
            &action,
            bytes,
            expected_sha256,
            expected_family,
            expected_report_sha256,
            now,
            expected_source_commit,
            expected_source_release,
            codec,
        )?;
    } else if version == RAIN_ACTION_VERSION {
        validate_rain_action(
            &action,
            bytes,
            expected_sha256,
            now,
            expected_source_commit,
            expected_source_release,
            codec,
        )?;
    } else {
        return Err(Error::Unsupported);
    }
    Ok(action)
}

// validate one model action without trusting fields selected by its public filename
#[allow(clippy::too_many_arguments)]
fn validate_model_action(
    action: &JsonValue,
    bytes: &[u8],
    expected_sha256: &str,
    expected_family: &str,
    expected_report_sha256: &str,
    now: &str,
    expected_source_commit: &str,
    expected_source_release: &str,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<(), Error> {
    const KEYS: &[&str] = &[
        "actionKind",
        "candidateGraphSha256",
        "candidateSha256",
        "contractVersion",
        "createdAt",
        "expectedSettingsSha256",
        "expectedSourceCommit",
        "expectedInstalledReceiptSha256",
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
    let kind = text(action, "actionKind").ok_or(Error::Action)?;
    let Some((policy, reason, candidate, predecessor)) = model_rule(&kind) else {
        return Err(Error::Unsupported);
    };
    let digest = codec.sha256(bytes).ok_or(Error::Action)?;
    let decision = text(action, "policyDecision").ok_or(Error::Action)?;
    let policy_matches = policy.map_or_else(
        || matches!(decision.as_str(), "qualified" | "regressed"),
        |expected| decision == expected,
    );
    // bind identity, source, report and fixed action semantics
    if !exact_keys(action, KEYS)
        || digest.as_str() != expected_sha256
        || text(action, "contractVersion").as_deref() != Some(MODEL_ACTION_VERSION)
        || text(action, "siteKey").as_deref() != Some("ballydidean")
        || text(action, "family").as_deref() != Some(expected_family)
        || !matches!(expected_family, "temperature" | "wind" | "rain")
        || text(action, "expectedSourceCommit").as_deref() != Some(expected_source_commit)
        || text(action, "expectedSourceRelease").as_deref() != Some(expected_source_release)
        || text(action, "policyReportSha256").as_deref() != Some(expected_report_sha256)
        || text(action, "reason").as_deref() != Some(reason)
        || !policy_matches
    {
        return Err(Error::Action);
    }
    // every required and nullable root retains its exact hash domain
    if ![
        "expectedSettingsSha256",
        "lifecycleHeadSha256",
        "policyReportSha256",
    ]
    .into_iter()
    .all(|key| text(action, key).is_some_and(|value| valid_hash(&value)))
        || ![
            "candidateGraphSha256",
            "candidateSha256",
            "fullMemberRootSha256",
            "expectedInstalledReceiptSha256",
            "predecessorActionSha256",
        ]
        .into_iter()
        .all(|key| nullable_hash(action, key))
    {
        return Err(Error::Action);
    }
    let candidate_sha = field(action, "candidateSha256");
    let candidate_graph = field(action, "candidateGraphSha256");
    let full_member = field(action, "fullMemberRootSha256");
    let prior = field(action, "predecessorActionSha256");
    let fencing = text(action, "fencingToken").ok_or(Error::Action)?;
    // preserve pair presence, member-root and predecessor bindings
    if !valid_commit(expected_source_commit)
        || !valid_release(expected_source_release)
        || !valid_u64(&fencing, true)
        || candidate != (non_null(candidate_sha) && non_null(candidate_graph))
        || is_null(candidate_sha) != is_null(candidate_graph)
        || matches!(kind.as_str(), "shadow" | "compensate_shadow") != is_null(full_member)
        || predecessor != non_null(prior)
    {
        return Err(Error::Action);
    }
    let created = instant(action, "createdAt").ok_or(Error::Action)?;
    let report = instant(action, "reportCreatedAt").ok_or(Error::Action)?;
    let through = instant(action, "validThrough").ok_or(Error::Action)?;
    let observed = parse_instant(now).ok_or(Error::Action)?;
    // apply the exact seven-day report lifetime and non-compensation freshness
    if through.checked_sub(report) != Some(7 * 86_400_000)
        || created < report
        || observed < created
        || (!predecessor && observed > through)
    {
        return Err(Error::Action);
    }
    Ok(())
}

// validate one pre-month rain control action and its compensation recovery form
#[allow(clippy::too_many_arguments)]
fn validate_rain_action(
    action: &JsonValue,
    bytes: &[u8],
    expected_sha256: &str,
    now: &str,
    expected_source_commit: &str,
    expected_source_release: &str,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<(), Error> {
    const KEYS: &[&str] = &[
        "actionKind",
        "contractVersion",
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
        "controlStateSha256",
        "sourceMemberRootSha256",
        "sourceReceiptRootSha256",
        "validThrough",
    ];
    let kind = text(action, "actionKind").ok_or(Error::Action)?;
    let compensation = kind == "compensate_control_reference";
    let digest = codec.sha256(bytes).ok_or(Error::Action)?;
    // bind exact control identity before interpreting its calendar window
    if !exact_keys(action, KEYS)
        || digest.as_str() != expected_sha256
        || text(action, "contractVersion").as_deref() != Some(RAIN_ACTION_VERSION)
        || text(action, "family").as_deref() != Some("rain")
        || !matches!(
            kind.as_str(),
            "control_reference" | "compensate_control_reference"
        )
        || text(action, "reason").as_deref()
            != Some(if compensation {
                "failed_control_reference"
            } else {
                "premonth_reference"
            })
        || text(action, "expectedSourceCommit").as_deref() != Some(expected_source_commit)
        || text(action, "expectedSourceRelease").as_deref() != Some(expected_source_release)
    {
        return Err(Error::Action);
    }
    // all control graph roots are required while two compare-and-swap roots are nullable
    if ![
        "expectedSettingsSha256",
        "graphManifestSha256",
        "ordinalArtifactSha256",
        "controlStateSha256",
        "sourceMemberRootSha256",
        "sourceReceiptRootSha256",
    ]
    .into_iter()
    .all(|key| text(action, key).is_some_and(|value| valid_hash(&value)))
        || !["expectedCatalogReceiptSha256", "predecessorActionSha256"]
            .into_iter()
            .all(|key| nullable_hash(action, key))
    {
        return Err(Error::Action);
    }
    let due = text(action, "dueMonth")
        .filter(|value| valid_month(value))
        .ok_or(Error::Action)?;
    let month_start = parse_instant(&format!("{due}-01T00:00:00.000Z")).ok_or(Error::Action)?;
    let created = instant(action, "createdAt").ok_or(Error::Action)?;
    let through = text(action, "validThrough").ok_or(Error::Action)?;
    let observed = parse_instant(now).ok_or(Error::Action)?;
    let fencing = text(action, "fencingToken").ok_or(Error::Action)?;
    // preserve the pre-month window and allow expired recovery only for compensation
    if !valid_commit(expected_source_commit)
        || !valid_release(expected_source_release)
        || !valid_u64(&fencing, false)
        || parse_instant(&through) != Some(month_start)
        || created < month_start - 7 * 86_400_000
        || created >= month_start
        || observed < created
        || (!compensation && observed > month_start)
        || compensation != non_null(field(action, "predecessorActionSha256"))
    {
        return Err(Error::Action);
    }
    Ok(())
}

// verify the compensation action is the exact target successor and recovery pair
fn verify_compensation_lineage(
    input: &NativeFamilyReleaseTargetAuthorityInput<'_>,
    target: &JsonValue,
    compensation: &JsonValue,
) -> Result<(), Error> {
    let control = text(target, "contractVersion").as_deref() == Some(RAIN_ACTION_VERSION);
    let common = text(compensation, "predecessorActionSha256").as_deref()
        == Some(input.action_sha256)
        && text(compensation, "fencingToken") == text(target, "fencingToken")
        && text(compensation, "validThrough") == text(target, "validThrough");
    let specific = if control {
        [
            "expectedCatalogReceiptSha256",
            "graphManifestSha256",
            "controlStateSha256",
            "ordinalArtifactSha256",
            "sourceMemberRootSha256",
            "sourceReceiptRootSha256",
            "dueMonth",
        ]
        .into_iter()
        .all(|key| field(compensation, key) == field(target, key))
    } else {
        [
            "policyDecision",
            "fullMemberRootSha256",
            "reportCreatedAt",
            "expectedInstalledReceiptSha256",
        ]
        .into_iter()
        .all(|key| field(compensation, key) == field(target, key))
    };
    // recovery must name exactly this failed target and preserve its immutable evidence
    if !text(compensation, "actionKind").is_some_and(|value| value.starts_with("compensate_"))
        || !common
        || !specific
    {
        return Err(Error::Action);
    }
    Ok(())
}

// verify the retained capacity coordinates and six literal immutable images
pub(crate) fn validate_capacity(
    capacity: &JsonValue,
    input: &NativeFamilyReleaseTargetAuthorityInput<'_>,
    current: &NativeFamilyReleaseCurrent,
    source: &JsonValue,
    target: &JsonValue,
    compensation: &JsonValue,
) -> Result<(), Error> {
    let measured = instant(capacity, "measuredAt").ok_or(Error::Capacity)?;
    let now = parse_instant(input.now).ok_or(Error::Capacity)?;
    let age = now.checked_sub(measured).ok_or(Error::Capacity)?;
    let images = array(capacity, "imageDigests").ok_or(Error::Capacity)?;
    // retain the source's freshness, no-credit and transaction identity checks
    if text(capacity, "contractVersion").as_deref() != Some(CAPACITY_VERSION)
        || text(capacity, "state").as_deref() != Some("capacity_ready")
        || text(capacity, "actionSha256").as_deref() != Some(input.action_sha256)
        || text(capacity, "family").as_deref() != Some(input.family)
        || text(capacity, "sourceRelease").as_deref() != Some(current.release.as_str())
        || text(capacity, "compensationScope").as_deref() != Some(COMPENSATION_SCOPE)
        || !(0..=900_000).contains(&age)
        || number(capacity, "retirementCreditBytes") != Some(0)
        || images.len() != 6
    {
        return Err(Error::Capacity);
    }
    let environments = [
        ("source", source),
        ("target", target),
        ("compensating", compensation),
    ];
    let mut seen = Vec::new();
    // bind every role/runtime digest to the independently read release environment
    for image in images {
        let role = text(image, "role").ok_or(Error::Capacity)?;
        let runtime = text(image, "runtime").ok_or(Error::Capacity)?;
        let digest = text(image, "digest").ok_or(Error::Capacity)?;
        let environment = environments
            .iter()
            .find_map(|(name, value)| (*name == role).then_some(*value))
            .ok_or(Error::Capacity)?;
        let key = match runtime.as_str() {
            "server" => "WEATHER_SERVER_IMAGE",
            "web" => "WEATHER_WEB_IMAGE",
            _ => return Err(Error::Capacity),
        };
        let expected = text(environment, key)
            .and_then(|reference| {
                reference
                    .split_once('@')
                    .map(|(_, digest)| digest.to_owned())
            })
            .ok_or(Error::Capacity)?;
        // duplicate coordinates or a capacity-selected alias cannot authorize mutation
        if digest != expected || seen.contains(&(role.clone(), runtime.clone())) {
            return Err(Error::Capacity);
        }
        seen.push((role, runtime));
    }
    Ok(())
}

// validate the closed action-specific file matrix from the exact direct-child compare
fn verify_changed_paths(
    action: &JsonValue,
    compare: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<(), Error> {
    let files = array(compare, "files").ok_or(Error::Git)?;
    let version = text(action, "contractVersion").ok_or(Error::Git)?;
    // use the disjoint source path matrix selected by the action contract
    if version == RAIN_ACTION_VERSION {
        verify_rain_changed_paths(action, files, codec)
    } else if version == MODEL_ACTION_VERSION {
        verify_model_changed_paths(action, files, codec)
    } else {
        Err(Error::Unsupported)
    }
}

// close the ordinary model action diff without permitting immutable evidence mutation
fn verify_model_changed_paths(
    action: &JsonValue,
    files: &[JsonValue],
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<(), Error> {
    let family = text(action, "family").ok_or(Error::Git)?;
    let kind = text(action, "actionKind").ok_or(Error::Git)?;
    let candidate = text(action, "candidateSha256");
    let action_bytes = codec.encode_canonical_json(action).ok_or(Error::Git)?;
    let action_sha = codec.sha256(&action_bytes).ok_or(Error::Git)?;
    let action_path = action_path(action_sha.as_str());
    let registry = format!(
        "config/forecast-adjustments/ballydidean-{family}-{}.json",
        if family == "rain" {
            "runtime"
        } else {
            "canary"
        }
    );
    let mut allowed = vec![action_path.clone()];
    // inactive shadows alone may add candidate-addressed immutable evidence
    if kind == "shadow" {
        let candidate = candidate
            .filter(|value| valid_hash(value))
            .ok_or(Error::Git)?;
        allowed.push(format!(
            "{ACTION_ROOT}/model-parity/{family}/sha256-{candidate}.json"
        ));
        allowed.push(format!(
            "{ACTION_ROOT}/shadow-catalog/{family}/sha256-{candidate}.json"
        ));
        let directory = if family == "rain" {
            "rain-runtime-artifacts".to_owned()
        } else {
            format!("{family}-canary-bundles")
        };
        let artifacts = matching_addressed(files, &format!("{ACTION_ROOT}/{directory}/"));
        // one serving artifact and rain's optional control artifact are the only open addresses
        if artifacts.is_empty() || artifacts.len() > if family == "rain" { 2 } else { 1 } {
            return Err(Error::Git);
        }
        allowed.extend(artifacts);
        // rain shadows alone may add one content-addressed package and control state
        if family == "rain" {
            allowed.push(format!(
                "{ACTION_ROOT}/rain-model-packages/sha256-{candidate}.json"
            ));
            let states = matching_addressed(
                files,
                &format!("{ACTION_ROOT}/rain-maintenance-control-states/"),
            );
            // legacy and control-state shadow matrices remain disjoint
            if states.len() > 1
                || allowed
                    .iter()
                    .filter(|path| path.contains("rain-runtime-artifacts"))
                    .count()
                    == 2
                    && states.len() != 1
            {
                return Err(Error::Git);
            }
            allowed.extend(states);
        }
    }
    // serving actions modify exactly their one family registry
    if !matches!(kind.as_str(), "shadow" | "compensate_shadow") {
        allowed.push(registry.clone());
    }
    // rain serving changes also replace the generated compiled source
    if family == "rain"
        && matches!(
            kind.as_str(),
            "promote" | "rollback_prior" | "compensate_incumbent"
        )
    {
        allowed.push("packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts".to_owned());
    }
    let names = files
        .iter()
        .map(|file| text(file, "filename").ok_or(Error::Git))
        .collect::<Result<Vec<_>, _>>()?;
    // require every and only the action-specific changed path once
    if files.is_empty()
        || files.len() > 10
        || unique_count(&names) != names.len()
        || !names.contains(&action_path)
        || files.iter().any(|file| {
            let Some(name) = text(file, "filename") else {
                return true;
            };
            let Some(status) = text(file, "status") else {
                return true;
            };
            !allowed.contains(&name)
                || !matches!(status.as_str(), "added" | "modified")
                || if name == registry {
                    status != "modified"
                } else {
                    status != "added"
                }
        })
        || (!matches!(kind.as_str(), "shadow" | "compensate_shadow") && !names.contains(&registry))
    {
        return Err(Error::Git);
    }
    Ok(())
}

// close the rain control reference diff without granting serving selector authority
fn verify_rain_changed_paths(
    action: &JsonValue,
    files: &[JsonValue],
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<(), Error> {
    let bytes = codec.encode_canonical_json(action).ok_or(Error::Git)?;
    let digest = codec.sha256(&bytes).ok_or(Error::Git)?;
    let action_path = action_path(digest.as_str());
    let selector = "config/forecast-adjustments/ballydidean-rain-control-reference.json";
    let compensation =
        text(action, "actionKind").as_deref() == Some("compensate_control_reference");
    let mut expected = vec![
        (action_path, vec!["added"]),
        (selector.to_owned(), vec!["modified", "removed"]),
    ];
    // ordinary control publication carries its exact state and ordinal artifact
    if !compensation {
        let state = text(action, "controlStateSha256")
            .filter(|value| valid_hash(value))
            .ok_or(Error::Git)?;
        let ordinal = text(action, "ordinalArtifactSha256")
            .filter(|value| valid_hash(value))
            .ok_or(Error::Git)?;
        expected = vec![
            (expected[0].0.clone(), vec!["added"]),
            (
                format!("{ACTION_ROOT}/rain-maintenance-control-states/sha256-{state}.json"),
                vec!["added"],
            ),
            (
                format!("{ACTION_ROOT}/rain-runtime-artifacts/sha256-{ordinal}.json"),
                vec!["added"],
            ),
            (selector.to_owned(), vec!["added", "modified"]),
        ];
    }
    // every expected path appears once with only its admitted source status
    if files.len() != expected.len()
        || files.iter().any(|file| {
            let Some(name) = text(file, "filename") else {
                return true;
            };
            let Some(status) = text(file, "status") else {
                return true;
            };
            expected
                .iter()
                .find(|(path, _)| *path == name)
                .is_none_or(|(_, statuses)| !statuses.contains(&status.as_str()))
        })
        || expected.iter().any(|(path, _)| {
            files
                .iter()
                .filter(|file| text(file, "filename").as_deref() == Some(path))
                .count()
                != 1
        })
    {
        return Err(Error::Git);
    }
    Ok(())
}

// discover exactly one new immutable compensation action from the closed comparison
fn discover_compensation_action_path(compare: &JsonValue) -> Result<String, Error> {
    let files = array(compare, "files").ok_or(Error::Git)?;
    let paths = files
        .iter()
        .filter_map(|file| {
            let path = text(file, "filename")?;
            (text(file, "status").as_deref() == Some("added")
                && action_hash_from_path(&path).is_some())
            .then_some(path)
        })
        .collect::<Vec<_>>();
    // the target-to-compensation diff must introduce one and only one action
    if paths.len() != 1 {
        return Err(Error::Git);
    }
    Ok(paths[0].clone())
}

// collect content-addressed files below one fixed package directory
fn matching_addressed(files: &[JsonValue], prefix: &str) -> Vec<String> {
    files
        .iter()
        .filter_map(|file| {
            let path = text(file, "filename")?;
            // end the temporary prefix borrow before moving the retained path
            let addressed = path
                .strip_prefix(prefix)
                .is_some_and(|name| action_hash_from_filename(name).is_some());
            addressed.then_some(path)
        })
        .collect()
}

// match the exact fixed public repository identity in workflow evidence
fn repository(value: Option<&JsonValue>) -> bool {
    value.is_some_and(|value| {
        integer(value, "id") == Some(REPOSITORY_ID)
            && text(value, "full_name").as_deref() == Some(REPOSITORY_NAME)
            && matches!(field(value, "fork"), Some(JsonValue::Boolean(false)))
    })
}

// return one exact content-addressed action path
fn action_path(sha256: &str) -> String {
    format!("{ACTION_ROOT}/actions/sha256-{sha256}.json")
}

// extract the hash only from the fixed action directory and filename grammar
fn action_hash_from_path(path: &str) -> Option<String> {
    let name = path.strip_prefix(&format!("{ACTION_ROOT}/actions/"))?;
    action_hash_from_filename(name)
}

// extract one lowercase SHA-256 from an exact content-addressed JSON filename
fn action_hash_from_filename(name: &str) -> Option<String> {
    let digest = name.strip_prefix("sha256-")?.strip_suffix(".json")?;
    valid_hash(digest).then(|| digest.to_owned())
}

// select the original model action rule tuple
fn model_rule(kind: &str) -> Option<(Option<&'static str>, &'static str, bool, bool)> {
    Some(match kind {
        "shadow" => (Some("pending"), "development_candidate", true, false),
        "promote" => (Some("qualified"), "qualified_candidate", true, false),
        "rollback_prior" => (Some("regressed"), "qualified_prior", true, false),
        "raw" => (Some("regressed"), "policy_raw", false, false),
        "compensate_incumbent" => (None, "failed_promotion", true, true),
        "compensate_raw" => (None, "invalid_incumbent", false, true),
        "compensate_shadow" => (Some("pending"), "development_shadow_failure", true, true),
        _ => return None,
    })
}

// accept null or one immutable lowercase SHA-256 member
fn nullable_hash(value: &JsonValue, key: &str) -> bool {
    matches!(field(value, key), Some(JsonValue::Null))
        || text(value, key).is_some_and(|value| valid_hash(&value))
}

// distinguish one present non-null scalar from absent or explicit null
fn non_null(value: Option<&JsonValue>) -> bool {
    value.is_some_and(|value| !matches!(value, JsonValue::Null))
}

// recognize the exact explicit null representation
fn is_null(value: Option<&JsonValue>) -> bool {
    matches!(value, Some(JsonValue::Null))
}

// admit one complete lowercase Git object id
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// admit one source calendar release tag without normalizing its spelling
fn valid_release(value: &str) -> bool {
    let Some((date, suffix)) = value.split_once('-') else {
        return false;
    };
    date.len() == 10
        && date.bytes().enumerate().all(|(index, byte)| {
            // dots occupy the two fixed calendar separators
            if matches!(index, 4 | 7) {
                byte == b'.'
            } else {
                byte.is_ascii_digit()
            }
        })
        && !suffix.is_empty()
        && !suffix.starts_with('0')
        && suffix.bytes().all(|byte| byte.is_ascii_digit())
}

// admit the source positive or optional-zero unsigned 64-bit decimal
fn valid_u64(value: &str, allow_zero: bool) -> bool {
    let shape = if allow_zero {
        value == "0" || !value.starts_with('0') && !value.is_empty()
    } else {
        !value.starts_with('0') && !value.is_empty()
    };
    shape && value.bytes().all(|byte| byte.is_ascii_digit()) && value.parse::<u64>().is_ok()
}

// admit one exact four-digit year and two-digit month
fn valid_month(value: &str) -> bool {
    value.len() == 7
        && value.as_bytes()[4] == b'-'
        && value[..4].bytes().all(|byte| byte.is_ascii_digit())
        && matches!(
            &value[5..],
            "01" | "02" | "03" | "04" | "05" | "06" | "07" | "08" | "09" | "10" | "11" | "12"
        )
}

// parse one canonical explicit UTC millisecond instant
fn parse_instant(value: &str) -> Option<i64> {
    NativeWirePrimitives.parse_explicit_utc_milliseconds(value)
}

// parse one named canonical instant member
fn instant(value: &JsonValue, key: &str) -> Option<i64> {
    parse_instant(&text(value, key)?)
}

// borrow one array member without coercion
fn array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(value, key)? else {
        return None;
    };
    Some(values)
}

// decode one exact JavaScript-safe nonnegative integer
fn integer(value: &JsonValue, key: &str) -> Option<u64> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    (number.is_finite()
        && *number >= 0.0
        && *number <= 9_007_199_254_740_991.0
        && number.fract() == 0.0)
        .then_some(*number as u64)
}

// require one positive JavaScript-safe integer
fn positive_integer(value: &JsonValue, key: &str) -> Option<u64> {
    integer(value, key).filter(|value| *value > 0)
}

// decode one exact integer-valued number
fn number(value: &JsonValue, key: &str) -> Option<i64> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    (number.is_finite()
        && *number >= i64::MIN as f64
        && *number <= i64::MAX as f64
        && number.fract() == 0.0)
        .then_some(*number as i64)
}

// count distinct names without accepting hidden duplicate paths
fn unique_count(values: &[String]) -> usize {
    let mut unique = values.to_vec();
    unique.sort();
    unique.dedup();
    unique.len()
}

// extract only the lowercase digest from an immutable Weather image reference
fn image_digest(reference: &str) -> Option<String> {
    let (_, digest) = reference.split_once("@sha256:")?;
    valid_hash(digest).then(|| digest.to_owned())
}

// admit one immutable Weather application image reference
fn valid_weather_image(reference: &str) -> bool {
    reference
        .strip_prefix("ghcr.io/anstosa/weather-server@sha256:")
        .or_else(|| reference.strip_prefix("ghcr.io/anstosa/weather-web@sha256:"))
        .is_some_and(valid_hash)
}

// admit one immutable dependency image without accepting whitespace or aliases
fn valid_generic_image(reference: &str) -> bool {
    let Some((name, digest)) = reference.split_once("@sha256:") else {
        return false;
    };
    !name.is_empty()
        && !name.chars().any(char::is_whitespace)
        && valid_hash(digest)
        && !digest.contains('@')
}

#[cfg(test)]
#[path = "native_family_release_target_authority_test.rs"]
mod tests;
