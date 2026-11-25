use std::path::PathBuf;
use std::process::Command;

use weather_adjustment::{
    AdmittedWindBundleRow, AdmittedWindRowCalendar, AdmittedWindRowFeatures,
    AdmittedWindRowSourceIdentity, evaluate_admitted_wind_bundle_row,
};
use weather_contracts::{
    CanonicalWeatherMetrics, ForecastAdjustmentCoefficient,
    ForecastAdjustmentCoefficientDaypartEnum as Daypart, ForecastAdjustmentCoefficientLevelEnum,
    ForecastAdjustmentCoefficientSeasonEnum as Season, ForecastAdjustmentDecision,
    ForecastAdjustmentForecastIdentity, ForecastAdjustmentForecastIdentityCohortLiteral,
    ForecastAdjustmentForecastIdentityReferenceKindLiteral, ForecastAdjustmentMetric,
    ForecastAdjustmentMetricBand, ForecastAdjustmentRawForecastProvenance,
    ForecastAdjustmentRawForecastProvenanceCohortLiteral,
    ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral, ForecastAdjustmentReasonCode,
    ForecastAdjustmentTrainingEnvelope, ForecastAdjustmentTrainingEnvelopeMetricEnum,
    ForecastAdjustmentTrainingProvenance, ForecastAdjustmentWindCanaryCandidateV1,
    ForecastAdjustmentWindCanaryCandidateV1AlgorithmContractVersionLiteral,
    ForecastAdjustmentWindCanaryCandidateV1ArtifactKindLiteral,
    ForecastAdjustmentWindCanaryCandidateV1ContractVersionLiteral,
    ForecastAdjustmentWindCanaryCandidateV1RuntimeFingerprint,
    ForecastAdjustmentWindCanaryCandidateV1SiteKeyLiteral,
    ForecastAdjustmentWindCanaryCandidateV1TimezoneLiteral, ForecastAdjustmentWindCanaryMetric,
    ForecastAdjustmentWindCanaryTrainingIdentity,
    ForecastAdjustmentWindCanaryTrainingIdentityCohortLiteral,
    ForecastAdjustmentWindCanaryTrainingIdentityDatasetLiteral,
    ForecastAdjustmentWindCanaryTrainingIdentityReferenceKindLiteral,
    ForecastAdjustmentWindCanaryTrainingIdentityUpstreamModelLiteral, ForecastLeadBandKey,
    Nullable,
};

struct Fixture {
    calendar: AdmittedWindRowCalendar,
    calendar_fingerprint_matches: bool,
    candidate: ForecastAdjustmentWindCanaryCandidateV1,
    metrics: CanonicalWeatherMetrics,
    provenance: ForecastAdjustmentRawForecastProvenance,
    source_identity: AdmittedWindRowSourceIdentity,
}

/// Compare hierarchy, correction, applicability, and reason mapping to compiled Node code.
#[test]
fn matches_compiled_wind_bundle_row_evaluation() {
    let cases = [
        "all-active",
        "lead-168",
        "season",
        "root",
        "unsupported-low",
        "unsupported-high",
        "identity-mismatch",
        "wrong-cohort",
        "all-null",
        "fingerprint",
        "direction-calm",
        "envelope",
        "coefficient-missing",
        "metric-disabled",
        "metric-out-of-bounds",
        "partial",
    ];
    // exercise both active values and every adapter-owned public fail mapping
    for name in cases {
        let fixture = fixture(name);
        let actual = evaluate_admitted_wind_bundle_row(
            AdmittedWindBundleRow {
                authorization_sha256: "authorization-sha",
                candidate: &fixture.candidate,
                transfer_report_sha256: "transfer-sha",
            },
            AdmittedWindRowFeatures {
                calendar: fixture.calendar,
                calendar_fingerprint_matches: fixture.calendar_fingerprint_matches,
                metrics: &fixture.metrics,
                raw_forecast_provenance: &fixture.provenance,
                source_identity: fixture.source_identity,
            },
        );
        assert_eq!(result_signature(actual), node_oracle(name), "{name}");
    }
}

