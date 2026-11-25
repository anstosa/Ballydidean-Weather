//! Atomic database custody for one complete fixed-gauge target revision group.

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptProjectionKindEnum,
    AdjustmentRevisionGap, AdjustmentRevisionGapInput, AdjustmentRevisionGapReason,
    AdjustmentRevisionGapReasonEnum, AdjustmentRevisionProjectionKind,
    AdjustmentWeatherRevisionAdmissionKey, AdjustmentWeatherRevisionPointer,
    AdjustmentWeatherRevisionPointerSourceKindEnum, FieldPresence, JsonValue,
    NormalizedWeatherRecord, Nullable, RainFixedGaugeTargetRevisionGroup,
    RainFixedGaugeTargetRevisionResult, RainFixedGaugeTargetRevisionResultCommitted,
    RainFixedGaugeTargetRevisionResultCommittedStateLiteral, RainFixedGaugeTargetRevisionResultGap,
    RainFixedGaugeTargetRevisionResultGapStateLiteral, RainFixedGaugeTargetSourceCatalog,
    SourceKind,
};

use crate::worker_rain_fixed_gauge_read::RAIN_FIXED_GAUGE_SOURCE_CATALOG_SQL;
use crate::{
    AdjustmentRevisionAdmissionKey, AdjustmentRevisionGapMarker, MaintenancePublicationPlan,
    SerializedJson, adjustment_revision_gap_plan,
};

const SOURCE_KEYS: [&str; 12] = [
    "rain-target-tempest-64255",
    "rain-target-tempest-225947",
    "rain-target-tempest-38270",
    "rain-target-tempest-168853",
    "rain-target-tempest-126537",
    "rain-target-tempest-201058",
    "rain-target-tempest-203055",
    "rain-target-tempest-66270",
    "rain-target-tempest-34768",
    "rain-target-tempest-88159",
    "rain-target-tempest-126197",
    "rain-target-tempest-27140",
];
const LOCATION_IDS: [u64; 12] = [
    64_255, 225_947, 38_270, 168_853, 126_537, 201_058, 203_055, 66_270, 34_768, 88_159, 126_197,
    27_140,
];
const CATALOG_SHA256: &str = "ae9af10659350a555b48e135a61a04f3febb231b52aa5852b64c000016cee55d";
const RUN_CONTRACT_SHA256: &str =
    "3d07bcb9a56470110197ff549e5cf3d07efe369e07998718b14ca5e2a8d12542";

/// Exact ordered transaction-lock namespace shared with normal source ingestion.
pub const RAIN_FIXED_GAUGE_SOURCE_LOCK_SQL: &str = r#"
    SELECT pg_advisory_xact_lock(hashtextextended('weather-source:' || source_id, 0))
    FROM unnest($1::text[]) WITH ORDINALITY AS locked(source_id, ordinal)
    ORDER BY ordinal
"#;

/// Exact all-or-none row lookup used for inserts, retries, and later gap marking.
pub const RAIN_FIXED_GAUGE_EXISTING_ROWS_SQL: &str = r#"
    SELECT source_id::text AS "sourceId", content_hash AS "contentHash",
      valid_at AS "validAt"
    FROM weather_records
    WHERE source_id = ANY($1::bigint[]) AND source_kind = 'physical_sensor'
      AND product_run_at IS NULL AND valid_at = $2::timestamptz
    ORDER BY array_position($1::bigint[], source_id)
    FOR UPDATE
"#;

/// Exact provenance-run insert issued once per row in frozen station order.
pub const RAIN_FIXED_GAUGE_INSERT_RUN_SQL: &str = r#"
    INSERT INTO ingestion_runs (
      source_id, mode, requested_start, requested_end_exclusive,
      source_config_fingerprint, adapter_version, deadline_at, completed_at,
      state, attempts, record_count, request_metadata, response_metadata,
      upstream_response_checksum
    ) VALUES (
      $1, 'scheduled', $2, $3, $4, $5, clock_timestamp() + interval '1 minute',
      clock_timestamp(), 'succeeded', 1, 1, $6::jsonb, $7::jsonb, $8
    ) RETURNING id
"#;

