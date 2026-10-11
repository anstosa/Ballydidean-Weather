# Forecast adjustment operations and governance

For the current admin-only temperature, wind/gust and rain scorecard and bounded
review-only research workflows, see [Adjustment performance and review](adjustment-performance.md).
Measurement does not authorize a serving-model replacement or band disablement.

Weather can evaluate a hyperlocal correction for the existing Open-Meteo v4
forecast from a frozen network of eleven physical weather stations. The initial
registry is intentionally inactive:

```json
{"activeBundle":null,"contractVersion":"forecast-adjustment-registry/v1"}
```

The first production-derived evaluation is expected to return
`insufficient_data`. Do not lower a data, skill, bootstrap, or network gate
to create an active model. A model becomes active only through a reviewed
repository change, a new server image, deployment, and API restart.

The pre-activation full-network policy reset uses v2 candidate, evaluation
report, qualification receipt, and runtime bundle contracts. V1 artifacts are
not accepted. The public decision and registry contracts remain v1 because
their wire shapes and semantics did not change; the inactive registry means no
active artifact requires compatibility migration.

## Training-data contract

Every physical station is eligible for the same five metrics when a value is
present and passes its source-specific checks:

- `temperatureC` (`T`)
- `relativeHumidityPercent` (`RH`)
- `windSpeedMps` (`WS`)
- `windGustMps` (`WG`)
- `windDirectionDegrees` (`WD`)

The source intervals below are half-open UTC intervals; `-infinity` and
`+infinity` are literal unbounded endpoints. The `Metrics` column is literal;
apparent temperature, pressure, precipitation, cloud cover, UV, PM2.5,
soil/property measurements, and radiation cannot enter the v1 model.

| Physical station | Provider family | Accepted source and adapter contract | Accepted UTC interval `[start,end)` | QC | Metrics | Checked fingerprint |
| --- | --- | --- | --- | --- | --- | --- |
| `ambient-maxweather` | `ambient` | `wunderground-maxweather-history-v1`; `wunderground-pws-history/v1` | `[2024-11-29T00:00:00Z,2026-08-24T00:00:00Z)` | status absent or `provider_qc_1`; no flags | T, RH, WS, WG, WD | `52dda6c5444d0a234fbe23d6218027d417ac966ecf291a7d5dfff42fd0dc207c` |
| `ambient-maxweather` | `ambient` | `ambient-maxweather-observations-v1`; `ambient-device-data/v1` | `[2026-08-24T00:00:00Z,+infinity)` | status absent; no flags | T, RH, WS, WG, WD | `7a7528a6278924ca5280a1a6045b6647b7e660b112d7fa3008c542a17ff99df4` |
| `ambient-merlin` | `ambient` | `ambient-merlin-observations-v1`; `ambient-device-data/v1` | `[2021-01-01T00:00:00Z,+infinity)` | status absent; no flags | T, RH, WS, WG, WD | `c3829701bfc25a050022dc3965569d3a87376e8a43b5fdcb7621533f1ae3c65d` |
| `ballydidean-ecowitt` | `ecowitt` | `ecowitt-88f15505d89f-local-live-v1`; `ecowitt-local-live/v1` | `[-infinity,+infinity)` | status absent; no flags | T, RH, WS, WG, WD | `0a44488714d0fa807b924f8aea14965b437722e8cf9f8eae4bc8c81da8a0149d` |
| `netatmo-nearby` | `netatmo` | `netatmo-nearby-observations-v1`; `netatmo-public-measures/v1` | `[2022-06-21T00:00:00Z,+infinity)` | status absent; no flags | T, RH, WS, WG, WD | `5495917dd2465a32d9878e73c68781a229b432901cdc4867875726351efbdbbc` |
| `tempest-126537` | `tempest` | `tempest-126537-observations-v2`; `tempest-observations/v2` | `[2023-12-17T00:00:00Z,+infinity)` | status absent; only `uv_index_out_of_range` flag allowed | T, RH, WS, WG, WD | `34dafbd6584c93d55ed4d3d43dc7e74a0876165d4ddfc921413f7b826dff7ab7` |
| `tempest-168853` | `tempest` | `tempest-168853-observations-v2`; `tempest-observations/v2` | `[2025-01-22T00:00:00Z,+infinity)` | status absent; only `uv_index_out_of_range` flag allowed | T, RH, WS, WG, WD | `1c7a402337a44a5441775246cbc02da7994599dad6ab83dc04b248303facfea5` |
| `tempest-201058` | `tempest` | `tempest-201058-observations-v2`; `tempest-observations/v2` | `[2025-12-22T00:00:00Z,+infinity)` | status absent; only `uv_index_out_of_range` flag allowed | T, RH, WS, WG, WD | `a61cce798cddf682da9608dc245659fc7734a6d5304068939d3115ae7d81a50e` |
| `tempest-203055` | `tempest` | `tempest-203055-observations-v2`; `tempest-observations/v2` | `[2025-12-25T00:00:00Z,+infinity)` | status absent; only `uv_index_out_of_range` flag allowed | T, RH, WS, WG, WD | `9ead4c5359a6a9640f334be91397180aa62b90b0f0ce813b9ff26fe84537acc4` |
| `tempest-225947` | `tempest` | `tempest-225947-observations-v2`; `tempest-observations/v2` | `[2026-07-14T00:00:00Z,+infinity)` | status absent; only `uv_index_out_of_range` flag allowed | T, RH, WS, WG, WD | `b4dd6105d9a56a7c5d0dc4063f830e1cf28d693222a8de15536dd83d3a6178c4` |
| `tempest-38270` | `tempest` | `tempest-38270-observations-v2`; `tempest-observations/v2` | `[2021-01-04T00:00:00Z,+infinity)` | status absent; only `uv_index_out_of_range` flag allowed | T, RH, WS, WG, WD | `ce162067aced4ab3522fb83145a21e608ff24dec189097726188e96fd6cca52f` |
| `tempest-64255` | `tempest` | `tempest-64255-observations-v2`; `tempest-observations/v2` | `[2021-12-10T00:00:00Z,+infinity)` | status absent; only `uv_index_out_of_range` flag allowed | T, RH, WS, WG, WD | `8eb488a358375fc3526347d9ef6c9f23080095a22ea874a42ec400b0317d868a` |

Each `tempest-<location>-observations-v1` lineage has an empty accepted interval
and cannot fill a v2 gap. Weather Underground owns MaxWeather before
`2026-08-24T00:00:00Z`; Ambient owns it at and after that instant. Out-of-range,
superseded, unknown-QC, and missing values remain excluded. A surviving
duplicate for one physical station, valid instant, and metric is a hard
collision rather than an averaging opportunity.

The exact excluded source list is:

```text
tempest-126537-observations-v1
tempest-168853-observations-v1
tempest-201058-observations-v1
tempest-203055-observations-v1
tempest-225947-observations-v1
tempest-38270-observations-v1
tempest-64255-observations-v1
```

Instant metrics use the closest eligible observation in
`[validAt - 5 minutes, validAt + 5 minutes)`, with an equal-distance tie going
to the earlier observation. Gust uses the maximum over
`(validAt - 1 hour, validAt]` only when both boundaries and every internal gap
are covered within ten minutes. Direction requires paired station wind speed of
at least `1 m/s`.

The network target requires at least three unique eligible stations and at
least one of the three nearest eligible stations. Available positive Haversine
weights use Earth radius `6,371,008.8 m` and
`1 / (1 + (distanceMeters / 2,000)^2)`, normalized in station-key order. Scalar
targets use the deterministic weighted median; direction uses the weighted
vector mean and becomes missing below resultant length `0.25`. Each resulting
`(validAt, metric, cohort, lead band)` network event has total model weight one.

The station/source/coordinate/spatial hashes are immutable training provenance.
The runtime loader verifies their internal bundle links but does not compare an
active candidate with the current station catalogs. A later station or source
change starts a new lineage and requires retraining; it does not silently alter
or disable already reviewed bundle bytes.

## Previous Runs and live v4 isolation

`open-meteo-previous-runs-v1` is historical-only. Its UTC backfill writes exact
24, 48, 72, 96, 120, 144, and 168-hour anchors to
`forecast_anchor_records` with cohort `fixed_lead_anchor`. An anchor has no
invented model-run or retrieval instant.

Plan the archive without provider or database writes:

```bash
npm run weather:backfill -- \
  --site ballydidean \
  --source open-meteo-previous-runs-v1 \
  --from 2024-01-01 \
  --to 2026-08-31 \
  --chunk-days 14 \
  --dry-run
```

Add `--resume` only when importing the separately authorized archive. A chunk
failure stops later chunks; rerun with `--resume` after correcting the cause.
Do not start the complete production backfill as part of model evaluation.

The scheduled `open-meteo-forecast-v4` source remains the only live forecast
input. Its `weather_records` projection uses cohort
`legacy_v4_retrieval_snapshot`, truthful retrieval-time provenance, and lead
bands 1–24 through 145–168 hours. The live query requires forecast capability,
so Previous Runs cannot enter `/forecast` or public history. Fixed-anchor and
retrieval-snapshot counts, fits, scores, coefficients, and evidence remain
separate. Only `legacy_v4_retrieval_snapshot` can qualify a correction served
beside v4. There is no v5 source, source cutover, scheduled duplication, or
transfer of anchor coefficients into v4.

## Bounded wind transfer canary

The previous canary is retired. The committed canary registry now has
`activeBundle: null`, and the old content-addressed bundle remains unchanged
for audit only. The normal qualified registry is also inactive, so these
committed selections serve raw forecasts for every metric. Registry changes
take effect through a reviewed image deployment, not by editing a running
service's files.

The wind canary is a separately versioned exception path, not a qualified v2
model. It fits fixed-lead Previous Runs against the same frozen all-station
network target, then scores those coefficients against the separate retained
live-v4 retrieval cohort. The retired canary enabled exactly:

- `windSpeedMps`
- `windGustMps`

Temperature, relative humidity, and wind direction were never enabled in that
canary. An enabled metric-band requires at least 30 scoreable
live-v4 bridge events and strictly positive point skill. This small transfer
cohort does not satisfy the normal seven-local-date moving-block bootstrap or
the full qualified-v2 evidence graph, so its artifacts must never be placed in
the normal bundle registry or described as qualified.

