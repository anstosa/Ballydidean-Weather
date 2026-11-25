use weather_contracts::{
    ForecastObservationInput, ForecastObservationInputRowSourceKindEnum, Nullable,
};

use crate::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};

/// maximum serialized private envelope admitted by the transport owner
pub const FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// one admitted fixed private projection geometry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedForecastObservationGeometry {
    pub projection_from: CanonicalInstant,
    pub query_from: CanonicalInstant,
}

/// admitted site-calendar and offset calculations for the private projection
pub trait ForecastObservationGeometryBoundary {
    /// resolve the expected fixed projection and query start instants
    fn resolve_expected_geometry(
        &self,
        data_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<ResolvedForecastObservationGeometry, CurrentProjectionError>;
}

/// exact private response contract literal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastObservationInputResponseContractVersionLiteral;

impl ForecastObservationInputResponseContractVersionLiteral {
    pub const WIRE_VALUE: &'static str = "forecast-observation-input/v1";
}

/// one compact private observation metric projection
#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastObservationInputResponseMetrics {
    pub apparentTemperatureC: Nullable<f64>,
    pub cloudCoverPercent: Nullable<f64>,
    pub pm25MicrogramsPerCubicMeter: Nullable<f64>,
    pub precipitationRateMmPerHour: Nullable<f64>,
    pub pressureHpa: Nullable<f64>,
    pub relativeHumidityPercent: Nullable<f64>,
    pub solarRadiationWm2: Nullable<f64>,
    pub temperatureC: Nullable<f64>,
    pub uvIndex: Nullable<f64>,
    pub windDirectionDegrees: Nullable<f64>,
    pub windGustMps: Nullable<f64>,
    pub windSpeedMps: Nullable<f64>,
}

/// one compact private observation provenance projection
#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastObservationInputResponseProvenance {
    pub providerKey: String,
    pub sourceId: String,
    pub sourceKey: String,
    pub sourceKind: ForecastObservationInputRowSourceKindEnum,
    pub stationSlug: String,
}

/// one compact private observation row
#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastObservationInputResponseRecord {
    pub id: String,
    pub metrics: ForecastObservationInputResponseMetrics,
    pub provenance: ForecastObservationInputResponseProvenance,
    pub receivedAt: String,
    pub validAt: String,
}

/// one fixed private site identity
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastObservationInputResponseSite {
    pub slug: String,
    pub timezone: String,
}

/// one transport-independent private forecast observation envelope
#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastObservationInputResponse {
    pub contractVersion: ForecastObservationInputResponseContractVersionLiteral,
    pub generatedAt: String,
    pub projectionFrom: String,
    pub queryFrom: String,
    pub records: Vec<ForecastObservationInputResponseRecord>,
    pub site: ForecastObservationInputResponseSite,
    pub through: String,
}

/// project one fixed private repository input to its exact compact DTO
pub fn project_forecast_observation_input(
    input: ForecastObservationInput,
    data_at: &CanonicalInstant,
    geometry_boundary: &dyn ForecastObservationGeometryBoundary,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<ForecastObservationInputResponse, CurrentProjectionError> {
    let timezone = weather_contracts::ForecastObservationInputTimezoneLiteral::WIRE_VALUE;
    let expected = geometry_boundary.resolve_expected_geometry(data_at, timezone)?;
    let projection_from = time_resolver.resolve(&input.projectionFrom)?;
    let query_from = time_resolver.resolve(&input.queryFrom)?;
    let through = time_resolver.resolve(&input.through)?;

    // bind the repository result to this request's one fixed geometry
    if through != *data_at
        || projection_from != expected.projection_from
        || query_from != expected.query_from
    {
        return Err(CurrentProjectionError {
            code: "forecast_observation.geometry",
            detail: "Forecast observation input geometry is invalid".to_owned(),
        });
    }
    let mut records = Vec::with_capacity(input.records.len());

    // retain repository order while projecting only edge-consumed fields
    for row in input.records {
        records.push(ForecastObservationInputResponseRecord {
            id: row.id,
            metrics: ForecastObservationInputResponseMetrics {
                apparentTemperatureC: canonical_metric(row.apparentTemperatureC),
                cloudCoverPercent: canonical_metric(row.cloudCoverPercent),
                pm25MicrogramsPerCubicMeter: canonical_metric(row.pm25MicrogramsPerCubicMeter),
                precipitationRateMmPerHour: canonical_metric(row.precipitationRateMmPerHour),
                pressureHpa: canonical_metric(row.pressureHpa),
                relativeHumidityPercent: canonical_metric(row.relativeHumidityPercent),
                solarRadiationWm2: canonical_metric(row.solarRadiationWm2),
                temperatureC: canonical_metric(row.temperatureC),
                uvIndex: canonical_metric(row.uvIndex),
                windDirectionDegrees: canonical_metric(row.windDirectionDegrees),
                windGustMps: canonical_metric(row.windGustMps),
                windSpeedMps: canonical_metric(row.windSpeedMps),
            },
            provenance: ForecastObservationInputResponseProvenance {
                providerKey: row.providerKey,
                sourceId: row.sourceId,
                sourceKey: row.sourceKey,
                sourceKind: row.sourceKind,
                stationSlug: row.stationSlug,
            },
            receivedAt: time_resolver.resolve(&row.lastReceivedAt)?.utc,
            validAt: time_resolver.resolve(&row.validAt)?.utc,
        });
    }

    Ok(ForecastObservationInputResponse {
        contractVersion: ForecastObservationInputResponseContractVersionLiteral,
        generatedAt: data_at.utc.clone(),
        projectionFrom: projection_from.utc,
        queryFrom: query_from.utc,
        records,
        site: ForecastObservationInputResponseSite {
            slug: weather_contracts::ForecastObservationInputSiteSlugLiteral::WIRE_VALUE.to_owned(),
            timezone: timezone.to_owned(),
        },
        through: through.utc,
    })
}

/// normalize non-finite storage metrics to the JSON null contract
fn canonical_metric(value: Nullable<f64>) -> Nullable<f64> {
    // retain only finite public metric values
    match value {
        Nullable::Value(value) if value.is_finite() => Nullable::Value(value),
        _ => Nullable::Null,
    }
}