/// Exact immutable target-row insert; the typed driver supplies its 32 ordered binds.
pub const RAIN_FIXED_GAUGE_INSERT_WEATHER_ROW_SQL: &str = r#"
    INSERT INTO weather_records (
      source_id, source_kind, valid_at, product_run_at,
      first_ingestion_run_id, last_ingestion_run_id,
      first_received_at, last_received_at, upstream_timezone, upstream_model,
      device_vendor, device_model, device_serial, quality_metadata, provider_metadata,
      temperature_c, apparent_temperature_c, precipitation_mm, wind_speed_mps,
      wind_gust_mps, pressure_hpa, relative_humidity_percent, cloud_cover_percent,
      wind_direction_degrees, black_globe_temperature_c,
      pm25_micrograms_per_cubic_meter, precipitation_rate_mm_per_hour,
      soil_electrical_conductivity_us_cm, soil_moisture_percent,
      solar_radiation_wm2, uv_index, wet_bulb_globe_temperature_c,
      water_level_m, content_hash
    ) VALUES (
      $1, $2, $3, $4, $5, $5, $6, $6, $7, $8, $9, $10, $11,
      $12::jsonb, $13::jsonb, $14, $15, $16, $17, $18, $19, $20, $21, $22,
      $23, $24, $25, $26, $27, $28, $29, $30, $31, $32
    ) ON CONFLICT ON CONSTRAINT weather_records_identity_key DO NOTHING
"#;

/// Exact single-element array binder used once per frozen station ordinal.
pub const RAIN_FIXED_GAUGE_BIND_REVISION_SQL: &str =
    "SELECT weather_bind_weather_record_revisions_v1($1::jsonb) AS value";

/// Catalog query with its immutable twelve source keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerRainFixedGaugeCatalogPlan {
    pub sql: &'static str,
    pub source_keys: [&'static str; 12],
}

/// Ordered advisory-lock statement for the catalog's resolved source IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRainFixedGaugeLockPlan<'a> {
    pub sql: &'static str,
    pub source_ids: Vec<&'a str>,
}

/// Existing row lookup in exact catalog source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRainFixedGaugeExistingRowsPlan<'a> {
    pub sql: &'static str,
    pub source_ids: Vec<&'a str>,
    pub valid_at: &'a str,
}

/// One locked stored row decoded from the exact retry query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRainFixedGaugeStoredRow {
    pub source_id: String,
    pub content_hash: String,
    pub valid_at: String,
}

/// Ordered bind domain for one provenance ingestion run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorkerRainFixedGaugeRunBind<'a> {
    Text(&'a str),
    Json(&'a str),
}

/// Exact eight-bind provenance-run insert.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkerRainFixedGaugeRunPlan<'a> {
    pub sql: &'static str,
    pub binds: [WorkerRainFixedGaugeRunBind<'a>; 8],
}

/// Ordered bind domain for one normalized weather row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorkerRainFixedGaugeWeatherBind<'a> {
    Text(&'a str),
    NullableText(Option<&'a str>),
    Json(&'a str),
    Number(Option<f64>),
}

/// Exact thirty-two-bind immutable target-row insert.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkerRainFixedGaugeWeatherPlan<'a> {
    pub sql: &'static str,
    pub binds: [WorkerRainFixedGaugeWeatherBind<'a>; 32],
}

/// One exact weather-record revision binder call.
#[derive(Debug, Clone, Copy)]
pub struct WorkerRainFixedGaugeBindPlan<'a> {
    pub sql: &'static str,
    pub pointers: &'a [AdjustmentWeatherRevisionPointer; 1],
}

/// Binder row retaining the supplied row identity beside the server receipt.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerRainFixedGaugeBoundRow {
    pub source_id: String,
    pub valid_at: String,
    pub stored_content_sha256: String,
    pub revision_receipt: AdjustmentRevisionCommitReceipt,
}

/// Exact named savepoint transitions in Node source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerRainFixedGaugeSavepointCommand {
    Create,
    RollbackTo,
    Release,
}

impl WorkerRainFixedGaugeSavepointCommand {
    /// Return the original fixed-gauge binder savepoint statement.
    pub const fn sql(self) -> &'static str {
        // preserve the source spelling for every transition
        match self {
            Self::Create => "SAVEPOINT rain_fixed_gauge_target_bind",
            Self::RollbackTo => "ROLLBACK TO SAVEPOINT rain_fixed_gauge_target_bind",
            Self::Release => "RELEASE SAVEPOINT rain_fixed_gauge_target_bind",
        }
    }
}

/// Retained transaction operations needed by the grouped writer.
pub trait WorkerRainFixedGaugeWriteSession {
    type Error;

