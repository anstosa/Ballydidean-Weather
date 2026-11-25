use weather_adjustment::{
    ForecastAdjustmentRainDiagnosticCalendar, ForecastAdjustmentScorecardCalendar,
};
use weather_contracts::{ForecastAdjustmentEvidenceClass, ForecastAdjustmentPerformancePair};

use super::*;

struct FixtureCalendar;

impl ForecastAdjustmentScorecardCalendar for FixtureCalendar {
    /// Map the fixture's canonical instants to an order-preserving clock.
    fn canonical_utc_epoch_milliseconds(&mut self, value: &str) -> Option<i64> {
        let date = value.get(8..10)?.parse::<i64>().ok()?;
        let hour = value.get(11..13)?.parse::<i64>().ok()?;
        Some((date * 24 + hour) * 3_600_000)
    }

    /// Preserve the fixture's date prefix as its admitted local date.
    fn los_angeles_local_date(&mut self, value: &str, _: i64) -> Option<String> {
        Some(value.get(..10)?.to_owned())
    }
}

impl ForecastAdjustmentRainDiagnosticCalendar for FixtureCalendar {
    /// Keep every fixture hour outside the winter diagnostic partition.
    fn los_angeles_is_winter(&mut self, _: &str) -> Option<bool> {
        Some(false)
    }
}

impl RainPerformanceReportCalendar for FixtureCalendar {
    /// Return one admitted October morning partition for fixture rows.
    fn los_angeles_report_labels(&mut self, _: &str) -> Option<RainPerformanceCalendarLabels> {
        Some(RainPerformanceCalendarLabels {
            daypart: RainPerformanceDaypart::Morning,
            month: 10,
            season: RainPerformanceSeason::Autumn,
        })
    }
}

/// Match the original empty CLI report as one complete closed JSON value.
#[test]
fn empty_evidence_matches_the_complete_original_closed_report() {
    let evidence = empty_evidence();
    let inputs = fixture_inputs();
    let actual = create_rain_performance_report(
        &evidence,
        &inputs,
        None,
        "2026-10-07T12:00:00.000Z",
        &"b".repeat(64),
        &mut FixtureCalendar,
    )
    .expect("empty report should close without evidence");

    assert_eq!(actual, expected_empty_report(&inputs));
}

/// Keep receipt qualification separate from the complete development reference.
#[test]
fn receipt_support_and_development_reference_use_disjoint_populations() {
    let mut evidence = empty_evidence();
    evidence.evidence_class = ForecastAdjustmentEvidenceClass::AsIssued;
    evidence.issued_amount_pairs = vec![
        pair(
            ForecastAdjustmentEvidenceClass::AsIssued,
            "eligible",
            "2026-10-07T12:00:00.000Z",
            Nullable::Value("2026-10-07T10:00:00.000Z".to_owned()),
            Nullable::Value("2026-10-07T09:00:00.000Z".to_owned()),
            Nullable::Value(1.0),
            true,
        ),
        pair(
            ForecastAdjustmentEvidenceClass::AsIssued,
            "missing",
            "2026-10-07T14:00:00.000Z",
            Nullable::Value("2026-10-07T11:00:00.000Z".to_owned()),
            Nullable::Value("2026-10-07T10:00:00.000Z".to_owned()),
            Nullable::Null,
            true,
        ),
        pair(
            ForecastAdjustmentEvidenceClass::AsIssued,
            "late",
            "2026-10-07T12:00:00.000Z",
            Nullable::Value("2026-10-07T13:00:00.000Z".to_owned()),
            Nullable::Value("2026-10-07T11:00:00.000Z".to_owned()),
            Nullable::Value(0.0),
            true,
        ),
    ];
    evidence.development_amount_pairs = (1..=7)
        .map(|day| {
            pair(
                ForecastAdjustmentEvidenceClass::Development,
                &format!("development-{day}"),
                &format!("2026-10-{day:02}T12:00:00.000Z"),
                Nullable::Null,
                Nullable::Null,
                Nullable::Value(day as f64),
                true,
            )
        })
        .collect();

    let report = create_rain_performance_report(
        &evidence,
        &fixture_inputs(),
        None,
        "2026-10-07T12:00:00.000Z",
        &"b".repeat(64),
        &mut FixtureCalendar,
    )
    .expect("partitioned report should assemble");
    let family = field(&report, "family");
    let support = field(family, "support");
    let development = field(&report, "developmentReference");

    assert_eq!(field(family, "evidenceClass"), &text("as_issued"));
    assert_eq!(
        field(family, "qualificationState"),
        &text("pending_support")
    );
    assert_eq!(field(support, "rowCount"), &number(3.0));
    assert_eq!(field(support, "targetRowCount"), &number(1.0));
    assert_eq!(field(support, "excludedCount"), &number(2.0));
    assert_eq!(field(support, "gapCount"), &number(2.0));
    assert_eq!(field(support, "wetDateCount"), &number(1.0));
    assert_eq!(field(support, "wetRowCount"), &number(1.0));
    assert_eq!(field(development, "rowCount"), &number(7.0));
    assert_ne!(
        field(field(development, "metrics"), "adjustedMae"),
        &JsonValue::Null,
    );
}

