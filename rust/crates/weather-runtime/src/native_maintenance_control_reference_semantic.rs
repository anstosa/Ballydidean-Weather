//! Exact pre-month rain control-reference fitting, graph publication, and typed material.

use std::fmt::{self, Display, Formatter};

use weather_api::{RainHurdleWindPortableModel, admit_rain_portable_artifact};
use weather_contracts::{JsonValue, RainHurdleWindPortableArtifact};
use weather_domain::admit_rain_maintenance_control_state;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec, RevisionGapSha256};

use crate::{
    ADJUSTMENT_ROLLING_SCHEDULE_SHA256, ApiMaintenanceReceiptCanonicalValues,
    ApiMaintenanceShadowEvidenceValues, ApiRainControlPrimitives,
    NativeMaintenanceAdmittedFitRuntimeReadiness, NativeMaintenanceArchiveJournal,
    NativeMaintenanceArchiveSegment, NativeMaintenanceArchiveSegmentMember,
    NativeMaintenanceArchiveWallClock, NativeMaintenanceCandidateOwnerError,
    NativeMaintenanceCaptureFamily, NativeMaintenanceCasMemberValidator,
    NativeMaintenanceCasStreamSession, NativeMaintenanceCasStreamValidator,
    NativeMaintenanceControlReferenceInput, NativeMaintenanceControllerArchiveValidationSession,
    NativeMaintenanceControllerArchiveValidators, NativeMaintenanceDue, NativeMaintenanceDueMode,
    NativeMaintenanceFitInput, NativeMaintenanceFitRequest, NativeMaintenanceFitResult,
    NativeMaintenanceFitRuntimeReadiness, NativeMaintenanceGapArchiveValidationSession,
    NativeMaintenanceGapArchiveValidators, NativeMaintenanceGraphCrossLink,
    NativeMaintenanceHistoricalCatalogReady, NativeMaintenanceHistoricalFamily,
    NativeMaintenanceInstalledCandidateOwner, NativeMaintenanceJournalStore,
    NativeMaintenancePlaintextArchive, NativeModelEvidenceValues, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness,
    build_native_adjustment_historical_fit_projection,
    build_native_adjustment_rain_historical_rows,
    maintenance_canonical_json::valid_hash,
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
    publish_native_maintenance_archive_segment, verify_native_maintenance_due,
};

const CONTROL_FIT_INPUT_VERSION: &str = "rain-control-reference-fit-input/v1";
const CONTROL_REPORT_VERSION: &str = "rain-control-reference-fit/v1";
const CONTROL_STATE_VERSION: &str = "rain-maintenance-control-state/v1";
const CONTROL_RECIPE_SHA256: &str =
    "afa04352b82ccb9704a49863c2dd8724d5c129760ca6d9e4760cb1dda72fb34a";
const DAY_MILLISECONDS: i64 = 86_400_000;
const MAXIMUM_ARTIFACT_BYTES: usize = 8 * 1_024 * 1_024;
const RETAINED_RAIN_ARTIFACT: &[u8] =
    include_bytes!("../../weather-contracts/retained-rain-hurdle-wind.json");
const REPORT_KEYS: &[&str] = &[
    "calibrationEndAt",
    "calibrationStartAt",
    "contractVersion",
    "generatedAt",
    "legacyCalibrationStartAt",
    "modelMonth",
    "ordinalArtifact",
    "parityRows",
    "reason",
    "scales",
    "state",
    "support",
    "trainingMaximumValidAt",
];
const SCALE_KEYS: &[&str] = &["legacy", "recent", "recentSupported", "sameWindow"];
const SUPPORT_REPORT_KEYS: &[&str] = &[
    "calibrationDates",
    "calibrationHours",
    "calibrationRows",
    "calibrationWetDates",
    "calibrationWetHours",
    "effectiveDates",
    "effectiveWetDates",
    "legacyCalibrationRows",
    "legacyTrainingRows",
    "legacyTrainingWetRows",
    "trainingDates",
    "trainingHours",
    "trainingRows",
    "trainingWetDates",
    "trainingWetHours",
];
const SUPPORT_COUNTER_KEYS: &[&str] = &[
    "calibrationDates",
    "calibrationHours",
    "calibrationRows",
    "calibrationWetDates",
    "calibrationWetHours",
    "legacyCalibrationRows",
    "legacyTrainingRows",
    "legacyTrainingWetRows",
    "trainingDates",
    "trainingHours",
    "trainingRows",
    "trainingWetDates",
    "trainingWetHours",
];
const PARITY_ROW_KEYS: &[&str] = &["features", "prediction", "probabilities", "raw"];

/// Exact genuine numerical support refusal returned by the isolated rain fitter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceControlReferenceNoCandidateReason {
    InsufficientControlSupport,
    InsufficientOrdinalHeadSupport,
}

