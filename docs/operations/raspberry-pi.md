# Raspberry Pi operations

This runbook describes the isolated Weather deployment for the ARM64 host
`blueberry`. It adapts the health-gated release pattern used by the neighboring
application without sharing that application's paths, identities, networks,
credentials, connector, unit, or lifecycle. Repository verification is
deliberately static and does not perform a production deployment.

## Isolation contract

- Compose project: `weather`
- release root: `/opt/weather/current`
- durable data: `/var/lib/weather/postgres` and `/var/lib/weather/xweather/usage.json`
- encrypted rolling backup: local `deploy/backups/weather-nightly.dump.age`
- application UID/GID: `10002:10002`
- SSH account: `weather-ssh`
- systemd unit: `weather-compose.service`
- public origin after the private gates: `https://weather.ballydidean.farm`

The production Compose file publishes no host ports. `edge`, `web_api`, and
`data` are internal. Only the worker joins `provider_egress`, only the web edge
joins `map_egress`, and only cloudflared joins `tunnel_egress`. The Weather
connector token is not interchangeable with any other connector token.
The service-level `mem_limit` values are authoritative on ordinary Compose;
do not run a host policy that widens those limits after container creation.

## Home-network homepage panels

The homepage shows indoor temperatures and soil moisture to administrators and
visitors sharing Blueberry's public IPv4 egress. This deliberately includes other
networks sharing the same home internet connection; it is not a private-subnet
check for `192.168.11.0/24`.

The web edge serves `/api/v1/viewer-context` with `private, no-store` caching and
returns only a `homeNetwork` boolean. It trusts `CF-Connecting-IP` only when the
immediate socket peer matches the isolated Compose `cloudflared` service's DNS
address. Forwarded-address chains, Workers subrequests, and IPv6 visitor headers
do not grant eligibility. See Cloudflare's [visitor-header reference](https://developers.cloudflare.com/fundamentals/reference/http-headers/).

