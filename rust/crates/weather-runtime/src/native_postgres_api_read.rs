//! Native API statement execution on the admitted process-owned PostgreSQL pool.

use tokio_postgres::Row;
use weather_contracts::*;
use weather_database::*;
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::{
    ApiReadPoolLease, ApiReadSessionDisposition, ApiReadSessionFailure, NativePostgresError,
    NativePostgresParameter as Parameter, NativePostgresPool, NativePostgresSession,
    NativePostgresType, NativeWirePrimitives, WeatherApiReadPool, WeatherApiReadPoolConnector,
    WeatherApiSharedPool, WeatherServicePostgresReadPort, native_postgres_weather_rows as field,
};

/// Prepare the real pool lazily through the established API connector boundary.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativePostgresApiConnector;

impl WeatherApiReadPoolConnector for NativePostgresApiConnector {
    type Error = NativePostgresError;
    type Pool = NativePostgresPool;

    /// Retain the exact role-specific pool settings without opening a connection at startup.
    fn prepare_pool(
        &mut self,
        configuration: DatabasePoolConfiguration,
    ) -> Result<Self::Pool, Self::Error> {
        NativePostgresPool::new(configuration)
    }
}

impl WeatherApiReadPool for NativePostgresPool {
    type Error = NativePostgresError;
    type Session<'pool> = NativePostgresSession;

    /// Acquire from the shared bounded pool rather than allocating per request.
    fn checkout(&mut self) -> Result<Self::Session<'_>, Self::Error> {
        NativePostgresPool::checkout(self)
    }

    /// Only recycle query sessions after successful server execution followed by decode failure.
    fn classify_session_failure(error: &Self::Error) -> ApiReadSessionFailure {
        if error.is_post_query_decode() {
            ApiReadSessionFailure::PostQueryDecode
        } else {
            ApiReadSessionFailure::StatementOrClient
        }
    }
}

impl WeatherApiSharedPool for NativePostgresPool {
    /// Retain the same process admission capacity in the private application lane.
    fn share(&self) -> Self {
        self.clone()
    }
}

impl ApiReadPoolLease for NativePostgresSession {
    /// Resolve the caller's exact session disposition without masking a query result.
    fn release(self, disposition: ApiReadSessionDisposition) {
        let _ = match disposition {
            ApiReadSessionDisposition::Recycle => NativePostgresSession::release(self),
            ApiReadSessionDisposition::Discard => self.discard(),
        };
    }
}

/// Bind an admitted canonical timestamp as native timestamptz without changing SQL.
fn timestamp(value: &str) -> Result<Parameter, NativePostgresError> {
    NativeWirePrimitives
        .canonical_utc_milliseconds(value)
        .map(|_| Parameter::Timestamp(value.to_owned()))
        .ok_or_else(|| NativePostgresError::decode("timestamp parameter", "invalid UTC instant"))
}

/// Bind bigint identifiers as the actual PostgreSQL type instead of passing them through floats.
fn bigint(value: &str) -> Result<Parameter, NativePostgresError> {
    value
        .parse::<i64>()
        .map(Parameter::I64)
        .map_err(|error| NativePostgresError::decode("bigint parameter", error))
}

/// Bind bounded limit values with PostgreSQL's bigint LIMIT type.
fn limit(value: u64) -> Result<Parameter, NativePostgresError> {
    i64::try_from(value)
        .map(Parameter::I64)
        .map_err(|error| NativePostgresError::decode("limit parameter", error))
}

/// Execute a statement once and decode only its driver-owned result rows.
fn rows<T>(
    session: &mut NativePostgresSession,
    sql: &str,
    params: &[Parameter],
    decode: impl Fn(&Row) -> Result<T, NativePostgresError>,
) -> Result<Vec<T>, NativePostgresError> {
    session.query(sql, params)?.iter().map(decode).collect()
}