impl NativeMaintenanceControlReferenceNoCandidateReason {
    /// Return the predecessor's exact stable reason spelling.
    pub const fn as_str(self) -> &'static str {
        // preserve the closed fitter result vocabulary
        match self {
            Self::InsufficientControlSupport => "insufficient_control_support",
            Self::InsufficientOrdinalHeadSupport => "insufficient_ordinal_head_support",
        }
    }
}

/// Owned source-exact material retained across due validation and later release publication.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceControlReferenceSemanticMaterial {
    pub control_state_bytes: Vec<u8>,
    pub graph_manifest_sha256: String,
    pub input: NativeMaintenanceControlReferenceInput,
    pub ordinal_artifact_bytes: Vec<u8>,
    pub source_member_root_sha256: String,
    pub source_receipt_root_sha256: String,
}

impl NativeMaintenanceControlReferenceSemanticMaterial {
    /// Borrow the typed bytes and identities expected by the installed release action owner.
    pub fn as_installed_material_input(
        &self,
    ) -> crate::native_maintenance_installed_control_release::NativeMaintenanceInstalledControlMaterialInput<'_>
    {
        crate::native_maintenance_installed_control_release::NativeMaintenanceInstalledControlMaterialInput {
            control_state_bytes: &self.control_state_bytes,
            graph_manifest_sha256: &self.graph_manifest_sha256,
            ordinal_artifact_bytes: &self.ordinal_artifact_bytes,
            source_member_root_sha256: &self.source_member_root_sha256,
            source_receipt_root_sha256: &self.source_receipt_root_sha256,
        }
    }
}

/// Complete control semantic terminal without fabricating material for unsupported history.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeMaintenanceControlReferenceSemanticOutcome {
    NoCandidate {
        input_manifest_sha256: String,
        reason: NativeMaintenanceControlReferenceNoCandidateReason,
    },
    Ready {
        input_manifest_sha256: String,
        material: Box<NativeMaintenanceControlReferenceSemanticMaterial>,
    },
}

/// Closed control semantic failure at the first source-owned boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceControlReferenceSemanticError {
    Input,
    History,
    Runtime,
    Fit,
    Report,
    Artifact,
    State,
    Parity,
    Archive,
    Codec,
}

impl Display for NativeMaintenanceControlReferenceSemanticError {
    /// Render one value-free stable owning-boundary diagnostic.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "rain control semantic input is invalid",
            Self::History => "rain control historical assembly failed",
            Self::Runtime => "rain control fit runtime is unavailable",
            Self::Fit => "rain control fit failed",
            Self::Report => "rain control fit report differs",
            Self::Artifact => "rain control ordinal artifact differs",
            Self::State => "rain control state differs",
            Self::Parity => "rain control synthetic parity differs",
            Self::Archive => "rain control graph publication failed",
            Self::Codec => "rain control canonical encoding failed",
        })
    }
}

impl std::error::Error for NativeMaintenanceControlReferenceSemanticError {}

/// Closed registry accepting the exact control state plus existing controller ancestry.
#[derive(Default)]
pub struct NativeMaintenanceControlReferenceArchiveValidators;

impl NativeMaintenanceControlReferenceArchiveValidators {
    /// Construct the value-free exact mixed registry.
    pub const fn new() -> Self {
        Self
    }
}

impl NativeMaintenanceCasMemberValidator for NativeMaintenanceControlReferenceArchiveValidators {
    /// Delegate only the control state to its gap validator and every other kind to controller custody.
    fn validate(&mut self, kind: &str, payload: &[u8]) -> bool {
        // keep the state under its original typed validator rather than generic canonical JSON
        if kind == CONTROL_STATE_VERSION {
            NativeMaintenanceGapArchiveValidators::new().validate(kind, payload)
        } else {
            NativeMaintenanceControllerArchiveValidators::new().validate(kind, payload)
        }
    }
}

/// One selected exact validator session for a control-reference archive member.
pub enum NativeMaintenanceControlReferenceArchiveValidationSession {
    Control(NativeMaintenanceGapArchiveValidationSession),
    Controller(NativeMaintenanceControllerArchiveValidationSession),
}

impl NativeMaintenanceCasStreamValidator for NativeMaintenanceControlReferenceArchiveValidators {
    type Session = NativeMaintenanceControlReferenceArchiveValidationSession;

    /// Open the same member-specific bounded validator used by direct admission.
    fn begin(&mut self, kind: &str, length: u64) -> Option<Self::Session> {
        // preserve state validation separately from the controller ancestry registry
        if kind == CONTROL_STATE_VERSION {
            NativeMaintenanceGapArchiveValidators::new()
                .begin(kind, length)
                .map(NativeMaintenanceControlReferenceArchiveValidationSession::Control)
        } else {
            NativeMaintenanceControllerArchiveValidators::new()
                .begin(kind, length)
                .map(NativeMaintenanceControlReferenceArchiveValidationSession::Controller)
        }
    }
}

