use super::*;

#[test]
fn retains_original_manifest_authorities_and_member_partitions() {
    assert_eq!(MIGRATIONS.len(), 17);
    assert_eq!(FORECAST_TRAINING_SOURCE_IDENTITIES.len(), 14);
    assert_eq!(FORECAST_TRAINING_STATION_KEYS.len(), 11);
    assert_eq!(FORECAST_TRAINING_METRIC_NAMES.len(), 5);

    assert_eq!(
        validate_member_path(
            "members/2026-11-23/station-hour/ambient-maxweather.jsonl.gz",
            "2026-11-23",
            ForecastTrainingRecordKind::StationHour,
            Some("ambient-maxweather"),
        ),
        Some(()),
    );
    assert_eq!(
        validate_member_path(
            "members/2026-11-23/fixed-lead-anchor/forecast.jsonl.gz",
            "2026-11-23",
            ForecastTrainingRecordKind::FixedLeadAnchor,
            None,
        ),
        Some(()),
    );
    assert_eq!(
        validate_member_path(
            "members/2026-11-23/legacy-v4-retrieval/forecast.jsonl.gz",
            "2026-11-23",
            ForecastTrainingRecordKind::LegacyV4RetrievalSnapshot,
            None,
        ),
        Some(()),
    );
}
