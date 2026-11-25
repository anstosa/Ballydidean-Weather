use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    FieldPresence, ForecastPressureContextQuery, ForecastProductSelection, ForecastQuery, Nullable,
    WeatherRecordRow,
};

const MAX_FORECAST_HOURS: u64 = 264;

const WEATHER_RECORD_SELECTION: &str = r#"
    wr.id,
    wr.content_hash AS "contentHash",
    wr.source_id AS "sourceId",
    NULL::text AS "sourceConfigFingerprint",
    NULL::text AS "adapterVersion",
    NULL::text AS "contractEpoch",
    s.source_key AS "sourceKey",
    wr.source_kind AS "sourceKind",
    wr.valid_at AS "validAt",
    wr.product_run_at AS "productRunAt",
    wr.first_received_at AS "firstReceivedAt",
    wr.last_received_at AS "lastReceivedAt",
    wr.upstream_timezone AS "upstreamTimezone",
    wr.upstream_model AS "upstreamModel",
    wr.device_vendor AS "deviceVendor",
    wr.device_model AS "deviceModel",
    wr.device_serial AS "deviceSerial",
    wr.quality_metadata AS "qualityMetadata",
    wr.provider_metadata AS "providerMetadata",
    wr.revision_count AS "revisionCount",
    wr.temperature_c AS "temperatureC",
    wr.apparent_temperature_c AS "apparentTemperatureC",
    wr.precipitation_mm AS "precipitationMm",
    wr.wind_speed_mps AS "windSpeedMps",
    wr.wind_gust_mps AS "windGustMps",
    wr.pressure_hpa AS "pressureHpa",
    wr.relative_humidity_percent AS "relativeHumidityPercent",
    wr.cloud_cover_percent AS "cloudCoverPercent",
    wr.wind_direction_degrees AS "windDirectionDegrees",
    wr.black_globe_temperature_c AS "blackGlobeTemperatureC",
    wr.pm25_micrograms_per_cubic_meter AS "pm25MicrogramsPerCubicMeter",
    wr.precipitation_rate_mm_per_hour AS "precipitationRateMmPerHour",
    wr.soil_electrical_conductivity_us_cm AS "soilElectricalConductivityMicrosiemensPerCm",
    wr.soil_moisture_percent AS "soilMoisturePercent",
    wr.solar_radiation_wm2 AS "solarRadiationWm2",
    wr.uv_index AS "uvIndex",
    wr.wet_bulb_globe_temperature_c AS "wetBulbGlobeTemperatureC",
    wr.water_level_m AS "waterLevelM",
    st.slug AS "stationSlug",
    si.slug AS "siteSlug",
    p.provider_key AS "providerKey"
  "#;

const WEATHER_RECORD_LATERAL_SELECTION: &str = r#"
    candidate.id,
    candidate.source_id,
    candidate.source_kind,
    candidate.valid_at,
    candidate.product_run_at,
    candidate.first_received_at,
    candidate.last_received_at,
    candidate.upstream_timezone,
    candidate.upstream_model,
    candidate.device_vendor,
    candidate.device_model,
    candidate.device_serial,
    candidate.quality_metadata,
    candidate.provider_metadata,
    candidate.temperature_c,
    candidate.apparent_temperature_c,
    candidate.precipitation_mm,
    candidate.wind_speed_mps,
    candidate.wind_gust_mps,
    candidate.pressure_hpa,
    candidate.relative_humidity_percent,
    candidate.cloud_cover_percent,
    candidate.wind_direction_degrees,
    candidate.content_hash,
    candidate.revision_count,
    candidate.black_globe_temperature_c,
    candidate.pm25_micrograms_per_cubic_meter,
    candidate.precipitation_rate_mm_per_hour,
    candidate.soil_electrical_conductivity_us_cm,
    candidate.soil_moisture_percent,
    candidate.solar_radiation_wm2,
    candidate.uv_index,
    candidate.wet_bulb_globe_temperature_c,
    candidate.water_level_m
  "#;