impl NativeMaintenanceCasStreamSession
    for NativeMaintenanceControlReferenceArchiveValidationSession
{
    /// Stream bytes only through the selected closed validator.
    fn write(&mut self, bytes: &[u8]) -> bool {
        // never switch schema after observing payload bytes
        match self {
            Self::Control(session) => session.write(bytes),
            Self::Controller(session) => session.write(bytes),
        }
    }

    /// Finish through the same selected closed validator.
    fn finish(self) -> bool {
        // preserve the owning validator's final canonical and schema checks
        match self {
            Self::Control(session) => session.finish(),
            Self::Controller(session) => session.finish(),
        }
    }
}

/// Concrete installed fit plus real durable journal/archive control semantic owner.
pub struct NativeMaintenanceInstalledControlReferenceSemanticOwner<'a> {
    source_commit: &'a str,
    candidate: &'a mut NativeMaintenanceInstalledCandidateOwner,
    journal: &'a mut NativeMaintenanceJournalStore,
    archive: NativeMaintenancePlaintextArchive<NativeMaintenanceControlReferenceArchiveValidators>,
}

impl<'a> NativeMaintenanceInstalledControlReferenceSemanticOwner<'a> {
    /// Bind the initialized candidate and journal to the actual fixed-root plaintext archive.
    pub fn initialize(
        source_commit: &'a str,
        candidate: &'a mut NativeMaintenanceInstalledCandidateOwner,
        journal: &'a mut NativeMaintenanceJournalStore,
    ) -> Result<Self, NativeMaintenanceControlReferenceSemanticError> {
        let archive = NativeMaintenancePlaintextArchive::initialize(
            NativeMaintenanceControlReferenceArchiveValidators::new(),
        )
        .map_err(|_| NativeMaintenanceControlReferenceSemanticError::Archive)?;
        Ok(Self {
            source_commit,
            candidate,
            journal,
            archive,
        })
    }

    /// Assemble, fit, validate, archive, and return one genuine pre-month reference.
    pub fn build(
        &mut self,
        due: &NativeMaintenanceDue,
        witness: &VerifiedNativeMaintenanceCaptureEpochWitness,
        catalog: &NativeMaintenanceHistoricalCatalogReady,
        generated_at: &str,
    ) -> Result<
        NativeMaintenanceControlReferenceSemanticOutcome,
        NativeMaintenanceControlReferenceSemanticError,
    > {
        use NativeMaintenanceControlReferenceSemanticError as Error;

        let due_month = control_due_month(due)?;
        let wire = NativeWirePrimitives;
        let requested = wire
            .canonical_utc_milliseconds(generated_at)
            .ok_or(Error::Input)?;
        let month_start = wire
            .canonical_utc_milliseconds(&format!("{due_month}-01T00:00:00.000Z"))
            .ok_or(Error::Input)?;
        // require the exact authenticated seven-day pre-month request window
        if requested
            < month_start
                .checked_sub(7 * DAY_MILLISECONDS)
                .ok_or(Error::Input)?
            || requested >= month_start
            || due.original_cutoff_at
                != wire
                    .format_utc_milliseconds(month_start - 7 * DAY_MILLISECONDS)
                    .ok_or(Error::Input)?
        {
            return Err(Error::Input);
        }
        let candidate_witness = self.candidate.witness().ok_or(Error::Input)?;
        // never fit a catalog under a different process-authenticated source epoch
        if candidate_witness != witness {
            return Err(Error::Input);
        }
        let historical = build_native_adjustment_historical_fit_projection(
            witness,
            NativeMaintenanceHistoricalFamily::Rain,
            due_month,
            &catalog.history,
        )
        .map_err(|_| Error::History)?;
        let assembled = build_native_adjustment_rain_historical_rows(witness, &historical)
            .map_err(|_| Error::History)?;
        let fit_input = control_fit_input(due_month, generated_at, assembled.rows)?;
        let fit_input = NativeMaintenanceFitInput { value: fit_input };
        let readiness = NativeMaintenanceFitRuntimeReadiness::native_installed(self.source_commit)
            .and_then(|mut owner| owner.capture())
            .map_err(|_| Error::Runtime)?;
        let runtime_value = wire
            .decode_json(readiness.manifest.canonical_json().as_bytes())
            .ok_or(Error::Runtime)?;
        let runtime = NativeMaintenanceAdmittedFitRuntimeReadiness {
            value: runtime_value,
        };
        let fit = self
            .candidate
            .run_fit(NativeMaintenanceFitRequest {
                family: NativeMaintenanceCaptureFamily::Rain,
                input: &fit_input,
                runtime_readiness: &runtime,
                runtime_readiness_sha256: &readiness.readiness_sha256,
            })
            .map_err(map_candidate_error)?;
        let witness_bytes = self.candidate.witness_bytes().ok_or(Error::Input)?;
        let witness_value = wire.decode_json(witness_bytes).ok_or(Error::Input)?;
        // require the retained bytes to be the exact verified witness supplied by the typed owner
        if wire.encode_sorted_json(&witness_value).as_deref() != Some(witness_bytes)
            || text(&witness_value, "witnessSha256").as_deref()
                != Some(witness.witness_sha256.as_str())
        {
            return Err(Error::Input);
        }
        let epoch_witness_sha256 = digest(witness_bytes);
        let completed = complete_fit(
            &fit,
            &readiness.readiness_sha256,
            due_month,
            generated_at,
            month_start,
            &epoch_witness_sha256,
            &assembled.source_member_root_sha256,
            &assembled.source_receipt_root_sha256,
        )?;
        let ready = match completed {
            CompletedFit::Ready(ready) => ready,
            CompletedFit::NoCandidate(reason) => {
                return Ok(
                    NativeMaintenanceControlReferenceSemanticOutcome::NoCandidate {
                        input_manifest_sha256: catalog.input_manifest_sha256.clone(),
                        reason,
                    },
                );
            }
        };
        let state_sha256 = digest(&ready.control_state_bytes);
        let artifact_sha256 = digest(&ready.ordinal_artifact_bytes);
        let segment = NativeMaintenanceArchiveSegment {
            members: vec![
                archive_member(
                    CONTROL_STATE_VERSION,
                    &state_sha256,
                    ready.control_state_bytes.clone(),
                )?,
                archive_member(
                    &ready.artifact_kind,
                    &artifact_sha256,
                    ready.ordinal_artifact_bytes.clone(),
                )?,
            ],
            cross_links: vec![NativeMaintenanceGraphCrossLink {
                from_identity_sha256: state_sha256.clone(),
                relation: "binds_ordinal_artifact".to_owned(),
                to_identity_sha256: artifact_sha256,
            }],
        };
        let due_key = format!("archive/control-reference/rain/{state_sha256}");
        let publication = publish_native_maintenance_archive_segment(
            &mut self.archive,
            &mut NativeMaintenanceArchiveJournal::new(self.journal),
            &mut NativeMaintenanceArchiveWallClock,
            &due_key,
            &segment,
        )
        .map_err(|_| Error::Archive)?;
        let graph_manifest_sha256 = format_digest(publication.manifest_object_sha256);
        let input = semantic_token(
            &ready.control_state_bytes,
            &graph_manifest_sha256,
            &ready.ordinal_artifact_bytes,
            &assembled.source_member_root_sha256,
            &assembled.source_receipt_root_sha256,
        )?;
        Ok(NativeMaintenanceControlReferenceSemanticOutcome::Ready {
            input_manifest_sha256: catalog.input_manifest_sha256.clone(),
            material: Box::new(NativeMaintenanceControlReferenceSemanticMaterial {
                control_state_bytes: ready.control_state_bytes,
                graph_manifest_sha256,
                input,
                ordinal_artifact_bytes: ready.ordinal_artifact_bytes,
                source_member_root_sha256: assembled.source_member_root_sha256,
                source_receipt_root_sha256: assembled.source_receipt_root_sha256,
            }),
        })
    }
}

