//! Worker-only reads for one bounded fixed-gauge target source group.

use weather_contracts::{
    RainFixedGaugeTargetCapture, RainFixedGaugeTargetSource, RainFixedGaugeTargetSourceCatalog,
    RainFixedGaugeTargetSourceCatalogContractVersionLiteral,
};

use crate::WorkerRainAdjustmentReadValues;

const MAXIMUM_CAPTURE_BODY_BYTES: usize = 2_000_000;
const MAXIMUM_AGGREGATE_BODY_BYTES: usize = 12 * MAXIMUM_CAPTURE_BODY_BYTES;
const CATALOG_SHA256: &str = "ae9af10659350a555b48e135a61a04f3febb231b52aa5852b64c000016cee55d";
const RUN_CONTRACT_SHA256: &str =
    "3d07bcb9a56470110197ff549e5cf3d07efe369e07998718b14ca5e2a8d12542";

/// Exact immutable source catalog query in frozen source-key order.
pub const RAIN_FIXED_GAUGE_SOURCE_CATALOG_SQL: &str = r#"
    SELECT source.id::text AS "sourceId", source.source_key AS "sourceKey",
      source.source_kind AS "sourceKind",
      source.material_provider_config AS "adapterConfig",
      source.source_config_fingerprint::text AS "sourceConfigFingerprint",
      source.capabilities, source.cadence_seconds AS "cadenceSeconds", source.active,
      station.slug AS "stationKey", station.station_kind AS "stationKind",
      station.latitude, station.longitude, station.serial,
      station.active AS "stationActive", provider.provider_key AS "providerKey",
      provider.active AS "providerActive", site.slug AS "siteKey",
      site.timezone, site.active AS "siteActive"
    FROM sources source
    JOIN stations station ON station.id = source.station_id
    JOIN providers provider ON provider.id = source.provider_id
    JOIN sites site ON site.id = station.site_id
    WHERE source.source_key = ANY($1::text[])
    ORDER BY array_position($1::text[], source.source_key)
"#;

/// Exact latest complete post-epoch hour query without inflating provider bodies.
pub const PENDING_RAIN_FIXED_GAUGE_HOUR_SQL: &str = r#"
    SELECT c.window_end_exclusive - interval '1 second' AS "validAt"
    FROM rain_capture_claims c
    JOIN rain_capture_receipts r ON r.claim_id = c.id
    WHERE c.kind = 'station' AND c.station_id = ANY($1::integer[])
      AND r.outcome = 'valid' AND c.window_start >= $2::timestamptz
      AND r.completed_at >= $2::timestamptz
      AND c.window_end_exclusive <= clock_timestamp()
      AND NOT EXISTS (
        SELECT 1 FROM weather_records wr
        JOIN sources source ON source.id = wr.source_id
        WHERE source.source_key = ANY($3::text[])
          AND wr.source_kind = 'physical_sensor'
          AND wr.product_run_at IS NULL
          AND wr.valid_at = c.window_end_exclusive - interval '1 second'
      )
    GROUP BY c.window_end_exclusive
    HAVING count(*) = $4 AND count(DISTINCT c.station_id) = $4
    ORDER BY c.window_end_exclusive DESC
    LIMIT 1
"#;

/// Exact second pending check performed after the external permanent-gap lookup.
pub const RAIN_FIXED_GAUGE_HOUR_ELIGIBILITY_SQL: &str = r#"
    SELECT count(*) = $4 AND count(DISTINCT c.station_id) = $4 AND NOT EXISTS (
      SELECT 1 FROM weather_records wr
      JOIN sources source ON source.id = wr.source_id
      WHERE source.source_key = ANY($3::text[])
        AND wr.source_kind = 'physical_sensor' AND wr.product_run_at IS NULL
        AND wr.valid_at = $2::timestamptz
    ) AS eligible
    FROM rain_capture_claims c
    JOIN rain_capture_receipts r ON r.claim_id = c.id
    WHERE c.kind = 'station' AND c.station_id = ANY($1::integer[])
      AND r.outcome = 'valid' AND c.window_start >= $5::timestamptz
      AND r.completed_at >= $5::timestamptz
      AND c.window_end_exclusive = $2::timestamptz + interval '1 second'
      AND c.window_end_exclusive <= clock_timestamp()
