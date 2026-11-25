//! Compiled-Node parity for the frozen wind development bootstrap and fold evaluator.

use std::{path::PathBuf, process::Command};

use weather_adjustment::{
    WindBootstrapDateSlot, WindBootstrapPairedEvent, WindBootstrapScoredWindow,
    create_wind_moving_block_start_plan, evaluate_wind_development_loso_fold,
    score_wind_moving_block_bootstrap,
};
use weather_contracts::{
    DevelopmentLosoFoldResultV1, DevelopmentLosoFoldResultV1ContractVersionLiteral,
    DevelopmentLosoFoldResultV1FoldEnum as Fold, DevelopmentLosoStationScoreV1,
    ForecastAdjustmentMetric as Metric, ForecastAdjustmentMetricBand,
    ForecastLeadBandKey as LeadBand, ForecastObservationProviderFamily as Provider,
    ForecastObservationStation, ForecastObservationStationKey as StationKey,
};

type Loss = (f64, f64);

const ORDINARY_LOSSES: [Loss; 5] = [
    (18.0, 15.0),
    (14.0, 12.0),
    (10.0, 9.0),
    (16.0, 14.0),
    (12.0, 10.0),
];
const ZERO_LOSSES: [Loss; 5] = [(0.0, 0.0); 5];
const IDENTITIES: [(StationKey, Provider); 5] = [
    (StationKey::Tempest64255, Provider::Tempest),
    (StationKey::NetatmoNearby, Provider::Netatmo),
    (StationKey::AmbientMaxweather, Provider::Ambient),
    (StationKey::Tempest126537, Provider::Tempest),
    (StationKey::BallydideanEcowitt, Provider::Ecowitt),
];

/// Compare every ordered bootstrap bit and generated fold field to compiled original Node.
#[test]
fn matches_compiled_wind_development_algorithms() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/wind_development_node_oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("compiled Node wind-development oracle starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = String::from_utf8(output.stdout).expect("oracle output is UTF-8");
    let actual = rust_projection().join("\n") + "\n";
    assert_eq!(actual, expected);
}

/// Run the identical retained-empty-slot and fold corpus through Rust.
fn rust_projection() -> Vec<String> {
    let mut rows = bootstrap_projection();
    rows.push(fold_projection("ordinary-pass", &ORDINARY_LOSSES, &[]));
    rows.push(fold_projection(
        "material-harm",
        &ORDINARY_LOSSES,
        &[StationKey::NetatmoNearby],
    ));
    rows.push(fold_projection("zero-raw", &ZERO_LOSSES, &[]));
    rows
}