/// Decode the source's first optional row without a second query.
fn first<T>(
    session: &mut NativePostgresSession,
    sql: &str,
    params: &[Parameter],
    decode: impl Fn(&Row) -> Result<T, NativePostgresError>,
) -> Result<Option<T>, NativePostgresError> {
    session.query(sql, params)?.first().map(decode).transpose()
}

// keep the structurally identical source text/timestamp bind families at one adapter boundary
macro_rules! timestamp_text_binds {
    ($values:expr, $kind:ident) => {
        $values
            .iter()
            .map(|value| match value {
                $kind::Text(text) => Ok(Parameter::Text(text.clone())),
                $kind::UtcInstant(instant) => timestamp(instant),
            })
            .collect::<Result<Vec<_>, NativePostgresError>>()?
    };
}

impl WeatherServicePostgresReadPort for NativePostgresSession {
    /// Execute exact active-site discovery and map its native scalar aliases.
    fn query_active_sites(
        &mut self,
        plan: &ListActiveSitesQuery,
    ) -> Result<Vec<ActiveSiteDbRow>, NativePostgresError> {
        rows(self, plan.sql, &[], field::active_site)
    }

    /// Preserve the nullable station and bigint source selection independently.
    fn query_current(
        &mut self,
        plan: &CurrentWeatherQueryPlan,
    ) -> Result<Vec<CurrentWeatherRecordRow>, NativePostgresError> {
        let [
            CurrentWeatherBind::Text(site),
            CurrentWeatherBind::NullableText(station),
            CurrentWeatherBind::NullableText(source),
            CurrentWeatherBind::UtcInstant(as_of),
        ] = &plan.binds
        else {
            return Err(NativePostgresError::decode(
                "current binds",
                "unexpected parameter shape",
            ));
        };
        let station = station
            .clone()
            .map_or(Parameter::Null(NativePostgresType::Text), Parameter::Text);
        let source = source
            .as_deref()
            .map(bigint)
            .transpose()?
            .unwrap_or(Parameter::Null(NativePostgresType::I64));
        rows(
            self,
            plan.sql,
            &[
                Parameter::Text(site.clone()),
                station,
                source,
                timestamp(as_of)?,
            ],
            field::current,
        )
    }

