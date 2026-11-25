use weather_contracts::{JsonValue, Nullable, RainAdjustmentCaptureKindEnum, RainAdjustmentHour};
use weather_worker::{
    AdmittedRainEdgeEvidence, AdmittedRainHurdleWindCalendar,
    ForecastAdjustmentRainDiagnosticCalendar, ForecastAdjustmentScorecardCalendar,
    RainAdjustmentFeatureAdmission, RainAdjustmentTimeAdapter, RainPerformanceCalendarLabels,
    RainPerformanceDaypart, RainPerformanceReportPackageInputs, RainPerformanceSeason,
    RainPerformanceTarget, TempestObservationEnvelope,
};

use super::*;
use crate::{
    RainArchiveTargetPreimage, WorkerRainForecastHourly, WorkerRainForecastPayload,
    WorkerRainGaugeReplayValues,
};

const BODY_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MODEL_HASH: &str = weather_contracts::retained_rain_hurdle_wind::RETAINED_RAIN_MODEL_SHA256;

struct FixtureOwners;

impl WorkerRainGaugeReplayValues for FixtureOwners {
    /// Keep unreachable station decoding closed in this forecast-only fixture.
    fn decode_tempest(&mut self, _: &[u8]) -> Result<TempestObservationEnvelope, TaskFailure> {
        Err(failure("unexpected station decode"))
    }

    /// Parse the fixture's exact September UTC spelling.
    fn parse_utc_milliseconds(&mut self, value: &str) -> Result<i64, TaskFailure> {
        fixture_epoch(value).ok_or_else(|| failure("fixture clock differs"))
    }

    /// Format one fixture epoch as canonical UTC milliseconds.
    fn format_utc_milliseconds(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        Ok(fixture_utc(epoch_ms))
    }
}

impl WorkerRainAdjustmentCaptureValues for FixtureOwners {
    /// Decode the fixture body address into one complete 48-hour forecast.
    fn decode_forecast(&mut self, body: &[u8]) -> Result<WorkerRainForecastPayload, TaskFailure> {
        let (&day, rest) = body
            .split_first()
            .ok_or_else(|| failure("fixture forecast body differs"))?;
        let &hour = rest
            .first()
            .ok_or_else(|| failure("fixture forecast body differs"))?;
        let initialized_at_ms = (i64::from(day) * 24 + i64::from(hour)) * HOUR_MILLISECONDS;
        let time = (0..=48)
            .map(|lead| fixture_open_meteo(initialized_at_ms + lead * HOUR_MILLISECONDS))
            .collect::<Vec<_>>();
        let values = |value| (0..=48).map(|_| Nullable::Value(value)).collect();
        Ok(WorkerRainForecastPayload {
            utc_offset_seconds: 0,
            hourly: WorkerRainForecastHourly {
                time,
                precipitation: values(1.0),
                temperature_2m: values(1.0),
                relative_humidity_2m: values(70.0),
                cloud_cover: values(20.0),
                surface_pressure: values(1_000.0),
                wind_speed_10m: values(3.0),
                wind_direction_10m: values(180.0),
            },
        })
    }

    /// Format one fixture epoch in Open-Meteo hourly spelling.
    fn format_open_meteo_hour(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        Ok(fixture_open_meteo(epoch_ms))
    }
}

impl RainPerformanceEvidenceValues for FixtureOwners {
    /// Preserve the fixture's UTC date as its local date.
    fn local_date(&mut self, valid_at: &str) -> Option<String> {
        Some(valid_at.get(..10)?.to_owned())
    }

    /// Return one complete fixed-gauge target for the fixture hour.
    fn rain_target(&mut self, valid_at: &str) -> Option<RainPerformanceTarget> {
        Some(RainPerformanceTarget {
            target: Nullable::Value(0.5),
            target_key: format!("target:{valid_at}"),
        })
    }

    /// Preserve a canonical fixture receipt clock.
    fn receipt_clock(&mut self, value: &str) -> Option<String> {
        fixture_epoch(value).map(|_| value.to_owned())
    }

    /// Keep unreachable edge hashing closed in this no-edge fixture.
    fn edge_row_identity(&mut self, _: &AdmittedRainEdgeEvidence, _: &str) -> Option<String> {
        None
    }

    /// Keep unreachable Best Match hashing closed in this no-edge fixture.
    fn best_match_edge_row_identity(&mut self, _: &AdmittedRainEdgeEvidence) -> Option<String> {
        None
    }
}

impl RainArchiveTargetIdentityValues for FixtureOwners {
    /// Keep unreachable target hashing closed for an archive without station rows.
    fn rain_target_identity_sha256(&mut self, _: &RainArchiveTargetPreimage) -> Option<String> {
        None
    }
}

struct FixtureAdmission;