const ANCHOR_PRODUCT_RUN_PREDICATE: &str = r#"candidate.product_run_at = (
            SELECT MAX(product.product_run_at)
            FROM weather_records product
            WHERE product.source_id = s.id
              AND product.source_kind = 'forecast'
              AND product.valid_at = $2
          )"#;

const LATEST_PRODUCT_RUN_PREDICATE: &str = r#"candidate.product_run_at = (
            SELECT product.product_run_at
            FROM public.forecast_products product
            WHERE product.source_id = s.id
            ORDER BY product.product_run_at DESC
            LIMIT 1
          )"#;

/// the exact source-owned optional provenance statement
pub const FORECAST_PROVENANCE_SQL: &str = r#"
        SELECT provenance.*
        FROM unnest($1::bigint[]) AS requested(weather_record_id)
        -- probe exact identities instead of scanning historical provenance
        CROSS JOIN LATERAL (
          SELECT
            weather_record_id AS "weatherRecordId",
            source_id AS "sourceId",
            source_key AS "sourceKey",
            source_config_fingerprint AS "sourceConfigFingerprint",
            adapter_version AS "adapterVersion",
            contract_epoch AS "contractEpoch"
          FROM forecast_runtime_provenance_v1
          WHERE weather_record_id = requested.weather_record_id
          -- retain duplicate-linkage detection without flattening the probe
          LIMIT 2
        ) provenance
        ORDER BY provenance."weatherRecordId" ASC
        LIMIT 264
      "#;

/// one forecast query carrying admitted canonical endpoint evidence
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedForecastQuery {
    query: ForecastQuery,
    as_of_epoch_milliseconds: i64,
    end_exclusive: String,
    end_exclusive_epoch_milliseconds: i64,
}

impl AdmittedForecastQuery {
    /// retain one query and end instant already resolved by the shared time boundary
    pub fn admitted(
        query: ForecastQuery,
        as_of_epoch_milliseconds: i64,
        end_exclusive: String,
        end_exclusive_epoch_milliseconds: i64,
    ) -> Self {
        Self {
            query,
            as_of_epoch_milliseconds,
            end_exclusive,
            end_exclusive_epoch_milliseconds,
        }
    }
}

/// one typed forecast statement bind in placeholder order
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastReadBind {
    Text(String),
    UtcInstant(String),
}

/// one exact raw forecast statement and typed binds
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastReadPlan {
    pub sql: String,
    pub binds: [ForecastReadBind; 3],
}

/// one rejected forecast repository query
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastReadPlanError {
    InvalidHours,
    InvalidResolvedWindow,
}

impl Display for ForecastReadPlanError {
    /// render the source repository failure category
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the source hours failure and bound injected-window failures
        match self {
            Self::InvalidHours => formatter.write_str("forecast hours must be between 1 and 264"),
            Self::InvalidResolvedWindow => {
                formatter.write_str("forecast resolved window is invalid")
            }
        }
    }
}

impl Error for ForecastReadPlanError {}

/// one typed provenance identifier array bind
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastProvenanceBind(pub Vec<String>);

/// one exact optional provenance statement
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastProvenancePlan {
    pub sql: &'static str,
    pub bind: ForecastProvenanceBind,
}

/// one decoded optional forecast provenance row
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastRuntimeProvenanceRow {
    pub adapter_version: String,
    pub contract_epoch: String,
    pub source_config_fingerprint: String,
    pub source_id: String,
    pub source_key: String,
    pub weather_record_id: String,
}

/// one pressure-context query carrying admitted offset evidence
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedForecastPressureContextQuery {
    query: ForecastPressureContextQuery,
    as_of_epoch_milliseconds: i64,
    context_start: String,
    context_start_epoch_milliseconds: i64,
    context_end: String,
    context_end_epoch_milliseconds: i64,
    oldest_run: String,
    oldest_run_epoch_milliseconds: i64,
}