/// Invoke the extracted compiled application loop for one named fixture.
fn node_oracle(name: &str) -> String {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/wind_row_evaluation_oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .arg(name)
        .output()
        .expect("Node oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Node oracle must emit UTF-8")
}

/// Build one typed candidate and admitted row matching the Node fixture.
fn fixture(name: &str) -> Fixture {
    let target_lead_hours = match name {
        "lead-168" => 168,
        "unsupported-low" => 0,
        "unsupported-high" => 169,
        _ => 24,
    };
    let lead_band = candidate_lead_band(target_lead_hours);
    let mut calendar = AdmittedWindRowCalendar {
        daypart: Daypart::Afternoon,
        month: 9,
        season: Season::Autumn,
    };
    let mut metrics = empty_metrics();
    metrics.windDirectionDegrees = Nullable::Value(350.0);
    metrics.windGustMps = Nullable::Value(10.0);
    metrics.windSpeedMps = Nullable::Value(5.0);
    let mut candidate = candidate(lead_band);
    let mut calendar_fingerprint_matches = true;
    let mut source_identity = AdmittedWindRowSourceIdentity::Match;

    // alter only the one decision dimension named by each fixture
    match name {
        "season" => {
            metrics.windDirectionDegrees = Nullable::Null;
            metrics.windGustMps = Nullable::Null;
            calendar.month = 8;
        }
        "root" => {
            metrics.windDirectionDegrees = Nullable::Null;
            metrics.windGustMps = Nullable::Null;
            calendar.daypart = Daypart::Evening;
        }
        "identity-mismatch" => {
            source_identity = AdmittedWindRowSourceIdentity::IdentityMismatch;
        }
        "wrong-cohort" => {
            source_identity = AdmittedWindRowSourceIdentity::WrongCohort;
        }
        "all-null" => {
            metrics.windDirectionDegrees = Nullable::Null;
            metrics.windGustMps = Nullable::Null;
            metrics.windSpeedMps = Nullable::Null;
        }
        "fingerprint" => {
            metrics.windDirectionDegrees = Nullable::Null;
            metrics.windGustMps = Nullable::Null;
            calendar_fingerprint_matches = false;
        }
        "direction-calm" => {
            metrics.windDirectionDegrees = Nullable::Value(10.0);
            metrics.windGustMps = Nullable::Null;
            metrics.windSpeedMps = Nullable::Value(0.5);
            candidate.enabledMetricBands = vec![ForecastAdjustmentMetricBand {
                leadBand: lead_band,
                metric: ForecastAdjustmentMetric::WindDirectionDegrees,
            }];
        }
        "envelope" => {
            metrics.windDirectionDegrees = Nullable::Null;
            metrics.windGustMps = Nullable::Value(31.0);
            metrics.windSpeedMps = Nullable::Null;
        }
        "coefficient-missing" => {
            metrics.windDirectionDegrees = Nullable::Null;
            metrics.windGustMps = Nullable::Null;
            candidate
                .coefficients
                .retain(|entry| entry.metric != ForecastAdjustmentMetric::WindSpeedMps);
        }
        "metric-disabled" => {
            metrics.windDirectionDegrees = Nullable::Null;
            metrics.windGustMps = Nullable::Null;
            candidate.enabledMetricBands.clear();
        }
        "metric-out-of-bounds" => {
            metrics.windDirectionDegrees = Nullable::Null;
            metrics.windGustMps = Nullable::Null;
            metrics.windSpeedMps = Nullable::Value(-1.0);
        }
        "partial" => {
            metrics.windDirectionDegrees = Nullable::Null;
            metrics.windGustMps = Nullable::Value(31.0);
        }
        "all-active" | "lead-168" | "unsupported-low" | "unsupported-high" => {}
        _ => panic!("unknown fixture {name}"),
    }

    Fixture {
        calendar,
        calendar_fingerprint_matches,
        candidate,
        metrics,
        provenance: provenance(target_lead_hours),
        source_identity,
    }
}