enum CompletedFit {
    NoCandidate(NativeMaintenanceControlReferenceNoCandidateReason),
    Ready(ReadyFit),
}

struct ReadyFit {
    artifact_kind: String,
    control_state_bytes: Vec<u8>,
    ordinal_artifact_bytes: Vec<u8>,
}

/// Complete one exact isolated fit receipt into unsupported or typed source material.
#[allow(clippy::too_many_arguments)]
fn complete_fit(
    fit: &NativeMaintenanceFitResult,
    expected_runtime_readiness_sha256: &str,
    due_month: &str,
    requested_at: &str,
    month_start: i64,
    epoch_witness_sha256: &str,
    source_member_root_sha256: &str,
    source_receipt_root_sha256: &str,
) -> Result<CompletedFit, NativeMaintenanceControlReferenceSemanticError> {
    use NativeMaintenanceControlReferenceSemanticError as Error;

    // bind the fit bytes to the actual isolated native receipt before parsing its report
    if fit.contract_version != "adjustment-fit-sandbox/v1"
        || fit.family != NativeMaintenanceCaptureFamily::Rain
        || fit.candidate_sha256 != digest(&fit.candidate_json)
        || fit.runtime_readiness_sha256 != expected_runtime_readiness_sha256
        || !valid_hash(&fit.code_snapshot_sha256)
        || !valid_hash(&fit.input_snapshot_sha256)
        || !valid_hash(&fit.runtime_readiness_sha256)
    {
        return Err(Error::Fit);
    }
    let wire = NativeWirePrimitives;
    let report = wire.decode_json(&fit.candidate_json).ok_or(Error::Report)?;
    // close the source report before reading any value-bearing member
    if !exact_keys(&report, REPORT_KEYS)
        || text(&report, "contractVersion").as_deref() != Some(CONTROL_REPORT_VERSION)
        || text(&report, "modelMonth").as_deref() != Some(due_month)
    {
        return Err(Error::Report);
    }
    let generated_at = text(&report, "generatedAt").ok_or(Error::Report)?;
    let generated = wire
        .canonical_utc_milliseconds(&generated_at)
        .ok_or(Error::Report)?;
    let requested = wire
        .canonical_utc_milliseconds(requested_at)
        .ok_or(Error::Report)?;
    // prohibit output backdating or crossing the due-month boundary
    if generated < requested || generated >= month_start {
        return Err(Error::Report);
    }
    let state = text(&report, "state").ok_or(Error::Report)?;
    let reason = text(&report, "reason").ok_or(Error::Report)?;
    // preserve the disjoint unsupported report without creating archive material
    if state == "unsupported" {
        let reason = match reason.as_str() {
            "insufficient_control_support" => {
                NativeMaintenanceControlReferenceNoCandidateReason::InsufficientControlSupport
            }
            "insufficient_ordinal_head_support" => {
                NativeMaintenanceControlReferenceNoCandidateReason::InsufficientOrdinalHeadSupport
            }
            _ => return Err(Error::Report),
        };
        // prohibit partial material in either genuine unsupported fitter result
        if !is_null(&report, "ordinalArtifact")
            || !is_null(&report, "scales")
            || !is_null(&report, "support")
            || !is_null(&report, "trainingMaximumValidAt")
            || array(&report, "parityRows").is_none_or(|rows| !rows.is_empty())
        {
            return Err(Error::Report);
        }
        return Ok(CompletedFit::NoCandidate(reason));
    }
    // accept only the one source-supported material discriminator
    if state != "supported" || reason != "pre_month_reference" {
        return Err(Error::Report);
    }
    let scales = field(&report, "scales").ok_or(Error::Report)?;
    let support = field(&report, "support").ok_or(Error::Report)?;
    // close both fitted scalar/support records before constructing state
    if !exact_keys(scales, SCALE_KEYS) || !exact_keys(support, SUPPORT_REPORT_KEYS) {
        return Err(Error::Report);
    }
    let ordinal_artifact = field(&report, "ordinalArtifact").ok_or(Error::Report)?;
    let ordinal_artifact_bytes = wire
        .encode_sorted_json(ordinal_artifact)
        .filter(|bytes| bytes.len() <= MAXIMUM_ARTIFACT_BYTES)
        .ok_or(Error::Artifact)?;
    let artifact = admit_artifact(ordinal_artifact, due_month)?;
    verify_parity(
        &artifact,
        array(&report, "parityRows").ok_or(Error::Parity)?,
    )?;
    let ordinal_artifact_sha256 = digest(&ordinal_artifact_bytes);
    let unsigned_state = control_state_unsigned(
        &report,
        scales,
        support,
        epoch_witness_sha256,
        &ordinal_artifact_sha256,
        source_member_root_sha256,
        source_receipt_root_sha256,
    )?;
    let state_sha256 = hash_json(&unsigned_state)?;
    let control_state = control_state_ordered(&unsigned_state, &state_sha256)?;
    let control_state_bytes = wire
        .encode_ordered_json(&control_state)
        .ok_or(Error::Codec)?;
    let values = NativeModelEvidenceValues::new();
    let admitted = admit_rain_maintenance_control_state(
        &ApiRainControlPrimitives(&values),
        &control_state_bytes,
    )
    .ok_or(Error::State)?;
    // independently bind the generated state to the exact fit and historical identities
    if admitted.epoch_witness_sha256 != epoch_witness_sha256
        || admitted.model_month != due_month
        || admitted.ordinal_artifact_sha256 != ordinal_artifact_sha256
        || admitted.source_member_root_sha256 != source_member_root_sha256
        || admitted.source_receipt_root_sha256 != source_receipt_root_sha256
        || admitted.generated_at != generated_at
    {
        return Err(Error::State);
    }
    Ok(CompletedFit::Ready(ReadyFit {
        artifact_kind: artifact_kind(&artifact).to_owned(),
        control_state_bytes,
        ordinal_artifact_bytes,
    }))
}

