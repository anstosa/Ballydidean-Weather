#!/usr/bin/env bash
set -euo pipefail

# keep the auxiliary controller separate from the immutable installed v17 bundle
readonly sp_anchor=2026.11.03-3
readonly sp_anchor_env_sha=7727f844ca07fdd7fbd7a717976628d5e4f860285d5fe527e990f13632eea72b
readonly sp_control_sha=6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb
readonly sp_update_sha=ea1185e04e0da2439c856b5b6989ce4f87f087a63ff7bafa1781c577cf42b8a0
readonly sp_root=/opt/weather/current/deploy/state/schema-preserving-release-v1
readonly sp_update=/opt/weather/current/deploy/scripts/update.sh
state_dir=/opt/weather/current/deploy/state
releases_dir=/opt/weather/current/deploy/releases
deploy_dir=/opt/weather/current/deploy
compose_file=/opt/weather/current/deploy/compose.yaml
sp_module=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/schema-preserving-release.mjs
sp_bundle=$(dirname "$sp_module")
sp_journal="$sp_root/journal.json"

# read a bounded root-private JSON primitive without evaluating shell input
sp_field() {
  node --max-old-space-size=64 "$sp_module" field "$1" "$2"
}

# hash one exact regular file
sp_sha() {
  sha256sum -- "$1" | awk '{print $1}'
}