Blueberry discovers its IPv4 egress over HTTPS using the `ip` field in
[Cloudflare's trace diagnostic](https://developers.cloudflare.com/privacy-proxy/get-started/).
Discovery is bounded, shared between concurrent requests, and refreshed after
60 seconds. Missing, expired, malformed, or failed discovery hides the extra
panels for non-admins; it never reuses stale addresses. The trace endpoint is a
diagnostic service rather than a guaranteed availability contract. IPv6-only
visitors do not match; a VPN using a different egress also prevents recognition.
Discovery failures emit one sanitized warning per refresh interval without
logging visitor addresses or upstream error details.

The browser does not persist eligibility in HTML, service-worker caches, or
local storage, and rechecks while the homepage is visible and after reconnecting
or returning to the tab. Network recognition never supplies an admin identity,
admin cookie, editor access, or write authorization. Existing sensor readings and
layout endpoints remain public: this feature controls homepage presentation,
not confidentiality of the underlying sensor API.

No host listener, router/DNS change, or control-plane installation is required;
the detector ships with the web image through the ordinary release process.

## Provisioning

1. Create `/opt/weather`, `/var/lib/weather/postgres`, `/var/lib/weather/xweather`,
   and `/var/lib/weather/backups` without touching neighboring application paths.
2. Provision the PostgreSQL directory for the pinned image's `999:999` user,
   provision the Xweather directory for `10002:10002`, and make
   `/opt/weather/current` root-owned after release extraction.
3. Copy `deploy/.env.example` to ignored `deploy/.env`. Set the server and web
   repositories, the pinned PostgreSQL tag, and the pinned cloudflared tag used
   as staging inputs. Staging replaces all four with exact Linux ARM64
   `name@sha256:<digest>` references; `WEATHER_RELEASE` remains a label only.
   Staging also records a digest and compatibility version for Compose,
   lifecycle scripts, and the runtime ACL contract. Stage, activate, recover,
   and rollback fail closed on an incompatible control-plane version or digest.
   Control-plane version 11 allowlists only the verified installed version 10
   production digest `b9a5cd866f7ec62b0ee32339340186d28041d32c9d65df5719c0f064b27ca722`
   as its predecessor. Versions 9 through 11 use the same eleven-field
   release format; administrator adjustment settings are stored separately in
   the backed-up web volume.
   All other cross-version or cross-digest handoffs are unsupported. Do not
   rewrite release metadata or use wildcard handoffs: every mutating lifecycle
   action rejects any other mismatch before changing images, containers, the
   database, or release state.
4. Generate separate long random administrator, owner, API, and ingestion
   passwords. The administrator and owner values must differ. Install the four
   `weather_postgres_*` sources for `999:999`; the administrator source is
   mounted only into PostgreSQL. Install matching owner, API, and ingestion
   application copies as `weather_migration_owner_password`,
   `weather_api_password`, and `weather_worker_ingest_password` for
   `10002:10002`. Install `weather_tempest_api_key`,
   `weather_xweather_client_id`, and `weather_xweather_client_secret` for
   `10002:10002`, and the tunnel token for `65532:65532`. Every source must be
   mode `0400`; Compose file secrets cannot repair host ownership or modes.
5. Copy `deploy/config/backup.env.example` to ignored `backup.env` and set only
   the public age recipient matching the local identity. The nightly pull stores
   only ciphertext in the repository's ignored `deploy/backups/` directory.
6. Install root-owned scripts as `/usr/local/bin/weather-ssh-dispatch` and
   `/usr/local/sbin/weather-remote-ops`; validate the sudoers rule with
   `visudo -cf deploy/sudoers/weather-ops`.
7. Configure an Ed25519 key in `weather-ssh`'s `authorized_keys` with the forced
   command `/usr/local/bin/weather-ssh-dispatch`, no agent/port/X11 forwarding,
   and no PTY. Configure `sshd` with `PermitRootLogin no`,
   `PasswordAuthentication no`, and a `Match User weather-ssh` block.
8. Install and enable the root-owned `weather-compose.service`. It has no
   dependency on another application unit.

## Independent dashboard releases

The dashboard has a separate, web-only release lane while the core remains on
the exact `2026.10.09-1` runtime and schema 0018. It requires the verified v14
control installer over that release's frozen v13 control identity. It does not
run the unrelated compatibility bridge or full maintenance migration.

Publish a new immutable tag only after its exact branch commit passes `Check`.
The existing image publication workflow must pass the ARM64 image-boundary
inspection. Then use the bounded forced-command wrapper:

```bash
deploy/scripts/ssh-run.sh dashboard-release 2026.10.10-1 2026.10.09-1 2026.10.09-1
deploy/scripts/ssh-run.sh status
```

The operands are target dashboard release, expected core release and expected
current dashboard release. Before the first override, the dashboard source is
the core release. Later updates require the exact separately committed dashboard
source. A moved core tag, incomplete or unsuccessful exact-commit workflow,
different publication tag, or source mismatch refuses the release.

Only the target web image is pulled and only the `web` service is restarted.
The existing server, database and tunnel images, non-web containers, complete
migration ledger, canary settings and administrator adjustment settings are
checked for drift. No migrations, model actions, capture epochs or registration
initialization run. Literal OCI-layer capacity accounting retains the existing
protected byte/inode floors and next-capture, pull and metadata reserves. It
does not count prospective image deletion as capacity.

Dashboard metadata is separate from the core lifecycle:

- `deploy/releases/<release>.web.env`
- `deploy/state/current-web-release`
- `deploy/state/previous-web-release`
- `deploy/state/active-web.env`
- private `deploy/state/dashboard-web-transaction.env` while a transaction is
  incomplete

The core `current-release`, `previous-release`, `schema-release` and `active.env`
remain unchanged. Consequently, the public HTML reports the new dashboard
release while `/api/v1/health` continues to report the retained core release.
This is intentional component provenance, not a partially migrated core.

The transaction verifies exact running web bytes and public-query compatibility
before committing its current dashboard marker last. A failed switch restores
the exact source web. `recover` reconciles an interrupted dashboard transaction
and restores a committed override after exact schema-0018 core recovery,
including host restarts. The pre-epoch recovery render excludes future-only
bind mounts and preserves already-running non-web containers. Roll back through
the same bounded lane:

```bash
deploy/scripts/ssh-run.sh dashboard-rollback
deploy/scripts/ssh-run.sh dashboard-rollback-core
```

Keep the committed core release's web image cached even while a dashboard
override is active: recovery verifies the complete core image set. If that
image is missing, restore the exact `WEATHER_WEB_IMAGE` digest from the committed
core release environment before retrying recovery; do not substitute the
dashboard image or rewrite release metadata.

`dashboard-rollback` selects the retained previous dashboard. After multiple
dashboard updates, use `dashboard-rollback-core` to return directly to the exact
core web and remove all override markers after its health checks pass.

Unified releases, model-family releases, the pinned compatibility bridge/full
maintenance handoff and v14 control recovery refuse active or interrupted
dashboard overrides. Finish dashboard recovery and rollback first; do not
rewrite core metadata or manually swap the web container. Once the override
markers are removed, the immutable `2026.10.09-2`/`2026.10.09-3` handoff remains
available through its original reviewed operations.

## Schema-preserving application releases

The revised schema-23 release `2026.11.03-3` uses the dedicated
`scripts/schema-preserving-release.sh` application-only lane. Do not run ordinary
`stage`, `activate`, `rollback`, or `recover` for its application descendants:
those installed commands still describe the older schema-21 upgrade.

This lane admits only the complete reviewed revised schema-23 history. It
preserves the installed version-17 deployment bundle, database, PostgreSQL and
tunnel images, canary switches, model settings, catalog, and genuine capture
epoch. It does not clone or migrate the database, apply grants, initialize model
state, or create another database backup. The unchanged `schema-release` remains
`2026.11.03-3`; `current-release`, `previous-release`, and `active.env` describe
the independently released application images.

Publish the immutable target tag only after its exact branch commit passes
`Check`. The normal ARM64 publication workflow must finish its image-boundary
inspection and confirm that published configurations equal the inspected
configurations. From the exact tagged checkout, prepare a private publication
document with:

```bash
node scripts/prepare-schema-preserving-publication.mjs \
  TARGET_RELEASE /absolute/private/directory/publication.json
```

The preparer verifies the local and GitHub tag, exact successful workflows,
published digest artifacts, ARM64 image configurations, and unchanged protected
deployment/database/model file contents. It does not pull images or fetch another
database. Retain its publication document with the release evidence.

An authorized host administrator installs the two-file operator bundle with
`scripts/install-schema-preserving-release.sh BUNDLE_DIRECTORY BUNDLE_SHA256`.
The bundle contains only `schema-preserving-release.mjs` and
`schema-preserving-release.sh`. Its SHA-256 hashes the two `sha256sum` manifest
lines in that basename order, including the final newline. The installer
verifies the exact existing source and control identities, installs the bundle
under `/opt/weather/operators/schema-preserving-release/BUNDLE_SHA256/`, and
adds a separately recorded systemd recovery drop-in. It reloads and verifies the
effective unit without restarting the service; the original stop command is
retained. Its ten-minute start allowance covers retained infrastructure and
application health windows, including a slow cold boot. Installed deployment
files and their version-17 digest are unchanged.

Replacing an installed auxiliary bundle is a separate guarded operation:

```bash
sudo scripts/install-schema-preserving-release.sh --upgrade \
  /absolute/root/private/bundle NEW_BUNDLE_SHA256 EXPECTED_OLD_BUNDLE_SHA256
```

Use the installer and bundle from the same validated checkout. The handoff accepts
only the explicitly named predecessor, its exact private receipt, unchanged base
unit and effective recovery commands, the retained schema owner, and a healthy
committed application descendant with no unfinished release journal. It verifies
the complete installed control digest before sourcing any control code.

The new bundle and immutable private receipt under
`deploy/state/schema-preserving-release-v1/installations/NEW_BUNDLE_SHA256.json`
are durable before the drop-in is atomically replaced. The new helper reads its
own receipt; the original `installation.json` remains untouched for the old
helper. Thus either side of an interrupted switch retains valid boot authority.
An exact retry can finish a pending switch or verify an already completed one.
Failures restore the exact predecessor drop-in and effective commands. Retain
both bundles, both receipts, and the private handoff metadata snapshots. The
handoff does not recreate containers, pull images, or change database, model,
capture-epoch, or backup data. Afterward, exercise the new helper's
`system-recover` and verify retained container identities and service health.

Use the installed, content-addressed helper for the entire lifecycle:

```bash
operator=/opt/weather/operators/schema-preserving-release/BUNDLE_SHA256/schema-preserving-release.sh
sudo "$operator" stage TARGET_RELEASE /absolute/root/private/publication.json PUBLICATION_SHA256
sudo "$operator" activate TARGET_RELEASE
sudo "$operator" status
# restore only the retained application images
sudo "$operator" rollback
# reconcile an interrupted switch or service restart
sudo "$operator" recover
```

If staging is interrupted before activation, `sudo "$operator" retire-stage
TARGET_RELEASE` quarantines only that uncommitted stage and its unused release
environment before an exact retry. It preserves the failed evidence and refuses
current/previous releases, committed health receipts, or an active transaction.
It never deletes an application image or a database file.

Staging checks both application images against the live database through
read-only API-role transactions. Activation replaces only API, worker, and web
containers. A private journal records the exact staged pair before the switch;
health and protected-state checks precede the `current-release` commit point.
Recovery selects the committed side of that journal. Restart recovery starts
retained infrastructure without recreating its containers and never invokes a
migration. Unknown histories, unrelated release pairs, altered receipts, changed
protected state, or modified boot commands fail closed.

Keep the installed bundle, recovery drop-in, installation receipt, staged
receipts, and current/previous images. Removing the auxiliary recovery path while
an application descendant is active is unsafe. Future adjustment-family release
operations still require a separate ancestry-aware controller repair; this lane
deliberately does not relabel historical model or capture evidence.

## Optional capacity diagnostics

The direct deployment path does not require a capacity sample. Run the retained
diagnostic manually when investigating host pressure:

```bash
sudo /opt/weather/current/deploy/scripts/preflight-capacity.sh \
  --sample-seconds 900 \
  --json /var/lib/weather/preflight-$(date -u +%Y%m%dT%H%M%SZ).json
```

The gate requires `aarch64`, at least four CPUs, 15-minute load no greater than
`0.50 * CPUs`, minimum available memory of 1,792 MiB, swap traffic no greater
than 1 MiB/minute, at least 10 GiB and `3 * database bytes + 5 GiB` free under
`/var/lib`, at least 4 GiB free in Docker's data root, and at least 10% free
inodes. A failed diagnostic does not authorize pruning Docker or changing
neighboring services.

## Database read-latency recovery

The historical archive dominates `weather_records`, so PostgreSQL's default
table-wide maintenance thresholds can leave the smaller live-source datasets
with stale planner statistics and poor visibility-map coverage. Check
`pg_stat_user_tables`, `pg_stat_progress_vacuum`, and the affected query's
`EXPLAIN (ANALYZE, BUFFERS)` before changing query timeouts or adding indexes.

Run this bounded online maintenance from an authorized host administrator shell.
The separate `-c` arguments keep `VACUUM` outside a transaction. Cost throttling,
disabled truncation, and disabled parallel vacuum limit interference with the
live stack; do not substitute `VACUUM FULL`.

```bash
sudo docker exec -u postgres weather-postgres-1 psql -X \
  -v ON_ERROR_STOP=1 -d weather \
  -c "SET lock_timeout='5s'; SET statement_timeout='30min'; SET vacuum_cost_delay='2ms'; SET vacuum_cost_limit=200; SET maintenance_work_mem='64MB';" \
  -c "VACUUM (ANALYZE, VERBOSE, TRUNCATE FALSE, PARALLEL 0) public.weather_records;"
```

After maintenance completes, the production table uses these persistent,
table-local overrides rather than widening global resource limits:

```sql
BEGIN;
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '10s';
ALTER TABLE public.weather_records SET (
  autovacuum_vacuum_scale_factor = 0.0005,
  autovacuum_vacuum_insert_scale_factor = 0.0005,
  autovacuum_analyze_scale_factor = 0.0005
);
COMMIT;
```

At roughly 12.5 million rows, these retain the default base thresholds while
making maintenance eligible after approximately 6,300 changed rows instead of
more than a million. The previous `0.001` factors had not triggered maintenance
after roughly 7,000 changes following the October 27, 2026 host restart. On
October 28, forecast reads took 5–11 seconds and exceeded the web edge's
five-second deadline. The bounded maintenance above reduced the latest-product
query's heap fetches from 35,794 to 6; desktop and mobile forecast requests then
returned successfully in 611–1,000 milliseconds. The factors were lowered to
`0.0005` to make automatic maintenance eligible sooner.

Verify the overrides after restoring a database, and watch maintenance duration,
application latency, and query heap fetches for 48–72 hours. These are
workload-specific starting values, not a universal PostgreSQL preset. At that
time, the latest forecast still scanned historical product identities;
maintenance alone reduced heap reads but did not remove that archive-growth
cost.
To return to inherited defaults, use the same short lock timeout with
`ALTER TABLE public.weather_records RESET (autovacuum_vacuum_scale_factor,
autovacuum_vacuum_insert_scale_factor, autovacuum_analyze_scale_factor)`.

See PostgreSQL 17's [vacuuming guidance](https://www.postgresql.org/docs/17/routine-vacuuming.html)
and [online VACUUM options](https://www.postgresql.org/docs/17/sql-vacuum.html).

## Validation and release pipeline

`Check` is the authoritative validation run for a commit. Push the branch before
creating its immutable release tag. Publishing waits for a successful `Check`
push run from this repository at the exact tag commit; a different commit, pull
request run, failed attempt, or missing validation cannot authorize publication.

Every check runs text/import lint, a dependency-ordered TypeScript build, fast
unit tests, static deployment checks, and a small browser smoke. The compiler
checks the same strict TypeScript configuration while emitting the files needed
by tests; there is no separate repeat of the entire no-emit/build chain.

Expensive suites run when the changed area requires them:

- model code and configuration: synthetic research and retained-promotion tests
- backend, database and shared dependencies: real PostgreSQL integration
- frontend and its dependencies: the full responsive browser suite
- deployment, containers, migrations and shared infrastructure: the complete
  disposable Compose upgrade, isolation, backup/restore and recovery rehearsal

Missing or unverified comparison baselines and unknown paths select full
coverage. Nightly and manually dispatched checks also run every suite. A change
filter never suppresses the entire workflow or replaces a failing check with
success. Fast model tests retain runtime and authorization safeguards; neither
test profile is a live forecast-accuracy qualification.

Native ARM64 runners build the production server and web targets using a shared
builder. Both candidate images are inspected for the model-filesystem boundary
before publication, then their cached layers are published with immutable
release tags and ARM64 digest artifacts. No emulated TypeScript compilation or
second copy of the repository test suite is required during publishing.

For local work, run the narrow affected tests after the final edit. `npm run
check` remains the full local check; `npm run check:fast` runs the fast profile.
The `:compiled` test commands consume an existing successful build, so run
`npm run build` first when invoking them directly. Do not repeat a full local
pass solely because the same exact commit is about to run in CI. The production
release identity, migrations, permissions, health checks and affected live-page
verification remain required.

## Direct deployment

After completing Weather changes, deploy the validated result to `blueberry`
through the release process below. Completion requires checking the live release
and affected browser behavior at `https://weather.ballydidean.farm`; a successful
local preview alone is not the completion gate.

Use the key-based client wrapper:

```bash
deploy/scripts/ssh-run.sh status
deploy/scripts/ssh-run.sh yolo 2026.08.25-1
deploy/scripts/ssh-run.sh status
```

The equivalent repository commands are:

```bash
npm run remote:status
npm run remote:deploy -- 2026.08.25-1
npm run remote:tunnel:check
npm run remote:tempest:backfill
npm run remote:public-stations:backfill
npm run remote:backup:pull
```

The SSH-backed commands require a loaded agent containing the deployment key.
The HTTPS tunnel check and test URL commands do not require SSH access.
The forced-command `ssh-run.sh yolo` uses bootstrap `deploy/.env`, not the
active release's settings. When an immutable new release must preserve reviewed
canary settings, use the host-administrator `update.sh yolo RELEASE --from`
form with the exact absolute path to the reviewed prior release file. Confirm
the new target release file does not already exist: `yolo` reuses an existing
rendered target and then ignores `--from`. Review the exact source first; a
kill-switch value of `1` kills its canary, while `0` permits the already
authorized runtime. The source also supplies database identity, application
image repositories, and infrastructure image references; the new release tag
is resolved separately to immutable application image digests.
Do not edit a published release environment to change these values.

For the `2026.09.13-2` canary-restoration release, the reviewed source is the
unchanged `2026.09.12-1.env` with both kill switches at `0`:

```bash
sudo /opt/weather/current/deploy/scripts/update.sh yolo 2026.09.13-2 \
  --from /opt/weather/current/deploy/releases/2026.09.12-1.env
```

The Tempest backfill command imports every active configured station through
yesterday, resumes only exact successful chunks, and stores its private report
under `/var/lib/weather` on the server.

### One-time version-eight to version-nine host handoff

The forced-command release client runs the already-installed `update.sh`; an
image-only `yolo` cannot install the new Compose, ACL, or lifecycle files. Before
the first rain-collection release, use a serialized host-administrator window
with no Weather `yolo`, stage, activate, rollback, recover, backup, restore, or
migration in flight. Keep the existing containers running. Do not modify
`deploy/.env`, `deploy/releases`, `deploy/state`, or `deploy/secrets`.

Build a candidate only from the reviewed immutable release tag, not the dirty
worktree. The five installed files are `deploy/compose.yaml`,
`deploy/postgres/runtime-acl-v2.sql`, `deploy/scripts/common.sh`,
`deploy/scripts/forecast-training-package.mjs`, and
`deploy/scripts/update.sh`. Replace `REVIEWED_TAG` with the actual tag:

```bash
tag=REVIEWED_TAG
git rev-parse --verify "$tag^{commit}"
handoff=$(mktemp -d)
git -c tar.umask=0022 archive --format=tar --output="$handoff/control.tar" "$tag" \
  deploy/scripts deploy/postgres deploy/systemd deploy/sudoers deploy/compose.yaml
git show "$tag:scripts/install-rain-control-plane.sh" >"$handoff/install.sh"
tar --no-same-owner --same-permissions -xf "$handoff/control.tar" -C "$handoff"
archive_sha=$(sha256sum "$handoff/control.tar" | awk '{print $1}')
helper_sha=$(sha256sum "$handoff/install.sh" | awk '{print $1}')
candidate_sha=$(bash -c 'source "$1"; control_digest "$2"' \
  handoff "$handoff/install.sh" "$handoff")
remote_handoff=$(ssh blueberry 'umask 077; mktemp -d "$HOME/.weather-rain-control.XXXXXX"')
scp "$handoff/control.tar" "$handoff/install.sh" "blueberry:$remote_handoff/"
ssh blueberry "printf '%s  %s\n%s  %s\n' \
  '$archive_sha' '$remote_handoff/control.tar' \
  '$helper_sha' '$remote_handoff/install.sh' | sha256sum -c -"
printf 'admin_stage=%q\narchive_sha=%q\nhelper_sha=%q\ncandidate_sha=%q\n' \
  "$remote_handoff" "$archive_sha" "$helper_sha" "$candidate_sha"
ssh -t blueberry
```

In that host-administrator session, enter `sudo -s` using the protected
interactive prompt. Paste the four exact assignment lines printed above, then
run the following in the root shell. The uploaded helper is never executed
directly: root first copies it into new private staging and verifies that copy.

```bash
set -euo pipefail
umask 077
install -d -m 0700 /var/lib/weather/control-plane-backups
root_handoff=$(mktemp -d /var/lib/weather/control-plane-backups/.handoff.XXXXXX)
trap 'rm -r -- "$root_handoff"' EXIT
install -m 0700 "$admin_stage/install.sh" "$root_handoff/install.sh"
printf '%s  %s\n' "$helper_sha" "$root_handoff/install.sh" | sha256sum -c -
bash "$root_handoff/install.sh" \
  "$admin_stage/control.tar" "$archive_sha" "$candidate_sha"
exit
```

The verified root-private helper requires the exact installed `2026.09.12-1`
version-eight release metadata and pinned predecessor digest. It snapshots and rehashes the
archive in private staging, allows only reviewed paths, enforces safe modes,
retains an exact private version-eight backup, and tests that only those five
files produce the candidate digest. It then replaces files by same-directory
atomic rename with `update.sh` last, verifies the final digest, and restores the
backup on a normal failure. Its private lock excludes another invocation of
this helper only; it does **not** exclude the existing release client. An
operator must maintain the serialized window throughout. The five renames are
not one transaction: a host crash or `SIGKILL` cannot run shell rollback. If
interrupted, inspect the retained backup and installed digest before any
release command; restore the exact version-eight files as an administrator if
needed. The version-nine allowlisted handoff trusts this verified root-owned
installation; it does not independently attest arbitrary later host-file drift
while the previous release still has version-eight metadata.

After the helper succeeds, use the ordinary `yolo` release command above, then
verify status and live behavior. Do not repeat the helper after version nine is
installed, and do not rewrite the previous release metadata to bypass a guard.

### One-time version-nine to version-ten station-capture handoff

Use the same serialized, hash-verified, five-file root handoff above for the
immutable station-capture release tag, but use the installer from that new tag.
Its exact predecessor is active `2026.09.13-2`, control-plane version 9, and
digest `0ee05795357e59877a1bf210da6a8147519c0ad46640f104a09a27d7c2fdf6e0`.
It retains a private version-nine backup and installs version 10. The previous
version-eight handoff helper must not be reused. Check the live release, digest,
quiet lifecycle window, and absent target release file before starting; neither
the helper lock nor `yolo` protects against a concurrent administrator command.

After the helper succeeds, deploy the already-published and validated immutable
release through the host-administrator command below. The exact active `2026.09.13-2.env` is the
source because default forced-command `yolo` instead reads bootstrap `.env` and
could kill the already-authorized wind and temperature canaries. Confirm both
source kill switches are `0` and that `2026.09.13-3.env` does not already exist.
The station-access migration permits only the frozen prospective capture policy;
it does not activate a rain adjustment model or retroactively qualify old data.

```bash
sudo /opt/weather/current/deploy/scripts/update.sh yolo 2026.09.13-3 \
  --from /opt/weather/current/deploy/releases/2026.09.13-2.env
```

Verify the live version, schema ledger through `0015_rain_station_access.sql`,
forecast and station receipt counts, and the original active wind and
temperature canary bundle identities. Keep the retained old environments and
backup immutable; do not rewrite old control-plane metadata to bypass a guard.

### One-time version-ten to version-eleven live-adjustment handoff

Use the same hash-verified, serialized root handoff steps above with the new
immutable release tag and that tag's installer. This handoff requires active
`2026.09.13-3`, control-plane version 10 and digest
`b9a5cd866f7ec62b0ee32339340186d28041d32c9d65df5719c0f064b27ca722`.
It retains the exact version-ten control plane in a new private backup. The
seven allowlisted replacement files are `deploy/compose.yaml`,
`deploy/postgres/runtime-acl-v2.sql`, `deploy/scripts/common.sh`,
`deploy/scripts/forecast-training-package.mjs`,
`deploy/scripts/weather-admin-store.mjs`, `deploy/scripts/web-server.mjs` and
`deploy/scripts/update.sh`. The earlier version-nine-to-ten helper must not be
reused. Verify no lifecycle or migration command overlaps this handoff.

After the helper succeeds, deploy the published immutable release from the
active `2026.09.13-3.env`, not bootstrap `.env`; preserve the existing wind and
temperature kill-switch values. Ensure the target release environment does not
already exist before running `yolo`. The new migration adds only bounded rain
model outputs and preserves private capture evidence. Check schema migration
`0016_rain_adjustment.sql`, real forecast adjustment decisions, each admin
switch and the all-off raw forecast before reporting success. Do not re-run
the helper after version eleven is installed, rewrite the old release metadata,
or remove the retained backup. The web volume retains both
`forecast-adjustment-settings.json` and its `.initialized` marker. Initial
enabled defaults require an existing valid admin-auth record; missing or
damaged retained state fails closed, and an authenticated switch update repairs
it. Preserve these files in the normal encrypted backup. See
[rain adjustment operations](rain-live-adjustment.md).

## Xweather map budget

The forecast map requests one 256×168 single-layer static image per provider
frame, which Xweather counts as one raster map unit. The browser permanently
caches historical frames, retains forecast frames for one hour, and warms only
the seven frames nearest the selected time for the selected layer. The server
coalesces concurrent misses, retains historical frames for its process lifetime,
and refreshes forecast frames on demand after one hour. There is no scheduled
all-layer refresh and no public manual refresh operation.

The server reserves every provider miss in
`/var/lib/weather/xweather/usage.json` before issuing it. The persisted ceiling
is 300 map units per UTC day and 10,000 per UTC month, leaving at least 5,000 of
Xweather's shared 15,000-access free tier for other account activity. The ledger
survives deploys and container restarts. When enabling the ledger after earlier
account use, seed its current `monthUnits` and `dayUnits` from the Xweather
dashboard before starting the web service; never reset it to recover capacity.

The non-secret remote host and tunnel origin are recorded in
`config/runtime-targets.json`. `npm run test:urls` prefers that tunnel;
`npm run test:urls:local` is the explicit loopback override.

`yolo` resolves and records all four exact ARM64 manifest digests, validates the
Compose render, pulls images, applies checked forward migrations, starts the
Weather stack with Compose health gates, and records release state last. It does
not require capacity evidence, create a compatibility database, or create a
deployment-time backup. The nightly local encrypted backup is the recovery copy.
Migration `0014_rain_collection.sql` adds private immutable capture claims and
receipts plus an aggregate-only status view. The production worker collects
prospective rain evidence. Migration `0015_rain_station_access.sql` authorizes
the same fixed station catalog under the new exact capture-policy digest; neither
migration activates a rain adjustment model.

## Legacy staged diagnostics

Stage requires a passing 15-minute capacity result from the prior hour, resolves
and records all four exact ARM64 manifest digests, validates the complete Compose
render, and pulls without changing running services. On upgrades it creates and
drops only a unique compatibility database, applies the candidate's trailing
migrations, runs the previous API image's `/api/v1/health`, `/api/v1/sites`,
`/current`, and `/history` reads, and runs one previous worker loop against a
credential-free deterministic provider stub. The existing release must match
the installed control plane before these checks. When staging adds migration
history, the same previous API and worker images must first reject the candidate
ledger without the exact authorization, so a binary that ignores the
authorization contract cannot be staged for rollback. Code-only upgrades keep
exact ordinary readiness and do not require a rejection probe. Only after every
applicable rejection and real previous-image check passes, staging publishes a
no-replace private authorization bound to that previous release and the exact
ordered candidate migration ledger. The first release records compatibility as
not applicable.

Activate requires the pre-provisioned Weather connector token, creates an
encrypted pre-migration backup, runs forward-only checked migrations, starts
the project with Compose health gates, and records the release last. It validates
the existing release against the installed control plane and validates its
rollback authorization before backup or migration. Activation records the
target schema release before the first migration, so a partial migration fails
closed into authorized recovery or operator restore of the encrypted backup.
After a migration-free rollback, every activation except the exact retained
schema release is rejected before capacity checks, backup, Compose, migration,
or state mutation while the runtime trails that schema. Use `recover` to restore
the recorded runtime against that retained schema authorization. The public
hostname route remains a separate, leader-authorized post-activation operation.

## Nightly local backup

The local user timer runs `deploy/scripts/pull-backup.sh` at 02:30. It requests
an age-encrypted PostgreSQL custom dump through the forced-command Weather SSH
account, verifies complete decryption and `pg_restore --list` locally, and then
atomically replaces the gitignored rolling backup and checksum. No plaintext
database dump is written on either host.

After a 15-minute ingestion soak, require at least 512 MiB available memory,
load no greater than `0.75 * CPUs`, no OOM/restart/limit breach, swap no greater
than 1 MiB/minute, and an unchanged neighboring-application baseline.

## Rollback and recovery

```bash
deploy/scripts/ssh-run.sh rollback
deploy/scripts/ssh-run.sh recover
sudo systemctl restart weather-compose.service
```

Rollback applies the previous four immutable Weather images with `--no-deps`; it
never runs backup or migration and never reverses a forward-compatible migration.
Ordinary API and worker readiness rejects every trailing migration. Rollback and
recovery inject the staged release-specific ledger authorization only when an
older proven-compatible image set must run against the retained newer schema;
forward activation clears it, and runtime database roles cannot alter it.
Failed initial activation removes only Weather containers and networks while
preserving `/var/lib/weather`. Cleanup is armed as soon as the first PostgreSQL
service starts, including backup, migration, and health failures. Recovery
reapplies the recorded exact image set after Docker or host restart.

PostgreSQL startup reconciles the mounted administrator secret on both empty and
retained data directories before opening a network listener. Recovery, rollback,
and activation force-recreate only the PostgreSQL container before starting the
runtime images; the data directory is retained. Its health gate authenticates as
`postgres` with the administrator secret rather than relying on `pg_isready`.
To rotate that secret, atomically replace
`deploy/secrets/weather_postgres_admin_password` with another `999:999`, mode
`0400` value that differs from the owner password, then run `recover` through the
normal root-owned operator path. Do not use a plain container restart: recreating
the container is required to pick up an atomically replaced file-secret inode.
Any reconciliation or authenticated-health failure stops recovery before the API,
worker, web, or connector is restarted.

`status` reports connector state, the latest ingestion run, overdue running
work, the latest chunk outcome, and bounded failure classification/code evidence
without printing persisted error messages.

## Local verification

Create local disposable secrets from the examples, then run:

```bash
docker compose --env-file deploy/.env.example \
  -f deploy/compose.yaml -f deploy/compose.local.yaml up -d --build --wait
curl --fail http://127.0.0.1:3000/
docker compose --env-file deploy/.env.example \
  -f deploy/compose.yaml -f deploy/compose.local.yaml down --volumes
```

The local web test URL is <http://127.0.0.1:3000/>. Local database/web bindings
remain on loopback and the local override replaces the real connector command.

For the separate read-only production snapshot, model-evidence, activation,
and restart-only rollback boundaries, see
[`forecast adjustment operations and governance`](forecast-adjustment.md).