/// Construct the source's exact control fitter request from genuine joined rows.
fn control_fit_input(
    due_month: &str,
    requested_at: &str,
    rows: Vec<JsonValue>,
) -> Result<JsonValue, NativeMaintenanceControlReferenceSemanticError> {
    let retained = NativeWirePrimitives
        .decode_json(RETAINED_RAIN_ARTIFACT)
        .ok_or(NativeMaintenanceControlReferenceSemanticError::Codec)?;
    let feature_names = field(&retained, "featureNames")
        .filter(|names| {
            matches!(names, JsonValue::Array(values) if values.len() == 107 && values.iter().all(|value| matches!(value, JsonValue::String(_))))
        })
        .cloned()
        .ok_or(NativeMaintenanceControlReferenceSemanticError::Codec)?;
    Ok(object(vec![
        ("contractVersion", string(CONTROL_FIT_INPUT_VERSION)),
        ("featureNames", feature_names),
        ("month", string(due_month)),
        ("requestedAt", string(requested_at)),
        ("trainingRows", JsonValue::Array(rows)),
    ]))
}

/// Build the unsigned state with the exact source field values and support order.
#[allow(clippy::too_many_arguments)]
fn control_state_unsigned(
    report: &JsonValue,
    scales: &JsonValue,
    support: &JsonValue,
    epoch_witness_sha256: &str,
    ordinal_artifact_sha256: &str,
    source_member_root_sha256: &str,
    source_receipt_root_sha256: &str,
) -> Result<JsonValue, NativeMaintenanceControlReferenceSemanticError> {
    use NativeMaintenanceControlReferenceSemanticError as Error;

    let recent_supported = boolean(scales, "recentSupported").ok_or(Error::Report)?;
    let legacy = finite_number(scales, "legacy").ok_or(Error::Report)?;
    let recent = finite_number(scales, "recent").ok_or(Error::Report)?;
    let same_window = finite_number(scales, "sameWindow").ok_or(Error::Report)?;
    let mut support_fields = Vec::with_capacity(15);
    // preserve the original support counter order while enforcing JS-safe integers
    for name in SUPPORT_COUNTER_KEYS {
        support_fields.push((
            name.encode_utf16().collect(),
            JsonValue::Number(safe_counter(support, name).ok_or(Error::Report)? as f64),
        ));
        // insert the effective support values at their exact source positions
        if *name == "calibrationWetHours" {
            support_fields.extend([
                (
                    "effectiveDates64".encode_utf16().collect(),
                    binary64(finite_number(support, "effectiveDates").ok_or(Error::Report)?),
                ),
                (
                    "effectiveWetDates64".encode_utf16().collect(),
                    binary64(finite_number(support, "effectiveWetDates").ok_or(Error::Report)?),
                ),
            ]);
        }
    }
    Ok(object(vec![
        (
            "calibrationEndAt",
            string(&required_text(report, "calibrationEndAt")?),
        ),
        (
            "calibrationStartAt",
            string(&required_text(report, "calibrationStartAt")?),
        ),
        ("contractVersion", string(CONTROL_STATE_VERSION)),
        ("epochWitnessSha256", string(epoch_witness_sha256)),
        (
            "generatedAt",
            string(&required_text(report, "generatedAt")?),
        ),
        (
            "legacyCalibrationStartAt",
            string(&required_text(report, "legacyCalibrationStartAt")?),
        ),
        ("legacyRawScale64", binary64(legacy)),
        ("modelMonth", string(&required_text(report, "modelMonth")?)),
        ("ordinalArtifactSha256", string(ordinal_artifact_sha256)),
        (
            "recentFallbackReason",
            string(if recent_supported {
                "recent_calibration"
            } else {
                "insufficient_effective_support"
            }),
        ),
        ("recentRawScale64", binary64(recent)),
        ("recentSupported", JsonValue::Boolean(recent_supported)),
        ("recipeSha256", string(CONTROL_RECIPE_SHA256)),
        ("sameWindowRawScale64", binary64(same_window)),
        (
            "scheduleContractSha256",
            string(ADJUSTMENT_ROLLING_SCHEDULE_SHA256),
        ),
        ("sourceMemberRootSha256", string(source_member_root_sha256)),
        (
            "sourceReceiptRootSha256",
            string(source_receipt_root_sha256),
        ),
        ("support", JsonValue::Object(support_fields)),
        (
            "trainingMaximumValidAt",
            string(&required_text(report, "trainingMaximumValidAt")?),
        ),
    ]))
}

