//! Bind the generated wind canary projection to its verified canonical source graph.

use weather_contracts::{
    self as c, ForecastAdjustmentWindCanaryAuthorizationV1,
    ForecastAdjustmentWindCanaryAuthorizationV1ContractVersionLiteral,
    ForecastAdjustmentWindCanaryAuthorizationV2,
    ForecastAdjustmentWindCanaryAuthorizationV2ContractVersionLiteral,
    ForecastAdjustmentWindCanaryCandidateV1, ForecastAdjustmentWindCanaryRuntimeBundle,
    ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral,
    ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral,
    ForecastAdjustmentWindCanaryTrainingIdentity, ForecastAdjustmentWindCanaryTransferReportV1,
    JsonValue,
};

use crate::api_forecast_model_projection::{
    array, project_coefficient, project_envelope, project_forecast_identity, project_metric_band,
    project_training_provenance, unsigned_field,
};
use crate::api_shadow_legacy_values::{boolean_field, number_field, text_equals};
use crate::api_standard_runtime_projection::{
    coefficient_matches, forecast_identity_matches, metric_band_matches, ordered_matches,
    text_matches, training_envelope_matches, training_provenance_matches,
};
use crate::maintenance_canonical_json::{field, text};

/// project one fully verified wind canary graph into its generated serving union
pub(crate) fn project_verified_wind_bundle(
    document: &JsonValue,
) -> Option<ForecastAdjustmentWindCanaryRuntimeBundle> {
    // require the bundle literals before constructing their generated representations
    text_equals(
        document,
        "artifactKind",
        c::ForecastAdjustmentWindCanaryRuntimeBundleV1ArtifactKindLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(
        document,
        "siteKey",
        c::ForecastAdjustmentWindCanaryRuntimeBundleV1SiteKeyLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(
        document,
        "timezone",
        c::ForecastAdjustmentWindCanaryRuntimeBundleV1TimezoneLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    let candidate = project_candidate(field(document, "candidate")?)?;
    let transfer_report = project_transfer_report(field(document, "transferReport")?)?;
    let authorization = field(document, "authorization")?;
    let bundle_sha256 = text(document, "bundleSha256")?;

    // preserve the reviewed finite and permanent authorization generations
    match text(document, "contractVersion")?.as_str() {
        c::ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral::WIRE_VALUE => {
            Some(ForecastAdjustmentWindCanaryRuntimeBundle::TimeLimited(
                c::ForecastAdjustmentWindCanaryRuntimeBundleV1 {
                    artifactKind: c::ForecastAdjustmentWindCanaryRuntimeBundleV1ArtifactKindLiteral,
                    authorization: project_finite_authorization(authorization)?,
                    bundleSha256: bundle_sha256,
                    candidate,
                    contractVersion:
                        c::ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral,
                    siteKey: c::ForecastAdjustmentWindCanaryRuntimeBundleV1SiteKeyLiteral,
                    timezone: c::ForecastAdjustmentWindCanaryRuntimeBundleV1TimezoneLiteral,
                    transferReport: transfer_report,
                },
            ))
        }
        c::ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral::WIRE_VALUE => {
            Some(ForecastAdjustmentWindCanaryRuntimeBundle::Permanent(
                c::ForecastAdjustmentWindCanaryRuntimeBundleV2 {
                    artifactKind: c::ForecastAdjustmentWindCanaryRuntimeBundleV1ArtifactKindLiteral,
                    bundleSha256: bundle_sha256,
                    candidate,
                    siteKey: c::ForecastAdjustmentWindCanaryRuntimeBundleV1SiteKeyLiteral,
                    timezone: c::ForecastAdjustmentWindCanaryRuntimeBundleV1TimezoneLiteral,
                    transferReport: transfer_report,
                    authorization: project_permanent_authorization(authorization)?,
                    contractVersion:
                        c::ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral,
                },
            ))
        }
        _ => None,
    }
}

/// project the exact fitted wind candidate retained by serving
fn project_candidate(node: &JsonValue) -> Option<c::ForecastAdjustmentWindCanaryCandidateV1> {
    // require every candidate literal before retaining its fitted values
    for (key, expected) in [
        (
            "algorithmContractVersion",
            c::ForecastAdjustmentWindCanaryCandidateV1AlgorithmContractVersionLiteral::WIRE_VALUE,
        ),
        (
            "artifactKind",
            c::ForecastAdjustmentWindCanaryCandidateV1ArtifactKindLiteral::WIRE_VALUE,
        ),
        (
            "contractVersion",
            c::ForecastAdjustmentWindCanaryCandidateV1ContractVersionLiteral::WIRE_VALUE,
        ),
        (
            "siteKey",
            c::ForecastAdjustmentWindCanaryCandidateV1SiteKeyLiteral::WIRE_VALUE,
        ),
        (
            "timezone",
            c::ForecastAdjustmentWindCanaryCandidateV1TimezoneLiteral::WIRE_VALUE,
        ),
    ] {
        text_equals(node, key, expected).then_some(())?;
    }
    let fingerprint = field(node, "runtimeFingerprint")?;
    Some(c::ForecastAdjustmentWindCanaryCandidateV1 {
        algorithmContractVersion:
            c::ForecastAdjustmentWindCanaryCandidateV1AlgorithmContractVersionLiteral,
        artifactKind: c::ForecastAdjustmentWindCanaryCandidateV1ArtifactKindLiteral,
        candidateArtifactSha256: text(node, "candidateArtifactSha256")?,
        coefficientPayloadSha256: text(node, "coefficientPayloadSha256")?,
        coefficients: array(node, "coefficients", project_coefficient)?,
        contractVersion: c::ForecastAdjustmentWindCanaryCandidateV1ContractVersionLiteral,
        enabledMetricBands: array(node, "enabledMetricBands", project_metric_band)?,
        exportManifestSha256: text(node, "exportManifestSha256")?,
        finalTrainingCutoff: text(node, "finalTrainingCutoff")?,
        runtimeFingerprint: c::ForecastAdjustmentWindCanaryCandidateV1RuntimeFingerprint {
            icuVersion: text(fingerprint, "icuVersion")?,
            tzdataVersion: text(fingerprint, "tzdataVersion")?,
        },
        servedForecastIdentity: project_forecast_identity(field(node, "servedForecastIdentity")?)?,
        siteKey: c::ForecastAdjustmentWindCanaryCandidateV1SiteKeyLiteral,
        timezone: c::ForecastAdjustmentWindCanaryCandidateV1TimezoneLiteral,
        trainingEnvelopes: array(node, "trainingEnvelopes", project_envelope)?,
        trainingForecastIdentity: project_training_identity(field(
            node,
            "trainingForecastIdentity",
        )?)?,
        trainingProvenance: project_training_provenance(field(node, "trainingProvenance")?)?,
    })
}

/// project the fixed-lead training identity without replacing its source scalars
fn project_training_identity(
    node: &JsonValue,
) -> Option<c::ForecastAdjustmentWindCanaryTrainingIdentity> {
    // require the four source literals represented by zero-sized generated fields
    for (key, expected) in [
        (
            "cohort",
            c::ForecastAdjustmentWindCanaryTrainingIdentityCohortLiteral::WIRE_VALUE,
        ),
        (
            "dataset",
            c::ForecastAdjustmentWindCanaryTrainingIdentityDatasetLiteral::WIRE_VALUE,
        ),
        (
            "referenceKind",
            c::ForecastAdjustmentWindCanaryTrainingIdentityReferenceKindLiteral::WIRE_VALUE,
        ),
        (
            "upstreamModel",
            c::ForecastAdjustmentWindCanaryTrainingIdentityUpstreamModelLiteral::WIRE_VALUE,
        ),
    ] {
        text_equals(node, key, expected).then_some(())?;
    }
    Some(c::ForecastAdjustmentWindCanaryTrainingIdentity {
        adapterVersion: text(node, "adapterVersion")?,
        cohort: c::ForecastAdjustmentWindCanaryTrainingIdentityCohortLiteral,
        contractEpoch: text(node, "contractEpoch")?,
        dataset: c::ForecastAdjustmentWindCanaryTrainingIdentityDatasetLiteral,
        referenceKind: c::ForecastAdjustmentWindCanaryTrainingIdentityReferenceKindLiteral,
        sourceConfigFingerprint: text(node, "sourceConfigFingerprint")?,
        sourceKey: text(node, "sourceKey")?,
        upstreamModel: c::ForecastAdjustmentWindCanaryTrainingIdentityUpstreamModelLiteral,
    })
}

/// project all positive bridge evidence linked to the fitted candidate
fn project_transfer_report(
    node: &JsonValue,
) -> Option<c::ForecastAdjustmentWindCanaryTransferReportV1> {
    text_equals(
        node,
        "artifactKind",
        c::ForecastAdjustmentWindCanaryTransferReportV1ArtifactKindLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(
        node,
        "contractVersion",
        c::ForecastAdjustmentWindCanaryTransferReportV1ContractVersionLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    (boolean_field(node, "passed")?
        == c::ForecastAdjustmentWindCanaryTransferReportV1PassedLiteral::WIRE_VALUE)
        .then_some(())?;
    Some(c::ForecastAdjustmentWindCanaryTransferReportV1 {
        artifactKind: c::ForecastAdjustmentWindCanaryTransferReportV1ArtifactKindLiteral,
        bridgeEndExclusive: text(node, "bridgeEndExclusive")?,
        bridgeEvaluations: array(node, "bridgeEvaluations", project_bridge_evaluation)?,
        bridgeStartInclusive: text(node, "bridgeStartInclusive")?,
        candidateArtifactSha256: text(node, "candidateArtifactSha256")?,
        contractVersion: c::ForecastAdjustmentWindCanaryTransferReportV1ContractVersionLiteral,
        enabledMetricBands: array(node, "enabledMetricBands", project_metric_band)?,
        passed: c::ForecastAdjustmentWindCanaryTransferReportV1PassedLiteral,
        servedForecastIdentity: project_forecast_identity(field(node, "servedForecastIdentity")?)?,
        trainingForecastIdentity: project_training_identity(field(
            node,
            "trainingForecastIdentity",
        )?)?,
        transferReportSha256: text(node, "transferReportSha256")?,
    })
}

/// project one retained bridge score at its original array position
fn project_bridge_evaluation(
    node: &JsonValue,
) -> Option<c::ForecastAdjustmentWindCanaryBridgeEvaluation> {
    let network = field(node, "network")?;
    Some(c::ForecastAdjustmentWindCanaryBridgeEvaluation {
        metricBand: project_metric_band(field(node, "metricBand")?)?,
        network: c::ForecastAdjustmentWindCanaryBridgeScore {
            adjustedLoss: number_field(network, "adjustedLoss")?,
            eventCount: unsigned_field(network, "eventCount")?,
            rawLoss: number_field(network, "rawLoss")?,
            skill: number_field(network, "skill")?,
        },
    })
}

/// hold the mutable fields shared by both authorization generations
struct CommonAuthorization {
    activated_at: String,
    authorization_reason: String,
    authorization_sha256: String,
    authorized_at: String,
    authorized_by: String,
    candidate_artifact_sha256: String,
    enabled_metric_bands: Vec<c::ForecastAdjustmentMetricBand>,
    transfer_report_sha256: String,
}

/// project mutable authorization fields shared by the finite and permanent shapes
fn project_common_authorization(node: &JsonValue) -> Option<CommonAuthorization> {
    Some(CommonAuthorization {
        activated_at: text(node, "activatedAt")?,
        authorization_reason: text(node, "authorizationReason")?,
        authorization_sha256: text(node, "authorizationSha256")?,
        authorized_at: text(node, "authorizedAt")?,
        authorized_by: text(node, "authorizedBy")?,
        candidate_artifact_sha256: text(node, "candidateArtifactSha256")?,
        enabled_metric_bands: array(node, "enabledMetricBands", project_metric_band)?,
        transfer_report_sha256: text(node, "transferReportSha256")?,
    })
}

/// project the finite operator authorization
fn project_finite_authorization(
    node: &JsonValue,
) -> Option<c::ForecastAdjustmentWindCanaryAuthorizationV1> {
    text_equals(
        node,
        "artifactKind",
        c::ForecastAdjustmentWindCanaryAuthorizationV1ArtifactKindLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(
        node,
        "contractVersion",
        c::ForecastAdjustmentWindCanaryAuthorizationV1ContractVersionLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    (boolean_field(node, "authorized")?
        == c::ForecastAdjustmentWindCanaryAuthorizationV1AuthorizedLiteral::WIRE_VALUE)
        .then_some(())?;
    let common = project_common_authorization(node)?;
    Some(c::ForecastAdjustmentWindCanaryAuthorizationV1 {
        activatedAt: common.activated_at,
        artifactKind: c::ForecastAdjustmentWindCanaryAuthorizationV1ArtifactKindLiteral,
        authorizationReason: common.authorization_reason,
        authorizationSha256: common.authorization_sha256,
        authorized: c::ForecastAdjustmentWindCanaryAuthorizationV1AuthorizedLiteral,
        authorizedAt: common.authorized_at,
        authorizedBy: common.authorized_by,
        candidateArtifactSha256: common.candidate_artifact_sha256,
        contractVersion: c::ForecastAdjustmentWindCanaryAuthorizationV1ContractVersionLiteral,
        enabledMetricBands: common.enabled_metric_bands,
        expiresAt: text(node, "expiresAt")?,
        transferReportSha256: common.transfer_report_sha256,
    })
}

/// project the permanent operator authorization
fn project_permanent_authorization(
    node: &JsonValue,
) -> Option<c::ForecastAdjustmentWindCanaryAuthorizationV2> {
    text_equals(
        node,
        "artifactKind",
        c::ForecastAdjustmentWindCanaryAuthorizationV2ArtifactKindLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    text_equals(
        node,
        "contractVersion",
        c::ForecastAdjustmentWindCanaryAuthorizationV2ContractVersionLiteral::WIRE_VALUE,
    )
    .then_some(())?;
    (boolean_field(node, "authorized")?
        == c::ForecastAdjustmentWindCanaryAuthorizationV2AuthorizedLiteral::WIRE_VALUE)
        .then_some(())?;
    (boolean_field(node, "permanent")?
        == c::ForecastAdjustmentWindCanaryAuthorizationV2PermanentLiteral::WIRE_VALUE)
        .then_some(())?;
    matches!(field(node, "expiresAt"), Some(JsonValue::Null)).then_some(())?;
    let common = project_common_authorization(node)?;
    Some(c::ForecastAdjustmentWindCanaryAuthorizationV2 {
        activatedAt: common.activated_at,
        artifactKind: c::ForecastAdjustmentWindCanaryAuthorizationV2ArtifactKindLiteral,
        authorizationReason: common.authorization_reason,
        authorizationSha256: common.authorization_sha256,
        authorized: c::ForecastAdjustmentWindCanaryAuthorizationV2AuthorizedLiteral,
        authorizedAt: common.authorized_at,
        authorizedBy: common.authorized_by,
        candidateArtifactSha256: common.candidate_artifact_sha256,
        contractVersion: c::ForecastAdjustmentWindCanaryAuthorizationV2ContractVersionLiteral,
        enabledMetricBands: common.enabled_metric_bands,
        expiresAt: c::ForecastAdjustmentWindCanaryAuthorizationV2ExpiresAtNull,
        permanent: c::ForecastAdjustmentWindCanaryAuthorizationV2PermanentLiteral,
        transferReportSha256: common.transfer_report_sha256,
    })
}

/// bind the selected wind canary generation to the verified source document
pub(crate) fn wind_runtime_projection_matches(
    bundle: &ForecastAdjustmentWindCanaryRuntimeBundle,
    document: &JsonValue,
) -> bool {
    // preserve each generation's distinct authorization shape
    match bundle {
        ForecastAdjustmentWindCanaryRuntimeBundle::TimeLimited(bundle) => {
            text_matches(
                document,
                "contractVersion",
                ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral::WIRE_VALUE,
            ) && text_matches(document, "bundleSha256", &bundle.bundleSha256)
                && candidate_matches(&bundle.candidate, field(document, "candidate"))
                && transfer_report_matches(
                    &bundle.transferReport,
                    field(document, "transferReport"),
                )
                && finite_authorization_matches(
                    &bundle.authorization,
                    field(document, "authorization"),
                )
        }
        ForecastAdjustmentWindCanaryRuntimeBundle::Permanent(bundle) => {
            text_matches(
                document,
                "contractVersion",
                ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral::WIRE_VALUE,
            ) && text_matches(document, "bundleSha256", &bundle.bundleSha256)
                && candidate_matches(&bundle.candidate, field(document, "candidate"))
                && transfer_report_matches(
                    &bundle.transferReport,
                    field(document, "transferReport"),
                )
                && permanent_authorization_matches(
                    &bundle.authorization,
                    field(document, "authorization"),
                )
        }
    }
}

/// bind all mutable candidate material used by wind canary serving
fn candidate_matches(
    candidate: &ForecastAdjustmentWindCanaryCandidateV1,
    node: Option<&JsonValue>,
) -> bool {
    let Some(node) = node else {
        return false;
    };
    // bind candidate addresses and training boundary
    for (key, expected) in [
        (
            "candidateArtifactSha256",
            candidate.candidateArtifactSha256.as_str(),
        ),
        (
            "coefficientPayloadSha256",
            candidate.coefficientPayloadSha256.as_str(),
        ),
        (
            "exportManifestSha256",
            candidate.exportManifestSha256.as_str(),
        ),
        (
            "finalTrainingCutoff",
            candidate.finalTrainingCutoff.as_str(),
        ),
    ] {
        if !text_matches(node, key, expected) {
            return false;
        }
    }
    // require both nested source identities
    let (Some(served_identity), Some(training_provenance)) = (
        field(node, "servedForecastIdentity"),
        field(node, "trainingProvenance"),
    ) else {
        return false;
    };
    ordered_matches(
        node,
        "coefficients",
        &candidate.coefficients,
        coefficient_matches,
    )
    .is_some()
        && ordered_matches(
            node,
            "enabledMetricBands",
            &candidate.enabledMetricBands,
            metric_band_matches,
        )
        .is_some()
        && fingerprint_matches(
            &candidate.runtimeFingerprint.icuVersion,
            &candidate.runtimeFingerprint.tzdataVersion,
            field(node, "runtimeFingerprint"),
        )
        && forecast_identity_matches(&candidate.servedForecastIdentity, served_identity).is_some()
        && ordered_matches(
            node,
            "trainingEnvelopes",
            &candidate.trainingEnvelopes,
            training_envelope_matches,
        )
        .is_some()
        && training_identity_matches(
            &candidate.trainingForecastIdentity,
            field(node, "trainingForecastIdentity"),
        )
        && training_provenance_matches(&candidate.trainingProvenance, training_provenance).is_some()
}

/// bind both mutable runtime fingerprint components
fn fingerprint_matches(icu_version: &str, tzdata_version: &str, node: Option<&JsonValue>) -> bool {
    let Some(node) = node else {
        return false;
    };
    text_matches(node, "icuVersion", icu_version)
        && text_matches(node, "tzdataVersion", tzdata_version)
}

/// bind every mutable member of the training forecast identity
fn training_identity_matches(
    identity: &ForecastAdjustmentWindCanaryTrainingIdentity,
    node: Option<&JsonValue>,
) -> bool {
    let Some(node) = node else {
        return false;
    };
    // bind all nonliteral lineage members
    for (key, expected) in [
        ("adapterVersion", identity.adapterVersion.as_str()),
        ("contractEpoch", identity.contractEpoch.as_str()),
        (
            "sourceConfigFingerprint",
            identity.sourceConfigFingerprint.as_str(),
        ),
        ("sourceKey", identity.sourceKey.as_str()),
    ] {
        if !text_matches(node, key, expected) {
            return false;
        }
    }
    true
}

/// bind the retained transfer report identity and candidate links
fn transfer_report_matches(
    report: &ForecastAdjustmentWindCanaryTransferReportV1,
    node: Option<&JsonValue>,
) -> bool {
    let Some(node) = node else {
        return false;
    };
    // require the report's source identity
    let Some(served_identity) = field(node, "servedForecastIdentity") else {
        return false;
    };
    // bind each mutable report address and retained bridge boundary
    for (key, expected) in [
        ("bridgeEndExclusive", report.bridgeEndExclusive.as_str()),
        ("bridgeStartInclusive", report.bridgeStartInclusive.as_str()),
        (
            "candidateArtifactSha256",
            report.candidateArtifactSha256.as_str(),
        ),
        ("transferReportSha256", report.transferReportSha256.as_str()),
    ] {
        if !text_matches(node, key, expected) {
            return false;
        }
    }
    ordered_matches(
        node,
        "bridgeEvaluations",
        &report.bridgeEvaluations,
        bridge_evaluation_matches,
    )
    .is_some()
        && ordered_matches(
            node,
            "enabledMetricBands",
            &report.enabledMetricBands,
            metric_band_matches,
        )
        .is_some()
        && forecast_identity_matches(&report.servedForecastIdentity, served_identity).is_some()
        && training_identity_matches(
            &report.trainingForecastIdentity,
            field(node, "trainingForecastIdentity"),
        )
}

/// bind one ordered bridge evaluation and every score scalar
fn bridge_evaluation_matches(
    evaluation: &c::ForecastAdjustmentWindCanaryBridgeEvaluation,
    node: &JsonValue,
) -> Option<()> {
    metric_band_matches(&evaluation.metricBand, field(node, "metricBand")?)?;
    let network = field(node, "network")?;
    // preserve exact binary64 scores and the safe unsigned event count
    for (key, expected) in [
        ("adjustedLoss", evaluation.network.adjustedLoss),
        ("rawLoss", evaluation.network.rawLoss),
        ("skill", evaluation.network.skill),
    ] {
        number_field(network, key)
            .is_some_and(|actual| actual.to_bits() == expected.to_bits())
            .then_some(())?;
    }
    (unsigned_field(network, "eventCount") == Some(evaluation.network.eventCount)).then_some(())
}

/// bind the complete mutable finite authorization projection
fn finite_authorization_matches(
    authorization: &ForecastAdjustmentWindCanaryAuthorizationV1,
    node: Option<&JsonValue>,
) -> bool {
    let Some(node) = node else {
        return false;
    };
    text_matches(
        node,
        "contractVersion",
        ForecastAdjustmentWindCanaryAuthorizationV1ContractVersionLiteral::WIRE_VALUE,
    ) && authorization_common_matches(
        &authorization.activatedAt,
        &authorization.authorizationReason,
        &authorization.authorizationSha256,
        &authorization.authorizedAt,
        &authorization.authorizedBy,
        &authorization.candidateArtifactSha256,
        &authorization.enabledMetricBands,
        &authorization.transferReportSha256,
        node,
    ) && text_matches(node, "expiresAt", &authorization.expiresAt)
}

/// bind the complete mutable permanent authorization projection
fn permanent_authorization_matches(
    authorization: &ForecastAdjustmentWindCanaryAuthorizationV2,
    node: Option<&JsonValue>,
) -> bool {
    let Some(node) = node else {
        return false;
    };
    text_matches(
        node,
        "contractVersion",
        ForecastAdjustmentWindCanaryAuthorizationV2ContractVersionLiteral::WIRE_VALUE,
    ) && authorization_common_matches(
        &authorization.activatedAt,
        &authorization.authorizationReason,
        &authorization.authorizationSha256,
        &authorization.authorizedAt,
        &authorization.authorizedBy,
        &authorization.candidateArtifactSha256,
        &authorization.enabledMetricBands,
        &authorization.transferReportSha256,
        node,
    ) && matches!(field(node, "expiresAt"), Some(JsonValue::Null))
}

/// bind shared authorization identities and ordered enabled bands
#[allow(clippy::too_many_arguments)]
fn authorization_common_matches(
    activated_at: &str,
    authorization_reason: &str,
    authorization_sha256: &str,
    authorized_at: &str,
    authorized_by: &str,
    candidate_artifact_sha256: &str,
    enabled_metric_bands: &[weather_contracts::ForecastAdjustmentMetricBand],
    transfer_report_sha256: &str,
    node: &JsonValue,
) -> bool {
    // bind every mutable authorization scalar
    for (key, expected) in [
        ("activatedAt", activated_at),
        ("authorizationReason", authorization_reason),
        ("authorizationSha256", authorization_sha256),
        ("authorizedAt", authorized_at),
        ("authorizedBy", authorized_by),
        ("candidateArtifactSha256", candidate_artifact_sha256),
        ("transferReportSha256", transfer_report_sha256),
    ] {
        if !text_matches(node, key, expected) {
            return false;
        }
    }
    ordered_matches(
        node,
        "enabledMetricBands",
        enabled_metric_bands,
        metric_band_matches,
    )
    .is_some()
}
