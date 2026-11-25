//! Bind the shared archive runner to checked catalog material and typed provider owners.

use std::collections::BTreeMap;

use weather_contracts::{
    PublicStationAdapter, PublicStationConfiguration, PublicStationConfigurationStation,
    PublicStationSourceConfiguration, SourceCapability,
};
use weather_worker::{
    AmbientWeatherBackfillProvider, AmbientWeatherRangeRequest, PublicStationArchiveBackfillBatch,
    PublicStationArchiveBackfillProvider, PublicStationArchiveBackfillSource,
    PublicStationBackfillOperationError, PublicStationBackfillPlanningSource, ScheduledWindow,
    SupplementalPublicStationScheduledRequest, TaskFailure,
};

use crate::SupplementalPublicStationBackfillProvider;
use crate::worker_public_station_dispatch::{material_identity, nullable_text, required_text};

/// Planned source identities paired with the provider bound to that catalog.
pub(crate) type BoundPublicStationArchive<'a, A, S, F> = (
    Vec<PublicStationArchiveBackfillSource>,
    CatalogPublicStationArchiveProvider<'a, A, S, F>,
);

/// Bind durable source identities to checked station material before opening any source session.
pub(crate) fn bind_public_station_archive<A, S, F>(
    configuration: &PublicStationConfiguration,
    planning_sources: Vec<PublicStationBackfillPlanningSource>,
    ambient: A,
    supplemental: S,
    pace: F,
) -> Result<BoundPublicStationArchive<'_, A, S, F>, TaskFailure> {
    let catalog = archive_catalog(configuration);
    let sources = planning_sources
        .into_iter()
        // preserve the source order returned by the checked association query
        .map(|planning| {
            let (station, source) = catalog
                .get(planning.source_key.as_str())
                .ok_or_else(archive_catalog_mismatch)?;
            // reject a source rebound to a different adapter or config version
            if planning.adapter != source.adapter
                || planning.source_config_fingerprint != source.fingerprint
            {
                return Err(archive_catalog_mismatch());
            }
            Ok(PublicStationArchiveBackfillSource {
                planning,
                station_key: station.key.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        sources,
        CatalogPublicStationArchiveProvider {
            catalog,
            ambient,
            supplemental,
            pace,
        },
    ))
}

// retain one stable error for a checked source no longer bound to the original catalog
fn archive_catalog_mismatch() -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: "public_station_backfill_catalog_mismatch".to_owned(),
    }
}

pub(crate) type ArchiveCatalog<'a> = BTreeMap<
    &'a str,
    (
        &'a PublicStationConfigurationStation,
        &'a PublicStationSourceConfiguration,
    ),
>;

// index references to the checked historical catalog once per invocation
pub(crate) fn archive_catalog(configuration: &PublicStationConfiguration) -> ArchiveCatalog<'_> {
    configuration
        .stations
        .iter()
        // retain station provenance alongside each active historical configuration
        .flat_map(|station| {
            station.sources.iter().filter_map(move |source| {
                (source.active && source.capabilities.contains(&SourceCapability::Historical))
                    .then_some((source.key.as_str(), (station, source)))
            })
        })
        .collect()
}

pub(crate) struct CatalogPublicStationArchiveProvider<'a, Ambient, Supplemental, Pace> {
    catalog: ArchiveCatalog<'a>,
    ambient: Ambient,
    supplemental: Supplemental,
    pace: Pace,
}

impl<Ambient, Supplemental, Pace> PublicStationArchiveBackfillProvider
    for CatalogPublicStationArchiveProvider<'_, Ambient, Supplemental, Pace>
where
    Ambient: AmbientWeatherBackfillProvider,
    Supplemental: SupplementalPublicStationBackfillProvider,
    Pace: FnMut(u64),
{
    // bind the exact checked request inside the committed chunk boundary
    fn fetch(
        &mut self,
        source: &PublicStationArchiveBackfillSource,
        window: ScheduledWindow,
        deadline_at_ms: i64,
    ) -> Result<PublicStationArchiveBackfillBatch, PublicStationBackfillOperationError> {
        let &(station, configured) = self
            .catalog
            .get(source.planning.source_key.as_str())
            .ok_or_else(|| {
                PublicStationBackfillOperationError::unexpected(
                    "public-station archive source missing",
                )
            })?;
        let model = nullable_text(&station.model);
        let serial = nullable_text(&station.serial);

        // keep Ambient on its typed device-data range operation
        if configured.adapter == PublicStationAdapter::AmbientWeather {
            let device_id = required_text(&configured.adapterConfig, "deviceId")
                .map_err(archive_material_failure)?;
            let mac_address = required_text(&configured.adapterConfig, "macAddress")
                .map_err(archive_material_failure)?;
            return self
                .ambient
                .fetch(
                    AmbientWeatherRangeRequest {
                        device_id,
                        end_exclusive_ms: window.end_exclusive_ms,
                        mac_address,
                        model,
                        serial,
                        source_id: source.planning.source_id.clone(),
                        start_ms: window.start_ms,
                        timezone: station.timezone.clone(),
                    },
                    deadline_at_ms,
                )
                .map(PublicStationArchiveBackfillBatch::Ambient);
        }

        let identity = material_identity(configured.adapter, &configured.adapterConfig)
            .map_err(archive_material_failure)?
            .ok_or_else(|| {
                PublicStationBackfillOperationError::unexpected(
                    "public-station archive adapter missing",
                )
            })?;
        self.supplemental
            .fetch(
                SupplementalPublicStationScheduledRequest {
                    identity,
                    model,
                    serial,
                    source_id: source.planning.source_id.clone(),
                    timezone: station.timezone.clone(),
                    window,
                },
                deadline_at_ms,
            )
            .map(PublicStationArchiveBackfillBatch::Supplemental)
    }

    // route provider pacing through the process clock effect
    fn pace_between_chunks(&mut self, delay_ms: u64) {
        (self.pace)(delay_ms);
    }
}

// reflect invalid typed material as the original unexpected chunk failure
fn archive_material_failure(_failure: TaskFailure) -> PublicStationBackfillOperationError {
    PublicStationBackfillOperationError::unexpected("invalid public-station archive material")
}