    /// Execute the exact dynamic history statement with its admitted placeholder domains.
    fn query_history(
        &mut self,
        plan: &HistoryReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, NativePostgresError> {
        let params = plan
            .binds
            .iter()
            .map(|bind| match bind {
                HistoryReadBind::Text(text) => Ok(Parameter::Text(text.clone())),
                HistoryReadBind::UtcInstant(instant) => timestamp(instant),
                HistoryReadBind::BigintText(id) => bigint(id),
                HistoryReadBind::SourceKind(kind) => {
                    let value = SourceKind::WIRE_VALUES
                        .iter()
                        .find_map(|(wire, value)| (*value == *kind).then_some(*wire))
                        .ok_or_else(|| {
                            NativePostgresError::decode("source kind", "unsupported source kind")
                        })?;
                    Ok(Parameter::Text(value.to_owned()))
                }
                HistoryReadBind::Limit(value) => limit(*value),
            })
            .collect::<Result<Vec<_>, _>>()?;
        rows(self, &plan.sql, &params, field::weather)
    }

    /// Execute the ordered tide statement without any client-side sort or truncation.
    fn query_tides(
        &mut self,
        plan: &TideReadPlan,
    ) -> Result<Vec<TideRecordRow>, NativePostgresError> {
        let params = plan
            .binds
            .iter()
            .map(|bind| match bind {
                TideReadBind::Text(text) => Ok(Parameter::Text(text.clone())),
                TideReadBind::UtcInstant(instant) => timestamp(instant),
                TideReadBind::Limit(value) => limit(*value),
            })
            .collect::<Result<Vec<_>, _>>()?;
        rows(self, plan.sql, &params, tide_row)
    }

    /// Execute the unchanged forecast SQL and return its raw typed records.
    fn query_forecast(
        &mut self,
        plan: &ForecastReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, NativePostgresError> {
        let params = timestamp_text_binds!(plan.binds, ForecastReadBind);
        rows(self, &plan.sql, &params, field::weather)
    }

    /// Query the exact bigint array of returned forecast identities.
    fn query_forecast_provenance(
        &mut self,
        plan: &ForecastProvenancePlan,
    ) -> Result<Vec<ForecastRuntimeProvenanceRow>, NativePostgresError> {
        let ids = plan
            .bind
            .0
            .iter()
            .map(|value| {
                value
                    .parse::<i64>()
                    .map_err(|error| NativePostgresError::decode("weather record id", error))
            })
            .collect::<Result<Vec<_>, _>>()?;
        rows(self, plan.sql, &[Parameter::I64Array(ids)], provenance_row)
    }

    /// Execute the complete-vintage pressure context statement as written.
    fn query_forecast_pressure_context(
        &mut self,
        plan: &ForecastPressureContextPlan,
    ) -> Result<Vec<WeatherRecordRow>, NativePostgresError> {
        let params = timestamp_text_binds!(plan.binds, ForecastPressureContextBind);
        rows(self, &plan.sql, &params, field::weather)
    }

    /// Execute the daily trend query without changing the original aggregation geometry.
    fn query_trends(
        &mut self,
        plan: &TrendReadPlan,
    ) -> Result<Vec<TrendPointRow>, NativePostgresError> {
        let params = timestamp_text_binds!(plan.binds, TrendReadBind);
        rows(self, plan.sql, &params, trend_row)
    }

    /// Execute the nearest-gauge precipitation query once.
    fn query_daily_precipitation(
        &mut self,
        plan: &DailyPrecipitationReadPlan,
    ) -> Result<Vec<DailyPrecipitationRow>, NativePostgresError> {
        let params = timestamp_text_binds!(plan.binds, DailyPrecipitationBind);
        rows(self, plan.sql, &params, precipitation_row)
    }

    /// Execute the fixed private observation query including its optional left-lateral records.
    fn query_forecast_observation_input(
        &mut self,
        plan: &ForecastObservationInputReadPlan,
    ) -> Result<Vec<ForecastObservationInputDbRow>, NativePostgresError> {
        let [ForecastObservationInputBind::UtcInstant(instant)] = &plan.binds;
        rows(self, plan.sql, &[timestamp(instant)?], observation_row)
    }

    /// Use the exact Node connectivity statement.
    fn query_health_ping(&mut self, plan: &HealthPingReadPlan) -> Result<(), NativePostgresError> {
        self.query(plan.sql, &[]).map(|_| ())
    }

    /// Decode the immutable ordered migration ledger and its admitted digests.
    fn query_health_migration_ledger(
        &mut self,
        plan: &HealthMigrationLedgerReadPlan,
    ) -> Result<Vec<AppliedMigration>, NativePostgresError> {
        rows(self, plan.sql, &[], |row| {
            Ok(AppliedMigration {
                name: field::column(row, "name")?,
                digest: MigrationDigest::admitted(field::digest(row, "checksum")?.as_str()),
            })
        })
    }

    /// Decode precisely the latest heartbeat timestamp, with no client time substitution.
    fn query_health_latest_worker_heartbeat(
        &mut self,
        plan: &HealthLatestWorkerHeartbeatReadPlan,
    ) -> Result<Option<HealthLatestWorkerHeartbeatRow>, NativePostgresError> {
        first(self, plan.sql, &[], |row| {
            Ok(HealthLatestWorkerHeartbeatRow {
                last_loop_at: field::date(row, "lastLoopAt")?,
            })
        })
    }
}

/// Decode tide record aliases and closed tide-only source kinds.
fn tide_row(row: &Row) -> Result<TideRecordRow, NativePostgresError> {
    Ok(TideRecordRow {
        attributionLabel: field::column(row, "attributionLabel")?,
        attributionUrl: field::column(row, "attributionUrl")?,
        predictionType: field::nullable(row, "predictionType")?,
        providerKey: field::column(row, "providerKey")?,
        sourceId: field::id(row, "sourceId")?,
        sourceKind: field::wire_enum(row, "sourceKind", TideRecordRowSourceKindEnum::WIRE_VALUES)?,
        stationName: field::column(row, "stationName")?,
        stationSlug: field::column(row, "stationSlug")?,
        validAt: field::date(row, "validAt")?,
        waterLevelM: field::column(row, "waterLevelM")?,
    })
}

/// Decode native provenance aliases without recalculating the stored identities.
fn provenance_row(row: &Row) -> Result<ForecastRuntimeProvenanceRow, NativePostgresError> {
    Ok(ForecastRuntimeProvenanceRow {
        adapter_version: field::column(row, "adapterVersion")?,
        contract_epoch: field::column(row, "contractEpoch")?,
        source_config_fingerprint: field::column(row, "sourceConfigFingerprint")?,
        source_id: field::id(row, "sourceId")?,
        source_key: field::column(row, "sourceKey")?,
        weather_record_id: field::id(row, "weatherRecordId")?,
    })
}

/// Decode aggregate trend metrics in their original nullable representation.
fn trend_row(row: &Row) -> Result<TrendPointRow, NativePostgresError> {
    let date = field::date(row, "validAt")?;
    let valid_at = match date {
        JsDateOrString::DateEpochMilliseconds(value) => JsDate::EpochMilliseconds(value),
        _ => JsDate::InvalidDate,
    };
    Ok(TrendPointRow {
        apparentTemperatureC: field::nullable(row, "apparentTemperatureC")?,
        pm25MicrogramsPerCubicMeter: field::nullable(row, "pm25MicrogramsPerCubicMeter")?,
        precipitationMm: field::nullable(row, "precipitationMm")?,
        pressureHpa: field::nullable(row, "pressureHpa")?,
        relativeHumidityPercent: field::nullable(row, "relativeHumidityPercent")?,
        temperatureC: field::nullable(row, "temperatureC")?,
        temperatureMaximumC: field::nullable(row, "temperatureMaximumC")?,
        temperatureMinimumC: field::nullable(row, "temperatureMinimumC")?,
        validAt: valid_at,
        windDirectionDegrees: field::nullable(row, "windDirectionDegrees")?,
        windGustMps: field::nullable(row, "windGustMps")?,
        windSpeedMps: field::nullable(row, "windSpeedMps")?,
    })
}

/// Decode the exact nearest-gauge result, retaining its final observation time.
fn precipitation_row(row: &Row) -> Result<DailyPrecipitationRow, NativePostgresError> {
    Ok(DailyPrecipitationRow {
        accumulationMm: field::column(row, "accumulationMm")?,
        sourceId: field::id(row, "sourceId")?,
        stationSlug: field::column(row, "stationSlug")?,
        validThrough: field::date(row, "validThrough")?,
    })
}

/// Preserve the optional lateral observation row while retaining site geometry on empty data.
fn observation_row(row: &Row) -> Result<ForecastObservationInputDbRow, NativePostgresError> {
    let id = field::column::<Option<i64>>(row, "id")?;
    let record = id
        .map(|id| {
            Ok::<_, NativePostgresError>(ForecastObservationInputRow {
                apparentTemperatureC: field::nullable(row, "apparentTemperatureC")?,
                cloudCoverPercent: field::nullable(row, "cloudCoverPercent")?,
                id: id.to_string(),
                lastReceivedAt: field::date(row, "lastReceivedAt")?,
                pm25MicrogramsPerCubicMeter: field::nullable(row, "pm25MicrogramsPerCubicMeter")?,
                precipitationRateMmPerHour: field::nullable(row, "precipitationRateMmPerHour")?,
                pressureHpa: field::nullable(row, "pressureHpa")?,
                providerKey: field::column(row, "providerKey")?,
                relativeHumidityPercent: field::nullable(row, "relativeHumidityPercent")?,
                solarRadiationWm2: field::nullable(row, "solarRadiationWm2")?,
                sourceId: field::id(row, "sourceId")?,
                sourceKey: field::column(row, "sourceKey")?,
                sourceKind: field::wire_enum(
                    row,
                    "sourceKind",
                    ForecastObservationInputRowSourceKindEnum::WIRE_VALUES,
                )?,
                stationSlug: field::column(row, "stationSlug")?,
                temperatureC: field::nullable(row, "temperatureC")?,
                uvIndex: field::nullable(row, "uvIndex")?,
                validAt: field::date(row, "validAt")?,
                windDirectionDegrees: field::nullable(row, "windDirectionDegrees")?,
                windGustMps: field::nullable(row, "windGustMps")?,
                windSpeedMps: field::nullable(row, "windSpeedMps")?,
            })
        })
        .transpose()?;
    Ok(ForecastObservationInputDbRow {
        projection_from: field::date(row, "projectionFrom")?,
        query_from: field::date(row, "queryFrom")?,
        record,
        site_slug: field::column(row, "siteSlug")?,
        through: field::date(row, "through")?,
        timezone: field::column(row, "timezone")?,
    })
}

impl EcmwfTemperatureCanaryReadPort for NativePostgresSession {
    type Error = NativePostgresError;

    /// Execute the latest sidecar run read with its original cutoff.
    fn select_sidecar_run(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarRunReadPlan,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        let params = timestamp_text_binds!(plan.binds, EcmwfTemperatureCanaryReadBind);
        first(self, plan.sql, &params, canary_run)
    }

    /// Select the original ordered lead band for the bigint run identity.
    fn select_sidecar_hours(
        &mut self,
        plan: &EcmwfTemperatureCanarySidecarHoursReadPlan,
    ) -> Result<Vec<EcmwfTemperatureCanaryHourReadRow>, Self::Error> {
        let [
            EcmwfTemperatureCanaryReadBind::Text(id),
            EcmwfTemperatureCanaryReadBind::UtcInstant(from),
            EcmwfTemperatureCanaryReadBind::UtcInstant(to),
        ] = &plan.binds
        else {
            return Err(NativePostgresError::decode(
                "canary hour binds",
                "unexpected parameter shape",
            ));
        };
        rows(
            self,
            plan.sql,
            &[bigint(id)?, timestamp(from)?, timestamp(to)?],
            canary_hour,
        )
    }

    /// Select the latest collector status without reconstructing its server count.
    fn select_status(
        &mut self,
        plan: &EcmwfTemperatureCanaryStatusReadPlan,
    ) -> Result<Option<EcmwfTemperatureCanaryStatusReadRow>, Self::Error> {
        let params = timestamp_text_binds!(plan.binds, EcmwfTemperatureCanaryReadBind);
        first(self, plan.sql, &params, |row| {
            Ok(EcmwfTemperatureCanaryStatusReadRow {
                first_received_at: field::utc(row, "firstReceivedAt")?,
                hour_count: field::unsigned(row, "hourCount")?,
                run_initialized_at: field::utc(row, "runInitializedAt")?,
                state_reason: field::column(row, "stateReason")?,
                state_status: field::wire_enum(
                    row,
                    "stateStatus",
                    EcmwfTemperatureCanaryRunStateStatusEnum::WIRE_VALUES,
                )?,
            })
        })
    }
}

/// Decode a retained canary row using the generated literal and enum types.
fn canary_run(row: &Row) -> Result<StoredEcmwfCanaryRun, NativePostgresError> {
    let adapter_version = field::column::<String>(row, "adapterVersion")?;
    let upstream_model = field::column::<String>(row, "upstreamModel")?;
    // check the literal wire values before constructing their generated zero-sized types
    if adapter_version != EcmwfTemperatureCanaryRunAdapterVersionLiteral::WIRE_VALUE
        || upstream_model != EcmwfTemperatureCanaryRunUpstreamModelLiteral::WIRE_VALUE
    {
        return Err(NativePostgresError::decode(
            "canary identity",
            "unsupported adapter or upstream model",
        ));
    }
    let recent_error = field::json(row, "recentErrorState")?
        .ok_or_else(|| NativePostgresError::decode("recentErrorState", "missing state"))?;
    Ok(StoredEcmwfCanaryRun {
        adapter_version: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
        content_hash: field::digest(row, "contentHash")?,
        first_received_at: field::utc(row, "firstReceivedAt")?,
        id: field::id(row, "id")?,
        model_cycle: field::wire_enum(
            row,
            "modelCycle",
            EcmwfTemperatureCanaryRunModelCycleEnum::WIRE_VALUES,
        )?,
        provider_response_sha256: field::digest(row, "providerResponseSha256")?,
        recent_error_state: recent_error_state(&recent_error).ok_or_else(|| {
            NativePostgresError::decode("recentErrorState", "invalid canary state")
        })?,
        recent_error_state_sha256: field::digest(row, "recentErrorStateSha256")?,
        run_initialized_at: field::utc(row, "runInitializedAt")?,
        state_reason: field::column(row, "stateReason")?,
        state_status: field::wire_enum(
            row,
            "stateStatus",
            EcmwfTemperatureCanaryRunStateStatusEnum::WIRE_VALUES,
        )?,
        upstream_model: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    })
}

/// Decode the closed JSON state contract without replacing invalid evidence with defaults.
fn recent_error_state(value: &JsonValue) -> Option<EcmwfTemperatureCanaryRecentErrorState> {
    use crate::maintenance_canonical_json::{field as get, text};
    // preserve nullable numbers and strings with explicit null rather than synthesized values
    let number = |key| match get(value, key)? {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::Number(number) if number.is_finite() => Some(Nullable::Value(*number)),
        _ => None,
    };
    let nullable_text = |key| match get(value, key)? {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::String(units) => String::from_utf16(units).ok().map(Nullable::Value),
        _ => None,
    };
    let unsigned = |key| match get(value, key)? {
        JsonValue::Number(number)
            if (0.0..=9_007_199_254_740_991.0).contains(number) && number.fract() == 0.0 =>
        {
            Some(*number as u64)
        }
        _ => None,
    };
    let required_text = |key| text(value, key);
    // require the source cohort and ordered source keys
    if required_text("cohort")? != EcmwfTemperatureCanaryRecentErrorStateCohortLiteral::WIRE_VALUE {
        return None;
    }
    let JsonValue::Array(keys) = get(value, "sourceKeys")? else {
        return None;
    };
    let source_keys = keys
        .iter()
        .map(|key| match key {
            JsonValue::String(units) => String::from_utf16(units).ok(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let JsonValue::Boolean(supported) = get(value, "supported")? else {
        return None;
    };
    Some(EcmwfTemperatureCanaryRecentErrorState {
        b24C: number("b24C")?,
        b72C: number("b72C")?,
        cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
        localDates: unsigned("localDates")?,
        mad72C: number("mad72C")?,
        maximumSourceRunInitializedAt: nullable_text("maximumSourceRunInitializedAt")?,
        maximumSourceValidAt: nullable_text("maximumSourceValidAt")?,
        n24: unsigned("n24")?,
        n72: unsigned("n72")?,
        sourceKeys: source_keys,
        supported: *supported,
        targetRunInitializedAt: required_text("targetRunInitializedAt")?,
        windowEndValidAt: required_text("windowEndValidAt")?,
    })
}

/// Decode the immutable lead scalar and digest fields in database order.
fn canary_hour(row: &Row) -> Result<EcmwfTemperatureCanaryHourReadRow, NativePostgresError> {
    Ok(EcmwfTemperatureCanaryHourReadRow {
        content_hash: field::digest(row, "contentHash")?,
        model_lead_hours: field::unsigned(row, "modelLeadHours")?,
        raw_relative_humidity_percent: field::nullable(row, "rawRelativeHumidityPercent")?,
        raw_temperature_c: field::column(row, "rawTemperatureC")?,
        raw_wind_speed_mps: field::nullable(row, "rawWindSpeedMps")?,
        valid_at: field::utc(row, "validAt")?,
    })
}

impl RainServingReadPort for NativePostgresSession {
    type Error = NativePostgresError;

    /// Select the exact last fresh immutable rain serving row.
    fn select_rain_adjustment_run(
        &mut self,
        plan: &RainAdjustmentRunReadPlan,
    ) -> Result<Option<RainAdjustmentRunReadRow>, Self::Error> {
        let [RainServingReadBind::UtcInstant(instant)] = &plan.binds;
        first(self, plan.sql, &[timestamp(instant)?], rain_run)
    }

    /// Decode the source-owned aggregate status without touching raw capture data.
    fn select_rain_collection_status(
        &mut self,
        plan: &RainCollectionStatusReadPlan,
    ) -> Result<Option<RainCollectionStatusReadRow>, Self::Error> {
        first(self, plan.sql, &[], rain_status)
    }
}

/// Decode a rain row and its generated immutable hour payloads through the existing projector.
fn rain_run(row: &Row) -> Result<RainAdjustmentRunReadRow, NativePostgresError> {
    let Some(JsonValue::Array(hours)) = field::json(row, "hours")? else {
        return Err(NativePostgresError::decode(
            "hours",
            "expected rain hour array",
        ));
    };
    let hours = hours
        .iter()
        .map(crate::rain_performance_adjustment_rows::project_hour)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| NativePostgresError::decode("hours", error))?;
    let instant = |key| field::utc(row, key).map(AdmittedRevisionUtcInstant::admitted);
    Ok(RainAdjustmentRunReadRow {
        decision_at: instant("decision_at")?,
        first_received_at: instant("first_received_at")?,
        forecast_claim_id: field::id(row, "forecast_claim_id")?,
        generated_at: instant("generated_at")?,
        hours,
        input_sha256: field::digest(row, "input_sha256")?,
        model_sha256: field::digest(row, "model_sha256")?,
        run_initialized_at: instant("run_initialized_at")?,
    })
}

/// Decode all aggregate status fields at their native integer and timestamp widths.
fn rain_status(row: &Row) -> Result<RainCollectionStatusReadRow, NativePostgresError> {
    Ok(RainCollectionStatusReadRow {
        claims: field::unsigned(row, "claims")?,
        compressed_bytes: field::unsigned(row, "compressed_bytes")?,
        contract_version: field::column(row, "contract_version")?,
        failed_requests: field::unsigned(row, "failed_requests")?,
        last_claim_at: field::optional_utc(row, "last_claim_at")?,
        last_forecast_receipt_at: field::optional_utc(row, "last_forecast_receipt_at")?,
        last_receipt_at: field::optional_utc(row, "last_receipt_at")?,
        last_station_receipt_at: field::optional_utc(row, "last_station_receipt_at")?,
        model_enabled: field::column(row, "model_enabled")?,
        paused_until: field::optional_utc(row, "paused_until")?,
        pending_requests: field::unsigned(row, "pending_requests")?,
        qualification_enabled: field::column(row, "qualification_enabled")?,
        receipts: field::unsigned(row, "receipts")?,
        stations_seen: field::unsigned(row, "stations_seen")?,
        timely_forecasts: field::unsigned(row, "timely_forecasts")?,
        unknown_requests: field::unsigned(row, "unknown_requests")?,
        valid_forecasts: field::unsigned(row, "valid_forecasts")?,
        valid_station_windows: field::unsigned(row, "valid_station_windows")?,
    })
}