/// Insert the self address at the source-defined ordered state position.
fn control_state_ordered(
    unsigned: &JsonValue,
    state_sha256: &str,
) -> Result<JsonValue, NativeMaintenanceControlReferenceSemanticError> {
    let JsonValue::Object(fields) = unsigned else {
        return Err(NativeMaintenanceControlReferenceSemanticError::State);
    };
    let mut ordered = Vec::with_capacity(fields.len() + 1);
    // retain all source fields and insert the self-address immediately before support
    for (name, value) in fields {
        // place the self address at its exact ordered-wire position
        if name.iter().copied().eq("support".encode_utf16()) {
            ordered.push(("stateSha256".encode_utf16().collect(), string(state_sha256)));
        }
        ordered.push((name.clone(), value.clone()));
    }
    Ok(JsonValue::Object(ordered))
}

/// Admit the exact portable artifact and bind it to the due month.
fn admit_artifact(
    value: &JsonValue,
    due_month: &str,
) -> Result<RainHurdleWindPortableArtifact, NativeMaintenanceControlReferenceSemanticError> {
    let values = NativeModelEvidenceValues::new();
    let artifact = admit_rain_portable_artifact(
        value,
        |document| values.decode_shadow_rain_artifact(document),
        |names| {
            let bytes = values.encode_shadow_ordered_json(names)?;
            ApiMaintenanceReceiptCanonicalValues::sha256(&values, &bytes)
        },
    )
    .ok_or(NativeMaintenanceControlReferenceSemanticError::Artifact)?;
    let (month, kind) = match &artifact {
        RainHurdleWindPortableArtifact::V1(value) => {
            (&value.modelMonth, "rain-hurdle-wind-runtime/v1")
        }
        RainHurdleWindPortableArtifact::V2(value) => {
            (&value.modelMonth, "rain-hurdle-wind-runtime/v2")
        }
    };
    // the source fitter must not return an artifact for another monthly control
    if month != due_month || text(value, "contractVersion").as_deref() != Some(kind) {
        return Err(NativeMaintenanceControlReferenceSemanticError::Artifact);
    }
    Ok(artifact)
}

