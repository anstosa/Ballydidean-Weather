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
  acquire_release_transaction_lock
  require_no_dashboard_override
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
    [[ "$(sp_field "$receipt" targetRelease)" == "$cursor" &&
      "$(sp_field "$receipt" targetEnvironmentSha256)" == "$(sp_sha "$(release_env "$cursor")")" ]] ||
      die "application release receipt differs"
    parent=$(sp_field "$receipt" sourceRelease)
    validate_release "$parent"
    [[ "$parent" != "$cursor" ]] || die "application release ancestry is cyclic"
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

# run the candidate's readiness checker through an explicitly read-only API transaction
sp_readiness() {
  local environment=$1 image
  image=$(env_value "$environment" WEATHER_SERVER_IMAGE)
  # compose 2.26 run lacks --pull; require its immutable image already cached
  [[ "$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$image")" == linux/arm64 ]] ||
    die "readiness image is not cached for ARM64"
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

# stage only application images against the unchanged live database
sp_stage() {
  local release=$1 publication=$2 expected_sha=$3 source source_release directory target temporary image
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
  WEATHER_ENV_FILE=$temporary compose config --quiet
  sp_readiness "$source"
  sp_readiness "$temporary"
  sp_database "$source"
  sp_unchanged "$source" "$directory/protected.json"
  [[ "$(sp_source)" == "$source" ]] || die "application source changed during stage"
  mv -- "$temporary" "$target"
  chmod 600 "$target"
  node --input-type=module - "$source_release" "$release" "$(sp_sha "$source")" "$(sp_sha "$target")" "$(sp_sha "$directory/publication.json")" "$(sp_sha "$directory/protected.json")" <<'JS' | sp_publish "$directory/stage.json"
const [sourceRelease,targetRelease,sourceEnvironmentSha256,targetEnvironmentSha256,publicationSha256,protectedSha256]=process.argv.slice(2);
console.log(JSON.stringify({contractVersion:'weather-schema-preserving-stage/v1',sourceRelease,targetRelease,sourceEnvironmentSha256,targetEnvironmentSha256,publicationSha256,protectedSha256}));
JS
  sync -f "$target" "$directory" "$releases_dir"
  printf 'Application release %s staged without a database copy or schema change.\n' "$release"
}

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
  # retain the same boundary for the published image proof
  node "$sp_module" validate-publication "$publication" "$release" >/dev/null || die "publication receipt is invalid"
  source=$(sp_environment "$(sp_field "$receipt" sourceRelease)")
  target=$(sp_environment "$release")
  [[ "$(sp_sha "$source")" == "$(sp_field "$receipt" sourceEnvironmentSha256)" &&
    "$(sp_sha "$target")" == "$(sp_field "$receipt" targetEnvironmentSha256)" &&
    "$(sp_sha "$publication")" == "$(sp_field "$receipt" publicationSha256)" &&
    "$(sp_sha "$directory/protected.json")" == "$(sp_field "$receipt" protectedSha256)" ]] ||
    die "staged application receipt differs"
  sp_image "$(env_value "$target" WEATHER_SERVER_IMAGE)" "$(sp_field "$publication" serverImageId)"
  sp_image "$(env_value "$target" WEATHER_WEB_IMAGE)" "$(sp_field "$publication" webImageId)"
  printf '%s\n' "$target"
}

# verify the exact application trio and ordinary weather read boundary
sp_health() {
  local environment=$1 release service container image expected_id
  release=$(env_value "$environment" WEATHER_RELEASE)
  # require each healthy running application to use its committed digest
  for service in api worker web; do
    container=$(WEATHER_ENV_FILE=$environment compose ps -q "$service")
    [[ "$container" =~ ^[a-f0-9]{64}$ ]] || die "application container is missing: $service"
    image=$(env_value "$environment" WEATHER_SERVER_IMAGE)
    [[ "$service" != web ]] || image=$(env_value "$environment" WEATHER_WEB_IMAGE)
    expected_id=$(docker image inspect --format '{{.Id}}' "$image")
    [[ "$(docker inspect --format '{{.Config.Image}}|{{.Image}}|{{.State.Running}}|{{.State.Health.Status}}' "$container")" == \
      "$image|$expected_id|true|healthy" ]] || die "application image or health differs: $service"
  done
  verify_dashboard_web_runtime "$environment" "$release" "$(env_value "$environment" WEATHER_WEB_IMAGE)"
}

# replace only the application trio while retaining live infrastructure containers
sp_switch() {
  local environment=$1
  WEATHER_ENV_FILE=$environment compose up -d --no-deps --no-build --pull never --wait api worker
  WEATHER_ENV_FILE=$environment compose up -d --no-deps --no-build --pull never --wait web
  sp_health "$environment"
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
  [[ "$source_release:$target_release" == "$stage_source:$stage_release" ||
    "$source_release:$target_release" == "$stage_release:$stage_source" ]] || die "journal application pair differs"
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
        publication.json|protected.json|environment.env|stage.json) sp_private "$file" ;;
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
sp_system_recover() {
  local current environment container service
  current=$(read_release_state "$state_dir/current-release")
  environment=$(sp_environment "$current")
  [[ "$(read_release_state "$state_dir/schema-release")" == "$sp_anchor" ]] || die "schema owner changed"
  # validate committed ancestry before starting a stopped service
  if [[ ! -e "$sp_journal" && ! -L "$sp_journal" ]]; then
    [[ "$(sp_source)" == "$environment" ]] || die "committed application ancestry differs"
  fi
  # start only retained infrastructure that was stopped by the inherited ExecStop
  sp_start_infrastructure "$environment"
  if [[ -e "$sp_journal" || -L "$sp_journal" ]]; then
    sp_reconcile
  else
    [[ "$(realpath -e "$state_dir/active.env")" == "$environment" ]] || die "active application environment differs"
    [[ "$(sp_source)" == "$environment" ]] || die "committed application ancestry differs"
    sp_database "$environment"
    sp_switch "$environment"
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
  (($# >= 1)) || { printf 'usage: schema-preserving-release.sh stage RELEASE PUBLICATION_JSON SHA256 | activate RELEASE | recover | system-recover | status\n' >&2; return 2; }
  local action=$1
  shift
  sp_load
  case "$action" in
    stage)
      (($# == 3)) || die "stage requires release, publication and checksum"
      sp_stage "$@"
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