impl AdmittedForecastPressureContextQuery {
    /// retain exact context offsets already resolved by the shared time boundary
    #[allow(clippy::too_many_arguments)]
    pub fn admitted(
        query: ForecastPressureContextQuery,
        as_of_epoch_milliseconds: i64,
        context_start: String,
        context_start_epoch_milliseconds: i64,
        context_end: String,
        context_end_epoch_milliseconds: i64,
        oldest_run: String,
        oldest_run_epoch_milliseconds: i64,
    ) -> Self {
        Self {
            query,
            as_of_epoch_milliseconds,
            context_start,
            context_start_epoch_milliseconds,
            context_end,
            context_end_epoch_milliseconds,
            oldest_run,
            oldest_run_epoch_milliseconds,
        }
    }
}

/// one typed pressure-context statement bind in placeholder order
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastPressureContextBind {
    Text(String),
    UtcInstant(String),
}

/// one exact pressure-context statement and typed binds
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastPressureContextPlan {
    pub sql: String,
    pub binds: [ForecastPressureContextBind; 5],
}

/// one rejected pressure-context boundary
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastPressureContextPlanError;

impl Display for ForecastPressureContextPlanError {
    /// render the admitted-boundary failure
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("forecast pressure context window is invalid")
    }
}

impl Error for ForecastPressureContextPlanError {}

/// ordered typed pressure-context rows after database decoding
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastPressureContextResult {
    pub rows: Vec<WeatherRecordRow>,
}

/// build the exact bounded raw forecast read plan
pub fn forecast_read_plan(
    query: &AdmittedForecastQuery,
) -> Result<ForecastReadPlan, ForecastReadPlanError> {
    // require the source-owned public bound
    if !(1..=MAX_FORECAST_HOURS).contains(&query.query.hours) {
        return Err(ForecastReadPlanError::InvalidHours);
    }

    let expected_duration = query
        .query
        .hours
        .checked_mul(3_600_000)
        .and_then(|duration| i64::try_from(duration).ok());
    let actual_duration = query
        .end_exclusive_epoch_milliseconds
        .checked_sub(query.as_of_epoch_milliseconds);

    // bind only a canonical end proven to match the admitted hourly horizon
    if expected_duration != actual_duration
        || !matches!(actual_duration, Some(duration) if duration > 0)
    {
        return Err(ForecastReadPlanError::InvalidResolvedWindow);
    }

    // choose the exact reviewed product predicate
    let product_run_predicate = match query.query.productSelection {
        FieldPresence::Missing | FieldPresence::Value(ForecastProductSelection::Latest) => {
            LATEST_PRODUCT_RUN_PREDICATE
        }
        FieldPresence::Value(ForecastProductSelection::AnchorContaining) => {
            ANCHOR_PRODUCT_RUN_PREDICATE
        }
    };
    let sql = format!(
        r#"
      SELECT
        {WEATHER_RECORD_SELECTION}
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      JOIN LATERAL (
        SELECT {WEATHER_RECORD_LATERAL_SELECTION}
        FROM weather_records candidate
        WHERE candidate.source_id = s.id
          -- expose the existing identity-index prefix before filtering old runs
          AND candidate.source_kind = 'forecast'
          AND {product_run_predicate}
          AND candidate.valid_at >= $2
          AND candidate.valid_at < $3
        ORDER BY candidate.valid_at ASC, candidate.id ASC
        LIMIT 264
      ) wr ON true
      WHERE si.slug = $1
        AND si.active
        AND st.active
        AND s.active
        AND p.active
        AND s.source_kind = 'forecast'
        AND s.capabilities @> '["forecast"]'::jsonb
        AND weather_source_is_current(s.id)
      ORDER BY wr.valid_at ASC, wr.id ASC
    "#,
    );

    Ok(ForecastReadPlan {
        sql,
        binds: [
            ForecastReadBind::Text(query.query.siteSlug.clone()),
            ForecastReadBind::UtcInstant(query.query.asOf.clone()),
            ForecastReadBind::UtcInstant(query.end_exclusive.clone()),
        ],
    })
}