/// Replay the three fixed synthetic rows through the independent native portable model.
fn verify_parity(
    artifact: &RainHurdleWindPortableArtifact,
    rows: &[JsonValue],
) -> Result<(), NativeMaintenanceControlReferenceSemanticError> {
    use NativeMaintenanceControlReferenceSemanticError::Parity;

    // retain the predecessor's exact three-row synthetic geometry
    if rows.len() != 3 {
        return Err(Parity);
    }
    let model = RainHurdleWindPortableModel::new(artifact).map_err(|_| Parity)?;
    // preserve the source's fixed zero, one, and null synthetic geometries
    for (index, row) in rows.iter().enumerate() {
        // close every synthetic row before reading values
        if !exact_keys(row, PARITY_ROW_KEYS) {
            return Err(Parity);
        }
        let raw = [0.5, 2.0, 4.0][index];
        // bind the fixed raw precipitation selected for this row
        if finite_number(row, "raw") != Some(raw) {
            return Err(Parity);
        }
        let features = array(row, "features").ok_or(Parity)?;
        // require the retained feature width before float32 allocation
        if features.len() != 107 {
            return Err(Parity);
        }
        let mut prepared = Vec::with_capacity(features.len());
        // require each immutable source synthetic feature before narrowing to float32
        for (position, value) in features.iter().enumerate() {
            // preserve the raw slot even for the otherwise-null third row
            let expected = if position == 5 {
                Some(raw)
            // retain all-null nonraw features only in the third row
            } else if index == 2 {
                None
            } else {
                Some(index as f64)
            };
            // narrow only an exact finite source value or its explicit null sentinel
            match (value, expected) {
                (JsonValue::Null, None) => prepared.push(f32::NAN),
                (JsonValue::Number(actual), Some(expected)) if *actual == expected => {
                    prepared.push(*actual as f32);
                }
                _ => return Err(Parity),
            }
        }
        // the source parity call supplies no validAt, so a v2 projection must refuse here too
        let predicted = model.predict(&prepared, None).map_err(|_| Parity)?;
        let probabilities = array(row, "probabilities").ok_or(Parity)?;
        let native = [
            finite_number(row, "prediction").ok_or(Parity)?,
            array_number(probabilities, 0).ok_or(Parity)?,
            array_number(probabilities, 1).ok_or(Parity)?,
            array_number(probabilities, 2).ok_or(Parity)?,
        ];
        let actual = [
            predicted.correctedPrecipitationMm,
            predicted.occurrenceProbabilityAtLeast0_1,
            predicted.occurrenceProbabilityAtLeast1_0,
            predicted.occurrenceProbabilityAtLeast2_5,
        ];
        // require the same strict less-than tolerance as the predecessor evaluator
        if probabilities.len() != 3
            || native
                .iter()
                .zip(actual)
                .any(|(native, actual)| !native.is_finite() || (native - actual).abs() >= 1e-5)
        {
            return Err(Parity);
        }
    }
    Ok(())
}

/// Build the exact closed due semantic token while retaining owned bytes separately.
fn semantic_token(
    control_state_bytes: &[u8],
    graph_manifest_sha256: &str,
    ordinal_artifact_bytes: &[u8],
    source_member_root_sha256: &str,
    source_receipt_root_sha256: &str,
) -> Result<NativeMaintenanceControlReferenceInput, NativeMaintenanceControlReferenceSemanticError>
{
    let wire = NativeWirePrimitives;
    Ok(NativeMaintenanceControlReferenceInput {
        value: object(vec![
            (
                "controlStateBytesBase64",
                string(
                    &wire
                        .encode_standard_base64(control_state_bytes)
                        .ok_or(NativeMaintenanceControlReferenceSemanticError::Codec)?,
                ),
            ),
            ("graphManifestSha256", string(graph_manifest_sha256)),
            (
                "ordinalArtifactBytesBase64",
                string(
                    &wire
                        .encode_standard_base64(ordinal_artifact_bytes)
                        .ok_or(NativeMaintenanceControlReferenceSemanticError::Codec)?,
                ),
            ),
            ("sourceMemberRootSha256", string(source_member_root_sha256)),
            (
                "sourceReceiptRootSha256",
                string(source_receipt_root_sha256),
            ),
        ]),
    })
}

