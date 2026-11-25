//! Build exact pre-holdout candidate and report bytes from admitted native folds.

use weather_contracts::{
    ForecastAdjustmentCandidateV2 as Candidate,
    ForecastAdjustmentCandidateV2AlgorithmContractVersionLiteral as AlgorithmVersion,
    ForecastAdjustmentCandidateV2ContractVersionLiteral as CandidateVersion,
    ForecastAdjustmentCandidateV2RuntimeFingerprint as RuntimeFingerprint,
    ForecastAdjustmentCandidateV2SiteKeyLiteral as CandidateSite,
    ForecastAdjustmentCandidateV2TimezoneLiteral as CandidateTimezone,
    ForecastAdjustmentCoefficient, ForecastAdjustmentDevelopmentReportV1 as DevelopmentReport,
    ForecastAdjustmentDevelopmentReportV1ContractVersionLiteral as DevelopmentVersion,
    ForecastAdjustmentDevelopmentReportV1GeneratedBeforeHoldoutLiteral as BeforeHoldout,
    ForecastAdjustmentForecastIdentity as ForecastIdentity,
    ForecastAdjustmentForecastIdentityCohortLiteral as ForecastCohort,
    ForecastAdjustmentForecastIdentityReferenceKindLiteral as ReferenceKind,
    ForecastAdjustmentInsufficientDataReportV1 as InsufficientReport,
    ForecastAdjustmentInsufficientDataReportV1ContractVersionLiteral as InsufficientVersion,
    ForecastAdjustmentInsufficientDataReportV1StateLiteral as InsufficientState,
    ForecastAdjustmentMetric as Metric, ForecastAdjustmentMetricBand,
    ForecastAdjustmentMetricPolicy, ForecastAdjustmentTrainingEnvelope,
    ForecastAdjustmentTrainingEnvelopeMetricEnum as EnvelopeMetric,
    ForecastAdjustmentTrainingProvenance, ForecastLeadBandKey as LeadBand,
    ForecastObservationStation, JsonValue,
};
use weather_database::Sha256Digest;
use weather_worker::wind_maintenance_development::{
    WindAuxiliaryModel, WindDevelopmentQualification, forecast_adjustment_metric_policies,
    validate_wind_development_fold, wind_development_pair_passes,
};

use crate::ApiMaintenanceShadowEvidenceValues;
use crate::api_shadow_legacy_runtime_candidate::{
    CANONICAL_FORECAST_IDENTITY_SHA256, CANONICAL_TRAINING_PROVENANCE_SHA256,
    METRIC_POLICIES_SHA256,
};
use crate::maintenance_canonical_json::valid_hash;
use crate::wind_maintenance_fit::valid_wind_due_month;
use crate::wind_maintenance_material::{
    auxiliary_model, coefficient, coefficients, development_fold, metric_band, object, text,
    wire_text,
};

/// Identity of the earlier phase, admitted by the manifest and calendar owner.
#[derive(Clone, Copy, Debug)]
pub struct WindMaintenanceCandidateIdentity<'a> {
    pub due_month: &'a str,
    pub d0: &'a str,
    pub d401: &'a str,
    pub snapshot_manifest_sha256: &'a Sha256Digest,
    pub final_training_cutoff: &'a str,
    pub forecast_identity: &'a ForecastIdentity,
    pub runtime_fingerprint: &'a RuntimeFingerprint,
    pub training_provenance: &'a ForecastAdjustmentTrainingProvenance,
}

/// Prepared candidate and development bytes; neither can authorize publication or confirmation.
#[derive(Debug, PartialEq)]
pub struct WindMaintenanceCandidateArtifacts {
    pub candidate: Candidate,
    pub development_report: DevelopmentReport,
    pub candidate_bytes: Vec<u8>,
    pub development_report_bytes: Vec<u8>,
    pub due_month: String,
}