    /// Begin the outer all-twelve-row transaction.
    fn begin(&mut self) -> Result<(), Self::Error>;
    /// Read and strictly project the fixed source catalog.
    fn read_source_catalog(
        &mut self,
        plan: WorkerRainFixedGaugeCatalogPlan,
    ) -> Result<RainFixedGaugeTargetSourceCatalog, Self::Error>;
    /// Lock every resolved source ID in catalog order.
    fn lock_sources(&mut self, plan: WorkerRainFixedGaugeLockPlan<'_>) -> Result<(), Self::Error>;
    /// Lock and decode zero or all exact target rows.
    fn read_existing_target_rows(
        &mut self,
        plan: WorkerRainFixedGaugeExistingRowsPlan<'_>,
    ) -> Result<Vec<WorkerRainFixedGaugeStoredRow>, Self::Error>;
    /// Insert one source-scoped succeeded ingestion run and return its ID row.
    fn insert_target_run(
        &mut self,
        plan: WorkerRainFixedGaugeRunPlan<'_>,
    ) -> Result<Vec<String>, Self::Error>;
    /// Insert one immutable normalized target row and return its affected row count.
    fn insert_target_weather_row(
        &mut self,
        plan: WorkerRainFixedGaugeWeatherPlan<'_>,
    ) -> Result<u64, Self::Error>;
    /// Execute one named savepoint transition.
    fn execute_savepoint(
        &mut self,
        command: WorkerRainFixedGaugeSavepointCommand,
    ) -> Result<(), Self::Error>;
    /// Bind one exact pointer array and decode its row identity plus receipt.
    fn bind_revision(
        &mut self,
        plan: WorkerRainFixedGaugeBindPlan<'_>,
    ) -> Result<Vec<WorkerRainFixedGaugeBoundRow>, Self::Error>;
    /// Execute the shared permanent-gap facade.
    fn write_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error>;
    /// Commit the complete group or complete gap marker set.
    fn commit(&mut self) -> Result<(), Self::Error>;
    /// Restore any failed outer transaction.
    fn rollback(&mut self) -> Result<(), Self::Error>;
    /// Return the retained client to its existing pool.
    fn release(&mut self);
}

/// Existing ingest pool that supplies one retained fixed-gauge transaction.
pub trait WorkerRainFixedGaugeWritePool {
    type Error;
    type Session: WorkerRainFixedGaugeWriteSession<Error = Self::Error>;

    /// Check out one client from the existing worker ingest pool.
    fn checkout(&mut self) -> Result<Self::Session, Self::Error>;
}

/// Approved canonical hash and clock admission used before storage mutations.
pub trait WorkerRainFixedGaugeWriteValues {
    type Error;

