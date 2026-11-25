//! Compiled-Node parity for the frozen wind robust hierarchy fitter.

use std::{path::PathBuf, process::Command};

use weather_adjustment::{
    AdmittedWindHierarchyObservation, WindRobustHierarchyError, fit_wind_robust_hierarchy,
};
use weather_contracts::{
    ForecastAdjustmentCoefficient, ForecastAdjustmentCoefficientDaypartEnum as Daypart,
    ForecastAdjustmentCoefficientLevelEnum as Level,
    ForecastAdjustmentCoefficientSeasonEnum as Season, ForecastAdjustmentMetric as Metric,
    ForecastLeadBandKey as LeadBand, Nullable,
};

struct OwnedObservation {
    daypart: Daypart,
    month: u8,
    residual: f64,
    season: Season,
    stable_id: String,
    valid_at: String,
}

/// reject a calendar pair the original derivation cannot produce
#[test]
fn rejects_inconsistent_month_parent() {
    let row = AdmittedWindHierarchyObservation {
        daypart: Daypart::Night,
        month: 1,
        reference_at: None,
        residual: 1.0,
        season: Season::Summer,
        stable_id: "row",
        target_lead_hours: 24,
        valid_at: "2025-01-01T00:00:00.000Z",
        weight: 1.0,
    };
    assert_eq!(
        fit_wind_robust_hierarchy(Metric::WindSpeedMps, LeadBand::Value001024, &[row]),
        Err(WindRobustHierarchyError::InvalidObservation),
    );
}

/// compare every generated field and binary64 result to compiled original Node
#[test]
fn matches_compiled_wind_robust_hierarchy() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/wind_robust_hierarchy_node_oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("compiled Node hierarchy oracle starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = String::from_utf8(output.stdout).expect("oracle output is UTF-8");
    let actual = rust_projection().join("\n") + "\n";
    assert_eq!(actual, expected);
}

/// run the identical bounded hierarchy corpus through the Rust fitter
fn rust_projection() -> Vec<String> {
    let threshold = threshold_rows();
    let mut rows = Vec::new();
    rows.extend(project(
        "below-root",
        Metric::WindGustMps,
        &threshold[..199],
    ));
    rows.extend(project("threshold-shrink", Metric::WindGustMps, &threshold));
    rows.extend(project(
        "speed-cap",
        Metric::WindSpeedMps,
        &same_cell_rows(200, |_| 100.0),
    ));
    rows.extend(project(
        "gust-cap",
        Metric::WindGustMps,
        &same_cell_rows(200, |_| 100.0),
    ));
    rows.extend(project(
        "direction-tie",
        Metric::WindDirectionDegrees,
        &same_cell_rows(200, |index| if index < 100 { -10.0 } else { 10.0 }),
    ));
    let mut wrapped = (0..100)
        .map(|index| observation(index, -170.0, 1, Season::Winter, Daypart::Night))
        .collect::<Vec<_>>();
    wrapped.extend(
        (0..101)
            .map(|index| observation(index + 100, 170.0, 7, Season::Summer, Daypart::Afternoon)),
    );
    rows.extend(project(
        "direction-wrap",
        Metric::WindDirectionDegrees,
        &wrapped,
    ));
    rows
}

/// fit one case and project the original generated coefficient field order
fn project(label: &str, metric: Metric, rows: &[OwnedObservation]) -> Vec<String> {
    let admitted = rows
        .iter()
        .map(|row| AdmittedWindHierarchyObservation {
            daypart: row.daypart,
            month: row.month,
            reference_at: None,
            residual: row.residual,
            season: row.season,
            stable_id: &row.stable_id,
            target_lead_hours: 24,
            valid_at: &row.valid_at,
            weight: 1.0,
        })
        .collect::<Vec<_>>();
    let coefficients = fit_wind_robust_hierarchy(metric, LeadBand::Value001024, &admitted)
        .expect("fixture observations are admitted");
    let mut output = vec![format!("CASE\t{label}\t{}", coefficients.len())];
    // retain the fitter's canonical root, season, then month order
    for (index, coefficient) in coefficients.iter().enumerate() {
        output.push(coefficient_row(label, index, coefficient));
    }
    output
}