/// Build the report metadata from the original differential fixture.
fn fixture_inputs() -> RainPerformanceReportPackageInputs {
    RainPerformanceReportPackageInputs {
        adjustment_evidence_manifest_sha256: "a".repeat(64),
        adjustment_evidence_watermark_sha256: "a".repeat(64),
        forecast_training_manifest_sha256: "a".repeat(64),
        local_date_from: "2026-10-07".to_owned(),
        local_date_to: "2026-10-07".to_owned(),
        target_cutoff_at: "2026-10-07T10:00:00.000Z".to_owned(),
    }
}

/// Return an evidence object with no admitted rows.
fn empty_evidence() -> RainPerformanceEvidence {
    RainPerformanceEvidence {
        development_amount_pairs: Vec::new(),
        development_probability_pairs: Vec::new(),
        evidence_class: ForecastAdjustmentEvidenceClass::Development,
        issued_amount_pairs: Vec::new(),
        issued_probability_pairs: Vec::new(),
        prospective_amount_pairs: Vec::new(),
        prospective_probability_pairs: Vec::new(),
        serving_identity_sha256: Nullable::Null,
    }
}

/// Build one amount row with stable unique fixture identities.
#[allow(clippy::too_many_arguments)]
fn pair(
    evidence_class: ForecastAdjustmentEvidenceClass,
    key: &str,
    valid_at: &str,
    first_edge_committed_at: Nullable<String>,
    source_receipt_at: Nullable<String>,
    target: Nullable<f64>,
    provenance_complete: bool,
) -> ForecastAdjustmentPerformancePair {
    ForecastAdjustmentPerformancePair {
        adjustedPrediction: 0.5,
        evidenceClass: evidence_class,
        fallback: false,
        firstEdgeCommittedAt: first_edge_committed_at,
        horizonHours: 4.0,
        key: key.to_owned(),
        localDate: valid_at[..10].to_owned(),
        provenanceComplete: provenance_complete,
        rawPrediction: 1.5,
        rowIdentity: format!("row-{key}"),
        sourceReceiptAt: source_receipt_at,
        target,
        targetKey: format!("target-{key}"),
        validAt: valid_at.to_owned(),
        vintageKey: format!("vintage-{key}"),
    }
}