    /// Admit one exact closed UTC hour.
    fn is_closed_utc_hour(&mut self, value: &str) -> Result<bool, Self::Error>;
    /// Derive the exact canonical logical-key digest from catalog source order.
    fn logical_key_sha256(
        &mut self,
        source_ids: &[&str],
        valid_at: &str,
    ) -> Result<String, Self::Error>;
    /// Derive the exact Node weather-record content digest.
    fn weather_record_content_sha256(
        &mut self,
        record: &NormalizedWeatherRecord,
    ) -> Result<String, Self::Error>;
    /// Admit the exact Tempest target metadata for one frozen station.
    fn is_exact_target_record(
        &mut self,
        record: &NormalizedWeatherRecord,
        station_id: u64,
    ) -> Result<bool, Self::Error>;
    /// Format the exact preceding UTC hour used as the provenance request start.
    fn previous_utc_hour(&mut self, valid_at: &str) -> Result<String, Self::Error>;
    /// Encode the exact fixed-gauge run request metadata object.
    fn encode_run_request_metadata(
        &mut self,
        projection_identity_sha256: &str,
        run_contract_sha256: &str,
    ) -> Result<SerializedJson, Self::Error>;
    /// Encode the exact logical-received-at response metadata object.
    fn encode_run_response_metadata(
        &mut self,
        logical_received_at: &str,
    ) -> Result<SerializedJson, Self::Error>;
    /// Encode one nullable ordered provider or quality metadata object.
    fn encode_record_metadata(
        &mut self,
        metadata: &Nullable<Vec<(Vec<u16>, JsonValue)>>,
    ) -> Result<SerializedJson, Self::Error>;
}

/// Transaction, admission, codec, or recovery failure without partial success.
#[derive(Debug, PartialEq)]
pub enum WorkerRainFixedGaugeWriteError<DriverError, ValuesError> {
    Driver(DriverError),
    Values(ValuesError),
    Admission,
    Receipt,
    GapMarker,
    Recovery(DriverError),
}

/// Atomically persist and bind one complete twelve-gauge target group.
pub fn commit_worker_rain_fixed_gauge_revision_group<Pool, Values>(
    pool: &mut Pool,
    values: &mut Values,
    group: &RainFixedGaugeTargetRevisionGroup,
) -> Result<
    RainFixedGaugeTargetRevisionResult,
    WorkerRainFixedGaugeWriteError<Pool::Error, Values::Error>,
>
where
    Pool: WorkerRainFixedGaugeWritePool,
    Values: WorkerRainFixedGaugeWriteValues,
{
    use WorkerRainFixedGaugeWriteError::{Driver, Recovery};

    let mut session = pool.checkout().map_err(Driver)?;
    let result = (|| {
        session.begin().map_err(Driver)?;
        let catalog = read_and_lock_catalog(&mut session)?;
        validate_group(values, group, &catalog)?;
        ensure_target_rows(&mut session, values, group, &catalog)?;
        let reason = if nullable_text(&group.stageReceiptSha256).is_none() {
            AdjustmentRevisionGapReason::ArchiveStageFailed
        } else {
            AdjustmentRevisionGapReason::DatabaseBindFailed
        };
        let gap = revision_gap(group, reason);
        // retain complete unqualified rows without issuing ordinals when staging failed
        let outcome = if nullable_text(&group.stageReceiptSha256).is_none() {
            mark_group_gap(&mut session, group, &gap)?;
            gap_result(gap)
        } else {
            bind_group_or_mark_gap(&mut session, group, &gap)?
        };
        session.commit().map_err(Driver)?;
        Ok(outcome)
    })();
    finish_session(&mut session, result, Recovery)
}

/// Mark one already-committed group after archive publication or admission failure.
pub fn mark_worker_rain_fixed_gauge_revision_group_gap<Pool, Values>(
    pool: &mut Pool,
    values: &mut Values,
    group: &RainFixedGaugeTargetRevisionGroup,
    reason: AdjustmentRevisionGapReason,
) -> Result<AdjustmentRevisionGap, WorkerRainFixedGaugeWriteError<Pool::Error, Values::Error>>
where
    Pool: WorkerRainFixedGaugeWritePool,
    Values: WorkerRainFixedGaugeWriteValues,
{
    use WorkerRainFixedGaugeWriteError::{Admission, Driver, Recovery};

    // this entrypoint owns only the two post-commit permanent dispositions
    if !matches!(
        reason,
        AdjustmentRevisionGapReason::ArchivePublishFailed
            | AdjustmentRevisionGapReason::DatabaseAdmissionFailed
    ) {
        return Err(Admission);
    }
    let mut session = pool.checkout().map_err(Driver)?;
    let result = (|| {
        session.begin().map_err(Driver)?;
        let catalog = read_and_lock_catalog(&mut session)?;
        validate_group(values, group, &catalog)?;
        require_target_rows(&mut session, group, &catalog)?;
        let gap = revision_gap(group, reason);
        mark_group_gap(&mut session, group, &gap)?;
        session.commit().map_err(Driver)?;
        Ok(project_gap(gap))
    })();
    finish_session(&mut session, result, Recovery)
}

/// Read, validate minimally closed catalog identities, then acquire ordered source locks.
fn read_and_lock_catalog<Session, ValuesError>(
    session: &mut Session,
) -> Result<
    RainFixedGaugeTargetSourceCatalog,
    WorkerRainFixedGaugeWriteError<Session::Error, ValuesError>,
>
where
    Session: WorkerRainFixedGaugeWriteSession,
{
    use WorkerRainFixedGaugeWriteError::{Admission, Driver};

    let catalog = session
        .read_source_catalog(WorkerRainFixedGaugeCatalogPlan {
            sql: RAIN_FIXED_GAUGE_SOURCE_CATALOG_SQL,
            source_keys: SOURCE_KEYS,
        })
        .map_err(Driver)?;
    // require the exact projected catalog before acquiring any aliased lock set
    if catalog.catalogSha256 != CATALOG_SHA256
        || catalog.runContractSha256 != RUN_CONTRACT_SHA256
        || catalog.sources.len() != SOURCE_KEYS.len()
        || catalog.sources.iter().enumerate().any(|(index, source)| {
            source.locationId != LOCATION_IDS[index]
                || source.sourceKey != SOURCE_KEYS[index]
                || !valid_positive_decimal(&source.sourceId)
                || !valid_sha256(&source.sourceConfigFingerprint)
        })
    {
        return Err(Admission);
    }
    session
        .lock_sources(WorkerRainFixedGaugeLockPlan {
            sql: RAIN_FIXED_GAUGE_SOURCE_LOCK_SQL,
            source_ids: catalog
                .sources
                .iter()
                .map(|source| source.sourceId.as_str())
                .collect(),
        })
        .map_err(Driver)?;
    Ok(catalog)
}

/// Cross-bind the staged group to its exact catalog and content digests.
fn validate_group<DriverError, Values>(
    values: &mut Values,
    group: &RainFixedGaugeTargetRevisionGroup,
    catalog: &RainFixedGaugeTargetSourceCatalog,
) -> Result<(), WorkerRainFixedGaugeWriteError<DriverError, Values::Error>>
where
    Values: WorkerRainFixedGaugeWriteValues,
{
    use WorkerRainFixedGaugeWriteError::{Admission, Values as Codec};

    let source_ids = catalog
        .sources
        .iter()
        .map(|source| source.sourceId.as_str())
        .collect::<Vec<_>>();
    let expected_logical = values
        .logical_key_sha256(&source_ids, &group.validAt)
        .map_err(Codec)?;
    // reject partial groups and every shared projection or stage identity drift
    if !values.is_closed_utc_hour(&group.validAt).map_err(Codec)?
        || !valid_sha256(&group.logicalKeySha256)
        || group.logicalKeySha256 != expected_logical
        || !valid_sha256(&group.projectionIdentitySha256)
        || group.projectionSha256 != group.projectionIdentitySha256
        || nullable_text(&group.stageReceiptSha256).is_some_and(|value| !valid_sha256(value))
        || group.rows.len() != catalog.sources.len()
    {
        return Err(Admission);
    }
    // preserve frozen station order and bind every normalized row to one dedicated source
    for (index, row) in group.rows.iter().enumerate() {
        let source = &catalog.sources[index];
        let record = &row.normalizedRecord;
        let content_sha256 = values
            .weather_record_content_sha256(record)
            .map_err(Codec)?;
        let exact_target = values
            .is_exact_target_record(record, row.stationId)
            .map_err(Codec)?;
        if row.stationId != source.locationId
            || record.sourceId != source.sourceId
            || record.sourceKind != SourceKind::PhysicalSensor
            || nullable_text(&record.productRunAt).is_some()
            || record.validAt != group.validAt
            || !exact_target
            || !valid_sha256(&row.storedContentSha256)
            || content_sha256 != row.storedContentSha256
        {
            return Err(Admission);
        }
    }
    Ok(())
}

/// Insert every provenance run and row only at an empty all-row boundary.
fn ensure_target_rows<Session, Values>(
    session: &mut Session,
    values: &mut Values,
    group: &RainFixedGaugeTargetRevisionGroup,
    catalog: &RainFixedGaugeTargetSourceCatalog,
) -> Result<(), WorkerRainFixedGaugeWriteError<Session::Error, Values::Error>>
where
    Session: WorkerRainFixedGaugeWriteSession,
    Values: WorkerRainFixedGaugeWriteValues,
{
    use WorkerRainFixedGaugeWriteError::{Admission, Driver, Receipt, Values as Codec};

    // an exact complete retry owns no duplicate ingestion-run effects
    if exact_existing_rows(session, group, catalog, false)? {
        return Ok(());
    }
    let requested_start = values.previous_utc_hour(&group.validAt).map_err(Codec)?;
    let request_metadata = values
        .encode_run_request_metadata(&group.projectionIdentitySha256, RUN_CONTRACT_SHA256)
        .map_err(Codec)?;
    // persist one source-scoped provenance run and target row in fixed order
    for (index, row) in group.rows.iter().enumerate() {
        let source = catalog.sources.get(index).ok_or(Admission)?;
        let response_metadata = values
            .encode_run_response_metadata(&row.normalizedRecord.receivedAt)
            .map_err(Codec)?;
        let run_ids = session
            .insert_target_run(WorkerRainFixedGaugeRunPlan {
                sql: RAIN_FIXED_GAUGE_INSERT_RUN_SQL,
                binds: [
                    WorkerRainFixedGaugeRunBind::Text(&source.sourceId),
                    WorkerRainFixedGaugeRunBind::Text(&requested_start),
                    WorkerRainFixedGaugeRunBind::Text(&group.validAt),
                    WorkerRainFixedGaugeRunBind::Text(&source.sourceConfigFingerprint),
                    WorkerRainFixedGaugeRunBind::Text("rain-fixed-gauge-target/v1"),
                    WorkerRainFixedGaugeRunBind::Json(request_metadata.as_str()),
                    WorkerRainFixedGaugeRunBind::Json(response_metadata.as_str()),
                    WorkerRainFixedGaugeRunBind::Text(&group.projectionIdentitySha256),
                ],
            })
            .map_err(Driver)?;
        let [run_id]: [String; 1] = run_ids.try_into().map_err(|_| Receipt)?;
        let quality = values
            .encode_record_metadata(&row.normalizedRecord.metadata.quality)
            .map_err(Codec)?;
        let provider = values
            .encode_record_metadata(&row.normalizedRecord.metadata.provider)
            .map_err(Codec)?;
        let plan = weather_row_plan(row, &run_id, &quality, &provider);
        // reject a concurrent or partial identity collision
        if session.insert_target_weather_row(plan).map_err(Driver)? != 1 {
            return Err(Admission);
        }
    }
    Ok(())
}

/// Require the already-committed exact all-row set before terminal gap marking.
fn require_target_rows<Session, ValuesError>(
    session: &mut Session,
    group: &RainFixedGaugeTargetRevisionGroup,
    catalog: &RainFixedGaugeTargetSourceCatalog,
) -> Result<(), WorkerRainFixedGaugeWriteError<Session::Error, ValuesError>>
where
    Session: WorkerRainFixedGaugeWriteSession,
{
    exact_existing_rows(session, group, catalog, true).map(|_| ())
}

/// Accept only an empty insert boundary or one complete exact retry set.
fn exact_existing_rows<Session, ValuesError>(
    session: &mut Session,
    group: &RainFixedGaugeTargetRevisionGroup,
    catalog: &RainFixedGaugeTargetSourceCatalog,
    require_complete: bool,
) -> Result<bool, WorkerRainFixedGaugeWriteError<Session::Error, ValuesError>>
where
    Session: WorkerRainFixedGaugeWriteSession,
{
    use WorkerRainFixedGaugeWriteError::{Admission, Driver};

    let source_ids = catalog
        .sources
        .iter()
        .map(|source| source.sourceId.as_str())
        .collect::<Vec<_>>();
    let existing = session
        .read_existing_target_rows(WorkerRainFixedGaugeExistingRowsPlan {
            sql: RAIN_FIXED_GAUGE_EXISTING_ROWS_SQL,
            source_ids,
            valid_at: &group.validAt,
        })
        .map_err(Driver)?;
    // preserve the only admissible empty boundary for first insertion
    if existing.is_empty() && !require_complete {
        return Ok(false);
    }
    if existing.len() != group.rows.len()
        || existing.iter().enumerate().any(|(index, stored)| {
            stored.source_id != group.rows[index].normalizedRecord.sourceId
                || stored.content_hash != group.rows[index].storedContentSha256
                || stored.valid_at != group.validAt
        })
    {
        return Err(Admission);
    }
    Ok(true)
}

/// Build the exact thirty-two PostgreSQL binds for one normalized target row.
fn weather_row_plan<'a>(
    row: &'a weather_contracts::RainFixedGaugeTargetRevisionRow,
    run_id: &'a str,
    quality: &'a SerializedJson,
    provider: &'a SerializedJson,
) -> WorkerRainFixedGaugeWeatherPlan<'a> {
    use WorkerRainFixedGaugeWeatherBind::{Json, NullableText, Number, Text};

    let record = &row.normalizedRecord;
    let device = nullable_value(&record.metadata.device);
    WorkerRainFixedGaugeWeatherPlan {
        sql: RAIN_FIXED_GAUGE_INSERT_WEATHER_ROW_SQL,
        binds: [
            Text(&record.sourceId),
            Text("physical_sensor"),
            Text(&record.validAt),
            NullableText(nullable_text(&record.productRunAt)),
            Text(run_id),
            Text(&record.receivedAt),
            Text(&record.metadata.upstreamTimezone),
            NullableText(nullable_text(&record.metadata.model)),
            NullableText(device.and_then(|value| presence_text(&value.vendor))),
            NullableText(device.and_then(|value| presence_text(&value.model))),
            NullableText(device.and_then(|value| presence_text(&value.serial))),
            Json(quality.as_str()),
            Json(provider.as_str()),
            Number(nullable_number(&record.metrics.temperatureC)),
            Number(nullable_number(&record.metrics.apparentTemperatureC)),
            Number(nullable_number(&record.metrics.precipitationMm)),
            Number(nullable_number(&record.metrics.windSpeedMps)),
            Number(nullable_number(&record.metrics.windGustMps)),
            Number(nullable_number(&record.metrics.pressureHpa)),
            Number(nullable_number(&record.metrics.relativeHumidityPercent)),
            Number(nullable_number(&record.metrics.cloudCoverPercent)),
            Number(nullable_number(&record.metrics.windDirectionDegrees)),
            Number(nullable_number(&record.metrics.blackGlobeTemperatureC)),
            Number(nullable_number(&record.metrics.pm25MicrogramsPerCubicMeter)),
            Number(nullable_number(&record.metrics.precipitationRateMmPerHour)),
            Number(nullable_number(
                &record.metrics.soilElectricalConductivityMicrosiemensPerCm,
            )),
            Number(nullable_number(&record.metrics.soilMoisturePercent)),
            Number(nullable_number(&record.metrics.solarRadiationWm2)),
            Number(nullable_number(&record.metrics.uvIndex)),
            Number(nullable_number(&record.metrics.wetBulbGlobeTemperatureC)),
            Number(nullable_number(&record.metrics.waterLevelM)),
            Text(&row.storedContentSha256),
        ],
    }
}