The isolated selection is:

```text
config/forecast-adjustments/ballydidean-wind-canary.json
config/forecast-adjustments/ballydidean/wind-canary-bundles/sha256-<bundleSha256>.json
```

Each bundle contains a candidate, live-v4 transfer report, and explicit
operator authorization. Authorization begins no earlier than its review time,
expires within 14 days, and is enforced at every application. The API reads the
canary once at startup and prefers it only while active. Any expiry, explicit
kill, missing bundle, invalid hash, lineage drift, runtime fingerprint drift,
or application error fails raw without falling through to another adjustment.
Only a genuinely inactive canary registry permits the normal qualified loader
to run.

Set the following deployment variable to the literal value `1`, then restart
the API, for an immediate fail-raw kill:

```dotenv
WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH=1
```

Only `1` activates the kill switch; `0` permits normal operation. Rollback may also
set the canary registry's `activeBundle` to `null` in a reviewed image. Do not
edit an existing content-addressed bundle. New UI sessions default to **Adjusted**;
an explicitly saved raw preference is retained. The switch keeps the same label
in both states, and Forecast has no adjustment infobox. Forecast alert thresholds
and displayed readings use the selected adjusted or raw values, including wind
gusts. A negative adjustment can clear a forecast warning. With Adjusted on,
current conditions retain the on-site-first source priority. With the switch off
(raw mode), current cards, pressure tendency, alerts, and the Now artwork use only
regional `model_current` records. Missing regional records or metrics remain
unavailable; local and nearby station readings never fill regional gaps. Artwork
caches are mode-scoped so saved local conditions cannot leak into raw mode.
The observed daily rain accumulation is unavailable in raw mode, and the
homepage's indoor and soil panels are hidden. Dedicated Map, Trends, Logs, and
Admin sensor views are unchanged. Current readings from the selected source can
still trigger their own warnings. Missing, disabled, or invalid forecast
adjustments fall back to raw forecast values without changing the selected
current-source mode.

Temperature, wind, and rain icons turn gold when the selected Adjusted mode has
active corrections for that family within the displayed forecast hours. Raw,
unavailable, disabled, and loading families retain their normal icon color.
Gold marks the selected adjustment family; it does not correct current readings
or change the metrics plotted in a chart. Switching the mode also selects the
current source as described above.

## Experimental WS90 sunlight cloud correction

With Adjusted on, the dashboard can raise or lower the current regional cloud
estimate and the next 24 hours of Open-Meteo cloud forecasts using the on-site
WS90 `solarRadiationWm2` reading. The Clouds card, its daily extrema and clearest
window and the forecast cloud chart use the selected forward values. Current
cover and live Now artwork require a fresh qualifying daytime reading; they
remain regional at night even when a daytime forecast correction is retained. Changed cloud icons are gold; the card identifies the sunlight estimate
without experimental wording. The proxy remains experimental and unqualified;
removing the display label does not promote a model. Raw/off mode remains
regional-only for current and future conditions.

This is an instantaneous sunlight proxy, not a measured fraction of the sky
and not a trained, qualified, or promoted adjustment family. Temperature, wind,
rain, their admin switches, public API records, history, map imagery, and widget
v2/v3 contracts are unchanged. The global display switch remains available even
when all three governed families are disabled, so the experiment can be turned
off without altering their exact settings contract.

The browser selects only `ecowitt-local` physical readings from
`ballydidean-ecowitt`, paired with Open-Meteo `model_current` cloud cover. A
GW3000 device label denotes the gateway, not a different radiation sensor.
Nearby stations, inverter feeds, and modeled radiation cannot substitute.
Both samples must be explicitly fresh, with valid timezone-bearing timestamps,
nonempty provenance identifiers, reception no earlier than observation and no
future timestamps. Observation and reception ages are at most 5 minutes for
the WS90 and 20 minutes for the model, with at most 20 minutes between their
valid times. Generic API freshness alone is insufficient.

At the observation and evaluation times, solar elevation must be at least
15 degrees and expected clear-sky irradiance at least 200 W/m². Radiation must
be greater than 5 and no greater than 2500 W/m²; ratios above 1.5 times clear-sky
irradiance are rejected. Missing, stale, delayed, suspicious, nighttime, and
low-sun inputs preserve raw current cloud values. A previously qualifying daytime
pair can still correct future hours until its original expiry. These conservative experimental
gates are defaults, not empirically calibrated thresholds.