/// Original refusal or a pre-holdout candidate that has not been published.
#[derive(Debug, PartialEq)]
pub enum PreparedWindMaintenanceCandidate {
    Insufficient {
        report: InsufficientReport,
        bytes: Vec<u8>,
    },
    DevelopmentCandidate(Box<WindMaintenanceCandidateArtifacts>),
}

/// A failed typed input or approved canonical-value operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindMaintenanceCandidateError {
    Identity,
    DevelopmentFolds,
    InvalidReason,
    CanonicalBytes,
    Digest,
}

/// Hash the original station model preimage through the selected runtime capability.
pub fn wind_maintenance_auxiliary_model_sha256(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    model: WindAuxiliaryModel<'_>,
) -> Result<Sha256Digest, WindMaintenanceCandidateError> {
    digest(values, &auxiliary_model(&model))
}

/// Finish the original canonical development candidate or sanitized insufficiency report.
pub fn prepare_wind_maintenance_candidate(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    identity: &WindMaintenanceCandidateIdentity<'_>,
    stations: &[ForecastObservationStation],
    qualification: WindDevelopmentQualification,
) -> Result<PreparedWindMaintenanceCandidate, WindMaintenanceCandidateError> {
    // preserve original month and digest spelling even on ordinary insufficiency
    if !valid_wind_due_month(identity.due_month)
        || !valid_hash(identity.snapshot_manifest_sha256.as_str())
    {
        return Err(WindMaintenanceCandidateError::Identity);
    }
    // preserve the original no-development-qualification path
    let WindDevelopmentQualification::Qualified(mut qualified) = qualification else {
        return prepare_wind_insufficiency(
            values,
            identity.snapshot_manifest_sha256,
            vec!["development_loso_qualification".to_owned()],
        );
    };
    qualified.sort_unstable_by_key(|item| pair_key(&item.pair));
    // require the exact original fold identity and promotion before hashing any report
    if qualified.is_empty()
        || qualified
            .windows(2)
            .any(|pair| pair[0].pair == pair[1].pair)
        || qualified.iter().any(|item| {
            wind_development_pair_passes(&item.pair, &item.folds) != Ok(true)
                || item
                    .folds
                    .iter()
                    .any(|fold| validate_wind_development_fold(fold, stations).is_err())
        })
    {
        return Err(WindMaintenanceCandidateError::DevelopmentFolds);
    }

    let metric_policies = forecast_adjustment_metric_policies().to_vec();
    let authority = [
        (
            forecast_identity(identity.forecast_identity),
            CANONICAL_FORECAST_IDENTITY_SHA256,
        ),
        (
            training_provenance(identity.training_provenance),
            CANONICAL_TRAINING_PROVENANCE_SHA256,
        ),
        (policies(&metric_policies), METRIC_POLICIES_SHA256),
    ];
    // bind both lineages and the frozen policy table to the existing canonical owner
    if values
        .canonical_utc_epoch_milliseconds(identity.final_training_cutoff)
        .is_none()
    {
        return Err(WindMaintenanceCandidateError::Identity);
    }
    // reuse the retained canonical identity checks rather than adopting a parallel lineage
    for (node, expected) in authority {
        // compare exact owner-produced digests
        if digest(values, &node)?.as_str() != expected {
            return Err(WindMaintenanceCandidateError::Identity);
        }
    }

    let enabled = qualified
        .iter()
        .map(|item| item.pair.clone())
        .collect::<Vec<_>>();
    // an incomplete wind mask cannot become a recurring candidate
    if !complete_wind_mask(&enabled) {
        return prepare_wind_insufficiency(
            values,
            identity.snapshot_manifest_sha256,
            vec!["maintenance_exact_thirteen_pairs".to_owned()],
        );
    }
    let mut report = DevelopmentReport {
        contractVersion: DevelopmentVersion,
        developmentReportSha256: String::new(),
        enabledMetricBands: enabled.clone(),
        folds: Vec::with_capacity(enabled.len() * 5),
        generatedBeforeHoldout: BeforeHoldout,
    };
    let mut fitted = Vec::new();
    let mut envelopes = Vec::with_capacity(enabled.len());
    // gather pair-sorted folds and envelopes before sorting coefficients by their actual wire bytes
    for item in qualified {
        report.folds.extend(item.folds);
        fitted.extend(item.coefficients);
        envelopes.push(item.envelope);
    }
    let mut report_value = unsigned_development_report(&report);
    report.developmentReportSha256 =
        seal_object(values, &mut report_value, "developmentReportSha256")?;
    let development_report_bytes = encode(values, &report_value)?;

    let fitted = sort_candidate_coefficients(values, fitted)?;
    let coefficient_payload_sha256 = digest(values, &coefficients(&fitted))?;
    let epoch_id = digest(
        values,
        &object([
            ("d0", text(identity.d0)),
            ("d401", text(identity.d401)),
            (
                "snapshotManifestSha256",
                text(identity.snapshot_manifest_sha256.as_str()),
            ),
        ]),
    )?;
    let mut candidate = Candidate {
        algorithmContractVersion: AlgorithmVersion,
        candidateArtifactSha256: String::new(),
        coefficientPayloadSha256: coefficient_payload_sha256.as_str().to_owned(),
        coefficients: fitted,
        contractVersion: CandidateVersion,
        developmentReportSha256: report.developmentReportSha256.clone(),
        enabledMetricBands: enabled,
        evaluationEpochId: epoch_id.as_str().to_owned(),
        exportManifestSha256: identity.snapshot_manifest_sha256.as_str().to_owned(),
        finalTrainingCutoff: identity.final_training_cutoff.to_owned(),
        forecastIdentity: identity.forecast_identity.clone(),
        metricPolicies: metric_policies,
        runtimeFingerprint: identity.runtime_fingerprint.clone(),
        siteKey: CandidateSite,
        timezone: CandidateTimezone,
        trainingEnvelopes: envelopes,
        trainingProvenance: identity.training_provenance.clone(),
    };
    let mut candidate_value = unsigned_candidate(&candidate);
    candidate.candidateArtifactSha256 =
        seal_object(values, &mut candidate_value, "candidateArtifactSha256")?;
    let candidate_bytes = encode(values, &candidate_value)?;
    Ok(PreparedWindMaintenanceCandidate::DevelopmentCandidate(
        Box::new(WindMaintenanceCandidateArtifacts {
            candidate,
            development_report: report,
            candidate_bytes,
            development_report_bytes,
            due_month: identity.due_month.to_owned(),
        }),
    ))
}