/// build the exact complete-vintage pressure-context read plan
pub fn forecast_pressure_context_plan(
    query: &AdmittedForecastPressureContextQuery,
) -> Result<ForecastPressureContextPlan, ForecastPressureContextPlanError> {
    let context_start_offset = query
        .context_start_epoch_milliseconds
        .checked_sub(query.as_of_epoch_milliseconds);
    let context_end_offset = query
        .context_end_epoch_milliseconds
        .checked_sub(query.as_of_epoch_milliseconds);
    let oldest_run_offset = query
        .oldest_run_epoch_milliseconds
        .checked_sub(query.as_of_epoch_milliseconds);

    // require the exact source-owned minus-three, plus-three, and minus-forty-eight offsets
    if context_start_offset != Some(-3 * 3_600_000)
        || context_end_offset != Some(3 * 3_600_000)
        || oldest_run_offset != Some(-48 * 3_600_000)
    {
        return Err(ForecastPressureContextPlanError);
    }

    let sql = format!(
        r#"
      SELECT
        {WEATHER_RECORD_SELECTION}
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      JOIN LATERAL (
        WITH slots AS MATERIALIZED (
          SELECT generated.slot
          FROM generate_series(
            $2::timestamptz,
            $3::timestamptz - interval '1 hour',
            interval '1 hour'
          ) AS generated(slot)
        ), identity_keys AS MATERIALIZED (
          -- singleton boundaries use exact covering-index suffix ranges
          SELECT
            point.source_id,
            point.source_kind,
            point.valid_at,
            point.product_run_at
          FROM slots
          CROSS JOIN LATERAL (
            SELECT
              identity.source_id,
              identity.source_kind,
              identity.valid_at,
              identity.product_run_at
            FROM weather_records identity
            WHERE identity.source_id = s.id
              AND identity.source_kind = 'forecast'
              AND ROW(identity.valid_at, identity.product_run_at)
                >= ROW(slots.slot, $4::timestamptz)
              AND ROW(identity.valid_at, identity.product_run_at)
                <= ROW(slots.slot, $5::timestamptz)
              AND identity.product_run_at >= $4
              AND identity.product_run_at <= $5
            -- retain the parameterized range without removing rows
            OFFSET 0
          ) point
          UNION ALL
          -- strict gaps preserve every nonhour identity without duplicating boundaries
          SELECT
            gap.source_id,
            gap.source_kind,
            gap.valid_at,
            gap.product_run_at
          FROM slots
          CROSS JOIN LATERAL (
            SELECT
              identity.source_id,
              identity.source_kind,
              identity.valid_at,
              identity.product_run_at
            FROM weather_records identity
            WHERE identity.source_id = s.id
              AND identity.source_kind = 'forecast'
              AND identity.valid_at > slots.slot
              AND identity.valid_at < slots.slot + interval '1 hour'
              AND identity.product_run_at >= $4
              AND identity.product_run_at <= $5
            -- retain the parameterized range without removing rows
            OFFSET 0
          ) gap
        )
        SELECT product.product_run_at, qualified.record_ids
        FROM public.forecast_products product
        JOIN LATERAL (
          SELECT
            array_agg(raw.id ORDER BY raw.valid_at ASC, raw.id ASC) AS record_ids
          FROM identity_keys identity
          JOIN weather_records raw
            ON raw.source_id = identity.source_id
            AND raw.source_kind = 'forecast'
            -- equal inclusive bounds retain every component of the unique identity
            AND ROW(raw.valid_at, raw.product_run_at) >= ROW(
              identity.valid_at, identity.product_run_at
            )
            AND ROW(raw.valid_at, raw.product_run_at) <= ROW(
              identity.valid_at, identity.product_run_at
            )
          WHERE identity.source_id = product.source_id
            AND identity.product_run_at = product.product_run_at
            AND raw.pressure_hpa > '-Infinity'::double precision
            AND raw.pressure_hpa < 'Infinity'::double precision
          HAVING COUNT(*) = 6
            AND MIN(raw.valid_at) = $2
            AND MAX(raw.valid_at) = $3::timestamptz - interval '1 hour'
            AND BOOL_AND(raw.valid_at = date_trunc('hour', raw.valid_at))
        ) qualified ON true
        WHERE product.source_id = s.id
          AND product.product_run_at >= $4
          AND product.product_run_at <= $5
        ORDER BY product.product_run_at DESC
        LIMIT 1
      ) context_run ON true
      JOIN LATERAL (
        SELECT {WEATHER_RECORD_LATERAL_SELECTION}
        FROM weather_records candidate
        WHERE candidate.id = ANY(context_run.record_ids)
        ORDER BY candidate.valid_at ASC, candidate.id ASC
      ) wr ON true
      WHERE si.slug = $1
        AND si.active
        AND st.active
        AND s.active
        AND p.active
        AND s.source_kind = 'forecast'
        AND s.source_key = 'open-meteo-forecast-v4'
        AND s.capabilities @> '["forecast"]'::jsonb
        AND weather_source_is_current(s.id)
      ORDER BY wr.valid_at ASC, wr.id ASC
      LIMIT 6
    "#,
    );

    Ok(ForecastPressureContextPlan {
        sql,
        binds: [
            ForecastPressureContextBind::Text(query.query.siteSlug.clone()),
            ForecastPressureContextBind::UtcInstant(query.context_start.clone()),
            ForecastPressureContextBind::UtcInstant(query.context_end.clone()),
            ForecastPressureContextBind::UtcInstant(query.oldest_run.clone()),
            ForecastPressureContextBind::UtcInstant(query.query.asOf.clone()),
        ],
    })
}

