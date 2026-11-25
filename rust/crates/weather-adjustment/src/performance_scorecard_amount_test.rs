use std::collections::HashMap;

use super::*;

#[derive(Default)]
struct Calendar {
    clocks: HashMap<String, i64>,
    dates: HashMap<String, String>,
}

impl ForecastAdjustmentScorecardCalendar for Calendar {
    /// resolve only fixture-owned canonical clocks.
    fn canonical_utc_epoch_milliseconds(&mut self, value: &str) -> Option<i64> {
        self.clocks.get(value).copied()
    }

    /// resolve the fixture's admitted Los Angeles date.
    fn los_angeles_local_date(&mut self, value: &str, _epoch_milliseconds: i64) -> Option<String> {
        self.dates.get(value).cloned()
    }
}

/// coalesce overlap surfaces and retain the earliest commit.
#[test]
fn coalesces_identical_overlap_at_earliest_commit() {
    let mut later = pair(
        "row",
        "key",
        "2026-01-01",
        "valid",
        "vintage",
        [10.0, 12.0, 11.0],
    );
    later.firstEdgeCommittedAt = Nullable::Value("commit-late".to_owned());
    later.sourceReceiptAt = Nullable::Value("source-late".to_owned());
    let mut earlier = later.clone();
    earlier.firstEdgeCommittedAt = Nullable::Value("commit-early".to_owned());
    earlier.sourceReceiptAt = Nullable::Value("source-early".to_owned());
    let mut calendar = calendar(&[
        ("commit-late", 30, "2025-12-31"),
        ("commit-early", 20, "2025-12-31"),
    ]);

    let result =
        coalesce_forecast_adjustment_performance_pairs(&[later, earlier.clone()], &mut calendar)
            .expect("coalesced");

    assert_eq!(result.duplicate_count, 1);
    assert_eq!(result.rows, vec![earlier]);
}

/// preserve source insertion order when every sort field ties.
#[test]
fn coalescing_sort_is_stable_for_distinct_identities() {
    let first = pair(
        "row-one",
        "same",
        "2026-01-01",
        "valid",
        "vintage",
        [10.0, 12.0, 11.0],
    );
    let second = pair(
        "row-two",
        "same",
        "2026-01-01",
        "valid",
        "vintage",
        [10.0, 12.0, 11.0],
    );

    let result = coalesce_forecast_adjustment_performance_pairs(
        &[second.clone(), first.clone()],
        &mut Calendar::default(),
    )
    .expect("coalesced");

    assert_eq!(result.rows, vec![second, first]);
}

/// preserve every exclusion counter and descriptive diagnostic independently.
#[test]
fn prepares_causal_population_with_exact_exclusions() {
    let mut included = pair(
        "one",
        "one",
        "2026-01-01",
        "valid-one",
        "v1",
        [2.0, 2.0, 2.0],
    );
    included.fallback = true;
    included.provenanceComplete = false;
    let mut missing = pair(
        "two",
        "two",
        "2026-01-01",
        "valid-two",
        "v2",
        [0.0, 1.0, 1.0],
    );
    missing.target = Nullable::Null;
    let mut invalid = pair(
        "three",
        "three",
        "2026-01-01",
        "valid-three",
        "v3",
        [0.0, 1.0, 1.0],
    );
    invalid.target = Nullable::Value(f64::NAN);
    let mut no_receipts = pair(
        "four",
        "four",
        "2026-01-01",
        "valid-four",
        "v4",
        [0.0, 1.0, 1.0],
    );
    no_receipts.evidenceClass = ForecastAdjustmentEvidenceClass::AsIssued;
    let mut late = pair(
        "five",
        "five",
        "2026-01-01",
        "valid-five",
        "v5",
        [0.0, 1.0, 1.0],
    );
    late.evidenceClass = ForecastAdjustmentEvidenceClass::ProspectiveReceipt;
    late.firstEdgeCommittedAt = Nullable::Value("commit-five".to_owned());
    late.sourceReceiptAt = Nullable::Value("source-five".to_owned());
    let mut calendar = calendar(&[
        ("valid-one", 100, "2026-01-01"),
        ("valid-two", 110, "2026-01-01"),
        ("valid-three", 120, "2026-01-01"),
        ("valid-four", 130, "2026-01-01"),
        ("valid-five", 140, "2026-01-01"),
        ("commit-five", 140, "2026-01-01"),
        ("source-five", 150, "2026-01-01"),
    ]);

    let prepared = prepare_forecast_adjustment_performance_pairs(
        &[included.clone(), missing, invalid, no_receipts, late],
        &mut calendar,
    )
    .expect("prepared");

    assert_eq!(prepared.rows, vec![included]);
    assert_eq!(prepared.fallbackCount, 1);
    assert_eq!(prepared.diagnostics.provenance_incomplete, 1);
    assert_eq!(prepared.exclusions.missing_target, 1);
    assert_eq!(prepared.exclusions.invalid_target, 1);
    assert_eq!(prepared.exclusions.receipt_chronology_missing, 1);
    assert_eq!(prepared.exclusions.issued_after_valid, 1);
    assert_eq!(prepared.exclusions.source_late, 1);
}