/// Emit the exact original bound reason report with no fitted or holdout fields.
pub(crate) fn prepare_wind_insufficiency(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    snapshot: &Sha256Digest,
    mut failed_gates: Vec<String>,
) -> Result<PreparedWindMaintenanceCandidate, WindMaintenanceCandidateError> {
    failed_gates.sort_unstable();
    failed_gates.dedup();
    // retain only the original nonempty ASCII reason segments and verified digest spelling
    if !valid_hash(snapshot.as_str())
        || failed_gates.is_empty()
        || failed_gates.iter().any(|gate| {
            gate.split('_').any(|segment| {
                segment.is_empty()
                    || !segment
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            })
        })
    {
        return Err(WindMaintenanceCandidateError::InvalidReason);
    }
    let mut report_value = unsigned_insufficiency(snapshot, &failed_gates);
    let report_sha256 = seal_object(values, &mut report_value, "reportSha256")?;
    Ok(PreparedWindMaintenanceCandidate::Insufficient {
        report: InsufficientReport {
            contractVersion: InsufficientVersion,
            failedGates: failed_gates,
            reportSha256: report_sha256,
            snapshotManifestSha256: snapshot.as_str().to_owned(),
            state: InsufficientState,
        },
        bytes: encode(values, &report_value)?,
    })
}

