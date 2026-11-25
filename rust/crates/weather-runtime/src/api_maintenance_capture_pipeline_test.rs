// Exercise a retained capture through the real Rust source path and value-free disposition.

use std::cell::RefCell;

use super::*;
use crate::{
    ApiMaintenanceCaptureExecutor, ApiMaintenanceCaptureJob, ApiMaintenancePredictionPorts,
    ApiMaintenanceReceiptCanonicalValues, InstalledApiWindCapture, MaintenanceReceiptScalar,
    PreparedApiMaintenanceCaptureOperations,
};
use weather_api::{
    AdjustmentMaintenanceGapCallback, AdmittedAdjustmentMaintenanceGap,
    AdmittedAdjustmentMaintenancePublish, AdmittedAdjustmentMaintenanceStage,
    AdmittedAdjustmentRevisionCaptureWitness, AdmittedWindRowCalendar, CurrentProjectionError,
    MaintenanceComparatorCanonicalCodec, MaintenanceComparatorDocument, MaintenancePredictionBody,
    MaintenancePredictionCanonicalCodec, MaintenanceSourceCanonicalCodec, MaintenanceSourceField,
    MaintenanceSourceProjection, MaintenanceWindModelCalendar,
    PreparedAdjustmentMaintenancePublish, ShadowPublishAdmissionError,
};
use weather_contracts::*;
use weather_database::{
    AdmittedApiMaintenanceRegistration, ApiMaintenanceRegistrationVersion, Sha256Digest,
};

const CANDIDATE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SOURCE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

struct PipelineTime;

impl CurrentTimeResolver for PipelineTime {
    /// Resolve the exact retained issue and due clock before source geometry.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        assert_eq!(
            value,
            &JsDateOrString::String(ISSUE_UTC.encode_utf16().collect())
        );
        Ok(CanonicalInstant {
            epoch_milliseconds: ISSUE_MS,
            utc: ISSUE_UTC.to_owned(),
        })
    }
}

impl MaintenanceWindModelCalendar for PipelineTime {
    /// The incomplete retained source never reaches model inference.
    fn calendar(&self, _: &str) -> Result<AdmittedWindRowCalendar, CurrentProjectionError> {
        unreachable!()
    }

    /// No fingerprint decision can precede source admission.
    fn fingerprint_matches(&self, _: &str, _: &str) -> bool {
        unreachable!()
    }
}

struct NoCodec;

impl MaintenanceSourceCanonicalCodec for NoCodec {
    /// Reject fixture execution if incomplete input tries to encode a source.
    fn encode_source(&mut self, _: &MaintenanceSourceProjection<'_>) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// Reject fixture execution if incomplete input tries to encode a row.
    fn encode_row(&mut self, _: &[MaintenanceSourceField]) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// Reject fixture execution if incomplete input reaches source identity work.
    fn sha256(&mut self, _: &[&[u8]]) -> Option<Sha256Digest> {
        unreachable!()
    }
}

impl MaintenancePredictionCanonicalCodec for NoCodec {
    /// No prediction may be encoded before the fixture has an admitted source.
    fn encode_prediction(&mut self, _: &MaintenancePredictionBody) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// No prediction may be hashed before the fixture has an admitted source.
    fn sha256(&mut self, _: &[&[u8]]) -> Option<Sha256Digest> {
        unreachable!()
    }
}

impl MaintenanceComparatorCanonicalCodec for NoCodec {
    /// No comparator may be hashed on the invalid-source branch.
    fn sha256(&mut self, _: &[u8]) -> Option<Sha256Digest> {
        unreachable!()
    }

    /// No incumbent authority may be consulted on the invalid-source branch.
    fn validate_authority(&mut self, _: &MaintenanceShadowServingAuthority, _: &str) -> bool {
        unreachable!()
    }

    /// No comparator may be encoded on the invalid-source branch.
    fn encode_comparator(&mut self, _: &MaintenanceComparatorDocument<'_>) -> Option<Vec<u8>> {
        unreachable!()
    }
}

impl ApiMaintenanceReceiptCanonicalValues for NoCodec {
    /// No receipt clock exists before staging or append.
    fn canonical_utc_epoch_milliseconds(&self, _: &str) -> Option<i64> {
        unreachable!()
    }

    /// No archive bytes are sent on the invalid-source branch.
    fn sha256(&self, _: &[u8]) -> Option<Sha256Digest> {
        unreachable!()
    }

    /// No archive receipt exists on the invalid-source branch.
    fn unsigned_receipt_sha256(
        &self,
        _: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        unreachable!()
    }
}

struct FixtureGap<'a> {
    events: &'a RefCell<Vec<&'static str>>,
    reason: MaintenanceShadowGapReason,
}

