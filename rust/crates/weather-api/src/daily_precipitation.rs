use weather_contracts::{
    ApiDailyPrecipitation, ApiDailyPrecipitationResponse, ApiDailyPrecipitationSource, ApiSite,
    DailyPrecipitationQuery, DailyPrecipitationRow, Nullable,
};

use crate::current_projection::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};

/// pre-resolved local-day query window
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDailyPrecipitationWindow {
    pub from: CanonicalInstant,
    pub to: CanonicalInstant,
}

/// build one active-site daily precipitation query
pub fn daily_precipitation_query(
    site: &ApiSite,
    window: &ResolvedDailyPrecipitationWindow,
) -> Result<DailyPrecipitationQuery, CurrentProjectionError> {
    // require the pre-resolved local-day window to be ordered
    if window.from.epoch_milliseconds >= window.to.epoch_milliseconds {
        return Err(CurrentProjectionError {
            code: "daily_precipitation.window",
            detail: "daily precipitation from must be earlier than to".to_owned(),
        });
    }

    Ok(DailyPrecipitationQuery {
        from: window.from.utc.clone(),
        siteSlug: site.slug.clone(),
        to: window.to.utc.clone(),
    })
}

/// project one nullable daily precipitation read
pub fn project_daily_precipitation_response(
    row: Option<DailyPrecipitationRow>,
    site: &ApiSite,
    generated_at: &CanonicalInstant,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<ApiDailyPrecipitationResponse, CurrentProjectionError> {
    let data = match row {
        None => Nullable::Null,
        Some(row) => {
            // reject a row outside the selected active site
            if !site_contains_source(site, &row.sourceId, &row.stationSlug) {
                return Err(CurrentProjectionError {
                    code: "daily_precipitation.source",
                    detail: format!(
                        "Missing active metadata for daily precipitation source {}",
                        row.sourceId
                    ),
                });
            }

            // preserve the finite public accumulation contract
            if !row.accumulationMm.is_finite() {
                return Err(CurrentProjectionError {
                    code: "daily_precipitation.accumulation",
                    detail: "daily precipitation accumulation must be finite".to_owned(),
                });
            }

            Nullable::Value(ApiDailyPrecipitation {
                accumulationMm: row.accumulationMm,
                source: ApiDailyPrecipitationSource {
                    sourceId: row.sourceId,
                    stationSlug: row.stationSlug,
                },
                validThrough: time_resolver.resolve(&row.validThrough)?.utc,
            })
        }
    };

    Ok(ApiDailyPrecipitationResponse {
        data,
        generatedAt: generated_at.utc.clone(),
        site: site.clone(),
    })
}

/// locate one source under its active station
fn site_contains_source(site: &ApiSite, source_id: &str, station_slug: &str) -> bool {
    site.stations.iter().any(|station| {
        station.slug == station_slug && station.sources.iter().any(|source| source.id == source_id)
    })
}