/// Return a usable candidate band for both supported and rejected lead fixtures.
fn candidate_lead_band(target_lead_hours: u64) -> ForecastLeadBandKey {
    // mirror the candidate setup while leaving rejection to the adapter
    match target_lead_hours {
        145..=168 => ForecastLeadBandKey::Value145168,
        _ => ForecastLeadBandKey::Value001024,
    }
}

/// Build the exact generated candidate shape around compact wind tables.
fn candidate(lead_band: ForecastLeadBandKey) -> ForecastAdjustmentWindCanaryCandidateV1 {
    ForecastAdjustmentWindCanaryCandidateV1 {
        algorithmContractVersion:
            ForecastAdjustmentWindCanaryCandidateV1AlgorithmContractVersionLiteral,
        artifactKind: ForecastAdjustmentWindCanaryCandidateV1ArtifactKindLiteral,
        candidateArtifactSha256: "candidate-sha".to_owned(),
        coefficientPayloadSha256: "coefficient-sha".to_owned(),
        coefficients: vec![
            coefficient(
                ForecastAdjustmentMetric::WindDirectionDegrees,
                lead_band,
                1,
                30.0,
                None,
            ),
            coefficient(
                ForecastAdjustmentMetric::WindGustMps,
                lead_band,
                1,
                2.0,
                None,
            ),
            coefficient(
                ForecastAdjustmentMetric::WindSpeedMps,
                lead_band,
                1,
                1.0,
                None,
            ),
            coefficient(
                ForecastAdjustmentMetric::WindSpeedMps,
                lead_band,
                2,
                2.0,
                Some((Daypart::Afternoon, None, Some(Season::Autumn))),
            ),
            coefficient(
                ForecastAdjustmentMetric::WindSpeedMps,
                lead_band,
                3,
                3.0,
                Some((Daypart::Afternoon, Some(9), None)),
            ),
        ],
        contractVersion: ForecastAdjustmentWindCanaryCandidateV1ContractVersionLiteral,
        enabledMetricBands: vec![
            ForecastAdjustmentMetricBand {
                leadBand: lead_band,
                metric: ForecastAdjustmentMetric::WindDirectionDegrees,
            },
            ForecastAdjustmentMetricBand {
                leadBand: lead_band,
                metric: ForecastAdjustmentMetric::WindGustMps,
            },
            ForecastAdjustmentMetricBand {
                leadBand: lead_band,
                metric: ForecastAdjustmentMetric::WindSpeedMps,
            },
        ],
        exportManifestSha256: "export-sha".to_owned(),
        finalTrainingCutoff: "2026-01-01T00:00:00.000Z".to_owned(),
        runtimeFingerprint: ForecastAdjustmentWindCanaryCandidateV1RuntimeFingerprint {
            icuVersion: "icu".to_owned(),
            tzdataVersion: "tzdata".to_owned(),
        },
        servedForecastIdentity: served_identity(),
        siteKey: ForecastAdjustmentWindCanaryCandidateV1SiteKeyLiteral,
        timezone: ForecastAdjustmentWindCanaryCandidateV1TimezoneLiteral,
        trainingEnvelopes: vec![
            ForecastAdjustmentTrainingEnvelope {
                leadBand: lead_band,
                maximum: 30.0,
                metric: ForecastAdjustmentTrainingEnvelopeMetricEnum::WindGustMps,
                minimum: 0.0,
            },
            ForecastAdjustmentTrainingEnvelope {
                leadBand: lead_band,
                maximum: 30.0,
                metric: ForecastAdjustmentTrainingEnvelopeMetricEnum::WindSpeedMps,
                minimum: 0.0,
            },
        ],
        trainingForecastIdentity: ForecastAdjustmentWindCanaryTrainingIdentity {
            adapterVersion: "adapter".to_owned(),
            cohort: ForecastAdjustmentWindCanaryTrainingIdentityCohortLiteral,
            contractEpoch: "epoch".to_owned(),
            dataset: ForecastAdjustmentWindCanaryTrainingIdentityDatasetLiteral,
            referenceKind: ForecastAdjustmentWindCanaryTrainingIdentityReferenceKindLiteral,
            sourceConfigFingerprint: "fingerprint".to_owned(),
            sourceKey: "source".to_owned(),
            upstreamModel: ForecastAdjustmentWindCanaryTrainingIdentityUpstreamModelLiteral,
        },
        trainingProvenance: ForecastAdjustmentTrainingProvenance {
            aggregationContractSha256: "aggregation".to_owned(),
            coordinateManifestSha256: "coordinates".to_owned(),
            metricEligibilitySha256: "eligibility".to_owned(),
            observationSourceLineageSha256: "lineage".to_owned(),
            observationStationManifestSha256: "stations".to_owned(),
            spatialWeightSha256: "weight".to_owned(),
        },
    }
}

