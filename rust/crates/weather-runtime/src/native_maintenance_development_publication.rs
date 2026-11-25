//! Source-exact inactive development-candidate release and custody material.

use std::fmt::{self, Display, Formatter};

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    ADJUSTMENT_ROLLING_SCHEDULE_SHA256, NativeFamilyReleaseCurrent,
    NativeMaintenanceArchiveSegment, NativeMaintenanceArchiveSegmentMember,
    NativeMaintenanceAttemptReport, NativeMaintenanceCandidateParityReceipt,
    NativeMaintenanceCaptureFamily, NativeMaintenanceConfirmationPreregistration,
    NativeMaintenanceGraphCrossLink, NativeMaintenancePortableCandidate,
    NativeMaintenanceVerifiedFullGraph, NativeWirePrimitives, adjustment_rolling_local_date_at,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_candidate_parity::admit_package,
    native_maintenance_daily_candidate_replay::{
        admit_temperature_package, build_native_rain_maintenance_artifact,
    },
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
};

const ACTION_VERSION: &str = "forecast-adjustment-model-action/v1";
const PARITY_VERSION: &str = "forecast-adjustment-model-parity/v1";
const SHADOW_PROJECTION_VERSION: &str = "forecast-adjustment-shadow-catalog-projection/v1";
const CUSTODY_BINDING_VERSION: &str = "adjustment-development-custody-graph/v1";
const GRAPH_REFERENCE_VERSION: &str = "adjustment-archive-graph-reference/v1";
const REGISTRATION_VERSION: &str = "adjustment-shadow-registration/v3";
const PACKAGE_MAXIMUM_BYTES: usize = 8 * 1_024 * 1_024;
const DAY_MILLISECONDS: i64 = 86_400_000;
const REGISTRATION_KEYS: &[&str] = &[
    "artifactSha256",
    "candidateSha256",
    "cohortSha256",
    "epochWitnessSha256",
    "family",
    "intervalEndAt",
    "intervalStartAt",
    "policySha256",
    "predecessorRegistrationSha256",
    "registrationSha256",
    "reservedKeySha256",
    "scheduleContractSha256",
    "siteKey",
    "sourceSha256",
    "targetCutoffAt",
    "terminalAt",
];

/// One exact public repository member before any worktree mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceDevelopmentPublicationFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// Borrowed, already verified inputs for one development-only publication.
pub struct NativeMaintenanceDevelopmentPublicationInput<'a> {
    pub candidate_bytes: &'a [u8],
    pub candidate_graph: &'a NativeMaintenanceVerifiedFullGraph,
    pub confirmation_preregistration: &'a NativeMaintenanceConfirmationPreregistration,
    pub created_at: &'a str,
    pub current: &'a NativeFamilyReleaseCurrent,
    pub expected_plan_sha256: &'a str,
    pub family: NativeMaintenanceCaptureFamily,
    pub fencing_token: &'a str,
    pub lifecycle_head_sha256: &'a str,
    pub parity: &'a NativeMaintenanceCandidateParityReceipt,
    pub portable: &'a NativeMaintenancePortableCandidate,
    pub registration: &'a JsonValue,
    pub report: &'a NativeMaintenanceAttemptReport,
    pub report_created_at: &'a str,
}

/// Complete immutable material needed before publication transport is allowed to run.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceDevelopmentPublication {
    pub action: JsonValue,
    pub action_bytes: Vec<u8>,
    pub action_path: String,
    pub action_sha256: String,
    pub confirmation_registration_sha256: String,
    pub custody_binding: JsonValue,
    pub custody_binding_bytes: Vec<u8>,
    pub custody_binding_sha256: String,
    pub custody_segment: NativeMaintenanceArchiveSegment,
    pub family_files: Vec<NativeMaintenanceDevelopmentPublicationFile>,
    pub package_files: Vec<NativeMaintenanceDevelopmentPublicationFile>,
    pub package_root_sha256: String,
    pub package_total_bytes: usize,
    pub release_date: String,
    pub release_run_id: String,
    pub shadow_projection: JsonValue,
    pub shadow_projection_bytes: Vec<u8>,
    pub shadow_projection_sha256: String,
}