impl RainAdjustmentFeatureAdmission for FixtureAdmission {
    /// Supply deterministic finite UTC harmonics.
    fn calendar(&mut self, _: i64) -> Result<AdmittedRainHurdleWindCalendar, TaskFailure> {
        Ok(AdmittedRainHurdleWindCalendar {
            annual_cos: 0.2,
            annual_sin: 0.1,
            daily_cos: 0.4,
            daily_sin: 0.3,
        })
    }

    /// Retain positive station weights in catalog order.
    fn station_weights(&mut self) -> Result<[f64; 12], TaskFailure> {
        Ok([1.0; 12])
    }
}

struct FixtureReplayTime;

impl RainAdjustmentTimeAdapter for FixtureReplayTime {
    /// Format one admitted fixture epoch.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        Ok(fixture_utc(epoch_ms))
    }
}

impl ForecastAdjustmentScorecardCalendar for FixtureOwners {
    /// Parse one fixture instant for scorecard chronology.
    fn canonical_utc_epoch_milliseconds(&mut self, value: &str) -> Option<i64> {
        fixture_epoch(value)
    }

    /// Preserve the fixture's UTC date as its admitted local date.
    fn los_angeles_local_date(&mut self, value: &str, _: i64) -> Option<String> {
        Some(value.get(..10)?.to_owned())
    }
}

impl ForecastAdjustmentRainDiagnosticCalendar for FixtureOwners {
    /// Keep the September fixture outside winter.
    fn los_angeles_is_winter(&mut self, _: &str) -> Option<bool> {
        Some(false)
    }
}

impl RainPerformanceReportCalendar for FixtureOwners {
    /// Return the fixture's admitted September afternoon labels.
    fn los_angeles_report_labels(&mut self, _: &str) -> Option<RainPerformanceCalendarLabels> {
        Some(RainPerformanceCalendarLabels {
            daypart: RainPerformanceDaypart::Afternoon,
            month: 9,
            season: RainPerformanceSeason::Autumn,
        })
    }
}

impl RainArchivePerformanceOwners for FixtureOwners {
    /// Treat fixture bytes as the already bounded decompressed body.
    fn gunzip_bounded(
        &mut self,
        compressed: &[u8],
        maximum_output_bytes: usize,
    ) -> Result<Vec<u8>, TaskFailure> {
        // retain the production bound in the fixture owner
        if compressed.len() > maximum_output_bytes {
            return Err(failure("fixture body exceeds bound"));
        }
        Ok(compressed.to_vec())
    }

    /// Return the receipt-bound fixture digest.
    fn body_sha256(&mut self, _: &[u8]) -> Result<String, TaskFailure> {
        Ok(BODY_HASH.to_owned())
    }
}

/// Carry verified bytes through the 0/6/12 cohort, evidence join, and report composer.
#[test]
fn admitted_archive_builds_one_composed_native_report() {
    let mut owners = FixtureOwners;
    let mut admission = FixtureAdmission;
    let mut replay_time = FixtureReplayTime;
    let report = create_archived_rain_performance_report(
        fixture_package(),
        &fixture_report_inputs(),
        "2026-09-15T00:00:00.000Z",
        &"c".repeat(40),
        &mut owners,
        &mut admission,
        &mut replay_time,
    )
    .expect("verified archive should compose");

    assert!(matches!(report.document, JsonValue::Object(_)));
    assert_eq!(report.verified_capture_count, 4);
    assert_eq!(report.replayed_run_count, 1);
    assert_eq!(report.evidence.development_amount_pairs.len(), 23);
}

/// Refuse a saved output that cannot bind its exact source claim before model replay.
#[test]
fn saved_run_without_exact_source_is_refused() {
    let mut package = fixture_package();
    package.runs[0].stored.forecastClaimId = "missing".to_owned();
    let mut owners = FixtureOwners;
    let mut admission = FixtureAdmission;
    let mut replay_time = FixtureReplayTime;
    let error = create_archived_rain_performance_report(
        package,
        &fixture_report_inputs(),
        "2026-09-15T00:00:00.000Z",
        &"c".repeat(40),
        &mut owners,
        &mut admission,
        &mut replay_time,
    )
    .expect_err("missing exact source should fail closed");

    assert_eq!(error, RainArchivePerformanceError::MissingSourceForecast);
}