/// match the compiled TypeScript nested weights and amount metrics.
#[test]
fn balances_and_scores_exact_nested_population() {
    let rows = vec![
        pair("a", "a", "2026-01-01", "h1", "z", [10.0, 12.0, 11.0]),
        pair("b", "b", "2026-01-01", "h1", "a", [10.0, 14.0, 12.0]),
        pair("c", "c", "2026-01-01", "h2", "a", [0.0, 4.0, 2.0]),
        pair("d", "d", "2026-01-02", "h3", "a", [5.0, 1.0, 3.0]),
    ];

    let score = score_balanced_forecast_adjustment_pairs(&rows).expect("score");

    assert_eq!(
        score
            .weighted_rows
            .iter()
            .map(|row| (row.key.as_str(), row.weight))
            .collect::<Vec<_>>(),
        vec![("b", 0.125), ("a", 0.125), ("c", 0.25), ("d", 0.5)]
    );
    assert_eq!(score.metrics.raw.mae, 3.75);
    assert_eq!(score.metrics.adjusted.mae, 1.875);
    assert_eq!(score.metrics.raw.bias, -0.25);
    assert_eq!(score.metrics.adjusted.bias, -0.125);
    assert_eq!(score.metrics.raw.p95AbsoluteError, 4.0);
    assert_eq!(score.metrics.adjusted.p95AbsoluteError, 2.0);
    assert_eq!(score.metrics.raw.rmse, 14.5_f64.sqrt());
    assert_eq!(score.metrics.adjusted.rmse, 3.625_f64.sqrt());
    assert_eq!(score.metrics.skill, 0.5);
}

/// build one generated pair with development evidence and no receipt claims.
fn pair(
    row_identity: &str,
    key: &str,
    local_date: &str,
    valid_at: &str,
    vintage_key: &str,
    [target, raw, adjusted]: [f64; 3],
) -> ForecastAdjustmentPerformancePair {
    ForecastAdjustmentPerformancePair {
        adjustedPrediction: adjusted,
        evidenceClass: ForecastAdjustmentEvidenceClass::Development,
        fallback: false,
        firstEdgeCommittedAt: Nullable::Null,
        horizonHours: 1.0,
        key: key.to_owned(),
        localDate: local_date.to_owned(),
        provenanceComplete: true,
        rawPrediction: raw,
        rowIdentity: row_identity.to_owned(),
        sourceReceiptAt: Nullable::Null,
        target: Nullable::Value(target),
        targetKey: format!("target-{key}"),
        validAt: valid_at.to_owned(),
        vintageKey: vintage_key.to_owned(),
    }
}

/// build one fixture calendar from value, epoch, and local-date triples.
fn calendar(entries: &[(&str, i64, &str)]) -> Calendar {
    let mut calendar = Calendar::default();
    // retain every explicit clock
    for (value, epoch, date) in entries {
        calendar.clocks.insert((*value).to_owned(), *epoch);
        calendar
            .dates
            .insert((*value).to_owned(), (*date).to_owned());
    }
    calendar
}