/// Closed failures from development publication material construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceDevelopmentPublicationError {
    Input,
    Clock,
    Current,
    Candidate,
    Parity,
    Registration,
    Graph,
    Report,
    Package,
    Codec,
}

impl Display for NativeMaintenanceDevelopmentPublicationError {
    /// Return one stable failure without exposing candidate values.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "development publication input is invalid",
            Self::Clock => "development publication clock is invalid",
            Self::Current => "development publication current authority differs",
            Self::Candidate => "development publication candidate differs",
            Self::Parity => "development publication parity differs",
            Self::Registration => "development publication registration differs",
            Self::Graph => "development publication graph member is unavailable",
            Self::Report => "development publication report differs",
            Self::Package => "development publication package is invalid",
            Self::Codec => "development publication serialization failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceDevelopmentPublicationError {}

/// Build the exact target action, inactive file matrix, and custody graph without side effects.
pub fn build_native_maintenance_development_publication(
    input: NativeMaintenanceDevelopmentPublicationInput<'_>,
) -> Result<NativeMaintenanceDevelopmentPublication, NativeMaintenanceDevelopmentPublicationError> {
    use NativeMaintenanceDevelopmentPublicationError as Error;

    let wire = NativeWirePrimitives;
    let family = input.family.wire();
    let candidate_graph_sha256 = format_digest(input.candidate_graph.manifest_object_sha256);
    let candidate_sha256 = &input.portable.candidate_sha256;
    let artifact_sha256 = &input.portable.artifact_sha256;
    // close the candidate and the current deployed source before deriving public material
    if input.current.family != family
        || input.current.contract_version != "adjustment-family-release-current-status/v1"
        || !valid_commit(&input.current.commit)
        || !valid_release(&input.current.release)
        || !valid_hash(&input.current.settings_sha256)
    {
        return Err(Error::Current);
    }
    // verify candidate byte identities
    if !valid_hash(candidate_sha256)
        || !valid_hash(artifact_sha256)
        || format_digest(wire.sha256(input.candidate_bytes)) != *candidate_sha256
        || !valid_hash(&input.portable.source_identity_sha256)
    {
        return Err(Error::Candidate);
    }
    let artifact =
        canonical_object(&wire, &input.portable.artifact_bytes).ok_or(Error::Candidate)?;
    let artifact_kind = text(&artifact, "contractVersion").ok_or(Error::Candidate)?;
    let artifact_identity_matches = format_digest(wire.sha256(&input.portable.artifact_bytes))
        == *artifact_sha256
        || text(&artifact, "bundleSha256").as_deref() == Some(artifact_sha256);
    // require one admitted artifact address
    if artifact_kind.is_empty() || !artifact_identity_matches {
        return Err(Error::Candidate);
    }

    let registration = admit_registration(&wire, input.registration, family)?;
    // bind all previously built post-fit identities before checking graph reachability
    if text(registration, "candidateSha256").as_deref() != Some(candidate_sha256)
        || text(registration, "artifactSha256").as_deref() != Some(artifact_sha256)
        || !valid_hash(input.expected_plan_sha256)
        || !valid_hash(&input.confirmation_preregistration.registration_sha256)
    {
        return Err(Error::Registration);
    }
    let parity_sha256 = admit_parity(&wire, input.parity, family, candidate_sha256)?;
    admit_report(
        input.report,
        family,
        candidate_sha256,
        &candidate_graph_sha256,
    )?;
    let registration_sha256 =
        text(registration, "registrationSha256").ok_or(Error::Registration)?;
    let policy_sha256 = text(registration, "policySha256").ok_or(Error::Registration)?;
    let cohort_sha256 = text(registration, "cohortSha256").ok_or(Error::Registration)?;
    let source_sha256 = text(registration, "sourceSha256").ok_or(Error::Registration)?;
    let required_graph_members = [
        candidate_sha256.as_str(),
        artifact_sha256.as_str(),
        registration_sha256.as_str(),
        policy_sha256.as_str(),
        cohort_sha256.as_str(),
        input.expected_plan_sha256,
        source_sha256.as_str(),
    ];
    // require actual head-manifest membership for every public registration dependency
    if required_graph_members.iter().any(|identity| {
        !input
            .candidate_graph
            .manifest
            .entries
            .iter()
            .any(|entry| entry.identity_sha256 == *identity)
    }) {
        return Err(Error::Graph);
    }

    let created_milliseconds = wire
        .canonical_utc_milliseconds(input.created_at)
        .ok_or(Error::Clock)?;
    let report_milliseconds = wire
        .canonical_utc_milliseconds(input.report_created_at)
        .ok_or(Error::Clock)?;
    let valid_through_milliseconds = report_milliseconds
        .checked_add(7 * DAY_MILLISECONDS)
        .ok_or(Error::Clock)?;
    let valid_through = wire
        .format_utc_milliseconds(valid_through_milliseconds)
        .ok_or(Error::Clock)?;
    // preserve source chronology and fences
    if created_milliseconds < report_milliseconds
        || created_milliseconds > valid_through_milliseconds
        || !valid_hash(input.lifecycle_head_sha256)
        || !valid_fencing_token(input.fencing_token)
        || input
            .current
            .shadow_installed_receipt_sha256
            .as_deref()
            .is_some_and(|value| !valid_hash(value))
    {
        return Err(Error::Input);
    }
    let action = object(vec![
        ("actionKind", string("shadow")),
        ("candidateGraphSha256", string(&candidate_graph_sha256)),
        ("candidateSha256", string(candidate_sha256)),
        ("contractVersion", string(ACTION_VERSION)),
        ("createdAt", string(input.created_at)),
        (
            "expectedInstalledReceiptSha256",
            nullable_string(input.current.shadow_installed_receipt_sha256.as_deref()),
        ),
        (
            "expectedSettingsSha256",
            string(&input.current.settings_sha256),
        ),
        ("expectedSourceCommit", string(&input.current.commit)),
        ("expectedSourceRelease", string(&input.current.release)),
        ("family", string(family)),
        ("fencingToken", string(input.fencing_token)),
        ("fullMemberRootSha256", JsonValue::Null),
        ("lifecycleHeadSha256", string(input.lifecycle_head_sha256)),
        ("policyDecision", string("pending")),
        ("policyReportSha256", string(&input.report.report_sha256)),
        ("predecessorActionSha256", JsonValue::Null),
        ("reason", string("development_candidate")),
        ("reportCreatedAt", string(input.report_created_at)),
        ("siteKey", string("ballydidean")),
        ("validThrough", string(&valid_through)),
    ]);
    let action_bytes = wire.encode_sorted_json(&action).ok_or(Error::Codec)?;
    let action_sha256 = format_digest(wire.sha256(&action_bytes));
    let action_path =
        format!("config/forecast-adjustments/ballydidean/actions/sha256-{action_sha256}.json");
    let family_files = build_family_files(
        &wire,
        input.family,
        input.candidate_bytes,
        input.portable,
        &artifact,
    )?;
    let shadow_projection = object(vec![
        ("actionSha256", string(&action_sha256)),
        ("artifactSha256", string(artifact_sha256)),
        ("bundleSha256", string(artifact_sha256)),
        ("candidateGraphSha256", string(&candidate_graph_sha256)),
        ("candidateSha256", string(candidate_sha256)),
        ("contractVersion", string(SHADOW_PROJECTION_VERSION)),
        ("family", string(family)),
        ("paritySha256", string(&parity_sha256)),
        ("registration", registration.clone()),
        ("siteKey", string("ballydidean")),
    ]);
    let shadow_projection_bytes = wire
        .encode_sorted_json(&shadow_projection)
        .ok_or(Error::Codec)?;
    let shadow_projection_sha256 = format_digest(wire.sha256(&shadow_projection_bytes));
    let mut package_files = family_files.clone();
    package_files.extend([
        NativeMaintenanceDevelopmentPublicationFile {
            path: action_path.clone(),
            bytes: action_bytes.clone(),
        },
        NativeMaintenanceDevelopmentPublicationFile {
            path: format!(
                "config/forecast-adjustments/ballydidean/model-parity/{family}/sha256-{candidate_sha256}.json"
            ),
            bytes: input.parity.bytes.clone(),
        },
        NativeMaintenanceDevelopmentPublicationFile {
            path: format!(
                "config/forecast-adjustments/ballydidean/shadow-catalog/{family}/sha256-{candidate_sha256}.json"
            ),
            bytes: shadow_projection_bytes.clone(),
        },
    ]);
    package_files.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    let (package_total_bytes, package_root_sha256) = package_identity(&wire, &package_files)?;
    let (custody_binding, custody_binding_bytes, custody_binding_sha256, custody_segment) =
        custody_material(
            &wire,
            &action,
            &action_bytes,
            &action_sha256,
            input.candidate_bytes,
            &candidate_graph_sha256,
            input.portable,
            &artifact_kind,
            registration,
            input.report,
        )?;
    let release_date = adjustment_rolling_local_date_at(input.created_at)
        .map_err(|_| Error::Clock)?
        .replace('-', ".");
    Ok(NativeMaintenanceDevelopmentPublication {
        action,
        action_bytes,
        action_path,
        action_sha256,
        confirmation_registration_sha256: input
            .confirmation_preregistration
            .registration_sha256
            .clone(),
        custody_binding,
        custody_binding_bytes,
        custody_binding_sha256,
        custody_segment,
        family_files,
        package_files,
        package_root_sha256,
        package_total_bytes,
        release_date,
        release_run_id: format!("release-{}", &candidate_sha256[..32]),
        shadow_projection,
        shadow_projection_bytes,
        shadow_projection_sha256,
    })
}

/// Admit the exact v3 rolling registration and its final-LF domain identity.
fn admit_registration<'a>(
    wire: &NativeWirePrimitives,
    registration: &'a JsonValue,
    family: &str,
) -> Result<&'a JsonValue, NativeMaintenanceDevelopmentPublicationError> {
    use NativeMaintenanceDevelopmentPublicationError::Registration;

    // close the registration shape
    if !exact_keys(registration, REGISTRATION_KEYS)
        || text(registration, "siteKey").as_deref() != Some("ballydidean")
        || text(registration, "family").as_deref() != Some(family)
        || text(registration, "scheduleContractSha256").as_deref()
            != Some(ADJUSTMENT_ROLLING_SCHEDULE_SHA256)
    {
        return Err(Registration);
    }
    // validate immutable registration fields
    for key in [
        "artifactSha256",
        "candidateSha256",
        "cohortSha256",
        "epochWitnessSha256",
        "policySha256",
        "registrationSha256",
        "reservedKeySha256",
        "scheduleContractSha256",
        "sourceSha256",
    ] {
        // reject missing or malformed immutable coordinates
        if !text(registration, key).is_some_and(|value| valid_hash(&value)) {
            return Err(Registration);
        }
    }
    let predecessor = nullable_text(registration, "predecessorRegistrationSha256")?;
    // retain optional predecessor identity
    if predecessor
        .as_deref()
        .is_some_and(|value| !valid_hash(value))
    {
        return Err(Registration);
    }
    // validate schedule boundary fields
    for key in [
        "intervalEndAt",
        "intervalStartAt",
        "targetCutoffAt",
        "terminalAt",
    ] {
        // retain only canonical millisecond UTC schedule boundaries
        if text(registration, key)
            .and_then(|value| wire.canonical_utc_milliseconds(&value))
            .is_none()
        {
            return Err(Registration);
        }
    }
    let identity = format!(
        "{REGISTRATION_VERSION}\nballydidean\n{family}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        text(registration, "candidateSha256").ok_or(Registration)?,
        text(registration, "artifactSha256").ok_or(Registration)?,
        text(registration, "policySha256").ok_or(Registration)?,
        text(registration, "cohortSha256").ok_or(Registration)?,
        text(registration, "reservedKeySha256").ok_or(Registration)?,
        text(registration, "sourceSha256").ok_or(Registration)?,
        text(registration, "epochWitnessSha256").ok_or(Registration)?,
        text(registration, "scheduleContractSha256").ok_or(Registration)?,
        predecessor.as_deref().unwrap_or("none"),
        text(registration, "intervalStartAt").ok_or(Registration)?,
        text(registration, "intervalEndAt").ok_or(Registration)?,
        text(registration, "targetCutoffAt").ok_or(Registration)?,
        text(registration, "terminalAt").ok_or(Registration)?,
    );
    let expected = format_digest(wire.sha256(identity.as_bytes()));
    (text(registration, "registrationSha256").as_deref() == Some(expected.as_str()))
        .then_some(registration)
        .ok_or(Registration)
}