impl AdjustmentMaintenanceGapCallback for FixtureGap<'_> {
    /// Observe the value-free handoff under the exact retained candidate registration.
    fn record_gap(&mut self, gap: AdmittedAdjustmentMaintenanceGap) -> Result<(), OperationError> {
        self.events.borrow_mut().push("capture-gap");
        assert_eq!(gap.request.dueKey, format!("capture/{ISSUE_UTC}"));
        assert_eq!(
            gap.request.family,
            AdjustmentMaintenanceGapInputFamilyEnum::Wind
        );
        assert_eq!(gap.request.reason, self.reason);
        assert_eq!(gap.request.registrationSha256, REGISTRATION);
        Err(OperationError::Rejected)
    }
}

impl ApiMaintenancePredictionPorts for FixtureGap<'_> {
    /// An incomplete source never stages source, comparator, or candidate material.
    fn stage(
        &mut self,
        _: &AdmittedAdjustmentMaintenanceStage,
    ) -> Result<AdjustmentMaintenanceStageReceipt, OperationError> {
        unreachable!()
    }

    /// An incomplete source never appends a compact database row.
    fn register_and_append(
        &mut self,
        _: &AdmittedApiMaintenanceRegistration<'_>,
        _: &AdjustmentShadowPrediction,
    ) -> Result<AdjustmentShadowPredictionReceipt, OperationError> {
        unreachable!()
    }

    /// An incomplete source never reads back a database receipt.
    fn admit(
        &mut self,
        _: PreparedAdjustmentMaintenancePublish,
    ) -> Result<AdmittedAdjustmentMaintenancePublish, ShadowPublishAdmissionError> {
        unreachable!()
    }

    /// An incomplete source never commits a publication.
    fn publish(
        &mut self,
        _: &AdmittedAdjustmentMaintenancePublish,
    ) -> Result<AdjustmentMaintenancePublishReceipt, OperationError> {
        unreachable!()
    }

    /// Attempt the ordinary gap using the same relay as the callback.
    fn record_gap(&mut self, gap: AdmittedAdjustmentMaintenanceGap) -> Result<(), OperationError> {
        AdjustmentMaintenanceGapCallback::record_gap(self, gap)
    }
}

struct FixtureExecutor<'a>(&'a RefCell<Vec<&'static str>>, MaintenanceShadowGapReason);

impl<'a> ApiMaintenanceCaptureExecutor<'a> for FixtureExecutor<'_> {
    type Pending = Result<CaptureApiMaintenanceShadowResult, OperationError>;

    /// Execute the concrete scheduled job before returning its result handle.
    fn start(&mut self, job: ApiMaintenanceCaptureJob<'a>) -> Self::Pending {
        self.0.borrow_mut().push("capture-start");
        let mut ports = FixtureGap {
            events: self.0,
            reason: self.1,
        };
        job.run(&mut NoCodec, &mut ports, &NoCodec)
    }

    /// Confirm an observed categorical gap does not turn into a failed capture request.
    fn finish(
        &mut self,
        temperature: Option<Self::Pending>,
        wind: Option<Self::Pending>,
    ) -> Result<(), OperationError> {
        self.0.borrow_mut().push("capture-finish");
        assert!(temperature.is_none());
        assert!(
            matches!(wind.expect("started wind")?, CaptureApiMaintenanceShadowResult::Gap(gap) if gap.reason == self.1)
        );
        Ok(())
    }
}

/// Bind the fixture registration to a concrete retained package before rendering or inference.
fn wind_package(candidate_sha: &str) -> ForecastAdjustmentWindMaintenanceRuntimePackage {
    let source = ForecastAdjustmentForecastIdentity {
        adapterVersion: "open-meteo-forecast-daily/v4".to_owned(),
        cohort: ForecastAdjustmentForecastIdentityCohortLiteral,
        contractEpoch: String::new(),
        dataset: "forecast".to_owned(),
        referenceKind: ForecastAdjustmentForecastIdentityReferenceKindLiteral,
        sourceConfigFingerprint: SOURCE.to_owned(),
        sourceKey: "open-meteo-forecast-v4".to_owned(),
        upstreamModel: "best_match".to_owned(),
    };
    ForecastAdjustmentWindMaintenanceRuntimePackage {
        bundleSha256: REGISTRATION.to_owned(),
        candidateSha256: candidate_sha.to_owned(),
        source: source.clone(),
        candidate: ForecastAdjustmentCandidateV2 {
            algorithmContractVersion: ForecastAdjustmentCandidateV2AlgorithmContractVersionLiteral,
            candidateArtifactSha256: CANDIDATE.to_owned(),
            coefficientPayloadSha256: SOURCE.to_owned(),
            coefficients: vec![],
            contractVersion: ForecastAdjustmentCandidateV2ContractVersionLiteral,
            developmentReportSha256: SOURCE.to_owned(),
            enabledMetricBands: vec![],
            evaluationEpochId: "fixture".to_owned(),
            exportManifestSha256: SOURCE.to_owned(),
            finalTrainingCutoff: ISSUE_UTC.to_owned(),
            forecastIdentity: source,
            metricPolicies: vec![],
            runtimeFingerprint: ForecastAdjustmentCandidateV2RuntimeFingerprint {
                icuVersion: String::new(),
                tzdataVersion: String::new(),
            },
            siteKey: ForecastAdjustmentCandidateV2SiteKeyLiteral,
            timezone: ForecastAdjustmentCandidateV2TimezoneLiteral,
            trainingEnvelopes: vec![],
            trainingProvenance: ForecastAdjustmentTrainingProvenance {
                aggregationContractSha256: SOURCE.to_owned(),
                coordinateManifestSha256: SOURCE.to_owned(),
                metricEligibilitySha256: SOURCE.to_owned(),
                observationSourceLineageSha256: SOURCE.to_owned(),
                observationStationManifestSha256: SOURCE.to_owned(),
                spatialWeightSha256: SOURCE.to_owned(),
            },
        },
        contractVersion: ForecastAdjustmentWindMaintenanceRuntimePackageContractVersionLiteral,
        dueMonth: "2026-09".to_owned(),
        family: ForecastAdjustmentWindMaintenanceRuntimePackageFamilyLiteral,
    }
}