/// Bind every station ordinal or recover the savepoint and persist one group gap.
fn bind_group_or_mark_gap<Session, ValuesError>(
    session: &mut Session,
    group: &RainFixedGaugeTargetRevisionGroup,
    gap: &AdjustmentRevisionGapInput,
) -> Result<
    RainFixedGaugeTargetRevisionResult,
    WorkerRainFixedGaugeWriteError<Session::Error, ValuesError>,
>
where
    Session: WorkerRainFixedGaugeWriteSession,
{
    use WorkerRainFixedGaugeWriteError::{Driver, Recovery};

    session
        .execute_savepoint(WorkerRainFixedGaugeSavepointCommand::Create)
        .map_err(Driver)?;
    let attempted = bind_group::<Session, ValuesError>(session, group).and_then(|receipts| {
        session
            .execute_savepoint(WorkerRainFixedGaugeSavepointCommand::Release)
            .map_err(Driver)?;
        Ok(receipts)
    });
    match attempted {
        Ok(receipts) => Ok(RainFixedGaugeTargetRevisionResult::Committed(
            RainFixedGaugeTargetRevisionResultCommitted {
                state: RainFixedGaugeTargetRevisionResultCommittedStateLiteral,
                revisionReceipts: receipts,
            },
        )),
        Err(_) => {
            session
                .execute_savepoint(WorkerRainFixedGaugeSavepointCommand::RollbackTo)
                .map_err(Recovery)?;
            session
                .execute_savepoint(WorkerRainFixedGaugeSavepointCommand::Release)
                .map_err(Recovery)?;
            mark_group_gap(session, group, gap)?;
            Ok(gap_result(gap.clone()))
        }
    }
}