/// preserve the six-row database order after typed decoding
pub fn project_forecast_pressure_context_rows(
    rows: Vec<WeatherRecordRow>,
) -> ForecastPressureContextResult {
    ForecastPressureContextResult { rows }
}

/// build the optional exact-record provenance read after a nonempty raw result
pub fn forecast_provenance_plan(rows: &[WeatherRecordRow]) -> Option<ForecastProvenancePlan> {
    // skip optional provenance without raw records
    if rows.is_empty() {
        return None;
    }

    Some(ForecastProvenancePlan {
        sql: FORECAST_PROVENANCE_SQL,
        bind: ForecastProvenanceBind(rows.iter().map(|row| row.id.clone()).collect()),
    })
}

/// decorate valid optional provenance or return every authoritative raw row unchanged
pub fn decorate_forecast_provenance(
    raw_rows: Vec<WeatherRecordRow>,
    provenance_rows: Vec<ForecastRuntimeProvenanceRow>,
) -> Vec<WeatherRecordRow> {
    let raw_by_id = raw_rows
        .iter()
        .map(|row| {
            (
                row.id.as_str(),
                (row.sourceId.as_str(), row.sourceKey.as_str()),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();

    // validate every optional linkage before decoration
    for provenance in &provenance_rows {
        let raw_identity = raw_by_id.get(provenance.weather_record_id.as_str());

        // fail raw for missing, cross-source, or duplicate linkage
        if raw_identity
            != Some(&(
                provenance.source_id.as_str(),
                provenance.source_key.as_str(),
            ))
            || !seen.insert(provenance.weather_record_id.as_str())
        {
            return raw_rows;
        }
    }

    let provenance_by_id = provenance_rows
        .into_iter()
        .map(|row| (row.weather_record_id.clone(), row))
        .collect::<HashMap<_, _>>();

    raw_rows
        .into_iter()
        .map(|mut row| {
            // decorate matching records without changing raw row ordering
            if let Some(provenance) = provenance_by_id.get(&row.id) {
                row.adapterVersion = Nullable::Value(provenance.adapter_version.clone());
                row.contractEpoch = Nullable::Value(provenance.contract_epoch.clone());
                row.sourceConfigFingerprint =
                    Nullable::Value(provenance.source_config_fingerprint.clone());
            }
            row
        })
        .collect()
}
