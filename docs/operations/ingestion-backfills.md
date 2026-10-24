# Ingestion and historical backfills

This runbook preserves provider-specific ingestion and operator commands.
The checked catalogs are the source of truth for station identity, availability,
cadence, and chunk limits. Commands below can contact providers or write to the
configured database unless `--dry-run` is supplied; they are not documentation
validation commands.

For production operations, use the forced-command remote wrappers and the
[Blueberry operations runbook](raspberry-pi.md). Repository-shell commands need
the worker's configured database and provider-file environment variables; see
the [worker configuration](../../apps/worker/src/config.ts). Keep credentials
outside Git.

## First-party Ecowitt ingestion

The checked gateway catalog is
[`config/ecowitt/gateways.json`](../../config/ecowitt/gateways.json). The worker
polls the GW3000 local HTTP API once per minute over the farm LAN and verifies
the gateway MAC before accepting a snapshot. The local live-data API does not
require the gateway UI password.

The adapter normalizes the primary outdoor temperature, apparent temperature,
humidity, relative pressure, wind, rain, solar, UV, black-globe,
wet-bulb-globe, PM2.5, and first soil-channel measurements. The dedicated WH40H
rain gauge is preferred when present; the WS90 piezo gauge is the fallback.
Optional PM2.5 and traditional-rain fields remain absent until reported.
Other repeated temperature, humidity, and soil channels remain at the gateway
boundary rather than being duplicated into one normalized point. See the
[sensor inventory](../sensor-measurements.md).

## WeatherFlow Tempest

The checked station catalog is
[`config/tempest/stations.json`](../../config/tempest/stations.json). Keep the
API key outside Git in `deploy/secrets/weather_tempest_api_key`. The worker
mounts the file and polls every active source once per completed UTC hour.
Each provider range is limited to one day because longer historical requests
are bucketed by Tempest. Distinct minute observations are retained despite the
hourly polling cadence.

Plan all configured station history without provider or database writes:

```bash
docker compose --env-file deploy/.env.example \
  -f deploy/compose.yaml -f deploy/compose.local.yaml run --rm --no-deps worker \
  node apps/worker/dist/tempest-backfill-cli.js \
  --site ballydidean --dry-run
```

Import all available history using each station's checked start date, skipping
only exact chunks that already succeeded:

```bash
npm run weather:tempest:backfill -- --site ballydidean --resume
```

Supported options are `--from`, `--to`, `--chunk-days 1`, `--resume`, `--dry-run`,
and `--report PATH`. Repeat `--station LOCATION_ID` to select a subset.
The default `--to` is yesterday so the scheduled worker owns the partial current
day. For the configured production stack:

```bash
npm run remote:tempest:backfill
```

## Nearby public stations

The checked catalog is
[`config/public-stations/stations.json`](../../config/public-stations/stations.json).
Its five normalized sources cover four physical stations:

| Source | Availability and sampling semantics |
| --- | --- |
| Ambient Weather / Merlin | Live and historical five-minute observations from January 1, 2021 |
| Ambient Weather / MaxWeather | Live five-minute observations |
| Weather Underground / MaxWeather | History from November 29, 2024 through August 23, 2026; the fixed cutoff prevents overlap with Ambient live ownership |
| Netatmo Weathermap | Live and historical five-minute observations from June 21, 2022; module timestamps merge into buckets without inventing missing module values |
| PurpleAir / Samara (The Headlands) | Raw two-minute observations; requested history starts January 1, 2019, while available sensor observations begin May 2019 |

PurpleAir PM2.5 uses the mean of both particulate channels; temperature,
humidity, and pressure retain the available enclosure-sensor measurements.

Plan every historical source without provider or database writes:

```bash
docker compose --env-file deploy/.env.example \
  -f deploy/compose.yaml -f deploy/compose.local.yaml run --rm --no-deps worker \
  node apps/worker/dist/public-stations-backfill-cli.js \
  --site ballydidean --dry-run
```

Import or resume the configured archives:

```bash
npm run weather:public-stations:backfill -- --site ballydidean --resume
npm run remote:public-stations:backfill
```

The two commands target the repository-shell environment and configured remote
stack respectively; select the intended environment rather than running both.
Repeat `--source SOURCE_KEY` to select sources. `--from`, `--to`, `--resume`,
`--dry-run`, and `--report PATH` are supported. Availability and chunk sizes remain
checked configuration: command-line ranges cannot query before a known start
or beyond the Weather Underground handoff cutoff.

Ambient Weather, Weather Underground, and PurpleAir map data use public
web-client interfaces rather than contractual bulk-data APIs. Their adapters
apply bounded retries, response-size limits, exact range filters, pacing, and
resumable chunks. PurpleAir's public map token and Netatmo's official public
Weathermap visitor token are short-lived and are not stored.

## Open-Meteo historical reanalysis

Open-Meteo reanalysis remains hourly because the Historical Weather API exposes
native hourly model output. It is not interpolated into artificial minute
points; rerunning that backfill refreshes the complete hourly archive. Source
configuration lives in
[`config/sites/ballydidean.json`](../../config/sites/ballydidean.json).
Historical forecast anchors and live forecast snapshots have separate
[forecast-governance contracts](forecast-adjustment.md).

## NOAA tide history

The checked [NOAA catalog](../../config/tides/noaa.json) defines two CO-OPS feeds
normalized into `weather_records.water_level_m`, with MLLW datum and UTC time:

- Port Townsend station `9444900`: verified six-minute observed water levels.
- Glendale station `9447814`: local high/low tide predictions.

The worker refreshes each source on its configured cadence. Historical imports
are resumable and chunked within the adapter's request limits:

```bash
npm run weather:tides:backfill -- --site ballydidean --from 2019-01-01 --dry-run
npm run weather:tides:backfill -- --site ballydidean --from 2019-01-01 --resume
```

Use `npm run remote:tides:backfill` for the configured remote stack. Recent
observations and the next seven days of predictions are available from the
[public tide endpoint](https://weather.ballydidean.farm/api/v1/sites/ballydidean/tides).
