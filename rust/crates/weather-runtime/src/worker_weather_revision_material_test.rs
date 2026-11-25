use weather_contracts::{
    CanonicalWeatherMetrics, JsonValue, NormalizedWeatherRecord, Nullable, SourceKind,
    WeatherRecordMetadata,
};
use weather_database::{
    AdmittedRevisionUtcInstant, PersistedWeatherAdjustmentRevision, Sha256Digest,
};

use super::*;

const PHYSICAL_SOURCE: &str = "ambient-maxweather-observations-v1";
const PHYSICAL_FINGERPRINT: &str =
    "7a7528a6278924ca5280a1a6045b6647b7e660b112d7fa3008c542a17ff99df4";

#[derive(Default)]
struct Values {
    encoded: usize,
}

impl WorkerWeatherRevisionMaterialValues for Values {
    /// Parse compact fixture clocks through the injected time owner.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Option<i64> {
        value
            .strip_prefix('t')?
            .parse::<f64>()
            .ok()
            .filter(|hour| hour.is_finite())
            .map(|hour| (hour * HOUR_MILLISECONDS as f64) as i64)
    }

    /// Assert the typed projection reached the original named-order codec.
    fn encode_revision_batch_projection(
        &mut self,
        projection: &AdjustmentRevisionBatchProjection,
    ) -> Option<Vec<u8>> {
        self.encoded += 1;
        (projection.logicalKey.len() == 4
            && projection.source.len() == 9
            && !projection.rows.is_empty())
        .then(|| {
            b"named-order-projection
"
            .to_vec()
        })
    }

    /// Distinguish the source contract epoch from the projection body digest.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        let digest = if bytes
            == b"named-order-projection
"
        {
            "b"
        } else {
            "a"
        };
        Some(Sha256Digest::admitted(digest.repeat(64)))
    }
}

/// Select and sort the exact 168 future hours from one six-hour Best Match cycle.
#[test]
fn complete_best_match_cycle_selects_one_ordered_projection_group() {
    let mut rows = (1..=BEST_MATCH_HOURS)
        .rev()
        .map(|hour| forecast_revision(hour as i64))
        .collect::<Vec<_>>();
    let mut values = Values::default();
    let selected = select_weather_revision_group(&rows, &[], "t0", &mut values).unwrap();
    assert_eq!(
        selected.projection_kind,
        AdjustmentRevisionBatchProjectionProjectionKindEnum::ActualBestMatch
    );
    assert_eq!(selected.revisions.len(), BEST_MATCH_HOURS);
    assert_eq!(selected.revisions.first().unwrap().record.validAt, "t1");
    assert_eq!(
        selected.revisions.last().unwrap().record.validAt,
        format!("t{BEST_MATCH_HOURS}")
    );

    // reject both an incomplete body and a cycle outside the four daily anchors
    rows.pop();
    assert!(select_weather_revision_group(&rows, &[], "t0", &mut values).is_none());
    let mut off_cycle = (2..=BEST_MATCH_HOURS + 1)
        .map(|hour| forecast_revision_at(hour as i64, "t1"))
        .collect::<Vec<_>>();
    assert!(select_weather_revision_group(&off_cycle, &[], "t0", &mut values).is_none());
    off_cycle[0].record.sourceId = "other".to_owned();
    assert!(select_weather_revision_group(&off_cycle, &[], "t0", &mut values).is_none());

    // preserve the TS UTC-hour gate even when the product clock has nonzero minutes
    let minute_cycle = (1..=BEST_MATCH_HOURS)
        .map(|hour| forecast_revision_at(hour as i64, "t0.5"))
        .collect::<Vec<_>>();
    assert!(select_weather_revision_group(&minute_cycle, &[], "t0", &mut values).is_some());

    // select old material but fail it before the original encoder or stage operation
    let pre_epoch = select_weather_revision_group(&minute_cycle, &[], "t1", &mut values).unwrap();
    let mut stage_values = Values::default();
    assert!(prepare_weather_revision_material(&pre_epoch, &mut stage_values).is_err());
    assert_eq!(stage_values.encoded, 0);
    let mut old_receipt = (7..=BEST_MATCH_HOURS + 6)
        .map(|hour| forecast_revision_at(hour as i64, "t6"))
        .collect::<Vec<_>>();
    old_receipt[0].logical_received_at = AdmittedRevisionUtcInstant::admitted("t0");
    let old_receipt = select_weather_revision_group(&old_receipt, &[], "t1", &mut values).unwrap();
    assert!(prepare_weather_revision_material(&old_receipt, &mut stage_values).is_err());
}

/// Admit only exact retained physical lineage rows at or after the capture epoch.
#[test]
fn physical_query_cohort_requires_source_lineage_epoch_and_order() {
    let changed = vec![physical_revision(10)];
    let pending = vec![physical_revision(10), physical_revision(11)];
    let mut values = Values::default();
    let selected = select_weather_revision_group(&changed, &pending, "t10", &mut values).unwrap();
    assert_eq!(
        selected.projection_kind,
        AdjustmentRevisionBatchProjectionProjectionKindEnum::TargetRevision
    );
    assert_eq!(selected.revisions.len(), 2);

    let mut wrong_lineage = changed.clone();
    wrong_lineage[0].source_config_fingerprint = "f".repeat(64);
    assert!(select_weather_revision_group(&wrong_lineage, &pending, "t10", &mut values).is_none());
    assert!(select_weather_revision_group(&changed, &pending, "t11", &mut values).is_none());
    let reversed = vec![physical_revision(11), physical_revision(10)];
    assert!(select_weather_revision_group(&changed, &reversed, "t10", &mut values).is_none());

    // trust the database-owned pending prefix without applying the current roster again
    let mut prior_pending = pending.clone();
    for revision in &mut prior_pending {
        revision.source_key = "prior-db-owned-lineage".to_owned();
        revision.source_config_fingerprint = "c".repeat(64);
    }
    let selected =
        select_weather_revision_group(&changed, &prior_pending, "t10", &mut values).unwrap();
    assert_eq!(selected.revisions[0].source_key, "prior-db-owned-lineage");
}