"#;

/// Exact ordered compressed capture query for one re-admitted logical hour.
pub const RAIN_FIXED_GAUGE_HOUR_CAPTURES_SQL: &str = r#"
    SELECT r.body_sha256 AS "bodySha256", c.id::text AS "claimId",
      r.completed_at AS "completedAt", r.compressed_body AS "compressedBody",
      c.station_id AS "stationId",
      c.window_end_exclusive - interval '1 second' AS "validAt",
      c.window_end_exclusive AS "windowEndExclusive", c.window_start AS "windowStart"
    FROM rain_capture_claims c
    JOIN rain_capture_receipts r ON r.claim_id = c.id
    WHERE c.kind = 'station' AND c.station_id = ANY($1::integer[])
      AND r.outcome = 'valid'
      AND c.window_end_exclusive = $2::timestamptz + interval '1 second'
      AND c.window_start >= $3::timestamptz AND r.completed_at >= $3::timestamptz
    ORDER BY array_position($1::integer[], c.station_id)
"#;

/// Closed scalar and array binds used by the four fixed-gauge read statements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerRainFixedGaugeReadBind {
    StationIds(Vec<u64>),
    SourceKeys(Vec<String>),
    UtcInstant(String),
    Unsigned(u64),
}

/// Exact one-bind source-catalog query plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RainFixedGaugeSourceCatalogReadPlan {
    pub sql: &'static str,
    pub binds: [WorkerRainFixedGaugeReadBind; 1],
}

/// Exact station, epoch, source, and count pending-hour query plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRainFixedGaugeHourReadPlan {
    pub sql: &'static str,
    pub binds: [WorkerRainFixedGaugeReadBind; 4],
}

/// Exact recheck plan after an external permanent-gap lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RainFixedGaugeHourEligibilityReadPlan {
    pub sql: &'static str,
    pub binds: [WorkerRainFixedGaugeReadBind; 5],
}

/// Exact ordered raw capture query plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RainFixedGaugeHourCapturesReadPlan {
    pub sql: &'static str,
    pub binds: [WorkerRainFixedGaugeReadBind; 3],
}

/// One JSON scalar retained by the approved database adapter for closed config validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RainFixedGaugeAdapterConfigValue {
    Text(String),
    Unsigned(u64),
}

/// One source-catalog row with JSON objects decoded but not reserialized locally.
#[derive(Debug, Clone, PartialEq)]
pub struct RainFixedGaugeSourceReadRow {
    pub source_id: String,
    pub source_key: String,
    pub source_kind: String,
    pub adapter_config: Vec<(String, RainFixedGaugeAdapterConfigValue)>,
    pub source_config_fingerprint: String,
    pub capabilities: Vec<String>,
    pub cadence_seconds: Option<u64>,
    pub active: bool,
    pub station_key: String,
    pub station_kind: String,
    pub latitude: f64,
    pub longitude: f64,
    pub serial: Option<String>,
    pub station_active: bool,
    pub provider_key: String,
    pub provider_active: bool,
    pub site_key: String,
    pub timezone: String,
    pub site_active: bool,
}

/// One server-clock-observed pending hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingRainFixedGaugeHourReadRow {
    pub valid_at_ms: i64,
}

/// One ordered compressed station capture row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RainFixedGaugeCaptureReadRow {
    pub body_sha256: String,
    pub claim_id: String,
    pub completed_at_ms: i64,
    pub compressed_body: Vec<u8>,
    pub station_id: u64,
    pub valid_at_ms: i64,
    pub window_end_exclusive_ms: i64,
    pub window_start_ms: i64,
}

/// Database boundary for the exact catalog, pending, recheck, and capture reads.
pub trait WorkerRainFixedGaugeReadPort {
    type Error;