/// project one coefficient with exact JavaScript binary64 bits
fn coefficient_row(
    label: &str,
    index: usize,
    coefficient: &ForecastAdjustmentCoefficient,
) -> String {
    [
        "COEFFICIENT".to_owned(),
        label.to_owned(),
        index.to_string(),
        metric_name(coefficient.metric).to_owned(),
        lead_band_name(coefficient.leadBand).to_owned(),
        level_name(coefficient.level).to_owned(),
        nullable_daypart(&coefficient.daypart).to_owned(),
        nullable_month(&coefficient.month),
        nullable_season(&coefficient.season).to_owned(),
        format!("{:016x}", coefficient.coefficient.to_bits()),
        format!("{:016x}", coefficient.effectiveEventCount.to_bits()),
    ]
    .join("\t")
}

/// construct exact root, season, and month support thresholds
fn threshold_rows() -> Vec<OwnedObservation> {
    let mut rows = (0..50)
        .map(|index| observation(index, 8.0, 1, Season::Winter, Daypart::Night))
        .collect::<Vec<_>>();
    rows.extend(
        (0..50).map(|index| observation(index + 50, 8.0, 2, Season::Winter, Daypart::Night)),
    );
    rows.extend(
        (0..49).map(|index| observation(index + 100, 8.0, 6, Season::Summer, Daypart::Afternoon)),
    );
    rows.extend(
        (0..50).map(|index| observation(index + 149, 8.0, 7, Season::Summer, Daypart::Afternoon)),
    );
    rows.push(observation(199, 8.0, 9, Season::Autumn, Daypart::Morning));
    rows
}

/// construct one same-cell residual corpus
fn same_cell_rows(count: usize, residual_for: impl Fn(usize) -> f64) -> Vec<OwnedObservation> {
    (0..count)
        .map(|index| {
            observation(
                index,
                residual_for(index),
                1,
                Season::Winter,
                Daypart::Night,
            )
        })
        .collect()
}

/// construct one unique-event hierarchy residual
fn observation(
    index: usize,
    residual: f64,
    month: u8,
    season: Season,
    daypart: Daypart,
) -> OwnedObservation {
    OwnedObservation {
        daypart,
        month,
        residual,
        season,
        stable_id: format!("row-{index:03}"),
        valid_at: format!("2025-01-01T00:00:00.{index:03}Z"),
    }
}

/// render the generated metric wire value
const fn metric_name(metric: Metric) -> &'static str {
    match metric {
        Metric::RelativeHumidityPercent => "relativeHumidityPercent",
        Metric::TemperatureC => "temperatureC",
        Metric::WindDirectionDegrees => "windDirectionDegrees",
        Metric::WindGustMps => "windGustMps",
        Metric::WindSpeedMps => "windSpeedMps",
    }
}

/// render the generated lead-band wire value
const fn lead_band_name(lead_band: LeadBand) -> &'static str {
    match lead_band {
        LeadBand::Value001024 => "001-024",
        LeadBand::Value025048 => "025-048",
        LeadBand::Value049072 => "049-072",
        LeadBand::Value073096 => "073-096",
        LeadBand::Value097120 => "097-120",
        LeadBand::Value121144 => "121-144",
        LeadBand::Value145168 => "145-168",
    }
}

/// render the generated hierarchy level wire value
const fn level_name(level: Level) -> &'static str {
    match level {
        Level::Value1 => "1",
        Level::Value2 => "2",
        Level::Value3 => "3",
    }
}

/// render one nullable generated daypart
fn nullable_daypart(daypart: &Nullable<Daypart>) -> &'static str {
    match daypart {
        Nullable::Null => "<null>",
        Nullable::Value(Daypart::Afternoon) => "afternoon",
        Nullable::Value(Daypart::Evening) => "evening",
        Nullable::Value(Daypart::Morning) => "morning",
        Nullable::Value(Daypart::Night) => "night",
    }
}

/// render one nullable generated month
fn nullable_month(month: &Nullable<u64>) -> String {
    match month {
        Nullable::Null => "<null>".to_owned(),
        Nullable::Value(month) => month.to_string(),
    }
}

/// render one nullable generated season
fn nullable_season(season: &Nullable<Season>) -> &'static str {
    match season {
        Nullable::Null => "<null>",
        Nullable::Value(Season::Autumn) => "autumn",
        Nullable::Value(Season::Spring) => "spring",
        Nullable::Value(Season::Summer) => "summer",
        Nullable::Value(Season::Winter) => "winter",
    }
}