/// Build one coefficient using generated nullable fields.
fn coefficient(
    metric: ForecastAdjustmentMetric,
    lead_band: ForecastLeadBandKey,
    level: u8,
    value: f64,
    calendar: Option<(Daypart, Option<u64>, Option<Season>)>,
) -> ForecastAdjustmentCoefficient {
    let (daypart, month, season) = match calendar {
        Some((daypart, month, season)) => (
            Nullable::Value(daypart),
            month.map_or(Nullable::Null, Nullable::Value),
            season.map_or(Nullable::Null, Nullable::Value),
        ),
        None => (Nullable::Null, Nullable::Null, Nullable::Null),
    };
    ForecastAdjustmentCoefficient {
        coefficient: value,
        daypart,
        effectiveEventCount: 100.0,
        leadBand: lead_band,
        level: match level {
            1 => ForecastAdjustmentCoefficientLevelEnum::Value1,
            2 => ForecastAdjustmentCoefficientLevelEnum::Value2,
            3 => ForecastAdjustmentCoefficientLevelEnum::Value3,
            _ => panic!("invalid fixture level {level}"),
        },
        metric,
        month,
        season,
    }
}

/// Build the immutable served identity fixture used before this adapter.
fn served_identity() -> ForecastAdjustmentForecastIdentity {
    ForecastAdjustmentForecastIdentity {
        adapterVersion: "adapter".to_owned(),
        cohort: ForecastAdjustmentForecastIdentityCohortLiteral,
        contractEpoch: "epoch".to_owned(),
        dataset: "dataset".to_owned(),
        referenceKind: ForecastAdjustmentForecastIdentityReferenceKindLiteral,
        sourceConfigFingerprint: "fingerprint".to_owned(),
        sourceKey: "source".to_owned(),
        upstreamModel: "model".to_owned(),
    }
}

/// Build one generated raw provenance row around the tested target lead.
fn provenance(target_lead_hours: u64) -> ForecastAdjustmentRawForecastProvenance {
    ForecastAdjustmentRawForecastProvenance {
        adapterVersion: "adapter".to_owned(),
        cohort: ForecastAdjustmentRawForecastProvenanceCohortLiteral,
        contractEpoch: "epoch".to_owned(),
        dataset: "dataset".to_owned(),
        referenceAt: "2026-01-01T00:00:00.000Z".to_owned(),
        referenceKind: ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral,
        sourceConfigFingerprint: "fingerprint".to_owned(),
        sourceKey: "source".to_owned(),
        targetLeadHours: target_lead_hours,
        upstreamModel: "model".to_owned(),
        validAt: "2026-01-02T00:00:00.000Z".to_owned(),
    }
}