    /// Read the immutable source catalog in source-key order.
    fn select_fixed_gauge_source_catalog(
        &mut self,
        plan: &RainFixedGaugeSourceCatalogReadPlan,
    ) -> Result<Vec<RainFixedGaugeSourceReadRow>, Self::Error>;

    /// Select one latest complete hour without reading raw bodies.
    fn select_pending_fixed_gauge_hour(
        &mut self,
        plan: &PendingRainFixedGaugeHourReadPlan,
    ) -> Result<Option<PendingRainFixedGaugeHourReadRow>, Self::Error>;

    /// Recheck the selected hour after external gap admission.
    fn select_fixed_gauge_hour_eligibility(
        &mut self,
        plan: &RainFixedGaugeHourEligibilityReadPlan,
    ) -> Result<Option<bool>, Self::Error>;

    /// Read the exact ordered compressed bodies only after the recheck succeeds.
    fn select_fixed_gauge_hour_captures(
        &mut self,
        plan: &RainFixedGaugeHourCapturesReadPlan,
    ) -> Result<Vec<RainFixedGaugeCaptureReadRow>, Self::Error>;
}

/// Fixed-gauge UTC admission added to the existing gzip, clock, and SHA owner.
pub trait WorkerRainFixedGaugeReadValues: WorkerRainAdjustmentReadValues {
    /// Parse only an exact canonical UTC millisecond representation.
    fn parse_canonical_utc_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error>;
}

/// Local refusal before returning mutable catalog or raw provider material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerRainFixedGaugeReadAdmissionError {
    SourceCatalogIncomplete,
    SourceCatalogMismatch,
    HourNoLongerPending,
    CaptureSetIncomplete,
    CaptureOrderMismatch,
    CaptureBodyTooLarge,
    AggregateBodyTooLarge,
    CaptureBodySha256Mismatch,
}

/// Complete fixed-gauge read failure with owning boundaries retained.
#[derive(Debug, PartialEq, Eq)]
pub enum WorkerRainFixedGaugeReadError<PortError, ValuesError> {
    Admission(WorkerRainFixedGaugeReadAdmissionError),
    Port(PortError),
    Values(ValuesError),
}

/// Read and validate the complete immutable source catalog in frozen station order.
pub fn read_rain_fixed_gauge_target_source_catalog<Port>(
    port: &mut Port,
) -> Result<RainFixedGaugeTargetSourceCatalog, WorkerRainFixedGaugeReadError<Port::Error, ()>>
where
    Port: WorkerRainFixedGaugeReadPort,
{
    let source_keys = source_keys();
    let rows = port
        .select_fixed_gauge_source_catalog(&RainFixedGaugeSourceCatalogReadPlan {
            sql: RAIN_FIXED_GAUGE_SOURCE_CATALOG_SQL,
            binds: [WorkerRainFixedGaugeReadBind::SourceKeys(source_keys)],
        })
        .map_err(WorkerRainFixedGaugeReadError::Port)?;
    // require one immutable source row for every frozen gauge
    if rows.len() != SOURCE_SPECS.len() {
        return Err(admission(
            WorkerRainFixedGaugeReadAdmissionError::SourceCatalogIncomplete,
        ));
    }
    let mut sources = Vec::with_capacity(rows.len());
    // preserve fixed source order while rejecting every mutable catalog field
    for (row, expected) in rows.into_iter().zip(SOURCE_SPECS) {
        if !source_row_matches(&row, expected) {
            return Err(admission(
                WorkerRainFixedGaugeReadAdmissionError::SourceCatalogMismatch,
            ));
        }
        sources.push(RainFixedGaugeTargetSource {
            locationId: expected.location_id,
            sourceConfigFingerprint: row.source_config_fingerprint,
            sourceId: row.source_id,
            sourceKey: expected.source_key.to_owned(),
        });
    }
    Ok(RainFixedGaugeTargetSourceCatalog {
        catalogSha256: CATALOG_SHA256.to_owned(),
        contractVersion: RainFixedGaugeTargetSourceCatalogContractVersionLiteral,
        runContractSha256: RUN_CONTRACT_SHA256.to_owned(),
        sources,
    })
}