/// Use the original wire text order for enabled pairs and their corresponding tables.
fn pair_key(pair: &ForecastAdjustmentMetricBand) -> (&'static str, &'static str) {
    (
        wire_name(pair.metric, Metric::WIRE_VALUES),
        wire_name(pair.leadBand, LeadBand::WIRE_VALUES),
    )
}

/// Borrow the original generated enum spelling.
fn wire_name<T: Copy + PartialEq>(value: T, variants: &[(&'static str, T)]) -> &'static str {
    variants
        .iter()
        .find_map(|(name, candidate)| (*candidate == value).then_some(*name))
        .expect("generated wire table is exhaustive")
}

/// Require seven speed pairs and the other six gust pairs, following the original mask.
fn complete_wind_mask(pairs: &[ForecastAdjustmentMetricBand]) -> bool {
    pairs.len() == 13
        && pairs.iter().all(|pair| match pair.metric {
            Metric::WindSpeedMps => true,
            Metric::WindGustMps => pair.leadBand != LeadBand::Value049072,
            _ => false,
        })
}

/// Derive sortable ECMAScript spellings once per coefficient, not once per comparison.
fn sort_candidate_coefficients(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    fitted: Vec<ForecastAdjustmentCoefficient>,
) -> Result<Vec<ForecastAdjustmentCoefficient>, WindMaintenanceCandidateError> {
    let mut keyed = fitted
        .into_iter()
        .map(|cell| {
            // these objects contain only ASCII enum labels and finite numeric wire values
            Ok((encode(values, &coefficient(&cell))?, cell))
        })
        .collect::<Result<Vec<_>, _>>()?;
    keyed.sort_by(|(left, _), (right, _)| left.cmp(right));
    Ok(keyed.into_iter().map(|(_, cell)| cell).collect())
}

/// Serialize solely through the existing sorted canonical owner.
fn encode(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    value: &JsonValue,
) -> Result<Vec<u8>, WindMaintenanceCandidateError> {
    values
        .encode_shadow_sorted_json(value)
        .filter(|bytes| bytes.ends_with(b"\n") && bytes.len() > 1 && !bytes.ends_with(b"\n\n"))
        .ok_or(WindMaintenanceCandidateError::CanonicalBytes)
}

/// Digest the canonical LF-inclusive preimage through the existing owner.
fn digest(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    value: &JsonValue,
) -> Result<Sha256Digest, WindMaintenanceCandidateError> {
    let raw = encode(values, value)?;
    values
        .sha256(&raw)
        .filter(|hash| valid_hash(hash.as_str()))
        .ok_or(WindMaintenanceCandidateError::Digest)
}

/// Add exactly one original self-digest after hashing the unsigned object.
fn seal_object(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    value: &mut JsonValue,
    key: &str,
) -> Result<String, WindMaintenanceCandidateError> {
    let sha = digest(values, value)?;
    add_text_field(value, key, sha.as_str())?;
    Ok(sha.as_str().to_owned())
}

/// Append the original field after the unsigned canonical preimage is complete.
fn add_text_field(
    value: &mut JsonValue,
    key: &str,
    field: &str,
) -> Result<(), WindMaintenanceCandidateError> {
    let JsonValue::Object(fields) = value else {
        return Err(WindMaintenanceCandidateError::CanonicalBytes);
    };
    fields.push((key.encode_utf16().collect(), text(field)));
    Ok(())
}

/// Project the original pre-holdout report before adding its own digest.
fn unsigned_development_report(report: &DevelopmentReport) -> JsonValue {
    object([
        ("contractVersion", text(DevelopmentVersion::WIRE_VALUE)),
        (
            "enabledMetricBands",
            metric_bands(&report.enabledMetricBands),
        ),
        (
            "folds",
            JsonValue::Array(report.folds.iter().map(development_fold).collect()),
        ),
        (
            "generatedBeforeHoldout",
            JsonValue::Boolean(BeforeHoldout::WIRE_VALUE),
        ),
    ])
}

/// Project the original insufficiency document before adding its own digest.
fn unsigned_insufficiency(snapshot: &Sha256Digest, failed_gates: &[String]) -> JsonValue {
    object([
        ("contractVersion", text(InsufficientVersion::WIRE_VALUE)),
        (
            "failedGates",
            JsonValue::Array(failed_gates.iter().map(|gate| text(gate)).collect()),
        ),
        ("snapshotManifestSha256", text(snapshot.as_str())),
        ("state", text(InsufficientState::WIRE_VALUE)),
    ])
}

/// Encode the original sandbox output, preserving the underlying insufficiency-report digest.
pub fn encode_wind_maintenance_fit_publication(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    due_month: &str,
    prepared: &PreparedWindMaintenanceCandidate,
) -> Result<Vec<u8>, WindMaintenanceCandidateError> {
    const FIT_VERSION: &str = "wind-maintenance-fit/v2";
    const MAXIMUM_OUTPUT_BYTES: usize = 8 * 1_024 * 1_024;
    // keep the wrapper month in the same original wire domain
    if !valid_wind_due_month(due_month) {
        return Err(WindMaintenanceCandidateError::Identity);
    }
    let doc = match prepared {
        PreparedWindMaintenanceCandidate::DevelopmentCandidate(artifact) => {
            // require the same original month as the admitted fit identity
            if due_month != artifact.due_month {
                return Err(WindMaintenanceCandidateError::Identity);
            }
            let mut candidate = unsigned_candidate(&artifact.candidate);
            add_text_field(
                &mut candidate,
                "candidateArtifactSha256",
                &artifact.candidate.candidateArtifactSha256,
            )?;
            let mut report = unsigned_development_report(&artifact.development_report);
            add_text_field(
                &mut report,
                "developmentReportSha256",
                &artifact.development_report.developmentReportSha256,
            )?;
            object([
                ("contractVersion", text(FIT_VERSION)),
                ("state", text("development_candidate")),
                ("candidate", candidate),
                ("developmentReport", report),
                ("dueMonth", text(due_month)),
                ("confirmationOpened", JsonValue::Boolean(false)),
            ])
        }
        PreparedWindMaintenanceCandidate::Insufficient { report, .. } => object([
            ("contractVersion", text(FIT_VERSION)),
            ("state", text("no_candidate")),
            (
                "failedGates",
                JsonValue::Array(report.failedGates.iter().map(|gate| text(gate)).collect()),
            ),
            ("reportSha256", text(&report.reportSha256)),
            (
                "snapshotManifestSha256",
                text(&report.snapshotManifestSha256),
            ),
            ("dueMonth", text(due_month)),
            ("confirmationOpened", JsonValue::Boolean(false)),
            ("reason", text("insufficient_development_support")),
        ]),
    };
    let bytes = encode(values, &doc)?;
    // retain the original exact output ceiling before handing bytes to the process owner
    if bytes.len() > MAXIMUM_OUTPUT_BYTES {
        return Err(WindMaintenanceCandidateError::CanonicalBytes);
    }
    Ok(bytes)
}

/// Project the unsigned fitted artifact without any post-fit evidence.
fn unsigned_candidate(candidate: &Candidate) -> JsonValue {
    object([
        (
            "algorithmContractVersion",
            text(AlgorithmVersion::WIRE_VALUE),
        ),
        (
            "coefficientPayloadSha256",
            text(&candidate.coefficientPayloadSha256),
        ),
        ("coefficients", coefficients(&candidate.coefficients)),
        ("contractVersion", text(CandidateVersion::WIRE_VALUE)),
        (
            "developmentReportSha256",
            text(&candidate.developmentReportSha256),
        ),
        (
            "enabledMetricBands",
            metric_bands(&candidate.enabledMetricBands),
        ),
        ("evaluationEpochId", text(&candidate.evaluationEpochId)),
        (
            "exportManifestSha256",
            text(&candidate.exportManifestSha256),
        ),
        ("finalTrainingCutoff", text(&candidate.finalTrainingCutoff)),
        (
            "forecastIdentity",
            forecast_identity(&candidate.forecastIdentity),
        ),
        ("metricPolicies", policies(&candidate.metricPolicies)),
        (
            "runtimeFingerprint",
            object([
                ("icuVersion", text(&candidate.runtimeFingerprint.icuVersion)),
                (
                    "tzdataVersion",
                    text(&candidate.runtimeFingerprint.tzdataVersion),
                ),
            ]),
        ),
        ("siteKey", text(CandidateSite::WIRE_VALUE)),
        ("timezone", text(CandidateTimezone::WIRE_VALUE)),
        (
            "trainingEnvelopes",
            JsonValue::Array(candidate.trainingEnvelopes.iter().map(envelope).collect()),
        ),
        (
            "trainingProvenance",
            training_provenance(&candidate.trainingProvenance),
        ),
    ])
}

/// Project the canonical served source identity.
fn forecast_identity(identity: &ForecastIdentity) -> JsonValue {
    object([
        ("adapterVersion", text(&identity.adapterVersion)),
        ("cohort", text(ForecastCohort::WIRE_VALUE)),
        ("contractEpoch", text(&identity.contractEpoch)),
        ("dataset", text(&identity.dataset)),
        ("referenceKind", text(ReferenceKind::WIRE_VALUE)),
        (
            "sourceConfigFingerprint",
            text(&identity.sourceConfigFingerprint),
        ),
        ("sourceKey", text(&identity.sourceKey)),
        ("upstreamModel", text(&identity.upstreamModel)),
    ])
}

/// Project the exact original observation, aggregation, and weight lineage.
fn training_provenance(provenance: &ForecastAdjustmentTrainingProvenance) -> JsonValue {
    object([
        (
            "aggregationContractSha256",
            text(&provenance.aggregationContractSha256),
        ),
        (
            "coordinateManifestSha256",
            text(&provenance.coordinateManifestSha256),
        ),
        (
            "metricEligibilitySha256",
            text(&provenance.metricEligibilitySha256),
        ),
        (
            "observationSourceLineageSha256",
            text(&provenance.observationSourceLineageSha256),
        ),
        (
            "observationStationManifestSha256",
            text(&provenance.observationStationManifestSha256),
        ),
        ("spatialWeightSha256", text(&provenance.spatialWeightSha256)),
    ])
}

/// Project the frozen policy table in source order.
fn policies(input: &[ForecastAdjustmentMetricPolicy]) -> JsonValue {
    JsonValue::Array(
        input
            .iter()
            .map(|policy| {
                object([
                    (
                        "correctionMaximum",
                        JsonValue::Number(policy.correctionMaximum),
                    ),
                    (
                        "correctionMinimum",
                        JsonValue::Number(policy.correctionMinimum),
                    ),
                    ("finalMaximum", JsonValue::Number(policy.finalMaximum)),
                    (
                        "finalMaximumExclusive",
                        JsonValue::Boolean(policy.finalMaximumExclusive),
                    ),
                    ("finalMinimum", JsonValue::Number(policy.finalMinimum)),
                    ("metric", wire_text(policy.metric, Metric::WIRE_VALUES)),
                    (
                        "wrapsFinalValue",
                        JsonValue::Boolean(policy.wrapsFinalValue),
                    ),
                ])
            })
            .collect(),
    )
}

/// Project one original scalar raw-value envelope.
fn envelope(input: &ForecastAdjustmentTrainingEnvelope) -> JsonValue {
    object([
        ("leadBand", wire_text(input.leadBand, LeadBand::WIRE_VALUES)),
        ("maximum", JsonValue::Number(input.maximum)),
        (
            "metric",
            wire_text(input.metric, EnvelopeMetric::WIRE_VALUES),
        ),
        ("minimum", JsonValue::Number(input.minimum)),
    ])
}

/// Project the original order already established by the candidate builder.
fn metric_bands(values: &[ForecastAdjustmentMetricBand]) -> JsonValue {
    JsonValue::Array(values.iter().map(metric_band).collect())
}