# reject substituted privileged inputs
sp_private() {
  [[ "$1" == /* && -f "$1" && ! -L "$1" && "$(realpath -e -- "$1")" == "$1" &&
    "$(stat -c '%u:%g:%a:%h' -- "$1")" == 0:0:600:1 &&
    "$(stat -c '%s' -- "$1")" -le 1048576 ]] || die "unsafe schema-preserving input: $1"
}

# calculate the complete pinned control identity without executing any of it
sp_control_digest() {
  local deploy=$1 root file
  local -a files
  root=$(dirname "$deploy")
  mapfile -d '' -t files < <(
    find "$deploy/scripts" "$deploy/postgres" "$deploy/systemd" "$deploy/sudoers" \
      -type f -print0 | LC_ALL=C sort -z
  )
  files+=("$deploy/compose.yaml")
  # preserve the installed relative-path digest algorithm
  for file in "${files[@]}"; do
    printf '%s\0' "${file#"$root/"}"
    sp_sha "$file"
  done | sha256sum | awk '{print $1}'
}

# load only the exact installed updater and its stable primitives
sp_load() {
  [[ "$EUID" == 0 && -f "$sp_update" && ! -L "$sp_update" &&
    "$(stat -c '%u:%g:%a:%h' "$sp_update")" == 0:0:755:1 &&
    "$(sp_sha "$sp_update")" == "$sp_update_sha" &&
    "$(sp_control_digest "${sp_update%/scripts/update.sh}")" == "$sp_control_sha" ]] || {
    printf 'error: schema-preserving release requires the exact root-owned v17 updater\n' >&2
    return 1
  }
  # shellcheck disable=SC1090,SC1091
  source "$sp_update"
  # shellcheck disable=SC2154
  [[ "$control_plane_version" == 17 && "$(control_plane_digest)" == "$sp_control_sha" ]] ||
    die "schema-preserving installed control identity differs"
  [[ "$deploy_dir" == /opt/weather/current/deploy && "$state_dir" == /opt/weather/current/deploy/state &&
    "$compose_file" == /opt/weather/current/deploy/compose.yaml &&
    "${WEATHER_COMPOSE_PROJECT_NAME:-weather}" == weather &&
    -z "${DOCKER_HOST:-}" && "${DOCKER_CONTEXT:-default}" == default ]] || die "release host or project override differs"
  # this lane never carries historical migration authority into an application
  unset WEATHER_MIGRATION_AUTHORIZATION_RELEASE WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256
  local expected manifest file mode
  expected=${sp_bundle##*/}
  [[ "$sp_bundle" == "/opt/weather/operators/schema-preserving-release/$expected" &&
    "$expected" =~ ^[a-f0-9]{64}$ && ! -L "$sp_bundle" &&
    "$(stat -c '%u:%g:%a' "$sp_bundle")" == 0:0:755 ]] || die "operator bundle location differs"
  manifest=
  # bind both auxiliary files to the content-addressed bundle
  for file in schema-preserving-release.mjs schema-preserving-release.sh; do
    mode=644
    [[ "$file" != *.sh ]] || mode=755
    [[ -f "$sp_bundle/$file" && ! -L "$sp_bundle/$file" &&
      "$(stat -c '%u:%g:%a:%h' "$sp_bundle/$file")" == "0:0:$mode:1" ]] ||
      die "operator bundle file metadata differs"
    manifest+="$(sp_sha "$sp_bundle/$file")  $file"$'\n'
  done
  [[ "$(printf '%s' "$manifest" | sha256sum | awk '{print $1}')" == "$expected" ]] ||
    die "operator bundle checksum differs"
  # internal image and dashboard recovery inherit the already held exact root release lock
  if [[ $# == 1 && ( "$1" == --inherited-family-lock || "$1" == --inherited-dashboard-lock ) ]]; then
    require_adjustment_family_release_lock
    release_transaction_lock_fd=$WEATHER_RELEASE_LOCK_FD
  else
    [[ $# == 0 || ( $# == 1 && ( "$1" == --dashboard-override || "$1" == --committed-dashboard-context || "$1" == --system-recovery ) ) ]] ||
      die "unexpected release controller lock mode"
    acquire_release_transaction_lock
  fi
  # permit committed override observation without granting a core image switch
  if [[ ${1-} == --dashboard-override || ${1-} == --inherited-dashboard-lock ]] ||
    { [[ ${1-} == --committed-dashboard-context || ${1-} == --system-recovery ]] && dashboard_override_present; }; then
    require_dashboard_fixed_roots
  else
    require_no_dashboard_override
  fi
  [[ -d "$sp_root" && ! -L "$sp_root" && "$(stat -c '%u:%g:%a' "$sp_root")" == 0:0:700 ]] ||
    die "auxiliary release state root differs"
  local receipt="$sp_root/installations/$expected.json"
  # retain the immutable bootstrap receipt for the original installed bundle
  if [[ ! -e "$receipt" && ! -L "$receipt" ]]; then
    receipt="$sp_root/installation.json"
  fi
  sp_private "$receipt"
  [[ "$(sp_field "$receipt" bundleSha256)" == "$expected" ]] ||
    die "installed auxiliary bundle differs"
  sp_installation "$receipt"
}

# bind the effective boot recovery path to the installed auxiliary receipt
sp_installation() {
  local receipt=${1:-"$sp_root/installation.json"} dropin base property vector
  dropin=/etc/systemd/system/weather-compose.service.d/10-schema-preserving-release.conf
  base=/etc/systemd/system/weather-compose.service
  [[ -f "$dropin" && ! -L "$dropin" && "$(stat -c '%u:%g:%a:%h' "$dropin")" == 0:0:644:1 &&
    "$(sp_sha "$dropin")" == "$(sp_field "$receipt" dropInSha256)" &&
    -f "$base" && ! -L "$base" && "$(realpath -e "$base")" == "$base" &&
    "$(stat -c '%u:%g:%a:%h' "$base")" == 0:0:644:1 &&
    "$(sp_sha "$base")" == "$(sp_field "$receipt" baseUnitSha256)" ]] ||
    die "auxiliary recovery unit identity differs"
  # require exactly one effective command for each lifecycle entrypoint
  for property in ExecStart ExecReload ExecStop; do
    vector=$(systemctl show weather-compose.service --property="$property" --value | python3 -c '
import re,sys
matches=re.findall(r"\{ path=([^ ;]+) ; argv\[\]=([^;]+?) ;",sys.stdin.read().strip())
if len(matches)!=1: raise SystemExit(1)
print(matches[0][0]+"|"+matches[0][1].strip())')
    [[ "$vector" == "$(sp_field "$receipt" "${property,}")" ]] || die "effective recovery command differs: $property"
  done
  [[ "$(sp_field "$receipt" timeoutStartSec)" == 600 &&
    "$(systemctl show weather-compose.service --property=TimeoutStartUSec --value)" == 10min ]] ||
    die "auxiliary cold-start health allowance differs"
}
# retain the immutable schema owner and the honest base-control identity
sp_environment() {
  local release=$1 path key anchor_env
  path=$(release_env "$release")
  anchor_env=$(release_env "$sp_anchor")
  sp_private "$path"
  sp_private "$anchor_env"
  [[ "$(sp_sha "$anchor_env")" == "$sp_anchor_env_sha" ]] || die "schema owner environment differs"
  validate_release_env "$path" "$release"
  require_control_plane_compatibility "$path"
  # permit only application image and label changes from the anchored runtime
  for key in POSTGRES_IMAGE CLOUDFLARED_IMAGE WEATHER_DATABASE_NAME WEATHER_POSTGRES_DIR \
    WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH \
    WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH \
    WEATHER_CONTROL_PLANE_SHA256 WEATHER_CONTROL_PLANE_VERSION; do
    [[ "$(env_value "$path" "$key")" == "$(env_value "$anchor_env" "$key")" ]] ||
      die "schema-preserving environment changed $key"
  done
  printf '%s\n' "$path"
}

# verify the retained generation metadata as part of each versioned stage receipt
sp_runtime_stage_metadata() {
  local receipt=$1 directory metadata publication
  directory=$(dirname "$receipt")
  metadata="$directory/runtime-generation.json"
  publication="$directory/publication.json"
  # legacy stages lack the generation sidecar
  if [[ "$(sp_field "$receipt" contractVersion)" == weather-schema-preserving-stage/v1 ]]; then
    [[ ! -e "$metadata" && ! -L "$metadata" ]] || die "legacy stage has unexpected runtime metadata"
    return
  fi
  sp_private "$metadata"
  sp_private "$publication"
  [[ "$(sp_sha "$metadata")" == "$(sp_field "$receipt" runtimeMetadataSha256)" &&
    "$(sp_sha "$publication")" == "$(sp_field "$receipt" publicationSha256)" ]] ||
    die "retained runtime generation metadata differs"
}

# validate the committed application ancestry without relabeling the database
sp_source() {
  local current environment receipt depth=0 cursor parent receipt_sha
  current=$(read_release_state "$state_dir/current-release")
  [[ "$(read_release_state "$state_dir/schema-release")" == "$sp_anchor" ]] || die "schema owner changed"
  environment=$(sp_environment "$current")
  [[ -L "$state_dir/active.env" && "$(realpath -e "$state_dir/active.env")" == "$environment" ]] ||
    die "active application environment differs from the current marker"
  cursor=$current
  # require every later environment to descend from a staged immutable receipt
  while [[ "$cursor" != "$sp_anchor" ]]; do
    ((depth < 64)) || die "application release ancestry is cyclic or too deep"
    receipt="$sp_root/$cursor/ready.json"
    sp_private "$receipt"
    # preserve schema rejection even inside a caller's command substitution
    node "$sp_module" validate-stage "$receipt" "$cursor" >/dev/null || die "application release receipt is invalid"
    sp_runtime_stage_metadata "$receipt"
    [[ "$(sp_field "$receipt" targetRelease)" == "$cursor" &&
      "$(sp_field "$receipt" targetEnvironmentSha256)" == "$(sp_sha "$(release_env "$cursor")")" ]] ||
      die "application release receipt differs"
    parent=$(sp_field "$receipt" sourceRelease)
    validate_release "$parent"
    [[ "$parent" != "$cursor" ]] || die "application release ancestry is cyclic"
    # independently rebind each committed native family descendant to its original source
    if [[ "$(sp_field "$receipt" contractVersion)" == weather-schema-preserving-stage/v2 ]]; then
      local publication="$sp_root/$cursor/publication.json"
      case "$(sp_field "$publication" contractVersion)" in
        weather-schema-preserving-family-publication/v1|weather-schema-preserving-native-descendant-publication/v1)
          sp_native_descendant_source_binding "$publication" "$parent"
          ;;
      esac
    fi
    receipt_sha=$(sp_field "$receipt" sourceEnvironmentSha256)
    [[ "$receipt_sha" == "$(sp_sha "$(sp_environment "$parent")")" ]] || die "application source receipt differs"
    cursor=$parent
    depth=$((depth + 1))
  done
  printf '%s\n' "$environment"
}

# recognize only the complete reviewed ledger and four deferred historical checks
sp_database() {
  local environment=$1 database result definition old new
  database=$(env_value "$environment" WEATHER_DATABASE_NAME)
  result=$(WEATHER_ENV_FILE=$environment compose exec -T postgres \
    psql -X -qAt --set=ON_ERROR_STOP=1 --username postgres --dbname "$database" <<'SQL'
BEGIN READ ONLY;
SET LOCAL lock_timeout='2s';
SET LOCAL statement_timeout='15s';
WITH ledger AS (
 SELECT count(*) AS total,
  encode(sha256(convert_to(string_agg(name||':'||checksum,E'\n' ORDER BY name)||E'\n','UTF8')),'hex') AS digest
 FROM public.schema_migrations
), expected(relation, name) AS (VALUES
 ('public.weather_records','weather_records_adjustment_revision_receipt_check'),
 ('public.forecast_anchor_records','forecast_anchor_records_adjustment_revision_receipt_check'),
 ('public.rain_adjustment_runs','rain_adjustment_runs_adjustment_revision_receipt_check'),
 ('public.ecmwf_temperature_canary_runs','ecmwf_temperature_canary_runs_adjustment_revision_receipt_check')
)
SELECT total=23 AND digest='1cfd6251eeb66d443e36d661299c9e49e73465fab55e5dab2cfa4ee86dd36ad2'
 AND NOT EXISTS (SELECT 1 FROM expected LEFT JOIN pg_constraint constraint_row
  ON constraint_row.conrelid=to_regclass(expected.relation) AND constraint_row.conname=expected.name
  WHERE constraint_row.oid IS NULL OR constraint_row.contype<>'c' OR constraint_row.convalidated
   OR pg_get_expr(constraint_row.conbin,constraint_row.conrelid)<>'weather_adjustment_revision_receipt_valid_v1(adjustment_revision_receipt)')
FROM ledger;
COMMIT;
SQL
  )
  [[ "$result" == t ]] || die "schema-preserving database envelope differs"
  old=e637542cb2e742fe8f22f60313a11f00adbc1d140a811ee718524849fcece12a
  new=7e48acb14710a625cdcbb796bf1bf49ab4e00bf1aeb9bb763ac86410e08663e0
  definition=$(declare -f verify_forecast_product_read_index)
  [[ "$(grep -o "$old" <<<"$definition" | wc -l)" == 1 ]] || die "installed index verifier differs"
  # adapt one checksum only after the complete alternate history is recognized
  (
    eval "${definition//$old/$new}"
    verify_runtime_database_acl "$environment" "$database"
  )
}

# capture immutable model metadata and unaffected infrastructure identities
sp_snapshot() {
  local environment=$1 output=$2 container service
  local temporary
  temporary=$(mktemp "$sp_root/.snapshot.XXXXXX")
  # record only the retained infrastructure configuration, not volatile health clocks
  for service in postgres cloudflared; do
    container=$(WEATHER_ENV_FILE=$environment compose ps -a -q "$service")
    [[ "$container" =~ ^[a-f0-9]{64}$ ]] || die "missing retained infrastructure: $service"
    docker inspect --format '{{.Id}}|{{.Image}}|{{.Config.Image}}|{{.HostConfig.Memory}}|{{.HostConfig.MemorySwap}}|{{.HostConfig.MemoryReservation}}|{{.HostConfig.NanoCpus}}|{{.HostConfig.CpuPeriod}}|{{.HostConfig.CpuQuota}}' \
      "$container" >>"$temporary"
  done
  node --input-type=module - "$temporary" "$output" "$state_dir" <<'JS'
import {createHash} from 'node:crypto';
import {readFileSync,lstatSync,writeFileSync} from 'node:fs';
const [temporary,output,state]=process.argv.slice(2);
const paths=[`${state}/schema-release`,`${state}/adjustment-candidate-catalog.json`,`${state}/adjustment-capture-epoch.json`,`${state}/adjustment-release-state/current.json`,'/var/lib/weather/xweather/forecast-adjustment-settings.json'];
// retain bytes and inode identity without copying sensitive state
const files=paths.map(path=>{const details=lstatSync(path);if(!details.isFile()||details.isSymbolicLink()||details.nlink!==1)throw new Error('protected state metadata differs');return {path,inode:`${details.dev}:${details.ino}`,mode:details.mode,owner:`${details.uid}:${details.gid}`,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')};});
writeFileSync(output,JSON.stringify({infrastructure:readFileSync(temporary,'utf8'),files})+'\n',{mode:0o600,flag:'wx'});
JS
  rm -- "$temporary"
}

# compare a fresh read-only snapshot without granting deletion credit
sp_unchanged() {
  local environment=$1 expected=$2 actual
  actual=$(mktemp "$sp_root/.compare.XXXXXX")
  rm -- "$actual"
  sp_snapshot "$environment" "$actual"
  cmp -s "$expected" "$actual" || { rm -- "$actual"; die "protected infrastructure or model metadata changed"; }
  rm -- "$actual"
}

# enforce real free space and retain the existing platform/resource gate
sp_capacity() {
  local blocks bytes inodes total
  require_image_pull_capacity_floor
  read -r blocks bytes inodes total < <(stat -f -c '%a %S %d %c' /var/lib/weather)
  ((blocks * bytes >= 8589934592 && total > 0 && inodes * 100 / total >= 10)) ||
    die "schema-preserving release capacity floor failed"
  # measure application-release pressure without a database-clone reservation
  node --input-type=module - <<'JS'
import {readFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {availableParallelism,loadavg} from 'node:os';
const architecture=execFileSync('uname',['-m'],{encoding:'utf8'}).trim();
const dockerArchitecture=execFileSync('docker',['info','--format','{{.Architecture}}'],{encoding:'utf8'}).trim();
const processors=availableParallelism();
const available=Number(readFileSync('/proc/meminfo','utf8').match(/^MemAvailable:\s+(\d+) kB$/m)?.[1])*1024;
const swapped=()=>readFileSync('/proc/vmstat','utf8').split('\n').filter(line=>/^pswp(?:in|out) /.test(line)).reduce((sum,line)=>sum+Number(line.split(/\s+/)[1]),0);
const first=swapped();
const start=performance.now();
await new Promise(resolve=>setTimeout(resolve,2000));
const swapRate=(swapped()-first)*Number(execFileSync('getconf',['PAGESIZE'],{encoding:'utf8'}).trim())/((performance.now()-start)/1000);
if(architecture!=='aarch64'||!['aarch64','arm64'].includes(dockerArchitecture)||processors<4||loadavg()[2]>processors*0.5||!Number.isSafeInteger(available)||available<1792*1024*1024||swapRate>1024*1024)throw new Error('application release resource pressure gate failed');
console.log(JSON.stringify({architecture,processors,load15:loadavg()[2],availableMemoryBytes:available,swapBytesPerSecond:Math.ceil(swapRate)}));
JS
}

# require exact cached ARM64 configuration identities
sp_image() {
  local image=$1 expected=$2 actual
  actual=$(docker image inspect --format '{{.Id}}|{{.Os}}/{{.Architecture}}' "$image")
  [[ "$actual" == "$expected|linux/arm64" ]] || die "published application image identity differs"
}

# select the original release generation using only an immutable staged receipt and image identities
sp_runtime_generation() {
  local environment=$1 release generation runtime_metadata stage publication server web server_id web_id server_marker web_marker
  release=$(env_value "$environment" WEATHER_RELEASE)
  validate_release "$release"
  stage="$sp_root/$release/stage.json"
  publication="$sp_root/$release/publication.json"
  runtime_metadata="$sp_root/$release/runtime-generation.json"
  server=$(env_value "$environment" WEATHER_SERVER_IMAGE)
  web=$(env_value "$environment" WEATHER_WEB_IMAGE)
  # the old Node generation has no sidecar; refuse a native label without explicit retained authority
  if [[ -e "$runtime_metadata" || -L "$runtime_metadata" ]]; then
    sp_private "$runtime_metadata"
    sp_private "$publication"
    if [[ -e "$stage" || -L "$stage" ]]; then
      sp_private "$stage"
      node "$sp_module" validate-stage "$stage" "$release" >/dev/null || die "runtime stage receipt is invalid"
      [[ "$(sp_field "$stage" contractVersion)" == weather-schema-preserving-stage/v2 &&
        "$(sp_sha "$runtime_metadata")" == "$(sp_field "$stage" runtimeMetadataSha256)" &&
        "$(sp_sha "$publication")" == "$(sp_field "$stage" publicationSha256)" ]] ||
        die "runtime generation receipt differs from stage"
    else
      # only an in-progress exact private stage may select the pre-published runtime metadata
      [[ "$environment" == "$sp_root/$release/environment.env" ]] || die "runtime generation stage is unavailable"
    fi
    server_id=$(docker image inspect --format '{{.Id}}' "$server") || die "runtime server image is unavailable"
    web_id=$(docker image inspect --format '{{.Id}}' "$web") || die "runtime web image is unavailable"
    generation=$(node "$sp_module" validate-runtime-generation "$runtime_metadata" "$release" "$(sp_sha "$publication")" \
      "$server" "$server_id" "$web" "$web_id") || die "runtime generation metadata differs"
  else
    # old source and rollback receipts remain exactly Node-v1
    if [[ -e "$stage" || -L "$stage" ]]; then
      sp_private "$stage"
      [[ "$(sp_field "$stage" contractVersion)" == weather-schema-preserving-stage/v1 ]] ||
        die "versioned runtime stage requires immutable metadata"
    fi
    generation=node-v1
  fi
  server_marker=$(docker image inspect --format '{{index .Config.Labels "org.weather.runtime.generation"}}' "$server") || die "runtime server marker is unavailable"
  web_marker=$(docker image inspect --format '{{index .Config.Labels "org.weather.runtime.generation"}}' "$web") || die "runtime web marker is unavailable"
  node "$sp_module" validate-runtime-markers "$generation" "$server_marker" "$web_marker" ||
    die "runtime image markers differ from retained generation"
}

# generate only the closed native service overlay in the operator's private temporary namespace
sp_compose() (
  local environment=$1 generation override
  shift
  generation=$(sp_runtime_generation "$environment") || exit 1
  if [[ "$generation" == node-v1 ]]; then
    WEATHER_ENV_FILE=$environment compose "$@"
    exit
  fi
  [[ "$generation" == rust-v1 ]] || die "runtime generation is invalid"
  override=$(mktemp "$sp_root/.runtime-compose.XXXXXX.json")
  trap 'rm -f -- "$override"' EXIT
  chmod 0600 "$override"
  node "$sp_module" runtime-compose-override "$generation" >"$override" ||
    die "native runtime Compose generation failed"
  docker compose --project-name weather --env-file "$environment" -f "$compose_file" -f "$override" "$@"
)

# run the candidate's readiness checker through an explicitly read-only API transaction
sp_readiness() {
  local environment=$1 image generation
  image=$(env_value "$environment" WEATHER_SERVER_IMAGE)
  generation=$(sp_runtime_generation "$environment") || die "runtime generation unavailable"
  # compose 2.26 run lacks --pull; require its immutable image already cached
  [[ "$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$image")" == linux/arm64 ]] ||
    die "readiness image is not cached for ARM64"
  # the Rust image exposes the same fixed read-only migration readiness as its own executable
  if [[ "$generation" == rust-v1 ]]; then
    sp_compose "$environment" run --rm -T --no-deps --entrypoint /opt/weather/bin/weather-api api --readiness
    return
  fi
  WEATHER_ENV_FILE=$environment compose run --rm -T --no-deps --entrypoint node api \
    --input-type=module - <<'JS'
import {createDatabasePool,loadDatabaseConfiguration,verifyMigrationReadiness} from '@weather/database';
const pool=createDatabasePool(await loadDatabaseConfiguration());
const client=await pool.connect();
try {await client.query("BEGIN READ ONLY");await client.query("SET LOCAL statement_timeout='15s'");const result=await verifyMigrationReadiness(client,'/opt/weather/packages/database/migrations');if(result.version!=='0023_forecast_product_read_index.sql')throw new Error('candidate schema version differs');await client.query('ROLLBACK');console.log('read-only migration readiness passed');}
finally {client.release();await pool.end();}
JS
}

# create immutable private metadata before any active application switch
sp_publish() {
  node --max-old-space-size=64 "$sp_module" write-json "$1"
}

# locate the retained installation authority for one exact bundle
sp_installation_receipt() {
  local bundle=$1 receipt
  [[ "$bundle" =~ ^[a-f0-9]{64}$ ]] || die "invalid installation bundle"
  receipt="$sp_root/installations/$bundle.json"
  # the first installed predecessor retains the original receipt
  if [[ ! -e "$receipt" && ! -L "$receipt" ]]; then receipt="$sp_root/installation.json"; fi
  sp_private "$receipt"
  [[ "$(sp_field "$receipt" bundleSha256)" == "$bundle" ]] || die "installation bundle differs"
  printf '%s\n' "$receipt"
}

# prove the exact installed finite bootstrap against the retained Node predecessor
sp_prove_native_bootstrap() {
  local approval=$1 approval_sha=$2 coordinates transition source_release source_commit source_sha canonical_approval
  local control old_bundle old_receipt_sha old_dropin target_release target_commit target_bundle source
  local old_receipt target_receipt old_path member mode manifest directory snapshot proof
  sp_private "$approval"
  [[ "$approval_sha" =~ ^[a-f0-9]{64}$ && "$(sp_sha "$approval")" == "$approval_sha" ]] ||
    die "bootstrap approval checksum differs"
  coordinates=$(node "$sp_module" native-bootstrap-coordinates "$approval") || die "bootstrap approval is invalid"
  IFS=$'\t' read -r transition source_release source_commit source_sha control old_bundle old_receipt_sha old_dropin \
    target_release target_commit target_bundle canonical_approval <<<"$coordinates"
  [[ "$target_bundle" == "${sp_bundle##*/}" && "$target_bundle" != "$old_bundle" && "$control" == "$sp_control_sha" ]] ||
    die "approved bootstrap does not name this installed operator"
  [[ ! -e "$sp_journal" && ! -L "$sp_journal" ]] || die "recover incomplete application transaction first"
  source=$(sp_source) || die "bootstrap source unavailable"
  [[ "$(env_value "$source" WEATHER_RELEASE)" == "$source_release" && "$(sp_sha "$source")" == "$source_sha" &&
    "$(sp_runtime_generation "$source")" == node-v1 ]] || die "bootstrap predecessor environment differs"
  [[ "$(docker image inspect --format '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$(env_value "$source" WEATHER_WEB_IMAGE)")" == "$source_commit" ]] ||
    die "bootstrap predecessor revision differs"
  old_receipt=$(sp_installation_receipt "$old_bundle") || die "predecessor installation receipt unavailable"
  target_receipt=$(sp_installation_receipt "$target_bundle") || die "target installation receipt unavailable"
  [[ "$(sp_sha "$old_receipt")" == "$old_receipt_sha" && "$(sp_field "$old_receipt" dropInSha256)" == "$old_dropin" &&
    "$(sp_field "$old_receipt" controlPlaneSha256)" == "$control" ]] || die "bootstrap predecessor receipt differs"
  old_path="$(dirname "$sp_bundle")/$old_bundle"
  [[ -d "$old_path" && ! -L "$old_path" && "$(stat -c '%u:%g:%a' "$old_path")" == 0:0:755 ]] ||
    die "retained predecessor bundle unavailable"
  manifest=
  # bind both retained predecessor helper bytes without sourcing either
  for member in schema-preserving-release.mjs schema-preserving-release.sh; do
    mode=644
    [[ "$member" != *.sh ]] || mode=755
    [[ -f "$old_path/$member" && ! -L "$old_path/$member" &&
      "$(stat -c '%u:%g:%a:%h' "$old_path/$member")" == "0:0:$mode:1" ]] || die "retained predecessor bundle member differs"
    manifest+="$(sp_sha "$old_path/$member")  $member"$'\n'
  done
  [[ "$(printf '%s' "$manifest" | sha256sum | awk '{print $1}')" == "$old_bundle" ]] ||
    die "retained predecessor bundle checksum differs"
  sp_installation "$target_receipt"
  sp_database "$source"
  sp_health "$source"
  directory="$sp_root/native-bootstrap-$transition"
  if [[ ! -e "$directory" && ! -L "$directory" ]]; then
    install -d -m 0700 -o 0 -g 0 "$directory"
  fi
  [[ -d "$directory" && ! -L "$directory" && "$(stat -c '%u:%g:%a' "$directory")" == 0:0:700 ]] ||
    die "bootstrap proof directory differs"
  snapshot="$directory/protected.json"
  proof="$directory/proof.json"
  # retain the original protected snapshot across retry
  if [[ ! -e "$snapshot" && ! -L "$snapshot" ]]; then sp_snapshot "$source" "$snapshot"; fi
  sp_private "$snapshot"
  sp_unchanged "$source" "$snapshot"
  # prove the newly installed system-recover entrypoint before any Rust image staging
  sp_system_recover
  [[ "$(sp_source)" == "$source" ]] || die "bootstrap recovery changed predecessor"
  sp_unchanged "$source" "$snapshot"
  sp_installation "$target_receipt"
  # publish or compare the exact proof; never replace retained evidence
  node --input-type=module - "$transition" "$canonical_approval" "$source_release" "$source_commit" "$source_sha" \
    "$old_bundle" "$old_receipt_sha" "$old_dropin" "$control" "$target_release" "$target_commit" \
    "$target_bundle" "$(sp_sha "$target_receipt")" "$(sp_sha "$snapshot")" <<'JS' | {
const [transitionSha256,approvalSha256,sourceRelease,sourceCommit,sourceEnvironmentSha256,
 predecessorBundleSha256,predecessorInstallationReceiptSha256,predecessorDropInSha256,controlSha256,
 targetRelease,targetCommit,targetBundleSha256,targetInstallationReceiptSha256,protectedSnapshotSha256]=process.argv.slice(2);
console.log(JSON.stringify({contractVersion:'weather-predecessor-native-bootstrap-proof/v1',transitionSha256,approvalSha256,
 sourceRelease,sourceCommit,sourceEnvironmentSha256,predecessorBundleSha256,predecessorInstallationReceiptSha256,
 predecessorDropInSha256,controlSha256,targetRelease,targetCommit,targetBundleSha256,targetInstallationReceiptSha256,protectedSnapshotSha256}));
JS
    if [[ -e "$proof" || -L "$proof" ]]; then
      sp_private "$proof"
      node --input-type=module -e 'import assert from "node:assert/strict";import {readFileSync} from "node:fs";assert.deepEqual(JSON.parse(readFileSync(0,"utf8")),JSON.parse(readFileSync(process.argv[1],"utf8")));' "$proof"
    else
      sp_publish "$proof"
    fi
  }
  printf 'Native bootstrap %s recovered retained Node predecessor without staging images.\n' "$transition"
}

# verify the exact target host evidence package through the installed root gateway
sp_native_host_readiness() {
  /usr/local/sbin/weather-remote-ops --verify-native-package "$1" ||
    die "native host evidence package is unavailable"
}

# require installed recovery evidence before admitting a native publication
sp_native_stage_admission() {
  local publication=$1 source=$2 transition bundle receipt directory proof snapshot
  transition=$(sp_field "$publication" transitionSha256)
  bundle=${sp_bundle##*/}
  [[ "$transition" =~ ^[a-f0-9]{64}$ ]] || die "native transition digest differs"
  [[ "$(sp_field "$publication" sourceRelease)" == "$(env_value "$source" WEATHER_RELEASE)" &&
    "$(sp_runtime_generation "$source")" == node-v1 ]] || die "native publication predecessor differs"
  receipt=$(sp_installation_receipt "$bundle") || die "native installation receipt unavailable"
  directory="$sp_root/native-bootstrap-$transition"
  proof="$directory/proof.json"
  snapshot="$directory/protected.json"
  sp_private "$proof"
  sp_private "$snapshot"
  [[ "$(sp_sha "$snapshot")" == "$(sp_field "$proof" protectedSnapshotSha256)" &&
    "$(sp_sha "$source")" == "$(sp_field "$proof" sourceEnvironmentSha256)" ]] || die "native bootstrap retained source differs"
  node "$sp_module" validate-native-bootstrap-proof "$proof" "$publication" "$(sp_sha "$receipt")" "$bundle" ||
    die "native bootstrap recovery proof is unavailable or differs"
  sp_unchanged "$source" "$snapshot"
  # require the generation-aware host application gateway for this exact target commit
  sp_native_host_readiness "$(sp_field "$publication" targetCommit)"
}

# bind the descendant document to the immutable publication of its recorded native ancestor
sp_native_descendant_source_binding() {
  local publication=$1 parent=$2 parent_publication="$sp_root/$2/publication.json"
  sp_private "$parent_publication"
  [[ "$(sp_field "$publication" sourceRelease)" == "$parent" &&
    "$(sp_field "$publication" sourceCommit)" == "$(sp_field "$parent_publication" targetCommit)" &&
    "$(sp_field "$publication" sourcePublicationSha256)" == "$(sp_sha "$parent_publication")" ]] ||
    die "native descendant publication source differs"
}

# require an exact committed native source before preparing any independently reviewed descendant
sp_native_descendant_stage_admission() {
  local publication=$1 source=$2 parent
  parent=$(env_value "$source" WEATHER_RELEASE)
  [[ "$(sp_runtime_generation "$source")" == rust-v1 ]] || die "native descendant source generation differs"
  sp_native_descendant_source_binding "$publication" "$parent"
  # the installed recovery and host evidence commands must already support the exact target
  sp_native_host_readiness "$(sp_field "$publication" targetCommit)"
}

# retain the original independently paired native family preparation boundary
sp_native_family_stage_admission() {
  sp_native_descendant_stage_admission "$@"
}

# bind two independently staged native family siblings to their exact common source and action
sp_native_family_pair() {
  local first=$1 second=$2 left right key
  [[ "$first" != "$second" ]] || return 1
  left="$sp_root/$first/publication.json"
  right="$sp_root/$second/publication.json"
  sp_target "$first" >/dev/null
  sp_target "$second" >/dev/null
  [[ "$(sp_field "$left" contractVersion)" == weather-schema-preserving-family-publication/v1 &&
    "$(sp_field "$right" contractVersion)" == weather-schema-preserving-family-publication/v1 &&
    "$(sp_field "$left" pairedRelease)" == "$second" &&
    "$(sp_field "$right" pairedRelease)" == "$first" &&
    "$(sp_field "$left" familyRole)" != "$(sp_field "$right" familyRole)" ]] || return 1
  # require the exact same original root and promotion action rather than an unrelated sibling
  for key in sourceRelease sourceCommit sourcePublicationSha256 family familyActionSha256; do
    [[ "$(sp_field "$left" "$key")" == "$(sp_field "$right" "$key")" ]] || return 1
  done
}

# create generation metadata bound to exact immutable native publication and pulled images
sp_native_runtime_metadata() {
  local publication=$1 directory=$2
  node --input-type=module - "$(sp_field "$publication" targetRelease)" "$(sp_sha "$publication")" \
    "$(sp_field "$publication" materialManifestSha256)" "$(sp_field "$publication" serverImage)" \
    "$(sp_field "$publication" serverImageId)" "$(sp_field "$publication" webImage)" "$(sp_field "$publication" webImageId)" <<'JS' |
const [release,publicationSha256,materialManifestSha256,serverImage,serverImageId,webImage,webImageId]=process.argv.slice(2);
console.log(JSON.stringify({contractVersion:'weather-runtime-generation/v1',generation:'rust-v1',
 release,publicationSha256,materialManifestSha256,serverImage,serverImageId,webImage,webImageId}));
JS
    sp_publish "$directory/runtime-generation.json"
}

# stage only application images against the unchanged live database
sp_stage() {
  local release=$1 publication=$2 expected_sha=$3 family_pair=${4-} source source_release directory target temporary image version runtime_hash
  validate_release "$release"
  sp_private "$publication"
  [[ "$(sp_sha "$publication")" == "$expected_sha" ]] || die "publication receipt checksum differs"
  # reject invalid publication before any staging side effect
  node "$sp_module" validate-publication "$publication" "$release" >/dev/null || die "publication receipt is invalid"
  [[ "$(sp_field "$publication" operatorSha256)" == "$(sp_sha "$sp_bundle/schema-preserving-release.sh")" &&
    "$(sp_field "$publication" validatorSha256)" == "$(sp_sha "$sp_module")" ]] || die "publication operator identity differs"
  [[ ! -e "$sp_journal" && ! -L "$sp_journal" ]] || die "recover the unfinished application transaction first"
  source=$(sp_source)
  source_release=$(env_value "$source" WEATHER_RELEASE)
  version=$(sp_field "$publication" contractVersion)
  # keep legacy publication and native admission separate before any staging mutation
  if [[ "$version" == weather-schema-preserving-publication/v3 ]]; then
    sp_native_stage_admission "$publication" "$source"
  elif [[ "$version" == weather-schema-preserving-family-publication/v1 ]]; then
    # the whole exact target/compensation pair must pass its capacity gate before either pull
    [[ "$family_pair" == family-pair ]] || die "native family stages require paired preparation"
    sp_native_family_stage_admission "$publication" "$source"
  elif [[ "$version" == weather-schema-preserving-native-descendant-publication/v1 ]]; then
    sp_native_descendant_stage_admission "$publication" "$source"
  else
    # retained legacy Node releases cannot stage application images from a committed native source
    [[ ! -e "$sp_root/$source_release/runtime-generation.json" &&
      ! -L "$sp_root/$source_release/runtime-generation.json" ]] ||
      die "native source requires an independently admitted native descendant publication"
  fi
  [[ "$source_release" != "$release" ]] || die "target application release is already active"
  directory="$sp_root/$release"
  [[ ! -e "$directory" && ! -L "$directory" ]] || die "application stage already exists"
  sp_health "$source"
  sp_database "$source"
  sp_capacity
  install -d -m 0700 -o 0 -g 0 "$directory"
  cp -- "$publication" "$directory/publication.json"
  chmod 600 "$directory/publication.json"
  sp_snapshot "$source" "$directory/protected.json"
  temporary="$directory/environment.env"
  write_release_env "$source" "$temporary" "$release" \
    "$(sp_field "$publication" serverImage)" "$(sp_field "$publication" webImage)" \
    "$(env_value "$source" POSTGRES_IMAGE)" "$(env_value "$source" CLOUDFLARED_IMAGE)"
  target=$(release_env "$release")
  [[ ! -e "$target" && ! -L "$target" ]] || die "target release environment already exists"
  # pull no infrastructure images and never start the migration service
  for image in "$(sp_field "$publication" serverImage)" "$(sp_field "$publication" webImage)"; do
    sp_capacity
    docker pull "$image"
  done
  sp_capacity
  sp_image "$(sp_field "$publication" serverImage)" "$(sp_field "$publication" serverImageId)"
  sp_image "$(sp_field "$publication" webImage)" "$(sp_field "$publication" webImageId)"
  [[ "$(docker image inspect --format '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$(sp_field "$publication" webImage)")" == \
    "$(sp_field "$publication" targetCommit)" ]] || die "web image revision differs"
  runtime_hash=
  # only an independently admitted native publication can create a generation sidecar
  if [[ "$version" == weather-schema-preserving-publication/v3 ||
    "$version" == weather-schema-preserving-family-publication/v1 ||
    "$version" == weather-schema-preserving-native-descendant-publication/v1 ]]; then
    [[ "$(docker image inspect --format '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$(sp_field "$publication" serverImage)")" == \
      "$(sp_field "$publication" targetCommit)" ]] || die "server image revision differs"
    sp_native_runtime_metadata "$directory/publication.json" "$directory"
    runtime_hash=$(sp_sha "$directory/runtime-generation.json")
  fi
  sp_compose "$temporary" config --quiet
  sp_readiness "$source"
  sp_readiness "$temporary"
  sp_database "$source"
  sp_unchanged "$source" "$directory/protected.json"
  [[ "$(sp_source)" == "$source" ]] || die "application source changed during stage"
  mv -- "$temporary" "$target"
  chmod 600 "$target"
  node --input-type=module - "$source_release" "$release" "$(sp_sha "$source")" "$(sp_sha "$target")" "$(sp_sha "$directory/publication.json")" "$(sp_sha "$directory/protected.json")" "$runtime_hash" <<'JS' | sp_publish "$directory/stage.json"
const [sourceRelease,targetRelease,sourceEnvironmentSha256,targetEnvironmentSha256,publicationSha256,protectedSha256,runtimeMetadataSha256]=process.argv.slice(2);
const receipt={contractVersion:runtimeMetadataSha256?'weather-schema-preserving-stage/v2':'weather-schema-preserving-stage/v1',sourceRelease,targetRelease,sourceEnvironmentSha256,targetEnvironmentSha256,publicationSha256,protectedSha256};
if(runtimeMetadataSha256)receipt.runtimeMetadataSha256=runtimeMetadataSha256;
console.log(JSON.stringify(receipt));
JS
  sync -f "$target" "$directory" "$releases_dir"
  printf 'Application release %s staged without a database copy or schema change.\n' "$release"
}

# invoke only the installed root gateway under the still-held release directory lock
sp_native_family_measure_capacity() {
  WEATHER_RELEASE_LOCK_FD=$release_transaction_lock_fd \
    /usr/local/sbin/weather-remote-ops --family-capacity-native-unlocked "$@"
}

# prepare two exact native family stages only after the original all-role capacity check
sp_stage_family_pair() (
  local target_publication=$1 target_sha=$2 compensation_publication=$3 compensation_sha=$4
  local source source_release target compensation action family capacity_root capacity temporary prior key publication release
  sp_private "$target_publication"
  sp_private "$compensation_publication"
  [[ "$(sp_sha "$target_publication")" == "$target_sha" &&
    "$(sp_sha "$compensation_publication")" == "$compensation_sha" ]] ||
    die "native family publication checksum differs"
  target=$(sp_field "$target_publication" targetRelease)
  compensation=$(sp_field "$compensation_publication" targetRelease)
  validate_release "$target"
  validate_release "$compensation"
  node "$sp_module" validate-publication "$target_publication" "$target" >/dev/null
  node "$sp_module" validate-publication "$compensation_publication" "$compensation" >/dev/null
  [[ "$(sp_field "$target_publication" contractVersion)" == weather-schema-preserving-family-publication/v1 &&
    "$(sp_field "$compensation_publication" contractVersion)" == weather-schema-preserving-family-publication/v1 &&
    "$(sp_field "$target_publication" familyRole)" == target &&
    "$(sp_field "$compensation_publication" familyRole)" == compensation &&
    "$(sp_field "$target_publication" pairedRelease)" == "$compensation" &&
    "$(sp_field "$compensation_publication" pairedRelease)" == "$target" ]] ||
    die "native family publication pair differs"
  # require both stage documents to bind the exact same source and transaction
  for key in sourceRelease sourceCommit sourcePublicationSha256 family familyActionSha256; do
    [[ "$(sp_field "$target_publication" "$key")" == "$(sp_field "$compensation_publication" "$key")" ]] ||
      die "native family publication pair identity differs"
  done
  source=$(sp_source)
  source_release=$(env_value "$source" WEATHER_RELEASE)
  sp_native_family_stage_admission "$target_publication" "$source"
  sp_native_family_stage_admission "$compensation_publication" "$source"
  action=$(sp_field "$target_publication" familyActionSha256)
  family=$(sp_field "$target_publication" family)
  # retain the original root-owned capacity receipt and exact six application image roles
  capacity_root="$state_dir/adjustment-release-state"
  ensure_adjustment_family_private_state_root "$capacity_root"
  capacity="$capacity_root/capacity-sha256-$action.json"
  temporary=$(mktemp "$capacity_root/.native-capacity.XXXXXXXX")
  trap 'rm -f -- "$temporary"' EXIT
  chmod 600 "$temporary"
  if ! sp_native_family_measure_capacity "$action" "$family" "$source_release" \
    "$(env_value "$source" WEATHER_SERVER_IMAGE)" "$(env_value "$source" WEATHER_WEB_IMAGE)" \
    "$(sp_field "$target_publication" serverImage)" "$(sp_field "$target_publication" webImage)" \
    "$(sp_field "$compensation_publication" serverImage)" "$(sp_field "$compensation_publication" webImage)" >"$temporary"; then
    cat "$temporary" >&2
    die "native family release capacity is blocked"
  fi
  # preserve prior measured receipts before replacing the source's latest capacity pointer
  if [[ -e "$capacity" || -L "$capacity" ]]; then
    sp_private "$capacity"
    prior="$capacity_root/capacity-history-sha256-$action-$(sp_sha "$capacity").json"
    if [[ ! -e "$prior" && ! -L "$prior" ]]; then
      cp --no-clobber -- "$capacity" "$prior"
      chmod 600 "$prior"
      sync -f "$prior"
    else
      sp_private "$prior"
      cmp -s "$capacity" "$prior" || die "prior family capacity receipt differs"
    fi
  fi
  mv -T -- "$temporary" "$capacity"
  sync -f "$capacity" "$capacity_root"
  # reuse only an exact already committed stage after an interrupted paired preparation
  for publication in "$target_publication" "$compensation_publication"; do
    release=$(sp_field "$publication" targetRelease)
    if [[ -e "$sp_root/$release/stage.json" ]]; then
      sp_target "$release" >/dev/null
      [[ "$(sp_sha "$sp_root/$release/publication.json")" == "$(sp_sha "$publication")" ]] ||
        die "retained native family stage differs"
    else
      sp_stage "$release" "$publication" "$(sp_sha "$publication")" family-pair
    fi
  done
  sp_native_family_pair "$target" "$compensation" || die "native family pair is not staged"
)

# validate immutable staging and cached images for an application transaction
sp_target() {
  local release=$1 directory publication receipt source target
  validate_release "$release"
  directory="$sp_root/$release"
  publication="$directory/publication.json"
  receipt="$directory/stage.json"
  sp_private "$publication"
  sp_private "$receipt"
  # fail closed independently of the caller's errexit context
  node "$sp_module" validate-stage "$receipt" "$release" >/dev/null || die "staged application receipt is invalid"
  sp_runtime_stage_metadata "$receipt"
  # retain the same boundary for the published image proof
  node "$sp_module" validate-publication "$publication" "$release" >/dev/null || die "publication receipt is invalid"
  source=$(sp_environment "$(sp_field "$receipt" sourceRelease)")
  target=$(sp_environment "$release")
  # prove the same immutable source witness when reading a retained native family stage
  case "$(sp_field "$publication" contractVersion)" in
    weather-schema-preserving-family-publication/v1|weather-schema-preserving-native-descendant-publication/v1)
      sp_native_descendant_source_binding "$publication" "$(sp_field "$receipt" sourceRelease)"
      ;;
  esac
  [[ "$(sp_sha "$source")" == "$(sp_field "$receipt" sourceEnvironmentSha256)" &&
    "$(sp_sha "$target")" == "$(sp_field "$receipt" targetEnvironmentSha256)" &&
    "$(sp_sha "$publication")" == "$(sp_field "$receipt" publicationSha256)" &&
    "$(sp_sha "$directory/protected.json")" == "$(sp_field "$receipt" protectedSha256)" ]] ||
    die "staged application receipt differs"
  sp_image "$(env_value "$target" WEATHER_SERVER_IMAGE)" "$(sp_field "$publication" serverImageId)"
  sp_image "$(env_value "$target" WEATHER_WEB_IMAGE)" "$(sp_field "$publication" webImageId)"
  printf '%s\n' "$target"
}

# verify the selected running application services against their committed images
sp_health_services() {
  local environment=$1 service container image expected_id
  shift
  # require each healthy running application to use its committed digest
  for service in "$@"; do
    container=$(WEATHER_ENV_FILE=$environment compose ps -q "$service")
    [[ "$container" =~ ^[a-f0-9]{64}$ ]] || die "application container is missing: $service"
    image=$(env_value "$environment" WEATHER_SERVER_IMAGE)
    [[ "$service" != web ]] || image=$(env_value "$environment" WEATHER_WEB_IMAGE)
    expected_id=$(docker image inspect --format '{{.Id}}' "$image")
    [[ "$(docker inspect --format '{{.Config.Image}}|{{.Image}}|{{.State.Running}}|{{.State.Health.Status}}' "$container")" == \
      "$image|$expected_id|true|healthy" ]] || die "application image or health differs: $service"
  done
}

# verify the exact application trio and ordinary weather read boundary
sp_health() {
  local environment=$1 release generation
  generation=$(sp_runtime_generation "$environment") || die "runtime generation unavailable"
  release=$(env_value "$environment" WEATHER_RELEASE)
  sp_health_services "$environment" api worker web
  # preserve the exact rendered release and readable API checks without executing Node in a Rust image
  if [[ "$generation" == rust-v1 ]]; then
    sp_compose "$environment" exec -T web /opt/weather/bin/weather-web-health web-release "$release" ||
      die "native dashboard web health or readable API verification failed"
  else
    verify_dashboard_web_runtime "$environment" "$release" "$(env_value "$environment" WEATHER_WEB_IMAGE)"
  fi
}

# replace only the application trio while retaining live infrastructure containers
sp_switch() {
  local environment=$1
  sp_compose "$environment" up -d --no-deps --no-build --pull never --wait api worker
  sp_compose "$environment" up -d --no-deps --no-build --pull never --wait web
  sp_health "$environment"
}

# validate an exact interrupted family image switch without changing the current release marker
sp_native_family_interrupted_journal() {
  local source target stage publication snapshot current expected paired previous
  sp_private "$sp_journal"
  source=$(sp_field "$sp_journal" sourceRelease)
  target=$(sp_field "$sp_journal" targetRelease)
  stage=$(sp_field "$sp_journal" stageRelease)
  previous=$(sp_field "$sp_journal" previousRelease)
  validate_release "$source"
  validate_release "$target"
  validate_release "$previous"
  [[ "$stage" == "$target" ]] || die "family journal stage differs"
  sp_target "$stage" >/dev/null
  publication="$sp_root/$stage/publication.json"
  [[ "$(sp_field "$publication" contractVersion)" == weather-schema-preserving-family-publication/v1 ]] ||
    die "interrupted application journal is not a native family switch"
  expected=$(sp_field "$publication" sourceRelease)
  paired=$(sp_field "$publication" pairedRelease)
  sp_native_family_pair "$stage" "$paired" || die "interrupted family journal pair differs"
  [[ "$source" == "$expected" || "$source" == "$paired" ]] || die "interrupted family journal source differs"
  [[ "$(sp_sha "$sp_root/$stage/stage.json")" == "$(sp_field "$sp_journal" stageSha256)" ]] ||
    die "interrupted family journal stage identity differs"
  snapshot=$(sp_field "$sp_journal" protectedSnapshot)
  [[ "${snapshot%/*}" == "$sp_root/$stage" && "${snapshot##*/}" =~ ^rollback-protected-[0-9]+-[0-9]+\.json$ ]] ||
    die "interrupted family journal snapshot differs"
  sp_private "$snapshot"
  [[ "$(sp_sha "$snapshot")" == "$(sp_field "$sp_journal" protectedSha256)" ]] ||
    die "interrupted family journal snapshot identity differs"
  current=$(read_release_state "$state_dir/current-release")
  [[ "$current" == "$source" || "$current" == "$target" ]] ||
    die "interrupted family journal commit marker differs"
  # the marker remains the sole selected authority even when its active link update was interrupted
  sp_environment "$current"
}

# retain an interrupted family journal before executing only its verified compensation
sp_native_family_resume_compensation() {
  local compensation=$1 paired=$2 action=$3 catalog=$4 environment current source target previous snapshot actual archived
  environment=$(sp_native_family_interrupted_journal)
  current=$(env_value "$environment" WEATHER_RELEASE)
  source=$(sp_field "$sp_journal" sourceRelease)
  target=$(sp_field "$sp_journal" targetRelease)
  previous=$(sp_field "$sp_journal" previousRelease)
  [[ "$target" == "$paired" || "$target" == "$compensation" ]] ||
    die "interrupted family recovery target differs"
  [[ "$(sp_field "$sp_root/$target/publication.json" familyActionSha256)" == "$action" ]] ||
    die "interrupted family recovery action differs"
  snapshot=$(sp_field "$sp_journal" protectedSnapshot)
  actual="$sp_root/$target/family-compensation-protected-${EPOCHSECONDS}-${BASHPID}.json"
  sp_database "$environment"
  sp_snapshot "$environment" "$actual"
  # a separately verified catalog replacement is the only permitted difference from the failed switch
  node "$sp_module" validate-family-recovery-snapshot "$snapshot" "$actual" "$state_dir/adjustment-candidate-catalog.json" "$catalog" ||
    die "interrupted family recovery protected state differs"
  # restore the recorded commit point without switching any application image
  write_active_symlink "$current"
  if [[ "$current" == "$source" ]]; then
    write_private_state "$state_dir/previous-release" "$previous"
  fi
  [[ "$(sp_source)" == "$environment" ]] || die "interrupted family recovery marker differs"
  archived="$sp_root/$target/family-interrupted-$(sp_sha "$sp_journal").json"
  [[ ! -e "$archived" && ! -L "$archived" ]] || die "interrupted family journal archive already exists"
  mv -T --no-clobber -- "$sp_journal" "$archived"
  sp_private "$archived"
  [[ ! -e "$sp_journal" && ! -L "$sp_journal" ]] || die "interrupted family journal remains"
  sync -f "$sp_root/$target" "$sp_root" "$state_dir"
}

# restore the current-marker side of an interrupted journal
sp_reconcile() {
  local source_release target_release current chosen environment directory previous stage_release stage_source snapshot
  sp_private "$sp_journal"
  source_release=$(sp_field "$sp_journal" sourceRelease)
  target_release=$(sp_field "$sp_journal" targetRelease)
  previous=$(sp_field "$sp_journal" previousRelease)
  stage_release=$(sp_field "$sp_journal" stageRelease)
  validate_release "$source_release"
  validate_release "$target_release"
  validate_release "$stage_release"
  directory="$sp_root/$stage_release"
  [[ "$(sp_sha "$directory/stage.json")" == "$(sp_field "$sp_journal" stageSha256)" ]] || die "journal stage identity differs"
  sp_target "$stage_release" >/dev/null
  stage_source=$(sp_field "$directory/stage.json" sourceRelease)
  # preserve ordinary source/target recovery and admit only the pre-staged reciprocal family pair
  if [[ "$source_release:$target_release" != "$stage_source:$stage_release" &&
    "$source_release:$target_release" != "$stage_release:$stage_source" ]]; then
    [[ "$target_release" == "$stage_release" ]] &&
      sp_native_family_pair "$source_release" "$target_release" ||
      die "journal application pair differs"
  fi
  snapshot=$(sp_field "$sp_journal" protectedSnapshot)
  [[ "$snapshot" == "$directory/protected.json" ||
    ( "$snapshot" == "$directory/"* &&
      "${snapshot##*/}" =~ ^rollback-protected-[0-9]+-[0-9]+\.json$ &&
      "${snapshot%/*}" == "$directory" ) ]] || die "journal snapshot location differs"
  sp_private "$snapshot"
  [[ "$(sp_sha "$snapshot")" == "$(sp_field "$sp_journal" protectedSha256)" ]] || die "journal protected snapshot differs"
  current=$(read_release_state "$state_dir/current-release")
  # current-release remains the sole application commit point
  if [[ "$current" == "$target_release" ]]; then
    chosen=$target_release
  elif [[ "$current" == "$source_release" ]]; then
    chosen=$source_release
  else
    die "journal current release is unrelated"
  fi
  environment=$(sp_environment "$chosen")
  sp_database "$environment"
  sp_unchanged "$environment" "$snapshot"
  sp_switch "$environment"
  sp_unchanged "$environment" "$snapshot"
  write_active_symlink "$chosen"
  # restore the previous marker only on the uncommitted side
  if [[ "$chosen" == "$source_release" ]]; then
    validate_release "$previous"
    write_private_state "$state_dir/previous-release" "$previous"
  fi
  rm -- "$sp_journal"
  sync -f "$sp_root" "$state_dir"
}

# commit one health-checked application-only forward or rollback transaction
sp_transaction() (
  local release=$1 stage_release=$2 snapshot=$3 source target directory previous completed=false
  source=$(sp_source)
  target=$(sp_environment "$release")
  sp_target "$stage_release" >/dev/null
  directory="$sp_root/$stage_release"
  [[ ! -e "$sp_journal" && ! -L "$sp_journal" ]] || die "application journal requires recovery"
  sp_database "$source"
  sp_unchanged "$source" "$snapshot"
  sp_capacity
  previous=$(read_release_state "$state_dir/previous-release")
  node --input-type=module - "$(env_value "$source" WEATHER_RELEASE)" "$release" "$previous" "$(sp_sha "$directory/stage.json")" "$stage_release" "$snapshot" "$(sp_sha "$snapshot")" <<'JS' | sp_publish "$sp_journal"
const [sourceRelease,targetRelease,previousRelease,stageSha256,stageRelease,protectedSnapshot,protectedSha256]=process.argv.slice(2);
console.log(JSON.stringify({contractVersion:'weather-schema-preserving-journal/v1',sourceRelease,targetRelease,previousRelease,stageSha256,stageRelease,protectedSnapshot,protectedSha256}));
JS
  # recover failed image switches without changing database authority
  # shellcheck disable=SC2317,SC2329
  sp_activation_exit() {
    local status=$?
    local recovery_status=0
    trap - EXIT
    if [[ "$completed" != true ]]; then
      # keep recovery outside an OR-list so every nested failure retains errexit
      set +e
      (set -e; sp_reconcile)
      recovery_status=$?
      set -e
      if ((recovery_status != 0)); then
        printf 'error: application recovery requires operator attention\n' >&2
        exit 1
      fi
    fi
    exit "$status"
  }
  trap sp_activation_exit EXIT
  sp_switch "$target"
  sp_database "$target"
  sp_unchanged "$target" "$snapshot"
  # publish health evidence before the current marker commit point
  # retain an identical earlier health receipt after a recovered retry
  if [[ "$release" == "$stage_release" ]]; then
    if [[ -e "$directory/ready.json" || -L "$directory/ready.json" ]]; then
      sp_private "$directory/ready.json"
      cmp -s "$directory/stage.json" "$directory/ready.json" || die "application health receipt differs"
    else
      cp -- "$directory/stage.json" "$directory/ready.json"
    fi
    chmod 600 "$directory/ready.json"
    sync -f "$directory/ready.json" "$directory"
  fi
  record_release_success "$release" "$(env_value "$source" WEATHER_RELEASE)"
  completed=true
  rm -- "$sp_journal"
  sync -f "$sp_root" "$state_dir"
  trap - EXIT
  printf 'Application release %s is active; schema owner remains %s.\n' "$release" "$sp_anchor"
)

# activate only the exact source that passed staging
sp_activate() {
  local release=$1 source directory
  source=$(sp_source)
  sp_target "$release" >/dev/null
  directory="$sp_root/$release"
  # family stages can be activated only by the independently verified native family transaction
  [[ "$(sp_field "$directory/publication.json" contractVersion)" != weather-schema-preserving-family-publication/v1 ]] ||
    die "native family publication requires family release authority"
  [[ "$(env_value "$source" WEATHER_RELEASE)" == "$(sp_field "$directory/stage.json" sourceRelease)" ]] ||
    die "staged application source is no longer current"
  sp_transaction "$release" "$release" "$directory/protected.json"
}

# quarantine only an uncommitted stage so interrupted publication can be retried
sp_retire_stage() {
  local release=$1 current previous directory target retired file
  validate_release "$release"
  sp_source >/dev/null
  current=$(read_release_state "$state_dir/current-release")
  previous=$(read_release_state "$state_dir/previous-release")
  [[ "$release" != "$current" && "$release" != "$previous" && "$release" != "$sp_anchor" &&
    ! -e "$sp_journal" && ! -L "$sp_journal" ]] || die "stage belongs to a retained or active transaction"
  directory="$sp_root/$release"
  target=$(release_env "$release")
  [[ ! -e "$directory/ready.json" && ! -L "$directory/ready.json" ]] || die "committed application evidence cannot be retired"
  [[ -e "$directory" || -e "$target" ]] || die "no partial stage exists"
  # reject unknown objects rather than moving arbitrary files
  if [[ -e "$directory" || -L "$directory" ]]; then
    [[ -d "$directory" && ! -L "$directory" && "$(stat -c '%u:%g:%a' "$directory")" == 0:0:700 ]] || die "partial stage root differs"
    while IFS= read -r -d '' file; do
      case "${file##*/}" in
        publication.json|protected.json|environment.env|stage.json|runtime-generation.json) sp_private "$file" ;;
        *) die "partial stage contains an unexpected object" ;;
      esac
    done < <(find -P "$directory" -mindepth 1 -maxdepth 1 -print0)
  fi
  if [[ -e "$target" || -L "$target" ]]; then
    sp_environment "$release" >/dev/null
  fi
  # never follow or claim an existing quarantine directory
  if [[ -e "$sp_root/retired" || -L "$sp_root/retired" ]]; then
    [[ -d "$sp_root/retired" && ! -L "$sp_root/retired" &&
      "$(realpath -e "$sp_root/retired")" == "$sp_root/retired" &&
      "$(stat -c '%u:%g:%a' "$sp_root/retired")" == 0:0:700 ]] || die "stage quarantine root differs"
  else
    install -d -m 0700 -o 0 -g 0 "$sp_root/retired"
  fi
  retired="$sp_root/retired/$release-${EPOCHSECONDS}-${BASHPID}"
  install -d -m 0700 -o 0 -g 0 "$retired"
  # retain both namespaces as evidence without deleting an image or database file
  if [[ -d "$directory" ]]; then
    mv -- "$directory" "$retired/stage"
  fi
  if [[ -f "$target" ]]; then
    mv -- "$target" "$retired/environment.env"
  fi
  sync -f "$retired" "$sp_root" "$releases_dir"
  printf 'Uncommitted stage retained at %s; stage may now be retried.\n' "$retired"
}

# restore the retained previous application without changing schema or model state
sp_rollback() {
  local source current previous stage_release directory snapshot
  source=$(sp_source)
  current=$(env_value "$source" WEATHER_RELEASE)
  previous=$(read_release_state "$state_dir/previous-release")
  # a family publication must use its verified compensation rather than discard its active catalog
  if [[ "$current" != "$sp_anchor" && "$(sp_field "$sp_root/$current/publication.json" contractVersion)" == weather-schema-preserving-family-publication/v1 ]]; then
    die "native family release requires its exact compensation transaction"
  fi
  sp_environment "$previous" >/dev/null
  stage_release=$current
  # a prior rollback can leave the forward package as the retained previous image
  if [[ "$current" == "$sp_anchor" || ! -f "$sp_root/$current/stage.json" ||
    "$(sp_field "$sp_root/$current/stage.json" sourceRelease)" != "$previous" ]]; then
    stage_release=$previous
    [[ "$(sp_field "$sp_root/$previous/stage.json" sourceRelease)" == "$current" ]] ||
      die "retained application pair has no staged authority"
  fi
  sp_target "$stage_release" >/dev/null
  directory="$sp_root/$stage_release"
  snapshot="$directory/rollback-protected-${EPOCHSECONDS}-${BASHPID}.json"
  sp_database "$source"
  sp_snapshot "$source" "$snapshot"
  sp_transaction "$previous" "$stage_release" "$snapshot"
}

# identify both retained infrastructure containers before starting either one
sp_retained_infrastructure() {
  local environment=$1 service container image expected_id actual
  for service in postgres cloudflared; do
    container=$(WEATHER_ENV_FILE=$environment compose ps -a -q "$service")
    [[ "$container" =~ ^[a-f0-9]{64}$ ]] || die "retained infrastructure container is absent: $service"
    image=$(env_value "$environment" POSTGRES_IMAGE)
    [[ "$service" != cloudflared ]] || image=$(env_value "$environment" CLOUDFLARED_IMAGE)
    expected_id=$(docker image inspect --format '{{.Id}}' "$image")
    [[ "$expected_id" =~ ^sha256:[a-f0-9]{64}$ ]] || die "retained infrastructure image is absent"
    actual=$(docker inspect --format '{{.Id}}|{{.Config.Image}}|{{.Image}}|{{index .Config.Labels "com.docker.compose.project"}}|{{index .Config.Labels "com.docker.compose.service"}}' "$container")
    [[ "$actual" == "$container|$image|$expected_id|weather|$service" ]] || die "retained infrastructure image or project differs"
    printf '%s\n' "$actual"
  done
}

# start exact stopped containers rather than allowing Compose to create replacements
sp_start_infrastructure() {
  local environment=$1 retained descriptor container attempt healthy
  retained=$(sp_retained_infrastructure "$environment")
  # all identities have already passed before the first start
  while IFS= read -r descriptor; do
    container=${descriptor%%|*}
    docker start "$container" >/dev/null
  done <<<"$retained"
  # wait only for these two retained containers to regain their own health
  for ((attempt=0; attempt<100; attempt++)); do
    healthy=true
    while IFS= read -r descriptor; do
      container=${descriptor%%|*}
      if [[ "$(docker inspect --format '{{.State.Running}}|{{.State.Health.Status}}' "$container")" != true\|healthy ]]; then
        healthy=false
      fi
    done <<<"$retained"
    if [[ "$healthy" == true ]]; then
      [[ "$(sp_retained_infrastructure "$environment")" == "$retained" ]] || die "retained infrastructure identity changed during recovery"
      return
    fi
    sleep 2
  done
  die "retained infrastructure did not become healthy"
}

# converge after a host/service restart without migrating or applying ACL changes
# resolve a durable applying family mutation independently of the generic application journal
sp_native_family_boot_recovery() {
  local environment=$1 stage publication generation
  # leave Node predecessor recovery and ordinary release journals on their original path
  generation=$(sp_runtime_generation "$environment") || die "family boot recovery generation unavailable"
  [[ "$generation" == rust-v1 ]] || return 0
  if [[ -e "$sp_journal" || -L "$sp_journal" ]]; then
    sp_private "$sp_journal"
    stage=$(sp_field "$sp_journal" stageRelease)
    validate_release "$stage"
    publication="$sp_root/$stage/publication.json"
    sp_private "$publication"
    [[ "$(sp_field "$publication" contractVersion)" == weather-schema-preserving-family-publication/v1 ]] || return 0
  fi
  # the original lock remains held across the actual root-native recovery process
  WEATHER_RELEASE_LOCK_FD=$release_transaction_lock_fd \
    /usr/local/sbin/weather-remote-ops --family-recover-native-unlocked
}

sp_system_recover() {
  local current environment container service dashboard=false
  current=$(read_release_state "$state_dir/current-release")
  environment=$(sp_environment "$current")
  [[ "$(read_release_state "$state_dir/schema-release")" == "$sp_anchor" ]] || die "schema owner changed"
  # validate committed ancestry before starting a stopped service
  if [[ ! -e "$sp_journal" && ! -L "$sp_journal" ]]; then
    [[ "$(sp_source)" == "$environment" ]] || die "committed application ancestry differs"
  fi
  # reject conflicting application and dashboard journals before starting any container
  if dashboard_override_present; then
    [[ ! -e "$sp_journal" && ! -L "$sp_journal" && "$(sp_runtime_generation "$environment")" == rust-v1 ]] ||
      die "dashboard recovery requires the committed native core"
    dashboard=true
  fi
  # start only retained infrastructure that was stopped by the inherited ExecStop
  sp_start_infrastructure "$environment"
  # keep a dashboard override on the selected web image while starting the unchanged native core
  if [[ "$dashboard" == true ]]; then
    sp_database "$environment"
    sp_compose "$environment" up -d --no-deps --no-build --pull never --wait api worker
    sp_health_services "$environment" api worker
    WEATHER_RELEASE_LOCK_FD=$release_transaction_lock_fd \
      /usr/local/sbin/weather-remote-ops --dashboard-recover-native-unlocked
  else
    # a crash before the generic image journal is also recoverable from the family owner's durable record
    sp_native_family_boot_recovery "$environment"
    current=$(read_release_state "$state_dir/current-release")
    environment=$(sp_environment "$current")
    if [[ -e "$sp_journal" || -L "$sp_journal" ]]; then
      sp_reconcile
    else
      [[ "$(realpath -e "$state_dir/active.env")" == "$environment" ]] || die "active application environment differs"
      [[ "$(sp_source)" == "$environment" ]] || die "committed application ancestry differs"
      sp_database "$environment"
      sp_switch "$environment"
    fi
  fi
  # keep the service start successful only after all retained infrastructure is healthy
  for service in postgres cloudflared; do
    container=$(WEATHER_ENV_FILE=$environment compose ps -q "$service")
    [[ "$(docker inspect --format '{{.State.Running}}|{{.State.Health.Status}}' "$container")" == true\|healthy ]] ||
      die "retained infrastructure is unhealthy"
  done
}

# dispatch only the dedicated auxiliary lifecycle verbs
sp_main() {
  (($# >= 1)) || { printf 'usage: schema-preserving-release.sh prove-native-bootstrap APPROVAL_JSON SHA256 | stage RELEASE PUBLICATION_JSON SHA256 | activate RELEASE | recover | system-recover | status\n' >&2; return 2; }
  local action=$1
  shift
  # boot recovery can converge a committed web override or its interrupted transaction
  if [[ "$action" == recover || "$action" == system-recover ]]; then
    sp_load --system-recovery
  else
    sp_load
  fi
  case "$action" in
    prove-native-bootstrap)
      (($# == 2)) || die "native bootstrap requires reviewed approval and checksum"
      sp_prove_native_bootstrap "$@"
      ;;
    stage)
      (($# == 3)) || die "stage requires release, publication and checksum"
      sp_stage "$@"
      ;;
    stage-family-pair)
      (($# == 4)) || die "stage-family-pair requires target and compensation publications and checksums"
      sp_stage_family_pair "$@"
      ;;
    activate)
      (($# == 1)) || die "activate requires one staged release"
      sp_activate "$1"
      ;;
    retire-stage)
      (($# == 1)) || die "retire-stage requires one uncommitted release"
      sp_retire_stage "$1"
      ;;
    rollback)
      (($# == 0)) || die "rollback takes no caller-selected source"
      sp_rollback
      ;;
    recover|system-recover)
      (($# == 0)) || die "recovery takes no caller-selected source"
      sp_system_recover
      ;;
    status)
      (($# == 0)) || die "status takes no operands"
      sp_source
      ;;
    *) die "unsupported schema-preserving operation" ;;
  esac
}

# allow isolated shell tests without running a privileged operation
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  sp_main "$@"
fi