/// Carry the previously admitted startup epoch with the original generated witness.
fn startup_witness() -> AdmittedAdjustmentRevisionCaptureWitness {
    AdmittedAdjustmentRevisionCaptureWitness::from_verified(
        AdjustmentRevisionCaptureEpochWitness {
            activationKind: AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral,
            archiveCommitOrdinal: AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral,
            catalogFrontierSha256: SOURCE.to_owned(),
            contractVersion: AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral,
            controlPlaneSha256: SOURCE.to_owned(),
            controlPlaneVersion:
                AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value14,
            databaseMigrationHistorySha256: SOURCE.to_owned(),
            epochAt: ISSUE_UTC.to_owned(),
            servingSnapshotSha256: SOURCE.to_owned(),
            sourceCommit: String::new(),
            sourceRelease: String::new(),
            sourceServerImageDigest: String::new(),
            sourceWebImageDigest: String::new(),
            witnessSha256: SOURCE.to_owned(),
        },
        admitted(AdjustmentMaintenanceCaptureRequest {
            dueKey: format!("capture/{ISSUE_UTC}"),
            issuedAt: ISSUE_UTC.to_owned(),
        })
        .issued_at,
    )
    .expect("matching fixture startup epoch")
}

/// Retained forecast reads enter the real Rust job, and a failed gap archive does not fail capture.
#[test]
fn retained_callback_runs_the_concrete_capture_job() {
    // the original installed-package check precedes the incomplete-source classification
    for (package_sha, has_capture_receipt_v1, reason) in [
        (
            CANDIDATE,
            true,
            MaintenanceShadowGapReason::SourceIncomplete,
        ),
        (
            SOURCE,
            true,
            MaintenanceShadowGapReason::CandidateUnavailable,
        ),
        (
            CANDIDATE,
            false,
            MaintenanceShadowGapReason::CandidateUnavailable,
        ),
    ] {
        let witness = startup_witness();
        let package = wind_package(package_sha);
        let incumbent = LoadedApiForecastAdjustmentRuntime::Standard(
            LoadedForecastAdjustmentRuntimeV1::Disabled(
                LoadedForecastAdjustmentRuntimeV1Disabled {
                    bundle: LoadedForecastAdjustmentRuntimeV1DisabledBundleNull,
                    reasonCode:
                        LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
                    state: LoadedForecastAdjustmentRuntimeV1DisabledStateLiteral,
                },
            ),
        );
        let events = RefCell::new(Vec::new());
        let operations = PreparedApiMaintenanceCaptureOperations {
            temperature: None,
            wind: Some(InstalledApiWindCapture {
                registration: AdmittedApiMaintenanceRegistration {
                    family: AdjustmentShadowFamily::Wind,
                    version: ApiMaintenanceRegistrationVersion::V3,
                    registration_sha256: REGISTRATION,
                    candidate_sha256: CANDIDATE,
                    source_sha256: SOURCE,
                    canonical_json: "{}",
                },
                has_capture_receipt_v1,
                package: &package,
                incumbent: &incumbent,
                calendar: &PipelineTime,
            }),
            witness: &witness,
            time: &PipelineTime,
            executor: FixtureExecutor(&events, reason),
            gap: FixtureGap {
                events: &events,
                reason,
            },
        };
        let mut capture = RetainedApiMaintenanceCapture::new(
            FixtureReads {
                events: &events,
                sidecar_present: false,
            },
            FixtureTime,
            operations,
        );
        let result = capture.capture(admitted(AdjustmentMaintenanceCaptureRequest {
            dueKey: format!("capture/{ISSUE_UTC}"),
            issuedAt: ISSUE_UTC.to_owned(),
        }));
        assert_eq!(result, Ok(()));
        assert_eq!(
            *events.borrow(),
            [
                "wind-read",
                "capture-start",
                "capture-gap",
                "capture-finish"
            ]
        );
    }
}