/// Require the exact fixed pre-month control due coordinates.
fn control_due_month(
    due: &NativeMaintenanceDue,
) -> Result<&str, NativeMaintenanceControlReferenceSemanticError> {
    // reject ordinary monthly rain fits and every non-rain due
    if due.mode != NativeMaintenanceDueMode::Monthly
        || due.family != Some(NativeMaintenanceCaptureFamily::Rain)
        || due.scope != "control-reference/rain"
        || !verify_native_maintenance_due(due)
    {
        return Err(NativeMaintenanceControlReferenceSemanticError::Input);
    }
    due.due_key
        .strip_prefix("control-reference/rain/")
        .filter(|month| valid_month(month))
        .ok_or(NativeMaintenanceControlReferenceSemanticError::Input)
}

/// Build one archive member after parsing its exact lowercase content address.
fn archive_member(
    kind: &str,
    identity: &str,
    payload: Vec<u8>,
) -> Result<NativeMaintenanceArchiveSegmentMember, NativeMaintenanceControlReferenceSemanticError> {
    Ok(NativeMaintenanceArchiveSegmentMember {
        kind: kind.to_owned(),
        identity_sha256: *RevisionGapSha256::from_lower_hex(identity)
            .ok_or(NativeMaintenanceControlReferenceSemanticError::Archive)?
            .as_bytes(),
        payload,
    })
}

/// Map the concrete candidate owner without exposing its internal source paths.
fn map_candidate_error(
    _error: NativeMaintenanceCandidateOwnerError,
) -> NativeMaintenanceControlReferenceSemanticError {
    NativeMaintenanceControlReferenceSemanticError::Fit
}

/// Return the exact registered portable artifact contract.
fn artifact_kind(artifact: &RainHurdleWindPortableArtifact) -> &'static str {
    // preserve the typed variant rather than trusting a second parsed string
    match artifact {
        RainHurdleWindPortableArtifact::V1(_) => "rain-hurdle-wind-runtime/v1",
        RainHurdleWindPortableArtifact::V2(_) => "rain-hurdle-wind-runtime/v2",
    }
}

/// Hash exact bytes into lowercase source form.
fn digest(bytes: &[u8]) -> String {
    format_digest(RevisionArchiveCanonicalCodec::sha256(
        &NativeWirePrimitives,
        bytes,
    ))
}

/// Hash one recursively sorted source JSON value including its final LF.
fn hash_json(value: &JsonValue) -> Result<String, NativeMaintenanceControlReferenceSemanticError> {
    NativeWirePrimitives
        .encode_sorted_json(value)
        .map(|bytes| digest(&bytes))
        .ok_or(NativeMaintenanceControlReferenceSemanticError::Codec)
}

/// Encode one finite number as lowercase network-order binary64.
fn binary64(value: f64) -> JsonValue {
    string(&format!("{:016x}", value.to_bits()))
}

/// Require one exact top-level object key set independent of producer order.
fn exact_keys(value: &JsonValue, keys: &[&str]) -> bool {
    let JsonValue::Object(fields) = value else {
        return false;
    };
    fields.len() == keys.len()
        && keys.iter().all(|key| {
            fields
                .iter()
                .any(|(name, _)| name.iter().copied().eq(key.encode_utf16()))
        })
}

/// Borrow one object field without accepting duplicate-normalized aliases.
fn field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
}

/// Borrow one object string.
fn text(value: &JsonValue, key: &str) -> Option<String> {
    let JsonValue::String(units) = field(value, key)? else {
        return None;
    };
    String::from_utf16(units).ok()
}

/// Require one object string under the report error category.
fn required_text(
    value: &JsonValue,
    key: &str,
) -> Result<String, NativeMaintenanceControlReferenceSemanticError> {
    text(value, key).ok_or(NativeMaintenanceControlReferenceSemanticError::Report)
}

/// Borrow one object array.
fn array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(value, key)? else {
        return None;
    };
    Some(values)
}

/// Read one finite JSON number.
fn finite_number(value: &JsonValue, key: &str) -> Option<f64> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    number.is_finite().then_some(*number)
}

/// Read one finite number from an array index.
fn array_number(values: &[JsonValue], index: usize) -> Option<f64> {
    let JsonValue::Number(number) = values.get(index)? else {
        return None;
    };
    number.is_finite().then_some(*number)
}

/// Read one strict JSON boolean.
fn boolean(value: &JsonValue, key: &str) -> Option<bool> {
    let JsonValue::Boolean(boolean) = field(value, key)? else {
        return None;
    };
    Some(*boolean)
}

/// Read one nonnegative JavaScript-safe integer.
fn safe_counter(value: &JsonValue, key: &str) -> Option<u64> {
    let number = finite_number(value, key)?;
    (number >= 0.0 && number.fract() == 0.0 && number <= 9_007_199_254_740_991.0)
        .then_some(number as u64)
}

/// Recognize one explicit null field.
fn is_null(value: &JsonValue, key: &str) -> bool {
    matches!(field(value, key), Some(JsonValue::Null))
}

/// Preserve the original four-digit year and two-digit due month grammar.
fn valid_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && matches!(&bytes[5..], [b'0', b'1'..=b'9'] | [b'1', b'0'..=b'2'])
}

#[cfg(test)]
#[path = "native_maintenance_control_reference_semantic_test.rs"]
mod tests;