/// Issue one single-element batch binder in frozen row order.
fn bind_group<Session, ValuesError>(
    session: &mut Session,
    group: &RainFixedGaugeTargetRevisionGroup,
) -> Result<
    Vec<AdjustmentRevisionCommitReceipt>,
    WorkerRainFixedGaugeWriteError<Session::Error, ValuesError>,
>
where
    Session: WorkerRainFixedGaugeWriteSession,
{
    use WorkerRainFixedGaugeWriteError::{Driver, Receipt};

    let stage_receipt_sha256 = nullable_text(&group.stageReceiptSha256).ok_or(Receipt)?;
    let mut receipts = Vec::with_capacity(group.rows.len());
    // preserve station order instead of source-ID lexical order
    for row in &group.rows {
        let pointers = [AdjustmentWeatherRevisionPointer {
            productRunAt: Nullable::Null,
            projectionIdentitySha256: group.projectionIdentitySha256.clone(),
            projectionSha256: group.projectionSha256.clone(),
            sourceId: row.normalizedRecord.sourceId.clone(),
            sourceKind: AdjustmentWeatherRevisionPointerSourceKindEnum::PhysicalSensor,
            stageReceiptSha256: stage_receipt_sha256.to_owned(),
            storedContentSha256: row.storedContentSha256.clone(),
            validAt: group.validAt.clone(),
        }];
        let bound = session
            .bind_revision(WorkerRainFixedGaugeBindPlan {
                sql: RAIN_FIXED_GAUGE_BIND_REVISION_SQL,
                pointers: &pointers,
            })
            .map_err(Driver)?;
        let [bound]: [WorkerRainFixedGaugeBoundRow; 1] = bound.try_into().map_err(|_| Receipt)?;
        // require the binder to return the exact stored row and target receipt
        if bound.source_id != row.normalizedRecord.sourceId
            || bound.valid_at != group.validAt
            || bound.stored_content_sha256 != row.storedContentSha256
            || bound.revision_receipt.projectionKind
                != AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision
        {
            return Err(Receipt);
        }
        receipts.push(bound.revision_receipt);
    }
    Ok(receipts)
}