/// Project the exact 42-date seeded plan and all 2,000 ordered replicate skills.
fn bootstrap_projection() -> Vec<String> {
    let event_rows = (0..42)
        .map(|index| {
            // retain every sixth date as a first-class empty sampler slot
            if index % 6 == 0 {
                Vec::new()
            } else {
                vec![WindBootstrapPairedEvent {
                    actual: 8.0 + (index % 9) as f64 * 0.5,
                    adjusted_prediction: 8.25 + (index % 7) as f64 * 0.5,
                    raw_prediction: 6.5 + (index % 5) as f64 * 0.75,
                }]
            }
        })
        .collect::<Vec<_>>();
    let date_slots = event_rows
        .iter()
        .enumerate()
        .map(|(index, events)| WindBootstrapDateSlot {
            events,
            local_day_ordinal: 20_089 + index as i64,
        })
        .collect::<Vec<_>>();
    let windows = [WindBootstrapScoredWindow {
        date_slots: &date_slots,
        key: "wind-speed:fold-5",
    }];
    let plan = create_wind_moving_block_start_plan(&[date_slots.len()])
        .expect("42-date bootstrap plan is admitted");
    let result = score_wind_moving_block_bootstrap(&windows, false)
        .expect("42-date bootstrap fixture is scoreable");
    let mut output = plan
        .iter()
        .enumerate()
        .map(|(index, replicate)| {
            format!(
                "PLAN\t{index}\t{}",
                replicate[0]
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>();
    output.push(
        [
            "BOOTSTRAP".to_owned(),
            result.event_count.to_string(),
            bits(result.raw_loss),
            bits(result.adjusted_loss),
            bits(result.skill),
            bits(result.bootstrap_lower_bound),
            bits(result.bootstrap_upper_bound),
        ]
        .join("\t"),
    );
    output.extend(
        result
            .replicate_skills
            .iter()
            .enumerate()
            .map(|(index, skill)| format!("REPLICATE\t{index}\t{}", bits(*skill))),
    );
    output
}

/// Evaluate one original fold case and project every generated output field.
fn fold_projection(label: &str, losses: &[Loss; 5], material_harm: &[StationKey]) -> String {
    let stations = IDENTITIES
        .iter()
        .map(|&(key, family)| admitted_station(key, family))
        .collect::<Vec<_>>();
    let station_scores = IDENTITIES
        .iter()
        .zip(losses)
        .map(
            |(&(key, family), &(raw_loss, adjusted_loss))| DevelopmentLosoStationScoreV1 {
                adjustedLoss: adjusted_loss,
                eventCount: 100,
                physicalStationKey: key,
                pointSkill: paired_skill(raw_loss, adjusted_loss),
                providerFamily: family,
                rawLoss: raw_loss,
                remainingNetworkScoreEvents: 100,
                scoreMatches: 100,
                trainingMatches: 500,
            },
        )
        .collect::<Vec<_>>();
    let hashes = ["5", "4", "3", "2", "1"]
        .map(|value| value.repeat(64))
        .to_vec();
    let result = evaluate_wind_development_loso_fold(
        &ForecastAdjustmentMetricBand {
            leadBand: LeadBand::Value001024,
            metric: Metric::WindSpeedMps,
        },
        Fold::Value5,
        &stations,
        &station_scores,
        &hashes,
        0.01,
        material_harm,
    )
    .expect("development fold fixture is admitted");
    fold_row(label, &result)
}

/// Construct the admitted catalog identity fields consumed by fold evaluation.
fn admitted_station(key: StationKey, provider_family: Provider) -> ForecastObservationStation {
    ForecastObservationStation {
        acceptedSourceKeys: Vec::new(),
        distanceMeters: 0.0,
        eligibleMetrics: vec![Metric::WindSpeedMps],
        key,
        latitude: 0.0,
        longitude: 0.0,
        nearestRank: 1,
        providerFamily: provider_family,
        unnormalizedSpatialWeight: 1.0,
    }
}

/// Reproduce the frozen zero-baseline paired-skill cases for fixture admission.
fn paired_skill(raw_loss: f64, adjusted_loss: f64) -> f64 {
    match (raw_loss, adjusted_loss) {
        (0.0, 0.0) => 0.0,
        (0.0, _) => -1.0,
        _ => (raw_loss - adjusted_loss) / raw_loss,
    }
}

/// Project one generated fold in the compiled original field order.
fn fold_row(label: &str, result: &DevelopmentLosoFoldResultV1) -> String {
    [
        "FOLD".to_owned(),
        label.to_owned(),
        DevelopmentLosoFoldResultV1ContractVersionLiteral::WIRE_VALUE.to_owned(),
        fold_name(result.fold).to_owned(),
        metric_name(result.metricBand.metric).to_owned(),
        lead_band_name(result.metricBand.leadBand).to_owned(),
        result.passed.to_string(),
        bits(result.bootstrapLowerBound),
        bits(result.providerBalancedRawLoss),
        bits(result.providerBalancedAdjustedLoss),
        bits(result.providerBalancedSkill),
        bits(result.nonnegativeStationFraction),
        result.auxiliaryModelSha256s.join(","),
        result.materialHarmSliceKeys.join(","),
        result
            .providerFamilies
            .iter()
            .map(|&family| provider_name(family))
            .collect::<Vec<_>>()
            .join(","),
        result
            .scoreableStationKeys
            .iter()
            .map(|&key| station_name(key))
            .collect::<Vec<_>>()
            .join(","),
    ]
    .join("\t")
}

/// Encode one Rust binary64 result in the oracle's big-endian hexadecimal form.
fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

/// Render the generated fold wire value.
const fn fold_name(fold: Fold) -> &'static str {
    match fold {
        Fold::Value1 => "1",
        Fold::Value2 => "2",
        Fold::Value3 => "3",
        Fold::Value4 => "4",
        Fold::Value5 => "5",
    }
}

/// Render the generated metric wire value.
const fn metric_name(metric: Metric) -> &'static str {
    match metric {
        Metric::RelativeHumidityPercent => "relativeHumidityPercent",
        Metric::TemperatureC => "temperatureC",
        Metric::WindDirectionDegrees => "windDirectionDegrees",
        Metric::WindGustMps => "windGustMps",
        Metric::WindSpeedMps => "windSpeedMps",
    }
}

/// Render the generated lead-band wire value.
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

/// Render the generated provider-family wire value.
const fn provider_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Ambient => "ambient",
        Provider::Ecowitt => "ecowitt",
        Provider::Netatmo => "netatmo",
        Provider::Tempest => "tempest",
    }
}

/// Render the generated station-key wire value.
const fn station_name(key: StationKey) -> &'static str {
    match key {
        StationKey::AmbientMaxweather => "ambient-maxweather",
        StationKey::AmbientMerlin => "ambient-merlin",
        StationKey::BallydideanEcowitt => "ballydidean-ecowitt",
        StationKey::NetatmoNearby => "netatmo-nearby",
        StationKey::Tempest126537 => "tempest-126537",
        StationKey::Tempest168853 => "tempest-168853",
        StationKey::Tempest201058 => "tempest-201058",
        StationKey::Tempest203055 => "tempest-203055",
        StationKey::Tempest225947 => "tempest-225947",
        StationKey::Tempest38270 => "tempest-38270",
        StationKey::Tempest64255 => "tempest-64255",
    }
}