/// Select one latest complete hour while leaving all compressed bodies unread.
pub fn read_pending_rain_fixed_gauge_target_hour<Port, Values>(
    port: &mut Port,
    values: &mut Values,
    capture_epoch_at: &str,
) -> Result<Option<String>, WorkerRainFixedGaugeReadError<Port::Error, Values::Error>>
where
    Port: WorkerRainFixedGaugeReadPort,
    Values: WorkerRainFixedGaugeReadValues,
{
    values
        .parse_canonical_utc_milliseconds(capture_epoch_at)
        .map_err(WorkerRainFixedGaugeReadError::Values)?;
    let row = port
        .select_pending_fixed_gauge_hour(&PendingRainFixedGaugeHourReadPlan {
            sql: PENDING_RAIN_FIXED_GAUGE_HOUR_SQL,
            binds: [
                WorkerRainFixedGaugeReadBind::StationIds(station_ids()),
                WorkerRainFixedGaugeReadBind::UtcInstant(capture_epoch_at.to_owned()),
                WorkerRainFixedGaugeReadBind::SourceKeys(source_keys()),
                WorkerRainFixedGaugeReadBind::Unsigned(SOURCE_SPECS.len() as u64),
            ],
        })
        .map_err(WorkerRainFixedGaugeReadError::Port)?;
    row.map(|row| {
        values
            .format_utc_milliseconds(row.valid_at_ms)
            .map_err(WorkerRainFixedGaugeReadError::Values)
    })
    .transpose()
}

/// Recheck and inflate one exact ordered hour only after external gap admission.
pub fn read_rain_fixed_gauge_target_captures_for_hour<Port, Values>(
    port: &mut Port,
    values: &mut Values,
    capture_epoch_at: &str,
    target_valid_at: &str,
) -> Result<
    Vec<RainFixedGaugeTargetCapture>,
    WorkerRainFixedGaugeReadError<Port::Error, Values::Error>,