/// Mark every exact row with one body-level terminal gap.
fn mark_group_gap<Session, ValuesError>(
    session: &mut Session,
    group: &RainFixedGaugeTargetRevisionGroup,
    gap: &AdjustmentRevisionGapInput,
) -> Result<(), WorkerRainFixedGaugeWriteError<Session::Error, ValuesError>>
where
    Session: WorkerRainFixedGaugeWriteSession,
{
    use WorkerRainFixedGaugeWriteError::{Driver, GapMarker};

    // persist all twelve markers inside the surrounding transaction
    for row in &group.rows {
        let key =
            AdjustmentRevisionAdmissionKey::WeatherRecord(AdjustmentWeatherRevisionAdmissionKey {
                productRunAt: Nullable::Null,
                sourceId: row.normalizedRecord.sourceId.clone(),
                sourceKind: AdjustmentWeatherRevisionPointerSourceKindEnum::PhysicalSensor,
                storedContentSha256: row.storedContentSha256.clone(),
                validAt: group.validAt.clone(),
            });
        let markers = session
            .write_revision_gap(adjustment_revision_gap_plan(&key, gap))
            .map_err(Driver)?;
        let [marker]: [AdjustmentRevisionGapMarker; 1] =
            markers.try_into().map_err(|_| GapMarker)?;
        // require the shared facade to retain the group and row identities exactly
        if marker.logical_key_sha256 != gap.logicalKeySha256
            || marker.projection_identity_sha256 != gap.projectionIdentitySha256
            || marker.projection_kind != gap.projectionKind
            || marker.projection_sha256 != gap.projectionSha256
            || marker.reason != gap.reason
            || marker.stored_content_sha256 != row.storedContentSha256
        {
            return Err(GapMarker);
        }
    }
    Ok(())
}