/// Build generated canonical metrics with every provider field absent.
fn empty_metrics() -> CanonicalWeatherMetrics {
    CanonicalWeatherMetrics {
        apparentTemperatureC: Nullable::Null,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Null,
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Null,
        relativeHumidityPercent: Nullable::Null,
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Null,
        uvIndex: Nullable::Null,
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Null,
        windGustMps: Nullable::Null,
        windSpeedMps: Nullable::Null,
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// Render the generated decision in the Node oracle's compact form.
fn result_signature(result: ForecastAdjustmentDecision) -> String {
    // distinguish the only two decision forms this adapter can produce
    match result {
        ForecastAdjustmentDecision::WindCanaryActive(active) => {
            let metrics = active
                .adjustedMetrics
                .0
                .iter()
                .map(|(metric, value)| {
                    format!("{}={:016x}", wind_metric_name(*metric), value.to_bits())
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "active:{}:{}:{}:{}:{}",
                lead_band_name(active.leadBand),
                metrics,
                active.authorizationSha256,
                active.candidateArtifactSha256,
                active.transferReportSha256,
            )
        }
        ForecastAdjustmentDecision::FailRaw(fail) => {
            format!("fail:{}", reason_name(fail.reasonCode))
        }
        ForecastAdjustmentDecision::Active(_) => panic!("unexpected legacy active decision"),
    }
}

/// Return one generated wind metric's canonical wire name.
fn wind_metric_name(metric: ForecastAdjustmentWindCanaryMetric) -> &'static str {
    // map the generated closed allowlist
    match metric {
        ForecastAdjustmentWindCanaryMetric::WindDirectionDegrees => "windDirectionDegrees",
        ForecastAdjustmentWindCanaryMetric::WindGustMps => "windGustMps",
        ForecastAdjustmentWindCanaryMetric::WindSpeedMps => "windSpeedMps",
    }
}

/// Return one generated lead band's canonical wire name.
fn lead_band_name(lead_band: ForecastLeadBandKey) -> &'static str {
    // map every frozen inclusive lead range
    match lead_band {
        ForecastLeadBandKey::Value001024 => "001-024",
        ForecastLeadBandKey::Value025048 => "025-048",
        ForecastLeadBandKey::Value049072 => "049-072",
        ForecastLeadBandKey::Value073096 => "073-096",
        ForecastLeadBandKey::Value097120 => "097-120",
        ForecastLeadBandKey::Value121144 => "121-144",
        ForecastLeadBandKey::Value145168 => "145-168",
    }
}

/// Return one generated reason's canonical wire name.
fn reason_name(reason: ForecastAdjustmentReasonCode) -> &'static str {
    // render the complete generated contract so unexpected results remain visible
    match reason {
        ForecastAdjustmentReasonCode::AdjustmentError => "adjustment_error",
        ForecastAdjustmentReasonCode::BundleInvalid => "bundle_invalid",
        ForecastAdjustmentReasonCode::BundleMissing => "bundle_missing",
        ForecastAdjustmentReasonCode::CanaryExpired => "canary_expired",
        ForecastAdjustmentReasonCode::CanaryKilled => "canary_killed",
        ForecastAdjustmentReasonCode::CoefficientMissing => "coefficient_missing",
        ForecastAdjustmentReasonCode::CrossLinkMismatch => "cross_link_mismatch",
        ForecastAdjustmentReasonCode::DirectionCalm => "direction_calm",
        ForecastAdjustmentReasonCode::EvidenceRedundancyMissing => "evidence_redundancy_missing",
        ForecastAdjustmentReasonCode::HashMismatch => "hash_mismatch",
        ForecastAdjustmentReasonCode::IdentityMismatch => "identity_mismatch",
        ForecastAdjustmentReasonCode::InsufficientData => "insufficient_data",
        ForecastAdjustmentReasonCode::MetricNotEnabled => "metric_not_enabled",
        ForecastAdjustmentReasonCode::MetricOutOfBounds => "metric_out_of_bounds",
        ForecastAdjustmentReasonCode::PolicyRaw => "policy_raw",
        ForecastAdjustmentReasonCode::QualificationFailed => "qualification_failed",
        ForecastAdjustmentReasonCode::RegistryInactive => "registry_inactive",
        ForecastAdjustmentReasonCode::RegistryInvalid => "registry_invalid",
        ForecastAdjustmentReasonCode::RuntimeFingerprintMismatch => "runtime_fingerprint_mismatch",
        ForecastAdjustmentReasonCode::TrainingEnvelopeMismatch => "training_envelope_mismatch",
        ForecastAdjustmentReasonCode::UnsupportedLead => "unsupported_lead",
        ForecastAdjustmentReasonCode::WrongCohort => "wrong_cohort",
    }
}