/// Admit the already computed source parity receipt and immutable address.
fn admit_parity(
    wire: &NativeWirePrimitives,
    parity: &NativeMaintenanceCandidateParityReceipt,
    family: &str,
    candidate_sha256: &str,
) -> Result<String, NativeMaintenanceDevelopmentPublicationError> {
    use NativeMaintenanceDevelopmentPublicationError::Parity;

    let value = canonical_object(wire, &parity.bytes).ok_or(Parity)?;
    let keys = [
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
    // bind receipt shape and hashes
    if value != parity.fixture
        || !exact_keys(&value, &keys)
        || text(&value, "contractVersion").as_deref() != Some(PARITY_VERSION)
        || text(&value, "family").as_deref() != Some(family)
        || text(&value, "candidateSha256").as_deref() != Some(candidate_sha256)
        || keys
            .iter()
            .filter(|key| key.ends_with("Sha256"))
            .any(|key| !text(&value, key).is_some_and(|hash| valid_hash(&hash)))
    {
        return Err(Parity);
    }
    let actual = format_digest(wire.sha256(&parity.bytes));
    (actual == parity.parity_sha256)
        .then_some(actual)
        .ok_or(Parity)
}

/// Bind the immutable attempt report to this monthly archived candidate.
fn admit_report(
    report: &NativeMaintenanceAttemptReport,
    family: &str,
    candidate_sha256: &str,
    candidate_graph_sha256: &str,
) -> Result<(), NativeMaintenanceDevelopmentPublicationError> {
    use NativeMaintenanceDevelopmentPublicationError::Report;

    let wire = NativeWirePrimitives;
    let actual = format_digest(wire.sha256(&report.canonical_bytes));
    // require the exact source terminal outcome used by production publication
    if actual != report.report_sha256
        || wire.decode_json(&report.canonical_bytes).as_ref() != Some(&report.value)
        || wire.encode_sorted_json(&report.value).as_deref() != Some(&report.canonical_bytes)
        || text(&report.value, "family").as_deref() != Some(family)
        || text(&report.value, "mode").as_deref() != Some("monthly")
        || text(&report.value, "reason").as_deref() != Some("development_candidate_archived")
        || text(&report.value, "state").as_deref() != Some("completed")
        || text(&report.value, "candidateSha256").as_deref() != Some(candidate_sha256)
        || text(&report.value, "candidateGraphSha256").as_deref() != Some(candidate_graph_sha256)
    {
        return Err(Report);
    }
    Ok(())
}

/// Select the exact inactive family file matrix without adding serving selectors.
fn build_family_files(
    wire: &NativeWirePrimitives,
    family: NativeMaintenanceCaptureFamily,
    candidate_bytes: &[u8],
    portable: &NativeMaintenancePortableCandidate,
    artifact: &JsonValue,
) -> Result<
    Vec<NativeMaintenanceDevelopmentPublicationFile>,
    NativeMaintenanceDevelopmentPublicationError,
> {
    use NativeMaintenanceDevelopmentPublicationError as Error;

    // rain retains its candidate-addressed technical receipt beside the immutable runtime artifact
    if family == NativeMaintenanceCaptureFamily::Rain {
        let rebuilt = build_native_rain_maintenance_artifact(candidate_bytes)
            .map_err(|_| Error::Candidate)?;
        // rederive rain's separately compiled technical package from the raw fit
        if rebuilt.candidate_sha256 != portable.candidate_sha256
            || rebuilt.artifact_sha256 != portable.artifact_sha256
            || rebuilt.artifact_bytes != portable.artifact_bytes
        {
            return Err(Error::Candidate);
        }
        let model_month = text(artifact, "modelMonth").ok_or(Error::Candidate)?;
        let artifact_path = format!(
            "config/forecast-adjustments/ballydidean/rain-runtime-artifacts/sha256-{}.json",
            portable.artifact_sha256
        );
        let registry = object(vec![
            (
                "activeArtifact",
                object(vec![("artifactSha256", string(&portable.artifact_sha256))]),
            ),
            (
                "contractVersion",
                string("forecast-adjustment-rain-runtime-registry/v1"),
            ),
            ("rawReason", JsonValue::Null),
            ("siteKey", string("ballydidean")),
        ]);
        let registry_bytes = wire.encode_sorted_json(&registry).ok_or(Error::Codec)?;
        let artifact_text =
            std::str::from_utf8(&portable.artifact_bytes).map_err(|_| Error::Candidate)?;
        let mut encoded_text = wire
            .encode_sorted_json(&string(artifact_text))
            .ok_or(Error::Codec)?;
        // canonical scalar JSON has one framing LF that is not part of the TypeScript literal
        if encoded_text.pop() != Some(b'\n') {
            return Err(Error::Codec);
        }
        let mut compiled = Vec::new();
        compiled.extend_from_slice(
            b"// generated by adjustment_rain_model_package.mjs from a sanitized fit candidate\n",
        );
        compiled.extend_from_slice(
            format!(
                "export const RAIN_HURDLE_WIND_ARTIFACT_SHA256 = \"{}\" as const;\n",
                portable.artifact_sha256
            )
            .as_bytes(),
        );
        compiled.extend_from_slice(b"export const RAIN_HURDLE_WIND_ARTIFACT_JSON = ");
        compiled.extend_from_slice(&encoded_text);
        compiled.extend_from_slice(b" as const;\n");
        let receipt = object(vec![
            (
                "artifactPath",
                string(&format!(
                    "rain-runtime-artifacts/sha256-{}.json",
                    portable.artifact_sha256
                )),
            ),
            ("artifactSha256", string(&portable.artifact_sha256)),
            ("candidateSha256", string(&portable.candidate_sha256)),
            (
                "compiledSourcePath",
                string("packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts"),
            ),
            (
                "compiledSourceSha256",
                string(&format_digest(wire.sha256(&compiled))),
            ),
            (
                "contractVersion",
                string("forecast-adjustment-rain-model-package/v1"),
            ),
            ("modelMonth", string(&model_month)),
            (
                "registrySha256",
                string(&format_digest(wire.sha256(&registry_bytes))),
            ),
        ]);
        let receipt_bytes = wire.encode_sorted_json(&receipt).ok_or(Error::Codec)?;
        // bind receipt to raw candidate bytes
        if format_digest(wire.sha256(candidate_bytes)) != portable.candidate_sha256 {
            return Err(Error::Candidate);
        }
        return Ok(vec![
            NativeMaintenanceDevelopmentPublicationFile {
                bytes: portable.artifact_bytes.clone(),
                path: artifact_path,
            },
            NativeMaintenanceDevelopmentPublicationFile {
                bytes: receipt_bytes,
                path: format!(
                    "config/forecast-adjustments/ballydidean/rain-model-packages/sha256-{}.json",
                    portable.candidate_sha256
                ),
            },
        ]);
    }
    // select the immutable bundle directory
    let directory = match family {
        NativeMaintenanceCaptureFamily::Temperature => {
            let (_, bundle_sha256) = admit_temperature_package(
                &portable.artifact_bytes,
                Some(portable.candidate_sha256.clone()),
            )
            .map_err(|_| Error::Candidate)?;
            // retain the artifact's self-declared bundle identity
            if bundle_sha256 != portable.artifact_sha256 {
                return Err(Error::Candidate);
            }
            "temperature-canary-bundles"
        }
        NativeMaintenanceCaptureFamily::Wind => {
            admit_package(portable).map_err(|_| Error::Candidate)?;
            "wind-canary-bundles"
        }
        NativeMaintenanceCaptureFamily::Rain => unreachable!(),
    };
    Ok(vec![NativeMaintenanceDevelopmentPublicationFile {
        bytes: portable.artifact_bytes.clone(),
        path: format!(
            "config/forecast-adjustments/ballydidean/{directory}/sha256-{}.json",
            portable.artifact_sha256
        ),
    }])
}

/// Derive the exact sorted package root while retaining the source size and privacy refusals.
fn package_identity(
    wire: &NativeWirePrimitives,
    files: &[NativeMaintenanceDevelopmentPublicationFile],
) -> Result<(usize, String), NativeMaintenanceDevelopmentPublicationError> {
    use NativeMaintenanceDevelopmentPublicationError::Package;

    let mut total = 0usize;
    let mut members = Vec::with_capacity(files.len());
    // retain package order in its root
    for file in files {
        // prohibit empty, unbounded, private, or repository-escaping material
        total = total.checked_add(file.bytes.len()).ok_or(Package)?;
        if file.bytes.is_empty()
            || file.bytes.len() > PACKAGE_MAXIMUM_BYTES
            || total > PACKAGE_MAXIMUM_BYTES
            || !valid_public_path(&file.path)
            || contains_private_material(&file.bytes)
        {
            return Err(Package);
        }
        members.push(object(vec![
            ("bytes", JsonValue::Number(file.bytes.len() as f64)),
            ("path", string(&file.path)),
            ("sha256", string(&format_digest(wire.sha256(&file.bytes)))),
        ]));
    }
    let bytes = wire
        .encode_sorted_json(&JsonValue::Array(members))
        .ok_or(NativeMaintenanceDevelopmentPublicationError::Codec)?;
    Ok((total, format_digest(wire.sha256(&bytes))))
}

/// Construct the exact source custody binding and seven-member graph segment.
#[allow(clippy::too_many_arguments)]
fn custody_material(
    wire: &NativeWirePrimitives,
    action: &JsonValue,
    action_bytes: &[u8],
    action_sha256: &str,
    candidate_bytes: &[u8],
    candidate_graph_sha256: &str,
    portable: &NativeMaintenancePortableCandidate,
    artifact_kind: &str,
    registration: &JsonValue,
    report: &NativeMaintenanceAttemptReport,
) -> Result<
    (JsonValue, Vec<u8>, String, NativeMaintenanceArchiveSegment),
    NativeMaintenanceDevelopmentPublicationError,
> {
    use NativeMaintenanceDevelopmentPublicationError as Error;

    let registration_sha256 =
        text(registration, "registrationSha256").ok_or(Error::Registration)?;
    let source_sha256 = text(registration, "sourceSha256").ok_or(Error::Registration)?;
    let graph_reference = object(vec![
        ("contractVersion", string(GRAPH_REFERENCE_VERSION)),
        ("graphSha256", string(candidate_graph_sha256)),
    ]);
    let graph_reference_bytes = wire
        .encode_sorted_json(&graph_reference)
        .ok_or(Error::Codec)?;
    let graph_reference_sha256 = format_digest(wire.sha256(&graph_reference_bytes));
    let registration_bytes = wire.encode_sorted_json(registration).ok_or(Error::Codec)?;
    let binding = object(vec![
        ("actionSha256", string(action_sha256)),
        ("artifactSha256", string(&portable.artifact_sha256)),
        ("candidateGraphSha256", string(candidate_graph_sha256)),
        ("candidateSha256", string(&portable.candidate_sha256)),
        ("contractVersion", string(CUSTODY_BINDING_VERSION)),
        ("policyReportSha256", string(&report.report_sha256)),
        ("registrationSha256", string(&registration_sha256)),
        ("sourceSha256", string(&source_sha256)),
    ]);
    let binding_bytes = wire.encode_sorted_json(&binding).ok_or(Error::Codec)?;
    let binding_sha256 = format_digest(wire.sha256(&binding_bytes));
    let family = text(action, "family").ok_or(Error::Input)?;
    let members = vec![
        member(CUSTODY_BINDING_VERSION, &binding_sha256, &binding_bytes)?,
        member(ACTION_VERSION, action_sha256, action_bytes)?,
        member(
            REGISTRATION_VERSION,
            &registration_sha256,
            &registration_bytes,
        )?,
        member(
            GRAPH_REFERENCE_VERSION,
            &graph_reference_sha256,
            &graph_reference_bytes,
        )?,
        member(
            &format!("forecast-adjustment-{family}-development-candidate/v1"),
            &portable.candidate_sha256,
            candidate_bytes,
        )?,
        member(
            artifact_kind,
            &portable.artifact_sha256,
            &portable.artifact_bytes,
        )?,
        member(
            "forecast-adjustment-maintenance-attempt/v2",
            &report.report_sha256,
            &report.canonical_bytes,
        )?,
    ];
    let cross_links = vec![
        link(&binding_sha256, "binds_public_action", action_sha256),
        link(
            &binding_sha256,
            "binds_shadow_registration",
            &registration_sha256,
        ),
        link(
            &binding_sha256,
            "binds_candidate_graph",
            &graph_reference_sha256,
        ),
        link(action_sha256, "binds_candidate", &portable.candidate_sha256),
        link(action_sha256, "binds_policy_report", &report.report_sha256),
        link(
            &registration_sha256,
            "binds_portable_artifact",
            &portable.artifact_sha256,
        ),
    ];
    Ok((
        binding,
        binding_bytes,
        binding_sha256,
        NativeMaintenanceArchiveSegment {
            members,
            cross_links,
        },
    ))
}

/// Build one typed archive member from its admitted lowercase digest.
fn member(
    kind: &str,
    identity: &str,
    payload: &[u8],
) -> Result<NativeMaintenanceArchiveSegmentMember, NativeMaintenanceDevelopmentPublicationError> {
    Ok(NativeMaintenanceArchiveSegmentMember {
        kind: kind.to_owned(),
        identity_sha256: digest_bytes(identity)
            .ok_or(NativeMaintenanceDevelopmentPublicationError::Codec)?,
        payload: payload.to_vec(),
    })
}

/// Build one source-ordered semantic cross-link.
fn link(from: &str, relation: &str, to: &str) -> NativeMaintenanceGraphCrossLink {
    NativeMaintenanceGraphCrossLink {
        from_identity_sha256: from.to_owned(),
        relation: relation.to_owned(),
        to_identity_sha256: to.to_owned(),
    }
}

/// Decode one canonical top-level object without accepting alternate bytes.
fn canonical_object(wire: &NativeWirePrimitives, bytes: &[u8]) -> Option<JsonValue> {
    let value = wire.decode_json(bytes)?;
    // require a canonical object rather than an alternate scalar or array encoding
    if !matches!(value, JsonValue::Object(_))
        || wire.encode_sorted_json(&value)?.as_slice() != bytes
    {
        return None;
    }
    Some(value)
}

/// Read one nullable source string without accepting another JSON type.
fn nullable_text(
    value: &JsonValue,
    key: &str,
) -> Result<Option<String>, NativeMaintenanceDevelopmentPublicationError> {
    // distinguish null from text
    match field(value, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .map(Some)
            .map_err(|_| NativeMaintenanceDevelopmentPublicationError::Registration),
        _ => Err(NativeMaintenanceDevelopmentPublicationError::Registration),
    }
}

/// Encode one optional string with explicit JSON null absence.
fn nullable_string(value: Option<&str>) -> JsonValue {
    value.map_or(JsonValue::Null, string)
}

/// Decode one exact lowercase hexadecimal SHA-256 address.
fn digest_bytes(value: &str) -> Option<[u8; 32]> {
    // close digest text before decoding
    if !valid_hash(value) {
        return None;
    }
    let mut bytes = [0u8; 32];
    // decode each fixed hexadecimal pair
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        // combine only the already admitted lowercase hexadecimal pair
        bytes[index] = hex_nibble(pair[0])?.checked_mul(16)? + hex_nibble(pair[1])?;
    }
    Some(bytes)
}