>
where
    Port: WorkerRainFixedGaugeReadPort,
    Values: WorkerRainFixedGaugeReadValues,
{
    values
        .parse_canonical_utc_milliseconds(capture_epoch_at)
        .map_err(WorkerRainFixedGaugeReadError::Values)?;
    let valid_at_ms = values
        .parse_canonical_utc_milliseconds(target_valid_at)
        .map_err(WorkerRainFixedGaugeReadError::Values)?;
    let eligible = port
        .select_fixed_gauge_hour_eligibility(&RainFixedGaugeHourEligibilityReadPlan {
            sql: RAIN_FIXED_GAUGE_HOUR_ELIGIBILITY_SQL,
            binds: [
                WorkerRainFixedGaugeReadBind::StationIds(station_ids()),
                WorkerRainFixedGaugeReadBind::UtcInstant(target_valid_at.to_owned()),
                WorkerRainFixedGaugeReadBind::SourceKeys(source_keys()),
                WorkerRainFixedGaugeReadBind::Unsigned(SOURCE_SPECS.len() as u64),
                WorkerRainFixedGaugeReadBind::UtcInstant(capture_epoch_at.to_owned()),
            ],
        })
        .map_err(WorkerRainFixedGaugeReadError::Port)?;
    // refuse raced, completed, and incomplete hours before reading raw values
    if eligible != Some(true) {
        return Err(admission(
            WorkerRainFixedGaugeReadAdmissionError::HourNoLongerPending,
        ));
    }
    let rows = port
        .select_fixed_gauge_hour_captures(&RainFixedGaugeHourCapturesReadPlan {
            sql: RAIN_FIXED_GAUGE_HOUR_CAPTURES_SQL,
            binds: [
                WorkerRainFixedGaugeReadBind::StationIds(station_ids()),
                WorkerRainFixedGaugeReadBind::UtcInstant(target_valid_at.to_owned()),
                WorkerRainFixedGaugeReadBind::UtcInstant(capture_epoch_at.to_owned()),
            ],
        })
        .map_err(WorkerRainFixedGaugeReadError::Port)?;
    // require exactly one immutable source response per frozen gauge
    if rows.len() != SOURCE_SPECS.len() {
        return Err(admission(
            WorkerRainFixedGaugeReadAdmissionError::CaptureSetIncomplete,
        ));
    }

    let mut decoded_bytes = 0_usize;
    let mut captures = Vec::with_capacity(rows.len());
    // retain database station order while bounding and authenticating every source body
    for (row, expected) in rows.into_iter().zip(SOURCE_SPECS) {
        if row.station_id != expected.location_id || row.valid_at_ms != valid_at_ms {
            return Err(admission(
                WorkerRainFixedGaugeReadAdmissionError::CaptureOrderMismatch,
            ));
        }
        let body = values
            .gunzip_bounded(&row.compressed_body, MAXIMUM_CAPTURE_BODY_BYTES)
            .map_err(WorkerRainFixedGaugeReadError::Values)?;
        // contain a codec owner that returns more than the requested body bound
        if body.len() > MAXIMUM_CAPTURE_BODY_BYTES {
            return Err(admission(
                WorkerRainFixedGaugeReadAdmissionError::CaptureBodyTooLarge,
            ));
        }
        decoded_bytes = decoded_bytes.checked_add(body.len()).ok_or(admission(
            WorkerRainFixedGaugeReadAdmissionError::AggregateBodyTooLarge,
        ))?;
        // bound the complete twelve-body proof set before returning any captures
        if decoded_bytes > MAXIMUM_AGGREGATE_BODY_BYTES {
            return Err(admission(
                WorkerRainFixedGaugeReadAdmissionError::AggregateBodyTooLarge,
            ));
        }
        let digest = values
            .sha256(&body)
            .map_err(WorkerRainFixedGaugeReadError::Values)?;
        // reject stored-body substitution before provider parsing
        if digest.as_str() != row.body_sha256 {
            return Err(admission(
                WorkerRainFixedGaugeReadAdmissionError::CaptureBodySha256Mismatch,
            ));
        }
        captures.push(project_capture(row, body, values)?);
    }
    Ok(captures)
}

/// Project one verified row into the generated fixed-gauge capture contract.
fn project_capture<PortError, Values>(
    row: RainFixedGaugeCaptureReadRow,
    body: Vec<u8>,
    values: &mut Values,
) -> Result<RainFixedGaugeTargetCapture, WorkerRainFixedGaugeReadError<PortError, Values::Error>>
where
    Values: WorkerRainFixedGaugeReadValues,
{
    Ok(RainFixedGaugeTargetCapture {
        body,
        bodySha256: row.body_sha256,
        claimId: row.claim_id,
        completedAt: format_utc(row.completed_at_ms, values)?,
        stationId: row.station_id,
        windowEndExclusive: format_utc(row.window_end_exclusive_ms, values)?,
        windowStart: format_utc(row.window_start_ms, values)?,
    })
}

/// Format one observed database clock through the approved UTC owner.
fn format_utc<PortError, Values>(
    epoch_milliseconds: i64,
    values: &mut Values,
) -> Result<String, WorkerRainFixedGaugeReadError<PortError, Values::Error>>
where
    Values: WorkerRainFixedGaugeReadValues,
{
    values
        .format_utc_milliseconds(epoch_milliseconds)
        .map_err(WorkerRainFixedGaugeReadError::Values)
}

/// Build the frozen station-id bind in original catalog order.
fn station_ids() -> Vec<u64> {
    SOURCE_SPECS
        .iter()
        .map(|source| source.location_id)
        .collect()
}

/// Build the frozen source-key bind in original catalog order.
fn source_keys() -> Vec<String> {
    SOURCE_SPECS
        .iter()
        .map(|source| source.source_key.to_owned())
        .collect()
}

