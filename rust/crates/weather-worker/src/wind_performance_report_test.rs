use weather_adjustment::{
    ForecastAdjustmentRainDiagnosticCalendar, ForecastAdjustmentScorecardCalendar,
};
use weather_contracts::{ForecastAdjustmentEvidenceClass, ForecastAdjustmentPerformancePair};

use super::*;
use crate::RainPerformanceCalendarLabels;

struct FixtureCalendar;

impl ForecastAdjustmentScorecardCalendar for FixtureCalendar {
    /// Map fixture instants to one order-preserving synthetic clock.
    fn canonical_utc_epoch_milliseconds(&mut self, value: &str) -> Option<i64> {
        let day = value.get(8..10)?.parse::<i64>().ok()?;
        let hour = value.get(11..13)?.parse::<i64>().ok()?;
        Some((day * 24 + hour) * 3_600_000)
    }

    /// Preserve the fixture's date prefix as its admitted local date.
    fn los_angeles_local_date(&mut self, value: &str, _: i64) -> Option<String> {
        Some(value.get(..10)?.to_owned())
    }
}

impl ForecastAdjustmentRainDiagnosticCalendar for FixtureCalendar {
    /// Keep every wind fixture outside the unused winter rain partition.
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

/// Keep the retrospective band population disjoint and explicitly unscored.
#[test]
fn counterfactual_rows_produce_pair_and_calendar_slices() {
    let band = "windSpeedMps:lead-1".to_owned();
    let counterfactual = BTreeMap::from([(
        band.clone(),
        vec![pair(
            ForecastAdjustmentEvidenceClass::RetrospectiveCounterfactual,
            "2026-10-07T12:00:00.000Z",
        )],
    )]);
    let issued = BTreeMap::from([(band.clone(), Vec::new())]);
    let report = create_wind_performance_report(
        counterfactual,
        issued,
        WindPerformanceNonActiveCounts::default(),
        &fixture_inputs(),
        &"c".repeat(64),
        &"d".repeat(64),
        "2026-10-08T00:00:00.000Z",
        &mut FixtureCalendar,
    )
    .expect("counterfactual report should assemble");

    let family = field(&report, "family");
    let pair_reviews = array(field(&report, "pairReviews"));
    let slices = array(field(family, "slices"));
    let support = field(family, "support");

    assert_eq!(
        field(family, "evidenceClass"),
        &text("retrospective_counterfactual")
    );
    assert_eq!(field(family, "comparisonState"), &text("unscored"));
    assert_eq!(
        field(family, "qualificationState"),
        &text("counterfactual_only")
    );
    assert_eq!(field(family, "supportState"), &text("insufficient"));
    assert_eq!(pair_reviews.len(), 1);
    assert_eq!(field(&pair_reviews[0], "pairIdentity"), &text(&band));
    assert_eq!(field(&pair_reviews[0], "rowCount"), &number(1.0));
    assert_eq!(field(support, "rowCount"), &number(1.0));
    assert_eq!(field(support, "eventCount"), &number(1.0));
    assert_eq!(slices.len(), 4);
    assert_eq!(field(&slices[0], "dimension"), &text("horizon"));
    assert_eq!(field(&slices[0], "label"), &text("windspeedmps-lead-1"));
    assert_eq!(field(&slices[1], "dimension"), &text("month"));
    assert_eq!(field(&slices[1], "label"), &text("10"));
}

/// Count unbanded fail-raw receipts in issued support without borrowing counterfactual rows.
#[test]
fn nonactive_receipts_select_the_empty_issued_population() {
    let band = "windGustMps:lead-1".to_owned();
    let counterfactual = BTreeMap::from([(
        band.clone(),
        vec![pair(
            ForecastAdjustmentEvidenceClass::RetrospectiveCounterfactual,
            "2026-10-07T12:00:00.000Z",
        )],
    )]);
    let issued = BTreeMap::from([(band, Vec::new())]);
    let report = create_wind_performance_report(
        counterfactual,
        issued,
        WindPerformanceNonActiveCounts {
            disabled: 2,
            not_applicable: 1,
        },
        &fixture_inputs(),
        &"c".repeat(64),
        &"d".repeat(64),
        "2026-10-08T00:00:00.000Z",
        &mut FixtureCalendar,
    )
    .expect("nonactive issued report should assemble");

    let family = field(&report, "family");
    let support = field(family, "support");
    let reasons = field(support, "exclusionReasons");

    assert_eq!(field(family, "evidenceClass"), &text("as_issued"));
    assert_eq!(
        field(family, "qualificationState"),
        &text("pending_support")
    );
    assert_eq!(field(support, "rowCount"), &number(3.0));
    assert_eq!(field(support, "eventCount"), &number(0.0));
    assert_eq!(field(support, "excludedCount"), &number(3.0));
    assert_eq!(field(support, "fallbackCount"), &number(3.0));
    assert_eq!(field(support, "gapCount"), &number(3.0));
    assert_eq!(field(reasons, "disabled"), &number(2.0));
    assert_eq!(field(reasons, "not_applicable"), &number(1.0));
}

/// Build the report metadata used by both aggregation fixtures.
fn fixture_inputs() -> RainPerformanceReportPackageInputs {
    RainPerformanceReportPackageInputs {
        adjustment_evidence_manifest_sha256: "a".repeat(64),
        adjustment_evidence_watermark_sha256: "a".repeat(64),
        forecast_training_manifest_sha256: "b".repeat(64),
        local_date_from: "2026-10-07".to_owned(),
        local_date_to: "2026-10-07".to_owned(),
        target_cutoff_at: "2026-10-08T00:00:00.000Z".to_owned(),
    }
}

/// Build one eligible band pair with stable fixture identities.
fn pair(
    evidence_class: ForecastAdjustmentEvidenceClass,
    valid_at: &str,
) -> ForecastAdjustmentPerformancePair {
    ForecastAdjustmentPerformancePair {
        adjustedPrediction: 2.0,
        evidenceClass: evidence_class,
        fallback: false,
        firstEdgeCommittedAt: Nullable::Null,
        horizonHours: 4.0,
        key: "pair-1".to_owned(),
        localDate: valid_at[..10].to_owned(),
        provenanceComplete: true,
        rawPrediction: 3.0,
        rowIdentity: "row-1".to_owned(),
        sourceReceiptAt: Nullable::Null,
        target: Nullable::Value(1.0),
        targetKey: "target-1".to_owned(),
        validAt: valid_at.to_owned(),
        vintageKey: "vintage-1".to_owned(),
    }
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

/// Read one expected JSON array.
fn array(value: &JsonValue) -> &[JsonValue] {
    let JsonValue::Array(values) = value else {
        panic!("expected array");
    };
    values
}