/// Build one verified typed archive containing current, prior, and excluded captures.
fn fixture_package() -> AdmittedRainArchivePackage {
    let forecasts = [
        ("current", 14_i64, 12_i64),
        ("prior-6", 14, 6),
        ("prior-12", 14, 0),
        ("stale-18", 13, 18),
    ];
    let mut claims = Vec::new();
    let mut receipts = Vec::new();
    let mut bodies = Vec::new();
    // retain a stable archive order across all forecast fixtures
    for (claim_id, day, hour) in forecasts {
        let member_path = format!("bodies/{claim_id}.json.gz");
        claims.push(AdmittedRainArchiveClaim {
            id: claim_id.to_owned(),
            kind: RainAdjustmentCaptureKindEnum::Forecast,
            run_initialized_at: Nullable::Value(fixture_utc((day * 24 + hour) * HOUR_MILLISECONDS)),
            station_id: Nullable::Null,
            window_end_exclusive: Nullable::Null,
            window_start: Nullable::Null,
        });
        receipts.push(AdmittedRainArchiveReceipt {
            body_member_path: member_path.clone(),
            body_sha256: BODY_HASH.to_owned(),
            claim_id: claim_id.to_owned(),
            completed_at: fixture_utc((day * 24 + hour) * HOUR_MILLISECONDS + 5 * 60_000),
            outcome: AdmittedRainArchiveReceiptOutcome::Valid,
        });
        bodies.push(AdmittedRainArchiveBody {
            member_path,
            compressed: vec![day as u8, hour as u8],
        });
    }
    // prove invalid receipts do not require claim or body joins
    receipts.push(AdmittedRainArchiveReceipt {
        body_member_path: "missing".to_owned(),
        body_sha256: "invalid".to_owned(),
        claim_id: "missing".to_owned(),
        completed_at: "invalid".to_owned(),
        outcome: AdmittedRainArchiveReceiptOutcome::Invalid,
    });
    AdmittedRainArchivePackage {
        bodies,
        claims,
        edges: Vec::new(),
        receipts,
        runs: vec![AdmittedRainArchiveRun {
            record_revision_identity: "revision".to_owned(),
            stored: fixture_stored_run(),
        }],
    }
}

/// Build one exact saved run matching the replay owner.
fn fixture_stored_run() -> RainAdjustmentRun {
    let initialized_at_ms = (14 * 24 + 12) * HOUR_MILLISECONDS;
    RainAdjustmentRun {
        runInitializedAt: "2026-09-14T12:00:00.000Z".to_owned(),
        modelSha256: MODEL_HASH.to_owned(),
        inputSha256: "d".repeat(64),
        forecastClaimId: "current".to_owned(),
        firstReceivedAt: "2026-09-14T12:05:00.000Z".to_owned(),
        decisionAt: "2026-09-14T20:00:00.000Z".to_owned(),
        generatedAt: "2026-09-14T20:00:01.000Z".to_owned(),
        hours: (9..=31)
            .map(|lead| RainAdjustmentHour {
                validAt: fixture_utc(initialized_at_ms + lead * HOUR_MILLISECONDS),
                modelLeadHours: lead as u64,
                rawPrecipitationMm: 1.0,
                correctedPrecipitationMm: 1.0,
                applied: false,
                reasonCode: Nullable::Value("phase_unsupported".to_owned()),
            })
            .collect(),
    }
}

/// Build original package identity inputs for report composition.
fn fixture_report_inputs() -> RainPerformanceReportPackageInputs {
    RainPerformanceReportPackageInputs {
        adjustment_evidence_manifest_sha256: "e".repeat(64),
        adjustment_evidence_watermark_sha256: "f".repeat(64),
        forecast_training_manifest_sha256: "1".repeat(64),
        local_date_from: "2026-09-14".to_owned(),
        local_date_to: "2026-09-14".to_owned(),
        target_cutoff_at: "2026-09-15T00:00:00.000Z".to_owned(),
    }
}

/// Parse the fixture's canonical UTC or Open-Meteo hour spelling.
fn fixture_epoch(value: &str) -> Option<i64> {
    let day = value.get(8..10)?.parse::<i64>().ok()?;
    let hour = value.get(11..13)?.parse::<i64>().ok()?;
    let minute = value.get(14..16)?.parse::<i64>().ok()?;
    Some(((day * 24 + hour) * 60 + minute) * 60_000)
}

/// Format one fixture epoch as a canonical UTC millisecond instant.
fn fixture_utc(epoch_ms: i64) -> String {
    let total_minutes = epoch_ms / 60_000;
    let day = total_minutes / (24 * 60);
    let hour = total_minutes / 60 % 24;
    let minute = total_minutes % 60;
    format!("2026-09-{day:02}T{hour:02}:{minute:02}:00.000Z")
}

/// Format one fixture epoch as an Open-Meteo hourly instant.
fn fixture_open_meteo(epoch_ms: i64) -> String {
    let total_hours = epoch_ms / HOUR_MILLISECONDS;
    let day = total_hours / 24;
    let hour = total_hours % 24;
    format!("2026-09-{day:02}T{hour:02}:00")
}

/// Build one bounded fixture task failure.
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}