/// Convert one lowercase hexadecimal digit.
fn hex_nibble(value: u8) -> Option<u8> {
    // accept lowercase source digits only
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

/// Recognize the source's canonical positive decimal uint64 fence.
fn valid_fencing_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && !value.starts_with('0')
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

/// Recognize one exact lowercase forty-character Git commit.
fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Recognize the source's immutable calendar release namespace.
fn valid_release(value: &str) -> bool {
    // split the calendar and revision once
    let Some((date, revision)) = value.split_once('-') else {
        return false;
    };
    let bytes = date.as_bytes();
    bytes.len() == 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'.'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'.'
        && bytes[8..].iter().all(u8::is_ascii_digit)
        && !revision.is_empty()
        && !revision.starts_with('0')
        && revision.bytes().all(|byte| byte.is_ascii_digit())
}

/// Reject paths outside the source's public repository-relative file grammar.
fn valid_public_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 240
        && !value.starts_with('/')
        && !value.contains("..")
        && !value.contains("//")
        && !value.starts_with(".github/")
        && !value.contains('\\')
}

/// Reject the source's closed set of obvious credentials and host-private paths.
fn contains_private_material(bytes: &[u8]) -> bool {
    // retain public binary behavior
    let Ok(value) = std::str::from_utf8(bytes) else {
        return false;
    };
    // reject fixed private markers
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
    // preserve the source token-prefix refusal without adding a regular-expression package
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