/// Build exact generated binary64/null rows and delegate named-order encoding and SHA.
#[test]
fn selected_group_builds_original_typed_projection_material() {
    let selected = SelectedWeatherRevisionGroup {
        capture_epoch: "t10".to_owned(),
        projection_kind: AdjustmentRevisionBatchProjectionProjectionKindEnum::TargetRevision,
        revisions: vec![physical_revision(10), physical_revision(11)],
    };
    let mut values = Values::default();
    let prepared = prepare_weather_revision_material(&selected, &mut values).unwrap();
    assert_eq!(values.encoded, 1);
    assert_eq!(
        prepared.bytes,
        b"named-order-projection
"
    );
    assert_eq!(prepared.identity.as_str(), "b".repeat(64));
    assert_eq!(
        prepared.projection.family,
        AdjustmentRevisionBatchProjectionFamilyEnum::Shared
    );
    assert_eq!(prepared.projection.rows.len(), 2);
    assert_eq!(
        row_value(&prepared.projection.rows[0], "temperatureC64"),
        &AdjustmentRevisionBatchProjectionRowsValue::String(
            "3ff8000000000000".encode_utf16().collect()
        )
    );
    assert_eq!(
        source_value(&prepared.projection, "contractEpoch"),
        &Nullable::Value(
            format!("normalized-weather/v1/{}", "a".repeat(64))
                .encode_utf16()
                .collect()
        )
    );
    assert_eq!(
        source_value(&prepared.projection, "upstreamModel"),
        &Nullable::Null
    );
}

/// Construct one Best Match revision with an independently selectable valid hour.
fn forecast_revision(hour: i64) -> PersistedWeatherAdjustmentRevision {
    forecast_revision_at(hour, "t0")
}

/// Construct one Best Match revision for a supplied product cycle.
fn forecast_revision_at(hour: i64, product_run_at: &str) -> PersistedWeatherAdjustmentRevision {
    let mut revision = revision(
        hour,
        SourceKind::Forecast,
        "open-meteo-forecast-v4",
        "f".repeat(64),
    );
    revision.record.productRunAt = Nullable::Value(product_run_at.to_owned());
    revision.record.metadata.model = Nullable::Value("best_match".to_owned());
    revision.record.metadata.provider = Nullable::Value(vec![(
        "dataset".encode_utf16().collect(),
        JsonValue::String("best_match".encode_utf16().collect()),
    )]);
    revision
}

/// Construct one retained target-source revision.
fn physical_revision(hour: i64) -> PersistedWeatherAdjustmentRevision {
    revision(
        hour,
        SourceKind::PhysicalSensor,
        PHYSICAL_SOURCE,
        PHYSICAL_FINGERPRINT.to_owned(),
    )
}

/// Construct one typed persisted weather revision with complete metric shape.
fn revision(
    hour: i64,
    source_kind: SourceKind,
    source_key: &str,
    source_config_fingerprint: String,
) -> PersistedWeatherAdjustmentRevision {
    PersistedWeatherAdjustmentRevision {
        adapter_version: "adapter/v1".to_owned(),
        content_hash: Sha256Digest::admitted(format!("{hour:064x}")),
        logical_received_at: AdmittedRevisionUtcInstant::admitted(format!("t{hour}")),
        provider_key: "provider".to_owned(),
        record: NormalizedWeatherRecord {
            metadata: WeatherRecordMetadata {
                device: Nullable::Null,
                model: Nullable::Null,
                provider: Nullable::Null,
                quality: Nullable::Null,
                upstreamTimezone: "UTC".to_owned(),
            },
            metrics: CanonicalWeatherMetrics {
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
                temperatureC: Nullable::Value(1.5),
                uvIndex: Nullable::Null,
                waterLevelM: Nullable::Null,
                windDirectionDegrees: Nullable::Null,
                windGustMps: Nullable::Null,
                windSpeedMps: Nullable::Null,
                wetBulbGlobeTemperatureC: Nullable::Null,
            },
            productRunAt: Nullable::Null,
            receivedAt: format!("t{hour}"),
            sourceId: "42".to_owned(),
            sourceKind: source_kind,
            validAt: format!("t{hour}"),
        },
        source_config_fingerprint,
        source_key: source_key.to_owned(),
    }
}

/// Borrow one generated row member by its original UTF-16 key.
fn row_value<'a>(
    row: &'a [(Vec<u16>, AdjustmentRevisionBatchProjectionRowsValue)],
    key: &str,
) -> &'a AdjustmentRevisionBatchProjectionRowsValue {
    row.iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
        .unwrap()
}

/// Borrow one generated source member by its original UTF-16 key.
fn source_value<'a>(
    projection: &'a AdjustmentRevisionBatchProjection,
    key: &str,
) -> &'a Nullable<Vec<u16>> {
    projection
        .source
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
        .unwrap()
}
