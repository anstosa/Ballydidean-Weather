# Local UV, pressure and cloud forecast corrections

These browser corrections are separate from the trained temperature, wind and
rain maintenance families. Their initial compiled forecast policies are zero:
future values remain regional pending independent evidence. Current farm/local
source priority, current WS90 cloud estimates, humidity, PM2.5, recorded graph
history, existing alerts and actual-difference gold icons remain unchanged.

## Inference and labels

The finite development grid is regional zero plus strengths 0.25, 0.5, 0.75 and
1 at horizons 3, 6, 12 and 24 hours. A future-only projection uses one taper from
the original observation time, never a refreshed expiry or compounded forecast.
Pressure remains a same-source three-hour tendency, including the graph's
isolated prior-vintage context. It does not correct absolute pressure datums.

The evaluator imports the compiled browser functions. Physical pressure targets
use instantaneous endpoints, not the historical graph's hourly averages. UV has
nominal point and preceding/following-hour sensitivity panels because its
upstream hourly timing semantics remain uncertain. Cloud targets use the fixed
pointwise WS90 radiation proxy and clear-sky geometry, not independently measured
cloud fraction. Regional model values never substitute for physical truth.

Five frozen primary cells separate farm and local-mean anchor/target regimes;
cloud remains WS90-only. Cross-tier observations are diagnostics, not a way to
rescue a losing cell. Comparisons use equal local-date, UTC-hour and distinct
retrieval-vintage weights, including regional/no-op hours. Development selects
only the shortest/weakest tied candidate; it cannot qualify a release.

## Build and retrospective development

Compile once before standalone commands:

```bash
npm run build
node scripts/research/evaluate_local_forecast_corrections.mjs \
  retrospective-development \
  --input .weather-data/local-corrections/development-input.json \
  --output .weather-data/local-corrections/development-report.json
```

Retrospective input uses contract
`weather-local-correction-retrospective-development/v1`, compact site geometry,
actual history `retrievedAt`, bounded normalized `historyRows` and retained
`issues` containing assumed `evaluatedAt`, current/forecast records, pressure
context and optional daylight evidence. Use the exported bounded
`captureRetrospectiveDevelopmentHistory` helper for opened public history;
never invent old HTTP receipts or use unrestricted database/export access.
Latest public revisions cannot establish first availability or recover discarded
older revisions. Every retrospective result remains development/counterfactual.

## Finite prospective capture

The capture CLI uses only the configured Weather HTTPS origin and public API.
Its immutable panel input freezes candidates, code/input identities and five
comparison cells before the earliest validation input. It declares 14 local
target dates, a 24-hour issue warm-up, an hourly fixed-minute schedule, a
27-hour development embargo and one label seal per date at date-end plus six
hours. The maximum schedule is 375 real UTC captures, including DST. Do not
create files on additional hosts or install a permanent Blueberry job.
Run the long-lived finite collector from a dedicated, detached worktree of its
exact committed revision on this host. Keep its compiled inference closure and
private evidence isolated from later development edits. Rebuilding or changing
that closure invalidates the panel's code hash; do not reinterpret the panel
under a newer runtime.

The input contains site geometry, five selected cells, the claims, serving-zero
state, target start date/count, capture minute, development end and earliest
validation input. Omit `candidateFrozenAt`: initialization records its actual
creation time after source discovery. Creation must precede every validation
input. The resulting manifest binds the discovery receipt and all frozen fields
in `panelDigest`; every issue, seal and attempt claim must match that digest.
Backdating a manifest or reusing members for a different candidate is rejected.
The frozen `sourceCatalog` includes each physical source's ID, key, provider and
station. Capture and evaluation enforce those tuples for anchors and labels;
the regional current model is a calibration input, never physical truth.

```bash
node scripts/research/capture_local_forecast_corrections.mjs init-panel \
  --origin https://weather.ballydidean.farm/api/v1 \
  --input .weather-data/local-corrections/panel-input.json \
  --root .weather-data/local-corrections/panel

node scripts/research/capture_local_forecast_corrections.mjs run-finite \
  --origin https://weather.ballydidean.farm/api/v1 \
  --panel .weather-data/local-corrections/panel/panel.json \
  --root .weather-data/local-corrections/panel
```

The finite runner captures current, five-day regional forecasts (sliced to the
needed next-day window) and compact daylight evidence. It seals raw physical
history through complete bounded pagination. All private evidence stays under
ignored `.weather-data`, with no device serials, property-sensor data or raw
provider bodies. Source sets, resource ceilings, exact panel identity, HTTP
receipts, hashes and unsupported/missed attempts are retained. An interrupted
label attempt can only recover to an unsupported terminal record, never a fresh
favorable read. Changing a candidate requires a fresh future panel.
The runner continues only terminal per-slot failures with validated durable
witnesses and bounded reason counts. Filesystem or panel-integrity failures stop
the run rather than silently burning the remaining schedule.

## Validation and release

```bash
node scripts/research/evaluate_local_forecast_corrections.mjs \
  --panel .weather-data/local-corrections/panel/panel.json \
  --issues .weather-data/local-corrections/panel/issues \
  --labels .weather-data/local-corrections/panel/labels \
  --clock "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
  --output .weather-data/local-corrections/validation-report.json
```

Before the whole panel closes, validation returns coverage only and does not
open outcomes or report partial gains. Passing requires the predeclared support,
5%/absolute gain, affected-horizon no-harm, UV sensitivity and seeded 10,000
replicate two-date bootstrap gates with fixed M=5. An incomplete or underpowered
panel is pending/unsupported, not zero error or a tested failure.

A candidate computed offline while serving constants are zero is permanently a
prospective causal counterfactual, not an actually issued adjusted forecast.
A passing report cannot activate a model. Nonzero compiled constants need a
second exact code commit, successful Check, immutable image publication/boundary
inspection and source-aware dashboard-only release, followed by live Home and
Forecast verification. Core images, database/schema, admin settings, trained
families and maintenance lineage remain untouched. Follow
[Raspberry Pi dashboard release operations](raspberry-pi.md#independent-dashboard-releases).