/// Build the shared gap input without claiming a revision receipt.
fn revision_gap(
    group: &RainFixedGaugeTargetRevisionGroup,
    reason: AdjustmentRevisionGapReason,
) -> AdjustmentRevisionGapInput {
    AdjustmentRevisionGapInput {
        logicalKeySha256: group.logicalKeySha256.clone(),
        projectionIdentitySha256: Nullable::Value(group.projectionIdentitySha256.clone()),
        projectionKind: AdjustmentRevisionProjectionKind::TargetRevision,
        projectionSha256: Nullable::Value(group.projectionSha256.clone()),
        reason,
    }
}

/// Project one generated gap result from the shared input.
fn gap_result(gap: AdjustmentRevisionGapInput) -> RainFixedGaugeTargetRevisionResult {
    RainFixedGaugeTargetRevisionResult::Gap(RainFixedGaugeTargetRevisionResultGap {
        state: RainFixedGaugeTargetRevisionResultGapStateLiteral,
        gap: project_gap(gap),
    })
}

/// Project one generated public gap from the shared database input.
fn project_gap(gap: AdjustmentRevisionGapInput) -> AdjustmentRevisionGap {
    AdjustmentRevisionGap {
        logicalKeySha256: gap.logicalKeySha256,
        projectionIdentitySha256: gap.projectionIdentitySha256,
        projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision,
        projectionSha256: gap.projectionSha256,
        reason: gap_reason(gap.reason),
    }
}

/// Convert the shared input reason to the generated response reason.
const fn gap_reason(reason: AdjustmentRevisionGapReason) -> AdjustmentRevisionGapReasonEnum {
    // preserve every original database disposition one-for-one
    match reason {
        AdjustmentRevisionGapReason::ArchiveStageFailed => {
            AdjustmentRevisionGapReasonEnum::ArchiveStageFailed
        }
        AdjustmentRevisionGapReason::DatabaseBindFailed => {
            AdjustmentRevisionGapReasonEnum::DatabaseBindFailed
        }
        AdjustmentRevisionGapReason::DatabaseAdmissionFailed => {
            AdjustmentRevisionGapReasonEnum::DatabaseAdmissionFailed
        }
        AdjustmentRevisionGapReason::ArchivePublishFailed => {
            AdjustmentRevisionGapReasonEnum::ArchivePublishFailed
        }
    }
}

/// Commit success or restore failure, then release the client exactly once.
fn finish_session<Session, ResultValue, ValuesError, RecoveryMap>(
    session: &mut Session,
    result: Result<ResultValue, WorkerRainFixedGaugeWriteError<Session::Error, ValuesError>>,
    recovery: RecoveryMap,
) -> Result<ResultValue, WorkerRainFixedGaugeWriteError<Session::Error, ValuesError>>
where
    Session: WorkerRainFixedGaugeWriteSession,
    RecoveryMap:
        FnOnce(Session::Error) -> WorkerRainFixedGaugeWriteError<Session::Error, ValuesError>,
{
    // recover only failed outer transactions before returning the retained session
    let result = match result {
        Ok(value) => Ok(value),
        Err(error) => match session.rollback() {
            Ok(()) => Err(error),
            Err(rollback) => Err(recovery(rollback)),
        },
    };
    session.release();
    result
}

/// Borrow one generated nullable string.
fn nullable_text(value: &Nullable<String>) -> Option<&str> {
    match value {
        Nullable::Value(value) => Some(value),
        Nullable::Null => None,
    }
}

/// Borrow one generated nullable value.
fn nullable_value<T>(value: &Nullable<T>) -> Option<&T> {
    match value {
        Nullable::Value(value) => Some(value),
        Nullable::Null => None,
    }
}

/// Copy one generated nullable metric into a driver bind.
fn nullable_number(value: &Nullable<f64>) -> Option<f64> {
    match value {
        Nullable::Value(value) => Some(*value),
        Nullable::Null => None,
    }
}

/// Borrow one optional device identity field.
fn presence_text(value: &FieldPresence<String>) -> Option<&str> {
    match value {
        FieldPresence::Value(value) => Some(value),
        FieldPresence::Missing => None,
    }
}

/// Recognize one lowercase hexadecimal SHA-256 identity.
fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Recognize one positive PostgreSQL bigint spelling.
fn valid_positive_decimal(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes()[0] != b'0'
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
#[path = "worker_rain_fixed_gauge_write_test.rs"]
mod tests;