/// Require one complete source row to match its immutable catalog specification.
fn source_row_matches(row: &RainFixedGaugeSourceReadRow, expected: &SourceSpec) -> bool {
    row.source_key == expected.source_key
        && row.station_key == expected.source_key
        && row.source_kind == "physical_sensor"
        && row.provider_key == "weatherflow-tempest"
        && row.site_key == "ballydidean"
        && row.station_kind == "physical"
        && row.timezone == "America/Los_Angeles"
        && row.latitude == expected.latitude
        && row.longitude == expected.longitude
        && row.serial.as_deref() == Some(expected.serial)
        && row.source_config_fingerprint == expected.source_config_fingerprint
        && adapter_config_matches(&row.adapter_config, expected)
        && matches!(
            row.capabilities.as_slice(),
            [capability] if capability == "historical"
        )
        && row.cadence_seconds.is_none()
        && row.active
        && row.station_active
        && row.provider_active
        && row.site_active
        && positive_decimal(&row.source_id)
}

/// Match the closed adapter configuration without implementing a JSON codec.
fn adapter_config_matches(
    entries: &[(String, RainFixedGaugeAdapterConfigValue)],
    expected: &SourceSpec,
) -> bool {
    entries.len() == 4
        && adapter_text(
            entries,
            "contractVersion",
            "rain-fixed-gauge-target-source/v1",
        )
        && adapter_unsigned(entries, "deviceId", expected.device_id)
        && adapter_unsigned(entries, "locationId", expected.location_id)
        && adapter_text(entries, "projection", "fixed-gauge-hour-target")
}

/// Require one unique text adapter member with its exact value.
fn adapter_text(
    entries: &[(String, RainFixedGaugeAdapterConfigValue)],
    key: &str,
    expected: &str,
) -> bool {
    let mut matched = entries.iter().filter(|(entry_key, _)| entry_key == key);
    matches!(
        (matched.next(), matched.next()),
        (
            Some((_, RainFixedGaugeAdapterConfigValue::Text(value))),
            None
        ) if value == expected
    )
}

/// Require one unique unsigned adapter member with its exact value.
fn adapter_unsigned(
    entries: &[(String, RainFixedGaugeAdapterConfigValue)],
    key: &str,
    expected: u64,
) -> bool {
    let mut matched = entries.iter().filter(|(entry_key, _)| entry_key == key);
    matches!(
        (matched.next(), matched.next()),
        (
            Some((_, RainFixedGaugeAdapterConfigValue::Unsigned(value))),
            None
        ) if *value == expected
    )
}

/// Recognize the Node query's positive decimal source identity domain.
fn positive_decimal(value: &str) -> bool {
    let mut bytes = value.bytes();
    // require a nonzero leading decimal digit before accepting the tail
    matches!(bytes.next(), Some(b'1'..=b'9')) && bytes.all(|byte| byte.is_ascii_digit())
}

/// Wrap one local admission refusal without changing either owner error.
const fn admission<PortError, ValuesError>(
    error: WorkerRainFixedGaugeReadAdmissionError,
) -> WorkerRainFixedGaugeReadError<PortError, ValuesError> {
    WorkerRainFixedGaugeReadError::Admission(error)
}

/// One immutable source specification copied from the frozen Node station catalog.
#[derive(Debug, Clone, Copy)]
struct SourceSpec {
    location_id: u64,
    device_id: u64,
    latitude: f64,
    longitude: f64,
    serial: &'static str,
    source_key: &'static str,
    source_config_fingerprint: &'static str,
}