/// Return the complete expected empty report document.
fn expected_empty_report(inputs: &RainPerformanceReportPackageInputs) -> JsonValue {
    object(vec![
        ("contractVersion", text(REPORT_VERSION)),
        (
            "family",
            object(vec![
                ("bestMatchDiagnostic", JsonValue::Null),
                ("comparisonState", text("unscored")),
                ("evidenceClass", text("development")),
                ("evidenceCutoffAt", text(&inputs.target_cutoff_at)),
                ("family", text("rain")),
                ("metrics", unavailable_metric()),
                ("qualificationState", text("development_only")),
                ("rainDiagnostics", expected_unavailable_rain_diagnostics()),
                ("recommendation", text("review_candidate")),
                ("servingIdentitySha256", JsonValue::Null),
                ("servingState", text("authorized_active")),
                ("slices", JsonValue::Array(Vec::new())),
                (
                    "support",
                    object(vec![
                        ("dateCount", number(0.0)),
                        ("effectiveWeightSum", number(0.0)),
                        ("eventCount", number(0.0)),
                        ("excludedCount", number(0.0)),
                        (
                            "exclusionReasons",
                            object(vec![
                                ("invalid_target", number(0.0)),
                                ("issued_after_valid", number(0.0)),
                                ("missing_target", number(0.0)),
                                ("receipt_chronology_missing", number(0.0)),
                                ("source_late", number(0.0)),
                                ("provenance_incomplete", number(0.0)),
                            ]),
                        ),
                        ("fallbackCount", number(0.0)),
                        ("fallbackReasons", object(Vec::new())),
                        ("gapCount", number(0.0)),
                        ("rowCount", number(0.0)),
                        ("targetRowCount", number(0.0)),
                        ("validHourCount", number(0.0)),
                        ("vintageCount", number(0.0)),
                        ("wetDateCount", number(0.0)),
                        ("wetRowCount", number(0.0)),
                    ]),
                ),
                ("supportState", text("insufficient")),
            ]),
        ),
        ("generatedAt", text("2026-10-07T12:00:00.000Z")),
        ("inputs", package_inputs_json(inputs)),
        (
            "developmentReference",
            object(vec![
                ("evidenceClass", text("development")),
                ("metrics", unavailable_metric()),
                ("rowCount", number(0.0)),
            ]),
        ),
        ("siteKey", text(SITE_KEY)),
        ("sourceRevision", text(&"b".repeat(64))),
    ])
}

/// Return all three unavailable thresholds and their thirty empty bins.
fn expected_unavailable_rain_diagnostics() -> JsonValue {
    object(vec![
        (
            "accumulations",
            JsonValue::Array(
                [6.0, 12.0, 23.0]
                    .into_iter()
                    .map(|hours| {
                        object(vec![
                            ("adjustedMae", JsonValue::Null),
                            ("completeWindows", number(0.0)),
                            ("hours", number(hours)),
                            ("rawMae", JsonValue::Null),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("annualBalancedVolumeRatio", JsonValue::Null),
        ("heavyAdjustedMae", JsonValue::Null),
        ("heavyRawMae", JsonValue::Null),
        ("probabilityOrderViolationCount", number(0.0)),
        (
            "thresholds",
            JsonValue::Array(
                [0.1, 1.0, 2.5]
                    .into_iter()
                    .map(expected_unavailable_threshold)
                    .collect(),
            ),
        ),
        ("wetAdjustedMae", JsonValue::Null),
        ("wetRawMae", JsonValue::Null),
        ("winterBalancedVolumeRatio", JsonValue::Null),
    ])
}

/// Return one unavailable threshold with exactly ten closed reliability bins.
fn expected_unavailable_threshold(threshold: f64) -> JsonValue {
    let bin = object(vec![
        ("count", number(0.0)),
        ("meanProbability", JsonValue::Null),
        ("observedFrequency", JsonValue::Null),
    ]);
    object(vec![
        ("adjustedBrier", JsonValue::Null),
        ("csi", JsonValue::Null),
        ("falseAlarms", number(0.0)),
        ("far", JsonValue::Null),
        ("hits", number(0.0)),
        ("misses", number(0.0)),
        ("pod", JsonValue::Null),
        ("rawBrier", JsonValue::Null),
        ("reliability", JsonValue::Array(vec![bin; 10])),
        ("thresholdMmPerHour", number(threshold)),
    ])
}

/// Read one expected object field by its UTF-16 key.
fn field<'a>(value: &'a JsonValue, key: &str) -> &'a JsonValue {
    let JsonValue::Object(entries) = value else {
        panic!("expected object field {key}");
    };
    entries
        .iter()
        .find_map(|(candidate, value)| {
            // match the stored UTF-16 key exactly
            if candidate == &key.encode_utf16().collect::<Vec<_>>() {
                Some(value)
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("missing object field {key}"))
}