Solar position follows [NOAA's solar calculations](https://gml.noaa.gov/grad/solcalc/calcdetails.html).
Clear-sky horizontal irradiance uses the Haurwitz expression
`1098 × cos(zenith) × exp(-0.059 / cos(zenith))`, as implemented by
[pvlib](https://github.com/pvlib/pvlib-python/blob/main/pvlib/clearsky.py).
The cloud proxy is
`100 × sqrt(clamp((1 - min(1, radiation / (0.85 × clearSky))) / 0.65, 0, 1))`.
The attenuation relationship is adapted from
[USACE's computed cloud cover method](https://www.hec.usace.army.mil/confluence/wqetm/input-data-sets/meteorological-data),
which uses daily radiation; this instantaneous adaptation is not validated by
that reference. The 15% clear-sky tolerance, ±80 percentage-point bias cap, and
24-hour fade are explicit experimental choices, not fitted coefficients.

The bias is proxy minus paired regional cloud cover, capped to ±80 points.
Current cover applies that bias at evaluation time, not the older modeled
observation time. A future hour receives
`bias × max(0, 1 - (hour - observationTime) / 24 hours)`, clamped to 0–100%.
Past hours and hours at or beyond `observationTime + 24 hours` remain exact raw.
Re-rendering an unchanged reading never slides its horizon forward. Each new
accepted reading replaces the bias, including a genuine zero correction. When
current inputs stop qualifying, Now retires to regional values while only future
hours retain the newest qualifying daytime correction until its original expiry.
Reloads and new nighttime visitors recover the same compact source pair from
`GET /api/v1/sites/ballydidean/solar-cloud-evidence`, not browser storage. The
versioned response contains original scalar values, observation/reception times
and exact WS90/regional source identities, never a serialized bias or private
sensor metadata. The browser re-evaluates daylight and sample ages at the original
maximum reception time; response generation, refetching and rendering cannot
renew expiry. Wrong-source, future, invalid or expired evidence fails raw.

Visible Home and Forecast pages refresh `/current` once per minute and
on connectivity or visibility resume. The optional queryless daylight-evidence
read is independently throttled to five minutes and singleflight, never blocks
required weather products, and is not refetched solely by adjustment toggles or
forecast range changes. It shares the completed bounded archive fill with
`forecast-observations`; failures retain genuine last-good evidence only until
its fixed original expiry. Requests do not overlap, late responses
from former routes/full loads are ignored, and failures retain last-good raw
records while re-evaluating correction expiry. Forecast selection is retained
across these current-only redraws. Artwork
caches deliberately store raw modeled cloud cover, never the experimental
correction, so offline or unrelated-route artwork cannot extend its lifetime.

Shading, dirty sensors, haze, smoke, humidity, and cloud-edge enhancement can
change radiation without changing sky fraction. No independent cloud truth or
forecast-accuracy improvement has been established. Evaluate against observed
sky conditions before tuning these defaults or treating the proxy as a model.

## Bounded production snapshot

Run the production export only with explicit authority. The command accepts
exactly two inclusive Ballydidean local dates and a loaded deployment SSH agent:

```bash
FROM_DATE="${FROM_DATE:?set the approved first local date}"
TO_DATE="${TO_DATE:?set the approved last local date}"
deploy/scripts/pull-forecast-training-export.sh "$FROM_DATE" "$TO_DATE"
```

Both operands must use `YYYY-MM-DD`, be ordered, and span at most 450 dates.
The forced SSH path accepts no source, site, SQL, database URL, output path, or
extra option. The database session uses only `weather_training_export`, whose
default is read-only and whose grants are limited to the two sanitized export
views. The fixed query begins a `REPEATABLE READ READ ONLY` transaction with a
15-minute statement timeout, five-second lock timeout, and 30-second idle
transaction timeout.

The exporter aborts before publication at row 4,000,001. Its conservative cap
oracle is:

```text
450 × ((24 retrievals × 264 raw horizon rows) +
       (11 stations × 24 hourly aggregates) +
       168 fixed anchors) = 3,045,600 export rows
4,000,000 - 3,045,600 = 954,400 rows of headroom
```

These are export rows before training matching, not eligible training-event
counts.

The pull verifies the package and atomically publishes it at
`.weather-data/<manifest-sha256>/`. It rejects traversal, special archive
nodes, schema/source/hash drift, tampering, an existing destination, and a
raced destination. Interrupted work remains under `.partial.*` and is removed.

Set the digest printed by the pull, then inspect only the sanitized control
plane:

```bash
SNAPSHOT_SHA256="${SNAPSHOT_SHA256:?set the printed manifest SHA-256}"
SNAPSHOT=".weather-data/${SNAPSHOT_SHA256}"
node --input-type=module - "$SNAPSHOT/manifest.json" <<'NODE'
import { readFile } from "node:fs/promises";

const manifest = JSON.parse(await readFile(process.argv[2], "utf8"));
const manifestSha256 = process.argv[2].split("/").at(-2);

// enforce the sanitized export boundary
if (
  manifest.transaction.readOnly !== "on" ||
  manifest.transaction.isolationLevel !== "repeatable read" ||
  manifest.limits.maxDays !== 450 ||
  manifest.limits.maxRows !== 4_000_000 ||
  manifest.totalRowCount > manifest.limits.maxRows
) {
  throw new Error("forecast-training export boundary failed");
}

process.stdout.write(`${JSON.stringify({
  aggregationContractSha256: manifest.aggregationContractSha256,
  coordinateManifestSha256: manifest.coordinateManifestSha256,
  fromLocalDate: manifest.fromLocalDate,
  manifestSha256,
  metricEligibilitySha256: manifest.metricEligibilitySha256,
  migrationHistorySha256: manifest.migrationHistorySha256,
  queryContractSha256: manifest.queryContractSha256,
  rowSchemaSha256: manifest.rowSchemaSha256,
  sourceLineageSha256: manifest.sourceLineageSha256,
  spatialWeightsSha256: manifest.spatialWeightsSha256,
  stationManifestSha256: manifest.stationManifestSha256,
  toLocalDate: manifest.toLocalDate,
  totalRowCount: manifest.totalRowCount,
  transaction: manifest.transaction,
}, null, 2)}\n`);
NODE
```

Export approval requires all of the following zero-write evidence:

1. fresh disposable-database tests prove that `weather_training_export` cannot
   run `INSERT`, `UPDATE`, `DELETE`, DDL, sequence operations, base-table reads,
   or application/reachable non-system `SECURITY DEFINER` functions;
2. the production package reports `readOnly=on`,
   `isolationLevel="repeatable read"`, and the exact local timeouts;
3. the forced query contains only the reviewed `SELECT`/`COPY TO STDOUT` path;
4. the production export event records no affected rows or model/ingestion
   mutation attributable to the export session; and
5. the repository and runtime registry remain unchanged by the export.

The forced interface deliberately exposes no arbitrary post-export SQL. Do not
weaken it to collect evidence. Use the already-authorized database audit or
session-scoped counters for item 4; if that evidence is unavailable, do not
claim zero production writes.

The broad local privilege and lifecycle gate is:

```bash
npm run test:deploy
```

## Deterministic inactive evaluation

The ignored local evaluator accepts only one direct child of `.weather-data/`
as input and one direct child of `.weather-models/` as output. It accepts no
database URL. Record the evidence and bundle inventories before the run:

```bash
find "${HOME}/.weather/model-evidence" -type f -printf '%P %s\n' \
  2>/dev/null | sort > /tmp/weather-model-evidence.before
find .weather-models/bundle-staging -type f -printf '%P %s\n' \
  2>/dev/null | sort > /tmp/weather-bundle-staging.before
```

Run the same verified snapshot twice. Exit code `2` is the expected
`insufficient_data` result:

```bash
evaluation_tag="$(date -u +%Y%m%d-%H%M%S)"
evaluation_a=".weather-models/production-eval-${evaluation_tag}-a"
evaluation_b=".weather-models/production-eval-${evaluation_tag}-b"
set +e
npm run weather:forecast-adjustment:evaluate -- \
  --snapshot ".weather-data/${SNAPSHOT_SHA256}" \
  --output "$evaluation_a"
first_exit=$?
npm run weather:forecast-adjustment:evaluate -- \
  --snapshot ".weather-data/${SNAPSHOT_SHA256}" \
  --output "$evaluation_b"
second_exit=$?
set -e
test "$first_exit" -eq 2
test "$second_exit" -eq 2
cmp \
  "$evaluation_a/insufficient-data.json" \
  "$evaluation_b/insufficient-data.json"
sha256sum \
  "$evaluation_a/insufficient-data.json" \
  "$evaluation_b/insufficient-data.json"
```

The two files must be byte-identical, contain the same `reportSha256`, and name
every failed gate. Stable insufficiency reasons include
`epoch_402_local_dates`, `network_330_local_dates`, network/season/pair support,
and development LOSO qualification. The network-date upper bound counts only
distinct `legacy-v4-retrieval` member dates; fixed-lead anchors cannot satisfy
it. No individual station is a manifest-level hard veto.

Confirm the inactive and no-promotion invariants:

```bash
node --input-type=module <<'NODE'
import { readFile } from "node:fs/promises";

const registry = JSON.parse(
  await readFile("config/forecast-adjustments/ballydidean.json", "utf8"),
);

// require the inactive reviewed registry
if (
  registry.contractVersion !== "forecast-adjustment-registry/v1" ||
  registry.activeBundle !== null
) {
  throw new Error("forecast-adjustment registry unexpectedly active");
}
NODE
find "${HOME}/.weather/model-evidence" -type f -printf '%P %s\n' \
  2>/dev/null | sort > /tmp/weather-model-evidence.after
find .weather-models/bundle-staging -type f -printf '%P %s\n' \
  2>/dev/null | sort > /tmp/weather-bundle-staging.after
cmp /tmp/weather-model-evidence.before /tmp/weather-model-evidence.after
cmp /tmp/weather-bundle-staging.before /tmp/weather-bundle-staging.after
git check-ignore -q .weather-data .weather-models
test -z "$(git ls-files .weather-data .weather-models)"
rm -f \
  /tmp/weather-model-evidence.before \
  /tmp/weather-model-evidence.after \
  /tmp/weather-bundle-staging.before \
  /tmp/weather-bundle-staging.after
```

An ignored local snapshot is valid only for an insufficient-data dry run. If
the evaluator finds sufficient metadata, it stops before fitting and requires
durable external retention. Move the snapshot into the approved evidence store
or delete it; never continue candidate or holdout work from `.weather-data/`.
No raw production observation, snapshot member, event loss, or credential may
enter Git.

## Inactive full-history research fit

The full-history research fitter is separate from qualified v2 and the wind
canary. It accepts two or more independently verified export packages under one
external retained-evidence or ephemeral decrypted root. Each child remains an
independent `REPEATABLE READ READ ONLY` export with the 450-date and 4,000,000-row
limits. The fitter does not merge their manifests or claim that the children
came from one atomic database snapshot.

Supply the child paths in chronological order and bind the required first and
last local date explicitly. Their declared local-date ranges must be
nonoverlapping and exactly contiguous. Site, query, row schema,
migration, training provenance, and the complete declared source identity
catalog must match. Every observed source inventory is reconstructed from the
verified rows and compared with its child manifest. A boundary gap, overlap,
source drift, recomputed manifest, member checksum change, row collision, or
partition mismatch aborts the fit.

Build the package, then write the returned canonical research object only to
approved non-Git evidence storage:

```bash
npm run build --workspace @weather/forecast-adjustment
EVIDENCE_ROOT="${EVIDENCE_ROOT:?set the retained or decrypted evidence root}"
OUTPUT="${OUTPUT:?set a non-Git research result path}"
FROM_DATE="${FROM_DATE:?set the required complete-history first date}"
TO_DATE="${TO_DATE:?set the required complete-history last date}"
SNAPSHOT_A="${SNAPSHOT_A:?set the first manifest-hash directory}"
SNAPSHOT_B="${SNAPSHOT_B:?set the second manifest-hash directory}"
SNAPSHOT_C="${SNAPSHOT_C:?set the third manifest-hash directory}"
node --input-type=module - \
  "$EVIDENCE_ROOT" "$OUTPUT" "$FROM_DATE" "$TO_DATE" \
  "$SNAPSHOT_A" "$SNAPSHOT_B" "$SNAPSHOT_C" <<'NODE'
import { writeFile } from "node:fs/promises";
import {
  canonicalJsonBytes,
  fitRetainedForecastAdjustmentFullHistoryResearch,
} from "@weather/forecast-adjustment";

const [evidenceRoot, output, fromLocalDate, toLocalDate, ...snapshotPaths] =
  process.argv.slice(2);
const result = await fitRetainedForecastAdjustmentFullHistoryResearch({
  evidenceRoot,
  expectedRange: { fromLocalDate, toLocalDate },
  snapshotPaths,
});
await writeFile(output, canonicalJsonBytes(result), { mode: 0o600, flag: "wx" });
process.stdout.write(`${result.researchArtifactSha256}\n`);
NODE
```

The fitter reads and verifies one complete local-date partition at a time so
station matching and forecast collision handling remain date-atomic without
retaining the entire raw export corpus. It derives all five supported metrics
and all seven fixed-anchor lead bands with the frozen
`robust-hierarchical-median/v1` algorithm, then refits coefficients and scalar
training envelopes on every eligible fixed-anchor network event. The result
binds every child manifest SHA-256, the separate fixed-anchor training and
live-v4 served identities, per-pair matched counts and date ranges, whole-range
and per-pair missing local dates, and SHA-256 identities of the built fitter,
algorithm, calendar, and domain adjustment-policy modules. Missing dates remain
missing; they are not imputed or presented as provider coverage.

Both reported scores are descriptive diagnostics, not qualification evidence:

- the archive diagnostic scores the 30 calendar dates immediately before the
  first live-v4 local date, using a different model fitted only before a full
  seven-date embargo; missing archive dates are reported explicitly; and
- the live-v4 diagnostic scores from the earliest retained live-v4 row onward,
  using another different fixed-anchor model fitted only before its full
  seven-date embargo.

Those dates have already been inspected and are not an untouched holdout. The
final all-history refit includes later fixed anchors and is therefore not either
diagnostic model. Its exact bytes have no independent live-v4 validation. The
result is always `promotable=false`, `productionActivationAllowed=false`,
`runtimeBundleCreated=false`, and `qualificationStatus="not_qualified"`.
It creates no candidate v2, qualification receipt, runtime bundle, registry
entry, authorization, or activation path. Do not rename it as qualified, place
it in either runtime registry, or relax the existing qualification or canary
gates.

## Qualification calendar and model gates

The evaluator uses the latest fully covered 402 inclusive
`America/Los_Angeles` local dates. `D401` is the last covered date and `D0` is
401 local-calendar days earlier. Empty station/date cells stay empty; DST does
not shift a boundary.

| Partition | Training | Embargo | Score |
| --- | --- | --- | --- |
| Fold 1 | `D0..D179` | `D180..D186` | `D187..D216` |
| Fold 2 | `D0..D216` | `D217..D223` | `D224..D253` |
| Fold 3 | `D0..D253` | `D254..D260` | `D261..D290` |
| Fold 4 | `D0..D290` | `D291..D297` | `D298..D327` |
| Fold 5 | `D0..D327` | `D328..D334` | `D335..D364` |
| Final candidate/holdout | fit once on `D0..D364` | `D365..D371` | `D372..D401` |

Pre-holdout fitting may open only date-sharded `D0..D364` members. The five
development folds complete before the final candidate is fitted. The enabled
set is the lexicographically sorted metric-band set that passes development;
operators cannot remove a pair after seeing holdout scores.

Development-only leave-one-station-out scoring requires, per fold and
metric-band:

- at least 500 matched training instants and 100 matched score instants for a
  scoreable station;
- at least 100 score-date network events after excluding that station;
- at least five scoreable physical stations across at least three of
  `ambient`, `ecowitt`, `netatmo`, and `tempest`;
- equal averaging within station, then within provider family, then across
  provider families, so seven Tempest stations do not multiply Tempest weight;
- at least 2% provider-balanced improvement with a 95% bootstrap lower bound
  above zero;
- nonnegative point skill for at least 80% of scoreable stations; and
- no material-harm station, provider-family, nearest-three, season, or daypart
  slice with at least 100 events.

The immutable final candidate is scored on the locked holdout without a LOSO
refit. Every enabled metric-band must also pass the locked network, station,
provider, nearest-three, and critical-slice gates. Ecowitt remains one ordinary
station and provider-family slice within that full-network evidence; it does
not have a separate activation veto.

## Frozen bootstrap v1

`moving-block-bootstrap/v1` is literal:

- seed: unsigned `0x5EEDB007`;
- PRNG: xorshift32 in order `x ^= x << 13`, `x ^= x >>> 17`,
  `x ^= x << 5`, applying `>>> 0` after each step;
- replicates: exactly 2,000, resetting the seed for each independently scored
  metric-band or required slice;
- draw order: replicate outer, then Fold 1 through Fold 5 using one continuous
  state within an invocation;
- blocks: seven consecutive non-circular local-date slots with starts
  `0..N-7`; reject `N < 7` before the first draw;
- sampling: append blocks until at least `N` slots exist, then truncate to `N`;
  retain empty date slots and reject a zero-event replicate;
- losses: paired absolute error on identical occurrences, using wrapped
  circular absolute error only for direction; and
- lower 95% bound: sort all 2,000 finite skills and select zero-based index 49
  with no interpolation.

For the eight-date hand oracle, the first two states are `0xF549AA51` and
`0xC07EA050`; both choose start 1. The selected offsets are
`[1,2,3,4,5,6,7,1]`. With offset 3 empty and all other selected occurrences at
paired loss `(2,1)`, the replicate has seven events and skill `0.5`.

For a canonical single 30-date window, serialize the 2,000 arrays as
`JSON.stringify(plan) + "\n"`. The first two arrays are `[22,18,14,9,5]` and
`[12,15,7,18,1]`; the required SHA-256 is
`17c35e92d35c7e0ef1644e9c3f33c1ffb98237eadb2fd19813c203c700d447ac`.
Any seed, plan, hash, draw-order, empty-date, loss, or quantile drift is a hard
qualification stop.

## Holdout burn and durable evidence

Sufficient evidence belongs outside Git at the operator-controlled root
`${HOME}/.weather/model-evidence`, mode `0700`. Protect the storage itself with
approved encryption at rest. The API and web never receive this directory or
its keys.

Before any designated holdout member is opened, decrypted, streamed, or
parsed, the evaluator:

1. verifies the immutable candidate and preregistration without holdout access;
2. locks `${HOME}/.weather/model-evidence/ledger.lock` and verifies the full
   predecessor chain in `${HOME}/.weather/model-evidence/ledger.jsonl`;
3. derives `lineageId` from the site, cohort/reference kind, forecast source
   key/fingerprint, dataset/model/contract epoch, station/source/metric/spatial
   hashes, and aggregation contract;
4. rejects a proposed same-lineage interval unless its start is strictly after
   the maximum prior end;
5. appends one `holdout_accessed/v1` marker, fsyncs the ledger and parent
   directory, rereads and verifies the exact durable tail, and releases the
   lock; and
6. only then opens the designated members.

The marker is irrevocable. Cancellation, crash, zero scored rows, scorer error,
or later rejection still burns the interval. There is no release or delete
operation. After `D372..D401`, `D373..D402` rejects; the first allowable fully
covered 30-date interval is `D402..D431`. A new candidate or epoch in the same
lineage does not permit reuse.

The complete graph retains separate content-addressed snapshot manifest,
development report, preregistration, holdout marker, candidate, evaluation
report, and qualification receipt objects. The evaluation cannot mutate the
candidate. Promotion verifies every object hash and cross-link, requires the
marker in the append-only ledger, and appends an fsynced lifecycle record.
Activation additionally requires a matching attestation for either a
restorable encrypted backup or an independently verified content-addressed
copy. The second copy must be byte-identical and physically separate, not a
symlink or hard link, and must include every retained snapshot member.
Set `WEATHER_MODEL_EVIDENCE_REDUNDANCY_ROOT` to an existing canonical directory
on a different storage device before evaluating sufficient data. The evaluator
checks the device boundary before opening the holdout and promotion rechecks it
for every redundant object and retained snapshot member. A second directory on
the primary filesystem is rejected.

After approved external tooling has staged the complete graph and redundancy
proof under the fixed evidence root, promote and reverify the exact identities:

```bash
CANDIDATE_SHA256="${CANDIDATE_SHA256:?set the candidate SHA-256}"
EVALUATION_SHA256="${EVALUATION_SHA256:?set the evaluation SHA-256}"
QUALIFICATION_SHA256="${QUALIFICATION_SHA256:?set the qualification SHA-256}"
npm run weather:forecast-adjustment:evidence -- promote \
  --candidate-sha256 "$CANDIDATE_SHA256" \
  --evaluation-sha256 "$EVALUATION_SHA256" \
  --qualification-sha256 "$QUALIFICATION_SHA256"
npm run weather:forecast-adjustment:evidence -- verify \
  --qualification-sha256 "$QUALIFICATION_SHA256"
```

Do not use these commands for an `insufficient_data` result. Broken hashes,
links, ledger history, missing objects, fake redundancy, or a nonpassing receipt
stop promotion.

## Bundle staging, activation, and rollback

A qualified triple remains inactive until the operator stages a sanitized
runtime bundle:

```bash
CANDIDATE_SHA256="${CANDIDATE_SHA256:?set the candidate SHA-256}"
EVALUATION_SHA256="${EVALUATION_SHA256:?set the evaluation SHA-256}"
QUALIFICATION_SHA256="${QUALIFICATION_SHA256:?set the qualification SHA-256}"
npm run weather:forecast-adjustment:bundle -- \
  --candidate-sha256 "$CANDIDATE_SHA256" \
  --evaluation-sha256 "$EVALUATION_SHA256" \
  --qualification-sha256 "$QUALIFICATION_SHA256"
```

The command verifies the external graph and redundancy, rejects rather than
redacts forbidden content, preserves the exact canonical candidate/report/
receipt objects, and atomically writes:

```text
.weather-models/bundle-staging/sha256-<bundleSha256>.json
```

Forbidden content includes raw rows or per-event losses, credentials, keys,
encrypted members, device/LAN identifiers, database URLs, and external evidence
or member paths.

Activation is a reviewed two-step repository change:

1. Copy the staged bytes unchanged to
   `config/forecast-adjustments/ballydidean/bundles/sha256-<bundleSha256>.json`.
2. Set `activeBundle` in `config/forecast-adjustments/ballydidean.json` to the
   exact relative path and bundle/candidate/evaluation/qualification hashes.

CI rehashes and scans the reviewed bytes. The server image alone copies the
tree to `/opt/weather/config/forecast-adjustments/`; the production registry is
`/opt/weather/config/forecast-adjustments/ballydidean.json`. The web image has
no bundle tree. API/web Compose services mount no `.weather-data/`,
`.weather-models/`, external evidence directory, encrypted evidence, or
decryption key.

Before opening its listener, the API loads the fixed registry and selected
bundle through `lstat`/`realpath` and regular-file/no-symlink checks, verifies
the filename, hashes, allowlist, cross-links, passing receipt, redundancy, and
runtime ICU/tzdata fingerprint, then deep-freezes and caches the result for the
process lifetime. It has no watcher, hot reload, external fetch, station-catalog
comparison, evidence-store access, or key access. Missing, substituted, or
invalid content disables adjustment while the existing v4 raw forecast remains
available.

Activate or roll back only through a reviewed image deployment. Rollback selects
a prior committed bundle or `activeBundle: null`, rebuilds/redeploys the server
image, and restarts the API. Never edit or delete an existing bundle, mutate raw
data, or expect a running process to reload registry changes.

## Hard stops

Stop the operation and retain only secret-safe diagnostics when any of these
conditions occurs:

- export range exceeds 450 dates, row 4,000,001 appears, the transaction is not
  read-only repeatable-read, or the schema/source/QC/cutover/hash contract drifts;
- any export-role write, DDL, base-table, sequence, function, membership, or old
  credential succeeds;
- a raw production row, credential, key, private path/address, or encrypted
  evidence member appears in Git or a runtime bundle;
- an ignored local snapshot appears sufficient, or durable evidence is missing,
  mutable, incomplete, nonredundant, or not independently retrievable;
- a holdout member is touched before the durable marker, a ledger predecessor
  breaks, a same-lineage interval overlaps or starts before/equal to a prior
  end, or a burned interval is reused;
- bootstrap output differs from the frozen seed, plan, oracle, hash, or index;
- any enabled pair, critical slice, provider-balanced LOSO gate, or immutable
  holdout gate fails; or
- activation would bypass review, package different bytes, mount evidence/keys,
  hot reload, or weaken raw-v4 fallback.

`insufficient_data` is a successful inactive evaluation outcome, not permission
to alter thresholds. Leave `activeBundle` null and continue ordinary raw v4
service until a future, fully retained, untouched qualification epoch passes.

## Inactive temperature lead experiment

`evaluateRetainedForecastAdjustmentTemperatureLeads` accepts the same verified
multi-export input as the full-history research fitter. It reuses that reader
without widening the production export contract. The returned
`forecast-adjustment-retained-temperature-lead-research/v1` artifact is never
promotable and creates no runtime bundle or registry selection.

The baseline remains a fixed-anchor temperature hierarchy trained before the
first live-v4 date's seven-local-date embargo. Its last training instant plus
one hour must also precede the earliest scored forecast retrieval. The
experiment diagnoses live-v4 errors by exact lead hour, six-hour bucket,
24-hour band, and daypart; it does not invent sub-daily archive anchors.
Every matched temperature forecast remains in the comparison. Missing
coefficients or out-of-envelope inputs use raw fallback with explicit coverage
counts rather than disappearing from scoring.

The frozen comparisons are raw, full baseline correction, a static
`leadHours / bandMaximumHours` taper, and prior-only six-hour-bucket shrinkage.
The taper intentionally resets at each broad-band boundary and is only an
experimental baseline. The shrinkage grid is `0, 0.25, 0.5, 0.75, 1`; calibration
weights each valid hour equally, requires thirty distinct valid hours across
three local dates in the same six-hour lead bucket, and breaks ties toward the
smaller correction. Daypart is descriptive, not an additional tuning dimension.
Insufficient support means raw fallback.

This is a **pseudo-real-time retrospective experiment**, not online validation.
An observation is assumed available one hour after its valid instant and can
influence only forecasts retrieved at or after that assumed availability time.
The sanitized historical exports do not establish actual measurement arrival
or revision times. The live dates were already inspected, so chronological
replay does not turn them into untouched qualification data. Both ordinary
forecast-example and equal-valid-hour errors must be distinguished from counts
of independent weather events. No result from this experiment authorizes
production activation; later independent live evidence remains necessary.

## Inactive temperature weather experiment

`evaluateRetainedForecastAdjustmentTemperatureWeather` reuses the same verified
multi-export reader and requires the SHA-256 of an externally retained plan
frozen before analysis. Its
`forecast-adjustment-retained-temperature-weather-research/v1` result is
non-promotable and never creates a runtime bundle or changes either registry.

Predictors come from the exact forecast row selected for temperature after
forecast jitter deduplication: forecast temperature, relative humidity, and
wind speed, plus lead band, local season, and local daypart. Valid-time station
humidity and wind are never predictors. Clouds, radiation, and precipitation
are absent from this export contract; the experiment makes no sunshine or
cloud-regime claim and requires no new production export.

The five frozen comparisons are raw, the existing calendar baseline,
calendar plus temperature refinement, calendar plus temperature and weather
refinements, and independently fitted raw-start temperature/weather refinements.
Temperature cells use `floor(forecastTemperatureC / 5)` with literal half-open
five-degree bins, including negative temperatures. Weather cells use humidity
`<50`, `[50,80)`, or `>=80` percent and wind `<2`, `[2,5)`, or `>=5` m/s.
Both cell families are also partitioned by lead band, season, and daypart.

Each residual cell requires at least fifty unique valid hours across ten local
dates. Unit-hour weighted medians use the existing scalar estimator with zero
parent coefficient and pseudocount one hundred. Temperature residuals subtract
the calendar prediction, or raw prediction for the independent raw-start path.
Weather residuals subtract that path's **unclipped** additive temperature
prediction. Component corrections retain the existing five-degree cap. Final
predictions add the cumulatively capped `[-5,5]` degree correction to raw, then
apply the existing physical temperature bounds once.

Missing weather features or unsupported cells contribute zero at that stage;
they do not remove forecasts from scoring. Baseline ineligibility or envelope
failure forces raw for every comparison, including the raw-start ablation.
Coverage distinguishes these cases. Every comparison reports both forecast-
example and equal-valid-hour errors, first-48-hour errors, lead/daypart/date
diagnostics, and observed forecast-weather slices.

Separate models are fitted for the first thirty dates of January, April, July,
and October 2025; the thirty dates immediately preceding live v4; and the live-v4
window itself. Each model trains only before its score window's seven-local-
date embargo and also satisfies `trainingCutoff + 1h <= scoreInformationBoundary`
in UTC. Live boundaries use truthful forecast retrieval timestamps. Archive
boundaries use `validAt - targetLeadHours` only as a conservative horizon
boundary, never as a fabricated model-run timestamp. Empty or uncovered windows
remain explicit rather than being treated as successful validation.

These previously consumed dates support retrospective model comparisons, not
untouched qualification. Seasonal archive results do not establish equivalent
live-v4 skill. No post-score threshold tuning or best-looking slice permits
activation; the production qualification and no-harm gates remain unchanged.

## Inactive temperature horizon-gating experiment

`evaluateRetainedForecastAdjustmentTemperatureWeatherHorizon` adds one fixed
gating ablation in a separately versioned retained research report. The existing
weather evaluator and its five-comparison report remain unchanged. Both entry
points reuse the same retained reader, chronological windows, baseline fitting,
and evidence identity binding.

The gate selects raw for integer `targetLeadHours <= 48` and the exact existing
raw-start temperature/weather prediction for `targetLeadHours > 48`. It does not
change fitting, predictors, support floors, fallbacks, or correction caps. Live
leads retain their validated ceiling convention: exactly forty-eight hours is
protected, while any positive amount beyond forty-eight maps to lead forty-nine.
Archive scores above the gate contain only the seventy-two through
one-hundred-sixty-eight-hour fixed anchors; coverage of leads forty-nine through
seventy-one comes from live retrievals, not additional historical subdaily runs.

The horizon report compares raw, ungated raw-start, and gated predictions on
the same events. Overall, first-forty-eight-hour, after-forty-eight-hour, and all
six diagnostic dimensions recompute losses per event before aggregation,
including equal-valid-hour scores. Coverage distinguishes protected forecasts,
longer-lead forecasts, actual nonzero corrections, and longer-lead fallback
reasons. No event-level records are retained in the report.

The threshold was chosen after examining the prior experiment's near-term
regression. The externally retained plan binds that prior artifact and plan,
the four unchanged snapshot receipts, and this fixed rule before replay.
First-forty-eight-hour equality to raw is guaranteed by construction, not a
qualification result. Previously consumed dates, including seasonal archive
windows, remain exploratory; no result from this ablation authorizes activation.

## Inactive temperature-only stage experiment

`evaluateRetainedForecastAdjustmentTemperatureOnly` isolates the raw-start
temperature component in a separately versioned
`forecast-adjustment-retained-temperature-only-research/v1` report. The original
weather and horizon reports remain unchanged. The new analyzer reuses their
exact temperature cells, baseline eligibility mask, chronological training
windows, retained inputs, and evidence identity binding. It does not refit a
different model or apply the prior forty-eight-hour gate.

The prediction contains only the existing raw-start temperature coefficient:
add that component to raw using the existing correction cap and physical
temperature bounds. Calendar and humidity/wind residual components contribute
nothing. Unsupported temperature cells and baseline-ineligible forecasts use
raw; missing weather features do not remove events. This ablation concerns
predictors of temperature error, not the separate humidity adjustment model.

The `temperatureOnly` view compares raw, raw-start temperature/weather, and
raw-start temperature-only predictions on identical event populations. Overall,
first-forty-eight-hour, after-forty-eight-hour, and all six diagnostic dimensions
recompute event and equal-valid-hour losses directly. Coverage reports support,
raw fallbacks, nonzero corrections, and prediction differences from the full
weather path; no event-level records are returned.

This single stage-removal hypothesis was frozen and retained before replay,
after examining prior temperature experiments. There is no threshold search,
new production export, or activation. Previously consumed live and archive
dates remain descriptive rather than independent qualification evidence.

## Inactive temperature adaptive-strength experiment

`evaluateRetainedForecastAdjustmentTemperatureWeatherAdaptive` replays one fixed
live-only shrinkage policy in a separately versioned
`forecast-adjustment-retained-temperature-weather-adaptive-research/v1` report.
The underlying raw-start temperature/weather prediction, fitted cells, baseline
eligibility mask, snapshots, and six chronological model windows are unchanged.
All prior report APIs and their source scores remain unchanged.

The analyzer privately reuses `analyzeTemperatureLeadResearch`, passing the
already capped raw-start temperature/weather prediction as its full correction.
The adaptive prediction is `raw + alpha * (fullPrediction - raw)`. Selection
uses the existing grid `0, 0.25, 0.5, 0.75, 1`, six-hour lead buckets, at least
thirty prior unique valid hours across three local dates, equal-valid-hour MAE,
and the existing smaller-alpha tie policy. Insufficient calibration support
selects zero, so the cold-start output is raw. Neither daypart nor weather
regime selects alpha; those dimensions are diagnostic only.

Calibration starts empty at the live score-window boundary and expands using
only same-bucket outcomes whose `validAt + 1 hour <= referenceAt` for the scored
forecast. Archive data and pre-window outcomes cannot warm the calibration.
The full model remains fitted strictly before the existing embargo and earliest
live information boundary. Missing-feature and baseline-ineligible events stay
in both calibration and scoring populations through the existing raw fallbacks.

The five archive diagnostics retain their original static models and scores,
but `adaptiveCorrection` explicitly reports `not_applicable_archive_cohort`
with null adaptive scores. Previous Runs anchors have no observed retrieval
timestamp; conservative `validAt - lead` boundaries are not substituted for
truthful `referenceAt` to manufacture adaptive archive validation.

Live overall, first-forty-eight-hour, after-forty-eight-hour, calibrated-subset,
and all six diagnostic comparisons recompute exact per-event losses, including
equal-valid-hour weighting. Aggregate coverage reports calibrated and cold-start
counts, selected alpha counts, nonzero corrections, and availability audit
violations. Neither raw records nor the internal event-level causal trace are
returned in the new report.

This is a post-hoc pseudo-real-time experiment on previously consumed dates.
Actual station measurement arrival latency is not reconstructed. A calibrated
subset is not the complete forecast population, and cold-start raw equality is
not learned skill. The experiment neither qualifies nor activates a model.

## Inactive temperature hybrid experiment

`evaluateRetainedForecastAdjustmentTemperatureWeatherHybrid` combines two
previously examined strategies without fitting or calibrating new parameters.
Its separately versioned
`forecast-adjustment-retained-temperature-weather-hybrid-research/v1` report
preserves the original weather models, chronological cutoffs, input snapshots,
and source comparisons. Existing report entry points remain unchanged.

The sole hybrid rule selects the exact prior causal-adaptive prediction for
integer lead hours at or below forty-eight and the unchanged full raw-start
temperature/weather prediction above forty-eight. Live leads retain their
ceiling convention: exactly forty-eight hours selects adaptive correction;
any positive duration beyond forty-eight maps to lead forty-nine and selects
the full correction. The existing calibration grid, six-hour buckets, support
floors, availability rule, cold start, raw fallbacks, and correction caps do
not change. The shared weather fit and causal replay each execute once.

The live `hybridCorrection` view compares raw, full weather correction,
causal-adaptive correction, the prior raw/full forty-eight-hour gate, and the
hybrid on identical events. Overall, first-forty-eight-hour, after-forty-eight-
hour, and all six diagnostic dimensions recompute losses per event, including
equal-valid-hour weighting. Coverage separates near-term calibration support
and alpha selections from longer-range full corrections, and counts exact
prediction differences from the prior gate. Internal causal traces and raw
records are not returned.

All five archive hybrid views remain explicitly not applicable with null
scores because observed retrieval timestamps are absent. Their static source
models and scores remain controls, not hybrid qualification evidence.

The threshold and component choices were selected after examining prior
results. Near-term equality to the adaptive strategy and longer-range equality
to the full correction are guaranteed by construction, not independent
validation. Previously consumed dates and assumed observation availability
remain limitations. No result authorizes production activation.

## Inactive temperature weather-component shrinkage experiment

`evaluateRetainedForecastAdjustmentTemperatureWeatherShrinkage` evaluates one
fixed half-strength weather component across all horizons. Its separately
versioned `forecast-adjustment-retained-temperature-weather-shrinkage-research/v1`
report preserves the original weather models, source comparisons, snapshots,
and six independent chronological fitting windows. Existing entry points and
their report payloads remain unchanged.

The prediction keeps the raw-start temperature coefficient at full strength
and multiplies only the existing humidity/wind residual coefficient by `0.5`.
The two contributions are summed before applying the existing cumulative
correction cap and physical temperature bounds once. This is not an average
of already clipped temperature-only and full-weather predictions. No model
is refitted differently, and no adaptive calibration or horizon gate applies.

Baseline-ineligible forecasts remain raw. Unsupported temperature cells
contribute zero; missing or unsupported weather cells contribute zero and
preserve the temperature-only prediction. These events remain in every score
denominator. The `weatherShrinkage` view compares raw, temperature-only,
half-weather, and full-weather predictions per event for overall, first and
later forty-eight-hour windows, and all six diagnostic dimensions. Equal-hour
scores are recomputed from event losses rather than blended endpoint scores.
Coverage distinguishes support, fallbacks, nonzero corrections, and exact
differences from both endpoint strategies; raw rows are not returned.

All five fixed-anchor archive windows can score this static rule using their
unchanged pre-window models and conservative information boundaries. This does
not recreate observed retrieval timestamps or validate the live-only adaptive
and hybrid experiments. The single weight is frozen before replay; it is not
chosen by searching scores. All dates were previously consumed, so seasonal
and live results remain exploratory and cannot authorize production activation.

## Inactive temperature recent-training experiment

`evaluateRetainedForecastAdjustmentTemperatureWeatherRecency` compares a fixed
recent-training model with the unchanged all-history raw-start model. The
`forecast-adjustment-retained-temperature-weather-recency-research/v1` report
preserves all original baseline models, eligibility masks, refinement models,
chronological cutoffs, snapshots, and source scores. Previous report entry
points remain unchanged.

For each diagnostic, the recent interval ends on the latest admitted training
local date, including baseline-ineligible training rows. Its inclusive start
is that date minus 364 calendar days, calculated using the existing local-date
helper rather than elapsed-hour arithmetic. Filtering occurs only within the
original training set, after its embargo and information-boundary checks.
There is no score-outcome-dependent anchor or search over lookback lengths.

The existing raw-start temperature stage is refitted on the retained subset.
The weather stage is then refitted on residuals after that new temperature
stage, using the same retained rows. Cell keys, support floors, weights,
pseudocount, clipping, and raw fallbacks do not change. The original baseline
and envelope remain fixed; no recent calendar-start candidate, adaptive
calibration, horizon gate, or coefficient scaling is added.

The `recencyTraining` view exposes the candidate models and their separate
canonical hash, available and retained training coverage, excluded row count,
nominal local-date boundaries, and actual retained time bounds. Its coverage
also counts temperature/weather support lost relative to the all-history
model. All matched score events remain in raw, all-history, and recent-model
comparisons, including fallbacks. Overall, first and later forty-eight-hour
windows, and all six diagnostics recompute exact event and equal-hour losses.

When the complete admitted training history already fits within the recent
interval, model and score equality is guaranteed by construction. Empty
training yields empty candidate models and null training bounds but retains
the complete score population with raw predictions. Missing dates are not
imputed, and the nominal 365-date interval does not imply 365 observed dates.

All five archive windows can score this static rule under their original
conservative information boundaries; they do not validate adaptive or hybrid
archive behavior. Results use previously consumed dates. Apparent gains may
come from different fitted coefficients or increased raw fallback, not proven
temporal drift. No result qualifies or activates a production model.

## Inactive temperature boosted-tree experiment

`evaluateRetainedForecastAdjustmentTemperatureBoosted` adds one external,
research-only gradient-boosted residual learner. It preserves the original
full-history baseline, envelope, static models, chronological windows and
report fields. Its new `boostedResidual` view compares raw, full static and
boosted predictions on the same complete forecast population. The frozen
hybrid remains a live-only reference; archive hybrid scores are not fabricated.

The isolated CPU-only XGBoost runtime is not an application dependency.
`scripts/research/temperature_boosted.py` accepts an exact JSON training and
prediction contract over stdin and returns native model/configuration JSON,
ordered prediction identities and residuals. `--describe` exposes its fixed
configuration. `--predict` reloads an already fitted native model without
training. Runtime versions, installed package files, the trainer source and
configuration are hashed separately and bound into the research evidence.
Caller-supplied hashes are identifiers; the executable research runner must
verify the referenced bytes before and after execution.

The twelve ordered predictors are raw forecast temperature, relative humidity,
wind speed, target lead hours, four local-season indicators and four local-
daypart indicators. Numerical predictors use their existing continuous values
rather than the previous manually chosen bins. This is a change in learner and
encoding, not an expansion of input sources. Only the selected temperature
forecast row supplies weather predictors. Missing humidity and wind retain
native missing-value routing. Observation-derived state, labels, absolute dates,
baseline corrections and other-model predictions are not predictors. Stable
`validAt|targetLeadHours` identities travel separately and must be echoed in the
same order; they are never columns in the feature matrix.

Each window uses the original all-history training cutoff and eligibility mask.
The response is `actual - rawForecast`. Each eligible training row receives
weight `1 / eligibleRowsAtSameValidAt`, giving one total input weight per valid
hour despite multiple archived lead anchors. The fixed 200-round, depth-three
CPU histogram learner uses MAE-aligned `reg:absoluteerror`, learning rate 0.05,
zero initial correction, seed 20260906 and a single thread. There is no search,
early stopping, score evaluation set, horizon gate, recency filter or adaptive
calibration. `min_child_weight=50` concerns weighted Hessian mass, not an exact
fifty-hour support floor. Full parameters are frozen in the retained plan and
the bridge's native saved configuration.

Eligible predictions add the residual capped to `[-5,5]` degrees to raw, then
apply physical temperature bounds once. Baseline-ineligible forecasts remain
raw. Empty eligible training skips the trainer and preserves all score rows
with raw predictions. Models and configuration retain their native JSON bytes
and hashes; request, feature-schema and row-partition hashes do not expose
training examples or individual predictions in the aggregate report.

All six historical windows retain both event and equal-valid-hour MAE, the
first and later forty-eight-hour scopes, and all existing diagnostics. Equal-
valid-hour MAE is primary. Training contains exact leads 24, 48, 72, 96, 120,
144 and 168 hours only: non-anchor live forecasts require cross-lead
generalization, and leads below twenty-four hours lie below the observed lead
range. Seen, unseen and below-range lead scopes are reported without a
post-result gate. These consumed dates remain exploratory, regardless of gain.

The prospective protocol reserves forecasts by observed retrieval/reference
local date from September 8 through October 7, 2026 in `America/Los_Angeles`,
with leads one through 168 hours. Candidate and comparator model identities
must be frozen before the first retrieval. There is no interim label inspection,
scoring for tuning, refit or window substitution. Evaluation cannot begin before
October 15 at 08:00 UTC and must also wait for all required outcomes and
coverage checks. The retained registration binds exact cohort boundaries,
model artifacts, completeness criteria and comparison rules. Registration does
not install a shadow collector or scorer; production retention remains
unchanged. One thirty-date prospective test does not establish all-season
safety or bypass existing production qualification and no-harm gates.

The bridge's independent tests run with the explicitly approved external
research interpreter, not the application test runtime:

```bash
"$HOME/.weather/research-runtimes/xgboost-cpu-3.4.1/bin/python" \
  -m unittest discover -s scripts/research -p 'test_temperature_boosted.py'
```

## Inactive temperature boosted-hybrid experiment

`evaluateRetainedForecastAdjustmentTemperatureBoostedHybrid` combines the
existing causal-adaptive prediction through forty-eight hours with the frozen
boosted prediction beyond forty-eight hours. The threshold, ceiling convention,
raw fallbacks and already-applied correction caps are unchanged. No additional
clipping, learner tuning or observation-derived feature is introduced.

The new `boostedHybrid` view retains raw, static, boosted, adaptive and original
hybrid controls on the same complete live population. Overall, first and later
forty-eight-hour MAE and all six diagnostics are recomputed from individual
predictions; separately normalized horizon MAEs are not averaged. The original
boosted model and report material remain unchanged. Near-term equality to the
original hybrid and longer-range equality to the boosted model are guaranteed
by construction, not independent evidence of generalization.

The experiment runner verifies the exact preceding native request, reloads its
frozen model through `--predict` without fitting, and requires identical native
predictions and model/configuration hashes. The existing static coefficients
are reconstructed only to verify equality with their frozen source. Adaptive
calibration still uses the original static correction, all original live lead
buckets, empty initial state and the documented `validAt + 1 hour` availability
assumption. It does not calibrate against the tree or change its support rules.

Archive hybrid scores are explicitly unavailable because the anchors lack
observed retrieval timestamps; their original boosted/static controls remain.
Historical dates and component choices were already examined, so this is a
non-promotable composition experiment. The existing prospective registration
is unchanged: this challenger is not silently substituted into that holdout,
and no shadow collection, scoring job or production activation is installed.

## Inactive first-twelve-hour recent-error persistence experiment

`evaluateRetainedForecastAdjustmentTemperatureNearNowcast` preserves the full
boosted-hybrid report and adds one fixed `nearNowcast` challenger. Only eligible
leads one through twelve can change. Hours thirteen through forty-eight keep
the exact prior adaptive prediction; later hours keep the exact frozen boosted
prediction. Unsupported targets keep the prior prediction rather than leaving
the score population. No tree is fitted, parameter searched, or dependency added.

At each live forecast's observed retrieval/reference time, source errors come
only from canonical matched live temperature rows with leads at most six hours.
The source reference must strictly precede the target reference. Its valid time
must lie inclusively within the preceding six hours, and its assumed observation
availability (`validAt + 1 hour`) must not exceed the target reference. Within
each source valid hour, the latest retained reference wins, with greatest
canonical key breaking ties. Conflicting actuals for a source hour fail closed.
Source errors include baseline-ineligible rows because that flag is the old
static envelope, not observation quality control.

Exactly three most recent distinct source hours are required. Their median
`actual - rawForecast` error is decayed by `2^(-ageHours / 6)`, where age runs
from the newest source valid time to the target valid time. The decayed
correction is capped to `[-5,5]` degrees, added directly to raw temperature,
then physically bounded to `[-100,70]` degrees. The prior adaptive correction
is not added. Both this source history and the unchanged adaptive calibration
start empty; there is no archive warm start or cross-row predictor expansion.

The primary test is fallback-inclusive, equal-valid-hour first-twelve-hour
MAE below both raw and the previous boosted hybrid. First-forty-eight-hour MAE
must not worsen against the prior hybrid, and every prediction after twelve
hours must remain exact. Event MAE is secondary. The report also splits hours
one through six, seven through twelve, thirteen through twenty-four, and
twenty-five through forty-eight. All six diagnostic dimensions are recomputed
for both the full population and the primary first-twelve-hour population.

An optional `onPrivatePredictionAudit` callback receives deeply frozen records
for every live score event, including labels, old/new predictions, selected
sources, availability times, decay and fallback reasons. These private records
must remain in owned temporary memory storage before encrypted evidence
retention; they must never be committed or included in the public report. The
public report binds only their canonical array SHA-256 and aggregate checks.
Independent verification reconstructs source selection, predictions and exact
event/equal-hour losses from that audit rather than trusting report aggregates.

The one-hour availability lag is an assumption, not reconstructed station
arrival or revision evidence. The matched observations and canonical forecast
rows are retrospective; this is not a proven real-time nowcast. Previously
examined dates remain exploratory and non-promotable. Archive nowcast scores
are explicitly unavailable, while existing archive controls remain untouched.
Production and the existing future-holdout registration are unchanged.

## Inactive weather-conditioned analog residual experiment

`createTemperatureAnalogPredictionAudit` in `temperature-analog-research.ts`
tests a different first-twelve-hour correction: learn the existing model's
remaining error from past forecasts with similar forecast weather and local
clock phase. It does not persist the latest raw error or reuse the rejected
nowcast prediction. The standalone research runner consumes the preceding
encrypted audit's exact frozen `priorBoostedHybridPrediction` values rather
than refitting or rerunning the native tree. No application runtime is wired
to this research helper.

Eligible targets have leads one through twelve, the original baseline
eligibility flag, and available raw humidity and wind. Sources must have a
strictly earlier reference, assumed observation availability at `validAt + 1
hour` no later than the target reference, and valid times within the preceding
336 elapsed hours inclusively. Sources and targets share the one-through-six
or seven-through-twelve lead band and have Los Angeles valid-time clock hours
within two hours circularly. Source baseline eligibility is not observation
quality control and does not exclude source errors.

After those filters, one forecast per source valid hour is chosen by closest
lead to the target, then latest reference, then greatest canonical key. This
choice occurs before computing weather distance. The squared distance sums
temperature differences scaled by 5°C, humidity by 20 percentage points, wind
by 3 m/s, circular clock-hour difference by 3 hours, and lead difference by 6
hours. Only distances at most four qualify. Exactly twelve closest distinct
valid hours are required; distance ties prefer the latest valid time and then
greatest key. DST repeated clock hours remain distinct UTC valid times.

The correction is the median of those twelve `actual - priorPrediction`
errors, using the mean of the sixth and seventh ordered values. It is added
to the target's prior prediction, with the total correction relative to raw
capped to `[-5,5]` degrees and final physical bounds `[-100,70]`. Sources always
use frozen prior predictions, never recursively updated analog predictions.
Unsupported targets and all leads after twelve keep the exact prior prediction
without extra clipping. In particular, hours thirteen through forty-eight and
the boosted component after forty-eight remain unchanged.

The helper returns deeply immutable private records for every target. They
retain candidate counts, source ranks, feature differences and distance terms,
even-median arithmetic, pre-cap and capped deltas, and explicit fallback
reasons. The runner joins only the original diagnostic labels and publishes
aggregate coverage, scores and the private array hash. Individual rows stay in
owned temporary memory storage and then encrypted evidence, never Git.

Acceptance still requires full-population, fallback-inclusive first-twelve-hour
equal-valid-hour MAE below both raw and the previous boosted hybrid, first-forty-eight-
hour no-harm, and exact later prediction equality. All nine horizon scopes and
both complete and first-twelve-hour diagnostic grids are checked independently.
No parameter, support threshold or favorable subgroup is selected after scoring.
The same consumed twelve-date cohort and assumed observation availability make
this exploratory rather than qualified. Production and the existing prospective
registration remain unchanged; no collector, scorer or deployment is installed.

## Expanded inactive analog validation

`analyzeTemperatureAnalogValidation` in `temperature-analog-validation.ts`
validates the frozen private prediction audit without fitting, changing analog
selection, or invoking the native tree. The existing candidate and every
forecast after twelve hours remain unchanged. This expands the checks, not the
number of independent observations: no new forecast dates enter this analysis.

The primary result remains fallback-inclusive first-twelve-hour equal-valid-hour
MAE, with raw and the previous boosted hybrid scored on identical targets.
Repeated forecast vintages are averaged within their common UTC valid hour
before hours are averaged. Secondary checks include event MAE, equal-hour signed
bias and RMSE, quantiles of hourly mean absolute errors, and equal-hour frequency
of individual errors exceeding two and three degrees Celsius. Quantiles of
hourly mean errors are not quantiles of individual forecast errors.

Paired circular moving-block bootstrap intervals use one-, two-, and three-local-
date blocks, twenty thousand replicates per block length, and a fixed xorshift32
seed. Each replicate draws the same dates for all three predictions and retains
partial-date hour weighting. All block lengths and both comparator intervals
are reported; the most favorable interval cannot be selected. These descriptive
intervals do not undo model selection on the same dates or establish independent
weather samples.

Every first-twelve-hour lead, represented date, season, daypart, and frozen
weather regime remains visible, including empty cells. Date-deletion checks
remove one date only from scoring: the predictions and original causal source
state remain frozen. They are influence diagnostics, not cross-validation.
The first and second chronological halves are similarly descriptive rather
than fresh holdouts. Supported and fallback subsets never replace the full
population in the primary comparison.

The retained four export manifests cover January 2024 through September 2026,
but their live retrieval records begin on August 26, 2026. Earlier Previous Runs
records provide only 24/48/72/96/120/144/168-hour anchors and no observed retrieval
reference. They cannot validate or warm the first-twelve-hour analog component;
no issue times or subdaily leads are synthesized. Further independent validation
requires additional live retrieval history and an explicitly registered frozen
candidate. The existing prospective registration is not silently changed, and
this validation does not install collection, scoring, or production activation.

Private records and event losses remain in owned temporary memory storage and
then encrypted evidence. Aggregate reports bind the source audit, candidate,
validation plan, and implementation hashes. Observation arrival/revision timing
still relies on the retrospective one-hour maturity assumption, and the
candidate remains unqualified regardless of the diagnostic intervals.

## Inactive half-strength analog tuning

`createTemperatureAnalogShrinkageAudit` in `temperature-analog-shrinkage.ts`
tests one fixed conservative weight after the expanded validation exposed
single-day sensitivity and inconsistent large-error improvements. It reruns the
unchanged full-strength analog helper, preserving every neighbor, residual,
support decision, and original prediction. Supported first-twelve-hour targets
use `prior + 0.5 * (fullAnalog - prior)`. Interpolation occurs after the existing
full-strength cumulative and physical caps; there is no additional clipping.
Fallbacks and all forecasts after twelve hours retain the exact prior value.
The source residuals never incorporate half-strength predictions.

The immutable private audit retains the original full-strength fields and adds
`fullStrengthIncrementC`, `retainedIncrementC`, `shrinkageWeight`, and
`shrunkAnalogPrediction`. In particular, its original `analogPrediction` and
correction fields still describe the full-strength model. Only the explicit
`shrunkAnalogPrediction` field identifies the challenger.

The expanded validator scores the same challenger against the old boosted
hybrid and against the full-strength analog in separately named views. Changing
`priorPrediction` in the second validation projection changes the comparator
only, never model inputs or source errors. Keys, targets, raw forecasts, new
predictions, times, leads, and support stay identical. Raw and challenger score
objects must match between views; duplicated raw intervals are not independent
evidence. The wrapper replaces the validator's generic no-parameter-selection
label with explicit post-hoc selection of this fixed half weight.

Retention as the next tuning candidate requires every preregistered gate:
first-twelve-hour equal-hour MAE strictly below raw and both older models;
first-six-hour and first-forty-eight-hour MAE no worse than full strength;
exact key-by-key equality after twelve hours; twelve nonempty score-only
leave-one-date-out comparisons with no harm against the old boosted hybrid;
and no increase against full strength in equal-hour frequencies of individual
errors strictly above two or three degrees. Missing metrics fail closed, and
gate comparisons use no epsilon. Date omissions retain original source state
and are not cross-validation. Every diagnostic slice and all three bootstrap
block lengths remain reported, including unfavorable or empty cells.

This is one trial on the same consumed twelve dates, not a grid search or new
independent validation. Negative evidence is retained without a fallback tuning
pass. Post-selection intervals are descriptive, regardless of whether every
retention gate passes. No native model is fitted or invoked, no forecast export
is added, and the original prospective registration and production runtime stay
unchanged. Private material remains in owned tmpfs before encrypted retention.

## Expanded half-strength stress validation

`analyzeTemperatureAnalogStress` in `temperature-analog-stress-validation.ts`
adds descriptive checks of the same frozen half-strength private audit. It
neither fits a model nor changes neighbor selection, the half weight, original
fallbacks, or production behavior. The original equal-valid-hour scores remain
the primary comparison. Raw, old hybrid, full analog, and half analog always
share the same scored population.

For the twelve consumed local dates, the battery retains all 66 unordered
pair deletions, identifying the eleven consecutive pairs as a subset rather
than additional evidence. It also retains all ten nonwrapping three-date
deletions and ten included-only rolling three-date windows. Deletions affect
scoring only: omitted dates can remain in the original source state. These are
influence diagnostics, not cross-validation or holdouts. Calendar gaps are
rejected rather than silently joined into apparently contiguous windows.

Equal-date MAE averages the twelve daily equal-valid-hour MAEs, including
partial boundary dates. Two secondary vintage views select the earliest or
latest reference for every first-twelve-hour valid time; equal-reference ties
use the greatest canonical key. Both retain every represented target hour and
report their selected-lead distributions because the lead mixture changes.
All 48 exact-lead/daypart cells retain scores, counts, and explicit nulls for
empty populations. No favorable date, vintage, cell, or weighting is selected.

Selected-source availability is separately stressed at one, two, three, six,
and twenty-four elapsed hours after each source valid time. When any of an
originally supported target's twelve frozen sources would be unavailable, both
analog variants fall back exactly to the old hybrid. Sources are not replaced;
original fallback rows, including their partial source audits, remain unchanged.
The one-hour scenario must reproduce every original score. Each scenario
reports retained/newly unsupported event and distinct-hour counts and all seven
horizons. The twenty-four-hour scenario is a severe fixed delay, not a complete
outage or statistical worst-case bound. The old hybrid already represents
complete loss of the analog component. This does not reconstruct actual station
arrival or revision timing or simulate the production source-selection process.

All rows and comparator ranges are retained, including adverse outcomes. This
is a many-comparison post-selection stress battery on already consumed dates,
not additional independent observations. It introduces no qualification gates,
parameter selection, production activation, or changes to the existing future
registration. Any production coverage inquiry is read-only and aggregate-only;
new data cannot be scored by inventing earlier retrieval times or refitting a
supposedly frozen comparator. Private evidence remains in owned tmpfs before
verified encrypted local and Blueberry retention.

## Frozen live-temperature replay

`replayFrozenTemperatureHybrid` in `temperature-frozen-replay.ts` reconstructs
the retained boosted hybrid without fitting, tuning, or changing the analog
candidate. It derives baseline eligibility and raw-start static predictions
from the frozen coefficients and training envelopes, reuses the original
causal calibration, and applies the frozen native tree only through an explicit
prediction callback. Every canonical event is sent to native prediction,
including baseline-ineligible events whose returned residual is ignored.
The original adaptive-through-48-hours and native-after-48-hours boundary stays
unchanged. Model, configuration, and feature-schema hashes must match.

`createTemperatureLiveReplayEvents` in `temperature-live-replay-events.ts`
accepts sanitized export rows and reuses the original forecast deduplication,
station catalog, and network-target rules. Weather inputs come from the exact
selected temperature forecast. Fixed Previous Runs anchors are never assigned
invented live retrieval times or subdaily leads. Duplicate station hours and
malformed rows fail closed. The caller must separately verify the export
manifest, checksums, authorized date bounds, and overlap with retained inputs.

An extension replay begins with the complete original live-event history,
not an empty state at the newly scored date. Before admitting new-date scores,
the runner must reproduce every retained prior prediction, analog source,
support decision, and half-strength prediction exactly. Native inference uses
the retained model and runtime; labels never enter its prediction features.
The analog remains `prior + 0.5 * (fullAnalog - prior)` on supported first-
twelve-hour events, with every original fallback and later prediction intact.

Newly scored retrospective dates must be distinguished from genuinely
prospective holdouts. A partial date cannot establish complete daypart coverage
or uniform improvement. Report missing cells and unfavorable comparisons, keep
the frozen scoring cutoff, and do not substitute a different candidate's future
registration or qualification receipt. These helpers do not export production
data, install collectors, activate a model, or change application behavior.
The documented forced-command export endpoint is required; a denied or absent
endpoint is an access blocker, not permission to export through another route.
Private inputs and native requests remain in owned tmpfs before encrypted
retention. Only aggregate diagnostics and artifact hashes belong in reports.

## Export endpoint compatibility repair

The September 2026 installation predates the standard export command. Its
bounded compatibility endpoint lives outside the active release tree at
`/usr/local/lib/weather/forecast-training-export-v1/`. Only the export route is
added to the root-owned SSH wrappers; existing operations remain unchanged.
The exporter checks the installed common-helper digest and uses the unchanged
repository query and package verifier. A disposable, resource-limited client
uses the active PostgreSQL image and network namespace, without restarting the
database or changing the release control-plane digest.

The dedicated credential remains server-side at
`/var/lib/weather/forecast-training-export/password`, mode `0400`, owner/group
`999:999`, beneath a root-private directory. The existing export role receives
only the missing login and database-connect capability; its default read-only
setting and two-view-only grants remain enforced. Provisioning checks the
complete authority envelope before and after the change and keeps exact wrapper
backups and rollback evidence under
`/var/lib/weather/forecast-training-export-repair-20260907/`.

A later deployment that changes the common helper makes this compatibility
exporter fail closed. Replace it through reviewed provisioning with the standard
release exporter and its documented secret mount; do not rewrite release
metadata or relax the hash check. Endpoint installation does not qualify or
activate an adjustment model.

## Representative near-term development panel and causal raw guard

The fixed primary development panel uses every canonical live-v4 forecast on
August 27 through September 6, 2026, inclusive. These eleven interior calendar
dates cover all times of day without selecting only densely populated or
favorable dates. Retrieval gaps and missing target hours remain visible;
August 26's startup fragment and September 7's partial date are separate stress
results. This is a broader late-summer development sample, not an all-season
sample, fresh holdout, or proof that additional independent dates were acquired.

Primary first-twelve-hour MAE averages event errors within each UTC valid hour,
then hours within each local date, then all eleven dates equally. Equal-hour and
event-weighted scores remain secondary. A sensitivity gives equal weight to all
48 exact-lead/daypart cells; a missing cell produces a null aggregate rather
than being silently omitted. All seven horizons, individual dates, leads,
dayparts, seasons, frozen weather bins and three chronological blocks remain
visible. The existing one-, two-, and three-date block bootstrap retains its
equal-hour weighting and is descriptive secondary evidence, not uncertainty
for the new equal-date primary statistic. State is never reset at block edges.

`createTemperatureCausalGuardAudit` in `temperature-causal-guard.ts` tests one
fixed decision rule between raw and the frozen half-strength analog prediction.
It only changes leads one through twelve. Source forecasts have a strictly
earlier reference, assumed observation availability at `validAt + 1 hour` no
later than the target reference, and valid times within the preceding 168
elapsed hours. Sources share the target's one-through-six or seven-through-
twelve lead band and Los Angeles valid-time daypart. Selection retains one
forecast per source valid hour: closest target lead, then latest reference,
then greatest canonical key. Selection does not inspect forecast errors.

At least twelve distinct source valid hours spanning three local dates are
required. Source MAE is averaged within each source date and then equally over
represented source dates. If the frozen half-strength prediction's source MAE
is greater than or equal to raw's, the target uses exact raw; otherwise it keeps
exact half strength. Unsupported targets also retain half strength. No extra
clipping, native prediction, fit, parameter search, or recursive use of guarded
source predictions occurs. Every prediction after twelve hours is unchanged.
Private audits retain support, source identities, losses and decisions.

Causal selection is not a guarantee of future no-harm: small source samples can
choose badly, and observation arrival/revision latency remains assumed. The
candidate must report every adverse date/cell, source-support shortage and
raw/half decision, and pass future-label isolation tests. Existing historical
selection and deployment limitations remain; no application integration or
production model activation is performed by this research helper.

Previous Runs' whole-day offsets cannot recreate subdaily issue histories.
Historical Forecast stitches early run output; Single Runs exposes run
initialization but does not establish equivalence with the stored legacy-v4
Best Match product or reconstruct historical public availability. Seasonal
proxy research must remain distinct from same-product near-term validation.
See the official [Previous Runs](https://open-meteo.com/en/docs/previous-runs-api),
[Historical Forecast](https://open-meteo.com/en/docs/historical-forecast-api), and
[Single Runs](https://open-meteo.com/en/docs/single-runs-api) documentation.

## Recorded history in forecast charts

The elapsed portion of each forecast chart uses recorded hourly values in both
display modes. The fixed web-edge `forecast-observations` route fully pages the
existing core physical and regional-current history from the earlier of local
midnight minus three hours or the fixed server timestamp minus 24 hours and
20 minutes. The additional pairing halo recovers still-live daytime cloud
evidence across midnight without changing the plotted current-day geometry or
public `{ generatedAt, data }` observation envelope. The shared fill retains its
20,000-row, 24-MiB, 128-pages-per-kind and 30-second bounds; daylight pairing also
has a 64,000-comparison cap and refuses rather than publishing a partial result. A completed compact projection
is shared for five minutes; failed, oversized, timed-out or cursor-invalid fills
are not published. Paging is not a database snapshot: later revisions are
incorporated on the next fill. Core, schema and model evidence capture are unchanged.

Selection is independent per metric and hour: Ballydidean's exact Ecowitt source,
then an equal mean of available local stations, then recorded regional current
values. Each source is reduced to an hourly scalar mean before duplicate station
feeds are collapsed. Gust uses the preceding-hour recorded maximum; rain uses
direct measured rates, never per-report precipitation increments. Maintenance-only
rain-target sources are excluded. Three-hour pressure changes require four
consecutive hourly bins from the same source, so station fallback changes do not
manufacture pressure jumps. Projection tiers, sample/station counts and partial-hour
recorded-through timestamps remain available in the compact read-only response.

If no recorded value is available for a field, the graph retains its raw regional
forecast value, not an adjusted estimate. Complete projection failure leaves the
future forecast usable and displays a recorded-history availability message.
Adjustment controls affect only future plotted values. The homepage current
source preference and forward correction models are unchanged.