const SOURCE_SPECS: &[SourceSpec] = &[
    SourceSpec {
        location_id: 64_255,
        device_id: 175_727,
        latitude: 47.95008,
        longitude: -122.43982,
        serial: "ST-00054713",
        source_key: "rain-target-tempest-64255",
        source_config_fingerprint: "c3485378e847b8c6dab3d0d6040f9e655e30527fb55ac410c4648ca483ed9fcb",
    },
    SourceSpec {
        location_id: 225_947,
        device_id: 1_239_187,
        latitude: 47.94215,
        longitude: -122.42542,
        serial: "ST-00212830",
        source_key: "rain-target-tempest-225947",
        source_config_fingerprint: "49c94de8dd8af63753ff11696065f999685a68371f34edaab7f955fac1e74a56",
    },
    SourceSpec {
        location_id: 38_270,
        device_id: 115_866,
        latitude: 47.95293,
        longitude: -122.41414,
        serial: "ST-00157152",
        source_key: "rain-target-tempest-38270",
        source_config_fingerprint: "7702ee49af9f745447caad4d7620f96136f99f55ab2a0c65183f3696df6410eb",
    },
    SourceSpec {
        location_id: 168_853,
        device_id: 401_592,
        latitude: 47.95498,
        longitude: -122.44074,
        serial: "ST-00170845",
        source_key: "rain-target-tempest-168853",
        source_config_fingerprint: "2257ecca8e6fd84698aa593d4475a48a6f1fe8255445e5119046c22cdd3fea6e",
    },
    SourceSpec {
        location_id: 126_537,
        device_id: 313_016,
        latitude: 47.9582,
        longitude: -122.44274,
        serial: "ST-00134621",
        source_key: "rain-target-tempest-126537",
        source_config_fingerprint: "8ea1a193367cc58209716100201cf591559be8bc5f5039b17a7ccf01086d360d",
    },
    SourceSpec {
        location_id: 201_058,
        device_id: 466_938,
        latitude: 47.96244,
        longitude: -122.43369,
        serial: "ST-00194085",
        source_key: "rain-target-tempest-201058",
        source_config_fingerprint: "b472d71fcf0c210ab316d859ff8d06abe6cd549eaa4c01527d642557fd71cecb",
    },
    SourceSpec {
        location_id: 203_055,
        device_id: 470_937,
        latitude: 47.96505,
        longitude: -122.4241,
        serial: "ST-00198967",
        source_key: "rain-target-tempest-203055",
        source_config_fingerprint: "b058225817f05b597e1303d157b60828edc45fb3fa41a1ca22a8514919c4fdae",
    },
    SourceSpec {
        location_id: 66_270,
        device_id: 180_230,
        latitude: 47.93134,
        longitude: -122.42912,
        serial: "ST-00173167",
        source_key: "rain-target-tempest-66270",
        source_config_fingerprint: "850a1dff37d523c36f46e38bde3987617d78b616ebc398de9005b0d73435d371",
    },
    SourceSpec {
        location_id: 34_768,
        device_id: 107_388,
        latitude: 47.91752,
        longitude: -122.41112,
        serial: "ST-00020495",
        source_key: "rain-target-tempest-34768",
        source_config_fingerprint: "1c1d7019f6407345beec3d359402e9954378425565b894555cc44c91808e1a39",
    },
    SourceSpec {
        location_id: 88_159,
        device_id: 230_560,
        latitude: 47.91563,
        longitude: -122.41845,
        serial: "ST-00094734",
        source_key: "rain-target-tempest-88159",
        source_config_fingerprint: "701bcfd337445ab48ac6fbe35dec1c8556aae905f7591dae2311ae11a42b788f",
    },
    SourceSpec {
        location_id: 126_197,
        device_id: 312_302,
        latitude: 47.91413,
        longitude: -122.41471,
        serial: "ST-00129187",
        source_key: "rain-target-tempest-126197",
        source_config_fingerprint: "dc642c28ed38c6f174db0225e6c69518d3eae696eb14c44936f64ad6856d3b0e",
    },
    SourceSpec {
        location_id: 27_140,
        device_id: 87_271,
        latitude: 47.98707,
        longitude: -122.46295,
        serial: "ST-00000360",
        source_key: "rain-target-tempest-27140",
        source_config_fingerprint: "96ab4d0ebc5e14de870ab3dae8e73eb821d6d1b875de2c528b8e881a0f244e0c",
    },
];
